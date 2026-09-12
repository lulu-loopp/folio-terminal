//! X-4 — the AppKit application delegate bridge, probed.
//!
//! A scratch probe, outside Folio's workspace. It asks one question: can a
//! `bt-platform` bridge running on winit 0.30.13's event loop receive the five
//! application-level AppKit events on the main thread and route each of them to
//! a named action with an explicit origin, without blocking and without
//! breaking winit?
//!
//! The route matters more than the events. winit 0.30.13's own
//! `src/platform/macos.rs` says "Winit guarantees that it will not register an
//! application delegate", and that sentence is **false in this version**:
//! `platform_impl/macos/event_loop.rs:240` calls `app.setDelegate(...)` with a
//! `WinitApplicationDelegate`, and `app_state.rs`'s `ApplicationDelegate::get`
//! panics with "tried to get a delegate that was not the one Winit has
//! registered" if anything else is found there — and it is called from the
//! CFRunLoop observers on every single loop iteration. So the documented route
//! (install your own delegate) takes winit down. This probe therefore installs
//! nothing: it adds the four selectors winit does **not** implement onto the
//! class winit already registered, with `class_addMethod`, and registers a
//! services provider object, which never goes near the delegate at all.
//!
//! Everything the probe does is logged with its origin and the thread it
//! arrived on, to `~/folio-port/logs/x4-<run>.log`. The first line is the pid.

use std::ffi::{c_char, c_void, CStr};
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{msg_send, sel, ClassType, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSApplicationTerminateReply, NSPasteboard, NSPerformService,
};
use objc2_foundation::{NSArray, NSObject, NSString, NSURL};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

// ---------------------------------------------------------------- logging ---

static START: OnceLock<Instant> = OnceLock::new();
static LOG: OnceLock<Mutex<std::fs::File>> = OnceLock::new();
static ROLE: OnceLock<String> = OnceLock::new();

/// The run name. `open` launches through LaunchServices, which passes none of
/// the shell's environment, so a file beside the logs is the only way a scripted
/// step can name the run the app writes into.
fn run_name() -> String {
    if let Ok(v) = std::env::var("X4_RUN") {
        return v;
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned());
    std::fs::read_to_string(PathBuf::from(home).join("folio-port/logs/x4-run-name"))
        .map(|s| s.trim().to_owned())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "adhoc".to_owned())
}

fn log_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned());
    let dir = PathBuf::from(home).join("folio-port/logs");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("x4-{}.log", run_name()))
}

fn logfile() -> &'static Mutex<std::fs::File> {
    LOG.get_or_init(|| {
        let f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path())
            .expect("probe log");
        Mutex::new(f)
    })
}

/// Every line carries the elapsed time, the role of the process and whether it
/// arrived on the main thread — the last is what "routed on the main thread
/// without blocking" is judged on.
macro_rules! say {
    ($($arg:tt)*) => {{
        let t = START.get_or_init(Instant::now).elapsed().as_millis();
        let role = ROLE.get().map(|s| s.as_str()).unwrap_or("?");
        let main = MainThreadMarker::new().is_some();
        let line = format!("[{t:>7}ms {role} main={main}] {}", format!($($arg)*));
        eprintln!("{line}");
        if let Ok(mut f) = logfile().lock() {
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    }};
}

// -------------------------------------------------- the bridge's own types ---

/// Where an application action came from. This is the "explicit origin" the
/// probe's pass condition names; nothing downstream has to guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    /// `applicationShouldHandleReopen:hasVisibleWindows:` — a second Finder or
    /// Dock launch, or a Dock click, of the already-running app.
    Reopen,
    /// A registered `NSServices` provider method.
    Services,
    /// `applicationShouldTerminate:`.
    Termination,
    /// `applicationShouldTerminateAfterLastWindowClosed:`.
    LastWindowClosed,
}

#[derive(Debug, Clone)]
enum Kind {
    /// "open a window" — the named action Q10 asks reopen to reach.
    OpenWindow { had_visible_windows: bool },
    /// "open these paths" — the named action a Service reaches.
    OpenPaths(Vec<String>),
    /// "the user asked to quit; answer" — the answer is deferred, never blocked.
    TerminationRequested,
    /// "the last window went away; stay in the Dock".
    StayResident,
}

#[derive(Debug, Clone)]
struct AppEvent {
    origin: Origin,
    kind: Kind,
}

/// The delegate methods are C functions with no `self` of ours, so the channel
/// they post into is global. `EventLoopProxy::send_event` is the wake-up: it is
/// non-blocking, and winit delivers the payload to `ApplicationHandler::user_event`
/// on the next turn of the same main-thread run loop.
static PROXY: OnceLock<Mutex<Option<EventLoopProxy<AppEvent>>>> = OnceLock::new();

fn post(ev: AppEvent) {
    if let Some(cell) = PROXY.get() {
        if let Ok(guard) = cell.lock() {
            if let Some(p) = guard.as_ref() {
                let _ = p.send_event(ev);
            }
        }
    }
}

/// 0 = answer Cancel, 1 = answer Now, 2 = answer Later (and refuse afterwards).
static TERM_POLICY: AtomicU8 = AtomicU8::new(0);
static REOPENS: AtomicUsize = AtomicUsize::new(0);
static SERVICES: AtomicUsize = AtomicUsize::new(0);
/// Milliseconds since start at which a deferred termination answer is due, or 0.
static REPLY_DEADLINE_MS: AtomicU64 = AtomicU64::new(0);

const TERMINATE_CANCEL: usize = 0;
const TERMINATE_NOW: usize = 1;
const TERMINATE_LATER: usize = 2;

// ---------------------------------------------- raw Objective-C runtime FFI ---

#[link(name = "objc", kind = "dylib")]
extern "C" {
    fn class_addMethod(
        cls: *mut AnyClass,
        name: Sel,
        imp: *const c_void,
        types: *const c_char,
    ) -> Bool;
    fn class_getInstanceMethod(cls: *const AnyClass, name: Sel) -> *const c_void;
    fn objc_allocateClassPair(
        superclass: *const AnyClass,
        name: *const c_char,
        extra_bytes: usize,
    ) -> *mut AnyClass;
    fn objc_registerClassPair(cls: *mut AnyClass);
    fn class_createInstance(cls: *const AnyClass, extra_bytes: usize) -> *mut AnyObject;
}

unsafe fn add_method(cls: &AnyClass, sel: Sel, imp: *const c_void, types: &CStr, label: &str) {
    let already = class_getInstanceMethod(cls as *const AnyClass, sel);
    let ptr = cls as *const AnyClass as *mut AnyClass;
    let ok = class_addMethod(ptr, sel, imp, types.as_ptr()).as_bool();
    say!(
        "INJECT {label}: winit already implemented it = {}, class_addMethod = {}",
        !already.is_null(),
        ok
    );
}

// ------------------------------------------------------- libdispatch FFI ---
//
// The main dispatch queue is drained by the main run loop *outside* winit's
// handler, which is the difference between a call AppKit makes for us and a
// call we make from inside `ApplicationHandler`. See the report: `terminate:`
// invoked from inside a winit callback re-enters winit's run-loop observers
// while its handler is already borrowed, and winit answers that by panicking
// once per turn.

extern "C" {
    fn dispatch_async_f(
        queue: *mut c_void,
        context: *mut c_void,
        work: extern "C-unwind" fn(*mut c_void),
    );
    static _dispatch_main_q: c_void;
}

fn main_queue() -> *mut c_void {
    unsafe { &_dispatch_main_q as *const c_void as *mut c_void }
}

extern "C-unwind" fn trampoline_terminate(_ctx: *mut c_void) {
    let mtm = MainThreadMarker::new().unwrap();
    say!("DISPATCH calling -[NSApplication terminate:]");
    NSApplication::sharedApplication(mtm).terminate(None);
    say!("DISPATCH terminate: returned, so the app refused to go");
}

// ------------------------------------------------- the four delegate methods ---

extern "C-unwind" fn imp_should_handle_reopen(
    _this: &AnyObject,
    _cmd: Sel,
    _app: &AnyObject,
    has_visible_windows: Bool,
) -> Bool {
    let visible = has_visible_windows.as_bool();
    REOPENS.fetch_add(1, Ordering::SeqCst);
    say!("DELEGATE applicationShouldHandleReopen:hasVisibleWindows: visible={visible}");
    post(AppEvent {
        origin: Origin::Reopen,
        kind: Kind::OpenWindow {
            had_visible_windows: visible,
        },
    });
    // NO when we take responsibility for producing a window ourselves; YES lets
    // AppKit do its default un-miniaturise when one is only minimised.
    Bool::new(visible)
}

extern "C-unwind" fn imp_should_terminate(
    _this: &AnyObject,
    _cmd: Sel,
    _app: &AnyObject,
) -> NSApplicationTerminateReply {
    let policy = TERM_POLICY.load(Ordering::SeqCst);
    let answer = match policy {
        1 => TERMINATE_NOW,
        2 => TERMINATE_LATER,
        _ => TERMINATE_CANCEL,
    };
    say!(
        "DELEGATE applicationShouldTerminate: answering {}",
        match answer {
            TERMINATE_NOW => "NSTerminateNow",
            TERMINATE_LATER => "NSTerminateLater",
            _ => "NSTerminateCancel",
        }
    );
    post(AppEvent {
        origin: Origin::Termination,
        kind: Kind::TerminationRequested,
    });
    if answer == TERMINATE_LATER {
        // The answer leaves this stack immediately. It cannot be scheduled on
        // the main dispatch queue: AppKit's deferred-termination loop does not
        // drain it (measured, run r2). winit's own handler *is* driven inside
        // that loop, so the answer is scheduled there instead.
        REPLY_DEADLINE_MS.store(
            START.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1200,
            Ordering::SeqCst,
        );
        say!("DELEGATE deferred answer scheduled for +1200ms; this stack returns now");
    }
    NSApplicationTerminateReply(answer as _)
}

extern "C-unwind" fn imp_should_terminate_after_last_window_closed(
    _this: &AnyObject,
    _cmd: Sel,
    _app: &AnyObject,
) -> Bool {
    say!("DELEGATE applicationShouldTerminateAfterLastWindowClosed: answering NO");
    post(AppEvent {
        origin: Origin::LastWindowClosed,
        kind: Kind::StayResident,
    });
    Bool::NO
}

extern "C-unwind" fn imp_open_urls(_this: &AnyObject, _cmd: Sel, _app: &AnyObject, urls: &AnyObject) {
    let arr: &NSArray<NSURL> = unsafe { &*(urls as *const AnyObject as *const NSArray<NSURL>) };
    let mut paths = Vec::new();
    for url in arr.iter() {
        if let Some(p) = url.path() {
            paths.push(p.to_string());
        }
    }
    say!("DELEGATE application:openURLs: {paths:?}");
    post(AppEvent {
        origin: Origin::Services,
        kind: Kind::OpenPaths(paths),
    });
}

// ------------------------------------------------ the services provider object ---

extern "C-unwind" fn imp_open_in_folio_probe(
    _this: &AnyObject,
    _cmd: Sel,
    pboard: &AnyObject,
    _user_data: *mut AnyObject,
    _error: *mut *mut AnyObject,
) {
    let pb: &NSPasteboard = unsafe { &*(pboard as *const AnyObject as *const NSPasteboard) };
    let mut paths = Vec::new();

    // Read the way Finder writes: one pasteboard item per selected file, each
    // carrying `public.file-url`. Percent-encoding is what carries the spaces
    // and the non-ASCII, so the URL is decoded rather than string-copied.
    let file_url_type = NSString::from_str("public.file-url");
    if let Some(items) = pb.pasteboardItems() {
        for item in items.iter() {
            if let Some(s) = item.stringForType(&file_url_type) {
                let raw = s.to_string();
                match NSURL::URLWithString(&s) {
                    Some(u) => match u.path() {
                        Some(p) => paths.push(p.to_string()),
                        None => paths.push(format!("<no path> {raw}")),
                    },
                    None => paths.push(format!("<no url> {raw}")),
                }
            }
        }
    }

    SERVICES.fetch_add(1, Ordering::SeqCst);
    say!("SERVICE openInFolioProbe:userData:error: count={} paths={paths:?}", paths.len());
    post(AppEvent {
        origin: Origin::Services,
        kind: Kind::OpenPaths(paths),
    });
}

unsafe fn install_services_provider(mtm: MainThreadMarker) {
    let name = c"ProbeX4ServicesProvider";
    let cls = objc_allocateClassPair(NSObject::class() as *const AnyClass, name.as_ptr(), 0);
    assert!(!cls.is_null(), "could not allocate the services provider class");
    let ok = class_addMethod(
        cls,
        sel!(openInFolioProbe:userData:error:),
        imp_open_in_folio_probe as *const c_void,
        c"v@:@@^@".as_ptr(),
    )
    .as_bool();
    objc_registerClassPair(cls);
    let obj = class_createInstance(cls as *const AnyClass, 0);
    assert!(!obj.is_null(), "could not create the services provider");
    let app = NSApplication::sharedApplication(mtm);
    app.setServicesProvider(Some(&*obj));
    let back = app.servicesProvider();
    say!(
        "SERVICES provider registered: method added = {ok}, NSApp.servicesProvider is set = {}",
        back.is_some()
    );
    // `class_createInstance` hands back a +1 reference that is never released:
    // the provider must outlive every Service invocation.
}

// ------------------------------------------------------------- the injection ---

/// The whole route, in four calls. It must run **after** `EventLoop::new`,
/// because that is what registers `WinitApplicationDelegate` with the runtime.
unsafe fn inject_into_winits_delegate() -> bool {
    let Some(cls) = AnyClass::get(c"WinitApplicationDelegate") else {
        say!("INJECT FAILED: no class named WinitApplicationDelegate in the runtime");
        return false;
    };
    say!("INJECT target class = {} (superclass {:?})", cls.name().to_string_lossy(), cls.superclass().map(|c| c.name().to_string_lossy().into_owned()));

    add_method(
        cls,
        sel!(applicationShouldHandleReopen:hasVisibleWindows:),
        imp_should_handle_reopen as *const c_void,
        c"B@:@B",
        "applicationShouldHandleReopen:hasVisibleWindows:",
    );
    add_method(
        cls,
        sel!(applicationShouldTerminate:),
        imp_should_terminate as *const c_void,
        c"Q@:@",
        "applicationShouldTerminate:",
    );
    add_method(
        cls,
        sel!(applicationShouldTerminateAfterLastWindowClosed:),
        imp_should_terminate_after_last_window_closed as *const c_void,
        c"B@:@",
        "applicationShouldTerminateAfterLastWindowClosed:",
    );
    add_method(
        cls,
        sel!(application:openURLs:),
        imp_open_urls as *const c_void,
        c"v@:@@",
        "application:openURLs:",
    );
    true
}

// ------------------------------------------------------------ remote control ---

fn control_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned());
    PathBuf::from(home).join(format!("folio-port/logs/x4-{}.control", run_name()))
}

/// Reads and clears the control file. Every command is a line; the file is a
/// stand-in for the parts of the exercise that would otherwise need
/// Accessibility (a Dock click, a menu pick).
fn take_control_commands() -> Vec<String> {
    let p = control_path();
    let Ok(text) = std::fs::read_to_string(&p) else {
        return Vec::new();
    };
    if text.trim().is_empty() {
        return Vec::new();
    }
    let _ = std::fs::write(&p, "");
    text.lines()
        .map(|l| l.trim().to_owned())
        .filter(|l| !l.is_empty())
        .collect()
}

// -------------------------------------------------------------- the winit app ---

struct App {
    windows: Vec<Window>,
    next_id: usize,
    about_to_wait: usize,
    window_events: usize,
    user_events: usize,
    last_control_poll: Instant,
}

impl App {
    fn new() -> Self {
        Self {
            windows: Vec::new(),
            next_id: 0,
            about_to_wait: 0,
            window_events: 0,
            user_events: 0,
            last_control_poll: Instant::now(),
        }
    }

    fn open_window(&mut self, el: &ActiveEventLoop, why: &str) {
        self.next_id += 1;
        let attrs = Window::default_attributes()
            .with_title(format!("Folio X-4 probe #{}", self.next_id))
            .with_inner_size(winit::dpi::LogicalSize::new(520.0, 320.0));
        match el.create_window(attrs) {
            Ok(w) => {
                say!("ACTION open_window #{} ({why}) — now {} open", self.next_id, self.windows.len() + 1);
                self.windows.push(w);
            }
            Err(e) => say!("ACTION open_window FAILED ({why}): {e}"),
        }
    }

    fn run_command(&mut self, el: &ActiveEventLoop, cmd: &str) {
        say!("CONTROL {cmd}");
        let mtm = MainThreadMarker::new().expect("control runs on the main thread");
        let app = NSApplication::sharedApplication(mtm);
        match cmd {
            "openwin" => self.open_window(el, "control"),
            "closeall" => {
                let n = self.windows.len();
                self.windows.clear();
                say!("ACTION closed {n} window(s); zero remain");
            }
            "hide" => app.hide(None),
            "unhide" => app.unhide(None),
            "mini" => {
                for w in &self.windows {
                    w.set_minimized(true);
                }
                say!("ACTION miniaturised {} window(s)", self.windows.len());
            }
            "term-cancel" | "term-later" | "term-now" => {
                TERM_POLICY.store(
                    match cmd {
                        "term-now" => 1,
                        "term-later" => 2,
                        _ => 0,
                    },
                    Ordering::SeqCst,
                );
                unsafe {
                    dispatch_async_f(main_queue(), std::ptr::null_mut(), trampoline_terminate)
                };
                say!("ACTION queued terminate: on the main dispatch queue for {cmd}");
            }
            "status" => {
                say!(
                    "STATUS windows={} reopens={} services={} winit(about_to_wait={} window_events={} user_events={})",
                    self.windows.len(),
                    REOPENS.load(Ordering::SeqCst),
                    SERVICES.load(Ordering::SeqCst),
                    self.about_to_wait,
                    self.window_events,
                    self.user_events
                );
            }
            "exit" => {
                say!("ACTION exiting the winit event loop");
                el.exit();
            }
            other => say!("CONTROL unknown command {other:?}"),
        }
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        say!("WINIT resumed");
        if self.windows.is_empty() && self.next_id == 0 {
            self.open_window(el, "startup");
        }
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.window_events += 1;
        if matches!(event, WindowEvent::CloseRequested) {
            say!("WINIT CloseRequested for {id:?}");
            self.windows.retain(|w| w.id() != id);
            say!("ACTION window closed; {} remain", self.windows.len());
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: AppEvent) {
        self.user_events += 1;
        say!("ROUTED origin={:?} kind={:?}", event.origin, event.kind);
        match (event.origin, event.kind) {
            (Origin::Reopen, Kind::OpenWindow { had_visible_windows }) => {
                // Q10: a reopen with no window open must produce one.
                if self.windows.is_empty() {
                    self.open_window(el, "reopen with no windows");
                } else {
                    say!(
                        "ACTION reopen with {} window(s) already open (had_visible={had_visible_windows}) — raising, not opening",
                        self.windows.len()
                    );
                    if let Some(w) = self.windows.first() {
                        w.set_minimized(false);
                        w.focus_window();
                    }
                }
            }
            (Origin::Services, Kind::OpenPaths(paths)) => {
                say!("ACTION open {} path(s) from a Service", paths.len());
                for p in &paths {
                    say!("ACTION   path {p}");
                }
                self.open_window(el, "services delivery");
            }
            (Origin::Termination, Kind::TerminationRequested) => {
                say!("ACTION the quit request reached the application while AppKit waits");
            }
            (Origin::LastWindowClosed, Kind::StayResident) => {
                say!("ACTION staying resident in the Dock with zero windows");
            }
            (o, k) => say!("ACTION unhandled {o:?} {k:?}"),
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.about_to_wait += 1;

        let due = REPLY_DEADLINE_MS.load(Ordering::SeqCst);
        if due != 0 && START.get_or_init(Instant::now).elapsed().as_millis() as u64 >= due {
            REPLY_DEADLINE_MS.store(0, Ordering::SeqCst);
            let mtm = MainThreadMarker::new().unwrap();
            say!("ACTION answering replyToApplicationShouldTerminate:NO — unsaved state kept the app alive");
            NSApplication::sharedApplication(mtm).replyToApplicationShouldTerminate(false);
            say!("ACTION reply delivered; AppKit's deferred-termination loop should have unwound");
        }

        if self.last_control_poll.elapsed() >= Duration::from_millis(150) {
            self.last_control_poll = Instant::now();
            for cmd in take_control_commands() {
                self.run_command(el, &cmd);
                if el.exiting() {
                    return;
                }
            }
        }

        el.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(150),
        ));
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        say!(
            "WINIT exiting — about_to_wait={} window_events={} user_events={} reopens={} services={}",
            self.about_to_wait,
            self.window_events,
            self.user_events,
            REOPENS.load(Ordering::SeqCst),
            SERVICES.load(Ordering::SeqCst)
        );
    }
}

// ------------------------------------------------------------ the sender mode ---

/// `probe-x4 --send-service <path>...` builds the pasteboard Finder builds and
/// hands it to `NSPerformService`, which is the only way to exercise a Service
/// end to end without a human in the Finder.
fn send_service(paths: &[String]) {
    let mtm = MainThreadMarker::new().expect("sender runs on the main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let name = NSString::from_str("ProbeX4Pasteboard");
    let pb = NSPasteboard::pasteboardWithName(&name);
    pb.clearContents();

    let urls: Vec<Retained<NSURL>> = paths
        .iter()
        .map(|p| NSURL::fileURLWithPath(&NSString::from_str(p)))
        .collect();
    let objs: Vec<Retained<objc2::runtime::ProtocolObject<dyn objc2_app_kit::NSPasteboardWriting>>> =
        urls.iter()
            .map(|u| objc2::runtime::ProtocolObject::from_retained(u.clone()))
            .collect();
    let arr = NSArray::from_retained_slice(&objs);
    let wrote = pb.writeObjects(&arr);
    say!("SENDER wrote {} url(s) to the pasteboard: {wrote}", urls.len());
    for p in paths {
        say!("SENDER   {p}");
    }

    let item = NSString::from_str(
        &std::env::var("X4_SERVICE_NAME").unwrap_or_else(|_| "Open in Folio Probe".to_owned()),
    );
    let ok = NSPerformService(&item, Some(&pb));
    say!("SENDER NSPerformService({item:?}) = {ok}");
}

// ------------------------------------------------------------------- main ---

fn main() {
    START.get_or_init(Instant::now);
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(|s| s.as_str()) == Some("--send-service") {
        let _ = ROLE.set("sender".to_owned());
        say!("pid={}", std::process::id());
        send_service(&args[1..]);
        return;
    }

    let _ = ROLE.set("app".to_owned());
    say!("pid={}", std::process::id());
    say!("argv={:?}", std::env::args().collect::<Vec<_>>());
    say!("control file = {}", control_path().display());

    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .expect("event loop");
    let _ = PROXY.set(Mutex::new(Some(event_loop.create_proxy())));

    let mtm = MainThreadMarker::new().unwrap();
    let bundle_id = objc2_app_kit::NSRunningApplication::currentApplication()
        .bundleIdentifier()
        .map(|s| s.to_string());
    say!("bundle identifier = {bundle_id:?}");

    unsafe {
        inject_into_winits_delegate();
        install_services_provider(mtm);
    }

    // Proof that the delegate is still winit's own object, which is what
    // `ApplicationDelegate::get` asserts on every run-loop iteration.
    let app = NSApplication::sharedApplication(mtm);
    let delegate = app.delegate();
    let delegate_class = delegate
        .as_ref()
        .map(|d| {
            let cls: &AnyClass = unsafe { msg_send![&**d, class] };
            cls.name().to_string_lossy().into_owned()
        })
        .unwrap_or_else(|| "<none>".to_owned());
    say!("NSApp.delegate class = {delegate_class}");
    if let Some(d) = delegate.as_ref() {
        for sel in [
            sel!(applicationShouldHandleReopen:hasVisibleWindows:),
            sel!(applicationShouldTerminate:),
            sel!(applicationShouldTerminateAfterLastWindowClosed:),
            sel!(application:openURLs:),
        ] {
            let responds: bool = unsafe { msg_send![&**d, respondsToSelector: sel] };
            say!("NSApp.delegate respondsToSelector:{sel:?} = {responds}");
        }
    }

    // `-[NSApplication setDelegate:]` caches which delegate methods exist at the
    // moment it is called, and winit called it before these four existed. Handing
    // the very same object back re-computes that mask; the object is still a
    // `WinitApplicationDelegate`, so winit's own assertion is untouched.
    let skip_reset = {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned());
        PathBuf::from(home)
            .join("folio-port/logs/x4-skip-delegate-reset")
            .exists()
    };
    if skip_reset {
        say!("DELEGATE mask NOT re-computed (x4-skip-delegate-reset present)");
    } else if let Some(d) = delegate.as_ref() {
        app.setDelegate(None);
        app.setDelegate(Some(d));
        say!("DELEGATE mask re-computed by handing winit's own delegate back to setDelegate:");
    }

    let mut app_handler = App::new();
    match event_loop.run_app(&mut app_handler) {
        Ok(()) => say!("RUN_APP returned Ok"),
        Err(e) => say!("RUN_APP returned Err: {e}"),
    }
    say!("ALL_DONE");
}
