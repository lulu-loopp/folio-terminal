//! A terminal colour scheme, and the chrome derived from it.
//!
//! # What a scheme is, and what it is not
//!
//! A scheme is **twenty-one colours**: the ANSI sixteen, plus a background, a
//! foreground, a caret, a selection fill and an accent (user ruling,
//! 2026-08-17, Q11 = A). That is the whole of it. The window's own chrome —
//! the ~139 fields of [`ChromePalette`] — is **derived** from those twenty-one
//! by [`ChromePalette::derive`] and stored nowhere, which is the difference
//! between a colour scheme and a skin: nobody has to name a divider, and no
//! scheme can produce a window whose tab strip disagrees with its terminal.
//!
//! Two things a scheme deliberately does not reach. **Profile identity marks**
//! keep their own colours (`UI-UX.md`: "you recognise PowerShell by that blue,
//! not by reading a word"), because a mark that changes with the palette is not
//! an identity. And the **status four**, the commit graph's eight lanes and the
//! seven syntax inks keep their fixed hues on each canvas: each was struck
//! against a contrast floor on *both* surfaces it can land on, and a red that
//! means "this failed" is not a colour a scheme gets a vote on.
//!
//! # High contrast
//!
//! `docs/DESIGN.md` §7.1.6 rules that the product follows the system palette in
//! high-contrast mode, and the ruling of 2026-08-17 says a custom scheme yields
//! to the system there. **Following high contrast is not implemented in this
//! build** — nothing in the tree reads `SPI_GETHIGHCONTRAST` — so this slice
//! does not invent it. What it does is leave exactly one door: every chrome
//! colour in the product comes out of [`ChromePalette::derive`] and every
//! terminal default out of the scheme handed to
//! [`crate::theme::set_schemes`], so a high-contrast reading enters by
//! substituting the scheme at that one call and by nowhere else.

use crate::theme::{
    ChromePalette, DARK_ACCENT_RGB, DARK_ANSI_16_RGB, DEFAULT_BACKGROUND_RGB, DEFAULT_CURSOR_RGB,
    DEFAULT_FOREGROUND_RGB, DEFAULT_SELECTION_BACKGROUND_RGB, GRAPH_LANE_COUNT, GRAPH_LANES_DARK,
    GRAPH_LANES_LIGHT, LIGHT_ACCENT_RGB, LIGHT_ANSI_16_RGB, LIGHT_BACKGROUND_FOREGROUND_RGB,
    LIGHT_BACKGROUND_RGB, LIGHT_CURSOR_RGB, LIGHT_SELECTION_BACKGROUND_RGB, background_is_light,
    ink_over, ink_over_bp,
};

/// The twenty-one colours a terminal colour scheme is.
///
/// Nameless on purpose: a name is how a *file* is identified, and the app owns
/// the registry of files. What crosses into the renderer is only the colours,
/// which keeps this `Copy` and keeps the derivation a pure function of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColourScheme {
    /// The terminal canvas, and the canvas every chrome surface is a step off.
    pub background: [u8; 3],
    /// Default ink. Also the top of the chrome's ink ladder — see
    /// [`ChromePalette::derive`] for how the ladder's source is recovered
    /// from it.
    pub foreground: [u8; 3],
    /// The caret's own ink.
    pub cursor: [u8; 3],
    /// The selection fill, background-only; foreground colours stay
    /// terminal-authored.
    pub selection: [u8; 3],
    /// ANSI 0..=15, normal then bright.
    pub ansi: [[u8; 3]; 16],
    /// What links, focus rings, the dragged divider and a search's current hit
    /// are drawn in. Windows Terminal's format has no such key; a file without
    /// one is read with its `blue` here.
    pub accent: [u8; 3],
}

/// This product's dark scheme — the colours every build before schemes existed
/// drew, to the byte.
pub const FOLIO_DARK: ColourScheme = ColourScheme {
    background: DEFAULT_BACKGROUND_RGB,
    foreground: DEFAULT_FOREGROUND_RGB,
    cursor: DEFAULT_CURSOR_RGB,
    selection: DEFAULT_SELECTION_BACKGROUND_RGB,
    ansi: DARK_ANSI_16_RGB,
    accent: DARK_ACCENT_RGB,
};

/// This product's light scheme, on the same terms.
pub const FOLIO_LIGHT: ColourScheme = ColourScheme {
    background: LIGHT_BACKGROUND_RGB,
    foreground: LIGHT_BACKGROUND_FOREGROUND_RGB,
    cursor: LIGHT_CURSOR_RGB,
    selection: LIGHT_SELECTION_BACKGROUND_RGB,
    ansi: LIGHT_ANSI_16_RGB,
    accent: LIGHT_ACCENT_RGB,
};

/// Everything the design says about a canvas that is *not* a colour of the
/// scheme standing on it.
///
/// Two of these exist and the mock-up is why: the design declares its tokens
/// twice, once in `:root` and once in `body.dark`, and the two are not one
/// declaration with different operands. On night `--ink` is `rgba(255,255,255,
/// .87)` and on paper it is the opaque `#37352F`; normalised against their own
/// `--ink` the two ladders are .632/.437 and .650/.450, three levels apart over
/// a 198-level range, so one ladder cannot reproduce both palettes and the
/// asymmetry is the design's rather than an accident to be smoothed away.
struct Canvas {
    /// `--ink`, `--ink2`, `--ink3` as thousandths of the ink source.
    ink: i32,
    ink2: i32,
    ink3: i32,
    /// `--win`, `--panel`, `--menu` as **signed steps off the canvas**, in
    /// levels.
    ///
    /// Steps and not destinations, because the destinations are not derivable:
    /// `--panel #F7F7F5` on paper is `#FFFFFF` moved by (8, 8, 10), and the
    /// ink's own distance from white is (200, 202, 208) — proportions
    /// 1 : 1.01 : 1.04 against the 1 : 1 : 1.25 that would be needed. It is a
    /// struck warm grey, so what carries onto another scheme is the step it
    /// represents, which keeps both the plane separation and the warmth.
    win: [i16; 3],
    panel: [i16; 3],
    menu: [i16; 3],
    /// `--border` and `--border-soft`, in thousandths of [`Self::shade`].
    border: i32,
    border_soft: i32,
    /// What a hairline is struck in — white on night, black on paper. Not the
    /// ink: `:root` writes `--border: rgba(0,0,0,.088)` while `--hover` on the
    /// same canvas is `rgba(55,53,47,.055)`, and the two are different colours.
    shade: [u8; 3],
    /// `--thumb` and `--thumb-hover`, in thousandths of the ink over
    /// `--termbg`.
    ///
    /// Struck alphas rather than a rung of the ink ladder, because the design
    /// declares them as their own pair and gives paper two hundredths more than
    /// night on both (`.24`/`.42` against `.22`/`.40`): ink laid on paper covers
    /// less ground per unit of alpha than ink laid on night, and folding the two
    /// into one number would leave the light bar the fainter of the two on the
    /// canvas it is already hardest to see against.
    thumb: i32,
    thumb_hover: i32,
    /// A divider under the pointer, in thousandths of the ink over `--win`.
    ///
    /// A struck step rather than a token: night wants .220 and paper .306, and
    /// no source at one alpha reaches both (paper alone needs .305, .3069 and
    /// .3077 on its three channels against the ink).
    divider_hover: i32,
    /// The seven syntax inks, in [`ChromePalette`]'s own field order.
    highlights: [[u8; 3]; 7],
    /// The commit graph's lane wheel.
    lanes: [[u8; 3]; GRAPH_LANE_COUNT],
    /// `--err`, `--warn`, `--pause`, `--ok`.
    status: [[u8; 3]; 4],
    /// `--err-deep`, worn by a failed command tick under the pointer.
    err_deep: [u8; 3],
    /// The five lifts a floating surface can wear — `--shadow`'s, a tooltip's,
    /// a transient float's, a pinned float's and the glance card's — as
    /// (inner, outer) rings in 1/255ths.
    ///
    /// Both rings and not "the outer is half the inner", because the design
    /// rounds each ring from its own CSS alpha rather than from the ring above
    /// it: `.20` and `.10` are 51 and 26, where halving 51 gives 25, and `.45`
    /// and `.225` are 115 and 57, where halving 115 gives 58. Two of the ten
    /// pairs disagree with the shortcut in opposite directions, which is the
    /// clearest possible sign that the shortcut is not the rule.
    shadows: [(u8, u8); 5],
    /// The rail's cast shade, in 1/255ths.
    rail_shade: u8,
}

/// A dark canvas — `design/ui-mockup.html` `body.dark`.
const NIGHT: Canvas = Canvas {
    ink: 870,
    ink2: 550,
    ink3: 380,
    win: [5, 5, 5],
    panel: [10, 10, 10],
    menu: [15, 15, 15],
    border: 94,
    border_soft: 60,
    shade: [0xff, 0xff, 0xff],
    thumb: 220,
    thumb_hover: 400,
    divider_hover: 220,
    highlights: [
        [0xae, 0x75, 0xd7],
        [0x47, 0x9e, 0x6b],
        [0x82, 0x8f, 0xa1],
        [0xba, 0x83, 0x36],
        [0x45, 0x98, 0xa8],
        [0x6e, 0x8a, 0xdd],
        [0x82, 0x8f, 0xa1],
    ],
    lanes: GRAPH_LANES_DARK,
    status: [
        [0xfb, 0x71, 0x85],
        [0xd9, 0x82, 0x2b],
        [0xc1, 0x9c, 0x00],
        [0x57, 0xab, 0x5a],
    ],
    err_deep: [0xf4, 0x3f, 0x5e],
    shadows: [(46, 23), (115, 57), (128, 64), (148, 74), (128, 64)],
    rail_shade: 87,
};

/// A light canvas — the mock-up's `:root` defaults.
const PAPER: Canvas = Canvas {
    ink: 1000,
    ink2: 650,
    ink3: 450,
    win: [0, 0, 0],
    panel: [-8, -8, -10],
    menu: [0, 0, 0],
    border: 88,
    border_soft: 55,
    shade: [0x00, 0x00, 0x00],
    thumb: 240,
    thumb_hover: 420,
    divider_hover: 306,
    highlights: [
        [0x91, 0x53, 0xbe],
        [0x20, 0x7e, 0x47],
        [0x64, 0x71, 0x84],
        [0xa5, 0x5e, 0x18],
        [0x1b, 0x78, 0x98],
        [0x47, 0x6a, 0xd1],
        [0x64, 0x71, 0x84],
    ],
    lanes: GRAPH_LANES_LIGHT,
    status: [
        [0xe1, 0x1d, 0x48],
        [0xd9, 0x82, 0x2b],
        [0xc1, 0x9c, 0x00],
        [0x1a, 0x7f, 0x37],
    ],
    err_deep: [0xbe, 0x12, 0x3c],
    shadows: [(18, 9), (26, 13), (51, 26), (61, 31), (46, 23)],
    rail_shade: 23,
};

/// `--hover`, the wash a pointer lays on a row, in thousandths.
const HOVER: i32 = 55;
/// `--active`, the wash a selection lays on one.
const ACTIVE: i32 = 90;

/// A surface stepped off its canvas, saturating at both ends.
fn step(canvas: [u8; 3], by: [i16; 3]) -> [u8; 3] {
    let mut out = [0u8; 3];
    for channel in 0..3 {
        out[channel] = (i16::from(canvas[channel]) + by[channel]).clamp(0, 255) as u8;
    }
    out
}

/// The colour that, composited at `alpha` thousandths over `canvas`, gives
/// `result` — [`ink_over`] run backwards.
///
/// The chrome's ink ladder is a fraction of a *source* rather than of the
/// foreground: `--ink3` over `--panel` is the source at .38 over the panel, and
/// the panel is not the background, so knowing only "the foreground" is not
/// enough. What the scheme states is the top of the ladder — the foreground
/// *is* `--ink` over the background — so the source is recovered by inverting
/// that one step. On this product's own dark scheme it comes back as
/// `27 + (225 − 27)/.87 = 254.6 → 255`, which is `rgba(255,255,255,α)`
/// exactly, and every sum below therefore reproduces the struck table.
fn unmix(canvas: [u8; 3], result: [u8; 3], alpha: i32) -> [u8; 3] {
    let mut out = [0u8; 3];
    for channel in 0..3 {
        let base = i32::from(canvas[channel]);
        let delta = i32::from(result[channel]) - base;
        let half = alpha / 2;
        let scaled = if delta >= 0 {
            (delta * 1000 + half) / alpha
        } else {
            (delta * 1000 - half) / alpha
        };
        out[channel] = (base + scaled).clamp(0, 255) as u8;
    }
    out
}

impl ChromePalette {
    /// Every colour the window's chrome wears, computed from a scheme.
    ///
    /// **This is the palette, not a second one.** Two tests
    /// (`the_derivation_reproduces_the_dark_palette_byte_for_byte` and its
    /// light twin) hold this function to [`crate::DARK_CHROME`] and
    /// [`crate::LIGHT_CHROME`] field by field, which is the only thing that
    /// keeps "the chrome follows your scheme" from meaning "there is a second
    /// chrome that mostly agrees with the first". Those two tables stay in the
    /// tree as the pin's expected values and as the record of where every
    /// number came from; nothing at runtime reads them.
    ///
    /// # How it is built
    ///
    /// Six tokens come out of the scheme — the canvas, the three chrome
    /// surfaces stepped off it, the ink source recovered by [`unmix`], and the
    /// accent — and then every field is the sum its own doc comment already
    /// stated, run through [`ink_over`], the one compositor. Nothing here is
    /// new arithmetic; it is the arithmetic that was previously done by hand
    /// and written into two tables.
    ///
    /// # What does not come from the scheme
    ///
    /// The status four, `--err-deep`, the eight graph lanes, the seven syntax
    /// inks, every shadow alpha, the scrim and the destructive close's red.
    /// Each is a signal or a lift rather than a surface, and each was struck
    /// against a contrast floor on both of its grounds; see this module's own
    /// header for why a scheme does not get a vote on them.
    #[must_use]
    pub fn derive(scheme: &ColourScheme) -> Self {
        let canvas: &Canvas = if background_is_light(scheme.background) {
            &PAPER
        } else {
            &NIGHT
        };

        let termbg = scheme.background;
        let win = step(termbg, canvas.win);
        let panel = step(termbg, canvas.panel);
        let menu = step(termbg, canvas.menu);
        let source = unmix(termbg, scheme.foreground, canvas.ink);
        let accent = scheme.accent;

        // `--ink`/`--ink2`/`--ink3` over a named ground, and the two washes.
        let ink = |ground: [u8; 3]| ink_over(ground, source, canvas.ink);
        let ink2 = |ground: [u8; 3]| ink_over(ground, source, canvas.ink2);
        let ink3 = |ground: [u8; 3]| ink_over(ground, source, canvas.ink3);
        let hover = |ground: [u8; 3]| ink_over(ground, source, HOVER);
        let active = |ground: [u8; 3]| ink_over(ground, source, ACTIVE);
        // `--border` and `--border-soft`, which are the shade and not the ink.
        let border = |ground: [u8; 3]| ink_over(ground, canvas.shade, canvas.border);
        let border_soft = |ground: [u8; 3]| ink_over(ground, canvas.shade, canvas.border_soft);

        // The four grounds the strip's `×` can land on, and the tree's three.
        let hovered_tab = hover(panel);
        let pill_on_content = active(termbg);
        let pill_on_hovered_tab = active(hovered_tab);
        let row_hover = hover(termbg);
        let row_selected = active(termbg);
        let float_hover = hover(win);
        let float_selected = active(win);
        let card_hover = hover(panel);
        let act_pill = active(card_hover);
        let rail_active = active(panel);
        let leaf_focus = active(menu);
        // The focus column's four grounds: a card's head at rest and on the
        // stage, and the pill that the pane-count badge and the hovered `×`
        // share on each. `--hover`/`--active` over `--menu` rather than over
        // `--panel`, because a card is an object with its own face and the head
        // is laid on that face.
        let focus_card = hover(menu);
        let focus_card_staged = active(menu);
        let focus_card_pill = active(focus_card);
        let focus_card_pill_staged = active(focus_card_staged);
        // `.pring .track { stroke: var(--border); opacity: .7 }`. Ten-thousandths
        // because `.094 × .7` is .0658 and `.088 × .7` is .0616, and a
        // thousandth rounds the second of those onto the wrong level.
        let ring = |ground: [u8; 3]| ink_over_bp(ground, canvas.shade, canvas.border * 7);
        let [status_err, status_warn, status_pause, status_ok] = canvas.status;
        let [
            hl_keyword,
            hl_string,
            hl_comment,
            hl_number,
            hl_type,
            hl_function,
            hl_punct_muted,
        ] = canvas.highlights;
        let [
            menu_shadow,
            tip_shadow,
            float_shadow,
            float_pinned_shadow,
            peek_shadow,
        ] = canvas.shadows;

        Self {
            seat_body: termbg,
            title_bar: panel,
            title_text: ink2(panel),
            title_text_hover: ink(panel),
            title_text_muted: ink3(panel),
            tab_close_pill_on_content: pill_on_content,
            tab_close_pill_on_hovered_tab: pill_on_hovered_tab,
            tab_close_glyph_on_active_tab: ink3(termbg),
            tab_close_glyph_on_hovered_tab: ink3(hovered_tab),
            tab_close_glyph_on_pill_over_active_tab: ink(pill_on_content),
            tab_close_glyph_on_pill_over_hovered_tab: ink(pill_on_hovered_tab),
            tab_pin_state_on_active_tab: ink(termbg),
            tab_pin_state_on_hovered_tab: ink(hovered_tab),
            body_hint_text: ink3(win),
            preview_body_text: ink(termbg),
            preview_grid_line: border_soft(termbg),
            preview_code_ground: panel,
            preview_code_border: border_soft(panel),
            preview_code_text: ink2(panel),
            preview_code_lang: ink3(panel),
            preview_diff_add: ink_over(termbg, status_ok, 130),
            preview_diff_del: ink_over(termbg, status_err, 100),
            preview_diff_hunk: accent,
            preview_table_head_text: ink(row_hover),
            preview_selection: scheme.selection,
            preview_caret: scheme.cursor,
            hl_keyword,
            hl_string,
            hl_comment,
            hl_number,
            hl_type,
            hl_function,
            hl_punct_muted,
            files_row_hover: row_hover,
            files_row_selected: row_selected,
            files_row_text: ink2(termbg),
            files_row_text_hover: ink2(row_hover),
            files_row_text_selected: ink(row_selected),
            files_row_muted: ink3(termbg),
            files_row_muted_hover: ink3(row_hover),
            files_row_muted_selected: ink3(row_selected),
            float_row_hover: float_hover,
            float_row_selected: float_selected,
            float_row_text: ink2(win),
            float_row_text_hover: ink2(float_hover),
            float_row_text_selected: ink(float_selected),
            float_row_muted: ink3(win),
            float_row_muted_hover: ink3(float_hover),
            float_row_muted_selected: ink3(float_selected),
            git_section: panel,
            git_row_hover: card_hover,
            git_row_selected: row_selected,
            git_row_match: ink_over(termbg, row_selected, 500),
            git_row_text: ink(panel),
            git_row_text_hover: ink(card_hover),
            git_row_muted: ink3(panel),
            git_row_muted_hover: ink3(card_hover),
            git_act_glyph: ink2(panel),
            git_act_glyph_hover: ink2(card_hover),
            git_act_pill: act_pill,
            git_act_glyph_on_pill: ink(act_pill),
            git_head_text: ink(termbg),
            git_head_muted: ink3(termbg),
            git_pill_text: ink2(termbg),
            git_pill_border: border(termbg),
            graph_lanes: canvas.lanes,
            // `--border` over `--win`: the plane that shows through the gap a
            // dragged divider opens. Not over `--panel`, which the mock-up's
            // own cascade would say — that reading gives 57 and 225 where the
            // struck line is 53 and 233, and the line the product draws is the
            // one this has to reproduce.
            divider: border(win),
            divider_hover: ink_over(win, source, canvas.divider_hover),
            divider_active: accent,
            collapse_bar: panel,
            // The neutral shade at `--hover`'s alpha, not the ink at it: on
            // paper the struck bar is (233, 233, 232), which is black at .055
            // over `--panel`, where the ink at .055 would be (236, 236, 234).
            // On night the two sources are the same white, so the canvas that
            // can tell them apart is the one that decides.
            collapse_bar_hover: ink_over(panel, canvas.shade, HOVER),
            caption_hover: hovered_tab,
            // `.capbtn.close-w:hover { background: #E5484D; color: #fff }` —
            // written once, overridden by neither canvas, and a destructive
            // hover is a signal rather than a surface.
            caption_close_hover: [0xe5, 0x48, 0x4d],
            caption_close_text: [0xff, 0xff, 0xff],
            active_tab: termbg,
            pane_head: termbg,
            pane_close_glyph: ink3(termbg),
            pane_close_pill: row_selected,
            pane_close_glyph_on_pill: ink(row_selected),
            termhost: panel,
            pane_head_edge: border_soft(termbg),
            pane_title: ink3(termbg),
            pane_title_focus: ink(termbg),
            accent,
            command_tick: ink3(termbg),
            command_tick_crest: ink_over([0x00, 0x00, 0x00], accent, 860),
            command_tick_fail_crest: canvas.err_deep,
            command_tick_search_crest: ink2(termbg),
            scroll_thumb: ink_over(termbg, source, canvas.thumb),
            scroll_thumb_hover: ink_over(termbg, source, canvas.thumb_hover),
            menu_surface: menu,
            menu_border: canvas.shade,
            menu_border_alpha: ((255 * canvas.border + 500) / 1000) as u8,
            menu_shadow: [0x00, 0x00, 0x00],
            menu_shadow_inner_alpha: menu_shadow.0,
            menu_shadow_outer_alpha: menu_shadow.1,
            // `.combo-menu`'s and `.drag-ghost`'s lifts are each written once
            // and never overridden on dark — see the fields for why the tip's
            // and the float's are, and these two are not.
            menu_popup_shadow_inner_alpha: 46,
            menu_popup_shadow_outer_alpha: 23,
            tip_shadow_inner_alpha: tip_shadow.0,
            tip_shadow_outer_alpha: tip_shadow.1,
            drag_ghost_shadow_inner_alpha: 64,
            drag_ghost_shadow_outer_alpha: 32,
            float_shadow_inner_alpha: float_shadow.0,
            float_shadow_outer_alpha: float_shadow.1,
            float_pinned_shadow_inner_alpha: float_pinned_shadow.0,
            float_pinned_shadow_outer_alpha: float_pinned_shadow.1,
            peek_card_shadow_inner_alpha: peek_shadow.0,
            peek_card_shadow_outer_alpha: peek_shadow.1,
            dialog_surface: win,
            dialog_title_text: ink(win),
            dialog_secondary_text: ink2(win),
            dialog_muted_text: ink3(win),
            dialog_hover: hover(win),
            menu_item_text: ink2(menu),
            menu_item_text_selected: ink(menu),
            menu_item_hover: hover(menu),
            peek_leaf_focus_fill: leaf_focus,
            peek_leaf_focus_edge: ink3(leaf_focus),
            peek_leaf_focus_text: ink(leaf_focus),
            // A scrim is not a surface of either palette, it is the absence of
            // one, so it is written once for both canvases and for every
            // scheme.
            modal_scrim: [0x0f, 0x0f, 0x0f],
            modal_scrim_alpha: 89,
            tab_badge_on_resting_tab: rail_active,
            tab_badge_text_on_active_tab: ink(pill_on_content),
            tab_badge_text_on_resting_tab: ink2(rail_active),
            tab_badge_text_on_hovered_tab: ink2(pill_on_hovered_tab),
            menu_item_hint_text: ink3(menu),
            // Half of `--ink3` over the same ground — the ladder
            // `.pv-nav.off`'s `opacity: .22` walks down, said as a colour
            // because the thing walking it is a run of text. See the field.
            menu_item_unavailable_text: ink_over(menu, source, canvas.ink3 / 2),
            status_err,
            status_warn,
            status_pause,
            status_ok,
            ring_track_on_active_tab: ring(termbg),
            ring_track_on_resting_tab: ring(panel),
            ring_track_on_hovered_tab: ring(hovered_tab),
            rail_tab_active: rail_active,
            rail_tab_active_text: ink(rail_active),
            rail_tab_hover_text: ink2(hovered_tab),
            rail_glyph_on_active_tab: ink3(rail_active),
            rail_seam: border(panel),
            rail_edge: border_soft(panel),
            rail_shade: [0x00, 0x00, 0x00],
            rail_shade_alpha: canvas.rail_shade,
            // The focus column's cards (§7.1.6b′). A card is a tab, so every
            // declaration here is one the strip or the rail already makes; what
            // is new is only the ground, because a card is an object with a
            // `--menu` face while a rail row lies straight on `--panel`.
            focus_card,
            focus_card_staged,
            focus_card_title: ink2(focus_card),
            focus_card_title_staged: ink(focus_card_staged),
            focus_card_glyph: ink3(focus_card),
            focus_card_glyph_staged: ink3(focus_card_staged),
            focus_card_pill,
            focus_card_pill_staged,
            focus_card_ink_on_pill: ink(focus_card_pill),
            focus_card_ink_on_pill_staged: ink(focus_card_pill_staged),
            focus_card_muted_on_pill: ink2(focus_card_pill),
            // The same `--border` on the same `--panel` `rail_seam` is struck
            // from, and written as the same call for that reason.
            focus_card_edge: border(panel),
            ring_track_on_focus_card: ring(focus_card),
            ring_track_on_focus_card_staged: ring(focus_card_staged),
            // The card's body is `--termbg`, so the scheme's own background is
            // what a mini seat's ink and edges are composited over — which is
            // what makes a thumbnail of a Solarized shell look like the Solarized
            // shell it is a thumbnail of.
            focus_mini_text: ink2(termbg),
            focus_mini_edge: border_soft(termbg),
            focus_mini_edge_focused: ink3(termbg),
            // The same ink the rows are set in, at the seam's own opacity, so a
            // Solarized card's seam is Solarized too. `ink_over` twice rather
            // than a single blended alpha because the first call is what makes
            // `focus_mini_text` the colour it is, and the seam is a wash of
            // *that* colour and not of the source.
            focus_mini_seam: ink_over(termbg, ink2(termbg), crate::theme::FOCUS_MINI_SEAM_ALPHA),
        }
    }
}

/// The default ink a scheme puts on its own canvas.
///
/// A free function rather than a field read so the one caller that has only a
/// background — `BT_BG`'s diagnostic path, which paints a canvas the scheme
/// never chose — reaches the same answer as everyone else.
pub(crate) fn foreground_of(scheme: &ColourScheme) -> [u8; 3] {
    scheme.foreground
}

/// A selection fill for a scheme whose file did not write one.
///
/// The same 30 % step, and that is not a shortcut: the light selection this
/// product ships was itself derived from `mark.srch`'s rule — "selection is the
/// same kind of mark, so it takes the same step" — so a scheme that leaves the
/// key out gets the rule rather than a borrowed blue from another product.
#[must_use]
pub fn selection_from_accent(background: [u8; 3], accent: [u8; 3]) -> [u8; 3] {
    ink_over(background, accent, 300)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK_CHROME, LIGHT_CHROME};

    /// PIN — the derivation **is** the dark palette.
    ///
    /// Field by field rather than one `assert_eq!` on the struct, because a
    /// whole-struct failure prints two 139-field debug dumps and names nothing.
    #[test]
    fn the_derivation_reproduces_the_dark_palette_byte_for_byte() {
        assert_palettes_match("dark", ChromePalette::derive(&FOLIO_DARK), DARK_CHROME);
    }

    /// PIN — and the light one.
    #[test]
    fn the_derivation_reproduces_the_light_palette_byte_for_byte() {
        assert_palettes_match("light", ChromePalette::derive(&FOLIO_LIGHT), LIGHT_CHROME);
    }

    /// PIN — **the two thumb tokens are the mock-up's own declarations**
    /// (P2-9 slice 1).
    ///
    /// `design/ui-mockup.html` 53-54 and 75-76 declare `--thumb` and
    /// `--thumb-hover` as four `rgba()` literals, and line 95 lays them straight
    /// on the terminal's canvas (`scrollbar-color: var(--thumb) transparent`).
    /// This restates that composite here rather than trusting the two struck
    /// tables to have been transcribed correctly — the formula and the CSS have
    /// to meet somewhere, and the tables are the thing being checked.
    #[test]
    fn the_scroll_thumb_is_the_mock_ups_own_alpha_over_the_terminals_canvas() {
        // `body.dark { --thumb: rgba(255,255,255,.22); --thumb-hover: … .4 }`
        // over `--termbg: #1B1B1B`.
        let night = ChromePalette::derive(&FOLIO_DARK);
        assert_eq!(
            night.scroll_thumb,
            ink_over(FOLIO_DARK.background, [0xff, 0xff, 0xff], 220)
        );
        assert_eq!(
            night.scroll_thumb_hover,
            ink_over(FOLIO_DARK.background, [0xff, 0xff, 0xff], 400)
        );
        // `:root { --thumb: rgba(55,53,47,.24); --thumb-hover: … .42 }` over
        // `--termbg: #FFFFFF`.
        let paper = ChromePalette::derive(&FOLIO_LIGHT);
        assert_eq!(
            paper.scroll_thumb,
            ink_over(FOLIO_LIGHT.background, [0x37, 0x35, 0x2f], 240)
        );
        assert_eq!(
            paper.scroll_thumb_hover,
            ink_over(FOLIO_LIGHT.background, [0x37, 0x35, 0x2f], 420)
        );
        // And hover is a *step*, not a redeclaration: on both canvases it moves
        // further from the ground than the resting mark does, which is what
        // "brought forward" has to mean for a colour.
        for (name, canvas, palette) in [
            ("night", FOLIO_DARK.background, night),
            ("paper", FOLIO_LIGHT.background, paper),
        ] {
            let distance = |ink: [u8; 3]| {
                (0..3)
                    .map(|channel| i32::from(ink[channel]) - i32::from(canvas[channel]))
                    .map(i32::abs)
                    .sum::<i32>()
            };
            assert!(
                distance(palette.scroll_thumb_hover) > distance(palette.scroll_thumb),
                "{name}: the hovered mark must be the louder of the two"
            );
        }
    }

    #[test]
    fn a_schemes_own_colours_reach_the_chrome_they_are_named_for() {
        let nord = ColourScheme {
            background: [0x2e, 0x34, 0x40],
            foreground: [0xd8, 0xde, 0xe9],
            cursor: [0xec, 0xef, 0xf4],
            selection: [0xec, 0xef, 0xf4],
            ansi: FOLIO_DARK.ansi,
            accent: [0x81, 0xa1, 0xc1],
        };
        let palette = ChromePalette::derive(&nord);
        assert_eq!(
            palette.seat_body, nord.background,
            "the canvas is the canvas"
        );
        assert_eq!(palette.pane_head, nord.background);
        assert_eq!(palette.active_tab, nord.background);
        assert_eq!(palette.accent, nord.accent, "the accent is the accent");
        assert_eq!(palette.divider_active, nord.accent);
        assert_eq!(palette.preview_diff_hunk, nord.accent);
        assert_eq!(palette.preview_caret, nord.cursor);
        assert_eq!(palette.preview_selection, nord.selection);
        // Night, so the three chrome planes step up off it and stay apart.
        assert_eq!(palette.title_bar, [0x38, 0x3e, 0x4a]);
        assert_eq!(palette.dialog_surface, [0x33, 0x39, 0x45]);
        assert_eq!(palette.menu_surface, [0x3d, 0x43, 0x4f]);
        // …and the signals do not follow it.
        assert_eq!(palette.status_ok, DARK_CHROME.status_ok);
        assert_eq!(palette.status_err, DARK_CHROME.status_err);
        assert_eq!(palette.graph_lanes, DARK_CHROME.graph_lanes);
    }

    /// A light scheme whose canvas is not white still gets a light chrome, and
    /// its panel steps *down* rather than clamping.
    #[test]
    fn a_light_scheme_that_is_not_white_keeps_the_light_ladder() {
        let solarized = ColourScheme {
            background: [0xfd, 0xf6, 0xe3],
            foreground: [0x65, 0x7b, 0x83],
            cursor: [0x00, 0x2b, 0x36],
            selection: [0x2c, 0x4d, 0x57],
            ansi: FOLIO_LIGHT.ansi,
            accent: [0x26, 0x8b, 0xd2],
        };
        let palette = ChromePalette::derive(&solarized);
        assert_eq!(palette.seat_body, solarized.background);
        assert_eq!(palette.title_bar, [0xf5, 0xee, 0xd9]);
        // `--ink` is opaque on paper, so the strongest ink is the foreground.
        assert_eq!(palette.preview_body_text, solarized.foreground);
        assert_eq!(palette.pane_title_focus, solarized.foreground);
        // The hairline is struck in black here, not in white.
        assert_eq!(palette.menu_border, [0x00, 0x00, 0x00]);
        assert_eq!(palette.menu_border_alpha, 22);
    }

    /// The ink source is the top of the ladder run backwards, and on this
    /// product's own dark scheme that is pure white.
    #[test]
    fn the_ink_source_is_recovered_from_the_foreground() {
        assert_eq!(
            unmix(FOLIO_DARK.background, FOLIO_DARK.foreground, 870),
            [0xff, 0xff, 0xff],
            "rgba(255,255,255,.87) over #1B1B1B is #E1E1E1"
        );
        assert_eq!(
            unmix(FOLIO_LIGHT.background, FOLIO_LIGHT.foreground, 1000),
            FOLIO_LIGHT.foreground,
            "an opaque ink is its own source"
        );
    }

    /// A scheme with no `selectionBackground` gets this product's rule, not a
    /// borrowed blue.
    #[test]
    fn a_missing_selection_is_the_accent_at_thirty_percent() {
        assert_eq!(
            selection_from_accent(FOLIO_LIGHT.background, FOLIO_LIGHT.accent),
            LIGHT_SELECTION_BACKGROUND_RGB,
            "the light selection this product ships was struck by exactly this rule"
        );
    }

    #[test]
    fn a_schemes_default_ink_is_its_foreground() {
        assert_eq!(foreground_of(&FOLIO_DARK), DEFAULT_FOREGROUND_RGB);
        assert_eq!(foreground_of(&FOLIO_LIGHT), LIGHT_BACKGROUND_FOREGROUND_RGB);
    }

    /// Every scheme, however extreme, has to produce a palette rather than
    /// panic or wrap: the surface steps saturate at both ends.
    #[test]
    fn the_extremes_of_the_gamut_still_derive() {
        for background in [[0x00, 0x00, 0x00], [0xff, 0xff, 0xff]] {
            for foreground in [[0x00, 0x00, 0x00], [0xff, 0xff, 0xff]] {
                let scheme = ColourScheme {
                    background,
                    foreground,
                    cursor: foreground,
                    selection: background,
                    ansi: FOLIO_DARK.ansi,
                    accent: [0x80, 0x80, 0x80],
                };
                let palette = ChromePalette::derive(&scheme);
                assert_eq!(palette.seat_body, background);
            }
        }
    }

    #[track_caller]
    fn assert_palettes_match(name: &str, got: ChromePalette, want: ChromePalette) {
        macro_rules! same {
            ($($field:ident),* $(,)?) => {$(
                assert_eq!(
                    got.$field, want.$field,
                    "{name}.{}: derived {:02x?} against the struck table's {:02x?}",
                    stringify!($field), got.$field, want.$field
                );
            )*};
        }
        same!(
            seat_body,
            title_bar,
            title_text,
            title_text_hover,
            title_text_muted,
            tab_close_pill_on_content,
            tab_close_pill_on_hovered_tab,
            tab_close_glyph_on_active_tab,
            tab_close_glyph_on_hovered_tab,
            tab_close_glyph_on_pill_over_active_tab,
            tab_close_glyph_on_pill_over_hovered_tab,
            tab_pin_state_on_active_tab,
            tab_pin_state_on_hovered_tab,
            body_hint_text,
            preview_body_text,
            preview_grid_line,
            preview_code_ground,
            preview_code_border,
            preview_code_text,
            preview_code_lang,
            preview_diff_add,
            preview_diff_del,
            preview_diff_hunk,
            preview_table_head_text,
            preview_selection,
            preview_caret,
            hl_keyword,
            hl_string,
            hl_comment,
            hl_number,
            hl_type,
            hl_function,
            hl_punct_muted,
            files_row_hover,
            files_row_selected,
            files_row_text,
            files_row_text_hover,
            files_row_text_selected,
            files_row_muted,
            files_row_muted_hover,
            files_row_muted_selected,
            float_row_hover,
            float_row_selected,
            float_row_text,
            float_row_text_hover,
            float_row_text_selected,
            float_row_muted,
            float_row_muted_hover,
            float_row_muted_selected,
            git_section,
            git_row_hover,
            git_row_selected,
            git_row_match,
            git_row_text,
            git_row_text_hover,
            git_row_muted,
            git_row_muted_hover,
            git_act_glyph,
            git_act_glyph_hover,
            git_act_pill,
            git_act_glyph_on_pill,
            git_head_text,
            git_head_muted,
            git_pill_text,
            git_pill_border,
            graph_lanes,
            divider,
            divider_hover,
            divider_active,
            collapse_bar,
            collapse_bar_hover,
            caption_hover,
            caption_close_hover,
            caption_close_text,
            active_tab,
            pane_head,
            pane_close_glyph,
            pane_close_pill,
            pane_close_glyph_on_pill,
            termhost,
            pane_head_edge,
            pane_title,
            pane_title_focus,
            accent,
            command_tick,
            command_tick_crest,
            command_tick_fail_crest,
            command_tick_search_crest,
            scroll_thumb,
            scroll_thumb_hover,
            menu_surface,
            menu_border,
            menu_border_alpha,
            menu_shadow,
            menu_shadow_inner_alpha,
            menu_shadow_outer_alpha,
            menu_popup_shadow_inner_alpha,
            menu_popup_shadow_outer_alpha,
            tip_shadow_inner_alpha,
            tip_shadow_outer_alpha,
            drag_ghost_shadow_inner_alpha,
            drag_ghost_shadow_outer_alpha,
            float_shadow_inner_alpha,
            float_shadow_outer_alpha,
            float_pinned_shadow_inner_alpha,
            float_pinned_shadow_outer_alpha,
            peek_card_shadow_inner_alpha,
            peek_card_shadow_outer_alpha,
            dialog_surface,
            dialog_title_text,
            dialog_secondary_text,
            dialog_muted_text,
            dialog_hover,
            menu_item_text,
            menu_item_text_selected,
            menu_item_hover,
            peek_leaf_focus_fill,
            peek_leaf_focus_edge,
            peek_leaf_focus_text,
            modal_scrim,
            modal_scrim_alpha,
            tab_badge_on_resting_tab,
            tab_badge_text_on_active_tab,
            tab_badge_text_on_resting_tab,
            tab_badge_text_on_hovered_tab,
            menu_item_hint_text,
            menu_item_unavailable_text,
            status_err,
            status_warn,
            status_pause,
            status_ok,
            ring_track_on_active_tab,
            ring_track_on_resting_tab,
            ring_track_on_hovered_tab,
            rail_tab_active,
            rail_tab_active_text,
            rail_tab_hover_text,
            rail_glyph_on_active_tab,
            rail_seam,
            rail_edge,
            rail_shade,
            rail_shade_alpha,
        );
        assert_eq!(got, want, "{name}: a field escaped the list above");
    }
}
