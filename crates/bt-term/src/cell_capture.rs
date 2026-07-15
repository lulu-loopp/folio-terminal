use alacritty_terminal::{
    Term,
    grid::Dimensions,
    index::{Column, Line},
    term::cell::{Cell, Flags},
    vte::ansi::{Color, NamedColor},
};
use bt_transcript::{CapturedCell, CapturedRow, CellFlags, CellStyle, TerminalColor};

#[cfg(test)]
use alacritty_terminal::vte::ansi::Rgb;

use crate::adapter::CaptureListener;

pub(crate) fn snapshot(term: &Term<CaptureListener>) -> Vec<Vec<Cell>> {
    (0..term.screen_lines())
        .map(|row| {
            (0..term.columns())
                .map(|column| term.grid()[Line(row as i32)][Column(column)].clone())
                .collect()
        })
        .collect()
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
