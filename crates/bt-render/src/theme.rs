//! Built-in terminal colors. M2 can replace this module with user-selectable themes without
//! changing the renderer's distinction between default colors and explicit ANSI palette colors.

/// Windows Terminal's Campbell defaults, from
/// `microsoft/terminal/src/cascadia/TerminalSettingsModel/defaults.json`.
pub const DEFAULT_BACKGROUND_RGB: [u8; 3] = [0x0c, 0x0c, 0x0c];
pub(crate) const DEFAULT_FOREGROUND_RGB: [u8; 3] = [0xcc, 0xcc, 0xcc];
/// Campbell's bright cursor treatment: use white rather than the pre-theme slate fill.
pub(crate) const DEFAULT_CURSOR_RGB: [u8; 3] = [0xff, 0xff, 0xff];
pub(crate) const DEFAULT_DIM_FOREGROUND_RGB: [u8; 3] = [0x88, 0x88, 0x88];

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
