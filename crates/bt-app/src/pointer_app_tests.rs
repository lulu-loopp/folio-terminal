//! **The pointer router and the guard on where pointer data enters**, as the
//! application holds them (T-POINTER-CAPTURE,
//! `docs/plans/design/pointer-capture-2026-10-09.md` §4.1 cut 1 and §4.2).

use super::*;
use crate::runtime::pointer::{
    BANDS_THAT_TAKE_NO_POINTER, CaptureOwner, FloatFacts, POINTER_LAYERS_TOP_FIRST, Plane,
    PointerCapture, PointerFacts, PointerHit, PointerScene, PressVerdict, Visits,
    press_against_the_slot, release_ends_the_capture, walk_pointer_layers,
};
use crate::test_support::{calls_of, source};
use bt_source::{Pattern, Search, View, needle};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::BTreeMap;

// ── the layers are the paint list ─────────────────────────────────────────────

/// RED (T-POINTER-CAPTURE cut 1, R-2) — **every band the overlay paints is a
/// pointer layer, in the paint order read top first, or a band that takes no
/// pointer.**
///
/// The router walks [`POINTER_LAYERS_TOP_FIRST`]; the paint folds
/// [`OverlayStack::bands_bottom_first`]. Each layer names the bands it is
/// painted in, so the two lists are compared band for band: a band added to
/// the paint fails here until it is a layer's or is listed as taking no
/// pointer, and a reorder of the paint fails here until the router follows.
/// The in-pane surfaces' own yield list, [`OVER_IN_PANE_TOP_FIRST`], is the
/// part of the same order above them.
///
/// MUTATION: swap two bands in `bands_bottom_first` (the toast and the
/// palette, say) and the first assertion names the two lists.
#[test]
fn every_band_is_a_pointer_layer_or_takes_no_pointer() {
    let painted_top_first: Vec<OverlayBand> = OverlayStack::default()
        .bands_bottom_first()
        .into_iter()
        .rev()
        .map(|(band, _)| band)
        .collect();
    let taking_the_pointer: Vec<OverlayBand> = painted_top_first
        .iter()
        .copied()
        .filter(|band| !BANDS_THAT_TAKE_NO_POINTER.contains(band))
        .collect();
    let layers: Vec<OverlayBand> = POINTER_LAYERS_TOP_FIRST
        .iter()
        .flat_map(|layer| layer.bands().iter().copied())
        .collect();
    assert_eq!(
        layers, taking_the_pointer,
        "the router's layers, band by band, are the paint order read top first"
    );
    for band in BANDS_THAT_TAKE_NO_POINTER {
        assert!(
            painted_top_first.contains(&band) && !layers.contains(&band),
            "{band:?} is painted and is no layer's"
        );
    }
    let in_pane = taking_the_pointer
        .iter()
        .position(|band| *band == OverlayBand::InPane)
        .expect("the in-pane surfaces are painted as one band");
    let over_in_pane: Vec<OverlayBand> = OVER_IN_PANE_TOP_FIRST
        .iter()
        .map(|family| family.band())
        .collect();
    assert_eq!(
        over_in_pane,
        taking_the_pointer[..in_pane],
        "OVER_IN_PANE_TOP_FIRST is the paint order above the in-pane surfaces, top first"
    );
}

// ── the walk's cost ────────────────────────────────────────────────────────────

/// The probe the floats are stacked over, and the frame every one of them has.
const FLOAT_FRAME: [f32; 4] = [100.0, 100.0, 500.0, 400.0];

/// A floating window's tenant, one of each kind the router reads a body for.
#[derive(Clone, Copy)]
enum Tenant {
    Tree,
    GitPage,
    Graph,
    Document,
    Refused,
}

const TENANTS: [Tenant; 5] = [
    Tenant::Tree,
    Tenant::GitPage,
    Tenant::Graph,
    Tenant::Document,
    Tenant::Refused,
];

/// What a walk is handed besides its facts: the layouts the window keeps.
struct Kept {
    toasts: Vec<toast::ToastLayout>,
    palette: palette::PaletteLayout,
    programs: profiles::ProfilePrograms,
    float_git_pages: BTreeMap<float::FloatId, git_panel::GitPanelContent>,
    graphs: BTreeMap<PreviewSurface, git_graph::GraphContent>,
    notices: BTreeMap<NoticeHost, NoticeStrip>,
    web_sheets: Vec<(SeatId, websheet::SheetLayout)>,
    capsule: search::Capsule,
}

impl Kept {
    fn scene(&self) -> PointerScene<'_> {
        PointerScene {
            toasts: &self.toasts,
            palette: Some(&self.palette),
            profile_programs: &self.programs,
            recent: &[],
            float_git_pages: &self.float_git_pages,
            graphs: &self.graphs,
            float_hover: None,
            notices: &self.notices,
            web_sheets: &self.web_sheets,
            search: Some(&self.capsule),
        }
    }
}

/// A surface large enough that the centred palette and the menus stand well
/// clear of the probes.
const SURFACE: (f32, f32) = (4000.0, 3000.0);

fn measure(text: &str, font_px: f32) -> f32 {
    text.chars().count() as f32 * font_px * 0.55
}

fn a_toast(id: u64, frame: [f32; 4]) -> toast::ToastLayout {
    toast::ToastLayout {
        id: toast::ToastId(id),
        kind: toast::ToastKind::Error,
        frame,
        mark: frame,
        close: [frame[2] - 20.0, frame[1], frame[2], frame[1] + 20.0],
        title: None,
        lines: Vec::new(),
        action: None,
    }
}

fn a_capsule(frame: [f32; 4]) -> search::Capsule {
    search::Capsule {
        frame,
        field: frame,
        counter: frame,
        case: frame,
        word: frame,
        regex: frame,
        separator: frame,
        previous: frame,
        next: frame,
        close: frame,
    }
}

fn a_sheet(body: [f32; 4]) -> websheet::SheetLayout {
    websheet::SheetLayout {
        body,
        frame: body,
        mark: body,
        say: Vec::new(),
        detail: body,
        verb: body,
        close: body,
        scale: 1.0,
    }
}

/// **Every fixed layer up** (§4.1 cut 1): the glance card, three toasts, the
/// palette, a pane menu with its child list open, the profile menu, two
/// download sheets and the capsule over a notice strip — each standing clear of
/// the probes, so a walk passes through every one of them.
fn every_fixed_layer_up(kept: &mut Kept, facts: &mut PointerFacts) {
    let scale = 1.0;
    facts.scale = scale;
    facts.glance = Some([10.0, 10.0, 60.0, 60.0]);
    kept.toasts = vec![
        a_toast(1, [3600.0, 2600.0, 3900.0, 2660.0]),
        a_toast(2, [3600.0, 2680.0, 3900.0, 2740.0]),
        a_toast(3, [3600.0, 2760.0, 3900.0, 2820.0]),
    ];
    let shortcuts = crate::shortcuts::Shortcuts::defaults();
    facts.pane_menu = Some(profiles::pane_menu_layout(
        [3000.0, 1800.0],
        SURFACE,
        scale,
        Some(profiles::PaneMenuRow::SplitWith),
        false,
        &[],
        &shortcuts,
        &kept.programs,
        &mut measure,
    ));
    facts.profile_menu = Some(profiles::layout(
        [3400.0, 40.0, 3440.0, 70.0],
        profiles::MenuSide::Below,
        &kept.programs,
        SURFACE,
        scale,
        &[],
        &shortcuts,
        &mut measure,
    ));
    kept.web_sheets = vec![
        (SeatId(1), a_sheet([600.0, 100.0, 700.0, 200.0])),
        (SeatId(2), a_sheet([600.0, 1100.0, 700.0, 1200.0])),
    ];
    kept.capsule = a_capsule([1000.0, 1500.0, 1300.0, 1530.0]);
    let strip = [900.0, 1500.0, 1400.0, 1530.0];
    kept.notices.insert(
        NoticeHost::Seat(SeatId(3)),
        NoticeStrip {
            bar: notice::lay_out(
                strip,
                notice::NoticeSay {
                    text: "changed on disk",
                    verbs: &[],
                    shape: notice::NoticeShape::Band,
                },
                &[],
                scale,
            ),
            say: "changed on disk".to_owned(),
        },
    );
}

fn a_kept_world() -> Kept {
    Kept {
        toasts: Vec::new(),
        palette: palette::layout(
            SURFACE,
            1.0,
            &palette::Listing { blocks: Vec::new() },
            palette::FieldLook {
                shown: "",
                before: "",
                typed: false,
            },
            0.0,
            &mut measure,
        ),
        programs: profiles::ProfilePrograms::default(),
        float_git_pages: BTreeMap::new(),
        graphs: BTreeMap::new(),
        notices: BTreeMap::new(),
        web_sheets: Vec::new(),
        capsule: a_capsule([0.0; 4]),
    }
}

/// One floating window over [`FLOAT_FRAME`] with `tenant` in its body.
fn a_float(kept: &mut Kept, id: float::FloatId, tenant: Tenant) -> FloatFacts {
    let scale = 1.0;
    let tools = float::FloatHeadTools {
        rail: matches!(tenant, Tenant::Document),
        ..float::FloatHeadTools::default()
    };
    let geometry = float::float_geometry(FLOAT_FRAME, float::FloatMode::Pinned, scale, 40.0, tools);
    let (mut tree, mut git, mut rail, mut card_button) = (None, None, None, None);
    match tenant {
        Tenant::Tree => {
            tree = Some(seats::files_tree_geometry(geometry.body, 40, 0.0, scale));
        }
        Tenant::GitPage => {
            let page = git_panel::sample_page_for_tests();
            git = Some(git_panel::git_panel_geometry(geometry.body, &page, scale));
            kept.float_git_pages.insert(id, page);
        }
        Tenant::Graph => {
            kept.graphs.insert(
                PreviewSurface::Float(id),
                git_graph::sample_graph_for_tests(geometry.body, scale),
            );
        }
        Tenant::Document => {
            rail = Some(seats::PreviewRailGeometry {
                band: geometry.rail.expect("a document's window wears a rail"),
                ..seats::PreviewRailGeometry::default()
            });
        }
        Tenant::Refused => {
            card_button =
                seats::preview_card_geometry(geometry.body, Some(90.0), false, 0, scale).button;
        }
    }
    FloatFacts {
        id,
        geometry,
        rail,
        card_button,
        tree,
        git,
    }
}

/// `count` floating windows stacked over [`FLOAT_FRAME`], front first, the
/// front one holding `front` and the rest every tenant in turn.
fn stacked_floats(kept: &mut Kept, facts: &mut PointerFacts, count: usize, front: Tenant) {
    facts.floats.clear();
    for index in 0..count {
        let tenant = if index == 0 {
            front
        } else {
            TENANTS[index % TENANTS.len()]
        };
        let float = a_float(kept, index as float::FloatId + 1, tenant);
        facts.floats.push(float);
    }
}

/// The float stack sizes the cost is held at (§4.1 cut 1): no cap exists, so
/// the size is a parameter and the visit count a formula in it.
const STACKS: [usize; 5] = [0, 1, 8, 64, 512];

/// One walk at `at`, with the planes standing for the window and answering
/// nothing, and what it visited.
fn walk_counted(kept: &Kept, facts: &PointerFacts, at: [f64; 2]) -> (Option<PointerHit>, Visits) {
    facts.visits.set(Visits::default());
    let hit = walk_pointer_layers(
        &kept.scene(),
        facts,
        PhysicalPosition::new(at[0], at[1]),
        |_: Plane, _| None,
    );
    (hit, facts.visits.get())
}

/// The front float's head, which no control stands on.
fn on_the_front_head(facts: &PointerFacts) -> [f64; 2] {
    let head = facts.floats[0].geometry.head;
    [
        f64::from(head[0] + 30.0),
        f64::from((head[1] + head[3]) / 2.0),
    ]
}

/// RED (T-POINTER-CAPTURE cut 1, R-4, §5) — **the walk visits one frame per
/// floating window down to the first that claims the point, and the parts of
/// that one only.**
///
/// Every fixed layer is up and `F` windows of every tenant are stacked over
/// one point, for `F` in [`STACKS`]. A walk is counted by layer: one visit per
/// layer it asks, one frame test per window, one part resolution per window
/// whose parts it reads. So with the point on the front window the count is
/// exactly `L + 1 + 1` — the nine layers above the windows, the front window's
/// frame, its parts — whatever `F` is; and with the point on a download sheet
/// beside the stack it is `L' + F`, every window's frame and nothing more.
///
/// MUTATION: resolve every window's parts, not only the claimant's — at
/// `F = 8` the part count reads 8 and the exact formula fails (at `F = 0` the
/// mutation cannot show, which is why it is not the only size).
#[test]
fn the_walk_visits_one_frame_per_float_and_the_parts_of_one() {
    const ABOVE_THE_FLOATS: usize = 9;
    for count in STACKS {
        let mut kept = a_kept_world();
        let mut facts = PointerFacts::default();
        every_fixed_layer_up(&mut kept, &mut facts);
        stacked_floats(&mut kept, &mut facts, count, Tenant::Tree);
        let beside = [650.0, 150.0];
        if count > 0 {
            let (hit, visits) = walk_counted(&kept, &facts, on_the_front_head(&facts));
            assert!(
                matches!(hit, Some(PointerHit::Float(1, _))),
                "F = {count}: the front window claims the point, got {hit:?}"
            );
            assert_eq!(
                visits,
                Visits {
                    above_floats: ABOVE_THE_FLOATS,
                    float_frames: 1,
                    float_parts: 1,
                    below_floats: 0,
                },
                "F = {count}: the layers above, the front frame, the front window's parts"
            );
        }
        let (hit, visits) = walk_counted(&kept, &facts, beside);
        assert!(
            matches!(hit, Some(PointerHit::DownloadSheet(SeatId(1), _))),
            "F = {count}: the sheet beside the stack claims it, got {hit:?}"
        );
        assert_eq!(
            visits,
            Visits {
                above_floats: ABOVE_THE_FLOATS,
                float_frames: count,
                float_parts: 0,
                below_floats: 1,
            },
            "F = {count}: every window's frame once, no window's parts"
        );
    }
}

// ── the walk allocates nothing ─────────────────────────────────────────────────

thread_local! {
    /// Whether this thread's allocations are being counted.
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    /// How many this thread made while counting.
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

/// **The test build's allocator: the system's, counting what one thread asks
/// for while that thread says so** — so a test on one thread can hold a
/// stretch of code to zero allocations while the others run.
struct CountingAllocator;

fn note_an_allocation() {
    // `try_with`: an allocation made while this thread's slots are being torn
    // down is not one any test is counting.
    let _ = COUNTING.try_with(|counting| {
        if counting.get() {
            let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        }
    });
}

#[expect(
    unsafe_code,
    reason = "permanent: a global allocator is an `unsafe impl`; this one only counts and hands every call to the system's"
)]
// SAFETY: every method forwards its arguments unchanged to `System`, which
// upholds `GlobalAlloc`'s contract; counting touches only a thread-local cell.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_an_allocation();
        // SAFETY: the caller's contract for `alloc` is `System`'s.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note_an_allocation();
        // SAFETY: as `alloc`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        note_an_allocation();
        // SAFETY: as `alloc`; `pointer` came from this allocator, which is `System`.
        unsafe { System.realloc(pointer, layout, size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` came from this allocator, which is `System`.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// How many allocations `work` makes on this thread.
fn allocations_in<T>(work: impl FnOnce() -> T) -> (T, usize) {
    ALLOCATIONS.with(|count| count.set(0));
    COUNTING.with(|counting| counting.set(true));
    let answer = work();
    COUNTING.with(|counting| counting.set(false));
    (answer, ALLOCATIONS.with(Cell::get))
}

/// A point on the front window's body that its tenant answers for — a tree
/// row, a Git page's row, a graph's row, the document, the refusal's button.
fn on_the_front_body(facts: &PointerFacts, tenant: Tenant) -> [f64; 2] {
    let front = &facts.floats[0];
    let body = front.geometry.body;
    let [x, y] = match tenant {
        Tenant::Refused => {
            let button = front.card_button.expect("the refusal draws its button");
            [(button[0] + button[2]) / 2.0, (button[1] + button[3]) / 2.0]
        }
        Tenant::GitPage => {
            let row = front.git.as_ref().expect("a page is laid out").row_rect(1);
            [row[2] - 6.0, (row[1] + row[3]) / 2.0]
        }
        Tenant::Tree | Tenant::Graph | Tenant::Document => {
            [(body[0] + body[2]) / 2.0, body[1] + 40.0]
        }
    };
    [f64::from(x), f64::from(y)]
}

/// RED (T-POINTER-CAPTURE cut 1, R-4) — **one walk allocates nothing**, at
/// every stack size and over every tenant's body.
///
/// The walk reads the frame's facts and the window's kept layouts in place:
/// it iterates the floating windows where they stand, borrows a Git page's
/// rows and a graph by the window's id, and finds a row's verb without
/// building the row's list of verbs. Counted by the test build's allocator on
/// this thread only, around exactly one walk.
///
/// MUTATION: collect the floating windows' ids into a `Vec` before walking
/// them (what `float_hit_at` does, `floats.rs`), or find a Git row's verb
/// through `act_boxes` — the count reads 1.
#[test]
fn the_walk_allocates_nothing() {
    for count in STACKS {
        for front in TENANTS {
            let mut kept = a_kept_world();
            let mut facts = PointerFacts::default();
            every_fixed_layer_up(&mut kept, &mut facts);
            stacked_floats(&mut kept, &mut facts, count, front);
            let mut probes = vec![[650.0, 150.0], [2.0, 2990.0]];
            if count > 0 {
                probes.push(on_the_front_head(&facts));
                probes.push(on_the_front_body(&facts, front));
            }
            for at in probes {
                let (hit, allocations) = allocations_in(|| walk_counted(&kept, &facts, at).0);
                assert_eq!(
                    allocations, 0,
                    "F = {count}, the front window's tenant {}: the walk at {at:?} \
                     allocated (it answered {hit:?})",
                    front as usize
                );
            }
        }
    }
}

// ── one walk per event ─────────────────────────────────────────────────────────

/// RED (T-POINTER-CAPTURE cut 1, R-4) — **a complete event walks the router
/// once.**
///
/// The walk has one caller in the product, the door that opens an event
/// ([`Runtime::open_pointer_event`], which keeps the answer for the event's
/// readers), and each of the three doors a pointer event comes through — a
/// move, a button, a burst of notches being spent — opens its event exactly
/// once. Every other reader reads the answer the door kept (the readers move
/// onto it in cut 8, family by family, each extending this pin).
///
/// MUTATION: let any reader call `pointer_layer_at` itself instead of reading
/// the kept answer — the first assertion names it; open an event twice in one
/// door — the second does.
#[test]
fn a_complete_event_walks_once() {
    let owners = |found: &bt_source::Found| -> Vec<(String, usize)> {
        found
            .in_the_product(source())
            .owners(source())
            .into_iter()
            .map(|(owner, count)| {
                (
                    format!("{}::{}", owner.type_owner.unwrap_or_default(), owner.name),
                    count,
                )
            })
            .collect()
    };
    assert_eq!(
        owners(&calls_of("Runtime", "pointer_layer_at")),
        [("Runtime::open_pointer_event".to_owned(), 1)],
        "the walk is asked by the door that opens an event, and by nothing else"
    );
    let mut opened = owners(&calls_of("Runtime", "open_pointer_event"));
    opened.sort();
    assert_eq!(
        opened,
        [
            ("Runtime::flush_wheel".to_owned(), 1),
            ("Runtime::mouse_input".to_owned(), 1),
            ("Runtime::pointer_moved".to_owned(), 1),
        ],
        "each door opens its event once"
    );
    assert_eq!(
        owners(
            &source()
                .search(
                    &Search::new(
                        needle!(Pattern::call("walk_pointer_layers")),
                        View::Identifiers
                    )
                    .exempting_declarations_of(bt_source::ItemQuery::function(
                        "walk_pointer_layers"
                    ))
                )
                .unwrap_or_else(|failure| panic!("{failure}"))
        ),
        [("Runtime::pointer_layer_at".to_owned(), 1)],
        "and the walk itself is reached only through it"
    );
}

// ── the guard ──────────────────────────────────────────────────────────────────

/// The module every pointer read belongs to.
const POINTER_MODULE: &str = "crate::runtime::pointer";

/// **The six ways pointer data enters Folio** (§4.2), each a set of
/// `bt_source` needles: the pointer fields (N-P), the position type (N-E), the
/// button and wheel types (N-B), the system cursor reads (N-S), the pointer
/// kinds of a window event (N-W) and the translated-touch road (N-T).
fn the_six_doors() -> Vec<(&'static str, Pattern)> {
    vec![
        ("N-P", Pattern::identifier("pointer_position")),
        ("N-P", Pattern::identifier("pointer_last_seen")),
        ("N-E", Pattern::identifier("PhysicalPosition")),
        ("N-B", Pattern::identifier("MouseButton")),
        ("N-B", Pattern::identifier("ElementState")),
        ("N-B", Pattern::identifier("MouseScrollDelta")),
        ("N-S", Pattern::path("bt_platform::pointer_position")),
        (
            "N-S",
            Pattern::path("bt_platform::pointer_position_in_window"),
        ),
        ("N-W", Pattern::path("WindowEvent::CursorMoved")),
        ("N-W", Pattern::path("WindowEvent::CursorLeft")),
        ("N-W", Pattern::path("WindowEvent::CursorEntered")),
        ("N-W", Pattern::path("WindowEvent::MouseInput")),
        ("N-W", Pattern::path("WindowEvent::MouseWheel")),
        ("N-T", Pattern::identifier("PanStep")),
        (
            "N-T",
            Pattern::path("bt_platform::let_the_system_translate_touch"),
        ),
    ]
}

/// **Every occurrence of a door's needle outside [`POINTER_MODULE`]**, one row
/// each: `owner<TAB>door`, the owner the item the occurrence stands in — or,
/// for an occurrence in no item (an import, an alias), the module it is
/// written in — sorted. No file and no line, so moving code that reads the
/// same things moves no row.
fn pointer_reads_outside_the_module() -> Vec<String> {
    let index = source();
    let mut rows = Vec::new();
    for (door, pattern) in the_six_doors() {
        let found = index
            .search(&Search::new(needle!(pattern), View::Identifiers))
            .unwrap_or_else(|failure| panic!("{failure}"))
            .in_the_product(index);
        for (owner, count) in found.owners(index) {
            if owner.module_path.starts_with(POINTER_MODULE) {
                continue;
            }
            rows.extend(std::iter::repeat_n(format!("{owner}\t{door}"), count));
        }
        for occurrence in found.occurrences() {
            if index
                .items()
                .iter()
                .any(|item| occurrence.span.within(item.whole()))
            {
                continue;
            }
            let file = index
                .file_at(occurrence.span.start())
                .expect("an occurrence stands in a file");
            for owner in file.owners() {
                if !owner.module_path.starts_with(POINTER_MODULE) {
                    rows.push(format!("{} (outside any item)\t{door}", owner.module_path));
                }
            }
        }
    }
    rows.sort();
    rows
}

const DEBT_HEADER: &str = "\
# Every read of pointer data in bt-app outside `crate::runtime::pointer`, one row per occurrence
# (T-POINTER-CAPTURE, docs/plans/design/pointer-capture-2026-10-09.md section 4.2).
#
# Generated by scripts/dev/generate-pointer-debt.ps1 from the query of
# `every_pointer_read_is_the_routers_or_a_captures`; do not edit by hand. A reader moved behind the
# router leaves this file in the same commit; nothing adds a row (scripts/ci/check-pointer-debt.ps1).
owner\tdoor
";

fn rendered_debt() -> String {
    let mut text = DEBT_HEADER.to_owned();
    for row in pointer_reads_outside_the_module() {
        text.push_str(&row);
        text.push('\n');
    }
    text
}

fn repository_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// RED (T-POINTER-CAPTURE cut 1, §4.2) — **every read of pointer data is the
/// router's or a capture's**: an occurrence of any of the six doors' needles in
/// any `bt-app` item outside `crate::runtime::pointer` is a row of
/// `docs/plans/POINTER-DEBT.tsv`, and the file is exactly those rows.
///
/// There is no list of allowed names: a reader is written inside the module,
/// where a reviewer sees it as one, or it is debt. The rendering is left in
/// `target/pointer-debt.tsv` for `scripts/dev/generate-pointer-debt.ps1`; the
/// file only shrinks (`scripts/ci/check-pointer-debt.ps1`).
///
/// MUTATION: read `pointer_position` in `Runtime::scroll_tab_strip` once its
/// row has left the file — the comparison names the new row.
#[test]
fn every_pointer_read_is_the_routers_or_a_captures() {
    let root = repository_root();
    let wanted = rendered_debt();
    let generated = root.join("target").join("pointer-debt.tsv");
    if let Some(parent) = generated.parent() {
        std::fs::create_dir_all(parent).expect("the workspace has a target directory");
    }
    std::fs::write(&generated, wanted.as_bytes()).expect("target/ is writable");
    let held = std::fs::read_to_string(root.join("docs").join("plans").join("POINTER-DEBT.tsv"))
        .expect("docs/plans/POINTER-DEBT.tsv is checked in")
        .replace("\r\n", "\n");
    // Row by row with multiplicity: one more occurrence of a read an owner
    // already has is one more row.
    let mut balance: BTreeMap<&str, i64> = BTreeMap::new();
    for row in wanted.lines() {
        *balance.entry(row).or_default() += 1;
    }
    for row in held.lines() {
        *balance.entry(row).or_default() -= 1;
    }
    let added: Vec<(&str, i64)> = balance
        .iter()
        .filter(|(_, count)| **count > 0)
        .map(|(row, count)| (*row, *count))
        .collect();
    let gone: Vec<(&str, i64)> = balance
        .iter()
        .filter(|(_, count)| **count < 0)
        .map(|(row, count)| (*row, -*count))
        .collect();
    assert!(
        added.is_empty() && gone.is_empty(),
        "docs/plans/POINTER-DEBT.tsv is not what the query finds — run \
         scripts/dev/generate-pointer-debt.ps1 if a reader left, and move the reader into \
         crate::runtime::pointer if one arrived\n  new reads: {added:#?}\n  rows with no read: {gone:#?}"
    );
    assert_eq!(
        held, wanted,
        "the file is the rendering, row for row and count for count"
    );
}

// ── the capture slot (cut 3) ───────────────────────────────────────────────────

/// A capture of `owner` latched in `window` by `button`.
fn a_capture(
    window: winit::window::WindowId,
    owner: CaptureOwner,
    button: MouseButton,
) -> PointerCapture {
    PointerCapture {
        window,
        owner,
        button,
        started: None,
    }
}

/// The layers that took a release before it reached the gesture that owned it
/// (§3.1 column a), each by the door its press arm calls in `mouse_input`.
const LAYERS_THAT_ATE_RELEASES: [(&str, &str); 7] = [
    ("a full-window card", "self.quit_card_layout()"),
    ("the toasts", "self.press_toast("),
    ("the settings sheet", "self.settings_mouse_input("),
    ("the glance card's head", "self.press_file_peek(button)"),
    ("a floating window", "self.press_float(position)"),
    (
        "a hosted page",
        "self.press_web_page(state,button,position)",
    ),
    ("the chrome's own router", "self.chrome_mouse_input("),
];

/// RED (T-POINTER-CAPTURE cut 3, R-6) — **a release reaches its owner over
/// every layer that eats releases.**
///
/// The release of the capture's button is decided by the capture alone — its
/// owner and its button, never the window and never what is under the pointer
/// — and it is asked before every layer of the press road: a card, the toasts,
/// the settings sheet, a menu, the glance card, a floating window, a hosted page
/// and the chrome's router all stand below `release_capture` in `mouse_input`.
/// The gestures that always ended on any button's release still do until cut 4
/// rules on the other button; every other one ends on its own.
///
/// MUTATION: put the page arm back above the release (move
/// `self.press_web_page(state, button, position)` ahead of
/// `self.release_capture(`) — the order assertion names the page.
#[test]
fn a_release_reaches_its_owner_over_every_layer_that_eats_releases() {
    let (a, b) = (
        winit::window::WindowId::from(1_u64),
        winit::window::WindowId::from(2_u64),
    );
    let tab = TabId(3);
    let surface = PreviewSurface::Peek;
    for (owner, any) in [
        (CaptureOwner::EditSelection(tab, surface), false),
        (CaptureOwner::VideoBar(surface), false),
        (CaptureOwner::Route(MouseRoute::MathBlock), true),
        (CaptureOwner::GlanceThumb(4.0), true),
        (CaptureOwner::SettingsMenuBar(12.0), true),
    ] {
        for window in [a, b] {
            let capture = a_capture(window, owner.clone(), MouseButton::Left);
            assert!(
                release_ends_the_capture(&capture, MouseButton::Left),
                "the capture's own button ends it, in any window"
            );
            assert_eq!(
                release_ends_the_capture(&capture, MouseButton::Right),
                any,
                "another button's release ends it exactly where it always did"
            );
        }
    }
    let road = crate::test_support::squeezed_body("Runtime", "mouse_input");
    let release = road
        .find("self.release_capture(")
        .expect("the release asks the capture first");
    for (layer, door) in LAYERS_THAT_ATE_RELEASES {
        let at = road
            .find(door)
            .unwrap_or_else(|| panic!("`{door}` ({layer}) is on the press road"));
        assert!(
            release < at,
            "{layer} is asked after the capture's release, so it cannot eat it"
        );
    }
}

/// RED (T-POINTER-CAPTURE cut 3, R-5, cell B20) — **a press of the held
/// button cancels the stale capture first**, in the capturing window and in
/// another window of the same application.
///
/// A button cannot go down twice without coming up, so a press of the held
/// capture's button proves its release was lost. The verdict compares buttons
/// and never windows; a press of another button keeps today's routing until cut
/// 4. The door asks it before any arm of the press road, and a stale capture of
/// another window is handed to that window, which runs its owner's cancel.
///
/// MUTATION: answer `Route` for the held button (route without cancelling) —
/// the verdicts fail; drop `cancel_stale_capture` from the door — the last
/// assertion fails.
#[test]
fn a_press_of_the_held_button_cancels_the_stale_capture_first() {
    let (a, b) = (
        winit::window::WindowId::from(1_u64),
        winit::window::WindowId::from(2_u64),
    );
    let held = a_capture(a, CaptureOwner::GlanceThumb(2.0), MouseButton::Left);
    assert_eq!(
        press_against_the_slot(None, MouseButton::Left),
        PressVerdict::Route
    );
    for pressed_in in [a, b] {
        assert_eq!(
            press_against_the_slot(Some(&held), MouseButton::Left),
            PressVerdict::CancelThenRoute,
            "a press of the held button in window {pressed_in:?} finds the capture stale"
        );
        assert_eq!(
            press_against_the_slot(Some(&held), MouseButton::Right),
            PressVerdict::Route,
            "another button is routed as it always was (cut 4 rules on it)"
        );
    }
    let road = crate::test_support::squeezed_body("Runtime", "mouse_input");
    let precedence = road
        .find("self.press_against_the_capture(button)?;")
        .expect("the press asks the slot");
    for (layer, door) in LAYERS_THAT_ATE_RELEASES {
        let at = road.find(door).expect("the arm is on the press road");
        assert!(precedence < at, "the slot is asked before {layer}");
    }
    assert!(
        crate::test_support::squeezed_body("Runtime", "press_against_the_capture")
            .contains("PressVerdict::CancelThenRoute=>self.cancel_stale_capture(),"),
        "a stale capture is cancelled before the press goes on"
    );
}

/// RED (T-POINTER-CAPTURE cut 3, cell C1·0) — **a divider let go anywhere
/// commits, through the whole dispatch.**
///
/// The divider tests of `app_panes_tests` drive the release into
/// `chrome_mouse_input` directly; this pins the road a real release takes:
/// `mouse_input` asks the capture first, the capture names the divider, and
/// the divider's release puts the cursor back and marks the session dirty —
/// the ratio it was moving is the ratio chosen — before any layer is asked.
///
/// MUTATION: make `release_capture` answer the divider by handing it on
/// (`Ok(false)`) — the second assertion fails.
#[test]
fn a_divider_released_anywhere_commits_through_the_whole_dispatch() {
    let road = crate::test_support::squeezed_body("Runtime", "mouse_input");
    assert!(
        road.find("self.release_capture(") < road.find("self.chrome_mouse_input("),
        "the capture is asked before the chrome's router"
    );
    let release = crate::test_support::squeezed_body("Runtime", "release_capture");
    assert!(
        release.contains("|CaptureOwner::TerminalFootMark(..)=>matchposition{Some(position)=>self.release_chrome_capture(button,position),"),
        "the divider's release goes to the chrome capture's own let-go"
    );
    let let_go = crate::test_support::squeezed_body("Runtime", "release_chrome_capture");
    let divider = let_go
        .find("ifself.take_divider_drag().is_some(){")
        .expect("the divider is let go");
    assert!(
        let_go[divider..].contains("self.mark_session_dirty(Instant::now());")
            && let_go[divider..].contains("returnOk(true);"),
        "and the ratio it was moving is committed and the release consumed"
    );
    assert_eq!(
        crate::test_support::squeezed_body("Runtime", "a_gesture_holds_the_pointer"),
        "{self.a_capture_holds_the_pointer()}",
        "every capture holds the pointer — the one slot, not a list of latches (supersedes \
         every_carry_this_window_can_hold_is_named_by_the_one_predicate)"
    );
}
