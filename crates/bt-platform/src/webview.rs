//! WebView2 in **composition hosting**: the engine's own visual tree spliced
//! into the window's, with every event the plan names attached before anything
//! navigates.
//!
//! # What this module is and is not
//!
//! It is the unsafe half. Every COM call the web preview makes is here, and
//! nothing here decides anything: which URL to go to, whether a chord belongs to
//! the window, when to rebuild after a crash and where the seat is on screen are
//! all questions `bt_app::webhost` answers. What crosses the boundary is plain
//! data — [`WebEvent`] out, [`WebChord`] and a minted target in — so the state
//! machine that drives all of this can be tested without a browser, which is the
//! whole reason it is a state machine.
//!
//! # One thread, no locks
//!
//! WebView2 delivers every callback on the thread that created the environment,
//! and that is the thread that owns the message pump, the window and the visual
//! tree. So the queue is an `Rc<RefCell<_>>` rather than a channel — the same
//! shape, and for the same reason, as the W0′ probe's evidence table
//! (`spikes/webview2-w0/src/host.rs`).
//!
//! # Nothing here blocks
//!
//! The probe could sit in its own message pump waiting for a creation callback.
//! A window cannot: the pump belongs to winit, and a nested one would run the
//! whole application re-entrantly. So creation is genuinely asynchronous here —
//! the callbacks push an event and ask the event loop for a turn, and the
//! generation token in `bt_app::webhost::WebMachine` is what makes that safe,
//! because a callback cannot be cancelled and will arrive for a pane that has
//! already gone.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::num::NonZeroIsize;
use std::path::Path;
use std::rc::Rc;

use webview2_com::Microsoft::Web::WebView2::Win32::*;
// Named one by one rather than globbed: `webview2_com` exports a `Result` alias
// of its own, and a glob here would quietly make every `Result<(), String>` in
// this file mean something else.
use webview2_com::{
    AcceleratorKeyPressedEventHandler, BrowserProcessExitedEventHandler,
    CreateCoreWebView2CompositionControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, CursorChangedEventHandler,
    DownloadStartingEventHandler, FocusChangedEventHandler, LaunchingExternalUriSchemeEventHandler,
    MoveFocusRequestedEventHandler, NavigationCompletedEventHandler,
    NavigationStartingEventHandler, NewBrowserVersionAvailableEventHandler,
    NewWindowRequestedEventHandler, PermissionRequestedEventHandler, ProcessFailedEventHandler,
    take_pwstr,
};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::core::{BOOL, HSTRING, Interface as _, PCWSTR, PWSTR};

use super::windows_impl::Compositor;

// ── Reading out-parameters ─────────────────────────────────────────────────
//
// Every WebView2 getter is `fn(&self, *mut T) -> Result<()>`. Reading one at
// each call site would bury the fact being read in five lines of ceremony, so
// the ceremony lives here once. A getter that fails yields the type's default,
// which for every field below reads as "the engine did not say".

fn read<T: Default>(getter: impl FnOnce(*mut T) -> windows::core::Result<()>) -> T {
    let mut value = T::default();
    let _ = getter(&mut value);
    value
}

fn read_bool(getter: impl FnOnce(*mut BOOL) -> windows::core::Result<()>) -> bool {
    read::<BOOL>(getter).as_bool()
}

fn read_string(getter: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>) -> String {
    let mut value = PWSTR::null();
    match getter(&mut value) {
        Ok(()) => take_pwstr(value),
        Err(_) => String::new(),
    }
}

fn failure(step: &str, error: &windows::core::Error) -> String {
    format!(
        "{step} failed: {} (0x{:08X})",
        error.message(),
        error.code().0 as u32
    )
}

// ── The plain data that crosses the boundary ───────────────────────────────

/// A chord the window claims from a focused page.
///
/// A Win32 virtual key and three booleans, because that is the vocabulary
/// `AcceleratorKeyPressed` speaks and there is no second one. Translating the
/// product's own table into this is `bt_app::webhost::claimable_chords`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebChord {
    pub virtual_key: u16,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// One key, as the accelerator callback saw it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebKey {
    pub chord: WebChord,
    /// `false` for the key-up half. The callback fires for both — measured, 30
    /// rows out of 30 (`w0p-evidence.md` §2.2, the `kind` column) — so the
    /// window can hold a chord and let go of it.
    pub down: bool,
}

/// Everything the engine says, in the window's own vocabulary.
#[derive(Clone, Debug, PartialEq)]
pub enum WebEvent {
    /// The environment callback for this generation came back.
    Environment {
        generation: u64,
        error: Option<String>,
    },
    /// The composition controller callback for this generation came back.
    Controller {
        generation: u64,
        error: Option<String>,
    },
    NavigationStarting {
        uri: String,
        cancelled: bool,
    },
    NavigationCompleted {
        uri: String,
        success: bool,
        status: i32,
    },
    /// A process under this WebView died. `kind` is
    /// `COREWEBVIEW2_PROCESS_FAILED_KIND`: `0` is the browser process, `1` the
    /// renderer, and the two mean entirely different things to the state
    /// machine.
    ProcessFailed {
        kind: i32,
        description: String,
    },
    /// The browser process is gone and the user data folder is nobody's.
    BrowserProcessExited {
        kind: i32,
    },
    /// Evergreen installed a newer build under a running process.
    NewBrowserVersionAvailable,
    AcceleratorKey {
        key: WebKey,
        /// Whether the host took it. Decided inside the callback, because
        /// `SetHandled` cannot be decided later.
        handled: bool,
    },
    /// Tab walked off the end of the page's own controls.
    MoveFocusRequested {
        /// `COREWEBVIEW2_MOVE_FOCUS_REASON`: 1 is next, 2 is previous.
        reason: i32,
    },
    GotFocus,
    LostFocus,
    /// The page wants a different mouse cursor. The number is a Win32
    /// `IDC_*` — 32512 arrow, 32513 I-beam, 32649 hand.
    CursorChanged {
        system_cursor_id: u32,
    },
}

/// What the caller's navigation policy says about a URI the engine is about to
/// go to.
///
/// Three answers and not two, because the policy is allowed to *change* a
/// target as well as refuse it: §3's loopback rule rewrites `0.0.0.0` to
/// `127.0.0.1` keeping port, path, query and fragment, and `NavigationStarting`
/// has no way to say "go somewhere else" — it can only cancel. So the rewrite is
/// a cancel and a fresh navigation, which is exactly what
/// [`WebNavigationVerdict::CancelAndNavigateTo`] names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebNavigationVerdict {
    /// Let it go where it said it was going.
    Proceed,
    /// Cancel it. The card that explains why belongs to a later slice; this one
    /// stops the navigation and nothing else.
    Cancel,
    /// Cancel it and go here instead.
    CancelAndNavigateTo(String),
}

/// Which mouse event to forward, named the way the caller names it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebMouseEvent {
    Move,
    /// **Refused by the engine, in all three spellings the API allows**
    /// (`w0p-evidence.md` §1 gate 3): `SendMouseInput` answers `LEAVE` with
    /// `E_INVALIDARG` whatever coordinates and button mask it is given. The
    /// variant exists so the caller can name the thing it wants; what actually
    /// makes a page believe the pointer left is a `Move` to a point outside the
    /// bounds, which is the substitute the same gate measured working.
    Leave,
    LeftDown,
    LeftUp,
    LeftDoubleClick,
    RightDown,
    RightUp,
    MiddleDown,
    MiddleUp,
    XDown(u16),
    XUp(u16),
    Wheel(i16),
    HorizontalWheel(i16),
}

impl WebMouseEvent {
    fn kind(self) -> COREWEBVIEW2_MOUSE_EVENT_KIND {
        match self {
            Self::Move => COREWEBVIEW2_MOUSE_EVENT_KIND_MOVE,
            Self::Leave => COREWEBVIEW2_MOUSE_EVENT_KIND_LEAVE,
            Self::LeftDown => COREWEBVIEW2_MOUSE_EVENT_KIND_LEFT_BUTTON_DOWN,
            Self::LeftUp => COREWEBVIEW2_MOUSE_EVENT_KIND_LEFT_BUTTON_UP,
            Self::LeftDoubleClick => COREWEBVIEW2_MOUSE_EVENT_KIND_LEFT_BUTTON_DOUBLE_CLICK,
            Self::RightDown => COREWEBVIEW2_MOUSE_EVENT_KIND_RIGHT_BUTTON_DOWN,
            Self::RightUp => COREWEBVIEW2_MOUSE_EVENT_KIND_RIGHT_BUTTON_UP,
            Self::MiddleDown => COREWEBVIEW2_MOUSE_EVENT_KIND_MIDDLE_BUTTON_DOWN,
            Self::MiddleUp => COREWEBVIEW2_MOUSE_EVENT_KIND_MIDDLE_BUTTON_UP,
            Self::XDown(_) => COREWEBVIEW2_MOUSE_EVENT_KIND_X_BUTTON_DOWN,
            Self::XUp(_) => COREWEBVIEW2_MOUSE_EVENT_KIND_X_BUTTON_UP,
            Self::Wheel(_) => COREWEBVIEW2_MOUSE_EVENT_KIND_WHEEL,
            Self::HorizontalWheel(_) => COREWEBVIEW2_MOUSE_EVENT_KIND_HORIZONTAL_WHEEL,
        }
    }

    /// `mouseData`: a wheel delta, or which X button, or nothing.
    fn data(self) -> u32 {
        match self {
            Self::Wheel(delta) | Self::HorizontalWheel(delta) => delta as i32 as u32,
            Self::XDown(button) | Self::XUp(button) => u32::from(button),
            _ => 0,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Leave => "leave",
            Self::LeftDown => "left-down",
            Self::LeftUp => "left-up",
            Self::LeftDoubleClick => "left-double-click",
            Self::RightDown => "right-down",
            Self::RightUp => "right-up",
            Self::MiddleDown => "middle-down",
            Self::MiddleUp => "middle-up",
            Self::XDown(_) => "x-down",
            Self::XUp(_) => "x-up",
            Self::Wheel(_) => "wheel",
            Self::HorizontalWheel(_) => "horizontal-wheel",
        }
    }
}

/// Which mouse buttons are down while an event is forwarded, in the bitmask
/// `SendMouseInput` takes.
pub mod web_mouse_buttons {
    pub const NONE: u32 = 0;
    pub const LEFT: u32 = 1;
    pub const RIGHT: u32 = 2;
    pub const MIDDLE: u32 = 16;
    pub const X1: u32 = 32;
    pub const X2: u32 = 64;
}

// ── The process-wide environment ───────────────────────────────────────────

thread_local! {
    /// **One environment per process** (`plan.md` §0). Two environments over one
    /// user data folder with different options is `0x8007139F`, and two with the
    /// same options is two browser process trees for no reason.
    static ENVIRONMENT: RefCell<Option<ICoreWebView2Environment>> = const { RefCell::new(None) };
}

/// Drop the cached environment without closing anything.
///
/// **The step a runtime update forces, and the easiest one to leave out.** A new
/// controller made over the *old* environment is a controller on the old browser
/// build, so the update takes effect for nobody. And the caller must already
/// have closed every controller and waited for the browser to go: a new
/// environment made while the old browser still holds the folder does not fail
/// loudly — measured — **it simply never calls back**
/// (`w0p-evidence.md` §3.4).
pub fn forget_web_environment() {
    ENVIRONMENT.with(|cell| *cell.borrow_mut() = None);
}

/// The runtime's version, asked of the loader rather than of the registry.
///
/// The registry lies and the API does not: gate 7 removed the runtime and the
/// `HKLM\WOW6432Node` key went on reporting a version that was no longer
/// installed, while this call failed with `0x80070002` in 0 ms.
pub fn webview2_runtime_version() -> Result<String, String> {
    let mut version = PWSTR::null();
    let answer =
        unsafe { GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut version) };
    // The loader allocated the string with `CoTaskMemAlloc`, and `take_pwstr`
    // is the matching free.
    match answer {
        Ok(()) if !version.is_null() => Ok(take_pwstr(version)),
        Ok(()) => Err(String::from(
            "GetAvailableCoreWebView2BrowserVersionString returned S_OK with no version",
        )),
        Err(error) => Err(failure(
            "GetAvailableCoreWebView2BrowserVersionString",
            &error,
        )),
    }
}

// ── The host ───────────────────────────────────────────────────────────────

/// Everything a callback needs to reach: the queue it pushes onto, the chord
/// table it consults, the navigation gate it asks, and the nudge that gets the
/// event loop to come and read what it wrote.
struct Shared {
    events: RefCell<VecDeque<WebEvent>>,
    chords: RefCell<Vec<WebChord>>,
    /// The caller's navigation policy, asked synchronously inside
    /// `NavigationStarting` because `SetCancel` cannot be decided later.
    ///
    /// A boxed closure and not a function pointer: what the policy needs to know
    /// besides the URI — which target this pane minted for itself — is the
    /// caller's state, and the caller captures it. This crate never sees it.
    gate: Box<dyn Fn(&str) -> WebNavigationVerdict>,
    /// The target of the rewrite currently in flight, if any.
    ///
    /// A cancel-and-renavigate raises `NavigationStarting` again for the new
    /// target, and a policy that rewrote a second time would loop. §3's
    /// normalisation is idempotent, so the second pass answers `Proceed` and
    /// this is only ever the belt: the same target is never rewritten twice in
    /// a row.
    rewriting_to: RefCell<Option<String>>,
    wake: Box<dyn Fn()>,
}

impl Shared {
    fn push(&self, event: WebEvent) {
        self.events.borrow_mut().push_back(event);
        (self.wake)();
    }
}

/// One web preview's engine.
///
/// Owns the controller and, through the process-wide cache, a share of the
/// environment. Everything it does is a step the caller's state machine told it
/// to take.
pub struct WebHost {
    shared: Rc<Shared>,
    controller: Option<ICoreWebView2Controller>,
    composition: Option<ICoreWebView2CompositionController>,
    webview: Option<ICoreWebView2>,
    environment: Option<ICoreWebView2Environment>,
    /// Where the controller callback puts what it was handed, until [`WebHost::install`]
    /// comes for it.
    ///
    /// The callback cannot hand the controller back through the event, because
    /// [`WebEvent`] is plain data by design and a COM interface is not; and it
    /// cannot store it on `self`, because it does not have `self`. So it stores
    /// it here, and the state machine decides — from the generation the event
    /// carried — whether it is wanted.
    pending_controller: Option<Rc<RefCell<Option<ICoreWebView2CompositionController>>>>,
}

impl WebHost {
    /// A host that has not started anything yet.
    ///
    /// `gate` is asked, synchronously inside `NavigationStarting`, what to do
    /// with a URI. `wake` is called after every event is queued and must get the
    /// event loop to call [`WebHost::drain`] — a callback that arrives while the
    /// window is idle would otherwise sit unread until somebody moved the mouse.
    pub fn new(gate: Box<dyn Fn(&str) -> WebNavigationVerdict>, wake: Box<dyn Fn()>) -> Self {
        Self {
            shared: Rc::new(Shared {
                events: RefCell::new(VecDeque::new()),
                chords: RefCell::new(Vec::new()),
                gate,
                rewriting_to: RefCell::new(None),
                wake,
            }),
            controller: None,
            composition: None,
            webview: None,
            environment: None,
            pending_controller: None,
        }
    }

    /// Everything the engine has said since the last time it was asked.
    pub fn drain(&self) -> Vec<WebEvent> {
        self.shared.events.borrow_mut().drain(..).collect()
    }

    /// The chords the window takes back from a focused page.
    ///
    /// Replaced whenever the effective shortcut table or the window's focus
    /// changes, because both change the answer and the callback has no way to
    /// ask.
    pub fn set_claimed_chords(&self, chords: Vec<WebChord>) {
        *self.shared.chords.borrow_mut() = chords;
    }

    pub fn has_controller(&self) -> bool {
        self.controller.is_some()
    }

    /// Ask for the process-wide environment, reporting the answer as a
    /// [`WebEvent::Environment`] for this generation.
    ///
    /// Returns immediately either way: when the environment is already cached
    /// the event is queued on the spot, and when it is not the loader answers
    /// on a later turn of the message pump.
    pub fn request_environment(&mut self, folder: &Path, generation: u64) -> Result<(), String> {
        if let Some(existing) = ENVIRONMENT.with(|cell| cell.borrow().clone()) {
            self.environment = Some(existing);
            self.shared.push(WebEvent::Environment {
                generation,
                error: None,
            });
            return Ok(());
        }
        let shared = Rc::clone(&self.shared);
        let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
            move |result, created| {
                let error = match (result, created) {
                    (Ok(()), Some(environment)) => {
                        ENVIRONMENT.with(|cell| *cell.borrow_mut() = Some(environment));
                        None
                    }
                    (Ok(()), None) => Some(String::from(
                        "the environment callback delivered no environment",
                    )),
                    (Err(error), _) => Some(failure("CreateCoreWebView2Environment", &error)),
                };
                shared.push(WebEvent::Environment { generation, error });
                Ok(())
            },
        ));
        let folder = HSTRING::from(folder.as_os_str());
        unsafe {
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                PCWSTR(folder.as_ptr()),
                None::<&ICoreWebView2EnvironmentOptions>,
                &handler,
            )
        }
        .map_err(|error| failure("CreateCoreWebView2EnvironmentWithOptions", &error))
    }

    /// Adopt the environment the last [`WebEvent::Environment`] reported.
    fn adopt_environment(&mut self) -> Result<ICoreWebView2Environment, String> {
        if let Some(environment) = self.environment.clone() {
            return Ok(environment);
        }
        let environment = ENVIRONMENT
            .with(|cell| cell.borrow().clone())
            .ok_or_else(|| String::from("no CoreWebView2Environment is cached"))?;
        self.environment = Some(environment.clone());
        Ok(environment)
    }

    /// Ask for a composition controller on this window, reporting the answer as
    /// a [`WebEvent::Controller`] for this generation.
    pub fn request_controller(
        &mut self,
        hwnd: NonZeroIsize,
        generation: u64,
    ) -> Result<(), String> {
        let environment = self.adopt_environment()?;
        let environment3: ICoreWebView2Environment3 = environment
            .cast()
            .map_err(|error| failure("ICoreWebView2Environment3", &error))?;
        let shared = Rc::clone(&self.shared);
        let holder = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&holder);
        let handler = CreateCoreWebView2CompositionControllerCompletedHandler::create(Box::new(
            move |result, controller| {
                let error = match (result, controller) {
                    (Ok(()), Some(controller)) => {
                        *sink.borrow_mut() = Some(controller);
                        None
                    }
                    (Ok(()), None) => Some(String::from(
                        "the controller callback delivered no controller",
                    )),
                    (Err(error), _) => {
                        Some(failure("CreateCoreWebView2CompositionController", &error))
                    }
                };
                shared.push(WebEvent::Controller { generation, error });
                Ok(())
            },
        ));
        self.pending_controller = Some(holder);
        let hwnd = HWND(hwnd.get() as *mut c_void);
        unsafe { environment3.CreateCoreWebView2CompositionController(hwnd, &handler) }
            .map_err(|error| failure("CreateCoreWebView2CompositionController", &error))
    }

    /// Take the controller the last [`WebEvent::Controller`] reported, point it
    /// at the window's web visual, configure it and attach **every** event.
    ///
    /// Nothing navigates here, and that is the point: the plan's §4 says events
    /// and policies are all installed before the first navigation, because a
    /// navigation started a moment earlier would run before `NavigationStarting`
    /// existed to check it.
    pub fn install(&mut self, compositor: &Compositor, seat: u64) -> Result<(), String> {
        let pending = self
            .pending_controller
            .take()
            .ok_or_else(|| String::from("no controller callback has been answered"))?;
        let composition: ICoreWebView2CompositionController = pending
            .borrow_mut()
            .take()
            .ok_or_else(|| String::from("the controller callback delivered no controller"))?;
        let controller: ICoreWebView2Controller = composition
            .cast()
            .map_err(|error| failure("ICoreWebView2Controller", &error))?;
        let webview = unsafe { controller.CoreWebView2() }
            .map_err(|error| failure("ICoreWebView2Controller::CoreWebView2", &error))?;
        self.composition = Some(composition);
        self.controller = Some(controller);
        self.webview = Some(webview);
        self.configure()?;
        self.attach_events()?;
        self.attach_environment_events()?;
        let visual = compositor
            .web_visual(seat)
            .ok_or_else(|| String::from("this seat has no web visual to render into"))?;
        unsafe { self.composition().SetRootVisualTarget(&visual) }
            .map_err(|error| failure("SetRootVisualTarget", &error))
    }

    fn composition(&self) -> &ICoreWebView2CompositionController {
        self.composition
            .as_ref()
            .expect("a composition controller, checked by the caller's state machine")
    }

    fn configure(&self) -> Result<(), String> {
        let Some(webview) = self.webview.as_ref() else {
            return Ok(());
        };
        let controller = self
            .controller
            .as_ref()
            .expect("a controller beside the webview");
        unsafe {
            let settings = webview
                .Settings()
                .map_err(|error| failure("ICoreWebView2::Settings", &error))?;
            // Slice ① hosts a page and offers it nothing. The bridge, the status
            // bar and the developer tools are all slice ②'s and slice ④'s to
            // decide about, and a default left on is a decision made by nobody.
            settings
                .SetIsWebMessageEnabled(false)
                .map_err(|error| failure("SetIsWebMessageEnabled", &error))?;
            // **The other half of the bridge** (W2 slice 5, plan section 3's
            // controlled file entry). `IsWebMessageEnabled` closes the page's
            // way *out*; this closes the host's way *in*. Both are off for the
            // same sentence: a local page opened out of the files column is
            // read, and nothing in this product offers it an object, a method
            // or a channel. Neither is conditional on where the page came from,
            // because a switch that is only thrown for `file:` pages is a
            // switch somebody has to remember to throw.
            settings
                .SetAreHostObjectsAllowed(false)
                .map_err(|error| failure("SetAreHostObjectsAllowed", &error))?;
            settings
                .SetIsStatusBarEnabled(false)
                .map_err(|error| failure("SetIsStatusBarEnabled", &error))?;
            settings
                .SetAreDevToolsEnabled(false)
                .map_err(|error| failure("SetAreDevToolsEnabled", &error))?;
            settings
                .SetAreDefaultContextMenusEnabled(false)
                .map_err(|error| failure("SetAreDefaultContextMenusEnabled", &error))?;
            let controller3: ICoreWebView2Controller3 = controller
                .cast()
                .map_err(|error| failure("ICoreWebView2Controller3", &error))?;
            // Physical pixels in, physical pixels out. `bt_layout` already works
            // in device pixels, and a scale factor applied here would apply it
            // twice.
            controller3
                .SetBoundsMode(COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS)
                .map_err(|error| failure("SetBoundsMode", &error))?;
            controller3
                .SetShouldDetectMonitorScaleChanges(true)
                .map_err(|error| failure("SetShouldDetectMonitorScaleChanges", &error))?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn attach_events(&self) -> Result<(), String> {
        let webview = self
            .webview
            .as_ref()
            .expect("a webview, taken a few lines above");
        let controller = self
            .controller
            .as_ref()
            .expect("a controller, taken a few lines above");
        let composition = self.composition();
        let mut token = 0i64;
        unsafe {
            // ── navigation ────────────────────────────────────────────────
            let shared = Rc::clone(&self.shared);
            webview
                .add_NavigationStarting(
                    &NavigationStartingEventHandler::create(Box::new(move |view, args| {
                        let Some(args) = args else { return Ok(()) };
                        let uri = read_string(|out| args.Uri(out));
                        // The rewrite already in flight arrives here as an
                        // ordinary candidate. It is not offered to the policy a
                        // second time: normalisation is idempotent, so a second
                        // answer could only be the same one, and asking anyway
                        // is what a loop looks like from the inside.
                        let in_flight =
                            shared.rewriting_to.borrow().as_deref() == Some(uri.as_str());
                        let verdict = if in_flight {
                            *shared.rewriting_to.borrow_mut() = None;
                            WebNavigationVerdict::Proceed
                        } else {
                            (shared.gate)(&uri)
                        };
                        let cancelled = match &verdict {
                            WebNavigationVerdict::Proceed => false,
                            WebNavigationVerdict::Cancel => {
                                args.SetCancel(true)?;
                                *shared.rewriting_to.borrow_mut() = None;
                                true
                            }
                            WebNavigationVerdict::CancelAndNavigateTo(target) => {
                                args.SetCancel(true)?;
                                *shared.rewriting_to.borrow_mut() = Some(target.clone());
                                if let Some(view) = view.as_ref() {
                                    view.Navigate(&HSTRING::from(target.as_str()))?;
                                }
                                true
                            }
                        };
                        shared.push(WebEvent::NavigationStarting { uri, cancelled });
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_NavigationStarting", &error))?;

            let shared = Rc::clone(&self.shared);
            webview
                .add_NavigationCompleted(
                    &NavigationCompletedEventHandler::create(Box::new(move |view, args| {
                        let Some(args) = args else { return Ok(()) };
                        let success = read_bool(|out| args.IsSuccess(out));
                        let status =
                            read::<COREWEBVIEW2_WEB_ERROR_STATUS>(|out| args.WebErrorStatus(out)).0;
                        let uri = view
                            .map(|view| read_string(|out| view.Source(out)))
                            .unwrap_or_default();
                        shared.push(WebEvent::NavigationCompleted {
                            uri,
                            success,
                            status,
                        });
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_NavigationCompleted", &error))?;

            // ── the doors slice ② will widen, shut for now ────────────────
            //
            // A window opened by a page, a download, a permission and an
            // external scheme are four separate rulings the plan has already
            // made and slice ② implements. What slice ① owes them is that none
            // of them can happen behind its back before it does — so each is
            // attached and each refuses.
            webview
                .add_NewWindowRequested(
                    &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        args.SetHandled(true)?;
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_NewWindowRequested", &error))?;

            let downloads: ICoreWebView2_4 = webview
                .cast()
                .map_err(|error| failure("ICoreWebView2_4", &error))?;
            downloads
                .add_DownloadStarting(
                    &DownloadStartingEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        args.SetCancel(true)?;
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_DownloadStarting", &error))?;

            webview
                .add_PermissionRequested(
                    &PermissionRequestedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_PermissionRequested", &error))?;

            let external: ICoreWebView2_18 = webview
                .cast()
                .map_err(|error| failure("ICoreWebView2_18", &error))?;
            external
                .add_LaunchingExternalUriScheme(
                    &LaunchingExternalUriSchemeEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        args.SetCancel(true)?;
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_LaunchingExternalUriScheme", &error))?;

            // ── process lifetime ──────────────────────────────────────────
            let shared = Rc::clone(&self.shared);
            webview
                .add_ProcessFailed(
                    &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let kind = read::<COREWEBVIEW2_PROCESS_FAILED_KIND>(|out| {
                            args.ProcessFailedKind(out)
                        })
                        .0;
                        let description = match args.cast::<ICoreWebView2ProcessFailedEventArgs2>()
                        {
                            Ok(args2) => read_string(|out| args2.ProcessDescription(out)),
                            Err(_) => String::new(),
                        };
                        shared.push(WebEvent::ProcessFailed { kind, description });
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_ProcessFailed", &error))?;

            // ── keyboard and focus ────────────────────────────────────────
            let shared = Rc::clone(&self.shared);
            controller
                .add_AcceleratorKeyPressed(
                    &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let virtual_key = read::<u32>(|out| args.VirtualKey(out)) as u16;
                        let kind =
                            read::<COREWEBVIEW2_KEY_EVENT_KIND>(|out| args.KeyEventKind(out)).0;
                        let (ctrl, shift, alt) = modifiers_down();
                        let chord = WebChord {
                            virtual_key,
                            ctrl,
                            shift,
                            alt,
                        };
                        // The whole reason this callback matters: it runs on the
                        // window's thread *before* the page sees the key, so a
                        // chord this window owns can be taken back here — and
                        // only here, synchronously.
                        let handled = shared.chords.borrow().contains(&chord);
                        if handled {
                            args.SetHandled(true)?;
                        }
                        // 0 = KEY_DOWN, 2 = SYSTEM_KEY_DOWN.
                        let down = kind == 0 || kind == 2;
                        shared.push(WebEvent::AcceleratorKey {
                            key: WebKey { chord, down },
                            handled,
                        });
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_AcceleratorKeyPressed", &error))?;

            let shared = Rc::clone(&self.shared);
            controller
                .add_MoveFocusRequested(
                    &MoveFocusRequestedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let reason =
                            read::<COREWEBVIEW2_MOVE_FOCUS_REASON>(|out| args.Reason(out)).0;
                        // The Tab contract: the page walked off its own last
                        // control, and the window takes the keyboard back.
                        args.SetHandled(true)?;
                        shared.push(WebEvent::MoveFocusRequested { reason });
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_MoveFocusRequested", &error))?;

            let shared = Rc::clone(&self.shared);
            controller
                .add_GotFocus(
                    &FocusChangedEventHandler::create(Box::new(move |_, _| {
                        shared.push(WebEvent::GotFocus);
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_GotFocus", &error))?;

            let shared = Rc::clone(&self.shared);
            controller
                .add_LostFocus(
                    &FocusChangedEventHandler::create(Box::new(move |_, _| {
                        shared.push(WebEvent::LostFocus);
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_LostFocus", &error))?;

            // ── cursor ────────────────────────────────────────────────────
            let shared = Rc::clone(&self.shared);
            composition
                .add_CursorChanged(
                    &CursorChangedEventHandler::create(Box::new(move |controller, _| {
                        if let Some(controller) = controller {
                            let mut id = 0u32;
                            let _ = controller.SystemCursorId(&mut id);
                            shared.push(WebEvent::CursorChanged {
                                system_cursor_id: id,
                            });
                        }
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_CursorChanged", &error))?;
        }
        Ok(())
    }

    /// The two events that belong to the environment rather than to any one
    /// controller: the browser going away, and a newer one arriving.
    fn attach_environment_events(&self) -> Result<(), String> {
        let environment = self
            .environment
            .as_ref()
            .ok_or_else(|| String::from("no environment to attach to"))?;
        let mut token = 0i64;
        unsafe {
            let environment5: ICoreWebView2Environment5 = environment
                .cast()
                .map_err(|error| failure("ICoreWebView2Environment5", &error))?;
            let shared = Rc::clone(&self.shared);
            environment5
                .add_BrowserProcessExited(
                    &BrowserProcessExitedEventHandler::create(Box::new(move |_, args| {
                        let kind = args
                            .map(|args| {
                                read::<COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND>(|out| {
                                    args.BrowserProcessExitKind(out)
                                })
                                .0
                            })
                            .unwrap_or(-1);
                        shared.push(WebEvent::BrowserProcessExited { kind });
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_BrowserProcessExited", &error))?;

            let shared = Rc::clone(&self.shared);
            environment
                .add_NewBrowserVersionAvailable(
                    &NewBrowserVersionAvailableEventHandler::create(Box::new(move |_, _| {
                        shared.push(WebEvent::NewBrowserVersionAvailable);
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_NewBrowserVersionAvailable", &error))?;
        }
        Ok(())
    }

    /// The size of the seat, in physical pixels.
    ///
    /// **Size and not rectangle.** The engine believes its own bounds start at
    /// `(0, 0)` and the *visual* carries the placement — the arrangement every
    /// visual-hosting sample uses, and the one gate 3 measured coordinates
    /// through: a client point `(511, 242)` less the seat origin `(224, 48)`,
    /// divided by the 2.0 device pixel ratio, arrived at the page as `(143, 97)`.
    pub fn set_size(&self, width: u32, height: u32) -> Result<(), String> {
        let Some(controller) = self.controller.as_ref() else {
            return Ok(());
        };
        let bounds = RECT {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };
        unsafe { controller.SetBounds(bounds) }
            .map_err(|error| failure("ICoreWebView2Controller::SetBounds", &error))
    }

    /// Show or hide the page.
    ///
    /// Hiding is not decoration: a hidden WebView stops its timers and its
    /// `requestAnimationFrame` entirely — 1 811 ms of CPU and 718 frames over
    /// six seconds visible, **0 and 0** hidden (`w0p-evidence.md` §1 gate 8) —
    /// which is the whole of how a page on a tab nobody is looking at costs
    /// nothing.
    pub fn set_visible(&self, visible: bool) -> Result<(), String> {
        let Some(controller) = self.controller.as_ref() else {
            return Ok(());
        };
        unsafe { controller.SetIsVisible(visible) }
            .map_err(|error| failure("ICoreWebView2Controller::SetIsVisible", &error))
    }

    pub fn navigate(&self, url: &str) -> Result<(), String> {
        let Some(webview) = self.webview.as_ref() else {
            return Ok(());
        };
        unsafe { webview.Navigate(&HSTRING::from(url)) }
            .map_err(|error| failure("ICoreWebView2::Navigate", &error))
    }

    pub fn reload(&self) -> Result<(), String> {
        let Some(webview) = self.webview.as_ref() else {
            return Ok(());
        };
        unsafe { webview.Reload() }.map_err(|error| failure("ICoreWebView2::Reload", &error))
    }

    /// Put the keyboard inside the page.
    pub fn focus_page(&self) -> Result<(), String> {
        let Some(controller) = self.controller.as_ref() else {
            return Ok(());
        };
        unsafe { controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) }
            .map_err(|error| failure("ICoreWebView2Controller::MoveFocus", &error))
    }

    pub fn browser_process_id(&self) -> u32 {
        let Some(webview) = self.webview.as_ref() else {
            return 0;
        };
        read::<u32>(|out| unsafe { webview.BrowserProcessId(out) })
    }

    /// Forward one mouse event. `point` is **seat-local** physical pixels — the
    /// caller subtracts the seat's origin, because the caller is the only one
    /// that knows where the seat is this frame.
    pub fn send_mouse(
        &self,
        event: WebMouseEvent,
        point: (i32, i32),
        buttons_down: u32,
    ) -> Result<(), String> {
        let Some(composition) = self.composition.as_ref() else {
            return Ok(());
        };
        unsafe {
            composition.SendMouseInput(
                event.kind(),
                COREWEBVIEW2_MOUSE_EVENT_VIRTUAL_KEYS(buttons_down as i32),
                event.data(),
                POINT {
                    x: point.0,
                    y: point.1,
                },
            )
        }
        .map_err(|error| failure(&format!("SendMouseInput({})", event.name()), &error))
    }

    /// Close a controller that arrived for a generation nobody wants any more.
    ///
    /// **Closed and not simply dropped.** The controller is real and running:
    /// letting the last reference go leaves a browser process tree with nobody
    /// pointing at it, which is the leak the generation token exists to prevent
    /// rather than to cause.
    pub fn close_pending_controller(&mut self) {
        let Some(pending) = self.pending_controller.take() else {
            return;
        };
        let Some(orphan) = pending.borrow_mut().take() else {
            return;
        };
        if let Ok(controller) = orphan.cast::<ICoreWebView2Controller>() {
            let _ = unsafe { controller.Close() };
        }
    }

    /// Close the controller. The browser process goes on living until it says
    /// otherwise — which is what the caller's state machine is waiting for.
    pub fn close(&mut self) {
        if let Some(controller) = self.controller.take() {
            let _ = unsafe { controller.Close() };
        }
        self.composition = None;
        self.webview = None;
    }
}

/// Which modifiers are physically down right now.
///
/// `AcceleratorKeyPressed` hands over the key but not the modifier state, so
/// the host has to read it — and reads it here, once, rather than at the two
/// places that would eventually disagree.
fn modifiers_down() -> (bool, bool, bool) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
    let down = |vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| {
        (unsafe { GetKeyState(i32::from(vk.0)) } as u16 & 0x8000) != 0
    };
    (down(VK_CONTROL), down(VK_SHIFT), down(VK_MENU))
}
