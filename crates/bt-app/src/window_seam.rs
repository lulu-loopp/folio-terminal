//! **The window seam** (T-HEADLESS-RUNTIME-SEAM): everything a window's runtime does to the
//! machine's window, its glass and its loop, named as traits.
//!
//! A [`crate::Runtime`] method changes the window's state, and it reaches the platform only
//! through these doors: the window handle ([`WindowHost`]), the window's half of the renderer
//! ([`Glass`]) and the process's device layer under it ([`DeviceLayer`]), the composition tree a
//! page's visual hangs in ([`CompositorDoor`]), the custom frame ([`FrameDoor`]), the window's
//! native pickers and menu ([`FolderPickerDoor`], [`ImagePickerDoor`], [`SavePickerDoor`],
//! [`MathMenuDoor`]), the input method's system caret ([`ImeCaretDoor`]) and the wake that brings
//! the loop round ([`LoopProxy`]). [`Backend`] names one implementation of each, and `App`,
//! `WindowRuntime`, `Runtime` and `FolioApp` are written over a backend.
//!
//! The product has exactly one backend, [`Native`]: winit's window, `bt-render`'s
//! `WindowRenderer` and `GpuContext`, and `bt-platform`'s handles, whose inherent methods every
//! trait here mirrors name for name and argument for argument — so a call written in a runtime
//! method is the same call it always was, and the product's behaviour is the same code. What the
//! runtime asks of the system about its window (where it is, how it is dressed, the keyboard, its
//! taskbar button, a page's place in it) is a [`WindowHost`] door of its own, whose native impl
//! makes the `bt-platform` call with the window's handle: no door hands the handle out. The only
//! other backend is the test suite's `headless::Headless`, which records what a window would have
//! done as typed values; nothing in the product asks which backend it is running on.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_platform::admission::{WaitToken, doors};
use bt_render::{
    CellMetrics, ChromeLabelWeight, ChromeTextAdvances, FrameTrigger, GlyphCensus, ImeCursorArea,
    OverlayGroup, OverlayLayer, PeekImageOverlay, PresentConfiguration, PresentOutcome,
    PresentPhase, PresentationGeometry, PreviewBody, PreviewImage, PreviewParagraph, PreviewRun,
    PreviewRunBox, PreviewTextFrame, PreviewTextRow, RenderError, RendererPresentState, SeatFrame,
    SeatViewport, TableBlockKey, TableBlockPaint, VideoLayer, WebHole,
};
use bt_render::{
    ChromeIcon, ChromeLabel, ChromeQuad, MathBandFace, MathBandTrace, MathHit, MathToolBoxes,
};
use bt_viewport::{MathBlockAnchor, ViewportFrame};
use winit::dpi::{PhysicalSize, Position, Size};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopClosed, EventLoopProxy};
use winit::window::{Cursor, Window, WindowId};

use crate::AppEvent;

/// **How any thread brings the window thread's loop round with news** — winit's
/// `EventLoopProxy` in the product, a recording channel in a test.
///
/// One type rather than a backend parameter because it crosses into every worker and watch
/// that wakes the loop, none of which is otherwise written over a backend. Cloned as freely as
/// the proxy it stands for, and sent from any thread.
#[derive(Clone)]
pub(crate) struct LoopProxy {
    send: Arc<dyn Fn(AppEvent) -> Result<(), EventLoopClosed<AppEvent>> + Send + Sync>,
}

impl LoopProxy {
    /// A wake that hands each event to `send` — the loop's own proxy, or a test's ledger.
    pub(crate) fn new(
        send: impl Fn(AppEvent) -> Result<(), EventLoopClosed<AppEvent>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            send: Arc::new(send),
        }
    }

    /// `EventLoopProxy::send_event`, word for word: the event, or the loop's refusal when it
    /// has already ended.
    pub(crate) fn send_event(&self, event: AppEvent) -> Result<(), EventLoopClosed<AppEvent>> {
        (self.send)(event)
    }
}

impl From<EventLoopProxy<AppEvent>> for LoopProxy {
    fn from(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self::new(move |event| proxy.send_event(event))
    }
}

/// **What a file of runtime methods brings into scope**: the backend and every door's trait, so
/// a call on `self.window.renderer` or `self.window.window` names the method it always named.
pub(crate) mod prelude {
    pub(crate) use super::{
        Backend, CompositorDoor, DeviceLayer, FolderPickerDoor, FrameDoor, Glass, ImagePickerDoor,
        ImeCaretDoor, LoopDoor, MathMenuDoor, SavePickerDoor, TaskbarDoor, WindowHost,
    };
}

/// **One implementation of every door a window has** — what `App`, `WindowRuntime`,
/// `Runtime` and `FolioApp` are written over.
///
/// The product's is [`Native`]; the test suite's is `headless::Headless`.
pub(crate) trait Backend: Sized + 'static {
    /// The window itself: its id, its size, its title, its visibility, its cursor.
    type Window: WindowHost;
    /// The process's device layer: the font environment every window measures against.
    type Device: DeviceLayer;
    /// One window's half of the renderer: its metrics, its measuring, its frame's inputs, its
    /// present.
    type Glass: Glass<Device = Self::Device>;
    /// The composition tree a page's visual hangs in beside the window's own surface.
    type Compositor: CompositorDoor;
    /// The window's custom frame: its title bar, its band, its size-move state.
    type Frame: FrameDoor;
    /// The formula's native context menu.
    type MathMenu: MathMenuDoor;
    /// The native folder chooser.
    type FolderPicker: FolderPickerDoor;
    /// The native file chooser.
    type ImagePicker: ImagePickerDoor;
    /// The native save dialog.
    type SavePicker: SavePickerDoor;
    /// The system caret an input method reads its candidate window's place from.
    type ImeCaret: ImeCaretDoor;
    /// The loop a turn runs in: how the application stops it and paces it.
    type Loop: LoopDoor;

    /// **Every window's surface, built again on the device layer** — what a device the driver
    /// took away, and the debug build's surface self-test, ask of every window at once.
    fn rebuild_every_surface(
        gpu: &mut Self::Device,
        windows: Vec<&mut crate::WindowRuntime<Self>>,
    ) -> Result<(), RenderError>;

    /// **A second window on this application**, opened on the loop — `Runtime::open_window`
    /// for the product.
    fn open_window(
        event_loop: &Self::Loop,
        app: &mut crate::App<Self>,
        plan: &crate::NewWindowPlan,
        like: Option<(
            bt_persist::SessionTabLayoutV1,
            bt_persist::SessionSidebarModeV1,
        )>,
    ) -> anyhow::Result<(WindowId, crate::WindowRuntime<Self>)>;
}

/// **The product's backend**: winit's window, `bt-render`'s renderer over the shared device,
/// `bt-platform`'s handles.
pub(crate) struct Native;

impl Backend for Native {
    type Window = Arc<Window>;
    type Device = bt_render::GpuContext;
    type Glass = bt_render::WindowRenderer;
    type Compositor = bt_platform::Compositor;
    type Frame = bt_platform::CustomWindowFrame;
    type MathMenu = bt_platform::MathContextMenu;
    type FolderPicker = bt_platform::FolderPicker;
    type ImagePicker = bt_platform::ImagePicker;
    type SavePicker = bt_platform::SaveFilePicker;
    type ImeCaret = bt_platform::ImeSystemCaret;
    type Loop = ActiveEventLoop;

    fn rebuild_every_surface(
        gpu: &mut Self::Device,
        windows: Vec<&mut crate::WindowRuntime<Self>>,
    ) -> Result<(), RenderError> {
        crate::rebuild_every_native_surface(gpu, windows)
    }

    fn open_window(
        event_loop: &ActiveEventLoop,
        app: &mut crate::App<Self>,
        plan: &crate::NewWindowPlan,
        like: Option<(
            bt_persist::SessionTabLayoutV1,
            bt_persist::SessionSidebarModeV1,
        )>,
    ) -> anyhow::Result<(WindowId, crate::WindowRuntime<Self>)> {
        crate::Runtime::open_window(event_loop, app, plan, like)
    }
}

/// **What a turn does to the loop it runs in** — `ActiveEventLoop`'s methods, name for name.
pub(crate) trait LoopDoor {
    fn exit(&self);
    fn set_control_flow(&self, control_flow: ControlFlow);
}

impl LoopDoor for ActiveEventLoop {
    fn exit(&self) {
        ActiveEventLoop::exit(self);
    }
    fn set_control_flow(&self, control_flow: ControlFlow) {
        ActiveEventLoop::set_control_flow(self, control_flow);
    }
}

/// **What a runtime does to its window** — winit's `Window` methods, name for name, and every
/// question and instruction the runtime puts to the system about it (its place on the desktop, its
/// dress, the keyboard and the foreground, its taskbar button, the system's news, a page's place in
/// it). The window's platform handle is the native backend's own: no door hands it out.
pub(crate) trait WindowHost {
    fn id(&self) -> WindowId;
    fn request_redraw(&self);
    fn inner_size(&self) -> PhysicalSize<u32>;
    /// Where the client area's top-left corner stands on the screen, in physical pixels —
    /// `Window::inner_position`, or `None` where the platform will not say.
    fn client_origin(&self) -> Option<(i32, i32)>;
    fn scale_factor(&self) -> f64;
    fn is_visible(&self) -> Option<bool>;
    fn is_maximized(&self) -> bool;
    fn is_minimized(&self) -> Option<bool>;
    fn has_focus(&self) -> bool;
    fn set_minimized(&self, minimized: bool);
    fn set_maximized(&self, maximized: bool);
    fn set_title(&self, title: &str);
    fn set_visible(&self, visible: bool);
    fn focus_window(&self);
    fn set_cursor(&self, cursor: Cursor);
    fn set_ime_cursor_area(&self, position: Position, size: Size);
    /// The size of every display the platform reports, in physical pixels.
    fn monitor_sizes(&self) -> Vec<PhysicalSize<u32>>;
    /// What the display this window is on says one of its frames is worth, in millihertz, or
    /// `None` when the platform will not say.
    fn frame_rate_millihertz(&self) -> Option<u32>;
    /// **Whether the system has this window iconic** — `bt_platform::is_window_minimized` of its
    /// own handle; `false` where there is no handle to ask about.
    fn is_iconic(&self) -> bool;
    /// **Whether the window manager has cloaked this window** (another virtual desktop, a
    /// suspended app) — `bt_platform::is_window_cloaked`; `false` without a handle.
    fn is_cloaked(&self) -> bool;
    /// **Whether the window manager puts this window under any of its own sample points** —
    /// `bt_platform::window_is_exposed`; `true` without a handle, the direction that function
    /// takes.
    fn is_exposed(&self) -> bool;
    /// The window's outer rectangle as the platform reports it — `bt_platform::get_window_rect`;
    /// `None` where it will not say.
    fn outer_rect(&self) -> Option<bt_platform::WindowRect>;
    /// The work area of the display this window is on — `bt_platform::get_work_area`; `None`
    /// where the platform will not say.
    fn work_area(&self) -> Option<bt_platform::WindowRect>;
    /// **The window stood at an outer rectangle on whichever display it names** —
    /// `bt_platform::stand_window_at`, which says it again until the display's scale has settled.
    fn stand_at(&self, rect: bt_platform::WindowRect) -> Result<(), String>;
    /// **Who owns the glass at a point on the screen**, physical pixels: this window, another
    /// one, or nobody the platform will name — `bt_platform::top_level_window_at` asked about
    /// this window's own handle.
    fn glass_at(&self, x: i32, y: i32) -> crate::GlassHere;
    /// Where the pointer is now, in this window's physical pixels —
    /// `bt_platform::pointer_position_in_window`; `None` where the platform will not say.
    fn pointer_in_window(&self) -> Option<(i32, i32)>;
    /// The platform's present facts about this window, for a diagnostic line —
    /// `bt_platform::native_present_facts`; every field unread where there is no handle.
    fn present_facts(&self) -> bt_platform::NativePresentFacts;
    /// **Whether a remembered foreground holder is this window** —
    /// `bt_platform::hotkey::Foreground::is_window`; `false` without a handle.
    fn holds_the_foreground(&self, holder: bt_platform::hotkey::Foreground) -> bool;
    /// The system's own blur behind the window's ground — `bt_platform::set_system_backdrop`.
    fn set_backdrop(&self, acrylic: bool) -> Result<(), String>;
    /// Whether the window stays above other windows — `bt_platform::set_window_topmost`.
    fn set_topmost(&self, topmost: bool) -> Result<(), String>;
    /// The frame's dark or light plate — `bt_platform::set_window_dark_mode`.
    fn set_dark_mode(&self, dark: bool) -> Result<(), String>;
    /// The colour the system paints the window with before its first frame, or none —
    /// `bt_platform::install_window_class_background`.
    fn set_class_background(&self, rgb: Option<[u8; 3]>) -> Result<(), String>;
    /// **The keyboard and the foreground, given to this window** —
    /// `bt_platform::hotkey::give_foreground_to`; whether the system let it take them.
    fn take_foreground(&self) -> bool;
    /// The keyboard focus taken back into the window from whatever child holds it (a page) —
    /// `bt_platform::take_keyboard_focus`.
    fn take_keyboard_focus(&self) -> Result<(), String>;
    /// The window asked to close, as its own close button asks — `bt_platform::request_window_close`.
    fn request_close(&self) -> Result<(), String>;
    /// The taskbar button flashed for attention — `bt_platform::flash_window`.
    fn flash(&self);
    /// The taskbar button's flash stopped — `bt_platform::stop_flashing_window`.
    fn stop_flashing(&self);
    /// **The window's taskbar button**, as its progress is told to it.
    type Taskbar: TaskbarDoor;
    /// The window's taskbar button, built — `bt_platform::Taskbar::new` — or why there is none.
    fn taskbar(&self) -> Result<Self::Taskbar, String>;
    /// **The system's preference broadcasts, heard by this window** —
    /// `bt_platform::SystemSettingsWatch::install`; `None` where the window cannot install one.
    fn watch_system_settings(
        &self,
        wake: Box<dyn Fn(bt_platform::SystemNews)>,
    ) -> Option<bt_platform::SystemSettingsWatch>;
    /// **The system's question when the session ends, heard by this window** —
    /// `bt_platform::session_end::SessionEndWatch::install`; `None` where it cannot be.
    fn watch_session_end(
        &self,
        hear: Box<dyn Fn(bt_platform::session_end::SessionEnd)>,
        reason: Box<dyn Fn() -> String>,
    ) -> Option<bt_platform::session_end::SessionEndWatch>;
    /// **A page's place in this window, as the web engine is told it** — the page's visual and
    /// this window's handle, sealed in a [`crate::webhost::SeatAddress`] only the engine reads.
    fn page_address(
        &self,
        page: bt_platform::PageVisual,
    ) -> Result<crate::webhost::SeatAddress, String>;
    /// **The window's input-method facts, read now** (T-IME-BRACKET-FACTS):
    /// `bt_platform::ime_observation::snapshot` of the window's own handle, read-only — it never
    /// associates a context, focuses or notifies. Default facts (every field `None`) where the
    /// platform has none to read: off Windows, or a window with no native handle.
    fn ime_facts(&self) -> bt_platform::ime_observation::NativeFacts;
    /// macOS's Option key as Alt, or as the composing key the keyboard prints with.
    fn set_option_as_alt(&self, option_sends_alt: bool);
    /// The window's scale and rectangle as the platform reports them, beside winit's.
    fn dpi_snapshot(&self) -> anyhow::Result<crate::DpiSnapshot>;
    /// `bt_platform::clipboard_text`: the clipboard's text, read on this window's thread.
    fn clipboard_text(&self) -> Result<String, String>;
    /// `bt_platform::set_clipboard_text`: text put on the clipboard.
    fn set_clipboard_text(&self, text: &str) -> Result<(), String>;
    /// `bt_platform::clipboard_payload`: whatever the clipboard holds, as a paste reads it.
    fn clipboard_payload(&self) -> Result<bt_platform::ClipboardPayload, String>;
    /// **One hand-off to the system, put on the OS hand-off lane** for this window — or why the
    /// window could not be named to the lane's door.
    fn hand_off(
        &self,
        lane: &crate::handoff_lane::HandoffLane,
        handoff: bt_platform::Handoff,
    ) -> Result<crate::handoff_lane::HandoffRequest, crate::handoff_lane::Declined>;
}

impl WindowHost for Arc<Window> {
    fn id(&self) -> WindowId {
        Window::id(self)
    }
    fn request_redraw(&self) {
        Window::request_redraw(self);
    }
    fn inner_size(&self) -> PhysicalSize<u32> {
        Window::inner_size(self)
    }
    fn client_origin(&self) -> Option<(i32, i32)> {
        Window::inner_position(self)
            .ok()
            .map(|origin| (origin.x, origin.y))
    }
    fn scale_factor(&self) -> f64 {
        Window::scale_factor(self)
    }
    fn is_visible(&self) -> Option<bool> {
        Window::is_visible(self)
    }
    fn is_maximized(&self) -> bool {
        Window::is_maximized(self)
    }
    fn is_minimized(&self) -> Option<bool> {
        Window::is_minimized(self)
    }
    fn has_focus(&self) -> bool {
        Window::has_focus(self)
    }
    fn set_minimized(&self, minimized: bool) {
        Window::set_minimized(self, minimized);
    }
    fn set_maximized(&self, maximized: bool) {
        Window::set_maximized(self, maximized);
    }
    fn set_title(&self, title: &str) {
        Window::set_title(self, title);
    }
    fn set_visible(&self, visible: bool) {
        Window::set_visible(self, visible);
    }
    fn focus_window(&self) {
        Window::focus_window(self);
    }
    fn set_cursor(&self, cursor: Cursor) {
        Window::set_cursor(self, cursor);
    }
    fn set_ime_cursor_area(&self, position: Position, size: Size) {
        Window::set_ime_cursor_area(self, position, size);
    }
    fn monitor_sizes(&self) -> Vec<PhysicalSize<u32>> {
        Window::available_monitors(self)
            .map(|monitor| monitor.size())
            .collect()
    }
    fn frame_rate_millihertz(&self) -> Option<u32> {
        crate::display_frame_rate_millihertz(self)
    }
    fn is_iconic(&self) -> bool {
        crate::native_window_of(self).is_ok_and(bt_platform::is_window_minimized)
    }
    fn is_cloaked(&self) -> bool {
        crate::native_window_of(self).is_ok_and(bt_platform::is_window_cloaked)
    }
    fn is_exposed(&self) -> bool {
        crate::native_window_of(self).map_or(true, bt_platform::window_is_exposed)
    }
    fn outer_rect(&self) -> Option<bt_platform::WindowRect> {
        crate::native_window_of(self)
            .ok()
            .and_then(|native| bt_platform::get_window_rect(native).ok())
    }
    fn work_area(&self) -> Option<bt_platform::WindowRect> {
        crate::native_window_of(self)
            .ok()
            .and_then(|native| bt_platform::get_work_area(native).ok())
    }
    fn stand_at(&self, rect: bt_platform::WindowRect) -> Result<(), String> {
        bt_platform::stand_window_at(native_or_say(self)?, rect)
    }
    fn glass_at(&self, x: i32, y: i32) -> crate::GlassHere {
        let Ok(mine) = crate::native_window_of(self) else {
            return crate::GlassHere::Unknown;
        };
        match bt_platform::top_level_window_at(x, y) {
            Some(under) if under == mine => crate::GlassHere::Ours,
            Some(_) => crate::GlassHere::Theirs,
            None => crate::GlassHere::Unknown,
        }
    }
    fn pointer_in_window(&self) -> Option<(i32, i32)> {
        crate::native_window_of(self)
            .ok()
            .and_then(crate::runtime::pointer::pointer_in_native_window)
    }
    fn present_facts(&self) -> bt_platform::NativePresentFacts {
        crate::native_window_of(self)
            .ok()
            .map(bt_platform::native_present_facts)
            .unwrap_or_default()
    }
    fn holds_the_foreground(&self, holder: bt_platform::hotkey::Foreground) -> bool {
        crate::native_window_of(self).is_ok_and(|native| holder.is_window(native))
    }
    fn set_backdrop(&self, acrylic: bool) -> Result<(), String> {
        bt_platform::set_system_backdrop(native_or_say(self)?, acrylic)
    }
    fn set_topmost(&self, topmost: bool) -> Result<(), String> {
        bt_platform::set_window_topmost(native_or_say(self)?, topmost)
    }
    fn set_dark_mode(&self, dark: bool) -> Result<(), String> {
        bt_platform::set_window_dark_mode(native_or_say(self)?, dark)
    }
    fn set_class_background(&self, rgb: Option<[u8; 3]>) -> Result<(), String> {
        bt_platform::install_window_class_background(native_or_say(self)?, rgb)
    }
    fn take_foreground(&self) -> bool {
        crate::native_window_of(self).is_ok_and(bt_platform::hotkey::give_foreground_to)
    }
    fn take_keyboard_focus(&self) -> Result<(), String> {
        bt_platform::take_keyboard_focus(native_or_say(self)?)
    }
    fn request_close(&self) -> Result<(), String> {
        bt_platform::request_window_close(native_or_say(self)?)
    }
    fn flash(&self) {
        if let Ok(native) = crate::native_window_of(self) {
            bt_platform::flash_window(native);
        }
    }
    fn stop_flashing(&self) {
        if let Ok(native) = crate::native_window_of(self) {
            bt_platform::stop_flashing_window(native);
        }
    }
    type Taskbar = bt_platform::Taskbar;
    fn taskbar(&self) -> Result<bt_platform::Taskbar, String> {
        bt_platform::Taskbar::new(native_or_say(self)?)
    }
    fn watch_system_settings(
        &self,
        wake: Box<dyn Fn(bt_platform::SystemNews)>,
    ) -> Option<bt_platform::SystemSettingsWatch> {
        let native = crate::native_window_of(self).ok()?;
        bt_platform::SystemSettingsWatch::install(native, wake).ok()
    }
    fn watch_session_end(
        &self,
        hear: Box<dyn Fn(bt_platform::session_end::SessionEnd)>,
        reason: Box<dyn Fn() -> String>,
    ) -> Option<bt_platform::session_end::SessionEndWatch> {
        let native = crate::native_window_of(self).ok()?;
        bt_platform::session_end::SessionEndWatch::install(native, hear, reason).ok()
    }
    fn page_address(
        &self,
        page: bt_platform::PageVisual,
    ) -> Result<crate::webhost::SeatAddress, String> {
        Ok(crate::webhost::SeatAddress::new(page, native_or_say(self)?))
    }
    fn ime_facts(&self) -> bt_platform::ime_observation::NativeFacts {
        crate::native_window_of(self)
            .map(bt_platform::ime_observation::snapshot)
            .unwrap_or_default()
    }
    fn set_option_as_alt(&self, option_sends_alt: bool) {
        crate::option_as_alt(self, option_sends_alt);
    }
    fn dpi_snapshot(&self) -> anyhow::Result<crate::DpiSnapshot> {
        crate::native_dpi_snapshot(self)
    }
    fn clipboard_text(&self) -> Result<String, String> {
        bt_platform::clipboard_text()
    }
    fn set_clipboard_text(&self, text: &str) -> Result<(), String> {
        bt_platform::set_clipboard_text(text)
    }
    fn clipboard_payload(&self) -> Result<bt_platform::ClipboardPayload, String> {
        bt_platform::clipboard_payload()
    }
    fn hand_off(
        &self,
        lane: &crate::handoff_lane::HandoffLane,
        handoff: bt_platform::Handoff,
    ) -> Result<crate::handoff_lane::HandoffRequest, crate::handoff_lane::Declined> {
        match crate::native_window_of(self) {
            Ok(native) => lane.submit(native, handoff),
            Err(error) => Err(crate::handoff_lane::Declined::NoWindow(format!(
                "{error:#}"
            ))),
        }
    }
}

/// winit's window's own handle, or why there is none in a door's own words.
fn native_or_say(window: &Window) -> Result<bt_platform::NativeWindow, String> {
    crate::native_window_of(window).map_err(|error| format!("{error:#}"))
}

/// **A window's taskbar button, as its progress is told to it** — `bt_platform::Taskbar`'s
/// method.
pub(crate) trait TaskbarDoor {
    fn set_progress(&self, progress: bt_platform::TaskbarProgress) -> Result<(), String>;
}

impl TaskbarDoor for bt_platform::Taskbar {
    fn set_progress(&self, progress: bt_platform::TaskbarProgress) -> Result<(), String> {
        bt_platform::Taskbar::set_progress(self, progress)
    }
}

/// **The process's device layer, as a window's runtime asks it** — `GpuContext`'s methods, name
/// for name.
pub(crate) trait DeviceLayer {
    fn font_environment_epoch(&self) -> u64;
    fn device_loss(&self) -> Option<&str>;
    fn terminal_font_size_logical_px(&self) -> f32;
    fn set_terminal_font(
        &mut self,
        family: &str,
        files: &[PathBuf],
        cjk_family: &str,
        cjk_files: &[PathBuf],
        size_logical_px: f32,
    );
}

impl DeviceLayer for bt_render::GpuContext {
    fn font_environment_epoch(&self) -> u64 {
        bt_render::GpuContext::font_environment_epoch(self)
    }
    fn device_loss(&self) -> Option<&str> {
        bt_render::GpuContext::device_loss(self)
    }
    fn terminal_font_size_logical_px(&self) -> f32 {
        bt_render::GpuContext::terminal_font_size_logical_px(self)
    }
    fn set_terminal_font(
        &mut self,
        family: &str,
        files: &[PathBuf],
        cjk_family: &str,
        cjk_files: &[PathBuf],
        size_logical_px: f32,
    ) {
        bt_render::GpuContext::set_terminal_font(
            self,
            family,
            files,
            cjk_family,
            cjk_files,
            size_logical_px,
        );
    }
}

/// **One window's half of the renderer, as its runtime asks it** — `WindowRenderer`'s methods,
/// name for name, each taking the device layer where the renderer's does.
pub(crate) trait Glass {
    /// The device layer this glass measures and presents against.
    type Device: DeviceLayer;

    fn scale_factor(&self) -> f64;
    fn dpi_milli(&self) -> NonZeroU32;
    fn base_metrics(&self) -> CellMetrics;
    fn presentation_geometry(&self) -> PresentationGeometry;
    fn presented_swapchain_size(&self) -> (u32, u32);
    fn seat_viewport(&self) -> SeatViewport;
    fn set_seat_viewport(&mut self, seat: SeatViewport) -> bool;
    fn font_revision(&self) -> u64;
    fn surface_generation(&self) -> u64;
    fn present_state(&self) -> RendererPresentState;
    fn present_configuration(&self, gpu: &Self::Device) -> PresentConfiguration;
    fn perf_frame(&self) -> u64;
    fn glyph_census(&self) -> Option<&GlyphCensus>;
    fn set_glyph_census(&mut self, enabled: bool);
    fn preview_text_frame(&self) -> PreviewTextFrame;
    fn video_layers(&self) -> &[VideoLayer];
    fn set_video_layers(&mut self, layers: Vec<VideoLayer>) -> bool;
    fn set_web_holes(&mut self, holes: Vec<WebHole>) -> bool;
    fn set_peek_overlay(&mut self, overlay: Option<PeekImageOverlay>) -> bool;
    fn set_preview_images(&mut self, images: Vec<PreviewImage>) -> bool;
    fn place_preview_image(&mut self, owner: u64, seat: SeatViewport, clip: SeatViewport) -> bool;
    fn set_preview_bodies(&mut self, bodies: Vec<PreviewBody>) -> bool;
    fn set_table_blocks(&mut self, blocks: HashMap<TableBlockKey, TableBlockPaint>) -> bool;
    fn set_modal_overlay(&mut self, layers: Vec<OverlayLayer>, groups: Vec<OverlayGroup>) -> bool;
    fn set_window_focused(&mut self, focused: bool) -> bool;
    fn set_cursor_blink_visible(&mut self, visible: bool) -> bool;
    fn set_chrome(
        &mut self,
        quads: Vec<ChromeQuad>,
        labels: Vec<ChromeLabel>,
        icons: Vec<ChromeIcon>,
    ) -> bool;
    fn math_hit_test(
        &self,
        metrics: CellMetrics,
        seat: SeatViewport,
        frame: &ViewportFrame,
        x: f64,
        y: f64,
    ) -> Option<MathHit>;
    fn math_tool_boxes(
        &self,
        metrics: CellMetrics,
        seat: SeatViewport,
        frame: &ViewportFrame,
        hovered: &MathBlockAnchor,
    ) -> Option<MathToolBoxes>;
    fn math_band_trace(
        &self,
        metrics: CellMetrics,
        seat: SeatViewport,
        frame: &ViewportFrame,
        named: &MathBlockAnchor,
    ) -> Option<MathBandTrace>;
    fn math_band_face(
        &self,
        metrics: CellMetrics,
        seat: SeatViewport,
        frame: &ViewportFrame,
        named: &MathBlockAnchor,
    ) -> Option<MathBandFace>;
    fn peek_thumbnail_extent(
        &self,
        seat: SeatViewport,
        image_width_px: u32,
        image_height_px: u32,
    ) -> Option<(u32, u32)>;
    fn ime_cursor_area(&self, metrics: CellMetrics, frame: &ViewportFrame) -> ImeCursorArea;
    fn resize(&mut self, gpu: &Self::Device, width: u32, height: u32) -> Result<(), RenderError>;
    fn update_scale_factor(
        &mut self,
        gpu: &mut Self::Device,
        scale_factor: f64,
    ) -> Result<CellMetrics, RenderError>;
    fn apply_font_change(&mut self, gpu: &mut Self::Device) -> Result<CellMetrics, RenderError>;
    fn pane_cell_metrics(
        &self,
        gpu: &mut Self::Device,
        font_size_logical_px: f32,
    ) -> Result<CellMetrics, RenderError>;
    fn present_frame_with_phases(
        &mut self,
        token: WaitToken<'_, doors::PresentFrame>,
        gpu: &mut Self::Device,
        seats: &[SeatFrame<'_>],
        trigger: FrameTrigger,
        phase: impl FnMut(PresentPhase),
    ) -> Result<PresentOutcome, RenderError>;
    fn measure_chrome_text(&mut self, gpu: &mut Self::Device, text: &str, font_size_px: f32)
    -> f32;
    fn measure_chrome_mono_text(
        &mut self,
        gpu: &mut Self::Device,
        text: &str,
        font_size_px: f32,
    ) -> f32;
    fn measure_chrome_label(
        &mut self,
        gpu: &mut Self::Device,
        text: &str,
        font_size_px: f32,
        weight: ChromeLabelWeight,
        letter_spacing_em: f32,
        tabular_numerals: bool,
    ) -> f32;
    fn chrome_text_advances(
        &mut self,
        gpu: &mut Self::Device,
        text: &str,
        font_size_px: f32,
    ) -> ChromeTextAdvances;
    fn chrome_label_advances(
        &mut self,
        gpu: &mut Self::Device,
        text: &str,
        font_size_px: f32,
        weight: ChromeLabelWeight,
        letter_spacing_em: f32,
        tabular_numerals: bool,
    ) -> ChromeTextAdvances;
    fn measure_preview_paragraph(
        &mut self,
        gpu: &mut Self::Device,
        runs: &[PreviewRun],
        width_px: f32,
        font_size_px: f32,
        line_height_px: f32,
    ) -> f32;
    fn measure_preview_paragraph_width(
        &mut self,
        gpu: &mut Self::Device,
        runs: &[PreviewRun],
        font_size_px: f32,
        line_height_px: f32,
    ) -> f32;
    fn measure_preview_run_boxes(
        &mut self,
        gpu: &mut Self::Device,
        paragraph: &PreviewParagraph,
    ) -> Vec<PreviewRunBox>;
    fn measure_preview_hit(
        &mut self,
        gpu: &mut Self::Device,
        paragraph: &PreviewParagraph,
        x: f32,
        y: f32,
    ) -> usize;
    fn measure_preview_rows(
        &mut self,
        gpu: &mut Self::Device,
        paragraph: &PreviewParagraph,
    ) -> Vec<PreviewTextRow>;
    fn measure_preview_highlight(
        &mut self,
        gpu: &mut Self::Device,
        paragraph: &PreviewParagraph,
        range: Range<usize>,
    ) -> Vec<[f32; 4]>;
    fn preview_mono_advance(&mut self, gpu: &mut Self::Device, font_size_px: f32) -> f32;
}

impl Glass for bt_render::WindowRenderer {
    type Device = bt_render::GpuContext;

    fn scale_factor(&self) -> f64 {
        bt_render::WindowRenderer::scale_factor(self)
    }
    fn dpi_milli(&self) -> NonZeroU32 {
        bt_render::WindowRenderer::dpi_milli(self)
    }
    fn base_metrics(&self) -> CellMetrics {
        bt_render::WindowRenderer::base_metrics(self)
    }
    fn presentation_geometry(&self) -> PresentationGeometry {
        bt_render::WindowRenderer::presentation_geometry(self)
    }
    fn presented_swapchain_size(&self) -> (u32, u32) {
        bt_render::WindowRenderer::presented_swapchain_size(self)
    }
    fn seat_viewport(&self) -> SeatViewport {
        bt_render::WindowRenderer::seat_viewport(self)
    }
    fn set_seat_viewport(&mut self, seat: SeatViewport) -> bool {
        bt_render::WindowRenderer::set_seat_viewport(self, seat)
    }
    fn font_revision(&self) -> u64 {
        bt_render::WindowRenderer::font_revision(self)
    }
    fn surface_generation(&self) -> u64 {
        bt_render::WindowRenderer::surface_generation(self)
    }
    fn present_state(&self) -> RendererPresentState {
        bt_render::WindowRenderer::present_state(self)
    }
    fn present_configuration(&self, gpu: &bt_render::GpuContext) -> PresentConfiguration {
        bt_render::WindowRenderer::present_configuration(self, gpu)
    }
    fn perf_frame(&self) -> u64 {
        bt_render::WindowRenderer::perf_frame(self)
    }
    fn glyph_census(&self) -> Option<&GlyphCensus> {
        bt_render::WindowRenderer::glyph_census(self)
    }
    fn set_glyph_census(&mut self, enabled: bool) {
        bt_render::WindowRenderer::set_glyph_census(self, enabled);
    }
    fn preview_text_frame(&self) -> PreviewTextFrame {
        bt_render::WindowRenderer::preview_text_frame(self)
    }
    fn video_layers(&self) -> &[VideoLayer] {
        bt_render::WindowRenderer::video_layers(self)
    }
    fn set_video_layers(&mut self, layers: Vec<VideoLayer>) -> bool {
        bt_render::WindowRenderer::set_video_layers(self, layers)
    }
    fn set_web_holes(&mut self, holes: Vec<WebHole>) -> bool {
        bt_render::WindowRenderer::set_web_holes(self, holes)
    }
    fn set_peek_overlay(&mut self, overlay: Option<PeekImageOverlay>) -> bool {
        bt_render::WindowRenderer::set_peek_overlay(self, overlay)
    }
    fn set_preview_images(&mut self, images: Vec<PreviewImage>) -> bool {
        bt_render::WindowRenderer::set_preview_images(self, images)
    }
    fn place_preview_image(&mut self, owner: u64, seat: SeatViewport, clip: SeatViewport) -> bool {
        bt_render::WindowRenderer::place_preview_image(self, owner, seat, clip)
    }
    fn set_preview_bodies(&mut self, bodies: Vec<PreviewBody>) -> bool {
        bt_render::WindowRenderer::set_preview_bodies(self, bodies)
    }
    fn set_table_blocks(&mut self, blocks: HashMap<TableBlockKey, TableBlockPaint>) -> bool {
        bt_render::WindowRenderer::set_table_blocks(self, blocks)
    }
    fn set_modal_overlay(&mut self, layers: Vec<OverlayLayer>, groups: Vec<OverlayGroup>) -> bool {
        bt_render::WindowRenderer::set_modal_overlay(self, layers, groups)
    }
    fn set_window_focused(&mut self, focused: bool) -> bool {
        bt_render::WindowRenderer::set_window_focused(self, focused)
    }
    fn set_cursor_blink_visible(&mut self, visible: bool) -> bool {
        bt_render::WindowRenderer::set_cursor_blink_visible(self, visible)
    }
    fn set_chrome(
        &mut self,
        quads: Vec<ChromeQuad>,
        labels: Vec<ChromeLabel>,
        icons: Vec<ChromeIcon>,
    ) -> bool {
        bt_render::WindowRenderer::set_chrome(self, quads, labels, icons)
    }
    fn math_hit_test(
        &self,
        metrics: CellMetrics,
        seat: SeatViewport,
        frame: &ViewportFrame,
        x: f64,
        y: f64,
    ) -> Option<MathHit> {
        bt_render::WindowRenderer::math_hit_test(self, metrics, seat, frame, x, y)
    }
    fn math_tool_boxes(
        &self,
        metrics: CellMetrics,
        seat: SeatViewport,
        frame: &ViewportFrame,
        hovered: &MathBlockAnchor,
    ) -> Option<MathToolBoxes> {
        bt_render::WindowRenderer::math_tool_boxes(self, metrics, seat, frame, hovered)
    }
    fn math_band_trace(
        &self,
        metrics: CellMetrics,
        seat: SeatViewport,
        frame: &ViewportFrame,
        named: &MathBlockAnchor,
    ) -> Option<MathBandTrace> {
        bt_render::WindowRenderer::math_band_trace(self, metrics, seat, frame, named)
    }
    fn math_band_face(
        &self,
        metrics: CellMetrics,
        seat: SeatViewport,
        frame: &ViewportFrame,
        named: &MathBlockAnchor,
    ) -> Option<MathBandFace> {
        bt_render::WindowRenderer::math_band_face(self, metrics, seat, frame, named)
    }
    fn peek_thumbnail_extent(
        &self,
        seat: SeatViewport,
        image_width_px: u32,
        image_height_px: u32,
    ) -> Option<(u32, u32)> {
        bt_render::WindowRenderer::peek_thumbnail_extent(
            self,
            seat,
            image_width_px,
            image_height_px,
        )
    }
    fn ime_cursor_area(&self, metrics: CellMetrics, frame: &ViewportFrame) -> ImeCursorArea {
        bt_render::WindowRenderer::ime_cursor_area(self, metrics, frame)
    }
    fn resize(
        &mut self,
        gpu: &bt_render::GpuContext,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        bt_render::WindowRenderer::resize(self, gpu, width, height)
    }
    fn update_scale_factor(
        &mut self,
        gpu: &mut bt_render::GpuContext,
        scale_factor: f64,
    ) -> Result<CellMetrics, RenderError> {
        bt_render::WindowRenderer::update_scale_factor(self, gpu, scale_factor)
    }
    fn apply_font_change(
        &mut self,
        gpu: &mut bt_render::GpuContext,
    ) -> Result<CellMetrics, RenderError> {
        bt_render::WindowRenderer::apply_font_change(self, gpu)
    }
    fn pane_cell_metrics(
        &self,
        gpu: &mut bt_render::GpuContext,
        font_size_logical_px: f32,
    ) -> Result<CellMetrics, RenderError> {
        bt_render::WindowRenderer::pane_cell_metrics(self, gpu, font_size_logical_px)
    }
    fn present_frame_with_phases(
        &mut self,
        token: WaitToken<'_, doors::PresentFrame>,
        gpu: &mut bt_render::GpuContext,
        seats: &[SeatFrame<'_>],
        trigger: FrameTrigger,
        phase: impl FnMut(PresentPhase),
    ) -> Result<PresentOutcome, RenderError> {
        bt_render::WindowRenderer::present_frame_with_phases(
            self, token, gpu, seats, trigger, phase,
        )
    }
    fn measure_chrome_text(
        &mut self,
        gpu: &mut bt_render::GpuContext,
        text: &str,
        font_size_px: f32,
    ) -> f32 {
        bt_render::WindowRenderer::measure_chrome_text(self, gpu, text, font_size_px)
    }
    fn measure_chrome_mono_text(
        &mut self,
        gpu: &mut bt_render::GpuContext,
        text: &str,
        font_size_px: f32,
    ) -> f32 {
        bt_render::WindowRenderer::measure_chrome_mono_text(self, gpu, text, font_size_px)
    }
    fn measure_chrome_label(
        &mut self,
        gpu: &mut bt_render::GpuContext,
        text: &str,
        font_size_px: f32,
        weight: ChromeLabelWeight,
        letter_spacing_em: f32,
        tabular_numerals: bool,
    ) -> f32 {
        bt_render::WindowRenderer::measure_chrome_label(
            self,
            gpu,
            text,
            font_size_px,
            weight,
            letter_spacing_em,
            tabular_numerals,
        )
    }
    fn chrome_text_advances(
        &mut self,
        gpu: &mut bt_render::GpuContext,
        text: &str,
        font_size_px: f32,
    ) -> ChromeTextAdvances {
        bt_render::WindowRenderer::chrome_text_advances(self, gpu, text, font_size_px)
    }
    fn chrome_label_advances(
        &mut self,
        gpu: &mut bt_render::GpuContext,
        text: &str,
        font_size_px: f32,
        weight: ChromeLabelWeight,
        letter_spacing_em: f32,
        tabular_numerals: bool,
    ) -> ChromeTextAdvances {
        bt_render::WindowRenderer::chrome_label_advances(
            self,
            gpu,
            text,
            font_size_px,
            weight,
            letter_spacing_em,
            tabular_numerals,
        )
    }
    fn measure_preview_paragraph(
        &mut self,
        gpu: &mut bt_render::GpuContext,
        runs: &[PreviewRun],
        width_px: f32,
        font_size_px: f32,
        line_height_px: f32,
    ) -> f32 {
        bt_render::WindowRenderer::measure_preview_paragraph(
            self,
            gpu,
            runs,
            width_px,
            font_size_px,
            line_height_px,
        )
    }
    fn measure_preview_paragraph_width(
        &mut self,
        gpu: &mut bt_render::GpuContext,
        runs: &[PreviewRun],
        font_size_px: f32,
        line_height_px: f32,
    ) -> f32 {
        bt_render::WindowRenderer::measure_preview_paragraph_width(
            self,
            gpu,
            runs,
            font_size_px,
            line_height_px,
        )
    }
    fn measure_preview_run_boxes(
        &mut self,
        gpu: &mut bt_render::GpuContext,
        paragraph: &PreviewParagraph,
    ) -> Vec<PreviewRunBox> {
        bt_render::WindowRenderer::measure_preview_run_boxes(self, gpu, paragraph)
    }
    fn measure_preview_hit(
        &mut self,
        gpu: &mut bt_render::GpuContext,
        paragraph: &PreviewParagraph,
        x: f32,
        y: f32,
    ) -> usize {
        bt_render::WindowRenderer::measure_preview_hit(self, gpu, paragraph, x, y)
    }
    fn measure_preview_rows(
        &mut self,
        gpu: &mut bt_render::GpuContext,
        paragraph: &PreviewParagraph,
    ) -> Vec<PreviewTextRow> {
        bt_render::WindowRenderer::measure_preview_rows(self, gpu, paragraph)
    }
    fn measure_preview_highlight(
        &mut self,
        gpu: &mut bt_render::GpuContext,
        paragraph: &PreviewParagraph,
        range: Range<usize>,
    ) -> Vec<[f32; 4]> {
        bt_render::WindowRenderer::measure_preview_highlight(self, gpu, paragraph, range)
    }
    fn preview_mono_advance(&mut self, gpu: &mut bt_render::GpuContext, font_size_px: f32) -> f32 {
        bt_render::WindowRenderer::preview_mono_advance(self, gpu, font_size_px)
    }
}

/// **The composition tree beside the window's surface** — `bt_platform::Compositor`'s methods
/// that a window's pages and its frame reach, name for name.
pub(crate) trait CompositorDoor {
    fn set_window_size(
        &self,
        token: WaitToken<'_, doors::CompositorWindowSize>,
        width: u32,
        height: u32,
    ) -> Result<(), String>;
    fn skirt_covers_anything(&self) -> bool;
    fn set_covered_size(&self, width: u32, height: u32) -> Result<(), String>;
    fn set_page_ground_color(&self, premultiplied_srgb: [f32; 4]) -> Result<(), String>;
    fn attach_web_visual(&self, page: bt_platform::PageVisual) -> Result<(), String>;
    fn detach_web_visual(&self, page: bt_platform::PageVisual) -> Result<(), String>;
    fn hide_web_visual(&self, page: bt_platform::PageVisual) -> Result<(), String>;
    fn place_web_visual(
        &self,
        page: bt_platform::PageVisual,
        offset: (i32, i32),
        clip: (f32, f32, f32, f32),
    ) -> Result<(), String>;
    fn set_page_cover(
        &self,
        page: bt_platform::PageVisual,
        rects: &[[f32; 4]],
    ) -> Result<(), String>;
    fn commit(&self, token: WaitToken<'_, doors::CompositorCommit>) -> Result<(), String>;
    /// **This tree as a page's engine hangs a page in it**, sealed ([`PageTree`]) — or why this
    /// tree is not one the engine could hang a page in.
    fn page_tree(&self) -> Result<PageTree<'_>, String>;
}

/// **The platform's composition tree, as a page's engine hangs a page in it** — sealed: the two
/// walks below are the only roads from it to the platform, and nothing outside the seam can open
/// it.
pub(crate) struct PageTree<'a>(&'a bt_platform::Compositor);

impl PageTree<'_> {
    /// `WebHost::install`: the page's controller told to render into its visual in this tree,
    /// with every event and policy of `generation` installed.
    pub(crate) fn install(
        &self,
        host: &mut bt_platform::WebHost,
        page: bt_platform::PageVisual,
        generation: u64,
    ) -> Result<bt_platform::WebInstallReport, String> {
        host.install(self.0, page, generation)
    }

    /// `WebHost::rehost`: the page's controller walked from its place in this tree (`from`, the
    /// page and the window it stands in) to its place in `to`'s.
    #[expect(
        clippy::too_many_arguments,
        reason = "permanent: the platform's walk takes both sides, the rectangle and the visibility"
    )]
    pub(crate) fn rehost(
        &self,
        to: &PageTree<'_>,
        token: WaitToken<'_, doors::WebRehost>,
        host: &mut bt_platform::WebHost,
        from: (bt_platform::PageVisual, bt_platform::NativeWindow),
        into: (bt_platform::PageVisual, bt_platform::NativeWindow),
        rect: (i32, i32, u32, u32),
        visible: bool,
    ) -> bt_platform::RehostOutcome {
        host.rehost(
            token,
            &bt_platform::RehostSide {
                compositor: self.0,
                page: from.0,
                window: from.1,
            },
            &bt_platform::RehostSide {
                compositor: to.0,
                page: into.0,
                window: into.1,
            },
            rect,
            visible,
        )
    }
}

impl CompositorDoor for bt_platform::Compositor {
    fn set_window_size(
        &self,
        token: WaitToken<'_, doors::CompositorWindowSize>,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        bt_platform::Compositor::set_window_size(self, token, width, height)
    }
    fn skirt_covers_anything(&self) -> bool {
        bt_platform::Compositor::skirt_covers_anything(self)
    }
    fn set_covered_size(&self, width: u32, height: u32) -> Result<(), String> {
        bt_platform::Compositor::set_covered_size(self, width, height)
    }
    fn set_page_ground_color(&self, premultiplied_srgb: [f32; 4]) -> Result<(), String> {
        bt_platform::Compositor::set_page_ground_color(self, premultiplied_srgb)
    }
    fn attach_web_visual(&self, page: bt_platform::PageVisual) -> Result<(), String> {
        bt_platform::Compositor::attach_web_visual(self, page)
    }
    fn detach_web_visual(&self, page: bt_platform::PageVisual) -> Result<(), String> {
        bt_platform::Compositor::detach_web_visual(self, page)
    }
    fn hide_web_visual(&self, page: bt_platform::PageVisual) -> Result<(), String> {
        bt_platform::Compositor::hide_web_visual(self, page)
    }
    fn place_web_visual(
        &self,
        page: bt_platform::PageVisual,
        offset: (i32, i32),
        clip: (f32, f32, f32, f32),
    ) -> Result<(), String> {
        bt_platform::Compositor::place_web_visual(self, page, offset, clip)
    }
    fn set_page_cover(
        &self,
        page: bt_platform::PageVisual,
        rects: &[[f32; 4]],
    ) -> Result<(), String> {
        bt_platform::Compositor::set_page_cover(self, page, rects)
    }
    fn commit(&self, token: WaitToken<'_, doors::CompositorCommit>) -> Result<(), String> {
        bt_platform::Compositor::commit(self, token)
    }
    fn page_tree(&self) -> Result<PageTree<'_>, String> {
        Ok(PageTree(self))
    }
}

/// **The window's own frame** — `bt_platform::CustomWindowFrame`'s methods, name for name.
pub(crate) trait FrameDoor {
    fn in_size_move(&self) -> bool;
    fn platform_chrome(&self) -> bt_platform::PlatformChrome;
    fn set_window_band(&self, band_logical_px: f32) -> Result<(), String>;
    fn set_title(&self, title: &str) -> bool;
    fn press_title_bar(&self) -> Result<(), String>;
    fn set_title_bar_boxes(&self, boxes: &[[i32; 4]]);
    fn set_min_client_size(&self, logical: Option<(u32, u32)>) -> Result<(), String>;
}

impl FrameDoor for bt_platform::CustomWindowFrame {
    fn in_size_move(&self) -> bool {
        bt_platform::CustomWindowFrame::in_size_move(self)
    }
    fn platform_chrome(&self) -> bt_platform::PlatformChrome {
        bt_platform::CustomWindowFrame::platform_chrome(self)
    }
    fn set_window_band(&self, band_logical_px: f32) -> Result<(), String> {
        bt_platform::CustomWindowFrame::set_window_band(self, band_logical_px)
    }
    fn set_title(&self, title: &str) -> bool {
        bt_platform::CustomWindowFrame::set_title(self, title)
    }
    fn press_title_bar(&self) -> Result<(), String> {
        bt_platform::CustomWindowFrame::press_title_bar(self)
    }
    fn set_title_bar_boxes(&self, boxes: &[[i32; 4]]) {
        bt_platform::CustomWindowFrame::set_title_bar_boxes(self, boxes);
    }
    fn set_min_client_size(&self, logical: Option<(u32, u32)>) -> Result<(), String> {
        bt_platform::CustomWindowFrame::set_min_client_size(self, logical)
    }
}

/// **The formula's native context menu** — `bt_platform::MathContextMenu`'s methods.
pub(crate) trait MathMenuDoor {
    fn request(&self) -> Result<bool, String>;
    fn take_result(&self) -> Option<Result<bool, String>>;
}

impl MathMenuDoor for bt_platform::MathContextMenu {
    fn request(&self) -> Result<bool, String> {
        bt_platform::MathContextMenu::request(self)
    }
    fn take_result(&self) -> Option<Result<bool, String>> {
        bt_platform::MathContextMenu::take_result(self)
    }
}

/// **The native folder chooser** — `bt_platform::FolderPicker`'s methods.
pub(crate) trait FolderPickerDoor {
    fn request(&self, start: Option<&Path>) -> Result<bool, String>;
    fn take_result(&self) -> Option<Result<Option<PathBuf>, String>>;
}

impl FolderPickerDoor for bt_platform::FolderPicker {
    fn request(&self, start: Option<&Path>) -> Result<bool, String> {
        bt_platform::FolderPicker::request(self, start)
    }
    fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
        bt_platform::FolderPicker::take_result(self)
    }
}

/// **The native file chooser** — `bt_platform::ImagePicker`'s methods.
pub(crate) trait ImagePickerDoor {
    fn request(
        &self,
        kind: bt_platform::FilePickKind,
        start: Option<&Path>,
    ) -> Result<bool, String>;
    fn take_result(&self) -> Option<Result<Option<PathBuf>, String>>;
}

impl ImagePickerDoor for bt_platform::ImagePicker {
    fn request(
        &self,
        kind: bt_platform::FilePickKind,
        start: Option<&Path>,
    ) -> Result<bool, String> {
        bt_platform::ImagePicker::request(self, kind, start)
    }
    fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
        bt_platform::ImagePicker::take_result(self)
    }
}

/// **The native save dialog** — `bt_platform::SaveFilePicker`'s methods.
pub(crate) trait SavePickerDoor {
    fn request(&self, start: Option<&Path>, name: &str) -> Result<bool, String>;
    fn take_result(&self) -> Option<Result<Option<PathBuf>, String>>;
}

impl SavePickerDoor for bt_platform::SaveFilePicker {
    fn request(&self, start: Option<&Path>, name: &str) -> Result<bool, String> {
        bt_platform::SaveFilePicker::request(self, start, name)
    }
    fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
        bt_platform::SaveFilePicker::take_result(self)
    }
}

/// **The system caret an input method places its candidates by** —
/// `bt_platform::ImeSystemCaret`'s methods.
pub(crate) trait ImeCaretDoor {
    fn update(&mut self, x: i32, y: i32) -> Result<(), String>;
    fn destroy(&mut self);
}

impl ImeCaretDoor for bt_platform::ImeSystemCaret {
    fn update(&mut self, x: i32, y: i32) -> Result<(), String> {
        bt_platform::ImeSystemCaret::update(self, x, y)
    }
    fn destroy(&mut self) {
        bt_platform::ImeSystemCaret::destroy(self);
    }
}
