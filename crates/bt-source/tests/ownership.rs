//! Who owns the fact that a file is test code, and where a nested declaration
//! looks for its file.
//!
//! One fixture answers both, because both are properties of the same thing — the
//! *path* from a crate root to a file — and the plan's §2.3 rule is that the
//! classification belongs to that path and not to the file. `tests/fixtures/
//! ownership` is laid out so that every rule has a case and no case needs a
//! second fixture:
//!
//! ```text
//! lib.rs      mod shared;                      product
//!             #[cfg(test)] mod gate;           test, and everything under it
//!             mod outer { mod leaf; }          inline ancestry: outer/leaf.rs
//!             #[cfg(test)] mod inline_tests { mod nested; }
//! gate.rs     #[path = "shared.rs"] mod shared_again;   a second way to shared
//!             mod helper;                      gate/helper.rs, test by descent
//! orphan.rs   declared by nothing              UNREACHED
//! ```

use std::path::{Path, PathBuf};

use bt_source::{
    Compilation, DiskScope, Enumeration, TargetId, TargetKind, TargetRoot, Universe, Vendor,
    enumerate,
};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("ownership")
}

fn enumerated() -> Enumeration {
    let directory = fixture_root();
    let universe = Universe::declare(
        "the ownership fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "ownership".to_owned(),
                kind: TargetKind::Library,
                name: "ownership".to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    enumerate(&universe).expect("the fixture resolves completely")
}

/// The fixture-relative paths of a set of files, with one separator whichever
/// platform is reading.
fn named(paths: impl IntoIterator<Item = PathBuf>) -> Vec<String> {
    let root = fixture_root();
    let mut names: Vec<String> = paths
        .into_iter()
        .map(|path| {
            path.strip_prefix(&root)
                .expect("a file of the fixture")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    names.sort();
    names
}

/// RED — **an inline module is a component of the module path, so the file its
/// child names is in a directory of that name.**
///
/// This is the rule `bt_app::file_reads_source_tests::scan` does not have: it
/// resolves a `mod leaf;` nested inside `mod outer { … }` against the declaring
/// file's own directory, which in this fixture is `lib.rs`'s, and would answer
/// with a file that is not there. In the real tree the shape is
/// `vendor/mitex/tests/cvt.rs`, where sixteen files are reached only through an
/// inline `mod cvt { … }`.
///
/// MUTATION: move `outer/leaf.rs` beside `lib.rs` and the walk refuses it.
#[test]
fn a_nested_declaration_looks_inside_the_inline_module() {
    let enumeration = enumerated();
    let leaf = enumeration
        .file(&fixture_root().join("outer").join("leaf.rs"))
        .expect("the inline module's child is reached");
    let owner = &leaf.owners()[0];
    assert_eq!(owner.module_path, "crate::outer::leaf");
    assert_eq!(owner.steps.len(), 2, "one inline step and one file step");
    assert_eq!(owner.steps[0].module, "outer");
    assert_eq!(owner.steps[1].module, "leaf");
    assert!(
        leaf.permits_product(),
        "nothing on this path is gated on `test`"
    );
}

/// RED — **a file reached through a product declaration and a test one is
/// product code** (plan §2.3).
///
/// Any owning path, not every one. The rule exists because the opposite reading
/// removes a file from every guard that skips test code the moment a test module
/// happens to name it, and a prohibition that stops applying is exactly what
/// this preparation is built to prevent.
///
/// MUTATION: put `#[cfg(test)]` on `mod shared;` in `lib.rs` — the only
/// remaining path is gated and the file becomes wholly test.
#[test]
fn a_file_reached_both_ways_is_still_product_code() {
    let enumeration = enumerated();
    let shared = enumeration
        .file(&fixture_root().join("shared.rs"))
        .expect("shared.rs is reached");
    assert_eq!(shared.owners().len(), 2, "twice, by two different paths");
    let compilations: Vec<Compilation> = shared
        .owners()
        .iter()
        .map(|owner| owner.compilation)
        .collect();
    assert!(compilations.contains(&Compilation::AlwaysInProduct));
    assert!(compilations.contains(&Compilation::NeverInProduct));
    assert!(shared.permits_product(), "one product path is enough");
    assert!(!shared.is_wholly_test());
}

/// RED — **the gate belongs to the declaration and carries transitively**,
/// whether it is written `mod tests { … }` or `mod tests;`.
///
/// `gate/helper.rs` carries no gate of its own and is test code all the same,
/// because there is no build in which `gate.rs` is absent and it is there.
#[test]
fn the_gate_belongs_to_the_declaration_and_carries_down() {
    let enumeration = enumerated();
    assert_eq!(
        named(enumeration.wholly_test_files()),
        ["gate.rs", "gate/helper.rs", "inline_tests/nested.rs"],
        "everything under a test declaration, and nothing else"
    );
    assert_eq!(
        named(enumeration.product_reachable_files()),
        ["lib.rs", "outer/leaf.rs", "shared.rs"]
    );
}

/// RED — **a file the declarations do not reach is reported, not dropped.**
///
/// It is a value and not a failure: the walk did its job and the answer is that
/// `orphan.rs` is not part of this compilation. Which of the two it is — a
/// repair or a file nobody compiles — belongs to whoever reads the diff.
#[test]
fn a_file_no_declaration_reaches_is_reported() {
    let enumeration = enumerated();
    assert_eq!(
        named(enumeration.unreached().iter().cloned()),
        ["orphan.rs"]
    );
    let diff = enumeration.cross_check();
    assert!(
        diff.only_declared.is_empty(),
        "every declared file is on the disk under this scope"
    );
    assert_eq!(diff.in_both.len(), 6);
    assert!(!diff.agrees(), "orphan.rs is the difference");
    assert!(diff.report().contains("UNREACHED"));
}
