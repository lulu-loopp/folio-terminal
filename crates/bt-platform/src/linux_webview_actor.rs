//! The process-wide Linux Chromium actor. Its browser child and CDP sessions
//! live here; no browser object crosses back to the window thread.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{
    PageVisual, WebColorScheme, WebEvent, WebFrame, WebImeEvent, WebInstallReport, WebKeyEvent,
    WebMouseEvent, WebNavigationVerdict, WebRequestVerdict,
};
use crate::admission::WorkerCtx;
use crate::{ThreadPriority, WebGuards};

pub(crate) type HostId = u64;

fn trace_browser_startup(_worker: &WorkerCtx, event: &str) {
    static STARTED: OnceLock<Instant> = OnceLock::new();
    let Some(path) = std::env::var_os("BT_WEB_TRACE") else {
        return;
    };
    let elapsed_ms = STARTED.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0;
    if let Ok(mut trace) = OpenOptions::new().append(true).open(PathBuf::from(path)) {
        let _ = writeln!(
            trace,
            "linux_actor actor_elapsed_ms={elapsed_ms:.3} {event}"
        );
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) enum GateKey {
    Fetch {
        session: String,
        request: String,
    },
    NavigationApi {
        session: String,
        context: i32,
        request: String,
        token: String,
        url: String,
        cancelable: bool,
    },
}

pub(crate) enum GateReply {
    Navigation(WebNavigationVerdict),
    Resource(WebRequestVerdict),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ContextWorld {
    name: String,
    is_default: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AwaitOperation {
    PageCreate,
    ControllerSetup,
    BrowserStartup,
    HostAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AwaitOwner {
    host: HostId,
    generation: u64,
    operation: AwaitOperation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AwaitCancellation {
    ControllerCanceled,
    HostClosed,
}

struct AwaitedResponse {
    owner: Option<AwaitOwner>,
    cancellation: Option<AwaitCancellation>,
}

fn default_context_for_frame(
    contexts: &HashMap<(String, i32), (HostId, String)>,
    worlds: &HashMap<(String, i32), ContextWorld>,
    session: &str,
    host: HostId,
    frame: &str,
) -> Option<i32> {
    let matches = contexts
        .iter()
        .filter_map(
            |((context_session, context_id), (context_host, context_frame))| {
                let is_default = worlds
                    .get(&(context_session.clone(), *context_id))
                    .is_some_and(|world| world.is_default);
                (context_session == session
                    && *context_host == host
                    && context_frame == frame
                    && is_default)
                    .then_some(*context_id)
            },
        )
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches[0])
}

pub(crate) enum ActorNotice {
    Event {
        generation: u64,
        event: WebEvent,
    },
    Environment {
        generation: u64,
        error: Option<String>,
        browser_process_id: Option<u32>,
    },
    Controller {
        generation: u64,
        error: Option<String>,
    },
    Installed {
        generation: u64,
        report: WebInstallReport,
    },
    Frame(WebFrame),
    NavigationRequest {
        key: GateKey,
        generation: u64,
        uri: String,
    },
    ResourceRequest {
        key: GateKey,
        generation: u64,
        uri: String,
    },
}

#[derive(Default)]
pub(crate) struct HostInbox(Mutex<VecDeque<ActorNotice>>);

impl HostInbox {
    fn push(&self, notice: ActorNotice) {
        let mut queued = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let ActorNotice::Frame(frame) = notice else {
            queued.push_back(notice);
            return;
        };
        let current = queued
            .iter()
            .position(|notice| matches!(notice, ActorNotice::Frame(_)));
        if let Some(index) = current {
            let is_newer = match &queued[index] {
                ActorNotice::Frame(previous) => {
                    (frame.generation, frame.sequence) > (previous.generation, previous.sequence)
                }
                _ => unreachable!("the located notice is a frame"),
            };
            if is_newer {
                queued[index] = ActorNotice::Frame(frame);
            }
        } else {
            queued.push_back(ActorNotice::Frame(frame));
        }
    }

    pub(crate) fn take(&self) -> Vec<ActorNotice> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
            .collect()
    }

    pub(crate) fn has_events(&self) -> bool {
        !self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    }

    fn discard_frames(&self) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|notice| !matches!(notice, ActorNotice::Frame(_)));
    }
}

pub(crate) enum ActorCommand {
    Boot {
        host: HostId,
        folder: PathBuf,
        generation: u64,
        rules: String,
        color_scheme: Option<WebColorScheme>,
        inbox: Arc<HostInbox>,
        wake: Arc<dyn Fn() + Send + Sync>,
    },
    CreatePage {
        host: HostId,
        generation: u64,
        bounds: (i32, i32, u32, u32),
        scale: f64,
        visible: bool,
        color_scheme: Option<WebColorScheme>,
        rules: String,
    },
    Install {
        host: HostId,
        page: PageVisual,
        generation: u64,
    },
    RequestRules {
        host: HostId,
        rules: String,
    },
    ColorScheme {
        host: HostId,
        scheme: WebColorScheme,
    },
    Bounds {
        host: HostId,
        bounds: (i32, i32, u32, u32),
        scale: f64,
    },
    Visible {
        host: HostId,
        visible: bool,
    },
    Navigate {
        host: HostId,
        url: String,
    },
    Reload {
        host: HostId,
    },
    StopLoading {
        host: HostId,
    },
    History {
        host: HostId,
        direction: i8,
    },
    Zoom {
        host: HostId,
        factor: f64,
    },
    Find {
        host: HostId,
        term: String,
        case_sensitive: bool,
    },
    FindStep {
        host: HostId,
        forwards: bool,
    },
    FindStop {
        host: HostId,
    },
    Focus {
        host: HostId,
    },
    Mouse {
        host: HostId,
        event: WebMouseEvent,
        point: (i32, i32),
        buttons_down: u32,
    },
    Key {
        host: HostId,
        event: WebKeyEvent,
    },
    Ime {
        host: HostId,
        event: WebImeEvent,
    },
    Capture {
        host: HostId,
    },
    Favicon {
        host: HostId,
    },
    MovePage {
        host: HostId,
        page: PageVisual,
        visible: bool,
    },
    CancelCreate {
        host: HostId,
        generation: u64,
    },
    GateVerdict {
        host: HostId,
        generation: u64,
        key: GateKey,
        reply: GateReply,
    },
    Close {
        host: HostId,
    },
    ForgetEnvironment,
    Shutdown,
}

fn interrupts_cdp_write(command: &ActorCommand) -> bool {
    matches!(
        command,
        ActorCommand::CancelCreate { .. }
            | ActorCommand::Close { .. }
            | ActorCommand::ForgetEnvironment
            | ActorCommand::Shutdown
    )
}

fn consume_actor_cancellation() {
    let _ =
        PENDING_ACTOR_CANCELLATIONS.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |pending| {
            pending.checked_sub(1)
        });
}

struct BootRequest {
    host: HostId,
    folder: PathBuf,
    generation: u64,
    rules: String,
    color_scheme: Option<WebColorScheme>,
    inbox: Arc<HostInbox>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

struct CreatePageRequest {
    host: HostId,
    generation: u64,
    bounds: (i32, i32, u32, u32),
    scale: f64,
    visible: bool,
    color_scheme: Option<WebColorScheme>,
    rules: String,
}

enum ActorInput {
    Command(ActorCommand),
    Protocol {
        epoch: u64,
        message: serde_json::Value,
    },
    ProtocolClosed {
        epoch: u64,
        reason: String,
    },
}

#[derive(Clone)]
pub(crate) struct ActorHandle(Sender<ActorInput>);

impl ActorHandle {
    pub(crate) fn send(&self, command: ActorCommand) -> Result<(), String> {
        let interrupts_write = interrupts_cdp_write(&command);
        if interrupts_write {
            PENDING_ACTOR_CANCELLATIONS.fetch_add(1, Ordering::SeqCst);
        }
        if self.0.send(ActorInput::Command(command)).is_err() {
            if interrupts_write {
                consume_actor_cancellation();
            }
            return Err(String::from("the Chromium actor worker has stopped"));
        }
        Ok(())
    }
}

struct HostPage {
    inbox: Arc<HostInbox>,
    wake: Arc<dyn Fn() + Send + Sync>,
    folder: PathBuf,
    generation: u64,
    page: Option<PageVisual>,
    rules: String,
    color_scheme: Option<WebColorScheme>,
    bounds: (i32, i32, u32, u32),
    scale: f64,
    visible: bool,
    sequence: u64,
    screencast_started: bool,
    screencast_frame_reported: bool,
    /// A moved page must publish a new image for its destination rectangle. The old stream may
    /// stay quiet when the document does not paint after rehosting.
    screencast_refresh_pending: bool,
    /// A refresh may start only after the destination window has supplied bounds and scale.
    destination_bounds_ready: bool,
    target: Option<String>,
    session: Option<String>,
    tab_id: Option<i32>,
    main_frame: Option<String>,
    policy_token: Option<String>,
    navigation_binding: Option<String>,
    navigation_permit: Option<String>,
    latest_navigation_request: Option<GateKey>,
    ime_binding: Option<String>,
    ime_token: Option<String>,
    last_ime_cursor: Option<Option<[f64; 4]>>,
    main_context: Option<i32>,
    browser_process_id: u32,
    installed: bool,
    main_loader: Option<String>,
    main_status: i32,
    find_term: String,
    find_case_sensitive: bool,
    find_count: i32,
    find_active: i32,
    last_url: String,
}

impl HostPage {
    fn post(&self, notice: ActorNotice) {
        self.inbox.push(notice);
        (self.wake)();
    }
}

fn navigation_api_verdict_is_current(
    page_generation: u64,
    generation: u64,
    page_session: Option<&str>,
    main_context: Option<i32>,
    page_policy_token: Option<&str>,
    latest_request: Option<&GateKey>,
    key: &GateKey,
) -> bool {
    let GateKey::NavigationApi {
        session,
        context,
        token,
        ..
    } = key
    else {
        return false;
    };
    page_generation == generation
        && page_session == Some(session.as_str())
        && main_context == Some(*context)
        && page_policy_token == Some(token.as_str())
        && latest_request == Some(key)
}

struct PendingFetch {
    session: String,
    request: String,
    service_worker: Option<String>,
    uri: String,
    hosts: HashMap<HostId, u64>,
    votes: HashMap<HostId, bool>,
}

struct PendingWorkerSetup {
    host: HostId,
    target: String,
    generation: u64,
    pending: HashSet<u64>,
}

struct BrowserProcess {
    child: Child,
    profile: PathBuf,
    extension: PathBuf,
    _reader: Option<JoinHandle<()>>,
    output: Option<crate::linux_process::OutputReaders>,
}

struct CdpWriter {
    file: File,
    next_id: u64,
    poisoned: bool,
    #[cfg(test)]
    write_blocked: Option<Sender<()>>,
}

impl CdpWriter {
    fn send(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: serde_json::Value,
    ) -> Result<u64, String> {
        if self.poisoned {
            return Err(String::from(
                "Chromium's CDP stream is poisoned by an incomplete command",
            ));
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let mut message = serde_json::json!({
            "id": id,
            "method": method,
            "params": params,
        });
        if let Some(session) = session {
            message["sessionId"] = serde_json::Value::String(session.to_owned());
        }
        let bytes = serde_json::to_vec(&message).map_err(|error| error.to_string())?;
        let mut frame_started = false;
        if let Err(error) = self.write_all(&bytes, method, &mut frame_started) {
            self.poisoned |= frame_started;
            return Err(error);
        }
        if let Err(error) = self.write_all(&[0], method, &mut frame_started) {
            self.poisoned |= frame_started;
            return Err(error);
        }
        Ok(id)
    }

    fn write_all(
        &mut self,
        bytes: &[u8],
        method: &str,
        frame_started: &mut bool,
    ) -> Result<(), String> {
        let mut written = 0;
        while written < bytes.len() {
            if ACTOR_SHUTDOWN_REQUESTED.load(Ordering::SeqCst)
                || PENDING_ACTOR_CANCELLATIONS.load(Ordering::SeqCst) != 0
            {
                return Err(format!(
                    "CDP {method} write was interrupted by actor cancellation"
                ));
            }
            match self.file.write(&bytes[written..]) {
                Ok(0) => {
                    return Err(format!(
                        "could not write CDP {method}: Chromium's command pipe accepted zero bytes"
                    ));
                }
                Ok(count) => {
                    written += count;
                    *frame_started = true;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    #[cfg(test)]
                    if let Some(observed) = self.write_blocked.take() {
                        let _ = observed.send(());
                    }
                    let mut descriptor = libc::pollfd {
                        fd: self.file.as_raw_fd(),
                        events: libc::POLLOUT,
                        revents: 0,
                    };
                    // `CDP_POLL` is also the actor's mailbox cancellation poll;
                    // it is an interval, not a deadline for the request.
                    let ready =
                        unsafe { libc::poll(&mut descriptor, 1, CDP_POLL.as_millis() as i32) };
                    if ready == -1 {
                        let error = io::Error::last_os_error();
                        if error.kind() == io::ErrorKind::Interrupted {
                            continue;
                        }
                        return Err(format!("could not poll CDP {method} pipe: {error}"));
                    }
                    if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                        return Err(format!("Chromium closed its CDP {method} pipe"));
                    }
                }
                Err(error) => return Err(format!("could not write CDP {method}: {error}")),
            }
        }
        Ok(())
    }

    fn notify(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: serde_json::Value,
    ) -> Result<u64, String> {
        self.send(session, method, params)
    }
}

const PRIVATE_EXTENSION_MANIFEST: &str = include_str!("linux_web_extension/manifest.json");
const PRIVATE_EXTENSION_BACKGROUND: &str = include_str!("linux_web_extension/background.js");
const PRIVATE_EXTENSION_PAGE: &str = include_str!("linux_web_extension/control.html");
const PRIVATE_EXTENSION_SCRIPT: &str = include_str!("linux_web_extension/control.js");
const NAVIGATION_GATE_SOURCE: &str = include_str!("linux_web_extension/navigation_gate.js");
const IME_CURSOR_SOURCE: &str = include_str!("linux_web_extension/ime_cursor.js");
const IME_WORLD_NAME: &str = "FolioImeCursor";
const IME_CARET_PROBE: &str = r#"(() => {
  const noCaret = () => JSON.stringify({kind: "none"});
  const editor = document.activeElement;
  if (!document.hasFocus() || !editor) return noCaret();
  const viewport = window.visualViewport;
  const metrics = {
    scale: window.devicePixelRatio,
    innerWidth: window.innerWidth,
    innerHeight: window.innerHeight,
    visualScale: viewport?.scale ?? 1,
    offsetLeft: viewport?.offsetLeft ?? 0,
    offsetTop: viewport?.offsetTop ?? 0
  };
  if (editor instanceof HTMLInputElement) {
    if (!["text", "search", "url", "tel", "password", "email"].includes(editor.type)) return noCaret();
    const offset = editor.selectionDirection === "backward" ? editor.selectionStart : editor.selectionEnd;
    if (offset === null) return noCaret();
    return JSON.stringify({kind: "input", inputType: editor.type, value: editor.value, offset, ...metrics});
  }
  if (editor instanceof HTMLTextAreaElement) {
    const offset = editor.selectionDirection === "backward" ? editor.selectionStart : editor.selectionEnd;
    return JSON.stringify({kind: "textarea", value: editor.value, offset, ...metrics});
  }
  if (editor.isContentEditable) {
    const selection = window.getSelection();
    if (!selection || !selection.rangeCount || !editor.contains(selection.focusNode)) return noCaret();
    try {
      const range = document.createRange();
      range.setStart(selection.focusNode, selection.focusOffset);
      range.collapse(true);
      const rect = range.getBoundingClientRect();
      return JSON.stringify({kind: "range", rect: [rect.x, rect.y, rect.width, rect.height], ...metrics});
    } catch (_) {
      return noCaret();
    }
  }
  return noCaret();
})()"#;
const PRIVATE_EXTENSION_ID: &str = "mmjcdpbekbohkpblfhhmhmdpopcnjodc";
const PDF_VIEWER_DOCUMENT_URL: &str =
    "chrome-extension://mhjfbmdgcfjbbpaeojofohoefgiehjai/index.html";
const CHROME_RESOURCES_PREFIX: &str = "chrome://resources/";
const CDP_READER_BYTES: usize = 64 * 1024;
const CDP_POLL: Duration = Duration::from_millis(50);

fn make_pipe() -> io::Result<[OwnedFd; 2]> {
    let mut raw = [-1; 2];
    // SAFETY: `raw` points to two writable `c_int`s for pipe2 to fill.
    if unsafe { libc::pipe2(raw.as_mut_ptr(), libc::O_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: pipe2 initialized both descriptors and ownership is transferred
    // exactly once into OwnedFd values.
    Ok(unsafe { [OwnedFd::from_raw_fd(raw[0]), OwnedFd::from_raw_fd(raw[1])] })
}

fn cdp_descriptors() -> io::Result<(File, File, OwnedFd, OwnedFd)> {
    let [to_browser_read, to_browser_write] = make_pipe()?;
    let [from_browser_read, from_browser_write] = make_pipe()?;
    let child_read = duplicate_for_child(to_browser_read.as_raw_fd())?;
    let child_write = duplicate_for_child(from_browser_write.as_raw_fd())?;
    drop(to_browser_read);
    drop(from_browser_write);
    let browser_write = File::from(to_browser_write);
    set_nonblocking(&browser_write)?;
    Ok((
        File::from(from_browser_read),
        browser_write,
        child_read,
        child_write,
    ))
}

fn set_nonblocking(file: &File) -> io::Result<()> {
    // SAFETY: `file` owns a live descriptor and both fcntl commands only read
    // or update its status flags.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the descriptor remains owned by `file`; setting O_NONBLOCK only
    // makes a full pipe return WouldBlock so the actor can observe cancellation.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn duplicate_for_child(fd: RawFd) -> io::Result<OwnedFd> {
    // SAFETY: fcntl duplicates a live descriptor; the returned descriptor is
    // newly owned by the caller and is close-on-exec until it is remapped.
    let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10) };
    if duplicate == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fcntl returned a fresh descriptor that this value now owns.
    Ok(unsafe { OwnedFd::from_raw_fd(duplicate) })
}

fn kill_browser_process_group(child: &Child) {
    let process_group = -(child.id() as libc::pid_t);
    // SAFETY: Chromium was started in a process group whose id is its child pid;
    // a negative pid targets only that owned group, not Folio's group.
    let _ = unsafe { libc::kill(process_group, libc::SIGKILL) };
}

fn chromium_path_switch(switch: &str, path: &Path) -> OsString {
    let mut argument = OsString::from(switch);
    argument.push("=");
    argument.push(path.as_os_str());
    argument
}

fn protocol_reader(_worker: &WorkerCtx, mut read: File, sender: Sender<ActorInput>, epoch: u64) {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; CDP_READER_BYTES];
    loop {
        let count = match read.read(&mut chunk) {
            Ok(0) => {
                let _ = sender.send(ActorInput::ProtocolClosed {
                    epoch,
                    reason: String::from("Chromium closed its private CDP pipe"),
                });
                return;
            }
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                let _ = sender.send(ActorInput::ProtocolClosed {
                    epoch,
                    reason: format!("could not read Chromium's private CDP pipe: {error}"),
                });
                return;
            }
        };
        bytes.extend_from_slice(&chunk[..count]);
        while let Some(end) = bytes.iter().position(|byte| *byte == 0) {
            let message = bytes.drain(..=end).collect::<Vec<_>>();
            let Ok(message) = serde_json::from_slice::<serde_json::Value>(&message[..end]) else {
                let _ = sender.send(ActorInput::ProtocolClosed {
                    epoch,
                    reason: String::from("Chromium sent malformed CDP JSON"),
                });
                return;
            };
            if sender
                .send(ActorInput::Protocol { epoch, message })
                .is_err()
            {
                return;
            }
        }
    }
}

fn write_private_extension(_worker: &WorkerCtx, directory: &PathBuf) -> Result<(), String> {
    fs::create_dir_all(directory)
        .map_err(|error| format!("could not create the private Chromium extension: {error}"))?;
    for (name, source) in [
        ("manifest.json", PRIVATE_EXTENSION_MANIFEST),
        ("background.js", PRIVATE_EXTENSION_BACKGROUND),
        ("control.html", PRIVATE_EXTENSION_PAGE),
        ("control.js", PRIVATE_EXTENSION_SCRIPT),
    ] {
        if let Err(error) = fs::write(directory.join(name), source.as_bytes()) {
            let _ = fs::remove_dir_all(directory);
            return Err(format!(
                "could not write private Chromium extension {name}: {error}"
            ));
        }
    }
    Ok(())
}

fn navigation_bootstrap(binding: &str, permit: &str, token: &str) -> String {
    NAVIGATION_GATE_SOURCE
        .replace("__FOLIO_BINDING_NAME__", binding)
        .replace("__FOLIO_PERMIT_NAME__", permit)
        .replace("__FOLIO_AUTH_TOKEN__", token)
}

fn ime_cursor_bootstrap(binding: &str, token: &str) -> String {
    IME_CURSOR_SOURCE
        .replace("__FOLIO_IME_BINDING__", binding)
        .replace("__FOLIO_IME_TOKEN__", token)
}

fn json_rect(value: &serde_json::Value) -> Option<[f64; 4]> {
    let values = value.as_array()?;
    Some([
        values.first()?.as_f64()?,
        values.get(1)?.as_f64()?,
        values.get(2)?.as_f64()?,
        values.get(3)?.as_f64()?,
    ])
}

fn json_quad(value: &serde_json::Value) -> Option<[f64; 8]> {
    let values = value.as_array()?;
    Some([
        values.first()?.as_f64()?,
        values.get(1)?.as_f64()?,
        values.get(2)?.as_f64()?,
        values.get(3)?.as_f64()?,
        values.get(4)?.as_f64()?,
        values.get(5)?.as_f64()?,
        values.get(6)?.as_f64()?,
        values.get(7)?.as_f64()?,
    ])
}

fn map_rect_through_quad(rect: [f64; 4], viewport: (f64, f64), quad: [f64; 8]) -> Option<[f64; 4]> {
    let (width, height) = viewport;
    if width <= 0.0
        || height <= 0.0
        || !width.is_finite()
        || !height.is_finite()
        || rect
            .iter()
            .chain(quad.iter())
            .any(|value| !value.is_finite())
    {
        return None;
    }
    let [x0, y0, x1, y1, x2, y2, x3, y3] = quad;
    let dx3 = x0 - x1 + x2 - x3;
    let dy3 = y0 - y1 + y2 - y3;
    let affine = dx3.abs() <= 1e-9 && dy3.abs() <= 1e-9;
    let (g, h, a, b, c, d, e, f) = if affine {
        (0.0, 0.0, x1 - x0, x3 - x0, x0, y1 - y0, y3 - y0, y0)
    } else {
        let dx1 = x1 - x2;
        let dx2 = x3 - x2;
        let dy1 = y1 - y2;
        let dy2 = y3 - y2;
        let denominator = dx1 * dy2 - dx2 * dy1;
        if denominator.abs() <= 1e-12 {
            return None;
        }
        let g = (dx3 * dy2 - dx2 * dy3) / denominator;
        let h = (dx1 * dy3 - dx3 * dy1) / denominator;
        (
            g,
            h,
            x1 - x0 + g * x1,
            x3 - x0 + h * x3,
            x0,
            y1 - y0 + g * y1,
            y3 - y0 + h * y3,
            y0,
        )
    };
    let transform = |x: f64, y: f64| -> Option<(f64, f64)> {
        let u = x / width;
        let v = y / height;
        let divisor = g * u + h * v + 1.0;
        if divisor.abs() <= 1e-12 {
            return None;
        }
        let mapped_x = (a * u + b * v + c) / divisor;
        let mapped_y = (d * u + e * v + f) / divisor;
        (mapped_x.is_finite() && mapped_y.is_finite()).then_some((mapped_x, mapped_y))
    };
    let points = [
        transform(rect[0], rect[1])?,
        transform(rect[2], rect[1])?,
        transform(rect[2], rect[3])?,
        transform(rect[0], rect[3])?,
    ];
    Some([
        points
            .iter()
            .map(|point| point.0)
            .fold(f64::INFINITY, f64::min),
        points
            .iter()
            .map(|point| point.1)
            .fold(f64::INFINITY, f64::min),
        points
            .iter()
            .map(|point| point.0)
            .fold(f64::NEG_INFINITY, f64::max),
        points
            .iter()
            .map(|point| point.1)
            .fold(f64::NEG_INFINITY, f64::max),
    ])
}

fn index_frame_tree(
    tree: &serde_json::Value,
    host: HostId,
    frames: &mut HashMap<String, HostId>,
    parents: &mut HashMap<String, String>,
    sessions: &mut HashMap<String, String>,
    urls: &mut HashMap<String, String>,
    session: &str,
) {
    let Some(frame) = tree.get("frame") else {
        return;
    };
    let Some(id) = frame.get("id").and_then(serde_json::Value::as_str) else {
        return;
    };
    frames.insert(id.to_owned(), host);
    sessions.insert(id.to_owned(), session.to_owned());
    if let Some(url) = frame.get("url").and_then(serde_json::Value::as_str) {
        urls.insert(id.to_owned(), url.to_owned());
    }
    if let Some(parent) = frame.get("parentId").and_then(serde_json::Value::as_str) {
        parents.insert(id.to_owned(), parent.to_owned());
    }
    if let Some(children) = tree
        .get("childFrames")
        .and_then(serde_json::Value::as_array)
    {
        for child in children {
            index_frame_tree(child, host, frames, parents, sessions, urls, session);
        }
    }
}

fn update_frame_parent(
    parents: &mut HashMap<String, String>,
    session_roots: &mut HashMap<String, String>,
    session: &str,
    frame: &str,
    parent: Option<&str>,
    browser_root_session: bool,
) {
    if let Some(parent) = parent {
        parents.insert(frame.to_owned(), parent.to_owned());
    } else if browser_root_session {
        parents.remove(frame);
        session_roots.insert(session.to_owned(), frame.to_owned());
    }
}

fn retire_detached_frames(
    reason: &str,
    frame: &str,
    frames: &mut HashMap<String, HostId>,
    parents: &mut HashMap<String, String>,
    sessions: &mut HashMap<String, String>,
    urls: &mut HashMap<String, String>,
) -> HashSet<String> {
    if reason == "swap" {
        return HashSet::new();
    }

    let mut detached = HashSet::from([frame.to_owned()]);
    loop {
        let descendants = parents
            .iter()
            .filter(|(_, parent)| detached.contains(*parent))
            .map(|(child, _)| child.clone())
            .collect::<Vec<_>>();
        let before = detached.len();
        detached.extend(descendants);
        if detached.len() == before {
            break;
        }
    }
    parents.retain(|child, parent| !detached.contains(child) && !detached.contains(parent));
    frames.retain(|id, _| !detached.contains(id));
    sessions.retain(|id, _| !detached.contains(id));
    urls.retain(|id, _| !detached.contains(id));
    detached
}

#[allow(clippy::too_many_arguments)] // Each argument is an independently observed CDP ownership fact.
fn is_owned_pdf_viewer_resource(
    host: HostId,
    session: &str,
    frame: &str,
    url: &str,
    main_frame: &str,
    frame_hosts: &HashMap<String, HostId>,
    frame_sessions: &HashMap<String, String>,
    frame_parents: &HashMap<String, String>,
    frame_urls: &HashMap<String, String>,
) -> bool {
    url.starts_with(CHROME_RESOURCES_PREFIX)
        && frame_hosts.get(frame) == Some(&host)
        && frame_hosts.get(main_frame) == Some(&host)
        && frame_sessions.contains_key(main_frame)
        && frame_sessions
            .get(frame)
            .is_some_and(|owner_session| owner_session == session)
        && frame_parents
            .get(frame)
            .is_some_and(|parent| parent == main_frame)
        && frame_urls
            .get(frame)
            .is_some_and(|frame_url| frame_url == PDF_VIEWER_DOCUMENT_URL)
}

fn navigation_hook_install_failure(result: &serde_json::Value) -> Option<String> {
    if let Some(exception) = result.get("exceptionDetails") {
        let detail = exception
            .pointer("/exception/description")
            .and_then(serde_json::Value::as_str)
            .or_else(|| exception.get("text").and_then(serde_json::Value::as_str))
            .unwrap_or("the bootstrap raised an exception");
        return Some(format!(
            "Chromium could not install the main-frame navigation policy hook: bootstrap exception: {detail}"
        ));
    }
    if result
        .pointer("/result/value/installed")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    let reason = result
        .pointer("/result/value/reason")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("the browser returned no success status");
    Some(format!(
        "Chromium could not install the main-frame navigation policy hook: {reason}"
    ))
}

fn keep_policy_page_attachment(
    attach_pending: bool,
    control_session: Option<&str>,
    attached_session: &str,
) -> bool {
    attach_pending || control_session == Some(attached_session)
}

fn document_load_has_completed(result: &serde_json::Value) -> bool {
    result
        .pointer("/result/value")
        .and_then(serde_json::Value::as_str)
        == Some("complete")
}

fn input_text_backend_node(
    element: &serde_json::Value,
    value: &str,
    offset: u32,
    allow_masked: bool,
) -> Option<(i64, u32)> {
    let expected = value.encode_utf16().count();
    fn visit(
        node: &serde_json::Value,
        expected: usize,
        offset: u32,
        allow_masked: bool,
        value: &str,
        answer: &mut Option<(i64, usize)>,
    ) {
        if answer.is_some() {
            return;
        }
        if node.get("nodeName").and_then(serde_json::Value::as_str) == Some("#text") {
            let text = node
                .get("nodeValue")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let units = text.encode_utf16().count();
            if (text == value || (allow_masked && units == expected))
                && (offset as usize) <= units
                && let Some(backend) = node
                    .get("backendNodeId")
                    .and_then(serde_json::Value::as_i64)
            {
                *answer = Some((backend, offset as usize));
            }
            return;
        }
        for key in ["shadowRoots", "children"] {
            if let Some(children) = node.get(key).and_then(serde_json::Value::as_array) {
                for child in children {
                    visit(child, expected, offset, allow_masked, value, answer);
                }
            }
        }
    }
    let mut answer = None;
    if let Some(shadow_roots) = element
        .get("shadowRoots")
        .and_then(serde_json::Value::as_array)
    {
        for root in shadow_roots {
            visit(root, expected, offset, allow_masked, value, &mut answer);
        }
    }
    answer.map(|(backend, offset)| (backend, offset as u32))
}

fn textarea_text_backend_node(
    element: &serde_json::Value,
    value: &str,
    offset: u32,
) -> Option<(i64, u32)> {
    #[derive(Clone)]
    enum Part {
        Text { backend: i64, value: String },
        Break,
    }
    fn collect(node: &serde_json::Value, parts: &mut Vec<Part>) {
        match node.get("nodeName").and_then(serde_json::Value::as_str) {
            Some("#text") => {
                if let (Some(backend), Some(value)) = (
                    node.get("backendNodeId")
                        .and_then(serde_json::Value::as_i64),
                    node.get("nodeValue").and_then(serde_json::Value::as_str),
                ) {
                    parts.push(Part::Text {
                        backend,
                        value: value.to_owned(),
                    });
                }
                return;
            }
            Some("BR") => {
                parts.push(Part::Break);
                return;
            }
            _ => {}
        }
        if let Some(children) = node.get("children").and_then(serde_json::Value::as_array) {
            for child in children {
                collect(child, parts);
            }
        }
    }
    let shadow_roots = element
        .get("shadowRoots")
        .and_then(serde_json::Value::as_array)?;
    let mut parts = Vec::new();
    for root in shadow_roots {
        collect(root, &mut parts);
    }
    let rendered = parts
        .iter()
        .map(|part| match part {
            Part::Text { value, .. } => value.as_str(),
            Part::Break => "\n",
        })
        .collect::<String>();
    if rendered != value {
        return None;
    }
    let offset = offset as usize;
    let mut at = 0;
    for part in parts {
        match part {
            Part::Text { backend, value } => {
                let units = value.encode_utf16().count();
                if offset >= at && offset <= at + units {
                    return Some((backend, (offset - at) as u32));
                }
                at += units;
            }
            Part::Break => at += 1,
        }
    }
    None
}

fn cdp_mouse_buttons(buttons: u32) -> u32 {
    let mut mapped = 0;
    if buttons & crate::web_mouse_buttons::LEFT != 0 {
        mapped |= 1;
    }
    if buttons & crate::web_mouse_buttons::RIGHT != 0 {
        mapped |= 2;
    }
    if buttons & crate::web_mouse_buttons::MIDDLE != 0 {
        mapped |= 4;
    }
    if buttons & crate::web_mouse_buttons::X1 != 0 {
        mapped |= 8;
    }
    if buttons & crate::web_mouse_buttons::X2 != 0 {
        mapped |= 16;
    }
    mapped
}

fn rgba_to_bgra(mut rgba: Vec<u8>) -> Vec<u8> {
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    rgba
}

fn prepare_profile(_worker: &WorkerCtx, profile: &Path) -> Result<(), String> {
    let default_profile = profile.join("Default");
    fs::create_dir_all(&default_profile)
        .map_err(|error| format!("could not create Chromium's default profile: {error}"))?;
    let preferences_path = default_profile.join("Preferences");
    let mut preferences = if preferences_path.exists() {
        let bytes = fs::read(&preferences_path)
            .map_err(|error| format!("could not read Chromium preferences: {error}"))?;
        serde_json::from_slice::<serde_json::Value>(&bytes)
            .map_err(|error| format!("Chromium preferences are invalid JSON: {error}"))?
    } else {
        serde_json::json!({})
    };
    let root = preferences
        .as_object_mut()
        .ok_or_else(|| String::from("Chromium preferences root is not an object"))?;
    root.entry("autofill")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| String::from("Chromium autofill preferences are not an object"))?
        .extend([
            (
                String::from("profile_enabled"),
                serde_json::Value::Bool(false),
            ),
            (
                String::from("credit_card_enabled"),
                serde_json::Value::Bool(false),
            ),
        ]);
    root.insert(
        String::from("credentials_enable_service"),
        serde_json::Value::Bool(false),
    );
    root.entry("profile")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| String::from("Chromium profile preferences are not an object"))?
        .insert(
            String::from("password_manager_enabled"),
            serde_json::Value::Bool(false),
        );
    fs::write(
        &preferences_path,
        serde_json::to_vec(&preferences).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("could not write Chromium preferences: {error}"))
}

struct Actor<'worker> {
    worker_ctx: &'worker WorkerCtx,
    worker: Sender<ActorInput>,
    input: Receiver<ActorInput>,
    deferred: VecDeque<ActorCommand>,
    protocol_events: VecDeque<serde_json::Value>,
    protocol_responses: HashMap<u64, serde_json::Value>,
    awaited_responses: HashMap<u64, AwaitedResponse>,
    pending_worker_setups: HashMap<String, PendingWorkerSetup>,
    pending_worker_responses: HashMap<u64, (String, &'static str)>,
    loaded_sessions: HashSet<String>,
    cdp: Option<CdpWriter>,
    browser: Option<BrowserProcess>,
    browser_epoch: u64,
    pending_page_create: Option<(HostId, u64)>,
    pending_controller_setup: Option<(HostId, u64, String)>,
    /// The one canceled `Target.createTarget` whose reply is still unknown.
    /// Page creation is serialized and gated until this disposition resolves.
    cancelled_target_create_response: Option<(u64, HostId, u64)>,
    pending_create: HashMap<String, (HostId, u64)>,
    pending_install: HashSet<HostId>,
    cancelled_hosts: HashSet<HostId>,
    pending_fetches: std::collections::HashMap<GateKey, PendingFetch>,
    hosts: std::collections::HashMap<HostId, HostPage>,
    sessions: std::collections::HashMap<String, (HostId, bool)>,
    session_targets: std::collections::HashMap<String, String>,
    contexts: std::collections::HashMap<(String, i32), (HostId, String)>,
    context_worlds: std::collections::HashMap<(String, i32), ContextWorld>,
    frame_hosts: std::collections::HashMap<String, HostId>,
    frame_parents: std::collections::HashMap<String, String>,
    frame_sessions: std::collections::HashMap<String, String>,
    frame_urls: std::collections::HashMap<String, String>,
    session_root_frames: std::collections::HashMap<String, String>,
    targets: std::collections::HashMap<String, HostId>,
    target_sessions: std::collections::HashMap<String, String>,
    service_worker_clients: std::collections::HashMap<String, Option<HashSet<String>>>,
    service_worker_targets: HashSet<String>,
    extension_target: Option<String>,
    extension_session: Option<String>,
    extension_attach_pending: bool,
    extension_attach_event: Option<String>,
    extension_id: Option<String>,
    active_boot: Option<(HostId, u64)>,
    running: bool,
}

impl<'worker> Actor<'worker> {
    fn new(
        worker_ctx: &'worker WorkerCtx,
        worker: Sender<ActorInput>,
        input: Receiver<ActorInput>,
    ) -> Self {
        Self {
            worker_ctx,
            worker,
            input,
            deferred: VecDeque::new(),
            protocol_events: VecDeque::new(),
            protocol_responses: HashMap::new(),
            awaited_responses: HashMap::new(),
            pending_worker_setups: HashMap::new(),
            pending_worker_responses: HashMap::new(),
            loaded_sessions: HashSet::new(),
            cdp: None,
            browser: None,
            browser_epoch: 0,
            pending_page_create: None,
            pending_controller_setup: None,
            cancelled_target_create_response: None,
            pending_create: HashMap::new(),
            pending_install: HashSet::new(),
            cancelled_hosts: HashSet::new(),
            pending_fetches: std::collections::HashMap::new(),
            hosts: std::collections::HashMap::new(),
            sessions: std::collections::HashMap::new(),
            session_targets: std::collections::HashMap::new(),
            contexts: std::collections::HashMap::new(),
            context_worlds: std::collections::HashMap::new(),
            frame_hosts: std::collections::HashMap::new(),
            frame_parents: std::collections::HashMap::new(),
            frame_sessions: std::collections::HashMap::new(),
            frame_urls: std::collections::HashMap::new(),
            session_root_frames: std::collections::HashMap::new(),
            targets: std::collections::HashMap::new(),
            target_sessions: std::collections::HashMap::new(),
            service_worker_clients: std::collections::HashMap::new(),
            service_worker_targets: HashSet::new(),
            extension_target: None,
            extension_session: None,
            extension_attach_pending: false,
            extension_attach_event: None,
            extension_id: None,
            active_boot: None,
            running: true,
        }
    }

    fn post(&self, host: HostId, notice: ActorNotice) {
        if let Some(page) = self.hosts.get(&host) {
            page.post(notice);
        }
    }

    fn send_command(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: serde_json::Value,
    ) -> Result<u64, String> {
        let sent = match self.cdp.as_mut() {
            Some(cdp) => cdp.send(session, method, params),
            None => Err(String::from("Chromium's CDP pipe is closed")),
        };
        match sent {
            Ok(id) => Ok(id),
            Err(error) => {
                if self.cdp.as_ref().is_some_and(|cdp| cdp.poisoned) {
                    let worker_ctx = self.worker_ctx;
                    self.browser_failed(worker_ctx, error.clone());
                }
                Err(error)
            }
        }
    }

    fn await_owner_for_call(&self, session: Option<&str>) -> Option<AwaitOwner> {
        if let Some((host, _)) = session.and_then(|session| self.sessions.get(session).copied()) {
            if host == 0 {
                return None;
            }
            let generation = self.hosts.get(&host)?.generation;
            return Some(AwaitOwner {
                host,
                generation,
                operation: self.await_operation_for(host, generation),
            });
        }
        if let Some((host, generation)) = self.pending_page_create {
            return Some(AwaitOwner {
                host,
                generation,
                operation: AwaitOperation::PageCreate,
            });
        }
        if let Some((host, generation, _)) = self.pending_controller_setup.as_ref() {
            return Some(AwaitOwner {
                host: *host,
                generation: *generation,
                operation: AwaitOperation::ControllerSetup,
            });
        }
        self.active_boot.map(|(host, generation)| AwaitOwner {
            host,
            generation,
            operation: AwaitOperation::BrowserStartup,
        })
    }

    fn await_operation_for(&self, host: HostId, generation: u64) -> AwaitOperation {
        if self.pending_page_create == Some((host, generation)) {
            AwaitOperation::PageCreate
        } else if self.pending_controller_setup.as_ref().is_some_and(
            |(owner, pending_generation, _)| *owner == host && *pending_generation == generation,
        ) {
            AwaitOperation::ControllerSetup
        } else if self.active_boot == Some((host, generation)) {
            AwaitOperation::BrowserStartup
        } else {
            AwaitOperation::HostAction
        }
    }

    fn call(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let owner = self.await_owner_for_call(session);
        let id = self.send_command(session, method, params)?;
        let worker_ctx = self.worker_ctx;
        self.await_response_with_owner(worker_ctx, id, method, owner)
    }

    fn handle_protocol_response(&mut self, message: serde_json::Value) {
        let Some(id) = message.get("id").and_then(serde_json::Value::as_u64) else {
            return;
        };
        if let Some((cancelled_id, host, generation)) = self.cancelled_target_create_response
            && cancelled_id == id
        {
            self.cancelled_target_create_response = None;
            if let Some(target) = message
                .pointer("/result/targetId")
                .and_then(serde_json::Value::as_str)
            {
                trace_browser_startup(
                    self.worker_ctx,
                    &format!(
                        "closing late canceled page target host={host} generation={generation} target={target}"
                    ),
                );
                let _ = self.notify(
                    None,
                    "Target.closeTarget",
                    serde_json::json!({"targetId": target}),
                );
            }
            return;
        }
        let Some((session, method)) = self.pending_worker_responses.remove(&id) else {
            if self.awaited_responses.contains_key(&id) {
                self.protocol_responses.insert(id, message);
            }
            return;
        };
        let error = message.get("error").map(ToString::to_string);
        let complete = self.pending_worker_setups.get_mut(&session).map(|setup| {
            setup.pending.remove(&id);
            setup.pending.is_empty()
        });
        if let Some(error) = error {
            self.fail_worker_setup(&session, format!("CDP {method} failed: {error}"));
        } else if complete == Some(true)
            && let Some(setup) = self.pending_worker_setups.remove(&session)
        {
            trace_browser_startup(
                self.worker_ctx,
                &format!(
                    "worker target setup ready session={session} target={} host={}",
                    setup.target, setup.host
                ),
            );
        }
    }

    fn fail_worker_setup(&mut self, session: &str, error: String) {
        let Some(setup) = self.pending_worker_setups.remove(session) else {
            return;
        };
        for id in setup.pending {
            self.pending_worker_responses.remove(&id);
        }
        let _ = self.notify(
            None,
            "Target.closeTarget",
            serde_json::json!({"targetId": setup.target}),
        );
        trace_browser_startup(
            self.worker_ctx,
            &format!(
                "worker target setup failed session={session} target={} error={error}",
                setup.target
            ),
        );
        if setup.host != 0
            && let Some(page) = self.hosts.get(&setup.host)
            && page.generation == setup.generation
        {
            page.post(ActorNotice::Event {
                generation: setup.generation,
                event: WebEvent::ProcessFailed {
                    kind: 1,
                    description: format!("the browser worker could not be guarded: {error}"),
                },
            });
        }
    }

    fn discard_worker_setup(&mut self, session: &str) {
        if let Some(setup) = self.pending_worker_setups.remove(session) {
            for id in setup.pending {
                self.pending_worker_responses.remove(&id);
            }
        }
    }

    fn abandon_target_create_response(&mut self, id: u64, host: HostId, generation: u64) {
        self.cancelled_hosts.insert(host);
        self.pending_page_create = None;
        debug_assert!(self.cancelled_target_create_response.is_none());
        self.cancelled_target_create_response = Some((id, host, generation));
    }

    fn prepare_page_create(&mut self, worker: &WorkerCtx, host: HostId) -> Result<bool, String> {
        let Some((_, canceled_host, canceled_generation)) = self.cancelled_target_create_response
        else {
            return Ok(false);
        };
        let another_live_host = self
            .hosts
            .keys()
            .any(|candidate| *candidate != host && !self.cancelled_hosts.contains(candidate));
        if another_live_host {
            return Err(format!(
                "Chromium is still resolving canceled page creation for host {canceled_host}, generation {canceled_generation}"
            ));
        }

        // With no other live host depending on this browser, retiring its
        // process ends the unknown operation and makes a fresh create safe.
        self.finish_browser(worker, true);
        Ok(true)
    }

    fn retire_host_session_state(&mut self, host: HostId) {
        let requests = self
            .pending_fetches
            .iter()
            .filter(|(_, pending)| pending.hosts.contains_key(&host))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in requests {
            self.fail_fetch(&key, "Aborted");
        }

        let host_sessions = self
            .sessions
            .iter()
            .filter(|(_, (owner, _))| *owner == host)
            .map(|(session, _)| session.clone())
            .collect::<HashSet<_>>();
        let shared_workers = self
            .session_targets
            .iter()
            .filter(|(session, target)| {
                host_sessions.contains(*session)
                    && self.service_worker_targets.contains(*target)
                    && self
                        .service_worker_clients
                        .get(*target)
                        .and_then(Option::as_ref)
                        .is_some_and(|clients| {
                            clients.iter().any(|client| {
                                self.targets.get(client).is_some_and(|owner| *owner != host)
                            })
                        })
            })
            .map(|(session, target)| (session.clone(), target.clone()))
            .collect::<HashMap<_, _>>();
        let shared_worker_sessions = shared_workers.keys().cloned().collect::<HashSet<_>>();
        let shared_worker_targets = shared_workers.values().cloned().collect::<HashSet<_>>();
        let retired_sessions = host_sessions
            .iter()
            .filter(|session| !shared_worker_sessions.contains(*session))
            .cloned()
            .collect::<HashSet<_>>();
        let mut retired_targets = self
            .targets
            .iter()
            .filter(|(target, owner)| **owner == host && !shared_worker_targets.contains(*target))
            .map(|(target, _)| target.clone())
            .collect::<HashSet<_>>();
        retired_targets.extend(
            self.session_targets
                .iter()
                .filter(|(session, _)| retired_sessions.contains(*session))
                .map(|(_, target)| target.clone()),
        );

        for session in &retired_sessions {
            self.discard_worker_setup(session);
            self.loaded_sessions.remove(session);
        }
        for target in &retired_targets {
            self.targets.remove(target);
            self.target_sessions.remove(target);
            self.service_worker_targets.remove(target);
            self.service_worker_clients.remove(target);
        }
        self.sessions.retain(|session, (owner, _)| {
            *owner != host || shared_worker_sessions.contains(session)
        });
        for session in &shared_worker_sessions {
            if let Some((owner, _)) = self.sessions.get_mut(session) {
                *owner = 0;
            }
        }
        self.targets
            .retain(|target, owner| *owner != host || shared_worker_targets.contains(target));
        for target in &shared_worker_targets {
            if let Some(owner) = self.targets.get_mut(target) {
                *owner = 0;
            }
        }
        self.session_targets.retain(|session, target| {
            !retired_sessions.contains(session) && !retired_targets.contains(target)
        });
        self.target_sessions.retain(|target, session| {
            !retired_sessions.contains(session) && !retired_targets.contains(target)
        });
        for clients in self.service_worker_clients.values_mut().flatten() {
            clients.retain(|client| !retired_targets.contains(client));
        }
        self.contexts.retain(|_, (owner, _)| *owner != host);
        self.context_worlds
            .retain(|(session, _), _| self.sessions.contains_key(session));
        self.session_root_frames
            .retain(|session, _| self.sessions.contains_key(session));
        self.loaded_sessions
            .retain(|session| self.sessions.contains_key(session));

        let frames = self
            .frame_hosts
            .iter()
            .filter(|(_, owner)| **owner == host)
            .map(|(frame, _)| frame.clone())
            .collect::<HashSet<_>>();
        self.frame_hosts.retain(|_, owner| *owner != host);
        self.frame_parents
            .retain(|frame, parent| !frames.contains(frame) && !frames.contains(parent));
        self.frame_sessions
            .retain(|frame, _| self.frame_hosts.contains_key(frame));
        self.frame_urls
            .retain(|frame, _| self.frame_hosts.contains_key(frame));
        self.pending_create.retain(|_, (owner, _)| *owner != host);
        self.pending_install.remove(&host);
    }

    fn close_controller_target(&mut self, host: HostId, generation: u64, target: &str) {
        self.cancelled_hosts.insert(host);
        self.retire_host_session_state(host);
        self.pending_create.remove(target);
        self.targets.remove(target);
        if let Some(page) = self.hosts.get_mut(&host)
            && page.generation == generation
            && page.target.as_deref() == Some(target)
        {
            page.target = None;
            page.session = None;
            page.tab_id = None;
            page.main_frame = None;
            page.main_loader = None;
            page.main_status = 0;
            page.last_url.clear();
            page.policy_token = None;
            page.navigation_binding = None;
            page.navigation_permit = None;
            page.latest_navigation_request = None;
            page.ime_binding = None;
            page.ime_token = None;
            page.last_ime_cursor = None;
            page.main_context = None;
            page.screencast_started = false;
            page.screencast_frame_reported = false;
            page.installed = false;
        }
        if self.pending_controller_setup.as_ref().is_some_and(
            |(owner, pending_generation, pending_target)| {
                *owner == host && *pending_generation == generation && pending_target == target
            },
        ) {
            self.pending_controller_setup = None;
        }
        let _ = self.notify(
            None,
            "Target.closeTarget",
            serde_json::json!({"targetId": target}),
        );
    }

    fn mark_active_awaits_cancelled(
        &mut self,
        host: HostId,
        generation: Option<u64>,
        setup_only: bool,
        cancellation: AwaitCancellation,
    ) -> bool {
        let mut matched = false;
        for awaited in self.awaited_responses.values_mut() {
            let Some(owner) = awaited.owner else {
                continue;
            };
            if owner.host == host
                && generation.is_none_or(|generation| generation == owner.generation)
                && (!setup_only
                    || matches!(
                        owner.operation,
                        AwaitOperation::PageCreate | AwaitOperation::ControllerSetup
                    ))
            {
                awaited.cancellation.get_or_insert(cancellation);
                matched = true;
            }
        }
        matched
    }

    fn active_await_cancellation(&self, id: u64) -> Option<AwaitCancellation> {
        self.awaited_responses
            .get(&id)
            .and_then(|awaited| awaited.cancellation)
    }

    fn page_create_wait_id(&self, host: HostId, generation: u64) -> Option<u64> {
        self.awaited_responses.iter().find_map(|(id, awaited)| {
            awaited
                .owner
                .is_some_and(|owner| {
                    owner.host == host
                        && owner.generation == generation
                        && owner.operation == AwaitOperation::PageCreate
                })
                .then_some(*id)
        })
    }

    fn interrupt_page_setup_response(&mut self, host: HostId, generation: u64) -> bool {
        if self.pending_page_create == Some((host, generation)) {
            let Some(id) = self.page_create_wait_id(host, generation) else {
                return false;
            };
            self.mark_active_awaits_cancelled(
                host,
                Some(generation),
                true,
                AwaitCancellation::ControllerCanceled,
            );
            self.abandon_target_create_response(id, host, generation);
            return true;
        }
        if let Some((setup_host, setup_generation, target)) = self.pending_controller_setup.as_ref()
            && *setup_host == host
            && *setup_generation == generation
        {
            let target = target.clone();
            self.mark_active_awaits_cancelled(
                host,
                Some(generation),
                true,
                AwaitCancellation::ControllerCanceled,
            );
            self.close_controller_target(host, generation, &target);
            return true;
        }
        false
    }

    #[cfg(test)]
    fn await_response(
        &mut self,
        worker: &WorkerCtx,
        id: u64,
        method: &str,
    ) -> Result<serde_json::Value, String> {
        let owner = self.await_owner_for_call(None);
        self.await_response_with_owner(worker, id, method, owner)
    }

    fn await_response_with_owner(
        &mut self,
        worker: &WorkerCtx,
        id: u64,
        method: &str,
        owner: Option<AwaitOwner>,
    ) -> Result<serde_json::Value, String> {
        let previous = self.awaited_responses.insert(
            id,
            AwaitedResponse {
                owner,
                cancellation: None,
            },
        );
        debug_assert!(
            previous.is_none(),
            "a CDP response id has one active waiter"
        );
        let result = self.await_response_inner(worker, id, method);
        self.awaited_responses.remove(&id);
        self.protocol_responses.remove(&id);
        result
    }

    fn await_response_inner(
        &mut self,
        worker: &WorkerCtx,
        id: u64,
        method: &str,
    ) -> Result<serde_json::Value, String> {
        let browser_epoch = self.browser_epoch;
        loop {
            if let Some(cancellation) = self
                .awaited_responses
                .get(&id)
                .and_then(|awaited| awaited.cancellation)
            {
                return Err(match cancellation {
                    AwaitCancellation::ControllerCanceled => format!(
                        "the web controller was canceled while Chromium awaited CDP {method}"
                    ),
                    AwaitCancellation::HostClosed => {
                        format!("the web seat closed while Chromium awaited CDP {method}")
                    }
                });
            }
            if browser_epoch != self.browser_epoch {
                return Err(format!(
                    "Chromium's browser epoch ended while waiting for CDP {method}"
                ));
            }
            if let Some(response) = self.protocol_responses.remove(&id) {
                if let Some(error) = response.get("error") {
                    return Err(format!("CDP {method} failed: {error}"));
                }
                return Ok(response
                    .get("result")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null));
            }
            match self.input.recv_timeout(Duration::from_millis(50)) {
                Ok(ActorInput::Protocol { epoch, message }) => {
                    if epoch != self.browser_epoch {
                        continue;
                    }
                    if message
                        .get("id")
                        .and_then(serde_json::Value::as_u64)
                        .is_some()
                    {
                        self.handle_protocol_response(message);
                    } else if self.pending_page_create.is_some() && is_page_attach_event(&message) {
                        self.protocol_events.push_back(message);
                    } else {
                        self.handle_protocol_event(message);
                    }
                }
                Ok(ActorInput::ProtocolClosed { epoch, reason }) => {
                    if epoch == self.browser_epoch {
                        return Err(reason);
                    }
                }
                Ok(ActorInput::Command(command)) => match command {
                    ActorCommand::Shutdown => {
                        consume_actor_cancellation();
                        self.running = false;
                        if let Some(browser) = self.browser.as_mut() {
                            let _ = browser.child.kill();
                        }
                        return Err(String::from("the Chromium actor is shutting down"));
                    }
                    ActorCommand::Close { host } => {
                        consume_actor_cancellation();
                        self.cancelled_hosts.insert(host);
                        self.mark_active_awaits_cancelled(
                            host,
                            None,
                            false,
                            AwaitCancellation::HostClosed,
                        );
                        if self.active_boot.is_some_and(|(owner, _)| owner == host) {
                            let current_cancellation = self.active_await_cancellation(id);
                            self.deferred.push_front(ActorCommand::Close { host });
                            self.finish_browser(worker, true);
                            if let Some(cancellation) = current_cancellation {
                                return Err(match cancellation {
                                    AwaitCancellation::ControllerCanceled => String::from(
                                        "the web controller was canceled while Chromium was starting",
                                    ),
                                    AwaitCancellation::HostClosed => String::from(
                                        "the web seat closed while Chromium was starting",
                                    ),
                                });
                            }
                            continue;
                        }
                        if let Some((owner, generation)) =
                            self.pending_page_create.filter(|(owner, _)| *owner == host)
                        {
                            if let Some(create_id) = self.page_create_wait_id(owner, generation) {
                                self.abandon_target_create_response(create_id, owner, generation);
                            }
                            self.deferred.push_front(ActorCommand::Close { host });
                            if let Some(cancellation) = self.active_await_cancellation(id) {
                                return Err(match cancellation {
                                    AwaitCancellation::ControllerCanceled => String::from(
                                        "the web controller was canceled while Chromium created its page",
                                    ),
                                    AwaitCancellation::HostClosed => String::from(
                                        "the web seat closed while Chromium created its page",
                                    ),
                                });
                            }
                            continue;
                        }
                        if let Some((owner, generation, target)) =
                            self.pending_controller_setup.as_ref()
                            && *owner == host
                        {
                            let (generation, target) = (*generation, target.clone());
                            self.close_controller_target(host, generation, &target);
                            self.deferred.push_front(ActorCommand::Close { host });
                            if let Some(cancellation) = self.active_await_cancellation(id) {
                                return Err(match cancellation {
                                    AwaitCancellation::ControllerCanceled => String::from(
                                        "the web controller was canceled while Chromium configured its page",
                                    ),
                                    AwaitCancellation::HostClosed => String::from(
                                        "the web seat closed while Chromium configured its page",
                                    ),
                                });
                            }
                            continue;
                        }
                        if let Some(target) = self
                            .hosts
                            .get(&host)
                            .and_then(|page| page.target.as_ref())
                            .cloned()
                        {
                            let _ = self.notify(
                                None,
                                "Target.closeTarget",
                                serde_json::json!({"targetId": target}),
                            );
                        }
                        self.deferred.push_front(ActorCommand::Close { host });
                        if let Some(cancellation) = self.active_await_cancellation(id) {
                            return Err(match cancellation {
                                AwaitCancellation::ControllerCanceled => String::from(
                                    "the web controller was canceled while Chromium handled a close",
                                ),
                                AwaitCancellation::HostClosed => String::from(
                                    "the web seat closed while Chromium handled a close",
                                ),
                            });
                        }
                        continue;
                    }
                    ActorCommand::CancelCreate { host, generation } => {
                        consume_actor_cancellation();
                        if self.interrupt_page_setup_response(host, generation) {
                            if self.active_await_cancellation(id)
                                == Some(AwaitCancellation::ControllerCanceled)
                            {
                                return Err(format!(
                                    "the web controller was canceled while Chromium awaited CDP {method}"
                                ));
                            }
                            continue;
                        }
                        self.deferred
                            .push_back(ActorCommand::CancelCreate { host, generation });
                    }
                    ActorCommand::GateVerdict {
                        host,
                        generation,
                        key,
                        reply,
                    } => self.gate_verdict(host, generation, key, reply),
                    ActorCommand::StopLoading { host } => self.stop_loading(host),
                    other => {
                        if interrupts_cdp_write(&other) {
                            consume_actor_cancellation();
                        }
                        self.deferred.push_back(other);
                    }
                },
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(browser) = self.browser.as_mut()
                        && let Some(status) = browser
                            .child
                            .try_wait()
                            .map_err(|error| format!("could not check Chromium process: {error}"))?
                    {
                        return Err(format!("Chromium exited with {status}"));
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(String::from("the Chromium actor channel closed"));
                }
            }
        }
    }

    fn notify(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), String> {
        self.send_command(session, method, params)?;
        Ok(())
    }

    fn start_browser(&mut self, worker: &WorkerCtx, folder: &PathBuf) -> Result<(), String> {
        if let Some(browser) = &self.browser {
            if browser.profile == *folder {
                return Ok(());
            }
            return Err(format!(
                "Chromium already owns profile {}; it cannot also own {}",
                browser.profile.display(),
                folder.display()
            ));
        }
        let data_root = folder
            .parent()
            .ok_or_else(|| format!("Chromium profile has no data root: {}", folder.display()))?;
        let expected_profile = crate::linux_web_profile(data_root);
        if expected_profile != *folder {
            return Err(format!(
                "request_environment passed {}, expected {}",
                folder.display(),
                expected_profile.display()
            ));
        }
        let directories = crate::prepare_linux_web_dirs(worker, data_root)?;
        if directories.profile != *folder {
            return Err(format!(
                "Linux web directory resolver returned {}, expected {}",
                directories.profile.display(),
                folder.display()
            ));
        }

        let extension = directories
            .runtime
            .join("WebPolicyExtension")
            .join(crate::instance::directory_tag(data_root));
        let executable = find_browser()?;
        prepare_profile(worker, &directories.profile)?;
        let (browser_read, browser_write, child_read, child_write) = cdp_descriptors()
            .map_err(|error| format!("could not create Chromium's private CDP pipe: {error}"))?;
        write_private_extension(worker, &extension)?;
        let mut command = crate::quiet_command(&executable);
        command.env_remove("DBUS_SESSION_BUS_ADDRESS");
        command
            .arg("--headless=new")
            .arg("--remote-debugging-pipe")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-background-networking")
            .arg("--disable-component-update")
            .arg("--disable-sync")
            .arg("--site-per-process")
            .arg("--window-size=1280,800")
            .arg(chromium_path_switch(
                "--user-data-dir",
                &directories.profile,
            ))
            .arg(chromium_path_switch("--disk-cache-dir", &directories.cache))
            .arg(chromium_path_switch("--load-extension", &extension))
            .arg(chromium_path_switch(
                "--disable-extensions-except",
                &extension,
            ))
            .env("TMPDIR", &directories.runtime)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let child_read_raw = child_read.as_raw_fd();
        let child_write_raw = child_write.as_raw_fd();
        // SAFETY: the pre-exec body only duplicates two owned pipe descriptors
        // into the descriptors Chromium reserves for --remote-debugging-pipe.
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(child_read_raw, 3) == -1 {
                    return Err(io::Error::last_os_error());
                }
                if libc::dup2(child_write_raw, 4) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command.process_group(0);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = fs::remove_dir_all(&extension);
                return Err(format!("could not start {}: {error}", executable.display()));
            }
        };
        drop(child_read);
        drop(child_write);

        let output = match crate::linux_process::OutputReaders::start(worker, &mut child) {
            Ok(readers) => readers,
            Err(error) => {
                kill_browser_process_group(&child);
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_dir_all(&extension);
                return Err(format!("could not collect Chromium diagnostics: {error}"));
            }
        };
        self.browser_epoch = self.browser_epoch.wrapping_add(1).max(1);
        let epoch = self.browser_epoch;
        let reader_sender = self.worker.clone();
        let reader = match crate::spawn_at_priority(
            "bt-linux-cdp-reader",
            ThreadPriority::BelowNormal,
            move |ctx| protocol_reader(ctx, browser_read, reader_sender, epoch),
        ) {
            Ok(reader) => reader,
            Err(error) => {
                kill_browser_process_group(&child);
                crate::linux_process::kill_reap_and_join(worker, &mut child, output);
                let _ = fs::remove_dir_all(&extension);
                return Err(format!("could not start Chromium's CDP reader: {error}"));
            }
        };
        self.cdp = Some(CdpWriter {
            file: browser_write,
            next_id: 1,
            poisoned: false,
            #[cfg(test)]
            write_blocked: None,
        });
        self.browser = Some(BrowserProcess {
            child,
            profile: directories.profile,
            extension,
            _reader: Some(reader),
            output: Some(output),
        });

        trace_browser_startup(self.worker_ctx, "browser.getVersion enter");
        let version = self.call(None, "Browser.getVersion", serde_json::json!({}))?;
        trace_browser_startup(self.worker_ctx, "browser.getVersion reply");
        let product = version
            .get("product")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if !product.starts_with("Chrome/") && !product.starts_with("Chromium/") {
            self.close_browser(worker);
            return Err(format!(
                "the Linux preview needs full Chromium with extension support; {product} does not provide it"
            ));
        }
        self.initialize_policy_extension(worker)?;
        Ok(())
    }

    fn initialize_policy_extension(&mut self, worker: &WorkerCtx) -> Result<(), String> {
        trace_browser_startup(self.worker_ctx, "policy target discovery enter");
        self.call(
            None,
            "Target.setDiscoverTargets",
            serde_json::json!({"discover": true}),
        )?;
        trace_browser_startup(self.worker_ctx, "policy target discovery reply");
        self.extension_id = Some(PRIVATE_EXTENSION_ID.to_owned());
        let extension_url = format!("chrome-extension://{PRIVATE_EXTENSION_ID}/control.html");
        trace_browser_startup(self.worker_ctx, "policy page create enter");
        let control_target = self
            .call(
                None,
                "Target.createTarget",
                serde_json::json!({"url": extension_url}),
            )?
            .get("targetId")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| String::from("Chromium did not create the policy control page"))?
            .to_owned();
        trace_browser_startup(self.worker_ctx, "policy page create reply");
        self.extension_target = Some(control_target.clone());
        self.extension_attach_pending = true;
        self.extension_attach_event = None;
        trace_browser_startup(self.worker_ctx, "policy page attach enter");
        let attached = self.call(
            None,
            "Target.attachToTarget",
            serde_json::json!({"targetId": control_target, "flatten": true}),
        )?;
        trace_browser_startup(self.worker_ctx, "policy page attach reply");
        self.extension_attach_pending = false;
        let control_session = attached
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| String::from("Chromium did not attach the policy control page"))?
            .to_owned();
        if let Some(event_session) = self.extension_attach_event.take()
            && event_session != control_session
        {
            let _ = self.notify(
                None,
                "Target.detachFromTarget",
                serde_json::json!({"sessionId": event_session}),
            );
            return Err(String::from(
                "Chromium attached a different session while opening the policy control page",
            ));
        }
        self.extension_session = Some(control_session.clone());
        trace_browser_startup(self.worker_ctx, "policy page enable enter");
        self.call(Some(&control_session), "Page.enable", serde_json::json!({}))?;
        trace_browser_startup(self.worker_ctx, "policy page enable reply");
        trace_browser_startup(self.worker_ctx, "policy runtime enable enter");
        self.call(
            Some(&control_session),
            "Runtime.enable",
            serde_json::json!({}),
        )?;
        trace_browser_startup(self.worker_ctx, "policy runtime enable reply");
        trace_browser_startup(self.worker_ctx, "policy page load wait enter");
        self.wait_for_page_load(worker, &control_session)?;
        trace_browser_startup(self.worker_ctx, "policy page load wait reply");
        trace_browser_startup(self.worker_ctx, "policy ready evaluate enter");
        let ready = self.call(
            Some(&control_session),
            "Runtime.evaluate",
            serde_json::json!({
                "expression": "window.folioPolicyReady.then(() => ({ready: true, id: chrome.runtime.id}))",
                "awaitPromise": true,
                "returnByValue": true
            }),
        )?;
        trace_browser_startup(self.worker_ctx, "policy ready evaluate reply");
        let extension_ready = ready
            .pointer("/result/value/ready")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let extension_id = ready
            .pointer("/result/value/id")
            .and_then(serde_json::Value::as_str);
        if ready.get("exceptionDetails").is_some()
            || !extension_ready
            || extension_id != Some(PRIVATE_EXTENSION_ID)
        {
            return Err(format!(
                "the selected browser did not load Folio's private MV3 policy extension at {extension_url}; use a full Chromium build that supports unpacked MV3 extensions"
            ));
        }
        trace_browser_startup(self.worker_ctx, "policy extension ready");

        self.call(
            None,
            "Target.setAutoAttach",
            serde_json::json!({
                "autoAttach": true,
                "waitForDebuggerOnStart": true,
                "flatten": true,
                "filter": [{"type": "page", "exclude": false}]
            }),
        )?;
        let targets = self.call(None, "Target.getTargets", serde_json::json!({}))?;
        if let Some(targets) = targets
            .get("targetInfos")
            .and_then(serde_json::Value::as_array)
        {
            for target in targets {
                if target.get("type").and_then(serde_json::Value::as_str) == Some("page")
                    && target.get("targetId").and_then(serde_json::Value::as_str)
                        != self.extension_target.as_deref()
                    && target
                        .get("url")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|url| url.starts_with("chrome://newtab"))
                    && let Some(target_id) =
                        target.get("targetId").and_then(serde_json::Value::as_str)
                {
                    let _ = self.notify(
                        None,
                        "Target.closeTarget",
                        serde_json::json!({"targetId": target_id}),
                    );
                }
            }
        }
        self.handle_protocol_events();
        Ok(())
    }

    fn wait_for_page_load(&mut self, _worker: &WorkerCtx, session: &str) -> Result<(), String> {
        trace_browser_startup(self.worker_ctx, "policy document.readyState enter");
        let ready_state = self.call(
            Some(session),
            "Runtime.evaluate",
            serde_json::json!({
                "expression": "document.readyState",
                "returnByValue": true
            }),
        )?;
        if document_load_has_completed(&ready_state) || self.loaded_sessions.contains(session) {
            trace_browser_startup(self.worker_ctx, "policy document already loaded");
            return Ok(());
        }
        while !self.loaded_sessions.contains(session) {
            match self.input.recv_timeout(CDP_POLL) {
                Ok(ActorInput::Protocol { epoch, message }) => {
                    if epoch != self.browser_epoch {
                        continue;
                    }
                    if message
                        .get("id")
                        .and_then(serde_json::Value::as_u64)
                        .is_some()
                    {
                        self.handle_protocol_response(message);
                    } else {
                        self.handle_protocol_event(message);
                    }
                }
                Ok(ActorInput::ProtocolClosed { epoch, reason }) => {
                    if epoch == self.browser_epoch {
                        return Err(reason);
                    }
                }
                Ok(ActorInput::Command(ActorCommand::Shutdown)) => {
                    consume_actor_cancellation();
                    self.running = false;
                    return Err(String::from("the Chromium actor is shutting down"));
                }
                Ok(ActorInput::Command(ActorCommand::Close { host })) => {
                    consume_actor_cancellation();
                    self.cancelled_hosts.insert(host);
                    self.deferred.push_front(ActorCommand::Close { host });
                    if !self.hosts.is_empty()
                        && self
                            .hosts
                            .keys()
                            .all(|host| self.cancelled_hosts.contains(host))
                    {
                        return Err(String::from(
                            "the web seat closed while the policy page loaded",
                        ));
                    }
                }
                Ok(ActorInput::Command(command)) => {
                    if interrupts_cdp_write(&command) {
                        consume_actor_cancellation();
                    }
                    self.deferred.push_back(command);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(browser) = self.browser.as_mut()
                        && let Some(status) = browser
                            .child
                            .try_wait()
                            .map_err(|error| error.to_string())?
                    {
                        return Err(format!("Chromium exited with {status}"));
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(String::from("the Chromium actor channel closed"));
                }
            }
        }
        Ok(())
    }

    fn close_browser(&mut self, worker: &WorkerCtx) {
        if self.browser.is_none() {
            return;
        }
        self.finish_browser(worker, false);
    }

    fn reader_closed(&mut self, worker: &WorkerCtx, epoch: u64, reason: String) {
        if epoch == self.browser_epoch {
            self.browser_failed(worker, reason);
        }
    }

    fn browser_failed(&mut self, worker: &WorkerCtx, reason: String) {
        if self.browser.is_none() {
            return;
        }
        self.finish_browser(worker, true);
        for page in self.hosts.values_mut() {
            let generation = page.generation;
            page.target = None;
            page.session = None;
            page.main_frame = None;
            page.tab_id = None;
            page.installed = false;
            page.post(ActorNotice::Event {
                generation,
                event: WebEvent::ProcessFailed {
                    kind: 0,
                    description: reason.clone(),
                },
            });
            page.post(ActorNotice::Event {
                generation,
                event: WebEvent::BrowserProcessExited { kind: 0 },
            });
        }
    }

    fn finish_browser(&mut self, worker: &WorkerCtx, force: bool) {
        let Some(mut browser) = self.browser.take() else {
            self.cdp = None;
            self.cancelled_target_create_response = None;
            return;
        };
        let mut force = force
            || ACTOR_SHUTDOWN_REQUESTED.load(Ordering::SeqCst)
            || self.cdp.as_ref().is_some_and(|cdp| cdp.poisoned);
        if !force {
            if let Some(cdp) = self.cdp.as_mut()
                && (cdp
                    .notify(None, "Browser.close", serde_json::json!({}))
                    .is_err()
                    || cdp.poisoned)
            {
                force = true;
            }
            while !force {
                match browser.child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) => {}
                    Err(_) => {
                        force = true;
                        break;
                    }
                }
                if ACTOR_SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
                    force = true;
                    break;
                }
                match self.input.recv_timeout(CDP_POLL) {
                    Ok(ActorInput::Command(ActorCommand::Shutdown)) => {
                        consume_actor_cancellation();
                        ACTOR_SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
                        self.running = false;
                        force = true;
                        break;
                    }
                    Ok(ActorInput::Command(command)) => {
                        if interrupts_cdp_write(&command) {
                            consume_actor_cancellation();
                        }
                        self.deferred.push_back(command);
                    }
                    Ok(ActorInput::Protocol { .. }) | Ok(ActorInput::ProtocolClosed { .. }) => {}
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        force = true;
                        break;
                    }
                }
            }
        }
        if force {
            kill_browser_process_group(&browser.child);
            if let Some(output) = browser.output.take() {
                crate::linux_process::kill_reap_and_join(worker, &mut browser.child, output);
            } else {
                let _ = browser.child.kill();
                let _ = browser.child.wait();
            }
        } else {
            let _ = browser.child.wait();
            if let Some(output) = browser.output.take() {
                let _ = output.finish(worker);
            }
        }
        if let Some(reader) = browser._reader.take() {
            let _ = reader.join();
        }
        self.browser_epoch = self.browser_epoch.wrapping_add(1).max(1);
        self.cdp = None;
        self.extension_target = None;
        self.extension_session = None;
        self.extension_attach_pending = false;
        self.extension_attach_event = None;
        self.protocol_events.clear();
        self.protocol_responses.clear();
        self.awaited_responses.clear();
        self.pending_worker_setups.clear();
        self.pending_worker_responses.clear();
        self.loaded_sessions.clear();
        self.active_boot = None;
        self.pending_page_create = None;
        self.pending_controller_setup = None;
        self.cancelled_target_create_response = None;
        self.pending_create.clear();
        self.pending_install.clear();
        self.pending_fetches.clear();
        self.sessions.clear();
        self.session_targets.clear();
        self.target_sessions.clear();
        self.targets.clear();
        self.contexts.clear();
        self.context_worlds.clear();
        self.frame_hosts.clear();
        self.frame_parents.clear();
        self.frame_sessions.clear();
        self.frame_urls.clear();
        self.session_root_frames.clear();
        self.service_worker_targets.clear();
        self.service_worker_clients.clear();
        for page in self.hosts.values_mut() {
            page.target = None;
            page.session = None;
            page.tab_id = None;
            page.main_frame = None;
            page.main_context = None;
            page.policy_token = None;
            page.navigation_binding = None;
            page.navigation_permit = None;
            page.latest_navigation_request = None;
            page.ime_binding = None;
            page.ime_token = None;
            page.last_ime_cursor = None;
            page.screencast_started = false;
            page.screencast_frame_reported = false;
            page.installed = false;
        }
        let _ = fs::remove_dir_all(browser.extension);
    }

    fn handle_protocol_events(&mut self) {
        while self.running {
            let Some(message) = self.protocol_events.pop_front() else {
                break;
            };
            self.handle_protocol_event(message);
        }
    }

    fn handle_protocol_event(&mut self, message: serde_json::Value) {
        let Some(method) = message.get("method").and_then(serde_json::Value::as_str) else {
            return;
        };
        match method {
            "Target.attachedToTarget" => self.attached_to_target(message),
            "Target.detachedFromTarget" => {
                self.trace_pdf_target_detached(&message);
                self.detached_from_target(message);
            }
            "Runtime.executionContextCreated" => self.execution_context_created(message),
            "Runtime.executionContextDestroyed" => self.execution_context_destroyed(message),
            "Runtime.executionContextsCleared" => self.execution_contexts_cleared(message),
            "Runtime.bindingCalled" => self.binding_called(message),
            "ServiceWorker.workerVersionUpdated" => self.service_worker_version_updated(message),
            "Fetch.requestPaused" => self.request_paused(message),
            "Page.screencastFrame" => self.screencast_frame(message),
            "Page.frameAttached" => {
                if let Some(params) = message.get("params") {
                    self.trace_pdf_frame_event("Page.frameAttached", &message, params);
                }
                self.frame_attached(message);
            }
            "Page.frameDetached" => {
                if let Some(params) = message.get("params") {
                    self.trace_pdf_frame_event("Page.frameDetached", &message, params);
                }
                self.frame_detached(message);
            }
            "Page.frameNavigated" => {
                if let Some(frame) = message.pointer("/params/frame") {
                    self.trace_pdf_frame_event("Page.frameNavigated", &message, frame);
                }
                self.frame_navigated(message);
            }
            "Page.windowOpen" => self.window_open(message),
            "Page.navigatedWithinDocument" => self.navigated_within_document(message),
            "Page.loadEventFired" => {
                if let Some(session) = message.get("sessionId").and_then(serde_json::Value::as_str)
                {
                    self.loaded_sessions.insert(session.to_owned());
                    if self.extension_session.as_deref() == Some(session) {
                        trace_browser_startup(self.worker_ctx, "policy loadEventFired");
                    }
                }
                self.load_event_fired(message);
            }
            "Page.javascriptDialogOpening" => self.javascript_dialog_opening(message),
            "Network.responseReceived" => self.response_received(message),
            "Network.loadingFailed" => self.loading_failed(message),
            _ => {}
        }
    }

    fn attached_to_target(&mut self, message: serde_json::Value) {
        let child_session = message
            .pointer("/params/sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let Some(info) = message.pointer("/params/targetInfo").cloned() else {
            return;
        };
        let target_id = info
            .get("targetId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let target_type = info
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let waiting = message
            .pointer("/params/waitingForDebugger")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if target_id == self.extension_target.as_deref().unwrap_or_default() {
            trace_browser_startup(self.worker_ctx, "policy target attach event");
            if waiting {
                let _ = self.notify(
                    Some(&child_session),
                    "Runtime.runIfWaitingForDebugger",
                    serde_json::json!({}),
                );
            }
            if keep_policy_page_attachment(
                self.extension_attach_pending,
                self.extension_session.as_deref(),
                &child_session,
            ) {
                if !self.extension_attach_pending
                    || self.extension_attach_event.as_deref() == Some(child_session.as_str())
                {
                    return;
                }
                if self.extension_attach_event.is_none() {
                    self.extension_attach_event = Some(child_session);
                    return;
                }
            }
            let _ = self.notify(
                None,
                "Target.detachFromTarget",
                serde_json::json!({"sessionId": child_session}),
            );
            return;
        }

        let parent_session = message
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let parent_owner = self.sessions.get(parent_session).map(|(host, _)| *host);
        let opener_owner = info
            .get("openerId")
            .and_then(serde_json::Value::as_str)
            .and_then(|target| self.targets.get(target).copied());
        let owner = parent_owner.or(opener_owner);

        if target_type == "page" {
            if let Some((host, generation)) = self.pending_create.remove(&target_id) {
                let live = !self.cancelled_hosts.contains(&host)
                    && self
                        .hosts
                        .get(&host)
                        .is_some_and(|page| page.generation == generation);
                if !live {
                    let _ = self.notify(
                        None,
                        "Target.closeTarget",
                        serde_json::json!({"targetId": target_id}),
                    );
                    return;
                }
                self.sessions.insert(child_session.clone(), (host, true));
                self.targets.insert(target_id.clone(), host);
                self.target_sessions
                    .insert(target_id.clone(), child_session.clone());
                self.session_targets
                    .insert(child_session.clone(), target_id.clone());
                if let Some(page) = self.hosts.get_mut(&host) {
                    page.target = Some(target_id.clone());
                    page.session = Some(child_session.clone());
                }
                if let Err(error) =
                    self.configure_page(host, &target_id, &child_session, generation, waiting)
                {
                    self.post(
                        host,
                        ActorNotice::Controller {
                            generation,
                            error: Some(error),
                        },
                    );
                    let _ = self.notify(
                        None,
                        "Target.closeTarget",
                        serde_json::json!({"targetId": target_id}),
                    );
                }
                return;
            }

            if self.target_sessions.contains_key(&target_id) {
                if waiting {
                    let _ = self.notify(
                        Some(&child_session),
                        "Runtime.runIfWaitingForDebugger",
                        serde_json::json!({}),
                    );
                }
                let _ = self.notify(
                    None,
                    "Target.detachFromTarget",
                    serde_json::json!({"sessionId": child_session}),
                );
                return;
            }

            let _ = self.notify(
                None,
                "Target.closeTarget",
                serde_json::json!({"targetId": target_id}),
            );
            return;
        }

        let is_policy_worker = target_type == "service_worker"
            && info
                .get("url")
                .and_then(serde_json::Value::as_str)
                .zip(self.extension_id.as_deref())
                .is_some_and(|(url, id)| url == format!("chrome-extension://{id}/background.js"));
        if is_policy_worker {
            if waiting {
                let _ = self.notify(
                    Some(&child_session),
                    "Runtime.runIfWaitingForDebugger",
                    serde_json::json!({}),
                );
            }
            return;
        }

        let host = owner.unwrap_or(0);
        self.sessions.insert(child_session.clone(), (host, false));
        self.targets.insert(target_id.clone(), host);
        self.target_sessions
            .insert(target_id.clone(), child_session.clone());
        self.session_targets
            .insert(child_session.clone(), target_id.clone());
        if target_type == "service_worker" {
            self.service_worker_targets.insert(target_id.clone());
            self.service_worker_clients
                .entry(target_id.clone())
                .or_insert(None);
        }
        if let Err(error) =
            self.configure_child(&child_session, &target_id, &target_type, host, waiting)
        {
            if host != 0
                && let Some(page) = self.hosts.get(&host)
            {
                page.post(ActorNotice::Event {
                    generation: page.generation,
                    event: WebEvent::ProcessFailed {
                        kind: 1,
                        description: format!("the browser worker could not be guarded: {error}"),
                    },
                });
            }
            let _ = self.notify(
                None,
                "Target.closeTarget",
                serde_json::json!({"targetId": target_id}),
            );
        }
        let workers = self
            .service_worker_clients
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for worker in workers {
            self.dispatch_service_worker_requests(&worker);
        }
    }

    fn configure_child(
        &mut self,
        session: &str,
        target: &str,
        target_type: &str,
        host: HostId,
        waiting: bool,
    ) -> Result<(), String> {
        if matches!(target_type, "worker" | "shared_worker" | "service_worker") {
            let fetch_id = self.send_command(
                Some(session),
                "Fetch.enable",
                serde_json::json!({
                    "patterns": [{"urlPattern": "*", "requestStage": "Request"}]
                }),
            )?;
            let auto_attach_id = self.send_command(
                Some(session),
                "Target.setAutoAttach",
                serde_json::json!({
                    "autoAttach": true,
                    "waitForDebuggerOnStart": true,
                    "flatten": true,
                    "filter": [
                        {"type": "iframe", "exclude": false},
                        {"type": "worker", "exclude": false},
                        {"type": "shared_worker", "exclude": false},
                        {"type": "page", "exclude": false},
                        {"type": "service_worker", "exclude": false}
                    ]
                }),
            )?;
            let pending = HashSet::from([fetch_id, auto_attach_id]);
            self.pending_worker_setups.insert(
                session.to_owned(),
                PendingWorkerSetup {
                    host,
                    target: target.to_owned(),
                    generation: self.hosts.get(&host).map_or(0, |page| page.generation),
                    pending,
                },
            );
            self.pending_worker_responses
                .insert(fetch_id, (session.to_owned(), "Fetch.enable"));
            self.pending_worker_responses
                .insert(auto_attach_id, (session.to_owned(), "Target.setAutoAttach"));
            if waiting {
                self.notify(
                    Some(session),
                    "Runtime.runIfWaitingForDebugger",
                    serde_json::json!({}),
                )?;
            }
            return Ok(());
        }
        self.call(Some(session), "Runtime.enable", serde_json::json!({}))?;
        self.call(Some(session), "Network.enable", serde_json::json!({}))?;
        self.call(
            Some(session),
            "Fetch.enable",
            serde_json::json!({
                "patterns": [{"urlPattern": "*", "requestStage": "Request"}]
            }),
        )?;
        self.call(
            Some(session),
            "Target.setAutoAttach",
            serde_json::json!({
                "autoAttach": true,
                "waitForDebuggerOnStart": true,
                "flatten": true,
                "filter": [
                    {"type": "iframe", "exclude": false},
                    {"type": "worker", "exclude": false},
                    {"type": "shared_worker", "exclude": false},
                    {"type": "page", "exclude": false},
                    {"type": "service_worker", "exclude": false}
                ]
            }),
        )?;
        if target_type == "iframe" {
            self.call(Some(session), "Page.enable", serde_json::json!({}))?;
            self.call(Some(session), "DOM.enable", serde_json::json!({}))?;
            self.call(Some(session), "ServiceWorker.enable", serde_json::json!({}))?;
            if host != 0 {
                self.install_ime_signals(host, session)?;
            }
            if waiting {
                self.call(
                    Some(session),
                    "Runtime.runIfWaitingForDebugger",
                    serde_json::json!({}),
                )?;
            }
            if host != 0 {
                let tree = self.call(Some(session), "Page.getFrameTree", serde_json::json!({}))?;
                if let Some(tree) = tree.get("frameTree") {
                    if let Some(root) = tree
                        .pointer("/frame/id")
                        .and_then(serde_json::Value::as_str)
                    {
                        self.session_root_frames
                            .insert(session.to_owned(), root.to_owned());
                    }
                    index_frame_tree(
                        tree,
                        host,
                        &mut self.frame_hosts,
                        &mut self.frame_parents,
                        &mut self.frame_sessions,
                        &mut self.frame_urls,
                        session,
                    );
                }
                if let Some(frame) = tree
                    .pointer("/frameTree/frame/id")
                    .and_then(serde_json::Value::as_str)
                {
                    self.frame_hosts.insert(frame.to_owned(), host);
                }
            }
        }
        if waiting && target_type != "iframe" {
            self.call(
                Some(session),
                "Runtime.runIfWaitingForDebugger",
                serde_json::json!({}),
            )?;
        }
        let _ = target;
        Ok(())
    }

    fn install_ime_signals(&mut self, host: HostId, session: &str) -> Result<(), String> {
        let page = self
            .hosts
            .get(&host)
            .ok_or_else(|| String::from("the web seat closed before IME signals were installed"))?;
        let (Some(binding), Some(token)) = (&page.ime_binding, &page.ime_token) else {
            return Err(String::from("the web seat has no IME signal identity"));
        };
        let binding = binding.clone();
        let token = token.clone();
        self.call(
            Some(session),
            "Runtime.addBinding",
            serde_json::json!({
                "name": binding,
                "executionContextName": IME_WORLD_NAME
            }),
        )?;
        let source = ime_cursor_bootstrap(&binding, &token);
        self.call(
            Some(session),
            "Page.addScriptToEvaluateOnNewDocument",
            serde_json::json!({
                "source": source,
                "worldName": IME_WORLD_NAME,
                "runImmediately": true
            }),
        )?;
        Ok(())
    }

    fn refresh_ime_cursor(&mut self, host: HostId, preferred: Option<(String, String, i32)>) {
        let Some((page_visual, generation, rasterization_scale, main_frame, main_session, visible)) =
            self.hosts.get(&host).and_then(|page| {
                Some((
                    page.page?,
                    page.generation,
                    page.scale,
                    page.main_frame.clone()?,
                    page.session.clone()?,
                    page.visible,
                ))
            })
        else {
            return;
        };
        let contexts = if !visible {
            Vec::new()
        } else if let Some((session, frame, context)) = preferred {
            vec![(session, frame, context)]
        } else {
            self.contexts
                .iter()
                .filter_map(|((session, context), (owner, frame))| {
                    if *owner == host
                        && !frame.is_empty()
                        && self
                            .context_worlds
                            .get(&(session.clone(), *context))
                            .is_some_and(|world| world.name == IME_WORLD_NAME)
                    {
                        Some((session.clone(), frame.clone(), *context))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };
        let mut rect = None;
        for (session, frame, context) in contexts {
            let probe = match self.call(
                Some(&session),
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": IME_CARET_PROBE,
                    "contextId": context,
                    "returnByValue": true
                }),
            ) {
                Ok(result) => result,
                Err(_) => continue,
            };
            if probe.get("exceptionDetails").is_some() {
                continue;
            }
            let Some(encoded) = probe
                .pointer("/result/value")
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let Ok(probe) = serde_json::from_str::<serde_json::Value>(encoded) else {
                continue;
            };
            let Some(kind) = probe.get("kind").and_then(serde_json::Value::as_str) else {
                continue;
            };
            if kind == "none" {
                continue;
            }
            let Some(device_scale) = probe.get("scale").and_then(serde_json::Value::as_f64) else {
                continue;
            };
            if (device_scale - rasterization_scale).abs() > 0.01 {
                continue;
            }
            let css_rect = match kind {
                "range" => probe
                    .get("rect")
                    .and_then(json_rect)
                    .map(|[x, y, width, height]| (x, y, width, height)),
                "input" | "textarea" => self.ime_text_control_rect(&session, context, kind, &probe),
                _ => None,
            };
            let Some((left, top, width, height)) = css_rect else {
                continue;
            };
            let Some(viewport_width) = probe.get("innerWidth").and_then(serde_json::Value::as_f64)
            else {
                continue;
            };
            let Some(viewport_height) =
                probe.get("innerHeight").and_then(serde_json::Value::as_f64)
            else {
                continue;
            };
            let local_rect = [left, top, left + width, top + height];
            let Some(css_rect) = self.map_ime_rect_to_main(
                host,
                &main_frame,
                &frame,
                local_rect,
                (viewport_width, viewport_height),
            ) else {
                continue;
            };
            let Some((offset_x, offset_y, visual_scale)) = self.ime_visual_viewport(&main_session)
            else {
                continue;
            };
            let scale = rasterization_scale * visual_scale;
            rect = Some([
                (css_rect[0] - offset_x) * scale,
                (css_rect[1] - offset_y) * scale,
                (css_rect[2] - offset_x) * scale,
                (css_rect[3] - offset_y) * scale,
            ]);
            break;
        }

        let Some(page) = self.hosts.get_mut(&host) else {
            return;
        };
        if page.last_ime_cursor == Some(rect) {
            return;
        }
        page.last_ime_cursor = Some(rect);
        page.post(ActorNotice::Event {
            generation,
            event: WebEvent::ImeCursorChanged {
                page: page_visual,
                generation,
                rect,
                rasterization_scale,
            },
        });
    }

    fn map_ime_rect_to_main(
        &mut self,
        host: HostId,
        main_frame: &str,
        frame: &str,
        mut rect: [f64; 4],
        mut viewport: (f64, f64),
    ) -> Option<[f64; 4]> {
        let mut current = frame.to_owned();
        let mut visited = HashSet::new();
        loop {
            if current == main_frame {
                return Some(rect);
            }
            if !visited.insert(current.clone()) || self.frame_hosts.get(&current) != Some(&host) {
                return None;
            }
            let parent = self.frame_parents.get(&current)?.clone();
            let parent_session = self.frame_sessions.get(&parent)?.clone();
            if self.frame_hosts.get(&parent) != Some(&host) {
                return None;
            }
            let owner = self
                .call(
                    Some(&parent_session),
                    "DOM.getFrameOwner",
                    serde_json::json!({"frameId": current}),
                )
                .ok()?;
            let backend_node = owner
                .get("backendNodeId")
                .and_then(serde_json::Value::as_i64)?;
            let model = self
                .call(
                    Some(&parent_session),
                    "DOM.getBoxModel",
                    serde_json::json!({"backendNodeId": backend_node}),
                )
                .ok()?;
            let quad = json_quad(model.pointer("/model/content")?)?;
            rect = map_rect_through_quad(rect, viewport, quad)?;

            current = self.session_root_frames.get(&parent_session)?.clone();
            if current == main_frame {
                return Some(rect);
            }
            let metrics = self
                .call(
                    Some(&parent_session),
                    "Page.getLayoutMetrics",
                    serde_json::json!({}),
                )
                .ok()?;
            viewport = (
                metrics
                    .pointer("/cssLayoutViewport/clientWidth")
                    .and_then(serde_json::Value::as_f64)?,
                metrics
                    .pointer("/cssLayoutViewport/clientHeight")
                    .and_then(serde_json::Value::as_f64)?,
            );
        }
    }

    fn ime_visual_viewport(&mut self, session: &str) -> Option<(f64, f64, f64)> {
        let metrics = self
            .call(
                Some(session),
                "Page.getLayoutMetrics",
                serde_json::json!({}),
            )
            .ok()?;
        let viewport = metrics.get("cssVisualViewport")?;
        Some((
            viewport
                .get("offsetX")
                .and_then(serde_json::Value::as_f64)?,
            viewport
                .get("offsetY")
                .and_then(serde_json::Value::as_f64)?,
            viewport.get("scale").and_then(serde_json::Value::as_f64)?,
        ))
    }

    fn ime_text_control_rect(
        &mut self,
        session: &str,
        context: i32,
        kind: &str,
        probe: &serde_json::Value,
    ) -> Option<(f64, f64, f64, f64)> {
        let value = probe.get("value")?.as_str()?;
        let offset = probe
            .get("offset")
            .and_then(serde_json::Value::as_u64)
            .and_then(|offset| u32::try_from(offset).ok())?;
        let input_type = probe
            .get("inputType")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let element = self
            .call(
                Some(session),
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": "document.activeElement",
                    "contextId": context,
                    "returnByValue": false
                }),
            )
            .ok()?;
        let object_id = element
            .pointer("/result/objectId")
            .and_then(serde_json::Value::as_str)?;
        let description = self
            .call(
                Some(session),
                "DOM.describeNode",
                serde_json::json!({
                    "objectId": object_id,
                    "depth": -1,
                    "pierce": true
                }),
            )
            .ok()?;
        let element = description.get("node")?;
        let position = if kind == "input" {
            input_text_backend_node(element, value, offset, input_type == "password")
        } else {
            textarea_text_backend_node(element, value, offset)
        }?;
        let node = self
            .call(
                Some(session),
                "DOM.resolveNode",
                serde_json::json!({
                    "backendNodeId": position.0,
                    "executionContextId": context
                }),
            )
            .ok()?;
        let node_object = node
            .pointer("/object/objectId")
            .and_then(serde_json::Value::as_str)?;
        let range = self
            .call(
                Some(session),
                "Runtime.callFunctionOn",
                serde_json::json!({
                    "objectId": node_object,
                    "functionDeclaration": "function(offset){const range=document.createRange();range.setStart(this,offset);range.collapse(true);const rect=range.getBoundingClientRect();return JSON.stringify([rect.x,rect.y,rect.width,rect.height]);}",
                    "arguments": [{"value": position.1}],
                    "returnByValue": true
                }),
            )
            .ok()?;
        if range.get("exceptionDetails").is_some() {
            return None;
        }
        let encoded = range
            .pointer("/result/value")
            .and_then(serde_json::Value::as_str)?;
        let rect = serde_json::from_str::<serde_json::Value>(encoded).ok()?;
        let rect = json_rect(&rect)?;
        Some((rect[0], rect[1], rect[2], rect[3]))
    }

    fn detached_from_target(&mut self, message: serde_json::Value) {
        if let Some(session) = message
            .pointer("/params/sessionId")
            .and_then(serde_json::Value::as_str)
        {
            self.discard_worker_setup(session);
            if let Some((host, true)) = self.sessions.get(session).copied()
                && let Some(page) = self.hosts.get_mut(&host)
                && page.session.as_deref() == Some(session)
            {
                page.session = None;
                page.main_context = None;
                page.latest_navigation_request = None;
            }
            let target = self.session_targets.remove(session).unwrap_or_default();
            if !target.is_empty() {
                self.target_sessions.remove(&target);
                self.service_worker_targets.remove(&target);
                self.service_worker_clients.remove(&target);
            }
            self.sessions.remove(session);
            self.contexts
                .retain(|(context_session, _), _| context_session != session);
            self.context_worlds
                .retain(|(context_session, _), _| context_session != session);
            self.frame_urls.retain(|frame, _| {
                self.frame_sessions
                    .get(frame)
                    .is_some_and(|owner_session| owner_session != session)
            });
            self.frame_sessions
                .retain(|_, owner_session| owner_session != session);
            self.session_root_frames.remove(session);
        }
    }

    fn service_worker_version_updated(&mut self, message: serde_json::Value) {
        let Some(versions) = message
            .pointer("/params/versions")
            .and_then(serde_json::Value::as_array)
        else {
            return;
        };
        let mut workers = Vec::new();
        for version in versions {
            let Some(target) = version.get("targetId").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let clients = version
                .get("controlledClients")
                .and_then(serde_json::Value::as_array)
                .map(|clients| {
                    clients
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_owned)
                        .collect::<HashSet<_>>()
                });
            self.service_worker_clients
                .insert(target.to_owned(), clients);
            workers.push(target.to_owned());
        }
        for target in workers {
            self.dispatch_service_worker_requests(&target);
        }
    }

    fn service_worker_hosts(&self, target: &str) -> Option<HashMap<HostId, u64>> {
        let mut hosts = HashMap::new();
        match self.service_worker_clients.get(target) {
            Some(Some(clients)) if !clients.is_empty() => {
                for client in clients {
                    let host = self.targets.get(client).copied()?;
                    if host != 0
                        && let Some(page) = self.hosts.get(&host)
                    {
                        hosts.insert(host, page.generation);
                    }
                }
            }
            Some(Some(_)) | Some(None) | None => {
                let session = self.target_sessions.get(target)?;
                let (host, _) = self.sessions.get(session)?;
                let page = self.hosts.get(host)?;
                hosts.insert(*host, page.generation);
            }
        }
        Some(hosts)
    }

    fn dispatch_service_worker_requests(&mut self, target: &str) {
        let requests = self
            .pending_fetches
            .iter()
            .filter(|(_, pending)| {
                pending.service_worker.as_deref() == Some(target) && pending.hosts.is_empty()
            })
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in requests {
            let Some(hosts) = self.service_worker_hosts(target) else {
                continue;
            };
            if hosts.is_empty() {
                self.fail_fetch(&key, "BlockedByClient");
                continue;
            }
            let Some(pending) = self.pending_fetches.get_mut(&key) else {
                continue;
            };
            pending.hosts.clone_from(&hosts);
            let uri = pending.uri.clone();
            for (host, generation) in hosts {
                self.post(
                    host,
                    ActorNotice::ResourceRequest {
                        key: key.clone(),
                        generation,
                        uri: uri.clone(),
                    },
                );
            }
        }
    }

    fn execution_context_created(&mut self, message: serde_json::Value) {
        let session = message
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let Some((host, root)) = self.sessions.get(&session).copied() else {
            return;
        };
        let Some(context) = message.pointer("/params/context") else {
            return;
        };
        let Some(id) = context.get("id").and_then(serde_json::Value::as_i64) else {
            return;
        };
        let frame = context
            .pointer("/auxData/frameId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        self.contexts
            .insert((session.clone(), id as i32), (host, frame.clone()));
        self.context_worlds.insert(
            (session.clone(), id as i32),
            ContextWorld {
                name: context
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                is_default: context
                    .pointer("/auxData/isDefault")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
            },
        );
        self.frame_hosts.insert(frame.clone(), host);
        self.frame_sessions.insert(frame.clone(), session.clone());
        let is_default = self
            .context_worlds
            .get(&(session.clone(), id as i32))
            .is_some_and(|world| world.is_default);
        if root
            && is_default
            && self
                .hosts
                .get(&host)
                .is_some_and(|page| page.main_frame.as_deref() == Some(frame.as_str()))
        {
            self.reconcile_main_context(host, &session, &frame);
        }
    }

    fn reconcile_main_context(&mut self, host: HostId, session: &str, frame: &str) {
        if !self
            .sessions
            .get(session)
            .is_some_and(|(owner, root)| *owner == host && *root)
        {
            return;
        }
        let context =
            default_context_for_frame(&self.contexts, &self.context_worlds, session, host, frame);
        let Some(page) = self.hosts.get_mut(&host) else {
            return;
        };
        if page.session.as_deref() != Some(session) || page.main_frame.as_deref() != Some(frame) {
            return;
        }
        if page.main_context != context {
            page.latest_navigation_request = None;
        }
        page.main_context = context;
        trace_browser_startup(
            self.worker_ctx,
            &format!(
                "main-frame default context reconciled generation={} context={context:?}",
                page.generation
            ),
        );
    }

    fn execution_context_destroyed(&mut self, message: serde_json::Value) {
        let session = message
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let context = message
            .pointer("/params/executionContextId")
            .and_then(serde_json::Value::as_i64)
            .map(|id| id as i32);
        if let Some(context) = context {
            let key = (session.to_owned(), context);
            let context_frame = self.contexts.get(&key).cloned();
            self.contexts.remove(&key);
            self.context_worlds.remove(&key);
            let mut reconcile = None;
            if let Some((host, true)) = self.sessions.get(session).copied()
                && let Some(page) = self.hosts.get_mut(&host)
                && page.session.as_deref() == Some(session)
                && context_frame.as_ref().is_some_and(|(owner, frame)| {
                    *owner == host && page.main_frame.as_deref() == Some(frame.as_str())
                })
            {
                if page.main_context == Some(context) {
                    page.main_context = None;
                    page.latest_navigation_request = None;
                }
                reconcile = page.main_frame.clone().map(|frame| (host, frame));
            }
            if let Some((host, frame)) = reconcile {
                self.reconcile_main_context(host, session, &frame);
            }
        }
    }

    fn execution_contexts_cleared(&mut self, message: serde_json::Value) {
        let session = message
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        self.contexts
            .retain(|(context_session, _), _| context_session != session);
        self.context_worlds
            .retain(|(context_session, _), _| context_session != session);
        if let Some((host, true)) = self.sessions.get(session).copied()
            && let Some(page) = self.hosts.get_mut(&host)
            && page.session.as_deref() == Some(session)
        {
            page.main_context = None;
            page.latest_navigation_request = None;
        }
    }

    fn binding_called(&mut self, message: serde_json::Value) {
        let session = message
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let Some(context) = message
            .pointer("/params/executionContextId")
            .and_then(serde_json::Value::as_i64)
            .map(|id| id as i32)
        else {
            return;
        };
        let Some(name) = message
            .pointer("/params/name")
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        let Some(payload) = message
            .pointer("/params/payload")
            .and_then(serde_json::Value::as_str)
            .and_then(|payload| serde_json::from_str::<serde_json::Value>(payload).ok())
        else {
            return;
        };
        if self.ime_binding_called(&session, context, name, &payload) {
            return;
        }
        let Some((owner, root)) = self.sessions.get(&session).copied() else {
            return;
        };
        if !root || owner == 0 {
            return;
        }
        let Some((context_owner, frame)) = self.contexts.get(&(session.clone(), context)) else {
            return;
        };
        let Some(page) = self.hosts.get(&owner) else {
            return;
        };
        if *context_owner != owner
            || page.session.as_deref() != Some(session.as_str())
            || page.main_context != Some(context)
            || page.navigation_binding.as_deref() != Some(name)
            || page.main_frame.as_deref() != Some(frame.as_str())
            || page.policy_token.as_deref()
                != payload.get("token").and_then(serde_json::Value::as_str)
        {
            return;
        }
        let (Some(url), Some(request), Some(cancelable)) = (
            payload.get("url").and_then(serde_json::Value::as_str),
            payload.get("id").and_then(serde_json::Value::as_u64),
            payload
                .get("cancelable")
                .and_then(serde_json::Value::as_bool),
        ) else {
            return;
        };
        let generation = page.generation;
        let token = page.policy_token.clone().unwrap_or_default();
        let key = GateKey::NavigationApi {
            session,
            context,
            request: request.to_string(),
            token,
            url: url.to_owned(),
            cancelable,
        };
        let Some(page) = self.hosts.get_mut(&owner) else {
            return;
        };
        page.latest_navigation_request = Some(key.clone());
        page.post(ActorNotice::NavigationRequest {
            key,
            generation,
            uri: url.to_owned(),
        });
    }

    fn ime_binding_called(
        &mut self,
        session: &str,
        context: i32,
        name: &str,
        payload: &serde_json::Value,
    ) -> bool {
        let Some((host, frame)) = self.contexts.get(&(session.to_owned(), context)) else {
            return false;
        };
        if self
            .context_worlds
            .get(&(session.to_owned(), context))
            .is_none_or(|world| world.name != IME_WORLD_NAME)
        {
            return false;
        }
        let Some(page) = self.hosts.get(host) else {
            return false;
        };
        if page.ime_binding.as_deref() != Some(name) {
            return false;
        }
        if page.ime_token.as_deref() != payload.get("token").and_then(serde_json::Value::as_str) {
            return true;
        }
        let host = *host;
        let frame = frame.clone();
        self.refresh_ime_cursor(host, Some((session.to_owned(), frame, context)));
        true
    }

    fn request_paused(&mut self, message: serde_json::Value) {
        let session = message
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let request_id = message
            .pointer("/params/requestId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let url = message
            .pointer("/params/request/url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let frame = message
            .pointer("/params/frameId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let resource_type = message
            .pointer("/params/resourceType")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let Some((host, root)) = self.sessions.get(&session).copied() else {
            let _ = self.notify(
                Some(&session),
                "Fetch.failRequest",
                serde_json::json!({"requestId": request_id, "errorReason": "BlockedByClient"}),
            );
            return;
        };
        let target = self.session_targets.get(&session).cloned();
        let service_worker = target
            .as_deref()
            .filter(|target| self.service_worker_targets.contains(*target))
            .map(str::to_owned);
        let key = GateKey::Fetch {
            session: session.clone(),
            request: request_id.clone(),
        };
        if let Some(target) = service_worker {
            self.pending_fetches.insert(
                key.clone(),
                PendingFetch {
                    session,
                    request: request_id,
                    service_worker: Some(target.clone()),
                    uri: url,
                    hosts: HashMap::new(),
                    votes: HashMap::new(),
                },
            );
            self.dispatch_service_worker_requests(&target);
            return;
        }
        if host == 0 {
            let _ = self.notify(
                Some(&session),
                "Fetch.failRequest",
                serde_json::json!({"requestId": request_id, "errorReason": "BlockedByClient"}),
            );
            return;
        }
        if self.pdf_viewer_chrome_resource(host, &session, frame, &url) {
            let _ = self.notify(
                Some(&session),
                "Fetch.continueRequest",
                serde_json::json!({"requestId": request_id}),
            );
            return;
        }
        let main = root
            && resource_type == "Document"
            && self
                .hosts
                .get(&host)
                .and_then(|page| page.main_frame.as_deref())
                == Some(frame);
        let generation = self.hosts.get(&host).map_or(0, |page| page.generation);
        if root && resource_type == "Document" {
            let scheme = url.split_once(':').map_or("unknown", |(scheme, _)| scheme);
            trace_browser_startup(
                self.worker_ctx,
                &format!(
                    "Fetch document paused generation={generation} scheme={scheme} main={main}"
                ),
            );
        }
        let hosts = HashMap::from([(host, generation)]);
        if main && let Some(page) = self.hosts.get_mut(&host) {
            page.latest_navigation_request = None;
        }
        self.pending_fetches.insert(
            key.clone(),
            PendingFetch {
                session,
                request: request_id,
                service_worker: None,
                uri: url.clone(),
                hosts,
                votes: HashMap::new(),
            },
        );
        if main {
            self.post(
                host,
                ActorNotice::NavigationRequest {
                    key,
                    generation,
                    uri: url,
                },
            );
        } else {
            self.post(
                host,
                ActorNotice::ResourceRequest {
                    key,
                    generation,
                    uri: url,
                },
            );
        }
    }

    fn pdf_viewer_chrome_resource(
        &self,
        host: HostId,
        session: &str,
        frame: &str,
        url: &str,
    ) -> bool {
        // Chrome's component PDF document imports its own UI through
        // chrome://resources. Keep the common resource rule closed to that
        // scheme; only this committed engine frame and its owning page pass.
        if !url.starts_with(CHROME_RESOURCES_PREFIX) {
            return false;
        }
        let Some(page) = self.hosts.get(&host) else {
            return false;
        };
        let main_frame = page.main_frame.as_deref();
        let main_session = page.session.as_deref();
        let main_frame_session_matches =
            main_frame
                .zip(main_session)
                .is_some_and(|(main_frame, main_session)| {
                    self.frame_sessions.get(main_frame).map(String::as_str) == Some(main_session)
                });
        let frame_owner_matches = self.frame_hosts.get(frame) == Some(&host);
        let main_owner_matches =
            main_frame.is_some_and(|main_frame| self.frame_hosts.get(main_frame) == Some(&host));
        let frame_session_matches =
            self.frame_sessions.get(frame).map(String::as_str) == Some(session);
        let frame_parent_matches = main_frame
            .zip(self.frame_parents.get(frame))
            .is_some_and(|(main_frame, parent)| parent == main_frame);
        let viewer_document_matches = self
            .frame_urls
            .get(frame)
            .is_some_and(|frame_url| frame_url == PDF_VIEWER_DOCUMENT_URL);
        let authorized = main_frame.is_some()
            && main_session.is_some()
            && main_frame_session_matches
            && is_owned_pdf_viewer_resource(
                host,
                session,
                frame,
                url,
                main_frame.unwrap_or_default(),
                &self.frame_hosts,
                &self.frame_sessions,
                &self.frame_parents,
                &self.frame_urls,
            );
        trace_browser_startup(
            self.worker_ctx,
            &format!(
                "PDF chrome resource owner request_session={session} target={:?} session_root={} request_frame={frame} session_root_frame={:?} current_document={:?} parent={:?} main_frame={main_frame:?} main_session={main_session:?} frame_owner={frame_owner_matches} main_owner={main_owner_matches} main_frame_session={main_frame_session_matches} frame_session={frame_session_matches} parent_match={frame_parent_matches} viewer_document={viewer_document_matches} allow={authorized}",
                self.session_targets.get(session),
                self.sessions.get(session).is_some_and(|(_, root)| *root),
                self.session_root_frames.get(session),
                self.frame_urls.get(frame).map(String::as_str),
                self.frame_parents.get(frame).map(String::as_str)
            ),
        );
        authorized
    }

    fn trace_pdf_frame_event(
        &self,
        method: &str,
        message: &serde_json::Value,
        frame: &serde_json::Value,
    ) {
        let session = message
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let frame_id = frame
            .get("id")
            .or_else(|| frame.get("frameId"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let event_parent = frame
            .get("parentId")
            .or_else(|| frame.get("parentFrameId"))
            .and_then(serde_json::Value::as_str);
        let event_url = frame.get("url").and_then(serde_json::Value::as_str);
        let pdf_document = event_url
            .filter(|url| url.starts_with("chrome-extension://mhjfbmdgcfjbbpaeojofohoefgiehjai/"));
        let file_document = event_url.filter(|url| url.starts_with("file:"));
        let owner = self
            .sessions
            .get(session)
            .map(|(host, root)| (*host, *root));
        let page_main = owner
            .and_then(|(host, root)| root.then(|| self.hosts.get(&host)).flatten())
            .and_then(|page| page.main_frame.as_deref());
        let tracked_parent = self.frame_parents.get(frame_id).map(String::as_str);
        let tracks_pdf_relation =
            pdf_document.is_some() || event_parent == page_main || tracked_parent == page_main;
        if !tracks_pdf_relation && file_document.is_none() {
            return;
        }
        let document = pdf_document
            .map(str::to_owned)
            .or_else(|| file_document.map(|_| String::from("file:")));
        trace_browser_startup(
            self.worker_ctx,
            &format!(
                "PDF frame event={method} owner={owner:?} session={session} target={:?} root_frame={:?} frame={frame_id} event_parent={event_parent:?} tracked_parent={tracked_parent:?} event_document={document:?} tracked_session={:?} tracked_document={:?}",
                self.session_targets.get(session),
                self.session_root_frames.get(session),
                self.frame_sessions.get(frame_id),
                self.frame_urls.get(frame_id)
            ),
        );
    }

    fn trace_pdf_target_detached(&self, message: &serde_json::Value) {
        let session = message
            .pointer("/params/sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let owner = self.sessions.get(session).copied();
        let main_frame = owner
            .and_then(|(host, root)| root.then(|| self.hosts.get(&host)).flatten())
            .and_then(|page| page.main_frame.as_deref());
        let frames = self
            .frame_sessions
            .iter()
            .filter(|(_, frame_session)| frame_session.as_str() == session)
            .filter_map(|(frame, _)| {
                let parent = self.frame_parents.get(frame).map(String::as_str);
                let document = self.frame_urls.get(frame).map(String::as_str);
                let viewer = document.is_some_and(|url| url.starts_with(PDF_VIEWER_DOCUMENT_URL));
                let child_of_main = parent == main_frame;
                (viewer || child_of_main).then(|| {
                    let document = document
                        .filter(|url| url.starts_with("chrome-extension://"))
                        .map(str::to_owned)
                        .or_else(|| {
                            document
                                .filter(|url| url.starts_with("file:"))
                                .map(|_| String::from("file:"))
                        });
                    format!("{frame} parent={parent:?} document={document:?}")
                })
            })
            .collect::<Vec<_>>();
        if frames.is_empty() {
            return;
        }
        trace_browser_startup(
            self.worker_ctx,
            &format!(
                "PDF target detached session={session} target={:?} owner={owner:?} main_frame={main_frame:?} frames={frames:?}",
                self.session_targets.get(session)
            ),
        );
    }

    fn screencast_frame(&mut self, message: serde_json::Value) {
        let session = message
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let Some((host, root)) = self.sessions.get(&session).copied() else {
            return;
        };
        let ack_id = message
            .pointer("/params/sessionId")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let _ = self.notify(
            Some(&session),
            "Page.screencastFrameAck",
            serde_json::json!({"sessionId": ack_id}),
        );
        if !root {
            return;
        }
        if self
            .hosts
            .get(&host)
            .is_some_and(|page| page.screencast_refresh_pending)
        {
            return;
        }
        let (first_frame, generation, page_visual, visible, installed) = {
            let Some(page) = self.hosts.get_mut(&host) else {
                return;
            };
            let first = !page.screencast_frame_reported;
            page.screencast_frame_reported = true;
            (
                first,
                page.generation,
                page.page,
                page.visible,
                page.installed,
            )
        };
        if first_frame {
            trace_browser_startup(
                self.worker_ctx,
                &format!(
                    "screencast first frame received generation={generation} page={page_visual:?} visible={visible} installed={installed}"
                ),
            );
        }
        let Some(data) = message
            .pointer("/params/data")
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        let Ok(bytes) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data)
        else {
            if first_frame {
                trace_browser_startup(
                    self.worker_ctx,
                    "screencast first frame rejected invalid base64",
                );
            }
            return;
        };
        let Ok(image) = image::load_from_memory(&bytes) else {
            if first_frame {
                trace_browser_startup(
                    self.worker_ctx,
                    "screencast first frame rejected invalid image",
                );
            }
            return;
        };
        let Some(page) = self.hosts.get_mut(&host) else {
            return;
        };
        if !page.visible || !page.installed {
            if first_frame {
                trace_browser_startup(
                    self.worker_ctx,
                    &format!(
                        "screencast first frame not posted visible={} installed={}",
                        page.visible, page.installed
                    ),
                );
            }
            return;
        }
        let Some(page_visual) = page.page else {
            if first_frame {
                trace_browser_startup(
                    self.worker_ctx,
                    "screencast first frame not posted without page label",
                );
            }
            return;
        };
        let sequence = page.sequence.saturating_add(1);
        page.sequence = sequence;
        let frame = WebFrame {
            page: page_visual,
            generation: page.generation,
            sequence,
            bounds_px: page.bounds,
            visible: page.visible,
            width_px: image.width(),
            height_px: image.height(),
            bgra: Arc::from(rgba_to_bgra(image.to_rgba8().into_raw())),
        };
        page.post(ActorNotice::Frame(frame));
        if sequence == 1 {
            trace_browser_startup(
                self.worker_ctx,
                &format!(
                    "screencast first frame posted generation={} page={page_visual:?} sequence={sequence} size={}x{}",
                    page.generation,
                    image.width(),
                    image.height()
                ),
            );
        }
    }

    fn frame_attached(&mut self, message: serde_json::Value) {
        let session = message
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let Some((host, _)) = self.sessions.get(session).copied() else {
            return;
        };
        let Some(frame) = message
            .pointer("/params/frameId")
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        let Some(parent) = message
            .pointer("/params/parentFrameId")
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        self.frame_parents
            .insert(frame.to_owned(), parent.to_owned());
        self.frame_hosts.insert(frame.to_owned(), host);
        self.frame_sessions
            .insert(frame.to_owned(), session.to_owned());
    }

    fn frame_detached(&mut self, message: serde_json::Value) {
        let Some(session) = message.get("sessionId").and_then(serde_json::Value::as_str) else {
            return;
        };
        let Some(frame) = message
            .pointer("/params/frameId")
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        if self.frame_sessions.get(frame).map(String::as_str) != Some(session) {
            return;
        }
        let host = self.frame_hosts.get(frame).copied();
        let reason = message
            .pointer("/params/reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        retire_detached_frames(
            reason,
            frame,
            &mut self.frame_hosts,
            &mut self.frame_parents,
            &mut self.frame_sessions,
            &mut self.frame_urls,
        );
        if let Some(host) = host {
            self.refresh_ime_cursor(host, None);
        }
    }

    fn frame_navigated(&mut self, message: serde_json::Value) {
        let session = message
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let Some((host, root)) = self.sessions.get(&session).copied() else {
            return;
        };
        let Some(frame) = message.pointer("/params/frame") else {
            return;
        };
        let Some(frame_id) = frame.get("id").and_then(serde_json::Value::as_str) else {
            return;
        };
        self.frame_hosts.insert(frame_id.to_owned(), host);
        self.frame_sessions
            .insert(frame_id.to_owned(), session.clone());
        if let Some(url) = frame.get("url").and_then(serde_json::Value::as_str) {
            self.frame_urls.insert(frame_id.to_owned(), url.to_owned());
        }
        update_frame_parent(
            &mut self.frame_parents,
            &mut self.session_root_frames,
            &session,
            frame_id,
            frame.get("parentId").and_then(serde_json::Value::as_str),
            root,
        );
        let is_main = root && frame.get("parentId").is_none();
        if !is_main {
            self.refresh_ime_cursor(host, None);
            return;
        }
        let url = frame
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let loader = frame
            .get("loaderId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if let Some(page) = self.hosts.get_mut(&host) {
            page.main_frame = Some(frame_id.to_owned());
            page.main_loader = Some(loader);
            page.main_status = 0;
            page.last_url = url.clone();
            page.main_context = None;
            page.latest_navigation_request = None;
            page.post(ActorNotice::Event {
                generation: page.generation,
                event: WebEvent::SourceChanged { uri: url },
            });
        }
        self.reconcile_main_context(host, &session, frame_id);
        self.history_changed(host);
        self.refresh_ime_cursor(host, None);
    }

    fn navigated_within_document(&mut self, message: serde_json::Value) {
        let session = message
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let Some((host, root)) = self.sessions.get(&session).copied() else {
            return;
        };
        if !root {
            return;
        }
        if message
            .pointer("/params/frameId")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|frame| {
                self.hosts
                    .get(&host)
                    .and_then(|page| page.main_frame.as_deref())
                    == Some(frame)
            })
            && let Some(page) = self.hosts.get_mut(&host)
        {
            page.latest_navigation_request = None;
        }
        let Some(url) = message
            .pointer("/params/url")
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        if let Some(page) = self.hosts.get_mut(&host) {
            page.last_url = url.to_owned();
            page.post(ActorNotice::Event {
                generation: page.generation,
                event: WebEvent::SourceChanged {
                    uri: url.to_owned(),
                },
            });
        }
        self.history_changed(host);
        self.refresh_ime_cursor(host, None);
    }

    fn window_open(&mut self, message: serde_json::Value) {
        if !message
            .pointer("/params/userGesture")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            return;
        }
        let Some(host) = message
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .and_then(|session| self.sessions.get(session).map(|(host, _)| *host))
            .filter(|host| *host != 0)
        else {
            return;
        };
        let Some(url) = message
            .pointer("/params/url")
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        let Some((session, generation)) = self.root_session(host) else {
            return;
        };
        if let Err(error) = self.notify(
            Some(&session),
            "Page.navigate",
            serde_json::json!({"url": url}),
        ) {
            self.post(
                host,
                ActorNotice::Event {
                    generation,
                    event: WebEvent::ProcessFailed {
                        kind: 1,
                        description: format!(
                            "Chromium could not open the requested window in this seat: {error}"
                        ),
                    },
                },
            );
        }
    }

    fn load_event_fired(&mut self, message: serde_json::Value) {
        let session = message
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let Some((host, root)) = self.sessions.get(&session).copied() else {
            return;
        };
        if !root {
            return;
        }
        let title = self
            .evaluate(&session, String::from("document.title"))
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        if let Some(page) = self.hosts.get(&host) {
            let generation = page.generation;
            page.post(ActorNotice::Event {
                generation,
                event: WebEvent::DocumentTitleChanged { title },
            });
            page.post(ActorNotice::Event {
                generation,
                event: WebEvent::NavigationCompleted {
                    uri: page.last_url.clone(),
                    success: true,
                    status: page.main_status,
                },
            });
        }
        self.history_changed(host);
        self.refresh_ime_cursor(host, None);
    }

    fn history_changed(&mut self, host: HostId) {
        let Some((session, generation)) = self.root_session(host) else {
            return;
        };
        let Ok(history) = self.call(
            Some(&session),
            "Page.getNavigationHistory",
            serde_json::json!({}),
        ) else {
            return;
        };
        let index = history
            .get("currentIndex")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let count = history
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .map_or(0_i64, |entries| entries.len() as i64);
        self.post(
            host,
            ActorNotice::Event {
                generation,
                event: WebEvent::HistoryChanged {
                    can_go_back: index > 0,
                    can_go_forward: index + 1 < count,
                },
            },
        );
    }

    fn javascript_dialog_opening(&mut self, message: serde_json::Value) {
        let session = message
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let Some((host, _)) = self.sessions.get(&session).copied() else {
            return;
        };
        let kind = message
            .pointer("/params/type")
            .and_then(serde_json::Value::as_str)
            .map_or(0, |kind| match kind {
                "alert" => 0,
                "confirm" => 1,
                "prompt" => 2,
                "beforeunload" => 3,
                _ => -1,
            });
        let _ = self.notify(
            Some(&session),
            "Page.handleJavaScriptDialog",
            serde_json::json!({"accept": false}),
        );
        let generation = self.hosts.get(&host).map_or(0, |page| page.generation);
        self.post(
            host,
            ActorNotice::Event {
                generation,
                event: WebEvent::ScriptDialogDismissed { kind },
            },
        );
    }

    fn response_received(&mut self, message: serde_json::Value) {
        let session = message
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let Some((host, root)) = self.sessions.get(session).copied() else {
            return;
        };
        if !root
            || message
                .pointer("/params/type")
                .and_then(serde_json::Value::as_str)
                != Some("Document")
        {
            return;
        }
        let frame = message
            .pointer("/params/frameId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if self
            .hosts
            .get(&host)
            .and_then(|page| page.main_frame.as_deref())
            != Some(frame)
        {
            return;
        }
        let status = message
            .pointer("/params/response/status")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0) as i32;
        if let Some(page) = self.hosts.get_mut(&host) {
            page.main_status = status;
        }
    }

    fn loading_failed(&mut self, _message: serde_json::Value) {}

    fn handle_command(&mut self, worker: &WorkerCtx, command: ActorCommand) {
        match command {
            ActorCommand::Boot {
                host,
                folder,
                generation,
                rules,
                color_scheme,
                inbox,
                wake,
            } => self.boot_host(
                worker,
                BootRequest {
                    host,
                    folder,
                    generation,
                    rules,
                    color_scheme,
                    inbox,
                    wake,
                },
            ),
            ActorCommand::CreatePage {
                host,
                generation,
                bounds,
                scale,
                visible,
                color_scheme,
                rules,
            } => self.create_page(
                worker,
                CreatePageRequest {
                    host,
                    generation,
                    bounds,
                    scale,
                    visible,
                    color_scheme,
                    rules,
                },
            ),
            ActorCommand::Install {
                host,
                page,
                generation,
            } => self.install_page(host, page, generation),
            ActorCommand::RequestRules { host, rules } => {
                self.update_rules(worker, host, rules);
            }
            ActorCommand::ColorScheme { host, scheme } => {
                self.set_color_scheme(host, scheme);
            }
            ActorCommand::Bounds {
                host,
                bounds,
                scale,
            } => {
                self.set_bounds(host, bounds, scale);
            }
            ActorCommand::Visible { host, visible } => self.set_visible(host, visible),
            ActorCommand::Navigate { host, url } => self.navigate(host, &url),
            ActorCommand::Reload { host } => self.reload(host),
            ActorCommand::StopLoading { host } => self.stop_loading(host),
            ActorCommand::History { host, direction } => self.history(host, direction),
            ActorCommand::Zoom { host, factor } => self.zoom(host, factor),
            ActorCommand::Find {
                host,
                term,
                case_sensitive,
            } => self.find(host, term, case_sensitive),
            ActorCommand::FindStep { host, forwards } => self.find_step(host, forwards),
            ActorCommand::FindStop { host } => self.find_stop(host),
            ActorCommand::Focus { host } => self.focus(host),
            ActorCommand::Mouse {
                host,
                event,
                point,
                buttons_down,
            } => self.mouse(host, event, point, buttons_down),
            ActorCommand::Key { host, event } => self.key(host, event),
            ActorCommand::Ime { host, event } => self.ime(host, event),
            ActorCommand::Capture { host } => self.capture(host),
            ActorCommand::Favicon { host } => self.favicon(host),
            ActorCommand::MovePage {
                host,
                page,
                visible,
            } => self.move_page(host, page, visible),
            ActorCommand::CancelCreate { host, generation } => self.cancel_create(host, generation),
            ActorCommand::GateVerdict {
                host,
                generation,
                key,
                reply,
            } => self.gate_verdict(host, generation, key, reply),
            ActorCommand::Close { host } => self.close_host(worker, host),
            ActorCommand::ForgetEnvironment => self.forget_browser(worker),
            ActorCommand::Shutdown => {
                let hosts = self.hosts.keys().copied().collect::<Vec<_>>();
                for host in hosts {
                    self.close_host(worker, host);
                }
                self.close_browser(worker);
                self.running = false;
            }
        }
    }

    fn boot_host(&mut self, worker: &WorkerCtx, request: BootRequest) {
        let BootRequest {
            host,
            folder,
            generation,
            rules,
            color_scheme,
            inbox,
            wake,
        } = request;
        self.cancelled_hosts.remove(&host);
        let page = self.hosts.entry(host).or_insert_with(|| HostPage {
            inbox,
            wake,
            folder: folder.clone(),
            generation,
            page: None,
            rules: rules.clone(),
            color_scheme,
            bounds: (0, 0, 0, 0),
            scale: 1.0,
            visible: false,
            sequence: 0,
            screencast_started: false,
            screencast_frame_reported: false,
            screencast_refresh_pending: false,
            destination_bounds_ready: true,
            target: None,
            session: None,
            tab_id: None,
            main_frame: None,
            policy_token: None,
            navigation_binding: None,
            navigation_permit: None,
            latest_navigation_request: None,
            ime_binding: None,
            ime_token: None,
            last_ime_cursor: None,
            main_context: None,
            browser_process_id: 0,
            installed: false,
            main_loader: None,
            main_status: 0,
            find_term: String::new(),
            find_case_sensitive: false,
            find_count: 0,
            find_active: 0,
            last_url: String::new(),
        });
        page.folder = folder.clone();
        page.generation = generation;
        page.latest_navigation_request = None;
        page.rules = rules;
        page.color_scheme = color_scheme;
        let result = self.start_browser_for_host(worker, &folder, host, generation);
        match result {
            Ok(()) => {
                let process_id = self
                    .browser
                    .as_ref()
                    .map_or(0, |browser| browser.child.id());
                if let Some(page) = self.hosts.get_mut(&host) {
                    page.browser_process_id = process_id;
                    page.post(ActorNotice::Environment {
                        generation,
                        error: None,
                        browser_process_id: Some(process_id),
                    });
                }
            }
            Err(error) => {
                self.close_browser(worker);
                self.post(
                    host,
                    ActorNotice::Environment {
                        generation,
                        error: Some(error),
                        browser_process_id: None,
                    },
                );
            }
        }
    }

    fn create_page(&mut self, worker: &WorkerCtx, request: CreatePageRequest) {
        let CreatePageRequest {
            host,
            generation,
            bounds,
            scale,
            visible,
            color_scheme,
            rules,
        } = request;
        let Some(folder) = self.hosts.get(&host).map(|page| page.folder.clone()) else {
            return;
        };
        if let Some(page) = self.hosts.get_mut(&host) {
            page.generation = generation;
            page.latest_navigation_request = None;
            page.bounds = bounds;
            page.scale = scale;
            page.visible = visible;
            page.sequence = 0;
            page.screencast_frame_reported = false;
            page.screencast_started = false;
            page.color_scheme = color_scheme;
            page.rules = rules;
            page.ime_binding = None;
            page.ime_token = None;
            page.last_ime_cursor = None;
        }
        let restarted_after_unknown_create = match self.prepare_page_create(worker, host) {
            Ok(restarted) => restarted,
            Err(error) => {
                self.post(
                    host,
                    ActorNotice::Controller {
                        generation,
                        error: Some(error),
                    },
                );
                return;
            }
        };
        let result = self.start_browser_for_host(worker, &folder, host, generation);
        if result.is_ok() && restarted_after_unknown_create {
            let process_id = self
                .browser
                .as_ref()
                .map_or(0, |browser| browser.child.id());
            if let Some(page) = self.hosts.get_mut(&host) {
                page.browser_process_id = process_id;
                page.post(ActorNotice::Environment {
                    generation,
                    error: None,
                    browser_process_id: Some(process_id),
                });
            }
        }
        let result = result.and_then(|()| self.create_browser_page(worker, host, generation));
        if let Err(error) = result {
            self.post(
                host,
                ActorNotice::Controller {
                    generation,
                    error: Some(error),
                },
            );
        }
    }

    fn start_browser_for_host(
        &mut self,
        worker: &WorkerCtx,
        folder: &PathBuf,
        host: HostId,
        generation: u64,
    ) -> Result<(), String> {
        self.active_boot = Some((host, generation));
        let result = self.start_browser(worker, folder);
        if self.active_boot == Some((host, generation)) {
            self.active_boot = None;
        }
        result
    }

    fn install_page(&mut self, host: HostId, page: PageVisual, generation: u64) {
        let session = {
            let Some(host_page) = self.hosts.get_mut(&host) else {
                return;
            };
            if host_page.generation != generation || host_page.session.is_none() {
                return;
            }
            host_page.page = Some(page);
            host_page.installed = true;
            host_page.sequence = 0;
            host_page.screencast_frame_reported = false;
            host_page.last_ime_cursor = None;
            host_page.session.clone()
        };
        trace_browser_startup(
            self.worker_ctx,
            &format!("web page installed generation={generation} page={page:?}"),
        );
        if let Some(session) = session {
            let visible = self.hosts.get(&host).is_some_and(|page| page.visible);
            if visible && let Err(error) = self.start_screencast_for_page(host, &session) {
                if let Some(page) = self.hosts.get_mut(&host) {
                    page.installed = false;
                }
                self.post(
                    host,
                    ActorNotice::Event {
                        generation,
                        event: WebEvent::ProcessFailed {
                            kind: 1,
                            description: error,
                        },
                    },
                );
                return;
            }
            self.refresh_ime_cursor(host, None);
        }
    }

    fn update_rules(&mut self, worker: &WorkerCtx, host: HostId, rules: String) {
        let target = match self.hosts.get_mut(&host) {
            Some(page) => {
                page.rules.clone_from(&rules);
                page.target.clone()
            }
            None => return,
        };
        let Some(target) = target else {
            return;
        };
        if let Err(error) = self.set_websocket_policy(host, &target, &rules) {
            self.browser_failed(
                worker,
                format!("the Linux web request gate could not be updated: {error}"),
            );
        }
    }

    fn set_color_scheme(&mut self, host: HostId, scheme: WebColorScheme) {
        let Some((session, _)) = self.root_session(host) else {
            if let Some(page) = self.hosts.get_mut(&host) {
                page.color_scheme = Some(scheme);
            }
            return;
        };
        if let Err(error) = self.apply_color_scheme(&session, scheme)
            && let Some(generation) = self.hosts.get(&host).map(|page| page.generation)
        {
            self.post(
                host,
                ActorNotice::Event {
                    generation,
                    event: WebEvent::ProcessFailed {
                        kind: 1,
                        description: error,
                    },
                },
            );
        }
    }

    fn set_bounds(&mut self, host: HostId, bounds: (i32, i32, u32, u32), scale: f64) {
        let Some((session, _)) = self.root_session(host) else {
            if let Some(page) = self.hosts.get_mut(&host) {
                page.bounds = bounds;
                page.scale = scale;
                if page.screencast_refresh_pending {
                    page.destination_bounds_ready = true;
                }
            }
            return;
        };
        if let Err(error) = self.apply_bounds(host, &session, bounds, scale) {
            let generation = self.hosts.get(&host).map_or(0, |page| page.generation);
            self.post(
                host,
                ActorNotice::Event {
                    generation,
                    event: WebEvent::ProcessFailed {
                        kind: 1,
                        description: error,
                    },
                },
            );
        } else {
            if let Some(page) = self.hosts.get_mut(&host)
                && page.screencast_refresh_pending
            {
                page.destination_bounds_ready = true;
            }
            if let Err(error) = self.start_screencast_for_page(host, &session) {
                let generation = self.hosts.get(&host).map_or(0, |page| page.generation);
                self.post(
                    host,
                    ActorNotice::Event {
                        generation,
                        event: WebEvent::ProcessFailed {
                            kind: 1,
                            description: error,
                        },
                    },
                );
            }
            self.refresh_ime_cursor(host, None);
        }
    }

    fn set_visible(&mut self, host: HostId, visible: bool) {
        let Some((session, _)) = self.root_session(host) else {
            if let Some(page) = self.hosts.get_mut(&host) {
                page.visible = visible;
            }
            return;
        };
        if let Some(page) = self.hosts.get_mut(&host) {
            page.visible = visible;
        }
        let result = if visible {
            self.start_screencast_for_page(host, &session)
        } else {
            self.stop_screencast(host);
            Ok(())
        };
        if let Err(error) = result {
            let generation = self.hosts.get(&host).map_or(0, |page| page.generation);
            self.post(
                host,
                ActorNotice::Event {
                    generation,
                    event: WebEvent::ProcessFailed {
                        kind: 1,
                        description: error,
                    },
                },
            );
        } else {
            self.refresh_ime_cursor(host, None);
        }
    }

    fn navigate(&mut self, host: HostId, url: &str) {
        let Some((session, generation)) = self.root_session(host) else {
            return;
        };
        let scheme = url.split_once(':').map_or("unknown", |(scheme, _)| scheme);
        if let Some(page) = self.hosts.get_mut(&host) {
            page.latest_navigation_request = None;
        }
        trace_browser_startup(
            self.worker_ctx,
            &format!("page navigation enter generation={generation} scheme={scheme}"),
        );
        let result = match self.permit_navigation_once(host, url) {
            Ok(()) => {
                trace_browser_startup(
                    self.worker_ctx,
                    &format!(
                        "page navigation permit accepted generation={generation} scheme={scheme}"
                    ),
                );
                trace_browser_startup(
                    self.worker_ctx,
                    &format!("Page.navigate enter generation={generation} scheme={scheme}"),
                );
                match self.call(
                    Some(&session),
                    "Page.navigate",
                    serde_json::json!({"url": url}),
                ) {
                    Ok(reply) => {
                        let error = reply
                            .get("errorText")
                            .and_then(serde_json::Value::as_str)
                            .filter(|error| !error.is_empty());
                        let is_download = reply
                            .get("isDownload")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false);
                        trace_browser_startup(
                            self.worker_ctx,
                            &format!(
                                "Page.navigate reply generation={generation} scheme={scheme} error={} download={is_download}",
                                error.is_some()
                            ),
                        );
                        if let Some(error) = error {
                            Err(format!(
                                "Chromium could not navigate to the requested {scheme} page: {error}"
                            ))
                        } else {
                            Ok(reply)
                        }
                    }
                    Err(error) => {
                        trace_browser_startup(
                            self.worker_ctx,
                            &format!(
                                "Page.navigate CDP error generation={generation} scheme={scheme}"
                            ),
                        );
                        Err(error)
                    }
                }
            }
            Err(error) => {
                trace_browser_startup(
                    self.worker_ctx,
                    &format!(
                        "page navigation permit failed generation={generation} scheme={scheme}"
                    ),
                );
                Err(error)
            }
        };
        match result {
            Ok(_) => {
                if let Some(page) = self.hosts.get_mut(&host) {
                    page.last_url = url.to_owned();
                }
            }
            Err(error) => self.post(
                host,
                ActorNotice::Event {
                    generation,
                    event: WebEvent::ProcessFailed {
                        kind: 1,
                        description: error,
                    },
                },
            ),
        }
    }

    fn reload(&mut self, host: HostId) {
        let Some((session, generation)) = self.root_session(host) else {
            return;
        };
        if let Some(page) = self.hosts.get_mut(&host) {
            page.latest_navigation_request = None;
        }
        if let Err(error) = self.call(
            Some(&session),
            "Page.reload",
            serde_json::json!({"ignoreCache": false}),
        ) {
            self.post(
                host,
                ActorNotice::Event {
                    generation,
                    event: WebEvent::ProcessFailed {
                        kind: 1,
                        description: error,
                    },
                },
            );
        }
    }

    fn stop_loading(&mut self, host: HostId) {
        let Some((session, _)) = self.root_session(host) else {
            return;
        };
        if let Some(page) = self.hosts.get_mut(&host) {
            page.latest_navigation_request = None;
        }
        let requests = self
            .pending_fetches
            .iter()
            .filter(|(_, request)| request.hosts.contains_key(&host))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in requests {
            self.fail_fetch(&key, "Aborted");
        }
        let _ = self.notify(Some(&session), "Page.stopLoading", serde_json::json!({}));
    }

    fn history(&mut self, host: HostId, direction: i8) {
        let Some((session, generation)) = self.root_session(host) else {
            return;
        };
        if let Some(page) = self.hosts.get_mut(&host) {
            page.latest_navigation_request = None;
        }
        let result = self
            .call(
                Some(&session),
                "Page.getNavigationHistory",
                serde_json::json!({}),
            )
            .and_then(|history| {
                let index = history
                    .get("currentIndex")
                    .and_then(serde_json::Value::as_i64)
                    .ok_or_else(|| String::from("Chromium returned no history index"))?;
                let entries = history
                    .get("entries")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| String::from("Chromium returned no history entries"))?;
                let target = index + i64::from(direction);
                let entry = entries
                    .get(usize::try_from(target).map_err(|error| error.to_string())?)
                    .ok_or_else(|| String::from("there is no page in that history direction"))?;
                let entry_id = entry
                    .get("id")
                    .and_then(serde_json::Value::as_i64)
                    .ok_or_else(|| String::from("Chromium history entry has no id"))?;
                self.call(
                    Some(&session),
                    "Page.navigateToHistoryEntry",
                    serde_json::json!({"entryId": entry_id}),
                )
                .map(|_| ())
            });
        if let Err(error) = result {
            self.post(
                host,
                ActorNotice::Event {
                    generation,
                    event: WebEvent::StatusBarTextChanged { text: error },
                },
            );
        }
    }

    fn zoom(&mut self, host: HostId, factor: f64) {
        let Some((session, _)) = self.root_session(host) else {
            return;
        };
        let _ = self.call(
            Some(&session),
            "Emulation.setPageScaleFactor",
            serde_json::json!({"pageScaleFactor": factor}),
        );
        self.refresh_ime_cursor(host, None);
    }

    fn find(&mut self, host: HostId, term: String, case_sensitive: bool) {
        let Some((session, generation)) = self.root_session(host) else {
            return;
        };
        let term_json = match serde_json::to_string(&term) {
            Ok(term) => term,
            Err(error) => {
                self.post(
                    host,
                    ActorNotice::Event {
                        generation,
                        event: WebEvent::ProcessFailed {
                            kind: 1,
                            description: error.to_string(),
                        },
                    },
                );
                return;
            }
        };
        let expression = format!(
            "(() => {{ const text = document.body?.innerText ?? ''; const needle = {term_json}; const haystack = {case_sensitive}.toString() === 'true' ? text : text.toLocaleLowerCase(); const query = {case_sensitive}.toString() === 'true' ? needle : needle.toLocaleLowerCase(); let count = 0; if (query) {{ let at = 0; while ((at = haystack.indexOf(query, at)) !== -1) {{ count += 1; at += Math.max(1, query.length); }} }} const found = window.find(needle, {case_sensitive}, false, true, false, false, false); return JSON.stringify({{count, found}}); }})()"
        );
        let result = self.evaluate(&session, expression);
        match result {
            Ok(value) => {
                let encoded = value.as_str().unwrap_or("{}");
                let decoded =
                    serde_json::from_str::<serde_json::Value>(encoded).unwrap_or_default();
                let count = decoded
                    .get("count")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0) as i32;
                let found = decoded
                    .get("found")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                if let Some(page) = self.hosts.get_mut(&host) {
                    page.find_term = term;
                    page.find_case_sensitive = case_sensitive;
                    page.find_count = count;
                    page.find_active = i32::from(found);
                }
                self.post(
                    host,
                    ActorNotice::Event {
                        generation,
                        event: WebEvent::FindMatches {
                            count,
                            active: i32::from(found),
                        },
                    },
                );
            }
            Err(error) => self.post(
                host,
                ActorNotice::Event {
                    generation,
                    event: WebEvent::ProcessFailed {
                        kind: 1,
                        description: error,
                    },
                },
            ),
        }
    }

    fn find_step(&mut self, host: HostId, forwards: bool) {
        let Some((session, generation)) = self.root_session(host) else {
            return;
        };
        let Some(page) = self.hosts.get(&host) else {
            return;
        };
        let term = page.find_term.clone();
        let case_sensitive = page.find_case_sensitive;
        let count = page.find_count;
        let term_json = serde_json::to_string(&term).unwrap_or_else(|_| String::from("\"\""));
        let expression = format!(
            "window.find({term_json},{case_sensitive},{backwards},true,false,false,false)",
            backwards = !forwards
        );
        let found = self
            .evaluate(&session, expression)
            .ok()
            .and_then(|value| {
                value
                    .as_bool()
                    .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
            })
            .unwrap_or(false);
        let active = if !found || count == 0 {
            0
        } else if let Some(page) = self.hosts.get_mut(&host) {
            if forwards {
                page.find_active = page.find_active.rem_euclid(count).saturating_add(1);
            } else {
                page.find_active = (page.find_active - 2).rem_euclid(count) + 1;
            }
            page.find_active
        } else {
            return;
        };
        self.post(
            host,
            ActorNotice::Event {
                generation,
                event: WebEvent::FindMatches { count, active },
            },
        );
    }

    fn find_stop(&mut self, host: HostId) {
        if let Some((session, _)) = self.root_session(host) {
            let _ = self.evaluate(
                &session,
                String::from("window.getSelection()?.removeAllRanges()"),
            );
        }
        if let Some(page) = self.hosts.get_mut(&host) {
            page.find_term.clear();
            page.find_count = 0;
            page.find_active = 0;
        }
    }

    fn focus(&mut self, host: HostId) {
        let Some((session, _)) = self.root_session(host) else {
            return;
        };
        let _ = self.call(Some(&session), "Page.bringToFront", serde_json::json!({}));
        let _ = self.evaluate(&session, String::from("window.focus()"));
        self.refresh_ime_cursor(host, None);
    }

    fn mouse(&mut self, host: HostId, event: WebMouseEvent, point: (i32, i32), buttons_down: u32) {
        let Some((session, _)) = self.root_session(host) else {
            if matches!(
                event,
                WebMouseEvent::LeftDown
                    | WebMouseEvent::LeftUp
                    | WebMouseEvent::LeftDoubleClick
                    | WebMouseEvent::RightDown
                    | WebMouseEvent::RightUp
                    | WebMouseEvent::MiddleDown
                    | WebMouseEvent::MiddleUp
            ) {
                trace_browser_startup(
                    self.worker_ctx,
                    &format!(
                        "mouse dispatch skipped host={host} event={event:?} root_session=None page={:?}",
                        self.hosts.get(&host).map(|page| (
                            page.session.as_deref(),
                            page.visible,
                            page.installed
                        ))
                    ),
                );
            }
            return;
        };
        let Some(page) = self.hosts.get(&host) else {
            return;
        };
        let x = f64::from(point.0) / page.scale;
        let y = f64::from(point.1) / page.scale;
        let mut params = serde_json::json!({
            "x": x,
            "y": y,
            "buttons": cdp_mouse_buttons(buttons_down)
        });
        let (kind, button, click_count) = match event {
            WebMouseEvent::Move | WebMouseEvent::Leave => ("mouseMoved", "none", 0),
            WebMouseEvent::LeftDown => ("mousePressed", "left", 1),
            WebMouseEvent::LeftUp => ("mouseReleased", "left", 1),
            WebMouseEvent::LeftDoubleClick => ("mousePressed", "left", 2),
            WebMouseEvent::RightDown => ("mousePressed", "right", 1),
            WebMouseEvent::RightUp => ("mouseReleased", "right", 1),
            WebMouseEvent::MiddleDown => ("mousePressed", "middle", 1),
            WebMouseEvent::MiddleUp => ("mouseReleased", "middle", 1),
            WebMouseEvent::XDown(1) => ("mousePressed", "back", 1),
            WebMouseEvent::XDown(_) => ("mousePressed", "forward", 1),
            WebMouseEvent::XUp(1) => ("mouseReleased", "back", 1),
            WebMouseEvent::XUp(_) => ("mouseReleased", "forward", 1),
            WebMouseEvent::Wheel(delta) => ("mouseWheel", "none", i32::from(delta)),
            WebMouseEvent::HorizontalWheel(delta) => ("mouseWheel", "none", i32::from(delta)),
        };
        params["type"] = serde_json::Value::String(kind.to_owned());
        if button != "none" {
            params["button"] = serde_json::Value::String(button.to_owned());
            params["clickCount"] = serde_json::Value::from(click_count);
        }
        match event {
            WebMouseEvent::Wheel(delta) => params["deltaY"] = serde_json::Value::from(-delta),
            WebMouseEvent::HorizontalWheel(delta) => {
                params["deltaX"] = serde_json::Value::from(delta)
            }
            _ => {}
        }
        let dispatched = self.call(Some(&session), "Input.dispatchMouseEvent", params);
        if matches!(
            event,
            WebMouseEvent::LeftDown
                | WebMouseEvent::LeftUp
                | WebMouseEvent::LeftDoubleClick
                | WebMouseEvent::RightDown
                | WebMouseEvent::RightUp
                | WebMouseEvent::MiddleDown
                | WebMouseEvent::MiddleUp
        ) {
            trace_browser_startup(
                self.worker_ctx,
                &format!(
                    "mouse dispatch event={event:?} session={session} viewport=({x},{y}) buttons={buttons_down} success={}",
                    dispatched.is_ok()
                ),
            );
        }
        self.refresh_ime_cursor(host, None);
    }

    fn key(&mut self, host: HostId, event: WebKeyEvent) {
        let Some((session, _)) = self.root_session(host) else {
            return;
        };
        let mut modifiers = 0;
        if event.modifiers.alt {
            modifiers |= 1;
        }
        if event.modifiers.ctrl {
            modifiers |= 2;
        }
        if event.modifiers.meta {
            modifiers |= 4;
        }
        if event.modifiers.shift {
            modifiers |= 8;
        }
        let mut params = serde_json::json!({
            "type": if event.down { "keyDown" } else { "keyUp" },
            "key": event.key,
            "code": event.code,
            "modifiers": modifiers,
            "location": event.location,
            "autoRepeat": event.repeat
        });
        if let Some(text) = event.text
            && event.down
        {
            params["text"] = serde_json::Value::String(text.clone());
            params["unmodifiedText"] = serde_json::Value::String(text);
        }
        let _ = self.call(Some(&session), "Input.dispatchKeyEvent", params);
        self.refresh_ime_cursor(host, None);
    }

    fn ime(&mut self, host: HostId, event: WebImeEvent) {
        let Some((session, _)) = self.root_session(host) else {
            return;
        };
        match event {
            WebImeEvent::Preedit {
                text,
                selection_utf16,
            } => {
                let (selection_start, selection_end) = selection_utf16.unwrap_or_else(|| {
                    let end = text.encode_utf16().count() as u32;
                    (end, end)
                });
                let _ = self.call(
                    Some(&session),
                    "Input.imeSetComposition",
                    serde_json::json!({
                        "text": text,
                        "selectionStart": selection_start,
                        "selectionEnd": selection_end
                    }),
                );
            }
            WebImeEvent::Commit(text) => {
                let _ = self.call(
                    Some(&session),
                    "Input.insertText",
                    serde_json::json!({"text": text}),
                );
            }
            WebImeEvent::Cancel => {
                let _ = self.call(
                    Some(&session),
                    "Input.imeSetComposition",
                    serde_json::json!({
                        "text": "",
                        "selectionStart": 0,
                        "selectionEnd": 0
                    }),
                );
            }
        }
        self.refresh_ime_cursor(host, None);
    }

    fn capture(&mut self, host: HostId) {
        let Some((session, generation)) = self.root_session(host) else {
            return;
        };
        let result = self.call(
            Some(&session),
            "Page.captureScreenshot",
            serde_json::json!({"format": "png", "fromSurface": true, "captureBeyondViewport": false}),
        );
        let png = result
            .ok()
            .and_then(|result| {
                result
                    .get("data")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .and_then(|encoded| {
                base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded).ok()
            });
        self.post(
            host,
            ActorNotice::Event {
                generation,
                event: WebEvent::Captured { png },
            },
        );
    }

    fn favicon(&mut self, host: HostId) {
        let Some((session, generation)) = self.root_session(host) else {
            return;
        };
        let source = self
            .evaluate(
                &session,
                String::from("document.querySelector('link[rel~=icon]')?.href ?? ''"),
            )
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        self.post(
            host,
            ActorNotice::Event {
                generation,
                event: WebEvent::FaviconChanged {
                    uri: source.clone(),
                },
            },
        );
        let Some(page) = self.hosts.get(&host) else {
            return;
        };
        let Some(frame) = page.main_frame.clone() else {
            return;
        };
        if source.is_empty() {
            self.post(
                host,
                ActorNotice::Event {
                    generation,
                    event: WebEvent::Favicon { png: None },
                },
            );
            return;
        }
        let png = self
            .call(
                Some(&session),
                "Page.getResourceContent",
                serde_json::json!({"frameId": frame, "url": source}),
            )
            .ok()
            .and_then(|resource| {
                let content = resource.get("content")?.as_str()?;
                if resource
                    .get("base64Encoded")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                {
                    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, content).ok()
                } else {
                    Some(content.as_bytes().to_vec())
                }
            })
            .and_then(|bytes| image::load_from_memory(&bytes).ok())
            .and_then(|image| {
                let mut png = std::io::Cursor::new(Vec::new());
                image.write_to(&mut png, image::ImageFormat::Png).ok()?;
                Some(png.into_inner())
            });
        self.post(
            host,
            ActorNotice::Event {
                generation,
                event: WebEvent::Favicon { png },
            },
        );
    }

    fn move_page(&mut self, host: HostId, page: PageVisual, visible: bool) {
        let Some(host_page) = self.hosts.get_mut(&host) else {
            return;
        };
        host_page.page = Some(page);
        host_page.visible = visible;
        host_page.last_ime_cursor = None;
        host_page.screencast_refresh_pending = true;
        host_page.destination_bounds_ready = false;
        host_page.inbox.discard_frames();
        self.stop_screencast(host);
        self.refresh_ime_cursor(host, None);
    }

    fn cancel_create(&mut self, host: HostId, generation: u64) {
        if let Some((setup_host, setup_generation, target)) = self.pending_controller_setup.as_ref()
            && *setup_host == host
            && *setup_generation == generation
        {
            let target = target.clone();
            self.close_controller_target(host, generation, &target);
            return;
        }
        if let Some(target) =
            self.pending_create
                .iter()
                .find_map(|(target, (owner, pending_generation))| {
                    (*owner == host && *pending_generation == generation).then(|| target.clone())
                })
        {
            self.close_controller_target(host, generation, &target);
        }
    }

    fn permit_navigation_once(&mut self, host: HostId, url: &str) -> Result<(), String> {
        if url.starts_with("http://") || url.starts_with("https://") {
            return Ok(());
        }
        let page = self
            .hosts
            .get(&host)
            .ok_or_else(|| String::from("the web seat was closed"))?;
        let (Some(session), Some(permit), Some(token), Some(context)) = (
            page.session.clone(),
            page.navigation_permit.clone(),
            page.policy_token.clone(),
            page.main_context,
        ) else {
            let scheme = url.split_once(':').map_or("unknown", |(scheme, _)| scheme);
            trace_browser_startup(
                self.worker_ctx,
                &format!(
                    "navigation permit unavailable scheme={scheme} session={} hook={} token={} main_context={}",
                    page.session.is_some(),
                    page.navigation_permit.is_some(),
                    page.policy_token.is_some(),
                    page.main_context.is_some()
                ),
            );
            return Err(String::from("the main-frame navigation gate is not ready"));
        };
        let url = serde_json::to_string(url).map_err(|error| error.to_string())?;
        let token = serde_json::to_string(&token).map_err(|error| error.to_string())?;
        let expression = format!("globalThis[{permit:?}]({url},{token})");
        let scheme = url.split_once(':').map_or("unknown", |(scheme, _)| scheme);
        trace_browser_startup(
            self.worker_ctx,
            &format!("navigation permit evaluate enter scheme={scheme} context={context}"),
        );
        let result = self.call(
            Some(&session),
            "Runtime.evaluate",
            serde_json::json!({
                "expression": expression,
                "contextId": context,
                "returnByValue": true
            }),
        )?;
        if let Some(exception) = result.get("exceptionDetails") {
            trace_browser_startup(
                self.worker_ctx,
                &format!("navigation permit evaluate exception scheme={scheme} context={context}"),
            );
            return Err(format!("the page navigation permit failed: {exception}"));
        }
        trace_browser_startup(
            self.worker_ctx,
            &format!("navigation permit evaluate reply scheme={scheme} context={context}"),
        );
        Ok(())
    }

    fn fail_fetch(&mut self, key: &GateKey, reason: &str) {
        if let Some(PendingFetch {
            session, request, ..
        }) = self.pending_fetches.remove(key)
        {
            let _ = self.notify(
                Some(&session),
                "Fetch.failRequest",
                serde_json::json!({"requestId": request, "errorReason": reason}),
            );
        }
    }

    fn gate_verdict(&mut self, host: HostId, generation: u64, key: GateKey, reply: GateReply) {
        match key.clone() {
            GateKey::Fetch { .. } => {
                let Some(pending) = self.pending_fetches.get(&key) else {
                    return;
                };
                if pending.hosts.get(&host) != Some(&generation) {
                    self.fail_fetch(&key, "Aborted");
                    return;
                }
                match reply {
                    GateReply::Navigation(WebNavigationVerdict::Proceed)
                    | GateReply::Resource(WebRequestVerdict::Allow) => {
                        let complete = self.pending_fetches.get_mut(&key).is_some_and(|pending| {
                            pending.votes.insert(host, true);
                            pending.votes.len() == pending.hosts.len()
                        });
                        if complete
                            && let Some(PendingFetch {
                                session, request, ..
                            }) = self.pending_fetches.remove(&key)
                        {
                            let _ = self.notify(
                                Some(&session),
                                "Fetch.continueRequest",
                                serde_json::json!({"requestId": request}),
                            );
                        }
                    }
                    GateReply::Navigation(WebNavigationVerdict::Cancel)
                    | GateReply::Resource(WebRequestVerdict::Refuse) => {
                        self.fail_fetch(&key, "BlockedByClient");
                    }
                    GateReply::Navigation(WebNavigationVerdict::CancelAndNavigateTo(target)) => {
                        self.fail_fetch(&key, "Aborted");
                        self.navigate(host, &target);
                    }
                }
            }
            GateKey::NavigationApi {
                url, cancelable, ..
            } => {
                let Some(page) = self.hosts.get_mut(&host) else {
                    return;
                };
                if !navigation_api_verdict_is_current(
                    page.generation,
                    generation,
                    page.session.as_deref(),
                    page.main_context,
                    page.policy_token.as_deref(),
                    page.latest_navigation_request.as_ref(),
                    &key,
                ) {
                    return;
                }
                page.latest_navigation_request = None;
                let page_target = page.target.clone();
                if !cancelable {
                    if let Some(target) = page_target {
                        let _ = self.notify(
                            None,
                            "Target.closeTarget",
                            serde_json::json!({"targetId": target}),
                        );
                    }
                    self.post(
                        host,
                        ActorNotice::Event {
                            generation,
                            event: WebEvent::ProcessFailed {
                                kind: 1,
                                description: String::from(
                                    "Chromium could not pause a non-network navigation before commit",
                                ),
                            },
                        },
                    );
                    return;
                }
                let target = match reply {
                    GateReply::Navigation(WebNavigationVerdict::Proceed) => Some(url),
                    GateReply::Navigation(WebNavigationVerdict::Cancel) => None,
                    GateReply::Navigation(WebNavigationVerdict::CancelAndNavigateTo(target)) => {
                        Some(target)
                    }
                    GateReply::Resource(_) => None,
                };
                let Some(target) = target else {
                    return;
                };
                self.navigate(host, &target);
            }
        }
    }

    fn close_host(&mut self, worker: &WorkerCtx, host: HostId) {
        self.cancelled_hosts.insert(host);
        if self
            .pending_page_create
            .is_some_and(|(owner, _)| owner == host)
        {
            self.pending_page_create = None;
        }
        if self
            .pending_controller_setup
            .as_ref()
            .is_some_and(|(owner, _, _)| *owner == host)
        {
            self.pending_controller_setup = None;
        }
        let Some(page) = self.hosts.remove(&host) else {
            return;
        };
        let requests = self
            .pending_fetches
            .iter()
            .filter(|(_, pending)| pending.hosts.contains_key(&host))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in requests {
            self.fail_fetch(&key, "Aborted");
        }
        if let Some(target) = page.target {
            self.targets.remove(&target);
            self.target_sessions.remove(&target);
            self.session_targets
                .retain(|_, owner_target| owner_target != &target);
            let _ = self.notify(
                None,
                "Target.closeTarget",
                serde_json::json!({"targetId": target}),
            );
        }
        let child_targets = self
            .session_targets
            .iter()
            .filter(|(session, _)| {
                self.sessions
                    .get(*session)
                    .is_some_and(|(owner, _)| *owner == host)
            })
            .map(|(_, target)| target.clone())
            .collect::<Vec<_>>();
        for target in child_targets {
            self.targets.remove(&target);
            self.target_sessions.remove(&target);
            self.service_worker_targets.remove(&target);
            self.service_worker_clients.remove(&target);
        }
        self.session_targets.retain(|session, _| {
            self.sessions
                .get(session)
                .is_none_or(|(owner, _)| *owner != host)
        });
        self.sessions.retain(|_, (owner, _)| *owner != host);
        self.contexts.retain(|_, (owner, _)| *owner != host);
        self.context_worlds
            .retain(|(session, _), _| self.sessions.contains_key(session));
        self.frame_hosts.retain(|_, owner| *owner != host);
        self.frame_sessions
            .retain(|frame, _| self.frame_hosts.contains_key(frame));
        self.frame_urls
            .retain(|frame, _| self.frame_hosts.contains_key(frame));
        self.session_root_frames
            .retain(|session, _| self.sessions.contains_key(session));
        self.pending_create.retain(|_, (owner, _)| *owner != host);
        self.pending_install.remove(&host);
        if self.hosts.is_empty() {
            self.close_browser(worker);
        }
    }

    fn forget_browser(&mut self, worker: &WorkerCtx) {
        self.close_browser(worker);
        self.targets.clear();
        self.sessions.clear();
        self.contexts.clear();
        self.frame_hosts.clear();
        self.frame_parents.clear();
        self.frame_sessions.clear();
        self.frame_urls.clear();
        self.session_root_frames.clear();
    }

    fn create_browser_page(
        &mut self,
        worker: &WorkerCtx,
        host: HostId,
        generation: u64,
    ) -> Result<(), String> {
        self.pending_page_create = Some((host, generation));
        let created = self.call(
            None,
            "Target.createTarget",
            serde_json::json!({"url": "about:blank"}),
        );
        self.pending_page_create = None;
        let target = created?
            .get("targetId")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| String::from("Chromium did not return the preview target id"))?
            .to_owned();
        self.targets.insert(target.clone(), host);
        self.pending_create
            .insert(target.clone(), (host, generation));
        if let Some(page) = self.hosts.get_mut(&host) {
            page.target = Some(target.clone());
            page.main_context = None;
            page.latest_navigation_request = None;
        }
        self.pending_controller_setup = Some((host, generation, target.clone()));
        let result = self.await_page_attachment(worker, &target, host, generation);
        if self.pending_controller_setup.as_ref().is_some_and(
            |(owner, pending_generation, pending_target)| {
                *owner == host && *pending_generation == generation && pending_target == &target
            },
        ) {
            self.pending_controller_setup = None;
        }
        result
    }

    fn await_page_attachment(
        &mut self,
        _worker: &WorkerCtx,
        target: &str,
        host: HostId,
        generation: u64,
    ) -> Result<(), String> {
        loop {
            self.handle_protocol_events();
            if self.cancelled_hosts.contains(&host)
                || self
                    .hosts
                    .get(&host)
                    .is_none_or(|page| page.generation != generation)
            {
                return Err(String::from(
                    "the web seat closed while Chromium attached its page",
                ));
            }
            if self.target_sessions.contains_key(target) {
                return Ok(());
            }
            match self.input.recv_timeout(CDP_POLL) {
                Ok(ActorInput::Protocol { epoch, message }) => {
                    if epoch != self.browser_epoch {
                        continue;
                    }
                    if message
                        .get("id")
                        .and_then(serde_json::Value::as_u64)
                        .is_some()
                    {
                        self.handle_protocol_response(message);
                    } else {
                        self.handle_protocol_event(message);
                    }
                }
                Ok(ActorInput::ProtocolClosed { epoch, reason }) => {
                    if epoch == self.browser_epoch {
                        return Err(reason);
                    }
                }
                Ok(ActorInput::Command(ActorCommand::Shutdown)) => {
                    consume_actor_cancellation();
                    self.running = false;
                    if let Some(browser) = self.browser.as_mut() {
                        let _ = browser.child.kill();
                    }
                    return Err(String::from("the Chromium actor is shutting down"));
                }
                Ok(ActorInput::Command(ActorCommand::CancelCreate {
                    host: cancel_host,
                    generation: cancel_generation,
                })) => {
                    consume_actor_cancellation();
                    if cancel_host == host && cancel_generation == generation {
                        self.close_controller_target(host, generation, target);
                        return Err(String::from(
                            "the web controller was canceled while Chromium attached its page",
                        ));
                    }
                    self.deferred.push_back(ActorCommand::CancelCreate {
                        host: cancel_host,
                        generation: cancel_generation,
                    });
                }
                Ok(ActorInput::Command(ActorCommand::Close { host: closing_host })) => {
                    consume_actor_cancellation();
                    self.cancelled_hosts.insert(closing_host);
                    if closing_host == host {
                        self.close_controller_target(host, generation, target);
                        self.deferred
                            .push_front(ActorCommand::Close { host: closing_host });
                        return Err(String::from(
                            "the web seat closed while Chromium attached its page",
                        ));
                    }
                    if let Some(target) = self
                        .hosts
                        .get(&closing_host)
                        .and_then(|page| page.target.as_deref())
                    {
                        let _ = self.notify(
                            None,
                            "Target.closeTarget",
                            serde_json::json!({"targetId": target}),
                        );
                    }
                    self.deferred
                        .push_front(ActorCommand::Close { host: closing_host });
                }
                Ok(ActorInput::Command(ActorCommand::GateVerdict {
                    host,
                    generation,
                    key,
                    reply,
                })) => self.gate_verdict(host, generation, key, reply),
                Ok(ActorInput::Command(ActorCommand::StopLoading { host })) => {
                    self.stop_loading(host);
                }
                Ok(ActorInput::Command(command)) => {
                    if interrupts_cdp_write(&command) {
                        consume_actor_cancellation();
                    }
                    self.deferred.push_back(command);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(browser) = self.browser.as_mut()
                        && let Some(status) = browser
                            .child
                            .try_wait()
                            .map_err(|error| error.to_string())?
                    {
                        return Err(format!("Chromium exited with {status}"));
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(String::from("the Chromium actor channel closed"));
                }
            }
        }
    }

    fn configure_page(
        &mut self,
        host: HostId,
        target: &str,
        session: &str,
        generation: u64,
        waiting: bool,
    ) -> Result<(), String> {
        self.call(Some(session), "Page.enable", serde_json::json!({}))?;
        self.call(Some(session), "Runtime.enable", serde_json::json!({}))?;
        self.call(Some(session), "DOM.enable", serde_json::json!({}))?;
        self.call(Some(session), "ServiceWorker.enable", serde_json::json!({}))?;
        self.call(Some(session), "Network.enable", serde_json::json!({}))?;
        self.call(
            Some(session),
            "Fetch.enable",
            serde_json::json!({
                "patterns": [{"urlPattern": "*", "requestStage": "Request"}]
            }),
        )?;
        self.call(
            Some(session),
            "Page.setLifecycleEventsEnabled",
            serde_json::json!({"enabled": true}),
        )?;

        let (bounds, scale, scheme, rules) = {
            let page = self.hosts.get(&host).ok_or_else(|| {
                String::from("the web seat closed while Chromium was configuring it")
            })?;
            (
                page.bounds,
                page.scale,
                page.color_scheme,
                page.rules.clone(),
            )
        };
        self.apply_bounds(host, session, bounds, scale)?;
        if let Some(scheme) = scheme {
            self.apply_color_scheme(session, scheme)?;
        }

        let policy_token = random_nonce()?;
        let binding = format!("folioNavGate_{}", &policy_token[..16]);
        let permit = format!("__folioNavPermit_{}", &policy_token[16..32]);
        let ime_token = random_nonce()?;
        let ime_binding = format!("folioImeSignal_{}", &ime_token[..16]);
        if let Some(page) = self.hosts.get_mut(&host) {
            page.latest_navigation_request = None;
            page.ime_binding = Some(ime_binding);
            page.ime_token = Some(ime_token);
            page.last_ime_cursor = None;
        }
        self.call(
            Some(session),
            "Runtime.addBinding",
            serde_json::json!({"name": binding}),
        )?;
        let bootstrap = navigation_bootstrap(&binding, &permit, &policy_token);
        self.call(
            Some(session),
            "Page.addScriptToEvaluateOnNewDocument",
            serde_json::json!({"source": bootstrap}),
        )?;
        let navigation_hook = self.call(
            Some(session),
            "Runtime.evaluate",
            serde_json::json!({"expression": bootstrap, "returnByValue": true}),
        )?;
        if let Some(error) = navigation_hook_install_failure(&navigation_hook) {
            return Err(error);
        }
        self.install_ime_signals(host, session)?;
        self.call(
            Some(session),
            "Target.setAutoAttach",
            serde_json::json!({
                "autoAttach": true,
                "waitForDebuggerOnStart": true,
                "flatten": true,
                "filter": [
                    {"type": "iframe", "exclude": false},
                    {"type": "worker", "exclude": false},
                    {"type": "shared_worker", "exclude": false},
                    {"type": "page", "exclude": false},
                    {"type": "service_worker", "exclude": false}
                ]
            }),
        )?;
        self.call(
            None,
            "Browser.setDownloadBehavior",
            serde_json::json!({"behavior": "deny"}),
        )?;
        let frame_tree = self.call(Some(session), "Page.getFrameTree", serde_json::json!({}))?;
        if let Some(tree) = frame_tree.get("frameTree") {
            if let Some(root) = tree
                .pointer("/frame/id")
                .and_then(serde_json::Value::as_str)
            {
                self.session_root_frames
                    .insert(session.to_owned(), root.to_owned());
            }
            index_frame_tree(
                tree,
                host,
                &mut self.frame_hosts,
                &mut self.frame_parents,
                &mut self.frame_sessions,
                &mut self.frame_urls,
                session,
            );
        }
        let main_frame = frame_tree
            .pointer("/frameTree/frame/id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| String::from("Chromium did not return the preview's main frame"))?
            .to_owned();
        let tab_id = self.set_websocket_policy(host, target, &rules)?;
        let report = install_report();
        if let Some(page) = self.hosts.get_mut(&host) {
            page.main_frame = Some(main_frame.clone());
            page.policy_token = Some(policy_token);
            page.navigation_binding = Some(binding);
            page.navigation_permit = Some(permit);
            page.tab_id = Some(tab_id);
            page.browser_process_id = self
                .browser
                .as_ref()
                .map_or(0, |browser| browser.child.id());
            page.last_url = String::from("about:blank");
        }
        self.reconcile_main_context(host, session, &main_frame);
        self.frame_hosts.insert(main_frame, host);
        if waiting {
            self.call(
                Some(session),
                "Runtime.runIfWaitingForDebugger",
                serde_json::json!({}),
            )?;
        }
        self.handle_protocol_events();
        self.post(host, ActorNotice::Installed { generation, report });
        self.post(
            host,
            ActorNotice::Controller {
                generation,
                error: None,
            },
        );
        Ok(())
    }

    fn apply_bounds(
        &mut self,
        host: HostId,
        session: &str,
        bounds: (i32, i32, u32, u32),
        scale: f64,
    ) -> Result<(), String> {
        let css_width = (f64::from(bounds.2) / scale).round().max(1.0) as u32;
        let css_height = (f64::from(bounds.3) / scale).round().max(1.0) as u32;
        self.call(
            Some(session),
            "Emulation.setDeviceMetricsOverride",
            serde_json::json!({
                "width": css_width,
                "height": css_height,
                "deviceScaleFactor": scale,
                "mobile": false
            }),
        )?;
        if let Some(page) = self.hosts.get_mut(&host) {
            page.bounds = bounds;
            page.scale = scale;
        }
        Ok(())
    }

    fn apply_color_scheme(&mut self, session: &str, scheme: WebColorScheme) -> Result<(), String> {
        let value = match scheme {
            WebColorScheme::Light => "light",
            WebColorScheme::Dark => "dark",
        };
        self.call(
            Some(session),
            "Emulation.setEmulatedMedia",
            serde_json::json!({
                "features": [{"name": "prefers-color-scheme", "value": value}]
            }),
        )?;
        Ok(())
    }

    fn set_websocket_policy(
        &mut self,
        host: HostId,
        target: &str,
        rules: &str,
    ) -> Result<i32, String> {
        let deny = denies_websocket(rules)?;
        let rule_base = host
            .checked_mul(2)
            .and_then(|value| value.checked_add(1))
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| String::from("the page id is too large for a DNR session rule"))?;
        let control = self
            .extension_session
            .clone()
            .ok_or_else(|| String::from("Chromium's policy extension is not attached"))?;
        let target_json = serde_json::to_string(target).map_err(|error| error.to_string())?;
        let expression =
            format!("window.folioSetWebSocketPolicy({target_json},{rule_base},{deny})");
        let result = self.call(
            Some(&control),
            "Runtime.evaluate",
            serde_json::json!({
                "expression": expression,
                "awaitPromise": true,
                "returnByValue": true
            }),
        )?;
        if let Some(exception) = result.get("exceptionDetails") {
            return Err(format!(
                "Chromium could not install the WebSocket gate: {exception}"
            ));
        }
        let tab_id = result
            .pointer("/result/value/tabId")
            .and_then(serde_json::Value::as_i64)
            .and_then(|tab| i32::try_from(tab).ok())
            .ok_or_else(|| String::from("Chromium did not return the preview target's tab id"))?;
        if let Some(page) = self.hosts.get_mut(&host) {
            page.tab_id = Some(tab_id);
            page.rules = rules.to_owned();
        }
        Ok(tab_id)
    }

    fn start_screencast(&mut self, host: HostId, session: &str) -> Result<(), String> {
        let should_start = self
            .hosts
            .get(&host)
            .is_some_and(|page| page.visible && page.installed && !page.screencast_started);
        if !should_start {
            return Ok(());
        }
        let (generation, page) = self.hosts.get(&host).map_or((0, None), |host_page| {
            (host_page.generation, host_page.page)
        });
        trace_browser_startup(
            self.worker_ctx,
            &format!("screencast start enter generation={generation} page={page:?}"),
        );
        self.call(
            Some(session),
            "Page.startScreencast",
            serde_json::json!({"format": "png", "everyNthFrame": 1}),
        )?;
        if let Some(page) = self.hosts.get_mut(&host) {
            page.screencast_started = true;
        }
        trace_browser_startup(
            self.worker_ctx,
            &format!("screencast start reply generation={generation} page={page:?}"),
        );
        Ok(())
    }

    /// A moved page's old stream may stay quiet when its document does not repaint. Restart it
    /// only after the destination window has supplied the bounds and scale for the new owner.
    fn start_screencast_for_page(&mut self, host: HostId, session: &str) -> Result<(), String> {
        let Some(page) = self.hosts.get(&host) else {
            return Ok(());
        };
        if !page.screencast_refresh_pending {
            return self.start_screencast(host, session);
        }
        if !page.destination_bounds_ready || !page.visible || !page.installed {
            return Ok(());
        }

        if let Some(page) = self.hosts.get_mut(&host) {
            page.screencast_refresh_pending = false;
        }
        if let Err(error) = self.start_screencast(host, session) {
            if let Some(page) = self.hosts.get_mut(&host) {
                page.screencast_refresh_pending = true;
            }
            return Err(error);
        }
        Ok(())
    }

    fn evaluate(&mut self, session: &str, expression: String) -> Result<serde_json::Value, String> {
        let result = self.call(
            Some(session),
            "Runtime.evaluate",
            serde_json::json!({"expression": expression, "returnByValue": true}),
        )?;
        if let Some(exception) = result.get("exceptionDetails") {
            return Err(format!("page evaluation failed: {exception}"));
        }
        Ok(result
            .pointer("/result/value")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }

    fn root_session(&self, host: HostId) -> Option<(String, u64)> {
        let page = self.hosts.get(&host)?;
        Some((page.session.clone()?, page.generation))
    }

    fn stop_screencast(&mut self, host: HostId) {
        let (started, session) = match self.hosts.get(&host) {
            Some(page) => (page.screencast_started, page.session.clone()),
            None => return,
        };
        if started {
            if let Some(session) = session {
                let _ = self.notify(Some(&session), "Page.stopScreencast", serde_json::json!({}));
            }
            if let Some(page) = self.hosts.get_mut(&host) {
                page.screencast_started = false;
            }
        }
    }
}

static ACTOR_SENDER: OnceLock<Mutex<Option<ActorHandle>>> = OnceLock::new();
static ENVIRONMENT_EPOCH: AtomicU64 = AtomicU64::new(0);
static ACTOR_SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
static PENDING_ACTOR_CANCELLATIONS: AtomicU64 = AtomicU64::new(0);

fn actor_sender() -> &'static Mutex<Option<ActorHandle>> {
    ACTOR_SENDER.get_or_init(|| Mutex::new(None))
}

pub(crate) fn start() -> Result<ActorHandle, String> {
    let mut slot = actor_sender()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if slot.is_none() {
        ACTOR_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        PENDING_ACTOR_CANCELLATIONS.store(0, Ordering::SeqCst);
        let (actor_tx, actor_rx) = mpsc::channel();
        let worker_tx = actor_tx.clone();
        let worker = crate::spawn_at_priority(
            "bt-linux-web-actor",
            ThreadPriority::BelowNormal,
            move |ctx| run(ctx, worker_tx, actor_rx),
        )
        .map_err(|error| format!("could not start the Chromium actor: {error}"))?;
        crate::linux_process::register_helper(worker);
        *slot = Some(ActorHandle(actor_tx));
    }
    slot.as_ref()
        .cloned()
        .ok_or_else(|| String::from("the Chromium actor worker did not start"))
}

pub(crate) fn forget_web_environment() {
    ENVIRONMENT_EPOCH.fetch_add(1, Ordering::SeqCst);
    if let Some(sender) = actor_sender()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
    {
        let _ = sender.send(ActorCommand::ForgetEnvironment);
    }
}

pub(crate) fn web_environment_epoch() -> u64 {
    ENVIRONMENT_EPOCH.load(Ordering::SeqCst)
}

pub(crate) fn shutdown() {
    ACTOR_SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
    if let Some(sender) = actor_sender()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    {
        let _ = sender.send(ActorCommand::Shutdown);
    }
}

fn run(worker: &WorkerCtx, sender: Sender<ActorInput>, receiver: Receiver<ActorInput>) {
    let mut actor = Actor::new(worker, sender, receiver);
    while actor.running {
        actor.handle_protocol_events();
        if !actor.running {
            break;
        }
        if let Some(command) = actor.deferred.pop_front() {
            actor.handle_command(worker, command);
            continue;
        }
        match actor.input.recv() {
            Ok(ActorInput::Command(command)) => {
                if interrupts_cdp_write(&command) {
                    consume_actor_cancellation();
                }
                actor.handle_command(worker, command);
            }
            Ok(ActorInput::Protocol { epoch, message }) => {
                if epoch == actor.browser_epoch {
                    if message
                        .get("id")
                        .and_then(serde_json::Value::as_u64)
                        .is_some()
                    {
                        actor.handle_protocol_response(message);
                    } else {
                        actor.protocol_events.push_back(message);
                    }
                }
            }
            Ok(ActorInput::ProtocolClosed { epoch, reason }) => {
                actor.reader_closed(worker, epoch, reason);
            }
            Err(_) => actor.running = false,
        }
    }
    actor.close_browser(worker);
}

pub(crate) fn browser_version() -> Result<String, String> {
    let path = find_browser()?;
    Ok(format!("Chromium ({})", path.display()))
}

fn find_browser() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("FOLIO_CHROMIUM_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "FOLIO_CHROMIUM_PATH is not a file: {}",
            path.display()
        ));
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&path) {
        for name in [
            "chromium",
            "chromium-browser",
            "google-chrome-stable",
            "google-chrome",
        ] {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(String::from(
        "a full Chromium browser is required for the Linux web preview",
    ))
}

fn random_nonce() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut bytes))
        .map_err(|error| format!("could not create a private navigation token: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn denies_websocket(rules: &str) -> Result<bool, String> {
    let rules: serde_json::Value = serde_json::from_str(rules)
        .map_err(|error| format!("the app's web resource rules are invalid JSON: {error}"))?;
    let rules = rules
        .as_array()
        .ok_or_else(|| String::from("the app's web resource rules are not an array"))?;
    Ok(rules.iter().any(|rule| {
        matches!(
            rule.pointer("/trigger/url-filter")
                .and_then(serde_json::Value::as_str),
            Some("^ws://" | "^wss://")
        )
    }))
}

fn install_report() -> WebInstallReport {
    WebInstallReport {
        guards: WebGuards {
            script_dialogs: true,
            frame_navigation: true,
            resource_requests: true,
        },
        unapplied: Vec::new(),
    }
}

fn is_page_attach_event(message: &serde_json::Value) -> bool {
    message.get("method").and_then(serde_json::Value::as_str) == Some("Target.attachedToTarget")
        && message
            .pointer("/params/targetInfo/type")
            .and_then(serde_json::Value::as_str)
            == Some("page")
}

#[cfg(test)]
mod main_frame_context_tests {
    use super::{ContextWorld, default_context_for_frame};
    use std::collections::HashMap;

    #[test]
    fn selects_only_the_owned_default_context_for_the_exact_main_frame() {
        let contexts = HashMap::from([
            (
                (String::from("root-session"), 10),
                (7, String::from("main-frame")),
            ),
            (
                (String::from("root-session"), 11),
                (7, String::from("main-frame")),
            ),
            (
                (String::from("root-session"), 12),
                (7, String::from("child-frame")),
            ),
            (
                (String::from("other-session"), 13),
                (7, String::from("main-frame")),
            ),
            (
                (String::from("root-session"), 14),
                (8, String::from("main-frame")),
            ),
        ]);
        let worlds = HashMap::from([
            (
                (String::from("root-session"), 10),
                ContextWorld {
                    name: String::new(),
                    is_default: true,
                },
            ),
            (
                (String::from("root-session"), 11),
                ContextWorld {
                    name: String::new(),
                    is_default: false,
                },
            ),
            (
                (String::from("root-session"), 12),
                ContextWorld {
                    name: String::new(),
                    is_default: true,
                },
            ),
            (
                (String::from("other-session"), 13),
                ContextWorld {
                    name: String::new(),
                    is_default: true,
                },
            ),
            (
                (String::from("root-session"), 14),
                ContextWorld {
                    name: String::new(),
                    is_default: true,
                },
            ),
        ]);

        assert_eq!(
            default_context_for_frame(&contexts, &worlds, "root-session", 7, "main-frame"),
            Some(10)
        );
        assert_eq!(
            default_context_for_frame(&contexts, &worlds, "root-session", 7, "child-frame"),
            Some(12)
        );
        assert_eq!(
            default_context_for_frame(&contexts, &worlds, "other-session", 7, "main-frame"),
            Some(13)
        );
        assert_eq!(
            default_context_for_frame(&contexts, &worlds, "root-session", 8, "main-frame"),
            Some(14)
        );
    }

    #[test]
    fn ambiguous_default_contexts_are_not_guessed() {
        let contexts = HashMap::from([
            (
                (String::from("root-session"), 10),
                (7, String::from("main-frame")),
            ),
            (
                (String::from("root-session"), 11),
                (7, String::from("main-frame")),
            ),
        ]);
        let worlds = HashMap::from([
            (
                (String::from("root-session"), 10),
                ContextWorld {
                    name: String::new(),
                    is_default: true,
                },
            ),
            (
                (String::from("root-session"), 11),
                ContextWorld {
                    name: String::new(),
                    is_default: true,
                },
            ),
        ]);

        assert_eq!(
            default_context_for_frame(&contexts, &worlds, "root-session", 7, "main-frame"),
            None
        );
    }
}

#[cfg(test)]
mod navigation_hook_tests {
    use super::navigation_hook_install_failure;

    #[test]
    fn a_navigation_hook_requires_an_explicit_success_result() {
        assert_eq!(
            navigation_hook_install_failure(&serde_json::json!({
                "result": {"value": {"installed": true, "scope": "main-frame"}}
            })),
            None
        );
        let missing_api = navigation_hook_install_failure(&serde_json::json!({
            "result": {"value": {"installed": false, "reason": "the browser does not expose the Navigation API"}}
        }))
        .expect("missing required navigation API must fail installation");
        assert!(missing_api.contains("does not expose the Navigation API"));
        let exception = navigation_hook_install_failure(&serde_json::json!({
            "exceptionDetails": {"text": "TypeError"},
            "result": {"value": {"installed": true}}
        }))
        .expect("a bootstrap exception cannot stand as an installed guard");
        assert!(exception.contains("bootstrap exception: TypeError"));
    }
}

#[cfg(test)]
mod policy_page_start_tests {
    use super::{document_load_has_completed, keep_policy_page_attachment};

    #[test]
    fn manual_policy_page_attach_event_survives_before_its_response() {
        assert!(keep_policy_page_attachment(true, None, "manual-session"));
        assert!(keep_policy_page_attachment(
            false,
            Some("manual-session"),
            "manual-session"
        ));
        assert!(!keep_policy_page_attachment(
            false,
            Some("manual-session"),
            "duplicate-session"
        ));
    }

    #[test]
    fn a_completed_policy_document_does_not_need_a_late_load_event() {
        assert!(document_load_has_completed(&serde_json::json!({
            "result": {"value": "complete"}
        })));
        assert!(!document_load_has_completed(&serde_json::json!({
            "result": {"value": "interactive"}
        })));
    }
}

#[cfg(test)]
static ACTOR_PIPE_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::{
        ACTOR_PIPE_TEST_LOCK, ACTOR_SHUTDOWN_REQUESTED, Actor, ActorCommand, ActorHandle,
        ActorInput, ActorNotice, BootRequest, BrowserProcess, CdpWriter, GateKey, HostInbox,
        PENDING_ACTOR_CANCELLATIONS, consume_actor_cancellation, make_pipe, map_rect_through_quad,
        navigation_api_verdict_is_current, set_nonblocking,
    };
    use std::collections::{HashMap, HashSet};
    use std::io::{BufRead, Read, Write};
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;

    fn read_cdp_command(peer: &mut impl BufRead) -> serde_json::Value {
        let mut frame = Vec::new();
        peer.read_until(0, &mut frame)
            .expect("read the actor's null-terminated CDP command");
        assert_eq!(frame.pop(), Some(0), "CDP command has a terminator");
        serde_json::from_slice(&frame).expect("CDP command is JSON")
    }

    #[test]
    fn a_reversed_navigation_verdict_cannot_replace_the_latest_main_frame_request() {
        let latest = GateKey::NavigationApi {
            session: String::from("root-session"),
            context: 12,
            request: String::from("2"),
            token: String::from("policy-token"),
            url: String::from("data:text/html,newer"),
            cancelable: true,
        };
        let older = GateKey::NavigationApi {
            session: String::from("root-session"),
            context: 12,
            request: String::from("1"),
            token: String::from("policy-token"),
            url: String::from("data:text/html,older"),
            cancelable: true,
        };

        assert!(navigation_api_verdict_is_current(
            5,
            5,
            Some("root-session"),
            Some(12),
            Some("policy-token"),
            Some(&latest),
            &latest,
        ));
        assert!(!navigation_api_verdict_is_current(
            5,
            5,
            Some("root-session"),
            Some(12),
            Some("policy-token"),
            Some(&latest),
            &older,
        ));
        assert!(!navigation_api_verdict_is_current(
            5,
            5,
            Some("root-session"),
            Some(13),
            Some("policy-token"),
            Some(&latest),
            &latest,
        ));
        assert!(!navigation_api_verdict_is_current(
            5,
            5,
            Some("replacement-session"),
            Some(12),
            Some("policy-token"),
            Some(&latest),
            &latest,
        ));
        assert!(!navigation_api_verdict_is_current(
            5,
            6,
            Some("root-session"),
            Some(12),
            Some("policy-token"),
            Some(&latest),
            &latest,
        ));
        assert!(!navigation_api_verdict_is_current(
            5,
            5,
            Some("root-session"),
            Some(12),
            Some("replacement-token"),
            Some(&latest),
            &latest,
        ));
        assert!(!navigation_api_verdict_is_current(
            5,
            5,
            Some("root-session"),
            Some(12),
            Some("policy-token"),
            None,
            &latest,
        ));
    }

    #[test]
    fn inbox_coalesces_frames_by_generation_and_sequence_without_dropping_controls() {
        let inbox = HostInbox::default();
        inbox.push(ActorNotice::Environment {
            generation: 4,
            error: None,
            browser_process_id: Some(42),
        });
        let page = crate::PageVisual { tab: 8, seat: 3 };
        for sequence in 1..=100 {
            inbox.push(ActorNotice::Frame(crate::WebFrame {
                page,
                generation: 7,
                sequence,
                bounds_px: (10, 20, 640, 360),
                visible: true,
                width_px: 640,
                height_px: 360,
                bgra: std::sync::Arc::from(vec![sequence as u8; 4]),
            }));
        }
        inbox.push(ActorNotice::NavigationRequest {
            key: GateKey::Fetch {
                session: String::from("session-a"),
                request: String::from("request-a"),
            },
            generation: 7,
            uri: String::from("https://example.test/next"),
        });
        inbox.push(ActorNotice::Frame(crate::WebFrame {
            page,
            generation: 6,
            sequence: 500,
            bounds_px: (0, 0, 1, 1),
            visible: false,
            width_px: 1,
            height_px: 1,
            bgra: std::sync::Arc::from(vec![0; 4]),
        }));
        inbox.push(ActorNotice::ResourceRequest {
            key: GateKey::Fetch {
                session: String::from("session-b"),
                request: String::from("request-b"),
            },
            generation: 7,
            uri: String::from("https://example.test/resource"),
        });
        inbox.push(ActorNotice::Frame(crate::WebFrame {
            page,
            generation: 8,
            sequence: 1,
            bounds_px: (10, 20, 640, 360),
            visible: true,
            width_px: 640,
            height_px: 360,
            bgra: std::sync::Arc::from(vec![8; 4]),
        }));
        let notices = inbox.take();
        let frames = notices
            .iter()
            .filter_map(|notice| match notice {
                ActorNotice::Frame(frame) => Some(frame),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(frames.len(), 1);
        assert_eq!((frames[0].generation, frames[0].sequence), (8, 1));
        assert_eq!(frames[0].page, page);
        assert_eq!(
            notices
                .iter()
                .filter(|notice| matches!(
                    notice,
                    ActorNotice::Environment { .. }
                        | ActorNotice::NavigationRequest { .. }
                        | ActorNotice::ResourceRequest { .. }
                ))
                .count(),
            3
        );
        assert!(matches!(
            notices.first(),
            Some(ActorNotice::Environment { .. })
        ));
        assert!(matches!(
            notices.get(2),
            Some(ActorNotice::NavigationRequest { .. })
        ));
        assert!(matches!(
            notices.get(3),
            Some(ActorNotice::ResourceRequest { .. })
        ));
    }

    #[test]
    fn moving_a_page_keeps_frame_sequences_above_the_host_watermark() {
        let (finished, wait_finished) = mpsc::channel();
        let worker = crate::spawn_at_priority(
            "bt-linux-web-move-page-sequence-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| {
                let (sender, receiver) = mpsc::channel();
                let mut actor = Actor::new(worker, sender, receiver);
                let host = 17;
                let page = crate::PageVisual { tab: 4, seat: 2 };
                actor.hosts.insert(
                    host,
                    super::HostPage {
                        inbox: std::sync::Arc::new(HostInbox::default()),
                        wake: std::sync::Arc::new(|| {}),
                        folder: std::env::temp_dir(),
                        generation: 3,
                        page: Some(page),
                        rules: String::new(),
                        color_scheme: None,
                        bounds: (0, 0, 640, 480),
                        scale: 1.0,
                        visible: false,
                        sequence: 6,
                        screencast_started: false,
                        screencast_frame_reported: true,
                        screencast_refresh_pending: false,
                        destination_bounds_ready: true,
                        target: None,
                        session: None,
                        tab_id: None,
                        main_frame: None,
                        policy_token: None,
                        navigation_binding: None,
                        navigation_permit: None,
                        latest_navigation_request: None,
                        ime_binding: None,
                        ime_token: None,
                        last_ime_cursor: None,
                        main_context: None,
                        browser_process_id: 0,
                        installed: true,
                        main_loader: None,
                        main_status: 0,
                        find_term: String::new(),
                        find_case_sensitive: false,
                        find_count: 0,
                        find_active: 0,
                        last_url: String::new(),
                    },
                );

                let moved_page = crate::PageVisual { tab: 4, seat: 2 };
                actor.move_page(host, moved_page, false);
                let host_page = actor
                    .hosts
                    .get(&host)
                    .expect("the moved host remains installed");
                finished
                    .send((
                        host_page.page,
                        host_page.sequence,
                        host_page.screencast_refresh_pending,
                        host_page.destination_bounds_ready,
                    ))
                    .expect("report the moved address, sequence, and destination-frame gate");
            },
        )
        .expect("start the actor move-page test worker");

        assert_eq!(
            wait_finished.recv().expect("the move-page test finished"),
            (Some(crate::PageVisual { tab: 4, seat: 2 }), 6, true, false),
            "rehosting starts a new screencast, not a new frame-sequence epoch"
        );
        worker.join().expect("the move-page worker returned");
    }

    #[test]
    fn a_moved_static_page_gets_a_new_frame_after_same_size_destination_bounds() {
        let _lock = ACTOR_PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ACTOR_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        PENDING_ACTOR_CANCELLATIONS.store(0, Ordering::SeqCst);
        let (finished, wait_finished) = mpsc::channel();
        let worker = crate::spawn_at_priority(
            "bt-linux-web-move-page-refresh-test",
            crate::ThreadPriority::BelowNormal,
            move |worker_ctx| {
                let [pipe_read, pipe_write] = make_pipe().expect("make the fake CDP pipe");
                let inbox = std::sync::Arc::new(HostInbox::default());
                let wake_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
                let wake_observer = std::sync::Arc::clone(&wake_count);
                let (sender, receiver) = mpsc::channel();
                let mut actor = Actor::new(worker_ctx, sender, receiver);
                actor.cdp = Some(CdpWriter {
                    file: std::fs::File::from(pipe_write),
                    next_id: 1,
                    poisoned: false,
                    write_blocked: None,
                });
                actor
                    .protocol_responses
                    .insert(3, serde_json::json!({"id": 3, "result": {}}));
                actor
                    .protocol_responses
                    .insert(4, serde_json::json!({"id": 4, "result": {}}));

                let host = 23;
                let page = crate::PageVisual { tab: 9, seat: 4 };
                let bounds = (179, 368, 428, 377);
                actor.hosts.insert(
                    host,
                    super::HostPage {
                        inbox: inbox.clone(),
                        wake: std::sync::Arc::new(move || {
                            wake_observer.fetch_add(1, Ordering::SeqCst);
                        }),
                        folder: std::env::temp_dir(),
                        generation: 7,
                        page: Some(page),
                        rules: String::new(),
                        color_scheme: None,
                        bounds,
                        scale: 1.0,
                        visible: true,
                        sequence: 6,
                        screencast_started: true,
                        screencast_frame_reported: true,
                        screencast_refresh_pending: false,
                        destination_bounds_ready: true,
                        target: None,
                        session: Some(String::from("root-session")),
                        tab_id: None,
                        main_frame: None,
                        policy_token: None,
                        navigation_binding: None,
                        navigation_permit: None,
                        latest_navigation_request: None,
                        ime_binding: None,
                        ime_token: None,
                        last_ime_cursor: None,
                        main_context: None,
                        browser_process_id: 0,
                        installed: true,
                        main_loader: None,
                        main_status: 0,
                        find_term: String::new(),
                        find_case_sensitive: false,
                        find_count: 0,
                        find_active: 0,
                        last_url: String::new(),
                    },
                );
                actor
                    .sessions
                    .insert(String::from("root-session"), (host, true));
                inbox.push(ActorNotice::Frame(crate::WebFrame {
                    page,
                    generation: 7,
                    sequence: 6,
                    bounds_px: bounds,
                    visible: true,
                    width_px: 428,
                    height_px: 377,
                    bgra: std::sync::Arc::from(vec![6; 428 * 377 * 4]),
                }));
                inbox.push(ActorNotice::Controller {
                    generation: 7,
                    error: None,
                });

                actor.move_page(host, page, true);
                let mut png = std::io::Cursor::new(Vec::new());
                image::DynamicImage::new_rgba8(bounds.2, bounds.3)
                    .write_to(&mut png, image::ImageFormat::Png)
                    .expect("encode the static page frame");
                let encoded = base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    png.get_ref(),
                );
                let frame_notice = |session_id| {
                    serde_json::json!({
                        "sessionId": "root-session",
                        "params": {
                            "sessionId": session_id,
                            "data": encoded,
                        }
                    })
                };
                actor.screencast_frame(frame_notice(11));
                let after_old_stream = inbox.take();
                let old_frames = after_old_stream
                    .iter()
                    .filter(|notice| matches!(notice, ActorNotice::Frame(_)))
                    .count();
                let retained_control = after_old_stream
                    .iter()
                    .any(|notice| matches!(notice, ActorNotice::Controller { .. }));

                // The destination uses the same float rectangle and scale. A static document
                // does not paint merely because its owner changed, so the actor must restart the
                // producer after this target-bounds command rather than wait for changed geometry.
                actor.set_bounds(host, bounds, 1.0);
                actor.screencast_frame(frame_notice(12));
                let published = inbox.take().into_iter().find_map(|notice| match notice {
                    ActorNotice::Frame(frame) => Some((
                        frame.page,
                        frame.generation,
                        frame.sequence,
                        frame.bounds_px,
                        frame.width_px,
                        frame.height_px,
                    )),
                    _ => None,
                });
                let host_page = actor.hosts.get(&host).expect("the moved host remains live");
                let stream_started = host_page.screencast_started;
                let refresh_pending = host_page.screencast_refresh_pending;
                let sequence = host_page.sequence;
                let wakes = wake_count.load(Ordering::SeqCst);

                drop(actor.cdp.take());
                let mut peer = std::fs::File::from(pipe_read);
                let mut bytes = Vec::new();
                peer.read_to_end(&mut bytes)
                    .expect("read the actor's CDP commands");
                let commands = bytes
                    .split(|byte| *byte == 0)
                    .filter(|frame| !frame.is_empty())
                    .map(|frame| {
                        let message: serde_json::Value =
                            serde_json::from_slice(frame).expect("parse a CDP command");
                        message["method"]
                            .as_str()
                            .expect("command method")
                            .to_owned()
                    })
                    .collect::<Vec<_>>();
                finished
                    .send((
                        old_frames,
                        retained_control,
                        published,
                        stream_started,
                        refresh_pending,
                        sequence,
                        wakes,
                        commands,
                    ))
                    .expect("report the move and fresh target frame");
            },
        )
        .expect("start the moved-page frame test worker");

        let (
            old_frames,
            retained_control,
            published,
            stream_started,
            refresh_pending,
            sequence,
            wakes,
            commands,
        ) = wait_finished
            .recv()
            .expect("the moved-page frame test finished");
        assert_eq!(old_frames, 0, "source frames are discarded during handoff");
        assert!(
            retained_control,
            "moving a page does not discard other actor notices"
        );
        assert_eq!(
            published,
            Some((
                crate::PageVisual { tab: 9, seat: 4 },
                7,
                7,
                (179, 368, 428, 377),
                428,
                377,
            )),
            "the destination receives a fresh frame with its own unchanged geometry"
        );
        assert!(stream_started, "the moved page has a live screencast again");
        assert!(!refresh_pending, "the fresh frame producer was resumed");
        assert_eq!(sequence, 7, "the page's sequence watermark never resets");
        assert_eq!(wakes, 1, "the newly published frame wakes the application");
        let stop = commands
            .iter()
            .position(|method| method == "Page.stopScreencast")
            .expect("the source stream was stopped before rehosting");
        let bounds = commands
            .iter()
            .position(|method| method == "Emulation.setDeviceMetricsOverride")
            .expect("the destination bounds were applied");
        let start = commands
            .iter()
            .position(|method| method == "Page.startScreencast")
            .expect("the destination stream was restarted");
        assert!(
            stop < bounds && bounds < start,
            "the new stream must use the destination geometry: {commands:?}"
        );
        worker
            .join()
            .expect("the moved-page frame test worker returned");
    }

    #[test]
    fn old_reader_eof_is_ignored_after_a_deferred_new_boot() {
        let (finished, wait_finished) = mpsc::channel();
        let worker = crate::spawn_at_priority(
            "bt-linux-web-reader-epoch-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| {
                let hygiene = bt_pty::test_shell::Hygiene::new();
                let mut command = hygiene.command("/bin/sh", crate::quiet_command);
                command
                    .args(["-c", "sleep 30"])
                    .process_group(0)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                let child = command
                    .spawn()
                    .expect("start the replacement browser fixture");
                let folder = std::env::temp_dir().join("folio-web-reader-epoch-profile");
                let (sender, receiver) = mpsc::channel();
                let inbox = std::sync::Arc::new(HostInbox::default());
                let mut actor = Actor::new(worker, sender.clone(), receiver);
                actor.browser_epoch = 2;
                actor.browser = Some(BrowserProcess {
                    child,
                    profile: folder.clone(),
                    extension: std::env::temp_dir().join("folio-web-reader-epoch-extension"),
                    _reader: None,
                    output: None,
                });
                sender
                    .send(ActorInput::ProtocolClosed {
                        epoch: 1,
                        reason: String::from("the previous Chromium reader closed"),
                    })
                    .expect("queue the old reader EOF");
                actor.deferred.push_back(ActorCommand::Boot {
                    host: 19,
                    folder,
                    generation: 41,
                    rules: String::new(),
                    color_scheme: None,
                    inbox: inbox.clone(),
                    wake: std::sync::Arc::new(|| {}),
                });

                let command = actor
                    .deferred
                    .pop_front()
                    .expect("the replacement boot is deferred ahead of reader packets");
                actor.handle_command(worker, command);
                let packet = actor.input.recv().expect("receive the old reader EOF");
                let ActorInput::ProtocolClosed { epoch, reason } = packet else {
                    panic!("the queued packet is the old reader EOF");
                };
                actor.reader_closed(worker, epoch, reason);

                let notices = inbox.take();
                let replacement_ready = notices.iter().any(|notice| {
                    matches!(
                        notice,
                        ActorNotice::Environment {
                            generation: 41,
                            error: None,
                            ..
                        }
                    )
                });
                let replacement_survived = actor.browser.is_some()
                    && actor.hosts.contains_key(&19)
                    && !notices.iter().any(|notice| {
                        matches!(
                            notice,
                            ActorNotice::Event {
                                event: crate::WebEvent::ProcessFailed { .. },
                                ..
                            }
                        )
                    });
                actor.finish_browser(worker, true);
                finished
                    .send(replacement_ready && replacement_survived)
                    .expect("report replacement process state");
            },
        )
        .expect("start the worker-context test");
        assert!(wait_finished.recv().expect("the epoch test finished"));
        worker.join().expect("the epoch test worker returned");
    }

    #[test]
    fn shutdown_interrupts_a_stalled_browser_close_and_reaps_its_process_group() {
        let _lock = ACTOR_PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ACTOR_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        let (actor_sender, actor_receiver) = mpsc::channel();
        let worker_sender = actor_sender.clone();
        let (ready, wait_ready) = mpsc::channel();
        let (finished, wait_finished) = mpsc::channel();
        let fixture = std::env::temp_dir().join(format!(
            "folio-web-stalled-close-{}",
            super::random_nonce().expect("name the owned process fixture")
        ));
        std::fs::create_dir(&fixture).expect("create the owned process fixture");
        let natural_exit = fixture.join("natural-exit");
        let child_exit = natural_exit.clone();
        let child_fixture = fixture.clone();
        let helper = crate::spawn_at_priority(
            "bt-linux-web-shutdown-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| {
                let hygiene = bt_pty::test_shell::Hygiene::new();
                let mut command = hygiene.command("/bin/sh", crate::quiet_command);
                command
                    .args([
                        "-c",
                        "sleep 30 & wait; printf finished > \"$FOLIO_TEST_NATURAL_EXIT\"",
                    ])
                    .env("FOLIO_TEST_NATURAL_EXIT", child_exit)
                    .process_group(0)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                let mut child = command.spawn().expect("start a stalled browser process");
                let pid = child.id();
                let output = crate::linux_process::OutputReaders::start(worker, &mut child)
                    .expect("drain both browser output pipes");
                let [pipe_read, pipe_write] = make_pipe().expect("make a silent CDP peer");
                let mut actor = Actor::new(worker, worker_sender, actor_receiver);
                actor.cdp = Some(CdpWriter {
                    file: std::fs::File::from(pipe_write),
                    next_id: 1,
                    poisoned: false,
                    write_blocked: None,
                });
                actor.browser = Some(BrowserProcess {
                    child,
                    profile: child_fixture.join("profile"),
                    extension: child_fixture.join("extension"),
                    _reader: None,
                    output: Some(output),
                });
                let _keep_cdp_peer_open: OwnedFd = pipe_read;
                ready.send(pid).expect("publish the child process id");
                actor.close_browser(worker);
                finished.send(()).expect("report a reaped browser");
            },
        )
        .expect("start the actor shutdown worker");
        let pid = wait_ready
            .recv()
            .expect("the actor started its browser child");
        actor_sender
            .send(ActorInput::Command(ActorCommand::Shutdown))
            .expect("deliver shutdown while Browser.close has no response");
        let result = wait_finished.recv();
        if result.is_err() {
            // SAFETY: the child process group is the owned shell and its sleep
            // child created by this test; this only prevents a failed test from
            // leaving that fixture alive.
            let _ = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
        }
        assert!(result.is_ok(), "shutdown did not interrupt Browser.close");
        helper.join().expect("the actor shutdown worker returned");
        let exited_naturally = natural_exit.exists();
        std::fs::remove_dir_all(&fixture).expect("remove the owned process fixture");
        assert!(
            !exited_naturally,
            "shutdown must kill the stalled process rather than await its natural exit"
        );
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "the browser process must be reaped before shutdown returns"
        );
        ACTOR_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    }

    #[test]
    fn cancel_create_releases_only_its_unanswered_target_create() {
        let _lock = ACTOR_PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ACTOR_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        PENDING_ACTOR_CANCELLATIONS.store(0, Ordering::SeqCst);

        let (commands, received) = mpsc::channel();
        let handle = ActorHandle(commands.clone());
        let (ready, wait_ready) = mpsc::channel();
        let (finished, wait_finished) = mpsc::channel();
        let [peer, pipe] = make_pipe().expect("make a live but silent CDP pipe");
        let actor_commands = commands;
        let worker = crate::spawn_at_priority(
            "bt-linux-web-create-response-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| {
                let mut actor = Actor::new(worker, actor_commands, received);
                actor.browser_epoch = 3;
                actor.pending_page_create = Some((9, 42));
                actor.cdp = Some(CdpWriter {
                    file: std::fs::File::from(pipe),
                    next_id: 1,
                    poisoned: false,
                    write_blocked: None,
                });
                let mut live_cdp_peer = std::io::BufReader::new(std::fs::File::from(peer));
                ready.send(()).expect("the actor entered the CDP wait");

                let answer = actor.await_response(worker, 77, "Target.createTarget");
                let page_create = actor.pending_page_create;
                let unresolved_create = actor.cancelled_target_create_response;
                actor.handle_protocol_response(serde_json::json!({
                    "id": 77,
                    "result": {"targetId": "late-target"}
                }));
                let disposition_cleared = actor.cancelled_target_create_response.is_none();
                let late_response_close = read_cdp_command(&mut live_cdp_peer);
                actor.handle_protocol_event(serde_json::json!({
                    "method": "Target.attachedToTarget",
                    "sessionId": "browser-root",
                    "params": {
                        "sessionId": "late-session",
                        "targetInfo": {"targetId": "late-target", "type": "page"},
                        "waitingForDebugger": true
                    }
                }));
                let late_attachment_close = read_cdp_command(&mut live_cdp_peer);
                let cancel_deferred = actor.deferred.iter().any(|command| {
                    matches!(
                        command,
                        ActorCommand::CancelCreate {
                            host: 8,
                            generation: 42
                        }
                    )
                });
                let stale_cancel_deferred = actor.deferred.iter().any(|command| {
                    matches!(
                        command,
                        ActorCommand::CancelCreate {
                            host: 9,
                            generation: 41
                        }
                    )
                });
                let canceled_current_host = actor.cancelled_hosts.contains(&9);
                let canceled_other_host = actor.cancelled_hosts.contains(&8);
                let no_late_target_owned = !actor.targets.contains_key("late-target")
                    && !actor.target_sessions.contains_key("late-target");
                finished
                    .send((
                        answer,
                        page_create,
                        unresolved_create,
                        disposition_cleared,
                        cancel_deferred,
                        stale_cancel_deferred,
                        canceled_current_host,
                        canceled_other_host,
                        no_late_target_owned,
                        late_response_close,
                        late_attachment_close,
                    ))
                    .expect("report how the pending create ended");
            },
        )
        .expect("start the fake actor worker");

        wait_ready
            .recv()
            .expect("the fake actor reached its pending Target.createTarget response");
        handle
            .send(ActorCommand::CancelCreate {
                host: 8,
                generation: 42,
            })
            .expect("queue cancellation for another host");
        handle
            .send(ActorCommand::CancelCreate {
                host: 9,
                generation: 41,
            })
            .expect("queue cancellation for an older generation of the same host");
        handle
            .send(ActorCommand::CancelCreate {
                host: 9,
                generation: 42,
            })
            .expect("cancel the host whose Target.createTarget response is pending");
        let (
            answer,
            page_create,
            unresolved_create,
            disposition_cleared,
            cancel_deferred,
            stale_cancel_deferred,
            canceled_current_host,
            canceled_other_host,
            no_late_target_owned,
            late_response_close,
            late_attachment_close,
        ) = wait_finished
            .recv()
            .expect("matching cancellation releases setup");
        worker.join().expect("the fake actor worker returned");
        assert!(matches!(answer, Err(error) if error.contains("cancel")));
        assert_eq!(page_create, None);
        assert_eq!(unresolved_create, Some((77, 9, 42)));
        assert!(disposition_cleared);
        assert!(cancel_deferred, "cancellation for host 8 stays queued");
        assert!(stale_cancel_deferred, "older generation stays queued");
        assert!(canceled_current_host);
        assert!(!canceled_other_host);
        assert!(no_late_target_owned);
        for command in [late_response_close, late_attachment_close] {
            assert_eq!(command["method"], "Target.closeTarget");
            assert_eq!(command["params"]["targetId"], "late-target");
        }
        assert_eq!(PENDING_ACTOR_CANCELLATIONS.load(Ordering::SeqCst), 0);
        ACTOR_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    }

    #[test]
    fn close_releases_its_unanswered_target_create_and_closes_a_late_target() {
        let _lock = ACTOR_PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        PENDING_ACTOR_CANCELLATIONS.store(0, Ordering::SeqCst);

        let (commands, received) = mpsc::channel();
        let handle = ActorHandle(commands.clone());
        let (ready, wait_ready) = mpsc::channel();
        let (finished, wait_finished) = mpsc::channel();
        let [peer, pipe] = make_pipe().expect("make a live but silent CDP pipe");
        let actor_commands = commands;
        let worker = crate::spawn_at_priority(
            "bt-linux-web-close-create-response-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| {
                let mut actor = Actor::new(worker, actor_commands, received);
                actor.browser_epoch = 4;
                actor.pending_page_create = Some((12, 7));
                actor.cdp = Some(CdpWriter {
                    file: std::fs::File::from(pipe),
                    next_id: 1,
                    poisoned: false,
                    write_blocked: None,
                });
                let mut live_cdp_peer = std::io::BufReader::new(std::fs::File::from(peer));
                ready.send(()).expect("the actor entered the CDP wait");

                let answer = actor.await_response(worker, 93, "Target.createTarget");
                let pending_create = actor.pending_page_create;
                let unresolved_create = actor.cancelled_target_create_response;
                let close_deferred = actor
                    .deferred
                    .iter()
                    .any(|command| matches!(command, ActorCommand::Close { host: 12 }));
                actor.handle_protocol_response(serde_json::json!({
                    "id": 93,
                    "result": {"targetId": "late-close-target"}
                }));
                let disposition_cleared = actor.cancelled_target_create_response.is_none();
                let close_target = read_cdp_command(&mut live_cdp_peer);
                let canceled_host = actor.cancelled_hosts.contains(&12);
                finished
                    .send((
                        answer,
                        pending_create,
                        unresolved_create,
                        disposition_cleared,
                        close_deferred,
                        close_target,
                        canceled_host,
                    ))
                    .expect("report host close and late target cleanup");
            },
        )
        .expect("start the fake actor worker");

        wait_ready
            .recv()
            .expect("the fake actor reached its pending Target.createTarget response");
        handle
            .send(ActorCommand::Close { host: 12 })
            .expect("close the host while its target id is still unknown");
        let (
            answer,
            pending_create,
            unresolved_create,
            disposition_cleared,
            close_deferred,
            close_target,
            canceled_host,
        ) = wait_finished.recv().expect("host close releases setup");
        worker.join().expect("the fake actor worker returned");
        assert!(matches!(answer, Err(error) if error.contains("closed")));
        assert_eq!(pending_create, None);
        assert_eq!(unresolved_create, Some((93, 12, 7)));
        assert!(disposition_cleared);
        assert!(
            close_deferred,
            "the normal host retirement still runs after unwind"
        );
        assert!(canceled_host);
        assert_eq!(close_target["method"], "Target.closeTarget");
        assert_eq!(close_target["params"]["targetId"], "late-close-target");
        assert_eq!(PENDING_ACTOR_CANCELLATIONS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn unanswered_canceled_setup_and_notify_responses_are_not_retained() {
        let _lock = ACTOR_PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ACTOR_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        PENDING_ACTOR_CANCELLATIONS.store(0, Ordering::SeqCst);

        let (commands, received) = mpsc::channel();
        let handle = ActorHandle(commands.clone());
        let (ready, wait_ready) = mpsc::channel();
        let (finished, wait_finished) = mpsc::channel();
        let [peer, pipe] = make_pipe().expect("make a CDP pipe for repeated setup cancellation");
        let actor_commands = commands;
        let worker = crate::spawn_at_priority(
            "bt-linux-web-response-owner-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| {
                let mut actor = Actor::new(worker, actor_commands, received);
                actor.cdp = Some(CdpWriter {
                    file: std::fs::File::from(pipe),
                    next_id: 1,
                    poisoned: false,
                    write_blocked: None,
                });
                let mut live_cdp_peer = std::io::BufReader::new(std::fs::File::from(peer));

                for generation in 1..=32 {
                    let target = format!("setup-target-{generation}");
                    actor.pending_controller_setup = Some((55, generation, target.clone()));
                    let setup_id = actor
                        .send_command(None, "Page.enable", serde_json::json!({}))
                        .expect("send one awaited controller setup command");
                    ready
                        .send((generation, setup_id))
                        .expect("the actor is waiting on this setup response");
                    let answer = actor.await_response(worker, setup_id, "Page.enable");
                    assert!(matches!(answer, Err(error) if error.contains("canceled")));
                    assert_eq!(
                        read_cdp_command(&mut live_cdp_peer)["method"],
                        "Page.enable"
                    );
                    let close = read_cdp_command(&mut live_cdp_peer);
                    assert_eq!(close["method"], "Target.closeTarget");
                    assert_eq!(close["params"]["targetId"], target);

                    actor
                        .notify(None, "Browser.getVersion", serde_json::json!({}))
                        .expect("send an unawaited protocol command");
                    assert_eq!(
                        read_cdp_command(&mut live_cdp_peer)["method"],
                        "Browser.getVersion"
                    );
                    assert!(actor.awaited_responses.is_empty());
                    assert!(actor.protocol_responses.is_empty());
                    assert!(actor.pending_worker_responses.is_empty());
                }
                finished
                    .send(())
                    .expect("all response ownership stayed bounded");
            },
        )
        .expect("start the response ownership actor worker");

        for generation in 1..=32 {
            assert_eq!(
                wait_ready
                    .recv()
                    .expect("the next setup reached its wait")
                    .0,
                generation
            );
            handle
                .send(ActorCommand::CancelCreate {
                    host: 55,
                    generation,
                })
                .expect("cancel this generation's pending setup");
        }
        wait_finished
            .recv()
            .expect("all canceled setup replies and notifications were dropped");
        worker
            .join()
            .expect("the response ownership worker returned");
        assert_eq!(PENDING_ACTOR_CANCELLATIONS.load(Ordering::SeqCst), 0);
        ACTOR_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    }

    #[test]
    fn active_outer_response_survives_a_nested_protocol_call() {
        let (commands, received) = mpsc::channel();
        let (finished, wait_finished) = mpsc::channel();
        let worker = crate::spawn_at_priority(
            "bt-linux-web-nested-response-owner-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| {
                let mut actor = Actor::new(worker, commands, received);
                actor.browser_epoch = 6;
                actor
                    .sessions
                    .insert(String::from("root-session"), (7, true));
                let [peer, pipe] = make_pipe().expect("make a CDP pipe for nested calls");
                actor.cdp = Some(CdpWriter {
                    file: std::fs::File::from(pipe),
                    next_id: 1,
                    poisoned: false,
                    write_blocked: None,
                });
                let mut live_cdp_peer = std::io::BufReader::new(std::fs::File::from(peer));
                let outer_id = actor
                    .send_command(None, "Browser.getVersion", serde_json::json!({}))
                    .expect("send the outer command");
                actor
                    .worker
                    .send(ActorInput::Protocol {
                        epoch: 6,
                        message: serde_json::json!({
                            "method": "Page.loadEventFired",
                            "sessionId": "root-session",
                            "params": {}
                        }),
                    })
                    .expect("queue the event that starts a nested evaluate");
                actor
                    .worker
                    .send(ActorInput::Protocol {
                        epoch: 6,
                        message: serde_json::json!({
                            "id": outer_id,
                            "result": {"status": "outer"}
                        }),
                    })
                    .expect("queue the outer response while the nested call is active");
                actor
                    .worker
                    .send(ActorInput::Protocol {
                        epoch: 6,
                        message: serde_json::json!({
                            "id": 2,
                            "result": {"result": {"value": "nested title"}}
                        }),
                    })
                    .expect("queue the nested evaluate response");

                let outer_result = actor
                    .await_response(worker, outer_id, "Browser.getVersion")
                    .expect("the outer response remains owned across the nested call");
                let outer_command = read_cdp_command(&mut live_cdp_peer);
                let nested_command = read_cdp_command(&mut live_cdp_peer);
                finished
                    .send((
                        outer_result,
                        outer_command,
                        nested_command,
                        actor.awaited_responses.len(),
                        actor.protocol_responses.len(),
                    ))
                    .expect("report active and retained response ownership");
            },
        )
        .expect("start the nested response worker");
        let (outer_result, outer_command, nested_command, awaited, retained) = wait_finished
            .recv()
            .expect("nested call and outer response completed");
        worker.join().expect("the nested response worker returned");
        assert_eq!(outer_result["status"], "outer");
        assert_eq!(outer_command["method"], "Browser.getVersion");
        assert_eq!(nested_command["method"], "Runtime.evaluate");
        assert_eq!(awaited, 0);
        assert_eq!(retained, 0);
    }

    fn nested_other_host_setup_survives_outer_host_retirement(close_host: bool) {
        let _lock = ACTOR_PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ACTOR_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        PENDING_ACTOR_CANCELLATIONS.store(0, Ordering::SeqCst);

        let (inputs, received) = mpsc::channel();
        let handle = ActorHandle(inputs.clone());
        let protocol_inputs = inputs.clone();
        let (ready, wait_ready) = mpsc::channel();
        let (finished, wait_finished) = mpsc::channel();
        let [peer, pipe] = make_pipe().expect("make a CDP pipe for nested host cancellation");
        let mut live_cdp_peer = std::io::BufReader::new(std::fs::File::from(peer));
        let actor_inputs = inputs.clone();
        let worker = crate::spawn_at_priority(
            "bt-linux-web-nested-host-cancel-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| {
                let mut actor = Actor::new(worker, actor_inputs, received);
                actor.browser_epoch = 9;
                let host_a = 101;
                let host_b = 202;
                let inbox_a = std::sync::Arc::new(HostInbox::default());
                let inbox_b = std::sync::Arc::new(HostInbox::default());
                let make_page =
                    |host, generation, target, session, inbox, ime_binding, ime_token| {
                        super::HostPage {
                            inbox,
                            wake: std::sync::Arc::new(|| {}),
                            folder: std::env::temp_dir(),
                            generation,
                            page: Some(crate::PageVisual { tab: host, seat: 1 }),
                            rules: String::new(),
                            color_scheme: None,
                            bounds: (0, 0, 640, 480),
                            scale: 1.0,
                            visible: true,
                            sequence: 0,
                            screencast_started: false,
                            screencast_frame_reported: false,
                            screencast_refresh_pending: false,
                            destination_bounds_ready: true,
                            target: Some(String::from(target)),
                            session: Some(String::from(session)),
                            tab_id: None,
                            main_frame: Some(String::from("root-frame")),
                            policy_token: None,
                            navigation_binding: None,
                            navigation_permit: None,
                            latest_navigation_request: None,
                            ime_binding,
                            ime_token,
                            last_ime_cursor: None,
                            main_context: None,
                            browser_process_id: 0,
                            installed: true,
                            main_loader: None,
                            main_status: 0,
                            find_term: String::new(),
                            find_case_sensitive: false,
                            find_count: 0,
                            find_active: 0,
                            last_url: String::new(),
                        }
                    };
                actor.hosts.insert(
                    host_a,
                    make_page(
                        host_a,
                        40,
                        "a-target",
                        "a-root-session",
                        inbox_a,
                        None,
                        None,
                    ),
                );
                actor.hosts.insert(
                    host_b,
                    make_page(
                        host_b,
                        90,
                        "b-root-target",
                        "b-root-session",
                        inbox_b.clone(),
                        Some(String::from("b-ime-binding")),
                        Some(String::from("b-ime-token")),
                    ),
                );
                actor
                    .sessions
                    .insert(String::from("a-root-session"), (host_a, true));
                actor
                    .sessions
                    .insert(String::from("b-root-session"), (host_b, true));
                actor.targets.insert(String::from("a-target"), host_a);
                actor.targets.insert(String::from("b-root-target"), host_b);
                actor
                    .target_sessions
                    .insert(String::from("a-target"), String::from("a-root-session"));
                actor.target_sessions.insert(
                    String::from("b-root-target"),
                    String::from("b-root-session"),
                );
                actor
                    .session_targets
                    .insert(String::from("a-root-session"), String::from("a-target"));
                actor.session_targets.insert(
                    String::from("b-root-session"),
                    String::from("b-root-target"),
                );
                actor.pending_controller_setup = Some((host_a, 40, String::from("a-target")));
                actor.cdp = Some(CdpWriter {
                    file: std::fs::File::from(pipe),
                    next_id: 1,
                    poisoned: false,
                    write_blocked: None,
                });
                let outer_id = actor
                    .send_command(Some("a-root-session"), "Page.enable", serde_json::json!({}))
                    .expect("start host A's controller setup call");
                ready
                    .send(outer_id)
                    .expect("host A is waiting while a protocol event is dispatched");
                let outer_result = actor.await_response(worker, outer_id, "Page.enable");
                actor.handle_protocol_response(serde_json::json!({
                    "id": outer_id,
                    "result": {"late": true}
                }));
                let b_setup_failed = inbox_b.take().iter().any(|notice| {
                    matches!(
                        notice,
                        ActorNotice::Event {
                            event: crate::WebEvent::ProcessFailed { .. },
                            ..
                        }
                    )
                });
                let host_a_retired = actor.cancelled_hosts.contains(&host_a)
                    && !actor.sessions.contains_key("a-root-session")
                    && !actor.targets.contains_key("a-target")
                    && actor
                        .hosts
                        .get(&host_a)
                        .is_some_and(|page| page.target.is_none() && page.session.is_none());
                let host_b_setup_alive = actor.sessions.get("b-iframe-session")
                    == Some(&(host_b, false))
                    && actor.targets.get("b-iframe-target") == Some(&host_b)
                    && actor
                        .target_sessions
                        .get("b-iframe-target")
                        .map(String::as_str)
                        == Some("b-iframe-session")
                    && actor.frame_hosts.get("b-iframe-root") == Some(&host_b);
                let outer_reply_dropped = !actor.protocol_responses.contains_key(&outer_id);
                let close_deferred = actor
                    .deferred
                    .iter()
                    .any(|command| matches!(command, ActorCommand::Close { host: 101 }));
                finished
                    .send((
                        outer_result,
                        b_setup_failed,
                        host_a_retired,
                        host_b_setup_alive,
                        outer_reply_dropped,
                        close_deferred,
                    ))
                    .expect("report request-scoped nested cancellation");
            },
        )
        .expect("start the nested cancellation actor worker");

        let outer_id = wait_ready
            .recv()
            .expect("host A's controller command was sent");
        let outer_command = read_cdp_command(&mut live_cdp_peer);
        assert_eq!(outer_command["id"], outer_id);
        assert_eq!(outer_command["method"], "Page.enable");
        protocol_inputs
            .send(ActorInput::Protocol {
                epoch: 9,
                message: serde_json::json!({
                    "method": "Target.attachedToTarget",
                    "sessionId": "b-root-session",
                    "params": {
                        "sessionId": "b-iframe-session",
                        "targetInfo": {
                            "targetId": "b-iframe-target",
                            "type": "iframe",
                            "url": "https://other.test/frame"
                        },
                        "waitingForDebugger": false
                    }
                }),
            })
            .expect("attach an iframe for healthy host B during host A's wait");
        let nested_command = read_cdp_command(&mut live_cdp_peer);
        assert_eq!(nested_command["id"], 2);
        assert_eq!(nested_command["sessionId"], "b-iframe-session");
        assert_eq!(nested_command["method"], "Runtime.enable");
        if close_host {
            handle
                .send(ActorCommand::Close { host: 101 })
                .expect("close host A while host B's nested setup is waiting");
        } else {
            handle
                .send(ActorCommand::CancelCreate {
                    host: 101,
                    generation: 40,
                })
                .expect("cancel only host A while host B's nested setup is waiting");
        }
        let close_a = read_cdp_command(&mut live_cdp_peer);
        assert_eq!(close_a["method"], "Target.closeTarget");
        assert_eq!(close_a["params"]["targetId"], "a-target");
        for id in [2, 4, 5, 6, 7, 8, 9, 10, 11, 12] {
            let result = if id == 12 {
                serde_json::json!({
                    "frameTree": {
                        "frame": {
                            "id": "b-iframe-root",
                            "url": "https://other.test/frame"
                        }
                    }
                })
            } else {
                serde_json::json!({})
            };
            protocol_inputs
                .send(ActorInput::Protocol {
                    epoch: 9,
                    message: serde_json::json!({"id": id, "result": result}),
                })
                .expect("answer the nested call owned by host B");
        }
        let (
            outer_result,
            b_setup_failed,
            host_a_retired,
            host_b_setup_alive,
            outer_reply_dropped,
            close_deferred,
        ) = wait_finished
            .recv()
            .expect("host B completes before canceled host A unwinds");
        worker.join().expect("nested cancellation worker returned");
        assert!(matches!(outer_result, Err(error) if if close_host {
            error.contains("closed")
        } else {
            error.contains("canceled")
        }));
        assert!(!b_setup_failed, "host B's nested setup must not fail");
        assert!(host_a_retired, "host A's controller state is retired");
        assert!(
            host_b_setup_alive,
            "host B's child session remains installed"
        );
        assert!(
            outer_reply_dropped,
            "host A's later response is unsolicited"
        );
        assert_eq!(close_deferred, close_host);
        assert_eq!(PENDING_ACTOR_CANCELLATIONS.load(Ordering::SeqCst), 0);
        ACTOR_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    }

    #[test]
    fn cancel_create_for_outer_host_does_not_unwind_nested_other_host() {
        nested_other_host_setup_survives_outer_host_retirement(false);
    }

    #[test]
    fn close_for_outer_host_does_not_unwind_nested_other_host() {
        nested_other_host_setup_survives_outer_host_retirement(true);
    }

    #[test]
    fn close_interrupts_an_unanswered_environment_setup_and_reaps_its_browser() {
        let _lock = ACTOR_PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ACTOR_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        PENDING_ACTOR_CANCELLATIONS.store(0, Ordering::SeqCst);
        let (commands, received) = mpsc::channel();
        let handle = ActorHandle(commands.clone());
        let actor_commands = commands;
        let (ready, wait_ready) = mpsc::channel();
        let (finished, wait_finished) = mpsc::channel();
        let fixture = std::env::temp_dir().join(format!(
            "folio-web-canceled-boot-{}",
            super::random_nonce().expect("name the owned process fixture")
        ));
        std::fs::create_dir(&fixture).expect("create the owned process fixture");
        let natural_exit = fixture.join("natural-exit");
        let child_exit = natural_exit.clone();
        let child_fixture = fixture.clone();
        let worker = crate::spawn_at_priority(
            "bt-linux-web-close-boot-response-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| {
                let [pipe_read, pipe_write] = make_pipe().expect("make a silent CDP peer");
                let [child_stdin, hold_child_stdin] =
                    make_pipe().expect("make a controlled browser stdin");
                let hygiene = bt_pty::test_shell::Hygiene::new();
                let mut command = hygiene.command("/bin/sh", crate::quiet_command);
                command
                    .args([
                        "-c",
                        "read _; printf finished > \"$FOLIO_TEST_NATURAL_EXIT\"",
                    ])
                    .env("FOLIO_TEST_NATURAL_EXIT", child_exit)
                    .process_group(0)
                    .stdin(Stdio::from(child_stdin))
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                let mut child = command.spawn().expect("start the owned browser stand-in");
                let pid = child.id();
                let output = crate::linux_process::OutputReaders::start(worker, &mut child)
                    .expect("drain browser output");
                let mut actor = Actor::new(worker, actor_commands, received);
                actor.active_boot = Some((12, 7));
                actor.cdp = Some(CdpWriter {
                    file: std::fs::File::from(pipe_write),
                    next_id: 1,
                    poisoned: false,
                    write_blocked: None,
                });
                actor.browser = Some(BrowserProcess {
                    child,
                    profile: child_fixture.join("profile"),
                    extension: child_fixture.join("extension"),
                    _reader: None,
                    output: Some(output),
                });
                let _live_cdp_peer: OwnedFd = pipe_read;
                let _hold_child_stdin = hold_child_stdin;
                ready
                    .send(pid)
                    .expect("the actor entered environment setup");
                let answer = actor.await_response(worker, 111, "Browser.getVersion");
                let browser_reaped = actor.browser.is_none()
                    && !std::path::Path::new(&format!("/proc/{pid}")).exists();
                finished
                    .send((answer, browser_reaped, natural_exit.exists()))
                    .expect("report the canceled environment startup");
            },
        )
        .expect("start the fake actor worker");

        let _pid = wait_ready
            .recv()
            .expect("the fake browser entered its setup response wait");
        handle
            .send(ActorCommand::Close { host: 12 })
            .expect("retire this host while its environment setup is pending");
        let (answer, browser_reaped, exited_naturally) = wait_finished
            .recv()
            .expect("host close releases environment setup");
        worker.join().expect("the fake actor worker returned");
        std::fs::remove_dir_all(&fixture).expect("remove the owned process fixture");
        assert!(matches!(answer, Err(error) if error.contains("starting")));
        assert!(browser_reaped);
        assert!(
            !exited_naturally,
            "the child was killed, not awaited naturally"
        );
        assert_eq!(PENDING_ACTOR_CANCELLATIONS.load(Ordering::SeqCst), 0);
        ACTOR_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    }

    #[test]
    fn cancel_create_closes_a_target_whose_setup_response_is_pending() {
        let _lock = ACTOR_PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        PENDING_ACTOR_CANCELLATIONS.store(0, Ordering::SeqCst);

        let (commands, received) = mpsc::channel();
        let handle = ActorHandle(commands.clone());
        let (ready, wait_ready) = mpsc::channel();
        let (finished, wait_finished) = mpsc::channel();
        let [peer, pipe] = make_pipe().expect("make a live CDP setup pipe");
        let actor_commands = commands;
        let worker = crate::spawn_at_priority(
            "bt-linux-web-configure-response-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| {
                let mut actor = Actor::new(worker, actor_commands, received);
                actor.browser_epoch = 5;
                actor.pending_controller_setup = Some((15, 23, String::from("page-target")));
                actor.cdp = Some(CdpWriter {
                    file: std::fs::File::from(pipe),
                    next_id: 1,
                    poisoned: false,
                    write_blocked: None,
                });
                let mut live_cdp_peer = std::io::BufReader::new(std::fs::File::from(peer));
                ready.send(()).expect("the actor entered the CDP wait");

                let answer = actor.await_response(worker, 104, "Page.enable");
                let setup_cleared = actor.pending_controller_setup.is_none();
                let close_target = read_cdp_command(&mut live_cdp_peer);
                actor.handle_protocol_response(serde_json::json!({
                    "id": 104,
                    "result": {}
                }));
                let late_response_dropped = !actor.protocol_responses.contains_key(&104)
                    && !actor.awaited_responses.contains_key(&104);
                finished
                    .send((answer, setup_cleared, close_target, late_response_dropped))
                    .expect("report setup retirement and late response disposal");
            },
        )
        .expect("start the fake actor worker");

        wait_ready
            .recv()
            .expect("the fake actor reached its pending Page.enable response");
        handle
            .send(ActorCommand::CancelCreate {
                host: 15,
                generation: 23,
            })
            .expect("cancel only this host generation's setup");
        let (answer, setup_cleared, close_target, late_response_dropped) = wait_finished
            .recv()
            .expect("controller cancellation releases setup");
        worker.join().expect("the fake actor worker returned");
        assert!(matches!(answer, Err(error) if error.contains("canceled")));
        assert!(setup_cleared);
        assert!(late_response_dropped);
        assert_eq!(close_target["method"], "Target.closeTarget");
        assert_eq!(close_target["params"]["targetId"], "page-target");
        assert_eq!(PENDING_ACTOR_CANCELLATIONS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_late_frame_from_a_canceled_generation_cannot_be_labeled_as_the_retry() {
        let (finished, wait_finished) = mpsc::channel();
        let worker = crate::spawn_at_priority(
            "bt-linux-web-stale-frame-after-cancel-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| {
                let (sender, receiver) = mpsc::channel();
                let mut actor = Actor::new(worker, sender, receiver);
                let host = 31;
                let other_host = 32;
                let page_visual = crate::PageVisual { tab: 5, seat: 7 };
                let inbox = std::sync::Arc::new(HostInbox::default());
                let other_inbox = std::sync::Arc::new(HostInbox::default());
                let make_page = |generation, page, target, session, inbox| super::HostPage {
                    inbox,
                    wake: std::sync::Arc::new(|| {}),
                    folder: std::env::temp_dir(),
                    generation,
                    page: Some(page),
                    rules: String::new(),
                    color_scheme: None,
                    bounds: (0, 0, 2, 2),
                    scale: 1.0,
                    visible: true,
                    sequence: 0,
                    screencast_started: true,
                    screencast_frame_reported: false,
                    screencast_refresh_pending: false,
                    destination_bounds_ready: true,
                    target: Some(String::from(target)),
                    session: Some(String::from(session)),
                    tab_id: None,
                    main_frame: Some(String::from("old-frame")),
                    policy_token: None,
                    navigation_binding: None,
                    navigation_permit: None,
                    latest_navigation_request: None,
                    ime_binding: None,
                    ime_token: None,
                    last_ime_cursor: None,
                    main_context: Some(7),
                    browser_process_id: 0,
                    installed: true,
                    main_loader: None,
                    main_status: 0,
                    find_term: String::new(),
                    find_case_sensitive: false,
                    find_count: 0,
                    find_active: 0,
                    last_url: String::new(),
                };
                actor.hosts.insert(
                    host,
                    make_page(23, page_visual, "old-target", "old-session", inbox.clone()),
                );
                actor.hosts.insert(
                    other_host,
                    make_page(
                        4,
                        crate::PageVisual { tab: 9, seat: 2 },
                        "other-target",
                        "other-session",
                        other_inbox,
                    ),
                );
                actor
                    .sessions
                    .insert(String::from("old-session"), (host, true));
                actor
                    .sessions
                    .insert(String::from("other-session"), (other_host, true));
                actor.targets.insert(String::from("old-target"), host);
                actor
                    .targets
                    .insert(String::from("other-target"), other_host);
                actor
                    .target_sessions
                    .insert(String::from("old-target"), String::from("old-session"));
                actor
                    .target_sessions
                    .insert(String::from("other-target"), String::from("other-session"));
                actor
                    .session_targets
                    .insert(String::from("old-session"), String::from("old-target"));
                actor
                    .session_targets
                    .insert(String::from("other-session"), String::from("other-target"));
                actor
                    .sessions
                    .insert(String::from("shared-worker-session"), (host, false));
                actor
                    .targets
                    .insert(String::from("shared-worker-target"), host);
                actor.target_sessions.insert(
                    String::from("shared-worker-target"),
                    String::from("shared-worker-session"),
                );
                actor.session_targets.insert(
                    String::from("shared-worker-session"),
                    String::from("shared-worker-target"),
                );
                actor
                    .service_worker_targets
                    .insert(String::from("shared-worker-target"));
                actor.service_worker_clients.insert(
                    String::from("shared-worker-target"),
                    Some(HashSet::from([
                        String::from("old-target"),
                        String::from("other-target"),
                    ])),
                );
                actor.contexts.insert(
                    (String::from("old-session"), 7),
                    (host, String::from("old-frame")),
                );
                actor.contexts.insert(
                    (String::from("other-session"), 8),
                    (other_host, String::from("other-frame")),
                );
                actor.frame_hosts.insert(String::from("old-frame"), host);
                actor
                    .frame_parents
                    .insert(String::from("old-frame"), String::from("old-parent"));
                actor
                    .frame_sessions
                    .insert(String::from("old-frame"), String::from("old-session"));
                actor
                    .frame_urls
                    .insert(String::from("old-frame"), String::from("https://old.test/"));
                actor
                    .frame_hosts
                    .insert(String::from("other-frame"), other_host);
                actor
                    .frame_parents
                    .insert(String::from("other-frame"), String::from("other-parent"));
                actor
                    .frame_sessions
                    .insert(String::from("other-frame"), String::from("other-session"));
                actor.frame_urls.insert(
                    String::from("other-frame"),
                    String::from("https://other.test/"),
                );
                actor.close_controller_target(host, 23, "old-target");

                if let Some(page) = actor.hosts.get_mut(&host) {
                    page.generation = 24;
                    page.target = Some(String::from("retry-target"));
                    page.session = Some(String::from("retry-session"));
                    page.main_frame = Some(String::from("retry-frame"));
                    page.main_context = None;
                    page.sequence = 0;
                    page.screencast_frame_reported = false;
                    page.installed = true;
                }
                actor.targets.insert(String::from("retry-target"), host);
                actor
                    .sessions
                    .insert(String::from("retry-session"), (host, true));
                actor
                    .target_sessions
                    .insert(String::from("retry-target"), String::from("retry-session"));
                actor
                    .session_targets
                    .insert(String::from("retry-session"), String::from("retry-target"));
                actor.frame_hosts.insert(String::from("retry-frame"), host);
                actor
                    .frame_parents
                    .insert(String::from("retry-frame"), String::from("retry-parent"));
                actor
                    .frame_sessions
                    .insert(String::from("retry-frame"), String::from("retry-session"));
                let mut png = std::io::Cursor::new(Vec::new());
                image::DynamicImage::new_rgba8(2, 2)
                    .write_to(&mut png, image::ImageFormat::Png)
                    .expect("encode the late old-generation frame");
                let encoded = base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    png.get_ref(),
                );
                actor.screencast_frame(serde_json::json!({
                    "sessionId": "old-session",
                    "params": {"sessionId": 11, "data": encoded}
                }));
                actor.frame_detached(serde_json::json!({
                    "sessionId": "old-session",
                    "params": {"frameId": "retry-frame", "reason": "remove"}
                }));
                let retry_frames = inbox
                    .take()
                    .into_iter()
                    .filter_map(|notice| match notice {
                        ActorNotice::Frame(frame) => {
                            Some((frame.generation, frame.sequence, frame.page))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let old_state_retired = !actor.sessions.contains_key("old-session")
                    && !actor.target_sessions.contains_key("old-target")
                    && !actor.session_targets.contains_key("old-session")
                    && !actor
                        .contexts
                        .contains_key(&(String::from("old-session"), 7))
                    && !actor.frame_hosts.contains_key("old-frame")
                    && !actor.frame_parents.contains_key("old-frame")
                    && !actor.frame_sessions.contains_key("old-frame")
                    && !actor.frame_urls.contains_key("old-frame");
                let other_state_preserved = actor.sessions.contains_key("other-session")
                    && actor.targets.get("other-target") == Some(&other_host)
                    && actor.frame_hosts.get("other-frame") == Some(&other_host)
                    && actor.frame_parents.get("other-frame").map(String::as_str)
                        == Some("other-parent");
                let shared_worker_preserved = actor.sessions.get("shared-worker-session")
                    == Some(&(0, false))
                    && actor.targets.get("shared-worker-target") == Some(&0)
                    && actor
                        .service_worker_targets
                        .contains("shared-worker-target")
                    && actor
                        .service_worker_clients
                        .get("shared-worker-target")
                        .and_then(Option::as_ref)
                        .is_some_and(|clients| {
                            clients == &HashSet::from([String::from("other-target")])
                        })
                    && actor.service_worker_hosts("shared-worker-target")
                        == Some(HashMap::from([(other_host, 4)]));
                let retry_frame_preserved = actor.frame_hosts.get("retry-frame") == Some(&host)
                    && actor.frame_sessions.get("retry-frame").map(String::as_str)
                        == Some("retry-session")
                    && actor.frame_parents.get("retry-frame").map(String::as_str)
                        == Some("retry-parent");
                finished
                    .send((
                        retry_frames,
                        old_state_retired,
                        other_state_preserved,
                        shared_worker_preserved,
                        retry_frame_preserved,
                    ))
                    .expect("report late-frame rejection and host isolation");
            },
        )
        .expect("start the stale-frame actor worker");
        let (
            retry_frames,
            old_state_retired,
            other_state_preserved,
            shared_worker_preserved,
            retry_frame_preserved,
        ) = wait_finished
            .recv()
            .expect("the stale-frame probe completed");
        worker.join().expect("the stale-frame worker returned");
        assert!(
            retry_frames.is_empty(),
            "old screencast frame is not published"
        );
        assert!(
            old_state_retired,
            "old session and frame state is retired synchronously"
        );
        assert!(
            other_state_preserved,
            "retirement leaves other host state untouched"
        );
        assert!(
            shared_worker_preserved,
            "a worker still used by another host survives"
        );
        assert!(
            retry_frame_preserved,
            "an old detach event cannot erase a retry's frame mapping"
        );
    }

    #[test]
    fn a_new_boot_generation_retries_after_a_canceled_create_without_dropping_other_hosts() {
        let _lock = ACTOR_PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (commands, received) = mpsc::channel();
        let (finished, wait_finished) = mpsc::channel();
        let worker = crate::spawn_at_priority(
            "bt-linux-web-create-retry-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| {
                let mut actor = Actor::new(worker, commands, received);
                actor.browser_epoch = 5;
                let [peer, pipe] = make_pipe().expect("make a live CDP peer for the retry");
                actor.cdp = Some(CdpWriter {
                    file: std::fs::File::from(pipe),
                    next_id: 1,
                    poisoned: false,
                    write_blocked: None,
                });
                let mut live_cdp_peer = std::io::BufReader::new(std::fs::File::from(peer));
                let folder = std::path::PathBuf::from("/tmp/folio-test-profile");
                let child = bt_pty::test_shell::Hygiene::new()
                    .command("/bin/true", crate::quiet_command)
                    .spawn()
                    .expect("start a disposable ready-process stand-in");
                actor.browser = Some(BrowserProcess {
                    child,
                    profile: folder.clone(),
                    extension: std::path::PathBuf::new(),
                    _reader: None,
                    output: None,
                });

                let retry_inbox = std::sync::Arc::new(HostInbox::default());
                let other_inbox = std::sync::Arc::new(HostInbox::default());
                actor.boot_host(
                    worker,
                    BootRequest {
                        host: 8,
                        folder: folder.clone(),
                        generation: 8,
                        rules: String::new(),
                        color_scheme: None,
                        inbox: other_inbox.clone(),
                        wake: std::sync::Arc::new(|| {}),
                    },
                );
                if let Some(page) = actor.hosts.get_mut(&8) {
                    page.page = Some(crate::PageVisual { tab: 2, seat: 3 });
                    page.session = Some(String::from("other-session"));
                    page.main_frame = Some(String::from("other-frame"));
                    page.visible = true;
                    page.installed = true;
                }
                actor
                    .sessions
                    .insert(String::from("other-session"), (8, true));

                actor.pending_page_create = Some((9, 42));
                actor.abandon_target_create_response(77, 9, 42);
                let boot_retry = |generation| BootRequest {
                    host: 9,
                    folder: folder.clone(),
                    generation,
                    rules: String::new(),
                    color_scheme: None,
                    inbox: retry_inbox.clone(),
                    wake: std::sync::Arc::new(|| {}),
                };
                let create_retry = |generation| super::CreatePageRequest {
                    host: 9,
                    generation,
                    bounds: (0, 0, 320, 240),
                    scale: 1.0,
                    visible: true,
                    color_scheme: None,
                    rules: String::new(),
                };
                for generation in 43..=45 {
                    actor.boot_host(worker, boot_retry(generation));
                    actor.create_page(worker, create_retry(generation));
                    assert_eq!(
                        actor.cancelled_target_create_response,
                        Some((77, 9, 42)),
                        "retries retain one authoritative unresolved create"
                    );
                    assert_eq!(
                        actor.cdp.as_ref().map(|cdp| cdp.next_id),
                        Some(1),
                        "refused retries send no CDP create and allocate no IDs"
                    );
                    assert!(actor.deferred.is_empty());
                    assert!(actor.pending_page_create.is_none());
                    assert!(actor.pending_create.is_empty());
                }

                actor
                    .protocol_responses
                    .insert(1, serde_json::json!({"id": 1, "result": {}}));
                actor.zoom(8, 1.25);
                let healthy_host_command = read_cdp_command(&mut live_cdp_peer);
                actor
                    .protocol_responses
                    .insert(2, serde_json::json!({"id": 2, "result": {}}));
                actor.navigate(8, "https://other.test/");
                let healthy_navigation_command = read_cdp_command(&mut live_cdp_peer);
                let mut png = std::io::Cursor::new(Vec::new());
                image::DynamicImage::new_rgba8(2, 2)
                    .write_to(&mut png, image::ImageFormat::Png)
                    .expect("encode the healthy host's frame");
                let encoded = base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    png.get_ref(),
                );
                actor.screencast_frame(serde_json::json!({
                    "sessionId": "other-session",
                    "params": {"sessionId": 17, "data": encoded}
                }));
                let healthy_frame_ack = read_cdp_command(&mut live_cdp_peer);
                let healthy_frame =
                    other_inbox
                        .take()
                        .into_iter()
                        .find_map(|notice| match notice {
                            ActorNotice::Frame(frame) => {
                                Some((frame.generation, frame.sequence, frame.page))
                            }
                            _ => None,
                        });
                let other_generation = actor.hosts.get(&8).map(|page| page.generation);
                let retry_generation = actor.hosts.get(&9).map(|page| page.generation);
                let retry_admitted = !actor.cancelled_hosts.contains(&9);
                actor.handle_protocol_response(serde_json::json!({
                    "id": 77,
                    "result": {"targetId": "old-generation-target"}
                }));
                let late_response_close = read_cdp_command(&mut live_cdp_peer);
                let disposition_cleared = actor.cancelled_target_create_response.is_none();
                actor.handle_protocol_event(serde_json::json!({
                    "method": "Target.attachedToTarget",
                    "sessionId": "browser-root",
                    "params": {
                        "sessionId": "old-generation-session",
                        "targetInfo": {
                            "targetId": "old-generation-target",
                            "type": "page"
                        },
                        "waitingForDebugger": true
                    }
                }));
                let late_attachment_close = read_cdp_command(&mut live_cdp_peer);
                let retry_still_live = actor.hosts.get(&9).is_some_and(|page| {
                    page.generation == 45 && !actor.cancelled_hosts.contains(&9)
                });

                actor.protocol_responses.insert(
                    6,
                    serde_json::json!({"id": 6, "result": {"targetId": "retry-target"}}),
                );
                actor
                    .target_sessions
                    .insert(String::from("retry-target"), String::from("retry-session"));
                actor
                    .session_targets
                    .insert(String::from("retry-session"), String::from("retry-target"));
                actor
                    .sessions
                    .insert(String::from("retry-session"), (9, true));
                if let Some(page) = actor.hosts.get_mut(&9) {
                    page.session = Some(String::from("retry-session"));
                }
                actor.create_page(worker, create_retry(45));
                let recovered_create = read_cdp_command(&mut live_cdp_peer);
                let recovery_accepted = actor.hosts.get(&9).is_some_and(|page| {
                    page.generation == 45 && page.target.as_deref() == Some("retry-target")
                });

                actor.cancelled_hosts.insert(8);
                actor.pending_page_create = Some((9, 46));
                actor.abandon_target_create_response(88, 9, 46);
                actor.boot_host(worker, boot_retry(46));
                let retired_epoch = actor.browser_epoch;
                let safe_to_restart = actor
                    .prepare_page_create(worker, 9)
                    .expect("no other live host depends on the unresolved browser");
                let browser_retired = actor.browser.is_none() && actor.cdp.is_none();
                let unresolved_create_cleared = actor.cancelled_target_create_response.is_none();
                let epoch_advanced = actor.browser_epoch == retired_epoch + 1;

                let [retry_peer, retry_pipe] =
                    make_pipe().expect("make the fresh browser's CDP pipe");
                actor.cdp = Some(CdpWriter {
                    file: std::fs::File::from(retry_pipe),
                    next_id: 1,
                    poisoned: false,
                    write_blocked: None,
                });
                let retry_child = bt_pty::test_shell::Hygiene::new()
                    .command("/bin/true", crate::quiet_command)
                    .spawn()
                    .expect("start the retired browser's replacement stand-in");
                actor.browser = Some(BrowserProcess {
                    child: retry_child,
                    profile: folder,
                    extension: std::path::PathBuf::new(),
                    _reader: None,
                    output: None,
                });
                let mut restarted_cdp_peer =
                    std::io::BufReader::new(std::fs::File::from(retry_peer));
                actor.protocol_responses.insert(
                    1,
                    serde_json::json!({"id": 1, "result": {"targetId": "fresh-target"}}),
                );
                actor
                    .target_sessions
                    .insert(String::from("fresh-target"), String::from("fresh-session"));
                actor
                    .session_targets
                    .insert(String::from("fresh-session"), String::from("fresh-target"));
                actor
                    .sessions
                    .insert(String::from("fresh-session"), (9, true));
                if let Some(page) = actor.hosts.get_mut(&9) {
                    page.session = Some(String::from("fresh-session"));
                }
                actor.create_page(worker, create_retry(46));
                let post_restart_create = read_cdp_command(&mut restarted_cdp_peer);
                let retried_after_retirement = actor.hosts.get(&9).is_some_and(|page| {
                    page.generation == 46 && page.target.as_deref() == Some("fresh-target")
                });
                actor.finish_browser(worker, true);
                finished
                    .send((
                        other_generation,
                        retry_generation,
                        retry_admitted,
                        retry_still_live,
                        healthy_host_command,
                        healthy_navigation_command,
                        healthy_frame_ack,
                        healthy_frame,
                        late_response_close,
                        late_attachment_close,
                        disposition_cleared,
                        recovered_create,
                        recovery_accepted,
                        safe_to_restart,
                        browser_retired,
                        unresolved_create_cleared,
                        epoch_advanced,
                        post_restart_create,
                        retried_after_retirement,
                    ))
                    .expect("report retry gating, live-host progress, and recovery");
            },
        )
        .expect("start the fake actor worker");
        let (
            other_generation,
            retry_generation,
            retry_admitted,
            retry_still_live,
            healthy_host_command,
            healthy_navigation_command,
            healthy_frame_ack,
            healthy_frame,
            late_response_close,
            late_attachment_close,
            disposition_cleared,
            recovered_create,
            recovery_accepted,
            safe_to_restart,
            browser_retired,
            unresolved_create_cleared,
            epoch_advanced,
            post_restart_create,
            retried_after_retirement,
        ) = wait_finished.recv().expect("the fake retry completed");
        worker.join().expect("the fake actor worker returned");
        assert_eq!(other_generation, Some(8));
        assert_eq!(retry_generation, Some(45));
        assert!(retry_admitted);
        assert!(retry_still_live);
        assert_eq!(
            healthy_host_command["method"],
            "Emulation.setPageScaleFactor"
        );
        assert_eq!(healthy_navigation_command["method"], "Page.navigate");
        assert_eq!(healthy_frame_ack["method"], "Page.screencastFrameAck");
        assert_eq!(
            healthy_frame,
            Some((8, 1, crate::PageVisual { tab: 2, seat: 3 }))
        );
        for command in [late_response_close, late_attachment_close] {
            assert_eq!(command["method"], "Target.closeTarget");
            assert_eq!(command["params"]["targetId"], "old-generation-target");
        }
        assert!(disposition_cleared);
        assert_eq!(recovered_create["method"], "Target.createTarget");
        assert!(recovery_accepted);
        assert!(safe_to_restart);
        assert!(browser_retired);
        assert!(unresolved_create_cleared);
        assert!(epoch_advanced);
        assert_eq!(post_restart_create["method"], "Target.createTarget");
        assert!(retried_after_retirement);
    }

    #[test]
    fn chromium_path_switch_preserves_spaces_and_non_utf8_bytes() {
        use std::os::unix::ffi::OsStrExt;

        let path_bytes = b"/tmp/Folio profile/\xffChromium";
        let path = std::path::Path::new(std::ffi::OsStr::from_bytes(path_bytes));
        let argument = super::chromium_path_switch("--user-data-dir", path);
        assert_eq!(
            argument.as_os_str().as_bytes(),
            b"--user-data-dir=/tmp/Folio profile/\xffChromium"
        );
    }

    #[test]
    fn a_full_cdp_pipe_yields_to_a_queued_close() {
        let _lock = ACTOR_PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ACTOR_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        PENDING_ACTOR_CANCELLATIONS.store(0, Ordering::SeqCst);
        let [reader, writer] = make_pipe().expect("make a CDP pipe");
        let mut writer = std::fs::File::from(writer);
        set_nonblocking(&writer).expect("make the actor's CDP writes cancellable");
        let full = [0_u8; 8192];
        loop {
            match writer.write(&full) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("fill the CDP pipe: {error}"),
            }
        }

        let (blocked, wait_blocked) = mpsc::channel();
        let cdp = CdpWriter {
            file: writer,
            next_id: 1,
            poisoned: false,
            write_blocked: Some(blocked),
        };
        let (commands, received) = mpsc::channel();
        let handle = ActorHandle(commands);
        let (result, wait_result) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            let mut cdp = cdp;
            let answer = cdp.send(
                None,
                "Input.insertText",
                serde_json::json!({"text": "paste data"}),
            );
            let _ = result.send((answer, cdp.poisoned));
        });
        wait_blocked
            .recv()
            .expect("the test writer reached the full pipe and polled for writable space");
        handle
            .send(ActorCommand::Close { host: 7 })
            .expect("queue host close while CDP is back-pressured");
        let reader_thread = std::thread::spawn(move || {
            let mut reader = std::fs::File::from(reader);
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).expect("drain the CDP peer");
        });
        let (answer, poisoned) = wait_result
            .recv()
            .expect("the full-pipe writer observed the queued cancellation");
        let error = answer.expect_err("a cancelled CDP write cannot report success");
        assert!(error.contains("actor cancellation"), "{error}");
        assert!(
            !poisoned,
            "a write canceled before its first byte keeps framing"
        );
        assert!(matches!(
            received
                .recv()
                .expect("the close remains in the actor mailbox"),
            ActorInput::Command(ActorCommand::Close { host: 7 })
        ));
        consume_actor_cancellation();
        writer.join().expect("the cancelled CDP writer returned");
        reader_thread.join().expect("the CDP peer reached EOF");
        assert_eq!(PENDING_ACTOR_CANCELLATIONS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_cancelled_partial_cdp_frame_poisons_the_stream_before_the_next_command() {
        let _lock = ACTOR_PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ACTOR_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        PENDING_ACTOR_CANCELLATIONS.store(0, Ordering::SeqCst);
        let [reader, writer] = make_pipe().expect("make a CDP pipe");
        let mut reader = std::fs::File::from(reader);
        let mut writer = std::fs::File::from(writer);
        set_nonblocking(&reader).expect("make the test pipe readable without waiting");
        set_nonblocking(&writer).expect("make the actor's CDP writes cancellable");
        let full = [0_u8; 8192];
        let mut capacity = 0_usize;
        loop {
            match writer.write(&full) {
                Ok(count) => capacity += count,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("fill the CDP pipe: {error}"),
            }
        }
        let mut remaining = capacity;
        let mut discarded = [0_u8; 8192];
        while remaining > 0 {
            let chunk_len = remaining.min(discarded.len());
            let count = reader
                .read(&mut discarded[..chunk_len])
                .expect("drain the prefilled pipe");
            assert_ne!(count, 0, "the measured pipe bytes must be readable");
            remaining -= count;
        }

        let (blocked, wait_blocked) = mpsc::channel();
        let cdp = CdpWriter {
            file: writer,
            next_id: 1,
            poisoned: false,
            write_blocked: Some(blocked),
        };
        let (commands, received) = mpsc::channel();
        let handle = ActorHandle(commands);
        let (result, wait_result) = mpsc::channel();
        let writer_thread = std::thread::spawn(move || {
            let mut cdp = cdp;
            let payload = "p".repeat(capacity + 8192);
            let answer = cdp.send(
                None,
                "Input.insertText",
                serde_json::json!({"text": payload}),
            );
            let poisoned = cdp.poisoned;
            let next = cdp.send(None, "Browser.getVersion", serde_json::json!({}));
            let _ = result.send((answer, poisoned, next));
        });
        wait_blocked
            .recv()
            .expect("the large CDP frame wrote a prefix, then blocked");
        handle
            .send(ActorCommand::Close { host: 9 })
            .expect("queue close while a partial command has filled the pipe");
        // The owned peer drains after cancellation, so ignoring Close completes
        // the write and fails the outcome assertions rather than hanging.
        // SAFETY: this is the test's live pipe descriptor; only its file status
        // flags change, and the reader owns it until EOF.
        assert_eq!(
            unsafe { libc::fcntl(reader.as_raw_fd(), libc::F_SETFL, 0) },
            0
        );
        let reader_thread = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).expect("drain the CDP peer");
            bytes
        });
        let (answer, poisoned, next) = wait_result
            .recv()
            .expect("the partial writer observes cancellation");
        let error = answer.expect_err("the interrupted command cannot report success");
        assert!(error.contains("actor cancellation"), "{error}");
        assert!(poisoned, "a partial JSON command must poison its stream");
        assert!(
            next.expect_err("the following command cannot share a partial CDP frame")
                .contains("poisoned")
        );
        assert!(matches!(
            received
                .recv()
                .expect("the close remains in the actor mailbox"),
            ActorInput::Command(ActorCommand::Close { host: 9 })
        ));
        consume_actor_cancellation();
        writer_thread
            .join()
            .expect("the partial CDP writer returned");
        let partial = reader_thread.join().expect("the CDP peer reached EOF");
        assert!(!partial.is_empty(), "the failed frame wrote a prefix");
        assert!(
            !partial.contains(&0),
            "the incomplete frame has no terminator"
        );
        assert_eq!(PENDING_ACTOR_CANCELLATIONS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn iframe_caret_uses_the_browser_content_quad_transform() {
        let transformed = map_rect_through_quad(
            [71.0, 17.0, 71.0, 38.0],
            (210.0, 120.0),
            [
                89.15625, 73.15625, 278.15625, 73.15625, 278.15625, 181.15625, 89.15625, 181.15625,
            ],
        )
        .expect("map a caret through the engine-reported OOPIF content quad");
        let expected = [153.05625, 88.45625, 153.05625, 107.35625];
        for (actual, expected) in transformed.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 0.001);
        }

        let perspective = map_rect_through_quad(
            [0.0, 0.0, 100.0, 100.0],
            (100.0, 100.0),
            [10.0, 20.0, 110.0, 20.0, 90.0, 100.0, 20.0, 100.0],
        )
        .expect("map all four corners through the engine-reported perspective quad");
        assert_eq!(perspective, [10.0, 20.0, 110.0, 100.0]);
    }
}

#[cfg(test)]
mod pdf_viewer_resource_tests {
    use super::{is_owned_pdf_viewer_resource, retire_detached_frames, update_frame_parent};
    use std::collections::HashMap;

    #[test]
    fn only_the_owned_builtin_pdf_frame_receives_chrome_resources() {
        let host = 7;
        let session = "pdf-viewer-session";
        let main = "file-main-frame";
        let viewer = "pdf-viewer-frame";
        let ordinary = "web-content-frame";
        let frame_hosts = HashMap::from([
            (main.to_owned(), host),
            (viewer.to_owned(), host),
            (ordinary.to_owned(), host),
        ]);
        let frame_sessions = HashMap::from([
            (main.to_owned(), String::from("main-session")),
            (viewer.to_owned(), session.to_owned()),
            (ordinary.to_owned(), session.to_owned()),
        ]);
        let frame_parents = HashMap::from([
            (viewer.to_owned(), main.to_owned()),
            (ordinary.to_owned(), main.to_owned()),
        ]);
        let frame_urls = HashMap::from([
            (
                viewer.to_owned(),
                String::from("chrome-extension://mhjfbmdgcfjbbpaeojofohoefgiehjai/index.html"),
            ),
            (ordinary.to_owned(), String::from("https://page.example/")),
        ]);

        assert!(is_owned_pdf_viewer_resource(
            host,
            session,
            viewer,
            "chrome://resources/css/text_defaults_md.css",
            main,
            &frame_hosts,
            &frame_sessions,
            &frame_parents,
            &frame_urls,
        ));
        assert!(!is_owned_pdf_viewer_resource(
            host,
            session,
            ordinary,
            "chrome://resources/css/text_defaults_md.css",
            main,
            &frame_hosts,
            &frame_sessions,
            &frame_parents,
            &frame_urls,
        ));
        assert!(!is_owned_pdf_viewer_resource(
            host,
            "another-session",
            viewer,
            "chrome://resources/css/text_defaults_md.css",
            main,
            &frame_hosts,
            &frame_sessions,
            &frame_parents,
            &frame_urls,
        ));
        let wrong_parent = HashMap::from([(viewer.to_owned(), String::from("other-main"))]);
        assert!(!is_owned_pdf_viewer_resource(
            host,
            session,
            viewer,
            "chrome://resources/css/text_defaults_md.css",
            main,
            &frame_hosts,
            &frame_sessions,
            &wrong_parent,
            &frame_urls,
        ));
        assert!(!is_owned_pdf_viewer_resource(
            host,
            session,
            viewer,
            "chrome://settings/",
            main,
            &frame_hosts,
            &frame_sessions,
            &frame_parents,
            &frame_urls,
        ));
        assert!(!is_owned_pdf_viewer_resource(
            host,
            session,
            viewer,
            "file:///home/user/private.pdf",
            main,
            &frame_hosts,
            &frame_sessions,
            &frame_parents,
            &frame_urls,
        ));
    }

    #[test]
    fn a_target_local_oopif_root_keeps_its_known_global_parent() {
        let mut parents =
            HashMap::from([("pdf-viewer-frame".to_owned(), "file-main-frame".to_owned())]);
        let mut roots = HashMap::from([(
            "pdf-viewer-session".to_owned(),
            "pdf-viewer-frame".to_owned(),
        )]);

        update_frame_parent(
            &mut parents,
            &mut roots,
            "pdf-viewer-session",
            "pdf-viewer-frame",
            None,
            false,
        );

        assert_eq!(
            parents.get("pdf-viewer-frame").map(String::as_str),
            Some("file-main-frame")
        );
        assert_eq!(
            roots.get("pdf-viewer-session").map(String::as_str),
            Some("pdf-viewer-frame")
        );
    }

    #[test]
    fn only_the_browser_root_can_replace_a_frame_with_no_parent() {
        let mut parents = HashMap::from([("main-frame".to_owned(), "stale-parent".to_owned())]);
        let mut roots = HashMap::new();

        update_frame_parent(
            &mut parents,
            &mut roots,
            "main-session",
            "main-frame",
            None,
            true,
        );

        assert!(!parents.contains_key("main-frame"));
        assert_eq!(
            roots.get("main-session").map(String::as_str),
            Some("main-frame")
        );
    }

    #[test]
    fn a_swapped_frame_keeps_its_global_owner_parent_and_child_session() {
        let mut frames = HashMap::from([
            ("main".to_owned(), 7),
            ("viewer".to_owned(), 7),
            ("nested".to_owned(), 7),
            ("sibling".to_owned(), 7),
        ]);
        let mut parents = HashMap::from([
            ("viewer".to_owned(), "main".to_owned()),
            ("nested".to_owned(), "viewer".to_owned()),
            ("sibling".to_owned(), "main".to_owned()),
        ]);
        let mut sessions = HashMap::from([
            ("main".to_owned(), "main-session".to_owned()),
            ("viewer".to_owned(), "new-oopif-session".to_owned()),
            ("nested".to_owned(), "nested-session".to_owned()),
            ("sibling".to_owned(), "main-session".to_owned()),
        ]);
        let mut urls = HashMap::from([
            ("main".to_owned(), "http://127.0.0.1/".to_owned()),
            ("viewer".to_owned(), "http://127.0.0.2/oopif".to_owned()),
            ("nested".to_owned(), "http://nested.example/".to_owned()),
            ("sibling".to_owned(), "http://127.0.0.1/sibling".to_owned()),
        ]);

        let retired = retire_detached_frames(
            "swap",
            "viewer",
            &mut frames,
            &mut parents,
            &mut sessions,
            &mut urls,
        );

        assert!(retired.is_empty());
        assert_eq!(frames.get("viewer"), Some(&7));
        assert_eq!(parents.get("viewer").map(String::as_str), Some("main"));
        assert_eq!(
            sessions.get("viewer").map(String::as_str),
            Some("new-oopif-session")
        );
        assert_eq!(
            urls.get("viewer").map(String::as_str),
            Some("http://127.0.0.2/oopif")
        );
        assert_eq!(parents.get("nested").map(String::as_str), Some("viewer"));

        let retired = retire_detached_frames(
            "remove",
            "viewer",
            &mut frames,
            &mut parents,
            &mut sessions,
            &mut urls,
        );

        assert_eq!(retired.len(), 2);
        assert!(!frames.contains_key("viewer"));
        assert!(!frames.contains_key("nested"));
        assert!(!parents.contains_key("viewer"));
        assert!(!parents.contains_key("nested"));
        assert!(frames.contains_key("main"));
        assert!(frames.contains_key("sibling"));
    }
}

#[cfg(test)]
mod worker_setup_tests {
    use super::{
        ACTOR_PIPE_TEST_LOCK, ACTOR_SHUTDOWN_REQUESTED, Actor, ActorNotice, CdpWriter, HostInbox,
        HostPage, PENDING_ACTOR_CANCELLATIONS, make_pipe,
    };
    use std::fs::File;
    use std::io::Read;
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, mpsc};

    struct SetupResult {
        methods: Vec<String>,
        target_closed: bool,
        setup_cleared: bool,
        owner_failed: bool,
        fetch_ack_kept_setup_pending: bool,
    }

    fn run_setup(fail_fetch: bool) -> SetupResult {
        let _lock = ACTOR_PIPE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ACTOR_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
        PENDING_ACTOR_CANCELLATIONS.store(0, Ordering::SeqCst);
        let (finished, wait_finished) = mpsc::channel();
        let worker = crate::spawn_at_priority(
            "bt-linux-web-worker-setup-test",
            crate::ThreadPriority::BelowNormal,
            move |worker_ctx| {
                let [pipe_read, pipe_write] = make_pipe().expect("make the fake CDP pipe");
                let (sender, receiver) = mpsc::channel();
                let mut actor = Actor::new(worker_ctx, sender, receiver);
                actor.cdp = Some(CdpWriter {
                    file: File::from(pipe_write),
                    next_id: 1,
                    poisoned: false,
                    write_blocked: None,
                });
                let inbox = Arc::new(HostInbox::default());
                actor.hosts.insert(
                    7,
                    HostPage {
                        inbox: inbox.clone(),
                        wake: Arc::new(|| {}),
                        folder: std::env::temp_dir().join("folio-worker-setup-test"),
                        generation: 19,
                        page: Some(crate::PageVisual { tab: 1, seat: 2 }),
                        rules: String::new(),
                        color_scheme: None,
                        bounds: (0, 0, 640, 360),
                        scale: 1.0,
                        visible: true,
                        sequence: 0,
                        screencast_started: false,
                        screencast_frame_reported: false,
                        screencast_refresh_pending: false,
                        destination_bounds_ready: true,
                        target: None,
                        session: None,
                        tab_id: None,
                        main_frame: None,
                        policy_token: None,
                        navigation_binding: None,
                        navigation_permit: None,
                        latest_navigation_request: None,
                        ime_binding: None,
                        ime_token: None,
                        last_ime_cursor: None,
                        main_context: None,
                        browser_process_id: 0,
                        installed: false,
                        main_loader: None,
                        main_status: 0,
                        find_term: String::new(),
                        find_case_sensitive: false,
                        find_count: 0,
                        find_active: 0,
                        last_url: String::new(),
                    },
                );
                actor
                    .configure_child("worker-session", "worker-target", "service_worker", 7, true)
                    .expect("queue worker policy setup before resuming it");
                let fetch_id = actor
                    .pending_worker_responses
                    .iter()
                    .find_map(|(id, (_, method))| (*method == "Fetch.enable").then_some(*id))
                    .expect("track the Fetch.enable result");
                let auto_attach_id = actor
                    .pending_worker_responses
                    .iter()
                    .find_map(|(id, (_, method))| {
                        (*method == "Target.setAutoAttach").then_some(*id)
                    })
                    .expect("track the Target.setAutoAttach result");

                let fetch_ack_kept_setup_pending = if fail_fetch {
                    actor.handle_protocol_response(serde_json::json!({
                        "id": fetch_id,
                        "error": {"message": "probe rejected Fetch.enable"}
                    }));
                    false
                } else {
                    actor.handle_protocol_response(serde_json::json!({
                        "id": fetch_id,
                        "result": {}
                    }));
                    actor.pending_worker_setups.contains_key("worker-session")
                };
                if !fail_fetch {
                    actor.handle_protocol_response(serde_json::json!({
                        "id": auto_attach_id,
                        "result": {}
                    }));
                }

                let setup_cleared = !actor.pending_worker_setups.contains_key("worker-session");
                let owner_failed = inbox.take().iter().any(|notice| {
                    matches!(
                        notice,
                        ActorNotice::Event {
                            generation: 19,
                            event: crate::WebEvent::ProcessFailed { description, .. }
                        } if description.contains("Fetch.enable")
                    )
                });
                drop(actor);

                let mut reader = File::from(pipe_read);
                let mut bytes = Vec::new();
                reader
                    .read_to_end(&mut bytes)
                    .expect("read the fake CDP command stream");
                let methods = bytes
                    .split(|byte| *byte == 0)
                    .filter(|frame| !frame.is_empty())
                    .map(|frame| {
                        serde_json::from_slice::<serde_json::Value>(frame)
                            .expect("decode a CDP command")
                    })
                    .map(|message| {
                        if message.get("method").and_then(serde_json::Value::as_str)
                            == Some("Target.closeTarget")
                        {
                            assert_eq!(
                                message
                                    .pointer("/params/targetId")
                                    .and_then(serde_json::Value::as_str),
                                Some("worker-target")
                            );
                        }
                        message
                            .get("method")
                            .and_then(serde_json::Value::as_str)
                            .expect("a command has a method")
                            .to_owned()
                    })
                    .collect();
                finished
                    .send(SetupResult {
                        methods,
                        target_closed: fail_fetch,
                        setup_cleared,
                        owner_failed,
                        fetch_ack_kept_setup_pending,
                    })
                    .expect("report the worker setup test");
            },
        )
        .expect("start the worker setup test");
        let result = wait_finished
            .recv()
            .expect("the worker setup test finished");
        worker.join().expect("the worker setup thread returned");
        result
    }

    #[test]
    fn worker_fetch_and_autoattach_are_queued_before_the_debugger_resumes() {
        let result = run_setup(false);

        assert_eq!(
            result.methods,
            [
                "Fetch.enable",
                "Target.setAutoAttach",
                "Runtime.runIfWaitingForDebugger"
            ]
        );
        assert!(result.fetch_ack_kept_setup_pending);
        assert!(result.setup_cleared);
        assert!(!result.target_closed);
        assert!(!result.owner_failed);
    }

    #[test]
    fn a_failed_worker_fetch_setup_closes_the_target_and_fails_its_owner() {
        let result = run_setup(true);

        assert_eq!(
            result.methods,
            [
                "Fetch.enable",
                "Target.setAutoAttach",
                "Runtime.runIfWaitingForDebugger",
                "Target.closeTarget"
            ]
        );
        assert!(result.setup_cleared);
        assert!(result.target_closed);
        assert!(result.owner_failed);
        assert!(!result.fetch_ack_kept_setup_pending);
    }
}
