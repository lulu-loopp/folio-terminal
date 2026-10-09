//! **A screen no frame cuts is presented exactly as before panes existed** (ticket 69a,
//! T-PANE-COLUMNS; `docs/plans/design/pane-columns-2026-09-29.md` §4, "on a screen with no frame,
//! each stage is today's code over one whole-screen pane").
//!
//! The session-level twin of `bt-detect`'s `a_plain_screen_detects_exactly_what_it_always_did`:
//! bytes go through the real terminal, the real arming, the real worker hand-off and the real
//! projection, and what the frame then holds — every row's cells, every math placement's geometry,
//! the oracle's ledger and its held-unbacked list — is compared with a fixture captured from main
//! `7b2e0841` before any line of 69a was written.
#![allow(clippy::disallowed_methods)]

use std::{fmt::Write as _, num::NonZeroU32, time::Duration, time::Instant};

use bt_doc::math::{MathRaster, MathRenderError};
use bt_term::{DualPlaneSession, LIVE_MATH_STABLE_INTERVAL, SessionMathTask};

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn raster(width_px: u32, height_px: u32) -> MathRaster {
    MathRaster {
        rgba: vec![0xff; width_px as usize * height_px as usize * 4],
        width_px,
        height_px,
        content_height_px: height_px,
        ascent_px: height_px as f32 - 4.0,
        descent_px: 4.0,
        baseline_px: height_px as f32 - 4.0,
        render_time: Duration::from_millis(1),
        inline_runs: Vec::new(),
    }
}

/// A composite for an inline line: forty raster pixels per run, side by side, as the renderer lays
/// a line's runs out.
fn inline_raster(runs: usize) -> MathRaster {
    let mut raster = raster(40 * runs.max(1) as u32, 18);
    raster.inline_runs = (0..runs)
        .map(|run| bt_doc::InlineRunPlacement {
            run: run as u32,
            x_px: 40 * run as u32,
            width_px: 40,
        })
        .collect();
    raster
}

/// Resolve and complete every queued math task with a fixed synthetic raster.
fn complete_math(session: &mut DualPlaneSession) {
    while let Some(task) = session.take_math_worker_task() {
        match task {
            SessionMathTask::Live(mut task) => {
                if bt_detect::resolve_live_detection_task(&mut task) {
                    let picture = if task.span.inline_runs.is_empty() {
                        raster(120, 36)
                    } else {
                        inline_raster(task.span.inline_runs.len())
                    };
                    session.complete_live_worker_result(task, Ok(picture));
                } else {
                    session.complete_live_worker_result(task, Err(MathRenderError::NotDetected));
                }
            }
            SessionMathTask::Frozen(mut task) => {
                if bt_detect::resolve_detection_task(&mut task) {
                    let picture = if task.span.inline_runs.is_empty() {
                        raster(120, 36)
                    } else {
                        inline_raster(task.span.inline_runs.len())
                    };
                    session.complete_worker_result(task, Ok(picture));
                } else {
                    session.complete_worker_result(task, Err(MathRenderError::NotDetected));
                }
            }
        }
    }
}

const PANE: [&str; 13] = [
    "Inline: $e^{i\\pi}+1=0$ stays inline.",
    "",
    "$$",
    "\\frac{1}{2}",
    "$$",
    "",
    "$$",
    "\\begin{pmatrix}",
    "a & b \\\\",
    "c & d \\\\",
    "e & f",
    "\\end{pmatrix}",
    "$$",
];

/// An alternate-screen repaint: every row addressed and cleared, then written.
fn alternate_repaint(rows: &[&str]) -> Vec<u8> {
    let mut out = b"\x1b[?1049h\x1b[2J".to_vec();
    for (row, line) in rows.iter().enumerate() {
        out.extend_from_slice(format!("\x1b[{};1H\x1b[K{line}", row + 1).as_bytes());
    }
    out
}

/// Ordinary output on the primary screen, one line after another.
fn primary_output(rows: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in rows {
        out.extend_from_slice(line.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out
}

struct Screen {
    name: &'static str,
    columns: u32,
    rows: u32,
    bytes: Vec<u8>,
}

fn screens() -> Vec<Screen> {
    let mut prose_then_pane = (0..30)
        .map(|index| format!("an ordinary line of output number {index}"))
        .collect::<Vec<_>>();
    prose_then_pane.extend(PANE.iter().map(|line| (*line).to_owned()));
    let prose_then_pane = prose_then_pane
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    vec![
        Screen {
            name: "pane-alternate",
            columns: 100,
            rows: 40,
            bytes: alternate_repaint(&PANE),
        },
        Screen {
            name: "pane-primary",
            columns: 80,
            rows: 24,
            bytes: primary_output(&PANE),
        },
        Screen {
            name: "pane-primary-scrolled-into-history",
            columns: 80,
            rows: 24,
            bytes: primary_output(&prose_then_pane),
        },
        Screen {
            name: "wide-characters-alternate",
            columns: 40,
            rows: 24,
            bytes: alternate_repaint(&["前置 $e^{i\\pi}+1=0$ 之后", "", "$$", "\\alpha", "$$"]),
        },
        Screen {
            name: "soft-wrapped-alternate",
            columns: 20,
            rows: 24,
            bytes: b"\x1b[?1049h\x1b[2J\x1b[Hprefix text $a+b$ and more text that wraps here\r\n$$\r\n\\beta\r\n$$"
                .to_vec(),
        },
    ]
}

fn dump(screen: &Screen) -> String {
    let start = Instant::now();
    let mut session = DualPlaneSession::new(nz(screen.columns), nz(screen.rows));
    session.feed_at(&screen.bytes, start).unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    complete_math(&mut session);
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL * 2);
    complete_math(&mut session);
    let mut projection = session.new_projection(session.layout_key());
    session.refresh_projection(&mut projection);
    let frame = session.viewport_frame(&mut projection).unwrap();

    let mut out = String::new();
    writeln!(out, "== {}", screen.name).unwrap();
    let columns = frame.columns.get() as usize;
    for (index, row) in frame.cells.chunks(columns).enumerate() {
        let text = row
            .iter()
            .map(|cell| {
                if cell.text.is_empty() {
                    " "
                } else {
                    cell.text.as_str()
                }
            })
            .collect::<String>();
        let text = text.trim_end();
        if !text.is_empty() {
            writeln!(out, "row {index:>2} |{text}|").unwrap();
        }
    }
    for block in &frame.math_blocks {
        writeln!(
            out,
            "block anchor={:?} top={} left={} content_offset={} clip={} display={:?} source_width_cells={} overflow={:?} toolbar={} source={:?} artifact={}x{}@{} scale={}",
            block.anchor,
            block.top_subpixels,
            block.left_subpixels,
            block.content_offset_subpixels,
            block.clip_height_subpixels,
            block.display,
            block.source_width_cells,
            block.horizontal_overflow,
            block.toolbar_visible,
            block.source,
            block.artifact.width_px,
            block.artifact.height_px,
            block.artifact.height_subpixels,
            block.artifact.render_scale_milli,
        )
        .unwrap();
    }
    writeln!(out, "held_unbacked={:?}", session.held_unbacked_records()).unwrap();
    writeln!(
        out,
        "isolation_gap={}",
        session.live_detection_isolation_gap()
    )
    .unwrap();
    out
}

/// RED (69a) — **a screen with no frame is detected, typeset and presented byte for byte as it was
/// before panes existed.**
///
/// The fixture holds what main `7b2e0841` published for five ordinary screens, alternate and
/// primary, wide characters, a soft wrap and a formula scrolled into history: the frame's text row by
/// row (which cells the pictures cleared), every math placement's geometry and face, the repaint
/// oracle's held-unbacked list and the isolation gap. 69a changes the capture, the arming, the
/// record's storage and every presentation width; on a screen no frame cuts none of it may move.
///
/// MUTATION: bound a whole-screen record's placement by a pane one column narrower than the grid,
/// or arm from a neutral checkpoint instead of the capture's own.
#[test]
fn a_plain_screen_is_presented_exactly_as_it_always_was() {
    let expected = include_str!("fixtures/plain_screen_presentation.baseline")
        .replace("\r\n", "\n")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| format!("{line}\n"))
        .collect::<String>();
    let actual = screens().iter().map(dump).collect::<String>();
    if expected != actual {
        let first = expected
            .lines()
            .zip(actual.lines())
            .position(|(left, right)| left != right);
        panic!(
            "the plain-screen presentation baseline moved; first differing line {first:?}\n--- actual ---\n{actual}--- end ---"
        );
    }
}
