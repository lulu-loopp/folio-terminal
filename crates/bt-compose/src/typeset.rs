//! **Typesetting a terminal's formulas** — the math execution a decoration task asks for
//! (design T-COMPOSE-CRATE §3.4 D-15: composition owns math execution).
//!
//! `bt-term` proves where a formula is and files the task; what the formula looks like is decided
//! here, with the host's [`MathEngine`]: the render key at the pane's em, the inline composite
//! with each run fitted to its row or left at its source, and the empty raster a proven table
//! comes back with. The answer is handed back to the session through its completions, by the
//! host's lane or by [`crate::pump`].

use std::{num::NonZeroU32, time::Duration};

use bt_detect::{
    DetectionTask, LiveDetectionTask, MathSpan, resolve_detection_task, resolve_live_detection_task,
};
use bt_doc::{
    BlockKind, InlineRunPlacement, LayoutKey, MathMode, SUBPIXELS_PER_PX, ScreenId,
    math::{MathRaster, MathRenderError, MathRenderKey},
};
use bt_math::MathEngine;
use bt_term::{SessionMathTask, extend_live_task_band, live_snapshot_logical_line_text};
use unicode_width::UnicodeWidthStr;

/// **Typeset one math task**, frozen or live, in `foreground_rgb`. The task is resolved against
/// its own snapshot first (a task whose formula is no longer proven answers
/// [`MathRenderError::NotDetected`]); a live task's band is settled on the way. The result is what
/// the session's completion for that task takes.
pub fn typeset(
    engine: &MathEngine,
    task: &mut SessionMathTask,
    foreground_rgb: [u8; 3],
) -> Result<MathRaster, MathRenderError> {
    match task {
        SessionMathTask::Frozen(task) => render_detection_task(engine, task, foreground_rgb),
        SessionMathTask::Live(task) => render_live_detection_task(engine, task, foreground_rgb),
    }
}

pub fn render_detection_task(
    engine: &MathEngine,
    task: &mut DetectionTask,
    foreground_rgb: [u8; 3],
) -> Result<MathRaster, MathRenderError> {
    if !resolve_detection_task(task) {
        return Err(MathRenderError::NotDetected);
    }
    if task.span.kind == BlockKind::Table {
        return Ok(unrendered_table_raster());
    }
    let line = task
        .inputs
        .iter()
        .find(|input| input.id == task.transcript_id)
        .map_or("", |input| input.text.as_str());
    render_task_math(
        engine,
        &task.span,
        line,
        InlineGridGeometry {
            pane_columns: task.versions.layout.width_cells.get(),
            cell_width_subpixels: task.cell_width_subpixels,
            cell_height_subpixels: task.cell_height_subpixels,
            ascii_baseline_subpixels: task.ascii_baseline_subpixels,
        },
        terminal_math_render_key(task.versions.layout, foreground_rgb, task.span.mode)?,
    )
}

pub fn render_live_detection_task(
    engine: &MathEngine,
    task: &mut LiveDetectionTask,
    foreground_rgb: [u8; 3],
) -> Result<MathRaster, MathRenderError> {
    if !resolve_live_detection_task(task) {
        return Err(MathRenderError::NotDetected);
    }
    if task.span.kind == BlockKind::Table {
        if task.screen == ScreenId::Primary {
            extend_live_task_band(task);
        } else {
            task.band_start_row = task.start.row;
            task.band_end_row = task.end.row;
        }
        return Ok(unrendered_table_raster());
    }
    if task.screen == ScreenId::Primary {
        extend_live_task_band(task);
    } else {
        task.band_start_row = task.start.row;
        task.band_end_row = task.end.row;
    }
    // The **logical** line, not the row the run starts on: a run's byte offsets are offsets into
    // the string the detector proved it on, and the fold is free to have put the rest of it — or
    // all of it — on a later row (§4.6c). The line as the block's own pane reads it (R7), because
    // those are the bytes the offsets count.
    let pane_inputs = task
        .capture
        .pane_inputs(task.pane)
        .ok_or(MathRenderError::NotDetected)?;
    let line = live_snapshot_logical_line_text(pane_inputs, task.start.row);
    render_task_math(
        engine,
        &task.span,
        &line,
        InlineGridGeometry {
            // The width the producer of this line had to work in: its pane's (R10), which is the
            // grid's on every screen no frame cuts.
            pane_columns: task.pane.width().max(1),
            cell_width_subpixels: task.cell_width_subpixels,
            cell_height_subpixels: task.cell_height_subpixels,
            ascii_baseline_subpixels: task.ascii_baseline_subpixels,
        },
        terminal_math_render_key(task.layout, foreground_rgb, task.span.mode)?,
    )
}

/// Inline mathematics shares the pane's physical em. Display keeps its band-scaled 12 pt.
fn terminal_math_render_key(
    layout: LayoutKey,
    foreground_rgb: [u8; 3],
    mode: MathMode,
) -> Result<MathRenderKey, MathRenderError> {
    if mode == MathMode::Inline {
        bt_math::key_for_em_px(
            layout.font_size_subpixels as f32 / SUBPIXELS_PER_PX as f32,
            foreground_rgb,
            mode,
        )
        .ok_or(MathRenderError::InlineGeometry)
    } else {
        Ok(MathRenderKey {
            dpi_milli: layout.dpi_milli,
            font_milli_pt: NonZeroU32::new(12_000).expect("12 pt is non-zero"),
            foreground_rgb,
            mode,
        })
    }
}

/// A proven table's answer from the worker: the block, and no picture.
///
/// **The worker's half of a table is the proof, not the paint.** Everything expensive about
/// deciding that a header row stands over a delimiter row belongs off the presentation thread, and
/// it has just been done by `resolve_detection_task` above. What is left — how wide each column
/// has to be to hold its widest cell — is a question only the window's own shaper can answer, and
/// that shaper is on the thread this one exists to keep free. So the raster comes back empty and
/// `bt-app` measures the block before handing it to the session; see
/// `bt_render::TableBlockPaint`.
///
/// A zero extent rather than a guess: a size invented here would be the size the record kept if
/// the measuring step were ever skipped, and a wrong height is rows of transcript covered by
/// nothing.
fn unrendered_table_raster() -> MathRaster {
    MathRaster {
        rgba: Vec::new(),
        width_px: 0,
        height_px: 0,
        content_height_px: 0,
        ascent_px: 0.0,
        descent_px: 0.0,
        baseline_px: 0.0,
        render_time: Duration::ZERO,
        inline_runs: Vec::new(),
    }
}

/// The grid a run is being typeset into: the width its line folds at, and the box one cell is.
///
/// One value because they are one fact and are always read together: the row owns the ink box,
/// and its ASCII baseline supplies the preferred alignment and the composite anchor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InlineGridGeometry {
    pane_columns: u32,
    cell_width_subpixels: i64,
    cell_height_subpixels: i64,
    ascii_baseline_subpixels: i64,
}

fn render_task_math(
    engine: &MathEngine,
    span: &MathSpan,
    line: &str,
    grid: InlineGridGeometry,
    key: MathRenderKey,
) -> Result<MathRaster, MathRenderError> {
    let InlineGridGeometry {
        pane_columns,
        cell_width_subpixels,
        cell_height_subpixels,
        ascii_baseline_subpixels,
    } = grid;
    if span.mode == MathMode::Display {
        return engine.render(&span.render_source, key);
    }
    if ascii_baseline_subpixels <= 0 {
        // Inline placement is baseline-anchored. Without the renderer's measured ASCII baseline,
        // retaining source is the only geometry-safe outcome.
        return Err(MathRenderError::InlineGeometry);
    }
    let Some(first) = span.inline_runs.first() else {
        return Err(MathRenderError::NotDetected);
    };
    let first_byte =
        usize::try_from(first.byte_start).map_err(|_| MathRenderError::InlineGeometry)?;
    let Some(prefix) = line.get(..first_byte) else {
        return Err(MathRenderError::InlineGeometry);
    };
    let base_column = UnicodeWidthStr::width(prefix);
    let cell_width_px = (cell_width_subpixels.max(1) as f32 / SUBPIXELS_PER_PX as f32).max(1.0);
    let terminal_baseline_subpixels =
        ascii_baseline_subpixels.clamp(1, cell_height_subpixels.max(1));
    let terminal_descent_subpixels = cell_height_subpixels
        .max(1)
        .saturating_sub(terminal_baseline_subpixels);
    // Per-run geometry verdict. A run renders in place when its raster fits the cells its own
    // source occupies, and falls back to its source text when it does not — by itself, whole. The
    // rule was always right; applying it to the whole line was not, because one wide formula then
    // dragged every other formula on that row back to source with it. A rejected run contributes
    // nothing to the composite and its cells are never cleared, so what stands there is the
    // terminal text that was already correct.
    //
    // An *engine* error is deliberately not per-run: a source that does not compile is a fact
    // worth telling the user about, and it surfaces as this record's failure reason.
    let mut rendered = Vec::with_capacity(span.inline_runs.len());
    let baseline_px = (terminal_baseline_subpixels / SUBPIXELS_PER_PX) as u32;
    let row_height_px = baseline_px + (terminal_descent_subpixels / SUBPIXELS_PER_PX) as u32;
    let mut render_time = Duration::ZERO;
    let pane_columns = pane_columns.max(1) as usize;
    for (index, run) in span.inline_runs.iter().enumerate() {
        let start = usize::try_from(run.byte_start).map_err(|_| MathRenderError::InlineGeometry)?;
        let end = usize::try_from(run.byte_end).map_err(|_| MathRenderError::InlineGeometry)?;
        let (Some(before), Some(delimited)) = (line.get(..start), line.get(start..end)) else {
            return Err(MathRenderError::InlineGeometry);
        };
        let column_in_line = UnicodeWidthStr::width(before);
        let column = column_in_line.saturating_sub(base_column);
        // **The cells its own source occupies, on the row the picture is drawn on.** A logical
        // line is folded at the pane width, and a run the fold split owns cells on two rows while
        // its picture is one box drawn where the run begins — so the box it has to fit in ends at
        // that row's edge. Unfolded, the whole run is on one row and this is the source width
        // exactly, which is what it has always been.
        let available_cells = UnicodeWidthStr::width(delimited)
            .min(pane_columns.saturating_sub(column_in_line % pane_columns));
        let available_px = (available_cells as f32 * cell_width_px).floor() as u32;
        let fitted = render_inline_run_fitted(
            engine,
            &run.source,
            key,
            terminal_baseline_subpixels,
            terminal_descent_subpixels,
        )?;
        let Some(raster) = fitted else {
            continue;
        };
        if raster.width_px > available_px.max(1) {
            continue;
        }

        render_time = render_time.saturating_add(raster.render_time);
        let x = (column as f32 * cell_width_px).round().max(0.0) as u32;
        let run_index = u32::try_from(index).map_err(|_| MathRenderError::InlineGeometry)?;
        rendered.push((run_index, x, raster));
    }
    // `max` over an empty set is how "every run fell back" arrives here: the line keeps its source
    // in full, which is exactly the old whole-line outcome, now reached only when it is true.
    let width_px = rendered
        .iter()
        .map(|(_, x, raster)| x.saturating_add(raster.width_px))
        .max()
        .ok_or(MathRenderError::InlineGeometry)?;
    let height_px = rendered
        .iter()
        .map(|(_, _, raster)| {
            inline_run_top(raster, baseline_px, row_height_px).saturating_add(raster.height_px)
        })
        .max()
        .ok_or(MathRenderError::InlineGeometry)?;
    if !baseline_box_fits(
        height_px,
        baseline_px as f32,
        terminal_baseline_subpixels,
        terminal_descent_subpixels,
    ) {
        return Err(MathRenderError::InlineGeometry);
    }
    let len = (width_px as usize)
        .checked_mul(height_px as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(MathRenderError::InvalidDimensions)?;
    let mut rgba = vec![0_u8; len];
    let mut inline_runs = Vec::with_capacity(rendered.len());
    for (run, x, raster) in rendered {
        let y = inline_run_top(&raster, baseline_px, row_height_px);
        for row in 0..raster.height_px {
            let source_start = row as usize * raster.width_px as usize * 4;
            let source_end = source_start + raster.width_px as usize * 4;
            let target_start = ((y + row) as usize * width_px as usize + x as usize) * 4;
            let target_end = target_start + raster.width_px as usize * 4;
            rgba[target_start..target_end].copy_from_slice(&raster.rgba[source_start..source_end]);
        }
        inline_runs.push(InlineRunPlacement {
            run,
            x_px: x,
            width_px: raster.width_px,
        });
    }
    Ok(MathRaster {
        rgba,
        width_px,
        height_px,
        content_height_px: height_px,
        ascent_px: baseline_px as f32,
        descent_px: height_px.saturating_sub(baseline_px) as f32,
        baseline_px: baseline_px as f32,
        render_time,
        inline_runs,
    })
}

/// How far an inline run may shrink from the pane em to fit its row before readability is lost.
///
/// The same half-size floor display math stops at, and for the same reason: past it the formula is
/// no longer being made to fit, it is being made unreadable, and unreadable typesetting is worth
/// less than the honest source text the run falls back to.
const INLINE_READABLE_FLOOR_MILLI: u32 = 500;

/// The whole-pixel ascent and descent of an already positioned composite.
///
/// The renderer aligns this anchor with its measured ASCII baseline. Round ascent upward here
/// so accepting the box cannot later place a fractional pixel outside the row. Individual runs
/// are positioned within that box by `inline_run_top` before this final containment check.
fn inline_run_box(height_px: u32, baseline_px: f32) -> (u32, u32) {
    let ascent_px = baseline_px.ceil().max(0.0) as u32;
    (ascent_px, height_px.saturating_sub(ascent_px))
}

/// Keep the ASCII baseline when possible, otherwise move the ink just enough to stay in its row.
fn inline_run_top(raster: &MathRaster, baseline_px: u32, row_height_px: u32) -> u32 {
    baseline_px
        .saturating_sub(raster.baseline_px.ceil().max(0.0) as u32)
        .min(row_height_px.saturating_sub(raster.height_px))
}

/// Fit the complete ink height into the row. The baseline split is a placement preference,
/// not a reason to shrink: a subscript may borrow unused ascent without crossing a row boundary.
/// Whole-pixel budgets match the compositor, including fractional ASCII baseline measurements.
fn inline_fit_milli(
    raster: &MathRaster,
    terminal_ascent_subpixels: i64,
    terminal_descent_subpixels: i64,
) -> u32 {
    let height_px = terminal_ascent_subpixels.max(0) / SUBPIXELS_PER_PX
        + terminal_descent_subpixels.max(0) / SUBPIXELS_PER_PX;
    (height_px.saturating_mul(1000) / i64::from(raster.height_px.max(1))).clamp(0, 1000) as u32
}

/// Render one inline run at the largest size that fits the full row, down to half the pane em.
///
/// Shrinking rather than rejecting is the whole point. The gate this replaces was a straight
/// accept-or-fall-back on the natural size, which meant every construction taller than a line box —
/// `\frac`, `\sum_i`, `\hat{m}_t`, anything with a subscript under a descender — silently stayed
/// source text no matter how nearly it fit. Shrinking to the line box is the inline sibling of the
/// readable scaling display math already does to fit its band; the only difference is what the
/// budget is made of, a width there and the row's full height here.
///
/// The size is re-rendered rather than the raster resampled, because a formula scaled by the
/// rasterizer is a formula whose stems and fraction bars land between pixels. Typst is asked for
/// the smaller size and lays it out properly.
///
/// It iterates because glyph layout is not linear in font size — hinting, rule thicknesses and
/// script sizes all step — so the scale computed from one measurement may still overshoot by a
/// pixel. Each pass measures what it actually got and compounds the correction, which converges in
/// one or two passes and is bounded so a pathological source cannot spin. Falling through the floor
/// or running out of passes returns `None`: this run keeps its source text, alone, and the other
/// runs on the line are unaffected.
fn render_inline_run_fitted(
    engine: &MathEngine,
    source: &str,
    key: MathRenderKey,
    terminal_ascent_subpixels: i64,
    terminal_descent_subpixels: i64,
) -> Result<Option<MathRaster>, MathRenderError> {
    const MAX_ATTEMPTS: usize = 6;
    /// Every pass must shrink the raster by at least this much, so `MAX_ATTEMPTS` is a real bound
    /// rather than a hopeful one.
    ///
    /// The estimate is strictly decreasing on its own, so the loop cannot spin; what it cannot
    /// promise is *speed*. An estimate that stops a hair on the wrong side of an integer asks next
    /// time for a shrink of a fraction of a percent, and a run needing to lose most of a pixel can
    /// then use up every attempt it has going nowhere. Measured against the real 192-DPI budgets,
    /// either this floor or the integer-budget targeting in `inline_fit_milli` is enough to make
    /// the corpus converge and removing both together is what makes it fall back to source; they
    /// are kept together because one bounds the work and the other aims it.
    const MIN_STEP_MILLI: u32 = 20;
    let fits = |raster: &MathRaster| {
        inline_fit_milli(
            raster,
            terminal_ascent_subpixels,
            terminal_descent_subpixels,
        ) == 1000
    };
    let mut raster = engine.render(source, key)?;
    let mut applied_milli = 1000_u32;
    for _ in 0..MAX_ATTEMPTS {
        if fits(&raster) {
            return Ok(Some(raster));
        }
        let step_milli = inline_fit_milli(
            &raster,
            terminal_ascent_subpixels,
            terminal_descent_subpixels,
        );
        let estimate_milli =
            u32::try_from(u64::from(applied_milli) * u64::from(step_milli) / 1000).unwrap_or(0);
        let next_milli = estimate_milli.min(applied_milli.saturating_sub(MIN_STEP_MILLI));
        if next_milli < INLINE_READABLE_FLOOR_MILLI {
            return Ok(None);
        }
        applied_milli = next_milli;
        let Some(font_milli_pt) =
            u32::try_from(u64::from(key.font_milli_pt.get()) * u64::from(applied_milli) / 1000)
                .ok()
                .and_then(NonZeroU32::new)
        else {
            return Ok(None);
        };
        raster = engine.render(
            source,
            MathRenderKey {
                font_milli_pt,
                ..key
            },
        )?;
    }
    Ok(fits(&raster).then_some(raster))
}

fn baseline_box_fits(
    height_px: u32,
    baseline_px: f32,
    terminal_ascent_subpixels: i64,
    terminal_descent_subpixels: i64,
) -> bool {
    let (ascent_px, descent_px) = inline_run_box(height_px, baseline_px);
    i64::from(ascent_px).saturating_mul(SUBPIXELS_PER_PX) <= terminal_ascent_subpixels
        && i64::from(descent_px).saturating_mul(SUBPIXELS_PER_PX) <= terminal_descent_subpixels
}

#[cfg(test)]
#[path = "typeset_differential_tests.rs"]
mod differential;
