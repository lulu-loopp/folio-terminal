//! **`FunctionPaths` follows a free function's path to an item's identity** (T-PANIC-ROWS-HONEST
//! round 3): the resolution matrix the contained-panic guard stands on, on two fixture crates —
//! `fp_app`, whose modules are one case each, and `fp_platform`, which it names.

use std::collections::BTreeSet;
use std::path::Path;

use bt_source::{
    DiskScope, FunctionItem, FunctionPaths, Index, TargetId, TargetKind, TargetRoot, Universe,
    Vendor, report,
};

/// One fixture crate, `fixtures/function_paths/<directory>`, as the library `name`.
fn crate_of(directory: &str, name: &str) -> Index {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("function_paths")
        .join(directory);
    let universe = Universe::declare(
        format!("the function-paths fixture {name}"),
        vec![TargetRoot {
            id: TargetId {
                package: name.to_owned(),
                kind: TargetKind::Library,
                name: name.to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

/// Where `module`'s `here` marker is written in `app`: the place a path of that case is read at.
fn here(app: &Index, module: &str) -> usize {
    app.items()
        .iter()
        .find(|item| item.name() == "here" && item.module_paths().contains(&module))
        .unwrap_or_else(|| panic!("{module} has its marker"))
        .whole()
        .start()
}

fn item(package: &str, module: &str, name: &str) -> FunctionItem {
    FunctionItem {
        package: package.to_owned(),
        module: module.to_owned(),
        name: name.to_owned(),
    }
}

fn path(spelled: &str) -> Vec<String> {
    spelled.split("::").map(str::to_owned).collect()
}

/// RED (T-PANIC-ROWS-HONEST round 3) — **the resolution matrix**:
///
/// * (a) a cross-crate re-export (`pub(crate) use fp_platform::boundary::contain;`) names the
///   platform's item, read inside the module and through a path to it from the crate root —
///   and not the platform's other `contain`;
/// * (b) a glob brings an unshadowed `f` in;
/// * (c) a local `f` beside the same glob is the local item, not the glob's;
/// * (d) two globs that both bring an `f` answer both — an ambiguous set, never one of them;
/// * (e) a method (`run`), a trait function (`act`), `Thing::run` and `Act::act` name no free
///   function: empty, not a guess;
/// * (f) an unknown path is empty.
///
/// MUTATIONS: ① `functions_into` without its glob step — (b) is empty; ② without its `use`-item
/// step — (a) is empty.
#[test]
fn a_path_is_followed_to_an_item_identity_and_no_further() {
    let app = crate_of("app", "fp_app");
    let platform = crate_of("platform", "fp_platform");
    let paths = FunctionPaths::read(&[&app, &platform]);
    let named = |module: &str, spelled: &str| {
        paths.functions_named(&app, here(&app, module), &path(spelled))
    };
    let none = BTreeSet::new();

    let boundary = BTreeSet::from([item("fp_platform", "crate::boundary", "contain")]);
    assert_eq!(named("crate::reexport", "contain"), boundary, "(a) inside");
    assert_eq!(
        named("crate::methods", "crate::reexport::contain"),
        boundary,
        "(a) through the re-export's path"
    );
    assert_eq!(
        named("crate::methods", "fp_platform::boundary::contain"),
        boundary,
        "(a) the platform's path in full"
    );

    assert_eq!(
        named("crate::globbed", "f"),
        BTreeSet::from([item("fp_app", "crate::provider_one", "f")]),
        "(b)"
    );
    assert_eq!(
        named("crate::shadowed", "f"),
        BTreeSet::from([item("fp_app", "crate::shadowed", "f")]),
        "(c)"
    );
    assert_eq!(
        named("crate::twice", "f"),
        BTreeSet::from([
            item("fp_app", "crate::provider_one", "f"),
            item("fp_app", "crate::provider_two", "f"),
        ]),
        "(d) both, so no single identity"
    );

    for spelled in ["run", "act", "Thing::run", "Act::act"] {
        assert_eq!(named("crate::methods", spelled), none, "(e) {spelled}");
    }

    for spelled in [
        "nowhere",
        "crate::nowhere::f",
        "fp_platform::nowhere::contain",
    ] {
        assert_eq!(named("crate::methods", spelled), none, "(f) {spelled}");
    }
}
