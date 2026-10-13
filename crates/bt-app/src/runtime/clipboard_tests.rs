//! **The clipboard's roads, driven** (T-HEADLESS-RUNTIME-SEAM): the keystroke's copy and paste
//! and the Edit menu's, run on a headless window whose clipboard is the window seam's own.

use crate::headless::{HeadlessFolio, WindowEffect, key_down};
use crate::preview_press::Spend;
use crate::preview_select::Grain;
use crate::test_support::tab_with_a_preview;
use crate::test_support::{buffer_saying, host_spelling, leaf_saying, seat_of, tab_holding};
use crate::{PreviewSurface, TabId};
use bt_platform::menu::AppMenuAction;
use winit::keyboard::{Key, KeyCode, ModifiersState};

/// This host's command modifier: Control, or Command on a Mac — `input::is_command_chord`'s
/// answer, not a second decision.
fn command() -> ModifiersState {
    [ModifiersState::CONTROL, ModifiersState::SUPER]
        .into_iter()
        .find(|modifiers| crate::input::is_command_chord(*modifiers))
        .expect("one of the two is the host's command modifier")
}

/// The command chord on `letter`, pressed in `folio`'s window.
fn chord(folio: &mut HeadlessFolio, letter: &str, code: KeyCode) {
    let mut runtime = folio.runtime();
    runtime.window.modifiers = command();
    runtime
        .keyboard_input(
            &key_down(Key::Character(letter.into()), code, Some(letter)),
            false,
        )
        .expect("the keystroke is answered");
    runtime.window.modifiers = ModifiersState::empty();
}

/// What the window put on the clipboard since the last time this was asked.
fn written(folio: &HeadlessFolio) -> Vec<String> {
    folio
        .ledger
        .take()
        .into_iter()
        .filter_map(|effect| match effect {
            WindowEffect::ClipboardWritten(text) => Some(text),
            _ => None,
        })
        .collect()
}

/// A Markdown page being typed into, its caret selecting `第二行 second`.
fn page_being_edited(tag: &str) -> (HeadlessFolio, PreviewSurface, &'static str) {
    const PAGE: &str = "第一行 first\n\n第二行 second\n";
    let path = host_spelling(r"C:\w\笔记.md");
    let (tab, seat) = tab_with_a_preview(1, vec![buffer_saying(&path, "笔记.md", PAGE)]);
    let mut folio = HeadlessFolio::with_tabs(tag, vec![tab], 0);
    let surface = seat_of(TabId(1), seat);
    folio.draw();
    let start = PAGE.find("第二行").unwrap();
    folio
        .runtime()
        .spend_preview_press(
            surface,
            Spend::Caret {
                offset: start,
                grain: Grain::Character,
                extend: false,
                head: Some(start + "第二行 second".len()),
            },
        )
        .expect("the caret selects the line");
    folio.ledger.take();
    (folio, surface, PAGE)
}

/// RED — **the Edit menu's clipboard verbs end at the keystroke's own doors** (M3-1): Edit ▸ Copy
/// puts on the clipboard what `Ctrl+C` (`⌘C`) puts there, from the page being typed into and
/// from the terminal's selection, and Edit ▸ Paste lands in the page what the chord lands —
/// with the file's own line breaks, which only the editor's paste door writes.
///
/// MUTATION: make the menu's `paste_into_document` door insert the clipboard's text itself
/// (`self.insert_into_preview(&text)` over `self.window.window.clipboard_text()`) instead of
/// `self.paste_into_preview()` — the menu's paste keeps the clipboard's `\r\n`.
#[test]
fn the_edit_menus_clipboard_verbs_land_where_the_keystrokes_do() {
    // Copy, from the page being typed into.
    let (mut folio, _, _) = page_being_edited("headless-menu-copy");
    chord(&mut folio, "c", KeyCode::KeyC);
    let by_key = written(&folio);
    folio
        .runtime()
        .run_an_application_menu_verb(AppMenuAction::CopySelection)
        .expect("the menu's copy is answered");
    let by_menu = written(&folio);
    assert_eq!(
        by_key,
        ["第二行 second".to_owned()],
        "the chord copies the caret's bytes"
    );
    assert_eq!(
        by_menu, by_key,
        "and the menu copies exactly what the chord copies"
    );

    // Paste, into the page: the clipboard's breaks become the file's.
    let pasted = |through_the_menu: bool| {
        let (mut folio, surface, _) = page_being_edited("headless-menu-paste");
        *folio.window.window.clipboard.borrow_mut() = Some("甲\r\n乙".to_owned());
        if through_the_menu {
            folio
                .runtime()
                .run_an_application_menu_verb(AppMenuAction::PasteIntoFocus)
                .expect("the menu's paste is answered");
        } else {
            chord(&mut folio, "v", KeyCode::KeyV);
        }
        folio
            .runtime()
            .preview_buffer_on(surface)
            .and_then(|buffer| buffer.text())
            .map(str::to_owned)
            .expect("the page's text")
    };
    let by_key = pasted(false);
    assert!(
        by_key.contains("甲\n乙"),
        "the chord pastes in the file's breaks: {by_key:?}"
    );
    assert_eq!(pasted(true), by_key, "and the menu's paste is the chord's");

    // Copy, from the terminal's selection.
    let leaf = leaf_saying("复制 retry me");
    let mut folio = HeadlessFolio::with_tabs("headless-menu-term", vec![tab_holding(leaf)], 0);
    let select = |folio: &mut HeadlessFolio| {
        let mut runtime = folio.runtime();
        let leaf = runtime.focused_mut().expect("the shell");
        let frame = leaf
            .session
            .viewport_frame(&mut leaf.projection)
            .expect("a frame");
        let selection = bt_viewport::ViewSelection {
            start: frame
                .anchor_at(0, 0, bt_doc::Bias::Before)
                .unwrap()
                .unwrap(),
            end: frame.anchor_at(0, 6, bt_doc::Bias::After).unwrap().unwrap(),
        };
        leaf.session.set_view_selection(Some(selection.clone()));
        leaf.projection.set_selection(Some(selection));
    };
    select(&mut folio);
    chord(&mut folio, "c", KeyCode::KeyC);
    let by_key = written(&folio);
    select(&mut folio);
    folio
        .runtime()
        .run_an_application_menu_verb(AppMenuAction::CopySelection)
        .expect("the menu's copy is answered");
    assert_eq!(
        by_key.len(),
        1,
        "the chord copies the terminal's selection: {by_key:?}"
    );
    assert_eq!(
        written(&folio),
        by_key,
        "and the menu copies what the chord copies"
    );
}

/// RED (B-AUDIT-046 RT-1) — **a paste goes to the text field that holds the keyboard, by the
/// chord and by Edit ▸ Paste, and never to the shell behind it**: with the find bar open over a
/// shell, both put the clipboard's one line into the find bar and nothing into the shell.
///
/// MUTATION: make `menubar::paste_seat` answer `clipboard_seat(focus)` whatever the field — the
/// chord and the menu both hand the paste to the terminal and the find bar stays empty.
#[test]
fn paste_goes_to_the_focused_field_and_never_to_the_shell_behind_it_by_either_road() {
    for through_the_menu in [false, true] {
        let leaf = leaf_saying("SHELL 外壳");
        let mut folio =
            HeadlessFolio::with_tabs("headless-field-paste", vec![tab_holding(leaf)], 0);
        let seat = folio.runtime().seats.identity();
        folio
            .runtime()
            .open_search(seat)
            .expect("the find bar opens");
        assert!(folio.runtime().window.search.is_focused());
        *folio.window.window.clipboard.borrow_mut() = Some("查找 needle\r\nsecond".to_owned());
        if through_the_menu {
            folio
                .runtime()
                .run_an_application_menu_verb(AppMenuAction::PasteIntoFocus)
                .expect("the menu's paste is answered");
        } else {
            chord(&mut folio, "v", KeyCode::KeyV);
        }
        let mut runtime = folio.runtime();
        assert_eq!(
            runtime.window.search.query(),
            crate::text_field::one_line("查找 needle\r\nsecond"),
            "the find bar takes the paste (through the menu: {through_the_menu})"
        );
        let shell = runtime.focused_mut().expect("the shell");
        assert!(
            shell.pending_paste.is_none() && shell.pending_typing.is_none(),
            "and nothing reached the shell behind it (through the menu: {through_the_menu})"
        );
    }
}
