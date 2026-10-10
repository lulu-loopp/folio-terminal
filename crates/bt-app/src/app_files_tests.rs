//! **The crate root: files column.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{
    A, TAB_ONE, buffer_read_from, calls_of, cross_seats, cross_solve, dir_entry, disk_scratch,
    files_column, found, host_path, host_spelling, in_product, leaf_saying, listed, method_body,
    move_the_disk_forward, pane_rects_of, saved_files_and_terminal, saved_row_of_two, seat_of,
    tab_with_a_files_column, tab_with_a_preview,
};
use bt_source::{Pattern, View, needle};
use std::time::Duration;

// ── `.files-tree:focus-visible` (user ruling 2026-08-12) ────────────────

/// PIN — **a press takes the keyboard without lighting the ring; the first
/// key the column answers lights it; the next press puts it out again.**
///
/// The mock-up rings the selection under `.files-tree:focus-visible .frow.sel`
/// (line 789), and `:focus-visible` is not `:focus`. A browser draws no ring
/// when focus arrives by pointer — the hand that put it there already knows
/// where it went — and draws one as soon as the element is being driven by
/// the keyboard, because that is the moment "which of these two lists moves
/// when I press ↓" becomes a real question.
///
/// Red gate, and it is the whole of the user's report: this was
/// `content.focused = (the column holds the keyboard)`, which is `:focus`. So
/// clicking a folder lit the accent ring, while the mock-up under the same
/// click shows only the `--active` pill. The three assertions are the three
/// gestures in the order the user made them — click, ↓, click.
///
/// The mutation that must re-redden it: make `visible` unconditionally true
/// in `FilesKeyboardFocus::arrive`, which is exactly the old behaviour.
#[test]
fn a_pointer_takes_the_keyboard_without_a_ring_and_the_first_key_lights_it() {
    let column = LeafId {
        tab: A,
        seat: SeatId(3),
    };
    let mut focus = FilesKeyboardFocus::default();

    // ① The click that selects a folder. The column has the keyboard — ↓ will
    //    move *this* list — and it does not say so, because you just pointed
    //    at it.
    assert!(focus.arrive(Some(column), FilesFocusArrival::Pointer));
    assert_eq!(focus.owner, Some(column), "the keyboard did move");
    assert!(
        !focus.visible,
        "and the ring stayed out: `:focus`, not `:focus-visible`"
    );

    // ② The first arrow key. Nothing about *who* owns the keyboard changed,
    //    so only a rule about being driven by one can light this.
    assert!(focus.navigated(), "the first answered key owes a frame");
    assert!(focus.visible, "and lights the ring");
    assert_eq!(focus.owner, Some(column), "without moving the keyboard");
    assert!(
        !focus.navigated(),
        "the second key owes nothing — the ring is already lit"
    );

    // ③ A click back onto the same column. The owner does not move and the
    //    ring must still go out, which is why the change is reported off the
    //    pair rather than off the owner.
    assert!(
        focus.arrive(Some(column), FilesFocusArrival::Pointer),
        "the ring going out is a change worth a frame"
    );
    assert!(!focus.visible, "a press puts the ring out again");
    assert_eq!(focus.owner, Some(column), "and keeps the keyboard");
}

/// PIN — the ring cannot outlive the column it belongs to, and a keyboard
/// route in arrives already lit.
///
/// Rules ③ and ④ of the same ruling. There is no keyboard route into a column
/// in this build — `toggle_files_pane` deliberately does not take the
/// keyboard — so ③ is pinned here rather than in a gesture, which is what
/// makes it true the day such a route is added instead of a thing that has to
/// be remembered.
#[test]
fn the_ring_never_outlives_its_column_and_a_key_that_arrives_brings_one() {
    let column = LeafId {
        tab: A,
        seat: SeatId(3),
    };
    let mut focus = FilesKeyboardFocus::default();

    // ③ Focus carried in *by* the keyboard is visible from the first frame:
    //    nothing else would tell you where the next ↓ is going.
    assert!(focus.arrive(Some(column), FilesFocusArrival::Keyboard));
    assert!(focus.visible, "a keyboard arrival rings at once");

    // ④ Escape, or a click in a shell. The bit is meaningless with no column
    //    to be about, and is never left set behind the keyboard's back.
    assert!(focus.arrive(None, FilesFocusArrival::Keyboard));
    assert_eq!(focus.owner, None);
    assert!(
        !focus.visible,
        "a ring belonging to no column is a ring on nothing"
    );
    assert!(
        !focus.navigated(),
        "and a key with no column to answer for lights nothing"
    );

    // A column that never had the keyboard cannot be lit into having it.
    let mut cold = FilesKeyboardFocus::default();
    assert!(!cold.navigated());
    assert_eq!(cold, FilesKeyboardFocus::default());
}

/// PIN (user ruling 2026-08-19) — **the files column's key for a directory
/// is `full_path`'s own inverse.**
///
/// A rename tells the column that holds the file's folder to read it again,
/// and it can only do that if it can turn a path back into the id the tree
/// walks by. A column rooted somewhere else answers `None` and is not asked.
///
/// Red gate: join the segments with the platform separator and the key stops
/// matching the ones `child_key` mints.
#[test]
fn a_directory_under_a_column_resolves_to_the_key_the_tree_walks_by() {
    let root = &host_spelling(r"C:\work");
    assert_eq!(
        files_key_under(root, host_path(r"C:\work").as_path()),
        Some(String::new()),
        "the root itself is the empty key"
    );
    assert_eq!(
        files_key_under(root, host_path(r"C:\work\src\ui").as_path()),
        Some("/src/ui".to_owned())
    );
    assert_eq!(
        files_key_under(root, host_path(r"C:\elsewhere\src").as_path()),
        None,
        "a column rooted somewhere else is not showing this folder"
    );
    // And the key it mints resolves back to the path it came from, which is
    // the round trip the refresh actually needs.
    let key = files_key_under(root, host_path(r"C:\work\src\ui").as_path())
        .expect("the folder is under the root");
    assert_eq!(files::full_path(root, &key), host_path(r"C:\work\src\ui"));
}

/// PIN (D4 of the 2026-09-11 adversarial review) — **a press that ends a
/// name editor lands on the row it was pressed on, not on the row that moved
/// up into its place.**
///
/// Closing the box removes the pending row from the very list the press was
/// measured against, so every row under it moves up one row height. The
/// handler re-asked the chrome at the **unchanged** pointer coordinates,
/// which named whichever row had moved into that space — and
/// `press_files_row` then selected or unfolded it. Re-deriving an *index* is
/// necessary; re-deriving the reader's *choice* from stale pixels is not.
///
/// RED GATE: dispatch on the index the press carried, or re-resolve the
/// target from the position after the rebuild, and the press names `/z.txt`
/// — the row the reader did not press.
#[test]
fn a_press_that_ends_the_editor_dispatches_to_the_row_it_pressed() {
    let (mut tab, seat) = files_column(r"D:\work");
    tab.files
        .get_mut(&seat)
        .expect("the column has state")
        .open
        .insert("/src".to_owned());
    {
        let cache = tab.file_trees.entry(seat).or_default();
        cache.accept(
            "",
            listed(vec![
                dir_entry("src", true),
                dir_entry("y.txt", false),
                dir_entry("z.txt", false),
            ]),
        );
        cache.accept("/src", listed(vec![dir_entry("x.txt", false)]));
    }
    let one = |tab: &TabState, place| -> BTreeMap<SeatId, seats::FilesTreeContent> {
        tab.files_tree_walk(place)
            .into_iter()
            .map(|(seat, (content, _))| (seat, content))
            .collect()
    };

    let measured = one(
        &tab,
        Some(FilesEditPlace {
            seat,
            at: FilesEditRow::New {
                parent: "/src",
                folder: false,
            },
        }),
    );
    let keys: Vec<&str> = measured[&seat]
        .rows
        .iter()
        .map(|row| row.key.as_str())
        .collect();
    assert_eq!(
        keys,
        vec!["/src", "/src/x.txt", "/src/", "/y.txt", "/z.txt"]
    );

    // The reader presses `/y.txt`, which the box has pushed down to row 3.
    let pressed = pressed_row_identity(
        Some(seats::ChromeTarget::FilesRow { seat, index: 3 }),
        &measured,
    );
    assert_eq!(pressed, Some((seat, "/y.txt".to_owned())));

    let rebuilt = one(&tab, None);
    assert_eq!(
        rebuilt[&seat].rows[3].key, "/z.txt",
        "row 3 is a different row now — this is what the stale pixels named"
    );
    assert_eq!(
        press_after_blur(pressed, &rebuilt),
        PressAfterBlur::Row(seat, 2),
        "and the press goes to the row it was pressed on, at its new index"
    );

    // A press that never named a row has nothing that can have moved.
    assert_eq!(pressed_row_identity(None, &measured), None);
    assert_eq!(
        press_after_blur(None, &rebuilt),
        PressAfterBlur::AskAgain,
        "so the chrome is re-asked at the pointer, exactly as it always was"
    );
}

/// PIN (0.3, the files column's second batch) — **the menu's two new verbs
/// go through the doors this window already has, and `Delete` goes through
/// the Recycle Bin and nothing else.**
///
/// A `Runtime` is a surface, a compositor and a filesystem, so "the row was
/// recycled and the tree re-read" is not a sentence this process can say
/// without a screen. What can be said without one is **which function calls
/// which**, and this module's standing rule is that those are read off the
/// source — the same shape `the_crumb_and_the_tree_row_open_the_head_s_own_
/// rename` already takes about the row above these.
///
/// Four claims:
///
/// ① the bin, and **only** the bin: no `remove_file`, no `remove_dir_all`,
/// no `remove_dir` anywhere in the verb — the row goes somewhere it can be
/// fetched back from, and that is the whole reason it is offered with no
/// question in front of it;
/// ② **the folder goes whole**, in one call naming the folder, so there is
/// no walk of its children to be found here;
/// ③ the file is made with **`create_new`** and not `create`, because
/// `File::create` truncates and a `New file…` that emptied somebody's notes
/// is not a refusal but a loss;
/// ④ the new row is **selected and not opened** — `New file…` says it makes
/// a file, and a menu row that did more than its own name says is a row
/// nobody can learn.
///
/// RED GATES: swap `bt_platform::recycle` for `std::fs::remove_file` and ①
/// names the call; use `File::create` and ③ goes red; call `open_preview`
/// after the create and ④ finds the door this row never promised.
#[test]
fn the_files_menus_second_batch_makes_a_row_and_recycles_one() {
    let body = |name: &str| method_body("Runtime", name);

    let delete = body("delete_files_row");
    assert!(
        delete.contains("bt_platform::recycle(&path)"),
        "non-Linux platforms keep their native recycle call"
    );
    assert!(
        delete.contains("self.app.submit_trash(path, target)"),
        "Linux admits the captured row path to the asynchronous trash lane"
    );
    for permanent in [
        "std::fs::remove_file",
        "std::fs::remove_dir_all",
        "std::fs::remove_dir",
    ] {
        assert!(
            !delete.contains(permanent),
            "and never through {permanent}, which cannot be undone"
        );
    }
    assert!(
        !delete.contains("read_dir") && !delete.contains("for "),
        "a folder goes whole, in one call naming the folder"
    );
    assert!(
        body("run_file_menu_row").contains("self.delete_files_row("),
        "the menu row presses that verb and no other"
    );
    assert!(
        !body("run_file_menu_row").contains("GateRequest"),
        "and asks nothing first — the bin is the undo"
    );

    let create = body("create_files_row");
    assert!(
        create.contains("std::fs::File::create_new(&path)"),
        "a new file is made with create_new, which refuses an existing one"
    );
    assert!(
        !create.contains("std::fs::File::create(&path)"),
        "and never with create, which truncates one"
    );
    assert!(
        create.contains("std::fs::create_dir(&path)"),
        "a new folder is one directory and not a path of them"
    );
    assert!(
        create.contains("self.refresh_files_dirs_at(&directory)")
            && create.contains("self.open_files_path_to(leaf.seat, &files::child_key("),
        "the folder is re-read and the row that appeared is selected"
    );
    assert!(
        !create.contains("open_preview"),
        "and not opened: that is the reader's next click"
    );
    assert!(
        body("run_file_menu_row").contains("self.open_files_row_new("),
        "the two New rows open the field rather than asking a dialog"
    );
    assert!(
        body("finish_rename").contains("RenameSubject::FilesNew"),
        "and the field they open leaves by the one exit every draft leaves by"
    );
}

/// PIN (0.3) — **the row a new entry is being named in is in the list the
/// hit test measures clicks against.**
///
/// This walk's own promise is that "the rows a click is measured against are
/// the rows that were drawn", and a row inserted for the open box shifts
/// every row under it. A box placed only at paint time would leave every
/// click below it landing one row off — which is the class of bug the walk
/// was made shared to prevent.
///
/// The rename case is the same claim with nothing inserted: the box is on a
/// row that is already there, so the list is exactly as long as it was.
///
/// RED GATE: insert the pending row in the dressing pass instead of the walk
/// and the first two assertions go red — the list the hit test sees is one
/// row shorter than the list on the glass.
#[test]
fn a_new_entrys_field_stands_in_the_list_the_hit_test_measures_against() {
    let (mut tab, seat) = files_column("D:\\work");
    tab.file_trees.entry(seat).or_default().accept(
        "",
        listed(vec![dir_entry("src", true), dir_entry("a.txt", false)]),
    );
    let plain = tab.files_tree_walk(None)[&seat].0.clone();
    assert_eq!(plain.rows.len(), 2);
    assert_eq!(plain.edit, None, "no box, no row for one");

    let making = tab.files_tree_walk(Some(FilesEditPlace {
        seat,
        at: FilesEditRow::New {
            parent: "",
            folder: false,
        },
    }))[&seat]
        .0
        .clone();
    let edit = making.edit.expect("the box is on a row of this column");
    assert_eq!(making.rows.len(), 3, "the box brought a row with it");
    assert_eq!(edit.at, 2, "at the end of the root's own run");
    assert_eq!(making.rows[edit.at].name, "", "with no name in it yet");
    assert_eq!(
        making.rows[edit.at].kind,
        files::RowKind::File,
        "wearing the kind it is going to be"
    );

    let renaming = tab.files_tree_walk(Some(FilesEditPlace {
        seat,
        at: FilesEditRow::Existing("/a.txt"),
    }))[&seat]
        .0
        .clone();
    assert_eq!(
        renaming.rows.len(),
        2,
        "a rename's box is on a row that is already there"
    );
    assert_eq!(
        renaming.edit.map(|edit| edit.at),
        Some(1),
        "and it is that row"
    );

    // A column that is not the one holding the box is untouched, which is
    // what keying the place by seat is for.
    let elsewhere = tab.files_tree_walk(Some(FilesEditPlace {
        seat: SeatId(seat.0 + 1),
        at: FilesEditRow::New {
            parent: "",
            folder: true,
        },
    }))[&seat]
        .0
        .clone();
    assert_eq!(elsewhere.rows.len(), 2);
    assert_eq!(elsewhere.edit, None);
}

/// **M170 — a saved files root survives a trip through this build.**
///
/// The bug this closes was silent and destructive in the worst combination:
/// `to_persisted` wrote `root: String::new(), open: vec![], sel: None`
/// unconditionally, and `from_persisted` read those three fields off disk and
/// dropped them, because `Seat` has nowhere to put them (red line L1). So a
/// `session.json` that named a folder was **flattened to the empty string by
/// the first save after it was read** — no error, no warning, and the loss
/// visible only as a files pane that had forgotten where it was.
///
/// Nothing on disk had to change to fix it: `FilesLeafV1` has carried all
/// four fields since v1 and the round-trip test in `bt-persist` has been
/// green the whole time. The schema was never the problem — there was no
/// runtime home for the value to be read *into*, which is what A3 is.
///
/// Written as read-then-write over the *same* bytes rather than as four
/// field assertions, because that is the property that actually matters and
/// it cannot be satisfied by accident: every field has to survive, in order,
/// with the width the tree carries separately put back beside the three the
/// side table holds.
///
/// Red gate: put `String::new()` back in `to_persisted`'s Files arm, or drop
/// the `files` map from `revive_plan`, and the comparison fails on the root.
#[test]
fn a_saved_files_root_comes_back_and_goes_out_again_unchanged() {
    let saved = bt_persist::FilesLeafV1 {
        view: bt_persist::FilesViewV1::Files,
        root: r"C:\Users\dev\project".to_owned(),
        // Deliberately not sorted the way a `BTreeSet` would emit them, so a
        // reader that merely echoed the input would pass while one that
        // genuinely round-trips a *set* has to canonicalise. The set has no
        // order of its own; the order it is written in is a rule.
        open: vec!["node-12".to_owned(), "node-45".to_owned()],
        sel: Some("node-45".to_owned()),
        // Nobody's default: 240 is what an undragged column writes, so a
        // width that survived by being re-derived rather than carried would
        // be indistinguishable from one that survived properly.
        width: 197,
        remotes_open: false,
    };
    let (seats, _, _, files, _preview) = revive_plan(&saved_files_and_terminal(saved.clone()));

    // ── the read half: the three facts landed on the seat that owns them ──
    let [column] = seats.files()[..] else {
        panic!("the saved tree holds exactly one files leaf");
    };
    let state = files
        .get(&column)
        .expect("the column came back with a root");
    assert_eq!(
        state.root, saved.root,
        "the root crossed the disk, which is the whole of M170"
    );
    assert_eq!(
        state.open.iter().cloned().collect::<Vec<_>>(),
        saved.open,
        "and so did the expanded set"
    );
    assert_eq!(state.sel, saved.sel, "and the selection");
    // The width is *not* in the side table: it rode back on the seat, where
    // a width lives. Asserted here so the split stays deliberate.
    assert_eq!(
        seats.fixed_extent_of(column),
        Some(bt_layout::LogicalPx::px(i64::from(saved.width))),
        "the width came back on the seat, not in the state beside it"
    );

    // ── the write half: the same bytes go back out ──
    let written = seats.to_persisted(
        &|_| TermLeafV1 {
            profile_id: "pwsh".to_owned(),
            cwd: String::new(),
            manual_name: None,
            card_skip: 0,
            last_command: String::new(),
        },
        &|seat| files.get(&seat).cloned().unwrap_or_default(),
    );
    let [out] = persisted_files_leaves(&written)[..] else {
        panic!("one files leaf in, one files leaf out");
    };
    assert_eq!(
        *out, saved,
        "read then written must be the same files leaf, field for field"
    );
}

/// **A files column that has never been rooted still writes a legal leaf.**
///
/// The honest empty case, and it is worth pinning beside the one above so
/// that "the root survives" is not accidentally satisfied by "the root is
/// always whatever was on disk". A column opened and never given a folder
/// writes the empty string — which is what the old code wrote for *every*
/// column, and the difference is that now it is a fact rather than a default.
#[test]
fn a_files_column_with_no_root_writes_an_honest_empty_leaf() {
    let (seats, _, _, files, _preview) =
        revive_plan(&saved_files_and_terminal(bt_persist::FilesLeafV1 {
            view: bt_persist::FilesViewV1::Files,
            root: String::new(),
            open: Vec::new(),
            sel: None,
            width: 240,
            remotes_open: false,
        }));
    let [column] = seats.files()[..] else {
        panic!("one files leaf");
    };
    assert_eq!(files[&column], seats::FilesLeafState::default());
    let written = seats.to_persisted(
        &|_| TermLeafV1 {
            profile_id: "pwsh".to_owned(),
            cwd: String::new(),
            manual_name: None,
            card_skip: 0,
            last_command: String::new(),
        },
        &|seat| files.get(&seat).cloned().unwrap_or_default(),
    );
    let [out] = persisted_files_leaves(&written)[..] else {
        panic!("one files leaf");
    };
    assert_eq!(out.root, "");
    assert_eq!(
        out.width, 240,
        "an undragged column keeps its opening width"
    );
}

/// **The pairing rule holds for files leaves too** — two columns, two roots,
/// and never crossed.
///
/// The files half of `each_saved_pane_comes_back_as_the_shell_it_was_saved_as`,
/// and it exists for the same reason: the pairing is a `zip` of two walks,
/// and two walks that agree everywhere except on one shape hand a column
/// somebody else's folder. That failure does not look like a bug — it looks
/// like a working files pane pointed at the wrong place.
///
/// Red gate: reverse either walk, or pair by anything other than tree order,
/// and the two roots swap.
#[test]
fn two_saved_files_columns_come_back_to_their_own_roots() {
    let files = |root: &str, width: u32| {
        Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Files(
            bt_persist::FilesLeafV1 {
                view: bt_persist::FilesViewV1::Files,
                root: root.to_owned(),
                open: Vec::new(),
                sel: None,
                width,
                remotes_open: false,
            },
        )))
    };
    let (seats, _, _, states, _preview) = revive_plan(&TabV1 {
        root: LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
            dir: bt_persist::SplitDirV1::Row,
            ratio: 500_000,
            children: [
                files(r"C:\left", 200),
                Box::new(LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
                    dir: bt_persist::SplitDirV1::Row,
                    ratio: 500_000,
                    children: [
                        Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
                            profile_id: "pwsh".to_owned(),
                            cwd: String::new(),
                            manual_name: None,
                            card_skip: 0,
                            last_command: String::new(),
                        }))),
                        files(r"D:\right", 300),
                    ],
                })),
            ],
        }),
        pinned: false,
        focused_leaf: "leaf-1".to_owned(),
        preview: None,
    });
    let [left, right] = seats.files()[..] else {
        panic!("two files leaves in, two files seats out");
    };
    assert_eq!(states[&left].root, r"C:\left");
    assert_eq!(states[&right].root, r"D:\right");
    assert_eq!(
        seats.fixed_extent_of(left),
        Some(bt_layout::LogicalPx::px(200)),
        "each column keeps its own width as well as its own root"
    );
    assert_eq!(
        seats.fixed_extent_of(right),
        Some(bt_layout::LogicalPx::px(300))
    );
}

/// A shell that never reported a folder starts where a fresh one would, and
/// says so by contributing no entry at all.
///
/// The same filter the single-cwd version had, kept per leaf: an empty `cwd`
/// is "nobody said", not "the root of the drive". Absent rather than
/// present-and-empty, so `create_tab_state` has one shape to read and not two.
/// A folder that has since been deleted is carried as saved: whether it still
/// stands is the pane's birth's question, never the window thread's
/// (G-SWEEP-048, `profiles::BirthPlace`), and the birth answers it as no folder.
#[test]
fn a_saved_pane_that_named_no_folder_contributes_no_entry() {
    let (seats, _, none, _files, _preview) = revive_plan(&saved_row_of_two("", ""));
    assert!(
        none.values().all(|leaf| leaf.cwd.is_none()),
        "two silent shells, two absences"
    );
    assert_eq!(
        none.len(),
        seats.terminals().len(),
        "an absent folder is an absent *folder*, not an absent leaf: the pane \
             still has a profile to come back as"
    );

    let here = std::env::current_dir().expect("a test runs somewhere");
    let (seats, _, one, _files, _preview) = revive_plan(&saved_row_of_two(
        &here.to_string_lossy(),
        "C:\\definitely\\not\\here",
    ));
    let [left, right] = seats.terminals()[..] else {
        panic!("a row of two terminals holds two terminal seats");
    };
    assert_eq!(one[&left].cwd, Some(profiles::SeedPlace::Carried(here)));
    assert_eq!(
        one[&right].cwd,
        Some(profiles::SeedPlace::Carried(PathBuf::from(
            r"C:\definitely\not\here"
        ))),
        "a folder that is no longer a directory is carried unasked, for the birth to ask about"
    );
}

/// PIN — L157/C38. A column asks for its root and for nothing else.
///
/// The whole of laziness is here: an unopened folder is not "not loaded
/// yet", it is a folder nobody asked about, and a walk that put it on the
/// list would read the disk to draw rows that are not on screen.
#[test]
fn a_column_asks_for_its_root_and_then_only_for_what_is_opened() {
    let (mut tab, seat) = files_column("D:\\work");
    let wanted = |tab: &TabState| tab.files_tree_walk(None)[&seat].1.clone();
    assert_eq!(
        wanted(&tab),
        vec![String::new()],
        "an unread column asks for its root"
    );

    let cache = tab.file_trees.entry(seat).or_default();
    cache.mark_pending("");
    assert!(
        wanted(&tab).is_empty(),
        "and does not ask a second time while the first is outstanding"
    );

    tab.file_trees
        .get_mut(&seat)
        .expect("the cache exists")
        .accept(
            "",
            listed(vec![dir_entry("src", true), dir_entry("a.txt", false)]),
        );
    assert!(
        wanted(&tab).is_empty(),
        "a folded folder is not a folder anybody is waiting for"
    );
    assert_eq!(
        tab.files_tree_walk(None)[&seat]
            .0
            .rows
            .iter()
            .map(|row| row.key.as_str())
            .collect::<Vec<_>>(),
        vec!["/src", "/a.txt"]
    );

    tab.files
        .get_mut(&seat)
        .expect("the column has state")
        .open
        .insert("/src".to_owned());
    assert_eq!(
        wanted(&tab),
        vec!["/src".to_owned()],
        "opening it is what makes it a question"
    );
}

/// PIN — K156, and the day the second door closed. **Every file opens its
/// preview.**
///
/// This test used to assert the opposite for everything that was not a
/// picture, and that half of it was written down as an interim from the
/// beginning: `DESIGN.md` §7.1.3 rules that activating any file opens the
/// preview, with "no preview" cards for what cannot be shown. The preview
/// block built the cards, so the interim ended and the assertion inverted.
///
/// The picture half stays, and stays measured against the *terminal's* own
/// list rather than a list written here, because the property it pins is
/// unchanged: a `.webp` clicked on a line of output and a `.webp`
/// double-clicked in the tree must land in the same pane — now on the same
/// seat by the same lane, chosen by [`path_is_previewable_image`].
///
/// Mutation: restore the `DefaultApp` branch for non-pictures in
/// [`files_row_activation`].
#[test]
fn every_file_opens_in_the_preview_and_a_rootless_column_opens_nothing() {
    let root = r"C:\work";
    for name in [
        "/shot.png",
        "/a/b/SHOT.JPEG",
        "/icon.svg",
        "/anim.gif",
        "/notes.md",
        "/Cargo.toml",
        "/README",
        "/image.bmp",
        "/setup.exe",
    ] {
        assert_eq!(
            files_row_activation(root, name),
            RowActivation::Preview(files::full_path(root, name)),
            "{name} opens in the preview"
        );
    }
    for picture in ["/shot.png", "/a/b/SHOT.JPEG", "/icon.svg", "/anim.gif"] {
        assert!(
            path_is_previewable_image(&files::full_path(root, picture)),
            "{picture} takes the decode lane"
        );
    }
    for document in ["/notes.md", "/Cargo.toml", "/README", "/setup.exe"] {
        assert!(
            !path_is_previewable_image(&files::full_path(root, document)),
            "{document} takes the buffer lane"
        );
    }
    assert_eq!(
        files_row_activation("", "/notes.md"),
        RowActivation::Nowhere,
        "a column that was never pointed anywhere has no path to open"
    );
    assert_eq!(
        files_row_activation(root, "/notes.md"),
        RowActivation::Preview(PathBuf::from(root).join("notes.md")),
        "L167: the id's slashes never reach the filesystem"
    );
}

/// PIN (user ruling 2026-08-25) — **"inside this tree" is a question about
/// whole path components, and the root is inside itself.**
///
/// The predicate the whole soft locate turns on: a folder inside the
/// column's tree is opened down to, and only one outside it re-roots. Two
/// answers are load-bearing and neither is obvious. The **root itself**
/// answers `Some("")` — a breadcrumb segment naming the column's own root
/// must scroll to the top of the tree rather than re-root onto the tree it
/// is already showing. And a **sibling whose name starts with the root's**
/// answers `None`: `D:\development` is not inside `D:\dev`, which is exactly
/// the trap a string prefix falls into and the reason this is component-wise.
///
/// RED EVIDENCE (2026-08-25): the function did not exist — every one of
/// these doors re-rooted unconditionally, which is the report
/// (「打开文件不许重根文件树」).
///
/// MUTATIONS: compare with `str::starts_with` and the sibling arm goes red;
/// answer `None` for the root and a press on the first crumb throws the tree
/// away.
#[test]
fn a_folder_is_inside_a_tree_by_whole_components_and_the_root_is_inside_itself() {
    let root = &host_spelling(r"D:\dev");
    assert_eq!(
        files_key_within(root, host_path(r"D:\dev").as_path()),
        Some(String::new())
    );
    assert_eq!(
        files_key_within(root, host_path(r"D:\dev\crates\bt-app").as_path()),
        Some("/crates/bt-app".to_owned()),
        "and the id is the one `files::full_path` would spend to get back"
    );
    assert_eq!(
        files::full_path(root, "/crates/bt-app"),
        host_path(r"D:\dev\crates\bt-app"),
        "which is the round trip that makes the id worth minting"
    );
    assert_eq!(
        files_key_within(root, host_path(r"D:\development\crates").as_path()),
        None,
        "a sibling whose name begins with the root's is not inside it"
    );
    assert_eq!(files_key_within(root, host_path(r"D:\").as_path()), None);
    assert_eq!(
        files_key_within("", host_path(r"D:\dev").as_path()),
        None,
        "a column with no root contains nothing"
    );
}

/// PIN (user ruling 2026-08-25) — **the way down is opened root first, and
/// the folder asked for is opened too.**
///
/// Root first because that is the order the tree reads them in, so a column
/// that has read nothing asks the worker for its directories in the order it
/// will draw them. The target's own id is on the list because "locate this
/// level" means the same thing a press on that row means, and a press on a
/// folder row opens it.
///
/// RED GATE: drop the root's `""`, and a locate into a tree that has read
/// nothing never asks for the top level; drop the last entry and the folder
/// you asked for stays shut.
#[test]
fn the_way_down_to_a_row_is_opened_from_the_root_and_includes_it() {
    assert_eq!(
        files_open_chain("/crates/bt-app/src"),
        vec![
            String::new(),
            "/crates".to_owned(),
            "/crates/bt-app".to_owned(),
            "/crates/bt-app/src".to_owned(),
        ]
    );
    assert_eq!(files_open_chain(""), vec![String::new()]);
}

/// PIN — **which menu each kind of tree row raises** (user ruling
/// 2026-08-25).
///
/// The ruling that ended K143's "目录行不弹", said once and in the one place
/// both doors into the menu ask: the pointer's ([`Runtime::file_row_under`])
/// and the keyboard's ([`Runtime::raise_file_menu_on_row`]).
///
/// RED GATE: answer `None` for `Directory` and the second and third lines
/// fail; answer `Some` for a cycle or a notice and the last two do.
#[test]
fn a_file_row_and_a_folder_row_each_raise_a_menu_and_the_dead_ends_raise_none() {
    assert_eq!(
        files_row_menu_subject(files::RowKind::File),
        Some(profiles::FileMenuSubject::File)
    );
    assert_eq!(
        files_row_menu_subject(files::RowKind::Directory { open: false }),
        Some(profiles::FileMenuSubject::Folder { expanded: false }),
    );
    assert_eq!(
        files_row_menu_subject(files::RowKind::Directory { open: true }),
        Some(profiles::FileMenuSubject::Folder { expanded: true }),
        "and the fold it is standing in travels with it, so the row that \
             turns it can wear the right face"
    );
    assert_eq!(
        files_row_menu_subject(files::RowKind::Cycle),
        None,
        "a folder that resolves to its own ancestor has no fold worth turning"
    );
    assert_eq!(
        files_row_menu_subject(files::RowKind::Notice(files::RowNotice::Empty)),
        None,
        "and a notice names no file at all"
    );
    // **And no row is ever the column's ground** (user ruling 2026-09-10).
    // That face is raised by a press that landed on *no* row, so a row
    // answering it would be the ground's menu — `Rename` and `Delete` taken
    // away, `Fold` taken away — coming up on a file.
    for kind in [
        files::RowKind::File,
        files::RowKind::Directory { open: false },
        files::RowKind::Directory { open: true },
        files::RowKind::Cycle,
        files::RowKind::Notice(files::RowNotice::Empty),
    ] {
        assert_ne!(
            files_row_menu_subject(kind),
            Some(profiles::FileMenuSubject::Root),
            "{kind:?} is a row, and the root is the column"
        );
    }
}

/// PIN (user ruling 2026-09-10) — **a right press on a files column's empty
/// ground raises the root folder's menu, through the one door a row's press
/// goes through.**
///
/// `file_row_under` answered rows and only rows, so a press below the last
/// row — which in a column standing in a folder of files is most of the
/// column — raised nothing at all, and the three verbs about *this folder*
/// had no way in. The fallback is on that function rather than beside it,
/// which is what keeps the ruling one sentence: the right-press opener is
/// unchanged, still asks one question, and still hangs the menu at the
/// pointer.
///
/// The geometry half of this — that the ground is the body below the rows
/// and that a row is never it — is `seats`'s
/// `the_ground_below_the_last_row_is_the_columns_own`. What is left here is
/// the wiring, read as **text** for `both_pointer_doors_tell_the_chevron_
/// clocks_where_the_hand_is`' reason: what it guards against is a second
/// opener, and a second opener that agrees today cannot be driven into
/// disagreeing by any state machine.
///
/// Red gate: return `None` from the row branch's `else` and the first
/// assertion fails by name; give the ground its own opener beside the row's
/// and the last one does.
#[test]
fn a_right_press_on_the_columns_ground_raises_the_roots_menu() {
    assert!(
        method_body("Runtime", "file_row_under")
            .contains("return self.files_ground_under(position);"),
        "a press that is on no row falls through to the column's ground \
             rather than to silence"
    );
    let ground = method_body("Runtime", "files_ground_under");
    assert!(
        ground.contains("seats::files_ground_at("),
        "and the ground is resolved against the geometry the rows were \
             placed by, not against the pane's whole rectangle"
    );
    assert!(
        ground.contains("subject: profiles::FileMenuSubject::Root,"),
        "the face it raises is the root's own"
    );
    assert!(
        ground.contains("key: String::new(),")
            && ground.contains("files_row_activation(&root, \"\")"),
        "addressed by the root's key, which is the empty one — so `New \
             file…` places its box at the top level and the path verbs get the \
             root itself"
    );
    // One opener, and it still hangs the menu where the press landed.
    //
    // The two needles are **assembled** rather than written out, for the
    // reason this whole family of tests has to watch for: a literal spelled
    // in full here is itself a line of `main.rs`, so `SOURCE.contains` would
    // be asking whether this test exists. Split across a `concat`, the
    // phrase appears in the file exactly where the code is.
    assert_eq!(
        in_product(&calls_of("Runtime", "file_row_under")),
        1,
        "the ground and the rows go through one right-press opener"
    );
    let anchored = [
        "self.open_file_menu(target, ",
        "[position.x as f32, position.y as f32])?;",
    ]
    .concat();
    assert!(
        !found(needle!(Pattern::text(anchored.as_str())), View::Raw).is_empty(),
        "hung at the pointer, which is where a menu raised by a press belongs"
    );
}

/// RED (review row D1, the ground half) — **a press on a float's body raises
/// no root menu for the column it is covering.**
///
/// The same hole, one fall-through further down: a point that named no
/// docked row went on to `files_ground_under`, so a right press on the empty
/// space below a floating tree's last row raised the *covered* column's Root
/// face, whose `New file…` opens an inline box in a tree the float is
/// standing on. A float's ground is the window's, which
/// `files_ground_under`'s own doc already said it intended.
///
/// Red gate: move the ground fallback above the float arm, or drop the
/// arm's `return None`, and the ordering assertion fails.
#[test]
fn a_press_on_a_floats_body_over_a_column_raises_no_root_menu() {
    let body = method_body("Runtime", "file_row_under");
    let claim = body
        .find("Some(PointerTarget::Float(")
        .expect("the float's claim is read first");
    let declined = body[claim..]
        .find("=> return None,")
        .map(|at| claim + at)
        .expect("a float part with no row behind it ends the question");
    let ground = body
        .find("return self.files_ground_under(position);")
        .expect("a press that is on no row still falls through to the column's ground");
    assert!(
        declined < ground,
        "the float's claim is answered before the ground is asked, so a \
             point inside a window never reaches the folder behind it"
    );
    let ground_body = method_body("Runtime", "files_ground_under");
    assert!(
        !ground_body.contains("float"),
        "and the ground path itself knows nothing about floats — the rule \
             is stated once, above it"
    );
}

/// RED (verifier note N2, carried by the D1 ticket) — **`Delete` resolves
/// its key against the live tree, the way `Rename` already does.**
///
/// `open_files_row_rename` looks the key up in `files_trees` before it acts,
/// with the argument that the menu can stand open while a directory lands
/// underneath it and a key that no longer names a row renames nothing.
/// `delete_files_row` went straight to `files::full_path(&root, key)`. The
/// asymmetry is what would turn a re-root under an open menu into a delete
/// of the wrong path, and the two verbs that act on a row should be asking
/// the same question.
///
/// Red gate: drop the look-up and the first assertion fails; move it below
/// the `recycle` call and the ordering one does.
#[test]
fn delete_resolves_its_key_against_the_live_tree() {
    let delete = method_body("Runtime", "delete_files_row");
    let looked_up = delete
        .find(".files_trees(")
        .expect("Delete asks the live tree whether this key still names a row");
    assert!(
        delete[looked_up..].contains("row.key == key"),
        "by the row's key, which is what the menu is carrying"
    );
    let recycled = delete
        .find("bt_platform::recycle(")
        .expect("and then, and only then, sends it to the bin");
    assert!(
        looked_up < recycled,
        "the question is asked before the path is built, not after it has \
             been recycled"
    );
    // The same question, asked the same way, by the other verb that acts on
    // one row — so neither can be the one that drifts.
    assert!(
        method_body("Runtime", "open_files_row_rename").contains("row.key == key"),
        "which is Rename's own guard, and the reason this one exists"
    );
}

/// PIN — K155. The second press on one *file* row opens it, and nothing else
/// counts as half of that.
#[test]
fn two_presses_on_one_file_row_are_an_opening_and_a_folder_press_is_never_half_of_one() {
    let now = Instant::now();
    let (a, b) = (RowHost::Column(SeatId(1)), RowHost::Column(SeatId(2)));
    let mut clicks = FilesRowClicks::default();
    assert_eq!(clicks.register(a, "/notes.md", now), TabClick::Single);
    assert_eq!(
        clicks.register(a, "/notes.md", now + Duration::from_millis(90)),
        TabClick::Double
    );
    // A double consumes its history: a third press starts a new pair rather
    // than opening the file a second time.
    assert_eq!(
        clicks.register(a, "/notes.md", now + Duration::from_millis(120)),
        TabClick::Single
    );

    // Two different rows are two first clicks, however close together.
    let mut clicks = FilesRowClicks::default();
    assert_eq!(clicks.register(a, "/one.md", now), TabClick::Single);
    assert_eq!(clicks.register(a, "/two.md", now), TabClick::Single);

    // Two columns are two first clicks, even on rows that read the same.
    let mut clicks = FilesRowClicks::default();
    assert_eq!(clicks.register(a, "/notes.md", now), TabClick::Single);
    assert_eq!(clicks.register(b, "/notes.md", now), TabClick::Single);

    // A floating tree is a host of its own (user report, 2026-08-16): its
    // rows pair with themselves and never with a column's, and two presses
    // on one of its file rows are an opening exactly as a column's are.
    let float = RowHost::Float(7);
    let mut clicks = FilesRowClicks::default();
    assert_eq!(clicks.register(a, "/notes.md", now), TabClick::Single);
    assert_eq!(clicks.register(float, "/notes.md", now), TabClick::Single);
    assert_eq!(
        clicks.register(float, "/notes.md", now + Duration::from_millis(90)),
        TabClick::Double
    );

    // Slower than the system's own interval is two clicks, which is what
    // "double click" means everywhere else in this window.
    let mut clicks = FilesRowClicks::default();
    assert_eq!(clicks.register(a, "/notes.md", now), TabClick::Single);
    assert_eq!(
        clicks.register(
            a,
            "/notes.md",
            now + MULTI_CLICK_INTERVAL + Duration::from_millis(1)
        ),
        TabClick::Single
    );

    // A folder press between the two breaks the chain, which is how folding
    // and unfolding quickly can never come out as an activation.
    let mut clicks = FilesRowClicks::default();
    assert_eq!(clicks.register(a, "/notes.md", now), TabClick::Single);
    clicks.interrupt();
    assert_eq!(
        clicks.register(a, "/notes.md", now + Duration::from_millis(90)),
        TabClick::Single
    );
}

/// PIN — the 2026-08-19 ruling. **The second press on a folder row is the
/// way in**: that folder becomes this column's root, in place, and the file
/// row's own second press is untouched beside it.
#[test]
fn a_second_press_on_a_folder_row_is_the_way_in_and_a_file_row_moves_no_root() {
    let root = &host_spelling(r"D:\work");
    let ui = host_spelling(r"D:\work\src\ui");
    // A folder row names the place it would stand the column at, which is
    // the folder itself and not its parent.
    assert_eq!(
        files_row_entry(root, "/src/ui", files::RowKind::Directory { open: false }).as_deref(),
        Some(ui.as_str())
    );
    // Already unfolded is the same folder: which way its triangle points is
    // not a fact about where it is.
    assert_eq!(
        files_row_entry(root, "/src/ui", files::RowKind::Directory { open: true }).as_deref(),
        Some(ui.as_str())
    );

    // A file row goes nowhere near the root — its second press is K156's
    // opening and stays that.
    assert_eq!(
        files_row_entry(root, "/notes.md", files::RowKind::File),
        None
    );
    assert!(matches!(
        files_row_activation(root, "/notes.md"),
        RowActivation::Preview(_)
    ));

    // A folder that resolves to one of its own ancestors is not a way in:
    // entering it is standing where you already stand.
    assert_eq!(files_row_entry(root, "/link", files::RowKind::Cycle), None);
    assert_eq!(
        files_row_entry(root, "", files::RowKind::Notice(files::RowNotice::Empty)),
        None
    );

    // A column with no root has nowhere to go, exactly as it has nothing to
    // open ([`files_row_activation`]'s own last sentence).
    assert_eq!(
        files_row_entry("   ", "/src", files::RowKind::Directory { open: false }),
        None
    );

    // And the way in lands: the door it is handed to is the root menu's.
    let mut state = seats::FilesLeafState {
        root: root.clone(),
        ..seats::FilesLeafState::default()
    };
    state.open.insert("/src".to_owned());
    let entered = files_row_entry(
        &state.root,
        "/src/ui",
        files::RowKind::Directory { open: true },
    )
    .expect("a folder row is a way in");
    assert!(reroot_files_state(&mut state, &entered));
    assert_eq!(state.root, ui);
}

/// PIN — E56. Re-rooting keeps the width and drops everything that was
/// about the old place.
#[test]
fn a_column_pointed_somewhere_else_forgets_the_old_place_and_keeps_its_width() {
    let (mut tab, seat) = files_column(r"C:\work");
    let width = tab
        .seats
        .fixed_extent_of(seat)
        .expect("a files column is a fixed column");
    {
        let state = tab.files.get_mut(&seat).expect("the column has state");
        state.open.insert("/src".to_owned());
        state.sel = Some("/src/main.rs".to_owned());
        assert!(reroot_files_state(state, r"D:\other"));
        assert_eq!(state.root, r"D:\other");
        assert!(
            state.open.is_empty(),
            "an expansion is about a place, and this is a different place"
        );
        assert_eq!(state.sel, None);
    }
    assert_eq!(
        tab.seats.fixed_extent_of(seat),
        Some(width),
        "how much room you gave the column is not a fact about the folder"
    );

    // Choosing the folder you are already in is not a way to lose your work.
    let state = tab.files.get_mut(&seat).expect("the column has state");
    state.open.insert("/a".to_owned());
    assert!(!reroot_files_state(state, r"D:\other"));
    assert!(state.open.contains("/a"));
}

/// PIN — D47. A keyboard lent to a column comes back by itself.
///
/// Each of these is a way the lending goes stale silently, and each would
/// show as arrow keys moving a tree the user cannot see while the shell in
/// front of them ignored them.
#[test]
fn a_keyboard_lent_to_a_column_returns_when_the_column_stops_being_there() {
    let mut tab = tab_with_a_files_column(1, r"C:\work");
    let [column] = tab.seats.files()[..] else {
        panic!("the tab holds one files column");
    };
    let here = LeafId {
        tab: tab.id,
        seat: column,
    };

    assert_eq!(files_keyboard_seat_of(Some(here), &tab), Some(column));
    assert_eq!(files_keyboard_seat_of(None, &tab), None);

    // The same seat number, one tab over, is a different pane.
    let elsewhere = LeafId {
        tab: TabId(tab.id.0 + 1),
        seat: column,
    };
    assert_eq!(files_keyboard_seat_of(Some(elsewhere), &tab), None);

    // A seat that is a terminal is not a tree, whatever was lent to it.
    let shell = LeafId {
        tab: tab.id,
        seat: tab.focused_leaf,
    };
    assert_eq!(files_keyboard_seat_of(Some(shell), &tab), None);

    // And a closed column takes the keyboard back with it.
    tab.files.remove(&column);
    assert_eq!(files_keyboard_seat_of(Some(here), &tab), None);
}

/// RED ① — **a document in the pool follows the disk even while no pane is
/// on it** (user report on a real machine, 2026-08-29; user ruling the same
/// day).
///
/// The defect, verbatim: a repository's `README.md` open in a preview pane
/// with seven other documents behind it in the pool, rewritten on disk by a
/// `git merge`, still showing the old body — **and still showing it after
/// switching to another document and back**. That last clause is the whole
/// diagnosis. `watched_preview_files` asked the *panes* what they were
/// showing, so a pooled buffer no pane was on was never subscribed to; and a
/// buffer is content that belongs to the tab, so coming back to one does not
/// re-read it — by design, and rightly, because that is what makes an
/// unsaved edit survive a switch. The two rules together meant the stale
/// body was the rest of the session.
///
/// Three blocks, on real files in a real tab:
///
/// **① The gate names the pooled document.** This is the fix and the whole
/// of it; everything below it already worked and had nothing to work on.
///
/// **② The disk moves and the buffer takes it**, through the same door the
/// watcher's news goes through.
///
/// **③ The scroll survives.** A reader half way down a document whose file
/// was rewritten must not be sent back to the top — the ruling's own clause.
/// The pane's scroll is a number the pane holds and the re-read never
/// touches it, and this is where that stops being an accident.
///
/// RED GATE ①: make [`files_a_tab_stands_on`] walk `tab.preview_panes` for
/// its documents instead of `tab.preview_pool` — which is the code as it
/// shipped — and block ① fails on `elsewhere.md`, which on a real machine is
/// the reported defect exactly. RED GATE ②: make
/// [`preview::PreviewBuffer::note_disk_moved`] answer `Nothing` for a clean
/// buffer and block ② fails: the pane is subscribed and told, and does not
/// act.
#[test]
fn a_pooled_document_no_pane_is_on_still_follows_its_file() {
    let dir = disk_scratch("pool");
    let shown = dir.join("shown.md");
    let elsewhere = dir.join("elsewhere.md");
    std::fs::write(&shown, "# shown\n").expect("write");
    std::fs::write(&elsewhere, "# version one\n\nthe first body.\n").expect("write");

    let (mut tab, seat) = tab_with_a_preview(
        1,
        vec![buffer_read_from(&shown), buffer_read_from(&elsewhere)],
    );
    let leaf = seat_of(TabId(1), seat);
    // A reader half way down the page they are *not* about to leave.
    tab.preview_panes.entry(leaf).scroll = [0.0, 420.0];

    // ① The gate. Both documents, and the one no pane is on is the point.
    let watched = files_a_tab_stands_on(&tab);
    assert!(
        watched.contains(&shown),
        "the document on the glass was never in doubt"
    );
    assert!(
        watched.contains(&elsewhere),
        "and the one behind it in the pool is the reported defect: \
             a buffer no pane is on is still a buffer somebody will come back to"
    );

    // ② Something outside this window rewrites it, and the watcher's news
    // reaches the buffer through the one door.
    std::fs::write(&elsewhere, "# version two\n\nthe second body.\n").expect("rewrite");
    move_the_disk_forward(&elsewhere);
    let source = preview::PreviewSource::file(&elsewhere);
    let buffer = tab
        .preview_pool
        .get_mut(&source)
        .expect("the pool is holding it");
    assert_eq!(
        buffer.note_disk_moved(true, preview::file_mtime(&elsewhere)),
        preview::DiskVerdict::ReadAgain,
        "a clean body behind its file is a head read, and nothing is said out loud"
    );
    assert!(
        buffer.is_behind_the_disk(),
        "which is what `request_stale_previews` walks the pool for"
    );
    buffer.accept(preview::read_head(&elsewhere));
    assert!(
        buffer
            .content
            .as_deref()
            .is_some_and(|body| body.contains("version two")),
        "and the body on the glass is the file's"
    );
    assert!(
        !buffer.is_behind_the_disk(),
        "the question is closed by its answer"
    );

    // ③ The scroll.
    assert_eq!(
        tab.preview_panes.entry(leaf).scroll,
        [0.0, 420.0],
        "a file rewritten under a reader does not send them back to the top"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// RED — **the file a preview pane's picture stands on is a file this tab
/// stands on** (user report 2026-08-31, `dist\folio-*.exe`).
///
/// The report: `card-3.png` open in a preview pane, replaced on disk by a
/// `mv` in another shell — 1037×735 out, 1242×1656 in — and the pane went on
/// showing the old picture and the old meta line (`1037 × 735 · PNG ·
/// 202 KB`) for good. No `Reload/Keep` strip either, and there could not
/// have been one: the news never arrived, because **no folder was ever
/// subscribed to on that file's behalf**. A picture is not a pool buffer —
/// `create_tab_state` says so in one line, and it is right, the pixels come
/// down the decode lane — so the pool arm of this set could not answer for
/// it, and nothing else mentioned it.
///
/// The watch is a set of *files*, not a set of documents, and this is where
/// that is said. A picture, a video, a document, a page and a picture inside
/// a page all answer with a path and are all subscribed to on identical
/// terms.
///
/// MUTATIONS: drop the `pane.image` arm from
/// [`files_a_tab_stands_on`] and the two picture lines go red — the report,
/// reproduced, and with it the reason the reader saw no strip. Read the
/// picture off the *pool* instead and the same two go red, because a picture
/// was never in one.
#[test]
fn the_picture_a_pane_is_showing_is_a_file_this_tab_stands_on() {
    let seats = seats::Seats::lone_terminal();
    let identity = seats.identity();
    let (layout, overflow) = cross_solve(&seats);
    let card = PathBuf::from(r"D:\folio-social\cards\card-3.png");
    // The picture lane is not the *image* lane: a video arrives on it on
    // identical terms, and a fix written against `.png` would say so.
    let reel = PathBuf::from(r"D:\folio-social\cards\reel.webm");
    let notes = PathBuf::from(r"D:\folio-social\notes.md");

    let mut panes = PreviewPanes::default();
    panes.entry(seat_of(TAB_ONE, identity)).image = Some(PreviewImageState::new(card.clone()));
    panes.entry(seat_of(TAB_ONE, SeatId(9))).image = Some(PreviewImageState::new(reel.clone()));
    let mut pool = preview::PreviewPool::default();
    pool.insert(preview::PreviewBuffer::new(
        preview::PreviewSource::file(notes.clone()),
        "notes.md".to_owned(),
    ));
    let tab = assemble_tab_state(
        TAB_ONE,
        BTreeMap::from([(identity, leaf_saying("SHELL"))]),
        BTreeMap::new(),
        pool,
        panes,
        BTreeMap::new(),
        identity,
        TabSeed::default(),
        seats,
        layout,
        overflow,
    );

    let watched = files_a_tab_stands_on(&tab);
    assert!(
        watched.contains(&card),
        "the picture on the glass is the file the reader is looking at, and \
             it was the one file this set left out"
    );
    assert!(
        watched.contains(&reel),
        "and the lane is the picture lane, not the `.png` lane"
    );
    assert!(
        watched.contains(&notes),
        "without disturbing the pooled document the ruling of 2026-08-29 added"
    );
    assert_eq!(watched.len(), 3, "and nothing else was invented");
}

/// **A dropped file's path goes into the pane it was let go of over, and
/// nowhere at all otherwise** (GitHub issue #1 ②; review 2026-09-17 P1-b).
///
/// The routing half of the drop, read against a real solved three-pane
/// layout: the same arithmetic a press is answered by, asked of the same
/// rectangles. The rest of the rule is its **refusals** — a point a float or
/// an open rail has claimed, a point in no pane at all, and a drop with
/// neither a pointer of its own nor a cursor the platform would answer with.
///
/// **Those three used to answer with the pane holding the keyboard, and the
/// owner's rule is that the target decides.** "I could not tell where you
/// let go, so I typed it where you were last typing" is the one answer a
/// drop must not give — and since the keyboard and the window's own
/// foreground now follow a path in, it would have moved the reader there
/// too. Every one of them is `None`, which `flush_dropped_files` spends on
/// nobody.
///
/// MUTATION ①: ignore `covered` and a point a floating window has claimed
/// answers with the pane it is standing on top of — the second assertion in
/// the loop goes red for all three. MUTATION ②: bring back the fall-back to
/// the keyboard's pane and the three refusals go red together.
#[test]
fn a_drop_lands_in_the_pane_under_it_and_otherwise_nowhere() {
    let seats = cross_seats(3);
    let (layout, _) = cross_solve(&seats);
    let rects = pane_rects_of(&layout);
    assert_eq!(rects.len(), 3, "a three-pane tab places three rectangles");
    // Which pane holds the keyboard is the caller's to say, and here it is
    // the tree's primary leaf. What the test needs of it is only that it is
    // a real pane of this layout and that two of the three points below are
    // *not* it — without that, a routing that always answered the fall-back
    // would pass.
    let focused = seats.identity();
    let mut elsewhere = 0;
    for (seat, rect) in &rects {
        let middle = PhysicalPosition::new(
            f64::from((rect[0] + rect[2]) / 2.0),
            f64::from((rect[1] + rect[3]) / 2.0),
        );
        assert_eq!(
            dropped_files_seat_at(&layout, Some(middle), false),
            Some(*seat),
            "a drop in the middle of {seat:?} is that pane's"
        );
        assert_eq!(
            dropped_files_seat_at(&layout, Some(middle), true),
            None,
            "a float or an open rail standing over {seat:?} keeps the drop \
                 off the pane it is covering, and gives it to nobody else"
        );
        if *seat != focused {
            elsewhere += 1;
        }
    }
    assert_eq!(
        elsewhere, 2,
        "two of the three panes are not the keyboard's, so the assertions \
             above are about the routing and not about a pane that happens to be \
             the focused one"
    );

    let off_every_pane = PhysicalPosition::new(-1.0, -1.0);
    assert_eq!(
        dropped_files_seat_at(&layout, Some(off_every_pane), false),
        None,
        "a drop on the chrome names no pane, so it is refused rather than \
             typed into the one holding the keyboard"
    );
    assert_eq!(
        dropped_files_seat_at(&layout, None, false),
        None,
        "and so is a drop the platform would give no cursor for: an unknown \
             target is a refusal"
    );

    // **The road a drag from another application really takes** (owner's
    // ruling 2026-09-16; release review 0.4.2 X-10). The point is the
    // cursor read as the drop arrived; the reading itself is native, and
    // what is read here is the plumbing under it — the physical pixels it
    // arrives in, and the pane that arithmetic then names.
    let (elsewhere_seat, elsewhere_rect) = *rects
        .iter()
        .find(|(seat, _)| *seat != focused)
        .expect("a three-pane tab has a pane that is not the keyboard's");
    let cursor = (
        ((elsewhere_rect[0] + elsewhere_rect[2]) / 2.0) as i32,
        ((elsewhere_rect[1] + elsewhere_rect[3]) / 2.0) as i32,
    );
    let point = platform_pointer_of(Some(cursor));
    assert_eq!(
        point,
        Some(PhysicalPosition::new(
            f64::from(cursor.0),
            f64::from(cursor.1)
        )),
        "the platform's answer is already in the window's physical pixels \
             and is not scaled a second time"
    );
    assert_eq!(
        dropped_files_seat_at(&layout, point, false),
        Some(elsewhere_seat),
        "a drop whose point came from the cursor lands in the pane under it, \
             not in the pane holding the keyboard"
    );
    assert_eq!(
        platform_pointer_of(None),
        None,
        "and a platform that will not say is not turned into a point at (0, 0)"
    );
}

/// PIN — **a cancelled chooser asks the window for nothing**, and neither
/// does one that could not be shown, nor one whose asker has gone.
///
/// The dialog is never opened here and must not be: `IFileDialog::Show` runs
/// a nested message loop, so a test that reached it would need a window, a
/// COM apartment and a hand to press Cancel. What is testable — and what the
/// bug would live in — is the decision made about the *answer*, which is why
/// that decision is a free function taking one.
///
/// Red gate: answer a cancel with the last-remembered path and
/// `New terminal in folder…` splits into whatever folder was chosen the time
/// before, which is the worst possible reading of "never mind".
#[test]
fn a_cancelled_folder_chooser_asks_the_window_for_nothing() {
    let asked = Some(FolderPick::SplitInto(SeatId(3)));
    assert_eq!(
        folder_pick_outcome(asked, Ok(Some(PathBuf::from(r"D:\repo")))),
        Some((FolderPick::SplitInto(SeatId(3)), PathBuf::from(r"D:\repo"))),
        "a chosen folder is spent by the verb that asked for it"
    );
    assert_eq!(
        folder_pick_outcome(asked, Ok(None)),
        None,
        "cancel does nothing at all — no split, no toast"
    );
    assert_eq!(
        folder_pick_outcome(asked, Err("no apartment".to_owned())),
        None,
        "and a chooser that could not be shown leaves everything where it was"
    );
    assert_eq!(
        folder_pick_outcome(None, Ok(Some(PathBuf::from(r"D:\repo")))),
        None,
        "an answer nobody asked for is nobody's"
    );
    // The two verbs are told apart by the tag, which is the whole reason it
    // exists: a `PathBuf` says nothing about why it was wanted.
    assert_eq!(
        folder_pick_outcome(
            Some(FolderPick::Reroot(SeatId(7))),
            Ok(Some(PathBuf::from(r"D:\repo")))
        )
        .map(|(asked, _)| asked),
        Some(FolderPick::Reroot(SeatId(7))),
    );
}
