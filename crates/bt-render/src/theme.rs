//! Built-in terminal colors. M2 can replace this module with user-selectable themes without
//! changing the renderer's distinction between default colors and explicit ANSI palette colors.

use std::sync::OnceLock;

/// Windows Terminal's Campbell defaults, from
/// `microsoft/terminal/src/cascadia/TerminalSettingsModel/defaults.json`.
pub const DEFAULT_BACKGROUND_RGB: [u8; 3] = [0x0c, 0x0c, 0x0c];
pub(crate) const DEFAULT_FOREGROUND_RGB: [u8; 3] = [0xcc, 0xcc, 0xcc];
const LIGHT_BACKGROUND_FOREGROUND_RGB: [u8; 3] = [0x0c, 0x0c, 0x0c];
/// Campbell's bright cursor treatment: use white rather than the pre-theme slate fill.
pub(crate) const DEFAULT_CURSOR_RGB: [u8; 3] = [0xff, 0xff, 0xff];
pub(crate) const DEFAULT_DIM_FOREGROUND_RGB: [u8; 3] = [0x88, 0x88, 0x88];
/// Background-only selection treatment; foreground colors remain terminal-authored.
pub(crate) const DEFAULT_SELECTION_BACKGROUND_RGB: [u8; 3] = [0x26, 0x4f, 0x78];
pub(crate) const DEFAULT_STATUS_BACKGROUND_RGB: [u8; 3] = [0x33, 0x33, 0x33];
/// Campbell bright-black: a quiet neutral frame for the hover-peek flyout on the dark default.
pub(crate) const DEFAULT_PEEK_BORDER_RGB: [u8; 3] = [0x76, 0x76, 0x76];

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
    /// A non-terminal seat's body fill. Matches the canvas so an empty pane
    /// reads as "nothing here yet" rather than as a second product.
    pub seat_body: [u8; 3],
    /// A seat title bar's fill: one quiet step off the canvas.
    pub title_bar: [u8; 3],
    /// The hairline between a title bar and the body it captions.
    pub title_bar_edge: [u8; 3],
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
}

/// Chrome over a dark canvas.
pub const DARK_CHROME: ChromePalette = ChromePalette {
    seat_body: [0x0c, 0x0c, 0x0c],
    title_bar: [0x17, 0x17, 0x17],
    title_bar_edge: [0x26, 0x26, 0x26],
    title_text: [0xa0, 0xa0, 0xa0],
    title_text_hover: [0xe6, 0xe6, 0xe6],
    body_hint_text: [0x7d, 0x7d, 0x7d],
    divider: [0x2e, 0x2e, 0x2e],
    divider_hover: [0x6e, 0x6e, 0x6e],
    divider_active: [0x3b, 0x78, 0xff],
    collapse_bar: [0x1f, 0x1f, 0x1f],
    collapse_bar_hover: [0x2e, 0x2e, 0x2e],
};

/// Chrome over a light canvas. Structurally identical; the accent deepens for
/// contrast against white, everything else is the ladder mirrored.
pub const LIGHT_CHROME: ChromePalette = ChromePalette {
    seat_body: [0xf5, 0xf5, 0xf5],
    title_bar: [0xea, 0xea, 0xea],
    title_bar_edge: [0xd8, 0xd8, 0xd8],
    title_text: [0x5c, 0x5c, 0x5c],
    title_text_hover: [0x1a, 0x1a, 0x1a],
    body_hint_text: [0x8a, 0x8a, 0x8a],
    divider: [0xd0, 0xd0, 0xd0],
    divider_hover: [0x9a, 0x9a, 0x9a],
    divider_active: [0x2a, 0x5f, 0xd6],
    collapse_bar: [0xe2, 0xe2, 0xe2],
    collapse_bar_hover: [0xd0, 0xd0, 0xd0],
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

/// A seat title bar's height, in logical pixels.
pub const SEAT_TITLE_BAR_LOGICAL_PX: f32 = 28.0;
/// A seat title's font size, in logical pixels.
pub const SEAT_TITLE_FONT_LOGICAL_PX: f32 = 13.0;
/// The inset between a title bar's edge and its text, in logical pixels.
pub const SEAT_TITLE_PADDING_LOGICAL_PX: f32 = 10.0;
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
    }

    #[test]
    fn foreground_and_revision_cover_dark_light_and_background_changes() {
        assert_eq!(foreground_for_background([0x0c, 0x0c, 0x0c]), [0xcc; 3]);
        assert_eq!(foreground_for_background([0xf5, 0xf5, 0xf5]), [0x0c; 3]);
        let dark = theme_revision_for_colors([0x0c; 3], [0xcc; 3]);
        let other_dark = theme_revision_for_colors([0x12, 0x12, 0x12], [0xcc; 3]);
        let light = theme_revision_for_colors([0xf5; 3], [0x0c; 3]);
        assert_ne!(dark, other_dark);
        assert_ne!(dark, light);
    }
}
