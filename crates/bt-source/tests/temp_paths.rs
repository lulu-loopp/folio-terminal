//! **A test's scratch path is named by `bt_testpath` and by nothing else** —
//! T-TEST-HYGIENE-048, the guard half of the one owner.
//!
//! Guard (source-reading by design): its subject is how the workspace's test
//! code is written, so it reads that code through `bt-source`'s index, the way
//! `timing` does, and binds to no file.
//!
//! # The shape it refuses
//!
//! Test code that names a path by the process id and the wall clock —
//! `format!("probe-{}-{nanos}", std::process::id())` with the nanoseconds read
//! from `SystemTime::now()` — reads as unique and is not: two threads of one
//! test binary that sample the clock inside one tick of its resolution get the
//! same number. On main on 2026-10-05 that was a `remove_file` that found
//! `NotFound` in `shell_integration_script.rs`, because a parallel test had
//! already removed the file both of them were named. The one owner,
//! `bt_testpath::unique_name`, uses a process-wide ordinal instead, which a
//! clock cannot tie.
//!
//! The same scan refuses the process id beside `temp_dir()` in one body, which
//! is the other half of the old shape — a name made here rather than by the
//! owner, unique across processes and not across the tests of one.
//!
//! # Exactly what is read
//!
//! * **Test code**: a function or method whose every declaration path is out
//!   of the product build (`cfg(test)`, a module under it, an integration-test
//!   target), or stands on a `cfg` that names `test` — `any(test, feature =
//!   "test-shell")` is how `bt_pty::test_shell` and `bt_platform::trust_harness`
//!   are compiled, and they are test helpers that a feature also exposes. Code
//!   a shipped build compiles names its own temporary files and is not this
//!   guard's subject (`bt_persist::atomic`'s suffix carries an ordinal of its
//!   own).
//! * **In its own body**, closures and nested items included: a call through
//!   the path `process::id` together with any one of `temp_dir(`, `UNIX_EPOCH`
//!   or `SystemTime::now`.
//! * **Every first-party package but `bt-testpath`**, which is the owner and
//!   the one place a process id is allowed to become a test's path.
//!
//! A test that reads its process id for another reason — a recording that
//! says whose it was — takes its paths from the owner, so `temp_dir()` and the
//! process id never meet in its body.
//!
//! # What it does not see
//!
//! A name split across two functions — one reads the clock, another formats
//! the process id — is two bodies and no hit. A process id reached through a
//! re-export under another name (`use std::process::id as pid;`) is not the
//! path `process::id`. Review owns both.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use bt_source::{
    DiskScope, Index, ItemRecord, Package, Pattern, Search, Span, TargetId, TargetKind, TargetRoot,
    Universe, Vendor, View, Workspace, is_vendored, needle, report, universes,
};

/// The owner, the one package allowed to turn a process id into a test's path.
const OWNER: &str = "bt-testpath";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The whole of a first-party package; `bt-source`'s `tests/fixtures/` are
/// source *about* compilation and are declared out, as `timing` does.
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

/// Every span of `pattern` in the identifier view of `index`, sorted.
fn spans(index: &Index, pattern: Pattern) -> Vec<Span> {
    let mut found = index
        .search(&Search::new(needle!(pattern), View::Identifiers))
        .unwrap_or_else(|failure| panic!("{failure}"))
        .spans();
    found.sort();
    found
}

/// Whether any span of `sorted` lies inside `outer`.
fn any_inside(sorted: &[Span], outer: Span) -> bool {
    let first = sorted.partition_point(|span| span.start() < outer.start());
    sorted[first..]
        .iter()
        .take_while(|span| span.start() < outer.end())
        .any(|span| span.within(outer))
}

/// Whether a `cfg` predicate, as written, names the `test` configuration — a
/// whole word outside any string, so `feature = "test-shell"` does not.
fn names_test(predicate: &str) -> bool {
    let mut outside = String::with_capacity(predicate.len());
    let mut quoted = false;
    for character in predicate.chars() {
        if character == '"' {
            quoted = !quoted;
            outside.push(' ');
        } else if !quoted {
            outside.push(character);
        }
    }
    outside
        .split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .any(|word| word == "test")
}

/// Whether every declaration of `item` is test code (see the module header).
fn is_test_code(item: &ItemRecord) -> bool {
    let mut identities = item.identities().peekable();
    identities.peek().is_some()
        && identities.all(|identity| {
            !identity.variant.permits_product()
                || identity
                    .variant
                    .predicates()
                    .iter()
                    .any(|predicate| names_test(predicate))
        })
}

/// Every test-code function of `index` whose own body names a path by the
/// process id beside the clock or the temporary directory, as
/// `file:line:column identity`.
fn scan(index: &Index) -> BTreeSet<String> {
    let pid = spans(index, Pattern::path("process::id"));
    let beside: Vec<Span> = {
        let mut all = spans(index, Pattern::call("temp_dir"));
        all.extend(spans(index, Pattern::identifier("UNIX_EPOCH")));
        all.extend(spans(index, Pattern::path("SystemTime::now")));
        all.sort();
        all
    };
    let mut found = BTreeSet::new();
    for item in index.items() {
        let Some(body) = item.body() else { continue };
        if !item.kind().is_callable() || !is_test_code(item) {
            continue;
        }
        if any_inside(&pid, body) && any_inside(&beside, body) {
            let place = index
                .locate(item.declaration().start())
                .map_or_else(String::new, |location| location.to_string());
            for identity in item.identities() {
                found.insert(format!(
                    "{place} {}::{}",
                    identity.module_path, identity.name
                ));
            }
        }
    }
    found
}

/// Every first-party package's index but the owner's.
fn workspace_indices() -> Vec<Index> {
    let workspace = Workspace::read(&workspace_root()).expect("this workspace");
    let packages: Vec<&Package> = workspace
        .packages()
        .iter()
        .filter(|package| !is_vendored(package.directory()))
        .collect();
    assert!(
        packages.iter().any(|package| package.name() == OWNER),
        "{OWNER} is a member of this workspace"
    );
    packages
        .into_iter()
        .filter(|package| package.name() != OWNER)
        .map(|package| {
            Index::build(&universe_for(package))
                .unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
        })
        .collect()
}

/// The `temp_paths` fixture.
fn fixture() -> Index {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("temp_paths");
    let universe = Universe::declare(
        "the temp_paths fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "temp_paths".to_owned(),
                kind: TargetKind::Library,
                name: "temp_paths".to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

/// RED (T-TEST-HYGIENE-048) — **no test code outside `bt-testpath` names a
/// path by the process id beside the wall clock or the temporary directory.**
///
/// MUTATION: put the old shape back at one site — in
/// `bt_term::session`'s `temporary_relative_image_tree`, replace
/// `bt_testpath::temp_path(..)` with
/// `std::env::temp_dir().join(format!("…-{}-{unique}", std::process::id()))`
/// and its `SystemTime` read — and this names that function.
#[test]
fn no_test_names_a_path_by_its_process_id_and_the_clock() {
    let found: BTreeSet<String> = workspace_indices().iter().flat_map(scan).collect();
    assert!(
        found.is_empty(),
        "these test functions name a scratch path themselves, by the process id beside the \
         clock or the temporary directory. Two parallel tests of one binary can be given the \
         same such name; take it from `bt_testpath::temp_path(tag)` (or \
         `root.join(bt_testpath::unique_name(tag))` under a root of the test's own), whose \
         process-wide ordinal no two calls share:\n  {}",
        found.into_iter().collect::<Vec<_>>().join("\n  ")
    );
}

/// RED (T-TEST-HYGIENE-048) — **the scan reads test code and only test code,
/// and a test helper behind `any(test, feature = …)` is test code.**
///
/// The fixture has the old shape three times: in a `#[test]` function, in a
/// helper compiled under `any(test, feature = "test-helper")`, and in a
/// product function. The first two are found and the third is not; a test
/// that only prints its process id, and one that takes its name from the
/// owner, are not found either.
///
/// MUTATION: make `names_test` answer `false` and the helper is lost; make
/// `is_test_code` answer `true` and the product function is found.
#[test]
fn the_scan_finds_the_old_shape_in_test_code_and_in_test_helpers_only() {
    let found: Vec<String> = scan(&fixture())
        .into_iter()
        .map(|hit| {
            hit.rsplit_once(' ')
                .map_or(hit.clone(), |(_, identity)| identity.to_owned())
        })
        .collect();
    assert_eq!(
        found,
        [
            "crate::helper_scratch",
            "crate::tests::a_test_named_by_pid_and_clock"
        ],
        "{found:#?}"
    );
}
