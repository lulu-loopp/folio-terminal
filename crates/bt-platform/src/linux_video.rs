//! Bounded Linux video stills, through the installed ffprobe and ffmpeg tools.
//!
//! This answers the existing first-frame contract only. It does not play video,
//! report that an embedded player exists, or turn a failed probe into a picture.
//! A dedicated worker runs both commands through the quiet-command door. Shared
//! output readers drain capped pipes; the worker kills and reaps each child when
//! the shared deadline or caller cancellation arrives.

use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use crate::admission::WorkerCtx;
use crate::video::{FIRST_FRAME_BUDGET, FirstFrameCost, SEEK_FRACTION, VideoFrame};

const MAX_SOURCE_PIXELS: u64 = 16_777_216;
const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
const MAX_PROBE_BYTES: usize = 16 * 1024;
const MAX_FFMPEG_ALLOCATION: usize = 64 * 1024 * 1024;
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(8);
const FRAME_WORKER: &str = "folio-video-frame";

struct Probe {
    width: u32,
    height: u32,
    duration_ms: Option<u64>,
    seek_seconds: f64,
}

struct CapturedCommand {
    stdout: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrameFailure {
    Cancelled,
    TimedOut,
    ProbeStart,
    ProbeOutput,
    ProbeFailed,
    ProbeMalformed,
    SourceTooLarge,
    FrameTooLarge,
    DecoderStart,
    DecoderOutput,
    DecoderFailed,
    FrameTruncated,
    FrameOversized,
    PipeStart,
    WaitFailed,
}

#[derive(Clone, Copy)]
enum ChildKind {
    Probe,
    Decoder,
}

impl ChildKind {
    fn start_failure(self) -> FrameFailure {
        match self {
            Self::Probe => FrameFailure::ProbeStart,
            Self::Decoder => FrameFailure::DecoderStart,
        }
    }

    fn output_failure(self) -> FrameFailure {
        match self {
            Self::Probe => FrameFailure::ProbeOutput,
            Self::Decoder => FrameFailure::DecoderOutput,
        }
    }

    fn exit_failure(self) -> FrameFailure {
        match self {
            Self::Probe => FrameFailure::ProbeFailed,
            Self::Decoder => FrameFailure::DecoderFailed,
        }
    }
}

/// Ask ffprobe and ffmpeg for one local-file poster frame from a worker.
///
/// The caller stops waiting for a frame at the shared budget and signals its
/// dedicated normal-priority decoder worker to cancel. It waits only for child
/// reaping and pipe readers to finish. Each child has the same absolute
/// deadline, and any start, probe, or frame error answers `None`.
#[must_use]
pub fn first_frame_on_worker(
    _worker: &WorkerCtx,
    path: &Path,
    fit_width: u32,
    fit_height: u32,
) -> Option<VideoFrame> {
    let deadline = Instant::now() + FIRST_FRAME_BUDGET;
    let path = path.to_path_buf();
    let (answer, wait) = mpsc::channel();
    let (cancel, cancelled) = mpsc::channel();
    crate::spawn_at_priority(FRAME_WORKER, crate::ThreadPriority::Normal, move |worker| {
        let result = decode_first_frame_measured(
            worker,
            &path,
            fit_width,
            fit_height,
            deadline,
            Some(&cancelled),
        );
        let _ = answer.send(result);
    })
    .ok()?;
    match wait.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok((result, _)) => result.ok(),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let _ = cancel.send(());
            let _ = wait.recv();
            None
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => None,
    }
}

fn decode_first_frame_measured(
    worker: &WorkerCtx,
    path: &Path,
    fit_width: u32,
    fit_height: u32,
    deadline: Instant,
    cancel: Option<&Receiver<()>>,
) -> (Result<VideoFrame, FrameFailure>, FirstFrameCost) {
    let mut cost = FirstFrameCost::default();

    let open_started = Instant::now();
    let probe_command = probe_command(worker, path);
    let probe_output = match run_bounded(
        worker,
        probe_command,
        MAX_PROBE_BYTES,
        deadline,
        cancel,
        ChildKind::Probe,
    ) {
        Ok(output) => output,
        Err(failure) => {
            cost.open = open_started.elapsed();
            return (Err(failure), cost);
        }
    };
    cost.open = open_started.elapsed();
    if probe_output.stdout.len() > MAX_PROBE_BYTES {
        return (Err(FrameFailure::ProbeOutput), cost);
    }

    let output_type_started = Instant::now();
    let Some(probe) = parse_probe(&probe_output.stdout) else {
        cost.output_type = output_type_started.elapsed();
        return (Err(FrameFailure::ProbeMalformed), cost);
    };
    let source_pixels = u64::from(probe.width) * u64::from(probe.height);
    if source_pixels == 0 || source_pixels > MAX_SOURCE_PIXELS {
        cost.output_type = output_type_started.elapsed();
        return (Err(FrameFailure::SourceTooLarge), cost);
    }
    let Some((width, height)) = fit_dimensions(
        (probe.width, probe.height),
        (fit_width.max(1), fit_height.max(1)),
    ) else {
        cost.output_type = output_type_started.elapsed();
        return (Err(FrameFailure::FrameTooLarge), cost);
    };
    let expected_bytes = usize::try_from(u64::from(width) * u64::from(height) * 4).ok();
    let Some(expected_bytes) = expected_bytes.filter(|bytes| *bytes <= MAX_FRAME_BYTES) else {
        cost.output_type = output_type_started.elapsed();
        return (Err(FrameFailure::FrameTooLarge), cost);
    };
    cost.output_type = output_type_started.elapsed();

    // ffmpeg performs input seek and one-frame decode in one child process, so
    // both costs belong to this span rather than an invented seek measurement.
    let read_sample_started = Instant::now();
    let ffmpeg_command = frame_command(worker, path, width, height, probe.seek_seconds);
    let frame_output = match run_bounded(
        worker,
        ffmpeg_command,
        expected_bytes,
        deadline,
        cancel,
        ChildKind::Decoder,
    ) {
        Ok(output) => output,
        Err(failure) => {
            cost.read_sample = read_sample_started.elapsed();
            return (Err(failure), cost);
        }
    };
    cost.read_sample = read_sample_started.elapsed();
    if let Err(failure) = validate_frame_bytes(&frame_output.stdout, expected_bytes) {
        return (Err(failure), cost);
    }

    let copy_started = Instant::now();
    // The filter discards source alpha before ffmpeg writes RGBA, so every
    // pixel follows the platform contract of an opaque straight-alpha frame.
    let frame = VideoFrame {
        rgba: frame_output.stdout,
        width,
        height,
        duration_ms: probe.duration_ms,
        native_width: probe.width,
        native_height: probe.height,
    };
    cost.copy = copy_started.elapsed();
    (Ok(frame), cost)
}

fn probe_command(_worker: &WorkerCtx, path: &Path) -> Command {
    let mut command = crate::quiet_command("ffprobe");
    let max_alloc = MAX_FFMPEG_ALLOCATION.to_string();
    command
        .args([
            "-v",
            "error",
            "-max_alloc",
            max_alloc.as_str(),
            "-protocol_whitelist",
            "file",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height:format=duration",
            "-of",
            "default=noprint_wrappers=1",
            "-i",
        ])
        .arg(path.as_os_str())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn frame_command(
    _worker: &WorkerCtx,
    path: &Path,
    width: u32,
    height: u32,
    seek_seconds: f64,
) -> Command {
    let mut command = crate::quiet_command("ffmpeg");
    let seek = format!("{seek_seconds:.6}");
    let max_pixels = MAX_SOURCE_PIXELS.to_string();
    let max_alloc = MAX_FFMPEG_ALLOCATION.to_string();
    let filter = format!("scale={width}:{height}:flags=bicubic,format=rgb24");
    command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-threads",
            "1",
            "-filter_threads",
            "1",
            "-max_alloc",
            max_alloc.as_str(),
            "-max_pixels",
            max_pixels.as_str(),
            "-protocol_whitelist",
            "file",
            "-ss",
            seek.as_str(),
            "-i",
        ])
        .arg(path.as_os_str())
        .args([
            "-map",
            "0:v:0",
            "-frames:v",
            "1",
            "-vf",
            filter.as_str(),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn parse_probe(bytes: &[u8]) -> Option<Probe> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut width = None;
    let mut height = None;
    let mut duration_seconds = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "width" => width = value.trim().parse::<u32>().ok(),
            "height" => height = value.trim().parse::<u32>().ok(),
            "duration" => {
                duration_seconds = value
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
            }
            _ => {}
        }
    }
    let width = width.filter(|value| *value > 0)?;
    let height = height.filter(|value| *value > 0)?;
    let duration_ms = duration_seconds
        .map(|seconds| (seconds * 1_000.0).round().clamp(0.0, u64::MAX as f64) as u64);
    let seek_seconds = duration_seconds
        .filter(|seconds| *seconds > 0.0)
        .map_or(0.0, |seconds| seconds * SEEK_FRACTION);
    Some(Probe {
        width,
        height,
        duration_ms,
        seek_seconds,
    })
}

fn fit_dimensions(native: (u32, u32), fit: (u32, u32)) -> Option<(u32, u32)> {
    if native.0 == 0 || native.1 == 0 || fit.0 == 0 || fit.1 == 0 {
        return None;
    }
    let scale = (f64::from(fit.0) / f64::from(native.0))
        .min(f64::from(fit.1) / f64::from(native.1))
        .min(1.0);
    let width = (f64::from(native.0) * scale)
        .round()
        .clamp(1.0, f64::from(fit.0)) as u32;
    let height = (f64::from(native.1) * scale)
        .round()
        .clamp(1.0, f64::from(fit.1)) as u32;
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))?
        .checked_mul(4)?;
    (bytes <= MAX_FRAME_BYTES as u64).then_some((width, height))
}

fn validate_frame_bytes(bytes: &[u8], expected: usize) -> Result<(), FrameFailure> {
    match bytes.len().cmp(&expected) {
        std::cmp::Ordering::Less => Err(FrameFailure::FrameTruncated),
        std::cmp::Ordering::Greater => Err(FrameFailure::FrameOversized),
        std::cmp::Ordering::Equal => Ok(()),
    }
}

fn run_bounded(
    worker: &WorkerCtx,
    mut command: Command,
    stdout_limit: usize,
    deadline: Instant,
    cancel: Option<&Receiver<()>>,
    kind: ChildKind,
) -> Result<CapturedCommand, FrameFailure> {
    if cancellation_requested(cancel) {
        return Err(FrameFailure::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(FrameFailure::TimedOut);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|_| kind.start_failure())?;
    let readers = crate::linux_process::OutputReaders::start_with_limits(
        worker,
        &mut child,
        stdout_limit,
        crate::linux_process::OUTPUT_LIMIT_BYTES,
    )
    .map_err(|_| FrameFailure::PipeStart)?;
    let status = match wait_for_child(worker, &mut child, deadline, cancel) {
        Ok(status) => status,
        Err(failure) => {
            crate::linux_process::kill_reap_and_join(worker, &mut child, readers);
            return Err(failure);
        }
    };
    let (stdout, _stderr) = readers.finish(worker).map_err(|_| kind.output_failure())?;
    if stdout.len() > stdout_limit {
        return Err(kind.output_failure());
    }
    if !status.success() {
        return Err(kind.exit_failure());
    }
    Ok(CapturedCommand { stdout })
}

fn wait_for_child(
    _worker: &WorkerCtx,
    child: &mut std::process::Child,
    deadline: Instant,
    cancel: Option<&Receiver<()>>,
) -> Result<ExitStatus, FrameFailure> {
    loop {
        if cancellation_requested(cancel) {
            return Err(FrameFailure::Cancelled);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(FrameFailure::TimedOut);
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(_) => return Err(FrameFailure::WaitFailed),
        }
        let wait = CHILD_POLL_INTERVAL.min(remaining);
        if let Some(cancel) = cancel {
            match cancel.recv_timeout(wait) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(FrameFailure::Cancelled);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        } else {
            thread::sleep(wait);
        }
    }
}

fn cancellation_requested(cancel: Option<&Receiver<()>>) -> bool {
    cancel.is_some_and(|cancel| {
        matches!(
            cancel.try_recv(),
            Ok(()) | Err(mpsc::TryRecvError::Disconnected)
        )
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::Instant;

    use crate::ThreadPriority;
    use crate::admission::WorkerCtx;

    use super::{
        ChildKind, FrameFailure, fit_dimensions, parse_probe, run_bounded, validate_frame_bytes,
        wait_for_child,
    };

    fn on_worker<T: Send + 'static>(work: impl FnOnce(&WorkerCtx) -> T + Send + 'static) -> T {
        let (answer, wait) = mpsc::channel();
        crate::spawn_at_priority("linux-video-test", ThreadPriority::Normal, move |worker| {
            let _ = answer.send(work(worker));
        })
        .expect("start a controlled worker");
        wait.recv()
            .expect("the controlled worker returns its answer")
    }

    #[test]
    fn probe_reads_named_fields_when_ffprobe_reorders_them() {
        let probe = parse_probe(b"duration=5.000000\nheight=120\nwidth=160\n").unwrap();
        assert_eq!((probe.width, probe.height), (160, 120));
        assert_eq!(probe.duration_ms, Some(5_000));
        assert_eq!(probe.seek_seconds, 0.5);
    }

    #[test]
    fn unknown_duration_keeps_a_start_frame_and_no_duration_fact() {
        let probe = parse_probe(b"width=160\nheight=120\nduration=N/A\n").unwrap();
        assert_eq!(probe.duration_ms, None);
        assert_eq!(probe.seek_seconds, 0.0);
    }

    #[test]
    fn fit_keeps_source_dimensions_when_the_requested_box_is_larger() {
        assert_eq!(fit_dimensions((160, 120), (1920, 1080)), Some((160, 120)));
        assert_eq!(
            fit_dimensions((3840, 2160), (1920, 1080)),
            Some((1920, 1080))
        );
        assert_eq!(fit_dimensions((1920, 1080), (640, 480)), Some((640, 360)));
    }

    #[test]
    fn frame_length_errors_name_truncation_and_oversize() {
        assert_eq!(
            validate_frame_bytes(b"short", 8),
            Err(FrameFailure::FrameTruncated)
        );
        assert_eq!(
            validate_frame_bytes(b"too long", 3),
            Err(FrameFailure::FrameOversized)
        );
        assert_eq!(validate_frame_bytes(b"exact", 5), Ok(()));
    }

    #[test]
    fn oversized_child_output_has_a_named_failure() {
        on_worker(|worker| {
            let hygiene = bt_pty::test_shell::Hygiene::new();
            let mut command = hygiene.command("/bin/sh", crate::quiet_command);
            command.args(["-c", "printf 1234567890"]);
            let result = run_bounded(
                worker,
                command,
                4,
                Instant::now() + super::FIRST_FRAME_BUDGET,
                None,
                ChildKind::Decoder,
            );
            assert!(matches!(result, Err(FrameFailure::DecoderOutput)));
        });
    }

    #[test]
    fn a_failed_child_has_a_named_failure() {
        on_worker(|worker| {
            let hygiene = bt_pty::test_shell::Hygiene::new();
            let mut command = hygiene.command("/bin/sh", crate::quiet_command);
            command.args(["-c", "exit 1"]);
            let result = run_bounded(
                worker,
                command,
                4,
                Instant::now() + super::FIRST_FRAME_BUDGET,
                None,
                ChildKind::Decoder,
            );
            assert!(matches!(result, Err(FrameFailure::DecoderFailed)));
        });
    }

    #[test]
    fn cancelling_a_child_reaps_it_before_joining_its_pipe_readers() {
        on_worker(|worker| {
            let hygiene = bt_pty::test_shell::Hygiene::new();
            let mut command = hygiene.command("/bin/sh", crate::quiet_command);
            command
                .args(["-c", "exec /bin/sleep 30"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = command.spawn().expect("start a controlled helper");
            let readers = crate::linux_process::OutputReaders::start_with_limits(
                worker,
                &mut child,
                4,
                crate::linux_process::OUTPUT_LIMIT_BYTES,
            )
            .expect("start bounded pipe readers");
            let (cancel, cancelled) = mpsc::channel();
            cancel
                .send(())
                .expect("queue cancellation for the live helper");
            assert_eq!(
                wait_for_child(
                    worker,
                    &mut child,
                    Instant::now() + super::FIRST_FRAME_BUDGET,
                    Some(&cancelled),
                ),
                Err(FrameFailure::Cancelled)
            );

            crate::linux_process::kill_reap_and_join(worker, &mut child, readers);
            assert!(
                child.try_wait().expect("read the reaped status").is_some(),
                "cancellation waits for the helper process to exit"
            );
        });
    }

    #[test]
    fn installed_ffmpeg_decodes_a_real_fixture_to_one_opaque_rgba_frame() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/assets/folio-video-test.mp4");
        let frame =
            on_worker(move |worker| super::first_frame_on_worker(worker, &fixture, 1920, 1080))
                .expect("the checked-in fixture decodes with the installed ffmpeg tools");
        assert_eq!((frame.width, frame.height), (160, 120));
        assert_eq!((frame.native_width, frame.native_height), (160, 120));
        assert_eq!(frame.duration_ms, Some(5_000));
        assert_eq!(frame.rgba.len(), 160 * 120 * 4);
        assert!(frame.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));
        let colored_pixels = frame
            .rgba
            .chunks_exact(4)
            .filter(|pixel| [pixel[0], pixel[1], pixel[2]] != [0, 0, 0])
            .count();
        assert!(
            colored_pixels > frame.width as usize * frame.height as usize / 8,
            "the frame at one tenth is past the fixture's black opening"
        );
    }
}
