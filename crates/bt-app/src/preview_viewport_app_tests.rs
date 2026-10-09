//! **`preview_viewport`, as the application drives it.** Tests whose first assertion is about
//! `preview_viewport`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{
    a_decode, a_held_raster, found, found_in, found_in_package, free_fn_body, in_product,
    method_body, package_item_body, source_block,
};
use bt_source::{ItemQuery, Pattern, Scope, View, needle};

#[test]
fn input_routes_only_to_the_active_tab_and_background_output_survives_switching() {
    let mut writes = [Vec::new(), Vec::new()];
    let active = 1;
    active_item_mut(&mut writes, active).extend_from_slice(b"whoami\r");
    assert!(writes[0].is_empty());
    assert_eq!(writes[1], b"whoami\r");

    let mut sessions = [
        DualPlaneSession::new(NonZeroU32::new(20).unwrap(), NonZeroU32::new(2).unwrap()),
        DualPlaneSession::new(NonZeroU32::new(20).unwrap(), NonZeroU32::new(2).unwrap()),
    ];
    sessions[0].feed(b"kept in background").unwrap();
    let mut projection = sessions[0].new_projection(sessions[0].layout_key());
    sessions[0].refresh_projection(&mut projection);
    let frame = sessions[0].viewport_frame(&mut projection).unwrap();
    let visible = frame
        .cells
        .iter()
        .map(|cell| cell.text.as_str())
        .collect::<String>();
    assert!(visible.contains("kept in background"));
}

/// RED — **a 24 megapixel photograph is resampled on the decode worker, and
/// the window thread never touches it** (owner's ruling 2026-09-12).
///
/// The reduction is a full Lanczos3 pass over the native decode — tenths of
/// a second on a photograph, which is the measurement §7.1.3j (e) was
/// written about. On the thread that answers the keyboard that is a window
/// that has stopped answering it, and the hang watchdog would not even catch
/// it: that watchdog times one turn, and this would be one turn.
///
/// So the pin is structural, because the cost is structural. `bt_term` puts
/// the resample inside the file lane itself rather than owing it back as an
/// errand, and this window builds exactly one picture decoder — inside the
/// decoration worker's closure.
///
/// MUTATION: build a second one anywhere outside that closure, or lift the
/// one there is out of it, and the count or the bounds go red.
#[test]
fn the_reduction_happens_on_the_worker_not_the_window_thread() {
    // The whole of `main.rs`, and not a production half of it. Until 2026-09-18
    // the count was taken over the text above `mod tests`, because a fixture in
    // that module builds a decoder of its own on a thread that is nobody's
    // window — and that module is this file now, so the second decoder is no
    // longer in `main.rs` and there is no half left to cut. Counting the whole
    // of it is the stricter reading: a decoder built anywhere in that file, in
    // its product or in any of the fifty-four test modules it kept, is named
    // here. The needle is still spelled in two pieces, which now costs nothing.
    let decoder = concat!("InlineImage", "Decoder");
    assert_eq!(
        in_product(&found(
            needle!(Pattern::path("InlineImageDecoder::default")),
            View::Identifiers,
        )),
        1,
        "this window builds more than one picture decoder"
    );
    for (what, needle) in [
        (
            "the decoder",
            format!("let mut image_decoder = {decoder}::default();"),
        ),
        (
            "the inline lane",
            "image_decoder.decode(task.clone())".to_owned(),
        ),
        (
            "the picture lane",
            "peek_pixels(&mut image_decoder, &path)".to_owned(),
        ),
    ] {
        assert!(
            free_fn_body("run_decoration_worker").contains(&needle),
            "{what} stands outside the decoration worker's own body"
        );
    }
    // And the pass itself is made where the decode is, rather than handed
    // back to whoever asked: an answer that came back as an errand would be
    // an errand for the thread that asked for it.
    let lane = package_item_body("bt-term", &ItemQuery::function("decode_local_image_bytes"));
    assert!(
        lane.contains("scale_inline_image(&InlineImageScaleTask {"),
        "the reduction is not made where the decode is:\n{lane}"
    );
    assert_eq!(
        found_in_package(
            "bt-term",
            needle!(Pattern::call("decode_local_image_bytes")),
            View::Identifiers,
            Scope::Module("crate::inline_image".to_owned()),
        )
        .len(),
        2,
        "and that lane is declared once and called once — from the file read, \
             which is the worker's own"
    );
}

/// RED — **more pictures on the glass than the decode store can hold still
/// settles** (adversarial review 2026-09-11, row RB-1).
///
/// RED EVIDENCE. The loop above is only visible at more than one surface: a
/// decode landing for pane A evicts pane B's, the refit that the arrival
/// triggers walks **every** picture host ([`Runtime::refresh_preview_for_layout`]),
/// B finds its miss and asks, and B's answer evicts C's. Four 4000×4000 PNGs
/// are 256 MiB against a 192 MiB store, which is four screenshots opened at
/// once.
///
/// The fixture is that story at the size a test can hold: four surfaces,
/// each on its own file, a store that can carry one decode, and one decode
/// landing per round the way `complete_peek_image` lands them. The claim is
/// the count — **one read per surface, for ever**.
///
/// MUTATION: the same one. Read a miss as "never asked" and the count climbs
/// by one per surface per round, which is the report.
#[test]
fn a_visible_working_set_over_the_cache_settles() {
    const PIXELS: usize = 3 * 1024 * 1024;
    const BUDGET: u64 = 4 * 1024 * 1024;
    const NATIVE: (u32, u32) = (1024, 768);
    const DRAWN: (u32, u32) = (500, 375);
    const ROUNDS: usize = 12;
    let paths: Vec<PathBuf> = (0..4)
        .map(|index| PathBuf::from(format!(r"D:\shots\{index}.png")))
        .collect();
    let keys: Vec<String> = paths
        .iter()
        .map(|path| bt_term::normalized_local_image_path_key(path))
        .collect();
    let content = |index: usize| format!("content-{index}");
    let mut peek = PeekCache::with_budget(BUDGET);
    // What each surface is holding, and what it wants: the box is the same
    // every round, so the size it wants is the same every round.
    let mut held: Vec<Option<PeekThumbnail>> = paths.iter().map(|_| None).collect();
    let mut native: Vec<Option<(u32, u32)>> = vec![None; paths.len()];
    let mut asked = 0_usize;
    let mut inbox: Vec<usize> = Vec::new();

    for _ in 0..ROUNDS {
        if !inbox.is_empty() {
            let index = inbox.remove(0);
            peek.insert(
                keys[index].clone(),
                a_decode(&content(index), NATIVE, PIXELS),
            );
        }
        for index in 0..paths.len() {
            let pixels = surface_pixels(
                &mut peek,
                held[index]
                    .as_ref()
                    .map(|raster| raster.content_key.as_str()),
                native[index],
                None,
                &keys[index],
            );
            // `refit_preview_picture`'s own order: a surface with nothing to
            // draw at all never reaches the errand — it hides the picture,
            // asks, and is done for this frame.
            if let SurfacePixels::Nothing { asked: already } = pixels {
                if !already {
                    asked += 1;
                    inbox.push(index);
                    peek.insert(keys[index].clone(), PeekCacheEntry::Pending);
                }
                continue;
            }
            let target: PeekThumbnailTarget = (content(index), DRAWN.0, DRAWN.1);
            let exact = held[index]
                .as_ref()
                .is_some_and(|raster| raster.matches(&target));
            match picture_errand(&pixels, exact) {
                PictureErrand::Nothing | PictureErrand::Wait => {}
                PictureErrand::Read => {
                    asked += 1;
                    inbox.push(index);
                    peek.insert(keys[index].clone(), PeekCacheEntry::Pending);
                }
                // The scale lane answers, the way it answers on the window:
                // the surface is handed the raster and holds it.
                PictureErrand::Resample => {
                    held[index] = Some(a_held_raster(&content(index), DRAWN));
                    native[index] = Some(NATIVE);
                }
            }
        }
    }
    assert_eq!(
        asked,
        paths.len(),
        "{asked} reads for {} pictures over {ROUNDS} rounds: the store let \
             one go and the surface drawing it took that for never having asked",
        paths.len(),
    );
    for (index, raster) in held.iter().enumerate() {
        assert!(
            raster.is_some(),
            "picture {index} went blank when the store let its pixels go"
        );
    }
}

/// **A Chinese line folds where the wrap says it folds** (user report,
/// 2026-09-11).
///
/// Chinese has no spaces, so every character is a break opportunity and a
/// fold lands *between two ideographs* — which is only safe if the rows the
/// painter draws are cut at the columns the fold was computed in, and if the
/// letters then stand on those very cells
/// ([`bt_render::PreviewParagraph::cell_advance`]).
#[test]
fn a_chinese_line_folds_at_the_columns_the_wrap_names() {
    let line = "这一段完全没有空格";
    let source = source_block(0, 0, line);
    // A ten-cell-wide block: five ideographs a row.
    let box_of_block = [100.0, 40.0, 180.0, 100.0];
    let wrap = source.wrap(box_of_block[2] - box_of_block[0]);
    assert_eq!(wrap.rows(), 2, "nine ideographs are eighteen cells");
    assert_eq!(
        wrap.row_span(0),
        Some((0, 0, 10)),
        "the first row is the ten cells that fit",
    );
    assert_eq!(
        wrap.row_span(1),
        Some((0, 10, 19)),
        "and the rest is the second, whose far end runs one column past the \n             line to stand the break in",
    );
    let palette = bt_render::chrome_palette();
    let mut quads = Vec::new();
    let mut paragraphs = Vec::new();
    push_markdown_source_block(
        (&mut quads, &mut paragraphs),
        &source,
        None,
        &highlight::Highlighting::plain(),
        box_of_block,
        [0.0, 0.0, 1000.0, 1000.0],
        &palette,
    );
    let [first, second] = paragraphs.as_slice() else {
        panic!("one paragraph a row: {paragraphs:#?}");
    };
    let text = |paragraph: &bt_render::PreviewParagraph| {
        paragraph
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>()
    };
    assert_eq!(
        text(first),
        "这一段完全",
        "five ideographs on the first row"
    );
    assert_eq!(text(second), "没有空格", "and four on the second");
    for paragraph in [first, second] {
        assert_eq!(
            paragraph.cell_advance,
            Some(8.0),
            "each row is drawn on the very cells it was folded in",
        );
    }
    assert!(
        bt_unicode::text_width(&text(first)) <= 10,
        "and no row is wider than the block it folded inside",
    );
}

/// Red gates for every query-time renewal that was an entry in the baseline
/// fold. The names make a failure identify all entries sharing that source;
/// the behavioral tests in `pace`, `debounce`, and above pin their replacement.
#[test]
fn deadline_owners_do_not_manufacture_appointments_from_the_query_time() {
    for (owners, retired_form) in [
        ("startup poll", "map(|delay| now + delay)"),
        (
            "strip, tooltip, key hint, Cards hint, toast, command flash, command rails, terminal thumbs, file peek, file-peek close and dwell, float, formula tools, formula toggle, refused frame",
            "next_animation_frame(now)",
        ),
        (
            "strip animation",
            "now + self.window.frame_clock.interval()",
        ),
        ("pane motion fold entry", "pane_motion.deadline("),
    ] {
        assert_eq!(
            in_product(&found(needle!(Pattern::text(retired_form)), View::Raw)),
            0,
            "{owners} still renew from the fold query time via `{retired_form}`",
        );
    }
    let drag = method_body("Runtime", "drag_autoscroll_deadline");
    assert!(
        !drag.contains(".map_or(now") && !drag.contains(".unwrap_or(now"),
        "drag auto-scroll still renews from the fold query time:\n{drag}",
    );
    assert!(
        found_in(
            needle!("Instant::now() + SESSION_DEBOUNCE"),
            View::Raw,
            Scope::Module("crate::persist".to_owned()),
        )
        .is_empty(),
        "session save still renews from the time its deadline is queried",
    );
}
