//! The enumeration, run over the tree it exists for.
//!
//! A fixture proves a rule; only the real tree proves the rule is the one this
//! workspace is written in. The independent filesystem/declaration cross-check
//! makes an undeclared source file or a declaration outside the package scope a
//! finding, never a count to adjust.

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
/// **It was empty, and that was a measured fact rather than an assumption.** Run
/// on 2026-09-21 over all nineteen members, the declarations reach every `.rs`
/// file under every member's `src/` and `tests/`, `vendor/` included — nothing
/// in this tree is a file that compiles nowhere. A row appearing here is a
/// finding with a reason beside it; it is never an entry added to make a red
/// test green.
///
/// Since A2a the package universe reads `examples/` and `benches/` too (a target
/// kind this crate now knows), and two vendored files there are no target: both
/// `mitex` manifests say `autobenches = false` because the vendoring dropped the
/// `divan` benchmark (see the comment at the top of each manifest), so the bench
/// sources ship in the directory and are compiled by nothing.
fn expected_unreached() -> BTreeMap<String, Vec<String>> {
    BTreeMap::from([
        (
            "mitex".to_owned(),
            vec!["vendor/mitex/benches/convert_large_projects.rs".to_owned()],
        ),
        (
            "mitex-parser".to_owned(),
            vec!["vendor/mitex-parser/benches/simple.rs".to_owned()],
        ),
    ])
}

/// RED — **every Rust source file under `bt-app/src` is reached by a declaration.**
///
/// The test asks the declarations and the disk independently. A source file that
/// compiles nowhere appears in `unreached`; a declaration pointing outside the
/// package's source scope makes the cross-check disagree.
///
/// MUTATION: add an undeclared `.rs` file under `bt-app/src`.
#[test]
fn every_bt_app_source_file_is_reached_by_a_declaration() {
    let workspace = workspace();
    let package = workspace.package("bt-app").expect("bt-app");
    let universe = universes::crate_sources(package, Vendor::Excluded).expect("bt-app's own src");
    let (enumeration, unreached) =
        enumerate(&universe).expect("bt-app's declarations resolve completely");
    unreached.expect_none("every .rs file under bt-app/src is reached by a declaration");

    println!("bt-app: {} files reached", enumeration.files().len());
    assert!(
        enumeration.cross_check().agrees(),
        "the declarations and the disk disagree about what bt-app/src holds:\n{}",
        enumeration.cross_check().report()
    );
}
