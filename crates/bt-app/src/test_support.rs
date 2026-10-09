//! **The four facts a ledger answers, as a test writes them** (audit 3 C-2).
//!
//! The routing table used to take a predicate that asked the disk; it takes a read of the pane's
//! own [`bt_term::PathVerdict`] ledger now, and `None` is a real answer — *nobody has asked yet*,
//! which the table reads as "not a link". These three are the whole vocabulary the tables below
//! need, spelled once so that a test that cares about an arm does not have to spell a struct.

// ── `bt-source`, and the one set of helpers every reader in this module asks
//    it through (`docs/plans/bt-app-split-prep.md` §6.3, tickets P3 and P14)
//
// **This file is one flat `mod tests`, and it holds eighty-two readers that
// used to ask `main.rs` for its text.** Each one is being moved to ask
// `bt-source` about an *item* of this crate instead, so that no fact written
// here is bound to the file the item happens to live in today. The pattern is
// `main.rs`'s `pty_drain_budget_tests`, copied once rather than eighty times:
//
// 1. **One index per process** — [`source`], which is
//    `bt_source::Index::of_package("bt-app")`: this package's own `src/`,
//    reached through its declarations. The package is named there and nowhere
//    else in this module.
// 2. **A body pin names an identity, not a file.** [`method_body`] takes the
//    type that owns the method, because the tuple of §2.4 is what stays the
//    same when the method moves to another file. The owner is an argument and
//    not a guess: what the deleted finders did was take the first
//    `\n    fn name(` in `main.rs`, which is a method of *whatever `impl`
//    happens to come first*.
// 3. **A whole-source count or negative becomes a `Search`**, with its view
//    said out loud and the scope that matches the claim.
// 4. **A refusal is never an answer.** Every helper here panics on a
//    `bt_source::QueryFailure`. An item that is not there, or is not unique,
//    is the failure this preparation exists to make loud.
//
// **`Index::body_of` answers with the braces**, while every finder being
// replaced began after the `{` the signature ends with — so a reader that
// reasons about the first statement of a body strips the brace and says so,
// and the equivalence commit compared the two readings on the body's interior,
// the text they really share.

use super::*;
use bt_source::{Found, Index, ItemQuery, Needle, Pattern, Scope, Search, View, needle};
use std::time::Duration;
use winit::keyboard::Key;

/// **This crate, indexed once per process** — the workspace read, this
/// package's own `src/` declared as the universe and lowered, on the first ask
/// of the process, behind one call.
pub(crate) fn source() -> &'static Index {
    Index::of_package("bt-app")
}

/// The body of one item, braces included — the identity of §2.4 rather than a
/// line of a file.
pub(crate) fn item_body(query: &ItemQuery) -> &'static str {
    source()
        .body_of(query)
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// The body of one inherent method of `owner`.
pub(crate) fn method_body(owner: &str, name: &str) -> &'static str {
    item_body(&ItemQuery::method(owner, name))
}

/// The body of one free function of this crate.
pub(crate) fn free_fn_body(name: &str) -> &'static str {
    item_body(&ItemQuery::function(name))
}

/// One item's text with its comments and its whitespace taken out.
///
/// The reading the deleted `method_text` made, lifted off the file it used to
/// cut so that it can be made of an item's body instead: the claims it serves
/// are about which *statement* is there, and a paragraph explaining why it is
/// there is not a statement.
pub(crate) fn squeezed(text: &str) -> String {
    text.lines()
        .map(|line| match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<String>()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

/// One inherent method's body, squeezed.
pub(crate) fn squeezed_body(owner: &str, name: &str) -> String {
    squeezed(method_body(owner, name))
}

/// The declaration of one item — its attributes, its visibility and its
/// signature, stopping in front of the body.
///
/// The reader that pins the answer `create_files_row` gives back is asking
/// about a signature, and a body reading cannot see one.
pub(crate) fn item_declaration(query: &ItemQuery) -> &'static str {
    source()
        .declaration_of(query)
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// One search over the whole crate, refusing loudly rather than answering a
/// smaller question.
pub(crate) fn found(needle: Needle, view: View) -> Found {
    source()
        .search(&Search::new(needle, view))
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// One search over a named scope — a Rust path, never a file.
pub(crate) fn found_in(needle: Needle, view: View, scope: Scope) -> Found {
    source()
        .search(&Search::new(needle, view).in_scope(scope))
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// One search over a named scope of **another** package of this workspace —
/// the reading that used to be a relative path from this file to that crate's
/// source. The scope is a Rust path in *that* crate, so `crate` is
/// `bt-platform`'s root module however its file is spelled.
pub(crate) fn found_in_package(package: &str, needle: Needle, view: View, scope: Scope) -> Found {
    Index::of_package(package)
        .search(&Search::new(needle, view).in_scope(scope))
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// The body of one item of another package of this workspace.
pub(crate) fn package_item_body(package: &str, query: &ItemQuery) -> &'static str {
    Index::of_package(package)
        .body_of(query)
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// Every call of `owner::name`, with the line that declares it left out
/// (§2.5) — the question "how many places reach this door" as a question
/// about names rather than about a spelling.
pub(crate) fn calls_of(owner: &str, name: &str) -> Found {
    source()
        .search(
            &Search::new(needle!(Pattern::call(name)), View::Identifiers)
                .exempting_declarations_of(ItemQuery::method(owner, name)),
        )
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// Every call of one free function of this crate, with the line that declares
/// it left out (§2.5) — [`calls_of`] for an owner that is no type.
pub(crate) fn free_calls_of(name: &str) -> Found {
    source()
        .search(
            &Search::new(needle!(Pattern::call(name)), View::Identifiers)
                .exempting_declarations_of(ItemQuery::function(name)),
        )
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// **How many of these occurrences a product build contains** —
/// `bt_source::Found::in_the_product`, which owns that rule.
///
/// It is what replaces a count taken over `main.rs`, which stopped being this
/// crate's product text on 2026-09-18. It reads at two grains, §2.3's file and
/// §2.4's item, where this module used to read only the first; both answer the
/// same number here, measured needle by needle before the readings were joined.
pub(crate) fn in_product(found: &Found) -> usize {
    found.in_the_product(source()).len()
}

/// **The names of the items these occurrences stand in** (§4.1), each named
/// once and in one order.
///
/// What it replaces is a backwards search for the nearest `\n    fn `, which
/// answers with the *previous* method for any occurrence that does not stand
/// in a method's own body.
pub(crate) fn reader_names(found: &Found) -> Vec<String> {
    let mut names: Vec<String> = found
        .owners(source())
        .into_keys()
        .map(|identity| identity.name)
        .collect();
    names.sort();
    names.dedup();
    names
}

/// A shell a forwarded press can be handed to.
pub(crate) fn a_shell() -> PasteTarget {
    PasteTarget {
        tab: TabId(1),
        seat: SeatId(1),
        incarnation: 1,
    }
}

/// A file on a volume this machine holds.
pub(crate) fn a_local_file() -> bt_term::PathVerdict {
    bt_term::PathVerdict {
        exists: true,
        bytes: Some(0),
        ..bt_term::PathVerdict::absent()
    }
}

/// A folder on a volume this machine holds.
pub(crate) fn a_local_folder() -> bt_term::PathVerdict {
    bt_term::PathVerdict {
        directory: true,
        ..a_local_file()
    }
}

/// A preview surface on one tab, spelled the way a test means it (§7.12 ⓑ).
///
/// Every one of these used to be `PreviewSurface::Seat(SeatId(n))`, which
/// stopped compiling on the day a seat number stopped being a name. A test
/// that does not care which tab says `TAB_ONE` and means it — and the two
/// that do care say two different tabs, which is the whole point.
pub(crate) fn seat_of(tab: TabId, seat: SeatId) -> PreviewSurface {
    PreviewSurface::Seat(LeafId { tab, seat })
}

/// The tab a test that has only ever had one tab is talking about.
pub(crate) const TAB_ONE: TabId = TabId(1);

// ── T4: the press promise (J105) and the tab-name editor (J99-J104) ──

pub(crate) const A: TabId = TabId(1);

pub(crate) fn at(x: f64, y: f64) -> PhysicalPosition<f64> {
    PhysicalPosition::new(x, y)
}

/// No modifier held, which is most of what this editor is typed at with.
pub(crate) const NO_MODIFIERS: ModifiersState = ModifiersState::empty();

/// One key press, through the door the window uses, with an empty clipboard.
///
/// Every verb of this editor is reached through [`rename_key`] since the 0.3
/// migration — the motions and the edits live in `text_field` now, so a test
/// that called them directly would be testing that module rather than this
/// editor's answer to a key.
pub(crate) fn press(editor: &mut TabRename, key: &Key, modifiers: ModifiersState) -> RenameVerdict {
    rename_key(editor, key, modifiers, &mut RenameClipboard::default())
}

// ── T3: one vault, three doors ──

pub(crate) fn saved_tab(profile_id: &str, cwd: &str, name: Option<&str>, pinned: bool) -> TabV1 {
    TabV1 {
        root: LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
            profile_id: profile_id.to_owned(),
            cwd: cwd.to_owned(),
            manual_name: name.map(str::to_owned),
            card_skip: 0,
            last_command: String::new(),
        })),
        pinned,
        focused_leaf: "leaf-0".to_owned(),
        preview: None,
    }
}

/// What one revived tab is made of: its leaves' kinds in tree order, and the
/// folder each terminal leaf's shell is started in.
pub(crate) type RevivedShape = (Vec<bt_layout::SeatKind>, Vec<Option<PathBuf>>);

/// **What the next launch opens from the session file at `session_path`**: the real reader,
/// `plan_windows`, `plan_launch` and `revive_plan` — the first window's plan and the shape each
/// opened tab is revived as.
pub(crate) fn launch_plan_on_disk(session_path: &Path) -> (LaunchPlan, Vec<RevivedShape>) {
    let (read, _, degradation) = bt_persist::read_session(session_path);
    assert!(degradation.is_clean(), "the document reads back whole");
    let windows = plan_windows(
        &read.windows,
        bt_persist::SettingsV1::default().quake_restore,
    );
    let first = windows.first.expect("a window opens");
    let plan = plan_launch(&first.tabs, first.active_tab as usize, false);
    let shapes = plan
        .open
        .iter()
        .map(|tab| {
            let (seats, _, leaves, _, _) = revive_plan(tab);
            let kinds = seats
                .tree()
                .seats_in_order()
                .iter()
                .map(|seat| seat.kind)
                .collect();
            let folders = seats
                .terminals()
                .iter()
                .map(|seat| {
                    leaves
                        .get(seat)
                        .and_then(|leaf| leaf.cwd.as_ref().map(|place| place.path().to_path_buf()))
                })
                .collect();
            (kinds, folders)
        })
        .collect();
    (plan, shapes)
}

// ── B-ENDSESSION: a shutdown, a restart or a sign-out is a quit ──

/// A pinned tab of `shells` shells side by side, every one standing in `cwd`: two for the tab
/// as the reader left it, one for what is left of it once the system has ended the other shell
/// and the ordinary "this shell has exited" road has closed its pane.
fn shells_tab(cwd: &Path, shells: usize) -> TabV1 {
    let cwd = cwd.to_string_lossy().into_owned();
    let shell = || {
        Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
            profile_id: "pwsh".to_owned(),
            cwd: cwd.clone(),
            manual_name: None,
            card_skip: 0,
            last_command: String::new(),
        })))
    };
    let root = if shells == 2 {
        LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
            dir: bt_persist::SplitDirV1::Row,
            ratio: 500_000,
            children: [shell(), shell()],
        })
    } else {
        *shell()
    };
    TabV1 {
        root,
        pinned: true,
        focused_leaf: "leaf-1".to_owned(),
        preview: None,
    }
}

/// One window of two pinned tabs, each of `shells` shells.
pub(crate) fn shells_document(cwd: &Path, shells: usize) -> bt_persist::SessionV1 {
    bt_persist::SessionV1 {
        windows: vec![bt_persist::SessionWindowV1 {
            tabs: vec![shells_tab(cwd, shells), shells_tab(cwd, shells)],
            ..bt_persist::SessionWindowV1::default()
        }],
        ..bt_persist::SessionV1::default()
    }
}

/// **What `App::record_session` does with a document**, asked the question it asks: nothing
/// while the system's end holds the document, the store's `record` otherwise. That the app asks
/// it — and asks it before the store is handed anything — is held on `App::record_session`'s own
/// text by [`a_shell_the_system_ends_after_the_freeze_keeps_its_pane_in_the_document`].
pub(crate) fn record_as_the_app_does(
    store: &mut persist::SessionStore,
    document: bt_persist::SessionV1,
) {
    if session_end::holds_the_document() {
        return;
    }
    store.record(document, Instant::now());
}

/// The windows `session.json` holds right now, read by the real reader.
pub(crate) fn windows_on_disk(session_path: &Path) -> Vec<bt_persist::SessionWindowV1> {
    bt_persist::read_session(session_path).0.windows
}

/// A scratch home for one of these tests, emptied, and the system's news forgotten on the way in
/// and on the way out — one thread can run every test (`--test-threads=1`), and a hold left
/// standing would stop the next test's recordings.
pub(crate) struct EndSessionHome(pub(crate) PathBuf);

impl EndSessionHome {
    pub(crate) fn new(name: &str) -> Self {
        session_end::forget();
        let home = bt_testpath::temp_path(&format!("bt-endsession-{name}"));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("a scratch home");
        Self(home)
    }

    pub(crate) fn session(&self) -> PathBuf {
        self.0.join("session.json")
    }

    pub(crate) fn sentinel(&self) -> PathBuf {
        self.0.join("session.lock")
    }
}

impl Drop for EndSessionHome {
    fn drop(&mut self) {
        session_end::forget();
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The system asks, through the platform's own answer to `WM_QUERYENDSESSION` and the `hear` the
/// window procedure is given; answers what the window procedure returns.
pub(crate) fn the_system_asks() -> Option<isize> {
    bt_platform::session_end::answer(
        bt_platform::session_end::WM_QUERYENDSESSION,
        0,
        &session_end::hear,
    )
}

/// The strip a launch composes, as the flags F57 is asked about: the revived
/// tabs in `plan.open` order with the command line's tab inserted where
/// [`cli_tab_slot`] puts it — the one line `Runtime::create` does with the
/// roots themselves.
pub(crate) fn strip_with_cli_tab(revived_pinned: &[bool], cli_pinned: bool) -> Vec<bool> {
    let mut strip = revived_pinned.to_vec();
    strip.insert(cli_tab_slot(revived_pinned), cli_pinned);
    strip
}

/// A tab of two terminals side by side, each standing where it was told.
pub(crate) fn saved_row_of_two(left: &str, right: &str) -> TabV1 {
    let term = |cwd: &str| {
        Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
            profile_id: "pwsh".to_owned(),
            cwd: cwd.to_owned(),
            manual_name: None,
            card_skip: 0,
            last_command: String::new(),
        })))
    };
    TabV1 {
        root: LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
            dir: bt_persist::SplitDirV1::Row,
            ratio: 500_000,
            children: [term(left), term(right)],
        }),
        pinned: false,
        focused_leaf: "leaf-0".to_owned(),
        preview: None,
    }
}

/// A tree carrying a files leaf, saved the way the runtime saves one.
///
/// `[files | terminal]`, which is also the shape `add_files_pane` produces:
/// the column takes the whole left edge and the shell keeps the rest.
pub(crate) fn saved_files_and_terminal(files: bt_persist::FilesLeafV1) -> TabV1 {
    TabV1 {
        root: LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
            dir: bt_persist::SplitDirV1::Row,
            ratio: 300_000,
            children: [
                Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Files(files))),
                Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
                    profile_id: "pwsh".to_owned(),
                    cwd: String::new(),
                    manual_name: None,
                    card_skip: 0,
                    last_command: String::new(),
                }))),
            ],
        }),
        pinned: false,
        focused_leaf: "leaf-1".to_owned(),
        preview: None,
    }
}

/// The shape slice 7's restore actually produces on a narrow window: a
/// terminal and two preview panes, which by the metrics table wants
/// 260 + 360 + 360 and cannot have it.
pub(crate) fn restored_two_previews_and_a_terminal() -> (seats::Seats, SeatMetrics, LogicalRect) {
    let dpi_milli = 2_000_u32;
    let metrics = seats::seat_metrics(dpi_milli);
    // 1920x1200 physical at 2x — the very window the real-machine capture
    // was taken in, which is 960x560 of usable logical room.
    let viewport = seats::logical_viewport(
        1920,
        1200,
        seats::scale_ppm(dpi_milli),
        0,
        seats::folio_band_device_px(seats::scale_ppm(dpi_milli)),
    );
    let mut seats = seats::Seats::lone_terminal();
    let pinned = seats
        .add_preview(&metrics)
        .expect("the first preview lands");
    assert!(seats.toggle_preview_lock(pinned));
    let second = seats
        .add_preview(&metrics)
        .expect("a pinned pane is not a reuse target");
    // The focused leaf a restore writes: the pane the user was last on.
    assert!(seats.set_focus(second));
    (seats, metrics, viewport)
}

pub(crate) fn logical_width(layout: &SeatLayout, seat: SeatId) -> i64 {
    layout
        .rects
        .iter()
        .find(|placement| placement.id == seat)
        .and_then(|placement| placement.rect)
        .expect("the seat was placed")
        .extent(bt_layout::Axis::Row)
        .floor_px()
}

pub(crate) fn presentation_of(layout: &SeatLayout, seat: SeatId) -> bt_layout::Presentation {
    layout
        .rects
        .iter()
        .find(|placement| placement.id == seat)
        .expect("the seat was placed")
        .presentation
}

/// The window a restore opens in on the machine the report came off: 960x600
/// logical at 200%, which is the *normal* rectangle `session.json` records
/// beside `maximized: true`, and the tree that window is asked to hold —
/// three terminals side by side, which by the metrics table want 260 apiece
/// and cannot have it once the focus column has taken its 280.
pub(crate) fn restored_three_terminals_before_the_window_is_maximized()
-> (seats::Seats, SeatMetrics, SeatLayout) {
    let dpi_milli = 2_000_u32;
    let metrics = seats::seat_metrics(dpi_milli);
    let scale_ppm = seats::scale_ppm(dpi_milli);
    // Focus mode, which is what the report's `settings.json` says and what
    // costs the stage the card column's own width.
    let rail = seats::RailState {
        focus: true,
        ..seats::RailState::default()
    };
    let viewport = seats::logical_viewport(
        1920,
        1200,
        scale_ppm,
        seats::rail_inset_device_px(rail, scale_ppm),
        seats::folio_band_device_px(scale_ppm),
    );
    let mut seats = seats::Seats::lone_terminal();
    let first = seats.identity();
    let second = seats
        .split_terminal(&metrics, first, bt_layout::Axis::Row, false)
        .expect("a second terminal seats beside the first");
    seats
        .split_terminal(&metrics, second, bt_layout::Axis::Row, false)
        .expect("and a third beside that");
    let layout = seats
        .solve(viewport, &metrics, SizePolicy::Lawful)
        .expect("L3 buys the room rather than refusing");
    (seats, metrics, layout)
}

/// Every combination of the three facts [`wheel_route`] turns on.
pub(crate) fn every_wheel_situation() -> impl Iterator<Item = (bool, bt_term::TerminalModes, bool)>
{
    use bt_term::{MouseTracking, TerminalModes};
    let trackings = [
        MouseTracking::Off,
        MouseTracking::Click,
        MouseTracking::Drag,
        MouseTracking::Motion,
    ];
    [true, false].into_iter().flat_map(move |shift| {
        [true, false].into_iter().flat_map(move |alternate_screen| {
            [true, false].into_iter().flat_map(move |alternate_scroll| {
                trackings.into_iter().flat_map(move |mouse_tracking| {
                    [true, false].into_iter().map(move |scrolled| {
                        (
                            shift,
                            TerminalModes {
                                alternate_screen,
                                alternate_scroll,
                                sgr_mouse: true,
                                mouse_tracking,
                                focus_reporting: false,
                                keyboard: bt_term::KeyboardProtocol::default(),
                            },
                            scrolled,
                        )
                    })
                })
            })
        })
    })
}

// ── T2: the tab strip's state channels ──

/// The facts the strip reads for one shell.
///
/// `output_revision` and `last_seen_revision` are handed over separately
/// because they are separate facts — what the shell has said, and how much of
/// that has been painted — and the status carries neither. See [`quiet`].
pub(crate) fn facts_with(
    status: SessionStatus,
    output_revision: u64,
    last_seen_revision: u64,
    tab_is_active: bool,
) -> SessionFacts {
    SessionFacts {
        status,
        output_revision,
        last_seen_revision,
        tab_is_active,
        // Not in the queue: the fixtures below are about the *output*
        // ledger, and a place in the queue outranks every one of its rules.
        // The tests that are about the queue say so ([`waiting`]).
        awaiting: false,
    }
}

/// The same shell, standing unanswered in the attention queue.
pub(crate) fn waiting(status: SessionStatus, tab_is_active: bool) -> SessionFacts {
    SessionFacts {
        awaiting: true,
        ..facts_with(status, 0, 0, tab_is_active)
    }
}

/// The ordinary case: nothing latched, nothing running, `output` said and
/// `seen` painted.
pub(crate) fn facts(
    output_revision: u64,
    last_seen_revision: u64,
    tab_is_active: bool,
) -> SessionFacts {
    facts_with(quiet(), output_revision, last_seen_revision, tab_is_active)
}

/// A session with nothing latched and nothing running.
///
/// Its `published_revision` is deliberately absurd, and that is the point.
/// bt-term counts *frames put on the glass* there, and frames are published
/// for a blinking cursor, a repainted chrome and the tab switch itself —
/// none of which is a program saying anything. Pinning it to a number no
/// ledger can reach means any code that goes back to measuring unread
/// against it fails every test in this section at once, instead of passing
/// them all because the two numbers happened to be given the same value.
pub(crate) fn quiet() -> SessionStatus {
    SessionStatus {
        progress: None,
        bell: None,
        failure_exit_code: None,
        working: false,
        alternate_screen: false,
        published_revision: u64::MAX,
        attention_request: None,
    }
}

/// A session whose bell has rung and whose last command failed — the two
/// latches [`DualPlaneSession::clear_attention`] retires together.
pub(crate) fn latched() -> SessionStatus {
    SessionStatus {
        bell: Some(bt_term::BellSource::Bel),
        failure_exit_code: Some(1),
        ..quiet()
    }
}

/// Apply the ledger's attention rule the way the runtime's own loop does,
/// and report what the tab ends up claiming.
///
/// The clearing is `clear_attention`'s two field writes, which is all the
/// runtime does with the answer — so this exercises the real decision
/// rather than a second copy of it.
pub(crate) fn claim_after_a_look(
    mut status: SessionStatus,
    tab_is_active: bool,
    window_is_focused: bool,
) -> StatusClaim {
    if attention_is_consumed(tab_is_active, window_is_focused) {
        status.bell = None;
        status.failure_exit_code = None;
    }
    // Behind by a mile, so nothing here is hidden by the ledger being up to
    // date: whatever survives is the attention rule's own doing.
    facts_with(status, 50, 0, tab_is_active).claim()
}

/// The mark state of a tab whose session is running, sampled mid-breath.
pub(crate) fn breathing(motion: Motion) -> seats::TabMarkState {
    let elapsed = Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS).mul_f32(0.5);
    seats::TabMarkState {
        opacity: mark_opacity(true, false, elapsed, motion),
        ..seats::TabMarkState::default()
    }
}

/// The same tab one instant after its command returned.
pub(crate) fn settled(motion: Motion) -> seats::TabMarkState {
    let elapsed = Duration::from_millis(WINDOW_TAB_BREATHE_PERIOD_MS).mul_f32(0.5);
    seats::TabMarkState {
        opacity: mark_opacity(false, false, elapsed, motion),
        ..seats::TabMarkState::default()
    }
}

/// A tab of `panes` shells, with a name that says which pane is which.
pub(crate) fn ringing_tab(id: u64, panes: usize) -> TabState {
    let names: Vec<&str> = ["A", "B", "C", "D"][..panes].to_vec();
    cross_tab(id, &names)
}

/// The `BEL` byte, arriving in one pane's shell.
///
/// It used to be the whole of this queue's signal. Since A2 it is an **announcement** — the end
/// of a turn, which is what `attention` plan §2's four recordings established it is — and the
/// tests below use it to show that it no longer opens the door it used to.
pub(crate) fn ring(tab: &mut TabState, seat: SeatId) {
    tab.sessions
        .get_mut(&seat)
        .expect("the fixture's seat holds a shell")
        .session
        .feed(b"\x07")
        .expect("a bell parses");
    assert!(
        tab.sessions[&seat].session.status().bell_latched(),
        "the fixture's own precondition: the bell really latched"
    );
}

/// `OSC 1337;RequestAttention=<value>` — the weak tier's whole vocabulary, in one pane's shell.
///
/// A real sequence through the real parser, because the thing under test is that a *standing*
/// request behaves differently from a bell, and a hand-set field would prove only that a field
/// can be set.
pub(crate) fn request_attention(tab: &mut TabState, seat: SeatId, value: &str) {
    let bytes = format!("\x1b]1337;RequestAttention={value}\x07");
    tab.sessions
        .get_mut(&seat)
        .expect("the fixture's seat holds a shell")
        .session
        .feed(bytes.as_bytes())
        .expect("the sequence parses");
}

// ---------------------------------------------------------------------------
// The wire itself: bytes a program wrote, and the lines they decided
// ---------------------------------------------------------------------------
//
// **Where `bt-term` meets the ledger**, and why these three live here rather than beside the ledger
// in `bt-workbench`: they feed a real `bt_term::DualPlaneSession`, and the ledger's crate may not
// name `bt-term` (`docs/plans/design/ownership-census-2026-09-25.md` §5.4). This crate is where
// the two halves are joined in the product (`deliver_osc_attention`), so this is where the join
// is pinned.

/// One pane's ledger and the window serial it draws places from, for the wire pins below — the
/// ledger's own grid (`bt_workbench::attention`'s tests) uses the same shape.
pub(crate) struct LedgerPane {
    pub(crate) ledger: attention::AttentionLedger,
    places: attention::Places,
    now: Instant,
}

impl LedgerPane {
    pub(crate) fn new() -> Self {
        Self {
            ledger: attention::AttentionLedger::default(),
            places: attention::Places::default(),
            now: Instant::now(),
        }
    }

    /// One arrival, and the lines it decided.
    pub(crate) fn at(&mut self, event: attention::Event) -> Vec<String> {
        let site = attention::Site {
            tab: 1,
            seat: SeatId(2),
        };
        self.ledger
            .apply(
                site,
                attention::Reach::Flash,
                event,
                &mut self.places,
                self.now,
            )
            .lines
    }

    pub(crate) fn state(&self) -> attention::State {
        self.ledger.state()
    }

    pub(crate) fn away(&mut self) -> Vec<String> {
        self.at(attention::Event::Settle {
            active: false,
            focused: false,
        })
    }
}

/// **This machine's names, installed for a test** — what `main` installs before its first
/// session (`host_answers::install`). Every fixture here that makes a session calls it first: a
/// working-directory report reads the names (`bt_term::local_host_names`, which panics before an
/// installation), and the same names again install as nothing, so every test may call it.
pub(crate) fn install_this_machines_names() {
    bt_term::install_host_names(crate::host_answers::this_machines_names());
}

/// One session fed real bytes, the way a pane's child writes them.
pub(crate) fn wired() -> bt_term::DualPlaneSession {
    install_this_machines_names();
    bt_term::DualPlaneSession::new(
        std::num::NonZeroU32::new(80).expect("a width"),
        std::num::NonZeroU32::new(8).expect("a height"),
    )
}

/// One turn of the event loop's attention pass over one window's tabs.
///
/// Both notification rows on, which is what a fresh install is.
pub(crate) const BOTH_NOTIFICATION_ROWS_ON: attention::NotificationSwitches =
    attention::NotificationSwitches {
        turn_end: true,
        desktop_messages: true,
    };

/// A window that is on a screen and a turn-end lane that is switched on, which is what a fresh
/// install is. The deliveries are collected and dropped: what these tests are about is the
/// ledger's own bookkeeping, and `raise_attention` is the window's.
/// A window on a screen with something on top of it, on a desktop whose taskbar is where
/// Windows puts it — which is what these fixtures are about when they say nothing about any of
/// them.
///
/// **Covered rather than in plain sight**, deliberately: these tests are about the ledger's own
/// bookkeeping and use the reach only as an observable, and the covered row is the one that
/// still tells `Flash` and `Toast` apart. A window the reader can see answers `Marks` for every
/// unfocused position and would flatten the very distinction they read.
pub(crate) fn on_a_screen(focused: bool) -> notify::WindowPlace {
    notify::WindowPlace {
        focused,
        hidden: false,
        exposed: false,
        taskbar_is_auto_hidden: false,
    }
}

pub(crate) fn one_turn(
    tabs: &mut [TabState],
    active: usize,
    focused: bool,
    next: &mut attention::Places,
) {
    settle_attention(
        tabs,
        active,
        on_a_screen(focused),
        BOTH_NOTIFICATION_ROWS_ON,
        next,
        Instant::now(),
        None,
        &mut Vec::new(),
    );
}

/// The out door, over a bare tab — **the runtime's own function**, so a fixture cannot drift
/// away from what the window does. `Runtime::answer_attention` is this call plus the borrow
/// split a window needs and a one-tab fixture does not.
pub(crate) fn answer(
    tabs: &mut [TabState],
    index: usize,
    seat: SeatId,
    by: UserInputKind,
    next: &mut attention::Places,
    trace: Option<&attention_trace::Trace>,
) {
    answer_attention_in(
        &mut tabs[index],
        index,
        seat,
        by,
        attention::Reach::Nothing,
        next,
        Instant::now(),
        trace,
    );
}

/// The same window, in logical pixels, on a 200% display and on a 150% one —
/// which is what a drag across that seam leaves behind (§7.50: the system's
/// suggested rectangle is a similarity of the one the window had).
pub(crate) const CARDS_AT_200: (f32, f32) = (1000.0, 2.0);

pub(crate) const CARDS_AT_150: (f32, f32) = (750.0, 1.5);

/// The card column of a window holding `tabs` tabs, scrolled `scroll`
/// physical pixels, on a display of a stated height and scale.
pub(crate) fn cards_column(
    (height, scale): (f32, f32),
    tabs: usize,
    scroll: f32,
) -> seats::FocusRailGeometry {
    seats::focus_rail_geometry(
        height,
        scale,
        seats::FOLIO_BAR,
        tabs,
        0,
        scroll,
        seats::RailState {
            focus: true,
            ..seats::RailState::default()
        },
    )
    .expect("focus mode puts a column on screen")
}

pub(crate) fn placement(bounds: WindowBoundsV1, maximized: bool) -> RestoredPlacement {
    RestoredPlacement {
        size: LogicalSize::new(f64::from(bounds.width), f64::from(bounds.height)),
        position: Some(LogicalPosition::new(
            f64::from(bounds.x),
            f64::from(bounds.y),
        )),
        maximized,
    }
}

pub(crate) fn hyperlink_hit(uri: &str) -> HyperlinkHit {
    HyperlinkHit {
        id: None,
        uri: uri.to_owned(),
        start: bt_doc::ContentAnchor::Live {
            screen: bt_doc::ScreenId::Primary,
            point: bt_doc::GridPoint { row: 1, column: 2 },
            bias: Bias::Before,
            generation: bt_doc::GridGeneration(1),
        },
        end: bt_doc::ContentAnchor::Live {
            screen: bt_doc::ScreenId::Primary,
            point: bt_doc::GridPoint { row: 1, column: 5 },
            bias: Bias::After,
            generation: bt_doc::GridGeneration(1),
        },
    }
}

/// **A ledger in which every name is an ordinary local file**, for the arms whose subject is not
/// the folder question (audit 3 C-2). It was `fn no_directories(_: &Path) -> bool { false }` until
/// the routing table stopped asking a disk and started reading a pane's ledger; `None` is a real
/// answer now — *nobody has asked yet* — so a test about some other arm has to say which.
pub(crate) fn no_directories(_: &Path) -> Option<bt_term::PathVerdict> {
    Some(a_local_file())
}

pub(crate) struct PtyPresentationHarness {
    pub(crate) session: DualPlaneSession,
    pub(crate) projection: ViewportProjection,
    pub(crate) pending: LatestFrameSlot,
    pub(crate) last_presented: Option<ViewportFrame>,
    pub(crate) publications: usize,
    /// **What the P0 fix is measured in.** Every call this harness makes to
    /// `DualPlaneSession::viewport_frame` — the whole-grid capture, the
    /// projection and the decoration pass that an animation tick used to
    /// pay for sixty times a second.
    pub(crate) viewport_frames: usize,
    /// [`WindowRuntime::terminal_content_revision`], on the same bump: a frame
    /// that actually entered the slot.
    pub(crate) content_revision: u64,
    /// [`WindowRuntime::presented_picture_revision`], on the same bump: a frame
    /// that actually reached the glass.
    pub(crate) presented_revision: u64,
}

impl PtyPresentationHarness {
    pub(crate) fn new(columns: u32, rows: u32) -> Self {
        install_this_machines_names();
        let session = DualPlaneSession::new(
            NonZeroU32::new(columns).unwrap(),
            NonZeroU32::new(rows).unwrap(),
        );
        let projection = session.new_projection(session.layout_key());
        Self {
            session,
            projection,
            pending: LatestFrameSlot::default(),
            last_presented: None,
            publications: 0,
            viewport_frames: 0,
            content_revision: 0,
            presented_revision: 0,
        }
    }

    /// What `Runtime::picture_on_glass` reads, off this harness's mirror of
    /// the same five facts.
    fn picture_on_glass(&self) -> PictureOnGlass {
        PictureOnGlass {
            frame_pending: self.pending.pending_frame().is_some(),
            has_presented_frame: self.last_presented.is_some(),
            presentation_hold: self.projection.presentation_hold(),
            content_revision: self.content_revision,
            presented_revision: self.presented_revision,
        }
    }

    /// One turn of the tab strip's indeterminate ring.
    ///
    /// `reuse` is the policy under test, injected rather than called
    /// directly so a test can run the *mutant* — the unconditional
    /// reprojection this window shipped with — through the identical loop
    /// and show what it costs.
    pub(crate) fn chrome_tick(&mut self, reuse: fn(PictureOnGlass) -> bool) {
        if reuse(self.picture_on_glass()) {
            // `Runtime::present_retained_picture`: the glass is redrawn
            // from the frame it already holds. No session, no projection.
            self.present_pending();
            return;
        }
        self.publish_expose_frame();
        self.present_pending();
    }

    /// `publish_frame_inner` for a source that is not PTY output: composes
    /// unconditionally, with no unchanged-frame gate to fall back on.
    pub(crate) fn publish_expose_frame(&mut self) -> bool {
        self.publish_expose_frame_inner(false)
    }

    fn publish_expose_frame_inner(&mut self, skip_unchanged: bool) -> bool {
        self.session.refresh_projection(&mut self.projection);
        self.viewport_frames += 1;
        let frame = self.session.viewport_frame(&mut self.projection).unwrap();
        if self.projection.presentation_hold() && self.last_presented.is_some() {
            return false;
        }
        if skip_unchanged
            && pty_frame_is_unchanged(
                self.pending.pending_frame(),
                self.last_presented.as_ref(),
                &frame,
            )
        {
            return false;
        }
        self.content_revision += 1;
        self.pending
            .publish(
                frame,
                FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::Expose,
                },
            )
            .unwrap();
        self.publications += 1;
        true
    }

    pub(crate) fn feed_drain(&mut self, bytes: &[u8]) -> bool {
        self.session.feed(bytes).unwrap();
        self.publish_pty_frame()
    }

    pub(crate) fn finish_synchronized_update(&mut self) -> (bool, bool) {
        let finished = self
            .session
            .finish_synchronized_update(Instant::now())
            .unwrap();
        let published = finished && self.publish_pty_frame();
        (finished, published)
    }

    pub(crate) fn publish_pty_frame(&mut self) -> bool {
        self.session.refresh_projection(&mut self.projection);
        self.viewport_frames += 1;
        let frame = self.session.viewport_frame(&mut self.projection).unwrap();
        // Mirror publish_frame_inner's combined review/exact-source presentation hold.
        if self.projection.presentation_hold() && self.last_presented.is_some() {
            return false;
        }
        if pty_frame_is_unchanged(
            self.pending.pending_frame(),
            self.last_presented.as_ref(),
            &frame,
        ) {
            return false;
        }
        self.pending
            .publish(
                frame,
                FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::PtyOutput,
                },
            )
            .unwrap();
        self.content_revision += 1;
        self.publications += 1;
        true
    }

    pub(crate) fn present_pending(&mut self) -> bool {
        let Some((frame, _)) = self.pending.take() else {
            return false;
        };
        self.last_presented = Some(frame);
        self.presented_revision = self.content_revision;
        true
    }
}

pub(crate) fn frame_row_text(frame: &ViewportFrame, row: usize) -> String {
    let columns = frame.columns.get() as usize;
    frame.cells[row * columns..(row + 1) * columns]
        .iter()
        .map(|cell| cell.text.as_str())
        .collect()
}

/// Everything a pane is showing, as one string, so a test can ask whether a
/// word is on the glass without knowing which row it landed on.
fn frame_text(frame: &ViewportFrame) -> String {
    (0..frame.drawable_rows())
        .map(|row| frame_row_text(frame, row))
        .collect::<Vec<_>>()
        .join("\n")
}

/// **The window this bug lives in**: the pane holding the keyboard, and a
/// pane beside it.
///
/// Driven through the same three steps the runtime takes on every turn of
/// its loop, in the same order and with the same dependency between them:
/// `drain_pty` hears every shell, `publish_frame_inner` composes **the
/// focused leaf only** and may drop the frame, and `redraw` — which runs for
/// a frame that reached the slot and for nothing else — is the one and only
/// place the *other* pane is projected and drawn.
///
/// That last sentence is the whole bug: judge the drain by the focused
/// pane's picture and a sibling that has just spoken loses the pass it is
/// painted in.
pub(crate) struct TwoPaneHarness {
    focused: DualPlaneSession,
    focused_projection: ViewportProjection,
    sibling: DualPlaneSession,
    sibling_projection: ViewportProjection,
    pending: LatestFrameSlot,
    /// `WindowRuntime::last_presented_frame` — the focused leaf's copy, and what
    /// the unchanged-frame gate compares against.
    focused_on_glass: Option<ViewportFrame>,
    /// `LeafSession::last_presented_frame` for the pane beside it: what the
    /// user can actually read in that half of the window.
    sibling_on_glass: Option<ViewportFrame>,
    /// [`WindowRuntime::unpainted_pane_output`].
    unpainted_pane_output: bool,
    pub(crate) presents: usize,
    /// Every projection this window paid for, both panes counted — the P0
    /// meter, so that a repair for one pane can be shown not to have bought
    /// back the whole-window recompose P0 removed.
    pub(crate) viewport_frames: usize,
}

impl TwoPaneHarness {
    pub(crate) fn new(columns: u32, rows: u32) -> Self {
        install_this_machines_names();
        let pane = || {
            let session = DualPlaneSession::new(
                NonZeroU32::new(columns).unwrap(),
                NonZeroU32::new(rows).unwrap(),
            );
            let projection = session.new_projection(session.layout_key());
            (session, projection)
        };
        let (focused, focused_projection) = pane();
        let (sibling, sibling_projection) = pane();
        Self {
            focused,
            focused_projection,
            sibling,
            sibling_projection,
            pending: LatestFrameSlot::default(),
            focused_on_glass: None,
            sibling_on_glass: None,
            unpainted_pane_output: false,
            presents: 0,
            viewport_frames: 0,
        }
    }

    /// One turn of `about_to_wait`: drain both shells, publish if anything
    /// arrived, then the redraw that publish asked for.
    pub(crate) fn turn(
        &mut self,
        focused_bytes: &[u8],
        sibling_bytes: &[u8],
        says_nothing_new: fn(bool, bool) -> bool,
    ) {
        let mut arrived = false;
        if !focused_bytes.is_empty() {
            self.focused.feed(focused_bytes).unwrap();
            arrived = true;
        }
        if !sibling_bytes.is_empty() {
            self.sibling.feed(sibling_bytes).unwrap();
            arrived = true;
            // `drain_tab_pty`: a leaf that is not this tab's focused leaf
            // has spoken, which is what `Runtime::drain_pty` raises the bit
            // for.
            self.unpainted_pane_output = true;
        }
        if arrived {
            self.publish_pty_frame(says_nothing_new);
        }
        self.redraw();
    }

    /// `publish_frame_inner` for a PTY drain: compose the focused leaf, then
    /// drop the frame only if the whole window has nothing to say.
    fn publish_pty_frame(&mut self, says_nothing_new: fn(bool, bool) -> bool) {
        self.focused
            .refresh_projection(&mut self.focused_projection);
        self.viewport_frames += 1;
        let frame = self
            .focused
            .viewport_frame(&mut self.focused_projection)
            .unwrap();
        if says_nothing_new(
            pty_frame_is_unchanged(
                self.pending.pending_frame(),
                self.focused_on_glass.as_ref(),
                &frame,
            ),
            self.unpainted_pane_output,
        ) {
            return;
        }
        self.pending
            .publish(
                frame,
                FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::PtyOutput,
                },
            )
            .unwrap();
    }

    /// `Runtime::redraw`. No frame in the slot, no pass — and the sibling
    /// goes on showing whatever it last showed.
    fn redraw(&mut self) {
        let Some((frame, _)) = self.pending.take() else {
            return;
        };
        self.sibling
            .refresh_projection(&mut self.sibling_projection);
        self.viewport_frames += 1;
        self.sibling_on_glass = Some(
            self.sibling
                .viewport_frame(&mut self.sibling_projection)
                .unwrap(),
        );
        self.focused_on_glass = Some(frame);
        self.unpainted_pane_output = false;
        self.presents += 1;
    }

    pub(crate) fn sibling_shows(&self, needle: &str) -> bool {
        self.sibling_on_glass
            .as_ref()
            .is_some_and(|frame| frame_text(frame).contains(needle))
    }

    pub(crate) fn focused_shows(&self, needle: &str) -> bool {
        self.focused_on_glass
            .as_ref()
            .is_some_and(|frame| frame_text(frame).contains(needle))
    }
}

// Runtime needs a window and a GPU. Exercise real terminal bytes,
// projection, equivalence and frame slots here; the wiring test below
// checks the Runtime doors that the headless harness cannot call.
pub(crate) fn wheel_pane_at_top() -> PtyPresentationHarness {
    let mut pane = PtyPresentationHarness::new(20, 3);
    pane.feed_drain(b"zero\r\none\r\ntwo\r\nthree\r\nfour\r\nfive");
    pane.projection.scroll_to_top();
    pane.publish_expose_frame();
    pane.present_pending();
    assert!(pane.projection.scroll_offset_subpixels() > 0);
    pane.publications = 0;
    pane
}

pub(crate) fn flush_test_wheel(pane: &mut PtyPresentationHarness, notches: f32) -> bool {
    let burst = WheelBurst::of(MouseScrollDelta::LineDelta(0.0, notches / 2.0))
        .plus(MouseScrollDelta::LineDelta(0.0, notches / 2.0))
        .unwrap();
    let MouseScrollDelta::LineDelta(_, lines) = burst.delta() else {
        panic!("line reports stay in their own currency");
    };
    let before = pane.projection.scroll_offset_subpixels();
    let mut remainder = f64::from(lines) * pane.projection.cell_height_subpixels().get() as f64;
    pane.projection
        .scroll_by_subpixels(drain_whole_units(&mut remainder, 1.0));
    let moved = pane.projection.scroll_offset_subpixels() != before;
    pane.publish_expose_frame_inner(true);
    moved
}

pub(crate) fn scale_task(content_key: &str, width: u32) -> bt_term::InlineImageScaleTask {
    bt_term::InlineImageScaleTask {
        occurrence_id: 0,
        content_key: content_key.to_owned(),
        rgba: Arc::from([0_u8, 0, 0, 255]),
        width_px: 1,
        height_px: 1,
        display_width_px: width,
        display_height_px: width,
    }
}

pub(crate) fn assert_close(left: f32, right: f32, what: &str) {
    assert!((left - right).abs() < 0.01, "{what}: {left} is not {right}");
}

// ── the window's title, once a frame at most (0.4.5 ticket 49) ────────────
//
// `Runtime` cannot be built without a window, so the policy is run on the real
// `TitleSlot` the window keeps — the same `want` the five roads call and the same
// `take_due` `Runtime::flush_title` spends — and the two facts about the runtime
// that a slot cannot show are read through `bt_source`.

/// The display frame the title tests are paced to: 60 Hz, as `FrameClock` is
/// born with.
pub(crate) const TITLE_FRAME: Duration = pace::DEFAULT_FRAME_INTERVAL;

/// A tab holding two terminal leaves at a deliberately lopsided ratio, solved
/// against a real viewport.
///
/// Uneven on purpose: two equal panes would let a mixed-up rectangle answer
/// the right number by accident, and the pin below would prove nothing. The
/// persisted shape is the one `seats.rs` documents for a row split, which is
/// also the only constructor that lets a test choose the ratio.
pub(crate) fn solved_lopsided_split(
    dpi_milli: u32,
    render_physical: PhysicalSize<u32>,
) -> (seats::Seats, SeatLayout) {
    let term = || {
        Box::new(bt_persist::LayoutNodeV1::Leaf(
            bt_persist::LeafNodeV1::Term(bt_persist::TermLeafV1 {
                profile_id: "pwsh.exe".to_owned(),
                cwd: String::new(),
                manual_name: None,
                card_skip: 0,
                last_command: String::new(),
            }),
        ))
    };
    let node = bt_persist::LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
        dir: bt_persist::SplitDirV1::Row,
        ratio: 800_000,
        children: [term(), term()],
    });
    let seats = seats::Seats::from_persisted(&node);
    let metrics = seats::seat_metrics(dpi_milli);
    let viewport = seats::logical_viewport(
        render_physical.width,
        render_physical.height,
        seats::scale_ppm(dpi_milli),
        0,
        seats::folio_band_device_px(seats::scale_ppm(dpi_milli)),
    );
    let layout = seats
        .solve(viewport, &metrics, SizePolicy::Lawful)
        .expect("a 1600x900 window divides four to one");
    (seats, layout)
}

/// The scheduling half of `Runtime`, with no GPU in it.
///
/// Every decision below is taken by the *production* functions — `plan_grid_change`,
/// `service_pending_pty_resize`, `commit_leaf_resize`, `pty_resize_wake_deadline` — driven
/// against a real `DualPlaneSession` fed real OSC 133 bytes. What the harness itself owns is
/// only the bookkeeping the runtime does around them, and the fake ConPTY: this leaf has no
/// child, so the `PtySession::resize` call `commit_leaf_resize` makes is recorded from the
/// decision that makes it rather than from the call.
pub(crate) struct ResizeGateHarness {
    pub(crate) session: DualPlaneSession,
    pending: Option<PendingPtyResize>,
    pub(crate) grid: GridSize,
    pub(crate) conpty: GridSize,
    /// **What the child was actually told**, in order. A release is not a notification: a
    /// gesture that ends on the grid the child already holds is released like any other and
    /// tells it nothing, which is why this reads `commit_leaf_resize`'s own answer.
    pub(crate) requests: Vec<GridSize>,
    /// Releases that ended the other way: the transaction was settled and the child was left
    /// alone. Counted so a test can say the gesture *did* end, rather than only that the child
    /// heard nothing about it.
    pub(crate) settlements: usize,
    /// Whether a button is still held on the thing that is moving this rectangle — a
    /// divider, or the window's own frame in the OS's modal loop.
    pub(crate) hand_down: bool,
}

impl ResizeGateHarness {
    pub(crate) fn new(columns: u16, rows: u16) -> Self {
        install_this_machines_names();
        let grid = grid_of(columns, rows);
        Self {
            session: DualPlaneSession::new(
                NonZeroU32::from(grid.columns),
                NonZeroU32::from(grid.rows),
            ),
            pending: None,
            grid,
            conpty: grid,
            requests: Vec::new(),
            settlements: 0,
            hand_down: false,
        }
    }

    pub(crate) fn feed(&mut self, bytes: &[u8], at: Instant) {
        self.session.feed_at(bytes, at).unwrap();
    }

    /// One `WindowEvent::Resized` worth of work: the seat has already moved, the solve has
    /// answered `columns`, and this is everything `Runtime::resize` does with that answer.
    pub(crate) fn drag_to(&mut self, columns: u16, at: Instant) {
        let next = grid_of(columns, self.grid.rows.get());
        if let Some(reflow) = plan_grid_change(
            &mut self.pending,
            next,
            self.conpty,
            self.grid,
            PhysicalSize::new(u32::from(columns) * 8, 600),
            at,
        ) {
            self.session
                .resize_at(
                    NonZeroU32::from(reflow.columns),
                    NonZeroU32::from(reflow.rows),
                    at,
                )
                .unwrap();
            self.grid = reflow;
        }
    }

    /// One `WindowEvent::Resized` **from the top**: the gate `Runtime::resize` opens with,
    /// then the solve, then everything [`Self::drag_to`] already models.
    ///
    /// The solve is stood in for rather than run — it wants a GPU — by the arithmetic it
    /// actually performed on the machine this bug came off: this pane is the middle third of
    /// a three-pane window, and a cell of the shipped face at 200% is 19 physical pixels
    /// wide. `CellMetrics::grid_for_pixels`' own floor is applied, because the floor is where
    /// the reported "约 2-4 列" came from.
    pub(crate) fn window_resized(
        &mut self,
        physical: PhysicalSize<u32>,
        minimized: bool,
        at: Instant,
    ) {
        if !resize_worth_solving(minimized, physical) {
            return;
        }
        let columns = ((physical.width / 3) / 19)
            .clamp(
                u32::from(bt_render::CellMetrics::MIN_COLUMNS),
                u32::from(u16::MAX),
            )
            .try_into()
            .expect("clamped into u16 immediately above");
        self.drag_to(columns, at);
    }

    /// One `about_to_wait`: drain has already happened, and this is what the loop then owes.
    pub(crate) fn tick(&mut self, at: Instant) {
        let (pending, _) = service_pending_pty_resize(&mut self.pending, at, self.hand_down);
        let Some(pending) = pending else {
            return;
        };
        let mut reanchor_debt = false;
        let commit = commit_leaf_resize(
            &mut self.session,
            None,
            ResizeReanchor {
                pending: &mut reanchor_debt,
                integration: profiles::Integration::PowerShellOptIn,
            },
            ReleaseGrids {
                local: self.grid,
                conpty: self.conpty,
                next: pending.grid,
            },
            pending.physical,
            at,
        )
        .unwrap();
        // Exactly what `release_due_leaf_resize` writes back.
        self.grid = pending.grid;
        self.conpty = pending.grid;
        if commit.told_the_child {
            self.requests.push(pending.grid);
        } else if commit.reconciled {
            self.settlements += 1;
        }
    }

    /// One turn of `Runtime::finish_resize_if_quiescent`: whether this closed a transaction.
    pub(crate) fn quiesce(&mut self, at: Instant) -> bool {
        self.session.finish_resize_if_quiescent(at).unwrap()
    }
}

/// One shell's address, for the dispatch probes that only care that work is routed somewhere.
pub(crate) fn probe_leaf() -> ShellAddress {
    ShellAddress {
        window: WindowId::from(1_u64),
        leaf: LeafId {
            tab: TabId(1),
            seat: SeatId(1),
        },
    }
}

pub(crate) fn grid_of(columns: u16, rows: u16) -> GridSize {
    GridSize {
        columns: std::num::NonZeroU16::new(columns).unwrap(),
        rows: std::num::NonZeroU16::new(rows).unwrap(),
    }
}

/// A tab of two terminal panes and the geometry that made them, at 1x in a
/// 1600x900 window.
///
/// `leading` decides which side the *arriving* pane takes, and the tests
/// below use `true` deliberately: with the new pane on the left, the
/// survivor's corner genuinely travels, and a FLIP that only ever changed an
/// extent would leave two of the three clauses of the counter-scale untested.
pub(crate) fn split_window(
    leading: bool,
) -> (seats::Seats, SeatLayout, SeatLayout, SeatId, SeatId) {
    let metrics = seats::seat_metrics(1_000);
    let viewport = seats::logical_viewport(
        1600,
        900,
        seats::scale_ppm(1_000),
        0,
        seats::folio_band_device_px(seats::scale_ppm(1_000)),
    );
    let mut seats = seats::Seats::lone_terminal();
    let survivor = seats.identity();
    let before = seats
        .solve(viewport, &metrics, SizePolicy::Lawful)
        .expect("a lone leaf solves");
    let arriving = seats
        .split_terminal(&metrics, survivor, bt_layout::Axis::Row, leading)
        .expect("a 1600x900 window divides");
    let after = seats
        .solve(viewport, &metrics, SizePolicy::Lawful)
        .expect("the split solves");
    (seats, before, after, survivor, arriving)
}

/// Every pane's box in physical pixels, in the solver's own order — the
/// shape [`Runtime::pane_rects`] hands [`PaneMotion`].
pub(crate) fn pane_rects_of(layout: &SeatLayout) -> Vec<(SeatId, [f32; 4])> {
    layout
        .rects
        .iter()
        .filter_map(|placement| {
            let device = placement.device_rect?;
            Some((
                placement.id,
                [
                    device.left as f32,
                    device.top as f32,
                    device.right as f32,
                    device.bottom as f32,
                ],
            ))
        })
        .collect()
}

pub(crate) fn pane_box_of(layout: &SeatLayout, seat: SeatId) -> [f32; 4] {
    pane_rects_of(layout)
        .into_iter()
        .find(|(id, _)| *id == seat)
        .expect("the seat was placed")
        .1
}

/// A tab holding one files column and one terminal, with the column rooted.
pub(crate) fn tab_with_a_files_column(id: u64, root: &str) -> TabState {
    let (seats, _, _, files, _preview) =
        revive_plan(&saved_files_and_terminal(bt_persist::FilesLeafV1 {
            view: bt_persist::FilesViewV1::Files,
            root: root.to_owned(),
            open: Vec::new(),
            sel: None,
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
    assemble_tab_state(
        TabId(id),
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
    )
}

// ── C36-C38 / L157-L167: the tree's state, lazily fed ──────────────────

pub(crate) fn dir_entry(name: &str, is_dir: bool) -> files::DirEntry {
    files::DirEntry {
        name: name.to_owned(),
        is_dir,
        is_symlink: false,
    }
}

pub(crate) fn listed(entries: Vec<files::DirEntry>) -> files::DirOutcome {
    files::DirOutcome::Listed(files::DirListing {
        entries,
        omitted: 0,
        canonical: None,
    })
}

/// A tab holding one files column, and that column's seat.
pub(crate) fn files_column(root: &str) -> (TabState, SeatId) {
    let tab = tab_with_a_files_column(1, root);
    let seat = tab.seats.files()[0];
    (tab, seat)
}

/// One formula's picture, without an engine.
///
/// The pixels are never looked at — every assertion about a formula is about
/// the *box*: how much room the page gave it and where it put it. A test that
/// needed real pixels would be a test of Typst.
pub(crate) fn one_picture(
    source: &str,
    mode: MathMode,
    em_px: f32,
    (width_px, height_px, baseline_px): (u32, u32, f32),
) -> DocumentMath {
    let mut math = DocumentMath::default();
    math.insert(
        &PreviewMathKey {
            source: source.to_owned(),
            mode,
            em_milli_px: math_em_milli(em_px),
            foreground_rgb: [0, 0, 0],
        },
        PreviewMathPicture {
            key: format!("test:{source}"),
            rgba: Arc::from(vec![0_u8; (width_px * height_px * 4) as usize].into_boxed_slice()),
            width_px,
            height_px,
            baseline_px,
        },
    );
    math
}

/// The words a card is drawing, in the order it draws them.
pub(crate) fn card_text(rendered: &BuiltMarkdown) -> Vec<String> {
    rendered
        .body
        .paragraphs
        .iter()
        .map(|paragraph| {
            paragraph
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
        })
        .collect()
}

/// One decode, in the shape the store holds it.
pub(crate) fn a_decode(content: &str, native: (u32, u32), bytes: usize) -> PeekCacheEntry {
    PeekCacheEntry::Ready {
        key: content.to_owned(),
        rgba: Arc::from(vec![0_u8; bytes].into_boxed_slice()),
        width_px: native.0,
        height_px: native.1,
        native_size: None,
    }
}

/// One exact-size raster, in the shape a picture surface holds it.
pub(crate) fn a_held_raster(content: &str, (width_px, height_px): (u32, u32)) -> PeekThumbnail {
    PeekThumbnail {
        content_key: content.to_owned(),
        key: bt_term::display_texture_key(content, width_px, height_px),
        rgba: Arc::from(
            vec![0_u8; (width_px as usize) * (height_px as usize) * 4].into_boxed_slice(),
        ),
        width_px,
        height_px,
    }
}

/// The page in the sharpening fixture: one screenshot, drawn in a column
/// 800 physical pixels wide.
const SHARPEN_SOURCE: &str = "![a shot](shots/one.png)\n";

const SHARPEN_DOCUMENT: &str = r"D:\proj\README.md";

pub(crate) const SHARPEN_CONTENT: &str = "content-a";

const SHARPEN_MEASURE: f32 = 800.0;

pub(crate) const SHARPEN_NATIVE: [u32; 2] = [1024, 768];

/// The raster the page is standing on: sharpened to some earlier, narrower
/// column.
const SOFT: [u32; 2] = [240, 180];

/// One rebuild of that page, resolved the way the window resolves it: the
/// real [`answer_one_picture`] in front of two real stores, and every read
/// it asks for latched by the decode store's own `Pending`.
pub(crate) fn resolve_sharpening_page(
    peek: &mut PeekCache,
    rasters: &mut MarkdownPictures,
    standing: &DocumentPictures,
    reads: &mut usize,
) -> DocumentPictures {
    let blocks = preview::parse_markdown(SHARPEN_SOURCE);
    resolve_document_pictures(
        &blocks,
        Some(Path::new(SHARPEN_DOCUMENT)),
        bt_render::Theme::Dark,
        PictureReach::from_the_top(),
        standing,
        &mut |path, fill, standing| {
            let mut needs_pixels = false;
            let answer = answer_one_picture(
                peek,
                rasters,
                standing,
                path,
                fill,
                SHARPEN_MEASURE,
                Instant::now(),
                &mut needs_pixels,
            );
            if needs_pixels {
                *reads += 1;
                peek.insert(
                    bt_term::normalized_local_image_path_key(path),
                    PeekCacheEntry::Pending,
                );
            }
            answer
        },
    )
}

/// **A page drawing a soft raster whose decode the store has let go of**, one
/// rebuild in — the fixture both halves of the sharpening rule are asked of.
pub(crate) struct Sharpening {
    pub(crate) page: DocumentPictures,
    pub(crate) peek: PeekCache,
    pub(crate) rasters: MarkdownPictures,
    pub(crate) file: PathBuf,
    pub(crate) reads: usize,
}

pub(crate) fn a_page_that_wants_a_sharper_picture() -> Sharpening {
    let file = PathBuf::from(r"D:\proj\shots/one.png");
    let mut standing = DocumentPictures::default();
    standing.by_source.insert(
        "shots/one.png".to_owned(),
        MarkdownPicture::Ready {
            key: bt_term::display_texture_key(SHARPEN_CONTENT, SOFT[0], SOFT[1]),
            content: SHARPEN_CONTENT.to_owned(),
            rgba: Arc::from(
                vec![0_u8; (SOFT[0] as usize) * (SOFT[1] as usize) * 4].into_boxed_slice(),
            ),
            raster: SOFT,
            native: SHARPEN_NATIVE,
        },
    );
    standing.files.insert(file.clone());
    let mut peek = PeekCache::with_budget(4 * 1024 * 1024);
    let mut rasters = MarkdownPictures::default();
    let mut reads = 0_usize;
    let page = resolve_sharpening_page(&mut peek, &mut rasters, &standing, &mut reads);
    Sharpening {
        page,
        peek,
        rasters,
        file,
        reads,
    }
}

/// The same, standing between two given rows of the page.
pub(crate) fn row_box(
    at: preview_select::Place,
    len: usize,
    text: &str,
    top: f32,
    bottom: f32,
) -> PreviewTextBox {
    PreviewTextBox {
        piece: PreviewTextPiece {
            at,
            len,
            lead: 0,
            atoms: Vec::new(),
            atomic: false,
        },
        rect: [0.0, top, 200.0, bottom],
        clip: [0.0, 0.0, 400.0, 400.0],
        paragraph: Some(bt_render::PreviewParagraph {
            runs: vec![bt_render::PreviewRun {
                text: text.to_owned(),
                color: [0, 0, 0],
                mono: false,
                bold: false,
                italic: false,
                font_scale: 1.0,
                inline_box_px: None,
            }],
            rect: [0.0, top, 200.0, bottom],
            font_size_px: 14.0,
            line_height_px: 20.0,
            wrap: true,
            letter_spacing_em: 0.0,
            align_right: false,
            align_center: false,
            cell_advance: None,
        }),
    }
}

/// A source block standing at a given index, in a face eight pixels wide and
/// twenty tall — the numbers a test can do arithmetic in its head with.
pub(crate) fn source_block(index: usize, at: usize, text: &str) -> MarkdownSourceBlock {
    MarkdownSourceBlock {
        index,
        range: at..at + text.len() + 1,
        lines: preview_edit::display_lines(text),
        text: text.to_owned(),
        font_size: 14.0,
        line_height: 20.0,
        advance: 8.0,
    }
}

/// The same block in the slot the document carries it in — the monospace
/// face, which is what a fence, a table and a formula wear (§7.1.3w).
pub(crate) fn mono_caret_block(index: usize, at: usize, text: &str) -> MarkdownCaretBlock {
    MarkdownCaretBlock::Mono(source_block(index, at, text))
}

/// **The caret's block in the prose face**: the same bytes, the same range,
/// set in the body face at twenty pixels a line (§7.1.3w).
pub(crate) fn prose_caret_block(index: usize, at: usize, text: &str) -> MarkdownCaretBlock {
    let metrics = seats::preview_markdown_metrics(1.0);
    MarkdownCaretBlock::Prose(MarkdownProseBlock {
        index,
        range: at..at + text.len() + 1,
        lines: prose_source_lines(text),
        text: text.to_owned(),
        heading: text.starts_with('#'),
        font_size: metrics.font_size,
        line_height: metrics.line_height,
    })
}

/// A page of paragraphs, one word each.
pub(crate) fn prose(words: &[&str]) -> Vec<preview::MarkdownBlock> {
    words
        .iter()
        .map(|word| preview::MarkdownBlock::Paragraph(vec![preview::Span::plain(word)]))
        .collect()
}

/// How wide a run of text is in a face eight pixels to the character — the
/// half of a stub shaper that is worth naming once. The counting closure
/// around it stays in each test, because what a test asserts about a shaper
/// is how often *it* asked.
pub(crate) fn cell_ink(runs: &[bt_render::PreviewRun]) -> f32 {
    runs.iter()
        .map(|run| run.text.chars().count())
        .sum::<usize>() as f32
        * 8.0
}

/// Just the pixels a document renders to, for the tests that are about
/// pixels. The links beside them are measured by the shaper and asked for
/// where a `Renderer` is, which is not here.
pub(crate) fn markdown_body(
    body: [f32; 4],
    metrics: seats::PreviewMarkdownMetrics,
    scroll: [f32; 2],
    bars: BlockScrollPaint<'_>,
    document: (&[preview::MarkdownBlock], &preview_viewport::Layout),
    palette: &bt_render::ChromePalette,
) -> bt_render::PreviewBody {
    // The intrinsics are the fences' highlighting and nothing else here, so
    // a test about pixels can pass none and get the ink it always got.
    let document = MarkdownPage {
        blocks: document.0,
        intrinsic: &[],
        layout: document.1,
        live: MarkdownLive::default(),
    };
    build_preview_markdown_body(
        body,
        metrics,
        scroll,
        bars,
        document,
        palette,
        PageArt {
            math: &DocumentMath::default(),
            pictures: &DocumentPictures::default(),
            theme: bt_render::Theme::Dark,
        },
    )
    .body
}

/// The bars at rest: the offsets under test, nothing lit, one device pixel
/// to the logical one.
/// A document key with both generations and the theme at rest.
///
/// Every test below that is not about a formula or a picture wants the same
/// four answers — the buffer, the face, the width and the scale — and naming
/// the other four at each of fourteen call sites said nothing except that
/// they were still zero.
pub(crate) fn document_key(
    buffer: &preview::PreviewBuffer,
    md_source: bool,
    width_px: f32,
    scale: f32,
) -> PreviewDocumentKey {
    preview_document_key(
        buffer,
        md_source,
        width_px,
        scale,
        PageArtKey {
            math_generation: 0,
            body_ink: [0, 0, 0],
            picture_generation: 0,
            picture_reach: PictureReach::from_the_top(),
            theme: bt_render::Theme::Dark,
        },
        None,
    )
}

pub(crate) fn rested_bars(offsets: &[f32]) -> BlockScrollPaint<'_> {
    BlockScrollPaint {
        offsets,
        lit: None,
        scale: 1.0,
    }
}

/// Whether one rectangle is wholly inside another.
pub(crate) fn inside(rect: [f32; 4], outer: [f32; 4]) -> bool {
    rect[0] >= outer[0] && rect[1] >= outer[1] && rect[2] <= outer[2] && rect[3] <= outer[3]
}

/// A measurer for the foot tests: every glyph half the point size wide,
/// which is about what Segoe UI averages and is exact enough to do
/// arithmetic with.
///
/// Deliberately not the real shaper. What these tests are about is the
/// *division* of a strip between two runs, and a fixture whose widths a
/// reader cannot compute in their head would be measuring the font instead
/// of the rule.
pub(crate) fn ruler(text: &str, size: f32) -> f32 {
    text.chars().count() as f32 * size / 2.0
}

// ── slice 3: quick edit ─────────────────────────────────────────────────

/// A buffer with a body, for the document tests below.
pub(crate) fn text_buffer(name: &str, body: &str) -> preview::PreviewBuffer {
    let mut buffer = preview::PreviewBuffer::new(
        preview::PreviewSource::file(format!(r"C:\w\{name}")),
        name.to_owned(),
    );
    buffer.accept(preview::HeadOutcome::Read {
        text: body.to_owned(),
        truncated: false,
        mtime: None,
        content_says_text: true,
        encoding: preview::HeadEncoding::Utf8,
        lossy: false,
    });
    buffer
}

/// One lone-terminal tab holding exactly this shell.
pub(crate) fn tab_holding(leaf: LeafSession) -> TabState {
    let seats = seats::Seats::lone_terminal();
    let identity = seats.identity();
    let (layout, overflow) = cross_solve(&seats);
    assemble_tab_state(
        TabId(1),
        BTreeMap::from([(identity, leaf)]),
        BTreeMap::new(),
        preview::PreviewPool::default(),
        PreviewPanes::default(),
        BTreeMap::new(),
        identity,
        TabSeed::default(),
        seats,
        layout,
        overflow,
    )
}

/// A buffer with a body already in it, so a test can talk about content
/// without a disk. `dirty` is the caller's — it is the field every clause of
/// the migration law turns on.
pub(crate) fn buffer_saying(path: &str, name: &str, body: &str) -> preview::PreviewBuffer {
    let mut buffer =
        preview::PreviewBuffer::new(preview::PreviewSource::file(path), name.to_owned());
    buffer.accept(preview::HeadOutcome::Read {
        text: body.to_owned(),
        truncated: false,
        mtime: None,
        content_says_text: true,
        encoding: preview::HeadEncoding::Utf8,
        lossy: false,
    });
    buffer
}

/// A tab of one terminal and one preview pane, the pane showing the **first**
/// buffer of the pool it is handed and the rest of the pool standing behind
/// it as history.
pub(crate) fn tab_with_a_preview(
    id: u64,
    buffers: Vec<preview::PreviewBuffer>,
) -> (TabState, SeatId) {
    let mut seats = seats::Seats::lone_terminal();
    let preview_seat = seats
        .add_preview(&cross_metrics())
        .expect("the preview seat lands");
    let focused = seats.identity();
    let mut pool = preview::PreviewPool::default();
    let showing = buffers.first().map(|buffer| buffer.source.clone());
    for buffer in buffers {
        pool.insert(buffer);
    }
    let mut panes = PreviewPanes::default();
    panes.entry(seat_of(TabId(id), preview_seat)).buffer = showing;
    let (layout, overflow) = cross_solve(&seats);
    let tab = assemble_tab_state(
        TabId(id),
        BTreeMap::from([(focused, leaf_saying("SHELL"))]),
        BTreeMap::new(),
        pool,
        panes,
        BTreeMap::new(),
        focused,
        TabSeed::default(),
        seats,
        layout,
        overflow,
    );
    (tab, preview_seat)
}

/// A folder of this test's own, emptied first so a previous run cannot
/// answer for this one.
pub(crate) fn disk_scratch(name: &str) -> PathBuf {
    let dir = bt_testpath::temp_path(&format!("bt-disk-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir
}

/// Open a real file into a buffer the way this window does: name it, then
/// give it the head the disk answered.
pub(crate) fn buffer_read_from(path: &Path) -> preview::PreviewBuffer {
    let mut buffer = preview::PreviewBuffer::new(
        preview::PreviewSource::file(path),
        files_row_display_name(path),
    );
    buffer.accept(preview::read_head(path));
    buffer
}

/// NTFS records the last-write time on a coarse tick, so two writes inside
/// one tick carry the same mtime and the rule under test never fires. The
/// disk is moved forward by hand so the test reads the rule, not the clock.
pub(crate) fn move_the_disk_forward(path: &Path) {
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    std::fs::File::options()
        .write(true)
        .open(path)
        .expect("open for touch")
        .set_modified(later)
        .expect("set mtime");
}

/// Every seat of `tab`, paired with the id it will answer to after the
/// renumbering — the `arrived` list a real merge hands over.
pub(crate) fn arriving_as(tab: &TabState, first: u64) -> Vec<(SeatId, SeatId)> {
    tab.seats
        .tree()
        .seats_in_order()
        .iter()
        .enumerate()
        .map(|(index, seat)| (seat.id, SeatId(first + index as u64)))
        .collect()
}

// ── U12 stage C: the cross-boundary gestures, over real tabs ─────────────
//
// `Runtime` owns a `Renderer` and cannot be stood up in a unit test, which
// is the whole reason the three verbs below are free functions over
// `TabState` rather than methods on the window: everything a tear-out, a
// merge and a replace *decide* is a fact about two tabs, and the only thing
// the window contributes is the solved rectangles and the tab ids. Those are
// arguments here. What follows therefore runs the real verbs against real
// `Seats` built by the real `Edit` chain, real plans built by the real
// `plan_drop`, and real `DualPlaneSession`s with distinct bytes fed into
// each one — so "the session travelled" is checked against something only
// that session knows, rather than against a count that any freshly spawned
// shell would also satisfy.

pub(crate) const CROSS_DPI: u32 = 1_000;

const CROSS_W: u32 = 1_600;

const CROSS_H: u32 = 900;

pub(crate) fn cross_metrics() -> SeatMetrics {
    seats::seat_metrics(CROSS_DPI)
}

fn cross_view() -> LogicalRect {
    seats::logical_viewport(
        CROSS_W,
        CROSS_H,
        seats::scale_ppm(CROSS_DPI),
        0,
        seats::folio_band_device_px(seats::scale_ppm(CROSS_DPI)),
    )
}

/// The window's contribution, supplied by hand: the real solver against a
/// real viewport (red line L10 — nothing here invents a rectangle).
pub(crate) fn cross_solve(seats: &seats::Seats) -> (SeatLayout, Option<seats::FitOverflow>) {
    (
        seats
            .solve(cross_view(), &cross_metrics(), SizePolicy::Lawful)
            .expect("the constructed trees all fit 1600x900"),
        None,
    )
}

pub(crate) fn card_restore_fixture() -> LeafSession {
    install_this_machines_names();
    let mut leaf = leaf_saying("");
    leaf.session = DualPlaneSession::new(nonzero_u32(10), nonzero_u32(40));
    leaf.grid = GridSize {
        columns: std::num::NonZeroU16::new(10).unwrap(),
        rows: std::num::NonZeroU16::new(40).unwrap(),
    };
    leaf.conpty_grid = leaf.grid;
    for number in 1..=100 {
        leaf.session
            .feed(format!("H{number:03}\r\n").as_bytes())
            .unwrap();
    }
    let live = (1..=20)
        .map(|number| format!("L{number:03}:abcdefghijk"))
        .collect::<Vec<_>>()
        .join("\r\n");
    assert!(live.split("\r\n").all(|line| line.len() == 16));
    leaf.session.feed(live.as_bytes()).unwrap();
    leaf.card_skip = 130;
    assert_eq!(leaf.session.document().entries().len(), 100);
    assert_eq!(
        (0..40)
            .filter(|row| leaf
                .session
                .live_row(*row)
                .is_some_and(|row| row.cells.iter().any(|cell| !cell.text.trim().is_empty())))
            .count(),
        40
    );
    assert_eq!(
        focus_thumb::transcript_tail(&leaf.session, 40, 200, 0)
            .0
            .len(),
        140
    );
    assert_eq!(card_restore_first(&leaf), "H007");
    leaf
}

/// The top row of this leaf's four-row card, drawn exactly as the product
/// draws it — and, since T-CARD-NO-PASSIVE-CLAMP, drawn without touching the
/// leaf's number: the draw's own clamp lives in `transcript_tail` and writes
/// nothing back, so looking at a card is not a way to move it.
pub(crate) fn card_restore_first(leaf: &LeafSession) -> String {
    focus_thumb::transcript_tail(&leaf.session, 40, 4, leaf.card_skip).0[0].clone()
}

pub(crate) fn card_restore_widen(leaf: &mut LeafSession) {
    schedule_leaf_grid_change(
        leaf,
        GridSize {
            columns: std::num::NonZeroU16::new(40).unwrap(),
            rows: std::num::NonZeroU16::new(40).unwrap(),
        },
        PhysicalSize::new(400, 400),
        Instant::now(),
        LeafOnStage::Shown,
        "resize card skip fixture",
        card_trace::Pane::untraced(),
    )
    .unwrap();
    assert_eq!(
        focus_thumb::transcript_tail(&leaf.session, 40, 200, 0)
            .0
            .len(),
        120
    );
}

pub(crate) fn card_restore_resize(
    leaf: &mut LeafSession,
    columns: u16,
    rows: u16,
    stage: LeafOnStage,
) {
    let next = GridSize {
        columns: std::num::NonZeroU16::new(columns).unwrap(),
        rows: std::num::NonZeroU16::new(rows).unwrap(),
    };
    schedule_leaf_grid_change(
        leaf,
        next,
        PhysicalSize::new(400, 400),
        Instant::now(),
        stage,
        "card skip resize",
        card_trace::Pane::untraced(),
    )
    .unwrap();
    if stage == LeafOnStage::Behind {
        let mut pending = false;
        commit_leaf_resize(
            &mut leaf.session,
            None,
            ResizeReanchor {
                pending: &mut pending,
                integration: profiles::Integration::None,
            },
            ReleaseGrids {
                local: leaf.grid,
                conpty: leaf.conpty_grid,
                next,
            },
            PhysicalSize::new(400, 400),
            Instant::now(),
        )
        .unwrap();
        leaf.grid = next;
        leaf.conpty_grid = next;
    }
}

pub(crate) fn card_restore_settle(leaf: &mut LeafSession) {
    leaf.session.mark_pty_resize_requested_at(
        nonzero_u32(leaf.grid.columns.get()),
        nonzero_u32(leaf.grid.rows.get()),
        Instant::now(),
    );
    let deadline = leaf.session.resize_finish_deadline().unwrap();
    leaf.session.finish_resize_if_quiescent(deadline).unwrap();
}

/// One shell with a word in it that no other shell in the test has.
///
/// `pty: None` — this is the same shell-less mode `BT_PROBE_INPUT` uses, so
/// nothing here spawns a ConPTY, and the scrollback is still a real
/// `DualPlaneSession`'s.
pub(crate) fn leaf_saying(text: &str) -> LeafSession {
    install_this_machines_names();
    let columns = NonZeroU32::new(40).unwrap();
    let rows = NonZeroU32::new(4).unwrap();
    let mut session = DualPlaneSession::with_quotas_and_cell_height(
        columns,
        rows,
        DEFAULT_STAGING_QUOTA,
        DEFAULT_FROZEN_LINE_QUOTA,
        std::num::NonZeroI64::new(22 * bt_viewport::SUBPIXELS_PER_PX).unwrap(),
    );
    session
        .feed(text.as_bytes())
        .expect("feed the marker bytes");
    let projection = session.new_projection(session.layout_key());
    let grid = GridSize {
        columns: std::num::NonZeroU16::new(40).unwrap(),
        rows: std::num::NonZeroU16::new(4).unwrap(),
    };
    LeafSession {
        // A fixture is a shell for the purposes of being told apart from the
        // next one, so it takes a number from the same counter production
        // takes one from.
        incarnation: next_incarnation(),
        pty: None,
        foreground_program_cadence: foreground_program::Cadence::default(),
        // No ConPTY, so no reader thread, so nothing to wake — see the field.
        wake: None,
        // Aimed at the tail, like every shell that has not been aimed.
        card_skip: 0,
        // A shell-less fixture is not a shell of some other kind: these
        // panes exist to carry scrollback, and the default profile is what
        // the pane they stand in for would have been started as.
        profile: profiles::fallback_profile_id().to_owned(),
        paste_recipient: profiles::paste_recipient(
            profiles::fallback_profile(),
            &bt_pty::SystemShellEnvironment,
        ),
        // And the door that profile is served through, which is the one the
        // spawn would have read for it.
        integration: profiles::row_of(profiles::fallback_profile_id())
            .map_or(profiles::Integration::None, |row| profiles::served_by(&row)),
        // And no program either, which is the honest shape of the same
        // fact: nothing was started, so nothing can have announced itself.
        program: None,
        // Nor a place: a fixture was never put down anywhere.
        spawn_place: None,
        born_named: false,
        session,
        attention: attention::AttentionLedger::default(),
        bell_reported: false,
        attention_clock: attention_wire::WaitClock::default(),
        // A fixture has no shell, so nothing was ever told a capability — and an empty string
        // is one no arriving message can match, which is the honest shape of that.
        attention_capability: String::new(),
        projection,
        // A fixture is a new view, so it is at 100 % (ticket 37), at the default face measured
        // by the production service. Its session keeps the fixture's own 22-pixel rows; a test
        // that needs the two to agree applies these through `apply_leaf_metrics`.
        text_scale: TextScale::ACTUAL,
        metrics: fixture_cell_metrics(1.0, 16.0),
        presented_metrics: fixture_cell_metrics(1.0, 16.0),
        thumb_awake: Instant::now(),
        column_awake: Instant::now(),
        grid,
        conpty_grid: grid,
        last_finished_command: None,
        // A fixture's bytes were fed before it was a leaf; whether they marked a
        // prompt is heard the way the drain hears it, by asking.
        has_rail: false,
        pending_pty_resize: None,
        pending_psreadline_resize_reanchor: false,
        output_revision: 0,
        last_seen_revision: 0,
        last_presented_frame: None,
        frame_image_references: FrameImageReferences::default(),
        // Nothing was started, so there is no `$PROFILE` this fixture could
        // be owed an answer about.
        // Nothing has been pasted into a fixture.
        pending_paste: None,
        // A fixture is not a restore, so nothing is owed to its prompt.
        pending_typing: None,
        // Nor a shell being born: it was never going to have one.
        birth: None,
        successor: None,
    }
}

/// What the shell filed under `seat` has on its screen — the identity check.
pub(crate) fn leaf_says(tab: &TabState, seat: SeatId) -> String {
    tab.sessions
        .get(&seat)
        .unwrap_or_else(|| panic!("no session filed under {seat:?}"))
        .session
        .terminal()
        .visible_text()
        .join("")
        .trim()
        .to_string()
}

pub(crate) fn tab_texts(tab: &TabState) -> Vec<String> {
    tab.seats
        .terminals()
        .into_iter()
        .map(|seat| leaf_says(tab, seat))
        .collect()
}

/// A tree of `panes` terminals, built by the same `SplitSeat` edit the split
/// verb runs, so its ids and its ratios are the ones a real tab would have.
pub(crate) fn cross_seats(panes: usize) -> seats::Seats {
    let mut seats = seats::Seats::lone_terminal();
    let mut last = SeatId(1);
    for _ in 1..panes {
        last = seats
            .split_terminal(&cross_metrics(), last, Axis::Row, false)
            .expect("the split fits");
    }
    seats
}

/// A tab of `texts.len()` panes, each shell saying its own word.
pub(crate) fn cross_tab(id: u64, texts: &[&str]) -> TabState {
    let seats = cross_seats(texts.len());
    let terminals = seats.terminals();
    let sessions: BTreeMap<SeatId, LeafSession> = terminals
        .iter()
        .zip(texts)
        .map(|(seat, text)| (*seat, leaf_saying(text)))
        .collect();
    let focused = terminals[0];
    let (layout, overflow) = cross_solve(&seats);
    assemble_tab_state(
        TabId(id),
        sessions,
        // `cross_seats` builds a row of terminals and nothing else.
        BTreeMap::new(),
        preview::PreviewPool::default(),
        PreviewPanes::default(),
        BTreeMap::new(),
        focused,
        TabSeed::default(),
        seats,
        layout,
        overflow,
    )
}

/// `commit_layout_drop`'s two lines that do not need a window: build the
/// plan the preview drew, adopt exactly it (D4), and answer the renaming.
pub(crate) fn cross_merge(
    source: &seats::Seats,
    target: &mut TabState,
    aim: seats::LayoutAim,
) -> Vec<(SeatId, SeatId)> {
    let plan = target
        .seats
        .plan_drop(
            &cross_metrics(),
            cross_view(),
            aim,
            seats::DropCargo::Layout(source.tree()),
        )
        .expect("the aim names a seat this tree has");
    assert!(plan.fits(), "the constructed merge fits 1600x900");
    let arrived = plan.arrived.clone();
    target.seats.adopt_drop(plan).expect("a fitting plan lands");
    arrived
}

// ── §7.1.6k: a pane moved into another tab, over real tabs ──────────────

/// The window's contribution to [`pane_into_tab`], supplied by hand — the
/// same shape [`cross_solve`] has, and the same viewport.
pub(crate) fn cross_move_at(
    from: &mut TabState,
    into: &mut TabState,
    seat: SeatId,
    aim: seats::LayoutAim,
    watched: bool,
) -> Option<PaneMove> {
    let metrics = cross_metrics();
    // Which tab the window is showing. `watched` here has always meant "the
    // tab the pane is going to is the one on screen", so the tab it names is
    // the target's — and since B4 the function works both halves out for
    // itself from the two tabs it was handed.
    let watching = watched.then_some(into.id);
    pane_into_tab(
        from,
        into,
        seat,
        &PaneArrival {
            metrics: &metrics,
            viewport: cross_view(),
            aim,
        },
        watching,
        cross_solve,
    )
}

/// The tab list's door: *"追加为树末尾分屏"*, which is the rim and nothing
/// else. Stated once here so that the older §7.1.6k tests go on saying what
/// they always said while §7.1.6k′'s say something wider.
pub(crate) fn cross_move(
    from: &mut TabState,
    into: &mut TabState,
    seat: SeatId,
    edge: seats::DropEdge,
    watched: bool,
) -> Option<PaneMove> {
    cross_move_at(from, into, seat, seats::LayoutAim::Rim(edge), watched)
}

// ── the document travels with the pane (defect #187, 2026-08-29) ─────────
//
// §7.1.3 has said "pane 拆出/被顶出时携带其当前缓冲(同一对象)" since it was
// written, and [`pane_into_new_tab`] has done it since it was written. The
// *other* door — [`pane_into_tab`], which every cross-tab move that is not a
// tear-out goes through — carried the view and left the buffer, on the older
// reading that "buffers live in the tab's pool, which does not migrate". A
// view is a `PreviewSource` and a pool lookup, so the pane arrived naming a
// document its new tab had never heard of: the empty state, and the file
// gone.
//
// The four cells below are the four ways a preview pane changes tab, and
// they are four because the user's ruling names four and not because the
// code has four paths — it has two, and the point of the last cell is to say
// which of them each gesture is.

/// A buffer with an unsaved edit in it, through the one door an edit takes.
pub(crate) fn edited_buffer(
    path: &str,
    name: &str,
    body: &str,
    added: &str,
) -> preview::PreviewBuffer {
    let mut buffer = buffer_saying(path, name, body);
    let added = added.to_owned();
    assert!(
        buffer.edit_content(|content| {
            content.push_str(&added);
            true
        }),
        "the fixture's own edit landed"
    );
    assert!(buffer.dirty, "so the buffer is dirty, which is the point");
    buffer
}

/// What a preview pane is showing, read the way the window reads it: the
/// source the *pane* names, resolved in the pool the *tab* holds.
///
/// `None` is exactly the defect — a pane naming a document nobody in this
/// tab can find is a pane that draws the empty state.
pub(crate) fn document_on(tab: &TabState, seat: SeatId) -> Option<&str> {
    let pane = tab.preview_panes.get(tab.preview_here(seat))?;
    let source = pane.buffer.as_ref()?;
    tab.preview_pool.get(source)?.content.as_deref()
}

// ── §7.1.6k⁗: the picture follows its pane, and the lane follows with it ─

/// A tab of one terminal and one preview pane **holding a picture** — the
/// shape [`tab_with_a_preview`] cannot make, because a picture is not a
/// buffer and lives on [`PreviewPane::image`] instead of in the pool.
pub(crate) fn tab_with_a_picture(id: u64, path: &str) -> (TabState, SeatId) {
    let mut seats = seats::Seats::lone_terminal();
    let preview_seat = seats
        .add_preview(&cross_metrics())
        .expect("the preview seat lands");
    let focused = seats.identity();
    let mut panes = PreviewPanes::default();
    panes.entry(seat_of(TabId(id), preview_seat)).image =
        Some(PreviewImageState::new(PathBuf::from(path)));
    let (layout, overflow) = cross_solve(&seats);
    let tab = assemble_tab_state(
        TabId(id),
        BTreeMap::from([(focused, leaf_saying("SHELL"))]),
        BTreeMap::new(),
        preview::PreviewPool::default(),
        panes,
        BTreeMap::new(),
        focused,
        TabSeed::default(),
        seats,
        layout,
        overflow,
    );
    (tab, preview_seat)
}

/// The two files this family of gates is written about, spelled once.
pub(crate) const SHOT_PATH: &str = r"D:\shots\B1-rest.png";

/// The files this tab is drawing, as the pictures its panes are holding
/// name them — [`TabState::seat_pictures`] read through to the disk.
pub(crate) fn pictures_drawn(tab: &TabState) -> Vec<&Path> {
    tab.seat_pictures()
        .into_iter()
        .filter_map(|surface| {
            Some(
                tab.preview_panes
                    .get(surface)?
                    .image
                    .as_ref()?
                    .path
                    .as_path(),
            )
        })
        .collect()
}

// ── peek and pin (owner ruling 2026-09-23, ticket 35) ───────────────────

/// A trigger that is a plain button, as the popup register writes one.
pub(crate) fn chevron_button(control: PopoverTrigger) -> Option<OwnTrigger> {
    Some(OwnTrigger {
        control,
        keeps_the_press: false,
    })
}

/// The three `⌄` controls, each with the popup it raises.
pub(crate) fn the_three_chevrons() -> [(Popup, PopoverTrigger); 3] {
    [
        (
            Popup::Profile,
            PopoverTrigger::Chrome(seats::ChromeTarget::NewTabMenu),
        ),
        (
            Popup::Pane,
            PopoverTrigger::Chrome(seats::ChromeTarget::PaneMenu(SeatId(1))),
        ),
        (
            Popup::File,
            PopoverTrigger::Rail(
                PreviewSurface::Seat(LeafId {
                    tab: TabId(1),
                    seat: SeatId(1),
                }),
                seats::PreviewRailPart::OpenWith,
            ),
        ),
    ]
}

/// The gates after a rest on `popup`'s `⌄` has matured and opened a peek —
/// the rest told through the one `observe`, the `Open` read through `due` and
/// answered by clearing the clocks, as `advance_chevrons` does.
pub(crate) fn peek_open(popup: Popup, start: Instant) -> ChevronGates {
    use profiles::ChevronPointer::{Away, Button};
    let mut gates = ChevronGates::default();
    let on = |this: Popup| {
        if this == popup {
            (Button, false)
        } else {
            (Away, false)
        }
    };
    gates.observe(on(Popup::Profile), on(Popup::Pane), on(Popup::File), start);
    let gate = gates.gate(popup).expect("a `⌄` governs this menu");
    assert_eq!(
        gate.due(start + profiles::CHEVRON_HOVER_OPEN_DELAY),
        Some(profiles::ChevronAction::Open)
    );
    gate.clear();
    assert!(!gate.is_pinned(), "a rest opens a peek");
    gates
}

/// The hand leaving every button and every menu, with `popup`'s menu up.
pub(crate) fn hand_leaves(gates: &mut ChevronGates, popup: Popup, now: Instant) {
    use profiles::ChevronPointer::Away;
    let up = |this: Popup| (Away, this == popup);
    gates.observe(up(Popup::Profile), up(Popup::Pane), up(Popup::File), now);
}

pub(crate) const TARGET: bt_layout::SeatId = bt_layout::SeatId(2);

pub(crate) fn centre() -> DropLanding {
    DropLanding::SeatCentre { target: TARGET }
}

// ── the bare strip in the corner (user report 2026-08-13) ───────────────

pub(crate) fn listing(names: &[(&str, bool)]) -> files::DirOutcome {
    files::DirOutcome::Listed(files::DirListing {
        entries: names
            .iter()
            .map(|(name, is_dir)| files::DirEntry {
                name: (*name).to_owned(),
                is_dir: *is_dir,
                is_symlink: false,
            })
            .collect(),
        omitted: 0,
        canonical: None,
    })
}

/// **The engine ledger is a fact about a process, and this binary is one**
/// (§7.42 ⑦; the same gate `bt_platform`'s own engine tests take, for the
/// same reason and in its own process).
///
/// `engines_outstanding` counts every engine this *process* holds. Two tests
/// in here open engines, `cargo` runs them on one thread per core, and a
/// test that reads the counter while another is moving it reads a race — the
/// red that three individually-correct arms added up to the first time the
/// whole workspace ran together. So every test in this binary that concludes
/// anything from the number takes this first.
///
/// It is not a fix to the invariant and must not be read as one: engines are
/// perfectly safe to open concurrently and nothing in the product serialises
/// them. What cannot be done concurrently is reading a global counter and
/// drawing a conclusion from the value.
pub(crate) fn ledger_gate() -> std::sync::MutexGuard<'static, ()> {
    static GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A poisoned gate means nothing: what it guards is a `()`. Taking the
    // inner value is what stops one red test turning every later one into a
    // second, unrelated failure.
    GATE.lock().unwrap_or_else(|held| held.into_inner())
}

/// **Wait for the ledger to reach `target`**, and answer where it got to.
///
/// `Engine::open` does not wait for the engine to be built (§7.44 ⑫), so "an
/// engine exists" becomes true shortly *after* the open returns rather than
/// before it: the counter is moved on the engine's own thread, where the
/// `IMFMediaEngine` is actually made — and taken off on that thread too,
/// possibly after a `shutdown` that ran out of its budget has returned. A test
/// that reads the counter on the next instruction is reading that race.
///
/// So the wait is on the ledger's own signal: it ends the moment an engine
/// thread moves the count onto `target`, whatever the machine's load, and
/// [`crate::lane::PATIENCE`] only bounds a wait for a movement that is never
/// coming — **which is a red here, by this helper**, naming the patience and
/// the count that stood, so no caller can pass by the wait running out. A
/// caller that wants the count itself asks
/// `bt_platform::video::engine::engines_outstanding_reaching`. Under
/// [`ledger_gate`], so nothing else is moving the number while this watches it.
pub(crate) fn engines_settling_to(target: u64) -> u64 {
    let reached =
        bt_platform::video::engine::engines_outstanding_reaching(target, crate::lane::PATIENCE);
    assert_eq!(
        reached,
        target,
        "the engine ledger did not reach {target} within the lane suite's patience ({:?});          {reached} engines stood",
        crate::lane::PATIENCE
    );
    reached
}

/// A real folder with a real file in it, for the glance-foot tests: the path
/// the card holds is a path on this disk, not a spelling made up for the test.
pub(crate) fn glance_fixture(name: &str) -> (PathBuf, PathBuf) {
    let folder = bt_testpath::temp_path(&format!("folio-glance-foot-{name}"));
    std::fs::create_dir_all(folder.join("notes")).expect("the fixture folder is made");
    let file = folder.join("notes").join("plan.md");
    std::fs::write(&file, "# plan\n").expect("the fixture file is written");
    (folder, file)
}

// ── ticket 32: the command rail never covers text ─────────────────────────

/// A shell that ran one command which failed and is back at its prompt: `OSC 133`
/// `A`/`B`/`C`/`D;1` and a fresh `A`, the bytes an integrated PowerShell prints
/// around `t15nosuchcommand`. Fed through a real session, so the ledger the rail
/// is laid out from is the one the product would hold.
pub(crate) const RAIL_FAILED_THEN_PROMPT: &str = "\u{1b}]133;A\u{7}PS> \u{1b}]133;B\u{7}t15nosuchcommand\r\n\
     \u{1b}]133;C\u{7}not recognized\r\n\u{1b}]133;D;1\u{7}\u{1b}]133;A\u{7}PS> \u{1b}]133;B\u{7}";

/// A pane body `logical` wide at `scale`, standing somewhere other than the
/// window's origin, because a real pane in a split does.
pub(crate) fn rail_test_body(logical: u32, scale: f64) -> SeatViewport {
    SeatViewport {
        x: (37.0 * scale).round() as u32,
        y: (61.0 * scale).round() as u32,
        width: (f64::from(logical) * scale).round() as u32,
        height: (600.0 * scale).round() as u32,
    }
}

// ── the resize present gate and the rail's arrival (0.4.4 ticket 47) ───────
//
// `Runtime::redraw` cannot be built without a window, so the gate it asks is a method of the tab
// (`TabState::admit_resize_present`), and these tests run it on a real tab holding a real leaf:
// the real `hear_first_mark` the drain asks, the real `LeafSession::grid_for` every solve asks and
// the real `schedule_leaf_grid_change` every solve goes through, and frames the leaf's own
// session composes.

/// Re-solve the tab's focused pane into `body`, as every road that re-solves the panes does.
pub(crate) fn resolve_focused_pane(
    tab: &mut TabState,
    metrics: &bt_render::CellMetrics,
    body: SeatViewport,
) {
    let leaf = tab.focused_mut().expect("the tab holds a shell");
    // The pane at these metrics (ticket 37: a pane's grid is solved at its own metrics).
    apply_leaf_metrics(leaf, *metrics);
    let next = leaf.grid_for(body);
    schedule_leaf_grid_change(
        leaf,
        next,
        PhysicalSize::new(body.width, body.height),
        Instant::now(),
        LeafOnStage::Shown,
        "ticket 47",
        card_trace::Pane::untraced(),
    )
    .expect("carry the solved grid to the pane");
}

/// The frame the tab's focused pane composes now.
pub(crate) fn focused_frame(tab: &mut TabState) -> ViewportFrame {
    let leaf = tab.focused_mut().expect("the tab holds a shell");
    leaf.session.refresh_projection(&mut leaf.projection);
    leaf.session
        .viewport_frame(&mut leaf.projection)
        .expect("compose the pane's frame")
}

// ── the multi-line paste card (0.4.4 ticket 02) ─────────────────────────────
//
// `Runtime` cannot be built without a window, so the decision `Runtime::deliver_paste` makes is a
// free function over a real tab (`stage_paste`), and these tests run it on real leaves: a real
// `DualPlaneSession` whose modes are set by the bytes a program would print, a paste prepared by
// the real `prepare_clipboard_paste` / `prepare_dropped_paste`, and the answer's bytes produced by
// the real `paste_text`. Where the claim is about the window's own wiring — which rung the keys
// reach, which door the answer leaves by — it is pinned on the method bodies through `bt_source`.

/// A shell-less leaf whose paste grammar is `grammar`, after the program printed `printed`.
pub(crate) fn paste_leaf(grammar: shell_literal::ShellGrammar, printed: &[u8]) -> LeafSession {
    let mut leaf = leaf_saying("");
    leaf.paste_recipient.encoder.grammar = grammar;
    leaf.session
        .feed(printed)
        .expect("feed the program's own bytes");
    leaf
}

/// One tab holding `leaf`, and the address a paste into it is aimed at.
pub(crate) fn paste_tab(leaf: LeafSession) -> (TabState, PasteTarget) {
    let tab = tab_holding(leaf);
    let (seat, leaf) = tab.sessions.iter().next().expect("a lone terminal");
    let target = PasteTarget {
        tab: tab.id,
        seat: *seat,
        incarnation: leaf.incarnation,
    };
    (tab, target)
}

/// The clipboard holding `text`, pasted into `target` the way `paste_from_clipboard_into` does,
/// by the Windows build.
pub(crate) fn paste_text_into(
    tab: &mut TabState,
    target: PasteTarget,
    text: &str,
    ask: bool,
) -> StagedPaste {
    paste_text_into_on(tab, target, text, ask, bt_platform::HostPlatform::Windows)
}

/// [`paste_text_into`], by the build for `host`.
pub(crate) fn paste_text_into_on(
    tab: &mut TabState,
    target: PasteTarget,
    text: &str,
    ask: bool,
    host: bt_platform::HostPlatform,
) -> StagedPaste {
    let recipient = tab.sessions[&target.seat].paste_recipient.clone();
    let prepared = prepare_clipboard_paste(
        Ok(bt_platform::ClipboardPayload::Text(text.to_owned())),
        &recipient,
        false,
    );
    stage_paste(
        tab,
        target,
        prepared.text.expect("text arrives as text"),
        prepared.clipboard_text,
        ask,
        host,
        "test paste",
    )
}

pub(crate) const THREE_LINES: &str = "dir\r\necho one\r\nver";

/// The restore card's rung in [`Runtime::keyboard_input`], squeezed, as 0.4.5 ticket 57 built it:
/// `Enter` answers with the focused button, an Esc the card does not consume falls to `_`, and the
/// rung returns for every key.
pub(crate) const RESTORE_CARD_RUNG: &str = "ifself.restore_card_is_up(){if!event.repeat{match&event.logical_key{Key::Named(NamedKey::Enter)=>{self.answer_restore_prompt(restore::FOCUSED_ANSWER)?;}Key::Named(NamedKey::Escape)ifself.window.restore_prompt.consumes_escape()=>{self.window.restore_prompt.close();ifself.refresh_chrome(){self.present_chrome_change()?;}}_=>{}}}returnOk(());}";

// ── the PowerShell input line (0.4.4 ticket 03) ─────────────────────────────

/// What PowerShell's integration prints at an idle prompt: `A`, the prompt, `B`.
pub(crate) const POWERSHELL_PROMPT: &[u8] = b"\x1b]133;A\x07PS C:\\> \x1b]133;B\x07";

/// The bytes the one writer writes for a staged paste, captured — `paste_body`, the tail of
/// `Runtime::send_paste` — or `None` for a held one.
pub(crate) fn staged_bytes_sent(
    tab: &mut TabState,
    seat: SeatId,
    staged: &StagedPaste,
) -> Option<Vec<u8>> {
    let body = match staged {
        StagedPaste::Send(text) => PasteBody::Text(text),
        StagedPaste::InputLine(bytes) => PasteBody::InputLine(bytes),
        StagedPaste::Held => return None,
    };
    let leaf = tab.sessions.get_mut(&seat).expect("the seat has a shell");
    let mut sent = Vec::new();
    paste_body(&mut leaf.session, &mut leaf.projection, body, |bytes| {
        sent.push(bytes.to_vec());
        Ok(())
    })
    .expect("a capture cannot fail");
    assert_eq!(sent.len(), 1, "a paste is one write");
    sent.pop()
}

/// **This test's thread is the window thread, and its loop is running** — what every test that
/// reaches an owner-thread door's minting statement calls first (design note 2026-09-26, revision
/// (e)2's test protocol). The thread is metered as the window thread is, so a door's witness reads
/// its admissions with `hang_watch::admissions_on_this_thread`. Test code: outside the universe in
/// which the phase writers' product call sites are pinned.
pub(crate) fn on_the_window_thread() {
    crate::hang_watch::meter_this_test_thread();
    assert!(
        bt_platform::admission::enter_window_thread(),
        "this test's thread enters as the window thread once"
    );
    assert!(
        bt_platform::admission::loop_running(),
        "and its loop takes its first turn"
    );
}

/// [`on_the_window_thread`], then on the way out: the phase the rows admitted only in `Exiting`
/// (15, 16, 16b, 17) are admitted in.
pub(crate) fn on_the_window_thread_exiting() {
    on_the_window_thread();
    assert!(
        bt_platform::admission::exiting(),
        "and it is on the way out"
    );
}

/// **Run one test again, alone, in a process of its own, and pass only if it passed there.**
///
/// For a claim that holds once per process: `bt_platform::admission::enter_standalone_main` lends
/// one `WorkerCtx` per process, so a second test of this binary that entered would be refused. The
/// parent runs this binary again with `--exact <selector>` and
/// `BT_STANDALONE_ENTRY_TEST_CHILD=<selector>`, writes `stdin` to the child's standard input and
/// closes it, and requires the child's own summary to say that one test ran and passed. A selector
/// that names nothing makes the harness print `running 0 tests` and exit 0, which is not a pass
/// (`docs/plans/bt-app-split-prep.md` §6.3, P9); the summary line is what tells the two apart.
///
/// Answers `true` in the child, where the caller goes on to make its claim, and `false` in the
/// parent once the child has passed.
pub(crate) fn alone_in_a_process(selector: &str, stdin: &[u8]) -> bool {
    const CHILD: &str = "BT_STANDALONE_ENTRY_TEST_CHILD";
    if std::env::var_os(CHILD).is_some_and(|named| named == selector) {
        return true;
    }
    let mut child = bt_platform::quiet_command(std::env::current_exe().unwrap())
        .args(["--exact", selector, "--nocapture", "--test-threads=1"])
        .env(CHILD, selector)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the harness can run one of its own tests");
    {
        use std::io::Write as _;
        let mut input = child.stdin.take().expect("the child's standard input");
        input
            .write_all(stdin)
            .expect("the child's standard input takes its bytes");
    }
    let started = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > std::time::Duration::from_secs(60) {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("`{selector}` ran for a minute in its own process and was stopped");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    let said = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && said.contains("test result: ok. 1 passed"),
        "`{selector}` did not pass as the one test of its own process ({}):\n{said}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    false
}
