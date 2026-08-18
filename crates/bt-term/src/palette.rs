//! The colours a terminal is wearing, as far as the programs inside it can ask.
//!
//! # Why this exists at all
//!
//! A program that draws its own themed surfaces — Claude Code's message rows,
//! `bat`'s gutter, a TUI's status bar — has exactly one way to find out what
//! canvas it landed on: it asks. `OSC 10;?` for the default ink, `OSC 11;?`
//! for the canvas, `OSC 4;N;?` for one of the sixteen. A terminal that stays
//! silent is not neutral: the asker times out and falls back to *dark*, which
//! is how a light Folio window ended up carrying near-black message bars and a
//! bottle-green diff block drawn by a program that had every intention of
//! matching us and no way to.
//!
//! # What a palette is here, and what it is not
//!
//! It is **not** a second copy of the colour scheme. `bt-term` has no palette
//! of its own and cannot invent one: this crate resolves nothing — every cell
//! it captures carries `Named`/`Indexed`/`Rgb` symbolically and the renderer
//! decides what those mean. So the nineteen colours below are *told* to the
//! terminal by the window that owns it, and re-told whenever the window
//! repaints itself in another scheme. What the terminal owns is only the
//! **protocol**: which index means what, and how an answer is spelled.
//!
//! That split is also why [`TerminalPalette`] carries [`TerminalCanvas`]. The
//! dark/light question is settled by one luma threshold that lives in the
//! renderer (`bt_render::background_is_light`), and a second copy of that
//! threshold here would be a second answer waiting to disagree — so the window
//! states its verdict and this crate takes it as given.

use bt_transcript::indexed_cube_color;

/// Which of the two canvases the window says it is painting.
///
/// Reported to a subscribed program as DEC mode 2031's `CSI ? 997 ; 1 n` /
/// `CSI ? 997 ; 2 n`; the numbering is that notification's, not ours.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalCanvas {
    Dark,
    Light,
}

impl TerminalCanvas {
    /// The parameter `CSI ? 997 ; Ps n` carries for this canvas.
    fn notification_parameter(self) -> u8 {
        match self {
            TerminalCanvas::Dark => 1,
            TerminalCanvas::Light => 2,
        }
    }
}

/// The nineteen colours a window hands its terminal so the programs inside can
/// ask what they are standing on.
///
/// `Copy`, and deliberately small: the owning app pushes it down the same path
/// it drains protocol replies from, so it is copied once per pipe-drain rather
/// than stored in a lock somebody has to remember to invalidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalPalette {
    /// Which canvas this palette is, as decided by the window's one threshold.
    pub canvas: TerminalCanvas,
    /// `OSC 11` — the terminal canvas.
    pub background: [u8; 3],
    /// `OSC 10` — default ink.
    pub foreground: [u8; 3],
    /// `OSC 12` — the caret's own colour.
    pub cursor: [u8; 3],
    /// `OSC 4;0..16` — ANSI 0..=15, normal then bright.
    pub ansi: [[u8; 3]; 16],
}

/// `NamedColor::Foreground` in the vendored colour table.
///
/// Spelled as a number rather than imported, because what this module answers
/// is not "whatever upstream calls slot 256" but the OSC contract: `OSC 10` is
/// the foreground, `OSC 11` the background, `OSC 12` the cursor, and the vte
/// parser maps those three onto 256/257/258 before we ever see them.
const FOREGROUND_INDEX: usize = 256;
const BACKGROUND_INDEX: usize = 257;
const CURSOR_INDEX: usize = 258;

impl TerminalPalette {
    /// The colour filed under one slot of the vendored 269-entry colour table,
    /// or `None` when this palette does not describe that slot.
    ///
    /// Only `0..=258` are reachable from an escape sequence: `OSC 4;N;?` parses
    /// `N` into a `u8`, and `OSC 10/11/12` are the only dynamic colours the
    /// parser turns into a query. Everything above — the dim ladder at
    /// `259..267`, the bright foreground, the dim background — exists so SGR
    /// can name it and has no query syntax at all, so answering `None` there is
    /// a statement about the protocol rather than a gap in this table.
    #[must_use]
    pub fn color(&self, index: usize) -> Option<[u8; 3]> {
        match index {
            FOREGROUND_INDEX => Some(self.foreground),
            BACKGROUND_INDEX => Some(self.background),
            CURSOR_INDEX => Some(self.cursor),
            index if index < 16 => Some(self.ansi[index]),
            index if index < 256 => indexed_cube_color(index as u8),
            _ => None,
        }
    }

    /// The bytes announcing a move to this palette's canvas, for a program that
    /// enabled DEC mode 2031.
    pub(crate) fn canvas_notification(&self) -> Vec<u8> {
        format!("\x1b[?997;{}n", self.canvas.notification_parameter()).into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> TerminalPalette {
        TerminalPalette {
            canvas: TerminalCanvas::Light,
            background: [0xff, 0xff, 0xff],
            foreground: [0x37, 0x35, 0x2f],
            cursor: [0x37, 0x35, 0x2f],
            ansi: [[0x11, 0x22, 0x33]; 16],
        }
    }

    #[test]
    fn the_three_dynamic_colours_answer_from_their_own_fields() {
        let palette = palette();
        assert_eq!(palette.color(FOREGROUND_INDEX), Some([0x37, 0x35, 0x2f]));
        assert_eq!(palette.color(BACKGROUND_INDEX), Some([0xff, 0xff, 0xff]));
        assert_eq!(palette.color(CURSOR_INDEX), Some([0x37, 0x35, 0x2f]));
    }

    #[test]
    fn the_sixteen_are_the_windows_and_the_two_hundred_forty_are_the_protocols() {
        let mut palette = palette();
        palette.ansi[2] = [0x00, 0xa6, 0x00];
        assert_eq!(palette.color(2), Some([0x00, 0xa6, 0x00]));
        assert_eq!(palette.color(196), Some([0xff, 0x00, 0x00]));
        assert_eq!(palette.color(255), Some([0xee, 0xee, 0xee]));
    }

    #[test]
    fn the_slots_no_escape_sequence_can_ask_for_are_not_answered() {
        let palette = palette();
        // 259..267 is the dim ladder, 267 the bright foreground, 268 the dim
        // background; SGR reaches them, no query does.
        for index in 259..269 {
            assert_eq!(palette.color(index), None, "index {index}");
        }
    }

    #[test]
    fn the_canvas_notification_is_dec_2031s_own_spelling() {
        let mut palette = palette();
        assert_eq!(palette.canvas_notification(), b"\x1b[?997;2n".to_vec());
        palette.canvas = TerminalCanvas::Dark;
        assert_eq!(palette.canvas_notification(), b"\x1b[?997;1n".to_vec());
    }
}
