//! **The runtime dependency map's rules, one fixture at a time** — J3
//! (T-RUNTIME-DEP-MAP, 2026-10-11; the ownership contract's §3.1, ticket O-1).
//!
//! The fixture under `tests/fixtures/dep_map/` proves what the map adds to
//! the census: a read is its own kind and never a write; a call is an edge to
//! the file its callee is declared in, found by the calling module's own `use`
//! items or by its receiver's type; a door is an item outside the field's type
//! that is its only writer, or one of two; the crate root's leftovers are
//! listed least-referenced first. The map of `bt-app` itself, and the guard
//! that holds the committed files to it, are in `census.rs`, beside the census
//! they are taken from.

use std::path::Path;
use std::sync::Arc;

use bt_source::{
    Column, DependencyMap, DiskScope, FieldCensus, Index, TargetId, TargetKind, TargetRoot,
    Universe, Vendor, report,
};

/// The fixture's two structs.
const FIXTURE_STRUCTS: [&str; 2] = ["Leaf", "Runtime"];

// ── the fixture ───────────────────────────────────────────────────────────

fn fixture() -> (Arc<Index>, FieldCensus) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("dep_map");
    let universe = Universe::declare(
        "the dependency-map fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "dep-map-fixture".to_owned(),
                kind: TargetKind::Library,
                name: "dep_map_fixture".to_owned(),
            },
            file: root.join("lib").with_extension("rs"),
        }],
        vec![DiskScope::under(&root)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    let index =
        Index::shared(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
    let census =
        FieldCensus::take(&index, &FIXTURE_STRUCTS, &[&index]).expect("the fixture's structs");
    assert!(
        census.unparsed().is_empty(),
        "every declaration of the fixture parses: {:?}",
        census.unparsed()
    );
    (index, census)
}

/// `(item, kind)` for every use of one field of the fixture.
fn uses(map: &DependencyMap, structure: &str, field: &str) -> Vec<(String, &'static str)> {
    map.fields()
        .iter()
        .filter(|row| row.structure == structure && row.field == field)
        .map(|row| (row.item.clone(), row.kind))
        .collect()
}

/// RED (J3) — **a read is its own kind, and a write is never also a read.**
///
/// The census proved writes and left reads to a lexical count that could
/// not tell `leaf.text_scale` from another struct's `text_scale` (the
/// contract note's §1.2, the † names). The map reads them through the
/// census's own receiver resolution: a value read, a pattern that binds the
/// field, a `&self` method called on it, and the field on the way to another
/// one that is read. A compound assignment writes and does not read, and a
/// field on the way to a write is neither.
///
/// MUTATION: in `Walk::visit`, record a read for a field reached by a write
/// as well (`Pending::Attributed` recorded like `Pending::Read`), and
/// `settle`'s write of `Leaf.seen` reads `Runtime.leaf` — a read row the
/// fixture does not have.
#[test]
fn a_read_is_its_own_kind_and_never_a_write() {
    let (index, census) = fixture();
    let map = DependencyMap::of(&census, &index);
    assert_eq!(
        uses(&map, "Leaf", "text_scale"),
        [
            ("crate::Runtime::refresh".to_owned(), "read"),
            ("crate::model::scale_of".to_owned(), "read"),
            ("crate::model::step_text_scale".to_owned(), "write"),
        ]
    );
    assert_eq!(
        uses(&map, "Leaf", "label"),
        [("crate::model::describe".to_owned(), "read")]
    );
    let label = census
        .reads()
        .iter()
        .find(|row| row.fact == "Leaf.label")
        .expect("the label is read");
    assert_eq!(label.column, Column::Read);
    assert_eq!(label.kinds, ["pattern", "read"]);
    assert_eq!(label.sites, 2);
    // The step to a field that is read is read; the step to one that is
    // written is not.
    assert_eq!(
        uses(&map, "Runtime", "leaf"),
        [
            ("crate::Runtime::refresh".to_owned(), "read"),
            ("crate::runtime::panes::Runtime::zoom".to_owned(), "access"),
        ]
    );
    // The census's own columns do not carry a read.
    assert!(census.rows().iter().all(|row| row.column != Column::Read));
}

/// RED (J3) — **a call is an edge to the file its callee is declared in,
/// found by the calling module's own declarations and `use` items.**
///
/// A topic move needs to know which files call the item it moves, and the
/// item can be named four ways: by a name a `use` item brought in
/// (`step_text_scale`), by a path (`super::tabs::retab`, `crate::settle`), or
/// as a method of its receiver's type (`runtime.zoom()`). A name two glob
/// imports both bring in is no edge: it is counted as not followed, never
/// given to the first declaration of the name. A method a type declares
/// itself is the one a call reaches, before a trait's method of the same
/// name (`self.refresh()`).
///
/// MUTATION: in `Walk::edge`, take the first of several items a call
/// answers to instead of listing it as not followed, and `ambiguous` adds an
/// edge from `runtime/tabs.rs` to `model.rs`. Drop the inherent method's
/// precedence in `Resolver::workspace_methods`, and `zoom`'s
/// `self.refresh()` is not followed: `runtime/panes.rs` reaches `lib.rs` once.
#[test]
fn a_call_is_an_edge_to_the_file_its_callee_is_declared_in() {
    let (index, census) = fixture();
    let map = DependencyMap::of(&census, &index);
    let edges: Vec<(String, String, usize)> = map
        .topics()
        .iter()
        .map(|edge| (edge.from.clone(), edge.to.clone(), edge.calls))
        .collect();
    let edge = |from: &str, to: &str, calls: usize| (from.to_owned(), to.to_owned(), calls);
    assert_eq!(
        edges,
        [
            edge("lib.rs", "runtime/tabs.rs", 1),
            edge("runtime/panes.rs", "lib.rs", 2),
            edge("runtime/panes.rs", "model.rs", 1),
            edge("runtime/panes.rs", "runtime/tabs.rs", 1),
            edge("runtime/tabs.rs", "runtime/panes.rs", 1),
        ]
    );
    let example = map
        .topics()
        .iter()
        .find(|edge| edge.from == "runtime/tabs.rs")
        .map(|edge| edge.example.clone());
    assert_eq!(
        example.as_deref(),
        Some("crate::runtime::tabs::retab -> crate::runtime::panes::Runtime::zoom")
    );
    let unfollowed: Vec<(&str, &str, &str)> = census
        .unfollowed()
        .iter()
        .map(|row| {
            (
                row.function.as_str(),
                row.called.as_str(),
                row.reason.as_str(),
            )
        })
        .collect();
    assert_eq!(
        unfollowed,
        [(
            "ambiguous",
            "helper",
            "a call more than one item answers to"
        )]
    );
}

/// RED (J3) — **a door is an item outside the field's own type that is its
/// only writer, or one of two.**
///
/// The contract's moves turn each such item into a method of the type that
/// owns the field (§3.1 item 3). A writer that is already the type's own
/// method is no door, and a field with three writers has none: its writes
/// are not yet behind one.
///
/// MUTATION: raise `DOOR_WRITERS` to 3, and the grid's two outside writers
/// become doors.
#[test]
fn a_door_is_an_outside_writer_of_at_most_two() {
    let (index, census) = fixture();
    let map = DependencyMap::of(&census, &index);
    let doors: Vec<(String, String, String, usize, String)> = map
        .doors()
        .iter()
        .map(|door| {
            (
                door.structure.clone(),
                door.field.clone(),
                door.door.clone(),
                door.writes,
                door.other_writers.clone(),
            )
        })
        .collect();
    let door = |field: &str, item: &str, other: &str| {
        (
            "Leaf".to_owned(),
            field.to_owned(),
            item.to_owned(),
            1,
            other.to_owned(),
        )
    };
    assert_eq!(
        doors,
        [
            door("seen", "crate::runtime::tabs::mark", "crate::settle"),
            door("seen", "crate::settle", "crate::runtime::tabs::mark"),
            door("text_scale", "crate::model::step_text_scale", "-"),
        ]
    );
}

/// RED (J3) — **the crate root's free functions and `Runtime` methods are
/// listed least-referenced first, with the topics their callers are written
/// in and the topics of what they call.**
///
/// K2 moves them in this order (the contract's §3.2), and a row's
/// `called_from` is its candidate home.
///
/// MUTATION: sort the list by name alone, and `wire` (called by nothing)
/// comes last instead of first.
#[test]
fn the_unclassified_are_listed_least_referenced_first() {
    let (index, census) = fixture();
    let map = DependencyMap::of(&census, &index);
    let rows: Vec<(&str, &str, usize, usize, &str, &str)> = map
        .unclassified()
        .iter()
        .map(|row| {
            (
                row.item.as_str(),
                row.shape,
                row.called_by,
                row.calls,
                row.called_from.as_str(),
                row.calls_into.as_str(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                "crate::wire",
                "function",
                0,
                2,
                "-",
                "lib.rs 1, runtime/tabs.rs 1"
            ),
            (
                "crate::Runtime::refresh",
                "method",
                1,
                0,
                "runtime/panes.rs 1",
                "-"
            ),
            (
                "crate::settle",
                "function",
                2,
                0,
                "lib.rs 1, runtime/panes.rs 1",
                "-"
            ),
        ]
    );
}

// ── calls into the subject's traits on another package's types ───────────

/// The foreign-trait fixture: the subject (`tests/fixtures/dep_map_foreign/app/`) and the other
/// package whose types it implements its own traits on (`…/term/`), each its own universe, the
/// census taken over both as the real tree's is over the workspace.
fn foreign_fixture() -> (Arc<Index>, FieldCensus) {
    two_package_fixture("dep_map_foreign", "foreign-fixture", "foreign_fixture")
}

/// A fixture of two packages under `tests/fixtures/<folder>/`: the subject (`app/`, its package
/// and crate named by the caller) and `term/` (`term_fixture`), the census taken over both.
fn two_package_fixture(folder: &str, subject: &str, krate: &str) -> (Arc<Index>, FieldCensus) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(folder);
    let package = |folder: &str, package: &str, name: &str| {
        let at = root.join(folder);
        let universe = Universe::declare(
            format!("the two-package fixture's {folder}"),
            vec![TargetRoot {
                id: TargetId {
                    package: package.to_owned(),
                    kind: TargetKind::Library,
                    name: name.to_owned(),
                },
                file: at.join("lib").with_extension("rs"),
            }],
            vec![DiskScope::under(&at)],
            Vendor::Excluded,
        )
        .expect("the fixture is there");
        Index::shared(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
    };
    let app = package("app", subject, krate);
    let term = package("term", "term-fixture", "term_fixture");
    let census =
        FieldCensus::take(&app, &FIXTURE_STRUCTS, &[&app, &term]).expect("the fixture's structs");
    assert!(
        census.unparsed().is_empty(),
        "every declaration of the fixture parses: {:?}",
        census.unparsed()
    );
    (app, census)
}

/// The fixture's not-followed rows, as `(function, called, reason)`.
fn not_followed(census: &FieldCensus) -> Vec<(&str, &str, &str)> {
    census
        .unfollowed()
        .iter()
        .map(|row| {
            (
                row.function.as_str(),
                row.called.as_str(),
                row.reason.as_str(),
            )
        })
        .collect()
}

/// RED (T-CENSUS-FOLLOWS-CALLS-INTO-BT-APP-TRAITS, 2026-10-27) — **a call into the subject's
/// own trait, implemented on another package's type, is an edge to the file the impl is
/// written in.**
///
/// The census keys a method by its type's bare name, so it kept a call's callees to the
/// declarations of the receiver type's own packages: a same-named type elsewhere must not
/// answer. That also dropped the subject's trait impls on the type, which the orphan rule puts
/// outside the type's package: O-3's `TermViewPorts` forwards on bt-term's `DualPlaneSession`
/// reached nothing, with no edge and no not-followed row. An impl outside the type's packages
/// now answers when its own self type is placed at the receiver type's declaration — the call
/// graph's rule (`trait_members`, by the resolved self type). `twin.rs`'s `Session`, named like
/// the other package's, has its own `Ports` and never answers for it.
///
/// RED before: no edge, and nothing not followed. MUTATION: admit every declaration of the
/// subject package beside the type's own (`packages` plus the subject), and the twin's `port`
/// answers too: the two calls are "a call more than one item answers to" and the edge is gone.
#[test]
fn a_call_into_a_subject_trait_on_another_packages_type_is_an_edge() {
    let (index, census) = foreign_fixture();
    let map = DependencyMap::of(&census, &index);
    let edges: Vec<(&str, &str, usize, &str)> = map
        .topics()
        .iter()
        .map(|edge| {
            (
                edge.from.as_str(),
                edge.to.as_str(),
                edge.calls,
                edge.example.as_str(),
            )
        })
        .collect();
    assert_eq!(
        edges,
        [(
            "runtime/panes.rs",
            "ports.rs",
            2,
            "crate::runtime::panes::poke -> crate::ports::<Session as Ports>::port"
        )]
    );
    assert!(
        !not_followed(&census)
            .iter()
            .any(|(function, ..)| *function == "poke"),
        "both of `poke`'s calls are followed: {:?}",
        not_followed(&census)
    );
}

/// RED (T-CENSUS-FOLLOWS-CALLS-INTO-BT-APP-TRAITS, 2026-10-27; review fold F1, F2) — **a call
/// the census cannot follow to one implementation is listed as not followed, by kind; never
/// dropped.**
///
/// - `prod`'s `nudge` reaches the subject's `impl Nudge for Cell`, whose self type is written
///   `Cell`: a standard type's name and the other package's. The census's rules do not read the
///   `use` that says which, so the impl cannot be placed at the receiver's type.
/// - `elsewhere`'s `other` reaches `imported.rs`'s `impl Other for Session`, `Session` brought in
///   by a `use` while the subject declares a `Session` of its own (`twin.rs`). Placed by package
///   it would be the twin's, the call reaching nothing; a self type is placed only where the
///   rules are sure (F1).
/// - `renamed_crate`'s `aliased` reaches `aliased.rs`'s `impl Aliased for t::Session`, `t` a
///   renamed crate: a path through a name that is no module is not placed by package (row 84,
///   N2; placed by package it was the twin's, and the call was listed for the wrong reason).
/// - `outside`'s `Thing::port(..)` names a type outside the workspace on which the subject
///   implements `Ports`: the language picks the outside type's own `port` first when it has one,
///   which the census does not read (row 84, N1; dropped before).
/// - `Window::paint`'s `self.glass.shine()` is a call through `B::Glass`, which no product impl
///   of `Backend` binds (row 84 follows a bound one: `dep_map_assoc`), and `lit`'s `T::shine(..)`
///   a call through a generic's trait by its path, which reaches whichever implementation the
///   argument names: the census follows each to none (F2, for the path).
/// - `qualified`'s `<term_fixture::Session as Ports>::port(..)`: the rules read no qualified
///   self type.
///
/// RED before the change: none is listed, and none is an edge. RED before the review fold:
/// only `paint` and `prod` are. MUTATION: in `Resolver::reached`, drop an outside impl whose
/// self type is not placed instead of refusing it, and `prod`'s and `elsewhere`'s rows are gone;
/// in `Resolver::placed_for_sure`, accept a placement by package, and `elsewhere` is refused
/// for the twin's reason while `prod`'s `Cell` impl is guessed into an edge;
/// in `Resolver::through_binding`, answer no binding with no callee instead of the refusal, and
/// `paint`'s row is gone; in `Resolver::placed_for_sure`, accept any path that reaches one
/// declaration again, and `renamed_crate` is refused as "only other types"; in
/// `Resolver::associated`, drop the outside-type check, and `outside`'s row is gone. `renamed`'s
/// `rename`, declared only by the twin `Session`, is followed through the other package's deref
/// to `Inner::rename` (row 84, N3; listed as "only other types" before): MUTATION — do not go
/// past the other types (`past_other_types` answering `None`), and its row is back.
#[test]
fn a_call_the_census_cannot_follow_to_one_implementation_is_listed_by_kind() {
    let (_, census) = foreign_fixture();
    assert_eq!(
        not_followed(&census),
        [
            (
                "Window::paint",
                "shine",
                "an associated type no product impl binds, which the census follows to no \
                 implementation: `B::Glass`"
            ),
            (
                "lit",
                "T::shine",
                "a call through a generic's trait, which the census follows to no \
                 implementation: `T::shine`"
            ),
            (
                "outside",
                "Thing::port",
                "a type's path on a type outside the workspace, whose own functions the census \
                 does not read: `Thing::port`, which a workspace trait implements on it"
            ),
            (
                "elsewhere",
                "other",
                "a method a trait implements outside its type's package, on a self type the \
                 rules do not place: `Session` is not declared in the impl's own module, and \
                 the workspace declares 2 types of that name"
            ),
            (
                "prod",
                "nudge",
                "a method a trait implements outside its type's package, on a self type the \
                 rules do not place: `Cell` is both a standard type and a workspace type"
            ),
            (
                "qualified",
                "Ports::port",
                "a call through a qualified path"
            ),
            (
                "renamed_crate",
                "aliased",
                "a method a trait implements outside its type's package, on a self type the \
                 rules do not place: `Session` is not declared in the impl's own module, and \
                 the workspace declares 2 types of that name"
            ),
        ]
    );
    let renamed: Vec<String> = census
        .calls()
        .iter()
        .filter(|row| row.caller.function == "renamed")
        .flat_map(|row| row.callees.iter().map(|callee| callee.function.clone()))
        .collect();
    assert_eq!(renamed, ["Inner::rename"]);
}

// ── calls through an associated type ─────────────────────────────────────

/// The associated-type fixture (`tests/fixtures/dep_map_assoc/`): a backend seam over the other
/// package's types, as the window seam is over bt-render's and bt-platform's.
fn assoc_fixture() -> (Arc<Index>, FieldCensus) {
    two_package_fixture("dep_map_assoc", "assoc-fixture", "assoc_fixture")
}

/// The fixture's topic edges, as `(from, to, calls, example)`.
fn edges_of(map: &DependencyMap) -> Vec<(&str, &str, usize, &str)> {
    map.topics()
        .iter()
        .map(|edge| {
            (
                edge.from.as_str(),
                edge.to.as_str(),
                edge.calls,
                edge.example.as_str(),
            )
        })
        .collect()
}

/// RED (T-CENSUS-FOLLOWS-ASSOCIATED-TYPE-CALLS, 2026-10-28) — **a call through a generic's
/// associated type reaches the bound trait's impl on the one type the product impls bind it to;
/// one the census cannot follow to one implementation is listed by kind.**
///
/// Row 83 listed every call through `B::Glass` as not followed (567 sites in bt-app: the window
/// seam's `renderer`, `window`, `gpu`, `compositor`, … fields). What it reaches is in the index:
/// the product `impl Backend for Native` binds `type Glass = …`, and the bound trait's impl on
/// that type is the seam's forward. `frame.rs`'s four calls — through a field of `B::Glass`, a
/// field of `B::Device`, a `G::Device` parameter (`Device` declared by two traits, both bound to
/// `Gpu`), and `B::Glass::paint` by its path — are edges into `seam.rs`, never to the other
/// package's inherent `paint`/`dev`, and the test-only `Headless` binds nothing the product has.
/// `lanes.rs` holds the refusals — `Answer` bound to two types, `Load` to an impl's own generic,
/// `Part` by no impl — and `light`'s `bulb.glow()`, which reaches `Glow`'s provided method, since
/// `Bulb`'s impl does not write it.
///
/// `choir`'s `voice.hum()` is refused (`Voice` bound to two types) though `hum` is only a provided
/// method: a product provided method's name is one of the package's, so its refusal is listed
/// (the review's N1).
///
/// RED before: no edge; seven calls listed as "a call through an associated type", and `light`'s
/// `glow` (no impl of the package declares the name) dropped without a row; before the review
/// fold, `choir`'s refusal dropped without a row the same way. MUTATION:
/// in `Resolver::through_binding`, let an inherent method of the bound type answer (drop the
/// `trait_name` filter) — `paint`/`dev` reach the other package's inherent methods and the edge is
/// gone; count the test-only impl's bindings (drop `binding.product`) — `Glass` and `Device` are
/// bound to two types each and refused; follow only an associated type bound by exactly one impl
/// — `Device` (two bindings, one type) is refused.
#[test]
fn a_call_through_an_associated_type_reaches_the_bound_traits_impl_on_its_product_binding() {
    let (index, census) = assoc_fixture();
    let map = DependencyMap::of(&census, &index);
    assert_eq!(
        edges_of(&map),
        [(
            "runtime/frame.rs",
            "seam.rs",
            4,
            "crate::runtime::frame::Window::draw -> crate::seam::<Gpu as Device>::dev"
        )]
    );
    assert_eq!(
        not_followed(&census),
        [
            (
                "ask",
                "speak",
                "an associated type the product impls bind to types that differ: `O::Answer` \
                 (`Loud`, `Quiet`)"
            ),
            (
                "carry",
                "speak",
                "an associated type an impl binds to its own generic, which the census follows \
                 to no implementation: `C::Load` (`A`)"
            ),
            (
                "choir",
                "hum",
                "an associated type the product impls bind to types that differ: `C::Voice` \
                 (`Loud`, `Quiet`)"
            ),
            (
                "lone",
                "speak",
                "an associated type no product impl binds, which the census follows to no \
                 implementation: `L::Part`"
            ),
        ]
    );
    let light: Vec<String> = census
        .calls()
        .iter()
        .filter(|row| row.caller.function == "light")
        .flat_map(|row| row.callees.iter().map(|callee| callee.key()))
        .collect();
    assert_eq!(light, ["crate::lanes::Glow::glow"]);
}
