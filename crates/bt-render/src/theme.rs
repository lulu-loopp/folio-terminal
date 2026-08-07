//! Built-in terminal colors. M2 can replace this module with user-selectable themes without
//! changing the renderer's distinction between default colors and explicit ANSI palette colors.

use std::sync::OnceLock;

/// The product's terminal defaults, from `design/ui-mockup.html` (the approved
/// styling): dark `--termbg #1B1B1B`, ink `rgba(255,255,255,.87)` composited
/// over it, light ink `--ink #37352F`. The ANSI 16 remain Campbell — those are
/// terminal-authored colors, not chrome.
pub const DEFAULT_BACKGROUND_RGB: [u8; 3] = [0x1b, 0x1b, 0x1b];
pub(crate) const DEFAULT_FOREGROUND_RGB: [u8; 3] = [0xe1, 0xe1, 0xe1];
const LIGHT_BACKGROUND_FOREGROUND_RGB: [u8; 3] = [0x37, 0x35, 0x2f];
/// The mock-up's `--cursor` on dark.
pub(crate) const DEFAULT_CURSOR_RGB: [u8; 3] = [0xd4, 0xd4, 0xd4];
/// The default cursor's width, as a fraction of one cell.
///
/// `.cursor { width: 7px; height: 15px }` in a 14px-font mock-up: a bar half a
/// cell wide, not the filled cell a VT's own default block would be. This is the
/// *default* form only — a settings surface will one day offer block / bar / underline
/// here, and a program that asks for a shape through DECSCUSR must be honoured over
/// both. Nothing in this build parses DECSCUSR (`bt_viewport::GridCursor` carries
/// row, column and visibility and no shape), so today there is exactly one form and
/// this is it.
pub const CURSOR_WIDTH_CELL_RATIO: f32 = 0.5;
pub(crate) const DEFAULT_DIM_FOREGROUND_RGB: [u8; 3] = [0x88, 0x88, 0x88];
/// Background-only selection treatment; foreground colors remain terminal-authored.
pub(crate) const DEFAULT_SELECTION_BACKGROUND_RGB: [u8; 3] = [0x26, 0x4f, 0x78];
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
    /// Title text and the `×` at rest. Bars are wayfinding, not content, so
    /// this ink sits a step below the terminal's own.
    pub title_text: [u8; 3],
    /// The `×` under the pointer: the one moment a bar glyph is the subject.
    pub title_text_hover: [u8; 3],
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
}

/// Chrome over a dark canvas — `design/ui-mockup.html` `body.dark`, with its
/// alpha hairlines pre-composited over the surface each one actually sits on
/// (our chrome quads are opaque): `--termbg #1B1B1B`, `--panel #252525`,
/// `--ink/2/3` at .87/.55/.38 white, `--border` at .094 white,
/// `--accent #828FFF`.
pub const DARK_CHROME: ChromePalette = ChromePalette {
    seat_body: [0x1b, 0x1b, 0x1b],
    title_bar: [0x25, 0x25, 0x25],
    title_text: [0x9d, 0x9d, 0x9d],
    title_text_hover: [0xe3, 0xe3, 0xe3],
    body_hint_text: [0x75, 0x75, 0x75],
    divider: [0x35, 0x35, 0x35],
    divider_hover: [0x51, 0x51, 0x51],
    divider_active: [0x82, 0x8f, 0xff],
    collapse_bar: [0x25, 0x25, 0x25],
    collapse_bar_hover: [0x31, 0x31, 0x31],
    caption_hover: [0x31, 0x31, 0x31],
    caption_close_hover: [0xe5, 0x48, 0x4d],
    caption_close_text: [0xff, 0xff, 0xff],
    active_tab: [0x1b, 0x1b, 0x1b],
    pane_head: [0x1b, 0x1b, 0x1b],
    pane_head_edge: [0x29, 0x29, 0x29],
    pane_title: [0x75, 0x75, 0x75],
    pane_title_focus: [0xe1, 0xe1, 0xe1],
    accent: [0x82, 0x8f, 0xff],
    menu_surface: [0x2a, 0x2a, 0x2a],
    menu_border: [0xff, 0xff, 0xff],
    menu_border_alpha: 24,
    menu_shadow: [0x00, 0x00, 0x00],
    menu_shadow_inner_alpha: 46,
    menu_shadow_outer_alpha: 23,
};

/// Chrome over a light canvas — the mock-up's `:root` defaults, composited the
/// same way: `--win #FFFFFF`, `--panel #F7F7F5`, `--ink #37352F` at
/// .65/.45 for the secondary steps, `--border` at .088 black,
/// `--accent #5E6AD2`.
pub const LIGHT_CHROME: ChromePalette = ChromePalette {
    seat_body: [0xff, 0xff, 0xff],
    title_bar: [0xf7, 0xf7, 0xf5],
    title_text: [0x7a, 0x79, 0x74],
    title_text_hover: [0x37, 0x35, 0x2f],
    body_hint_text: [0xa5, 0xa4, 0xa1],
    divider: [0xe9, 0xe9, 0xe9],
    divider_hover: [0xc2, 0xc1, 0xbf],
    divider_active: [0x5e, 0x6a, 0xd2],
    collapse_bar: [0xf7, 0xf7, 0xf5],
    collapse_bar_hover: [0xe9, 0xe9, 0xe8],
    caption_hover: [0xec, 0xec, 0xea],
    caption_close_hover: [0xe5, 0x48, 0x4d],
    caption_close_text: [0xff, 0xff, 0xff],
    active_tab: [0xff, 0xff, 0xff],
    pane_head: [0xff, 0xff, 0xff],
    pane_head_edge: [0xf1, 0xf1, 0xf1],
    pane_title: [0xa5, 0xa4, 0xa1],
    pane_title_focus: [0x37, 0x35, 0x2f],
    accent: [0x5e, 0x6a, 0xd2],
    menu_surface: [0xff, 0xff, 0xff],
    menu_border: [0x00, 0x00, 0x00],
    menu_border_alpha: 22,
    menu_shadow: [0x00, 0x00, 0x00],
    menu_shadow_inner_alpha: 18,
    menu_shadow_outer_alpha: 9,
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
/// One tab's CSS cap; this slice draws exactly one current-session tab.
pub const WINDOW_TAB_MAX_WIDTH_LOGICAL_PX: f32 = 200.0;
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
/// A seat title's font size (`.panehead { font-size: 11.5px }`).
pub const SEAT_TITLE_FONT_LOGICAL_PX: f32 = 11.5;
/// The inset between a title bar's edge and its first item
/// (`.panehead { padding: 0 6px 0 12px }`).
pub const SEAT_TITLE_PADDING_LOGICAL_PX: f32 = 12.0;
/// `.panehead { gap: 7px }` — between the mark and the title.
pub const SEAT_TITLE_GAP_LOGICAL_PX: f32 = 7.0;
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
pub const SEAT_DIVIDER_HIT_LOGICAL_PX: f32 = 6.0;
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
/// Breathing room between a previewed image and its seat's edges, in logical
/// pixels. Skipped entirely when the body is too small to afford it, because a
/// margin that eats the picture serves nobody.
pub const PREVIEW_BODY_INSET_LOGICAL_PX: f32 = 12.0;

/// Process-wide background selected before the first window or renderer is created.
///
/// `BT_BG` is a diagnostic reveal switch, not a second theme system. It stays in sRGB byte form
/// here so the Win32 class brush and terminal default-color resolution share the same value; the
/// renderer's existing upload boundary remains the only sRGB-to-linear conversion point.
pub fn background_rgb() -> [u8; 3] {
    static BACKGROUND: OnceLock<[u8; 3]> = OnceLock::new();
    *BACKGROUND.get_or_init(|| {
        let Some(value) = std::env::var_os("BT_BG") else {
            return DEFAULT_BACKGROUND_RGB;
        };
        let Some(rgb) = value.to_str().and_then(parse_background_rgb) else {
            eprintln!("BT_THEME invalid_BT_BG={value:?} ignored default=#0C0C0C expected=#RRGGBB");
            return DEFAULT_BACKGROUND_RGB;
        };
        rgb
    })
}

/// Default terminal ink paired with the process theme background. The current product surface
/// exposes `BT_BG` as its theme diagnostic; choosing the higher-contrast Campbell ink here also
/// gives math rasterization the same dark/light decision as ordinary default-colored text.
pub fn foreground_rgb() -> [u8; 3] {
    foreground_for_background(background_rgb())
}

/// Stable identity for every color which affects theme-authored layout artifacts. A different
/// `BT_BG` therefore invalidates CPU math rasters and their independently keyed GPU textures even
/// when it remains on the same side of the dark/light foreground threshold.
pub fn theme_revision() -> u64 {
    theme_revision_for_colors(background_rgb(), foreground_rgb())
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

fn theme_revision_for_colors(background: [u8; 3], foreground: [u8; 3]) -> u64 {
    u64::from_be_bytes([
        1,
        background[0],
        background[1],
        background[2],
        foreground[0],
        foreground[1],
        foreground[2],
        0,
    ])
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
/// order. Explicit palette black intentionally matches the default background numerically, while
/// remaining a separate `TerminalColor` value so SGR 39/49 can resolve through the theme defaults.
pub(crate) const ANSI_16_RGB: [[u8; 3]; 16] = [
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

#[cfg(test)]
mod tests {
    use super::*;

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

    /// PIN (visual pass): the three numbers the mock-up gives a floating window
    /// and the caret, held here so overturning one is an edit to this block.
    #[test]
    fn float_window_and_cursor_tokens_are_the_mock_ups_own() {
        // `.cursor { width: 7px }` against the 15px cell its 14px font sets.
        assert_eq!(CURSOR_WIDTH_CELL_RATIO, 0.5);
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

    #[test]
    fn foreground_and_revision_cover_dark_light_and_background_changes() {
        assert_eq!(
            foreground_for_background([0x1b, 0x1b, 0x1b]),
            [0xe1, 0xe1, 0xe1]
        );
        assert_eq!(
            foreground_for_background([0xf5, 0xf5, 0xf5]),
            [0x37, 0x35, 0x2f]
        );
        let dark = theme_revision_for_colors([0x1b; 3], [0xe1; 3]);
        let other_dark = theme_revision_for_colors([0x12, 0x12, 0x12], [0xe1; 3]);
        let light = theme_revision_for_colors([0xf5; 3], [0x37, 0x35, 0x2f]);
        assert_ne!(dark, other_dark);
        assert_ne!(dark, light);
    }
}
