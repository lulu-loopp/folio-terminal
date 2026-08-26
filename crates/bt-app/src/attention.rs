//! **The attention ledger: two producers, one episode, one place that decides.**
//!
//! `docs/plans/attention/plan.md` is the specification and §11.1 is this file. Everything here is
//! bookkeeping — no dot is drawn here and no toast is raised here — but since A2 it is the
//! bookkeeping the window's own queue is made of: the place this hands out is the place
//! `StatusClaim::Awaiting` is drawn from. What it owns is the one question the old machine could not
//! answer: **is this the same request as the last one, and has the user dealt with it?**
//!
//! # The defect this shape exists to remove
//!
//! Today's "waiting for you" has exactly one producer — a `BEL` — and a bell is an *event* that
//! rings at the end of a turn. The plan's §0 says what that costs: a one-shot event was projected
//! onto a badge that means "it is standing there waiting for you", so the badge lights when it
//! should not, stays lit when it should not, and cannot say why. The fix is not a better bell; it
//! is a credential that can be **withdrawn**, and a ledger that can tell an unanswered credential
//! from one the user has already dealt with.
//!
//! # Why a generation and not a bit
//!
//! A bit cannot express *the user answered, and the program has not withdrawn yet*. With a bit,
//! the frame after the answer reads "still asserted" and hands out a fresh badge — the same class
//! of defect as 2026-08-21's "the badge outlived the thing it reported", moved to another pillar.
//!
//! So each producer mints a strictly increasing **generation**, and an answer is recorded as a
//! **watermark**. `generation > watermark` is an unanswered credential; `generation <= watermark`
//! is one that has been dealt with. Two consequences fall straight out and both are load-bearing:
//!
//! * a withdrawal never has to wind a watermark back, because the next generation is larger than
//!   anything the last one was compared against — **one fewer cleanup path is one fewer place to
//!   forget**;
//! * a restatement of the same credential cannot swallow itself, and a genuinely new request
//!   cannot be swallowed by an old answer. The second half is the one that matters: a swallowed
//!   request is a person waiting on an agent that never lit up.
//!
//! # Two producers, and neither may forge the other
//!
//! The **weak** tier is a fact about bytes (`OSC 1337;RequestAttention=yes`) and lives in
//! `bt-term`, where it is a pure function of the stream and can be replayed offline. The **strong**
//! tier is a fact about a pane-local endpoint (`folio attention wait`) and lives here, on the leaf.
//! Neither can mint an episode, because neither can see the other. **The coordinator can**, which
//! is the whole reason it is a third thing and not a field on one of them.
//!
//! A third tier, [`Credential::Announced`] — a bare bell, `RequestAttention=once`,
//! `OSC 9`/`777`/`99`, the end of a turn — reaches this file through two doors and **neither of
//! them opens onto the machine above**: [`AttentionLedger::announce_turn_end`], which mints nothing
//! and takes no ticket, and [`AttentionLedger::announce`], which does not even write a line — all
//! it may do is lend a request the words the program itself wrote. That is the plan's red line 14,
//! and the reason it is a red line is that every regression this block is about began with an event
//! being promoted to a state.
//!
//! # What is deliberately not here
//!
//! Nothing in this file reads the window, **and nothing in it reads a clock**. The four-state
//! machine is a pure function of the fields below, the trace lines are returned rather than
//! written, and the facts that belong to the frame — whether the tab is active and the window
//! focused, how far a notification can reach, and what time it is — are handed in. That is what
//! lets the arrival grid of §11.1.4 be tested cell by cell instead of by driving a terminal, and it
//! is why [`AttentionLedger::is_agent_seat`] can be asked about a pane whose last signal was nine
//! minutes ago without anybody having to wait nine minutes.

// **A2 took the blanket `dead_code` excuse away**, which is what it said it would do: this module
// is the running build's attention queue now, and the two things left unconstructed below carry
// their own one-line reasons at the point where the gap is rather than over the whole file.

use std::{fmt, time::Instant};

use bt_layout::SeatId;

use crate::attention_wire::WAIT_TTL;

/// How many outstanding strong credentials one pane may hold (`attention` plan §11.4.1).
///
/// Eight, and the bound is reachable only by a keyed producer: a pane whose kinds are all running
/// in level mode can hold at most one credential per kind, and there are four kinds. Overflow drops
/// the **oldest** rather than refusing the newest, because the newest is the one that is actually
/// happening.
const MAX_OUTSTANDING_WAITS: usize = 8;

/// The longest association key an endpoint may hand in (`attention` plan §11.4.1).
const MAX_WAIT_KEY_BYTES: usize = 64;

// ---------------------------------------------------------------------------
// Vocabulary
// ---------------------------------------------------------------------------

/// The pane a line is about. `claim` is the one verb that is about a tab instead.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Site {
    pub tab: usize,
    pub seat: SeatId,
}

impl fmt::Display for Site {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "tab={} seat={:?}", self.tab, self.seat)
    }
}

/// **How far a request gets on this desktop** (`attention` plan §10.7's eight-row table).
///
/// Named here because it is the ledger's own vocabulary — every `toast` line carries one, and both
/// doors evaluate the same three-way answer. The *function* that computes it from the window's
/// facts is [`crate::notify::desktop_reach`]; this is the answer's shape, and the shape is what the
/// ledger and its trace need to agree on first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Reach {
    /// You are looking at it. Adding anything would be telling you what you can already see.
    Nothing,
    /// On this screen but not in front of your eyes: the in-window marks, and a taskbar flash
    /// where Windows will honour one.
    Flash,
    /// Out of reach of the window entirely — minimised or on a virtual desktop the reader has
    /// switched away from. The desktop is what is left.
    ///
    /// The two facts behind it are `IsIconic` and `DWMWA_CLOAKED`, and "completely covered by
    /// another window" is deliberately **not** one of them: winit's `Occluded` is never delivered
    /// by the Windows backend, and computing it here would mean walking the z-order and
    /// differencing regions every frame — a heuristic, priced per frame. The cost of leaving it
    /// out is that a window buried under a full-screen editor flashes its taskbar button instead
    /// of raising a toast, and a taskbar button is visible in exactly that situation.
    Toast,
}

impl fmt::Display for Reach {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Nothing => "nothing",
            Self::Flash => "flash",
            Self::Toast => "toast",
        })
    }
}

/// **How strong the evidence behind a live episode is right now** (`attention` plan §10.8).
///
/// Not a severity and not a second dot — the two share one pixel and differ only in what the words
/// beside it say. `RequestAttention=yes` proves a program *wants* you; it does not prove the
/// program is *blocked on you*, and promoting the first into the second is the same mistake this
/// whole block is correcting in the bell.
///
/// **Derived, never latched.** It is a function of whether an unanswered strong credential exists
/// at this instant, so a strong tier that withdraws while a weak one keeps asserting takes the
/// wording back down with it. A latched "only ever rises" version would keep saying "waiting for
/// you" on the strength of a credential that no longer exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Grounds {
    /// "Attention requested" — a program wants you.
    Requested,
    /// "Waiting for you" — a program is blocked on your input.
    AwaitingInput,
}

impl fmt::Display for Grounds {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Requested => "requested",
            Self::AwaitingInput => "awaiting",
        })
    }
}

/// **Which of the three credential levels an arrival is worth** (`attention` plan §11.6).
///
/// The level A7 added to the plan, made a type. Before it, an event had no name in this vocabulary
/// and had to be argued out of the ledger one reading at a time; a level whose *mints an episode*
/// and *takes a place* columns both read **no** says that once, and says it somewhere a mapping row
/// can declare rather than somewhere a parser has to remember.
///
/// **Having a name in the table is not the same as having power in it.** That sentence is the whole
/// of red line 14, and [`Self::Announced`] is the level it is about.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Credential {
    /// A bare bell, `RequestAttention=once`, `OSC 9`/`777`/`99`, the end of a turn.
    ///
    /// One-shot, and the consequence is not a policy: a sentence with no "off" cannot be withdrawn,
    /// so nothing built on it could ever be taken back — which is the defect this whole block is
    /// the correction of, and the reason this level mints nothing and holds nothing.
    Announced,
    /// `OSC 1337;RequestAttention=yes`, taken back by `=no`. A program **wants** you.
    Weak,
    /// `folio attention wait`. A program is **blocked on** you.
    #[allow(
        dead_code,
        reason = "the pipe lane spells this tier as a `WaitKind` row today; the C slices name it"
    )]
    Strong,
}

/// The four states of `attention` plan §11.1.2, which are a **pure function** of the fields of
/// [`AttentionLedger`] and are stored nowhere.
///
/// Not storing them is the point: two producers each keeping their own copy of "are we asking" is
/// exactly how the two would drift, and a test can build a field combination directly instead of
/// walking a path of events to reach it.
///
/// **The window reads [`AttentionLedger::ticket`] and [`AttentionLedger::grounds`] rather than
/// this**, and the asymmetry is deliberate: a dot is drawn from what is being asked for, and a
/// caller handed a four-way state would have to `match` its way back to that. What this is for is
/// the grid's own cells — a test that asserts the whole state would otherwise have to reconstruct
/// it out of the two accessors, which is the reconstruction the derivation exists to make
/// unnecessary.
#[allow(
    dead_code,
    reason = "the derived state is what the grid's cells are spelled against"
)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum State {
    /// Neither tier is asserting anything. Every leaf is born here.
    Idle,
    /// Something unanswered is asserted, and no place in the queue has been taken yet.
    Requested(u64),
    /// Something unanswered is asserted, and this pane holds place `ticket`.
    Queued { episode: u64, ticket: u64 },
    /// **Credentials are still up, and you have answered all of them.**
    ///
    /// The state a single bit cannot represent, and the reason the whole ledger is shaped this
    /// way: it takes no ticket, raises no toast, and is a fixed point under the per-frame pass.
    Acknowledged(u64),
}

/// **How a fact got in** — the transport class, and it is closed (`attention` plan §13.2.2).
///
/// Separated from *who said it* ([`Via`]) because one field cannot answer both: a turn-end that
/// arrived over `OSC 777` and one that arrived as a bare `0x07` are different arrivals, they are
/// fixed by different requests upstream, and recording the first as the second makes it impossible
/// to tell whether an adapter is installed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Transport {
    /// The pane-local named pipe behind `folio attention`.
    Pipe,
    /// An OSC sequence in this pane's byte stream.
    Osc,
    /// A bare `BEL`. **Only** `0x07` is this.
    Bel,
}

impl fmt::Display for Transport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pipe => "pipe",
            Self::Osc => "osc",
            Self::Bel => "bel",
        })
    }
}

/// **Who said it** — the sequence or upstream event name, and it is deliberately open.
///
/// Adding a CLI adds a name here and never touches [`Transport`]. That asymmetry is the reason the
/// two are separate fields rather than one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Via {
    /// Claude Code's `Stop` hook: the main agent finished responding.
    Stop,
    /// Claude Code's `StopFailure` hook: the turn ended on an API error.
    StopFailure,
    /// A bare bell at the end of a turn — a literal `0x07` and nothing else.
    Bel,
    // **The four sequences, and why each is here rather than folded into `Bel`.** §13.2.2 exists
    // because a survey found codex reaching Folio as a bare bell only *because* codex does not
    // recognise this terminal, and pi reaching it over `OSC 777` only when somebody configures the
    // example extension by hand. Recording either of those as "a bell" makes "did the adapter
    // install?" a question with no answer in the file that is supposed to answer it.
    /// `OSC 1337;RequestAttention=once` — iTerm2's one-shot arm, which latches the bell.
    Osc1337,
    /// iTerm2's free-text arm of `OSC 9`. codex's `notification_method = osc9`.
    Osc9,
    /// urxvt's `OSC 777;notify`. What pi's example extension can be pointed at.
    Osc777,
    /// kitty's `OSC 99`. **No producer in the parser yet** — named because the vocabulary is fixed
    /// here, so the day one arrives it is a row and not a value.
    #[allow(dead_code, reason = "`bt-term` has no `OSC 99` parser yet")]
    Osc99,
    /// codex's `notify` program, whose only `type` today is `agent-turn-complete`.
    Notify,
    /// pi's `agent_settled`: "fires only once a run fully settles".
    AgentSettled,
}

impl fmt::Display for Via {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Stop => "stop",
            Self::StopFailure => "stop-failure",
            Self::Bel => "bel",
            Self::Osc1337 => "osc-1337",
            Self::Osc9 => "osc-9",
            Self::Osc777 => "osc-777",
            Self::Osc99 => "osc-99",
            Self::Notify => "notify",
            Self::AgentSettled => "agent-settled",
        })
    }
}

/// The four things a program can be waiting for (`attention` plan §11.4).
///
/// A closed list, and it is closed on purpose: the mapping tables are data, and a family that
/// wants a fifth kind is asking for a ruling, not for a row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WaitKind {
    Permission,
    Elicitation,
    Agent,
    Quota,
}

impl fmt::Display for WaitKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Permission => "permission",
            Self::Elicitation => "elicitation",
            Self::Agent => "agent",
            Self::Quota => "quota",
        })
    }
}

/// **The slot one strong credential occupies**, and the two shapes are the two modes of §12.1.
///
/// `Keyed` is used when the family's payload carries a stable identifier the producer also puts on
/// the receipt. `Level` is the default and it is one slot per kind: without an identifier there is
/// no evidence of sameness, so the ledger stops pretending there is and uses the one decidable
/// fact it owns — the watermark.
///
/// **Red line 13**: the key is an association key inside one endpoint, never an address. An
/// endpoint is bound to one leaf when it is created, so a key cannot choose a pane, cannot cross to
/// another, and — as the trace vocabulary below shows — never leaves this process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WaitSlot {
    Keyed { kind: WaitKind, key: String },
    Level(WaitKind),
}

impl WaitSlot {
    pub(crate) fn kind(&self) -> WaitKind {
        match self {
            Self::Keyed { kind, .. } | Self::Level(kind) => *kind,
        }
    }
}

/// Whether an association key is one this endpoint will accept (`attention` plan §11.4.1).
///
/// Bounded and alphabet-restricted, for one reason each: a bound because it arrives from the far
/// end of a pipe, and an alphabet because a key that could contain a separator could be read back
/// as two fields by anything that ever printed it.
pub(crate) fn wait_key_is_well_formed(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_WAIT_KEY_BYTES
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

/// What one `clear` is aimed at.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ClearSelector {
    All,
    Kind(WaitKind),
    Key { kind: WaitKind, key: String },
}

/// **How much a `clear` is entitled to remove** (`attention` plan §13.1, the three-way split).
///
/// The question this answers is not "is it a receipt" but **"how badly could it remove the wrong
/// thing"**, and the three classes are the three answers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClearClass {
    /// A receipt. It removes what it names, and what it may name depends on the selector:
    ///
    /// * a **key** — it cannot reach any other credential, so it removes unconditionally. A late
    ///   duplicate is harmless: the credential it names is already gone, and clearing a key that is
    ///   not there is a no-op with no line;
    /// * a **kind** — it cannot say *which* credential ended, so it may remove only ones the user
    ///   has already answered. Letting it remove an unanswered one means a stale echo of the last
    ///   tool call erasing the request that is standing right now. **That is a swallowed request,
    ///   the class of failure this block exists to remove.**
    Receipt,
    /// You replied, the turn ended, the session ended, a timer fired. Removes everything it
    /// selects, answered or not — these are not receipts, they are the end of the story.
    Boundary,
    /// **The program itself ended the wait for one kind.** Removes that kind's credential
    /// unconditionally, and it must: an event of this class arrives precisely when *nobody
    /// answered* (the program resumed on its own, or stopped waiting), so a watermark gate would
    /// bar the one exit this kind has.
    ///
    /// Two conditions qualify a source, both declarable in the mapping table: the event's meaning
    /// is "the wait for this kind is over" rather than "the last thing finished", **and** the kind
    /// cannot have two concurrent waits in one session — so "clear the kind" and "clear that one"
    /// are the same act and there is no wrong thing left to remove.
    BoundaryKind,
}

/// Why a credential left, as the word that goes in the trace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClearReason {
    /// The program withdrew it over its own channel (`RequestAttention=no`).
    Program,
    /// An upstream hook said so.
    Hook,
    /// The program resumed or stopped waiting by itself.
    AutoResume,
    Ttl,
    SessionEnd,
    /// Evicted because this pane already held its bound.
    Overflow,
}

impl fmt::Display for ClearReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Program => "program",
            Self::Hook => "hook",
            Self::AutoResume => "auto-resume",
            Self::Ttl => "ttl",
            Self::SessionEnd => "session-end",
            Self::Overflow => "overflow",
        })
    }
}

impl ClearReason {
    /// The word a `downgrade` writes for the clear that caused it.
    ///
    /// A shorter vocabulary than `clear`'s own, and deliberately: a downgrade does not care which
    /// class of clear lowered the wording, only that one did (`attention` plan §13.1.5).
    fn downgrade_reason(self) -> &'static str {
        match self {
            Self::Program | Self::Hook | Self::AutoResume => "clear",
            Self::Ttl => "ttl",
            Self::SessionEnd => "session-end",
            Self::Overflow => "overflow",
        }
    }

    /// **Whether the far end is the one that said this** — the half of a clear that counts as an
    /// agent having spoken in this pane ([`AttentionLedger::is_agent_seat`]).
    ///
    /// Three of the six are the program's own sentence: it withdrew the request, its hook said the
    /// turn was over, or it went back to work. The other three are **this** side talking to itself —
    /// a timer that fired, a shell that ended, a bound this pane hit — and reading one of those as
    /// "an agent just spoke here" would let the seat renew itself out of its own expiry.
    fn is_the_programs_own(self) -> bool {
        match self {
            Self::Program | Self::Hook | Self::AutoResume => true,
            Self::Ttl | Self::SessionEnd | Self::Overflow => false,
        }
    }
}

/// **The kinds of user action that count as answering** (`attention` plan §11.3).
///
/// Six, and the seventh — a forwarded mouse *motion* — is not here at all. That absence is
/// structural rather than remembered: motion cannot be spelled as an answer, so a program that
/// turns on `?1003h` cannot have its request retired by a pointer crossing the pane. A defect of
/// that kind fires only on some programs and only while the mouse moves, which makes it about the
/// hardest thing there is to catch by hand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AnswerKind {
    Keyboard,
    /// An IME **commit**. Composition is not an answer: it puts no byte in the pipe.
    Ime,
    Paste,
    /// A path inserted into the shell from the files row.
    FilesRow,
    /// A forwarded press or release.
    MouseButton,
    /// A forwarded wheel. On the wire a wheel *is* a press (SGR button 64/65, with no release of
    /// its own), so "presses count" already covered it.
    MouseWheel,
}

impl fmt::Display for AnswerKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Keyboard => "keyboard",
            Self::Ime => "ime",
            Self::Paste => "paste",
            Self::FilesRow => "files-row",
            Self::MouseButton => "mouse-button",
            Self::MouseWheel => "mouse-wheel",
        })
    }
}

// ---------------------------------------------------------------------------
// The mapping table: four declared columns per row (§12.1 R1)
// ---------------------------------------------------------------------------

/// Whether a family's `kind` is running with identifiers or on a bare level (§12.1 R3).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Mode {
    /// Every wait row and every keyed receipt for this kind declares a field path, and they name
    /// the same namespace. Sameness is then the producer's evidence, which is stronger than ours.
    Id,
    /// **The default.** One slot per kind; sameness is decided by the watermark.
    Level,
}

/// Which layer of a family's signal is installed for one kind (§12.1 R2).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Tier {
    /// The zero-delay event.
    Primary,
    /// The delayed notification an older upstream is limited to.
    Fallback,
}

/// Where the identifier for one row lives, or that there is none.
///
/// `None` is not a failure and is the default. A path may be written **only** when the upstream
/// document has been quoted verbatim; guessing a field name here is how a kind ends up half in one
/// mode and half in the other, which is precisely the mixture §12.1 forbids.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IdSource {
    None,
    /// **No shipped row is one** (§12.1.6), which is the honest state of the evidence rather than a
    /// gap: a path may be written only from a verbatim quotation of an upstream payload, and none
    /// has been taken. The day one is, that is a data change in `attention_map` and nothing else —
    /// which is what `a_receipt_aims_as_narrowly_as_its_evidence_allows` exercises.
    #[allow(
        dead_code,
        reason = "§12.1.6: no upstream identifier has been quoted yet"
    )]
    Path(&'static str),
}

/// How wide a mapped `clear` aims.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClearScope {
    /// `--all`.
    All,
    /// This row's kind — narrowed to a key when the kind is in [`Mode::Id`] and the payload
    /// carries one.
    ThisKind,
}

/// What one mapped upstream event does.
///
/// A sum rather than a struct of options, so that "a wait row declares a tier and a clear row
/// declares a class" is a fact the type system holds rather than a rule a validator repeats.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MappedAction {
    Wait {
        tier: Tier,
    },
    Clear {
        class: ClearClass,
        scope: ClearScope,
        reason: ClearReason,
        /// Whether this event also means **you have started the next turn**, which is the one
        /// thing that re-arms the turn-end announcement. `UserPromptSubmit` is this; `Stop` is
        /// emphatically not — `Stop` *is* the turn end.
        begins_turn: bool,
    },
}

/// **One row of an adapter's mapping table — the unit in which a CLI is supported.**
///
/// Adding a family is adding rows. There is no per-family code, no `match` on a family name, and
/// nothing here that a second family could need a different version of: the verb, the kind, the
/// identifier and the tier or class are all *declared*, and the ledger below reads the declaration.
///
/// The columns are R1's four, and the fourth is the one that keeps being got wrong by inference:
/// **whether the family has a stable identifier is written down, not guessed at per message.**
/// A family that cannot prove one writes [`IdSource::None`], and `None` has a written-down
/// consequence rather than a shrug.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MappingRow {
    pub family: &'static str,
    /// The upstream event's own name, spelled the way upstream spells it.
    pub event: &'static str,
    pub kind: WaitKind,
    pub id: IdSource,
    pub action: MappedAction,
}

impl MappingRow {
    pub(crate) fn is_wait(&self) -> bool {
        matches!(self.action, MappedAction::Wait { .. })
    }

    fn is_keyed_receipt(&self) -> bool {
        matches!(
            self.action,
            MappedAction::Clear {
                class: ClearClass::Receipt,
                scope: ClearScope::ThisKind,
                ..
            }
        )
    }

    /// The slot a `wait` from this row occupies, given its kind's mode and whatever identifier the
    /// payload carried.
    pub(crate) fn slot(&self, mode: Mode, id: Option<&str>) -> WaitSlot {
        match (mode, id) {
            (Mode::Id, Some(key)) if wait_key_is_well_formed(key) => WaitSlot::Keyed {
                kind: self.kind,
                key: key.to_owned(),
            },
            _ => WaitSlot::Level(self.kind),
        }
    }
}

/// **R2 — one kind, one tier, in one installed configuration.**
///
/// The zero-delay event and the delayed notification describe the *same* request; installing both
/// is how one request becomes two credentials. Which one to install is a question for the program
/// itself (does this version have the event?), never for a version number we guessed at.
///
/// Answers the offending kind rather than a bool, because a table that fails this has to say where.
///
/// **Nothing in the running build calls it**, and that is what it is for: the shipped catalogue
/// deliberately holds both layers of `permission` so that `installed_rows` has something to choose
/// between, and this is the checker that proves the *installed* set never does. A rule whose only
/// witness is a test is still a rule; a rule with no witness at all is a comment.
#[allow(
    dead_code,
    reason = "R2's checker; its witness is the catalogue's own red form"
)]
pub(crate) fn duplicated_tier(rows: &[MappingRow]) -> Option<(&'static str, WaitKind)> {
    for row in rows.iter().filter(|row| row.is_wait()) {
        let MappedAction::Wait { tier } = row.action else {
            continue;
        };
        let clash = rows.iter().any(|other| {
            other.is_wait()
                && other.family == row.family
                && other.kind == row.kind
                && !matches!(other.action, MappedAction::Wait { tier: t } if t == tier)
        });
        if clash {
            return Some((row.family, row.kind));
        }
    }
    None
}

/// **R3 — a kind is in [`Mode::Id`] only if every row that would have to agree does.**
///
/// Every wait row *and* every keyed receipt for that kind must declare a path. One `IdSource::None`
/// puts the whole kind on the level path, because a kind that is half keyed and half not is exactly
/// the mixture that produces both failure directions at once: a stale fixed key swallowing the next
/// real request, and two layers of one request minting two credentials.
pub(crate) fn kind_mode(rows: &[MappingRow], family: &str, kind: WaitKind) -> Mode {
    let relevant = rows
        .iter()
        .filter(|row| row.family == family && row.kind == kind)
        .filter(|row| row.is_wait() || row.is_keyed_receipt());
    let mut any_wait = false;
    for row in relevant {
        any_wait |= row.is_wait();
        if matches!(row.id, IdSource::None) {
            return Mode::Level;
        }
    }
    if any_wait { Mode::Id } else { Mode::Level }
}

// ---------------------------------------------------------------------------
// Events and outcomes
// ---------------------------------------------------------------------------

/// One arrival at the ledger — the columns of `attention` plan §11.1.4's grid.
///
/// `StrongWait` is one variant and not two: whether an arrival is a fresh assertion or a
/// restatement is **decided from the ledger** by §12.1's R4, not declared by the caller. A caller
/// that could declare it would be a caller that could get it wrong.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Event {
    /// `bt-term` reports a weak generation this ledger has not seen. See
    /// [`AttentionLedger::weak_edge`], which is how a polled level becomes this.
    WeakYes(u64),
    /// `bt-term` reports the weak tier has been withdrawn.
    WeakNo,
    /// `folio attention wait` arrived for one slot.
    StrongWait(WaitSlot),
    /// `folio attention clear`, a timer, or an upstream boundary event.
    StrongClear {
        selector: ClearSelector,
        class: ClearClass,
        reason: ClearReason,
        /// See [`MappedAction::Clear::begins_turn`].
        begins_turn: bool,
    },
    /// One turn of the per-frame pass, carrying the two facts it decides from.
    Settle { active: bool, focused: bool },
    /// A user action with a source, in this very seat.
    Answer(AnswerKind),
    /// The pane is gone.
    LeafGone,
    /// The tab was switched to, and its one-shot latches were spent. **This ledger is untouched**
    /// (`attention` plan §10.9): a look spends a bell, and a standing request is not a bell.
    MarkSeen,
}

impl Event {
    /// **Whether this arrival is a program talking, rather than the window or the person**
    /// (`attention` plan §11.10.4, user ruling 乙, 2026-08-25).
    ///
    /// The seat criterion is "an agent has spoken in this pane", and the only place that question
    /// can be answered without getting it wrong is **on the enum** — the same shape and the same
    /// reason as `UserInputKind::is_answer()` one lane over: a path added later has to take a
    /// position here, in one `match` the compiler checks, rather than be remembered at whichever
    /// call site happens to construct it.
    ///
    /// The four that count are the two withdrawable tiers and their withdrawals — a program saying
    /// `RequestAttention=yes` or `=no` over its own tty, and a `folio attention wait` or its clear
    /// over the pane's pipe. `Settle` is this window's own heartbeat, `Answer` is the person,
    /// `LeafGone` is the pane dying and `MarkSeen` is a look; none of the four is anybody's voice
    /// but ours. A clear is asked one question further — see [`ClearReason::is_the_programs_own`].
    fn is_the_programs_voice(&self) -> bool {
        match self {
            Self::WeakYes(_) | Self::WeakNo | Self::StrongWait(_) => true,
            Self::StrongClear { reason, .. } => reason.is_the_programs_own(),
            Self::Settle { .. } | Self::Answer(_) | Self::LeafGone | Self::MarkSeen => false,
        }
    }
}

/// A desktop interruption the ledger has decided to allow, exactly once.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Raised {
    pub why: Why,
    pub reach: Reach,
    /// The place in the queue. `None` for a turn end, which takes none.
    pub ticket: Option<u64>,
    /// The request. `None` for a turn end, which is not one.
    pub episode: Option<u64>,
    /// **The program's own words, when it wrote any** (`attention` plan §11.6 rule 2).
    ///
    /// An event-level announcement carries text and the ledger does not; when one arrives while a
    /// request of this pane is standing un-interrupted-about, its sentence is the one this
    /// interruption should say, because *a program's own words beat words we composed*. `None` is
    /// the ordinary case and means the caller says it in its own voice.
    ///
    /// **Borrowed, never promoted.** Lending a sentence is the only thing the announcement did: it
    /// minted no episode, took no place and moved no grounds, and if it had been allowed to do any
    /// of those this field would be the seam an event crawled through to become a state.
    pub body: Option<String>,
}

/// Which of the two doors a desktop interruption came through.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Why {
    /// The queue's door: a held place whose grounds are `AwaitingInput`.
    Awaiting,
    /// The event door: a turn ended. It mints nothing and queues nothing.
    TurnEnd,
}

impl fmt::Display for Why {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Awaiting => "awaiting",
            Self::TurnEnd => "turn-end",
        })
    }
}

/// What one arrival decided.
///
/// **Lines are returned rather than written.** The ledger has no opinion about files, a test can
/// assert on the exact bytes of the schema, and the caller does the one `emit` per line. Nothing is
/// formatted on the frames where nothing was decided, and those are almost all of them — the
/// vector is empty and empty vectors do not allocate.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Outcome {
    pub lines: Vec<String>,
    pub raised: Option<Raised>,
}

// ---------------------------------------------------------------------------
// The ledger
// ---------------------------------------------------------------------------

/// **One pane's attention account** (`attention` plan §11.1.1 and §12.2.1).
///
/// Five cursors and they only ever go up; the live values beside them are cleared, replaced and
/// dropped freely. **The cleanup paths and the minting paths never touch the same field**, which is
/// what carries "a generation is never reused" through a withdrawal, and what lets a `mint` line
/// point back at the previous episode across however many times the account fell idle in between.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AttentionLedger {
    /// This ledger's mirror of `bt-term`'s live weak generation; `0` is "not asserting".
    weak_gen: u64,
    /// One live generation per outstanding strong credential.
    strong: Vec<(WaitSlot, u64)>,
    /// Cursor. Never wound back by a clear, a timer, or a re-assertion.
    next_strong_gen: u64,
    /// The live episode, or `None` when the account is idle.
    episode: Option<u64>,
    /// The last episode minted. **A drop does not clear it**, which is what a `mint`'s `prev=` is
    /// read from and what a `claim` line uses to name the request whose dot just went out.
    last_episode: Option<u64>,
    /// Cursor.
    next_episode: u64,
    /// The place in the window's queue, when one is held. This is the field the existing queue
    /// already keeps; the ledger holds its own so that A-core owes the running build nothing.
    ticket: Option<u64>,
    /// Watermarks. `0` is "never answered anything".
    acked_weak: u64,
    acked_strong: u64,
    /// Whether this episode has spent its one desktop interruption. Lives and dies with the
    /// episode, which is what makes "at most once per request" a property rather than a promise.
    toasted: bool,
    /// Whether this episode has already been refused for being watched.
    ///
    /// Same shape as `toasted` and the same reason. The per-frame pass asks the same question sixty
    /// times a second, and a program holding a request up while you look at its tab would otherwise
    /// write a `refuse` line on every one of those frames — the flood the "one line per decision"
    /// rule exists to prevent. A refusal is a decision; **being still refused is not**.
    refused: bool,
    /// **A sentence an event-level announcement left for this episode's one interruption**
    /// (`attention` plan §11.6 rule 2), or `None` when no program has written one.
    ///
    /// Lives and dies with the episode, and there is one place an episode begins — [`Self::mint`] —
    /// so there is one place this is cleared. Words left over from a request that is finished with
    /// would otherwise be spoken about the next one, which is a toast quoting a program about
    /// something it did not say that about.
    lent: Option<String>,
    /// Whether this **turn**'s ending has already been decided about. Belongs to no episode,
    /// because a turn ending mints none (`attention` plan §13.3).
    announced_turn_end: bool,
    /// **The sentence the last accepted turn-end decision carried**, or `None` when it carried
    /// none.
    ///
    /// Lives and dies with [`Self::announced_turn_end`], and it is what keeps that bit from
    /// swallowing something it was never about — see [`Self::announce_turn_end`] for the whole of
    /// why a wordless second source is the same fact and a second *sentence* is not.
    announced_words: Option<String>,
    /// **When a program last spoke in this pane**, or `None` when none ever has (`attention` plan
    /// §11.10.4, user ruling 乙, 2026-08-25).
    ///
    /// Stamped in [`Self::apply`] for every arrival [`Event::is_the_programs_voice`] admits, and
    /// nowhere else — one place, so that a lane added later cannot forget it. It is the *only*
    /// field here that is a fact about wall-clock time, and it is a stamp rather than a countdown
    /// because a countdown would have to be driven, and a ledger nobody drives would answer this
    /// question with whatever it was told last.
    ///
    /// **A drop does not clear it**, exactly as `last_episode` is not cleared: the pane goes on
    /// being the pane an agent was working in after the agent stops asking for anything, and that
    /// is the whole content of [`Self::is_agent_seat`]'s second arm.
    spoke_at: Option<Instant>,
}

impl Default for AttentionLedger {
    fn default() -> Self {
        Self {
            weak_gen: 0,
            strong: Vec::new(),
            next_strong_gen: 1,
            episode: None,
            last_episode: None,
            next_episode: 1,
            ticket: None,
            acked_weak: 0,
            acked_strong: 0,
            toasted: false,
            refused: false,
            lent: None,
            announced_turn_end: false,
            announced_words: None,
            spoke_at: None,
        }
    }
}

/// A snapshot taken before an arrival mutates anything, so the lines can describe a *change*.
#[derive(Clone, Copy)]
struct Before {
    asking: bool,
    grounds: Grounds,
    ticket: Option<u64>,
    episode: Option<u64>,
}

impl AttentionLedger {
    // -- derived facts (§11.1.2) --------------------------------------------

    fn strong_gen(&self) -> u64 {
        self.strong
            .iter()
            .map(|(_, generation)| *generation)
            .max()
            .unwrap_or(0)
    }

    fn unanswered_weak(&self) -> bool {
        self.weak_gen > self.acked_weak
    }

    fn unanswered_strong(&self) -> bool {
        self.strong_gen() > self.acked_strong
    }

    /// The first argument of the door into the queue.
    fn asking(&self) -> bool {
        self.unanswered_weak() || self.unanswered_strong()
    }

    fn any_asserted(&self) -> bool {
        self.weak_gen != 0 || self.strong_gen() != 0
    }

    /// See [`Grounds`] — an instant function of the strong tier, never a latch.
    pub(crate) fn grounds(&self) -> Grounds {
        if self.unanswered_strong() {
            Grounds::AwaitingInput
        } else {
            Grounds::Requested
        }
    }

    /// [`State`], derived — see that type for why the window does not ask for it.
    #[allow(
        dead_code,
        reason = "the derived state is what the grid's cells are spelled against"
    )]
    pub(crate) fn state(&self) -> State {
        let Some(episode) = self.episode else {
            return State::Idle;
        };
        if self.asking() {
            match self.ticket {
                Some(ticket) => State::Queued { episode, ticket },
                None => State::Requested(episode),
            }
        } else {
            State::Acknowledged(episode)
        }
    }

    /// The place this pane holds, for the caller that draws the queue.
    pub(crate) fn ticket(&self) -> Option<u64> {
        self.ticket
    }

    /// **Whether an agent sits in this pane** — the one criterion, and it is the ledger's
    /// (`attention` plan §11.10.4, **user ruling 乙, 2026-08-25**).
    ///
    /// The question the eighth mark `Attached` asks, and the plan left two candidate answers for a
    /// person to choose between. **甲** was `alternate_screen == true`: honest, free, and a fact
    /// about the screen rather than about who is using it. **乙** is this — the pane's own
    /// attention signals. The user ruled 乙 on an observation the survey had already measured and
    /// the plan had not drawn the consequence from: `codex` runs **inline**, on the main screen,
    /// and never sets DECSET 1049 at all (survey §2.5 — `?1049` hits are zero across four
    /// recordings). Under 甲 a `codex` the reader is looking straight at answers "no full-screen
    /// program here", and every rule downstream of the mark inherits that mistake.
    ///
    /// **So the alternate screen is not a criterion any more**, and the substitution is not a
    /// widening of 甲 (which is what §11.10.4's 乙 row proposed — `alt-screen` *or* a live
    /// endpoint). It is a replacement: a screen mode says which buffer the bytes are landing in,
    /// and this asks whether anything in this pane has ever spoken the language a request is
    /// written in. The two are independent, and keeping the first as an `or` would have kept
    /// exactly the class of answer the ruling was made to remove.
    ///
    /// **Two arms, and the second is why there is a clock in this at all.**
    ///
    /// * A live credential — [`Self::any_asserted`], which is true while either tier is asserting
    ///   and which the pane's `WaitClock` already ages out at [`WAIT_TTL`]. A pane that is holding
    ///   somebody up right now is beyond argument an agent's seat.
    /// * A signal inside [`WAIT_TTL`] of now. An agent that has answered and gone quiet is still
    ///   the thing sitting in that pane, and a criterion that flickered off between one request and
    ///   the next would be a mark that blinked once per turn.
    ///
    /// **The same ten minutes as a standing credential, on purpose**: the number means "how long a
    /// thing this producer said is still current", and it would be a second number to keep in step
    /// for no second reason. **An announcement is deliberately not an arm** — red line 14 says an
    /// event-level arrival mints nothing and holds nothing, and "there is an agent in this pane" is
    /// as much a state as any other; a bare `BEL` from `make` establishing a seat would be that red
    /// line broken by the one door built to respect it.
    #[allow(
        dead_code,
        reason = "the eighth mark `Attached` is §7.1.5b's own row and is not built; the ruling \
                  it is built from is, and its witness is the three cells below"
    )]
    pub(crate) fn is_agent_seat(&self, now: Instant) -> bool {
        self.any_asserted()
            || self
                .spoke_at
                .is_some_and(|when| now.saturating_duration_since(when) <= WAIT_TTL)
    }

    /// The episode a `claim` line names (`attention` plan §13.2.1).
    ///
    /// Three branches and **the third is the common one**: a claim line is drawn from the whole
    /// projection, and most changes to it — unread, bell, running — belong to no request at all.
    /// `None` renders as `-`, the same way a first episode's `prev=` does. Saying `-` honestly is
    /// what lets the "every line names its request" contract have no exception, and an exception is
    /// what would make every reader of the file pause on every line to remember which one it was.
    pub(crate) fn claim_episode(&self, was_attention: bool, now_attention: bool) -> Option<u64> {
        if now_attention {
            self.episode
        } else if was_attention {
            self.last_episode
        } else {
            None
        }
    }

    /// Turn `bt-term`'s polled weak level into at most one event.
    ///
    /// The weak tier is a *level* on a status snapshot and this machine takes *edges*, so the
    /// translation has to happen somewhere; here, next to the mirror it compares against, rather
    /// than at a call site that would have to keep its own copy.
    pub(crate) fn weak_edge(&self, reported: Option<u64>) -> Option<Event> {
        match reported {
            Some(generation) if generation != self.weak_gen => Some(Event::WeakYes(generation)),
            Some(_) => None,
            None if self.weak_gen != 0 => Some(Event::WeakNo),
            None => None,
        }
    }

    // -- the arrival grid ---------------------------------------------------

    /// **One arrival, one answer** (`attention` plan §11.1.4).
    ///
    /// Every arm follows the same shape: change the generations and the watermarks, let the state
    /// fall out of §11.1.2's derivation, and describe the change. Nothing recomputes a state it
    /// stored, because none is stored.
    ///
    /// `next_ticket` is the window's serial and is handed in rather than held, because places are
    /// ordered across the whole window and a per-pane counter would order nothing. `now` is handed
    /// in for the same reason every other fact about the frame is (see this module's header): the
    /// ledger reads no clock of its own, and the one thing it does with the instant is stamp the
    /// arrivals a program made, so that [`Self::is_agent_seat`] can answer without a caller having
    /// to remember to tell it.
    pub(crate) fn apply(
        &mut self,
        at: Site,
        reach: Reach,
        event: Event,
        next_ticket: &mut u64,
        now: Instant,
    ) -> Outcome {
        let mut out = Outcome::default();
        // **Before the arm and not inside one of them.** Every lane that can carry a program's
        // voice is one `match` away from being added, and a stamp written in four arms is a stamp
        // the fifth forgets.
        if event.is_the_programs_voice() {
            self.spoke_at = Some(now);
        }
        match event {
            Event::WeakYes(generation) => self.weak_yes(at, generation, &mut out),
            Event::WeakNo => self.weak_no(at, &mut out),
            Event::StrongWait(slot) => self.strong_wait(at, slot, &mut out),
            Event::StrongClear {
                selector,
                class,
                reason,
                begins_turn,
            } => self.strong_clear(at, &selector, class, reason, begins_turn, &mut out),
            Event::Settle { active, focused } => {
                self.settle(at, active, focused, next_ticket, &mut out)
            }
            Event::Answer(by) => self.answer(at, by, &mut out),
            Event::LeafGone => self.leaf_gone(at, &mut out),
            // The one arrival with nothing to say. A look spends latches, and this ledger holds
            // none: the request is the program's sentence and only the program or an answer ends it.
            Event::MarkSeen => {}
        }
        self.toast_gate(at, reach, &mut out);
        out
    }

    fn before(&self) -> Before {
        Before {
            asking: self.asking(),
            grounds: self.grounds(),
            ticket: self.ticket,
            episode: self.episode,
        }
    }

    fn weak_yes(&mut self, at: Site, generation: u64, out: &mut Outcome) {
        if generation == self.weak_gen {
            return;
        }
        let before = self.before();
        self.weak_gen = generation;
        if !before.asking && self.asking() {
            self.mint(at, Transport::Osc, generation, out);
        }
    }

    fn weak_no(&mut self, at: Site, out: &mut Outcome) {
        if self.weak_gen == 0 {
            return;
        }
        let before = self.before();
        let generation = self.weak_gen;
        self.weak_gen = 0;
        self.credentials_left(
            at,
            Transport::Osc,
            &[generation],
            ClearReason::Program,
            before,
            out,
        );
    }

    /// §12.1's R4 in one place: whether this arrival is a fresh assertion or a restatement.
    ///
    /// The two modes ask different questions and both answers are decidable here. With an
    /// identifier the producer's evidence is stronger than ours and is taken as given — the same id
    /// is the same request, answered or not. Without one the watermark is all there is, and it is
    /// enough: **a credential that goes up again after you have answered can only be a new thing.**
    fn raises(&self, slot: &WaitSlot) -> bool {
        match slot {
            WaitSlot::Keyed { .. } => !self.strong.iter().any(|(held, _)| held == slot),
            WaitSlot::Level(_) => self
                .strong
                .iter()
                .find(|(held, _)| held == slot)
                .is_none_or(|(_, generation)| *generation <= self.acked_strong),
        }
    }

    fn strong_wait(&mut self, at: Site, slot: WaitSlot, out: &mut Outcome) {
        if !self.raises(&slot) {
            return;
        }
        let before = self.before();
        let generation = self.next_strong_gen;
        self.next_strong_gen += 1;
        match self.strong.iter_mut().find(|(held, _)| *held == slot) {
            // A level that is going up again takes its own slot back rather than a second one:
            // one kind is one wait, and the bound below is therefore unreachable without keys.
            Some(entry) => entry.1 = generation,
            None => self.strong.push((slot, generation)),
        }
        match before.episode.filter(|_| before.asking) {
            Some(episode) => out.lines.push(format!(
                "upgrade {at} {}episode={episode} grounds=awaiting src=pipe gen={generation}",
                held(self.ticket),
            )),
            None => self.mint(at, Transport::Pipe, generation, out),
        }
        self.evict_overflow(at, out);
    }

    /// Hold this pane to its bound, oldest first (`attention` plan §11.4.1).
    fn evict_overflow(&mut self, at: Site, out: &mut Outcome) {
        while self.strong.len() > MAX_OUTSTANDING_WAITS {
            let Some(index) = self
                .strong
                .iter()
                .enumerate()
                .min_by_key(|(_, (_, generation))| *generation)
                .map(|(index, _)| index)
            else {
                return;
            };
            let before = self.before();
            let (_, generation) = self.strong.remove(index);
            self.credentials_left(
                at,
                Transport::Pipe,
                &[generation],
                ClearReason::Overflow,
                before,
                out,
            );
        }
    }

    fn strong_clear(
        &mut self,
        at: Site,
        selector: &ClearSelector,
        class: ClearClass,
        reason: ClearReason,
        begins_turn: bool,
        out: &mut Outcome,
    ) {
        if begins_turn {
            self.rearm_turn_end();
        }
        let acked = self.acked_strong;
        let removes = |slot: &WaitSlot, generation: u64| -> bool {
            let selected = match selector {
                ClearSelector::All => true,
                ClearSelector::Kind(kind) => slot.kind() == *kind,
                ClearSelector::Key { kind, key } => {
                    slot == &WaitSlot::Keyed {
                        kind: *kind,
                        key: key.clone(),
                    }
                }
            };
            if !selected {
                return false;
            }
            match class {
                ClearClass::Boundary | ClearClass::BoundaryKind => true,
                // The gate, and it guards exactly one of the three shapes: a receipt that can only
                // name a kind. One that names a key removes what it names and nothing else.
                ClearClass::Receipt => match selector {
                    ClearSelector::Key { .. } => true,
                    ClearSelector::All | ClearSelector::Kind(_) => generation <= acked,
                },
            }
        };
        let removed = self
            .strong
            .iter()
            .filter(|(slot, generation)| removes(slot, *generation))
            .map(|(_, generation)| *generation)
            .collect::<Vec<_>>();
        if removed.is_empty() {
            return;
        }
        let before = self.before();
        self.strong
            .retain(|(slot, generation)| !removes(slot, *generation));
        self.credentials_left(at, Transport::Pipe, &removed, reason, before, out);
    }

    /// **What one or more credentials leaving means for the episode that held them.**
    ///
    /// Three shapes, and which one is written depends on whether a place was held — that is the
    /// whole of `attention` plan §11.1.5's field contract, and the reason it exists is that
    /// `withdraw` and `expire` carry a `ticket=` that two of the four states do not have.
    ///
    /// * still asking — a `clear` per credential, plus a `downgrade` if the wording fell back;
    /// * asking has stopped, a place was held — one `withdraw`, and the place goes;
    /// * asking has stopped, no place — the `clear`s, plus a `drop` if the account has fallen idle.
    fn credentials_left(
        &mut self,
        at: Site,
        source: Transport,
        removed: &[u64],
        reason: ClearReason,
        before: Before,
        out: &mut Outcome,
    ) {
        let Some(episode) = before.episode else {
            return;
        };
        if let Some(ticket) = before.ticket.filter(|_| !self.asking()) {
            self.ticket = None;
            out.lines.push(format!(
                "withdraw {at} ticket={ticket} episode={episode} reason=program src={source}"
            ));
        } else {
            for generation in removed {
                out.lines.push(format!(
                    "clear {at} episode={episode} src={source} gen={generation} reason={reason}"
                ));
            }
            if self.asking()
                && before.grounds == Grounds::AwaitingInput
                && self.grounds() == Grounds::Requested
            {
                out.lines.push(format!(
                    "downgrade {at} {}episode={episode} grounds=requested src=pipe reason={}",
                    held(self.ticket),
                    reason.downgrade_reason(),
                ));
            }
            if !self.any_asserted() {
                out.lines
                    .push(format!("drop {at} episode={episode} reason={reason}"));
            }
        }
        if !self.any_asserted() {
            self.episode = None;
        }
    }

    fn settle(
        &mut self,
        at: Site,
        active: bool,
        focused: bool,
        next_ticket: &mut u64,
        out: &mut Outcome,
    ) {
        let Some(episode) = self.episode else {
            return;
        };
        // Acknowledged is a fixed point here, and that is the single property a bit could not
        // give: the program is still asserting, and the pass does not hand out a fresh place for
        // something the user has already dealt with.
        if !self.asking() || self.ticket.is_some() {
            return;
        }
        if active && focused {
            if !self.refused {
                self.refused = true;
                out.lines.push(format!(
                    "refuse {at} episode={episode} reason=watched active=1 focused=1"
                ));
            }
            return;
        }
        let ticket = *next_ticket;
        *next_ticket += 1;
        self.ticket = Some(ticket);
        out.lines.push(format!(
            "admit {at} ticket={ticket} episode={episode} grounds={} active={} focused={}",
            self.grounds(),
            u8::from(active),
            u8::from(focused),
        ));
    }

    /// **Answering answers everything that is on the table.**
    ///
    /// Both watermarks move to both current generations, because "I answered" is not a statement
    /// about one credential — the user saw one pane and dealt with what it was asking. That is why
    /// a watermark is enough and a set of answered ids is not needed.
    fn answer(&mut self, at: Site, by: AnswerKind, out: &mut Outcome) {
        // A reply is the start of your next turn, whatever else it is.
        self.rearm_turn_end();
        let Some(episode) = self.episode else {
            return;
        };
        if !self.asking() {
            return;
        }
        self.acked_weak = self.weak_gen;
        self.acked_strong = self.strong_gen();
        let ticket = held(self.ticket.take());
        out.lines.push(format!(
            "answer {at} {ticket}episode={episode} by={by} weak={} strong={}",
            self.acked_weak, self.acked_strong,
        ));
    }

    fn leaf_gone(&mut self, at: Site, out: &mut Outcome) {
        let Some(episode) = self.episode else {
            return;
        };
        match self.ticket.take() {
            Some(ticket) => out.lines.push(format!(
                "expire {at} ticket={ticket} episode={episode} reason=leaf-gone"
            )),
            None => out
                .lines
                .push(format!("drop {at} episode={episode} reason=leaf-gone")),
        }
        self.episode = None;
        self.weak_gen = 0;
        self.strong.clear();
        // The window's serial is not wound back, here or anywhere: a place that is gone was still
        // handed out, and reusing its number would make two different waits indistinguishable in
        // the one file that is supposed to tell them apart.
    }

    /// **The place this pane held in a window it has just left** (`attention` plan §4 B4).
    ///
    /// A tear-out carries the shell, its credentials and its episode into another window and
    /// leaves the *place* behind — 号不跨窗, and the reason is arithmetic rather than policy:
    /// places are ordered by a serial each window owns, so a number carried across is a number two
    /// panes could hold at once and the walk over "who has been waiting longest" would have two
    /// answers. The pane goes on asking, so the first pass in the window it landed in admits it
    /// there, with that window's own next serial.
    ///
    /// **Nothing is wound back**, for [`Self::leaf_gone`]'s reason exactly, and nothing else is
    /// touched: the credentials, the episode and its spent interruption all belong to the program
    /// and the person, neither of whom did anything by dragging a tab.
    pub(crate) fn surrender_place(&mut self, at: Site) -> Outcome {
        let mut out = Outcome::default();
        if let (Some(ticket), Some(episode)) = (self.ticket.take(), self.episode) {
            out.lines.push(format!(
                "expire {at} ticket={ticket} episode={episode} reason=torn-out"
            ));
        }
        out
    }

    fn mint(&mut self, at: Site, source: Transport, generation: u64, out: &mut Outcome) {
        let previous = self.last_episode;
        let episode = self.next_episode;
        self.next_episode += 1;
        self.last_episode = Some(episode);
        self.episode = Some(episode);
        self.toasted = false;
        self.refused = false;
        // A new request borrows nothing. Words a program wrote about the last one would otherwise
        // be spoken about this one, which is this terminal quoting a program out of context.
        self.lent = None;
        out.lines.push(format!(
            "mint {at} episode={episode} src={source} gen={generation} grounds={} prev={}",
            self.grounds(),
            previous.map_or_else(|| "-".to_owned(), |episode| episode.to_string()),
        ));
    }

    /// **The one door a queued request takes to the desktop** (`attention` plan §11.2, red line 12).
    ///
    /// A *level* rather than an edge, evaluated after every arrival, and that is the fix for the
    /// hole an edge left: a weak credential that is admitted first (no interruption, correctly)
    /// and confirmed by a strong one six seconds later would, on an admit-only edge, never
    /// interrupt at all — and that late confirmation is the exact moment the pane really is
    /// waiting for you.
    ///
    /// Once per episode, and the three sources of that are worth naming: `toasted` lives and dies
    /// with the episode, a repeated fall and rise of the wording does not clear it, and a repeated
    /// credential is a restatement that gets no further than [`Self::raises`].
    ///
    /// **`Reach::Nothing` still spends it.** Deciding not to interrupt is a decision, made from the
    /// facts of that moment; leaving the door open so a later frame can interrupt about the same
    /// request is what "queue it and deliver it when they look away" means, and that is the thing
    /// notifications here are explicitly not allowed to do.
    fn toast_gate(&mut self, at: Site, reach: Reach, out: &mut Outcome) {
        let (Some(episode), Some(ticket)) = (self.episode, self.ticket) else {
            return;
        };
        if self.toasted || self.grounds() != Grounds::AwaitingInput {
            return;
        }
        self.toasted = true;
        out.lines.push(format!(
            "toast {at} why=awaiting ticket={ticket} episode={episode} reach={reach}"
        ));
        out.raised = Some(Raised {
            why: Why::Awaiting,
            reach,
            ticket: Some(ticket),
            episode: Some(episode),
            // Taken rather than copied: the sentence was lent for *this* interruption, and this
            // request gets one. Leaving it behind would be leaving something for a second delivery
            // that is never going to be allowed to happen.
            body: self.lent.take(),
        });
    }

    /// **One event-level announcement arrived** (`attention` plan §11.6) — and this is all of what
    /// it is allowed to do.
    ///
    /// `OSC 9;<text>`, `OSC 777;notify`, `OSC 99`, `RequestAttention=once`, a bare bell: every one
    /// of them is a sentence with no *off*, so none of them mints an episode, takes a place, or
    /// moves the grounds of one that exists. **Rule 1 is therefore a method that returns nothing and
    /// writes no line** — a decision was not made here, and a file that recorded one would be
    /// recording a thing the ledger did not do.
    ///
    /// What it may do is **lend its words** (rule 2). A program that wrote "Allow Bash to run
    /// `rm -rf`?" has said the sentence this pane's standing request deserves far better than
    /// anything composed from a pane name, so if a request of this pane is live and has not spent
    /// its one interruption yet, that sentence is kept for it. Two conditions and both are the
    /// gate's own: **no live episode** means there is nothing for the words to be about, and
    /// **already interrupted about** means the sentence would have to be delivered a second time,
    /// which is the replay red line 5 forbids.
    ///
    /// The last words win. A program that says two things before anyone is interrupted has changed
    /// what it is saying, and the older sentence is the one that is out of date.
    ///
    /// # What the answer is for
    ///
    /// `true` means **this pane's live request has taken responsibility for this announcement**,
    /// and the caller's event door is therefore not to raise a second interruption about it. §11.6
    /// pin ② is explicit that a message arriving on a pane with a live, un-interrupted-about
    /// request produces the program's words **once** — so the words go to exactly one place, and
    /// the place they go to is the request, because a standing request is the stronger claim and
    /// the one a reader can still act on.
    ///
    /// `false` is a pane with nothing standing, or one whose request has already spent its single
    /// interruption. Either way there is no request for the announcement to be folded into, and it
    /// is an event of its own — which is precisely what the event lane is.
    ///
    /// **The answer does not depend on whether there were words.** A message with an empty body on
    /// a pane that is already asking is still that request's business; taking the branch on the
    /// text would make an empty `OSC 9` raise a second interruption that a non-empty one does not.
    pub(crate) fn announce(&mut self, words: Option<&str>) -> bool {
        if self.episode.is_none() || self.toasted {
            return false;
        }
        if let Some(words) = words.map(str::trim).filter(|words| !words.is_empty()) {
            self.lent = Some(words.to_owned());
        }
        true
    }

    /// **A turn ended** (`attention` plan §11.7 and §13.3) — the event door, which mints nothing.
    ///
    /// Four sources say this same thing and any two of them can arrive together, so it is decided
    /// **once**: the first accepted arrival settles it, and every later one for the same turn is
    /// silent. What re-arms it is your next turn — a submitted prompt, or any answer — and never a
    /// clock, because "have you started talking again" is a fact this build already holds and a
    /// millisecond window is a guess that is wrong on slow machines in one direction and on fast
    /// ones in the other.
    ///
    /// **A `Nothing` counts as accepted**, and that is the whole of §13.3: with the window focused
    /// the answer "do not interrupt" is a decision about this turn's end, not a deferral of it. If
    /// it did not count, walking away afterwards would produce a flash about a turn that ended
    /// while you were watching — a notification queued and delivered late, which is exactly what is
    /// forbidden.
    ///
    /// With the setting off the whole door is shut: no evaluation, no line, and **no bit set**, so
    /// that turning it off and on again cannot leave half a state behind.
    ///
    /// # What the bit is allowed to swallow, and what it is not
    ///
    /// §13.3 sets the bit on the first *accepted decision* so that a second source cannot deliver
    /// a late flash about a turn that ended while you were watching. The sources it was written
    /// about say nothing but "the turn ended" — a hook `Stop`, a bare bell, an `OSC 1337;…=once`
    /// — and two of those in one turn are **the same fact arriving twice**, which is precisely
    /// what deduplication is for.
    ///
    /// **An arrival carrying the program's own sentence is not that.** `OSC 9;<text>`,
    /// `OSC 777;notify` and `OSC 99` are messages: they have words, and §11.6 rule 2 already
    /// establishes that a program's own words are the one thing this terminal must not throw away.
    /// A bit that swallowed the second of two *different* sentences would be this terminal deciding
    /// that a build finishing and a deploy finishing are one event because nobody pressed a key in
    /// between — and it would silently take away a delivery this product has always made. So the
    /// rule is stated on the decision rather than on the arrival: **a later arrival for the same
    /// turn is silent unless it says something the last accepted decision did not.** A restatement
    /// of the same sentence is still the same fact and is still swallowed.
    ///
    /// `words` is the program's own sentence, or `None` for the sources that have none.
    pub(crate) fn announce_turn_end(
        &mut self,
        at: Site,
        reach: Reach,
        enabled: bool,
        source: Transport,
        via: Via,
        words: Option<&str>,
    ) -> Outcome {
        let mut out = Outcome::default();
        // Trimmed before anything reads it, and once: the same sentence with a stray space is the
        // same sentence, and a comparison made against the untrimmed spelling would let a program
        // interrupt twice by printing its message a second time with a newline in front of it.
        // It is also the form the reader sees — a toast body that begins with three spaces is a
        // toast that looks broken.
        let words = words.map(str::trim).filter(|words| !words.is_empty());
        if !enabled {
            return out;
        }
        if self.announced_turn_end && words.is_none_or(|new| Some(new) == self.announced_words()) {
            return out;
        }
        self.announced_turn_end = true;
        self.announced_words = words.map(str::to_owned);
        out.lines.push(format!(
            "toast {at} why=turn-end episode=- reach={reach} src={source} via={via}"
        ));
        out.raised = Some(Raised {
            why: Why::TurnEnd,
            reach,
            ticket: None,
            episode: None,
            // **The arrival's own words, and never the lent ones.** What [`Self::lent`] holds
            // belongs to one request (§11.6 rule 2) and a turn ending is not one — red line 14, and
            // borrowing across that line is how an event would crawl into being a state. What is
            // carried here is the sentence *this* arrival brought, which is the case §11.7's
            // wording clause is about: a program that wrote a sentence has said this better than
            // anything composed from a pane name. `None` means the caller says it in its own voice.
            body: words.map(str::to_owned),
        });
        out
    }

    fn announced_words(&self) -> Option<&str> {
        self.announced_words.as_deref()
    }

    /// **Your next turn has begun**, so the one before it may be announced again.
    ///
    /// The two together and never one of them: a bit saying "already decided" beside the sentence
    /// that decision carried is one fact in two fields, and clearing half of it would leave a
    /// sentence from a finished turn able to silence an identical one in the next.
    fn rearm_turn_end(&mut self) {
        self.announced_turn_end = false;
        self.announced_words = None;
    }
}

/// The `ticket=<t> ` a line carries when a place is held, and nothing at all when none is.
///
/// The first clause of `attention` plan §11.1.5's field contract, written once: `upgrade`,
/// `downgrade` and `answer` all happen in two of the four states, and only one of those two has a
/// place to name. Three copies of this would be three chances for one of them to print
/// `ticket=0` for a pane that holds nothing — and `0` is a real serial.
fn held(ticket: Option<u64>) -> String {
    ticket.map_or_else(String::new, |ticket| format!("ticket={ticket} "))
}

/// Render one `claim` line (`attention` plan §11.1.5, §13.2.1).
///
/// A free function because a claim is about a **tab** — the loudest of the claims its panes make —
/// while a ledger is about one pane. The episode comes from
/// [`AttentionLedger::claim_episode`] on whichever pane's claim won.
pub(crate) fn claim_line(tab: usize, episode: Option<u64>, was: &str, now: &str) -> String {
    format!(
        "claim tab={tab} episode={} was={was} now={now}",
        episode.map_or_else(|| "-".to_owned(), |episode| episode.to_string())
    )
}

#[cfg(test)]
mod tests;
