//! **The crate root: the storage pass's card** (T-STORE-NOTICE; design note
//! `docs/plans/design/settings-reread-2026-10-11.md` §3.6, §8 test 6, §9.5,
//! (b)11). The four real stores, on one scripted lane, driven through the pass
//! the product runs (`storage_pass::take_merges`) on a window of the test's own
//! ([`crate::test_support::StoragePassRig`]); written in the crate root's scope
//! (`use super::*`), beside `raise_merge_notice_on`.

use super::*;
use crate::document_request::{MergeNotice, ReplacedInFile};
use crate::test_support::StoragePassRig;
use bt_persist::{
    BindingOverrideV1, PINS_SCHEMA_VERSION, PROFILES_SCHEMA_VERSION, PinEntryV1, PinKind, PinsV1,
    ProfileEntryV1, ProfilesV1, ProgramV1, SettingsV1, ThemeModeV1,
};
use std::ffi::OsString;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn binding(action: &str, chord: &str) -> BindingOverrideV1 {
    BindingOverrideV1 {
        action: action.to_owned(),
        chord: Some(chord.to_owned()),
    }
}

fn keybindings_of(lines: Vec<BindingOverrideV1>) -> bt_persist::KeybindingsV1 {
    persist::keybindings_document(lines, persist::keybindings_platform())
}

fn row(id: &str, title: &str) -> ProfileEntryV1 {
    ProfileEntryV1 {
        id: id.to_owned(),
        display_title: Some(title.to_owned()),
        program: Some(ProgramV1::Path(format!("C:\\工具\\{id}.exe"))),
        ..ProfileEntryV1::default()
    }
}

fn profiles_of(rows: &[ProfileEntryV1]) -> ProfilesV1 {
    ProfilesV1 {
        schema_version: PROFILES_SCHEMA_VERSION,
        profiles: rows.to_vec(),
    }
}

fn pins_of(rows: &[PinEntryV1]) -> PinsV1 {
    PinsV1 {
        schema_version: PINS_SCHEMA_VERSION,
        pins: rows.to_vec(),
        extra: serde_json::Map::new(),
    }
}

fn settings_with(theme_mode: ThemeModeV1) -> SettingsV1 {
    SettingsV1 {
        theme_mode,
        ..SettingsV1::default()
    }
}

/// The folder a pin of the tests names, in mixed script.
const FOLDER: &str = r"C:\项目 folio";

/// **Folio sets one unit of each of the four documents, and the disk refuses
/// every write**: each stays owed — in memory, not in its file.
fn press_all_four_and_refuse(rig: &mut StoragePassRig) {
    assert!(rig.settings.store(settings_with(ThemeModeV1::Light)));
    assert!(rig.keybindings.store(vec![binding("zoom-pane", "Ctrl+i")]));
    assert!(
        rig.profiles
            .store(profiles_of(&[row("a", "甲 我们的 Ours")]))
    );
    assert!(
        !rig.pins.toggle(PinKind::Folder, FOLDER),
        "the pin is taken off"
    );
    rig.refuse_writes();
    rig.turn().expect("a turn of refusals");
    assert!(rig.cards().is_empty(), "a refused write merges nothing");
}

/// **The four files rewritten on disk, each against Folio's unwritten unit** —
/// what one `git checkout` leaves.
fn check_out_all_four(root: &Path) {
    StoragePassRig::write(
        root,
        persist::SETTINGS_FILE_NAME,
        &settings_with(ThemeModeV1::Dark),
    );
    StoragePassRig::write(
        root,
        persist::KEYBINDINGS_FILE_NAME,
        &keybindings_of(vec![binding("zoom-pane", "Ctrl+u")]),
    );
    StoragePassRig::write(
        root,
        persist::PROFILES_FILE_NAME,
        &profiles_of(&[row("a", "甲 他们的 Theirs")]),
    );
    let mut noted = PinEntryV1::new(PinKind::Folder, FOLDER);
    noted.extra.insert(
        "note".to_owned(),
        serde_json::Value::String("他们的 theirs".to_owned()),
    );
    StoragePassRig::write(root, pins::PINS_FILE_NAME, &pins_of(&[noted]));
}

/// The stores' write generations, in the watch's document order.
fn generations(rig: &StoragePassRig) -> [u64; 4] {
    [
        rig.settings.write_generation(),
        rig.keybindings.write_generation(),
        rig.profiles.write_generation(),
        rig.pins.write_generation(),
    ]
}

/// RED (design note §8 test 6; carry-forwards 1 and 2, C1) — **a checkout of
/// all four files is four merges and one notice.**
///
/// Each store has one owed unit of Folio's that its file's new contents
/// conflict with. One burst names all four documents — remove, add and modify
/// per document, 400 µs apart, as E-4 measured a `git checkout` — and the
/// folder's one clock falls due once, owing all four reads. The worker answers
/// them on four separate turns, the last asked first. Each turn merges what it
/// heard; no card is raised while a read is still to be answered; after the
/// fourth, one card names the four files in the pass's order, each with the
/// value its file won.
///
/// MUTATIONS: one card per document (in `storage_pass::take_merges`, raise a
/// card for each file of the due notice) — four cards; one clock per document
/// (in `StorageWatch::advance`, a `WatchClock` per document) — the first
/// document's read is asked, answered and merged a quiet window after its own
/// last name, and its card stands before the others are read; one card per
/// turn (in `MergeNotice::gather`, ignore `reading`) — four cards.
#[test]
fn a_checkout_of_all_four_files_is_four_merges_and_one_notice() {
    let root = StoragePassRig::scratch("checkout");
    StoragePassRig::write(
        &root,
        pins::PINS_FILE_NAME,
        &pins_of(&[PinEntryV1::new(PinKind::Folder, FOLDER)]),
    );
    let mut rig = StoragePassRig::open(root.clone());
    press_all_four_and_refuse(&mut rig);
    check_out_all_four(&root);

    let mut watch = storage_watch::StorageWatch::default();
    let clock = Arc::new(Mutex::new(Instant::now()));
    let listener = watch.listener_on({
        let clock = Arc::clone(&clock);
        move || *clock.lock().unwrap()
    });
    let start = *clock.lock().unwrap();
    let mut at = start;
    let mut quiet_after = Vec::new();
    for document in [
        persist::KEYBINDINGS_FILE_NAME,
        pins::PINS_FILE_NAME,
        persist::PROFILES_FILE_NAME,
        persist::SETTINGS_FILE_NAME,
    ] {
        for _ in ["removed", "added", "modified"] {
            *clock.lock().unwrap() = at;
            listener(
                &root,
                bt_platform::DirChange::Named(&[OsString::from(document)]),
            );
            at += Duration::from_micros(400);
        }
        quiet_after.push(at - Duration::from_micros(400) + watch_clock::WATCH_QUIET);
    }
    *clock.lock().unwrap() = at;
    listener(
        &root,
        bt_platform::DirChange::Named(&[OsString::from(".git")]),
    );

    // The loop's turns: the watch at each document's own quiet instant, the
    // read answers each on a turn of its own.
    let mut cards_after_each_turn = Vec::new();
    for now in quiet_after {
        let due = watch.advance(now, generations(&rig));
        if due.contains(storage_watch::Document::Settings) {
            rig.settings.ask_reread();
        }
        if due.contains(storage_watch::Document::Keybindings) {
            rig.keybindings.ask_reread();
        }
        if due.contains(storage_watch::Document::Profiles) {
            rig.profiles.ask_reread();
        }
        if due.contains(storage_watch::Document::Pins) {
            rig.pins.ask_reread();
        }
        let mut reads = rig.reads();
        while let Some(read) = reads.pop() {
            StoragePassRig::serve(read);
            rig.turn().expect("a turn of the storage pass");
            cards_after_each_turn.push(rig.cards().len());
        }
    }

    assert_eq!(
        cards_after_each_turn,
        vec![0, 0, 0, 1],
        "four reads on four turns, and one card once none is still reading"
    );
    let merged: Vec<&str> = rig
        .said
        .iter()
        .map(|line| {
            assert!(line.contains("conflicts 1"), "one conflict each: {line}");
            line.trim_start_matches("BT_PERSIST ")
                .split(' ')
                .next()
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(
        merged,
        vec![
            pins::PINS_FILE_NAME,
            persist::PROFILES_FILE_NAME,
            persist::KEYBINDINGS_FILE_NAME,
            persist::SETTINGS_FILE_NAME,
        ],
        "four merges, in the order the answers came"
    );
    let [notice] = rig.raised.as_slice() else {
        panic!("one notice: {:?}", rig.raised);
    };
    let files: Vec<&str> = notice.files.iter().map(|file| file.file).collect();
    assert_eq!(
        files,
        vec![
            persist::SETTINGS_FILE_NAME,
            persist::KEYBINDINGS_FILE_NAME,
            persist::PROFILES_FILE_NAME,
            pins::PINS_FILE_NAME,
        ],
        "the four files, in the pass's order"
    );
    assert_eq!(
        notice.files[0].values,
        vec![settings::SettingsRow::Theme.title().to_owned()]
    );
    assert_eq!(
        notice.files[1].values,
        vec![
            shortcuts::Shortcuts::defaults()
                .row("zoom-pane")
                .expect("a row")
                .title
                .text()
                .to_owned()
        ]
    );
    assert_eq!(notice.files[2].values.len(), 1, "{notice:?}");
    assert_eq!(notice.files[3].values, vec![FOLDER.to_owned()]);
    assert!(
        notice.files.iter().all(|file| file.not_applied.is_empty()),
        "{notice:?}"
    );
    let [card] = rig.cards() else {
        panic!("one card: {:?}", rig.cards());
    };
    assert_eq!(card.body(), i18n::merge_notice(notice));
    let _ = std::fs::remove_dir_all(&root);
}

/// RED (design note §3.6, (b)11; C3, C4) — **the card names each file on a line
/// of its own, at most three names per clause, and carries no choice.**
///
/// The English lines: `settings.json: Theme and Font size came from disk.`,
/// both clauses on one line, five names cut to three and how many more, and a
/// clause of names not applied. The card is an Info card anchored to the
/// window, with no title and no verb; one card holds the four files; an empty
/// notice raises nothing.
///
/// MUTATIONS: drop the bound (`None` for `Some(NAMES_ON_A_CARD)` in
/// `i18n::merge_notice_in`) — the five names are all listed; give the card a
/// verb (an action in `raise_merge_notice_on`) — the card offers something to
/// press.
#[test]
fn the_merge_notice_names_each_file_bounded_with_no_choice() {
    use i18n::Lang;
    let file = |file, values: &[&str], not_applied: &[&str]| ReplacedInFile {
        file,
        values: values.iter().map(|name| (*name).to_owned()).collect(),
        not_applied: not_applied.iter().map(|name| (*name).to_owned()).collect(),
    };
    let cases = [
        (
            file("settings.json", &["Theme", "Font size"], &[]),
            "settings.json: Theme and Font size came from disk.",
        ),
        (
            file("keybindings.json", &["Zoom pane"], &["Split right"]),
            "keybindings.json: Zoom pane came from disk; Split right not applied.",
        ),
        (
            file("profiles.json", &["A", "B", "C", "D", "E"], &[]),
            "profiles.json: A, B, C and 2 more came from disk.",
        ),
        (
            file(
                "keybindings.json",
                &["甲 Zoom", "乙 Quit", "丙 Copy", "丁 Paste"],
                &["戊 Split", "己 Close", "庚 Find", "辛 Next", "壬 Prev"],
            ),
            "keybindings.json: 甲 Zoom, 乙 Quit, 丙 Copy and 1 more came from disk; 戊 \
             Split, 己 Close, 庚 Find and 2 more not applied.",
        ),
    ];
    for (entry, line) in &cases {
        let notice = MergeNotice {
            files: vec![entry.clone()],
        };
        assert_eq!(i18n::merge_notice_in(Lang::English, &notice), *line);
    }

    let notice = MergeNotice {
        files: vec![
            file("settings.json", &["Theme", "Font size"], &[]),
            file("keybindings.json", &["Zoom pane"], &["Split right"]),
            file("profiles.json", &["A", "B", "C", "D", "E"], &[]),
            file("pins.json", &[r"C:\项目 folio"], &[]),
        ],
    };
    assert_eq!(
        i18n::merge_notice_in(Lang::English, &notice),
        "settings.json: Theme and Font size came from disk.\nkeybindings.json: Zoom pane came \
         from disk; Split right not applied.\nprofiles.json: A, B, C and 2 more came from \
         disk.\npins.json: C:\\项目 folio came from disk.",
        "one line per file, in the notice's order"
    );
    let now = Instant::now();
    let mut toasts = toast::ToastHost::default();
    let card = raise_merge_notice_on(&mut toasts, &notice, Motion::Full, now);
    let [raised] = toasts.toasts() else {
        panic!("one card for four files: {:?}", toasts.toasts());
    };
    assert_eq!(Some(raised.id()), card);
    assert_eq!(raised.kind(), toast::ToastKind::Info);
    assert_eq!(raised.anchor(), toast::ToastAnchor::Window);
    assert_eq!(raised.body(), i18n::merge_notice(&notice));
    assert_eq!(raised.action(), None, "nothing to choose: the file has won");

    let mut toasts = toast::ToastHost::default();
    assert_eq!(
        raise_merge_notice_on(&mut toasts, &MergeNotice::default(), Motion::Full, now),
        None
    );
    assert!(toasts.toasts().is_empty(), "an empty notice raises nothing");
}

/// RED (carry-forward 2; ticket 6's rule) — **a failed presentation still
/// raises the notice**, and its error is still answered.
///
/// A press of Folio's (the theme) is owed when a hand edit gives the theme
/// another value; the pass merges it, the presentation after the merges fails,
/// and the card names the theme all the same, while the turn answers the
/// presentation's error.
///
/// MUTATION: answer the presentation before the raise (in
/// `storage_pass::take_merges`, `presented?` first, the order
/// `take_document_answers` had) — no card.
#[test]
fn a_failed_presentation_still_raises_the_notice() {
    let root = StoragePassRig::scratch("failed-presentation");
    let mut rig = StoragePassRig::open(root.clone());
    assert!(rig.settings.store(settings_with(ThemeModeV1::Light)));
    rig.refuse_writes();
    rig.turn().expect("a turn of refusals");
    StoragePassRig::write(
        &root,
        persist::SETTINGS_FILE_NAME,
        &settings_with(ThemeModeV1::Dark),
    );
    rig.settings.ask_reread();
    for read in rig.reads() {
        StoragePassRig::serve(read);
    }
    rig.presentation_fails = true;

    let error = rig
        .turn()
        .expect_err("the presentation's error is answered");
    assert!(error.to_string().contains("呈现失败"), "{error}");
    assert_eq!(rig.presented, 2, "one presentation per turn");
    let [notice] = rig.raised.as_slice() else {
        panic!("one notice: {:?}", rig.raised);
    };
    assert_eq!(
        notice.files,
        vec![ReplacedInFile {
            file: persist::SETTINGS_FILE_NAME,
            values: vec![settings::SettingsRow::Theme.title().to_owned()],
            not_applied: Vec::new(),
        }]
    );
    assert_eq!(rig.cards().len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

/// RED (C2; T-QUIT-DOCUMENTS' hand-off) — **what a cancelled quit merged is
/// named on the next pass's card**, once, and its line was written once.
///
/// Folio set the theme and the terminal's font size, and the write is on its
/// way when a quit begins. The disk answers `Moved`: the file now holds another
/// theme. The documents phase merges it without its doors — the file's theme,
/// Folio's size — says so in one line, sends the merged document again, and,
/// with no answer by its deadline, is cancelled. The next storage pass puts the
/// merge in force and its card names the theme; its own line counts only what
/// it merged itself — nothing.
///
/// MUTATIONS: drop `merged_at_quit`'s conflicts from the card (in
/// `SettingsPass::replaced`, name only the pass's own) — no card; write the
/// at-quit merge again in the next pass's line (in `SettingsStore::begin_merge`,
/// fold the quit's account into the pass's) — the pass's line counts the
/// conflict a second time.
#[test]
fn what_a_cancelled_quit_merged_is_named_on_the_next_passs_card() {
    let root = StoragePassRig::scratch("cancelled-quit");
    let mut rig = StoragePassRig::open(root.clone());
    assert!(rig.settings.store(SettingsV1 {
        theme_mode: ThemeModeV1::Light,
        terminal_font_size: 19,
        ..SettingsV1::default()
    }));

    let start = Instant::now();
    let mut quit = quit::Quit::begin(Vec::new());
    let mut lines = Vec::new();
    let mut turn = |rig: &mut StoragePassRig, quit: &mut quit::Quit, now| {
        let step = quit.documents_turn(
            &mut [
                &mut rig.settings,
                &mut rig.keybindings,
                &mut rig.profiles,
                &mut rig.pins,
            ],
            now,
        );
        lines.extend(quit.take_merged_lines());
        step
    };
    assert_eq!(
        turn(&mut rig, &mut quit, start),
        quit::QuitStep::Documents,
        "the write is on the worker"
    );
    let theirs = settings_with(ThemeModeV1::Dark);
    let Some(persist::ScriptedJob::Document(write)) = rig.jobs.next() else {
        panic!("the press's write is queued");
    };
    let _ = write.answer.send(document_request::DocumentAnswer {
        generation: write.generation,
        version: write.version,
        asker: write.asker,
        outcome: document_request::Outcome::Moved {
            bytes: Some(Arc::from(
                document_request::StoredDocument::to_bytes(&theirs).expect("serializes"),
            )),
        },
        not_durable: None,
        retired: Vec::new(),
        left: None,
    });
    assert_eq!(
        turn(&mut rig, &mut quit, start),
        quit::QuitStep::Documents,
        "merged, and sent again"
    );
    assert_ne!(
        turn(&mut rig, &mut quit, start + quit::QUIT_DOCUMENTS_BY),
        quit::QuitStep::Documents,
        "no answer by the deadline"
    );
    assert!(quit.take_cancelled().is_some(), "the quit is cancelled");
    assert_eq!(
        lines,
        vec![
            "BT_PERSIST settings.json merged at quit: took 0 from the file, kept 1 from Folio, \
             conflicts 1"
                .to_owned()
        ],
        "the quit's merge, said once"
    );

    rig.turn().expect("the next storage pass");
    assert_eq!(
        rig.said,
        vec![
            "BT_PERSIST settings.json merged: took 0 from the file, kept 0 from Folio, \
             conflicts 0, doors refused: 0"
                .to_owned()
        ],
        "the pass counts only what it merged"
    );
    assert_eq!(
        rig.raised,
        vec![MergeNotice {
            files: vec![ReplacedInFile {
                file: persist::SETTINGS_FILE_NAME,
                values: vec![settings::SettingsRow::Theme.title().to_owned()],
                not_applied: Vec::new(),
            }]
        }],
        "the card names what the quit's merge took from the file"
    );
    assert_eq!(rig.settings.loaded().theme_mode, ThemeModeV1::Dark);
    assert_eq!(rig.settings.loaded().terminal_font_size, 19);
    let _ = std::fs::remove_dir_all(&root);
}
