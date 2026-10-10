//! Deferred Linux file choosers backed by the installed GTK and KDE helpers.
//!
//! The helpers own their GUI loops. Folio starts them on worker threads, parks
//! one answer per picker, and wakes the event loop after the helper exits.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Duration;

use crate::admission::WorkerCtx;
use crate::{IMAGE_FILE_EXTENSIONS, NativeWindow, ShellPickKind};

const PICKER_THREAD: &str = "bt-linux-file-picker";
const CANCEL_POLL: Duration = Duration::from_millis(50);

type DialogWake = Box<dyn Fn() + Send + Sync + 'static>;
static DIALOG_WAKE: OnceLock<DialogWake> = OnceLock::new();

/// Install the event-loop wake used by every Linux picker.
///
/// Call once before constructing application windows. The callback only asks
/// the loop to run; picker results remain parked until `take_result` is read.
pub fn install_dialog_wake(wake: impl Fn() + Send + Sync + 'static) -> Result<(), String> {
    DIALOG_WAKE
        .set(Box::new(wake))
        .map_err(|_| "the Linux dialog wake has already been installed".to_owned())
}

fn wake_dialog_loop() {
    if let Some(wake) = DIALOG_WAKE.get() {
        wake();
    }
}

#[derive(Debug)]
enum Phase {
    Idle,
    Waiting {
        generation: u64,
        cancel: mpsc::Sender<()>,
    },
    Complete {
        generation: u64,
        result: Result<Option<PathBuf>, String>,
    },
}

#[derive(Debug)]
struct PickerState {
    generation: u64,
    phase: Phase,
}

/// One chooser slot. A second request is declined until the answer is taken.
#[derive(Debug)]
struct DeferredPicker {
    state: Arc<Mutex<PickerState>>,
    dropped: Arc<AtomicBool>,
}

impl DeferredPicker {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(PickerState {
                generation: 0,
                phase: Phase::Idle,
            })),
            dropped: Arc::new(AtomicBool::new(false)),
        }
    }

    fn request(&self, request: PickRequest) -> Result<bool, String> {
        if DIALOG_WAKE.get().is_none() {
            return Err("the Linux dialog event-loop wake is not installed".to_owned());
        }

        let (generation, cancel_tx, cancel_rx) = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !matches!(state.phase, Phase::Idle) {
                return Ok(false);
            }
            state.generation = state.generation.wrapping_add(1);
            let generation = state.generation;
            let (cancel_tx, cancel_rx) = mpsc::channel();
            state.phase = Phase::Waiting {
                generation,
                cancel: cancel_tx.clone(),
            };
            (generation, cancel_tx, cancel_rx)
        };

        let state = Arc::clone(&self.state);
        let dropped = Arc::clone(&self.dropped);
        let worker = crate::spawn_at_priority(
            PICKER_THREAD,
            crate::ThreadPriority::BelowNormal,
            move |worker| run_picker(worker, state, dropped, generation, cancel_rx, request),
        );
        match worker {
            Ok(worker) => crate::linux_process::register_helper(worker),
            Err(error) => {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if matches!(state.phase, Phase::Waiting { generation: live, .. } if live == generation)
                {
                    state.phase = Phase::Idle;
                }
                drop(cancel_tx);
                return Err(format!(
                    "could not start the Linux file chooser worker: {error}"
                ));
            }
        }
        Ok(true)
    }

    fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let phase = std::mem::replace(&mut state.phase, Phase::Idle);
        match phase {
            Phase::Complete { generation, result } if generation == state.generation => {
                Some(result)
            }
            other => {
                state.phase = other;
                None
            }
        }
    }
}

impl Drop for DeferredPicker {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Phase::Waiting { cancel, .. } = &mut state.phase {
            let _ = cancel.send(());
        }
    }
}

#[derive(Debug)]
enum PickRequest {
    Folder(Option<PathBuf>),
    File {
        kind: ShellPickKind,
        start: Option<PathBuf>,
    },
    Save {
        start: Option<PathBuf>,
        name: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Helper {
    Zenity,
    KDialog,
}

impl Helper {
    const ALL: [Self; 2] = [Self::Zenity, Self::KDialog];

    const fn executable(self) -> &'static str {
        match self {
            Self::Zenity => "zenity",
            Self::KDialog => "kdialog",
        }
    }

    fn command(self, request: &PickRequest) -> Command {
        let mut command = crate::quiet_command(self.executable());
        command.arg("--title").arg("Folio");
        if self == Self::Zenity {
            command.arg("--modal");
        }
        match (self, request) {
            (Self::Zenity, PickRequest::Folder(start)) => {
                command.arg("--file-selection").arg("--directory");
                add_zenity_start(&mut command, start.as_deref());
            }
            (
                Self::Zenity,
                PickRequest::File {
                    kind: ShellPickKind::Folder,
                    start,
                },
            ) => {
                command.arg("--file-selection").arg("--directory");
                add_zenity_start(&mut command, start.as_deref());
            }
            (Self::Zenity, PickRequest::File { kind, start }) => {
                command.arg("--file-selection");
                add_zenity_start(&mut command, start.as_deref());
                if let Some(filter) = zenity_filter(*kind) {
                    command.arg(format!("--file-filter={filter}"));
                }
            }
            (Self::Zenity, PickRequest::Save { start, name }) => {
                command.arg("--file-selection").arg("--save");
                command.arg(path_option(
                    "--filename=",
                    &save_target(start.as_deref(), name),
                ));
                if is_settings_name(name) {
                    command.arg("--file-filter=Folio settings | *.json");
                }
            }
            (Self::KDialog, PickRequest::Folder(start)) => {
                command.arg("--getexistingdirectory");
                if let Some(start) = start {
                    command.arg(start);
                }
            }
            (
                Self::KDialog,
                PickRequest::File {
                    kind: ShellPickKind::Folder,
                    start,
                },
            ) => {
                command.arg("--getexistingdirectory");
                if let Some(start) = start {
                    command.arg(start);
                }
            }
            (Self::KDialog, PickRequest::File { kind, start }) => {
                command.arg("--getopenfilename");
                if let Some(filter) = kdialog_filter(*kind) {
                    command.arg(start.as_deref().unwrap_or_else(|| Path::new(".")));
                    command.arg(filter);
                } else if let Some(start) = start {
                    command.arg(start);
                }
            }
            (Self::KDialog, PickRequest::Save { start, name }) => {
                command.arg("--getsavefilename");
                command.arg(save_target(start.as_deref(), name));
                if is_settings_name(name) {
                    command.arg("Folio settings (*.json)");
                }
            }
        }
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        command
    }
}

fn add_zenity_start(command: &mut Command, start: Option<&Path>) {
    if let Some(start) = start {
        let mut bytes = start.as_os_str().as_bytes().to_vec();
        if !bytes.ends_with(b"/") {
            bytes.push(b'/');
        }
        command.arg(path_option(
            "--filename=",
            Path::new(OsStr::from_bytes(&bytes)),
        ));
    }
}

fn path_option(prefix: &str, path: &Path) -> OsString {
    let mut option = OsString::from(prefix);
    option.push(path.as_os_str());
    option
}

fn save_target(start: Option<&Path>, name: &str) -> PathBuf {
    match start {
        Some(start) => start.join(name),
        None => PathBuf::from(name),
    }
}

fn is_settings_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("json"))
}

fn zenity_filter(kind: ShellPickKind) -> Option<String> {
    match kind {
        ShellPickKind::Folder | ShellPickKind::Program => None,
        ShellPickKind::Image => Some(format!(
            "Folio images | {}",
            IMAGE_FILE_EXTENSIONS
                .iter()
                .map(|extension| format!("*.{extension}"))
                .collect::<Vec<_>>()
                .join(" ")
        )),
        ShellPickKind::SettingsFile => Some("Folio settings | *.json".to_owned()),
    }
}

fn kdialog_filter(kind: ShellPickKind) -> Option<String> {
    match kind {
        ShellPickKind::Folder | ShellPickKind::Program => None,
        ShellPickKind::Image => Some(format!(
            "Folio images ({})",
            IMAGE_FILE_EXTENSIONS
                .iter()
                .map(|extension| format!("*.{extension}"))
                .collect::<Vec<_>>()
                .join(" ")
        )),
        ShellPickKind::SettingsFile => Some("Folio settings (*.json)".to_owned()),
    }
}

fn run_picker(
    worker: &WorkerCtx,
    state: Arc<Mutex<PickerState>>,
    dropped: Arc<AtomicBool>,
    generation: u64,
    cancel: mpsc::Receiver<()>,
    request: PickRequest,
) {
    let result = run_helper(worker, &request, &cancel);
    if dropped.load(Ordering::Acquire) || cancel.try_recv().is_ok() {
        return;
    }
    let mut state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if matches!(state.phase, Phase::Waiting { generation: live, .. } if live == generation) {
        state.phase = Phase::Complete { generation, result };
        drop(state);
        wake_dialog_loop();
    }
}

fn run_helper(
    worker: &WorkerCtx,
    request: &PickRequest,
    cancel: &mpsc::Receiver<()>,
) -> Result<Option<PathBuf>, String> {
    for helper in Helper::ALL {
        if cancellation_requested(cancel) {
            return Ok(None);
        }
        let mut child = match helper.command(request).spawn() {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                continue;
            }
            Err(error) => {
                return Err(format!("could not start {}: {error}", helper.executable()));
            }
        };
        return finish_helper(worker, helper, &mut child, cancel);
    }
    Err(format!(
        "no Linux file chooser is installed (tried {} and {})",
        Helper::ALL[0].executable(),
        Helper::ALL[1].executable()
    ))
}

fn cancellation_requested(cancel: &mpsc::Receiver<()>) -> bool {
    matches!(
        cancel.try_recv(),
        Ok(()) | Err(mpsc::TryRecvError::Disconnected)
    )
}

fn finish_helper(
    worker: &WorkerCtx,
    helper: Helper,
    child: &mut std::process::Child,
    cancel: &mpsc::Receiver<()>,
) -> Result<Option<PathBuf>, String> {
    let readers = match crate::linux_process::OutputReaders::start(worker, child) {
        Ok(readers) => readers,
        Err(error) => {
            return Err(format!("could not read chooser output: {error}"));
        }
    };
    let status = loop {
        match cancel.recv_timeout(CANCEL_POLL) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                crate::linux_process::kill_reap_and_join(worker, child, readers);
                return Ok(None);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                crate::linux_process::kill_reap_and_join(worker, child, readers);
                return Err(format!(
                    "could not read {} status: {error}",
                    helper.executable()
                ));
            }
        }
    };
    let (stdout, stderr) = readers.finish(worker)?;
    read_helper_result(helper, status, stdout, stderr)
}

fn read_helper_result(
    helper: Helper,
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
) -> Result<Option<PathBuf>, String> {
    if status.code() == Some(1) {
        return Ok(None);
    }
    if !status.success() {
        let detail = String::from_utf8_lossy(&stderr).trim().to_owned();
        let suffix = if detail.is_empty() {
            String::new()
        } else {
            format!(": {detail}")
        };
        return Err(format!(
            "{} exited with {}{suffix}",
            helper.executable(),
            status
        ));
    }
    parse_selection(stdout).map(Some)
}

fn parse_selection(mut bytes: Vec<u8>) -> Result<PathBuf, String> {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.is_empty() {
        return Err("the Linux file chooser returned an empty path".to_owned());
    }
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

/// The folder chooser, on Linux.
pub struct FolderPicker {
    _window: NativeWindow,
    picker: DeferredPicker,
}

impl FolderPicker {
    /// Construct the deferred chooser without contacting the desktop.
    pub fn new(window: NativeWindow) -> Result<Self, String> {
        Ok(Self {
            _window: window,
            picker: DeferredPicker::new(),
        })
    }

    /// Ask for a folder. The dialog runs outside the window thread.
    pub fn request(&self, start: Option<&Path>) -> Result<bool, String> {
        self.picker
            .request(PickRequest::Folder(start.map(Path::to_path_buf)))
    }

    /// Take the completed choice once; `None` means no result is ready.
    pub fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
        self.picker.take_result()
    }
}

/// The picture and program chooser, on Linux.
pub struct ImagePicker {
    _window: NativeWindow,
    picker: DeferredPicker,
}

impl ImagePicker {
    /// Construct the deferred chooser without contacting the desktop.
    pub fn new(window: NativeWindow) -> Result<Self, String> {
        Ok(Self {
            _window: window,
            picker: DeferredPicker::new(),
        })
    }

    /// Ask for one file of the requested kind.
    pub fn request(&self, kind: ShellPickKind, start: Option<&Path>) -> Result<bool, String> {
        self.picker.request(PickRequest::File {
            kind,
            start: start.map(Path::to_path_buf),
        })
    }

    /// Take the completed choice once; `None` means no result is ready.
    pub fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
        self.picker.take_result()
    }
}

/// The save dialog, on Linux.
pub struct SaveFilePicker {
    _window: NativeWindow,
    picker: DeferredPicker,
}

impl SaveFilePicker {
    /// Construct the deferred dialog without contacting the desktop.
    pub fn new(window: NativeWindow) -> Result<Self, String> {
        Ok(Self {
            _window: window,
            picker: DeferredPicker::new(),
        })
    }

    /// Ask for a destination using `name` as the suggested filename.
    pub fn request(&self, start: Option<&Path>, name: &str) -> Result<bool, String> {
        self.picker.request(PickRequest::Save {
            start: start.map(Path::to_path_buf),
            name: name.to_owned(),
        })
    }

    /// Take the completed choice once; `None` means no result is ready.
    pub fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
        self.picker.take_result()
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::process::Stdio;
    use std::sync::mpsc;

    use super::{
        DeferredPicker, Helper, Phase, PickRequest, finish_helper, kdialog_filter, parse_selection,
        save_target, zenity_filter,
    };
    use crate::ShellPickKind;
    use crate::admission::WorkerCtx;

    fn on_worker<T: Send + 'static>(work: impl FnOnce(&WorkerCtx) -> T + Send + 'static) -> T {
        let (answer, wait) = mpsc::channel();
        crate::spawn_at_priority(
            "bt-linux-dialog-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| {
                let _ = answer.send(work(worker));
            },
        )
        .expect("start a controlled worker");
        wait.recv()
            .expect("the controlled worker returns its answer")
    }

    #[test]
    fn image_filter_uses_the_decoder_extension_inventory() {
        let zenity = zenity_filter(ShellPickKind::Image).expect("image filter");
        let kdialog = kdialog_filter(ShellPickKind::Image).expect("image filter");
        for extension in crate::IMAGE_FILE_EXTENSIONS {
            assert!(zenity.contains(&format!("*.{extension}")));
            assert!(kdialog.contains(&format!("*.{extension}")));
        }
    }

    #[test]
    fn program_selection_has_no_filter_and_settings_selection_is_json_only() {
        assert_eq!(zenity_filter(ShellPickKind::Program), None);
        assert_eq!(kdialog_filter(ShellPickKind::Program), None);
        assert_eq!(
            zenity_filter(ShellPickKind::SettingsFile).as_deref(),
            Some("Folio settings | *.json")
        );
        assert_eq!(
            kdialog_filter(ShellPickKind::SettingsFile).as_deref(),
            Some("Folio settings (*.json)")
        );
    }

    #[test]
    fn save_target_keeps_the_suggested_name_inside_the_requested_folder() {
        assert_eq!(
            save_target(
                Some(std::path::Path::new("/tmp/a folder")),
                "folio export.json"
            ),
            std::path::PathBuf::from("/tmp/a folder/folio export.json")
        );
    }

    #[test]
    fn helper_arguments_are_values_not_shell_fragments() {
        let request = PickRequest::Save {
            start: Some(std::path::PathBuf::from("/tmp/a folder; echo no")),
            name: "report & notes.json".to_owned(),
        };
        let command = Helper::Zenity.command(&request);
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.contains(&"--filename=/tmp/a folder; echo no/report & notes.json".to_owned()));
        assert!(args.contains(&"--file-filter=Folio settings | *.json".to_owned()));
    }

    #[test]
    fn kdialog_keeps_the_optional_start_directory_before_its_filter() {
        let command = Helper::KDialog.command(&PickRequest::File {
            kind: ShellPickKind::SettingsFile,
            start: None,
        });
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let open = args
            .iter()
            .position(|arg| arg == "--getopenfilename")
            .expect("open chooser");
        assert_eq!(args[open + 1], ".");
        assert_eq!(args[open + 2], "Folio settings (*.json)");
    }

    #[test]
    fn selection_parser_removes_one_record_terminator_and_keeps_other_bytes() {
        let path = parse_selection(b"/tmp/name\nwith-newline\n".to_vec()).expect("path");
        assert_eq!(path.as_os_str().as_bytes(), b"/tmp/name\nwith-newline");
    }

    #[test]
    fn selection_parser_preserves_non_utf8_path_bytes() {
        let path = parse_selection(vec![b'/', b't', b'm', b'p', b'/', 0xff, b'\n']).expect("path");
        assert_eq!(path.as_os_str().as_bytes(), b"/tmp/\xff");
    }

    #[test]
    fn picker_arguments_preserve_non_utf8_start_path_bytes() {
        let start = std::path::PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/f\xff".to_vec()));
        let zenity = Helper::Zenity.command(&PickRequest::Folder(Some(start.clone())));
        let zenity_args = zenity.get_args().collect::<Vec<_>>();
        assert!(
            zenity_args
                .iter()
                .any(|argument| { argument.as_bytes() == b"--filename=/tmp/f\xff/" })
        );

        let kdialog = Helper::KDialog.command(&PickRequest::Folder(Some(start)));
        let kdialog_args = kdialog.get_args().collect::<Vec<_>>();
        let folder_at = kdialog_args
            .iter()
            .position(|argument| *argument == "--getexistingdirectory")
            .expect("the folder operation is selected");
        assert_eq!(kdialog_args[folder_at + 1].as_bytes(), b"/tmp/f\xff");
    }

    #[test]
    fn helper_drains_exactly_128_kibibytes_of_stderr_before_returning_selection() {
        let script = "i=0; while [ \"$i\" -lt 32 ]; do printf '%4096s' '' >&2; i=$((i + 1)); done; printf '/tmp/selected\\n'";
        on_worker(move |worker| {
            let hygiene = bt_pty::test_shell::Hygiene::new();
            let mut child = hygiene
                .command("/bin/sh", crate::quiet_command)
                .args(["-c", script])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("start controlled chooser");

            let (_cancel_tx, cancel_rx) = mpsc::channel();
            let selected = finish_helper(worker, Helper::Zenity, &mut child, &cancel_rx)
                .expect("drain the helper output")
                .expect("helper selection");
            assert_eq!(selected, std::path::PathBuf::from("/tmp/selected"));
        });
    }

    #[test]
    fn helper_drains_beyond_the_retained_output_limit_and_reports_overflow() {
        let script = "i=0; while [ \"$i\" -lt 33 ]; do printf '%4096s' '' >&2; i=$((i + 1)); done; printf '/tmp/selected\\n'";
        on_worker(move |worker| {
            let hygiene = bt_pty::test_shell::Hygiene::new();
            let mut child = hygiene
                .command("/bin/sh", crate::quiet_command)
                .args(["-c", script])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("start controlled chooser");

            let (_cancel_tx, cancel_rx) = mpsc::channel();
            let error = finish_helper(worker, Helper::Zenity, &mut child, &cancel_rx)
                .expect_err("output beyond the retained limit is rejected");
            assert!(error.contains("exceeds 128 KiB"));
        });
    }

    #[test]
    fn cancellation_kills_and_reaps_a_running_helper() {
        on_worker(|worker| {
            let mut child = crate::quiet_command("/bin/sleep")
                .arg("30")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("start controlled chooser");
            let pid = child.id();
            let (cancel_tx, cancel_rx) = mpsc::channel();
            cancel_tx.send(()).expect("queue cancellation");

            assert_eq!(
                finish_helper(worker, Helper::Zenity, &mut child, &cancel_rx).unwrap(),
                None
            );
            assert!(
                !std::path::Path::new(&format!("/proc/{pid}")).exists(),
                "the canceled chooser process must be reaped"
            );
        });
    }

    #[test]
    fn completed_picker_result_is_consumed_once() {
        let picker = DeferredPicker::new();
        let selected = std::path::PathBuf::from("/tmp/selected");
        {
            let mut state = picker
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.generation = 9;
            state.phase = Phase::Complete {
                generation: 9,
                result: Ok(Some(selected.clone())),
            };
        }

        assert_eq!(picker.take_result(), Some(Ok(Some(selected))));
        assert_eq!(picker.take_result(), None);
    }
}
