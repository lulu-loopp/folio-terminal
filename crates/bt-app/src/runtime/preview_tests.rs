//! **The rendered page's caret, driven** (T-HEADLESS-RUNTIME-SEAM): the press, the release, the
//! keys and the blur that put a caret in a Markdown page and take it out again, run on a headless
//! window through the window's own methods.

use super::*;
use crate::headless::{HeadlessFolio, key_down};
use crate::preview_press::Spend;
use crate::preview_select::{Grain, Selection};
use crate::test_support::{buffer_saying, host_spelling, seat_of, tab_with_a_preview};
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
        .cancel_preview_text_drag()
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
/// `runtime.cancel_preview_text_drag()` from the blur in `FolioApp::on_window_event` — the
/// gesture outlives the window's focus and the span with it.
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
/// then the body's repaint and a frame — because a typed key reveals its caret, and the reveal's
/// own passes would put a missing span back before anything could look (one wasted re-flow per
/// keystroke, which is the defect).
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
        .cancel_preview_text_drag()
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
        .cancel_preview_text_drag()
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
