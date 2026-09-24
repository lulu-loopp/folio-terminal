//! The four universes that are already in this tree (plan §3.1).
//!
//! They are here as values so that the readers that own them can be migrated
//! onto the same words they are written in today — and so that the difference
//! between a text walk and a declaration walk is a diff somebody reads rather
//! than a change nobody notices. P1a builds them and wires none of them: the
//! guards named below are untouched, and the tickets that migrate them are P7,
//! P12 and P13.
//!
//! | Guard | Its universe |
//! | --- | --- |
//! | `bt_platform::native_window_door_tests::a_stand_in_window_is_only_named_by_tests` | [`stand_in_windows`] |
//! | `bt_platform::quiet_door_tests::shipped_sources` | [`quiet_doors`] |
//! | `bt_app::diagnostics::bt_environment_doc_tests::shipped_sources` | [`shipped_program`] |
//! | `bt_platform::native_window_door_tests::crate_sources` | [`crate_sources`] |
//!
//! **Each carries its target roots as well as its text walk**, which the guards
//! do not have today because a directory walk has no notion of a compilation.
//! The roots a universe takes are the compilations whose root file its own text
//! walk already covers, so the two readings answer for the same files and the
//! cross-check between them is the evidence §3.2 asks for. It is also how the
//! first of the four keeps covering `src/bin/` targets: a library's module graph
//! does not contain them, and replacing that walk with one would have dropped
//! every binary target in the workspace without a word.

use crate::manifest::{Package, TargetRoot, Workspace};
use crate::reject::Rejection;
use crate::universe::{DiskScope, Universe, Vendor};

/// The directory names the three shipped-source walkers refuse to descend into.
const NOT_SHIPPED: [&str; 3] = ["bin", "tests", "target"];

/// Every target whose root file one of `scopes` already covers.
///
/// # Errors
///
/// Whatever [`DiskScope::files`] rejects.
pub fn roots_within(
    packages: &[Package],
    scopes: &[DiskScope],
) -> Result<Vec<TargetRoot>, Vec<Rejection>> {
    let mut covered = std::collections::BTreeSet::new();
    for scope in scopes {
        covered.extend(scope.files()?);
    }
    let mut roots = Vec::new();
    for package in packages {
        for target in package.targets() {
            if covered.contains(&target.file) {
                roots.push(target.clone());
            }
        }
    }
    roots.sort();
    Ok(roots)
}

/// **The whole workspace `crates/` tree, retained to paths with a `src`
/// component, with no directory exclusions at all** — so `src/bin/` targets are
/// in, which is the property that makes this universe different from the others
/// and the one a per-crate module graph would have thrown away.
///
/// # Errors
///
/// Whatever [`Universe::declare`] and the disk walk reject.
pub fn stand_in_windows(workspace: &Workspace) -> Result<Universe, Vec<Rejection>> {
    let scopes = vec![DiskScope::under(workspace.root().join("crates")).retaining_component("src")];
    let roots = roots_within(workspace.packages(), &scopes)?;
    Universe::declare(
        "a stand-in window is only named by tests",
        roots,
        scopes,
        Vendor::Excluded,
    )
    .map_err(|rejection| vec![rejection])
}

/// `crates/`, retained to `src`, **excluding directories named `bin`, `tests`
/// and `target`** — a development binary is a console program on purpose and an
/// integration test runs under the harness's console, so neither is asked to
/// hold the door this guard is about. Inline test code is deliberately in.
///
/// # Errors
///
/// Whatever [`Universe::declare`] and the disk walk reject.
pub fn quiet_doors(workspace: &Workspace) -> Result<Universe, Vec<Rejection>> {
    let scopes = vec![
        DiskScope::under(workspace.root().join("crates"))
            .excluding(&NOT_SHIPPED)
            .retaining_component("src"),
    ];
    let roots = roots_within(workspace.packages(), &scopes)?;
    Universe::declare("the quiet door", roots, scopes, Vendor::Excluded)
        .map_err(|rejection| vec![rejection])
}

/// **Every `.rs` file that can end up in `folio.exe`** — `crates/` *and*
/// `vendor/`, excluding `bin`, `tests` and `target`, with no `src` retention, so
/// a `build.rs` beside a crate is in it.
///
/// This is the one universe that says yes to vendored code, and it says so out
/// loud: the rule it serves is about what the shipped executable carries, which
/// is not a question about whose code it is.
///
/// # Errors
///
/// Whatever [`Universe::declare`] and the disk walk reject.
pub fn shipped_program(workspace: &Workspace) -> Result<Universe, Vec<Rejection>> {
    let scopes = vec![
        DiskScope::under(workspace.root().join("crates")).excluding(&NOT_SHIPPED),
        DiskScope::under(workspace.root().join("vendor")).excluding(&NOT_SHIPPED),
    ];
    let roots = roots_within(workspace.packages(), &scopes)?;
    Universe::declare(
        "every file that can end up in folio.exe",
        roots,
        scopes,
        Vendor::Included,
    )
    .map_err(|rejection| vec![rejection])
}

/// One package's own `src/`, recursively.
///
/// `vendor` is an argument rather than an inference: asking a vendored package
/// this question is a legitimate thing to want, and whether a reading covers
/// upstream code is the caller's decision every time.
///
/// # Errors
///
/// Whatever [`Universe::declare`] and the disk walk reject.
pub fn crate_sources(package: &Package, vendor: Vendor) -> Result<Universe, Vec<Rejection>> {
    let scopes = vec![DiskScope::under(package.directory().join("src"))];
    let roots = roots_within(std::slice::from_ref(package), &scopes)?;
    Universe::declare(
        format!("{}'s own sources", package.name()),
        roots,
        scopes,
        vendor,
    )
    .map_err(|rejection| vec![rejection])
}

/// One package's whole compilation: every target, and the directories they are
/// written in.
///
/// Not one of the four — it is the universe a reader wants when its question is
/// "what is this crate made of", and the real-tree tests of this crate use it.
///
/// # Errors
///
/// Whatever [`Universe::declare`] and the disk walk reject.
pub fn whole_package(package: &Package, vendor: Vendor) -> Result<Universe, Vec<Rejection>> {
    let mut scopes = vec![DiskScope::under(package.directory().join("src"))];
    let tests = package.directory().join("tests");
    if tests.is_dir() {
        scopes.push(DiskScope::under(tests));
    }
    Universe::declare(
        format!("the whole of {}", package.name()),
        package.targets().to_vec(),
        scopes,
        vendor,
    )
    .map_err(|rejection| vec![rejection])
}
