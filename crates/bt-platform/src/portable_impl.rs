//! **The window backend on a platform whose windows have not been written
//! yet** — the non-Windows twin of `windows_impl` (ticket M1-1).
//!
//! # Why this file exists at all
//!
//! `bt-app` names `bt_platform::` at five hundred and twenty-eight places
//! across thirty-six files and carries a platform `cfg` in eleven of them
//! (`docs/plans/port/macos-plan-2026-09-12.md` §4.3, and the count is the
//! backend inventory's own §4). **That ratio is the design**: the application
//! says what it wants and this crate is the only thing that knows which machine
//! it is on. It follows that every name the application writes has to exist on
//! every platform the application is built for, and that is what this module
//! is: the same seventy-odd names, answering for a machine that has no Win32.
//!
//! # What each item is allowed to do, and it is not a free choice
//!
//! The plan's §4.4 gives five classes and the inventory adds a sixth. Three of
//! them are used here and the difference between them is the whole of this
//! file's content:
//!
//! * **X — refuse when invoked.** The item exists, costs nothing at startup,
//!   and answers *"not on this platform"* with a reason the moment somebody
//!   asks it to do the thing. This is the default, and it is what makes a
//!   deferred service constructible: `MathContextMenu::new` returns a value and
//!   `MathContextMenu::request` is where the refusal lives. **A refusal at
//!   construction time would kill a launch; a refusal at invocation time is a
//!   toast.**
//! * **N — a harmless no-op.** The item does nothing and nothing downstream
//!   notices, because the thing it does on Windows is not a thing this platform
//!   has. `adopt_parent_console` is the example the plan gives: a Unix process
//!   already has its parent's stdio, so borrowing one is not a stub standing in
//!   for work, it is the work already being done.
//! * **R, really done.** A handful of items here are neither: they are small
//!   enough and load-bearing enough that the honest answer is the real one.
//!   `leave_process` cannot refuse — its return type has no empty answer — and
//!   `write_to_console` and `message_box` are the last two things a process
//!   says before it dies, which is a bad moment to say nothing.
//!
//! # What is deliberately **not** here
//!
//! The nine `windows_impl` names `bt-app` never writes — `apartments_left`,
//! `std_error_is_console`, `exposure_probe_points`, `exposed_from_probe`,
//! `cloaked_from_attribute`, `taskbar_auto_hidden_from_state`,
//! `process_image_path`, `quiet_command_named` and `monospace_font_families` —
//! stay on the Windows side. Lifting a name nobody calls would be porting a
//! shape rather than a behaviour, which is the mistake the inventory's §5 asks
//! M4-2 not to make fourteen times over. The four `bt-app` *does* name behind
//! its own `#[cfg(windows)]` — `documents_directory`, `file_product_version`
//! and the two registry readers — stay there too, and the eleven files §4.3
//! lists are where those gates already are.
//!
//! # Where the real work went
//!
//! Every refusal below names the ticket that ends it. M1-3 owns the window and
//! screen backend, M1-4 the Metal surface, M2-1 FSEvents, M2-2 the process
//! door and the trash, M2-3 the pickers and the alert, M3-7 stdio, M4-1 the
//! `CALayer` composition, M4-6 notifications and the Dock tile. This file is
//! not a plan for them; it is what stands where they will, so that a window
//! opens today.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::{
    ContextMenuShape, ContextMenuTree, NativeWindow, PageVisual, TaskbarProgress,
    WheelScrollAmount, WindowRect,
};

/// The sentence a refusal says, with the thing that was asked for in front of
/// it.
///
/// One spelling for all of them, because the reader who sees it in a toast or
/// in `diagnostics.log` is entitled to recognise the shape: *what* was asked
/// for, and that the answer is about the platform rather than about their
/// machine. `a_deferred_service_that_is_not_on_this_platform_refuses_when_invoked_not_at_startup`
/// reads this word.
fn not_here(what: &str) -> String {
    format!("{what} is not on this platform yet")
}

// ── the composition tree (M1-4, M4-1) ──────────────────────────────────────

/// **The window's visual tree, on a platform that has not been given one**
/// (M1-4 for the surface, M4-1 for the page composition).
///
/// On Windows this owns a DirectComposition device, a target on the window and
/// a tree of visuals; the swapchain hangs off its GPU visual and the web pane's
/// hole shows the page composed underneath. None of that exists here yet, and
/// X-1 has already settled what will replace it: a `CAMetalLayer` the platform
/// arm owns on the window's view, cleared of its sublayers before every
/// reconstruction, with premultiplied pixels declared to wgpu as
/// `PostMultiplied` (`docs/plans/port/probe-x1-metal-alpha-2026-09-12.md`).
///
/// **So this is constructible and inert.** It has to be constructible because
/// `Runtime::create` holds one per window and builds it before the first frame;
/// it is inert because the frame does not go through it — `bt-app`'s
/// `window_surface_target` hands wgpu the window itself on this platform, which
/// is the seam M1-4 replaces (see that function's note).
///
/// The methods divide the way the plan's classes divide. **The ones a frame
/// makes are no-ops**: `commit`, `set_window_size` and `set_covered_size` are
/// called on every resize and every present, and a refusal would be one line of
/// stderr per frame for a tree that is not there. **The ones a page makes
/// refuse**, because a page cannot be opened here at all — `WebHost::new`
/// refuses first — so an `attach_web_visual` reaching this arm is a defect
/// worth hearing about.
pub struct Compositor {
    /// The window this tree would hang on. Held rather than used: M1-4 needs it
    /// the moment there is a layer to put on the view, and a `Compositor` that
    /// did not remember its window would be a different type by then.
    #[expect(
        dead_code,
        reason = "M1-4 reads it to reach the window's view; holding it now is what keeps the \
                  type's shape the same on both platforms"
    )]
    window: NativeWindow,
}

impl Compositor {
    /// Build the tree. **Never fails**, which is the point: this call is step
    /// 13 of the sixteen-step startup path and it is one of the seven that
    /// propagate with `?` (`backend-inventory-2026-09-12.md` §6 ⑥).
    pub fn new(window: NativeWindow) -> Result<Self, String> {
        Ok(Self { window })
    }

    /// The tree is told the window's new size. Nothing to tell.
    pub fn set_window_size(&self, width: u32, height: u32) -> Result<(), String> {
        let _ = (width, height);
        Ok(())
    }

    /// The tree is told how much of the window the swapchain covers — the
    /// skirt's input. There is no skirt here, because there is no visual under
    /// the surface for one to be cut out of.
    pub fn set_covered_size(&self, width: u32, height: u32) -> Result<(), String> {
        let _ = (width, height);
        Ok(())
    }

    /// Whether the skirt is covering anything. It is not: there is none.
    #[must_use]
    pub fn skirt_covers_anything(&self) -> bool {
        false
    }

    /// The colour under a page. Nothing composes under this surface yet, so
    /// there is no floor to paint and nothing downstream notices — M4-1 gives
    /// this arm the `CALayer` the ground is.
    pub fn set_page_ground_color(&self, premultiplied_srgb: [f32; 4]) -> Result<(), String> {
        let _ = premultiplied_srgb;
        Ok(())
    }

    /// A page arrives in the tree. Refused: there are no pages here until M4-2,
    /// and a caller that got this far went past a `WebHost` that already said so.
    pub fn attach_web_visual(&self, page: PageVisual) -> Result<(), String> {
        let _ = page;
        Err(not_here("the web preview's visual tree"))
    }

    /// A page is put somewhere. Refused, as [`Self::attach_web_visual`].
    pub fn place_web_visual(
        &self,
        page: PageVisual,
        offset: (i32, i32),
        clip: (f32, f32, f32, f32),
    ) -> Result<(), String> {
        let _ = (page, offset, clip);
        Err(not_here("the web preview's visual tree"))
    }

    /// A page is taken off the glass. Refused, as [`Self::attach_web_visual`].
    pub fn hide_web_visual(&self, page: PageVisual) -> Result<(), String> {
        let _ = page;
        Err(not_here("the web preview's visual tree"))
    }

    /// A page leaves the tree. Refused, as [`Self::attach_web_visual`].
    pub fn detach_web_visual(&self, page: PageVisual) -> Result<(), String> {
        let _ = page;
        Err(not_here("the web preview's visual tree"))
    }

    /// Everything said since the last commit becomes visible. Nothing was said.
    pub fn commit(&self) -> Result<(), String> {
        Ok(())
    }
}

// ── the self-drawn frame (M3-3) ────────────────────────────────────────────

/// **The window's own frame, against traffic lights that are not there yet**
/// (M3-3).
///
/// On Windows this is a `WM_NCCALCSIZE` subclass that makes the client area the
/// whole outer rectangle, and installing it is step 5 of the startup path — one
/// of the seven fatal `?`. macOS draws its own title bar and its own three
/// buttons, and a window that wears Folio's frame there is a
/// `titlebarAppearsTransparent` / `NSFullSizeContentView` window with the
/// traffic lights placed by hand. That is M3-3's whole ticket.
///
/// Until then the window wears the system frame, which is a window that opens
/// and can be moved rather than a window that is missing. The three accessors
/// answer what a frame that is not there would answer.
pub struct CustomWindowFrame {
    /// As [`Compositor::window`]: M3-3 needs it, and holding it keeps one shape.
    #[expect(
        dead_code,
        reason = "M3-3 reads it to reach the NSWindow whose title bar it takes over"
    )]
    window: NativeWindow,
}

impl CustomWindowFrame {
    /// Install the frame. **Never fails** — see the type's note and §4.4.
    pub fn install(
        window: NativeWindow,
        geometry: crate::CustomFrameGeometry,
    ) -> Result<Self, String> {
        let _ = geometry;
        Ok(Self { window })
    }

    /// Whether the window is inside a system move/resize loop. macOS has no
    /// modal loop of that kind to be inside, so: no.
    #[must_use]
    pub fn in_size_move(&self) -> bool {
        false
    }

    /// Where the tab strip ends, for the caption's hit test. Nothing hit-tests
    /// against this frame yet.
    pub fn set_tab_strip_right_px(&self, tab_strip_right_px: i32) {
        let _ = tab_strip_right_px;
    }

    /// The smallest client the window may be dragged to. The system frame
    /// enforces its own until M3-3; refusing would be one line of stderr every
    /// time a pane is added, so this is an N and not an X.
    pub fn set_min_client_size(&self, logical: Option<(u32, u32)>) -> Result<(), String> {
        let _ = logical;
        Ok(())
    }
}

// ── the Dock tile (M4-6) ───────────────────────────────────────────────────

/// **Progress on the taskbar button, where the taskbar is a Dock** (M4-6).
///
/// Built lazily on the first progress report and remembering its refusal — the
/// shape the inventory points at as the one every macOS refusal should copy
/// (`backend-inventory-2026-09-12.md` §3 (a), "Deferred, but inside M1's
/// acceptance line"). So this one is allowed to refuse at construction: its
/// caller is not a launch, it is a progress bar, and `bt_app`'s own
/// `TaskbarProgressButton` writes one line of stderr and never asks again.
pub struct Taskbar {
    /// Never constructed: [`Taskbar::new`] refuses.
    _never: std::convert::Infallible,
}

impl Taskbar {
    /// Refused. The Dock tile is M4-6.
    pub fn new(window: NativeWindow) -> Result<Self, String> {
        let _ = window;
        Err(not_here("progress on the Dock tile"))
    }

    /// Unreachable: there is no value of this type. Present so that the type's
    /// shape is the same on both platforms.
    pub fn set_progress(&self, progress: TaskbarProgress) -> Result<(), String> {
        let _ = progress;
        match self._never {}
    }
}

// ── the system preferences watch (M1-3) ────────────────────────────────────

/// **A wake when the system's own settings change** (M1-3, M3-3).
///
/// `WM_SETTINGCHANGE` on Windows; `NSDistributedNotificationCenter` and
/// `NSWorkspace`'s notifications here. Already best-effort at the call site —
/// step 16 of the startup path installs it with `.ok()` — so this arm is
/// constructible and silent rather than refusing, because a refusal would print
/// a line for every window opened about a subscription nobody has missed yet.
///
/// What is lost until M1-3: the reader switching the system between light and
/// dark while Folio is open is not noticed. The theme is still read once at
/// startup.
pub struct SystemSettingsWatch {
    /// The callback, held so that its lifetime is the watch's exactly as on
    /// Windows — a wake that could fire after the watch was dropped is the one
    /// defect this type's shape exists to make impossible.
    #[expect(
        dead_code,
        reason = "M1-3 calls it from the notification observer; holding it now keeps the \
                  ownership contract identical on both platforms"
    )]
    wake: Box<dyn Fn()>,
}

impl SystemSettingsWatch {
    /// Install the watch. Constructs, subscribes to nothing, and never wakes.
    pub fn install(window: NativeWindow, wake: Box<dyn Fn()>) -> Result<Self, String> {
        let _ = window;
        Ok(Self { wake })
    }
}

// ── notifications (M4-6) ───────────────────────────────────────────────────

/// **The desktop notification, before `UNUserNotificationCenter`** (M4-6).
///
/// Built on the first toast and remembering its refusal, exactly as [`Taskbar`]
/// is, so refusing at construction costs one line of stderr for the run rather
/// than one per message. `UNUserNotificationCenter` also refuses a process with
/// no bundle identifier, which is why §4.5 of the plan builds the bundle from
/// M1 rather than M5 — the refusal this arm gives today is the refusal that
/// one would give to an unbundled binary anyway.
pub struct Notifier {
    /// Never constructed: [`Notifier::new`] refuses.
    _never: std::convert::Infallible,
}

impl Notifier {
    /// Refused. Notifications are M4-6.
    pub fn new(wake: Box<dyn Fn() + Send>) -> Result<Self, String> {
        let _ = wake;
        Err(not_here("desktop notifications"))
    }

    /// Unreachable: there is no value of this type.
    pub fn show(&mut self, title: &str, body: &str, launch: &str) -> Result<(), String> {
        let _ = (title, body, launch);
        match self._never {}
    }

    /// Unreachable: there is no value of this type.
    #[must_use]
    pub fn take_activations(&self) -> Vec<String> {
        match self._never {}
    }
}

// ── the three deferred shell dialogs (M2-3) ────────────────────────────────

/// **What a picker is being opened for.**
///
/// The same three cases the Windows arm names, because the cases are the
/// product's and not the dialog's: `NSOpenPanel` takes content types where
/// `IFileDialog` takes a filter string, and the *choice* between a folder, a
/// picture and a program is made above both.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellPickKind {
    Folder,
    Image,
    /// A profile's own shell — unfiltered on purpose, for the Windows arm's
    /// reason: what may be started is the operating system's answer rather than
    /// this dialog's.
    Program,
}

/// The public name for [`ShellPickKind`], as on Windows.
///
/// An alias and not a second enum: the inventory's §6 ⑤ warns that the public
/// name and the defined name differ here and that a mechanical rename would
/// break it.
pub type FilePickKind = ShellPickKind;

/// **The formula context menu, before there is an `NSMenu` to pop** (M2-3, and
/// the menu itself is §7.20's).
///
/// Step 7 of the startup path and one of the seven fatal `?`. Constructed
/// harmlessly; [`Self::request`] is where the refusal lives, and its caller
/// already turns a refusal into a toast rather than into a dead window.
pub struct MathContextMenu {
    #[expect(dead_code, reason = "M2-3 pops the menu over this window")]
    window: NativeWindow,
}

impl MathContextMenu {
    /// Install the deferred menu. Never fails.
    pub fn new(window: NativeWindow) -> Result<Self, String> {
        Ok(Self { window })
    }

    /// Ask for the menu. Refused, with the reason a toast can carry.
    pub fn request(&self) -> Result<bool, String> {
        Err(not_here("the formula menu"))
    }

    /// The menu's answer. There is never one, because [`Self::request`] refused.
    #[must_use]
    pub fn take_result(&self) -> Option<Result<bool, String>> {
        None
    }
}

/// **The folder chooser, before `NSOpenPanel`** (M2-3).
///
/// Step 8 of the startup path. As [`MathContextMenu`]: constructed harmlessly,
/// refuses when asked.
///
/// X-4 leaves M2-3 a warning worth repeating here, because it is the reason
/// this is not a two-line port: AppKit's modal loops — `NSOpenPanel` and
/// `NSAlert` both — do not drain the main dispatch queue, so a panel run from
/// the wrong place stops the event loop that would answer it.
pub struct FolderPicker {
    #[expect(dead_code, reason = "M2-3 sheets the panel onto this window")]
    window: NativeWindow,
}

impl FolderPicker {
    /// Install the deferred chooser. Never fails.
    pub fn new(window: NativeWindow) -> Result<Self, String> {
        Ok(Self { window })
    }

    /// Ask for the chooser. Refused.
    pub fn request(&self, start: Option<&Path>) -> Result<bool, String> {
        let _ = start;
        Err(not_here("the folder chooser"))
    }

    /// The chooser's answer. There is never one.
    #[must_use]
    pub fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
        None
    }
}

/// **The picture and program chooser, before `NSOpenPanel`** (M2-3).
///
/// Step 9 of the startup path. As [`FolderPicker`].
pub struct ImagePicker {
    #[expect(dead_code, reason = "M2-3 sheets the panel onto this window")]
    window: NativeWindow,
}

impl ImagePicker {
    /// Install the deferred chooser. Never fails.
    pub fn new(window: NativeWindow) -> Result<Self, String> {
        Ok(Self { window })
    }

    /// Ask for the chooser. Refused.
    pub fn request(&self, kind: ShellPickKind, start: Option<&Path>) -> Result<bool, String> {
        let _ = (kind, start);
        Err(not_here("the file chooser"))
    }

    /// The chooser's answer. There is never one.
    #[must_use]
    pub fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
        None
    }
}

// ── the input method's caret (M1-8) ────────────────────────────────────────

/// **Where the candidate window goes** (M1-8).
///
/// Step 6 of the startup path, and the one constructor there that cannot fail
/// on either platform. On Windows this is IMM32's system caret; on macOS it is
/// `NSTextInputClient`'s `firstRectForCharacterRange:`, which winit already
/// implements and feeds from `Window::set_ime_cursor_area` — so M1-8's work is
/// mostly *removing* this door on that platform rather than reimplementing it.
///
/// Until that is decided, updating costs nothing and the candidate window
/// appears wherever the system puts it. `update`'s caller already ignores the
/// answer.
pub struct ImeSystemCaret {
    #[expect(dead_code, reason = "M1-8 decides whether this door survives at all")]
    window: NativeWindow,
}

impl ImeSystemCaret {
    /// Infallible on both platforms — the type has no error to return.
    #[must_use]
    pub fn new(window: NativeWindow) -> Self {
        Self { window }
    }

    /// Move the caret. Nothing is moved; see the type's note.
    pub fn update(&mut self, x: i32, y: i32) -> Result<(), String> {
        let _ = (x, y);
        Ok(())
    }

    /// Take the caret down. There is none.
    pub fn destroy(&mut self) {}
}

// ── the directory watch (M2-1) ─────────────────────────────────────────────

/// **What one completion of the watch said changed.**
///
/// The same two answers FSEvents gives — named entries, or *more than I could
/// write down* — which is why the enum travels unchanged. M2-1 owns all three
/// of the contracts the three constructors carry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirChange<'a> {
    /// The entries the watcher named, relative to the watched directory.
    Named(&'a [OsString]),
    /// The watcher lost track. Something changed; nothing can say what. **A
    /// name filter must let this through.**
    Unknown,
}

/// **A subscription to a directory, before FSEvents** (M2-1).
///
/// The three contracts — `Tree`, `HereOnly`, named `HereOnly` — are three
/// constructors here as on Windows, and M2-1 must preserve all three rather
/// than folding them: the inventory's §6 ⑤ notes that the depth enum is not
/// part of the public interface, so the contracts *are* the doors.
///
/// Refused rather than silently never waking, because the caller's failure
/// policy is already "log it, and the watch is absent": a files column that is
/// not being watched is a files column that needs refreshing by hand, and a
/// reader is better served by one line saying so than by a tree that quietly
/// stops agreeing with the disk.
pub struct DirWatch {
    /// Never constructed: every constructor refuses.
    _never: std::convert::Infallible,
}

impl DirWatch {
    /// The tree contract. Refused; M2-1.
    pub fn start(path: &Path, wake: impl Fn() + Send + 'static) -> Result<Self, std::io::Error> {
        let _ = (path, wake);
        Err(unwatched())
    }

    /// The here-only contract. Refused; M2-1.
    pub fn start_shallow(
        path: &Path,
        wake: impl Fn() + Send + 'static,
    ) -> Result<Self, std::io::Error> {
        let _ = (path, wake);
        Err(unwatched())
    }

    /// The named here-only contract. Refused; M2-1.
    pub fn start_shallow_named(
        path: &Path,
        wake: impl Fn(DirChange<'_>) + Send + 'static,
    ) -> Result<Self, std::io::Error> {
        let _ = (path, wake);
        Err(unwatched())
    }
}

/// The one error every [`DirWatch`] door answers with.
fn unwatched() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        not_here("watching a directory"),
    )
}

// ── the window and the screen (M1-3) ───────────────────────────────────────
//
// Every read here has a real answer on macOS and M1-3 is the ticket that gives
// it one. What they answer today is the refusal their callers are already
// written to survive — and `bt-app`'s startup path was changed in this same
// ticket so that the three it used to propagate with `?` no longer do.

/// The window's outer rectangle. `NSWindow.frame`, flipped; M1-3.
pub fn get_window_rect(window: NativeWindow) -> Result<WindowRect, String> {
    let _ = window;
    Err(not_here("reading a window's rectangle"))
}

/// Place the window's outer rectangle. `setFrame:display:`; M1-3.
pub fn set_window_outer_rect(window: NativeWindow, rect: WindowRect) -> Result<(), String> {
    let _ = (window, rect);
    Err(not_here("placing a window"))
}

/// The quake drop — place the window and read it back until it agrees. M1-3.
pub fn stand_window_at(window: NativeWindow, rect: WindowRect) -> Result<(), String> {
    let _ = (window, rect);
    Err(not_here("standing a window at a rectangle"))
}

/// The work area of the display this window is on. `NSScreen.visibleFrame`; M1-3.
pub fn get_work_area(window: NativeWindow) -> Result<WindowRect, String> {
    let _ = window;
    Err(not_here("reading a display's work area"))
}

/// The work area of the display under a point. M1-3.
pub fn work_area_at(x: i32, y: i32) -> Result<WindowRect, String> {
    let _ = (x, y);
    Err(not_here("reading a display's work area"))
}

/// The union of every display. M1-3.
///
/// **Not a refusal, because the type has no empty answer** — and the caller
/// reads it as a fallback for `work_area_at`, so an empty rectangle is the
/// honest "nothing is known about the desktop" rather than a lie about its
/// size. The one thing it must not be is a made-up screen: a window clamped to
/// an invented rectangle is a window in the wrong place, which is §7.50's own
/// history.
#[must_use]
pub fn virtual_screen_rect() -> WindowRect {
    WindowRect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    }
}

/// The DPI of the display under a point. M1-3.
///
/// `96` is the identity, not a guess: every caller divides by 96 to get a scale
/// factor, so this answers *scale 1.0* — "nothing here says otherwise" — and
/// the arithmetic downstream is unchanged rather than skewed.
#[must_use]
pub fn dpi_at(x: i32, y: i32) -> u32 {
    let _ = (x, y);
    96
}

/// This window's authoritative DPI. `NSWindow.backingScaleFactor` × 96; M1-3.
///
/// **A refusal and not `96`**, unlike [`dpi_at`], and the difference is the
/// caller: `bt_app::dpi_snapshot` asks this for a *second opinion* about a
/// window winit has already told it the scale of, and on macOS winit's own
/// `scale_factor()` **is** `backingScaleFactor` — there is no second source to
/// disagree with. Answering `96` would be this crate inventing a scale of 1.0
/// for a Retina window; refusing is the truth, and the caller falls back to
/// winit's number.
pub fn get_dpi_for_window(window: NativeWindow) -> Result<u32, String> {
    let _ = window;
    Err(not_here("a second opinion about a window's backing scale"))
}

/// Which display a point is on, as a stable name. M1-3, and R5's case-folding
/// question is M3-4's.
#[must_use]
pub fn monitor_id_at(x: i32, y: i32) -> Option<String> {
    let _ = (x, y);
    None
}

/// Where the pointer is, in screen coordinates. `NSEvent.mouseLocation`,
/// flipped; M1-3.
#[must_use]
pub fn pointer_position() -> Option<(i32, i32)> {
    None
}

/// Which top-level window the window manager puts under a screen point. M1-3.
#[must_use]
pub fn top_level_window_at(x: i32, y: i32) -> Option<NativeWindow> {
    let _ = (x, y);
    None
}

/// Which window of this thread holds the mouse capture.
///
/// **Not a refusal deferred to a ticket — there is no such thing here.** AppKit
/// has no per-thread mouse capture to report, so `None` is the whole and final
/// answer, and the one reader is a diagnostic field that already treats it as
/// optional (the inventory files this as class X for that reason).
#[must_use]
pub fn thread_mouse_capture() -> Option<NativeWindow> {
    None
}

/// Whether this window is iconic. `isMiniaturized`; M1-3.
///
/// **`false` and not a refusal**, because the type has no empty answer and the
/// direction matters: a window wrongly called minimised has its geometry
/// thrown away, and a window wrongly called normal has its geometry saved. One
/// of those two is recoverable.
#[must_use]
pub fn is_window_minimized(window: NativeWindow) -> bool {
    let _ = window;
    false
}

/// Whether the desktop compositor is holding this window back from the screen.
///
/// Cloaking is a DWM notion with no macOS twin — occlusion is the nearest fact
/// and it is not the same one — so `false` is the answer rather than a ticket's
/// placeholder. It is also the direction `cloaked_from_attribute` argues for:
/// of the two wrong answers, under-stating flashes a Dock icon nobody sees and
/// over-stating puts a notification in front of somebody looking at the pane it
/// is about.
#[must_use]
pub fn is_window_cloaked(window: NativeWindow) -> bool {
    let _ = window;
    false
}

/// Whether the reader can actually see this window. `NSWindowOcclusionState`;
/// M4-6 reads it through the notification gate.
///
/// `true` for the same reason the Windows arm reports an unreadable rectangle
/// as exposed: of the two wrong answers, one leaves the marks inside a window
/// the reader is looking at and the other puts a notification on a desktop they
/// can see.
#[must_use]
pub fn window_is_exposed(window: NativeWindow) -> bool {
    let _ = window;
    true
}

/// Put this window in front and give it the keyboard. `makeKeyAndOrderFront:`;
/// M1-3.
pub fn take_keyboard_focus(window: NativeWindow) -> Result<(), String> {
    let _ = window;
    Err(not_here("taking the keyboard focus"))
}

/// Ask the window to close. `performClose:`; M1-3.
pub fn request_window_close(window: NativeWindow) -> Result<(), String> {
    let _ = window;
    Err(not_here("asking a window to close"))
}

/// Keep this window above the others. `NSWindow.level`; M1-3.
pub fn set_window_topmost(window: NativeWindow, topmost: bool) -> Result<(), String> {
    let _ = (window, topmost);
    Err(not_here("keeping a window above the others"))
}

/// Ask the system to draw this window's chrome dark. `NSAppearance`; M1-3.
pub fn set_window_dark_mode(window: NativeWindow, dark: bool) -> Result<(), String> {
    let _ = (window, dark);
    Err(not_here("the window's system appearance"))
}

/// The blurred material behind a translucent window. `NSVisualEffectView`; M1-3.
pub fn set_system_backdrop(window: NativeWindow, acrylic: bool) -> Result<(), String> {
    let _ = (window, acrylic);
    Err(not_here("the translucent window material"))
}

/// Whether this system knows what a backdrop is — the Acrylic row's answer.
///
/// `false` until M1-3 writes the `NSVisualEffectView` arm, which is what makes
/// the settings row read *unavailable* rather than offering a switch that does
/// nothing. The inventory expects this to become a constant `true` on macOS;
/// it is not one yet, and saying so is the row's whole job.
#[must_use]
pub fn system_backdrop_available(window: NativeWindow) -> bool {
    let _ = window;
    false
}

/// The window's backing colour before the first present. On Windows this is a
/// class brush and it is step 2 of the startup path; on macOS it is
/// `contentView.layer.backgroundColor`, which is M1-3's.
pub fn install_window_class_background(
    window: NativeWindow,
    rgb: Option<[u8; 3]>,
) -> Result<(), String> {
    let _ = (window, rgb);
    Err(not_here("the window's backing colour"))
}

/// Call the reader's eye to this window. `NSApp.requestUserAttention:`; M4-6.
pub fn flash_window(window: NativeWindow) {
    let _ = window;
}

/// Hide every window of this process — the panic path's last act, so that a
/// dying program does not leave a half-drawn window on the glass.
///
/// `NSApp.hide:`; M4-11. Nothing is hidden yet, and the count says so.
#[must_use]
pub fn hide_every_window_of_this_process() -> usize {
    0
}

/// Whether the taskbar hides itself. The Dock's `autohide` preference; M4-6.
///
/// `false` — the direction `taskbar_auto_hidden_from_state` argues for: a bar
/// reported visible costs a Dock bounce nobody sees, and a bar reported hidden
/// costs a notification in front of somebody who is looking at the pane.
#[must_use]
pub fn taskbar_is_auto_hidden() -> bool {
    false
}

/// Register a window as one the clipboard may act through.
///
/// **Nothing to register, and that is M1-9's own finding**: `NSPasteboard` is a
/// property of the session rather than of a window, so the list the Windows arm
/// keeps has no counterpart here. The door exists because the constructor calls
/// it once per window on every platform.
pub fn register_clipboard_owner(owner: NativeWindow) {
    let _ = owner;
}

// ── keyboard and system preferences (M1-7, M1-3) ───────────────────────────

/// The Win32 virtual key a character is typed on **this layout** — the web
/// chord's other half.
///
/// **There is no such number here**, and that is §4.4 ②'s point rather than a
/// missing implementation: a Win32 virtual key is a Windows fact, and the macOS
/// answer is a `kVK_*` key code from `UCKeyTranslate` against the current
/// `TISInputSource`. M1-7 decides what `WebChord` carries on this platform —
/// the Command field it gained in this ticket is the data half of that
/// decision — and until it does, a page claims no chords.
#[must_use]
pub fn virtual_key_for_character(character: char) -> Option<u16> {
    let _ = character;
    None
}

/// How far one wheel notch scrolls. M1-3 reads the scroll preference.
///
/// The caller's own fallback is three lines plus one line of stderr, so
/// refusing here is a line per run rather than per notch.
pub fn wheel_scroll_amount() -> Result<WheelScrollAmount, String> {
    Err(not_here("the wheel's scroll preference"))
}

/// Whether the reader has asked for animation. *Reduce Motion*; M1-3.
pub fn client_area_animation_enabled() -> Result<bool, String> {
    Err(not_here("the reduce-motion preference"))
}

/// Whether the system is in light mode. `AppleInterfaceStyle`; M1-3.
///
/// `None` is "the system has no opinion", which the theme resolver already
/// handles: a `System` theme with no system answer takes the product's default.
#[must_use]
pub fn system_uses_light_apps() -> Option<bool> {
    None
}

/// The language the operating system is being read in.
///
/// `NSLocale.preferredLanguages`' first entry, and M1-3 is where it is read.
/// **`en` and not a refusal**, because the type has no empty answer and the
/// caller resolves a language from it at startup: what this says today is "the
/// system has not been asked", and the product's own default is what the reader
/// sees. `settings.json`'s `language` still overrides it, so a reader who wants
/// Chinese on macOS today has a row to set.
#[must_use]
pub fn os_ui_language() -> String {
    "en".to_owned()
}

// ── the shell, the trash, the volume (M2-2) ────────────────────────────────

/// Move a path to the trash. `NSFileManager.trashItemAtURL:`; M2-2.
pub fn recycle(path: &Path) -> Result<bool, String> {
    let _ = path;
    Err(not_here("moving a file to the trash"))
}

/// **Whether this volume treats two spellings of one name as one file.**
///
/// Really answered, and really answered *per directory*, because on APFS that
/// is the only way it can be: case sensitivity is a property of the volume a
/// path is on, and one machine has both — a case-insensitive boot volume and a
/// case-sensitive one somebody formatted that way. R5 of the plan is about the
/// Windows arm folding case the NTFS way; this arm asks the volume instead.
///
/// **The question is asked and not assumed.** Take the directory's own name,
/// spell it with the case of its letters flipped, and ask the file system
/// whether both spellings name the same object — same device, same inode. That
/// is one `stat` on each and it reads nothing, creates nothing and needs no
/// write permission.
///
/// A directory whose final component has no cased letter cannot answer, so the
/// question is put to the nearest ancestor that has one; a path with no cased
/// letter anywhere — `/`, `/1` — answers `false`, which is the safe direction:
/// treating two names as different when the volume would fold them shows a
/// reader two rows where there is one file, and treating them as the same hides
/// a file that exists.
#[must_use]
pub fn directory_folds_case(directory: &Path) -> bool {
    for candidate in directory.ancestors() {
        let Some(name) = candidate.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(flipped) = flip_case(name) else {
            continue;
        };
        let Some(parent) = candidate.parent() else {
            continue;
        };
        return same_file(candidate, &parent.join(flipped));
    }
    false
}

/// The same name with the case of every cased letter the other way round, or
/// `None` when the name has no cased letter to flip.
fn flip_case(name: &str) -> Option<String> {
    if !name.chars().any(char::is_alphabetic) {
        return None;
    }
    let flipped: String = name
        .chars()
        .flat_map(|character| {
            if character.is_uppercase() {
                character.to_lowercase().collect::<Vec<_>>()
            } else {
                character.to_uppercase().collect::<Vec<_>>()
            }
        })
        .collect();
    (flipped != name).then_some(flipped)
}

/// Whether two paths name one object on the disk.
///
/// Device and inode, which is the file system's own identity and not a string
/// comparison — the whole point is that the two strings are different.
#[cfg(unix)]
fn same_file(one: &Path, other: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let (Ok(one), Ok(other)) = (std::fs::metadata(one), std::fs::metadata(other)) else {
        return false;
    };
    one.dev() == other.dev() && one.ino() == other.ino()
}

/// The same question where there are no inodes to compare.
#[cfg(not(unix))]
fn same_file(one: &Path, other: &Path) -> bool {
    let _ = (one, other);
    false
}

// ── the Explorer verb, which macOS does not have (M4-9) ────────────────────
//
// `NSServices` is the macOS answer and it is declarative: the entry is read out
// of `Info.plist` and there is no registry to write, nothing to announce, and
// nothing to read back. So these three are not deferred work with a ticket
// behind them — they are a Windows mechanism that has no counterpart, and the
// settings row above them already has an *unsupported* state to show, which is
// what `bt_app::explorer_menu::supported()` answers off Windows.

/// Write the classic Explorer verb. There is no registry.
pub fn install_context_menu(classes: &str, shape: &ContextMenuShape) -> Result<(), String> {
    let _ = (classes, shape);
    Err(not_here("the Explorer context-menu entry"))
}

/// Delete the classic Explorer verb. There is none.
pub fn remove_context_menu(classes: &str) -> Result<(), String> {
    let _ = classes;
    Err(not_here("the Explorer context-menu entry"))
}

/// Read what is written. Nothing is, and an empty list is the honest reading of
/// a registry that does not exist — `context_menu_verdict` turns it into
/// `Absent`, which is exactly right.
#[must_use]
pub fn read_context_menu(classes: &str) -> Vec<ContextMenuTree> {
    let _ = classes;
    Vec::new()
}

/// Tell the shell the verb changed. Nothing changed and nobody is listening.
pub fn announce_explorer_menu_change() {}

// ── the console and the process (M3-7) ─────────────────────────────────────

/// **Borrow the console of whoever started this process.**
///
/// A no-op, and the plan names this one as the example of its class: a Unix
/// process is *given* its parent's stdio by the kernel, so there is nothing to
/// adopt and nothing downstream notices the call did nothing.
pub fn adopt_parent_console() {}

/// **Leave that console's process group.**
///
/// The other half of the same fact: there is no console membership here to
/// leave, so nothing is left. The `bool` says so.
pub fn detach_console() -> bool {
    false
}

/// **Write to the console this process is attached to.**
///
/// Really done, because this is how `folio --help` answers and how the
/// attention verb prints. On Windows it has to attach to the parent's console
/// first; here the parent's stdout is already this process's stdout, so the
/// whole of the port is one write — and the `bool` is whether it landed, which
/// is what the caller chains on.
pub fn write_to_console(text: &str) -> bool {
    use std::io::Write;

    let mut out = std::io::stdout().lock();
    out.write_all(text.as_bytes()).is_ok() && out.flush().is_ok()
}

/// **Send stdout and stderr to a file for the rest of the run** (M3-7).
///
/// §4.4 ③ calls this load-bearing and it is: `bt_app::diagnostics` picks
/// `Channel::Log` or `Channel::Nowhere` off this `bool`, so an arm that
/// answered `true` without redirecting anything would put the run's diagnostics
/// nowhere at all while claiming a log file that stays empty.
///
/// So it answers `false`, honestly, and the two things that follow are both
/// right: the channel becomes `Nowhere`, and [`silence_std_streams`] — which is
/// what `Nowhere` means on Windows — does nothing here, so the streams stay
/// where the launcher put them. A Folio started from a shell prints to that
/// shell, which is what M1's acceptance needs; a Folio started from Finder
/// prints into the system log, which is where a `.app` with no redirect always
/// printed. M3-7 makes it a real `dup2` and takes the bundle's launch with it.
pub fn redirect_std_streams_to_file(path: &Path) -> bool {
    let _ = path;
    false
}

/// **Send stdout and stderr nowhere** (M3-7).
///
/// Deliberately nothing — see [`redirect_std_streams_to_file`]. Silencing the
/// streams of a process that has no log file to write to instead would throw
/// away the only diagnostics this platform currently has.
pub fn silence_std_streams() {}

/// **A handler for the interrupt the console sends** (M3-7).
///
/// `SIGINT` and `SIGTERM` are the Unix shape and M3-7 owns them. `false` says
/// no handler was installed, which is what the caller records.
pub fn install_console_ctrl_handler() -> bool {
    false
}

/// **End this process, now.**
///
/// Really done, and it cannot be anything else: §4.4 ③ names this as a return
/// type with no empty answer. `std::process::exit` runs no destructors, which
/// is the Windows arm's behaviour too — everything that had to be flushed was
/// flushed by the caller before it got here.
pub fn leave_process(code: i32) -> ! {
    std::process::exit(code)
}

/// **The last-resort fault report** (M2-3 gives it `NSAlert`).
///
/// Really done, to stderr, and not deferred — this is what a process says when
/// it has already failed to open a window, and a refusal here would mean the
/// one message that matters is the one nobody sees. The two lines it writes are
/// the two `NSAlert` would show.
pub fn message_box(title: &str, text: &str) {
    eprintln!("{title}");
    eprintln!("{text}");
}

/// **The same claim, made by calling the doors** — the behavioural twin of
/// `deferred_service_tests`, which is a source pin and runs on Windows.
///
/// This one only compiles where the module does, so it runs on the Mac and on
/// nothing else. That is the point: a source pin reads what the file says and
/// this reads what the file does, and between them the rule holds on the
/// machine that authors it and on the machine that accepts it.
///
/// MUTATION: make any of the seven constructors refuse and the first test goes
/// red; make any of the four doors succeed and the second does.
#[cfg(test)]
mod refusal_tests {
    use super::*;

    /// A window token that names no window — see `NativeWindow::stand_in`.
    /// Nothing below reaches the machine, which is why a stand-in is enough.
    fn window() -> NativeWindow {
        NativeWindow::stand_in(1)
    }

    /// RED — **the startup path's constructors are harmless.**
    #[test]
    fn every_startup_constructor_builds_rather_than_refusing() {
        assert!(Compositor::new(window()).is_ok(), "the visual tree");
        assert!(
            CustomWindowFrame::install(
                window(),
                crate::CustomFrameGeometry {
                    title_bar_logical_px: 40,
                    caption_button_logical_px: 46,
                },
            )
            .is_ok(),
            "the self-drawn frame"
        );
        assert!(MathContextMenu::new(window()).is_ok(), "the formula menu");
        assert!(FolderPicker::new(window()).is_ok(), "the folder chooser");
        assert!(ImagePicker::new(window()).is_ok(), "the picture chooser");
        assert!(
            SystemSettingsWatch::install(window(), Box::new(|| {})).is_ok(),
            "the system settings watch"
        );
        // `ImeSystemCaret::new` returns `Self` and has no failure to test; that
        // it compiles at all is the claim.
        let mut caret = ImeSystemCaret::new(window());
        assert!(
            caret.update(0, 0).is_ok(),
            "moving a caret that is not there"
        );
    }

    /// RED — **and asking them to do the thing refuses, with the reason.**
    #[test]
    fn a_deferred_service_that_is_not_on_this_platform_refuses_when_invoked_not_at_startup() {
        let menu = MathContextMenu::new(window()).expect("built above");
        let refusal = menu.request().expect_err("there is no menu to pop");
        assert!(
            refusal.contains("not on this platform"),
            "a refusal says which platform it is about: {refusal}"
        );
        assert!(
            menu.take_result().is_none(),
            "a request that refused leaves no answer to collect"
        );

        let folder = FolderPicker::new(window()).expect("built above");
        assert!(folder.request(None).is_err(), "there is no panel to sheet");

        let picture = ImagePicker::new(window()).expect("built above");
        assert!(
            picture.request(ShellPickKind::Image, None).is_err(),
            "there is no panel to sheet"
        );

        let compositor = Compositor::new(window()).expect("built above");
        let page = PageVisual { tab: 1, seat: 1 };
        assert!(
            compositor.attach_web_visual(page).is_err(),
            "there is no visual tree to put a page in"
        );
        // And the frame's own calls are the no-ops a frame makes, not refusals:
        // one line of stderr per present is not a diagnostic, it is a fault.
        assert!(
            compositor.commit().is_ok(),
            "committing nothing costs nothing"
        );
        assert!(
            compositor.set_window_size(800, 600).is_ok(),
            "a resize tells the tree nothing and succeeds at it"
        );
    }

    /// PIN — **the window doors refuse rather than pretending.**
    ///
    /// The reads and writes M1-3 owns. Each one is a place where answering
    /// `Ok(())` would leave `bt-app` believing a window had been placed,
    /// focused or closed.
    #[test]
    fn the_window_backend_refuses_rather_than_pretending() {
        let rect = WindowRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        assert!(get_window_rect(window()).is_err());
        assert!(set_window_outer_rect(window(), rect).is_err());
        assert!(get_work_area(window()).is_err());
        assert!(take_keyboard_focus(window()).is_err());
        assert!(request_window_close(window()).is_err());
        assert!(get_dpi_for_window(window()).is_err());
    }

    /// PIN — **the volume is asked, not assumed.**
    ///
    /// `directory_folds_case` is the one door here that really answers, and the
    /// claim is only that it answers *about the volume* rather than about the
    /// platform: the temporary directory and the root are two different
    /// questions and the function has to be willing to give two different
    /// answers. What that answer is on this machine is the machine's business.
    #[test]
    fn the_volume_answers_the_case_question_itself() {
        let temporary = std::env::temp_dir();
        let folds = directory_folds_case(&temporary);
        assert_eq!(
            folds,
            directory_folds_case(&temporary),
            "the same directory answers the same way twice"
        );
        assert!(
            !directory_folds_case(std::path::Path::new("/")),
            "a path with no cased letter anywhere cannot answer, and says no"
        );
    }
}
