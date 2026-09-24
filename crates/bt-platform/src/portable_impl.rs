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

#[cfg(not(any(windows, target_os = "macos")))]
use std::ffi::OsString;
use std::path::Path;
// `PathBuf` is a chooser's answer and nothing else in this module's, so on macOS
// — where M2-3 took the two choosers next door — there is nothing left here that
// names one.
#[cfg(not(any(windows, target_os = "macos")))]
use std::path::PathBuf;

use crate::{ContextMenuShape, ContextMenuTree, NativeWindow};
// The one type only the Dock tile names, which on macOS is `macos_notify`'s and
// not this module's (M4-6).
#[cfg(not(any(windows, target_os = "macos")))]
use crate::TaskbarProgress;
// The one type only the composition names, which on macOS is `macos_compose`'s
// and not this module's (M4-1).
#[cfg(not(target_os = "macos"))]
use crate::PageVisual;
// The two types only the window and screen group names, which on macOS is
// `macos_impl`'s group and not this module's (M1-3).
#[cfg(not(any(windows, target_os = "macos")))]
use crate::{WheelScrollAmount, WindowRect};

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
///
/// **Not compiled on macOS.** M4-1 wrote that arm — `macos_compose::Compositor`
/// — and a second definition of the same name would be two `Compositor`s in one
/// crate root. The same split M1-3 made for the window group and M2-1 for the
/// watches.
#[cfg(not(target_os = "macos"))]
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

#[cfg(not(target_os = "macos"))]
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

    /// The window says where its own chrome stands over a page. Refused, as
    /// [`Self::attach_web_visual`]: there is no page here to be stood over.
    pub fn set_page_cover(&self, page: PageVisual, rects: &[[f32; 4]]) -> Result<(), String> {
        let _ = (page, rects);
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

/// **The window's own frame, against the traffic lights** (M3-3).
///
/// On Windows this is a `WM_NCCALCSIZE` subclass that takes the system title
/// bar away outright: the client area becomes the whole outer rectangle and
/// Folio draws all four caption slots itself. Installing it is step 5 of the
/// startup path — one of the seven fatal `?`.
///
/// **On macOS the opposite move, ruled by the owner on 2026-09-12**: the
/// native title bar is *kept* and made part of Folio's chrome —
/// `NSFullSizeContentView`, `titlebarAppearsTransparent`, no title text — so
/// that the three traffic lights stay exactly where macOS puts them and Folio
/// draws no minimise, zoom or close of its own. One window, one set of window
/// controls. The work is [`crate::macos_impl::adopt_window_chrome`]'s, because
/// AppKit lives there and not here; what this type keeps is the measurement it
/// hands back, and [`Self::platform_chrome`] is where `bt-app` reads it.
///
/// On a third platform there is no title bar to take over, and every accessor
/// answers what a frame that is not there would answer.
pub struct CustomWindowFrame {
    /// As the composition's own window field: the drag door reaches the
    /// `NSWindow` through it, and holding it keeps one shape. Not an intra-doc
    /// link, because on macOS the `Compositor` that field belongs to is
    /// `macos_compose`'s and this one is not compiled.
    #[cfg_attr(
        not(target_os = "macos"),
        expect(
            dead_code,
            reason = "only the macOS arm has a native title bar to reach back into"
        )
    )]
    window: NativeWindow,
    /// What the platform goes on drawing in this window's bar — measured at
    /// install, because it is a fact about *this* window and not about the
    /// host.
    chrome: crate::PlatformChrome,
    /// **What keeps the platform's own buttons on the band this window wears**
    /// (T-MAC-PILL, amended by T-MAC-LIGHTS): the subscriptions that re-state
    /// their placement every time AppKit lays the title bar out again, holding
    /// the band [`Self::set_window_band`] last named. Held here and nowhere
    /// else, so it is removed when the frame is — a window's observer outliving
    /// its window is the bug this shape makes unwritable.
    ///
    /// `None` on a window whose title bar is not the platform's, which is every
    /// window on every other host and any window this crate could not measure.
    #[cfg(target_os = "macos")]
    buttons: Option<crate::macos_impl::WindowButtonsWatch>,
}

impl CustomWindowFrame {
    /// Install the frame. **Never fails** — see the type's note and §4.4.
    pub fn install(
        window: NativeWindow,
        geometry: crate::CustomFrameGeometry,
        wake: Box<dyn Fn()>,
    ) -> Result<Self, String> {
        // **`geometry` is read now, and the field it is read for is the bar's
        // own height** (T-MAC-PILL). It was taken and dropped while this arm had
        // nothing in that band to agree with; the traffic lights are centred on
        // the strip Folio draws, and the height of that strip is the caller's
        // fact rather than AppKit's.
        let adopted = adopt_platform_chrome(window, f64::from(geometry.title_bar_logical_px), wake);
        #[cfg(target_os = "macos")]
        let (chrome, buttons) = adopted;
        #[cfg(not(target_os = "macos"))]
        let chrome = adopted;
        Ok(Self {
            window,
            chrome,
            #[cfg(target_os = "macos")]
            buttons,
        })
    }

    /// **What the platform draws in this window's title bar** — the one read
    /// `bt-app` makes, and the reason it never asks which host it is on.
    ///
    /// **One answer per window *state*, not one for the window's life**
    /// (§13.48). The measurement taken at `install` holds until macOS takes this
    /// window's buttons away, which it does in full screen and undoes on the way
    /// out; the watch is what hears those two transitions and re-measures, so on
    /// a window that has one this accessor asks the watch rather than the copy
    /// `install` bound. The field below is that copy, and it is the answer for
    /// every window with no watch — which is every window on every other host,
    /// and any window this crate could not measure.
    #[must_use]
    pub fn platform_chrome(&self) -> crate::PlatformChrome {
        #[cfg(target_os = "macos")]
        if let Some(buttons) = &self.buttons {
            return buttons.chrome();
        }
        self.chrome
    }

    /// **This window wears a different band now — put the platform's own
    /// buttons on it** (T-MAC-LIGHTS x T-MAC-PILL, the owner's rulings of
    /// 2026-09-12 read together).
    ///
    /// The band a window wears is not fixed for its life: Folio's own 40-point
    /// strip stands across the top while the tabs run along it, and every
    /// layout that puts the tab list down the side wears a header of the
    /// platform's own height instead — and the reader changes between them
    /// without relaunching anything. So this is a door and not an argument to
    /// [`Self::install`]: whoever decides the band says so again every time it
    /// is decided.
    ///
    /// **`bt-app` decides the number and this crate decides what to do with
    /// it.** The height arrives in logical pixels, the unit
    /// [`crate::CustomFrameGeometry`] already speaks, and on a host that draws
    /// nothing in this window's bar there is nothing to place: the answer is
    /// `Ok(())` and the call costs a branch.
    pub fn set_window_band(&self, band_logical_px: f32) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        if let Some(buttons) = &self.buttons {
            buttons.follow_the_band(f64::from(band_logical_px));
        }
        #[cfg(not(target_os = "macos"))]
        let _ = band_logical_px;
        Ok(())
    }

    /// **Answer a press on the empty part of the title bar.** The macOS
    /// `HTCAPTION`: the application has decided this press landed where its own
    /// chrome is not, and this is it saying so — a drag, or, on a double click,
    /// whatever the reader has asked a title bar's double click to do.
    pub fn press_title_bar(&self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            crate::macos_impl::press_title_bar(self.window)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(not_here("a press on the window's own title bar"))
        }
    }

    /// Whether the window is inside a system move/resize loop. macOS has no
    /// modal loop of that kind to be inside, so: no.
    #[must_use]
    pub fn in_size_move(&self) -> bool {
        false
    }

    /// The boxes the application draws in the title bar, for the caption's hit
    /// test. There is no native hit-test message here to answer: the application
    /// decides inside the press and says so through [`Self::press_title_bar`],
    /// off the very same list (§13.11 ⑥).
    pub fn set_title_bar_boxes(&self, boxes: &[[i32; 4]]) {
        let _ = boxes;
    }

    /// The smallest client the window may be dragged to. The system frame
    /// enforces its own; refusing would be one line of stderr every time a pane
    /// is added, so this is an N and not an X.
    pub fn set_min_client_size(&self, logical: Option<(u32, u32)>) -> Result<(), String> {
        let _ = logical;
        Ok(())
    }
}

/// [`CustomWindowFrame::install`]'s macOS half, **reported rather than
/// propagated** (§13.8 ②).
///
/// A free function and not a line of `install`, because `install` is step 5 of
/// the startup path and may not refuse: a window that failed to open is a
/// launch with nothing on the screen and nothing said. The refusal this can
/// meet is one of the two the AppKit module makes — asked off the window's
/// thread, or handed a view that is in no window — and neither is reachable
/// from the constructor that calls it.
///
/// **The answer on the refusing path is coherent rather than convenient.** If
/// the title bar could not be taken over, then it was not taken over: the
/// window still wears the frame the system gave it, the traffic lights are
/// still in a bar of their own above Folio's, and the run Folio draws in its
/// own bar is the run it drew before this ticket. That is a window that looks
/// wrong and says why on stderr, not a window that has been quietly told the
/// platform draws buttons it does not.
#[cfg(target_os = "macos")]
fn adopt_platform_chrome(
    window: NativeWindow,
    bar_logical_px: f64,
    wake: Box<dyn Fn()>,
) -> (
    crate::PlatformChrome,
    Option<crate::macos_impl::WindowButtonsWatch>,
) {
    match crate::macos_impl::adopt_window_chrome(window, bar_logical_px, wake) {
        Ok(adopted) => adopted,
        Err(reason) => {
            eprintln!("bt-platform: {reason}; the window keeps the title bar the system gave it");
            (crate::PlatformChrome::FOLIO_DRAWS_THE_WHOLE_BAR, None)
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn adopt_platform_chrome(
    window: NativeWindow,
    bar_logical_px: f64,
    wake: Box<dyn Fn()>,
) -> crate::PlatformChrome {
    // The wake is dropped here rather than refused: a window whose bar the
    // platform draws nothing in has nothing that can change about it, so there
    // is nothing this host would ever ask a turn for.
    let _ = (window, bar_logical_px, wake);
    crate::PlatformChrome::FOLIO_DRAWS_THE_WHOLE_BAR
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
///
/// **Off on macOS since M4-6**, where `macos_notify::Taskbar` writes the badge
/// on the real tile.
#[cfg(not(any(windows, target_os = "macos")))]
pub struct Taskbar {
    /// Never constructed: [`Taskbar::new`] refuses.
    _never: std::convert::Infallible,
}

#[cfg(not(any(windows, target_os = "macos")))]
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
/// What is lost where this arm still stands: the reader switching the system
/// between light and dark while Folio is open is not noticed. The theme is still
/// read once at startup. **On macOS it no longer stands** — `macos_impl` is the
/// arm there, and it observes the application's appearance and the
/// accessibility display options.
#[cfg(not(any(windows, target_os = "macos")))]
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

#[cfg(not(any(windows, target_os = "macos")))]
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
///
/// **Off on macOS since M4-6**, where `macos_notify::Notifier` really speaks to
/// the notification centre — and where the unbundled refusal this arm describes
/// is the one that arm now gives, in its own words and for the same reason.
#[cfg(not(any(windows, target_os = "macos")))]
pub struct Notifier {
    /// Never constructed: [`Notifier::new`] refuses.
    _never: std::convert::Infallible,
}

#[cfg(not(any(windows, target_os = "macos")))]
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
    /// An exported settings file (0.4.4 ticket 05: `Settings ▸ Import…`),
    /// filtered to `*.json` — the one type an export is written as.
    SettingsFile,
}

/// The public name for [`ShellPickKind`], as on Windows.
///
/// An alias and not a second enum: the inventory's §6 ⑤ warns that the public
/// name and the defined name differ here and that a mechanical rename would
/// break it.
pub type FilePickKind = ShellPickKind;

/// **The formula context menu, on a platform with no menu to pop** (M2-3 wrote
/// the macOS arm; the menu itself is §7.20's).
///
/// Step 7 of the startup path and one of the seven fatal `?`. Constructed
/// harmlessly; [`Self::request`] is where the refusal lives, and its caller
/// already turns a refusal into a toast rather than into a dead window.
///
/// **This is the Linux-only arm now.** macOS pops a real `NSMenu`
/// (`macos_dialogs::MathContextMenu`), so on that platform this type is not
/// compiled at all; what is left here is the refusal a target with neither Win32
/// nor AppKit gets, which §4.6 of the plan requires to keep building.
#[cfg(not(any(windows, target_os = "macos")))]
pub struct MathContextMenu {
    #[expect(dead_code, reason = "M2-3 pops the menu over this window")]
    window: NativeWindow,
}

#[cfg(not(any(windows, target_os = "macos")))]
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

/// **The folder chooser, on a platform with no panel to put up** (M2-3 wrote
/// the macOS arm).
///
/// Step 8 of the startup path. As [`MathContextMenu`]: constructed harmlessly,
/// refuses when asked, and **Linux-only** — macOS sheets a real `NSOpenPanel`
/// onto the window (`macos_dialogs::FolderPicker`).
///
/// X-4's warning to M2-3 is what shaped that arm and is recorded where it acts:
/// AppKit's modal loops do not drain the main dispatch queue, so a panel run
/// from the wrong place stops the event loop that would answer it. The answer
/// there was a sheet, which is not a modal loop at all.
#[cfg(not(any(windows, target_os = "macos")))]
pub struct FolderPicker {
    #[expect(dead_code, reason = "M2-3 sheets the panel onto this window")]
    window: NativeWindow,
}

#[cfg(not(any(windows, target_os = "macos")))]
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

/// **The picture and program chooser, on a platform with no panel to put up**
/// (M2-3 wrote the macOS arm).
///
/// Step 9 of the startup path. As [`FolderPicker`], including being Linux-only:
/// macOS has `macos_dialogs::ImagePicker`, which restricts the picture row to
/// [`crate::IMAGE_FILE_EXTENSIONS`] through `allowedContentTypes` and leaves the
/// program row unfiltered.
#[cfg(not(any(windows, target_os = "macos")))]
pub struct ImagePicker {
    #[expect(dead_code, reason = "M2-3 sheets the panel onto this window")]
    window: NativeWindow,
}

#[cfg(not(any(windows, target_os = "macos")))]
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

/// **The save dialog, on a platform with no panel to put up** (0.4.4 ticket 05
/// wrote the Windows and macOS arms).
///
/// As [`ImagePicker`]: constructed harmlessly, refuses when asked, and
/// **Linux-only** — macOS sheets a real `NSSavePanel` onto the window
/// (`macos_dialogs::SaveFilePicker`).
#[cfg(not(any(windows, target_os = "macos")))]
pub struct SaveFilePicker {
    #[expect(dead_code, reason = "a Linux arm would put its panel over this window")]
    window: NativeWindow,
}

#[cfg(not(any(windows, target_os = "macos")))]
impl SaveFilePicker {
    /// Install the deferred dialog. Never fails.
    pub fn new(window: NativeWindow) -> Result<Self, String> {
        Ok(Self { window })
    }

    /// Ask for the dialog. Refused.
    pub fn request(&self, start: Option<&Path>, name: &str) -> Result<bool, String> {
        let _ = (start, name);
        Err(not_here("the save dialog"))
    }

    /// The dialog's answer. There is never one.
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

// ── the directory watch (M2-1 for everything but macOS) ────────────────────
//
// M2-1 gave macOS a real arm over FSEvents (`macos_watch.rs`), so the three
// doors below are now what a third platform meets and nothing else. They keep
// the shape rather than the behaviour: the contracts are three constructors and
// the enum is not part of the interface, which is what a Linux arm will have to
// preserve when it is written.

/// **What one completion of the watch said changed.**
///
/// The same two answers FSEvents gives — named entries, or *more than I could
/// write down* — which is why the enum travels unchanged. M2-1 owns all three
/// of the contracts the three constructors carry.
#[cfg(not(any(windows, target_os = "macos")))]
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
#[cfg(not(any(windows, target_os = "macos")))]
pub struct DirWatch {
    /// Never constructed: every constructor refuses.
    _never: std::convert::Infallible,
}

#[cfg(not(any(windows, target_os = "macos")))]
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
#[cfg(not(any(windows, target_os = "macos")))]
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
#[cfg(not(any(windows, target_os = "macos")))]
pub fn get_window_rect(window: NativeWindow) -> Result<WindowRect, String> {
    let _ = window;
    Err(not_here("reading a window's rectangle"))
}

/// Place the window's outer rectangle. `setFrame:display:`; M1-3.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn set_window_outer_rect(window: NativeWindow, rect: WindowRect) -> Result<(), String> {
    let _ = (window, rect);
    Err(not_here("placing a window"))
}

/// The quake drop — place the window and read it back until it agrees. M1-3.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn stand_window_at(window: NativeWindow, rect: WindowRect) -> Result<(), String> {
    let _ = (window, rect);
    Err(not_here("standing a window at a rectangle"))
}

/// The work area of the display this window is on. `NSScreen.visibleFrame`; M1-3.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn get_work_area(window: NativeWindow) -> Result<WindowRect, String> {
    let _ = window;
    Err(not_here("reading a display's work area"))
}

/// The work area of the display under a point. M1-3.
#[cfg(not(any(windows, target_os = "macos")))]
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
#[cfg(not(any(windows, target_os = "macos")))]
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
#[cfg(not(any(windows, target_os = "macos")))]
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
#[cfg(not(any(windows, target_os = "macos")))]
pub fn get_dpi_for_window(window: NativeWindow) -> Result<u32, String> {
    let _ = window;
    Err(not_here("a second opinion about a window's backing scale"))
}

/// Which display a point is on, as a stable name. M1-3, and R5's case-folding
/// question is M3-4's.
#[cfg(not(any(windows, target_os = "macos")))]
#[must_use]
pub fn monitor_id_at(x: i32, y: i32) -> Option<String> {
    let _ = (x, y);
    None
}

/// Where the pointer is, in screen coordinates. `NSEvent.mouseLocation`,
/// flipped; M1-3.
#[cfg(not(any(windows, target_os = "macos")))]
#[must_use]
pub fn pointer_position() -> Option<(i32, i32)> {
    None
}

/// Where the pointer is inside one window's client area, in physical pixels from
/// its top-left corner. `NSEvent.mouseLocation` put through the window and the
/// view; GitHub issue #1 ②.
#[cfg(not(any(windows, target_os = "macos")))]
#[must_use]
pub fn pointer_position_in_window(window: NativeWindow) -> Option<(i32, i32)> {
    let _ = window;
    None
}

/// Which top-level window the window manager puts under a screen point. M1-3.
#[cfg(not(any(windows, target_os = "macos")))]
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

/// **Let the system translate touch into the mouse** — and off Windows there
/// is nothing to let (N, a harmless no-op; owner ruling 2026-09-21).
///
/// On Windows this undoes a registration winit makes and hands four messages
/// back to `DefWindowProc`, because that registration switches the system's own
/// gesture engine off for the window. Neither half has a subject here.
///
/// **macOS never took the engine away.** A trackpad's clicks, drags and
/// two-finger scrolling arrive as ordinary `NSEvent` mouse and scroll-wheel
/// events, which winit already delivers as `CursorMoved`, `MouseInput` and
/// `MouseWheel` — the very events this program has always read — and a Mac with
/// a touch screen does not exist. So there is nothing to unregister, nothing to
/// route around, and no ticket behind this: it is not deferred work, it is work
/// the platform does.
///
/// The `report` is dropped for that reason rather than kept: it says *a touch
/// arrived and was handed over*, and a host that hands nothing over would be
/// reporting a road it has not got. `panned` is dropped for the same reason: a
/// trackpad's two-finger scroll already arrives as `MouseWheel` with a
/// `PixelDelta`, so there is no pan gesture here for anything to answer.
pub fn let_the_system_translate_touch(
    window: NativeWindow,
    report: Box<dyn Fn()>,
    panned: Box<dyn Fn(crate::PanStep)>,
) -> Result<(), String> {
    let _ = (window, report, panned);
    Ok(())
}

/// Whether this window is iconic. `isMiniaturized`; M1-3.
///
/// **`false` and not a refusal**, because the type has no empty answer and the
/// direction matters: a window wrongly called minimised has its geometry
/// thrown away, and a window wrongly called normal has its geometry saved. One
/// of those two is recoverable.
#[cfg(not(any(windows, target_os = "macos")))]
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
#[cfg(not(any(windows, target_os = "macos")))]
#[must_use]
pub fn window_is_exposed(window: NativeWindow) -> bool {
    let _ = window;
    true
}

/// Put this window in front and give it the keyboard. `makeKeyAndOrderFront:`;
/// M1-3.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn take_keyboard_focus(window: NativeWindow) -> Result<(), String> {
    let _ = window;
    Err(not_here("taking the keyboard focus"))
}

/// Ask the window to close. `performClose:`; M1-3.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn request_window_close(window: NativeWindow) -> Result<(), String> {
    let _ = window;
    Err(not_here("asking a window to close"))
}

/// Keep this window above the others. `NSWindow.level`; M1-3.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn set_window_topmost(window: NativeWindow, topmost: bool) -> Result<(), String> {
    let _ = (window, topmost);
    Err(not_here("keeping a window above the others"))
}

/// Ask the system to draw this window's chrome dark. `NSAppearance`; M1-3.
#[cfg(not(any(windows, target_os = "macos")))]
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
#[cfg(not(any(windows, target_os = "macos")))]
pub fn install_window_class_background(
    window: NativeWindow,
    rgb: Option<[u8; 3]>,
) -> Result<(), String> {
    let _ = (window, rgb);
    Err(not_here("the window's backing colour"))
}

/// Call the reader's eye to this window. `NSApp.requestUserAttention:`; M4-6.
///
/// **Off on macOS since M4-6**, where that is exactly what the arm in
/// `macos_notify` calls.
#[cfg(not(any(windows, target_os = "macos")))]
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
///
/// **Off on macOS since M4-6**, where the arm in `macos_notify` reads the
/// Dock's own `autohide` preference and keeps this same direction for the case
/// where it cannot.
#[cfg(not(any(windows, target_os = "macos")))]
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
#[cfg(not(any(windows, target_os = "macos")))]
pub fn wheel_scroll_amount() -> Result<WheelScrollAmount, String> {
    Err(not_here("the wheel's scroll preference"))
}

/// Whether the reader has asked for animation. *Reduce Motion*; M1-3.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn client_area_animation_enabled() -> Result<bool, String> {
    Err(not_here("the reduce-motion preference"))
}

/// Whether the system is in light mode. `AppleInterfaceStyle`; M1-3.
///
/// `None` is "the system has no opinion", which the theme resolver already
/// handles: a `System` theme with no system answer takes the product's default.
#[cfg(not(any(windows, target_os = "macos")))]
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
#[cfg(not(any(windows, target_os = "macos")))]
#[must_use]
pub fn os_ui_language() -> String {
    "en".to_owned()
}

// ── the shell, the trash, the volume (M2-2) ────────────────────────────────

/// Move a path to the trash, on a platform whose desktop has not been asked.
///
/// **`macos_files` is where this really happens** (M2-2). What is left here is
/// the third platform's answer, and it is a refusal rather than a
/// `remove_file`: the product's own sentence is that a deleted file goes
/// somewhere it can be fetched back from, and a door that quietly destroyed one
/// because this platform has no trash implementation would be keeping the
/// signature and breaking the promise. X11 desktops do have a trash — the
/// freedesktop.org spec puts it at `$XDG_DATA_HOME/Trash` with a `.trashinfo`
/// file per entry — and writing it is a Linux backend's decision, which this
/// workspace has not scheduled.
#[cfg(not(target_os = "macos"))]
pub fn recycle(path: &Path) -> Result<bool, String> {
    let _ = path;
    Err(not_here("moving a file to the trash"))
}

/// **The monospaced families this machine has**, on a platform with no font
/// enumeration written (M2-4).
///
/// A `Vec` and not a refusal, because the type has no empty answer and the
/// caller is a picker that has to draw something: what comes back is
/// [`crate::order_monospace_families`]'s guarantee and nothing else — one row,
/// the family the renderer is already drawing. That is exactly what
/// `bt-app`'s settings page did behind its own `#[cfg(not(windows))]` until
/// M2-4; the arm moved here so that the application names one function on every
/// platform and asks no question about the machine it is on.
#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn monospace_font_families() -> Vec<crate::MonospaceFamily> {
    crate::order_monospace_families(Vec::new())
}

/// **One family by name**, on a platform with no font system written (ticket
/// 50). `None`: there is no family this arm can locate, and the renderer then
/// draws the face it falls back to — the same one-row answer
/// [`monospace_font_families`] gives here.
#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn monospace_family_named(name: &str) -> Option<crate::MonospaceFamily> {
    let _ = name;
    None
}

/// No portable font-database enumeration is available on this target.
#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn cjk_font_families() -> Vec<crate::CjkFamily> {
    Vec::new()
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
///
/// **M3-7 measured what that means for the two launches and left the body
/// empty.** Started from a terminal, this process inherits the tty on all three
/// descriptors before its first instruction; started by LaunchServices — `open
/// -a`, a double click in Finder, the Dock — it inherits `/dev/null` on all
/// three, which is launchd's own answer and not an absence this call could
/// repair. Either way the streams already point where the launcher put them,
/// which is exactly the state the Windows arm spends `AttachConsole` and
/// `SetStdHandle` reaching. The two calls the Windows arm makes have no Unix
/// counterpart because the thing they build is already built.
///
/// So the whole of the ordering in `main` is still right, and for the same
/// reason it was written: everything above `diagnostics::enter_resident_run`
/// answers the command somebody just typed — a `--help`, a refused flag, a
/// trace they asked for by name — on whatever the launcher gave this process,
/// and everything below it belongs in the log file [`redirect_std_streams_to_file`]
/// installs. A Finder launch answers the front door into `/dev/null`, which is
/// correct: nobody typed a command, so there is no answer owed to a screen.
pub fn adopt_parent_console() {}

/// **Leave that console's process group.**
///
/// The other half of the same fact: there is no console membership here to
/// leave, so nothing is left. The `bool` says so, and `false` is the same word
/// the Windows arm answers when nothing happened — `FreeConsole` on a process
/// that never attached to one fails, which is the double-click case and is not
/// a failure there either.
///
/// **Nothing reads it**, on any platform: `bt_app::diagnostics` calls this for
/// its effect and drops the answer. Both halves of that are pinned — this arm by
/// `macos_stdio_tests::the_two_console_doors_stay_the_no_ops_their_class_says_they_are`
/// and the caller by `bt_app::diagnostics`' own
/// `nothing_branches_on_the_two_console_no_ops`. That is the whole of why this
/// door may be a no-op off Windows while
/// [`redirect_std_streams_to_file`] beside it may not: the `bool` this one
/// returns branches nothing, and the `bool` that one returns chooses the
/// channel the rest of the run talks on.
///
/// The Unix half of the fault the Windows arm exists for — a `Ctrl+C` typed at
/// the parent shell reaching this process, because it is in that shell's
/// foreground process group — is a *signal*, and its door is
/// [`install_console_ctrl_handler`]. It is not closed here; see
/// `docs/DESIGN.md` §13.23 for what that ticket left open and why.
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

/// **Point this process's `stdout` and `stderr` at a file, for good** (M3-7).
///
/// §4.4 ③ calls this load-bearing and it is: `bt_app::diagnostics` picks
/// `Channel::Log` or `Channel::Nowhere` off this `bool`, so an arm that
/// answered `true` without redirecting anything would put the run's diagnostics
/// nowhere at all while claiming a log file that stays empty. It is the one
/// thing a Finder-launched Folio has instead of a screen: launchd gives that
/// process `/dev/null` on all three descriptors, so until this call lands every
/// `eprintln!` in the workspace is thrown away by the kernel.
///
/// **`dup2` and not a Rust-side writer**, which is the Windows arm's reason
/// turned into the Unix spelling: the point is the *channel* and not the call
/// sites, and several hundred `eprintln!` across this workspace must not each
/// have to know where diagnostics go. Renumbering descriptor 1 and descriptor 2
/// moves every one of them at once and for the rest of the run, including the
/// writes of any library linked into this process and of anything that inherits
/// these descriptors.
///
/// **Append, and `0o600`.** `O_APPEND` is the kernel's own append — every write
/// goes to the end of the file whichever thread issues it, with no seek of its
/// own to race, which is the same guarantee `FILE_APPEND_DATA` buys on the other
/// platform. The mode is this user's alone because a `diagnostics.log` carries
/// window titles, file paths and shell output, and the directory it is created
/// in is a home directory on a machine that may have several people on it. A
/// file that already exists keeps the mode it has; `open` applies this one only
/// to a file it creates, which is the right half of the promise to make — the
/// other half would be this call changing the permissions of a file somebody
/// deliberately opened up.
///
/// **The descriptor is never closed, on any path.** On the path that works it
/// *is* the process's diagnostic stream and lives exactly as long as the
/// process, which is word for word what the Windows arm says of its handle. On
/// the path that does not, closing it would be a call that has just refused to
/// move anything reaching for a descriptor `open` may have been handed *as*
/// descriptor 1 — the state of a process started with its standard streams
/// closed — and taking out the stream it was asked to move.
///
/// **A refusal leaves both descriptors where they were**, which is what lets the
/// caller's `else` mean something: the saved duplicate is taken *before*
/// anything moves, and if the second `dup2` fails the first is put back from it.
/// `F_DUPFD_CLOEXEC` with a floor of 3 rather than `dup`, for two reasons that
/// are both about which number comes back: `dup` answers the lowest free
/// descriptor, which on a process started with its standard error closed is 2 —
/// the descriptor the next line is about to write — and it does not set
/// close-on-exec, which would put a spare copy of somebody's terminal into every
/// shell this window opens.
#[cfg(unix)]
pub fn redirect_std_streams_to_file(path: &Path) -> bool {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::IntoRawFd;

    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
    else {
        return false;
    };
    // Rust opens every file close-on-exec, so the log itself needs no flag of
    // its own; what a pane's child inherits is descriptors 1 and 2, which is
    // the `bt-pty` layer's business and is unchanged by this.
    let log = file.into_raw_fd();
    // SAFETY: the descriptor is this process's own standard output, named by
    // the platform's constant. `fcntl` reads it and answers a new descriptor or
    // `-1`; nothing is borrowed and nothing is freed.
    let kept = unsafe { libc::fcntl(libc::STDOUT_FILENO, libc::F_DUPFD_CLOEXEC, 3) };
    if kept == -1 {
        return false;
    }
    // SAFETY: both arguments are descriptors this process holds — `log` from
    // the open above, the target from the platform's constant — and `dup2`
    // takes ownership of neither: it renumbers, and the caller keeps both.
    let moved = unsafe { libc::dup2(log, libc::STDOUT_FILENO) } != -1
        // SAFETY: as above, for the other stream.
        && unsafe { libc::dup2(log, libc::STDERR_FILENO) } != -1;
    if !moved {
        // SAFETY: `kept` is the duplicate taken above and names what standard
        // output was before the line that moved it.
        let _ = unsafe { libc::dup2(kept, libc::STDOUT_FILENO) };
    }
    // SAFETY: `kept` is this function's own descriptor, is at least 3 by the
    // floor above, and is not read again.
    let _ = unsafe { libc::close(kept) };
    moved
}

/// **The same door where there are no descriptors to renumber.**
///
/// `false`, honestly, and the caller's `else` is then right: the channel becomes
/// `Nowhere` and [`silence_std_streams`] beside it does nothing either, so the
/// streams stay where the launcher put them. No such target is built from this
/// workspace today — the plan's §4.6 names Windows, macOS and a Linux server —
/// and this arm is here so that the pair below it is a platform question rather
/// than a `libc` that has to exist everywhere.
#[cfg(not(unix))]
pub fn redirect_std_streams_to_file(path: &Path) -> bool {
    let _ = path;
    false
}

/// **Put these bytes on this process's standard error without taking Rust's
/// shared `Stderr` lock** (X-7).
///
/// For the one writer that can be stuck in this call for seconds: the trace
/// sink's thread, writing a batch to a terminal whose reader has stopped
/// reading. `eprintln!` reaches the same descriptor through one process-wide
/// lock, so a writer parked inside `write` while holding it parks every other
/// thread that says anything — the window thread included, which is the fault
/// the sink was built to remove, moved one layer out. Writing the descriptor
/// directly leaves those threads to wait on the *device* they chose and never
/// on this one's turn at a mutex.
///
/// `EINTR` is retried because a signal is not an answer about the bytes. A
/// short write is continued from where the kernel stopped; a `write` that
/// reports nothing written without an error is a descriptor that will never
/// take them, and looping on it would be the wait this refuses.
///
/// Answers whether every byte reached the descriptor.
#[cfg(unix)]
pub fn write_std_error(bytes: &[u8]) -> bool {
    let mut rest = bytes;
    while !rest.is_empty() {
        // SAFETY: the pointer and length name this call's own slice, which
        // outlives the synchronous call; the descriptor is the platform's
        // constant and is not owned, borrowed or closed here.
        let written = unsafe {
            libc::write(
                libc::STDERR_FILENO,
                rest.as_ptr().cast::<libc::c_void>(),
                rest.len(),
            )
        };
        if written < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return false;
        }
        let Ok(written) = usize::try_from(written) else {
            return false;
        };
        let Some(remaining) = rest.get(written..) else {
            return false;
        };
        if remaining.len() == rest.len() {
            return false;
        }
        rest = remaining;
    }
    true
}

/// **The same door where there is no descriptor to write to.**
///
/// The pair to [`redirect_std_streams_to_file`]'s `not(unix)` arm, and the same
/// honesty: no such target is built from this workspace today, and a build for
/// one is better told that nothing was written than given a lock this function
/// exists to avoid.
#[cfg(not(unix))]
pub fn write_std_error(bytes: &[u8]) -> bool {
    let _ = bytes;
    false
}

/// **Send `stdout` and `stderr` nowhere at all** (M3-7).
///
/// The floor under [`redirect_std_streams_to_file`], and the Unix spelling of
/// the same rule: a run whose log file cannot be opened must not fall back to
/// the terminal, because the terminal is the one destination that belongs to
/// somebody else — a Folio started from a shell is very often started from a
/// pane inside another Folio, which is the report the whole channel came from.
///
/// `/dev/null` is that platform's spelling of a descriptor that reports every
/// byte written and keeps none, which is exactly what a null standard handle
/// does on Windows: `eprintln!` stays a no-op rather than becoming a panic, and
/// no call site has to know.
///
/// The descriptor is not closed, for [`redirect_std_streams_to_file`]'s reason.
/// A `/dev/null` that could not be opened leaves the streams alone rather than
/// closing them: a closed descriptor 2 is the state where the *next* file this
/// process opens becomes its standard error, which is a worse answer than a
/// diagnostic somebody can see.
#[cfg(unix)]
pub fn silence_std_streams() {
    use std::os::unix::io::IntoRawFd;

    let Ok(sink) = std::fs::OpenOptions::new().write(true).open("/dev/null") else {
        return;
    };
    let sink = sink.into_raw_fd();
    for slot in [libc::STDOUT_FILENO, libc::STDERR_FILENO] {
        // SAFETY: both arguments are descriptors this process holds — `sink`
        // from the open above, the target from the platform's constant — and
        // `dup2` renumbers rather than taking ownership of either.
        let _ = unsafe { libc::dup2(sink, slot) };
    }
}

/// **The same door where there is no `/dev/null` to point at.**
///
/// Nothing, which is right beside the `false` above: streams that were never
/// moved are still the launcher's, and silencing them would throw away the only
/// diagnostics such a platform has.
#[cfg(not(unix))]
pub fn silence_std_streams() {}

/// **A handler for the interrupt the console sends** (M3-7 left this one open).
///
/// `SIGINT` and `SIGTERM` are the Unix shape. `false` says no handler was
/// installed, which is what the caller records — and it is still the answer
/// after M3-7, deliberately: what the two signals should *do* is a decision and
/// not a translation. `SIGINT` is the one the Windows arm refuses, and refusing
/// it here is the same sentence; `SIGTERM` is not, and on this platform it is
/// how the system asks an application to go away at logout and at shutdown, so
/// a handler for it is a path that has to write the session document and leave
/// through [`leave_process`] — which is M3-1's application delegate and M3-5's
/// single writer, neither of which exists yet. `docs/DESIGN.md` §13.23 books it
/// rather than guessing at it.
pub fn install_console_ctrl_handler() -> bool {
    false
}

/// **End this process, now** (M3-7).
///
/// Really done, and it cannot be anything else: §4.4 ③ names this as a return
/// type with no empty answer.
///
/// **`std::process::exit` and not the Windows arm's `TerminateProcess`.** That
/// call is there for one measured reason and the reason is a tenant: a process
/// that has loaded the Edge WebView2 client DLL cannot walk out through the
/// loader's `DLL_PROCESS_DETACH`, because Chromium's detach path expects an
/// apartment that still pumps and threads that are still alive. Nothing on this
/// platform is that tenant today, so the ordinary exit is the honest door, and
/// it is also the better one: it runs the `atexit` chain, which is where a
/// library that registered a handler gets its turn.
///
/// **The buffers first, and that line is not redundant.** `std::process::exit`
/// does flush `stdout` on the way out, through the standard library's own
/// cleanup — but that is a property of *this* exit primitive, and the one thing
/// every caller of this function is promised is that the last line it wrote is
/// on the disk. Writing the flush here makes it a property of `leave_process`
/// instead, so that an arm which later has to leave by a faster door — `_exit`,
/// which the backend inventory's own row anticipates for the day a WKWebView is
/// in this process — cannot take the run's footer with it. It is the same two
/// lines the Windows arm opens with, for the same sentence.
///
/// **Everything else that had to be flushed already was, and none of it by this
/// platform's accident.** `bt_pty`'s recording writes through an unbuffered
/// `File` and publishes with `sync_data` on its own thread; the session document
/// is written and its sentinel removed inside the loop, above every caller of
/// this function; and the run's footer is an `eprintln!`, which Rust does not
/// buffer. The order in `bt_app::main` is therefore the order on both platforms.
pub fn leave_process(code: i32) -> ! {
    use std::io::Write;

    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(code)
}

/// **The last-resort fault report, where there is nothing to raise a box with**
/// (M2-3 gave macOS `NSAlert`).
///
/// Really done, to stderr, and not deferred — this is what a process says when
/// it has already failed to open a window, and a refusal here would mean the
/// one message that matters is the one nobody sees.
///
/// **Linux-only now**, and the two lines it writes are the two the macOS arm
/// falls back to itself: `macos_dialogs::message_box` raises an `NSAlert` when
/// it is on the main thread and an application exists, and says exactly this
/// when it is not, because a panic hook runs on whichever thread panicked and a
/// refused launch runs before there is any application at all.
#[cfg(not(any(windows, target_os = "macos")))]
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
        #[cfg(not(target_os = "macos"))]
        assert!(Compositor::new(window()).is_ok(), "the visual tree");
        assert!(
            CustomWindowFrame::install(
                window(),
                crate::CustomFrameGeometry {
                    title_bar_logical_px: 40,
                },
                Box::new(|| {}),
            )
            .is_ok(),
            "the self-drawn frame"
        );
        // The three dialog services are only this module's where no backend
        // has them: M2-3 wrote the macOS arm, and the constructors that have to
        // be harmless there are `macos_dialogs`' own — held by
        // `a_dialog_door_asked_off_the_window_thread_refuses`, which builds all
        // three before it asks any of them for anything.
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            assert!(MathContextMenu::new(window()).is_ok(), "the formula menu");
            assert!(FolderPicker::new(window()).is_ok(), "the folder chooser");
            assert!(ImagePicker::new(window()).is_ok(), "the picture chooser");
            assert!(SaveFilePicker::new(window()).is_ok(), "the save dialog");
        }
        // The settings watch is only this module's where no backend has one:
        // on macOS it is `macos_impl`'s, and the constructor that has to be
        // harmless there is that one (`a_window_door_asked_off_the_window_thread_refuses`
        // is where it is held).
        #[cfg(not(any(windows, target_os = "macos")))]
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
    ///
    /// **Not the three dialogs on macOS**, where none of them is this module's
    /// any more: what the same three doors answer there is not "not on this
    /// platform" but a real panel, and the refusal that replaces this claim is
    /// `macos_dialogs`' `a_dialog_door_asked_off_the_window_thread_refuses` —
    /// the same shape about the same doors, with the thread as the thing that is
    /// wrong instead of the platform.
    #[test]
    fn a_deferred_service_that_is_not_on_this_platform_refuses_when_invoked_not_at_startup() {
        #[cfg(not(any(windows, target_os = "macos")))]
        {
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

            let save = SaveFilePicker::new(window()).expect("built above");
            assert!(
                save.request(None, "folio-settings.json").is_err(),
                "there is no save panel to sheet"
            );
            assert!(save.take_result().is_none());
        }

        // The composition is only this module's where no backend has one:
        // M4-1 wrote the macOS arm, and what holds the same doors there is
        // `macos_compose`'s own suite and its `.app` proof.
        #[cfg(not(target_os = "macos"))]
        {
            let compositor = Compositor::new(window()).expect("built above");
            let page = PageVisual { tab: 1, seat: 1 };
            assert!(
                compositor.attach_web_visual(page).is_err(),
                "there is no visual tree to put a page in"
            );
            // And the frame's own calls are the no-ops a frame makes, not
            // refusals: one line of stderr per present is not a diagnostic, it
            // is a fault.
            assert!(
                compositor.commit().is_ok(),
                "committing nothing costs nothing"
            );
            assert!(
                compositor.set_window_size(800, 600).is_ok(),
                "a resize tells the tree nothing and succeeds at it"
            );
        }
    }

    /// PIN — **the window doors refuse rather than pretending.**
    ///
    /// The reads and writes M1-3 owns. Each one is a place where answering
    /// `Ok(())` would leave `bt-app` believing a window had been placed,
    /// focused or closed.
    ///
    /// **Not compiled on macOS**, where none of these names is this module's any
    /// more: M1-3 wrote the arm, and what holds the same six doors there is
    /// `macos_impl`'s own suite.
    #[cfg(not(any(windows, target_os = "macos")))]
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

/// **The stdio door, made to move a real stream and then put it back** (M3-7,
/// `docs/DESIGN.md` §13.23).
///
/// The behavioural twin of `crate::macos_stdio_tests`, which is a source pin and
/// runs on the Windows workstation where this module is not compiled at all.
/// This one runs where the module does, which today is the Mac.
///
/// **It really renumbers this process's own descriptors**, because there is no
/// smaller claim that would be worth making: the whole of what
/// [`redirect_std_streams_to_file`] promises is that a call site which knows
/// nothing about it — an `eprintln!` in another crate — lands in the file
/// afterwards, and a test that redirected some *other* descriptor would be
/// testing a function this product does not call. So both standard descriptors
/// are duplicated first and put back at the end, which is what makes it safe to
/// do inside a test binary that has its own output to write.
#[cfg(all(test, unix))]
mod stream_tests {
    use std::io::{Read, Write};
    use std::os::unix::io::RawFd;

    /// **One line, written the way the descriptor sees it.**
    ///
    /// Not `eprintln!`, and the reason is libtest rather than this door:
    /// `print!` and `eprint!` both go through `std::io::print_to`, which hands
    /// the bytes to the harness's per-test capture buffer instead of to the
    /// handle whenever a case is being captured — which is every case that is
    /// not run with `--nocapture`. A test written with the macros would
    /// therefore pass with a completely empty implementation of the door, for a
    /// reason that has nothing to do with the door. `std::io::stdout()` and
    /// `std::io::stderr()` are the real handles and resolve descriptors 1 and 2,
    /// which is what every `eprintln!` in a *running* Folio resolves to as well.
    fn say(to_stdout: bool, line: &str) {
        if to_stdout {
            let mut out = std::io::stdout().lock();
            writeln!(out, "{line}").expect("standard output takes bytes");
            out.flush().expect("and the line is not left in the buffer");
        } else {
            let mut err = std::io::stderr().lock();
            writeln!(err, "{line}").expect("standard error takes bytes");
        }
    }

    /// **This process has one pair of standard descriptors, so these cases take
    /// turns.**
    ///
    /// Not tidiness: libtest runs cases on several threads, and a case that read
    /// descriptor 1 while the case beside it had it pointed at a file would be
    /// reading the other case's answer. The lock is held for the whole of each
    /// body, including the restore, which is the only window that matters.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A duplicate of descriptors 1 and 2, and the way back.
    ///
    /// `F_DUPFD_CLOEXEC` with a floor of 3 for the door's own reason: what comes
    /// back must not be one of the two numbers about to be written.
    struct StreamsPutBack {
        out: RawFd,
        err: RawFd,
    }

    impl StreamsPutBack {
        fn taken() -> Self {
            // SAFETY: both are this process's own standard descriptors, named
            // by the platform's constants; `fcntl` reads them and answers a new
            // descriptor or `-1`.
            unsafe {
                Self {
                    out: libc::fcntl(libc::STDOUT_FILENO, libc::F_DUPFD_CLOEXEC, 3),
                    err: libc::fcntl(libc::STDERR_FILENO, libc::F_DUPFD_CLOEXEC, 3),
                }
            }
        }
    }

    impl Drop for StreamsPutBack {
        /// **In `Drop`, so that a failed assertion does not leave the rest of
        /// the run writing into a temporary file.** A panic here unwinds through
        /// a `libtest` that is about to print the failure, and where it prints
        /// it is what this type is for.
        fn drop(&mut self) {
            for (kept, slot) in [
                (self.out, libc::STDOUT_FILENO),
                (self.err, libc::STDERR_FILENO),
            ] {
                if kept == -1 {
                    continue;
                }
                // SAFETY: `kept` is this type's own descriptor and `slot` is the
                // standard one it was taken from.
                unsafe {
                    libc::dup2(kept, slot);
                    libc::close(kept);
                }
            }
        }
    }

    /// RED — **what the workspace writes after the call is in the file, and what
    /// it writes after the silence is nowhere.**
    ///
    /// MUTATIONS: answer `true` without calling `dup2` and the first assertion
    /// goes red with an empty file — which is the exact failure §4.4 ③ says a
    /// no-op arm would hide, a run that claims `Channel::Log` and writes
    /// nothing. Make `silence_std_streams` a no-op and the second goes red,
    /// because the line meant for nowhere is appended to the log instead.
    #[test]
    fn the_streams_move_to_the_file_and_then_to_nowhere() {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|held| held.into_inner());
        let log = std::env::temp_dir().join(format!(
            "folio-m3-7-{}-{:?}.log",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&log);
        let put_back = StreamsPutBack::taken();
        assert!(
            super::redirect_std_streams_to_file(&log),
            "the door refused a file it had just been given the directory of"
        );
        say(false, "M3-7 this line is the log's");
        say(true, "M3-7 and so is this one");
        super::silence_std_streams();
        say(false, "M3-7 this line is nobody's");
        drop(put_back);

        let mut written = String::new();
        std::fs::File::open(&log)
            .expect("the file the streams were pointed at exists")
            .read_to_string(&mut written)
            .expect("and is readable");
        let _ = std::fs::remove_file(&log);
        assert!(
            written.contains("M3-7 this line is the log's"),
            "a write to standard error after the call did not reach the file:\n{written}"
        );
        assert!(
            written.contains("M3-7 and so is this one"),
            "a write to standard output after the call did not reach the file — \
             the door moved one stream and not both:\n{written}"
        );
        assert!(
            !written.contains("M3-7 this line is nobody's"),
            "a line written after the streams were silenced is in the log, so \
             the silence is not one:\n{written}"
        );
    }

    /// RED — **a refusal leaves the streams where they were.**
    ///
    /// The other half of the `bool`, and the half `bt_app::diagnostics` acts on:
    /// a `false` sends it to `silence_std_streams`, and a door that had already
    /// half-moved the streams before refusing would have made that decision for
    /// it. A directory is the refusal that needs no fixture — `open` cannot give
    /// out an appendable file for one on any Unix.
    ///
    /// MUTATION: move the `dup2` calls above the `open` and this goes red — both
    /// descriptors would then name the file the open was about to fail on.
    #[test]
    fn a_refused_redirect_moves_nothing() {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|held| held.into_inner());
        let put_back = StreamsPutBack::taken();
        let before = (names(libc::STDOUT_FILENO), names(libc::STDERR_FILENO));
        let refused = super::redirect_std_streams_to_file(&std::env::temp_dir());
        let after = (names(libc::STDOUT_FILENO), names(libc::STDERR_FILENO));
        drop(put_back);
        assert!(
            !refused,
            "a directory was accepted as this run's diagnostic stream"
        );
        assert_eq!(
            before, after,
            "the refusal renumbered a descriptor on its way out, so the caller's \
             `else` is deciding what to do about streams that have already moved"
        );
    }

    /// What a descriptor points at, as the file system's own identity.
    ///
    /// Device and inode, which is the same question `same_file` asks of two
    /// paths and the only one worth asking of a descriptor: the *number* is
    /// unchanged by a `dup2` and what it names is the whole of what changes.
    ///
    /// Written out as text rather than as a pair of integers because `dev_t` is
    /// signed on one Unix and unsigned on another, and a cast written to make
    /// the two agree would be a cast this comparison does not need: what is
    /// compared is whether the same descriptor still names the same object.
    fn names(slot: RawFd) -> String {
        // SAFETY: the descriptor is one of this process's own standard two, and
        // the out-parameter is a local this call fills and does not keep.
        let stat = unsafe {
            let mut stat = std::mem::zeroed::<libc::stat>();
            assert!(
                libc::fstat(slot, &raw mut stat) == 0,
                "a standard descriptor this process holds could not be read"
            );
            stat
        };
        format!("{}:{}", stat.st_dev, stat.st_ino)
    }
}
