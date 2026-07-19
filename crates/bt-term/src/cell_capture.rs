use alacritty_terminal::{
    Term,
    grid::Dimensions,
    index::{Column, Line},
    term::cell::{Cell, Flags},
    vte::ansi::{Color, NamedColor},
};
use bt_transcript::{CapturedCell, CapturedRow, CellFlags, CellStyle, TerminalColor};
use std::hash::{Hash, Hasher};

#[cfg(test)]
use alacritty_terminal::vte::ansi::Rgb;

use crate::adapter::CaptureListener;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CapturedRowFingerprint(u64);

pub(crate) fn snapshot(term: &Term<CaptureListener>) -> Vec<Vec<Cell>> {
    (0..term.screen_lines())
        .map(|row| {
            (0..term.columns())
                .map(|column| term.grid()[Line(row as i32)][Column(column)].clone())
                .collect()
        })
        .collect()
}

/// Hash the exact stable row semantics without allocating captured `String`s. The builder is
/// randomly keyed per terminal session. A collision would merely defer a live-math invalidation
/// until a later differing repaint; even an intentionally conservative 10^12 lifetime comparisons
/// has union-bound probability below 5.5e-8. It cannot affect terminal cells, transcript ownership,
/// or memory safety, so this is an acceptable visual-cache tradeoff rather than a security boundary.
pub(crate) fn captured_row_fingerprint(
    term: &Term<CaptureListener>,
    row: usize,
    seed: u64,
) -> CapturedRowFingerprint {
    let mut hasher = RowHasher(seed);
    0x4254_524f_5731_u64.hash(&mut hasher);
    for column in 0..term.columns() {
        let cell = &term.grid()[Line(row as i32)][Column(column)];
        cell.c.hash(&mut hasher);
        cell.zerowidth().hash(&mut hasher);
        capture_flags(cell.flags).hash(&mut hasher);
        capture_color(cell.fg).hash(&mut hasher);
        capture_color(cell.bg).hash(&mut hasher);
        if let Some(link) = cell.hyperlink() {
            true.hash(&mut hasher);
            link.uri().hash(&mut hasher);
        } else {
            false.hash(&mut hasher);
        }
        cell.flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            .hash(&mut hasher);
    }
    let continues = term.columns() != 0
        && term.grid()[Line(row as i32)][Column(term.columns() - 1)]
            .flags
            .contains(Flags::WRAPLINE);
    continues.hash(&mut hasher);
    CapturedRowFingerprint(hasher.finish())
}

/// Allocation-free, per-session-seeded mixer for non-security visual cache keys. Implementing the
/// primitive writes avoids SipHash's intentionally expensive adversarial-map protection on the TUI
/// repaint path while retaining a well-dispersed 64-bit summary.
struct RowHasher(u64);

impl RowHasher {
    fn mix(&mut self, value: u64) {
        self.0 ^= value.wrapping_add(0x9e37_79b9_7f4a_7c15);
        self.0 = self
            .0
            .rotate_left(27)
            .wrapping_mul(0x3c79_ac49_2ba7_b653)
            .wrapping_add(0x1c69_b3f7_4ac4_ae35);
    }
}

impl Hasher for RowHasher {
    fn finish(&self) -> u64 {
        let mut value = self.0;
        value ^= value >> 33;
        value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
        value ^= value >> 33;
        value = value.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        value ^ (value >> 33)
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.mix(u64::from_le_bytes(chunk.try_into().unwrap()));
        }
        let remainder = chunks.remainder();
        if !remainder.is_empty() {
            let mut tail = [0_u8; 8];
            tail[..remainder.len()].copy_from_slice(remainder);
            self.mix(u64::from_le_bytes(tail) ^ ((remainder.len() as u64) << 56));
        }
        self.mix(bytes.len() as u64);
    }

    fn write_u8(&mut self, value: u8) {
        self.mix(u64::from(value));
    }

    fn write_u16(&mut self, value: u16) {
        self.mix(u64::from(value));
    }

    fn write_u32(&mut self, value: u32) {
        self.mix(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.mix(value);
    }

    fn write_usize(&mut self, value: usize) {
        self.mix(value as u64);
    }
}

pub(crate) fn captured_row_is_blank(row: &CapturedRow) -> bool {
    row.cells
        .iter()
        .all(|cell| !cell.wide_spacer && cell.text.chars().all(char::is_whitespace))
}

pub(crate) fn to_captured_row(row: &[Cell]) -> CapturedRow {
    let continues = row
        .last()
        .is_some_and(|cell| cell.flags.contains(Flags::WRAPLINE));
    let cells = row
        .iter()
        .map(|cell| {
            let mut text = cell.c.to_string();
            if let Some(zero_width) = cell.zerowidth() {
                text.extend(zero_width);
            }
            CapturedCell {
                text,
                style: CellStyle {
                    flags: capture_flags(cell.flags),
                    foreground: capture_color(cell.fg),
                    background: capture_color(cell.bg),
                },
                hyperlink: cell.hyperlink().map(|link| link.uri().to_string()),
                wide_spacer: cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
            }
        })
        .collect();
    CapturedRow {
        cells,
        continues,
        shell_mark: None,
    }
}

fn capture_flags(flags: Flags) -> CellFlags {
    let mut captured = CellFlags::empty();
    for (upstream, stable) in [
        (Flags::INVERSE, CellFlags::INVERSE),
        (Flags::BOLD, CellFlags::BOLD),
        (Flags::ITALIC, CellFlags::ITALIC),
        (Flags::UNDERLINE, CellFlags::UNDERLINE),
        (Flags::DIM, CellFlags::DIM),
        (Flags::HIDDEN, CellFlags::HIDDEN),
        (Flags::STRIKEOUT, CellFlags::STRIKEOUT),
        (Flags::DOUBLE_UNDERLINE, CellFlags::DOUBLE_UNDERLINE),
        (Flags::UNDERCURL, CellFlags::UNDERCURL),
        (Flags::DOTTED_UNDERLINE, CellFlags::DOTTED_UNDERLINE),
        (Flags::DASHED_UNDERLINE, CellFlags::DASHED_UNDERLINE),
        (Flags::WIDE_CHAR, CellFlags::WIDE_CHAR),
    ] {
        if flags.contains(upstream) {
            captured.insert(stable);
        }
    }
    captured
}

#[cfg(test)]
fn decode_flags(flags: CellFlags) -> Flags {
    let mut decoded = Flags::empty();
    for (stable, upstream) in [
        (CellFlags::INVERSE, Flags::INVERSE),
        (CellFlags::BOLD, Flags::BOLD),
        (CellFlags::ITALIC, Flags::ITALIC),
        (CellFlags::UNDERLINE, Flags::UNDERLINE),
        (CellFlags::DIM, Flags::DIM),
        (CellFlags::HIDDEN, Flags::HIDDEN),
        (CellFlags::STRIKEOUT, Flags::STRIKEOUT),
        (CellFlags::DOUBLE_UNDERLINE, Flags::DOUBLE_UNDERLINE),
        (CellFlags::UNDERCURL, Flags::UNDERCURL),
        (CellFlags::DOTTED_UNDERLINE, Flags::DOTTED_UNDERLINE),
        (CellFlags::DASHED_UNDERLINE, Flags::DASHED_UNDERLINE),
        (CellFlags::WIDE_CHAR, Flags::WIDE_CHAR),
    ] {
        if flags.contains(stable) {
            decoded.insert(upstream);
        }
    }
    decoded
}

fn capture_color(color: Color) -> TerminalColor {
    match color {
        Color::Named(named) => TerminalColor::Named(capture_named_color(named)),
        Color::Indexed(index) => TerminalColor::Indexed(index),
        Color::Spec(rgb) => TerminalColor::Rgb(rgb.r, rgb.g, rgb.b),
    }
}

#[cfg(test)]
fn decode_color(color: TerminalColor) -> Option<Color> {
    match color {
        TerminalColor::Named(code) => decode_named_color(code).map(Color::Named),
        TerminalColor::Indexed(index) => Some(Color::Indexed(index)),
        TerminalColor::Rgb(r, g, b) => Some(Color::Spec(Rgb { r, g, b })),
    }
}

fn capture_named_color(color: NamedColor) -> u8 {
    match color {
        NamedColor::Black => 0,
        NamedColor::Red => 1,
        NamedColor::Green => 2,
        NamedColor::Yellow => 3,
        NamedColor::Blue => 4,
        NamedColor::Magenta => 5,
        NamedColor::Cyan => 6,
        NamedColor::White => 7,
        NamedColor::BrightBlack => 8,
        NamedColor::BrightRed => 9,
        NamedColor::BrightGreen => 10,
        NamedColor::BrightYellow => 11,
        NamedColor::BrightBlue => 12,
        NamedColor::BrightMagenta => 13,
        NamedColor::BrightCyan => 14,
        NamedColor::BrightWhite => 15,
        NamedColor::Foreground => 16,
        NamedColor::Background => 17,
        NamedColor::Cursor => 18,
        NamedColor::DimBlack => 19,
        NamedColor::DimRed => 20,
        NamedColor::DimGreen => 21,
        NamedColor::DimYellow => 22,
        NamedColor::DimBlue => 23,
        NamedColor::DimMagenta => 24,
        NamedColor::DimCyan => 25,
        NamedColor::DimWhite => 26,
        NamedColor::BrightForeground => 27,
        NamedColor::DimForeground => 28,
    }
}

#[cfg(test)]
fn decode_named_color(code: u8) -> Option<NamedColor> {
    Some(match code {
        0 => NamedColor::Black,
        1 => NamedColor::Red,
        2 => NamedColor::Green,
        3 => NamedColor::Yellow,
        4 => NamedColor::Blue,
        5 => NamedColor::Magenta,
        6 => NamedColor::Cyan,
        7 => NamedColor::White,
        8 => NamedColor::BrightBlack,
        9 => NamedColor::BrightRed,
        10 => NamedColor::BrightGreen,
        11 => NamedColor::BrightYellow,
        12 => NamedColor::BrightBlue,
        13 => NamedColor::BrightMagenta,
        14 => NamedColor::BrightCyan,
        15 => NamedColor::BrightWhite,
        16 => NamedColor::Foreground,
        17 => NamedColor::Background,
        18 => NamedColor::Cursor,
        19 => NamedColor::DimBlack,
        20 => NamedColor::DimRed,
        21 => NamedColor::DimGreen,
        22 => NamedColor::DimYellow,
        23 => NamedColor::DimBlue,
        24 => NamedColor::DimMagenta,
        25 => NamedColor::DimCyan,
        26 => NamedColor::DimWhite,
        27 => NamedColor::BrightForeground,
        28 => NamedColor::DimForeground,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_flag_maps_and_round_trips_independently() {
        for (upstream, stable) in [
            (Flags::INVERSE, CellFlags::INVERSE),
            (Flags::BOLD, CellFlags::BOLD),
            (Flags::ITALIC, CellFlags::ITALIC),
            (Flags::UNDERLINE, CellFlags::UNDERLINE),
            (Flags::DIM, CellFlags::DIM),
            (Flags::HIDDEN, CellFlags::HIDDEN),
            (Flags::STRIKEOUT, CellFlags::STRIKEOUT),
            (Flags::DOUBLE_UNDERLINE, CellFlags::DOUBLE_UNDERLINE),
            (Flags::UNDERCURL, CellFlags::UNDERCURL),
            (Flags::DOTTED_UNDERLINE, CellFlags::DOTTED_UNDERLINE),
            (Flags::DASHED_UNDERLINE, CellFlags::DASHED_UNDERLINE),
            (Flags::WIDE_CHAR, CellFlags::WIDE_CHAR),
        ] {
            assert_eq!(capture_flags(upstream), stable, "capture {upstream:?}");
            assert_eq!(decode_flags(stable), upstream, "decode {stable:?}");
            assert_eq!(decode_flags(capture_flags(upstream)), upstream);
        }
    }

    #[test]
    fn every_named_color_maps_and_round_trips_independently() {
        for (named, code) in [
            (NamedColor::Black, 0),
            (NamedColor::Red, 1),
            (NamedColor::Green, 2),
            (NamedColor::Yellow, 3),
            (NamedColor::Blue, 4),
            (NamedColor::Magenta, 5),
            (NamedColor::Cyan, 6),
            (NamedColor::White, 7),
            (NamedColor::BrightBlack, 8),
            (NamedColor::BrightRed, 9),
            (NamedColor::BrightGreen, 10),
            (NamedColor::BrightYellow, 11),
            (NamedColor::BrightBlue, 12),
            (NamedColor::BrightMagenta, 13),
            (NamedColor::BrightCyan, 14),
            (NamedColor::BrightWhite, 15),
            (NamedColor::Foreground, 16),
            (NamedColor::Background, 17),
            (NamedColor::Cursor, 18),
            (NamedColor::DimBlack, 19),
            (NamedColor::DimRed, 20),
            (NamedColor::DimGreen, 21),
            (NamedColor::DimYellow, 22),
            (NamedColor::DimBlue, 23),
            (NamedColor::DimMagenta, 24),
            (NamedColor::DimCyan, 25),
            (NamedColor::DimWhite, 26),
            (NamedColor::BrightForeground, 27),
            (NamedColor::DimForeground, 28),
        ] {
            let upstream = Color::Named(named);
            let stable = TerminalColor::Named(code);
            assert_eq!(capture_color(upstream), stable, "capture {named:?}");
            assert_eq!(decode_color(stable), Some(upstream), "decode {named:?}");
            assert_eq!(decode_color(capture_color(upstream)), Some(upstream));
        }
    }

    #[test]
    fn indexed_and_rgb_colors_round_trip_through_stable_types() {
        for color in [Color::Indexed(213), Color::Spec(Rgb { r: 3, g: 5, b: 8 })] {
            assert_eq!(decode_color(capture_color(color)), Some(color));
        }
        assert_eq!(decode_color(TerminalColor::Named(u8::MAX)), None);
    }
}
