//! **The pointer router and the guard on where pointer data enters**, as the
//! application holds them (T-POINTER-CAPTURE,
//! `docs/plans/design/pointer-capture-2026-10-09.md` §4.1 cut 1 and §4.2).

use super::*;
use crate::runtime::pointer::{
    BANDS_THAT_TAKE_NO_POINTER, ButtonSet, CaptureOwner, FloatFacts, POINTER_LAYERS_TOP_FIRST,
    Plane, PointerCapture, PointerFacts, PointerHit, PointerScene, PressVerdict, ReleaseVerdict,
    Visits, cancel_then_route, press_against_the_slot, release_against_the_slot,
    walk_pointer_layers,
};
use crate::test_support::{calls_of, source};
use bt_source::{Pattern, Search, View, needle};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

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

/// RED (T-POINTER-CAPTURE cut 1, R-4) — **the overlay-layer part of one walk
/// allocates nothing**, at every stack size and over every tenant's body.
///
/// The walk reads the frame's facts and the window's kept layouts in place:
/// it iterates the floating windows where they stand, borrows a Git page's
/// rows and a graph by the window's id, and finds a row's verb without
/// building the row's list of verbs. Counted by the test build's allocator on
/// this thread only, around exactly one walk. `walk_counted` answers `None`
/// for planes 13–16, whose allocation budget remains open until cut 9.
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
/// readers), and each door that handles pointer geometry — a move, a button,
/// a burst of notches being spent, or an application-routed release in the
/// owner window — opens its event exactly once. Every other reader reads the
/// answer the door kept (the readers move onto it in cut 8, family by family,
/// each extending this pin).
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
            ("Runtime::release_capture_from_application".to_owned(), 1),
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

/// The window seam, where the window's cursor door is declared and implemented: exempt from the
/// system-cursor door (N-S) alone, since a door's declaration and its backends' bodies are not reads.
const SEAM_MODULE: &str = "crate::window_seam";

fn is_pointer_module(module: &str) -> bool {
    module == POINTER_MODULE
        || module
            .strip_prefix(POINTER_MODULE)
            .is_some_and(|suffix| suffix.starts_with("::"))
}

/// RED (T-POINTER-CAPTURE cut 1 guard) — the exemption is one module and its
/// descendants, not every module whose name begins with the same bytes.
///
/// MUTATION: use `module.starts_with(POINTER_MODULE)`; the
/// `crate::runtime::pointer_escape` row becomes exempt and this test fails.
#[test]
fn the_pointer_guard_exemption_ends_at_the_module_boundary() {
    assert!(is_pointer_module("crate::runtime::pointer"));
    assert!(is_pointer_module("crate::runtime::pointer::capture"));
    assert!(!is_pointer_module("crate::runtime::pointer_escape"));
}

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
        // The window's own door onto the system cursor (`WindowHost::pointer_in_window`),
        // declared and implemented in the seam, which is exempt for it: every call is a read.
        ("N-S", Pattern::identifier("pointer_in_window")),
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
/// each: `owner<TAB>door<TAB>module`, the owner the item the occurrence stands
/// in — or, for an occurrence in no item (an import, an alias), the module it
/// is written in — and the module the owner is written in, sorted. No file and
/// no line; the module is its own column so that `check-pointer-debt.ps1` reads
/// a row moved to another module as that row moved, not as one added
/// (T-MOVED-NOT-ADDED-RULE).
fn pointer_reads_outside_the_module() -> Vec<String> {
    let index = source();
    let mut rows = Vec::new();
    for (door, pattern) in the_six_doors() {
        let found = index
            .search(&Search::new(needle!(pattern), View::Identifiers))
            .unwrap_or_else(|failure| panic!("{failure}"))
            .in_the_product(index);
        for (owner, count) in found.owners(index) {
            if is_pointer_module(&owner.module_path)
                || (door == "N-S" && owner.module_path == SEAM_MODULE)
            {
                continue;
            }
            rows.extend(std::iter::repeat_n(
                format!("{owner}\t{door}\t{}", owner.module_path),
                count,
            ));
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
                if !is_pointer_module(&owner.module_path) {
                    rows.push(format!(
                        "{0} (outside any item)\t{door}\t{0}",
                        owner.module_path
                    ));
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
# `module` is the module the owner is written in: a row moved to another module is that row moved.
owner\tdoor\tmodule
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
        os: None,
        last: None,
    }
}

/// The layers that took a release before it reached the gesture that owned it
/// (§3.1 column a), each by the door its press arm calls in `mouse_input`.
const LAYERS_THAT_ATE_RELEASES: [(&str, &str); 23] = [
    ("the quit card", "self.quit_card_layout()"),
    ("the dirty gate", "self.dirty_gate_layout()"),
    ("the first-run card", "self.first_run_layout()"),
    ("the paste card", "self.paste_card_layout()"),
    ("the update card", "self.update_card_layout()"),
    ("the uninstall card", "self.uninstall_card_layout()"),
    ("the toasts", "self.press_toast("),
    ("the settings sheet", "self.settings_mouse_input("),
    ("the restore card", "self.restore_layout()"),
    ("the Git menu", "self.git_menu_layout()"),
    ("the terminal menu", "self.term_menu_layout()"),
    ("the file menu", "self.file_menu_layout()"),
    ("the pane menu", "self.pane_menu_layout()"),
    ("the tab menu", "self.tab_menu_layout()"),
    ("the palette", "self.window.palette_layout.clone()"),
    ("the graph-filter menu", "self.graph_filter_menu_layout()"),
    ("the glance card's head", "self.press_file_peek(button)"),
    ("a floating window", "self.press_float(position)"),
    ("the profile menu", "self.profile_menu_layout()"),
    ("the root menu", "self.root_menu_layout()"),
    ("the preview menu", "self.preview_menu_layout()"),
    (
        "a hosted page",
        "self.press_web_page(state,button,position)",
    ),
    ("the chrome's own router", "self.chrome_mouse_input("),
];

/// Every payload arm in the capture record, which the release dispatch names
/// one by one.
const CAPTURE_OWNER_ARMS: [&str; 22] = [
    "Divider",
    "TabPress",
    "PanePress",
    "RowPress",
    "Drag",
    "FloatHeadPress",
    "FloatDrag",
    "GlanceHeadPress",
    "GlanceThumb",
    "VideoBar",
    "PreviewBodyThumb",
    "BlockThumb",
    "PicturePan",
    "EditSelection",
    "RenderedSelection",
    "TerminalThumb",
    "TerminalFootMark",
    "Route",
    "SettingsSlider",
    "SettingsMenuBar",
    "VideoPicture",
    "RowMiddlePress",
];

/// Owners of every ending policy the slot has had: a tab's surface, a
/// window's surface, the cell route, and the four that ended on any button's
/// release until cut 4 (the formula block, the glance card's thumb, a floating
/// window and the settings sheet's bar).
fn representative_owners() -> [CaptureOwner; 6] {
    let surface = PreviewSurface::Peek;
    [
        CaptureOwner::EditSelection(TabId(3), surface),
        CaptureOwner::VideoBar(surface),
        CaptureOwner::Route(MouseRoute::MathBlock),
        CaptureOwner::GlanceThumb(4.0),
        CaptureOwner::FloatHeadPress(FloatHeadPress::armed(
            1,
            PhysicalPosition::new(10.0, 10.0),
            [0.0, 0.0, 100.0, 100.0],
        )),
        CaptureOwner::SettingsMenuBar(12.0),
    ]
}

/// The buttons a hand can press, the platform-numbered ones past the named
/// five included.
const BUTTONS: [MouseButton; 7] = [
    MouseButton::Left,
    MouseButton::Right,
    MouseButton::Middle,
    MouseButton::Back,
    MouseButton::Forward,
    MouseButton::Other(5),
    MouseButton::Other(63),
];

/// RED (T-POINTER-CAPTURE cut 3, R-6) — **a release reaches its owner over
/// every layer that eats releases.**
///
/// The release of the capture's button is decided by the capture alone — its
/// button, never the window, never the owner's kind and never what is under the
/// pointer — and the slot is asked before every layer of the press road: a
/// card, the toasts, the settings sheet, a menu, the glance card, a floating
/// window, a hosted page and the chrome's router all stand below
/// `button_meets_the_slot` in `mouse_input`. Since cut 4 no gesture ends on
/// another button's release.
///
/// MUTATION: put the page arm back above the slot (move
/// `self.press_web_page(state, button, position)` ahead of
/// `self.button_meets_the_slot(`) — the order assertion names the page.
#[test]
fn a_release_reaches_its_owner_over_every_layer_that_eats_releases() {
    let (a, b) = (
        winit::window::WindowId::from(1_u64),
        winit::window::WindowId::from(2_u64),
    );
    for owner in representative_owners() {
        for window in [a, b] {
            let capture = a_capture(window, owner.clone(), MouseButton::Left);
            assert_eq!(
                release_against_the_slot(Some(&capture), ButtonSet::default(), MouseButton::Left),
                ReleaseVerdict::ToOwner,
                "the capture's own button ends it, in any window"
            );
        }
    }
    let dispatch = crate::test_support::squeezed_body("Runtime", "release_capture");
    for owner in CAPTURE_OWNER_ARMS {
        assert!(
            dispatch.contains(&format!("CaptureOwner::{owner}")),
            "the release dispatch names {owner}"
        );
    }
    let road = crate::test_support::squeezed_body("Runtime", "mouse_input");
    let release = road
        .find("self.button_meets_the_slot(")
        .expect("the release asks the slot first");
    for (layer, door) in LAYERS_THAT_ATE_RELEASES {
        let at = road
            .find(door)
            .unwrap_or_else(|| panic!("`{door}` ({layer}) is on the press road"));
        assert!(
            release < at,
            "{layer} is asked after the slot, so it cannot eat the release"
        );
    }
}

/// RED (T-POINTER-CAPTURE cut 3, R-5, cell B20) — **a press of the held
/// button cancels the stale capture first**, in the capturing window and in
/// another window of the same application.
///
/// A button cannot go down twice without coming up, so a press of the held
/// capture's button proves its release was lost. The verdict compares buttons
/// and never windows (it is not handed one). The door asks it before any arm
/// of the press road, and a stale capture of another window is handed to that
/// window, which runs its owner's cancel before the press is routed.
///
/// MUTATION: answer `Route` for the held button, or route the continuation
/// before `cancel_owner`; the state assertions fail.
#[test]
fn a_press_of_the_held_button_cancels_the_stale_capture_first() {
    let a = winit::window::WindowId::from(1_u64);
    for button in BUTTONS {
        assert_eq!(
            press_against_the_slot(None, button),
            PressVerdict::Route,
            "{button:?} with no gesture held is routed"
        );
        let held = a_capture(a, CaptureOwner::GlanceThumb(2.0), button);
        assert_eq!(
            press_against_the_slot(Some(&held), button),
            PressVerdict::CancelThenRoute,
            "a press of the held {button:?} finds the capture stale"
        );
    }
    #[derive(Default)]
    struct State {
        owner_active: bool,
        live_payloads: usize,
        routed: bool,
    }
    let mut state = State {
        owner_active: true,
        live_payloads: 1,
        routed: false,
    };
    cancel_then_route(
        &mut state,
        |state| {
            assert!(state.owner_active, "the owner-specific cancel runs once");
            state.owner_active = false;
            state.live_payloads -= 1;
            Ok::<(), ()>(())
        },
        |state| {
            assert!(
                !state.owner_active,
                "the old owner is cancelled before routing"
            );
            assert_eq!(
                state.live_payloads, 0,
                "routing starts with no live payload"
            );
            state.routed = true;
            state.live_payloads += 1;
            Ok::<(), ()>(())
        },
    )
    .expect("the model has no fallible step");
    assert!(state.routed);
    assert_eq!(
        state.live_payloads, 1,
        "two gesture payloads are never live"
    );
}

/// RED (T-POINTER-CAPTURE cut 4, R-8, cells A·b and B21) — **a second button
/// under any capture reaches nothing**: its press is swallowed and so is its
/// release, whatever the owner, whichever button holds the pointer and
/// whichever window either arrives in (the verdicts are not handed a window).
///
/// MUTATION: answer `Route` for another button in `press_against_the_slot`
/// (let the press through) — the first assertion fails; answer `Route` for
/// another button's release in `release_against_the_slot` — the second does.
#[test]
fn a_second_button_under_any_capture_reaches_nothing() {
    let a = winit::window::WindowId::from(1_u64);
    for owner in representative_owners() {
        for held in BUTTONS {
            let capture = a_capture(a, owner.clone(), held);
            for other in BUTTONS.into_iter().filter(|other| *other != held) {
                assert_eq!(
                    press_against_the_slot(Some(&capture), other),
                    PressVerdict::Swallow,
                    "{other:?} pressed while {held:?} holds the pointer goes nowhere"
                );
                assert_eq!(
                    release_against_the_slot(Some(&capture), ButtonSet::default(), other),
                    ReleaseVerdict::Swallow,
                    "and {other:?} let go while {held:?} holds it goes nowhere either"
                );
            }
        }
    }
}

/// RED (T-POINTER-CAPTURE cut 4, R-5 rule 3) — **with no gesture held, a
/// button is routed as it always was**, and a gesture's own release still ends
/// it: the swallow is only ever a held gesture's or a swallowed press's.
///
/// MUTATION: answer `Swallow` for an empty slot in either verdict — the first
/// assertions fail; answer `Swallow` for the held button's release — the last
/// one does.
#[test]
fn with_no_gesture_held_a_button_is_routed_as_before() {
    let a = winit::window::WindowId::from(1_u64);
    for button in BUTTONS {
        assert_eq!(press_against_the_slot(None, button), PressVerdict::Route);
        assert_eq!(
            release_against_the_slot(None, ButtonSet::default(), button),
            ReleaseVerdict::Route,
            "{button:?} let go with nothing held and nothing swallowed is routed"
        );
        let capture = a_capture(a, CaptureOwner::VideoBar(PreviewSurface::Peek), button);
        assert_eq!(
            release_against_the_slot(Some(&capture), ButtonSet::default(), button),
            ReleaseVerdict::ToOwner,
            "the held {button:?}'s own release still ends its gesture"
        );
    }
}

/// RED (T-POINTER-CAPTURE cut 4, R-5 rule 3, cell B22) — **a swallowed
/// button stays swallowed until its own release**, in any order of the
/// buttons coming up, and a swallowed press whose release was lost is proved
/// stale by the next press of that button, which is routed.
///
/// The application's own answers, in sequence on its one slot: left held;
/// right and middle pressed (swallowed); left let go (the gesture ends, the
/// slot empties); middle and right let go — both swallowed, with nothing held;
/// then right pressed again — routed. And: right swallowed, its release lost,
/// the gesture ended; the next right press is routed and so is its release.
///
/// MUTATION: clear `swallowed` in `App::take_capture_where` (when a capture
/// ends) — the right release reaches the ordinary road and the assertion
/// naming B22 fails; do not clear a stale bit in `App::answer_press` — the
/// last release is swallowed.
#[test]
fn a_swallowed_button_stays_swallowed_until_its_own_release() {
    let mut folio = crate::headless::HeadlessFolio::with_tabs(
        "pointer-swallowed-order",
        vec![crate::test_support::tab_holding(
            crate::test_support::leaf_saying("swallowed"),
        )],
        0,
    );
    let mut runtime = folio.runtime();
    let latch = |runtime: &mut Runtime<'_, crate::headless::Headless>| {
        runtime.latch_capture(
            CaptureOwner::VideoBar(PreviewSurface::Peek),
            MouseButton::Left,
        );
    };
    latch(&mut runtime);
    assert_eq!(
        runtime.app.answer_press(MouseButton::Right),
        PressVerdict::Swallow
    );
    assert_eq!(
        runtime.app.answer_press(MouseButton::Middle),
        PressVerdict::Swallow,
        "a third button under the same gesture is swallowed too"
    );
    assert_eq!(
        runtime.app.answer_release(MouseButton::Left),
        ReleaseVerdict::ToOwner,
        "the gesture's own release still goes to its owner"
    );
    runtime.drop_own_capture();
    assert_eq!(
        runtime.app.answer_release(MouseButton::Middle),
        ReleaseVerdict::Swallow,
        "B22: a swallowed press's release is swallowed after the gesture has ended"
    );
    assert_eq!(
        runtime.app.answer_release(MouseButton::Right),
        ReleaseVerdict::Swallow,
        "in whatever order the swallowed buttons come up"
    );
    assert_eq!(
        runtime.app.answer_press(MouseButton::Right),
        PressVerdict::Route,
        "its own release spent the memory: the next press is routed"
    );
    assert_eq!(
        runtime.app.answer_release(MouseButton::Right),
        ReleaseVerdict::Route
    );

    latch(&mut runtime);
    assert_eq!(
        runtime.app.answer_press(MouseButton::Right),
        PressVerdict::Swallow
    );
    runtime.drop_own_capture();
    assert_eq!(
        runtime.app.answer_press(MouseButton::Right),
        PressVerdict::Route,
        "a second press of a swallowed button proves its release was lost"
    );
    assert_eq!(
        runtime.app.answer_release(MouseButton::Right),
        ReleaseVerdict::Route,
        "and that press's release is routed like the press"
    );
}

/// RED (T-POINTER-CAPTURE cut 4, R-4) — **the swallow arm allocates
/// nothing**: a second button's press and release while a gesture is held, and
/// a swallowed release after it has ended, answered by the window's door,
/// counted by the test build's allocator on this thread.
///
/// MUTATION: build anything on the heap in the swallow arm of
/// `App::answer_press` (`std::hint::black_box(vec![button])`) — the count
/// reads 1.
#[test]
fn the_swallow_arm_allocates_nothing() {
    let mut folio = crate::headless::HeadlessFolio::with_tabs(
        "pointer-swallow-allocations",
        vec![crate::test_support::tab_holding(
            crate::test_support::leaf_saying("swallow"),
        )],
        0,
    );
    let mut runtime = folio.runtime();
    runtime.latch_capture(
        CaptureOwner::VideoBar(PreviewSurface::Peek),
        MouseButton::Left,
    );
    for button in [MouseButton::Right, MouseButton::Other(9)] {
        let (spent, allocations) = allocations_in(|| {
            [
                runtime.button_meets_the_slot(ElementState::Pressed, button, None),
                runtime.button_meets_the_slot(ElementState::Released, button, None),
            ]
        });
        assert_eq!(allocations, 0, "{button:?} under a held gesture allocated");
        assert!(
            spent
                .into_iter()
                .all(|spent| spent.is_ok_and(|spent| spent)),
            "both were spent at the slot"
        );
    }
    let _ = runtime
        .button_meets_the_slot(ElementState::Pressed, MouseButton::Middle, None)
        .expect("the press is swallowed");
    runtime.drop_own_capture();
    let (spent, allocations) = allocations_in(|| {
        runtime.button_meets_the_slot(ElementState::Released, MouseButton::Middle, None)
    });
    assert_eq!(allocations, 0, "the late release allocated");
    assert!(spent.is_ok_and(|spent| spent), "and it was swallowed");
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
        road.find("self.button_meets_the_slot(") < road.find("self.chrome_mouse_input("),
        "the slot is asked before the chrome's router"
    );
    let release = crate::test_support::squeezed_body("Runtime", "release_capture");
    assert!(
        release.contains("CaptureOwner::Divider(_)=>matchposition{Some(position)=>self.release_chrome_capture(button,position),None=>Ok(false),}"),
        "the divider's own arm routes to the chrome capture's let-go"
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
        "{self.app.pointer_capture.is_some()}",
        "every capture holds the pointer — the one slot, the application's since cut 6, not a list of latches (supersedes \
         every_carry_this_window_can_hold_is_named_by_the_one_predicate)"
    );
}

// ── the endings (cut 5, R-9) ──────────────────────────────────────────────────

/// **The lifecycle doors** (§4.1 cut 5's matrix): each door, and the call through which it ends
/// the capture before it changes what the owner names. The resize door's sample is its own item
/// (the divider's, generalised), so its row names that call; every other door calls
/// `end_capture` itself.
const LIFECYCLE_DOORS: [(&str, &str); 15] = [
    ("window_blurred", "end_capture"),
    (
        "flush_pending_pty_resize",
        "end_a_capture_that_lost_its_pointer",
    ),
    ("scale_factor_changed", "end_capture"),
    ("hide_quake_window", "end_capture"),
    ("close_window", "end_capture"),
    ("activate_tab", "end_capture"),
    ("close_tab", "end_capture"),
    ("close_pane", "end_capture"),
    ("toggle_pane_zoom", "end_capture"),
    ("sweep_preview_panes", "end_capture"),
    ("leave_preview_buffer_in", "end_capture"),
    ("rebuild_preview_document", "end_capture"),
    ("dismiss_float", "end_capture"),
    ("hide_file_peek", "end_capture"),
    ("settings_closed", "end_capture"),
];

/// **The matrix's rows that are not items yet**, each with when it lands — never silent
/// (2026-10-21, round 2): the OS drag-out door lands with its feature (§7: no drag-out exists),
/// and the page seat's two doors end a page's own capture, which is cut 7's. The drag-out door is
/// asserted absent, so the day it is written this list fails until it moves into
/// [`LIFECYCLE_DOORS`].
const DOORS_NOT_YET_ITEMS: [(&str, &str, &str); 3] = [
    (
        "Runtime",
        "drag_out_to_the_system",
        "2026-10-21: lands with the OS drag-out feature (design §7), with `OsDragHandoff`",
    ),
    (
        "WebSeat",
        "place",
        "2026-10-21: cut 7 — the seat's own button-up before a page leaves the glass",
    ),
    (
        "WebSeat",
        "park_for_handoff",
        "2026-10-21: cut 7 — the seat's own button-up before a page is handed over",
    ),
];

/// The callers of `end_capture` that are not doors of the matrix, each with the road it is: the
/// resize door's sample, a press of the held button proving the capture stale (R-5 rule 2), and
/// a preview surface turned to another document — the buffer-left door's twin.
const ENDINGS_REACHED_ANOTHER_WAY: [&str; 3] = [
    "Runtime::end_a_capture_that_lost_its_pointer",
    "Runtime::cancel_own_capture",
    "Runtime::land_preview_source_on",
];

/// The product callers of a method, as `Owner::name`, with how many calls each makes.
fn product_callers(owner: &str, name: &str) -> BTreeMap<String, usize> {
    calls_of(owner, name)
        .in_the_product(source())
        .owners(source())
        .into_iter()
        .map(|(caller, count)| {
            (
                format!("{}::{}", caller.type_owner.unwrap_or_default(), caller.name),
                count,
            )
        })
        .collect()
}

/// **Every product item whose body calls `type_owner::name`**, resolved by bt-source's call
/// graph rather than by the name (a `close` is not the sheet's `close`): the items that spell
/// the name are read first, and each is asked what its calls of that name reach. Each caller is
/// `module::Owner::name`.
fn resolved_callers_of(type_owner: &str, name: &str) -> BTreeSet<String> {
    use bt_source::call_graph::{Arms, Resolver, Target};
    let index = source();
    let resolver = Resolver::new(&[("bt-app", index)], Arms::product())
        .unwrap_or_else(|refusals| panic!("the call graph refuses bt-app: {refusals:?}"));
    let callee = index
        .one(&bt_source::ItemQuery::method(type_owner, name))
        .unwrap_or_else(|failure| panic!("{failure}"));
    let callee = resolver
        .id_of("bt-app", callee)
        .expect("an indexed item is an item of the call graph");
    let spelled = index
        .search(&Search::new(
            needle!(Pattern::call(name)),
            View::Identifiers,
        ))
        .unwrap_or_else(|failure| panic!("{failure}"))
        .in_the_product(index);
    let mut callers = BTreeSet::new();
    for caller in spelled.owners(index).into_keys() {
        let mut query = match &caller.type_owner {
            Some(owner) => bt_source::ItemQuery::method(owner, &caller.name),
            None => bt_source::ItemQuery::function(&caller.name),
        }
        .in_module(&caller.module_path);
        if let Some(trait_name) = &caller.trait_name {
            query = query.of_trait(trait_name);
        }
        for record in index
            .find(&query)
            .unwrap_or_else(|failure| panic!("{failure}"))
        {
            let Some(id) = resolver.id_of("bt-app", record) else {
                continue;
            };
            let reaches = resolver
                .resolve_callees(id)
                .iter()
                .any(|edge| edge.site.name == name && edge.to == Target::Item(callee));
            if reaches {
                callers.insert(format!(
                    "{}::{}::{}",
                    caller.module_path,
                    caller.type_owner.clone().unwrap_or_default(),
                    caller.name
                ));
            }
        }
    }
    callers
}

/// RED (T-POINTER-CAPTURE cut 5, R-9) — **every lifecycle door ends the capture**, and no caller
/// of the ending is outside the matrix: one row per door of §4.1 cut 5, read off the index, that
/// the door's body makes its ending call, and every caller of `end_capture` is a door or one of
/// the three roads named beside them. Two doors are reached by more than one road, so those roads
/// are held too: every closer of the settings sheet outside the sheet's own module — a caller of
/// `SettingsPanel::close` or `SettingsPanel::toggle`, resolved by the call graph — is
/// `settings_closed` or a caller of it (the sheet shutting itself answers
/// `SettingsKeyVerdict::Closed`, whose arms call it), and the dispatcher's blur arm says nothing
/// but `window_blurred`: the dispatcher calls none of the blur's own work.
///
/// MUTATION: remove the `end_capture` call from `hide_quake_window` — its row names the door.
#[test]
fn every_lifecycle_door_ends_the_capture() {
    for (owner, door, lands) in DOORS_NOT_YET_ITEMS {
        // The drag-out door is not written yet; the page seat's two are, and end no capture yet.
        let written = usize::from(owner == "WebSeat");
        source()
            .find(&bt_source::ItemQuery::method(owner, door).expecting(written))
            .unwrap_or_else(|failure| {
                panic!("`{owner}::{door}` is no longer what this list says ({lands}): {failure}")
            });
        assert!(
            !product_callers("Runtime", "end_capture").contains_key(&format!("{owner}::{door}")),
            "`{owner}::{door}` ends a capture: give it its row in the matrix ({lands})"
        );
    }
    for (door, call) in LIFECYCLE_DOORS {
        let callers = product_callers("Runtime", call);
        assert!(
            callers.contains_key(&format!("Runtime::{door}")),
            "the lifecycle door `Runtime::{door}` ends the capture through `{call}`; its callers \
             are {callers:?}"
        );
    }
    let doors: BTreeSet<String> = LIFECYCLE_DOORS
        .iter()
        .filter(|(_, call)| *call == "end_capture")
        .map(|(door, _)| format!("Runtime::{door}"))
        .chain(
            ENDINGS_REACHED_ANOTHER_WAY
                .iter()
                .map(|road| (*road).to_owned()),
        )
        .collect();
    let ending: BTreeSet<String> = product_callers("Runtime", "end_capture")
        .into_keys()
        .collect();
    assert_eq!(
        ending, doors,
        "every caller of `end_capture` is a door of the matrix or a road named beside it"
    );
    assert_eq!(
        product_callers("Runtime", "end_a_capture_that_lost_its_pointer"),
        BTreeMap::from([("Runtime::flush_pending_pty_resize".to_owned(), 1)]),
        "the resize door is the one turn-by-turn sample of the system's capture"
    );

    // The settings sheet's closers.
    let closing: BTreeSet<String> = product_callers("Runtime", "settings_closed")
        .into_keys()
        .collect();
    let closers: BTreeSet<String> = ["close", "toggle"]
        .into_iter()
        .flat_map(|name| resolved_callers_of("SettingsPanel", name))
        .collect();
    for (door, why) in [
        ("toggle_settings_panel", "the gear"),
        (
            "open_scheme_for_editing",
            "the verb that hands the reader a scheme file",
        ),
    ] {
        assert!(
            closers.contains(&format!("crate::Runtime::{door}")),
            "the call graph finds {why} shutting the sheet: {closers:?}"
        );
    }
    {
        let name = "close or toggle";
        for closer in closers {
            if closer.starts_with("crate::settings::") {
                continue;
            }
            let (rest, item) = closer.rsplit_once("::").expect("a caller has a name");
            let owner = rest.rsplit_once("::").map_or(rest, |(_, owner)| owner);
            let short = format!("{owner}::{item}");
            assert!(
                short == "Runtime::settings_closed" || closing.contains(&short),
                "`{closer}` shuts the settings sheet (`SettingsPanel::{name}`) without the door \
                 that lets go of a gesture held on it; `settings_closed` is called by {closing:?}"
            );
        }
    }

    // The blur arm. **A deny-list, by what the index can say**: a match arm is not an item, so the
    // index can name `window_blurred`'s callers and the calls an item makes, but not the
    // statements of one arm; reading the arm's text would be a new source-text pin, which this
    // ticket does not add. So the dispatcher is held to being the blur's only caller and to making
    // none of the blur's own calls itself — a statement of the blur's work written beside
    // `window_blurred` in the dispatcher names its call here.
    let dispatcher = "FolioApp::on_window_event".to_owned();
    assert_eq!(
        product_callers("Runtime", "window_blurred"),
        BTreeMap::from([(dispatcher.clone(), 1)]),
        "the window event dispatcher's blur arm is the blur's one road"
    );
    for blur_work in [
        "end_capture",
        "finish_rename",
        "hide_file_peek",
        "blurred",
        "close_command_palette",
        "cancel_drag",
        "cancel_divider_drag",
    ] {
        assert!(
            !product_callers("Runtime", blur_work).contains_key(&dispatcher),
            "the dispatcher does a blur's work (`{blur_work}`) beside `window_blurred`"
        );
    }
}
