//! Runtime-selectable built-in terminal and chrome colors, without changing the renderer's
//! distinction between default colors and explicit ANSI palette colors.

use std::{
    ffi::OsStr,
    sync::{
        OnceLock, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::scheme::{ColourScheme, FOLIO_DARK, FOLIO_LIGHT};

/// The product's terminal defaults, from `design/ui-mockup.html` (the approved
/// styling): dark `--termbg #1B1B1B`, ink `rgba(255,255,255,.87)` composited
/// over it, light ink `--ink #37352F`. Explicit ANSI colors remain distinct
/// from these defaults and use the palette selected by the current theme.
pub const DEFAULT_BACKGROUND_RGB: [u8; 3] = [0x1b, 0x1b, 0x1b];
pub const LIGHT_BACKGROUND_RGB: [u8; 3] = [0xff, 0xff, 0xff];
pub(crate) const DEFAULT_FOREGROUND_RGB: [u8; 3] = [0xe1, 0xe1, 0xe1];
pub(crate) const LIGHT_BACKGROUND_FOREGROUND_RGB: [u8; 3] = [0x37, 0x35, 0x2f];
/// The mock-up's `--cursor` on dark.
pub(crate) const DEFAULT_CURSOR_RGB: [u8; 3] = [0xd4, 0xd4, 0xd4];
/// The mock-up's `--cursor` on light — `:root { --cursor: #37352F }`, which on
/// that canvas is the ink itself.
pub(crate) const LIGHT_CURSOR_RGB: [u8; 3] = [0x37, 0x35, 0x2f];

/// The caret's ink paired with the canvas it stands on.
///
/// Like the terminal's own default ink and the selection fill, this is a
/// *default* colour, so it takes the background-luma decision rather than
/// [`current_theme`]'s: under a `BT_BG` override the caret follows the canvas
/// it is actually drawn on.
pub(crate) fn cursor_for_background(background: [u8; 3]) -> [u8; 3] {
    scheme_for_background(background).cursor
}

/// The caret ink in force, from the same atomic snapshot as its background.
pub(crate) fn cursor_rgb() -> [u8; 3] {
    cursor_for_background(background_rgb())
}

/// How far an unfocused caret's ink falls back toward its canvas, in percent.
///
/// One constant, both canvases. On light it reproduces the mock-up's own
/// `--ink3` exactly — there `--cursor` *is* `--ink`, and `--ink3` is that ink at
/// .45 — so the quiet caret is the same third step the design already spends on
/// a resting pane title, rather than a new number.
pub(crate) const UNFOCUSED_CURSOR_ALPHA_PERCENT: i32 = 45;

/// `ink` at `alpha` thousandths over `canvas`, composited in sRGB and rounded
/// half away from zero.
///
/// **The one compositor.** Every translucent token in the design — `--ink2` at
/// .65, `--hover` at .055, a status badge's `color-mix(… 15%, transparent)` —
/// lands on a surface as this arithmetic, and it is here so that a palette entry
/// can be *derived* rather than transcribed. A hand-computed byte triple is a
/// number nobody can check without redoing the sum; `ink_over(PANEL_DARK, WHITE,
/// 380)` is the CSS declaration itself, and the compiler does the sum.
///
/// Thousandths and not percent because the design's smallest alpha is `.055` and
/// the second smallest is `.088`; a percent would round both away before the
/// composite ever happened.
///
/// `const` so the two palette tables stay compile-time constants and the pipeline
/// is still handed plain opaque colours. It is also called at *draw* time, by the
/// Git page's status badges — see [`ChromePalette::status_ok`] — because one of
/// that mix's two operands is chosen per row and so cannot be a constant. The
/// arithmetic is the same either way, which is the point of there being one.
#[must_use]
pub const fn ink_over(canvas: [u8; 3], ink: [u8; 3], alpha: i32) -> [u8; 3] {
    ink_over_bp(canvas, ink, alpha * 10)
}

/// [`ink_over`] with `alpha` in **ten-thousandths**.
///
/// One design token needs it and the whole ladder is defined in terms of it so
/// there is still one compositor: `.pring .track` is `--border` at
/// `opacity: .7`, which is `.094 × .7 = .0658` on night and `.088 × .7 = .0616`
/// on paper. Rounded to a thousandth the second becomes `.062`, and `.062` puts
/// the track over a hovered tab one level off the value the design's own
/// renderer produces. Ten-thousandths carry it exactly.
///
/// [`ink_over`]'s answers are unchanged by construction: `(10x + 5000)/10000`
/// and `(x + 500)/1000` are the same integer for every `x`, in both signs, so
/// `ink_over(c, i, a)` and `ink_over_bp(c, i, a * 10)` are one function called
/// two ways rather than two roundings that happen to agree today.
#[must_use]
pub const fn ink_over_bp(canvas: [u8; 3], ink: [u8; 3], alpha: i32) -> [u8; 3] {
    let mut faded = [0u8; 3];
    let mut channel = 0;
    while channel < 3 {
        let base = canvas[channel] as i32;
        let scaled = (ink[channel] as i32 - base) * alpha;
        let step = if scaled >= 0 {
            (scaled + 5000) / 10000
        } else {
            (scaled - 5000) / 10000
        };
        faded[channel] = (base + step) as u8;
        channel += 1;
    }
    faded
}

/// `ink` at [`UNFOCUSED_CURSOR_ALPHA_PERCENT`] over `canvas`.
///
/// The pre-composition is the convention [`ChromePalette`] documents; doing it
/// here rather than in a comment means the alpha and the bytes cannot drift
/// apart, and the compiler still hands the pipeline a plain opaque colour.
const fn cursor_ink_faded_over(canvas: [u8; 3], ink: [u8; 3]) -> [u8; 3] {
    ink_over(canvas, ink, UNFOCUSED_CURSOR_ALPHA_PERCENT * 10)
}

// ── the design's own tokens, for the entries that are derived ───────────────
//
// Only the ones [`ink_over`] needs as operands. The rest of this palette
// transcribes its bytes with the sum written in a comment beside them, which was
// the only option before there was a compositor to call; these are here because
// the Git page added fifteen entries at once and fifteen hand-checked sums is
// fifteen chances to be wrong in a way no test would catch.

/// `--termbg #1B1B1B` on the dark canvas.
const TERMBG_DARK: [u8; 3] = [0x1b, 0x1b, 0x1b];
/// `--termbg #FFFFFF` on the light canvas.
const TERMBG_LIGHT: [u8; 3] = [0xff, 0xff, 0xff];
/// `--panel #252525` on the dark canvas.
/// How many roads the commit graph's wheel has (R18's floor).
///
/// Declared here rather than in the app because it is the length of a palette
/// field, and a palette whose length is a fact somewhere else is a palette that
/// can be indexed off the end of.
pub const GRAPH_LANE_COUNT: usize = 8;

/// The lane wheel on the dark canvas — HSL S 52% L 66%, hues 225 265 305 350 35
/// 75 145 190. See [`ChromePalette::graph_lanes`] for the derivation.
pub(crate) const GRAPH_LANES_DARK: [[u8; 3]; GRAPH_LANE_COUNT] = [
    [0x7b, 0x92, 0xd5],
    [0xa1, 0x7b, 0xd5],
    [0xd5, 0x7b, 0xce],
    [0xd5, 0x7b, 0x8a],
    [0xd5, 0xb0, 0x7b],
    [0xbf, 0xd5, 0x7b],
    [0x7b, 0xd5, 0xa1],
    [0x7b, 0xc6, 0xd5],
];

/// The same eight hues on the light canvas — S 55% L 38%, which is where they
/// clear white by the same margin the dark set clears `#1b1b1b`.
pub(crate) const GRAPH_LANES_LIGHT: [[u8; 3]; GRAPH_LANE_COUNT] = [
    [0x2c, 0x46, 0x96],
    [0x58, 0x2c, 0x96],
    [0x96, 0x2c, 0x8d],
    [0x96, 0x2c, 0x3d],
    [0x96, 0x6a, 0x2c],
    [0x7c, 0x96, 0x2c],
    [0x2c, 0x96, 0x58],
    [0x2c, 0x84, 0x96],
];

const PANEL_DARK: [u8; 3] = [0x25, 0x25, 0x25];
/// `--panel #F7F7F5` on the light canvas.
const PANEL_LIGHT: [u8; 3] = [0xf7, 0xf7, 0xf5];
/// The colour every dark ink is a fraction of: `rgba(255,255,255,α)`.
const DARK_INK_SOURCE: [u8; 3] = [0xff, 0xff, 0xff];
/// The colour every light ink is a fraction of: `rgba(55,53,47,α)` — and, at
/// α = 1, `--ink #37352F` itself.
const LIGHT_INK_SOURCE: [u8; 3] = [0x37, 0x35, 0x2f];

/// The colour `--border` and `--border-soft` are a fraction of on the dark
/// canvas: white, the same source the inks are drawn from.
///
/// Its own name and not [`DARK_INK_SOURCE`], because the two only coincide on
/// night — the shade a hairline is struck from is **black** on paper while the
/// ink there is `#37352F`, and reading `--border` as "the ink at .088" puts
/// every light hairline five levels off the one the design's own renderer draws.
const DARK_SHADE_SOURCE: [u8; 3] = [0xff, 0xff, 0xff];
/// The same on light: `--border rgba(0,0,0,.088)`.
const LIGHT_SHADE_SOURCE: [u8; 3] = [0x00, 0x00, 0x00];

/// `--menu #2A2A2A` on the dark canvas — the face of anything that floats over
/// the window rather than being part of its frame.
const MENU_DARK: [u8; 3] = [0x2a, 0x2a, 0x2a];
/// `--menu #FFFFFF` on the light canvas.
const MENU_LIGHT: [u8; 3] = [0xff, 0xff, 0xff];
/// `--win #202020` on the dark canvas — the window's own face, which a control
/// inset in a panel stands on (`.focus-exit { background: var(--win) }`).
const WIN_DARK: [u8; 3] = [0x20, 0x20, 0x20];
/// `--win #FFFFFF` on the light canvas.
const WIN_LIGHT: [u8; 3] = [0xff, 0xff, 0xff];

/// `--border` over `--panel`, dark — the hairline worn by everything struck on
/// the rail's own ground.
///
/// One constant and two fields ([`ChromePalette::rail_seam`] and
/// [`ChromePalette::focus_card_edge`]), because it is one composite rather than
/// two that happen to agree: the pinned run's rule and a focus card's edge are
/// the same `--border` laid on the same `--panel`, and a second literal here
/// would be the second definition of one quantity this repo has already been
/// bitten by once.
const PANEL_HAIRLINE_DARK: [u8; 3] = ink_over(PANEL_DARK, DARK_SHADE_SOURCE, 94);
/// The same on light.
const PANEL_HAIRLINE_LIGHT: [u8; 3] = ink_over(PANEL_LIGHT, LIGHT_SHADE_SOURCE, 88);

/// A focus card's head at rest, dark — `.fc-head { background: var(--hover) }`
/// over the card's own `--menu` face (§7.1.6b′).
///
/// **Over `--menu` and not over `--panel`**, even though F1 draws no part of the
/// card's face uncovered: the head is laid on the card, and the card is laid on
/// the column. Compositing it against the column instead would be right by
/// accident today and wrong the day F2 puts a body under the head.
const FOCUS_CARD_DARK: [u8; 3] = ink_over(MENU_DARK, DARK_INK_SOURCE, 55);
/// The same on light.
const FOCUS_CARD_LIGHT: [u8; 3] = ink_over(MENU_LIGHT, LIGHT_INK_SOURCE, 55);
/// The staged card's head, dark — `.fcard.staged .fc-head { background:
/// var(--active) }`, the column's answer to "which tab am I looking at".
const FOCUS_CARD_STAGED_DARK: [u8; 3] = ink_over(MENU_DARK, DARK_INK_SOURCE, 90);
/// The same on light.
const FOCUS_CARD_STAGED_LIGHT: [u8; 3] = ink_over(MENU_LIGHT, LIGHT_INK_SOURCE, 90);
/// `--active` over a resting card's head, dark — the pane-count badge's pill and
/// the `×`'s hover pill, which are one declaration on one ground.
const FOCUS_CARD_PILL_DARK: [u8; 3] = ink_over(FOCUS_CARD_DARK, DARK_INK_SOURCE, 90);
/// The same on light.
const FOCUS_CARD_PILL_LIGHT: [u8; 3] = ink_over(FOCUS_CARD_LIGHT, LIGHT_INK_SOURCE, 90);
/// The same pair over the staged card's head, dark.
const FOCUS_CARD_PILL_STAGED_DARK: [u8; 3] = ink_over(FOCUS_CARD_STAGED_DARK, DARK_INK_SOURCE, 90);
/// The same on light.
const FOCUS_CARD_PILL_STAGED_LIGHT: [u8; 3] =
    ink_over(FOCUS_CARD_STAGED_LIGHT, LIGHT_INK_SOURCE, 90);
/// The Exit button under the pointer, dark — `--hover` over `--win`.
const FOCUS_EXIT_HOVER_DARK: [u8; 3] = ink_over(WIN_DARK, DARK_INK_SOURCE, 55);
/// The same on light.
const FOCUS_EXIT_HOVER_LIGHT: [u8; 3] = ink_over(WIN_LIGHT, LIGHT_INK_SOURCE, 55);

/// `.gsec` under `.grow:hover`, dark: `--hover rgba(255,255,255,.055)`.
const GIT_ROW_HOVER_DARK: [u8; 3] = ink_over(PANEL_DARK, DARK_INK_SOURCE, 55);
/// The same on light: `--hover rgba(55,53,47,.055)`.
const GIT_ROW_HOVER_LIGHT: [u8; 3] = ink_over(PANEL_LIGHT, LIGHT_INK_SOURCE, 55);
/// `.gact:hover { background: var(--active) }` over the hovered row, dark.
const GIT_ACT_PILL_DARK: [u8; 3] = ink_over(GIT_ROW_HOVER_DARK, DARK_INK_SOURCE, 90);
/// The same on light.
const GIT_ACT_PILL_LIGHT: [u8; 3] = ink_over(GIT_ROW_HOVER_LIGHT, LIGHT_INK_SOURCE, 90);

/// The unfocused caret's ink, paired with its canvas.
///
/// The caret does not change shape when the window loses focus — a bar stays
/// that bar — so the whole of the focus cue lives in this one colour, and it is
/// pre-composited rather than blended at draw time for the reason
/// [`ChromePalette`] gives: this pipeline composites in linear light, so handing
/// the blender an sRGB alpha does not reproduce the design's own value. The
/// surface under a caret *is* known — it is the terminal canvas — so the
/// composite is knowable, and here it is.
pub(crate) fn unfocused_cursor_for_background(background: [u8; 3]) -> [u8; 3] {
    cursor_ink_faded_over(background, cursor_for_background(background))
}

/// The unfocused caret ink in force, from the same atomic snapshot as its
/// background.
pub(crate) fn unfocused_cursor_rgb() -> [u8; 3] {
    unfocused_cursor_for_background(background_rgb())
}
/// Focused bar cursor width in logical pixels, DPI-rounded and never below one device pixel.
pub const CURSOR_BAR_WIDTH_LOGICAL_PX: f32 = 1.0;
/// Focused underline cursor height in logical pixels, DPI-rounded and never below one device pixel.
pub const CURSOR_UNDERLINE_HEIGHT_LOGICAL_PX: f32 = 2.0;
pub(crate) const DEFAULT_DIM_FOREGROUND_RGB: [u8; 3] = [0x88, 0x88, 0x88];
/// Background-only selection treatment; foreground colors remain terminal-authored.
pub(crate) const DEFAULT_SELECTION_BACKGROUND_RGB: [u8; 3] = [0x26, 0x4f, 0x78];
/// The same treatment over a light canvas.
///
/// The mock-up declares no `::selection` — the terminal surface there is a
/// static mock with nothing selected in it — so this value is derived rather
/// than copied, and it is derived from inside the mock-up's own palette instead
/// of imported from another product. `--accent` is #3059D8; the one in-terminal
/// highlight the mock-up does draw, `mark.srch`, is that accent at 30% over
/// `--termbg`. Selection is the same kind of mark, so it takes the same step:
/// 30% of #3059D8 over the light `--termbg` #FFFFFF, which is 255 − 207×.30,
/// 255 − 166×.30, 255 − 39×.30 → #C1CDF3.
///
/// That lands where it should on both counts. Against the reference the eye is
/// trained on — VS Code Light's #ADD6FF — it is the same weight, dropping the
/// canvas from luminance 1.0 to .615 where #ADD6FF drops it to .642; and it is
/// accent's own hue rather than a borrowed blue, so the selection reads as this
/// product's palest surface instead of a guest from another one.
///
/// Re-derived 2026-08-10 when the accent moved from #5E6AD2 to the cobalt
/// #3059D8: this is a *shadow* of the accent, not a colour of its own, so it is
/// recomputed from the ruling rather than kept. It sits one step deeper than the
/// indigo's #CFD2F2 did (.615 against .658) because cobalt is the darker parent,
/// and it stays on the pale side of the ink's own luma threshold, which is what
/// keeps `--ink` #37352F legible on it without an inverted selection foreground.
pub(crate) const LIGHT_SELECTION_BACKGROUND_RGB: [u8; 3] = [0xc1, 0xcd, 0xf3];

/// The selection fill paired with the canvas it lies on.
///
/// This is a *default* colour in the same sense as the terminal's own ink, not
/// an explicit ANSI entry, so it takes the ink's dark/light decision — the
/// background-luma threshold — and not [`current_theme`]'s. Under a `BT_BG`
/// override the two disagree on purpose: `BT_BG=#FFFFFF` keeps the selected
/// theme dark while painting a light canvas, and the ink already follows the
/// canvas there. A selection fill that followed the theme instead would be the
/// one colour on that screen still dressed for the other canvas.
pub(crate) fn selection_background_for_background(background: [u8; 3]) -> [u8; 3] {
    scheme_for_background(background).selection
}

/// The selection fill in force, read through the same atomic snapshot as the
/// background it sits on, so a runtime theme switch changes it on the next frame.
pub(crate) fn selection_background_rgb() -> [u8; 3] {
    selection_background_for_background(background_rgb())
}

// ── in-pane search: the two grounds a hit can wear (§7.1.5d, S3) ─────────────
//
// `mark.srch { background: color-mix(in srgb, var(--accent) 30%, transparent) }`
// and `mark.srch.cur { background: var(--accent); color: var(--termbg) }`
// (mock-up 1530-1532). Two grounds and one ink, and the ink is the whole of the
// ruling the stylesheet wrote a paragraph about at 1526-1529:
//
// > "hits stay readable in place; the CURRENT one is unmistakable. **`--termbg`
// > as the current hit's ink works in both themes**: dark text on the
// > light-theme indigo would fail, white text on the dark-theme periwinkle would
// > fail — each theme's terminal background is exactly the ink that contrasts."
//
// So there is no new colour here at all in the sense a palette usually means
// one: the current hit's ground is the accent this window already has and its
// ink is the canvas it is standing on. What *is* new is the 30% ground, and it
// is pre-composited over the canvas for [`ChromePalette`]'s reason — this
// pipeline composites in linear light, so handing the blender an sRGB alpha does
// not reproduce the design's own value, and the surface under a hit is known.

/// `--accent` over a dark canvas.
pub(crate) const DARK_ACCENT_RGB: [u8; 3] = [0x7a, 0x99, 0xff];
/// `--accent` over a light one.
pub(crate) const LIGHT_ACCENT_RGB: [u8; 3] = [0x30, 0x59, 0xd8];

/// `--accent #7A99FF` at 30% over `--termbg #1B1B1B`: 27 + (122 − 27)×.3 = 55.5,
/// 27 + (153 − 27)×.3 = 64.8, 27 + (255 − 27)×.3 = 95.4 → #38415F.
///
/// The value this product's own dark scheme produces. It is no longer read at
/// draw time — the ground in force is computed from whichever accent and canvas
/// the scheme in force names, see [`search_match_for_background`] — so it stays
/// as the record of the rule and as the pin that holds the computation to it,
/// which is the same footing [`DARK_CHROME`] now stands on.
#[cfg(test)]
pub(crate) const DEFAULT_SEARCH_MATCH_RGB: [u8; 3] =
    ink_over(DEFAULT_BACKGROUND_RGB, DARK_ACCENT_RGB, 300);
/// The same over `--termbg #FFFFFF` → #C1CDF3.
///
/// It comes out identical to [`LIGHT_SELECTION_BACKGROUND_RGB`], and that is not
/// a coincidence to be broken: that constant's own note records that it *was
/// derived from this rule* — "the one in-terminal highlight the mock-up does
/// draw, `mark.srch`, is that accent at 30% over `--termbg`". They are two marks
/// of the same weight and the light canvas gives them the same answer. It is
/// written out again here rather than aliased because the two are free to move
/// apart the day either ruling does, and because the dark canvas already has
/// them apart (`#264F78` against `#38415F`).
#[cfg(test)]
pub(crate) const LIGHT_SEARCH_MATCH_RGB: [u8; 3] =
    ink_over(LIGHT_BACKGROUND_RGB, LIGHT_ACCENT_RGB, 300);

/// The ordinary hit's ground, paired with the canvas it lies on.
///
/// Takes the ink's dark/light decision — the background-luma threshold — and not
/// [`current_theme`]'s, for exactly [`selection_background_for_background`]'s
/// reason: under a `BT_BG` override the two disagree on purpose, and a hit that
/// followed the theme instead would be the one mark on that screen still dressed
/// for the other canvas.
pub(crate) fn search_match_for_background(background: [u8; 3]) -> [u8; 3] {
    let scheme = scheme_for_background(background);
    ink_over(scheme.background, scheme.accent, 300)
}

/// The current hit's ground: the accent, solid.
pub(crate) fn search_current_for_background(background: [u8; 3]) -> [u8; 3] {
    scheme_for_background(background).accent
}

/// The ordinary hit's ground in force, from the same atomic snapshot as its canvas.
pub(crate) fn search_match_rgb() -> [u8; 3] {
    search_match_for_background(background_rgb())
}

/// The current hit's ground in force.
pub(crate) fn search_current_rgb() -> [u8; 3] {
    search_current_for_background(background_rgb())
}

/// The current hit's **ink** — `var(--termbg)`, which is to say the canvas
/// itself, whatever a `BT_BG` override has made it.
pub(crate) fn search_current_ink_rgb() -> [u8; 3] {
    background_rgb()
}
pub(crate) const DEFAULT_STATUS_BACKGROUND_RGB: [u8; 3] = [0x33, 0x33, 0x33];

// ---------------------------------------------------------------------------
// Seat chrome — the styling pass (user-approved 2026-08-07).
//
// One structural idea: every colour the chrome wears is a field of
// `ChromePalette`, and the palette exists twice — once for a dark canvas, once
// for a light one — so "we support both themes" is a fact about a type rather
// than a promise about future work. Which palette is in force follows the same
// background-luma threshold the terminal's own default ink already uses; the
// settings surface that will one day choose explicitly gets to *set* the
// background and everything else follows.
//
// Each field is a *policy* position in the sense of
// `docs/M2-layout-solver-spec.md` §1.4: overturning one edits this block and
// nothing else. Nothing here is a structural ruling — those (a divider occupies
// real space, a collapsed seat is a real rectangle) live in `bt-layout` and are
// not colours.
// ---------------------------------------------------------------------------

/// Every colour the window's own chrome is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChromePalette {
    /// A non-terminal seat's body fill. Exactly `--termbg`, matching terminal.
    pub seat_body: [u8; 3],
    /// A seat title bar's fill: one quiet step off the canvas.
    ///
    /// There is deliberately no companion hairline. `.titlebar` in the mock-up
    /// declares a background and nothing else — the step from `--panel` to
    /// `--termbg` is the separation, and in the active tab's own span there must
    /// be no step at all, because the tab *is* `--termbg`. A rule across the
    /// bar's foot is exactly the line that severs the tab from the terminal it
    /// is shaped to join.
    pub title_bar: [u8; 3],
    /// Title text and a caption button's glyph at rest (`--ink2` over
    /// `--panel`). Bars are wayfinding, not content, so this ink sits a step
    /// below the terminal's own.
    pub title_text: [u8; 3],
    /// A title-bar glyph under the pointer: the one moment a bar glyph is the
    /// subject.
    pub title_text_hover: [u8; 3],
    /// `--ink3` over `--panel`: the step *below* [`Self::title_text`], worn by
    /// the tab's `×` and by the `+`/`˅` pair at the end of the strip.
    ///
    /// The mock-up spends three inks on the title bar and means all three:
    /// `.capbtn` is `--ink2`, while `.tab .close` and `.newtab` are `--ink3`.
    /// Drawing the strip's controls at `--ink2` made every tab carry a `×` as
    /// loud as the caption run — a control you use rarely, shouting as loudly as
    /// the one that closes the window.
    pub title_text_muted: [u8; 3],
    /// `.tab .close:hover` — `--active` over the *active* tab, whose fill is
    /// `--termbg`.
    ///
    /// This pill is the one place in the chrome where a single mock-up
    /// declaration lands on two different surfaces, so it gets two entries
    /// rather than one translucent fill. That is not a shortcut, it is the only
    /// correct reading: this pipeline composites in **linear** light and a
    /// browser composites in sRGB, so handing the blender `--active`'s own .09
    /// does not reproduce `--active` — measured on the dark palette, a
    /// translucent pill came out at 89 where the design's own renderer puts it
    /// at 48, nearly twice the lift. An alpha is only ever honest here when
    /// nothing under it is known (see [`Self::menu_border`]); when the surface
    /// *is* known, the composite is.
    pub tab_close_pill_on_content: [u8; 3],
    /// `--active` over `--hover` over `--panel` — the same pill on a tab you are
    /// merely hovering, where the tab's own hover fill is already down.
    ///
    /// The two are the whole set: `.tab.active` never takes `--hover`, and a tab
    /// that is not hovered cannot have a hovered `×` on it.
    pub tab_close_pill_on_hovered_tab: [u8; 3],

    // ── the `×` itself, over the four surfaces it can land on ──
    //
    // Its pill above learned this lesson first and the glyph standing *in* that
    // pill did not: `.tab .close { color: var(--ink3) }` rising to
    // `var(--ink)` on hover is one declaration over five different grounds —
    // `--panel` on a resting tab, the tab's own `--hover` fill on a hovered
    // one, `--termbg` on the active one, and, once the pointer is on the `×`
    // itself, whichever of the two pills is under it. Translucent inks cannot
    // be handed to this blender for the reason spelled out at
    // [`Self::tab_close_pill_on_content`], so each ground gets its composite.
    //
    // The resting tab's is not new: `--ink3` over `--panel` is exactly
    // [`Self::title_text_muted`], which the `+`/`˅` pair beside the strip
    // already wears, so the call site uses that name for that one surface.
    /// `--ink3` over `--termbg` — the `×` on the active tab, the one tab whose
    /// fill is the terminal's own surface rather than the bar's.
    ///
    /// Numerically this is [`Self::pane_title`] on both canvases, and for a
    /// real reason: a pane head is the terminal surface too, so the same ink
    /// over the same ground must land on the same grey. They are two names
    /// because they are two declarations — a strip control and a caption — and
    /// either could be re-struck without the other.
    pub tab_close_glyph_on_active_tab: [u8; 3],
    /// `--ink3` over `--hover` over `--panel` — the `×` on a tab the pointer is
    /// somewhere inside, but not on the `×`.
    pub tab_close_glyph_on_hovered_tab: [u8; 3],
    /// `.tab .close:hover { color: var(--ink) }` over
    /// [`Self::tab_close_pill_on_content`] — the lit `×` inside its own pill on
    /// the active tab.
    pub tab_close_glyph_on_pill_over_active_tab: [u8; 3],
    /// The same lit `×` over [`Self::tab_close_pill_on_hovered_tab`].
    ///
    /// On light these two are one number and on dark they are not, because
    /// `--ink` is opaque in `:root` and translucent in `body.dark` — the exact
    /// asymmetry that let a single `title_text_hover` look correct for years on
    /// the canvas that cannot tell the surfaces apart.
    pub tab_close_glyph_on_pill_over_hovered_tab: [u8; 3],

    // ── the pin's *state* tier, which the `×` has no equivalent of ──
    //
    // `.tab .pin` stands in the `×`'s own slot and wears the same two inks over
    // the same grounds, so almost all of it is the four fields above: the
    // unpinned pin is `--ink3` on a bare tab, and a hovered pin is `--ink` on
    // one of the two pills.
    //
    // `.tab .pin.on { color: var(--ink) }` is the one combination the `×` can
    // never produce. The `×` only reaches `--ink` under the pointer, and under
    // the pointer there is always a pill beneath it; a pinned pin reaches
    // `--ink` as a *state*, standing on the bare tab with no pill at all. So
    // `--ink` has to be mixed over the tab surfaces too, and those are not the
    // pill mixes: on dark the bare active tab gives 0xE1 where its pill gives
    // 0xE4, three levels apart, and the two happen to agree on the hovered tab
    // only by rounding.
    //
    // A pinned pin on a *resting* tab is `--ink` over `--panel`, which is
    // [`Self::title_text_hover`] — already here, like `title_text_muted` is for
    // the muted tier.
    /// `.tab .pin.on` on the active tab: `--ink` over `--termbg`, with no pill
    /// under it.
    ///
    /// Numerically [`Self::pane_title_focus`], for the same reason
    /// [`Self::tab_close_glyph_on_active_tab`] is `pane_title` — the active tab
    /// and a pane head are both the terminal's own surface.
    pub tab_pin_state_on_active_tab: [u8; 3],
    /// `.tab .pin.on` on a tab the pointer is inside but not on the pin itself:
    /// `--ink` over `--hover` over `--panel`.
    pub tab_pin_state_on_hovered_tab: [u8; 3],
    /// Body state notices — an empty pane's invitation, "Loading …", a failure.
    pub body_hint_text: [u8; 3],
    /// A preview's own text body — `.pv-edit { color: var(--ink) }` (mock-up
    /// 599-604) over `--termbg`.
    ///
    /// Its own field rather than a re-use of [`Self::files_row_text_selected`],
    /// which happens to hold the same two numbers today, on exactly the
    /// precedent the block below cites for `files_row_*`: two declarations
    /// either of which could be re-struck without the other. A file's text is
    /// the strongest ink this window puts on a body — full `--ink`, not the
    /// `--ink2` a *list of file names* is set in — because a preview is the one
    /// surface where the content is the point and the chrome is the frame.
    pub preview_body_text: [u8; 3],
    // ── the read-only view family (mock-up 599-623, 1201-1211, 1638-1644) ──
    //
    // Nine entries, and every one of them is a translucent design token
    // pre-mixed over the ground it actually stands on. Two grounds appear here
    // rather than one: a preview body is `--termbg`, but a markdown code fence
    // lays `--panel` over it and everything inside the fence stands on *that*.
    // `--ink3` over the body and `--ink3` over a fence are different colours,
    // and only one of them is what a browser would have shown.
    /// `.pv-table th, .pv-table td { border: 1px solid var(--border-soft) }` and
    /// the same hairline anywhere else a preview rules a line, over `--termbg`.
    pub preview_grid_line: [u8; 3],
    /// `.md-code { background: var(--panel) }` — opaque, so it is the token.
    pub preview_code_ground: [u8; 3],
    /// `.md-code { border: 1px solid var(--border-soft) }`, over the fence's own
    /// `--panel` ground rather than over the body.
    pub preview_code_border: [u8; 3],
    /// The fence's body — `--ink2`, standing on [`Self::preview_code_ground`].
    pub preview_code_text: [u8; 3],
    /// `.md-code .lang` — `--ink3` on the same ground.
    pub preview_code_lang: [u8; 3],
    /// `.pv-diff .dadd { background: color-mix(in srgb, var(--ok) 13%, transparent) }`
    /// resolved over `--termbg`.
    pub preview_diff_add: [u8; 3],
    /// `.pv-diff .ddel { … var(--err) 10% … }`, likewise.
    pub preview_diff_del: [u8; 3],
    /// `.pv-diff .dhunk { color: var(--accent) }` — opaque, so it is the token,
    /// named separately because a hunk marker is not the unread dot and the two
    /// are free to part company.
    pub preview_diff_hunk: [u8; 3],
    /// `.pv-table th { color: var(--ink) }`, standing on the head row's own
    /// `--hover` fill rather than on the bare body.
    pub preview_table_head_text: [u8; 3],
    /// What a quick edit's selection lies under.
    ///
    /// The terminal's own selection fill, and deliberately not a second one: a
    /// preview body stands on `--termbg` exactly as the grid does, the mock-up
    /// declares no `::selection` for either, and two blues on one screen for one
    /// idea would be the window disagreeing with itself about what "selected"
    /// looks like. Its value is therefore
    /// [`DEFAULT_SELECTION_BACKGROUND_RGB`]/[`LIGHT_SELECTION_BACKGROUND_RGB`],
    /// picked here at palette-build time rather than read from the atomic
    /// snapshot because everything else in this struct is.
    pub preview_selection: [u8; 3],
    /// A quick edit's caret — the terminal's `--cursor`, for the same reason.
    pub preview_caret: [u8; 3],
    // ── the highlight family: seven inks a preview may set source in ──────
    //
    // **Colour is a token, not a theme** (ticket #49, 2026-08-16). The preview
    // asks `syntect` what *scope* every span of a source file is in and asks
    // nothing else of it: syntect ships colour themes and this window uses none
    // of them, because a library's idea of purple is not a member of this
    // palette and a second colour authority is how a window stops looking like
    // one window. What comes back is a scope name; what goes on the glass is
    // one of the seven below.
    //
    // **Seven, and the eighth is the absence of one.** Anything the scope table
    // does not name — an identifier, an operator syntect did not scope, plain
    // prose in a fence — is drawn in the body's own ink, so the *default* state
    // of a highlighted document is the un-highlighted one and the colour is
    // only ever an addition. That is what "reading level" means here: a
    // paragraph of code should read as a paragraph, with its keywords and its
    // strings *findable*, not as a mosaic.
    //
    // **Two canvases, one set.** These inks stand on a preview body (`--termbg`)
    // and inside a markdown fence (`--panel`), and the pin below holds every one
    // of them to 4.5:1 — the text bar, because every one of them is text — on
    // both, in both themes. That floor is why none of them *is* `--ink3`: over
    // `--termbg` `--ink3` is 3.58:1 dark and 2.49:1 light, so the comment and
    // the punctuation get their own named fields at the quietest ink that
    // clears the bar rather than a re-use that would have been illegible.
    // They still share one value with each other, so a later retune of "the
    // quiet ink" is one number in each theme.
    /// `keyword`, `storage` — a muted violet.
    pub hl_keyword: [u8; 3],
    /// `string`, `constant.character.escape` — a muted green.
    pub hl_string: [u8; 3],
    /// `comment` — the quiet ink, cooled a little off neutral so a comment
    /// inside a fence is a *category* and not just a fainter body.
    pub hl_comment: [u8; 3],
    /// `constant.numeric`, `constant.language` — a muted amber.
    pub hl_number: [u8; 3],
    /// `entity.name.type`, `support.type`, `support.class` — a muted teal.
    pub hl_type: [u8; 3],
    /// `entity.name.function`, `support.function` — [`Self::accent`]'s own hue,
    /// desaturated so a page of calls does not read as a page of links.
    pub hl_function: [u8; 3],
    /// `punctuation` — the same quiet ink [`Self::hl_comment`] wears, named
    /// apart because braces and comments are two decisions.
    pub hl_punct_muted: [u8; 3],
    // ── the file tree's rows, mixed over the one ground a files body has ──
    //
    // A files pane's body is `--termbg` and nothing else (B15/U11), so unlike
    // the tab's four grounds this list has exactly three: the bare body, a row
    // under the pointer, and the selected row. Every translucent token the
    // mock-up puts on a row is therefore pre-mixed three ways here rather than
    // once, because `--ink2` over `#1B1B1B` and `--ink2` over the hover fill
    // above it are different colours and only one of them is what a browser
    // would have shown.
    //
    // Several are numerically fields that already exist — `files_row_selected`
    // is `pane_close_pill`, `files_row_muted` is `pane_close_glyph`,
    // `files_row_text_selected` is `pane_close_glyph_on_pill` — and they are
    // named separately on exactly the precedent those three were themselves
    // named on: two declarations, either of which could be re-struck without
    // the other.
    /// `.frow:hover { background: var(--hover) }` over `--termbg`.
    pub files_row_hover: [u8; 3],
    /// `.frow.sel { background: var(--active) }` over `--termbg`.
    pub files_row_selected: [u8; 3],
    /// `.frow { color: var(--ink2) }` over `--termbg`.
    pub files_row_text: [u8; 3],
    /// The same `--ink2`, standing on [`Self::files_row_hover`].
    pub files_row_text_hover: [u8; 3],
    /// `.frow.sel { color: var(--ink) }`, standing on
    /// [`Self::files_row_selected`].
    pub files_row_text_selected: [u8; 3],
    /// `.tri` and `.fico.file`, both `--ink3`, over the bare body.
    pub files_row_muted: [u8; 3],
    /// The same `--ink3`, standing on [`Self::files_row_hover`].
    pub files_row_muted_hover: [u8; 3],
    /// The same `--ink3`, standing on [`Self::files_row_selected`].
    pub files_row_muted_selected: [u8; 3],
    // ── the same eight rows, mixed over the *other* ground a tree can stand on ──
    //
    // C39's rule is that the tree body is shared byte for byte between the
    // docked column and the floating window, and that rule is about the
    // *drawing*. It cannot be about the colours, because the two hosts do not
    // stand on the same plane: a docked column's body is `--termbg` (B15/U11,
    // the eight above), and `#files-flyout`'s face is `--win` (mock-up 674).
    // Those are two different greys on dark, so `--ink2` over one and `--ink2`
    // over the other are two different colours, and a browser would have shown
    // two different colours. One pre-mixed set could not have served both
    // without lying to one of them — the same argument the eight above are
    // themselves named on, applied one host further out.
    //
    // On light the two sets are numerically identical, because `--termbg` and
    // `--win` are both `#FFFFFF` there. That coincidence is the reason this
    // could have gone unnoticed for a whole theme, and it is not a reason to
    // fold them: the dark values are the ones a reader is checking, and a
    // single field would be wrong on exactly the theme this product is used in.
    /// `.frow:hover` over `--win` — [`Self::files_row_hover`]'s twin inside a
    /// floating window.
    pub float_row_hover: [u8; 3],
    /// `.frow.sel` over `--win`.
    pub float_row_selected: [u8; 3],
    /// `--ink2` over `--win`.
    pub float_row_text: [u8; 3],
    /// The same `--ink2`, standing on [`Self::float_row_hover`].
    pub float_row_text_hover: [u8; 3],
    /// `--ink` standing on [`Self::float_row_selected`].
    pub float_row_text_selected: [u8; 3],
    /// `--ink3` over `--win`.
    pub float_row_muted: [u8; 3],
    /// The same `--ink3`, standing on [`Self::float_row_hover`].
    pub float_row_muted_hover: [u8; 3],
    /// The same `--ink3`, standing on [`Self::float_row_selected`].
    pub float_row_muted_selected: [u8; 3],

    // ── the Git page's third ground (mock-up 1591-1650) ──
    //
    // The Files column's second view brings a surface neither of the two families
    // above has: `.gsec { background: var(--panel) }` — "each section is a soft
    // region card; one fill says *this is a region*" (mock-up 1605-1607). So a
    // row on the Git page does not stand on `--termbg` like a tree row, nor on
    // `--win` like a flyout row, but on a card lifted off `--termbg`, and every
    // translucent ink the page puts on that card is a third pre-mix.
    //
    // The masthead and the group headings are the exception and are named apart
    // below: they sit *outside* the cards, directly on the body, and their inks
    // are therefore mixed over `--termbg` like the tree's.
    //
    // Two of these are numerically fields that already exist —
    // [`Self::git_head_muted`] is [`Self::files_row_muted`], [`Self::git_section`]
    // is [`Self::termhost`] — and are named separately on the precedent this
    // palette has set five times over: two declarations, either of which could be
    // re-struck without the other.
    /// `.gsec { background: var(--panel) }` — the card a group's rows stand on.
    /// Opaque in both themes, so there is nothing to composite.
    pub git_section: [u8; 3],
    /// `.grow:hover { background: var(--hover) }` over the card.
    pub git_row_hover: [u8; 3],
    /// **The selected row of the commit graph** (V8, 2026-08-16).
    ///
    /// Numerically [`Self::files_row_selected`], and that is the *ruling* rather
    /// than a coincidence: a graph's rows stand on the pane's own body exactly as
    /// a tree's do — which is why they already borrow `files_row_hover` for their
    /// hover — so "this row is the one you are on" has to be the same grey in
    /// both places or the window would have two answers to one question.
    ///
    /// Named apart from `files_row_selected` on the precedent this palette has
    /// set six times over: two declarations, either of which could be re-struck
    /// without the other. The three inks that stand on it are
    /// `files_row_{text,muted}_selected`, already premixed over this very value.
    pub git_row_selected: [u8; 3],
    /// **A row the search matched** (T4, 2026-08-16).
    ///
    /// [`Self::git_row_selected`] at half its distance from the body, and the
    /// halving is the ruling. A match is not a selection: the reader's cursor is
    /// on exactly one row and the search may have lit seventeen, so if a matched
    /// row wore the same grey the page would be claiming seventeen cursors. It
    /// has to be *visibly quieter* than the selection standing on top of it, and
    /// it has to survive being drawn under one.
    ///
    /// Derived rather than struck, so it cannot come adrift from the two grounds
    /// it stands between: it is the exact midpoint of the pane body and the
    /// selected row, which is what "half alpha" means once the compositing is
    /// done at build time rather than by the GPU.
    pub git_row_match: [u8; 3],
    /// `.grow bdi { color: var(--ink) }` over the card — a changed file's path,
    /// and a commit's subject.
    pub git_row_text: [u8; 3],
    /// The same `--ink`, standing on [`Self::git_row_hover`].
    pub git_row_text_hover: [u8; 3],
    /// `--ink3` over the card: a commit's short hash, its age, and the mini
    /// graph's line.
    pub git_row_muted: [u8; 3],
    /// The same `--ink3`, standing on [`Self::git_row_hover`].
    pub git_row_muted_hover: [u8; 3],
    /// `.gact { color: var(--ink2) }` over the card.
    ///
    /// Drawn at rest as well as on hover even though the mock-up's button is
    /// `visibility: hidden` until its row is pointed at — R12 replaced that
    /// two-step reveal with `.pv-tool`'s three (0 → .7 → 1), and the middle step
    /// needs an ink to be seven-tenths of.
    pub git_act_glyph: [u8; 3],
    /// The same `--ink2`, standing on [`Self::git_row_hover`].
    pub git_act_glyph_hover: [u8; 3],
    /// `.gact:hover { background: var(--active) }` — over a row that is itself
    /// hovered, because a button cannot be under the pointer while its row is not.
    pub git_act_pill: [u8; 3],
    /// `.gact:hover { color: var(--ink) }`, standing on [`Self::git_act_pill`].
    pub git_act_glyph_on_pill: [u8; 3],
    /// `.git-branch { color: var(--ink) }` over `--termbg` — the branch name,
    /// the largest text on the page.
    pub git_head_text: [u8; 3],
    /// `--ink3` over `--termbg`: the group headings, and the one empty state.
    pub git_head_muted: [u8; 3],
    /// `.gud { color: var(--ink2) }` over `--termbg` — the ahead/behind pills.
    pub git_pill_text: [u8; 3],
    /// `.gud { border: 1px solid var(--border) }` over `--termbg`.
    pub git_pill_border: [u8; 3],
    /// **The commit graph's lane wheel** (R18) — eight roads, told apart by hue.
    ///
    /// The mock-up declared three (`LANE_COLOR = [accent, ok, warn]`) and
    /// indexed straight into them, so a repository with four concurrent branches
    /// painted its fourth road `undefined`. R18 struck that and set a floor of
    /// eight; what is here is a **family** rather than four semantic tokens
    /// stretched to fill it, because a lane is not a status: an amber road does
    /// not mean "careful" and a red one does not mean "wrong". Reusing the
    /// status four as lanes would be the one thing this palette's whole
    /// discipline forbids — a colour that means something, used where it means
    /// nothing.
    ///
    /// **How it is derived.** Eight hues on one ring, anchored so that four of
    /// them land on the hues this product already speaks in — 225° is
    /// [`Self::accent`]'s own hue, 145° the green of `status_ok`, 35° the amber
    /// of `status_warn`, 350° the red of `status_err` — with four more
    /// interleaved between them at roughly even spacing. (The red moved to 347°
    /// when `status_err` became a rose on 2026-08-16; the wheel keeps its 350
    /// because these eight are literals struck once against both canvases, not a
    /// function of the status four, and three degrees is not a colour.) Every
    /// hue is then given
    /// **the same** saturation and lightness, which is what makes the eight read
    /// as one family and not as eight decisions: dark canvas S 52% L 66%, light
    /// canvas S 55% L 38%. Low, deliberately (R18's "low-saturation family"):
    /// these are furniture behind the text, not signals in front of it.
    ///
    /// **Both canvases, checked.** Every entry clears 3:1 — the non-text
    /// contrast floor — against its own canvas *and* against the card ground,
    /// which is the bar these earn: a lane is a 1.7-pixel line and a 7.2-pixel
    /// dot, a graphical object rather than a letter. The two ends of each ladder
    /// are pinned by
    /// `the_graph_lane_wheel_is_eight_colours_that_clear_both_canvases`.
    pub graph_lanes: [[u8; 3]; GRAPH_LANE_COUNT],

    /// A divider at rest: one logical pixel of quiet separation.
    pub divider: [u8; 3],
    /// A divider under the pointer: "this edge is a thing you can touch".
    pub divider_hover: [u8; 3],
    /// A divider being dragged — the accent, and the only saturated colour the
    /// chrome is allowed.
    pub divider_active: [u8; 3],
    /// A collapsed seat's clickable bar (`M2-layout-solver-spec.md` §2.6.3).
    pub collapse_bar: [u8; 3],
    /// The same bar under the pointer.
    pub collapse_bar_hover: [u8; 3],
    /// Window title-bar button hover (`--hover`) composited over `--panel`.
    pub caption_hover: [u8; 3],
    /// Destructive window-close hover from `.capbtn.close-w:hover`.
    pub caption_close_hover: [u8; 3],
    /// Ink on the destructive close hover.
    pub caption_close_text: [u8; 3],
    /// The active horizontal tab, which joins the terminal surface.
    pub active_tab: [u8; 3],
    /// The pane-head surface: exactly `--termbg`, not panel chrome.
    pub pane_head: [u8; 3],
    // ── the pane head's `×`, mixed over the one ground a pane head has ──
    //
    // A pane head is `--termbg` and nothing else: C24 rules out a fill for
    // focus, and there is no hover fill on a pane head either, so unlike the
    // tab's `×` — which has to answer to four grounds — this one has exactly
    // two states over one ground, plus its pill.
    //
    // They are numerically the tab's own three on both canvases, because the
    // active tab and a pane head stand on the same surface. Named separately
    // for the reason [`Self::tab_close_glyph_on_active_tab`] gives for being
    // named separately from [`Self::pane_title`]: two declarations, either of
    // which could be re-struck without the other.
    /// `.panehead .pane-close { color: var(--ink3) }` over `--termbg`.
    pub pane_close_glyph: [u8; 3],
    /// `.panehead .pane-close:hover { background: var(--active) }` over
    /// `--termbg`.
    pub pane_close_pill: [u8; 3],
    /// `.panehead .pane-close:hover { color: var(--ink) }`, standing on
    /// [`Self::pane_close_pill`] and never on the bare head.
    pub pane_close_glyph_on_pill: [u8; 3],
    /// `.termhost { background: var(--panel) }` (mock-up 1022-1023).
    ///
    /// The one surface in the seat layer that is *chrome* rather than terminal,
    /// and the mock-up's own comment says where it is seen: only through the gap
    /// that opens between the panes while a divider is being dragged. Numerically
    /// [`Self::title_bar`] on both canvases and a separate field all the same —
    /// the title bar is the frame around the window and this is the floor the
    /// panes sit on, and F63's card gap is the only place the floor shows.
    pub termhost: [u8; 3],
    /// `--border-soft` composited over the pane-head surface.
    pub pane_head_edge: [u8; 3],
    /// Unfocused `.panehead` ink (`--ink3`) over its terminal surface.
    pub pane_title: [u8; 3],
    /// Focused `.panehead` ink (`--ink`) over its terminal surface.
    pub pane_title_focus: [u8; 3],
    /// The mock-up accent used by structural pane/tab marks.
    pub accent: [u8; 3],
    // ── the command marks rail, mixed over the one ground a terminal pane has ──
    //
    // Three declarations of `design/ui-mockup.html` 1362-1376, and three fields
    // rather than three borrowings, for the reason the pane head's `×` is not
    // [`Self::files_row_muted`]: the numbers coincide today and the rules do not,
    // so either could be re-struck without the other. The rail's rest ink is
    // numerically [`Self::pane_title`] on both canvases — both are `--ink3` over
    // `--termbg` — and its `.fail` ink is [`Self::status_err`] outright, which is
    // the one borrowing that *is* the rule: "signals earn permanent colour" means
    // an error tick wears the window's error red and not a red of its own.
    /// `.cmdtick { background: var(--ink3) }` over `--termbg`.
    ///
    /// The tick's own `opacity: .45` is **not** folded in here. It rides as the
    /// quad's alpha, because it is the property the mock-up animates
    /// (`transition: … opacity .12s ease`) and a colour with an animation baked
    /// into it is a colour that can only be drawn at one moment of it.
    pub command_tick: [u8; 3],
    /// `.cmdrail.hot .cmdtick.crest { background: color-mix(in srgb,
    /// var(--accent) 86%, #000) }` — the tick under the pointer.
    ///
    /// **Fourteen per cent black, and `docs/DESIGN.md` §7.1.5c says twelve.** The
    /// mock-up wins: it is the executable artefact, and the twelve is a middle
    /// state from the same day's second round of errata that reached the prose
    /// and not the stylesheet. The deviation is recorded in §7.1.5c's own
    /// S1-UI note rather than left for the next reader to rediscover.
    pub command_tick_crest: [u8; 3],
    /// `.cmdrail.hot .cmdtick.crest.fail { background: var(--err-deep) }` — the
    /// failed tick under the pointer, deepened the way its neighbours are but in
    /// its own hue, because a signal does not stop being one when it is pointed
    /// at.
    pub command_tick_fail_crest: [u8; 3],
    /// `.cmdrail.srch-mode.hot .cmdtick.crest { background: var(--ink2) }` — the
    /// crest **while the rail is carrying search results** (S4, mock 1546).
    ///
    /// Grey and not the accent, and that is the whole reason it needs a name of
    /// its own: while a search is open the accent belongs to the *matches*, so a
    /// command crest deepening into it would say "this is a hit". The rail's two
    /// sources are told apart by hue, and the crest — the one tick the pointer
    /// has singled out — must not be the exception that breaks the reading.
    ///
    /// Numerically [`Self::files_row_text`] on both canvases, because both are
    /// `--ink2` over `--termbg`; named separately on the precedent that governs
    /// this whole struct — two declarations, either of which could be re-struck
    /// without the other.
    pub command_tick_search_crest: [u8; 3],
    // ── the scroll thumb that runs in the lane beside the rail ──────────────
    //
    // `body, body * { scrollbar-width: thin; scrollbar-color: var(--thumb)
    // transparent }` (`design/ui-mockup.html` 95) is the whole of the mock-up's
    // scroll bar, and the comment above it says what the two tokens are for: *"a
    // chunky opaque thing, and a scrollbar in a terminal should be a mark on the
    // text, not a piece of furniture beside it"* (86-94). So this is a **thumb
    // and no track** — `transparent` is the track, stated rather than defaulted —
    // and the pair below is `--thumb`/`--thumb-hover` composited over the one
    // ground a terminal pane has.
    //
    // Two fields and not four: hover is the only state the design gives a thumb.
    // A *held* thumb wears the hover colour, because the mock-up's stylesheet has
    // no `:active` on it and inventing one would be this palette answering a
    // question the design did not ask.
    /// `--thumb` over `--termbg`: white at .22 on night, `#37352F` at .24 on
    /// paper.
    ///
    /// The two alphas differ by two hundredths and that is the design's own
    /// asymmetry, the same one `--ink`/`--ink2`/`--ink3` carry between `:root`
    /// and `body.dark`: ink laid on paper covers more per unit of alpha than ink
    /// laid on night, so a light thumb needs a shade more to read as the same
    /// mark. Folding them into one number would make the light bar the fainter
    /// of the two, which is the theme it is hardest to see on.
    pub scroll_thumb: [u8; 3],
    /// `--thumb-hover` over `--termbg`: .4 on night, .42 on paper — the same
    /// mark, brought forward while a hand is near it.
    ///
    /// **Brightness and not width.** `scrollbar-width: thin` is declared once for
    /// every state the mock-up has; nothing in it widens under the pointer, and
    /// a bar that grew would move the character cell under it, which is exactly
    /// what the reserved lane exists to stop happening.
    pub scroll_thumb_hover: [u8; 3],
    /// A floating window's face — `--menu`, worn by `.float-win`, the term menu
    /// and the hover-peek flyout. It is deliberately *not* `title_bar`: a window
    /// that floats over content is a different plane from the chrome that frames
    /// it, and the mock-up gives that plane its own variable.
    pub menu_surface: [u8; 3],
    /// A floating window's hairline — `--border`, kept as the mock-up's own
    /// colour *and* alpha rather than pre-composited like every field above it.
    ///
    /// The rest of this palette may composite because each of its hairlines sits
    /// on a surface we know. A flyout floats over whatever the terminal happens
    /// to be showing, so there is no such surface, and the honest hairline is the
    /// one the renderer blends at draw time.
    pub menu_border: [u8; 3],
    /// `--border`'s alpha, in 1/255ths: `.094` white on dark, `.088` black on light.
    pub menu_border_alpha: u8,
    /// The colour a floating window's shadow is cast in (`--shadow`: black at
    /// both stops, on both themes).
    pub menu_shadow: [u8; 3],
    /// The inner of the two shadow rings, in 1/255ths — see
    /// [`FLOAT_WINDOW_SHADOW_LOGICAL_PX`] for what these two rings can and cannot
    /// stand in for.
    pub menu_shadow_inner_alpha: u8,
    /// The outer ring, roughly half the inner one, so the two compose into a
    /// falloff instead of a band.
    pub menu_shadow_outer_alpha: u8,
    /// A popup menu's own lift (`.combo-menu { box-shadow: 0 10px 28px
    /// rgba(0,0,0,.18) }`), split into the same two rings. It is *not* `--shadow`
    /// and is deliberately not theme-varied: the mock-up gives the combo's menu
    /// one shadow declaration and never overrides it on dark.
    pub menu_popup_shadow_inner_alpha: u8,
    /// Half the inner one, same falloff rule as the float window's.
    pub menu_popup_shadow_outer_alpha: u8,
    /// A tooltip's own lift (`.tip { box-shadow: 0 4px 14px rgba(0,0,0,.1) }`,
    /// mock-up 1217), split into the same two rings.
    ///
    /// A third pair rather than a reuse of either above it, because the mock-up
    /// writes a third declaration and — unlike the combo's — overrides it on dark
    /// (`body.dark .tip { … rgba(0,0,0,.45) }`, line 1219). The gap is the whole
    /// point: a tip is the smallest thing that floats, and on dark it is a small
    /// pale box on a dark plane, which needs a far heavier lift to read as
    /// floating than a large one does. Borrowing `--shadow`'s .18 would have left
    /// it flat against the night.
    pub tip_shadow_inner_alpha: u8,
    /// Half the inner one, same falloff rule as every other floating surface's.
    pub tip_shadow_outer_alpha: u8,
    /// The drag ghost's own lift (`.drag-ghost { box-shadow: 0 8px 24px
    /// rgba(0,0,0,.25) }`, mock-up 1723), split into the same two rings.
    ///
    /// A fourth pair, and — unlike the tip's — deliberately *not* theme-varied:
    /// the mock-up writes this declaration once and never overrides it on dark.
    /// The tip needed the override because it is a small pale box that has to
    /// separate from a dark plane it may be sitting flat against; the ghost is
    /// always moving, and motion has already said it is above everything.
    pub drag_ghost_shadow_inner_alpha: u8,
    /// Half the inner one, same falloff rule as every other floating surface's.
    pub drag_ghost_shadow_outer_alpha: u8,
    /// A transient float's own lift (`#files-flyout { box-shadow: 0 12px 34px
    /// rgba(0,0,0,.20) }`, mock-up 676), split into the same two rings.
    ///
    /// A fifth pair, theme-varied like the tip's (`body.dark #files-flyout { …
    /// rgba(0,0,0,.5) }`, line 679) and for the tip's reason at a larger size: a
    /// pale panel on a dark plane needs a heavier lift to read as floating than
    /// the same panel on a light one.
    pub float_shadow_inner_alpha: u8,
    /// Half the inner one, same falloff rule as every other floating surface's.
    pub float_shadow_outer_alpha: u8,
    /// A *pinned* float's lift (`.float-win.pinned { box-shadow: 0 16px 40px
    /// rgba(0,0,0,.24) }`, mock-up 702; `.58` on dark, line 703).
    ///
    /// A sixth pair rather than a reuse of the fifth, because the mock-up writes
    /// a second declaration and means it: a peek is a thing hovering a moment
    /// over the window, and a pinned window has been *torn off* it. The extra
    /// lift is the whole of how that reads at a glance, and it is the only
    /// visual difference between the two modes that survives being still.
    pub float_pinned_shadow_inner_alpha: u8,
    /// Half the inner one, same falloff rule as every other floating surface's.
    pub float_pinned_shadow_outer_alpha: u8,
    /// The file glance card's own lift (`.file-peek { box-shadow: 0 10px 28px
    /// rgba(0,0,0,.18) }`, mock-up 1785; `.5` on dark, line 1788).
    ///
    /// A seventh pair, and it is the card's own declaration rather than the
    /// tooltip's, which is what the card borrowed until 2026-08-13. The two are
    /// nearly two-to-one apart on light (.18 against .1): a tooltip is a strip of
    /// words that barely leaves the surface, and the glance is a 300px document
    /// standing over a file tree. Borrowing the tip's numbers left it lying flat
    /// against the rows it was supposed to be hovering above.
    ///
    /// The dark declaration's 32px of reach is not carried — the card asks for 28
    /// on both themes. The curve meets zero with zero slope at the reach, so four
    /// more pixels of a tail that is already under one 255th changes nothing that
    /// can be drawn; the alpha is the whole of the difference and the alpha is
    /// here.
    pub peek_card_shadow_inner_alpha: u8,
    /// Half the inner one, same falloff rule as every other floating surface's.
    pub peek_card_shadow_outer_alpha: u8,
    /// A modal dialog's face — `--win`. Three surfaces have to be told apart here
    /// and the mock-up names all three: `--termbg` is what a terminal shows,
    /// `--menu` is what floats over it, and `--win` is the window's own plane,
    /// which a modal dialog borrows. On light all three are `#FFFFFF`; on dark
    /// they are three different greys, which is why one field cannot serve.
    pub dialog_surface: [u8; 3],
    /// `--ink` over `--win`: the dialog's title, a row's own title, the value a
    /// combo is showing.
    pub dialog_title_text: [u8; 3],
    /// `--ink2` over `--win`: a row's description, and the `×` at rest.
    pub dialog_secondary_text: [u8; 3],
    /// `--ink3` over `--win`: a group label, and a combo's chevron.
    pub dialog_muted_text: [u8; 3],
    /// `--hover` over `--win`: the `×`'s pill and a combo button under the pointer.
    pub dialog_hover: [u8; 3],
    /// `--ink2` over `--menu`: a combo item that is not the selected one.
    pub menu_item_text: [u8; 3],
    /// `--ink` over `--menu` (`.combo-item.selected { color: var(--ink) }`).
    pub menu_item_text_selected: [u8; 3],
    /// `--hover` over `--menu`: a combo item under the pointer.
    pub menu_item_hover: [u8; 3],
    /// `--active` over `--menu`: the focused cell of a layout peek's schematic
    /// (`.mini-leaf.focused { background: var(--active) }`, mock-up 1921).
    ///
    /// A fourth wash rather than a reuse of [`Self::menu_item_hover`], because
    /// `--hover` and `--active` are two different tokens — `.055` against `.09`
    /// (mock-up 24-25) — and the mock-up spends them on two different states.
    /// Borrowing the hover would have drawn "this is the pane you are in" at the
    /// weight of "the pointer happens to be here".
    pub peek_leaf_focus_fill: [u8; 3],
    /// `--ink3` over [`Self::peek_leaf_focus_fill`]: that cell's border
    /// (`.mini-leaf.focused { border-color: var(--ink3) }`).
    ///
    /// Composited over the wash rather than over `--menu`, because that is what
    /// it sits on — the leaf's background paints under its own border
    /// (`background-clip: border-box`), so the hairline never touches the menu's
    /// face. The difference is 12/255 on dark, which is most of a step in a
    /// palette whose steps are this small.
    pub peek_leaf_focus_edge: [u8; 3],
    /// `--ink` over [`Self::peek_leaf_focus_fill`]: that cell's name
    /// (`.mini-leaf.focused { color: var(--ink) }`).
    pub peek_leaf_focus_text: [u8; 3],
    /// A modal scrim's colour (`.overlay { background: rgba(15,15,15,.35) }`).
    /// Blended at draw time over whatever the window happens to be showing, and
    /// the one palette entry the mock-up declares once for both themes: a scrim
    /// is not a surface of either palette, it is the absence of one.
    pub modal_scrim: [u8; 3],
    /// The scrim's alpha, in 1/255ths: `.35`.
    pub modal_scrim_alpha: u8,

    // ── `.panecount`, the tab's pane-count badge (mock-up lines 292-304) ──
    //
    // The badge is `background: var(--active)` on every tab and takes its ink
    // from the tab's state — `--ink2` at rest, `--ink` on the active tab. That
    // is one declaration landing on three different surfaces, so it composites
    // into three pairs rather than one translucent fill, for exactly the reason
    // spelled out at `tab_close_pill_on_content`: this pipeline blends in linear
    // light and a browser blends in sRGB, so handing the blender `--active`'s
    // own .09 does not reproduce `--active`.
    //
    // Two of the three *fills* are already in this palette, because the `×`'s
    // pill is the same `--active` over the same two surfaces: use
    // `tab_close_pill_on_content` on the active tab and
    // `tab_close_pill_on_hovered_tab` on a hovered one. Only the resting tab's
    // fill is new — the `×` is never drawn there, so nothing had needed it.
    /// `--active` over `--panel`: the badge on a tab that is neither the active
    /// one nor under the pointer. The third surface the `×`'s pill never meets.
    pub tab_badge_on_resting_tab: [u8; 3],
    /// `.tab.active .panecount { color: var(--ink) }`, over the badge's own fill
    /// on the active tab — which is `--active` over `--termbg`, not `--termbg`,
    /// so this is a step off the tab title's own ink and not the same value.
    ///
    /// Deliberately **not** the accent: the mock-up's comment at line 297 rules
    /// that filling this badge with the accent on the active tab made it say
    /// "you are here" in the colour reserved for "that one wants you", in the
    /// strip where unread dots live.
    pub tab_badge_text_on_active_tab: [u8; 3],
    /// `--ink2` over the badge's fill on a resting tab.
    pub tab_badge_text_on_resting_tab: [u8; 3],
    /// `--ink2` over the badge's fill on a hovered tab.
    pub tab_badge_text_on_hovered_tab: [u8; 3],

    /// `--ink3` over `--menu` — a menu row's trailing annotation, worn by the
    /// profile picker's `default` hint (`.default-hint`, mock-up line 998).
    ///
    /// It exists because the nearest thing already in this palette,
    /// `dialog_muted_text`, is the same ink over `--win`: right for the settings
    /// dialog it is named for, and a surface too dark for a menu. The two agree
    /// exactly in the light theme, where `--win` and `--menu` are both white,
    /// and part by six levels in the dark one — which is precisely the kind of
    /// error a shared name hides.
    pub menu_item_hint_text: [u8; 3],

    // ── Status semantics (mock-up lines 20-46, 74) ──
    //
    // The mock-up declares these in `:root` with a comment that rules them
    // explicitly: "every 'something happened' colour goes through these four,
    // never a literal". They are opaque hex in the design, so unlike most of
    // this palette there is nothing to pre-composite — they land as written.
    //
    // **Two of them are one set and three are not** (R29, 2026-08-15; widened
    // 2026-08-16). The comment above them in the design says the four are
    // declared once "so both themes share them **until a walkthrough proves a
    // theme needs its own**", and `--ok` was the first the walkthrough caught:
    // `body.dark` overrides it to `#57ab5a` (mock-up 74) because `#1a7f37` on
    // `#1B1B1B` is a green nobody can read. `--err` is the second, and it split
    // for the same reason the moment it became a rose — a rose dark enough to
    // clear 4.5:1 on white is a bruise on `#1B1B1B`, and one light enough to
    // read on `#1B1B1B` is unreadable on white. So the set that varies by canvas
    // is [`Self::accent`], [`Self::status_ok`] and [`Self::status_err`], and the
    // set that does not is `--warn` and `--pause`. Naming the split is the whole
    // point — a reader who was told "the status colours are one table" and then
    // found a per-theme entry would rightly distrust the rest.
    /// `--err` — `#e11d48` light, `#fb7185` dark. A session that finished with a
    /// failing exit code, worn by the tab's dot, by a progress ring reporting
    /// `OSC 9;4` state 2, by the Git page's `D` and `U` badges (a file gone, a
    /// merge unresolved), and by a toast that carries a failure.
    ///
    /// **A rose and not the danger red** (user ruling, 2026-08-16). The mock-up
    /// declared `#c50f1f`, which is GitHub's own danger red, and on a refused
    /// checkout it read as a fire alarm over a sentence that only means "git
    /// would not do that". The ruling replaces it with the pinkish red modern
    /// minimal interfaces use — Tailwind's rose-600 on the light canvas, rose-400
    /// on the dark — and the mock-up's literal was retuned with it rather than
    /// left behind, so the design record and this table still say one thing.
    /// Both clear 4.5:1 against their own canvas, which the danger red did not
    /// on the dark one.
    pub status_err: [u8; 3],
    /// `--warn #d9822b` — the bell, and (once the attention queue lands) an
    /// agent blocked on you.
    pub status_warn: [u8; 3],
    /// `--pause #c19c00` — a progress ring reporting `OSC 9;4` state 4.
    pub status_pause: [u8; 3],
    /// `--ok` — `#1a7f37` light, `#57ab5a` dark. The Git page's `A` and `C`
    /// badges, and (in G-4) the merge line curving into the mini graph.
    ///
    /// **The one status colour with two values**, for the reason the block
    /// comment above gives. It arrived with the Git panel because that panel is
    /// the first surface in the product to stand a green, a blue and a red side
    /// by side at rest, which is the arrangement that makes an unreadable one
    /// obvious.
    pub status_ok: [u8; 3],

    // ── The progress ring's track (mock-up line 278) ──
    //
    // `.pring .track { stroke: var(--border); opacity: .7 }` — one declaration
    // landing on the three surfaces a tab can wear, pre-composited into three
    // entries for the reason spelled out at `tab_close_pill_on_content`.
    //
    // There is no fourth: a ring is only ever drawn in a tab's mark slot, and a
    // tab is exactly one of active, hovered, or at rest.
    /// `--border` at `.7` over the active tab, whose fill is `--termbg`.
    pub ring_track_on_active_tab: [u8; 3],
    /// The same over a resting tab, whose fill is `--panel`.
    pub ring_track_on_resting_tab: [u8; 3],
    /// The same over a hovered tab — `--hover` over `--panel`, which is the
    /// value [`Self::caption_hover`] already carries.
    pub ring_track_on_hovered_tab: [u8; 3],

    // ── R1: the rail's own surfaces ──
    //
    // The rail stands on `--panel`, exactly as the title bar does, so its ground
    // is [`Self::title_bar`] and needs no field. Most of what it draws is
    // likewise already here, because the ink and the ground are both the strip's:
    //
    // * a row's ink at rest is `--ink2` over `--panel` = [`Self::title_text`];
    // * the "Tabs" heading and a resting row's `×`/pin are `--ink3` over
    //   `--panel` = [`Self::title_text_muted`];
    // * a row's hover fill is `--hover` over `--panel` = [`Self::caption_hover`];
    // * a hovered row's `×` is `--ink3` over that = already
    //   [`Self::tab_close_glyph_on_hovered_tab`].
    //
    // What is genuinely new is the *active row*, and it is new for a structural
    // reason rather than an oversight: the horizontal strip's active tab is
    // `--termbg` — it is shaped to join the terminal — while the rail's active
    // row never leaves the panel, so it is `--active` over `--panel`. That ground
    // exists nowhere in the strip, so every ink standing on it is a new mix.
    /// `.vtab.active { background: var(--active) }` — `--active` over `--panel`.
    ///
    /// Numerically [`Self::tab_badge_on_resting_tab`] on both canvases, and a
    /// separate field for the reason [`Self::termhost`] is separate from
    /// [`Self::title_bar`]: a badge on a resting tab and the rail's selection are
    /// two declarations that happen to have been struck from the same pair, and
    /// either could be re-struck without the other.
    pub rail_tab_active: [u8; 3],
    /// `.vtab.active { color: var(--ink) }` over [`Self::rail_tab_active`].
    ///
    /// The mock-up also puts this row at `font-weight: 500`, which is not a
    /// colour and so is not here — but the two are one decision: the active row
    /// is told apart by weight *and* ink, never by ink alone.
    pub rail_tab_active_text: [u8; 3],
    /// `--ink2` over `--hover` over `--panel` — a hovered row's title.
    ///
    /// `.vtab:hover` changes only the background; the ink stays `--ink2`. So this
    /// is the same declaration as [`Self::title_text`] standing on a different
    /// ground, and on dark the two are 0x9D and 0xA2 — five levels apart, which
    /// is exactly the kind of drift a single shared field would hide.
    pub rail_tab_hover_text: [u8; 3],
    /// `--ink3` over [`Self::rail_tab_active`] — the `×` on the active row.
    ///
    /// The rail needs this where the strip never did, and Q174 is why: in the
    /// strip a narrow active tab is the *only* one that keeps its `×`, but it
    /// keeps it on `--termbg`. Here every unpinned row wears its `×` at rest,
    /// including the active one, so the glyph has to be mixed over the selection
    /// fill as well.
    pub rail_glyph_on_active_tab: [u8; 3],
    /// `.pin-seam { background: var(--border) }` over `--panel`.
    ///
    /// `--border` and not `--border-soft`: the seam is the one line in the rail
    /// that has to be *read* rather than merely felt, because in icon mode it is
    /// the only remaining statement that a row is pinned.
    pub rail_seam: [u8; 3],
    /// `.rail { border-right: 1px solid var(--border-soft) }` over `--panel`.
    ///
    /// Not [`Self::pane_head_edge`], which is the same `--border-soft` over the
    /// pane head's `--termbg`: same declaration, different ground, and on light
    /// they part by a level.
    pub rail_edge: [u8; 3],
    /// The shade the open rail casts on the terminal — black at both stops on
    /// both canvases (`linear-gradient(to right, rgba(0,0,0,α), rgba(0,0,0,0))`).
    ///
    /// Kept as a colour plus an alpha rather than pre-composited, for the reason
    /// [`Self::menu_border`] gives: this gradient falls on whatever the terminal
    /// happens to be showing, so there is no known surface to mix it over.
    pub rail_shade: [u8; 3],
    /// The shade's alpha at its inner stop, in 1/255ths — `.09` on light and
    /// `.34` on dark.
    ///
    /// The dark canvas needs nearly four times the light one because a shadow
    /// works by darkening, and there is very little headroom left to darken
    /// `--termbg #1B1B1B` with: the same `.09` that reads as a soft edge on white
    /// is invisible on night.
    pub rail_shade_alpha: u8,

    // ── the focus column's cards (§7.1.6b′) ──
    //
    // A card is a tab, so every mark it wears is the strip's own — the profile
    // mark, the dot, the ring, the badge, the pin, the `×`. What is new is only
    // the surface they stand on: the rail's rows lie directly on `--panel`,
    // while a card is an object with its own `--menu` face and a `--hover` head
    // over it. Same declarations, a different ground, so each one composites
    // again — which is the very discipline `rail_tab_hover_text` was added
    // under.
    /// `.fc-head { background: var(--hover) }` over the card's `--menu` face.
    ///
    /// In F1 this *is* the card: the slice ships the head alone (the F2
    /// thumbnail is the body that will hang under it), so this fill covers
    /// everything inside the card's border.
    pub focus_card: [u8; 3],
    /// `.fcard.staged .fc-head { background: var(--active) }` — the tab that is
    /// currently on the stage, marked in the column rather than removed from it.
    pub focus_card_staged: [u8; 3],
    /// `.fc-head { color: var(--ink2) }` over [`Self::focus_card`] — a card's
    /// name.
    pub focus_card_title: [u8; 3],
    /// `.fcard.staged .fc-head { color: var(--ink) }` over
    /// [`Self::focus_card_staged`].
    pub focus_card_title_staged: [u8; 3],
    /// `--ink3` over [`Self::focus_card`] — the resting `×` and the pin mark,
    /// which the mock-up sets from one declaration (`.fc-head .pinsvg` and
    /// `.fc-head .fc-close` are both `var(--ink3)`).
    pub focus_card_glyph: [u8; 3],
    /// The same over [`Self::focus_card_staged`].
    pub focus_card_glyph_staged: [u8; 3],
    /// `--active` over [`Self::focus_card`] — the pane-count badge's pill, and
    /// the `×`'s own pill under the pointer.
    ///
    /// One field for both because the mock-up strikes both from `var(--active)`
    /// on the same ground; two fields would be two definitions of one colour.
    pub focus_card_pill: [u8; 3],
    /// The same over [`Self::focus_card_staged`].
    pub focus_card_pill_staged: [u8; 3],
    /// `--ink` over [`Self::focus_card_pill`] — the `×` on its hover pill.
    pub focus_card_ink_on_pill: [u8; 3],
    /// `--ink` over [`Self::focus_card_pill_staged`] — the `×` on its pill, and
    /// the staged card's badge digits, which the strip also lifts to `--ink`
    /// (`.tab.active .panecount, .vtab.active .panecount { color: var(--ink) }`).
    pub focus_card_ink_on_pill_staged: [u8; 3],
    /// `--ink2` over [`Self::focus_card_pill`] — a resting card's badge digits.
    pub focus_card_muted_on_pill: [u8; 3],
    /// `.fcard { border: 1px solid var(--border) }` over `--panel`, which is the
    /// ground the card's own edge stands on — and the same hairline the Exit
    /// button wears.
    ///
    /// Numerically [`Self::rail_seam`] on both canvases, and struck from the
    /// shared `PANEL_HAIRLINE_*` rather than written twice: it is not a
    /// coincidence, it is the same `--border` on the same `--panel`.
    pub focus_card_edge: [u8; 3],
    /// The progress ring's track on a resting card — `--border` at
    /// `opacity: .7`, the same ten-thousandths the strip's three tracks use.
    pub ring_track_on_focus_card: [u8; 3],
    /// The same on a staged card.
    pub ring_track_on_focus_card_staged: [u8; 3],
    /// `.focus-exit { background: var(--win) }` — door 5's face. Opaque on both
    /// canvases, so there is nothing to composite.
    pub focus_exit: [u8; 3],
    /// `.focus-exit { color: var(--ink2) }` over [`Self::focus_exit`].
    pub focus_exit_text: [u8; 3],
    /// `.focus-exit:hover { background: var(--hover) }` over the same face.
    pub focus_exit_hover: [u8; 3],
    /// `.focus-exit:hover { color: var(--ink) }` over [`Self::focus_exit_hover`].
    pub focus_exit_text_hover: [u8; 3],
}

/// Chrome over a dark canvas — `design/ui-mockup.html` `body.dark`, with its
/// alpha hairlines pre-composited over the surface each one actually sits on
/// (our chrome quads are opaque): `--termbg #1B1B1B`, `--panel #252525`,
/// `--ink/2/3` at .87/.55/.38 white, `--border` at .094 white,
/// `--accent #7A99FF`.
pub const DARK_CHROME: ChromePalette = ChromePalette {
    seat_body: [0x1b, 0x1b, 0x1b],
    title_bar: [0x25, 0x25, 0x25],
    title_text: [0x9d, 0x9d, 0x9d],
    title_text_hover: [0xe3, 0xe3, 0xe3],
    title_text_muted: [0x78, 0x78, 0x78],
    tab_close_pill_on_content: [0x30, 0x30, 0x30],
    tab_close_pill_on_hovered_tab: [0x44, 0x44, 0x44],
    // `--ink3` (white .38) over `--termbg` #1B1B1B: 27 + 228×.38 = 113.6.
    tab_close_glyph_on_active_tab: [0x72, 0x72, 0x72],
    // The same ink over `--hover` (white .055) over `--panel` #252525, which is
    // 37 + 218×.055 = 49.0: 49.0 + 206×.38 = 127.3.
    tab_close_glyph_on_hovered_tab: [0x7f, 0x7f, 0x7f],
    // `--ink` (white .87) over the two pills — 47.5 on the active tab, 67.5 on
    // a hovered one: 47.5 + 207.5×.87 = 228.0, and 67.5 + 187.5×.87 = 230.6.
    tab_close_glyph_on_pill_over_active_tab: [0xe4, 0xe4, 0xe4],
    tab_close_glyph_on_pill_over_hovered_tab: [0xe7, 0xe7, 0xe7],
    // `--ink` over the two bare tab surfaces a pinned pin can stand on:
    // 27 + 228×.87 = 225.4, and 49.0 + 206×.87 = 228.2.
    tab_pin_state_on_active_tab: [0xe1, 0xe1, 0xe1],
    tab_pin_state_on_hovered_tab: [0xe4, 0xe4, 0xe4],
    body_hint_text: [0x75, 0x75, 0x75],
    // `--ink` is white at .87 over `--termbg #1B1B1B` = 27: 27 + 228×.87 = 225.4
    // — which is #E1, the terminal's own default ink, arrived at over the bare
    // body rather than over the selection fill.
    //
    // **Corrected 2026-08-17 by the derivation pin.** It read #E4 for a year:
    // that is `--ink` over the *selection fill* (`files_row_text_selected`),
    // transcribed onto a surface it does not stand on, while the comment above
    // it had the right sum all along. The light half of this field is `--ink`
    // itself, so the two halves now say one thing.
    preview_body_text: [0xe1, 0xe1, 0xe1],
    // `--border-soft` (white .06) over `--termbg` 27: 27 + 228×.06 = 40.7.
    preview_grid_line: [0x29, 0x29, 0x29],
    // `--panel #252525` = 37, and the same hairline over *it*:
    // 37 + 218×.06 = 50.1.
    preview_code_ground: [0x25, 0x25, 0x25],
    preview_code_border: [0x32, 0x32, 0x32],
    // `--ink2` (white .55) and `--ink3` (white .38) over that 37:
    // 37 + 218×.55 = 156.9, 37 + 218×.38 = 119.8.
    preview_code_text: [0x9d, 0x9d, 0x9d],
    preview_code_lang: [0x78, 0x78, 0x78],
    // `--ok #57ab5a` at 13% over 27: (34.8, 45.7, 35.2).
    preview_diff_add: [0x23, 0x2e, 0x23],
    // `--err #fb7185` at 10% over 27: (49.4, 35.6, 37.6).
    preview_diff_del: [0x31, 0x24, 0x26],
    preview_diff_hunk: [0x7a, 0x99, 0xff],
    // `--ink` (white .87) over `--hover` over `--termbg`: 27 + 228×.055 = 39.5,
    // then 39.5 + 215.5×.87 = 227.0.
    preview_table_head_text: [0xe3, 0xe3, 0xe3],
    preview_selection: DEFAULT_SELECTION_BACKGROUND_RGB,
    preview_caret: DEFAULT_CURSOR_RGB,
    // The highlight family, walked up from black in each hue until both this
    // theme's canvases — `--termbg #1B1B1B` and a fence's `--panel #252525` —
    // clear 4.5:1, and stopped at the first step that does. "As loud as the
    // floor demands and not a step louder" is the whole rule; the pin below
    // reads back 5.19/4.62 for the violet and its neighbours are within a
    // tenth of that.
    hl_keyword: [0xae, 0x75, 0xd7],
    hl_string: [0x47, 0x9e, 0x6b],
    hl_comment: [0x82, 0x8f, 0xa1],
    hl_number: [0xba, 0x83, 0x36],
    hl_type: [0x45, 0x98, 0xa8],
    // The accent's hue, not the accent: `--accent #7A99FF` would clear the bar
    // (6.42:1 here) but it is this window's word for *a link and a focus ring*,
    // and a page of function calls painted in it reads as a page of links.
    hl_function: [0x6e, 0x8a, 0xdd],
    hl_punct_muted: [0x82, 0x8f, 0xa1],
    // The file tree's three grounds on `--termbg #1B1B1B`:
    //   `--hover`  white .055 → 27 + 228×.055 = 39.5
    //   `--active` white .09  → 27 + 228×.09  = 47.5
    files_row_hover: [0x28, 0x28, 0x28],
    files_row_selected: [0x30, 0x30, 0x30],
    // `--ink2` (white .55) over each: 27 + 228×.55 = 152.4,
    // 39.5 + 215.5×.55 = 158.0. And `--ink` (white .87) over the selected
    // fill: 47.5 + 207.5×.87 = 228.0.
    files_row_text: [0x98, 0x98, 0x98],
    files_row_text_hover: [0x9e, 0x9e, 0x9e],
    files_row_text_selected: [0xe4, 0xe4, 0xe4],
    // `--ink3` (white .38) over the same three: 27 + 228×.38 = 113.6, then over
    // the two fills **as they are painted** — 40 and 48, the very bytes two
    // fields above — 40 + 215×.38 = 121.7 and 48 + 207×.38 = 126.7.
    //
    // **Corrected 2026-08-17 by the derivation pin.** These two read 0x79 and
    // 0x7E, computed from the unrounded 39.5 and 47.5 while the fills under
    // them go to the GPU as 40 and 48. An ink is composited over the surface
    // that is drawn, not over the real number that surface was rounded from.
    files_row_muted: [0x72, 0x72, 0x72],
    files_row_muted_hover: [0x7a, 0x7a, 0x7a],
    files_row_muted_selected: [0x7f, 0x7f, 0x7f],
    // The same eight over `--win #202020` = 32, which is the ground inside a
    // floating window:
    //   `--hover`  white .055 → 32 + 223×.055 = 44.3
    //   `--active` white .09  → 32 + 223×.09  = 52.1
    float_row_hover: [0x2c, 0x2c, 0x2c],
    float_row_selected: [0x34, 0x34, 0x34],
    // `--ink2` (white .55) over the bare face and over the hover fill:
    // 32 + 223×.55 = 154.7, 44.3 + 210.7×.55 = 160.2. And `--ink` (white .87)
    // over the selected fill: 52.1 + 202.9×.87 = 228.6.
    float_row_text: [0x9b, 0x9b, 0x9b],
    float_row_text_hover: [0xa0, 0xa0, 0xa0],
    float_row_text_selected: [0xe5, 0xe5, 0xe5],
    // `--ink3` (white .38) over the same three: 32 + 223×.38 = 116.7,
    // 44.3 + 210.7×.38 = 124.4, 52.1 + 202.9×.38 = 129.2.
    float_row_muted: [0x75, 0x75, 0x75],
    float_row_muted_hover: [0x7c, 0x7c, 0x7c],
    float_row_muted_selected: [0x81, 0x81, 0x81],
    // The Git page's card, and the six inks that stand on it. Derived rather
    // than transcribed — see `ink_over`.
    git_section: PANEL_DARK,
    git_row_hover: GIT_ROW_HOVER_DARK,
    // The graph's selected row stands on `--termbg` with the tree's, not on the
    // card — see the field.
    git_row_selected: [0x30, 0x30, 0x30],
    // Half way from the pane body to that — see the field.
    git_row_match: ink_over(TERMBG_DARK, [0x30, 0x30, 0x30], 500),
    git_row_text: ink_over(PANEL_DARK, DARK_INK_SOURCE, 870),
    git_row_text_hover: ink_over(GIT_ROW_HOVER_DARK, DARK_INK_SOURCE, 870),
    git_row_muted: ink_over(PANEL_DARK, DARK_INK_SOURCE, 380),
    git_row_muted_hover: ink_over(GIT_ROW_HOVER_DARK, DARK_INK_SOURCE, 380),
    git_act_glyph: ink_over(PANEL_DARK, DARK_INK_SOURCE, 550),
    git_act_glyph_hover: ink_over(GIT_ROW_HOVER_DARK, DARK_INK_SOURCE, 550),
    git_act_pill: GIT_ACT_PILL_DARK,
    git_act_glyph_on_pill: ink_over(GIT_ACT_PILL_DARK, DARK_INK_SOURCE, 870),
    // Outside the cards, on the pane's own body.
    git_head_text: ink_over(TERMBG_DARK, DARK_INK_SOURCE, 870),
    git_head_muted: ink_over(TERMBG_DARK, DARK_INK_SOURCE, 380),
    git_pill_text: ink_over(TERMBG_DARK, DARK_INK_SOURCE, 550),
    git_pill_border: ink_over(TERMBG_DARK, DARK_INK_SOURCE, 94),
    graph_lanes: GRAPH_LANES_DARK,
    divider: [0x35, 0x35, 0x35],
    divider_hover: [0x51, 0x51, 0x51],
    divider_active: [0x7a, 0x99, 0xff],
    collapse_bar: [0x25, 0x25, 0x25],
    collapse_bar_hover: [0x31, 0x31, 0x31],
    caption_hover: [0x31, 0x31, 0x31],
    caption_close_hover: [0xe5, 0x48, 0x4d],
    caption_close_text: [0xff, 0xff, 0xff],
    active_tab: [0x1b, 0x1b, 0x1b],
    pane_head: [0x1b, 0x1b, 0x1b],
    // `--ink3` (white .38) over `--termbg` #1B1B1B: 27 + 228×.38 = 113.6.
    pane_close_glyph: [0x72, 0x72, 0x72],
    // `--active` (white .09) over the same: 27 + 228×.09 = 47.5.
    pane_close_pill: [0x30, 0x30, 0x30],
    // `--ink` (white .87) over that pill: 47.5 + 207.5×.87 = 228.0.
    pane_close_glyph_on_pill: [0xe4, 0xe4, 0xe4],
    termhost: [0x25, 0x25, 0x25],
    pane_head_edge: [0x29, 0x29, 0x29],
    // `--ink3` over `--termbg #1B1B1B`, not over `--win #202020`: the pane head
    // is the terminal surface (see `pane_head` above), and mixing this ink over
    // the dialog's grey instead — which is where the `0x75` this replaced came
    // from — makes an unfocused pane title three levels too pale.
    pane_title: [0x72, 0x72, 0x72],
    pane_title_focus: [0xe1, 0xe1, 0xe1],
    accent: [0x7a, 0x99, 0xff],
    // `--ink3` (white .38) over the dark `--termbg`, which is `pane_title`'s own
    // mix; `#7A99FF` at 86% over black; and the dark `--err-deep` `#f43f5e`.
    command_tick: [0x72, 0x72, 0x72],
    command_tick_crest: [0x69, 0x84, 0xdb],
    command_tick_fail_crest: [0xf4, 0x3f, 0x5e],
    // `--ink2` (white .72) over the dark `--termbg` — the same mix a file row's
    // name is set in.
    command_tick_search_crest: [0x98, 0x98, 0x98],
    // `--thumb` (white .22) and `--thumb-hover` (white .4) over the dark
    // `--termbg`.
    scroll_thumb: [0x4d, 0x4d, 0x4d],
    scroll_thumb_hover: [0x76, 0x76, 0x76],
    menu_surface: [0x2a, 0x2a, 0x2a],
    menu_border: [0xff, 0xff, 0xff],
    menu_border_alpha: 24,
    menu_shadow: [0x00, 0x00, 0x00],
    menu_shadow_inner_alpha: 46,
    menu_shadow_outer_alpha: 23,
    menu_popup_shadow_inner_alpha: 46,
    menu_popup_shadow_outer_alpha: 23,
    // `.45` black — the dark override at mock-up 1219.
    tip_shadow_inner_alpha: 115,
    tip_shadow_outer_alpha: 57,
    // `.25` black, the ghost's single declaration: 255 × .25 = 63.75.
    drag_ghost_shadow_inner_alpha: 64,
    drag_ghost_shadow_outer_alpha: 32,
    // `.5` and `.58` black — the dark overrides at mock-up 679 and 703.
    float_shadow_inner_alpha: 128,
    float_shadow_outer_alpha: 64,
    float_pinned_shadow_inner_alpha: 148,
    float_pinned_shadow_outer_alpha: 74,
    // `.5` black — the glance card's dark override at mock-up 1788.
    peek_card_shadow_inner_alpha: 128,
    peek_card_shadow_outer_alpha: 64,
    dialog_surface: [0x20, 0x20, 0x20],
    dialog_title_text: [0xe2, 0xe2, 0xe2],
    dialog_secondary_text: [0x9b, 0x9b, 0x9b],
    dialog_muted_text: [0x75, 0x75, 0x75],
    dialog_hover: [0x2c, 0x2c, 0x2c],
    menu_item_text: [0x9f, 0x9f, 0x9f],
    menu_item_text_selected: [0xe3, 0xe3, 0xe3],
    menu_item_hover: [0x36, 0x36, 0x36],
    // `--active` rgba(255,255,255,.09) over `--menu` #2A2A2A:
    // 42 + 213×.09 = 61.17.
    peek_leaf_focus_fill: [0x3d, 0x3d, 0x3d],
    // `--ink3` rgba(255,255,255,.38) over that: 61 + 194×.38 = 134.72.
    peek_leaf_focus_edge: [0x87, 0x87, 0x87],
    // `--ink` rgba(255,255,255,.87) over that: 61 + 194×.87 = 229.78.
    peek_leaf_focus_text: [0xe6, 0xe6, 0xe6],
    modal_scrim: [0x0f, 0x0f, 0x0f],
    modal_scrim_alpha: 89,
    // `--active` (white .09) over `--panel` #252525: 37 + 218×.09 = 56.6.
    tab_badge_on_resting_tab: [0x39, 0x39, 0x39],
    // `--ink` (white .87) over that badge's fill on the active tab (47.5).
    tab_badge_text_on_active_tab: [0xe4, 0xe4, 0xe4],
    // `--ink2` (white .55) over 56.6, and over the hovered tab's 67.5.
    tab_badge_text_on_resting_tab: [0xa6, 0xa6, 0xa6],
    tab_badge_text_on_hovered_tab: [0xab, 0xab, 0xab],
    // `--ink3` (white .38) over `--menu` #2A2A2A: 42 + 213×.38 = 122.9.
    menu_item_hint_text: [0x7b, 0x7b, 0x7b],
    // Two of the mock-up's status semantics live in `:root` and `body.dark`
    // overrides neither, so the dark canvas wears the same two literals.
    status_warn: [0xd9, 0x82, 0x2b],
    status_pause: [0xc1, 0x9c, 0x00],
    // The other two are overridden. `--ok` since mock-up 74: `#1a7f37` on
    // `#1B1B1B` is a green that reads as a smudge. `--err` since the rose ruling
    // (2026-08-16): rose-400, because rose-600 over this canvas is 2.8:1 — the
    // same smudge in the other hue.
    status_err: [0xfb, 0x71, 0x85],
    status_ok: [0x57, 0xab, 0x5a],
    // `--border` (white at .094) at `opacity: .7` — .0658 white — over
    // `--termbg` #1B1B1B, `--panel` #252525, and `--hover`-over-`--panel`
    // #313131 respectively.
    ring_track_on_active_tab: [0x2a, 0x2a, 0x2a],
    ring_track_on_resting_tab: [0x33, 0x33, 0x33],
    ring_track_on_hovered_tab: [0x3f, 0x3f, 0x3f],
    // `--active` (white .09) over `--panel` #252525: 37 + 218×.09 = 56.62.
    rail_tab_active: [0x39, 0x39, 0x39],
    // `--ink` (white .87) over that: 56.62 + 198.38×.87 = 229.21.
    rail_tab_active_text: [0xe5, 0xe5, 0xe5],
    // `--ink2` (white .55) over `--hover`-over-`--panel` 48.99: + 206.01×.55.
    rail_tab_hover_text: [0xa2, 0xa2, 0xa2],
    // `--ink3` (white .38) over the selection fill 56.62: + 198.38×.38 = 132.0.
    rail_glyph_on_active_tab: [0x84, 0x84, 0x84],
    // `--border` (white .094) over `--panel`: 37 + 218×.094 = 57.49.
    rail_seam: PANEL_HAIRLINE_DARK,
    // `--border-soft` (white .06) over `--panel`: 37 + 218×.06 = 50.08.
    rail_edge: [0x32, 0x32, 0x32],
    // The gradient is black on both canvases; only its alpha is theme-varied.
    rail_shade: [0x00, 0x00, 0x00],
    // `.34` of 255.
    rail_shade_alpha: 87,
    // The focus column's cards. Derived rather than transcribed — see
    // `ink_over`, and see the Git page's fifteen entries for why: a card wears
    // eighteen composites over four grounds, and eighteen hand-checked sums is
    // eighteen chances to be wrong in a way no test would catch.
    focus_card: FOCUS_CARD_DARK,
    focus_card_staged: FOCUS_CARD_STAGED_DARK,
    focus_card_title: ink_over(FOCUS_CARD_DARK, DARK_INK_SOURCE, 550),
    focus_card_title_staged: ink_over(FOCUS_CARD_STAGED_DARK, DARK_INK_SOURCE, 870),
    focus_card_glyph: ink_over(FOCUS_CARD_DARK, DARK_INK_SOURCE, 380),
    focus_card_glyph_staged: ink_over(FOCUS_CARD_STAGED_DARK, DARK_INK_SOURCE, 380),
    focus_card_pill: FOCUS_CARD_PILL_DARK,
    focus_card_pill_staged: FOCUS_CARD_PILL_STAGED_DARK,
    focus_card_ink_on_pill: ink_over(FOCUS_CARD_PILL_DARK, DARK_INK_SOURCE, 870),
    focus_card_ink_on_pill_staged: ink_over(FOCUS_CARD_PILL_STAGED_DARK, DARK_INK_SOURCE, 870),
    focus_card_muted_on_pill: ink_over(FOCUS_CARD_PILL_DARK, DARK_INK_SOURCE, 550),
    focus_card_edge: PANEL_HAIRLINE_DARK,
    // `--border` at `opacity: .7` — `.094 × .7 = .0658`, in ten-thousandths, the
    // same ladder `ring_track_on_active_tab` and its two siblings ride.
    ring_track_on_focus_card: ink_over_bp(FOCUS_CARD_DARK, DARK_SHADE_SOURCE, 658),
    ring_track_on_focus_card_staged: ink_over_bp(FOCUS_CARD_STAGED_DARK, DARK_SHADE_SOURCE, 658),
    focus_exit: WIN_DARK,
    focus_exit_text: ink_over(WIN_DARK, DARK_INK_SOURCE, 550),
    focus_exit_hover: FOCUS_EXIT_HOVER_DARK,
    focus_exit_text_hover: ink_over(FOCUS_EXIT_HOVER_DARK, DARK_INK_SOURCE, 870),
};

/// Chrome over a light canvas — the mock-up's `:root` defaults, composited the
/// same way: `--win #FFFFFF`, `--panel #F7F7F5`, `--ink #37352F` at
/// .65/.45 for the secondary steps, `--border` at .088 black,
/// `--accent #3059D8`.
pub const LIGHT_CHROME: ChromePalette = ChromePalette {
    seat_body: [0xff, 0xff, 0xff],
    title_bar: [0xf7, 0xf7, 0xf5],
    title_text: [0x7a, 0x79, 0x74],
    title_text_hover: [0x37, 0x35, 0x2f],
    title_text_muted: [0xa1, 0xa0, 0x9c],
    tab_close_pill_on_content: [0xed, 0xed, 0xec],
    tab_close_pill_on_hovered_tab: [0xdc, 0xdc, 0xd9],
    // `--ink3` (rgb(55,53,47) at .45) over `--termbg` #FFFFFF, which on this
    // canvas is also what `pane_title` mixes over.
    tab_close_glyph_on_active_tab: [0xa5, 0xa4, 0xa1],
    // The same ink over `--hover` over `--panel` #F7F7F5 — 236.4/236.3/234.1
    // under it, then 154.8/153.8/149.9.
    tab_close_glyph_on_hovered_tab: [0x9b, 0x9a, 0x96],
    // `--ink` #37352F is opaque in this theme, so the lit `×` is that literal
    // whichever pill it stands on. Two entries all the same: the pair has to
    // exist for dark, and spelling the light values as one shared constant
    // would hide the day a light `--ink` grows an alpha.
    tab_close_glyph_on_pill_over_active_tab: [0x37, 0x35, 0x2f],
    tab_close_glyph_on_pill_over_hovered_tab: [0x37, 0x35, 0x2f],
    // Opaque `--ink` again: a pinned pin is the same literal on every ground.
    tab_pin_state_on_active_tab: [0x37, 0x35, 0x2f],
    tab_pin_state_on_hovered_tab: [0x37, 0x35, 0x2f],
    body_hint_text: [0xa5, 0xa4, 0xa1],
    // `--ink rgba(55,53,47,.87)` is opaque enough to be itself in the mock-up's
    // own light palette, and `--termbg` here is `#FFFFFF`, so this is `--ink`.
    preview_body_text: [0x37, 0x35, 0x2f],
    // `--border-soft` (black .055) over `--termbg #FFFFFF`: 255×.945 = 241.0.
    preview_grid_line: [0xf1, 0xf1, 0xf1],
    // `--panel #F7F7F5`, and the same hairline over it:
    // (247, 247, 245)×.945 = (233.4, 233.4, 231.5).
    preview_code_ground: [0xf7, 0xf7, 0xf5],
    preview_code_border: [0xe9, 0xe9, 0xe8],
    // `--ink2` (.65) and `--ink3` (.45) of `rgb(55,53,47)` over that panel:
    // (122.2, 120.9, 116.3) and (160.6, 159.7, 155.9).
    preview_code_text: [0x7a, 0x79, 0x74],
    preview_code_lang: [0xa1, 0xa0, 0x9c],
    // `--ok #1a7f37` at 13% over white: (225.2, 238.4, 229.0).
    preview_diff_add: [0xe1, 0xee, 0xe5],
    // `--err #e11d48` at 10% over white: (252.0, 232.4, 236.7).
    preview_diff_del: [0xfc, 0xe8, 0xed],
    preview_diff_hunk: [0x30, 0x59, 0xd8],
    // `--ink #37352F` is opaque on this canvas, so the head row's ink is itself.
    preview_table_head_text: [0x37, 0x35, 0x2f],
    preview_selection: LIGHT_SELECTION_BACKGROUND_RGB,
    preview_caret: LIGHT_CURSOR_RGB,
    // The same seven hues, walked *down* from white until `--termbg #FFFFFF`
    // and a fence's `--panel #F7F7F5` both clear 4.5:1 — the mirror of the dark
    // block's rule, and the reason these are not the dark values darkened: a
    // hue that is quiet on ink is loud on paper at the same saturation.
    hl_keyword: [0x91, 0x53, 0xbe],
    hl_string: [0x20, 0x7e, 0x47],
    hl_comment: [0x64, 0x71, 0x84],
    hl_number: [0xa5, 0x5e, 0x18],
    hl_type: [0x1b, 0x78, 0x98],
    hl_function: [0x47, 0x6a, 0xd1],
    hl_punct_muted: [0x64, 0x71, 0x84],
    // The file tree's three grounds on `--termbg #FFFFFF`. The inks here are
    // rgb(55,53,47), so each channel steps down by {200,202,208}×alpha:
    //   `--hover`  .055 → (244.0, 243.9, 243.6)
    //   `--active` .09  → (237.0, 236.8, 236.3)
    files_row_hover: [0xf4, 0xf4, 0xf4],
    files_row_selected: [0xed, 0xed, 0xec],
    // `--ink2` (.65) over the bare body: 255 − {200,202,208}×.65 =
    // (125.0, 123.7, 119.8); over the hover fill: (121.2, 119.8, 115.8).
    // `--ink` is opaque, so on the selected row it is simply itself.
    files_row_text: [0x7d, 0x7c, 0x78],
    files_row_text_hover: [0x79, 0x78, 0x74],
    files_row_text_selected: [0x37, 0x35, 0x2f],
    // `--ink3` (.45) over the same three: (165.0, 164.1, 161.4),
    // (159.0, 158.0, 155.1), (155.1, 154.1, 151.1).
    files_row_muted: [0xa5, 0xa4, 0xa1],
    files_row_muted_hover: [0x9f, 0x9e, 0x9b],
    files_row_muted_selected: [0x9b, 0x9a, 0x97],
    // The same eight over `--win`, which on this canvas is the same `#FFFFFF`
    // `--termbg` is — so these are the eight above, value for value. Spelled out
    // rather than shared for the reason the two `×`-on-pill entries above are:
    // the pair has to exist for dark, and one constant standing for both would
    // hide the day the two grounds stop coinciding here too.
    float_row_hover: [0xf4, 0xf4, 0xf4],
    float_row_selected: [0xed, 0xed, 0xec],
    float_row_text: [0x7d, 0x7c, 0x78],
    float_row_text_hover: [0x79, 0x78, 0x74],
    float_row_text_selected: [0x37, 0x35, 0x2f],
    float_row_muted: [0xa5, 0xa4, 0xa1],
    float_row_muted_hover: [0x9f, 0x9e, 0x9b],
    float_row_muted_selected: [0x9b, 0x9a, 0x97],
    // `--ink` is opaque on this canvas, so the two inks that wear it land as
    // `#37352F` however many surfaces are under them — `ink_over(…, 1000)` says
    // so in the same grammar as its neighbours rather than by a bare literal
    // that would look like a different decision.
    git_section: PANEL_LIGHT,
    git_row_hover: GIT_ROW_HOVER_LIGHT,
    git_row_selected: [0xed, 0xed, 0xec],
    git_row_match: ink_over(TERMBG_LIGHT, [0xed, 0xed, 0xec], 500),
    git_row_text: ink_over(PANEL_LIGHT, LIGHT_INK_SOURCE, 1000),
    git_row_text_hover: ink_over(GIT_ROW_HOVER_LIGHT, LIGHT_INK_SOURCE, 1000),
    git_row_muted: ink_over(PANEL_LIGHT, LIGHT_INK_SOURCE, 450),
    git_row_muted_hover: ink_over(GIT_ROW_HOVER_LIGHT, LIGHT_INK_SOURCE, 450),
    git_act_glyph: ink_over(PANEL_LIGHT, LIGHT_INK_SOURCE, 650),
    git_act_glyph_hover: ink_over(GIT_ROW_HOVER_LIGHT, LIGHT_INK_SOURCE, 650),
    git_act_pill: GIT_ACT_PILL_LIGHT,
    git_act_glyph_on_pill: ink_over(GIT_ACT_PILL_LIGHT, LIGHT_INK_SOURCE, 1000),
    git_head_text: ink_over(TERMBG_LIGHT, LIGHT_INK_SOURCE, 1000),
    git_head_muted: ink_over(TERMBG_LIGHT, LIGHT_INK_SOURCE, 450),
    git_pill_text: ink_over(TERMBG_LIGHT, LIGHT_INK_SOURCE, 650),
    // `--border` on light is `rgba(0,0,0,.088)` — black, not the ink.
    git_pill_border: ink_over(TERMBG_LIGHT, [0x00, 0x00, 0x00], 88),
    graph_lanes: GRAPH_LANES_LIGHT,
    divider: [0xe9, 0xe9, 0xe9],
    divider_hover: [0xc2, 0xc1, 0xbf],
    divider_active: [0x30, 0x59, 0xd8],
    collapse_bar: [0xf7, 0xf7, 0xf5],
    collapse_bar_hover: [0xe9, 0xe9, 0xe8],
    caption_hover: [0xec, 0xec, 0xea],
    caption_close_hover: [0xe5, 0x48, 0x4d],
    caption_close_text: [0xff, 0xff, 0xff],
    active_tab: [0xff, 0xff, 0xff],
    pane_head: [0xff, 0xff, 0xff],
    // `--ink3` (rgb(55,53,47) at .45) over `--termbg` #FFFFFF.
    pane_close_glyph: [0xa5, 0xa4, 0xa1],
    // `--active` (the same ink at .09) over the same white.
    pane_close_pill: [0xed, 0xed, 0xec],
    // `--ink` #37352F is opaque on this canvas, so the lit `×` is that literal.
    pane_close_glyph_on_pill: [0x37, 0x35, 0x2f],
    termhost: [0xf7, 0xf7, 0xf5],
    pane_head_edge: [0xf1, 0xf1, 0xf1],
    pane_title: [0xa5, 0xa4, 0xa1],
    pane_title_focus: [0x37, 0x35, 0x2f],
    accent: [0x30, 0x59, 0xd8],
    // The same three on the light canvas: `--ink3` (black .45) over the light
    // `--termbg`, `#3059D8` at 86% over black, and `--err-deep` `#be123c`.
    command_tick: [0xa5, 0xa4, 0xa1],
    command_tick_crest: [0x29, 0x4d, 0xba],
    command_tick_fail_crest: [0xbe, 0x12, 0x3c],
    // `--ink2` (black .72) over the light `--termbg`.
    command_tick_search_crest: [0x7d, 0x7c, 0x78],
    // `--thumb` (`#37352F` at .24) and `--thumb-hover` (the same ink at .42)
    // over the light `--termbg`. The ink is warm, so the third channel parts
    // company with the other two — which is the whole reason this is a triple
    // and not a grey level.
    scroll_thumb: [0xcf, 0xcf, 0xcd],
    scroll_thumb_hover: [0xab, 0xaa, 0xa8],
    menu_surface: [0xff, 0xff, 0xff],
    menu_border: [0x00, 0x00, 0x00],
    menu_border_alpha: 22,
    menu_shadow: [0x00, 0x00, 0x00],
    menu_shadow_inner_alpha: 18,
    menu_shadow_outer_alpha: 9,
    menu_popup_shadow_inner_alpha: 46,
    menu_popup_shadow_outer_alpha: 23,
    // `.1` black — the base declaration at mock-up 1217.
    tip_shadow_inner_alpha: 26,
    tip_shadow_outer_alpha: 13,
    // The ghost's `.25` is written once for both themes, so daylight gets the
    // same pair the night does — see the field's own note.
    drag_ghost_shadow_inner_alpha: 64,
    drag_ghost_shadow_outer_alpha: 32,
    // `.20` and `.24` black — the base declarations at mock-up 676 and 702.
    float_shadow_inner_alpha: 51,
    float_shadow_outer_alpha: 26,
    float_pinned_shadow_inner_alpha: 61,
    float_pinned_shadow_outer_alpha: 31,
    // `.18` black — the glance card's base declaration at mock-up 1785.
    peek_card_shadow_inner_alpha: 46,
    peek_card_shadow_outer_alpha: 23,
    dialog_surface: [0xff, 0xff, 0xff],
    dialog_title_text: [0x37, 0x35, 0x2f],
    dialog_secondary_text: [0x7d, 0x7c, 0x78],
    dialog_muted_text: [0xa5, 0xa4, 0xa1],
    dialog_hover: [0xf4, 0xf4, 0xf4],
    menu_item_text: [0x7d, 0x7c, 0x78],
    menu_item_text_selected: [0x37, 0x35, 0x2f],
    menu_item_hover: [0xf4, 0xf4, 0xf4],
    // `--active` rgba(55,53,47,.09) over `--menu` #FFFFFF:
    // 255 − 200×.09 = 237, 255 − 202×.09 = 236.82, 255 − 208×.09 = 236.28.
    peek_leaf_focus_fill: [0xed, 0xed, 0xec],
    // `--ink3` rgba(55,53,47,.45) over that: 237 − 182×.45 = 155.1,
    // 237 − 184×.45 = 154.2, 236 − 189×.45 = 150.95.
    peek_leaf_focus_edge: [0x9b, 0x9a, 0x97],
    // `--ink` #37352F is opaque on light: it composites to itself.
    peek_leaf_focus_text: [0x37, 0x35, 0x2f],
    modal_scrim: [0x0f, 0x0f, 0x0f],
    modal_scrim_alpha: 89,
    // `--active` (rgb(55,53,47) at .09) over `--panel` #F7F7F5.
    tab_badge_on_resting_tab: [0xe6, 0xe6, 0xe3],
    // `--ink` #37352F is opaque in this theme, so it lands unchanged.
    tab_badge_text_on_active_tab: [0x37, 0x35, 0x2f],
    // `--ink2` (the same ink at .65) over the resting and hovered fills, each
    // taken as painted. The hovered one's blue was 0x6B until the derivation
    // pin of 2026-08-17: that level comes from compositing over the unrounded
    // 217.27 rather than over the `#DCDCD9` the pill is actually drawn in.
    tab_badge_text_on_resting_tab: [0x74, 0x73, 0x6e],
    tab_badge_text_on_hovered_tab: [0x71, 0x6f, 0x6a],
    // `--ink3` (the ink at .45) over `--menu` #FFFFFF — which in this theme is
    // the same white as `--win`, so it agrees with `dialog_muted_text` exactly.
    menu_item_hint_text: [0xa5, 0xa4, 0xa1],
    // Opaque in the mock-up's `:root`, and not overridden by either canvas.
    status_warn: [0xd9, 0x82, 0x2b],
    status_pause: [0xc1, 0x9c, 0x00],
    // `:root`'s own `--err` and `--ok`, each of which only the dark canvas
    // overrides. Rose-600 is 4.7:1 on white.
    status_err: [0xe1, 0x1d, 0x48],
    status_ok: [0x1a, 0x7f, 0x37],
    // `--border` (black at .088) at `opacity: .7` — .0616 black — over
    // `--termbg` #FFFFFF, `--panel` #F7F7F5, and `--hover`-over-`--panel`
    // #ECECEA respectively.
    ring_track_on_active_tab: [0xef, 0xef, 0xef],
    ring_track_on_resting_tab: [0xe8, 0xe8, 0xe6],
    ring_track_on_hovered_tab: [0xdd, 0xdd, 0xdc],
    // `--active` (rgb(55,53,47) at .09) over `--panel` #F7F7F5 — the same pair
    // `tab_badge_on_resting_tab` is struck from, and the same three bytes.
    rail_tab_active: [0xe6, 0xe6, 0xe3],
    // `--ink` #37352F is opaque on this canvas: it composites to itself.
    rail_tab_active_text: [0x37, 0x35, 0x2f],
    // `--ink2` (the ink at .65) over `--hover`-over-`--panel`, which is painted
    // as `#ECECEA` — the value `caption_hover` already carries. Its red was
    // 0x77 until the derivation pin of 2026-08-17, from the unrounded 236.4
    // landing at 118.50 and rounding away from zero where the painted 236
    // lands at 118.
    rail_tab_hover_text: [0x76, 0x75, 0x70],
    // `--ink3` (the ink at .45) over the selection fill 229.7/229.5/227.2.
    rail_glyph_on_active_tab: [0x97, 0x96, 0x92],
    // `--border` (black at .088) over `--panel`: 247×.912 = 225.26, 245×.912.
    rail_seam: PANEL_HAIRLINE_LIGHT,
    // `--border-soft` (black at .055) over `--panel`: 247×.945 = 233.42.
    rail_edge: [0xe9, 0xe9, 0xe8],
    // Black on both canvases; only the alpha differs.
    rail_shade: [0x00, 0x00, 0x00],
    // `.09` of 255.
    rail_shade_alpha: 23,
    // The focus column's cards, derived exactly as the dark set is.
    focus_card: FOCUS_CARD_LIGHT,
    focus_card_staged: FOCUS_CARD_STAGED_LIGHT,
    focus_card_title: ink_over(FOCUS_CARD_LIGHT, LIGHT_INK_SOURCE, 650),
    focus_card_title_staged: ink_over(FOCUS_CARD_STAGED_LIGHT, LIGHT_INK_SOURCE, 1000),
    focus_card_glyph: ink_over(FOCUS_CARD_LIGHT, LIGHT_INK_SOURCE, 450),
    focus_card_glyph_staged: ink_over(FOCUS_CARD_STAGED_LIGHT, LIGHT_INK_SOURCE, 450),
    focus_card_pill: FOCUS_CARD_PILL_LIGHT,
    focus_card_pill_staged: FOCUS_CARD_PILL_STAGED_LIGHT,
    focus_card_ink_on_pill: ink_over(FOCUS_CARD_PILL_LIGHT, LIGHT_INK_SOURCE, 1000),
    focus_card_ink_on_pill_staged: ink_over(FOCUS_CARD_PILL_STAGED_LIGHT, LIGHT_INK_SOURCE, 1000),
    focus_card_muted_on_pill: ink_over(FOCUS_CARD_PILL_LIGHT, LIGHT_INK_SOURCE, 650),
    focus_card_edge: PANEL_HAIRLINE_LIGHT,
    // `--border` at `opacity: .7` — `.088 × .7 = .0616`, in ten-thousandths.
    ring_track_on_focus_card: ink_over_bp(FOCUS_CARD_LIGHT, LIGHT_SHADE_SOURCE, 616),
    ring_track_on_focus_card_staged: ink_over_bp(FOCUS_CARD_STAGED_LIGHT, LIGHT_SHADE_SOURCE, 616),
    focus_exit: WIN_LIGHT,
    focus_exit_text: ink_over(WIN_LIGHT, LIGHT_INK_SOURCE, 650),
    focus_exit_hover: FOCUS_EXIT_HOVER_LIGHT,
    focus_exit_text_hover: ink_over(FOCUS_EXIT_HOVER_LIGHT, LIGHT_INK_SOURCE, 1000),
};

/// The palette in force, decided by the same background-luma threshold that
/// already chooses the terminal's default ink — one dark/light decision for the
/// whole product, never two.
pub fn chrome_palette() -> ChromePalette {
    chrome_palette_for_background(background_rgb())
}

fn chrome_palette_for_background(background: [u8; 3]) -> ChromePalette {
    let schemes = active_schemes();
    if background_is_light(background) {
        schemes.light_chrome
    } else {
        schemes.dark_chrome
    }
}

// ---------------------------------------------------------------------------
// The two schemes in force, and the chrome each one derives.
//
// Kept beside the theme's own atomic rather than inside it, because they are a
// different shape of state: the atomic exists so a render thread can read the
// background with one uncontended load, and twenty-one colours plus a
// hundred-and-thirty-nine derived ones do not fit in sixty-four bits. What ties
// the two together is `theme_revision`. A scheme change bumps it exactly as a
// dark/light switch does, so every artefact keyed on it — CPU math rasters,
// their GPU textures, the composed-row cache — is invalidated by one mechanism
// and not by a second list somebody has to remember to extend.
//
// The palettes are derived **once, here**, and not per call: `chrome_palette()`
// has 201 call sites and some of them are inside per-frame loops, so a
// derivation on the read path would run the whole ladder a few hundred times a
// frame to produce the same answer.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct ActiveSchemes {
    light: ColourScheme,
    dark: ColourScheme,
    light_chrome: ChromePalette,
    dark_chrome: ChromePalette,
}

impl ActiveSchemes {
    fn new(light: ColourScheme, dark: ColourScheme) -> Self {
        Self {
            light,
            dark,
            light_chrome: ChromePalette::derive(&light),
            dark_chrome: ChromePalette::derive(&dark),
        }
    }
}

fn process_schemes() -> &'static RwLock<ActiveSchemes> {
    static SCHEMES: OnceLock<RwLock<ActiveSchemes>> = OnceLock::new();
    SCHEMES.get_or_init(|| RwLock::new(ActiveSchemes::new(FOLIO_LIGHT, FOLIO_DARK)))
}

// A poisoned lock is read through rather than panicked on. Nothing in here can
// be left half-written — the write path replaces the whole struct in one move —
// so the worst a panicking writer can have done is fail before assigning, and
// the previous palette is a better answer than taking every window down.
fn active_schemes() -> ActiveSchemes {
    *process_schemes()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The scheme whose canvas this background belongs to.
///
/// Keyed on the background's own luma and not on [`current_theme`], for the
/// reason every `*_for_background` reader already gives: under a `BT_BG`
/// override the two disagree on purpose, and a default colour that followed the
/// theme would be the one thing on that screen still dressed for the other
/// canvas.
pub(crate) fn scheme_for_background(background: [u8; 3]) -> ColourScheme {
    let schemes = active_schemes();
    if background_is_light(background) {
        schemes.light
    } else {
        schemes.dark
    }
}

/// The scheme in force for a theme, whatever canvas is currently painted.
pub(crate) fn scheme_for_theme(theme: Theme) -> ColourScheme {
    let schemes = active_schemes();
    match theme {
        Theme::Dark => schemes.dark,
        Theme::Light => schemes.light,
    }
}

/// The scheme the canvas on screen is actually wearing.
///
/// The one reader outside this crate is the answer to a program's colour query
/// (`OSC 4/10/11/12;?`). That answer has to be the colours the glass is showing
/// and not the colours the settings file names, which is why this is keyed on
/// the painted background's own luma exactly as [`scheme_for_background`] is:
/// under a `BT_BG` override the two disagree on purpose, and a program told the
/// settings' colours would dress itself for a canvas nobody is looking at.
#[must_use]
pub fn scheme_in_force() -> ColourScheme {
    scheme_for_background(background_rgb())
}

/// The pair in force, as `(light, dark)`.
///
/// The one reader is the schemes folder's watcher (§7.1.6c-4c). When the file
/// behind the scheme a canvas is wearing stops parsing, the ruling is that the
/// colours on screen do not move — and "do not move" can only be said by handing
/// the same colours back to [`set_schemes`], because the other canvas may well
/// have changed in the same rescan and the pair goes in together.
#[must_use]
pub fn schemes_in_force() -> (ColourScheme, ColourScheme) {
    let schemes = active_schemes();
    (schemes.light, schemes.dark)
}

/// Put a pair of schemes in force and bump [`theme_revision`].
///
/// **One call for both**, because a window only ever wears one of them and the
/// other is a fact about what happens when the theme flips; setting them
/// separately would mean two revisions and one wasted repaint for a change
/// nobody can see yet. Returns [`ThemeChange::Unchanged`] when neither moved,
/// so a settings write that did not touch the schemes costs nothing.
///
/// A `BT_BG` override keeps its canvas — that is the whole of what the lock
/// means — but the chrome still re-derives, because the override was only ever
/// a claim about the terminal's background.
pub fn set_schemes(light: ColourScheme, dark: ColourScheme) -> ThemeChange {
    {
        let mut schemes = process_schemes()
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if schemes.light == light && schemes.dark == dark {
            return ThemeChange::Unchanged;
        }
        *schemes = ActiveSchemes::new(light, dark);
    }
    process_theme().refresh();
    ThemeChange::Changed
}

/// Advance [`theme_revision`] without changing a colour.
///
/// The door the window's ground comes through (`crate::ground`). A background
/// picture and a ground alpha are theme-authored appearance that no palette
/// describes, and they invalidate exactly the artefacts a palette change
/// invalidates — so they ride the one revision channel rather than growing a
/// second one nobody would remember to extend.
pub(crate) fn bump_theme_revision() {
    process_theme().refresh();
}

/// A seat title bar's height, in logical pixels (`.panehead { height: 30px }`).
///
/// Twenty-eight until the 2026-08-12 ruling raised it to thirty. The font did
/// not move with it: the head's contents are centred on the strip's own middle
/// (`.panehead { align-items: center }`), so the two extra rows are shared
/// between the caption's top and bottom padding and every box in the head —
/// mark, `×`, trigger, root button — re-centres by construction rather than by
/// a second number being edited to match.
pub const SEAT_TITLE_BAR_LOGICAL_PX: f32 = 30.0;
/// **The band down a terminal pane's right edge that belongs to its scroll bar,
/// and to nothing else** (user ruling 2026-08-16, inventory D-14).
///
/// # A lane declared before the instrument that runs in it
///
/// **The instrument arrived on 2026-08-18** (P2-9 slice 1,
/// `crates/bt-app/src/termscroll.rs`), and this section stays as written because
/// it is the record of why the lane could be reserved two days early. When it
/// was written there was no terminal scroll bar at all, and the constant existed
/// precisely because there was not. The mock-up's rail carries an accident report
/// in its own stylesheet (`design/ui-mockup.html` 1355-1357): *"inboard of the
/// scrollbar gutter (thin ≈ 8px): the rail and the thumb are different
/// instruments and may not share a lane (user report 2026-07-18 — ticks sat on
/// top of the thumb)"*. A rail that measured its inset from the pane's own edge
/// would be right today and wrong on the day the bar lands, and it would be
/// wrong in exactly the way that report describes. So the lane is reserved now,
/// by a number both the rail and the future bar are derived from, and neither
/// can be moved without the other.
///
/// # Why it is not [`BLOCK_SCROLL_THICKNESS_LOGICAL_PX`]'s eight
///
/// The preview's bar has no lane constant to share: `crates/bt-app/src/preview.rs`
/// declares how thick a bar is **drawn** (two logical pixels, an overlay rule on
/// the surface's own far edge) and how far it reaches for a **hand**, and neither
/// of those is a reserved band — an overlay bar deliberately takes no gutter out
/// of the document it rides over, because taking one would change the width the
/// document lays out at. A terminal's grid is different in kind: it is a
/// character lattice whose right-most column is a place text actually is, and a
/// nine-pixel tick that grows to twenty-seven under the pointer would cover it.
/// Eight is the mock-up's own `thin` gutter, which is what its `right: 11px`
/// (this, plus a three-pixel gap) was derived from.
pub const TERMINAL_SCROLL_LANE_LOGICAL_PX: f32 = 8.0;
/// The self-drawn window title bar (`--titleh`).
pub const WINDOW_TITLE_BAR_LOGICAL_PX: f32 = 40.0;
/// Every settings/min/max/close box in `.capbtn` (`width: 46px; height: 40px`).
pub const WINDOW_CAPTION_BUTTON_LOGICAL_PX: f32 = 46.0;
/// `.capbtn svg { width: 10px; height: 10px }` — minimise, maximise, close.
pub const WINDOW_CAPTION_GLYPH_LOGICAL_PX: f32 = 10.0;
/// `.capbtn.gear svg { width: 14px; height: 14px }` — the settings gear alone
/// is larger, because its silhouette is a ring of teeth rather than one stroke.
pub const WINDOW_CAPTION_GEAR_GLYPH_LOGICAL_PX: f32 = 14.0;
/// The active horizontal tab's height (`.tab { height: 34px }`).
pub const WINDOW_TAB_HEIGHT_LOGICAL_PX: f32 = 34.0;
/// `--tabr`, shared by the active tab's two top corners *and* by the two
/// outward skirt corners that join it to the content plane
/// (`.tab.active::before/::after`).
pub const WINDOW_TAB_RADIUS_LOGICAL_PX: f32 = 7.0;
/// One tab's CSS cap (`design/ui-mockup.html` line 208).
pub const WINDOW_TAB_MAX_WIDTH_LOGICAL_PX: f32 = 200.0;
/// One tab's CSS floor (`.tab { min-width: 46px }`, `design/ui-mockup.html`
/// line 208) — the point at which equal-share compression stops.
///
/// It is a floor and not a suggestion: the stylesheet's own comment at line 187
/// rules that *past* this width the strip scrolls rather than compressing
/// further, because the alternative is tabs spilling into the caption buttons.
pub const WINDOW_TAB_MIN_WIDTH_LOGICAL_PX: f32 = 46.0;
/// Equal spacing between horizontal tabs (`design/ui-mockup.html` line 183).
pub const WINDOW_TAB_GAP_BETWEEN_LOGICAL_PX: f32 = 1.0;
/// `.tab { padding: 0 6px 0 12px }` — the leading inset before the mark.
pub const WINDOW_TAB_PADDING_LEFT_LOGICAL_PX: f32 = 12.0;
/// `.tab { padding: 0 6px … }` — the trailing inset after the title.
pub const WINDOW_TAB_PADDING_RIGHT_LOGICAL_PX: f32 = 6.0;
/// `.tab { gap: 8px }` — between the mark and the title.
pub const WINDOW_TAB_GAP_LOGICAL_PX: f32 = 8.0;
/// `.tab { font-size: 13px }`.
pub const WINDOW_TAB_FONT_LOGICAL_PX: f32 = 13.0;
/// `.ticon`/`.pmark` inside a tab: a 15px square profile mark.
pub const WINDOW_TAB_MARK_LOGICAL_PX: f32 = 15.0;
// ── T2: the state channels that ride on a tab's mark slot ──
//
// The mock-up hangs three things off `.ticon-wrap` (line 238), a wrapper whose
// entire job is to be a positioning origin: the mark itself, an absolutely
// positioned dot at its corner, and an absolutely positioned progress ring.
// Because the two additions are *absolute*, neither one takes part in the tab's
// flex layout — adding or removing them moves nothing. That is a property the
// design depends on, and every constant below is chosen to preserve it.

/// `.unreaddot { width: 6px; height: 6px; border-radius: 50% }` (line 255).
///
/// `border-radius: 50%` on a square is a circle, so this doubles as the
/// diameter: the dot's radius is half of it, and it is drawn as a pill whose
/// round is its own half-width rather than as a separate circle primitive.
pub const WINDOW_TAB_STATUS_DOT_LOGICAL_PX: f32 = 6.0;
/// `.unreaddot { top: -2px; right: -4px }` — the dot's offsets from the mark
/// slot's own top-right corner, in the sign CSS uses: negative is *outward*.
///
/// The dot deliberately overhangs the slot on both axes. It is a badge on the
/// mark, not a thing beside it, and a badge that fits neatly inside its host
/// reads as part of the artwork.
pub const WINDOW_TAB_STATUS_DOT_TOP_LOGICAL_PX: f32 = -2.0;
pub const WINDOW_TAB_STATUS_DOT_RIGHT_LOGICAL_PX: f32 = -4.0;

/// `.pring circle { stroke-width: 2 }`, read as logical pixels.
///
/// The stroke is taken at its declared weight. It used to need an argument:
/// the mock-up drew a 25px box over a 20-unit `viewBox`, landing the stroke at
/// 2.5 physical units, and that scale existed only to clear the mark
/// underneath. The replacement ring was a deviation then; the mock-up has since
/// been written back to it (ruling 2026-08-08), so the two now agree and the
/// declared weight is simply the weight.
pub const WINDOW_TAB_RING_STROKE_LOGICAL_PX: f32 = 2.0;
/// The ring's path radius inside [`WINDOW_TAB_MARK_LOGICAL_PX`].
///
/// A stroke straddles its path, so the outer edge sits half a stroke beyond the
/// radius; for the ring to fill the slot exactly and clip nowhere, the radius
/// is the slot's half-width less half a stroke.
pub const WINDOW_TAB_RING_RADIUS_LOGICAL_PX: f32 =
    (WINDOW_TAB_MARK_LOGICAL_PX - WINDOW_TAB_RING_STROKE_LOGICAL_PX) / 2.0;
/// The indeterminate arc's length as a fraction of one turn.
///
/// Struck as `13` units of a `PRING_C = 53.4` circumference when the ring was
/// still the mock-up's r=8.5 halo, and kept as that exact ratio rather than as
/// a length: an absolute dash is a different arc on a different circle, and the
/// design means the arc. The mock-up now states the same fraction at the
/// replacement ring's own size — `stroke-dasharray: 9.94 30.9` against
/// `PRING_C = 40.84` for r=6.5 — which is this ratio to four decimals. The
/// original spelling is kept here because it is the exact one; the test below
/// asserts the two agree, so a future edit to either cannot drift alone.
pub const WINDOW_TAB_RING_INDETERMINATE_TURNS: f32 = 13.0 / 53.4;

/// `.ticon.working { animation: breathe 1.7s ease-in-out infinite }` (line 245).
pub const WINDOW_TAB_BREATHE_PERIOD_MS: u64 = 1_700;
/// `@keyframes breathe { 0%, 100% { opacity: 1 } 50% { opacity: .28 } }`.
pub const WINDOW_TAB_BREATHE_MIN_OPACITY: f32 = 0.28;
/// `@media (prefers-reduced-motion: reduce) { .ticon.working { opacity: .6 } }`
/// (lines 1925-1928) — with the animation off, "working" still has to be said,
/// so the breath collapses to one held value rather than to nothing.
pub const WINDOW_TAB_BREATHE_REDUCED_OPACITY: f32 = 0.6;
/// `.pring.indeterminate { animation: pring-spin 1.1s linear infinite }` (282).
pub const WINDOW_TAB_RING_SPIN_PERIOD_MS: u64 = 1_100;
/// `.pring .arc { transition: stroke-dashoffset .3s ease }` (line 279) — a
/// progress report jumps, and the arc that reports it must not.
pub const WINDOW_TAB_RING_SWEEP_TRANSITION_MS: u64 = 300;
/// `transition: width .16s ease, margin-left .16s ease` (line 341) — the pin's
/// zero-width expansion. One continuous layout change, not a fade-in on top of a
/// jump: the control widens *in* and the badge beside it slides aside.
pub const WINDOW_TAB_PIN_REVEAL_MS: u64 = 160;
/// `opacity .12s ease` from the same declaration — the ink arrives a touch
/// ahead of the box finishing, which is what keeps the expansion from reading as
/// an empty gap opening first.
pub const WINDOW_TAB_PIN_FADE_MS: u64 = 120;
/// `.ticon-wrap.dead .ticon { opacity: .35 }` (line 285).
pub const WINDOW_TAB_DEAD_MARK_OPACITY: f32 = 0.35;

/// The tab close affordance (`design/ui-mockup.html` lines 305-311).
pub const WINDOW_TAB_CLOSE_BOX_LOGICAL_PX: f32 = 17.0;
pub const WINDOW_TAB_CLOSE_GLYPH_LOGICAL_PX: f32 = 8.0;
/// `.tab .close { border-radius: 4px }` — the pill under the pointer.
pub const WINDOW_TAB_CLOSE_RADIUS_LOGICAL_PX: f32 = 4.0;
/// The pane-count badge (`.panecount`, `design/ui-mockup.html` lines 292-304):
/// a pill that states how many panes a tab holds.
///
/// `min-width: 15px; height: 15px; padding: 0 4px` — so it is a 15px square
/// until the number needs more, and then it grows by its own padding. It is
/// drawn only when the count is greater than one (`paneBadge`, line 4189), and
/// it takes no space at all otherwise: "how many panes this tab holds — only
/// shown once it holds more than one".
pub const WINDOW_TAB_BADGE_MIN_WIDTH_LOGICAL_PX: f32 = 15.0;
pub const WINDOW_TAB_BADGE_HEIGHT_LOGICAL_PX: f32 = 15.0;
pub const WINDOW_TAB_BADGE_PADDING_X_LOGICAL_PX: f32 = 4.0;
/// `.panecount { border-radius: 4px }`.
pub const WINDOW_TAB_BADGE_RADIUS_LOGICAL_PX: f32 = 4.0;
/// `.panecount { font-size: 10px }`.
pub const WINDOW_TAB_BADGE_FONT_LOGICAL_PX: f32 = 10.0;
/// The two width tiers a tab crosses as the strip fills, both of them *measured*
/// in the mock-up (`updateTabSqueeze`) rather than counted:
///
/// * below 140px the hover controls stand down so the title keeps its room, and
///   a tab that is not the active one drops its `×`;
/// * below 90px there is no legible room for words at all and the tab becomes
///   its centred mark.
pub const WINDOW_TAB_TIGHT_LOGICAL_PX: f32 = 140.0;
pub const WINDOW_TAB_SQUEEZED_LOGICAL_PX: f32 = 90.0;
/// `.tab.squeezed { padding: 0 4px }` — the only padding a squeezed tab keeps.
pub const WINDOW_TAB_SQUEEZED_PADDING_LOGICAL_PX: f32 = 4.0;
/// The new-tab button and its placement (`design/ui-mockup.html` lines 386-408).
pub const WINDOW_NEW_TAB_BOX_LOGICAL_PX: f32 = 28.0;
pub const WINDOW_NEW_TAB_GLYPH_LOGICAL_PX: f32 = 10.0;
/// `.newtab { border-radius: 6px }` — the round on the hover fill, and the whole
/// of what the button is at rest: `background: none` until the pointer arrives.
pub const WINDOW_NEW_TAB_RADIUS_LOGICAL_PX: f32 = 6.0;
pub const WINDOW_NEW_TAB_MARGIN_LEFT_LOGICAL_PX: f32 = 6.0;
pub const WINDOW_NEW_TAB_MARGIN_BOTTOM_LOGICAL_PX: f32 = 3.0;
/// `.chevbtn svg { width: 9px; height: 6px }` — the profile picker's arrow. It
/// wears the same 28px `.newtab` box as the `+` beside it (`.tabs-inline
/// .chevbtn { margin-left: 0 }`): two buttons of the same kind, side by side, at
/// two different widths reads as a mistake rather than as a hierarchy.
pub const WINDOW_NEW_TAB_CHEVRON_WIDTH_LOGICAL_PX: f32 = 9.0;
pub const WINDOW_NEW_TAB_CHEVRON_HEIGHT_LOGICAL_PX: f32 = 6.0;

// ── R1/R2: the vertical rail (`design/ui-mockup.html` 802-964) ──
//
// The rail is the same tab list on the other axis, so almost nothing here is a
// new *idea* — it is the horizontal strip's furniture at the sizes a 220px
// column can afford. Where a value is simply the strip's own, it is written as
// that constant rather than as a second copy of the number, for the reason
// `WINDOW_TAB_PIN_BOX_LOGICAL_PX` gives: two copies can drift apart while every
// test that only checked the number still passes.

/// `.rail { width: 220px }` — the rail fully open, and `--railw`'s open value.
pub const RAIL_WIDTH_LOGICAL_PX: f32 = 220.0;
/// `--railpark: 46px` — the icon rail's resting width, and the strip of space
/// the terminal keeps clear in icon mode whatever the rail is currently doing.
///
/// Numerically [`WINDOW_CAPTION_BUTTON_LOGICAL_PX`] and deliberately its own
/// constant: that the parked rail is exactly as wide as a caption button is a
/// coincidence of two unrelated designs, and tying them would make re-striking
/// either one silently move the other.
pub const RAIL_PARK_LOGICAL_PX: f32 = 46.0;
/// `.rail { padding: 6px 8px 10px }`.
pub const RAIL_PADDING_TOP_LOGICAL_PX: f32 = 6.0;
pub const RAIL_PADDING_X_LOGICAL_PX: f32 = 8.0;
pub const RAIL_PADDING_BOTTOM_LOGICAL_PX: f32 = 10.0;
/// `.rail { gap: 1px }` — between rows. The strip's own between-tab gap is the
/// same 1px, so this is written as that constant.
pub const RAIL_GAP_LOGICAL_PX: f32 = WINDOW_TAB_GAP_BETWEEN_LOGICAL_PX;
/// `.vtab { height: 30px; flex: none }`.
///
/// `flex: none` is the whole of Q172 and the mock-up's own user report: rows
/// never compress, the LIST scrolls. A rail of thirty tabs is a scroller, not
/// thirty 10px slivers.
pub const RAIL_TAB_HEIGHT_LOGICAL_PX: f32 = 30.0;
/// `.vtab { padding: 0 5px 0 10px }`.
pub const RAIL_TAB_PADDING_LEFT_LOGICAL_PX: f32 = 10.0;
pub const RAIL_TAB_PADDING_RIGHT_LOGICAL_PX: f32 = 5.0;
/// `.vtab { border-radius: 6px }`.
pub const RAIL_TAB_RADIUS_LOGICAL_PX: f32 = 6.0;
/// `.vtab { font-size: 13px }` — the strip's own tab font.
pub const RAIL_TAB_FONT_LOGICAL_PX: f32 = WINDOW_TAB_FONT_LOGICAL_PX;
/// `.vtab { gap: 8px }` — between the mark, the title and the trailing cluster.
pub const RAIL_TAB_GAP_LOGICAL_PX: f32 = WINDOW_TAB_GAP_LOGICAL_PX;
/// The parked rail's horizontal padding on a row (`.window.rail-icons .rail
/// .vtab { padding-left: 7.5px; padding-right: 7.5px }`).
///
/// The mock-up's comment is the specification and the reason: the rail's own 8px
/// plus this 7.5px puts a 15px mark's centre at exactly 46/2 = 23. **The same
/// padding holds when open**, so the icon column does not move by one pixel as
/// the panel slides — which is the entire promise of Q180.
pub const RAIL_TAB_PARKED_PADDING_X_LOGICAL_PX: f32 = 7.5;
/// `.rail { border-right: 1px solid … }`.
///
/// The rail is `box-sizing: border-box` like everything else in the mock-up, so
/// this pixel comes out of [`RAIL_WIDTH_LOGICAL_PX`] rather than being added to
/// it: a 220px rail has a 203px content run, not a 204px one. Measured in the
/// mock-up itself (220 − 8 − 8 − 1).
pub const RAIL_BORDER_LOGICAL_PX: f32 = 1.0;
/// `.rail .label { font-size: 11px }` — the "Tabs" heading.
pub const RAIL_LABEL_FONT_LOGICAL_PX: f32 = 11.0;
/// The "Tabs" heading's line box, which its stylesheet leaves at `line-height:
/// normal` and therefore does not state as a number.
///
/// 13px, measured off the mock-up rather than guessed: the label's border box
/// comes out 23px tall against `padding: 4px 10px 6px`, and 23 − 4 − 6 = 13.
/// A ratio would have been the wrong thing to store — `normal` is resolved from
/// the font's own ascent/descent/line-gap, so it is a measurement, not a rule.
pub const RAIL_LABEL_LINE_LOGICAL_PX: f32 = 13.0;
/// `.rail .label { letter-spacing: .04em }`, as a fraction of the font size.
pub const RAIL_LABEL_TRACKING_EM: f32 = 0.04;
/// `.rail .label { padding: 4px 10px 6px }`.
pub const RAIL_LABEL_PADDING_TOP_LOGICAL_PX: f32 = 4.0;
pub const RAIL_LABEL_PADDING_X_LOGICAL_PX: f32 = 10.0;
pub const RAIL_LABEL_PADDING_BOTTOM_LOGICAL_PX: f32 = 6.0;
/// `.rail-new { gap: 2px; margin-top: 2px }`.
pub const RAIL_NEW_GAP_LOGICAL_PX: f32 = 2.0;
pub const RAIL_NEW_MARGIN_TOP_LOGICAL_PX: f32 = 2.0;
/// `.rail .rail-new { position: sticky; bottom: -10px; padding-bottom: 4px;
/// margin-bottom: -4px }` (mock-up 818-821) — how far the stuck `+` row's
/// *visible* top sits above the rail's own bottom edge, over and above its
/// [`RAIL_TAB_HEIGHT_LOGICAL_PX`].
///
/// The 4px is padding the row wears only so that its own `--panel` fill reaches
/// the rail's edge while the list scrolls underneath it; the negative margin of
/// the same size gives the space straight back, so the padding costs the flow
/// nothing. What it does change is where `position: sticky` parks the row: the
/// element's *border box* bottom is what the sticky constraint pins, so the
/// 30px the eye sees begins 34px above the edge rather than 30.
///
/// `bottom: -10px` is the other half, and it is why the constraint is measured
/// from the rail's outer bottom rather than from the inside of its
/// [`RAIL_PADDING_BOTTOM_LOGICAL_PX`]: the negative offset buys the stuck row
/// permission to sit inside the rail's own 10px foot padding. Without it the
/// row would park 10px higher and leave a band of bare panel below itself that
/// the list scrolls through — a gap that only exists while you are scrolling,
/// which is the worst kind.
pub const RAIL_NEW_STICKY_PADDING_BOTTOM_LOGICAL_PX: f32 = 4.0;
/// `.rail-new .nt-main { height: 30px; padding: 0 10px }` — the `+` row, which
/// is a tab-shaped row and therefore a tab's height.
pub const RAIL_NEW_MAIN_PADDING_X_LOGICAL_PX: f32 = 10.0;
/// `.rail-new .nt-chev { width: 28px; height: 30px }` — the profile picker.
pub const RAIL_NEW_CHEVRON_BOX_LOGICAL_PX: f32 = 28.0;
/// `.pin-seam { height: 1px }` — the rule between the pinned run and the rest.
pub const RAIL_SEAM_THICKNESS_LOGICAL_PX: f32 = 1.0;
/// `.pin-seam { width: calc(100% - 12px); margin: 5px 6px }` — 6px in from each
/// side of the rail's content box.
///
/// Inset rather than full-bleed, and the mock-up's comment rules why: "a rule
/// that reaches both walls reads as 'a new section starts here', and this is a
/// boundary inside one list."
pub const RAIL_SEAM_INSET_X_LOGICAL_PX: f32 = 6.0;
/// `.pin-seam { margin: 5px 6px }` — the clear space above and below the rule.
pub const RAIL_SEAM_MARGIN_Y_LOGICAL_PX: f32 = 5.0;
/// The shade the open rail casts on the terminal
/// (`.window.rail-icons .termhost::before { width: 14px }`).
///
/// A one-sided gradient and **not** a box-shadow, which is a ruling with a
/// stated failure behind it: a box-shadow spreads in every direction, so an 18px
/// blur on a panel whose top edge *is* the title bar's bottom edge paints ~9px
/// of grey across that join — "the panel drew its own seam against the very
/// thing it was supposed to be continuous with".
pub const RAIL_SHADE_WIDTH_LOGICAL_PX: f32 = 14.0;
/// The rail's open/close transition (`width .18s ease, padding .18s ease,
/// opacity .18s ease`, and the shade's `left .18s ease`) — P168.
pub const RAIL_TRANSITION_MS: u64 = 180;
/// The label/title/badge/`×` fade in icon mode (`opacity .1s ease`) — Q183.
///
/// The text fades rather than being removed, so the layout is identical in both
/// states and the icons never jump; the rail's own overflow does the clipping
/// while the width animates.
pub const RAIL_TEXT_FADE_MS: u64 = 100;
/// `transition-delay: .06s` on the way *open* only — the panel gets a moment to
/// be wide enough to hold words before the words arrive.
pub const RAIL_TEXT_FADE_OPEN_DELAY_MS: u64 = 60;

// ── §7.1.6b′: the focus column, one card per tab ──
//
// The card column lives in the rail, at the rail's own width, and it is the rail
// that is scrolled and padded — `#focus-rail` contributes a gap and nothing
// else. So almost every measurement here is the rail's, written as the rail's
// constant, and what is genuinely new is the card's own chrome: a border, a
// radius, a head's padding, and the bar that carries door 5.
//
// **F1 ships the head alone.** The mock-up draws a body under it — the whole
// tab's split tree in miniature, tagged `F2` on its own face — and that is the
// next slice's projection budget, not this one's. A card is therefore exactly
// its head plus its border, and [`FOCUS_CARD_HEAD_*`] is what decides how tall
// one is.

/// `#focus-rail { gap: 8px }` — between cards.
///
/// Not [`RAIL_GAP_LOGICAL_PX`]'s 1px, and the difference is the point: rail rows
/// are a list and read as one run, while cards are separate objects and have to
/// be seen to end.
pub const FOCUS_CARD_GAP_LOGICAL_PX: f32 = 8.0;
/// `.fcard { border: 1px solid var(--border) }`.
///
/// Inside the card's box like every other border in the mock-up
/// (`box-sizing: border-box`), so it comes out of the head's height rather than
/// being added to it.
pub const FOCUS_CARD_BORDER_LOGICAL_PX: f32 = 1.0;
/// `.fcard { border-radius: 10px }`.
pub const FOCUS_CARD_RADIUS_LOGICAL_PX: f32 = 10.0;
/// `.fc-head { padding: 5px 8px }`.
pub const FOCUS_CARD_HEAD_PADDING_X_LOGICAL_PX: f32 = 8.0;
pub const FOCUS_CARD_HEAD_PADDING_Y_LOGICAL_PX: f32 = 5.0;
/// `.fc-head { gap: 6px }` — between the mark, the name and the trailing run.
pub const FOCUS_CARD_HEAD_GAP_LOGICAL_PX: f32 = 6.0;
/// `.fc-head { font-size: 11px }` — a card's name.
///
/// Two steps under the strip's 13px tab title, because a card says the same
/// thing in a narrower column and the mock-up gives the whole card this size.
pub const FOCUS_CARD_FONT_LOGICAL_PX: f32 = 11.0;
/// `.fc-head .fc-close { width: 16px; height: 16px }`.
///
/// Its own 16 and not [`WINDOW_TAB_CLOSE_BOX_LOGICAL_PX`]'s 17: the mock-up
/// writes a different number for the card's `×`, and it is the tallest thing in
/// the head, so it is what sets the card's height.
pub const FOCUS_CARD_CLOSE_BOX_LOGICAL_PX: f32 = 16.0;
/// `.fc-head .fc-close { border-radius: 4px }` — the pill under the hovered `×`.
pub const FOCUS_CARD_CLOSE_RADIUS_LOGICAL_PX: f32 = 4.0;
/// `.fc-head .pinsvg { width: 11px; height: 11px }` — the pin mark a pinned
/// tab's card wears.
///
/// A **mark and not a button**: F1's card states that its tab is pinned and
/// offers no pinning, because the offer is a hover-revealed control on a tab row
/// and the card has no second idiom for it. See §7.1.6b′ ④.
pub const FOCUS_CARD_PIN_BOX_LOGICAL_PX: f32 = 11.0;
/// `.fc-head .fc-close svg { width: 8px; height: 8px }` — the `×` inside its
/// box, the same glyph the strip's own `×` is drawn at.
pub const FOCUS_CARD_CLOSE_GLYPH_LOGICAL_PX: f32 = WINDOW_TAB_CLOSE_GLYPH_LOGICAL_PX;
/// `.focus-bar { padding: 4px 2px 6px }` — the bar that stands where the rail's
/// "Tabs" heading stands the rest of the time.
pub const FOCUS_BAR_PADDING_TOP_LOGICAL_PX: f32 = 4.0;
pub const FOCUS_BAR_PADDING_X_LOGICAL_PX: f32 = 2.0;
pub const FOCUS_BAR_PADDING_BOTTOM_LOGICAL_PX: f32 = 6.0;
/// `.focus-bar { gap: 6px }` — between the label and the way out.
pub const FOCUS_BAR_GAP_LOGICAL_PX: f32 = 6.0;
/// `.focus-exit { height: 22px }` — door 5.
pub const FOCUS_EXIT_HEIGHT_LOGICAL_PX: f32 = 22.0;
/// `.focus-exit { padding: 0 8px }`.
pub const FOCUS_EXIT_PADDING_X_LOGICAL_PX: f32 = 8.0;
/// `.focus-exit { border-radius: 6px }`.
pub const FOCUS_EXIT_RADIUS_LOGICAL_PX: f32 = 6.0;
/// `.focus-exit { gap: 5px }` — between the glyph and the word.
pub const FOCUS_EXIT_GAP_LOGICAL_PX: f32 = 5.0;
/// `.focus-exit svg { width: 10px; height: 10px }`.
pub const FOCUS_EXIT_GLYPH_LOGICAL_PX: f32 = 10.0;
/// `.focus-exit { font-size: 11px }` — the word `Exit`.
pub const FOCUS_EXIT_FONT_LOGICAL_PX: f32 = 11.0;

/// A seat title's font size (`.panehead { font-size: 11.5px }`).
pub const SEAT_TITLE_FONT_LOGICAL_PX: f32 = 11.5;
/// The inset between a title bar's edge and its first item
/// (`.panehead { padding: 0 6px 0 12px }`).
pub const SEAT_TITLE_PADDING_LOGICAL_PX: f32 = 12.0;
/// `.panehead { gap: 7px }` — between the mark and the title.
pub const SEAT_TITLE_GAP_LOGICAL_PX: f32 = 7.0;
/// The other half of `.panehead { padding: 0 6px 0 12px }`: the inset the
/// trailing control run stops at.
///
/// Its own constant rather than a second reading of
/// [`SEAT_TITLE_PADDING_LOGICAL_PX`], because the two are different numbers in
/// the one declaration they come from — the head is padded 12px on the side that
/// holds words and 6px on the side that holds buttons, since a 17px button box
/// already carries its own visual margin inside it.
pub const SEAT_TITLE_TRAILING_PADDING_LOGICAL_PX: f32 = 6.0;
/// `.panehead .pane-close { width: 17px; height: 17px }` (mock-up 1650-1654).
///
/// The same 17 as `.tab .close`, and deliberately spelled again rather than
/// aliased: they are two declarations in two stylesheets' worth of rules, and a
/// pane head could be re-struck without the tab strip moving.
pub const SEAT_PANE_CLOSE_BOX_LOGICAL_PX: f32 = 17.0;
/// `.panehead .pane-close { border-radius: 4px }`.
pub const SEAT_PANE_CLOSE_RADIUS_LOGICAL_PX: f32 = 4.0;
/// `.panehead .pane-close svg { width: 8px; height: 8px }`.
pub const SEAT_PANE_CLOSE_GLYPH_LOGICAL_PX: f32 = 8.0;
/// A terminal pane head wears the session's profile mark, at `.pmark`'s 15px.
pub const PANE_HEAD_PROFILE_MARK_LOGICAL_PX: f32 = 15.0;
/// `.preview-head .files-ico { width: 14px; height: 14px; color: var(--accent) }`.
pub const PANE_HEAD_FILE_MARK_LOGICAL_PX: f32 = 14.0;
/// `.files-head .files-ico { width: 13px; height: 13px; color: var(--accent) }`.
pub const PANE_HEAD_FOLDER_MARK_LOGICAL_PX: f32 = 13.0;
/// The hairline under a title bar, in logical pixels.
pub const SEAT_TITLE_EDGE_LOGICAL_PX: f32 = 1.0;
/// A divider's drawn width, in logical pixels. `DIVIDER` in `bt-layout` is the
/// space it *occupies*; this is what it *looks like*, and the two are allowed to
/// differ only because the visual one may snap to the physical grid for
/// sharpness (§2.5).
pub const SEAT_DIVIDER_VISUAL_LOGICAL_PX: f32 = 1.0;
/// A divider's hit zone, in logical pixels — wider than its line, because a
/// one-pixel target is not a target.
///
/// Seven, which is `.split-row > .divider { width: 7px; margin: 0 -3px }`
/// (mock-up 1475-1476) and `DESIGN.md` §7.1.1 both, read literally. The negative
/// margin is why seven costs nothing: the box overhangs its two neighbours by
/// 3px each and still spends only [`SEAT_DIVIDER_VISUAL_LOGICAL_PX`] of layout.
/// It carried 6 until this pass, which was a number no document ever said.
pub const SEAT_DIVIDER_HIT_LOGICAL_PX: f32 = 7.0;
/// The grip on a divider — `.divider::after`, mock-up 1485-1491. Three logical
/// pixels across the band and twenty-eight along it, so it reads as a handle on
/// the line rather than as a second line.
pub const SEAT_DIVIDER_GRIP_THICKNESS_LOGICAL_PX: f32 = 3.0;
/// `height: 28px` on a row divider, `width: 28px` on a column one.
pub const SEAT_DIVIDER_GRIP_LENGTH_LOGICAL_PX: f32 = 28.0;
/// `border-radius: 2px`.
pub const SEAT_DIVIDER_GRIP_RADIUS_LOGICAL_PX: f32 = 2.0;
/// `.slot.resizing .pane { margin: 5px }` (mock-up 1465-1469) — how far the two
/// panes a divider drag is resizing pull in from their own edges.
pub const SEAT_RESIZING_CARD_MARGIN_LOGICAL_PX: f32 = 5.0;
/// `.slot.resizing .pane { border-radius: 8px }`.
pub const SEAT_RESIZING_CARD_RADIUS_LOGICAL_PX: f32 = 8.0;
/// A floating window's corner radius (`.float-win`, `#files-flyout`,
/// `.term-menu`: `border-radius: 10px`). Shared by every window that floats over
/// content, the hover-peek flyout included — they are one shape in the mock-up
/// (§7.0: 小窗是一个形态), so they are one number here.
pub const FLOAT_WINDOW_RADIUS_LOGICAL_PX: f32 = 10.0;
/// Its hairline (`border: 1px solid var(--border)`).
pub const FLOAT_WINDOW_BORDER_LOGICAL_PX: f32 = 1.0;
/// How far a floating window's shadow reaches past its own edge.
///
/// The mock-up's `--shadow` is a pair of blurred, downward-offset drop shadows
/// (`0 16px 48px`, `0 2px 8px`). This pipeline draws opaque-or-blended quads and
/// has no blur, so what it can honestly offer is two concentric rings — the
/// outer one this far out, the inner one half as far and about twice as strong —
/// whose alphas compose into a short falloff. It is a *lift*, not the mock-up's
/// shadow: no gaussian tail, and no downward offset, so it sits symmetrically
/// around the box instead of pooling below it.
pub const FLOAT_WINDOW_SHADOW_LOGICAL_PX: f32 = 3.0;
/// `#files-flyout { width: 264px }` — what a float opens at, in both modes.
pub const FLOAT_WINDOW_WIDTH_LOGICAL_PX: f32 = 264.0;
/// `max-height: min(62vh, 460px)` — the taller of the two caps on a float that
/// is sizing itself to its content.
///
/// The mock-up hangs this cap on `#files-flyout` and takes it off again under
/// `.pinned` (`max-height: none`, line 702). That reads as "a pinned window has
/// no height limit", and it is not: what `max-height` governs in a browser is
/// *automatic* sizing, and a pinned window's height stops being automatic the
/// moment it is given one by the grip. So the cap applies to every float that
/// is asked to size itself — peek and pinned alike, since a click-to-pin opens
/// at the same default size a peek does — and the grip is what is not bound by
/// it. Reading `max-height: none` as "a fresh pinned window may open as tall as
/// its content" would let one directory of a thousand entries open a window
/// taller than the screen, with its own header off the top edge.
pub const FLOAT_WINDOW_MAX_HEIGHT_LOGICAL_PX: f32 = 460.0;
/// The other cap: `62vh` of the viewport a float is allowed to occupy.
pub const FLOAT_WINDOW_MAX_HEIGHT_VIEWPORT_FRACTION: f32 = 0.62;
/// `.float-win .fly-head { height: 30px }`.
pub const FLOAT_WINDOW_HEAD_LOGICAL_PX: f32 = 30.0;
/// `.float-win .fly-foot { height: 30px }`.
pub const FLOAT_WINDOW_FOOT_LOGICAL_PX: f32 = 30.0;
/// `.float-win.pinned { min-width: 200px }` — the grip's own floor.
pub const FLOAT_WINDOW_MIN_WIDTH_LOGICAL_PX: f32 = 200.0;
/// `.float-win.pinned { min-height: 150px }`.
pub const FLOAT_WINDOW_MIN_HEIGHT_LOGICAL_PX: f32 = 150.0;
/// **The honest floor** a squeezed pinned float stops at —
/// `M2-tiny-window-priority.md` §3.4's `PINNED_FLOAT_MIN_STRIP`, pinned to a
/// number here as that document says it would be ("具体像素值实现时钉").
///
/// It is [`FLOAT_WINDOW_HEAD_LOGICAL_PX`] and cannot sensibly be anything else:
/// the ruling is that the window shrinks to "只剩浮窗自身标题条(含 ×/拖拽手柄)
/// 那一条高度" and never to nothing, precisely so the two things that can undo
/// the squeeze — the `×` and the drag handle — are still there to be used. A
/// floor below the header would take away the header, which is the only reason
/// there is a floor.
///
/// Written as its own name rather than used inline because it means something
/// the header's height does not: §7.1.2 says a pinned float is closed by
/// `×`/Esc/Dock/re-click and by nothing else, and this constant is where that
/// promise is kept against geometry. The day the header changes height, this
/// follows it — but the day someone wants "collapse it to zero when it does not
/// fit", they have to come here and argue with the doc comment.
pub const FLOAT_WINDOW_MIN_STRIP_LOGICAL_PX: f32 = FLOAT_WINDOW_HEAD_LOGICAL_PX;
/// `.float-win .fly-resize { width: 16px; height: 16px }` — the corner grip.
pub const FLOAT_WINDOW_GRIP_LOGICAL_PX: f32 = 16.0;
/// How far a float opens below the trigger that summoned it (§7.1.2「触发器→
/// 浮层间距 6px」).
pub const FLOAT_WINDOW_TRIGGER_GAP_LOGICAL_PX: f32 = 6.0;
/// How close to the viewport's edge a float may be placed (§7.1.2「视口安全边距
/// 8px」).
pub const FLOAT_WINDOW_VIEWPORT_MARGIN_LOGICAL_PX: f32 = 8.0;
/// How far a *dragged* pinned float may be pushed against the viewport's edge.
///
/// Six rather than the eight above, and the mock-up means the difference: the
/// eight is where the app *places* a window you did not position, and this is
/// how far your own hand is allowed to push one (`Math.max(6, …)`, mock-up
/// 8665-8666). A margin you chose to close is not the same as a margin the app
/// chose for you.
pub const FLOAT_WINDOW_DRAG_MARGIN_LOGICAL_PX: f32 = 6.0;
/// `@keyframes flyIn/flyOut` — `.12s` in and the same back out (§7.1.2「进出
/// 动画 120ms」).
pub const FLOAT_WINDOW_ANIMATION_MS: u64 = 120;
/// `transform: translateY(-5px)` — how far a float rises into place, and falls
/// back out of it.
pub const FLOAT_WINDOW_RISE_LOGICAL_PX: f32 = 5.0;

/// `.drag-ghost { border-radius: 7px }` (mock-up 1719) — the label that rides
/// the pointer for the length of a drag.
///
/// Its own number rather than [`FLOAT_WINDOW_RADIUS_LOGICAL_PX`]'s 10, and the
/// mock-up means it: every other floating surface is a *window* over the page,
/// and this one is a thing in your hand. It is the only surface in the design
/// that moves with the pointer, and it is rounded less because it is smaller
/// than all of them — a 10px radius on a 26px-tall box is most of its height.
pub const DRAG_GHOST_RADIUS_LOGICAL_PX: f32 = 7.0;
/// `.drag-ghost { border: 1px solid var(--border) }`.
pub const DRAG_GHOST_BORDER_LOGICAL_PX: f32 = 1.0;
/// `.drag-ghost { padding: 5px 12px }` — the horizontal half.
pub const DRAG_GHOST_PADDING_X_LOGICAL_PX: f32 = 12.0;
/// `.drag-ghost { padding: 5px 12px }` — the vertical half.
pub const DRAG_GHOST_PADDING_Y_LOGICAL_PX: f32 = 5.0;
/// `.drag-ghost { gap: 7px }` — between the mark and the name.
pub const DRAG_GHOST_GAP_LOGICAL_PX: f32 = 7.0;
/// `.drag-ghost { font-size: 12.5px }`.
pub const DRAG_GHOST_FONT_LOGICAL_PX: f32 = 12.5;
/// How far below and to the right of the pointer the ghost hangs — `g.style.left
/// = clientX + 10`, `g.style.top = clientY + 8` (mock-up 6765-6766).
///
/// Down-and-right of the hotspot rather than centred on it, so the label never
/// covers the thing the pointer is aiming at. It is not clamped to the window:
/// the mock-up's is `position: fixed` with no bound, and a ghost that stopped at
/// the edge would be reporting a pointer position that is not where the pointer
/// is.
pub const DRAG_GHOST_POINTER_OFFSET_LOGICAL_PX: [f32; 2] = [10.0, 8.0];

/// `#dock-preview { border-radius: 8px }` (mock-up 1661) — the box that says
/// where the thing in your hand is about to be.
pub const DOCK_PREVIEW_RADIUS_LOGICAL_PX: f32 = 8.0;
/// `#dock-preview { border: 1.5px solid var(--accent) }`, and the same 1.5 the
/// refused and displaced outlines are drawn at.
///
/// One constant for all three because the mock-up writes 1.5 three times: the
/// arriving box, the dashed refusal and the dashed destinations are cells of one
/// drawing (M154), and a stroke that differed between them would be reporting a
/// difference the drawing does not mean.
pub const DOCK_PREVIEW_BORDER_LOGICAL_PX: f32 = 1.5;
/// `background: color-mix(in srgb, var(--accent) 13%, transparent)`, in 1/255ths.
///
/// Not a palette field, because it is not a composite: `--accent` is opaque on
/// both themes and the mix is against *transparent*, so the honest expression is
/// the accent blended at draw time over whatever the pane happens to be showing —
/// which is the same argument [`ChromePalette::menu_border`] makes for itself.
pub const DOCK_PREVIEW_FILL_ALPHA: u8 = 33;
/// `#dock-preview { font-size: 12.5px }` — L137's word inside the box.
pub const DOCK_PREVIEW_FONT_LOGICAL_PX: f32 = 12.5;
/// `#dock-preview { letter-spacing: .04em }`.
pub const DOCK_PREVIEW_LETTER_SPACING_EM: f32 = 0.04;
/// `#dock-shift i { border-radius: 6px }` — a destination outline.
///
/// Smaller than the arriving box's 8, and the mock-up means the difference: the
/// filled box is the thing that lands and these are places that are still empty.
pub const DOCK_SHIFT_RADIUS_LOGICAL_PX: f32 = 6.0;
/// `border: 1.5px dashed color-mix(in srgb, var(--accent) 85%, transparent)`.
pub const DOCK_SHIFT_BORDER_ALPHA: u8 = 217;
/// `background: color-mix(in srgb, var(--accent) 5%, transparent)`.
///
/// Five percent and not the eleven it started at, because the dashes carry this
/// drawing and the fill has to get out of their way: a wash at 11% behind the
/// line put the two at nearly the same value and the line lost — and the line is
/// the only part that says "outline, not surface" (M151).
pub const DOCK_SHIFT_FILL_ALPHA: u8 = 13;
/// `SHIFT_INSET = 1` (mock-up 6479), and it is worn by the arriving box as well
/// as by the destinations.
///
/// **M154 — the inset earns its pixel, and it must be the same one everywhere.**
/// A split halves one axis and the divider seam halves with it, so on that axis a
/// 1px gap becomes 0.5px and two neighbouring dashed borders fuse into one
/// muddled stripe while the untouched axis keeps a clean 1px — which is exactly
/// the "one direction looks joined and the other looks spaced" the drawing shows
/// without it. Inset only the dashed ones and the seam beside the *arriving* pane
/// comes out a pixel narrower than the seams between the others, which is visible
/// the moment three cells sit in a column.
pub const DOCK_SHIFT_INSET_LOGICAL_PX: f32 = 1.0;
/// How long a dash and the gap after it are, as a multiple of the stroke width.
///
/// **A ruling, because CSS does not make one.** `border-style: dashed` leaves the
/// pattern to the engine, so there is no authoritative number to copy out of the
/// mock-up — what a browser shows is Blink's choice. Three is Blink's ratio, and
/// it is picked here for the same reason it was picked there: at one and a half
/// pixels of stroke it is the shortest dash that still reads as a dash rather
/// than as a dotted line, which matters because these outlines say "this is a
/// place, not a surface" and a dotted rule says "this is a boundary".
///
/// The pattern is fitted to each straight run (see `dashed_outline`), so the
/// ratio sets the look and the run sets the count.
pub const DOCK_DASH_RATIO: f32 = 3.0;

/// Breathing room between a previewed image and its seat's edges, in logical
/// pixels. Skipped entirely when the body is too small to afford it, because a
/// margin that eats the picture serves nobody.
pub const PREVIEW_BODY_INSET_LOGICAL_PX: f32 = 12.0;

/// The two built-in runtime themes. Theme choice is process-wide because the Win32 class brush is
/// process-class state and every renderer/worker must agree on default terminal colors.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

/// The focused terminal cursor shape selected for the process.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CursorStyle {
    #[default]
    Bar,
    Block,
    Underline,
}

impl CursorStyle {
    const fn bits(self) -> u64 {
        match self {
            Self::Bar => 0,
            Self::Block => 1,
            Self::Underline => 2,
        }
    }

    const fn from_bits(bits: u64) -> Self {
        match bits & 0b11 {
            1 => Self::Block,
            2 => Self::Underline,
            _ => Self::Bar,
        }
    }
}

// One acquire load is the complete cursor-style snapshot: bits 0..=1 are the style and 2..=63
// are a monotonic revision. The revision makes concurrent writers observable without allowing a
// reader to combine a style from one update with metadata from another.
struct CursorStyleState {
    packed: AtomicU64,
}

impl CursorStyleState {
    const fn new() -> Self {
        Self {
            packed: AtomicU64::new(0),
        }
    }

    fn load(&self) -> u64 {
        self.packed.load(Ordering::Acquire)
    }

    fn set(&self, style: CursorStyle) -> bool {
        let mut current = self.load();
        loop {
            if CursorStyle::from_bits(current) == style {
                return false;
            }
            let revision = (current >> 2).saturating_add(1);
            let next = (revision << 2) | style.bits();
            match self.packed.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(observed) => current = observed,
            }
        }
    }
}

fn process_cursor_style() -> &'static CursorStyleState {
    static CURSOR_STYLE: CursorStyleState = CursorStyleState::new();
    &CURSOR_STYLE
}

/// Set the process-wide focused cursor shape. Returns whether the shape changed.
pub fn set_cursor_style(style: CursorStyle) -> bool {
    process_cursor_style().set(style)
}

/// Read the process-wide focused cursor shape from one atomic snapshot.
pub fn current_cursor_style() -> CursorStyle {
    CursorStyle::from_bits(process_cursor_style().load())
}

impl Theme {
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    fn background(self) -> [u8; 3] {
        scheme_for_theme(self).background
    }
}

/// Result of asking the process theme state to change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThemeChange {
    Changed,
    Unchanged,
    /// A valid `BT_BG` diagnostic override owns the process colors for its entire lifetime.
    LockedByEnvironment,
}

// One acquire load is the complete read path. Keeping background, selected theme, lock bit and
// revision in the same atomic also prevents readers from combining a new color with an old key.
//
// 0: selected theme, 1: BT_BG lock, 2..=25: sRGB background, 26..=63: revision.
const LOCKED_BIT: u64 = 1 << 1;
const RGB_SHIFT: u32 = 2;
const RGB_MASK: u64 = 0x00ff_ffff << RGB_SHIFT;
const REVISION_SHIFT: u32 = 26;
const REVISION_MAX: u64 = (1 << (64 - REVISION_SHIFT)) - 1;

struct ThemeState {
    packed: AtomicU64,
}

impl ThemeState {
    fn new(theme: Theme, background: [u8; 3], locked: bool) -> Self {
        Self {
            packed: AtomicU64::new(pack_theme_state(theme, background, locked, 1)),
        }
    }

    fn from_environment(value: Option<&OsStr>, report: bool) -> Self {
        let Some(value) = value else {
            return Self::new(Theme::Dark, DEFAULT_BACKGROUND_RGB, false);
        };
        let Some(rgb) = value.to_str().and_then(parse_background_rgb) else {
            if report {
                eprintln!(
                    "BT_THEME invalid_BT_BG={value:?} ignored default=#1B1B1B expected=#RRGGBB runtime_theme_locked=false"
                );
            }
            return Self::new(Theme::Dark, DEFAULT_BACKGROUND_RGB, false);
        };
        if report {
            eprintln!(
                "BT_THEME BT_BG_override=#{:02X}{:02X}{:02X} runtime_theme_locked=true",
                rgb[0], rgb[1], rgb[2]
            );
        }
        Self::new(Theme::Dark, rgb, true)
    }

    fn load(&self) -> u64 {
        self.packed.load(Ordering::Acquire)
    }

    fn set(&self, theme: Theme) -> ThemeChange {
        let mut current = self.load();
        loop {
            if current & LOCKED_BIT != 0 {
                return ThemeChange::LockedByEnvironment;
            }
            if unpack_theme(current) == theme {
                return ThemeChange::Unchanged;
            }
            let revision = unpack_revision(current).saturating_add(1).min(REVISION_MAX);
            let next = pack_theme_state(theme, theme.background(), false, revision);
            match self.packed.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return ThemeChange::Changed,
                Err(observed) => current = observed,
            }
        }
    }
}

impl ThemeState {
    // Re-read the background from the scheme now in force and advance the
    // revision. The theme itself does not move: a scheme never overrides the
    // Light/Dark/System decision, it only says what that decision *looks* like.
    fn refresh(&self) {
        let mut current = self.load();
        loop {
            let locked = current & LOCKED_BIT != 0;
            let theme = unpack_theme(current);
            let background = if locked {
                unpack_background(current)
            } else {
                theme.background()
            };
            let revision = unpack_revision(current).saturating_add(1).min(REVISION_MAX);
            let next = pack_theme_state(theme, background, locked, revision);
            match self.packed.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }
}

fn process_theme() -> &'static ThemeState {
    static THEME: OnceLock<ThemeState> = OnceLock::new();
    THEME.get_or_init(|| ThemeState::from_environment(std::env::var_os("BT_BG").as_deref(), true))
}

fn pack_theme_state(theme: Theme, background: [u8; 3], locked: bool, revision: u64) -> u64 {
    let theme = u64::from(theme == Theme::Light);
    let locked = if locked { LOCKED_BIT } else { 0 };
    let rgb = (u64::from(background[0]) << 16)
        | (u64::from(background[1]) << 8)
        | u64::from(background[2]);
    theme | locked | (rgb << RGB_SHIFT) | (revision.min(REVISION_MAX) << REVISION_SHIFT)
}

fn unpack_theme(packed: u64) -> Theme {
    if packed & 1 == 0 {
        Theme::Dark
    } else {
        Theme::Light
    }
}

fn unpack_background(packed: u64) -> [u8; 3] {
    let rgb = (packed & RGB_MASK) >> RGB_SHIFT;
    [
        ((rgb >> 16) & 0xff) as u8,
        ((rgb >> 8) & 0xff) as u8,
        (rgb & 0xff) as u8,
    ]
}

fn unpack_revision(packed: u64) -> u64 {
    packed >> REVISION_SHIFT
}

/// Switch the process theme. A valid `BT_BG` override returns
/// [`ThemeChange::LockedByEnvironment`] and leaves all four theme readings untouched.
pub fn set_theme(theme: Theme) -> ThemeChange {
    process_theme().set(theme)
}

/// The selected built-in theme. Under `BT_BG`, this remains dark because the diagnostic override is
/// deliberately not a third persisted theme.
pub fn current_theme() -> Theme {
    unpack_theme(process_theme().load())
}

/// Process-wide sRGB background, read with one uncontended atomic load on the render hot path.
pub fn background_rgb() -> [u8; 3] {
    unpack_background(process_theme().load())
}

/// Default terminal ink paired with the process theme background. The current product surface
/// exposes `BT_BG` as its theme diagnostic; choosing the higher-contrast Campbell ink here also
/// gives math rasterization the same dark/light decision as ordinary default-colored text.
pub fn foreground_rgb() -> [u8; 3] {
    foreground_for_background(background_rgb())
}

/// Monotonic process identity for theme-authored layout artifacts. `BT_BG` is fixed before any
/// artifact exists; runtime dark/light changes advance this revision so CPU math rasters and their
/// independently keyed GPU textures cannot be reused under the new colors.
pub fn theme_revision() -> u64 {
    unpack_revision(process_theme().load())
}

/// Which canvas a background belongs to — the one dark/light decision the whole
/// product takes, at one threshold.
///
/// Public because the app has to file a scheme under the row whose canvas it
/// can actually wear: a light scheme in the dark slot would paint a light
/// canvas and then be handed the dark scheme's chrome, and the only way to make
/// that unreachable is for the picker to ask the same question the renderer
/// asks.
#[must_use]
pub fn background_is_light(background: [u8; 3]) -> bool {
    let background_luma = u32::from(background[0]) * 299
        + u32::from(background[1]) * 587
        + u32::from(background[2]) * 114;
    background_luma >= 128_000
}

fn foreground_for_background(background: [u8; 3]) -> [u8; 3] {
    crate::scheme::foreground_of(&scheme_for_background(background))
}

pub(crate) fn parse_background_rgb(value: &str) -> Option<[u8; 3]> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    Some([
        u8::from_str_radix(&hex[0..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..6], 16).ok()?,
    ])
}

/// ANSI colors 0-15 from Windows Terminal's built-in Campbell scheme, in normal then bright
/// order. This is the dark theme's compatibility palette and must remain byte-for-byte Campbell.
pub(crate) const DARK_ANSI_16_RGB: [[u8; 3]; 16] = [
    [0x0c, 0x0c, 0x0c],
    [0xc5, 0x0f, 0x1f],
    [0x13, 0xa1, 0x0e],
    [0xc1, 0x9c, 0x00],
    [0x00, 0x37, 0xda],
    [0x88, 0x17, 0x98],
    [0x3a, 0x96, 0xdd],
    [0xcc, 0xcc, 0xcc],
    [0x76, 0x76, 0x76],
    [0xe7, 0x48, 0x56],
    [0x16, 0xc6, 0x0c],
    [0xf9, 0xf1, 0xa5],
    [0x3b, 0x78, 0xff],
    [0xb4, 0x00, 0x9e],
    [0x61, 0xd6, 0xd6],
    [0xf2, 0xf2, 0xf2],
];

/// ANSI colors 0-15 for the light theme, using macOS Terminal.app's default palette.
pub(crate) const LIGHT_ANSI_16_RGB: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0x99, 0x00, 0x00],
    [0x00, 0xa6, 0x00],
    [0x99, 0x99, 0x00],
    [0x00, 0x00, 0xb2],
    [0xb2, 0x00, 0xb2],
    [0x00, 0xa6, 0xb2],
    [0xbf, 0xbf, 0xbf],
    [0x66, 0x66, 0x66],
    [0xe6, 0x00, 0x00],
    [0x00, 0xd9, 0x00],
    [0xe5, 0xe5, 0x00],
    [0x00, 0x00, 0xff],
    [0xe6, 0x00, 0xe6],
    [0x00, 0xe6, 0xe6],
    [0xe6, 0xe6, 0xe6],
];

/// The explicit ANSI palette selected by the process theme.
///
/// By value rather than by `&'static`, which is the whole cost of the sixteen
/// becoming a *scheme's* sixteen: there is no longer a static to borrow, and
/// forty-eight bytes copied at the three call sites is cheaper than a lock held
/// across a caller's loop.
pub(crate) fn ansi_16_rgb() -> [[u8; 3]; 16] {
    ansi_16_rgb_for(current_theme())
}

fn ansi_16_rgb_for(theme: Theme) -> [[u8; 3]; 16] {
    scheme_for_theme(theme).ansi
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_style_snapshot_survives_one_writer_for_200_frames() {
        use std::sync::Arc;

        let state = Arc::new(CursorStyleState::new());
        let writer_state = Arc::clone(&state);
        let writer = std::thread::spawn(move || {
            for index in 0..200 {
                let style =
                    [CursorStyle::Bar, CursorStyle::Block, CursorStyle::Underline][index % 3];
                writer_state.set(style);
            }
        });
        let mut previous_revision = 0;
        for _frame in 0..200 {
            let snapshot = state.load();
            assert!(snapshot & 0b11 <= CursorStyle::Underline.bits());
            let revision = snapshot >> 2;
            assert!(revision >= previous_revision);
            previous_revision = revision;
        }
        writer.join().unwrap();
        assert!(state.load() >> 2 > 0);
    }

    #[test]
    fn ansi_palettes_pin_campbell_dark_and_mac_terminal_light_byte_for_byte() {
        const CAMPBELL: [[u8; 3]; 16] = [
            [0x0c, 0x0c, 0x0c],
            [0xc5, 0x0f, 0x1f],
            [0x13, 0xa1, 0x0e],
            [0xc1, 0x9c, 0x00],
            [0x00, 0x37, 0xda],
            [0x88, 0x17, 0x98],
            [0x3a, 0x96, 0xdd],
            [0xcc, 0xcc, 0xcc],
            [0x76, 0x76, 0x76],
            [0xe7, 0x48, 0x56],
            [0x16, 0xc6, 0x0c],
            [0xf9, 0xf1, 0xa5],
            [0x3b, 0x78, 0xff],
            [0xb4, 0x00, 0x9e],
            [0x61, 0xd6, 0xd6],
            [0xf2, 0xf2, 0xf2],
        ];
        const MAC_TERMINAL: [[u8; 3]; 16] = [
            [0x00, 0x00, 0x00],
            [0x99, 0x00, 0x00],
            [0x00, 0xa6, 0x00],
            [0x99, 0x99, 0x00],
            [0x00, 0x00, 0xb2],
            [0xb2, 0x00, 0xb2],
            [0x00, 0xa6, 0xb2],
            [0xbf, 0xbf, 0xbf],
            [0x66, 0x66, 0x66],
            [0xe6, 0x00, 0x00],
            [0x00, 0xd9, 0x00],
            [0xe5, 0xe5, 0x00],
            [0x00, 0x00, 0xff],
            [0xe6, 0x00, 0xe6],
            [0x00, 0xe6, 0xe6],
            [0xe6, 0xe6, 0xe6],
        ];
        assert_eq!(ansi_16_rgb_for(Theme::Dark), CAMPBELL);
        assert_eq!(ansi_16_rgb_for(Theme::Light), MAC_TERMINAL);
    }

    /// PIN: the selection fill is a *default* colour, so it follows the same
    /// background-luma switch that already chooses the terminal's own ink —
    /// never one value for both canvases.
    ///
    /// Dark keeps the value it shipped with; light is the mock-up's `--accent`
    /// #3059D8 at the 30% its own in-terminal highlight (`mark.srch`) uses,
    /// composited over the light `--termbg` #FFFFFF in sRGB the way every other
    /// translucent-over-known-surface colour in this file is pre-composited.
    /// PIN (A38-A40, §7.1.5d) — **a search hit wears two grounds and one borrowed ink**, and the
    /// ink is the terminal's own background on both canvases.
    ///
    /// The stylesheet spends a paragraph on why (mock 1526-1529): *"dark text on the light-theme
    /// indigo would fail, white text on the dark-theme periwinkle would fail — each theme's
    /// terminal background is exactly the ink that contrasts."* So there is no new ink at all —
    /// the current hit takes `--termbg` — and the two grounds are the accent, once at 30% and once
    /// solid.
    ///
    /// MUTATIONS:
    /// (1) make the current hit's ink a fixed white or black — one of the two canvases loses its
    ///     contrast, which is the ruling's own argument run backwards;
    /// (2) give the ordinary hit the solid accent — the current one stops being distinguishable,
    ///     which is what the two grounds exist to make it.
    #[test]
    fn a_search_hit_is_the_accent_twice_over_and_takes_the_canvas_as_its_ink() {
        // `--accent #7A99FF` at 30% over `--termbg #1B1B1B`: 27 + (122-27)x.3 = 55.5,
        // 27 + (153-27)x.3 = 64.8, 27 + (255-27)x.3 = 95.4 -> #38415F.
        assert_eq!(
            search_match_for_background(DEFAULT_BACKGROUND_RGB),
            [0x38, 0x41, 0x5f],
        );
        // 255 - (255 - 48) * .3 and its companions, over `--termbg #FFFFFF`.
        assert_eq!(
            search_match_for_background(LIGHT_BACKGROUND_RGB),
            [0xc1, 0xcd, 0xf3],
        );
        // The current hit is the accent itself, which is what makes it unmistakable beside the
        // shadow of it the others wear.
        assert_eq!(
            search_current_for_background(DEFAULT_BACKGROUND_RGB),
            [0x7a, 0x99, 0xff],
        );
        assert_eq!(
            search_current_for_background(LIGHT_BACKGROUND_RGB),
            [0x30, 0x59, 0xd8],
        );
        // The ordinary ground is pale enough to leave the text on it alone, on either canvas —
        // which is the whole of "hits stay readable in place".
        assert!(!background_is_light(search_match_for_background(
            DEFAULT_BACKGROUND_RGB
        )));
        assert!(background_is_light(search_match_for_background(
            LIGHT_BACKGROUND_RGB
        )));
        // And the current hit's ground is the *other* side of the threshold from its own ink on
        // both canvases, which is the ruling stated as an inequality rather than as a paragraph.
        for canvas in [DEFAULT_BACKGROUND_RGB, LIGHT_BACKGROUND_RGB] {
            assert_ne!(
                background_is_light(search_current_for_background(canvas)),
                background_is_light(canvas),
                "`--termbg` as the current hit's ink only works if the accent is on the other                  side of the threshold from it",
            );
        }
        // The switch is the terminal ink's switch, taken at the same threshold as the selection's
        // beside it — so a `BT_BG` override moves both together.
        assert_eq!(
            search_match_for_background([0x0c; 3]),
            DEFAULT_SEARCH_MATCH_RGB
        );
        assert_eq!(
            search_match_for_background([0xf5; 3]),
            LIGHT_SEARCH_MATCH_RGB
        );
    }

    #[test]
    fn selection_background_follows_the_same_luma_threshold_as_the_terminal_ink() {
        assert_eq!(
            selection_background_for_background(DEFAULT_BACKGROUND_RGB),
            [0x26, 0x4f, 0x78],
            "the dark canvas keeps the fill the user never complained about",
        );
        assert_eq!(
            selection_background_for_background(LIGHT_BACKGROUND_RGB),
            [0xc1, 0xcd, 0xf3],
            "the light canvas gets accent's palest face, not the dark navy",
        );
        // The switch is the ink's switch, taken at the same threshold.
        assert_eq!(
            selection_background_for_background([0x0c; 3]),
            DEFAULT_SELECTION_BACKGROUND_RGB
        );
        assert_eq!(
            selection_background_for_background([0xf5; 3]),
            LIGHT_SELECTION_BACKGROUND_RGB
        );
        // Default ink stays legible on whichever fill it lands on: the light
        // fill is pale enough that `--ink` #37352F still reads, which is why no
        // inverted selection foreground is invented anywhere in the renderer.
        assert_ne!(
            LIGHT_SELECTION_BACKGROUND_RGB,
            DEFAULT_SELECTION_BACKGROUND_RGB
        );
        assert!(
            background_is_light(LIGHT_SELECTION_BACKGROUND_RGB),
            "black text on the light selection, so the fill must stay a light one"
        );
        assert!(
            !background_is_light(DEFAULT_SELECTION_BACKGROUND_RGB),
            "light text on the dark selection"
        );
    }

    /// PIN (styling pass): the chrome's dark/light decision is the terminal
    /// ink's decision — same threshold, one switch for the whole product.
    #[test]
    fn chrome_palette_follows_the_same_luma_threshold_as_the_terminal_ink() {
        assert_eq!(chrome_palette_for_background([0x0c; 3]), DARK_CHROME);
        assert_eq!(chrome_palette_for_background([0xf5; 3]), LIGHT_CHROME);
        // The two palettes disagree everywhere it matters, so selecting the
        // wrong one cannot pass unnoticed.
        assert_ne!(DARK_CHROME.title_bar, LIGHT_CHROME.title_bar);
        assert_ne!(DARK_CHROME.divider_active, LIGHT_CHROME.divider_active);
        assert_eq!(DARK_CHROME.caption_hover, [0x31, 0x31, 0x31]);
        assert_eq!(LIGHT_CHROME.caption_hover, [0xec, 0xec, 0xea]);
        assert_eq!(DARK_CHROME.caption_close_hover, [0xe5, 0x48, 0x4d]);
        assert_eq!(LIGHT_CHROME.caption_close_hover, [0xe5, 0x48, 0x4d]);
        assert_eq!(DARK_CHROME.active_tab, DEFAULT_BACKGROUND_RGB);
        assert_eq!(LIGHT_CHROME.active_tab, [0xff, 0xff, 0xff]);
    }

    /// PIN (T2 progress ring): the ring *replaces* the mark rather than
    /// encircling it, so it inherits the mark's own 15px box and its radius is
    /// derived from that box and the stroke — not carried over from the 25px
    /// overlay the mock-up used to draw, whose only reason to be 25px was
    /// clearing the mark it sat on top of.
    ///
    /// This began as a deviation from `.pring` under a user ruling; the mock-up
    /// was written back to the replacement ring on 2026-08-08, so the box below
    /// is now the design's own and no longer a departure from it.
    ///
    /// The stroke must lie *inside* the box: a stroke is centred on its path,
    /// so the outer edge is `radius + stroke/2` and that has to be exactly half
    /// the box. Getting this wrong clips the ring against the slot, which is
    /// the one failure a replacement ring cannot survive.
    #[test]
    fn the_progress_ring_fits_the_mark_slot_it_replaces() {
        assert_eq!(WINDOW_TAB_RING_STROKE_LOGICAL_PX, 2.0);
        assert_eq!(WINDOW_TAB_RING_RADIUS_LOGICAL_PX, 6.5);
        let outer = WINDOW_TAB_RING_RADIUS_LOGICAL_PX + WINDOW_TAB_RING_STROKE_LOGICAL_PX / 2.0;
        assert_eq!(
            outer,
            WINDOW_TAB_MARK_LOGICAL_PX / 2.0,
            "the ring's outer edge is the slot's edge — no more, and no less"
        );
    }

    /// PIN (T2 progress ring): the indeterminate arc's *length* is the mock-up's
    /// own, carried across the size change as the ratio it actually is.
    ///
    /// The arc was first struck as `stroke-dasharray: 13 40.4` against a
    /// `PRING_C` of 53.4 ("2πr for r=8.5"). A dash *length* in absolute units
    /// would have shrunk to a speck once the ring took the mark's smaller slot;
    /// the fraction of the circle is what the design actually means, so that is
    /// what this constant holds.
    ///
    /// The mock-up has since been written back to the replacement ring (ruling
    /// 2026-08-08) and re-struck the dash at that size — `9.94 30.9` against
    /// `PRING_C = 40.84` for r=6.5. The last two assertions are the seam
    /// between the two documents: they check that the design's new spelling and
    /// this constant describe the same arc, so an edit to either one alone
    /// fails here rather than drifting quietly.
    #[test]
    fn the_indeterminate_arc_keeps_the_mock_ups_fraction_of_the_circle() {
        let mock_circumference = 2.0 * std::f32::consts::PI * 8.5;
        assert!(
            (mock_circumference - 53.4).abs() < 0.05,
            "the mock-up's original PRING_C must be 2πr for r=8.5"
        );
        assert!(
            (WINDOW_TAB_RING_INDETERMINATE_TURNS - 13.0 / 53.4).abs() < 1e-6,
            "13 of 53.4 units"
        );
        // Roughly a quarter turn: plainly an arc and plainly not a full ring,
        // which is the whole of what "indeterminate" has to say.
        assert!((0.2..0.3).contains(&WINDOW_TAB_RING_INDETERMINATE_TURNS));

        // The mock-up's re-struck dash, against the circumference it now
        // declares. Both are quoted as the design spells them, to two decimals,
        // so this reads as the arithmetic check it is.
        let restruck_circumference = 2.0 * std::f32::consts::PI * WINDOW_TAB_RING_RADIUS_LOGICAL_PX;
        assert!(
            (restruck_circumference - 40.84).abs() < 0.005,
            "the mock-up's current PRING_C must be 2πr for this ring's radius"
        );
        assert!(
            (9.94 / 40.84 - WINDOW_TAB_RING_INDETERMINATE_TURNS).abs() < 1e-4,
            "the design's re-struck dash and this ratio must be one arc"
        );
    }

    /// PIN (T2 tab status, amended by R29 2026-08-15 and by the rose ruling of
    /// 2026-08-16): **two of the "something happened" colours are one set and
    /// three are not**, and which is which is read off the design rather than
    /// chosen.
    ///
    /// This test used to say the split itself was forbidden — "a theme split here
    /// would be an invention, not a reading". That was true of the three it was
    /// written about and was never true of the design's own comment, which
    /// declares the four in `:root` "so both themes share them **until a
    /// walkthrough proves a theme needs its own**". `body.dark` proved it first
    /// for `--ok` (mock-up 74) and now for `--err` as well. So the claim the test
    /// makes is narrowed to the two that are genuinely shared, and the three that
    /// vary are pinned as *varying* — which is a stronger statement than the old
    /// one, not a weaker one, because a future palette that quietly folded either
    /// back to one value would now fail here instead of passing.
    #[test]
    fn the_status_colours_are_one_set_shared_by_both_canvases() {
        for palette in [DARK_CHROME, LIGHT_CHROME] {
            assert_eq!(palette.status_warn, [0xd9, 0x82, 0x2b], "--warn");
            assert_eq!(palette.status_pause, [0xc1, 0x9c, 0x00], "--pause");
        }
        // The rose, which is the third that varies (user ruling, 2026-08-16).
        assert_eq!(LIGHT_CHROME.status_err, [0xe1, 0x1d, 0x48], "--err");
        assert_eq!(DARK_CHROME.status_err, [0xfb, 0x71, 0x85], "dark --err");
        // The accent is the fourth claim ("finished, unread") and is one of the
        // two that *do* vary by canvas, so the dot's four colours are never a
        // single constant table.
        assert_ne!(DARK_CHROME.accent, LIGHT_CHROME.accent);
        // And the fifth, which the Git page brought: `:root`'s green, and
        // `body.dark`'s lighter one over the dark canvas.
        assert_eq!(LIGHT_CHROME.status_ok, [0x1a, 0x7f, 0x37], "--ok");
        assert_eq!(DARK_CHROME.status_ok, [0x57, 0xab, 0x5a], "dark --ok");
    }

    /// PIN (user ruling, 2026-08-16) — **the rose is legible on its own canvas,
    /// which is the whole reason it is two values and not one.**
    ///
    /// The floor is 4.5:1 and not the lane wheel's 3:1, because this ink is worn
    /// by *words*: a toast's mark stands beside a sentence, and the Git page's
    /// `D` badge is a letter. The danger red the ruling replaced managed 2.8:1
    /// over `#1B1B1B` — legible on paper, a bruise on the dark canvas — and the
    /// assertion below is what stops a future retune from landing there again by
    /// picking one rose and using it twice.
    #[test]
    fn the_error_rose_reads_on_both_canvases() {
        for (palette, canvas) in [(DARK_CHROME, TERMBG_DARK), (LIGHT_CHROME, TERMBG_LIGHT)] {
            let body = contrast(palette.status_err, canvas);
            assert!(body >= 4.5, "the rose over its own canvas: {body:.2}:1");
            // And over `--menu`, which is the face a toast is drawn on — the
            // first surface to wear this ink over something other than the body.
            let card = contrast(palette.status_err, palette.menu_surface);
            assert!(card >= 4.5, "the rose over a menu's face: {card:.2}:1");
        }
        // And the one that was replaced does not: this is the ruling's own
        // arithmetic, kept so the reason survives the change.
        assert!(contrast([0xc5, 0x0f, 0x1f], TERMBG_DARK) < 3.0);
    }

    /// The command marks rail's three inks, each stated as the mixing rule it
    /// came from rather than as the six bytes it came out as.
    ///
    /// The crest's is the one worth pinning: `docs/DESIGN.md` §7.1.5c says the
    /// accent is deepened with **twelve** per cent black and the mock-up's own
    /// `color-mix(in srgb, var(--accent) 86%, #000)` says fourteen. The
    /// stylesheet wins — it is the executable artefact — and this is where that
    /// ruling is kept, so a future reader who finds the prose first is corrected
    /// by an assertion rather than by a paragraph.
    #[test]
    fn the_command_ticks_wear_the_mock_ups_own_three_mixes() {
        for palette in [DARK_CHROME, LIGHT_CHROME] {
            // Rest: `--ink3` over `--termbg`, which is what an unfocused pane
            // title is mixed from — the same two colours, so the same answer.
            assert_eq!(
                palette.command_tick, palette.pane_title,
                "--ink3 on --termbg"
            );
            // Crest: the accent at 86% over black, channel by channel.
            for channel in 0..3 {
                let mixed = (f32::from(palette.accent[channel]) * 0.86).round() as u8;
                assert_eq!(
                    palette.command_tick_crest[channel], mixed,
                    "the crest is the accent with 14% black, not 12%"
                );
            }
        }
        // Fail crest: `--err-deep`, one value per canvas like the rose it deepens.
        assert_eq!(LIGHT_CHROME.command_tick_fail_crest, [0xbe, 0x12, 0x3c]);
        assert_eq!(DARK_CHROME.command_tick_fail_crest, [0xf4, 0x3f, 0x5e]);
        // And it *is* a deepening: darker than the rose on the light canvas,
        // lighter on the dark one, because "deeper" means further from the ground
        // rather than further from white.
        assert!(
            luminance(LIGHT_CHROME.command_tick_fail_crest) < luminance(LIGHT_CHROME.status_err)
        );
        assert!(luminance(DARK_CHROME.command_tick_fail_crest) < luminance(DARK_CHROME.status_err));
    }

    /// The search crest (S4): `--ink2` over `--termbg`, and **not** the accent.
    ///
    /// MUTATION: point `command_tick_search_crest` at the accent and a command
    /// tick under the pointer becomes indistinguishable from a match — which is
    /// the one distinction the results rail exists to draw.
    #[test]
    fn the_search_crest_is_grey_because_the_accent_belongs_to_the_matches() {
        for palette in [DARK_CHROME, LIGHT_CHROME] {
            assert_eq!(
                palette.command_tick_search_crest, palette.files_row_text,
                "--ink2 on --termbg"
            );
            assert_ne!(palette.command_tick_search_crest, palette.accent);
            assert_ne!(
                palette.command_tick_search_crest,
                palette.command_tick_crest
            );
            // Deeper than the resting tick, which is `--ink3` over the same
            // ground: the crest is the tick that has been singled out, and it is
            // singled out by standing further from the background than its
            // neighbours rather than by changing hue.
            assert_ne!(
                luminance(palette.command_tick_search_crest),
                luminance(palette.pane_title)
            );
        }
    }

    /// Relative luminance, sRGB, as WCAG defines it — the one arithmetic a
    /// contrast claim can be checked with.
    fn luminance(colour: [u8; 3]) -> f64 {
        let channel = |byte: u8| {
            let value = f64::from(byte) / 255.0;
            if value <= 0.040_45 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(colour[0]) + 0.7152 * channel(colour[1]) + 0.0722 * channel(colour[2])
    }

    fn contrast(one: [u8; 3], other: [u8; 3]) -> f64 {
        let (a, b) = (luminance(one), luminance(other));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// PIN (R18): eight lanes, all eight legible on both canvases, and no two
    /// of them the same colour.
    ///
    /// The floor is **3:1**, which is the non-text contrast bar and the right
    /// one here: a lane is a 1.7-pixel line and a 7.2-pixel dot, a graphical
    /// object rather than a letter. Both grounds are checked, because the graph
    /// draws on the pane's own body and its rows light up on a card.
    ///
    /// This is what the mock-up could not have passed: it declared three lane
    /// colours and indexed straight into them, so a fourth lane's stroke was the
    /// string `undefined`. The distinctness assertion below is the half of R18
    /// that says the *wheel* is a wheel — eight roads a reader can tell apart —
    /// and the contrast assertion is the half that says every one of them is
    /// there at all.
    #[test]
    fn the_graph_lane_wheel_is_eight_colours_that_clear_both_canvases() {
        for (palette, canvas, card) in [
            (DARK_CHROME, TERMBG_DARK, PANEL_DARK),
            (LIGHT_CHROME, TERMBG_LIGHT, PANEL_LIGHT),
        ] {
            assert_eq!(palette.graph_lanes.len(), GRAPH_LANE_COUNT);
            for (index, lane) in palette.graph_lanes.iter().enumerate() {
                assert!(
                    contrast(*lane, canvas) >= 3.0,
                    "lane {index} {lane:02x?} against the body: {:.2}:1",
                    contrast(*lane, canvas)
                );
                assert!(
                    contrast(*lane, card) >= 3.0,
                    "lane {index} {lane:02x?} against the card: {:.2}:1",
                    contrast(*lane, card)
                );
            }
            for (index, lane) in palette.graph_lanes.iter().enumerate() {
                for (other, second) in palette.graph_lanes.iter().enumerate().skip(index + 1) {
                    assert_ne!(lane, second, "lanes {index} and {other} are one colour");
                }
            }
        }
    }

    /// PIN (#49): the seven highlight inks are legible **as text** on both of
    /// the two grounds a preview sets source on, in both themes.
    ///
    /// The floor here is **4.5:1** and not the lanes' 3:1, and the difference is
    /// the difference between a stroke and a letter: a lane is a drawn object,
    /// a keyword is something you read. Both grounds are checked because the
    /// same seven inks serve a source-file body (`--termbg`) and a markdown
    /// fence (`--panel`), which are two colours in both themes.
    ///
    /// The distinctness half matters as much as the floor: seven inks that a
    /// reader cannot tell apart are one ink spent seven times. Comment and
    /// punctuation are the one deliberate pair — they *are* one value, named
    /// twice — so they are exempt from that half and from nothing else.
    #[test]
    fn the_seven_highlight_inks_clear_the_text_bar_on_both_preview_grounds() {
        for (theme, palette, body, fence) in [
            ("dark", DARK_CHROME, TERMBG_DARK, PANEL_DARK),
            ("light", LIGHT_CHROME, TERMBG_LIGHT, PANEL_LIGHT),
        ] {
            let inks = [
                ("hl_keyword", palette.hl_keyword),
                ("hl_string", palette.hl_string),
                ("hl_comment", palette.hl_comment),
                ("hl_number", palette.hl_number),
                ("hl_type", palette.hl_type),
                ("hl_function", palette.hl_function),
                ("hl_punct_muted", palette.hl_punct_muted),
            ];
            for (name, ink) in inks {
                assert!(
                    contrast(ink, body) >= 4.5,
                    "{theme} {name} {ink:02x?} on the preview body: {:.2}:1",
                    contrast(ink, body)
                );
                assert!(
                    contrast(ink, fence) >= 4.5,
                    "{theme} {name} {ink:02x?} inside a fence: {:.2}:1",
                    contrast(ink, fence)
                );
            }
            // The comment and the punctuation are one value on purpose; every
            // other pair has to be two.
            assert_eq!(
                palette.hl_comment, palette.hl_punct_muted,
                "{theme}: the quiet ink is one value under two names"
            );
            for (index, (name, ink)) in inks.iter().enumerate() {
                for (other, second) in inks.iter().skip(index + 1) {
                    if (*name == "hl_comment" && *other == "hl_punct_muted")
                        || (*name == "hl_punct_muted" && *other == "hl_comment")
                    {
                        continue;
                    }
                    assert_ne!(ink, second, "{theme}: {name} and {other} are one colour");
                }
            }
            // And none of the seven is the accent: a call is not a link.
            assert_ne!(palette.hl_function, palette.accent, "{theme}");
        }
    }

    /// PIN (R29): the Git page's card and the inks on it are `--panel` and the
    /// design's own alphas over it, not a second grey invented for the panel.
    ///
    /// Spot-checked at both ends of the ladder rather than field by field: if
    /// `ink_over` and the tokens are right, every entry between them is right by
    /// the same arithmetic, and if either is wrong these two are the first to
    /// say so. The dark card is `#252525`; `--ink` at .87 over it is
    /// 37 + 218×.87 = 226.7 → `#E3`; `--ink3` at .38 is 37 + 218×.38 = 119.8 →
    /// `#78`.
    #[test]
    fn the_git_card_and_its_inks_are_the_designs_own_tokens() {
        assert_eq!(DARK_CHROME.git_section, [0x25, 0x25, 0x25]);
        assert_eq!(LIGHT_CHROME.git_section, [0xf7, 0xf7, 0xf5]);
        assert_eq!(DARK_CHROME.git_row_text, [0xe3, 0xe3, 0xe3]);
        assert_eq!(DARK_CHROME.git_row_muted, [0x78, 0x78, 0x78]);
        // `--ink` is opaque on light, so a path on the card is the design's own
        // `#37352F` and not a mix of it.
        assert_eq!(LIGHT_CHROME.git_row_text, [0x37, 0x35, 0x2f]);
        // A hovered row is lighter than its card on dark and darker on light —
        // the one thing a mis-signed composite would get backwards.
        assert!(DARK_CHROME.git_row_hover[0] > DARK_CHROME.git_section[0]);
        assert!(LIGHT_CHROME.git_row_hover[0] < LIGHT_CHROME.git_section[0]);
    }

    /// PIN (V8, 2026-08-16): the graph's selected row is the tree's selected
    /// row, and the description on it is still a description.
    ///
    /// Two claims, and the second is why this is a test rather than a comment. A
    /// *selected* ground is lighter than the hover it replaces on dark and darker
    /// on light, so a token struck by hand could easily land somewhere the ink
    /// standing on it no longer clears the bar — and the ink standing on it is
    /// `files_row_text_selected`, which was premixed over
    /// [`ChromePalette::files_row_selected`] and would be quietly wrong the day
    /// these two stopped being one colour. The floor is **4.5:1**: the
    /// description is body text, not furniture.
    #[test]
    fn the_graphs_selected_row_is_the_trees_and_its_description_stays_legible() {
        for (name, palette) in [("dark", &DARK_CHROME), ("light", &LIGHT_CHROME)] {
            assert_eq!(
                palette.git_row_selected, palette.files_row_selected,
                "{name}: one grey for 'the row you are on', or the window has two"
            );
            // Selected is a step further from the body than hover is, on both
            // canvases — the one thing a mis-signed value would get backwards.
            assert_ne!(palette.git_row_selected, palette.files_row_hover);
            let ratio = contrast(palette.files_row_text_selected, palette.git_row_selected);
            assert!(
                ratio >= 4.5,
                "{name}: a subject on the selected row reads at {ratio:.2}:1"
            );
            // **The search's own ground stands between the two** (T4,
            // 2026-08-16). Three claims, and each one is a way the token could
            // be wrong: it has to be a step off the body, so a match is visible
            // at all; it has to be *short* of the selection, so seventeen
            // matches do not read as seventeen cursors; and the subject on it
            // has to stay as readable as it was on the body it came off.
            //
            // The third is written as a *ratio* and not as a floor, and that is
            // deliberate. A matched row wears the row's ordinary rest ink —
            // `files_row_text`, not the premixed selected one — which clears
            // 4.18:1 over the light body and has always done so; a 4.5 floor
            // here would be this ticket inventing a bar the product has never
            // met anywhere, failing on this one row and nowhere else. What this
            // ground may not do is *cost* that ink anything a reader would
            // notice, and a seventh is the whole of the room it is given.
            let body = if name == "dark" {
                TERMBG_DARK
            } else {
                TERMBG_LIGHT
            };
            let distance = |ground: [u8; 3]| i32::from(ground[0]) - i32::from(body[0]);
            assert_ne!(
                palette.git_row_match, palette.git_row_selected,
                "{name}: a matched row wearing the selected grey claims a cursor it has not got"
            );
            assert!(
                distance(palette.git_row_match).abs() > 0,
                "{name}: a matched row that is the pane's own ground is not marked at all"
            );
            assert!(
                distance(palette.git_row_match).abs() < distance(palette.git_row_selected).abs(),
                "{name}: a match must be quieter than the selection standing on it"
            );
            let matched = contrast(palette.files_row_text, palette.git_row_match);
            let plain = contrast(palette.files_row_text, body);
            assert!(
                matched >= plain * 0.85,
                "{name}: a matched row costs the subject too much — {matched:.2}:1 \
                 against {plain:.2}:1 on the body it came off"
            );
            // The muted columns beside it are held to nothing here on purpose:
            // `--ink3` is the design's *quiet* ink and clears 2.4:1 over this
            // ground on light — the same 2.4 it clears over every other ground
            // in this palette. A floor invented for this one row would be a bar
            // the product has never claimed to meet, failing here and nowhere
            // else. What has to be true is that they are *the same* ink over
            // *this* ground, which the equality above already pins.
        }
    }

    /// PIN: [`ink_over`] is the sRGB lerp the design's `color-mix` is, rounded
    /// half away from zero, and it is exact at both ends.
    ///
    /// The two endpoints are what make it safe to call at draw time as well as at
    /// compile time: a badge asked for its ink at 1000 must get the ink itself
    /// back, and one asked at 0 must get its ground — otherwise the "no mix"
    /// case would be a colour a browser never showed.
    #[test]
    fn a_mix_at_either_end_is_the_colour_it_started_from() {
        let ground = [0x25, 0x25, 0x25];
        let ink = [0x57, 0xab, 0x5a];
        assert_eq!(ink_over(ground, ink, 0), ground);
        assert_eq!(ink_over(ground, ink, 1000), ink);
        // 37 + (87−37)×.15 = 44.5 → 45; 37 + (171−37)×.15 = 57.1 → 57.
        assert_eq!(ink_over(ground, ink, 150), [45, 57, 45]);
        // Rounds half away from zero in the darkening direction too.
        assert_eq!(ink_over(ink, ground, 1000), ground);
    }

    /// PIN (T2 progress ring): the ring's track is `--border` at `opacity: .7`
    /// (mock-up line 278), pre-composited over each of the three surfaces a tab
    /// can wear — the same three the pane-count badge needs, and for the same
    /// reason spelled out at `tab_close_pill_on_content`: this pipeline blends
    /// in linear light and a browser blends in sRGB, so handing the blender
    /// `--border`'s own alpha would not reproduce `--border`.
    ///
    /// The track must be plainly *there* and plainly quieter than the arc: on
    /// each canvas it has to sit strictly between the tab it lies on and that
    /// canvas's accent, which is what stops a "subtle" track from being
    /// rounded away into the tab it is supposed to sit on.
    #[test]
    fn the_progress_track_is_the_border_at_seven_tenths_over_each_tab_surface() {
        // .094 white × .7 = .0658 over #1B1B1B / #252525 / #313131.
        assert_eq!(DARK_CHROME.ring_track_on_active_tab, [0x2a, 0x2a, 0x2a]);
        assert_eq!(DARK_CHROME.ring_track_on_resting_tab, [0x33, 0x33, 0x33]);
        assert_eq!(DARK_CHROME.ring_track_on_hovered_tab, [0x3f, 0x3f, 0x3f]);
        // .088 black × .7 = .0616 over #FFFFFF / #F7F7F5 / #ECECEA.
        assert_eq!(LIGHT_CHROME.ring_track_on_active_tab, [0xef, 0xef, 0xef]);
        assert_eq!(LIGHT_CHROME.ring_track_on_resting_tab, [0xe8, 0xe8, 0xe6]);
        assert_eq!(LIGHT_CHROME.ring_track_on_hovered_tab, [0xdd, 0xdd, 0xdc]);

        for (palette, surfaces) in [
            (
                DARK_CHROME,
                [
                    (DARK_CHROME.active_tab, DARK_CHROME.ring_track_on_active_tab),
                    (DARK_CHROME.title_bar, DARK_CHROME.ring_track_on_resting_tab),
                    (
                        DARK_CHROME.caption_hover,
                        DARK_CHROME.ring_track_on_hovered_tab,
                    ),
                ],
            ),
            (
                LIGHT_CHROME,
                [
                    (
                        LIGHT_CHROME.active_tab,
                        LIGHT_CHROME.ring_track_on_active_tab,
                    ),
                    (
                        LIGHT_CHROME.title_bar,
                        LIGHT_CHROME.ring_track_on_resting_tab,
                    ),
                    (
                        LIGHT_CHROME.caption_hover,
                        LIGHT_CHROME.ring_track_on_hovered_tab,
                    ),
                ],
            ),
        ] {
            for (surface, track) in surfaces {
                assert_ne!(track, surface, "a track that vanishes into its tab");
                // Quieter than the arc it carries: the track never competes
                // with the progress it is the backdrop for.
                let distance = |a: [u8; 3], b: [u8; 3]| {
                    a.iter()
                        .zip(b)
                        .map(|(x, y)| i32::from(*x) - i32::from(y))
                        .map(|d| d * d)
                        .sum::<i32>()
                };
                assert!(
                    distance(track, surface) < distance(palette.accent, surface),
                    "the track must sit nearer its tab than the arc does"
                );
            }
        }
    }

    /// PIN: the unfocused caret is the focused caret's own ink, faded toward
    /// the canvas by one constant, pre-composited the way every other paired
    /// colour in this module is. Both canvases are checked, and on each the
    /// faded ink must land strictly between the canvas and the focused ink —
    /// plainly there, plainly quieter.
    #[test]
    fn the_unfocused_caret_is_the_focused_caret_faded_toward_its_canvas() {
        assert_eq!(
            cursor_for_background(DEFAULT_BACKGROUND_RGB),
            [0xd4, 0xd4, 0xd4]
        );
        assert_eq!(
            cursor_for_background(LIGHT_BACKGROUND_RGB),
            [0x37, 0x35, 0x2f]
        );
        assert_eq!(
            unfocused_cursor_for_background(DEFAULT_BACKGROUND_RGB),
            [0x6e, 0x6e, 0x6e]
        );
        assert_eq!(
            unfocused_cursor_for_background(LIGHT_BACKGROUND_RGB),
            [0xa5, 0xa4, 0xa1]
        );
        // On light this is byte-for-byte the mock-up's own `--ink3` over white,
        // because there `--cursor` *is* `--ink` and `--ink3` is that ink at .45.
        assert_eq!(
            unfocused_cursor_for_background(LIGHT_BACKGROUND_RGB),
            LIGHT_CHROME.pane_title
        );

        for background in [DEFAULT_BACKGROUND_RGB, LIGHT_BACKGROUND_RGB] {
            let focused = cursor_for_background(background);
            let faded = unfocused_cursor_for_background(background);
            assert_ne!(
                faded, focused,
                "the two focus states must not share one ink"
            );
            let alpha = f64::from(UNFOCUSED_CURSOR_ALPHA_PERCENT) / 100.0;
            for channel in 0..3 {
                let canvas = f64::from(background[channel]);
                let ink = f64::from(focused[channel]);
                assert_eq!(
                    f64::from(faded[channel]),
                    (canvas + (ink - canvas) * alpha).round(),
                    "channel {channel} is not the caret's ink at {alpha} over its canvas"
                );
                let (low, high) = (canvas.min(ink), canvas.max(ink));
                assert!(
                    f64::from(faded[channel]) > low && f64::from(faded[channel]) < high,
                    "channel {channel} must sit between the canvas and the focused ink"
                );
            }
        }
    }

    /// PIN (visual pass): the three numbers the mock-up gives a floating window
    /// and the caret, held here so overturning one is an edit to this block.
    #[test]
    fn float_window_and_cursor_tokens_are_the_mock_ups_own() {
        assert_eq!(CURSOR_BAR_WIDTH_LOGICAL_PX, 1.0);
        assert_eq!(CURSOR_UNDERLINE_HEIGHT_LOGICAL_PX, 2.0);
        // `border-radius: 10px`, `border: 1px solid var(--border)`.
        assert_eq!(FLOAT_WINDOW_RADIUS_LOGICAL_PX, 10.0);
        assert_eq!(FLOAT_WINDOW_BORDER_LOGICAL_PX, 1.0);
        // Enough reach for two rings to fall off in, and not so much that a
        // "lift" becomes a halo.
        assert!((1.0..=6.0).contains(&FLOAT_WINDOW_SHADOW_LOGICAL_PX));
        // `--menu` on each canvas, and `--border`'s own alpha: .094 white on
        // dark, .088 black on light.
        assert_eq!(DARK_CHROME.menu_surface, [0x2a, 0x2a, 0x2a]);
        assert_eq!(LIGHT_CHROME.menu_surface, [0xff, 0xff, 0xff]);
        assert_eq!(DARK_CHROME.menu_border, [0xff, 0xff, 0xff]);
        assert_eq!(LIGHT_CHROME.menu_border, [0x00, 0x00, 0x00]);
        // .094 × 255 = 23.97, .088 × 255 = 22.44.
        assert_eq!(DARK_CHROME.menu_border_alpha, 24);
        assert_eq!(LIGHT_CHROME.menu_border_alpha, 22);
        // A floating window is its own plane, never the seat chrome's panel. (It
        // *is* `--win` on light, where the mock-up gives `--menu` the same
        // #FFFFFF — a white window on a white page separated by its shadow.)
        for palette in [DARK_CHROME, LIGHT_CHROME] {
            assert_ne!(palette.menu_surface, palette.title_bar);
            // The lift is cast in black on both themes (`--shadow`), and fades.
            assert_eq!(palette.menu_shadow, [0, 0, 0]);
            assert!(palette.menu_shadow_inner_alpha > palette.menu_shadow_outer_alpha);
            assert!(palette.menu_shadow_outer_alpha > 0);
        }
        // The dark canvas needs the heavier lift — `--shadow` is .5/.35 there
        // against .13/.06 on light.
        for (ring, dark, light) in [
            (
                "inner",
                DARK_CHROME.menu_shadow_inner_alpha,
                LIGHT_CHROME.menu_shadow_inner_alpha,
            ),
            (
                "outer",
                DARK_CHROME.menu_shadow_outer_alpha,
                LIGHT_CHROME.menu_shadow_outer_alpha,
            ),
        ] {
            assert!(
                dark > light,
                "the {ring} ring is {dark} on dark and {light} on light"
            );
        }
    }

    /// PIN (U1): the pane head's `×` composites over `--termbg` and over its own
    /// pill, and the head's floor is `--panel`.
    ///
    /// The same discipline as the tab `×`'s six composites, applied to a control
    /// that has fewer grounds to answer to rather than more: a pane head has
    /// exactly one surface, because C24 rules out a fill for focus and there is
    /// no hover fill on a head at all. So the `×` is two inks over `--termbg`
    /// plus the pill between them — three, not six — and the test's job is to
    /// pin the arithmetic and to pin the *reuse*, which is the part that could
    /// silently go wrong: each of the three is numerically the tab's own
    /// counterpart, because the active tab stands on `--termbg` too, and any
    /// future divergence there is a bug in one of the two.
    #[test]
    fn the_pane_heads_close_button_is_mixed_over_the_head_and_over_its_own_pill() {
        // `--ink3` over `--termbg`, which is `pane_title`'s own ground.
        assert_eq!(DARK_CHROME.pane_close_glyph, [0x72, 0x72, 0x72]);
        assert_eq!(LIGHT_CHROME.pane_close_glyph, [0xa5, 0xa4, 0xa1]);
        // `--active` over `--termbg`: 27 + 228×.09 on dark, 255 - {200,202,208}
        // ×.09 on light.
        assert_eq!(DARK_CHROME.pane_close_pill, [0x30, 0x30, 0x30]);
        assert_eq!(LIGHT_CHROME.pane_close_pill, [0xed, 0xed, 0xec]);
        // `--ink` over that pill — never over the bare head.
        assert_eq!(DARK_CHROME.pane_close_glyph_on_pill, [0xe4, 0xe4, 0xe4]);
        assert_eq!(LIGHT_CHROME.pane_close_glyph_on_pill, [0x37, 0x35, 0x2f]);
        // The reuse, stated: same ink, same ground, therefore same number.
        for palette in [DARK_CHROME, LIGHT_CHROME] {
            assert_eq!(
                palette.pane_close_glyph, palette.pane_title,
                "the resting `×` is `--ink3` over the head, which is the title's own ink"
            );
            assert_eq!(
                palette.pane_close_pill, palette.tab_close_pill_on_content,
                "a pane head and the active tab are the same surface"
            );
            assert_eq!(
                palette.pane_close_glyph_on_pill, palette.tab_close_glyph_on_pill_over_active_tab,
                "so are the two pills, and so is the ink that stands on them"
            );
            // The lit `×` has to be a real step above the resting one, or the
            // hover says nothing.
            assert_ne!(palette.pane_close_glyph, palette.pane_close_glyph_on_pill);
            assert_ne!(palette.pane_close_pill, palette.pane_head);
        }
    }

    /// PIN (U1/E57): the termhost floor is `--panel`, and it is not the pane's
    /// own surface.
    ///
    /// A1's whole note is that this colour is invisible until a divider drag
    /// opens the gap F63 draws. Which means nothing on screen can catch it being
    /// wrong — only this can.
    #[test]
    fn the_termhost_floor_is_panel_and_parts_from_the_pane_surface() {
        assert_eq!(DARK_CHROME.termhost, [0x25, 0x25, 0x25], "--panel");
        assert_eq!(LIGHT_CHROME.termhost, [0xf7, 0xf7, 0xf5]);
        for palette in [DARK_CHROME, LIGHT_CHROME] {
            assert_eq!(
                palette.termhost, palette.title_bar,
                "one `--panel`, two declarations"
            );
            assert_ne!(
                palette.termhost, palette.pane_head,
                "a gap that matched the pane it opened between would not be a gap"
            );
        }
    }

    /// PIN (U11/B15): a files or preview body is `--termbg`, never `--panel`.
    ///
    /// The mock-up's own ruling (466-480) and the one place the pane rebuild can
    /// be undone by a single hex digit: painting a files pane `--panel` makes it
    /// read as "a fourth slab of chrome that leaked into the split", visually
    /// glued to the tab rail above it — which is the reading panes exist to kill.
    /// The two greys are four levels apart on dark (`#1B1B1B` against `#252525`)
    /// and that is the whole of the difference.
    ///
    /// Red gate: light hides this the way it hides every surface question —
    /// `--termbg` is `#FFFFFF` and `--panel` is `#F7F7F5`, close enough that a
    /// wrong value looks merely a little warm. So the dark canvas carries the
    /// inequality and both canvases carry the value, and the companion
    /// assertion is that this body is exactly the head that sits on it: one
    /// terminal surface, two declarations, no seam where a pane meets its brow.
    #[test]
    fn a_non_terminal_pane_body_is_the_terminal_surface_and_not_panel_chrome() {
        assert_eq!(DARK_CHROME.seat_body, [0x1b, 0x1b, 0x1b], "--termbg");
        assert_eq!(LIGHT_CHROME.seat_body, [0xff, 0xff, 0xff]);
        for palette in [DARK_CHROME, LIGHT_CHROME] {
            assert_eq!(
                palette.seat_body, palette.pane_head,
                "a pane's body and its head are one surface"
            );
            assert_ne!(
                palette.seat_body, palette.termhost,
                "a files pane painted `--panel` is chrome that leaked into the split"
            );
        }
    }

    /// PIN: the pane head's four tokens composite over `--termbg`, and nothing
    /// else.
    ///
    /// `.panehead` (mock-up 1498-1506) sits on the terminal surface, not on
    /// chrome — `pane_head` says so in its own value — so every ink and edge
    /// that lands on it is mixed over `--termbg`: `#1B1B1B` on dark, `#FFFFFF`
    /// on light. That single fact is the whole test, because the light canvas
    /// hides its violation: there `--termbg`, `--win` and `--menu` are all
    /// white, so an ink mixed over the wrong one of the three still comes out
    /// right, and only the dark canvas can tell them apart.
    ///
    /// Red gate: `pane_title` carried `0x75` on dark, which is `--ink3` over
    /// `--win #202020` — `dialog_muted_text`'s value, correct for the settings
    /// dialog and three levels too pale for a terminal. It reached this field
    /// by way of the light theme, where the two genuinely are one number, and
    /// no test had ever asked the dark pair to differ. This is the error
    /// `menu_item_hint_text` was split out to prevent, caught a second time in
    /// a second family; the assertions below therefore pin each dark composite
    /// to its arithmetic *and* assert that the two names part on dark.
    #[test]
    fn the_pane_heads_inks_are_mixed_over_the_terminal_and_not_over_chrome() {
        // The surface, stated by the palette itself.
        assert_eq!(DARK_CHROME.pane_head, [0x1b, 0x1b, 0x1b], "--termbg");
        assert_eq!(LIGHT_CHROME.pane_head, [0xff, 0xff, 0xff]);

        // --border-soft rgba(255,255,255,.06) over #1B1B1B:
        //   27 + 228 * .06 = 40.68 -> 41 = 0x29.
        assert_eq!(DARK_CHROME.pane_head_edge, [0x29, 0x29, 0x29]);
        // --ink3 rgba(255,255,255,.38) over #1B1B1B:
        //   27 + 228 * .38 = 113.64 -> 114 = 0x72.
        assert_eq!(DARK_CHROME.pane_title, [0x72, 0x72, 0x72]);
        // --ink rgba(255,255,255,.87) over #1B1B1B:
        //   27 + 228 * .87 = 225.36 -> 225 = 0xe1.
        assert_eq!(DARK_CHROME.pane_title_focus, [0xe1, 0xe1, 0xe1]);

        // --ink3 rgba(55,53,47,.45) over #FFFFFF, channel by channel:
        //   255 - 200 * .45 = 165 = 0xa5 / 255 - 202 * .45 = 164.1 -> 0xa4
        //   / 255 - 208 * .45 = 161.4 -> 0xa1.
        assert_eq!(LIGHT_CHROME.pane_title, [0xa5, 0xa4, 0xa1]);
        // --ink is opaque on light, so the focused head is `--ink` itself.
        assert_eq!(LIGHT_CHROME.pane_title_focus, [0x37, 0x35, 0x2f]);

        // The two muted inks are one number on light and must not be on dark:
        // white over white is white, and `--win` is a fifth of a shade lighter
        // than `--termbg` everywhere else.
        assert_eq!(LIGHT_CHROME.pane_title, LIGHT_CHROME.dialog_muted_text);
        assert_ne!(
            DARK_CHROME.pane_title, DARK_CHROME.dialog_muted_text,
            "`--ink3` over `--termbg` and over `--win` are different greys"
        );
    }

    /// PIN (D1): the tab `×`'s ink is mixed over the surface it actually lands
    /// on — all four of them, plus the resting tab's that it shares with the
    /// strip's other muted controls.
    ///
    /// Red gate: the glyph had exactly two inks, `title_text_muted` and
    /// `title_text_hover`, both mixed over `--panel`. The pill *under* it had
    /// already been split into two composites six lines away in the same
    /// function, so on dark a `×` on the active tab was drawn at `0x78` where
    /// its ground asks for `0x72`, and the lit `×` at `0xe3` on both pills
    /// where they ask for `0xe4` and `0xe7`. This is the third catch in the
    /// same family — `menu_item_hint_text`, then `pane_title`, now this — and
    /// each one was a translucent ink that had been mixed once and then reused
    /// wherever the same declaration reappeared.
    #[test]
    fn the_tab_closes_glyph_is_mixed_over_every_surface_a_tab_can_wear() {
        // The grounds, stated by the palette itself: `--panel` under a resting
        // tab, `--termbg` under the active one, and the two pills.
        assert_eq!(DARK_CHROME.title_bar, [0x25, 0x25, 0x25], "--panel");
        assert_eq!(DARK_CHROME.active_tab, [0x1b, 0x1b, 0x1b], "--termbg");
        assert_eq!(DARK_CHROME.tab_close_pill_on_content, [0x30, 0x30, 0x30]);
        assert_eq!(
            DARK_CHROME.tab_close_pill_on_hovered_tab,
            [0x44, 0x44, 0x44]
        );

        // --ink3 rgba(255,255,255,.38) over each:
        //   #252525 -> 37 + 218 * .38 = 119.8 -> 120 = 0x78 (title_text_muted),
        //   #1B1B1B -> 27 + 228 * .38 = 113.6 -> 114 = 0x72,
        //   --hover over --panel is 37 + 218 * .055 = 49.0, and
        //     49.0 + 206 * .38 = 127.3 -> 127 = 0x7f.
        assert_eq!(DARK_CHROME.title_text_muted, [0x78, 0x78, 0x78]);
        assert_eq!(
            DARK_CHROME.tab_close_glyph_on_active_tab,
            [0x72, 0x72, 0x72]
        );
        assert_eq!(
            DARK_CHROME.tab_close_glyph_on_hovered_tab,
            [0x7f, 0x7f, 0x7f]
        );
        // --ink rgba(255,255,255,.87) over the pills' own composites, 47.5 and
        // 67.5 before rounding:
        //   47.5 + 207.5 * .87 = 228.0 -> 0xe4,
        //   67.5 + 187.5 * .87 = 230.6 -> 231 = 0xe7.
        assert_eq!(
            DARK_CHROME.tab_close_glyph_on_pill_over_active_tab,
            [0xe4, 0xe4, 0xe4]
        );
        assert_eq!(
            DARK_CHROME.tab_close_glyph_on_pill_over_hovered_tab,
            [0xe7, 0xe7, 0xe7]
        );

        // --ink3 rgba(55,53,47,.45) over #FFFFFF and over --hover-on-#F7F7F5.
        assert_eq!(
            LIGHT_CHROME.tab_close_glyph_on_active_tab,
            [0xa5, 0xa4, 0xa1]
        );
        assert_eq!(
            LIGHT_CHROME.tab_close_glyph_on_hovered_tab,
            [0x9b, 0x9a, 0x96]
        );
        // --ink is opaque on light, so the lit `×` is that literal everywhere.
        for lit in [
            LIGHT_CHROME.tab_close_glyph_on_pill_over_active_tab,
            LIGHT_CHROME.tab_close_glyph_on_pill_over_hovered_tab,
            LIGHT_CHROME.title_text_hover,
        ] {
            assert_eq!(lit, [0x37, 0x35, 0x2f], "`--ink` #37352F, unmixed");
        }

        // The whole point of the split, asserted where it can fail: on dark the
        // four are four different greys, and none of them is the `--panel` mix
        // the glyph used to wear on every one of them.
        let dark = [
            DARK_CHROME.title_text_muted,
            DARK_CHROME.tab_close_glyph_on_active_tab,
            DARK_CHROME.tab_close_glyph_on_hovered_tab,
            DARK_CHROME.tab_close_glyph_on_pill_over_active_tab,
            DARK_CHROME.tab_close_glyph_on_pill_over_hovered_tab,
        ];
        for (i, left) in dark.iter().enumerate() {
            for right in &dark[i + 1..] {
                assert_ne!(left, right, "five grounds, five composites on dark");
            }
        }
        assert_ne!(
            DARK_CHROME.title_text_hover, DARK_CHROME.tab_close_glyph_on_pill_over_active_tab,
            "`--ink` over `--panel` is not `--ink` over the pill"
        );

        // A pane head is the terminal surface and so is the active tab, so the
        // same ink over the same ground must agree — on both canvases.
        assert_eq!(
            DARK_CHROME.tab_close_glyph_on_active_tab, DARK_CHROME.pane_title,
            "one ink, one ground, one grey"
        );
        assert_eq!(
            LIGHT_CHROME.tab_close_glyph_on_active_tab,
            LIGHT_CHROME.pane_title
        );
    }

    /// PIN (D1, the pin's half): `.tab .pin.on { color: var(--ink) }` is the one
    /// ink the `×` cannot lend the pin, and it is mixed over the bare tab.
    ///
    /// The pin stands in the `×`'s slot and shares its two declarations, so the
    /// muted tier and the two pill mixes are literally the `×`'s fields. What
    /// does not carry across is the *state* tier: the `×` only ever reaches
    /// `--ink` under the pointer, where a pill is always beneath it, while a
    /// pinned pin reaches `--ink` standing on nothing but the tab.
    ///
    /// Red gate: the pin drew both of its tiers from `title_text_hover` and
    /// `title_text_muted`, one pair of `--panel` mixes, on all five grounds —
    /// the same fault as the `×`'s, in the same slot, six lines away. Reusing
    /// the `×`'s *pill* entries for the state tier is the near-miss this test
    /// exists to catch: on the hovered tab the two agree by rounding, so a
    /// wrong reading is invisible there and three levels out on the active tab.
    #[test]
    fn a_pinned_pins_ink_is_mixed_over_the_bare_tab_and_not_over_a_pill() {
        // --ink rgba(255,255,255,.87) over the three tab surfaces:
        //   --panel  #252525 -> 37 + 218 * .87 = 226.7 -> 227 = 0xe3,
        //   --hover over --panel is 49.0, and 49.0 + 206 * .87 = 228.2 -> 0xe4,
        //   --termbg #1B1B1B -> 27 + 228 * .87 = 225.4 -> 225 = 0xe1.
        assert_eq!(DARK_CHROME.title_text_hover, [0xe3, 0xe3, 0xe3]);
        assert_eq!(DARK_CHROME.tab_pin_state_on_hovered_tab, [0xe4, 0xe4, 0xe4]);
        assert_eq!(DARK_CHROME.tab_pin_state_on_active_tab, [0xe1, 0xe1, 0xe1]);
        for lit in [
            LIGHT_CHROME.tab_pin_state_on_active_tab,
            LIGHT_CHROME.tab_pin_state_on_hovered_tab,
        ] {
            assert_eq!(lit, [0x37, 0x35, 0x2f], "opaque `--ink` on light");
        }

        // The near-miss, stated both ways: the bare active tab is three levels
        // off its own pill, and the hovered pair agree only because both round
        // to 228 — so the equality is asserted as the coincidence it is, and
        // the inequality as the fault it would hide.
        assert_ne!(
            DARK_CHROME.tab_pin_state_on_active_tab,
            DARK_CHROME.tab_close_glyph_on_pill_over_active_tab,
            "a pinned pin has no pill under it"
        );
        assert_eq!(
            DARK_CHROME.tab_pin_state_on_hovered_tab,
            DARK_CHROME.tab_close_glyph_on_pill_over_active_tab,
            "228.2 and 228.0 land on the same byte — a coincidence, not a rule"
        );

        // And the same ink over the same ground as a focused pane head, which
        // is the terminal surface the active tab also wears.
        assert_eq!(
            DARK_CHROME.tab_pin_state_on_active_tab, DARK_CHROME.pane_title_focus,
            "one ink, one ground, one grey"
        );
        assert_eq!(
            LIGHT_CHROME.tab_pin_state_on_active_tab,
            LIGHT_CHROME.pane_title_focus
        );
    }

    /// PIN (settings dialog): every token the modal wears is the mock-up's own
    /// value, composited over the surface the mock-up actually puts it on.
    ///
    /// The numbers were read out of `design/ui-mockup.html`'s own renderer, one
    /// composite per row: `--ink/2/3` and `--hover` over `--win` for the dialog,
    /// `--ink`/`--ink2`/`--hover` over `--menu` for its popup. Three surfaces
    /// have to stay apart on dark and the first assertion is that they do —
    /// `--termbg #1B1B1B`, `--win #202020`, `--menu #2A2A2A`.
    ///
    /// Red gate: reuse one surface's ink on another and the composites collide;
    /// each pair below is a different grey, and on the light canvas — where the
    /// mock-up genuinely gives all three surfaces `#FFFFFF` — they are asserted
    /// equal instead, so the test cannot be satisfied by picking one at random.
    #[test]
    fn the_dialogs_tokens_are_the_mock_ups_own_composites() {
        assert_eq!(DARK_CHROME.dialog_surface, [0x20, 0x20, 0x20], "--win");
        assert_eq!(LIGHT_CHROME.dialog_surface, [0xff, 0xff, 0xff]);
        // Three planes, three greys — a dialog is not a flyout is not a terminal.
        assert_ne!(DARK_CHROME.dialog_surface, DARK_CHROME.menu_surface);
        assert_ne!(DARK_CHROME.dialog_surface, DARK_CHROME.seat_body);
        assert_eq!(LIGHT_CHROME.dialog_surface, LIGHT_CHROME.menu_surface);

        // --ink .87 / --ink2 .55 / --ink3 .38 white over #202020.
        assert_eq!(DARK_CHROME.dialog_title_text, [0xe2, 0xe2, 0xe2]);
        assert_eq!(DARK_CHROME.dialog_secondary_text, [0x9b, 0x9b, 0x9b]);
        assert_eq!(DARK_CHROME.dialog_muted_text, [0x75, 0x75, 0x75]);
        // --ink is opaque on light; --ink2/.65 and --ink3/.45 over #FFFFFF.
        assert_eq!(LIGHT_CHROME.dialog_title_text, [0x37, 0x35, 0x2f]);
        assert_eq!(LIGHT_CHROME.dialog_secondary_text, [0x7d, 0x7c, 0x78]);
        assert_eq!(LIGHT_CHROME.dialog_muted_text, [0xa5, 0xa4, 0xa1]);

        // --hover .055 over --win, and the same alpha over --menu, which is a
        // different grey on dark and therefore a different hover.
        assert_eq!(DARK_CHROME.dialog_hover, [0x2c, 0x2c, 0x2c]);
        assert_eq!(DARK_CHROME.menu_item_hover, [0x36, 0x36, 0x36]);
        assert_ne!(DARK_CHROME.dialog_hover, DARK_CHROME.menu_item_hover);
        assert_eq!(LIGHT_CHROME.dialog_hover, [0xf4, 0xf4, 0xf4]);
        assert_eq!(LIGHT_CHROME.menu_item_hover, [0xf4, 0xf4, 0xf4]);

        // `.combo-item` is --ink2 and `.combo-item.selected` is --ink, both over
        // --menu: the selected one is the one that reads at full strength.
        assert_eq!(DARK_CHROME.menu_item_text, [0x9f, 0x9f, 0x9f]);
        assert_eq!(DARK_CHROME.menu_item_text_selected, [0xe3, 0xe3, 0xe3]);
        assert_eq!(LIGHT_CHROME.menu_item_text, [0x7d, 0x7c, 0x78]);
        assert_eq!(LIGHT_CHROME.menu_item_text_selected, [0x37, 0x35, 0x2f]);

        for palette in [DARK_CHROME, LIGHT_CHROME] {
            // `.overlay { background: rgba(15,15,15,.35) }` — declared once, and
            // never overridden on dark. .35 × 255 = 89.25.
            assert_eq!(palette.modal_scrim, [0x0f, 0x0f, 0x0f]);
            assert_eq!(palette.modal_scrim_alpha, 89);
            // `.combo-menu { box-shadow: 0 10px 28px rgba(0,0,0,.18) }`, split
            // into the same two rings, and likewise not theme-varied.
            assert_eq!(palette.menu_popup_shadow_inner_alpha, 46);
            assert_eq!(palette.menu_popup_shadow_outer_alpha, 23);
        }
        assert_eq!(
            DARK_CHROME.modal_scrim, LIGHT_CHROME.modal_scrim,
            "a scrim is not a surface of either palette"
        );
    }

    /// PIN — a scheme change advances the one revision every palette-keyed
    /// artefact is invalidated by, and a `BT_BG` lock keeps its canvas while
    /// still repainting.
    ///
    /// Against a *local* `ThemeState`, like every other test in this module:
    /// the process-wide one is read by every palette assertion here, so a test
    /// that moved it would be a test that broke its neighbours on a thread
    /// schedule.
    #[test]
    fn adopting_schemes_advances_the_revision_and_respects_the_environment_lock() {
        let state = ThemeState::new(Theme::Dark, DEFAULT_BACKGROUND_RGB, false);
        let before = unpack_revision(state.load());
        state.refresh();
        assert!(unpack_revision(state.load()) > before);
        assert_eq!(
            unpack_theme(state.load()),
            Theme::Dark,
            "the mode does not move"
        );

        let locked = ThemeState::from_environment(Some(OsStr::new("#123456")), false);
        let canvas = unpack_background(locked.load());
        let revision = unpack_revision(locked.load());
        locked.refresh();
        assert_eq!(
            unpack_background(locked.load()),
            canvas,
            "`BT_BG` owns the canvas; that is the whole of what the lock means"
        );
        assert!(
            unpack_revision(locked.load()) > revision,
            "the chrome still re-derives, so every window still has to repaint"
        );
    }

    /// PIN — the process is born wearing Folio's own pair, and asking for the
    /// pair already in force costs nothing.
    ///
    /// Read-only by construction: `Unchanged` is the assertion *and* the reason
    /// this may touch the process-wide store where the test above may not.
    #[test]
    fn the_process_is_born_wearing_folios_own_pair() {
        assert_eq!(
            set_schemes(FOLIO_LIGHT, FOLIO_DARK),
            ThemeChange::Unchanged,
            "and so a settings write that did not touch the schemes repaints nothing"
        );
        assert_eq!(scheme_for_theme(Theme::Dark), FOLIO_DARK);
        assert_eq!(scheme_for_theme(Theme::Light), FOLIO_LIGHT);
        assert_eq!(ansi_16_rgb_for(Theme::Dark), DARK_ANSI_16_RGB);
        assert_eq!(ansi_16_rgb_for(Theme::Light), LIGHT_ANSI_16_RGB);
    }

    /// A pair of schemes derives a pair of palettes, each from its own half.
    #[test]
    fn each_half_of_a_pair_derives_from_its_own_scheme() {
        let nord = ColourScheme {
            background: [0x2e, 0x34, 0x40],
            ..FOLIO_DARK
        };
        let schemes = ActiveSchemes::new(FOLIO_LIGHT, nord);
        assert_eq!(schemes.dark_chrome.seat_body, nord.background);
        assert_eq!(schemes.light_chrome, LIGHT_CHROME);
        assert_ne!(schemes.dark_chrome, DARK_CHROME);
    }

    #[test]
    fn runtime_switch_changes_all_readings_and_revision_is_monotonic() {
        let state = ThemeState::new(Theme::Dark, DEFAULT_BACKGROUND_RGB, false);
        let readings = |packed| {
            let background = unpack_background(packed);
            (
                background,
                foreground_for_background(background),
                chrome_palette_for_background(background),
                unpack_revision(packed),
            )
        };
        let dark = readings(state.load());
        assert_eq!(state.set(Theme::Light), ThemeChange::Changed);
        let light = readings(state.load());
        assert_ne!(dark.0, light.0);
        assert_ne!(dark.1, light.1);
        assert_ne!(dark.2, light.2);
        assert!(light.3 > dark.3);

        assert_eq!(state.set(Theme::Dark), ThemeChange::Changed);
        let dark_again = readings(state.load());
        assert_eq!(dark_again.0, dark.0);
        assert!(dark_again.3 > light.3);
        assert_eq!(state.set(Theme::Dark), ThemeChange::Unchanged);
        assert_eq!(readings(state.load()), dark_again);
    }

    #[test]
    fn valid_bt_bg_overrides_and_locks_while_invalid_values_keep_dark_unlocked() {
        let locked = ThemeState::from_environment(Some(OsStr::new("#123456")), false);
        let before = locked.load();
        assert_eq!(unpack_background(before), [0x12, 0x34, 0x56]);
        assert_ne!(before & LOCKED_BIT, 0);
        assert_eq!(locked.set(Theme::Light), ThemeChange::LockedByEnvironment);
        assert_eq!(locked.load(), before);

        let invalid = ThemeState::from_environment(Some(OsStr::new("123456")), false);
        assert_eq!(unpack_background(invalid.load()), DEFAULT_BACKGROUND_RGB);
        assert_eq!(invalid.load() & LOCKED_BIT, 0);
        assert_eq!(invalid.set(Theme::Light), ThemeChange::Changed);
        assert_eq!(unpack_background(invalid.load()), LIGHT_BACKGROUND_RGB);
    }
}
