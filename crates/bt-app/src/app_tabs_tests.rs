//! **The crate root: tabs and rename.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{
    A, NO_MODIFIERS, PtyPresentationHarness, SHOT_PATH, TAB_ONE, at, breathing, buffer_saying,
    calls_of, cross_metrics, cross_move, cross_seats, cross_solve, cross_tab, dir_entry,
    document_on, edited_buffer, free_fn_body, item_body, item_declaration, leaf_saying, leaf_says,
    listed, method_body, pictures_drawn, press, reader_names, saved_files_and_terminal, seat_of,
    settled, squeezed, strip_with_cli_tab, tab_texts, tab_with_a_files_column, tab_with_a_picture,
    tab_with_a_preview,
};
use bt_source::ItemQuery;
use std::time::Duration;
use winit::keyboard::{Key, NamedKey};

/// **This host's command modifier** — Control, or Command on a Mac — read off
/// [`input::is_command_chord`], which is the modifier `rename_key` opens its
/// verbs on. The pins below that say `Ctrl` press this.
fn command_modifier() -> ModifiersState {
    [ModifiersState::CONTROL, ModifiersState::SUPER]
        .into_iter()
        .find(|modifiers| input::is_command_chord(*modifiers))
        .expect("one of the two is the host's command modifier")
}

/// The braces one type's members are written in, and what is between them.
fn type_body(name: &str) -> &'static str {
    item_body(&ItemQuery::type_item(name))
}

const B: TabId = TabId(2);

/// J105 (mock-up 5743-5765) — a press chooses a tab; the switch lands 180ms
/// later.
///
/// Red gate: `chrome_mouse_input` activated on the press itself
/// (`ChromeTarget::Tab(index) => self.activate_tab(index, false)`), so there
/// was no interval in which the tab was chosen but not yet shown — which is
/// the entire mechanism. The three assertions are the three instants: at the
/// press, one tick short of the deadline, and at it.
#[test]
fn a_press_chooses_a_tab_and_the_view_follows_a_hundred_and_eighty_milliseconds_later() {
    let now = Instant::now();
    let mut press = TabPress::armed(A, at(100.0, 20.0), now);

    assert_eq!(press.promise, TabPressPromise::Pending, "chosen, not shown");
    assert!(
        !press.matured(now + Duration::from_millis(179)),
        "179ms is still inside the grace period"
    );
    assert!(
        press.matured(now + TAB_PRESS_ACTIVATION_GRACE),
        "at 180ms the press has waited long enough to be believed"
    );
    assert_eq!(press.promise, TabPressPromise::Paid);
    assert!(
        !press.matured(now + Duration::from_secs(1)),
        "and it is paid once, not once per wake-up"
    );
}

/// J105's other half — "松开时不足 180ms → 立即激活(点击就是点击)".
///
/// The release pays on the tab it pressed and nowhere else, which is the
/// mock-up's `click` handler (5735) stated in geometry: `click` fires on the
/// element the press and the release share.
#[test]
fn letting_go_on_the_tab_you_pressed_is_a_click_and_shows_it_at_once() {
    let now = Instant::now();

    let mut quick = TabPress::armed(A, at(100.0, 20.0), now);
    assert!(
        quick.released_over(Some(A)),
        "a click well inside the grace period is still a click"
    );
    assert_eq!(quick.promise, TabPressPromise::Paid);

    let mut elsewhere = TabPress::armed(A, at(100.0, 20.0), now);
    assert!(
        !elsewhere.released_over(Some(B)),
        "lifting on a different tab is not a click on this one"
    );
    assert!(
        !elsewhere.released_over(None),
        "and lifting off the strip is not a click at all"
    );

    let mut already = TabPress::armed(A, at(100.0, 20.0), now);
    assert!(already.matured(now + TAB_PRESS_ACTIVATION_GRACE));
    assert!(
        !already.released_over(Some(A)),
        "a promise is paid once — the release must not switch a second time"
    );
}

/// J105 — "位移超过拖拽阈值(6px)之前,不切换内容 ... 快速拖走时不会闪一下".
///
/// The threshold is `startDrag`'s own 6 logical pixels (mock-up 6727) and it
/// is *logical*: the same hand movement on a 200% display crosses twice the
/// physical pixels and is the same movement.
#[test]
fn travelling_past_six_pixels_abandons_the_delayed_switch() {
    let now = Instant::now();
    for scale in [1.0, 1.5, 2.0] {
        let mut press = TabPress::armed(A, at(100.0, 20.0), now);
        assert!(
            !press.travelled(at(100.0 + 5.9 * scale, 20.0), scale),
            "just under the threshold at {scale}x is still a press"
        );
        assert_eq!(press.promise, TabPressPromise::Pending);
        assert!(
            press.travelled(at(100.0 + 6.0 * scale, 20.0), scale),
            "at the threshold the press becomes a drag at {scale}x"
        );
        assert_eq!(
            press.promise,
            TabPressPromise::Slipped,
            "the state T5 hangs its drag on"
        );
        assert!(
            !press.matured(now + TAB_PRESS_ACTIVATION_GRACE),
            "and the timer that is still running must find nothing to do"
        );
        assert_eq!(press.wake_deadline(), None, "nor ask to be woken for it");
    }
}

/// J105/J108 — the promise a slipped press still carries.
///
/// Two facts T5 needs and this slice must not get wrong: travelling after
/// the switch has already landed does not take it back, and a slipped press
/// that comes home still pays (the mock-up's release path re-selects on
/// exactly this condition, 6888 and 7134).
#[test]
fn a_slipped_press_keeps_its_promise_and_a_paid_one_cannot_be_unpaid() {
    let now = Instant::now();

    let mut after_paying = TabPress::armed(A, at(100.0, 20.0), now);
    assert!(after_paying.matured(now + TAB_PRESS_ACTIVATION_GRACE));
    assert!(
        after_paying.travelled(at(400.0, 20.0), 1.0),
        "a finger held past the grace period is still a finger that can carry the tab"
    );
    assert_eq!(
        after_paying.promise,
        TabPressPromise::Paid,
        "dragging a tab you are already looking at does not un-choose it"
    );

    let mut came_home = TabPress::armed(A, at(100.0, 20.0), now);
    assert!(came_home.travelled(at(120.0, 20.0), 1.0));
    assert!(
        came_home.released_over(Some(A)),
        "down and up on one tab is a click however the pointer wandered between"
    );
    assert!(
        !TabPress::armed(A, at(100.0, 20.0), now).released_over(Some(B)),
        "and the promise stays unpaid when the release lands elsewhere"
    );
}

/// J105 — "在已激活的 tab 上按下" owes nothing, and must not be able to
/// start owing something later (mock-up 5755: `wsId !== state.active`).
#[test]
fn pressing_the_tab_you_are_already_on_promises_nothing() {
    let now = Instant::now();
    let mut press = TabPress::settled(A, at(100.0, 20.0), now);
    assert_eq!(press.promise, TabPressPromise::Paid);
    assert_eq!(press.wake_deadline(), None, "no timer is armed");
    assert!(!press.matured(now + TAB_PRESS_ACTIVATION_GRACE));
    assert!(!press.released_over(Some(A)));
}

/// The bug T5 shipped with, off real hardware: with two tabs open, the tab
/// you were looking at could not be dragged *at all*. It read as
/// "sometimes" because which tab that is changes as you work — and the tab
/// a hand reaches for is very often the one already in front of it.
///
/// Red gate: `travelled` refused any press whose promise was not `Pending`,
/// and a press onto the active tab is born `Paid` (`TabPress::settled`), so
/// it never reported the move a drag begins on, at any distance.
#[test]
fn the_tab_you_are_already_on_is_still_draggable() {
    let now = Instant::now();
    for scale in [1.0, 1.5, 2.0] {
        let mut press = TabPress::settled(A, at(100.0, 20.0), now);
        assert!(
            !press.travelled(at(100.0 + 5.9 * scale, 20.0), scale),
            "the active tab is held to the same 6px as every other tab"
        );
        assert!(
            press.travelled(at(100.0 + 7.0 * scale, 20.0), scale),
            "and past it the press is a drag at {scale}x, owing nothing or not"
        );
        assert_eq!(
            press.promise,
            TabPressPromise::Paid,
            "the drag neither borrows from the promise nor pays it back"
        );
        assert!(
            !press.travelled(at(400.0, 20.0), scale),
            "and it still starts exactly once"
        );
    }
}

/// J99/K111 — the second press of a would-be rename lands on the tab the
/// first click just activated, so it too owes nothing. Holding it and
/// moving carries the tab; it does not sit inside the double-click window
/// waiting for an editor.
#[test]
fn pressing_again_inside_the_double_click_window_and_moving_is_a_drag() {
    let now = Instant::now();
    let mut clicks = TabClicks::default();

    let mut first = TabPress::armed(A, at(100.0, 20.0), now);
    assert!(first.released_over(Some(A)), "click one shows the tab");
    assert_eq!(clicks.register(A, now), TabClick::Single);

    // Press two, well inside the window — and now onto the active tab.
    let again = now + Duration::from_millis(80);
    let mut second = TabPress::settled(A, at(100.0, 20.0), again);
    assert!(
        second.travelled(at(106.0, 20.0), 1.0),
        "a press-and-move inside the double-click window is a drag, not half a rename"
    );

    // And the gesture takes its own release with it: `drop_tab_drag`
    // interrupts the chain, so the lift that ends the drag cannot land as
    // the second click and open the editor behind the drop.
    clicks.interrupt();
    assert_eq!(
        clicks.register(A, again + Duration::from_millis(10)),
        TabClick::Single,
        "a click that became a drag is not the first half of anything"
    );
}

/// J99/J105 — the two clicks of a rename land on one tab inside the
/// system's own double-click window, and nothing else pairs with them.
#[test]
fn two_presses_on_one_tab_inside_the_double_click_window_are_a_double_click() {
    let now = Instant::now();
    let mut clicks = TabClicks::default();

    assert_eq!(clicks.register(A, now), TabClick::Single);
    assert_eq!(
        clicks.register(A, now + MULTI_CLICK_INTERVAL),
        TabClick::Double,
        "the far edge of the window still pairs"
    );
    assert_eq!(
        clicks.register(A, now + MULTI_CLICK_INTERVAL + Duration::from_millis(1)),
        TabClick::Single,
        "a double click consumes its own history — a third press starts over"
    );

    let mut slow = TabClicks::default();
    assert_eq!(slow.register(A, now), TabClick::Single);
    assert_eq!(
        slow.register(A, now + MULTI_CLICK_INTERVAL + Duration::from_millis(1)),
        TabClick::Single,
        "past the window it is two single clicks"
    );

    let mut wandering = TabClicks::default();
    assert_eq!(wandering.register(A, now), TabClick::Single);
    assert_eq!(
        wandering.register(B, now + Duration::from_millis(10)),
        TabClick::Single,
        "two tabs are two elements, and `dblclick` needs one"
    );

    // J99: "`.close`/`.pin` 上的双击不算(那是两次按钮点击)" — the button
    // press never registers, and it breaks the chain on its way past.
    let mut interrupted = TabClicks::default();
    assert_eq!(interrupted.register(A, now), TabClick::Single);
    interrupted.interrupt();
    assert_eq!(
        interrupted.register(A, now + Duration::from_millis(10)),
        TabClick::Single,
        "a click on the × between them is not the first half of anything"
    );
}

/// The same, with something on the clipboard, answering what a copy or a cut
/// asked to put there.
fn press_with_clipboard(
    editor: &mut TabRename,
    key: &Key,
    modifiers: ModifiersState,
    holding: &str,
) -> (RenameVerdict, Option<String>) {
    let mut clipboard = RenameClipboard {
        paste: holding,
        copied: None,
    };
    let verdict = rename_key(editor, key, modifiers, &mut clipboard);
    (verdict, clipboard.copied)
}

/// PIN (user ruling 2026-08-19) — **a rename moves the file, refuses a
/// collision, and never overwrites.**
///
/// `std::fs::rename` is `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` on
/// this platform, so a rename onto an existing name would eat it silently.
/// This walks the real filesystem because that is the only place that claim
/// is true or false.
///
/// Red gate: drop the identity question in `rename_would_replace_another_entry`
/// and the first half of this test finds one file where it left two.
#[test]
fn a_rename_never_eats_the_file_that_already_has_the_name() {
    let directory = bt_testpath::temp_path("folio-rename-pin");
    std::fs::create_dir_all(&directory).expect("a temp folder");
    let from = directory.join("notes.md");
    let taken = directory.join("taken.md");
    std::fs::write(&from, "one").expect("write the file being renamed");
    std::fs::write(&taken, "two").expect("write the file in the way");

    // The door both rename surfaces go through, asked for real: a destination
    // that exists and is a different entry is refused and nothing moves.
    let onto_taken = directory.join("taken.md");
    assert!(
        rename_would_replace_another_entry(&from, &onto_taken),
        "a name another file holds is refused"
    );
    assert_eq!(
        std::fs::read_to_string(&taken).expect("the file in the way is still there"),
        "two",
        "and nothing has moved onto it"
    );

    // A name nothing else has moves the file and leaves no copy behind.
    let onto_free = directory.join("todo.md");
    assert!(!onto_free.exists());
    assert!(!rename_would_replace_another_entry(&from, &onto_free));
    std::fs::rename(&from, &onto_free).expect("the filesystem lets it go");
    assert!(!from.exists(), "the old name is gone");
    assert_eq!(
        std::fs::read_to_string(&onto_free).expect("the new name holds the bytes"),
        "one"
    );
    std::fs::remove_dir_all(&directory).ok();
}

/// RED (B-AUDIT-046 RT-3) — **a rename that differs from another file's name
/// only in case never replaces that file; a case-only rename of the file itself
/// still goes through.**
///
/// The gate this replaces compared the two paths lower-cased, which only means
/// "the same file" on a volume that folds case the way `str::to_lowercase`
/// does. Where two such names are two files — case-sensitive APFS, Linux, a
/// Windows directory with the case-sensitivity flag, a WSL tree — the gate
/// passed and `std::fs::rename` replaced the other file with no prompt and no
/// undo. The door now asks the volume for both entries' identity.
///
/// **Which road this walks is the volume's answer, asked here and printed.**
/// Three pairs whose lower-cased spellings are equal are offered to the temp
/// folder, and the first one the volume stores as *two* files is the
/// collision: ASCII case on a case-sensitive volume (Linux, case-sensitive
/// APFS); the Kelvin sign `K` beside `k`, and `ẞ` beside `ß`, on NTFS, whose
/// upcase table folds neither while `to_lowercase` folds both. A volume that
/// folds all three (case-insensitive APFS) has no collision to build, and the
/// test then asserts the identity comparison directly on two distinct files.
/// Either way the case-only rename of one file is walked for real.
///
/// The rename doors' own sequence is walked for real: the identity question
/// both doors ask, then `std::fs::rename` exactly when it says the name is
/// free.
///
/// MUTATION: in `rename_would_replace_another_entry`, replace
/// `bt_platform::same_file(old, new)` with a comparison of the two paths
/// lower-cased — the collision road finds `theirs` replaced by `mine`.
#[test]
fn a_case_only_rename_never_replaces_a_different_file() {
    let directory = bt_testpath::temp_path("bt-audit046-rename");
    std::fs::create_dir_all(&directory).expect("a temp folder");

    let pairs = [
        ("readme.md", "README.md"),
        ("k.md", "\u{212a}.md"),
        ("\u{df}.md", "\u{1e9e}.md"),
    ];
    let mut collision = None;
    for (index, (one, other)) in pairs.into_iter().enumerate() {
        assert_eq!(
            one.to_lowercase(),
            other.to_lowercase(),
            "every pair is one name to a lower-cased comparison"
        );
        let folder = directory.join(format!("pair-{index}"));
        std::fs::create_dir_all(&folder).expect("a folder per pair");
        std::fs::write(folder.join(one), "mine").expect("write the file being renamed");
        let second = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(folder.join(other));
        match second {
            Ok(mut file) => {
                use std::io::Write as _;
                file.write_all(b"theirs").expect("write the other file");
                collision = Some((folder, one, other));
                break;
            }
            // The volume folds this pair: the second name *is* the first file.
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("the volume refused {other:?} for a reason of its own: {error}"),
        }
    }

    match &collision {
        Some((folder, one, other)) => {
            eprintln!("road: collision ({one:?} beside {other:?} are two files here)");
            let from = folder.join(one);
            let onto = folder.join(other);
            assert!(
                !bt_platform::same_file(&from, &onto),
                "the volume says these are two files"
            );
            // The doors' own sequence: ask, and move only when the name is free.
            if !rename_would_replace_another_entry(&from, &onto) {
                std::fs::rename(&from, &onto).expect("the filesystem lets it go");
            }
            assert_eq!(
                std::fs::read_to_string(&onto).expect("the other file is still there"),
                "theirs",
                "and nothing has moved onto it"
            );
            assert_eq!(
                std::fs::read_to_string(&from).expect("the renamed file stayed put"),
                "mine"
            );
        }
        None => {
            eprintln!("road: identity (this volume folds every pair; no collision to build)");
            let one = directory.join("one.md");
            let two = directory.join("two.md");
            std::fs::write(&one, "mine").expect("write one");
            std::fs::write(&two, "theirs").expect("write two");
            assert!(!bt_platform::same_file(&one, &two), "two files are two");
            assert!(
                bt_platform::same_file(&one, &directory.join("ONE.md")),
                "and a folding volume's other spelling is the same file"
            );
        }
    }

    // The case-only rename of the file itself, on whatever this volume is.
    let folder = directory.join("own");
    std::fs::create_dir_all(&folder).expect("a folder for the own-name rename");
    std::fs::write(folder.join("notes.md"), "own").expect("write the file");
    let (own, recased) = (folder.join("notes.md"), folder.join("Notes.md"));
    assert!(
        !rename_would_replace_another_entry(&own, &recased),
        "a case-only rename of one file is a rename"
    );
    std::fs::rename(&own, &recased).expect("the filesystem lets it go");
    let names: Vec<String> = std::fs::read_dir(&folder)
        .expect("list the folder")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["Notes.md"], "one file, under the new spelling");
    std::fs::remove_dir_all(&directory).ok();
}

/// J101 (mock-up 5863-5870) — the box opens holding YOUR name and nothing
/// else, with all of it selected and the caret at its end.
#[test]
fn the_editor_opens_holding_only_the_name_you_typed() {
    let named = TabRename::open(A, Some("build"));
    assert_eq!(named.text(), "build");
    assert_eq!(
        named.caret(),
        5,
        "`input.select()` leaves the caret at the end"
    );
    assert_eq!(named.selection(), 0..5, "and the whole of it selected");

    // The auto name is never *in* the box — it is behind it. A tab that has
    // never been named opens empty, which is what makes the placeholder the
    // only thing you can see.
    let unnamed = TabRename::open(A, None);
    assert_eq!(unnamed.text(), "");
    assert_eq!(unnamed.caret(), 0);
    assert!(
        unnamed.selection().is_empty(),
        "there is nothing to select, so nothing is"
    );
}

/// J101/J102 — typing over the opening selection replaces it, and the
/// draft's own verbs move on character boundaries rather than byte ones.
#[test]
fn typing_over_the_opening_selection_replaces_the_whole_name() {
    let mut editor = TabRename::open(A, Some("build"));
    editor.insert("x");
    assert_eq!(
        editor.text(),
        "x",
        "the selection went with the first keystroke"
    );
    assert_eq!(editor.caret(), 1);
    assert!(editor.selection().is_empty());

    // Backspace on a fresh selection clears it rather than eating one letter.
    let mut cleared = TabRename::open(A, Some("build"));
    press(&mut cleared, &Key::Named(NamedKey::Backspace), NO_MODIFIERS);
    assert_eq!(cleared.text(), "");
    assert_eq!(cleared.caret(), 0);

    // Arrow keys collapse to the near edge, so an accidental select-all is
    // recoverable rather than destructive.
    let mut left = TabRename::open(A, Some("build"));
    press(&mut left, &Key::Named(NamedKey::ArrowLeft), NO_MODIFIERS);
    assert_eq!((left.text(), left.caret()), ("build", 0));
    let mut right = TabRename::open(A, Some("build"));
    press(&mut right, &Key::Named(NamedKey::ArrowRight), NO_MODIFIERS);
    assert_eq!((right.text(), right.caret()), ("build", 5));
}

/// J102/§7.1.5 — the editor's minimum verb set, over text that is not ASCII.
///
/// A tab name is exactly the kind of short label that gets typed in Chinese,
/// and a caret that counts bytes would land inside a character and panic on
/// the next slice.
#[test]
fn the_editors_verbs_move_by_character_and_not_by_byte() {
    let key = |named: NamedKey| Key::Named(named);
    let mut editor = TabRename::open(A, Some("构建"));
    press(&mut editor, &key(NamedKey::ArrowLeft), NO_MODIFIERS);
    assert_eq!(editor.caret(), 0, "collapse to the near edge first");
    press(&mut editor, &key(NamedKey::ArrowRight), NO_MODIFIERS);
    assert_eq!(editor.caret(), 3, "one three-byte character");
    editor.insert("x");
    assert_eq!(editor.text(), "构x建");
    press(&mut editor, &key(NamedKey::Backspace), NO_MODIFIERS);
    assert_eq!(editor.text(), "构建");
    assert_eq!(editor.caret(), 3);
    press(&mut editor, &key(NamedKey::Delete), NO_MODIFIERS);
    assert_eq!(editor.text(), "构", "Delete takes the character in front");
    press(&mut editor, &key(NamedKey::Home), NO_MODIFIERS);
    assert_eq!(editor.caret(), 0);
    press(&mut editor, &key(NamedKey::Delete), NO_MODIFIERS);
    assert_eq!(editor.text(), "");
    press(&mut editor, &key(NamedKey::Backspace), NO_MODIFIERS);
    assert_eq!(editor.text(), "", "and an empty draft survives a backspace");
    press(&mut editor, &key(NamedKey::End), NO_MODIFIERS);
    assert_eq!(editor.caret(), 0);
}

/// J102 (mock-up 5883) — "空串 = 撤销 override(name=null)".
///
/// The sanitiser runs before the emptiness test, so a draft of nothing but
/// spaces is the same answer as a draft of nothing: a name of no characters
/// is the absence of a name, not a name that is blank.
#[test]
fn an_empty_draft_drops_the_override_rather_than_naming_the_tab_nothing() {
    let mut editor = TabRename::open(A, Some("build"));
    press(&mut editor, &Key::Named(NamedKey::Backspace), NO_MODIFIERS);
    assert_eq!(editor.committed_name(), None, "the override is dropped");

    let mut spaces = TabRename::open(A, None);
    spaces.insert("   ");
    assert_eq!(
        spaces.committed_name(),
        None,
        "trimmed to nothing is nothing"
    );

    let mut named = TabRename::open(A, None);
    named.insert("  build  ");
    assert_eq!(
        named.committed_name().as_deref(),
        Some("build"),
        "and a real name arrives sanitised, exactly as every other layer does"
    );

    let mut long = TabRename::open(A, None);
    long.insert(&"n".repeat(60));
    assert_eq!(
        long.committed_name().map(|name| name.chars().count()),
        Some(TITLE_MAX_CHARS),
        "the cap the other three layers already answer to"
    );
}

/// J103 — "编辑期间键盘输入不进终端(编辑器独占)".
///
/// Every arm returns a verdict and there is no arm that hands the key on.
/// The two that leave are the two the mock-up has: Enter commits, Escape
/// abandons (5895-5896).
#[test]
fn the_open_editor_owns_the_keyboard_and_gives_back_only_two_keys() {
    let none = NO_MODIFIERS;
    let mut editor = TabRename::open(A, None);

    assert_eq!(
        press(&mut editor, &Key::Character("a".into()), none),
        RenameVerdict::Held
    );
    assert_eq!(editor.text(), "a");
    assert_eq!(
        press(&mut editor, &Key::Named(NamedKey::Space), none),
        RenameVerdict::Held
    );
    assert_eq!(editor.text(), "a ", "space is text, not a verb");

    // A chord with no verb here must not fall through to the shell:
    // `Ctrl+Q` in a name box is not a quit, it is nothing. (`Ctrl+C` used to
    // be this test's example and is a **copy** since 0.3 — see
    // `the_name_editor_carries_a_selection_word_verbs_and_a_clipboard`.)
    assert_eq!(
        press(
            &mut editor,
            &Key::Character("q".into()),
            ModifiersState::CONTROL
        ),
        RenameVerdict::Held
    );
    assert_eq!(editor.text(), "a ", "and it typed nothing either");

    assert_eq!(
        press(&mut editor, &Key::Named(NamedKey::F5), none),
        RenameVerdict::Held,
        "a key with no verb is swallowed, not passed to the terminal"
    );

    assert_eq!(
        press(&mut editor, &Key::Named(NamedKey::Enter), none),
        RenameVerdict::Commit
    );
    assert_eq!(
        press(&mut editor, &Key::Named(NamedKey::Escape), none),
        RenameVerdict::Cancel
    );
}

/// PIN (0.3, the debt `text_field.rs` recorded on the day it was written) —
/// **the name editor is `TextField` now, and it carries everything that
/// buys.**
///
/// The migration is not a tidy-up: the field a *new* file's name is typed
/// into is this same field, so the alternative was a fourth single-line
/// editor written beside the third, in the same file. What it gains is
/// asserted here rather than assumed, one claim per verb the old editor did
/// not have:
///
/// * **`Shift` with a motion drags the selection's far end** — the old
///   editor's selection could only ever be a prefix, because there was no
///   gesture that could make any other kind;
/// * **`Ctrl+←/→` walk by word**, and `Ctrl+Backspace` eats one — over the
///   same word edge, which is what stops the two disagreeing;
/// * **`Ctrl+C` and `Ctrl+X` put the selection on the clipboard** and
///   `Ctrl+V` puts one line back, which is the whole reason `Ctrl+C` stopped
///   being a chord this editor swallows.
///
/// **And the ownership did not change**: every one of these is still
/// answered *inside* the editor and none of them falls through to the shell,
/// which is what `the_open_editor_owns_the_keyboard_and_gives_back_only_two_
/// keys` says about the keys that have no verb at all.
///
/// RED GATES: put `modifiers.control_key()` back into `rename_key`'s
/// `chorded` and every chord below stops doing anything; hand `false` for
/// `shift` to `step` and the two selection assertions collapse to a caret
/// move; copy without checking the selection is non-empty and the last
/// assertion finds an empty string on the clipboard instead of nothing.
#[test]
fn the_name_editor_carries_a_selection_word_verbs_and_a_clipboard() {
    let ctrl = command_modifier();
    let shift = ModifiersState::SHIFT;
    let ctrl_shift = command_modifier() | ModifiersState::SHIFT;
    let left = Key::Named(NamedKey::ArrowLeft);
    let right = Key::Named(NamedKey::ArrowRight);

    // Shift-selection from the caret, which the old editor could not express
    // at all: this run does not start at the draft's beginning.
    let mut selecting = TabRename::open(A, Some("release notes"));
    press(&mut selecting, &Key::Named(NamedKey::End), NO_MODIFIERS);
    press(&mut selecting, &left, shift);
    press(&mut selecting, &left, shift);
    assert_eq!(
        selecting.selection(),
        11..13,
        "the anchor stayed where the caret was and the far end moved"
    );
    assert_eq!(selecting.caret(), 11, "and the caret IS that far end");

    // Word motions, and the word `Ctrl+Backspace` eats is the one `Ctrl+←`
    // stops at.
    let mut words = TabRename::open(A, Some("release notes"));
    press(&mut words, &Key::Named(NamedKey::End), NO_MODIFIERS);
    press(&mut words, &left, ctrl);
    assert_eq!(words.caret(), 8, "back to the start of the word behind it");
    press(&mut words, &right, ctrl);
    assert_eq!(words.caret(), 13);
    press(&mut words, &Key::Named(NamedKey::Backspace), ctrl);
    assert_eq!(
        words.text(),
        "release ",
        "and the word it ate is the word it walked"
    );

    let mut spanning = TabRename::open(A, Some("release notes"));
    press(&mut spanning, &Key::Named(NamedKey::Home), NO_MODIFIERS);
    press(&mut spanning, &right, ctrl_shift);
    assert_eq!(
        spanning.selection(),
        0..7,
        "Ctrl and Shift together select by word"
    );

    // The clipboard, in both directions. A cut takes what it copied; a copy
    // with nothing selected puts nothing there rather than emptying it.
    let mut copying = TabRename::open(A, Some("release"));
    let (verdict, copied) =
        press_with_clipboard(&mut copying, &Key::Character("c".into()), ctrl, "");
    assert_eq!(verdict, RenameVerdict::Held, "the chord never leaves");
    assert_eq!(copied.as_deref(), Some("release"));
    assert_eq!(copying.text(), "release", "and a copy takes nothing away");

    let mut cutting = TabRename::open(A, Some("release"));
    let (_, cut) = press_with_clipboard(&mut cutting, &Key::Character("x".into()), ctrl, "");
    assert_eq!(cut.as_deref(), Some("release"));
    assert_eq!(cutting.text(), "", "a cut takes what it copied");

    let mut pasting = TabRename::open(A, None);
    press_with_clipboard(
        &mut pasting,
        &Key::Character("v".into()),
        ctrl,
        "pasted name",
    );
    assert_eq!(pasting.text(), "pasted name");
    assert_eq!(pasting.caret(), "pasted name".len());

    let mut empty = TabRename::open(A, None);
    let (_, nothing) = press_with_clipboard(&mut empty, &Key::Character("c".into()), ctrl, "");
    assert_eq!(
        nothing, None,
        "a copy with nothing selected leaves the clipboard as it was"
    );
}

/// A shaping table for a face where every byte is one pixel wide.
///
/// The door [`bt_render::ChromeTextAdvances::from_stops`] exists for: every
/// offset a pin below reads is then a length, and no font is involved in a
/// claim that is not about one. A real face answers the same shape through
/// `Renderer::chrome_text_advances`.
#[allow(clippy::cast_precision_loss)]
fn one_pixel_per_byte(text: &str) -> bt_render::ChromeTextAdvances {
    bt_render::ChromeTextAdvances::from_stops(
        text.char_indices()
            .map(|(at, _)| (at, at as f32))
            .chain(std::iter::once((text.len(), text.len() as f32))),
    )
}

/// The leaf a files-column pin means when it has only ever had one.
const LEAF_ONE: LeafId = LeafId {
    tab: TAB_ONE,
    seat: SeatId(0),
};

/// PIN (D2 of the 2026-09-11 adversarial review) — **fitting a draft into
/// its box shapes the draft once, however long the draft is.**
///
/// `TabRename::fit` used to be handed a `measure(&str) -> f32` and to call it
/// once per *prefix*: the window's start was walked forward one character at
/// a time with the whole remaining prefix re-measured on every step, and
/// every one of those measurements built a fresh `cosmic_text::Buffer` and
/// ran the shaper over it. N characters cost ~N passes over slices averaging
/// N/2 — quadratic work on the window thread, with nothing able to cancel
/// it. A long line pasted into a ~200 px name box was not a slow frame, it
/// was a window that never drew again: the 0910 freeze's own failure class
/// reached by another road.
///
/// The claim is a **bound on measurements**, not a wall-clock reading: a
/// timing assertion would be a test about the machine it ran on, and the
/// defect was never a constant factor.
///
/// RED GATE: put the walk back — step `first_visible` forward one character
/// at a time and measure `shown[first_visible..caret]` on each step — and
/// the count is in the hundreds for a draft this field will hold at all.
#[test]
fn a_long_pasted_name_costs_one_shaping_pass() {
    let ctrl = command_modifier();
    let mut editor = TabRename::open_files_new(LEAF_ONE, "", false, true);
    press_with_clipboard(
        &mut editor,
        &Key::Character("v".into()),
        ctrl,
        &"j".repeat(200_000),
    );

    let mut passes = 0usize;
    {
        let mut shape = |text: &str| {
            passes += 1;
            one_pixel_per_byte(text)
        };
        // A files column's name box, near enough: narrow, and the caret is at
        // the end of everything the field took.
        let drawn = editor.fit(200.0, 1.0, true, &mut shape);
        assert!(
            !drawn.text.is_empty() && drawn.caret_px <= 200.0,
            "the box still shows the end of the draft with the caret in it"
        );
    }
    assert_eq!(
        passes, 1,
        "the composed draft is shaped once and every offset is read off the \
             table"
    );

    // The same for a field with nothing in it: the pass is not skipped on
    // one draft and taken on another, because a surface that measured
    // sometimes is a surface whose cost depends on what was typed.
    let mut empty = TabRename::open_files_new(LEAF_ONE, "", false, true);
    let mut idle = 0usize;
    {
        let mut shape = |text: &str| {
            idle += 1;
            one_pixel_per_byte(text)
        };
        let _ = empty.fit(200.0, 1.0, true, &mut shape);
    }
    assert_eq!(idle, 1);
}

/// PIN (D2) — **a name field takes a name's worth of characters and no
/// more, and an address field takes an address's worth.**
///
/// The other half of the answer above, and the half that is about the
/// *field* rather than about the arithmetic: one shaping pass over a 200 KB
/// draft is linear rather than quadratic and still not work a frame should
/// do. The bound is a property of [`RenameSubject::draft_limit`] — the kind
/// of thing being named — and not a guard at the paste site, which is why
/// the third case below matters: a plain `insert` is bounded too, so an IME
/// commit and a held key cannot walk around it.
///
/// RED GATE: bound the clipboard in `clipboard_line` instead and the direct
/// `insert` below carries 200 000 characters into the draft; drop the bound
/// and the first assertion reads 200 000.
#[test]
fn a_name_field_accepts_no_more_than_a_name() {
    let ctrl = command_modifier();
    let huge = "j".repeat(200_000);

    let mut name = TabRename::open_files_new(LEAF_ONE, "", false, true);
    press_with_clipboard(&mut name, &Key::Character("v".into()), ctrl, &huge);
    assert_eq!(
        name.text().chars().count(),
        NAME_DRAFT_LIMIT,
        "a name goes no further than a directory entry's own maximum"
    );
    press(&mut name, &Key::Character("k".into()), NO_MODIFIERS);
    assert_eq!(
        name.text().chars().count(),
        NAME_DRAFT_LIMIT,
        "and a full field takes nothing more"
    );

    let mut address = TabRename::open_address(LEAF_ONE, "");
    press_with_clipboard(&mut address, &Key::Character("v".into()), ctrl, &huge);
    assert_eq!(
        address.text().chars().count(),
        ADDRESS_DRAFT_LIMIT,
        "an address is a URL and keeps its own, larger limit"
    );
    assert!(
        address.text().chars().count() > name.text().chars().count(),
        "which is larger on purpose, and not the same number twice"
    );

    // Not the clipboard's limit — the field's. This door is the one an IME
    // commit and a held key come through.
    let mut typed = TabRename::open_files_new(LEAF_ONE, "", false, true);
    typed.insert(&huge);
    assert_eq!(typed.text().chars().count(), NAME_DRAFT_LIMIT);

    // And the cut falls on a character, never inside one.
    let mut wide = TabRename::open_files_new(LEAF_ONE, "", false, true);
    wide.insert(&"字".repeat(400));
    assert_eq!(wide.text().chars().count(), NAME_DRAFT_LIMIT);
    assert_eq!(wide.text().len(), NAME_DRAFT_LIMIT * 3, "whole characters");
}

/// PIN (D2) — **where the drawn window starts is read off the one shaping
/// pass, and it is the same place the walk used to arrive at.**
///
/// The bound above says the measurements are gone; this says the answer did
/// not change with them. Both halves of the old arithmetic are pinned: the
/// window is pushed along until the caret is inside the box less the caret's
/// own width, and it is pulled back again while the *whole* tail still fits,
/// so deleting from the end reveals the head instead of leaving the box
/// parked where the longest draft left it.
///
/// RED GATE: derive `first_visible` from a character count and an average
/// advance and the numbers below move; drop the pull-back and the shrunk
/// draft is still drawn from where the long one left the window.
#[test]
fn the_window_start_is_read_from_advances_not_remeasured() {
    let mut shape = |text: &str| one_pixel_per_byte(text);
    let mut editor = TabRename::open(A, Some(&"j".repeat(100)));
    press(&mut editor, &Key::Named(NamedKey::End), NO_MODIFIERS);

    let drawn = editor.fit(20.0, 1.0, true, &mut shape);
    assert_eq!(
        drawn.text.len(),
        20,
        "the window holds exactly the box, ending at the caret"
    );
    assert_eq!(drawn.caret_px, 20.0, "and the caret stands at its far edge");

    // The whole tail fits now, so the window comes back to the head.
    for _ in 0..90 {
        press(&mut editor, &Key::Named(NamedKey::Backspace), NO_MODIFIERS);
    }
    let shrunk = editor.fit(20.0, 1.0, true, &mut shape);
    assert_eq!(shrunk.text.len(), 10, "the head is back in sight");
    assert_eq!(shrunk.caret_px, 10.0);

    // A box that does not scroll never leaves the start, whatever is in it.
    let mut crumb = TabRename::open(A, Some(&"j".repeat(100)));
    press(&mut crumb, &Key::Named(NamedKey::End), NO_MODIFIERS);
    let fixed = crumb.fit(20.0, 1.0, false, &mut shape);
    assert_eq!(fixed.text.len(), 100, "and the whole draft is handed over");
}

/// PIN (D8(a)) — **the refusal that stopped the commit is the refusal the
/// box shows.**
///
/// Everything the box draws by itself is a *prediction* of what Enter will
/// do, and a prediction can be wrong — a file that arrived since the last
/// listing, two spellings of one name, a volume that refused for a reason no
/// listing shows. When it was wrong the field looked valid and Enter did
/// nothing at all. So the refusal the commit really raised is kept on the
/// editor and drawn in the same red, which makes this defect visible and any
/// future commit-only refusal visible with it.
///
/// **Paired with the draft it was raised on** rather than cleared by every
/// verb: an edit changes the text and the refusal stops applying by itself,
/// and typing the refused name back in shows it again — which is right,
/// because it is still refused.
///
/// RED GATE: have `create_files_row` answer a bare `bool` again and there is
/// nothing to record; clear `refused` on open and never write it and the box
/// goes on showing a valid-looking field over a name Enter will not take.
/// RED (M-SWEEP-048) — **a page's address field keeps what the commit said
/// against the draft it was said about**, as the files box does (D8(a)), and
/// one door's refusal is never read as the other's.
///
/// MUTATION: have `address_refusal` answer whatever stands, of either kind, and
/// the cross-reading assertion goes red.
#[test]
fn an_address_the_disk_refused_is_shown_against_its_draft() {
    let mut editor = TabRename::open_address(LEAF_ONE, "https://例子.test/");
    editor.insert("./不在的.md");
    assert_eq!(
        editor.address_refusal(),
        None,
        "nothing has been put through yet"
    );

    editor.refuse_address(webhost::AddressRefusal::NoSuchFile);
    assert_eq!(
        editor.address_refusal(),
        Some(webhost::AddressRefusal::NoSuchFile),
        "the commit's refusal stands against this draft"
    );
    assert_eq!(
        editor.refusal(),
        None,
        "and it is not a name the files box refused"
    );
    editor.insert("x");
    assert_eq!(
        editor.address_refusal(),
        None,
        "and stops standing the moment the draft is different"
    );
    press(&mut editor, &Key::Named(NamedKey::Backspace), NO_MODIFIERS);
    assert_eq!(
        editor.address_refusal(),
        Some(webhost::AddressRefusal::NoSuchFile),
        "typing the refused path back in shows it again"
    );

    let mut files = TabRename::open_files_new(LEAF_ONE, "", false, true);
    files.insert("notes.md");
    files.refuse(files::NewNameRefusal::Taken);
    assert_eq!(files.address_refusal(), None, "a name is not an address");
}

#[test]
fn a_refusal_the_commit_raised_is_shown_in_the_box() {
    let body = |name: &str| method_body("Runtime", name);

    let mut editor = TabRename::open_files_new(LEAF_ONE, "", false, true);
    editor.insert("notes.md");
    assert_eq!(editor.refusal(), None, "nothing has been put through yet");

    editor.refuse(files::NewNameRefusal::Taken);
    assert_eq!(
        editor.refusal(),
        Some(files::NewNameRefusal::Taken),
        "the refusal the commit raised stands against this draft"
    );

    editor.insert("x");
    assert_eq!(
        editor.refusal(),
        None,
        "and stops standing the moment the draft is different"
    );
    press(&mut editor, &Key::Named(NamedKey::Backspace), NO_MODIFIERS);
    assert_eq!(
        editor.refusal(),
        Some(files::NewNameRefusal::Taken),
        "typing the refused name back in shows it again — it is still refused"
    );

    // The wiring, read off the source: a `Runtime` is a surface and a
    // filesystem, and this module's standing rule is that what calls what is
    // read rather than driven.
    assert!(
        item_declaration(&ItemQuery::method("Runtime", "create_files_row"))
            .contains("-> Result<Option<files::NewNameRefusal>>"),
        "the commit answers which refusal stopped it, not a bare bool"
    );
    assert!(
        body("finish_rename").contains("editor.refuse(refusal)"),
        "and the editor is told before it goes back"
    );
    let dress = body("dress_files_tree_editor");
    assert!(
        dress.contains("editor.refusal().is_some()"),
        "the box draws the refusal the commit raised"
    );
    assert!(
        dress.contains("files::names_are_one(&row.key, &taken, folds_case)")
            && !dress.contains("row.key == taken"),
        "and its own advisory asks the folder about case rather than \
             assuming an answer"
    );
    assert!(
        dress.contains("!editor.text().is_empty()"),
        "while an empty draft is still a field nobody has finished, and is \
             not coloured"
    );
    assert!(
        body("open_files_row_new").contains("bt_platform::directory_folds_case("),
        "the folder is asked once, when the box opens — never on a frame"
    );
    assert!(
        !dress.contains("directory_folds_case"),
        "which is red line R-i: no disk call on a frame the caret blinks in"
    );
}

/// PIN (0.3) — **an IME composition is drawn at the caret and is not in the
/// name.**
///
/// Both halves matter and they are the same fact seen from two sides. The
/// pre-edit used to be thrown away — only its commit reached the editor — so
/// a name typed with an IME appeared one committed syllable at a time with
/// nothing on the glass in between. It is drawn now because `TextField`
/// holds it **out of the buffer**, which is what makes drawing it safe: an
/// Escape that cancels a composition leaves the name exactly as it was, with
/// nothing to un-type.
///
/// Measured through [`TabRename::fit`], which is the one arithmetic all five
/// surfaces that draw this editor read — so this is the string that really
/// reaches the glass and the offset the caret is really placed at.
///
/// RED GATES: splice the pre-edit into `text` instead of holding it beside
/// the buffer and the second assertion finds it in the committed name; leave
/// the caret at the buffer's own offset and the composition is drawn *after*
/// the insertion point instead of at it.
#[test]
fn a_composition_is_drawn_at_the_caret_and_never_enters_the_name() {
    // One pixel per byte, which makes every offset below readable as a
    // length — the shaping is not what this pin is about.
    let mut shape = |text: &str| one_pixel_per_byte(text);
    let mut editor = TabRename::open(A, Some("ab"));
    press(&mut editor, &Key::Named(NamedKey::End), NO_MODIFIERS);
    editor.field.set_preedit("ni");

    let drawn = editor.fit(1_000.0, 1.0, true, &mut shape);
    assert_eq!(
        drawn.text, "abni",
        "what is on the glass is the draft with the composition spliced in"
    );
    assert_eq!(
        drawn.caret_px, 4.0,
        "and the caret stands after it, where the next committed character \
             will land"
    );
    assert_eq!(
        editor.text(),
        "ab",
        "while the name itself has not changed — an Escape here un-types \
             nothing"
    );
    assert_eq!(editor.committed_name().as_deref(), Some("ab"));

    // The commit is an ordinary insert, so it replaces whatever was selected
    // by the one rule every other source of text goes through.
    editor.insert("你");
    assert_eq!(editor.text(), "ab你");
    assert!(
        editor.field.preedit().is_empty(),
        "and the composition is finished with"
    );
}

/// PIN (0.3) — **the box a new name is typed into is the box a rename is
/// typed into**, seeded empty because there is no name under it.
///
/// One editor and one set of exits: Enter creates, Escape leaves, and both
/// go through `finish_rename` like every other draft in this window. The
/// seed is the whole of what differs — a rename opens holding the name with
/// its stem selected, and this opens holding nothing at all, so the first
/// character typed lands at the start rather than replacing something.
///
/// RED GATE: seed it with a placeholder name and the first two assertions go
/// red with a word this window made up standing in the reader's box.
#[test]
fn a_new_entrys_box_is_the_rename_box_opened_empty() {
    let leaf = LeafId {
        tab: TAB_ONE,
        seat: SeatId(3),
    };
    for folder in [false, true] {
        let editor = TabRename::open_files_new(leaf, "/src", folder, true);
        assert_eq!(editor.text(), "", "there is no name under this one");
        assert!(editor.selection().is_empty(), "so nothing is selected");
        assert_eq!(editor.caret(), 0, "and the first character lands first");
        assert_eq!(editor.tab(), None, "a row of a tree is not a tab");
        assert!(matches!(
            &editor.subject,
            RenameSubject::FilesNew { parent, folder: kind, .. }
                if parent == "/src" && *kind == folder
        ));
    }
    // The column's own root is a real answer and not a missing one: making a
    // file at the top of the tree names no parent row.
    let at_root = TabRename::open_files_new(leaf, "", false, true);
    assert!(matches!(
        &at_root.subject,
        RenameSubject::FilesNew { parent, .. } if parent.is_empty()
    ));
}

/// PIN (§7.2 ∧ F57) — **the pinned run is the head of the strip, and
/// the command line's tab is the first thing after it.**
///
/// The two "first" rules were written eight days apart and neither named the
/// other, so `folio --cwd D:\proj` over a session with a pinned tab put an
/// unpinned tab at slot 0 ahead of a pinned one — a broken partition, which
/// `tab_trailers` asserts against on the first frame it tries to draw.
///
/// Red gate: make [`cli_tab_slot`] answer `0` — which is literally what
/// `Runtime::create` used to do, by chaining the command line's root in front
/// of the revived ones — and the first two cases fail on the flags.
#[test]
fn a_command_line_tab_falls_in_behind_the_pinned_run() {
    // The report: one pinned tab in the session file, one `--cwd` on the
    // command line. Slot 0 is not available; slot 1 is the front of what is
    // left, and the strip that comes out is still partitioned.
    assert_eq!(cli_tab_slot(&[true]), 1);
    assert!(
        seed::pins_are_normalized(&strip_with_cli_tab(&[true], false), |pinned| *pinned),
        "F57 holds over the strip a command line actually composed"
    );

    // A whole run, not just its first tab: the seam is where the run ends.
    assert_eq!(cli_tab_slot(&[true, true, true]), 3);
    assert!(seed::pins_are_normalized(
        &strip_with_cli_tab(&[true, true, true], false),
        |pinned| *pinned
    ));

    // With nothing pinned, §7.2's "first" is unqualified and reads exactly as
    // it was written — this is the case the rule was tested against, and the
    // clamp must not have quietly moved it.
    assert_eq!(
        cli_tab_slot(&[]),
        0,
        "a first run has no seam to sit behind"
    );
    assert_eq!(
        cli_tab_slot(&[false, false]),
        0,
        "unpinned tabs never displace the tab that was asked for"
    );

    // The arriving tab's own pin is not asked, and does not need to be: the
    // end of the pinned run and the head of the unpinned one are one slot, so
    // a command line that could ask for a pinned tab lands right for the
    // other reason. Written down because the day such a flag exists, this is
    // the line that says nothing has to change.
    assert!(seed::pins_are_normalized(
        &strip_with_cli_tab(&[true, true], true),
        |pinned| *pinned
    ));
}

/// PIN (§7.2) — **the command line's tab takes the seat**, wherever the
/// partition stood it, and without one the tab you were on still does.
///
/// MUTATION: return `active_open.unwrap_or(0)` unconditionally and the
/// first case fails — `folio --cwd D:\proj` with two pinned tabs saved would
/// honour the argument behind the tab it landed on.
#[test]
fn a_command_line_takes_the_seat_and_otherwise_the_tab_you_were_on_keeps_it() {
    assert_eq!(launch_active_tab(Some(0), None, 1), 0);
    assert_eq!(
        launch_active_tab(Some(2), Some(1), 3),
        2,
        "the seat follows the tab, and the tab is behind the pinned run"
    );
    assert_eq!(launch_active_tab(None, Some(1), 3), 1);
    assert_eq!(launch_active_tab(None, None, 3), 0);
    assert_eq!(
        launch_active_tab(None, Some(9), 3),
        2,
        "an index off the end of a shorter list is the last tab, never a panic"
    );
}

/// A tab that has never opened a preview writes no content section at all,
/// so its bytes are the bytes it wrote before the field existed.
#[test]
fn a_tab_with_no_preview_pane_writes_no_content_section() {
    let seats = cross_seats(1);
    let focused = seats.identity();
    let (layout, overflow) = cross_solve(&seats);
    let tab = assemble_tab_state(
        TabId(1),
        BTreeMap::from([(focused, leaf_saying("SHELL"))]),
        BTreeMap::new(),
        preview::PreviewPool::default(),
        PreviewPanes::default(),
        BTreeMap::new(),
        focused,
        TabSeed::default(),
        seats,
        layout,
        overflow,
    );
    assert!(tab.preview_content().is_none());
    assert!(tab.preview_pages().is_empty());
}

/// PIN (T2, real-machine bug): the frame on which motion *stops* is owed a
/// redraw, and the tab asks for it without waiting for any later event.
///
/// This is the bug's real seat. The scheduler used to ask "is anything
/// moving?", which is the right question for how long to keep waking up and
/// the wrong one for whether to draw — and the two part company at exactly
/// one moment, the moment motion ends. Nothing was moving any more, so the
/// old predicate said "nothing owed" and returned before rebuilding,
/// leaving whatever half-transparent frame the breath ended on as the last
/// thing ever drawn. Nothing else was coming: the command was over, so the
/// terminal had gone quiet too.
#[test]
fn the_frame_on_which_the_breath_stops_is_owed_a_redraw() {
    let mid_breath = breathing(Motion::Full);
    let finished = settled(Motion::Full);
    assert_ne!(mid_breath, finished, "the two frames really do differ");
    assert!(
        tab_owes_frame(Some(mid_breath), finished),
        "the working -> idle transition must ask for the frame that \
             puts the mark back to full opacity"
    );
    // What that frame carries is the settled state, at full opacity.
    assert_eq!(finished.opacity, 1.0);
    // Having drawn it, the tab stops asking — the fix must not turn a
    // missing frame into an endless stream of them.
    assert!(
        !tab_owes_frame(Some(finished), finished),
        "a settled tab owes nothing further"
    );
    // A tab that has never drawn anything owes its first frame.
    assert!(tab_owes_frame(None, finished));
}

/// PIN (user report, 2026-08-17) — **a peek is a thing you walk into, so
/// the folder that opened it stays where it is.**
///
/// The bug this is the red gate for: hovering a tab's folder in the
/// sidebar opened the files peek, and the peek vanished the instant the
/// pointer moved off the button and onto the window it had just summoned.
/// Nothing about the float's own grace was wrong —
/// `FloatHost::hold`/`release` were reading the pointer as *inside* and
/// cancelling the dismissal correctly. What took it down was
/// [`Runtime::advance_float`]'s other rule, the one for a peek whose header
/// has died: a tab's trailing run is hover-revealed and its boxes are
/// `width × reveal`, so leaving the tab shrank the folder to nothing,
/// [`Runtime::trigger_rect`] answered `None`, and a trigger that is merely
/// *not under the pointer* was read as a trigger that no longer exists.
///
/// Red gate: drop `peeking` from either half of [`tab_trailing_targets`]
/// and the corresponding half here goes red. The width matters because the
/// rectangle is what the dismissal, the hit test and「再点触发器」all read;
/// the light matters because a control whose popup is open must not look
/// like a control at rest.
#[test]
fn a_folder_that_has_a_peek_hanging_off_it_keeps_its_box_and_its_light() {
    let resting = TabTriggerHand::default();
    assert_eq!(
        tab_trailing_targets(resting),
        (0.0, 0.0),
        "a tab nobody is pointing at wears no run at all"
    );

    let peeking = TabTriggerHand {
        peeking: true,
        ..TabTriggerHand::default()
    };
    let (run, lit) = tab_trailing_targets(peeking);
    assert_eq!(
        run, 1.0,
        "the pointer has walked into the peek, and the trigger it came from \
             still has to have a rectangle for `trigger_rect` to find"
    );
    assert_eq!(
        lit, 1.0,
        "and it is the open one, so it wears the rung it wore when it was \
             pressed rather than the dark it wears at rest"
    );

    // The ladder it was built on is untouched: H76/H104's three rungs of
    // light, and a run opened by the pointer or by the pin.
    let on_tab = TabTriggerHand {
        hovered: true,
        ..TabTriggerHand::default()
    };
    assert_eq!(
        tab_trailing_targets(on_tab),
        (1.0, seats::TAB_FILES_TRIGGER_REVEAL),
        "half-lit while the pointer is anywhere on the tab"
    );
    assert_eq!(
        tab_trailing_targets(TabTriggerHand {
            on_trigger: true,
            ..on_tab
        }),
        (1.0, 1.0),
        "full while it is on the folder itself"
    );
    assert_eq!(
        tab_trailing_targets(TabTriggerHand {
            pinned: true,
            ..TabTriggerHand::default()
        }),
        (1.0, 0.0),
        "a pinned tab holds its run open and its folder dark"
    );
}

/// PIN (T2 D34): a tab wears the loudest claim any of its sessions makes,
/// and the ladder is `fail > bell > unread`.
///
/// Red gate, and the mock-up records it as a user correction (line 1930):
/// "panes wore dots while the tab wore none". A tab is a lid over sessions
/// the user cannot see, so a claim it fails to pass up is a claim that is
/// simply lost.
#[test]
fn a_tab_wears_the_loudest_claim_of_its_sessions() {
    use StatusClaim::{Bell, Failed, Silent, Unread};
    assert_eq!(loudest_claim([Silent, Unread, Bell]), Bell);
    assert_eq!(loudest_claim([Unread, Failed, Bell]), Failed);
    assert_eq!(loudest_claim([Silent, Unread]), Unread);
    assert_eq!(loudest_claim([Silent, Silent]), Silent);
    // A tab with no sessions at all claims nothing rather than panicking.
    assert_eq!(loudest_claim([]), Silent);
    // The ladder itself, stated once so the `max` above cannot silently
    // reorder: every louder claim outranks every quieter one.
    assert!(Silent < Unread && Unread < Bell && Bell < Failed);
    // And a tab never says *less* than any one of its members.
    for members in [
        vec![Silent, Failed],
        vec![Bell, Unread, Silent],
        vec![Unread],
    ] {
        let tab = loudest_claim(members.iter().copied());
        for member in members {
            assert!(tab >= member, "a tab must never say less than its panes");
        }
    }
}

#[test]
fn closing_a_background_tab_preserves_the_same_active_identity() {
    assert_eq!(
        tab_close_action(4, 2, 0),
        TabCloseAction::Keep { active_tab: 1 }
    );
    assert_eq!(
        tab_close_action(4, 1, 3),
        TabCloseAction::Keep { active_tab: 1 }
    );
}

/// Ctrl+Tab and Ctrl+Shift+Tab walk the strip in opposite directions and both
/// wrap, so a four-tab window is a ring with no dead end at either edge.
#[test]
fn the_tab_chords_walk_the_strip_in_a_ring() {
    assert_eq!(stepped_tab(4, 0, true), Some(1));
    assert_eq!(stepped_tab(4, 3, true), Some(0), "forward wraps at the end");
    assert_eq!(stepped_tab(4, 3, false), Some(2));
    assert_eq!(
        stepped_tab(4, 0, false),
        Some(3),
        "backward wraps at the start"
    );
    // A full lap in either direction returns to where it started.
    let mut index = 1;
    for _ in 0..4 {
        index = stepped_tab(4, index, true).expect("four tabs step");
    }
    assert_eq!(index, 1);

    // A lone tab is already the answer, so nothing is re-activated.
    assert_eq!(stepped_tab(1, 0, true), None);
    assert_eq!(stepped_tab(1, 0, false), None);
    assert_eq!(stepped_tab(0, 0, true), None);
}

/// Ctrl+Shift+1..9 counts the strip the way the user does, and an ordinal past
/// the end of it names no tab at all.
#[test]
fn goto_tab_counts_from_one_and_ignores_an_ordinal_off_the_end() {
    assert_eq!(
        goto_tab_index(4, 1),
        Some(0),
        "the first tab is Ctrl+Shift+1"
    );
    assert_eq!(goto_tab_index(4, 4), Some(3));
    assert_eq!(
        goto_tab_index(4, 5),
        None,
        "there is no fifth tab, and clamping onto the fourth would answer a \
             question nobody asked"
    );
    assert_eq!(goto_tab_index(4, 9), None);
    assert_eq!(goto_tab_index(0, 1), None);
    assert_eq!(goto_tab_index(4, 0), None, "there is no zeroth tab");
}

/// PIN (R1-23) — **a format character cannot reorder a name a reader is
/// told to check.**
///
/// The sanitisers dropped `Cc` and stopped there, so `U+202E` and its
/// family reached the tab strip and the hover line. A right-to-left
/// override in a title reverses the run after it, which is how
/// `report.exe` prints as `report.txt` above the pane that will run it, and
/// a bidi isolate in the hover address reorders the host under the cells
/// the address was printed in.
///
/// MUTATION: filter `char::is_control` alone and every string below keeps
/// the character that moved the text.
#[test]
fn a_title_and_a_hover_address_carry_no_format_characters() {
    const REORDERING: [char; 9] = [
        '\u{202e}', '\u{202a}', '\u{200b}', '\u{200f}', '\u{feff}', '\u{2066}', '\u{2069}',
        '\u{00ad}', '\u{061c}',
    ];
    for character in REORDERING {
        let sneaky = format!("a{character}b");
        let cleaned = clean_title_capped(&sneaky, TITLE_MAX_CHARS);
        assert_eq!(
            cleaned, "ab",
            "a title still carries U+{:04X}",
            character as u32
        );
        let printed = printable_address(&sneaky);
        assert!(
            !printed.contains(character),
            "a hover address still carries U+{:04X}: {printed:?}",
            character as u32
        );
    }
    // The tag block, which spells a whole second name inside one.
    assert_eq!(clean_title_capped("a\u{e0041}\u{e007f}b", 40), "ab");
    assert!(!printable_address("a\u{e0041}b").contains('\u{e0041}'));
    // The letters themselves are untouched, in every script.
    assert_eq!(clean_title_capped("  中文 README  ", 40), "中文 README");
    assert_eq!(
        printable_address("https://例え.test/ページ"),
        "https://例え.test/ページ"
    );
}

#[test]
fn a_resize_reflow_holds_until_projectable_staging_reanchors_the_reprint() {
    let start = Instant::now();
    let mut harness = PtyPresentationHarness::new(40, 10);
    let mut lines = Vec::new();
    for index in 0..60 {
        lines.extend_from_slice(format!("line-{index:03}\r\n").as_bytes());
    }
    harness.session.feed(&lines).unwrap();
    assert!(harness.publish_pty_frame());
    harness.present_pending();

    // Enter review.
    harness.projection.scroll_by_rows(20);
    assert!(harness.publish_pty_frame());
    harness.present_pending();
    assert_eq!(
        harness.last_presented.as_ref().unwrap().scroll_offset_rows,
        20
    );
    let publications_before = harness.publications;

    // A resize opens the transaction and Codex clears scrollback: the review anchor vanishes
    // and history is transiently empty. The interim frame is bottom-snapped, but presentation
    // must hold the last frame rather than flash to the bottom.
    harness
        .session
        .resize_at(
            NonZeroU32::new(40).unwrap(),
            NonZeroU32::new(12).unwrap(),
            start,
        )
        .unwrap();
    harness.session.mark_pty_resize_requested_at(
        NonZeroU32::new(40).unwrap(),
        NonZeroU32::new(12).unwrap(),
        start + Duration::from_millis(10),
    );
    harness
        .session
        .feed_at(b"\x1b[2J\x1b[3J\x1b[H", start + Duration::from_millis(20))
        .unwrap();
    assert!(
        !harness.publish_pty_frame(),
        "the hold skips publishing the bottom-snapped interim frame"
    );
    assert!(
        !harness.present_pending(),
        "nothing was published to present"
    );
    assert_eq!(
        harness.publications, publications_before,
        "no new publication during the hold"
    );
    assert_eq!(
        harness.last_presented.as_ref().unwrap().scroll_offset_rows,
        20,
        "the screen still shows the reviewing frame, not the bottom"
    );

    // The reprint enters projectable resize staging: publication resumes immediately at the
    // restored review position — a direct hand-off with no bottom frame ever presented.
    harness
        .session
        .feed_at(&lines, start + Duration::from_millis(30))
        .unwrap();
    assert!(
        harness.publish_pty_frame(),
        "resize staging releases the hold as soon as the reprint is reachable"
    );
    harness.present_pending();
    assert_eq!(
        harness.last_presented.as_ref().unwrap().scroll_offset_rows,
        20,
        "presentation resumes exactly at the staged review displacement"
    );

    // Quiescence commits the same staging ids through normal history relocation and must not
    // move the already-restored reading position.
    assert!(
        harness
            .session
            .finish_resize_if_quiescent(start + Duration::from_millis(280))
            .unwrap()
    );
    if harness.publish_pty_frame() {
        harness.present_pending();
    }
    assert_eq!(
        harness.last_presented.as_ref().unwrap().scroll_offset_rows,
        20,
        "final harvest preserves the already-restored review displacement"
    );
}

/// RED GATE (丙2) — **the tab menu holds an id, and resolves it at the
/// moment a verb runs.**
///
/// A menu is a question the reader takes several seconds to answer, and the
/// strip does not hold still for them: a drag reorders it, a pin
/// re-partitions it, a tab arriving from another window lands in it, a tab
/// closing leaves it. A menu that had written down "the third tab" when it
/// opened would run its verb on whatever had slid into the third slot — and
/// `Close tab` is on this menu, so the failure is not cosmetic.
///
/// [`TabId`] is minted once per process and never reused, so an id that stops
/// naming a tab names nothing at all; every verb here asks
/// [`Runtime::tab_slot_of`] or [`Runtime::tab_state`] for the tab and does
/// nothing when the answer is `None`. That is the same discipline
/// [`PaneMenuState`] keeps with a seat and [`FileMenuTreeRow`] with a key,
/// written down here because this is the one subject in the window that is
/// *routinely* re-ordered under an open menu.
///
/// Read off the source for `a_right_press_on_a_tab_raises_its_menu_and_leaves_the_active_tab_alone`'s
/// reason: the failure is a *shape*, and the shape is what is asserted.
///
/// MUTATIONS that must turn it red:
/// ① give [`TabMenuState`] an index instead of an id — the first assertion;
/// ② resolve the index once, at raise time, and carry it — the second and
///    third, which count the resolutions inside the runner;
/// ③ let any verb reach for `active_tab` — the fourth.
#[test]
fn the_tab_menus_subject_is_an_id_resolved_when_the_verb_runs() {
    let body = |name: &str| method_body("Runtime", name);

    // ① the state carries the identity and not the place.
    let state = type_body("TabMenuState");
    assert!(
        state.contains("tab: TabId,"),
        "the menu's subject is the tab's identity"
    );
    assert!(
        !state.contains("index") && !state.contains("slot"),
        "and nowhere in it is a place in the strip: {state}"
    );

    // ② every verb that needs a place asks for one, at the moment it runs.
    let runner = body("run_tab_menu_row");
    assert_eq!(
        runner.matches("self.tab_slot_of(tab)").count(),
        2,
        "`Pin` and `Close tab` are the two rows that need a slot, and each \
             asks for its own — a slot resolved once and shared would be the \
             index this menu refuses to carry"
    );
    assert!(
        runner.contains("let Some(index) = self.tab_slot_of(tab) else {"),
        "and a tab that has gone answers nothing rather than panicking"
    );

    // ③ so do the three verbs the runner hands the id straight to.
    for (door, lookup) in [
        ("duplicate_tab", "self.tab_state(tab)"),
        ("move_tab_to_new_window", "self.tab_slot_of(tab).is_none()"),
        ("move_tab_to_window", "self.tab_slot_of(tab).is_none()"),
    ] {
        assert!(
            body(door).contains(lookup),
            "`{door}` resolves its subject by id, through `{lookup}`"
        );
    }

    // ④ and nothing in the family reaches for whichever tab is in front.
    for door in [
        "run_tab_menu_row",
        "duplicate_tab",
        "move_tab_to_new_window",
        "move_tab_to_window",
        "open_tab_menu_at",
    ] {
        assert!(
            !body(door).contains("active_tab"),
            "`{door}` names the active tab — the menu's subject is the tab \
                 that was pointed at, whichever tab is in front"
        );
    }

    // And the duplicate is seeded from *that* tab's own leaf, through the one
    // door every other new tab goes through.
    let duplicate = body("duplicate_tab");
    assert!(
        duplicate.contains("state.focused()")
            && duplicate.contains("leaf.profile")
            && duplicate.contains("leaf.seed_place_for_a_new_shell()"),
        "both facts come off one leaf of the tab the menu names"
    );
    assert!(
        duplicate.contains("self.new_tab_seeded_from("),
        "and the tab is made by the one function that makes tabs"
    );
}

/// **PIN — which surface the `+` stands on, and the one place that decides.**
///
/// [`profile_menu_anchor`] hangs the menu's box off this answer and
/// [`popup_owner`] hangs its ownership off it; the day the two disagree is
/// the day the sidebar retracts out from under a menu it is still drawing.
#[test]
fn the_focus_column_supersedes_both_ordinary_tab_surfaces() {
    for layout in [
        seats::TabLayoutMode::Horizontal,
        seats::TabLayoutMode::Vertical,
    ] {
        assert_eq!(
            tab_surface(true, layout),
            TabSurface::FocusColumn,
            "{layout:?}: focus mode carries the `+` in either tab layout"
        );
    }
    assert_eq!(
        tab_surface(false, seats::TabLayoutMode::Vertical),
        TabSurface::Rail
    );
    assert_eq!(
        tab_surface(false, seats::TabLayoutMode::Horizontal),
        TabSurface::Strip
    );
}

/// PIN (schema v5): both new fields go to the file and come back unchanged,
/// every value of them.
#[test]
fn the_tab_layout_and_sidebar_mode_survive_the_round_trip_through_the_session() {
    use seats::{RailMode, TabLayoutMode};
    for layout in [TabLayoutMode::Horizontal, TabLayoutMode::Vertical] {
        assert_eq!(render_tab_layout(session_tab_layout(layout)), layout);
    }
    for persisted in [SessionTabLayoutV1::Horizontal, SessionTabLayoutV1::Vertical] {
        assert_eq!(session_tab_layout(render_tab_layout(persisted)), persisted);
    }
    for mode in [RailMode::Expanded, RailMode::Icons] {
        assert_eq!(render_sidebar_mode(session_sidebar_mode(mode)), mode);
    }
    for persisted in [SessionSidebarModeV1::Expanded, SessionSidebarModeV1::Icons] {
        assert_eq!(
            session_sidebar_mode(render_sidebar_mode(persisted)),
            persisted
        );
    }
}

/// PIN (58) — **every producer of the gate reads its answer as three.**
///
/// The verbs that run on their own when there is nothing to ask — the tab's
/// close, the pane's close, `Clear scrollback…` — go on only on `proceeds()`;
/// the git requests, whose verb only the answer performs, go through the one
/// door that does nothing either way (`ask_before_the_verb`); and the OS close
/// is the first test's arm. A caller that went back to "stop only when this
/// raised it" would reopen R1 for its own verb.
///
/// MUTATION: write `if self.raise_dirty_gate(..)? == GateRaise::Raised`
/// in `close_tab` — the first assertion goes red.
#[test]
fn every_producer_of_the_dirty_gate_goes_on_only_when_there_was_nothing_to_ask() {
    for (name, request) in [
        ("close_tab", "restore::GateRequest::CloseTab(index)"),
        ("close_pane", "restore::GateRequest::ClosePane(seat)"),
        (
            "run_term_menu_row",
            "restore::GateRequest::ClearScrollback(seat)",
        ),
    ] {
        let body = squeezed(method_body("Runtime", name));
        assert!(
            body.contains(&format!(
                "if!self.raise_dirty_gate({request})?.proceeds(){{returnOk(());}}"
            )),
            "`Runtime::{name}` goes on for a reason other than nothing to ask:\n{body}"
        );
    }
    assert_eq!(
        reader_names(&calls_of("Runtime", "raise_dirty_gate")),
        [
            "ask_before_the_verb",
            "close_pane",
            "close_tab",
            "run_term_menu_row",
            "the_summon_lets_the_run_end",
            "window_event",
        ],
        "a new producer of the gate: pin it above once it goes on only on `proceeds()`"
    );
    // **The run's end, asked in the summoned terminal** (T-SUMMON-DIRTY-PREVIEW):
    // the ending close goes on only on `proceeds()`.
    let summon = squeezed(method_body("FolioApp", "the_summon_lets_the_run_end"));
    assert!(
        summon.contains("ifraised.proceeds(){returnOk(true);}"),
        "`FolioApp::the_summon_lets_the_run_end` lets the run end for a reason other than \
         nothing to ask:\n{summon}"
    );
}

#[test]
fn a_grabbed_tab_follows_the_hand_until_the_strip_runs_out() {
    // K115. The tab travels in x, and stops at the strip's own edges rather
    // than being carried out over the caption buttons.
    let viewport = [0.0, 900.0];
    let (slot_left, width) = (204.0_f32, 200.0);
    assert_eq!(
        grabbed_offset(slot_left, width, viewport, 300.0),
        96.0,
        "free of both edges, the offset is simply the distance"
    );
    assert_eq!(
        grabbed_offset(slot_left, width, viewport, -50.0),
        -slot_left,
        "held past the left edge it stops with its leading edge on it"
    );
    assert_eq!(
        grabbed_offset(slot_left, width, viewport, 5_000.0),
        700.0 - slot_left,
        "and past the right edge with its trailing edge on that one"
    );
    assert_eq!(
        grabbed_offset(0.0, 200.0, [0.0, 120.0], 5_000.0),
        0.0,
        "a viewport narrower than the tab keeps the leading edge, not the trailing one"
    );
}

#[test]
fn a_tab_displaced_again_mid_slide_starts_from_where_it_actually_is() {
    // The mock-up measures `getBoundingClientRect`, which includes the live
    // transform (6579-6583): a second swap inside the first slide's 160ms
    // must not snap the tab back to a slot it never reached.
    let now = Instant::now();
    let mut flip = FlipTween::default();
    flip.displace(-96.0, now, Motion::Full);
    let at = now + Duration::from_millis(80);
    let (mid, _) = flip.sample(at, Motion::Full);
    flip.displace(-204.0, at, Motion::Full);
    let (restarted, _) = flip.sample(at, Motion::Full);
    assert!(
        (restarted - (mid - 204.0)).abs() < 1e-3,
        "the new slide starts at the old one's live position plus the new delta"
    );
}

#[test]
fn reduced_motion_puts_a_displaced_tab_straight_into_its_slot() {
    // **Ruling.** The mock-up writes these transitions from JavaScript, where
    // no `prefers-reduced-motion` block can reach them, so its silence here
    // is its medium rather than a decision. A transform travelling across the
    // screen is precisely what the preference is about — unlike the progress
    // ring's sweep, which carries a reading and is deliberately left running.
    let now = Instant::now();
    let mut flip = FlipTween::default();
    flip.displace(-96.0, now, Motion::Reduced);
    assert_eq!(flip.sample(now, Motion::Reduced), (0.0, false));
}

#[test]
fn a_cancelled_drag_puts_the_tab_back_without_crossing_the_pinned_seam() {
    // F57, applied to the one move geometry did not choose (K128's restore).
    let pinned = [true, true, false, false];
    assert_eq!(
        partition_clamped(&pinned, 3, 2),
        2,
        "inside its own partition the restore reaches its slot"
    );
    assert_eq!(
        partition_clamped(&pinned, 3, 0),
        2,
        "and stops at the seam rather than landing among the pinned tabs"
    );
    assert_eq!(partition_clamped(&pinned, 0, 3), 1);
    assert_eq!(partition_clamped(&pinned, 2, 2), 2, "a move to nowhere");
    assert_eq!(
        partition_clamped(&[false, false, false], 2, 0),
        0,
        "with no seam there is nothing to stop at"
    );
}

#[test]
fn a_drag_that_never_starts_leaves_the_press_exactly_as_t4_left_it() {
    // The seam between T4 and T5, from T5's side: the 6px and the identity
    // are the press's, and the move it names as the drag's first is the
    // only thing this slice reads.
    let now = Instant::now();
    let mut press = TabPress::armed(TabId(1), PhysicalPosition::new(100.0, 20.0), now);
    assert!(!press.travelled(PhysicalPosition::new(105.0, 20.0), 1.0));
    assert_eq!(press.promise, TabPressPromise::Pending);
    assert!(press.travelled(PhysicalPosition::new(106.0, 20.0), 1.0));
    assert_eq!(press.promise, TabPressPromise::Slipped);
    assert!(
        !press.travelled(PhysicalPosition::new(300.0, 20.0), 1.0),
        "the drag starts once — every later move is the gesture, not its start"
    );
}

/// **§7.1.6k — the two offers a tab list makes a pane, and which pointer gets
/// which.**
///
/// The ruling: *"拖着 pane 悬在 tab 上 … 直接松在 tab 上 = 移入该 tab"*, with
/// the tear-out left exactly where it was. So the decision is read off two
/// questions about one pointer — whose tab is under it, and between which two
/// would a new tab land — and the four rows below are the whole table.
///
/// Red gate: answer `StripExtract` for a pointer over a foreign tab (which is
/// what this file did until §7.1.6k) and the first row fails; let the pane's
/// **own** tab answer `StripAdopt` *while it is the tab on screen* and the
/// second does; fall back to the tear-out when the target will not fit and
/// the last one does.
///
/// **Every surface hands these rows in as `BOTH` since 2026-08-29.** The
/// rows that used to be run a second time with the card column's own
/// `ADOPT_ONLY` are gone with that value: the user withdrew ② entire, so a
/// column's answers *are* the rows above rather than a second table beside
/// them, and the surface that answers them is pinned in `seats.rs` by
/// `the_card_column_takes_a_tab_reorder_and_a_panes_two_offers`.
///
/// Every row here is a hand that is **still standing in the tab it picked
/// the pane up from** (`showing == mine`). §7.1.6k″'s rows — the same
/// pointers once the spring has moved the stage — are
/// [`a_pane_that_sprang_away_comes_home_by_its_own_tab_entry`]'s.
#[test]
fn the_strip_hands_a_pane_to_the_tab_under_it_and_makes_a_new_one_in_the_gaps() {
    let mine = TabId(1);
    let other = TabId(2);
    let both = seats::PaneOffers::BOTH;
    assert_eq!(
        pane_strip_landing(both, Some(other), mine, mine, true, Some(3)),
        Some(DropLanding::StripAdopt { tab: other }),
        "resting on somebody else's tab is asking to be put in it"
    );
    assert_eq!(
        pane_strip_landing(both, Some(mine), mine, mine, true, Some(3)),
        Some(DropLanding::StripExtract { slot: 3 }),
        "resting on your own tab while you are standing in it is not a \
             move, so the strip's ordinary offer stands"
    );
    assert_eq!(
        pane_strip_landing(both, None, mine, mine, true, Some(3)),
        Some(DropLanding::StripExtract { slot: 3 }),
        "and the run's padding is the gap between tabs, which is where a \
             new tab has always been made"
    );
    assert_eq!(
        pane_strip_landing(both, Some(other), mine, mine, false, Some(3)),
        None,
        "M147: a tab whose tree will not take the pane says so by not \
             lighting up — and emphatically does not fall through to a tear-out"
    );
    assert_eq!(
        pane_strip_landing(both, None, mine, mine, true, None),
        None,
        "and a pane that cannot become a tab of its own is still turned \
             away in the gaps (K124/G84)"
    );
}

/// **§7.1.6k″ — the way back, and it is the way out read in the other
/// direction** (user's report 2026-08-24, Claude's ruling).
///
/// The report: pick a pane up in tab A, rest it on tab B until the spring
/// takes you there, change your mind — and there is nowhere to go. A's own
/// entry answered the tear-out, which arms no spring, and the stage under
/// the pointer is B's for the rest of the gesture. **Out to any tab, never
/// back.**
///
/// The ruling is one clause: your own tab is a room like any other from the
/// moment you are not standing in it. Nothing else about the gesture is new
/// — the same `StripAdopt`, the same `SpringGate`, the same quarter second,
/// the same `DragRelease::Adopt` — which is why this test is a walk down the
/// existing chain rather than a description of a second one.
///
/// **Letting go on it moves no pane**, and that is the whole of "回家" rather
/// than a hole in it: the tree was never touched while the pane was in the
/// air (`DragCarry::Pane`), so [`Runtime::move_pane_across_tabs`]'s
/// `from == into` answers `false`, `release_drag` falls to
/// [`Runtime::settle_home`], and the pane is exactly where the user left it.
/// The honest inverse of `Move pane to new tab` is that the move never
/// happened.
///
/// Red gate (verified red on `813fa7d`, before the fix): the condition
/// `tab != holding` alone answers `Some(StripExtract { slot: 3 })` for the
/// first assertion, the spring is then told `None`, and the second one gets
/// no tab back at all.
#[test]
fn a_pane_that_sprang_away_comes_home_by_its_own_tab_entry() {
    let mine = TabId(1);
    let other = TabId(2);
    let both = seats::PaneOffers::BOTH;

    // The hand holds a pane of `mine`; the spring has left the stage on
    // `other`.
    let away = pane_strip_landing(both, Some(mine), mine, other, true, Some(3));
    assert_eq!(
        away,
        Some(DropLanding::StripAdopt { tab: mine }),
        "your own tab is a room like any other once you are not standing \
             in it"
    );
    assert_eq!(
        pane_strip_landing(both, Some(mine), mine, mine, true, Some(3)),
        Some(DropLanding::StripExtract { slot: 3 }),
        "and it stops being one the moment you are back in it — K135's own \
             sentence, unchanged, because there the landing really would do \
             nothing"
    );

    // The spring reads the survey's answer and never the pointer, so the
    // way home arms it without being told that home is special.
    let start = Instant::now();
    let mut spring = SpringGate::default();
    spring.observe(
        match away {
            Some(DropLanding::StripAdopt { tab }) => Some(tab),
            _ => None,
        },
        start,
    );
    assert_eq!(
        spring.due(start + profiles::CHEVRON_HOVER_OPEN_DELAY),
        Some(mine),
        "and resting on it brings the stage back, on the same quarter \
             second that took you away"
    );
    assert_eq!(
        release_verdict(away),
        DragRelease::Adopt { tab: mine },
        "letting go on it is the tab entry's own verdict, which for the \
             tab the pane never left is a move from a tab to itself — no tree \
             is edited and the pane stays exactly where it is"
    );

    // H93 is not asked about the tab the pane is already in: `plan_drop`
    // would be answering whether that tree could hold one *more* pane, and
    // nothing is arriving.
    assert_eq!(
        pane_strip_landing(both, Some(mine), mine, other, false, Some(3)),
        Some(DropLanding::StripAdopt { tab: mine }),
        "a pane fits in the tree it is standing in, whatever a plan for a \
             tree with one more pane in it would have said"
    );
    assert_eq!(
        pane_strip_landing(both, Some(other), mine, other, false, Some(3)),
        None,
        "while the tab it would really arrive in is judged exactly as \
             before (M147)"
    );

    // §7.1.6b′ ②'s silence was 「the hand-over would do nothing」, and that
    // premise is what the spring falsifies — so the card column gets the
    // way home for the same reason and by the same line. It needs no row of
    // its own here any more: since 2026-08-29 the column hands this function
    // the same `BOTH` the strip does, so the rows above *are* its rows.
}

/// **§7.1.6b′ ② as re-judged 2026-08-23 and withdrawn 2026-08-29 — the card
/// column's whole chain, end to end.**
///
/// The first ruling: *"卡片接收 pane"* — rest a carried pane on a card for the
/// chevron's own quarter second and the stage goes to that tab with the pane
/// still in the air; let go on the card and the pane moves into that tab,
/// appended at the end of its tree, which is *"与 tab 条目同义"*.
///
/// The second took the other half. What stood here was *"pane 在卡列空白处
/// 松手仍不撕新 tab"*, and the user met it on the machine: a pane held over
/// the blank by the `+` row showed a ghost and did nothing when the hand
/// opened. The ruling is ① read literally — *"舞台就是真的那棵树,零新规、
/// 不禁任何动词"* — so the blank between two cards, and the tail below the
/// last one, make a tab at that slot exactly as the strip's gaps do, by the
/// same `Runtime::extract_pane_into_new_tab`.
///
/// Every link is asserted against the machinery the strip already had rather
/// than against a card-shaped copy of it, because that is the claim: the
/// column answers `StripAdopt` like any run, the spring reads the survey's
/// answer rather than the pointer's coordinates so it arms without being
/// told about cards, and both release verdicts are the strip's own.
///
/// Red gate: give the column one bit for both verbs — which is what this file
/// did until 2026-08-23 — and the first assertion answers `None`, the spring
/// never arms, and focus mode has no door to another tab at all; put it back
/// on `ADOPT_ONLY` and the blank stops making tabs, which is the user's
/// 2026-08-29 bug report word for word.
#[test]
fn a_card_takes_a_pane_and_springs_while_the_blank_beside_it_makes_a_new_tab() {
    let mine = TabId(1);
    let other = TabId(2);
    // The very value `seats::focus_rail_run` puts on the column's run, which
    // since 2026-08-29 is the value all three surfaces put on theirs.
    let cards = seats::PaneOffers::BOTH;

    let on_a_card = pane_strip_landing(cards, Some(other), mine, mine, true, Some(3));
    assert_eq!(
        on_a_card,
        Some(DropLanding::StripAdopt { tab: other }),
        "a card is a room, and pointing at it asks to be put in it"
    );
    assert_eq!(
        pane_strip_landing(cards, None, mine, mine, true, Some(3)),
        Some(DropLanding::StripExtract { slot: 3 }),
        "and the blank beside the cards is the gap between two tabs — the \
             place a new one has always been made"
    );

    // The spring is told what the survey answered, never where the pointer
    // is, which is why the column had nothing to add to it.
    let start = Instant::now();
    let mut spring = SpringGate::default();
    spring.observe(
        match on_a_card {
            Some(DropLanding::StripAdopt { tab }) => Some(tab),
            _ => None,
        },
        start,
    );
    assert_eq!(
        spring.deadline(),
        Some(start + profiles::CHEVRON_HOVER_OPEN_DELAY),
        "the same quarter second the `⌄` menus rest for — one constant, and \
             the state lives on the drag rather than being shared with them"
    );
    assert_eq!(
        spring.due(start + profiles::CHEVRON_HOVER_OPEN_DELAY),
        Some(other),
        "so the stage goes to that tab with the pane still in the air"
    );

    assert_eq!(
        release_verdict(on_a_card),
        DragRelease::Adopt { tab: other },
        "and letting go on the card is the tab entry's own verdict: move it \
             in, appended at the end of that tree"
    );
    assert_eq!(
        release_verdict(pane_strip_landing(cards, None, mine, mine, true, Some(3))),
        DragRelease::Extract { slot: 3 },
        "while letting go in the blank tears the pane out into a card of \
             its own at that slot — `Runtime::commit_pane_extract`, the strip's \
             own door"
    );
    assert_eq!(
        release_verdict(pane_strip_landing(cards, None, mine, mine, true, None)),
        DragRelease::Home,
        "and the one pointer in the blank that still goes home is the one \
             holding a pane that cannot become a tab at all (K124/G84) — a \
             refusal of the *pane*, which every surface has always made"
    );
}

/// **The spring's clock — every transition the ruling names.**
///
/// *"连续停 250ms 切到该 tab"*, *"移开又移回来 = 重新计时"*, *"切过去之后指针
/// 仍在同一个 tab 上,不该反复重切"*. All three are properties of this gate
/// alone, and it is a gate rather than two fields on the window so that they
/// can be driven without a pointer, a window or a sleep.
///
/// Red gate: give the spring a number of its own instead of
/// `profiles::CHEVRON_HOVER_OPEN_DELAY` and the first block fails; leave `observe`
/// assigning rather than keeping the instant it already has and a pointer
/// twitching inside one tab never matures; drop `SpringAim::Sprung` and the
/// last block switches to the tab it is already showing, every 250ms, for as
/// long as the hand stays put.
#[test]
fn the_spring_matures_at_the_chevrons_own_quarter_second_and_is_spent_once() {
    let start = Instant::now();
    let mut gate = SpringGate::default();
    assert_eq!(gate.due(start), None, "an empty hand owes nothing");
    assert_eq!(gate.deadline(), None, "and costs no wake-ups");

    gate.observe(Some(TabId(7)), start);
    assert_eq!(
        gate.deadline(),
        Some(start + profiles::CHEVRON_HOVER_OPEN_DELAY),
        "one number for 'a hand has settled on something', shared with the \
             two chevrons rather than copied"
    );
    assert_eq!(
        gate.due(start + profiles::CHEVRON_HOVER_OPEN_DELAY - Duration::from_millis(1)),
        None,
        "a millisecond short is still a hand passing through"
    );
    // A pointer that moved two pixels inside the same tab is still resting
    // on it, and must not throw away the rest it has earned.
    gate.observe(Some(TabId(7)), start + Duration::from_millis(200));
    assert_eq!(
        gate.due(start + profiles::CHEVRON_HOVER_OPEN_DELAY),
        Some(TabId(7)),
        "the quarter second is measured from when the hand arrived"
    );

    // Off the tab and back again: a hand that wandered has not been resting.
    let mut wandering = SpringGate::default();
    wandering.observe(Some(TabId(7)), start);
    wandering.observe(None, start + Duration::from_millis(200));
    assert_eq!(
        wandering.due(start + profiles::CHEVRON_HOVER_OPEN_DELAY),
        None,
        "leaving spends nothing and starts nothing"
    );
    wandering.observe(Some(TabId(7)), start + Duration::from_millis(240));
    assert_eq!(
        wandering.due(start + profiles::CHEVRON_HOVER_OPEN_DELAY),
        None,
        "and coming back starts the clock again from zero"
    );
    assert_eq!(
        wandering.due(start + Duration::from_millis(240) + profiles::CHEVRON_HOVER_OPEN_DELAY),
        Some(TabId(7))
    );

    // Sprung, with the pointer still on the tab it sprang to.
    let matured = start + profiles::CHEVRON_HOVER_OPEN_DELAY;
    gate.spend(TabId(7));
    assert_eq!(gate.deadline(), None, "a spent rest costs no wake-ups");
    for later in [1_u64, 250, 5_000] {
        gate.observe(Some(TabId(7)), matured + Duration::from_millis(later));
        assert_eq!(
            gate.due(matured + Duration::from_millis(later)),
            None,
            "the hand is resting on the tab it is already looking at, and \
                 that is not a second request"
        );
    }
    // A different tab is a different request.
    gate.observe(Some(TabId(9)), matured);
    assert_eq!(
        gate.due(matured),
        None,
        "which starts its own quarter second"
    );
    assert_eq!(
        gate.due(matured + profiles::CHEVRON_HOVER_OPEN_DELAY),
        Some(TabId(9))
    );
}

/// **T5 — every pane a person put there may become a tab of its own; only a
/// leaf this build cannot name may not.**
///
/// The predicate that used to answer I106 by refusing three kinds out of
/// four. I106 was never a ruling — it was a *limit*, and its own note said
/// so: "I106 does not say such a pane may not leave its tab; it says it must
/// become a **files tab**". The sessionless-tab slice built that, so the
/// limit is spent and the answer flips for the two kinds a person can point
/// at.
///
/// `Placeholder` stays `false`, and it is the reason this function survives
/// rather than deleting itself the way the old note predicted. A placeholder
/// is a leaf whose kind this build did not recognise when it read the session
/// file; a tab made of one would be a strip entry that can say nothing about
/// itself, name nothing and be reopened as nothing. Refusing it is the same
/// sentence §1.1 already writes about never silently promoting one into a
/// terminal.
///
/// Red gate: answer `false` for Files or Preview and the two new tab shapes
/// are unreachable through the one door that offers them — the strip drag —
/// while `survey_strip` goes on drawing no caret over a pane the release
/// could now perfectly well host. Answer `true` for `Placeholder` and an
/// unreadable leaf becomes a tab nothing can name.
#[test]
fn every_pane_but_an_unreadable_one_can_become_a_tab_of_its_own() {
    assert!(
        pane_can_become_a_tab(bt_layout::SeatKind::Terminal),
        "a terminal pane brings a shell with it"
    );
    assert!(
        pane_can_become_a_tab(bt_layout::SeatKind::Files),
        "T5: a files column becomes a files-root tab, identified by its folder"
    );
    assert!(
        pane_can_become_a_tab(bt_layout::SeatKind::Preview),
        "T5: a preview becomes a preview-root tab, identified by its file"
    );
    assert!(
        !pane_can_become_a_tab(bt_layout::SeatKind::Placeholder),
        "a leaf this build cannot name is a tab this build cannot name"
    );
}

/// PIN — a restored column comes back open and *re-reads*, rather than
/// coming back to rows it has no business believing.
///
/// The expansion set crosses the disk; the directories do not. So the first
/// walk after a restore has to name every restored folder as a question,
/// which is what turns "I left it open here" into rows again.
#[test]
fn a_restored_expansion_comes_back_as_questions_and_not_as_rows() {
    let (seats, _, _, files, _preview) =
        revive_plan(&saved_files_and_terminal(bt_persist::FilesLeafV1 {
            view: bt_persist::FilesViewV1::Files,
            root: "D:\\work".to_owned(),
            open: vec!["/src".to_owned()],
            sel: Some("/src/main.rs".to_owned()),
            width: 240,
            remotes_open: false,
        }));
    let terminals = seats.terminals();
    let focused = terminals[0];
    let sessions: BTreeMap<SeatId, LeafSession> = terminals
        .iter()
        .map(|s| (*s, leaf_saying("SHELL")))
        .collect();
    let (layout, overflow) = cross_solve(&seats);
    let mut tab = assemble_tab_state(
        TabId(1),
        sessions,
        files,
        preview::PreviewPool::default(),
        PreviewPanes::default(),
        BTreeMap::new(),
        focused,
        TabSeed::default(),
        seats,
        layout,
        overflow,
    );
    let seat = tab.seats.files()[0];
    assert_eq!(
        tab.files_tree_walk(None)[&seat].1,
        vec![String::new()],
        "nothing is known yet, so the root is the only question there is"
    );
    assert_eq!(
        tab.files_state(seat).sel.as_deref(),
        Some("/src/main.rs"),
        "and the restored selection is still there, unproven and unharmed"
    );

    tab.file_trees
        .entry(seat)
        .or_default()
        .accept("", listed(vec![dir_entry("src", true)]));
    assert_eq!(
        tab.files_tree_walk(None)[&seat].1,
        vec!["/src".to_owned()],
        "once the root lands, the folder it was left open at is asked for"
    );
}

/// **N159, on the other table — a files column crossing into another tab
/// keeps what it was looking at, and is re-keyed on the way in.**
///
/// The merge renumbers the arriving tab's seats, which is why `arrived` is a
/// list of pairs. A session is moved across under its new id; so must a
/// root, or the column arrives blank and the user watches a folder they
/// chose turn into nothing because the pane changed tabs.
///
/// Red gate: drop the `files` half of `absorb_tab_sessions`'s loop and the
/// root assertion fails while every terminal assertion stays green — which
/// is exactly how the bug would reach a release.
#[test]
fn a_merge_carries_a_files_columns_root_across_the_renumbering() {
    let mut source = tab_with_a_files_column(1, r"D:\notes");
    let mut target = cross_tab(2, &["ALPHA"]);
    let [column] = source.seats.files()[..] else {
        panic!("one column");
    };
    let source_terminal = source.seats.terminals()[0];

    // The renumbering a real merge would hand over: both leaves land on new
    // ids in the target's tree.
    let landed_column = SeatId(90);
    let landed_terminal = SeatId(91);
    absorb_tab_sessions(
        &mut source,
        &mut target,
        &[(column, landed_column), (source_terminal, landed_terminal)],
    );

    assert_eq!(
        target
            .files
            .get(&landed_column)
            .map(|state| state.root.clone()),
        Some(r"D:\notes".to_owned()),
        "the root travelled, and under the id the pane now answers to"
    );
    assert!(
        source.files.is_empty(),
        "T226: the merge took the whole tree, folders included"
    );
    assert!(
        target.sessions.contains_key(&landed_terminal),
        "and the shell beside it moved the same way"
    );
}

/// **N158 — an unpinned tab cannot take a slot ahead of the pinned run.**
///
/// Four cases, and each is a different way the clamp can be wrong. With no
/// pinned tabs there is no partition and the pointer's answer stands
/// untouched — a clamp that always pushed forward would make every drop land
/// one slot late. With a pinned run, a slot *inside* it is pushed to the
/// first free one and a slot after it is left alone. And a raw slot past the
/// end is the ordinary answer `insert_index_at` gives for a pointer to the
/// right of every tab, which must stay an append rather than being clamped
/// off the end of the run.
///
/// Red gate: clamp with `min(lead)` instead of `max` and every drop lands in
/// slot 0; bound the top at `len - 1` and a pane let go past the last tab
/// lands second-to-last forever.
#[test]
fn an_arriving_tab_is_clamped_behind_the_pinned_run() {
    assert_eq!(strip_insert_slot(0, &[false, false, false]), 0);
    assert_eq!(strip_insert_slot(2, &[false, false, false]), 2);
    assert_eq!(
        strip_insert_slot(0, &[true, true, false]),
        2,
        "N158: 你放在槽 0 的 tab 会被弹到 pinned run 之后"
    );
    assert_eq!(strip_insert_slot(1, &[true, true, false]), 2);
    assert_eq!(
        strip_insert_slot(3, &[true, true, false]),
        3,
        "past the end is an append, not an overflow"
    );
    assert_eq!(strip_insert_slot(9, &[true, false]), 2);
    assert_eq!(
        strip_insert_slot(0, &[true, true]),
        2,
        "a strip that is all pins still accepts an arrival at its end"
    );
}

/// **I103/T226 — closing the last *seat* is closing the tab, and nothing
/// else is.**
///
/// This rule had a second clause for as long as a tab was required to hold a
/// shell: closing the last Terminal closed the tab too, however many panes
/// survived it, because what would have been left — a files column, a
/// preview, and a `focused_leaf` naming a seat with nothing behind it — was
/// I106's crash. The sessionless-tab slice made that leftover a **legal tab**
/// (`docs/DESIGN.md` §7.1.6h), so the clause is now a rule that closes a tab
/// the user did not ask to close: the shell in a `[files | shell]` tab exits,
/// and the folder you were reading goes with it.
///
/// One clause left, and it is T226's own: an empty tab is not a state that
/// exists, so the last seat leaving takes the tab with it. Every other close
/// is an ordinary pane close, whichever kind the pane is and whether or not
/// it was the last one running anything.
///
/// Red gate: put the shell clause back and closing the shell of a
/// files-plus-shell tab takes the column with it — the ruling's own "关最后
/// 一个座位 = 关 tab" read as "关最后一个终端". Drop the pane clause and the
/// last pane of a tab refuses to close instead of closing the tab.
#[test]
fn closing_the_last_seat_closes_the_tab_and_the_last_shell_does_not() {
    assert!(
        !closing_this_pane_closes_the_tab(3),
        "an ordinary fleet just loses a pane"
    );
    assert!(
        closing_this_pane_closes_the_tab(1),
        "T226: the last pane closing is the tab closing, whatever kind it is"
    );
    assert!(
        !closing_this_pane_closes_the_tab(2),
        "T5: the last shell leaving a two-pane tab leaves a sessionless tab, \
             not a closed one"
    );
}

// ── T5 / §7.1.6h: tabs made of a folder or a file, with no shell ─────────

/// **T5 — a files column torn into the strip is a tab identified by its
/// folder.**
///
/// The first of the two shapes the sessionless slice adds, through the door
/// it uses: [`tear_pane_into_tab`], which is the same function the strip drag
/// and the `Move pane to new tab` row both run. What arrives is a tab with an
/// **empty `sessions` map** — the sentence that was unstateable while I106
/// stood — carrying the column's own root, so the folder you were reading is
/// the folder the new tab is standing in and not a fresh unrooted column.
///
/// The name is the column's own name and not a second rule invented for the
/// strip: `files_head_name`'s last segment (B14), the same string the pane
/// head prints. The mark is the same folder glyph for the same reason — "给
/// 同一个对象发明第二套词汇" is the thing §7.1.6b′ names as the start of a
/// mode talking two languages, and a tab is no more entitled to a second
/// vocabulary than a card is.
///
/// The tab it left is checked too, because a tear-out makes two tabs: the
/// shell that stayed is still there, still saying what it was saying.
///
/// Red gate: refuse Files in `pane_can_become_a_tab` and the tear answers
/// `None`; drop the files state from `pane_into_new_tab` and the new tab is a
/// column pointed at nothing; leave the title reading through the old deref
/// and this panics on a tab that has no focused shell.
#[test]
fn a_files_column_torn_out_is_a_tab_with_no_shell_named_by_its_folder() {
    let mut source = tab_with_a_files_column(1, "D:\\work\\folio");
    let column = source.seats.files()[0];
    let torn = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        column,
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("T5: a files column may become a tab of its own");

    assert!(
        torn.sessions.is_empty(),
        "a files-root tab holds no shell at all"
    );
    assert_eq!(torn.seats.pane_count(), 1, "and exactly the one pane");
    assert_eq!(
        torn.files
            .values()
            .map(|state| state.root.as_str())
            .collect::<Vec<_>>(),
        vec!["D:\\work\\folio"],
        "carrying the root the column was standing on"
    );
    assert_eq!(
        torn.display_title(),
        "folio",
        "B14: named by its folder's last segment, the same string its head prints"
    );
    assert_eq!(
        torn.tab_mark(&BTreeMap::new()),
        marks::ChromeMark::Folder,
        "and wearing the same folder mark, not a shell's"
    );

    assert!(
        !source.sessions.is_empty(),
        "the tab it left keeps the shell that stayed"
    );
    assert!(source.sessions_match_terminals());
    assert!(source.files_match_files_seats());
    assert!(
        torn.files_match_files_seats(),
        "A3: and so does the tab it became"
    );
}

/// **T5 — a preview torn into the strip is a tab identified by its file.**
///
/// The second shape, and the half that had to carry more than a root: a
/// preview pane's content is a *buffer*, and §7.1.3 rules the pool belongs to
/// the tab. So the pane crosses with the buffer it was showing, exactly as
/// `absorb_tab_sessions` already carries one the other way — the same object,
/// unsaved edits and all, because "两个手势看起来一样而在用户的工作是否幸存
/// 上不同" is the thing this file refuses everywhere else.
///
/// Red gate: leave `PreviewPool::default()` in `pane_into_new_tab` and the
/// pane arrives pointing at a path its new tab has no buffer for — the pane
/// draws empty and the edits are in memory with nothing on screen able to
/// reach them, which is P126's own bug in a new place.
#[test]
fn a_preview_torn_out_is_a_tab_with_no_shell_named_by_its_file() {
    let (mut source, pane) = tab_with_a_preview(
        1,
        vec![buffer_saying("D:\\work\\notes.md", "notes.md", "hello")],
    );
    let torn = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        pane,
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("T5: a preview may become a tab of its own");

    assert!(
        torn.sessions.is_empty(),
        "a preview-root tab holds no shell at all"
    );
    assert_eq!(torn.seats.pane_count(), 1);
    let landed = torn.seats.preview_seats();
    assert_eq!(landed.len(), 1, "the pane crossed as a preview seat");
    assert_eq!(
        torn.preview_showing(landed[0]),
        Some(preview::PreviewSource::file("D:\\work\\notes.md")),
        "still showing the file it was showing"
    );
    assert_eq!(
        torn.display_title(),
        "notes.md",
        "P15: named by the file, the same string the pane head prints"
    );
    assert_eq!(
        torn.tab_mark(&BTreeMap::new()),
        marks::ChromeMark::File,
        "and wearing the file mark"
    );
    assert!(
        !source.sessions.is_empty(),
        "the tab it left keeps the shell that stayed"
    );
}

/// **§7.7 ② — the strip row of a tab whose identity seat is hosting a page
/// wears the globe, and the seat beside it does not** (W2 slice ③ ④).
///
/// `tab_mark` takes the window's whole set of pages rather than one, because
/// slice ③ gave the window a page *per pane* and a window with two pages open
/// on two tabs has to answer for both strip rows at once. What makes that
/// safe is that the set names [`LeafId`]s (F1b′) — the tab is part of the
/// name, so membership is already the per-tab question.
///
/// The third assertion is the one that says so, and it is the live defect
/// rather than an invented case: it hands the tab a page on **another tab's
/// seat 1** while this tab's own identity seat is also 1, which is exactly
/// what a window holding the same `.html` in two tabs produces.
///
/// Red gate: ask the set about the *focused* leaf rather than the identity
/// seat and the first assertion still passes on this one-pane tab while the
/// strip goes wrong for every split; answer the globe for any non-empty set
/// and the third assertion fails; drop the tab out of the key and the third
/// assertion fails too, with a folder tab wearing a browser's globe.
#[test]
fn a_tab_identified_by_a_seat_that_is_hosting_a_page_wears_the_globe() {
    let (mut source, pane) = tab_with_a_preview(
        1,
        vec![buffer_saying("D:\\work\\notes.md", "notes.md", "hello")],
    );
    let torn = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        pane,
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a preview may become a tab of its own");
    let seat = torn.seats.preview_seats()[0];
    let here = LeafId { tab: torn.id, seat };

    assert_eq!(
        torn.tab_mark(&BTreeMap::from([(here, None)])),
        marks::ChromeMark::Globe { favicon: None },
        "the pane is hosting a page, so the strip row says page"
    );
    // **And the strip wears the site's own icon where the page has one**
    // (the favicon slice, `docs/DESIGN.md` §7.13). The map is the same map — "which leaves hold a
    // page" and "what each of those pages wears" are one fact — so this is
    // the third assertion of the same sentence rather than a second door.
    //
    // MUTATION: drop the favicon out of `tab_mark`'s `Preview` arm and this
    // fails while the two either side of it still pass, which is exactly the
    // shape of a strip that has stopped agreeing with the head under it.
    let icon = favicon::FaviconId::for_tests(3);
    assert_eq!(
        torn.tab_mark(&BTreeMap::from([(here, Some(icon))])),
        marks::ChromeMark::Globe {
            favicon: Some(icon)
        },
        "the strip row draws what the head under it draws"
    );
    assert_eq!(
        torn.tab_mark(&BTreeMap::new()),
        marks::ChromeMark::File,
        "and says file again the moment the page leaves that pane"
    );
    let another_tabs_page = LeafId {
        tab: TabId(torn.id.0 + 1),
        seat,
    };
    assert_eq!(
        torn.tab_mark(&BTreeMap::from([(another_tabs_page, None)])),
        marks::ChromeMark::File,
        "a page on another tab's seat is not this tab's page — and the seat \
             number is deliberately the same one, because that is the state a \
             window holding one .html in two tabs is actually in"
    );
}

/// **T5 — a folder tab and a file tab survive a restart, through the same two
/// functions every other tab crosses the disk with.**
///
/// The write half is `Seats::to_persisted` fed by the tab's own two readers,
/// which is literally the expression `session_snapshot` builds each `TabV1`
/// from; the read half is [`revive_plan`]. Both are asked here rather than
/// hand-building a `TabV1`, because the failure this guards against is the
/// two halves disagreeing about a tree they have never had to carry before.
///
/// **The tree is the whole of the new persistence**, and that is the finding
/// rather than an omission: a `files` leaf has carried its root since v1 and
/// the content section has carried a preview's file since it was added, so a
/// term-less tab was *expressible* on disk long before it was constructible
/// in the program. What had to change was the reader — `from_persisted`
/// answered `None` for a tree with no Term leaf and `revive_plan` answered
/// that `None` with a lone terminal, so a folder tab came back as an empty
/// shell. The one thing the schema genuinely gained (v7 → v8) is the vault's
/// third seed shape, which is pinned in `bt-persist`'s own round trip.
///
/// Red gate: restore the `unwrap_or_else(lone_terminal)` and the first block
/// comes back holding a terminal seat and no column at all.
#[test]
fn a_folder_tab_and_a_file_tab_cross_the_disk_and_come_back_as_themselves() {
    let mut source = tab_with_a_files_column(1, "D:\\work\\folio");
    let column = source.seats.files()[0];
    let folder = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        column,
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a files column may become a tab of its own");

    let saved = TabV1 {
        root: folder
            .seats
            .to_persisted(&|seat| folder.term_leaf(seat, false), &|seat| {
                folder.files_state(seat)
            }),
        pinned: folder.pinned,
        focused_leaf: format!("leaf-{}", focus_leaf_index(&folder.seats)),
        preview: folder.preview_content(),
    };
    assert!(
        matches!(saved.root, LayoutNodeV1::Leaf(LeafNodeV1::Files(_))),
        "a folder tab writes one files leaf and nothing else: {:?}",
        saved.root
    );
    let (seats, _, leaves, files, _) = revive_plan(&saved);
    assert!(
        seats.terminals().is_empty(),
        "and comes back with no shell invented for it"
    );
    assert!(
        leaves.is_empty(),
        "so there is no leaf to seed a shell from"
    );
    assert_eq!(seats.files().len(), 1);
    assert_eq!(
        files
            .values()
            .map(|state| state.root.as_str())
            .collect::<Vec<_>>(),
        vec!["D:\\work\\folio"],
        "standing on the folder it was standing on"
    );
    assert_eq!(
        seats.identity(),
        seats.files()[0],
        "and identified by it, which is what names the tab"
    );

    // The other shape, through the same two functions.
    let (mut source, pane) = tab_with_a_preview(
        2,
        vec![buffer_saying("D:\\work\\notes.md", "notes.md", "hello")],
    );
    let file = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        pane,
        TabId(10),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a preview may become a tab of its own");
    let saved = TabV1 {
        root: file
            .seats
            .to_persisted(&|seat| file.term_leaf(seat, false), &|seat| {
                file.files_state(seat)
            }),
        pinned: file.pinned,
        focused_leaf: format!("leaf-{}", focus_leaf_index(&file.seats)),
        preview: file.preview_content(),
    };
    assert!(
        matches!(saved.root, LayoutNodeV1::Leaf(LeafNodeV1::Preview(_))),
        "a file tab writes one preview leaf and nothing else: {:?}",
        saved.root
    );
    assert_eq!(
        saved
            .preview
            .as_ref()
            .expect("a file tab carries a content section")
            .panes
            .iter()
            .filter_map(|pane| pane.cur.as_deref())
            .collect::<Vec<_>>(),
        vec!["D:\\work\\notes.md"],
        "with the file it was on beside the tree, where content lives (L1)"
    );
    let (seats, _, leaves, _, preview) = revive_plan(&saved);
    assert!(seats.terminals().is_empty() && leaves.is_empty());
    assert_eq!(seats.preview_seats().len(), 1);
    assert_eq!(
        preview.cur.values().collect::<Vec<_>>(),
        vec![&preview::PreviewSource::file("D:\\work\\notes.md")],
        "and back on that file"
    );
}

/// **§7.1.6k — the tab whose last pane leaves does not close; it is emptied
/// by a move, and the window takes its entry out.**
///
/// *"原 tab 若因此空了,按现有「关最后一个座位 = 关 tab」的既有规矩走"* — and
/// the existing rule is [`closing_this_pane_closes_the_tab`], which says the
/// tab goes rather than the pane being refused (G84 would refuse it). What it
/// must **not** be is [`Runtime::close_tab`]: that files a tab into Recent and
/// shuts its shells down, and this shell is alive in another tab.
///
/// Red gate: let `close_seat` be called for the lone pane and it refuses
/// (G84), so the move either does nothing or leaves the pane in two trees;
/// answer `source_emptied: false` and the window leaves an empty tab standing
/// in the strip, which §2.1 says is not a state that exists.
#[test]
fn a_lone_pane_moved_away_empties_its_tab_rather_than_being_refused() {
    let mut from = cross_tab(1, &["ALPHA"]);
    let mut into = cross_tab(2, &["GAMMA"]);
    let travelling = from.seats.terminals()[0];
    assert!(
        closing_this_pane_closes_the_tab(from.seats.pane_count()),
        "the existing rule is what decides this, not a new one"
    );

    let moved = cross_move(
        &mut from,
        &mut into,
        travelling,
        seats::DropEdge::Bottom,
        true,
    )
    .expect("the last pane of a tab may still be moved");
    assert!(moved.source_emptied, "so its tab has to leave the strip");
    assert!(
        from.sessions.is_empty(),
        "T226: it is leaving with no shell filed under it — the shell moved"
    );
    assert_eq!(
        tab_texts(&into),
        vec!["GAMMA".to_string(), "ALPHA".to_string()],
        "and it is running in the tab it was dropped on"
    );
    assert_eq!(
        into.focused_leaf, moved.landed,
        "which is the tab on screen here, so the keyboard follows the pane"
    );
}

/// **Cell ② — the pane that was alone in a tab, dragged back.**
///
/// The user's own report (#187): a preview pane torn out into a tab of its
/// own and then dropped back on the tab it came from came back **empty**.
///
/// Two things were wrong and both are here. `pane_into_tab` left the shown
/// buffer behind (cell ①), and the whole-pool rule read `preview_seats()` on
/// a tree whose seat had deliberately not been closed — so a tab that is
/// about to be taken out of the strip answered "I still have a door onto my
/// pool", and everything in it went out with the tab.
///
/// Red gate: drop the `source_emptied ||` from `source_stranded` and the
/// second assertion goes red (the history is dropped with the tab); with the
/// target holding no preview seat of its own, the other arm fires and moves
/// the *target's* pool into the dying tab instead.
#[test]
fn the_lone_preview_pane_of_a_tab_brings_its_whole_pool_back_with_it() {
    let (mut origin, preview_seat) = tab_with_a_preview(
        1,
        vec![
            edited_buffer("D:\\work\\notes.md", "notes.md", "hello", " and unsaved"),
            buffer_saying("D:\\work\\read.md", "read.md", "history"),
        ],
    );
    // The gesture that made the tab: `Move pane to new tab`, which is also
    // the drag onto the strip — one verb, [`pane_into_new_tab`].
    let mut alone = tear_pane_into_tab(
        &mut origin,
        &cross_metrics(),
        preview_seat,
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a preview may become a tab of its own");
    let lone_seat = alone.seats.preview_seats()[0];
    assert_eq!(
        document_on(&alone, lone_seat),
        Some("hello and unsaved"),
        "cell ④'s door already carried it — this is the starting state, not \
             the claim"
    );

    let mut back = cross_tab(2, &["GAMMA"]);
    let moved = cross_move(
        &mut alone,
        &mut back,
        lone_seat,
        seats::DropEdge::Right,
        true,
    )
    .expect("the last pane of a tab may still be moved");
    assert!(
        moved.source_emptied,
        "so the tab it left is about to leave the strip — which is the \
             whole reason its pool may not be left in it"
    );

    assert_eq!(
        document_on(&back, moved.landed),
        Some("hello and unsaved"),
        "the document is on the glass again, unsaved edit intact"
    );
    assert_eq!(
        back.preview_pool
            .get(&preview::PreviewSource::file("D:\\work\\read.md"))
            .and_then(|buffer| buffer.content.as_deref()),
        Some("history"),
        "and the history behind it came too: 若是原 tab 最后一个预览 pane \
             则整池随行"
    );
    assert_eq!(
        back.preview_pool.len(),
        2,
        "two buffers, one door onto them"
    );
}

/// **Cell ④ — the tear-out and the tear-out-to-a-window, which are one
/// door.**
///
/// `Move pane to new tab`, the drag onto this window's tab strip and
/// `Move pane to new window` all reach [`pane_into_new_tab`] — the last of
/// them by promoting the pane into a tab and then handing that whole
/// `TabState` to `transfer_tab`, so what the pane carries across a window
/// boundary is exactly what it carries here. This cell exists to say so and
/// to keep it said.
///
/// Red gate: take the `preview_pool.take` out of `pane_into_new_tab` and the
/// torn-out tab shows nothing.
#[test]
fn a_preview_torn_into_a_tab_of_its_own_carries_the_document_and_the_history() {
    let (mut origin, preview_seat) = tab_with_a_preview(
        1,
        vec![
            edited_buffer("D:\\work\\notes.md", "notes.md", "hello", " and unsaved"),
            buffer_saying("D:\\work\\read.md", "read.md", "history"),
        ],
    );
    origin
        .preview_panes
        .entry(seat_of(TabId(1), preview_seat))
        .scroll = [0.0, 640.0];

    let torn = tear_pane_into_tab(
        &mut origin,
        &cross_metrics(),
        preview_seat,
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a preview may become a tab of its own");
    let seat = torn.seats.preview_seats()[0];

    assert_eq!(
        document_on(&torn, seat),
        Some("hello and unsaved"),
        "the document rode across"
    );
    assert_eq!(
        torn.preview_panes
            .get(torn.preview_here(seat))
            .expect("the torn pane")
            .scroll,
        [0.0, 640.0],
        "and the reader is where the reader was"
    );
    assert_eq!(
        torn.preview_pool.len(),
        2,
        "with the history behind it, because the tab it left has no preview \
             pane to reach the pool from"
    );
    assert_eq!(origin.preview_pool.len(), 0, "and it is a move");
}

/// RED ① — **the tab a picture left stops drawing it**
/// (user report on `next22`, defects #202/#204).
///
/// The tab named one seat as the holder of `bt_render`'s single
/// `set_preview_image` slot, and the only doors that wrote that name were
/// the ones that *land* a view on a surface and the constructor. A **move**
/// is neither: it changes which panes a tab holds without landing anything,
/// so the tab a picture was torn out of went on naming a surface it no
/// longer has, and the picture dragged back met its own departed address as
/// an incumbent. The name is gone with the slot (§7.1.6k⁷) and what is
/// asserted here is the property it was standing in for, read off the panes
/// themselves.
///
/// RED GATE: make [`TabState::seat_pictures`] answer from a remembered seat
/// rather than from the panes and this goes red — the tab left behind draws
/// a picture it has not got.
#[test]
fn the_tab_a_picture_left_stops_drawing_it() {
    let (mut origin, picture_seat) = tab_with_a_picture(1, SHOT_PATH);
    assert_eq!(
        pictures_drawn(&origin),
        vec![Path::new(SHOT_PATH)],
        "the starting state: the tab is drawing the picture"
    );

    let torn = tear_pane_into_tab(
        &mut origin,
        &cross_metrics(),
        picture_seat,
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a picture may become a tab of its own");

    assert_eq!(
        pictures_drawn(&torn),
        vec![Path::new(SHOT_PATH)],
        "the tab it became draws it"
    );
    assert!(
        pictures_drawn(&origin).is_empty(),
        "and the tab it left has no picture to draw"
    );
}

/// RED ② — **the picture comes back with its pane** (defects #202/#204, the
/// gesture the user actually reported).
///
/// Open a `.png` on a pane, tear the pane into a tab of its own, drag it
/// back. Both halves of the pane's address changed twice, and the tab it
/// returns to used to still be naming the address it had the first time — so
/// `get_or_insert` found an incumbent, kept it, and the arriving picture was
/// filtered straight out of [`Runtime::preview_picture_hosts`]. On the glass
/// that is a blank body under a head and a fact line that travelled with the
/// pane: *「1440 × 900 · PNG · 36 KB · Fit」* over nothing.
///
/// RED GATE: filter [`TabState::seat_pictures`] down to one remembered seat
/// and the returning pane is not among the pictures this tab draws.
#[test]
fn a_picture_dragged_back_into_the_tab_it_left_is_drawn_again() {
    let (mut origin, picture_seat) = tab_with_a_picture(1, SHOT_PATH);
    let mut alone = tear_pane_into_tab(
        &mut origin,
        &cross_metrics(),
        picture_seat,
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a picture may become a tab of its own");
    let lone_seat = alone.seats.preview_seats()[0];

    let moved = cross_move(
        &mut alone,
        &mut origin,
        lone_seat,
        seats::DropEdge::Right,
        true,
    )
    .expect("the lone pane of a tab may still be moved");

    assert_eq!(
        origin.seat_pictures(),
        vec![seat_of(TabId(1), moved.landed)],
        "the tab draws the pane the picture is standing in now"
    );
    assert_eq!(
        pictures_drawn(&origin),
        vec![Path::new(SHOT_PATH)],
        "which is the picture being on the glass again rather than a head \
             and a fact line over nothing"
    );
}

/// PIN (user ruling 2026-08-25, B10) — **a tab switch leaves no menu
/// standing.**
///
/// `Ctrl+Tab` was already immediate — [`stepped_tab`] answers on the key
/// down, there is no held-modifier overlay to linger, and the preview's own
/// switcher shuts itself on every row it answers. What did linger was
/// everything E61 calls a [`Popup`]: `activate_tab` cleared eleven kinds of
/// per-tab transient and not one of the eight menus, so a `⌄` left open on
/// the tab you stepped away from was still open when you came back — drawn
/// nowhere in between, because §7.1.5a′ had already taught each of them to
/// fold to `None` off screen, which is exactly what let the state survive
/// unnoticed.
///
/// A menu is a question about the surface it was raised over, and stepping
/// to another tab is that question being dropped. So the switch takes them,
/// all eight, through the one function that knows what "all of them" means.
///
/// Red gate: take the call out of `activate_tab` and this names the line
/// that is missing.
#[test]
fn a_tab_switch_leaves_no_menu_standing() {
    assert!(
        method_body("Runtime", "activate_tab").contains("self.close_every_popup();"),
        "the tab that arrives finds no popup left over from the one that left"
    );
    // And the closer is the one that answers for the whole list, not a hand
    // written run: `close_every_popup` walks `Popup::ALL` through the same
    // `close_popup` arm `close_popups_except` walks, so a ninth popup is
    // closed here the day it compiles.
    assert!(
        method_body("Runtime", "close_every_popup").contains("for popup in Popup::ALL"),
        "the switch closes the list, not a copy of it"
    );
}

/// PIN — **`Move pane to new tab` moves the leaf; it does not start a new
/// one.**
///
/// The row's whole promise is that the shell survives: its scrollback, its
/// children, the program it is in the middle of running. A respawn would look
/// identical for the first frame and then be a pane that had lost an hour of
/// output, which is the failure this pins shut.
///
/// It asserts against [`tear_pane_into_tab`] because that is what the row
/// runs — `run_pane_menu_row` → `move_pane_to_new_tab` →
/// `extract_pane_into_new_tab` → here, the same chain the drag walks. The
/// marker bytes are the identity: a fresh `LeafSession` says nothing, so a
/// tab whose pane still says `BETA` is a tab holding the object that was
/// there before, not a copy of it.
///
/// Red gate: rebuild the session on the way across and the new tab comes up
/// blank while the assertion below still names a seat that exists.
#[test]
fn moving_a_pane_to_a_new_tab_carries_its_shell_rather_than_starting_one() {
    let mut source = cross_tab(1, &["ALPHA", "BETA"]);
    let before = source.sessions.len();
    let torn = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        SeatId(2),
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a two-pane tab can spare one");
    assert_eq!(
        tab_texts(&torn),
        vec!["BETA".to_string()],
        "the pane arrives still saying what it said — the session moved"
    );
    assert_eq!(tab_texts(&source), vec!["ALPHA".to_string()]);
    assert_eq!(
        source.sessions.len() + torn.sessions.len(),
        before,
        "one shell in, one shell out: nothing was spawned and nothing killed"
    );
    assert!(source.sessions_match_terminals());
    assert!(torn.sessions_match_terminals());
}

/// **The keyboard follows the pane out, and what stays keeps a shell to type
/// into.**
///
/// `focused_leaf` names the shell a keystroke belongs to, and the tear-out
/// can take it. Both sides are asked: the new tab types into the pane that
/// travelled, and the old tab's keyboard has moved to a leaf it still has
/// rather than staying on a seat that is gone —
/// [`TabState::refocus_after_losing`], the rule `close_pane` and this share.
///
/// Red gate: drop the `refocus_after_losing` call and `source.focused()`
/// panics on its own invariant.
#[test]
fn tearing_the_focused_pane_out_moves_the_keyboard_on_both_sides() {
    let mut source = cross_tab(1, &["ALPHA", "BETA"]);
    source.focused_leaf = SeatId(2);
    let torn = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        SeatId(2),
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a two-pane tab can spare one");
    assert_eq!(source.focused_leaf, SeatId(1));
    assert_eq!(leaf_says(&torn, torn.focused_leaf), "BETA");
    assert_eq!(leaf_says(&source, source.focused_leaf), "ALPHA");
}

/// **G84 — the last pane has nowhere to be torn to, and the attempt changes
/// nothing.**
///
/// `close_seat` refuses to empty a tree, so the gesture is a no-op rather
/// than a tab that closes behind your back. The survey never offers the
/// landing ([`Runtime::tear_out_is_hostable`] asks `tear_out`, which answers
/// `None` for the same reason), so this is the belt to that brace: even
/// called outright, the tab keeps its tree, its shell and its keyboard.
///
/// Red gate: run the session `remove` before the tree edit instead of after
/// and the lone tab comes back with no shell at all — I106 by a different
/// road.
#[test]
fn the_only_pane_of_a_tab_cannot_be_torn_out() {
    let mut source = cross_tab(1, &["ALPHA"]);
    let torn = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        SeatId(1),
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    );
    assert!(torn.is_none(), "G84: a tree may not be emptied");
    assert_eq!(source.seats.terminals(), vec![SeatId(1)]);
    assert_eq!(leaf_says(&source, SeatId(1)), "ALPHA");
    assert!(source.sessions_match_terminals());
}

/// **B3 in place: the replace is refused when the pane it would eject cannot
/// be a tab (M147).**
///
/// Asked of the pure predicate against the seat the plan would displace,
/// which is what [`Runtime::plan_for`] does with a `Seats` in hand.
///
/// **What it turns down moved with §7.1.6h.** It used to be a files column,
/// on I106's argument that ejecting one would push a tab with nothing
/// running in it into the strip; a files column ejected today becomes a
/// folder tab, which is a tab like any other. What is still refused is a
/// `Placeholder` — a leaf read off disk whose kind this build has no code
/// for — because a tab made of one could be named by nothing and reopened as
/// nothing. Refusing turns the promise dashed while the hand is still
/// moving, which is the whole of M147.
///
/// The seat the plan reads is taken from the live tree rather than from the
/// plan's, because after `ReplaceSeat` the displaced seat is not in the
/// plan's tree at all — the same ordering the commit depends on.
#[test]
fn a_replace_reads_its_refusal_off_the_seat_it_would_eject() {
    let target = cross_tab(2, &["TGTA", "TGTB"]);
    let kind = target
        .seats
        .tree()
        .find_seat(SeatId(2))
        .expect("in the live tree")
        .kind;
    assert!(
        pane_can_become_a_tab(kind),
        "a terminal pane may be ejected to the strip"
    );
    assert!(
        pane_can_become_a_tab(bt_layout::SeatKind::Files),
        "T5: and so may a files column, which becomes a folder tab"
    );
    assert!(
        !pane_can_become_a_tab(bt_layout::SeatKind::Placeholder),
        "and the same question still turns an unreadable leaf down"
    );
}

// ── §7.1.1: a row's two tab-strip verbs (user ruling 2026-07-17, built
//    2026-08-30 after the user's report #200) ─────────────────────────────

/// PIN — **§7.1.1: a row let go on the run's padding becomes a tab, and a
/// row let go on one of the run's entries opens in that tab.**
///
/// The whole tab-strip half of the file-drag ruling, as a table over the two
/// questions a [`seats::TabRun`] answers about one pointer. It is
/// [`pane_strip_landing`]'s shape deliberately: the padding is "this run
/// gains an entry" ([`DropLanding::StripExtract`]) and an entry is "a room to
/// be put in" ([`DropLanding::StripAdopt`]) — which is what lets §7.1.6k's
/// spring arm itself over a row without having to learn a row exists, since
/// [`Runtime::drive_drag`] reads the survey's answer and never the pointer.
///
/// RED EVIDENCE (user report #200, 2026-08-30): `survey_strip` answered
/// `DragSource::Row(_) => None`, so a `folio-pdf-test.pdf` dragged from the
/// tree onto the tab strip showed a ghost and did nothing when the hand
/// opened — over the padding and over every tab alike.
///
/// Mutation: fall through from a refused adopt to the new tab, and the same
/// pointer on the same tab means "open it in here" in a large window and
/// "make a new tab" in a small one — arithmetic nobody can see.
#[test]
fn a_row_makes_a_tab_on_the_padding_and_opens_in_the_entry_it_rests_on() {
    assert_eq!(
        row_strip_landing(None, false, 3),
        Some(DropLanding::StripExtract { slot: 3 }),
        "the gap between two entries and the tail past the last one are one \
             answer: a new tab at that slot"
    );
    assert_eq!(
        row_strip_landing(Some(TabId(7)), true, 3),
        Some(DropLanding::StripAdopt { tab: TabId(7) }),
        "and an entry the pointer is standing on is a room to be put in"
    );
    assert_eq!(
        row_strip_landing(Some(TabId(7)), false, 3),
        None,
        "a room that will not take it leaves nothing there, and never the \
             new tab the padding beside it would have made"
    );
}

/// PIN — **§7.1.6b′ ③ overturned (user ruling 2026-08-30): one arm, every
/// tab surface**, and the slot it reads is clamped past the pinned run.
///
/// ③ read *"一份 tab 清单没有一个非任意的 tab 可以接住一个文件"* and was
/// implemented as `DragSource::Row(_) => None` in
/// [`Runtime::survey_strip`] — global to the strip, the vertical rail and
/// the card column alike. Cards *are* tabs, so the premise went the way ①
/// and ② went: what a card column offers is what its window's tab list
/// offers. This arm is what makes that free — it asks the run and never the
/// window, so a card on a card, the seam between two cards and the blank
/// below the last one are the strip's own three answers on the column's
/// rectangles.
///
/// **The two questions moved above the `match` on 2026-09-06 and are still
/// the run's** (§7.1.6k⁷, the seam band). They used to be asked inside this
/// arm and inside the pane's, once each; the band made that a hazard rather
/// than a duplication, because the hysteresis it carries is *one* latch and
/// two readers of it would be two hysteresis with their own histories. So
/// they are asked once, of the same run, and both arriving payloads are
/// handed the answers — which is the same sentence this test has always
/// made, one indentation further out. What the arm still owns is N158's
/// clamp.
///
/// Mutation: read `window.focus_mode` here and the column grows a second set
/// of rules ①'s whole argument forbids; drop the `strip_insert_slot` and a
/// file dropped after a pinned tab makes an unpinned tab inside the pinned
/// run (N158).
#[test]
fn a_rows_strip_arm_asks_the_run_and_never_the_window() {
    let survey = method_body("Runtime", "survey_strip");
    let (asked, arms) = survey
        .split_once("match source {")
        .expect("survey_strip chooses on what is in the hand");
    assert!(
        asked.contains("run.aim("),
        "\"whose tab is under my hand\" and \"which join am I in\" are the \
             run's own questions, asked once:\n{asked}"
    );
    assert!(
        asked.contains("seats::insert_index_at("),
        "and so is \"between which two would a new one land\", for every \
             pointer the band does not claim:\n{asked}"
    );
    let arm = arms
        .split_once("DragSource::Row(payload) => ")
        .expect("survey_strip answers a row")
        .1;
    assert!(
        arm.contains("strip_insert_slot("),
        "N158: the caret the reader watches is the slot the release inserts \
             at, clamped on this run's own pinned partition:\n{arm}"
    );
    // Asked of the arm and of the questions above it, and deliberately not
    // of the whole method: the *pane* arm names `window.focus_mode` in a
    // comment explaining why it does not read it, and a test that could not
    // tell a sentence about a field from a use of it would be a test nobody
    // could write that sentence beside.
    assert!(
        !asked.contains("focus_mode") && !arm.contains("focus_mode"),
        "the surface is chosen once, in `tab_run`, and never re-read here:\n{asked}\n{arm}"
    );
}

/// PIN — **§7.1.1 ①: 「新工作区含单个预览 pane,缓冲生于新 tab 自己的池,插位钳
/// 在 pinned 分区之后」, plus the activation the user ruled on 2026-08-30.**
///
/// Four clauses, and each is one line of this commit:
///
/// * **one preview pane** — a lone `Preview` seat, which is exactly the
///   shape `reopen_recent` hands `create_tab_state` for a `Seed::Preview`;
/// * **the buffer is born in the new tab's own pool** — the tab is built
///   with an empty [`PreviewRestore`] and the page is opened *after* it is
///   on the strip and activated, through
///   [`Runtime::open_preview_onto`], the tree's own door. A pool belongs to
///   a `TabState`, and this one is new, so there is nothing to fork;
/// * **the slot is clamped past the pins** — by the survey (N158), so the
///   `min` here is the ordinary bound on an insertion index;
/// * **the tab is activated.** §7.1.1 says a folder's new tab activates
///   「即刻」 and says nothing about a file's, while N157's tear-out
///   explicitly does not. The ruling of 2026-08-30 makes the two one:
///   dragging a row out to look at it and being left looking at something
///   else is a gesture that appears to have failed.
///
/// Mutation: pass the page in on a `PreviewRestore` instead and the buffer
/// is minted before the tab is on the strip — a pool filled behind a tab
/// nothing has activated, which is the one arrangement §7.1.3's reuse rule
/// cannot be asked about.
#[test]
fn the_tab_a_file_row_makes_is_activated_and_owns_the_buffer_it_opens() {
    let text = method_body("Runtime", "commit_row_into_new_tab");
    assert!(
        text.contains("bt_layout::SeatKind::Preview"),
        "a file's tab is one preview pane:\n{text}"
    );
    assert!(
        text.contains("&PreviewRestore::default()"),
        "born with an empty pool:\n{text}"
    );
    let opened = text
        .find("self.open_preview_onto(surface, payload.path.clone())")
        .expect("the page goes through the ordinary door");
    let activated = text
        .find("self.activate_tab(slot, true)?")
        .expect("and the tab the user dragged a file out to see is the one on screen");
    assert!(
        activated < opened,
        "the tab is standing and on the stage before its pool is asked for \
             anything, which is what makes the buffer the *new* tab's:\n{text}"
    );
    assert!(
        text.contains("pinned: false"),
        "N158: a tab a gesture just made is not one the user promised to \
             bring back every time:\n{text}"
    );
    assert!(
        text.contains("slot.min(self.window.tabs.len())"),
        "the survey already clamped the partition; this is the ordinary \
             bound on an insertion index:\n{text}"
    );
}

/// PIN — **§7.1.1: 「目录行拖到标签条 = 新 files tab(即刻激活)」**, rooted
/// where the row was dragged from and as wide as a fresh column opens.
///
/// Mutation: leave the `fixed_extent` off and the column arrives taking a
/// ratio share of the tab it is the whole of — a files leaf that is not a
/// fixed column is the one shape §7.1.1's fixed-column semantics has no word
/// for.
#[test]
fn a_folder_row_makes_a_files_tab_rooted_where_it_was_dragged_from() {
    let text = method_body("Runtime", "commit_row_into_new_tab");
    assert!(
        text.contains("bt_layout::SeatKind::Files"),
        "a folder's tab is one files column:\n{text}"
    );
    assert!(
        text.contains("with_fixed_extent(bt_layout::FILES_W)"),
        "at the width a fresh column opens at (F62's 240):\n{text}"
    );
    assert!(
        text.contains("root: payload.path.display().to_string()"),
        "rooted at the folder the row named, and never at the tree it came \
             out of:\n{text}"
    );
}

/// PIN — **§7.1.1's second verb goes through the target tab's own two
/// doors** (user ruling 2026-08-30).
///
/// A file takes [`Runtime::open_preview_file`], which is the tree's
/// double-click and therefore *is* the ruling's two cases already: the tab's
/// landing preview pane if it has one, and a fresh preview at the fixed
/// right seat if it has not. A folder takes
/// [`Runtime::seat_a_files_column`], which is §7.1.1's own space verb —
/// 「裂出新 files pane 根在该目录」 — landing where `Ctrl+Shift+B` puts one:
/// the **leading** side of the root rim, at `FILES_W`.
///
/// **The folder half is deliberately not a re-root.** §7.1.1 gives a folder
/// exactly one content verb and it is at *a files pane's own centre*; a
/// strip entry is not a pane centre. Reaching the hard re-root from here
/// would also skip the question about range the ruling of 2026-08-25 put in
/// front of it — see `re_rooting_is_reached_only_after_the_question_about_
/// range`, which counts that door's callers.
///
/// Mutation: drop the activation and the file opens into a tab nobody is
/// looking at, which from the outside is the drop having done nothing.
#[test]
fn a_row_on_an_entry_opens_through_that_tabs_own_doors() {
    let text = method_body("Runtime", "commit_row_into_tab");
    assert!(
        text.contains("self.activate_tab(index, false)?"),
        "the tab you aimed at is the tab you are looking at:\n{text}"
    );
    assert!(
        text.contains("self.open_preview_file(payload.path.clone())"),
        "a file goes through the tree's own double-click:\n{text}"
    );
    assert!(
        text.contains("self.seat_a_files_column("),
        "and a folder through §7.1.1's own space verb:\n{text}"
    );
    assert!(
        !text.contains("_in_files_column(&"),
        "never the re-root doors, which name a root rather than ask for a \
             pane:\n{text}"
    );
}

// ── the window's own address door (§7.7 ⑨, Claude 定 2026-08-24) ────────

/// PIN (§7.7 ⑨) — **the door remembers what it took, so it can give it
/// back.**
///
/// The landing rule has three outcomes and a withdrawal owes a different
/// thing to each: a pane that came with the page goes with it, a pane that
/// was reading a document gets the document back, and a pane that was empty
/// is left empty. Both facts are destroyed the instant the page lands — the
/// document is cleared off the pane, and a pane minted a moment ago is
/// indistinguishable from one that was already there — so the answer has to
/// be taken before, and this is the function that takes it.
///
/// MUTATIONS:
/// ① drop the `the_pane_was_already_open` arm and read only the buffer — a
///    pane minted for the page reports `LeaveThePaneEmpty`, and Escape leaves a
///    bare preview pane standing where there was none;
/// ② return `LeaveThePaneEmpty` for a pane that was showing something — Escape
///    silently discards the document the door borrowed the pane from.
#[test]
fn the_blank_pages_door_remembers_what_it_took_so_it_can_give_it_back() {
    let notes = preview::PreviewSource::file(r"D:\notes\today.md");
    assert_eq!(
        blank_page_return(false, None),
        BlankPageReturn::TakeThePane,
        "a pane that did not exist before the page is the page's own"
    );
    assert_eq!(
        blank_page_return(false, Some(notes.clone())),
        BlankPageReturn::TakeThePane,
        "and a pane that did not exist cannot have been showing anything — \
             the landing rule is asked first, not the buffer"
    );
    assert_eq!(
        blank_page_return(true, Some(notes.clone())),
        BlankPageReturn::PutTheDocumentBack(notes),
        "a document the door borrowed a pane from is owed that pane back"
    );
    assert_eq!(
        blank_page_return(true, None),
        BlankPageReturn::LeaveThePaneEmpty,
        "and an empty pane is owed nothing but its emptiness"
    );
}

/// PIN (§7.7 ⑨) — **the field over a blank page opens empty, and nothing
/// arranges that.**
///
/// `about:blank` is this host's word for "no page yet", not the reader's
/// address, so `WebSeat` already refuses to put it in the head — the field is
/// seeded from that same empty string, and there is no second rule about
/// blank pages anywhere in the editor. The pair is pinned together because
/// the *other* seeding must not change with it: an address that is one opens
/// selected whole, because going somewhere almost always means going
/// somewhere else.
///
/// MUTATION: seed the field with `webnav::BLANK_PAGE` when the page has no
/// address — the first three assertions go red, and the first thing a reader
/// types replaces a word this window made up about itself.
#[test]
fn the_blank_pages_address_field_opens_empty() {
    let leaf = LeafId {
        tab: TabId(1),
        seat: SeatId(2),
    };
    let empty = TabRename::open_address(leaf, "");
    assert_eq!(empty.text(), "");
    assert!(empty.selection().is_empty(), "there is nothing to select");
    assert_eq!(empty.caret(), 0);
    let addressed = TabRename::open_address(leaf, "http://localhost:5173/app");
    assert_eq!(
        addressed.selection(),
        0..addressed.text().len(),
        "a page that has an address opens with the whole of it selected"
    );
    assert_eq!(addressed.caret(), addressed.text().len());
}

/// PIN (§7.7 ⑨) — **the three sentences of the door, read off the file that
/// makes them.**
///
/// A `WindowRuntime` is a surface, a compositor and a browser, so "the chord
/// minted a page and the caret landed in it" is not a sentence this process
/// can say without a screen. What can be said without one is which function
/// calls which, and this module's standing rule is that those are read as
/// text — the same witness `every_door_that_lands_a_source_opens_a_page_as_a_page`
/// takes for the pool's own fork.
///
/// The three: the chord forks on whether the tab already has a page; the arm
/// that has none goes out through the minted door rather than straight at the
/// engine; and an address field that closes without a navigation takes its
/// blank page with it, while one that navigated does not.
///
/// MUTATIONS:
/// ① point the chord at `open_web_address` — a tab with no page answers
///    nothing at all, and the first assertion goes red;
/// ② navigate to `BLANK_PAGE` directly instead of through
///    `open_minted_page` — the second goes red, and this window starts a
///    navigation that never passed a door;
/// ③ drop the `forget_a_blank_page` before the outcomes are applied — the
///    last goes red, and a page that was just sent somewhere is withdrawn out
///    from under the navigation it was given.
#[test]
fn the_windows_address_door_mints_a_page_and_can_be_taken_back() {
    let body = |name: &str| method_body("Runtime", name);

    let door = body("open_address_here");
    assert!(
        door.contains("self.page_on_the_landing_pane()")
            && door.contains("self.open_web_address_on(leaf)")
            && door.contains("self.mint_a_blank_page_and_open_its_address()"),
        "the chord does not fork on whether the pane a page would land on \
             already holds one:\n{door}"
    );

    let mint = body("mint_a_blank_page_and_open_its_address");
    assert!(
        mint.contains("self.open_minted_page(webnav::Mint::Blank)")
            && mint.contains("blank_page_return(landing.is_some(), was_showing)")
            && mint.contains("self.open_web_address_on(leaf)"),
        "the blank page is not minted through the door every navigation \
             passes, or the way back from it is not taken before it lands:\n{mint}"
    );

    let finish = body("finish_rename");
    assert!(
        finish.contains("self.withdraw_a_blank_page(leaf)?")
            && finish.contains("self.forget_a_blank_page(leaf);")
            && finish.contains("!editor.text().trim().is_empty()"),
        "an address field that closes does not answer for the blank page it \
             was opened over:\n{finish}"
    );
}

/// PIN (§7.7 W2 片④ ①, user report 2026-08-24) — **an address the door
/// refuses is a field you can still leave.**
///
/// 「同一组 Enter/Escape/blur、同一种沉默的拒绝」 names four things and the
/// refusal is one of them, not all of them: 「回车什么都不做,页面原地不动」
/// is a sentence about **Enter**, and blur is not a keystroke — it is having
/// left already. A `file:` page's address is refused by `address_bar` on
/// every attempt, so an editor that answers a *blur* with the refusal is an
/// editor no gesture can close: Enter re-opens it, a press on the chrome
/// re-opens it, losing the window re-opens it, and a press on the page —
/// which returns above the blur guard and hands the keyboard to the engine —
/// leaves it standing with nothing that can reach it. That is the report,
/// exactly: 「html 文件改名一旦进入这个状态就出不来」.
///
/// Read off the file for this module's standing reason: a `WindowRuntime` is
/// a surface, a compositor and a browser, so "Escape closed the field" is not
/// a sentence this process can say without a screen. Which exit may keep a
/// refused field, and which call stands before the engine takes the keyboard,
/// are both claims about the code.
///
/// MUTATIONS:
/// ① widen the re-open guard back to every committing exit — the first
///    assertion goes red, and a blur on a refused address re-opens the field
///    it was leaving;
/// ② drop the `finish_rename` at the top of `press_web_page` — the second
///    goes red, and the field the press blurred is left standing over a page
///    that has just taken every key away from it.
#[test]
fn a_refused_address_is_a_field_that_can_still_be_left() {
    let finish = method_body("Runtime", "finish_rename");
    assert!(
        finish.contains("exit.may_stay_open()")
            && finish.contains("self.window.rename = Some(editor);"),
        "the refusal that keeps an address field open is not fenced to \
             Enter, so blur cannot leave a `file:` page's address:\n{finish}"
    );

    let press = method_body("Runtime", "press_web_page");
    let blur = press
        .find("self.finish_rename(RenameExit::Blur)?")
        .unwrap_or(usize::MAX);
    let takes = press.find("web.focus_page()").unwrap_or(0);
    assert!(
        blur < takes,
        "a press inside a page takes the keyboard without settling the field \
             standing over it, which leaves an editor nothing can reach:\n{press}"
    );
}

/// The names of a carried environment — what these tests assert and print, never a value.
fn carried_names(environment: Option<&cli::CarriedEnvironment>) -> Option<Vec<String>> {
    environment.map(|environment| {
        environment
            .pairs()
            .iter()
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect()
    })
}

/// RED (F-SWEEP-2-048, coordinator's ruling 2026-10-09) — **the environment `--with-environment`
/// carried belongs to the tab**: the tab holds it, and every shell born in the tab is born with
/// it, whatever its own seed said; a revived tab holds none, because it was never saved.
///
/// MUTATIONS: `born_in_tab` keeping the seed's own environment and a split of a tab carrying one
/// is born without it; `assemble_tab_state` dropping `TabSeed::carried_environment` and the tab holds
/// nothing to give.
#[test]
fn a_tabs_carried_environment_is_the_tabs_and_every_shell_born_in_it_takes_it() {
    let carried =
        cli::CarriedEnvironment::from_pairs(vec![("FSWEEP2_TAB_环境".into(), "1".into())]);
    let seats = cross_seats(1);
    let focused = seats.identity();
    let (layout, overflow) = cross_solve(&seats);
    let tab = assemble_tab_state(
        TabId(1),
        BTreeMap::from([(focused, leaf_saying("SHELL"))]),
        BTreeMap::new(),
        preview::PreviewPool::default(),
        PreviewPanes::default(),
        BTreeMap::new(),
        focused,
        TabSeed {
            carried_environment: Some(carried.clone()),
            ..TabSeed::default()
        },
        seats,
        layout,
        overflow,
    );
    assert_eq!(
        carried_names(tab.carried_environment.as_ref()),
        Some(vec!["FSWEEP2_TAB_环境".to_owned()]),
        "the tab holds what was carried into it"
    );
    // A split, a duplicate and a restart are each a seed born in the tab.
    let split = SplitSeed::Inherit.applied("pwsh", None);
    assert_eq!(
        split.carried_environment, None,
        "a split's own seed carries nothing"
    );
    for (verb, seed) in [
        ("split", split),
        ("Restart shell", restart_seed("pwsh", None)),
    ] {
        assert_eq!(
            carried_names(
                born_in_tab(seed, tab.carried_environment.as_ref())
                    .carried_environment
                    .as_ref()
            ),
            Some(vec!["FSWEEP2_TAB_环境".to_owned()]),
            "{verb} in the tab is born with the tab's environment"
        );
    }
    // A tab no launch carried one into gives its shells the account's environment.
    let launched = LeafSeed {
        carried_environment: Some(carried),
        ..restart_seed("pwsh", None)
    };
    assert_eq!(born_in_tab(launched, None).carried_environment, None);
}

/// RED (F-SWEEP-2-048, coordinator's ruling 2026-10-09) — **every verb that starts a shell inside
/// a tab hands it the tab's carried environment**: the tab's own panes at its creation, a split
/// (also `Duplicate pane` and `Split with`), `Restart shell`, and `Duplicate tab`, which carries it
/// to the new tab.
///
/// Those verbs spawn a ConPTY and cannot run here, so this reads their bodies, as
/// `every_verb_that_starts_a_shell_in_a_panes_place_reads_the_one_ladder` does for the folder.
///
/// MUTATIONS, each red: drop it on a split (`split_seat` without `born_in_tab`), on a restart
/// (`restart_shell` without it), on a duplicate (`duplicate_tab` passing `None`), or at the tab's
/// birth (`create_tab_state` without it).
#[test]
fn every_shell_born_in_a_tab_is_born_with_the_tabs_carried_environment() {
    for door in ["restart_shell", "split_seat"] {
        let body = method_body("Runtime", door);
        let joined = body.split_whitespace().collect::<String>();
        assert!(
            joined.contains("born_in_tab(") && joined.contains(".carried_environment.as_ref()"),
            "`{door}` is born in its tab:\n{body}"
        );
    }
    let duplicate = method_body("Runtime", "duplicate_tab");
    assert!(
        duplicate.contains("state.carried_environment.clone()"),
        "`Duplicate tab` carries the tab's environment to the new tab:\n{duplicate}"
    );
    let birth = free_fn_body("create_tab_state")
        .split_whitespace()
        .collect::<String>();
    assert!(
        birth.contains("born_in_tab(") && birth.contains("seed.carried_environment.as_ref()"),
        "a tab's own panes are born with its environment:\n{birth}"
    );
}
