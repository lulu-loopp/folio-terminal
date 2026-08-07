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
mod persist;
mod seats;

use anyhow::{Context, Result, anyhow, ensure};
use bt_doc::{Bias, LayoutKey};
use bt_layout::{Axis, SeatLayout, SeatMetrics, SplitId, WorkAreaHint};
use bt_math::{MathEngine, MathRaster, MathRenderError};
use bt_persist::{SESSION_SCHEMA_VERSION, SessionV1, TabV1, WindowBoundsV1, WindowStateV1};
use bt_pty::{OutputWake, PSREADLINE_INVOKE_PROMPT_INPUT, PtySession, PtySize};
use bt_render::{
    FrameSource, FrameTrigger, GridSize, ImeCursorArea, LatestFrameSlot, MathHit, MathHitTarget,
    PeekImageOverlay, Preedit, PresentOutcome, PreviewImage, Renderer, SeatViewport,
    background_rgb, compose_preedit, foreground_rgb, frame_content_digest,
    frame_is_alternate_screen, preview_image_extent, theme_revision,
};
use bt_term::{
    DualPlaneSession, InlineImageDecoder, MathLayoutOptions, MouseTracking, SessionDecorationTask,
    SessionMathTask, TerminalModes, normalized_local_image_path_key, render_detection_task,
    render_live_detection_task,
};
use bt_transcript::DEFAULT_STAGING_QUOTA;
use bt_viewport::{
    HyperlinkHit, MathBlockAnchor, ViewSelection, ViewportFrame, ViewportProjection,
};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize},
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
/// Whether a live OSC 133 input buffer withholds terminal and ConPTY resizes (user ruling
/// 2026-08-06). **A policy bit, not a structure.**
///
/// `false` lets every resize through the existing 200 ms coalescer even while input is present.
/// The post-commit private shell-integration injection remains active, so PSReadLine repairs its
/// cached anchor at every landed width. Empty buffers use the integration's output-free re-anchor;
/// non-empty buffers retain InvokePrompt. Flipping this back to `true` restores the retained
/// confirm-then-release machine verbatim; `typed_input_defers_resize` is the single production
/// convergence point.
const TYPED_INPUT_RESIZE_DEFERRAL: bool = false;
/// M0-alpha's single-session frozen-line budget; later configuration work may expose it.
const M0_FROZEN_LINE_QUOTA: NonZeroUsize = NonZeroUsize::new(100_000).unwrap();
const MULTI_CLICK_INTERVAL: Duration = Duration::from_millis(500);
const HYPERLINK_HOVER_DELAY: Duration = Duration::from_millis(300);
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

enum MathWorkerRequest {
    Decoration(MathWorkerTask),
    /// Hover-peek decode: read and decode a local image off-thread without touching any
    /// decoration record. The completion routes only to the app-side peek cache, so the band
    /// creation gates (cursor line, semantic input region) are never bypassed.
    PeekImage {
        path: PathBuf,
    },
    /// Resample a peeked decode into the flyout's display box. Same worker as every other
    /// resample, and for the same reason: a wallpaper-sized Lanczos3 pass is tens of milliseconds
    /// and the event thread must not spend them. The completion routes only to the peek slot.
    PeekScale(bt_term::InlineImageScaleTask),
    PreviewScale(bt_term::InlineImageScaleTask),
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
    /// A decoded image resampled into the display box a band shows it in. Resampling a
    /// wallpaper-sized decode is tens of milliseconds, so it belongs here and never on the event
    /// thread.
    ScaleInlineImage {
        scaled: bt_term::ScaledInlineImage,
    },
    PeekImage {
        path: PathBuf,
        result: std::result::Result<bt_term::DecodedInlineImage, bt_term::InlineImageDecodeError>,
    },
    PeekScaledImage {
        scaled: bt_term::ScaledInlineImage,
    },
    PreviewScaledImage {
        scaled: bt_term::ScaledInlineImage,
    },
}

struct MathWorker {
    tasks: mpsc::Sender<MathWorkerRequest>,
    results: mpsc::Receiver<MathWorkerResult>,
}

impl MathWorker {
    fn spawn(proxy: EventLoopProxy<AppEvent>) -> Result<Self> {
        let (task_tx, task_rx) = mpsc::channel::<MathWorkerRequest>();
        let (result_tx, result_rx) = mpsc::channel::<MathWorkerResult>();
        thread::Builder::new()
            .name("bt-math-worker".to_owned())
            .spawn(move || {
                let engine = MathEngine::new();
                let mut image_decoder = InlineImageDecoder::default();
                while let Ok(work) = task_rx.recv() {
                    let completion = match work {
                        MathWorkerRequest::Decoration(MathWorkerTask {
                            task,
                            foreground_rgb,
                        }) => match task {
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
                                    let result = render_live_detection_task(
                                        &engine,
                                        &mut task,
                                        foreground_rgb,
                                    );
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
                            SessionDecorationTask::ScaleInlineImage(task) => {
                                DecorationWorkerCompletion::ScaleInlineImage {
                                    scaled: bt_term::scale_inline_image(&task),
                                }
                            }
                        },
                        MathWorkerRequest::PeekImage { path } => {
                            let result = image_decoder.decode(bt_term::InlineImageTask {
                                occurrence_id: 0,
                                source: bt_term::InlineImageSource::LocalPath(path.clone()),
                            });
                            DecorationWorkerCompletion::PeekImage { path, result }
                        }
                        MathWorkerRequest::PeekScale(task) => {
                            DecorationWorkerCompletion::PeekScaledImage {
                                scaled: bt_term::scale_inline_image(&task),
                            }
                        }
                        MathWorkerRequest::PreviewScale(task) => {
                            DecorationWorkerCompletion::PreviewScaledImage {
                                scaled: bt_term::scale_inline_image(&task),
                            }
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
    tasks: &mpsc::Sender<MathWorkerRequest>,
    running: &mut bool,
    notice_pending: &mut bool,
) -> bool {
    if !*running {
        return false;
    }
    while let Some(task) = session.take_decoration_worker_task() {
        if tasks
            .send(MathWorkerRequest::Decoration(MathWorkerTask {
                task,
                foreground_rgb: foreground_rgb(),
            }))
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
    /// One-shot startup notice from `PtySession::spawn_default` falling back to `powershell.exe`.
    /// Shown on the first frame published, then discarded — see `shell_fallback_notice` at the
    /// `spawn_default` call site.
    shell_fallback_notice: Option<String>,
    projection: ViewportProjection,
    pending_frames: LatestFrameSlot,
    grid: GridSize,
    /// The size the child has actually been told about — never a size that has only been solved or
    /// queued. It is the same value as `grid` at rest, and the two are deliberately separate only
    /// where they genuinely differ: inside the `WINDOW_RESIZE_QUIET` coalescing window our grid has
    /// already reflowed while the child has not heard yet, and under the typed-input deferral
    /// (`schedule_grid_change`) neither has moved but a target is queued. Asking "does the child
    /// need to hear this?" of our own grid answers that question with the wrong fact, and a drag
    /// that comes back to where the child already sits would then still send it a resize.
    conpty_grid: GridSize,
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
    /// Wheel detents awaiting a mouse-protocol report; per-notch, no system-lines multiplier.
    notch_wheel_remainder: f64,
    /// Fractional subpixels awaiting consumption by the LOCAL scroll routes only. Forwarding
    /// routes keep their own line-quantized accumulators above; the two never pour into each
    /// other, so switching routes cannot dump parked residue as a sudden jump.
    local_wheel_subpixel_remainder: f64,
    pending_pty_resize: Option<PendingPtyResize>,
    /// One private PSReadLine anchor repair owed by the current resize transaction. ConPTY cursor
    /// reads are a terminal round trip (`CSI 6 n` -> CPR), so writing the chord beside
    /// `PtySession::resize` can make the handler sample a cursor the child has not settled yet.
    /// A boolean is intentional: every commit in one drag replaces the same debt, and only the
    /// final transaction quiescence may pay it.
    pending_psreadline_resize_reanchor: bool,
    pending_resize_present: Option<GridSize>,
    hyperlink_hover: HyperlinkHover,
    /// Every image reference the frame currently on screen draws, as the cells that draw it — the
    /// session's scan of that frame, kept because the four verbs must read the *same* list the paint
    /// read (user ruling 2026-08-04). Rescanning on each pointer event would be a second opinion
    /// about a frame that has not changed; this is the first one, held.
    frame_image_references: FrameImageReferences,
    /// The reference the frame on screen was drawn with the solid underline over. Read only to
    /// decide whether the pointer has moved onto or off a reference and a repaint is therefore owed.
    underlined_image_reference: Option<bt_term::FrameImageReference>,
    peek_hover: PeekHover,
    peek_cache: std::collections::HashMap<String, PeekCacheEntry>,
    /// The one display-sized thumbnail the flyout can draw, and the one resample in flight. See
    /// `PeekThumbnail` for why a single entry is the whole policy.
    peek_thumbnail: Option<PeekThumbnail>,
    peek_thumbnail_pending: Option<PeekThumbnailTarget>,
    preview_image: Option<PreviewImageState>,
    math_hover_anchor: Option<MathBlockAnchor>,
    math_hover_clear_at: Option<Instant>,
    pending_math_context_anchor: Option<MathBlockAnchor>,
    /// The layout tree this window hosts. A lone terminal leaf by default, which
    /// is today's window written down.
    seats: seats::Seats,
    /// The most recent answer from `solve`. Every rectangle the renderer and the
    /// input router use comes from here, so the picture and the hit test can
    /// never be two geometries (D4).
    seat_layout: SeatLayout,
    seat_pointer: seats::ChromePointer,
    divider_drag: Option<DividerDrag>,
    /// The last work area that was successfully observed (tiny-window §4.4).
    work_area: WorkAreaHint,
    session_store: persist::SessionStore,
}

/// A divider drag in flight. Holds only the split's identity: the geometry is
/// re-read from the current solve on every pointer move, because the answer to
/// "where is this divider" must not be a second copy of the layout.
#[derive(Clone, Copy, Debug)]
struct DividerDrag {
    split: SplitId,
    dir: Axis,
}

#[derive(Clone, Copy, Debug)]
struct PendingPtyResize {
    grid: GridSize,
    physical: PhysicalSize<u32>,
    deadline: Instant,
    /// When the typed-input gate started answering "empty", or `None` if the last sample of it
    /// found content. See `sample_typed_input_gate`.
    blank_since: Option<Instant>,
}

impl PendingPtyResize {
    /// Has the gate answered "empty" for an unbroken `WINDOW_RESIZE_QUIET`, ending at `now`?
    fn blank_confirmed_at(&self, now: Instant) -> bool {
        self.blank_since
            .is_some_and(|since| now >= since + WINDOW_RESIZE_QUIET)
    }

    /// The first instant this request could be released: its own coalescing deadline, and — once a
    /// blank run has started — the end of that run's confirmation window.
    fn release_deadline(&self) -> Instant {
        self.blank_since.map_or(self.deadline, |since| {
            self.deadline.max(since + WINDOW_RESIZE_QUIET)
        })
    }
}

/// Record what the typed-input gate answered at this wake.
///
/// The gate is a question about the grid's *current* contents, and the child rewrites that grid in
/// whatever pieces a 16 KiB read happens to cut its output into. PSReadLine redraws by parking the
/// cursor on `B`, erasing, and writing the buffer back; when the erase and the rewrite arrive in
/// different reads, the wake in between finds a grid that honestly holds nothing. So a single
/// sample cannot be a release condition — only an unbroken run of them can, and any sample that
/// finds content starts the next run from scratch.
fn sample_typed_input_gate(
    pending: &mut Option<PendingPtyResize>,
    now: Instant,
    typed_input_live: bool,
) {
    let Some(pending) = pending.as_mut() else {
        return;
    };
    if typed_input_live {
        pending.blank_since = None;
    } else {
        pending.blank_since.get_or_insert(now);
    }
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
        // A new size is not a new answer about the child's buffer: a drag over an idle prompt keeps
        // sampling "empty" from its first frame on, so the confirmation window it started runs
        // alongside the coalescer's and adds nothing to the wait.
        blank_since: pending.as_ref().and_then(|pending| pending.blank_since),
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

/// The single gate between a solved grid and ConPTY.
///
/// A solve that answers what `conpty_grid` already holds must schedule nothing: the ConPTY
/// sidecar review pinned at 83dbcd3 found that any live resize call is unsafe while a shell is
/// still initializing (PSReadLine caches its own cursor anchor, and a reflow invalidates it — a
/// defect in conhost itself, not the sidecar), and a call whose columns and rows do not move is
/// never the resize that opens that window; it is only ever a spurious repeat of one already
/// applied. Startup's post-`ShowWindow` DPI reconciliation, a live OS `Resized`, and a divider drag
/// (`commit_seat_geometry`) all solve through this one point, so a clean, same-DPI session restore
/// — whose spawn-time grid already *is* the seat's resolved grid — reaches zero ConPTY resize
/// requests after spawn. Returns whether the grid actually changed, so a call site can gate its own
/// terminal-actor resize on the identical decision.
///
/// `conpty_grid` is what the child was last *told*, which is why the same answer also cancels a
/// queued request: a drag that wanders out and comes back leaves the child exactly where it already
/// was, and replaying the intermediate size at release would be a resize nobody asked for. (Before
/// the typed-input deferral the two grids could not diverge far enough for this to be observable;
/// under it they can, and "exactly one resize at release, carrying the final size" would otherwise
/// be false whenever the final size is the one the child never left.)
fn coalesce_pty_resize_on_grid_change(
    pending: &mut Option<PendingPtyResize>,
    next_grid: GridSize,
    conpty_grid: GridSize,
    physical: PhysicalSize<u32>,
    observed_at: Instant,
) -> bool {
    let changed = next_grid != conpty_grid;
    if changed {
        coalesce_pty_resize(pending, next_grid, physical, observed_at);
    } else {
        *pending = None;
    }
    changed
}

/// The retained typed-input half of the same gate (policy introduced 2026-08-04, default disabled
/// by user ruling 2026-08-06).
///
/// PSReadLine — 2.0.0 and 2.4.5 alike — reduces its cached render-anchor *column* modulo the new
/// width whenever the pane narrows, and never restores it when the pane widens again. The fault
/// arms only while its input buffer is non-empty across that narrowing; the next redraw then
/// splices into the prompt row. Three commits of ours produced byte-identical corruption and an
/// independent reference terminal reproduced it from the child's own bytes, so there is nothing on
/// our side to correct — the only faithful mitigation is not to tell the child about a width it
/// would re-anchor against while it is holding text.
///
/// When the policy feeds this machine `typed_input_live = true`, a due resize is held back and
/// released once that has been false for an unbroken `WINDOW_RESIZE_QUIET`. What releases is still
/// a *question about current state* and
/// never a timer over the drag: the input region closing (submission), its content going empty (the
/// line cleared), a screen switch, a reset, ED 2 or an alternate-screen transition all make
/// `DualPlaneSession::typed_shell_input_live` answer `false`, and every one of them reaches us as
/// child output — which wakes the event loop on its own. A buffer that still holds text is held for
/// as long as it holds it, however long the drag has been over.
///
/// The confirmation window is what a single sample cannot give: the gate reads the grid, and a
/// redraw split across two reads leaves the grid momentarily empty between them
/// (`sample_typed_input_gate`). Waiting out the same quiet period the coalescer already uses costs
/// an idle prompt nothing — its blank run starts at the drag's first frame — and costs a line that
/// was genuinely emptied one quiet window.
fn release_due_pty_resize(
    pending: &mut Option<PendingPtyResize>,
    now: Instant,
    typed_input_live: bool,
) -> Option<PendingPtyResize> {
    sample_typed_input_gate(pending, now, typed_input_live);
    pending
        .is_some_and(|pending| pending.blank_confirmed_at(now))
        .then(|| take_due_pty_resize(pending, now))
        .flatten()
}

/// The whole scheduling decision `Runtime::schedule_grid_change` makes, with no window in it:
/// queue (or cancel) what the child still owes, and answer the grid our own actor should reflow to
/// *now* — `None` while the retained gate holds. With the policy off, `typed_input_live` reaches
/// this function as false and the actor always follows the solved grid immediately.
fn plan_grid_change(
    pending: &mut Option<PendingPtyResize>,
    next_grid: GridSize,
    conpty_grid: GridSize,
    local_grid: GridSize,
    physical: PhysicalSize<u32>,
    observed_at: Instant,
    typed_input_live: bool,
) -> Option<GridSize> {
    coalesce_pty_resize_on_grid_change(pending, next_grid, conpty_grid, physical, observed_at);
    // A pointer frame is a sample of the gate like any other, and taking it here is what keeps the
    // confirmation window free for the case it must stay free for: a drag over an idle prompt has
    // been reading "empty" since its first frame, so the window has long since elapsed by the time
    // the drag stops and the coalescer's own deadline arrives.
    sample_typed_input_gate(pending, observed_at, typed_input_live);
    (next_grid != local_grid && !typed_input_live).then_some(next_grid)
}

/// The instant the coalesced ConPTY resize could next be released, or `None` while nothing is owed
/// or the typed-input gate is holding it.
///
/// Withholding it while held is not an optimisation: the deadline has usually already passed by
/// then, and handing an event loop a past instant to wait until is a spin. There is nothing to
/// wait *for* either — a gate that is holding on *content* is released by child output, which wakes
/// the loop by itself. A gate reading empty is the opposite case: the confirmation window it is
/// serving may well end with no further output at all, so the loop is given that instant to wake
/// at.
fn pty_resize_wake_deadline(
    pending: Option<PendingPtyResize>,
    typed_input_live: bool,
    now: Instant,
) -> Option<Instant> {
    (!typed_input_live)
        .then_some(pending)
        .flatten()
        .map(|pending| pending.release_deadline())
        // `about_to_wait` has already serviced everything due at `now`. Re-offering that instant
        // (or an older coalescing deadline retained across a gate transition) makes winit wake
        // immediately and re-enter this turn without an external event. Only a future confirmation
        // boundary is a useful wake-up request.
        .filter(|deadline| *deadline > now)
}

/// Advance the resize gate and derive its next wake from the same gate sample.
///
/// Keeping these two decisions together prevents `about_to_wait` from servicing a content-holding
/// state and then deriving `WaitUntil` from a different, empty-state reading of the session. The
/// latter can expose the request's already-due coalescing deadline while `blank_since` still says
/// that no empty confirmation run has begun.
fn service_pending_pty_resize(
    pending: &mut Option<PendingPtyResize>,
    now: Instant,
    typed_input_live: bool,
) -> (Option<PendingPtyResize>, Option<Instant>) {
    let due = release_due_pty_resize(pending, now, typed_input_live);
    let wake = pty_resize_wake_deadline(*pending, typed_input_live, now);
    (due, wake)
}

const fn typed_input_resize_deferral_active(
    policy: bool,
    pty_present: bool,
    typed_input_live: bool,
) -> bool {
    policy && pty_present && typed_input_live
}

/// The private shell-integration input owed after one successful ConPTY resize commit.
///
/// `false` includes every session that has never emitted OSC 133, every closed input region, and
/// every alternate screen. Returning `None` is what makes those cases a byte-for-byte no-op rather
/// than a best-effort guess about which shell might be present.
fn psreadline_resize_repaint_input(shell_input_region_open: bool) -> Option<&'static [u8]> {
    shell_input_region_open.then_some(PSREADLINE_INVOKE_PROMPT_INPUT)
}

fn replace_psreadline_resize_reanchor_debt(pending: &mut bool, shell_input_region_open: bool) {
    *pending = psreadline_resize_repaint_input(shell_input_region_open).is_some();
}

fn take_psreadline_resize_reanchor_input(
    pending: &mut bool,
    shell_input_region_open: bool,
) -> Option<&'static [u8]> {
    std::mem::take(pending)
        .then(|| psreadline_resize_repaint_input(shell_input_region_open))
        .flatten()
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
    local_image_activation: LocalImageActivation,
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
        // The underline is the affordance and follows the candidate immediately; only the status
        // tooltip waits out the hover delay. A candidate change therefore needs a republish too.
        let candidate_changed = self.candidate.is_some() || hit.is_some();
        self.candidate = hit;
        self.show_at = self.candidate.as_ref().map(|_| now + HYPERLINK_HOVER_DELAY);
        active_changed || candidate_changed
    }

    /// The link whose span should render underlined right now: the settled hover if the tooltip is
    /// up, otherwise the instant candidate under the pointer.
    fn underline_target(&self) -> Option<&HyperlinkHit> {
        self.active.as_ref().or(self.candidate.as_ref())
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

/// What a settled hover is a hover *of*. Two shapes reach the flyout, and they differ in exactly
/// one thing: whether a cache miss can be filled by reading a file.
///
/// A printed path, a `file://` URI and an OSC 8 link target all name a file on disk — the worker
/// reads it. An OSC 1337 payload names nothing: its bytes arrived once in the stream and were
/// decoded then, so the flyout can only show what was already remembered under the decoder's
/// content key. Both shapes are one identity string in `peek_cache`, which is why the rest of the
/// pipeline never has to ask which is which.
#[derive(Clone, Debug)]
struct PeekSubject {
    /// Identity in `peek_cache`: a normalized path for a named file, the decoder's content key for
    /// a stream payload. Two observations are the same hover exactly when these agree.
    key: String,
    /// The file to read on a cache miss, when there is one.
    path: Option<PathBuf>,
}

/// Identity is the key alone — the path beside it is only how a miss is filled. A file the terminal
/// printed twice under two spellings is one picture, and sliding between them must not restart the
/// settle clock or re-decode anything.
impl PartialEq for PeekSubject {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for PeekSubject {}

impl PeekSubject {
    fn from_path(path: PathBuf) -> Self {
        Self {
            key: normalized_local_image_path_key(&path),
            path: Some(path),
        }
    }

    /// A payload the session already decoded, addressed by the key it was cached under.
    fn from_content_key(key: String) -> Self {
        Self { key, path: None }
    }
}

#[derive(Clone, Debug)]
struct PeekCandidate {
    subject: PeekSubject,
    /// Physical pointer position of the most recent observation; the flyout anchors where the
    /// hover settles, not where it began.
    pointer: PhysicalPosition<f64>,
}

/// Hover state for the local-image peek flyout (preview matrix §4): same 300ms settle clock as
/// the hyperlink tooltip, keyed by path so sliding along one path span never restarts the timer.
#[derive(Default)]
struct PeekHover {
    candidate: Option<PeekCandidate>,
    show_at: Option<Instant>,
    /// The settled hover whose flyout is showing (or whose decode is in flight). The settle
    /// pointer is kept so a decode that finishes later still places the flyout where the hover
    /// settled.
    active: Option<PeekCandidate>,
}

impl PeekHover {
    /// Track the path under the pointer. Returns true when a previously shown flyout must be
    /// hidden because the pointer left its path span.
    fn observe(
        &mut self,
        subject: Option<PeekSubject>,
        pointer: PhysicalPosition<f64>,
        now: Instant,
    ) -> bool {
        if self.active.is_some()
            && subject.as_ref() == self.active.as_ref().map(|active| &active.subject)
        {
            self.candidate = None;
            self.show_at = None;
            return false;
        }
        let active_hidden = self.active.take().is_some();
        match subject {
            Some(subject) => {
                let same_span =
                    self.candidate.as_ref().map(|candidate| &candidate.subject) == Some(&subject);
                self.candidate = Some(PeekCandidate { subject, pointer });
                if !same_span {
                    self.show_at = Some(now + HYPERLINK_HOVER_DELAY);
                }
            }
            None => {
                self.candidate = None;
                self.show_at = None;
            }
        }
        active_hidden
    }

    /// The candidate whose settle deadline has passed, promoted to active. The caller resolves
    /// the thumbnail (cache or worker) and shows the flyout.
    fn activate_if_due(&mut self, now: Instant) -> Option<PeekCandidate> {
        if self.show_at.is_none_or(|deadline| now < deadline) {
            return None;
        }
        self.show_at = None;
        let candidate = self.candidate.take()?;
        self.active = Some(candidate.clone());
        Some(candidate)
    }

    /// Drop all peek state. Returns true when a visible flyout must be hidden.
    fn clear(&mut self) -> bool {
        self.candidate = None;
        self.show_at = None;
        self.active.take().is_some()
    }
}

/// App-side memory of peek decode outcomes, keyed by `PeekSubject::key` — the decoder's normalized
/// path identity for a named file, its content key for a stream payload.
/// `Failed` entries keep a missing or corrupt file from re-hitting the disk on every hover;
/// the payload bytes are shared `Arc`s with the worker decoder's own cache.
///
/// `Ready` holds the *native* decode. It is CPU memory the worker's decoder already retains, and
/// it is what a resample at any later display size is computed from — it never reaches the GPU.
enum PeekCacheEntry {
    Pending,
    Failed,
    Ready {
        key: String,
        rgba: Arc<[u8]>,
        width_px: u32,
        height_px: u32,
    },
}

/// One content at one display size: `(content key, display width, display height)`.
type PeekThumbnailTarget = (String, u32, u32);

/// The display-sized raster the flyout hands the renderer — the only peek pixels that ever reach
/// the GPU.
///
/// Cache policy: exactly one entry. A peek is transient and singular (one flyout at a time), the
/// display box depends on the viewport at hover time, and the native decodes stay path-keyed in
/// `peek_cache`, so re-showing at a size this entry does not hold costs one worker resample and
/// never a disk read. `key` carries the display size (`display_texture_key`), so the shared GPU
/// LRU can never serve a raster rastered for a different box.
struct PeekThumbnail {
    /// Identity of the native decode this was resampled from, matched against `PeekCacheEntry`.
    content_key: String,
    key: String,
    rgba: Arc<[u8]>,
    width_px: u32,
    height_px: u32,
}

/// Persistent preview-seat state. Native pixels remain in `peek_cache`; this holds only the one
/// display-sized raster and the question currently in flight for the solver's body rectangle.
struct PreviewImageState {
    path: PathBuf,
    pending: Option<PeekThumbnailTarget>,
    raster: Option<PeekThumbnail>,
    failure: Option<String>,
}

impl PreviewImageState {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            pending: None,
            raster: None,
            failure: None,
        }
    }

    fn title(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| self.path.to_string_lossy().into_owned())
    }

    fn message(&self) -> Option<&str> {
        self.failure.as_deref().or_else(|| {
            (self.pending.is_some() || self.raster.is_none()).then_some("Loading image…")
        })
    }
}

impl PeekThumbnail {
    fn from_scaled(scaled: bt_term::ScaledInlineImage) -> Self {
        Self {
            content_key: scaled.content_key,
            key: scaled.key,
            rgba: scaled.rgba,
            width_px: scaled.width_px,
            height_px: scaled.height_px,
        }
    }

    fn matches(&self, target: &PeekThumbnailTarget) -> bool {
        self.content_key == target.0 && (self.width_px, self.height_px) == (target.1, target.2)
    }

    /// The overlay the renderer draws: display-sized pixels under a display-sized identity, and a
    /// pointer anchor. This is the only path by which peek pixels reach the GPU.
    fn overlay(&self, pointer: PhysicalPosition<f64>) -> PeekImageOverlay {
        PeekImageOverlay {
            key: self.key.clone(),
            rgba: Arc::clone(&self.rgba),
            width_px: self.width_px,
            height_px: self.height_px,
            pointer_x: pointer.x as f32,
            pointer_y: pointer.y as f32,
        }
    }
}

/// The one resample that stands between a native decode and the flyout, addressed to the worker.
fn peek_scale_task(
    target: &PeekThumbnailTarget,
    rgba: Arc<[u8]>,
    native_width_px: u32,
    native_height_px: u32,
) -> bt_term::InlineImageScaleTask {
    let (content_key, display_width_px, display_height_px) = target;
    bt_term::InlineImageScaleTask {
        // The peek is not an occurrence in the document; its identity is the content key.
        occurrence_id: 0,
        content_key: content_key.clone(),
        rgba,
        width_px: native_width_px,
        height_px: native_height_px,
        display_width_px: *display_width_px,
        display_height_px: *display_height_px,
    }
}

/// The `peek_cache` identity a decoration-worker decode must be remembered under, so that the
/// hover which later asks for the same picture finds it instead of asking the disk again.
///
/// It is `PeekSubject`'s own rule, stated once: a named file is its normalized path, a stream
/// payload is the decoder's content key. Deriving it here rather than at the two call sites is what
/// makes "verification warms the peek" a fact about one key rather than a coincidence between two.
fn peek_cache_key_for_decode(
    source: &bt_term::InlineImageSource,
    decoded: &bt_term::DecodedInlineImage,
) -> String {
    match source {
        bt_term::InlineImageSource::Osc1337(_) => decoded.key.clone(),
        bt_term::InlineImageSource::LocalPath(path) => normalized_local_image_path_key(path),
    }
}

/// One frame's image references, with the row stride they were scanned at.
///
/// The stride travels with the list because a `GridHit` is a row and a column while a reference is
/// a set of cell indices; keeping the frame's own column count beside the scan is what makes the
/// two the same coordinate rather than two coordinates that usually agree.
///
/// One hovered cell resolves to at most one reference: the first that covers it. The scan pushes
/// printed text before link targets, so where a cell carries both — a link whose label spells the
/// path — the answer is what the pointer is actually standing on. Both name the same file anyway.
#[derive(Default)]
struct FrameImageReferences {
    columns: u32,
    references: Vec<bt_term::FrameImageReference>,
}

impl FrameImageReferences {
    fn at(&self, hit: bt_render::GridHit) -> Option<&bt_term::FrameImageReference> {
        let cell = hit.row.checked_mul(self.columns)?.checked_add(hit.column)?;
        self.references
            .iter()
            .find(|reference| reference.covers(cell))
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum LocalImageActivation {
    None,
    Preview(PathBuf),
    External(PathBuf),
}

impl LocalImageActivation {
    fn path(&self) -> Option<&std::path::Path> {
        match self {
            Self::None => None,
            Self::Preview(path) | Self::External(path) => Some(path),
        }
    }
}

fn local_image_activation(
    control: bool,
    click_no_drag: bool,
    worker_verified_path: Option<&std::path::Path>,
) -> LocalImageActivation {
    let Some(path) = worker_verified_path.filter(|_| click_no_drag) else {
        return LocalImageActivation::None;
    };
    if control {
        LocalImageActivation::External(path.to_path_buf())
    } else {
        LocalImageActivation::Preview(path.to_path_buf())
    }
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
        // Read the previous session before the window exists, so its bounds can
        // be the window's opening bounds rather than a correction applied after
        // the user has already seen it somewhere else.
        let session_store = persist::SessionStore::open();
        let restored = restore_window_placement(event_loop, session_store.loaded());
        let attributes = Window::default_attributes()
            .with_title(WINDOW_TITLE)
            .with_inner_size(
                restored
                    .map(|(_, size)| size)
                    .unwrap_or(LogicalSize::new(INITIAL_WIDTH, INITIAL_HEIGHT)),
            )
            // Do not expose the system class brush while the first swapchain image is pending.
            .with_visible(false);
        let attributes = match restored {
            Some((position, _)) => attributes.with_position(position),
            None => attributes,
        };
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
        let mut renderer = pollster::block_on(Renderer::new(
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
        // The seam of §4.2, in the one order it is allowed to run: window
        // geometry -> viewport -> solve -> seat rects -> the terminal seat's
        // cols/rows. The persisted tree is layout *intent* (L11); the rectangle
        // it produces here is computed fresh against this machine's DPI.
        let seats = session_store
            .loaded()
            .tabs
            .first()
            .and_then(|tab| seats::Seats::from_persisted(&tab.root))
            .unwrap_or_else(seats::Seats::lone_terminal);
        let (seat_layout, terminal_seat) = solve_seats(&seats, &renderer, render_physical);
        renderer.set_seat_viewport(terminal_seat);
        let grid = renderer
            .metrics()
            .grid_for_pixels(terminal_seat.width, terminal_seat.height);
        let probe_input = load_probe_input()?;
        let pty_proxy = proxy.clone();
        let wake: OutputWake = Arc::new(move || {
            let _ = pty_proxy.send_event(AppEvent::PtyOutput);
        });
        let phase_started = Instant::now();
        let mut pty = if probe_input.is_none() {
            Some(
                PtySession::spawn_default(
                    pty_size(grid, terminal_pty_physical(&renderer, render_physical)),
                    wake,
                )
                .context("spawn default PowerShell in ConPTY")?,
            )
        } else {
            None
        };
        // `spawn_default` already fell back to `powershell.exe` and logged the failure if its
        // resolved shell (a `BT_SHELL` override, or `pwsh.exe`) could not start; surface that
        // one-line notice on the very first frame through the same status-text channel the
        // math-worker downgrade notice uses.
        let shell_fallback_notice = pty
            .as_mut()
            .and_then(PtySession::take_shell_fallback_notice);
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
            shell_fallback_notice,
            projection,
            pending_frames: LatestFrameSlot::default(),
            grid,
            // `PtySession::spawn_default` above was handed exactly this grid.
            conpty_grid: grid,
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
            notch_wheel_remainder: 0.0,
            local_wheel_subpixel_remainder: 0.0,
            pending_pty_resize: None,
            pending_psreadline_resize_reanchor: false,
            pending_resize_present: None,
            hyperlink_hover: HyperlinkHover::default(),
            frame_image_references: FrameImageReferences::default(),
            underlined_image_reference: None,
            peek_hover: PeekHover::default(),
            peek_cache: std::collections::HashMap::new(),
            peek_thumbnail: None,
            peek_thumbnail_pending: None,
            preview_image: None,
            math_hover_anchor: None,
            math_hover_clear_at: None,
            pending_math_context_anchor: None,
            seats,
            seat_layout,
            seat_pointer: seats::ChromePointer::default(),
            divider_drag: None,
            work_area: WorkAreaHint::NeverKnown,
            session_store,
        };
        runtime.refresh_work_area();
        runtime.apply_window_min_inner_size();
        runtime.refresh_chrome();
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

    /// Re-solve the tree against the current surface and place the terminal
    /// seat. Returns the grid the terminal seat's rectangle asks for.
    ///
    /// This is the only place cols/rows are derived from pixels once seats
    /// exist, and it derives them from the *seat's* rectangle rather than the
    /// window's. The direction is one-way (red line L10): what comes back out
    /// of the terminal never re-enters here.
    fn resolve_seat_layout(&mut self, render_physical: PhysicalSize<u32>) -> GridSize {
        let (layout, terminal_seat) = solve_seats(&self.seats, &self.renderer, render_physical);
        self.seat_layout = layout;
        self.renderer.set_seat_viewport(terminal_seat);
        self.refresh_preview_for_layout();
        self.refresh_chrome();
        self.renderer
            .metrics()
            .grid_for_pixels(terminal_seat.width, terminal_seat.height)
    }

    fn seat_metrics(&self) -> SeatMetrics {
        seats::seat_metrics(self.renderer.metrics().dpi_milli().get())
    }

    /// Rebuild the chrome quads and labels from the current solve. Returns
    /// whether anything visible changed.
    fn refresh_chrome(&mut self) -> bool {
        let scale = self.renderer.metrics().scale_factor as f32;
        let preview_title = self.preview_image.as_ref().map(PreviewImageState::title);
        let preview_message = self
            .preview_image
            .as_ref()
            .and_then(PreviewImageState::message);
        let (quads, labels) = seats::build_chrome_with_preview(
            &self.seats,
            &self.seat_layout,
            scale,
            self.seat_pointer,
            preview_title.as_deref(),
            preview_message,
        );
        self.renderer.set_chrome(quads, labels)
    }

    /// Ask the OS for the work area of the display this window is on.
    ///
    /// tiny-window §4.4: a query that fails leaves the last successful answer in
    /// place, because the work area rarely changes between two queries and
    /// reusing the old number is more honest than inventing one. Having never
    /// succeeded is a different state with a different answer — no minimum at
    /// all, rather than a guess that could lock the user's window.
    fn refresh_work_area(&mut self) {
        let Ok(hwnd) = window_hwnd(&self.window) else {
            return;
        };
        let Ok(rect) = bt_platform::get_work_area(hwnd) else {
            return;
        };
        let scale = self.renderer.metrics().scale_factor.max(f64::MIN_POSITIVE);
        let width = ((rect.right - rect.left).max(0) as f64 / scale).round() as i64;
        let height = ((rect.bottom - rect.top).max(0) as f64 / scale).round() as i64;
        self.work_area = WorkAreaHint::Known(bt_layout::LogicalSize::px(width, height));
    }

    /// Hand the OS the minimum inner size the tree needs (§2.6.5, L12).
    fn apply_window_min_inner_size(&mut self) {
        let metrics = self.seat_metrics();
        let minimum = self.seats.min_inner_size(&metrics, self.work_area);
        self.window.set_min_inner_size(minimum.map(|size| {
            LogicalSize::new(
                size.width.floor_px().max(1) as f64,
                size.height.floor_px().max(1) as f64,
            )
        }));
    }

    /// The durable form of everything this window would want back after a
    /// restart. Layout *intent* only (L11): no rectangle, no cols/rows, no DPI
    /// of a seat — those are all recomputed by the next `solve`.
    fn session_snapshot(&self) -> SessionV1 {
        let mut session = self.session_store.loaded().clone();
        let scale = self.renderer.metrics().scale_factor.max(f64::MIN_POSITIVE);
        let inner = self.window.inner_size();
        let position = self
            .window
            .outer_position()
            .map(|p| (p.x, p.y))
            .unwrap_or((session.window.bounds.x, session.window.bounds.y));
        session.schema_version = SESSION_SCHEMA_VERSION;
        session.window = WindowStateV1 {
            bounds: WindowBoundsV1 {
                x: (f64::from(position.0) / scale).round() as i32,
                y: (f64::from(position.1) / scale).round() as i32,
                width: (f64::from(inner.width) / scale).round().max(1.0) as u32,
                height: (f64::from(inner.height) / scale).round().max(1.0) as u32,
            },
            dpi: self.renderer.metrics().dpi_milli().get(),
            maximized: self.window.is_maximized(),
            monitor_id: session.window.monitor_id.clone(),
        };
        session.tabs = vec![TabV1 {
            root: self.seats.to_persisted(),
            pinned: false,
            // Positional rather than a stable id: this slice has no leaf-id
            // registry to draw from, and the in-order index is a function of
            // the same tree shape the file already carries, so it cannot point
            // at a leaf the document does not have.
            focused_leaf: format!("leaf-{}", self.focus_leaf_index()),
        }];
        session.active_tab = 0;
        session
    }

    fn focus_leaf_index(&self) -> usize {
        self.seats
            .tree()
            .seats_in_order()
            .iter()
            .position(|seat| seat.id == self.seats.focus())
            .unwrap_or(0)
    }

    /// Record a meaningful change and start the debounce window (§5.1).
    fn mark_session_dirty(&mut self, now: Instant) {
        let snapshot = self.session_snapshot();
        self.session_store.record(snapshot, now);
    }

    /// The dev-only preview toggle, and everything one costs: the tree changes,
    /// so the window minimum, the terminal's columns, the ConPTY coalescer and
    /// the session file all follow it, in that order.
    fn toggle_preview_seat(&mut self) -> Result<()> {
        let metrics = self.seat_metrics();
        let was_open = self.seats.preview().is_some();
        if !self.seats.toggle_preview(&metrics) {
            return Ok(());
        }
        if was_open {
            self.preview_image = None;
            self.renderer.set_preview_image(None);
        }
        self.seat_pointer = seats::ChromePointer::default();
        self.divider_drag = None;
        self.apply_window_min_inner_size();
        self.commit_seat_geometry()
    }

    /// Open the ruled preview seat if necessary, otherwise reuse its geometry, then ask the shared
    /// worker/cache pipeline for this image. Keyboard focus deliberately remains on the terminal.
    fn open_preview_image(&mut self, path: PathBuf) -> Result<()> {
        self.preview_image = Some(PreviewImageState::new(path));
        self.renderer.set_preview_image(None);
        if self.seats.preview().is_none() {
            return self.toggle_preview_seat();
        }
        self.refresh_preview_for_layout();
        self.refresh_chrome();
        self.present_chrome_change()
    }

    /// Refit the current image to the solver's latest preview body. Every decode and Lanczos3
    /// resample stays on the existing decoration worker; this event-thread method only routes data.
    fn refresh_preview_for_layout(&mut self) {
        let Some(preview_seat) = self.seats.preview() else {
            self.renderer.set_preview_image(None);
            return;
        };
        let scale = self.renderer.metrics().scale_factor as f32;
        let Some(body) = seats::preview_body_viewport(&self.seat_layout, preview_seat, scale)
        else {
            self.renderer.set_preview_image(None);
            return;
        };
        let Some(path) = self
            .preview_image
            .as_ref()
            .map(|preview| preview.path.clone())
        else {
            self.renderer.set_preview_image(None);
            return;
        };
        let cache_key = normalized_local_image_path_key(&path);
        let decoded = match self.peek_cache.get(&cache_key) {
            Some(PeekCacheEntry::Ready {
                key,
                rgba,
                width_px,
                height_px,
            }) => Some((key.clone(), Arc::clone(rgba), *width_px, *height_px)),
            Some(PeekCacheEntry::Pending) => {
                self.renderer.set_preview_image(None);
                return;
            }
            Some(PeekCacheEntry::Failed) => {
                if let Some(preview) = self.preview_image.as_mut() {
                    preview.failure.get_or_insert_with(|| {
                        "Preview failed: image could not be loaded".to_owned()
                    });
                }
                self.renderer.set_preview_image(None);
                return;
            }
            None => None,
        };
        let Some((content_key, rgba, native_width, native_height)) = decoded else {
            self.renderer.set_preview_image(None);
            if !self.math_worker_running {
                if let Some(preview) = self.preview_image.as_mut() {
                    preview.failure =
                        Some("Preview failed: image worker is unavailable".to_owned());
                }
                return;
            }
            if self
                .math_worker
                .tasks
                .send(MathWorkerRequest::PeekImage { path })
                .is_ok()
            {
                self.peek_cache.insert(cache_key, PeekCacheEntry::Pending);
            } else if let Some(preview) = self.preview_image.as_mut() {
                preview.failure = Some("Preview failed: image worker is unavailable".to_owned());
            }
            return;
        };
        let Some((display_width, display_height)) =
            preview_image_extent(body.width, body.height, native_width, native_height)
        else {
            if let Some(preview) = self.preview_image.as_mut() {
                preview.failure = Some("Preview failed: preview seat is too small".to_owned());
            }
            self.renderer.set_preview_image(None);
            return;
        };
        let target = (content_key.clone(), display_width, display_height);
        if let Some(raster) = self
            .preview_image
            .as_ref()
            .and_then(|preview| preview.raster.as_ref())
            && raster.matches(&target)
        {
            self.renderer.set_preview_image(Some(PreviewImage {
                seat: body,
                key: raster.key.clone(),
                rgba: Arc::clone(&raster.rgba),
                width_px: raster.width_px,
                height_px: raster.height_px,
            }));
            return;
        }
        self.renderer.set_preview_image(None);
        if self
            .preview_image
            .as_ref()
            .is_some_and(|preview| preview.pending.as_ref() == Some(&target))
            || !self.math_worker_running
        {
            return;
        }
        let task = peek_scale_task(&target, rgba, native_width, native_height);
        if self
            .math_worker
            .tasks
            .send(MathWorkerRequest::PreviewScale(task))
            .is_ok()
        {
            if let Some(preview) = self.preview_image.as_mut() {
                preview.pending = Some(target);
                preview.failure = None;
            }
        } else if let Some(preview) = self.preview_image.as_mut() {
            preview.failure = Some("Preview failed: image worker is unavailable".to_owned());
        }
    }

    /// Re-solve after a tree edit and carry the consequences to the terminal.
    ///
    /// Deliberately routed through the same coalescer a window resize uses: a
    /// divider drag and an OS resize are the same event as far as ConPTY is
    /// concerned, and §4.2 says the solver does not participate in that
    /// debounce — it answers every frame, and someone else decides when the
    /// child hears about it.
    fn commit_seat_geometry(&mut self) -> Result<()> {
        let trace_started = self.trace_perf.then(Instant::now);
        let render_physical = presentation_physical_size(self.renderer.presentation_geometry());
        if render_physical.width == 0 || render_physical.height == 0 {
            return Ok(());
        }
        // A seat rectangle changing is a resize as far as a transient flyout is
        // concerned: its anchor was a physical point on the old pane and its
        // raster was sized to it. tiny-window §3.5 generalises the existing
        // dissolve rule to exactly this case, so the same two lines `resize`
        // already runs run here.
        self.peek_hover.clear();
        self.renderer.set_peek_overlay(None);
        let next_grid = self.resolve_seat_layout(render_physical);
        let solved_at = trace_started.map(|_| Instant::now());
        let now = Instant::now();
        self.schedule_grid_change(
            next_grid,
            terminal_pty_physical(&self.renderer, render_physical),
            now,
            "resize terminal actor for a seat layout change",
        )?;
        let resized_at = trace_started.map(|_| Instant::now());
        self.sync_math_layout_key();
        // The grid actually in force, which under the typed-input gate is still the old one. The
        // present gate admits the grid the frame will really carry, never the one merely solved.
        self.pending_resize_present = Some(self.grid);
        self.mark_session_dirty(now);
        self.publish_frame(FrameTrigger {
            occurred_at: now,
            source: FrameSource::Resize,
        })?;
        let published_at = trace_started.map(|_| Instant::now());
        let synchronous_present = self.divider_drag.is_none();
        // Pointer motion must stay ahead of the swapchain. `publish_frame` already requested a
        // redraw and `LatestFrameSlot` keeps the newest geometry, so presenting synchronously here
        // would make every divider event wait on GPU acquire/vsync before Windows can deliver the
        // next event. Non-drag seat edits still present immediately; a live drag is frame-paced by
        // RedrawRequested and may coalesce only superseded intermediate positions.
        if synchronous_present {
            self.redraw()?;
        }
        if let (Some(started), Some(solved), Some(resized), Some(published)) =
            (trace_started, solved_at, resized_at, published_at)
        {
            eprintln!(
                "BT_PERF_TRACE resize_frame solve_us={} actor_us={} publish_us={} redraw_us={} total_us={} queued={} columns={} rows={}",
                solved.saturating_duration_since(started).as_micros(),
                resized.saturating_duration_since(solved).as_micros(),
                published.saturating_duration_since(resized).as_micros(),
                Instant::now()
                    .saturating_duration_since(published)
                    .as_micros(),
                started.elapsed().as_micros(),
                u8::from(!synchronous_present),
                next_grid.columns,
                next_grid.rows,
            );
        }
        Ok(())
    }

    /// The pointer, expressed in the terminal seat's own coordinates, or `None`
    /// when it is not over the terminal seat at all.
    ///
    /// Every existing hit test — the grid, math blocks, hyperlinks, the peek
    /// flyout's anchor — reads through here, so all of them keep working with
    /// exactly one correction applied in exactly one place.
    fn terminal_pointer(&self) -> Option<PhysicalPosition<f64>> {
        let position = self.pointer_position?;
        let seat = self.renderer.seat_viewport();
        if !seats::terminal_contains(
            &self.seat_layout,
            self.seats.terminal(),
            position.x,
            position.y,
        ) {
            return None;
        }
        Some(PhysicalPosition::new(
            position.x - f64::from(seat.x),
            position.y - f64::from(seat.y),
        ))
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
        if let Some(hyperlink) = self.hyperlink_hover.underline_target()
            && terminal_frame.underline_hyperlink(hyperlink)
            && self.hyperlink_hover.active.is_some()
        {
            terminal_frame.status_text = self
                .hyperlink_hover
                .status_text(terminal_frame.columns.get() as usize);
        }
        // This frame's own references, scanned once for the whole of this frame's life on screen.
        // The hover upgrade below, the Ctrl+click verb and the peek all read this one list, so no
        // two of them can disagree about where a reference is or whether it is one. The session
        // scanned the same frame once more when it painted the resting dots inside `viewport_frame`;
        // collapsing the two would mean hanging the scan on the session as state, which is the very
        // kind of thing this affordance was rebuilt to be rid of.
        self.frame_image_references = FrameImageReferences {
            columns: terminal_frame.columns.get(),
            references: self.session.frame_image_references(&terminal_frame),
        };
        // The verified reference under the pointer turns solid, on the same terms the link does:
        // the underline is the affordance and follows the pointer immediately, while the 300ms
        // settle belongs to what the hover *reveals* — a tooltip there, a thumbnail here.
        let hovered_reference = self.hovered_image_reference();
        if let Some(reference) = hovered_reference.as_ref() {
            terminal_frame.underline_cells(&reference.cells, true);
        }
        self.underlined_image_reference = hovered_reference;
        if let Some(notice) = take_math_worker_notice(&mut self.math_worker_notice_pending) {
            terminal_frame.status_text = Some(notice.to_owned());
        }
        if let Some(notice) = self.shell_fallback_notice.take() {
            terminal_frame.status_text = Some(notice);
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
                            self.remember_decode_for_peek(&task, result.as_ref().ok());
                            self.session.complete_inline_image_result(task, result)
                        }
                        DecorationWorkerCompletion::ScaleInlineImage { scaled } => {
                            self.session.complete_inline_image_scale(scaled)
                        }
                        DecorationWorkerCompletion::PeekImage { path, result } => {
                            self.complete_peek_image(path, result)?;
                            // Peek state never enters frames, so no republish is needed.
                            false
                        }
                        DecorationWorkerCompletion::PeekScaledImage { scaled } => {
                            self.complete_peek_scale(scaled)?;
                            false
                        }
                        DecorationWorkerCompletion::PreviewScaledImage { scaled } => {
                            self.complete_preview_scale(scaled)?;
                            false
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
        //
        // A seat-origin one does. The caret rectangle is computed by the same
        // frame machinery as every other content pixel, so it is expressed in
        // the terminal seat's own coordinates; winit and IMM32 both want the
        // window's. This is the inverse of `terminal_pointer`'s correction and
        // the only place it runs in this direction (§4.1, one translation).
        let area = window_ime_cursor_area(self.renderer.seat_viewport(), area);
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
            // `[Console]::CursorLeft/Top` in the PSReadLine handler makes ConPTY ask the terminal
            // `CSI 6 n`. Pay the coalesced repair only after the final resize request *and* every
            // child byte it caused have been quiet. A new geometry event re-opens the transaction,
            // so a divider storm cannot install an intermediate commit's still-moving cursor.
            if let Some(reanchor_input) = take_psreadline_resize_reanchor_input(
                &mut self.pending_psreadline_resize_reanchor,
                self.session.shell_input_region_open(),
            ) && let Some(pty) = self.pty.as_mut()
            {
                pty.write(reanchor_input)
                    .context("request PSReadLine anchor repair after resize quiescence")?;
            }
            self.publish_frame(FrameTrigger {
                occurred_at: now,
                source: FrameSource::Expose,
            })?;
        }
        Ok(())
    }

    /// Carry a freshly solved grid to the child and to our own grid — or, when the retained
    /// typed-input policy is enabled and the shell is holding input, to neither.
    ///
    /// The seat rectangle has already moved by the time this is called (`resolve_seat_layout` is
    /// what produced `next_grid`), so a drag stays exactly as responsive as it is today. What is
    /// deferred is the pair: the ruling forbids desynchronizing our grid from ConPTY's, so under
    /// the gate they both stay at the old width and `flush_pending_pty_resize` moves them together.
    /// A grid that is wider than its seat is already the ordinary case for the renderer — the seat
    /// viewport scissors it — and it is the case a divider drag produces on every frame.
    fn schedule_grid_change(
        &mut self,
        next_grid: GridSize,
        physical: PhysicalSize<u32>,
        observed_at: Instant,
        context: &'static str,
    ) -> Result<()> {
        let deferred = self.typed_input_defers_resize();
        let Some(reflow) = plan_grid_change(
            &mut self.pending_pty_resize,
            next_grid,
            self.conpty_grid,
            self.grid,
            physical,
            observed_at,
            deferred,
        ) else {
            return Ok(());
        };
        self.session
            .resize(
                nonzero_u32(reflow.columns.get()),
                nonzero_u32(reflow.rows.get()),
            )
            .context(context)?;
        self.grid = reflow;
        Ok(())
    }

    /// Is a PTY resize deferred right now?
    ///
    /// This is the single policy convergence point. With `TYPED_INPUT_RESIZE_DEFERRAL` off it is
    /// always false. If that bit is restored, a child holding typed input defers as before, while
    /// `BT_PROBE_INPUT` — which spawns no child — still reflows immediately.
    fn typed_input_defers_resize(&self) -> bool {
        typed_input_resize_deferral_active(
            TYPED_INPUT_RESIZE_DEFERRAL,
            self.pty.is_some(),
            self.session.typed_shell_input_live(),
        )
    }

    fn flush_pending_pty_resize(&mut self, now: Instant) -> Result<Option<Instant>> {
        let deferred = self.typed_input_defers_resize();
        let (pending, wake_deadline) =
            service_pending_pty_resize(&mut self.pending_pty_resize, now, deferred);
        let Some(pending) = pending else {
            return Ok(wake_deadline);
        };
        // The deferred local reflow lands here, immediately before the child hears the same size,
        // in the same order the undeferred path uses (actor first, then ConPTY, then the vendor
        // reconcile). When nothing was deferred this is a no-op: our grid already moved at the
        // `Resized` that scheduled this.
        let reflowed = pending.grid != self.grid;
        if reflowed {
            self.session
                .resize(
                    nonzero_u32(pending.grid.columns.get()),
                    nonzero_u32(pending.grid.rows.get()),
                )
                .context("resize terminal actor for a released ConPTY resize")?;
            self.grid = pending.grid;
            self.sync_math_layout_key();
            self.pending_resize_present = Some(pending.grid);
        }
        // Snapshot the OSC 133 phase before resize reconciliation mutates terminal geometry. This
        // is broader than the typed-input gate on purpose: an empty, already-printed prompt caches
        // the same PSReadLine anchor and needs the same post-resize repair. Its 2.4.x handler is
        // output-free for an empty buffer; using InvokePrompt there abandons old wrapped rows on
        // every committed divider stop (the real-ConPTY chain probe pins that distinction).
        let shell_input_region_open = self.session.shell_input_region_open();
        if let Some(pty) = self.pty.as_mut() {
            pty.resize(pty_size(pending.grid, pending.physical))
                .context("commit coalesced final ConPTY resize")?;
        }
        // Replace, rather than accumulate, the current transaction's repair debt. The send happens
        // in `finish_resize_if_quiescent`, after ConPTY output has also been silent; a closed input
        // region records no debt and therefore still writes exactly zero private bytes.
        replace_psreadline_resize_reanchor_debt(
            &mut self.pending_psreadline_resize_reanchor,
            shell_input_region_open,
        );
        self.conpty_grid = pending.grid;
        // The quiet boundary is also where a resize *ends*, so it is the
        // meaningful change §5.1 asks the session write to be debounced behind.
        // Marking it on every intermediate `Resized` would turn one drag of a
        // window corner into a hundred disk writes.
        self.mark_session_dirty(now);
        let reconciled = self.session.mark_pty_resize_requested_at(
            nonzero_u32(pending.grid.columns.get()),
            nonzero_u32(pending.grid.rows.get()),
            now,
        );
        if reconciled || reflowed {
            self.publish_frame(FrameTrigger {
                occurred_at: now,
                source: FrameSource::Resize,
            })?;
        }
        Ok(wake_deadline)
    }

    fn frame_hit(&self) -> Option<bt_render::GridHit> {
        let position = self.terminal_pointer()?;
        let frame = self.last_presented_frame.as_ref()?;
        self.renderer
            .metrics()
            .hit_test_frame(frame, position.x, position.y)
    }

    fn math_hit(&self) -> Option<MathHit> {
        let position = self.terminal_pointer()?;
        let frame = self.last_presented_frame.as_ref()?;
        self.renderer.math_hit_test(frame, position.x, position.y)
    }

    fn hyperlink_hit(&self, hit: bt_render::GridHit) -> Option<HyperlinkHit> {
        self.last_presented_frame
            .as_ref()?
            .hyperlink_at(hit.row, hit.column)
    }

    /// The file a Ctrl+click at `hit` may hand to the system viewer (preview matrix §4, "leave this
    /// product"). Never path-*looking* text: the verb has always required that a worker actually
    /// opened and decoded the file, so that a misdetected word cannot launch anything.
    ///
    /// The cells are the frame's own — the very cells wearing the underline that promised the
    /// picture — so the verb and the mark cannot reach different text. Verification is the scan's
    /// `verified` (the decoration worker's record of this file) or the peek's own cache entry, which
    /// is the same worker and the same decoder reached by hovering rather than by detection.
    fn local_image_path_hit(&self, hit: bt_render::GridHit) -> Option<PathBuf> {
        let reference = self.frame_image_references.at(hit)?;
        (reference.verified
            || matches!(
                self.peek_cache
                    .get(&normalized_local_image_path_key(&reference.path)),
                Some(PeekCacheEntry::Ready { .. })
            ))
        .then(|| reference.path.clone())
    }

    /// The verified image reference the pointer is currently standing on, if any — the cells whose
    /// resting dots become a solid underline for as long as the pointer is there.
    ///
    /// Resolved from the frame's own scan per publish rather than remembered in hover state, because
    /// it is a pure function of "where is the pointer" and "what does this frame draw there", and
    /// both can change without a pointer event: a decode landing turns plain text into an underlined
    /// reference under a pointer that never moved.
    fn hovered_image_reference(&self) -> Option<bt_term::FrameImageReference> {
        let hit = self.frame_hit()?;
        self.frame_image_references
            .at(hit)
            .filter(|reference| reference.verified)
            .cloned()
    }

    /// Repaint when the pointer has moved onto or off a verified reference, so the solid underline
    /// arrives with the pointer rather than with the next unrelated frame.
    ///
    /// The other direction — a decode landing under a pointer that never moved — needs nothing
    /// here: `apply_math_results` already republishes when a completion changed session state, and
    /// the compose step asks the session afresh.
    fn refresh_image_reference_underline(&mut self) -> Result<()> {
        if self.hovered_image_reference() == self.underlined_image_reference {
            return Ok(());
        }
        self.publish_interaction_frame()
    }

    /// The image a hover at `hit` may preview, from any shape the screen can offer it in.
    ///
    /// The frame's own scan comes first, and it already carries every shape whose file can be named
    /// from what is drawn: a printed path, an OSC 7 relative form, a bare `file://` URI, and the
    /// target of an OSC 8 link whose visible text names no file at all. It is the same list the
    /// underline is painted from, which is what makes "you can peek exactly what is marked" a fact
    /// about one list rather than an agreement between two. Verification is not required here: a
    /// hover is how a picture nobody has opened yet gets opened.
    ///
    /// Last comes the one shape that names no file: an OSC 1337 payload, hovered over the
    /// `[image]` placeholder the adapter wrote for it. It is asked last because it is the only
    /// source that cannot be re-read — where a path and a placeholder somehow shared a cell, the
    /// text the pointer was actually put on is still what wins.
    ///
    /// The complement of inline admission is checked once, here, before any source is consulted —
    /// a link that happens to lie across a banded content point does not smuggle a second
    /// presentation of it.
    fn peek_target(&self, hit: bt_render::GridHit) -> Option<PeekSubject> {
        let anchor = self
            .last_presented_frame
            .as_ref()?
            .anchor_at(hit.row, hit.column, Bias::Before)
            .ok()??;
        if !self.session.peek_admits_at(&anchor) {
            return None;
        }
        self.frame_image_references
            .at(hit)
            .map(|reference| PeekSubject::from_path(reference.path.clone()))
            .or_else(|| {
                self.session
                    .inline_image_payload_peek_at(&anchor)
                    .map(PeekSubject::from_content_key)
            })
    }

    /// Present a pure peek-overlay change. The overlay lives beside the frame, not inside it, so
    /// `redraw` would find nothing queued and skip: when nothing newer is pending, the frame that
    /// is already on screen re-enters the slot; a queued newer frame carries the overlay along on
    /// its own redraw.
    fn present_peek_overlay(&mut self, overlay: Option<PeekImageOverlay>) -> Result<()> {
        if !self.renderer.set_peek_overlay(overlay) {
            return Ok(());
        }
        // Not while a resize present is outstanding: that gate admits only the newly projected
        // grid, and the frame on screen is the previous one. A repaint is already owed to the
        // resize, and it carries the renderer-side overlay state with it.
        if self.pending_resize_present.is_none()
            && self.pending_frames.pending_frame().is_none()
            && let Some(frame) = self.last_presented_frame.clone()
        {
            self.pending_frames
                .publish(
                    frame,
                    FrameTrigger {
                        occurred_at: Instant::now(),
                        source: FrameSource::Expose,
                    },
                )
                .context("re-present the on-screen frame for a peek overlay change")?;
        }
        self.window.request_redraw();
        Ok(())
    }

    /// Drop peek hover state and hide the flyout. Idempotent; used by every dismiss gesture
    /// (pointer off the span, pointer left, wheel, click, any key).
    fn dismiss_peek(&mut self) -> Result<()> {
        self.peek_hover.clear();
        self.present_peek_overlay(None)
    }

    fn activate_peek_if_due(&mut self, now: Instant) -> Result<()> {
        if let Some(candidate) = self.peek_hover.activate_if_due(now) {
            self.show_or_request_peek(&candidate)?;
        }
        Ok(())
    }

    /// Resolve the flyout for a settled hover: decode the subject if it is new, resample the decode
    /// into the box this viewport will draw it in if that raster is not the one already held, and
    /// present when display-sized pixels are in hand. Each miss is one worker round trip and the
    /// completion re-enters here, so the event thread neither decodes nor resamples.
    fn show_or_request_peek(&mut self, candidate: &PeekCandidate) -> Result<()> {
        let cache_key = candidate.subject.key.clone();
        let (content_key, native_rgba, native_width_px, native_height_px) =
            match self.peek_cache.get(&cache_key) {
                Some(PeekCacheEntry::Ready {
                    key,
                    rgba,
                    width_px,
                    height_px,
                }) => (key.clone(), Arc::clone(rgba), *width_px, *height_px),
                // A failed decode stays silent: the terminal text is the honest surface, and the
                // negative entry keeps hovers from re-hitting the disk.
                Some(PeekCacheEntry::Pending) | Some(PeekCacheEntry::Failed) => return Ok(()),
                None => {
                    // Nothing to read: a stream payload is cached when its decode lands or never,
                    // and the session only names one whose decode already succeeded, so a miss
                    // here is a hover that arrived first. The next one finds it.
                    let Some(path) = candidate.subject.path.clone() else {
                        return Ok(());
                    };
                    if !self.math_worker_running {
                        return Ok(());
                    }
                    if self
                        .math_worker
                        .tasks
                        .send(MathWorkerRequest::PeekImage { path })
                        .is_ok()
                    {
                        self.peek_cache.insert(cache_key, PeekCacheEntry::Pending);
                    }
                    return Ok(());
                }
            };
        // The renderer owns the box; a pane too small to host the flyout shows none, and nothing
        // is resampled for it.
        let Some((display_width_px, display_height_px)) = self
            .renderer
            .peek_thumbnail_extent(native_width_px, native_height_px)
        else {
            return Ok(());
        };
        let target: PeekThumbnailTarget = (content_key, display_width_px, display_height_px);
        if let Some(thumbnail) = self.peek_thumbnail.as_ref()
            && thumbnail.matches(&target)
        {
            let overlay = thumbnail.overlay(candidate.pointer);
            return self.present_peek_overlay(Some(overlay));
        }
        if self.peek_thumbnail_pending.as_ref() == Some(&target) || !self.math_worker_running {
            return Ok(());
        }
        if self
            .math_worker
            .tasks
            .send(MathWorkerRequest::PeekScale(peek_scale_task(
                &target,
                native_rgba,
                native_width_px,
                native_height_px,
            )))
            .is_ok()
        {
            self.peek_thumbnail_pending = Some(target);
        }
        Ok(())
    }

    /// Remember a decoration-worker decode in the peek cache, under the identity the peek asks by:
    /// the decoder's content key for an OSC 1337 payload, the normalized path for a named file.
    ///
    /// Two reasons, one seam. A stream payload is the one image the peek cannot go and fetch — the
    /// bytes were in the stream, the session decoded them once, and nothing else will ever ask for
    /// them again — so catching it here is the only way the `[image]` placeholder can peek at all.
    /// A named file *can* be re-read, but under the 2026-08-04 verification ruling the session has
    /// just had a worker open, size-check, format-check and decode it in order to earn the resting
    /// underline; letting that decode reach the flyout's cache is what makes the promised peek
    /// appear at once instead of after a second read of the same file. The ruling names that as the
    /// beneficial side effect to preserve, and this is where it is preserved.
    ///
    /// One file still has one entry: the key is `normalized_local_image_path_key`, the very key
    /// `PeekSubject::from_path` computes, so this fills the entry the peek would have created
    /// rather than adding a second one that could disagree.
    fn remember_decode_for_peek(
        &mut self,
        task: &bt_term::InlineImageTask,
        decoded: Option<&bt_term::DecodedInlineImage>,
    ) {
        let Some(decoded) = decoded else {
            return;
        };
        let cache_key = peek_cache_key_for_decode(&task.source, decoded);
        self.peek_cache.insert(
            cache_key,
            PeekCacheEntry::Ready {
                key: decoded.key.clone(),
                rgba: Arc::clone(&decoded.rgba),
                width_px: decoded.width_px,
                height_px: decoded.height_px,
            },
        );
    }

    /// Record a peek decode outcome and, when the hover is still settled on that path, show the
    /// flyout at the settle pointer.
    fn complete_peek_image(
        &mut self,
        path: PathBuf,
        result: std::result::Result<bt_term::DecodedInlineImage, bt_term::InlineImageDecodeError>,
    ) -> Result<()> {
        let cache_key = normalized_local_image_path_key(&path);
        let preview_matches = self
            .preview_image
            .as_ref()
            .is_some_and(|preview| normalized_local_image_path_key(&preview.path) == cache_key);
        match result {
            Ok(decoded) => {
                self.peek_cache.insert(
                    cache_key.clone(),
                    PeekCacheEntry::Ready {
                        key: decoded.key,
                        rgba: decoded.rgba,
                        width_px: decoded.width_px,
                        height_px: decoded.height_px,
                    },
                );
                if let Some(active) = self.peek_hover.active.clone()
                    && active.subject.key == cache_key
                {
                    self.show_or_request_peek(&active)?;
                }
            }
            Err(error) => {
                self.peek_cache
                    .insert(cache_key.clone(), PeekCacheEntry::Failed);
                if preview_matches && let Some(preview) = self.preview_image.as_mut() {
                    preview.failure = Some(format!("Preview failed: {error}"));
                }
            }
        }
        if preview_matches {
            self.refresh_preview_for_layout();
            self.refresh_chrome();
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Take delivery of the flyout's display-sized raster. Only the question still outstanding is
    /// answered here: an earlier size arriving after the viewport moved on leaves the newer request
    /// in flight rather than asking for it twice.
    fn complete_peek_scale(&mut self, scaled: bt_term::ScaledInlineImage) -> Result<()> {
        let delivered: PeekThumbnailTarget = (
            scaled.content_key.clone(),
            scaled.width_px,
            scaled.height_px,
        );
        if self.peek_thumbnail_pending.as_ref() == Some(&delivered) {
            self.peek_thumbnail_pending = None;
        }
        self.peek_thumbnail = Some(PeekThumbnail::from_scaled(scaled));
        if let Some(active) = self.peek_hover.active.clone() {
            self.show_or_request_peek(&active)?;
        }
        Ok(())
    }

    fn complete_preview_scale(&mut self, scaled: bt_term::ScaledInlineImage) -> Result<()> {
        let delivered: PeekThumbnailTarget = (
            scaled.content_key.clone(),
            scaled.width_px,
            scaled.height_px,
        );
        let Some(preview) = self.preview_image.as_mut() else {
            return Ok(());
        };
        if preview.pending.as_ref() != Some(&delivered) {
            return Ok(());
        }
        preview.pending = None;
        preview.raster = Some(PeekThumbnail::from_scaled(scaled));
        preview.failure = None;
        self.refresh_preview_for_layout();
        self.refresh_chrome();
        self.present_chrome_change()
    }

    fn activate_hyperlink_hover_if_due(&mut self, now: Instant) -> Result<()> {
        if self.hyperlink_hover.activate_if_due(now) {
            self.publish_interaction_frame()?;
        }
        Ok(())
    }

    fn pointer_left(&mut self) -> Result<()> {
        self.pointer_position = None;
        if self.seat_pointer.hover.take().is_some() && self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        self.dismiss_peek()?;
        let hyperlink_changed = self.hyperlink_hover.clear();
        if self.math_hover_anchor.is_some() {
            self.math_hover_clear_at = Some(Instant::now() + Duration::from_millis(500));
        }
        if hyperlink_changed {
            self.publish_interaction_frame()?;
        }
        // The pointer is gone, so `hovered_image_reference` now answers `None`: any solid underline
        // still on screen must fall back to its resting dots.
        self.refresh_image_reference_underline()?;
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
        let subpixels =
            i64::from(rows).saturating_mul(self.projection.cell_height_subpixels().get());
        self.projection.scroll_by_subpixels(subpixels);
        self.publish_interaction_frame()
    }

    fn begin_local_selection(&mut self, hit: bt_render::GridHit) -> Result<()> {
        self.dismiss_peek()?;
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
        let local_image_activation = local_image_activation(
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
                (SelectionDragMode::Word, selection.clone(), Some(selection))
            }
            3 => {
                let selection = frame
                    .line_selection(hit.row)
                    .context("reject non-rectangular frame during line selection")?
                    .context("line selection hit has no anchor")?;
                (SelectionDragMode::Line, selection.clone(), Some(selection))
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
                    None,
                )
            }
        };
        // A linear press begins a possible drag but owns no selection yet. Only movement creates
        // one, so click-no-drag cannot briefly feed copy-on-select or leave a zero-width selection.
        self.session.set_view_selection(initial);
        self.mouse_route = Some(MouseRoute::Local(SelectionDrag {
            mode,
            origin_row: hit.row,
            origin_column: hit.column,
            origin,
            hyperlink,
            open_hyperlink_on_release,
            local_image_activation,
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
        // A divider drag owns the pointer outright: while one is in flight the
        // terminal hears nothing, which is the same rule an in-progress
        // selection drag already lives by.
        if self.drive_divider_drag(position)? {
            return Ok(());
        }
        self.update_chrome_hover(position)?;
        // Everything below reads the pointer through `terminal_pointer` — one
        // correction, in one place, applied to hover, peek, selection and
        // protocol forwarding alike. Outside the seat it answers `None`, which
        // is exactly what a pointer outside the grid already answered, so the
        // clearing paths below run unchanged.
        let local = self.terminal_pointer();
        let position = local.unwrap_or(position);
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
        self.refresh_image_reference_underline()?;
        let peek_path = hit
            .filter(|_| {
                math_hit.is_none() && !matches!(self.mouse_route, Some(MouseRoute::Local(_)))
            })
            .and_then(|hit| self.peek_target(hit));
        if self.peek_hover.observe(peek_path, position, now) {
            self.present_peek_overlay(None)?;
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

    /// Advance a divider drag. Returns whether the pointer was consumed.
    ///
    /// Every frame re-reads the split's slot from the current solve and asks
    /// `bt-layout::apply` for the ratio: the clamp, and the refusal when the
    /// clamp is unsatisfiable, are §2.4's and are not re-derived here. Red line
    /// L9 is upheld by the edit itself — `DragDivider`'s focus set is exactly
    /// that one split, so nothing rebalances mid-gesture.
    fn drive_divider_drag(&mut self, position: PhysicalPosition<f64>) -> Result<bool> {
        let Some(drag) = self.divider_drag else {
            return Ok(false);
        };
        let Some(slot) = self
            .seats
            .split_slots(&self.seat_layout)
            .into_iter()
            .find(|slot| slot.id == drag.split)
        else {
            self.divider_drag = None;
            return Ok(false);
        };
        let along = match drag.dir {
            Axis::Row => position.x,
            Axis::Col => position.y,
        };
        let scale_ppm = seats::scale_ppm(self.renderer.metrics().dpi_milli().get());
        let Some((requested, usable)) = seats::requested_ratio(slot, scale_ppm, along) else {
            return Ok(true);
        };
        let metrics = self.seat_metrics();
        // A refusal, and a clamp that changed nothing, both mean "do not
        // re-solve": §2.4 rules that an infeasible drag has zero side effects
        // rather than writing a value the next solve will "correct", which
        // would dress a refusal up as a jitter.
        if self
            .seats
            .drag_divider(&metrics, drag.split, requested, usable)
            == Ok(true)
        {
            self.commit_seat_geometry()?;
        }
        Ok(true)
    }

    /// Repaint the chrome when the pointer moves onto or off a divider, a close
    /// affordance or a collapsed bar.
    fn update_chrome_hover(&mut self, position: PhysicalPosition<f64>) -> Result<()> {
        let scale = self.renderer.metrics().scale_factor as f32;
        let hover = seats::hit_chrome(
            &self.seats,
            &self.seat_layout,
            scale,
            position.x,
            position.y,
        );
        if self.seat_pointer.hover == hover {
            return Ok(());
        }
        self.seat_pointer.hover = hover;
        if self.refresh_chrome() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// Put the frame already on screen back in the slot so a pure chrome change
    /// reaches the glass. Chrome lives beside the frame, exactly as the peek
    /// flyout does, so `redraw` would otherwise find nothing queued and skip.
    fn present_chrome_change(&mut self) -> Result<()> {
        if self.pending_resize_present.is_none()
            && self.pending_frames.pending_frame().is_none()
            && let Some(frame) = self.last_presented_frame.clone()
        {
            self.pending_frames
                .publish(
                    frame,
                    FrameTrigger {
                        occurred_at: Instant::now(),
                        source: FrameSource::Expose,
                    },
                )
                .context("re-present the on-screen frame for a seat chrome change")?;
        }
        self.window.request_redraw();
        Ok(())
    }

    /// Route a press onto seat chrome. Returns whether the button was consumed.
    fn chrome_mouse_input(
        &mut self,
        state: ElementState,
        button: MouseButton,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        if button != MouseButton::Left {
            return Ok(false);
        }
        if state == ElementState::Released {
            let Some(drag) = self.divider_drag.take() else {
                return Ok(false);
            };
            let _ = drag;
            self.seat_pointer.dragging = None;
            if self.refresh_chrome() {
                self.present_chrome_change()?;
            }
            // The end of a drag is a meaningful change (§5.1): the ratio that
            // was being explored is now the ratio the user chose.
            self.mark_session_dirty(Instant::now());
            return Ok(true);
        }
        let scale = self.renderer.metrics().scale_factor as f32;
        let Some(target) = seats::hit_chrome(
            &self.seats,
            &self.seat_layout,
            scale,
            position.x,
            position.y,
        ) else {
            // Not on chrome, but possibly not on the terminal either — a press
            // in a preview's body belongs to that seat and must not reach the
            // grid underneath it. With a lone leaf there is no other seat for a
            // press to belong to, so nothing is claimed and every existing path
            // sees the button exactly as before.
            return Ok(!self.seats.is_lone_terminal()
                && !seats::terminal_contains(
                    &self.seat_layout,
                    self.seats.terminal(),
                    position.x,
                    position.y,
                ));
        };
        match target {
            seats::ChromeTarget::Divider(split) => {
                let Some(slot) = self
                    .seats
                    .split_slots(&self.seat_layout)
                    .into_iter()
                    .find(|slot| slot.id == split)
                else {
                    return Ok(true);
                };
                self.divider_drag = Some(DividerDrag {
                    split,
                    dir: slot.dir,
                });
                self.seat_pointer.dragging = Some(split);
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
            }
            seats::ChromeTarget::Close(seat) => {
                let metrics = self.seat_metrics();
                let closed_preview = self.seats.preview() == Some(seat);
                if self.seats.close_seat(&metrics, seat) {
                    if closed_preview {
                        self.preview_image = None;
                        self.renderer.set_preview_image(None);
                    }
                    self.seat_pointer = seats::ChromePointer::default();
                    self.apply_window_min_inner_size();
                    self.commit_seat_geometry()?;
                }
            }
            seats::ChromeTarget::CollapseBar(seat) => {
                // §2.6.3: clicking a collapsed bar expands it, by promoting it
                // to the focus — W2 then makes it the last seat to fall, and
                // the concession chain gives it the room by itself. Keyboard
                // focus does not move; v1 keeps that on the terminal.
                if self.seats.set_focus(seat) {
                    self.apply_window_min_inner_size();
                    self.commit_seat_geometry()?;
                }
            }
        }
        Ok(true)
    }

    fn mouse_input(&mut self, state: ElementState, button: MouseButton) -> Result<()> {
        if let Some(position) = self.pointer_position
            && self.chrome_mouse_input(state, button, position)?
        {
            return Ok(());
        }
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
                let (single_click, hyperlink_to_open, local_image_action) =
                    if let Some(MouseRoute::Local(SelectionDrag {
                        mode: SelectionDragMode::Linear,
                        origin_row,
                        origin_column,
                        hyperlink,
                        open_hyperlink_on_release,
                        local_image_activation,
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
                            local_image_activation
                                .path()
                                .is_some_and(|pressed| {
                                    release_local_image_path.as_deref() == Some(pressed)
                                })
                                .then(|| local_image_activation.clone()),
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
                if let Some(activation) = local_image_action {
                    match activation {
                        LocalImageActivation::None => {}
                        LocalImageActivation::Preview(path) => self.open_preview_image(path)?,
                        LocalImageActivation::External(path) => {
                            self.activate_local_image_path(&path)
                        }
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn mouse_wheel(&mut self, delta: MouseScrollDelta) -> Result<()> {
        // A notch over another seat is that seat's, not the terminal's. Guarded
        // on there being another seat at all, so a lone leaf scrolls exactly as
        // it always has — including before the pointer has ever moved.
        if !self.seats.is_lone_terminal()
            && let Some(position) = self.pointer_position
            && !seats::terminal_contains(
                &self.seat_layout,
                self.seats.terminal(),
                position.x,
                position.y,
            )
        {
            return Ok(());
        }
        // Scrolling moves the content the flyout was anchored to; the transient peek dissolves.
        self.dismiss_peek()?;
        // One physical event, two currencies. Local routes scroll by exact subpixels (stage C of
        // the pixel-scroll plan); forwarding routes speak whole wheel lines because that is the
        // application protocol. Route is decided first, then only that route's accumulator moves.
        let cell_subpixels = self.projection.cell_height_subpixels().get() as f64;
        let event_subpixels = match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                let multiplier =
                    match recoverable_wheel_scroll_amount(bt_platform::wheel_scroll_amount()) {
                        bt_platform::WheelScrollAmount::Lines(lines) => lines as f64,
                        bt_platform::WheelScrollAmount::Page => self.grid.rows.get() as f64,
                    };
                f64::from(y) * multiplier * cell_subpixels
            }
            MouseScrollDelta::PixelDelta(position) => {
                position.y * bt_viewport::SUBPIXELS_PER_PX as f64
            }
        };
        if let Some(math_hit) = self.math_hit() {
            // A hovered math block pans by whole pixels derived from the same exact motion, so
            // trackpads feel identical over blocks and text. The commit is tentative: nothing is
            // taken from the local accumulator until the block actually scrolls, because a
            // non-scrollable block falls through to the ordinary routes with the event intact.
            let mut tentative = self.local_wheel_subpixel_remainder + event_subpixels;
            let take_px = drain_whole_units(&mut tentative, bt_viewport::SUBPIXELS_PER_PX as f64);
            let delta_px = i32::try_from(-take_px).unwrap_or(0);
            if delta_px == 0 {
                self.local_wheel_subpixel_remainder = tentative;
                return Ok(());
            }
            let horizontal = if self.modifiers.shift_key() {
                delta_px
            } else {
                0
            };
            let vertical = if self.modifiers.shift_key() {
                0
            } else {
                delta_px
            };
            if self
                .session
                .scroll_math_block(&math_hit.anchor, horizontal, vertical)
            {
                self.local_wheel_subpixel_remainder = tentative;
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
            return self.scroll_view_exact(event_subpixels);
        }
        if !self.modifiers.shift_key() && modes.mouse_tracking != MouseTracking::Off {
            // Mouse-protocol wheel reports are per-notch, never per-system-scroll-line: the
            // application applies its own lines-per-event step, so multiplying by the Windows
            // wheel setting had TUIs (user report 2026-08-01: Claude Code transcript) scrolling
            // three times too far per notch.
            let notches = self.take_forward_wheel_notches(delta);
            if notches == 0 {
                return Ok(());
            }
            let Some(hit) = self.frame_hit() else {
                return Ok(());
            };
            let frame = self
                .last_presented_frame
                .as_ref()
                .context("missing frame for forwarded wheel hit")?;
            let hit = live_viewport_mouse_hit(frame, hit);
            let button = if notches > 0 {
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
                &one.repeat(notches.unsigned_abs() as usize),
                "forward SGR mouse wheel to PTY",
                UserInputKind::Mouse,
            );
        }
        if modes.alternate_screen {
            // Alternate-screen wheel emulation belongs to the application. Shift is the explicit
            // local override for reviewing projection-only rows displaced above this screen.
            if self.modifiers.shift_key() {
                return self.scroll_view_exact(event_subpixels);
            }
            if modes.alternate_scroll {
                let lines = self.take_forward_wheel_lines(delta);
                if lines == 0 {
                    return Ok(());
                }
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
        self.scroll_view_exact(event_subpixels)
    }

    /// Whole-line quantization for the alternate-scroll emulation route: arrow-key emulation
    /// mirrors the local scroll distance, so the system lines-per-notch setting applies here.
    /// Fractional motion parks in the forwarding accumulators, never the local subpixel one.
    fn take_forward_wheel_lines(&mut self, delta: MouseScrollDelta) -> i32 {
        match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                let multiplier =
                    match recoverable_wheel_scroll_amount(bt_platform::wheel_scroll_amount()) {
                        bt_platform::WheelScrollAmount::Lines(lines) => lines as f64,
                        bt_platform::WheelScrollAmount::Page => self.grid.rows.get() as f64,
                    };
                self.line_wheel_remainder += f64::from(y) * multiplier;
                drain_whole_units(&mut self.line_wheel_remainder, 1.0) as i32
            }
            MouseScrollDelta::PixelDelta(position) => {
                self.pixel_wheel_remainder += position.y;
                let cell_px = self.renderer.metrics().cell_height_px as f64;
                drain_whole_units(&mut self.pixel_wheel_remainder, cell_px) as i32
            }
        }
    }

    /// Per-notch quantization for the mouse-protocol route: one wheel report per detent, the
    /// xterm convention every TUI calibrates its own scroll step against. Trackpad pixel deltas
    /// emit one report per accrued cell height of travel.
    fn take_forward_wheel_notches(&mut self, delta: MouseScrollDelta) -> i32 {
        match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                self.notch_wheel_remainder += f64::from(y);
                drain_whole_units(&mut self.notch_wheel_remainder, 1.0) as i32
            }
            MouseScrollDelta::PixelDelta(position) => {
                self.pixel_wheel_remainder += position.y;
                let cell_px = self.renderer.metrics().cell_height_px as f64;
                drain_whole_units(&mut self.pixel_wheel_remainder, cell_px) as i32
            }
        }
    }

    /// Local pixel-exact wheel consumption: accumulate the event's fractional subpixels and
    /// scroll by the integral part. Positive subpixels move into history, matching the wheel's
    /// upward direction, and residue below one subpixel simply waits for the next event.
    fn scroll_view_exact(&mut self, event_subpixels: f64) -> Result<()> {
        self.local_wheel_subpixel_remainder += event_subpixels;
        let take = drain_whole_units(&mut self.local_wheel_subpixel_remainder, 1.0);
        if take == 0 {
            return Ok(());
        }
        self.projection.scroll_by_subpixels(take);
        self.publish_interaction_frame()
    }

    fn keyboard_input(&mut self, event: &KeyEvent) -> Result<()> {
        if event.state != ElementState::Pressed {
            return Ok(());
        }
        // Any keystroke dismisses the transient peek flyout (Esc included, per the peek verb
        // ruling) without consuming the key: typing means the user has moved on from hovering.
        self.dismiss_peek()?;

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
        // Dev-only: open or close the preview seat at its ruled fixed-right
        // address, so the layout can be felt before the verbs that will really
        // open it exist. Ctrl+Shift+P is a placeholder binding and is documented
        // as such; it is checked here, above the PTY encoder, so the chord never
        // reaches the child.
        if is_preview_toggle_shortcut(&event.logical_key, self.modifiers) {
            if !event.repeat {
                self.toggle_preview_seat()?;
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
        // Both halves of a visible flyout belong to the viewport that produced them: the anchor is
        // a physical point on the old surface, and the raster was sized to the old pane. A resize
        // dissolves it exactly as a wheel notch does; the retained thumbnail is re-derived, and
        // resampled if the new pane asks for another box, on the next settled hover. The frame
        // this handler publishes below is the repaint that drops it, so nothing is queued here:
        // the frame on screen belongs to the old grid and the resize gate would refuse it.
        self.peek_hover.clear();
        self.renderer.set_peek_overlay(None);
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
        // solve -> seat rects -> cols/rows -> the existing 200ms ConPTY quiet
        // coalescing -> LayoutKey. §4.2 fixes this order and forbids the reverse
        // (red line L10); the solver itself takes no part in the debounce.
        let next_grid = self.resolve_seat_layout(render_physical);
        let observed_at = Instant::now();
        // A `Resized` that settles back onto the grid ConPTY already has — the common shape of
        // the very first delivery after a clean, same-DPI session restore — must not schedule a
        // real ConPTY resize at all; see `coalesce_pty_resize_on_grid_change`.
        self.schedule_grid_change(
            next_grid,
            terminal_pty_physical(&self.renderer, render_physical),
            observed_at,
            "resize terminal actor",
        )?;
        self.sync_math_layout_key();
        // The grid actually in force, which under the typed-input gate is still the old one.
        self.pending_resize_present = Some(self.grid);
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
        // Touching the surface is what obliges a solve, not changing the DPI:
        // every seat rectangle is a function of the surface, so the answer that
        // was true of the old one is not yet true of this one. The equal-scale
        // path below returns early and this method publishes a frame, so the
        // solve has to happen here rather than after that branch — otherwise the
        // frame is drawn against a rectangle nobody re-derived, which is how the
        // terminal came to be drawn over its neighbour's seat. `solve` is pure
        // and the tree has not changed, so on the common no-op path this is the
        // same answer arrived at again.
        if physical.width > 0 && physical.height > 0 {
            self.resolve_seat_layout(render_physical);
        }
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
            // A DPI change is a similarity transform: the same tree solved again
            // on a new rectangle. Red line L5 — not one ratio, not one fixed
            // extent is rewritten on this path, and `resolve_seat_layout` writes
            // none because `solve` is pure.
            self.refresh_work_area();
            self.apply_window_min_inner_size();
            let next_grid = self.resolve_seat_layout(render_physical);
            self.schedule_grid_change(
                next_grid,
                terminal_pty_physical(&self.renderer, render_physical),
                Instant::now(),
                "rebuild terminal grid after authoritative DPI correction",
            )?;
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
                frame.grid_rows
            );
        }
        let has_text = frame
            .cells
            .iter()
            .take(
                frame
                    .drawable_rows()
                    .saturating_mul(frame.columns.get() as usize),
            )
            .any(|cell| !cell.text.trim().is_empty());
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
                        frame.grid_rows
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
        // The clean-exit path (§5.5): flush whatever the debounce still owes,
        // then drop this run's sentinel. Its absence next time is the whole
        // signal that this run reached here at all, so it must be removed
        // before anything below is allowed to fail.
        self.mark_session_dirty(Instant::now());
        self.session_store.close();
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
        runtime.session_store.flush_if_due(now);
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
        if let Err(error) = runtime.activate_peek_if_due(now) {
            self.fail(event_loop, error);
            return;
        }
        if let Err(error) = runtime.clear_math_hover_if_due(now) {
            self.fail(event_loop, error);
            return;
        }
        // Service the PTY gate after every other due task that can mutate session state, then carry
        // the deadline derived from that exact sample into the control-flow decision below.
        let pty_resize_deadline = match runtime.flush_pending_pty_resize(now) {
            Ok(deadline) => deadline,
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        };
        let startup_deadline =
            startup_poll_delay(runtime.first_text_presented).map(|delay| now + delay);
        let wake_deadline = [
            startup_deadline,
            runtime.ime_cursor_throttle.deadline(),
            pty_resize_deadline,
            runtime.session.resize_finish_deadline(),
            runtime.session.synchronized_update_deadline(),
            runtime.session.live_stability_deadline(),
            runtime.hyperlink_hover.show_at,
            runtime.peek_hover.show_at,
            runtime.math_hover_clear_at,
            runtime.session_store.deadline(),
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

/// Takes as many whole `unit`s as the accumulated remainder holds and returns the signed count,
/// leaving the sub-unit residue in place. Truncation is symmetric around zero, so reversing the
/// wheel never manufactures motion from residue of the opposite sign.
fn drain_whole_units(remainder: &mut f64, unit: f64) -> i64 {
    let units = (*remainder / unit).trunc() as i64;
    *remainder -= units as f64 * unit;
    units
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
        && frame.grid_rows.get() == u32::from(grid.rows.get())
}

fn presentation_equivalent(previous: &ViewportFrame, next: &ViewportFrame) -> bool {
    previous.columns == next.columns
        && previous.grid_rows == next.grid_rows
        && previous.rows == next.rows
        && previous.presentation_offset_subpixels == next.presentation_offset_subpixels
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

/// The dev-only preview toggle: `Ctrl+Shift+P`.
///
/// Matched on the *character* the layout produced rather than on a physical key
/// so it behaves the same on every keyboard layout, and required to carry both
/// Ctrl and Shift and nothing else — a bare `Ctrl+P` is a real terminal control
/// byte (DLE) and must keep reaching the child.
fn is_preview_toggle_shortcut(key: &Key, modifiers: ModifiersState) -> bool {
    modifiers == ModifiersState::CONTROL | ModifiersState::SHIFT
        && matches!(key, Key::Character(text) if text.eq_ignore_ascii_case("p"))
}

/// Solve the tree against the current surface, and pick out the terminal seat.
///
/// The whole §4.1 conversion lives here and nowhere else: physical pixels in,
/// logical viewport, one `solve`, seat rectangles out. The solver is pure and
/// `O(seats)`, so calling it on every resize and every frame of a divider drag
/// is the preferred implementation rather than something to optimise away
/// (§4.4) — one geometry beats a cached one.
fn solve_seats(
    seats: &seats::Seats,
    renderer: &Renderer,
    render_physical: PhysicalSize<u32>,
) -> (SeatLayout, SeatViewport) {
    let dpi_milli = renderer.metrics().dpi_milli().get();
    let metrics = seats::seat_metrics(dpi_milli);
    let viewport = seats::logical_viewport(
        render_physical.width,
        render_physical.height,
        seats::scale_ppm(dpi_milli),
    );
    let layout = seats
        .solve(viewport, &metrics)
        .unwrap_or_else(|_| seats::fit_what_fits(seats, viewport, &metrics));
    let terminal = seats::seat_viewport(&layout, seats.terminal()).unwrap_or(SeatViewport::whole(
        render_physical.width.max(1),
        render_physical.height.max(1),
    ));
    (layout, terminal)
}

/// A caret rectangle the frame produced, moved from the terminal seat's
/// coordinates into the window's — what winit and IMM32 are asking for.
///
/// The identity for a lone leaf is the point: its seat origin is `(0, 0)`, so
/// this is the number that was passed before seats existed.
fn window_ime_cursor_area(seat: SeatViewport, area: ImeCursorArea) -> ImeCursorArea {
    ImeCursorArea {
        x: area.x.saturating_add_unsigned(seat.x),
        y: area.y.saturating_add_unsigned(seat.y),
        ..area
    }
}

/// The pixel size ConPTY is told about: the terminal *seat's*, not the window's.
/// They are the same number for a lone leaf.
fn terminal_pty_physical(renderer: &Renderer, window: PhysicalSize<u32>) -> PhysicalSize<u32> {
    let seat = renderer.seat_viewport();
    if seat.width == 0 || seat.height == 0 {
        return window;
    }
    PhysicalSize::new(seat.width, seat.height)
}

/// Where a restored window should open, or `None` to let the OS decide.
///
/// docs/M2-persistence-schema-v1.md §3.1: hitting the recorded monitor and the
/// recorded logical coordinates is best effort, but "does not crash, does not
/// land off-screen" is the hard floor. So a rectangle that no monitor can see
/// forfeits its position — the size is still honoured, because a size is never
/// off-screen.
fn restore_window_placement(
    event_loop: &ActiveEventLoop,
    session: &SessionV1,
) -> Option<(LogicalPosition<f64>, LogicalSize<f64>)> {
    // An empty tab list is the first run: there is nothing to restore, and the
    // product's opening size stands.
    if session.tabs.is_empty() {
        return None;
    }
    let bounds = session.window.bounds;
    let size = LogicalSize::new(f64::from(bounds.width), f64::from(bounds.height));
    let position = LogicalPosition::new(f64::from(bounds.x), f64::from(bounds.y));
    let visible = event_loop.available_monitors().any(|monitor| {
        let scale = monitor.scale_factor().max(f64::MIN_POSITIVE);
        let origin = monitor.position();
        let extent = monitor.size();
        let left = f64::from(origin.x) / scale;
        let top = f64::from(origin.y) / scale;
        let right = left + f64::from(extent.width) / scale;
        let bottom = top + f64::from(extent.height) / scale;
        // "Some of the window is reachable" rather than "all of it fits": a
        // window whose title bar is on screen can always be dragged the rest of
        // the way, and demanding containment would move windows the user
        // deliberately parked half off a monitor.
        position.x < right
            && position.y < bottom
            && position.x + size.width > left
            && position.y + size.height > top
    });
    Some((position, size)).filter(|_| visible)
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
            id: None,
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
            local_image_activation: LocalImageActivation::None,
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
    fn local_image_click_routes_preview_external_and_no_effect() {
        let verified = std::path::Path::new(r"C:\tmp\decoded.png");
        assert_eq!(
            local_image_activation(false, true, Some(verified)),
            LocalImageActivation::Preview(verified.to_path_buf()),
            "plain click carries the exact hit path into preview"
        );
        assert_eq!(
            local_image_activation(true, true, Some(verified)),
            LocalImageActivation::External(verified.to_path_buf()),
            "Ctrl+click retains the system-viewer verb"
        );
        assert_eq!(
            local_image_activation(false, true, None),
            LocalImageActivation::None,
            "an unmarked cell has no click side effect"
        );
        assert_eq!(
            local_image_activation(false, false, Some(verified)),
            LocalImageActivation::None,
            "dragging remains selection"
        );
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

        // The underline is the immediate affordance: a fresh candidate republishes right away and
        // is the underline target long before the tooltip deadline; only the status text waits.
        assert!(hover.observe(Some(link.clone()), start));
        assert_eq!(hover.underline_target(), Some(&link));
        assert!(hover.active.is_none(), "tooltip must not appear instantly");
        assert!(!hover.activate_if_due(start + Duration::from_millis(299)));
        assert!(hover.activate_if_due(start + Duration::from_millis(300)));
        assert_eq!(
            hover.status_text(80).as_deref(),
            Some("file:///actual-target")
        );
        assert!(hover.observe(None, start + Duration::from_millis(301)));
        assert!(hover.active.is_none());
        assert!(hover.underline_target().is_none());
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
    fn peek_hover_settles_after_the_delay_and_slides_along_one_span_without_restarting() {
        let start = Instant::now();
        let path = PathBuf::from(r"C:\img\a.png");
        let mut hover = PeekHover::default();

        let subject = PeekSubject::from_path(path.clone());
        assert!(!hover.observe(
            Some(subject.clone()),
            PhysicalPosition::new(10.0, 10.0),
            start
        ));
        assert!(
            hover
                .activate_if_due(start + Duration::from_millis(299))
                .is_none()
        );
        // Sliding along the same path span refreshes the anchor but keeps the original clock:
        // the flyout settles where the pointer last was, without ever restarting the delay.
        assert!(!hover.observe(
            Some(subject.clone()),
            PhysicalPosition::new(30.0, 12.0),
            start + Duration::from_millis(200)
        ));
        let settled = hover
            .activate_if_due(start + Duration::from_millis(300))
            .expect("original deadline must fire");
        assert_eq!(settled.subject, subject);
        assert_eq!(settled.pointer.x, 30.0);
        // While active, staying on the span neither hides nor re-arms.
        assert!(!hover.observe(
            Some(subject.clone()),
            PhysicalPosition::new(31.0, 12.0),
            start + Duration::from_millis(400)
        ));
        assert!(hover.show_at.is_none());
        // Leaving the span hides the flyout and drops all state.
        assert!(hover.observe(
            None,
            PhysicalPosition::new(31.0, 40.0),
            start + Duration::from_millis(500)
        ));
        assert!(hover.active.is_none());
    }

    #[test]
    fn peek_hover_switching_paths_hides_the_old_flyout_and_restarts_the_clock() {
        let start = Instant::now();
        let first = PeekSubject::from_path(PathBuf::from(r"C:\img\a.png"));
        let second = PeekSubject::from_path(PathBuf::from(r"C:\img\b.png"));
        let mut hover = PeekHover::default();
        hover.observe(Some(first), PhysicalPosition::new(10.0, 10.0), start);
        assert!(
            hover
                .activate_if_due(start + Duration::from_millis(300))
                .is_some()
        );
        let hidden = hover.observe(
            Some(second.clone()),
            PhysicalPosition::new(50.0, 10.0),
            start + Duration::from_millis(400),
        );
        assert!(hidden, "switching spans must hide the visible flyout");
        assert!(
            hover
                .activate_if_due(start + Duration::from_millis(600))
                .is_none(),
            "the second span runs a fresh settle clock"
        );
        let settled = hover
            .activate_if_due(start + Duration::from_millis(700))
            .expect("second span settles on its own deadline");
        assert_eq!(settled.subject, second);
    }

    /// PIN (user repro 2026-08-02, re-seated by the frame-derived ruling 2026-08-04): one hovered
    /// cell resolves to one reference, through the frame's own row stride, and where a printed path
    /// and a link target cover the same cell the pointer answers with the text it is standing on.
    ///
    /// What the four verbs share is this lookup: the scan produced the list, and hover, click and
    /// peek all ask it the same question about the same `GridHit`. The list's own contents — which
    /// shapes are in it, and that a `file://` to a `.txt` is in none — are bt-term's to pin, beside
    /// the detector that decides it (`underline_coverage_equals_peek_coverage_for_every_shape`).
    #[test]
    fn one_hit_resolves_to_one_reference_through_the_frames_own_stride() {
        let printed = PathBuf::from(r"D:\from-text.png");
        let linked = PathBuf::from(r"D:\layout-preview.png");
        let references = FrameImageReferences {
            columns: 10,
            references: vec![
                // The scan's order: printed text first, link targets after it.
                bt_term::FrameImageReference {
                    path: printed.clone(),
                    cells: vec![12, 13, 14],
                    verified: true,
                },
                bt_term::FrameImageReference {
                    path: linked.clone(),
                    cells: vec![10, 11, 12, 13, 14, 15],
                    verified: true,
                },
            ],
        };
        let at = |row, column| {
            references
                .at(bt_render::GridHit { row, column })
                .map(|reference| reference.path.clone())
        };
        assert_eq!(
            at(1, 2),
            Some(printed),
            "text under the pointer is what the pointer was put on",
        );
        assert_eq!(
            at(1, 0),
            Some(linked),
            "and the link answers where its label spells no file",
        );
        assert_eq!(at(1, 6), None, "one column past the link is ordinary text");
        assert_eq!(at(0, 2), None, "the row is part of the address");
        assert_eq!(
            FrameImageReferences::default().at(bt_render::GridHit { row: 0, column: 0 }),
            None,
            "a frame with no references answers nothing anywhere",
        );
    }

    /// PIN (band retirement ruling, 2026-08-03, docs §6.1): the peek's third source is an OSC 1337
    /// payload, which names no file, and the pipeline tells the two apart by exactly one property —
    /// whether a cache miss has anything to read.
    ///
    /// A named file's identity is its normalized path, so the same file spelled two ways is one
    /// hover and one cache entry. A payload's identity is the decoder's content key, and it carries
    /// no path at all: the bytes came through the stream and were remembered when the decode landed,
    /// so a miss is a hover that arrived early, never a disk read to schedule. That `path: None` is
    /// the whole of the difference is what keeps `show_or_request_peek` one function.
    #[test]
    fn a_stream_payload_is_a_peek_subject_with_nothing_to_read() {
        let by_path = PeekSubject::from_path(PathBuf::from(r"C:\img\a.png"));
        assert_eq!(
            by_path.key,
            normalized_local_image_path_key(std::path::Path::new(r"C:\img\a.png")),
            "a named file is identified the way the decoder identifies it",
        );
        assert!(by_path.path.is_some(), "a named file is readable on a miss");
        assert_eq!(
            PeekSubject::from_path(PathBuf::from(r"C:\IMG\A.PNG")),
            by_path,
            "one file spelled two ways is one hover and one cache entry",
        );

        let payload = PeekSubject::from_content_key("image:sha-abc".to_owned());
        assert_eq!(payload.key, "image:sha-abc");
        assert!(
            payload.path.is_none(),
            "a stream payload has no file behind it, so a cache miss reads nothing",
        );
        assert_ne!(payload, by_path);
    }

    /// PIN (verification ruling 2026-08-04, the warm peek): the decode a verified reference already
    /// paid for is filed under the very key the hover looks up, so the flyout opens from cache and
    /// no second read of the same file is ever scheduled.
    ///
    /// `show_or_request_peek` sends a `PeekImage` task on exactly one condition — a `None` entry
    /// under `PeekSubject::key`. So "the peek is warm" and "the two keys are the same string" are
    /// the same statement, and it is the one asserted here. The stream-payload arm is asserted
    /// beside it because both shapes go through this one function and must not converge: a payload
    /// has no path to key by.
    ///
    /// RED CHECK: keying a named file's verification decode by `decoded.key` (its content identity)
    /// instead of its path leaves the hover's lookup missing, and the first assertion goes red —
    /// which is precisely the "decoded twice, cached twice" defect the shared key rules out.
    #[test]
    fn a_verified_references_decode_is_filed_under_the_key_the_hover_asks_by() {
        let path = PathBuf::from(r"C:\img\Sunset.PNG");
        let decoded = bt_term::DecodedInlineImage {
            occurrence_id: 7,
            key: "image:0123456789abcdef0123456789abcdef".to_owned(),
            rgba: Arc::from(vec![0u8; 4]),
            width_px: 1,
            height_px: 1,
            animated: false,
        };

        assert_eq!(
            peek_cache_key_for_decode(
                &bt_term::InlineImageSource::LocalPath(path.clone()),
                &decoded
            ),
            PeekSubject::from_path(path.clone()).key,
            "the verification decode lands exactly where the hover will look for it",
        );
        assert_eq!(
            peek_cache_key_for_decode(
                &bt_term::InlineImageSource::LocalPath(PathBuf::from(r"c:/img/sunset.png")),
                &decoded
            ),
            PeekSubject::from_path(path).key,
            "and one file spelled two ways is still one warm entry",
        );
        assert_eq!(
            peek_cache_key_for_decode(
                &bt_term::InlineImageSource::Osc1337(b"AAAA".to_vec()),
                &decoded
            ),
            PeekSubject::from_content_key(decoded.key.clone()).key,
            "a stream payload has no path, so it stays keyed by content",
        );
    }

    /// Pin (a) of the peek raster defect: every peek pixel that reaches the renderer is one the
    /// flyout draws. The chain the app runs — the renderer's box, the worker's resample, the
    /// thumbnail slot, the overlay — is walked end to end here, so a future edit that hands the
    /// renderer a native decode again fails on the resident byte count and on the texture key.
    #[test]
    fn the_peek_overlay_carries_display_sized_pixels_under_a_display_sized_key() {
        // A decode far larger than any flyout: 1024x768 in a 640x480 pane.
        let (native_width_px, native_height_px) = (1024_u32, 768_u32);
        let native_rgba: Arc<[u8]> =
            Arc::from(vec![
                0x40_u8;
                native_width_px as usize * native_height_px as usize * 4
            ]);
        let content_key = "image:0123456789abcdef0123456789abcdef".to_owned();

        let (display_width_px, display_height_px) = bt_render::peek_thumbnail_extent(
            640.0,
            480.0,
            8.0,
            1.0,
            native_width_px,
            native_height_px,
        )
        .expect("the pane can host the flyout");
        assert!(
            display_width_px < native_width_px && display_height_px < native_height_px,
            "the 40% cap is what makes the flyout smaller than its decode",
        );

        let target: PeekThumbnailTarget =
            (content_key.clone(), display_width_px, display_height_px);
        let task = peek_scale_task(
            &target,
            Arc::clone(&native_rgba),
            native_width_px,
            native_height_px,
        );
        let thumbnail = PeekThumbnail::from_scaled(bt_term::scale_inline_image(&task));
        let overlay = thumbnail.overlay(PhysicalPosition::new(120.0, 90.0));
        assert_eq!(
            (overlay.width_px, overlay.height_px),
            (display_width_px, display_height_px),
        );
        assert_eq!(
            overlay.rgba.len(),
            display_width_px as usize * display_height_px as usize * 4,
            "the resident bytes the renderer uploads are the display box, not the decode",
        );
        assert!(
            overlay.rgba.len() * 16 < native_rgba.len(),
            "the defect uploaded {} bytes where {} suffice",
            native_rgba.len(),
            overlay.rgba.len(),
        );
        assert_eq!(
            overlay.key,
            bt_term::display_texture_key(&content_key, display_width_px, display_height_px),
            "the display size is part of the texture identity, so the shared LRU can never \
             serve a raster sized for another box",
        );
        assert!(
            thumbnail.matches(&target),
            "the slot answers the question the hover asked, so a raster for another box is \
             never presented as this one",
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
    fn a_resize_reflow_holds_until_projectable_staging_reanchors_the_reprint() {
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

        // The reprint enters projectable resize staging: publication resumes immediately at the
        // restored review position — a direct hand-off with no bottom frame ever presented.
        harness
            .session
            .feed_at(&lines, start + Duration::from_millis(30))
            .unwrap();
        assert!(
            harness.publish_pty_frame(),
            "resize staging releases the hold as soon as the reprint is reachable"
        );
        harness.present_pending();
        assert_eq!(
            harness.last_presented.as_ref().unwrap().scroll_offset_rows,
            20,
            "presentation resumes exactly at the staged review displacement"
        );

        // Quiescence commits the same staging ids through normal history relocation and must not
        // move the already-restored reading position.
        assert!(
            harness
                .session
                .finish_resize_if_quiescent(start + Duration::from_millis(280))
                .unwrap()
        );
        if harness.publish_pty_frame() {
            harness.present_pending();
        }
        assert_eq!(
            harness.last_presented.as_ref().unwrap().scroll_offset_rows,
            20,
            "final harvest preserves the already-restored review displacement"
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
    fn wheel_accumulator_preserves_fractional_residue_across_events() {
        // Eight trackpad ticks of 0.375 units (binary-exact) must add up to exactly 3 whole
        // units drained, never 0 (per-event truncation) and never 4 (double counting).
        let mut remainder = 0.0;
        let mut drained = 0;
        for _ in 0..8 {
            remainder += 0.375;
            drained += drain_whole_units(&mut remainder, 1.0);
        }
        assert_eq!(drained, 3);
        assert_eq!(remainder, 0.0);

        // Whole-line wheel notches with a 17px cell drain exactly one line per 17px, residue 8.
        let mut pixels = 25.0;
        assert_eq!(drain_whole_units(&mut pixels, 17.0), 1);
        assert!((pixels - 8.0).abs() < 1e-9);
    }

    #[test]
    fn wheel_accumulator_truncates_symmetrically_and_never_flips_sign_on_reversal() {
        // +0.6 then -0.7: neither direction has accrued a whole unit, so nothing drains and the
        // residue nets out — reversal must not manufacture a step from opposite-sign residue.
        let mut remainder = 0.0;
        remainder += 0.6;
        assert_eq!(drain_whole_units(&mut remainder, 1.0), 0);
        remainder += -0.7;
        assert_eq!(drain_whole_units(&mut remainder, 1.0), 0);
        assert!((remainder + 0.1).abs() < 1e-9);

        // A full downward unit drains as -1 with the same magnitude rules as upward.
        let mut down = -1.4;
        assert_eq!(drain_whole_units(&mut down, 1.0), -1);
        assert!((down + 0.4).abs() < 1e-9);
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

    /// The caret rectangle leaves the frame in the terminal seat's coordinates
    /// and reaches winit and IMM32 in the window's. This is the one place the
    /// seat correction runs in that direction, so it is pinned in both: a lone
    /// leaf's origin is `(0, 0)` and the number is unchanged, and a seat that
    /// has been moved carries the caret with it.
    ///
    /// Red gate: return `area` unchanged and the second case fails — the
    /// candidate window would open a seat's width away from the caret.
    #[test]
    fn the_ime_caret_leaves_the_seat_in_the_windows_coordinates() {
        let area = ImeCursorArea {
            x: 250,
            y: 100,
            width: 18,
            height: 44,
        };
        assert_eq!(
            window_ime_cursor_area(SeatViewport::whole(1920, 1200), area),
            area,
            "a lone leaf's seat is the window, so nothing moves"
        );
        let moved = window_ime_cursor_area(
            SeatViewport {
                x: 976,
                y: 0,
                width: 944,
                height: 1200,
            },
            area,
        );
        assert_eq!(
            moved,
            ImeCursorArea {
                x: 250 + 976,
                y: 100,
                width: 18,
                height: 44,
            },
            "a seat with an origin carries the caret to the window's axis"
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
    fn private_resize_repaint_input_is_exact_and_integration_gated() {
        assert_eq!(
            psreadline_resize_repaint_input(true),
            Some(PSREADLINE_INVOKE_PROMPT_INPUT)
        );
        assert_eq!(
            psreadline_resize_repaint_input(false),
            None,
            "a session without an open OSC 133 input region injects zero bytes"
        );
    }

    #[test]
    fn resize_storm_reanchor_debt_is_replaced_and_paid_once() {
        let mut pending = false;
        for _ in 0..3 {
            replace_psreadline_resize_reanchor_debt(&mut pending, true);
        }
        assert_eq!(
            take_psreadline_resize_reanchor_input(&mut pending, true),
            Some(PSREADLINE_INVOKE_PROMPT_INPUT),
            "three commits in one open-input transaction coalesce to one chord"
        );
        assert_eq!(
            take_psreadline_resize_reanchor_input(&mut pending, true),
            None,
            "the repair debt is one shot"
        );

        replace_psreadline_resize_reanchor_debt(&mut pending, true);
        replace_psreadline_resize_reanchor_debt(&mut pending, false);
        assert_eq!(
            take_psreadline_resize_reanchor_input(&mut pending, true),
            None,
            "a later closed-region commit replaces stale open-prompt debt"
        );
        replace_psreadline_resize_reanchor_debt(&mut pending, true);
        assert_eq!(
            take_psreadline_resize_reanchor_input(&mut pending, false),
            None,
            "a prompt that closes before quiescence receives no stale chord"
        );
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

    /// The pure half of `solve_seats` (main.rs's own free function), parameterized on a
    /// `dpi_milli` instead of `&Renderer` so a startup scenario can be solved without a live GPU
    /// device. Numerically identical to what `Runtime::create` and `resolve_seat_layout` do with
    /// an actual renderer in hand — see `solve_seats`'s own body.
    fn solved_terminal_seat(
        seats: &seats::Seats,
        dpi_milli: u32,
        render_physical: PhysicalSize<u32>,
    ) -> bt_render::SeatViewport {
        let metrics = seats::seat_metrics(dpi_milli);
        let viewport = seats::logical_viewport(
            render_physical.width,
            render_physical.height,
            seats::scale_ppm(dpi_milli),
        );
        let layout = seats
            .solve(viewport, &metrics)
            .unwrap_or_else(|_| seats::fit_what_fits(seats, viewport, &metrics));
        seats::seat_viewport(&layout, seats.terminal()).unwrap_or(bt_render::SeatViewport::whole(
            render_physical.width.max(1),
            render_physical.height.max(1),
        ))
    }

    /// PIN (startup order): a session restore with a preview seat open must spawn ConPTY at the
    /// seat's own grid and ask it for nothing more.
    ///
    /// `Runtime::create` resolves the seat layout (`seats::Seats::from_persisted` -> `solve_seats`)
    /// *before* `PtySession::spawn_default`, and seeds `self.grid` to that same solve's grid. §4.2
    /// says solve is pure, so the very first re-solve after `ShowWindow`
    /// (`reconcile_authoritative_dpi`) — run against the identical tree and an identical,
    /// same-DPI viewport — reproduces the identical seat rectangle, and therefore the identical
    /// `GridSize`. `coalesce_pty_resize_on_grid_change` is the single point every later solve
    /// (a live OS `Resized`, a divider drag, a DPI reconciliation) funnels through; fed the exact
    /// pair a clean restore produces, it must schedule nothing.
    #[test]
    fn a_restored_split_tree_reaches_zero_pty_resize_requests_after_a_matching_dpi_spawn() {
        // The shape a preview-narrowed terminal round-trips to `session.json` as: a row split,
        // the terminal first, a pinned preview second (`seats.rs`'s `LeafNodeV1::Preview` docs).
        let node = bt_persist::LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
            dir: bt_persist::SplitDirV1::Row,
            ratio: 700_000,
            children: [
                Box::new(bt_persist::LayoutNodeV1::Leaf(
                    bt_persist::LeafNodeV1::Term(bt_persist::TermLeafV1 {
                        profile_id: "pwsh.exe".to_owned(),
                        cwd: String::new(),
                        manual_name: None,
                    }),
                )),
                Box::new(bt_persist::LayoutNodeV1::Leaf(
                    bt_persist::LeafNodeV1::Preview(bt_persist::PreviewLeafV1 { pinned: true }),
                )),
            ],
        });
        let seats = seats::Seats::from_persisted(&node).expect("the tree carries a terminal leaf");
        let dpi_milli = 1_000_u32; // the restored session's recorded DPI equals the monitor's at show
        let render_physical = PhysicalSize::new(1600, 900);

        // The spawn-time solve (before `PtySession::spawn_default`) and the post-`ShowWindow`
        // re-solve, run back to back exactly as startup does.
        let spawn_rect = solved_terminal_seat(&seats, dpi_milli, render_physical);
        let resolved_rect = solved_terminal_seat(&seats, dpi_milli, render_physical);
        assert_eq!(
            spawn_rect, resolved_rect,
            "an unchanged tree against an unchanged viewport must solve to the same seat twice"
        );
        assert!(
            spawn_rect.width < render_physical.width,
            "the preview seat must actually narrow the terminal, or this pin proves nothing"
        );

        // `CellMetrics::grid_for_pixels` is a pure function of the seat rectangle and the
        // (unchanged) font metrics, so an identical rectangle answers an identical `GridSize` on
        // both solves — that arithmetic is already pinned in `bt-render`. What this test pins is
        // the gate downstream of it, fed the one pair of grids a clean restore ever produces.
        let seat_grid = GridSize {
            columns: std::num::NonZeroU16::new(100).unwrap(),
            rows: std::num::NonZeroU16::new(30).unwrap(),
        };
        let physical = PhysicalSize::new(spawn_rect.width, spawn_rect.height);

        // `Runtime::create` seeds `self.grid` to exactly the spawn-time grid; the first
        // post-show solve compares against that same value.
        let current_grid = seat_grid;
        let mut pending = None;
        let now = Instant::now();
        let scheduled = coalesce_pty_resize_on_grid_change(
            &mut pending,
            seat_grid,
            current_grid,
            physical,
            now,
        );
        assert!(
            !scheduled,
            "the first post-spawn solve answers the exact grid the PTY was spawned with"
        );
        assert!(
            take_due_pty_resize(&mut pending, now + WINDOW_RESIZE_QUIET).is_none(),
            "spawn size already equals the seat grid; a matching-DPI restore must schedule zero \
             ConPTY resizes"
        );
    }

    /// RED-CHECK for the pin above: proves it is not vacuous. The pre-fix `resize()` and
    /// `commit_seat_geometry()` called `coalesce_pty_resize` unconditionally — the old
    /// spawn-then-resize shape this pin exists to forbid — which schedules a real ConPTY resize
    /// even when the grid the PTY was spawned with never moved. Restoring that unconditional call
    /// at the two real call sites is exactly what turns the pin above red.
    #[test]
    fn the_old_unconditional_coalesce_would_have_scheduled_a_resize_for_an_unchanged_grid() {
        let grid = GridSize {
            columns: std::num::NonZeroU16::new(100).unwrap(),
            rows: std::num::NonZeroU16::new(30).unwrap(),
        };
        let mut pending = None;
        let now = Instant::now();
        // The old shape: no `next_grid != current_grid` gate at all.
        coalesce_pty_resize(&mut pending, grid, PhysicalSize::new(1000, 700), now);
        assert!(
            take_due_pty_resize(&mut pending, now + WINDOW_RESIZE_QUIET).is_some(),
            "an unconditional coalesce call schedules a resize even for an unchanged grid"
        );
    }

    /// The scheduling half of `Runtime`, with no GPU in it.
    ///
    /// Every decision below is taken by the *production* functions — `plan_grid_change`,
    /// `release_due_pty_resize`, `pty_resize_wake_deadline` — and the gate answer comes from a real
    /// `DualPlaneSession` fed real OSC 133 bytes, not from a bool a test invented. What the harness
    /// itself owns is only the bookkeeping the runtime does around them: applying the reflow to the
    /// session, and recording the ConPTY requests that were actually issued.
    struct ResizeGateHarness {
        session: DualPlaneSession,
        pending: Option<PendingPtyResize>,
        grid: GridSize,
        conpty: GridSize,
        requests: Vec<GridSize>,
        typed_input_resize_deferral: bool,
    }

    impl ResizeGateHarness {
        fn new(columns: u16, rows: u16) -> Self {
            Self::with_policy(columns, rows, TYPED_INPUT_RESIZE_DEFERRAL)
        }

        fn with_policy(columns: u16, rows: u16, typed_input_resize_deferral: bool) -> Self {
            let grid = grid_of(columns, rows);
            Self {
                session: DualPlaneSession::new(
                    NonZeroU32::from(grid.columns),
                    NonZeroU32::from(grid.rows),
                ),
                pending: None,
                grid,
                conpty: grid,
                requests: Vec::new(),
                typed_input_resize_deferral,
            }
        }

        fn deferring(&self) -> bool {
            typed_input_resize_deferral_active(
                self.typed_input_resize_deferral,
                true,
                self.session.typed_shell_input_live(),
            )
        }

        fn feed(&mut self, bytes: &[u8], at: Instant) {
            self.session.feed_at(bytes, at).unwrap();
        }

        /// One `WindowEvent::Resized` worth of work: the seat has already moved, the solve has
        /// answered `columns`, and this is everything `Runtime::resize` does with that answer.
        fn drag_to(&mut self, columns: u16, at: Instant) {
            let next = grid_of(columns, self.grid.rows.get());
            let deferred = self.deferring();
            if let Some(reflow) = plan_grid_change(
                &mut self.pending,
                next,
                self.conpty,
                self.grid,
                PhysicalSize::new(u32::from(columns) * 8, 600),
                at,
                deferred,
            ) {
                self.session
                    .resize(
                        NonZeroU32::from(reflow.columns),
                        NonZeroU32::from(reflow.rows),
                    )
                    .unwrap();
                self.grid = reflow;
            }
        }

        /// One `about_to_wait`: drain has already happened, so the gate is asked again here.
        fn tick(&mut self, at: Instant) {
            let deferred = self.deferring();
            let (pending, _) = service_pending_pty_resize(&mut self.pending, at, deferred);
            let Some(pending) = pending else {
                return;
            };
            if pending.grid != self.grid {
                self.session
                    .resize(
                        NonZeroU32::from(pending.grid.columns),
                        NonZeroU32::from(pending.grid.rows),
                    )
                    .unwrap();
                self.grid = pending.grid;
            }
            self.conpty = pending.grid;
            self.requests.push(pending.grid);
        }

        fn wake_deadline(&self, now: Instant) -> Option<Instant> {
            pty_resize_wake_deadline(self.pending, self.deferring(), now)
        }
    }

    fn grid_of(columns: u16, rows: u16) -> GridSize {
        GridSize {
            columns: std::num::NonZeroU16::new(columns).unwrap(),
            rows: std::num::NonZeroU16::new(rows).unwrap(),
        }
    }

    /// POLICY PIN (user ruling 2026-08-06): typed input does not hold a resize by default. The
    /// local grid follows the drag immediately, while ConPTY still receives exactly the coalesced
    /// final size at the ordinary quiet boundary.
    #[test]
    fn typed_input_resize_deferral_is_off_and_live_resize_is_the_default() {
        const {
            assert!(!TYPED_INPUT_RESIZE_DEFERRAL);
        }
        assert!(!typed_input_resize_deferral_active(false, true, true));
        assert!(typed_input_resize_deferral_active(true, true, true));
        assert!(!typed_input_resize_deferral_active(true, false, true));
        assert!(!typed_input_resize_deferral_active(true, true, false));

        let start = Instant::now();
        let mut harness = ResizeGateHarness::new(100, 24);
        harness.feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07Get-ChildItem", start);
        assert!(harness.session.typed_shell_input_live());

        harness.drag_to(80, start);
        assert_eq!(
            harness.grid,
            grid_of(80, 24),
            "policy=false must reflow locally even while the shell holds text"
        );
        harness.tick(start + WINDOW_RESIZE_QUIET - Duration::from_millis(1));
        assert!(
            harness.requests.is_empty(),
            "the 200 ms coalescer remains in force"
        );
        harness.tick(start + WINDOW_RESIZE_QUIET);
        assert_eq!(
            harness.requests,
            vec![grid_of(80, 24)],
            "policy=false commits the coalesced resize while input remains live"
        );
    }

    /// PIN: a PTY deadline handed to `ControlFlow::WaitUntil` must still be in the future.
    ///
    /// The gate can be sampled as live while servicing the pending resize and read empty by the
    /// later control-flow calculation. In that state `blank_since` is still `None`, so the pending
    /// request's coalescing deadline is the only deadline available -- and it is already due. A
    /// past `WaitUntil` makes winit immediately re-enter `about_to_wait` instead of sleeping.
    #[test]
    fn a_gate_transition_cannot_offer_wait_until_an_already_due_resize_deadline() {
        let start = Instant::now();
        let mut pending = None;
        coalesce_pty_resize(
            &mut pending,
            grid_of(80, 24),
            PhysicalSize::new(800, 600),
            start,
        );
        let now = start + WINDOW_RESIZE_QUIET;

        assert!(
            pty_resize_wake_deadline(pending, false, now).is_none_or(|deadline| deadline > now),
            "ControlFlow::WaitUntil must never receive an already-due PTY deadline"
        );
        let (released, wake_deadline) = service_pending_pty_resize(&mut pending, now, false);
        assert!(released.is_none(), "one empty sample cannot release");
        assert_eq!(
            wake_deadline,
            Some(now + WINDOW_RESIZE_QUIET),
            "the same gate sample starts confirmation and schedules its future boundary"
        );
    }

    /// MACHINERY PIN (policy=true): while an OSC 133 input region holds typed content, a PTY
    /// resize is deferred — zero requests during the drag, exactly one at release carrying the
    /// final dragged size, and our own grid held in lockstep with the child's the whole way.
    ///
    /// This is the minimal repro's shape, in the app's own scheduling terms: type without
    /// submitting, drag narrower than the prompt, drag wider again, and only then let go.
    #[test]
    fn a_drag_while_the_shell_holds_typed_input_reaches_conpty_exactly_once_at_release() {
        let start = Instant::now();
        let mut harness = ResizeGateHarness::with_policy(100, 24, true);
        harness.feed(
            b"\x1b]133;A\x07PS D:\\Developer\\BetterTerminal> \x1b]133;B\x07Get-ChildItem",
            start,
        );
        assert!(harness.session.typed_shell_input_live());

        // The narrowing drag, then the widening one, with the loop turning between each step.
        for (step, columns) in [92_u16, 84, 70, 39, 55, 80, 96].into_iter().enumerate() {
            let at = start + Duration::from_millis(20 * step as u64);
            harness.drag_to(columns, at);
            harness.tick(at + Duration::from_millis(1));
        }
        // Long past every quiet deadline the drag ever set: the gate is not a debounce.
        harness.tick(start + Duration::from_secs(3600));

        assert!(
            harness.requests.is_empty(),
            "zero ConPTY resizes are owed while the child is holding text, got {:?}",
            harness.requests
        );
        assert_eq!(
            (harness.grid, harness.conpty),
            (grid_of(100, 24), grid_of(100, 24)),
            "our grid and the child's stay in lockstep at the old width across the whole drag"
        );
        assert!(
            harness
                .wake_deadline(start + Duration::from_secs(3600))
                .is_none(),
            "a held resize must not offer the loop a deadline it would spin on"
        );

        // RELEASE: the command is submitted, and the empty line that follows it is confirmed over
        // one quiet window (`a_redraw_split_across_two_reads_never_releases_in_its_blank_window` is
        // why the drain that carries `133;C` cannot be trusted on its own). It carries the last size
        // the drag reached — not the narrow one it passed through, and not one request per step.
        let released_at = start + Duration::from_secs(3600);
        harness.feed(b"\r\n\x1b]133;C\x07", released_at);
        harness.tick(released_at);
        assert!(harness.requests.is_empty());
        assert_eq!(
            harness.wake_deadline(released_at),
            Some(released_at + WINDOW_RESIZE_QUIET),
            "the loop is told when to come back and confirm, since no further output need arrive"
        );
        harness.tick(released_at + WINDOW_RESIZE_QUIET);
        assert_eq!(
            harness.requests,
            vec![grid_of(96, 24)],
            "exactly one resize lands at release, carrying the final dragged size"
        );
        assert_eq!(harness.grid, grid_of(96, 24));
        assert_eq!(harness.conpty, grid_of(96, 24));
    }

    /// MACHINERY PIN (policy=true): the three non-holding/release cases stay cheap.
    #[test]
    fn an_idle_prompt_a_bare_screen_and_a_cleared_line_all_resize_without_deferral() {
        let start = Instant::now();

        // An empty prompt: the drag lands as it always has. The confirmation window a release now
        // waits out started at this drag's own first frame, so it has already elapsed when the
        // coalescer's deadline arrives — the deadline the loop is given is the coalescer's own.
        let mut idle = ResizeGateHarness::with_policy(100, 24, true);
        idle.feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07", start);
        idle.drag_to(80, start);
        assert_eq!(
            idle.grid,
            grid_of(80, 24),
            "an empty prompt reflows immediately"
        );
        assert_eq!(
            idle.wake_deadline(start),
            Some(start + WINDOW_RESIZE_QUIET),
            "confirming an already-idle prompt must not add a second quiet window to the drag"
        );
        idle.tick(start + WINDOW_RESIZE_QUIET);
        assert_eq!(idle.requests, vec![grid_of(80, 24)]);

        // A screen that never emitted OSC 133 is untouched by any of this.
        let mut bare = ResizeGateHarness::with_policy(100, 24, true);
        bare.feed(b"PS> Get-ChildItem", start);
        bare.drag_to(80, start);
        assert_eq!(bare.grid, grid_of(80, 24));
        bare.tick(start + WINDOW_RESIZE_QUIET);
        assert_eq!(
            bare.requests,
            vec![grid_of(80, 24)],
            "honest degradation: without shell integration this is today's path, unchanged"
        );

        // Clearing the line is a release in its own right — no submission needed. What it does need
        // is the confirmation window, because "the line is empty" is exactly what a redraw's erase
        // chunk also says one millisecond before its rewrite arrives.
        let mut cleared = ResizeGateHarness::with_policy(100, 24, true);
        cleared.feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07Get-ChildItem", start);
        cleared.drag_to(80, start);
        cleared.tick(start + WINDOW_RESIZE_QUIET);
        assert!(cleared.requests.is_empty());
        let emptied_at = start + WINDOW_RESIZE_QUIET;
        cleared.feed(b"\x1b[5G\x1b[K", emptied_at);
        cleared.tick(emptied_at);
        assert!(
            cleared.requests.is_empty(),
            "the first blank sample only starts the confirmation"
        );
        cleared.tick(emptied_at + WINDOW_RESIZE_QUIET);
        assert_eq!(
            cleared.requests,
            vec![grid_of(80, 24)],
            "a line that stays empty releases the gate with no submission"
        );
    }

    /// MACHINERY PIN (policy=true): a drag that wanders away and comes back tells the child
    /// nothing at all. Without this,
    /// "exactly one resize at release" would be false whenever the final size is the one the child
    /// never left — and that resize would be a gratuitous reflow of a settled prompt.
    #[test]
    fn a_drag_that_returns_to_the_childs_own_size_releases_nothing() {
        let start = Instant::now();
        let mut harness = ResizeGateHarness::with_policy(100, 24, true);
        harness.feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07Get-ChildItem", start);
        for columns in [70_u16, 39, 100] {
            harness.drag_to(columns, start);
        }
        harness.feed(b"\r\n\x1b]133;C\x07", start);
        harness.tick(start + WINDOW_RESIZE_QUIET);
        assert!(
            harness.requests.is_empty(),
            "the child is already 100 columns wide; nothing is owed"
        );
        assert_eq!(harness.grid, grid_of(100, 24));
    }

    /// MACHINERY PIN (policy=true, confirm-then-release): the blank instant inside a redraw is not
    /// an empty prompt.
    ///
    /// The reader thread hands the loop whatever a single read returned — up to 16 KiB — and wakes
    /// it for every chunk. PSReadLine redraws a line by parking the cursor on `B`, erasing, and
    /// writing the buffer back, so a redraw that straddles two reads leaves a wake in between where
    /// the grid honestly holds nothing. Sampling the gate once at that wake and releasing on it is
    /// how the narrow width reached the child through a gate with no bypass in it.
    ///
    /// So a release needs the *same* answer for `WINDOW_RESIZE_QUIET` — the quiet period already
    /// used for "the drag has stopped" — and any sample that reads content restarts the clock. The
    /// millisecond-scale blank of a redraw can never clear that bar; a line the user actually
    /// emptied always does.
    #[test]
    fn a_redraw_split_across_two_reads_never_releases_in_its_blank_window() {
        let start = Instant::now();
        let mut harness = ResizeGateHarness::with_policy(100, 24, true);
        // A 12-column prompt: `B` lands on column 12, which is CUP column 13.
        harness.feed(
            b"\x1b]133;A\x07PS D:\\dist> \x1b]133;B\x07Get-ChildItem",
            start,
        );
        assert!(harness.session.typed_shell_input_live());

        harness.drag_to(35, start);
        harness.tick(start + Duration::from_millis(1));
        assert!(
            harness.requests.is_empty(),
            "the buffer is full; nothing is owed"
        );

        // The chunk that carries the erase but not the rewrite, drained at a wake past the
        // coalescer's own deadline.
        let erased_at = start + WINDOW_RESIZE_QUIET + Duration::from_millis(5);
        harness.feed(b"\x1b[13G\x1b[J", erased_at);
        assert!(
            !harness.session.typed_shell_input_live(),
            "the fixture must really empty the grid, or this pin proves nothing"
        );
        harness.tick(erased_at);
        assert!(
            harness.requests.is_empty(),
            "one blank sample is not an empty prompt: {:?}",
            harness.requests
        );

        // The rewrite lands three milliseconds later. The buffer was never empty.
        let rewritten_at = erased_at + Duration::from_millis(3);
        harness.feed(b"Get-ChildItem2", rewritten_at);
        harness.tick(rewritten_at);
        assert!(harness.requests.is_empty());

        // A second redraw's erase, ten milliseconds after the first. The confirmation window must
        // restart here, not run from the first blank sample.
        let erased_again_at = erased_at + Duration::from_millis(10);
        harness.feed(b"\x1b[13G\x1b[J", erased_again_at);
        harness.tick(erased_again_at);
        harness.tick(erased_at + WINDOW_RESIZE_QUIET);
        assert!(
            harness.requests.is_empty(),
            "a content sample between the two blanks restarts the confirmation: {:?}",
            harness.requests
        );

        harness.feed(b"Get-ChildItem23", erased_at + WINDOW_RESIZE_QUIET);
        harness.tick(erased_again_at + WINDOW_RESIZE_QUIET);
        assert!(harness.requests.is_empty());

        // RELEASE: the command is submitted, and the line stays empty through the window.
        let submitted_at = start + Duration::from_secs(1);
        harness.feed(b"\r\n\x1b]133;C\x07", submitted_at);
        harness.tick(submitted_at);
        assert!(
            harness.requests.is_empty(),
            "even a submission is confirmed before it releases"
        );
        harness.tick(submitted_at + WINDOW_RESIZE_QUIET);
        assert_eq!(
            harness.requests,
            vec![grid_of(35, 24)],
            "exactly one resize, carrying the dragged size, once the line is confirmed empty"
        );
    }

    /// RED-CHECK for the deferral pin: prove it is not vacuous. This is the pre-mitigation path —
    /// the same drag with the gate answer forced to `false` — and it is exactly what the forensic
    /// run showed reaching PSReadLine: the 39-column width, narrower than the prompt, delivered
    /// while the input buffer is non-empty.
    #[test]
    fn without_the_gate_the_same_drag_hands_conpty_the_narrow_width_mid_input() {
        let start = Instant::now();
        let mut pending = None;
        let mut conpty = grid_of(100, 24);
        let mut local = conpty;
        let mut requests = Vec::new();
        for (step, columns) in [92_u16, 84, 70, 39, 55, 80, 96].into_iter().enumerate() {
            let at = start + Duration::from_millis(20 * step as u64);
            let next = grid_of(columns, 24);
            // `typed_input_live: false` is the clause removed.
            if let Some(reflow) = plan_grid_change(
                &mut pending,
                next,
                conpty,
                local,
                PhysicalSize::new(u32::from(columns) * 8, 600),
                at,
                false,
            ) {
                local = reflow;
            }
            if let Some(due) = release_due_pty_resize(&mut pending, at + WINDOW_RESIZE_QUIET, false)
            {
                conpty = due.grid;
                requests.push(due.grid);
            }
        }
        assert!(
            requests.contains(&grid_of(39, 24)),
            "the ungated path really does hand the child a width narrower than the prompt, \
             which is the whole precondition of the PSReadLine anchor fault: {requests:?}"
        );
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
        assert_eq!(frame.grid_rows.get(), 2);
        assert_eq!(frame.rows.get(), 3);
        assert_eq!(frame.drawable_rows(), 2);
        assert_eq!(render_rows.len(), 3);
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
