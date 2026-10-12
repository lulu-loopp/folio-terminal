//! **The runtime dependency map is generated, and the committed map is the
//! code's** — J3 (T-RUNTIME-DEP-MAP, 2026-10-11; the ownership contract's
//! §3.1, ticket O-1).
//!
//! The fixture under `tests/fixtures/dep_map/` proves the three rules the map
//! adds to the census, one at a time: a read is its own kind and never a
//! write; a free function's call is an edge to the file its declaration is
//! written in, found by the module's own `use` items; a door is an item
//! outside the field's type that is its only writer, or one of two.

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
