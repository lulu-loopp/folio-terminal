//! **A middle click on a files row, driven** (0.4.9 plan row 45, owner's ruling 2026-10-23) on a
//! headless window: the column's tree beside a shell, its listing in the column's cache, and the
//! middle button pressed and released over a row as winit reports it — through `mouse_input`,
//! the slot and the release door.

use crate::headless::{Headless, HeadlessFolio};
use crate::test_support::{dir_entry, listed, seat_of, tab_with_a_files_column};
use crate::{PreviewSurface, RowHost, TabId, preview, seats};
use bt_layout::SeatId;
use std::path::{Path, PathBuf};
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton};

const NOTES: &str = "notes.md";
const OTHER: &str = "另一篇 other.md";
const FOLDER: &str = "src";

/// **A window of one files column beside a shell**, rooted at a scratch folder holding two
/// Markdown files and one folder, its listing already in the column's cache; the window, the
/// column's seat and the folder.
fn a_column(tag: &str) -> (HeadlessFolio, SeatId, PathBuf) {
    let root = bt_testpath::temp_path(tag).join("根 root");
    std::fs::create_dir_all(root.join(FOLDER)).expect("the scratch folder");
    for name in [NOTES, OTHER] {
        std::fs::write(root.join(name), format!("# {name}\n")).expect("a file in it");
    }
    let tab = tab_with_a_files_column(1, &root.display().to_string());
    let mut folio = HeadlessFolio::with_tabs(tag, vec![tab], 0);
    let mut runtime = folio.runtime();
    let seat = runtime
        .seats
        .files_seat()
        .expect("the tab has a files column");
    runtime.file_trees.entry(seat).or_default().accept(
        "",
        listed(vec![
            dir_entry(FOLDER, true),
            dir_entry(NOTES, false),
            dir_entry(OTHER, false),
        ]),
    );
    (folio, seat, root)
}

/// The middle of the row named `name` in the column's tree, as it is laid out now.
fn row_point(folio: &mut HeadlessFolio, seat: SeatId, name: &str) -> PhysicalPosition<f64> {
    let mut runtime = folio.runtime();
    let host = RowHost::Column(seat);
    let (_, rows) = runtime.host_rows(host).expect("the column has rows");
    let index = rows
        .iter()
        .position(|row| row.name == name)
        .unwrap_or_else(|| panic!("the tree lists {name}"));
    let rect = runtime
        .row_geometry(host)
        .expect("the column is laid out")
        .row_rect(index);
    PhysicalPosition::new(
        f64::from((rect[0] + rect[2]) / 2.0),
        f64::from((rect[1] + rect[3]) / 2.0),
    )
}

/// The middle button down at `at` and up at `up`, with the pointer moving between them.
fn middle_click(folio: &mut HeadlessFolio, at: PhysicalPosition<f64>, up: PhysicalPosition<f64>) {
    folio.move_to(at);
    folio
        .runtime()
        .mouse_input(ElementState::Pressed, MouseButton::Middle)
        .expect("a headless press is answered");
    folio.move_to(up);
    folio
        .runtime()
        .mouse_input(ElementState::Released, MouseButton::Middle)
        .expect("a headless release is answered");
}

/// The file the preview pane `seat` of tab 1 is showing.
fn shown_on(runtime: &crate::Runtime<'_, Headless>, seat: SeatId) -> Option<PathBuf> {
    let surface: PreviewSurface = seat_of(TabId(1), seat);
    match runtime.preview_pane(surface)?.source()? {
        preview::PreviewSource::File(path) => Some(path.clone()),
        _ => None,
    }
}

/// What [`untouched`] reads.
type Untouched = (
    SeatId,
    Option<SeatId>,
    Option<PreviewSurface>,
    String,
    Option<String>,
    std::collections::BTreeSet<String>,
);

/// Everything about the window a middle click must leave as it found it: the layout focus,
/// the keyboard (the column's and the preview's), and the column's root, selection and open
/// folders.
fn untouched(runtime: &crate::Runtime<'_, Headless>, column: SeatId) -> Untouched {
    let state = runtime.files_state(column);
    (
        runtime.seats.focus(),
        runtime.files_keyboard_seat(),
        runtime.preview_keyboard_surface(),
        state.root,
        state.sel,
        state.open,
    )
}

fn file(root: &Path, name: &str) -> PathBuf {
    crate::files::full_path(&root.display().to_string(), &format!("/{name}"))
}

/// RED (row 45) — **a middle click on a file row opens that file in a new preview pane beside
/// the current one**: the pane a double click would replace keeps what it shows, the new pane
/// is cut out of it (the pane menu's split, `Seats::split_preview`), and the focus, the
/// keyboard and the column stay as they were. The next double click still lands on the first
/// pane.
///
/// MUTATIONS (observed red): take the middle press's step out of `mouse_input` (no second
/// pane); open the click through `open_preview` (the landing rule) instead of the split — the
/// first pane is replaced; give the new pane the focus (`focus_seat(arriving)`) at the end of
/// `open_preview_beside`.
#[test]
fn a_middle_click_on_a_file_row_opens_it_in_a_new_pane_beside_the_preview() {
    let (mut folio, column, root) = a_column("files-middle-beside");
    let current = {
        let mut runtime = folio.runtime();
        runtime
            .open_preview(file(&root, NOTES))
            .expect("the first file opens");
        let seats = runtime.seats.preview_seats();
        assert_eq!(
            seats.len(),
            1,
            "the fixture's own precondition: one preview pane"
        );
        seats[0]
    };
    let before = untouched(&folio.runtime(), column);
    let was =
        seats::pane_rect(&folio.runtime().seat_layout, current).expect("the pane is laid out");

    let at = row_point(&mut folio, column, OTHER);
    middle_click(&mut folio, at, at);

    let runtime = folio.runtime();
    let panes = runtime.seats.preview_seats();
    assert_eq!(panes.len(), 2, "a new preview pane opened: {panes:?}");
    let arrived = panes[1];
    assert_eq!(panes[0], current, "the first pane is still first");
    assert_eq!(
        shown_on(&runtime, current),
        Some(file(&root, NOTES)),
        "the current pane keeps what it shows"
    );
    assert_eq!(
        shown_on(&runtime, arrived),
        Some(file(&root, OTHER)),
        "the new pane shows the clicked file"
    );
    let contains = |outer: [f32; 4], inner: [f32; 4]| {
        inner[0] >= outer[0] - 0.5
            && inner[1] >= outer[1] - 0.5
            && inner[2] <= outer[2] + 0.5
            && inner[3] <= outer[3] + 0.5
    };
    let now = seats::pane_rect(&runtime.seat_layout, current).expect("the pane is laid out");
    let new = seats::pane_rect(&runtime.seat_layout, arrived).expect("the new pane is laid out");
    assert!(
        contains(was, now) && contains(was, new) && now != was,
        "the new pane is split out of the current one: was {was:?}, now {now:?} and {new:?}"
    );
    assert_eq!(
        untouched(&runtime, column),
        before,
        "the focus, the keyboard and the column stay"
    );
    assert_eq!(
        runtime.seats.landing_preview(),
        Some(current),
        "a double click still opens into the first pane"
    );
    assert!(
        runtime.own_capture().is_none(),
        "the release ended the press"
    );
}

/// RED (row 45) — **a middle click on a folder row does nothing**: no pane, no new root, no
/// fold, no selection.
///
/// MUTATION (observed red): let `press_files_row_middle` take every node row rather than a file
/// row — the folder opens as a "file" in a new pane.
#[test]
fn a_middle_click_on_a_folder_row_does_nothing() {
    let (mut folio, column, root) = a_column("files-middle-folder");
    folio
        .runtime()
        .open_preview(file(&root, NOTES))
        .expect("the first file opens");
    let before = untouched(&folio.runtime(), column);
    let at = row_point(&mut folio, column, FOLDER);
    middle_click(&mut folio, at, at);

    let runtime = folio.runtime();
    assert_eq!(runtime.seats.preview_seats().len(), 1, "no pane opened");
    assert_eq!(
        untouched(&runtime, column),
        before,
        "and the column is as it was"
    );
}

/// RED (row 45) — **a middle press that travels is no click**: past the row drag's six pixels
/// the release opens nothing, and nothing was carried.
///
/// MUTATION (observed red): open on the release whether or not the latch has begun.
#[test]
fn a_middle_press_that_travels_opens_nothing() {
    let (mut folio, column, root) = a_column("files-middle-travel");
    folio
        .runtime()
        .open_preview(file(&root, NOTES))
        .expect("the first file opens");
    let at = row_point(&mut folio, column, OTHER);
    let away = PhysicalPosition::new(at.x + 2.0, at.y + 12.0);
    middle_click(&mut folio, at, away);

    let runtime = folio.runtime();
    assert_eq!(runtime.seats.preview_seats().len(), 1, "no pane opened");
    assert!(
        runtime.own_capture().is_none(),
        "and nothing is held or carried"
    );

    // A press that stays inside the six pixels is still a click.
    let near = PhysicalPosition::new(at.x + 2.0, at.y + 2.0);
    middle_click(&mut folio, at, near);
    assert_eq!(
        folio.runtime().seats.preview_seats().len(),
        2,
        "a near release is a click"
    );
}

/// RED (row 45) — **a tab with no preview pane opens the first one, as a double click does**:
/// one preview pane, on the clicked file, landed by the same rule (`preview_landing_surface`),
/// and the focus stays.
///
/// MUTATION (observed red): take the middle press's step out of `mouse_input` — no pane.
#[test]
fn a_middle_click_in_a_tab_with_no_preview_opens_the_first_one() {
    let (mut folio, column, root) = a_column("files-middle-first");
    let (mut twin, _, twin_root) = a_column("files-middle-first-twin");
    let before = untouched(&folio.runtime(), column);
    let at = row_point(&mut folio, column, OTHER);
    middle_click(&mut folio, at, at);

    let runtime = folio.runtime();
    let panes = runtime.seats.preview_seats();
    assert_eq!(panes.len(), 1, "the first preview pane opened");
    assert_eq!(shown_on(&runtime, panes[0]), Some(file(&root, OTHER)));
    assert_eq!(
        untouched(&runtime, column),
        before,
        "the focus and the keyboard stay"
    );

    // The pane stands where a double click's would: the landing rule's own place.
    twin.runtime()
        .open_preview(file(&twin_root, OTHER))
        .expect("the twin opens its first file");
    let twin_runtime = twin.runtime();
    let twin_pane = twin_runtime.seats.preview_seats()[0];
    assert_eq!(
        seats::pane_rect(&runtime.seat_layout, panes[0]),
        seats::pane_rect(&twin_runtime.seat_layout, twin_pane),
        "the middle click's pane is the double click's"
    );
}

/// RED (row 45) — **a floating tree's file row answers a middle click as a column's does**: the
/// file opens in a new pane beside the tab's preview, the float stays where it is and the
/// preview it stood beside keeps its file.
///
/// MUTATION (observed red): answer only `ChromeTarget::FilesRow` in `press_files_row_middle` —
/// the float's row opens nothing.
#[test]
fn a_middle_click_on_a_floating_trees_file_row_opens_it_beside_the_preview() {
    let (mut folio, _, root) = a_column("files-middle-float");
    let (current, id) = {
        let mut runtime = folio.runtime();
        runtime
            .open_preview(file(&root, NOTES))
            .expect("the first file opens");
        let mut cache = crate::files::DirCache::default();
        cache.accept(
            "",
            listed(vec![
                dir_entry(FOLDER, true),
                dir_entry(NOTES, false),
                dir_entry(OTHER, false),
            ]),
        );
        runtime
            .place_float(
                crate::float::FloatMode::Pinned,
                None,
                crate::float::FloatFiles {
                    files: seats::FilesLeafState {
                        root: root.display().to_string(),
                        ..seats::FilesLeafState::default()
                    },
                    cache,
                    ..crate::float::FloatFiles::default()
                },
                None,
            )
            .expect("the floating tree opens");
        let id = runtime
            .window
            .float
            .live_windows()
            .map(|win| win.epoch)
            .next()
            .expect("the float is open");
        (runtime.seats.preview_seats()[0], id)
    };
    let at = {
        let mut runtime = folio.runtime();
        let host = RowHost::Float(id);
        let (_, rows) = runtime.host_rows(host).expect("the float has rows");
        let index = rows
            .iter()
            .position(|row| row.name == OTHER)
            .expect("the float lists the file");
        let rect = runtime
            .row_geometry(host)
            .expect("the float is laid out")
            .row_rect(index);
        PhysicalPosition::new(
            f64::from((rect[0] + rect[2]) / 2.0),
            f64::from((rect[1] + rect[3]) / 2.0),
        )
    };
    middle_click(&mut folio, at, at);

    let runtime = folio.runtime();
    let panes = runtime.seats.preview_seats();
    assert_eq!(panes.len(), 2, "a new preview pane opened: {panes:?}");
    assert_eq!(shown_on(&runtime, current), Some(file(&root, NOTES)));
    assert_eq!(shown_on(&runtime, panes[1]), Some(file(&root, OTHER)));
    assert!(runtime.window.float.live(id).is_some(), "the float stays");
}
