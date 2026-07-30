use std::{
    backtrace::Backtrace,
    fs::OpenOptions,
    io::Write,
    num::{NonZeroI64, NonZeroU32, NonZeroUsize},
    panic,
    path::PathBuf,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

mod input;

use anyhow::{Context, Result, anyhow, ensure};
use bt_doc::{Bias, LayoutKey};
use bt_math::{MathEngine, MathRaster, MathRenderError};
use bt_pty::{OutputWake, PtySession, PtySize};
use bt_render::{
    FrameSource, FrameTrigger, GridSize, ImeCursorArea, LatestFrameSlot, MathHit, MathHitTarget,
    Preedit, PresentOutcome, Renderer, background_rgb, compose_preedit, foreground_rgb,
    frame_content_digest, frame_is_alternate_screen, theme_revision,
};
use bt_term::{
    DualPlaneSession, InlineImageDecoder, MathLayoutOptions, MouseTracking, SessionDecorationTask,
    SessionMathTask, TerminalModes, render_detection_task, render_live_detection_task,
};
use bt_transcript::DEFAULT_STAGING_QUOTA;
use bt_viewport::{
    HyperlinkHit, MathBlockAnchor, ViewSelection, ViewportFrame, ViewportProjection,
};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::{Key, ModifiersState, NamedKey},
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::{Window, WindowId},
};

const INITIAL_WIDTH: f64 = 960.0;
const INITIAL_HEIGHT: f64 = 600.0;
const WINDOW_TITLE: &str = "BetterTerminal M0-beta";
const WIN32_DEFAULT_DPI: f64 = 96.0;
const STARTUP_PTY_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);
/// One 60 Hz frame: coalesce cursor-area churn without leaving the final position unsent.
const IME_CURSOR_AREA_INTERVAL: Duration = Duration::from_millis(16);
/// Winit 0.30 has no enter/exit-size-move event; the final ConPTY size is committed after this
/// silence interval while the local surface and terminal grid continue to follow every event.
const WINDOW_RESIZE_QUIET: Duration = bt_term::RESIZE_REQUEST_QUIET;
/// M0-alpha's single-session frozen-line budget; later configuration work may expose it.
const M0_FROZEN_LINE_QUOTA: NonZeroUsize = NonZeroUsize::new(100_000).unwrap();
const MULTI_CLICK_INTERVAL: Duration = Duration::from_millis(500);
const HYPERLINK_HOVER_DELAY: Duration = Duration::from_millis(400);
const MATH_WORKER_STOPPED_NOTICE: &str =
    "Formula rendering stopped; terminal input and output remain available";
const PANIC_LOG_FILENAME: &str = "bt-app-panic.log";

#[derive(Clone, Copy, Debug)]
enum AppEvent {
    PtyOutput,
    MathReady,
}

struct MathWorkerResult {
    completion: DecorationWorkerCompletion,
}

struct MathWorkerTask {
    task: SessionDecorationTask,
    foreground_rgb: [u8; 3],
}

enum DecorationWorkerCompletion {
    Math {
        task: Box<SessionMathTask>,
        result: std::result::Result<MathRaster, MathRenderError>,
    },
    InlineImage {
        task: bt_term::InlineImageTask,
        result: std::result::Result<bt_term::DecodedInlineImage, bt_term::InlineImageDecodeError>,
    },
}

struct MathWorker {
    tasks: mpsc::Sender<MathWorkerTask>,
    results: mpsc::Receiver<MathWorkerResult>,
}

impl MathWorker {
    fn spawn(proxy: EventLoopProxy<AppEvent>) -> Result<Self> {
        let (task_tx, task_rx) = mpsc::channel::<MathWorkerTask>();
        let (result_tx, result_rx) = mpsc::channel::<MathWorkerResult>();
        thread::Builder::new()
            .name("bt-math-worker".to_owned())
            .spawn(move || {
                let engine = MathEngine::new();
                let mut image_decoder = InlineImageDecoder::default();
                while let Ok(work) = task_rx.recv() {
                    let MathWorkerTask {
                        task,
                        foreground_rgb,
                    } = work;
                    let completion = match task {
                        SessionDecorationTask::Math(task) => match *task {
                            SessionMathTask::Frozen(mut task) => {
                                let result =
                                    render_detection_task(&engine, &mut task, foreground_rgb);
                                DecorationWorkerCompletion::Math {
                                    task: Box::new(SessionMathTask::Frozen(task)),
                                    result,
                                }
                            }
                            SessionMathTask::Live(mut task) => {
                                let result =
                                    render_live_detection_task(&engine, &mut task, foreground_rgb);
                                DecorationWorkerCompletion::Math {
                                    task: Box::new(SessionMathTask::Live(task)),
                                    result,
                                }
                            }
                        },
                        SessionDecorationTask::InlineImage(task) => {
                            let result = image_decoder.decode(task.clone());
                            DecorationWorkerCompletion::InlineImage { task, result }
                        }
                    };
                    if result_tx.send(MathWorkerResult { completion }).is_err() {
                        break;
                    }
                    let _ = proxy.send_event(AppEvent::MathReady);
                }
            })
            .context("spawn math rendering worker")?;
        Ok(Self {
            tasks: task_tx,
            results: result_rx,
        })
    }
}

fn disable_math_worker_state(running: &mut bool, notice_pending: &mut bool) -> bool {
    if !*running {
        return false;
    }
    *running = false;
    *notice_pending = true;
    eprintln!("formula rendering worker stopped; terminal input and output remain available");
    true
}

fn take_math_worker_notice(notice_pending: &mut bool) -> Option<&'static str> {
    if std::mem::take(notice_pending) {
        Some(MATH_WORKER_STOPPED_NOTICE)
    } else {
        None
    }
}

/// Drain the real session queue into the renderer channel. A dead optional-decoration worker is
/// a one-way feature downgrade, never a terminal/runtime error.
fn dispatch_pending_math_tasks(
    session: &mut DualPlaneSession,
    tasks: &mpsc::Sender<MathWorkerTask>,
    running: &mut bool,
    notice_pending: &mut bool,
) -> bool {
    if !*running {
        return false;
    }
    while let Some(task) = session.take_decoration_worker_task() {
        if tasks
            .send(MathWorkerTask {
                task,
                foreground_rgb: foreground_rgb(),
            })
            .is_err()
        {
            return disable_math_worker_state(running, notice_pending);
        }
    }
    false
}

#[derive(Clone, Copy, Debug)]
struct DpiSnapshot {
    winit_scale: f64,
    win32_dpi: u32,
    authoritative_scale: f64,
    rect: bt_platform::WindowRect,
}

struct Runtime {
    renderer: Renderer,
    pty: Option<PtySession>,
    session: DualPlaneSession,
    math_worker: MathWorker,
    math_worker_running: bool,
    math_worker_notice_pending: bool,
    projection: ViewportProjection,
    pending_frames: LatestFrameSlot,
    grid: GridSize,
    modifiers: ModifiersState,
    pending_keyboard_at: Option<Instant>,
    math_context_menu: bt_platform::MathContextMenu,
    window: Arc<Window>,
    startup_started: Instant,
    trace_startup: bool,
    trace_resize: bool,
    trace_perf: bool,
    resize_trace_logged_transaction: u64,
    resize_trace_logged_events: usize,
    background_visible: Option<Duration>,
    first_text_visible: Option<Duration>,
    window_shown: bool,
    first_visible_present_dpi_checked: bool,
    first_text_presented: bool,
    last_presented_frame: Option<ViewportFrame>,
    preedit: Option<Preedit>,
    ime_active: bool,
    ime_cursor_throttle: ImeCursorThrottle,
    ime_system_caret: bt_platform::ImeSystemCaret,
    pointer_position: Option<PhysicalPosition<f64>>,
    mouse_route: Option<MouseRoute>,
    click_tracker: ClickTracker,
    line_wheel_remainder: f64,
    pixel_wheel_remainder: f64,
    pending_pty_resize: Option<PendingPtyResize>,
    pending_resize_present: Option<GridSize>,
    hyperlink_hover: HyperlinkHover,
    math_hover_anchor: Option<MathBlockAnchor>,
    math_hover_clear_at: Option<Instant>,
    pending_math_context_anchor: Option<MathBlockAnchor>,
}

#[derive(Clone, Copy, Debug)]
struct PendingPtyResize {
    grid: GridSize,
    physical: PhysicalSize<u32>,
    deadline: Instant,
}

fn coalesce_pty_resize(
    pending: &mut Option<PendingPtyResize>,
    grid: GridSize,
    physical: PhysicalSize<u32>,
    observed_at: Instant,
) {
    *pending = Some(PendingPtyResize {
        grid,
        physical,
        deadline: observed_at + WINDOW_RESIZE_QUIET,
    });
}

fn take_due_pty_resize(
    pending: &mut Option<PendingPtyResize>,
    now: Instant,
) -> Option<PendingPtyResize> {
    pending
        .is_some_and(|resize| now >= resize.deadline)
        .then(|| {
            pending
                .take()
                .expect("due resize was present immediately before take")
        })
}

#[derive(Clone)]
enum MouseRoute {
    Local(SelectionDrag),
    Forward(input::MouseProtocolButton),
    MathBlock,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UserInputKind {
    Keyboard,
    Mouse,
}

impl UserInputKind {
    fn returns_view_to_live(self) -> bool {
        matches!(self, Self::Keyboard)
    }
}

fn live_viewport_mouse_hit(frame: &ViewportFrame, hit: bt_render::GridHit) -> bt_render::GridHit {
    bt_render::GridHit {
        // The mapping belongs to the actual presented frame. Frozen/staging pixels above the
        // mutable viewport retain Windows Terminal's clamp-to-live-row-zero behaviour.
        row: frame
            .live_point_at(hit.row, hit.column)
            .map_or(0, |point| point.row),
        column: hit.column,
    }
}

fn route_forwarded_mouse_button(
    route: &mut Option<MouseRoute>,
    state: ElementState,
    button: input::MouseProtocolButton,
    hit: bt_render::GridHit,
    modes: TerminalModes,
    modifiers: ModifiersState,
) -> Option<Vec<u8>> {
    let forward = !modifiers.shift_key() && modes.mouse_tracking != MouseTracking::Off;
    match state {
        ElementState::Pressed if forward => {
            *route = Some(MouseRoute::Forward(button));
            Some(input::mouse_bytes(
                modes.sgr_mouse,
                button,
                input::MouseProtocolEvent::Press,
                hit.row,
                hit.column,
                modifiers,
            ))
        }
        ElementState::Released if matches!(route, Some(MouseRoute::Forward(_))) => {
            *route = None;
            Some(input::mouse_bytes(
                modes.sgr_mouse,
                button,
                input::MouseProtocolEvent::Release,
                hit.row,
                hit.column,
                modifiers,
            ))
        }
        _ => None,
    }
}

#[derive(Clone)]
struct SelectionDrag {
    mode: SelectionDragMode,
    origin_row: u32,
    origin_column: u32,
    origin: ViewSelection,
    hyperlink: Option<HyperlinkHit>,
    open_hyperlink_on_release: bool,
    local_image_path: Option<PathBuf>,
    open_local_image_on_release: bool,
}

#[derive(Clone, Copy)]
enum SelectionDragMode {
    Linear,
    Word,
    Line,
}

fn should_copy_on_select_release(route: Option<&MouseRoute>, single_click: bool) -> bool {
    !single_click && matches!(route, Some(MouseRoute::Local(_)))
}

#[derive(Default)]
struct ClickTracker {
    last_at: Option<Instant>,
    last_cell: Option<(u32, u32)>,
    count: u8,
}

impl ClickTracker {
    fn register(&mut self, row: u32, column: u32, now: Instant) -> u8 {
        if self.last_cell == Some((row, column))
            && self
                .last_at
                .is_some_and(|last| now.saturating_duration_since(last) <= MULTI_CLICK_INTERVAL)
        {
            self.count = self.count % 3 + 1;
        } else {
            self.count = 1;
        }
        self.last_at = Some(now);
        self.last_cell = Some((row, column));
        self.count
    }
}

#[derive(Default)]
struct HyperlinkHover {
    candidate: Option<HyperlinkHit>,
    show_at: Option<Instant>,
    active: Option<HyperlinkHit>,
    blocked: bool,
}

impl HyperlinkHover {
    fn observe(&mut self, hit: Option<HyperlinkHit>, now: Instant) -> bool {
        if self.active.is_some() && hit.as_ref() == self.active.as_ref() {
            self.candidate = None;
            self.show_at = None;
            return false;
        }
        if self.candidate.is_some() && hit.as_ref() == self.candidate.as_ref() {
            return false;
        }
        let active_changed = self.active.take().is_some();
        self.blocked = false;
        self.candidate = hit;
        self.show_at = self.candidate.as_ref().map(|_| now + HYPERLINK_HOVER_DELAY);
        active_changed
    }

    fn activate_if_due(&mut self, now: Instant) -> bool {
        if self.show_at.is_none_or(|deadline| now < deadline) {
            return false;
        }
        self.show_at = None;
        self.active = self.candidate.take();
        self.blocked = false;
        self.active.is_some()
    }

    fn show_blocked(&mut self, hyperlink: HyperlinkHit) {
        self.candidate = None;
        self.show_at = None;
        self.active = Some(hyperlink);
        self.blocked = true;
    }

    fn clear(&mut self) -> bool {
        self.candidate = None;
        self.show_at = None;
        self.blocked = false;
        self.active.take().is_some()
    }

    fn status_text(&self, columns: usize) -> Option<String> {
        let uri = self
            .active
            .as_ref()?
            .uri
            .chars()
            .map(|character| {
                if character.is_control() {
                    '\u{fffd}'
                } else {
                    character
                }
            })
            .collect::<Vec<_>>();
        if columns == 0 {
            return None;
        }
        let suffix = if self.blocked { " · blocked" } else { "" };
        let suffix_len = suffix.chars().count();
        if columns <= suffix_len {
            return Some("blocked".chars().take(columns).collect());
        }
        let target_columns = columns - suffix_len;
        let mut status = if uri.len() > target_columns {
            uri.into_iter()
                .take(target_columns.saturating_sub(1))
                .chain(['…'])
                .collect::<String>()
        } else {
            uri.into_iter().collect()
        };
        status.push_str(suffix);
        Some(status)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HyperlinkActivation {
    None,
    Open,
    Blocked,
}

fn hyperlink_activation(control: bool, click_no_drag: bool, uri: &str) -> HyperlinkActivation {
    if !control || !click_no_drag {
        return HyperlinkActivation::None;
    }
    let allowed = uri.split_once(':').is_some_and(|(scheme, remainder)| {
        (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
            && remainder.starts_with("//")
            && remainder.len() > 2
    }) && !uri
        .chars()
        .any(|character| character.is_control() || character.is_whitespace());
    if allowed {
        HyperlinkActivation::Open
    } else {
        HyperlinkActivation::Blocked
    }
}

fn local_image_activation(
    control: bool,
    click_no_drag: bool,
    worker_verified_path: Option<&std::path::Path>,
) -> bool {
    control && click_no_drag && worker_verified_path.is_some()
}

#[derive(Debug, Default)]
struct ImeCursorThrottle {
    last_sent_at: Option<Instant>,
    last_sent_area: Option<ImeCursorArea>,
    pending: Option<ImeCursorArea>,
}

impl ImeCursorThrottle {
    fn offer(&mut self, area: ImeCursorArea, now: Instant) -> Option<ImeCursorArea> {
        if self.last_sent_area == Some(area) {
            self.pending = None;
            return None;
        }
        if self
            .last_sent_at
            .is_none_or(|last| now.saturating_duration_since(last) >= IME_CURSOR_AREA_INTERVAL)
        {
            self.mark_sent(area, now);
            Some(area)
        } else {
            self.pending = Some(area);
            None
        }
    }

    fn flush_due(&mut self, now: Instant) -> Option<ImeCursorArea> {
        let area = self.pending?;
        if now < self.deadline()? {
            return None;
        }
        self.mark_sent(area, now);
        Some(area)
    }

    fn deadline(&self) -> Option<Instant> {
        self.pending.and(
            self.last_sent_at
                .map(|last| last + IME_CURSOR_AREA_INTERVAL),
        )
    }

    fn mark_sent(&mut self, area: ImeCursorArea, now: Instant) {
        self.last_sent_at = Some(now);
        self.last_sent_area = Some(area);
        self.pending = None;
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

impl Runtime {
    fn create(
        event_loop: &ActiveEventLoop,
        proxy: EventLoopProxy<AppEvent>,
        startup_started: Instant,
    ) -> Result<Self> {
        let trace_startup = std::env::var_os("BT_STARTUP_TRACE").is_some();
        let trace_resize = std::env::var_os("BT_RESIZE_TRACE").is_some();
        let trace_perf = std::env::var_os("BT_PERF_TRACE").is_some();
        let phase_started = Instant::now();
        let attributes = Window::default_attributes()
            .with_title(WINDOW_TITLE)
            .with_inner_size(LogicalSize::new(INITIAL_WIDTH, INITIAL_HEIGHT))
            // Do not expose the system class brush while the first swapchain image is pending.
            .with_visible(false);
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .context("create native window")?,
        );
        install_theme_class_background(&window)?;
        window.set_ime_allowed(true);
        let hwnd = window_hwnd(&window)?;
        let ime_system_caret = bt_platform::ImeSystemCaret::new(hwnd);
        let math_context_menu = bt_platform::MathContextMenu::new(hwnd)
            .map_err(|error| anyhow!(error))
            .context("install deferred formula context menu")?;
        let window_time = phase_started.elapsed();
        let physical = window.inner_size();
        let startup_dpi = dpi_snapshot(&window)?;
        let startup_scale_factor = startup_dpi.authoritative_scale;
        let phase_started = Instant::now();
        let renderer = pollster::block_on(Renderer::new(
            Arc::clone(&window),
            physical.width,
            physical.height,
            startup_scale_factor,
        ))
        .context("initialize wgpu renderer")?;
        ensure_metrics_match_authoritative_scale(
            renderer.metrics().scale_factor,
            startup_scale_factor,
        )?;
        trace_surface_size_clamp(
            trace_startup || trace_resize,
            "BT_STARTUP",
            physical,
            renderer.presentation_geometry(),
        );
        ensure_swapchain_matches_inner(&renderer, physical)?;
        log_dpi_snapshot(
            "create",
            startup_dpi,
            None,
            renderer.presentation_geometry(),
            physical,
        );
        let renderer_time = phase_started.elapsed();
        let render_physical = presentation_physical_size(renderer.presentation_geometry());
        let grid = renderer
            .metrics()
            .grid_for_pixels(render_physical.width, render_physical.height);
        let probe_input = load_probe_input()?;
        let pty_proxy = proxy.clone();
        let wake: OutputWake = Arc::new(move || {
            let _ = pty_proxy.send_event(AppEvent::PtyOutput);
        });
        let phase_started = Instant::now();
        let pty = if probe_input.is_none() {
            Some(
                PtySession::spawn_default(pty_size(grid, render_physical), wake)
                    .context("spawn default PowerShell in ConPTY")?,
            )
        } else {
            None
        };
        let conpty_source = pty
            .as_ref()
            .map(|pty| pty.conpty_source().to_string())
            .unwrap_or_else(|| "direct-input".to_string());
        if trace_startup || trace_resize {
            eprintln!("BT_CONPTY_SOURCE source={conpty_source:?}");
        }
        let pty_time = phase_started.elapsed();
        let columns = nonzero_u32(grid.columns.get());
        let rows = nonzero_u32(grid.rows.get());
        let mut session = DualPlaneSession::with_quotas_and_cell_height(
            columns,
            rows,
            DEFAULT_STAGING_QUOTA,
            M0_FROZEN_LINE_QUOTA,
            renderer.metrics().cell_height_subpixels(),
        );
        session.set_cell_width_subpixels(cell_width_subpixels(renderer.metrics()));
        session.set_ascii_baseline_subpixels(renderer.metrics().ascii_baseline_subpixels());
        session.set_math_layout_options(MathLayoutOptions {
            detect_image_paths: true,
            ..MathLayoutOptions::default()
        });
        session.set_layout_key(LayoutKey {
            width_cells: columns,
            dpi_milli: renderer.metrics().dpi_milli(),
            font_rev: 1,
            theme_rev: theme_revision(),
        });
        if let Some(bytes) = probe_input.as_deref() {
            session
                .feed(bytes)
                .context("feed BT_PROBE_INPUT bytes directly into terminal")?;
        }
        let projection = session.new_projection(session.layout_key());
        let math_worker = MathWorker::spawn(proxy)?;
        let mut runtime = Self {
            renderer,
            pty,
            session,
            math_worker,
            math_worker_running: true,
            math_worker_notice_pending: false,
            projection,
            pending_frames: LatestFrameSlot::default(),
            grid,
            modifiers: ModifiersState::default(),
            pending_keyboard_at: None,
            math_context_menu,
            window,
            startup_started,
            trace_startup,
            trace_resize,
            trace_perf,
            resize_trace_logged_transaction: 0,
            resize_trace_logged_events: 0,
            background_visible: None,
            first_text_visible: None,
            window_shown: false,
            first_visible_present_dpi_checked: false,
            first_text_presented: false,
            last_presented_frame: None,
            preedit: None,
            ime_active: false,
            ime_cursor_throttle: ImeCursorThrottle::default(),
            ime_system_caret,
            pointer_position: None,
            mouse_route: None,
            click_tracker: ClickTracker::default(),
            line_wheel_remainder: 0.0,
            pixel_wheel_remainder: 0.0,
            pending_pty_resize: None,
            pending_resize_present: None,
            hyperlink_hover: HyperlinkHover::default(),
            math_hover_anchor: None,
            math_hover_clear_at: None,
            pending_math_context_anchor: None,
        };
        if trace_startup {
            let renderer_phases = runtime.renderer.init_timings();
            eprintln!(
                "BT_STARTUP window={}ms adapter={}ms device={}ms surface={}ms fonts={}ms metrics={}ms render_resources={}ms renderer_total={}ms pty_spawn={}ms probe_input={} conpty_source={conpty_source:?} runtime_ready={}ms",
                window_time.as_millis(),
                renderer_phases.adapter.as_millis(),
                renderer_phases.device.as_millis(),
                renderer_phases.surface_configure.as_millis(),
                renderer_phases.font_system.as_millis(),
                renderer_phases.font_metrics.as_millis(),
                renderer_phases.render_resources.as_millis(),
                renderer_time.as_millis(),
                pty_time.as_millis(),
                probe_input.as_ref().map_or(0, Vec::len),
                startup_started.elapsed().as_millis(),
            );
        }
        runtime.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        // A hidden surface can either accept the clear or report Occluded. In the latter case
        // redraw() republishes the frame; the second call presents immediately after ShowWindow.
        runtime.redraw()?;
        runtime.window.set_visible(true);
        runtime.window_shown = true;
        // Showing a hidden Win32 window can synchronously settle it onto a different monitor.
        // Query Win32 directly: winit's cached scale can race during initial monitor placement.
        runtime.reconcile_authoritative_dpi("show")?;
        // Force one presentation after ShowWindow so the first-present reconciliation below is a
        // visible startup stage even when the hidden pre-clear already succeeded.
        runtime.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        runtime.redraw()?;
        let background_visible = startup_started.elapsed();
        runtime.background_visible = Some(background_visible);
        if trace_startup {
            eprintln!(
                "BT_STARTUP background_visible={}ms",
                background_visible.as_millis()
            );
        }
        Ok(runtime)
    }

    fn publish_frame(&mut self, trigger: FrameTrigger) -> Result<()> {
        let skip_unchanged = matches!(trigger.source, FrameSource::PtyOutput);
        self.publish_frame_inner(trigger, skip_unchanged)
            .map(|_| ())
    }

    fn publish_frame_inner(&mut self, trigger: FrameTrigger, skip_unchanged: bool) -> Result<bool> {
        // Real-machine decoration-state trace (`BT_DECOR_TRACE=<path>`). Runs on every frame trigger
        // — including held/skipped frames — so a persistent stuck-source block is captured even when
        // the presented frame does not change. Zero cost when the variable is unset.
        self.session.trace_decorations();
        if matches!(trigger.source, FrameSource::Keyboard) {
            self.session.release_presentation_hold_for_user_input();
        }
        dispatch_pending_math_tasks(
            &mut self.session,
            &self.math_worker.tasks,
            &mut self.math_worker_running,
            &mut self.math_worker_notice_pending,
        );
        self.session.refresh_projection(&mut self.projection);
        let mut terminal_frame = self
            .session
            .viewport_frame(&mut self.projection)
            .context("project terminal grid into viewport frame")?;
        // State-driven frame hold. Review displacement holds a vanished scroll anchor during a
        // resize reprint. Independently, an unmatched off-band stale-pending DPI record holds the
        // previous complete formula frame while a proven primary reprint is between clear and exact
        // source re-anchor. Both release through projection/session facts (re-anchor, explicit user
        // takeover, or hard lifecycle retirement), never a timer.
        if self.projection.presentation_hold() && self.last_presented_frame.is_some() {
            if self.trace_perf {
                eprintln!(
                    "BT_PERF_TRACE hold=presentation source={:?} review={} exact_source={}",
                    trigger.source,
                    u8::from(self.projection.review_hold()),
                    u8::from(self.projection.exact_source_reprint_hold()),
                );
            }
            return Ok(false);
        }
        if self.session.schedule_visible_artifacts(&terminal_frame) != 0 {
            dispatch_pending_math_tasks(
                &mut self.session,
                &self.math_worker.tasks,
                &mut self.math_worker_running,
                &mut self.math_worker_notice_pending,
            );
        }
        if let Some(hyperlink) = self.hyperlink_hover.active.as_ref()
            && terminal_frame.underline_hyperlink(hyperlink)
        {
            terminal_frame.status_text = self
                .hyperlink_hover
                .status_text(terminal_frame.columns.get() as usize);
        }
        if let Some(notice) = take_math_worker_notice(&mut self.math_worker_notice_pending) {
            terminal_frame.status_text = Some(notice.to_owned());
        }
        let composed = compose_preedit(&terminal_frame, self.preedit.as_ref())
            .context("reject non-rectangular frame before IME composition")?;
        if skip_unchanged
            && pty_frame_is_unchanged(
                self.pending_frames.pending_frame(),
                self.last_presented_frame.as_ref(),
                &composed.frame,
            )
        {
            if self.trace_perf {
                let digest_started = Instant::now();
                let digest = frame_content_digest(&composed.frame);
                let alternate_screen = frame_is_alternate_screen(&composed.frame);
                let digest_elapsed = digest_started.elapsed();
                eprintln!(
                    "BT_PERF_TRACE skip=unchanged source={:?} content_fnv={:016x} alt={} digest_us={}",
                    trigger.source,
                    digest.content_fnv,
                    u8::from(alternate_screen),
                    digest_elapsed.as_micros(),
                );
            }
            return Ok(false);
        }
        if self.ime_active {
            let area = self.renderer.ime_cursor_area(&composed.frame);
            if let Some(area) = self.ime_cursor_throttle.offer(area, Instant::now()) {
                self.apply_ime_cursor_area(area);
            }
        }
        self.session
            .record_published_frame(&composed.frame, trigger.occurred_at);
        self.flush_resize_trace();
        self.pending_frames
            .publish(composed.frame, trigger)
            .context("reject non-rectangular frame at publish boundary")?;
        self.window.request_redraw();
        Ok(true)
    }

    fn disable_math_worker(&mut self) -> bool {
        disable_math_worker_state(
            &mut self.math_worker_running,
            &mut self.math_worker_notice_pending,
        )
    }

    fn apply_math_results(&mut self) -> Result<()> {
        let mut changed = false;
        loop {
            match self.math_worker.results.try_recv() {
                Ok(completion) => {
                    changed |= match completion.completion {
                        DecorationWorkerCompletion::Math { task, result } => match *task {
                            SessionMathTask::Frozen(task) => {
                                self.session.complete_worker_result(task, result)
                            }
                            SessionMathTask::Live(task) => {
                                self.session.complete_live_worker_result(task, result)
                            }
                        },
                        DecorationWorkerCompletion::InlineImage { task, result } => {
                            self.session.complete_inline_image_result(task, result)
                        }
                    };
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    changed |= self.disable_math_worker();
                    break;
                }
            }
        }
        dispatch_pending_math_tasks(
            &mut self.session,
            &self.math_worker.tasks,
            &mut self.math_worker_running,
            &mut self.math_worker_notice_pending,
        );
        if changed {
            self.publish_frame(FrameTrigger {
                occurred_at: Instant::now(),
                source: FrameSource::Expose,
            })?;
        }
        Ok(())
    }

    fn advance_live_math_if_due(&mut self, now: Instant) -> Result<()> {
        if self
            .session
            .live_stability_deadline()
            .is_some_and(|deadline| now >= deadline)
        {
            self.session.advance_live_stability(now);
            let disabled = dispatch_pending_math_tasks(
                &mut self.session,
                &self.math_worker.tasks,
                &mut self.math_worker_running,
                &mut self.math_worker_notice_pending,
            );
            if disabled {
                self.publish_frame(FrameTrigger {
                    occurred_at: now,
                    source: FrameSource::Expose,
                })?;
            }
        }
        Ok(())
    }

    fn publish_pty_drain_frame(&mut self, now: Instant) -> Result<()> {
        let keyboard_at = self.pending_keyboard_at;
        let published = self.publish_frame_inner(
            FrameTrigger {
                occurred_at: keyboard_at.unwrap_or(now),
                source: if keyboard_at.is_some() {
                    FrameSource::Keyboard
                } else {
                    FrameSource::PtyOutput
                },
            },
            true,
        )?;
        let sync_open = self.session.synchronized_update_deadline().is_some();
        if published || !sync_open {
            self.pending_keyboard_at = None;
        } else if self.trace_perf {
            eprintln!("BT_PERF_TRACE defer=synchronized-update");
        }
        Ok(())
    }

    fn flush_resize_trace(&mut self) {
        if !self.trace_resize {
            return;
        }
        let transaction = self.session.resize_trace_transaction();
        if transaction != self.resize_trace_logged_transaction {
            self.resize_trace_logged_transaction = transaction;
            self.resize_trace_logged_events = 0;
        }
        let trace = self.session.resize_trace();
        let conpty_source = self
            .pty
            .as_ref()
            .map(|pty| pty.conpty_source().to_string())
            .unwrap_or_else(|| "direct-input".to_string());
        for event in &trace[self.resize_trace_logged_events.min(trace.len())..] {
            eprintln!("BT_RESIZE_TRACE conpty_source={conpty_source:?} {event:?}");
        }
        self.resize_trace_logged_events = trace.len();
    }

    fn apply_ime_cursor_area(&mut self, area: ImeCursorArea) {
        // Renderer pixels, winit PhysicalPosition, and a per-monitor-aware Win32 client area all
        // share the client-origin device-pixel axis. No screen-origin translation belongs here.
        self.window.set_ime_cursor_area(
            PhysicalPosition::new(area.x, area.y),
            PhysicalSize::new(area.width, area.height),
        );
        if let Err(error) = self.ime_system_caret.update(area.x, area.y) {
            eprintln!("Chinese IME system-caret update ignored: {error}");
        }
    }

    fn flush_ime_cursor_area(&mut self, now: Instant) {
        if let Some(area) = self.ime_cursor_throttle.flush_due(now) {
            self.apply_ime_cursor_area(area);
        }
    }

    fn drain_pty(&mut self) -> Result<()> {
        if self.pty.is_none() {
            return Ok(());
        }
        let mut changed = false;
        loop {
            let bytes = self
                .pty
                .as_ref()
                .expect("PTY mode checked above")
                .read_output();
            if bytes.is_empty() {
                break;
            }
            debug_assert!(bytes.len() <= bt_pty::TERM_READ_QUANTUM.get());
            self.session
                .feed_at(&bytes, Instant::now())
                .context("apply PTY output")?;
            for reply in self.session.take_pty_writes() {
                self.pty
                    .as_mut()
                    .expect("PTY mode checked above")
                    .write(&reply)
                    .context("return terminal protocol reply to PTY")?;
            }
            changed = true;
        }
        if changed {
            // The vendor parser withholds bytes inside an open DEC 2026 block, so projecting here
            // cannot expose its intermediate state. It can expose ordinary output before a
            // trailing BSU or a completed update before the next BSU; the unchanged-frame gate in
            // publish_frame cheaply suppresses drains containing only still-buffered sync bytes.
            self.publish_pty_drain_frame(Instant::now())?;
        }
        Ok(())
    }

    fn finish_synchronized_update_if_due(&mut self, now: Instant) -> Result<()> {
        let due = self
            .session
            .synchronized_update_deadline()
            .is_some_and(|deadline| deadline <= now);
        if due
            && self
                .session
                .finish_synchronized_update(now)
                .context("finish timed-out DEC 2026 synchronized update")?
        {
            self.publish_pty_drain_frame(now)?;
        }
        Ok(())
    }

    fn finish_resize_if_quiescent(&mut self, now: Instant) -> Result<()> {
        if self
            .session
            .finish_resize_if_quiescent(now)
            .context("finish ConPTY resize transaction")?
        {
            self.publish_frame(FrameTrigger {
                occurred_at: now,
                source: FrameSource::Expose,
            })?;
        }
        Ok(())
    }

    fn flush_pending_pty_resize(&mut self, now: Instant) -> Result<()> {
        let Some(pending) = take_due_pty_resize(&mut self.pending_pty_resize, now) else {
            return Ok(());
        };
        if let Some(pty) = self.pty.as_ref() {
            pty.resize(pty_size(pending.grid, pending.physical))
                .context("commit coalesced final ConPTY resize")?;
        }
        let reconciled = self.session.mark_pty_resize_requested_at(
            nonzero_u32(pending.grid.columns.get()),
            nonzero_u32(pending.grid.rows.get()),
            now,
        );
        if reconciled {
            self.publish_frame(FrameTrigger {
                occurred_at: now,
                source: FrameSource::Resize,
            })?;
        }
        Ok(())
    }

    fn frame_hit(&self) -> Option<bt_render::GridHit> {
        let position = self.pointer_position?;
        let frame = self.last_presented_frame.as_ref()?;
        self.renderer
            .metrics()
            .hit_test_frame(frame, position.x, position.y)
    }

    fn math_hit(&self) -> Option<MathHit> {
        let position = self.pointer_position?;
        let frame = self.last_presented_frame.as_ref()?;
        self.renderer.math_hit_test(frame, position.x, position.y)
    }

    fn hyperlink_hit(&self, hit: bt_render::GridHit) -> Option<HyperlinkHit> {
        self.last_presented_frame
            .as_ref()?
            .hyperlink_at(hit.row, hit.column)
    }

    fn local_image_path_hit(&self, hit: bt_render::GridHit) -> Option<PathBuf> {
        let anchor = self
            .last_presented_frame
            .as_ref()?
            .anchor_at(hit.row, hit.column, Bias::Before)
            .ok()??;
        self.session.decoded_local_image_path_at(&anchor)
    }

    fn activate_hyperlink_hover_if_due(&mut self, now: Instant) -> Result<()> {
        if self.hyperlink_hover.activate_if_due(now) {
            self.publish_interaction_frame()?;
        }
        Ok(())
    }

    fn pointer_left(&mut self) -> Result<()> {
        self.pointer_position = None;
        let hyperlink_changed = self.hyperlink_hover.clear();
        if self.math_hover_anchor.is_some() {
            self.math_hover_clear_at = Some(Instant::now() + Duration::from_millis(500));
        }
        if hyperlink_changed {
            self.publish_interaction_frame()?;
        }
        Ok(())
    }

    fn activate_hyperlink(&mut self, hyperlink: HyperlinkHit) -> Result<()> {
        match hyperlink_activation(true, true, &hyperlink.uri) {
            HyperlinkActivation::None => {}
            HyperlinkActivation::Open => {
                let result = window_hwnd(&self.window).and_then(|hwnd| {
                    bt_platform::shell_execute(hwnd, &hyperlink.uri)
                        .map_err(|error| anyhow!(error))
                        .context("open HTTP hyperlink in the system browser")
                });
                if let Err(error) = result {
                    eprintln!("recoverable hyperlink open failure: {error:#}");
                }
            }
            HyperlinkActivation::Blocked => {
                self.hyperlink_hover.show_blocked(hyperlink);
                self.publish_interaction_frame()?;
            }
        }
        Ok(())
    }

    fn activate_local_image_path(&self, path: &std::path::Path) {
        let result = window_hwnd(&self.window).and_then(|hwnd| {
            bt_platform::open_local_file(hwnd, path)
                .map_err(|error| anyhow!(error))
                .context("open decoded local image in the system viewer")
        });
        if let Err(error) = result {
            eprintln!("recoverable local image open failure: {error:#}");
        }
    }

    fn update_math_hover(&mut self, now: Instant) -> Result<Option<MathHit>> {
        let hit = self.math_hit();
        if let Some(hit) = hit.as_ref() {
            self.math_hover_clear_at = None;
            if self.math_hover_anchor.as_ref() != Some(&hit.anchor) {
                self.math_hover_anchor = Some(hit.anchor.clone());
                if self.session.set_math_hover(Some(&hit.anchor)) {
                    self.publish_interaction_frame()?;
                }
            }
        } else if self.math_hover_anchor.is_some() && self.math_hover_clear_at.is_none() {
            self.math_hover_clear_at = Some(now + Duration::from_millis(500));
        }
        Ok(hit)
    }

    fn clear_math_hover_if_due(&mut self, now: Instant) -> Result<()> {
        if self
            .math_hover_clear_at
            .is_none_or(|deadline| now < deadline)
        {
            return Ok(());
        }
        self.math_hover_clear_at = None;
        self.math_hover_anchor = None;
        if self.session.set_math_hover(None) {
            self.publish_interaction_frame()?;
        }
        Ok(())
    }

    fn copy_math_latex(&mut self, anchor: &MathBlockAnchor) {
        let Some(source) = self.session.math_source(anchor) else {
            return;
        };
        let result = window_hwnd(&self.window).and_then(|hwnd| {
            bt_platform::set_clipboard_text(hwnd, source)
                .map_err(|error| anyhow!(error))
                .context("copy original LaTeX source to clipboard")
        });
        recoverable_clipboard_write(result, "formula copy");
    }

    fn apply_math_context_menu_result(&mut self) {
        let Some(result) = self.math_context_menu.take_result() else {
            return;
        };
        let anchor = self.pending_math_context_anchor.take();
        self.mouse_route = None;
        match (result, anchor) {
            (Ok(true), Some(anchor)) => self.copy_math_latex(&anchor),
            (Ok(true), None) => {
                eprintln!("recoverable formula context-menu result had no pending anchor");
            }
            (Ok(false), _) => {}
            (Err(error), _) => {
                eprintln!("recoverable formula context-menu failure: {error}");
            }
        }
    }

    fn publish_interaction_frame(&mut self) -> Result<()> {
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })
    }

    fn clear_selection(&mut self) {
        self.session.set_view_selection(None);
        self.projection.set_selection(None);
    }

    fn return_to_live_for_input(&mut self) -> bool {
        let changed = self.session.view_selection().is_some()
            || self.projection.is_scrolled()
            || self.session.release_presentation_hold_for_user_input();
        self.clear_selection();
        self.projection.scroll_to_bottom();
        changed
    }

    fn send_user_input(
        &mut self,
        bytes: &[u8],
        context: &'static str,
        kind: UserInputKind,
    ) -> Result<()> {
        let view_changed = kind.returns_view_to_live() && self.return_to_live_for_input();
        self.pending_keyboard_at = Some(Instant::now());
        if let Some(pty) = self.pty.as_mut() {
            pty.write(bytes).with_context(|| context)?;
        }
        if view_changed {
            self.publish_frame(FrameTrigger {
                occurred_at: self.pending_keyboard_at.unwrap_or_else(Instant::now),
                source: FrameSource::Keyboard,
            })?;
        }
        Ok(())
    }

    fn copy_selection(&mut self) -> Result<()> {
        let window = Arc::clone(&self.window);
        if !copy_selection(&mut self.session, &mut self.projection, |text| {
            write_terminal_clipboard_text(&window, text)
        }) {
            return Ok(());
        }
        self.publish_interaction_frame()
    }

    fn copy_selection_on_release(&self) {
        let window = Arc::clone(&self.window);
        write_selection_text(&self.session, true, |text| {
            write_terminal_clipboard_text(&window, text)
        });
    }

    fn scroll_view(&mut self, rows: i32) -> Result<()> {
        self.projection.scroll_by_rows(rows);
        self.publish_interaction_frame()
    }

    fn begin_local_selection(&mut self, hit: bt_render::GridHit) -> Result<()> {
        let count = self
            .click_tracker
            .register(hit.row, hit.column, Instant::now());
        let local_image_path = self.local_image_path_hit(hit);
        let frame = self
            .last_presented_frame
            .as_ref()
            .context("missing frame for mouse hit")?;
        let hyperlink = frame.hyperlink_at(hit.row, hit.column);
        let open_hyperlink_on_release = self.modifiers.control_key() && hyperlink.is_some();
        let open_local_image_on_release = local_image_activation(
            self.modifiers.control_key(),
            true,
            local_image_path.as_deref(),
        );
        // Local hits are clamped to a continuous frame, which supplies anchors for every grid cell.
        let (mode, origin, initial) = match count {
            2 => {
                let selection = frame
                    .word_selection(hit.row, hit.column)
                    .context("reject non-rectangular frame during word selection")?
                    .context("word selection hit has no anchor")?;
                (SelectionDragMode::Word, selection.clone(), selection)
            }
            3 => {
                let selection = frame
                    .line_selection(hit.row)
                    .context("reject non-rectangular frame during line selection")?
                    .context("line selection hit has no anchor")?;
                (SelectionDragMode::Line, selection.clone(), selection)
            }
            _ => {
                let start = frame
                    .anchor_at(hit.row, hit.column, Bias::Before)
                    .context("reject non-rectangular frame during anchor lookup")?
                    .context("selection hit has no start anchor")?;
                let end = frame
                    .anchor_at(hit.row, hit.column, Bias::After)
                    .context("reject non-rectangular frame during anchor lookup")?
                    .context("selection hit has no end anchor")?;
                (
                    SelectionDragMode::Linear,
                    ViewSelection {
                        start: start.clone(),
                        end,
                    },
                    ViewSelection {
                        start: start.clone(),
                        end: start,
                    },
                )
            }
        };
        self.session.set_view_selection(Some(initial));
        self.mouse_route = Some(MouseRoute::Local(SelectionDrag {
            mode,
            origin_row: hit.row,
            origin_column: hit.column,
            origin,
            hyperlink,
            open_hyperlink_on_release,
            local_image_path,
            open_local_image_on_release,
        }));
        self.publish_interaction_frame()
    }

    fn extend_local_selection(&mut self, hit: bt_render::GridHit) -> Result<()> {
        let Some(MouseRoute::Local(drag)) = self.mouse_route.as_ref().cloned() else {
            return Ok(());
        };
        if matches!(drag.mode, SelectionDragMode::Linear)
            && hit.row == drag.origin_row
            && hit.column == drag.origin_column
        {
            return Ok(());
        }
        let frame = self
            .last_presented_frame
            .as_ref()
            .context("missing frame for mouse drag")?;
        let current = match drag.mode {
            SelectionDragMode::Linear => ViewSelection {
                start: frame
                    .anchor_at(hit.row, hit.column, Bias::Before)
                    .context("reject non-rectangular frame during drag anchor lookup")?
                    .context("drag hit has no start anchor")?,
                end: frame
                    .anchor_at(hit.row, hit.column, Bias::After)
                    .context("reject non-rectangular frame during drag anchor lookup")?
                    .context("drag hit has no end anchor")?,
            },
            SelectionDragMode::Word => frame
                .word_selection(hit.row, hit.column)
                .context("reject non-rectangular frame during word drag")?
                .context("word drag hit has no anchor")?,
            SelectionDragMode::Line => frame
                .line_selection(hit.row)
                .context("reject non-rectangular frame during line drag")?
                .context("line drag hit has no anchor")?,
        };
        let after_origin = (hit.row, hit.column) >= (drag.origin_row, drag.origin_column);
        self.session.set_view_selection(Some(if after_origin {
            ViewSelection {
                start: drag.origin.start,
                end: current.end,
            }
        } else {
            ViewSelection {
                start: current.start,
                end: drag.origin.end,
            }
        }));
        self.publish_interaction_frame()
    }

    fn pointer_moved(&mut self, position: PhysicalPosition<f64>) -> Result<()> {
        self.pointer_position = Some(position);
        let now = Instant::now();
        let math_hit = self.update_math_hover(now)?;
        let hit = self.frame_hit();
        let hyperlink = hit
            .filter(|_| {
                math_hit.is_none() && !matches!(self.mouse_route, Some(MouseRoute::Local(_)))
            })
            .and_then(|hit| self.hyperlink_hit(hit));
        if self.hyperlink_hover.observe(hyperlink, now) {
            self.publish_interaction_frame()?;
        }
        let Some(hit) = hit else {
            return Ok(());
        };
        if math_hit.is_some() || matches!(self.mouse_route, Some(MouseRoute::MathBlock)) {
            return Ok(());
        }
        if matches!(self.mouse_route, Some(MouseRoute::Local(_))) {
            return self.extend_local_selection(hit);
        }
        let frame = self
            .last_presented_frame
            .as_ref()
            .context("missing frame for forwarded mouse motion")?;
        let hit = live_viewport_mouse_hit(frame, hit);
        let modes = self.session.terminal_modes();
        if self.modifiers.shift_key() || modes.mouse_tracking == MouseTracking::Off {
            return Ok(());
        }
        let button = match self.mouse_route {
            Some(MouseRoute::Forward(button)) if modes.mouse_tracking != MouseTracking::Click => {
                button
            }
            None if modes.mouse_tracking == MouseTracking::Motion => {
                input::MouseProtocolButton::None
            }
            _ => return Ok(()),
        };
        let bytes = input::mouse_bytes(
            modes.sgr_mouse,
            button,
            input::MouseProtocolEvent::Motion,
            hit.row,
            hit.column,
            self.modifiers,
        );
        self.send_user_input(
            &bytes,
            "forward SGR mouse motion to PTY",
            UserInputKind::Mouse,
        )
    }

    fn mouse_input(&mut self, state: ElementState, button: MouseButton) -> Result<()> {
        if state == ElementState::Released
            && matches!(self.mouse_route, Some(MouseRoute::MathBlock))
        {
            self.mouse_route = None;
            return Ok(());
        }
        if state == ElementState::Pressed
            && let Some(math_hit) = self.math_hit()
            && matches!(button, MouseButton::Left | MouseButton::Right)
        {
            // Formula pixels are one indivisible presentation object in this slice. Swallowing the
            // complete press/release pair intentionally prevents half-source selections and keeps
            // both local selection and application mouse reporting from seeing synthetic cells.
            self.mouse_route = Some(MouseRoute::MathBlock);
            match (button, math_hit.target) {
                (MouseButton::Left, MathHitTarget::ToggleSource) => {
                    if self.session.toggle_math_source(&math_hit.anchor) {
                        self.clear_selection();
                        self.publish_interaction_frame()?;
                    }
                }
                (MouseButton::Left, MathHitTarget::CopyLatex) => {
                    self.copy_math_latex(&math_hit.anchor);
                }
                (MouseButton::Right, _) => match self.math_context_menu.request() {
                    Ok(true) => {
                        self.pending_math_context_anchor = Some(math_hit.anchor.clone());
                    }
                    Ok(false) => {}
                    Err(error) => {
                        eprintln!("recoverable formula context-menu queue failure: {error}");
                    }
                },
                (MouseButton::Left, MathHitTarget::Block) => {
                    if self.session.view_selection().is_some() {
                        self.clear_selection();
                        self.publish_interaction_frame()?;
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        let Some(hit) = self.frame_hit() else {
            return Ok(());
        };
        let Some(protocol_button) = protocol_mouse_button(button) else {
            return Ok(());
        };
        let modes = self.session.terminal_modes();
        let frame = self
            .last_presented_frame
            .as_ref()
            .context("missing frame for forwarded mouse hit")?;
        let forwarded_hit = live_viewport_mouse_hit(frame, hit);
        if let Some(bytes) = route_forwarded_mouse_button(
            &mut self.mouse_route,
            state,
            protocol_button,
            forwarded_hit,
            modes,
            self.modifiers,
        ) {
            return self.send_user_input(
                &bytes,
                "forward mouse button event to PTY",
                UserInputKind::Mouse,
            );
        }
        match state {
            ElementState::Pressed if button == MouseButton::Left => self.begin_local_selection(hit),
            ElementState::Released => {
                self.extend_local_selection(hit)?;
                let release_hyperlink = self.hyperlink_hit(hit);
                let release_local_image_path = self.local_image_path_hit(hit);
                let (single_click, hyperlink_to_open, local_image_to_open) =
                    if let Some(MouseRoute::Local(SelectionDrag {
                        mode: SelectionDragMode::Linear,
                        origin_row,
                        origin_column,
                        hyperlink,
                        open_hyperlink_on_release,
                        local_image_path,
                        open_local_image_on_release,
                        ..
                    })) = self.mouse_route.as_ref()
                        && (*origin_row, *origin_column) == (hit.row, hit.column)
                    {
                        (
                            true,
                            open_hyperlink_on_release
                                .then(|| hyperlink.clone())
                                .flatten()
                                .filter(|pressed| release_hyperlink.as_ref() == Some(pressed)),
                            open_local_image_on_release
                                .then(|| local_image_path.clone())
                                .flatten()
                                .filter(|pressed| {
                                    release_local_image_path.as_ref() == Some(pressed)
                                }),
                        )
                    } else {
                        (false, None, None)
                    };
                let copy_on_select =
                    should_copy_on_select_release(self.mouse_route.as_ref(), single_click);
                self.mouse_route = None;
                if single_click {
                    self.clear_selection();
                    self.publish_interaction_frame()?;
                } else if copy_on_select {
                    self.copy_selection_on_release();
                }
                if let Some(hyperlink) = hyperlink_to_open {
                    self.activate_hyperlink(hyperlink)?;
                }
                if let Some(path) = local_image_to_open {
                    self.activate_local_image_path(&path);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn mouse_wheel(&mut self, delta: MouseScrollDelta) -> Result<()> {
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                let multiplier =
                    match recoverable_wheel_scroll_amount(bt_platform::wheel_scroll_amount()) {
                        bt_platform::WheelScrollAmount::Lines(lines) => lines as f64,
                        bt_platform::WheelScrollAmount::Page => self.grid.rows.get() as f64,
                    };
                self.line_wheel_remainder += f64::from(y) * multiplier;
                let lines = self.line_wheel_remainder.trunc() as i32;
                self.line_wheel_remainder -= f64::from(lines);
                lines
            }
            MouseScrollDelta::PixelDelta(position) => {
                self.pixel_wheel_remainder += position.y;
                let lines = (self.pixel_wheel_remainder
                    / self.renderer.metrics().cell_height_px as f64)
                    .trunc() as i32;
                self.pixel_wheel_remainder -=
                    lines as f64 * self.renderer.metrics().cell_height_px as f64;
                lines
            }
        };
        if lines == 0 {
            return Ok(());
        }
        if let Some(math_hit) = self.math_hit() {
            let delta = -lines.saturating_mul(self.renderer.metrics().cell_height_px as i32);
            let horizontal = if self.modifiers.shift_key() { delta } else { 0 };
            let vertical = if self.modifiers.shift_key() { 0 } else { delta };
            if self
                .session
                .scroll_math_block(&math_hit.anchor, horizontal, vertical)
            {
                return self.publish_interaction_frame();
            }
        }
        let modes = self.session.terminal_modes();
        // Sticky local review: while the alternate-screen viewport is displaced into the
        // projection-local overflow (Shift+wheel entered it), the user is looking at displaced
        // pixels, not the application's live pane — forwarding wheel bytes there would scroll a
        // surface the user cannot see. Plain wheel therefore stays local in both directions;
        // scrolling back to the resting bottom (offset 0) exits and restores forwarding.
        if modes.alternate_screen && self.projection.is_scrolled() {
            return self.scroll_view(lines);
        }
        if !self.modifiers.shift_key() && modes.mouse_tracking != MouseTracking::Off {
            let Some(hit) = self.frame_hit() else {
                return Ok(());
            };
            let frame = self
                .last_presented_frame
                .as_ref()
                .context("missing frame for forwarded wheel hit")?;
            let hit = live_viewport_mouse_hit(frame, hit);
            let button = if lines > 0 {
                input::MouseProtocolButton::WheelUp
            } else {
                input::MouseProtocolButton::WheelDown
            };
            let one = input::mouse_bytes(
                modes.sgr_mouse,
                button,
                input::MouseProtocolEvent::Press,
                hit.row,
                hit.column,
                self.modifiers,
            );
            return self.send_user_input(
                &one.repeat(lines.unsigned_abs() as usize),
                "forward SGR mouse wheel to PTY",
                UserInputKind::Mouse,
            );
        }
        if modes.alternate_screen {
            // Alternate-screen wheel emulation belongs to the application. Shift is the explicit
            // local override for reviewing projection-only rows displaced above this screen.
            if self.modifiers.shift_key() {
                return self.scroll_view(lines);
            }
            if modes.alternate_scroll {
                let bytes =
                    input::alternate_scroll_bytes(lines, self.session.application_cursor_mode());
                return self.send_user_input(
                    &bytes,
                    "forward alternate-screen wheel to PTY",
                    UserInputKind::Mouse,
                );
            }
            return Ok(());
        }
        self.scroll_view(lines)
    }

    fn keyboard_input(&mut self, event: &KeyEvent) -> Result<()> {
        if event.state != ElementState::Pressed {
            return Ok(());
        }

        // A non-empty winit Preedit is the composition authority. Editing/navigation keys are
        // intentionally left to the IME here even if it also exposes a physical named key; no PTY
        // byte may escape this branch and regress M0-beta's composition isolation.
        if self.preedit.is_some() && input::is_ime_owned_key(&event.logical_key, self.modifiers) {
            return Ok(());
        }
        if input::should_copy_selection(
            &event.logical_key,
            self.modifiers,
            self.session.view_selection().is_some(),
        ) {
            if !event.repeat {
                self.copy_selection()?;
            }
            return Ok(());
        }
        if input::is_paste_shortcut(&event.logical_key, self.modifiers) {
            if !event.repeat {
                self.paste_from_clipboard()?;
            }
            return Ok(());
        }

        if !self.session.terminal_modes().alternate_screen {
            let page = self.grid.rows.get() as i32;
            match &event.logical_key {
                Key::Named(NamedKey::PageUp) if self.modifiers == ModifiersState::SHIFT => {
                    return self.scroll_view(page);
                }
                Key::Named(NamedKey::PageDown) if self.modifiers == ModifiersState::SHIFT => {
                    return self.scroll_view(-page);
                }
                Key::Named(NamedKey::Home) if self.modifiers == ModifiersState::CONTROL => {
                    self.projection.scroll_to_top();
                    return self.publish_interaction_frame();
                }
                Key::Named(NamedKey::End) if self.modifiers == ModifiersState::CONTROL => {
                    self.projection.scroll_to_bottom();
                    return self.publish_interaction_frame();
                }
                _ => {}
            }
        } else if matches!(&event.logical_key, Key::Named(NamedKey::End))
            && self.modifiers == ModifiersState::CONTROL
            && self.projection.is_scrolled()
        {
            // The application's own jump-to-bottom binding: also return the projection-local
            // overflow review to rest so both scroll layers land at the bottom together. The
            // bytes still reach the application below.
            self.projection.scroll_to_bottom();
            self.publish_interaction_frame()?;
        }

        let application_cursor_mode = self.session.application_cursor_mode();
        let Some(bytes) =
            input::keyboard_bytes(&event.logical_key, self.modifiers, application_cursor_mode)
        else {
            return Ok(());
        };
        self.send_user_input(
            &bytes,
            "write keyboard input to PTY",
            UserInputKind::Keyboard,
        )
    }

    fn paste_from_clipboard(&mut self) -> Result<()> {
        let window = Arc::clone(&self.window);
        let pty = &mut self.pty;
        if !paste_from_clipboard(
            &mut self.session,
            &mut self.projection,
            || {
                window_hwnd(&window).and_then(|hwnd| {
                    bt_platform::clipboard_text(hwnd)
                        .map_err(|error| anyhow!(error))
                        .context("read clipboard text")
                })
            },
            |chunk| {
                if let Some(pty) = pty.as_mut() {
                    pty.write(chunk).context("write clipboard paste to PTY")?;
                }
                Ok(())
            },
        )? {
            return Ok(());
        }
        self.pending_keyboard_at = Some(Instant::now());
        self.publish_frame(FrameTrigger {
            occurred_at: self.pending_keyboard_at.unwrap_or_else(Instant::now),
            source: FrameSource::Keyboard,
        })
    }

    fn ime_input(&mut self, event: Ime) -> Result<()> {
        match event {
            Ime::Enabled => {
                self.ime_active = true;
                self.ime_cursor_throttle.reset();
                self.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Expose,
                })
            }
            Ime::Preedit(text, cursor_range) => {
                // Spike 04 found all three Chinese IMEs report a collapsed caret. Target-clause
                // styling is intentionally outside M0-beta; keep only the caret's start offset.
                self.preedit = (!text.is_empty()).then_some(Preedit {
                    text,
                    cursor_byte: cursor_range.map(|(start, _)| start),
                });
                self.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Keyboard,
                })
            }
            Ime::Commit(text) => {
                self.preedit = None;
                self.return_to_live_for_input();
                self.pending_keyboard_at = Some(Instant::now());
                // IMM32 also emits this commit when focus/layout changes mid-composition. M0-beta
                // deliberately accepts it exactly like Windows Terminal: every commit reaches PTY.
                if let Some(pty) = self.pty.as_mut() {
                    pty.write(&ime_commit_bytes(&text))
                        .context("write IME UTF-8 commit to PTY")?;
                }
                self.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Keyboard,
                })
            }
            Ime::Disabled => {
                self.preedit = None;
                self.ime_active = false;
                self.ime_cursor_throttle.reset();
                self.ime_system_caret.destroy();
                self.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Expose,
                })
            }
        }
    }

    fn resize(&mut self, physical: PhysicalSize<u32>) -> Result<()> {
        if physical.width == 0 || physical.height == 0 {
            return Ok(());
        }
        // The Resized payload is already in physical pixels. Synchronize presentation before any
        // DPI reconciliation can publish a frame, then reconcile once more against inner_size().
        self.renderer
            .resize(physical.width, physical.height)
            .context("synchronize renderer swapchain with resized physical client")?;
        self.reconcile_authoritative_dpi("resized")?;
        let requested_physical = self.window.inner_size();
        if requested_physical.width == 0 || requested_physical.height == 0 {
            return Ok(());
        }
        let presentation = self.renderer.presentation_geometry();
        trace_surface_size_clamp(
            self.trace_resize,
            "BT_RESIZE_TRACE",
            requested_physical,
            presentation,
        );
        let render_physical = presentation_physical_size(presentation);
        let resize_trigger = FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Resize,
        };
        let next_grid = self
            .renderer
            .metrics()
            .grid_for_pixels(render_physical.width, render_physical.height);
        let observed_at = Instant::now();
        coalesce_pty_resize(
            &mut self.pending_pty_resize,
            next_grid,
            render_physical,
            observed_at,
        );
        if next_grid != self.grid {
            self.session
                .resize(
                    nonzero_u32(next_grid.columns.get()),
                    nonzero_u32(next_grid.rows.get()),
                )
                .context("resize terminal actor")?;
            self.grid = next_grid;
        }
        self.sync_math_layout_key();
        self.pending_resize_present = Some(next_grid);
        self.publish_frame(resize_trigger)?;
        // Windows dispatches Resized from its modal move/size loop. `Renderer::resize` only records
        // the requested swapchain geometry; `present` prepares this newly projected frame first,
        // then performs ResizeBuffers immediately before acquire/submit. Thus the handler exposes
        // no intermediate "new surface + old grid" frame. Until this callback completes, DWM may
        // scale the previous complete back buffer as one image, which is the all-frame fallback.
        self.redraw()
    }

    fn scale_factor_changed(&mut self) -> Result<()> {
        self.reconcile_authoritative_dpi("scale-factor-changed")?;
        self.resize(self.window.inner_size())
    }

    fn reconcile_authoritative_dpi(&mut self, stage: &'static str) -> Result<bool> {
        let physical = self.window.inner_size();
        if physical.width > 0 && physical.height > 0 {
            self.renderer
                .resize(physical.width, physical.height)
                .context("reconcile swapchain with physical client size")?;
            ensure_swapchain_matches_inner(&self.renderer, physical)?;
        }
        let render_physical = presentation_physical_size(self.renderer.presentation_geometry());
        let snapshot = dpi_snapshot(&self.window)?;
        log_dpi_snapshot(
            stage,
            snapshot,
            Some(self.renderer.metrics().scale_factor),
            self.renderer.presentation_geometry(),
            physical,
        );
        self.refresh_startup_trace_title(snapshot.authoritative_scale);
        if scale_factors_match(
            self.renderer.metrics().scale_factor,
            snapshot.authoritative_scale,
        ) {
            return Ok(false);
        }

        self.apply_scale_factor(snapshot.authoritative_scale)?;
        if physical.width > 0 && physical.height > 0 {
            let next_grid = self
                .renderer
                .metrics()
                .grid_for_pixels(render_physical.width, render_physical.height);
            if next_grid != self.grid {
                self.session
                    .resize(
                        nonzero_u32(next_grid.columns.get()),
                        nonzero_u32(next_grid.rows.get()),
                    )
                    .context("rebuild terminal grid after authoritative DPI correction")?;
                self.grid = next_grid;
                coalesce_pty_resize(
                    &mut self.pending_pty_resize,
                    next_grid,
                    render_physical,
                    Instant::now(),
                );
            }
        }
        self.sync_math_layout_key();
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(true)
    }

    fn refresh_startup_trace_title(&self, scale_factor: f64) {
        if !self.trace_startup {
            return;
        }
        let title = match (self.background_visible, self.first_text_visible) {
            (Some(background), Some(text)) => startup_trace_title(background, text, scale_factor),
            _ => startup_scale_title(scale_factor),
        };
        self.window.set_title(&title);
    }

    fn sync_math_layout_key(&mut self) {
        // Future runtime theme switching has one required hook: update renderer theme colors, then
        // call this method. `LayoutKey` contains `theme_rev`, so a theme change must invalidate old
        // textures; the revision enters both the worker gate and GPU texture identity. The session
        // keeps same-source old pixels only while the replacement is pending.
        self.session.set_layout_key(LayoutKey {
            width_cells: nonzero_u32(self.grid.columns.get()),
            dpi_milli: self.renderer.metrics().dpi_milli(),
            font_rev: 1,
            theme_rev: theme_revision(),
        });
    }

    fn apply_scale_factor(&mut self, scale_factor: f64) -> Result<()> {
        let metrics = self
            .renderer
            .update_scale_factor(scale_factor)
            .context("remeasure terminal font at new DPI")?;
        ensure_metrics_match_authoritative_scale(metrics.scale_factor, scale_factor)?;
        self.session
            .set_cell_height_subpixels(metrics.cell_height_subpixels());
        self.session
            .set_cell_width_subpixels(cell_width_subpixels(metrics));
        self.session
            .set_ascii_baseline_subpixels(metrics.ascii_baseline_subpixels());
        Ok(())
    }

    fn redraw(&mut self) -> Result<()> {
        let Some((frame, trigger)) = self.pending_frames.take() else {
            return Ok(());
        };
        if let Some(expected) = self.pending_resize_present {
            ensure!(
                frame_matches_grid(&frame, expected),
                "resize presentation requires the newly projected grid: expected {}x{}, got {}x{}",
                expected.columns,
                expected.rows,
                frame.columns,
                frame.rows
            );
        }
        let has_text = frame.cells.iter().any(|cell| !cell.text.trim().is_empty());
        match self
            .renderer
            .present(&frame, trigger)
            .context("render terminal frame")?
        {
            PresentOutcome::Presented(receipt) => {
                if self.window_shown && !self.first_visible_present_dpi_checked {
                    self.first_visible_present_dpi_checked = true;
                    self.reconcile_authoritative_dpi("first-present")?;
                }
                let latency = receipt.latency();
                if self.trace_startup
                    && matches!(trigger.source, FrameSource::Resize)
                    && let Ok(latency) = latency
                {
                    eprintln!(
                        "BT_RESIZE present={}us columns={} rows={}",
                        latency.event_to_present_call.as_micros(),
                        frame.columns,
                        frame.rows
                    );
                }
                if has_text && !self.first_text_presented {
                    self.first_text_presented = true;
                    if self.trace_startup {
                        let text_visible = self.startup_started.elapsed();
                        self.first_text_visible = Some(text_visible);
                        eprintln!(
                            "BT_STARTUP first_text_present={}ms",
                            text_visible.as_millis()
                        );
                        if self.background_visible.is_some() {
                            self.refresh_startup_trace_title(self.renderer.metrics().scale_factor);
                        }
                    }
                }
                self.last_presented_frame = Some(frame);
                self.pending_resize_present = None;
            }
            PresentOutcome::Skipped | PresentOutcome::Reconfigure => {
                self.pending_frames
                    .publish(frame, trigger)
                    .context("reject non-rectangular frame during redraw retry")?;
                self.window.request_redraw();
            }
        }
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        self.ime_system_caret.destroy();
        if let Some(pty) = self.pty.as_mut() {
            pty.shutdown().context("shut down child process")?;
        }
        Ok(())
    }
}

struct BetterTerminalApp {
    runtime: Option<Runtime>,
    proxy: EventLoopProxy<AppEvent>,
    startup_started: Instant,
}

impl BetterTerminalApp {
    fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self {
            runtime: None,
            proxy,
            startup_started: Instant::now(),
        }
    }
}

impl BetterTerminalApp {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        eprintln!("BetterTerminal stopped: {error:#}");
        if let Some(runtime) = self.runtime.as_mut()
            && let Err(shutdown_error) = runtime.shutdown()
        {
            eprintln!("child shutdown also failed: {shutdown_error:#}");
        }
        self.runtime = None;
        event_loop.exit();
    }
}

impl ApplicationHandler<AppEvent> for BetterTerminalApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.runtime.is_some() {
            return;
        }
        match Runtime::create(event_loop, self.proxy.clone(), self.startup_started) {
            Ok(runtime) => {
                self.runtime = Some(runtime);
                // Output can already be buffered after the long GPU initialization path. Drain
                // once after installing Runtime instead of waiting for another event-loop turn.
                if let Some(runtime) = self.runtime.as_mut()
                    && let Err(error) = runtime.drain_pty()
                {
                    self.fail(event_loop, error);
                    return;
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + STARTUP_PTY_POLL_INTERVAL,
                ));
            }
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::PtyOutput => {
                if let Some(runtime) = self.runtime.as_mut()
                    && let Err(error) = runtime.drain_pty()
                {
                    self.fail(event_loop, error);
                }
            }
            AppEvent::MathReady => {
                if let Some(runtime) = self.runtime.as_mut()
                    && let Err(error) = runtime.apply_math_results()
                {
                    self.fail(event_loop, error);
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        if runtime.window.id() != window_id {
            return;
        }
        let result = match event {
            WindowEvent::CloseRequested => {
                let result = runtime.shutdown();
                self.runtime = None;
                event_loop.exit();
                result
            }
            WindowEvent::KeyboardInput { event, .. } => runtime.keyboard_input(&event),
            WindowEvent::Ime(event) => runtime.ime_input(event),
            WindowEvent::ModifiersChanged(modifiers) => {
                runtime.modifiers = modifiers.state();
                Ok(())
            }
            WindowEvent::CursorMoved { position, .. } => runtime.pointer_moved(position),
            WindowEvent::CursorLeft { .. } => runtime.pointer_left(),
            WindowEvent::MouseInput { state, button, .. } => runtime.mouse_input(state, button),
            WindowEvent::MouseWheel { delta, .. } => runtime.mouse_wheel(delta),
            WindowEvent::Resized(size) => runtime.resize(size),
            WindowEvent::ScaleFactorChanged { .. } => runtime.scale_factor_changed(),
            WindowEvent::RedrawRequested => runtime.redraw(),
            WindowEvent::Focused(false) => {
                // Do not cancel or synthesize anything: IMM32 may synchronously deliver a partial
                // Commit during this transition, and the product decision is to accept it.
                runtime.ime_active = false;
                runtime.ime_cursor_throttle.reset();
                runtime.ime_system_caret.destroy();
                runtime.renderer.set_window_focused(false);
                runtime.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Expose,
                })
            }
            WindowEvent::Focused(true) => {
                runtime.renderer.set_window_focused(true);
                runtime.publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Expose,
                })
            }
            WindowEvent::Occluded(false) => runtime.publish_frame(FrameTrigger {
                occurred_at: Instant::now(),
                source: FrameSource::Expose,
            }),
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.fail(event_loop, error);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        runtime.apply_math_context_menu_result();
        if let Err(error) = runtime.drain_pty() {
            self.fail(event_loop, error);
            return;
        }
        let now = Instant::now();
        if let Err(error) = runtime.finish_synchronized_update_if_due(now) {
            self.fail(event_loop, error);
            return;
        }
        if let Err(error) = runtime.flush_pending_pty_resize(now) {
            self.fail(event_loop, error);
            return;
        }
        runtime.flush_ime_cursor_area(now);
        if let Err(error) = runtime.finish_resize_if_quiescent(now) {
            self.fail(event_loop, error);
            return;
        }
        if let Err(error) = runtime.advance_live_math_if_due(now) {
            self.fail(event_loop, error);
            return;
        }
        if let Err(error) = runtime.activate_hyperlink_hover_if_due(now) {
            self.fail(event_loop, error);
            return;
        }
        if let Err(error) = runtime.clear_math_hover_if_due(now) {
            self.fail(event_loop, error);
            return;
        }
        let startup_deadline =
            startup_poll_delay(runtime.first_text_presented).map(|delay| now + delay);
        let wake_deadline = [
            startup_deadline,
            runtime.ime_cursor_throttle.deadline(),
            runtime.pending_pty_resize.map(|pending| pending.deadline),
            runtime.session.resize_finish_deadline(),
            runtime.session.synchronized_update_deadline(),
            runtime.session.live_stability_deadline(),
            runtime.hyperlink_hover.show_at,
            runtime.math_hover_clear_at,
        ]
        .into_iter()
        .flatten()
        .min();
        event_loop
            .set_control_flow(wake_deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
        let Some(pty) = runtime.pty.as_mut() else {
            return;
        };
        match pty.try_wait() {
            Ok(Some(_)) => {
                if let Err(error) = runtime.drain_pty().and_then(|_| runtime.shutdown()) {
                    eprintln!("shell exit cleanup failed: {error:#}");
                }
                self.runtime = None;
                event_loop.exit();
            }
            Ok(None) => {}
            Err(error) => self.fail(event_loop, error.into()),
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(runtime) = self.runtime.as_mut()
            && let Err(error) = runtime.shutdown()
        {
            eprintln!("child shutdown failed: {error:#}");
        }
        self.runtime = None;
    }
}

fn ime_commit_bytes(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

fn copy_selection(
    session: &mut DualPlaneSession,
    projection: &mut ViewportProjection,
    write: impl FnOnce(&str) -> Result<()>,
) -> bool {
    if !write_selection_text(session, false, write) {
        return false;
    }
    session.set_view_selection(None);
    projection.set_selection(None);
    true
}

fn write_selection_text(
    session: &DualPlaneSession,
    ignore_empty: bool,
    write: impl FnOnce(&str) -> Result<()>,
) -> bool {
    let Some(text) = session.selection_text() else {
        return false;
    };
    if ignore_empty && text.is_empty() {
        return false;
    }
    if !recoverable_clipboard_write(write(&text), "copy") {
        return false;
    }
    true
}

fn write_terminal_clipboard_text(window: &Window, text: &str) -> Result<()> {
    window_hwnd(window).and_then(|hwnd| {
        bt_platform::set_clipboard_text(hwnd, text)
            .map_err(|error| anyhow!(error))
            .context("write terminal selection to clipboard")
    })
}

fn recoverable_clipboard_write(result: Result<()>, action: &str) -> bool {
    match result {
        Ok(()) => true,
        Err(error) => {
            eprintln!("clipboard is temporarily unavailable; {action} ignored: {error:#}");
            false
        }
    }
}

fn paste_from_clipboard(
    session: &mut DualPlaneSession,
    projection: &mut ViewportProjection,
    read: impl FnOnce() -> Result<String>,
    mut write: impl FnMut(&[u8]) -> Result<()>,
) -> Result<bool> {
    let Some(text) = recoverable_clipboard_read(read()) else {
        return Ok(false);
    };
    let bytes = input::paste_bytes(&text, session.bracketed_paste_mode());
    session.set_view_selection(None);
    projection.set_selection(None);
    projection.scroll_to_bottom();
    // Keep paste on the sole synchronous PTY writer. Fixed-size writes let ConPTY's existing
    // write_all/flush path apply backpressure instead of one unbounded call.
    for chunk in bytes.chunks(input::PASTE_WRITE_CHUNK_BYTES) {
        write(chunk)?;
    }
    Ok(true)
}

fn recoverable_clipboard_read(result: Result<String>) -> Option<String> {
    match result {
        Ok(text) => Some(text),
        Err(error) => {
            eprintln!("clipboard does not contain readable text; paste ignored: {error:#}");
            None
        }
    }
}

fn recoverable_wheel_scroll_amount(
    result: std::result::Result<bt_platform::WheelScrollAmount, String>,
) -> bt_platform::WheelScrollAmount {
    match result {
        Ok(amount) => amount,
        Err(error) => {
            const FALLBACK_WHEEL_LINES: u32 = 3;
            eprintln!(
                "system wheel scroll setting unavailable; using {FALLBACK_WHEEL_LINES} lines: {error}"
            );
            bt_platform::WheelScrollAmount::Lines(FALLBACK_WHEEL_LINES)
        }
    }
}

fn protocol_mouse_button(button: MouseButton) -> Option<input::MouseProtocolButton> {
    match button {
        MouseButton::Left => Some(input::MouseProtocolButton::Left),
        MouseButton::Middle => Some(input::MouseProtocolButton::Middle),
        MouseButton::Right => Some(input::MouseProtocolButton::Right),
        MouseButton::Back | MouseButton::Forward | MouseButton::Other(_) => None,
    }
}

fn startup_poll_delay(first_text_presented: bool) -> Option<std::time::Duration> {
    (!first_text_presented).then_some(STARTUP_PTY_POLL_INTERVAL)
}

fn startup_scale_title(scale_factor: f64) -> String {
    format!("{WINDOW_TITLE} · {}x", display_scale_factor(scale_factor))
}

fn startup_trace_title(
    background_visible: Duration,
    first_text_visible: Duration,
    scale_factor: f64,
) -> String {
    format!(
        "{WINDOW_TITLE} — bg {}ms · text {}ms · {}x",
        background_visible.as_millis(),
        first_text_visible.as_millis(),
        display_scale_factor(scale_factor),
    )
}

fn display_scale_factor(scale_factor: f64) -> String {
    format!("{scale_factor:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn scale_factors_match(left: f64, right: f64) -> bool {
    (left - right).abs() <= f64::EPSILON
}

fn ensure_metrics_match_authoritative_scale(
    metrics_scale: f64,
    authoritative_scale: f64,
) -> Result<()> {
    ensure!(
        scale_factors_match(metrics_scale, authoritative_scale),
        "terminal metrics scale factor {metrics_scale} does not match authoritative Win32 scale factor {authoritative_scale}"
    );
    Ok(())
}

fn dpi_snapshot(window: &Window) -> Result<DpiSnapshot> {
    let hwnd = window_hwnd(window)?;
    let win32_dpi = bt_platform::get_dpi_for_window(hwnd)
        .map_err(|error| anyhow!(error))
        .context("query authoritative window DPI with GetDpiForWindow")?;
    let rect = bt_platform::get_window_rect(hwnd)
        .map_err(|error| anyhow!(error))
        .context("query native window rectangle for DPI diagnostics")?;
    Ok(DpiSnapshot {
        winit_scale: window.scale_factor(),
        win32_dpi,
        authoritative_scale: f64::from(win32_dpi) / WIN32_DEFAULT_DPI,
        rect,
    })
}

fn log_dpi_snapshot(
    stage: &str,
    snapshot: DpiSnapshot,
    metrics_scale: Option<f64>,
    presentation: bt_render::PresentationGeometry,
    inner_size: PhysicalSize<u32>,
) {
    let metrics_scale = metrics_scale.map_or_else(|| "n/a".to_owned(), display_scale_factor);
    eprintln!(
        "BT_DPI stage={stage} winit_scale={} win32_dpi={} authoritative_scale={} metrics_scale={} rect={},{},{},{} swapchain_size={}x{} inner_size={}x{}",
        display_scale_factor(snapshot.winit_scale),
        snapshot.win32_dpi,
        display_scale_factor(snapshot.authoritative_scale),
        metrics_scale,
        snapshot.rect.left,
        snapshot.rect.top,
        snapshot.rect.right,
        snapshot.rect.bottom,
        presentation.swapchain_size.0,
        presentation.swapchain_size.1,
        inner_size.width,
        inner_size.height,
    );
}

fn ensure_swapchain_matches_inner(
    renderer: &Renderer,
    inner_size: PhysicalSize<u32>,
) -> Result<()> {
    let presentation = renderer.presentation_geometry();
    let swapchain_size = presentation.swapchain_size;
    ensure!(
        swapchain_size_matches_inner(
            swapchain_size,
            inner_size,
            presentation.max_texture_dimension_2d,
        ),
        "swapchain size {}x{} does not match clamped winit physical inner size {}x{} (device limit {})",
        swapchain_size.0,
        swapchain_size.1,
        inner_size.width,
        inner_size.height,
        presentation.max_texture_dimension_2d,
    );
    Ok(())
}

fn swapchain_size_matches_inner(
    swapchain_size: (u32, u32),
    inner_size: PhysicalSize<u32>,
    max_texture_dimension_2d: u32,
) -> bool {
    let limit = max_texture_dimension_2d.max(1);
    swapchain_size
        == (
            inner_size.width.max(1).min(limit),
            inner_size.height.max(1).min(limit),
        )
}

fn presentation_physical_size(presentation: bt_render::PresentationGeometry) -> PhysicalSize<u32> {
    PhysicalSize::new(presentation.swapchain_size.0, presentation.swapchain_size.1)
}

fn trace_surface_size_clamp(
    enabled: bool,
    prefix: &str,
    requested: PhysicalSize<u32>,
    presentation: bt_render::PresentationGeometry,
) {
    if !enabled
        || (requested.width <= presentation.swapchain_size.0
            && requested.height <= presentation.swapchain_size.1)
    {
        return;
    }
    eprintln!(
        "{prefix} surface_size_clamped requested={}x{} configured={}x{} max_texture_dimension_2d={}",
        requested.width,
        requested.height,
        presentation.swapchain_size.0,
        presentation.swapchain_size.1,
        presentation.max_texture_dimension_2d,
    );
}

fn install_theme_class_background(window: &Window) -> Result<()> {
    bt_platform::install_window_class_background(window_hwnd(window)?, background_rgb())
        .map_err(|error| anyhow!(error))
        .context("install theme-colored winit class background brush")
}

fn window_hwnd(window: &Window) -> Result<std::num::NonZeroIsize> {
    let handle = window.window_handle().context("get Win32 window handle")?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(anyhow!("bt-app requires a Win32 window handle"));
    };
    Ok(handle.hwnd)
}

fn cell_width_subpixels(metrics: bt_render::CellMetrics) -> NonZeroI64 {
    let value = (metrics.cell_width_px * bt_viewport::SUBPIXELS_PER_PX as f32).round() as i64;
    NonZeroI64::new(value.max(1)).expect("cell width is clamped above zero")
}

fn nonzero_u32(value: u16) -> NonZeroU32 {
    NonZeroU32::new(u32::from(value)).expect("grid dimensions originate from NonZeroU16")
}

fn frame_matches_grid(frame: &ViewportFrame, grid: GridSize) -> bool {
    frame.columns.get() == u32::from(grid.columns.get())
        && frame.rows.get() == u32::from(grid.rows.get())
}

fn presentation_equivalent(previous: &ViewportFrame, next: &ViewportFrame) -> bool {
    previous.columns == next.columns
        && previous.rows == next.rows
        && previous.cells == next.cells
        && previous.cursor == next.cursor
        && previous.cell_anchors == next.cell_anchors
        && previous.row_map == next.row_map
        && previous.selection_spans == next.selection_spans
        && previous.math_blocks == next.math_blocks
        && previous.status_text == next.status_text
        && previous.viewport_origin == next.viewport_origin
        && previous.scroll_offset_rows == next.scroll_offset_rows
        && previous.layout_key == next.layout_key
}

fn pty_frame_is_unchanged(
    pending: Option<&ViewportFrame>,
    last_presented: Option<&ViewportFrame>,
    next: &ViewportFrame,
) -> bool {
    pending
        .or(last_presented)
        .is_some_and(|previous| presentation_equivalent(previous, next))
}

fn pty_size(grid: GridSize, physical: PhysicalSize<u32>) -> PtySize {
    PtySize {
        columns: grid.columns,
        rows: grid.rows,
        pixel_width: physical.width.min(u32::from(u16::MAX)) as u16,
        pixel_height: physical.height.min(u32::from(u16::MAX)) as u16,
    }
}

fn load_probe_input() -> Result<Option<Vec<u8>>> {
    let Some(path) = std::env::var_os("BT_PROBE_INPUT") else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    std::fs::read(&path)
        .with_context(|| format!("read BT_PROBE_INPUT {}", path.display()))
        .map(Some)
}

fn install_panic_log_hook() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("unnamed");
        let report = format!(
            "unix_ms={timestamp_ms} thread={thread_name}\npanic: {info}\nbacktrace:\n{}\n",
            Backtrace::force_capture()
        );
        let path = panic_log_path();
        if let Err(error) = append_panic_report(&path, &report) {
            eprintln!("failed to write panic report {}: {error}", path.display());
        }
        previous(info);
    }));
}

fn panic_log_path() -> PathBuf {
    std::env::temp_dir().join(PANIC_LOG_FILENAME)
}

fn append_panic_report(path: &std::path::Path, report: &str) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{report}")
}

fn main() -> Result<()> {
    install_panic_log_hook();
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .context("create winit event loop")?;
    let mut application = BetterTerminalApp::new(event_loop.create_proxy());
    event_loop
        .run_app(&mut application)
        .map_err(|error| anyhow!(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use winit::keyboard::{Key, NamedKey};

    fn hyperlink_hit(uri: &str) -> HyperlinkHit {
        HyperlinkHit {
            uri: uri.to_owned(),
            start: bt_doc::ContentAnchor::Live {
                screen: bt_doc::ScreenId::Primary,
                point: bt_doc::GridPoint { row: 1, column: 2 },
                bias: Bias::Before,
                generation: bt_doc::GridGeneration(1),
            },
            end: bt_doc::ContentAnchor::Live {
                screen: bt_doc::ScreenId::Primary,
                point: bt_doc::GridPoint { row: 1, column: 5 },
                bias: Bias::After,
                generation: bt_doc::GridGeneration(1),
            },
        }
    }

    fn local_selection_route(mode: SelectionDragMode) -> MouseRoute {
        let hit = hyperlink_hit("https://example.test");
        MouseRoute::Local(SelectionDrag {
            mode,
            origin_row: 1,
            origin_column: 2,
            origin: ViewSelection {
                start: hit.start,
                end: hit.end,
            },
            hyperlink: None,
            open_hyperlink_on_release: false,
            local_image_path: None,
            open_local_image_on_release: false,
        })
    }

    #[test]
    fn selection_release_copy_policy_covers_drag_word_and_line_but_not_click_or_forwarding() {
        for mode in [
            SelectionDragMode::Linear,
            SelectionDragMode::Word,
            SelectionDragMode::Line,
        ] {
            let route = local_selection_route(mode);
            assert!(should_copy_on_select_release(Some(&route), false));
        }

        let click = local_selection_route(SelectionDragMode::Linear);
        assert!(!should_copy_on_select_release(Some(&click), true));
        let forwarded = MouseRoute::Forward(input::MouseProtocolButton::Left);
        assert!(!should_copy_on_select_release(Some(&forwarded), false));
        assert!(!should_copy_on_select_release(None, false));
    }

    #[test]
    fn hyperlink_activation_requires_ctrl_and_click_without_drag() {
        assert_eq!(
            hyperlink_activation(true, true, "https://example.test/path"),
            HyperlinkActivation::Open
        );
        assert_eq!(
            hyperlink_activation(true, true, "HTTP://localhost:3000"),
            HyperlinkActivation::Open
        );
        assert_eq!(
            hyperlink_activation(false, true, "https://example.test"),
            HyperlinkActivation::None
        );
        assert_eq!(
            hyperlink_activation(true, false, "https://example.test"),
            HyperlinkActivation::None
        );
    }

    #[test]
    fn local_image_activation_requires_ctrl_click_and_worker_success_capability() {
        let verified = std::path::Path::new(r"C:\tmp\decoded.png");
        assert!(local_image_activation(true, true, Some(verified)));
        assert!(!local_image_activation(false, true, Some(verified)));
        assert!(!local_image_activation(true, false, Some(verified)));
        assert!(!local_image_activation(true, true, None));
    }

    #[test]
    fn hyperlink_activation_blocks_every_non_http_scheme_and_unsafe_target() {
        for uri in [
            "file:///C:/secret.txt",
            "mailto:person@example.test",
            "custom://payload",
            "https://example.test/\nspoof",
        ] {
            assert_eq!(
                hyperlink_activation(true, true, uri),
                HyperlinkActivation::Blocked,
                "{uri:?}"
            );
        }
    }

    #[test]
    fn hyperlink_hover_delay_and_departure_are_event_driven() {
        let start = Instant::now();
        let link = hyperlink_hit("file:///actual-target");
        let mut hover = HyperlinkHover::default();

        assert!(!hover.observe(Some(link.clone()), start));
        assert!(!hover.activate_if_due(start + Duration::from_millis(399)));
        assert!(hover.activate_if_due(start + Duration::from_millis(400)));
        assert_eq!(
            hover.status_text(80).as_deref(),
            Some("file:///actual-target")
        );
        assert!(hover.observe(None, start + Duration::from_millis(401)));
        assert!(hover.active.is_none());
        assert!(hover.show_at.is_none());

        hover.show_blocked(link);
        assert_eq!(
            hover.status_text(80).as_deref(),
            Some("file:///actual-target · blocked")
        );
        assert_eq!(
            hover.status_text(20).as_deref(),
            Some("file:///a… · blocked"),
            "narrow chrome keeps the real target prefix and the blocked verdict visible"
        );
    }

    #[test]
    fn osc_8_display_text_hits_the_real_target_uri() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(24).unwrap(), NonZeroU32::new(2).unwrap());
        session
            .feed(b"\x1b]8;;https://actual.example/login\x1b\\trusted label\x1b]8;;\x1b\\")
            .unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let mut frame = session.viewport_frame(&mut projection).unwrap();

        let hit = frame.hyperlink_at(0, 4).unwrap();
        assert_eq!(hit.uri, "https://actual.example/login");
        assert!(frame.underline_hyperlink(&hit));
        assert!(frame.cells[..13].iter().all(|cell| {
            cell.style
                .flags
                .contains(bt_transcript::CellFlags::UNDERLINE)
        }));
        assert!(
            !frame.cells[13]
                .style
                .flags
                .contains(bt_transcript::CellFlags::UNDERLINE)
        );
    }

    struct PtyPresentationHarness {
        session: DualPlaneSession,
        projection: ViewportProjection,
        pending: LatestFrameSlot,
        last_presented: Option<ViewportFrame>,
        publications: usize,
    }

    impl PtyPresentationHarness {
        fn new(columns: u32, rows: u32) -> Self {
            let session = DualPlaneSession::new(
                NonZeroU32::new(columns).unwrap(),
                NonZeroU32::new(rows).unwrap(),
            );
            let projection = session.new_projection(session.layout_key());
            Self {
                session,
                projection,
                pending: LatestFrameSlot::default(),
                last_presented: None,
                publications: 0,
            }
        }

        fn feed_drain(&mut self, bytes: &[u8]) -> bool {
            self.session.feed(bytes).unwrap();
            self.publish_pty_frame()
        }

        fn finish_synchronized_update(&mut self) -> (bool, bool) {
            let finished = self
                .session
                .finish_synchronized_update(Instant::now())
                .unwrap();
            let published = finished && self.publish_pty_frame();
            (finished, published)
        }

        fn publish_pty_frame(&mut self) -> bool {
            self.session.refresh_projection(&mut self.projection);
            let frame = self.session.viewport_frame(&mut self.projection).unwrap();
            // Mirror publish_frame_inner's combined review/exact-source presentation hold.
            if self.projection.presentation_hold() && self.last_presented.is_some() {
                return false;
            }
            if pty_frame_is_unchanged(
                self.pending.pending_frame(),
                self.last_presented.as_ref(),
                &frame,
            ) {
                return false;
            }
            self.pending
                .publish(
                    frame,
                    FrameTrigger {
                        occurred_at: Instant::now(),
                        source: FrameSource::PtyOutput,
                    },
                )
                .unwrap();
            self.publications += 1;
            true
        }

        fn present_pending(&mut self) -> bool {
            let Some((frame, _)) = self.pending.take() else {
                return false;
            };
            self.last_presented = Some(frame);
            true
        }
    }

    fn frame_row_text(frame: &ViewportFrame, row: usize) -> String {
        let columns = frame.columns.get() as usize;
        frame.cells[row * columns..(row + 1) * columns]
            .iter()
            .map(|cell| cell.text.as_str())
            .collect()
    }

    #[test]
    fn a_resize_reflow_holds_the_presented_frame_until_the_reprint_re_anchors() {
        let start = Instant::now();
        let mut harness = PtyPresentationHarness::new(40, 10);
        let mut lines = Vec::new();
        for index in 0..60 {
            lines.extend_from_slice(format!("line-{index:03}\r\n").as_bytes());
        }
        harness.session.feed(&lines).unwrap();
        assert!(harness.publish_pty_frame());
        harness.present_pending();

        // Enter review.
        harness.projection.scroll_by_rows(20);
        assert!(harness.publish_pty_frame());
        harness.present_pending();
        assert_eq!(
            harness.last_presented.as_ref().unwrap().scroll_offset_rows,
            20
        );
        let publications_before = harness.publications;

        // A resize opens the transaction and Codex clears scrollback: the review anchor vanishes
        // and history is transiently empty. The interim frame is bottom-snapped, but presentation
        // must hold the last frame rather than flash to the bottom.
        harness
            .session
            .resize_at(
                NonZeroU32::new(40).unwrap(),
                NonZeroU32::new(12).unwrap(),
                start,
            )
            .unwrap();
        harness.session.mark_pty_resize_requested_at(
            NonZeroU32::new(40).unwrap(),
            NonZeroU32::new(12).unwrap(),
            start + Duration::from_millis(10),
        );
        harness
            .session
            .feed_at(b"\x1b[2J\x1b[3J\x1b[H", start + Duration::from_millis(20))
            .unwrap();
        assert!(
            !harness.publish_pty_frame(),
            "the hold skips publishing the bottom-snapped interim frame"
        );
        assert!(
            !harness.present_pending(),
            "nothing was published to present"
        );
        assert_eq!(
            harness.publications, publications_before,
            "no new publication during the hold"
        );
        assert_eq!(
            harness.last_presented.as_ref().unwrap().scroll_offset_rows,
            20,
            "the screen still shows the reviewing frame, not the bottom"
        );

        // The reprint refills history and the transaction quiesces: publication resumes at the
        // restored review position — a direct hand-off with no bottom frame ever presented.
        harness
            .session
            .feed_at(&lines, start + Duration::from_millis(30))
            .unwrap();
        assert!(
            !harness.publish_pty_frame(),
            "still held while the reprint is staged inside the transaction"
        );
        assert!(
            harness
                .session
                .finish_resize_if_quiescent(start + Duration::from_millis(280))
                .unwrap()
        );
        assert!(
            harness.publish_pty_frame(),
            "the hold releases once the transaction closes and the displacement re-anchors"
        );
        harness.present_pending();
        assert_eq!(
            harness.last_presented.as_ref().unwrap().scroll_offset_rows,
            20,
            "presentation resumes exactly at the restored review displacement"
        );
    }

    #[test]
    fn a_late_zoom_reprint_keeps_the_last_formula_frame_until_exact_source_reanchors() {
        let start = Instant::now();
        let mut harness = PtyPresentationHarness::new(40, 24);
        harness
            .session
            .feed_at(b"intro\r\n$$x$$\r\nbarrier", start)
            .unwrap();
        assert_eq!(
            harness
                .session
                .advance_live_stability(start + bt_term::LIVE_MATH_STABLE_INTERVAL),
            1
        );
        let mut initial_task = harness.session.take_live_worker_task().unwrap();
        let initial_raster =
            render_live_detection_task(&MathEngine::new(), &mut initial_task, foreground_rgb())
                .expect("initial formula rasterizes");
        assert!(
            harness
                .session
                .complete_live_worker_result(initial_task, Ok(initial_raster))
        );
        assert!(harness.publish_pty_frame());
        assert!(harness.present_pending());
        assert_eq!(
            harness.last_presented.as_ref().unwrap().math_blocks.len(),
            1
        );

        // Match reconcile_authoritative_dpi: metrics, grid resize, then the new-DPI layout key.
        let zoom_at = start + Duration::from_millis(210);
        harness.session.set_cell_height_subpixels(
            NonZeroI64::new(14 * bt_viewport::SUBPIXELS_PER_PX).unwrap(),
        );
        harness
            .session
            .set_cell_width_subpixels(NonZeroI64::new(7 * bt_viewport::SUBPIXELS_PER_PX).unwrap());
        harness.session.set_ascii_baseline_subpixels(
            NonZeroI64::new(11 * bt_viewport::SUBPIXELS_PER_PX).unwrap(),
        );
        harness
            .session
            .resize_at(
                NonZeroU32::new(52).unwrap(),
                NonZeroU32::new(32).unwrap(),
                zoom_at,
            )
            .unwrap();
        harness.session.mark_pty_resize_requested_at(
            NonZeroU32::new(52).unwrap(),
            NonZeroU32::new(32).unwrap(),
            zoom_at,
        );
        harness.session.set_layout_key(bt_doc::LayoutKey {
            width_cells: NonZeroU32::new(52).unwrap(),
            dpi_milli: NonZeroU32::new(800).unwrap(),
            font_rev: 1,
            theme_rev: harness.session.layout_key().theme_rev,
        });
        let delayed_relayout = harness.session.take_live_worker_task().unwrap();
        assert!(harness.publish_pty_frame());
        assert!(harness.present_pending());
        assert!(
            harness
                .session
                .finish_resize_if_quiescent(zoom_at + Duration::from_millis(300))
                .unwrap()
        );

        let publications_before_gap = harness.publications;
        harness
            .session
            .feed_at(
                b"\x1b[2J\x1b[H\x1b[3Jintro\r\n$",
                zoom_at + Duration::from_millis(310),
            )
            .unwrap();
        assert!(
            !harness.publish_pty_frame(),
            "publish_frame_inner must skip the diagnosed incomplete reprint frame"
        );
        assert!(!harness.present_pending());
        assert_eq!(harness.publications, publications_before_gap);
        assert_eq!(
            harness.last_presented.as_ref().unwrap().math_blocks.len(),
            1,
            "the swapchain remains on the last complete formula frame"
        );

        harness
            .session
            .feed_at(b"$x$$\r\nbarrier", zoom_at + Duration::from_millis(324))
            .unwrap();
        let reanchor_published = harness.publish_pty_frame();
        assert!(
            !harness.projection.presentation_hold(),
            "exact-source re-anchor releases the hold immediately"
        );
        if reanchor_published {
            assert!(harness.present_pending());
        }
        assert_eq!(
            harness.last_presented.as_ref().unwrap().math_blocks.len(),
            1
        );

        // The re-anchored stale frame may be content-identical to last_presented and therefore need
        // no publication. Either way, no incomplete grid frame entered the presentation slot.
        drop(delayed_relayout);
    }

    #[test]
    fn keyboard_mapping_is_ascii_only_and_preserves_terminal_controls() {
        assert_eq!(
            input::keyboard_bytes(
                &Key::Character("hello".into()),
                ModifiersState::empty(),
                false
            ),
            Some(b"hello".to_vec())
        );
        assert_eq!(
            input::keyboard_bytes(&Key::Named(NamedKey::Enter), ModifiersState::empty(), false),
            Some(vec![b'\r'])
        );
        assert_eq!(
            input::keyboard_bytes(
                &Key::Named(NamedKey::Backspace),
                ModifiersState::empty(),
                false
            ),
            Some(vec![0x7f])
        );
        assert_eq!(
            input::keyboard_bytes(&Key::Named(NamedKey::Space), ModifiersState::empty(), false),
            Some(vec![b' '])
        );
        assert_eq!(
            input::keyboard_bytes(&Key::Character("c".into()), ModifiersState::CONTROL, false),
            Some(vec![0x03])
        );
        assert_eq!(
            input::keyboard_bytes(&Key::Character("x".into()), ModifiersState::CONTROL, false),
            None
        );
        assert_eq!(
            input::keyboard_bytes(&Key::Character("中".into()), ModifiersState::empty(), false),
            None
        );
        assert_eq!(
            input::keyboard_bytes(
                &Key::Named(NamedKey::Process),
                ModifiersState::CONTROL,
                false
            ),
            None
        );
    }

    #[test]
    fn ime_commit_is_the_exact_utf8_pty_payload() {
        assert_eq!(
            ime_commit_bytes("你好世界"),
            vec![
                0xe4, 0xbd, 0xa0, 0xe5, 0xa5, 0xbd, 0xe4, 0xb8, 0x96, 0xe7, 0x95, 0x8c,
            ]
        );
    }

    #[test]
    fn committed_utf8_projects_as_alacritty_wide_lead_and_spacer_cells() {
        let mut session = DualPlaneSession::with_quotas_and_cell_height(
            NonZeroU32::new(8).unwrap(),
            NonZeroU32::new(2).unwrap(),
            DEFAULT_STAGING_QUOTA,
            M0_FROZEN_LINE_QUOTA,
            std::num::NonZeroI64::new(22 * bt_viewport::SUBPIXELS_PER_PX).unwrap(),
        );
        session.feed(&ime_commit_bytes("A你B")).unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();

        assert_eq!(frame.cells[0].text, "A");
        assert_eq!(frame.cells[1].text, "你");
        assert!(
            frame.cells[1]
                .style
                .flags
                .contains(bt_transcript::CellFlags::WIDE_CHAR)
        );
        assert!(frame.cells[2].wide_spacer);
        assert_eq!(frame.cells[3].text, "B");
    }

    #[test]
    fn bracketed_paste_follows_vendor_decset_and_normalizes_crlf() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(8).unwrap(), NonZeroU32::new(2).unwrap());

        session.feed(b"\x1b[?2004h").unwrap();
        assert_eq!(
            input::paste_bytes("one\r\ntwo\n", session.bracketed_paste_mode()),
            b"\x1b[200~one\rtwo\r\x1b[201~"
        );

        session.feed(b"\x1b[?2004l").unwrap();
        assert_eq!(
            input::paste_bytes("one\r\ntwo\n", session.bracketed_paste_mode()),
            b"one\rtwo\r"
        );
    }

    #[test]
    fn unavailable_clipboard_copy_keeps_selection_and_allows_a_retry() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
        session.feed(b"retry me").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let selection = ViewSelection {
            start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
            end: frame.anchor_at(0, 7, Bias::After).unwrap().unwrap(),
        };
        session.set_view_selection(Some(selection.clone()));
        projection.set_selection(Some(selection));

        let copied = copy_selection(&mut session, &mut projection, |_| {
            Err(anyhow!("injected clipboard owner contention"))
        });
        assert!(
            !copied,
            "clipboard contention must not escape as a fatal error"
        );
        assert_eq!(session.selection_text().as_deref(), Some("retry me"));
        assert!(projection.selection().is_some());

        let mut clipboard = String::new();
        let copied = copy_selection(&mut session, &mut projection, |text| {
            clipboard.push_str(text);
            Ok(())
        });
        assert!(copied);
        assert_eq!(clipboard, "retry me");
        assert!(session.view_selection().is_none());
        assert!(projection.selection().is_none());
    }

    #[test]
    fn ctrl_c_keeps_its_existing_empty_text_write_and_clear_semantics() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
        session.feed(b"   ").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let selection = ViewSelection {
            start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
            end: frame.anchor_at(0, 2, Bias::After).unwrap().unwrap(),
        };
        session.set_view_selection(Some(selection.clone()));
        projection.set_selection(Some(selection));
        let mut writes = Vec::new();

        assert!(copy_selection(&mut session, &mut projection, |text| {
            writes.push(text.to_owned());
            Ok(())
        }));
        assert_eq!(writes, [""]);
        assert!(session.view_selection().is_none());
        assert!(projection.selection().is_none());
    }

    #[test]
    fn copy_on_select_writes_nonempty_text_and_keeps_the_selection() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
        session.feed(b"drag me").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let selection = ViewSelection {
            start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
            end: frame.anchor_at(0, 6, Bias::After).unwrap().unwrap(),
        };
        session.set_view_selection(Some(selection));
        let mut clipboard = String::new();

        assert!(write_selection_text(&session, true, |text| {
            clipboard.push_str(text);
            Ok(())
        }));
        assert_eq!(clipboard, "drag me");
        assert_eq!(session.selection_text().as_deref(), Some("drag me"));
    }

    #[test]
    fn copy_on_select_does_not_touch_the_clipboard_for_an_empty_selection() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
        session.feed(b"click").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let anchor = frame.anchor_at(0, 2, Bias::Before).unwrap().unwrap();
        session.set_view_selection(Some(ViewSelection {
            start: anchor.clone(),
            end: anchor,
        }));
        let mut writes = 0;

        assert!(!write_selection_text(&session, true, |_| {
            writes += 1;
            Ok(())
        }));
        assert_eq!(writes, 0);
    }

    #[test]
    fn unavailable_clipboard_during_copy_on_select_keeps_the_selection() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
        session.feed(b"retry me").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        session.set_view_selection(Some(ViewSelection {
            start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
            end: frame.anchor_at(0, 7, Bias::After).unwrap().unwrap(),
        }));

        assert!(!write_selection_text(&session, true, |_| {
            Err(anyhow!("injected clipboard owner contention"))
        }));
        assert_eq!(session.selection_text().as_deref(), Some("retry me"));
    }

    #[test]
    fn unavailable_clipboard_paste_keeps_state_and_allows_a_retry() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
        session.feed(b"selected").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let selection = ViewSelection {
            start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
            end: frame.anchor_at(0, 7, Bias::After).unwrap().unwrap(),
        };
        session.set_view_selection(Some(selection.clone()));
        projection.set_selection(Some(selection));
        let mut pty_writes = Vec::new();

        assert!(
            !paste_from_clipboard(
                &mut session,
                &mut projection,
                || Err(anyhow!("injected clipboard owner contention")),
                |chunk| {
                    pty_writes.extend_from_slice(chunk);
                    Ok(())
                },
            )
            .unwrap()
        );
        assert!(pty_writes.is_empty());
        assert!(session.view_selection().is_some());
        assert!(projection.selection().is_some());

        assert!(
            paste_from_clipboard(
                &mut session,
                &mut projection,
                || Ok("paste me".to_owned()),
                |chunk| {
                    pty_writes.extend_from_slice(chunk);
                    Ok(())
                },
            )
            .unwrap()
        );
        assert_eq!(pty_writes, b"paste me");
        assert!(session.view_selection().is_none());
        assert!(projection.selection().is_none());
    }

    #[test]
    fn unavailable_system_wheel_setting_uses_the_windows_default() {
        assert_eq!(
            recoverable_wheel_scroll_amount(Err("injected SPI failure".to_owned())),
            bt_platform::WheelScrollAmount::Lines(3)
        );
    }

    #[test]
    fn disconnected_math_dispatch_downgrades_once_and_leaves_the_real_session_usable() {
        let start = Instant::now();
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(40).unwrap(), NonZeroU32::new(2).unwrap());
        session.feed_at(b"$$x$$\x1b[?25l", start).unwrap();
        assert_eq!(
            session.advance_live_stability(start + bt_term::LIVE_MATH_STABLE_INTERVAL),
            1
        );
        let (tasks, receiver) = mpsc::channel();
        drop(receiver);
        let mut running = true;
        let mut notice_pending = false;

        assert!(dispatch_pending_math_tasks(
            &mut session,
            &tasks,
            &mut running,
            &mut notice_pending,
        ));
        assert!(!running);
        assert!(notice_pending);
        session.feed(b"\r\nterminal-still-running").unwrap();
        assert!(
            session
                .terminal()
                .visible_text()
                .iter()
                .any(|row| row.contains("terminal-still-running"))
        );

        assert_eq!(
            take_math_worker_notice(&mut notice_pending),
            Some(MATH_WORKER_STOPPED_NOTICE)
        );
        assert!(!notice_pending);
        assert_eq!(take_math_worker_notice(&mut notice_pending), None);
        assert!(!dispatch_pending_math_tasks(
            &mut session,
            &tasks,
            &mut running,
            &mut notice_pending,
        ));
        assert!(
            !notice_pending,
            "the user-visible downgrade notice is one-shot"
        );
    }

    #[test]
    fn ime_cursor_area_throttle_coalesces_to_sixty_hz_and_flushes_the_last_area() {
        let start = Instant::now();
        let first = ImeCursorArea {
            x: 10,
            y: 20,
            width: 9,
            height: 22,
        };
        let latest = ImeCursorArea { x: 30, ..first };
        let mut throttle = ImeCursorThrottle::default();

        assert_eq!(throttle.offer(first, start), Some(first));
        assert_eq!(
            throttle.offer(latest, start + Duration::from_millis(3)),
            None
        );
        assert_eq!(throttle.flush_due(start + Duration::from_millis(15)), None);
        assert_eq!(
            throttle.flush_due(start + IME_CURSOR_AREA_INTERVAL),
            Some(latest)
        );
        assert_eq!(throttle.deadline(), None);
    }

    #[test]
    fn startup_polls_pty_until_the_first_text_frame_is_presented() {
        assert_eq!(startup_poll_delay(false), Some(STARTUP_PTY_POLL_INTERVAL));
        assert_eq!(startup_poll_delay(true), None);
    }

    #[test]
    fn startup_trace_title_is_human_readable_without_console_output() {
        assert_eq!(startup_scale_title(1.5), "BetterTerminal M0-beta · 1.5x");
        assert_eq!(
            startup_trace_title(
                Duration::from_millis(682),
                Duration::from_millis(1089),
                1.25,
            ),
            "BetterTerminal M0-beta — bg 682ms · text 1089ms · 1.25x"
        );
    }

    #[test]
    fn startup_metrics_must_match_the_authoritative_win32_scale_factor() {
        assert!(ensure_metrics_match_authoritative_scale(1.5, 1.5).is_ok());
        assert!(ensure_metrics_match_authoritative_scale(1.0, 1.5).is_err());
    }

    #[test]
    fn recorded_swapchain_size_matches_clamped_physical_inner_after_every_reconcile_size() {
        const LIMIT: u32 = 8192;
        for inner_size in [
            PhysicalSize::new(960, 600),
            PhysicalSize::new(1440, 900),
            PhysicalSize::new(1920, 1200),
            PhysicalSize::new(2560, 1440),
        ] {
            assert!(swapchain_size_matches_inner(
                (inner_size.width, inner_size.height),
                inner_size,
                LIMIT,
            ));
        }
        assert!(swapchain_size_matches_inner(
            (534, LIMIT),
            PhysicalSize::new(534, 65_464),
            LIMIT,
        ));
        assert!(!swapchain_size_matches_inner(
            (3840, 2160),
            PhysicalSize::new(1920, 1200),
            LIMIT,
        ));
    }

    #[test]
    fn pty_pixel_size_is_clamped_to_backend_width() {
        let size = pty_size(
            GridSize {
                columns: std::num::NonZeroU16::new(80).unwrap(),
                rows: std::num::NonZeroU16::new(24).unwrap(),
            },
            PhysicalSize::new(100_000, 80_000),
        );
        assert_eq!((size.pixel_width, size.pixel_height), (u16::MAX, u16::MAX));
    }

    #[test]
    fn window_resize_coalescer_keeps_only_the_last_size_and_resets_quiet_deadline() {
        let start = Instant::now();
        let first = GridSize {
            columns: std::num::NonZeroU16::new(80).unwrap(),
            rows: std::num::NonZeroU16::new(24).unwrap(),
        };
        let final_grid = GridSize {
            columns: std::num::NonZeroU16::new(112).unwrap(),
            rows: std::num::NonZeroU16::new(31).unwrap(),
        };
        let mut pending = None;
        coalesce_pty_resize(&mut pending, first, PhysicalSize::new(960, 600), start);
        coalesce_pty_resize(
            &mut pending,
            final_grid,
            PhysicalSize::new(1440, 900),
            start + Duration::from_millis(150),
        );

        assert!(take_due_pty_resize(&mut pending, start + Duration::from_millis(349)).is_none());
        let committed =
            take_due_pty_resize(&mut pending, start + Duration::from_millis(350)).unwrap();
        assert_eq!(committed.grid, final_grid);
        assert_eq!(committed.physical, PhysicalSize::new(1440, 900));
        assert!(pending.is_none());
    }

    #[test]
    fn resize_atomic_present_gate_rejects_the_previous_grid_frame() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(80).unwrap(), NonZeroU32::new(24).unwrap());
        let mut projection = session.new_projection(session.layout_key());
        let old_frame = session.viewport_frame(&mut projection).unwrap();
        let new_grid = GridSize {
            columns: std::num::NonZeroU16::new(42).unwrap(),
            rows: std::num::NonZeroU16::new(12).unwrap(),
        };
        assert!(!frame_matches_grid(&old_frame, new_grid));

        session
            .resize(NonZeroU32::new(42).unwrap(), NonZeroU32::new(12).unwrap())
            .unwrap();
        session.refresh_projection(&mut projection);
        let new_frame = session.viewport_frame(&mut projection).unwrap();
        assert!(frame_matches_grid(&new_frame, new_grid));
    }

    #[test]
    fn pty_mode_only_update_is_presentation_equivalent_but_text_is_not() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(3).unwrap());
        let mut projection = session.new_projection(session.layout_key());
        let before = session.viewport_frame(&mut projection).unwrap();

        session.feed(b"\x1b[?2004h").unwrap();
        session.refresh_projection(&mut projection);
        let mode_only = session.viewport_frame(&mut projection).unwrap();
        assert!(presentation_equivalent(&before, &mode_only));

        session.feed(b"visible").unwrap();
        session.refresh_projection(&mut projection);
        let text = session.viewport_frame(&mut projection).unwrap();
        assert!(!presentation_equivalent(&mode_only, &text));
    }

    #[test]
    fn content_before_trailing_bsu_is_published_and_survives_empty_sync_timeout() {
        let mut harness = PtyPresentationHarness::new(20, 2);

        assert!(harness.feed_drain(b"visible-before-bsu\x1b[?2026h"));
        assert!(harness.session.synchronized_update_deadline().is_some());
        assert_eq!(harness.publications, 1);
        assert!(
            frame_row_text(harness.pending.pending_frame().unwrap(), 0)
                .contains("visible-before-bsu")
        );

        let (finished, republished) = harness.finish_synchronized_update();
        assert!(finished);
        assert!(
            !republished,
            "the already-pending visible frame is equivalent"
        );
        assert_eq!(harness.publications, 1);
        assert!(
            frame_row_text(harness.pending.pending_frame().unwrap(), 0)
                .contains("visible-before-bsu")
        );
    }

    #[test]
    fn completed_sync_update_is_published_before_a_trailing_bsu_in_the_same_drain() {
        let mut harness = PtyPresentationHarness::new(24, 2);

        assert!(harness.feed_drain(b"\x1b[?2026h\x1b[H\x1b[2Kclosed-update\x1b[?2026l\x1b[?2026h"));
        assert!(harness.session.synchronized_update_deadline().is_some());
        assert_eq!(harness.publications, 1);
        assert!(
            frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains("closed-update")
        );
    }

    #[test]
    fn open_synchronized_update_still_suppresses_its_intermediate_state() {
        let mut harness = PtyPresentationHarness::new(24, 2);
        assert!(harness.feed_drain(b"base"));
        assert!(harness.present_pending());

        assert!(!harness.feed_drain(b"\x1b[?2026h\rhidden-intermediate"));
        assert!(harness.session.synchronized_update_deadline().is_some());
        assert_eq!(harness.publications, 1);
        assert!(harness.pending.pending_frame().is_none());
        assert!(frame_row_text(harness.last_presented.as_ref().unwrap(), 0).contains("base"));

        assert!(harness.feed_drain(b"\x1b[?2026l"));
        assert!(harness.session.synchronized_update_deadline().is_none());
        assert_eq!(harness.publications, 2);
        assert!(
            frame_row_text(harness.pending.pending_frame().unwrap(), 0)
                .contains("hidden-intermediate")
        );
    }

    #[test]
    fn rapid_synchronized_update_chain_never_withholds_a_completed_frame() {
        const UPDATE_COUNT: usize = 81;
        let mut harness = PtyPresentationHarness::new(24, 2);

        for update in 0..UPDATE_COUNT {
            let prefix = if update == 0 { "\x1b[?2026h" } else { "" };
            let bytes = format!("{prefix}\x1b[H\x1b[2Kframe-{update:02}\x1b[?2026l\x1b[?2026h");
            assert!(
                harness.feed_drain(bytes.as_bytes()),
                "completed update {update} was not published"
            );
            assert!(harness.session.synchronized_update_deadline().is_some());
            assert!(
                frame_row_text(harness.pending.pending_frame().unwrap(), 0)
                    .contains(&format!("frame-{update:02}"))
            );
        }

        assert_eq!(harness.publications, UPDATE_COUNT);
        assert!(!harness.feed_drain(b"\x1b[?2026l"));
        assert!(harness.session.synchronized_update_deadline().is_none());
        assert!(frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains("frame-80"));
        assert!(harness.present_pending());
        assert!(harness.pending.pending_frame().is_none());
    }

    #[test]
    fn pending_frame_is_the_a_b_a_equivalence_baseline() {
        let mut harness = PtyPresentationHarness::new(4, 1);

        assert!(harness.feed_drain(b"A"));
        assert!(harness.present_pending());
        assert!(frame_row_text(harness.last_presented.as_ref().unwrap(), 0).contains('A'));

        assert!(harness.feed_drain(b"\rB"));
        assert!(frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains('B'));
        assert!(harness.feed_drain(b"\rA"));
        assert!(frame_row_text(harness.pending.pending_frame().unwrap(), 0).contains('A'));
        assert_eq!(harness.publications, 3);
    }

    #[test]
    fn anchored_mouse_forwarding_uses_live_viewport_rows_and_clamps_frozen_rows() {
        assert!(!UserInputKind::Mouse.returns_view_to_live());
        assert!(UserInputKind::Keyboard.returns_view_to_live());

        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
        session
            .feed(b"zero\r\none\r\ntwo\r\nthree\r\nfour\r\nfive")
            .unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let bottom_frame = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(bottom_frame.scroll_offset_rows, 0);

        let physical_hit = bt_render::GridHit { row: 3, column: 5 };
        assert_eq!(
            live_viewport_mouse_hit(&bottom_frame, physical_hit),
            physical_hit
        );

        projection.scroll_by_rows(2);
        session.refresh_projection(&mut projection);
        let anchored_frame = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(anchored_frame.scroll_offset_rows, 2);

        let live_hit = live_viewport_mouse_hit(&anchored_frame, physical_hit);
        assert_eq!(live_hit, bt_render::GridHit { row: 1, column: 5 });
        assert_eq!(
            input::mouse_bytes(
                true,
                input::MouseProtocolButton::Left,
                input::MouseProtocolEvent::Press,
                live_hit.row,
                live_hit.column,
                ModifiersState::empty(),
            ),
            b"\x1b[<0;6;2M"
        );

        assert_eq!(
            live_viewport_mouse_hit(&anchored_frame, bt_render::GridHit { row: 1, column: 5 },),
            bt_render::GridHit { row: 0, column: 5 }
        );
    }

    #[test]
    fn expanded_presented_row_map_drives_forwarded_mouse_row_and_column() {
        let start = Instant::now();
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(8).unwrap(), NonZeroU32::new(12).unwrap());
        session
            .feed_at(
                b"\x1b[?1049h$$x^2$$\r\nbarrier1\r\nbarrier2\r\nbottom",
                start,
            )
            .unwrap();
        assert_eq!(
            session.advance_live_stability(start + bt_term::LIVE_MATH_STABLE_INTERVAL),
            1
        );
        let mut task = session.take_live_worker_task().unwrap();
        let raster = render_live_detection_task(&MathEngine::new(), &mut task, foreground_rgb())
            .expect("test formula rasterizes through the production live worker entry");
        let ink_height_px = raster.height_px;
        assert!(session.complete_live_worker_result(task, Ok(raster)));

        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let cell_height = 18 * bt_viewport::SUBPIXELS_PER_PX;
        assert!(frame.row_map[0].height_subpixels > cell_height);
        assert_eq!(frame.math_blocks[0].artifact.render_scale_milli, 1000);
        let padding = cell_height / 4;
        assert_eq!(
            frame.math_blocks[0].artifact.height_subpixels,
            i64::from(ink_height_px) * bt_viewport::SUBPIXELS_PER_PX + 2 * padding,
            "display box height is alpha-tight ink plus symmetric 25% cell padding"
        );
        assert_eq!(
            frame.math_blocks[0].artifact.vertical_padding_subpixels,
            padding
        );

        let target = frame.row_map[2];
        let target_y = target.top_subpixels + target.height_subpixels / 2;
        let visual_hit = bt_render::GridHit {
            row: frame
                .visual_row_at(target_y)
                .expect("expanded logical row remains hittable in the presented frame"),
            column: 5,
        };
        let forwarded = live_viewport_mouse_hit(&frame, visual_hit);
        assert_eq!(forwarded, bt_render::GridHit { row: 2, column: 5 });
        assert_eq!(
            input::mouse_bytes(
                true,
                input::MouseProtocolButton::Left,
                input::MouseProtocolEvent::Press,
                forwarded.row,
                forwarded.column,
                ModifiersState::empty(),
            ),
            b"\x1b[<0;6;3M"
        );
    }

    #[test]
    fn forwarded_mouse_hit_stays_bound_to_the_presented_frame_during_an_unpresented_shift() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(6).unwrap());
        session
            .feed(b"\x1b[?1003h\x1b[?1006ha\r\nb\r\nc\r\nheader\r\nx\r\ny")
            .unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let presented = session.viewport_frame(&mut projection).unwrap();
        assert!(frame_row_text(&presented, 3).contains("header"));

        session.feed(b"\r\nexpanded-1\r\nexpanded-2").unwrap();
        session.refresh_projection(&mut projection);
        let unpresented = session.viewport_frame(&mut projection).unwrap();
        assert!(frame_row_text(&unpresented, 1).contains("header"));
        assert!(!frame_row_text(&unpresented, 3).contains("header"));

        let stale_aim = bt_render::GridHit { row: 3, column: 0 };
        let forwarded = live_viewport_mouse_hit(&presented, stale_aim);
        let mut route = None;
        let bytes = route_forwarded_mouse_button(
            &mut route,
            ElementState::Pressed,
            input::MouseProtocolButton::Left,
            forwarded,
            session.terminal_modes(),
            ModifiersState::empty(),
        )
        .unwrap();

        assert_eq!(bytes, b"\x1b[<0;1;4M");
        assert_eq!(
            forwarded.row, 3,
            "the stale row correctly misses live row 1"
        );
    }

    #[test]
    fn tracked_tui_mouse_keeps_priority_over_ctrl_link_gesture() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(4).unwrap());
        session.feed(b"\x1b[?1000h\x1b[?1006h").unwrap();
        let hit = bt_render::GridHit { row: 1, column: 2 };
        let mut route = None;
        let forwarded = route_forwarded_mouse_button(
            &mut route,
            ElementState::Pressed,
            input::MouseProtocolButton::Left,
            hit,
            session.terminal_modes(),
            ModifiersState::CONTROL,
        );
        assert!(forwarded.is_some());
        assert!(matches!(route, Some(MouseRoute::Forward(_))));

        let mut shifted_route = None;
        assert!(
            route_forwarded_mouse_button(
                &mut shifted_route,
                ElementState::Pressed,
                input::MouseProtocolButton::Left,
                hit,
                session.terminal_modes(),
                ModifiersState::CONTROL | ModifiersState::SHIFT,
            )
            .is_none()
        );
        assert!(shifted_route.is_none());
    }

    #[test]
    fn stationary_double_click_stays_strictly_paired_across_tui_repaints() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(20).unwrap(), NonZeroU32::new(6).unwrap());
        session.feed(b"\x1b[?1000h\x1b[?1006hready").unwrap();
        let mut projection = session.new_projection(session.layout_key());
        let hit = bt_render::GridHit { row: 3, column: 8 };
        let mut route = None;
        let mut captured_user_input = Vec::new();

        for repaint in [
            b"\x1b[Hfirst repaint\r\na\r\nb\r\nc\r\nd\r\ne\r\nf".as_slice(),
            b"\x1b[Hsecond repaint\r\ng\r\nh\r\ni\r\nj\r\nk\r\nl",
        ] {
            let frame = session.viewport_frame(&mut projection).unwrap();
            assert_eq!(frame.scroll_offset_rows, 0);
            let live_hit = live_viewport_mouse_hit(&frame, hit);
            for state in [ElementState::Pressed, ElementState::Released] {
                captured_user_input.push(
                    route_forwarded_mouse_button(
                        &mut route,
                        state,
                        input::MouseProtocolButton::Left,
                        live_hit,
                        session.terminal_modes(),
                        ModifiersState::empty(),
                    )
                    .expect("tracked click must produce one PTY write per edge"),
                );
            }
            assert!(route.is_none());
            session.feed(repaint).unwrap();
            session.refresh_projection(&mut projection);
        }

        assert_eq!(
            captured_user_input,
            [
                b"\x1b[<0;9;4M".to_vec(),
                b"\x1b[<0;9;4m".to_vec(),
                b"\x1b[<0;9;4M".to_vec(),
                b"\x1b[<0;9;4m".to_vec(),
            ]
        );
    }

    #[test]
    fn post_drag_wheel_frame_reaches_the_renderer_text_row_slice_boundary() {
        let start = Instant::now();
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(8).unwrap(), NonZeroU32::new(3).unwrap());
        session.feed_at(b"a\r\nb\r\nc", start).unwrap();
        session
            .resize_at(
                NonZeroU32::new(4).unwrap(),
                NonZeroU32::new(2).unwrap(),
                start + Duration::from_millis(10),
            )
            .unwrap();
        session.mark_pty_resize_requested_at(
            NonZeroU32::new(4).unwrap(),
            NonZeroU32::new(2).unwrap(),
            start + Duration::from_millis(210),
        );
        session
            .feed_at(b"\r\nx", start + Duration::from_millis(220))
            .unwrap();
        assert!(
            session
                .finish_resize_if_quiescent(start + Duration::from_millis(420))
                .unwrap()
        );

        let mut projection = session.new_projection(session.layout_key());
        projection.scroll_by_rows(1);
        session.refresh_projection(&mut projection);
        let frame = session.viewport_frame(&mut projection).unwrap();
        session.record_published_frame(&frame, start + Duration::from_millis(421));
        let render_rows = bt_render::text_row_cells(&frame)
            .unwrap()
            .collect::<Vec<_>>();
        assert_eq!(render_rows.len(), 2);
        assert!(render_rows.iter().all(|row| row.len() == 4));
    }

    #[test]
    fn panic_log_uses_the_process_temp_directory_without_requiring_stderr() {
        assert_eq!(
            panic_log_path(),
            std::env::temp_dir().join("bt-app-panic.log")
        );
    }

    #[test]
    fn one_cell_terminal_and_zero_pixel_transition_are_defended() {
        let one = std::num::NonZeroU16::new(1).unwrap();
        let grid = GridSize {
            columns: one,
            rows: one,
        };
        let backend = pty_size(grid, PhysicalSize::new(0, 0));
        assert_eq!((backend.columns.get(), backend.rows.get()), (1, 1));
        assert_eq!((backend.pixel_width, backend.pixel_height), (0, 0));

        let mut session =
            DualPlaneSession::new(NonZeroU32::new(1).unwrap(), NonZeroU32::new(1).unwrap());
        session.feed(b"A").unwrap();
        session
            .resize(NonZeroU32::new(1).unwrap(), NonZeroU32::new(1).unwrap())
            .unwrap();
        assert_eq!(session.terminal().visible_text(), ["A"]);
    }

    #[test]
    fn direct_width_fixture_places_legacy_and_2027_closing_bars_on_their_rulers() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(80).unwrap(), NonZeroU32::new(24).unwrap());
        session
            .feed(include_bytes!("../../../scripts/dev/width-probe-input.vt"))
            .unwrap();

        let closing_bar = |row: u32, start: usize| {
            session
                .terminal()
                .visible_row(row)
                .unwrap()
                .cells
                .iter()
                .enumerate()
                .skip(start)
                .filter(|(_, cell)| cell.text == "|")
                .map(|(column, _)| column)
                .nth(1)
                .unwrap()
        };
        let rows = [2, 4, 6, 8, 10, 12, 14];
        let legacy_widths = [8, 4, 2, 1, 7, 1, 1];
        let mode_2027_widths = [2, 2, 2, 1, 7, 1, 2];
        for ((row, legacy), clustered) in rows.into_iter().zip(legacy_widths).zip(mode_2027_widths)
        {
            assert_eq!(closing_bar(row, 0), 1 + legacy, "legacy content row {row}");
            assert_eq!(
                closing_bar(row + 1, 0),
                1 + legacy,
                "legacy ruler row {}",
                row + 1
            );
            assert_eq!(
                closing_bar(row, 40),
                41 + clustered,
                "2027 content row {row}"
            );
            assert_eq!(
                closing_bar(row + 1, 40),
                41 + clustered,
                "2027 ruler row {}",
                row + 1
            );
        }
    }

    #[test]
    fn direct_glyph_fixture_preserves_emoji_and_ambiguous_cell_occupancy() {
        let mut session =
            DualPlaneSession::new(NonZeroU32::new(80).unwrap(), NonZeroU32::new(24).unwrap());
        session
            .feed(include_bytes!("../../../scripts/dev/glyph-probe-input.vt"))
            .unwrap();

        let bar_columns = |row: u32| {
            session
                .terminal()
                .visible_row(row)
                .unwrap()
                .cells
                .iter()
                .enumerate()
                .filter(|(_, cell)| cell.text == "|")
                .map(|(column, _)| column)
                .collect::<Vec<_>>()
        };
        for (content_row, width) in [(2, 2), (4, 2), (6, 2), (8, 2), (10, 1)] {
            assert_eq!(bar_columns(content_row), [0, 1 + width]);
            assert_eq!(bar_columns(content_row + 1), [0, 1 + width]);
        }
        assert_eq!(bar_columns(13), [0, 2, 4]);
        assert_eq!(bar_columns(14), [0, 2, 4]);
    }

    #[test]
    fn real_powershell_input_reaches_a_viewport_owned_frame() {
        let columns = std::num::NonZeroU16::new(48).unwrap();
        let rows = std::num::NonZeroU16::new(10).unwrap();
        let mut pty =
            PtySession::spawn_default(PtySize::cells(columns, rows), Arc::new(|| {})).unwrap();
        let mut session = DualPlaneSession::with_quotas_and_cell_height(
            nonzero_u32(columns.get()),
            nonzero_u32(rows.get()),
            DEFAULT_STAGING_QUOTA,
            M0_FROZEN_LINE_QUOTA,
            std::num::NonZeroI64::new(22 * bt_viewport::SUBPIXELS_PER_PX).unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut command_sent = false;
        let mut output_seen = false;
        while Instant::now() < deadline {
            let bytes = pty.read_output();
            if !bytes.is_empty() {
                session.feed(&bytes).unwrap();
                let replies = session.take_pty_writes();
                for reply in &replies {
                    pty.write(reply).unwrap();
                }
                if !command_sent && !replies.is_empty() {
                    pty.write(&ime_commit_bytes("Write-Output ('BT_APP_' + 'INPUT_OK')\r"))
                        .unwrap();
                    command_sent = true;
                }
                output_seen = session
                    .terminal()
                    .visible_text()
                    .iter()
                    .any(|line| line.contains("BT_APP_INPUT_OK"));
                if output_seen {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        assert!(
            command_sent,
            "PowerShell never completed its terminal handshake"
        );
        assert!(output_seen, "PowerShell command output never reached Term");
        let mut projection = session.new_projection(session.layout_key());
        let frame = session.viewport_frame(&mut projection).unwrap();
        let rendered_text = frame
            .cells
            .iter()
            .map(|cell| cell.text.as_str())
            .collect::<String>();
        assert!(rendered_text.contains("BT_APP_INPUT_OK"));
        pty.write(b"exit\r").unwrap();
        pty.shutdown().unwrap();
    }
}
