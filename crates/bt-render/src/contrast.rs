//! The floor a cell's ink is held to against the paper it is printed on —
//! `docs/DESIGN.md` §2.6.
//!
//! # What this module is for
//!
//! A colour scheme is free to give an ANSI index a value within a hair of its own background,
//! and the scheme this product ships does something stronger than that. Read
//! `assets/schemes/solarized-dark.json`: `"black": "#002B36"` and `"background": "#002B36"` are
//! the same twenty-four bits, so ANSI colour 0 under Solarized Dark is **1.00:1** — not a dim
//! colour but a colour that is not drawn at all. `brightBlack` (`#073642`) is the next one up at
//! **1.16:1**, which is the same fact with a rounding error on it. Every program that reaches
//! for those indices to draw the quiet half of a line — a prompt's path segment, `git status`'s
//! hints, a diff's context, an argument list — prints into the void, and what the reader sees is
//! that some of their words went missing.
//!
//! This module is the one repair, and it is the narrowest repair that works: **the ink moves,
//! and nothing else does.**
//!
//! Three things it deliberately does not do, each of which was the obvious alternative:
//!
//! - **It does not touch the background.** A cell's paper is a fact the program declared and
//!   the reader's scheme answered; moving it would break every run of cells that share a
//!   ground into a patchwork, and it would change what a *selection* is drawn over.
//! - **It does not touch the scheme.** Re-inking the palette would fix the pair that collides
//!   and break every pair that did not — a scheme is twenty-one colours chosen against each
//!   other, and this floor sees exactly two at a time.
//! - **It does not reach outside the grid.** The chrome's ~139 colours are already struck
//!   against contrast floors by [`crate::ChromePalette::derive`], and a picture behind the
//!   window is a picture.
//!
//! # What a selection, a search hit and the caret do to it — nothing
//!
//! The question "does the floor apply to selected text, to a search hit, and to the cell under
//! the caret" has two upstream answers and they disagree, so it is settled here against the one
//! this product's own structure already gives:
//!
//! - **Windows Terminal**: selection and search bypass the setting entirely — each has its own
//!   always-on colour rule that overwrites whatever the attribute path produced — and the caret
//!   runs an unconditional nudge of its own that the setting cannot turn off.
//! - **xterm.js**: the floor is applied to selected and decorated cells too, and *against the
//!   selection's own background*, so the ratio is recomputed for the ground actually drawn.
//!
//! **Folio gives Windows Terminal's answer, and gives it structurally rather than by a branch.**
//! `rectangles`' own content-versus-state ruling already puts a selection, both search grounds
//! and the caret on the far side of the line from a cell's colours: they are opaque marks drawn
//! *over* the grid and they never enter [`crate::resolve_colors`]. So a floor applied at that
//! chokepoint is, by construction, a fact about the cell's ink against the cell's paper — the
//! two colours the program named — and the marks are outside it. The current search hit's ink,
//! which *is* overridden, is overridden after the fact at the draw, and that override stands.
//!
//! The caret is the one place the two products differ in substance rather than in plumbing, and
//! Folio sides with its own scheme: a caret here is a named colour in the twenty-one, struck
//! against its canvas by [`crate::ChromePalette::derive`], not a pair this floor could be asked
//! about.
//!
//! # The arithmetic
//!
//! Contrast is **WCAG 2 relative luminance**, the same formula
//! [`crate::theme`]'s own palette pins are checked with, and the same one the setting's four
//! rungs are named after: `(L₁ + 0.05) / (L₂ + 0.05)`, lighter over darker, over the sRGB
//! relative luminance `0.2126 R + 0.7152 G + 0.0722 B` of the linearised channels.
//!
//! The **adjustment** is a move along a straight line in [Oklab], from the ink toward white or
//! toward black. That line has three properties this feature needs and which are the whole
//! reason it was chosen over the two obvious alternatives (scaling the sRGB bytes, or holding
//! Oklab chroma constant and searching lightness):
//!
//! 1. **Hue is exactly preserved.** Both endpoints have `a = b = 0`, so along the line `a` and
//!    `b` are scaled by the same positive factor `1 − t`; `atan2(b, a)` is therefore invariant.
//!    A blue that has to be lightened comes out blue.
//! 2. **Luminance is strictly monotonic in `t`.** Each of `l′, m′, s′` moves linearly toward
//!    the endpoint's, each is cubed, and luminance is a positive combination of the result, so
//!    the search below is a bisection over a monotone function rather than a hunt.
//! 3. **The endpoints are exactly white and black**, so the reachable contrast in each
//!    direction is known in closed form — which is what lets this function be *total* (see
//!    [`raise_against`]'s "when the floor cannot be met" paragraph) instead of having a failure
//!    mode.
//!
//! What the line costs is **chroma**: it decays as `1 − t`, because a colour cannot be
//! lightened past its own gamut without giving some up. Holding chroma constant instead would
//! preserve more of it for the low-chroma near-greys this feature mostly repairs, but it puts
//! the search inside a gamut-mapping loop whose monotonicity is not provable and whose
//! endpoints are not white and black — which costs both properties 2 and 3 above. The trade is
//! taken deliberately, and [`only_lightness_moves`](tests::only_lightness_moves) is the
//! assertion that names what is kept: hue exact, chroma never rising, lightness the only thing
//! aimed at.
//!
//! [Oklab]: https://bottosson.github.io/posts/oklab/

use std::{
    cell::RefCell,
    sync::atomic::{AtomicU8, Ordering},
};

/// The contrast floor in force, process-wide — `settings.json`'s `minimum_contrast`.
///
/// **Four rungs, and no upstream ships exactly these four** — DESIGN §2.6 records the survey,
/// and it is worth having near the enum because the shape of this type is the one place a
/// reader might assume it was copied. The two products with a comparable feature are:
///
/// - **VS Code / xterm.js** (`terminal.integrated.minimumContrastRatio`) — the same *quantity*
///   this enum names, and the same WCAG arithmetic, but declared as a free `number` defaulting
///   to `4.5`. A free number in a dialog with no numeric fields would be a text box nobody
///   could answer; four rungs are the discrete reading of it, and `4.5` and `21` are among the
///   examples VS Code's own description lists.
/// - **Windows Terminal** (`adjustIndistinguishableColors`) — *not* a contrast ratio at all,
///   but a ΔEOK distance nudge in Oklab against a fixed threshold, whose four values
///   (`never`/`indexed`/`always`/`automatic`) name *when* to adjust rather than *how far*.
///
/// So the ratios are VS Code's arithmetic, the colour space is Windows Terminal's, and the
/// ladder — `Off` first, then the two WCAG AA bars with a "merely visible" rung under them —
/// is this product's own.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MinimumContrast {
    /// Every colour drawn as the program asked for it.
    #[default]
    Off,
    /// 2:1 — visible, and no claim beyond that.
    Ratio2,
    /// 3:1 — WCAG AA for large text and non-text objects.
    Ratio3,
    /// 4.5:1 — WCAG AA for body text.
    Ratio45,
}

impl MinimumContrast {
    /// The ratio this rung asks for, or `None` for [`Self::Off`].
    ///
    /// `Off` is an absence rather than `1.0` on purpose: `1.0` is met by every pair including a
    /// colour drawn on itself, so the two would behave alike — but only the absence lets
    /// [`raise_against`] leave the hot path before it has read a single channel, which is what
    /// makes the default cost one atomic load. See the structural pin
    /// [`off_changes_no_byte`](tests::off_changes_no_byte).
    #[must_use]
    pub const fn ratio(self) -> Option<f64> {
        match self {
            Self::Off => None,
            Self::Ratio2 => Some(2.0),
            Self::Ratio3 => Some(3.0),
            Self::Ratio45 => Some(4.5),
        }
    }

    const fn bits(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Ratio2 => 1,
            Self::Ratio3 => 2,
            Self::Ratio45 => 3,
        }
    }

    const fn from_bits(bits: u8) -> Self {
        match bits {
            1 => Self::Ratio2,
            2 => Self::Ratio3,
            3 => Self::Ratio45,
            _ => Self::Off,
        }
    }
}

/// The highest floor that is reachable from **every** background, `√21 ≈ 4.5826`.
///
/// Not a limit this module enforces — [`raise_against`] answers sensibly above it, and there is
/// a test that walks that path — but the reason the four rungs above stop where they do, and a
/// fact worth writing down because it is not obvious. Against a background of relative
/// luminance `Y`, pushing the ink all the way to white buys `1.05 / (Y + 0.05)` and all the way
/// to black buys `(Y + 0.05) / 0.05`; their product is exactly `21` for every `Y`, so both
/// can fall short of a floor `T` only when `T² > 21`. Below `√21` at least one direction always
/// arrives, which is why the picker's top rung is 4.5 and not WCAG AAA's 7.
pub const ALWAYS_REACHABLE_RATIO_LIMIT: f64 = 4.582_575_694_955_84;

/// WCAG 2's relative luminance of an sRGB byte triple.
///
/// The one arithmetic a contrast claim in this product can be checked with, and it is spelled
/// here so that the renderer and the palette pins in [`crate::theme`] cannot drift apart.
#[must_use]
pub fn relative_luminance(colour: [u8; 3]) -> f64 {
    let linear = srgb_to_linear(colour);
    0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]
}

/// WCAG 2's contrast ratio between two sRGB byte triples — lighter over darker, both offset by
/// `0.05`. Ranges from `1.0` (a colour against itself) to `21.0` (black against white).
#[must_use]
pub fn contrast_ratio(one: [u8; 3], other: [u8; 3]) -> f64 {
    ratio_of_luminances(relative_luminance(one), relative_luminance(other))
}

fn ratio_of_luminances(one: f64, other: f64) -> f64 {
    (one.max(other) + 0.05) / (one.min(other) + 0.05)
}

/// Set the process-wide contrast floor. Returns whether it moved.
///
/// **The revision bump is here and not at the caller**, which is
/// `crate::theme::bump_theme_revision`'s own ruling applied a second time: a background picture
/// and a ground alpha ride the one revision channel because they invalidate exactly what a
/// palette change invalidates, and so does this — the composed-row cache keys on
/// [`crate::theme_revision`], and every row it is holding was shaped with the old floor's ink.
/// A caller who had to remember the bump is a caller who will forget it, and the symptom would
/// be a settings row that appears to do nothing until the next scroll.
pub fn set_minimum_contrast(floor: MinimumContrast) -> bool {
    let moved = process_floor().swap(floor.bits(), Ordering::AcqRel) != floor.bits();
    if moved {
        crate::theme::bump_theme_revision();
    }
    moved
}

/// Read the process-wide contrast floor from one atomic load.
#[must_use]
pub fn current_minimum_contrast() -> MinimumContrast {
    MinimumContrast::from_bits(process_floor().load(Ordering::Acquire))
}

fn process_floor() -> &'static AtomicU8 {
    static FLOOR: AtomicU8 = AtomicU8::new(0);
    &FLOOR
}

/// Held by every test that reads or writes the process floor — including the structural pin in
/// `lib.rs`, which is in another module and must take *this* lock rather than one of its own.
///
/// A process-wide setting under a test harness that runs tests in parallel threads is a shared
/// mutable global, and the failure it produces is the worst kind: a test that passes alone,
/// passes on a rerun, and goes red once a week on CI because a sibling had the floor raised
/// while it was asserting `Off`. The lock is the harness's missing `#[serial]`; every test that
/// touches [`set_minimum_contrast`] or [`raise_to_floor`] takes it first and restores what it
/// found before dropping it.
#[cfg(test)]
pub(crate) static FLOOR_UNDER_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The guard, with the restore attached, so that a test cannot take the lock and forget to put
/// the floor back — a panicking assertion inside a test would otherwise leave every later test
/// reading a raised floor.
#[cfg(test)]
pub(crate) struct FloorGuard {
    restore: MinimumContrast,
    _held: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl FloorGuard {
    pub(crate) fn take() -> Self {
        let held = FLOOR_UNDER_TEST
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self {
            restore: current_minimum_contrast(),
            _held: held,
        }
    }

    pub(crate) fn set(&self, floor: MinimumContrast) -> bool {
        set_minimum_contrast(floor)
    }
}

#[cfg(test)]
impl Drop for FloorGuard {
    fn drop(&mut self) {
        set_minimum_contrast(self.restore);
    }
}

/// **The one function a cell's ink passes through**, and the reason this feature is not a
/// patch at nine call sites: `resolve_colors` is the single place a `CellStyle` becomes two
/// byte triples, so the floor is applied there and every drawing path — the glyph, the
/// underline, the dotted underline, the procedural box-drawing geometry — inherits it by
/// construction rather than by nine matching edits.
///
/// Returns `foreground` untouched when the floor is [`MinimumContrast::Off`], when the pair
/// already clears it, or when the two colours are equal in the one way that cannot be
/// repaired — which is none of them, because a colour drawn on itself is repaired like any
/// other pair.
#[must_use]
pub fn raise_to_floor(foreground: [u8; 3], background: [u8; 3]) -> [u8; 3] {
    let Some(floor) = current_minimum_contrast().ratio() else {
        // The default's whole cost, and the structural pin's whole content: one acquire load
        // and a branch, before a single channel has been read.
        return foreground;
    };
    memoised(foreground, background, floor)
}

/// [`raise_to_floor`] with the floor named rather than read, and with no memo in front of it —
/// the arithmetic itself.
///
/// **When the floor cannot be met** (only possible above [`ALWAYS_REACHABLE_RATIO_LIMIT`], so
/// never for a rung this product offers), the answer is the endpoint that gets furthest: white
/// or black, whichever stands further from the background. Giving up and returning the original
/// would leave a reader who asked for 7:1 staring at the invisible text they asked to be rid
/// of, and "as much contrast as this background permits" is the honest continuous extension of
/// what every reachable floor already does.
#[must_use]
pub fn raise_against(foreground: [u8; 3], background: [u8; 3], floor: f64) -> [u8; 3] {
    let background_luminance = relative_luminance(background);
    let foreground_luminance = relative_luminance(foreground);
    if ratio_of_luminances(foreground_luminance, background_luminance) >= floor {
        return foreground;
    }

    let origin = Oklab::of(foreground);
    // The two luminances that would put this pair exactly on the floor, one on each side of the
    // background. Either may be out of range, which is precisely "this direction cannot arrive".
    let toward_white = floor * (background_luminance + 0.05) - 0.05;
    let toward_black = (background_luminance + 0.05) / floor - 0.05;

    let lighter = Candidate::new(&origin, Endpoint::White, toward_white);
    let darker = Candidate::new(&origin, Endpoint::Black, toward_black);

    let chosen = match (lighter.arrives, darker.arrives) {
        // Both directions can carry this pair over the floor, so the ruling's own words decide:
        // push whichever way is nearer, measured as the distance travelled in lightness. It is
        // the same quantity the reader would name — how much paler or darker the word got —
        // and it is also what is conserved, because chroma decays with it.
        (true, true) => {
            if lighter.lightness_travelled <= darker.lightness_travelled {
                lighter
            } else {
                darker
            }
        }
        (true, false) => lighter,
        (false, true) => darker,
        // Neither arrives: above `ALWAYS_REACHABLE_RATIO_LIMIT`. Take the further endpoint.
        (false, false) => {
            if ratio_of_luminances(1.0, background_luminance)
                >= ratio_of_luminances(0.0, background_luminance)
            {
                lighter
            } else {
                darker
            }
        }
    };
    chosen.colour
}

/// Which end of the lightness axis a move is aimed at. Both have `a = b = 0`, which is what
/// makes the hue along the path invariant.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Endpoint {
    White,
    Black,
}

impl Endpoint {
    const fn lightness(self) -> f64 {
        match self {
            Self::White => 1.0,
            Self::Black => 0.0,
        }
    }
}

/// One direction's answer: the colour it lands on, whether that colour clears the floor, and
/// how far the ink's lightness had to travel to get there.
struct Candidate {
    colour: [u8; 3],
    arrives: bool,
    lightness_travelled: f64,
}

impl Candidate {
    /// Bisect the path for the first point whose luminance reaches `target`.
    ///
    /// The predicate is monotone in `t` (module doc, property 2), so 40 halvings settle `t` far
    /// below the precision an 8-bit channel can express; the loop is fixed-length rather than
    /// tolerance-driven so that its cost is a constant this module can be reasoned about.
    fn new(origin: &Oklab, endpoint: Endpoint, target: f64) -> Self {
        let reachable = match endpoint {
            Endpoint::White => target <= 1.0,
            Endpoint::Black => target >= 0.0,
        };
        if !reachable {
            // The endpoint itself is as far as this direction goes.
            return Self::at(origin, endpoint, 1.0, false);
        }
        let mut low = 0.0_f64;
        let mut high = 1.0_f64;
        for _ in 0..40 {
            let middle = f64::midpoint(low, high);
            let luminance = linear_luminance(origin.stepped(endpoint, middle).to_linear_srgb());
            let past = match endpoint {
                Endpoint::White => luminance >= target,
                Endpoint::Black => luminance <= target,
            };
            if past {
                high = middle;
            } else {
                low = middle;
            }
        }
        Self::at(origin, endpoint, high, true)
    }

    fn at(origin: &Oklab, endpoint: Endpoint, step: f64, arrives: bool) -> Self {
        let moved = origin.stepped(endpoint, step);
        Self {
            colour: moved.to_srgb_bytes(endpoint),
            arrives,
            lightness_travelled: (moved.lightness - origin.lightness).abs(),
        }
    }
}

/// A colour in Oklab. `lightness` is Ottosson's `L`; `green_red` and `blue_yellow` are his `a`
/// and `b`, named for what they measure so that "only lightness moves" is legible at the one
/// place it happens ([`Oklab::stepped`]).
#[derive(Clone, Copy)]
struct Oklab {
    lightness: f64,
    green_red: f64,
    blue_yellow: f64,
}

impl Oklab {
    fn of(colour: [u8; 3]) -> Self {
        let [red, green, blue] = srgb_to_linear(colour);
        let long =
            (0.412_221_470_8 * red + 0.536_332_536_3 * green + 0.051_445_992_9 * blue).cbrt();
        let medium =
            (0.211_903_498_2 * red + 0.680_699_545_1 * green + 0.107_396_956_6 * blue).cbrt();
        let short =
            (0.088_302_461_9 * red + 0.281_718_837_6 * green + 0.629_978_700_5 * blue).cbrt();
        Self {
            lightness: 0.210_454_255_3 * long + 0.793_617_785_0 * medium - 0.004_072_046_8 * short,
            green_red: 1.977_998_495_1 * long - 2.428_592_205_0 * medium + 0.450_593_709_9 * short,
            blue_yellow: 0.025_904_037_1 * long + 0.782_771_766_2 * medium
                - 0.808_675_766_0 * short,
        }
    }

    /// The point a fraction `step` of the way from this colour to `endpoint`.
    ///
    /// **This is the whole of "only the lightness moves".** `green_red` and `blue_yellow` are
    /// both multiplied by the same `1 − step`, because the endpoint's are both zero — so the
    /// hue `atan2(b, a)` is unchanged for every `step < 1`, and the chroma `hypot(a, b)` is
    /// scaled rather than steered. Lightness is the only coordinate given a destination.
    fn stepped(&self, endpoint: Endpoint, step: f64) -> Self {
        let remaining = 1.0 - step;
        Self {
            lightness: self.lightness + step * (endpoint.lightness() - self.lightness),
            green_red: self.green_red * remaining,
            blue_yellow: self.blue_yellow * remaining,
        }
    }

    fn to_linear_srgb(self) -> [f64; 3] {
        let long = (self.lightness
            + 0.396_337_777_4 * self.green_red
            + 0.215_803_757_3 * self.blue_yellow)
            .powi(3);
        let medium = (self.lightness
            - 0.105_561_345_8 * self.green_red
            - 0.063_854_172_8 * self.blue_yellow)
            .powi(3);
        let short = (self.lightness
            - 0.089_484_177_5 * self.green_red
            - 1.291_485_548_0 * self.blue_yellow)
            .powi(3);
        [
            4.076_741_662_1 * long - 3.307_711_591_3 * medium + 0.230_969_929_2 * short,
            -1.268_438_004_6 * long + 2.609_757_401_1 * medium - 0.341_319_396_5 * short,
            -0.004_196_086_3 * long - 0.703_418_614_7 * medium + 1.707_614_701_0 * short,
        ]
    }

    /// Quantise to sRGB bytes, rounding **away from the background**.
    ///
    /// Not `round`, and the difference is load-bearing. The bisection above settles on a
    /// continuous colour that sits exactly on the floor; rounding it to the nearer byte would
    /// land under the floor half the time, by up to one 8-bit step, and a test asserting "≥ the
    /// ratio" would then be red for arithmetic reasons rather than for a bug. Rounding up when
    /// lightening and down when darkening puts the quantised colour on the far side of the
    /// continuous answer instead — every channel of an sRGB triple moves luminance the same
    /// way, so this is exactly the direction that preserves the ratio.
    fn to_srgb_bytes(self, endpoint: Endpoint) -> [u8; 3] {
        let linear = self.to_linear_srgb();
        let mut bytes = [0_u8; 3];
        for (byte, channel) in bytes.iter_mut().zip(linear) {
            // The straight line between two in-gamut colours can leave the cube by a hair —
            // measured at under 3e-4 in linear light, a fifth of the first byte step — and the
            // clamp is the gamut boundary, not a guess.
            let encoded = linear_channel_to_srgb(channel.clamp(0.0, 1.0)) * 255.0;
            let rounded = match endpoint {
                Endpoint::White => encoded.ceil(),
                Endpoint::Black => encoded.floor(),
            };
            *byte = rounded as u8;
        }
        bytes
    }
}

fn linear_luminance(linear: [f64; 3]) -> f64 {
    0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]
}

/// sRGB's electro-optical transfer function, per IEC 61966-2-1 and WCAG 2's own restatement of
/// it. The 0.04045 knee and the 2.4 exponent are the standard's, not a fit.
fn srgb_to_linear(colour: [u8; 3]) -> [f64; 3] {
    colour.map(|byte| {
        let value = f64::from(byte) / 255.0;
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    })
}

/// The inverse of [`srgb_to_linear`]'s per-channel half.
fn linear_channel_to_srgb(channel: f64) -> f64 {
    if channel <= 0.003_130_8 {
        channel * 12.92
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    }
}

// ---------------------------------------------------------------------------
// The memo
// ---------------------------------------------------------------------------

/// How many answers one thread remembers. A power of two, because the index is a mask.
///
/// 512 is far more than a frame needs — a screen of terminal output rarely holds more than a
/// few dozen distinct ink-on-paper pairs — and the whole table is 4 KiB, which is cheaper than
/// the one bisection it saves.
const MEMO_SLOTS: usize = 512;

/// Bit 63 of a packed key, set on every occupied slot, so that the all-zero table that a thread
/// starts with reads as empty rather than as "black on black, floor `Off`".
const MEMO_OCCUPIED: u64 = 1 << 63;

thread_local! {
    static MEMO: RefCell<Memo> = const {
        RefCell::new(Memo {
            keys: [0; MEMO_SLOTS],
            values: [[0; 3]; MEMO_SLOTS],
        })
    };
}

struct Memo {
    keys: [u64; MEMO_SLOTS],
    values: [[u8; 3]; MEMO_SLOTS],
}

/// [`raise_against`] behind a per-thread direct-mapped memo.
///
/// **A memo and not a shortcut**: [`raise_against`] is a pure function of its three arguments,
/// so a hit is the answer a miss would have computed, byte for byte — which is what
/// [`the_memo_never_changes_an_answer`](tests::the_memo_never_changes_an_answer) asserts over a
/// sweep wide enough to collide slots repeatedly. It exists because the floor is read **per
/// cell**: a full screen asks this question tens of thousands of times a frame, and two Oklab
/// bisections at that rate is milliseconds of frame time to answer the same handful of
/// questions over and over.
///
/// Per-thread rather than shared, because a lock on this path would cost more than it saves and
/// a renderer thread's working set is its own window's palette.
fn memoised(foreground: [u8; 3], background: [u8; 3], floor: f64) -> [u8; 3] {
    let key = memo_key(foreground, background, floor);
    let slot = memo_slot(key);
    let remembered = MEMO.with(|memo| {
        let table = memo.borrow();
        (table.keys[slot] == key).then(|| table.values[slot])
    });
    if let Some(answer) = remembered {
        return answer;
    }
    let answer = raise_against(foreground, background, floor);
    MEMO.with(|memo| {
        let mut table = memo.borrow_mut();
        table.keys[slot] = key;
        table.values[slot] = answer;
    });
    answer
}

fn memo_key(foreground: [u8; 3], background: [u8; 3], floor: f64) -> u64 {
    let pack = |colour: [u8; 3]| {
        (u64::from(colour[0]) << 16) | (u64::from(colour[1]) << 8) | u64::from(colour[2])
    };
    // The floor's bits rather than the ratio's, so the key stays integral: the four rungs are
    // the only floors `raise_to_floor` ever passes, and a caller reaching `raise_against`
    // directly does not come through here.
    let rung = u64::from(ratio_rung(floor));
    MEMO_OCCUPIED | (rung << 48) | (pack(foreground) << 24) | pack(background)
}

fn ratio_rung(floor: f64) -> u8 {
    for rung in [
        MinimumContrast::Ratio2,
        MinimumContrast::Ratio3,
        MinimumContrast::Ratio45,
    ] {
        if rung.ratio() == Some(floor) {
            return rung.bits();
        }
    }
    MinimumContrast::Off.bits()
}

/// splitmix64's finaliser, masked to the table. A direct-mapped index wants the key's bits
/// mixed, because the low bits of a packed colour pair are one channel of the background and
/// a screen's worth of cells share it.
fn memo_slot(key: u64) -> usize {
    let mut mixed = key;
    mixed ^= mixed >> 30;
    mixed = mixed.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    mixed ^= mixed >> 27;
    mixed = mixed.wrapping_mul(0x94d0_49bb_1331_11eb);
    mixed ^= mixed >> 31;
    (mixed as usize) & (MEMO_SLOTS - 1)
}

/// The polar reading of [`Oklab`]'s two chromatic axes, which only the assertions need: the
/// production path never asks for a hue or a chroma, it only scales both axes at once.
#[cfg(test)]
impl Oklab {
    fn chroma(self) -> f64 {
        self.green_red.hypot(self.blue_yellow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The pair that opened this slice**, read out of `assets/schemes/solarized-dark.json`
    /// rather than invented: that file gives `black` the value `#002B36` and gives
    /// `background` the value `#002B36`. They are the same twenty-four bits. Anything a program
    /// prints in ANSI colour 0 under this scheme is drawn in the paper's own colour — **1.00:1**
    /// — and the reason it reads as "the parameters went invisible" rather than as "the
    /// terminal is broken" is that colour 0 is what a great many prompts and `ls` palettes
    /// reach for to draw the quiet half of a line.
    const SOLARIZED_BLACK: [u8; 3] = [0x00, 0x2b, 0x36];
    /// The scheme's own ground, and therefore also the value above.
    const SOLARIZED_GROUND: [u8; 3] = [0x00, 0x2b, 0x36];
    /// `brightBlack`, the next one up, at 1.16:1 — invisible for every practical purpose and
    /// the second half of the same report.
    const SOLARIZED_BRIGHT_BLACK: [u8; 3] = [0x07, 0x36, 0x42];
    /// `brightGreen`, at 2.79:1 against the same ground. It is here because it is the one rung
    /// boundary in the scheme: it clears `2:1` and misses `3:1`, so it is the pair that proves
    /// the four rungs are four different answers rather than one switch.
    const SOLARIZED_BRIGHT_GREEN: [u8; 3] = [0x58, 0x6e, 0x75];

    /// A spread of paper to hold ink against: the two Solarized grounds, both rails, and four
    /// mid-tones chosen to straddle the band where *both* directions can reach a floor.
    const GROUNDS: [[u8; 3]; 9] = [
        [0x00, 0x00, 0x00],
        [0xff, 0xff, 0xff],
        SOLARIZED_GROUND,
        [0xfd, 0xf6, 0xe3],
        [0x40, 0x40, 0x40],
        [0x60, 0x60, 0x60],
        [0x80, 0x80, 0x80],
        [0xa0, 0xa0, 0xa0],
        [0x1e, 0x3a, 0x5f],
    ];

    fn every_rung() -> [f64; 3] {
        [2.0, 3.0, 4.5]
    }

    /// Red first: the arithmetic itself, on values that can be checked by hand, and then on the
    /// three measurements this slice was opened by.
    ///
    /// Black on white is WCAG's own maximum, 21:1, and a colour against itself is 1:1 by
    /// construction — between them they pin both ends of the formula, the `0.05` offsets
    /// included. The three Solarized readings are the case: 1.00, 1.16 and 2.79, and which
    /// rungs each of them clears is the whole argument for offering four rungs rather than a
    /// switch.
    #[test]
    fn the_ratio_is_wcag_s() {
        assert!((contrast_ratio([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio([0x7f, 0x21, 0x99], [0x7f, 0x21, 0x99]) - 1.0).abs() < 1e-12);
        assert!((relative_luminance([255, 255, 255]) - 1.0).abs() < 1e-9);
        assert!(relative_luminance([0, 0, 0]).abs() < 1e-12);

        // The scheme really does print colour 0 on its own value. This is the bug.
        assert_eq!(SOLARIZED_BLACK, SOLARIZED_GROUND);
        let invisible = contrast_ratio(SOLARIZED_BLACK, SOLARIZED_GROUND);
        assert!((invisible - 1.0).abs() < 1e-12, "{invisible}");

        let nearly = contrast_ratio(SOLARIZED_BRIGHT_BLACK, SOLARIZED_GROUND);
        assert!(
            (nearly - 1.16).abs() < 0.01,
            "brightBlack measures {nearly}"
        );

        let boundary = contrast_ratio(SOLARIZED_BRIGHT_GREEN, SOLARIZED_GROUND);
        assert!(
            (boundary - 2.79).abs() < 0.01,
            "brightGreen measures {boundary}"
        );
        // Clears the bottom rung, misses the two above it — four rungs, three answers.
        assert!((2.0..3.0).contains(&boundary));
        for rung in every_rung() {
            assert!(invisible < rung && nearly < rung, "{rung}");
        }
    }

    /// The two halves of "does this pair need repairing": a pair that already clears the floor
    /// is returned byte for byte, and a pair that does not comes back clearing it.
    ///
    /// The second half is asserted over a sweep rather than one pair, because a floor that is
    /// met for the colours somebody thought to write down and missed for the rest is not a
    /// floor. 16 inks × 9 grounds × 3 rungs, and every one of the 432 answers must be at or
    /// above the rung it was asked for.
    #[test]
    fn every_pair_comes_back_over_the_floor() {
        for rung in every_rung() {
            for ground in GROUNDS {
                for ink in sweep_inks() {
                    let before = contrast_ratio(ink, ground);
                    let after = raise_against(ink, ground, rung);
                    if before >= rung {
                        assert_eq!(
                            after, ink,
                            "{ink:?} on {ground:?} already cleared {rung}:1 and was moved"
                        );
                        continue;
                    }
                    let achieved = contrast_ratio(after, ground);
                    assert!(
                        achieved >= rung,
                        "{ink:?} on {ground:?} at {rung}:1 landed on {after:?}, only {achieved}:1"
                    );
                }
            }
        }
    }

    /// The sixteen ANSI colours of Solarized Dark plus its two grounds — real palette entries
    /// rather than a synthetic ramp, because the colours that collide in practice are the ones
    /// a scheme author placed near their own background.
    fn sweep_inks() -> [[u8; 3]; 16] {
        [
            [0x07, 0x36, 0x42],
            [0xdc, 0x32, 0x2f],
            [0x85, 0x99, 0x00],
            [0xb5, 0x89, 0x00],
            [0x26, 0x8b, 0xd2],
            [0xd3, 0x36, 0x82],
            [0x2a, 0xa1, 0x98],
            [0xee, 0xe8, 0xd5],
            SOLARIZED_BRIGHT_GREEN,
            [0xcb, 0x4b, 0x16],
            [0x58, 0x6e, 0x75],
            [0x65, 0x7b, 0x83],
            [0x83, 0x94, 0x96],
            [0x6c, 0x71, 0xc4],
            [0x93, 0xa1, 0xa1],
            [0xfd, 0xf6, 0xe3],
        ]
    }

    /// **Only the lightness is aimed at.** Hue comes back exact; chroma never rises; and the
    /// lightness moves away from the ground rather than toward it.
    ///
    /// Hue is asserted as a **distance and not an angle**, which is the difference between an
    /// assertion that means something and one that is about the quantiser. The continuous path
    /// holds `atan2(b, a)` invariant by construction ([`Oklab::stepped`]); what the shipped
    /// bytes can add to that is one rounding step in each channel, and near the achromatic axis
    /// one rounding step *is* a large angle — Solarized's `#FDF6E3` darkened onto white comes
    /// out with a chroma of about `0.004`, where a single byte swings the hue by a degree and a
    /// half while the colour has moved a distance nobody can see. So the test measures how far
    /// the answer's `(a, b)` lies off the ray the original's hue defines, in Oklab units, and
    /// holds that under one step. An answer that genuinely turned would fail this at any
    /// chroma; the cream one passes, correctly.
    #[test]
    fn only_lightness_moves() {
        for rung in every_rung() {
            for ground in GROUNDS {
                for ink in sweep_inks() {
                    let after = raise_against(ink, ground, rung);
                    if after == ink {
                        continue;
                    }
                    let (before_lab, after_lab) = (Oklab::of(ink), Oklab::of(after));
                    let (before_c, after_c) = (before_lab.chroma(), after_lab.chroma());
                    if before_c > 0.0 {
                        // |before × after| / |before| — the perpendicular offset of the answer's
                        // chromatic vector from the original's direction.
                        let off_ray = (before_lab.green_red * after_lab.blue_yellow
                            - before_lab.blue_yellow * after_lab.green_red)
                            .abs()
                            / before_c;
                        assert!(
                            off_ray < 0.01,
                            "{ink:?} on {ground:?} at {rung}:1 left its hue's ray by {off_ray}"
                        );
                    }
                    assert!(
                        after_c <= before_c + 1e-3,
                        "{ink:?} on {ground:?} at {rung}:1 gained chroma, {before_c} -> {after_c}"
                    );
                    let ground_lightness = Oklab::of(ground).lightness;
                    assert!(
                        (after_lab.lightness - ground_lightness).abs()
                            > (before_lab.lightness - ground_lightness).abs(),
                        "{ink:?} on {ground:?} at {rung}:1 moved toward the ground"
                    );
                }
            }
        }
    }

    /// The direction rule, on the pair it was written for, and the ladder's monotonicity.
    ///
    /// The ground is dark, so darkening cannot arrive at any of the three rungs — the
    /// arithmetic says so before the search runs, because the luminance it would need is
    /// negative — and the answer must come back lighter than what was sent. This is the
    /// assertion that goes red if the two candidates are ever compared the wrong way round.
    ///
    /// The second half is what makes the picker four answers and not one: a higher rung is
    /// never a smaller repair, so a reader who moves from `2:1` to `4.5:1` sees the text move
    /// further, in the same direction, every time.
    #[test]
    fn the_invisible_pair_is_lightened_and_the_ladder_climbs() {
        for ink in [SOLARIZED_BLACK, SOLARIZED_BRIGHT_BLACK] {
            let mut previous = relative_luminance(ink);
            for rung in every_rung() {
                let after = raise_against(ink, SOLARIZED_GROUND, rung);
                let luminance = relative_luminance(after);
                assert!(
                    luminance > previous,
                    "{ink:?} at {rung}:1 landed on {after:?}, not above {previous}"
                );
                assert!(
                    contrast_ratio(after, SOLARIZED_GROUND) >= rung,
                    "{ink:?} at {rung}:1 landed on {after:?}, short of the floor"
                );
                previous = luminance;
            }
        }
        // The boundary colour is the control: `2:1` must leave it exactly alone, because it
        // already clears that rung, and the two above it must not.
        assert_eq!(
            raise_against(SOLARIZED_BRIGHT_GREEN, SOLARIZED_GROUND, 2.0),
            SOLARIZED_BRIGHT_GREEN
        );
        for rung in [3.0, 4.5] {
            assert_ne!(
                raise_against(SOLARIZED_BRIGHT_GREEN, SOLARIZED_GROUND, rung),
                SOLARIZED_BRIGHT_GREEN,
                "{rung}:1 left a 2.79:1 pair where it was"
            );
        }
    }

    /// `Off` is not a rung with the ratio `1.0` in it; it is an absence that leaves the hot
    /// path before a channel has been read. This is the type-level half of the structural pin —
    /// the byte-level half is `resolve_colors`' own test in `lib.rs`.
    #[test]
    fn off_changes_no_byte() {
        assert_eq!(MinimumContrast::Off.ratio(), None);
        assert_eq!(MinimumContrast::default(), MinimumContrast::Off);
        let floor = FloorGuard::take();
        floor.set(MinimumContrast::Off);
        for ground in GROUNDS {
            for ink in sweep_inks() {
                assert_eq!(raise_to_floor(ink, ground), ink);
            }
        }
    }

    /// The bound that lets [`raise_against`] be total, checked rather than asserted in prose.
    ///
    /// Every rung this product offers is under `√21`, and `√21` is where the two directions'
    /// reach — `1.05 / (Y + 0.05)` and `(Y + 0.05) / 0.05` — stop covering the whole range
    /// between them. The second half walks the far side of that bound with a floor no picker
    /// offers, and pins what happens there: the best a background permits, not a refusal.
    #[test]
    fn the_ladder_stops_under_the_reachable_bound() {
        assert!(
            (ALWAYS_REACHABLE_RATIO_LIMIT * ALWAYS_REACHABLE_RATIO_LIMIT - 21.0).abs() < 1e-9,
            "the bound is √21 or it is nothing"
        );
        for rung in every_rung() {
            assert!(rung < ALWAYS_REACHABLE_RATIO_LIMIT);
        }

        // A mid-grey ground is exactly where neither rail is far enough for WCAG AAA.
        let ground = [0x80, 0x80, 0x80];
        let ground_luminance = relative_luminance(ground);
        assert!(ratio_of_luminances(1.0, ground_luminance) < 7.0);
        assert!(ratio_of_luminances(0.0, ground_luminance) < 7.0);
        let after = raise_against([0x88, 0x88, 0x88], ground, 7.0);
        assert_eq!(
            after,
            [0x00, 0x00, 0x00],
            "above the bound the answer is the further rail, and black is further from mid-grey"
        );
    }

    /// A memo is only a memo if it never changes an answer.
    ///
    /// The sweep is deliberately wider than the table (16 × 9 × 3 = 432 questions into 512
    /// slots, mixed), so slots are shared and a key comparison that was wrong — or absent —
    /// would hand one pair's answer to another. Run twice: the first pass fills, the second
    /// reads back.
    #[test]
    fn the_memo_never_changes_an_answer() {
        let guard = FloorGuard::take();
        for rung in [
            MinimumContrast::Ratio2,
            MinimumContrast::Ratio3,
            MinimumContrast::Ratio45,
        ] {
            guard.set(rung);
            let floor = rung.ratio().expect("a rung above Off has a ratio");
            for _ in 0..2 {
                for ground in GROUNDS {
                    for ink in sweep_inks() {
                        assert_eq!(
                            raise_to_floor(ink, ground),
                            raise_against(ink, ground, floor),
                            "the memo answered {ink:?} on {ground:?} at {floor}:1 differently"
                        );
                    }
                }
            }
        }
    }

    /// The setting is one atomic that reports whether it moved, because the caller owes a
    /// theme-revision bump — and therefore a reshape of every cached row — on exactly the
    /// changes, not on every write of the settings file.
    #[test]
    fn the_floor_reports_only_real_movement() {
        let guard = FloorGuard::take();
        guard.set(MinimumContrast::Off);
        assert!(guard.set(MinimumContrast::Ratio3));
        assert_eq!(current_minimum_contrast(), MinimumContrast::Ratio3);
        assert!(!guard.set(MinimumContrast::Ratio3));
        assert!(guard.set(MinimumContrast::Off));
    }
}
