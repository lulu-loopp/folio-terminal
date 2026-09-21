//! One test per refusal.
//!
//! Every one of these is a condition the two resolvers already in the tree
//! answer instead of refusing: `bt_platform::…::module_file` takes the `name.rs`
//! form without asking whether `name/mod.rs` is there too, and
//! `bt_app::file_reads_source_tests::scan` walks past a declaration it cannot
//! follow. A universe that quietly shrinks is the failure
//! `docs/plans/bt-app-split-prep.md` exists to remove, so each of these is
//! staged on a tiny tree of its own under `tests/fixtures/` and asserted by
//! variant rather than by message.

use std::path::{Path, PathBuf};

use bt_source::{
    DiskScope, Rejection, TargetId, TargetKind, TargetRoot, Universe, Vendor, Workspace, enumerate,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// A one-root, one-scope universe over the fixture directory `name`.
fn over(name: &str, root: &str) -> Universe {
    let directory = fixture(name);
    Universe::declare(
        name,
        vec![TargetRoot {
            id: TargetId {
                package: name.to_owned(),
                kind: TargetKind::Library,
                name: name.to_owned(),
            },
            file: directory.join(root),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there and is not vendored")
}

/// The refusals `enumerate` made over the fixture directory `name`.
fn refusals(name: &str) -> Vec<Rejection> {
    enumerate(&over(name, "lib.rs")).expect_err("this fixture is built to be refused")
}

/// RED — **two files could hold one module, and the walk names both.**
///
/// MUTATION: delete either `x.rs` or `x/mod.rs` and this stops being a refusal,
/// which is the point — the ambiguity is the tree's, not the reader's.
#[test]
fn two_candidate_files_for_one_declaration_are_a_refusal() {
    let found = refusals("ambiguity");
    let Some(Rejection::AmbiguousModule {
        module, candidates, ..
    }) = found.first()
    else {
        panic!("expected an ambiguity, got {found:#?}");
    };
    assert_eq!(module, "x");
    assert_eq!(
        candidates.len(),
        2,
        "both candidates are named: {candidates:#?}"
    );
    assert!(candidates.iter().any(|path| path.ends_with("x.rs")));
    assert!(
        candidates
            .iter()
            .any(|path| path.ends_with(Path::new("x").join("mod.rs")))
    );
}

/// RED — **a declaration that names no file is a refusal, and the places the
/// walk looked are in the message**, because the usual cause is that it looked
/// in the wrong directory.
#[test]
fn a_declaration_that_resolves_to_nothing_is_a_refusal() {
    let found = refusals("unresolved");
    let Some(Rejection::UnresolvedModule { module, tried, .. }) = found.first() else {
        panic!("expected a non-resolution, got {found:#?}");
    };
    assert_eq!(module, "missing");
    assert_eq!(tried.len(), 2, "both spellings were tried: {tried:#?}");
}

/// RED — **a module that reaches itself is a refusal**, and not a walk that
/// never finishes.
#[test]
fn a_module_that_reaches_itself_is_a_refusal() {
    let found = refusals("cycle");
    let Some(Rejection::ModuleCycle { module, file, .. }) = found.first() else {
        panic!("expected a cycle, got {found:#?}");
    };
    assert_eq!(module, "again");
    assert!(file.ends_with("a.rs"));
}

/// RED — **a file the walk reached and cannot read is a refusal**, never a
/// smaller answer. The fixture stages it the one way that behaves the same on
/// Windows and on macOS: the root is a directory that is named like a file.
#[test]
fn a_root_that_cannot_be_read_is_a_refusal() {
    let found = refusals("unreadable");
    let Some(Rejection::UnreadableFile { file, .. }) = found.first() else {
        panic!("expected an unreadable file, got {found:#?}");
    };
    assert!(file.ends_with("lib.rs"));
}

/// RED — **a file that does not parse is a refusal**, because a file whose
/// declarations are unknown is a subtree that is missing without a word.
#[test]
fn a_file_that_does_not_parse_is_a_refusal() {
    let found = refusals("unparsable");
    let Some(Rejection::UnparsableFile { file, .. }) = found.first() else {
        panic!("expected an unparsable file, got {found:#?}");
    };
    assert!(file.ends_with("broken.rs"));
}

/// RED — **a compilation root that is not there is a refusal at declaration
/// time**, before anything is walked. A universe whose root has been renamed
/// would otherwise enumerate nothing and satisfy every negative in the tree.
#[test]
fn a_target_root_that_is_not_there_is_a_refusal() {
    let directory = fixture("ambiguity");
    let declared = Universe::declare(
        "a root that moved",
        vec![TargetRoot {
            id: TargetId {
                package: "gone".to_owned(),
                kind: TargetKind::Binary,
                name: "gone".to_owned(),
            },
            file: directory.join("not-here.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    );
    assert!(matches!(declared, Err(Rejection::MissingTargetRoot { .. })));
}

/// RED — **a disk scope that is not a directory is a refusal.** A scope that
/// walks nothing is how a text guard reads an empty universe and passes.
#[test]
fn a_disk_scope_that_is_not_there_is_a_refusal() {
    let universe = Universe::declare(
        "a scope that moved",
        Vec::new(),
        vec![DiskScope::under(fixture("no-such-directory"))],
        Vendor::Excluded,
    )
    .expect("the scope is checked when it is walked, not when it is named");
    let found = enumerate(&universe).expect_err("the scope is not a directory");
    assert!(matches!(
        found.first(),
        Some(Rejection::MissingDiskScope { .. })
    ));
}

/// RED — **reaching into `vendor/` without saying so is a refusal.**
///
/// Upstream code is held to upstream's choices — the workspace lint table
/// refuses to apply this project's lints to it for that reason — so whether a
/// rule covers it is a decision and never a default.
#[test]
fn reaching_into_vendor_without_saying_so_is_a_refusal() {
    let vendored = fixture("vendored").join("vendor").join("upstream");
    let refused = Universe::declare(
        "silently vendored",
        Vec::new(),
        vec![DiskScope::under(&vendored)],
        Vendor::Excluded,
    );
    assert!(matches!(refused, Err(Rejection::VendorNotDeclared { .. })));
    Universe::declare(
        "vendored on purpose",
        Vec::new(),
        vec![DiskScope::under(&vendored)],
        Vendor::Included,
    )
    .expect("said out loud, it is allowed");
}

/// RED — **a manifest written in a shape this reader does not read is a
/// refusal**, not a workspace with no members.
#[test]
fn a_manifest_this_reader_cannot_read_is_a_refusal() {
    let refused = Workspace::read(&fixture("manifests").join("unclosed"));
    assert!(matches!(refused, Err(Rejection::Manifest { .. })));
}

/// RED — **a universe that names a package the workspace does not have is a
/// refusal**, because the alternative is an empty file set that satisfies every
/// negative asked of it.
#[test]
fn a_package_the_workspace_does_not_have_is_a_refusal() {
    let workspace =
        Workspace::read(&fixture("manifests").join("tiny")).expect("a two-line workspace");
    assert_eq!(workspace.packages().len(), 1);
    assert_eq!(workspace.packages()[0].name(), "one");
    assert!(matches!(
        workspace.package("nope"),
        Err(Rejection::NoSuchPackage { .. })
    ));
}
