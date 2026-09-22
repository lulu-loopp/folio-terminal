//! Structural red lines, guarded the way the spec says to guard them.
//!
//! D3 and L7 cannot be caught by asserting on output. There is exactly one
//! allocation path in this crate, so a float in it agrees with itself perfectly
//! — the drift D3 fears appears only when a *second* path exists, which is the
//! thing discipline ① forbids in the first place. Both red lines are therefore
//! guarded at the source level, which is the mechanism the spec names for L7
//! ("CI can grep their `use`") applied to its sibling.
//!
//! **The equivalence commit of P8** (`docs/plans/bt-app-split-prep.md` §6.3,
//! and §6.0 rule 3). The walk below reads `src/` one directory deep and is one
//! of the three non-recursive walkers of §3.3: `crates/bt-layout/src/` is flat
//! today, so it loses nothing today, and it loses every file of the first
//! subdirectory anybody adds. Beside it now stands the declared universe —
//! this package's own `src/`, reached through the `mod` declarations that
//! actually build it — and every reading below is taken twice and the two
//! answers are compared. The commit after this one deletes the older of the
//! two, because two implementations of one judgement do not vouch for each
//! other (`docs/CONVENTIONS.md` §十 rule 4).

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

/// One search over every file this crate's own declarations reach.
fn found(needle: Needle, view: View) -> Found {
    source()
        .search(&Search::new(needle, view))
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// `file:line` for every occurrence, deduplicated — the shape the line walk's
/// answer already has, so the two readings compare directly. A line holding a
/// spelling twice is one line to the walk and two occurrences to the search,
/// and the place is what both of them mean.
fn places(found: &Found) -> BTreeSet<String> {
    found
        .spans()
        .into_iter()
        .filter_map(|span| source().locate(span.start()))
        .map(|at| {
            format!(
                "{}:{}",
                at.file.file_name().unwrap_or_default().to_string_lossy(),
                at.line
            )
        })
        .collect()
}

fn sources() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out: Vec<(String, String)> = fs::read_dir(&dir)
        .expect("the crate has a src directory")
        .filter_map(|entry| {
            let path = entry.expect("readable entry").path();
            (path.extension()?.to_str()? == "rs").then(|| {
                (
                    path.file_name()
                        .expect("a file")
                        .to_string_lossy()
                        .into_owned(),
                    fs::read_to_string(&path).expect("readable source"),
                )
            })
        })
        .collect();
    out.sort();
    assert!(out.len() >= 7, "the scan must actually see the crate");
    out
}

/// Lines that carry code, with block-comment bodies and `///` docs removed.
fn code_lines(source: &str) -> Vec<(usize, String)> {
    let mut in_block = false;
    let mut out = Vec::new();
    for (n, raw) in source.lines().enumerate() {
        let line = raw.trim();
        if in_block {
            if let Some(rest) = line.split_once("*/") {
                in_block = false;
                out.push((n + 1, rest.1.to_string()));
            }
            continue;
        }
        if line.starts_with("//") {
            continue;
        }
        if let Some((before, _)) = line.split_once("/*") {
            in_block = !line.contains("*/");
            out.push((n + 1, before.to_string()));
            continue;
        }
        let code = line.split_once("//").map_or(line, |(before, _)| before);
        out.push((n + 1, code.to_string()));
    }
    out
}

/// Every place the line walk reports one of `needles`, as `file:line`.
fn walked_places(needles: &[&str]) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (name, source) in sources() {
        for (line_no, line) in code_lines(&source) {
            for needle in needles {
                if line.contains(needle) {
                    found.insert(format!("{name}:{line_no}"));
                }
            }
        }
    }
    found
}

/// Every place the declared universe reports one of `needles`, as `file:line`.
///
/// `View::CodeKeepingLiterals` is [`code_lines`]'s replacement and a strict
/// improvement on it: the line filter above drops whole comment lines and cuts
/// every other line at its first `//`, which mangles a line holding a URL
/// inside a string, while the view masks comments whole and preserves every
/// literal verbatim (plan §2.1).
fn declared_places(needles: &[&str]) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for needle in needles {
        found.extend(places(&found_text(needle)));
    }
    found
}

fn found_text(spelling: &str) -> Found {
    found(needle!(Pattern::text(spelling)), View::CodeKeepingLiterals)
}

fn listed(places: &BTreeSet<String>) -> String {
    places.iter().cloned().collect::<Vec<_>>().join("\n")
}

/// **The file-set diff §3.2 asks every migrated walker to ship** — not a count,
/// the sorted paths on each side.
#[test]
fn the_declared_universe_holds_the_files_the_walk_holds() {
    let walked: BTreeSet<PathBuf> = {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        fs::read_dir(&dir)
            .expect("the crate has a src directory")
            .filter_map(|entry| {
                let path = entry.expect("readable entry").path();
                (path.extension()?.to_str()? == "rs").then(|| bt_source::normalized(&path))
            })
            .collect()
    };
    let declared: BTreeSet<PathBuf> = source()
        .files()
        .iter()
        .map(|file| file.path().to_path_buf())
        .collect();
    // Printed, not only compared: §3.2 asks for the sorted list of paths and
    // not a count, and `--nocapture` is where a ticket takes it from.
    for path in &walked {
        println!("walked:   {}", path.display());
    }
    for path in &declared {
        println!("declared: {}", path.display());
    }
    let only_walked: Vec<String> = walked
        .difference(&declared)
        .map(|path| format!("walked, not declared: {}", path.display()))
        .collect();
    let only_declared: Vec<String> = declared
        .difference(&walked)
        .map(|path| format!("declared, not walked: {}", path.display()))
        .collect();
    assert!(
        only_walked.is_empty() && only_declared.is_empty(),
        "P8 file-set diff for bt-layout:\n{}\n{}",
        only_walked.join("\n"),
        only_declared.join("\n")
    );
    assert!(
        source().cross_check().agrees(),
        "a file of this crate's src is reached by no declaration:\n{}",
        source().cross_check().report()
    );
}

/// D3: fixed point, never floating point.
///
/// D1 wants bit-identical output, and `avail * ratio` in floating point can
/// differ by one ULP between two builds or two code paths, which after rounding
/// becomes a whole physical pixel of difference — the "two geometries always
/// drift" failure stated in the small.
#[test]
fn the_solver_uses_no_floating_point() {
    let needles = ["f32", "f64"];
    let walked = walked_places(&needles);
    let declared = declared_places(&needles);
    assert_eq!(
        walked,
        declared,
        "P8 equivalence, D3: the file walk and the declared universe disagree\n\
         walked only:\n{}\ndeclared only:\n{}",
        listed(&walked.difference(&declared).cloned().collect()),
        listed(&declared.difference(&walked).cloned().collect())
    );
    assert!(
        declared.is_empty(),
        "red line D3: floating point on a solve path:\n{}",
        listed(&declared)
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
    let needles = ["use bt_", "bt_viewport", "bt_doc"];
    let walked = walked_places(&needles);
    let declared = declared_places(&needles);
    assert_eq!(
        walked,
        declared,
        "P8 equivalence, L7: the file walk and the declared universe disagree\n\
         walked only:\n{}\ndeclared only:\n{}",
        listed(&walked.difference(&declared).cloned().collect()),
        listed(&declared.difference(&walked).cloned().collect())
    );
    assert!(declared.is_empty(), "red line L7: {}", listed(&declared));

    // And nothing at all in the manifest, so the graph cannot grow one quietly.
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
    let needles = ["HashMap", "HashSet"];
    let walked = walked_places(&needles);
    let declared = declared_places(&needles);
    assert_eq!(
        walked,
        declared,
        "P8 equivalence, L8: the file walk and the declared universe disagree\n\
         walked only:\n{}\ndeclared only:\n{}",
        listed(&walked.difference(&declared).cloned().collect()),
        listed(&declared.difference(&walked).cloned().collect())
    );
    assert!(declared.is_empty(), "red line L8: {}", listed(&declared));
}
