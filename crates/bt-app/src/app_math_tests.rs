//! **The crate root: math and formulas.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{PtyPresentationHarness, method_body, row_box};
use std::time::Duration;

#[test]
fn osc_8_display_text_hits_the_real_target_uri() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(24).unwrap(), NonZeroU32::new(2).unwrap());
    session
        .feed(b"\x1b]8;;https://actual.example/login\x1b\\trusted label\x1b]8;;\x1b\\")
        .unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let mut frame = session.viewport_frame(&mut projection).unwrap();

    let hit = frame.hyperlink_at(0, 4).unwrap();
    assert_eq!(hit.uri, "https://actual.example/login");
    assert!(frame.underline_hyperlink(&hit));
    assert!(frame.cells[..13].iter().all(|cell| {
        cell.style
            .flags
            .contains(bt_transcript::CellFlags::UNDERLINE)
    }));
    assert!(
        !frame.cells[13]
            .style
            .flags
            .contains(bt_transcript::CellFlags::UNDERLINE)
    );
}

#[test]
fn a_late_zoom_reprint_keeps_the_last_formula_frame_until_exact_source_reanchors() {
    let start = Instant::now();
    let mut harness = PtyPresentationHarness::new(40, 24);
    harness
        .session
        .feed_at(b"intro\r\n$$x$$\r\nbarrier", start)
        .unwrap();
    assert_eq!(
        harness
            .session
            .advance_live_stability(start + bt_term::LIVE_MATH_STABLE_INTERVAL),
        1
    );
    let mut initial_task = harness.session.take_live_worker_task().unwrap();
    let initial_raster =
        render_live_detection_task(&MathEngine::new(), &mut initial_task, foreground_rgb())
            .expect("initial formula rasterizes");
    assert!(
        harness
            .session
            .complete_live_worker_result(initial_task, Ok(initial_raster))
    );
    assert!(harness.publish_pty_frame());
    assert!(harness.present_pending());
    assert_eq!(
        harness.last_presented.as_ref().unwrap().math_blocks.len(),
        1
    );

    // Match reconcile_authoritative_dpi: metrics, grid resize, then the new-DPI layout key.
    let zoom_at = start + Duration::from_millis(210);
    harness
        .session
        .set_cell_height_subpixels(NonZeroI64::new(14 * bt_viewport::SUBPIXELS_PER_PX).unwrap());
    harness
        .session
        .set_cell_width_subpixels(NonZeroI64::new(7 * bt_viewport::SUBPIXELS_PER_PX).unwrap());
    harness
        .session
        .set_ascii_baseline_subpixels(NonZeroI64::new(11 * bt_viewport::SUBPIXELS_PER_PX).unwrap());
    harness
        .session
        .resize_at(
            NonZeroU32::new(52).unwrap(),
            NonZeroU32::new(32).unwrap(),
            zoom_at,
        )
        .unwrap();
    harness.session.mark_pty_resize_requested_at(
        NonZeroU32::new(52).unwrap(),
        NonZeroU32::new(32).unwrap(),
        zoom_at,
    );
    harness.session.set_layout_key(bt_doc::LayoutKey {
        width_cells: NonZeroU32::new(52).unwrap(),
        dpi_milli: NonZeroU32::new(800).unwrap(),
        font_size_subpixels: 16 * 1024,
        font_rev: 1,
        theme_rev: harness.session.layout_key().theme_rev,
        lang_rev: harness.session.layout_key().lang_rev,
        profile_rev: harness.session.layout_key().profile_rev,
        line_wrapping: true,
    });
    let delayed_relayout = harness.session.take_live_worker_task().unwrap();
    assert!(harness.publish_pty_frame());
    assert!(harness.present_pending());
    assert!(
        harness
            .session
            .finish_resize_if_quiescent(zoom_at + Duration::from_millis(300))
            .unwrap()
    );

    let publications_before_gap = harness.publications;
    harness
        .session
        .feed_at(
            b"\x1b[2J\x1b[H\x1b[3Jintro\r\n$",
            zoom_at + Duration::from_millis(310),
        )
        .unwrap();
    assert!(
        !harness.publish_pty_frame(),
        "publish_frame_inner must skip the diagnosed incomplete reprint frame"
    );
    assert!(!harness.present_pending());
    assert_eq!(harness.publications, publications_before_gap);
    assert_eq!(
        harness.last_presented.as_ref().unwrap().math_blocks.len(),
        1,
        "the swapchain remains on the last complete formula frame"
    );

    harness
        .session
        .feed_at(b"$x$$\r\nbarrier", zoom_at + Duration::from_millis(324))
        .unwrap();
    let reanchor_published = harness.publish_pty_frame();
    assert!(
        !harness.projection.presentation_hold(),
        "exact-source re-anchor releases the hold immediately"
    );
    if reanchor_published {
        assert!(harness.present_pending());
    }
    assert_eq!(
        harness.last_presented.as_ref().unwrap().math_blocks.len(),
        1
    );

    // The re-anchored stale frame may be content-identical to last_presented and therefore need
    // no publication. Either way, no incomplete grid frame entered the presentation slot.
    drop(delayed_relayout);
}

#[test]
fn copy_on_select_writes_nonempty_text_and_keeps_the_selection() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
    session.feed(b"drag me").unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let selection = ViewSelection {
        start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
        end: frame.anchor_at(0, 6, Bias::After).unwrap().unwrap(),
    };
    session.set_view_selection(Some(selection));
    let mut clipboard = String::new();

    assert!(write_selection_text(&session, true, |text| {
        clipboard.push_str(text);
        Ok(())
    }));
    assert_eq!(clipboard, "drag me");
    assert_eq!(session.selection_text().as_deref(), Some("drag me"));
}

#[test]
fn copy_on_select_does_not_touch_the_clipboard_for_an_empty_selection() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
    session.feed(b"click").unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let anchor = frame.anchor_at(0, 2, Bias::Before).unwrap().unwrap();
    session.set_view_selection(Some(ViewSelection {
        start: anchor.clone(),
        end: anchor,
    }));
    let mut writes = 0;

    assert!(!write_selection_text(&session, true, |_| {
        writes += 1;
        Ok(())
    }));
    assert_eq!(writes, 0);
}

#[test]
fn unavailable_clipboard_during_copy_on_select_keeps_the_selection() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
    session.feed(b"retry me").unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    session.set_view_selection(Some(ViewSelection {
        start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
        end: frame.anchor_at(0, 7, Bias::After).unwrap().unwrap(),
    }));

    assert!(!write_selection_text(&session, true, |_| {
        Err(anyhow!("injected clipboard owner contention"))
    }));
    assert_eq!(session.selection_text().as_deref(), Some("retry me"));
}

/// PIN (ticket #62) — **`Copy` on the menu is copy-on-select's own door: the
/// same bytes, and the selection is still there afterwards.**
///
/// The contrast with `Ctrl+C`'s door is the content. A keystroke that ends a
/// gesture may reasonably clear the highlight; a menu row reached by
/// *pointing at the highlight* may not, because the thing the reader aimed at
/// would vanish as the reward for having used it. Both doors are exercised on
/// one selection here so the difference cannot be read as an accident of two
/// separate fixtures.
///
/// MUTATION: point the `Copy` row at `copy_selection` and the second half of
/// this test goes red on the selection that is no longer standing.
#[test]
fn the_menus_copy_is_copy_on_selects_door_and_leaves_the_selection_standing() {
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(12).unwrap(), NonZeroU32::new(2).unwrap());
    session.feed(b"copy me").unwrap();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let selection = ViewSelection {
        start: frame.anchor_at(0, 0, Bias::Before).unwrap().unwrap(),
        end: frame.anchor_at(0, 6, Bias::After).unwrap().unwrap(),
    };
    session.set_view_selection(Some(selection.clone()));
    projection.set_selection(Some(selection));

    // The menu row's door, which is the one copy-on-select spends.
    let mut menu_clipboard = String::new();
    assert!(write_selection_text(&session, true, |text| {
        menu_clipboard.push_str(text);
        Ok(())
    }));
    assert_eq!(menu_clipboard, "copy me");
    assert!(
        session.view_selection().is_some(),
        "the row was reached by pointing at this selection; it stays"
    );
    assert!(projection.selection().is_some());

    // The keyboard's door, on the same selection, for the difference.
    let mut keyboard_clipboard = String::new();
    assert!(copy_selection(&mut session, &mut projection, |text| {
        keyboard_clipboard.push_str(text);
        Ok(())
    }));
    assert_eq!(
        keyboard_clipboard, menu_clipboard,
        "the two doors put the same bytes on the clipboard"
    );
    assert!(session.view_selection().is_none());
}

#[test]
fn the_sentence_a_crash_raises_names_the_file_it_left() {
    let path = std::env::temp_dir().join("folio-panic.log");
    let text = panic_alert_text(&path);
    assert!(
        text.contains(&path.display().to_string()),
        "the reader is told where the file is: {text}"
    );
    assert!(
        text.contains(&version::banner()),
        "and which build left it: {text}"
    );

    // **And it is raised, not filed.** Once the front door has closed this
    // process's `stdout` is `diagnostics.log`, so "did the write succeed"
    // is not the question — the write always succeeds and reaches nobody.
    //
    // MUTATION: return `true` for `Channel::Log` and the box stops
    // appearing in exactly the case it was written for; the crash goes into
    // the log twice and the user still watches the window vanish.
    use diagnostics::Channel;
    assert!(
        !diagnostics::a_screen_is_watching(Some(Channel::Log)),
        "a resident run's stdout is its own log file"
    );
    assert!(
        !diagnostics::a_screen_is_watching(Some(Channel::Nowhere)),
        "and a run with nowhere to write has nowhere to write"
    );
    assert!(
        diagnostics::a_screen_is_watching(Some(Channel::Console)),
        "a run that kept its console kept somebody looking at it"
    );
    assert!(
        diagnostics::a_screen_is_watching(None),
        "and before the front door closes, stdout is still the caller's"
    );
}

/// RED — **a page keeps the formulas it was handed when the formula cache
/// lets them go** (adversarial review 2026-09-11, row RB-2; §7.1.3u ③).
///
/// RED EVIDENCE. [`PreviewMathCache`] is bounded in bytes and evicts by last
/// use; `resolve_document_math` had no standing-answer parameter, unlike its
/// twin for pictures, so it asked the engine for every key the cache had no
/// entry for. A page whose distinct rasters are worth more than
/// [`PREVIEW_MATH_CACHE_BUDGET_BYTES`] therefore evicted one it was drawing
/// on every arrival and typeset it again — §7.1.3u's own report, one lane
/// over. It is reachable because the cache is window-wide and the size is
/// part of the key, so each preview zoom step mints a fresh set of every
/// formula on the page.
///
/// MUTATION: drop the standing arm from [`answer_one_formula`]'s `None`
/// branch and the evicted formula is asked for again, which is the first
/// turn of the loop.
#[test]
fn a_page_keeps_its_formula_answers_across_an_eviction() {
    /// Three of these do not fit under the budget; two do.
    const BYTES: usize = 20 * 1024 * 1024;
    let ink = [17_u8, 18, 19];
    let key = |source: &str| PreviewMathKey {
        source: source.to_owned(),
        mode: MathMode::Display,
        em_milli_px: math_em_milli(13.0),
        foreground_rgb: ink,
    };
    let mut cache = PreviewMathCache::default();
    let mut page = DocumentMath::default();
    let sources = ["a", "b", "c"];
    for source in sources {
        let picture = PreviewMathPicture {
            key: format!("preview-math:{source}"),
            rgba: Arc::from(vec![0_u8; BYTES].into_boxed_slice()),
            width_px: 100,
            height_px: 50,
            baseline_px: 10.0,
        };
        cache.land(key(source), PreviewMathArtifact::Ready(picture.clone()));
        // What the page was handed, which is what it is drawing.
        page.insert(&key(source), picture);
    }
    assert_eq!(
        cache.entries.len(),
        2,
        "the fixture is a cache that cannot hold the page: {} bytes resident",
        cache.resident_bytes,
    );
    let gone = sources
        .iter()
        .find(|source| !cache.entries.contains_key(&key(source)))
        .expect("the budget let one of them go");

    let mut needs_typesetting = false;
    let answer = answer_one_formula(&mut cache, &page, &key(gone), &mut needs_typesetting);
    assert!(
        answer.is_some(),
        "the page was told what `{gone}` looks like and is still drawing it"
    );
    assert!(
        !needs_typesetting,
        "but it asked the engine to set `{gone}` again, because the cache \
             no longer holds the pixels it was given"
    );

    // And the one thing a standing answer may not survive: the page being
    // set in another ink, which is a different picture of the same formula.
    let mut in_another_theme = key(gone);
    in_another_theme.foreground_rgb = [200, 200, 200];
    let mut needs_typesetting = false;
    let answer = answer_one_formula(&mut cache, &page, &in_another_theme, &mut needs_typesetting);
    assert!(
        answer.is_none() && needs_typesetting,
        "a formula set in a new ink is a new picture"
    );
}

/// RED — **an eviction is not an invalidation of the documents that still
/// hold the answer** (adversarial review 2026-09-11, row RB-2).
///
/// RED EVIDENCE. `PreviewMathCache::evict_to_budget` bumped `generation`,
/// which is part of [`PageArtKey`] and therefore re-keys every page in the
/// window — a full re-flow each. That was right while the cache was a page's
/// only copy of a formula: the block went back to standing on its source
/// text. It is exactly wrong once a page carries what it was handed, and it
/// is the engine of the loop: land → generation++ → rebuild → miss on the
/// key just evicted → ask → land → evict the next.
///
/// MUTATION: tick the generation in `evict_to_budget` again and the count
/// below is one higher per eviction, which is one whole-document re-flow per
/// eviction for a page that has not changed.
#[test]
fn an_eviction_does_not_invalidate_a_document_that_still_holds_the_answer() {
    const BYTES: usize = 20 * 1024 * 1024;
    let mut cache = PreviewMathCache::default();
    for source in ["a", "b", "c"] {
        cache.land(
            PreviewMathKey {
                source: source.to_owned(),
                mode: MathMode::Display,
                em_milli_px: math_em_milli(13.0),
                foreground_rgb: [0, 0, 0],
            },
            PreviewMathArtifact::Ready(PreviewMathPicture {
                key: format!("preview-math:{source}"),
                rgba: Arc::from(vec![0_u8; BYTES].into_boxed_slice()),
                width_px: 100,
                height_px: 50,
                baseline_px: 10.0,
            }),
        );
    }
    assert_eq!(
        cache.entries.len(),
        2,
        "the fixture is a cache that had to let one go"
    );
    assert_eq!(
        cache.generation, 3,
        "one tick per formula that arrived, and none at all for the \
             eviction: a page still holding the answer has not changed, and \
             re-keying it is a re-shape of every paragraph on it for nothing",
    );
}

/// PIN (same report) — **every formula on a page reaches the engine, from
/// wherever on the page it stands.**
///
/// A formula is a run of inline text, and a run of inline text can be in a
/// heading, a list item, a quoted line or a table cell as easily as in a
/// paragraph — there is one inline parser and it serves all of them. A walk
/// that only looked at paragraphs would leave every other one standing on its
/// source for good, with nobody ever asking for it. A fence is the one place
/// a dollar is never markup, and it is excluded here as well as by the parser.
///
/// MUTATION: drop any arm of `document_formulas` and its member goes missing.
#[test]
fn every_formula_on_the_page_is_asked_for_wherever_it_stands() {
    let blocks = preview::parse_markdown(
        "# The $\\alpha$ chapter\n\
             \n\
             Prose with $x^2$ in it.\n\
             \n\
             - an item with $y^2$\n\
             \n\
             > a quote with $z^2$\n\
             \n\
             | a | b |\n\
             |---|---|\n\
             | $c^2$ | plain |\n\
             \n\
             $$\n\
             \\int_0^1 f\n\
             $$\n\
             \n\
             ```text\n\
             $not^2$\n\
             ```\n",
    );
    let metrics = seats::preview_markdown_metrics(1.0);
    let mut found = document_formulas(&blocks, metrics)
        .into_iter()
        .map(|(source, mode, em_px)| {
            (
                source,
                matches!(mode, MathMode::Display),
                math_em_milli(em_px),
            )
        })
        .collect::<Vec<_>>();
    found.sort();
    let body = math_em_milli(metrics.font_size);
    assert_eq!(
        found,
        vec![
            (
                "\\alpha".to_owned(),
                false,
                math_em_milli(metrics.heading_font(1))
            ),
            ("\\int_0^1 f".to_owned(), true, body),
            ("c^2".to_owned(), false, body),
            ("x^2".to_owned(), false, body),
            ("y^2".to_owned(), false, body),
            ("z^2".to_owned(), false, body),
        ],
        "five inline formulas, one display block, nothing from the fence — and \
             the one in the masthead asked for at the masthead's size",
    );
}

/// **Where a pointer lands when it is not on any letter at all.**
///
/// A drag does not stay inside the column of prose it started in: it goes
/// out into the margin, above the first block and below the last, and every
/// one of those has to be an answer or the selection stops growing at the
/// edge of the text.
///
/// MUTATION: return `None` for a point outside every rectangle and a drag
/// out of the pane freezes.
#[test]
fn a_point_off_the_text_lands_on_the_piece_it_is_nearest_to() {
    let boxes = vec![
        row_box(preview_select::Place::new(0, 0, 0), 5, "alpha", 0.0, 20.0),
        row_box(preview_select::Place::new(1, 0, 0), 4, "beta", 40.0, 60.0),
    ];
    assert_eq!(preview_text_box_at(&boxes, 30.0, 10.0), Some(0), "on it");
    assert_eq!(
        preview_text_box_at(&boxes, 300.0, 10.0),
        Some(0),
        "out in the margin beside it",
    );
    assert_eq!(
        preview_text_box_at(&boxes, 30.0, -50.0),
        Some(0),
        "above the whole page",
    );
    assert_eq!(
        preview_text_box_at(&boxes, 30.0, 500.0),
        Some(1),
        "below the whole page",
    );
    assert_eq!(
        preview_text_box_at(&boxes, 30.0, 31.0),
        Some(1),
        "in the gap between two blocks, on the nearer one",
    );
    assert_eq!(preview_text_box_at(&[], 0.0, 0.0), None, "an empty page");
    // Two cells side by side on one row: the gap between them belongs to the
    // one it is nearer to, not to whichever was laid out first.
    let mut left = row_box(preview_select::Place::new(2, 0, 0), 4, "name", 0.0, 20.0);
    left.rect = [0.0, 0.0, 100.0, 20.0];
    let mut right = row_box(preview_select::Place::new(2, 1, 0), 4, "size", 0.0, 20.0);
    right.rect = [140.0, 0.0, 240.0, 20.0];
    let row = vec![left, right];
    assert_eq!(preview_text_box_at(&row, 110.0, 10.0), Some(0));
    assert_eq!(preview_text_box_at(&row, 135.0, 10.0), Some(1));
    assert_eq!(preview_text_box_at(&row, 400.0, 10.0), Some(1));
}

/// **An inline formula's placeholder is one thing to the shaper and a
/// whole `$x$` to the document**, and the two byte spaces rejoin after it.
///
/// MUTATION: carry the difference forwards with the wrong sign and every
/// word after a formula in the same paragraph selects the word beside it.
#[test]
fn an_inline_formulas_placeholder_stands_for_the_whole_of_its_source() {
    // `see $x^2$ there` — the middle run is drawn as a picture, so the
    // shaper is handed one non-breaking space where the document has six
    // bytes.
    let piece = PreviewTextPiece {
        at: preview_select::Place::new(0, 0, 0),
        len: "see $x^2$ there".len(),
        lead: 0,
        atoms: vec![PreviewTextAtom {
            shaped: (4, 6),
            doc: (4, 9),
        }],
        atomic: false,
    };
    assert_eq!(piece.doc_offset(0), 0, "before it, the two agree");
    assert_eq!(piece.doc_offset(4), 4, "and at its own first byte");
    assert_eq!(piece.doc_offset(6), 9, "past it, the document has run on");
    assert_eq!(
        piece.doc_offset(8),
        11,
        "and every byte after it keeps that distance",
    );
    assert_eq!(
        piece.doc_offset(5),
        4,
        "an offset the shaper cannot return — inside the one placeholder \
             glyph — is the formula's own beginning rather than a byte of it",
    );
    // Backwards: a range that cuts into the formula covers all of it.
    assert_eq!(piece.shaped_range(0..6, 12), 0..6);
    assert_eq!(piece.shaped_range(0..11, 12), 0..8);
    assert_eq!(piece.shaped_range(9..15, 12), 6..12);
}

/// PIN — a composition **opens a space in the line** rather than being
/// painted over it, and the IME's caret stands inside it.
///
/// The terminal's machine writes preedit into grid cells; this surface has
/// none, so the form is copied and the mechanism cannot be. This used to
/// copy it by drawing the row whole and laying the letters on top over an
/// opaque patch of the *pane's* ground — which real-machine capture
/// (2026-08-13) showed fails twice over: the patch hides only the cells the
/// composition itself covers, so every character after the caret stays where
/// it was and the composition and the rest of the word share cells neither
/// can be read in; and on a float, whose body stands on the window's face
/// rather than a pane's, the patch is the wrong colour as well. Splitting the
/// row cures both, because there is then nothing left to hide.
///
/// Asserted at **two geometries** — a docked pane's body and a float's, which
/// is the surface the colour half of the bug only showed on — because a body
/// that is a pure function of its rectangle must not have learned which kind
/// of container it is in.
///
/// MUTATIONS: draw the row unsplit (one run over `from..to`) and the tail's
/// offset assertion goes red; put the ground quad back and the "no pane ink"
/// assertion does; leave the caret at the document's column and the last one
/// does.
#[test]
fn a_composition_in_the_preview_opens_a_space_in_the_line_it_lands_in() {
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_text_metrics(1.0);
    let lines = vec!["one".to_owned(), "paragraph".to_owned()];
    let advance = 8.0;
    let wrap = preview_edit::WrapLayout::unwrapped(&lines);
    // Mid-word, which is the case that was unreadable: four characters of
    // "paragraph" stand before the caret and five after it.
    let paint = PreviewEditPaint {
        bands: Vec::new(),
        caret: Some((1, 4)),
        caret_width: 1.0,
        preedit: Some(PreviewPreedit {
            row: 1,
            column: 4,
            text: "ni".to_owned(),
            columns: 2,
            caret_columns: 1,
        }),
    };
    // A pane's body and a float's — the same builder, two containers.
    for (surface, body) in [
        ("a docked pane", [0.0, 0.0, 400.0, 200.0]),
        ("a preview float", [1866.0, 145.0, 2266.0, 345.0]),
    ] {
        let geometry = seats::preview_mono_geometry(
            body,
            metrics,
            metrics.line_height * 2.0,
            12,
            advance,
            [0.0, 0.0],
        );
        let built = build_preview_text_body(
            &geometry,
            &lines,
            &wrap,
            &highlight::Highlighting::default(),
            advance,
            Some(&paint),
            &palette,
        );
        let row = geometry.line_rect(1);
        let at = |columns: f32| row[0] + advance * columns;
        let run_at = |text: &str| {
            built
                .paragraphs
                .iter()
                .find(|paragraph| paragraph.runs.iter().any(|run| run.text == text))
                .unwrap_or_else(|| panic!("{surface}: no run drawing {text:?}"))
                .rect[0]
        };

        // The line is two runs with the composition's own width between them.
        assert_eq!(run_at("para"), at(0.0), "{surface}: the head stays put");
        assert_eq!(
            run_at("graph"),
            at(6.0),
            "{surface}: and the tail is pushed right by the whole composition \
                 — four columns of head plus its own two"
        );
        let letters = run_at("ni");
        assert_eq!(
            letters,
            at(4.0),
            "{surface}: the letters land at the caret, in the space just opened"
        );
        assert!(
            letters + advance * 2.0 <= run_at("graph") + 0.001,
            "{surface}: and stop before the tail begins — nothing overlaps"
        );

        // Nothing is masked, so no pane-only ink is painted at all: that is
        // the half of the bug a float showed and a pane hid.
        assert!(
            !built
                .quads
                .iter()
                .any(|quad| quad.color == palette.seat_body),
            "{surface}: a split line needs no ground under the composition"
        );
        let rule = built
            .quads
            .iter()
            .find(|quad| quad.color == palette.preview_body_text)
            .unwrap_or_else(|| panic!("{surface}: underlined, as a preedit is"));
        assert_eq!(
            rule.rect[3], row[3],
            "{surface}: along the bottom of its line"
        );
        let caret = built
            .quads
            .iter()
            .find(|quad| quad.color == palette.preview_caret)
            .unwrap_or_else(|| panic!("{surface}: the caret is drawn"));
        assert_eq!(
            caret.rect[0],
            at(5.0),
            "{surface}: inside the composition, where the IME says it is"
        );
    }
}

/// PIN (user ruling 2026-08-25) — **every control the breadcrumb row grew
/// says what it is, and the two that name a place say the whole place.**
///
/// 「预览头与地址行/面包屑行的新控件全部无 tooltip」 was the report. The two
/// interesting halves are the ones a static string could not have covered:
/// a segment's tip is the **whole path** it stands for, because the word
/// drawn in it is only that path's last piece; and the `…`'s is the run of
/// levels it is hiding, **root first**, because it is a stretch of a path
/// being read left to right — the opposite order from the *menu* behind it,
/// which is a list of destinations.
///
/// RED EVIDENCE (2026-08-25): none of these anchors existed, so hovering any
/// of the five produced nothing at all.
///
/// MUTATIONS: give the flip one wording for both faces; give `⧉` one
/// wording for both rows; turn the fold's tip round.
#[test]
fn the_breadcrumb_rows_controls_each_say_what_they_are() {
    let path = PathBuf::from(r"D:\Developer\folio-terminal\test-assets\huge.txt");
    let segments = crumb_segments(&path);
    let folded = [1, 2, 3];
    let tip = |tip, kind, to_source| {
        preview_rail_tip_text(
            tip,
            kind,
            &segments,
            &folded,
            to_source,
            "Read-only over 8 MB",
        )
    };
    assert_eq!(
        tip(
            seats::PreviewRailTip::Crumb(2),
            seats::PreviewRailKind::Crumbs,
            false
        ),
        r"D:\Developer\folio-terminal",
        "a segment names the whole place, not the word drawn in it"
    );
    assert_eq!(
        tip(
            seats::PreviewRailTip::Fold,
            seats::PreviewRailKind::Crumbs,
            false
        ),
        format!(
            "Developer {sep} folio-terminal {sep} test-assets",
            sep = seats::PREVIEW_CRUMB_SEPARATOR
        ),
        "and the chip names the run it stands for, the way the row reads"
    );
    // `⧉` is one glyph on both rows and two sentences, which is the whole
    // reason it needed a tip.
    assert_ne!(
        tip(
            seats::PreviewRailTip::Copy,
            seats::PreviewRailKind::Crumbs,
            false
        ),
        tip(
            seats::PreviewRailTip::Copy,
            seats::PreviewRailKind::Address,
            false
        ),
    );
    // And `</>` names the destination rather than the state, because the
    // button changes.
    assert_ne!(
        tip(
            seats::PreviewRailTip::Flip,
            seats::PreviewRailKind::Crumbs,
            true
        ),
        tip(
            seats::PreviewRailTip::Flip,
            seats::PreviewRailKind::Crumbs,
            false
        ),
    );
    for text in [
        tip(
            seats::PreviewRailTip::Open,
            seats::PreviewRailKind::Crumbs,
            false,
        ),
        tip(
            seats::PreviewRailTip::Flip,
            seats::PreviewRailKind::Crumbs,
            true,
        ),
        tip(
            seats::PreviewRailTip::Copy,
            seats::PreviewRailKind::Address,
            false,
        ),
    ] {
        assert!(!text.is_empty(), "a control that registers has words");
    }
}

/// RED GATE (§13.40) — **a formula that lands rebuilds whatever was
/// standing on its source text**, the pane as well as the card.
///
/// The Mac's reading sweep opened a page holding `$$\int_0^1 x\,dx$$` and
/// photographed `\int_0^1 x\,dx` printed where the integral belonged, on
/// every run, thirty seconds in and after a press. `BT_PREVIEW_TRACE` says
/// what happened: `math formulas=1 drawn=0 asked=1 worker=1` at 111 ms, the
/// page built at 175 ms, `math answered set=1` at **1684 ms** — and then no
/// `build` line ever again. The picture was set, it landed, and nothing
/// asked the page to be laid out a second time, so
/// `PreviewMathCache::generation` — which exists for exactly that next
/// layout — was read by nobody.
///
/// This is a race and not a platform: `refresh_preview_body` is reached
/// from a resize, a scroll, an edit, an open and a palette change, and a
/// formula arriving is none of them, so the picture only ever reached the
/// glass because the page had not settled yet. The port is what lost the
/// race — a Mac's first typesetting of a session takes about 1.7 s against
/// a startup that settles inside 200 ms — and a slow first formula on any
/// machine is the same defect.
///
/// A **source pin**, because the shape being asserted is a call in a method
/// that needs a window, a device and a worker to run at all, and the three
/// facts worth keeping are all in the text: that the rebuild is asked for,
/// that it is gated on a *picture* rather than on `changed` (a refusal
/// leaves the block drawing exactly what it was drawing), and that it
/// happens before the publish rather than after it.
///
/// MUTATIONS:
/// ① drop the `refresh_preview_body` call — this goes red, and a page whose
///    formula is slow stands on its LaTeX for as long as the reader leaves
///    it alone;
/// ② gate it on `changed` instead — the flag assertion goes red, and every
///    refusal costs a rebuild of every document in the window;
/// ③ move it below the publish — the order assertion goes red, and the
///    frame that goes out is one behind the picture.
#[test]
fn a_formula_that_lands_rebuilds_the_page_that_was_standing_on_its_source() {
    let body = method_body("Runtime", "apply_math_results");
    let rebuild = body
        .find("self.refresh_preview_body();")
        .expect("a landed picture asks the page to be laid out again");
    let publish = body
        .find("self.publish_frame(FrameTrigger {")
        .expect("and the window still owes a frame after it");
    assert!(
        rebuild < publish,
        "the rebuild goes before the publish, so the frame that goes out \
             carries the new body rather than the one after it"
    );
    let guard = body[..rebuild]
        .rfind("if picture_landed {")
        .expect("the rebuild is gated on a picture");
    assert!(
        guard < rebuild && body[guard..rebuild].find("changed").is_none(),
        "gated on a picture rather than on `changed`: a refusal leaves the \
             block drawing exactly what it was drawing and owes no rebuild"
    );
    assert!(
        body.contains("picture_landed |= matches!(artifact, PreviewMathArtifact::Ready(_));"),
        "and the flag is set by a picture, never by a refusal"
    );
}

/// RED — **the texture a page is cached under names the file, its version,
/// which page it is, and its size**, so the shared GPU cache can never serve
/// one for another.
///
/// MUTATION: drop any one of the four from the key and the matching
/// assertion goes red — which on screen is the wrong document, yesterday's
/// document, **page 1 in every slot of the column** (the one the 2026-08-26
/// ruling added), or a page rastered for another monitor drawn soft on this
/// one.
#[test]
fn one_page_texture_is_one_page_of_one_file_at_one_version_at_one_size() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let then = now + Duration::from_secs(1);
    let key =
        |path: &str, mtime, page, w, h| peek_page_texture_key(Path::new(path), mtime, page, w, h);
    let base = key(r"D:\reports\q3.pdf", Some(now), 0, 124, 160);
    assert_ne!(base, key(r"D:\reports\q4.pdf", Some(now), 0, 124, 160));
    assert_ne!(base, key(r"D:\reports\q3.pdf", Some(then), 0, 124, 160));
    assert_ne!(base, key(r"D:\reports\q3.pdf", Some(now), 0, 248, 320));
    assert_ne!(
        base,
        key(r"D:\reports\q3.pdf", Some(now), 1, 124, 160),
        "two pages of one document are two pictures"
    );
    assert_eq!(base, key(r"D:\reports\q3.pdf", Some(now), 0, 124, 160));
    // A filesystem that will not say when a file was written gets a name of
    // its own rather than one that could collide with a real stamp — such a
    // file is re-drawn on every question anyway.
    assert_ne!(base, key(r"D:\reports\q3.pdf", None, 0, 124, 160));
    assert_ne!(
        key(r"D:\reports\q3.pdf", None, 0, 124, 160),
        key(
            r"D:\reports\q3.pdf",
            Some(SystemTime::UNIX_EPOCH),
            0,
            124,
            160
        )
    );
}

/// RED — **the texture a video's frame is cached under names the file, its
/// version and its size** (user ruling 2026-08-27; §7.23).
///
/// A picture file's texture is named by a hash of its own bytes, and a frame
/// cannot be: the bytes it came from are a container this process never held
/// whole, and re-decoding a hundred megabytes to name the eight it produced
/// would be the read the cache exists to avoid. So it is named the way a
/// PDF's page is — by the file, by when the file was last written, and by
/// the box it came back in.
///
/// **The modification time is the load-bearing field.** The window keeps a
/// video's pixels for as long as its decode cache holds them; a key without
/// a version would go on naming the same texture after the file underneath
/// had been re-recorded, and the GPU cache would serve yesterday's frame for
/// a clip that no longer contains it.
///
/// RED GATE: drop the stamp from [`video_frame_texture_key`] and the second
/// assertion goes red — on screen, a capture overwritten while the window
/// was open goes on showing the frame it used to have.
#[test]
fn one_video_texture_is_one_file_at_one_version_at_one_size() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let then = now + Duration::from_secs(1);
    let key = |path: &str, mtime, w, h| video_frame_texture_key(Path::new(path), mtime, w, h);
    let base = key(r"D:\shots\clip.mp4", Some(now), 1920, 1080);
    assert_ne!(base, key(r"D:\shots\other.mp4", Some(now), 1920, 1080));
    assert_ne!(
        base,
        key(r"D:\shots\clip.mp4", Some(then), 1920, 1080),
        "a capture re-recorded under the window is a different picture"
    );
    assert_ne!(base, key(r"D:\shots\clip.mp4", Some(now), 960, 540));
    assert_eq!(base, key(r"D:\shots\clip.mp4", Some(now), 1920, 1080));
    // A filesystem that will not say gets a name of its own rather than one
    // that could collide with a real stamp.
    assert_ne!(base, key(r"D:\shots\clip.mp4", None, 1920, 1080));
    // And it cannot collide with a page's, which shares the cache.
    assert!(base.starts_with("video-frame:"));
    assert!(
        peek_page_texture_key(Path::new(r"D:\shots\clip.mp4"), Some(now), 0, 1920, 1080) != base
    );
}
