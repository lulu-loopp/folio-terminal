//! **The two choosers, the formula menu and the last-resort alert, on macOS** —
//! the AppKit twin of the four dialog doors in `windows_impl` (ticket M2-3).
//!
//! # What this file is, and why it is not part of `macos_impl`
//!
//! M1-3 moved the window and screen group into [`macos_impl`](super::macos_impl)
//! and left the rest of the portable module refusing by name. Four of those
//! refusals were this ticket's: `FolderPicker`, `ImagePicker`, `MathContextMenu`
//! and `message_box`. They are here rather than next door because the two files
//! answer different questions. Everything in `macos_impl` is a *reading* or a
//! *statement* about a window that already exists — where it is, what scale it
//! is at, what appearance it wears — and every one of those doors returns in the
//! same turn of the loop it was asked in. Everything here **puts something in
//! front of the reader and waits for them**, which is a different shape with a
//! different hazard, and the hazard is the whole design of the file.
//!
//! What they do share is the thread: every call below is AppKit, and AppKit is
//! the main thread's. The gate is `macos_impl`'s own
//! [`window_thread`](super::macos_impl::window_thread) /
//! [`window_for`](super::macos_impl::window_for), reached through the same
//! `MainThreadMarker`, and a door asked from anywhere else refuses with the same
//! sentence rather than reaching AppKit from the wrong thread.
//!
//! # The hazard, and the two different answers it gets here
//!
//! X-4 left M2-3 a warning and it is the reason this is not a two-line port:
//! **AppKit's modal loops do not drain the main dispatch queue**, and more
//! generally a nested event loop started from inside a winit callback runs the
//! application's own event handling while the borrow that started it is still
//! live. That is E55 on the other platform — the reason `windows_impl` defers
//! `TrackPopupMenu` and `IFileDialog::Show` behind a posted message instead of
//! calling them where the press arrives. winit 0.30.13's AppKit backend does
//! guard itself against the re-entrance (`handle_redraw` checks
//! `event_handler.in_use()`, `cleared` checks `event_handler.ready()`), so the
//! failure here is dropped frames rather than a panic — but a chooser that a
//! reader leaves open for a minute would be a minute in which this process
//! drains no pty, publishes no frame and answers no shell. So the hazard is
//! avoided rather than survived, and macOS offers two different ways out of it:
//!
//! * **The panels are not modal at all.** `beginSheetModalForWindow:completionHandler:`
//!   attaches the panel to the window as a sheet and **returns immediately**;
//!   the answer arrives later, in a block AppKit runs on the main thread. So
//!   [`FolderPicker::request`] and [`ImagePicker::request`] really do what the
//!   Windows arm's `PostMessageW` only pretends to: they come back before the
//!   reader has decided anything. There is no nested loop to keep out of a
//!   callback, because there is no nested loop.
//! * **The menu is modal, so it is deferred.** `popUpMenuPositioningItem:atLocation:inView:`
//!   runs its own tracking loop and blocks until the menu closes, exactly like
//!   `TrackPopupMenu`. [`MathContextMenu::request`] therefore only *schedules*:
//!   `-[NSObject performSelector:withObject:afterDelay:]` with a delay of zero,
//!   which is this platform's `PostMessageW` — the selector fires from the run
//!   loop's default mode after the callback that asked for it has returned, and
//!   the tracking loop then starts on a stack with no winit handler on it.
//!
//! # How the answer reaches `bt-app`
//!
//! The same way it does on Windows: it is **parked**, and the next turn of the
//! loop collects it. `Runtime::turn` calls `apply_math_context_menu_result`,
//! `apply_folder_pick_result` and `apply_image_pick_result` one after the other,
//! and each of them is a `take_result()` that answers at most once.
//!
//! What is new here is that the moment the answer lands is **not** inside
//! winit's event handling. On Windows it is: the subclass runs the modal dialog
//! from inside `DispatchMessageW`, so by the time the answer exists the loop is
//! already mid-turn and `about_to_wait` follows on its own — the only thing that
//! had to wake the loop was the *request*, and `PostMessageW` did that. Here the
//! completion block is run by the main run loop directly, with no winit callback
//! anywhere on the stack, so something has to make winit deliver an event.
//!
//! **What this file does: it asks the window for a frame** —
//! `-[NSView setNeedsDisplay:YES]` on the very view winit owns, which is what
//! [`NativeWindow`] holds. AppKit calls `drawRect:` on the next display pass,
//! winit's own view turns that into `WindowEvent::RedrawRequested`, and
//! `about_to_wait` — and therefore `Runtime::turn`, and therefore
//! `take_result()` — follows every delivered event. It is `Window::request_redraw`
//! by another route, chosen because this crate holds the `NSView` and not the
//! winit `Window`, and it is the right statement to make anyway: an answer that
//! has just landed is a change the window has to redraw for.
//!
//! Worth recording, because it is the kind of thing that quietly stops being
//! true: winit 0.30.13 would very probably have got there without being asked.
//! Its control-flow observers are registered in `kCFRunLoopCommonModes`, and
//! the `kCFRunLoopBeforeWaiting` one ends in `Event::AboutToWait` — so *any*
//! turn of the main run loop reaches `bt-app`'s poll, event or no event. That is
//! an implementation detail of one pinned version rather than a contract, and a
//! door whose answer is only collected because of it would be a door that breaks
//! on an upgrade with no compiler error. The redraw is the contract.
//!
//! # One gesture in flight
//!
//! [`Deferred`] is the local twin of `windows_impl`'s `DeferredState` and holds
//! the same rule for the same reason: a second `request` while one is up, or
//! while an answer is waiting to be collected, is refused with `Ok(false)`
//! rather than stacking a second sheet behind the first. It has one phase fewer
//! — there is no `Posted` for the panels, because a sheet is up the moment it is
//! asked for — and the menu's single waiting phase covers both of the Windows
//! arm's.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertStyle, NSApplication, NSEvent, NSMenu, NSMenuItem, NSModalResponse,
    NSModalResponseCancel, NSModalResponseOK, NSOpenPanel, NSView, NSWindow,
};
use objc2_foundation::{NSArray, NSObjectNSDelayedPerforming, NSString, NSURL, ns_string};
use objc2_uniform_type_identifiers::UTType;

use crate::macos_impl::{nothing_to_ask, window_for, window_of, window_thread};
use crate::{IMAGE_FILE_EXTENSIONS, NativeWindow, ShellPickKind};

// ── one gesture in flight, and its answer parked ───────────────────────────

/// Where a deferred gesture is, and — once — what it answered.
#[derive(Debug)]
enum Phase<Answer> {
    /// Nothing asked for; the next request is allowed.
    Idle,
    /// A sheet is on the window, or a menu is scheduled or tracking.
    Waiting,
    /// The answer is here and has not been collected.
    Complete(Answer),
}

/// The gate that keeps one gesture in flight and parks its answer for the turn
/// of the loop that collects it — `windows_impl`'s `DeferredState`, one phase
/// shorter.
///
/// `Mutex` rather than `RefCell` for the Windows arm's reason turned round: on
/// that platform the state really is shared with a subclass procedure, and here
/// it is shared with a block whose thread AppKit chooses. Both ends are in fact
/// the main thread; the lock costs nothing uncontended and it is what makes the
/// type `Sync`, which is what lets the block hold an `Arc` of it at all.
#[derive(Debug)]
struct Deferred<Answer> {
    phase: Mutex<Phase<Answer>>,
}

impl<Answer> Deferred<Answer> {
    fn new() -> Self {
        Self {
            phase: Mutex::new(Phase::Idle),
        }
    }

    /// Claim the one slot. `false` when a gesture is already up or an answer is
    /// still waiting to be taken — the coalescing `Ok(false)` reports.
    fn begin(&self) -> bool {
        let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
        if !matches!(*phase, Phase::Idle) {
            return false;
        }
        *phase = Phase::Waiting;
        true
    }

    /// Give the slot back, for a request that claimed it and could not then be
    /// put up. Nothing is parked: the caller is being told `Err` to its face.
    fn give_up(&self) {
        let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
        if matches!(*phase, Phase::Waiting) {
            *phase = Phase::Idle;
        }
    }

    /// [`Self::give_up`] on the failing half of a `Result`, and the value
    /// through on the other — the twin of `windows_impl`'s `cancel_request`
    /// after a failed `PostMessageW`.
    fn give_back_on_refusal<T>(&self, asked: Result<T, String>) -> Result<T, String> {
        if asked.is_err() {
            self.give_up();
        }
        asked
    }

    /// Park the answer. Ignored unless a gesture really is in flight, so a
    /// completion that arrives twice cannot overwrite an answer nobody read.
    fn complete(&self, answer: Answer) {
        let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
        if matches!(*phase, Phase::Waiting) {
            *phase = Phase::Complete(answer);
        }
    }

    /// The answer, once, and the slot is free again afterwards.
    fn take(&self) -> Option<Answer> {
        let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
        let Phase::Complete(_) = &*phase else {
            return None;
        };
        let Phase::Complete(answer) = std::mem::replace(&mut *phase, Phase::Idle) else {
            unreachable!("phase was matched as complete immediately before replacement")
        };
        Some(answer)
    }
}

/// What a chooser answers: the path, `None` for a cancelled panel, or the reason
/// the panel could not be shown.
type Choice = Result<Option<PathBuf>, String>;

// ── the wake ───────────────────────────────────────────────────────────────

/// **Make winit deliver an event, so that `about_to_wait` collects the answer.**
///
/// The module header says why this is here rather than left to winit's run-loop
/// observer. The view is winit's own — it is what a [`NativeWindow`] holds — so
/// marking it dirty is `Window::request_redraw` reached from the side this crate
/// stands on.
fn ask_for_a_frame(window: NativeWindow, _mtm: MainThreadMarker) {
    // SAFETY: the handle is winit's live `ns_view` pointer, held for as long as
    // the `Window` it came from is alive, and the marker is the proof that this
    // is that window's thread. Identical to `macos_impl::window_of`'s read.
    let view: &NSView = unsafe { window.as_ns_view().as_ref() };
    view.setNeedsDisplay(true);
}

// ── the panel, configured three ways ───────────────────────────────────────

/// The extensions the decoder honours, as the content types a panel filters on.
///
/// **Built from [`IMAGE_FILE_EXTENSIONS`] rather than written out**, which is
/// the inventory's instruction for this row and the same rule
/// `image_file_filter_spec` obeys on the other platform: a chooser that offered
/// a format the decoder refuses is a dialog that lets you pick a file and then
/// says no. `image_file_filter_spec` itself is *not* read here — a
/// `*.png;*.jpg` string is Windows' spelling of the question and there is
/// nothing on this platform to hand it to.
///
/// `+[UTType typeWithFilenameExtension:]` answers `nil` for an extension the
/// system knows no type for, and such an extension is dropped rather than
/// refused: a machine that has never heard of `webp` is a machine whose chooser
/// should still offer `png`, and the decoder is the thing that decides anyway.
/// `the_picture_chooser_offers_every_format_the_decoder_honours` is what stops
/// that tolerance from hiding a list that has silently emptied.
fn image_content_types() -> Vec<Retained<UTType>> {
    IMAGE_FILE_EXTENSIONS
        .iter()
        .filter_map(|extension| UTType::typeWithFilenameExtension(&NSString::from_str(extension)))
        .collect()
}

/// Dress one `NSOpenPanel` as whichever of the three choosers is being asked
/// for.
///
/// One function and three guises rather than three functions, for
/// `show_shell_picker`'s reason: everything around the two lines that differ is
/// identical and is the part that is easy to get subtly wrong three times.
fn dress(panel: &NSOpenPanel, kind: ShellPickKind) {
    // Never more than one, on every row: each of the three answers a single
    // path and a multi-selection would have to be silently truncated.
    panel.setAllowsMultipleSelection(false);
    // An alias resolves to what it points at, which is what `FOS_FORCEFILESYSTEM`
    // buys on the other platform: the answer is a place something can be opened
    // at rather than a pointer nobody downstream knows how to follow.
    panel.setResolvesAliases(true);
    match kind {
        ShellPickKind::Folder => {
            panel.setCanChooseDirectories(true);
            panel.setCanChooseFiles(false);
        }
        ShellPickKind::Image => {
            panel.setCanChooseDirectories(false);
            panel.setCanChooseFiles(true);
            let types = image_content_types();
            panel.setAllowedContentTypes(&NSArray::from_retained_slice(&types));
        }
        ShellPickKind::Program => {
            panel.setCanChooseDirectories(false);
            panel.setCanChooseFiles(true);
            // **Unfiltered on purpose**, for the Windows arm's reason: what may
            // be started is the operating system's answer rather than this
            // dialog's, and a chooser that offered only executables would refuse
            // every shell shipped as a script.
            //
            // **And a `.app` is chosen whole.** With file packages treated as
            // directories the panel would descend into `Terminal.app` and hand
            // back something inside it; with the flag off the bundle is one
            // item, and **the path written into `profiles.json` is the bundle's
            // own** — `/Applications/Foo.app` — which is what `NSWorkspace`
            // (M2-2) opens and what a reader recognises in the row.
            panel.setTreatsFilePackagesAsDirectories(false);
        }
    }
}

/// Open the panel where the caller is looking, if that is still a place.
///
/// `start` is a **folder** on both platforms and for the same reason: a shell
/// item that is a file is not a place to open at. A folder that has since been
/// deleted or unplugged is not this function's failure either — the panel opens
/// at the system's last place instead, which is a perfectly good outcome and not
/// one worth refusing to open over.
fn start_at(panel: &NSOpenPanel, start: Option<&Path>) {
    let Some(start) = start else {
        return;
    };
    let Some(text) = start.to_str() else {
        return;
    };
    if text.is_empty() {
        return;
    }
    let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(text), true);
    panel.setDirectoryURL(Some(&url));
}

/// Put the panel up as a sheet on the window and arrange for its answer.
///
/// The panel is retained by the block, so it is alive for as long as AppKit
/// needs it and is released when the block is — which is the whole of this
/// arm's memory management, and the reason it is not written out anywhere else.
fn sheet(
    panel: Retained<NSOpenPanel>,
    host: &NSWindow,
    window: NativeWindow,
    state: Arc<Deferred<Choice>>,
    noun: &'static str,
) {
    let answering = panel.clone();
    let block = RcBlock::new(move |response: NSModalResponse| {
        state.complete(read_choice(&answering, response, noun));
        // The completion block runs on the main thread — AppKit's contract for
        // it — so the marker is here rather than assumed. Nothing to do if it
        // is not, which is a state this cannot be in.
        if let Some(mtm) = MainThreadMarker::new() {
            ask_for_a_frame(window, mtm);
        }
    });
    panel.beginSheetModalForWindow_completionHandler(host, &block);
}

/// The half of a sheet's answer that is settled by the response alone, and
/// `None` for the one response that means "now go and read the panel".
///
/// Split out so that the distinction the product depends on is decidable
/// without a panel: `bt-app` keeps the row's old value on a cancel and writes a
/// line to `diagnostics.log` on a failure, and the two swapped would silently
/// clear a setting the reader never touched.
fn settled_by_the_response(response: NSModalResponse, noun: &str) -> Option<Choice> {
    if response == NSModalResponseCancel {
        return Some(Ok(None));
    }
    if response != NSModalResponseOK {
        // `NSModalResponseAbort` is the documented answer for a panel that
        // failed to display, and anything else is a response this arm does not
        // know. Both are reported rather than read as a cancel: a reader who
        // never saw a dialog has not chosen to keep what the row had.
        return Some(Err(format!(
            "the {noun} chooser could not be shown (NSModalResponse {response})"
        )));
    }
    None
}

/// What the sheet came back with, in the three shapes `take_result` promises.
fn read_choice(panel: &NSOpenPanel, response: NSModalResponse, noun: &str) -> Choice {
    if let Some(settled) = settled_by_the_response(response, noun) {
        return settled;
    }
    let Some(url) = panel.URL() else {
        // OK with nothing selected is not a shape AppKit produces; if it ever
        // does, it is a cancel by every reading that matters downstream.
        return Ok(None);
    };
    if !url.isFileURL() {
        // `FOS_FORCEFILESYSTEM`'s twin. A panel can be shown things that are not
        // on a volume — and a picture that is not on a volume has no path for
        // the decoder to open.
        return Err(format!("the chosen {noun} is not a file on this machine"));
    }
    let Some(path) = url.path() else {
        return Err(format!("the chosen {noun} has no path of its own"));
    };
    Ok(Some(PathBuf::from(path.to_string())))
}

// ── the folder chooser (M2-3) ──────────────────────────────────────────────

/// **The system's own folder chooser, as a sheet on the window** (M2-3).
///
/// Step 8 of the startup path, and the constructor is harmless for §4.4's
/// reason: it is one of the seven fatal `?` between `main` and the first frame,
/// so a refusal here would be a launch that shows no window at all rather than a
/// row that says why.
///
/// [`Self::request`] answers on the Windows arm's terms exactly: `Ok(true)` when
/// the sheet is up, `Ok(false)` when one already is or an answer is still
/// waiting to be collected, and `Err` when there is no window on this thread to
/// sheet it onto. The panel's own failure is **not** a `request` failure — it
/// arrives as `NSModalResponseAbort` and is parked as the `Err` that
/// [`Self::take_result`] hands over, because by then the caller has already been
/// told a chooser was opened.
pub struct FolderPicker {
    window: NativeWindow,
    state: Arc<Deferred<Choice>>,
}

impl FolderPicker {
    /// Install the chooser. Never fails; there is nothing to install.
    pub fn new(window: NativeWindow) -> Result<Self, String> {
        Ok(Self {
            window,
            state: Arc::new(Deferred::new()),
        })
    }

    /// Put the chooser up once, opening at `start` if that names a folder.
    pub fn request(&self, start: Option<&Path>) -> Result<bool, String> {
        let what = "the folder chooser";
        if !self.state.begin() {
            return Ok(false);
        }
        // The slot is claimed before the window is resolved and given back if it
        // cannot be, which is the order the Windows arm takes for the same
        // reason: a request that is about to be refused to the caller's face
        // must not leave the row unable to ask again.
        let (mtm, host) = self
            .state
            .give_back_on_refusal(window_for(self.window, what))?;
        let panel = NSOpenPanel::openPanel(mtm);
        dress(&panel, ShellPickKind::Folder);
        start_at(&panel, start);
        sheet(panel, &host, self.window, Arc::clone(&self.state), "folder");
        Ok(true)
    }

    /// The chosen folder, `None` for a cancelled sheet, or the reason the panel
    /// could not be shown — once, and only once the sheet is gone.
    pub fn take_result(&self) -> Option<Choice> {
        self.state.take()
    }
}

// ── the picture and program chooser (M2-3) ─────────────────────────────────

/// **The system's own file chooser** — [`FolderPicker`]'s twin, and every word
/// of its note applies unchanged (M2-3).
///
/// It answers for two rows (§7.1.6c-6b): the window's ground picture, filtered
/// by [`image_content_types`] to the formats the decoder honours, and a
/// profile's program, which is filtered to nothing at all. A `ShellPickKind::Folder`
/// asked of *this* bridge shows a folder panel, exactly as `show_shell_picker`
/// does with the same argument on Windows — the kind travels with the request
/// and the bridge does not second-guess it.
///
/// **A `.app` is chosen whole.** The program row turns
/// `treatsFilePackagesAsDirectories` off, so the panel stops at the bundle
/// rather than descending into it, and what the profile is given is the bundle's
/// own path — `/Applications/Foo.app`.
///
/// **One bridge and not two**, unlike the folder chooser beside it, and for the
/// Windows arm's reason: the deferral's contract is "one gesture in flight", and
/// these two rows are in the same dialog and cannot both be pressed at once. The
/// kind travels *with* the request, so the answer can never be handed to the row
/// that did not ask.
pub struct ImagePicker {
    window: NativeWindow,
    state: Arc<Deferred<Choice>>,
}

impl ImagePicker {
    /// Install the chooser. Never fails; there is nothing to install.
    pub fn new(window: NativeWindow) -> Result<Self, String> {
        Ok(Self {
            window,
            state: Arc::new(Deferred::new()),
        })
    }

    /// Put the chooser up once, opening in `start` if that names a folder.
    pub fn request(&self, kind: ShellPickKind, start: Option<&Path>) -> Result<bool, String> {
        let what = "the file chooser";
        if !self.state.begin() {
            return Ok(false);
        }
        let (mtm, host) = self
            .state
            .give_back_on_refusal(window_for(self.window, what))?;
        let panel = NSOpenPanel::openPanel(mtm);
        dress(&panel, kind);
        start_at(&panel, start);
        let noun = match kind {
            ShellPickKind::Folder => "folder",
            ShellPickKind::Image => "picture",
            ShellPickKind::Program => "program",
        };
        sheet(panel, &host, self.window, Arc::clone(&self.state), noun);
        Ok(true)
    }

    /// The chosen file, `None` for a cancelled sheet, or the reason the panel
    /// could not be shown — once, and only once the sheet is gone.
    pub fn take_result(&self) -> Option<Choice> {
        self.state.take()
    }
}

// ── the formula menu (M2-3; the menu itself is §7.20's) ────────────────────

/// What a scheduled pop needs when it fires.
struct MenuDeferral {
    window: NativeWindow,
    state: Arc<Deferred<Result<bool, String>>>,
}

define_class!(
    // SAFETY:
    // - `NSObject` has no subclassing requirements.
    // - This class does not implement `Drop`; its ivars do, and the macro's
    //   generated `dealloc` runs them.
    #[unsafe(super(NSObject))]
    #[name = "FolioDeferredFormulaMenu"]
    #[ivars = MenuDeferral]
    struct DeferredFormulaMenu;

    impl DeferredFormulaMenu {
        /// **The nested tracking loop starts here and nowhere else** — from the
        /// run loop's default mode, one turn after the press that asked for it,
        /// with no winit callback anywhere on the stack.
        ///
        /// A name of Folio's own, prefixed, because it is added to a class this
        /// program defines and a plain `pop` would be a selector the Objective-C
        /// runtime shares with everything else that ever thought of the word.
        #[unsafe(method(folioPopFormulaMenu))]
        fn pop(&self) {
            let Some(mtm) = MainThreadMarker::new() else {
                // The main run loop is the main thread's by construction; this
                // is the one state this method cannot be in, and the answer to
                // it is to leave the gesture parked rather than reach AppKit.
                return;
            };
            let ivars = self.ivars();
            let answer = track_formula_menu(ivars.window, mtm);
            ivars.state.complete(answer);
            ask_for_a_frame(ivars.window, mtm);
        }
    }

    unsafe impl NSObjectProtocol for DeferredFormulaMenu {}
);

impl DeferredFormulaMenu {
    fn new(window: NativeWindow) -> Retained<Self> {
        let this = Self::alloc().set_ivars(MenuDeferral {
            window,
            state: Arc::new(Deferred::new()),
        });
        // SAFETY: `NSObject`'s designated initializer, called on a fresh
        // allocation whose ivars are set.
        unsafe { msg_send![super(this), init] }
    }
}

/// **The formula context menu, over `NSMenu`** (M2-3; what is on it is §7.20's).
///
/// Step 7 of the startup path, harmless to construct for [`FolderPicker`]'s
/// reason. The Windows arm builds its one item in `bt-platform`
/// (`AppendMenuW(… "Copy LaTeX")`) and hands `bt-app` back only whether it was
/// chosen, so that split is kept: the menu's content is here and the caller's
/// answer is the same `Option<Result<bool, String>>`.
///
/// **Deferred, because this one really is modal.**
/// `popUpMenuPositioningItem:atLocation:inView:` blocks until the menu closes,
/// so [`Self::request`] schedules the pop with
/// `performSelector:withObject:afterDelay:0` — the twin of the Windows arm's
/// `PostMessageW` — and returns before anything is on screen. A second request
/// while one is scheduled, tracking, or waiting to be collected is an ordinary
/// coalesced UI race and answers `Ok(false)`.
pub struct MathContextMenu {
    deferral: Retained<DeferredFormulaMenu>,
}

impl MathContextMenu {
    /// Install the menu bridge. Never fails; there is nothing to install.
    pub fn new(window: NativeWindow) -> Result<Self, String> {
        Ok(Self {
            deferral: DeferredFormulaMenu::new(window),
        })
    }

    /// Schedule the menu once.
    pub fn request(&self) -> Result<bool, String> {
        let what = "the formula menu";
        let ivars = self.deferral.ivars();
        if !ivars.state.begin() {
            return Ok(false);
        }
        let (_mtm, _host) = ivars
            .state
            .give_back_on_refusal(window_for(ivars.window, what))?;
        // SAFETY: the selector is one this class defines above and takes no
        // argument, which is what `withObject: nil` says; the run loop retains
        // the target until the perform fires, and `Drop` cancels any perform
        // that has not.
        unsafe {
            self.deferral.performSelector_withObject_afterDelay(
                sel!(folioPopFormulaMenu),
                None,
                0.0,
            );
        }
        Ok(true)
    }

    /// Whether an item was chosen — once, and only once the menu is gone.
    pub fn take_result(&self) -> Option<Result<bool, String>> {
        self.deferral.ivars().state.take()
    }
}

impl Drop for MathContextMenu {
    /// Take back a pop that has been scheduled and has not fired.
    ///
    /// The twin of `RemoveWindowSubclass`, and it matters for the same reason:
    /// this object is dropped when the window closes, and a perform still in the
    /// run loop would pop a menu over a window that has gone.
    fn drop(&mut self) {
        if window_thread("taking down the formula menu").is_err() {
            // A drop off the window's thread cannot touch the run loop that
            // holds the perform. It is also not a state `bt-app` reaches: the
            // window owns this and is dropped on the loop.
            return;
        }
        let target: &AnyObject = &self.deferral;
        // SAFETY: the target is this object, the selector is the one it was
        // scheduled with, and cancelling a perform that was never scheduled is
        // documented as doing nothing.
        unsafe {
            NSObject::cancelPreviousPerformRequestsWithTarget_selector_object(
                target,
                sel!(folioPopFormulaMenu),
                None,
            );
        }
    }
}

/// Pop the menu at the pointer, over the window, and say whether an item was
/// chosen.
///
/// **The pointer, and the view.** The Windows arm asks `GetCursorPos` and hands
/// `TrackPopupMenu` a screen coordinate; here the same reading —
/// `+[NSEvent mouseLocation]`, which is where the pointer is now rather than
/// where the press that asked for this was — is converted through the window
/// into the view's own space, so the menu is positioned by the view it belongs
/// to. Passing `nil` for the view would place it in screen coordinates and is
/// the same arithmetic with one fewer thing that knows which window this is.
///
/// `autoenablesItems` is turned off because the one item has no action: AppKit
/// disables an item whose target and action are both `nil` unless the menu is
/// told it does its own enabling, and a greyed-out item can never be chosen —
/// which would make this function answer `false` forever.
fn track_formula_menu(window: NativeWindow, mtm: MainThreadMarker) -> Result<bool, String> {
    let what = "the formula menu";
    let host = window_of(window, mtm).ok_or_else(|| nothing_to_ask(what))?;
    // SAFETY: as `ask_for_a_frame` — winit's live view, on winit's own thread.
    let view: &NSView = unsafe { window.as_ns_view().as_ref() };
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    let item = NSMenuItem::new(mtm);
    item.setTitle(ns_string!("Copy LaTeX"));
    item.setEnabled(true);
    menu.addItem(&item);
    let on_the_screen = NSEvent::mouseLocation();
    let in_the_window = host.convertPointFromScreen(on_the_screen);
    let in_the_view = view.convertPoint_fromView(in_the_window, None);
    Ok(menu.popUpMenuPositioningItem_atLocation_inView(None, in_the_view, Some(view)))
}

// ── the last-resort fault report (M2-3) ────────────────────────────────────

// AppKit's own global: the shared application, or null when nothing has made
// one yet.
//
// Declared here rather than taken from a crate for `macos_watch`'s reason —
// `objc2-app-kit` has no binding for it (its `NSApp` helper calls
// `+[NSApplication sharedApplication]`, which *creates* the instance, which is
// precisely the thing `message_box` must not do). One C declaration is what
// this crate is for. A plain comment and not a doc comment, because rustdoc
// does not document an extern block and says so.
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    static NSApp: *mut AnyObject;
}

/// **Say one thing in a box, with no window behind it** (M2-3).
///
/// The single message box this product is allowed to raise, for the Windows
/// arm's reason: everything Folio says once it has a window it says on a card
/// anchored at the surface the news is about, and a program that has drawn
/// nothing has nothing to interrupt. Its two callers are
/// `bt_app::announce_panic` — the panic hook, when the resident run has no
/// console to write to — and `say_at_the_front_door`, which is a launch that is
/// about to exit.
///
/// **Two conditions, and the fallback below them is not a degradation.**
/// `-[NSAlert runModal]` may only be called on the main thread, and it needs a
/// running `NSApplication` to run the modal session on. A panic hook runs on
/// whichever thread panicked, and a refused launch runs before anything has made
/// an application at all — so both conditions are genuinely reachable, and the
/// answer to either is the two `eprintln!` lines the portable arm writes, which
/// say the same two things to the same reader. `NSApp` is read rather than
/// `+[NSApplication sharedApplication]` asked, because asking *creates* the
/// application: a command-line Folio that printed a refusal would otherwise
/// acquire an AppKit application on its way out of the door.
///
/// **Why this can never nest inside a sheet's completion handler.** X-4's
/// warning is that an AppKit modal loop does not drain the main dispatch queue,
/// so a second modal started from inside one can wait forever. It cannot happen
/// from here. The completion block [`sheet`] installs contains exactly two
/// statements — park the answer, mark the view dirty — and calls nothing in
/// `bt-app`; neither of this function's two callers is reachable from it. The
/// only path that could is a Rust panic *inside* the block reaching the hook,
/// and a panic there unwinds into Objective-C and ends the process before any
/// hook of ours is consulted (X-2 measured exactly that). The sheet is also not
/// a modal loop to begin with: `beginSheetModalForWindow:` returns immediately
/// and the block runs from the ordinary run loop.
///
/// **And a third question, which is M4-11's** (`docs/DESIGN.md` §13.31). The
/// two above ask whether a box *can* be raised. This one asks whether raising
/// it would reach anybody, and it exists because of what the box does once it
/// is up: `runModal` does not return until somebody presses the button, so the
/// panic hook's last two statements — the footer and `leave_process` — wait
/// behind it. §13.23 measured exactly that on a bundle launch: the run's file
/// has no footer, because the box was still standing when the probe ended the
/// process by its pid.
///
/// A box in front of a reader is that reader's news and is worth the wait. A
/// box in a process that is **not the foreground application and has no window
/// on the screen** shows nothing to anybody and keeps the process from leaving
/// — which on a Finder launch, where `stderr` is already the log file, is a
/// Folio that has crashed and simply stands there.
///
/// So the rule, in one sentence: **the alert is raised only when this
/// application is frontmost or has a window a reader can see; otherwise the
/// same two lines go where the run's diagnostics already go and the caller
/// carries on to its own exit.** Both halves are needed — `isActive` is the
/// crash that happens while somebody is looking at Folio, and a visible window
/// is the one that happens while they are in another application, where the
/// Dock and `Command`-`Tab` still reach the box.
///
/// It is deliberately **not** a rule about which thread panicked or about how
/// the process was launched: those three readings are what decide whether there
/// is a reader, and a launch shape is only ever a proxy for them.
pub fn message_box(title: &str, text: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return say_to_stderr(title, text);
    };
    // SAFETY: AppKit's own global, read on the main thread — which is the only
    // thread that writes it — as a pointer and nothing more.
    if unsafe { NSApp }.is_null() {
        return say_to_stderr(title, text);
    }
    if !an_alert_would_be_seen(mtm) {
        return say_to_stderr(title, text);
    }
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(text));
    // `MB_ICONINFORMATION`'s twin. **No button is added**: a fresh `NSAlert`
    // already carries exactly one, and it is the system's own OK in the
    // reader's language — which is what `MB_OK` gives on the other platform and
    // what an `addButtonWithTitle:` of a literal `"OK"` would take away.
    alert.setAlertStyle(NSAlertStyle::Informational);
    alert.runModal();
}

/// **Is there a reader for a modal box right now?** (M4-11)
///
/// The rule stated in [`message_box`]'s note, read off the three things AppKit
/// will answer about itself. `sharedApplication` rather than the `NSApp` global
/// is safe *here* and only here: the caller has already found that global
/// non-null, so the application exists and asking for it returns the one that
/// is already there rather than making one.
///
/// `-[NSWindow isVisible]` and not `hasVisibleWindows`: X-4 measured that
/// AppKit's flag is YES for a minimised window and YES for one hidden with
/// `-[NSApplication hide:]` (`app_delegate::AppDelegateEventKind::Reopen`), and
/// neither of those is a window a reader can see. A window's own `isVisible` is
/// NO for both, which is the reading this rule wants.
fn an_alert_would_be_seen(mtm: MainThreadMarker) -> bool {
    let application = NSApplication::sharedApplication(mtm);
    application.isActive()
        || application
            .windows()
            .iter()
            .any(|window| window.isVisible())
}

/// The two lines, when there is no alert to raise them in.
fn say_to_stderr(title: &str, text: &str) {
    eprintln!("{title}");
    eprintln!("{text}");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The picture chooser offers every format the decoder honours, and the
    /// list is the decoder's own.**
    ///
    /// Needs no window and no application: `UTType` is a system-wide table.
    ///
    /// MUTATION: write the list out here instead of reading
    /// `IMAGE_FILE_EXTENSIONS` and the count goes red the day the decoder learns
    /// a seventh format; drop the `filter_map` and a machine with no type for an
    /// extension stops offering the five it does know.
    #[test]
    fn the_picture_chooser_offers_every_format_the_decoder_honours() {
        let types = image_content_types();
        assert_eq!(
            types.len(),
            IMAGE_FILE_EXTENSIONS.len(),
            "every extension the decoder honours resolves to a content type on this machine"
        );
        for extension in ["svg", "webp"] {
            assert!(
                IMAGE_FILE_EXTENSIONS.contains(&extension),
                "`{extension}` is one of the decoder's own"
            );
            let resolved = UTType::typeWithFilenameExtension(&NSString::from_str(extension))
                .unwrap_or_else(|| panic!("`{extension}` names a content type"));
            assert!(
                !resolved.identifier().to_string().is_empty(),
                "and the type it names has an identifier"
            );
        }
    }

    /// **The last-resort report says its two lines rather than reaching AppKit,
    /// when it is not on the thread AppKit belongs to.**
    ///
    /// The case runs on a thread the harness made, so the function is in exactly
    /// the state its first guard is for. What it must not do is call
    /// `-[NSAlert runModal]`, which off the main thread is undefined behaviour
    /// rather than a wrong answer — so the claim the case can make is that it
    /// returns at all.
    ///
    /// MUTATION: drop the `MainThreadMarker` guard and this reaches AppKit from
    /// a test harness thread.
    #[test]
    fn the_last_resort_report_writes_to_stderr_off_the_main_thread() {
        assert!(
            MainThreadMarker::new().is_none(),
            "a test harness thread is not the main thread; this case has nothing to say otherwise"
        );
        message_box("Folio", "the two lines a reader with no window gets");
    }

    /// **Every door here refuses when it is asked from a thread that is not the
    /// window's**, rather than reaching AppKit from it.
    ///
    /// The twin of `macos_impl`'s `a_window_door_asked_off_the_window_thread_refuses`,
    /// and the constructors above it are the other half of §4.4's rule: the
    /// three that stand on the startup path build without complaint, because a
    /// refusal there is a launch that never draws.
    ///
    /// MUTATION: drop the `window_for` gate from any of the three doors and it
    /// reaches AppKit off the main thread.
    #[test]
    fn a_dialog_door_asked_off_the_window_thread_refuses() {
        let window = NativeWindow::stand_in(2);
        let folder = FolderPicker::new(window).expect("the folder chooser builds");
        let picture = ImagePicker::new(window).expect("the picture chooser builds");
        let menu = MathContextMenu::new(window).expect("the formula menu builds");
        assert!(folder.request(None).is_err(), "the folder chooser");
        assert!(
            picture.request(ShellPickKind::Image, None).is_err(),
            "the picture chooser"
        );
        assert!(
            picture.request(ShellPickKind::Program, None).is_err(),
            "the program chooser"
        );
        assert!(menu.request().is_err(), "the formula menu");
        assert!(
            folder.take_result().is_none(),
            "a request that refused leaves no answer to collect"
        );
        assert!(picture.take_result().is_none(), "and nor does the file one");
        assert!(menu.take_result().is_none(), "and nor does the menu");
    }

    /// **One gesture in flight, and its answer collected once.**
    ///
    /// The state machine on its own, which is the half of the contract that has
    /// no window in it: a second request while one is up is refused, an answer
    /// is handed over exactly once, and a request that could not be put up gives
    /// the slot back rather than jamming the row forever.
    ///
    /// MUTATION: let `begin` succeed from `Waiting` and the first assertion goes
    /// red — two sheets would stack on one window; let `take` leave the phase at
    /// `Complete` and the third does.
    #[test]
    fn one_gesture_is_in_flight_and_its_answer_is_taken_once() {
        let state = Deferred::<Choice>::new();
        assert!(state.begin(), "the first request claims the slot");
        assert!(
            !state.begin(),
            "and the second is coalesced rather than stacked"
        );
        assert!(state.take().is_none(), "there is nothing to collect yet");
        state.complete(Ok(Some(PathBuf::from("/tmp"))));
        assert!(
            !state.begin(),
            "an answer nobody has read still holds the slot, as it does on Windows"
        );
        assert_eq!(state.take(), Some(Ok(Some(PathBuf::from("/tmp")))));
        assert_eq!(state.take(), None, "and only once");
        assert!(state.begin(), "the slot is free again");
        state.give_up();
        assert!(
            state.begin(),
            "a request that could not be put up gives it back"
        );
    }

    /// **A cancelled sheet is `Ok(None)`, a failed one is `Err`, and neither is
    /// the other.**
    ///
    /// MUTATION: read `NSModalResponseAbort` as a cancel and the second half
    /// goes red — a chooser that never appeared would then be indistinguishable
    /// from one the reader dismissed.
    #[test]
    fn a_cancelled_sheet_and_a_sheet_that_never_opened_are_different_answers() {
        assert_eq!(
            settled_by_the_response(NSModalResponseCancel, "folder"),
            Some(Ok(None)),
            "a dismissed sheet leaves the row alone"
        );
        assert_eq!(
            settled_by_the_response(NSModalResponseOK, "folder"),
            None,
            "and an accepted one sends the caller to the panel"
        );
        let refused = settled_by_the_response(-1001, "folder")
            .expect("abort is settled without a panel")
            .expect_err("and it is a failure");
        assert!(
            refused.contains("could not be shown"),
            "and it says so in a sentence a toast can carry: {refused}"
        );
    }
}
