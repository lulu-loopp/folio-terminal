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
use winit::keyboard::KeyCode;

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

// ── a recording's picture, driven (T-VIDEO-POINTER-AND-KEYS) ─────────────────────────────────

/// The recording every host's engine decodes: five seconds at five frames a second.
fn the_recording() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets/folio-video-test.mp4")
}

/// A window whose preview pane stands the recording's still beside a shell that holds the focus,
/// drawn once.
fn a_recording_beside_a_shell(tag: &str) -> (HeadlessFolio, PreviewSurface) {
    let (tab, seat) = tab_with_a_preview(1, vec![]);
    let mut folio = HeadlessFolio::with_tabs(tag, vec![tab], 0);
    let surface = seat_of(TabId(1), seat);
    a_picture_on(&mut folio, surface, &the_recording());
    folio.draw();
    assert_ne!(
        folio.window.tabs[0].seats.focus(),
        seat,
        "the fixture's own precondition: the shell holds the focus"
    );
    (folio, surface)
}

/// The rectangle the recording is drawn in on `surface`.
fn stage_of(folio: &mut HeadlessFolio, surface: PreviewSurface) -> [f32; 4] {
    folio
        .runtime()
        .video_shape_of(surface, 1.0, Instant::now())
        .expect("the pane stages the recording")
        .rect()
}

/// A point on the picture `left` pixels in from its left edge and a quarter of the way down —
/// clear of the bar along its bottom and of the play disc in its middle.
fn on_the_picture(stage: [f32; 4], left: f32) -> PhysicalPosition<f64> {
    PhysicalPosition::new(
        f64::from(stage[0] + left),
        f64::from(stage[1] + (stage[3] - stage[1]) / 4.0),
    )
}

/// One click: down and up at `at`.
fn click(folio: &mut HeadlessFolio, at: PhysicalPosition<f64>) {
    folio.press(at);
    folio.release(at);
}

/// The engine's state on `surface`, once `until` holds.
fn played(
    folio: &HeadlessFolio,
    surface: PreviewSurface,
    until: impl Fn(&bt_platform::video::engine::EngineState) -> bool,
) -> bt_platform::video::engine::EngineState {
    folio
        .window
        .video
        .get(surface)
        .expect("the surface is playing the recording")
        .state_reaching(until)
}

/// The recording on `surface` loaded and playing — a seat publishes the play it was opened with
/// before the file has said how long it is, and `End` has no end to go to until it has.
fn loaded(folio: &HeadlessFolio, surface: PreviewSurface) {
    let state = played(folio, surface, |state| state.ready && state.playing);
    assert!(
        state.ready && state.playing,
        "the recording plays: {state:?}"
    );
}

/// The centre of a box.
fn centre(rect: [f32; 4]) -> PhysicalPosition<f64> {
    PhysicalPosition::new(
        f64::from((rect[0] + rect[2]) / 2.0),
        f64::from((rect[1] + rect[3]) / 2.0),
    )
}

/// A key the pane's player answers, pressed with no modifier.
fn pane_key(
    folio: &mut HeadlessFolio,
    key: winit::keyboard::Key,
    code: KeyCode,
    text: Option<&str>,
) {
    assert!(
        folio
            .runtime()
            .preview_key(&key_down(key, code, text))
            .expect("a headless key is answered"),
        "the pane takes {code:?}"
    );
}

/// RED (T-VIDEO-POINTER-AND-KEYS, row 38) — **one click on a recording's picture plays it and the
/// next pauses it; the first click on a pane that does not hold the focus focuses it and plays;
/// a press that wanders is not a click; and a press on the bar's own ground is the bar's.**
///
/// MUTATION: drop `PreviewBodyRung::VideoPicture` from `PreviewBodyRung::LADDER` — the press
/// focuses the pane and nothing plays, and the first assertion goes red. MUTATION: drop the
/// `wandered` test from `Runtime::release_video_picture` — the drag pauses the recording.
#[test]
fn a_click_on_a_recordings_picture_plays_and_pauses_it() {
    let _ledger = crate::test_support::ledger_gate();
    let before = bt_platform::video::engine::engines_outstanding();
    let (mut folio, surface) = a_recording_beside_a_shell("video-picture-click");
    let PreviewSurface::Seat(leaf) = surface else {
        unreachable!("a docked pane")
    };
    let stage = stage_of(&mut folio, surface);

    click(&mut folio, on_the_picture(stage, 20.0));
    assert_eq!(
        folio.window.tabs[0].seats.focus(),
        leaf.seat,
        "the first click focuses the pane"
    );
    assert!(
        folio.window.video.get(surface).is_some(),
        "and starts the recording it shows"
    );
    loaded(&folio, surface);

    // The next click, far enough along not to pair with the first, pauses.
    click(&mut folio, on_the_picture(stage, 60.0));
    assert!(!played(&folio, surface, |state| !state.playing).playing);

    // A press that wanders past the double click's slop before it is let go is a drag.
    let from = on_the_picture(stage, 100.0);
    folio.press(from);
    folio.move_to(PhysicalPosition::new(from.x + 40.0, from.y + 10.0));
    folio.release(PhysicalPosition::new(from.x + 40.0, from.y + 10.0));
    assert!(
        !folio
            .window
            .video
            .get(surface)
            .expect("still there")
            .state()
            .playing,
        "a drag across the picture is no click"
    );

    // The bar's own ground — its elapsed clock, which is no control — is the bar's.
    let layout = folio
        .runtime()
        .video_bar_layout_of(surface)
        .expect("a paused recording's bar is up");
    let clock = layout.at.expect("a pane's bar has its elapsed clock");
    click(&mut folio, centre(clock));
    assert!(
        !folio
            .window
            .video
            .get(surface)
            .expect("still there")
            .state()
            .playing,
        "a press on the bar is not a press on the picture"
    );
    folio.window.video.shutdown_all();
    crate::test_support::engines_settling_to(before);
}

/// RED (T-VIDEO-POINTER-AND-KEYS, row 39) — **a double click on the picture zooms the pane and
/// leaves playback as it was**: playing stays playing, paused stays paused at its playhead, and a
/// second double click lets the zoom go.
///
/// MUTATION: drop the undo from `Runtime::double_click_video_picture` — the first click of the
/// pair pauses a playing recording and the pane zooms over a stopped one.
#[test]
fn a_double_click_on_a_recording_zooms_its_pane_and_leaves_playback_as_it_was() {
    let _ledger = crate::test_support::ledger_gate();
    let before = bt_platform::video::engine::engines_outstanding();
    let (mut folio, surface) = a_recording_beside_a_shell("video-picture-double-click");
    let PreviewSurface::Seat(leaf) = surface else {
        unreachable!("a docked pane")
    };
    let stage = stage_of(&mut folio, surface);
    click(&mut folio, on_the_picture(stage, 20.0));
    loaded(&folio, surface);
    played(&folio, surface, |state| {
        state.playing && state.position_secs > 0.2
    });
    assert_eq!(folio.window.tabs[0].seats.zoom(), None);

    let pair = on_the_picture(stage, 80.0);
    click(&mut folio, pair);
    click(&mut folio, pair);
    assert_eq!(
        folio.window.tabs[0].seats.zoom(),
        Some(leaf.seat),
        "a double click is the pane's zoom"
    );
    let still_playing = played(&folio, surface, |state| state.playing);
    assert!(
        still_playing.playing,
        "and playing stays playing: {still_playing:?}"
    );

    // Paused at a playhead: the pair leaves it paused there.
    pane_key(
        &mut folio,
        Key::Named(NamedKey::Space),
        KeyCode::Space,
        Some(" "),
    );
    let paused = played(&folio, surface, |state| !state.playing);
    let stage = stage_of(&mut folio, surface);
    let pair = on_the_picture(stage, 140.0);
    click(&mut folio, pair);
    click(&mut folio, pair);
    assert_eq!(
        folio.window.tabs[0].seats.zoom(),
        None,
        "and a second lets it go"
    );
    let after = played(&folio, surface, |state| {
        !state.playing && (state.position_secs - paused.position_secs).abs() < 0.05
    });
    assert!(
        !after.playing && (after.position_secs - paused.position_secs).abs() < 0.05,
        "paused stays paused at {}: {after:?}",
        paused.position_secs
    );
    folio.window.video.shutdown_all();
    crate::test_support::engines_settling_to(before);
}

/// RED (T-VIDEO-POINTER-AND-KEYS, row 40) — **a wheel notch over the picture is one volume step;
/// over the bar's tracks it is the tracks', and with `Ctrl` held it keeps its window-wide
/// meaning.**
///
/// MUTATION: drop the `modifiers.is_empty()` test from the wheel's video station — `Ctrl`-wheel
/// over the picture moves the volume and the last assertion goes red.
#[test]
fn a_wheel_notch_over_a_recordings_picture_is_its_volume() {
    let _ledger = crate::test_support::ledger_gate();
    let before = bt_platform::video::engine::engines_outstanding();
    let (mut folio, surface) = a_recording_beside_a_shell("video-picture-wheel");
    let stage = stage_of(&mut folio, surface);
    let over = on_the_picture(stage, 20.0);
    click(&mut folio, over);
    loaded(&folio, surface);
    folio
        .window
        .video
        .get_mut(surface)
        .expect("playing")
        .set_volume(0.5, Instant::now());
    played(&folio, surface, |state| (state.volume - 0.5).abs() < 1e-6);
    let notch = |folio: &mut HeadlessFolio, at: PhysicalPosition<f64>, lines: f32| {
        folio.move_to(at);
        let mut runtime = folio.runtime();
        runtime
            .queue_wheel(winit::event::MouseScrollDelta::LineDelta(0.0, lines))
            .expect("a notch is taken");
        runtime.flush_wheel().expect("and spent");
    };

    notch(&mut folio, over, 1.0);
    let louder = played(&folio, surface, |state| (state.volume - 0.55).abs() < 1e-6);
    assert!(
        (louder.volume - 0.55).abs() < 1e-6,
        "one notch up: {louder:?}"
    );
    notch(&mut folio, over, -2.0);
    let quieter = played(&folio, surface, |state| (state.volume - 0.45).abs() < 1e-6);
    assert!(
        (quieter.volume - 0.45).abs() < 1e-6,
        "two down: {quieter:?}"
    );

    // Over the bar's elapsed clock: not the picture's.
    let layout = folio
        .runtime()
        .video_bar_layout_of(surface)
        .expect("the bar is up");
    notch(
        &mut folio,
        centre(layout.at.expect("the elapsed clock")),
        1.0,
    );
    // With `Ctrl` held, over the picture: the window's own meaning.
    folio.window.modifiers = winit::keyboard::ModifiersState::CONTROL;
    notch(&mut folio, over, 1.0);
    folio.window.modifiers = winit::keyboard::ModifiersState::empty();
    // One notch the picture does take, so the wait below ends on a state the two above had
    // their chance to change.
    notch(&mut folio, over, -1.0);
    let last = played(&folio, surface, |state| (state.volume - 0.40).abs() < 1e-6);
    assert!(
        (last.volume - 0.40).abs() < 1e-6,
        "neither the bar's notch nor Ctrl's moved the volume: {last:?}"
    );
    folio.window.video.shutdown_all();
    crate::test_support::engines_settling_to(before);
}

/// RED (T-VIDEO-POINTER-AND-KEYS, rows 42 and 43) — **`End` takes a recording to its end, and
/// every play door then plays it from the start**: the bar's button, Space, and a click on the
/// picture. `Home` is the start.
///
/// MUTATION: start a recording from where it stands in `VideoSeat::toggle` rather than from the
/// start when it [`video_seat`] stands at its end — the three doors each play the last frame and
/// stop, and each goes red.
#[test]
fn every_play_door_plays_an_ended_recording_from_the_start() {
    let _ledger = crate::test_support::ledger_gate();
    let before = bt_platform::video::engine::engines_outstanding();
    let (mut folio, surface) = a_recording_beside_a_shell("video-every-play-door");
    let stage = stage_of(&mut folio, surface);
    // A click starts it and focuses the pane, so the keys below are the pane's.
    click(&mut folio, on_the_picture(stage, 20.0));
    loaded(&folio, surface);
    pane_key(
        &mut folio,
        Key::Named(NamedKey::Space),
        KeyCode::Space,
        Some(" "),
    );
    played(&folio, surface, |state| !state.playing);

    /// One way to start a recording: the folio and the surface it is on.
    type Door = fn(&mut HeadlessFolio, PreviewSurface);
    let doors: [(&str, Door); 3] = [
        ("the bar's button", |folio, surface| {
            let layout = folio
                .runtime()
                .video_bar_layout_of(surface)
                .expect("the bar is up");
            click(folio, centre(layout.play));
        }),
        ("Space", |folio, _| {
            pane_key(
                folio,
                Key::Named(NamedKey::Space),
                KeyCode::Space,
                Some(" "),
            );
        }),
        ("a click on the picture", |folio, surface| {
            let stage = stage_of(folio, surface);
            click(folio, on_the_picture(stage, 160.0));
        }),
    ];
    for (door, open) in doors {
        pane_key(&mut folio, Key::Named(NamedKey::End), KeyCode::End, None);
        let at_end = played(&folio, surface, |state| state.position_secs >= 4.9);
        assert!(
            at_end.position_secs >= 4.9 && !at_end.playing,
            "{door}: {at_end:?}"
        );
        open(&mut folio, surface);
        let again = played(&folio, surface, |state| {
            state.playing && state.position_secs < 2.0
        });
        assert!(
            again.playing && again.position_secs < 2.0,
            "{door} plays an ended recording from the start: {again:?}"
        );
        pane_key(
            &mut folio,
            Key::Named(NamedKey::Space),
            KeyCode::Space,
            Some(" "),
        );
        played(&folio, surface, |state| !state.playing);
    }
    pane_key(&mut folio, Key::Named(NamedKey::Home), KeyCode::Home, None);
    let start = played(&folio, surface, |state| state.position_secs < 0.05);
    assert!(start.position_secs < 0.05, "Home is the start: {start:?}");
    folio.window.video.shutdown_all();
    crate::test_support::engines_settling_to(before);
}

/// RED (T-VIDEO-POINTER-AND-KEYS, rows 38–40, the float's face) — **a floating window's picture
/// answers a click, a double click and the wheel as a pane's does**, through the same door; and a
/// double click there, where a recording has no zoom, only leaves playback as it was.
///
/// MUTATION: drop the `VideoPicture` arm from `PreviewBodyRung::LADDER` — the float's body takes
/// the press for nothing and the first assertion goes red.
#[test]
fn a_floating_windows_picture_answers_as_a_panes_does() {
    let _ledger = crate::test_support::ledger_gate();
    let before = bt_platform::video::engine::engines_outstanding();
    let (mut folio, surface) = a_recording_beside_a_shell("video-picture-float");
    let PreviewSurface::Seat(leaf) = surface else {
        unreachable!("a docked pane")
    };
    folio
        .runtime()
        .pop_out_preview(leaf.seat)
        .expect("the recording is torn off");
    folio.draw();
    let id = folio
        .window
        .float
        .live_windows()
        .next()
        .expect("a window holds the recording")
        .id();
    let float = PreviewSurface::Float(id);
    folio.draw();
    let stage = stage_of(&mut folio, float);

    click(&mut folio, on_the_picture(stage, 20.0));
    loaded(&folio, float);
    click(&mut folio, on_the_picture(stage, 60.0));
    let paused = played(&folio, float, |state| !state.playing);
    assert!(!paused.playing, "the next click pauses: {paused:?}");

    let pair = on_the_picture(stage, 100.0);
    click(&mut folio, pair);
    click(&mut folio, pair);
    let after = played(&folio, float, |state| {
        !state.playing && (state.position_secs - paused.position_secs).abs() < 0.05
    });
    assert!(
        !after.playing && (after.position_secs - paused.position_secs).abs() < 0.05,
        "a double click leaves it paused where it was: {after:?}"
    );
    assert_eq!(
        folio.window.tabs[0].seats.zoom(),
        None,
        "and zooms no pane of the tab behind it"
    );

    let volume = folio
        .window
        .video
        .get(float)
        .expect("playing")
        .state()
        .volume;
    let at = on_the_picture(stage, 20.0);
    folio.move_to(at);
    let mut runtime = folio.runtime();
    runtime
        .queue_wheel(winit::event::MouseScrollDelta::LineDelta(0.0, -1.0))
        .expect("a notch is taken");
    runtime.flush_wheel().expect("and spent");
    let quieter = played(&folio, float, |state| {
        (state.volume - (volume - 0.05)).abs() < 1e-6
    });
    assert!(
        (quieter.volume - (volume - 0.05)).abs() < 1e-6,
        "a notch over the window's picture is one step: {quieter:?}"
    );
    folio.window.video.shutdown_all();
    crate::test_support::engines_settling_to(before);
}

/// **The double click's pair window runs out** for the last press any tab of `folio` counted — its
/// moment set back past [`crate::MULTI_CLICK_INTERVAL`], as the clock would have moved it — so the
/// next press is a first one.
fn the_pair_window_lapses(folio: &mut HeadlessFolio) {
    for tab in &mut folio.window.tabs {
        if let Some((_, _, when)) = tab.preview_image_clicks.last.as_mut() {
            *when = when
                .checked_sub(crate::MULTI_CLICK_INTERVAL + Duration::from_millis(1))
                .expect("the clock has run longer than one pair window");
        }
    }
}

/// The window's own loop, run headless for `span`: each turn answered, and the next one taken
/// when the turn said it wants waking (or at the end of the span) — nothing drawn that the loop
/// itself did not draw.
fn run_the_loop_for(folio: &mut HeadlessFolio, span: Duration) {
    let end = Instant::now() + span;
    loop {
        let now = Instant::now();
        if now >= end {
            return;
        }
        let wake = folio
            .runtime()
            .turn(now, true)
            .expect("the turn is answered");
        let until = wake
            .map_or(end, |(_, at)| at.min(end))
            .max(now + Duration::from_millis(1));
        std::thread::sleep(until.saturating_duration_since(Instant::now()));
    }
}

/// **The labels the window's overlay is drawing over a recording's bar** — inside the bar's
/// width, standing on its top edge (a label's bottom is a few pixels clear of it, the bar's own
/// gap) — and nothing else the overlay holds: a card or a dialog drawn over the window is not the
/// bar's.
fn labels_over_the_bar(
    folio: &HeadlessFolio,
    layout: &crate::video_seat::BarLayout,
) -> Vec<bt_render::ChromeLabel> {
    let [left, top, right, _] = layout.bar;
    folio
        .window
        .renderer
        .overlay
        .0
        .iter()
        .flat_map(|layer| layer.labels.iter())
        .filter(|label| {
            label.rect[0] >= left
                && label.rect[2] <= right
                && label.rect[3] <= top
                && label.rect[3] >= top - 12.0
        })
        .cloned()
        .collect()
}

/// A recording beside a shell, played by a click, loaded, then paused — the scrubber's usual
/// case — with the bar up and drawn. Returns the bar's layout.
fn a_paused_recording(tag: &str) -> (HeadlessFolio, PreviewSurface, crate::video_seat::BarLayout) {
    let (mut folio, surface) = a_recording_beside_a_shell(tag);
    // This window's launch has had its first-run card already. The loop the tests below run
    // turns the application's clocks, and a headless window is a first launch: on a machine
    // whose agent folders and install channel settle within the test, the card would come up
    // over the pane and take the wheel's notch and the overlay's labels.
    folio.app.first_run_attempted = true;
    let stage = stage_of(&mut folio, surface);
    click(&mut folio, on_the_picture(stage, 20.0));
    loaded(&folio, surface);
    folio
        .window
        .video
        .get_mut(surface)
        .expect("playing")
        .toggle(Instant::now());
    let paused = played(&folio, surface, |state| !state.playing);
    assert!(!paused.playing, "the fixture pauses: {paused:?}");
    run_the_loop_for(&mut folio, Duration::from_millis(300));
    folio.draw();
    let layout = folio
        .runtime()
        .video_bar_layout_of(surface)
        .expect("a paused recording's bar is up");
    (folio, surface, layout)
}

/// RED (T-VIDEO-POINTER-AND-KEYS review round 1, blocker 1) — **what a click did is put back only
/// by the press that pairs with that click**: a drag across the picture is no click, and a press
/// that pairs with the drag's own press must not undo a click made long before it.
///
/// MUTATION: keep `WindowRuntime::video_click` across a first press in
/// `Runtime::press_video_picture` — the pairing press plays the paused recording from where the
/// old click found it, and the assertion goes red.
#[test]
fn a_press_pairing_with_a_drag_puts_back_no_older_click() {
    let _ledger = crate::test_support::ledger_gate();
    let before = bt_platform::video::engine::engines_outstanding();
    let (mut folio, surface) = a_recording_beside_a_shell("video-picture-drag-pair");
    let stage = stage_of(&mut folio, surface);
    click(&mut folio, on_the_picture(stage, 20.0));
    loaded(&folio, surface);
    the_pair_window_lapses(&mut folio);
    // A click that pauses, and records what it did.
    click(&mut folio, on_the_picture(stage, 60.0));
    let paused = played(&folio, surface, |state| !state.playing);
    assert!(!paused.playing, "the click pauses: {paused:?}");
    the_pair_window_lapses(&mut folio);
    // A drag across the picture, then a press where the drag began, inside the pair window.
    let origin = on_the_picture(stage, 100.0);
    folio.press(origin);
    let away = PhysicalPosition::new(origin.x + 40.0, origin.y);
    folio.move_to(away);
    folio.release(away);
    folio.press(origin);
    folio.release(origin);
    // A fence: the engine answers its commands in order, so once this volume has landed any play
    // or seek the pair had sent has landed before it.
    folio
        .window
        .video
        .get_mut(surface)
        .expect("the seat stands")
        .set_volume(0.3, Instant::now());
    let after = played(&folio, surface, |state| (state.volume - 0.3).abs() < 1e-6);
    assert!(
        !after.playing && (after.position_secs - paused.position_secs).abs() < 0.05,
        "the pairing press left playback as it was: {after:?} against {paused:?}"
    );
    folio.window.video.shutdown_all();
    crate::test_support::engines_settling_to(before);
}

/// RED (T-VIDEO-POINTER-AND-KEYS review round 1, blocker 2) — **the scrubber's time follows the
/// hand on a paused recording**, painted by the window's own loop: no decoded frame comes to
/// redraw the bar while it is paused, so the move itself owes the frame.
///
/// MUTATION: have `Runtime::note_video_hover` ask for no frame — the label never reaches the
/// overlay and the first assertion goes red.
#[test]
fn the_scrubbers_time_follows_the_hand_on_a_paused_recording() {
    let _ledger = crate::test_support::ledger_gate();
    let before = bt_platform::video::engine::engines_outstanding();
    let (mut folio, surface, layout) = a_paused_recording("video-scrub-label-paused");
    let track = layout.seek;
    let row = f64::from((track[1] + track[3]) / 2.0);
    let along = |fraction: f32| {
        PhysicalPosition::new(f64::from(track[0] + (track[2] - track[0]) * fraction), row)
    };
    let said = |folio: &HeadlessFolio| -> Vec<(String, f32)> {
        labels_over_the_bar(folio, &layout)
            .into_iter()
            .map(|label| (label.text, (label.rect[0] + label.rect[2]) / 2.0))
            .collect()
    };
    assert!(
        said(&folio).is_empty(),
        "no label before the hand is on the scrubber"
    );

    for (fraction, text) in [(0.3, "0:01"), (0.7, "0:03")] {
        let at = along(fraction);
        folio.move_to(at);
        run_the_loop_for(&mut folio, Duration::from_millis(120));
        let labels = said(&folio);
        assert!(
            labels
                .iter()
                .any(|(said, centre)| said == text && (centre - at.x as f32).abs() <= 2.0),
            "at {fraction} of the track the overlay says {text} over the hand: {labels:?}"
        );
    }
    // Off the bar, onto the picture: the label goes.
    let stage = stage_of(&mut folio, surface);
    folio.move_to(on_the_picture(stage, 20.0));
    run_the_loop_for(&mut folio, Duration::from_millis(120));
    assert!(
        said(&folio).is_empty(),
        "the label leaves with the hand: {:?}",
        said(&folio)
    );
    folio.window.video.shutdown_all();
    crate::test_support::engines_settling_to(before);
}

/// RED (T-VIDEO-POINTER-AND-KEYS review round 1, blocker 2) — **the volume's flash goes out on a
/// paused recording** when its clock runs out, by the window's own loop.
///
/// MUTATION: drop the bars' fold from `Runtime::advance_strip_animation` — the loop wakes for the
/// flash's end, finds nothing owed, and the panel stands; the last assertion goes red.
#[test]
fn the_volumes_flash_goes_out_on_a_paused_recording() {
    let _ledger = crate::test_support::ledger_gate();
    let before = bt_platform::video::engine::engines_outstanding();
    let (mut folio, surface, layout) = a_paused_recording("video-gain-flash-paused");
    let stage = stage_of(&mut folio, surface);
    let over = on_the_picture(stage, 20.0);
    folio.move_to(over);
    {
        let mut runtime = folio.runtime();
        runtime
            .queue_wheel(winit::event::MouseScrollDelta::LineDelta(0.0, -1.0))
            .expect("a notch is taken");
        runtime.flush_wheel().expect("and spent");
    }
    let flashed = |folio: &HeadlessFolio| {
        labels_over_the_bar(folio, &layout)
            .iter()
            .any(|label| label.text.ends_with('%'))
    };
    run_the_loop_for(&mut folio, Duration::from_millis(100));
    assert!(
        flashed(&folio),
        "the notch flashes the volume: {:?} bar {:?}",
        folio
            .window
            .renderer
            .overlay
            .0
            .iter()
            .flat_map(|l| l.labels.iter())
            .map(|l| (l.text.clone(), l.rect))
            .collect::<Vec<_>>(),
        layout.bar
    );
    run_the_loop_for(
        &mut folio,
        crate::FOOT_REVEAL_FEEDBACK + Duration::from_millis(300),
    );
    assert!(
        !flashed(&folio),
        "the flash is gone once its clock has run out: {:?}",
        labels_over_the_bar(&folio, &layout)
    );
    folio.window.video.shutdown_all();
    crate::test_support::engines_settling_to(before);
}

/// RED (T-VIDEO-POINTER-AND-KEYS review round 2) — **a click's record answers only for the seat it
/// was made on**: once another recording has been put on the surface between a click and the
/// press that pairs with it (file travel lands the next file's still, and its play disc opens a
/// new seat), the pair must not put the old one's playhead onto the new one.
///
/// MUTATION: drop the seat-key test from `Runtime::double_click_video_picture`'s filter — the
/// pair pauses the new recording at the old one's playhead and the last assertion goes red.
#[test]
fn a_pair_after_the_seat_is_replaced_puts_nothing_back_on_the_new_one() {
    let _ledger = crate::test_support::ledger_gate();
    let before = bt_platform::video::engine::engines_outstanding();
    let (mut folio, surface) = a_recording_beside_a_shell("video-picture-replaced-pair");
    let stage = stage_of(&mut folio, surface);
    let at = on_the_picture(stage, 20.0);
    click(&mut folio, at);
    loaded(&folio, surface);
    folio
        .window
        .video
        .get_mut(surface)
        .expect("playing")
        .toggle(Instant::now());
    let paused = played(&folio, surface, |state| !state.playing);
    the_pair_window_lapses(&mut folio);
    // A click that plays from `paused.position_secs`, recording "pause it there" as its undo.
    click(&mut folio, at);
    played(&folio, surface, |state| state.playing);
    let first = folio
        .window
        .video
        .get(surface)
        .expect("the seat stands")
        .key()
        .to_owned();
    // Another recording put on the surface: file travel's own door lands the next file's still,
    // and the door the still's play disc takes plays it.
    let next =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/assets/folio-video-test.mov");
    folio
        .runtime()
        .open_preview_onto(surface, next.clone())
        .expect("travel lands the next file");
    folio.draw();
    folio
        .runtime()
        .play_video_file_on(surface, &next)
        .expect("the next recording opens");
    let replaced = folio
        .window
        .video
        .get(surface)
        .expect("a seat plays the next recording")
        .key()
        .to_owned();
    assert_ne!(
        replaced, first,
        "the fixture's own precondition: another seat"
    );
    // The press that pairs with the click.
    folio.press(at);
    folio.release(at);
    // A fence: the engine answers its commands in order, so once this volume has landed any
    // pause or seek the pair had sent has landed before it.
    folio
        .window
        .video
        .get_mut(surface)
        .expect("the seat stands")
        .set_volume(0.3, Instant::now());
    let after = played(&folio, surface, |state| {
        state.ready && (state.volume - 0.3).abs() < 1e-6
    });
    assert!(
        after.playing,
        "the pair put the old recording's pause at {:.2} s onto the new one: {after:?}",
        paused.position_secs
    );
    folio.window.video.shutdown_all();
    crate::test_support::engines_settling_to(before);
}

// ── a picture's keys, driven (T-PICTURE-KEYS-PAN-AND-ZOOM) ──────────────────────────────────

/// The decode's size every picture below claims: larger than any headless pane at 100 %, on
/// both axes.
const LARGE: [u32; 2] = [2000, 1500];

/// A picture of [`LARGE`] whose decode has answered, standing on `surface`. The file is never
/// written: nothing here pumps the decode lane, so the size the surface was told is the size it
/// keeps, and every key below is asked of geometry alone.
fn a_large_picture_on(folio: &mut HeadlessFolio, surface: PreviewSurface, file: &Path) {
    let mut picture = crate::PreviewImageState::new(file.to_path_buf());
    picture.native = Some((LARGE[0], LARGE[1]));
    folio.window.tabs[0]
        .preview_panes
        .entry(surface)
        .show_picture_state(picture);
}

/// A window whose preview pane shows a [`LARGE`] picture beside a shell, drawn once. With
/// `focused` the pane holds the keyboard; without it the shell does.
fn a_large_picture_beside_a_shell(
    tag: &str,
    name: &str,
    focused: bool,
) -> (HeadlessFolio, PreviewSurface) {
    let (tab, seat) = tab_with_a_preview(1, vec![]);
    let mut folio = HeadlessFolio::with_tabs(tag, vec![tab], 0);
    let surface = seat_of(TabId(1), seat);
    a_large_picture_on(&mut folio, surface, &scratch_png_named(name));
    if focused {
        assert!(
            folio.window.tabs[0].seats.set_focus(seat),
            "the preview pane accepts the focus"
        );
    }
    folio.draw();
    (folio, surface)
}

/// The body `surface` hangs its picture in.
fn picture_body(folio: &mut HeadlessFolio, surface: PreviewSurface) -> [f32; 4] {
    folio
        .runtime()
        .preview_image_geometry(surface)
        .expect("the surface knows its picture's size and its body")
        .0
}

fn zoom_of(folio: &mut HeadlessFolio, surface: PreviewSurface) -> crate::ImageZoom {
    folio.runtime().preview_image_zoom(surface)
}

/// The file `surface` is showing.
fn picture_path(folio: &mut HeadlessFolio, surface: PreviewSurface) -> std::path::PathBuf {
    folio
        .runtime()
        .preview_picture(surface)
        .expect("the surface shows a picture")
        .path
        .clone()
}

/// One key pressed with `modifiers` held, **through the whole keyboard ladder**
/// (`Runtime::keyboard_input`): every rung above the preview's — the popups, the shortcut table,
/// the files column — is asked first, as it is for a hand.
fn picture_key(
    folio: &mut HeadlessFolio,
    modifiers: winit::keyboard::ModifiersState,
    key: winit::keyboard::Key,
    code: KeyCode,
    text: Option<&str>,
) {
    folio.window.modifiers = modifiers;
    folio
        .runtime()
        .keyboard_input(&key_down(key, code, text), false)
        .expect("a headless key is answered");
    folio.window.modifiers = winit::keyboard::ModifiersState::empty();
}

fn arrow(folio: &mut HeadlessFolio, named: NamedKey, code: KeyCode) {
    picture_key(
        folio,
        winit::keyboard::ModifiersState::empty(),
        Key::Named(named),
        code,
        None,
    )
}

fn digit(folio: &mut HeadlessFolio, text: &str, code: KeyCode) {
    picture_key(
        folio,
        winit::keyboard::ModifiersState::empty(),
        Key::Character(text.into()),
        code,
        Some(text),
    )
}

fn close_to(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

/// RED (T-PICTURE-KEYS-PAN-AND-ZOOM, row 44) — **the arrows pan a picture zoomed past its box**:
/// one press is an eighth of the box on that axis and a `Shift` press half of it, the view moves
/// the way the arrow points, and at each of the four edges the picture stops there and the key
/// is still the picture's — it does not travel to the next file.
///
/// MUTATION: drop the pan arm from `Runtime::preview_browse_key` — `→` goes to file travel and
/// the pan stays at nothing, and the first assertion goes red.
#[test]
fn the_arrows_pan_a_picture_zoomed_past_its_box_and_stop_at_each_edge() {
    let (mut folio, surface) = a_large_picture_beside_a_shell("picture-pan", "平移 pan", true);
    let file = picture_path(&mut folio, surface);
    let body = picture_body(&mut folio, surface);
    let [width, height] = [body[2] - body[0], body[3] - body[1]];
    assert!(
        folio
            .runtime()
            .set_preview_image_zoom(surface, crate::ImageZoom::scaled(1.0))
            .expect("the zoom lands"),
        "the fixture's own precondition: 100 %"
    );
    let drawn = crate::image_destination(body, LARGE, zoom_of(&mut folio, surface));
    assert!(
        drawn[2] - drawn[0] > width && drawn[3] - drawn[1] > height,
        "the fixture's own precondition: at 100 % the picture overflows its box on both axes"
    );

    arrow(&mut folio, NamedKey::ArrowRight, KeyCode::ArrowRight);
    let pan = zoom_of(&mut folio, surface).pan;
    assert!(
        close_to(pan[0], -width / 8.0) && close_to(pan[1], 0.0),
        "`→` shows an eighth of the box more of the right side: {pan:?}"
    );
    picture_key(
        &mut folio,
        winit::keyboard::ModifiersState::SHIFT,
        Key::Named(NamedKey::ArrowDown),
        KeyCode::ArrowDown,
        None,
    );
    let pan = zoom_of(&mut folio, surface).pan;
    assert!(
        close_to(pan[0], -width / 8.0) && close_to(pan[1], -height / 2.0),
        "`Shift+↓` shows half the box more of the bottom: {pan:?}"
    );

    // Each edge in turn: far more presses than the overflow needs.
    let edges = [
        (NamedKey::ArrowLeft, KeyCode::ArrowLeft, 0_usize, "left"),
        (NamedKey::ArrowUp, KeyCode::ArrowUp, 1, "top"),
        (NamedKey::ArrowRight, KeyCode::ArrowRight, 2, "right"),
        (NamedKey::ArrowDown, KeyCode::ArrowDown, 3, "bottom"),
    ];
    for (named, code, side, name) in edges {
        for _ in 0..40 {
            arrow(&mut folio, named, code);
        }
        let held = zoom_of(&mut folio, surface);
        let drawn = crate::image_destination(body, LARGE, held);
        assert!(
            close_to(drawn[side], body[side]),
            "carried to its {name} edge, the picture's edge rests on the body's: {drawn:?} in {body:?}"
        );
        arrow(&mut folio, named, code);
        assert_eq!(
            zoom_of(&mut folio, surface),
            held,
            "one more press at the {name} edge moves nothing"
        );
        assert_eq!(
            picture_path(&mut folio, surface),
            file,
            "and travels to no other file"
        );
    }
}

/// GUARD (T-PICTURE-KEYS-PAN-AND-ZOOM) — **a picture that fits keeps the arrows for the folder**:
/// at Fit, `→` opens the next picture in the folder, as it did before the arrows could pan.
///
/// MUTATION: make the pan arm in `Runtime::preview_browse_key` take an arrow whatever the
/// picture's size (drop the overflow test in `image_pan_key`) — `→` is spent on a pan that
/// cannot move and the folder is never travelled.
#[test]
fn a_picture_that_fits_keeps_the_arrows_for_the_folder() {
    let (mut folio, surface) =
        a_large_picture_beside_a_shell("picture-fits-travels", "合适 fits", true);
    let file = picture_path(&mut folio, surface);
    let folder = file.parent().expect("a scratch folder").to_path_buf();
    let name = |path: &Path| -> String {
        path.file_name()
            .expect("a file name")
            .to_string_lossy()
            .into_owned()
    };
    let next = folder.join("zz 下一张 next.png");
    let listing = crate::files::DirListing {
        entries: [file.as_path(), next.as_path()]
            .into_iter()
            .map(|path| crate::files::DirEntry {
                name: name(path),
                is_dir: false,
                is_symlink: false,
            })
            .collect(),
        ..crate::files::DirListing::default()
    };
    folio.window.tabs[0]
        .preview_panes
        .entry(surface)
        .hold_neighbours(crate::preview_neighbours::Folder {
            path: folder,
            listing: Some(listing),
            asked_by: surface,
        });
    assert!(zoom_of(&mut folio, surface).is_fit());

    arrow(&mut folio, NamedKey::ArrowRight, KeyCode::ArrowRight);
    assert_eq!(
        picture_path(&mut folio, surface),
        next,
        "at Fit, `→` is the next file"
    );
}

/// PIN (T-PICTURE-KEYS-PAN-AND-ZOOM; the keys are ticket #60's) — **`1` and `0` land exactly where
/// the double click lands**: `1` from Fit is the double click's 100 %, centred, and `0` from a
/// panned 300 % is the double click's Fit.
///
/// MUTATION: drop the `"1"` arm (or the `"0"` arm) from `image_zoom_key` — the digit falls
/// through to the swallow and the zoom stays where it was.
#[test]
fn the_digit_stops_land_where_the_double_click_lands() {
    let (mut folio, surface) = a_large_picture_beside_a_shell("picture-stops", "两站 stops", true);
    let body = picture_body(&mut folio, surface);
    let centre = PhysicalPosition::new(
        f64::from((body[0] + body[2]) / 2.0),
        f64::from((body[1] + body[3]) / 2.0),
    );
    let double_click = |folio: &mut HeadlessFolio| {
        click(folio, centre);
        click(folio, centre);
    };
    let panned = crate::ImageZoom {
        pan: [90.0, -40.0],
        ..crate::ImageZoom::scaled(3.0)
    };
    let set = |folio: &mut HeadlessFolio, zoom: crate::ImageZoom| {
        folio
            .runtime()
            .set_preview_image_zoom(surface, zoom)
            .expect("the zoom lands");
    };

    double_click(&mut folio);
    let clicked_from_fit = zoom_of(&mut folio, surface);
    assert_eq!(
        clicked_from_fit,
        crate::ImageZoom::scaled(1.0),
        "the fixture's own precondition: the double click from Fit is 100 %, centred"
    );
    set(&mut folio, crate::ImageZoom::FIT);
    digit(&mut folio, "1", KeyCode::Digit1);
    assert_eq!(
        zoom_of(&mut folio, surface),
        clicked_from_fit,
        "`1` is that stop"
    );

    set(&mut folio, panned);
    double_click(&mut folio);
    let clicked_from_zoomed = zoom_of(&mut folio, surface);
    assert_eq!(
        clicked_from_zoomed,
        crate::ImageZoom::FIT,
        "the fixture's own precondition: the double click from 300 % is Fit"
    );
    set(&mut folio, panned);
    digit(&mut folio, "0", KeyCode::Digit0);
    assert_eq!(
        zoom_of(&mut folio, surface),
        clicked_from_zoomed,
        "`0` is that stop"
    );
}

/// RED (T-PICTURE-KEYS-PAN-AND-ZOOM) — **a floating window's picture answers the same keys**: torn
/// off, pinned and given the keyboard by a click as a hand gives it, a zoomed picture pans one step
/// at `→`, and `0` and `1` land where the window's own double click lands.
///
/// MUTATION: drop the pan arm from `Runtime::preview_browse_key` — the float's `→` pans nothing.
#[test]
fn a_floating_windows_picture_pans_and_stops_on_the_same_keys() {
    let (mut folio, surface) = a_large_picture_beside_a_shell("picture-float", "悬浮 float", true);
    let PreviewSurface::Seat(leaf) = surface else {
        unreachable!("a docked pane")
    };
    folio
        .runtime()
        .pop_out_preview(leaf.seat)
        .expect("the picture is torn off");
    folio.draw();
    let id = folio
        .window
        .float
        .live_windows()
        .next()
        .expect("a window holds the picture")
        .id();
    let float = PreviewSurface::Float(id);
    assert!(
        folio.window.float.is_pinned(id),
        "the fixture's own precondition: the torn-off window is pinned"
    );
    // Whatever the tear-off left, the keyboard starts elsewhere, so the click below is what
    // hands it over.
    folio.window.float.blur();
    folio.draw();
    assert_ne!(
        folio.runtime().preview_keyboard_surface(),
        Some(float),
        "the fixture's own precondition: the window does not hold the keyboard yet"
    );
    let body = picture_body(&mut folio, float);
    let centre = PhysicalPosition::new(
        f64::from((body[0] + body[2]) / 2.0),
        f64::from((body[1] + body[3]) / 2.0),
    );
    // A click near the body's corner, far past the double click's slop from the centre, so it
    // is not half of the pair below.
    click(
        &mut folio,
        PhysicalPosition::new(f64::from(body[0] + 24.0), f64::from(body[1] + 24.0)),
    );
    assert_eq!(
        folio.runtime().preview_keyboard_surface(),
        Some(float),
        "the fixture's own precondition: the click gave the window the keyboard"
    );

    click(&mut folio, centre);
    click(&mut folio, centre);
    let clicked = zoom_of(&mut folio, float);
    assert_eq!(
        clicked,
        crate::ImageZoom::scaled(1.0),
        "the fixture's own precondition: the window's double click is 100 %"
    );
    digit(&mut folio, "0", KeyCode::Digit0);
    assert!(zoom_of(&mut folio, float).is_fit(), "`0` is Fit");
    digit(&mut folio, "1", KeyCode::Digit1);
    assert_eq!(
        zoom_of(&mut folio, float),
        clicked,
        "`1` is the double click's 100 %"
    );

    arrow(&mut folio, NamedKey::ArrowRight, KeyCode::ArrowRight);
    let pan = zoom_of(&mut folio, float).pan;
    assert!(
        close_to(pan[0], -(body[2] - body[0]) / 8.0) && close_to(pan[1], 0.0),
        "`→` pans the window's picture one step: {pan:?}"
    );
}

/// CONTROL (T-PICTURE-KEYS-PAN-AND-ZOOM) — **a digit or an arrow typed while the shell holds the
/// keyboard is the shell's**: through the whole ladder, the picture in the preview beside it does
/// not move.
#[test]
fn a_digit_typed_into_the_shell_never_reaches_the_picture_beside_it() {
    let (mut folio, surface) =
        a_large_picture_beside_a_shell("picture-shell-keys", "旁边 beside", false);
    folio
        .runtime()
        .set_preview_image_zoom(surface, crate::ImageZoom::scaled(1.0))
        .expect("the zoom lands");
    let held = zoom_of(&mut folio, surface);
    assert_eq!(
        folio.runtime().preview_keyboard_surface(),
        None,
        "the fixture's own precondition: the shell holds the keyboard"
    );
    digit(&mut folio, "0", KeyCode::Digit0);
    arrow(&mut folio, NamedKey::ArrowRight, KeyCode::ArrowRight);
    assert_eq!(
        zoom_of(&mut folio, surface),
        held,
        "and the picture stayed as it was"
    );
}

// ── T-PREVIEW-NEIGHBOUR-BUTTON-HOVER: a picture's `<` / `>` answer the hand ────────────────────

/// A focused docked picture whose folder holds `before` pictures ahead of it and `after` behind
/// it, so its side buttons are on the glass. The neighbours are names only: a step opens them as
/// the arrow road does, and nothing here reads them from the disk.
fn a_picture_with_neighbours(
    tag: &str,
    before: usize,
    after: usize,
) -> (HeadlessFolio, PreviewSurface, Vec<std::path::PathBuf>) {
    let (mut folio, surface) = a_large_picture_beside_a_shell(tag, "邻居 neighbours", true);
    let file = picture_path(&mut folio, surface);
    let folder = file.parent().expect("a scratch folder").to_path_buf();
    let ahead = (0..before).map(|index| folder.join(format!("{index} 前 before.png")));
    let behind = (0..after).map(|index| folder.join(format!("zz{index} 后 after.png")));
    let paths: Vec<std::path::PathBuf> = ahead.chain(std::iter::once(file)).chain(behind).collect();
    let listing = crate::files::DirListing {
        entries: paths
            .iter()
            .map(|path| crate::files::DirEntry {
                name: path
                    .file_name()
                    .expect("a file name")
                    .to_string_lossy()
                    .into_owned(),
                is_dir: false,
                is_symlink: false,
            })
            .collect(),
        ..crate::files::DirListing::default()
    };
    folio.window.tabs[0]
        .preview_panes
        .entry(surface)
        .hold_neighbours(crate::preview_neighbours::Folder {
            path: folder,
            listing: Some(listing),
            asked_by: surface,
        });
    folio.draw();
    (folio, surface, paths)
}

fn neighbour_button(
    folio: &mut HeadlessFolio,
    surface: PreviewSurface,
    direction: crate::preview_neighbours::Direction,
) -> [f32; 4] {
    let index = match direction {
        crate::preview_neighbours::Direction::Previous => 0,
        crate::preview_neighbours::Direction::Next => 1,
    };
    folio.runtime().preview_neighbour_buttons(surface)[index]
        .expect("the fixture's own precondition: the button is on the glass")
}

fn centre_of(rect: [f32; 4]) -> PhysicalPosition<f64> {
    PhysicalPosition::new(
        f64::from((rect[0] + rect[2]) / 2.0),
        f64::from((rect[1] + rect[3]) / 2.0),
    )
}

/// The inks the two buttons are drawn in this frame, `<` first: `(pill, chevron)` each, or
/// `None` for a button that is not on the glass.
fn neighbour_inks(
    folio: &mut HeadlessFolio,
    surface: PreviewSurface,
) -> [Option<([u8; 3], [u8; 3])>; 2] {
    let buttons = folio.runtime().preview_neighbour_buttons(surface);
    let sprites = folio
        .runtime()
        .preview_neighbour_layer(surface)
        .map(|layer| layer.sprites)
        .unwrap_or_default();
    let mut drawn = sprites.chunks(2).map(|pair| (pair[0].color, pair[1].color));
    buttons.map(|button| button.and_then(|_| drawn.next()))
}

/// A button at rest: the pill in the head's own ground and the chevron in the bar's ink.
fn plain_neighbour() -> ([u8; 3], [u8; 3]) {
    let palette = bt_render::chrome_palette();
    (palette.pane_head, palette.title_text)
}

/// A button under the hand: `.pv-nav:hover`'s wash and ink.
fn lit_neighbour() -> ([u8; 3], [u8; 3]) {
    let palette = bt_render::chrome_palette();
    (palette.preview_nav_pill, palette.preview_nav_glyph_on_pill)
}

/// How many frames the window was asked for since the last take.
fn redraws(folio: &HeadlessFolio) -> usize {
    folio
        .ledger
        .take()
        .into_iter()
        .filter(|effect| *effect == WindowEffect::Redraw)
        .count()
}

/// RED (T-PREVIEW-NEIGHBOUR-BUTTON-HOVER, owner report 2026-10-27) — **the side button under the
/// hand lights**: the pill takes the mock-up's `--hover` wash and the chevron `--ink`
/// (`.pv-nav:hover`), and only the button under the hand does.
///
/// MUTATION: drop the write in `Runtime::note_preview_neighbour_hover` — the button keeps its
/// resting inks under the hand and the first assertion goes red.
#[test]
fn a_picture_neighbour_button_lights_under_the_hand() {
    use crate::preview_neighbours::Direction::{Next, Previous};
    let (mut folio, surface, _) = a_picture_with_neighbours("neighbour-lights", 1, 1);
    let previous = neighbour_button(&mut folio, surface, Previous);
    folio.move_to(centre_of(previous));
    assert_eq!(
        neighbour_inks(&mut folio, surface),
        [Some(lit_neighbour()), Some(plain_neighbour())],
        "the `<` under the hand lights and the `>` stays at rest"
    );
    let next = neighbour_button(&mut folio, surface, Next);
    folio.move_to(centre_of(next));
    assert_eq!(
        neighbour_inks(&mut folio, surface),
        [Some(plain_neighbour()), Some(lit_neighbour())],
        "and the hand crossing to the `>` takes the light with it"
    );
}

/// RED (T-PREVIEW-NEIGHBOUR-BUTTON-HOVER) — **a floating window's picture lights its buttons
/// too**: the float under the hand is the button's own, so it is no cover for it.
///
/// MUTATION: gate the note on `free` alone in `Runtime::pointer_moved` — a float is never free,
/// and its `<` stays at rest under the hand.
#[test]
fn a_floating_pictures_neighbour_button_lights_under_the_hand() {
    use crate::preview_neighbours::Direction::Previous;
    let (mut folio, surface, _) = a_picture_with_neighbours("neighbour-float", 1, 1);
    let PreviewSurface::Seat(leaf) = surface else {
        unreachable!("a docked pane")
    };
    folio
        .runtime()
        .pop_out_preview(leaf.seat)
        .expect("the picture is torn off");
    folio.draw();
    let id = folio
        .window
        .float
        .live_windows()
        .next()
        .expect("a window holds the picture")
        .id();
    let float = PreviewSurface::Float(id);
    let body = picture_body(&mut folio, float);
    // Onto the window's body first: a float shows its buttons while its body is hovered.
    folio.move_to(PhysicalPosition::new(
        f64::from((body[0] + body[2]) / 2.0),
        f64::from((body[1] + body[3]) / 2.0),
    ));
    folio.draw();
    let previous = neighbour_button(&mut folio, float, Previous);
    folio.move_to(centre_of(previous));
    assert_eq!(
        neighbour_inks(&mut folio, float)[0],
        Some(lit_neighbour()),
        "the float's `<` under the hand lights"
    );
}

/// RED (T-PREVIEW-NEIGHBOUR-BUTTON-HOVER) — **the light goes out with the hand, and each change
/// asks for exactly one frame**: a move over the body that changes nothing asks for none.
///
/// MUTATION: drop the `over == self.preview_neighbour_hover` return in
/// `Runtime::note_preview_neighbour_hover` — a move along the lit button asks a frame of its own
/// and the count after it goes red.
#[test]
fn the_neighbour_hover_goes_out_and_asks_one_frame_per_change() {
    use crate::preview_neighbours::Direction::Previous;
    let (mut folio, surface, _) = a_picture_with_neighbours("neighbour-frames", 1, 1);
    let body = picture_body(&mut folio, surface);
    let previous = neighbour_button(&mut folio, surface, Previous);
    let middle = |dy: f32| {
        PhysicalPosition::new(
            f64::from((body[0] + body[2]) / 2.0),
            f64::from((body[1] + body[3]) / 2.0 + dy),
        )
    };
    folio.move_to(middle(0.0));
    folio.ledger.take();
    folio.move_to(middle(12.0));
    assert_eq!(
        redraws(&folio),
        0,
        "a move over the body that lights nothing"
    );
    folio.move_to(centre_of(previous));
    assert_eq!(redraws(&folio), 1, "onto the `<`: one frame");
    assert_eq!(
        neighbour_inks(&mut folio, surface)[0],
        Some(lit_neighbour())
    );
    let on = centre_of(previous);
    folio.move_to(PhysicalPosition::new(on.x + 2.0, on.y + 2.0));
    assert_eq!(
        redraws(&folio),
        0,
        "a move along the lit button changes nothing"
    );
    folio.move_to(middle(12.0));
    assert_eq!(
        redraws(&folio),
        1,
        "off the `<`, still in the pane: one frame"
    );
    assert_eq!(
        neighbour_inks(&mut folio, surface)[0],
        Some(plain_neighbour()),
        "and it is at rest again"
    );
}

/// PIN (T-PREVIEW-NEIGHBOUR-BUTTON-HOVER) — **a hand over the pane but off the buttons lights
/// neither**: the buttons are on the glass because the pane is hovered, and both stand at rest.
///
/// MUTATION: have `Runtime::pointer_moved` note the surface's `Previous` whenever
/// `preview_neighbour_buttons` has one — the `<` lights with the hand nowhere near it.
#[test]
fn over_the_pane_but_not_the_button_is_plain() {
    let (mut folio, surface, _) = a_picture_with_neighbours("neighbour-plain", 1, 1);
    let body = picture_body(&mut folio, surface);
    folio.move_to(PhysicalPosition::new(
        f64::from((body[0] + body[2]) / 2.0),
        f64::from(body[1] + 6.0),
    ));
    assert_eq!(
        neighbour_inks(&mut folio, surface),
        [Some(plain_neighbour()), Some(plain_neighbour())],
        "both buttons at rest"
    );
}

/// RED (T-PREVIEW-NEIGHBOUR-BUTTON-HOVER) — **the hover ends with the document it was on, and
/// with the hand leaving the window**:
/// - a step by the arrow key lets go of it (the button relights at the next move, ruling D5);
/// - a click on the button steps, and the button the hand is still on answers it again;
/// - a pointer leaving the window puts it out, though a focused pane keeps its buttons.
///
/// MUTATION: drop the neighbour arm from `Runtime::let_go_of_preview_hovers_on` — the `>` stays
/// lit on the next picture after the key step and the second assertion goes red.
#[test]
fn the_neighbour_hover_ends_with_the_document() {
    use crate::preview_neighbours::Direction::{Next, Previous};
    let (mut folio, surface, paths) = a_picture_with_neighbours("neighbour-let-go", 1, 3);
    let next = neighbour_button(&mut folio, surface, Next);
    folio.move_to(centre_of(next));
    assert_eq!(
        neighbour_inks(&mut folio, surface)[1],
        Some(lit_neighbour())
    );
    arrow(&mut folio, NamedKey::ArrowRight, KeyCode::ArrowRight);
    assert_eq!(
        picture_path(&mut folio, surface),
        paths[2],
        "the key stepped"
    );
    assert_eq!(
        neighbour_inks(&mut folio, surface)[1],
        Some(plain_neighbour()),
        "the key's step let go of the hover: the `>` stands at rest until the hand moves"
    );

    click(&mut folio, centre_of(next));
    assert_eq!(
        picture_path(&mut folio, surface),
        paths[3],
        "the click stepped"
    );
    assert_eq!(
        neighbour_inks(&mut folio, surface)[1],
        Some(lit_neighbour()),
        "the hand is still on the `>`, so the next picture's `>` answers it"
    );

    let previous = neighbour_button(&mut folio, surface, Previous);
    folio.move_to(centre_of(previous));
    assert_eq!(
        neighbour_inks(&mut folio, surface)[0],
        Some(lit_neighbour())
    );
    folio.runtime().pointer_left().expect("the hand leaves");
    assert_eq!(
        neighbour_inks(&mut folio, surface)[0],
        Some(plain_neighbour()),
        "the focused pane keeps its `<`, at rest"
    );
}

/// RED (T-PREVIEW-NEIGHBOUR-BUTTON-HOVER, ruling D3) — **the finger beats the open hand over a
/// side button**: on a picture zoomed past its body the body wears the open hand, but a press on
/// the button steps the folder, so the button wears the finger.
///
/// MUTATION: hand `pointer_cursor` the unfiltered `image_grasp()` in
/// `Runtime::apply_pointer_cursor` — the open hand wins over the button.
#[test]
fn the_finger_wins_over_the_grab_on_a_neighbour_button() {
    use crate::preview_neighbours::Direction::Previous;
    use winit::window::CursorIcon;
    let (mut folio, surface, _) = a_picture_with_neighbours("neighbour-finger", 1, 1);
    folio
        .runtime()
        .set_preview_image_zoom(surface, crate::ImageZoom::scaled(3.0))
        .expect("the zoom lands");
    folio.draw();
    let last_cursor = |folio: &HeadlessFolio| {
        folio
            .ledger
            .take()
            .into_iter()
            .filter_map(|effect| match effect {
                WindowEffect::Cursor(cursor) => Some(cursor),
                _ => None,
            })
            .last()
    };
    let body = picture_body(&mut folio, surface);
    folio.move_to(PhysicalPosition::new(
        f64::from((body[0] + body[2]) / 2.0),
        f64::from((body[1] + body[3]) / 2.0),
    ));
    folio.runtime().apply_pointer_cursor();
    assert_eq!(
        last_cursor(&folio),
        Some(CursorIcon::Grab.into()),
        "the fixture's own precondition: the zoomed picture wears the open hand"
    );
    let previous = neighbour_button(&mut folio, surface, Previous);
    folio.move_to(centre_of(previous));
    assert_eq!(
        last_cursor(&folio),
        Some(CursorIcon::Pointer.into()),
        "over the `<` the finger says what a press will do"
    );
}
