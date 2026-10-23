//! **The rendered page's caret, driven** (T-HEADLESS-RUNTIME-SEAM): the press, the release, the
//! keys and the blur that put a caret in a Markdown page and take it out again, run on a headless
//! window through the window's own methods. **And the peek completions, driven**
//! (T-PEEK-COMPLETIONS-RUNTIME-PINS): a file's read answered on the real computation lane and
//! taken by the window as a `MathReady` turn takes it.

use super::*;
use crate::headless::{HeadlessFolio, WindowEffect, key_down};
use crate::preview_press::Spend;
use crate::preview_select::{Grain, Selection};
use crate::test_support::{
    buffer_saying, host_spelling, png_bytes, scratch_png_named, seat_of, tab_with_a_preview,
};
use crate::{PreviewSurface, TabId};

/// A heading and three paragraphs: the second with a bold word whose marks the rendered face
/// hides and the source face shows, the third with a link.
const PAGE: &str = "# 标题 Title\n\n第一段 first paragraph.\n\n第二段 **粗体** bold here.\n\n\
                    第三段 [链接 link](https://example.com/) third.\n";

/// A window holding one Markdown page in a preview pane beside a shell, drawn once.
fn drawn_page(tag: &str) -> (HeadlessFolio, PreviewSurface) {
    let path = host_spelling(r"C:\w\说明.md");
    let (tab, seat) = tab_with_a_preview(1, vec![buffer_saying(&path, "说明.md", PAGE)]);
    let mut folio = HeadlessFolio::with_tabs(tag, vec![tab], 0);
    let surface = seat_of(TabId(1), seat);
    folio.draw();
    assert!(
        folio.runtime().preview_shows_live_markdown(surface),
        "the fixture's own precondition: a page that can be typed into"
    );
    (folio, surface)
}

/// The byte of [`PAGE`] where `text` begins.
fn byte_of(text: &str) -> usize {
    PAGE.find(text)
        .unwrap_or_else(|| panic!("`{text}` is on the page"))
}

/// A point on the glass over the drawn paragraph whose text holds `text`, one pixel inside its
/// left edge and halfway down its first line.
fn point_on(folio: &HeadlessFolio, text: &str) -> PhysicalPosition<f64> {
    let paragraph = folio
        .window
        .renderer
        .preview_bodies
        .iter()
        .flat_map(|body| body.paragraphs.iter())
        .find(|paragraph| {
            paragraph
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
                .contains(text)
        })
        .unwrap_or_else(|| panic!("a drawn paragraph holds `{text}`"));
    PhysicalPosition::new(
        f64::from(paragraph.rect[0]) + 1.0,
        f64::from(paragraph.rect[1]) + f64::from(paragraph.line_height_px) / 2.0,
    )
}

/// RED — **entering a page is one door and it does every one of its writes** (T5 ①): the caret
/// moves to the byte the gesture named, the page's piece selection lets go, the page draws the
/// caret's block as the file's own bytes, and the keyboard arrives.
///
/// MUTATIONS (each in `Runtime::seat_preview_caret`): drop `pane.select(None)` — the pieces stay
/// selected under the caret; drop `pane.enter_page()` — the block keeps its rendered face and the
/// bold word's marks are not drawn; drop `self.preview_edit_focus = Some(surface)` — the keyboard
/// stays where it was.
#[test]
fn entering_a_page_moves_the_caret_drops_the_pieces_draws_its_block_and_takes_the_keyboard() {
    let (mut folio, surface) = drawn_page("headless-enter");
    let first = point_on(&folio, "first paragraph");
    let mut runtime = folio.runtime();
    let place = runtime
        .preview_place_at(surface, first)
        .expect("a piece stands under the pointer");
    runtime
        .preview_pane_mut(surface)
        .select(Some(Selection::collapsed(place, Grain::Character)));
    assert!(runtime.preview_edit_focus.is_none());

    let offset = byte_of("粗体");
    runtime
        .spend_preview_press(
            surface,
            Spend::Caret {
                offset,
                grain: Grain::Character,
                extend: false,
                head: None,
            },
        )
        .expect("the release spends the press");

    let pane = runtime.preview_pane(surface).expect("the pane");
    assert_eq!(
        pane.caret().range(),
        offset..offset,
        "the caret is at the byte"
    );
    assert!(pane.md_select().is_none(), "the piece selection let go");
    assert!(pane.md_caret(), "the caret stands on the page");
    assert_eq!(
        runtime.preview_edit_focus,
        Some(surface),
        "the keyboard arrived"
    );
    folio.draw();
    let drawn: String = folio
        .window
        .renderer
        .preview_bodies
        .iter()
        .flat_map(|body| body.paragraphs.iter())
        .flat_map(|paragraph| paragraph.runs.iter())
        .map(|run| run.text.as_str())
        .collect();
    assert!(
        drawn.contains("**粗体**"),
        "the caret's block is drawn as the file's own bytes:\n{drawn}"
    );
}

/// RED — **the caret answers before the piece selection, in both readers of "what is selected"**
/// (T5 ④, research §10 Q3): with a caret selection and a piece selection standing on one page,
/// the clipboard takes the caret's range of the file's own bytes and the highlight is the
/// caret's.
///
/// MUTATION: ask `pane.md_select()` before `self.preview_live_caret(surface)` in
/// `Runtime::preview_selected_text` — the piece's words are copied; or ask the provenance before
/// the caret in `Runtime::preview_caret_selection_places` — no caret places are answered.
#[test]
fn the_caret_answers_before_the_piece_selection_in_the_clipboard_and_the_highlight() {
    let (mut folio, surface) = drawn_page("headless-caret-first");
    let first = point_on(&folio, "first paragraph");
    let mut runtime = folio.runtime();
    let start = byte_of("**粗体**");
    let end = start + "**粗体**".len();
    runtime
        .spend_preview_press(
            surface,
            Spend::Caret {
                offset: start,
                grain: Grain::Character,
                extend: false,
                head: Some(end),
            },
        )
        .expect("a caret selection over the bold word");
    let place = runtime
        .preview_place_at(surface, first)
        .expect("a piece stands under the pointer");
    let mut pieces = Selection::collapsed(place, Grain::Word);
    pieces.head = place;
    runtime.preview_pane_mut(surface).select(Some(pieces));
    assert!(runtime.preview_pane(surface).unwrap().md_select().is_some());

    assert_eq!(
        runtime.preview_selected_text(surface).as_deref(),
        Some("**粗体**"),
        "the clipboard takes the caret's bytes, marks and all"
    );
    let (from, to) = runtime
        .preview_caret_selection_places(surface)
        .expect("the highlight is the caret's");
    assert_ne!(from, to, "and it spans the caret's range");
}

/// The caret put in the page at the byte where `text` begins, as a click there leaves it.
fn enter_at(folio: &mut HeadlessFolio, surface: PreviewSurface, text: &str) -> usize {
    let offset = byte_of(text);
    folio
        .runtime()
        .spend_preview_press(
            surface,
            Spend::Caret {
                offset,
                grain: Grain::Character,
                extend: false,
                head: None,
            },
        )
        .expect("the caret enters the page");
    folio.draw();
    assert!(
        folio.runtime().preview_pane(surface).unwrap().md_caret(),
        "the fixture's own precondition: the caret stands on the page"
    );
    offset
}

/// Whether the caret stands on the page, and whether the keyboard is with it.
fn on_the_page(folio: &mut HeadlessFolio, surface: PreviewSurface) -> (bool, bool) {
    let runtime = folio.runtime();
    (
        runtime.preview_pane(surface).unwrap().md_caret(),
        runtime.preview_edit_focus == Some(surface),
    )
}

/// RED — **leaving is one door, and `Esc`, a press on the page's empty ground and a press
/// somewhere else all reach it; leaving keeps the caret** (T5 ③ as the owner reversed it,
/// 2026-09-10; the ground's gesture spent at the release, 2026-09-21).
///
/// A press on a link is not empty ground: it names letters of the file, so it records nothing
/// and the page it stands in stays as it is. Handing the surface another file takes the page's
/// view with the document.
///
/// MUTATIONS: drop the `leave_preview_page` call from the blur in `Runtime::chrome_mouse_input`
/// — a press in the shell leaves the block as source; key the empty-ground arm of
/// `Runtime::press_preview_text` on `placing` instead of `caret_at` — a press on a link records
/// the ground; set the caret in the view's `leave_page` — the caret moves on the way out.
#[test]
fn leaving_is_one_door_that_escape_the_ground_and_a_blur_reach_and_it_keeps_the_caret() {
    let (mut folio, surface) = drawn_page("headless-leave");

    // `Esc`.
    let offset = enter_at(&mut folio, surface, "粗体");
    let escape = key_down(
        winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape),
        winit::keyboard::KeyCode::Escape,
        None,
    );
    assert!(
        folio
            .runtime()
            .preview_key(&escape)
            .expect("Esc is answered")
    );
    assert_eq!(
        on_the_page(&mut folio, surface),
        (false, false),
        "Esc leaves"
    );
    assert_eq!(
        folio
            .runtime()
            .preview_pane(surface)
            .unwrap()
            .caret()
            .range(),
        offset..offset,
        "and leaving keeps the caret"
    );

    // The page's empty ground — a page of rules has no letters to stand on — spent at the
    // release.
    let path = host_spelling(r"C:\w\规则.md");
    let (tab, seat) = tab_with_a_preview(1, vec![buffer_saying(&path, "规则.md", "---\n\n***\n")]);
    let mut rules = HeadlessFolio::with_tabs("headless-leave-ground", vec![tab], 0);
    let ruled = seat_of(TabId(1), seat);
    rules.draw();
    rules
        .runtime()
        .spend_preview_press(
            ruled,
            Spend::Caret {
                offset: 0,
                grain: Grain::Character,
                extend: false,
                head: None,
            },
        )
        .expect("the caret enters the page");
    rules.draw();
    let clip = rules.window.renderer.preview_bodies[0].clip;
    let ground = PhysicalPosition::new(
        f64::from(clip[0] + clip[2]) / 2.0,
        f64::from(clip[1] + clip[3]) / 2.0,
    );
    assert_eq!(on_the_page(&mut rules, ruled), (true, true));
    rules.press(ground);
    assert!(
        on_the_page(&mut rules, ruled).0,
        "the press changes nothing yet"
    );
    rules.release(ground);
    assert_eq!(
        on_the_page(&mut rules, ruled),
        (false, false),
        "the ground leaves at the release"
    );

    // A press on a link is the link's: no record, and the page stays.
    enter_at(&mut folio, surface, "粗体");
    let line = point_on(&folio, "链接");
    let link = (0..600)
        .map(|step| PhysicalPosition::new(line.x + f64::from(step), line.y))
        .find(|at| folio.runtime().preview_link_at(*at).is_some())
        .expect("the link is drawn on its paragraph's first line");
    folio.press(link);
    let pressed = folio
        .runtime()
        .held_preview_text_drag()
        .map(|drag| drag.pressed);
    assert_eq!(
        pressed,
        Some(None),
        "a press on a link records neither a byte nor the ground"
    );
    folio
        .runtime()
        .cancel_own_capture()
        .expect("the gesture is let go");
    assert!(on_the_page(&mut folio, surface).0, "and the page stays");

    // A press somewhere else: the shell beside the page.
    folio.press(PhysicalPosition::new(20.0, 200.0));
    assert!(
        !on_the_page(&mut folio, surface).0,
        "losing the keyboard to a press elsewhere leaves"
    );

    // Handing the surface another file takes the view with the document.
    enter_at(&mut folio, surface, "粗体");
    folio.runtime().leave_preview_buffer_in(0, surface);
    assert!(
        !folio
            .runtime()
            .preview_pane(surface)
            .is_some_and(|pane| pane.md_caret()),
        "the next document opens as a page to read"
    );
}

/// RED — **a press on a rendered page changes no block's face; the release does** (owner's report
/// and ruling 2026-09-21, as its closure review narrowed it).
///
/// The press records what it would do and the release spends it. The one press answered where
/// it stands is the one that changes nothing: inside the seat the page is already drawing from.
///
/// MUTATION: drop `in_the_seat &&` from the press's own spend in `Runtime::press_preview_text`
/// — a press in another paragraph moves the caret, and that block's face, while the button is
/// down (the 0.4.3 timing).
#[test]
fn a_press_on_a_rendered_page_changes_no_blocks_face_until_the_release() {
    let (mut folio, surface) = drawn_page("headless-press-face");
    let standing = enter_at(&mut folio, surface, "粗体");
    let first = point_on(&folio, "first paragraph");
    folio.press(first);
    assert_eq!(
        folio
            .runtime()
            .preview_pane(surface)
            .unwrap()
            .caret()
            .range(),
        standing..standing,
        "a press outside the seat leaves the caret, and so every block's face, as it was"
    );
    let named = folio
        .runtime()
        .held_preview_text_drag()
        .and_then(|drag| drag.pressed);
    assert!(
        matches!(named, Some(crate::preview_press::Pressed::Byte { .. })),
        "it records the byte it named: {named:?}"
    );
    folio.release(first);
    let caret = folio.runtime().preview_pane(surface).unwrap().caret();
    assert!(
        caret.range().start < byte_of("第二段"),
        "the release spends the record: the caret is in the first paragraph ({caret:?})"
    );

    // Inside the seat the page is drawing from, the press is answered where it stands.
    let seat = enter_at(&mut folio, surface, "bold here");
    let inside = point_on(&folio, "bold here");
    folio.press(inside);
    assert_ne!(
        folio
            .runtime()
            .preview_pane(surface)
            .unwrap()
            .caret()
            .range(),
        seat..seat,
        "a press inside the seat moves the caret at once — nothing changes face"
    );
    folio.release(inside);
}

/// RED — **a repeated press keeps its grain until the release, and a page still buying its file
/// keeps the caret it was asked for** — both carried on the press's record.
///
/// MUTATIONS: drop `self.widen_preview_caret(surface, grain)` from `Runtime::spend_preview_press`
/// — a double click takes a character; drop `want_caret_at(Some(offset))` — a press on a page
/// whose file is still being read is lost; drop the owed caret's read in
/// `Runtime::settle_preview_caret` — nothing spends it when the body lands.
#[test]
fn a_repeated_press_keeps_its_grain_and_a_page_still_reading_keeps_its_caret_owed() {
    let (mut folio, surface) = drawn_page("headless-grain");
    let at = point_on(&folio, "first paragraph");
    folio.press(at);
    folio.release(at);
    folio.press(at);
    folio.release(at);
    let range = folio
        .runtime()
        .preview_pane(surface)
        .unwrap()
        .caret()
        .range();
    let word = crate::preview_select::word_start(PAGE, range.start)
        ..crate::preview_select::word_end(PAGE, range.start);
    assert!(
        !range.is_empty(),
        "a double click takes more than a character"
    );
    assert_eq!(range, word, "it takes the word under the press");

    // A page whose head alone has been read.
    let path = host_spelling(r"C:\w\长文.md");
    let mut buffer = crate::preview::PreviewBuffer::new(
        crate::preview::PreviewSource::file(path),
        "长文.md".to_owned(),
    );
    buffer.accept(crate::preview::HeadOutcome::Read {
        text: PAGE[..byte_of("第二段")].to_owned(),
        truncated: true,
        mtime: None,
        bytes: None,
        content_says_text: true,
        encoding: crate::preview::HeadEncoding::Utf8,
        lossy: false,
    });
    let (tab, seat) = tab_with_a_preview(1, vec![buffer]);
    let mut folio = HeadlessFolio::with_tabs("headless-owed", vec![tab], 0);
    let surface = seat_of(TabId(1), seat);
    folio.draw();
    let wanted = byte_of("first");
    folio
        .runtime()
        .spend_preview_press(
            surface,
            Spend::Caret {
                offset: wanted,
                grain: Grain::Character,
                extend: false,
                head: None,
            },
        )
        .expect("the release spends the press");
    assert!(
        !folio.runtime().preview_pane(surface).unwrap().md_caret(),
        "no caret on a page that cannot be typed into yet"
    );
    folio
        .runtime()
        .preview_buffer_on_mut(surface)
        .expect("the page's buffer")
        .accept(crate::preview::HeadOutcome::Read {
            text: PAGE.to_owned(),
            truncated: false,
            mtime: None,
            bytes: Some(std::sync::Arc::from(PAGE.as_bytes())),
            content_says_text: true,
            encoding: crate::preview::HeadEncoding::Utf8,
            lossy: false,
        });
    assert!(
        folio.runtime().settle_preview_caret(surface),
        "the body landed"
    );
    let runtime = folio.runtime();
    let pane = runtime.preview_pane(surface).unwrap();
    assert!(pane.md_caret(), "the owed caret is spent");
    assert_eq!(
        pane.caret().range(),
        wanted..wanted,
        "where it was asked for"
    );
}

/// The source span the page was last drawn with, as its document's key names it.
fn drawn_span(folio: &mut HeadlessFolio, surface: PreviewSurface) -> Option<String> {
    folio
        .runtime()
        .preview_pane(surface)
        .and_then(|pane| pane.doc_key())
        .and_then(|key| key.source.as_ref())
        .map(|span| format!("{span:?}"))
}

/// RED (owner's ruling 2026-09-23, B) — **the window draws the span one function computes, holds
/// it while a gesture is in flight, and lets it go when the window loses focus.**
///
/// A drag drawn across the page from the paragraph above the caret's block to the one below
/// selects pieces in three blocks, and the page keeps the face it had when the hand went down; a
/// blur ends the gesture — a lost release must not hold the span for ever.
///
/// MUTATIONS: read the fresh span in `Runtime::rebuild_preview_document` without
/// `preview_press::held_span` — the page changes shape mid-drag; drop
/// `self.end_capture(Ending::Cancel)` from the blur's door, `Runtime::window_blurred`
/// (T-POINTER-CAPTURE cut 5) — the gesture outlives the window's focus and the span with it.
#[test]
fn the_window_holds_the_span_a_gesture_starts_on_and_frees_it_on_a_blur() {
    let (mut folio, surface) = drawn_page("headless-span");
    enter_at(&mut folio, surface, "bold here");
    let held = drawn_span(&mut folio, surface);
    assert!(held.is_some(), "the caret's block is drawn as source");
    let first = point_on(&folio, "first paragraph");
    let third = point_on(&folio, "第三段");
    folio.press(first);
    folio.move_to(PhysicalPosition::new(first.x + 40.0, first.y));
    folio.move_to(third);
    folio.draw();
    assert!(
        folio.runtime().held_preview_text_drag().is_some(),
        "the gesture is in flight"
    );
    assert_eq!(
        drawn_span(&mut folio, surface),
        held,
        "the page keeps the span its gesture began on while the hand is down"
    );

    let mut application = crate::headless::HeadlessApplication::of(folio);
    application.window_event(winit::event::WindowEvent::Focused(false));
    assert!(
        application.runtime().held_preview_text_drag().is_none(),
        "a blur ends the gesture"
    );
}

/// The caret's page typed into: `text` typed at the caret, as the keyboard hands it.
fn type_on_the_page(folio: &mut HeadlessFolio, text: &str) {
    let key = key_down(
        winit::keyboard::Key::Character(winit::keyboard::SmolStr::new(text)),
        winit::keyboard::KeyCode::KeyX,
        Some(text),
    );
    assert!(
        folio
            .runtime()
            .preview_key(&key)
            .expect("the key is answered"),
        "the page takes the key"
    );
}

/// Every run of text the glass was last handed for the page, joined.
fn drawn_text(folio: &HeadlessFolio) -> String {
    folio
        .window
        .renderer
        .preview_bodies
        .iter()
        .flat_map(|body| body.paragraphs.iter())
        .flat_map(|paragraph| paragraph.runs.iter())
        .map(|run| run.text.as_str())
        .collect()
}

/// The page's laid-out wrap, by its address: a re-flow lays the page out into a new one.
fn laid_out(folio: &mut HeadlessFolio, surface: PreviewSurface) -> usize {
    match folio.runtime().preview_pane(surface).map(|pane| pane.doc()) {
        Some(PreviewDocument::Markdown { wrap, .. }) => std::sync::Arc::as_ptr(wrap) as usize,
        _ => panic!("the pane holds a laid-out page"),
    }
}

/// RED — **the parse of an edit draws the caret's block as the file's own bytes and writes that
/// block into the document's key, and the frame after it lays nothing out again** (§7.1.3q): the
/// key the parse is asked under carries the span read off the page *before* the edit, and the
/// parse — the only one that indexes the new bytes — computes the span itself, draws it and keys
/// it, so the frame that follows, asked nothing new, re-flows nothing.
///
/// Driven as one parse — the buffer's bytes moved, then `Runtime::rebuild_preview_document` once,
/// then the body's repaint and a frame (the page's wrap the same allocation, its block
/// measurements neither hit nor missed again) — because a typed key reveals its caret, and the
/// reveal's own passes would put a missing span back before anything could look (one wasted
/// re-flow per keystroke, which is the defect).
///
/// MUTATIONS (each in `Runtime::rebuild_preview_document`): write `key_parsed_source(None)` — the
/// key says no span where the document drew one; hand `markdown_source_blocks` `None` for the
/// fresh span — the page draws the block rendered, its bold marks hidden.
#[test]
fn an_edit_draws_and_keys_its_block_as_source_in_its_one_draw() {
    let (mut folio, surface) = drawn_page("headless-edit-span");
    let caret = enter_at(&mut folio, surface, "bold here");
    {
        let mut runtime = folio.runtime();
        let index = runtime.preview_tab_index(surface);
        let source = runtime
            .preview_pane(surface)
            .and_then(|pane| pane.source().cloned())
            .expect("the pane shows a file");
        assert!(
            runtime.window.tabs[index]
                .preview_pool
                .get_mut(&source)
                .expect("the page's buffer")
                .edit_content(|content| {
                    content.insert(caret, 'x');
                    true
                }),
            "the bytes move under the caret"
        );
        let scale = runtime.window.renderer.scale_factor() as f32;
        let body = runtime
            .preview_surface_body_rect(surface, scale)
            .expect("the page has a body");
        runtime.rebuild_preview_document(surface, body, scale);
    }
    assert!(
        drawn_span(&mut folio, surface).is_some(),
        "the edit's parse writes the span it drew into the document's key"
    );
    let parsed = laid_out(&mut folio, surface);
    let measured = (
        folio.window.markdown_intrinsics.hits,
        folio.window.markdown_intrinsics.misses,
    );
    folio
        .runtime()
        .repaint_preview()
        .expect("the page asks for its frame");
    folio.draw();
    assert_eq!(
        laid_out(&mut folio, surface),
        parsed,
        "the frame after the parse re-flows nothing it laid out"
    );
    assert_eq!(
        (
            folio.window.markdown_intrinsics.hits,
            folio.window.markdown_intrinsics.misses,
        ),
        measured,
        "and measures no block again"
    );
    let drawn = drawn_text(&folio);
    assert!(
        drawn.contains("**粗体** xbold"),
        "the page shows the caret's block as the file's own bytes:\n{drawn}"
    );
}

/// RED — **a save the storage lane refuses is said on the pane, and the page stays dirty**,
/// driven on a headless window, whose storage lane no worker serves (S-6a, C2): `Ctrl+S`'s verb
/// hands the edited page to the lane, the lane refuses it at the door, and the pane says why.
///
/// MUTATION: in `Runtime::save_preview_on`, answer a refused request as sent — the pane says
/// nothing and the page looks saved-on-its-way.
#[test]
fn a_save_the_lane_refuses_is_said_on_the_pane_and_the_page_stays_dirty() {
    let (mut folio, surface) = drawn_page("headless-save-refused");
    enter_at(&mut folio, surface, "bold here");
    type_on_the_page(&mut folio, "x");
    let mut runtime = folio.runtime();
    runtime.save_preview_on(surface).expect("the verb runs");
    let said = runtime
        .preview_pane(surface)
        .and_then(|pane| pane.notice())
        .map(|(sentence, _)| sentence.clone());
    let not_started = crate::i18n::Text::PreviewSaveNotStarted.text().to_owned();
    assert_eq!(
        said,
        Some(crate::i18n::not_saved(&not_started)),
        "the pane says the save was not started, and why"
    );
    let index = runtime.preview_tab_index(surface);
    let source = runtime
        .preview_pane(surface)
        .and_then(|pane| pane.source().cloned())
        .expect("the pane shows a file");
    assert!(
        runtime.window.tabs[index]
            .preview_pool
            .get(&source)
            .is_some_and(preview::PreviewBuffer::dirty),
        "the edit is still the page's, unsaved"
    );
    assert_eq!(
        runtime.app.preview_saves.out(),
        0,
        "nothing is owed an answer"
    );
}

/// The drawn paragraph whose text holds `text`, as the glass was last handed it.
fn drawn_rect(folio: &HeadlessFolio, text: &str) -> [f32; 4] {
    folio
        .window
        .renderer
        .preview_bodies
        .iter()
        .flat_map(|body| body.paragraphs.iter())
        .find(|paragraph| {
            paragraph
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
                .contains(text)
        })
        .map(|paragraph| paragraph.rect)
        .unwrap_or_else(|| panic!("a drawn paragraph holds `{text}`"))
}

/// The point on the first line of the link the page draws.
fn on_the_link(folio: &mut HeadlessFolio) -> PhysicalPosition<f64> {
    let line = point_on(folio, "链接");
    (0..600)
        .map(|step| PhysicalPosition::new(line.x + f64::from(step), line.y))
        .find(|at| folio.runtime().preview_link_at(*at).is_some())
        .expect("the link is drawn on its paragraph's first line")
}

/// RED (preview report 2026-09-23, A) — **the source block's band is chosen by one function and
/// drawn from it in both faces**: a drag begun in a rendered paragraph and carried into the block
/// the caret is drawing as source bands that block from the bytes the hand reached, and a drag
/// begun on a link — which spends no press — still records the byte it reached.
///
/// MUTATIONS: put `.bands(&caret.selection)` back in `Runtime::build_preview_body_in` — the
/// prose source block is drawn with no band under the hand; gate the `drag.reached` write on
/// `spends` in `Runtime::drag_preview_text` — a drag begun on a link reaches nothing; drop
/// `if reach_moved { self.repaint_preview()?; }` from it — a hand moving inside the source block
/// asks for no frame.
#[test]
fn the_source_blocks_band_is_drawn_from_the_drag_that_reaches_into_it() {
    let (mut folio, surface) = drawn_page("headless-band");
    enter_at(&mut folio, surface, "bold here");
    let block = drawn_rect(&folio, "bold here");
    let first = point_on(&folio, "first paragraph");
    let into = PhysicalPosition::new(
        f64::from(block[0]) + 40.0,
        f64::from(block[1]) + f64::from(block[3] - block[1]) / 2.0,
    );
    folio.press(first);
    folio.move_to(PhysicalPosition::new(first.x + 30.0, first.y));
    folio.move_to(into);
    folio.draw();
    let selection = bt_render::chrome_palette().preview_selection;
    let banded = folio
        .window
        .renderer
        .preview_bodies
        .iter()
        .flat_map(|body| body.quads.iter())
        .filter(|quad| quad.color == selection)
        .any(|quad| quad.rect[1] >= block[1] - 1.0 && quad.rect[1] < block[3]);
    assert!(
        banded,
        "the drag reaching into the source block bands it from the bytes it reached"
    );
    // A hand moving only inside the source block moves nothing on the rendered page, and still
    // asks the window for a frame: the band it draws is the bytes it reached.
    folio.ledger.take();
    folio.move_to(PhysicalPosition::new(into.x + 24.0, into.y));
    assert!(
        folio
            .ledger
            .take()
            .contains(&crate::headless::WindowEffect::Redraw),
        "a reach that moved inside the source block asks for a frame"
    );
    folio
        .runtime()
        .cancel_own_capture()
        .expect("the gesture is let go");

    // A drag begun on a link spends no press, and still records where it reached.
    let link = on_the_link(&mut folio);
    folio.press(link);
    folio.move_to(PhysicalPosition::new(link.x + 30.0, link.y));
    folio.move_to(into);
    let reached = folio
        .runtime()
        .held_preview_text_drag()
        .and_then(|drag| drag.reached);
    assert!(
        reached.is_some_and(|offset| offset >= byte_of("第二段") && offset < byte_of("第三段")),
        "the drag remembers the byte of the source block it reached: {reached:?}"
    );
    folio
        .runtime()
        .cancel_own_capture()
        .expect("the gesture is let go");
}

/// The first point of the window, walked every four pixels, where `hit` answers.
fn first_point(
    folio: &mut HeadlessFolio,
    mut hit: impl FnMut(
        &mut crate::Runtime<'_, crate::headless::Headless>,
        PhysicalPosition<f64>,
    ) -> bool,
) -> Option<PhysicalPosition<f64>> {
    let mut runtime = folio.runtime();
    (0..300)
        .flat_map(|row| (0..200).map(move |column| (column, row)))
        .map(|(column, row)| PhysicalPosition::new(f64::from(column) * 6.0, f64::from(row) * 2.7))
        .find(|at| hit(&mut runtime, *at))
}

/// RED — **the button on a page this window cannot show runs one verb, in a pane and in a
/// window torn off it**: it hands the surface's own file to the system's handler, whichever lane
/// — the buffer's or the picture's — the surface came down (owner's ruling 2026-09-12).
///
/// MUTATIONS: answer the pane's `ChromeTarget::PreviewOpenButton` in `Runtime::chrome_mouse_input`
/// with nothing — the pane's button opens nothing; drop the picture's lane from
/// `Runtime::preview_file_on` — a refused picture's button names no file.
#[test]
fn the_button_on_a_page_this_window_cannot_show_hands_its_file_to_the_system() {
    let path = host_spelling(r"C:\w\数据.bin");
    let mut buffer = crate::preview::PreviewBuffer::new(
        crate::preview::PreviewSource::file(path.clone()),
        "数据.bin".to_owned(),
    );
    buffer.accept(crate::preview::HeadOutcome::Read {
        text: "\u{0}\u{1}二进制".to_owned(),
        truncated: false,
        mtime: None,
        bytes: Some(std::sync::Arc::from("\u{0}\u{1}二进制".as_bytes())),
        content_says_text: false,
        encoding: crate::preview::HeadEncoding::Utf8,
        lossy: false,
    });
    let (tab, seat) = tab_with_a_preview(1, vec![buffer]);
    let mut folio = HeadlessFolio::with_tabs("headless-open-button", vec![tab], 0);
    folio.draw();
    let button = first_point(&mut folio, |runtime, at| {
        matches!(
            runtime.pointer_target_at(at),
            Some(crate::PointerTarget::Chrome(
                crate::seats::ChromeTarget::PreviewOpenButton(_)
            ))
        )
    })
    .expect("the unknown page's card offers its button");
    folio.ledger.take();
    folio.press(button);
    folio.release(button);
    let opened: Vec<_> = folio
        .ledger
        .take()
        .into_iter()
        .filter_map(|effect| match effect {
            crate::headless::WindowEffect::HandedOff(handoff) => Some(handoff),
            _ => None,
        })
        .collect();
    assert_eq!(
        opened,
        [bt_platform::Handoff::Open(std::path::PathBuf::from(&path))],
        "the pane's button hands its file to the system"
    );

    // The same page torn off into a window of its own: its card's button is the same verb.
    folio
        .runtime()
        .pop_out_preview(seat)
        .expect("the page is torn off");
    folio.draw();
    let button = first_point(&mut folio, |runtime, at| {
        matches!(
            runtime.pointer_target_at(at),
            Some(crate::PointerTarget::Float(
                _,
                crate::float::FloatPart::CardButton
            ))
        )
    })
    .expect("the torn-off window's card offers its button");
    folio.ledger.take();
    folio.press(button);
    folio.release(button);
    assert!(
        folio.ledger.take().into_iter().any(|effect| effect
            == crate::headless::WindowEffect::HandedOff(bt_platform::Handoff::Open(
                std::path::PathBuf::from(&path)
            ))),
        "the window's button hands the same file to the system"
    );

    // A picture this window refuses comes down the picture lane, and has no buffer at all.
    let picture = host_spelling(r"C:\w\巨图.png");
    let (tab, seat) = crate::test_support::tab_with_a_picture(1, &picture);
    let mut folio = HeadlessFolio::with_tabs("headless-open-picture", vec![tab], 0);
    folio
        .runtime()
        .open_preview_externally_on(seat_of(TabId(1), seat))
        .expect("the verb is answered");
    assert!(
        folio.ledger.take().into_iter().any(|effect| effect
            == crate::headless::WindowEffect::HandedOff(bt_platform::Handoff::Open(
                std::path::PathBuf::from(&picture)
            ))),
        "a picture's card names the picture's file"
    );
}

/// How many places of the product write `text`, read as code with its literals kept.
fn product_sites(text: &str) -> usize {
    crate::test_support::found(
        bt_source::Needle::new(bt_source::Pattern::text(text)),
        bt_source::View::CodeKeepingLiterals,
    )
    .in_the_product(crate::test_support::source())
    .len()
}

/// GUARD (kept, T-HEADLESS-RUNTIME-SEAM round 2) — **the page has one entrance, one exit and one
/// place that owes a caret, and both readers of the source span hand it the rendered selection.**
/// The behaviour of each is driven above (entering, leaving, the owed caret, the held span);
/// what no driven test can say is that there is no *second* place doing it — a count of call
/// sites is a fact about the source, so these counts, and only these, are read from it:
///
/// * `pane.enter_page()` — one entrance (T5 ①);
/// * `md_caret = false` — one exit; the file swap takes the whole view with the document;
/// * `want_caret_at(Some(` — one place owes the caret;
/// * `pane.md_select()` handed to `preview_live::selection_span` by the fresh parse
///   (`rebuild_preview_document`) and by the standing span (`standing_source_span`) — the two
///   producers of the span read the same selection. Whether the fresh parse's span is drawn and
///   keyed is not read here: an edit's one draw drives it
///   (`an_edit_draws_and_keys_its_block_as_source_in_its_one_draw`).
#[test]
fn the_page_has_one_entrance_one_exit_and_one_owed_caret() {
    for (text, why) in [
        (
            "pane.enter_page()",
            "one entrance, a second is a ruling and not an edit",
        ),
        (
            "md_caret = false",
            "one exit; the file swap takes the whole view with the document",
        ),
        ("want_caret_at(Some(", "one place owes the caret"),
    ] {
        assert_eq!(product_sites(text), 1, "{text}: {why}");
    }
    let fresh = crate::test_support::method_body("Runtime", "rebuild_preview_document");
    let standing = crate::test_support::method_body("Runtime", "standing_source_span");
    for (name, body) in [
        ("rebuild_preview_document", fresh),
        ("standing_source_span", standing),
    ] {
        assert!(
            body.contains("preview_live::selection_span(") && body.contains("md_select()"),
            "{name} hands the span's producer the page's rendered selection"
        );
    }
}

// ── the peek completions, driven (T-PEEK-COMPLETIONS-RUNTIME-PINS) ─────────

/// **A window of one shell beside two empty preview panes, its computation lane the suite's
/// scripted executor under the test's eye**: the window, the lane, the lane's gate, the shell's
/// seat and the two panes. A test puts its picture and its page on the panes once the lane is in
/// place, so every read the window asks goes down the lane the test watches.
fn two_panes_on_a_watched_lane(
    tag: &str,
) -> (
    HeadlessFolio,
    crate::computation_lane::fixtures::Watched,
    Arc<crate::lane::Gate>,
    bt_layout::SeatId,
    [PreviewSurface; 2],
) {
    two_panes_on_a_watched_lane_beside(tag, Vec::new())
}

/// [`two_panes_on_a_watched_lane`], with `others` after it in the strip; the panes' tab is in
/// front.
fn two_panes_on_a_watched_lane_beside(
    tag: &str,
    others: Vec<crate::TabState>,
) -> (
    HeadlessFolio,
    crate::computation_lane::fixtures::Watched,
    Arc<crate::lane::Gate>,
    bt_layout::SeatId,
    [PreviewSurface; 2],
) {
    let mut seats = crate::seats::Seats::lone_terminal();
    let shell = seats.identity();
    // The first pane pinned, so the second is a pane of its own rather than the first reused.
    let pinned = seats
        .add_preview(&crate::test_support::cross_metrics())
        .expect("the first preview seat lands");
    assert!(seats.toggle_preview_lock(pinned));
    let second = seats
        .add_preview(&crate::test_support::cross_metrics())
        .expect("a pinned pane is not a reuse target");
    let panes = [pinned, second];
    let (layout, overflow) = crate::test_support::cross_solve(&seats);
    let tab = crate::assemble_tab_state(
        TabId(1),
        std::collections::BTreeMap::from([(shell, crate::test_support::leaf_saying("SHELL"))]),
        std::collections::BTreeMap::new(),
        preview::PreviewPool::default(),
        crate::PreviewPanes::default(),
        std::collections::BTreeMap::new(),
        shell,
        crate::TabSeed::default(),
        seats,
        layout,
        overflow,
    );
    let mut tabs = vec![tab];
    tabs.extend(others);
    let mut folio = HeadlessFolio::with_tabs(tag, tabs, 0);
    let gate = Arc::new(crate::lane::Gate::default());
    let lane = crate::computation_lane::fixtures::watched(crate::lane::Executor::scripted(
        Arc::clone(&gate),
    ));
    folio.app.math_worker = Arc::clone(&lane.lane);
    (
        folio,
        lane,
        gate,
        shell,
        panes.map(|seat| seat_of(TabId(1), seat)),
    )
}

/// `surface` shows the picture in `file`.
fn a_picture_on(folio: &mut HeadlessFolio, surface: PreviewSurface, file: &Path) {
    folio.window.tabs[0]
        .preview_panes
        .entry(surface)
        .show_picture(file.to_path_buf());
}

/// `surface` shows a Markdown page beside `file` whose one picture is `file`.
fn a_page_on(folio: &mut HeadlessFolio, surface: PreviewSurface, file: &Path) {
    let name = file
        .file_name()
        .expect("the picture has a name")
        .to_string_lossy();
    let page = file.with_extension("md");
    let held = folio.window.tabs[0]
        .preview_pool
        .insert(crate::test_support::buffer_saying(
            &page.display().to_string(),
            "说明 page.md",
            &format!("# 截图 shots\n\n![截图 shot](<{name}>)\n"),
        ));
    folio.window.tabs[0]
        .preview_panes
        .entry(surface)
        .show_source(held);
}

/// The read the window awaits for `file`, if one is out.
fn read_awaited(folio: &HeadlessFolio, file: &Path) -> Option<ComputationId> {
    match folio
        .window
        .peek_cache
        .peek(&normalized_local_image_path_key(file))
    {
        Some(PeekCacheEntry::Pending { asked }) => Some(*asked),
        _ => None,
    }
}

/// **The window takes these terminals** as a `MathReady` turn hands them to it: sorted
/// ([`crate::sort_math_terminals`]) and applied in one batch.
fn the_window_takes(folio: &mut HeadlessFolio, ended: Vec<crate::computation_lane::Ended>) {
    let mut batch = crate::sort_math_terminals(ended).done;
    folio
        .runtime()
        .apply_math_results(&mut batch, false)
        .expect("the window takes its answers");
}

/// **What the window holds and draws about one file**, the facts a set-aside answer must leave as
/// they are.
#[derive(Debug, PartialEq)]
struct HeldAbout {
    /// The cache's entry: the identity a read is out under, or what was filed.
    entry: String,
    /// The sentence the pane shows instead of the picture.
    refusal: Option<String>,
    /// The decode's own size the pane was told.
    native: Option<(u32, u32)>,
    /// The facts filed for a recording.
    facts: Option<preview::VideoFacts>,
    /// The pages' picture generation, and the exact-size passes owed.
    generation: u64,
    owed: Vec<String>,
    /// The pictures the last walk handed the glass.
    images: usize,
}

impl HeldAbout {
    fn of(folio: &HeadlessFolio, file: &Path, pane: PreviewSurface) -> Self {
        let key = normalized_local_image_path_key(file);
        let picture = folio.window.tabs[0]
            .preview_panes
            .get(pane)
            .and_then(PreviewPane::image);
        let mut owed: Vec<String> = folio
            .window
            .markdown_pictures
            .owed
            .keys()
            .cloned()
            .collect();
        owed.sort();
        Self {
            entry: match folio.window.peek_cache.peek(&key) {
                Some(PeekCacheEntry::Pending { asked }) => format!("pending {}", asked.number()),
                Some(PeekCacheEntry::Ready { key, .. }) => format!("ready {key}"),
                Some(PeekCacheEntry::Failed(failure)) => format!("failed {failure:?}"),
                None => "nothing".to_owned(),
            },
            refusal: picture
                .and_then(|picture| picture.failure.as_ref())
                .map(|refusal| refusal.notice.clone()),
            native: picture.and_then(|picture| picture.native),
            facts: folio.window.video_facts.get(&key).copied(),
            generation: folio.window.markdown_pictures.generation,
            owed,
            images: folio.window.renderer.preview_images.len(),
        }
    }
}

/// RED (T-PEEK-COMPLETIONS-RUNTIME-PINS; T-PEEK-ANSWER-IDENTITY) — **a set-aside peek answer
/// changes nothing on the window**: a pane (and, for a picture, a Markdown page) shows a file;
/// the window reads it, the file changes under the read, and the walk asks about the bytes there
/// now. The older read's answer — a half-written picture's failure, or a recording's facts from
/// its shorter bytes — comes home before the newer one or after it, through the window's own
/// drain on the real lane, and is set aside: no pane refused, no page re-flowed or sharpened, no
/// `video_facts` filed, and no redraw asked.
///
/// MUTATIONS: drop the early `return` after `file_peek_answer` in
/// `Runtime::complete_peek_image` — the half-written bytes' failure refuses the pane and asks a
/// redraw; write `video_facts` before the gate in `Runtime::complete_peek_video_frame` — the
/// shorter bytes' facts are filed.
#[test]
fn a_set_aside_peek_answer_changes_nothing_on_the_window() {
    let picture = png_bytes(6, 3);
    let kinds = [
        (
            "截图 picture",
            "png",
            picture[..picture.len() / 3].to_vec(),
            picture.clone(),
        ),
        ("录屏 recording", "mp4", vec![0x5a; 64], vec![0x5a; 256]),
    ];
    for (name, extension, older_bytes, newer_bytes) in kinds {
        for older_second in [true, false] {
            let case = format!("{name}, the older answer arriving second: {older_second}");
            let (mut folio, mut lane, _gate, _shell, [pane, page]) =
                two_panes_on_a_watched_lane("peek-set-aside");
            let file = scratch_png_named(name).with_extension(extension);
            let _written = crate::test_support::ScratchFile(file.clone());
            std::fs::write(&file, &older_bytes).expect("a scratch file this test owns");
            a_picture_on(&mut folio, pane, &file);
            if extension == "png" {
                a_page_on(&mut folio, page, &file);
            }
            folio.runtime().refresh_preview_for_layout();
            let older = read_awaited(&folio, &file)
                .unwrap_or_else(|| panic!("{case}: the surfaces read the file"));
            let older_answer = lane.next_ended(
                crate::computation_lane::Share::Decoration,
                crate::computation_lane::fixtures::nobody,
            );
            assert_eq!(older_answer.1, older, "{case}: the older read is answered");

            std::fs::write(&file, &newer_bytes).expect("the writer finishes");
            folio.runtime().forget_the_picture_in(&file);
            folio.runtime().refresh_preview_for_layout();
            let newer = read_awaited(&folio, &file)
                .unwrap_or_else(|| panic!("{case}: the walk asks about the bytes there now"));
            assert_ne!(older, newer, "{case}: a new question, under a new identity");
            let mut newer_answer = Some(lane.next_ended(
                crate::computation_lane::Share::Decoration,
                crate::computation_lane::fixtures::nobody,
            ));
            assert_eq!(
                newer_answer.as_ref().map(|answer| answer.1),
                Some(newer),
                "{case}: the newer read is answered"
            );

            if older_second {
                the_window_takes(&mut folio, newer_answer.take().into_iter().collect());
                assert!(
                    read_awaited(&folio, &file).is_none(),
                    "{case}: the fixture's own precondition: the newer answer is filed"
                );
                folio.draw();
                folio.ledger.take();
            }
            let before = HeldAbout::of(&folio, &file, pane);
            the_window_takes(&mut folio, vec![older_answer]);
            assert_eq!(
                HeldAbout::of(&folio, &file, pane),
                before,
                "{case}: the set-aside answer changes nothing the window holds or draws"
            );
            let effects = folio.ledger.take();
            assert!(
                !effects.iter().any(|effect| matches!(
                    effect,
                    WindowEffect::Redraw | WindowEffect::Presented { .. }
                )),
                "{case}: and asks no redraw: {effects:?}"
            );
            if let Some(answer) = newer_answer.take() {
                the_window_takes(&mut folio, vec![answer]);
                assert!(
                    read_awaited(&folio, &file).is_none(),
                    "{case}: the newer answer, arriving second, is filed"
                );
            }
            if extension == "mp4" {
                assert_eq!(
                    folio
                        .window
                        .video_facts
                        .get(&normalized_local_image_path_key(&file))
                        .and_then(|facts| facts.bytes),
                    Some(newer_bytes.len() as u64),
                    "{case}: the recording's facts are the newer read's"
                );
            }
        }
    }
}

/// RED (T-PEEK-COMPLETIONS-RUNTIME-PINS; T-PEEK-ANSWER-IDENTITY round 2) — **an awaited peek
/// answer that lands after a terminal's own decode of the file still tells every surface waiting
/// on it**: a picture pane, a Markdown page, the hover flyout and the glance card all wait on one
/// file's read while the shell's verification decode of the same file is on the worker. Both come
/// home in one drain, the verification first: it leaves the read's record alone
/// (`remember_decode`), so the read's answer is the awaited one — the pane is told the picture's
/// size, the page's picture generation ticks, the flyout asks for its resample, the card's overlay
/// is rebuilt and asks for its picture, and a redraw is asked. **And both resamples are answered**
/// (T-PEEK-SCALE-ADDRESS-BY-SURFACE): asked while the scale share is busy, so both wait together,
/// neither supersedes the other, and each lands on its own surface — the flyout holds its picture
/// at its own size and the card its own.
///
/// MUTATIONS: drop `remember_decode`'s `Pending` guard — the verification's pixels are filed over
/// the record, the read's answer is set aside, and no surface is told; give both peek surfaces one
/// purpose in `ScaleWorkerRequest::purpose` — the card's request supersedes the flyout's.
#[test]
fn an_awaited_peek_answer_after_a_terminals_decode_tells_every_waiting_surface() {
    let (mut folio, mut lane, gate, shell, [pane, page]) =
        two_panes_on_a_watched_lane("peek-awaited");
    let file = scratch_png_named("等着 awaited");
    let _written = crate::test_support::ScratchFile(file.clone());
    // Large enough that the card's box and the flyout's are two sizes. Over one file, from one
    // tab's focused shell, the two resamples differ only by the surface that asks
    // (`ScaleWorkerRequest::same_target`).
    std::fs::write(&file, png_bytes(600, 300)).expect("a scratch file this test owns");

    // The shell's verification of the printed path is on the worker when the surfaces ask.
    gate.hold();
    let leaf = folio.runtime().focused_shell_address();
    let verification = lane
        .lane
        .decorate(MathWorkerRequest::InlineImage {
            leaf,
            task: bt_term::InlineImageTask {
                occurrence_id: 1,
                source: bt_term::InlineImageSource::LocalPath(file.clone()),
            },
        })
        .ok()
        .expect("the verification is admitted");
    assert!(gate.wait_entered(1), "and reaches the worker");
    a_picture_on(&mut folio, pane, &file);
    a_page_on(&mut folio, page, &file);
    folio.window.peek_hover.active = Some(crate::PeekCandidate {
        subject: crate::PeekSubject::from_path(file.clone()),
        seat: shell,
        pointer: PhysicalPosition::new(40.0, 40.0),
    });
    let now = Instant::now();
    folio.window.file_peek = Some(crate::test_support::card_over(
        crate::RowHost::Terminal(shell),
        &file,
        crate::PeekClock::Settling(now),
    ));
    folio
        .runtime()
        .advance_file_peek(now)
        .expect("the card comes up");
    folio.runtime().refresh_preview_for_layout();
    let asked = read_awaited(&folio, &file).expect("the surfaces' read is out");
    gate.release();
    let first = lane.next_ended(
        crate::computation_lane::Share::Decoration,
        crate::computation_lane::fixtures::nobody,
    );
    let second = lane.next_ended(
        crate::computation_lane::Share::Decoration,
        crate::computation_lane::fixtures::nobody,
    );
    assert_eq!(
        [first.1, second.1],
        [verification, asked],
        "the fixture's own precondition: the verification comes home first"
    );
    let generation = folio.window.markdown_pictures.generation;
    let builds = folio.window.renderer.overlay_builds;
    assert!(
        folio.window.peek_thumbnail_pending.is_none() && folio.window.peek_card_pending.is_none(),
        "the fixture's own precondition: neither the flyout nor the card has asked a resample"
    );
    folio.ledger.take();

    // The scale share is busy across the take, so every resample the take asks waits behind it.
    gate.hold();
    let entered = gate.entered();
    let busy = a_busy_resample(&lane, leaf);
    assert!(
        gate.wait_entered(entered + 1),
        "the busy resample reaches the worker"
    );
    the_window_takes(&mut folio, vec![first, second]);
    let picture = folio.window.tabs[0]
        .preview_panes
        .get(pane)
        .and_then(PreviewPane::image);
    assert_eq!(
        picture.and_then(|picture| picture.native),
        Some((600, 300)),
        "the pane is told the picture"
    );
    assert!(
        picture.is_some_and(|picture| picture.drawn.is_some()),
        "and places it"
    );
    assert!(
        folio.window.markdown_pictures.generation > generation,
        "the page's picture generation ticks"
    );
    assert!(
        folio.window.peek_thumbnail_pending.is_some(),
        "the flyout asks for its picture at its own size"
    );
    assert!(
        folio.window.renderer.overlay_builds > builds && folio.window.peek_card_pending.is_some(),
        "the card's overlay is rebuilt, and asks for its picture at its own size"
    );
    let effects = folio.ledger.take();
    assert!(
        effects.contains(&WindowEffect::Redraw),
        "and a redraw is asked: {effects:?}"
    );

    let flyout = folio
        .window
        .peek_thumbnail_pending
        .clone()
        .expect("the flyout's question");
    assert_ne!(
        Some(&flyout),
        folio.window.peek_card_pending.as_ref(),
        "the fixture's own precondition: the card and the flyout ask two sizes"
    );
    gate.release();
    let resamples = every_resample_but(&mut lane, busy);
    assert!(
        !resamples.iter().any(|(_, _, terminal, _)| matches!(
            terminal,
            crate::lane::Terminal::Superseded { .. }
        )),
        "neither surface's resample supersedes the other's"
    );
    the_window_takes(&mut folio, resamples);
    assert!(
        folio.window.peek_thumbnail_pending.is_none()
            && folio
                .window
                .peek_thumbnail
                .as_ref()
                .is_some_and(|thumbnail| thumbnail.matches(&flyout)),
        "the flyout holds its picture at its own size"
    );
    assert!(
        folio.window.peek_card_pending.is_none() && folio.window.peek_picture.is_some(),
        "and the card holds its own"
    );
}

/// **A resample put on the scale share ahead of the window's own**, under `leaf`: held by the
/// lane's gate, it keeps the worker busy while the requests the window asks next wait together.
fn a_busy_resample(
    lane: &crate::computation_lane::fixtures::Watched,
    leaf: crate::ShellAddress,
) -> ComputationId {
    lane.lane
        .scale(crate::ScaleWorkerRequest::Preview {
            leaf,
            asker: crate::PictureAsker::mint(),
            task: crate::test_support::scale_task("忙着 busy.png", 8),
        })
        .ok()
        .expect("the busy resample is admitted")
}

/// Every terminal the scale share ends until it holds nothing, but `busy`'s.
fn every_resample_but(
    lane: &mut crate::computation_lane::fixtures::Watched,
    busy: ComputationId,
) -> Vec<crate::computation_lane::Ended> {
    lane.every_ended(
        crate::computation_lane::Share::Scale,
        crate::computation_lane::fixtures::nobody,
    )
    .into_iter()
    .filter(|(_, id, ..)| *id != busy)
    .collect()
}

/// RED (T-PEEK-SCALE-ADDRESS-BY-SURFACE) — **the flyout is not left blank by a card over the same
/// file**: the flyout asks its resample while the scale share is busy, the pointer leaves its span
/// (the flyout comes down and its question stays out), a files-column row over the same file
/// raises the glance card, and the card asks its own resample behind the flyout's. Both land; the
/// next hover over the terminal's reference shows the flyout at once, from the picture its own
/// answer filed, and asks nothing again.
///
/// MUTATION: give both peek surfaces one purpose in `ScaleWorkerRequest::purpose` — the card's
/// request supersedes the flyout's, and the next hover finds the flyout's question still out and
/// shows nothing.
#[test]
fn a_card_over_the_flyouts_file_leaves_the_flyout_its_own_answer() {
    let (mut folio, mut lane, gate, shell, _panes) = two_panes_on_a_watched_lane("peek-by-surface");
    let file = scratch_png_named("各问各的 by surface");
    let _written = crate::test_support::ScratchFile(file.clone());
    std::fs::write(&file, png_bytes(600, 300)).expect("a scratch file this test owns");
    let hover = crate::PeekCandidate {
        subject: crate::PeekSubject::from_path(file.clone()),
        seat: shell,
        pointer: PhysicalPosition::new(40.0, 40.0),
    };

    // The flyout settles over the reference and reads the file.
    folio.window.peek_hover.active = Some(hover.clone());
    folio
        .runtime()
        .show_or_request_peek(&hover, true)
        .expect("the settled hover is answered");
    assert!(
        read_awaited(&folio, &file).is_some(),
        "the fixture's own precondition: the flyout reads the file"
    );
    let read = lane.next_ended(
        crate::computation_lane::Share::Decoration,
        crate::computation_lane::fixtures::nobody,
    );

    // The scale share is busy when the read lands, so the flyout's resample waits.
    gate.hold();
    let entered = gate.entered();
    let leaf = folio.runtime().focused_shell_address();
    let busy = a_busy_resample(&lane, leaf);
    assert!(
        gate.wait_entered(entered + 1),
        "the busy resample reaches the worker"
    );
    the_window_takes(&mut folio, vec![read]);
    let flyout = folio
        .window
        .peek_thumbnail_pending
        .clone()
        .expect("the flyout asks its resample");

    // The pointer leaves the reference for a cell of the shell that names nothing, through the
    // window's own pointer-move handler: the flyout comes down, and its question stays out.
    let body = crate::seats::pane_body_viewport(
        &folio.window.tabs[0].seats,
        &folio.window.tabs[0].seat_layout,
        shell,
        folio.window.renderer.scale_factor() as f32,
    )
    .expect("the shell has a body");
    folio.move_to(PhysicalPosition::new(
        f64::from(body.x) + 4.0,
        f64::from(body.y + body.height) - 4.0,
    ));
    assert!(
        folio.window.peek_hover.active.is_none(),
        "the fixture's own precondition: leaving the span hides the flyout"
    );
    assert_eq!(
        folio.window.peek_thumbnail_pending.as_ref(),
        Some(&flyout),
        "and its question stays out"
    );
    let now = Instant::now();

    // A files-column row over the same file raises the card, which asks its own resample.
    folio.window.file_peek = Some(crate::test_support::card_over(
        crate::RowHost::Column(shell),
        &file,
        crate::PeekClock::Settling(now),
    ));
    folio
        .runtime()
        .advance_file_peek(now)
        .expect("the card comes up");
    assert!(
        folio.window.peek_card_pending.is_some(),
        "the fixture's own precondition: the card asks its resample while the flyout's waits"
    );

    gate.release();
    let resamples = every_resample_but(&mut lane, busy);
    the_window_takes(&mut folio, resamples);
    assert!(
        folio.window.peek_card_pending.is_none() && folio.window.peek_picture.is_some(),
        "the card holds its picture"
    );

    // The next hover over the terminal's reference.
    folio.window.peek_hover.active = Some(hover.clone());
    folio
        .runtime()
        .show_or_request_peek(&hover, true)
        .expect("the next hover is answered");
    assert!(
        folio.window.renderer.peek_overlay.is_some(),
        "the next hover shows the flyout"
    );
    assert!(
        folio.window.peek_thumbnail_pending.is_none()
            && folio
                .window
                .peek_thumbnail
                .as_ref()
                .is_some_and(|thumbnail| thumbnail.matches(&flyout)),
        "from the picture its own answer filed"
    );
    assert_eq!(
        folio
            .app
            .math_worker
            .held(crate::computation_lane::Share::Scale),
        0,
        "and asks nothing again"
    );
}

/// RED (T-PEEK-SCALE-ADDRESS-BY-SURFACE review) — **a card answer the card no longer awaits is
/// dropped**: the card asks its resample while the scale share is busy and comes down
/// (`hide_file_peek`) before the answer lands. The late answer leaves `peek_picture` empty, as the
/// teardown left it.
///
/// MUTATION: adopt every card answer in `Runtime::complete_peek_scale` (drop the awaited-size
/// guard on the card's arm) — the late answer refills the slot of a card that is gone.
#[test]
fn a_late_card_answer_after_the_card_came_down_is_dropped() {
    let (mut folio, mut lane, gate, shell, _panes) = two_panes_on_a_watched_lane("peek-card-late");
    let file = scratch_png_named("晚到 late card");
    let _written = crate::test_support::ScratchFile(file.clone());
    std::fs::write(&file, png_bytes(600, 300)).expect("a scratch file this test owns");
    let now = Instant::now();
    folio.window.file_peek = Some(crate::test_support::card_over(
        crate::RowHost::Column(shell),
        &file,
        crate::PeekClock::Settling(now),
    ));
    folio
        .runtime()
        .advance_file_peek(now)
        .expect("the card comes up");
    let read = lane.next_ended(
        crate::computation_lane::Share::Decoration,
        crate::computation_lane::fixtures::nobody,
    );

    // The scale share is busy when the read lands, so the card's resample waits.
    gate.hold();
    let entered = gate.entered();
    let leaf = folio.runtime().focused_shell_address();
    let busy = a_busy_resample(&lane, leaf);
    assert!(
        gate.wait_entered(entered + 1),
        "the busy resample reaches the worker"
    );
    the_window_takes(&mut folio, vec![read]);
    assert!(
        folio.window.peek_card_pending.is_some(),
        "the fixture's own precondition: the card asks its resample"
    );

    assert!(
        folio.runtime().hide_file_peek(),
        "the card that was showing comes down"
    );
    assert!(
        folio.window.peek_card_pending.is_none() && folio.window.peek_picture.is_none(),
        "the fixture's own precondition: the card's teardown clears both slots"
    );
    gate.release();
    let resamples = every_resample_but(&mut lane, busy);
    assert!(
        !resamples.is_empty(),
        "the fixture's own precondition: the card's answer comes home"
    );
    the_window_takes(&mut folio, resamples);
    assert!(
        folio.window.peek_picture.is_none(),
        "a late card answer leaves the slot of a card that has gone empty"
    );
}

// ── every resample's address names its asker (T-SCALE-ADDRESS-PER-ASKER) ─────

/// `surface`'s picture, in the panes' tab.
fn picture_on(folio: &HeadlessFolio, surface: PreviewSurface) -> &crate::PreviewImageState {
    folio.window.tabs[0]
        .preview_panes
        .get(surface)
        .and_then(PreviewPane::image)
        .expect("the surface shows its picture")
}

/// **Both panes over one picture ask their resamples while the scale share is busy**: each shows
/// `file` (the second at `second_zoom`, when given), the window reads it, and the read lands while
/// a busy resample holds the worker. Answers the busy request and the two pending targets.
fn two_panes_ask_together(
    folio: &mut HeadlessFolio,
    lane: &mut crate::computation_lane::fixtures::Watched,
    gate: &crate::lane::Gate,
    panes: [PreviewSurface; 2],
    file: &Path,
    second_zoom: Option<f32>,
) -> (ComputationId, [crate::PeekThumbnailTarget; 2]) {
    for surface in panes {
        a_picture_on(folio, surface, file);
    }
    if let Some(scale) = second_zoom {
        folio.window.tabs[0]
            .preview_panes
            .entry(panes[1])
            .zoom_to(crate::ImageZoom::scaled(scale));
    }
    folio.runtime().refresh_preview_for_layout();
    assert!(
        read_awaited(folio, file).is_some(),
        "the fixture's own precondition: the panes read the file"
    );
    let read = lane.next_ended(
        crate::computation_lane::Share::Decoration,
        crate::computation_lane::fixtures::nobody,
    );
    gate.hold();
    let entered = gate.entered();
    let leaf = folio.runtime().focused_shell_address();
    let busy = a_busy_resample(lane, leaf);
    assert!(
        gate.wait_entered(entered + 1),
        "the busy resample reaches the worker"
    );
    the_window_takes(folio, vec![read]);
    let asked = panes.map(|surface| {
        picture_on(folio, surface)
            .pending
            .clone()
            .expect("each pane asks its resample")
    });
    (busy, asked)
}

/// RED (T-SCALE-ADDRESS-PER-ASKER) — **two preview panes over one picture at two sizes each get
/// their picture**: the panes ask while the scale share is busy, the second zoomed out so the two
/// questions are two sizes. Neither request supersedes the other, and each pane holds its own
/// size's raster with nothing left pending.
///
/// MUTATION: give every preview picture one address (`Self::Preview { .. } =>
/// ScalePurpose::Preview(PictureAsker(0))` in `ScaleWorkerRequest::purpose`) — the second pane's
/// request displaces the first's, and the first pane waits for good.
#[test]
fn two_preview_panes_at_two_sizes_each_get_their_picture() {
    let (mut folio, mut lane, gate, _shell, panes) =
        two_panes_on_a_watched_lane("preview-two-sizes");
    let file = scratch_png_named("两种大小 two sizes");
    let _written = crate::test_support::ScratchFile(file.clone());
    std::fs::write(&file, png_bytes(240, 120)).expect("a scratch file this test owns");
    let (busy, asked) =
        two_panes_ask_together(&mut folio, &mut lane, &gate, panes, &file, Some(0.25));
    assert_ne!(
        asked[0], asked[1],
        "the fixture's own precondition: the two panes ask two sizes"
    );
    gate.release();
    let resamples = every_resample_but(&mut lane, busy);
    assert!(
        !resamples.iter().any(|(_, _, terminal, _)| matches!(
            terminal,
            crate::lane::Terminal::Superseded { .. }
        )),
        "neither pane's resample supersedes the other's"
    );
    the_window_takes(&mut folio, resamples);
    for (surface, target) in panes.into_iter().zip(&asked) {
        let picture = picture_on(&folio, surface);
        assert!(
            picture.pending.is_none()
                && picture
                    .raster
                    .as_ref()
                    .is_some_and(|raster| raster.matches(target)),
            "each pane holds its picture at its own size ({surface:?})"
        );
    }
}

/// RED (T-SCALE-ADDRESS-PER-ASKER) — **two preview panes over one picture at one size are both
/// painted**: the panes ask the same size while the scale share is busy; each request is
/// answered, each answer goes to the pane that asked it, and the next frame draws both pictures.
///
/// MUTATIONS: give every preview picture one address (as above) — the second request joins the
/// first, its one answer goes to the first pane, and the second waits for good; deliver every
/// answer to the first pane showing the picture, whatever asked and whatever it awaits (the
/// `PreviewPanes::awaiting_scale` predicate made `true`) — both answers reach the first pane and
/// the second is never painted.
#[test]
fn two_preview_panes_at_one_size_are_both_painted() {
    let (mut folio, mut lane, gate, _shell, panes) =
        two_panes_on_a_watched_lane("preview-one-size");
    let file = scratch_png_named("同一大小 one size");
    let _written = crate::test_support::ScratchFile(file.clone());
    // Smaller than either pane, so both draw it at its own pixels.
    std::fs::write(&file, png_bytes(24, 12)).expect("a scratch file this test owns");
    let (busy, asked) = two_panes_ask_together(&mut folio, &mut lane, &gate, panes, &file, None);
    assert_eq!(
        asked[0], asked[1],
        "the fixture's own precondition: the two panes ask one size"
    );
    gate.release();
    let resamples = every_resample_but(&mut lane, busy);
    the_window_takes(&mut folio, resamples);
    for surface in panes {
        let picture = picture_on(&folio, surface);
        assert!(
            picture.pending.is_none()
                && picture
                    .raster
                    .as_ref()
                    .is_some_and(|raster| raster.matches(&asked[0])),
            "each pane holds its picture ({surface:?})"
        );
    }
    folio.draw();
    assert_eq!(
        folio.window.renderer.preview_images.len(),
        2,
        "and the next frame paints both"
    );
}

/// RED (T-SCALE-ADDRESS-PER-ASKER) — **one Markdown page's picture at two sizes: both sizes land
/// and nothing stays `Pending`**. The page reads its picture and owes the exact-size pass at the
/// size it draws it; the pass is sent while the scale share is busy; then the window is narrowed
/// (`Runtime::resized`), the page is laid out again to its narrower column and owes the same
/// picture at a second size, and sends that too while the first still waits. Both rasters land in the window's cache, and no entry of it is left `Pending`.
///
/// MUTATION: give every page picture one address (`Self::MarkdownImage { .. } =>
/// ScalePurpose::MarkdownImage((0, 0))` in `ScaleWorkerRequest::purpose`) — the second size
/// displaces the first, whose entry stays `Pending` and is never asked again.
#[test]
fn a_page_picture_at_two_sizes_lands_both_and_leaves_nothing_pending() {
    let (mut folio, mut lane, gate, _shell, [page, _]) =
        two_panes_on_a_watched_lane("markdown-two-sizes");
    let file = scratch_png_named("页面两种大小 page sizes");
    let _written = crate::test_support::ScratchFile(file.clone());
    // Wider than the page's column at either window width, so the page draws it to the column.
    std::fs::write(&file, png_bytes(2400, 1200)).expect("a scratch file this test owns");
    a_page_on(&mut folio, page, &file);
    folio.runtime().refresh_preview_for_layout();
    assert!(
        read_awaited(&folio, &file).is_some(),
        "the fixture's own precondition: the page reads its picture"
    );
    let read = lane.next_ended(
        crate::computation_lane::Share::Decoration,
        crate::computation_lane::fixtures::nobody,
    );
    the_window_takes(&mut folio, vec![read]);
    let first = folio
        .window
        .markdown_pictures
        .owed
        .values()
        .next()
        .cloned()
        .expect("the page owes its picture's exact-size pass");

    gate.hold();
    let entered = gate.entered();
    let leaf = folio.runtime().focused_shell_address();
    let busy = a_busy_resample(&lane, leaf);
    assert!(
        gate.wait_entered(entered + 1),
        "the busy resample reaches the worker"
    );
    let quiet = |folio: &HeadlessFolio| {
        folio
            .window
            .markdown_pictures
            .settle_deadline
            .expect("a pass is owed")
    };
    let deadline = quiet(&folio);
    folio
        .runtime()
        .finish_preview_scale_if_quiet(deadline)
        .expect("the quiet sends the first size");
    // The reader narrows the window while that pass is out, through the window's own resize door:
    // the page is laid out again to its narrower column and owes the picture at the new size.
    let wide = folio.window.window.size;
    let narrow = winit::dpi::PhysicalSize::new(wide.width * 2 / 3, wide.height);
    folio.window.window.size = narrow;
    folio
        .runtime()
        .resized(narrow)
        .expect("the window takes its new rectangle");
    folio.draw();
    let second = folio
        .window
        .markdown_pictures
        .owed
        .values()
        .next()
        .cloned()
        .expect("the narrower page owes its picture at the new size");
    assert_eq!(
        second.key.content, first.key.content,
        "the fixture's own precondition: the same picture"
    );
    assert_ne!(
        (second.key.width_px, second.key.height_px),
        (first.key.width_px, first.key.height_px),
        "the fixture's own precondition: at a second size"
    );
    let deadline = quiet(&folio);
    folio
        .runtime()
        .finish_preview_scale_if_quiet(deadline)
        .expect("the quiet sends the second size");
    for request in [&first, &second] {
        assert!(
            matches!(
                folio.window.markdown_pictures.raster(&request.key),
                Some(crate::MarkdownRaster::Pending)
            ),
            "the fixture's own precondition: both sizes are asked ({:?})",
            request.key
        );
    }

    gate.release();
    let resamples = every_resample_but(&mut lane, busy);
    the_window_takes(&mut folio, resamples);
    for request in [&first, &second] {
        assert!(
            matches!(
                folio.window.markdown_pictures.raster(&request.key),
                Some(crate::MarkdownRaster::Ready { .. })
            ),
            "both sizes land ({:?})",
            request.key
        );
    }
    assert!(
        !folio
            .window
            .markdown_pictures
            .rasters
            .values()
            .any(|(raster, _)| matches!(raster, crate::MarkdownRaster::Pending)),
        "and nothing stays pending"
    );
}

/// RED (T-SCALE-ADDRESS-PER-ASKER) — **a tab switch while the flyout's resample is out keeps the
/// answer for the flyout, which shows it on return**: the flyout asks while the scale share is
/// busy, the reader steps to another tab, the answer lands there, and back on the first tab the
/// next hover over the same reference shows the flyout from the picture that answer filed, asking
/// nothing again.
///
/// MUTATION: gate `complete_peek_scale` on the answer's tab being in front again
/// (`if target_active { … }` in `Runtime::apply_math_results`) — the answer is dropped, the
/// flyout's question stays out, and the next hover finds it "already asked" and shows nothing.
#[test]
fn a_flyout_answer_landing_on_another_tab_is_shown_on_return() {
    let (mut folio, mut lane, gate, shell, _panes) = two_panes_on_a_watched_lane_beside(
        "peek-tab-switch",
        vec![crate::test_support::cross_tab(2, &["另一个 other"])],
    );
    let file = scratch_png_named("切走 tab switch");
    let _written = crate::test_support::ScratchFile(file.clone());
    std::fs::write(&file, png_bytes(600, 300)).expect("a scratch file this test owns");
    let hover = crate::PeekCandidate {
        subject: crate::PeekSubject::from_path(file.clone()),
        seat: shell,
        pointer: PhysicalPosition::new(40.0, 40.0),
    };
    folio.window.peek_hover.active = Some(hover.clone());
    folio
        .runtime()
        .show_or_request_peek(&hover, true)
        .expect("the settled hover is answered");
    let read = lane.next_ended(
        crate::computation_lane::Share::Decoration,
        crate::computation_lane::fixtures::nobody,
    );
    gate.hold();
    let entered = gate.entered();
    let leaf = folio.runtime().focused_shell_address();
    let busy = a_busy_resample(&lane, leaf);
    assert!(
        gate.wait_entered(entered + 1),
        "the busy resample reaches the worker"
    );
    the_window_takes(&mut folio, vec![read]);
    let flyout = folio
        .window
        .peek_thumbnail_pending
        .clone()
        .expect("the flyout asks its resample");

    folio
        .runtime()
        .activate_tab(1, false)
        .expect("the reader steps to the other tab");
    assert!(
        folio.window.peek_hover.active.is_none(),
        "the fixture's own precondition: the switch takes the hover with it"
    );
    gate.release();
    let resamples = every_resample_but(&mut lane, busy);
    the_window_takes(&mut folio, resamples);
    assert!(
        folio.window.peek_thumbnail_pending.is_none()
            && folio
                .window
                .peek_thumbnail
                .as_ref()
                .is_some_and(|thumbnail| thumbnail.matches(&flyout)),
        "the answer is kept for the flyout that asked it"
    );

    folio
        .runtime()
        .activate_tab(0, false)
        .expect("the reader comes back");
    folio.window.peek_hover.active = Some(hover.clone());
    folio
        .runtime()
        .show_or_request_peek(&hover, true)
        .expect("the next hover is answered");
    assert!(
        folio.window.renderer.peek_overlay.is_some(),
        "the next hover shows the flyout"
    );
    assert_eq!(
        folio
            .app
            .math_worker
            .held(crate::computation_lane::Share::Scale),
        0,
        "and asks nothing again"
    );
}

/// RED (T-NATIVE-LABELS-FROM-THE-CALLER) — **`Choose…` on the Background image row asks the
/// system's chooser for pictures, with the filter's name from the interface table**: the words
/// cross with the call, and `bt-platform` names the filter with what it was handed.
///
/// MUTATION: hand `image_picker.request` any other string in
/// `Runtime::browse_for_background_image` and the name the chooser was asked with is not the
/// table's.
#[test]
fn the_picture_chooser_is_asked_with_the_tables_name_for_its_filter() {
    let mut folio = crate::headless::HeadlessFolio::with_tabs(
        "native-labels-picture-chooser",
        vec![crate::test_support::cross_tab(1, &["一 one"])],
        0,
    );
    folio.ledger.take();
    folio.runtime().browse_for_background_image();
    let asked: Vec<(bt_platform::FilePickKind, String)> = folio
        .ledger
        .take()
        .into_iter()
        .filter_map(|effect| match effect {
            crate::headless::WindowEffect::FileAsked(kind, _, name) => Some((kind, name)),
            _ => None,
        })
        .collect();
    assert_eq!(
        asked,
        vec![(
            bt_platform::FilePickKind::Image,
            crate::i18n::Text::ImageFileType.text().to_owned()
        )],
    );
}
