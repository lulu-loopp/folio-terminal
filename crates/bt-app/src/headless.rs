//! **The headless backend** (T-HEADLESS-RUNTIME-SEAM): a [`Backend`] with no winit window, no
//! wgpu device or renderer and no compositor, so a test can build `App`, `WindowRuntime`,
//! `Runtime` and `FolioApp` and drive their methods.
//!
//! Every door records what the window would have done as a typed [`WindowEffect`] on the
//! window's [`Ledger`]; the loop's wakes land on [`LoopEvents`]. The glass measures with the
//! production font environment through `bt_render::DeviceFreeMeasure` — the shaping functions a
//! `WindowRenderer` measures with — and keeps what a frame was handed (its preview bodies, its
//! chrome, its overlay) so a test can read the frame's inputs back. Nothing here reaches the
//! machine: the stores are at a test's own scratch paths, the pickers answer what the test
//! scripts, the window's questions to the system answer the stand-ins of [`PlatformAnswers`]
//! (a test scripts them), and its compositor is no tree a page's engine could hang in.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::num::NonZeroU32;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Instant;

use bt_platform::admission::{WaitToken, doors};
use bt_render::{
    CellMetrics, ChromeIcon, ChromeLabel, ChromeLabelWeight, ChromeQuad, ChromeTextAdvances,
    DeviceFreeMeasure, FrameSource, FrameTrigger, GlyphCensus, ImeCursorArea, MathBandFace,
    MathBandTrace, MathHit, MathToolBoxes, OverlayGroup, OverlayLayer, PeekImageOverlay,
    PresentConfiguration, PresentOutcome, PresentPhase, PresentReceipt, PresentationGeometry,
    PreviewBody, PreviewImage, PreviewParagraph, PreviewRun, PreviewRunBox, PreviewTextFrame,
    PreviewTextRow, RenderError, RendererPresentState, SeatFrame, SeatViewport, TableBlockKey,
    TableBlockPaint, VideoLayer, WebHole,
};
use bt_viewport::{MathBlockAnchor, ViewportFrame};
use winit::dpi::{PhysicalPosition, PhysicalSize, Position, Size};
use winit::event_loop::ControlFlow;
use winit::window::{Cursor, WindowId};

use crate::ime_report::NativeFacts;
use crate::window_seam::{
    Backend, CompositorDoor, DeviceLayer, FolderPickerDoor, FrameDoor, Glass, ImagePickerDoor,
    ImeCaretDoor, LoopDoor, LoopProxy, MathMenuDoor, PageTree, SavePickerDoor, TaskbarDoor,
    WindowHost,
};
use crate::{App, AppEvent, TabState, WindowRuntime};

/// **The test suite's backend.**
pub(crate) struct Headless;

/// **One thing a window would have done to the machine**, as the headless window records it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum WindowEffect {
    /// `Window::request_redraw`.
    Redraw,
    /// `Window::set_title`.
    Title(String),
    /// `Window::set_visible`.
    Visible(bool),
    /// `Window::set_minimized`.
    Minimized(bool),
    /// `Window::set_maximized`.
    Maximized(bool),
    /// `Window::focus_window`.
    Focused,
    /// `Window::set_cursor`.
    Cursor(Cursor),
    /// `Window::set_ime_cursor_area`, in physical pixels: x, y, width, height.
    ImeArea([i32; 4]),
    /// macOS's Option key told to send Alt, or not.
    OptionAsAlt(bool),
    /// The window's input-method facts read (`WindowHost::ime_facts`).
    ImeFactsRead,
    /// A line the window's runtime said in `diagnostics.log` (`WindowHost::note`), as it would
    /// have been written — at most [`NOTES_KEPT`] of them between two takes of the ledger.
    Noted(String),
    /// The system's blur behind the ground, on or off (`WindowHost::set_backdrop`).
    Backdrop(bool),
    /// The window told to stay above other windows, or not (`WindowHost::set_topmost`).
    Topmost(bool),
    /// The frame's dark plate, or its light one (`WindowHost::set_dark_mode`).
    DarkMode(bool),
    /// The window stood at an outer rectangle (`WindowHost::stand_at`).
    StoodAt(bt_platform::WindowRect),
    /// The colour painted before the first frame, or none (`WindowHost::set_class_background`).
    ClassBackground(Option<[u8; 3]>),
    /// The keyboard and the foreground given to the window (`WindowHost::take_foreground`).
    ForegroundTaken,
    /// The keyboard focus taken back from a child (`WindowHost::take_keyboard_focus`).
    KeyboardFocusTaken,
    /// The window asked to close (`WindowHost::request_close`).
    CloseRequested,
    /// The taskbar button flashed (`WindowHost::flash`).
    Flashed,
    /// The taskbar button's flash stopped (`WindowHost::stop_flashing`).
    FlashStopped,
    /// The taskbar button told a progress (`TaskbarDoor::set_progress`).
    TaskbarProgress(bt_platform::TaskbarProgress),
    /// The system's preference broadcasts asked for (`WindowHost::watch_system_settings`).
    SystemSettingsWatched,
    /// The system's end-of-session question asked for (`WindowHost::watch_session_end`).
    SessionEndWatched,
    /// The system asked to start the program again after its restart, on these occasions
    /// (`Backend::register_restart`).
    RestartRegistered { flags: u32 },
    /// That registration taken back (`Backend::withdraw_restart`).
    RestartWithdrawn,
    /// A page's place in the window asked for, for the web engine (`WindowHost::page_address`).
    PageAddressed(bt_platform::PageVisual),
    /// A hand-off to the system: a file opened, a path revealed, an address handed over.
    HandedOff(bt_platform::Handoff),
    /// A frame presented: how many seats it carried and what asked for it.
    Presented { seats: usize, source: FrameSource },
    /// The surface resized to a physical width and height.
    Resized(u32, u32),
    /// The window moved to another display scale.
    Rescaled(f64),
    /// The terminal face changed under the window.
    FontApplied,
    /// The composition told the window's size.
    CompositionSize(u32, u32),
    /// The composition told how much of the window the surface covers.
    CoveredSize(u32, u32),
    /// The ground a page stands on.
    PageGround([f32; 4]),
    /// A page's visual hung in the tree.
    PageAttached(bt_platform::PageVisual),
    /// A page's visual taken out of it.
    PageDetached(bt_platform::PageVisual),
    /// A page's visual hidden.
    PageHidden(bt_platform::PageVisual),
    /// A page's visual placed at an offset under a clip.
    PagePlaced(bt_platform::PageVisual),
    /// What covers a page.
    PageCovered(bt_platform::PageVisual, Vec<[f32; 4]>),
    /// The composition committed.
    Committed,
    /// The frame's own title.
    FrameTitle(String),
    /// A press on the title bar handed to the platform.
    TitleBarPressed,
    /// The band the frame wears, in logical pixels.
    WindowBand(f32),
    /// The title bar's own boxes, each with its role.
    TitleBarBoxes(Vec<bt_platform::TitleBarBox>),
    /// The client's smallest size.
    MinClientSize(Option<(u32, u32)>),
    /// The folder chooser asked for, starting where it was told.
    FolderAsked(Option<PathBuf>),
    /// The file chooser asked for one kind of file, with the image filter's name the caller
    /// passed.
    FileAsked(bt_platform::FilePickKind, Option<PathBuf>, String),
    /// The save dialog asked for, with its starting folder and name.
    SaveAsked(Option<PathBuf>, String),
    /// The formula's menu asked for, with the row title the caller passed.
    MathMenuAsked(String),
    /// The input method's system caret placed.
    ImeCaret(i32, i32),
    /// The input method's system caret taken down.
    ImeCaretDestroyed,
    /// The loop told to stop.
    Exit,
    /// The loop told how to wait.
    ControlFlow(ControlFlow),
    /// Every surface built again.
    SurfacesRebuilt(usize),
    /// A second window asked of the loop.
    WindowOpened,
}

/// **How many `diagnostics.log` lines a ledger keeps between two takes** (T-DIAGNOSTICS-TEST-SINK).
/// A line past them is counted ([`Ledger::notes_beyond_the_cap`]) and not kept, so a road a test
/// drives through a long run of events does not grow the ledger by a line per event, and a plain
/// [`Ledger::take`] after one refuses with the count rather than hand back a shorter record. A
/// test's bound, not a product budget and not derived from one: 32 mirrors the input method's
/// bound on a window's bracket lines (`ime_report::PAIRING_LINES_MAX`); the product's own bounds
/// on a road's lines are the road's.
pub(crate) const NOTES_KEPT: usize = 32;

/// **A window's effects, in the order its doors were knocked on.** One per headless window,
/// shared by its stand-ins.
#[derive(Clone, Debug, Default)]
pub(crate) struct Ledger(Rc<RefCell<Entries>>);

/// What a [`Ledger`] holds: the effects, and the `diagnostics.log` lines counted past
/// [`NOTES_KEPT`] since the last take.
#[derive(Debug, Default)]
struct Entries {
    effects: Vec<WindowEffect>,
    notes_kept: usize,
    notes_beyond_the_cap: usize,
}

impl Ledger {
    fn record(&self, effect: WindowEffect) {
        self.0.borrow_mut().effects.push(effect);
    }

    /// A line said in `diagnostics.log`: kept as [`WindowEffect::Noted`] while fewer than
    /// [`NOTES_KEPT`] are held, counted after.
    fn note(&self, line: &str) {
        let mut entries = self.0.borrow_mut();
        if entries.notes_kept < NOTES_KEPT {
            entries.notes_kept += 1;
            entries.effects.push(WindowEffect::Noted(line.to_owned()));
        } else {
            entries.notes_beyond_the_cap += 1;
        }
    }

    /// Everything recorded so far, emptied.
    ///
    /// # Panics
    ///
    /// When a line was said past [`NOTES_KEPT`] since the last take: what a test reads back would
    /// be missing lines the road said, so it fails loudly with the count rather than pass on a
    /// shorter record. The cap's own test reads the count first ([`Self::take_with_the_count`]).
    pub(crate) fn take(&self) -> Vec<WindowEffect> {
        let beyond = self.notes_beyond_the_cap();
        assert!(
            beyond == 0,
            "{beyond} diagnostics.log line(s) were said past the ledger's {NOTES_KEPT} and not \
             kept; take the ledger more often"
        );
        self.take_with_the_count().0
    }

    /// Everything recorded so far, emptied, and how many lines were said past the cap — which the
    /// take empties too.
    fn take_with_the_count(&self) -> (Vec<WindowEffect>, usize) {
        let mut entries = self.0.borrow_mut();
        entries.notes_kept = 0;
        let beyond = std::mem::take(&mut entries.notes_beyond_the_cap);
        (std::mem::take(&mut entries.effects), beyond)
    }

    /// How many `diagnostics.log` lines were said past [`NOTES_KEPT`] since the last take.
    pub(crate) fn notes_beyond_the_cap(&self) -> usize {
        self.0.borrow().notes_beyond_the_cap
    }
}

/// **What the loop was woken with**, from any thread.
#[derive(Clone, Default)]
pub(crate) struct LoopEvents(Arc<Mutex<Vec<AppEvent>>>);

impl LoopEvents {
    /// The proxy every worker and watch of a headless application wakes the loop through.
    pub(crate) fn proxy(&self) -> LoopProxy {
        let events = Arc::clone(&self.0);
        LoopProxy::new(move |event| {
            events
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(event);
            Ok(())
        })
    }

    /// Everything sent so far, emptied.
    pub(crate) fn take(&self) -> Vec<AppEvent> {
        std::mem::take(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Backend for Headless {
    type Window = HeadlessWindow;
    type Device = HeadlessDevice;
    type Glass = HeadlessGlass;
    type Compositor = HeadlessCompositor;
    type Frame = HeadlessFrame;
    type MathMenu = HeadlessPicker;
    type FolderPicker = HeadlessPicker;
    type ImagePicker = HeadlessPicker;
    type SavePicker = HeadlessPicker;
    type ImeCaret = HeadlessImeCaret;
    type Loop = HeadlessLoop;

    fn rebuild_every_surface(
        _gpu: &mut HeadlessDevice,
        windows: Vec<&mut WindowRuntime<Self>>,
    ) -> Result<(), RenderError> {
        let count = windows.len();
        for window in windows {
            window
                .window
                .ledger
                .record(WindowEffect::SurfacesRebuilt(count));
        }
        Ok(())
    }

    fn open_window(
        event_loop: &HeadlessLoop,
        _app: &mut App<Self>,
        _plan: &crate::NewWindowPlan,
        _like: Option<(
            bt_persist::SessionTabLayoutV1,
            bt_persist::SessionSidebarModeV1,
        )>,
    ) -> anyhow::Result<(WindowId, WindowRuntime<Self>)> {
        event_loop.ledger.record(WindowEffect::WindowOpened);
        Err(anyhow::anyhow!("a headless loop opens no second window"))
    }

    fn register_restart(event_loop: &HeadlessLoop, flags: u32) -> Result<(), String> {
        event_loop
            .ledger
            .record(WindowEffect::RestartRegistered { flags });
        Ok(())
    }

    fn withdraw_restart(event_loop: &HeadlessLoop) -> Result<(), String> {
        event_loop.ledger.record(WindowEffect::RestartWithdrawn);
        Ok(())
    }
}

/// **A window with no window**: an id, a size, a scale and a ledger.
pub(crate) struct HeadlessWindow {
    pub(crate) id: WindowId,
    pub(crate) size: PhysicalSize<u32>,
    pub(crate) scale: f64,
    pub(crate) focused: bool,
    pub(crate) ledger: Ledger,
    /// What the clipboard holds, as this window's clipboard door reads and writes it on the
    /// clipboard lane's worker.
    pub(crate) clipboard: HeadlessClipboard,
    /// What the window's input-method facts door answers: whatever the test scripted, default
    /// facts (every field `None`) until it does.
    pub(crate) ime_facts: Cell<NativeFacts>,
    /// What the window's questions to the system answer: [`PlatformAnswers::default`] until a
    /// test scripts them.
    pub(crate) answers: Cell<PlatformAnswers>,
}

/// **What a headless window's questions to the system answer** — the documented stand-ins, one
/// per question, each a field a test may script to drive the branch behind it.
///
/// **A recorder, by design, not a window without a handle.** The read defaults are the product's
/// answers about a window whose handle cannot be got — on the screen and uncovered, no rectangle,
/// no glass, no pointer, no present facts, not the foreground's holder — so a test that scripts
/// nothing drives the branches such a window drives. The instructions are not: where the product
/// with no handle is refused (`take_foreground` answers `false`, every `Result` door an `Err`),
/// the headless window records the instruction and lets it through (`takes_the_foreground`
/// defaults to `true`, every other instruction answers `Ok`), so the code after an instruction is
/// driven too; a test that wants the refusal scripts it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlatformAnswers {
    /// `WindowHost::is_minimized`, winit's own reading: `Some(false)`.
    pub(crate) minimized: Option<bool>,
    /// `WindowHost::is_iconic`: `false`.
    pub(crate) iconic: bool,
    /// `WindowHost::is_cloaked`: `false`.
    pub(crate) cloaked: bool,
    /// `WindowHost::is_exposed`: `true`.
    pub(crate) exposed: bool,
    /// `WindowHost::outer_rect`: `None`.
    pub(crate) outer_rect: Option<bt_platform::WindowRect>,
    /// `WindowHost::work_area`: `None`.
    pub(crate) work_area: Option<bt_platform::WindowRect>,
    /// `WindowHost::glass_at`, at every point: `GlassHere::Unknown`.
    pub(crate) glass: crate::GlassHere,
    /// `WindowHost::pointer_in_window`: `None`.
    pub(crate) pointer: Option<(i32, i32)>,
    /// `WindowHost::holds_the_foreground`, for every holder: `false`.
    pub(crate) holds_the_foreground: bool,
    /// `WindowHost::take_foreground`: `true`, the system lets it.
    pub(crate) takes_the_foreground: bool,
    /// `WindowHost::present_facts`: every fact unread.
    pub(crate) present_facts: bt_platform::NativePresentFacts,
}

impl Default for PlatformAnswers {
    fn default() -> Self {
        Self {
            minimized: Some(false),
            iconic: false,
            cloaked: false,
            exposed: true,
            outer_rect: None,
            work_area: None,
            glass: crate::GlassHere::Unknown,
            pointer: None,
            holds_the_foreground: false,
            takes_the_foreground: true,
            present_facts: bt_platform::NativePresentFacts::default(),
        }
    }
}

/// **A headless window's taskbar button**: each progress it is told, recorded.
pub(crate) struct HeadlessTaskbar {
    ledger: Ledger,
}

impl TaskbarDoor for HeadlessTaskbar {
    fn set_progress(&self, progress: bt_platform::TaskbarProgress) -> Result<(), String> {
        self.ledger.record(WindowEffect::TaskbarProgress(progress));
        Ok(())
    }
}

impl WindowHost for HeadlessWindow {
    fn id(&self) -> WindowId {
        self.id
    }
    fn request_redraw(&self) {
        self.ledger.record(WindowEffect::Redraw);
    }
    fn inner_size(&self) -> PhysicalSize<u32> {
        self.size
    }
    fn client_origin(&self) -> Option<(i32, i32)> {
        Some((0, 0))
    }
    fn scale_factor(&self) -> f64 {
        self.scale
    }
    fn is_visible(&self) -> Option<bool> {
        Some(true)
    }
    fn is_maximized(&self) -> bool {
        false
    }
    fn is_minimized(&self) -> Option<bool> {
        self.answers.get().minimized
    }
    fn has_focus(&self) -> bool {
        self.focused
    }
    fn set_minimized(&self, minimized: bool) {
        self.ledger.record(WindowEffect::Minimized(minimized));
    }
    fn set_maximized(&self, maximized: bool) {
        self.ledger.record(WindowEffect::Maximized(maximized));
    }
    fn set_title(&self, title: &str) {
        self.ledger.record(WindowEffect::Title(title.to_owned()));
    }
    fn set_visible(&self, visible: bool) {
        self.ledger.record(WindowEffect::Visible(visible));
    }
    fn focus_window(&self) {
        self.ledger.record(WindowEffect::Focused);
    }
    fn set_cursor(&self, cursor: Cursor) {
        self.ledger.record(WindowEffect::Cursor(cursor));
    }
    fn set_ime_cursor_area(&self, position: Position, size: Size) {
        let at: PhysicalPosition<i32> = position.to_physical(self.scale);
        let extent: PhysicalSize<i32> = size.to_physical(self.scale);
        self.ledger.record(WindowEffect::ImeArea([
            at.x,
            at.y,
            extent.width,
            extent.height,
        ]));
    }
    fn monitor_sizes(&self) -> Vec<PhysicalSize<u32>> {
        Vec::new()
    }
    fn frame_rate_millihertz(&self) -> Option<u32> {
        None
    }
    fn is_iconic(&self) -> bool {
        self.answers.get().iconic
    }
    fn is_cloaked(&self) -> bool {
        self.answers.get().cloaked
    }
    fn is_exposed(&self) -> bool {
        self.answers.get().exposed
    }
    fn outer_rect(&self) -> Option<bt_platform::WindowRect> {
        self.answers.get().outer_rect
    }
    fn work_area(&self) -> Option<bt_platform::WindowRect> {
        self.answers.get().work_area
    }
    fn stand_at(&self, rect: bt_platform::WindowRect) -> Result<(), String> {
        self.ledger.record(WindowEffect::StoodAt(rect));
        Ok(())
    }
    fn glass_at(&self, _x: i32, _y: i32) -> crate::GlassHere {
        self.answers.get().glass
    }
    fn pointer_in_window(&self) -> Option<(i32, i32)> {
        self.answers.get().pointer
    }
    fn present_facts(&self) -> bt_platform::NativePresentFacts {
        self.answers.get().present_facts
    }
    fn holds_the_foreground(&self, _holder: bt_platform::hotkey::Foreground) -> bool {
        self.answers.get().holds_the_foreground
    }
    fn set_backdrop(&self, acrylic: bool) -> Result<(), String> {
        self.ledger.record(WindowEffect::Backdrop(acrylic));
        Ok(())
    }
    fn set_topmost(&self, topmost: bool) -> Result<(), String> {
        self.ledger.record(WindowEffect::Topmost(topmost));
        Ok(())
    }
    fn set_dark_mode(&self, dark: bool) -> Result<(), String> {
        self.ledger.record(WindowEffect::DarkMode(dark));
        Ok(())
    }
    fn set_class_background(&self, rgb: Option<[u8; 3]>) -> Result<(), String> {
        self.ledger.record(WindowEffect::ClassBackground(rgb));
        Ok(())
    }
    fn take_foreground(&self) -> bool {
        self.ledger.record(WindowEffect::ForegroundTaken);
        self.answers.get().takes_the_foreground
    }
    fn take_keyboard_focus(&self) -> Result<(), String> {
        self.ledger.record(WindowEffect::KeyboardFocusTaken);
        Ok(())
    }
    fn request_close(&self) -> Result<(), String> {
        self.ledger.record(WindowEffect::CloseRequested);
        Ok(())
    }
    fn flash(&self) {
        self.ledger.record(WindowEffect::Flashed);
    }
    fn stop_flashing(&self) {
        self.ledger.record(WindowEffect::FlashStopped);
    }
    type Taskbar = HeadlessTaskbar;
    fn taskbar(&self) -> Result<HeadlessTaskbar, String> {
        Ok(HeadlessTaskbar {
            ledger: self.ledger.clone(),
        })
    }
    fn watch_system_settings(
        &self,
        _wake: Box<dyn Fn(bt_platform::SystemNews)>,
    ) -> Option<bt_platform::SystemSettingsWatch> {
        self.ledger.record(WindowEffect::SystemSettingsWatched);
        None
    }
    fn watch_session_end(
        &self,
        _hear: Box<dyn Fn(bt_platform::session_end::SessionEnd)>,
        _reason: Box<dyn Fn() -> String>,
    ) -> Option<bt_platform::session_end::SessionEndWatch> {
        self.ledger.record(WindowEffect::SessionEndWatched);
        None
    }
    /// **A headless window hosts no web engine**: the place is recorded, and the answer is the
    /// refusal a page's open takes when there is no engine to open it in.
    fn page_address(
        &self,
        page: bt_platform::PageVisual,
    ) -> Result<crate::webhost::SeatAddress, String> {
        self.ledger.record(WindowEffect::PageAddressed(page));
        Err("a headless window hosts no web engine".to_owned())
    }
    fn ime_facts(&self) -> NativeFacts {
        self.ledger.record(WindowEffect::ImeFactsRead);
        self.ime_facts.get()
    }
    fn note(&self, line: &str) {
        self.ledger.note(line);
    }
    fn set_option_as_alt(&self, option_sends_alt: bool) {
        self.ledger
            .record(WindowEffect::OptionAsAlt(option_sends_alt));
    }
    fn clipboard(
        &self,
        lane: &crate::clipboard_lane::ClipboardLane,
        ask: crate::clipboard_lane::Ask,
    ) -> Result<crate::clipboard_lane::ClipboardRequest, crate::clipboard_lane::Declined> {
        lane.submit(Arc::new(self.clipboard.clone()), ask)
    }
    fn hand_off(
        &self,
        _lane: &crate::handoff_lane::HandoffLane,
        handoff: bt_platform::Handoff,
    ) -> Result<crate::handoff_lane::HandoffRequest, crate::handoff_lane::Declined> {
        self.ledger.record(WindowEffect::HandedOff(handoff));
        Err(crate::handoff_lane::Declined::NoWindow(
            "a headless window is named to no door".to_owned(),
        ))
    }
    fn dpi_snapshot(&self) -> anyhow::Result<crate::DpiSnapshot> {
        Ok(crate::DpiSnapshot {
            winit_scale: self.scale,
            win32_dpi: (self.scale * 96.0).round() as u32,
            authoritative_scale: self.scale,
            rect: bt_platform::WindowRect {
                left: 0,
                top: 0,
                right: self.size.width as i32,
                bottom: self.size.height as i32,
            },
        })
    }
}

/// **The headless window's clipboard** — the scripted door its requests are served through on
/// the clipboard lane's worker (T-CLIPBOARD-LANE): what it holds, what was written to it, the
/// opens it refuses (as another program holding it open would make `OpenClipboard` refuse), and
/// a hold the test can keep the worker in. Nothing here reaches the machine's clipboard.
#[derive(Clone, Default)]
pub(crate) struct HeadlessClipboard(Arc<ClipboardScript>);

#[derive(Default)]
pub(crate) struct ClipboardScript {
    held: Mutex<Option<String>>,
    written: Mutex<Vec<String>>,
    refused_opens: Mutex<usize>,
    hold: crate::lane::Hold,
}

/// How many times the platform's door asks `OpenClipboard` before it gives up — `bt_platform`'s
/// five attempts.
const OPEN_ATTEMPTS: usize = 5;

impl HeadlessClipboard {
    /// The clipboard now holds `text`, as if another program had copied it.
    pub(crate) fn holds(&self, text: &str) {
        *locked(&self.0.held) = Some(text.to_owned());
    }

    /// Everything written to it since the last time this was asked, in order.
    pub(crate) fn take_written(&self) -> Vec<String> {
        std::mem::take(&mut locked(&self.0.written))
    }

    /// The next `opens` attempts to open it are refused, as while another program holds it.
    pub(crate) fn refuse_opens(&self, opens: usize) {
        *locked(&self.0.refused_opens) = opens;
    }

    /// The hold the worker stands in before it opens the clipboard.
    pub(crate) fn worker_hold(&self) -> &crate::lane::Hold {
        &self.0.hold
    }
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl crate::clipboard_lane::ClipboardDoor for HeadlessClipboard {
    fn serve(
        &self,
        _worker: &bt_platform::admission::WorkerCtx,
        ask: &crate::clipboard_lane::Ask,
    ) -> Result<crate::clipboard_lane::Reply, String> {
        use crate::clipboard_lane::{Ask, Reply};
        self.0.hold.pass();
        {
            let mut refused = locked(&self.0.refused_opens);
            let spent = (*refused).min(OPEN_ATTEMPTS);
            *refused -= spent;
            if spent == OPEN_ATTEMPTS {
                return Err(format!(
                    "OpenClipboard failed after {OPEN_ATTEMPTS} attempts: the headless clipboard \
                     is held open by another program"
                ));
            }
        }
        match ask {
            Ask::Text => locked(&self.0.held)
                .clone()
                .map(Reply::Text)
                .ok_or_else(|| "the headless clipboard holds no text".to_owned()),
            Ask::Payload => locked(&self.0.held)
                .clone()
                .map(|text| Reply::Payload(bt_platform::ClipboardPayload::Text(text)))
                .ok_or_else(|| "the headless clipboard holds nothing".to_owned()),
            Ask::Write(text) => {
                *locked(&self.0.held) = Some(text.clone());
                locked(&self.0.written).push(text.clone());
                Ok(Reply::Written)
            }
        }
    }
}

/// **The process's measuring**, made once: a font system is the costly part of a glass.
fn shared_measure() -> Arc<Mutex<DeviceFreeMeasure>> {
    static MEASURE: OnceLock<Arc<Mutex<DeviceFreeMeasure>>> = OnceLock::new();
    Arc::clone(MEASURE.get_or_init(|| Arc::new(Mutex::new(DeviceFreeMeasure::new()))))
}

/// **A device layer with no device**: the font environment and the face size.
pub(crate) struct HeadlessDevice {
    measure: Arc<Mutex<DeviceFreeMeasure>>,
    terminal_font_size_logical_px: f32,
    font_environment_epoch: u64,
}

impl HeadlessDevice {
    pub(crate) fn new(terminal_font_size_logical_px: f32) -> Self {
        Self {
            measure: shared_measure(),
            terminal_font_size_logical_px,
            font_environment_epoch: 0,
        }
    }

    fn measure(&self) -> std::sync::MutexGuard<'_, DeviceFreeMeasure> {
        self.measure.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl DeviceLayer for HeadlessDevice {
    fn font_environment_epoch(&self) -> u64 {
        self.font_environment_epoch
    }
    fn device_loss(&self) -> Option<&str> {
        None
    }
    fn terminal_font_size_logical_px(&self) -> f32 {
        self.terminal_font_size_logical_px
    }
    fn set_terminal_font(
        &mut self,
        _family: &str,
        _files: &[PathBuf],
        _cjk_family: &str,
        _cjk_files: &[PathBuf],
        size_logical_px: f32,
    ) {
        self.terminal_font_size_logical_px = size_logical_px;
        self.font_environment_epoch += 1;
    }
}

/// **A window's glass with no surface**: its metrics, its measuring and what the last frame was
/// handed.
pub(crate) struct HeadlessGlass {
    ledger: Ledger,
    base_metrics: CellMetrics,
    size: (u32, u32),
    seat: SeatViewport,
    window_focused: bool,
    cursor_blink_visible: bool,
    retained_revision: u64,
    font_revision: u64,
    perf_frame: u64,
    video_layers: Vec<VideoLayer>,
    /// What the last frame was handed, read back by a test.
    pub(crate) preview_bodies: Vec<PreviewBody>,
    pub(crate) preview_images: Vec<PreviewImage>,
    pub(crate) chrome: (Vec<ChromeQuad>, Vec<ChromeLabel>, Vec<ChromeIcon>),
    pub(crate) overlay: (Vec<OverlayLayer>, Vec<OverlayGroup>),
    pub(crate) peek_overlay: Option<PeekImageOverlay>,
    /// How many times the overlay has been handed over — one build of it each.
    pub(crate) overlay_builds: usize,
    pub(crate) web_holes: Vec<WebHole>,
    pub(crate) table_blocks: HashMap<TableBlockKey, TableBlockPaint>,
}

impl HeadlessGlass {
    pub(crate) fn new(
        device: &HeadlessDevice,
        ledger: Ledger,
        size: (u32, u32),
        scale: f64,
    ) -> Self {
        let base_metrics = device
            .measure()
            .cell_metrics(scale, device.terminal_font_size_logical_px)
            .expect("the production font environment measures a cell");
        Self {
            ledger,
            base_metrics,
            size,
            seat: SeatViewport::whole(size.0, size.1),
            window_focused: true,
            cursor_blink_visible: true,
            retained_revision: 0,
            font_revision: 0,
            perf_frame: 0,
            video_layers: Vec::new(),
            preview_bodies: Vec::new(),
            preview_images: Vec::new(),
            chrome: (Vec::new(), Vec::new(), Vec::new()),
            overlay: (Vec::new(), Vec::new()),
            peek_overlay: None,
            overlay_builds: 0,
            web_holes: Vec::new(),
            table_blocks: HashMap::new(),
        }
    }

    /// The setter's own answer: the frame's inputs changed.
    fn changed(&mut self) -> bool {
        self.retained_revision += 1;
        true
    }
}

impl Glass for HeadlessGlass {
    type Device = HeadlessDevice;

    fn scale_factor(&self) -> f64 {
        self.base_metrics.scale_factor
    }
    fn dpi_milli(&self) -> NonZeroU32 {
        self.base_metrics.dpi_milli()
    }
    fn base_metrics(&self) -> CellMetrics {
        self.base_metrics
    }
    fn presentation_geometry(&self) -> PresentationGeometry {
        PresentationGeometry {
            swapchain_size: self.size,
            max_texture_dimension_2d: 16_384,
        }
    }
    fn presented_swapchain_size(&self) -> (u32, u32) {
        self.size
    }
    fn seat_viewport(&self) -> SeatViewport {
        self.seat
    }
    fn set_seat_viewport(&mut self, seat: SeatViewport) -> bool {
        let changed = self.seat != seat;
        self.seat = seat;
        changed
    }
    fn font_revision(&self) -> u64 {
        self.font_revision
    }
    fn surface_generation(&self) -> u64 {
        0
    }
    fn present_state(&self) -> RendererPresentState {
        RendererPresentState {
            retained_revision: self.retained_revision,
            surface_generation: 0,
            size: self.size,
            scale_factor: self.base_metrics.scale_factor.to_bits(),
            font_revision: self.font_revision,
            theme_revision: bt_render::theme_revision(),
            cursor_style: bt_render::current_cursor_style(),
            cursor_blink_visible: self.cursor_blink_visible,
            window_focused: self.window_focused,
            seat: self.seat,
        }
    }
    fn present_configuration(&self, _gpu: &HeadlessDevice) -> PresentConfiguration {
        PresentConfiguration {
            generation: 0,
            mode: bt_render::PresentMode::Fifo,
            latency: 1,
            wait: "headless",
        }
    }
    fn perf_frame(&self) -> u64 {
        self.perf_frame
    }
    fn glyph_census(&self) -> Option<&GlyphCensus> {
        None
    }
    fn set_glyph_census(&mut self, _enabled: bool) {}
    fn preview_text_frame(&self) -> PreviewTextFrame {
        PreviewTextFrame::default()
    }
    fn video_layers(&self) -> &[VideoLayer] {
        &self.video_layers
    }
    fn set_video_layers(&mut self, layers: Vec<VideoLayer>) -> bool {
        self.video_layers = layers;
        self.changed()
    }
    fn set_web_holes(&mut self, holes: Vec<WebHole>) -> bool {
        self.web_holes = holes;
        self.changed()
    }
    fn set_peek_overlay(&mut self, overlay: Option<PeekImageOverlay>) -> bool {
        self.peek_overlay = overlay;
        self.changed()
    }
    fn set_preview_images(&mut self, images: Vec<PreviewImage>) -> bool {
        self.preview_images = images;
        self.changed()
    }
    fn place_preview_image(
        &mut self,
        _owner: u64,
        _seat: SeatViewport,
        _clip: SeatViewport,
    ) -> bool {
        self.changed()
    }
    fn set_preview_bodies(&mut self, bodies: Vec<PreviewBody>) -> bool {
        self.preview_bodies = bodies;
        self.changed()
    }
    fn set_table_blocks(&mut self, blocks: HashMap<TableBlockKey, TableBlockPaint>) -> bool {
        self.table_blocks = blocks;
        self.changed()
    }
    fn set_modal_overlay(&mut self, layers: Vec<OverlayLayer>, groups: Vec<OverlayGroup>) -> bool {
        self.overlay = (layers, groups);
        self.overlay_builds += 1;
        self.changed()
    }
    fn set_window_focused(&mut self, focused: bool) -> bool {
        let changed = self.window_focused != focused;
        self.window_focused = focused;
        changed
    }
    fn set_cursor_blink_visible(&mut self, visible: bool) -> bool {
        let changed = self.cursor_blink_visible != visible;
        self.cursor_blink_visible = visible;
        changed
    }
    fn set_chrome(
        &mut self,
        quads: Vec<ChromeQuad>,
        labels: Vec<ChromeLabel>,
        icons: Vec<ChromeIcon>,
    ) -> bool {
        self.chrome = (quads, labels, icons);
        self.changed()
    }
    fn math_hit_test(
        &self,
        metrics: CellMetrics,
        seat: SeatViewport,
        frame: &ViewportFrame,
        x: f64,
        y: f64,
    ) -> Option<MathHit> {
        bt_render::math_hit_test_for(metrics, seat, frame, x, y)
    }
    fn math_tool_boxes(
        &self,
        metrics: CellMetrics,
        seat: SeatViewport,
        frame: &ViewportFrame,
        hovered: &MathBlockAnchor,
    ) -> Option<MathToolBoxes> {
        bt_render::math_tool_boxes_for(metrics, seat, frame, hovered)
    }
    fn math_band_trace(
        &self,
        metrics: CellMetrics,
        seat: SeatViewport,
        frame: &ViewportFrame,
        named: &MathBlockAnchor,
    ) -> Option<MathBandTrace> {
        bt_render::math_band_trace_for(metrics, seat, frame, named)
    }
    fn math_band_face(
        &self,
        metrics: CellMetrics,
        seat: SeatViewport,
        frame: &ViewportFrame,
        named: &MathBlockAnchor,
    ) -> Option<MathBandFace> {
        bt_render::math_band_face_for(metrics, seat, frame, named)
    }
    fn peek_thumbnail_extent(
        &self,
        seat: SeatViewport,
        image_width_px: u32,
        image_height_px: u32,
    ) -> Option<(u32, u32)> {
        bt_render::peek_thumbnail_extent(
            seat.width as f32,
            seat.height as f32,
            self.base_metrics.padding_px,
            self.base_metrics.scale_factor as f32,
            image_width_px,
            image_height_px,
        )
    }
    fn ime_cursor_area(&self, metrics: CellMetrics, frame: &ViewportFrame) -> ImeCursorArea {
        bt_render::ime_cursor_area_for_metrics(metrics, frame)
    }
    fn resize(
        &mut self,
        _gpu: &HeadlessDevice,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        self.size = (width.max(1), height.max(1));
        self.ledger.record(WindowEffect::Resized(width, height));
        Ok(())
    }
    fn update_scale_factor(
        &mut self,
        gpu: &mut HeadlessDevice,
        scale_factor: f64,
    ) -> Result<CellMetrics, RenderError> {
        self.base_metrics = gpu
            .measure()
            .cell_metrics(scale_factor, gpu.terminal_font_size_logical_px)?;
        self.ledger.record(WindowEffect::Rescaled(scale_factor));
        Ok(self.base_metrics)
    }
    fn apply_font_change(&mut self, gpu: &mut HeadlessDevice) -> Result<CellMetrics, RenderError> {
        self.base_metrics = gpu.measure().cell_metrics(
            self.base_metrics.scale_factor,
            gpu.terminal_font_size_logical_px,
        )?;
        self.font_revision += 1;
        self.ledger.record(WindowEffect::FontApplied);
        Ok(self.base_metrics)
    }
    fn pane_cell_metrics(
        &self,
        gpu: &mut HeadlessDevice,
        font_size_logical_px: f32,
    ) -> Result<CellMetrics, RenderError> {
        gpu.measure()
            .cell_metrics(self.base_metrics.scale_factor, font_size_logical_px)
    }
    fn present_frame_with_phases(
        &mut self,
        token: WaitToken<'_, doors::PresentFrame>,
        _gpu: &mut HeadlessDevice,
        seats: &[SeatFrame<'_>],
        trigger: FrameTrigger,
        _phase: impl FnMut(PresentPhase),
    ) -> Result<PresentOutcome, RenderError> {
        let _ = token;
        self.perf_frame += 1;
        self.ledger.record(WindowEffect::Presented {
            seats: seats.len(),
            source: trigger.source,
        });
        let now = Instant::now();
        Ok(PresentOutcome::Presented(PresentReceipt {
            trigger,
            submitted_at: now,
            present_called_at: now,
        }))
    }
    fn measure_chrome_text(
        &mut self,
        gpu: &mut HeadlessDevice,
        text: &str,
        font_size_px: f32,
    ) -> f32 {
        gpu.measure().chrome_label(
            text,
            font_size_px,
            ChromeLabelWeight::Regular,
            0.0,
            false,
            false,
        )
    }
    fn measure_chrome_mono_text(
        &mut self,
        gpu: &mut HeadlessDevice,
        text: &str,
        font_size_px: f32,
    ) -> f32 {
        gpu.measure().chrome_label(
            text,
            font_size_px,
            ChromeLabelWeight::Regular,
            0.0,
            false,
            true,
        )
    }
    fn measure_chrome_label(
        &mut self,
        gpu: &mut HeadlessDevice,
        text: &str,
        font_size_px: f32,
        weight: ChromeLabelWeight,
        letter_spacing_em: f32,
        tabular_numerals: bool,
    ) -> f32 {
        gpu.measure().chrome_label(
            text,
            font_size_px,
            weight,
            letter_spacing_em,
            tabular_numerals,
            false,
        )
    }
    fn chrome_text_advances(
        &mut self,
        gpu: &mut HeadlessDevice,
        text: &str,
        font_size_px: f32,
    ) -> ChromeTextAdvances {
        gpu.measure().chrome_label_advances(
            text,
            font_size_px,
            ChromeLabelWeight::Regular,
            0.0,
            false,
        )
    }
    fn chrome_label_advances(
        &mut self,
        gpu: &mut HeadlessDevice,
        text: &str,
        font_size_px: f32,
        weight: ChromeLabelWeight,
        letter_spacing_em: f32,
        tabular_numerals: bool,
    ) -> ChromeTextAdvances {
        gpu.measure().chrome_label_advances(
            text,
            font_size_px,
            weight,
            letter_spacing_em,
            tabular_numerals,
        )
    }
    fn measure_preview_paragraph(
        &mut self,
        gpu: &mut HeadlessDevice,
        runs: &[PreviewRun],
        width_px: f32,
        font_size_px: f32,
        line_height_px: f32,
    ) -> f32 {
        gpu.measure()
            .preview_paragraph(runs, width_px, font_size_px, line_height_px)
    }
    fn measure_preview_paragraph_width(
        &mut self,
        gpu: &mut HeadlessDevice,
        runs: &[PreviewRun],
        font_size_px: f32,
        line_height_px: f32,
    ) -> f32 {
        gpu.measure()
            .preview_paragraph_width(runs, font_size_px, line_height_px)
    }
    fn measure_preview_run_boxes(
        &mut self,
        gpu: &mut HeadlessDevice,
        paragraph: &PreviewParagraph,
    ) -> Vec<PreviewRunBox> {
        gpu.measure().preview_run_boxes(paragraph)
    }
    fn measure_preview_hit(
        &mut self,
        gpu: &mut HeadlessDevice,
        paragraph: &PreviewParagraph,
        x: f32,
        y: f32,
    ) -> usize {
        gpu.measure().preview_hit(paragraph, x, y)
    }
    fn measure_preview_rows(
        &mut self,
        gpu: &mut HeadlessDevice,
        paragraph: &PreviewParagraph,
    ) -> Vec<PreviewTextRow> {
        gpu.measure().preview_rows(paragraph)
    }
    fn measure_preview_highlight(
        &mut self,
        gpu: &mut HeadlessDevice,
        paragraph: &PreviewParagraph,
        range: Range<usize>,
    ) -> Vec<[f32; 4]> {
        gpu.measure().preview_highlight(paragraph, range)
    }
    fn preview_mono_advance(&mut self, gpu: &mut HeadlessDevice, font_size_px: f32) -> f32 {
        gpu.measure().preview_mono_advance(font_size_px)
    }
}

/// **A composition tree that is not the platform's**: it records, and hosts no page's engine.
pub(crate) struct HeadlessCompositor {
    ledger: Ledger,
}

impl CompositorDoor for HeadlessCompositor {
    fn set_window_size(
        &self,
        token: WaitToken<'_, doors::CompositorWindowSize>,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let _ = token;
        self.ledger
            .record(WindowEffect::CompositionSize(width, height));
        Ok(())
    }
    fn skirt_covers_anything(&self) -> bool {
        false
    }
    fn set_covered_size(&self, width: u32, height: u32) -> Result<(), String> {
        self.ledger.record(WindowEffect::CoveredSize(width, height));
        Ok(())
    }
    fn set_page_ground_color(&self, premultiplied_srgb: [f32; 4]) -> Result<(), String> {
        self.ledger
            .record(WindowEffect::PageGround(premultiplied_srgb));
        Ok(())
    }
    fn attach_web_visual(&self, page: bt_platform::PageVisual) -> Result<(), String> {
        self.ledger.record(WindowEffect::PageAttached(page));
        Ok(())
    }
    fn detach_web_visual(&self, page: bt_platform::PageVisual) -> Result<(), String> {
        self.ledger.record(WindowEffect::PageDetached(page));
        Ok(())
    }
    fn hide_web_visual(&self, page: bt_platform::PageVisual) -> Result<(), String> {
        self.ledger.record(WindowEffect::PageHidden(page));
        Ok(())
    }
    fn place_web_visual(
        &self,
        page: bt_platform::PageVisual,
        _offset: (i32, i32),
        _clip: (f32, f32, f32, f32),
    ) -> Result<(), String> {
        self.ledger.record(WindowEffect::PagePlaced(page));
        Ok(())
    }
    fn set_page_cover(
        &self,
        page: bt_platform::PageVisual,
        rects: &[[f32; 4]],
    ) -> Result<(), String> {
        self.ledger
            .record(WindowEffect::PageCovered(page, rects.to_vec()));
        Ok(())
    }
    fn commit(&self, token: WaitToken<'_, doors::CompositorCommit>) -> Result<(), String> {
        let _ = token;
        self.ledger.record(WindowEffect::Committed);
        Ok(())
    }
    fn page_tree(&self) -> Result<PageTree<'_>, String> {
        Err("a headless window's tree hosts no page engine".to_owned())
    }
}

/// **A frame Folio draws whole**, as the Windows frame is.
pub(crate) struct HeadlessFrame {
    ledger: Ledger,
}

impl FrameDoor for HeadlessFrame {
    fn in_size_move(&self) -> bool {
        false
    }
    fn platform_chrome(&self) -> bt_platform::PlatformChrome {
        bt_platform::PlatformChrome::FOLIO_DRAWS_THE_WHOLE_BAR
    }
    fn set_window_band(&self, band_logical_px: f32) -> Result<(), String> {
        self.ledger
            .record(WindowEffect::WindowBand(band_logical_px));
        Ok(())
    }
    fn set_title(&self, title: &str) -> bool {
        self.ledger
            .record(WindowEffect::FrameTitle(title.to_owned()));
        false
    }
    fn press_title_bar(&self) -> Result<(), String> {
        self.ledger.record(WindowEffect::TitleBarPressed);
        Ok(())
    }
    fn set_title_bar_boxes(&self, boxes: &[bt_platform::TitleBarBox]) {
        self.ledger
            .record(WindowEffect::TitleBarBoxes(boxes.to_vec()));
    }
    fn set_min_client_size(&self, logical: Option<(u32, u32)>) -> Result<(), String> {
        self.ledger.record(WindowEffect::MinClientSize(logical));
        Ok(())
    }
}

/// **A chooser, a dialog or a menu that answers what the test scripts.**
pub(crate) struct HeadlessPicker {
    ledger: Ledger,
    /// The answers the next `take_result`s hand back, oldest first.
    pub(crate) answers: RefCell<VecDeque<Result<Option<PathBuf>, String>>>,
    pub(crate) menu_answers: RefCell<VecDeque<Result<bool, String>>>,
}

impl HeadlessPicker {
    fn new(ledger: Ledger) -> Self {
        Self {
            ledger,
            answers: RefCell::default(),
            menu_answers: RefCell::default(),
        }
    }
}

impl FolderPickerDoor for HeadlessPicker {
    fn request(&self, start: Option<&Path>) -> Result<bool, String> {
        self.ledger
            .record(WindowEffect::FolderAsked(start.map(Path::to_path_buf)));
        Ok(true)
    }
    fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
        self.answers.borrow_mut().pop_front()
    }
}

impl ImagePickerDoor for HeadlessPicker {
    fn request(
        &self,
        kind: bt_platform::FilePickKind,
        start: Option<&Path>,
        image_types: &str,
    ) -> Result<bool, String> {
        self.ledger.record(WindowEffect::FileAsked(
            kind,
            start.map(Path::to_path_buf),
            image_types.to_owned(),
        ));
        Ok(true)
    }
    fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
        self.answers.borrow_mut().pop_front()
    }
}

impl SavePickerDoor for HeadlessPicker {
    fn request(&self, start: Option<&Path>, name: &str) -> Result<bool, String> {
        self.ledger.record(WindowEffect::SaveAsked(
            start.map(Path::to_path_buf),
            name.to_owned(),
        ));
        Ok(true)
    }
    fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
        self.answers.borrow_mut().pop_front()
    }
}

impl MathMenuDoor for HeadlessPicker {
    fn request(&self, copy_latex: &str) -> Result<bool, String> {
        self.ledger
            .record(WindowEffect::MathMenuAsked(copy_latex.to_owned()));
        Ok(true)
    }
    fn take_result(&self) -> Option<Result<bool, String>> {
        self.menu_answers.borrow_mut().pop_front()
    }
}

/// **The input method's system caret**, recorded.
pub(crate) struct HeadlessImeCaret {
    ledger: Ledger,
}

impl ImeCaretDoor for HeadlessImeCaret {
    fn update(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.ledger.record(WindowEffect::ImeCaret(x, y));
        Ok(())
    }
    fn destroy(&mut self) {
        self.ledger.record(WindowEffect::ImeCaretDestroyed);
    }
}

/// **A loop that is not running**: it records being told to stop and how to wait.
pub(crate) struct HeadlessLoop {
    pub(crate) ledger: Ledger,
}

impl LoopDoor for HeadlessLoop {
    fn exit(&self) {
        self.ledger.record(WindowEffect::Exit);
    }
    fn set_control_flow(&self, control_flow: ControlFlow) {
        self.ledger.record(WindowEffect::ControlFlow(control_flow));
    }
}

/// **A headless window's native parts**, all recording to one ledger.
pub(crate) struct HeadlessParts {
    pub(crate) window: HeadlessWindow,
    pub(crate) glass: HeadlessGlass,
    pub(crate) compositor: HeadlessCompositor,
    pub(crate) frame: HeadlessFrame,
    pub(crate) math_menu: HeadlessPicker,
    pub(crate) folder_picker: HeadlessPicker,
    pub(crate) image_picker: HeadlessPicker,
    pub(crate) save_picker: HeadlessPicker,
    pub(crate) ime_caret: HeadlessImeCaret,
}

impl HeadlessParts {
    /// Every door of one window of `size` physical pixels at `scale`, recording to `ledger`.
    pub(crate) fn new(
        device: &HeadlessDevice,
        id: u64,
        size: (u32, u32),
        scale: f64,
        ledger: &Ledger,
    ) -> Self {
        Self {
            window: HeadlessWindow {
                id: WindowId::from(id),
                size: PhysicalSize::new(size.0, size.1),
                scale,
                focused: true,
                ledger: ledger.clone(),
                clipboard: HeadlessClipboard::default(),
                ime_facts: Cell::new(NativeFacts::default()),
                answers: Cell::new(PlatformAnswers::default()),
            },
            glass: HeadlessGlass::new(device, ledger.clone(), size, scale),
            compositor: HeadlessCompositor {
                ledger: ledger.clone(),
            },
            frame: HeadlessFrame {
                ledger: ledger.clone(),
            },
            math_menu: HeadlessPicker::new(ledger.clone()),
            folder_picker: HeadlessPicker::new(ledger.clone()),
            image_picker: HeadlessPicker::new(ledger.clone()),
            save_picker: HeadlessPicker::new(ledger.clone()),
            ime_caret: HeadlessImeCaret {
                ledger: ledger.clone(),
            },
        }
    }
}

/// **A headless application and its one window**, with what each records.
pub(crate) struct HeadlessFolio {
    pub(crate) app: App<Headless>,
    pub(crate) window: WindowRuntime<Headless>,
    pub(crate) ledger: Ledger,
    /// The scratch folder the application's stores write under, removed with it.
    pub(crate) scratch: Scratch,
}

/// **A test's scratch folder**, emptied away when the test lets go of it.
pub(crate) struct Scratch(pub(crate) PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl HeadlessFolio {
    /// One window of 1200 × 800 physical pixels at scale 1, holding `tabs` with `active` in front.
    pub(crate) fn with_tabs(tag: &str, tabs: Vec<TabState>, active: usize) -> Self {
        // A test that opens a second headless window is already on the window thread.
        if bt_platform::admission::role() != bt_platform::admission::Role::Window {
            crate::test_support::on_the_window_thread();
        }
        let root = bt_testpath::temp_path(tag);
        std::fs::create_dir_all(&root).expect("the headless application's scratch folder");
        let ledger = Ledger::default();
        let app = headless_app(
            HeadlessDevice::new(14.0),
            LoopEvents::default().proxy(),
            &root,
        );
        let parts = HeadlessParts::new(&app.gpu, 1, (1200, 800), 1.0, &ledger);
        let window = headless_window(&app, parts, tabs, active);
        let mut folio = Self {
            app,
            window,
            ledger,
            scratch: Scratch(root),
        };
        // As a window the product opens does once it holds its tabs: the seat set's layout,
        // carried to the panes.
        folio
            .runtime()
            .settle_seat_set_change()
            .expect("the window's tabs take their layout");
        folio.ledger.take();
        folio
    }

    /// One expose frame composed and handed to the glass, as a window's first paint is.
    pub(crate) fn draw(&mut self) {
        let mut runtime = self.runtime();
        runtime
            .publish_frame(FrameTrigger {
                occurred_at: Instant::now(),
                source: FrameSource::Expose,
            })
            .expect("a headless frame composes");
        runtime.redraw().expect("and reaches the glass");
    }

    /// The pointer moved to `position`, as winit reports a move.
    pub(crate) fn move_to(&mut self, position: PhysicalPosition<f64>) {
        self.runtime()
            .pointer_moved(position)
            .expect("a headless move is answered");
    }

    /// The left button went down at `position`.
    pub(crate) fn press(&mut self, position: PhysicalPosition<f64>) {
        self.move_to(position);
        self.runtime()
            .mouse_input(
                winit::event::ElementState::Pressed,
                winit::event::MouseButton::Left,
            )
            .expect("a headless press is answered");
    }

    /// The left button came up at `position`.
    pub(crate) fn release(&mut self, position: PhysicalPosition<f64>) {
        self.move_to(position);
        self.runtime()
            .mouse_input(
                winit::event::ElementState::Released,
                winit::event::MouseButton::Left,
            )
            .expect("a headless release is answered");
    }

    /// The facade over the two layers, as every event's handler gets it.
    pub(crate) fn runtime(&mut self) -> crate::Runtime<'_, Headless> {
        crate::Runtime {
            app: &mut self.app,
            window: &mut self.window,
        }
    }

    /// **Every clipboard request the window has made, answered and spent** (T-CLIPBOARD-LANE):
    /// the clipboard lane's worker serves them on its own thread, so the test waits, within the
    /// lane suite's patience, until the window's turn — which takes each answer as the loop's wake
    /// would bring it — finds nothing owed.
    ///
    /// The turn's clock is held at the moment the owed paste was asked, so the paste's deadline
    /// never falls inside the wait: a test that means the deadline passes its own clock to
    /// `Runtime::settle_clipboard`.
    pub(crate) fn settle_clipboard(&mut self) {
        crate::lane::wait_until("the window's clipboard answers", || {
            let mut runtime = self.runtime();
            let now = runtime
                .window
                .clipboard
                .pastes
                .iter()
                .map(|owed| owed.asked_at)
                .min()
                .unwrap_or_else(Instant::now);
            runtime
                .settle_clipboard(now)
                .expect("the clipboard's answers are spent");
            runtime.window.clipboard.is_empty()
        });
    }
}

/// **A headless application with its one window**: the loop's own `FolioApp`, for the doors a
/// `Runtime` does not hold — the quit's transaction, the dispatch of a window event.
pub(crate) struct HeadlessApplication {
    pub(crate) folio: crate::FolioApp<Headless>,
    pub(crate) id: WindowId,
    /// The loop the application's turns run in, recording to the window's ledger.
    pub(crate) event_loop: HeadlessLoop,
    _scratch: Scratch,
}

impl HeadlessApplication {
    /// `folio`'s application and window, handed to a `FolioApp` as a launch hands them.
    pub(crate) fn of(folio: HeadlessFolio) -> Self {
        let HeadlessFolio {
            app,
            window,
            ledger,
            scratch,
        } = folio;
        let id = window.window.id;
        // The loop's witnesses over values of this test's own: a headless application must not
        // settle the process's data directory (`%APPDATA%`), and its profile table is the one the
        // test's `App` was built over.
        let mut application = crate::FolioApp::<Headless>::new(
            app.event_proxy.clone(),
            crate::cli::CliRequest::default(),
            None,
            // Leaked per application rather than shared: one test's witness is no other test's.
            crate::startup::Settled::planted(Box::leak(Box::new(
                crate::persist::DataDirectory::planted(PathBuf::new()),
            ))),
            crate::profiles::settle_registry(),
            crate::explorer_menu::settle_package(),
        );
        application.app = Some(app);
        application.windows.insert(id, window);
        Self {
            folio: application,
            id,
            event_loop: HeadlessLoop { ledger },
            _scratch: scratch,
        }
    }

    /// The window's facade, as the application builds it for an event.
    pub(crate) fn runtime(&mut self) -> crate::Runtime<'_, Headless> {
        self.folio
            .runtime(self.id)
            .expect("the headless window is open")
    }

    /// One window event, dispatched as the loop dispatches it.
    pub(crate) fn window_event(&mut self, event: winit::event::WindowEvent) {
        self.folio.on_window_event(&self.event_loop, self.id, event);
    }
}

/// **A headless application**: every field at the value a launch with nothing on disk gives it,
/// its stores at `root`, its workers started and waking `proxy` — but its storage lane, which no
/// worker serves and none can start: every job handed to it is refused at the door (a preview
/// save says "not saved"), and nothing a test does reaches a disk through it.
pub(crate) fn headless_app(gpu: HeadlessDevice, proxy: LoopProxy, root: &Path) -> App<Headless> {
    use crate::*;
    let wake = |event: fn() -> AppEvent| {
        let proxy = proxy.clone();
        move || {
            let _ = proxy.send_event(event());
        }
    };
    let lane = persist::StorageLane::without_a_worker(wake(|| AppEvent::DocumentsAnswered));
    let session_store =
        persist::SessionStore::at(root.join("session.json"), root.join("session.lock"));
    let settings_store = persist::SettingsStore::at_on(root.join("settings.json"), lane.clone());
    let mut keybindings_store =
        persist::KeybindingsStore::at_on(root.join("keybindings.json"), lane.clone());
    let (shortcuts, _) =
        settings_bundle::shortcuts_at_start(&mut keybindings_store, bt_platform::host_platform());
    let git_mutations = git_mutation_lane::GitMutationLane::new(
        git_mutation_lane::RepoTurns::new(),
        wake(|| AppEvent::GitReady),
    );
    let git_turns = Arc::clone(git_mutations.turns());
    App::assemble(AppParts {
        gpu,
        tab_ids: TabIds::new(),
        event_proxy: proxy.clone(),
        handoff_lane: handoff_lane::HandoffLane::spawn(wake(|| AppEvent::HandoffAnswered))
            .expect("the handoff lane starts"),
        clipboard_lane: clipboard_lane::ClipboardLane::spawn(wake(|| AppEvent::ClipboardAnswered)),
        layout_tables: layout_tables::LayoutTables::spawn(proxy.clone(), Vec::new())
            .expect("the layout tables start"),
        math_worker: Arc::new(computation_lane::ComputationLane::new(wake(|| {
            AppEvent::MathReady
        }))),
        preview_saves: preview::PreviewSaves::on(lane.clone()),
        git_mutations,
        foreground_program_worker: foreground_program::Worker::spawn(wake(|| AppEvent::MathReady))
            .expect("the foreground worker starts"),
        files_worker: files::FilesWorker::spawn(proxy.clone()).expect("the files worker starts"),
        file_index_worker: palette_index::IndexWorker::spawn(proxy.clone())
            .expect("the index worker starts"),
        preview_worker: preview::PreviewWorker::spawn(proxy.clone())
            .expect("the preview worker starts"),
        git_worker: git::GitWorker::spawn(proxy.clone(), git_turns).expect("the git worker starts"),
        acrylic_available: false,
        startup_started: Instant::now(),
        trace_startup: false,
        trace_resize: false,
        trace_layout_events: false,
        trace_perf: false,
        motion: Motion::Full,
        profile_programs: profiles::ProfilePrograms::unknown(),
        startup_shells: None,
        context_menu_installed: false,
        claude_hooks_installed: false,
        agent_config_refusals: [None, None, None],
        codex_notify_installed: false,
        copilot_hooks_installed: false,
        copilot_readiness: attention_copilot::Readiness::default(),
        notifications: NotificationDesk::new(proxy.clone()),
        session_store,
        settings_store,
        shortcuts,
        keybindings_store,
        keybindings_fault: None,
        cli_refusals: Vec::new(),
        cli_preview: None,
        profiles_store: persist::ProfilesStore::at_on(root.join("profiles.json"), lane.clone()),
        profiles_fault: None,
        pins_store: pins::PinsStore::at_on(root.join("pins.json"), lane),
        pins_fault: None,
        settings_fault: None,
        session_fault: None,
        recent: seed::SeedVault::default(),
        recent_folders: recent_folders::RecentFolders::default(),
        window_pictures: Vec::new(),
        update_job: update_job::Job::default(),
    })
}

/// **A headless window over `parts`**, holding `tabs` with `active` in front, assembled by the
/// product's own `new_window_runtime` from the resting value of everything else.
pub(crate) fn headless_window(
    app: &App<Headless>,
    parts: HeadlessParts,
    tabs: Vec<TabState>,
    active: usize,
) -> WindowRuntime<Headless> {
    use crate::*;
    let HeadlessParts {
        window,
        glass,
        compositor,
        frame,
        math_menu,
        folder_picker,
        image_picker,
        save_picker,
        ime_caret,
    } = parts;
    let size = window.size;
    let mut runtime = new_window_runtime::<Headless>(NewWindowParts {
        ime_report: ime_report::Report::default(),
        parked_pans: runtime::pointer::touch::ParkedPans::default(),
        favicons: Rc::clone(&app.favicons),
        renderer: glass,
        tabs,
        active_tab: active,
        pty_wake: PtyWakeSignal::new(app.event_proxy.clone()),
        translucency_available: false,
        custom_window_frame: frame,
        compositor,
        window,
        math_context_menu: math_menu,
        folder_picker,
        image_picker,
        save_picker: Some(save_picker),
        ime_system_caret: ime_caret,
        rail: seats::RailState::default(),
        seat_viewport: LogicalRect::new(
            bt_layout::LogicalPx::ZERO,
            bt_layout::LogicalPx::ZERO,
            bt_layout::LogicalPx::px(i64::from(size.width)),
            bt_layout::LogicalPx::px(i64::from(size.height)),
        ),
        pending_restore: Vec::new(),
        raise_restore_prompt: false,
        placeholder_tab: None,
        motion: app.motion,
        event_proxy: app.event_proxy.clone(),
    });
    runtime.window_shown = true;
    runtime
}

/// **A key going down**, as the dispatch hands it to the window: `logical` with no modifier
/// applied either way, at the standard location, typing `text`.
pub(crate) fn key_down(
    logical: winit::keyboard::Key,
    physical: winit::keyboard::KeyCode,
    text: Option<&str>,
) -> crate::input::Keystroke {
    crate::input::Keystroke {
        physical_key: winit::keyboard::PhysicalKey::Code(physical),
        logical_key: logical.clone(),
        text: text.map(winit::keyboard::SmolStr::new),
        location: winit::keyboard::KeyLocation::Standard,
        pressed: true,
        repeat: false,
        unmodified_key: logical,
        modified_text: text.map(winit::keyboard::SmolStr::new),
    }
}

/// **One step of a start**, as the headless start records it (T-STARTUP-ORDER-SHARED): the
/// steps `startup::begin` and `startup::resume` take, each named once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartupStep {
    /// A removal verb answered.
    Removal,
    /// The command line read.
    CommandLine,
    /// The parsed request handed over, or kept.
    HandOver,
    /// Windows PowerShell's language-mode question started.
    LanguageQuestion,
    /// The startup's `$PROFILE` observation started.
    StartupMigration,
    /// Folio's own script's preparation started.
    ScriptPreparation,
    /// The first window opened, through the backend's door.
    WindowOpened,
    /// The parser questions about the PowerShell rows put to the probe.
    PowerShellQuestions,
    /// The first window's first tab made.
    FirstTab,
}

/// **What a headless start did**: its steps in the order they ran, and what each step that
/// carries something was handed.
#[derive(Debug, Default)]
pub(crate) struct HeadlessStart {
    pub(crate) steps: Vec<StartupStep>,
    /// The request the hand-over was handed.
    pub(crate) handed_over: Option<String>,
    /// The program answers the PowerShell questions were asked about.
    pub(crate) asked: Vec<String>,
    /// The program answers the first tab was made from.
    pub(crate) born_from: Vec<String>,
}

/// **A start with no process to leave and no window to open**: `startup::begin` and then, as the
/// loop's first resume does, `startup::resume`, each step a stand-in that records
/// itself. The command line is `argv`; the program walk answers `walk` one batch per look, as
/// `programs_lane::take` hands over whatever has landed since the last — one look when the
/// window is titled, one when the questions are asked.
pub(crate) fn start(argv: &str, walk: Vec<Vec<String>>) -> HeadlessStart {
    let ran = RefCell::new(HeadlessStart::default());
    let step = |step: StartupStep| ran.borrow_mut().steps.push(step);
    let walk = RefCell::new(VecDeque::from(walk));
    let look = |answers: &mut Vec<String>| {
        answers.extend(walk.borrow_mut().pop_front().unwrap_or_default());
    };
    let request = crate::startup::begin(crate::startup::FrontDoor {
        removal: || step(StartupStep::Removal),
        parse: || {
            step(StartupStep::CommandLine);
            argv.to_owned()
        },
        claim: |request: String| {
            step(StartupStep::HandOver);
            ran.borrow_mut().handed_over = Some(request.clone());
            request
        },
    });
    let mut answers = Vec::new();
    let opened = crate::startup::resume(
        &mut answers,
        crate::startup::FirstWindow {
            language: || {
                step(StartupStep::LanguageQuestion);
                crate::shell_integration::LanguageAsked::stand_in()
            },
            migration: || {
                step(StartupStep::StartupMigration);
                crate::shell_integration::Migrated::stand_in()
            },
            script: || {
                step(StartupStep::ScriptPreparation);
                crate::shell_integration::ScriptPrepared::stand_in()
            },
            // Handed the request the run carries, as `Runtime::create` is handed the parsed one.
            window: |answers: &mut Vec<String>| {
                look(answers);
                step(StartupStep::WindowOpened);
                Ok::<_, std::convert::Infallible>(request)
            },
            questions: |answers: &mut Vec<String>| {
                look(answers);
                step(StartupStep::PowerShellQuestions);
                ran.borrow_mut().asked = answers.clone();
                crate::shell_integration::QuestionsAsked::stand_in()
            },
            first_tab: |answers: &mut Vec<String>, _window: &mut String| {
                step(StartupStep::FirstTab);
                ran.borrow_mut().born_from = answers.clone();
                Ok(())
            },
        },
    );
    let Ok(_) = opened;
    ran.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_platform::admission::admitted;
    use winit::window::CursorIcon;

    /// RED — **the headless runtime records what the window would do**: every door of the
    /// window seam, knocked on once, leaves exactly one typed effect carrying its arguments, in
    /// the order the doors were knocked on; the loop's wake lands on the loop's own list.
    ///
    /// MUTATION: drop the `record` from any one door (the window's `set_visible`, the frame's
    /// `press_title_bar`, the loop's `exit`, …) and the ledger is one effect short.
    #[test]
    fn the_headless_runtime_records_what_the_window_would_do() {
        crate::test_support::on_the_window_thread();
        let ledger = Ledger::default();
        let mut device = HeadlessDevice::new(14.0);
        let mut parts = HeadlessParts::new(&device, 7, (640, 480), 1.5, &ledger);
        let page = bt_platform::PageVisual { tab: 3, seat: 4 };
        let folder = PathBuf::from("文件夹");

        let window = &parts.window;
        assert_eq!(window.id(), WindowId::from(7));
        assert_eq!(window.inner_size(), PhysicalSize::new(640, 480));
        window.request_redraw();
        window.set_title("标题 title");
        window.set_visible(true);
        window.set_minimized(true);
        window.set_maximized(false);
        window.focus_window();
        window.set_cursor(Cursor::Icon(CursorIcon::Text));
        window.set_ime_cursor_area(
            PhysicalPosition::new(10, 20).into(),
            PhysicalSize::new(3, 18).into(),
        );
        window.set_option_as_alt(true);
        window.note("诊断 a line, said as it is written");
        // The window's questions to the system answer the documented stand-ins, and record
        // nothing; its instructions record what they were told.
        assert!(!window.is_iconic() && !window.is_cloaked() && window.is_exposed());
        assert_eq!((window.outer_rect(), window.work_area()), (None, None));
        assert_eq!(window.glass_at(3, 4), crate::GlassHere::Unknown);
        assert_eq!(window.pointer_in_window(), None);
        assert_eq!(
            window.present_facts(),
            bt_platform::NativePresentFacts::default()
        );
        let rect = bt_platform::WindowRect {
            left: 10,
            top: 20,
            right: 650,
            bottom: 500,
        };
        window.stand_at(rect).expect("recorded");
        window.set_backdrop(true).expect("recorded");
        window.set_topmost(true).expect("recorded");
        window.set_dark_mode(false).expect("recorded");
        window
            .set_class_background(Some([1, 2, 3]))
            .expect("recorded");
        assert!(window.take_foreground(), "the system lets it");
        window.take_keyboard_focus().expect("recorded");
        window.request_close().expect("recorded");
        window.flash();
        window.stop_flashing();
        window
            .taskbar()
            .expect("a headless window has a button")
            .set_progress(bt_platform::TaskbarProgress::CLEARED)
            .expect("recorded");
        assert!(window.watch_system_settings(Box::new(|_| {})).is_none());
        assert!(
            window
                .watch_session_end(Box::new(|_| {}), Box::new(String::new))
                .is_none()
        );
        assert!(
            window.page_address(page).is_err(),
            "a headless window hosts no web engine, and says so"
        );
        // The clipboard's request reaches the window's own scripted door, on the lane's worker.
        let clipboard_lane = crate::clipboard_lane::ClipboardLane::spawn(|| {});
        let mut write = window
            .clipboard(
                &clipboard_lane,
                crate::clipboard_lane::Ask::Write("剪贴 clip".to_owned()),
            )
            .expect("the lane admits the write");
        let mut ended = None;
        crate::lane::wait_until("the headless clipboard's write", || {
            ended = write.take();
            ended.is_some()
        });
        assert_eq!(
            ended,
            Some(crate::lane::Terminal::Done(
                crate::clipboard_lane::Reply::Written
            ))
        );
        assert_eq!(window.clipboard.take_written(), ["剪贴 clip".to_owned()]);
        let lane =
            crate::handoff_lane::HandoffLane::spawn(|| {}).expect("the hand-off lane starts");
        assert!(
            window
                .hand_off(&lane, bt_platform::Handoff::Reveal(folder.clone()))
                .is_err(),
            "a headless window is named to no door, and says so"
        );

        let glass = &mut parts.glass;
        assert_eq!(glass.scale_factor(), 1.5);
        let trigger = FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        };
        let presented = admitted::<doors::PresentFrame, _>(|token| {
            glass.present_frame_with_phases(token, &mut device, &[], trigger, |_| {})
        })
        .expect("the present door is admitted on a test thread")
        .expect("a headless present answers");
        assert!(matches!(presented, PresentOutcome::Presented(_)));
        glass.resize(&device, 800, 600).expect("a headless resize");
        glass
            .update_scale_factor(&mut device, 2.0)
            .expect("a headless rescale");
        assert_eq!(glass.scale_factor(), 2.0);
        glass
            .apply_font_change(&mut device)
            .expect("a headless face change");

        let compositor = &parts.compositor;
        admitted::<doors::CompositorWindowSize, _>(|token| {
            compositor.set_window_size(token, 800, 600)
        })
        .expect("admitted")
        .expect("recorded");
        compositor.set_covered_size(799, 599).expect("recorded");
        compositor
            .set_page_ground_color([0.1, 0.2, 0.3, 1.0])
            .expect("recorded");
        compositor.attach_web_visual(page).expect("recorded");
        compositor
            .place_web_visual(page, (1, 2), (0.0, 0.0, 9.0, 9.0))
            .expect("recorded");
        compositor
            .set_page_cover(page, &[[1.0, 2.0, 3.0, 4.0]])
            .expect("recorded");
        compositor.hide_web_visual(page).expect("recorded");
        compositor.detach_web_visual(page).expect("recorded");
        admitted::<doors::CompositorCommit, _>(|token| compositor.commit(token))
            .expect("admitted")
            .expect("recorded");
        assert!(
            compositor.page_tree().is_err(),
            "and it is no tree a page's engine could hang in"
        );

        let frame = &parts.frame;
        assert!(
            !frame.set_title("帧 frame"),
            "the window's own title holds it"
        );
        frame.press_title_bar().expect("recorded");
        frame.set_window_band(40.0).expect("recorded");
        let maximize = bt_platform::TitleBarBox {
            rect: [0, 0, 10, 10],
            role: bt_platform::TitleBarBoxRole::MaximizeButton,
        };
        frame.set_title_bar_boxes(&[maximize]);
        frame
            .set_min_client_size(Some((300, 200)))
            .expect("recorded");

        FolderPickerDoor::request(&parts.folder_picker, Some(&folder)).expect("asked");
        ImagePickerDoor::request(
            &parts.image_picker,
            bt_platform::FilePickKind::Image,
            None,
            "图片 images",
        )
        .expect("asked");
        SavePickerDoor::request(&parts.save_picker, Some(&folder), "导出.json").expect("asked");
        MathMenuDoor::request(&parts.math_menu, "复制 copy").expect("asked");
        parts
            .folder_picker
            .answers
            .borrow_mut()
            .push_back(Ok(Some(folder.clone())));
        assert_eq!(
            FolderPickerDoor::take_result(&parts.folder_picker),
            Some(Ok(Some(folder.clone()))),
            "a chooser answers what the test scripted, once"
        );
        assert_eq!(FolderPickerDoor::take_result(&parts.folder_picker), None);
        parts.ime_caret.update(5, 6).expect("recorded");
        parts.ime_caret.destroy();

        let event_loop = HeadlessLoop {
            ledger: ledger.clone(),
        };
        event_loop.set_control_flow(ControlFlow::Poll);
        event_loop.exit();

        let events = LoopEvents::default();
        events
            .proxy()
            .send_event(AppEvent::PtyOutput)
            .expect("a headless loop takes every wake");

        assert_eq!(
            ledger.take(),
            vec![
                WindowEffect::Redraw,
                WindowEffect::Title("标题 title".to_owned()),
                WindowEffect::Visible(true),
                WindowEffect::Minimized(true),
                WindowEffect::Maximized(false),
                WindowEffect::Focused,
                WindowEffect::Cursor(Cursor::Icon(CursorIcon::Text)),
                WindowEffect::ImeArea([10, 20, 3, 18]),
                WindowEffect::OptionAsAlt(true),
                WindowEffect::Noted("诊断 a line, said as it is written".to_owned()),
                WindowEffect::StoodAt(bt_platform::WindowRect {
                    left: 10,
                    top: 20,
                    right: 650,
                    bottom: 500,
                }),
                WindowEffect::Backdrop(true),
                WindowEffect::Topmost(true),
                WindowEffect::DarkMode(false),
                WindowEffect::ClassBackground(Some([1, 2, 3])),
                WindowEffect::ForegroundTaken,
                WindowEffect::KeyboardFocusTaken,
                WindowEffect::CloseRequested,
                WindowEffect::Flashed,
                WindowEffect::FlashStopped,
                WindowEffect::TaskbarProgress(bt_platform::TaskbarProgress::CLEARED),
                WindowEffect::SystemSettingsWatched,
                WindowEffect::SessionEndWatched,
                WindowEffect::PageAddressed(page),
                WindowEffect::HandedOff(bt_platform::Handoff::Reveal(folder.clone())),
                WindowEffect::Presented {
                    seats: 0,
                    source: FrameSource::Expose,
                },
                WindowEffect::Resized(800, 600),
                WindowEffect::Rescaled(2.0),
                WindowEffect::FontApplied,
                WindowEffect::CompositionSize(800, 600),
                WindowEffect::CoveredSize(799, 599),
                WindowEffect::PageGround([0.1, 0.2, 0.3, 1.0]),
                WindowEffect::PageAttached(page),
                WindowEffect::PagePlaced(page),
                WindowEffect::PageCovered(page, vec![[1.0, 2.0, 3.0, 4.0]]),
                WindowEffect::PageHidden(page),
                WindowEffect::PageDetached(page),
                WindowEffect::Committed,
                WindowEffect::FrameTitle("帧 frame".to_owned()),
                WindowEffect::TitleBarPressed,
                WindowEffect::WindowBand(40.0),
                WindowEffect::TitleBarBoxes(vec![maximize]),
                WindowEffect::MinClientSize(Some((300, 200))),
                WindowEffect::FolderAsked(Some(folder.clone())),
                WindowEffect::FileAsked(
                    bt_platform::FilePickKind::Image,
                    None,
                    "图片 images".to_owned()
                ),
                WindowEffect::SaveAsked(Some(folder), "导出.json".to_owned()),
                WindowEffect::MathMenuAsked("复制 copy".to_owned()),
                WindowEffect::ImeCaret(5, 6),
                WindowEffect::ImeCaretDestroyed,
                WindowEffect::ControlFlow(ControlFlow::Poll),
                WindowEffect::Exit,
            ],
        );
        let sent = events.take();
        assert!(
            matches!(sent.as_slice(), [AppEvent::PtyOutput]),
            "the loop's one wake: {sent:?}"
        );
    }

    /// RED (T-DIAGNOSTICS-TEST-SINK) — **the window's `diagnostics.log` door keeps the exact text
    /// of every line up to its cap, and counts the rest**: [`NOTES_KEPT`] lines are kept in the
    /// order said, byte for byte (a line of several scripts, a line with a tab); the one past them
    /// is counted and not kept; a plain take refuses, loudly, rather than hand back a record
    /// missing a line; a take with the count empties both, and the next line is kept again.
    ///
    /// MUTATION: keep every line (no cap) — the ledger holds one line too many and counts none;
    /// record a line trimmed or reformatted — the kept text differs; let `take` pass with lines
    /// past the cap — the refusal does not come.
    #[test]
    fn the_window_keeps_the_lines_it_is_told_up_to_its_cap_and_counts_the_rest() {
        crate::test_support::on_the_window_thread();
        let ledger = Ledger::default();
        let device = HeadlessDevice::new(14.0);
        let parts = HeadlessParts::new(&device, 8, (640, 480), 1.0, &ledger);
        let said: Vec<String> = (0..=NOTES_KEPT)
            .map(|line| format!("Folio: 第{line}行 line	{line} — said "))
            .collect();
        for line in &said {
            parts.window.note(line);
        }
        assert_eq!(
            ledger.notes_beyond_the_cap(),
            1,
            "the line past the cap is counted"
        );
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ledger.take()));
        assert!(
            refused.is_err(),
            "a take with a line past the cap fails loudly, and leaves the ledger as it was"
        );
        let kept: Vec<WindowEffect> = said[..NOTES_KEPT]
            .iter()
            .cloned()
            .map(WindowEffect::Noted)
            .collect();
        assert_eq!(
            ledger.take_with_the_count(),
            (kept, 1),
            "the first {NOTES_KEPT}, as they were said, and the one past them counted"
        );
        assert_eq!(ledger.notes_beyond_the_cap(), 0, "a take empties the count");
        parts.window.note("after the take");
        assert_eq!(
            ledger.take(),
            vec![WindowEffect::Noted("after the take".to_owned())],
            "the cap is per take"
        );
    }

    /// RED — **the backend's two doors that are not a part's record what they were asked**: every
    /// window's surfaces rebuilt after a device loss, and a second window asked of the loop — which
    /// a headless loop refuses, having recorded it.
    ///
    /// MUTATION: drop the `record` from `Headless::rebuild_every_surface` or `Headless::open_window`
    /// and the ledger is that effect short.
    #[test]
    fn the_backends_rebuild_and_second_window_are_recorded() {
        let mut folio = HeadlessFolio::with_tabs(
            "headless-backend-doors",
            vec![crate::test_support::tab_holding(
                crate::test_support::leaf_saying("后端 backend"),
            )],
            0,
        );
        folio.ledger.take();
        Headless::rebuild_every_surface(&mut folio.app.gpu, vec![&mut folio.window])
            .expect("a headless rebuild answers");
        let like = folio.window.window.id();
        let event_loop = HeadlessLoop {
            ledger: folio.ledger.clone(),
        };
        assert!(
            Headless::open_window(
                &event_loop,
                &mut folio.app,
                &crate::NewWindowPlan::fresh(like),
                None,
            )
            .is_err(),
            "a headless loop opens no second window"
        );
        assert_eq!(
            folio.ledger.take(),
            vec![WindowEffect::SurfacesRebuilt(1), WindowEffect::WindowOpened]
        );
    }
}
