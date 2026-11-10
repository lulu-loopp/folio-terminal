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
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: read-failed; road=clipboard; sequence=none"],
        "and its one line (plan row 81)"
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
/// each ends once, taken by the window: nothing reaches the lane's sink, nothing is said. Its
/// presses are counted at one held moment ([`HeadlessFolio::hold_the_paste_clock`]), so the first
/// paste is never past its deadline at the second press however long the machine takes.
///
/// MUTATION: in `Runtime::ask_for_a_paste`, supersede the paste still owed by the newer one
/// (`self.app.clipboard_lane.supersede(older, id)` on the last owed paste) — the first press's
/// text is lost (`k粘贴 pasted`).
#[test]
fn two_pastes_into_one_pane_queue_in_the_order_they_were_pressed() {
    let mut folio = a_window_pasting("clipboard-lane-queue", "粘贴 pasted");
    folio.hold_the_paste_clock();
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
/// that opened over it both land — the shell's in the shell, the palette's in its box — its
/// presses counted at one held moment ([`HeadlessFolio::hold_the_paste_clock`]).
///
/// MUTATION: in `Runtime::ask_for_a_paste`, supersede the paste still owed by the newer one — the
/// shell is sent nothing.
#[test]
fn a_shell_paste_still_owed_lands_beside_a_later_paste_into_a_field() {
    let mut folio = a_window_pasting("clipboard-lane-cross", "粘贴 pasted");
    folio.hold_the_paste_clock();
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
/// was busy, and the pastes before it all land, in order, when the worker goes on. Its presses
/// are counted at one held moment ([`HeadlessFolio::hold_the_paste_clock`]): on the wall clock a
/// loaded machine took more than `PASTE_DEADLINE` across the first eight once, and the first
/// paste's deadline said the clipboard was busy before the desk was full (0.4.9 plan row 172).
///
/// MUTATION: drop the desk's check in `Runtime::ask_for_a_paste` — the ninth press is queued and
/// nothing is said.
#[test]
fn a_paste_past_the_windows_desk_is_refused_on_it() {
    let mut folio = a_window_pasting("clipboard-lane-desk", "粘");
    folio.hold_the_paste_clock();
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
    folio.ledger.take();
    chord(&mut folio, "v", KeyCode::KeyV);
    assert_eq!(
        notices(&mut folio, crate::i18n::Text::PasteClipboardBusy.text()),
        1,
        "the press past the desk says so once"
    );
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: busy (desk-full); road=clipboard; sequence=none"]
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
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: busy (deadline); road=clipboard; sequence=none"]
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
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: busy (held-over-the-cap); road=clipboard; sequence=none"]
    );
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
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: read-failed (worker-gone); road=clipboard; sequence=none"]
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
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: busy (lane-full); road=clipboard; sequence=none"]
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

/// The first six columns of the focused shell's first row, selected as a drag would leave them.
fn select_the_first_six(folio: &mut HeadlessFolio) {
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

/// RED (plan row 52, R4) — **a copy while the clipboard is held never holds the window**: the
/// clipboard's worker stands in its hold (another program holding the clipboard), and `Ctrl+C` on
/// a selection returns; a key typed next reaches the shell while the hold stands; once the hold
/// lets go the copy lands, once. [`crate::lane::Hold::release`] fails the test if the hold had let
/// go on its own: something waited on the worker standing in it.
///
/// MUTATION: serve the write on the window thread in the headless `WindowHost::clipboard` (the
/// door stands in the hold itself) — the hold lapses at its patience and the release fails.
#[test]
fn a_copy_while_the_clipboard_is_held_never_holds_the_window() {
    let mut folio = a_window_pasting("clipboard-lane-held-copy", "held 持有");
    select_the_first_six(&mut folio);
    let hold = folio.window.window.clipboard.clone();
    hold.worker_hold().hold();
    chord(&mut folio, "c", KeyCode::KeyC);
    hold.worker_hold()
        .wait_standing("the copy on the clipboard lane's worker");
    type_key(&mut folio, "x", KeyCode::KeyX);
    assert_eq!(
        sent_to_the_shell(&mut folio, 0),
        "x",
        "the key typed while the copy is held reaches the shell"
    );
    hold.worker_hold().release();
    assert_eq!(
        written(&mut folio).len(),
        1,
        "the copy lands once the clipboard lets go"
    );
}

/// RED (plan row 52, R5) — **a picture save refused at its door is said on the next turn, never on
/// the paste's own**: with every place taken, the door returns with nothing drawn and wakes the
/// window as the saving thread does (`ClipboardPictureReady`); the next look draws the one notice
/// the door used to draw, in its kind and words. A save still running for an earlier paste lands
/// as it would have: the refusal mints no generation, so the earlier answer is still the one
/// waited for, and its path reaches the shell.
///
/// It takes every place of the process-global `CLIPBOARD_PICTURE_JOBS_RUNNING` for its whole length,
/// so it must stay the binary's only test that takes a place (review N5, 2026-11-04): a second test
/// that saves a picture would find the places gone, or leave this one short of four. The test-globals
/// guard does not see the global, which is written in the product's `ClipboardPictureJob::take`
/// through `fetch_update`, so `docs/plans/TEST-GLOBALS.tsv` holds no row for it (its note says why).
///
/// MUTATIONS: draw the notice in `Runtime::save_clipboard_picture` as before plan row 52 — a notice
/// on the paste's own turn; withdraw a generation on a refusal — the earlier save's path is dropped.
#[test]
fn a_picture_save_refused_at_its_door_is_said_on_the_next_turn() {
    let _places = staging_places();
    let mut folio = a_window_pasting("clipboard-picture-refused", "unused 未用");
    let events = crate::headless::LoopEvents::default();
    folio.app.event_proxy = events.proxy();
    let target = {
        let runtime = folio.runtime();
        let tab = &runtime.window.tabs[0];
        let (seat, pane) = tab.sessions.iter().next().expect("the shell");
        crate::PasteTarget {
            tab: tab.id,
            seat: *seat,
            incarnation: pane.session.incarnation,
        }
    };
    // Every place in the queue, held by the test and given back at its end.
    let places: Vec<crate::ClipboardPictureJob> =
        std::iter::from_fn(crate::ClipboardPictureJob::take).collect();
    assert_eq!(places.len(), crate::CLIPBOARD_PICTURE_JOBS);
    let earlier = folio.runtime().window.clipboard_picture.withdraw();
    let words = crate::i18n::Text::PasteClipboardPicture.text();
    folio
        .runtime()
        .save_clipboard_picture(target, Some(bt_platform::ClipboardSequence(41)), Vec::new())
        .expect("the door returns");
    assert_eq!(
        notices(&mut folio, words),
        0,
        "nothing is drawn on the paste's own turn"
    );
    assert!(
        matches!(
            events.take().as_slice(),
            [crate::AppEvent::ClipboardPictureReady]
        ),
        "the window is woken, once"
    );
    let inbox = folio.runtime().window.clipboard_picture.inbox();
    crate::runtime::deliver_clipboard_picture(
        &inbox,
        crate::ClipboardPictureAnswer {
            generation: earlier,
            target,
            staged: crate::StagingAnswer::Picture {
                sequence: None,
                result: Ok(std::path::PathBuf::from("planted-picture.png")),
            },
        },
    );
    folio
        .runtime()
        .adopt_clipboard_picture()
        .expect("the wake's look is answered");
    let said: Vec<crate::toast::ToastKind> = folio
        .runtime()
        .window
        .toasts
        .toasts()
        .iter()
        .filter(|toast| toast.body() == words)
        .map(|toast| toast.kind())
        .collect();
    assert_eq!(
        said,
        [crate::toast::ToastKind::Error],
        "one notice, the door's"
    );
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: picture (workers-busy); road=picture; sequence=41"],
        "and its one line, with the reason the door kept (plan row 81)"
    );
    assert!(
        sent_to_the_shell(&mut folio, 0).contains("planted-picture.png"),
        "the earlier save's answer lands: {:?}",
        sent_to_the_shell(&mut folio, 0)
    );
    drop(places);
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
            .is_some_and(|app| app.clipboard_lane.drain_mark().owed().is_none())
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
            .is_some_and(|app| app.clipboard_lane.drain_mark().owed().is_none())
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
            .is_some_and(|app| app.clipboard_lane.drain_mark().owed().is_none())
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

/// **The staging places are the process's** (`crate::CLIPBOARD_PICTURE_JOBS`): a test that takes
/// every one, and a test whose paste needs one, take this first, so neither reads the other's.
static STAGING_PLACES: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn staging_places() -> std::sync::MutexGuard<'static, ()> {
    STAGING_PLACES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The `paste refused:` lines the window said in `diagnostics.log` since the ledger was last
/// read, in order; the ledger is emptied.
fn refusal_lines(folio: &mut HeadlessFolio) -> Vec<String> {
    noted_lines(folio, "paste refused:")
}

/// The lines starting `prefix` the window said in `diagnostics.log` since the ledger was last
/// read; the ledger is emptied.
fn noted_lines(folio: &mut HeadlessFolio, prefix: &str) -> Vec<String> {
    folio
        .ledger
        .take()
        .into_iter()
        .filter_map(|effect| match effect {
            crate::headless::WindowEffect::Noted(line) if line.starts_with(prefix) => Some(line),
            _ => None,
        })
        .collect()
}

/// The address of the shell in the window's first tab.
fn the_shell(folio: &mut HeadlessFolio) -> crate::PasteTarget {
    let runtime = folio.runtime();
    let tab = &runtime.window.tabs[0];
    let (seat, pane) = tab.sessions.iter().next().expect("the shell");
    crate::PasteTarget {
        tab: tab.id,
        seat: *seat,
        incarnation: pane.session.incarnation,
    }
}

/// A path no shell can be handed: a control character in its name.
fn an_unspellable_path() -> std::path::PathBuf {
    std::path::PathBuf::from(format!("{}\u{7}秘密.txt", host_spelling(r"C:\w\bell")))
}

/// What the staging worker answered for this window, laid in its inbox as the worker lays it.
fn staged_answer(
    folio: &mut HeadlessFolio,
    target: crate::PasteTarget,
    staged: crate::StagingAnswer,
) {
    let generation = folio.runtime().window.clipboard_picture.withdraw();
    let inbox = folio.runtime().window.clipboard_picture.inbox();
    crate::runtime::deliver_clipboard_picture(
        &inbox,
        crate::ClipboardPictureAnswer {
            generation,
            target,
            staged,
        },
    );
    folio
        .runtime()
        .adopt_clipboard_picture()
        .expect("the wake's look is answered");
}

fn sequence(number: u64) -> Option<bt_platform::ClipboardSequence> {
    Some(bt_platform::ClipboardSequence(number))
}

/// RED (plan row 81, T-PASTE-REFUSAL-SAYS-WHY) — **a refused path says one line on every road,
/// with its kind, its road and the clipboard's sequence, and never the path**: two files copied,
/// one of them a name no shell can be handed — the other is pasted, the notice names the refused
/// one, and `diagnostics.log` has `paths (control, 1 of 2 paths); road=clipboard; sequence=<the
/// read's>`; the same path dropped, dragged, inserted from a files column (review N7), saved from
/// a picture and rendered from a promise says the same kind on its own road, the last two with
/// the sequence their read carried.
///
/// MUTATIONS: drop the `note` in `Runtime::refuse_paste` — no line; format the refused paths into
/// `PasteRefusal::refusal_words` — the name is in the line; pass `PasteFrom::Clipboard(None)` in
/// `apply_clipboard_payload` — `sequence=none`; toast `shell_literal::refusal_notice` in
/// `insert_path_into_terminal` again — no insert line.
#[test]
fn a_refused_path_says_one_line_on_every_road_and_never_the_path() {
    let mut folio = a_window_pasting("paste-refused-paths", "unused 未用");
    let good = std::path::PathBuf::from(host_spelling(r"C:\w\好.txt"));
    let read_at =
        folio
            .window
            .window
            .clipboard
            .holds_payload(bt_platform::ClipboardPayload::Files(vec![
                good,
                an_unspellable_path(),
            ]));
    folio.ledger.take();
    chord(&mut folio, "v", KeyCode::KeyV);
    folio.settle_clipboard();
    assert_eq!(
        refusal_lines(&mut folio),
        [format!(
            "paste refused: paths (control, 1 of 2 paths); road=clipboard; sequence={read_at}"
        )]
    );
    assert!(
        sent_to_the_shell(&mut folio, 0).contains("好.txt"),
        "the file that could be spelled is pasted"
    );
    let shown: Vec<String> = folio
        .runtime()
        .window
        .toasts
        .toasts()
        .iter()
        .map(|toast| toast.body().to_owned())
        .collect();
    assert!(
        shown.iter().any(|body| body.contains("秘密")),
        "the notice names the refused file, as it did: {shown:?}"
    );

    let target = the_shell(&mut folio);
    for (from, road) in [
        (crate::runtime::clipboard::PasteFrom::Drop, "drop"),
        (crate::runtime::clipboard::PasteFrom::Drag, "drag"),
    ] {
        folio
            .runtime()
            .paste_paths_into(target, vec![an_unspellable_path()], from)
            .expect("the paths are answered");
        assert_eq!(
            refusal_lines(&mut folio),
            [format!(
                "paste refused: paths (control, 1 of 1 paths); road={road}; sequence=none"
            )]
        );
    }
    folio
        .runtime()
        .insert_path_into_terminal(&an_unspellable_path())
        .expect("the insertion is answered");
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: paths (control, 1 of 1 paths); road=insert; sequence=none"]
    );
    staged_answer(
        &mut folio,
        target,
        crate::StagingAnswer::Picture {
            sequence: sequence(7),
            result: Ok(an_unspellable_path()),
        },
    );
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: paths (control, 1 of 1 paths); road=picture; sequence=7"]
    );
    staged_answer(
        &mut folio,
        target,
        crate::StagingAnswer::Promise {
            sequence: sequence(8),
            picture_passed_over: false,
            result: Ok(bt_platform::clipboard_promise::RenderedPromise {
                pasted: vec![an_unspellable_path()],
                files: 1,
                bytes: 3,
            }),
            took: std::time::Duration::from_millis(5),
        },
    );
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: paths (control, 1 of 1 paths); road=promise; sequence=8"]
    );
}

/// RED (plan row 81) — **a promise that is not pasted says why, in the line and in its notice**:
/// a promise the clipboard offered with a picture that did not render, unsupported where it was
/// read, says the picture could not be read; one whose source did not render an entry says the
/// result code on the promise road; one past the caps says it is too large; one refused at the
/// staging door says the door's reason on the next turn.
///
/// MUTATIONS: always `PasteClipboardPromise` in `RefusedKind::toast_text` — the picture-unread and
/// too-large notices are missing; the inbox counts instead of keeping reasons — the workers-busy
/// line is the picture's.
#[test]
fn a_promise_that_is_not_pasted_says_why() {
    let _places = staging_places();
    let mut folio = a_window_pasting("paste-refused-promise", "unused 未用");
    let events = crate::headless::LoopEvents::default();
    folio.app.event_proxy = events.proxy();
    let read_at =
        folio
            .window
            .window
            .clipboard
            .holds_payload(bt_platform::ClipboardPayload::Refused(
                bt_platform::UnsupportedKind::Promise {
                    picture_passed_over: true,
                },
            ));
    folio.ledger.take();
    chord(&mut folio, "v", KeyCode::KeyV);
    folio.settle_clipboard();
    assert_eq!(
        refusal_lines(&mut folio),
        [format!(
            "paste refused: promise (unsupported, picture passed over); road=clipboard; \
             sequence={read_at}"
        )]
    );
    let unread = crate::i18n::Text::PasteClipboardPictureUnread.text();
    assert_eq!(notices(&mut folio, unread), 1);

    let target = the_shell(&mut folio);
    staged_answer(
        &mut folio,
        target,
        crate::StagingAnswer::Promise {
            sequence: sequence(1756),
            picture_passed_over: false,
            result: Err(crate::runtime::PromiseWhy::Refused(
                bt_platform::clipboard_promise::PromiseRefused::NotRendered(Some(0x8004_01D3)),
            )),
            took: std::time::Duration::from_millis(5),
        },
    );
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: promise (not-rendered 0x800401D3); road=promise; sequence=1756"]
    );
    assert_eq!(
        notices(&mut folio, crate::i18n::Text::PasteClipboardPromise.text()),
        1
    );
    staged_answer(
        &mut folio,
        target,
        crate::StagingAnswer::Promise {
            sequence: sequence(1757),
            picture_passed_over: true,
            result: Err(crate::runtime::PromiseWhy::Refused(
                bt_platform::clipboard_promise::PromiseRefused::TooMany {
                    offered: 120,
                    cap: 64,
                },
            )),
            took: std::time::Duration::from_millis(5),
        },
    );
    assert_eq!(
        refusal_lines(&mut folio),
        [
            "paste refused: promise (too-many 120 of 64, picture passed over); road=promise; \
             sequence=1757"
        ]
    );
    assert_eq!(
        notices(
            &mut folio,
            crate::i18n::Text::PasteClipboardPromiseTooLarge.text()
        ),
        1,
        "too large is said as too large, whatever was passed over"
    );

    let places: Vec<crate::ClipboardPictureJob> =
        std::iter::from_fn(crate::ClipboardPictureJob::take).collect();
    folio
        .runtime()
        .stage_clipboard_promise(
            target,
            sequence(1758),
            bt_platform::PromisedFiles {
                entries: vec![bt_platform::PromisedEntry {
                    name: "a.txt".to_owned(),
                    size: Some(1),
                    folder: false,
                }],
                picture_passed_over: false,
            },
        )
        .expect("the door returns");
    assert_eq!(
        noted_lines(&mut folio, "clipboard promise workers"),
        ["clipboard promise workers are all busy; paste ignored"],
        "the door's own line, on the paste's turn"
    );
    folio
        .runtime()
        .adopt_clipboard_picture()
        .expect("the wake's look is answered");
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: promise (workers-busy); road=promise; sequence=1758"],
        "the refusal keeps its reason until it is said"
    );
    drop(places);
}

/// RED (plan row 81) — **a picture whose file was not written says `picture (not-saved)`** on
/// the picture road, with the sequence its read carried, beside the line that gives the reason.
///
/// MUTATION: say the picture's save failure with `self.toast` in `adopt_clipboard_picture` instead
/// of the refusal door — no line.
#[test]
fn a_picture_that_was_not_saved_says_so_once() {
    let mut folio = a_window_pasting("paste-refused-picture", "unused 未用");
    let target = the_shell(&mut folio);
    folio.ledger.take();
    staged_answer(
        &mut folio,
        target,
        crate::StagingAnswer::Picture {
            sequence: sequence(1763),
            result: Err("the file could not be written: disk full".to_owned()),
        },
    );
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: picture (not-saved); road=picture; sequence=1763"]
    );
    assert_eq!(
        notices(&mut folio, crate::i18n::Text::PasteClipboardPicture.text()),
        1
    );
}

/// RED (plan row 81, Part B) — **a promise on the clipboard is written and pasted as a copied file
/// is**: `Ctrl+V` over a clipboard that promises `report 报告.pdf`; the lane answers the promise
/// at once, a `clipboard-promise` worker has the source write it into a folder of the paste's own
/// in this window's staging folder, and the shell is sent its path spelled exactly as a copied
/// file of that path is — one line says the promise was saved, with counts and no name.
///
/// MUTATIONS: drop the promise answer in `adopt_clipboard_picture` — nothing is sent; spell the
/// promised path without `paste_paths_into` (its raw text) — the spelling differs.
#[test]
fn a_promise_on_the_clipboard_is_written_and_pasted_as_a_copied_file_is() {
    let _places = staging_places();
    let mut folio = a_window_pasting("paste-promise-honoured", "unused 未用");
    let events = crate::headless::LoopEvents::default();
    folio.app.event_proxy = events.proxy();
    let clipboard = folio.window.window.clipboard.clone();
    let staging = clipboard.staged_here();
    let _ = std::fs::remove_dir_all(&staging);
    let read_at = clipboard.holds_payload(bt_platform::ClipboardPayload::Promise(
        bt_platform::PromisedFiles {
            entries: vec![bt_platform::PromisedEntry {
                name: "report 报告.pdf".to_owned(),
                size: Some(9),
                folder: false,
            }],
            picture_passed_over: false,
        },
    ));
    clipboard.promise_renders(vec![Ok(b"%PDF-1.7\n".to_vec())]);
    folio.ledger.take();
    chord(&mut folio, "v", KeyCode::KeyV);
    folio.settle_clipboard();
    crate::lane::wait_until("the promise's worker answers", || {
        folio
            .runtime()
            .adopt_clipboard_picture()
            .expect("the wake's look is answered");
        !sent_to_the_shell(&mut folio, 0).is_empty()
    });
    let folders: Vec<std::path::PathBuf> = std::fs::read_dir(&staging)
        .expect("the staging folder was made")
        .flatten()
        .map(|entry| entry.path())
        .collect();
    assert_eq!(
        folders.len(),
        1,
        "one folder for the one paste: {folders:?}"
    );
    let written = folders[0].join("report 报告.pdf");
    assert_eq!(
        std::fs::read(&written).expect("the file was written"),
        b"%PDF-1.7\n"
    );
    let (recipient, leading_space) = {
        let runtime = folio.runtime();
        let pane = runtime.window.tabs[0]
            .sessions
            .values()
            .next()
            .expect("the shell");
        (
            pane.session.launch.paste_recipient.clone(),
            crate::runtime::input_line_needs_a_space_first(&pane.session.session),
        )
    };
    let as_a_copied_file = crate::runtime::prepare_clipboard_paste(
        bt_platform::ClipboardPayload::Files(vec![written]),
        &recipient,
        leading_space,
    );
    assert_eq!(
        Some(sent_to_the_shell(&mut folio, 0)),
        as_a_copied_file.text,
        "the promised file is spelled as a copied file of its path"
    );
    let effects = folio.ledger.take();
    let noted: Vec<&String> = effects
        .iter()
        .filter_map(|effect| match effect {
            crate::headless::WindowEffect::Noted(line) => Some(line),
            _ => None,
        })
        .collect();
    let saved: Vec<&&String> = noted
        .iter()
        .filter(|line| line.starts_with("clipboard promise saved: 1 files, 9 bytes, "))
        .collect();
    assert_eq!(saved.len(), 1, "{noted:?}");
    assert!(
        saved[0].ends_with(&format!("; sequence={read_at}")) && !saved[0].contains("report"),
        "{saved:?}"
    );
    assert!(
        !noted.iter().any(|line| line.starts_with("paste refused:")),
        "{noted:?}"
    );
    let _ = std::fs::remove_dir_all(&staging);
}

/// RED (plan row 81) — **a panic in the promise's render is the paste's failure, not the
/// process's end**: the worker contains it, removes the paste's folder and answers
/// `not-rendered`, which is said on the next turn as any refusal is.
///
/// MUTATION: render without `contained_panic::contain` in `clipboard_picture::stage_promise` —
/// the render thread dies with its answer owed, and the watcher answers `not-rendered` without
/// removing the folder.
#[test]
fn a_promise_whose_render_panics_is_refused_and_leaves_nothing() {
    let _places = staging_places();
    let mut folio = a_window_pasting("paste-promise-panics", "unused 未用");
    let events = crate::headless::LoopEvents::default();
    folio.app.event_proxy = events.proxy();
    let clipboard = folio.window.window.clipboard.clone();
    let staging = clipboard.staged_here();
    let _ = std::fs::remove_dir_all(&staging);
    let read_at = clipboard.holds_payload(bt_platform::ClipboardPayload::Promise(
        bt_platform::PromisedFiles {
            entries: vec![bt_platform::PromisedEntry {
                name: "photo.jpg".to_owned(),
                size: Some(3),
                folder: false,
            }],
            picture_passed_over: false,
        },
    ));
    clipboard.promise_renders(vec![Ok(b"jpg".to_vec())]);
    clipboard.promise_render_panics();
    folio.ledger.take();
    chord(&mut folio, "v", KeyCode::KeyV);
    folio.settle_clipboard();
    let mut lines = Vec::new();
    crate::lane::wait_until("the promise's worker answers its panic", || {
        folio
            .runtime()
            .adopt_clipboard_picture()
            .expect("the wake's look is answered");
        lines.extend(refusal_lines(&mut folio));
        !lines.is_empty()
    });
    assert_eq!(
        lines,
        [format!(
            "paste refused: promise (not-rendered); road=promise; sequence={read_at}"
        )]
    );
    let left: Vec<std::path::PathBuf> = std::fs::read_dir(&staging)
        .expect("the staging folder was made")
        .flatten()
        .map(|entry| entry.path())
        .collect();
    assert!(
        left.is_empty(),
        "the paste's folder went with the panic: {left:?}"
    );
    assert!(
        sent_to_the_shell(&mut folio, 0).is_empty(),
        "nothing was pasted"
    );
    let _ = std::fs::remove_dir_all(&staging);
}

/// PIN (plan row 81) — **every refusal line is spelled from its value**: each kind, each detail
/// and each road, in the line's one shape, with words and counts only.
#[test]
fn every_refusal_line_is_spelled_from_its_value() {
    use super::{PasteBusy, PasteFrom, PasteRefusal, PictureNotSaved, PromiseWhy, RefusedKind};
    use bt_platform::clipboard_promise::PromiseRefused;
    let line = |kind: RefusedKind, from: PasteFrom| PasteRefusal { kind, from }.refusal_words();
    let at = PasteFrom::Clipboard(sequence(1720));
    assert_eq!(
        line(
            RefusedKind::Paths {
                refused: vec![
                    (
                        an_unspellable_path(),
                        crate::shell_literal::Refusal::PowerShellQuote
                    ),
                    (
                        an_unspellable_path(),
                        crate::shell_literal::Refusal::Control
                    ),
                    (
                        an_unspellable_path(),
                        crate::shell_literal::Refusal::PowerShellQuote
                    ),
                ],
                of: 4,
            },
            at
        ),
        "paste refused: paths (powershell-quote, control, 3 of 4 paths); road=clipboard; \
         sequence=1720"
    );
    for (kind, words) in [
        (
            RefusedKind::Promise {
                why: PromiseWhy::Unsupported,
                picture_passed_over: false,
            },
            "promise (unsupported)",
        ),
        (
            RefusedKind::Promise {
                why: PromiseWhy::Refused(PromiseRefused::WriteFailed(
                    std::io::ErrorKind::StorageFull,
                )),
                picture_passed_over: false,
            },
            "promise (write-failed: StorageFull)",
        ),
        (
            RefusedKind::Promise {
                why: PromiseWhy::NotStarted,
                picture_passed_over: true,
            },
            "promise (not-started, picture passed over)",
        ),
        (
            RefusedKind::Promise {
                why: PromiseWhy::Refused(PromiseRefused::Stalled),
                picture_passed_over: false,
            },
            "promise (stalled)",
        ),
        (
            RefusedKind::ReadFailed { worker_gone: false },
            "read-failed",
        ),
        (
            RefusedKind::ReadFailed { worker_gone: true },
            "read-failed (worker-gone)",
        ),
        (RefusedKind::Busy(PasteBusy::DeskFull), "busy (desk-full)"),
        (RefusedKind::Busy(PasteBusy::LaneFull), "busy (lane-full)"),
        (RefusedKind::Busy(PasteBusy::LaneGone), "busy (lane-gone)"),
        (
            RefusedKind::Busy(PasteBusy::NotStarted),
            "busy (not-started)",
        ),
        (RefusedKind::Busy(PasteBusy::Deadline), "busy (deadline)"),
        (
            RefusedKind::Busy(PasteBusy::HeldOverTheCap),
            "busy (held-over-the-cap)",
        ),
        (
            RefusedKind::PictureNotSaved(PictureNotSaved::WorkersBusy),
            "picture (workers-busy)",
        ),
        (
            RefusedKind::PictureNotSaved(PictureNotSaved::NotStarted),
            "picture (not-started)",
        ),
        (
            RefusedKind::PictureNotSaved(PictureNotSaved::SaveFailed),
            "picture (not-saved)",
        ),
    ] {
        assert_eq!(
            line(kind, at),
            format!("paste refused: {words}; road=clipboard; sequence=1720")
        );
    }
    for (from, tail) in [
        (PasteFrom::Clipboard(None), "road=clipboard; sequence=none"),
        (PasteFrom::Picture(sequence(3)), "road=picture; sequence=3"),
        (PasteFrom::Promise(sequence(4)), "road=promise; sequence=4"),
        (PasteFrom::Drop, "road=drop; sequence=none"),
        (PasteFrom::Drag, "road=drag; sequence=none"),
        (PasteFrom::Insert, "road=insert; sequence=none"),
    ] {
        assert_eq!(
            line(RefusedKind::ReadFailed { worker_gone: false }, from),
            format!("paste refused: read-failed; {tail}")
        );
    }
}

/// PIN (plan row 81) — **every paste refusal leaves by one door**: `Runtime::refuse_paste` is
/// called from the five places a paste is refused — the delivery of a prepared paste, the spend
/// of a failed read, the busy notice, the staging worker's answer and a files column's inserted
/// path — and none of them, nor the staging doors, raises a notice of its own; the paste notices
/// are named by `RefusedKind::toast_text` alone, the refused paths' (`refusal_notice`) as well
/// as the clipboard's (review N7); and the picture door's lines reach `diagnostics.log`.
///
/// MUTATIONS: put `self.toast(.., PasteClipboardRead ..)` back in `spend_paste` — its body raises
/// a notice; `crate::diagnostics::stderr_line` back in `save_clipboard_picture` — the line is
/// the console's only; toast `shell_literal::refusal_notice` in `insert_path_into_terminal` —
/// a second reader of the path notices.
#[test]
fn every_paste_refusal_leaves_by_one_door() {
    use crate::test_support::{calls_of, found, method_body, reader_names, source};
    use bt_source::{Pattern, View, needle};
    assert_eq!(
        reader_names(&calls_of("Runtime", "refuse_paste")),
        [
            "adopt_clipboard_picture",
            "deliver_paste",
            "insert_path_into_terminal",
            "say_the_clipboard_busy",
            "spend_paste",
        ]
    );
    for road in [
        "deliver_paste",
        "spend_paste",
        "say_the_clipboard_busy",
        "adopt_clipboard_picture",
        "save_clipboard_picture",
        "stage_clipboard_promise",
        "insert_path_into_terminal",
    ] {
        let body = method_body("Runtime", road);
        assert!(
            !body.contains("self.toast("),
            "{road} raises no notice of its own:\n{body}"
        );
        assert!(
            !body.contains("stderr_line"),
            "{road}'s lines reach diagnostics.log:\n{body}"
        );
    }
    for text in [
        "PasteClipboardPromise",
        "PasteClipboardRead",
        "PasteClipboardBusy",
        "PasteClipboardPicture",
        "PasteClipboardPictureUnread",
        "PasteClipboardPromiseTooLarge",
    ] {
        let named =
            found(needle!(Pattern::identifier(text)), View::Identifiers).in_the_product(source());
        assert_eq!(
            reader_names(&named),
            ["on", "toast_text"],
            "{text} is said only through the refusal door's notice:\n{}",
            named.report(source())
        );
    }
    let path_notices =
        crate::test_support::free_calls_of("refusal_notice").in_the_product(source());
    assert_eq!(
        reader_names(&path_notices),
        ["toast_text"],
        "the refused paths' notice is raised only by the refusal door:\n{}",
        path_notices.report(source())
    );
}

/// PIN (plan row 81; review N16) — **the promise is rendered on its own worker and nowhere
/// else**: `ClipboardDoor::render_promised_files` is called by `clipboard_picture::stage_promise`
/// alone, which only the `clipboard-promise` worker's body in `Runtime::stage_clipboard_promise`
/// calls; the clipboard lane's worker (`PlatformClipboard::serve`) names no render; and the
/// platform's render behind the door, `bt_platform::render_clipboard_promise`, is called by the
/// door's `render_promised_files` alone (re-review F4), so no other product function renders a
/// promise on a worker of its own. R1's "never the lane worker" is a guard, not a fact of today's
/// callers.
///
/// MUTATIONS: call `render_promised_files` from another product function — a second caller;
/// call `stage_promise` from the window thread's adopt — a second reader; render inside `serve`
/// — its body names a render; call `bt_platform::render_clipboard_promise` from another product
/// function — a second reader of the platform render.
#[test]
fn the_promise_is_rendered_on_its_own_worker_only() {
    use crate::test_support::{found, free_calls_of, item_body, reader_names, source};
    use bt_source::{ItemQuery, Pattern, View, needle};
    let rendered = found(
        needle!(Pattern::call("render_promised_files")),
        View::Identifiers,
    )
    .in_the_product(source());
    // The trait's own default and its one product implementation declare the name; neither calls
    // a render of another door.
    let callers: Vec<String> = reader_names(&rendered)
        .into_iter()
        .filter(|name| name != "render_promised_files")
        .collect();
    assert_eq!(
        callers,
        ["stage_promise"],
        "the render's callers:\n{}",
        rendered.report(source())
    );
    let staged = free_calls_of("stage_promise").in_the_product(source());
    assert_eq!(
        reader_names(&staged),
        ["stage_clipboard_promise"],
        "the promise's worker is started from one door:\n{}",
        staged.report(source())
    );
    let serve =
        item_body(&ItemQuery::method("PlatformClipboard", "serve").of_trait("ClipboardDoor"));
    assert!(
        !serve.contains("render_"),
        "the clipboard lane's worker renders nothing:\n{serve}"
    );
    let platform = free_calls_of("render_clipboard_promise").in_the_product(source());
    assert_eq!(
        reader_names(&platform),
        ["render_promised_files"],
        "the platform's render is reached through the door alone:\n{}",
        platform.report(source())
    );
}

/// RED (plan row 81; review N15) — **a paste asked of a lane whose worker has ended says
/// `busy (lane-gone)`**: the lane's worker ends with a paste owed (said `read-failed
/// (worker-gone)`), and the next `Ctrl+V` is refused at the lane's door, synchronously — one line
/// and the busy notice, nothing owed, no input held.
///
/// MUTATION: map `Declined::LaneGone` to `PasteBusy::LaneFull` in `Runtime::paste_clipboard` —
/// the line says `lane-full`.
#[test]
fn a_paste_after_the_lanes_worker_ended_says_lane_gone() {
    let mut folio = a_window_pasting("paste-refused-lane-gone", "粘贴 never");
    let gate = std::sync::Arc::new(crate::lane::Gate::default());
    let lane = recording_lane(
        &mut folio,
        &crate::lane::Executor::scripted(std::sync::Arc::clone(&gate)),
    );
    gate.hold();
    chord(&mut folio, "v", KeyCode::KeyV);
    assert!(gate.wait_entered(1), "the paste is on the worker");
    gate.kill();
    gate.release();
    crate::lane::wait_for_a_wake(&lane.wakes, "the worker's end");
    folio.settle_clipboard();
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: read-failed (worker-gone); road=clipboard; sequence=none"]
    );
    chord(&mut folio, "v", KeyCode::KeyV);
    assert!(
        folio.runtime().window.clipboard.pastes.is_empty(),
        "refused at the door"
    );
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: busy (lane-gone); road=clipboard; sequence=none"]
    );
    assert_eq!(
        notices(&mut folio, crate::i18n::Text::PasteClipboardBusy.text()),
        1
    );
    type_key(&mut folio, "y", KeyCode::KeyY);
    assert_eq!(sent_to_the_shell(&mut folio, 0), "y", "no input is held");
}

/// RED (plan row 81; review N15) — **a paste whose lane worker would not start says
/// `busy (not-started)`**: the thread door refuses the lane's first start; the `Ctrl+V` that asked
/// for it is refused at the lane's door with one line and the busy notice, and the next one starts
/// the worker again and is read.
///
/// MUTATION: map `Declined::NotStarted` to `PasteBusy::LaneGone` in `Runtime::paste_clipboard` —
/// the line says `lane-gone`.
#[test]
fn a_paste_whose_lane_would_not_start_says_not_started() {
    let mut folio = a_window_pasting("paste-refused-not-started", "粘贴 once");
    let gate = std::sync::Arc::new(crate::lane::Gate::default());
    let lane = recording_lane(
        &mut folio,
        &crate::lane::Executor::scripted(std::sync::Arc::clone(&gate)),
    );
    gate.refuse_next_start();
    chord(&mut folio, "v", KeyCode::KeyV);
    assert!(
        folio.runtime().window.clipboard.pastes.is_empty(),
        "refused at the door"
    );
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: busy (not-started); road=clipboard; sequence=none"]
    );
    assert_eq!(
        notices(&mut folio, crate::i18n::Text::PasteClipboardBusy.text()),
        1
    );
    chord(&mut folio, "v", KeyCode::KeyV);
    crate::lane::wait_for_a_wake(&lane.wakes, "the second paste's answer");
    folio.settle_clipboard();
    assert!(
        sent_to_the_shell(&mut folio, 0).contains("粘贴 once"),
        "the next paste starts the worker and is read"
    );
}

/// RED (plan row 81; review N15) — **a picture or a promise whose worker would not start says
/// `not-started` on its own road**: the thread door refuses the staging worker's start; the door
/// says its own line on the paste's turn and the refusal on the next, with the sequence the read
/// carried, and the place it took is back.
///
/// MUTATIONS: answer `PictureNotSaved::WorkersBusy` for the refused start in
/// `Runtime::save_clipboard_picture` — the picture's line says `workers-busy`; answer
/// `PromiseWhy::WorkersBusy` in `Runtime::stage_clipboard_promise` — the promise's does.
#[test]
fn a_staging_worker_that_would_not_start_says_not_started() {
    let _places = staging_places();
    let mut folio = a_window_pasting("paste-refused-staging-start", "unused 未用");
    let events = crate::headless::LoopEvents::default();
    folio.app.event_proxy = events.proxy();
    let target = the_shell(&mut folio);
    folio.ledger.take();
    folio
        .runtime()
        .window
        .clipboard_picture
        .refuse_the_next_start();
    folio
        .runtime()
        .save_clipboard_picture(target, sequence(42), Vec::new())
        .expect("the door returns");
    assert_eq!(
        noted_lines(&mut folio, "clipboard picture worker"),
        ["clipboard picture worker could not be started; paste ignored"]
    );
    folio
        .runtime()
        .adopt_clipboard_picture()
        .expect("the wake's look is answered");
    assert_eq!(
        refusal_lines(&mut folio),
        ["paste refused: picture (not-started); road=picture; sequence=42"]
    );
    assert_eq!(
        notices(&mut folio, crate::i18n::Text::PasteClipboardPicture.text()),
        1
    );

    folio
        .runtime()
        .window
        .clipboard_picture
        .refuse_the_next_start();
    folio
        .runtime()
        .stage_clipboard_promise(
            target,
            sequence(43),
            bt_platform::PromisedFiles {
                entries: vec![bt_platform::PromisedEntry {
                    name: "a.txt".to_owned(),
                    size: Some(1),
                    folder: false,
                }],
                picture_passed_over: true,
            },
        )
        .expect("the door returns");
    assert_eq!(
        noted_lines(&mut folio, "clipboard promise worker"),
        ["clipboard promise worker could not be started; paste ignored"]
    );
    folio
        .runtime()
        .adopt_clipboard_picture()
        .expect("the wake's look is answered");
    assert_eq!(
        refusal_lines(&mut folio),
        [
            "paste refused: promise (not-started, picture passed over); road=promise; \
             sequence=43"
        ]
    );
    assert_eq!(
        notices(
            &mut folio,
            crate::i18n::Text::PasteClipboardPictureUnread.text()
        ),
        1,
        "beside a picture that did not render, the picture's notice"
    );
    let places: Vec<crate::ClipboardPictureJob> =
        std::iter::from_fn(crate::ClipboardPictureJob::take).collect();
    assert_eq!(
        places.len(),
        crate::CLIPBOARD_PICTURE_JOBS,
        "every place is back"
    );
}

/// RED (plan row 81; review N14, N15) — **a promise whose source stalls is given up, says
/// `promise (stalled)`, and leaves nothing behind**: `Ctrl+V` over a promise whose source hands
/// nothing over; once the door's stall bound has passed with no step, the worker abandons the
/// render, removes the paste's folder and its marker, gives its place back, and the paste is
/// refused with one line and the promise's notice — while the source is still standing. When it
/// is let go it writes nothing.
///
/// MUTATIONS: never give up in `clipboard_picture::watch_the_render` — no line (the wait runs
/// out); keep the place until after the answer in `Runtime::stage_clipboard_promise` — a place is
/// still taken when the line is read; skip the folder's removal on `Stalled` in `stage_promise` —
/// the folder is left.
#[test]
fn a_promise_whose_source_stalls_is_given_up_and_leaves_nothing() {
    let _places = staging_places();
    let mut folio = a_window_pasting("paste-promise-stalls", "unused 未用");
    let events = crate::headless::LoopEvents::default();
    folio.app.event_proxy = events.proxy();
    let clipboard = folio.window.window.clipboard.clone();
    let staging = clipboard.staged_here();
    let _ = std::fs::remove_dir_all(&staging);
    let read_at = clipboard.holds_payload(bt_platform::ClipboardPayload::Promise(
        bt_platform::PromisedFiles {
            entries: vec![bt_platform::PromisedEntry {
                name: "slow.bin".to_owned(),
                size: Some(4),
                folder: false,
            }],
            picture_passed_over: false,
        },
    ));
    clipboard.promise_renders(vec![Ok(b"slow".to_vec())]);
    let stall = clipboard.promise_render_stalls(std::time::Duration::from_millis(200));
    folio.ledger.take();
    chord(&mut folio, "v", KeyCode::KeyV);
    folio.settle_clipboard();
    let mut lines = Vec::new();
    crate::lane::wait_until("the promise's worker gives the render up", || {
        folio
            .runtime()
            .adopt_clipboard_picture()
            .expect("the wake's look is answered");
        lines.extend(refusal_lines(&mut folio));
        !lines.is_empty()
    });
    assert_eq!(
        lines,
        [format!(
            "paste refused: promise (stalled); road=promise; sequence={read_at}"
        )]
    );
    assert_eq!(
        notices(&mut folio, crate::i18n::Text::PasteClipboardPromise.text()),
        1
    );
    let places: Vec<crate::ClipboardPictureJob> =
        std::iter::from_fn(crate::ClipboardPictureJob::take).collect();
    assert_eq!(
        places.len(),
        crate::CLIPBOARD_PICTURE_JOBS,
        "the stalled render holds no place"
    );
    drop(places);
    let left = |staging: &std::path::Path| -> Vec<std::path::PathBuf> {
        std::fs::read_dir(staging)
            .expect("the staging folder was made")
            .flatten()
            .map(|entry| entry.path())
            .collect()
    };
    assert!(
        left(&staging).is_empty(),
        "the paste's folder and its marker went: {:?}",
        left(&staging)
    );
    stall.release();
    crate::lane::wait_until("the let-go source is refused its write", || {
        clipboard.promise_renders_ended() == 1
    });
    assert!(
        left(&staging).is_empty(),
        "the source let go writes nothing: {:?}",
        left(&staging)
    );
    assert!(
        sent_to_the_shell(&mut folio, 0).is_empty(),
        "nothing was pasted"
    );
    let _ = std::fs::remove_dir_all(&staging);
}
