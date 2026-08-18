//! **The command marks rail** — the column of ticks down a terminal pane's right
//! edge, one per command the shell reported (`design/ui-mockup.html` 1346-1383
//! and 4603-4707; `docs/DESIGN.md` §7.1.5c).
//!
//! # What it is, in the one sentence the ruling gives it
//!
//! *"Codex-style (user ruling 2026-07-18): an ORDINAL list, **not a minimap** —
//! evenly spaced ticks in a block centred on the pane's right edge."* Everything
//! awkward about this file follows from that sentence and from nothing else. The
//! ticks are **not** at the height their commands are at: position carries
//! *order*, and the ordinal stack is oldest at the top, which is why nothing here
//! ever consults a scroll offset or a projected y. A minimap would have to be
//! rebuilt whenever the viewport moved; this has to be rebuilt when the ledger
//! changes and when the pane is resized, and those are the only two, which is
//! what [`RailKey`] exists to state.
//!
//! # Why the lane, and why the number is not in this file
//!
//! [`bt_render::TERMINAL_SCROLL_LANE_LOGICAL_PX`] carries the whole argument: the
//! stylesheet's own comment beside `right: 11px` is an accident report — *"the
//! rail and the thumb are different instruments and may not share a lane (user
//! report 2026-07-18 — ticks sat on top of the thumb)"* — and eleven is that
//! lane's eight plus a three-pixel gap. Deriving the inset here rather than
//! writing `11.0` is what makes the accident unrepeatable when the terminal's own
//! scroll bar lands: there is one number, and moving it moves both instruments.
//!
//! # The pointer, and why it is a *curve* and not a hit test (S2)
//!
//! Four things arrive together because they are one gesture. The rail colours as
//! a whole (`hot` is a rail-level class, mock 1371 — not something a tick wears);
//! the tick nearest the pointer's *ordinal* becomes the crest and its neighbours
//! grow along a cosine skirt (mock 8452-8457); a bucket the pointer lands on
//! unfolds into its members with the neighbours compressing to keep the block's
//! extent (mock 8438-8451); and a card appears beside the crest saying what the
//! command was (`#cmd-peek`, mock 8465-8477). None of them is separable: the card
//! names the tick the crest picked, the crest is picked in the geometry the
//! fisheye produced, and all three are only ever true while the rail is hot.
//!
//! **Selection first, curve second** is the ruling that shapes the code (five
//! rounds of errata, 2026-07-18): the bell is a function of the selected
//! *ordinal* and never of pointer pixels, so at every instant there is exactly
//! one peak and a pointer resting on the exact midpoint between two ticks lights
//! one of them fully rather than both at half height.
//!
//! # One rail, two sources (S4)
//!
//! While a search capsule is open on the pane **with something typed in it**, the
//! ordinal stack stops being the command ledger and becomes the *merge* of the
//! ledger and the matched lines — `[...new Set([...cmdLines, ...sSet])].sort()`
//! (mock 4633-4639). Everything above survives it untouched: the density floor,
//! the buckets, the fisheye, the cosine skirt and the glance card are all asked
//! of [`Entry`] rather than of a command, and a command is simply the kind of
//! entry that was there before. That is the sentence §7.1.5d asks this slice to
//! make true — *"the rail machinery is reused whole; only what a tick is a
//! picture of changes"*.
//!
//! Three things do change, and they are the three the mock-up itself changes:
//!
//! * **Ink.** `.cmdrail.srch-mode` recedes every command tick to a fainter grey
//!   and lifts the matched ones to the accent, so hue tells the two sources
//!   apart (A45-A50). A tick carries the strongest signal in it —
//!   [`Signal`], max-wins, the tab dot's own rule — and [`Signal::Fail`] outranks
//!   a plain match, which is **a deliberate departure from the stylesheet**: see
//!   [`Signal::Fail`] for D-9.
//! * **What a press means.** A match tick *selects* its hit; a command tick still
//!   jumps (B40-B41). [`Target`] is that fork, and it is a field on the tick
//!   rather than a question asked at the press, because the tick is the thing
//!   that knows which of the two it was drawn as.
//! * **What the card says.** The noun follows the source: `k lines` where it said
//!   `k commands`, and both counts when one bucket holds both (B36).
//!
//! Close the capsule, or empty the field, and the stack is the ledger again —
//! bit for bit, which is what [`Stack`] being an input rather than a mode flag
//! inside this file buys and what the restore test asserts.

use std::f32::consts::PI;
use std::time::{Duration, Instant};

use bt_render::{
    ChromePalette, OverlayQuad, TERMINAL_SCROLL_LANE_LOGICAL_PX, rounded_overlay_fill,
};
use bt_term::{CommandMark, CommandMarkId};
use bt_viewport::SearchLine;

use crate::marks::OverlayLayer;
use crate::tooltip::NAME_PLACE_SEPARATOR;
use crate::{Motion, RevealTween};

/// `.cmdtick { width: 9px }` — a tick's resting **length**.
///
/// Length and not height: the bar lies sideways, and the terminology was untangled
/// on 2026-07-18 after a round of errata in which the user's "height" turned out
/// to mean the long axis. The stylesheet's own comment says which way round it
/// settled — *"width animates, thickness stays 2px"* — and this file uses the
/// mock-up's property names so the two can be read side by side.
pub const TICK_LENGTH_LOGICAL_PX: f32 = 9.0;
/// `.cmdtick { height: 2px }` — and it never changes. Every growth in this file
/// is horizontal.
pub const TICK_THICKNESS_LOGICAL_PX: f32 = 2.0;
/// `.cmdtick { border-radius: 2px }` — a two-pixel bar with a two-pixel radius is
/// a stadium, which is what the mock-up draws.
pub const TICK_RADIUS_LOGICAL_PX: f32 = 2.0;
/// The crest's length: `9 · (1 + 2·cos²(0))` = 27, the peak of the mock-up's own
/// magnification curve (8452-8457) evaluated at the tick the pointer selected.
///
/// The rest of that curve — the ±1 and ±2 neighbours at 22.5 and 13.5 — comes out
/// of [`curve_length`], which is this same expression at a non-zero distance.
pub const TICK_CREST_LENGTH_LOGICAL_PX: f32 = 27.0;
/// How many ordinals the bell reaches before it is back at the resting length —
/// the `3` of `u = min(|i − nearest| / 3, 1)` (mock 8453).
///
/// Five ticks take part in the magnification and no more: the crest, and two
/// neighbours on each side. The sixth is already at `u = 1`, where `cos(π/2)` is
/// zero and the curve has returned to nine pixels.
pub const CREST_CURVE_REACH: f32 = 3.0;
/// `.cmdtick.sub { margin-right: 4px }` — how far an unfolded member steps *out*
/// of the right-aligned column.
///
/// The stylesheet's comment is a bug report with the fix attached: *"the expanded
/// group STEPS OUT of the right-aligned rail — visibly 'opened' even when a
/// bucket only holds two members (user report 2026-07-18: the expansion was
/// imperceptible; and never shorter than the crest — **one width base for
/// everyone**)"*. The second half is the part that is easy to get wrong: an
/// unfolded member is **not** drawn thinner or shorter than a folded tick. It is
/// the same nine pixels, moved four to the left, and the group reads as opened
/// because its whole column is offset rather than because its ticks are smaller.
/// `docs/DESIGN.md` §7.1.5c said "one step thinner, 7px" for a while; the
/// stylesheet is the executable artefact and it says otherwise.
pub const TICK_SUB_OFFSET_LOGICAL_PX: f32 = 4.0;
/// The gap between the reserved scroll lane and the rail's own box — the other
/// half of the mock-up's `right: 11px`.
pub const RAIL_LANE_GAP_LOGICAL_PX: f32 = 3.0;
/// `.cmdrail { padding: 8px 3px }`, horizontal half.
pub const RAIL_PADDING_X_LOGICAL_PX: f32 = 3.0;
/// `.cmdrail { padding: 8px 3px }`, vertical half.
pub const RAIL_PADDING_Y_LOGICAL_PX: f32 = 8.0;
/// `avail = max(60, paneH * .8)` (mock 4642) — the block may take four fifths of
/// the pane and no more, so a rail never runs edge to edge and never reads as a
/// scroll bar.
pub const RAIL_BLOCK_FRACTION: f32 = 0.8;
/// The other half of the same expression: below sixty pixels the fraction stops
/// shrinking, because a pane can be short enough that four fifths of it cannot
/// hold four ticks.
pub const RAIL_BLOCK_MIN_LOGICAL_PX: f32 = 60.0;
/// `pitch = max(4, min(9, avail / N))` (mock 4643) — the density floor.
pub const TICK_PITCH_MIN_LOGICAL_PX: f32 = 4.0;
/// The same expression's ceiling: ticks never spread further apart than nine
/// pixels however few of them there are, so two commands do not produce two marks
/// at opposite ends of the pane.
pub const TICK_PITCH_MAX_LOGICAL_PX: f32 = 9.0;
/// `gap = max(2, pitch - 2)` (mock 4644) — pitch less the tick's own thickness,
/// floored so that ticks never touch.
pub const TICK_GAP_MIN_LOGICAL_PX: f32 = 2.0;
/// `.cmdtick { opacity: .45 }` — at rest the ticks are grey, because *"queries
/// stay quiet"*.
pub const TICK_REST_OPACITY: f32 = 0.45;
/// `.cmdtick.fail { opacity: .6 }` — *"signals earn permanent colour"*. A failed
/// command is red without being pointed at, and this is the one thing on the rail
/// that is.
pub const TICK_FAIL_REST_OPACITY: f32 = 0.6;
/// `.cmdrail.hot .cmdtick { opacity: .55 }` — the whole rail lifts when the
/// pointer is on it, which is the mock-up's *"hovering the rail colours them"*.
pub const TICK_HOT_OPACITY: f32 = 0.55;
/// `.cmdrail.hot .cmdtick.fail { opacity: .65 }` — a failure lifts too, in its
/// own hue and by its own five hundredths.
pub const TICK_FAIL_HOT_OPACITY: f32 = 0.65;
/// `.cmdrail.hot .cmdtick.crest { opacity: 1 }` — *"the SELECTED tick:
/// unmistakable"*.
pub const TICK_CREST_OPACITY: f32 = 1.0;
/// `.cmdrail.srch-mode .cmdtick { opacity: .22 }` (mock 1544) — a command tick
/// **while the rail is carrying results**.
///
/// Half of the resting `.45`, which is the stylesheet saying the thing out loud:
/// the commands have not gone, they have stepped back. They are still there to be
/// counted, still there to be clicked (B62), and the reader's eye is not
/// competing with them for the matches.
pub const TICK_SEARCH_REST_OPACITY: f32 = 0.22;
/// `.cmdrail.srch-mode.hot .cmdtick { opacity: .35 }` (mock 1545) — the same
/// receded grey, lifted because a hand is on the rail.
pub const TICK_SEARCH_HOT_OPACITY: f32 = 0.35;
/// `.cmdrail.srch-mode.hot .cmdtick.crest { opacity: .9 }` (mock 1546) — a
/// *command* crest during a search.
///
/// Nine tenths and not the full opacity a crest wears otherwise, and it is the
/// stylesheet keeping one step of headroom above it for the thing that does reach
/// `1`: the current match.
pub const TICK_SEARCH_CREST_OPACITY: f32 = 0.9;
/// `.cmdrail.srch-mode .cmdtick.smt { opacity: .6 }` (mock 1547) — **a match is
/// lit at rest**.
///
/// The `.fail` doctrine applied to a second signal: *"signals earn colour at
/// rest"*. A reader who has typed a query and taken their hand off the mouse can
/// still see where in the scrollback the answers are.
pub const TICK_MATCH_REST_OPACITY: f32 = 0.6;
/// `.cmdrail.srch-mode.hot .cmdtick.smt { opacity: .75 }` (mock 1548).
pub const TICK_MATCH_HOT_OPACITY: f32 = 0.75;
/// `transition: width .1s ease` (mock 1369) — the crest travelling from one
/// ordinal to the next.
///
/// It is what makes the mock-up's own sentence true: *"指针跨中点时峰整刻度切换,
/// 宽度过渡使切换读作运动"* — the peak jumps a whole ordinal at the midpoint, and
/// the tenth of a second is what turns that jump into a movement rather than a
/// flicker.
pub const TICK_WIDTH_TRANSITION: Duration = Duration::from_millis(100);
/// `transition: background .14s ease` (mock 1369) — grey to accent as the rail
/// warms, and accent to the deepened crest under the pointer.
pub const TICK_BACKGROUND_TRANSITION: Duration = Duration::from_millis(140);
/// `transition: opacity .12s ease` (mock 1369) — `.45 → .55 → 1`.
///
/// **A separate span from the background's**, and deliberately not rounded to
/// match it: the stylesheet declares three durations on one line and they differ
/// by a fortieth of a second each. Collapsing them would be an aesthetic decision
/// taken by an implementer, and the sum of the three is what the rail's warming
/// actually looks like.
pub const TICK_OPACITY_TRANSITION: Duration = Duration::from_millis(120);
/// `left = r.left − peek.offsetWidth − 12` (mock 8476) — the card stands this far
/// to the **left** of the tick it names.
///
/// Left and not below, because the rail is already on the pane's right edge: a
/// card below would have nowhere to go, and a card to the right would be off the
/// window. The whole of `#cmd-peek`'s placement is this and [`PEEK_RISE_LOGICAL_PX`].
pub const PEEK_GAP_LOGICAL_PX: f32 = 12.0;
/// `top = r.top − 8` (mock 8475) — the card's top edge sits eight pixels *above*
/// the two-pixel bar it names, so a 24-pixel card is roughly centred on it
/// without the arithmetic pretending to be a centring rule it is not.
pub const PEEK_RISE_LOGICAL_PX: f32 = 8.0;
/// What the card says about a mark whose text the ledger never got
/// (`command_marks.rs` leaves it empty when `C` and the output that scrolled the
/// prompt away arrived in one PTY read).
///
/// Drawn in the muted ink, because the word is a *category* and not a quotation:
/// the card's whole contract is "this is the command", and a card that said
/// nothing at all would read as a bug in the card rather than as an honest gap in
/// the ledger.
pub const PEEK_EMPTY_TEXT: &str = "command";
/// How many times [`resolve`] may lay the rail out again before it answers.
///
/// The mock-up's own `depth < 2` (8443, 8449), kept as a number because the
/// ruling names it — but this file does not need it the way the mock-up does. See
/// [`resolve`] for why: its rule has no feedback loop to bound, and two is simply
/// the count it reaches by construction.
pub const FISHEYE_RELAYOUT_CAP: usize = 2;
/// `.term > div.cmd-jump { animation: cmdflash .95s ease }` — how long the row a
/// jump landed on says so.
pub const JUMP_FLASH: Duration = Duration::from_millis(950);
/// `@keyframes cmdflash { 0% { background: color-mix(in srgb, var(--accent) 22%,
/// transparent) } }` — the flash's opening alpha, easing to nothing.
pub const JUMP_FLASH_OPACITY: f32 = 0.22;
/// `.term > div.cmd-jump { border-radius: 4px }`.
pub const JUMP_FLASH_RADIUS_LOGICAL_PX: f32 = 4.0;
/// `term.scrollTo({ top: max(0, line.offsetTop - 8) })` (mock 4686) — the jumped-to
/// row lands eight pixels below the top of the viewport rather than flush with it,
/// so it reads as a line *in* a page instead of a page that begins there.
pub const JUMP_TOP_INSET_LOGICAL_PX: f32 = 8.0;

/// What a press on a tick means — the rail's two sources, at the one point they
/// differ (B40-B41).
///
/// *"Clicking a match tick **SELECTS** that match (current advances there, count
/// follows); a plain command tick keeps its **normal jump**"* (mock 8519-8525).
/// Both verbs already existed before this slice — `jump_to_command_mark` since
/// S1, `SearchState::set_current` since S3 — so the fork is the whole of the new
/// code and it is one `match` at one press.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    /// Scroll to this command's own row and flash it (S1).
    ///
    /// For a bucket of several it is the **newest** member, which is the mock-up's
    /// own `data-line="${members[members.length - 1]}"` (4666). A collapsed bucket
    /// is a range of history and the newest end of it is the one a reader is
    /// looking for; S2's fisheye is what makes the older members reachable exactly.
    Command(CommandMarkId),
    /// Make this hit current — an index into `SearchState::hits`.
    ///
    /// The **first** hit on the line the tick stands for, which is the mock-up's
    /// `srch.marks.findIndex(m => m.line === line)`. A line with six matches on it
    /// is one tick (B56's per-line dedup), and pressing it puts the reader at the
    /// top of that line's six rather than at an arbitrary one of them; `Enter`
    /// walks the other five from there.
    Match(usize),
}

/// The four inks one tick can be drawn in, weakest first.
///
/// **Maximum wins**, which is not a rule invented here: *"a collapsed bucket
/// carries its strongest member's signal — a match inside lights it, the current
/// match deepens it (**same maximum-wins rule as the tab dot**)"* (mock
/// 4660-4663, inventory B19/B56). Deriving [`Ord`] in this declaration order *is*
/// the rule, so the folding code is one `max` and there is no table of cases to
/// get out of step with the stylesheet.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum Signal {
    /// A command that did not fail. Grey — `.cmdtick` at rest, or the receded
    /// `.cmdrail.srch-mode .cmdtick` while a search is open.
    #[default]
    Command,
    /// `.smt` — a line carrying at least one search hit. The accent, at rest.
    Match,
    /// `.fail` — a command whose exit code was not zero. Red, at rest.
    ///
    /// # D-9: the red survives the search, and the stylesheet says it does not
    ///
    /// `.cmdtick.fail` has specificity (0,2,0) and `.cmdrail.srch-mode .cmdtick`
    /// has (0,3,0), so in the mock-up a failure opening a search turns grey — and
    /// there is no `.cmdrail.srch-mode .cmdtick.fail` rule anywhere to stop it.
    /// That is an **accident of the cascade** rather than a decision: the same
    /// stylesheet's own comment beside the resting red is *"signals earn permanent
    /// colour"*, and the sentence introducing the results rail names `.fail` as
    /// the doctrine the match ticks are following. A signal that stops signalling
    /// because a search box is open is a signal the reader learns not to trust.
    ///
    /// So the red stays, and it outranks a match: a line that both failed and
    /// matched is red, because "this command failed" is the rarer and the more
    /// consequential of the two things it has to say. Recorded as an intentional
    /// deviation in `docs/DESIGN.md` §7.1.5d and pinned by
    /// `a_failed_commands_tick_keeps_its_red_while_the_search_is_open`.
    Fail,
    /// `.smt.cur` — the line holding the current match. The deepened accent, at
    /// rest and at every temperature: *"the current match tick is the deep accent
    /// even unhovered"* (mock 1542-1543).
    Current,
}

/// One line of the ordinal stack, before density has decided how many of them
/// share a tick.
///
/// A line and not a command, because the merge is a set union over *lines*: a
/// command's own prompt row can be a row the query matches, and the mock-up's
/// `new Set([...cmdLines, ...sSet])` gives that row one entry rather than two.
/// Such an entry carries both — which is why the two fields are independent
/// `Option`s rather than a two-armed enum.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Entry {
    /// The command whose prompt row this line is, when it is one.
    pub mark: Option<CommandMarkId>,
    /// The first hit on this line, when the query matched it.
    pub hit: Option<usize>,
    /// The strongest of what this line carries.
    pub signal: Signal,
}

impl Entry {
    /// What a press on this line means.
    ///
    /// **A match wins**, which is the mock-up's own order of tests: `if (searching
    /// && the line is a match line) srchSetCur; else jumpToPromptLine`. A prompt
    /// row that matched is a place in the *search*, because the reader is holding
    /// a query and looking at a lit tick.
    ///
    /// The colour is decided separately, by [`Signal`]'s maximum, so a failed
    /// command whose prompt row matched is a red tick that selects a hit. The two
    /// questions genuinely are different: the ink says what this line *is*, the
    /// press says what the reader is doing.
    #[must_use]
    pub fn target(&self) -> Target {
        match (self.hit, self.mark) {
            (Some(hit), _) => Target::Match(hit),
            (None, Some(mark)) => Target::Command(mark),
            // Unreachable by construction — every entry comes from [`commands`]
            // or [`merge`], and both give every entry at least one source. Stated
            // as the newest command rather than as a panic because a rail is a
            // decoration and a decoration may not take the window down.
            (None, None) => Target::Command(CommandMarkId(0)),
        }
    }
}

/// The ordinal stack, and which of the two modes it was built in.
///
/// The mode is carried beside the entries rather than derived from them because
/// the two are genuinely independent: a search with **no hits at all** still
/// recedes the commands to `.22` — the reader typed something and the rail is
/// answering "not here" — while a rail of ticks that happen to all be matches is
/// not in search mode unless a capsule is open. The mock-up spells this the same
/// way: `srch-mode` is a class on the rail, `data-slines` is its content.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stack {
    pub entries: Vec<Entry>,
    /// `.cmdrail.srch-mode` — a capsule is open on this pane **and there is
    /// something typed in it**.
    ///
    /// The second half is this file's, not the mock-up's: the prototype adds the
    /// class the moment a capsule exists, so its rail goes grey at `Ctrl+F` before
    /// a key is pressed. An empty field has asked nothing, so there is nothing for
    /// the commands to recede behind.
    pub searching: bool,
}

/// One drawn tick: where it is, what it stands for, and what a press on it means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tick {
    /// The verb this tick carries — the newest member's, for a bucket.
    pub target: Target,
    /// How many entries this tick stands for. One at every density the pane can
    /// actually hold; more only once the four-pixel floor has been passed.
    pub members: usize,
    /// How many of those entries carry a search hit — the glance card's `k lines`.
    pub matched: usize,
    /// How many of them are commands — the glance card's `m commands`. It and
    /// [`Self::matched`] can sum past [`Self::members`], because a line that is
    /// both a prompt row and a match is one entry counted in both.
    pub commanded: usize,
    /// The strongest signal any member carries — [`Signal`], maximum wins.
    pub signal: Signal,
    /// Which **bucket** this tick belongs to — the mock-up's `data-slot`.
    ///
    /// It is the identity the fisheye is keyed on, and the reason the members of
    /// an unfolded bucket all carry the slot of the bucket they came out of
    /// rather than a number of their own: *"Members keep the bucket's slot id, so
    /// hovering inside the expansion never collapses it"* (mock 8440-8442). One
    /// tick per command means slot and index agree; past the density floor they
    /// part company, and every question about *staying open* is asked of the slot.
    pub slot: usize,
    /// `.cmdtick.sub` — one member of an unfolded bucket, drawn
    /// [`TICK_SUB_OFFSET_LOGICAL_PX`] further left than the column it stepped out
    /// of.
    pub sub: bool,
    /// The resting rectangle, in physical pixels.
    ///
    /// **Resting**, so its right edge is the one thing about it that never moves:
    /// the crest and the whole cosine skirt grow leftward out of `rect[2]`, and
    /// [`paint`] reads exactly that edge. Vertical layout is a function of this
    /// rectangle alone, which is what *"width only grows leftward, so vertical
    /// layout never shifts and there is no jitter"* buys.
    pub rect: [f32; 4],
}

/// What is drawn on one pane's right edge, and the answer to "which tick is the
/// pointer on".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rail {
    /// The rail's pointer surface, in physical pixels — empty (all zeros) when
    /// there is nothing to draw.
    ///
    /// **The rail is the surface, not the tick** (mock 8404-8409): a bar two
    /// pixels tall is not something a hand can be asked to land on, so the band
    /// takes the press and the nearest tick answers it. That is the mock-up's own
    /// mechanism, minus its bell curve.
    ///
    /// It is the width of the mock-up's *resting* box — a tick and its two
    /// three-pixel paddings — and [`Self::hot_bounds`] is the same band grown to
    /// the crest's own width. Which of the two answers depends on whether the
    /// rail is already hot, and that asymmetry is the whole ruling: see
    /// [`band`].
    pub bounds: [f32; 4],
    /// The band **while the rail is hot** — [`Self::bounds`] grown leftward to
    /// hold a crest, and an unfolded member's four-pixel step besides.
    ///
    /// **D-17, the middle path (ruled 2026-08-16).** A CSS flex box is sized by
    /// its widest child, so the browser's rail grows the instant a tick expands;
    /// S1 refused to reproduce that, because thirty-three logical pixels down
    /// every pane's right edge in which a hyperlink stops underlining and a
    /// Ctrl+click stops opening is a real cost, and the mock-up only gets away
    /// with it because its `.term` carries an 18px right padding no native grid
    /// has. But a fixed band has its own cost, and S1 wrote it down: a crest is
    /// drawn twelve pixels left of where the band reaches, so a hand walking
    /// *onto* the crest leaves the rail and the crest relaxes under it.
    ///
    /// Both costs are avoidable because they are not costs of the same state. At
    /// **rest** the band is the small one, so nothing about the pane's right edge
    /// changes for a reader who is not pointing at the rail — that is the
    /// hyperlink's case, and it is the case that is true almost all the time.
    /// While **hot** the band is the crest's, so the crest is a thing a hand can
    /// be on. The growth cannot flap: it happens only on the way in, and the
    /// shrink happens only once the pointer has left the *larger* box, so the two
    /// thresholds are never the same pixel.
    pub hot_bounds: [f32; 4],
    /// Oldest at the top — *"position carries order, not scroll geometry"*.
    pub ticks: Vec<Tick>,
    /// `k` — how many commands each tick stands for (mock 4646). One until the
    /// density floor is passed.
    pub bucket_size: usize,
    /// The vertical space between one tick and the next, in physical pixels —
    /// `max(2, pitch − 2)` (mock 4644), or the compressed gap an unfolded bucket
    /// leaves its neighbours.
    ///
    /// Kept on the rail rather than recomputed because two readers need it and
    /// they must agree: the layout places the ticks with it, and [`resolve`]
    /// measures an open group's *dominion* — the zone that keeps it open — half a
    /// gap beyond its outermost members.
    pub gap: f32,
    /// The scale the geometry above was laid out at, kept so the paint can round
    /// its radii the same way the layout rounded its boxes.
    pub scale: f32,
    /// [`Stack::searching`], carried through so that [`paint`] can read it.
    ///
    /// The ink a *command* tick is drawn in depends on the mode and not on the
    /// tick — `.45` grey normally, `.22` while the rail is carrying results — so
    /// the mode has to survive the layout. Carried rather than re-derived from the
    /// ticks for [`Stack::searching`]'s own reason: a search with no hits has no
    /// match tick to be recognised by, and it is exactly the search whose rail
    /// most needs to look answered.
    pub searching: bool,
}

/// What the search contributes to a rail's identity — the second half of
/// [`RailKey`] (S4).
///
/// Three numbers, and between them they name every input the merge has. Not the
/// hits themselves, and not a hash of them: a hit set is up to a hundred thousand
/// entries and the rail is asked whether it is stale on every frame, so the key
/// has to be a constant-size fingerprint of the *things that produce* the hits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RailSearch {
    /// What was typed and how it was switched — `Runtime::search_revision`,
    /// bumped by every edit of the field and every toggle.
    ///
    /// Kept beside [`Self::hits`] rather than folded into it because they answer
    /// different questions: this one moves when the reader does, and a query that
    /// finds the same lines twice in a row still asks a new question.
    pub query: u64,
    /// `SearchState::revision` — bumped whenever the hit set is replaced, which
    /// is what makes new output re-merge the rail (R2, ticket item 6).
    pub hits: u64,
    /// `SearchState::current_index` — which tick wears [`Signal::Current`].
    ///
    /// An index rather than the hit, because `Enter` walking from one identical
    /// line to another identical one still moves the `.cur` tick.
    pub current: Option<usize>,
}

/// What a laid-out rail is a function of, and therefore what may invalidate one.
///
/// **Not the scroll offset** — an ordinal stack does not move when the page does
/// — and **not the crest**, whose whole travel is a set of widths grown leftward
/// out of rectangles this key already fixed. What *is* here beside the ledger and
/// the pane is the unfolded bucket, because a fisheye genuinely moves ticks: it
/// is a different arrangement of the same marks and not a different colour on the
/// same arrangement.
///
/// [`Self::search`] is the fifth, and its being an `Option` is the mode switch
/// itself: `None` is the plain command rail, and a key that goes from `Some` back
/// to `None` differs from the key it wore before the search opened only if
/// something else moved too — which is exactly the "zero residue" the acceptance
/// anchor asks for, stated as an equality rather than as a clean-up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RailKey {
    /// [`bt_term::DualPlaneSession::command_marks_revision`] — bumped by ledger
    /// changes and by nothing else, which is the only reason it exists.
    pub revision: u64,
    /// The pane's body, in physical pixels.
    pub body: [f32; 4],
    pub scale: f32,
    /// Which bucket, if any, is open under the pointer.
    pub expanded: Option<usize>,
    /// The search this rail is carrying, or `None` when it is carrying none.
    pub search: Option<RailSearch>,
}

/// A vector of numbers easing to a vector of numbers, on one clock.
///
/// [`RevealTween`] is the scalar of this and stays the type for everything with
/// one value to move. A rail has one value *per tick* — every tick's length
/// changes when the crest travels, and every tick's crest-ness changes with it —
/// and the three ways to express that are all worse than this one: a tween per
/// tick multiplies the clock by the tick count and lets two of them disagree
/// about `now`; a single scalar plus a formula cannot ease *from* the widths a
/// half-finished travel was actually showing; and easing the pointer's position
/// instead of the widths would put the curve's peak between two ordinals, which
/// is the ambiguous state the whole selection-first ruling exists to forbid.
///
/// A length change means the drawn ticks are not the ticks that were drawn — a
/// bucket opened, the ledger grew, the pane was resized — so the travel is
/// **reseeded from [`Self::rest`]** rather than continued between two sets that do
/// not correspond. Continuing would be easing tick 4's width toward what is now
/// tick 7's; and rest is the right origin rather than the target because rest is
/// what [`build`] actually draws a freshly laid-out rail at, which makes the
/// seeding a statement about the picture on the glass instead of a convention.
#[derive(Clone, Debug)]
struct VecTween {
    from: Vec<f32>,
    to: Vec<f32>,
    started: Option<Instant>,
    span: Duration,
    /// The value a tick has when the rail is cold — nine pixels of length, no
    /// crest at all.
    rest: f32,
}

impl VecTween {
    fn over(span: Duration, rest: f32) -> Self {
        Self {
            from: Vec::new(),
            to: Vec::new(),
            started: None,
            span,
            rest,
        }
    }

    fn retarget(&mut self, target: Vec<f32>, now: Instant, motion: Motion) {
        if self.to == target {
            return;
        }
        let from = if self.to.len() == target.len() {
            self.sample(now, motion)
        } else {
            vec![self.rest; target.len()]
        };
        // Nothing to travel — a rail that gained a tick while nobody was pointing
        // at it is already showing the answer, and a clock started here would ask
        // for a hundred milliseconds of frames that all draw the same picture.
        self.started = (motion == Motion::Full && from != target).then_some(now);
        self.from = from;
        self.to = target;
    }

    fn sample(&self, now: Instant, motion: Motion) -> Vec<f32> {
        let Some(eased) = self.progress(now, motion) else {
            return self.to.clone();
        };
        self.from
            .iter()
            .zip(&self.to)
            .map(|(from, to)| from + (to - from) * eased)
            .collect()
    }

    fn is_running(&self, now: Instant, motion: Motion) -> bool {
        self.progress(now, motion).is_some()
    }

    /// The eased fraction, or `None` when this tween is standing still.
    fn progress(&self, now: Instant, motion: Motion) -> Option<f32> {
        let started = self.started.filter(|_| motion == Motion::Full)?;
        if self.from.len() != self.to.len() {
            return None;
        }
        let elapsed = now.saturating_duration_since(started);
        (elapsed < self.span).then(|| {
            crate::cubic_bezier(elapsed.as_secs_f32() / self.span.as_secs_f32(), crate::EASE)
        })
    }
}

/// Everything about one rail that the *pointer* decides, and the four clocks it
/// decides them on.
///
/// Kept beside the geometry rather than in it because the two invalidate on
/// different events: a rail is rebuilt when the ledger or the pane moves, and
/// this changes when a hand does. The one place they meet is the unfolded bucket,
/// which is why [`RailKey`] carries [`Self::expanded`] and nothing else from here.
#[derive(Clone, Debug)]
pub struct RailPointer {
    expanded: Option<usize>,
    /// `background .14s` at the rail level — `0` grey, `1` the hot ink.
    hot_ink: RevealTween,
    /// `opacity .12s` at the rail level — `.45 → .55`, or `.6 → .65` for a
    /// failure.
    hot_alpha: RevealTween,
    /// `background .14s` per tick — `0` the hot ink, `1` the deepened crest.
    crest_ink: VecTween,
    /// `opacity .12s` per tick — `0` the hot alpha, `1` fully opaque.
    crest_alpha: VecTween,
    /// `width .1s` per tick, in **logical** pixels: the cosine skirt.
    width: VecTween,
}

impl Default for RailPointer {
    /// A cold rail with nothing in flight. Written out rather than derived
    /// because none of the four clocks has a meaningful zero: a tween with no
    /// span is either an instant nobody asked for or somebody else's duration
    /// borrowed by accident, which is the same reason [`RevealTween`] has no
    /// `Default` either.
    fn default() -> Self {
        Self {
            expanded: None,
            hot_ink: RevealTween::over(TICK_BACKGROUND_TRANSITION),
            hot_alpha: RevealTween::over(TICK_OPACITY_TRANSITION),
            crest_ink: VecTween::over(TICK_BACKGROUND_TRANSITION, 0.0),
            crest_alpha: VecTween::over(TICK_OPACITY_TRANSITION, 0.0),
            width: VecTween::over(TICK_WIDTH_TRANSITION, TICK_LENGTH_LOGICAL_PX),
        }
    }
}

impl RailPointer {
    /// Which bucket is open under the pointer.
    #[must_use]
    pub fn expanded(&self) -> Option<usize> {
        self.expanded
    }

    /// Aim every clock at the state `hot` and `nearest` describe.
    ///
    /// One call for all four, because they are one gesture: a rail that warmed
    /// its colour on one frame and moved its crest on another would read as two
    /// things happening to it.
    pub fn aim(
        &mut self,
        ticks: usize,
        nearest: Option<usize>,
        hot: bool,
        now: Instant,
        motion: Motion,
    ) {
        let crest = nearest.filter(|_| hot);
        let widths = (0..ticks)
            .map(|index| match crest {
                Some(peak) => curve_length(index as i64 - peak as i64),
                None => TICK_LENGTH_LOGICAL_PX,
            })
            .collect();
        let peaks: Vec<f32> = (0..ticks)
            .map(|index| f32::from(u8::from(crest == Some(index))))
            .collect();
        self.width.retarget(widths, now, motion);
        self.crest_ink.retarget(peaks.clone(), now, motion);
        self.crest_alpha.retarget(peaks, now, motion);
        let warmth = f32::from(u8::from(hot));
        self.hot_ink.retarget(warmth, now, motion);
        self.hot_alpha.retarget(warmth, now, motion);
    }

    /// Remember which bucket is open. Returns whether the answer moved, because
    /// the rail's geometry has to be laid out again when it did.
    pub fn expand(&mut self, expanded: Option<usize>) -> bool {
        let moved = self.expanded != expanded;
        self.expanded = expanded;
        moved
    }

    /// Whether any of the four clocks still owes a frame.
    #[must_use]
    pub fn is_animating(&self, now: Instant, motion: Motion) -> bool {
        self.hot_ink.sample(now, motion).1
            || self.hot_alpha.sample(now, motion).1
            || self.width.is_running(now, motion)
            || self.crest_ink.is_running(now, motion)
            || self.crest_alpha.is_running(now, motion)
    }

    /// Whether this rail is drawn exactly as [`build`] would draw it — cold, and
    /// with nothing left to ease. The one condition under which a cached picture
    /// is still the true one.
    #[must_use]
    pub fn is_resting(&self, now: Instant, motion: Motion) -> bool {
        !self.is_animating(now, motion)
            && self.hot_ink.sample(now, motion).0 == 0.0
            && self
                .crest_ink
                .sample(now, motion)
                .iter()
                .all(|mix| *mix == 0.0)
    }
}

/// One pane's rail, the key it was built under, and what the pointer is doing to
/// it.
#[derive(Clone, Debug, Default)]
pub struct RailCache {
    key: Option<RailKey>,
    rail: Rail,
    layer: OverlayLayer,
    pointer: RailPointer,
}

impl RailCache {
    /// Whether the rail this cache is holding was built for something else.
    ///
    /// **Asked separately from [`Self::install`] on purpose**, and the reason is a
    /// borrow rather than a taste: the ledger a rail is laid out from and the
    /// cache it is stored in hang off the same window, so a `&mut` cache cannot be
    /// handed a closure that reads the `&` session beside it. Splitting the
    /// question from the answer lets the caller lay out under a shared borrow and
    /// store under an exclusive one, which is also the honest shape — a cache
    /// decides *whether*, and a painter decides *what*.
    #[must_use]
    pub fn needs_rebuild(&self, key: RailKey) -> bool {
        self.key != Some(key)
    }

    /// Take a freshly laid-out rail and paint its resting picture.
    ///
    /// Nothing is done to the pointer here, deliberately. A rebuild that changed
    /// the tick count reseeds the per-tick travels the next time they are aimed —
    /// see [`VecTween`], whose seed is the resting length this very call has just
    /// drawn the new rail at — and the rail-level warmth is not a fact about any
    /// tick, so a bucket opening under a hand must not make the whole column blink
    /// back to grey on its way open.
    pub fn install(&mut self, key: RailKey, rail: Rail, palette: &ChromePalette) {
        self.layer = build(&rail, palette);
        self.rail = rail;
        self.key = Some(key);
    }

    pub fn rail(&self) -> &Rail {
        &self.rail
    }

    pub fn layer(&self) -> &OverlayLayer {
        &self.layer
    }

    pub fn pointer(&self) -> &RailPointer {
        &self.pointer
    }

    pub fn pointer_mut(&mut self) -> &mut RailPointer {
        &mut self.pointer
    }

    /// Fold whatever the fisheye had open, if the rail is about to change mode
    /// (S4).
    ///
    /// `srchRail()` and `closeSearch()` both do `rail.__expanded = null` before
    /// they re-render (mock 8613, 8672), and it is not housekeeping: an open
    /// bucket is a statement about *these* entries at *these* heights, and the
    /// mode switch replaces every entry in the stack. Leaving slot 7 open across
    /// it would unfold whatever three things happen to land in slot 7 of the new
    /// stack — a bucket the hand never asked for, under a hand that has not
    /// moved.
    ///
    /// Asked before [`RailKey`] is computed, because the key carries the open
    /// bucket and a fold that arrived after it would be a frame late.
    pub fn fold_for_mode(&mut self, searching: bool) {
        if self.rail.searching != searching {
            self.pointer.expand(None);
        }
    }

    /// Take the rail's own picture for this frame: the cached resting layer while
    /// nothing is happening to it, a fresh one while something is.
    #[must_use]
    pub fn picture(&self, palette: &ChromePalette, now: Instant, motion: Motion) -> OverlayLayer {
        if self.pointer.is_resting(now, motion) {
            return self.layer().clone();
        }
        paint(&self.rail, &self.pointer, palette, now, motion)
    }

    /// Forget everything. The palette is not part of [`RailKey`] — a theme change
    /// is not a fact about a pane — so the one thing that can silently outlive a
    /// rebuild is the ink, and this is how the theme switch says so.
    pub fn clear(&mut self) {
        self.key = None;
        self.rail = Rail::default();
        self.layer = OverlayLayer::default();
        self.pointer = RailPointer::default();
    }
}

/// The pane rectangle a rail hangs off, or `None` when this pane is not showing
/// one.
///
/// **The alternate screen suspends the picture and not the data**, and the two are
/// worth keeping apart. The ledger already refuses to record an alternate-screen
/// marker — §3.2 puts that screen in an isolated namespace, and a full-screen TUI
/// emitting `A/B/C/D` for its own redraws is describing its own canvas rather than
/// this session's history — so a pane running `vim` still holds a ledger full of
/// perfectly true marks: the commands that ran *before* `vim` started. What must
/// not happen is those being drawn over somebody else's canvas, over rows they say
/// nothing about, beside a scrollback that is not there. Switch back and every tick
/// is where it was.
///
/// It is a function rather than an `if` at the one call site so the ruling has a
/// name and a test, and so that the next surface to ask the same question — the
/// search capsule (S3), whose own ruling is the same "not on the alternate screen"
/// — asks it here instead of restating it.
#[must_use]
pub fn host_rect(body: [f32; 4], alternate_screen: bool) -> Option<[f32; 4]> {
    (!alternate_screen).then_some(body)
}

/// The mock-up's magnification curve: how long the tick `delta` ordinals from the
/// crest is drawn, in logical pixels.
///
/// `u = min(|Δi| / 3, 1)`, `w = 9 · (1 + 2·cos²(u·π/2))` — copied out of mock
/// 8453-8455 rather than re-derived, because the shape it makes is a ruling and
/// not an implementation detail: 27 at the crest, 22.5 and 13.5 for the first two
/// neighbours, and back to the resting 9 from the third outward. *"ONE base: the
/// crest is always the longest."*
///
/// **A function of the ordinal distance and never of the pointer's pixels.** That
/// is the five-rounds-of-errata ruling in one signature: the argument is an
/// integer, so between two ticks the answer does not slide — it belongs wholly to
/// one of them, and the transition on the width is what makes the handover read
/// as movement rather than as a flicker.
#[must_use]
pub fn curve_length(delta: i64) -> f32 {
    let u = (delta.unsigned_abs() as f32 / CREST_CURVE_REACH).min(1.0);
    TICK_LENGTH_LOGICAL_PX * (1.0 + 2.0 * (u * PI / 2.0).cos().powi(2))
}

/// One command as the merge sees it (S4).
///
/// `line` is an `Option` because a command's anchor can have been degraded out
/// from under it — the output it names was cleared, or evicted past the quota —
/// and the honest answer to "which line is it on" is then "no longer sayable".
/// Such a command is **not dropped**: see [`merge`] for where it goes and why.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandLine {
    pub mark: CommandMarkId,
    pub line: Option<SearchLine>,
    pub failed: bool,
}

/// One matched line as the merge sees it (S4).
///
/// **One per line, not one per hit** — B56's own note: *"the rail dedups by line;
/// a line with several matches gets one tick, while the counter counts hits"*. A
/// `grep` output where every line matches would otherwise draw a tick per hit and
/// the rail would say nothing at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MatchLine {
    pub line: SearchLine,
    /// The index of the **first** hit on this line, in the search's own order.
    pub hit: usize,
    /// Whether the current match is one of this line's.
    pub current: bool,
}

/// The plain rail's stack: one entry per command, in ledger order.
#[must_use]
pub fn commands(marks: &[CommandMark]) -> Stack {
    Stack {
        entries: marks
            .iter()
            .map(|mark| Entry {
                mark: Some(mark.id),
                hit: None,
                signal: if mark.failed() {
                    Signal::Fail
                } else {
                    Signal::Command
                },
            })
            .collect(),
        searching: false,
    }
}

/// The results rail's stack: the ledger and the matched lines, merged into one
/// ordinal stack in document order (B8).
///
/// # Why a merge and not a sort
///
/// The mock-up writes `[...new Set([...cmdLines, ...sSet])].sort(asc)`, which it
/// can afford because both its sides are integers into one array. Ours are not:
/// a command carries a [`CommandLine::line`] that may be `None`, and a `sort`
/// would have to invent a place for it. A two-pointer merge does not — both
/// inputs are already ascending (the ledger appends, the scan walks the document
/// in order), so walking them together is both cheaper and the only formulation
/// in which *"a command whose line cannot be named keeps its ledger slot"* is a
/// consequence rather than a special case. A rail that quietly lost a tick
/// because a search was open would break the acceptance anchor on the way back
/// out.
///
/// # Where a line that is both goes
///
/// Into one entry carrying both, which is what the mock-up's `Set` does. Its ink
/// is [`Signal`]'s maximum of the two and its verb is [`Entry::target`]'s — see
/// both for why those two answers are allowed to disagree.
///
/// `matches` must be ascending and already deduplicated per line; `commands` must
/// be in ledger order, which is document order for every command whose line is
/// still sayable.
#[must_use]
pub fn merge(commands: &[CommandLine], matches: &[MatchLine]) -> Stack {
    let of_match = |line: &MatchLine| Entry {
        mark: None,
        hit: Some(line.hit),
        signal: if line.current {
            Signal::Current
        } else {
            Signal::Match
        },
    };
    let mut entries = Vec::with_capacity(commands.len() + matches.len());
    let mut next = 0;
    for command in commands {
        let command_signal = if command.failed {
            Signal::Fail
        } else {
            Signal::Command
        };
        let mut hit = None;
        let mut signal = command_signal;
        if let Some(line) = command.line {
            while next < matches.len() && matches[next].line < line {
                entries.push(of_match(&matches[next]));
                next += 1;
            }
            if next < matches.len() && matches[next].line == line {
                hit = Some(matches[next].hit);
                signal = command_signal.max(of_match(&matches[next]).signal);
                next += 1;
            }
        }
        entries.push(Entry {
            mark: Some(command.mark),
            hit,
            signal,
        });
    }
    entries.extend(matches[next..].iter().map(of_match));
    Stack {
        entries,
        searching: true,
    }
}

/// Place the ticks for `stack` inside a pane body, in physical pixels, with
/// bucket `expanded` — if there is one, and if it holds more than one entry —
/// unfolded into its members.
///
/// # The arithmetic, and the one place it knowingly leaves the mock-up standing
///
/// Everything here is `renderRailTicks` (mock 4640-4670): `avail` from the pane,
/// `capacity` from the four-pixel density floor, `k` from `capacity`, `pitch`
/// from `avail`, and `gap` from `pitch` — and the *whole* of it, search branch
/// included, because [`Stack`] has already made the two sources one list. That is
/// the mock-up's own arrangement and the point of it: `renderRailTicks` has one
/// body, and the takeover changes what `lines` holds rather than what the
/// function does with it.
///
/// **The mock-up's own arithmetic is not self-consistent, and S2 is where it gets
/// corrected (inventory B15).** `pitch` is computed there from the *unaggregated*
/// count while `ceil(N/k)` ticks are actually drawn, so past the density floor the
/// block comes out far shorter than the space it was given: two hundred commands
/// in an eight-hundred-pixel pane draw a hundred ticks at the four-pixel floor and
/// fill 398 of their 640 pixels — sixty-two per cent of a rail, centred, reading
/// as a pane whose history stopped somewhere in the middle. S1 reproduced it
/// deliberately, because changing it changes the *density* the rail reads at and
/// density was tier ②'s question — which is this slice. Here it is answered:
/// **pitch comes from the number of ticks actually drawn**, so a dense rail fills
/// `avail` and the aggregation is legible as ticks standing closer together
/// rather than as a rail that shrank.
///
/// # What an unfolded bucket does to the block
///
/// It does not lengthen it. `gap` is recomputed for the drawn count against the
/// *folded* block's extent, so the members open into room the neighbours give up —
/// the fisheye's own bargain, and the reason the rail's outer edges do not move
/// when a bucket opens under the pointer. Only once the compression reaches the
/// two-pixel floor does the block grow, and then it grows symmetrically about the
/// pane's middle, which is where it was centred to begin with.
#[must_use]
pub fn lay_out(body: [f32; 4], stack: &Stack, scale: f32, expanded: Option<usize>) -> Rail {
    let marks = &stack.entries;
    // An empty stack draws nothing at all — no box, no band, no error (inventory
    // C13). `cmd.exe` never sends an OSC 133 and a PowerShell without the
    // integration script never sends one either, so this is the *ordinary* state
    // of a large fraction of panes and not an edge case. The mock-up emits the
    // rail element regardless, because its search takeover needs a surface to hang
    // results off; there is no element here to emit, so the empty rail is simply
    // empty — and a search *with* results puts entries in the stack, which is the
    // same thing said without an element.
    if marks.is_empty() || body[2] <= body[0] || body[3] <= body[1] {
        return Rail {
            scale,
            searching: stack.searching,
            ..Rail::default()
        };
    }
    let px = |logical: f32| logical * scale;
    let height = body[3] - body[1];
    let avail = px(RAIL_BLOCK_MIN_LOGICAL_PX).max(height * RAIL_BLOCK_FRACTION);
    let thickness = px(TICK_THICKNESS_LOGICAL_PX).round().max(1.0);
    // `capacity = max(4, floor(avail / 4))` — how many ticks fit at the density
    // floor; `k = ceil(N / capacity)` — how many commands each one has to stand
    // for. Four is a floor on the floor: a pane too short to hold four ticks still
    // gets four rather than one, because a rail of one tick says nothing at all.
    let capacity = (avail / px(TICK_PITCH_MIN_LOGICAL_PX)).floor().max(4.0) as usize;
    let bucket_size = marks.len().div_ceil(capacity).max(1);
    // B15's correction: the *drawn* count, not `marks.len()`.
    let folded = marks.len().div_ceil(bucket_size);
    let pitch =
        (avail / folded as f32).clamp(px(TICK_PITCH_MIN_LOGICAL_PX), px(TICK_PITCH_MAX_LOGICAL_PX));
    let folded_gap = px(TICK_GAP_MIN_LOGICAL_PX).max(pitch - px(TICK_THICKNESS_LOGICAL_PX));
    // The extent the folded rail would occupy, and the extent an unfolded one is
    // held to: the members open into room their neighbours give up.
    let block = folded as f32 * thickness + (folded - 1) as f32 * folded_gap;
    let open = expanded.filter(|slot| {
        marks
            .chunks(bucket_size)
            .nth(*slot)
            .is_some_and(|chunk| chunk.len() > 1)
    });
    let mut ticks: Vec<Tick> = Vec::with_capacity(folded + bucket_size);
    for (slot, members) in marks.chunks(bucket_size).enumerate() {
        if open == Some(slot) {
            ticks.extend(members.iter().map(|entry| Tick {
                target: entry.target(),
                members: 1,
                matched: usize::from(entry.hit.is_some()),
                commanded: usize::from(entry.mark.is_some()),
                signal: entry.signal,
                slot,
                sub: true,
                rect: [0.0; 4],
            }));
        } else {
            ticks.push(Tick {
                // The newest member (mock 4666), which for a chunk is its last.
                target: members[members.len() - 1].target(),
                members: members.len(),
                matched: members.iter().filter(|entry| entry.hit.is_some()).count(),
                commanded: members.iter().filter(|entry| entry.mark.is_some()).count(),
                // Maximum wins — the bucket carries its strongest member's
                // signal, which for [`Signal`] is literally its maximum.
                signal: members
                    .iter()
                    .map(|entry| entry.signal)
                    .max()
                    .unwrap_or_default(),
                slot,
                sub: false,
                rect: [0.0; 4],
            });
        }
    }
    let drawn = ticks.len();
    let gap = if drawn > 1 {
        ((block - drawn as f32 * thickness) / (drawn - 1) as f32).max(px(TICK_GAP_MIN_LOGICAL_PX))
    } else {
        folded_gap
    };
    let extent = drawn as f32 * thickness + (drawn - 1) as f32 * gap;
    // `top: 50%; transform: translateY(-50%)` — the *block* is centred, not each
    // tick, which is what makes a rail of three ticks and a rail of thirty read as
    // the same instrument at different densities.
    let top = ((body[1] + body[3]) / 2.0 - extent / 2.0).round();
    let right = (body[2]
        - px(TERMINAL_SCROLL_LANE_LOGICAL_PX + RAIL_LANE_GAP_LOGICAL_PX)
        - px(RAIL_PADDING_X_LOGICAL_PX))
    .round();
    for (index, tick) in ticks.iter_mut().enumerate() {
        let tick_top = (top + index as f32 * (thickness + gap)).round();
        // `.cmdtick.sub { margin-right: 4px }` — the opened group's own column,
        // one step out of the right-aligned one it came from.
        let tick_right = right
            - if tick.sub {
                px(TICK_SUB_OFFSET_LOGICAL_PX)
            } else {
                0.0
            };
        tick.rect = [
            tick_right - px(TICK_LENGTH_LOGICAL_PX),
            tick_top,
            tick_right,
            tick_top + thickness,
        ];
    }
    let last = ticks[ticks.len() - 1].rect;
    // The band, padded on all four sides the way `.cmdrail { padding: 8px 3px }`
    // pads the flex box, and at the resting width its ticks are — see
    // [`Rail::bounds`] and [`Rail::hot_bounds`] for what those two widths are a
    // decision about.
    let band_right = right + px(RAIL_PADDING_X_LOGICAL_PX);
    let band_top = ticks[0].rect[1] - px(RAIL_PADDING_Y_LOGICAL_PX);
    let band_bottom = last[3] + px(RAIL_PADDING_Y_LOGICAL_PX);
    // The four pixels an unfolded member steps out by are held open at every
    // temperature the rail can be bucketed at, so that the hot band's own edge
    // does not move when a bucket opens inside it — a target that resizes because
    // it was pointed at is a target that can shake off the hover that reached it.
    let step_out = if bucket_size > 1 {
        TICK_SUB_OFFSET_LOGICAL_PX
    } else {
        0.0
    };
    Rail {
        bounds: [
            band_right - px(TICK_LENGTH_LOGICAL_PX + RAIL_PADDING_X_LOGICAL_PX * 2.0),
            band_top,
            band_right,
            band_bottom,
        ],
        hot_bounds: [
            band_right
                - px(TICK_CREST_LENGTH_LOGICAL_PX + step_out + RAIL_PADDING_X_LOGICAL_PX * 2.0),
            band_top,
            band_right,
            band_bottom,
        ],
        ticks,
        bucket_size,
        gap,
        scale,
        searching: stack.searching,
    }
}

/// The band that answers the pointer at this temperature — see
/// [`Rail::hot_bounds`] for why there are two of them and why the asymmetry
/// cannot flap.
#[must_use]
pub fn band(rail: &Rail, hot: bool) -> [f32; 4] {
    if hot { rail.hot_bounds } else { rail.bounds }
}

/// Which tick a pointer at `(x, y)` means, or `None` when the pointer is not on
/// the rail at all.
///
/// **Selection first, curve second** (user ruling 2026-07-18, after five rounds of
/// errata): the answer is a function of the *ordinal* the pointer is nearest to,
/// never of the pixels themselves. There is exactly one peak at any moment and no
/// ambiguous state where a pointer resting between two ticks lights both at half
/// strength. [`curve_length`] is applied on top of this answer; it does not
/// replace it.
#[must_use]
pub fn nearest(rail: &Rail, hot: bool, x: f32, y: f32) -> Option<usize> {
    let bounds = band(rail, hot);
    if rail.ticks.is_empty() || x < bounds[0] || x > bounds[2] || y < bounds[1] || y > bounds[3] {
        return None;
    }
    nearest_ordinal(rail, y)
}

/// The same answer with no band test at all — the tick a given height belongs to.
///
/// Split out because [`resolve`] asks it of geometries the pointer has not been
/// admitted to yet: deciding whether a bucket should open means asking the
/// *folded* rail which tick the hand is over, and that rail is a hypothesis rather
/// than the surface under the pointer.
#[must_use]
pub fn nearest_ordinal(rail: &Rail, y: f32) -> Option<usize> {
    let centre = |tick: &Tick| (tick.rect[1] + tick.rect[3]) / 2.0;
    // Ties go to the older tick — the earlier index — so that a pointer exactly on
    // a midpoint always answers the same way rather than depending on iteration.
    rail.ticks
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (centre(a) - y).abs().total_cmp(&(centre(b) - y).abs()))
        .map(|(index, _)| index)
}

/// The rail, the bucket left open, and the crest — all three settled together.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub rail: Rail,
    pub expanded: Option<usize>,
    pub nearest: Option<usize>,
}

/// The vertical zone that keeps bucket `slot` open, or `None` when it is not open
/// in this geometry.
///
/// Half a gap past the outermost member on each side, which is the same boundary
/// [`nearest_ordinal`] would draw between two ticks — so the zone is exactly the
/// union of the members' own dominions and never a hand-picked margin.
#[must_use]
fn group_span(rail: &Rail, slot: usize) -> Option<[f32; 2]> {
    let mut members = rail
        .ticks
        .iter()
        .filter(|tick| tick.sub && tick.slot == slot)
        .peekable();
    let first = members.peek().copied()?;
    let last = members.last()?;
    Some([
        first.rect[1] - rail.gap / 2.0,
        last.rect[3] + rail.gap / 2.0,
    ])
}

/// Settle the fisheye and the crest for a pointer at height `y`.
///
/// # The rule, and why it cannot flap
///
/// *"Members keep the bucket's slot id, so hovering inside the expansion never
/// collapses it (hysteresis); moving to another slot swaps the expansion"* (mock
/// 8440-8442). Two conditions, and they are deliberately asymmetric:
///
/// * **Staying open** is asked of the *open* geometry — the pointer is anywhere
///   inside the unfolded group's own zone, which is `k` ticks tall.
/// * **Opening** is asked of the *folded* geometry — the tick the pointer is
///   nearest to, when the rail is drawn with nothing unfolded, is a bucket.
///
/// The second half is a pure function of `y`: it consults a rail that does not
/// depend on what is currently open, so it cannot disagree with itself. That is
/// what makes the whole thing a fixed point after a single re-layout, and it is
/// where this parts company with the mock-up, whose rule feeds its own output back
/// in and needs `depth < 2` to stop the recursion. A guard on a runaway is not
/// hysteresis: it bounds the work per frame and leaves the *state* free to
/// alternate between frames, which is exactly the boundary flicker the ruling is
/// about. [`FISHEYE_RELAYOUT_CAP`] is kept as the count this reaches, and a test
/// stands a pointer on a group's edge and asserts two consecutive frames agree.
#[must_use]
pub fn resolve(
    body: [f32; 4],
    stack: &Stack,
    scale: f32,
    expanded: Option<usize>,
    y: f32,
) -> Resolved {
    // The folded rail: the hypothesis every question about *opening* is asked of,
    // and the one geometry here that does not depend on what is currently open.
    // It is laid out once, before the loop, because it is the same rail on every
    // pass — which is the same sentence as "this terminates".
    let folded = lay_out(body, stack, scale, None);
    let opens = nearest_ordinal(&folded, y)
        .and_then(|index| (folded.ticks[index].members > 1).then_some(folded.ticks[index].slot));
    let mut open = expanded;
    let mut rail = match open {
        Some(slot) => lay_out(body, stack, scale, Some(slot)),
        None => folded.clone(),
    };
    for _ in 0..FISHEYE_RELAYOUT_CAP {
        let held = open.is_some_and(|slot| {
            group_span(&rail, slot).is_some_and(|span| y >= span[0] && y <= span[1])
        });
        if held || open == opens {
            break;
        }
        open = opens;
        rail = match open {
            Some(slot) => lay_out(body, stack, scale, Some(slot)),
            None => folded.clone(),
        };
    }
    let nearest = nearest_ordinal(&rail, y);
    Resolved {
        rail,
        expanded: open,
        nearest,
    }
}

/// The resting rail: every tick at its own ink, nothing lit.
///
/// [`paint`] with a pointer that has never been anywhere, and written as a
/// function of it rather than beside it so that the picture a cold rail is cached
/// as and the picture a cooling one arrives at are the same code — a resting state
/// that two functions each have their own opinion of is a state that goes
/// subtly wrong the day one of them is edited.
#[must_use]
pub fn build(rail: &Rail, palette: &ChromePalette) -> OverlayLayer {
    paint(
        rail,
        &RailPointer::default(),
        palette,
        Instant::now(),
        Motion::Reduced,
    )
}

/// The three (ink, opacity) pairs one tick travels between.
///
/// Three and not two because the stylesheet's cascade has three levels for a
/// tick — the rule on `.cmdtick`, the one under `.cmdrail.hot`, and the one under
/// `.cmdrail.hot .cmdtick.crest` — and every state this rail can be in is one of
/// those three or a point between two of them. [`paint`] does the travelling; this
/// says where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TickInk {
    /// No pointer on the rail at all.
    pub rest: ([u8; 3], f32),
    /// A hand somewhere on the rail, this tick not the one it singled out.
    pub hot: ([u8; 3], f32),
    /// The tick the pointer selected.
    pub crest: ([u8; 3], f32),
}

/// The stylesheet's whole tick palette, as one table (mock 1370-1374 and
/// 1544-1551).
///
/// # Why one table rather than four `if`s at the paint
///
/// Because there are now eight rules across two modes, and the interesting ones
/// are the *absences*: `.fail` has no `srch-mode` variant in the stylesheet and
/// `.smt` has none outside it. Written as branches those absences become things a
/// reader has to notice; written as a table they become rows, and D-9 — the one
/// place this table deliberately does not follow the cascade — is a row with a
/// comment on it rather than a missing branch.
///
/// [`Signal::Match`] and [`Signal::Current`] cannot occur with `searching` false:
/// a match is a thing only a query produces, and [`commands`] gives every entry
/// one of the other two signals. They fall to the command rows there rather than
/// to a panic, on this file's standing rule that a decoration may not take the
/// window down.
#[must_use]
pub fn tick_ink(signal: Signal, searching: bool, palette: &ChromePalette) -> TickInk {
    // `.cmdtick.fail` / `.cmdrail.hot .cmdtick.fail` / `.cmdrail.hot
    // .cmdtick.crest.fail` — **and the same three while a search is open**, which
    // is D-9: the cascade would swallow the red and the doctrine says it may not.
    // See [`Signal::Fail`].
    if signal == Signal::Fail {
        return TickInk {
            rest: (palette.status_err, TICK_FAIL_REST_OPACITY),
            hot: (palette.status_err, TICK_FAIL_HOT_OPACITY),
            crest: (palette.command_tick_fail_crest, TICK_CREST_OPACITY),
        };
    }
    if !searching {
        return TickInk {
            rest: (palette.command_tick, TICK_REST_OPACITY),
            hot: (palette.accent, TICK_HOT_OPACITY),
            crest: (palette.command_tick_crest, TICK_CREST_OPACITY),
        };
    }
    match signal {
        // `.cmdrail.srch-mode .cmdtick.smt.cur` — the deepened accent at full
        // opacity, **at rest**: *"the current match tick is the deep accent even
        // unhovered"*. All three levels are the same value, which is the
        // stylesheet saying there is nothing above this one to travel to.
        Signal::Current => {
            let deep = (palette.command_tick_crest, TICK_CREST_OPACITY);
            TickInk {
                rest: deep,
                hot: deep,
                crest: deep,
            }
        }
        // `.cmdrail.srch-mode .cmdtick.smt` — lit at rest, because a signal earns
        // its colour without being pointed at.
        Signal::Match => TickInk {
            rest: (palette.accent, TICK_MATCH_REST_OPACITY),
            hot: (palette.accent, TICK_MATCH_HOT_OPACITY),
            crest: (palette.command_tick_crest, TICK_CREST_OPACITY),
        },
        // `.cmdrail.srch-mode .cmdtick` — the commands, receded. The crest is
        // `--ink2` and not the accent: the accent belongs to the matches now, and
        // a command crest borrowing it would say "this is a hit".
        Signal::Command | Signal::Fail => TickInk {
            rest: (palette.command_tick, TICK_SEARCH_REST_OPACITY),
            hot: (palette.command_tick, TICK_SEARCH_HOT_OPACITY),
            crest: (palette.command_tick_search_crest, TICK_SEARCH_CREST_OPACITY),
        },
    }
}

/// The rail at whatever moment its four clocks have reached.
///
/// # The three transitions, and what each of them is a transition *of*
///
/// `transition: width .1s ease, background .14s ease, opacity .12s ease` (mock
/// 1369) is declared on `.cmdtick`, so all three belong to the ticks — the glance
/// card has no transition at all, it is `display: none` and then `display: block`.
/// They compose in the order the stylesheet's cascade does:
///
/// * the **rail** warms from its resting ink to its hot one (a failure from its
///   red at `.6` to the same red at `.65` — *"signals earn permanent colour"*, so
///   it moves in opacity only);
/// * the **crest** then deepens out of whatever that left;
/// * every tick's **length** travels along [`curve_length`] toward its distance
///   from the crest.
///
/// The three ends of those travels come out of [`tick_ink`], which is where the
/// stylesheet's two modes live. Nothing below knows what a match is.
///
/// *"Width only grows leftward, so vertical layout never shifts and there is no
/// jitter"* (mock 1367-1368): the right edge of every quad here is the right edge
/// of the resting tick, and all the growth opens into the pane.
#[must_use]
pub fn paint(
    rail: &Rail,
    pointer: &RailPointer,
    palette: &ChromePalette,
    now: Instant,
    motion: Motion,
) -> OverlayLayer {
    let radius = TICK_RADIUS_LOGICAL_PX * rail.scale;
    let widths = pointer.width.sample(now, motion);
    let crest_ink = pointer.crest_ink.sample(now, motion);
    let crest_alpha = pointer.crest_alpha.sample(now, motion);
    let (hot_ink, _) = pointer.hot_ink.sample(now, motion);
    let (hot_alpha, _) = pointer.hot_alpha.sample(now, motion);
    OverlayLayer {
        quads: rail
            .ticks
            .iter()
            .enumerate()
            .flat_map(|(index, tick)| {
                let TickInk {
                    rest: (rest, rest_alpha),
                    hot: (warm, warm_alpha),
                    crest: (peak, peak_alpha),
                } = tick_ink(tick.signal, rail.searching, palette);
                let deepen = crest_ink.get(index).copied().unwrap_or(0.0);
                let lift = crest_alpha.get(index).copied().unwrap_or(0.0);
                let ink = mix(mix(rest, warm, hot_ink), peak, deepen);
                let alpha = lerp(lerp(rest_alpha, warm_alpha, hot_alpha), peak_alpha, lift);
                let length =
                    widths.get(index).copied().unwrap_or(TICK_LENGTH_LOGICAL_PX) * rail.scale;
                rounded_overlay_fill(
                    [
                        tick.rect[2] - length,
                        tick.rect[1],
                        tick.rect[2],
                        tick.rect[3],
                    ],
                    radius,
                    ink,
                    alpha,
                )
            })
            .collect(),
        ..OverlayLayer::default()
    }
}

/// The box the glance card is placed against: the crest, at its full length.
///
/// The *target* length rather than the tweened one, so that a card settling
/// beside a tick that is still growing does not slide the last twelve pixels with
/// it. The mock-up reads `nearest.getBoundingClientRect()` after the widths are
/// written and gets the same answer for the same reason.
#[must_use]
pub fn peek_host(rail: &Rail, index: usize) -> Option<[f32; 4]> {
    let tick = rail.ticks.get(index)?;
    Some([
        tick.rect[2] - TICK_CREST_LENGTH_LOGICAL_PX * rail.scale,
        tick.rect[1],
        tick.rect[2],
        tick.rect[3],
    ])
}

/// What the glance card says about a **match** tick whose line no longer has any
/// text to quote.
///
/// The mirror of [`PEEK_EMPTY_TEXT`], and it exists for the same reason: a hit on
/// a line the transcript has since evicted is a real state, and a blank card would
/// read as the card being broken rather than as the line being gone.
pub const PEEK_EMPTY_LINE_TEXT: &str = "line";

/// A command's run time, in the coarsest form that is still true.
///
/// The ladder is the git panel's ([`crate::git::relative_time`]) with the calendar
/// half cut off, because a command is not a commit: it spans seconds to hours, so
/// `{n}s`, `{n}m`, `{n}h` and nothing below or above.
///
/// **Under a second says nothing at all.** Most commands are instant, and a card
/// reading `ls · 0s` would spend a third of its width to tell the reader that
/// nothing happened; `437ms` is worse, because it is a number nobody glances at.
/// Silence here is the same silence the exit code keeps for zero — see
/// [`peek_text`] — and it means the same thing: there is nothing here worth a
/// reader's eye.
///
/// The unit boundary truncates rather than rounds, so a label never claims a
/// minute that has not elapsed: 119 seconds is `1m`, not `2m`.
#[must_use]
pub fn peek_duration(elapsed: Duration) -> Option<String> {
    let seconds = elapsed.as_secs();
    if seconds < 1 {
        return None;
    }
    if seconds < 60 {
        return Some(format!("{seconds}s"));
    }
    if seconds < 60 * 60 {
        return Some(format!("{}m", seconds / 60));
    }
    Some(format!("{}h", seconds / (60 * 60)))
}

/// What the glance card says about one tick, and whether it says it in the muted
/// ink.
///
/// # The readings
///
/// * a **single command tick** — the command, then what became of it:
///   `"cargo test{sep}42s{sep}exit 1"`. Both suffixes are conditional (below);
///   a fast command that succeeded is still the bare command line it always was.
/// * a **failed** command adds a **second line**: the last non-empty line of its
///   own output, as `output_tail` gives it. That line is where the error is,
///   essentially always, and a card that made the reader open the pane to find
///   out *what* failed would be a card that stopped one word short. Success cards
///   stay one line: there is nothing there a reader wants.
/// * a command that is still running — `"running{sep}{cmd}"`, unchanged. There is
///   no status to report and no duration either: [`bt_term::CommandMark::duration`]
///   deliberately refuses to answer with the clock, and a card whose number moved
///   while it was being read would be churn rather than information.
/// * [`PEEK_EMPTY_TEXT`] in the muted ink when the ledger never got the text —
///   which since the adapter began pausing at shell-integration markers means only
///   a command typed past the top of its own grid (`session.rs`'s
///   `absorb_command_text`). The *facts* still ride along: a card that withheld
///   `exit 1` because the text was missing would be punishing the reader for the
///   ledger's gap, and the muted ink already says which half of the card is the gap.
/// * a **single match tick** (S4) — the matched line, as `line_text` gives it.
///   Not the query and not the match's own substring: the reader knows what they
///   typed, and what they are choosing between is the *lines*.
/// * a **folded bucket** — `"{k} {noun}{sep}latest: {text}"` (mock 8468). The
///   latest body is the same body a single card would show, suffixes and all; the
///   quoted output line is not, because a bucket is a *count* and a card that grew
///   a second line per fold would stop being one.
///
/// # `exit 0` is not written, and that is a ruling
///
/// A zero exit is the absence of news, and the rail has already said it in the one
/// place it is cheapest to read: the tick is not red. Writing it would also make
/// silence ambiguous — `D` can arrive with no status parameter at all
/// (`exit_code: None`), and that genuinely-unknown case must be silent, so
/// silence cannot also be made to mean "zero" without the two collapsing into one
/// another. What the card says is therefore: nothing about the status unless the
/// status was bad.
///
/// # The command is flattened to one line
///
/// A witness spans hard row boundaries — a here-string, a PowerShell continuation
/// — and those arrive as `\n`. The card's second line belongs to the quoted error,
/// so a command's own row breaks become spaces rather than lines. The transcript
/// text is not otherwise touched: this is a glance card made of the terminal's own
/// words, not a paraphrase of them.
///
/// # The noun (B36), and the case the mock-up does not have
///
/// The prototype's noun is a two-way switch: `srch-mode ? "lines" : "commands"`.
/// It can afford that because its buckets are homogeneous by accident of how it
/// counts — but ours genuinely are not, since the merged stack interleaves the two
/// sources and a bucket of three can hold two commands and a match. So the rule is
/// stated over the counts the tick already carries:
///
/// * all commands — `"{m} commands"`, the prototype's non-search reading;
/// * all matches — `"{k} lines"`, the prototype's search reading;
/// * both — `"{k} lines, {m} commands"`, which is the reading the prototype has no
///   case for.
///
/// **Lines are named first** in the mixed reading, which is the ticket's *"ties go
/// to lines while searching"* landing where it can still be seen: a bucket is
/// mixed only while a search is open, and while a search is open the lines are
/// what the reader came for. And there is no majority to take, because naming both
/// counts leaves a majority nothing to decide — the more specific rule wins, and it
/// is the honest one: a card that said "3 lines" over a bucket holding one would be
/// wrong in the only way a glance card can be wrong.
///
/// `k + m` may exceed the member count, and that is not an error either: one line
/// can be both a prompt row and a match, and it is counted in both because it *is*
/// both.
#[must_use]
pub fn peek_text(
    tick: &Tick,
    marks: &[CommandMark],
    line_text: Option<&str>,
    output_tail: Option<&str>,
) -> (String, bool) {
    let mark = match tick.target {
        Target::Command(id) => marks.iter().find(|candidate| candidate.id == id),
        Target::Match(_) => None,
    };
    let (mut body, muted) = match tick.target {
        Target::Match(_) => {
            let text = line_text.unwrap_or("").trim();
            if text.is_empty() {
                (PEEK_EMPTY_LINE_TEXT.to_owned(), true)
            } else {
                (text.to_owned(), false)
            }
        }
        Target::Command(_) => {
            let text = mark.map_or("", |mark| mark.command_text.trim());
            if text.is_empty() {
                (PEEK_EMPTY_TEXT.to_owned(), true)
            } else {
                (text.replace('\n', " "), false)
            }
        }
    };
    // The two suffixes, in the order they happened: how long it took, then how it
    // ended. A running command has neither and is spelled by its own prefix below.
    if let Some(mark) = mark.filter(|mark| !mark.is_running()) {
        if let Some(elapsed) = mark.duration().and_then(peek_duration) {
            body.push_str(NAME_PLACE_SEPARATOR);
            body.push_str(&elapsed);
        }
        if let Some(code) = mark.exit_code.filter(|code| *code != 0) {
            body.push_str(NAME_PLACE_SEPARATOR);
            body.push_str(&format!("exit {code}"));
        }
    }
    if tick.members > 1 {
        let count = match (tick.matched, tick.commanded) {
            (0, commanded) => format!("{commanded} commands"),
            (matched, 0) => format!("{matched} lines"),
            (matched, commanded) => format!("{matched} lines, {commanded} commands"),
        };
        return (
            format!("{count}{NAME_PLACE_SEPARATOR}latest: {body}"),
            muted,
        );
    }
    // *"running · {cmd}"* — a command with no `D` yet. Only ever a command: a
    // matched line has no lifetime of its own to report.
    if mark.is_some_and(CommandMark::is_running) {
        return (format!("running{NAME_PLACE_SEPARATOR}{body}"), muted);
    }
    // The quoted error, on its own line. Only for a command the shell called
    // failed, and only when there is a line to quote.
    if mark.is_some_and(CommandMark::failed)
        && let Some(tail) = output_tail.map(str::trim).filter(|tail| !tail.is_empty())
    {
        body.push('\n');
        body.push_str(&tail.replace('\n', " "));
    }
    (body, muted)
}

/// Two inks, `t` of the way from the first to the second.
fn mix(from: [u8; 3], to: [u8; 3], t: f32) -> [u8; 3] {
    std::array::from_fn(|channel| {
        lerp(f32::from(from[channel]), f32::from(to[channel]), t)
            .round()
            .clamp(0.0, 255.0) as u8
    })
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t.clamp(0.0, 1.0)
}

/// How strong the jump flash is `elapsed` after it began, or `None` once it is
/// over.
///
/// `curve` is the house `EASE`, which is CSS's own `ease` and therefore the
/// literal reading of `animation: cmdflash .95s ease`. It is passed in rather than
/// imported so that this file has no opinion about the window's timing and the
/// test can state the curve it is asserting.
///
/// **No reduced-motion branch, deliberately.** The mock-up declares five
/// `prefers-reduced-motion` overrides and none of them is this one, which is the
/// stylesheet saying that a fading row band is not the kind of motion that rule is
/// about: it does not travel, and what it carries is a *reading* — which of a
/// thousand rows the jump landed on — rather than polish. Killing it would leave a
/// silent scroll with nothing to point at the answer.
#[must_use]
pub fn flash_alpha(elapsed: Duration, curve: impl FnOnce(f32) -> f32) -> Option<f32> {
    if !flash_is_running(elapsed) {
        return None;
    }
    let progress = elapsed.as_secs_f32() / JUMP_FLASH.as_secs_f32();
    Some(JUMP_FLASH_OPACITY * (1.0 - curve(progress)))
}

/// Whether a flash that began `elapsed` ago is still owed a frame.
///
/// The **same** predicate [`flash_alpha`] answers `None` by, deliberately shared
/// rather than restated: the deadline that wakes the loop and the paint that runs
/// when it does have to agree exactly, or the window either spins for ever on a
/// flash that draws nothing or stops waking one frame before it has finished
/// fading — and the second of those is invisible until someone looks closely at a
/// band that never quite reaches transparent.
#[must_use]
pub fn flash_is_running(elapsed: Duration) -> bool {
    elapsed < JUMP_FLASH
}

/// The band a jump paints across the row it landed on.
///
/// A **full-width** quad on the row, because the mock-up's `.cmd-jump` is a
/// background on the logical line's own `div` and a `div` in a terminal is the
/// whole width of it. Clipped to the pane's body by the caller: a row partly
/// scrolled off the top must flash the part that is on screen and not a strip of
/// the pane head above it.
#[must_use]
pub fn flash_quads(
    row: [f32; 4],
    alpha: f32,
    scale: f32,
    palette: &ChromePalette,
) -> Vec<OverlayQuad> {
    if row[2] <= row[0] || row[3] <= row[1] {
        return Vec::new();
    }
    rounded_overlay_fill(
        row,
        JUMP_FLASH_RADIUS_LOGICAL_PX * scale,
        palette.accent,
        alpha,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_doc::AnchorId;

    const EASE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];

    fn mark(id: u64, exit_code: Option<i32>) -> CommandMark {
        CommandMark {
            id: CommandMarkId(id),
            prompt: Some(AnchorId(id * 10)),
            start: AnchorId(id * 10 + 1),
            executed: None,
            finished: exit_code.map(|_| AnchorId(id * 10 + 3)),
            command_text: String::new(),
            exit_code,
            executed_at: None,
            finished_at: None,
        }
    }

    fn ok(id: u64) -> CommandMark {
        mark(id, Some(0))
    }

    fn failed(id: u64) -> CommandMark {
        mark(id, Some(1))
    }

    fn said(id: u64, text: &str) -> CommandMark {
        CommandMark {
            command_text: text.to_owned(),
            ..ok(id)
        }
    }

    /// A pane 800 physical pixels tall at scale 1, which makes `avail` 640 and
    /// every number below readable against the mock-up's own arithmetic.
    const BODY: [f32; 4] = [100.0, 50.0, 500.0, 850.0];

    fn rail_of(marks: &[CommandMark]) -> Rail {
        lay_out(BODY, &commands(marks), 1.0, None)
    }

    /// The command a tick jumps to, for the tests written before the stack had a
    /// second source.
    fn mark_of(tick: &Tick) -> CommandMarkId {
        match tick.target {
            Target::Command(mark) => mark,
            Target::Match(hit) => panic!("tick {hit} is a match, not a command"),
        }
    }

    fn is_failed(tick: &Tick) -> bool {
        tick.signal == Signal::Fail
    }

    /// The x every pointer test uses: the middle of the resting band, which is
    /// inside the hot one too.
    fn on_band(rail: &Rail) -> f32 {
        (rail.bounds[0] + rail.bounds[2]) / 2.0
    }

    fn centre(tick: &Tick) -> f32 {
        (tick.rect[1] + tick.rect[3]) / 2.0
    }

    /// A full-screen program owns its canvas, and the rail steps off it.
    ///
    /// MUTATION: return the body unconditionally and a pane running `vim` wears a
    /// column of ticks about commands that ran before `vim` started, beside rows
    /// they say nothing about.
    #[test]
    fn a_pane_showing_the_alternate_screen_hangs_no_rail_on_it() {
        assert_eq!(host_rect(BODY, false), Some(BODY));
        assert_eq!(host_rect(BODY, true), None);
    }

    #[test]
    fn a_pane_with_no_command_marks_draws_nothing_at_all() {
        let rail = rail_of(&[]);
        assert!(rail.ticks.is_empty());
        assert_eq!(rail.bounds, [0.0; 4]);
        assert_eq!(rail.hot_bounds, [0.0; 4]);
        assert!(build(&rail, &bt_render::chrome_palette()).quads.is_empty());
        assert_eq!(nearest(&rail, false, 490.0, 450.0), None);
        assert_eq!(nearest(&rail, true, 490.0, 450.0), None);
    }

    /// The ordinal stack: one tick per command, oldest at the top, the whole block
    /// centred on the pane's own middle.
    #[test]
    fn a_handful_of_marks_becomes_an_evenly_spaced_block_centred_on_the_pane() {
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let rail = rail_of(&marks);
        assert_eq!(rail.ticks.len(), 5);
        assert_eq!(rail.bucket_size, 1);
        // `avail / N` = 128, capped at the nine-pixel ceiling, so pitch is 9 and
        // gap is 7 — the stylesheet's own resting `gap: 7px`.
        let pitch: Vec<f32> = rail
            .ticks
            .windows(2)
            .map(|pair| pair[1].rect[1] - pair[0].rect[1])
            .collect();
        assert_eq!(pitch, vec![9.0; 4]);
        assert_eq!(rail.gap, 7.0);
        assert!(
            rail.ticks
                .iter()
                .all(|tick| tick.rect[3] - tick.rect[1] == 2.0)
        );
        assert!(
            rail.ticks
                .iter()
                .all(|tick| tick.rect[2] - tick.rect[0] == TICK_LENGTH_LOGICAL_PX)
        );
        // Oldest at the top.
        assert_eq!(mark_of(&rail.ticks[0]), CommandMarkId(1));
        assert_eq!(mark_of(&rail.ticks[4]), CommandMarkId(5));
        // 5 ticks × 2px + 4 gaps × 7px = 38, centred on the body's middle of 450.
        let block = (rail.ticks[0].rect[1], rail.ticks[4].rect[3]);
        assert_eq!(block, (431.0, 469.0));
        // One tick per command means slot and index agree, and nothing is a member
        // of an expansion.
        assert!(
            rail.ticks
                .iter()
                .enumerate()
                .all(|(index, tick)| tick.slot == index && !tick.sub)
        );
    }

    /// The right inset is the lane plus its gap plus the rail's own padding, and it
    /// is *derived* — the test states the derivation rather than the number, so
    /// widening the lane moves this assertion with the pixels.
    #[test]
    fn the_ticks_sit_inboard_of_the_reserved_scroll_lane() {
        let rail = lay_out(BODY, &commands(&[ok(1)]), 2.0, None);
        let inset = BODY[2] - rail.ticks[0].rect[2];
        assert_eq!(
            inset,
            (TERMINAL_SCROLL_LANE_LOGICAL_PX
                + RAIL_LANE_GAP_LOGICAL_PX
                + RAIL_PADDING_X_LOGICAL_PX)
                * 2.0
        );
        assert!(
            rail.ticks[0].rect[2] + TERMINAL_SCROLL_LANE_LOGICAL_PX * 2.0 <= BODY[2],
            "a thumb in its own lane must never meet a tick"
        );
    }

    /// Past the four-pixel density floor the rail aggregates: one tick per bucket
    /// of `k`, with `k = ceil(N / capacity)`.
    #[test]
    fn more_commands_than_the_block_can_hold_are_bucketed_at_the_density_floor() {
        // avail 640 ⇒ capacity 160.
        let marks: Vec<_> = (1..=400).map(ok).collect();
        let rail = rail_of(&marks);
        assert_eq!(rail.bucket_size, 3, "ceil(400 / 160)");
        assert_eq!(rail.ticks.len(), 134, "ceil(400 / 3)");
        assert_eq!(rail.ticks[0].members, 3);
        assert_eq!(rail.ticks[133].members, 1, "400 = 133×3 + 1");
        // A bucket jumps to its newest member.
        assert_eq!(mark_of(&rail.ticks[0]), CommandMarkId(3));
        assert_eq!(mark_of(&rail.ticks[1]), CommandMarkId(6));
        // Under the floor nothing is bucketed at all.
        let few: Vec<_> = (1..=160).map(ok).collect();
        assert_eq!(rail_of(&few).bucket_size, 1);
    }

    /// **B15, corrected.** The pitch comes from the ticks that are drawn, so a
    /// dense rail fills the four fifths of the pane it was given instead of the
    /// sixty-two per cent of them the mock-up's own arithmetic leaves it at.
    ///
    /// MUTATION: divide `avail` by `marks.len()` instead of by the drawn count and
    /// the block collapses to 398 of its 640 pixels — the mock-up's bug, reproduced
    /// in S1 on purpose and answered here.
    #[test]
    fn two_hundred_commands_fill_the_block_they_were_given_rather_than_five_eighths_of_it() {
        let marks: Vec<_> = (1..=200).map(ok).collect();
        let rail = rail_of(&marks);
        // capacity 160 ⇒ k = 2 ⇒ 100 ticks drawn.
        assert_eq!(rail.bucket_size, 2);
        assert_eq!(rail.ticks.len(), 100);
        let avail = (BODY[3] - BODY[1]) * RAIL_BLOCK_FRACTION;
        assert_eq!(avail, 640.0);
        let block = rail.ticks[99].rect[3] - rail.ticks[0].rect[1];
        assert!(
            block / avail > 0.98,
            "the drawn block fills its allowance: {block} of {avail}"
        );
        // And the mock-up's own number, stated so the correction is legible: the
        // uncorrected pitch is the four-pixel floor, which is a 398-pixel block.
        let mock_pitch = (avail / marks.len() as f32).clamp(4.0, 9.0);
        let mock_block = 100.0 * 2.0 + 99.0 * (mock_pitch - 2.0).max(2.0);
        assert_eq!(mock_block, 398.0);
        assert!(block > mock_block * 1.5);
    }

    /// Maximum wins: one failure anywhere in a bucket colours the whole tick, and
    /// it colours it at rest.
    #[test]
    fn a_bucket_holding_one_failure_stays_red_without_being_pointed_at() {
        let mut marks: Vec<_> = (1..=400).map(ok).collect();
        marks[4] = failed(5);
        let rail = rail_of(&marks);
        assert_eq!(rail.bucket_size, 3);
        assert!(
            is_failed(&rail.ticks[1]),
            "marks 4,5,6 — the middle one failed"
        );
        assert!(!is_failed(&rail.ticks[0]));
        assert!(!is_failed(&rail.ticks[2]));
        let palette = bt_render::chrome_palette();
        let layer = build(&rail, &palette);
        assert!(
            layer
                .quads
                .iter()
                .any(|quad| quad.color == palette.status_err),
            "the failure earns its colour with no pointer anywhere near it"
        );
    }

    /// A command still in flight — `D` never arrived — has no exit status, and no
    /// exit status is not a failure.
    #[test]
    fn a_command_still_running_is_not_a_failed_command() {
        let rail = rail_of(&[mark(1, None)]);
        assert_eq!(rail.ticks.len(), 1, "the tick is drawn all the same");
        assert!(!is_failed(&rail.ticks[0]));
    }

    /// The pointer picks an ordinal, not a pixel: one peak, ties to the older tick,
    /// and nothing at all off the band.
    #[test]
    fn the_rail_answers_with_the_tick_nearest_the_pointer_and_only_on_its_own_band() {
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let rail = rail_of(&marks);
        assert_eq!(
            rail.bounds[2] - rail.bounds[0],
            TICK_LENGTH_LOGICAL_PX + RAIL_PADDING_X_LOGICAL_PX * 2.0
        );
        assert!(
            rail.ticks.iter().all(|tick| tick.rect[0] >= rail.bounds[0]
                && tick.rect[2] <= rail.bounds[2]
                && tick.rect[1] >= rail.bounds[1]
                && tick.rect[3] <= rail.bounds[3]),
            "every resting tick is inside the band that answers for it"
        );
        let x = on_band(&rail);
        for index in 0..5 {
            assert_eq!(
                nearest(&rail, false, x, centre(&rail.ticks[index])),
                Some(index)
            );
            // Four pixels off a tick is still that tick — the gap is only seven.
            assert_eq!(
                nearest(&rail, false, x, centre(&rail.ticks[index]) + 3.0),
                Some(index)
            );
        }
        // The pane's own body is not the rail.
        let midpoint = (rail.ticks[0].rect[3] + rail.ticks[1].rect[1]) / 2.0;
        assert_eq!(nearest(&rail, false, rail.bounds[0] - 1.0, midpoint), None);
        assert_eq!(nearest(&rail, false, x, rail.bounds[1] - 1.0), None);
        assert_eq!(nearest(&rail, false, x, rail.bounds[3] + 1.0), None);
        // Inside the band but above every tick still answers the topmost: the band
        // is padded, and a press on its padding is a press on the rail.
        assert_eq!(nearest(&rail, false, x, rail.bounds[1] + 1.0), Some(0));
    }

    /// **The exact midpoint yields exactly one crest.** The ruling's own sentence,
    /// as an assertion: "指针停在两条之间不出现并列半高的含糊态".
    ///
    /// MUTATION: light every tick within half a pitch and a pointer parked on a
    /// boundary lights two at once, which is the ambiguity five rounds of errata
    /// were spent removing.
    #[test]
    fn a_pointer_resting_exactly_between_two_ticks_still_lights_exactly_one() {
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let rail = rail_of(&marks);
        let x = on_band(&rail);
        for pair in 0..4 {
            let midpoint = (centre(&rail.ticks[pair]) + centre(&rail.ticks[pair + 1])) / 2.0;
            // Ties go to the older tick, every time and not by iteration order.
            assert_eq!(nearest(&rail, false, x, midpoint), Some(pair));
            let mut pointer = RailPointer::default();
            let now = Instant::now();
            pointer.aim(rail.ticks.len(), Some(pair), true, now, Motion::Reduced);
            let widths = pointer.width.sample(now, Motion::Reduced);
            assert_eq!(
                widths
                    .iter()
                    .filter(|width| **width == TICK_CREST_LENGTH_LOGICAL_PX)
                    .count(),
                1,
                "one crest and no half heights: {widths:?}"
            );
        }
    }

    /// The mock-up's own five widths, to the tenth of a pixel it prints them at.
    #[test]
    fn the_magnification_curve_is_the_mock_ups_cosine_and_reaches_three_ordinals() {
        for (delta, expected) in [(0, 27.0), (1, 22.5), (2, 13.5), (3, 9.0)] {
            for signed in [delta, -delta] {
                assert!(
                    (curve_length(signed) - expected).abs() < 0.05,
                    "Δ{signed} is {} and should be {expected}",
                    curve_length(signed)
                );
            }
        }
        // Past the reach the curve has already returned to the resting length and
        // stays there — `u` is clamped, so a rail of a hundred ticks costs nothing
        // beyond the five that take part.
        for delta in [4, 40, 400] {
            assert_eq!(curve_length(delta), TICK_LENGTH_LOGICAL_PX);
        }
        assert_eq!(curve_length(0), TICK_CREST_LENGTH_LOGICAL_PX);
    }

    /// **`hot` is a rail-level state.** Every tick changes ink, not just the one
    /// under the pointer.
    ///
    /// MUTATION: colour only the crest and the rail stops answering the hand as a
    /// single instrument — "hovering the rail colours them" becomes "hovering the
    /// rail colours one of them".
    #[test]
    fn pointing_at_the_rail_colours_the_whole_rail_and_not_one_tick_of_it() {
        let palette = bt_render::chrome_palette();
        let mut marks: Vec<_> = (1..=5).map(ok).collect();
        marks[3] = failed(4);
        let rail = rail_of(&marks);
        let now = Instant::now();
        let cold = build(&rail, &palette);
        assert!(
            cold.quads
                .iter()
                .all(|quad| quad.color == palette.command_tick || quad.color == palette.status_err),
            "at rest the ticks are grey and a failure is red"
        );
        let mut pointer = RailPointer::default();
        pointer.aim(rail.ticks.len(), Some(1), true, now, Motion::Reduced);
        let hot = paint(&rail, &pointer, &palette, now, Motion::Reduced);
        // Every ordinary tick is accent, including the four the pointer is not on.
        assert!(hot.quads.iter().any(|quad| quad.color == palette.accent));
        for (index, tick) in rail.ticks.iter().enumerate() {
            let quad = hot
                .quads
                .iter()
                .find(|quad| quad.rect[1] >= tick.rect[1] && quad.rect[3] <= tick.rect[3])
                .expect("every tick draws");
            let expected = match (index == 1, is_failed(tick)) {
                (true, true) => palette.command_tick_fail_crest,
                (true, false) => palette.command_tick_crest,
                (false, true) => palette.status_err,
                (false, false) => palette.accent,
            };
            assert_eq!(quad.color, expected, "tick {index}");
        }
        // The crest is opaque; the rest of the rail lifts to .55, and a failure to
        // its own .65.
        let alpha_of = |index: usize| {
            let tick = rail.ticks[index].rect;
            hot.quads
                .iter()
                .find(|quad| quad.rect[1] >= tick[1] && quad.rect[3] <= tick[3])
                .expect("every tick draws")
                .alpha
        };
        assert!((alpha_of(1) - TICK_CREST_OPACITY).abs() < 0.001);
        assert!((alpha_of(0) - TICK_HOT_OPACITY).abs() < 0.001);
        assert!((alpha_of(3) - TICK_FAIL_HOT_OPACITY).abs() < 0.001);
    }

    /// The crest covers its own resting tick exactly, and the skirt lifts its
    /// neighbours — all of it leftward, none of it downward.
    #[test]
    fn the_crest_and_its_neighbours_grow_leftward_over_the_ticks_they_replace() {
        let palette = bt_render::chrome_palette();
        let marks: Vec<_> = (1..=6).map(ok).collect();
        let rail = rail_of(&marks);
        let now = Instant::now();
        let mut pointer = RailPointer::default();
        pointer.aim(rail.ticks.len(), Some(2), true, now, Motion::Reduced);
        let layer = paint(&rail, &pointer, &palette, now, Motion::Reduced);
        for (index, tick) in rail.ticks.iter().enumerate() {
            let quads: Vec<_> = layer
                .quads
                .iter()
                .filter(|quad| quad.rect[1] >= tick.rect[1] && quad.rect[3] <= tick.rect[3])
                .collect();
            let left = quads
                .iter()
                .map(|quad| quad.rect[0])
                .fold(f32::INFINITY, f32::min);
            let right = quads
                .iter()
                .map(|quad| quad.rect[2])
                .fold(f32::NEG_INFINITY, f32::max);
            assert_eq!(
                right, tick.rect[2],
                "tick {index}: the right edge never moves"
            );
            // Within a pixel: the quads are snapped to the grid on their way out
            // of `rounded_overlay_fill`, and the curve's own halves (22.5, 13.5)
            // cannot survive that exactly.
            assert!(
                (right - left - curve_length(index as i64 - 2)).abs() <= 1.0,
                "tick {index} is {} long",
                right - left
            );
            assert!(
                quads
                    .iter()
                    .all(|quad| quad.rect[1] >= tick.rect[1] && quad.rect[3] <= tick.rect[3]),
                "thickness stays 2px — only the length answers the pointer"
            );
        }
    }

    /// **The band grows only while the rail is hot** (D-17's middle path).
    ///
    /// MUTATION: return the hot band unconditionally and thirty-seven logical
    /// pixels of every pane's right edge stop underlining hyperlinks for a reader
    /// who never went near the rail. Return the resting one unconditionally and a
    /// hand walking onto the crest it just raised drops off the rail.
    #[test]
    fn the_band_takes_the_crests_width_only_while_the_rail_is_already_hot() {
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let rail = rail_of(&marks);
        assert_eq!(
            rail.bounds[2] - rail.bounds[0],
            TICK_LENGTH_LOGICAL_PX + RAIL_PADDING_X_LOGICAL_PX * 2.0
        );
        assert_eq!(
            rail.hot_bounds[2] - rail.hot_bounds[0],
            TICK_CREST_LENGTH_LOGICAL_PX + RAIL_PADDING_X_LOGICAL_PX * 2.0
        );
        assert_eq!(
            rail.bounds[2], rail.hot_bounds[2],
            "the right edge is one edge"
        );
        assert_eq!(
            [rail.bounds[1], rail.bounds[3]],
            [rail.hot_bounds[1], rail.hot_bounds[3]]
        );
        // A point over the drawn crest but outside the resting box: nothing while
        // cold, the crest's own tick while hot.
        let y = centre(&rail.ticks[2]);
        let on_crest = rail.bounds[0] - 4.0;
        assert!(on_crest > rail.hot_bounds[0]);
        assert_eq!(nearest(&rail, false, on_crest, y), None);
        assert_eq!(nearest(&rail, true, on_crest, y), Some(2));
        // Leaving the *larger* box is what cools it, so the two thresholds are
        // never the same pixel and the growth cannot flap.
        assert_eq!(nearest(&rail, true, rail.hot_bounds[0] - 1.0, y), None);
        // A bucketed rail holds the unfolded members' four-pixel step open too.
        let dense: Vec<_> = (1..=400).map(ok).collect();
        let dense = rail_of(&dense);
        assert_eq!(
            dense.hot_bounds[2] - dense.hot_bounds[0],
            TICK_CREST_LENGTH_LOGICAL_PX
                + TICK_SUB_OFFSET_LOGICAL_PX
                + RAIL_PADDING_X_LOGICAL_PX * 2.0
        );
    }

    /// Pointing at a bucket opens it: its members become ticks of their own, one
    /// step out of the column, and the neighbours give up the room.
    #[test]
    fn a_bucket_under_the_pointer_unfolds_into_its_members_and_the_block_holds_its_extent() {
        let marks: Vec<_> = (1..=400).map(ok).collect();
        let folded = rail_of(&marks);
        assert_eq!(folded.bucket_size, 3);
        let open = lay_out(BODY, &commands(&marks), 1.0, Some(7));
        // Two more ticks than the folded rail: one bucket of three became three.
        assert_eq!(open.ticks.len(), folded.ticks.len() + 2);
        let members: Vec<_> = open.ticks.iter().filter(|tick| tick.sub).collect();
        assert_eq!(members.len(), 3);
        assert!(
            members
                .iter()
                .all(|tick| tick.slot == 7 && tick.members == 1),
            "members keep the bucket's slot id — that is what the hysteresis is keyed on"
        );
        assert_eq!(
            members.iter().map(|tick| mark_of(tick)).collect::<Vec<_>>(),
            vec![CommandMarkId(22), CommandMarkId(23), CommandMarkId(24)],
            "slot 7 of buckets of three is commands 22, 23 and 24"
        );
        // One width base for everyone: an unfolded member is the same length as
        // any other tick, moved four pixels left.
        assert!(
            members
                .iter()
                .all(|tick| tick.rect[2] - tick.rect[0] == TICK_LENGTH_LOGICAL_PX)
        );
        let column = folded.ticks[0].rect[2];
        assert!(
            members
                .iter()
                .all(|tick| tick.rect[2] == column - TICK_SUB_OFFSET_LOGICAL_PX)
        );
        assert!(
            open.ticks
                .iter()
                .filter(|tick| !tick.sub)
                .all(|tick| tick.rect[2] == column)
        );
        // Neighbours compress: the block keeps its extent rather than growing by
        // two more rows.
        let extent = |rail: &Rail| rail.ticks[rail.ticks.len() - 1].rect[3] - rail.ticks[0].rect[1];
        assert!(
            (extent(&open) - extent(&folded)).abs() <= 1.0,
            "folded {} vs open {}",
            extent(&folded),
            extent(&open)
        );
        assert!(open.gap < folded.gap, "the neighbours gave the room up");
    }

    /// **Hysteresis.** A pointer standing on the boundary of an open group answers
    /// the same on the next frame, and on the frame after that.
    ///
    /// MUTATION: decide the expansion from the *open* geometry's nearest tick and
    /// the rail alternates between folded and unfolded on consecutive frames — the
    /// mock-up's own recursion guard bounds the work and leaves precisely this
    /// flicker in place.
    #[test]
    fn the_unfolded_bucket_does_not_flap_when_the_pointer_sits_on_its_boundary() {
        let marks: Vec<_> = (1..=400).map(ok).collect();
        let folded = rail_of(&marks);
        // Every boundary of every drawn tick, and the two edges of the group that
        // opens under each — the exhaustive version of "a pointer on the seam".
        for slot in [0_usize, 1, 7, 60, 133] {
            let seed = centre(&folded.ticks[slot]);
            let first = resolve(BODY, &commands(&marks), 1.0, None, seed);
            let mut state = first.expanded;
            let mut rail = first.rail.clone();
            for frame in 0..4 {
                let next = resolve(BODY, &commands(&marks), 1.0, state, seed);
                assert_eq!(
                    next.expanded, first.expanded,
                    "slot {slot} changed its mind on frame {frame}"
                );
                assert_eq!(next.rail, rail, "slot {slot} redrew on frame {frame}");
                state = next.expanded;
                rail = next.rail;
            }
            let Some(open) = first.expanded else { continue };
            let span = group_span(&first.rail, open).expect("the group is open");
            for edge in [
                span[0],
                span[1],
                span[0] - 0.5,
                span[1] + 0.5,
                (span[0] + span[1]) / 2.0,
            ] {
                let once = resolve(BODY, &commands(&marks), 1.0, Some(open), edge);
                let twice = resolve(BODY, &commands(&marks), 1.0, once.expanded, edge);
                assert_eq!(
                    once.expanded, twice.expanded,
                    "slot {open} flapped at {edge}"
                );
                assert_eq!(once.rail, twice.rail);
                assert_eq!(once.nearest, twice.nearest);
            }
        }
    }

    /// Hovering *inside* an expansion keeps it open, and moving to another bucket
    /// swaps it rather than closing it.
    #[test]
    fn walking_through_an_expansion_keeps_it_open_and_leaving_it_hands_it_to_the_next_bucket() {
        let marks: Vec<_> = (1..=400).map(ok).collect();
        let folded = rail_of(&marks);
        let opened = resolve(BODY, &commands(&marks), 1.0, None, centre(&folded.ticks[7]));
        assert_eq!(opened.expanded, Some(7));
        // Every member of the open group holds it open, including the far ends,
        // which is the whole of the mock-up's hysteresis sentence.
        let members: Vec<f32> = opened
            .rail
            .ticks
            .iter()
            .filter(|tick| tick.sub)
            .map(centre)
            .collect();
        assert_eq!(members.len(), 3);
        for y in members {
            let held = resolve(BODY, &commands(&marks), 1.0, Some(7), y);
            assert_eq!(held.expanded, Some(7));
            assert!(held.rail.ticks[held.nearest.expect("a crest")].sub);
        }
        // Far away is another bucket, and the expansion moves with the hand.
        let elsewhere = resolve(
            BODY,
            &commands(&marks),
            1.0,
            Some(7),
            centre(&folded.ticks[60]),
        );
        assert_eq!(elsewhere.expanded, Some(60));
        // A rail with nothing to aggregate never opens anything.
        let few: Vec<_> = (1..=5).map(ok).collect();
        let single = rail_of(&few);
        let plain = resolve(BODY, &commands(&few), 1.0, None, centre(&single.ticks[2]));
        assert_eq!(plain.expanded, None);
        assert_eq!(plain.nearest, Some(2));
        // And a stale slot — the ledger shrank under an open group — folds back
        // rather than laying out a bucket that is not there.
        let stale = resolve(
            BODY,
            &commands(&few),
            1.0,
            Some(90),
            centre(&single.ticks[2]),
        );
        assert_eq!(stale.expanded, None);
    }

    /// The card's four readings.
    #[test]
    fn the_glance_card_says_the_command_the_count_the_running_state_or_the_honest_gap() {
        let marks = vec![
            said(1, "cargo test --workspace"),
            said(2, "git status"),
            said(3, "  "),
            CommandMark {
                command_text: "sleep 30".to_owned(),
                finished: None,
                exit_code: None,
                ..ok(4)
            },
        ];
        let tick = |mark: u64, members: usize| Tick {
            target: Target::Command(CommandMarkId(mark)),
            members,
            matched: 0,
            commanded: members,
            signal: Signal::Command,
            slot: 0,
            sub: false,
            rect: [0.0; 4],
        };
        assert_eq!(
            peek_text(&tick(1, 1), &marks, None, None),
            ("cargo test --workspace".to_owned(), false)
        );
        assert_eq!(
            peek_text(&tick(2, 4), &marks, None, None),
            ("4 commands · latest: git status".to_owned(), false)
        );
        assert_eq!(
            peek_text(&tick(4, 1), &marks, None, None),
            ("running · sleep 30".to_owned(), false)
        );
        // The ledger's honest empty — a word, in the muted ink.
        assert_eq!(
            peek_text(&tick(3, 1), &marks, None, None),
            ("command".to_owned(), true)
        );
        assert_eq!(
            peek_text(&tick(3, 9), &marks, None, None),
            ("9 commands · latest: command".to_owned(), true)
        );
        // A mark that is no longer in the ledger says the same honest thing rather
        // than nothing at all.
        assert_eq!(
            peek_text(&tick(99, 1), &marks, None, None),
            ("command".to_owned(), true)
        );
    }

    /// The duration ladder, as a table.
    #[test]
    fn a_run_time_is_said_in_one_coarse_unit_or_not_at_all() {
        let table = [
            (0, None),
            (999, None),
            (1_000, Some("1s")),
            (42_000, Some("42s")),
            (59_999, Some("59s")),
            (60_000, Some("1m")),
            (119_000, Some("1m")),
            (3_599_000, Some("59m")),
            (3_600_000, Some("1h")),
            (7_320_000, Some("2h")),
            (360_000_000, Some("100h")),
        ];
        for (millis, expected) in table {
            assert_eq!(
                peek_duration(Duration::from_millis(millis)).as_deref(),
                expected,
                "{millis}ms"
            );
        }
    }

    /// A finished command's card carries what became of it — and says nothing about
    /// a success beyond how long it took.
    #[test]
    fn a_finished_card_carries_its_duration_and_only_a_bad_exit_code() {
        let clock = Instant::now();
        let ran = |id: u64, text: &str, millis: u64, code: i32| CommandMark {
            command_text: text.to_owned(),
            exit_code: Some(code),
            executed_at: Some(clock),
            finished_at: Some(clock + Duration::from_millis(millis)),
            ..mark(id, Some(code))
        };
        let marks = vec![
            ran(1, "cargo test", 42_000, 1),
            ran(2, "cargo fmt", 3_000, 0),
            // Fast and clean: neither suffix has anything to say.
            ran(3, "cd ..", 40, 0),
            // Finished, but the shell reported no status at all.
            CommandMark {
                exit_code: None,
                ..ran(4, "make", 90_000, 0)
            },
            // Still running: no duration, no status, and the running prefix.
            CommandMark {
                finished: None,
                finished_at: None,
                exit_code: None,
                ..ran(5, "cargo build", 42_000, 0)
            },
        ];
        let tick = |mark: u64| Tick {
            target: Target::Command(CommandMarkId(mark)),
            members: 1,
            matched: 0,
            commanded: 1,
            signal: Signal::Command,
            slot: 0,
            sub: false,
            rect: [0.0; 4],
        };
        let card = |mark: u64| peek_text(&tick(mark), &marks, None, None).0;
        assert_eq!(card(1), "cargo test · 42s · exit 1");
        assert_eq!(card(2), "cargo fmt · 3s");
        assert_eq!(card(3), "cd ..");
        assert_eq!(
            card(4),
            "make · 1m",
            "a `D` with no status parameter is not a zero"
        );
        assert_eq!(card(5), "running · cargo build");
    }

    /// The failing command's own last word, on a second line — and nowhere else.
    #[test]
    fn a_failed_card_quotes_its_last_output_line_and_a_successful_one_does_not() {
        let clock = Instant::now();
        let ran = |id: u64, text: &str, code: i32| CommandMark {
            command_text: text.to_owned(),
            executed_at: Some(clock),
            finished_at: Some(clock + Duration::from_secs(9)),
            ..mark(id, Some(code))
        };
        let marks = vec![ran(1, "cargo build", 1), ran(2, "cargo build", 0)];
        let tick = |mark: u64, members: usize| Tick {
            target: Target::Command(CommandMarkId(mark)),
            members,
            matched: 0,
            commanded: members,
            signal: Signal::Command,
            slot: 0,
            sub: false,
            rect: [0.0; 4],
        };
        let quote = Some("error[E0308]: mismatched types");
        assert_eq!(
            peek_text(&tick(1, 1), &marks, None, quote).0,
            "cargo build · 9s · exit 1\nerror[E0308]: mismatched types"
        );
        assert_eq!(
            peek_text(&tick(2, 1), &marks, None, quote).0,
            "cargo build · 9s",
            "a success has nothing in its output a glance wants"
        );
        assert_eq!(
            peek_text(&tick(1, 1), &marks, None, Some("   ")).0,
            "cargo build · 9s · exit 1",
            "a blank quote is no quote"
        );
        // A bucket stays one line: it is a count, and a second line per fold would
        // stop it being one.
        assert_eq!(
            peek_text(&tick(1, 4), &marks, None, quote).0,
            "4 commands · latest: cargo build · 9s · exit 1"
        );
    }

    /// A command that spans rows is still a one-line card; the second line belongs
    /// to the quoted error and to nothing else.
    #[test]
    fn a_command_written_across_rows_is_flattened_into_the_cards_one_line() {
        let marks = vec![said(1, "git commit -m 'first\nsecond'")];
        let tick = Tick {
            target: Target::Command(CommandMarkId(1)),
            members: 1,
            matched: 0,
            commanded: 1,
            signal: Signal::Command,
            slot: 0,
            sub: false,
            rect: [0.0; 4],
        };
        assert_eq!(
            peek_text(&tick, &marks, None, None).0,
            "git commit -m 'first second'"
        );
    }

    /// The card is placed against the crest, not against the two-pixel bar.
    #[test]
    fn the_glance_card_stands_against_the_crest_at_its_full_length() {
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let rail = rail_of(&marks);
        let host = peek_host(&rail, 2).expect("a tick to stand against");
        assert_eq!(host[2], rail.ticks[2].rect[2]);
        assert_eq!(host[2] - host[0], TICK_CREST_LENGTH_LOGICAL_PX);
        assert_eq!(
            [host[1], host[3]],
            [rail.ticks[2].rect[1], rail.ticks[2].rect[3]]
        );
        assert_eq!(peek_host(&rail, 9), None);
    }

    /// The three timelines, each on its own span and each still owing frames until
    /// its own is up.
    #[test]
    fn the_three_transitions_run_for_their_own_durations_and_then_stop_asking_for_frames() {
        let palette = bt_render::chrome_palette();
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let rail = rail_of(&marks);
        let start = Instant::now();
        let mut pointer = RailPointer::default();
        assert!(pointer.is_resting(start, Motion::Full));
        pointer.aim(rail.ticks.len(), Some(2), true, start, Motion::Full);
        assert!(pointer.is_animating(start, Motion::Full));
        assert!(!pointer.is_resting(start, Motion::Full));
        // Halfway through the shortest span everything is between its two ends.
        let mid = start + Duration::from_millis(50);
        let widths = pointer.width.sample(mid, Motion::Full);
        assert!(
            widths[2] > TICK_LENGTH_LOGICAL_PX && widths[2] < TICK_CREST_LENGTH_LOGICAL_PX,
            "the crest is on its way: {}",
            widths[2]
        );
        let (warmth, moving) = pointer.hot_ink.sample(mid, Motion::Full);
        assert!(warmth > 0.0 && warmth < 1.0 && moving);
        // Each span ends on its own schedule, longest last.
        assert!(pointer.is_animating(start + TICK_WIDTH_TRANSITION, Motion::Full));
        assert!(pointer.is_animating(start + TICK_OPACITY_TRANSITION, Motion::Full));
        assert!(!pointer.is_animating(start + TICK_BACKGROUND_TRANSITION, Motion::Full));
        assert!(
            TICK_WIDTH_TRANSITION < TICK_OPACITY_TRANSITION
                && TICK_OPACITY_TRANSITION < TICK_BACKGROUND_TRANSITION
        );
        // Landed, the picture is the one the aim asked for.
        let landed = start + TICK_BACKGROUND_TRANSITION;
        let quads = paint(&rail, &pointer, &palette, landed, Motion::Full).quads;
        assert!(
            quads
                .iter()
                .any(|quad| quad.color == palette.command_tick_crest)
        );
        assert!(quads.iter().any(|quad| quad.color == palette.accent));
        // Cooling brings it all the way back to the resting picture, and only then
        // is the cached layer true again.
        pointer.aim(rail.ticks.len(), None, false, landed, Motion::Full);
        assert!(!pointer.is_resting(landed, Motion::Full));
        let cold = landed + TICK_BACKGROUND_TRANSITION;
        assert!(pointer.is_resting(cold, Motion::Full));
        assert_eq!(
            paint(&rail, &pointer, &palette, cold, Motion::Full).quads,
            build(&rail, &palette).quads
        );
        // Reduced motion has no timeline at all: the aim is simply true.
        let mut still = RailPointer::default();
        still.aim(rail.ticks.len(), Some(2), true, start, Motion::Reduced);
        assert!(!still.is_animating(start, Motion::Reduced));
    }

    /// A rebuild drops the travels the old ticks owned; the rail's own warmth
    /// survives it.
    #[test]
    fn a_bucket_opening_mid_hover_reseeds_the_widths_and_keeps_the_rail_warm() {
        let palette = bt_render::chrome_palette();
        let marks: Vec<_> = (1..=400).map(ok).collect();
        let now = Instant::now();
        let mut cache = RailCache::default();
        let key = RailKey {
            revision: 1,
            body: BODY,
            scale: 1.0,
            expanded: None,
            search: None,
        };
        cache.install(key, rail_of(&marks), &palette);
        let ticks = cache.rail().ticks.len();
        cache
            .pointer_mut()
            .aim(ticks, Some(7), true, now, Motion::Full);
        assert!(cache.pointer().is_animating(now, Motion::Full));
        // The bucket opens: more ticks, so the widths in flight belonged to
        // somebody else and are dropped.
        cache.pointer_mut().expand(Some(7));
        let opened = RailKey {
            expanded: Some(7),
            ..key
        };
        cache.install(
            opened,
            lay_out(BODY, &commands(&marks), 1.0, Some(7)),
            &palette,
        );
        let ticks = cache.rail().ticks.len();
        assert_eq!(ticks, 136, "the bucket of three opened");
        let later = now + Duration::from_millis(20);
        cache
            .pointer_mut()
            .aim(ticks, Some(7), true, later, Motion::Full);
        // The travel starts over from the resting length rather than continuing
        // from widths that belonged to a different set of ticks — and it starts
        // over from *rest* because rest is what `install` has just drawn.
        let widths = cache.pointer().width.sample(later, Motion::Full);
        assert_eq!(widths.len(), ticks);
        assert!(widths.iter().all(|width| *width == TICK_LENGTH_LOGICAL_PX));
        assert!(cache.pointer().width.is_running(later, Motion::Full));
        // But the rail is still hot — a column that blinked back to grey because a
        // bucket opened would read as the hover having been lost.
        assert!(cache.pointer().hot_ink.sample(later, Motion::Full).0 > 0.0);
        assert!(!cache.pointer().is_resting(later, Motion::Full));
    }

    /// The cache is a function of the ledger, the pane and the open bucket, and of
    /// nothing else.
    #[test]
    fn the_rail_cache_rebuilds_on_the_revision_and_the_geometry_and_on_nothing_else() {
        let palette = bt_render::chrome_palette();
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let key = RailKey {
            revision: 7,
            body: BODY,
            scale: 1.0,
            expanded: None,
            search: None,
        };
        let mut cache = RailCache::default();
        assert!(cache.needs_rebuild(key));
        cache.install(
            key,
            lay_out(key.body, &commands(&marks), key.scale, key.expanded),
            &palette,
        );
        assert_eq!(cache.rail().ticks.len(), 5);
        assert_eq!(
            cache.layer().quads.len(),
            build(cache.rail(), &palette).quads.len()
        );
        assert!(
            !cache.needs_rebuild(key),
            "the same frame asks again for free"
        );
        // The four things a rail is a function of, one at a time.
        assert!(cache.needs_rebuild(RailKey { revision: 8, ..key }));
        assert!(cache.needs_rebuild(RailKey {
            body: [BODY[0], BODY[1], BODY[2], BODY[3] + 1.0],
            ..key
        }));
        assert!(cache.needs_rebuild(RailKey { scale: 2.0, ..key }));
        assert!(cache.needs_rebuild(RailKey {
            expanded: Some(2),
            ..key
        }));
        // And the things it is not: the pointer's *height* over the rail and the
        // viewport scrolling are neither of them in the key — the crest travels as
        // a set of widths over geometry this key already fixed.
        cache.clear();
        assert!(cache.needs_rebuild(key));
        assert_eq!(cache.pointer().expanded(), None);
    }

    /// A cold, still rail is handed back its cached picture; anything else is
    /// painted fresh.
    #[test]
    fn the_cache_hands_back_its_own_layer_only_while_nothing_is_happening_to_the_rail() {
        let palette = bt_render::chrome_palette();
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let now = Instant::now();
        let mut cache = RailCache::default();
        cache.install(
            RailKey {
                revision: 1,
                body: BODY,
                scale: 1.0,
                expanded: None,
                search: None,
            },
            rail_of(&marks),
            &palette,
        );
        assert_eq!(
            cache.picture(&palette, now, Motion::Full).quads,
            cache.layer().quads
        );
        let ticks = cache.rail().ticks.len();
        cache
            .pointer_mut()
            .aim(ticks, Some(0), true, now, Motion::Reduced);
        assert_ne!(
            cache.picture(&palette, now, Motion::Reduced).quads,
            cache.layer().quads
        );
    }

    /// The 950 ms row flash, from its opening alpha to nothing.
    #[test]
    fn the_jump_flash_runs_for_950ms_and_then_stops_existing() {
        let ease = |x: f32| crate::cubic_bezier(x, EASE);
        assert_eq!(flash_alpha(Duration::ZERO, ease), Some(JUMP_FLASH_OPACITY));
        let midway = flash_alpha(Duration::from_millis(475), ease).expect("still running");
        assert!(
            midway > 0.0 && midway < JUMP_FLASH_OPACITY,
            "halfway is between the two ends, not at either: {midway}"
        );
        let late = flash_alpha(Duration::from_millis(900), ease).expect("still running");
        assert!(late < midway, "the curve only ever fades");
        assert_eq!(flash_alpha(JUMP_FLASH, ease), None);
        assert_eq!(flash_alpha(Duration::from_secs(3), ease), None);
        // The deadline the loop wakes on and the paint that runs when it does are
        // the same predicate, at every instant on both sides of the end.
        for millis in [0, 1, 474, 949, 950, 951, 3_000] {
            let elapsed = Duration::from_millis(millis);
            assert_eq!(
                flash_is_running(elapsed),
                flash_alpha(elapsed, ease).is_some(),
                "the wake-up and the picture disagree at {millis}ms"
            );
        }
    }

    #[test]
    fn a_flash_paints_the_whole_row_and_nothing_at_all_for_an_empty_one() {
        let palette = bt_render::chrome_palette();
        let quads = flash_quads([100.0, 200.0, 500.0, 218.0], 0.22, 1.0, &palette);
        assert!(!quads.is_empty());
        assert!(quads.iter().all(|quad| quad.color == palette.accent));
        let left = quads
            .iter()
            .map(|quad| quad.rect[0])
            .fold(f32::INFINITY, f32::min);
        let right = quads
            .iter()
            .map(|quad| quad.rect[2])
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!((left, right), (100.0, 500.0));
        assert!(flash_quads([100.0, 200.0, 100.0, 218.0], 0.22, 1.0, &palette).is_empty());
        assert!(flash_quads([100.0, 218.0, 500.0, 218.0], 0.22, 1.0, &palette).is_empty());
    }
    // ────────────────────────── S4: one rail, two sources ──────────────────────────

    fn history(id: u64) -> SearchLine {
        SearchLine::History(bt_transcript::TranscriptId(id))
    }

    /// A command standing on history line `line`.
    fn at(mark: u64, line: u64) -> CommandLine {
        CommandLine {
            mark: CommandMarkId(mark),
            line: Some(history(line)),
            failed: false,
        }
    }

    /// A matched line, and which hit of the search's own list it selects.
    fn hit(line: u64, hit: usize) -> MatchLine {
        MatchLine {
            line: history(line),
            hit,
            current: false,
        }
    }

    /// **B8.** The two sources become one stack in document order, a line that
    /// matches several times gets one entry, and a line that is both a prompt row
    /// and a match gets one entry carrying both.
    ///
    /// MUTATION: append the matches after the commands instead of merging them and
    /// the rail stops being an ordinal picture of the pane — every match sinks to
    /// the bottom of a stack whose whole claim is that position carries order.
    #[test]
    fn a_search_merges_its_lines_into_the_ledger_in_document_order() {
        // Commands on lines 10, 30 and 50; matches on 5, 30 and 40. Line 30 is
        // both — the mock-up's `new Set` gives it one place, and so does this.
        let commands = [at(1, 10), at(2, 30), at(3, 50)];
        // The hit indices are the search's own: line 30 is its third hit because
        // line 5 carried two.
        let matches = [hit(5, 0), hit(30, 2), hit(40, 3)];
        let stack = merge(&commands, &matches);
        assert!(stack.searching);
        assert_eq!(stack.entries.len(), 5, "six sources, one line shared");
        assert_eq!(
            stack
                .entries
                .iter()
                .map(|entry| (entry.mark, entry.hit))
                .collect::<Vec<_>>(),
            vec![
                (None, Some(0)),
                (Some(CommandMarkId(1)), None),
                (Some(CommandMarkId(2)), Some(2)),
                (None, Some(3)),
                (Some(CommandMarkId(3)), None),
            ]
        );
        // Per-line dedup: six hits on one line are one entry, and it is the
        // *first* of them the entry selects — `srch.marks.findIndex`.
        let crowded = merge(&[], &[hit(7, 4)]);
        assert_eq!(crowded.entries.len(), 1);
        assert_eq!(crowded.entries[0].target(), Target::Match(4));
    }

    /// A command whose anchor has been degraded out from under it keeps its slot.
    ///
    /// MUTATION: drop the unnameable commands and a search opened over a cleared
    /// scrollback silently shortens the rail — and the closing test's byte-for-byte
    /// identity is the thing that would notice, one interaction too late.
    #[test]
    fn a_command_whose_line_can_no_longer_be_named_keeps_its_place_in_the_ledger() {
        let gone = CommandLine {
            mark: CommandMarkId(2),
            line: None,
            failed: false,
        };
        let stack = merge(&[at(1, 10), gone, at(3, 50)], &[hit(20, 0), hit(60, 1)]);
        assert_eq!(
            stack
                .entries
                .iter()
                .map(|entry| (entry.mark, entry.hit))
                .collect::<Vec<_>>(),
            vec![
                (Some(CommandMarkId(1)), None),
                (Some(CommandMarkId(2)), None),
                (None, Some(0)),
                (Some(CommandMarkId(3)), None),
                (None, Some(1)),
            ],
            "the placeless command stays between its neighbours in the ledger"
        );
        assert_eq!(stack.entries.len(), 5, "and is not dropped");
    }

    /// **B40-B41, the fork.** A match tick selects; a command tick jumps; and a
    /// line that is both selects, because the reader is holding a query.
    ///
    /// MUTATION: prefer the command in [`Entry::target`] and a prompt row that
    /// matched becomes the one tick on a lit rail that does not answer the search.
    #[test]
    fn a_press_means_select_on_a_match_tick_and_jump_on_a_command_one() {
        let stack = merge(&[at(1, 10), at(2, 30)], &[hit(30, 7), hit(40, 9)]);
        let targets: Vec<_> = stack.entries.iter().map(Entry::target).collect();
        assert_eq!(
            targets,
            vec![
                Target::Command(CommandMarkId(1)),
                Target::Match(7),
                Target::Match(9),
            ]
        );
        // And the plain rail has only the one verb.
        let plain = commands(&[ok(1), ok(2)]);
        assert!(
            plain
                .entries
                .iter()
                .all(|entry| matches!(entry.target(), Target::Command(_))),
            "no search, no second verb"
        );
    }

    /// **A45-A50, and D-9.** Every kind of tick, at all three temperatures.
    ///
    /// MUTATION: give the search crest the accent and a command the pointer landed
    /// on becomes indistinguishable from the match beside it — the one distinction
    /// the results rail exists to draw.
    #[test]
    fn each_kind_of_tick_wears_its_own_ink_while_the_search_is_open() {
        let palette = bt_render::chrome_palette();
        let command = tick_ink(Signal::Command, true, &palette);
        assert_eq!(command.rest, (palette.command_tick, 0.22));
        assert_eq!(command.hot, (palette.command_tick, 0.35));
        assert_eq!(command.crest, (palette.command_tick_search_crest, 0.9));
        let matched = tick_ink(Signal::Match, true, &palette);
        assert_eq!(matched.rest, (palette.accent, 0.6));
        assert_eq!(matched.hot, (palette.accent, 0.75));
        assert_eq!(matched.crest, (palette.command_tick_crest, 1.0));
        // The current match is the deepened accent at rest — *"even unhovered"* —
        // so all three levels are the same and there is nowhere for the pointer to
        // move it to.
        let current = tick_ink(Signal::Current, true, &palette);
        assert_eq!(current.rest, (palette.command_tick_crest, 1.0));
        assert_eq!(current.rest, current.hot);
        assert_eq!(current.rest, current.crest);
        // The commands really do recede: half the opacity they wear on a plain
        // rail, in the same ink.
        let plain = tick_ink(Signal::Command, false, &palette);
        assert_eq!(plain.rest.0, command.rest.0);
        assert!(command.rest.1 < plain.rest.1 / 2.0 + 0.001);
        assert_eq!(
            plain.hot.0, palette.accent,
            "the accent is free when nobody is searching"
        );
    }

    /// **D-9, the intentional deviation.** The mock-up's cascade swallows the red
    /// — `.cmdrail.srch-mode .cmdtick` (0,3,0) beats `.cmdtick.fail` (0,2,0) and
    /// there is no `srch-mode` rule for `.fail` anywhere. Ours keeps it.
    ///
    /// MUTATION: let a failure fall through to the receded grey and *"signals earn
    /// permanent colour"* becomes "signals earn colour until somebody opens a
    /// search box", which is the same as not earning it.
    #[test]
    fn a_failed_commands_tick_keeps_its_red_while_the_search_is_open() {
        let palette = bt_render::chrome_palette();
        assert_eq!(
            tick_ink(Signal::Fail, true, &palette),
            tick_ink(Signal::Fail, false, &palette),
            "the search changes nothing at all about a failure"
        );
        assert_eq!(
            tick_ink(Signal::Fail, true, &palette).rest.0,
            palette.status_err
        );
        // And it is red on the glass, not merely in the table: a rail carrying one
        // failed command and one match paints both signals at rest.
        let stack = merge(
            &[CommandLine {
                mark: CommandMarkId(1),
                line: Some(history(10)),
                failed: true,
            }],
            &[hit(30, 0)],
        );
        let layer = build(&lay_out(BODY, &stack, 1.0, None), &palette);
        assert!(
            layer
                .quads
                .iter()
                .any(|quad| quad.color == palette.status_err)
        );
        assert!(layer.quads.iter().any(|quad| quad.color == palette.accent));
    }

    /// **B19/B56, max wins over four levels.** `current > fail > match > command`,
    /// and the order is the enum's own so there is no table to fall out of step.
    ///
    /// MUTATION: take the *last* member's signal instead of the maximum and a
    /// bucket holding the current match reports whatever happened to be newest —
    /// which for a rail whose whole job is "where are the answers" is the one
    /// reading that must not be lost.
    #[test]
    fn a_bucket_carries_the_strongest_of_the_four_signals_in_it() {
        assert!(Signal::Current > Signal::Fail);
        assert!(Signal::Fail > Signal::Match);
        assert!(Signal::Match > Signal::Command);
        // Four hundred entries bucket into threes; each of the four signals is
        // planted in a bucket of its own alongside two plain commands.
        let mut commands: Vec<CommandLine> = (1..=400).map(|id| at(id, id * 10)).collect();
        commands[4].failed = true; // slot 1 — marks 4, 5, 6
        let matches = [
            MatchLine {
                line: history(80),
                hit: 0,
                current: false,
            }, // command 8 — slot 2
            MatchLine {
                line: history(110),
                hit: 1,
                current: true,
            }, // command 11 — slot 3
        ];
        let stack = merge(&commands, &matches);
        assert_eq!(
            stack.entries.len(),
            400,
            "both matches landed on prompt rows"
        );
        let rail = lay_out(BODY, &stack, 1.0, None);
        assert_eq!(rail.bucket_size, 3);
        assert_eq!(rail.ticks[0].signal, Signal::Command);
        assert_eq!(rail.ticks[1].signal, Signal::Fail);
        assert_eq!(rail.ticks[2].signal, Signal::Match);
        assert_eq!(rail.ticks[3].signal, Signal::Current);
        // A bucket holding a failure *and* the current match reports the current
        // match: it is the stronger of the two, and it is the one the reader is
        // standing on.
        let both = [
            Entry {
                mark: Some(CommandMarkId(1)),
                hit: None,
                signal: Signal::Fail,
            },
            Entry {
                mark: None,
                hit: Some(0),
                signal: Signal::Current,
            },
        ];
        assert_eq!(
            [both[0].signal, both[1].signal].into_iter().max(),
            Some(Signal::Current)
        );
    }

    /// **B36.** The card's noun follows what the bucket is made of, and a bucket
    /// made of both says so rather than picking a side.
    ///
    /// MUTATION: hard-code `lines` whenever a search is open and a bucket of three
    /// commands under a query reads "3 lines", which is a card lying about a thing
    /// the reader can count.
    #[test]
    fn the_glance_card_names_lines_commands_or_both_by_what_the_bucket_holds() {
        let marks = vec![said(1, "cargo build")];
        let bucket = |members: usize, matched: usize, commanded: usize, target: Target| Tick {
            target,
            members,
            matched,
            commanded,
            signal: Signal::Command,
            slot: 0,
            sub: false,
            rect: [0.0; 4],
        };
        // All commands — the prototype's non-search reading, unchanged.
        assert_eq!(
            peek_text(
                &bucket(3, 0, 3, Target::Command(CommandMarkId(1))),
                &marks,
                None,
                None
            ),
            ("3 commands · latest: cargo build".to_owned(), false)
        );
        // All matches — the prototype's search reading.
        assert_eq!(
            peek_text(
                &bucket(3, 3, 0, Target::Match(0)),
                &marks,
                Some("total 42"),
                None
            ),
            ("3 lines · latest: total 42".to_owned(), false)
        );
        // Both — the reading the prototype has no case for.
        assert_eq!(
            peek_text(
                &bucket(4, 2, 3, Target::Match(0)),
                &marks,
                Some("total 42"),
                None
            ),
            ("2 lines, 3 commands · latest: total 42".to_owned(), false)
        );
        // A single match tick reads the line itself, ellipsis and all left to the
        // card; and a line the transcript no longer holds says so in the muted ink
        // rather than saying nothing.
        let single = bucket(1, 1, 0, Target::Match(0));
        assert_eq!(
            peek_text(&single, &marks, Some("  fn main() {  "), None),
            ("fn main() {".to_owned(), false)
        );
        assert_eq!(
            peek_text(&single, &marks, None, None),
            ("line".to_owned(), true)
        );
    }

    /// **The acceptance anchor.** Open a search, type into it, close it — and the
    /// rail is the rail it was, geometry and picture alike.
    ///
    /// MUTATION: leave `searching` set on the way out, or leave the fisheye where
    /// the search left it, and the ticks come back at `.22` grey or with a bucket
    /// hanging open under a hand that never asked — the "zero residue" §7.1.5d
    /// promises, broken in the one direction nobody photographs.
    #[test]
    fn closing_the_search_hands_the_rail_back_exactly_as_it_was() {
        let palette = bt_render::chrome_palette();
        let marks: Vec<_> = (1..=400).map(ok).collect();
        let plain = commands(&marks);
        let before_rail = lay_out(BODY, &plain, 1.0, None);
        let before_layer = build(&before_rail, &palette);
        let key = RailKey {
            revision: 3,
            body: BODY,
            scale: 1.0,
            expanded: None,
            search: None,
        };
        let mut cache = RailCache::default();
        cache.install(key, before_rail.clone(), &palette);

        // Open, and type: the stack is the merge, the rail is a different picture,
        // and the cache says so.
        let lines: Vec<CommandLine> = (1..=400).map(|id| at(id, id * 10)).collect();
        let searched = merge(&lines, &[hit(15, 0), hit(25, 1)]);
        let searching = RailKey {
            search: Some(RailSearch {
                query: 4,
                hits: 9,
                current: Some(0),
            }),
            ..key
        };
        assert!(cache.needs_rebuild(searching));
        cache.fold_for_mode(true);
        cache.install(searching, lay_out(BODY, &searched, 1.0, None), &palette);
        assert!(cache.rail().searching);
        assert_eq!(
            searched.entries.len(),
            marks.len() + 2,
            "two lines nobody ran a command on"
        );
        assert_ne!(cache.layer().quads, before_layer.quads);
        // A bucket opened under the pointer while the search was up.
        cache.pointer_mut().expand(Some(9));

        // Close: the mode switch folds the fisheye, and the plain key rebuilds the
        // rail the pane had before any of this.
        cache.fold_for_mode(false);
        assert_eq!(
            cache.pointer().expanded(),
            None,
            "the fisheye resets with the mode switch"
        );
        assert!(cache.needs_rebuild(key));
        cache.install(key, lay_out(BODY, &commands(&marks), 1.0, None), &palette);
        assert_eq!(*cache.rail(), before_rail, "geometry, byte for byte");
        assert_eq!(
            cache.layer().quads,
            before_layer.quads,
            "picture, byte for byte"
        );
        assert!(!cache.needs_rebuild(key), "and the key it came back under");
        // Including the click targets: the same verb on the same mark, tick by
        // tick.
        assert_eq!(
            cache
                .rail()
                .ticks
                .iter()
                .map(|tick| tick.target)
                .collect::<Vec<_>>(),
            before_rail
                .ticks
                .iter()
                .map(|tick| tick.target)
                .collect::<Vec<_>>()
        );
    }

    /// An empty query is not a search: the stack, the geometry and the picture are
    /// the plain rail's, whatever the capsule is doing.
    ///
    /// MUTATION: key `srch-mode` on the capsule alone — the prototype's own
    /// reading — and `Ctrl+F` greys out the command history before a letter is
    /// typed, which reads as the marks having gone rather than as a field waiting.
    #[test]
    fn an_empty_query_leaves_the_plain_rail_untouched() {
        let palette = bt_render::chrome_palette();
        let marks: Vec<_> = (1..=20).map(ok).collect();
        let plain = commands(&marks);
        assert!(!plain.searching);
        // `merge` with nothing matched is *not* the same thing: it is a search that
        // found nothing, and its commands recede. That difference is what makes the
        // empty-query case a decision at the caller rather than a fall-out here.
        let nothing_found = merge(&(1..=20).map(|id| at(id, id * 10)).collect::<Vec<_>>(), &[]);
        assert!(nothing_found.searching);
        assert_eq!(nothing_found.entries.len(), plain.entries.len());
        let quiet = build(&lay_out(BODY, &plain, 1.0, None), &palette);
        let answered = build(&lay_out(BODY, &nothing_found, 1.0, None), &palette);
        assert_ne!(
            quiet.quads, answered.quads,
            "one of them has been asked a question"
        );
    }

    /// **Ticket item 5.** The key gains the search generation, and it moves on
    /// exactly the three things the merge is a function of.
    ///
    /// MUTATION: leave the current index out of the key and `Enter` walks the
    /// matches while the rail's deep tick stays where it was — the count says `4/9`
    /// and the picture says `1/9`.
    #[test]
    fn the_rail_rebuilds_exactly_when_the_merge_input_moves() {
        let palette = bt_render::chrome_palette();
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let search = RailSearch {
            query: 11,
            hits: 4,
            current: Some(2),
        };
        let key = RailKey {
            revision: 7,
            body: BODY,
            scale: 1.0,
            expanded: None,
            search: Some(search),
        };
        let mut cache = RailCache::default();
        cache.install(
            key,
            lay_out(key.body, &commands(&marks), 1.0, None),
            &palette,
        );
        assert!(!cache.needs_rebuild(key));
        // Another letter typed, or a toggle flipped.
        assert!(cache.needs_rebuild(RailKey {
            search: Some(RailSearch {
                query: 12,
                ..search
            }),
            ..key
        }));
        // Output arrived and the hit set was replaced (R2, ticket item 6).
        assert!(cache.needs_rebuild(RailKey {
            search: Some(RailSearch { hits: 5, ..search }),
            ..key
        }));
        // `Enter`.
        assert!(cache.needs_rebuild(RailKey {
            search: Some(RailSearch {
                current: Some(3),
                ..search
            }),
            ..key
        }));
        // And the capsule closing, which is the mode switch itself.
        assert!(cache.needs_rebuild(RailKey {
            search: None,
            ..key
        }));
        // The ledger and the pane still count, and nothing else has been added:
        // the same key twice is still free.
        assert!(cache.needs_rebuild(RailKey { revision: 8, ..key }));
        assert!(!cache.needs_rebuild(key));
    }

    /// The keyboard walks the ledger; the pointer walks the merged stack. **The
    /// two indexings are not interchangeable**, and this is why `step_command_mark`
    /// reads `session.command_marks()` rather than the rail it is standing beside.
    ///
    /// MUTATION: walk the rail's ticks with `Ctrl+Shift+↑/↓` and the chord starts
    /// landing on search hits — a key that walks commands, quietly redefined by
    /// whatever is typed in a box somewhere else on the pane.
    #[test]
    fn the_keyboard_walks_the_ledger_while_the_pointer_walks_the_merged_stack() {
        let marks: Vec<_> = (1..=3).map(ok).collect();
        let plain = commands(&marks);
        let searched = merge(
            &[at(1, 10), at(2, 30), at(3, 50)],
            &[hit(20, 0), hit(40, 1)],
        );
        assert_eq!(plain.entries.len(), marks.len());
        assert_eq!(searched.entries.len(), 5);
        // Ordinal 1 is command 2 on one and a match on the other, so a walk over
        // tick indices would answer differently depending on what is typed.
        assert_eq!(plain.entries[1].target(), Target::Command(CommandMarkId(2)));
        assert_eq!(searched.entries[1].target(), Target::Match(0));
        // The ledger's own step is unchanged by any of it — `stepped_command_mark`
        // is a function of the mark count and the position in it.
        assert_eq!(
            crate::stepped_command_mark(marks.len(), Some(0), crate::Step::Forward),
            Some(1)
        );
    }
}
