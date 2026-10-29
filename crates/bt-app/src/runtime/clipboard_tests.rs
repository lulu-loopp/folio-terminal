//! **The clipboard's roads, driven** (T-HEADLESS-RUNTIME-SEAM; T-CLIPBOARD-LANE): the
//! keystroke's copy and paste and the Edit menu's, run on a headless window whose clipboard is the
//! window seam's own scripted door, served on the clipboard lane's worker — never the machine's.

use crate::headless::{
    HeadlessApplication, HeadlessFolio, HeadlessParts, headless_window, key_down,
};
use crate::preview_press::Spend;
use crate::preview_select::Grain;
use crate::test_support::tab_with_a_preview;
use crate::test_support::{buffer_saying, host_spelling, leaf_saying, seat_of, tab_holding};
use crate::{PreviewSurface, TabId};
use bt_platform::menu::AppMenuAction;
use std::time::Instant;
use winit::keyboard::{Key, KeyCode, ModifiersState};
use winit::window::WindowId;

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

/// What the window put on the clipboard since the last time this was asked, once the clipboard
/// lane has answered every write it asked for.
fn written(folio: &mut HeadlessFolio) -> Vec<String> {
    folio.settle_clipboard();
    folio.window.window.clipboard.take_written()
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
/// MUTATION: make the lane's answer for a page insert the clipboard's text itself
/// (`self.insert_into_preview(&text)` in `Runtime::spend_paste`) instead of
/// `self.apply_clipboard_text_to_preview(&text)` — the paste keeps the clipboard's `\r\n`.
#[test]
fn the_edit_menus_clipboard_verbs_land_where_the_keystrokes_do() {
    // Copy, from the page being typed into.
    let (mut folio, _, _) = page_being_edited("headless-menu-copy");
    chord(&mut folio, "c", KeyCode::KeyC);
    let by_key = written(&mut folio);
    folio
        .runtime()
        .run_an_application_menu_verb(AppMenuAction::CopySelection)
        .expect("the menu's copy is answered");
    let by_menu = written(&mut folio);
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
        folio.window.window.clipboard.holds("甲\r\n乙");
        if through_the_menu {
            folio
                .runtime()
                .run_an_application_menu_verb(AppMenuAction::PasteIntoFocus)
                .expect("the menu's paste is answered");
        } else {
            chord(&mut folio, "v", KeyCode::KeyV);
        }
        folio.settle_clipboard();
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
        let leaf = runtime.focused_pane_mut().expect("the shell");
        let frame = leaf
            .session
            .session
            .viewport_frame(&mut leaf.view.projection)
            .expect("a frame");
        let selection = bt_viewport::ViewSelection {
            start: frame
                .anchor_at(0, 0, bt_doc::Bias::Before)
                .unwrap()
                .unwrap(),
            end: frame.anchor_at(0, 6, bt_doc::Bias::After).unwrap().unwrap(),
        };
        leaf.session
            .session
            .set_view_selection(Some(selection.clone()));
        leaf.view.projection.set_selection(Some(selection));
    };
    select(&mut folio);
    chord(&mut folio, "c", KeyCode::KeyC);
    let by_key = written(&mut folio);
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
        written(&mut folio),
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
                    crate::runtime::keyboard::ime_owner(runtime.keyboard_owner()),
                    crate::ImeOwner::GraphSearch,
                    "the fixture's own precondition: the graph's search holds the keyboard"
                );
            }
            TextFieldSeat::Settings => runtime
                .open_settings_on_row(crate::settings::SettingsRow::QuakeCommand)
                .expect("the dialog opens on the field"),
        }
    }
    folio.window.window.clipboard.holds(PASTED);
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
    folio.settle_clipboard();
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
/// `settings_field_key` or of `palette_key`, or the rename's paste chord in `keyboard_input` — that
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
        folio
            .window
            .window
            .clipboard
            .holds("第一行 one\r\n第二行 two");
        if through_the_menu {
            folio
                .runtime()
                .run_an_application_menu_verb(AppMenuAction::PasteIntoFocus)
                .expect("the menu's paste is answered");
        } else {
            chord(&mut folio, "v", KeyCode::KeyV);
        }
        folio.settle_clipboard();
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

// ── the clipboard lane, through the window (T-CLIPBOARD-LANE) ──────────────────────────────────

/// What the clipboard lane a test hands the window said in `diagnostics.log`'s place, what its
/// sink was handed, and its wake.
struct Recorded {
    said: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    sunk: std::sync::Arc<std::sync::Mutex<Vec<crate::clipboard_lane::ClipboardTerminal>>>,
    wakes: std::sync::mpsc::Receiver<()>,
}

impl Recorded {
    fn said(&self) -> Vec<String> {
        self.said
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn sunk(&self) -> Vec<crate::clipboard_lane::ClipboardTerminal> {
        self.sunk
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// **The window's clipboard lane, swapped for one whose lines and sink the test reads** — the
/// product's lane (`clipboard_lane::ClipboardLane::start`), its worker started by `executor`.
fn recording_lane(folio: &mut HeadlessFolio, executor: &crate::lane::Executor) -> Recorded {
    use std::sync::{Arc, Mutex, PoisonError};
    let said = Arc::new(Mutex::new(Vec::new()));
    let sunk = Arc::new(Mutex::new(Vec::new()));
    let (wake, wakes) = crate::lane::wake_channel();
    let (heard, kept) = (Arc::clone(&said), Arc::clone(&sunk));
    folio.app.clipboard_lane = crate::clipboard_lane::ClipboardLane::start(
        executor,
        wake,
        move |line: &str| {
            heard
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(line.to_owned());
        },
        move |_, terminal| {
            kept.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(terminal);
        },
    );
    Recorded { said, sunk, wakes }
}

/// A shell whose pseudoconsole is still being born: what is offered to it is held, in order, in
/// its birth's queue — which is what the test reads back as what the shell was sent.
fn a_shell_being_born() -> crate::TerminalPane {
    let mut leaf = leaf_saying("SHELL 外壳");
    leaf.session.life.begin_birth(crate::PaneBirth {
        seed: crate::LeafSeed {
            profile: crate::profiles::DEFAULT_IDENTITY.to_owned(),
            ..crate::LeafSeed::default()
        },
        probe_input: None,
        typed: std::cell::RefCell::default(),
        waiting: crate::BirthWait::Programs {
            _waits: crate::programs_lane::pane_waits(),
        },
    });
    leaf
}

/// One window holding [`a_shell_being_born`], the shell holding the keyboard, the clipboard
/// holding `text`.
fn a_window_pasting(tag: &str, text: &str) -> HeadlessFolio {
    let folio = HeadlessFolio::with_tabs(tag, vec![tab_holding(a_shell_being_born())], 0);
    folio.window.window.clipboard.holds(text);
    folio
}

/// What the shell in the window's tab `tab` has been sent, in order.
fn sent_to_the_shell(folio: &mut HeadlessFolio, tab: usize) -> String {
    let runtime = folio.runtime();
    let leaf = runtime.window.tabs[tab]
        .sessions
        .values()
        .next()
        .expect("the shell");
    let typed = leaf
        .session
        .life
        .birth()
        .expect("in birth")
        .typed
        .borrow()
        .clone();
    String::from_utf8(typed).expect("UTF-8 was sent")
}

/// How many notices on the window say `text`.
fn notices(folio: &mut HeadlessFolio, text: &str) -> usize {
    folio
        .runtime()
        .window
        .toasts
        .toasts()
        .iter()
        .filter(|toast| toast.body() == text)
        .count()
}

/// A key typed into whatever holds the keyboard.
fn type_key(folio: &mut HeadlessFolio, letter: &str, code: KeyCode) {
    folio
        .runtime()
        .keyboard_input(
            &key_down(Key::Character(letter.into()), code, Some(letter)),
            false,
        )
        .expect("the keystroke is answered");
}

/// An input method's commit.
fn commit(folio: &mut HeadlessFolio, text: &str) {
    folio
        .runtime()
        .ime_input(winit::event::Ime::Commit(text.to_owned()))
        .expect("the commit is answered");
}

/// RED (T-CLIPBOARD-LANE) — **a paste whose `OpenClipboard` is refused five times says so once and
/// writes nothing**: the lane's ledger says the failure once, in the door's words, and the window
/// says its one notice — on the clipboard lane's worker, never the window thread.
///
/// MUTATION: in `Runtime::spend_paste`, let `Failed` write nothing on screen — the notice is
/// missing (`0`, not `1`); or let it fall to `apply_clipboard_payload` of an empty payload — no
/// notice either.
#[test]
fn a_paste_whose_open_is_refused_five_times_says_so_once_and_writes_nothing() {
    let mut folio = a_window_pasting("clipboard-lane-busy", "粘贴 never");
    let lane = recording_lane(
        &mut folio,
        &crate::lane::Executor::thread_door(bt_platform::ThreadPriority::Normal),
    );
    folio.window.window.clipboard.refuse_opens(5);
    chord(&mut folio, "v", KeyCode::KeyV);
    folio.settle_clipboard();
    assert_eq!(
        sent_to_the_shell(&mut folio, 0),
        "",
        "nothing reached the shell"
    );
    assert_eq!(
        notices(&mut folio, crate::i18n::Text::PasteClipboardRead.text()),
        1,
        "the notice is said once"
    );
    let said = lane.said();
    assert_eq!(said.len(), 1, "the ledger says the failure once: {said:?}");
    assert!(
        said[0].contains("OpenClipboard failed after 5 attempts"),
        "{said:?}"
    );
    assert!(
        lane.sunk().is_empty(),
        "the window took its terminal itself"
    );
}

/// RED (T-CLIPBOARD-LANE; the side-effects contract's (c)4) — **keys typed after a paste reach
/// the shell after the pasted text**: a paste answered by a held worker lands where the press was,
/// and the key, the Chinese commit and the mixed-script key typed while it was owed follow it, in
/// the order they were typed.
///
/// MUTATION: make `ShellSession::person_input_target` answer `self.input_target()` whatever the
/// hold — the keys reach the shell before the paste (`x中文a粘贴 pasted`).
#[test]
fn keys_typed_after_a_paste_reach_the_shell_after_the_pasted_text() {
    let mut folio = a_window_pasting("clipboard-lane-order", "粘贴 pasted");
    let hold = folio.window.window.clipboard.clone();
    hold.worker_hold().hold();
    chord(&mut folio, "v", KeyCode::KeyV);
    hold.worker_hold()
        .wait_standing("the paste on the clipboard lane's worker");
    type_key(&mut folio, "x", KeyCode::KeyX);
    commit(&mut folio, "中文");
    type_key(&mut folio, "a", KeyCode::KeyA);
    assert_eq!(
        sent_to_the_shell(&mut folio, 0),
        "",
        "the keys wait behind the paste that has not answered"
    );
    hold.worker_hold().release();
    folio.settle_clipboard();
    assert_eq!(sent_to_the_shell(&mut folio, 0), "粘贴 pastedx中文a");
}

/// RED (T-CLIPBOARD-LANE) — **a paste whose pane closes before it is answered ends `Cancelled`**:
/// once, in the lane's sink and on no surface; nothing is written anywhere, the worker that took
/// it answers into nothing, and the window says nothing on screen — the ledger's place
/// (`diagnostics.log`) has the cancel.
///
/// MUTATION: in `Runtime::paste_withdrawn`, answer `None` for a target that is gone — the paste
/// stays owed after its pane closed, and the sink is empty.
#[test]
fn a_paste_whose_pane_closes_ends_cancelled_and_writes_nothing() {
    let mut other = tab_holding(leaf_saying("另一个 other"));
    other.id = crate::TabId(2);
    let mut folio = HeadlessFolio::with_tabs(
        "clipboard-lane-cancel",
        vec![tab_holding(a_shell_being_born()), other],
        0,
    );
    folio.window.window.clipboard.holds("粘贴 never");
    let lane = recording_lane(
        &mut folio,
        &crate::lane::Executor::thread_door(bt_platform::ThreadPriority::Normal),
    );
    let hold = folio.window.window.clipboard.clone();
    hold.worker_hold().hold();
    chord(&mut folio, "v", KeyCode::KeyV);
    hold.worker_hold()
        .wait_standing("the paste on the clipboard lane's worker");
    folio
        .runtime()
        .close_tab(0)
        .expect("the pasting tab closes");
    let now = Instant::now();
    folio
        .runtime()
        .settle_clipboard(now)
        .expect("the turn settles the clipboard");
    assert!(
        folio.runtime().window.clipboard.pastes.is_empty(),
        "withdrawn"
    );
    assert_eq!(lane.sunk(), [crate::lane::Terminal::Cancelled]);
    hold.worker_hold().release();
    crate::lane::wait_for_a_wake(&lane.wakes, "the worker's answer into nothing");
    folio.settle_clipboard();
    assert_eq!(
        lane.sunk(),
        [crate::lane::Terminal::Cancelled],
        "ended once"
    );
    assert_eq!(
        notices(&mut folio, crate::i18n::Text::PasteClipboardBusy.text())
            + notices(&mut folio, crate::i18n::Text::PasteClipboardRead.text()),
        0,
        "said nowhere on screen"
    );
}

/// RED (T-CLIPBOARD-LANE round 2) — **two pastes into one pane queue, in the order they were
/// pressed**: `Ctrl+V`, `k`, `Ctrl+V` with the clipboard's worker held reaches the shell as the
/// first paste, `k`, the second — what the shell is sent when the clipboard answers at once — and
/// each ends once, taken by the window: nothing reaches the lane's sink, nothing is said.
///
/// MUTATION: in `Runtime::ask_for_a_paste`, supersede the paste still owed by the newer one
/// (`self.app.clipboard_lane.supersede(older, id)` on the last owed paste) — the first press's
/// text is lost (`k粘贴 pasted`).
#[test]
fn two_pastes_into_one_pane_queue_in_the_order_they_were_pressed() {
    let mut folio = a_window_pasting("clipboard-lane-queue", "粘贴 pasted");
    let lane = recording_lane(
        &mut folio,
        &crate::lane::Executor::thread_door(bt_platform::ThreadPriority::Normal),
    );
    let hold = folio.window.window.clipboard.clone();
    hold.worker_hold().hold();
    chord(&mut folio, "v", KeyCode::KeyV);
    hold.worker_hold()
        .wait_standing("the first paste on the clipboard lane's worker");
    type_key(&mut folio, "k", KeyCode::KeyK);
    chord(&mut folio, "v", KeyCode::KeyV);
    hold.worker_hold().release();
    folio.settle_clipboard();
    assert_eq!(sent_to_the_shell(&mut folio, 0), "粘贴 pastedk粘贴 pasted");
    assert!(
        lane.sunk().is_empty(),
        "each terminal was taken by the window"
    );
    assert!(lane.said().is_empty(), "and none was a failure");
}

/// RED (T-CLIPBOARD-LANE round 2) — **a paste owed to the shell is not lost to a later paste into a
/// field**: with the worker held, a paste pressed in the shell and then one pressed in the palette
/// that opened over it both land — the shell's in the shell, the palette's in its box.
///
/// MUTATION: in `Runtime::ask_for_a_paste`, supersede the paste still owed by the newer one — the
/// shell is sent nothing.
#[test]
fn a_shell_paste_still_owed_lands_beside_a_later_paste_into_a_field() {
    let mut folio = a_window_pasting("clipboard-lane-cross", "粘贴 pasted");
    let hold = folio.window.window.clipboard.clone();
    hold.worker_hold().hold();
    chord(&mut folio, "v", KeyCode::KeyV);
    hold.worker_hold()
        .wait_standing("the shell's paste on the clipboard lane's worker");
    folio
        .runtime()
        .open_command_palette()
        .expect("the palette opens");
    chord(&mut folio, "v", KeyCode::KeyV);
    hold.worker_hold().release();
    folio.settle_clipboard();
    assert_eq!(sent_to_the_shell(&mut folio, 0), "粘贴 pasted");
    let query = folio
        .runtime()
        .window
        .palette
        .as_ref()
        .map(|palette| palette.query().into_owned())
        .unwrap_or_default();
    assert_eq!(query, "粘贴 pasted");
}

/// RED (T-CLIPBOARD-LANE round 2) — **a window owes at most `PASTES_OWED` pastes**: with the
/// worker held, the press past that desk is refused on the window, says once that the clipboard
/// was busy, and the pastes before it all land, in order, when the worker goes on.
///
/// MUTATION: drop the desk's check in `Runtime::ask_for_a_paste` — the ninth press is queued and
/// nothing is said.
#[test]
fn a_paste_past_the_windows_desk_is_refused_on_it() {
    let mut folio = a_window_pasting("clipboard-lane-desk", "粘");
    let hold = folio.window.window.clipboard.clone();
    hold.worker_hold().hold();
    chord(&mut folio, "v", KeyCode::KeyV);
    hold.worker_hold()
        .wait_standing("the first paste on the clipboard lane's worker");
    for _ in 1..crate::clipboard_lane::PASTES_OWED {
        chord(&mut folio, "v", KeyCode::KeyV);
    }
    assert_eq!(
        notices(&mut folio, crate::i18n::Text::PasteClipboardBusy.text()),
        0
    );
    chord(&mut folio, "v", KeyCode::KeyV);
    assert_eq!(
        notices(&mut folio, crate::i18n::Text::PasteClipboardBusy.text()),
        1,
        "the press past the desk says so once"
    );
    assert_eq!(
        folio.runtime().window.clipboard.pastes.len(),
        crate::clipboard_lane::PASTES_OWED
    );
    hold.worker_hold().release();
    folio.settle_clipboard();
    assert_eq!(
        sent_to_the_shell(&mut folio, 0),
        "粘".repeat(crate::clipboard_lane::PASTES_OWED)
    );
}

/// RED (T-CLIPBOARD-LANE; (c)4) — **a paste past its deadline is cancelled, the held keys go in
/// order, and its late answer is ignored**: at `PASTE_DEADLINE` (the turn's clock, here a fake
/// one) the paste ends `Cancelled` in the sink, the key held behind it reaches the shell, the
/// window says once that the clipboard was busy, and the worker's answer when it comes writes
/// nothing.
///
/// MUTATION: no deadline in `Runtime::paste_withdrawn` — the key stays held for ever (`""`).
#[test]
fn a_paste_past_its_deadline_is_cancelled_and_the_held_keys_go_in_order() {
    let mut folio = a_window_pasting("clipboard-lane-deadline", "粘贴 late");
    let lane = recording_lane(
        &mut folio,
        &crate::lane::Executor::thread_door(bt_platform::ThreadPriority::Normal),
    );
    let hold = folio.window.window.clipboard.clone();
    hold.worker_hold().hold();
    chord(&mut folio, "v", KeyCode::KeyV);
    hold.worker_hold()
        .wait_standing("the paste on the clipboard lane's worker");
    commit(&mut folio, "中文");
    let past = folio
        .runtime()
        .window
        .clipboard
        .deadline()
        .expect("a paste is owed");
    folio
        .runtime()
        .settle_clipboard(past)
        .expect("the turn settles the clipboard");
    assert_eq!(sent_to_the_shell(&mut folio, 0), "中文");
    assert_eq!(lane.sunk(), [crate::lane::Terminal::Cancelled]);
    assert_eq!(
        notices(&mut folio, crate::i18n::Text::PasteClipboardBusy.text()),
        1
    );
    hold.worker_hold().release();
    crate::lane::wait_for_a_wake(&lane.wakes, "the late answer");
    folio.settle_clipboard();
    assert_eq!(
        sent_to_the_shell(&mut folio, 0),
        "中文",
        "the late answer is ignored"
    );
}

/// RED (T-CLIPBOARD-LANE; (c)4; round 2) — **held input past the cap cancels the paste and goes in
/// order, each write as it was typed**: a commit, then a flood larger than the pane's input ring
/// on its own (`bt_pty::PTY_INPUT_RING_BYTES`), then a key — on the turn the cap is passed the
/// paste is withdrawn and the three are offered to the shell as a burst, in their order, each as
/// its own write: the first and the key are taken, and the flood, which does not fit beside the
/// first, is refused and lost, its one line said by the pane's own door.
///
/// MUTATION: make `PasteHold::over_the_cap` answer `false` — the paste stays owed and nothing is
/// sent (`""`).
#[test]
fn held_input_over_the_cap_cancels_the_paste_and_goes_in_order() {
    let mut folio = a_window_pasting("clipboard-lane-cap", "粘贴 never");
    let lane = recording_lane(
        &mut folio,
        &crate::lane::Executor::thread_door(bt_platform::ThreadPriority::Normal),
    );
    let hold = folio.window.window.clipboard.clone();
    hold.worker_hold().hold();
    chord(&mut folio, "v", KeyCode::KeyV);
    hold.worker_hold()
        .wait_standing("the paste on the clipboard lane's worker");
    commit(&mut folio, "中文");
    let flood = "汉".repeat(bt_pty::PTY_INPUT_RING_BYTES.get() / 3 + 1);
    commit(&mut folio, &flood);
    type_key(&mut folio, "z", KeyCode::KeyZ);
    let asked = folio
        .runtime()
        .window
        .clipboard
        .pastes
        .first()
        .map(|owed| owed.asked_at)
        .expect("a paste is owed");
    folio
        .runtime()
        .settle_clipboard(asked)
        .expect("the turn settles the clipboard");
    assert_eq!(sent_to_the_shell(&mut folio, 0), "中文z");
    assert_eq!(lane.sunk(), [crate::lane::Terminal::Cancelled]);
    hold.worker_hold().release();
}

/// RED (T-CLIPBOARD-LANE round 3) — **a write that fits alone but not beside the held writes
/// before it is lost at the release**: two commits, each smaller than the pane's input ring, that
/// together pass it, and a key. On the turn the cap is passed the paste is withdrawn and what was
/// held goes as a burst in that one turn: the first commit and the key are taken, the second does
/// not fit beside the first and is refused, with one line — where, unheld, the two would have
/// reached the shell spread over the paste's second, and the shell could have read the first
/// before the second came. What the person sees is what the release really does, not "nothing is
/// lost".
///
/// MUTATION: make `PasteHold::over_the_cap` answer `false` — the paste stays owed and nothing is
/// sent (`""`).
#[test]
fn a_write_that_fits_alone_but_not_beside_the_held_writes_is_lost_at_the_release() {
    let mut folio = a_window_pasting("clipboard-lane-cap-burst", "粘贴 never");
    let lane = recording_lane(
        &mut folio,
        &crate::lane::Executor::thread_door(bt_platform::ThreadPriority::Normal),
    );
    let hold = folio.window.window.clipboard.clone();
    hold.worker_hold().hold();
    chord(&mut folio, "v", KeyCode::KeyV);
    hold.worker_hold()
        .wait_standing("the paste on the clipboard lane's worker");
    let ring = bt_pty::PTY_INPUT_RING_BYTES.get();
    let first = "汉".repeat(ring / 6);
    let second = "字".repeat(ring / 6 + 4);
    assert!(
        second.len() <= ring && first.len() + second.len() > ring,
        "the fixture's own precondition: the second fits alone and not beside the first"
    );
    commit(&mut folio, &first);
    commit(&mut folio, &second);
    type_key(&mut folio, "z", KeyCode::KeyZ);
    let asked = folio
        .runtime()
        .window
        .clipboard
        .pastes
        .first()
        .map(|owed| owed.asked_at)
        .expect("a paste is owed");
    folio
        .runtime()
        .settle_clipboard(asked)
        .expect("the turn settles the clipboard");
    assert_eq!(sent_to_the_shell(&mut folio, 0), format!("{first}z"));
    assert_eq!(lane.sunk(), [crate::lane::Terminal::Cancelled]);
    hold.worker_hold().release();
}

/// RED (T-CLIPBOARD-LANE round 3) — **a paste reads the clipboard as it stood at its press**: with
/// the worker busy, `Ctrl+C` (the first copy), `Ctrl+V`, `Ctrl+C` (the second copy) — the second
/// copy does not replace the first, because a paste was asked between them: it queues behind the
/// paste, the paste pastes the first copy's text, and the clipboard ends holding the second's.
///
/// MUTATION: in `Runtime::submit_clipboard_write`, let a newer copy replace the older one whatever
/// was asked between them (`replaces` always `true`) — the paste reads the clipboard as it was
/// before the first copy (`旧 old`).
#[test]
fn a_paste_between_two_copies_reads_the_first_and_the_second_queues_behind_it() {
    let mut folio = a_window_pasting("clipboard-lane-copy-paste-copy", "旧 old");
    let hold = folio.window.window.clipboard.clone();
    hold.worker_hold().hold();
    // Something already on the worker, so every request below waits in its line.
    let door: std::sync::Arc<dyn crate::clipboard_lane::ClipboardDoor> =
        std::sync::Arc::new(hold.clone());
    let busy = folio
        .app
        .clipboard_lane
        .submit(door, crate::clipboard_lane::Ask::Text)
        .expect("admitted");
    hold.worker_hold()
        .wait_standing("a request on the clipboard lane's worker");
    folio
        .runtime()
        .submit_clipboard_write(
            "一 first".to_owned(),
            "the first copy",
            crate::ClipboardWriteEffect::None,
        )
        .expect("the first copy is asked");
    chord(&mut folio, "v", KeyCode::KeyV);
    folio
        .runtime()
        .submit_clipboard_write(
            "二 second".to_owned(),
            "the second copy",
            crate::ClipboardWriteEffect::None,
        )
        .expect("the second copy is asked");
    hold.worker_hold().release();
    folio.settle_clipboard();
    assert_eq!(sent_to_the_shell(&mut folio, 0), "一 first");
    assert_eq!(
        folio.window.window.clipboard.take_written(),
        ["一 first".to_owned(), "二 second".to_owned()],
        "both copies were written, in order"
    );
    drop(busy);
}

/// RED (T-CLIPBOARD-LANE; (c)4) — **the worker's end ends the paste and releases the keys**: the
/// lane's worker ends with the paste owed; the paste ends `WorkerGone` — the seam's terminal for
/// it, not a cancel — the key held behind it reaches the shell, and the window says the
/// read-failed notice once.
///
/// MUTATION: drop `Terminal::WorkerGone` from `Runtime::spend_paste`'s notice arm — the notice is
/// not said (`0`).
#[test]
fn worker_gone_ends_the_paste_and_releases_the_keys() {
    let mut folio = a_window_pasting("clipboard-lane-gone", "粘贴 never");
    let gate = std::sync::Arc::new(crate::lane::Gate::default());
    let lane = recording_lane(
        &mut folio,
        &crate::lane::Executor::scripted(std::sync::Arc::clone(&gate)),
    );
    gate.hold();
    chord(&mut folio, "v", KeyCode::KeyV);
    assert!(gate.wait_entered(1), "the paste is on the worker");
    type_key(&mut folio, "g", KeyCode::KeyG);
    gate.kill();
    gate.release();
    crate::lane::wait_for_a_wake(&lane.wakes, "the worker's end");
    folio.settle_clipboard();
    assert_eq!(sent_to_the_shell(&mut folio, 0), "g");
    assert_eq!(
        notices(&mut folio, crate::i18n::Text::PasteClipboardRead.text()),
        1
    );
}

/// RED (T-CLIPBOARD-LANE; (c)4; round 2) — **a request past the lane's bound is refused at
/// once**: with `clipboard_lane::BOUND` owed (a desk of pastes and a copy in each of two windows),
/// a paste is refused at the lane's door, synchronously; the window says once that the clipboard
/// was busy, owes nothing, and holds no input.
///
/// MUTATION: build the lane's ledger with no bound (`None` for `Some(BOUND)` in
/// `ClipboardLane::start`) — the paste is queued, and nothing is said.
#[test]
fn a_request_past_the_lanes_bound_is_refused_at_once() {
    let mut folio = a_window_pasting("clipboard-lane-full", "粘贴 never");
    let gate = std::sync::Arc::new(crate::lane::Gate::default());
    let _lane = recording_lane(
        &mut folio,
        &crate::lane::Executor::scripted(std::sync::Arc::clone(&gate)),
    );
    gate.hold();
    let door: std::sync::Arc<dyn crate::clipboard_lane::ClipboardDoor> =
        std::sync::Arc::new(folio.window.window.clipboard.clone());
    let owed: Vec<_> = (0..crate::clipboard_lane::BOUND)
        .map(|_| {
            folio
                .app
                .clipboard_lane
                .submit(
                    std::sync::Arc::clone(&door),
                    crate::clipboard_lane::Ask::Text,
                )
                .expect("admitted below the bound")
        })
        .collect();
    chord(&mut folio, "v", KeyCode::KeyV);
    assert!(
        folio.runtime().window.clipboard.pastes.is_empty(),
        "refused at the door"
    );
    assert_eq!(
        notices(&mut folio, crate::i18n::Text::PasteClipboardBusy.text()),
        1
    );
    type_key(&mut folio, "y", KeyCode::KeyY);
    assert_eq!(sent_to_the_shell(&mut folio, 0), "y", "no input is held");
    drop(owed);
    gate.release();
}

/// RED (T-CLIPBOARD-LANE) — **the copy write's refusal is said once**: a `Ctrl+C` whose
/// `OpenClipboard` is refused five times is its request's `Failed`, said once in the lane's
/// ledger; nothing was written, nothing is said on screen, and the selection stands to be copied
/// again.
///
/// MUTATION: publish `Done` for the door's refusal in `run_clipboard_lane` — the refusal is said
/// nowhere (no line, not one).
#[test]
fn the_copy_writes_refusal_is_said_once() {
    let leaf = leaf_saying("复制 retry me");
    let mut folio = HeadlessFolio::with_tabs("clipboard-lane-copy", vec![tab_holding(leaf)], 0);
    let lane = recording_lane(
        &mut folio,
        &crate::lane::Executor::thread_door(bt_platform::ThreadPriority::Normal),
    );
    {
        let mut runtime = folio.runtime();
        let leaf = runtime.focused_pane_mut().expect("the shell");
        let frame = leaf
            .session
            .session
            .viewport_frame(&mut leaf.view.projection)
            .expect("a frame");
        let selection = bt_viewport::ViewSelection {
            start: frame
                .anchor_at(0, 0, bt_doc::Bias::Before)
                .unwrap()
                .unwrap(),
            end: frame.anchor_at(0, 6, bt_doc::Bias::After).unwrap().unwrap(),
        };
        leaf.session
            .session
            .set_view_selection(Some(selection.clone()));
        leaf.view.projection.set_selection(Some(selection));
    }
    folio.window.window.clipboard.refuse_opens(5);
    chord(&mut folio, "c", KeyCode::KeyC);
    assert!(written(&mut folio).is_empty(), "nothing was written");
    let said = lane.said();
    assert_eq!(said.len(), 1, "the refusal is said once: {said:?}");
    assert!(
        said[0].contains("OpenClipboard failed after 5 attempts"),
        "{said:?}"
    );
    assert!(
        folio.runtime().window.toasts.toasts().is_empty(),
        "and nothing on screen"
    );
    assert!(
        folio
            .runtime()
            .focused_mut()
            .is_some_and(|leaf| leaf.session.view_selection().is_some()),
        "the selection stands to be copied again"
    );
}

/// The bodies of the notices on window `id` of `application`, oldest first.
fn notices_of(application: &mut HeadlessApplication, id: WindowId) -> Vec<String> {
    application
        .folio
        .runtime(id)
        .expect("the window is open")
        .window
        .toasts
        .toasts()
        .iter()
        .map(|toast| toast.body().to_owned())
        .collect()
}

/// RED (T-CLIPBOARD-ORDER-HOLDS-BY-SPEED, plan row 72) — **the application answers the
/// clipboard's requests in the order they were asked, copies and pastes together, across the
/// windows**: one drain of the lane's wake (`FolioApp::answer_clipboard`) takes every window's
/// answers and answers them in admission order, each in its own window.
///
/// Window 1 pastes (the clipboard refuses its five opens, so the paste says the read-failed
/// notice), window 2 copies, window 1 copies, each copy saying it landed; the worker is held on
/// the paste until all three are asked and then let go, and the drain comes once every one has
/// ended. Window 1 says the paste's notice before its copy's. No timing decides it: the three
/// have ended before the drain begins.
///
/// MUTATION: answer each window's copies before its pastes, as `Runtime::settle_clipboard` did
/// before this ticket (take copies first in `ClipboardAsks::answered` and answer them in that
/// order) — window 1 says its copy first.
#[test]
fn the_clipboards_answers_are_spent_in_the_order_they_were_asked() {
    let folio = a_window_pasting("clipboard-lane-admission-order", "粘贴 never");
    let door = folio.window.window.clipboard.clone();
    let second = headless_window(
        &folio.app,
        HeadlessParts::new(&folio.app.gpu, 2, (1200, 800), 1.0, &folio.ledger),
        vec![tab_holding(leaf_saying("SECOND 第二"))],
        0,
    );
    let one = folio.window.window.id;
    let two = WindowId::from(2);
    let mut application = HeadlessApplication::of(folio);
    application.folio.windows.insert(two, second);
    let toast = |text: &str| crate::ClipboardWriteEffect::Toast {
        anchor: crate::toast::ToastAnchor::Window,
        text: text.to_owned(),
    };

    door.worker_hold().hold();
    door.refuse_opens(5);
    application
        .runtime()
        .paste_from_clipboard()
        .expect("window 1 pastes");
    door.worker_hold()
        .wait_standing("window 1's paste on the clipboard lane's worker");
    application
        .folio
        .runtime(two)
        .expect("window 2 is open")
        .submit_clipboard_write(
            "二 second".to_owned(),
            "window 2's copy",
            toast("二 copied"),
        )
        .expect("window 2 copies");
    application
        .runtime()
        .submit_clipboard_write("三 third".to_owned(), "window 1's copy", toast("三 copied"))
        .expect("window 1 copies");
    door.worker_hold().release();
    crate::lane::wait_until("every request ended", || {
        application
            .folio
            .app
            .as_ref()
            .is_some_and(|app| app.clipboard_lane.earliest_owed().is_none())
    });

    application
        .folio
        .answer_clipboard(Instant::now())
        .expect("the drain answers every window");
    assert_eq!(
        notices_of(&mut application, one),
        [
            crate::i18n::Text::PasteClipboardRead.text().to_owned(),
            "三 copied".to_owned()
        ],
        "window 1 answers its paste before the copy it asked after it"
    );
    assert_eq!(notices_of(&mut application, two), ["二 copied".to_owned()]);
    assert!(
        application.runtime().window.clipboard.is_empty(),
        "nothing is left owed"
    );
}

/// What the shell in window `id`'s first tab has been sent, in order.
fn sent_to_the_shell_of(application: &mut HeadlessApplication, id: WindowId) -> String {
    let runtime = application.folio.runtime(id).expect("the window is open");
    let leaf = runtime.window.tabs[0]
        .sessions
        .values()
        .next()
        .expect("the shell");
    let typed = leaf
        .session
        .life
        .birth()
        .expect("in birth")
        .typed
        .borrow()
        .clone();
    String::from_utf8(typed).expect("UTF-8 was sent")
}

/// RED (T-CLIPBOARD-ORDER-HOLDS-BY-SPEED review F1) — **a copy a newer one replaces after it has
/// landed says so in its turn, not on the press**: the window pastes (the clipboard refuses its
/// five opens, so the paste says the read-failed notice) and copies, and both end; then it copies
/// again, which replaces the first copy and finds it landed. The drain answers the paste, then the
/// first copy, then the second. No timing decides it: every request has ended before the drain.
///
/// MUTATION: answer the replaced copy's terminal on the press, as `submit_clipboard_write` did
/// (pop it and `answer_copy` at once) — the first copy says it landed before the paste's notice.
#[test]
fn a_replaced_copy_that_had_landed_says_so_after_the_paste_asked_before_it() {
    let folio = a_window_pasting("clipboard-supersede-in-order", "粘贴 never");
    let door = folio.window.window.clipboard.clone();
    let one = folio.window.window.id;
    let mut application = HeadlessApplication::of(folio);
    let toast = |text: &str| crate::ClipboardWriteEffect::Toast {
        anchor: crate::toast::ToastAnchor::Window,
        text: text.to_owned(),
    };
    let all_ended = |application: &HeadlessApplication| {
        application
            .folio
            .app
            .as_ref()
            .is_some_and(|app| app.clipboard_lane.earliest_owed().is_none())
    };

    door.worker_hold().hold();
    door.refuse_opens(5);
    application
        .runtime()
        .paste_from_clipboard()
        .expect("the window pastes");
    door.worker_hold()
        .wait_standing("the paste on the clipboard lane's worker");
    application
        .runtime()
        .submit_clipboard_write("一 first".to_owned(), "the first copy", toast("一 copied"))
        .expect("the window copies");
    door.worker_hold().release();
    crate::lane::wait_until("the paste and the first copy ended", || {
        all_ended(&application)
    });
    application
        .runtime()
        .submit_clipboard_write(
            "二 second".to_owned(),
            "the second copy",
            toast("二 copied"),
        )
        .expect("the window copies again");
    crate::lane::wait_until("the second copy ended", || all_ended(&application));

    application
        .folio
        .answer_clipboard(Instant::now())
        .expect("the drain answers the window");
    assert_eq!(
        notices_of(&mut application, one),
        [
            crate::i18n::Text::PasteClipboardRead.text().to_owned(),
            "一 copied".to_owned(),
            "二 copied".to_owned()
        ],
        "the paste's notice comes before the copy asked after it, replaced or not"
    );
    assert!(
        application.runtime().window.clipboard.is_empty(),
        "nothing is left owed"
    );
}

/// RED (T-CLIPBOARD-ORDER-HOLDS-BY-SPEED review F2 and N3) — **one drain spends each answer in
/// its own turn, across the windows**: window 1 pastes into its shell, window 2 copies, window 1
/// pastes into the same shell again; all three end before the drain. The drain's answers
/// (`FolioApp::clipboard_answers`) come window 1, window 2, window 1, and answered one at a time
/// as `FolioApp::answer_clipboard` answers them, the first paste's answer writes the first paste
/// alone: the second waits in the pane's hold until its own turn, after window 2's copy has said
/// it landed. No timing decides it: every request has ended before the drain.
///
/// MUTATION (F2): mark every paste ended when the drain takes it (`ClipboardAsks::answered`), as
/// before the fold — the first answer writes both pastes, ahead of window 2's copy. MUTATION
/// (N3): answer window by window instead of merging (`lane::in_admission_order` dropped) — window
/// 1's two pastes come before window 2's copy.
#[test]
fn each_answer_of_a_drain_is_spent_in_its_own_turn_across_the_windows() {
    let folio = a_window_pasting("clipboard-each-in-its-turn", "一 first");
    let door = folio.window.window.clipboard.clone();
    let second = headless_window(
        &folio.app,
        HeadlessParts::new(&folio.app.gpu, 2, (1200, 800), 1.0, &folio.ledger),
        vec![tab_holding(leaf_saying("SECOND 第二"))],
        0,
    );
    let one = folio.window.window.id;
    let two = WindowId::from(2);
    let mut application = HeadlessApplication::of(folio);
    application.folio.windows.insert(two, second);

    door.worker_hold().hold();
    application
        .runtime()
        .paste_from_clipboard()
        .expect("window 1 pastes");
    door.worker_hold()
        .wait_standing("window 1's paste on the clipboard lane's worker");
    application
        .folio
        .runtime(two)
        .expect("window 2 is open")
        .submit_clipboard_write(
            "二 second".to_owned(),
            "window 2's copy",
            crate::ClipboardWriteEffect::Toast {
                anchor: crate::toast::ToastAnchor::Window,
                text: "二 copied".to_owned(),
            },
        )
        .expect("window 2 copies");
    application
        .runtime()
        .paste_from_clipboard()
        .expect("window 1 pastes again");
    door.worker_hold().release();
    crate::lane::wait_until("every request ended", || {
        application
            .folio
            .app
            .as_ref()
            .is_some_and(|app| app.clipboard_lane.earliest_owed().is_none())
    });

    let answers = application.folio.clipboard_answers();
    let order: Vec<(WindowId, &str)> = answers
        .iter()
        .map(|(index, _, answered)| {
            let window = application
                .folio
                .windows
                .key_at(*index)
                .expect("the window is open");
            let kind = match answered {
                crate::clipboard_lane::Answered::Copy { .. } => "copy",
                crate::clipboard_lane::Answered::Paste { .. } => "paste",
            };
            (window, kind)
        })
        .collect();
    assert_eq!(
        order,
        [(one, "paste"), (two, "copy"), (one, "paste")],
        "the drain's answers are in the order they were asked, across the windows"
    );

    let mut seen = Vec::new();
    for (index, id, answered) in answers {
        application
            .folio
            .runtime_at(index)
            .expect("the window is open")
            .answer_clipboard_request(id, answered)
            .expect("the answer is answered");
        let pasted = sent_to_the_shell_of(&mut application, one)
            .matches("一 first")
            .count();
        seen.push((pasted, notices_of(&mut application, two).len()));
    }
    assert_eq!(
        seen,
        [(1, 0), (1, 1), (2, 1)],
        "window 1's second paste reaches its shell after window 2's copy, not with its first"
    );
    application
        .folio
        .answer_clipboard(Instant::now())
        .expect("the windows settle");
    assert!(
        application.runtime().window.clipboard.is_empty(),
        "nothing is left owed"
    );
}
