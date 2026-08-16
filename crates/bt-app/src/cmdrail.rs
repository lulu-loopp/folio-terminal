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
//! # What this slice does not do
//!
//! S2 owns the pointer's *curve* — the rail-wide `hot` colouring (mock 1371), the
//! smooth bell that lengthens the crest's neighbours (mock 8452-8457), the
//! fisheye that opens a collapsed bucket into its members (mock 8438-8451) and
//! the glance card (`#cmd-peek`). S4 owns the search takeover, where matches join
//! this same ordinal stack. This file draws the resting rail, lights the one tick
//! under the pointer, and answers which mark a press or a chord means.

use std::time::Duration;

use bt_render::{
    ChromePalette, OverlayQuad, TERMINAL_SCROLL_LANE_LOGICAL_PX, rounded_overlay_fill,
};
use bt_term::{CommandMark, CommandMarkId};

use crate::marks::OverlayLayer;

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
/// The rest of that curve — the ±1 and ±2 neighbours at 22.5 and 13.5 — is S2's.
/// The peak is here because without it a press has no visible target: a
/// nine-by-two bar that does not answer the pointer is a control nobody can tell
/// they are on.
pub const TICK_CREST_LENGTH_LOGICAL_PX: f32 = 27.0;
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
/// `.cmdrail.hot .cmdtick.crest { opacity: 1 }` — *"the SELECTED tick:
/// unmistakable"*.
pub const TICK_CREST_OPACITY: f32 = 1.0;
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

/// One drawn tick, and the command a press on it means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tick {
    /// Which command this tick jumps to.
    ///
    /// For a bucket of several it is the **newest** member, which is the mock-up's
    /// own `data-line="${members[members.length - 1]}"` (4666). A collapsed bucket
    /// is a range of history and the newest end of it is the one a reader is
    /// looking for; S2's fisheye is what makes the older members reachable exactly.
    pub mark: CommandMarkId,
    /// How many commands this tick stands for. One at every density the pane can
    /// actually hold; more only once the four-pixel floor has been passed.
    pub members: usize,
    /// Whether any member of this tick failed — **maximum wins**, the same rule
    /// the tab dot has followed since 2026-07-18. A bucket carries its strongest
    /// member's signal, and a failure is the strongest signal a command has.
    pub failed: bool,
    /// The resting rectangle, in physical pixels.
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
    /// It is a **fixed** band the width of the mock-up's *resting* box — a tick
    /// and its two three-pixel paddings — and not the width of an expanded crest,
    /// which is a choice about how much of the terminal it is allowed to shadow.
    ///
    /// A flex box is sized by its widest child, so the browser's rail grows the
    /// moment a tick expands. Reproducing that here would mean a surface whose
    /// size changes because it is being pointed at, and — more to the point —
    /// thirty-three logical pixels of every pane's right edge in which a hyperlink
    /// no longer underlines and a Ctrl+click no longer opens. The mock-up can
    /// afford it because its `.term` carries an 18px right padding and no text
    /// ever reaches under the rail; the native grid has no such inset yet
    /// (inventory D-17, still to be ruled on), so the band takes the smallest
    /// reading the mock-up itself supports until it is.
    ///
    /// What that costs is one oddity: a crest is drawn twelve pixels further left
    /// than the band reaches, so a hand that walks *onto* the drawn crest leaves
    /// the rail and the crest relaxes. It costs nothing to use — *"a click
    /// anywhere on the rail jumps to the nearest tick"*, so the crest is never
    /// something to aim at — and the box-growth question belongs with the fisheye
    /// and with D-17, which are the same slice.
    pub bounds: [f32; 4],
    /// Oldest at the top — *"position carries order, not scroll geometry"*.
    pub ticks: Vec<Tick>,
    /// `k` — how many commands each tick stands for (mock 4646). One until the
    /// density floor is passed.
    pub bucket_size: usize,
    /// The scale the geometry above was laid out at, kept so the paint can round
    /// its radii the same way the layout rounded its boxes.
    pub scale: f32,
}

/// What a laid-out rail is a function of, and therefore what may invalidate one.
///
/// Three things and no fourth. **Not the scroll offset** — an ordinal stack does
/// not move when the page does — and **not the pointer**, whose crest is drawn as
/// one extra quad over the resting picture rather than by rebuilding it (see
/// [`crest`]). A frame that changed neither the ledger nor the pane redraws the
/// rail it already had.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RailKey {
    /// [`bt_term::DualPlaneSession::command_marks_revision`] — bumped by ledger
    /// changes and by nothing else, which is the only reason it exists.
    pub revision: u64,
    /// The pane's body, in physical pixels.
    pub body: [f32; 4],
    pub scale: f32,
}

/// One pane's rail, and the key it was built under.
#[derive(Clone, Debug, Default)]
pub struct RailCache {
    key: Option<RailKey>,
    rail: Rail,
    layer: OverlayLayer,
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

    /// Forget everything. The palette is not part of [`RailKey`] — a theme change
    /// is not a fact about a pane — so the one thing that can silently outlive a
    /// rebuild is the ink, and this is how the theme switch says so.
    pub fn clear(&mut self) {
        self.key = None;
        self.rail = Rail::default();
        self.layer = OverlayLayer::default();
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

/// Place the ticks for `marks` inside a pane body, in physical pixels.
///
/// # The arithmetic, and the one place it knowingly leaves the mock-up standing
///
/// Everything here is `renderRailTicks` (mock 4640-4670) with the search branch
/// removed: `avail` from the pane, `pitch` from `avail` and the mark count, `gap`
/// from `pitch`, `capacity` from the four-pixel density floor, and `k` from
/// `capacity`. One deviation is deliberate and it is the mock-up's own bug:
/// `pitch` is computed from the **unaggregated** count while `ceil(N/k)` ticks are
/// actually drawn, so past the floor the block comes out shorter than `avail`
/// rather than filling it — two hundred commands in an eight-hundred-pixel pane
/// fill about four hundred of the six hundred and forty they are allotted. It is
/// reproduced rather than fixed because the fix changes the *density* the rail
/// reads at, and density is the tier-② question — the one the ruling put in S2,
/// beside the fisheye that answers it. A rail that quietly chose its own density
/// here would be answering a question nobody has asked it yet. The block stays
/// centred either way, so the error is legible as "shorter than it could be" and
/// never as "in the wrong place".
#[must_use]
pub fn lay_out(body: [f32; 4], marks: &[CommandMark], scale: f32) -> Rail {
    // An empty ledger draws nothing at all — no box, no band, no error (inventory
    // C13). `cmd.exe` never sends an OSC 133 and a PowerShell without the
    // integration script never sends one either, so this is the *ordinary* state
    // of a large fraction of panes and not an edge case. The mock-up emits the
    // rail element regardless because S4's search results need a surface to hang
    // off; there is no element here to emit, so the empty rail is simply empty.
    if marks.is_empty() || body[2] <= body[0] || body[3] <= body[1] {
        return Rail {
            scale,
            ..Rail::default()
        };
    }
    let px = |logical: f32| logical * scale;
    let height = body[3] - body[1];
    let avail = px(RAIL_BLOCK_MIN_LOGICAL_PX).max(height * RAIL_BLOCK_FRACTION);
    let thickness = px(TICK_THICKNESS_LOGICAL_PX).round().max(1.0);
    let pitch = (avail / marks.len() as f32)
        .clamp(px(TICK_PITCH_MIN_LOGICAL_PX), px(TICK_PITCH_MAX_LOGICAL_PX));
    let gap = px(TICK_GAP_MIN_LOGICAL_PX).max(pitch - px(TICK_THICKNESS_LOGICAL_PX));
    // `capacity = max(4, floor(avail / 4))` — how many ticks fit at the density
    // floor; `k = ceil(N / capacity)` — how many commands each one has to stand
    // for. Four is a floor on the floor: a pane too short to hold four ticks still
    // gets four rather than one, because a rail of one tick says nothing at all.
    let capacity = (avail / px(TICK_PITCH_MIN_LOGICAL_PX)).floor().max(4.0) as usize;
    let bucket_size = marks.len().div_ceil(capacity).max(1);
    let ticks: Vec<Tick> = marks
        .chunks(bucket_size)
        .map(|members| Tick {
            // The newest member (mock 4666), which for a chunk is its last.
            mark: members[members.len() - 1].id,
            members: members.len(),
            failed: members.iter().any(CommandMark::failed),
            rect: [0.0; 4],
        })
        .collect();
    let block = ticks.len() as f32 * thickness + (ticks.len() - 1) as f32 * gap;
    // `top: 50%; transform: translateY(-50%)` — the *block* is centred, not each
    // tick, which is what makes a rail of three ticks and a rail of thirty read as
    // the same instrument at different densities.
    let top = ((body[1] + body[3]) / 2.0 - block / 2.0).round();
    let right = (body[2]
        - px(TERMINAL_SCROLL_LANE_LOGICAL_PX + RAIL_LANE_GAP_LOGICAL_PX)
        - px(RAIL_PADDING_X_LOGICAL_PX))
    .round();
    let ticks = ticks
        .into_iter()
        .enumerate()
        .map(|(index, tick)| {
            let tick_top = (top + index as f32 * (thickness + gap)).round();
            Tick {
                rect: [
                    right - px(TICK_LENGTH_LOGICAL_PX),
                    tick_top,
                    right,
                    tick_top + thickness,
                ],
                ..tick
            }
        })
        .collect::<Vec<_>>();
    let last = ticks[ticks.len() - 1].rect;
    // The band, padded on all four sides the way `.cmdrail { padding: 8px 3px }`
    // pads the flex box, and at the resting width its ticks are — see
    // [`Rail::bounds`] for what that width is a decision about.
    let band_right = right + px(RAIL_PADDING_X_LOGICAL_PX);
    Rail {
        bounds: [
            band_right - px(TICK_LENGTH_LOGICAL_PX + RAIL_PADDING_X_LOGICAL_PX * 2.0),
            ticks[0].rect[1] - px(RAIL_PADDING_Y_LOGICAL_PX),
            band_right,
            last[3] + px(RAIL_PADDING_Y_LOGICAL_PX),
        ],
        ticks,
        bucket_size,
        scale,
    }
}

/// Which tick a pointer at `(x, y)` means, or `None` when the pointer is not on
/// the rail at all.
///
/// **Selection first, curve second** (user ruling 2026-07-18, after five rounds of
/// errata): the answer is a function of the *ordinal* the pointer is nearest to,
/// never of the pixels themselves. There is exactly one peak at any moment and no
/// ambiguous state where a pointer resting between two ticks lights both at half
/// strength. S2 adds the curve on top of this answer; it does not replace it.
#[must_use]
pub fn nearest(rail: &Rail, x: f32, y: f32) -> Option<usize> {
    if rail.ticks.is_empty()
        || x < rail.bounds[0]
        || x > rail.bounds[2]
        || y < rail.bounds[1]
        || y > rail.bounds[3]
    {
        return None;
    }
    let centre = |tick: &Tick| (tick.rect[1] + tick.rect[3]) / 2.0;
    // Ties go to the older tick — the earlier index — so that a pointer exactly on
    // a midpoint always answers the same way rather than depending on iteration.
    rail.ticks
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (centre(a) - y).abs().total_cmp(&(centre(b) - y).abs()))
        .map(|(index, _)| index)
}

/// The resting rail: every tick at its own ink, nothing lit.
///
/// The crest is **not** drawn here, and that is what lets [`RailCache`] hold this
/// layer across a hover: an expanded tick is opaque, exactly as thick, and grows
/// only leftward, so [`crest`]'s single quad covers the resting one completely
/// and the cached picture underneath stays true.
#[must_use]
pub fn build(rail: &Rail, palette: &ChromePalette) -> OverlayLayer {
    let radius = TICK_RADIUS_LOGICAL_PX * rail.scale;
    OverlayLayer {
        quads: rail
            .ticks
            .iter()
            .flat_map(|tick| {
                let (ink, alpha) = if tick.failed {
                    (palette.status_err, TICK_FAIL_REST_OPACITY)
                } else {
                    (palette.command_tick, TICK_REST_OPACITY)
                };
                rounded_overlay_fill(tick.rect, radius, ink, alpha)
            })
            .collect(),
        ..OverlayLayer::default()
    }
}

/// The one tick under the pointer, lengthened and deepened.
///
/// *"Width only grows leftward, so vertical layout never shifts and there is no
/// jitter"* (mock 1367-1368) — the tick's right edge is where it was, and the
/// twenty-seven pixels open out into the pane. The neighbours are untouched: the
/// bell that lifts them is S2's, and a peak with no shoulders is still exactly one
/// unambiguous peak.
#[must_use]
pub fn crest(rail: &Rail, index: usize, palette: &ChromePalette) -> Vec<OverlayQuad> {
    let Some(tick) = rail.ticks.get(index) else {
        return Vec::new();
    };
    let ink = if tick.failed {
        palette.command_tick_fail_crest
    } else {
        palette.command_tick_crest
    };
    rounded_overlay_fill(
        [
            tick.rect[2] - TICK_CREST_LENGTH_LOGICAL_PX * rail.scale,
            tick.rect[1],
            tick.rect[2],
            tick.rect[3],
        ],
        TICK_RADIUS_LOGICAL_PX * rail.scale,
        ink,
        TICK_CREST_OPACITY,
    )
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
        }
    }

    fn ok(id: u64) -> CommandMark {
        mark(id, Some(0))
    }

    fn failed(id: u64) -> CommandMark {
        mark(id, Some(1))
    }

    /// A pane 800 physical pixels tall at scale 1, which makes `avail` 640 and
    /// every number below readable against the mock-up's own arithmetic.
    const BODY: [f32; 4] = [100.0, 50.0, 500.0, 850.0];

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
        let rail = lay_out(BODY, &[], 1.0);
        assert!(rail.ticks.is_empty());
        assert_eq!(rail.bounds, [0.0; 4]);
        assert!(build(&rail, &bt_render::chrome_palette()).quads.is_empty());
        assert_eq!(nearest(&rail, 490.0, 450.0), None);
    }

    /// The ordinal stack: one tick per command, oldest at the top, the whole block
    /// centred on the pane's own middle.
    #[test]
    fn a_handful_of_marks_becomes_an_evenly_spaced_block_centred_on_the_pane() {
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let rail = lay_out(BODY, &marks, 1.0);
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
        assert_eq!(rail.ticks[0].mark, CommandMarkId(1));
        assert_eq!(rail.ticks[4].mark, CommandMarkId(5));
        // 5 ticks × 2px + 4 gaps × 7px = 38, centred on the body's middle of 450.
        let block = (rail.ticks[0].rect[1], rail.ticks[4].rect[3]);
        assert_eq!(block, (431.0, 469.0));
    }

    /// The right inset is the lane plus its gap plus the rail's own padding, and it
    /// is *derived* — the test states the derivation rather than the number, so
    /// widening the lane moves this assertion with the pixels.
    #[test]
    fn the_ticks_sit_inboard_of_the_reserved_scroll_lane() {
        let rail = lay_out(BODY, &[ok(1)], 2.0);
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
        let rail = lay_out(BODY, &marks, 1.0);
        assert_eq!(rail.bucket_size, 3, "ceil(400 / 160)");
        assert_eq!(rail.ticks.len(), 134, "ceil(400 / 3)");
        assert_eq!(rail.ticks[0].members, 3);
        assert_eq!(rail.ticks[133].members, 1, "400 = 133×3 + 1");
        // A bucket jumps to its newest member.
        assert_eq!(rail.ticks[0].mark, CommandMarkId(3));
        assert_eq!(rail.ticks[1].mark, CommandMarkId(6));
        // Under the floor nothing is bucketed at all.
        let few: Vec<_> = (1..=160).map(ok).collect();
        assert_eq!(lay_out(BODY, &few, 1.0).bucket_size, 1);
    }

    /// Maximum wins: one failure anywhere in a bucket colours the whole tick, and
    /// it colours it at rest.
    #[test]
    fn a_bucket_holding_one_failure_stays_red_without_being_pointed_at() {
        let mut marks: Vec<_> = (1..=400).map(ok).collect();
        marks[4] = failed(5);
        let rail = lay_out(BODY, &marks, 1.0);
        assert_eq!(rail.bucket_size, 3);
        assert!(rail.ticks[1].failed, "marks 4,5,6 — the middle one failed");
        assert!(!rail.ticks[0].failed);
        assert!(!rail.ticks[2].failed);
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
        let rail = lay_out(BODY, &[mark(1, None)], 1.0);
        assert_eq!(rail.ticks.len(), 1, "the tick is drawn all the same");
        assert!(!rail.ticks[0].failed);
    }

    /// The pointer picks an ordinal, not a pixel: one peak, ties to the older tick,
    /// and nothing at all off the band.
    #[test]
    fn the_rail_answers_with_the_tick_nearest_the_pointer_and_only_on_its_own_band() {
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let rail = lay_out(BODY, &marks, 1.0);
        // The band is the mock-up's resting box and not a crest's width — see
        // [`Rail::bounds`]. Stated here because it is the number that decides how
        // much of the terminal's own right edge stops answering a hover.
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
        let x = (rail.bounds[0] + rail.bounds[2]) / 2.0;
        for index in 0..5 {
            let centre = (rail.ticks[index].rect[1] + rail.ticks[index].rect[3]) / 2.0;
            assert_eq!(nearest(&rail, x, centre), Some(index));
            // Four pixels off a tick is still that tick — the gap is only seven.
            assert_eq!(nearest(&rail, x, centre + 3.0), Some(index));
        }
        // Exactly between two ticks answers the older one, every time.
        let midpoint = (rail.ticks[0].rect[3] + rail.ticks[1].rect[1]) / 2.0;
        assert_eq!(nearest(&rail, x, midpoint), Some(0));
        // The pane's own body is not the rail.
        assert_eq!(nearest(&rail, rail.bounds[0] - 1.0, midpoint), None);
        assert_eq!(nearest(&rail, x, rail.bounds[1] - 1.0), None);
        assert_eq!(nearest(&rail, x, rail.bounds[3] + 1.0), None);
        // Inside the band but above every tick still answers the topmost: the band
        // is padded, and a press on its padding is a press on the rail.
        assert_eq!(nearest(&rail, x, rail.bounds[1] + 1.0), Some(0));
    }

    /// The crest covers its own resting tick exactly, which is what lets the
    /// resting layer be cached across a hover.
    #[test]
    fn the_crest_grows_leftward_over_the_tick_it_replaces() {
        let marks: Vec<_> = (1..=3).map(ok).collect();
        let rail = lay_out(BODY, &marks, 1.0);
        let palette = bt_render::chrome_palette();
        let quads = crest(&rail, 1, &palette);
        let tick = rail.ticks[1].rect;
        let left = quads
            .iter()
            .map(|quad| quad.rect[0])
            .fold(f32::INFINITY, f32::min);
        let right = quads
            .iter()
            .map(|quad| quad.rect[2])
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(right, tick[2], "the right edge does not move");
        assert_eq!(right - left, TICK_CREST_LENGTH_LOGICAL_PX);
        assert!(
            quads
                .iter()
                .all(|quad| quad.rect[1] >= tick[1] && quad.rect[3] <= tick[3]),
            "thickness stays 2px — only the length answers the pointer"
        );
        assert!(
            quads
                .iter()
                .any(|quad| quad.color == palette.command_tick_crest)
        );
        // A failed tick deepens in its own hue.
        let mut marks: Vec<_> = (1..=3).map(ok).collect();
        marks[1] = failed(2);
        let rail = lay_out(BODY, &marks, 1.0);
        assert!(
            crest(&rail, 1, &palette)
                .iter()
                .any(|quad| quad.color == palette.command_tick_fail_crest)
        );
    }

    /// The cache is a function of the ledger and the pane, and of nothing else.
    #[test]
    fn the_rail_cache_rebuilds_on_the_revision_and_the_geometry_and_on_nothing_else() {
        let palette = bt_render::chrome_palette();
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let key = RailKey {
            revision: 7,
            body: BODY,
            scale: 1.0,
        };
        let mut cache = RailCache::default();
        assert!(cache.needs_rebuild(key));
        cache.install(key, lay_out(key.body, &marks, key.scale), &palette);
        assert_eq!(cache.rail().ticks.len(), 5);
        assert_eq!(
            cache.layer().quads.len(),
            build(cache.rail(), &palette).quads.len()
        );
        assert!(
            !cache.needs_rebuild(key),
            "the same frame asks again for free"
        );
        // The three things a rail is a function of, one at a time.
        assert!(cache.needs_rebuild(RailKey { revision: 8, ..key }));
        assert!(cache.needs_rebuild(RailKey {
            body: [BODY[0], BODY[1], BODY[2], BODY[3] + 1.0],
            ..key
        }));
        assert!(cache.needs_rebuild(RailKey { scale: 2.0, ..key }));
        // And the things it is not: the pointer moving over the rail and the
        // viewport scrolling are neither of them in the key, which is the whole
        // claim — an ordinal stack does not move when the page does, and the
        // crest is drawn *over* this picture rather than into it.
        cache.clear();
        assert!(cache.needs_rebuild(key));
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
}
