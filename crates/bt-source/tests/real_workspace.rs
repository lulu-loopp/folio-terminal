//! The enumeration, run over the tree it exists for.
//!
//! A fixture proves a rule; only the real tree proves the rule is the one this
//! workspace is written in. Two questions are asked here, and the second is the
//! one P1a is measured by: the wholly-test set of `bt-app` computed from the
//! declarations has to be the twelve files `docs/plans/bt-app-split-prep.md`
//! §6.6 names. A difference is a finding about whichever reading is wrong, never
//! a number to adjust — §6.0 rule 5.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bt_source::{
    DiskScope, Package, Universe, Vendor, Workspace, enumerate, is_vendored, report, universes,
};

fn workspace() -> Workspace {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    Workspace::read(&root).expect("this workspace")
}

/// `path` written from the workspace root, with one separator on both platforms.
fn from_root(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The universe this test asks of one package: everything its targets reach, and
/// everything under `src/` and `tests/` as text.
///
/// `bt-source`'s own `tests/fixtures/` is declared out of it, and that is the
/// whole point of a universe being declared: those files are *about* compilation
/// and are not part of one. A walk that had to infer the difference could not.
fn universe_for(package: &Package) -> Universe {
    let vendor = if is_vendored(package.directory()) {
        Vendor::Included
    } else {
        Vendor::Excluded
    };
    if package.name() == "bt-source" {
        let scopes = vec![
            DiskScope::under(package.directory().join("src")),
            DiskScope::under(package.directory().join("tests")).excluding(&["fixtures"]),
        ];
        return Universe::declare(
            "the whole of bt-source, fixtures aside",
            package.targets().to_vec(),
            scopes,
            vendor,
        )
        .expect("this crate is where it says it is");
    }
    universes::whole_package(package, vendor).expect("a package of this workspace")
}

/// RED — **every member crate enumerates with no ambiguity and no unfollowed
/// declaration**, and what the declarations do not reach is named.
///
/// The `UNREACHED` rows are the part worth reading. They are a value, not a
/// failure: each is a `.rs` file under a crate's `src/` or `tests/` that no
/// compilation of that package reaches, and the expected set below is what that
/// is true of today, file by file.
///
/// MUTATION: add a `.rs` file to any crate's `src/` without declaring it and it
/// appears here.
#[test]
fn every_member_crate_enumerates_without_a_refusal() {
    let workspace = workspace();
    let started = std::time::Instant::now();
    let mut never_reached: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut only_declared: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut refused: Vec<String> = Vec::new();
    let mut files = 0usize;

    for package in workspace.packages() {
        let universe = universe_for(package);
        match enumerate(&universe) {
            Ok((enumeration, unreached)) => {
                files += enumeration.files().len();
                let rows = named(unreached.carried_forward(), workspace.root());
                if !rows.is_empty() {
                    never_reached.insert(package.name().to_owned(), rows);
                }
                let outside = named(
                    enumeration.cross_check().only_declared.iter().cloned(),
                    workspace.root(),
                );
                if !outside.is_empty() {
                    only_declared.insert(package.name().to_owned(), outside);
                }
            }
            Err(rejections) => {
                refused.push(format!("{}:\n{}", package.name(), report(&rejections)))
            }
        }
    }

    println!(
        "{} packages, {files} files, {:?}",
        workspace.packages().len(),
        started.elapsed()
    );
    println!("UNREACHED, per package:\n{never_reached:#?}");
    println!("declared and outside every scope, per package:\n{only_declared:#?}");
    assert!(
        refused.is_empty(),
        "the walk refused to answer for these packages:\n{}",
        refused.join("\n")
    );
    assert_eq!(
        never_reached,
        expected_unreached(),
        "the set of files no declaration reaches changed; each row is a finding with a reason, \
         never a number to adjust"
    );
    assert!(
        only_declared.is_empty(),
        "a declaration reached a file outside every disk scope of its own package: \
         {only_declared:#?}"
    );
}

fn named(paths: impl IntoIterator<Item = PathBuf>, root: &Path) -> Vec<String> {
    let mut rows: Vec<String> = paths
        .into_iter()
        .map(|path| from_root(&path, root))
        .collect();
    rows.sort();
    rows
}

/// Every `.rs` file in a member crate's `src/` or `tests/` that no compilation
/// of that crate reaches, with the reason each one is here.
///
/// **It is empty, and that is a measured fact rather than an assumption.** Run
/// on 2026-09-21 over all nineteen members, the declarations reach every `.rs`
/// file under every member's `src/` and `tests/`, `vendor/` included — nothing
/// in this tree is a file that compiles nowhere. A row appearing here is a
/// finding with a reason beside it; it is never an entry added to make a red
/// test green.
fn expected_unreached() -> BTreeMap<String, Vec<String>> {
    BTreeMap::new()
}

/// RED — **`bt-app`'s wholly-test files are the twelve the plan names**, derived
/// from the declarations rather than listed. Twelve when the plan was written
/// (`docs/plans/bt-app-split-prep.md` §6.6); a thirteenth,
/// `text_size_tests.rs` (0.4.5 ticket 37, 2026-09-24), declared `#[cfg(test)] mod
/// text_size_tests;` in `main.rs`; twelve again since 0.4.6 census-3
/// (2026-09-25), which moved `attention.rs` and its `attention/tests.rs` out of
/// `bt-app` into `bt-workbench`; fourteen with A5's two lane files; fifteen
/// with `update_eligibility.rs` (0.4.6 ticket U-8), the build script's decision
/// that `main.rs` declares `#[cfg(test)]` so its tests run.
///
/// `scripts/dev/bt-app-graph.py` carries this set as a hand-written literal of
/// five names, and §6.6 of the plan is about the seven it is missing — four of
/// which contain source readers. This is the reading P0 makes that table agree
/// with, so the day a thirteenth appears is a red test rather than a table that
/// quietly means less than it says.
///
/// MUTATION: take `#[cfg(test)]` off `mod tests;` in `main.rs` and the set loses
/// `tests.rs`; put one on `mod quake;` and it gains `quake.rs`.
#[test]
fn the_wholly_test_files_of_bt_app_are_the_sixteen() {
    let workspace = workspace();
    let package = workspace.package("bt-app").expect("bt-app");
    let universe = universes::crate_sources(package, Vendor::Excluded).expect("bt-app's own src");
    let (enumeration, unreached) =
        enumerate(&universe).expect("bt-app's declarations resolve completely");
    unreached.expect_none("every .rs file under bt-app/src is reached by a declaration");

    let root = package.directory().join("src");
    let wholly = named(enumeration.wholly_test_files(), &root);
    assert_eq!(
        wholly,
        [
            "attention_words/tests.rs",
            "file_reads_source_tests.rs",
            "focus_thumb_restore_tests.rs",
            "ime_report_tests.rs",
            "journeys_tests.rs",
            "lane.rs",
            "lane_contract_tests.rs",
            "present_diagnostics_tests.rs",
            "preview_typing.rs",
            "preview_viewport_tests.rs",
            "source_pin.rs",
            "tests.rs",
            "text_size_tests.rs",
            "uninstall_tests.rs",
            "update_eligibility.rs",
            "window_waits_tests.rs",
        ],
        "the sixteen of `docs/plans/bt-app-split-prep.md` §6.6"
    );
    println!("bt-app: {} files reached", enumeration.files().len());
    assert!(
        enumeration.cross_check().agrees(),
        "the declarations and the disk disagree about what bt-app/src holds:\n{}",
        enumeration.cross_check().report()
    );
}
