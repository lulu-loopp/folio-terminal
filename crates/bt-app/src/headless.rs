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
//! scripts, the window has no native handle and its compositor no platform tree.

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
    ImeCaretDoor, LoopDoor, LoopProxy, MathMenuDoor, SavePickerDoor, WindowHost,
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
    /// Text put on the clipboard.
    ClipboardWritten(String),
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
    /// The title bar's own boxes.
    TitleBarBoxes(Vec<[i32; 4]>),
    /// The client's smallest size.
    MinClientSize(Option<(u32, u32)>),
    /// The folder chooser asked for, starting where it was told.
    FolderAsked(Option<PathBuf>),
    /// The file chooser asked for one kind of file.
    FileAsked(bt_platform::FilePickKind, Option<PathBuf>),
    /// The save dialog asked for, with its starting folder and name.
    SaveAsked(Option<PathBuf>, String),
    /// The formula's menu asked for.
    MathMenuAsked,
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

/// **A window's effects, in the order its doors were knocked on.** One per headless window,
/// shared by its stand-ins.
#[derive(Clone, Debug, Default)]
pub(crate) struct Ledger(Rc<RefCell<Vec<WindowEffect>>>);

impl Ledger {
    fn record(&self, effect: WindowEffect) {
        self.0.borrow_mut().push(effect);
    }

    /// Everything recorded so far, emptied.
    pub(crate) fn take(&self) -> Vec<WindowEffect> {
        std::mem::take(&mut self.0.borrow_mut())
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
}

/// **A window with no window**: an id, a size, a scale and a ledger.
pub(crate) struct HeadlessWindow {
    pub(crate) id: WindowId,
    pub(crate) size: PhysicalSize<u32>,
    pub(crate) scale: f64,
    pub(crate) focused: bool,
    pub(crate) ledger: Ledger,
    /// What the clipboard holds, as this window's doors read and write it.
    pub(crate) clipboard: RefCell<Option<String>>,
    /// What the window's input-method facts door answers: whatever the test scripted, default
    /// facts (every field `None`) until it does.
    pub(crate) ime_facts: Cell<NativeFacts>,
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
        Some(false)
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
    fn native_handle(&self) -> anyhow::Result<bt_platform::NativeWindow> {
        Err(anyhow::anyhow!("a headless window has no native handle"))
    }
    fn ime_facts(&self) -> NativeFacts {
        self.ledger.record(WindowEffect::ImeFactsRead);
        self.ime_facts.get()
    }
    fn set_option_as_alt(&self, option_sends_alt: bool) {
        self.ledger
            .record(WindowEffect::OptionAsAlt(option_sends_alt));
    }
    fn clipboard_text(&self) -> Result<String, String> {
        self.clipboard
            .borrow()
            .clone()
            .ok_or_else(|| "the headless clipboard holds no text".to_owned())
    }
    fn set_clipboard_text(&self, text: &str) -> Result<(), String> {
        *self.clipboard.borrow_mut() = Some(text.to_owned());
        self.ledger
            .record(WindowEffect::ClipboardWritten(text.to_owned()));
        Ok(())
    }
    fn clipboard_payload(&self) -> Result<bt_platform::ClipboardPayload, String> {
        self.clipboard
            .borrow()
            .clone()
            .map(bt_platform::ClipboardPayload::Text)
            .ok_or_else(|| "the headless clipboard holds nothing".to_owned())
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
    fn platform_tree(&self) -> Result<&bt_platform::Compositor, String> {
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
    fn set_title_bar_boxes(&self, boxes: &[[i32; 4]]) {
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
    ) -> Result<bool, String> {
        self.ledger
            .record(WindowEffect::FileAsked(kind, start.map(Path::to_path_buf)));
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
    fn request(&self) -> Result<bool, String> {
        self.ledger.record(WindowEffect::MathMenuAsked);
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
                clipboard: RefCell::new(None),
                ime_facts: Cell::new(NativeFacts::default()),
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
        let mut application = crate::FolioApp::<Headless>::new(
            app.event_proxy.clone(),
            crate::cli::CliRequest::default(),
            None,
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
        psreadline_documents: None,
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
        assert!(
            window.native_handle().is_err(),
            "a headless window has no handle"
        );
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
        window
            .set_clipboard_text("剪贴 clip")
            .expect("the headless clipboard takes text");
        assert_eq!(window.clipboard_text().as_deref(), Ok("剪贴 clip"));
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
            compositor.platform_tree().is_err(),
            "and it is no platform tree a page's engine could hang in"
        );

        let frame = &parts.frame;
        assert!(
            !frame.set_title("帧 frame"),
            "the window's own title holds it"
        );
        frame.press_title_bar().expect("recorded");
        frame.set_window_band(40.0).expect("recorded");
        frame.set_title_bar_boxes(&[[0, 0, 10, 10]]);
        frame
            .set_min_client_size(Some((300, 200)))
            .expect("recorded");

        FolderPickerDoor::request(&parts.folder_picker, Some(&folder)).expect("asked");
        ImagePickerDoor::request(&parts.image_picker, bt_platform::FilePickKind::Image, None)
            .expect("asked");
        SavePickerDoor::request(&parts.save_picker, Some(&folder), "导出.json").expect("asked");
        MathMenuDoor::request(&parts.math_menu).expect("asked");
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
                WindowEffect::ClipboardWritten("剪贴 clip".to_owned()),
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
                WindowEffect::TitleBarBoxes(vec![[0, 0, 10, 10]]),
                WindowEffect::MinClientSize(Some((300, 200))),
                WindowEffect::FolderAsked(Some(folder.clone())),
                WindowEffect::FileAsked(bt_platform::FilePickKind::Image, None),
                WindowEffect::SaveAsked(Some(folder), "导出.json".to_owned()),
                WindowEffect::MathMenuAsked,
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
