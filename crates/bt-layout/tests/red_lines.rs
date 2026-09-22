//! Structural red lines, guarded the way the spec says to guard them.
//!
//! D3 and L7 cannot be caught by asserting on output. There is exactly one
//! allocation path in this crate, so a float in it agrees with itself perfectly
//! — the drift D3 fears appears only when a *second* path exists, which is the
//! thing discipline ① forbids in the first place. Both red lines are therefore
//! guarded at the source level, which is the mechanism the spec names for L7
//! ("CI can grep their `use`") applied to its sibling.
//!
//! **What "the source" is, is declared and not walked** (P8 of
//! `docs/plans/bt-app-split-prep.md`; §3.1). Until the commit before this one
//! these rules were read off a listing of `src/`, one directory deep — the
//! second of §3.3's three non-recursive walkers. It lost nothing while this
//! crate's `src/` was flat, and it would have lost every file of the first
//! subdirectory anybody added, silently and without changing a verdict. What
//! the rules ask now is `bt-source`: the files this package's own `mod`
//! declarations reach, wherever they are written, with a file on the disk that
//! no declaration reaches reported rather than skipped.
//!
//! **The view is named and there is no default** (§2.1).
//! `View::CodeKeepingLiterals` replaces the hand-rolled line filter that stood
//! here: it masks comments and doc comments whole and preserves every string
//! literal verbatim, where the filter dropped whole comment lines and cut every
//! other line at its first `//` — which mangles a line holding a URL inside a
//! string.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use bt_source::{Found, Index, Needle, Pattern, Search, View, needle};

/// **This crate, indexed once per process** — the workspace read, this
/// package's own `src/` declared as the universe and lowered, on the first ask
/// of the process, behind one call.
fn source() -> &'static Index {
    Index::of_package("bt-layout")
}

/// One search over every file this crate's own declarations reach, refusing
/// loudly rather than answering a smaller question.
fn found(needle: Needle, view: View) -> Found {
    source()
        .search(&Search::new(needle, view))
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// Every place one of `spellings` is written in code — comments masked,
/// literals kept — as one report a reader can act on.
fn lines_holding(spellings: &[&str]) -> Vec<String> {
    let mut places = BTreeSet::new();
    for spelling in spellings {
        let hits = found(needle!(Pattern::text(spelling)), View::CodeKeepingLiterals);
        for span in hits.spans() {
            if let Some(at) = source().locate(span.start()) {
                places.insert(format!("{at}: {spelling}"));
            }
        }
    }
    places.into_iter().collect()
}

/// **The evidence §3.2 asks a migrated walker to ship**, kept rather than
/// spent: the declared file set, printed in full, and the cross-check that
/// says the disk holds nothing the declarations do not reach.
///
/// This is what the deleted directory walk's `assert!(out.len() >= 7)` meant —
/// that the scan really saw the crate — said about declarations instead of
/// about a listing, and it is also what makes a `.rs` file dropped into `src/`
/// without a `mod` for it a red test rather than a file nobody reads.
#[test]
fn every_file_of_the_solver_is_reached_by_a_declaration() {
    let index = source();
    // Printed, not only compared: §3.2 asks for the sorted list of paths and
    // not a count, and `--nocapture` is where a ticket takes it from.
    for file in index.files() {
        println!("declared: {}", file.path().display());
    }
    assert!(
        index.cross_check().agrees(),
        "a `.rs` file under this crate's `src/` is reached by no declaration, \
         so no rule below is read against it:\n{}",
        index.cross_check().report()
    );
    assert!(
        index.files().len() >= 7,
        "the reading must actually see the crate, saw {}",
        index.files().len()
    );
}

/// D3: fixed point, never floating point.
///
/// D1 wants bit-identical output, and `avail * ratio` in floating point can
/// differ by one ULP between two builds or two code paths, which after rounding
/// becomes a whole physical pixel of difference — the "two geometries always
/// drift" failure stated in the small.
///
/// The needle is plain bytes and not an identifier, deliberately: `1.0f32` is
/// a suffixed literal and not a name, and a reading that only saw names would
/// stop seeing exactly the spelling that puts a float in an expression.
#[test]
fn the_solver_uses_no_floating_point() {
    let found = lines_holding(&["f32", "f64"]);
    assert!(
        found.is_empty(),
        "red line D3: floating point on a solve path:\n{}",
        found.join("\n")
    );
}

/// L7: `bt-layout` depends on nothing, and on `bt-viewport` / `bt-doc` /
/// `bt-term` / `bt-render` least of all.
///
/// The judgement in one sentence: the solver answers what shape a tree unfolds
/// into, never what is drawn inside that shape. It therefore does not know cell
/// sizes, cols/rows, scroll anchors, height trees or layout keys.
#[test]
fn the_solver_depends_on_no_other_crate() {
    let found = lines_holding(&["use bt_", "bt_viewport", "bt_doc"]);
    assert!(found.is_empty(), "red line L7:\n{}", found.join("\n"));

    // And nothing at all in the manifest, so the graph cannot grow one quietly.
    // `[dev-dependencies]` is a different section and a different claim: the
    // reading above is a dev-dependency itself, and what ships is what
    // `[dependencies]` names.
    let manifest = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("readable manifest");
    let deps = manifest
        .split_once("[dependencies]")
        .expect("the manifest states its dependency section explicitly")
        .1;
    let declared: Vec<&str> = deps
        .lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with('['))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    assert!(
        declared.is_empty(),
        "bt-layout grew a dependency: {declared:?}"
    );
}

/// L8: no hash container's iteration order may reach a geometric decision.
///
/// A traversal order that happens to be stable is only an order your samples
/// have not falsified yet — the same "no heuristics" rule that governs the VT
/// layer, applied to geometry.
#[test]
fn geometry_never_depends_on_hash_iteration_order() {
    let found = lines_holding(&["HashMap", "HashSet"]);
    assert!(found.is_empty(), "red line L8:\n{}", found.join("\n"));
}
