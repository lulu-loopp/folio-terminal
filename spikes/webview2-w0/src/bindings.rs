//! The product's shortcut table, copied verbatim so gate 5 can fire every row
//! of it at a page that has the focus.
//!
//! This is a **transcription**, not an import: the spike is outside the
//! workspace and `bt_app::shortcuts::BINDINGS` is `pub(crate)`. It is kept
//! honest by `LEN`, which asserts the same 34 rows the product's own test
//! asserts (`crates/bt-app/src/shortcuts.rs:2265`) — 30 with a chord and the 4
//! picture-in-picture rows that have none.

/// One row: what a person presses, and the name the product gives it.
#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub id: &'static str,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// The Win32 virtual key. `character("n")` in the product's table is the
    /// letter's own VK; the two OEM rows spell out which OEM key they mean.
    pub vk: u16,
    /// A label for the log, so a reader does not have to decode `0xbd`.
    pub chord: &'static str,
}

const fn row(
    id: &'static str,
    ctrl: bool,
    shift: bool,
    alt: bool,
    vk: u16,
    chord: &'static str,
) -> Row {
    Row {
        id,
        ctrl,
        shift,
        alt,
        vk,
        chord,
    }
}

pub const VK_TAB: u16 = 0x09;
pub const VK_ESCAPE: u16 = 0x1b;
pub const VK_F3: u16 = 0x72;
pub const VK_UP: u16 = 0x26;
pub const VK_DOWN: u16 = 0x28;
const VK_OEM_MINUS: u16 = 0xbd;
const VK_OEM_PLUS: u16 = 0xbb;
const VK_OEM_COMMA: u16 = 0xbc;

/// The 30 rows of `BINDINGS` that carry a chord.
pub const CHORDS: &[Row] = &[
    row("new-tab", true, true, false, b'N' as u16, "Ctrl+Shift+N"),
    row("new-window", true, true, false, b'M' as u16, "Ctrl+Shift+M"),
    row("close-pane", true, true, false, b'W' as u16, "Ctrl+Shift+W"),
    row("next-tab", true, false, false, VK_TAB, "Ctrl+Tab"),
    row("prev-tab", true, true, false, VK_TAB, "Ctrl+Shift+Tab"),
    row("goto-tab-1", true, true, false, b'1' as u16, "Ctrl+Shift+1"),
    row("goto-tab-2", true, true, false, b'2' as u16, "Ctrl+Shift+2"),
    row("goto-tab-3", true, true, false, b'3' as u16, "Ctrl+Shift+3"),
    row("goto-tab-4", true, true, false, b'4' as u16, "Ctrl+Shift+4"),
    row("goto-tab-5", true, true, false, b'5' as u16, "Ctrl+Shift+5"),
    row("goto-tab-6", true, true, false, b'6' as u16, "Ctrl+Shift+6"),
    row("goto-tab-7", true, true, false, b'7' as u16, "Ctrl+Shift+7"),
    row("goto-tab-8", true, true, false, b'8' as u16, "Ctrl+Shift+8"),
    row("goto-tab-9", true, true, false, b'9' as u16, "Ctrl+Shift+9"),
    row(
        "reopen-closed",
        true,
        true,
        false,
        b'T' as u16,
        "Ctrl+Shift+T",
    ),
    row(
        "jump-attention",
        true,
        true,
        false,
        b'A' as u16,
        "Ctrl+Shift+A",
    ),
    row(
        "command-palette",
        true,
        true,
        false,
        b'P' as u16,
        "Ctrl+Shift+P",
    ),
    row("focus-mode", true, true, false, b'Z' as u16, "Ctrl+Shift+Z"),
    row(
        "split-horizontal",
        false,
        true,
        true,
        VK_OEM_MINUS,
        "Alt+Shift+-",
    ),
    row(
        "split-vertical",
        false,
        true,
        true,
        VK_OEM_PLUS,
        "Alt+Shift+=",
    ),
    row(
        "duplicate-pane-split",
        true,
        true,
        false,
        b'D' as u16,
        "Ctrl+Shift+D",
    ),
    row("files-pane", true, true, false, b'B' as u16, "Ctrl+Shift+B"),
    row("git-page", true, true, false, b'G' as u16, "Ctrl+Shift+G"),
    row("open-settings", true, false, false, VK_OEM_COMMA, "Ctrl+,"),
    row("save-preview", true, false, false, b'S' as u16, "Ctrl+S"),
    row(
        "prev-command-mark",
        true,
        true,
        false,
        VK_UP,
        "Ctrl+Shift+Up",
    ),
    row(
        "next-command-mark",
        true,
        true,
        false,
        VK_DOWN,
        "Ctrl+Shift+Down",
    ),
    row("open-search", true, false, false, b'F' as u16, "Ctrl+F"),
    row("next-match", false, false, false, VK_F3, "F3"),
    row("prev-match", false, true, false, VK_F3, "Shift+F3"),
];

/// The four rows with no chord: `summon-pip-1` … `summon-pip-4`.
pub const UNBOUND: usize = 4;

/// The product's own count, restated. If `BINDINGS` grows and this transcription
/// does not, the gate is measuring last week's product.
pub const LEN: usize = 34;

const _: () = assert!(CHORDS.len() + UNBOUND == LEN);

/// Whether this product claims a chord, given the key and the modifiers that
/// were physically down when `AcceleratorKeyPressed` fired.
pub fn claims(vk: u32, ctrl: bool, shift: bool, alt: bool) -> bool {
    CHORDS.iter().any(|row| {
        u32::from(row.vk) == vk && row.ctrl == ctrl && row.shift == shift && row.alt == alt
    })
}

/// Keys this product needs but which are **not** chords: the bare `Tab` that
/// walks a page's controls and then leaves through `MoveFocusRequested`, and the
/// bare `Esc` that dismisses whatever is up.
pub const BARE_KEYS: &[(&str, u16, bool)] = &[
    ("Tab", VK_TAB, false),
    ("Shift+Tab", VK_TAB, true),
    ("Escape", VK_ESCAPE, false),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transcription_is_the_size_the_product_asserts() {
        assert_eq!(CHORDS.len() + UNBOUND, LEN);
    }

    #[test]
    fn no_row_uses_ctrl_alt() {
        // The table's own header rule: Windows reports AltGr as Ctrl+Alt, so a
        // row bound there would steal a character from every composing layout.
        for row in CHORDS {
            assert!(!(row.ctrl && row.alt), "{}", row.id);
        }
    }

    #[test]
    fn every_chord_is_unique() {
        for (index, row) in CHORDS.iter().enumerate() {
            for other in CHORDS.iter().skip(index + 1) {
                assert!(
                    !(row.vk == other.vk
                        && row.ctrl == other.ctrl
                        && row.shift == other.shift
                        && row.alt == other.alt),
                    "{} and {} share a chord",
                    row.id,
                    other.id
                );
            }
        }
    }

    #[test]
    fn claims_matches_the_table() {
        assert!(claims(u32::from(b'N'), true, true, false));
        assert!(!claims(u32::from(b'N'), true, false, false));
        assert!(claims(u32::from(VK_TAB), true, false, false));
        assert!(!claims(u32::from(VK_TAB), false, false, false));
    }
}
