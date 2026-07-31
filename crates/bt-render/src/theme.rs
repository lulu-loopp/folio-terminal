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

fn foreground_for_background(background: [u8; 3]) -> [u8; 3] {
    let background_luma = u32::from(background[0]) * 299
        + u32::from(background[1]) * 587
        + u32::from(background[2]) * 114;
    if background_luma >= 128_000 {
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
