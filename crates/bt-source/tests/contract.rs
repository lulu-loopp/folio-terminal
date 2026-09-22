//! **The contract, asserted on the tree it exists for.**
//!
//! A fixture proves a rule; only the real tree proves the rule is the one this
//! workspace is written in. Three things live here, and all three are facts
//! about `main` today that go red the day they stop being true:
//!
//! 1. **The eleven duplicated conditional identities** of §2.4, *regenerated*
//!    from the index and compared with the plan's list — so a twelfth is a red
//!    test and not a surprise in P3.
//! 2. **The macro facts of §2.7**: two `macro_rules!` definitions in `bt-app`,
//!    neither of them constructing an item; no source inclusion, no
//!    `module_path!`, no `compile_error!`; one line-number invocation, in the
//!    item the plan names.
//! 3. **Needle provenance** (§2.6), in both of its cases, with the needles
//!    written in this file: one query whose caller is inside the universe being
//!    read, and one whose caller is outside it — which is the shape of the two
//!    `bt-term` integration tests that read `bt-app`'s source.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use bt_source::{
    DiskScope, Index, ItemRecord, MacroKind, MacroShape, Needle, Package, Pattern, Provenance,
    QueryFailure, Search, Site, Span, Universe, Vendor, View, Why, Workspace, needle, report,
    universes,
};

fn workspace() -> Workspace {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    Workspace::read(&root).expect("this workspace")
}

/// `bt-app`'s own `src/` — the universe §5's measurement is written about, and
/// the one the eleven and the macro facts are facts about.
fn bt_app() -> Arc<Index> {
    let workspace = workspace();
    let package = workspace.package("bt-app").expect("bt-app");
    let universe = universes::crate_sources(package, Vendor::Excluded).expect("bt-app's own src");
    Index::shared(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

/// **This crate itself** — the universe whose files include the one these tests
/// are written in, which is what makes the first provenance case real.
fn bt_source() -> Arc<Index> {
    let workspace = workspace();
    let package: &Package = workspace.package("bt-source").expect("bt-source");
    let scopes = vec![
        DiskScope::under(package.directory().join("src")),
        DiskScope::under(package.directory().join("tests")).excluding(&["fixtures"]),
    ];
    let universe = Universe::declare(
        "the whole of bt-source, fixtures aside",
        package.targets().to_vec(),
        scopes,
        Vendor::Excluded,
    )
    .expect("this crate is where it says it is");
    Index::shared(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

/// The smallest callable whose bytes hold `span`.
fn item_at(index: &Index, span: Span) -> Option<&ItemRecord> {
    index
        .items()
        .iter()
        .filter(|record| span.within(record.whole()))
        .min_by_key(|record| record.whole().len())
}

// ── §2.4 — the eleven, regenerated ────────────────────────────────────────

/// RED — **`bt-app` declares exactly eleven callable identities twice**, and
/// this set is computed from the index rather than copied from the plan.
///
/// "The full module path is unique" is false in this tree, and these are the
/// rows that make it false. A query for any of them by name alone is a refusal
/// (§2.4), which is why P3 needs the list to be a thing that goes red rather
/// than a paragraph somebody read once. `run_probe` is declared four times
/// across two modules, so the key is the module path and never the name.
///
/// MUTATION: add a second `#[cfg(unix)]` arm to any function in `bt-app` and a
/// twelfth row appears here; take `#[cfg(debug_assertions)]` off
/// `panic_selftest_if_due`'s pair and one disappears.
#[test]
fn the_identities_bt_app_declares_twice_are_the_eleven() {
    let index = bt_app();
    let mut by_identity: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for record in index.items() {
        for identity in record.identities() {
            let owner = match (&identity.type_owner, &identity.trait_name) {
                (Some(owner), Some(trait_name)) => format!("<{owner} as {trait_name}>::"),
                (Some(owner), None) => format!("{owner}::"),
                (None, Some(trait_name)) => format!("{trait_name}::"),
                (None, None) => String::new(),
            };
            by_identity
                .entry(format!(
                    "{}::{owner}{}",
                    identity.module_path, identity.name
                ))
                .or_default()
                .push(identity.variant.to_string());
        }
    }
    let duplicated: BTreeMap<String, Vec<String>> = by_identity
        .into_iter()
        .filter(|(_, variants)| variants.len() > 1)
        .collect();
    println!("declared more than once:\n{duplicated:#?}");

    let names: Vec<&str> = duplicated.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        [
            "crate::FolioApp::surface_selftest_if_due",
            "crate::attention_copilot::run_probe",
            "crate::explorer_menu::read_state",
            "crate::files::is_concealed",
            "crate::hang_watch::run_selftest_if_due",
            "crate::panic_selftest_if_due",
            "crate::psreadline::run_probe",
            "crate::shell_integration::installed_powershells",
            "crate::shell_integration::run_profile_probe",
            "crate::wsl::<CurrentUser as Registry>::string",
            "crate::wsl::<CurrentUser as Registry>::subkeys",
        ],
        "the eleven of `docs/plans/bt-app-split-prep.md` §2.4, regenerated — a twelfth is a \
         finding about the tree, never a row added to make this green"
    );
    for (identity, variants) in &duplicated {
        assert_eq!(variants.len(), 2, "{identity} has {} arms", variants.len());
        let distinct: BTreeSet<&String> = variants.iter().collect();
        assert_eq!(
            distinct.len(),
            2,
            "{identity}'s two declarations stand on the same predicate"
        );
    }
}

// ── §2.7 — the macro facts about today's tree ─────────────────────────────

/// RED — **the two `macro_rules!` definitions in `bt-app`, and the shapes the
/// traversal cannot classify.**
///
/// §2.7's claim is that the mechanism outlives 2a, so the facts it rests on are
/// asserted rather than remembered: `psreadline::asset` and
/// `shell_integration::profile_marks::managed_line` are the two definitions,
/// neither constructs an item (so neither can be making a `Runtime` method that
/// this index does not hold), and the only invocation shapes reported are the
/// ones listed below.
///
/// MUTATION: write a `macro_rules!` arm in `bt-app` that expands to a `fn` and
/// the `ItemConstructingArm` assertion goes red; add an `include!` and the
/// source-inclusion one does.
#[test]
fn the_macro_facts_of_this_tree_are_asserted() {
    let index = bt_app();

    let definitions: Vec<&str> = index
        .macros()
        .iter()
        .filter(|record| record.kind() == MacroKind::Definition)
        .map(|record| record.path())
        .collect();
    assert_eq!(
        definitions,
        ["asset", "managed_line"],
        "bt-app has exactly two `macro_rules!` definitions"
    );

    let mut by_shape: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for reported in index.unsupported_macro_shapes() {
        by_shape
            .entry(format!("{:?}", reported.shape))
            .or_default()
            .push(format!(
                "{} — {}",
                index
                    .locate(reported.span.start())
                    .map_or_else(|| "?".to_owned(), |location| location.to_string()),
                reported.spelling.replace('\n', " ")
            ));
    }
    println!("unsupported macro shapes in bt-app:\n{by_shape:#?}");

    for absent in [
        MacroShape::SourceInclusion,
        MacroShape::ModulePath,
        MacroShape::CompileError,
        MacroShape::ItemConstructingArm,
    ] {
        assert!(
            !by_shape.contains_key(&format!("{absent:?}")),
            "bt-app has no {absent:?} today: {by_shape:#?}"
        );
    }

    let line_numbers: Vec<&bt_source::UnsupportedMacroShape> = index
        .unsupported_macro_shapes()
        .iter()
        .filter(|reported| reported.shape == MacroShape::LineNumber)
        .collect();
    assert_eq!(line_numbers.len(), 1, "{line_numbers:#?}");
    let owner = item_at(&index, line_numbers[0].span).expect("it is inside a test");
    assert_eq!(
        owner.name(),
        "done_with_the_powershell_row_off_removes_nothing_and_says_nothing",
        "the single line-number invocation is where §2.7 says it is"
    );
}

/// RED — **a name written inside a macro is in the index**, which is the
/// coverage floor §2.7 refuses to lose, and the coverage `syn`'s own visitor
/// does not have.
#[test]
fn names_inside_macros_are_part_of_the_reading() {
    let index = bt_app();
    let inside: usize = index
        .search(&Search::new(
            Needle::new(Pattern::identifier("MANAGED_LINE")),
            View::Identifiers,
        ))
        .expect("a name of bt-app")
        .counts()
        .1;
    println!("`MANAGED_LINE` occurrences inside macro token trees: {inside}");
    assert!(
        index
            .search(&Search::new(
                Needle::new(Pattern::identifier("assert_eq")),
                View::Identifiers,
            ))
            .expect("a name")
            .len()
            > 1000,
        "the macro invocations themselves are names like any others"
    );
}

// ── §2.6 — where the needle came from ─────────────────────────────────────

/// RED — **a needle built inside the universe being read excludes its own
/// construction, and only that.**
///
/// This test's own source is one of `bt-source`'s files, so the string below is
/// in the universe the query reads: without the exclusion the reader would
/// match itself, which is what the 110 `[..].concat()` halves in this workspace
/// exist to prevent.
///
/// MUTATION: exclude nothing and the count is one; exclude the whole enclosing
/// function and the second block — a genuine occurrence in the same function —
/// disappears with it.
#[test]
fn a_needle_built_inside_the_queried_universe_excludes_its_construction() {
    let index = bt_source();
    let found = index
        .search(&Search::new(
            needle!(Pattern::text("a_spelling_only_this_needle_writes")),
            View::Raw,
        ))
        .expect("the construction is locatable");
    assert!(
        found.is_empty(),
        "the only occurrence is the needle itself:\n{}",
        found.report(&index)
    );
    let excluded = found.excluded();
    assert_eq!(excluded.len(), 1, "{}", found.report(&index));
    assert_eq!(excluded[0].why, Why::NeedleConstruction);
    let Provenance::Excluded { at, file } = found.provenance() else {
        panic!(
            "this file is one of the universe's own: {:?}",
            found.provenance()
        );
    };
    assert!(file.ends_with("contract.rs"), "{}", file.display());
    let construction = index.text(*at);
    assert!(
        construction.starts_with("needle!("),
        "the excluded span is the expression that built it, and nothing wider: {construction}"
    );
    assert!(
        construction.len() < 120,
        "and it is the expression, not the function: {construction}"
    );

    // **The exclusion is the construction, not the function around it.** This
    // second needle is written in the same function and names something this
    // crate really writes; its own construction goes, and the occurrences in
    // `src/` stay.
    let real = index
        .search(&Search::new(
            needle!(Pattern::identifier("ConditionalDeclarationAttribute")),
            View::Identifiers,
        ))
        .expect("the construction is locatable");
    assert!(
        real.len() >= 2,
        "the rejection variant is declared and matched on:\n{}",
        real.report(&index)
    );
}

/// RED — **a needle built outside the universe being read is a recorded answer,
/// not a failure** (§2.6 rule 1).
///
/// This is the shape of `bt-term/tests/shell_integration_cmd.rs` and
/// `shell_integration_wsl.rs`: both join a relative path onto their own
/// manifest directory and read `bt-app`'s source, and their caller file will
/// never resolve into `bt-app`'s enumeration. Resolution goes through the
/// caller's own crate — this file is found through `bt-source`'s manifest
/// directory — and the exclusion then applies to nothing, because the queried
/// source does not contain the site.
///
/// MUTATION: panic when the site is outside and both `bt-term` readers become
/// unmigratable; exclude by file name instead of by site and the occurrences
/// below vanish.
#[test]
fn a_needle_built_outside_the_queried_universe_is_recorded_and_excludes_nothing() {
    let index = bt_app();
    let found = index
        .search(&Search::new(
            needle!(Pattern::identifier("FolioApp")),
            View::Identifiers,
        ))
        .expect("an outside caller is an answer");
    println!("`FolioApp` as a name: {} occurrence(s)", found.len());
    assert!(
        found.len() >= 4,
        "the struct, its two impl blocks and the one place it is built:
{}",
        found.report(&index)
    );
    assert!(found.excluded().is_empty(), "{:?}", found.excluded());
    match found.provenance() {
        Provenance::Outside { file } => {
            assert!(file.ends_with("contract.rs"), "{}", file.display())
        }
        other => panic!("the caller lives in bt-source, not bt-app: {other:?}"),
    }
}

/// RED — **a site inside the universe whose construction cannot be found is a
/// refusal**, because an exclusion that silently excludes nothing is the quiet
/// failure §2.6 exists to prevent.
///
/// The site below is the first line of this file, which is a doc comment and
/// builds no needle.
#[test]
fn a_construction_that_cannot_be_located_inside_the_universe_is_loud() {
    let index = bt_source();
    let site = Site::new(
        "crates/bt-source/tests/contract.rs",
        1,
        1,
        env!("CARGO_MANIFEST_DIR"),
    );
    let failure = index
        .search(&Search::new(
            Needle::at(Pattern::text("anything"), site),
            View::Raw,
        ))
        .expect_err("line 1 of this file builds no needle");
    assert!(
        matches!(failure, QueryFailure::NeedleConstructionLost { .. }),
        "{failure}"
    );
}

/// RED — **the refusal above names the stale test binary**, which is what
/// actually causes it.
///
/// P3's pilot met this failure twice, both times because a comment edited above
/// a `needle!` moved the expression while the test binary went on naming the
/// line `line!()` had been baked with. The message described the other cause
/// only — a needle built outside the file its site names — so the reader's next
/// move was to go looking for a wrong site instead of typing `cargo test`
/// again. A refusal that does not say what to do about it costs the same as no
/// refusal.
///
/// MUTATION: take the rebuild sentence out and this goes red; it is asserted on
/// the message rather than on the variant because the variant was never the
/// thing that was wrong.
#[test]
fn the_lost_construction_refusal_says_to_rebuild_the_test() {
    let said = QueryFailure::NeedleConstructionLost {
        file: std::path::PathBuf::from("crates/bt-app/src/main.rs"),
        line: 114_000,
        column: 29,
    }
    .to_string();
    assert!(
        said.contains("rebuild"),
        "the stale binary is the usual cause and rebuilding is the answer: {said}"
    );
    assert!(
        said.contains("line!()") && said.contains("stale test binary"),
        "the message has to say why the line no longer matches the source: {said}"
    );
    assert!(
        said.contains("outside the file its site names"),
        "and the second cause is still in it: {said}"
    );
    assert!(
        said.contains("main.rs") && said.contains("114000") && said.contains("29"),
        "with the site it was given: {said}"
    );
}

/// RED — **`file!()` that resolves to nothing is a panic**, not an exclusion of
/// nothing.
#[test]
#[should_panic(expected = "`file!()` gave")]
fn a_needle_whose_file_cannot_be_resolved_panics() {
    let _ = Site::new(
        "crates/bt-source/tests/no-such-file-is-here.rs",
        1,
        1,
        env!("CARGO_MANIFEST_DIR"),
    )
    .resolve();
}
