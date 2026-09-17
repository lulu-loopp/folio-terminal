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
        Config, Osc52, ScrollOutCause, ScrollRegionScope, TermDamage, TermMode, TranscriptEvent,
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

/// How many uncommitted bytes the replay tail keeps before it stops keeping them.
///
/// The same number as vte's own synchronized-update buffer (`SYNC_BUFFER_SIZE`, 2 MiB), and that
/// is where it comes from: the tail exists to hold what the vendored parser has not committed,
/// the parser force-ends an update whose buffer reaches that size, and so a tail past it is
/// holding bytes no replay can ever want. Reaching it is treated as the end of the update, which
/// is the same conclusion the parser behind it has already come to.
const PARSER_TAIL_MAX_BYTES: usize = 2 * 1024 * 1024;

/// This window's answer to XTVERSION: `DCS > | Folio(<version>) ST`, in the shape xterm defined and
/// every terminal that answers at all uses. The version is the one the product ships under and
/// nothing else — no commit, no build host, no operating system. A program asking this is asking
/// what it may speak, not who built it.
const XTVERSION_REPLY: &str = concat!("\x1bP>|Folio(", env!("CARGO_PKG_VERSION"), ")\x1b\\");

/// The buffer size `vte` 0.15 will not let a synchronized update reach — `SYNC_BUFFER_SIZE` in
/// `vte-0.15.0/src/ansi.rs`.
///
/// It is written down here because the rule built on it is arithmetic this side can do for itself:
/// `advance_sync` gives up on a block when what it is already holding plus *the whole slice it has
/// just been handed* would reach `SYNC_BUFFER_SIZE - 1`, and then commits the block and parses that
/// slice in the same call. Both terms of that sum are things the adapter knows —
/// [`TerminalAdapter::synchronized_update_pending_bytes`] is the vendored buffer's own length, and
/// the other is the length of the segment about to be handed over — so the byte the block ends on
/// can be found rather than waited for. See [`TerminalAdapter::advance_parsers_through_any_overflow`].
const VENDOR_SYNC_BUFFER_SIZE: usize = 0x20_0000;

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
    /// The raw bytes the vendor terminal has taken but not yet committed: the sequence that is
    /// still open at the end of a slice, and everything a synchronized update is holding back.
    /// A resize replays exactly this into the canonical fork's parser.
    ///
    /// Bounded by [`PARSER_TAIL_MAX_BYTES`], because what goes in it is chosen by the child.
    /// XTVERSION queries heard but not yet answered, because a DEC 2026 block is still buffering
    /// the bytes that carried them. See [`TerminalAdapter::answer_xtversion_if_not_buffering`].
    xtversion_replies_owed: usize,
    parser_tail: Vec<u8>,
    /// Where in [`Self::parser_tail`] the sequence that is still open begins — the tail's own
    /// length when nothing is open.
    ///
    /// It is what makes releasing a synchronized update's retention exact: the update's bytes go
    /// and the half-written sequence after them stays, instead of the whole tail being thrown
    /// away and a resize seeding its parser mid-escape.
    parser_tail_open_start: usize,
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
    /// How many times a resize transaction has deep-copied this terminal.
    ///
    /// A `Term` owns both grids, and inside a transaction the primary one owns
    /// the whole mutable resize tail, so a copy of it costs one allocation per
    /// row of history — on the window thread, for every shown pane, before any
    /// frame of the new size is drawn. Arming the canonical branch is the one
    /// copy that path is allowed to make, and this counter is what lets a test
    /// say it made one rather than that it looks like one. See
    /// [`Self::arm_resize_canonical`].
    resize_forks: u64,
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

/// A [`Handler`] that keeps nothing, for a replay whose only product is parser state.
///
/// [`TerminalAdapter::arm_resize_canonical`] replays the uncommitted parser tail to bring a fresh
/// [`Processor`] to the position the displayed parser already stands at. Every semantic action that
/// replay dispatches has been applied to the terminal once already, so every one of them has to be
/// dropped — which is exactly what the vendored trait's own defaults do with all of them.
struct ParserTailSink;

impl Handler for ParserTailSink {}

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
    /// **Somebody asked which terminal this is** — XTVERSION, `CSI > q` or `CSI > 0 q`.
    xtversion_queried: bool,
}

/// What one byte through the boundary parser leaves for the terminal processor's pace to decide:
/// everything else a byte changes is settled where it is seen. See
/// [`TerminalAdapter::advance_terminal_bytes`].
struct BoundaryByte {
    /// A bell to report, once the processor has reached the same byte — the child can order a bell
    /// against a title, and a title comes from the processor.
    bell: bool,
    /// An XTVERSION query ended on this byte, so the stream's place for its answer is here: after
    /// everything the processor answers before this byte, and before everything it answers after.
    xtversion_queried: bool,
    /// A DEC 2026 terminator ended on this byte. Only interesting when an answer is owed from
    /// inside the block it ends, which is the moment that answer becomes due.
    sync_ended: bool,
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

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        // The sequence ended either way — that is what closes the tail — but a sequence the real
        // parser threw away must not be read for meaning here.
        self.complete = true;
        // **The same refusal the vendored handler makes**, byte for byte
        // (`vte-0.15.0/src/ansi.rs`, the head of its own `csi_dispatch`): a parameter list past
        // vte's thirty-two, or more than two intermediates, and the sequence does nothing. Read
        // any further and a `CSI ? 2026;2026;…h` with thirty-three parameters would open a
        // synchronized update on this side that no ESU on the other side can ever close.
        if ignore || intermediates.len() > 2 {
            return;
        }
        let sync_mode = intermediates == b"?"
            && params
                .iter()
                .next()
                .is_some_and(|parameter| parameter == [2026]);
        self.sync_start = sync_mode && action == 'h';
        self.sync_end = sync_mode && action == 'l';
        // **XTVERSION, and only XTVERSION.** `CSI > q` and its explicit spelling `CSI > 0 q` are the
        // question "which terminal am I talking to"; every other parameter after `CSI >` is a
        // different question this window has no answer for, and `CSI Ps SP q` (DECSCUSR, the cursor
        // shape a shell sets on every prompt) is not this sequence at all — it has an intermediate
        // of its own and no `>`.
        self.xtversion_queried = action == 'q'
            && intermediates == b">"
            && params
                .iter()
                .next()
                .is_none_or(|parameter| parameter == [0] && params.len() == 1);
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
            // **This terminal does not act on OSC 52 in either direction**, so it says so here
            // rather than decoding a store and dropping the result. The vendored default is
            // `OnlyCopy`, which accepts a store, base64-decodes the whole payload into a `String`
            // and sends it as an event — and [`CaptureListener`] has no arm for that event, so
            // every byte of that work is spent on something nobody reads. Declaring the refusal
            // is also the honest statement: a program cannot put text on this reader's clipboard,
            // and cannot read it back either.
            osc52: Osc52::Disabled,
            ..Config::default()
        };
        let size = GridSize { columns, rows };
        let listener = CaptureListener::default();
        let mut term = Term::new(config, &size, listener.clone());
        // Fail closed from the first byte: until a session says otherwise, nothing written here is
        // a command's output. The vendored default is the empty one, so that a caller which never
        // speaks — upstream's own test suite — writes exactly the cells upstream writes.
        term.set_write_provenance(false, false);
        install_transcript_hook(&mut term, &listener);
        let row_fingerprint_seed = RandomState::new().build_hasher().finish();
        Self {
            term,
            processor: Processor::new(),
            listener,
            parser_boundary: Parser::new(),
            xtversion_replies_owed: 0,
            parser_tail: Vec::new(),
            parser_tail_open_start: 0,
            parser_sync_active: false,
            parser_dcs_active: false,
            parser_sequence_open: false,
            cursor_row_positioned_explicitly: false,
            osc1337_scanner: Osc1337Scanner::default(),
            pending_stream: VecDeque::new(),
            resize_canonical: None,
            resize_forks: 0,
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
                    // **An image names a cell too, and for the same reason it has to be read
                    // against a grid the bytes before it have reached.** Everything the block was
                    // holding — the `CUP` that put the cursor where the image belongs, the swap
                    // that changed which screen it belongs to — is parsed first; without it an
                    // image drawn inside a synchronized update was filed at the cursor from before
                    // the block, on the screen from before the block, and its placeholder was
                    // measured against a column it was not written at. The placeholder itself was
                    // never misplaced: it goes through the same parser, so it landed correctly and
                    // the record pointed somewhere else.
                    events.extend(self.commit_synchronized_update_before_marker());
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
                    // **A marker is a statement about the grid, so the grid has to have caught
                    // up with the bytes before it.** DEC 2026 is the one thing that puts writes
                    // out of order with this stream: the vendored parser holds a block's bytes
                    // and applies them all at the terminator, while this scanner has already
                    // handed their markers out. So a `B` read the cursor from before the prompt
                    // the block was drawing, a marker after a buffered `?1049l` named the screen
                    // the block had already left, and the provenance the session states for the
                    // next segment was stamped on text that arrived under the last one — a
                    // prompt's `$…$` typeset as output, and a command's left as source when it
                    // printed inside a block that ended after the prompt came back.
                    //
                    // Committing the block here is the commit its own deadline already makes,
                    // taken at the one other point where the order is load-bearing. **Every
                    // recognised marker ends the block, `A` and `B` included**: what precedes the
                    // first marker in it is committed whole and what follows draws
                    // unsynchronised, so a producer that puts markers inside a block can be seen
                    // drawing in pieces. None of the integrations this terminal ships, and none
                    // of the prompt painters read for the 2026-09-17 review, does that — their
                    // markers arrive with no block open, where this costs one stored `Option`
                    // read. What it buys is that a marker never describes a grid the parser has
                    // not reached.
                    events.extend(self.commit_synchronized_update_before_marker());
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

    /// Write out a DEC 2026 block that is still holding bytes back, so that the fact about to be
    /// reported — a shell marker, an inline image — is read against a grid those bytes have
    /// reached.
    ///
    /// The same commit [`Self::finish_synchronized_update`] makes when the block's own deadline
    /// passes, and it reports the same events — with this one's grid writes as well, because the
    /// session records which rows a command line was typed on and those rows are written here.
    /// Silent, and free, when no block is open: one stored `Option` read and an empty vector.
    fn commit_synchronized_update_before_marker(&mut self) -> Vec<AdapterEvent> {
        if self.synchronized_update_deadline().is_none() {
            return Vec::new();
        }
        let mut events = self.finish_synchronized_update();
        events.extend(self.drain_grid_write_events());
        events
    }

    /// **Parse what the child sent, and answer each question where it stands in the stream.**
    ///
    /// DA1, DSR and DECRQM are answered by the terminal processor as it reaches them, into the same
    /// queue the XTVERSION reply goes into; XTVERSION is answered from the boundary parser instead,
    /// for the reasons in [`Self::answer_xtversion_if_not_buffering`]. The two have to meet, because
    /// the order the answers leave in is itself an answer. A program writes XTVERSION and then DA1
    /// as a sentinel — DA1 is answered by every terminal ever made — reads until the sentinel, and
    /// concludes from "DA1 came first" that this terminal does not answer XTVERSION at all. Answered
    /// in the wrong order is not answered.
    ///
    /// So the feed is cut at the end of each completed XTVERSION query, the processor is advanced
    /// segment by segment, and that query's answer is pushed between the segments. The queue is then
    /// the stream's own order by construction — in both directions, for any interleaving, and at
    /// every pty read boundary — rather than one kind of answer being moved to the front or held to
    /// the back.
    ///
    /// **A query inside a DEC 2026 block is answered at that block's commit**, and the ESU that
    /// commits it is cut at for the same reason and in the same way. Waiting instead for a moment
    /// when no block happens to be buffering is not the same rule and is not enough: a program
    /// repainting in synchronized frames opens the next block in the write that closed the last, so
    /// the answer was held for an unrelated later frame, and anything the child asked *after* the
    /// block — a DA1 sentinel above all — was answered ahead of it. The cut is made only when an
    /// answer is actually owed, so a block in a feed that asked nothing costs nothing; and whether
    /// the answer may leave is still decided by the processor's own deadline afterwards, never by
    /// the boundary parser's reading of the terminator, so a terminator this side recognises and
    /// the vendored parser does not simply cuts a segment and changes nothing.
    ///
    /// **A feed that carries no query and owes no answer is one segment**, which is every feed in
    /// ordinary use: the processor sees the whole slice in a single `advance`, the boundary parser
    /// makes the per-byte pass it has always made, and this adds no allocation of its own. A feed
    /// asking nothing while an earlier answer is still owed by an open block may be cut — at that
    /// block's ending, which is the point.
    ///
    /// Finding where a segment ends means the boundary parser runs ahead of the processor over that
    /// segment, which is safe because nothing it does per byte reads the processor or the grid: it
    /// keeps its own parse state, the retained tail, and adapter-side facts — `announced_focus`,
    /// `cursor_row_positioned_explicitly` — that only it writes. The two things that do cross over
    /// are handled rather than assumed. A bell is an event the child can order against a title the
    /// processor reports, so it is counted here and pushed once the processor has reached the same
    /// byte, which is exactly where it was pushed before. And every question that is put to the
    /// processor — is a synchronized update still open, and may a reply leave yet — is asked after
    /// the advance it depends on, never before it.
    fn advance_terminal_bytes(&mut self, bytes: &[u8]) {
        let mut segment_start = 0;
        let mut bells = 0usize;
        for (index, &byte) in bytes.iter().enumerate() {
            let step = self.observe_parser_boundary_byte(byte);
            bells += usize::from(step.bell);
            // A completed query is where its own answer belongs. A completed ESU, when an answer is
            // already owed from inside the block it ends, is where *that* answer belongs — the same
            // cut, made for the same reason.
            let commits_a_debt = step.sync_ended && self.xtversion_replies_owed > 0;
            if step.xtversion_queried || commits_a_debt {
                self.advance_parsers_through_any_overflow(
                    &bytes[segment_start..=index],
                    &mut bells,
                );
                segment_start = index + 1;
                if step.xtversion_queried {
                    self.xtversion_replies_owed = self.xtversion_replies_owed.saturating_add(1);
                }
                self.answer_xtversion_if_not_buffering();
            }
        }
        self.advance_parsers_through_any_overflow(&bytes[segment_start..], &mut bells);
        self.settle_parser_boundary();
    }

    /// Advance over one segment, cutting it at the byte a synchronized update the answer is owed by
    /// will end on, when the vendored parser is about to give that block up.
    ///
    /// The other way a block ends — its ESU — is a sequence, so the boundary parser finds it and
    /// [`Self::advance_terminal_bytes`] cuts there. This ending is not a sequence but a rule about
    /// size: `vte` gives up on a block when what it holds plus the slice it is handed would reach
    /// [`VENDOR_SYNC_BUFFER_SIZE`] `- 1`, and it then commits the block *and parses the rest of that
    /// slice* in the one call — so if the slice went over whole, the block's answer was left behind
    /// every reply the rest of the slice asked for, and behind a new block if the rest opened one,
    /// which put it back where this feed cut started: owed, with no block of its own left to end.
    ///
    /// The rule is arithmetic and both of its terms are visible from here, so the block's last byte
    /// is found instead. The segment goes over in three pieces: the largest prefix that does *not*
    /// reach the rule, then the single byte that does — which commits the block with a one-byte
    /// remainder — and then the rest, after the answer has left. Where those cuts fall inside a
    /// sequence does not matter to either parser: both are byte-stream state machines that hold a
    /// half-read sequence across calls, which is the same thing that makes a pty read boundary
    /// harmless.
    ///
    /// **The commit is observed, not assumed.** The vendored buffer is empty afterwards exactly
    /// when the block was given up on, so that is what the answer waits for; if the vendored rule
    /// ever stops working out this way the segment simply goes over as one piece, which is what it
    /// did before. And a reply produced by the one tripping byte comes out *ahead* of the answer:
    /// such a byte can only complete a sequence that began among the bytes the block was holding —
    /// a DA1 whose `CSI` was inside the frame — which makes it one of the block's own replies, on
    /// the block's side of the limit, exactly where the ordinary commit puts it.
    ///
    /// **Nothing here happens unless an answer is owed by a block that is still buffering.** A feed
    /// that owes nothing costs one comparison against zero.
    fn advance_parsers_through_any_overflow(&mut self, segment: &[u8], bells: &mut usize) {
        if self.xtversion_replies_owed == 0 || self.synchronized_update_deadline().is_none() {
            self.advance_parsers(segment, bells);
            return;
        }
        // How many more bytes this block can be handed at once before the vendored parser gives up
        // on it. `vte`'s own invariant keeps the buffer below the size it gives up at, so there is
        // always at least one; a zero would mean that no longer holds, and the segment goes over
        // whole rather than on arithmetic that has stopped describing anything.
        let trips_at =
            (VENDOR_SYNC_BUFFER_SIZE - 1).saturating_sub(self.synchronized_update_pending_bytes());
        if trips_at == 0 || segment.len() < trips_at {
            self.advance_parsers(segment, bells);
            return;
        }
        let held = trips_at - 1;
        self.advance_parsers(&segment[..held], bells);
        self.advance_parsers(&segment[held..=held], bells);
        if self.synchronized_update_pending_bytes() == 0 {
            self.push_xtversion_replies();
        }
        self.advance_parsers(&segment[held + 1..], bells);
    }

    /// Advance both terminal parsers over one segment of a feed, then report the bells the boundary
    /// parser found in it. The canonical fork a resize transaction keeps sees the same bytes in the
    /// same order as the displayed branch, segmented or not, and its replies are thrown away either
    /// way. See [`Self::advance_terminal_bytes`] for why the bells wait for this call.
    fn advance_parsers(&mut self, segment: &[u8], bells: &mut usize) {
        self.processor.advance(&mut self.term, segment);
        if let Some(canonical) = self.resize_canonical.as_mut() {
            canonical.processor.advance(&mut canonical.term, segment);
            discard_listener_output(&canonical.listener);
            let _ = canonical.term.take_input_writes();
        }
        if *bells > 0 {
            let mut events = self
                .listener
                .adapter_events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for _ in 0..std::mem::take(bells) {
                events.push(AdapterEvent::Bell);
            }
        }
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

    /// How many bytes an open DEC 2026 block is holding back — the block's own
    /// output, waiting for the ESU or the timeout that writes it to the grid.
    ///
    /// It is the block's answer to the question `feed` answers with
    /// `!bytes.is_empty()`: these are the bytes that will reach the screen when
    /// the block commits, and a block holding none of them cannot change a cell.
    /// Zero while no block is open.
    pub fn synchronized_update_pending_bytes(&self) -> usize {
        self.processor.sync_bytes_count()
    }

    /// Commit a synchronized update whose ESU terminator did not arrive before its deadline.
    pub fn finish_synchronized_update(&mut self) -> Vec<AdapterEvent> {
        if self.synchronized_update_deadline().is_none() {
            // The vendored parser has already ended the update, either because the ESU arrived or
            // because its own buffer overflowed and it gave up. Either way this side stops
            // retaining bytes for it.
            self.release_synchronized_update_retention();
            self.answer_xtversion_if_not_buffering();
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
        self.parser_tail.shrink_to_fit();
        self.parser_tail_open_start = 0;
        // The block's bytes are on the grid now, so a question they carried is answered now.
        self.answer_xtversion_if_not_buffering();
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
    ///
    /// **One clone of the terminal, and it is the branch itself.** The fork is not a picture taken
    /// to be read and dropped: [`Self::reconcile_resize_transaction_to_viewport`] installs it as
    /// the displayed terminal, so it has to be a whole terminal — both grids, and during a
    /// transaction the primary one owns the entire mutable resize tail. That is the one copy this
    /// path is allowed, and [`Self::resize_forks`] counts it so a test can say so.
    fn arm_resize_canonical(&mut self) {
        let listener = CaptureListener::default();
        let mut term = self.term.fork(listener.clone());
        install_transcript_hook(&mut term, &listener);
        self.resize_forks = self.resize_forks.saturating_add(1);

        // A transaction can begin between two bytes of a CSI/OSC/DCS/UTF-8 sequence or while a
        // synchronized update is buffered. Seed a fresh processor with that exact uncommitted raw
        // tail; the canonical term already contains every committed semantic action and must not
        // receive the tail twice, which is why the replay goes to a handler that keeps nothing.
        //
        // What is being carried across is the parser's own position, and that is independent of
        // who is handling it: every `Handler` method returns `()` and the `Processor` reads none
        // of them back, so a `ParserTailSink` leaves it where a real terminal would. That sink
        // used to be a second `fork`, which made opening a transaction two deep copies of the whole
        // resize tail — and then dropped one of them unread.
        let mut processor = Processor::new();
        processor.advance(&mut ParserTailSink, &self.parser_tail);

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

    /// The same row read off the **primary** screen, whichever screen is showing.
    ///
    /// A full-screen program parks the primary grid; it does not freeze it. The vendor reflows the
    /// inactive grid on every resize, so a window edge dragged while `vim` is up re-cuts every
    /// logical line behind it, and an owner of coordinates on that screen has to be able to read
    /// the rows at the one moment they are not on display.
    ///
    /// Uncached, unlike [`Self::visible_row`]: the capture cache is a per-frame instrument keyed to
    /// the grid on screen, and this is asked on a resize rather than on a frame.
    pub fn primary_row(&self, row: u32) -> Option<CapturedRow> {
        if !self.term.mode().contains(TermMode::ALT_SCREEN) {
            return self.visible_row(row);
        }
        if row >= self.rows.get() {
            return None;
        }
        let grid = self.term.primary_grid();
        let cells = (0..self.columns.get())
            .map(|column| grid[Line(row as i32)][Column(column as usize)].clone())
            .collect::<Vec<_>>();
        Some(to_captured_row(&cells))
    }

    /// How many rows have been captured out of this terminal since it opened.
    /// See [`Self::captures`].
    pub fn captures(&self) -> u64 {
        self.captures.get()
    }

    /// How many times a resize transaction has deep-copied this terminal since it opened.
    /// See [`Self::resize_forks`].
    pub fn resize_forks(&self) -> u64 {
        self.resize_forks
    }

    /// Whether `row` soft-wraps into the row below it — the `continues` flag of `visible_row`, read
    /// without capturing the row.
    ///
    /// The affordance scan asks this once per presentation row of every published frame, only to
    /// decide where one logical line ends, so capturing (and cloning) a whole row of cells for one
    /// bit is the wrong price. The bit itself is where the capture reads it: WRAPLINE on the row's
    /// last cell.
    /// Say whether the bytes fed from here on are a shell command's output, **for each screen**.
    ///
    /// Stamped by the terminal onto every cell it prints, and read back off the captured cells as
    /// [`bt_transcript::CapturedCell::command_output_write`]. The session states it before every
    /// segment: the adapter pauses the stream at each shell-integration marker, so a segment
    /// carries no change of phase inside it. What a segment *can* carry inside it is a screen
    /// swap, which is a change of answer with no marker to restate it — so both screens' answers
    /// are stated here and the terminal takes up the one belonging to the screen that is showing.
    ///
    /// The resize transaction's canonical branch is told as well: it parses the same bytes into its
    /// own grid, and rows harvested from it become transcript lines like any other.
    pub fn set_write_provenance(
        &mut self,
        primary_is_command_output: bool,
        alternate_is_command_output: bool,
    ) {
        self.term
            .set_write_provenance(primary_is_command_output, alternate_is_command_output);
        if let Some(canonical) = self.resize_canonical.as_mut() {
            canonical
                .term
                .set_write_provenance(primary_is_command_output, alternate_is_command_output);
        }
    }

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

    /// Put one byte of the feed through the boundary parser and fold what it said into this side's
    /// state, reporting back only the two things the caller has to act on at the processor's pace.
    ///
    /// Everything else this touches is the boundary parser's own — its parse state, the retained
    /// tail, and the adapter-side facts nothing downstream writes — so it is correct here whether
    /// the processor has reached this byte yet or not. See [`Self::advance_terminal_bytes`].
    fn observe_parser_boundary_byte(&mut self, byte: u8) -> BoundaryByte {
        let execute_at_ground = !self.parser_sequence_open;
        let sequence_was_open = self.parser_sequence_open;
        if self.parser_tail.len() < PARSER_TAIL_MAX_BYTES {
            self.parser_tail.push(byte);
        } else {
            // See [`PARSER_TAIL_MAX_BYTES`]: nothing is still legitimately uncommitted after
            // this much, so the update the tail was being kept for is treated as ended. The
            // next completed sequence clears the tail on the ordinary path below.
            self.parser_sync_active = false;
        }
        let mut performer = BoundaryPerformer {
            execute_at_ground,
            ..BoundaryPerformer::default()
        };
        self.parser_boundary
            .advance(&mut performer, std::slice::from_ref(&byte));
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

        let mut tail_cleared = false;
        if performer.sync_start {
            self.parser_sync_active = true;
        } else if performer.sync_end {
            self.parser_sync_active = false;
            self.parser_tail.clear();
            tail_cleared = true;
        } else if performer.complete && !self.parser_sync_active {
            self.parser_dcs_active = false;
            self.parser_tail.clear();
            tail_cleared = true;
            // ESC can terminate OSC/DCS while simultaneously starting the ST escape. Keep it
            // as the seed for the parser's new Escape state.
            if byte == 0x1b {
                self.parser_tail.push(byte);
            }
        }

        if tail_cleared {
            // Whatever is left is the sequence this byte opened, and it starts at the front.
            self.parser_tail_open_start = 0;
        } else if !self.parser_sequence_open {
            self.parser_tail_open_start = self.parser_tail.len();
        } else if !sequence_was_open {
            self.parser_tail_open_start = self.parser_tail.len().saturating_sub(1);
        }
        self.parser_tail_open_start = self.parser_tail_open_start.min(self.parser_tail.len());

        BoundaryByte {
            bell: performer.bell,
            xtversion_queried: performer.xtversion_queried,
            sync_ended: performer.sync_end,
        }
    }

    /// Everything the boundary pass owes once the processor has been advanced over the whole feed —
    /// both of these read the processor, which is why they are here and not in the per-byte step.
    fn settle_parser_boundary(&mut self) {
        // **The vendored parser owns whether a synchronized update is open.** It force-ends one
        // whose buffer overflows and clears its deadline in the same breath
        // (`vte-0.15.0/src/ansi.rs` `advance_sync`), which leaves this side armed over a parser
        // that has already put everything it was holding on the grid — and then every later byte
        // is retained for a replay that will never happen, and every resize replays the lot. So
        // the flag follows the parser rather than only the bytes.
        if self.parser_sync_active && self.processor.sync_timeout().sync_timeout().is_none() {
            self.release_synchronized_update_retention();
        }
        self.answer_xtversion_if_not_buffering();
    }

    /// **What this window answers to XTVERSION** — `DCS > | Folio(<version>) ST`, and nothing else.
    ///
    /// Why here and not in the vendored handler: `vte` 0.15 has no arm for `q` with a `>`
    /// intermediate (`ansi.rs` dispatches `('q', [b' '])` for DECSCUSR and falls through to its
    /// `unhandled!` log for everything else), so the sequence never reaches `Term`, and `vte` is a
    /// registry dependency rather than one of this repository's vendored crates. The boundary
    /// parser is the right place regardless of that: it is a real `Perform`, so the question is
    /// *parsed* — a query split across two pty reads is held by the parser's own state, exactly like
    /// every other sequence, and no run of bytes is matched as a substring, so the text a program
    /// prints or pastes is never mistaken for the question. (That is a claim about matching and not
    /// about what may appear inside a string: an ESC ends an OSC, DCS or APC payload in `vte`, so a
    /// query written after one inside the same read is a query, and is answered. It is the parser's
    /// answer, not a guess about quoting.) It runs once per byte on the real stream only, never on
    /// the resize canonical fork whose replies `discard_listener_output` throws away. The reply
    /// joins the same queue DA1 and DSR use, so it is drained in order with them and inherits the
    /// same "no PTY writer, no reply" behaviour from the caller.
    /// [`TerminalAdapter::advance_terminal_bytes`] is what makes that order the stream's own: it
    /// cuts the feed at this query so the answer is pushed where the question stood.
    ///
    /// The string is fixed. Nothing from the query is echoed back, and a flood of queries costs one
    /// short reply each, which is what DA1 costs.
    ///
    /// **The limit, stated, and it is the width of one block.** A query inside a DEC 2026 block is
    /// answered at that block's commit — at its ESU, at its deadline, or at the byte the vendored
    /// parser gives up on it at, whichever of the three the block ends by. So **nothing outside the
    /// block is ever overtaken**: everything asked before the block is answered before it, and
    /// everything asked after the block is answered after it. What is not the stream's order is the
    /// inside: the vendored processor buffers the block's bytes and replays them all at once, so
    /// this side cannot stand between two of them, and the block's own replies come out of that
    /// replay ahead of the XTVERSION answer however the two were interleaved within the block.
    /// Getting *that* right would mean cutting the replay the way the feed is cut here, which is
    /// inside `vte` — a registry dependency, not one of this repository's vendored crates. A
    /// question the block asked and the byte it ended on finished — a DA1 whose `CSI` was inside the
    /// frame — is one of the block's own, and is answered with them, ahead of the answer the block
    /// owed. What holds in every case is what a child can act on: exactly one answer per question,
    /// and never from inside a frame that is not on the screen yet.
    ///
    /// **A reset does not un-ask a question.** A RIS that arrives while a block is still buffering
    /// clears the screen and the modes, and the reply owed from inside that block still goes out at
    /// the commit: xterm answers what it parsed, the child is blocked on an answer it asked for
    /// before the reset, and dropping it would hang a probe over a screen clear.
    ///
    /// **The name is this product's own.** A program that allow-lists terminal names will simply not
    /// match it, which is the honest outcome; pretending to be another terminal would claim its
    /// bugs and its capabilities alike.
    fn answer_xtversion_if_not_buffering(&mut self) {
        // A query inside a DEC 2026 block is answered when that block's bytes are parsed, the same
        // moment the grid they describe appears, so the child never hears from inside a frame that
        // is not on the screen yet.
        if self.synchronized_update_deadline().is_some() {
            return;
        }
        self.push_xtversion_replies();
    }

    /// Push every answer still owed. The one caller that does not go through
    /// [`Self::answer_xtversion_if_not_buffering`] is
    /// [`Self::advance_parsers_through_any_overflow`], which has just watched the block that owed
    /// them commit and must not ask again whether a block is open — by then the remainder of the
    /// same slice may have opened the next one, which is the bug this whole path exists to close.
    fn push_xtversion_replies(&mut self) {
        if self.xtversion_replies_owed == 0 {
            return;
        }
        let mut writes = self
            .listener
            .pty_writes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for _ in 0..std::mem::take(&mut self.xtversion_replies_owed) {
            writes.push(PendingReply::Bytes(XTVERSION_REPLY.as_bytes().to_vec()));
        }
    }

    /// Stop retaining bytes for a synchronized update that is over, keeping the sequence that is
    /// still open at the end of the tail.
    fn release_synchronized_update_retention(&mut self) {
        self.parser_sync_active = false;
        let open = self.parser_tail_open_start.min(self.parser_tail.len());
        self.parser_tail.drain(..open);
        self.parser_tail_open_start = 0;
        self.parser_tail.shrink_to_fit();
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
    use crate::inline_image::MAX_UNOWNED_OSC_BYTES;

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
        const PROMPT: &str = "(base) PS D:\\Developer\\folio-terminal> ";
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

    /// A resize transaction copies this terminal once per point where it arms the canonical
    /// branch, and the length of the history it is holding does not change that number.
    ///
    /// Arming used to `fork` twice: once for the branch a commit installs, and once for a
    /// throwaway terminal that existed only to give the replayed parser tail somebody to talk to.
    /// A fork is `Term::clone`, which copies both grids a row at a time, and inside a transaction
    /// the primary grid owns the whole mutable resize tail — so the second copy was the price of
    /// the history, paid on the window thread, for every shown pane, before a single frame of the
    /// new size reached the screen. It is gone. The first one stays, and stays whole, because
    /// what it arms is not a picture of the terminal but the terminal the commit installs.
    ///
    /// The history has to be built inside the transaction because that is the only place this
    /// terminal has any: steady-state vendor scrollback is [`SCROLLBACK_LINES`], zero, and the
    /// transcript's frozen lines are not the grid's and were never in the copy.
    ///
    /// A count and not a clock. The claim is how many copies were made, and a stopwatch could only
    /// ever guess at that from how long they took.
    #[test]
    fn a_resize_transaction_copies_the_terminal_once_however_long_its_history() {
        const HISTORY_ROWS: usize = 50_000;
        const CHUNK_ROWS: usize = 500;

        let mut terminal = TerminalAdapter::new(nz(20), nz(4));
        terminal.begin_resize_transaction();
        assert_eq!(
            terminal.resize_forks(),
            1,
            "opening the transaction armed the canonical branch once"
        );

        let mut row = 0;
        while row < HISTORY_ROWS {
            let mut chunk = Vec::new();
            for _ in 0..CHUNK_ROWS {
                chunk.extend_from_slice(format!("line {row}\r\n").as_bytes());
                row += 1;
            }
            terminal.feed(&chunk);
        }
        // Everything fed but the screenful still on screen has scrolled into native history.
        assert!(
            terminal.resize_transaction_history_size() >= HISTORY_ROWS - 4,
            "the next fork is standing on {} rows of native history",
            terminal.resize_transaction_history_size()
        );

        // Arm the next branch mid-sequence as well, so the replay of the uncommitted tail — the
        // work the second fork used to carry — happens with all of that history in the grid.
        terminal.feed(b"\x1b[?2026h\x1b[93mheld");
        assert!(!terminal.parser_tail.is_empty());
        terminal.resize(nz(20), nz(2));
        terminal.reconcile_resize_transaction_to_viewport();

        assert_eq!(
            terminal.resize_forks(),
            2,
            "the commit installed the branch and armed one more; neither point copied twice"
        );
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

    /// **What a program asking "which terminal is this?" hears back.**
    ///
    /// XTVERSION is `CSI > q`, and `CSI > 0 q` is the same question spelled with its default
    /// parameter. Both are answered with this window's own name and shipping version, and nothing
    /// else; a terminal that answers nothing at all is indistinguishable from one that cannot do
    /// anything, which is how the flicker of 2026-09-17 came about (Claude Code asks this first,
    /// and only asks about synchronised output at all when something answered).
    #[test]
    fn xtversion_is_answered_with_this_terminals_own_name() {
        for query in [b"\x1b[>q".as_slice(), b"\x1b[>0q"] {
            let mut terminal = TerminalAdapter::new(nz(20), nz(3));
            terminal.feed(query);
            assert_eq!(
                terminal.take_pty_writes(),
                vec![XTVERSION_REPLY.as_bytes().to_vec()],
                "{query:?} went unanswered"
            );
            assert!(
                terminal.take_pty_writes().is_empty(),
                "{query:?} was answered twice"
            );
        }
        assert_eq!(
            XTVERSION_REPLY,
            format!("\x1bP>|Folio({})\x1b\\", env!("CARGO_PKG_VERSION")),
            "the reply is the product's own name and its shipping version, and carries nothing else"
        );
    }

    /// The question is parsed, not matched: the parser holds its own state between reads, so a
    /// query the operating system splits — macOS caps a pty read at 1 KiB and a query can land on
    /// that boundary like anything else — is still one question with one answer.
    #[test]
    fn a_split_xtversion_query_is_still_one_question() {
        let query = b"\x1b[>0q";
        for split in 1..query.len() {
            let mut terminal = TerminalAdapter::new(nz(20), nz(3));
            terminal.feed(&query[..split]);
            assert!(
                terminal.take_pty_writes().is_empty(),
                "half a query at {split} was answered"
            );
            terminal.feed(&query[split..]);
            assert_eq!(
                terminal.take_pty_writes(),
                vec![XTVERSION_REPLY.as_bytes().to_vec()],
                "a query split at {split} went unanswered"
            );
        }
    }

    /// Everything else after `CSI >` is a different question, and DECSCUSR — which a shell sets on
    /// every prompt — only looks like this one if you are matching bytes instead of parsing them.
    #[test]
    fn only_xtversion_is_answered() {
        for quiet in [
            b"\x1b[>1q".as_slice(),
            b"\x1b[>2q",
            b"\x1b[>0;1q",
            // DECSCUSR: `CSI Ps SP q`, a cursor shape, with its own intermediate and no `>`.
            b"\x1b[2 q",
            b"\x1b[0 q",
            b"\x1b[q",
        ] {
            let mut terminal = TerminalAdapter::new(nz(20), nz(3));
            terminal.feed(quiet);
            assert!(
                terminal.take_pty_writes().is_empty(),
                "{quiet:?} is not XTVERSION and must not be answered"
            );
        }
    }

    /// A question asked inside a synchronised update is answered when that update's bytes reach the
    /// grid, not while they are still being held back — the child never hears from inside a frame
    /// that is not on the screen yet. One answer, at the commit.
    #[test]
    fn a_query_inside_a_synchronized_update_is_answered_at_its_commit() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(3));
        terminal.feed(b"\x1b[?2026h\x1b[>0q");
        assert!(
            terminal.take_pty_writes().is_empty(),
            "the update is still buffering, so its bytes have not been read out yet"
        );
        terminal.feed(b"\x1b[?2026l");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![XTVERSION_REPLY.as_bytes().to_vec()]
        );
    }

    /// A resize transaction runs a second, canonical parser over the same bytes so the reflow can be
    /// measured; its replies are thrown away. The question must be answered once, by the stream the
    /// child is actually talking to.
    #[test]
    fn a_query_during_a_resize_transaction_is_answered_once() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(3));
        terminal.begin_resize_transaction();
        terminal.feed(b"\x1b[>0q");
        let replies = terminal.take_pty_writes();
        terminal.resize(nz(16), nz(3));
        let _ = terminal.finish_resize_transaction();
        assert_eq!(replies, vec![XTVERSION_REPLY.as_bytes().to_vec()]);
        assert!(terminal.take_pty_writes().is_empty());
    }

    /// The alternate screen is where the programs that ask this question live.
    #[test]
    fn xtversion_is_answered_on_the_alternate_screen() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(3));
        terminal.feed(b"\x1b[?1049h");
        let _ = terminal.take_pty_writes();
        terminal.feed(b"\x1b[>0q");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![XTVERSION_REPLY.as_bytes().to_vec()]
        );
    }

    /// **The whole point of the answer, end to end.**
    ///
    /// This is Claude Code's capability probe, in its order (verified against 2.1.274): it asks
    /// XTVERSION, and only if something answered does it go on to ask whether this terminal does
    /// synchronised output. Folio has always answered the second question correctly — `2` is
    /// DECRPM's "reset", which means "the mode exists and is currently off" and is one of the three
    /// statuses the probe accepts — but the first went unanswered, so the second was never asked and
    /// full-screen programs fell back to repainting without a synchronised update. Every formula
    /// that flashed back to LaTeX while the owner scrolled Claude Code on macOS came from that
    /// silence.
    #[test]
    fn the_capability_probe_gets_both_answers_in_order() {
        let mut terminal = TerminalAdapter::new(nz(40), nz(6));
        terminal.feed(b"\x1b[>0q");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![XTVERSION_REPLY.as_bytes().to_vec()]
        );
        terminal.feed(b"\x1b[?2026$p");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![b"\x1b[?2026;2$y".to_vec()],
            "the mode must be reported as a mode this terminal has"
        );
    }

    /// **Answers leave in the order their questions were asked**, and the probe that matters most
    /// depends on it. A program writes XTVERSION and DA1 in one breath and uses DA1 as a sentinel:
    /// DA1 is answered by every terminal ever made, so hearing it back *before* an XTVERSION reply
    /// is how the program concludes that this terminal does not answer XTVERSION at all and stops
    /// waiting. Answering both but in the wrong order is therefore the same as not answering.
    #[test]
    fn a_question_asked_before_da1_is_answered_before_da1() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(3));
        terminal.feed(b"\x1b[>q\x1b[c");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![XTVERSION_REPLY.as_bytes().to_vec(), b"\x1b[?6c".to_vec()],
            "the sentinel must not overtake the question it was written to follow"
        );
    }

    /// The same rule read from the other end: a question asked after DA1 is answered after it. This
    /// is the arm that a fix which simply put every XTVERSION reply first would break.
    #[test]
    fn a_question_asked_after_da1_is_answered_after_da1() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(3));
        terminal.feed(b"\x1b[c\x1b[>q");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![b"\x1b[?6c".to_vec(), XTVERSION_REPLY.as_bytes().to_vec()],
        );
    }

    /// Four questions of three kinds, interleaved in one feed: the queue is the stream's own order,
    /// not one kind of answer batched behind another.
    #[test]
    fn four_questions_in_one_feed_are_answered_in_the_order_they_were_asked() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(3));
        terminal.feed(b"\x1b[>q\x1b[6n\x1b[>q\x1b[c");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![
                XTVERSION_REPLY.as_bytes().to_vec(),
                b"\x1b[1;1R".to_vec(),
                XTVERSION_REPLY.as_bytes().to_vec(),
                b"\x1b[?6c".to_vec(),
            ]
        );
    }

    /// Where a pty read happens to end is not a fact about the stream, so it cannot be a fact about
    /// the answers: every one of these streams, cut at every byte position and delivered as two
    /// feeds, gives back the same replies in the same order as the whole.
    #[test]
    fn cutting_a_stream_at_any_byte_changes_neither_the_answers_nor_their_order() {
        for (stream, expected) in [
            (
                b"\x1b[>q\x1b[c".as_slice(),
                vec![XTVERSION_REPLY.as_bytes().to_vec(), b"\x1b[?6c".to_vec()],
            ),
            (
                b"\x1b[c\x1b[>q",
                vec![b"\x1b[?6c".to_vec(), XTVERSION_REPLY.as_bytes().to_vec()],
            ),
            (
                b"\x1b[>q\x1b[6n\x1b[>q\x1b[c",
                vec![
                    XTVERSION_REPLY.as_bytes().to_vec(),
                    b"\x1b[1;1R".to_vec(),
                    XTVERSION_REPLY.as_bytes().to_vec(),
                    b"\x1b[?6c".to_vec(),
                ],
            ),
        ] {
            for split in 0..=stream.len() {
                let mut terminal = TerminalAdapter::new(nz(20), nz(3));
                terminal.feed(&stream[..split]);
                let mut replies = terminal.take_pty_writes();
                terminal.feed(&stream[split..]);
                replies.extend(terminal.take_pty_writes());
                assert_eq!(
                    replies, expected,
                    "{stream:?} cut at {split} was answered differently"
                );
            }
        }
    }

    /// **The probe as a real program writes it**, which is one `write` carrying both questions
    /// rather than the two turns [`the_capability_probe_gets_both_answers_in_order`] takes. The
    /// program reads until it sees DA1; everything it received before DA1 is what it believes about
    /// this terminal, so the XTVERSION reply has to be in there. Only then does it ask the second
    /// question.
    #[test]
    fn the_capability_probe_written_in_one_breath_gets_its_answers_in_order() {
        let mut terminal = TerminalAdapter::new(nz(40), nz(6));
        terminal.feed(b"\x1b[>q\x1b[c");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![XTVERSION_REPLY.as_bytes().to_vec(), b"\x1b[?6c".to_vec()]
        );
        terminal.feed(b"\x1b[?2026$p");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![b"\x1b[?2026;2$y".to_vec()],
            "the mode must be reported as a mode this terminal has"
        );
    }

    /// **The one place the queue is not the stream's order, and it is a limit rather than a
    /// choice.** Inside a DEC 2026 block the vendored parser holds the bytes and replays them all at
    /// the commit, so this side cannot stand between two of them: the block's own replies are
    /// produced by that replay and the XTVERSION reply is appended after it. The guarantee that
    /// survives is the one the child can act on — exactly one answer, and not before the frame those
    /// bytes describe is on the screen.
    #[test]
    fn a_query_inside_a_synchronized_update_is_answered_after_the_blocks_own_replies() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(3));
        terminal.feed(b"\x1b[?2026h\x1b[>q\x1b[c");
        assert!(
            terminal.take_pty_writes().is_empty(),
            "the block is still holding its bytes, so nothing it carried has been read out"
        );
        terminal.feed(b"\x1b[?2026l");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![b"\x1b[?6c".to_vec(), XTVERSION_REPLY.as_bytes().to_vec()],
            "one answer, at the commit, behind the replies the block's own replay produced"
        );
        assert!(terminal.take_pty_writes().is_empty());
    }

    /// **An answer owed from inside a block leaves at that block's commit, not whenever no block
    /// happens to be open.** A program that repaints in synchronized frames opens the next one in
    /// the same write that closed the last, so "is a block buffering right now?" is a question that
    /// is true again a handful of bytes later — and an answer held on that condition was held until
    /// some unrelated later frame ended, or not delivered in this feed at all.
    #[test]
    fn a_debt_from_one_block_is_not_held_by_the_next_block() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(3));
        terminal.feed(b"\x1b[?2026h\x1b[>q\x1b[?2026l\x1b[?2026h");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![XTVERSION_REPLY.as_bytes().to_vec()],
            "the block that carried the question ended, so the answer is due"
        );
    }

    /// **The limit stays inside the block it is a limit about.** A program is free to wrap its
    /// capability probe in a synchronized frame, and if it does, the answer owed from inside that
    /// frame must still come back ahead of the DA1 sentinel written after it — otherwise the probe
    /// fails exactly as it did before this window answered at all.
    #[test]
    fn a_debt_from_one_block_is_not_overtaken_by_a_reply_after_it() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(3));
        terminal.feed(b"\x1b[?2026h\x1b[>q\x1b[?2026l\x1b[c");
        assert_eq!(
            terminal.take_pty_writes(),
            vec![XTVERSION_REPLY.as_bytes().to_vec(), b"\x1b[?6c".to_vec()],
            "nothing outside the block may overtake an answer the block already owed"
        );
    }

    /// The same two streams cut at every byte position: where a pty read ended is not a fact about
    /// the stream, and a block that ends in one read and is reopened in the next is the ordinary
    /// shape of a repaint.
    #[test]
    fn cutting_a_block_that_owes_an_answer_changes_neither_the_answer_nor_its_place() {
        for (stream, expected) in [
            (
                b"\x1b[?2026h\x1b[>q\x1b[?2026l\x1b[?2026h".as_slice(),
                vec![XTVERSION_REPLY.as_bytes().to_vec()],
            ),
            (
                b"\x1b[?2026h\x1b[>q\x1b[?2026l\x1b[c",
                vec![XTVERSION_REPLY.as_bytes().to_vec(), b"\x1b[?6c".to_vec()],
            ),
        ] {
            for split in 0..=stream.len() {
                let mut terminal = TerminalAdapter::new(nz(20), nz(3));
                terminal.feed(&stream[..split]);
                let mut replies = terminal.take_pty_writes();
                terminal.feed(&stream[split..]);
                replies.extend(terminal.take_pty_writes());
                assert_eq!(
                    replies, expected,
                    "{stream:?} cut at {split} was answered differently"
                );
            }
        }
    }

    /// **Segmenting must not change what lands on the grid, and the one place it could is the
    /// vendored parser's own overflow rule.** `vte` 0.15 gives up on a synchronized block when the
    /// bytes it is holding plus *the slice it was just handed* would reach its 2 MiB buffer
    /// (`ansi.rs` `advance_sync`), so cutting a feed in two can move the moment that trips: a block
    /// large enough to overflow a whole feed can be buffered when the same bytes arrive as two
    /// segments and overflow on the second. Both endings write the same bytes to the grid in the
    /// same order — the block's held bytes first, then the rest parsed live — and this is the pin
    /// on that. The stream is built to straddle the rule deliberately, and the straddle is asserted
    /// rather than assumed, so it cannot quietly stop testing anything.
    #[test]
    fn a_block_that_straddles_the_vendored_overflow_rule_lands_the_same_either_way() {
        // `vte-0.15.0/src/ansi.rs`: `SYNC_BUFFER_SIZE`, and `advance_sync` gives up when
        // `buffer.len() + bytes.len() >= SYNC_BUFFER_SIZE - 1`.
        const SYNC_BUFFER_SIZE: usize = 0x20_0000;
        const HEAD: &[u8] = b"\x1b[HTOP";
        const QUERY: &[u8] = b"\x1b[>q";
        const FOOT: &[u8] = b"\x1b[2;1HEND";
        const BSU: &[u8] = b"\x1b[?2026h";
        const ESU: &[u8] = b"\x1b[?2026l";
        // NUL is ignored by both parsers and never leaves the ground state, so the filler is size
        // and nothing else. Chosen so that the whole feed without the query reaches the rule in one
        // slice, while the segment the query ends does not.
        let filler = vec![0u8; SYNC_BUFFER_SIZE - 14];
        let queried: Vec<u8> = [BSU, HEAD, &filler, QUERY, FOOT, ESU].concat();
        let quiet: Vec<u8> = [BSU, HEAD, &filler, FOOT, ESU].concat();
        assert!(
            HEAD.len() + filler.len() + FOOT.len() + ESU.len() >= SYNC_BUFFER_SIZE - 1,
            "the whole feed must reach the vendored overflow rule in one slice"
        );
        assert!(
            HEAD.len() + filler.len() + QUERY.len() < SYNC_BUFFER_SIZE - 1,
            "the segment the query ends must not reach it"
        );

        // The straddle itself: the same bytes, one slice short of the block's terminator, leave the
        // vendored parser in two different states — still holding the block, or already given up.
        let mut buffering = TerminalAdapter::new(nz(20), nz(3));
        buffering.feed(&queried[..queried.len() - FOOT.len() - ESU.len()]);
        assert!(
            buffering.synchronized_update_pending_bytes() > 0,
            "the segment ending at the query is under the rule and must still be held"
        );
        let mut overflowed = TerminalAdapter::new(nz(20), nz(3));
        overflowed.feed(&quiet[..quiet.len() - ESU.len()]);
        assert_eq!(
            overflowed.synchronized_update_pending_bytes(),
            0,
            "the unsegmented feed is over the rule and must already have been given up on"
        );

        let mut segmented = TerminalAdapter::new(nz(20), nz(3));
        segmented.feed(&queried);
        assert_eq!(
            segmented.take_pty_writes(),
            vec![XTVERSION_REPLY.as_bytes().to_vec()]
        );
        let mut whole = TerminalAdapter::new(nz(20), nz(3));
        whole.feed(&quiet);
        assert!(whole.take_pty_writes().is_empty());
        assert_eq!(
            segmented.visible_text(),
            whole.visible_text(),
            "the two endings of the same block must leave the same screen"
        );
        assert_eq!(segmented.visible_text()[0].trim_end(), "TOP");
        assert_eq!(segmented.visible_text()[1].trim_end(), "END");
    }

    /// A synchronized block big enough for the vendored parser to give up on, with an answer owed
    /// from inside it. The filler is NUL, which both parsers ignore and which never leaves the
    /// ground state, so the block is size and nothing else.
    fn an_overflowing_block_owing_one_answer(tail: &[u8]) -> Vec<u8> {
        [
            b"\x1b[?2026h\x1b[>q".as_slice(),
            &vec![0u8; VENDOR_SYNC_BUFFER_SIZE],
            tail,
        ]
        .concat()
    }

    /// **The other way a block ends, and the answer leaves there too.** A block whose bytes would
    /// overflow the vendored parser's buffer is given up on mid-slice, and the rest of that slice is
    /// then parsed as ordinary output — so a DA1 after it was answered first, and a `BSU` after it
    /// opened a new block that went on holding the old block's answer until some later frame ended.
    /// The size rule is arithmetic and both its terms are visible from here, so the byte the block
    /// ends on is found and cut at, exactly as its ESU would be.
    #[test]
    fn an_answer_owed_by_an_overflowing_block_leaves_where_that_block_ends() {
        for tail in [b"\x1b[c\x1b[?2026h".as_slice(), b"\x1b[c"] {
            let mut terminal = TerminalAdapter::new(nz(20), nz(3));
            terminal.feed(b"\x1b[?2026h\x1b[>q");
            assert!(
                terminal.take_pty_writes().is_empty(),
                "the block is still holding the question"
            );
            terminal.feed(&[vec![0u8; VENDOR_SYNC_BUFFER_SIZE], tail.to_vec()].concat());
            assert_eq!(
                terminal.take_pty_writes(),
                vec![XTVERSION_REPLY.as_bytes().to_vec(), b"\x1b[?6c".to_vec()],
                "with {} bytes of tail, the answer must leave where its block ended",
                tail.len()
            );
        }
    }

    /// The same stream cut across two feeds: at each seam byte by byte, on both sides of the byte
    /// the size rule trips at, and in the middle of the block. Where a pty read ended decides which
    /// feed carries the overflow, and must decide nothing else.
    #[test]
    fn cutting_an_overflowing_block_changes_neither_its_answer_nor_its_place() {
        let stream = an_overflowing_block_owing_one_answer(b"\x1b[c\x1b[?2026h");
        let expected = vec![XTVERSION_REPLY.as_bytes().to_vec(), b"\x1b[?6c".to_vec()];
        // Fed whole, the block is handed `\x1b[>q` first, so the rule trips this far into the rest.
        let trips_at = b"\x1b[?2026h\x1b[>q".len() + (VENDOR_SYNC_BUFFER_SIZE - 1) - 4 - 1;
        let seams = (0..=16)
            .chain([trips_at - 1, trips_at, trips_at + 1, stream.len() / 2])
            .chain(stream.len() - 16..=stream.len());
        for split in seams {
            let mut terminal = TerminalAdapter::new(nz(20), nz(3));
            terminal.feed(&stream[..split]);
            let mut replies = terminal.take_pty_writes();
            terminal.feed(&stream[split..]);
            replies.extend(terminal.take_pty_writes());
            assert_eq!(replies, expected, "cut at {split} was answered differently");
        }
    }

    /// **Which side of the limit the byte that ends the block is on.** The block ends on one byte,
    /// and that byte is parsed as ordinary output right after the commit — so it can complete a
    /// sequence, but only one whose `CSI` was among the bytes the block was holding. A DA1 written
    /// inside the frame and finished by that byte is the frame's own question, so its answer belongs
    /// where every other reply the block produced belongs: ahead of the answer the block owed. A DA1
    /// written after the block is answered after it, like anything else outside.
    #[test]
    fn a_reply_the_overflowing_block_asked_for_itself_stays_on_the_blocks_side() {
        // Sized so that the byte the rule trips on is the `c` of the first DA1: the vendored buffer
        // already holds the four bytes of the query, so it can take `SYNC_BUFFER_SIZE - 1 - 4 - 1`
        // more, and the `\x1b[` must be the last two of those.
        let held = (VENDOR_SYNC_BUFFER_SIZE - 1) - b"\x1b[>q".len() - 1;
        let inside: Vec<u8> =
            [vec![0u8; held - 2], b"\x1b[c".to_vec(), b"\x1b[c".to_vec()].concat();
        let mut terminal = TerminalAdapter::new(nz(20), nz(3));
        terminal.feed(b"\x1b[?2026h\x1b[>q");
        assert!(terminal.take_pty_writes().is_empty());
        terminal.feed(&inside);
        assert_eq!(
            terminal.take_pty_writes(),
            vec![
                b"\x1b[?6c".to_vec(),
                XTVERSION_REPLY.as_bytes().to_vec(),
                b"\x1b[?6c".to_vec(),
            ],
            "the DA1 the block itself asked for comes first, the one after the block comes last"
        );
    }

    /// **A feed that asks nothing reports its events in the order it always did.** Cutting the feed
    /// is what a query costs, and a feed without one must not pay it: the bell the boundary parser
    /// found, the title the processor reported and the rows it wrote come back exactly as they came
    /// back before segments existed, which is transcript, then adapter events, then grid writes.
    #[test]
    fn a_feed_that_asks_nothing_reports_its_events_in_the_order_it_always_did() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(3));
        let events = terminal.feed(b"\x07\x1b]0;title\x07text");
        assert_eq!(
            events,
            vec![
                AdapterEvent::Title {
                    title: "title".to_string()
                },
                AdapterEvent::Bell,
                AdapterEvent::GridWrites {
                    screen: RemovalScreen::Primary,
                    rows: vec![0],
                },
            ]
        );
    }

    /// A shell marker stops the stream where it stands, so the bells on either side of one are found
    /// in different turns of the pump. Each is reported once, in its own turn, and neither is lost
    /// to the early return the pause makes.
    #[test]
    fn bells_on_both_sides_of_a_paused_marker_are_each_reported_once() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(3));
        let before = terminal.feed(b"\x07\x1b]133;A\x07\x07");
        assert_eq!(
            before
                .iter()
                .filter(|event| **event == AdapterEvent::Bell)
                .count(),
            1,
            "the bell before the marker is reported with the bytes before the marker"
        );
        assert!(terminal.stream_paused());
        let after = terminal.resume_stream();
        assert_eq!(
            after
                .iter()
                .filter(|event| **event == AdapterEvent::Bell)
                .count(),
            1,
            "the bell after the marker is reported when the stream continues"
        );
        assert!(!terminal.stream_paused());
    }

    /// **The one order this branch changes, pinned deliberately.** A feed that does carry a query is
    /// cut at it, and the bells found before the cut are reported when the processor reaches the
    /// cut — so a bell written before the query now precedes a title written after it, where before
    /// every bell in a feed came after every title in it. This is the more faithful of the two: the
    /// bell really did come first in the stream. Nothing downstream can tell the difference in any
    /// case — a bell sets `bell` on the session and a title sets `window_title`, two fields neither
    /// of which is read while the other is applied.
    #[test]
    fn a_bell_before_a_query_is_reported_before_a_title_after_it() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(3));
        let events = terminal.feed(b"\x07\x1b[>q\x1b]0;title\x07");
        assert_eq!(
            events,
            vec![
                AdapterEvent::Bell,
                AdapterEvent::Title {
                    title: "title".to_string()
                },
            ]
        );
        assert_eq!(
            terminal.take_pty_writes(),
            vec![XTVERSION_REPLY.as_bytes().to_vec()]
        );
    }

    /// A character that takes more than one byte is held by the parser's own state across an
    /// `advance`, so cutting a feed into segments cannot break one in half — and the canonical fork
    /// a resize transaction keeps is fed the same segments, so it cannot break one either. Cut at
    /// every byte position, with the fork armed and a reflow taken at the end.
    #[test]
    fn a_multi_byte_character_survives_every_cut_with_the_canonical_fork_armed() {
        let stream = "café\x1b[>qnaïve".as_bytes();
        for split in 0..=stream.len() {
            let mut terminal = TerminalAdapter::new(nz(20), nz(3));
            terminal.begin_resize_transaction();
            terminal.feed(&stream[..split]);
            let mut replies = terminal.take_pty_writes();
            terminal.feed(&stream[split..]);
            replies.extend(terminal.take_pty_writes());
            assert_eq!(
                replies,
                vec![XTVERSION_REPLY.as_bytes().to_vec()],
                "cut at {split}"
            );
            assert_eq!(
                terminal.visible_text()[0].trim_end(),
                "cafénaïve",
                "cut at {split}"
            );
            terminal.resize(nz(16), nz(3));
            let _ = terminal.finish_resize_transaction();
            assert_eq!(
                terminal.visible_text()[0].trim_end(),
                "cafénaïve",
                "cut at {split}, after the reflow"
            );
        }
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

    /// **Twelve grapheme families, each kept whole in the cell it opened, however its bytes are
    /// cut** (review 2026-09-17 third pass).
    ///
    /// The retained cluster is checked against the cell it describes before it is extended
    /// (`Term::cell_holds_cluster`), which is what stops an erase, a tab or a scroll under that
    /// coordinate from resurrecting text the screen no longer holds. The other side of that check
    /// is this: ordinary text must never trip it. These are the families the stock fixtures did not
    /// cover — Devanagari, Thai and an ideographic variation sequence among them — driven whole, at
    /// every byte boundary, and one byte at a time, which is how a cluster actually arrives from a
    /// pipe.
    ///
    /// The width each family occupies is the oracle's business and is deliberately not asserted
    /// here; what is asserted is that the cell holds the complete text, so nothing was dropped and
    /// nothing was pushed into a cell of its own.
    #[test]
    fn every_grapheme_family_keeps_its_cluster_however_its_bytes_arrive() {
        let clusters = [
            (
                "family with zero-width joiners",
                "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}",
            ),
            ("regional indicator flag", "\u{1f1ef}\u{1f1f5}"),
            ("skin-tone modifier", "\u{1f44d}\u{1f3fd}"),
            ("two stacked accents", "e\u{301}\u{327}"),
            ("Devanagari vowel sign", "\u{915}\u{93f}"),
            ("Devanagari conjunct", "\u{915}\u{94d}\u{937}"),
            ("Thai tone mark", "\u{e01}\u{e49}"),
            ("CJK with a text selector", "\u{6f22}\u{fe0e}"),
            ("CJK with an emoji selector", "\u{6f22}\u{fe0f}"),
            ("ideographic variation sequence", "\u{6f22}\u{e0100}"),
            ("an arrow the selector widens", "\u{2194}\u{fe0f}"),
            ("a watch the selector narrows", "\u{231a}\u{fe0e}"),
        ];

        for (name, cluster) in clusters {
            let bytes = cluster.as_bytes();
            let mut feeds = vec![vec![bytes.to_vec()]];
            for split in 1..bytes.len() {
                feeds.push(vec![bytes[..split].to_vec(), bytes[split..].to_vec()]);
            }
            feeds.push(bytes.iter().map(|byte| vec![*byte]).collect());

            for pieces in feeds {
                let mut terminal = TerminalAdapter::new(nz(10), nz(2));
                terminal.feed(b"\x1b[?2027h");
                for piece in &pieces {
                    terminal.feed(piece);
                }
                let row = terminal.visible_row(0).expect("row 0 is on the grid");
                assert_eq!(
                    row.cells[0].text.as_str(),
                    cluster,
                    "{name}: the cell that opened the cluster holds all of it, fed as {pieces:?}"
                );
                assert_eq!(
                    row.cells
                        .iter()
                        .skip(1)
                        .map(|cell| cell.text.as_str())
                        .collect::<String>()
                        .trim(),
                    "",
                    "{name}: and no part of it was pushed into a cell of its own"
                );
            }
        }
    }

    /// One base64 `image/png` payload: the smallest complete PNG there is, a single opaque pixel.
    /// Built rather than pasted so that what it is stays readable — the eight-byte signature, the
    /// header, one zlib-stored scanline and the end marker, each with its own CRC.
    fn one_pixel_png() -> String {
        use base64::Engine as _;

        fn chunk(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
            let crc = crc32(&[kind.as_slice(), payload].concat());
            let mut bytes = (payload.len() as u32).to_be_bytes().to_vec();
            bytes.extend_from_slice(kind);
            bytes.extend_from_slice(payload);
            bytes.extend_from_slice(&crc.to_be_bytes());
            bytes
        }

        fn crc32(bytes: &[u8]) -> u32 {
            let mut crc = u32::MAX;
            for byte in bytes {
                crc ^= u32::from(*byte);
                for _ in 0..8 {
                    crc = if crc & 1 == 1 {
                        (crc >> 1) ^ 0xEDB8_8320
                    } else {
                        crc >> 1
                    };
                }
            }
            !crc
        }

        // One stored-mode deflate block holding the single scanline `00 ff ff ff` (filter 0, then
        // one opaque white pixel), wrapped in the zlib header and Adler-32 the format asks for.
        let scanline = [0u8, 0xff, 0xff, 0xff];
        let mut deflate = vec![0x78, 0x01, 0x01, 0x04, 0x00, 0xfb, 0xff];
        deflate.extend_from_slice(&scanline);
        let (mut a, mut b) = (1u32, 0u32);
        for byte in scanline {
            a = (a + u32::from(byte)) % 65521;
            b = (b + a) % 65521;
        }
        deflate.extend_from_slice(&((b << 16) | a).to_be_bytes());

        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]));
        png.extend(chunk(b"IDAT", &deflate));
        png.extend(chunk(b"IEND", &[]));
        base64::engine::general_purpose::STANDARD.encode(png)
    }

    /// **An image is a fact about the grid, so it is read against a grid the bytes before it have
    /// reached** (review 2026-09-17 third pass).
    ///
    /// DEC 2026 holds a block of writes back and applies them at the terminator, and the image's
    /// own position was read while they were still held: the `CUP` that put the cursor where the
    /// image belongs had not been parsed, nor had the swap that decides which screen it is on. So
    /// an image drawn inside a synchronized update was filed at the cursor from before the block,
    /// on the screen from before it, while its placeholder — which goes through the same parser —
    /// landed in the right place. The record pointed at a cell that held something else, which is
    /// where a reader's hover and peek look.
    ///
    /// Every byte split of each sequence, because the defect is about when a fact is read and a
    /// read that happens to fall on a feed boundary is not a different rule.
    #[test]
    fn an_image_inside_a_synchronized_update_is_filed_where_its_placeholder_lands() {
        let payload = one_pixel_png();
        let held = format!("\x1b[?2026h\x1b[3;5H\x1b]1337;File=inline=1:{payload}\x07\x1b[?2026l");
        let swapped = format!(
            "\x1b[?2026h\x1b[?1049h\x1b[3;5H\x1b]1337;File=inline=1:{payload}\x07\x1b[?2026l"
        );
        // The right margin, where the placeholder is measured rather than simply counted: seven
        // columns of `[image]` do not fit in the three left at column 37 of a 40-column grid.
        let margin =
            format!("\x1b[?2026h\x1b[1;38H\x1b]1337;File=inline=1:{payload}\x07\x1b[?2026l");

        for (name, stream, expected) in [
            (
                "a held update",
                held,
                AdapterEvent::InlineImage {
                    screen: RemovalScreen::Primary,
                    row: 2,
                    column: 4,
                    placeholder_columns: 7,
                    encoded: payload.clone().into_bytes(),
                },
            ),
            (
                "a held update that took the alternate screen first",
                swapped,
                AdapterEvent::InlineImage {
                    screen: RemovalScreen::Alternate,
                    row: 2,
                    column: 4,
                    placeholder_columns: 7,
                    encoded: payload.clone().into_bytes(),
                },
            ),
            (
                "a held update at the right margin",
                margin,
                AdapterEvent::InlineImage {
                    screen: RemovalScreen::Primary,
                    row: 0,
                    column: 37,
                    placeholder_columns: 3,
                    encoded: payload.clone().into_bytes(),
                },
            ),
        ] {
            let bytes = stream.as_bytes();
            for split in 0..=bytes.len() {
                let mut terminal = TerminalAdapter::new(nz(40), nz(4));
                let mut events = terminal.feed(&bytes[..split]);
                events.extend(terminal.feed(&bytes[split..]));
                let images = events
                    .iter()
                    .filter(|event| matches!(event, AdapterEvent::InlineImage { .. }))
                    .collect::<Vec<_>>();
                assert_eq!(
                    images,
                    vec![&expected],
                    "{name}, split={split}: the image is filed where its placeholder is written"
                );
            }
        }
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

    /// R1-13. vte force-ends a DEC 2026 update once its own 2 MiB buffer overflows
    /// (`vte-0.15.0/src/ansi.rs` `advance_sync`) and clears the deadline with it. The adapter's
    /// own flag has to follow that, or every byte after the overflow is retained for a replay
    /// that will never happen.
    #[test]
    fn a_forced_synchronized_end_stops_the_replay_tail_from_growing() {
        let mut terminal = TerminalAdapter::new(nz(80), nz(24));
        terminal.feed(b"\x1b[?2026h");
        assert!(terminal.parser_sync_active, "the update opened");

        // Past vte's own SYNC_BUFFER_SIZE, so the vendored parser gives up on the update.
        terminal.feed(&vec![b'a'; 3 * 1024 * 1024]);
        assert!(
            terminal.synchronized_update_deadline().is_none(),
            "the vendored parser force-ended the update on its buffer overflow"
        );
        assert!(
            !terminal.parser_sync_active,
            "so the adapter's own flag is down too"
        );

        terminal.feed(&vec![b'b'; 1024 * 1024]);
        assert!(
            terminal.parser_tail.len() <= 1,
            "a printable byte outside an update completes a sequence and clears the tail, so \
             nothing is retained: {} bytes",
            terminal.parser_tail.len()
        );
    }

    /// R3-2. `CSI ? 2026 h` carrying more than vte's 32 parameters is refused by the real parser
    /// (`ignore` is set and its `csi_dispatch` returns before the mode is read), so it arms
    /// nothing here either.
    #[test]
    fn a_synchronized_start_the_parser_refused_arms_no_retention() {
        let mut terminal = TerminalAdapter::new(nz(80), nz(24));
        let mut sequence = b"\x1b[?".to_vec();
        for _ in 0..32 {
            sequence.extend_from_slice(b"2026;");
        }
        sequence.extend_from_slice(b"2026h");
        terminal.feed(&sequence);

        assert!(
            terminal.synchronized_update_deadline().is_none(),
            "the vendored parser refused the sequence"
        );
        assert!(
            !terminal.parser_sync_active,
            "so no update is open here either"
        );

        terminal.feed(&vec![b'c'; 512 * 1024]);
        assert!(
            terminal.parser_tail.len() <= 1,
            "nothing is retained for a replay: {} bytes",
            terminal.parser_tail.len()
        );
    }

    /// R1-14, the adapter's half: an OSC nobody terminates cannot make the replay tail grow
    /// without end either.
    #[test]
    fn an_unterminated_osc_leaves_the_replay_tail_bounded() {
        let mut terminal = TerminalAdapter::new(nz(80), nz(24));
        let mut bytes = b"\x1b]0;".to_vec();
        bytes.extend(std::iter::repeat_n(b'A', 4 * 1024 * 1024));
        terminal.feed(&bytes);
        assert!(
            terminal.parser_tail.len() <= MAX_UNOWNED_OSC_BYTES + 16,
            "the tail holds at most the scanner's own ceiling: {} bytes",
            terminal.parser_tail.len()
        );
    }

    /// R3-4. A grapheme cluster is tens of code points; a child sending ten thousand combining
    /// marks is not describing one.
    #[test]
    fn a_grapheme_cluster_stops_growing_at_the_ceiling() {
        let mut terminal = TerminalAdapter::new(nz(80), nz(24));
        terminal.feed(b"\x1b[?2027h");
        let mut bytes = "a".to_string();
        for _ in 0..10_000 {
            bytes.push('\u{301}');
        }
        terminal.feed(bytes.as_bytes());

        let row = terminal.visible_row(0).expect("the first row");
        let cluster = row.cells[0].text.as_str().chars().count();
        assert!(
            cluster <= bt_unicode::MAX_GRAPHEME_CLUSTER_CHARS,
            "the cell holds a bounded cluster: {cluster} code points"
        );
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
