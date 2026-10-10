//! Concurrent, bounded reads of desktop helper output.

use std::io::{self, Read};
use std::process::Child;
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;

use crate::admission::WorkerCtx;

pub(crate) const OUTPUT_LIMIT_BYTES: usize = 128 * 1024;

type Reader = JoinHandle<io::Result<Vec<u8>>>;
type Pipe = Box<dyn Read + Send + 'static>;
type HelperWorker = JoinHandle<()>;

static HELPER_WORKERS: OnceLock<Mutex<Vec<HelperWorker>>> = OnceLock::new();

fn helper_workers() -> &'static Mutex<Vec<HelperWorker>> {
    HELPER_WORKERS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Retain a Linux desktop helper until it finishes or shutdown drains it.
///
/// Registration is used by the window thread, so finished handles are
/// discarded without joining. Shutdown joins the remaining handles through
/// [`shutdown_helpers`].
pub fn register_helper_worker(worker: JoinHandle<()>) {
    let mut active = helper_workers()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    active.retain(|helper| !helper.is_finished());
    active.push(worker);
}

pub(crate) fn register_helper(worker: JoinHandle<()>) {
    register_helper_worker(worker);
}

/// Join every helper worker after the dialog and notification owners have been
/// dropped and have sent their cancellation requests.
///
/// The caller must be a shutdown worker: a helper can spend one cancellation
/// poll interval killing and reaping its child before it returns.
pub fn shutdown_helpers(_worker: &WorkerCtx) -> Result<(), String> {
    crate::webview::shutdown_linux_actor();
    let workers = {
        let mut active = helper_workers()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut *active)
    };

    let mut panicked = 0;
    for worker in workers {
        let worker: std::thread::JoinHandle<()> = worker;
        if worker.join().is_err() {
            panicked += 1;
        }
    }
    if panicked == 0 {
        Ok(())
    } else {
        Err(format!(
            "{panicked} Linux desktop helper worker(s) panicked"
        ))
    }
}

pub(crate) struct OutputReaders {
    stdout: Option<Reader>,
    stderr: Option<Reader>,
}

impl OutputReaders {
    pub(crate) fn start(worker: &WorkerCtx, child: &mut Child) -> io::Result<Self> {
        Self::start_with_limits(worker, child, OUTPUT_LIMIT_BYTES, OUTPUT_LIMIT_BYTES)
    }

    pub(crate) fn start_with_limits(
        worker: &WorkerCtx,
        child: &mut Child,
        stdout_limit: usize,
        stderr_limit: usize,
    ) -> io::Result<Self> {
        Self::start_with(worker, child, stdout_limit, stderr_limit, spawn_reader)
    }

    fn start_with(
        worker: &WorkerCtx,
        child: &mut Child,
        stdout_limit: usize,
        stderr_limit: usize,
        mut start_reader: impl FnMut(&WorkerCtx, &'static str, Pipe, usize) -> io::Result<Reader>,
    ) -> io::Result<Self> {
        let stdout = match child.stdout.take() {
            Some(pipe) => match start_reader(
                worker,
                "bt-helper-stdout",
                Box::new(pipe) as Pipe,
                stdout_limit,
            ) {
                Ok(reader) => Some(reader),
                Err(error) => {
                    kill_reap_and_join(
                        worker,
                        child,
                        Self {
                            stdout: None,
                            stderr: None,
                        },
                    );
                    return Err(error);
                }
            },
            None => None,
        };

        let stderr = match child.stderr.take() {
            Some(pipe) => match start_reader(
                worker,
                "bt-helper-stderr",
                Box::new(pipe) as Pipe,
                stderr_limit,
            ) {
                Ok(reader) => Some(reader),
                Err(error) => {
                    kill_reap_and_join(
                        worker,
                        child,
                        Self {
                            stdout,
                            stderr: None,
                        },
                    );
                    return Err(error);
                }
            },
            None => None,
        };

        Ok(Self { stdout, stderr })
    }

    pub(crate) fn finish(self, worker: &WorkerCtx) -> Result<(Vec<u8>, Vec<u8>), String> {
        let stdout = join_reader(worker, self.stdout);
        let stderr = join_reader(worker, self.stderr);
        match (stdout, stderr) {
            (Ok(stdout), Ok(stderr)) => Ok((stdout, stderr)),
            (Err(error), Ok(_)) | (Ok(_), Err(error)) => Err(error),
            (Err(stdout), Err(stderr)) => Err(format!("{stdout}; {stderr}")),
        }
    }
}

fn spawn_reader(
    _worker: &WorkerCtx,
    name: &'static str,
    pipe: Pipe,
    limit: usize,
) -> io::Result<Reader> {
    crate::spawn_at_priority(name, crate::ThreadPriority::BelowNormal, move |worker| {
        collect(worker, pipe, limit)
    })
}

fn join_reader(_worker: &WorkerCtx, reader: Option<Reader>) -> Result<Vec<u8>, String> {
    match reader {
        None => Ok(Vec::new()),
        Some(reader) => reader
            .join()
            .map_err(|_| "desktop helper output reader failed".to_owned())?
            .map_err(|error| format!("could not read desktop helper output: {error}")),
    }
}

pub(crate) fn kill_reap_and_join(worker: &WorkerCtx, child: &mut Child, readers: OutputReaders) {
    let _ = child.kill();
    let _ = child.wait();
    let _ = readers.finish(worker);
}

fn collect(_worker: &WorkerCtx, mut reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut overflowed = false;
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        let remaining = limit.saturating_sub(bytes.len());
        bytes.extend_from_slice(&buffer[..count.min(remaining)]);
        overflowed |= count > remaining;
    }
    if overflowed {
        let limit = if limit == OUTPUT_LIMIT_BYTES {
            "128 KiB".to_owned()
        } else {
            format!("{limit} bytes")
        };
        Err(io::Error::other(format!(
            "desktop helper output exceeds {limit}"
        )))
    } else {
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::{OUTPUT_LIMIT_BYTES, OutputReaders, collect};
    use crate::admission::WorkerCtx;
    use bt_pty::test_shell::Hygiene;
    use std::io;
    use std::process::Stdio;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, mpsc};

    fn on_worker<T: Send + 'static>(work: impl FnOnce(&WorkerCtx) -> T + Send + 'static) -> T {
        let (answer, wait) = mpsc::channel();
        crate::spawn_at_priority(
            "bt-linux-process-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| {
                let _ = answer.send(work(worker));
            },
        )
        .expect("start a controlled worker");
        wait.recv()
            .expect("the controlled worker returns its answer")
    }

    fn shell(hygiene: &Hygiene, script: &str) -> std::process::Command {
        let mut command = hygiene.command("/bin/sh", crate::quiet_command);
        command
            .args(["-c", script])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    #[test]
    fn preserves_small_stdout_and_stderr_exactly() {
        on_worker(|worker| {
            let hygiene = Hygiene::new();
            let mut child = shell(&hygiene, "printf out; printf err >&2")
                .spawn()
                .expect("start controlled helper");
            let readers = OutputReaders::start(worker, &mut child).expect("start output readers");
            assert!(child.wait().expect("wait for helper").success());
            assert_eq!(
                readers.finish(worker).unwrap(),
                (b"out".to_vec(), b"err".to_vec())
            );
        });
    }

    #[test]
    fn retains_at_most_128_kibibytes_per_stream() {
        let script = "i=0; while [ \"$i\" -lt 32 ]; do printf '%4096s' ''; printf '%4096s' '' >&2; i=$((i + 1)); done";
        on_worker(move |worker| {
            let hygiene = Hygiene::new();
            let mut child = shell(&hygiene, script)
                .spawn()
                .expect("start controlled helper");
            let readers = OutputReaders::start(worker, &mut child).expect("start output readers");
            assert!(child.wait().expect("wait for helper").success());
            let (stdout, stderr) = readers.finish(worker).expect("retain the limit exactly");
            assert_eq!(stdout.len(), OUTPUT_LIMIT_BYTES);
            assert_eq!(stderr.len(), OUTPUT_LIMIT_BYTES);
        });
    }

    #[test]
    fn start_with_limits_applies_each_stream_limit_to_its_own_pipe() {
        let script = "printf '%4096s' ''; i=0; while [ \"$i\" -lt 2 ]; do printf '%4096s' '' >&2; i=$((i + 1)); done";
        on_worker(move |worker| {
            let hygiene = Hygiene::new();
            let mut child = shell(&hygiene, script)
                .spawn()
                .expect("start controlled helper");
            let readers = OutputReaders::start_with_limits(worker, &mut child, 4096, 8192)
                .expect("start output readers with distinct limits");
            assert!(child.wait().expect("wait for helper").success());
            let (stdout, stderr) = readers.finish(worker).expect("both limits are exact");
            assert_eq!(stdout.len(), 4096);
            assert_eq!(stderr.len(), 8192);
        });
    }

    #[test]
    fn drains_both_pipes_after_the_retained_limit_is_reached() {
        let script = "i=0; while [ \"$i\" -lt 33 ]; do printf '%4096s' ''; printf '%4096s' '' >&2; i=$((i + 1)); done";
        on_worker(move |worker| {
            let hygiene = Hygiene::new();
            let mut child = shell(&hygiene, script)
                .spawn()
                .expect("start controlled helper");
            let readers = OutputReaders::start(worker, &mut child).expect("start output readers");
            assert!(child.wait().expect("wait for helper").success());
            let error = readers.finish(worker).expect_err("reject oversized output");
            assert!(error.contains("exceeds 128 KiB"));
        });
    }

    #[test]
    fn second_reader_start_failure_kills_reaps_and_joins_the_first_reader() {
        on_worker(move |worker| {
            let hygiene = Hygiene::new();
            let mut child = shell(&hygiene, "exec /bin/sleep 30")
                .spawn()
                .expect("start controlled helper");
            let pid = child.id();
            let starts = AtomicUsize::new(0);
            let first_reader_finished = Arc::new(AtomicBool::new(false));
            let first_reader_flag = Arc::clone(&first_reader_finished);

            let result = OutputReaders::start_with(
                worker,
                &mut child,
                OUTPUT_LIMIT_BYTES,
                OUTPUT_LIMIT_BYTES,
                move |_worker, name, pipe, limit| {
                    if starts.fetch_add(1, Ordering::SeqCst) == 1 {
                        return Err(io::Error::other("controlled second-reader failure"));
                    }
                    let first_reader_flag = Arc::clone(&first_reader_flag);
                    crate::spawn_at_priority(
                        name,
                        crate::ThreadPriority::BelowNormal,
                        move |reader_worker| {
                            let result = collect(reader_worker, pipe, limit);
                            first_reader_flag.store(true, Ordering::Release);
                            result
                        },
                    )
                },
            );

            assert!(result.is_err());
            assert!(
                child
                    .try_wait()
                    .expect("read reaped child status")
                    .is_some(),
                "the helper must be waited after reader startup fails"
            );
            assert!(
                !std::path::Path::new(&format!("/proc/{pid}")).exists(),
                "the failed-start helper process must be reaped"
            );
            assert!(first_reader_finished.load(Ordering::Acquire));
        });
    }
}
