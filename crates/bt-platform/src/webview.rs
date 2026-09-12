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
//! (`docs/plans/web-preview/w0-evidence.md`).
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

// **The data half of this file is compiled everywhere and the engine half is
// not** (M1-1). `bt-app`'s `webhost.rs` names thirteen items from this module
// and twelve of them are plain data — a chord, an event, a verdict, the two
// ends of a rehost — which describe what a window wants from a page rather than
// what WebView2 does about it. Those stay ungated and are the same types on
// both platforms, exactly as `docs/plans/port/backend-inventory-2026-09-12.md`
// §6 ② asks: the macOS name set is decided here, before the module is split,
// rather than after. The thirteenth is `WebHost`, and off Windows it is
// `webview_portable.rs` at the bottom of this file.
#[cfg(windows)]
use std::cell::RefCell;
#[cfg(windows)]
use std::collections::VecDeque;
#[cfg(windows)]
use std::path::Path;
#[cfg(windows)]
use std::rc::Rc;

use crate::NativeWindow;
#[cfg(windows)]
use webview2_com::Microsoft::Web::WebView2::Win32::*;
// Named one by one rather than globbed: `webview2_com` exports a `Result` alias
// of its own, and a glob here would quietly make every `Result<(), String>` in
// this file mean something else.
#[cfg(windows)]
use webview2_com::{
    AcceleratorKeyPressedEventHandler, BrowserProcessExitedEventHandler,
    CapturePreviewCompletedHandler, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2CompositionControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, CursorChangedEventHandler,
    DocumentTitleChangedEventHandler, DownloadStartingEventHandler, FaviconChangedEventHandler,
    FindActiveMatchIndexChangedEventHandler, FindMatchCountChangedEventHandler,
    FindStartCompletedHandler, FocusChangedEventHandler, GetFaviconCompletedHandler,
    HistoryChangedEventHandler, IsDocumentPlayingAudioChangedEventHandler,
    LaunchingExternalUriSchemeEventHandler, MoveFocusRequestedEventHandler,
    NavigationCompletedEventHandler, NavigationStartingEventHandler,
    NewBrowserVersionAvailableEventHandler, NewWindowRequestedEventHandler,
    PermissionRequestedEventHandler, ProcessFailedEventHandler, ScriptDialogOpeningEventHandler,
    SourceChangedEventHandler, StatusBarTextChangedEventHandler, WebResourceRequestedEventHandler,
    take_pwstr,
};
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, POINT, RECT};
#[cfg(windows)]
use windows::core::{BOOL, HSTRING, IUnknown, Interface as _, PCWSTR, PWSTR};

use super::PageVisual;
use crate::Compositor;

// ── Reading out-parameters ─────────────────────────────────────────────────
//
// Every WebView2 getter is `fn(&self, *mut T) -> Result<()>`. Reading one at
// each call site would bury the fact being read in five lines of ceremony, so
// the ceremony lives here once. A getter that fails yields the type's default,
// which for every field below reads as "the engine did not say".

#[cfg(windows)]
fn read<T: Default>(getter: impl FnOnce(*mut T) -> windows::core::Result<()>) -> T {
    let mut value = T::default();
    let _ = getter(&mut value);
    value
}

#[cfg(windows)]
fn read_bool(getter: impl FnOnce(*mut BOOL) -> windows::core::Result<()>) -> bool {
    read::<BOOL>(getter).as_bool()
}

#[cfg(windows)]
fn read_string(getter: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>) -> String {
    let mut value = PWSTR::null();
    match getter(&mut value) {
        Ok(()) => take_pwstr(value),
        Err(_) => String::new(),
    }
}

#[cfg(windows)]
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
/// A Win32 virtual key and four booleans, because that is the vocabulary
/// `AcceleratorKeyPressed` speaks and there is no second one. Translating the
/// product's own table into this is `bt_app::webhost::claimable_chords`.
///
/// **`command` is macOS's, and it is data before it is routing** (M1-1, for
/// M1-7). The plan's §4.4 ② names this struct as a signature that encodes
/// Windows without saying so: three modifiers is not a missing field on
/// Windows, it is the complete set, and on macOS the modifier that carries
/// every application shortcut — `Cmd+C`, `Cmd+T`, `Cmd+W` — had nowhere to go,
/// so a Command chord over a focused page degraded to an unmodified key
/// (`webhost.rs` copied three booleans and the fourth did not exist to copy).
/// The field is added here, now, so that the type is the same on both
/// platforms before anything is written against it; **what fills it and what
/// it means for `BINDINGS` is M1-7**, and the Windows arm leaves it `false`
/// because Win32's accelerator callback has no such modifier to report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebChord {
    pub virtual_key: u16,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// The macOS Command modifier. Always `false` on Windows — see the type's
    /// own note.
    pub command: bool,
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
    /// **The document on this seat started or stopped making a sound**
    /// (`ICoreWebView2_8::DocumentPlayingAudioChanged`; user ruling 2026-08-27,
    /// `docs/DESIGN.md` §7.23 ⑩).
    ///
    /// The engine's own reading of its document, forwarded rather than inferred:
    /// a host that concluded "a video was opened, therefore there is a sound"
    /// would put a mark on a tab whose video is paused, is muted, or has run to
    /// its end, and would miss a page that makes a noise without this window
    /// having opened a video at all.
    ///
    /// **A level and not an edge.** The engine fires the change and the boolean
    /// travels with it, so a caller never has to keep a count of how many times
    /// it has been told — the last thing it heard is the truth, which is what
    /// makes a rebuilt browser's silence correct without anybody resetting
    /// anything.
    PlayingAudioChanged {
        playing: bool,
    },
    /// **A page asked for a modal dialog and did not get one** (R1-21).
    ///
    /// `kind` is `COREWEBVIEW2_SCRIPT_DIALOG_KIND`: 0 alert, 1 confirm, 2
    /// prompt, 3 the one a page raises on its way out. The engine's own dialogs
    /// are off, so this is the whole of what happens — the ask is answered as a
    /// dismissal inside the callback, because `ICoreWebView2ScriptDialogOpeningEventArgs::Accept`
    /// cannot be decided later, and what the seat does with the news is say so
    /// on its foot.
    ScriptDialogDismissed {
        kind: i32,
    },
    /// **A frame or a subresource this seat would not fetch** (R1-10).
    ///
    /// Not a [`Self::NavigationStarting`] with `cancelled` set: that one is
    /// about where the seat itself was going, and the caller answers it with a
    /// card and a loading state. This is about something inside a document that
    /// is already on the glass, and the answer to it is the request being
    /// refused — which has already happened by the time this is queued. It
    /// travels so that a page missing a picture is a line in the trace rather
    /// than a mystery.
    RequestRefused {
        uri: String,
    },
}

/// What the caller's policy says about one thing a document asked for that is
/// not a navigation — a picture, a stylesheet, a script, a font or a frame.
///
/// Two answers and not three: a subresource cannot be sent somewhere else the
/// way a navigation can, because the only thing that could act on a rewrite is
/// the document that named it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebRequestVerdict {
    /// Let the engine fetch it.
    Allow,
    /// Answer it here, with nothing.
    Refuse,
}

/// **Which of the seat's guarantees this controller actually carries** (R2-16).
///
/// Every one of them is a handler or a switch that install either attached or
/// could not, and a seat that cannot say which it has is a seat that has to
/// assume it has them all. So they are counted rather than assumed, and the
/// caller fails closed on what is missing: a local file is not opened on a
/// controller that cannot gate what the document loads or cannot answer its
/// dialogs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebGuards {
    /// `AreDefaultScriptDialogsEnabled(false)` **and** `ScriptDialogOpening`.
    /// Both, because either one alone is the wrong half: the switch without the
    /// handler is a page whose `alert` never returns, and the handler without
    /// the switch is the engine's own window opening anyway.
    pub script_dialogs: bool,
    /// `FrameNavigationStarting` — where a frame in the document is going.
    pub frame_navigation: bool,
    /// `WebResourceRequested`, with a filter over every context — everything
    /// else the document names.
    pub resource_requests: bool,
}

impl WebGuards {
    /// Nothing attached yet, which is what a controller has before install
    /// walks it.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            script_dialogs: false,
            frame_navigation: false,
            resource_requests: false,
        }
    }

    /// Whether every one of them stands. The caller's whole question.
    #[must_use]
    pub const fn all_stand(self) -> bool {
        self.script_dialogs && self.frame_navigation && self.resource_requests
    }

    /// The ones that do not, named as the engine names them, for the line of
    /// fact under the card.
    #[must_use]
    pub fn missing(self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if !self.script_dialogs {
            missing.push("ScriptDialogOpening");
        }
        if !self.frame_navigation {
            missing.push("FrameNavigationStarting");
        }
        if !self.resource_requests {
            missing.push("WebResourceRequested");
        }
        missing
    }
}

/// What [`WebHost::install`] managed to establish.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebInstallReport {
    pub guards: WebGuards,
    /// The switches this build of the runtime would not take. Never a
    /// [`WebSettingRule::Required`] one — that is an `Err` and the seat has no
    /// engine.
    pub unapplied: Vec<WebSetting>,
}

/// One step of taking a controller into service.
///
/// The same shape as [`RehostStep`] and for the same reason: what a failure
/// owes is decided by how far the walk got, and a table is the only form of
/// that fact a test can hold.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallStep {
    /// Take the controller the callback left, and read its `CoreWebView2` and
    /// its composition interface off it.
    TakeController,
    /// Walk [`WEB_SETTINGS`].
    Configure,
    /// Attach every handler that belongs to this controller, the three gates
    /// among them.
    AttachEvents,
    /// Attach the two that belong to the process-wide environment.
    AttachEnvironmentEvents,
    /// Point the controller at this seat's visual.
    PointAtVisual,
}

/// Install, in order. [`WebHost::install`] walks exactly this.
pub const INSTALL_SEQUENCE: [InstallStep; 5] = [
    InstallStep::TakeController,
    InstallStep::Configure,
    InstallStep::AttachEvents,
    InstallStep::AttachEnvironmentEvents,
    InstallStep::PointAtVisual,
];

/// What a half-finished install has to take back (R2-16).
///
/// Two booleans and not a list of steps, for [`RehostCompensation`]'s reason:
/// the undo is not the walk run backwards. A controller that was configured and
/// then failed to attach its events is closed, not un-configured, and the
/// environment's own subscriptions are removed whether one of them or both got
/// on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InstallRollback {
    /// A controller exists and must be closed. Dropping it is not closing it:
    /// the browser process tree goes on running with nobody pointing at it,
    /// which is the leak the generation token exists to prevent.
    pub controller: bool,
    /// Handlers were put on the process-wide environment and must come off it,
    /// or every failed install leaves one more of them alive for the life of
    /// the process (R2-10).
    pub environment_events: bool,
}

impl InstallStep {
    /// What running this step leaves behind, and therefore what a **later**
    /// step's failure owes.
    fn leaves(self) -> InstallRollback {
        let mut left = InstallRollback::default();
        match self {
            Self::TakeController => left.controller = true,
            Self::AttachEnvironmentEvents => left.environment_events = true,
            // Configuring changes a controller that is already owed, attaching
            // this controller's own handlers dies with it, and pointing it at a
            // visual is undone by closing it.
            Self::Configure | Self::AttachEvents | Self::PointAtVisual => {}
        }
        left
    }
}

/// What has to be taken back when install fails **at** `failed_at`.
///
/// Folded over the steps that already ran rather than written out, exactly as
/// [`rehost_compensation`] is: a second copy of the sequence is the thing that
/// goes stale when a step moves.
#[must_use]
pub fn install_rollback(failed_at: InstallStep) -> InstallRollback {
    let mut owed = InstallRollback::default();
    for step in INSTALL_SEQUENCE {
        if step == failed_at {
            break;
        }
        let left = step.leaves();
        owed.controller |= left.controller;
        owed.environment_events |= left.environment_events;
    }
    owed
}

/// **One thing closing a seat lets go of** (R2-10, R2-12, R2-13, R2-24).
///
/// A table for [`WEB_SETTINGS`]'s reason: what has to be right is the **set**,
/// and the set is everything install created. Four of the six were missing from
/// the run of statements this replaces, and each cost something different — a
/// handler left on the process-wide environment for the life of the process, a
/// cached environment that made the next rebuild adopt the very thing it was
/// rebuilding away from, a controller callback still holding a slot nobody
/// would ever come for, and a latch that made a rebuilt page stop counting its
/// own search matches. None of them is visible in a run of statements; all of
/// them are visible as a row that is not there.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseStep {
    /// `ICoreWebView2Controller::Close`, and the controller with it.
    Controller,
    /// The composition controller this host held beside it.
    Composition,
    /// The `ICoreWebView2` read off the controller.
    Webview,
    /// `remove_BrowserProcessExited` and `remove_NewBrowserVersionAvailable` on
    /// the environment they were added to (R2-10).
    EnvironmentEvents,
    /// The environment this host cached for itself (R2-12). The process-wide
    /// one is not this host's to drop — [`forget_web_environment`] is the
    /// caller's separate decision — but a host that kept its own copy would
    /// hand it back to the rebuild that exists to abandon it.
    CachedEnvironment,
    /// The slot the controller callback writes into (R2-13). A close that left
    /// it standing left the next generation's `install` free to adopt a
    /// controller made for the generation before it.
    PendingController,
    /// The latch that says the find session's counters are subscribed (R2-24).
    /// The session belongs to the controller, so a new controller has a new
    /// session and the latch is a statement about a page that is gone.
    FindLatch,
}

/// Everything closing lets go of. [`WebHost::close`] walks exactly this.
pub const WEB_CLOSE_STEPS: [CloseStep; 7] = [
    CloseStep::Controller,
    CloseStep::Composition,
    CloseStep::Webview,
    CloseStep::EnvironmentEvents,
    CloseStep::CachedEnvironment,
    CloseStep::PendingController,
    CloseStep::FindLatch,
];

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

#[cfg(windows)]
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

#[cfg(windows)]
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
#[cfg(windows)]
pub fn forget_web_environment() {
    ENVIRONMENT.with(|cell| *cell.borrow_mut() = None);
}

/// The runtime's version, asked of the loader rather than of the registry.
///
/// The registry lies and the API does not: gate 7 removed the runtime and the
/// `HKLM\WOW6432Node` key went on reporting a version that was no longer
/// installed, while this call failed with `0x80070002` in 0 ms.
#[cfg(windows)]
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

/// One switch this host decides for the engine rather than inheriting.
///
/// The names are the engine's own — [`WebSetting::api`] gives back the exact
/// method, which is what a failure has to name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebSetting {
    /// `ICoreWebView2Settings::IsWebMessageEnabled` — the page's way out to the
    /// host.
    WebMessage,
    /// `ICoreWebView2Settings::AreHostObjectsAllowed` — the host's way in to the
    /// page.
    HostObjects,
    /// `ICoreWebView2Settings::IsStatusBarEnabled`.
    StatusBar,
    /// `ICoreWebView2Settings::AreDevToolsEnabled`.
    DevTools,
    /// `ICoreWebView2Settings::AreDefaultContextMenusEnabled`.
    DefaultContextMenus,
    /// `ICoreWebView2Settings4::IsGeneralAutofillEnabled` — form data saved into
    /// the profile directory.
    GeneralAutofill,
    /// `ICoreWebView2Settings4::IsPasswordAutosaveEnabled` — passwords saved into
    /// the profile directory.
    PasswordAutosave,
    /// `ICoreWebView2Settings::IsScriptEnabled` — whether the page runs its own
    /// script at all.
    Script,
    /// `ICoreWebView2Settings::AreDefaultScriptDialogsEnabled` — the engine's
    /// own modal `alert`, `confirm` and `prompt` windows.
    DefaultScriptDialogs,
}

/// **What a switch that could not be set costs**, which is the only thing that
/// decides what happens next (R2-16).
///
/// Written per row rather than "all or nothing", because the seven were not all
/// the same kind of decision and a single cast in front of the loop said they
/// were: a runtime too old to answer one of them applied none, so a preference
/// nobody would miss took the page's way out to the host down with it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebSettingRule {
    /// The seat has no engine without it. Installing fails and the seat wears
    /// the card that says the engine did not start.
    Required,
    /// The seat may still browse, but it will not open a local file: a document
    /// off somebody's own disk is the case this switch was the guard for.
    Guard,
    /// A decision this host makes and would rather have, worth saying out loud
    /// when a build cannot take it and not worth refusing a page over.
    Preference,
}

impl WebSetting {
    /// The engine method this switch is set through, for the failure that names
    /// it.
    #[must_use]
    pub const fn api(self) -> &'static str {
        match self {
            Self::WebMessage => "SetIsWebMessageEnabled",
            Self::HostObjects => "SetAreHostObjectsAllowed",
            Self::StatusBar => "SetIsStatusBarEnabled",
            Self::DevTools => "SetAreDevToolsEnabled",
            Self::DefaultContextMenus => "SetAreDefaultContextMenusEnabled",
            Self::GeneralAutofill => "SetIsGeneralAutofillEnabled",
            Self::PasswordAutosave => "SetIsPasswordAutosaveEnabled",
            Self::Script => "SetIsScriptEnabled",
            Self::DefaultScriptDialogs => "SetAreDefaultScriptDialogsEnabled",
        }
    }

    /// **The lowest interface that carries this switch** (R2-16).
    ///
    /// The one fact that decides which runtimes can be told this, and it used
    /// to be nobody's: the loop was written inside a single
    /// `ICoreWebView2Settings4` cast, so the two switches that genuinely need
    /// that interface made the five that do not unreachable on any build
    /// without it.
    #[must_use]
    pub const fn interface(self) -> &'static str {
        match self {
            Self::WebMessage
            | Self::HostObjects
            | Self::StatusBar
            | Self::DevTools
            | Self::DefaultContextMenus
            | Self::Script
            | Self::DefaultScriptDialogs => "ICoreWebView2Settings",
            Self::GeneralAutofill | Self::PasswordAutosave => "ICoreWebView2Settings4",
        }
    }

    /// What a build that cannot take this switch costs the reader.
    #[must_use]
    pub const fn rule(self) -> WebSettingRule {
        match self {
            // The page's way out to the host and the host's way in to the page.
            // A seat that cannot shut both is not a seat this window offers.
            Self::WebMessage | Self::HostObjects => WebSettingRule::Required,
            // Both file what a person typed into a profile directory on disk
            // that outlives the window. There is no page worth that.
            Self::GeneralAutofill | Self::PasswordAutosave => WebSettingRule::Required,
            // The guard the dialog handler stands behind: with the engine's own
            // dialogs still on, a page answers for its own modality and can sit
            // on the seat with them.
            Self::DefaultScriptDialogs => WebSettingRule::Guard,
            Self::StatusBar | Self::DevTools | Self::DefaultContextMenus | Self::Script => {
                WebSettingRule::Preference
            }
        }
    }
}

/// **Every engine switch this host decides, and the value it decides on.**
/// [`WebHost::configure`] walks exactly this.
///
/// A table rather than a run of calls, because the thing that has to be right is
/// the **set**. Each row is a value WebView2 has a default for, and every default
/// this host does not overrule is a decision made by nobody — which is exactly
/// how two autofill switches came to be on in a shipped build (release audit
/// 2026-08-27, Codex 漏 10). So the set is a value a test can hold, and a switch
/// added to the engine's API arrives here as a row somebody had to type on
/// purpose.
///
/// The order is the order they are set in, and nothing depends on it.
pub const WEB_SETTINGS: [(WebSetting, bool); 9] = [
    // Slice ① hosts a page and offers it nothing. The bridge, the status bar and
    // the developer tools are all slice ②'s and slice ④'s to decide about.
    (WebSetting::WebMessage, false),
    // **The other half of the bridge** (W2 slice 5, plan section 3's controlled
    // file entry). `IsWebMessageEnabled` closes the page's way *out*; this closes
    // the host's way *in*. Both are off for the same sentence: a local page
    // opened out of the files column is read, and nothing in this product offers
    // it an object, a method or a channel. Neither is conditional on where the
    // page came from, because a switch that is only thrown for `file:` pages is a
    // switch somebody has to remember to throw.
    (WebSetting::HostObjects, false),
    (WebSetting::StatusBar, false),
    // **On since slice ④**, which is what a verb for it means: the head carries a
    // `Developer tools` tool and `F12` is a row of the shortcut table, and
    // neither can do anything against a controller that has the tools switched
    // off. The window keeps the key — `webhost::claimable_chords` claims `F12`
    // while a page has the focus — so the engine's own accelerator never reaches
    // the page and there is still one door.
    (WebSetting::DevTools, true),
    (WebSetting::DefaultContextMenus, false),
    // **Off, and the reason is the profile.** WebView2 defaults general autofill
    // on, and this host runs a *persistent* user-data folder —
    // `%LOCALAPPDATA%\Folio\WebView2`, `bt_app::webhost::user_data_folder` — so a
    // default left alone would file names, addresses, phone numbers and card
    // details typed into a previewed page into a profile directory on disk that
    // outlives the window, the session and the reason the page was open. A
    // terminal that shows a page is not a browser somebody chose to trust with
    // that, and the preview has no verb for reviewing or clearing it. Off is the
    // only value this host can honestly ship.
    (WebSetting::GeneralAutofill, false),
    // **Off, and set rather than inherited.** Password autosave's default has
    // moved between runtime versions, and "whatever the engine on this machine
    // happens to think" is not a privacy answer a release note can print. The
    // same profile argument applies and applies harder: the one thing that must
    // never end up in a directory this product creates behind a preview pane is
    // somebody's password.
    (WebSetting::PasswordAutosave, false),
    // **On, and said rather than inherited** (R1-21). The seat is a browser as
    // well as a viewer — a dev server's page is script from top to bottom — so
    // this is not the switch that answers the dialog problem. It is here
    // because it was the one switch in this family nobody had decided: the
    // engine's default is on, this host wants it on, and a value that matches
    // the default by accident is a value that moves when the default does.
    (WebSetting::Script, true),
    // **Off, and this is the one that answers it** (R1-21). WebView2's default
    // is the browser's: `alert()` opens a modal window of the engine's own,
    // and a page in a loop opens another the moment it is dismissed — which is
    // a page holding a pane of this window shut. With this off the engine
    // raises `ScriptDialogOpening` instead and the host answers, which it does
    // without blocking anything: the dialog is dismissed and the seat's foot
    // says a message was turned away. See `WebEvent::ScriptDialogDismissed`.
    (WebSetting::DefaultScriptDialogs, false),
];

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
    pub window: NativeWindow,
}

/// Everything the undo needs, read **before** the handoff touches anything.
///
/// Read and not recomputed: the bounds and the visibility being put back are the
/// ones the page actually had, and asking the controller for them after the walk
/// has started would be asking a half-moved object about a state it is no longer
/// in.
#[cfg(windows)]
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
#[cfg(windows)]
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
    /// **The same caller's policy, asked about everything a document names**
    /// (R1-10) — a picture, a stylesheet, a script, a font, a frame.
    ///
    /// A second closure and not the first one reused, because the two questions
    /// have different answers and only one of them can be redirected: a
    /// navigation the policy rewrites is cancelled and started again, and a
    /// subresource has nobody to restart it. Both run synchronously inside
    /// their callback for `gate`'s reason — neither `SetCancel` nor
    /// `SetResponse` can be decided later.
    request_gate: Box<dyn Fn(&str) -> WebRequestVerdict>,
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

#[cfg(windows)]
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
#[cfg(windows)]
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
    /// **The generation the slot was opened for, beside it** (R2-13).
    ///
    /// One slot and no generation was a slot that answered whoever asked. The
    /// callback cannot be cancelled, so a seat that was closed and asked again
    /// has two of them in flight, and an `install` for the second generation
    /// would take the first one's controller — a live browser pointed at a
    /// window that had already let go of it. The generation is asked for by
    /// name now, and a slot that does not carry it is not adopted.
    pending_controller: Option<(u64, Rc<RefCell<Option<ICoreWebView2CompositionController>>>)>,
    /// **The environment these handlers were put on, and their two tokens**
    /// (R2-10).
    ///
    /// Kept rather than discarded because the environment is process-wide and
    /// the handlers are not: every controller ever installed added two more of
    /// them, each holding an `Rc<Shared>` of a host that may be long closed, and
    /// nothing ever took one off. So the environment is held beside the tokens,
    /// because `remove_` is a call on the object the handler was added to and
    /// the cached one may have moved on by the time this host closes.
    environment_events: Option<(ICoreWebView2Environment, i64, i64)>,
    /// Whether the find session's two counter events have been attached.
    ///
    /// `ICoreWebView2::Find` hands back the same session object every time, so
    /// subscribing on each call would stack a fresh handler per keystroke and
    /// report one count several times over. A `Cell` and not a plain `bool`
    /// because the attaching happens behind `&self`, as every other verb here
    /// does.
    ///
    /// **Cleared on close** ([`CloseStep::FindLatch`], R2-24). The session
    /// belongs to the controller, so a rebuilt seat has a new one with nothing
    /// subscribed to it — and a latch left standing said otherwise, which is a
    /// search capsule whose match count stopped moving for the rest of the
    /// session.
    find_attached: std::cell::Cell<bool>,
}

#[cfg(windows)]
impl WebHost {
    /// A host that has not started anything yet.
    ///
    /// `gate` is asked, synchronously inside `NavigationStarting`, what to do
    /// with a URI, and `request_gate` is asked the same way inside
    /// `FrameNavigationStarting` and `WebResourceRequested` about everything a
    /// document names. `wake` is called after every event is queued and must get
    /// the event loop to call [`WebHost::drain`] — a callback that arrives while
    /// the window is idle would otherwise sit unread until somebody moved the
    /// mouse.
    pub fn new(
        gate: Box<dyn Fn(&str) -> WebNavigationVerdict>,
        request_gate: Box<dyn Fn(&str) -> WebRequestVerdict>,
        wake: Box<dyn Fn()>,
    ) -> Self {
        Self {
            shared: Rc::new(Shared {
                events: RefCell::new(VecDeque::new()),
                chords: RefCell::new(Vec::new()),
                gate,
                request_gate,
                rewriting_to: RefCell::new(None),
                wake,
            }),
            controller: None,
            composition: None,
            webview: None,
            environment: None,
            pending_controller: None,
            environment_events: None,
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
        // **One browser argument, and it is about a gesture this host already
        // has** (user ruling 2026-08-27, route A; `docs/DESIGN.md` §7.23 ⑩).
        //
        // Chromium's desktop default is `document-user-activation-required`: a
        // page may not start audible playback until somebody has interacted with
        // *it*. Measured on this build, that is exactly what happened — the
        // player shell came up with the engine's own controls and the recording
        // paused on its first frame, because the press that asked for it landed
        // on **this window's** play button and a page cannot see a press its
        // host received. A reader who has already said "play" being asked to say
        // it again, in a second control they did not know was there, is the
        // gesture arriving nowhere.
        //
        // **What this widens, exactly.** The policy governs pages this window
        // opens, which are local files a reader chose out of the files column
        // and the shells this process wrote itself; there is no third party
        // here, no ad frame and no site that arrived by itself. It is a
        // *policy*, not a capability: nothing that could not already play can
        // play, and the four switches that matter — no message bridge, no host
        // objects, no default context menus, every download cancelled — are
        // untouched, as is the navigation gate every URL still passes.
        //
        // Written as `AdditionalBrowserArguments` and not as a per-page setting
        // because the runtime has no per-page spelling of it: it is a command
        // line to the browser process, and the browser process is made once.
        // **And it writes none** (route B slice ②, 2026-08-28; `docs/DESIGN.md`
        // §7.44 ④). The one argument that was ever here was Chromium's
        // `autoplay-policy` switch, set to `no-user-gesture-required` — spelled
        // in pieces because the pin that keeps it gone reads this whole file and
        // a sentence about a retired flag must not look like the flag — and it
        // existed for one reason: a shell page this window wrote carried a
        // self-starting player, and
        // Chromium's policy would not start it because the gesture that asked
        // for it happened on a button the page could not see. There is no shell
        // page any more — a recording is decoded by Media Foundation and drawn
        // on this window's own glass — so the argument has nothing left to
        // permit, and a command line kept "in case" is a command line nobody
        // re-reads.
        let options: ICoreWebView2EnvironmentOptions =
            CoreWebView2EnvironmentOptions::default().into();
        unsafe {
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                PCWSTR(folder.as_ptr()),
                Some(&options),
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
        window: NativeWindow,
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
        // **The generation the slot is opened for** (R2-13). A slot already
        // standing here belongs to an attempt this one supersedes, and the
        // controller it may yet receive is one nobody will come for — so it is
        // closed rather than dropped, which is the same rule
        // [`Self::close_pending_controller`] states for the caller's side of it.
        self.close_pending_controller();
        self.pending_controller = Some((generation, holder));
        let hwnd = window.as_hwnd();
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
    /// `generation` is the one the caller's state machine is currently on, and
    /// the slot is adopted only when it was opened for that one (R2-13).
    ///
    /// # Nothing half-installed survives (R2-16)
    ///
    /// Every step after the controller is taken can fail, and each of them used
    /// to leave the controller and its page standing on this host: a browser
    /// process with no gates on it, kept by the very object that failed to put
    /// gates on it. So the walk is [`INSTALL_SEQUENCE`], the undo is
    /// [`install_rollback`], and a failure closes what it made before it
    /// answers.
    pub fn install(
        &mut self,
        compositor: &Compositor,
        page: PageVisual,
        generation: u64,
    ) -> Result<WebInstallReport, String> {
        let mut guards = WebGuards::none();
        let mut unapplied = Vec::new();
        let mut failure_at = None;
        for step in INSTALL_SEQUENCE {
            let done = match step {
                InstallStep::TakeController => self.take_the_controller(generation),
                InstallStep::Configure => self.configure().map(|could_not| unapplied = could_not),
                InstallStep::AttachEvents => self.attach_events(&mut guards),
                InstallStep::AttachEnvironmentEvents => self.attach_environment_events(),
                InstallStep::PointAtVisual => compositor
                    .web_visual(page)
                    .ok_or_else(|| String::from("this page has no web visual to render into"))
                    .and_then(|visual| {
                        unsafe { self.composition().SetRootVisualTarget(&visual) }
                            .map_err(|error| failure("SetRootVisualTarget", &error))
                    }),
            };
            if let Err(error) = done {
                failure_at = Some((step, error));
                break;
            }
        }
        let Some((failed_at, error)) = failure_at else {
            // **The handler is only half of that guard.** With the engine's own
            // dialogs still on, the handler is raised and the window opens
            // anyway, so a build that would not take the switch does not have
            // this gate however well the subscription went.
            guards.script_dialogs &= !unapplied.contains(&WebSetting::DefaultScriptDialogs);
            return Ok(WebInstallReport { guards, unapplied });
        };
        let owed = install_rollback(failed_at);
        if owed.environment_events {
            self.take_the_environment_events_off();
        }
        if owed.controller {
            self.close_the_controller();
        }
        Err(error)
    }

    /// Take the controller the callback left for `generation`, and read the two
    /// interfaces this host works through off it.
    fn take_the_controller(&mut self, generation: u64) -> Result<(), String> {
        let (opened_for, pending) = self
            .pending_controller
            .take()
            .ok_or_else(|| String::from("no controller callback has been answered"))?;
        if opened_for != generation {
            // The slot belongs to an attempt this seat has moved on from. It is
            // closed rather than adopted, for the reason it would have been
            // closed had the caller's own machine caught it: a controller
            // nobody points at is a browser process nobody points at.
            self.pending_controller = Some((opened_for, pending));
            self.close_pending_controller();
            return Err(format!(
                "the controller that answered was asked for by generation {opened_for}, not {generation}"
            ));
        }
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
        Ok(())
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
            hwnd: from.window.as_hwnd(),
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
                    unsafe { controller.SetParentWindow(to.window.as_hwnd()) }
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

    /// Walk [`WEB_SETTINGS`], answering with the switches this build would not
    /// take.
    ///
    /// **Each through the lowest interface that carries it** (R2-16). What
    /// stood here was one `ICoreWebView2Settings4` cast in front of the whole
    /// loop, so a runtime without that interface applied not one of the seven —
    /// including the two that shut the page's channel to the host, which live
    /// on the base interface every runtime has had since the first one. The
    /// cast is per row now, and what a refusal costs is the row's own
    /// [`WebSettingRule`]: `Required` is an error and this seat has no engine,
    /// and anything else is recorded and said.
    fn configure(&self) -> Result<Vec<WebSetting>, String> {
        let Some(webview) = self.webview.as_ref() else {
            return Ok(Vec::new());
        };
        let controller = self
            .controller
            .as_ref()
            .expect("a controller beside the webview");
        let mut unapplied = Vec::new();
        unsafe {
            let settings = webview
                .Settings()
                .map_err(|error| failure("ICoreWebView2::Settings", &error))?;
            for (setting, value) in WEB_SETTINGS {
                let applied = match setting {
                    WebSetting::WebMessage => settings.SetIsWebMessageEnabled(value),
                    WebSetting::HostObjects => settings.SetAreHostObjectsAllowed(value),
                    WebSetting::StatusBar => settings.SetIsStatusBarEnabled(value),
                    WebSetting::DevTools => settings.SetAreDevToolsEnabled(value),
                    WebSetting::DefaultContextMenus => {
                        settings.SetAreDefaultContextMenusEnabled(value)
                    }
                    WebSetting::Script => settings.SetIsScriptEnabled(value),
                    WebSetting::DefaultScriptDialogs => {
                        settings.SetAreDefaultScriptDialogsEnabled(value)
                    }
                    WebSetting::GeneralAutofill | WebSetting::PasswordAutosave => settings
                        .cast::<ICoreWebView2Settings4>()
                        .and_then(|settings4| match setting {
                            WebSetting::GeneralAutofill => {
                                settings4.SetIsGeneralAutofillEnabled(value)
                            }
                            _ => settings4.SetIsPasswordAutosaveEnabled(value),
                        }),
                };
                if let Err(error) = applied {
                    match setting.rule() {
                        WebSettingRule::Required => {
                            return Err(failure(setting.api(), &error));
                        }
                        WebSettingRule::Guard | WebSettingRule::Preference => {
                            unapplied.push(setting);
                        }
                    }
                }
            }
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
        Ok(unapplied)
    }

    #[allow(clippy::too_many_lines)]
    fn attach_events(&self, guards: &mut WebGuards) -> Result<(), String> {
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

            // ── the two doors the main frame's was standing in for (R1-10) ──
            //
            // `NavigationStarting` is asked about the document and about
            // nothing the document contains, so a previewed local page naming
            // `<iframe src="file:///C:/…">` or `<img src="…">` outside its own
            // folder was fetched without anybody being asked. A frame is a
            // navigation of its own and everything else is a request, so there
            // are two of them, and both run the caller's `request_gate` — the
            // same rule, asked about the same seat, in the two shapes the
            // engine offers.
            let shared = Rc::clone(&self.shared);
            webview
                .add_FrameNavigationStarting(
                    &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let uri = read_string(|out| args.Uri(out));
                        if matches!((shared.request_gate)(&uri), WebRequestVerdict::Allow) {
                            return Ok(());
                        }
                        // `SetCancel` cannot be decided later, exactly as it
                        // cannot in the main frame's handler above.
                        args.SetCancel(true)?;
                        shared.push(WebEvent::RequestRefused { uri });
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_FrameNavigationStarting", &error))?;
            guards.frame_navigation = true;

            // The environment is what a refusal is *made of*: blocking a
            // request means answering it, and the answer is an empty 403 the
            // environment mints. A clone rather than a borrow because the
            // handler outlives this call, and it is let go when the handler is
            // taken off — which is [`CloseStep::Controller`]'s doing, since the
            // handler belongs to the controller being closed.
            let environment = self
                .environment
                .clone()
                .ok_or_else(|| String::from("no environment to answer a refused request with"))?;
            let shared = Rc::clone(&self.shared);
            webview
                .add_WebResourceRequested(
                    &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let uri = match args.Request() {
                            Ok(request) => read_string(|out| request.Uri(out)),
                            // A request whose own address cannot be read is a
                            // request nothing can be decided about, and the
                            // empty string is refused by every arm of the rule.
                            Err(_) => String::new(),
                        };
                        if matches!((shared.request_gate)(&uri), WebRequestVerdict::Allow) {
                            return Ok(());
                        }
                        // **Answered, not merely dropped.** A handler that
                        // returned without setting a response lets the engine
                        // go and fetch it; the empty 403 is what makes the
                        // refusal the whole of what happens, and it is the same
                        // answer the document would get from a server that
                        // refused it, which is a case every loader already
                        // handles.
                        let response = environment.CreateWebResourceResponse(
                            None,
                            403,
                            &HSTRING::from("Blocked"),
                            &HSTRING::from(""),
                        )?;
                        args.SetResponse(&response)?;
                        shared.push(WebEvent::RequestRefused { uri });
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_WebResourceRequested", &error))?;
            // **Every address, in every context.** A filter narrower than this
            // is a list of the request kinds somebody thought of, and the kinds
            // nobody thought of are exactly the ones a document would be built
            // out of to get past it.
            webview
                .AddWebResourceRequestedFilter(
                    &HSTRING::from("*"),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                )
                .map_err(|error| failure("AddWebResourceRequestedFilter", &error))?;
            // And, where the runtime carries it, the same filter over the
            // sources that are not the document at all — a service worker or a
            // shared worker fetching on the page's behalf. Best effort: a build
            // without `_22` has no such sources to filter, so its absence is
            // not a gate that failed.
            if let Ok(sources) = webview.cast::<ICoreWebView2_22>() {
                sources
                    .AddWebResourceRequestedFilterWithRequestSourceKinds(
                        &HSTRING::from("*"),
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                        COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                    )
                    .map_err(|error| {
                        failure(
                            "AddWebResourceRequestedFilterWithRequestSourceKinds",
                            &error,
                        )
                    })?;
            }
            guards.resource_requests = true;

            // **The page's own modal windows, answered rather than opened**
            // (R1-21). `AreDefaultScriptDialogsEnabled` is off in
            // [`WEB_SETTINGS`], so the engine raises this instead of putting a
            // window on the screen, and this host answers it the only way a
            // window with a message pump of its own can: immediately, without a
            // deferral, by not accepting. A page in a loop gets its `alert`
            // back on the spot every time and never holds the seat; what the
            // reader gets is one line on the pane's foot saying a message was
            // turned away, which is the seat's own band and costs no press.
            let shared = Rc::clone(&self.shared);
            webview
                .add_ScriptDialogOpening(
                    &ScriptDialogOpeningEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let kind = read::<COREWEBVIEW2_SCRIPT_DIALOG_KIND>(|out| args.Kind(out)).0;
                        shared.push(WebEvent::ScriptDialogDismissed { kind });
                        Ok(())
                    })),
                    &mut token,
                )
                .map_err(|error| failure("add_ScriptDialogOpening", &error))?;
            guards.script_dialogs = true;

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

            // **Whether this document is making a sound** (user ruling
            // 2026-08-27; `docs/DESIGN.md` §7.23 ⑩). A tab switched away from
            // is hidden and not stopped — `SetIsVisible(false)` makes the page
            // stop *rendering* and says nothing about its audio — so a video
            // goes on playing behind a tab nobody is looking at, which is the
            // browser convention the ruling adopted. What it owes the reader in
            // exchange is a way to find the sound, and this is the only signal
            // in the engine that tells the truth about one: not "a video was
            // opened" but "this document is audible now".
            //
            // Cast rather than assumed: `ICoreWebView2_8` has been in the
            // runtime since 1.0.1072.54, and a machine older than that gets a
            // window with no speaker on its tabs rather than no window at all.
            let shared = Rc::clone(&self.shared);
            if let Ok(audio) = webview.cast::<ICoreWebView2_8>() {
                audio
                    .add_IsDocumentPlayingAudioChanged(
                        &IsDocumentPlayingAudioChangedEventHandler::create(Box::new(
                            move |view, _| {
                                // The event says only *that* it changed, so the
                                // level is read back off the same interface —
                                // the shape `StatusBarTextChanged` above uses,
                                // and for its reason: the handler is handed the
                                // base interface and the fact lives on the
                                // versioned one.
                                let playing = match view
                                    .as_ref()
                                    .and_then(|view| view.cast::<ICoreWebView2_8>().ok())
                                {
                                    Some(view8) => {
                                        read_bool(|out| view8.IsDocumentPlayingAudio(out))
                                    }
                                    None => false,
                                };
                                shared.push(WebEvent::PlayingAudioChanged { playing });
                                Ok(())
                            },
                        )),
                        &mut token,
                    )
                    .map_err(|error| failure("add_IsDocumentPlayingAudioChanged", &error))?;
            }

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
                            // Win32's accelerator callback has no Command
                            // modifier to report — see `WebChord`'s note.
                            command: false,
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
    fn attach_environment_events(&mut self) -> Result<(), String> {
        let environment = self
            .environment
            .clone()
            .ok_or_else(|| String::from("no environment to attach to"))?;
        let mut token = 0i64;
        // **Both tokens are kept** (R2-10). The environment is the process's,
        // not this host's, and a handler added to it outlives every controller
        // — so a token that was written into a scratch variable and forgotten
        // was a subscription nobody could ever take off, holding this host's
        // whole event queue alive on a shared object for as long as the program
        // ran.
        let (browser_exited, new_version);
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
            browser_exited = token;

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
            new_version = token;
        }
        self.environment_events = Some((environment, browser_exited, new_version));
        Ok(())
    }

    /// Take this host's two environment handlers off the environment they were
    /// put on (R2-10).
    ///
    /// The environment they were added to and not the cached one: a rebuild
    /// replaces the cache, and `remove_` on an object the handler was never
    /// added to removes nothing.
    fn take_the_environment_events_off(&mut self) {
        let Some((environment, browser_exited, new_version)) = self.environment_events.take()
        else {
            return;
        };
        unsafe {
            if let Ok(environment5) = environment.cast::<ICoreWebView2Environment5>() {
                let _ = environment5.remove_BrowserProcessExited(browser_exited);
            }
            let _ = environment.remove_NewBrowserVersionAvailable(new_version);
        }
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
        let Some((_, pending)) = self.pending_controller.take() else {
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
    ///
    /// **And let go of everything else install created** ([`WEB_CLOSE_STEPS`]).
    /// What stood here closed the controller and dropped two interfaces, which
    /// is three of the seven things a live seat holds; the other four were each
    /// a defect of their own, and each of them is a row of that table now
    /// rather than a line somebody has to remember to write.
    pub fn close(&mut self) {
        for step in WEB_CLOSE_STEPS {
            match step {
                CloseStep::Controller => self.close_the_controller(),
                // Closing the controller already dropped these two; the rows
                // exist because the *set* is what has to be right, and a
                // controller that refused to close must not leave them behind.
                CloseStep::Composition => self.composition = None,
                CloseStep::Webview => self.webview = None,
                CloseStep::EnvironmentEvents => self.take_the_environment_events_off(),
                CloseStep::CachedEnvironment => self.environment = None,
                CloseStep::PendingController => self.close_pending_controller(),
                CloseStep::FindLatch => self.find_attached.set(false),
            }
        }
    }

    /// The controller alone, closed and let go of.
    ///
    /// Shared by [`Self::close`] and by install's own rollback, which owes the
    /// controller and nothing else: a host whose install failed has no
    /// environment subscriptions to take off except the ones
    /// [`install_rollback`] names.
    fn close_the_controller(&mut self) {
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
#[cfg(windows)]
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
#[cfg(windows)]
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
#[cfg(windows)]
fn bounds_rect(x: i32, y: i32, width: u32, height: u32) -> RECT {
    RECT {
        left: x,
        top: y,
        right: x + width as i32,
        bottom: y + height as i32,
    }
}

/// **The rectangle the engine is given carries the seat's origin.**
#[cfg(all(test, windows))]
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
/// **What this host decides for the engine, held without a browser.**
///
/// The set is the policy: a WebView2 default this host does not overrule is a
/// decision nobody made, and it is not a thing a COM call can be asked about
/// afterwards. So it is a value here, [`WebHost::configure`] walks it, and this
/// holds it.
#[cfg(all(test, windows))]
mod engine_settings_tests {
    use super::*;

    /// RED — **the switch set, and the two that were missing from it.**
    ///
    /// Release audit 2026-08-27 (Codex 漏 10): `rg 'IsGeneralAutofillEnabled|IsPasswordAutosaveEnabled'`
    /// over this file returned nothing, so a persistent profile under
    /// `%LOCALAPPDATA%\Folio\WebView2` was saving form data and whatever password
    /// autosave defaulted to on that machine. Both are now decided here.
    ///
    /// MUTATIONS: drop either autofill row and this fails; flip `DevTools` off
    /// and the `Developer tools` verb becomes a button that does nothing; flip
    /// `WebMessage` or `HostObjects` on and slice ①'s "hosts a page and offers it
    /// nothing" stops being true. Every one of those is a change somebody should
    /// have to type twice.
    #[test]
    fn the_engine_is_configured_with_exactly_these_switches() {
        assert_eq!(
            WEB_SETTINGS,
            [
                (WebSetting::WebMessage, false),
                (WebSetting::HostObjects, false),
                (WebSetting::StatusBar, false),
                (WebSetting::DevTools, true),
                (WebSetting::DefaultContextMenus, false),
                (WebSetting::GeneralAutofill, false),
                (WebSetting::PasswordAutosave, false),
                (WebSetting::Script, true),
                (WebSetting::DefaultScriptDialogs, false),
            ]
        );
    }

    /// RED — **a page cannot hold the seat with its own dialogs** (R1-21).
    ///
    /// The table set neither the script switch nor the dialog one, so both were
    /// whatever the engine on the machine happened to default to — and the
    /// engine's default for dialogs is the browser's, which is a modal window
    /// the page opens and can open again the moment it is dismissed. Script
    /// stays on, because the seat is a browser as well as a viewer; the dialogs
    /// go, and the host answers them instead.
    ///
    /// RED GATE: take either row out of [`WEB_SETTINGS`] and this fails, which
    /// on the machine is `while (true) alert('')` in a previewed page.
    #[test]
    fn script_stays_on_and_the_engines_own_dialogs_do_not() {
        assert!(
            WEB_SETTINGS.contains(&(WebSetting::Script, true)),
            "a seat that is also a browser runs the page's script"
        );
        assert!(
            WEB_SETTINGS.contains(&(WebSetting::DefaultScriptDialogs, false)),
            "and answers the page's dialogs itself rather than letting it open one"
        );
        // The switch alone is only half of it: with the engine's dialogs off and
        // nobody answering `ScriptDialogOpening`, a page's `alert` never
        // returns. So the guard names both.
        assert_eq!(
            WebSetting::DefaultScriptDialogs.rule(),
            WebSettingRule::Guard,
            "a build that will not take it opens no local file"
        );
    }

    /// RED — **every switch is set through the lowest interface that carries
    /// it** (R2-16).
    ///
    /// The loop stood inside one `ICoreWebView2Settings4` cast, and the cast
    /// failing was the whole table failing: a runtime without that interface
    /// applied not one of the seven, including the two that shut the page's
    /// channel to the host and that have lived on the base interface since the
    /// first runtime there was.
    ///
    /// RED GATE: answer `"ICoreWebView2Settings4"` for every row — which is
    /// what the code did — and the first assertion fails for seven of the nine.
    #[test]
    fn every_switch_names_the_lowest_interface_that_carries_it() {
        for (setting, _) in WEB_SETTINGS {
            let expected = match setting {
                WebSetting::GeneralAutofill | WebSetting::PasswordAutosave => {
                    "ICoreWebView2Settings4"
                }
                _ => "ICoreWebView2Settings",
            };
            assert_eq!(setting.interface(), expected, "{}", setting.api());
        }
        // And what a refusal costs is a decision per row rather than one for the
        // table: nothing that files what a person typed is ever a preference.
        for (setting, _) in WEB_SETTINGS {
            if matches!(
                setting,
                WebSetting::WebMessage
                    | WebSetting::HostObjects
                    | WebSetting::GeneralAutofill
                    | WebSetting::PasswordAutosave
            ) {
                assert_eq!(
                    setting.rule(),
                    WebSettingRule::Required,
                    "{} is not a switch a seat may go without",
                    setting.api()
                );
            }
        }
    }

    /// RED — **nothing that saves what a person typed is left to a default.**
    ///
    /// The switch-set test above pins the whole table and would have to be
    /// re-typed to change any of it; this one says the *reason* out loud, so a
    /// future row added to the table cannot quietly be a saving one.
    #[test]
    fn no_switch_that_would_file_what_a_person_typed_is_on() {
        for (setting, value) in WEB_SETTINGS {
            if matches!(
                setting,
                WebSetting::GeneralAutofill | WebSetting::PasswordAutosave
            ) {
                assert!(
                    !value,
                    "{} would save into the persistent profile directory",
                    setting.api()
                );
            }
        }
    }

    /// Every switch names the engine method it is set through, and no two name
    /// the same one — a failure that could not say which call refused would be a
    /// failure nobody could act on.
    #[test]
    fn every_switch_names_one_engine_method_of_its_own() {
        let mut named: Vec<&str> = WEB_SETTINGS
            .iter()
            .map(|(setting, _)| setting.api())
            .collect();
        assert!(named.iter().all(|api| api.starts_with("Set")));
        named.sort_unstable();
        let count = named.len();
        named.dedup();
        assert_eq!(named.len(), count, "two switches share one method name");
    }
}

/// **The one thing a rule about strings cannot say: that the engine asks it at
/// all** (R1-10).
///
/// Every other test of the resource door is a function of two strings, which is
/// the right shape for a rule and the wrong shape for the finding. What the
/// review actually found was that `NavigationStarting` is raised for the
/// document and for nothing the document contains — so the question this cannot
/// be answered without a runtime is whether an `<img>` and an `<iframe>` in a
/// local page reach a gate at all.
///
/// # It is a probe, and it is `#[ignore]`d for the reason the others are
///
/// It needs a WebView2 runtime, a window and a message pump, which is a
/// question with a different right answer on every machine —
/// `scripts/ci/ignored-tests.txt` states that policy and carries this name. Run
/// it with `cargo test -p bt-platform -- --ignored --nocapture`.
///
/// # It measures the before and the after in one run
///
/// The first pass opens the page behind a gate that allows everything, which is
/// exactly what a seat with no resource door had, and the page reports that the
/// picture outside its folder **loaded**. The second opens the same page behind
/// the folder rule and the same picture reports that it did **not**. Nothing
/// here reaches a share: the stand-in for `\\attacker\share` is a second
/// temporary folder on this disk, because a test that actually ran would be a
/// machine reaching for somebody else's server.
///
/// # It takes nothing from the person running it
///
/// The window is `WS_POPUP` and is never shown, so the foreground is never
/// taken; the profile is a temporary folder of this test's own, so nothing of
/// the user's is read or written; and both windows are destroyed and both
/// folders removed however the run ends.
#[cfg(all(test, windows))]
mod webview2_runtime_probe {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE,
        PeekMessageW, RegisterClassW, TranslateMessage, WNDCLASSW, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_POPUP,
    };
    use windows::core::{HSTRING, PCWSTR};

    use super::*;
    use crate::{Compositor, PageVisual};

    /// A one-pixel PNG, so that a picture inside the folder has something real
    /// to load and `onload` is a fact rather than a hope.
    const ONE_PIXEL_PNG: [u8; 67] = [
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// How long any one step of the probe is allowed to say nothing.
    const STEP: Duration = Duration::from_secs(30);

    /// A window that exists and is never shown.
    ///
    /// `WS_POPUP` with no `ShowWindow` never appears and never takes the
    /// foreground, which is the whole of what a controller needs a parent for:
    /// a place to hang its own popups off and a handle to be reparented under.
    struct HiddenWindow(HWND);

    impl HiddenWindow {
        fn open() -> Self {
            let class = HSTRING::from("FolioWebResourceProbe");
            let wnd = WNDCLASSW {
                lpfnWndProc: Some(procedure),
                lpszClassName: PCWSTR(class.as_ptr()),
                ..Default::default()
            };
            // A second registration of one class name answers zero and is not an
            // error worth stopping for: the class from the first pass is still
            // registered and is the one that will be used.
            unsafe { RegisterClassW(&wnd) };
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                    PCWSTR(class.as_ptr()),
                    PCWSTR(HSTRING::from("").as_ptr()),
                    WS_POPUP,
                    0,
                    0,
                    800,
                    600,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .expect("a window for the probe to host a page in");
            Self(hwnd)
        }

        fn key(&self) -> crate::NativeWindow {
            crate::NativeWindow::from_hwnd(self.0).expect("a real window handle")
        }
    }

    impl Drop for HiddenWindow {
        fn drop(&mut self) {
            let _ = unsafe { DestroyWindow(self.0) };
        }
    }

    unsafe extern "system" fn procedure(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, message, w, l) }
    }

    /// A directory of this test's own, removed however the run ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn make(tag: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("folio-web-probe-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("a scratch directory");
            Self(root)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The rule the second and third passes run: the folder the page was opened
    /// in, and the engine's own parts. `folder` is a lower-cased `file:` URL
    /// ending in a slash.
    ///
    /// Deliberately not `bt_app::webnav::resource_request` — that lives in the
    /// crate above this one, and what this probe is measuring is not the rule
    /// but whether the engine asks anybody at all.
    fn inside_the_folder(candidate: &str, folder: &str) -> bool {
        let candidate = candidate.to_lowercase();
        if candidate.starts_with("file:") {
            return candidate.starts_with(folder);
        }
        !candidate.starts_with("http:") && !candidate.starts_with("https:")
    }

    /// The `file:` URL of a path, spelled the one way this product spells one.
    fn file_url(path: &Path) -> String {
        format!("file:///{}", path.display().to_string().replace('\\', "/"))
    }

    /// Turn the pump until `done` answers or the step's budget runs out.
    ///
    /// The pump is the probe's, not winit's: WebView2 delivers every callback on
    /// the thread that made the environment and delivers none of them to a
    /// thread that is not pumping.
    fn pump_until(host: &WebHost, seen: &mut Vec<WebEvent>, done: impl Fn(&[WebEvent]) -> bool) {
        let deadline = Instant::now() + STEP;
        loop {
            turn_the_pump();
            seen.extend(host.drain());
            if done(seen) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the engine said nothing for {STEP:?}; what it had said was {seen:#?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Turn the pump for a fixed while, for the answers that arrive after the
    /// one that was waited for — a picture's `onerror` lands after the
    /// document's own navigation has completed.
    fn pump_for(host: &WebHost, seen: &mut Vec<WebEvent>, span: Duration) {
        let until = Instant::now() + span;
        while Instant::now() < until {
            turn_the_pump();
            seen.extend(host.drain());
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Dispatch whatever this thread has waiting and nothing else.
    ///
    /// Its own function because the teardown needs it with no host to drain:
    /// a controller that has been closed goes on posting to this thread for a
    /// moment, and a thread that stopped dispatching would tear its apartment
    /// down underneath the engine.
    fn turn_the_pump() {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            let _ = unsafe { TranslateMessage(&message) };
            unsafe { DispatchMessageW(&message) };
        }
    }

    /// Give the engine a moment to finish what closing started.
    fn settle() {
        let until = Instant::now() + Duration::from_millis(600);
        while Instant::now() < until {
            turn_the_pump();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The last title the document gave itself, which is where the page writes
    /// down what loaded and what did not.
    fn title(seen: &[WebEvent]) -> String {
        seen.iter()
            .rev()
            .find_map(|event| match event {
                WebEvent::DocumentTitleChanged { title } => Some(title.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// What one pass of the probe measured.
    struct Pass {
        /// The last name the document gave itself, which is where the page
        /// writes down what loaded and what did not.
        title: String,
        /// Whether the document itself committed.
        loaded: bool,
        /// Every address the gate was asked about, and what it answered.
        asked: Vec<(String, bool)>,
        guards: WebGuards,
    }

    /// One pass: open the page behind `allow` and answer with what the document
    /// said about itself and what the gate was asked.
    fn open_the_page(
        profile: &Path,
        page_url: &str,
        allow: impl Fn(&str) -> bool + 'static,
    ) -> Pass {
        let window = HiddenWindow::open();
        let asked: Rc<RefCell<Vec<(String, bool)>>> = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&asked);
        let mut host = WebHost::new(
            Box::new(|_| WebNavigationVerdict::Proceed),
            Box::new(move |candidate| {
                let allowed = allow(candidate);
                log.borrow_mut().push((candidate.to_owned(), allowed));
                if allowed {
                    WebRequestVerdict::Allow
                } else {
                    WebRequestVerdict::Refuse
                }
            }),
            Box::new(|| {}),
        );
        let mut seen = Vec::new();
        host.request_environment(profile, 1)
            .expect("the environment was asked for");
        pump_until(&host, &mut seen, |events| {
            events
                .iter()
                .any(|event| matches!(event, WebEvent::Environment { .. }))
        });
        host.request_controller(window.key(), 1)
            .expect("the controller was asked for");
        pump_until(&host, &mut seen, |events| {
            events
                .iter()
                .any(|event| matches!(event, WebEvent::Controller { .. }))
        });

        let compositor = Compositor::new(window.key()).expect("a composition tree");
        let page = PageVisual { tab: 1, seat: 1 };
        compositor.attach_web_visual(page).expect("a web visual");
        let report = host
            .install(&compositor, page, 1)
            .expect("the controller was taken into service");
        host.set_bounds(0, 0, 800, 600).expect("bounds");
        host.set_visible(true).expect("visible");
        compositor.commit().expect("a commit");
        host.navigate(page_url).expect("a navigation");
        pump_until(&host, &mut seen, |events| {
            events
                .iter()
                .any(|event| matches!(event, WebEvent::NavigationCompleted { .. }))
        });
        // The document's own verdicts on its pictures arrive after its
        // navigation has completed, so the pump is turned for a while longer
        // rather than for another event.
        pump_for(&host, &mut seen, Duration::from_secs(2));
        host.close();
        // The controller goes on posting to this thread for a moment after it
        // is closed, and a window destroyed under those messages is where a
        // probe crashes instead of failing.
        settle();
        let asked = asked.borrow().clone();
        Pass {
            title: title(&seen),
            loaded: seen.iter().any(
                |event| matches!(event, WebEvent::NavigationCompleted { success, .. } if *success),
            ),
            asked,
            guards: report.guards,
        }
    }

    /// RED — **an `<img>` and an `<iframe>` in a previewed local page reach the
    /// gate, and what they reach outside the page's folder does not load**
    /// (R1-10).
    ///
    /// RED GATE: this test contains its own. The first pass *is* the code before
    /// the ticket — a gate that answers `Allow` to everything is what a seat
    /// with only `NavigationStarting` had — and it asserts that the picture
    /// outside the folder loaded. The second asserts that the same picture, in
    /// the same page, behind the folder rule, did not.
    #[test]
    #[ignore = "needs a WebView2 runtime, a window and a message pump: run it with --ignored"]
    fn a_local_page_reaches_only_its_own_folder() {
        webview2_runtime_version().expect("a WebView2 runtime on this machine");
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .expect("an apartment for the engine's callbacks");

        let scratch = Scratch::make("tree");
        let profile = Scratch::make("profile");
        let inside = scratch.0.join("page");
        let outside = scratch.0.join("outside");
        std::fs::create_dir_all(&inside).expect("the page's folder");
        std::fs::create_dir_all(&outside).expect("a folder the page was not opened in");
        std::fs::write(inside.join("beside.png"), ONE_PIXEL_PNG).expect("a picture beside it");
        // The stand-in for a share: a real path on this disk, outside the tree.
        std::fs::write(outside.join("secret.png"), ONE_PIXEL_PNG).expect("a picture outside");
        std::fs::write(
            outside.join("frame.html"),
            b"<!doctype html><title>f</title>",
        )
        .expect("a document outside");

        let document = format!(
            "<!doctype html><meta charset=\"utf-8\"><title>start</title>\
             <script>var seen=[];function mark(m){{if(seen.indexOf(m)<0){{seen.push(m);seen.sort();document.title=seen.join('|');}}}}</script>\
             <img src=\"beside.png\" onload=\"mark('beside-loaded')\" onerror=\"mark('beside-blocked')\">\
             <img src=\"{outside_png}\" onload=\"mark('outside-loaded')\" onerror=\"mark('outside-blocked')\">\
             <iframe src=\"{outside_html}\"></iframe>",
            outside_png = file_url(&outside.join("secret.png")),
            outside_html = file_url(&outside.join("frame.html")),
        );
        let page_path = inside.join("report.html");
        std::fs::write(&page_path, document).expect("the page");
        let page_url = file_url(&page_path);
        let folder = format!("{}/", file_url(&inside).to_lowercase());

        // ── before: the seat as it stood, with nothing asked ──────────────
        let before = open_the_page(&profile.0, &page_url, |_| true);
        assert!(
            before.guards.all_stand(),
            "a current runtime carries every gate: {:?}",
            before.guards.missing()
        );
        assert!(
            before.title.contains("outside-loaded"),
            "the page's own report was {:?}; before the gate, the picture outside its folder loaded",
            before.title
        );
        assert!(
            before
                .asked
                .iter()
                .any(|(uri, _)| uri.to_lowercase().contains("secret.png")),
            "the picture reached the gate at all, which is the finding: {:#?}",
            before.asked
        );
        assert!(
            before
                .asked
                .iter()
                .any(|(uri, _)| uri.to_lowercase().contains("frame.html")),
            "and so did the frame: {:#?}",
            before.asked
        );

        // ── after: the same page, behind the folder rule ──────────────────
        // The measurement, printed because that is what a probe is for: the
        // same page, the same two folders, with and without the door.
        eprintln!(
            "probe: with no resource door the page reported {:?}",
            before.title
        );
        let inside_only = folder.clone();
        let after = open_the_page(&profile.0, &page_url, move |candidate| {
            inside_the_folder(candidate, &inside_only)
        });
        assert!(
            after.title.contains("outside-blocked"),
            "the page's own report was {:?}; the picture outside its folder must not load",
            after.title
        );
        assert!(
            after.title.contains("beside-loaded"),
            "the page's own report was {:?}; the picture beside it must still load",
            after.title
        );
        eprintln!(
            "probe: behind the folder rule it reported {:?}",
            after.title
        );
        for (uri, allowed) in &after.asked {
            let uri = uri.to_lowercase();
            if uri.contains("secret.png") || uri.contains("frame.html") {
                assert!(!allowed, "{uri} was allowed");
            }
        }

        // ── and the engine's own furniture still stands ───────────────────
        //
        // A `.pdf` opened out of the files column is drawn by a page of the
        // browser's own, served over `chrome-extension:`. A door that gated the
        // document's own contents and forgot that would leave a reader a blank
        // rectangle where their document was, so the case is measured rather
        // than reasoned about.
        let pdf = inside.join("folio-pdf-test.pdf");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets/folio-pdf-test.pdf"),
            &pdf,
        )
        .expect("the repository's own test document");
        let viewer = open_the_page(&profile.0, &file_url(&pdf), move |candidate| {
            inside_the_folder(candidate, &folder)
        });
        assert!(
            viewer.loaded,
            "a local PDF still opens behind the door: {:#?}",
            viewer.asked
        );
        for (uri, allowed) in &viewer.asked {
            assert!(
                *allowed,
                "the engine's own viewer was refused {uri}, which is a blank rectangle where a document was"
            );
        }
        eprintln!(
            "probe: the viewer asked for {} addresses and was refused none",
            viewer.asked.len()
        );

        // **The environment is let go of before the apartment is.** It is
        // cached on this thread, so a thread that ended with it still cached
        // would release a COM object into an apartment that had already gone —
        // which is a crash and not a failure, and says nothing about the rule
        // this probe is here to measure.
        forget_web_environment();
        settle();
    }
}

/// **What install creates and what closing lets go of, held without a browser.**
///
/// Both are sets rather than runs of statements, and a set is exactly the thing
/// a run of statements cannot be asked about afterwards: the four defects this
/// module's tests were written for are each one row that was not there.
#[cfg(all(test, windows))]
mod install_and_close_contract_tests {
    use super::*;

    /// A host with no engine behind it. `WebHost::new` touches no COM at all —
    /// it builds a queue, three closures and a wake — so the whole of this
    /// module's state can be shot at on a machine with no runtime, which is the
    /// same argument `bt_app::webhost::WebMachine` is a separate object for.
    fn a_host() -> WebHost {
        WebHost::new(
            Box::new(|_| WebNavigationVerdict::Proceed),
            Box::new(|_| WebRequestVerdict::Allow),
            Box::new(|| {}),
        )
    }

    /// RED — **closing lets go of everything install created** (R2-10, R2-12,
    /// R2-13, R2-24).
    ///
    /// Four of these seven were missing, and not one of the four is visible in
    /// a run of statements — each is a line somebody did not write. As rows
    /// they are readable, and each names what its absence cost.
    ///
    /// RED GATE: drop any row from [`WEB_CLOSE_STEPS`] and this fails; drop the
    /// arm that performs it from [`WebHost::close`] and the file will not
    /// compile, because the walk is a match over this table and not a run of
    /// statements beside it.
    #[test]
    fn closing_lets_go_of_every_one_of_these() {
        // Compared as slices so that a row taken out of the table is a failing
        // assertion naming the row, and not a type error naming an array
        // length: the point of the test is which step is missing.
        assert_eq!(
            WEB_CLOSE_STEPS.as_slice(),
            [
                CloseStep::Controller,
                CloseStep::Composition,
                CloseStep::Webview,
                // R2-10: two handlers on the process-wide environment, added by
                // every install and removed by nothing.
                CloseStep::EnvironmentEvents,
                // R2-12: the cached environment, which a rebuild adopted — the
                // very environment the rebuild exists to abandon.
                CloseStep::CachedEnvironment,
                // R2-13: the slot the controller callback writes into.
                CloseStep::PendingController,
                // R2-24: the latch that says the find counters are subscribed.
                CloseStep::FindLatch,
            ]
            .as_slice()
        );
    }

    /// RED — **a rebuilt page counts its own search matches again** (R2-24).
    ///
    /// `ICoreWebView2::Find` hands back one session per controller, so the latch
    /// that keeps the counters from being subscribed twice is a statement about
    /// *this* controller. Left standing across a close it was a statement about
    /// a page that is gone, and the seat that came back after a crash or a
    /// runtime update never subscribed at all: the capsule went on showing
    /// whatever number it last heard.
    ///
    /// RED GATE: take [`CloseStep::FindLatch`] out of the walk and this fails.
    #[test]
    fn closing_takes_the_find_latch_off() {
        let mut host = a_host();
        host.find_attached.set(true);
        host.close();
        assert!(
            !host.find_attached.get(),
            "the next controller has a find session of its own and nothing subscribed to it"
        );
    }

    /// RED — **a closed seat leaves no slot for a controller to arrive into**
    /// (R2-13).
    ///
    /// The creation callback cannot be cancelled. A seat closed while one was in
    /// flight kept the slot, so the controller arrived, sat in it, and the next
    /// `install` — a different generation, a different attempt — took it: a live
    /// browser pointed at a window that had already let go of it.
    ///
    /// RED GATE: take [`CloseStep::PendingController`] out of the walk and this
    /// fails.
    #[test]
    fn closing_empties_the_slot_a_controller_would_arrive_into() {
        let mut host = a_host();
        host.pending_controller = Some((7, Rc::new(RefCell::new(None))));
        host.close();
        assert!(
            host.pending_controller.is_none(),
            "a controller that answers now has nowhere to be adopted from"
        );
    }

    /// RED — **and a slot belongs to the generation it was opened for**
    /// (R2-13).
    ///
    /// The other half of the same defect: one slot with no name on it answered
    /// whoever asked. Two attempts can be in flight at once, because the
    /// callback cannot be cancelled — so `install` names the generation it is
    /// installing for, and a slot opened for another one is closed rather than
    /// adopted.
    #[test]
    fn a_controller_asked_for_by_one_generation_is_not_adopted_by_another() {
        let mut host = a_host();
        host.pending_controller = Some((3, Rc::new(RefCell::new(None))));
        let refused = host
            .take_the_controller(4)
            .expect_err("the slot was opened for generation 3");
        assert!(refused.contains('3') && refused.contains('4'), "{refused}");
        assert!(
            host.pending_controller.is_none(),
            "and the slot is emptied rather than left for a third attempt"
        );
        // The generation it *was* opened for still finds it, and fails on the
        // controller having never been delivered rather than on the generation.
        let mut host = a_host();
        host.pending_controller = Some((3, Rc::new(RefCell::new(None))));
        let refused = host
            .take_the_controller(3)
            .expect_err("the callback delivered nothing");
        assert!(refused.contains("delivered no controller"), "{refused}");
    }

    /// RED — **a failed install keeps nothing** (R2-16).
    ///
    /// Every step after the controller is taken can fail, and each of them left
    /// the controller and the page standing on this host: a browser process
    /// with no gates on it, kept by the object that had just failed to put
    /// gates on it, while the caller's state machine sat in `ControllerPending`
    /// beside it.
    ///
    /// RED GATE: answer `InstallRollback::default()` for every step — which is
    /// what keeping everything amounts to — and the four middle assertions
    /// fail.
    #[test]
    fn an_install_that_fails_closes_what_it_had_already_made() {
        // Nothing has been taken yet, so nothing is owed.
        assert_eq!(
            install_rollback(InstallStep::TakeController),
            InstallRollback::default()
        );
        // From the moment the controller is in hand, every later failure closes
        // it.
        for step in [
            InstallStep::Configure,
            InstallStep::AttachEvents,
            InstallStep::AttachEnvironmentEvents,
            InstallStep::PointAtVisual,
        ] {
            assert!(
                install_rollback(step).controller,
                "{step:?} left a live browser nobody points at"
            );
        }
        // And the environment's own handlers come off only once they are on:
        // putting back what was never taken is the other half of a compensation
        // being right.
        assert!(
            !install_rollback(InstallStep::AttachEnvironmentEvents).environment_events,
            "they had not been added yet"
        );
        assert!(
            install_rollback(InstallStep::PointAtVisual).environment_events,
            "a failure after them leaves two more on the process's environment (R2-10)"
        );
    }

    /// RED — **rollback only grows.** A step that created something can never be
    /// dropped from a later step's undo, which is the property that makes
    /// [`INSTALL_SEQUENCE`] safe to extend: a sixth step inherits everything the
    /// five before it made.
    #[test]
    fn every_later_install_failure_takes_back_at_least_what_an_earlier_one_does() {
        let mut previous = InstallRollback::default();
        for step in INSTALL_SEQUENCE {
            let owed = install_rollback(step);
            assert!(
                !previous.controller || owed.controller,
                "{step:?} drops a controller an earlier step owed"
            );
            assert!(
                !previous.environment_events || owed.environment_events,
                "{step:?} drops the environment handlers an earlier step owed"
            );
            previous = owed;
        }
    }

    /// The gates a controller carries are counted rather than assumed, and one
    /// that has none says which — the line of fact under the card that refuses
    /// to open a local file (R2-16).
    #[test]
    fn a_controller_says_which_of_its_gates_it_does_not_have() {
        assert!(!WebGuards::none().all_stand());
        assert_eq!(
            WebGuards::none().missing(),
            [
                "ScriptDialogOpening",
                "FrameNavigationStarting",
                "WebResourceRequested"
            ]
        );
        let all = WebGuards {
            script_dialogs: true,
            frame_navigation: true,
            resource_requests: true,
        };
        assert!(all.all_stand());
        assert!(all.missing().is_empty());
    }
}

#[cfg(all(test, windows))]
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

/// **The page host, on a platform whose engine has not been written yet**
/// (M4-2, gated on X-2).
///
/// X-2 has already settled what replaces it: one Objective-C class conforming
/// to `WKNavigationDelegate` and `WKUIDelegate` calling the same
/// `navigation_gate`, plus a `WKContentRuleList` compiled from the same
/// constants `resource_request` reads, with two guarantees named as
/// unsupported (`docs/plans/port/probe-x2-wkwebview-policy-2026-09-12.md`).
/// None of that is M1-1's, and the twelve data types above travel unchanged
/// into it.
///
/// **`WebHost::new` cannot refuse**, because its return type is `Self`: the
/// window builds one per web seat and holds it. So the refusal lives where a
/// page is actually asked for — `request_environment` — and everything after
/// that is unreachable until a seat gets past it, which no seat does.
#[cfg(not(windows))]
#[path = "webview_portable.rs"]
mod portable;

#[cfg(not(windows))]
pub use portable::{WebHost, forget_web_environment, webview2_runtime_version};
