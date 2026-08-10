//! Runtime-selectable built-in terminal and chrome colors, without changing the renderer's
//! distinction between default colors and explicit ANSI palette colors.

use std::{
    ffi::OsStr,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

/// The product's terminal defaults, from `design/ui-mockup.html` (the approved
/// styling): dark `--termbg #1B1B1B`, ink `rgba(255,255,255,.87)` composited
/// over it, light ink `--ink #37352F`. Explicit ANSI colors remain distinct
/// from these defaults and use the palette selected by the current theme.
pub const DEFAULT_BACKGROUND_RGB: [u8; 3] = [0x1b, 0x1b, 0x1b];
pub const LIGHT_BACKGROUND_RGB: [u8; 3] = [0xff, 0xff, 0xff];
pub(crate) const DEFAULT_FOREGROUND_RGB: [u8; 3] = [0xe1, 0xe1, 0xe1];
const LIGHT_BACKGROUND_FOREGROUND_RGB: [u8; 3] = [0x37, 0x35, 0x2f];
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
    if background_is_light(background) {
        LIGHT_CURSOR_RGB
    } else {
        DEFAULT_CURSOR_RGB
    }
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

/// `ink` at [`UNFOCUSED_CURSOR_ALPHA_PERCENT`] over `canvas`, composited in sRGB
/// and rounded half away from zero.
///
/// The pre-composition is the convention [`ChromePalette`] documents; doing it
/// here rather than in a comment means the alpha and the bytes cannot drift
/// apart, and the compiler still hands the pipeline a plain opaque colour.
const fn cursor_ink_faded_over(canvas: [u8; 3], ink: [u8; 3]) -> [u8; 3] {
    let mut faded = [0u8; 3];
    let mut channel = 0;
    while channel < 3 {
        let base = canvas[channel] as i32;
        let scaled = (ink[channel] as i32 - base) * UNFOCUSED_CURSOR_ALPHA_PERCENT;
        let step = if scaled >= 0 {
            (scaled + 50) / 100
        } else {
            (scaled - 50) / 100
        };
        faded[channel] = (base + step) as u8;
        channel += 1;
    }
    faded
}

/// [`DEFAULT_CURSOR_RGB`] faded over `--termbg #1B1B1B`:
/// 27 + (212 − 27) × .45 = 110.25 → #6E6E6E.
pub(crate) const DEFAULT_UNFOCUSED_CURSOR_RGB: [u8; 3] =
    cursor_ink_faded_over(DEFAULT_BACKGROUND_RGB, DEFAULT_CURSOR_RGB);
/// [`LIGHT_CURSOR_RGB`] faded over `--termbg #FFFFFF`:
/// 255 − (255 − 55) × .45 = 165 and its two companions → #A5A4A1.
pub(crate) const LIGHT_UNFOCUSED_CURSOR_RGB: [u8; 3] =
    cursor_ink_faded_over(LIGHT_BACKGROUND_RGB, LIGHT_CURSOR_RGB);

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
    if background_is_light(background) {
        LIGHT_UNFOCUSED_CURSOR_RGB
    } else {
        DEFAULT_UNFOCUSED_CURSOR_RGB
    }
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
    if background_is_light(background) {
        LIGHT_SELECTION_BACKGROUND_RGB
    } else {
        DEFAULT_SELECTION_BACKGROUND_RGB
    }
}

/// The selection fill in force, read through the same atomic snapshot as the
/// background it sits on, so a runtime theme switch changes it on the next frame.
pub(crate) fn selection_background_rgb() -> [u8; 3] {
    selection_background_for_background(background_rgb())
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

    // ── Status semantics (mock-up lines 28-35) ──
    //
    // The mock-up declares these in `:root` with a comment that rules them
    // explicitly: "every 'something happened' colour goes through these four,
    // never a literal". `body.dark` overrides `--accent` and leaves these
    // alone, so three of the four are one set shared by both canvases and the
    // fourth is [`Self::accent`]. They are opaque hex in the design, so unlike
    // most of this palette there is nothing to pre-composite — they land as
    // written.
    /// `--err #c50f1f` — a session that finished with a failing exit code, worn
    /// by the tab's dot and by a progress ring reporting `OSC 9;4` state 2.
    pub status_err: [u8; 3],
    /// `--warn #d9822b` — the bell, and (once the attention queue lands) an
    /// agent blocked on you.
    pub status_warn: [u8; 3],
    /// `--pause #c19c00` — a progress ring reporting `OSC 9;4` state 4.
    pub status_pause: [u8; 3],

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
    // The mock-up's status semantics live in `:root` and `body.dark` overrides
    // none of them, so the dark canvas wears the same three literals.
    status_err: [0xc5, 0x0f, 0x1f],
    status_warn: [0xd9, 0x82, 0x2b],
    status_pause: [0xc1, 0x9c, 0x00],
    // `--border` (white at .094) at `opacity: .7` — .0658 white — over
    // `--termbg` #1B1B1B, `--panel` #252525, and `--hover`-over-`--panel`
    // #313131 respectively.
    ring_track_on_active_tab: [0x2a, 0x2a, 0x2a],
    ring_track_on_resting_tab: [0x33, 0x33, 0x33],
    ring_track_on_hovered_tab: [0x3f, 0x3f, 0x3f],
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
    // `--ink2` (the same ink at .65) over the resting and hovered fills.
    tab_badge_text_on_resting_tab: [0x74, 0x73, 0x6e],
    tab_badge_text_on_hovered_tab: [0x71, 0x6f, 0x6b],
    // `--ink3` (the ink at .45) over `--menu` #FFFFFF — which in this theme is
    // the same white as `--win`, so it agrees with `dialog_muted_text` exactly.
    menu_item_hint_text: [0xa5, 0xa4, 0xa1],
    // Opaque in the mock-up's `:root`, and not overridden by either canvas.
    status_err: [0xc5, 0x0f, 0x1f],
    status_warn: [0xd9, 0x82, 0x2b],
    status_pause: [0xc1, 0x9c, 0x00],
    // `--border` (black at .088) at `opacity: .7` — .0616 black — over
    // `--termbg` #FFFFFF, `--panel` #F7F7F5, and `--hover`-over-`--panel`
    // #ECECEA respectively.
    ring_track_on_active_tab: [0xef, 0xef, 0xef],
    ring_track_on_resting_tab: [0xe8, 0xe8, 0xe6],
    ring_track_on_hovered_tab: [0xdd, 0xdd, 0xdc],
};

/// The palette in force, decided by the same background-luma threshold that
/// already chooses the terminal's default ink — one dark/light decision for the
/// whole product, never two.
pub fn chrome_palette() -> ChromePalette {
    chrome_palette_for_background(background_rgb())
}

fn chrome_palette_for_background(background: [u8; 3]) -> ChromePalette {
    if background_is_light(background) {
        LIGHT_CHROME
    } else {
        DARK_CHROME
    }
}

/// A seat title bar's height, in logical pixels (`.panehead { height: 28px }`).
pub const SEAT_TITLE_BAR_LOGICAL_PX: f32 = 28.0;
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

    const fn background(self) -> [u8; 3] {
        match self {
            Self::Dark => DEFAULT_BACKGROUND_RGB,
            Self::Light => LIGHT_BACKGROUND_RGB,
        }
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

fn background_is_light(background: [u8; 3]) -> bool {
    let background_luma = u32::from(background[0]) * 299
        + u32::from(background[1]) * 587
        + u32::from(background[2]) * 114;
    background_luma >= 128_000
}

fn foreground_for_background(background: [u8; 3]) -> [u8; 3] {
    if background_is_light(background) {
        LIGHT_BACKGROUND_FOREGROUND_RGB
    } else {
        DEFAULT_FOREGROUND_RGB
    }
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
const DARK_ANSI_16_RGB: [[u8; 3]; 16] = [
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
const LIGHT_ANSI_16_RGB: [[u8; 3]; 16] = [
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
pub(crate) fn ansi_16_rgb() -> &'static [[u8; 3]; 16] {
    ansi_16_rgb_for(current_theme())
}

const fn ansi_16_rgb_for(theme: Theme) -> &'static [[u8; 3]; 16] {
    match theme {
        Theme::Dark => &DARK_ANSI_16_RGB,
        Theme::Light => &LIGHT_ANSI_16_RGB,
    }
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
        assert_eq!(ansi_16_rgb_for(Theme::Dark), &CAMPBELL);
        assert_eq!(ansi_16_rgb_for(Theme::Light), &MAC_TERMINAL);
    }

    /// PIN: the selection fill is a *default* colour, so it follows the same
    /// background-luma switch that already chooses the terminal's own ink —
    /// never one value for both canvases.
    ///
    /// Dark keeps the value it shipped with; light is the mock-up's `--accent`
    /// #3059D8 at the 30% its own in-terminal highlight (`mark.srch`) uses,
    /// composited over the light `--termbg` #FFFFFF in sRGB the way every other
    /// translucent-over-known-surface colour in this file is pre-composited.
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

    /// PIN (T2 tab status): the four "something happened" colours are the
    /// mock-up's own status semantics, declared once in its `:root` (lines
    /// 30-35) and never overridden by `body.dark`. Both palettes therefore
    /// carry the *same* three literals — a theme split here would be an
    /// invention, not a reading, and this test is what forbids one.
    #[test]
    fn the_status_colours_are_one_set_shared_by_both_canvases() {
        for palette in [DARK_CHROME, LIGHT_CHROME] {
            assert_eq!(palette.status_err, [0xc5, 0x0f, 0x1f], "--err");
            assert_eq!(palette.status_warn, [0xd9, 0x82, 0x2b], "--warn");
            assert_eq!(palette.status_pause, [0xc1, 0x9c, 0x00], "--pause");
        }
        // The accent is the fourth claim ("finished, unread") and is the one
        // that *does* vary by canvas, so the dot's four colours are never a
        // single constant table.
        assert_ne!(DARK_CHROME.accent, LIGHT_CHROME.accent);
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
