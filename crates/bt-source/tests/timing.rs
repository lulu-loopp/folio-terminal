//! **The timing-bound tests are exactly the ones on the list** — T-GATES-047,
//! the discovery half of `docs/plans/TIMING-BOUND-TESTS.tsv`.
//!
//! The list names every test whose verdict can depend on real elapsed time, so
//! that the number only goes down: `scripts/ci/check-timing-bound.ps1` refuses a
//! row added against the merge base, and this test refuses a timing-bound test
//! that is not a row, or a row that names no such test. Between them a new test
//! that waits on the real clock is red either way — unlisted here, or listed
//! there.
//!
//! # What "timing-bound" means here, exactly
//!
//! A **test** is a function carrying `#[test]` (outside comments), in any
//! first-party package's `src/`, `tests/`, `examples/` or `benches/`. It is
//! **timing-bound** when its own body — the braces of that function, closures
//! and nested items included — calls one of [`REAL_WAITS`] or [`MEASURES`]:
//! a stable standard-library wait whose length is real time (`thread::sleep`,
//! `Receiver::recv_timeout`, `Condvar::wait_timeout`, `thread::park_timeout`
//! and their variants) or a reading of how much real time passed
//! (`Instant::elapsed`).
//!
//! The scan matches the called name, not the callee, so the names are only
//! the standard library's while nothing in the workspace declares a function
//! or method by one of them — and that is asserted, not assumed
//! ([`no_workspace_function_is_named_like_a_real_wait`]). A controlled-clock
//! helper named like a trigger is a finding to rename, never a test silently
//! listed as timing-bound; `CardLoop::sleep_until` in `focus_thumb`'s tests,
//! which advances a model clock, is why `sleep_until` is not a trigger (it is
//! not a stable `std::thread` item either).
//!
//! # What it does not see, and who does
//!
//! * **A clock reached through a function the test calls.** A test that hands
//!   `Duration::from_secs(5)` to a helper that polls, or that passes a product
//!   function a deadline, carries a wall-clock bound this scan does not follow:
//!   the definition is the test's own body, and product functions do not count.
//!   Review owns discovering those; a row for one is written by hand and kept
//!   by the same shrink-only rule, but this test refuses it as stale, so today
//!   the list holds only what the scan proves.
//! * **`Instant` and `Duration` as values.** Most of the tree's tests build a
//!   controlled clock from `Instant::now()` plus `Duration::from_*` offsets,
//!   and their verdict does not move with the machine's speed (880 test bodies
//!   name `Instant` on 2026-09-30, and all but a few dozen are this). A
//!   measurement written as `Instant::now() - start` rather than `elapsed()` is
//!   in this blind spot too; review owns it.
//!
//! Each run leaves the list it expects in
//! `target/timing-bound-tests/TIMING-BOUND-TESTS.tsv`, with a seam column
//! chosen by what the body calls, for a ticket that converts or removes tests
//! to copy rows from — never to add them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use bt_source::{
    DiskScope, Index, ItemRecord, Package, Pattern, Search, Span, TargetId, TargetKind, TargetRoot,
    Universe, Vendor, View, Workspace, is_vendored, needle, report, universes,
};

/// The list, from the workspace root.
const LIST: &str = "docs/plans/TIMING-BOUND-TESTS.tsv";

/// The list's column header.
const HEADER: &str = "test\tcrate\twall_clock_assumption\tdeterministic_seam";

/// Stable standard-library calls that wait for a length of real time:
/// `thread::sleep`, `thread::sleep_ms`, `Receiver::recv_timeout`,
/// `Condvar::wait_timeout`, `Condvar::wait_timeout_ms`,
/// `Condvar::wait_timeout_while`, `thread::park_timeout`,
/// `thread::park_timeout_ms`.
const REAL_WAITS: [&str; 8] = [
    "sleep",
    "sleep_ms",
    "recv_timeout",
    "wait_timeout",
    "wait_timeout_ms",
    "wait_timeout_while",
    "park_timeout",
    "park_timeout_ms",
];

/// Calls that read how much real time passed.
const MEASURES: [&str; 1] = ["elapsed"];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The whole of a first-party package; `bt-source`'s `tests/fixtures/` are
/// source *about* compilation and are declared out, as `real_workspace` does.
fn universe_for(package: &Package) -> Universe {
    if package.name() == "bt-source" {
        let scopes = vec![
            DiskScope::under(package.directory().join("src")),
            DiskScope::under(package.directory().join("tests")).excluding(&["fixtures"]),
        ];
        return Universe::declare(
            "the whole of bt-source, fixtures aside",
            package.targets().to_vec(),
            scopes,
            Vendor::Excluded,
        )
        .expect("this crate is where it says it is");
    }
    universes::whole_package(package, Vendor::Excluded).expect("a package of this workspace")
}

/// Every span of `pattern` in `view`, sorted.
fn spans(index: &Index, search: &Search) -> Vec<Span> {
    let mut found = index
        .search(search)
        .unwrap_or_else(|failure| panic!("{failure}"))
        .spans();
    found.sort();
    found
}

/// The spans of `sorted` that lie inside `outer`.
fn inside(sorted: &[Span], outer: Span) -> impl Iterator<Item = &Span> {
    let first = sorted.partition_point(|span| span.start() < outer.start());
    sorted[first..]
        .iter()
        .take_while(move |span| span.start() < outer.end())
        .filter(move |span| span.within(outer))
}

/// The names a test answers to, one per target that compiles it:
/// `<package>:<kind>:<target> <module path>::<name>`, with the crate root's
/// `crate` left off — the target and the filter `cargo test` takes.
fn identities(index: &Index, item: &ItemRecord) -> BTreeSet<String> {
    let owners = index.file_of(item).owners();
    let mut found = BTreeSet::new();
    for declared in item.module_paths() {
        let owner = owners
            .iter()
            .filter(|owner| {
                declared == owner.module_path
                    || declared.starts_with(&format!("{}::", owner.module_path))
            })
            .max_by_key(|owner| owner.module_path.len())
            .expect("an item's module path extends its file's");
        let path = declared
            .strip_prefix("crate")
            .map_or(declared, |rest| rest.trim_start_matches("::"));
        let name = if path.is_empty() {
            item.name().to_owned()
        } else {
            format!("{path}::{}", item.name())
        };
        found.insert(format!("{} {name}", owner.target));
    }
    found
}

/// One timing-bound test the scan found.
struct Found {
    package: String,
    waits: BTreeSet<&'static str>,
    measures: bool,
}

impl Found {
    /// The row this test would have if it were listed today.
    fn row(&self, identity: &str) -> String {
        let calls: Vec<&str> = self
            .waits
            .iter()
            .copied()
            .chain(self.measures.then_some("elapsed"))
            .collect();
        let (assumption, seam) = match (self.waits.is_empty(), self.measures) {
            (false, false) => (
                "the awaited event happens within the numeric wait",
                "completion signal, controlled receiver or controlled sleeper",
            ),
            (true, true) => (
                "the operation finishes within the measured real time",
                "operation counter or controlled clock",
            ),
            _ => (
                "the awaited event happens within the numeric wait, and the measured real time stays within its bound",
                "completion signal plus operation counter or controlled clock",
            ),
        };
        format!(
            "{identity}\t{}\t{assumption} (calls {} in its own body)\t{seam}",
            self.package,
            calls.join(", ")
        )
    }
}

/// Every timing-bound test of one index, added to `found` by identity.
fn scan_index(index: &Index, package: &str, found: &mut BTreeMap<String, Found>) {
    let attributes = spans(
        index,
        &Search::new(needle!(Pattern::text("#[test]")), View::CodeKeepingLiterals),
    );
    let hits: Vec<(&'static str, bool, Vec<Span>)> = REAL_WAITS
        .iter()
        .map(|name| (*name, false))
        .chain(MEASURES.iter().map(|name| (*name, true)))
        .map(|(name, measures)| {
            let search = Search::new(needle!(Pattern::call(name)), View::Identifiers);
            (name, measures, spans(index, &search))
        })
        .collect();
    for item in index.items() {
        let Some(body) = item.body() else { continue };
        if !item.kind().is_callable() || inside(&attributes, item.declaration()).next().is_none() {
            continue;
        }
        let mut waits = BTreeSet::new();
        let mut measured = false;
        for (name, measures, sites) in &hits {
            if inside(sites, body).next().is_some() {
                if *measures {
                    measured = true;
                } else {
                    waits.insert(*name);
                }
            }
        }
        if waits.is_empty() && !measured {
            continue;
        }
        for identity in identities(index, item) {
            found.insert(
                identity,
                Found {
                    package: package.to_owned(),
                    waits: waits.clone(),
                    measures: measured,
                },
            );
        }
    }
}

/// Every function or method an index declares under a trigger's name.
fn declared_triggers(index: &Index) -> Vec<String> {
    index
        .items()
        .iter()
        .filter(|item| item.kind().is_callable())
        .filter(|item| REAL_WAITS.contains(&item.name()) || MEASURES.contains(&item.name()))
        .flat_map(|item| item.identities().map(|identity| identity.to_string()))
        .collect()
}

/// Every first-party package's index.
fn workspace_indices() -> Vec<(String, Index)> {
    let workspace = Workspace::read(&workspace_root()).expect("this workspace");
    workspace
        .packages()
        .iter()
        .filter(|package| !is_vendored(package.directory()))
        .map(|package| {
            let index = Index::build(&universe_for(package))
                .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
            (package.name().to_owned(), index)
        })
        .collect()
}

/// The `timing` fixture: a controlled clock whose method is named like a wait,
/// and a real sleep.
fn fixture() -> Index {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("timing");
    let universe = Universe::declare(
        "the timing fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "timing".to_owned(),
                kind: TargetKind::Library,
                name: "timing".to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

/// The committed rows, by test identity, with their crate column.
fn committed() -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(workspace_root().join(LIST)).expect("the list is committed");
    let mut lines = text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    assert_eq!(lines.next(), Some(HEADER), "{LIST}'s column header");
    let mut rows = BTreeMap::new();
    for line in lines {
        let columns: Vec<&str> = line.split('\t').collect();
        assert_eq!(columns.len(), 4, "{LIST}: a row is four columns: {line}");
        assert!(
            rows.insert(columns[0].to_owned(), columns[1].to_owned())
                .is_none(),
            "{LIST} names {} twice",
            columns[0]
        );
    }
    rows
}

/// Leave the list this scan expects where a ticket can copy rows from it.
fn leave_rendering(found: &BTreeMap<String, Found>) {
    let out = workspace_root().join("target").join("timing-bound-tests");
    std::fs::create_dir_all(&out).expect("target/ is writable");
    let mut text = String::from(HEADER);
    text.push('\n');
    for (identity, test) in found {
        text.push_str(&test.row(identity));
        text.push('\n');
    }
    std::fs::write(out.join("TIMING-BOUND-TESTS.tsv"), text).expect("written");
}

/// RED (T-GATES-047) — **every test whose own body waits on or measures the
/// real clock is a row of the timing-bound list, and every row is such a
/// test.**
///
/// The list was first written by attributing clock calls to the nearest test
/// above them, which listed source-reading tests for a product function's
/// `Instant` and could not see a new test at all. This is the list's other
/// half: the list only shrinks against the merge base
/// (`scripts/ci/check-timing-bound.ps1`), and here the tree is held to the
/// list, so a new timing-bound test cannot land by leaving the list alone.
///
/// MUTATION: add `#[test] fn planted() { std::thread::sleep(std::time::Duration::from_millis(1)); }`
/// to any crate and this names it as unlisted; delete one row and this names
/// that test.
#[test]
fn the_timing_bound_tests_are_exactly_the_listed_ones() {
    let mut found = BTreeMap::new();
    let mut misnamed = Vec::new();
    for (package, index) in workspace_indices() {
        scan_index(&index, &package, &mut found);
        misnamed.extend(declared_triggers(&index));
    }
    assert!(
        misnamed.is_empty(),
        "the scan reads these names as the standard library's real waits and measurements, and \
         the workspace declares functions by them — rename each (a controlled clock is not a \
         real wait), so no test calling it is listed as timing-bound:\n  {}",
        misnamed.join("\n  ")
    );
    leave_rendering(&found);
    let listed = committed();
    println!(
        "{} timing-bound tests found, {} listed",
        found.len(),
        listed.len()
    );

    let unlisted: Vec<String> = found
        .iter()
        .filter(|(identity, _)| !listed.contains_key(*identity))
        .map(|(identity, test)| format!("  {}", test.row(identity)))
        .collect();
    let stale: Vec<String> = listed
        .keys()
        .filter(|identity| !found.contains_key(*identity))
        .map(|identity| format!("  {identity}"))
        .collect();
    let wrong_crate: Vec<String> = found
        .iter()
        .filter_map(|(identity, test)| {
            let written = listed.get(identity)?;
            (written != &test.package)
                .then(|| format!("  {identity}: {written} is not {}", test.package))
        })
        .collect();
    assert!(
        unlisted.is_empty() && stale.is_empty() && wrong_crate.is_empty(),
        "{LIST} and the tree disagree.\n\n\
         Timing-bound tests that are not listed ({}) — give each a seam instead of the real \
         clock (a completion signal, a controlled receiver, sleeper or clock); a row is never \
         added to let one past, and scripts/ci/check-timing-bound.ps1 refuses one:\n{}\n\n\
         Rows that name no timing-bound test ({}) — the test was converted, renamed or moved; \
         remove the row (a moved test that still waits is a new row, and refused):\n{}\n\n\
         Rows with the wrong crate ({}):\n{}\n\n\
         The list this tree implies is in target/timing-bound-tests/TIMING-BOUND-TESTS.tsv.",
        unlisted.len(),
        unlisted.join("\n"),
        stale.len(),
        stale.join("\n"),
        wrong_crate.len(),
        wrong_crate.join("\n"),
    );
}

/// RED (T-GATES-047 round 4) — **a controlled clock's method named like a wait
/// is not a real wait, and a real `std::thread::sleep` still is.**
///
/// The fixture declares `CardLoop::sleep_until`, which advances a model clock
/// the way `focus_thumb`'s card loop does, and two tests: one that calls it and
/// one that sleeps. The scan must find only the second, and the fixture's own
/// `sleep_until` must not be a declared trigger.
///
/// MUTATION: put `"sleep_until"` back into `REAL_WAITS` and the controlled
/// test is found.
#[test]
fn a_controlled_sleep_until_is_not_a_real_wait_and_a_real_sleep_is() {
    let index = fixture();
    let mut found = BTreeMap::new();
    scan_index(&index, "timing", &mut found);
    let names: Vec<&str> = found.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        ["timing:lib:timing a_test_that_really_sleeps"],
        "only the real sleep is timing-bound"
    );
    assert_eq!(declared_triggers(&index), Vec::<String>::new());
}

/// RED (T-GATES-047 round 4) — **nothing in the workspace declares a function
/// or method by a trigger's name**, so every call the scan matches is the
/// standard library's.
///
/// The list test asserts the same thing over the same indices before it
/// compares; this one says it on its own, fixture-free.
///
/// MUTATION: put `"sleep_until"` back into `REAL_WAITS` and this names
/// `focus_thumb`'s `CardLoop::sleep_until`.
#[test]
fn no_workspace_function_is_named_like_a_real_wait() {
    let misnamed: Vec<String> = workspace_indices()
        .iter()
        .flat_map(|(_, index)| declared_triggers(index))
        .collect();
    assert!(misnamed.is_empty(), "{misnamed:#?}");
}
