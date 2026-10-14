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

/// How a paste is asked for.
#[derive(Clone, Copy, Debug)]
enum PasteRoad {
    /// The command chord on `V`.
    Chord,
    /// `Shift+Insert`, the older paste key — a paste on every host but a Mac, which has no
    /// `Insert` key's clipboard pair.
    ShiftInsert,
    /// Edit ▸ Paste.
    Menu,
}

/// The roads a paste is asked by on this host: the chord and the menu everywhere, and
/// `Shift+Insert` off a Mac. The host decides it here, not the window's own predicate
/// (`input::is_paste_shortcut_on`): a predicate that forgot the key would otherwise take its road
/// out of the test with it.
fn paste_roads() -> Vec<PasteRoad> {
    let shift_insert = bt_platform::host_platform() != bt_platform::HostPlatform::MacOs;
    [PasteRoad::Chord, PasteRoad::ShiftInsert, PasteRoad::Menu]
        .into_iter()
        .filter(|road| shift_insert || !matches!(road, PasteRoad::ShiftInsert))
        .collect()
}

/// One text field a paste can land in, opened in a fresh window: what the clipboard holds is
/// pasted by `road`, and the field's text is read back with whether the shell behind it (when
/// there is one) was handed anything.
fn paste_into(field: crate::TextFieldSeat, road: PasteRoad) -> (String, bool) {
    use crate::TextFieldSeat;
    const PASTED: &str = "粘贴 pasted\r\nsecond";
    let preview = field == TextFieldSeat::GraphSearch;
    // The repository the graph page is about: a name only, which nothing reads.
    let root = bt_testpath::temp_path("headless-graph-paste 图");
    let (tab, preview_seat) = if preview {
        // A commit graph's page: the search field lives in the graph's view, and a view is kept
        // only on a surface that shows a graph.
        let graph = crate::preview::PreviewBuffer::new(
            crate::preview::PreviewSource::GitGraph { root: root.clone() },
            "图 graph".to_owned(),
        );
        let (tab, seat) = tab_with_a_preview(1, vec![graph]);
        (tab, Some(seat))
    } else {
        (tab_holding(leaf_saying("SHELL 外壳")), None)
    };
    let mut folio = HeadlessFolio::with_tabs("headless-field-paste", vec![tab], 0);
    folio.draw();
    {
        let mut runtime = folio.runtime();
        let tab = runtime.window.tabs[0].id;
        let seat = runtime.seats.identity();
        match field {
            TextFieldSeat::FindBar => runtime.open_search(seat).expect("the find bar opens"),
            TextFieldSeat::Palette => runtime.open_command_palette().expect("the palette opens"),
            TextFieldSeat::TabName => {
                runtime.window.rename = Some(crate::TabRename::open(tab, None));
            }
            TextFieldSeat::GitPrompt => {
                runtime.window.git_menu = Some(crate::GitMenuState {
                    point: [10.0, 10.0],
                    origin: crate::GitOrigin::Graph(host_spelling(r"C:\w").into()),
                    root: host_spelling(r"C:\w").into(),
                    target: crate::profiles::GitMenuTarget::LocalBranch {
                        name: "主干 main".to_owned(),
                        current: true,
                    },
                    graph_row: None,
                    hover: None,
                    prompt: Some(crate::GitPromptState {
                        kind: crate::profiles::GitPromptKind::CreateBranch,
                        caption: String::new(),
                        field: crate::text_field::TextField::default(),
                        asked_empty: false,
                    }),
                });
            }
            TextFieldSeat::GraphSearch => {
                let seat = preview_seat.expect("the graph's seat");
                runtime.seats.set_focus(seat);
                runtime.window.tabs[0].git_graph_view.insert(
                    seat_of(tab, seat),
                    crate::GraphView {
                        root: root.clone(),
                        search_focused: true,
                        ..crate::GraphView::default()
                    },
                );
                assert_eq!(
                    crate::ime_owner(runtime.keyboard_owner()),
                    crate::ImeOwner::GraphSearch,
                    "the fixture's own precondition: the graph's search holds the keyboard"
                );
            }
            TextFieldSeat::Settings => runtime
                .open_settings_on_row(crate::settings::SettingsRow::QuakeCommand)
                .expect("the dialog opens on the field"),
        }
    }
    *folio.window.window.clipboard.borrow_mut() = Some(PASTED.to_owned());
    match road {
        PasteRoad::Menu => folio
            .runtime()
            .run_an_application_menu_verb(AppMenuAction::PasteIntoFocus)
            .expect("the menu's paste is answered"),
        PasteRoad::Chord => chord(&mut folio, "v", KeyCode::KeyV),
        PasteRoad::ShiftInsert => {
            let mut runtime = folio.runtime();
            runtime.window.modifiers = ModifiersState::SHIFT;
            runtime
                .keyboard_input(
                    &key_down(
                        Key::Named(winit::keyboard::NamedKey::Insert),
                        KeyCode::Insert,
                        None,
                    ),
                    false,
                )
                .expect("the keystroke is answered");
            runtime.window.modifiers = ModifiersState::empty();
        }
    }
    let mut runtime = folio.runtime();
    let text = match field {
        TextFieldSeat::FindBar => runtime.window.search.query().to_string(),
        TextFieldSeat::Palette => runtime
            .window
            .palette
            .as_ref()
            .map(|palette| palette.query().into_owned())
            .unwrap_or_default(),
        TextFieldSeat::TabName => runtime
            .window
            .rename
            .as_ref()
            .map(|rename| rename.field.text().to_owned())
            .unwrap_or_default(),
        TextFieldSeat::GitPrompt => runtime
            .window
            .git_menu
            .as_ref()
            .and_then(|menu| menu.prompt.as_ref())
            .map(|prompt| prompt.field.text().to_owned())
            .unwrap_or_default(),
        TextFieldSeat::GraphSearch => runtime.window.tabs[0]
            .git_graph_view
            .values()
            .next()
            .map(|view| view.search.text().to_owned())
            .unwrap_or_default(),
        TextFieldSeat::Settings => runtime
            .window
            .settings
            .text_field(crate::settings::SettingsTarget::Field(
                crate::settings::SettingsRow::QuakeCommand,
            ))
            .map(|field| field.text().to_owned())
            .unwrap_or_default(),
    };
    let shell_untouched = runtime
        .focused_mut()
        .is_none_or(|shell| shell.pending_paste.is_none() && shell.pending_typing.is_none());
    (text, shell_untouched)
}

/// RED (B-AUDIT-046 RT-1) — **a paste goes to the text field that holds the keyboard, by the
/// chord, by `Shift+Insert` (where the host reads it as a paste) and by Edit ▸ Paste, and never to
/// the shell behind it** — every field there is: the find
/// bar, the graph's search, a git menu's prompt, the palette, a tab's name and a settings field.
/// Each takes the clipboard's line (the settings field drops its breaks, the others take the first
/// line through `text_field::one_line`), by both roads, and the shell behind it is handed nothing.
/// The fields whose rungs stand above the clipboard rung (`git_menu_key`, `settings_field_key`,
/// `palette_key`, the rename's `rename_pastes`) take the chord themselves; Edit ▸ Paste reaches
/// each through `paste_into_field`, whose per-field inserts are the ones driven here.
///
/// MUTATIONS: make `menubar::paste_seat` answer `clipboard_seat(focus)` whatever the field — the
/// menu hands the paste to the terminal; drop the paste arm of `git_menu_key`, of
/// `settings_field_key` or of `palette_key`, or `rename_pastes`' arm of the rename key — that
/// field's chord pastes nothing; send one of `apply_clipboard_text_to_field`'s six fields to
/// another's insert — that field stays empty by the menu; drop `shift_insert` from
/// `input::is_paste_shortcut_on` — off a Mac, every field stays empty by `Shift+Insert`.
#[test]
fn paste_goes_to_the_focused_field_and_never_to_the_shell_behind_it_by_either_road() {
    use crate::TextFieldSeat;
    for field in [
        TextFieldSeat::FindBar,
        TextFieldSeat::GraphSearch,
        TextFieldSeat::GitPrompt,
        TextFieldSeat::Palette,
        TextFieldSeat::TabName,
        TextFieldSeat::Settings,
    ] {
        let expected = if field == TextFieldSeat::Settings {
            "粘贴 pastedsecond".to_owned()
        } else {
            crate::text_field::one_line("粘贴 pasted\r\nsecond")
        };
        for road in paste_roads() {
            let (text, shell_untouched) = paste_into(field, road);
            assert_eq!(text, expected, "{field:?} takes the paste by {road:?}");
            assert!(
                shell_untouched,
                "{field:?}: nothing reached the shell behind it by {road:?}"
            );
        }
    }
}

/// What a paste left on the shell holding the keyboard: a paste held for it, typing held for it,
/// and the multi-line paste card raised over it.
fn what_the_shell_was_handed(folio: &mut HeadlessFolio) -> (bool, Option<(String, bool)>, bool) {
    let mut runtime = folio.runtime();
    let card = runtime.paste_card_seat().is_some();
    let shell = runtime.focused_mut().expect("the shell");
    (
        shell.pending_paste.is_some(),
        shell.pending_typing.clone(),
        card,
    )
}

/// RED (M3-1) — **Edit ▸ Paste with the shell holding the keyboard is the chord's paste**: the
/// clipboard's two lines reach the shell by the same door, and leave it in the same state —
/// whatever that door does with a multi-line paste (hold it, raise the card over it), it does
/// once, the same way, by either road.
///
/// MUTATION: make the menu's terminal door write the clipboard's text into the shell itself
/// instead of `self.paste_from_clipboard()` — the menu's paste skips the door's multi-line policy.
#[test]
fn the_menus_paste_into_the_shell_is_the_chords() {
    let handed = |through_the_menu: bool| {
        let leaf = leaf_saying("SHELL 外壳");
        let mut folio =
            HeadlessFolio::with_tabs("headless-shell-paste", vec![tab_holding(leaf)], 0);
        *folio.window.window.clipboard.borrow_mut() = Some("第一行 one\r\n第二行 two".to_owned());
        if through_the_menu {
            folio
                .runtime()
                .run_an_application_menu_verb(AppMenuAction::PasteIntoFocus)
                .expect("the menu's paste is answered");
        } else {
            chord(&mut folio, "v", KeyCode::KeyV);
        }
        what_the_shell_was_handed(&mut folio)
    };
    let by_key = handed(false);
    assert_ne!(
        by_key,
        (false, None, false),
        "the chord hands the shell its paste"
    );
    assert_eq!(handed(true), by_key, "and the menu hands it the same");
}

/// GUARD (T-HEADLESS-RUNTIME-SEAM round 3) — **the keystroke ladder has one road to the
/// terminal's paste**: `keyboard_input` calls `self.paste_from_clipboard()` once, in the clipboard
/// rung's `Terminal` arm, after `menubar::paste_seat` has answered. The paste test above drives
/// every field and the shell by every paste key; what it cannot say is that no *other* key or
/// rung reaches the terminal's paste without asking the seat — a second call site is a fact about
/// the source, so this count, and only it, is read from it.
///
/// MUTATION: add a second `self.paste_from_clipboard()?;` to any rung of `keyboard_input` — the
/// count is two.
#[test]
fn the_keystroke_ladder_has_one_road_to_the_terminals_paste() {
    let ladder = crate::test_support::method_body("Runtime", "keyboard_input");
    assert_eq!(
        ladder.matches("self.paste_from_clipboard()").count(),
        1,
        "a second road to the terminal's paste in the keystroke ladder"
    );
}
