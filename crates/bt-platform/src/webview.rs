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
    CapturePreviewCompletedHandler, CreateCoreWebView2CompositionControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, CursorChangedEventHandler,
    DocumentTitleChangedEventHandler, DownloadStartingEventHandler, FaviconChangedEventHandler,
    FindActiveMatchIndexChangedEventHandler, FindMatchCountChangedEventHandler,
    FindStartCompletedHandler, FocusChangedEventHandler, GetFaviconCompletedHandler,
    HistoryChangedEventHandler, LaunchingExternalUriSchemeEventHandler,
    MoveFocusRequestedEventHandler, NavigationCompletedEventHandler,
    NavigationStartingEventHandler, NewBrowserVersionAvailableEventHandler,
    NewWindowRequestedEventHandler, PermissionRequestedEventHandler, ProcessFailedEventHandler,
    SourceChangedEventHandler, StatusBarTextChangedEventHandler, take_pwstr,
};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::core::{BOOL, HSTRING, IUnknown, Interface as _, PCWSTR, PWSTR};

use super::PageVisual;
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
    /// The navigation stack moved: a page was pushed onto it, popped off it, or
    /// replaced through the history API.
    ///
    /// **The two booleans ride on the event and are never polled** (slice ④; the
    /// W1 report names this: "`CanGoBack`/`CanGoForward` 要以引擎为准,不要照抄
    /// 小样的 420ms 假节拍"). They are read inside `HistoryChanged`, which is the
    /// only moment the engine promises they are settled — a getter called on the
    /// window's own clock would be sampling a value that changes on somebody
    /// else's.
    HistoryChanged {
        can_go_back: bool,
        can_go_forward: bool,
    },
    /// The document said what it is called. This is what the head's name cell
    /// shows, and it arrives separately from the URL because a page can rename
    /// itself without navigating.
    DocumentTitleChanged {
        title: String,
    },
    /// The committed URL changed — a navigation, a redirect, or a `pushState`.
    ///
    /// Distinct from [`Self::NavigationCompleted`] on purpose: the history API
    /// changes the address without completing a navigation, and an address field
    /// that only followed completions would sit on a stale URL for the whole of
    /// a single-page application.
    SourceChanged {
        uri: String,
    },
    /// What a browser would put in its status bubble — the target of whatever
    /// the pointer is over, or empty when it is over nothing.
    ///
    /// The engine's own bar is switched off (`SetIsStatusBarEnabled(false)`);
    /// this is the text it would have drawn, handed over so the preview's foot
    /// can be the one band that says both things (§7.7 ③).
    StatusBarTextChanged {
        text: String,
    },
    /// A download started and was cancelled. `uri` is where it was coming from
    /// and `file_name` is what it would have been called.
    ///
    /// Cancelled in the callback and not by the caller, because
    /// `ICoreWebView2DownloadStartingEventArgs::SetCancel` cannot be decided
    /// later — the same shape as `AcceleratorKeyPressed`'s `SetHandled`. What
    /// the caller decides is what happens *instead*, which is a hand-off or a
    /// card (§7.7 ④).
    DownloadStarting {
        uri: String,
        file_name: String,
    },
    /// The find session's tally: how many matches the page holds, and which one
    /// is current (1-based; `0` while there is no current one).
    FindMatches {
        count: i32,
        active: i32,
    },
    /// **A `CapturePreview` finished** — the encoded PNG of the page's viewport,
    /// or `None` if the engine refused or the stream could not be read
    /// (W2 slice ⑥).
    ///
    /// It arrives as an event rather than as a return value for the reason every
    /// other line of this file does: the answer comes back tens of milliseconds
    /// later on the engine's own clock, and the window has a frame to finish.
    /// The `Option` is the whole of the failure vocabulary, because a picture
    /// that did not arrive has exactly one consequence for the caller — the seat
    /// still shows the last one it had.
    Captured {
        png: Option<Vec<u8>>,
    },
    /// **The page now wears a different icon** — `uri` is where that icon lives,
    /// or empty when the page has none (the favicon slice, `docs/DESIGN.md` §7.13, §7.7 ②).
    ///
    /// The address and not the bytes, because `FaviconChanged` carries neither:
    /// the engine says *that* it changed and the picture is a second, asynchronous
    /// ask ([`WebHost::get_favicon`]). Handing the caller the empty string rather
    /// than swallowing it is the whole of the "site with no icon" case — a page
    /// that navigates from one that had an icon to one that has not fires this
    /// with nothing in it, and a caller that never heard would leave the previous
    /// site's drawing standing.
    FaviconChanged {
        uri: String,
    },
    /// **A `GetFavicon` finished** — the icon as PNG, or `None` if the engine
    /// refused or the stream could not be read.
    ///
    /// The same shape and the same `Option` as [`Self::Captured`], for the same
    /// reason: the answer arrives tens of milliseconds later on the engine's own
    /// clock, and a picture that did not arrive has exactly one consequence,
    /// which is that the seat goes on wearing what it already wore.
    Favicon {
        png: Option<Vec<u8>>,
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

// ── Rehosting: one live page, from one window to another ───────────────────

/// One step of the parent-window handoff.
///
/// The nine are the contract `plan.md`'s v3 增补 F1a fixes, spelled out so the
/// order can be held by a test and so a failure can say *where* rather than
/// only *that* — which is the difference between compensating and guessing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RehostStep {
    /// `SetIsVisible(false)`. A page that stayed on the glass through the
    /// handoff would be composed, for at least one frame, into a visual whose
    /// window it has already left.
    Hide,
    /// `SetRootVisualTarget(nullptr)` — the engine lets go of the source
    /// window's visual **before** it is told about another window.
    ClearRootVisualTarget,
    /// The source `IDCompositionDevice::Commit` that publishes the release.
    CommitSource,
    /// `put_ParentWindow(new_hwnd)`.
    ParentWindow,
    /// `SetRootVisualTarget(target seat's visual)`.
    SetRootVisualTarget,
    /// The target `IDCompositionDevice::Commit` that publishes the attachment.
    CommitTarget,
    /// `SetBounds` for the seat's size in the window it has arrived in.
    Bounds,
    /// `SetIsVisible` for what the target window wants shown.
    Presence,
    /// `NotifyParentWindowPositionChanged` — the engine's own popups, tooltips
    /// and IME candidate window are placed off the parent's screen position,
    /// and nothing else tells it that position changed.
    NotifyPosition,
}

/// The handoff, in order. [`WebHost::rehost`] walks exactly this.
pub const REHOST_SEQUENCE: [RehostStep; 9] = [
    RehostStep::Hide,
    RehostStep::ClearRootVisualTarget,
    RehostStep::CommitSource,
    RehostStep::ParentWindow,
    RehostStep::SetRootVisualTarget,
    RehostStep::CommitTarget,
    RehostStep::Bounds,
    RehostStep::Presence,
    RehostStep::NotifyPosition,
];

/// What a half-finished handoff has to put back.
///
/// Four booleans and not a list of steps: the undo is not the sequence run
/// backwards — `CommitSource` and `CommitTarget` publish rather than change, and
/// putting a visual target back is one call whether it was cleared once or set
/// twice. What has to be restored is *state*, and there are exactly four pieces
/// of it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RehostCompensation {
    /// The controller's parent HWND has moved and must go back.
    pub parent_window: bool,
    /// The controller's root visual target is not the source seat's any more.
    pub root_visual_target: bool,
    /// The controller's bounds are the target window's and must go back.
    pub bounds: bool,
    /// The controller's visibility has been touched.
    pub presence: bool,
}

impl RehostCompensation {
    /// Nothing was changed, so nothing has to be put back — the source window
    /// still holds a page that never noticed the attempt.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self == Self::default()
    }

    fn absorb(&mut self, other: Self) {
        self.parent_window |= other.parent_window;
        self.root_visual_target |= other.root_visual_target;
        self.bounds |= other.bounds;
        self.presence |= other.presence;
    }
}

impl RehostStep {
    /// What running this step changes, and therefore what a **later** step's
    /// failure owes the source window.
    fn changes(self) -> RehostCompensation {
        let mut changed = RehostCompensation::default();
        match self {
            Self::Hide | Self::Presence => changed.presence = true,
            Self::ClearRootVisualTarget | Self::SetRootVisualTarget => {
                changed.root_visual_target = true;
            }
            Self::ParentWindow => changed.parent_window = true,
            Self::Bounds => changed.bounds = true,
            // A commit publishes what the calls around it changed; it owns no
            // state of its own, and the compensation's own commits undo it.
            Self::CommitSource | Self::CommitTarget | Self::NotifyPosition => {}
        }
        changed
    }
}

/// What has to be put back when the handoff fails **at** `failed_at`.
///
/// Derived by folding [`RehostStep::changes`] over the steps that already ran,
/// rather than written out as a table: a table is a second copy of the sequence
/// and would be the thing that goes stale when a step moves.
#[must_use]
pub fn rehost_compensation(failed_at: RehostStep) -> RehostCompensation {
    let mut owed = RehostCompensation::default();
    for step in REHOST_SEQUENCE {
        if step == failed_at {
            break;
        }
        owed.absorb(step.changes());
    }
    owed
}

/// How one attempt to move a live page to another window ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RehostOutcome {
    /// The page is on the target window, with everything it was holding.
    Moved,
    /// The handoff failed and the source window still has the page: either
    /// nothing had changed yet, or the compensation put back what had.
    ///
    /// The caller's model must **not** move — this is the branch that makes a
    /// failed tear-out a no-op rather than a lost page.
    KeptSource {
        failed_at: RehostStep,
        error: String,
        /// What the compensation actually had to undo. Empty means the failure
        /// came before anything was touched.
        compensation: RehostCompensation,
    },
    /// The handoff failed **and so did the compensation**. The controller has
    /// been closed, because a controller whose parent, target and bounds are in
    /// an unknown mixture of two windows is not a page anybody can be shown.
    ///
    /// The caller rebuilds from the last good URL — in the **target** window,
    /// which is where the person put it — and the page's in-document state is
    /// gone. This is the lossy branch, and it says so rather than claiming the
    /// source was left as it was.
    Lost {
        failed_at: RehostStep,
        error: String,
        compensation_error: String,
    },
}

/// One end of a rehost: which window, which page, and the compositor that owns
/// that window's visual tree.
pub struct RehostSide<'a> {
    pub compositor: &'a Compositor,
    pub page: PageVisual,
    pub hwnd: NonZeroIsize,
}

/// Everything the undo needs, read **before** the handoff touches anything.
///
/// Read and not recomputed: the bounds and the visibility being put back are the
/// ones the page actually had, and asking the controller for them after the walk
/// has started would be asking a half-moved object about a state it is no longer
/// in.
struct Restore<'a> {
    source: &'a Compositor,
    target: &'a Compositor,
    /// The window the page came from.
    hwnd: HWND,
    /// The source seat's visual, which is what the page goes back to rendering
    /// into.
    visual: IUnknown,
    bounds: RECT,
    visible: bool,
}

/// What the engine says about who owns this page's device scale.
///
/// Read rather than assumed, because the answer decides whose job the DPI is:
/// with `detects_monitor_scale_changes` on, the engine watches the monitor its
/// parent window is on and sets its own rasterization scale, and a host that
/// also wrote one would be the second writer of a single value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WebDpiOwnership {
    /// `ICoreWebView2Controller3::ShouldDetectMonitorScaleChanges`.
    pub detects_monitor_scale_changes: bool,
    /// `ICoreWebView2Controller3::RasterizationScale`, as the engine has it now.
    pub rasterization_scale: f64,
    /// Whether `BoundsMode` is `USE_RAW_PIXELS` — i.e. whether the bounds this
    /// host sets are physical pixels, which is what makes the scale a question
    /// about rastering and not about layout.
    pub bounds_mode_is_raw_pixels: bool,
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
    /// Whether the find session's two counter events have been attached.
    ///
    /// `ICoreWebView2::Find` hands back the same session object every time, so
    /// subscribing on each call would stack a fresh handler per keystroke and
    /// report one count several times over. A `Cell` and not a plain `bool`
    /// because the attaching happens behind `&self`, as every other verb here
    /// does.
    find_attached: std::cell::Cell<bool>,
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
            find_attached: std::cell::Cell::new(false),
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
    pub fn install(&mut self, compositor: &Compositor, page: PageVisual) -> Result<(), String> {
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
            .web_visual(page)
            .ok_or_else(|| String::from("this page has no web visual to render into"))?;
        unsafe { self.composition().SetRootVisualTarget(&visual) }
            .map_err(|error| failure("SetRootVisualTarget", &error))
    }

    /// **Move this live page to another window** — the whole of F1a.
    ///
    /// Nothing navigates, nothing reloads and nothing is rebuilt: the same
    /// controller, the same browser process and the same document come out the
    /// other side, which is the difference between a page that was moved and a
    /// page that was opened again at the same address.
    ///
    /// # Prepare, then a compensable platform handoff, then the caller's commit
    ///
    /// Everything that can be discovered without touching the controller is
    /// discovered first — the target seat's visual, the source seat's visual for
    /// the undo, and the bounds and visibility to put back. Only then does the
    /// walk over [`REHOST_SEQUENCE`] begin, and every step of it has a written
    /// compensation ([`rehost_compensation`]). A failure that the compensation
    /// undoes is [`RehostOutcome::KeptSource`] and the caller's model must not
    /// move; a failure the compensation cannot undo closes the controller and is
    /// [`RehostOutcome::Lost`], which says the page has to be rebuilt rather
    /// than pretending the source window still has it.
    ///
    /// # One environment, asserted rather than assumed
    ///
    /// A controller can only be reparented inside the environment that made it,
    /// and this process has exactly one — `ENVIRONMENT` is a process-wide cache
    /// and `bt_app::webhost` never asks for a second. The check is here because
    /// "there is only one" is a fact about the whole program that this function
    /// depends on and cannot see: if it ever stops being true, this refuses
    /// before it touches anything rather than reparenting across environments.
    ///
    /// # The caller checks `has_controller` first
    ///
    /// A seat whose controller has not arrived yet has nothing to hand over and
    /// still has to follow its tab. That is the caller's own address move, not a
    /// handoff, and asking for one here answers `KeptSource`.
    pub fn rehost(
        &mut self,
        from: &RehostSide<'_>,
        to: &RehostSide<'_>,
        rect: (i32, i32, u32, u32),
        visible: bool,
    ) -> RehostOutcome {
        let refuse = |error: String| RehostOutcome::KeptSource {
            failed_at: RehostStep::Hide,
            error,
            compensation: RehostCompensation::default(),
        };
        let (Some(controller), Some(composition)) =
            (self.controller.clone(), self.composition.clone())
        else {
            return refuse(String::from(
                "this host has no controller to move; the caller moves its own address instead",
            ));
        };
        if !self.holds_the_process_environment() {
            return refuse(String::from(
                "this controller was not made by the process's one environment, and a controller cannot cross environments",
            ));
        }
        let Some(target_visual) = to.compositor.web_visual(to.page) else {
            return refuse(format!(
                "the target window has no web visual for tab {} seat {}",
                to.page.tab, to.page.seat
            ));
        };
        let Some(source_visual) = from.compositor.web_visual(from.page) else {
            return refuse(format!(
                "the source window has no web visual for tab {} seat {}, so a failed handoff could not be put back",
                from.page.tab, from.page.seat
            ));
        };
        let restore = Restore {
            source: from.compositor,
            target: to.compositor,
            hwnd: HWND(from.hwnd.get() as *mut c_void),
            visual: source_visual,
            bounds: read::<RECT>(|out| unsafe { controller.Bounds(out) }),
            visible: read_bool(|out| unsafe { controller.IsVisible(out) }),
        };
        let bounds = RECT {
            left: rect.0,
            top: rect.1,
            right: rect.0 + rect.2 as i32,
            bottom: rect.1 + rect.3 as i32,
        };
        let mut failure_at = None;
        for step in REHOST_SEQUENCE {
            let done = match step {
                RehostStep::Hide => unsafe { controller.SetIsVisible(false) }
                    .map_err(|error| failure("SetIsVisible(false)", &error)),
                RehostStep::ClearRootVisualTarget => {
                    unsafe { composition.SetRootVisualTarget(None::<&IUnknown>) }
                        .map_err(|error| failure("SetRootVisualTarget(nullptr)", &error))
                }
                RehostStep::CommitSource => from.compositor.commit(),
                RehostStep::ParentWindow => {
                    unsafe { controller.SetParentWindow(HWND(to.hwnd.get() as *mut c_void)) }
                        .map_err(|error| failure("put_ParentWindow", &error))
                }
                RehostStep::SetRootVisualTarget => {
                    unsafe { composition.SetRootVisualTarget(&target_visual) }
                        .map_err(|error| failure("SetRootVisualTarget(target)", &error))
                }
                RehostStep::CommitTarget => to.compositor.commit(),
                RehostStep::Bounds => unsafe { controller.SetBounds(bounds) }
                    .map_err(|error| failure("SetBounds", &error)),
                RehostStep::Presence => unsafe { controller.SetIsVisible(visible) }
                    .map_err(|error| failure("SetIsVisible", &error)),
                RehostStep::NotifyPosition => {
                    unsafe { controller.NotifyParentWindowPositionChanged() }
                        .map_err(|error| failure("NotifyParentWindowPositionChanged", &error))
                }
            };
            if let Err(error) = done {
                failure_at = Some((step, error));
                break;
            }
        }
        let Some((failed_at, error)) = failure_at else {
            return RehostOutcome::Moved;
        };
        let compensation = rehost_compensation(failed_at);
        match self.compensate(compensation, &restore) {
            Ok(()) => RehostOutcome::KeptSource {
                failed_at,
                error,
                compensation,
            },
            Err(compensation_error) => {
                // Half in one window and half in another is not a page: closing
                // the controller is what makes the caller's rebuild a rebuild
                // rather than a second thing pointing at the same browser.
                self.close();
                RehostOutcome::Lost {
                    failed_at,
                    error,
                    compensation_error,
                }
            }
        }
    }

    /// Put back exactly what [`rehost_compensation`] says was taken.
    ///
    /// Not the sequence run backwards: the target's visual target is dropped and
    /// published *first*, so that the moment the parent goes back there is
    /// nothing of this page hanging in the window it is leaving.
    fn compensate(&self, owed: RehostCompensation, restore: &Restore<'_>) -> Result<(), String> {
        let controller = self
            .controller
            .as_ref()
            .expect("a controller, taken before the walk began");
        let composition = self
            .composition
            .as_ref()
            .expect("a composition controller, taken before the walk began");
        if owed.root_visual_target {
            unsafe { composition.SetRootVisualTarget(None::<&IUnknown>) }
                .map_err(|error| failure("compensate SetRootVisualTarget(nullptr)", &error))?;
            restore.target.commit()?;
        }
        if owed.parent_window {
            unsafe { controller.SetParentWindow(restore.hwnd) }
                .map_err(|error| failure("compensate put_ParentWindow", &error))?;
        }
        if owed.root_visual_target {
            unsafe { composition.SetRootVisualTarget(&restore.visual) }
                .map_err(|error| failure("compensate SetRootVisualTarget(source)", &error))?;
            restore.source.commit()?;
        }
        if owed.bounds {
            unsafe { controller.SetBounds(restore.bounds) }
                .map_err(|error| failure("compensate SetBounds", &error))?;
        }
        if owed.presence {
            unsafe { controller.SetIsVisible(restore.visible) }
                .map_err(|error| failure("compensate SetIsVisible", &error))?;
        }
        // Best effort, and last: the parent is back where it was, so the engine's
        // own popups are told so. A refusal here cannot make the page any less
        // usable than it already is, and turning it into a `Lost` would close a
        // controller that is whole.
        let _ = unsafe { controller.NotifyParentWindowPositionChanged() };
        Ok(())
    }

    /// Whether this host's environment is the one the process caches.
    fn holds_the_process_environment(&self) -> bool {
        let Some(mine) = self.environment.as_ref() else {
            return false;
        };
        ENVIRONMENT.with(|cell| {
            cell.borrow()
                .as_ref()
                .is_some_and(|process| process.as_raw() == mine.as_raw())
        })
    }

    /// Who owns this page's device scale, read off the engine rather than
    /// assumed. `None` while there is no controller.
    #[must_use]
    pub fn dpi_ownership(&self) -> Option<WebDpiOwnership> {
        let controller3: ICoreWebView2Controller3 = self.controller.as_ref()?.cast().ok()?;
        Some(WebDpiOwnership {
            detects_monitor_scale_changes: read_bool(|out| unsafe {
                controller3.ShouldDetectMonitorScaleChanges(out)
            }),
            rasterization_scale: read::<f64>(|out| unsafe { controller3.RasterizationScale(out) }),
            bounds_mode_is_raw_pixels: read::<COREWEBVIEW2_BOUNDS_MODE>(|out| unsafe {
                controller3.BoundsMode(out)
            }) == COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS,
        })
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
            // **On since slice ④**, which is what a verb for it means: the head
            // carries a `Developer tools` tool and `F12` is a row of the
            // shortcut table, and neither can do anything against a controller
            // that has the tools switched off. The window keeps the key —
            // `webhost::claimable_chords` claims `F12` while a page has the
            // focus — so the engine's own accelerator never reaches the page and
            // there is still one door.
            settings
                .SetAreDevToolsEnabled(true)
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
            // **Off, because this host says the scale itself and one number may
            // have one author.** A composition-hosted controller has no window
            // of its own; what it can watch is the parent this host gives it,
            // and watching it is late — measured before the host took the job,
            // a window carried onto a 1.5 display still had its page at
            // `devicePixelRatio` 2 when it arrived and reached 1.5 somewhere
            // after that, on a later disturbance rather than on the change. See
            // [`WebHost::set_rasterization_scale`].
            controller3
                .SetShouldDetectMonitorScaleChanges(false)
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

            // ── what the head shows (slice ④) ─────────────────────────────
            //
            // Three facts, three events, and not one of them polled. The head is
            // rebuilt every frame from what the seat last heard, so a getter
            // called on the window's clock would be asking the engine a question
            // it has already answered — and, for the two history flags, asking
            // it at a moment when the answer is explicitly not settled.
            let shared = Rc::clone(&self.shared);
            webview
                .add_HistoryChanged(
                    &HistoryChangedEventHandler::create(Box::new(move |view, _| {
                        if let Some(view) = view.as_ref() {
                            shared.push(WebEvent::HistoryChanged {
                                can_go_back: read_bool(|out| view.CanGoBack(out)),
                                can_go_forward: read_bool(|out| view.CanGoForward(out)),
                            });
                        }
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_HistoryChanged", &error))?;

            let shared = Rc::clone(&self.shared);
            webview
                .add_DocumentTitleChanged(
                    &DocumentTitleChangedEventHandler::create(Box::new(move |view, _| {
                        if let Some(view) = view.as_ref() {
                            shared.push(WebEvent::DocumentTitleChanged {
                                title: read_string(|out| view.DocumentTitle(out)),
                            });
                        }
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_DocumentTitleChanged", &error))?;

            let shared = Rc::clone(&self.shared);
            webview
                .add_SourceChanged(
                    &SourceChangedEventHandler::create(Box::new(move |view, _| {
                        if let Some(view) = view.as_ref() {
                            shared.push(WebEvent::SourceChanged {
                                uri: read_string(|out| view.Source(out)),
                            });
                        }
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_SourceChanged", &error))?;

            // The status bubble's text, with the engine's own bubble switched
            // off above: the preview's foot is already a band that says where
            // this seat's content lives, and §7.7 ③ makes it the same band that
            // says where a link goes.
            let shared = Rc::clone(&self.shared);
            let status: ICoreWebView2_12 = webview
                .cast()
                .map_err(|error| failure("ICoreWebView2_12", &error))?;
            status
                .add_StatusBarTextChanged(
                    &StatusBarTextChangedEventHandler::create(Box::new(move |view, _| {
                        let text = match view.as_ref().and_then(|view| view.cast().ok()) {
                            Some(view12) => {
                                let view12: ICoreWebView2_12 = view12;
                                read_string(|out| view12.StatusBarText(out))
                            }
                            None => String::new(),
                        };
                        shared.push(WebEvent::StatusBarTextChanged { text });
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_StatusBarTextChanged", &error))?;

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

            // **The download is still cancelled here, and now it is also
            // reported** (方案 §0: 「取消并外开可重放的 GET URL,不可重放者提示无法
            // 下载」). `SetCancel` cannot be decided later, so the refusal is
            // unconditional and the *answer* — hand the address to the machine's
            // browser, or raise the sheet — is the caller's, on its own turn.
            let shared = Rc::clone(&self.shared);
            let downloads: ICoreWebView2_4 = webview
                .cast()
                .map_err(|error| failure("ICoreWebView2_4", &error))?;
            downloads
                .add_DownloadStarting(
                    &DownloadStartingEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        args.SetCancel(true)?;
                        let (uri, file_name) = match args.DownloadOperation() {
                            Ok(operation) => (
                                read_string(|out| operation.Uri(out)),
                                read_string(|out| operation.ResultFilePath(out)),
                            ),
                            Err(_) => (String::new(), String::new()),
                        };
                        shared.push(WebEvent::DownloadStarting { uri, file_name });
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

            // **The site's own icon** (the favicon slice, `docs/DESIGN.md` §7.13, §7.7 ②). Announced and
            // never polled, exactly as the title and the address beside it are:
            // the head is rebuilt from what the seat last heard, and the engine
            // is the only thing that knows when a page has swapped its icon.
            //
            // `ICoreWebView2_15` is the interface that carries it, and the cast
            // is made once here rather than inside the callback for the reason
            // the `_12` cast above is made outside its own: a build whose runtime
            // is too old to answer this must fail while the handlers are being
            // installed — before the first navigation — and not silently draw
            // globes for ever.
            let shared = Rc::clone(&self.shared);
            let icons: ICoreWebView2_15 = webview
                .cast()
                .map_err(|error| failure("ICoreWebView2_15", &error))?;
            icons
                .add_FaviconChanged(
                    &FaviconChangedEventHandler::create(Box::new(move |view, _| {
                        // The address is read off the sender rather than the
                        // args, because `FaviconChanged`'s args are a bare
                        // `IUnknown` — the event says only *that* it changed.
                        let uri = match view.as_ref().and_then(|view| view.cast().ok()) {
                            Some(view15) => {
                                let view15: ICoreWebView2_15 = view15;
                                read_string(|out| view15.FaviconUri(out))
                            }
                            None => String::new(),
                        };
                        shared.push(WebEvent::FaviconChanged { uri });
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_FaviconChanged", &error))?;

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

    /// **The seat's rectangle inside the parent window**, in physical pixels —
    /// origin as well as size.
    ///
    /// # The origin used to be zero, and that was a defect (user report
    /// 2026-08-25, the right-click menu in the window's corner)
    ///
    /// Visual hosting makes the *visual* carry the placement, and this method
    /// used to take a size alone on that reasoning: the pixels come out of
    /// [`Self::send_mouse`]'s seat-local space and land where the visual's
    /// offset puts them, so an origin of `(0, 0)` cost the drawing nothing.
    /// Gate 3's measurement — a client point `(511, 242)` less the seat origin
    /// `(224, 48)`, divided by the 2.0 device pixel ratio, arriving at the page
    /// as `(143, 97)` — is still true and still the contract, because the point
    /// this host sends is **relative to these bounds** and the caller subtracts
    /// the same origin it passes here.
    ///
    /// What the reasoning missed is everything the engine draws *outside* its
    /// own visual. A context menu, a `<select>` popup, a print dialog and the
    /// IME candidate window are windows the engine positions itself, and the
    /// only thing it can position them against is the parent HWND's screen
    /// rectangle plus **these bounds**: DirectComposition offsets are invisible
    /// to it. With the origin pinned at zero every one of those appeared at the
    /// top-left corner of the whole terminal window, however far from the pane
    /// the press was — photographed on the machine with `Print / Save / Save as
    /// / Full screen` hanging over the files column while the click was three
    /// panes away.
    ///
    /// So the origin is sent, and [`WebHost::send_mouse`]'s doc says the other
    /// half of the same sentence: bounds carry where the seat is, the point
    /// carries where in the seat the pointer is, and neither is asked to mean
    /// the other.
    pub fn set_bounds(&self, x: i32, y: i32, width: u32, height: u32) -> Result<(), String> {
        let Some(controller) = self.controller.as_ref() else {
            return Ok(());
        };
        unsafe { controller.SetBounds(bounds_rect(x, y, width, height)) }
            .map_err(|error| failure("ICoreWebView2Controller::SetBounds", &error))
    }

    /// **How many device pixels one CSS pixel is** — the page's
    /// `devicePixelRatio`, told to the engine rather than discovered by it.
    ///
    /// With [`COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS`] the bounds this host
    /// sends are physical, so this number is the whole of what the page knows
    /// about the display it is on: it divides the rectangle into CSS pixels and
    /// it is what every raster inside the engine is sized for.
    ///
    /// **The host is the authority for it, and `configure` switches the
    /// engine's own detection off** so that it is the only one. A
    /// composition-hosted controller has no window to be told about a display
    /// change through; what it has is the parent window this host lends it, and
    /// what it does with that is late (see `configure`). The window knows the
    /// moment the scale factor moves, so it is the window that says so — from
    /// `bt_app::Runtime::apply_scale_factor`, and from the first placement a
    /// newly built controller gets.
    ///
    /// A no-op before the controller arrives, like every other setter here.
    pub fn set_rasterization_scale(&self, scale: f64) -> Result<(), String> {
        let Some(controller) = self.controller.as_ref() else {
            return Ok(());
        };
        let controller3: ICoreWebView2Controller3 = controller
            .cast()
            .map_err(|error| failure("ICoreWebView2Controller3", &error))?;
        unsafe { controller3.SetRasterizationScale(scale) }
            .map_err(|error| failure("SetRasterizationScale", &error))
    }

    /// **The parent window moved on the desktop.**
    ///
    /// The engine hangs its own windows — context menu, `<select>` popup, print
    /// dialog, IME candidates — off the parent HWND's screen rectangle plus its
    /// [`Self::set_bounds`]. In composition hosting it receives no window
    /// messages at all, so this call is the only way it learns the first of
    /// those two moved. `WM_MOVE` on the host window is the whole of when to
    /// say it.
    ///
    /// A no-op before the controller arrives, like every other setter here: the
    /// first `set_bounds` after it does is what tells a new controller where it
    /// stands.
    pub fn notify_parent_window_moved(&self) -> Result<(), String> {
        let Some(controller) = self.controller.as_ref() else {
            return Ok(());
        };
        unsafe { controller.NotifyParentWindowPositionChanged() }
            .map_err(|error| failure("NotifyParentWindowPositionChanged", &error))
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

    /// Stop whatever is loading. What the reload button turns into while a
    /// navigation is in flight (§7.7 ②).
    pub fn stop(&self) -> Result<(), String> {
        let Some(webview) = self.webview.as_ref() else {
            return Ok(());
        };
        unsafe { webview.Stop() }.map_err(|error| failure("ICoreWebView2::Stop", &error))
    }

    /// Walk the page's own navigation stack backwards.
    ///
    /// **Not guarded on `CanGoBack` here.** The caller draws the button from
    /// [`WebEvent::HistoryChanged`] and does not offer a press it cannot honour;
    /// a second guard on this side would be a second opinion about the same
    /// stack, read a frame later than the one the reader is looking at.
    pub fn go_back(&self) -> Result<(), String> {
        let Some(webview) = self.webview.as_ref() else {
            return Ok(());
        };
        unsafe { webview.GoBack() }.map_err(|error| failure("ICoreWebView2::GoBack", &error))
    }

    /// The same, forwards.
    pub fn go_forward(&self) -> Result<(), String> {
        let Some(webview) = self.webview.as_ref() else {
            return Ok(());
        };
        unsafe { webview.GoForward() }.map_err(|error| failure("ICoreWebView2::GoForward", &error))
    }

    /// Open the developer tools on this page, in the window the engine keeps for
    /// them.
    ///
    /// A window of the browser's own and not a surface of this one, which is the
    /// whole of what「C-精简」costs here: the tools are the engine's, they are
    /// worth a verb, and they are not worth this window growing a docked panel
    /// it would then have to lay out beside a page.
    pub fn open_dev_tools(&self) -> Result<(), String> {
        let Some(webview) = self.webview.as_ref() else {
            return Ok(());
        };
        unsafe { webview.OpenDevToolsWindow() }
            .map_err(|error| failure("ICoreWebView2::OpenDevToolsWindow", &error))
    }

    /// The page's zoom, as the engine holds it. `1.0` is unzoomed.
    pub fn zoom(&self) -> f64 {
        let Some(controller) = self.controller.as_ref() else {
            return 1.0;
        };
        let factor = read::<f64>(|out| unsafe { controller.ZoomFactor(out) });
        if factor > 0.0 { factor } else { 1.0 }
    }

    /// Set the page's zoom.
    ///
    /// The controller's zoom and not a transform on the visual: a scaled visual
    /// would resample the page's own raster, and what a reader asks for when
    /// they zoom a document is more text laid out larger, not the same text
    /// magnified.
    pub fn set_zoom(&self, factor: f64) -> Result<(), String> {
        let Some(controller) = self.controller.as_ref() else {
            return Ok(());
        };
        unsafe { controller.SetZoomFactor(factor) }
            .map_err(|error| failure("ICoreWebView2Controller::SetZoomFactor", &error))
    }

    /// Start — or restart — a find session over the page.
    ///
    /// The engine's own find dialog is suppressed, because the box a reader is
    /// typing into is this window's search capsule (§7.7 ②: 「第二个 host,不是第
    /// 二份实现」). Every count that comes back arrives as
    /// [`WebEvent::FindMatches`]; nothing here is polled, for
    /// [`WebEvent::HistoryChanged`]'s reason.
    pub fn find(&self, term: &str, case_sensitive: bool) -> Result<(), String> {
        let Some(find) = self.find_session()? else {
            return Ok(());
        };
        let environment: ICoreWebView2Environment15 = self
            .environment
            .as_ref()
            .ok_or_else(|| String::from("no environment to make find options from"))?
            .cast()
            .map_err(|error| failure("ICoreWebView2Environment15", &error))?;
        unsafe {
            let options = environment
                .CreateFindOptions()
                .map_err(|error| failure("CreateFindOptions", &error))?;
            options
                .SetFindTerm(&HSTRING::from(term))
                .map_err(|error| failure("SetFindTerm", &error))?;
            options
                .SetIsCaseSensitive(case_sensitive)
                .map_err(|error| failure("SetIsCaseSensitive", &error))?;
            options
                .SetShouldHighlightAllMatches(true)
                .map_err(|error| failure("SetShouldHighlightAllMatches", &error))?;
            options
                .SetSuppressDefaultFindDialog(true)
                .map_err(|error| failure("SetSuppressDefaultFindDialog", &error))?;
            let shared = Rc::clone(&self.shared);
            let session = find.clone();
            find.Start(
                &options,
                &FindStartCompletedHandler::create(Box::new(move |_| {
                    shared.push(WebEvent::FindMatches {
                        count: read::<i32>(|out| session.MatchCount(out)),
                        active: read::<i32>(|out| session.ActiveMatchIndex(out)),
                    });
                    Ok(())
                })),
            )
            .map_err(|error| failure("ICoreWebView2Find::Start", &error))
        }
    }

    /// Walk to the next match, or the previous one. Both wrap, which is what the
    /// capsule's own walk does.
    pub fn find_step(&self, forwards: bool) -> Result<(), String> {
        let Some(find) = self.find_session()? else {
            return Ok(());
        };
        unsafe {
            if forwards {
                find.FindNext()
            } else {
                find.FindPrevious()
            }
        }
        .map_err(|error| failure("ICoreWebView2Find::FindNext", &error))
    }

    /// End the find session and take the page's highlights off.
    pub fn find_stop(&self) -> Result<(), String> {
        let Some(find) = self.find_session()? else {
            return Ok(());
        };
        unsafe { find.Stop() }.map_err(|error| failure("ICoreWebView2Find::Stop", &error))
    }

    /// The page's find session, with its two counters' events attached once.
    ///
    /// `Ok(None)` means there is no controller yet — the seat is still coming
    /// up, and a find asked for before the page exists is a find with nothing to
    /// search. A runtime too old to carry `ICoreWebView2_28` is an `Err`, said
    /// out loud rather than silently answering zero: "no matches" and "this
    /// build cannot count" are two different things and only one of them is
    /// about the page.
    fn find_session(&self) -> Result<Option<ICoreWebView2Find>, String> {
        let Some(webview) = self.webview.as_ref() else {
            return Ok(None);
        };
        let view28: ICoreWebView2_28 = webview
            .cast()
            .map_err(|error| failure("ICoreWebView2_28", &error))?;
        let find =
            unsafe { view28.Find() }.map_err(|error| failure("ICoreWebView2::Find", &error))?;
        if self.find_attached.replace(true) {
            return Ok(Some(find));
        }
        let mut token = 0i64;
        unsafe {
            let shared = Rc::clone(&self.shared);
            find.add_MatchCountChanged(
                &FindMatchCountChangedEventHandler::create(Box::new(move |session, _| {
                    if let Some(session) = session.as_ref() {
                        shared.push(WebEvent::FindMatches {
                            count: read::<i32>(|out| session.MatchCount(out)),
                            active: read::<i32>(|out| session.ActiveMatchIndex(out)),
                        });
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(|error| failure("add_MatchCountChanged", &error))?;
            let shared = Rc::clone(&self.shared);
            find.add_ActiveMatchIndexChanged(
                &FindActiveMatchIndexChangedEventHandler::create(Box::new(move |session, _| {
                    if let Some(session) = session.as_ref() {
                        shared.push(WebEvent::FindMatches {
                            count: read::<i32>(|out| session.MatchCount(out)),
                            active: read::<i32>(|out| session.ActiveMatchIndex(out)),
                        });
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(|error| failure("add_ActiveMatchIndexChanged", &error))?;
        }
        Ok(Some(find))
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
    ///
    /// It is the same origin the caller hands [`Self::set_bounds`], and that is
    /// the whole of the coordinate contract: the bounds say where the seat is in
    /// the window, this point says where in the seat the pointer is. The engine
    /// adds them back together itself when it has to name a screen position —
    /// which is what a context menu is.
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

    /// **Ask the engine for a picture of what is on its glass** (W2 slice ⑥).
    ///
    /// `CapturePreview` is the only pixel channel the SDK offers a hosted page
    /// (`plan.md` §1's second row), and it has three properties this signature is
    /// shaped around:
    ///
    /// * **It is asynchronous, and the wait is not the caller's to make.** The
    ///   measured latency is 33–85 ms depending on the viewport
    ///   (`w0p-evidence` gate 11), and a window that pumped its own messages
    ///   until the answer came would run the whole application re-entrantly —
    ///   the same reason nothing else in this file blocks. So this returns the
    ///   moment the ask is made, and the picture arrives later as
    ///   [`WebEvent::Captured`]. The synchronous half measured **0.115 ms**,
    ///   which is what the caller's frame actually pays.
    /// * **It has no size parameter.** What comes back is the viewport, at the
    ///   size the controller was last given. Anything smaller is the caller's
    ///   resample.
    /// * **A hidden WebView never answers at all.** Measured three times across
    ///   two re-verification runs: the completion handler is not called and the
    ///   ask simply hangs. So the caller must not ask unless the page is on the
    ///   glass, and this cannot check that for it — `SetIsVisible` is state the
    ///   caller owns.
    ///
    /// PNG rather than JPEG: the two encode at the same speed (63.9 ms against
    /// 64.0 at pane size) and one of them is lossless.
    pub fn capture_preview(&self) -> Result<(), String> {
        use windows::Win32::Foundation::HGLOBAL;
        use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
        let Some(webview) = self.webview.as_ref() else {
            return Ok(());
        };
        let stream = unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) }
            .map_err(|error| failure("CreateStreamOnHGlobal", &error))?;
        let shared = Rc::clone(&self.shared);
        // The stream is moved into the handler rather than kept here: the only
        // moment its bytes are wanted is the moment the engine says it has
        // finished writing them, and a stream held on `self` would be a second
        // owner of a buffer whose life is exactly one call long.
        let sink = stream.clone();
        let handler = CapturePreviewCompletedHandler::create(Box::new(
            move |result: windows::core::Result<()>| {
                let png = result.ok().and_then(|()| read_stream(&sink));
                shared.push(WebEvent::Captured { png });
                Ok(())
            },
        ));
        unsafe {
            webview.CapturePreview(
                COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                &stream,
                &handler,
            )
        }
        .map_err(|error| failure("CapturePreview", &error))
    }

    /// **Ask the engine for the icon the page is wearing** (the favicon slice, `docs/DESIGN.md` §7.13).
    ///
    /// Asked rather than pushed, and asked only after
    /// [`WebEvent::FaviconChanged`] has said there is something new: the engine
    /// re-reads the resource each time, so a caller that asked on its own clock
    /// would be paying for an answer it already had.
    ///
    /// Three things this signature is shaped around, and two of them are
    /// [`Self::capture_preview`]'s:
    ///
    /// * **It is asynchronous**, so this returns the moment the ask is made and
    ///   the picture arrives as [`WebEvent::Favicon`].
    /// * **The stream is the engine's**, not the caller's — unlike
    ///   `CapturePreview`, which is handed one to write into. So there is no
    ///   `CreateStreamOnHGlobal` here; the completion hands over a stream that
    ///   is already full.
    /// * **PNG and not JPEG**, and here that is not a tie broken on speed. What
    ///   a site actually served is very often an `.ico`, a format nothing in
    ///   this workspace can decode; asking for PNG makes the engine re-encode
    ///   whatever it holds, which is how the `.ico` problem stops being ours.
    ///   Lossless also matters at this size in a way it does not for a
    ///   thumbnail: fourteen pixels of a drawing have no detail to spare.
    pub fn get_favicon(&self) -> Result<(), String> {
        let Some(webview) = self.webview.as_ref() else {
            return Ok(());
        };
        let icons: ICoreWebView2_15 = webview
            .cast()
            .map_err(|error| failure("ICoreWebView2_15", &error))?;
        let shared = Rc::clone(&self.shared);
        let handler = GetFaviconCompletedHandler::create(Box::new(
            move |result: windows::core::Result<()>,
                  stream: Option<windows::Win32::System::Com::IStream>| {
                let png = result
                    .ok()
                    .and_then(|()| stream.as_ref().and_then(read_stream));
                shared.push(WebEvent::Favicon { png });
                Ok(())
            },
        ));
        unsafe { icons.GetFavicon(COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, &handler) }
            .map_err(|error| failure("GetFavicon", &error))
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

/// Everything an `IStream` holds, from its start.
///
/// `None` rather than an error string: the one caller is a completion handler,
/// which has nowhere to report to and one thing to say — there is a picture, or
/// there is not.
fn read_stream(stream: &windows::Win32::System::Com::IStream) -> Option<Vec<u8>> {
    use windows::Win32::System::Com::{STREAM_SEEK_END, STREAM_SEEK_SET};
    let mut length = 0u64;
    unsafe { stream.Seek(0, STREAM_SEEK_END, Some(&mut length)) }.ok()?;
    unsafe { stream.Seek(0, STREAM_SEEK_SET, None) }.ok()?;
    let mut bytes = vec![0u8; usize::try_from(length).ok()?];
    let mut read = 0u32;
    unsafe {
        stream.Read(
            bytes.as_mut_ptr().cast(),
            u32::try_from(bytes.len()).ok()?,
            Some(&mut read),
        )
    }
    .ok()
    .ok()?;
    bytes.truncate(read as usize);
    Some(bytes)
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

/// **One seat's rectangle in the parent window's client space**, as the engine
/// is given it.
///
/// A function of four numbers, held apart from the COM call so that the one
/// thing this repository got wrong about it can be held by a test: the origin.
/// See [`WebHost::set_bounds`] for what pinning it at zero cost.
fn bounds_rect(x: i32, y: i32, width: u32, height: u32) -> RECT {
    RECT {
        left: x,
        top: y,
        right: x + width as i32,
        bottom: y + height as i32,
    }
}

/// **The rectangle the engine is given carries the seat's origin.**
#[cfg(test)]
mod bounds_geometry_tests {
    use super::*;

    /// RED — **the engine is told where the seat is, not only how big it is**
    /// (user report 2026-08-25: the page's own right-click menu opened in the
    /// window's top-left corner, panes away from the press).
    ///
    /// This is the whole of the defect, held as arithmetic. The engine positions
    /// every window it owns — context menu, `<select>` popup, print dialog, IME
    /// candidates — at the parent HWND's screen origin plus these bounds, and a
    /// DirectComposition offset is a thing it cannot see. So a rectangle whose
    /// `left`/`top` are zero says "this seat begins at the window's corner", and
    /// the menu obeys.
    ///
    /// RED GATE: put `left: 0, top: 0` back into [`bounds_rect`] — which is
    /// exactly what stood here until this ticket — and the first two assertions
    /// fail while the size ones still pass, which is precisely how the defect
    /// hid: everything that was *drawn* stayed right.
    #[test]
    fn the_bounds_the_engine_is_given_begin_at_the_seat_and_not_at_the_window() {
        // The measurement gate 3 took, and the seat it took it in: a pane whose
        // origin inside the window is (224, 48).
        let rect = bounds_rect(224, 48, 800, 600);
        assert_eq!(rect.left, 224, "the engine is told where the seat begins");
        assert_eq!(rect.top, 48, "on both axes");
        assert_eq!(rect.right, 1024, "and the far edge follows the origin");
        assert_eq!(rect.bottom, 648);
        assert_eq!(rect.right - rect.left, 800, "the size is unchanged by it");
        assert_eq!(rect.bottom - rect.top, 600);
    }

    /// A seat at the window's own corner is the one case the old spelling got
    /// right, and it still has to be right — otherwise a fix that only ever
    /// added an offset would be untestable against the case it came from.
    #[test]
    fn a_seat_at_the_corner_is_the_rectangle_it_always_was() {
        assert_eq!(
            bounds_rect(0, 0, 1920, 1200),
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1200,
            }
        );
    }
}

/// **The handoff order and its compensation table, held without a browser.**
///
/// The order is the whole safety argument of [`WebHost::rehost`] — a page that
/// changed parent window before its old root visual target was let go composes
/// into a visual belonging to a window it is no longer in — and it is exactly
/// the thing a COM call cannot be asked about afterwards. So it is a value here,
/// the executor walks it, and these tests hold it.
#[cfg(test)]
mod rehost_contract_tests {
    use super::*;

    /// RED — the handoff order, and the one order that is safe.
    ///
    /// `plan.md` v3 增补 F1a: 隐藏 controller → `put_RootVisualTarget(nullptr)` →
    /// 源 device commit → `put_ParentWindow(new_hwnd)` → 设目标 target → 目标
    /// device commit → bounds/presence → `NotifyParentWindowPositionChanged`.
    ///
    /// MUTATIONS:
    /// ① move `CommitSource` after `ParentWindow` — the source device would
    ///    publish a tree whose content belongs to another window;
    /// ② move `ClearRootVisualTarget` after `ParentWindow` — the engine would be
    ///    asked to let go of a visual under a parent it no longer has.
    #[test]
    fn the_handoff_walks_the_one_order_the_contract_fixes() {
        assert_eq!(
            REHOST_SEQUENCE,
            [
                RehostStep::Hide,
                RehostStep::ClearRootVisualTarget,
                RehostStep::CommitSource,
                RehostStep::ParentWindow,
                RehostStep::SetRootVisualTarget,
                RehostStep::CommitTarget,
                RehostStep::Bounds,
                RehostStep::Presence,
                RehostStep::NotifyPosition,
            ]
        );
    }

    /// RED — a failure at the first step has nothing to put back.
    #[test]
    fn a_handoff_that_fails_before_it_changes_anything_compensates_nothing() {
        assert_eq!(
            rehost_compensation(RehostStep::Hide),
            RehostCompensation::default()
        );
        assert!(rehost_compensation(RehostStep::Hide).is_empty());
    }

    /// RED — once the old target is let go, the old target is what comes back.
    ///
    /// And **not** the parent window: `ParentWindow` has not run yet, so a
    /// compensation that set it would be putting back something never taken.
    #[test]
    fn a_handoff_that_fails_after_letting_go_of_the_old_target_puts_the_old_target_back() {
        for step in [RehostStep::CommitSource, RehostStep::ParentWindow] {
            let put_back = rehost_compensation(step);
            assert!(put_back.root_visual_target, "{step:?}");
            assert!(put_back.presence, "{step:?}");
            assert!(!put_back.parent_window, "{step:?}");
            assert!(!put_back.bounds, "{step:?}");
        }
    }

    /// RED — once the parent moved, the parent comes back too.
    #[test]
    fn a_handoff_that_fails_after_the_parent_moved_puts_the_parent_back() {
        for step in [
            RehostStep::SetRootVisualTarget,
            RehostStep::CommitTarget,
            RehostStep::Bounds,
        ] {
            let put_back = rehost_compensation(step);
            assert!(put_back.parent_window, "{step:?}");
            assert!(put_back.root_visual_target, "{step:?}");
            assert!(put_back.presence, "{step:?}");
            assert!(!put_back.bounds, "{step:?}");
        }
    }

    /// RED — and the last two steps put every one of the four back.
    #[test]
    fn a_handoff_that_fails_at_the_end_puts_all_four_back() {
        for step in [RehostStep::Presence, RehostStep::NotifyPosition] {
            assert_eq!(
                rehost_compensation(step),
                RehostCompensation {
                    parent_window: true,
                    root_visual_target: true,
                    bounds: true,
                    presence: true,
                },
                "{step:?}"
            );
        }
    }

    /// RED — **compensation only grows.** A step that changed something can
    /// never be dropped from a later step's undo list, which is the property
    /// that makes the table safe to extend: a tenth step added to
    /// [`REHOST_SEQUENCE`] inherits everything the nine before it changed.
    #[test]
    fn every_later_failure_puts_back_at_least_what_an_earlier_one_does() {
        let mut previous = RehostCompensation::default();
        for step in REHOST_SEQUENCE {
            let put_back = rehost_compensation(step);
            for (was, is) in [
                (previous.parent_window, put_back.parent_window),
                (previous.root_visual_target, put_back.root_visual_target),
                (previous.bounds, put_back.bounds),
                (previous.presence, put_back.presence),
            ] {
                assert!(
                    !was || is,
                    "{step:?} drops a compensation an earlier step owed"
                );
            }
            previous = put_back;
        }
    }
}
