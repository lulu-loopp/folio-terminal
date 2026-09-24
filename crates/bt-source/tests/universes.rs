//! The four universes of `docs/plans/bt-app-split-prep.md` §3.1, as values.
//!
//! Four different universes are in the tree today and they differ in ways that
//! change what a rule covers. This is the test that says what each one contains
//! — its compilation roots, listed — so that the day one of them is migrated the
//! ticket can show a file-set diff against something written down rather than
//! against a walk nobody re-read.

use std::path::{Path, PathBuf};

use bt_source::{DiskScope, Rejection, TargetKind, Universe, Vendor, Workspace, universes};

fn workspace() -> Workspace {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    Workspace::read(&root).expect("this workspace")
}

/// The roots of `universe`, as `package:kind:name`, sorted.
fn roots(universe: &Universe) -> Vec<String> {
    let mut named: Vec<String> = universe
        .roots()
        .iter()
        .map(|root| root.id.to_string())
        .collect();
    named.sort();
    named
}

/// RED — **the first universe includes `src/bin/` targets**, which is the
/// property a per-crate module graph would have thrown away.
///
/// A library's module graph does not contain its binary targets, and
/// `crates/bt-corpus/src/bin/bt-conpty-width-probe.rs` is the one the plan's
/// §8.1 is written about: it links against the package's normal dependencies and
/// is invisible to a walk that starts at `lib.rs`. It was `bt-pty`'s until P21
/// moved it to the tools crate, and that move is exactly the file-set diff this
/// test exists to make visible — the probe left one package and joined another.
#[test]
fn the_stand_in_universe_reaches_the_binary_targets() {
    let workspace = workspace();
    let universe = universes::stand_in_windows(&workspace).expect("the crates tree");
    let named = roots(&universe);
    assert!(
        named.contains(&"bt-corpus:bin:bt-conpty-width-probe".to_owned()),
        "a `src/bin/` target is part of this universe: {named:#?}"
    );
    assert!(named.contains(&"bt-app:bin:folio".to_owned()));
    assert!(named.contains(&"bt-platform:lib:bt-platform".to_owned()));
    assert!(
        !named.iter().any(|name| name.contains(":test:")),
        "an integration test is not under a `src` component: {named:#?}"
    );
    assert_eq!(universe.vendor(), Vendor::Excluded);
    assert_eq!(universe.disk_scopes().len(), 1);
    assert!(
        universe.disk_scopes()[0].excluded_directories().is_empty(),
        "this walk has no directory exclusions at all, which is what makes it the widest"
    );
    println!("stand_in_windows roots:\n{named:#?}");
}

/// RED — **the second universe excludes `bin`, and so loses exactly the targets
/// the first one keeps.** The two differ by that and by nothing else, which is
/// the difference a ticket migrating either has to show.
#[test]
fn the_quiet_door_universe_stops_at_the_bin_directory() {
    let workspace = workspace();
    let wide = roots(&universes::stand_in_windows(&workspace).expect("the crates tree"));
    let universe = universes::quiet_doors(&workspace).expect("the crates tree");
    let named = roots(&universe);
    assert!(!named.contains(&"bt-corpus:bin:bt-conpty-width-probe".to_owned()));
    assert!(named.contains(&"bt-app:bin:folio".to_owned()), "{named:#?}");
    let lost: Vec<&String> = wide.iter().filter(|name| !named.contains(name)).collect();
    assert!(
        lost.iter().all(|name| name.contains(":bin:")),
        "only binary targets are lost to the `bin` exclusion: {lost:#?}"
    );
    assert_eq!(
        universe.disk_scopes()[0].excluded_directories(),
        ["bin", "target", "tests"],
        "the three, kept sorted so two universes compare by what they exclude"
    );
}

/// RED — **the third universe is the only one that says yes to `vendor/`, and it
/// says so out loud.**
#[test]
fn the_shipped_program_universe_declares_its_vendored_half() {
    let workspace = workspace();
    let universe = universes::shipped_program(&workspace).expect("the crates and vendor trees");
    let named = roots(&universe);
    assert_eq!(universe.vendor(), Vendor::Included);
    assert_eq!(universe.disk_scopes().len(), 2);
    assert!(
        named.contains(&"alacritty_terminal:lib:alacritty_terminal".to_owned()),
        "{named:#?}"
    );
    assert!(named.contains(&"bt-app:bin:folio".to_owned()));
    assert!(
        universe.disk_scopes()[0].required_component().is_none(),
        "no `src` retention, so a `build.rs` beside a crate is in it"
    );

    // The same scopes without the declaration are refused.
    let silent = Universe::declare(
        "the shipped program, unsaid",
        Vec::new(),
        universe.disk_scopes().to_vec(),
        Vendor::Excluded,
    );
    assert!(matches!(silent, Err(Rejection::VendorNotDeclared { .. })));
}

/// RED — **the fourth universe is one crate's own `src/`**, and its roots are
/// that crate's alone.
#[test]
fn the_crate_sources_universe_is_one_package() {
    let workspace = workspace();
    let package = workspace.package("bt-platform").expect("bt-platform");
    let universe = universes::crate_sources(package, Vendor::Excluded).expect("its own src");
    assert_eq!(roots(&universe), ["bt-platform:lib:bt-platform"]);
    assert_eq!(universe.disk_scopes().len(), 1);
    assert!(
        universe.disk_scopes()[0]
            .root()
            .ends_with(Path::new("bt-platform").join("src"))
    );
}

/// RED — **the manifests decide what the targets are, not a list here.**
///
/// The three kinds a source guard can care about are the library, every binary
/// and every integration test, and a package is more than its library: the
/// explicit `[[bin]]` and `[[test]]` tables and cargo's own discovery have to
/// agree about the same files.
#[test]
fn the_targets_come_out_of_the_manifests() {
    let workspace = workspace();
    let app = workspace.package("bt-app").expect("bt-app");
    let files: Vec<PathBuf> = app
        .targets()
        .iter()
        .map(|target| target.file.clone())
        .collect();
    assert!(
        files
            .iter()
            .any(|file| file.ends_with(Path::new("src").join("main.rs"))),
        "the `[[bin]] name = \"folio\"` table and the autodiscovered `src/main.rs` are one \
         target: {files:#?}"
    );
    assert_eq!(
        files
            .iter()
            .filter(|file| file.ends_with("main.rs"))
            .count(),
        1,
        "and it is claimed once"
    );
    assert!(
        app.targets()
            .iter()
            .any(|target| target.id.kind == TargetKind::IntegrationTest),
        "bt-app has an integration test target"
    );

    // `vendor/alacritty_terminal` turns cargo's discovery off and names its
    // targets by hand; the reader has to honour that or it invents targets.
    let upstream = workspace.package("alacritty_terminal").expect("vendored");
    let kinds: Vec<TargetKind> = upstream
        .targets()
        .iter()
        .map(|target| target.id.kind)
        .collect();
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == TargetKind::IntegrationTest)
            .count(),
        1,
        "`autotests = false` and one `[[test]]`: {:#?}",
        upstream.targets()
    );
}

/// RED — **a scope states its own exclusions and its own retention**, and both
/// are readable afterwards, because a ticket has to be able to print the
/// universe it migrated a guard onto.
#[test]
fn a_scope_says_what_it_walks() {
    let scope = DiskScope::under(Path::new("crates"))
        .excluding(&["bin", "tests", "target"])
        .retaining_component("src");
    assert_eq!(scope.root(), Path::new("crates"));
    assert_eq!(scope.excluded_directories(), ["bin", "target", "tests"]);
    assert_eq!(scope.required_component(), Some("src"));
}
