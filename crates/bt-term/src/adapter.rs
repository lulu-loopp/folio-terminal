use std::{
    cell::{Cell, RefCell},
    collections::{VecDeque, hash_map::RandomState},
    hash::{BuildHasher, Hasher},
    num::NonZeroU32,
    sync::{Arc, Mutex, MutexGuard},
    time::Instant,
};

use alacritty_terminal::{
    Term,
    event::{Event, EventListener},
    grid::Dimensions,
    index::{Column, Line},
    term::{
        Config, ScrollOutCause, ScrollRegionScope, TermDamage, TermMode, TranscriptEvent,
        TranscriptScreen, cell::Flags,
    },
    vte::{
        Params, Parser, Perform,
        ansi::{Handler, NamedPrivateMode, PrivateMode, Processor, Rgb},
    },
};
use bt_transcript::CapturedRow;

use crate::cell_capture::{
    CapturedRowFingerprint, captured_row_fingerprint, snapshot, to_captured_row,
};
use crate::inline_image::{InlineImageStreamAction, Osc1337Scanner, ShellIntegrationMarker};
use crate::palette::{TerminalCanvas, TerminalPalette};

pub const SCROLLBACK_LINES: usize = 0;

#[derive(Clone, Copy)]
struct GridSize {
    columns: NonZeroU32,
    rows: NonZeroU32,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows.get() as usize
    }

    fn screen_lines(&self) -> usize {
        self.rows.get() as usize
    }

    fn columns(&self) -> usize {
        self.columns.get() as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemovalCause {
    NormalScroll,
    DeleteLines,
    Resize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemovalScreen {
    Primary,
    Alternate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemovalScope {
    FullScreen,
    Partial,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemovalContext {
    pub cause: RemovalCause,
    pub screen: RemovalScreen,
    pub scope: RemovalScope,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemovedLiveRow {
    pub live_row: u32,
    pub row: CapturedRow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalCursor {
    pub row: u32,
    pub column: u32,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseTracking {
    Off,
    Click,
    Drag,
    Motion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalModes {
    pub alternate_screen: bool,
    pub alternate_scroll: bool,
    pub sgr_mouse: bool,
    pub mouse_tracking: MouseTracking,
    /// DECSET 1004: the child asked to be told when this window gains and loses
    /// focus. Reported here for the same reason the mouse bits are — it is a
    /// mode a *program* turns on, and something has to be able to see that it is
    /// still on after the program that wanted it is gone.
    pub focus_reporting: bool,
}

/// Facts emitted by the alacritty compatibility seam. DESIGN.md §3.1 policy is intentionally
/// absent: every removed row carries its cause, screen, scope, and stable captured cells so the
/// lifecycle layer can decide whether the transcript changes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdapterEvent {
    RowsRemoved {
        context: RemovalContext,
        rows: Vec<RemovedLiveRow>,
    },
    GridScrolled,
    ScreenCleared,
    ClearHistory,
    Reset,
    Deccolm,
    PrimaryParked,
    PrimaryRestored,
    InlineImage {
        screen: RemovalScreen,
        row: u32,
        column: u32,
        /// How many columns of the `[image]` placeholder the adapter actually wrote at
        /// `(row, column)`. Near the right edge the label is truncated, so the span the hover-peek
        /// layer must answer over is reported by the only party that knows it rather than
        /// recomputed — one formula, one owner.
        placeholder_columns: u32,
        encoded: Vec<u8>,
    },
    ShellIntegration {
        screen: RemovalScreen,
        row: u32,
        column: u32,
        marker: ShellIntegrationMarker,
    },
    /// The shell reported its working directory over OSC 7. The fact carried is the `file://` URI
    /// exactly as received; what it names — a usable directory, or nothing this terminal can
    /// resolve — is a session decision, not a vendor-seam one. No screen is attached: a working
    /// directory belongs to the shell, and the alternate-screen TUI it launched inherits it.
    WorkingDirectory {
        uri: String,
    },
    /// The child changed the session's window title through OSC 0/2. Window titles are UI state,
    /// not grid or transcript state, and apply across primary/alternate screen switches.
    Title {
        title: String,
    },
    /// The child restored the terminal's default window title.
    ResetTitle,
    Bell,
    Progress(Option<crate::session::ProgressState>),
    /// A program asked for a desktop notification, over `OSC 9` or `OSC 777;notify`.
    ///
    /// A *request*, and this seam is deliberately the last place that is true of it: whether
    /// anything reaches the operating system is an application question — see
    /// `DualPlaneSession::take_notifications` and `bt-app`'s own gate — and a terminal that
    /// decided it here would be one that could not be turned off.
    Notification(crate::session::TerminalNotification),
    /// A program entered, restated or withdrew a standing request for attention
    /// (`OSC 1337;RequestAttention=`).
    ///
    /// Reported exactly as it arrived, including the restatements. Which of them is a *new*
    /// request is a question about what this session was already asserting, and the session is
    /// where it is answered — this seam says what the bytes said.
    AttentionRequest(crate::session::AttentionRequest),
    GridWrites {
        screen: RemovalScreen,
        rows: Vec<u32>,
    },
}

/// Stable, vendor-free damage fact consumed by the live decoration lifecycle. Column bounds are
/// intentionally omitted: a formula owns complete grid rows and any mutation in one of them must
/// invalidate the transient block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalDamage {
    Full,
    Rows(Vec<u32>),
}

/// One entry in the queue of bytes owed to the child.
///
/// Two variants and not one, because a colour query cannot be answered where it
/// is heard. The vendored terminal raises `Event::ColorRequest` from inside a
/// `&mut Term` borrow, so the listener that receives it can see neither the
/// colours the child has already overridden (they live on that same `Term`) nor
/// the palette the window is wearing (which lives on the adapter). Queuing the
/// question **in its place in the stream** and resolving it at drain time keeps
/// the one property that matters: a program which writes `OSC 11;?` and then
/// `CSI 6n` reads its two answers back in the order it asked them.
enum PendingReply {
    /// Bytes the terminal state machine already knows in full - DSR, DA, DECRQM.
    Bytes(Vec<u8>),
    /// A colour query, held with the formatter the vendored parser built for it.
    /// That closure is the only carrier of the terminator the asker used, so
    /// answering through it is what makes a BEL-terminated query get a
    /// BEL-terminated reply.
    Color {
        index: usize,
        format: Arc<dyn Fn(Rgb) -> String + Sync + Send + 'static>,
    },
}

impl PendingReply {
    /// The bytes to send, or `None` when this window cannot honestly answer.
    ///
    /// `overridden` is what the *child* set with `OSC 4/10/11/12;<colour>`,
    /// which outranks the window palette for the same reason a program's own
    /// SGR 31 outranks the scheme: the terminal was told, and a terminal that
    /// forgets what it was told is lying.
    fn bytes(
        self,
        overridden: impl FnOnce(usize) -> Option<Rgb>,
        palette: Option<&TerminalPalette>,
    ) -> Option<Vec<u8>> {
        match self {
            PendingReply::Bytes(bytes) => Some(bytes),
            PendingReply::Color { index, format } => {
                let color = overridden(index).or_else(|| {
                    palette
                        .and_then(|palette| palette.color(index))
                        .map(|[r, g, b]| Rgb { r, g, b })
                })?;
                Some(format(color).into_bytes())
            }
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct CaptureListener {
    transcript_events: Arc<Mutex<Vec<TranscriptEvent>>>,
    pty_writes: Arc<Mutex<Vec<PendingReply>>>,
    adapter_events: Arc<Mutex<Vec<AdapterEvent>>>,
}

impl EventListener for CaptureListener {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => self
                .pty_writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(PendingReply::Bytes(text.into_bytes())),
            Event::ColorRequest(index, format) => self
                .pty_writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(PendingReply::Color { index, format }),
            Event::Title(title) => self
                .adapter_events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(AdapterEvent::Title { title }),
            Event::ResetTitle => self
                .adapter_events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(AdapterEvent::ResetTitle),
            _ => {}
        }
    }
}

fn lock_events(listener: &CaptureListener) -> MutexGuard<'_, Vec<TranscriptEvent>> {
    listener
        .transcript_events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn install_transcript_hook(term: &mut Term<CaptureListener>, listener: &CaptureListener) {
    let transcript_events = listener.transcript_events.clone();
    term.set_transcript_hook(Some(Arc::new(move |event| {
        transcript_events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(event);
    })));
}

fn discard_listener_output(listener: &CaptureListener) {
    lock_events(listener).clear();
    listener
        .pty_writes
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
    listener
        .adapter_events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

/// Vendor-facing terminal adapter. It translates upstream facts into stable Folio facts
/// and never owns or mutates the canonical transcript.
pub struct TerminalAdapter {
    term: Term<CaptureListener>,
    processor: Processor,
    listener: CaptureListener,
    parser_boundary: Parser,
    parser_tail: Vec<u8>,
    parser_sync_active: bool,
    parser_dcs_active: bool,
    parser_sequence_open: bool,
    cursor_row_positioned_explicitly: bool,
    osc1337_scanner: Osc1337Scanner,
    /// What [`Self::feed`] scanned out of the byte stream and has not handed the
    /// vendor terminal yet.
    ///
    /// It is non-empty only between a shell-integration marker and the caller's
    /// [`Self::resume_stream`], and that pause is the whole reason the queue
    /// exists — see [`Self::feed`].
    pending_stream: VecDeque<InlineImageStreamAction>,
    resize_canonical: Option<ResizeCanonical>,
    staged_resize_history_size: usize,
    columns: NonZeroU32,
    rows: NonZeroU32,
    row_fingerprint_seed: u64,
    /// How many rows this terminal has actually captured — cache misses only.
    ///
    /// A capture clones a whole row of vendor cells and turns them into
    /// [`bt_transcript::CapturedRow`]; a frame that captures the grid three
    /// times used to pay for it three times. The counter is what lets a test
    /// say how many times, rather than how many times it looks like. A [`Cell`]
    /// because [`Self::visible_row`] is `&self` and a measurement must not be
    /// allowed to change that.
    captures: Cell<u64>,
    /// Each visible row's capture, filed under the fingerprint of the vendor
    /// cells it was made from. See [`Self::visible_row`].
    captured_rows: RefCell<Vec<Option<(CapturedRowFingerprint, CapturedRow)>>>,
    /// What the window says it is painted in, or `None` while nobody has said.
    ///
    /// `None` is a real state and not an oversight: this crate is a logic-only
    /// terminal that also runs headless in tests and in the `bt-pty` harnesses,
    /// and a colour query there has no honest answer. It goes unanswered rather
    /// than answered with a guess - see `crate::palette`.
    color_palette: Option<TerminalPalette>,
    /// The canvas last handed to [`Self::set_color_palette`].
    ///
    /// Held apart from the palette itself so the DEC 2031 notification fires on
    /// a *change* and not on every drain. It is updated whether or not anybody
    /// is subscribed, so that a program which enables 2031 mid-session is not
    /// immediately told about a switch that happened before it was listening -
    /// its own `OSC 11;?` is how it learns where it started.
    announced_canvas: Option<TerminalCanvas>,
    /// Where a DEC 1004 subscriber was last told the keyboard is.
    ///
    /// Held for [`Self::announced_canvas`]'s reason — a report of transitions
    /// needs to remember what it last reported — but reset rather than kept
    /// across an unsubscribe, and that asymmetry is the whole of
    /// [`AnnouncedFocus::Unknown`].
    announced_focus: AnnouncedFocus,
}

/// What a DEC 1004 subscriber has been told about where the keyboard is.
///
/// **Three states and not a `bool`**, because "nobody has been told anything
/// yet" is the state every pane is born in and the one the *initial* report is
/// owed to. A pane that opens in a background tab, or in a window that is not
/// the foreground one, is already without the keyboard when its child
/// subscribes; an implementation that only ever answered a `focused → not
/// focused` change would leave that child believing it had the keyboard for as
/// long as it lived, which is the one belief this whole report exists to
/// correct.
///
/// It goes back to `Unknown` on two occasions, and the second one is the whole
/// of whether this works on Windows at all:
///
/// 1. **The mode goes off.** What was said was said to a subscriber that has
///    gone.
/// 2. **Anybody asks for it** — every `CSI ? 1004 h`, not only the one that
///    changes the level. On this platform ConPTY subscribes on the transport's
///    behalf before any program runs (`docs/DESIGN.md` §7.1.5i), so a child's own
///    `?1004h` sets a bit that is already set. **Measured, 2026-08-25**: a real
///    `claude.exe` in a real pane, born in a window without the keyboard, was
///    duly sent `CSI O` — and heard nothing, because the console host only
///    re-encodes a focus event into `CSI I`/`CSI O` for a client that has asked
///    for focus events, and at the moment of the opening report the client had
///    not yet started. Treating the ask as the subscription edge is what puts the
///    opening report *after* the child is listening. The cost of the reading is
///    one repeated report per ask; the cost of the other reading is that the
///    opening report is never heard by anyone.
///
/// This is deliberately the opposite of [`TerminalAdapter::announced_canvas`],
/// which is kept up to date whether or not anybody is listening: a program can
/// ask where the canvas started with its own `OSC 11;?`, and there is **no query
/// for focus** — the terminal telling it is the only road there is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AnnouncedFocus {
    /// Nothing has been said to whoever is subscribed now.
    Unknown,
    /// It was last told the keyboard is here.
    Focused,
    /// It was last told the keyboard is elsewhere.
    Unfocused,
}

struct ResizeCanonical {
    term: Term<CaptureListener>,
    processor: Processor,
    listener: CaptureListener,
}

#[derive(Default)]
struct BoundaryPerformer {
    complete: bool,
    execute_at_ground: bool,
    sync_start: bool,
    sync_end: bool,
    dcs_hook: bool,
    dcs_put: bool,
    bell: bool,
    /// **Somebody asked for focus reports in this slice** — `CSI ? 1004 h`,
    /// alone or among other modes.
    ///
    /// Watched here rather than read off [`TerminalModes::focus_reporting`]
    /// because what matters is the *asking*, and by the time the child asks the
    /// mode is already on: on this platform ConPTY turns 1004 on for the
    /// transport before any program runs (`docs/DESIGN.md` §7.1.5i), so the
    /// child's own `?1004h` sets a bit that is already set and changes no level
    /// anybody could compare. See [`AnnouncedFocus`] for what that costs and why
    /// this bit is the fix.
    focus_reports_requested: bool,
    cursor_row_positioned_explicitly: Option<bool>,
}

impl Perform for BoundaryPerformer {
    fn print(&mut self, _character: char) {
        self.complete = true;
        // A printable can autowrap and therefore land the cursor on another row. A later CUP/HVP
        // in the same repaint burst restores the explicit fact.
        self.cursor_row_positioned_explicitly = Some(false);
    }

    fn execute(&mut self, byte: u8) {
        self.complete = self.execute_at_ground || matches!(byte, 0x18 | 0x1a);
        self.bell = self.execute_at_ground && byte == 0x07;
        if matches!(byte, b'\n' | b'\x0b' | b'\x0c' | b'\r') {
            self.cursor_row_positioned_explicitly = Some(false);
        }
    }

    fn unhook(&mut self) {
        self.complete = true;
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {
        self.dcs_hook = true;
    }

    fn put(&mut self, _byte: u8) {
        self.dcs_put = true;
    }

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {
        self.complete = true;
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        self.complete = true;
        let sync_mode = intermediates == b"?"
            && params
                .iter()
                .next()
                .is_some_and(|parameter| parameter == [2026]);
        self.sync_start = sync_mode && action == 'h';
        self.sync_end = sync_mode && action == 'l';
        // **Every parameter, not just the first** — `CSI ? 1004 ; 1006 h` is one
        // program asking for two things, and a reader that looked only at the
        // head of the list would hear half of it.
        self.focus_reports_requested = intermediates == b"?"
            && action == 'h'
            && params
                .iter()
                .any(|parameter| parameter.first() == Some(&1004));
        if intermediates.is_empty() {
            self.cursor_row_positioned_explicitly = match action {
                // CUP and HVP are the absolute row-placement family used by line editors.
                'H' | 'f' => Some(true),
                // Every other control which can land the cursor on another physical row is a
                // non-qualifying placement. Horizontal-only motion deliberately leaves the last
                // row-placement fact intact.
                'A' | 'B' | 'E' | 'F' | 'L' | 'M' | 'S' | 'T' | 'd' | 'e' | 'r' | 'u' => {
                    Some(false)
                }
                'J' if params
                    .iter()
                    .next()
                    .is_some_and(|parameter| parameter == [2]) =>
                {
                    Some(false)
                }
                _ => None,
            };
        } else if intermediates == b"?"
            && matches!(action, 'h' | 'l')
            && params
                .iter()
                .any(|parameter| matches!(parameter, [3] | [47] | [1047] | [1049]))
        {
            self.cursor_row_positioned_explicitly = Some(false);
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        self.complete = true;
        if matches!(byte, b'8' | b'D' | b'E' | b'M' | b'c') {
            self.cursor_row_positioned_explicitly = Some(false);
        }
    }
}

impl TerminalAdapter {
    pub fn new(columns: NonZeroU32, rows: NonZeroU32) -> Self {
        let config = Config {
            scrolling_history: SCROLLBACK_LINES,
            ..Config::default()
        };
        let size = GridSize { columns, rows };
        let listener = CaptureListener::default();
        let mut term = Term::new(config, &size, listener.clone());
        install_transcript_hook(&mut term, &listener);
        let row_fingerprint_seed = RandomState::new().build_hasher().finish();
        Self {
            term,
            processor: Processor::new(),
            listener,
            parser_boundary: Parser::new(),
            parser_tail: Vec::new(),
            parser_sync_active: false,
            parser_dcs_active: false,
            parser_sequence_open: false,
            cursor_row_positioned_explicitly: false,
            osc1337_scanner: Osc1337Scanner::default(),
            pending_stream: VecDeque::new(),
            resize_canonical: None,
            staged_resize_history_size: 0,
            columns,
            rows,
            row_fingerprint_seed,
            captures: Cell::new(0),
            captured_rows: RefCell::new(Vec::new()),
            color_palette: None,
            announced_canvas: None,
            announced_focus: AnnouncedFocus::Unknown,
        }
    }

    /// Tell this terminal what the window it lives in is painted in.
    ///
    /// Idempotent and cheap, so the owning app can call it on every turn of its
    /// pipe drain rather than maintaining a list of every place a theme can
    /// change; comparing the canvas here is what makes calling it repeatedly
    /// free.
    ///
    /// A canvas change queues DEC mode 2031's `CSI ? 997 ; Ps n` for a program
    /// that subscribed to it. Programs that did not subscribe are told nothing:
    /// the sequence would be read as input by anything that never asked for it.
    pub fn set_color_palette(&mut self, palette: TerminalPalette) {
        let moved = self
            .announced_canvas
            .is_some_and(|announced| announced != palette.canvas);
        self.announced_canvas = Some(palette.canvas);
        self.color_palette = Some(palette);
        if moved && self.theme_update_notification() {
            self.listener
                .pty_writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(PendingReply::Bytes(palette.canvas_notification()));
        }
    }

    /// Tell this terminal whether the keyboard is in the pane it draws, so a
    /// program inside it can know whether anybody is looking (DEC private mode
    /// 1004).
    ///
    /// Idempotent and cheap for [`Self::set_color_palette`]'s reason, and the
    /// owning app is meant to call it the same way: on every turn of its drain,
    /// rather than from each of the dozen places a window can hand the keyboard
    /// around. The comparison here is what makes calling it repeatedly free.
    ///
    /// **Only a change is reported.** DEC 1004 is a report of transitions —
    /// `CSI I` when the keyboard arrives, `CSI O` when it leaves — so a level
    /// re-sent every turn would be this terminal typing at its child sixty times
    /// a second. The first call after a subscription is a change by definition
    /// ([`AnnouncedFocus::Unknown`]), which is how a pane born in the background
    /// learns it is in the background.
    ///
    /// A program that did not subscribe is told nothing, for the reason 2031's
    /// notification is withheld from one: the sequence would be read as ordinary
    /// input by anything that never asked for it.
    pub fn set_keyboard_focus(&mut self, focused: bool) {
        if !self.focus_reporting() {
            self.announced_focus = AnnouncedFocus::Unknown;
            return;
        }
        let standing = if focused {
            AnnouncedFocus::Focused
        } else {
            AnnouncedFocus::Unfocused
        };
        if self.announced_focus == standing {
            return;
        }
        self.announced_focus = standing;
        let report: &[u8] = if focused { b"\x1b[I" } else { b"\x1b[O" };
        self.listener
            .pty_writes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(PendingReply::Bytes(report.to_vec()));
    }

    /// Whether the child asked to be told when the keyboard enters and leaves
    /// this pane (DEC private mode 1004).
    ///
    /// On this platform ConPTY subscribes on every session's behalf before any
    /// program runs, and consumes `CSI I`/`CSI O` into a console `FOCUS_EVENT`
    /// — see `docs/DESIGN.md` §7.1.5i for the A/B that measured it. So this is
    /// nearly always true, and the pane that is *not* subscribed is the
    /// interesting one: a headless [`TerminalAdapter`] in a test, or a child
    /// that turned the mode off itself.
    pub fn focus_reporting(&self) -> bool {
        self.term.mode().contains(TermMode::FOCUS_IN_OUT)
    }

    /// Whether the child asked to be told when the window changes canvas
    /// (DEC private mode 2031).
    pub fn theme_update_notification(&self) -> bool {
        self.term
            .mode()
            .contains(TermMode::THEME_UPDATE_NOTIFICATION)
    }

    pub fn alacritty_history_size(&self) -> usize {
        self.term.grid().history_size()
    }

    pub fn dimensions(&self) -> (NonZeroU32, NonZeroU32) {
        (self.columns, self.rows)
    }

    /// Take a slice of the child's output and report what it meant — **stopping
    /// at the first shell-integration marker in it.**
    ///
    /// # Why it stops
    ///
    /// The vendor terminal is advanced over whole slices, so everything this
    /// method hands it has already landed on the grid by the time the caller
    /// looks at the events. That is harmless for a fact that carries its own
    /// coordinates (a removed row arrives with the row in it) and ruinous for
    /// one whose meaning is *the grid at that instant*: an OSC 133 `B`/`C` names
    /// a cell, and a session that reads that cell after the rest of the slice
    /// has scrolled the screen reads somebody else's line, or none at all. That
    /// was the whole of the "a command whose output arrives in the same read has
    /// no text" defect — one PTY read carrying `C` and a flood.
    ///
    /// So the stream pauses there. Everything before the marker is on the grid,
    /// nothing after it is, and the caller may apply the marker against exactly
    /// the screen the shell was describing. [`Self::stream_paused`] then answers
    /// `true` and [`Self::resume_stream`] continues from the same point. Markers
    /// are a handful per command, so the split costs nothing a flood can feel —
    /// and it is a rule about *when a fact is read*, not a size to tune.
    ///
    /// Bytes that arrive while the stream is paused simply queue behind it; the
    /// scanner's own state is unaffected, because the split is downstream of it.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<AdapterEvent> {
        let actions = self.osc1337_scanner.scan(bytes);
        self.pending_stream.extend(actions);
        self.pump_stream()
    }

    /// Is there stream left that [`Self::feed`] deliberately stopped short of?
    pub fn stream_paused(&self) -> bool {
        !self.pending_stream.is_empty()
    }

    /// Continue the stream [`Self::feed`] paused, up to the next marker or its end.
    pub fn resume_stream(&mut self) -> Vec<AdapterEvent> {
        self.pump_stream()
    }

    /// Throw away what the paused stream still holds.
    ///
    /// For the one caller that has abandoned the slice it was feeding: a session
    /// whose parse failed mid-quantum is not going to apply the rest of it, and
    /// bytes left queued here would otherwise be replayed into the next feed
    /// against a state that has already been reset.
    pub fn discard_paused_stream(&mut self) {
        self.pending_stream.clear();
    }

    fn pump_stream(&mut self) -> Vec<AdapterEvent> {
        let mut events = Vec::new();
        while let Some(action) = self.pending_stream.pop_front() {
            let marker = matches!(action, InlineImageStreamAction::ShellIntegration(_));
            match action {
                InlineImageStreamAction::Bytes(bytes) => {
                    self.advance_terminal_bytes(&bytes);
                    events.extend(self.drain_transcript_events());
                    events.extend(self.drain_adapter_events());
                    events.extend(self.drain_grid_write_events());
                }
                InlineImageStreamAction::Image(encoded) => {
                    let cursor = self.cursor();
                    let screen = if self.modes().alternate_screen {
                        RemovalScreen::Alternate
                    } else {
                        RemovalScreen::Primary
                    };
                    let placeholder_columns = self.write_inline_image_placeholder(b"[image]");
                    events.extend(self.drain_transcript_events());
                    events.extend(self.drain_grid_write_events());
                    events.push(AdapterEvent::InlineImage {
                        screen,
                        row: cursor.row,
                        column: cursor.column,
                        placeholder_columns,
                        encoded,
                    });
                }
                InlineImageStreamAction::ShellIntegration(marker) => {
                    let cursor = self.cursor();
                    let screen = if self.modes().alternate_screen {
                        RemovalScreen::Alternate
                    } else {
                        RemovalScreen::Primary
                    };
                    events.push(AdapterEvent::ShellIntegration {
                        screen,
                        row: cursor.row,
                        column: cursor.column,
                        marker,
                    });
                }
                InlineImageStreamAction::WorkingDirectory(uri) => {
                    // A URI is ASCII by construction (RFC 3986); bytes that are not UTF-8 are not
                    // a URI, and the empty report they become is the same "no directory" fact an
                    // empty OSC 7 payload carries.
                    events.push(AdapterEvent::WorkingDirectory {
                        uri: String::from_utf8(uri).unwrap_or_default(),
                    });
                }
                InlineImageStreamAction::Progress(progress) => {
                    events.push(AdapterEvent::Progress(progress));
                }
                InlineImageStreamAction::Notification(notification) => {
                    events.push(AdapterEvent::Notification(notification));
                }
                InlineImageStreamAction::AttentionRequest(request) => {
                    events.push(AdapterEvent::AttentionRequest(request));
                }
                InlineImageStreamAction::TooLarge => {
                    self.write_inline_image_placeholder(b"[image:too-large]");
                    events.extend(self.drain_transcript_events());
                    events.extend(self.drain_grid_write_events());
                }
            }
            if marker {
                break;
            }
        }
        events
    }

    fn advance_terminal_bytes(&mut self, bytes: &[u8]) {
        self.processor.advance(&mut self.term, bytes);
        if let Some(canonical) = self.resize_canonical.as_mut() {
            canonical.processor.advance(&mut canonical.term, bytes);
            discard_listener_output(&canonical.listener);
            let _ = canonical.term.take_input_writes();
        }
        self.observe_parser_boundary(bytes);
    }

    fn drain_grid_write_events(&mut self) -> Vec<AdapterEvent> {
        let mut primary = Vec::new();
        let mut alternate = Vec::new();
        for (screen, row) in self.term.take_input_writes() {
            let Ok(row) = u32::try_from(row) else {
                continue;
            };
            match screen {
                TranscriptScreen::Primary => primary.push(row),
                TranscriptScreen::Alternate => alternate.push(row),
            }
        }
        primary.sort_unstable();
        primary.dedup();
        alternate.sort_unstable();
        alternate.dedup();

        let mut events = Vec::with_capacity(
            usize::from(!primary.is_empty()) + usize::from(!alternate.is_empty()),
        );
        if !primary.is_empty() {
            events.push(AdapterEvent::GridWrites {
                screen: RemovalScreen::Primary,
                rows: primary,
            });
        }
        if !alternate.is_empty() {
            events.push(AdapterEvent::GridWrites {
                screen: RemovalScreen::Alternate,
                rows: alternate,
            });
        }
        events
    }

    /// Write the `[image]` text placeholder at the cursor and report how many columns of it landed.
    /// The label is ASCII, so one byte is one column.
    fn write_inline_image_placeholder(&mut self, label: &[u8]) -> u32 {
        let remaining = self
            .columns
            .get()
            .saturating_sub(self.cursor().column)
            .max(1) as usize;
        let visible = &label[..label.len().min(remaining)];
        self.advance_terminal_bytes(visible);
        visible.len() as u32
    }

    /// Consume damage exactly once after a parser/resize action. `Term::damage` also accounts for
    /// cursor movement; treating that as damage is deliberately conservative for a live overlay.
    pub fn take_damage(&mut self) -> TerminalDamage {
        let damage = match self.term.damage() {
            TermDamage::Full => TerminalDamage::Full,
            TermDamage::Partial(lines) => TerminalDamage::Rows(
                lines
                    .filter_map(|line| u32::try_from(line.line).ok())
                    .collect(),
            ),
        };
        self.term.reset_damage();
        damage
    }

    /// Deadline for a DEC 2026 synchronized update buffered by the parser.
    pub fn synchronized_update_deadline(&self) -> Option<Instant> {
        self.processor.sync_timeout().sync_timeout()
    }

    /// Commit a synchronized update whose ESU terminator did not arrive before its deadline.
    pub fn finish_synchronized_update(&mut self) -> Vec<AdapterEvent> {
        if self.synchronized_update_deadline().is_none() {
            return Vec::new();
        }
        self.processor.stop_sync(&mut self.term);
        if let Some(canonical) = self.resize_canonical.as_mut() {
            canonical.processor.stop_sync(&mut canonical.term);
            discard_listener_output(&canonical.listener);
        }
        self.parser_sync_active = false;
        self.parser_sequence_open = false;
        self.parser_tail.clear();
        let mut events = self.drain_transcript_events();
        events.extend(self.drain_adapter_events());
        events
    }

    pub fn resize(&mut self, columns: NonZeroU32, rows: NonZeroU32) -> Vec<AdapterEvent> {
        self.term.resize(GridSize { columns, rows });
        self.columns = columns;
        self.rows = rows;
        self.cursor_row_positioned_explicitly = false;
        self.drain_transcript_events()
    }

    pub fn begin_resize_transaction(&mut self) -> usize {
        if self.resize_canonical.is_some() {
            return 0;
        }

        let restored = self.term.begin_resize_transaction();
        self.staged_resize_history_size = 0;
        self.arm_resize_canonical();
        restored
    }

    /// Return transcript-staged resize rows to the displayed vendor branch without disturbing the
    /// canonical ConPTY branch maintained for the whole coalesced transaction.
    pub fn resume_resize_transaction(&mut self) -> usize {
        let restored = self.term.begin_resize_transaction();
        self.staged_resize_history_size = 0;
        restored
    }

    /// Fork the branch that will only ever be given the sizes ConPTY is actually told about.
    ///
    /// Armed at every point where the displayed grid and the child's grid are known to agree: the
    /// start of a transaction, and each commit inside it. Between two such points the displayed
    /// branch follows the pointer through sizes the child never had, so it is the canonical fork —
    /// which receives the same bytes and exactly one resize — that the next commit installs.
    fn arm_resize_canonical(&mut self) {
        let listener = CaptureListener::default();
        let mut term = self.term.fork(listener.clone());
        install_transcript_hook(&mut term, &listener);

        // A transaction can begin between two bytes of a CSI/OSC/DCS/UTF-8 sequence or while a
        // synchronized update is buffered. Seed a fresh processor with that exact uncommitted raw
        // tail against a disposable fork; the canonical term already contains every committed
        // semantic action and must not receive the tail twice.
        let seed_listener = CaptureListener::default();
        let mut seed_term = self.term.fork(seed_listener);
        let mut processor = Processor::new();
        processor.advance(&mut seed_term, &self.parser_tail);

        self.resize_canonical = Some(ResizeCanonical {
            term,
            processor,
            listener,
        });
    }

    pub fn finish_resize_transaction(&mut self) -> Vec<CapturedRow> {
        // The normal final-size commit consumes the canonical branch first. This fallback only
        // covers callers which abort a transaction without committing a pseudoconsole resize.
        self.resize_canonical = None;
        self.staged_resize_history_size = 0;
        self.term
            .finish_resize_transaction()
            .iter()
            .map(|row| to_captured_row(&row[..]))
            .collect()
    }

    /// Move the displayed branch's mutable history into transcript staging between actor calls.
    /// The canonical branch is intentionally left open and receives the same PTY bytes separately.
    pub fn stage_resize_transaction(&mut self) -> Vec<CapturedRow> {
        let rows = self
            .term
            .stage_resize_transaction()
            .iter()
            .map(|row| to_captured_row(&row[..]))
            .collect::<Vec<_>>();
        self.staged_resize_history_size = rows.len();
        rows
    }

    /// Close a transaction whose final vendor history is already owned by transcript resize
    /// staging. Vendor keeps only the unfinished suffix needed for a future native grow.
    pub fn finish_staged_resize_transaction(&mut self, unfinished_rows: usize) {
        self.resize_canonical = None;
        let history_was_staged = self.staged_resize_history_size != 0;
        self.staged_resize_history_size = 0;
        if history_was_staged {
            self.term
                .retain_resize_staging_candidate_rows(unfinished_rows);
        } else {
            debug_assert!(self.term.finish_resize_transaction().is_empty());
        }
    }

    pub fn clear_resize_transaction_history(&mut self) {
        self.staged_resize_history_size = 0;
        self.term.clear_resize_transaction_history();
        if let Some(canonical) = self.resize_canonical.as_mut() {
            canonical.term.clear_resize_transaction_history();
        }
    }

    pub fn resize_transaction_history_size(&self) -> usize {
        self.term
            .resize_transaction_history_size()
            .max(self.staged_resize_history_size)
    }

    pub fn retain_resize_staging_candidate_rows(&mut self, rows: usize) {
        self.term.retain_resize_staging_candidate_rows(rows);
    }

    pub fn resize_staging_candidate_rows(&self) -> usize {
        self.term.resize_staging_candidate_rows()
    }

    pub fn reconcile_resize_transaction_to_viewport(&mut self) -> (usize, usize) {
        let history_before = self.term.resize_transaction_history_size();
        let Some(mut canonical) = self.resize_canonical.take() else {
            return self.term.reconcile_resize_transaction_to_viewport();
        };

        canonical.term.resize(GridSize {
            columns: self.columns,
            rows: self.rows,
        });
        discard_listener_output(&canonical.listener);
        let (_, history_after) = canonical.term.reconcile_resize_transaction_to_viewport();
        discard_listener_output(&canonical.listener);

        // Only the displayed branch can own replies. Preserve any reply queued immediately before
        // the atomic branch replacement; the canonical parser's duplicate replies were discarded.
        let pending_writes = std::mem::take(
            &mut *self
                .listener
                .pty_writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        canonical
            .listener
            .pty_writes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extend(pending_writes);

        self.term = canonical.term;
        self.processor = canonical.processor;
        self.listener = canonical.listener;
        // One drag is not one commit: the app coalesces `Resized` on a silence, so a drag that
        // pauses and moves again commits a second pseudoconsole size inside this same transaction.
        // The branch just installed is the child's grid as of this commit, which makes it the only
        // honest starting point for the next one — so re-arm immediately. Without this the second
        // commit inherited the path-dependent displayed branch and reintroduced the S12 desync.
        self.arm_resize_canonical();
        (history_before, history_after)
    }

    pub fn visible_text(&self) -> Vec<String> {
        snapshot(&self.term)
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.c)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    /// One visible row of the grid, as stable captured cells.
    ///
    /// **Captured once per distinct row content, not once per caller.** A
    /// single published frame asked for the whole grid at least twice — once to
    /// project it and once more to build the live detection context — and each
    /// ask cloned eighty vendor cells and rebuilt eighty captured ones. The
    /// row's own fingerprint is the key: it hashes exactly the vendor fields
    /// this capture reads, without allocating, so a hit is a fact about the
    /// cells rather than a guess about who has touched them since. **Nothing
    /// invalidates this cache, because content is the key** — a row that
    /// changed hashes differently and is captured again.
    ///
    /// The one field the fingerprint leaves out is a hyperlink's *id*, which
    /// the vendor synthesizes per emission; [`CellHyperlink`] compares and
    /// hashes on the uri alone for that exact reason, so two captures the
    /// fingerprint calls equal are equal everywhere this workspace looks.
    pub fn visible_row(&self, row: u32) -> Option<CapturedRow> {
        if row >= self.rows.get() {
            return None;
        }
        let fingerprint =
            captured_row_fingerprint(&self.term, row as usize, self.row_fingerprint_seed);
        let mut cache = self.captured_rows.borrow_mut();
        let rows = self.rows.get() as usize;
        if cache.len() != rows {
            cache.clear();
            cache.resize(rows, None);
        }
        if let Some((cached_fingerprint, cached)) = &cache[row as usize]
            && *cached_fingerprint == fingerprint
        {
            return Some(cached.clone());
        }
        self.captures.set(self.captures.get().saturating_add(1));
        let cells = (0..self.columns.get())
            .map(|column| self.term.grid()[Line(row as i32)][Column(column as usize)].clone())
            .collect::<Vec<_>>();
        let captured = to_captured_row(&cells);
        cache[row as usize] = Some((fingerprint, captured.clone()));
        Some(captured)
    }

    /// How many rows have been captured out of this terminal since it opened.
    /// See [`Self::captures`].
    pub fn captures(&self) -> u64 {
        self.captures.get()
    }

    /// Whether `row` soft-wraps into the row below it — the `continues` flag of `visible_row`, read
    /// without capturing the row.
    ///
    /// The affordance scan asks this once per presentation row of every published frame, only to
    /// decide where one logical line ends, so capturing (and cloning) a whole row of cells for one
    /// bit is the wrong price. The bit itself is where the capture reads it: WRAPLINE on the row's
    /// last cell.
    pub fn visible_row_continues(&self, row: u32) -> bool {
        let columns = self.columns.get() as usize;
        row < self.rows.get()
            && columns != 0
            && self.term.grid()[Line(row as i32)][Column(columns - 1)]
                .flags
                .contains(Flags::WRAPLINE)
    }

    pub(crate) fn visible_row_fingerprint(&self, row: u32) -> Option<CapturedRowFingerprint> {
        (row < self.rows.get())
            .then(|| captured_row_fingerprint(&self.term, row as usize, self.row_fingerprint_seed))
    }

    pub fn cursor(&self) -> TerminalCursor {
        let point = self.term.grid().cursor.point;
        TerminalCursor {
            row: point.line.0.max(0) as u32,
            column: point.column.0 as u32,
            visible: self.term.mode().contains(TermMode::SHOW_CURSOR),
        }
    }

    /// Whether the cursor's most recent physical-row placement was CUP/HVP rather than stream
    /// progression or another cursor-motion family.
    pub(crate) fn cursor_row_was_explicitly_positioned(&self) -> bool {
        self.cursor_row_positioned_explicitly
    }

    /// Read DEC private modes from the vendor terminal, the single protocol-state authority.
    pub fn application_cursor_mode(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    pub fn bracketed_paste_mode(&self) -> bool {
        self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    pub fn modes(&self) -> TerminalModes {
        let mode = self.term.mode();
        let mouse_tracking = if mode.contains(TermMode::MOUSE_MOTION) {
            MouseTracking::Motion
        } else if mode.contains(TermMode::MOUSE_DRAG) {
            MouseTracking::Drag
        } else if mode.contains(TermMode::MOUSE_REPORT_CLICK) {
            MouseTracking::Click
        } else {
            MouseTracking::Off
        };
        TerminalModes {
            alternate_screen: mode.contains(TermMode::ALT_SCREEN),
            alternate_scroll: mode.contains(TermMode::ALTERNATE_SCROLL),
            sgr_mouse: mode.contains(TermMode::SGR_MOUSE),
            mouse_tracking,
            focus_reporting: mode.contains(TermMode::FOCUS_IN_OUT),
        }
    }

    /// Turn off the input modes that only a *program* ever asks for, and leave
    /// every mode the shell itself uses exactly where it is.
    ///
    /// A mouse-tracking mode is switched on by the program that wants to read
    /// mouse reports, and switched off by that same program on its way out. When
    /// the program never gets out — killed, crashed, or an exit path that did not
    /// run to the end — nobody switches it off: ConPTY does not send `?1003l` on
    /// a dead child's behalf, and there is no other party to the protocol who
    /// could. The mode then outlives its owner, and whoever is standing at the
    /// front afterwards inherits it (see [`DualPlaneSession`]'s prompt-start
    /// handler, the one caller, for when that inheritance is provably wrong).
    ///
    /// The list is exactly the modes a shell has no use for: the three mouse
    /// tracking levels (`1000`/`1002`/`1003`) and the two report encodings the
    /// vendor terminal knows (`1005`/`1006` — `1015` is not a mode this terminal
    /// can be in, so there is nothing of it to retire). Bracketed paste, the
    /// alternate screen, alternate scroll and the keyboard modes are deliberately
    /// absent: a shell and its line editor use those themselves.
    ///
    /// **Focus reporting (`1004`) is deliberately absent too, and it is the one
    /// entry that had to be taken back off** (user ruling, 2026-08-21). It reads
    /// like the same illness with `\e[I`/`\e[O` for a symptom, and it was on the
    /// list for a day on that reading. Then the A/B probe against a real
    /// `pwsh 7.6.5` over a real ConPTY showed **every session receiving
    /// `\e[?1004h` before any program runs**, right behind ConPTY's own `\e[1t` /
    /// `\e[c` handshake and `\e[?9001h`: ConPTY wants focus events because ConPTY
    /// consumes them itself and hands the console app a `FOCUS_EVENT`. A mode
    /// that has been on since the session's first byte is not evidence of a dead
    /// program, and switching it off here would be this terminal countermanding
    /// its own transport. [`TerminalModes::focus_reporting`] keeps reporting it —
    /// that bit is a true fact either way.
    ///
    /// It goes through the vendor's own [`Handler`] entry point rather than at
    /// the mode bits, so this is the same state change the escape sequence would
    /// have made, down to the `MouseCursorDirty` the vendor emits with it. The
    /// resize oracle is stepped alongside for the reason it is stepped alongside
    /// every other byte: a shadow terminal that disagreed about the modes would
    /// answer a resize as a different terminal.
    pub fn retire_program_input_modes(&mut self) {
        for mode in [
            NamedPrivateMode::ReportMouseClicks,
            NamedPrivateMode::ReportCellMouseMotion,
            NamedPrivateMode::ReportAllMouseMotion,
            NamedPrivateMode::Utf8Mouse,
            NamedPrivateMode::SgrMouse,
        ] {
            self.term.unset_private_mode(PrivateMode::Named(mode));
            if let Some(canonical) = self.resize_canonical.as_mut() {
                canonical.term.unset_private_mode(PrivateMode::Named(mode));
            }
        }
    }

    /// Drain protocol replies generated by the terminal state machine (for example DSR).
    ///
    /// Colour queries are resolved here rather than where they were heard, and
    /// in the order they were asked - see [`PendingReply`].
    pub fn take_pty_writes(&self) -> Vec<Vec<u8>> {
        let pending = std::mem::take(
            &mut *self
                .listener
                .pty_writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        let colors = self.term.colors();
        pending
            .into_iter()
            .filter_map(|reply| reply.bytes(|index| colors[index], self.color_palette.as_ref()))
            .collect()
    }

    fn drain_adapter_events(&self) -> Vec<AdapterEvent> {
        std::mem::take(
            &mut *self
                .listener
                .adapter_events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }

    fn drain_transcript_events(&mut self) -> Vec<AdapterEvent> {
        let transcript_events = std::mem::take(&mut *lock_events(&self.listener));
        transcript_events
            .into_iter()
            .map(|event| match event {
                TranscriptEvent::ScrollOut { cause, rows } => AdapterEvent::RowsRemoved {
                    context: removal_context(cause),
                    rows: rows
                        .into_iter()
                        .map(|row| RemovedLiveRow {
                            live_row: row.live_row as u32,
                            row: to_captured_row(&row.cells),
                        })
                        .collect(),
                },
                TranscriptEvent::GridScrolled => AdapterEvent::GridScrolled,
                TranscriptEvent::ScreenCleared => AdapterEvent::ScreenCleared,
                TranscriptEvent::ClearHistory => AdapterEvent::ClearHistory,
                TranscriptEvent::Reset => AdapterEvent::Reset,
                TranscriptEvent::Deccolm => AdapterEvent::Deccolm,
                TranscriptEvent::PrimaryParked => AdapterEvent::PrimaryParked,
                TranscriptEvent::PrimaryRestored => AdapterEvent::PrimaryRestored,
            })
            .collect()
    }

    fn observe_parser_boundary(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            let execute_at_ground = !self.parser_sequence_open;
            self.parser_tail.push(byte);
            let mut performer = BoundaryPerformer {
                execute_at_ground,
                ..BoundaryPerformer::default()
            };
            self.parser_boundary
                .advance(&mut performer, std::slice::from_ref(&byte));
            if performer.bell {
                self.listener
                    .adapter_events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(AdapterEvent::Bell);
            }
            if performer.focus_reports_requested {
                // A new subscriber has nothing to inherit: whatever was last
                // said was said to whoever asked before it. See
                // [`AnnouncedFocus`] — this is the "re-enabled" half of the
                // subscription edge, and on this platform it is the *only* half
                // a child ever gets.
                self.announced_focus = AnnouncedFocus::Unknown;
            }
            if performer.complete {
                self.parser_sequence_open = byte == 0x1b;
            } else if !self.parser_sequence_open {
                self.parser_sequence_open = true;
            }
            if let Some(explicit) = performer.cursor_row_positioned_explicitly {
                self.cursor_row_positioned_explicitly = explicit;
            }

            if performer.dcs_hook {
                self.parser_dcs_active = true;
            } else if performer.dcs_put && self.parser_dcs_active {
                // Once the DCS hook has selected its handler, payload bytes do not affect parser
                // state. The disposable seed term must only replay the introducer, not retain an
                // unbounded sixel/image payload.
                self.parser_tail.pop();
            }

            if performer.sync_start {
                self.parser_sync_active = true;
            } else if performer.sync_end {
                self.parser_sync_active = false;
                self.parser_tail.clear();
            } else if performer.complete && !self.parser_sync_active {
                self.parser_dcs_active = false;
                self.parser_tail.clear();
                // ESC can terminate OSC/DCS while simultaneously starting the ST escape. Keep it
                // as the seed for the parser's new Escape state.
                if byte == 0x1b {
                    self.parser_tail.push(byte);
                }
            }
        }
    }
}

fn removal_context(cause: ScrollOutCause) -> RemovalContext {
    let stable_screen = |screen| match screen {
        TranscriptScreen::Primary => RemovalScreen::Primary,
        TranscriptScreen::Alternate => RemovalScreen::Alternate,
    };
    let stable_scope = |scope| match scope {
        ScrollRegionScope::FullScreen => RemovalScope::FullScreen,
        ScrollRegionScope::Partial => RemovalScope::Partial,
    };
    match cause {
        ScrollOutCause::Normal { screen, scope } => RemovalContext {
            cause: RemovalCause::NormalScroll,
            screen: stable_screen(screen),
            scope: stable_scope(scope),
        },
        ScrollOutCause::DeleteLines { screen, scope } => RemovalContext {
            cause: RemovalCause::DeleteLines,
            screen: stable_screen(screen),
            scope: stable_scope(scope),
        },
        ScrollOutCause::Resize => RemovalContext {
            cause: RemovalCause::Resize,
            screen: RemovalScreen::Primary,
            scope: RemovalScope::FullScreen,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nz(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).unwrap()
    }

    fn apply_r2_extreme_resize_trace(terminal: &mut TerminalAdapter) {
        let sizes = [
            (95, 24),
            (62, 16),
            (48, 12),
            (38, 10),
            (28, 8),
            (25, 8),
            (23, 7),
            (20, 7),
            (18, 6),
            (16, 6),
            (16, 5),
            (16, 6),
            (19, 7),
            (23, 8),
            (26, 8),
            (29, 9),
            (36, 11),
            (41, 12),
            (46, 14),
            (49, 14),
            (52, 15),
            (56, 16),
            (61, 17),
            (66, 17),
            (71, 18),
            (77, 20),
            (85, 21),
            (78, 19),
            (66, 17),
            (60, 16),
            (52, 14),
            (25, 9),
            (13, 6),
            (12, 6),
            (15, 7),
            (33, 10),
            (51, 13),
            (60, 15),
            (61, 15),
            (47, 12),
            (23, 7),
            (11, 3),
            (11, 4),
            (11, 6),
            (22, 8),
            (30, 9),
            (34, 10),
            (37, 10),
            (38, 10),
            (38, 11),
            (38, 10),
            (26, 8),
            (12, 6),
            (11, 5),
            (13, 6),
            (35, 10),
            (46, 12),
            (49, 12),
            (43, 10),
            (39, 10),
            (38, 10),
            (37, 10),
            (35, 9),
            (34, 9),
            (31, 9),
            (30, 9),
        ];
        for (columns, rows) in sizes {
            terminal.resize(nz(columns), nz(rows));
        }
    }

    fn removed_context(events: &[AdapterEvent]) -> Option<RemovalContext> {
        events.iter().find_map(|event| match event {
            AdapterEvent::RowsRemoved { context, .. } => Some(*context),
            _ => None,
        })
    }

    #[test]
    fn full_screen_scroll_reports_cells_without_owning_history() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(2));
        let events = terminal.feed(b"one\r\ntwo\r\nthree");
        assert_eq!(terminal.alacritty_history_size(), 0);
        assert_eq!(
            removed_context(&events),
            Some(RemovalContext {
                cause: RemovalCause::NormalScroll,
                screen: RemovalScreen::Primary,
                scope: RemovalScope::FullScreen,
            })
        );
        assert!(matches!(
            events
                .iter()
                .find(|event| matches!(event, AdapterEvent::RowsRemoved { .. }))
                .unwrap(),
            AdapterEvent::RowsRemoved { rows, .. }
                if rows.first().is_some_and(|row| row.row.cells[0].text == "o")
        ));
    }

    #[test]
    fn local_scroll_delete_lines_and_alt_screen_are_reported_as_facts() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(4));
        terminal.feed(b"a\r\nb\r\nc\r\nd");
        let local = terminal.feed(b"\x1b[2;3r\x1b[3;1H\n");
        assert_eq!(
            removed_context(&local),
            Some(RemovalContext {
                cause: RemovalCause::NormalScroll,
                screen: RemovalScreen::Primary,
                scope: RemovalScope::Partial,
            })
        );

        let deleted = terminal.feed(b"\x1b[r\x1b[1;1H\x1b[1M");
        assert_eq!(
            removed_context(&deleted),
            Some(RemovalContext {
                cause: RemovalCause::DeleteLines,
                screen: RemovalScreen::Primary,
                scope: RemovalScope::FullScreen,
            })
        );

        terminal.feed(b"\x1b[?1049h");
        let alternate = terminal.feed(b"1\r\n2\r\n3\r\n4\r\n5");
        assert_eq!(
            removed_context(&alternate),
            Some(RemovalContext {
                cause: RemovalCause::NormalScroll,
                screen: RemovalScreen::Alternate,
                scope: RemovalScope::FullScreen,
            })
        );
    }

    #[test]
    fn explicit_screen_scroll_is_not_a_transcript_removal_fact() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(4));
        terminal.feed(b"a\r\nb\r\nc\r\nd");

        let explicit = terminal.feed(b"\x1b[S");
        assert!(explicit.contains(&AdapterEvent::GridScrolled));
        assert!(
            !explicit
                .iter()
                .any(|event| matches!(event, AdapterEvent::RowsRemoved { .. }))
        );
        assert_eq!(terminal.visible_text(), ["b", "c", "d", ""]);

        // LF at the bottom remains output scroll and still carries exact removed cells.
        let output = terminal.feed(b"\x1b[4;1H\n");
        assert_eq!(
            removed_context(&output),
            Some(RemovalContext {
                cause: RemovalCause::NormalScroll,
                screen: RemovalScreen::Primary,
                scope: RemovalScope::FullScreen,
            })
        );
    }

    #[test]
    fn parser_boundary_distinguishes_cup_hvp_from_natural_row_progression() {
        let mut terminal = TerminalAdapter::new(nz(80), nz(8));

        terminal.feed(b"\x1b[");
        assert!(!terminal.cursor_row_was_explicitly_positioned());
        terminal.feed(b"3;1H");
        assert!(terminal.cursor_row_was_explicitly_positioned());
        terminal.feed(b"\x1b[?25h\x1b[0m");
        assert!(
            terminal.cursor_row_was_explicitly_positioned(),
            "visibility and presentation controls do not reposition the cursor"
        );

        terminal.feed(b"\r\n");
        assert!(
            !terminal.cursor_row_was_explicitly_positioned(),
            "CR/LF is natural stream progression, not an editor placement"
        );
        terminal.feed(b"\x1b[4;2f");
        assert!(
            terminal.cursor_row_was_explicitly_positioned(),
            "HVP belongs to the same absolute-placement family as CUP"
        );
        terminal.feed(b"\x1b[B");
        assert!(
            !terminal.cursor_row_was_explicitly_positioned(),
            "a later non-CUP row motion replaces the placement fact"
        );
        terminal.feed(b"\x1b[2;1H");
        terminal.resize(nz(100), nz(10));
        assert!(
            !terminal.cursor_row_was_explicitly_positioned(),
            "resize invalidates the old physical-row placement"
        );
    }

    #[test]
    fn row_fingerprint_covers_text_sgr_combining_and_wide_semantics() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(1));
        terminal.feed(b"\x1b[31mA");
        let red = terminal.visible_row(0).unwrap();
        let red_fingerprint = terminal.visible_row_fingerprint(0).unwrap();

        terminal.feed(b"\x1b[1;1H\x1b[2K\x1b[31mA");
        assert_eq!(terminal.visible_row(0).unwrap(), red);
        assert_eq!(
            terminal.visible_row_fingerprint(0).unwrap(),
            red_fingerprint
        );

        terminal.feed(b"\x1b[1;1H\x1b[2K\x1b[32mA");
        assert_ne!(terminal.visible_row(0).unwrap(), red);
        assert_ne!(
            terminal.visible_row_fingerprint(0).unwrap(),
            red_fingerprint
        );

        terminal.feed("\x1b[1;1H\x1b[2Ke\u{301}".as_bytes());
        let combining = terminal.visible_row_fingerprint(0).unwrap();
        terminal.feed("\x1b[1;1H\x1b[2K\u{754c}".as_bytes());
        let wide = terminal.visible_row(0).unwrap();
        assert!(
            wide.cells[0]
                .style
                .flags
                .contains(bt_transcript::CellFlags::WIDE_CHAR)
        );
        assert!(wide.cells[1].wide_spacer);
        assert_ne!(terminal.visible_row_fingerprint(0).unwrap(), combining);
    }

    #[test]
    fn oversized_delete_lines_reports_only_rows_inside_the_remaining_region() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(4));
        terminal.feed(b"a\r\nb\r\nc\r\nd");
        let events = terminal.feed(b"\x1b[4;1H\x1b[999M");
        let rows = events.iter().find_map(|event| match event {
            AdapterEvent::RowsRemoved { rows, .. } => Some(rows),
            _ => None,
        });
        assert_eq!(rows.map(Vec::len), Some(1));
    }

    #[test]
    fn resize_transaction_vendor_history_is_the_only_mutable_tail_owner() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(4));
        terminal.feed(b"a\r\nb\r\nc\r\nd");
        terminal.begin_resize_transaction();
        let events = terminal.resize(nz(8), nz(2));
        assert!(matches!(&events[0], AdapterEvent::RowsRemoved { rows, .. } if rows.len() == 2));
        assert_eq!(terminal.resize_transaction_history_size(), 2);

        terminal.resize(nz(8), nz(4));
        assert!(terminal.finish_resize_transaction().is_empty());
        assert_eq!(terminal.visible_text(), vec!["a", "b", "c", "d"]);
        assert_eq!(terminal.alacritty_history_size(), 0);
    }

    #[test]
    fn resize_history_can_transfer_to_staging_and_return_before_reflow() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(4));
        terminal.feed(b"r1\r\nr2\r\nr3\r\nr4");
        terminal.begin_resize_transaction();
        terminal.resize(nz(8), nz(2));

        let staged = terminal.stage_resize_transaction();
        assert_eq!(staged.len(), 2);
        assert_eq!(terminal.alacritty_history_size(), 0);
        assert_eq!(terminal.resize_transaction_history_size(), 2);

        assert_eq!(terminal.resume_resize_transaction(), 2);
        terminal.resize(nz(8), nz(4));
        assert!(terminal.stage_resize_transaction().is_empty());
        assert_eq!(terminal.visible_text(), ["r1", "r2", "r3", "r4"]);
    }

    #[test]
    fn r2_extreme_local_path_stays_at_the_coalesced_conpty_viewport() {
        const WARNING: &str = "Did not find path entry D:\\App\\Base\\anaconda3\\bin";
        const PROMPT: &str = "(base) PS D:\\Developer\\BetterTerminal> ";
        let input = format!("{WARNING}\r\n{PROMPT}");

        let mut direct = TerminalAdapter::new(nz(104), nz(26));
        direct.feed(input.as_bytes());
        direct.begin_resize_transaction();
        direct.resize(nz(30), nz(9));
        assert_eq!(direct.reconcile_resize_transaction_to_viewport(), (0, 0));
        let direct_rows = direct.visible_text();
        let direct_cursor = direct.cursor();
        assert_eq!((direct_cursor.row, direct_cursor.column), (3, 9));

        let mut projected = TerminalAdapter::new(nz(104), nz(26));
        projected.feed(input.as_bytes());
        projected.begin_resize_transaction();
        apply_r2_extreme_resize_trace(&mut projected);
        assert_eq!(projected.resize_transaction_history_size(), 0);
        assert_eq!(projected.visible_text(), direct_rows);
        assert_eq!(projected.cursor(), direct_cursor);
        assert!(projected.finish_resize_transaction().is_empty());

        let mut reconciled = TerminalAdapter::new(nz(104), nz(26));
        reconciled.feed(input.as_bytes());
        reconciled.begin_resize_transaction();
        apply_r2_extreme_resize_trace(&mut reconciled);
        assert_eq!(
            reconciled.reconcile_resize_transaction_to_viewport(),
            (0, 0)
        );
        assert_eq!(reconciled.visible_text(), direct_rows);
        assert_eq!(reconciled.cursor(), direct_cursor);
        assert!(reconciled.finish_resize_transaction().is_empty());
    }

    /// The local sizes one divider drag projects onto the grid before ConPTY hears anything. Both
    /// coalescing pins below are measured against these same bytes and these same sizes; the only
    /// difference between them is how many pseudoconsole commits the drag contains.
    const S12_STORM_SIZES: [(u32, u32); 44] = [
        (111, 20),
        (46, 7),
        (12, 1),
        (13, 2),
        (28, 7),
        (71, 15),
        (79, 16),
        (66, 14),
        (22, 7),
        (18, 6),
        (42, 12),
        (98, 21),
        (60, 14),
        (16, 6),
        (27, 9),
        (79, 17),
        (89, 19),
        (85, 18),
        (25, 7),
        (19, 7),
        (51, 11),
        (90, 16),
        (53, 11),
        (11, 5),
        (42, 10),
        (86, 15),
        (85, 15),
        (49, 10),
        (31, 8),
        (64, 13),
        (104, 18),
        (99, 17),
        (46, 10),
        (38, 9),
        (59, 14),
        (117, 21),
        (118, 21),
        (72, 13),
        (33, 9),
        (39, 10),
        (79, 18),
        (92, 20),
        (95, 20),
        (96, 20),
    ];

    /// Deterministic reduction of the S12 mix: soft wraps, CUP, save/restore, erase, and cursor
    /// visibility. The transient storm finishes with no native history, but its cursor is still
    /// path-dependent; this is exactly the branch the old history-only reconcile skipped.
    fn s12_storm_input() -> String {
        let mut state = 10u64;
        let mut input = String::new();
        for _ in 0..48 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let row = 1 + ((state >> 8) % 22);
            let column = 1 + ((state >> 16) % 118);
            let length = 1 + ((state >> 24) % 180) as usize;
            match state % 9 {
                0 => input.push_str(&format!("\x1b[{row};{column}H")),
                1 => input.push_str("\r\n"),
                2 => input.push_str("\x1b[2K"),
                3 => input.push_str("\x1b[K"),
                4 => input.push_str("\x1b7"),
                5 => input.push_str("\x1b8"),
                6 => input.push_str(&"q".repeat(length)),
                7 => input.push_str(&format!("\x1b[93m{}\x1b[0m", "p".repeat(length))),
                _ => input.push_str("\x1b[?25l\x1b[?25h"),
            }
        }
        input
    }

    #[test]
    fn coalesced_final_resize_replaces_the_path_dependent_live_branch() {
        let sizes = S12_STORM_SIZES;
        let input = s12_storm_input();

        let mut direct = TerminalAdapter::new(nz(119), nz(23));
        direct.feed(input.as_bytes());
        direct.begin_resize_transaction();
        direct.resize(nz(96), nz(20));
        direct.reconcile_resize_transaction_to_viewport();
        let direct_cursor = direct.cursor();
        let direct_rows = direct.visible_text();

        let mut storm = TerminalAdapter::new(nz(119), nz(23));
        storm.feed(input.as_bytes());
        storm.begin_resize_transaction();
        for (columns, rows) in sizes {
            storm.resize(nz(columns), nz(rows));
        }
        assert_eq!(storm.resize_transaction_history_size(), 0);
        assert_ne!(storm.cursor(), direct_cursor);

        assert_eq!(storm.reconcile_resize_transaction_to_viewport(), (0, 0));
        assert_eq!(storm.cursor(), direct_cursor);
        assert_eq!(storm.visible_text(), direct_rows);
    }

    /// One human drag is not one pseudoconsole commit.
    ///
    /// The app coalesces `Resized` on a 200 ms silence, so a drag that pauses to look, then moves
    /// again, commits a *second* pseudoconsole size while the same resize transaction is still open
    /// (the transaction only closes after the final request and the child's output have both been
    /// quiet). Every commit is a size the child really received, so after each one the displayed
    /// grid owes the same debt the first one does: it must be the child's bytes reflowed to the
    /// sizes ConPTY actually saw, never to the sizes only the pointer passed through.
    #[test]
    fn every_coalesced_commit_in_one_transaction_replaces_the_path_dependent_branch() {
        let (first_drag, second_drag) = S12_STORM_SIZES.split_at(S12_STORM_SIZES.len() / 2);
        let first_commit = (96, 20);
        let second_commit = (64, 14);
        let input = s12_storm_input();

        // What the child saw: the start size, then the two committed sizes, in order.
        let mut direct = TerminalAdapter::new(nz(119), nz(23));
        direct.feed(input.as_bytes());
        direct.begin_resize_transaction();
        direct.resize(nz(first_commit.0), nz(first_commit.1));
        direct.reconcile_resize_transaction_to_viewport();
        direct.resize(nz(second_commit.0), nz(second_commit.1));
        direct.reconcile_resize_transaction_to_viewport();
        let direct_cursor = direct.cursor();
        let direct_rows = direct.visible_text();

        // What the pointer did: two bursts of projected sizes, one commit at the end of each.
        let mut storm = TerminalAdapter::new(nz(119), nz(23));
        storm.feed(input.as_bytes());
        storm.begin_resize_transaction();
        for (columns, rows) in first_drag {
            storm.resize(nz(*columns), nz(*rows));
        }
        storm.resize(nz(first_commit.0), nz(first_commit.1));
        storm.reconcile_resize_transaction_to_viewport();
        for (columns, rows) in second_drag {
            storm.resize(nz(*columns), nz(*rows));
        }
        storm.resize(nz(second_commit.0), nz(second_commit.1));
        storm.reconcile_resize_transaction_to_viewport();

        assert_eq!(
            storm.cursor(),
            direct_cursor,
            "the second commit left the cursor on a row only the drag's intermediate sizes explain"
        );
        assert_eq!(
            storm.visible_text(),
            direct_rows,
            "the second commit published a screen the child's own sizes never produced"
        );
    }

    #[test]
    fn canonical_resize_branch_inherits_a_split_parser_sequence() {
        let mut direct = TerminalAdapter::new(nz(20), nz(4));
        direct.feed(b"prompt> \x1b[");
        direct.begin_resize_transaction();
        direct.feed(b"93mhistory\x1b[0m");
        direct.resize(nz(12), nz(4));
        direct.reconcile_resize_transaction_to_viewport();

        let mut storm = TerminalAdapter::new(nz(20), nz(4));
        storm.feed(b"prompt> \x1b[");
        storm.begin_resize_transaction();
        storm.resize(nz(5), nz(2));
        storm.feed(b"93mhistory\x1b[0m");
        storm.resize(nz(12), nz(4));
        storm.reconcile_resize_transaction_to_viewport();

        assert_eq!(storm.visible_text(), direct.visible_text());
        assert_eq!(storm.cursor(), direct.cursor());
    }

    #[test]
    fn canonical_resize_branch_inherits_a_buffered_synchronized_update() {
        let prefix = b"base\x1b[?2026h\x1b[93mheld";
        let suffix = b"-until-end\x1b[0m\x1b[?2026l";

        let mut direct = TerminalAdapter::new(nz(20), nz(4));
        direct.feed(prefix);
        assert!(!direct.parser_tail.is_empty());
        direct.begin_resize_transaction();
        direct.feed(suffix);
        direct.resize(nz(12), nz(4));
        direct.reconcile_resize_transaction_to_viewport();

        let mut storm = TerminalAdapter::new(nz(20), nz(4));
        storm.feed(prefix);
        storm.begin_resize_transaction();
        storm.resize(nz(5), nz(2));
        storm.feed(suffix);
        storm.resize(nz(12), nz(4));
        storm.reconcile_resize_transaction_to_viewport();

        assert!(storm.parser_tail.is_empty());
        assert_eq!(storm.visible_text(), direct.visible_text());
        assert_eq!(storm.cursor(), direct.cursor());
    }

    #[test]
    fn synchronized_update_exposes_deadline_until_esu_commits_the_buffer() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(4));
        terminal.feed(b"before\x1b[?2026h\rhidden");

        assert!(terminal.synchronized_update_deadline().is_some());
        assert_eq!(terminal.visible_text()[0], "before");

        terminal.feed(b"-until-esu\x1b[?2026l");
        assert!(terminal.synchronized_update_deadline().is_none());
        assert_eq!(terminal.visible_text()[0], "hidden-until-esu");
    }

    #[test]
    fn synchronized_update_timeout_commits_without_an_esu() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(4));
        terminal.feed(b"before\x1b[?2026h\rtimeout");

        assert!(terminal.synchronized_update_deadline().is_some());
        terminal.finish_synchronized_update();

        assert!(terminal.synchronized_update_deadline().is_none());
        assert_eq!(terminal.visible_text()[0], "timeout");
    }

    #[test]
    fn canonical_parser_seed_does_not_retain_dcs_payload() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(4));
        terminal.feed(b"\x1bPq");
        terminal.feed(&vec![b'x'; 64 * 1024]);

        assert!(terminal.parser_dcs_active);
        assert_eq!(terminal.parser_tail, b"\x1bPq");
        terminal.begin_resize_transaction();
        terminal.resize(nz(12), nz(3));
        terminal.feed(b"\x1b\\done");
        terminal.reconcile_resize_transaction_to_viewport();

        assert!(!terminal.parser_dcs_active);
        assert!(terminal.parser_tail.is_empty());
        assert!(
            terminal
                .visible_text()
                .iter()
                .any(|row| row.contains("done"))
        );
    }

    #[test]
    fn transaction_harvest_preserves_an_internal_user_blank_line() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(6));
        terminal.feed(b"top\r\n\r\nmiddle\r\nlower\r\ntail\r\nend");

        terminal.begin_resize_transaction();
        terminal.resize(nz(8), nz(3));
        let rows = terminal.finish_resize_transaction();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].cells[0].text, "t");
        assert!(rows[1].cells.iter().all(|cell| cell.text.trim().is_empty()));
        assert_eq!(rows[2].cells[0].text, "m");
        assert_eq!(terminal.alacritty_history_size(), 0);
    }

    #[test]
    fn native_transaction_tail_reflows_a_large_hard_line_without_loss() {
        let mut terminal = TerminalAdapter::new(nz(80), nz(104));
        let expected = (0..104)
            .map(|index| format!("{index:03}{}", "x".repeat(76)))
            .collect::<Vec<_>>();
        terminal.feed(expected.join("\r\n").as_bytes());

        terminal.begin_resize_transaction();
        terminal.resize(nz(80), nz(4));
        terminal.resize(nz(30), nz(4));
        terminal.resize(nz(80), nz(104));
        assert!(terminal.finish_resize_transaction().is_empty());

        assert_eq!(terminal.visible_text(), expected);
        assert_eq!(terminal.alacritty_history_size(), 0);
    }

    #[test]
    fn reset_and_deccolm_are_distinct_facts() {
        let mut terminal = TerminalAdapter::new(nz(4), nz(3));
        assert!(terminal.feed(b"\x1bc").contains(&AdapterEvent::Reset));
        assert!(terminal.feed(b"\x1b[?3h").contains(&AdapterEvent::Deccolm));
    }

    #[test]
    fn osc_title_events_are_ui_facts_without_grid_writes() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        assert_eq!(
            terminal.feed("\x1b]0;Claude ✳ 任务\x07".as_bytes()),
            vec![AdapterEvent::Title {
                title: "Claude ✳ 任务".to_owned(),
            }]
        );
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        assert_eq!(
            terminal.feed(b"\x1b[22;0t\x1b]2;temporary\x07\x1b[23;0t"),
            vec![
                AdapterEvent::Title {
                    title: "temporary".to_owned(),
                },
                AdapterEvent::ResetTitle,
            ]
        );
    }

    #[test]
    fn terminal_protocol_replies_are_exposed_to_the_single_pty_writer() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        terminal.feed(b"\x1b[6n");
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[1;1R".to_vec()]);
        assert!(terminal.take_pty_writes().is_empty());
    }

    #[test]
    fn input_modes_are_read_directly_from_vendor_decset_state() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        assert!(!terminal.application_cursor_mode());
        assert!(!terminal.bracketed_paste_mode());

        terminal.feed(b"\x1b[?1h\x1b[?2004h");
        assert!(terminal.application_cursor_mode());
        assert!(terminal.bracketed_paste_mode());

        terminal.feed(b"\x1b[?1l\x1b[?2004l");
        assert!(!terminal.application_cursor_mode());
        assert!(!terminal.bracketed_paste_mode());

        terminal.feed(b"\x1b[?1049h\x1b[?1007h\x1b[?1002h\x1b[?1006h");
        assert_eq!(
            terminal.modes(),
            TerminalModes {
                alternate_screen: true,
                alternate_scroll: true,
                sgr_mouse: true,
                mouse_tracking: MouseTracking::Drag,
                focus_reporting: false,
            }
        );
        terminal.feed(b"\x1b[?1004h");
        assert!(terminal.modes().focus_reporting);
        terminal.feed(b"\x1b[?1004l");
        assert!(!terminal.modes().focus_reporting);
        terminal.feed(b"\x1b[?1003h");
        assert_eq!(terminal.modes().mouse_tracking, MouseTracking::Motion);
        terminal.feed(b"\x1b[?1049l\x1b[?1007l\x1b[?1002l\x1b[?1003l\x1b[?1006l");
        assert_eq!(terminal.modes().mouse_tracking, MouseTracking::Off);
        assert!(!terminal.modes().alternate_screen);
    }

    /// The light scheme this product ships, as far as a colour query can see it.
    fn light_palette() -> TerminalPalette {
        let mut ansi = [[0x00, 0x00, 0x00]; 16];
        ansi[2] = [0x00, 0xa6, 0x00];
        TerminalPalette {
            canvas: TerminalCanvas::Light,
            background: [0xff, 0xff, 0xff],
            foreground: [0x37, 0x35, 0x2f],
            cursor: [0x37, 0x35, 0x2f],
            ansi,
        }
    }

    fn dark_palette() -> TerminalPalette {
        TerminalPalette {
            canvas: TerminalCanvas::Dark,
            background: [0x1b, 0x1b, 0x1b],
            foreground: [0xe1, 0xe1, 0xe1],
            cursor: [0xd4, 0xd4, 0xd4],
            ansi: [[0x0c, 0x0c, 0x0c]; 16],
        }
    }

    #[test]
    fn a_colour_query_is_answered_out_of_the_palette_the_window_is_wearing() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        terminal.set_color_palette(light_palette());

        terminal.feed(b"\x1b]11;?\x1b\\");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\".to_vec()]
        );

        terminal.feed(b"\x1b]10;?\x1b\\");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![b"\x1b]10;rgb:3737/3535/2f2f\x1b\\".to_vec()]
        );

        terminal.feed(b"\x1b]12;?\x1b\\");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![b"\x1b]12;rgb:3737/3535/2f2f\x1b\\".to_vec()]
        );
    }

    #[test]
    fn osc_4_answers_the_schemes_sixteen_and_the_protocols_two_hundred_forty() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        terminal.set_color_palette(light_palette());

        terminal.feed(b"\x1b]4;2;?\x1b\\");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![b"\x1b]4;2;rgb:0000/a6a6/0000\x1b\\".to_vec()]
        );

        // 196 and 255 are the cube and the grey ramp: nobody's scheme, so the
        // answer must be the same on either canvas.
        terminal.feed(b"\x1b]4;196;?\x1b\\\x1b]4;255;?\x1b\\");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![
                b"\x1b]4;196;rgb:ffff/0000/0000\x1b\\".to_vec(),
                b"\x1b]4;255;rgb:eeee/eeee/eeee\x1b\\".to_vec(),
            ]
        );
    }

    #[test]
    fn a_colour_answer_is_terminated_the_way_the_question_was() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        terminal.set_color_palette(light_palette());

        terminal.feed(b"\x1b]11;?\x07");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![b"\x1b]11;rgb:ffff/ffff/ffff\x07".to_vec()]
        );
    }

    #[test]
    fn colour_answers_keep_their_place_in_the_reply_stream() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        terminal.set_color_palette(light_palette());

        terminal.feed(b"\x1b]11;?\x1b\\\x1b[6n");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![
                b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\".to_vec(),
                b"\x1b[1;1R".to_vec(),
            ]
        );
    }

    #[test]
    fn a_colour_query_before_the_window_has_said_what_it_wears_is_not_answered() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        terminal.feed(b"\x1b]11;?\x1b\\\x1b[6n");
        // Silence for the colour, and the DSR reply still on time behind it.
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[1;1R".to_vec()]);
    }

    #[test]
    fn a_new_palette_answers_the_next_query_and_never_the_last_one() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        terminal.set_color_palette(dark_palette());
        terminal.feed(b"\x1b]11;?\x1b\\");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![b"\x1b]11;rgb:1b1b/1b1b/1b1b\x1b\\".to_vec()]
        );

        terminal.set_color_palette(light_palette());
        terminal.feed(b"\x1b]11;?\x1b\\");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\".to_vec()]
        );
    }

    #[test]
    fn a_colour_the_child_set_itself_outranks_the_windows_palette() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        terminal.set_color_palette(light_palette());

        terminal.feed(b"\x1b]11;#123456\x1b\\\x1b]11;?\x1b\\");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![b"\x1b]11;rgb:1212/3434/5656\x1b\\".to_vec()]
        );

        // And it is still the child's colour after the window repaints itself
        // in another scheme: the terminal was told, and does not forget.
        terminal.set_color_palette(dark_palette());
        terminal.feed(b"\x1b]11;?\x1b\\");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![b"\x1b]11;rgb:1212/3434/5656\x1b\\".to_vec()]
        );
    }

    #[test]
    fn a_palette_survives_the_branch_swap_at_the_end_of_a_resize() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        terminal.set_color_palette(light_palette());

        terminal.begin_resize_transaction();
        terminal.resize(nz(12), nz(3));
        terminal.reconcile_resize_transaction_to_viewport();
        terminal.finish_resize_transaction();

        terminal.feed(b"\x1b]11;?\x1b\\");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\".to_vec()]
        );
    }

    #[test]
    fn dec_mode_2031_query_set_and_reset_use_standard_decrqm_semantics() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(3));
        terminal.feed(b"\x1b[?2031$p");
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[?2031;2$y".to_vec()]);
        assert!(!terminal.theme_update_notification());

        terminal.feed(b"\x1b[?2031h\x1b[?2031$p");
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[?2031;1$y".to_vec()]);
        assert!(terminal.theme_update_notification());

        terminal.feed(b"\x1b[?2031l\x1b[?2031$p");
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[?2031;2$y".to_vec()]);
        assert!(!terminal.theme_update_notification());
    }

    #[test]
    fn a_canvas_change_is_announced_only_to_a_pane_that_subscribed_to_it() {
        let mut unsubscribed = TerminalAdapter::new(nz(8), nz(3));
        unsubscribed.set_color_palette(dark_palette());
        unsubscribed.set_color_palette(light_palette());
        assert!(unsubscribed.take_pty_writes().is_empty());

        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        terminal.set_color_palette(dark_palette());
        terminal.feed(b"\x1b[?2031h");
        assert!(terminal.take_pty_writes().is_empty());

        terminal.set_color_palette(light_palette());
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[?997;2n".to_vec()]);

        // A repaint in the same canvas is not a change and says nothing.
        terminal.set_color_palette(light_palette());
        assert!(terminal.take_pty_writes().is_empty());

        terminal.set_color_palette(dark_palette());
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[?997;1n".to_vec()]);
    }

    /// RED GATE (attention block, slice A0.5) — **a pane that subscribed to
    /// focus reporting is told when the keyboard arrives and when it leaves.**
    ///
    /// This is the byte Folio has never sent. Every agent that gates a bell on
    /// "is anybody looking" — Claude Code, codex and opencode all do — reads
    /// `CSI I`/`CSI O`, and a terminal that sends neither is a terminal that
    /// says you are present forever, so it never interrupts you.
    ///
    /// Mutation: report a level instead of an edge, or send to a pane that never
    /// subscribed, or drop either direction.
    #[test]
    fn a_subscribed_pane_is_told_when_the_keyboard_arrives_and_when_it_leaves() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        terminal.feed(b"\x1b[?1004h");
        assert!(terminal.focus_reporting());
        // The first answer after a subscription is owed whichever way it falls:
        // this pane has the keyboard, and nobody had told it so.
        terminal.set_keyboard_focus(true);
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[I".to_vec()]);

        // A level repeated is not a transition and says nothing.
        terminal.set_keyboard_focus(true);
        terminal.set_keyboard_focus(true);
        assert!(terminal.take_pty_writes().is_empty());

        terminal.set_keyboard_focus(false);
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[O".to_vec()]);
        terminal.set_keyboard_focus(false);
        assert!(terminal.take_pty_writes().is_empty());

        terminal.set_keyboard_focus(true);
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[I".to_vec()]);
    }

    /// RED GATE (attention block, slice A0.5) — **the initial state is owed to
    /// the subscription, not to a change in the window.**
    ///
    /// A pane born in a background tab, or in a window that is not the
    /// foreground one, never sees a `focused → unfocused` edge: it was never
    /// focused. Waiting for one leaves its child believing it has the keyboard
    /// for as long as it lives. So the subscription itself is an edge, and a
    /// program that turns 1004 off and on again is answered afresh rather than
    /// left holding what the previous subscriber was told.
    ///
    /// Mutation: keep the last answer across an unsubscribe, or make the first
    /// call after `?1004h` say nothing.
    #[test]
    fn a_pane_born_without_the_keyboard_is_told_so_the_moment_it_subscribes() {
        let mut background = TerminalAdapter::new(nz(8), nz(3));
        // Before the subscription there is nobody to tell, and the sequence
        // would be read as input by a program that never asked for it.
        background.set_keyboard_focus(false);
        assert!(background.take_pty_writes().is_empty());

        background.feed(b"\x1b[?1004h");
        background.set_keyboard_focus(false);
        assert_eq!(background.take_pty_writes(), vec![b"\x1b[O".to_vec()]);

        // Off, and the standing goes with it: what was said was said to a
        // subscriber that has gone.
        background.feed(b"\x1b[?1004l");
        background.set_keyboard_focus(false);
        background.set_keyboard_focus(false);
        assert!(background.take_pty_writes().is_empty());

        background.feed(b"\x1b[?1004h");
        background.set_keyboard_focus(false);
        assert_eq!(background.take_pty_writes(), vec![b"\x1b[O".to_vec()]);
    }

    /// RED GATE (attention block, slice A0.5) — **asking is the subscription
    /// edge, even when the mode was already on.**
    ///
    /// ConPTY enables 1004 for the transport before any program runs, so the
    /// child's own `?1004h` never changes a level. Measured on 2026-08-25: a
    /// real `claude.exe` born without the keyboard was sent its `CSI O` before it
    /// started, the console host dropped it because no client had asked for focus
    /// events yet, and the agent went on believing it was being read — the very
    /// belief this report exists to correct. So every ask restates the standing.
    ///
    /// Every parameter of the sequence is read, not just the first: `?1004;1006h`
    /// is one program asking for two things.
    ///
    /// Mutation: reset only on a level change, or look at the head of the
    /// parameter list alone.
    #[test]
    fn a_second_subscriber_is_told_where_it_stands_although_the_mode_was_already_on() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        // The transport's own subscription, before any program runs.
        terminal.feed(b"\x1b[?1004h");
        terminal.set_keyboard_focus(false);
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[O".to_vec()]);

        // The program starts and asks for the same mode. Nothing about the
        // terminal's state changed; everything about who is listening did.
        terminal.feed(b"\x1b[?1004h");
        terminal.set_keyboard_focus(false);
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[O".to_vec()]);

        // And it is still an edge report: the standing is not restated again
        // until somebody asks again or it changes.
        terminal.set_keyboard_focus(false);
        assert!(terminal.take_pty_writes().is_empty());

        // Asked for among other modes, and split across two feeds — the ask is
        // read off a parser, not off a byte pattern.
        terminal.feed(b"\x1b[?1000;10");
        terminal.feed(b"04;1006h");
        terminal.set_keyboard_focus(false);
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[O".to_vec()]);

        // A mode list without 1004 in it is not an ask.
        terminal.feed(b"\x1b[?1000;1006h");
        terminal.set_keyboard_focus(false);
        assert!(terminal.take_pty_writes().is_empty());
    }

    #[test]
    fn dec_mode_2027_query_set_and_reset_use_standard_decrqm_semantics() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(3));
        terminal.feed(b"\x1b[?2027$p");
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[?2027;2$y".to_vec()]);

        terminal.feed(b"\x1b[?2027h\x1b[?2027$p");
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[?2027;1$y".to_vec()]);

        terminal.feed(b"\x1b[?2027l\x1b[?2027$p");
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[?2027;2$y".to_vec()]);
    }

    #[test]
    fn grapheme_mode_clusters_the_m1_width_matrix_while_legacy_mode_stays_compatible() {
        let cases = [
            ("👨‍👩‍👧‍👦", 2),
            ("👍🏽", 2),
            ("e\u{301}", 1),
            ("☂\u{fe0e}", 1),
            ("☂\u{fe0f}", 2),
            ("⌚\u{fe0e}", 1),
            ("🇺🇸", 2),
            ("☆", 1),
        ];
        for (text, expected) in cases {
            let mut terminal = TerminalAdapter::new(nz(20), nz(3));
            terminal.feed(b"\x1b[?2027h");
            for byte in text.as_bytes() {
                terminal.feed(std::slice::from_ref(byte));
            }
            assert_eq!(terminal.cursor().column, expected, "{text:?}");
            let row = terminal.visible_row(0).unwrap();
            assert_eq!(row.cells[0].text, text, "{text:?}");
            assert_eq!(
                row.cells.get(1).is_some_and(|cell| cell.wide_spacer),
                expected == 2
            );
        }

        let mut legacy = TerminalAdapter::new(nz(20), nz(3));
        legacy.feed("👨‍👩‍👧‍👦".as_bytes());
        assert_eq!(legacy.cursor().column, 8);
        legacy.feed(b"\x1b[?2027h\r");
        legacy.feed("👨‍👩‍👧‍👦".as_bytes());
        assert_eq!(legacy.cursor().column, 2);
        legacy.feed(b"\x1b[?2027l\r");
        legacy.feed("👍🏽".as_bytes());
        assert_eq!(legacy.cursor().column, 4);
    }

    #[test]
    fn decawm_margin_pressure_then_decrst_2027_cannot_consume_legacy_text() {
        let mut terminal = TerminalAdapter::new(nz(80), nz(24));
        terminal.feed(b"\x1b[?2027h\x1b[?7l\x1b[999G");
        terminal.feed("☂\u{fe0f}".as_bytes());
        terminal.feed(b"\r\n\x1b[?7hBT_PANIC_SURVIVED\r\n\x1b[?2027l|");
        terminal.feed("👨\u{200d}👩\u{200d}👧\u{200d}👦".as_bytes());
        terminal.feed(b"|");

        let family_row = terminal
            .visible_text()
            .iter()
            .position(|row| row.starts_with('|'))
            .expect("post-DECRST family row remains visible");
        let row = terminal.visible_row(family_row as u32).unwrap();
        assert_eq!(row.cells[0].text, "|");
        assert!(row.cells[1].text.starts_with('👨'));
        assert_eq!(row.cells[9].text, "|");
        assert_eq!(terminal.cursor().column, 10);
        assert!(
            terminal
                .visible_text()
                .iter()
                .any(|row| row.contains("BT_PANIC_SURVIVED"))
        );
    }

    #[test]
    fn mixed_clusters_wrap_as_an_indivisible_wide_lead_and_spacer() {
        let mut terminal = TerminalAdapter::new(nz(4), nz(3));
        terminal.feed(b"\x1b[?2027habc");
        terminal.feed("👨‍👩‍👧‍👦中Z".as_bytes());

        let first = terminal.visible_row(0).unwrap();
        assert_eq!(first.cells[0].text, "a");
        assert_eq!(first.cells[3].text, " ");
        let second = terminal.visible_row(1).unwrap();
        assert_eq!(second.cells[0].text, "👨‍👩‍👧‍👦");
        assert!(
            second.cells[0]
                .style
                .flags
                .contains(bt_transcript::CellFlags::WIDE_CHAR)
        );
        assert!(second.cells[1].wide_spacer);
        assert_eq!(second.cells[2].text, "中");
        assert!(second.cells[3].wide_spacer);
        let third = terminal.visible_row(2).unwrap();
        assert_eq!(third.cells[0].text, "Z");
    }

    #[test]
    fn late_vs_and_flag_width_changes_rewrite_atomically_at_the_right_margin() {
        for text in ["☂\u{fe0f}", "🇺🇸"] {
            let mut terminal = TerminalAdapter::new(nz(4), nz(3));
            terminal.feed(b"\x1b[?2027habc");
            terminal.feed(text.as_bytes());
            assert_eq!(terminal.cursor().row, 1, "{text:?}");
            assert_eq!(terminal.cursor().column, 2, "{text:?}");
            let first = terminal.visible_row(0).unwrap();
            assert_eq!(first.cells[3].text, " ", "{text:?}");
            let second = terminal.visible_row(1).unwrap();
            assert_eq!(second.cells[0].text, text, "{text:?}");
            assert!(second.cells[1].wide_spacer, "{text:?}");
        }

        let mut text_presentation = TerminalAdapter::new(nz(4), nz(3));
        text_presentation.feed(b"\x1b[?2027habc");
        text_presentation.feed("⌚\u{fe0e}".as_bytes());
        assert_eq!(text_presentation.cursor().row, 0);
        assert_eq!(text_presentation.cursor().column, 3);
        let first = text_presentation.visible_row(0).unwrap();
        assert_eq!(first.cells[3].text, "⌚\u{fe0e}");
        assert!(
            !first.cells[3]
                .style
                .flags
                .contains(bt_transcript::CellFlags::WIDE_CHAR)
        );
        let cleared = text_presentation.visible_row(1).unwrap();
        assert!(cleared.cells[0].text.trim().is_empty());
        assert!(!cleared.cells[0].wide_spacer);
    }

    #[test]
    fn late_cluster_width_changes_preserve_insert_mode_tail_cells() {
        let mut upgrade = TerminalAdapter::new(nz(8), nz(2));
        upgrade.feed(b"ABCDE\r\x1b[2C\x1b[4h\x1b[?2027h");
        upgrade.feed("🇺🇸".as_bytes());
        let row = upgrade.visible_row(0).unwrap();
        assert_eq!(row.cells[0].text, "A");
        assert_eq!(row.cells[1].text, "B");
        assert_eq!(row.cells[2].text, "🇺🇸");
        assert!(row.cells[3].wide_spacer);
        assert_eq!(row.cells[4].text, "C");
        assert_eq!(row.cells[5].text, "D");
        assert_eq!(row.cells[6].text, "E");

        let mut shrink = TerminalAdapter::new(nz(8), nz(2));
        shrink.feed(b"ABCDE\r\x1b[2C\x1b[4h\x1b[?2027h");
        shrink.feed("⌚\u{fe0e}".as_bytes());
        let row = shrink.visible_row(0).unwrap();
        assert_eq!(row.cells[2].text, "⌚\u{fe0e}");
        assert!(!row.cells[3].wide_spacer);
        assert_eq!(row.cells[3].text, "C");
        assert_eq!(row.cells[4].text, "D");
        assert_eq!(row.cells[5].text, "E");
    }

    #[test]
    fn resize_never_separates_cluster_text_from_its_wide_spacer() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        terminal.feed(b"\x1b[?2027hA");
        terminal.feed("👨‍👩‍👧‍👦".as_bytes());
        terminal.feed(b"BCDE");
        let events = terminal.resize(nz(8), nz(3));

        let cluster = (0..3).find_map(|row| {
            let row = terminal.visible_row(row)?;
            let column = row.cells.iter().position(|cell| cell.text == "👨‍👩‍👧‍👦")?;
            Some((row, column))
        });
        let (row, column) = cluster.unwrap_or_else(|| {
            panic!(
                "cluster survived resize: {:?}; {events:?}",
                terminal.visible_text()
            )
        });
        assert!(
            row.cells[column]
                .style
                .flags
                .contains(bt_transcript::CellFlags::WIDE_CHAR)
        );
        assert!(row.cells[column + 1].wide_spacer);
    }

    #[test]
    fn resize_between_codepoints_reanchors_the_in_progress_cluster() {
        let mut family = TerminalAdapter::new(nz(4), nz(3));
        family.feed(b"\x1b[?2027hA");
        family.feed("👨‍".as_bytes());
        family.resize(nz(8), nz(3));
        family.feed("👩‍👧‍👦".as_bytes());
        let row = family.visible_row(0).unwrap();
        assert_eq!(row.cells[1].text, "👨‍👩‍👧‍👦");
        assert!(row.cells[2].wide_spacer);
        assert_eq!(family.cursor().column, 3);

        let mut flag = TerminalAdapter::new(nz(4), nz(3));
        flag.feed(b"\x1b[?2027habc");
        flag.feed("🇺".as_bytes());
        flag.resize(nz(8), nz(3));
        flag.feed("🇸".as_bytes());
        let row = flag.visible_row(0).unwrap();
        assert_eq!(row.cells[3].text, "🇺🇸");
        assert!(row.cells[4].wide_spacer);
        assert_eq!(flag.cursor().column, 5);
    }

    #[test]
    fn clear_history_is_only_reported_by_the_vt_ed3_action() {
        for payload in [
            b"\x1b_payload [3J\x1b\\".as_slice(),
            b"\x1bP0;1|[3J\x1b\\".as_slice(),
        ] {
            let mut terminal = TerminalAdapter::new(nz(8), nz(2));
            assert!(!terminal.feed(payload).contains(&AdapterEvent::ClearHistory));
        }

        let mut terminal = TerminalAdapter::new(nz(8), nz(2));
        assert!(
            terminal
                .feed(b"\x1b[3J")
                .contains(&AdapterEvent::ClearHistory)
        );
    }

    #[test]
    fn unterminated_apc_and_dcs_can_resynchronize_on_escape() {
        for introducer in [b"\x1b_stuck".as_slice(), b"\x1bP0;1|stuck".as_slice()] {
            let mut terminal = TerminalAdapter::new(nz(8), nz(2));
            terminal.feed(introducer);
            terminal.feed(b"\x1b[2J\x1b[1;1Hhello");
            assert!(terminal.visible_text().iter().any(|line| line == "hello"));
        }
    }

    #[test]
    fn osc_1337_inline_image_is_emitted_once_across_feed_boundaries() {
        let mut terminal = TerminalAdapter::new(nz(40), nz(4));
        assert_eq!(
            terminal.feed(b"pre\x1b]1337;Fi"),
            vec![AdapterEvent::GridWrites {
                screen: RemovalScreen::Primary,
                rows: vec![0],
            }]
        );
        assert!(terminal.feed(b"le=inline=1;name=eA==:YW").is_empty());
        let events = terminal.feed(b"JjZA==\x1b\\post");
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, AdapterEvent::InlineImage { .. }))
                .collect::<Vec<_>>(),
            vec![&AdapterEvent::InlineImage {
                screen: RemovalScreen::Primary,
                row: 0,
                column: 3,
                // `[image]` fits whole at column 3 of a 40-column grid.
                placeholder_columns: 7,
                encoded: b"YWJjZA==".to_vec(),
            }]
        );
        assert!(events.iter().any(|event| matches!(
            event,
            AdapterEvent::GridWrites {
                screen: RemovalScreen::Primary,
                rows
            } if rows == &[0]
        )));
        assert_eq!(terminal.visible_text()[0], "pre[image]post");
    }

    #[test]
    fn osc_1337_inline_zero_is_ignored_without_a_placeholder() {
        let mut terminal = TerminalAdapter::new(nz(40), nz(4));
        let events = terminal.feed(b"left\x1b]1337;File=inline=0:YWJj\x07right");
        assert!(
            events
                .iter()
                .all(|event| matches!(event, AdapterEvent::GridWrites { .. }))
        );
        assert_eq!(terminal.visible_text()[0], "leftright");
    }

    #[test]
    fn grid_write_facts_exclude_cursor_only_crlf_and_cup_motion() {
        let mut terminal = TerminalAdapter::new(nz(40), nz(4));
        assert_eq!(
            terminal.feed(b"written"),
            vec![AdapterEvent::GridWrites {
                screen: RemovalScreen::Primary,
                rows: vec![0],
            }]
        );
        assert!(
            terminal
                .feed(b"\r\n\x1b[3;12H\r")
                .iter()
                .all(|event| !matches!(event, AdapterEvent::GridWrites { .. }))
        );
    }
}
