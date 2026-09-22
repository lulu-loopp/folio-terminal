//! The index, proved on a fixture and on the tree it exists for.
//!
//! Four claims, and the first of them is checked by the compiler:
//!
//! 1. **The index is `Send + Sync`, so no parser object is inside it.** This is
//!    not a formality. `proc_macro2::Span` is deliberately neither — in the
//!    fallback build it is an index into a thread-local table — so a `syn` tree
//!    that survived the lowering would take both auto traits away and the
//!    assertion below would not compile. It is the one check that proves the
//!    thing the ticket is about.
//! 2. **The lowering is total**: every file the enumeration reaches has a text
//!    record, and every span in the index lies inside the file it came from.
//! 3. **Spans round-trip**: the union sliced at an item's span re-parses as that
//!    item, with that name.
//! 4. **One index per process per universe**, the same object for the same
//!    universe and a different one for a different universe.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_source::{
    DiskScope, Index, ItemQuery, Package, QueryFailure, Span, TargetId, TargetKind, TargetRoot,
    Universe, Vendor, View, Workspace, report, universes,
};

// ── 1. the compile-time proof ─────────────────────────────────────────────

const fn only_if_send_and_sync<T: Send + Sync>() {}

/// **The assertion is this line**, evaluated at compile time. If `Index` ever
/// holds a `syn::File`, a `proc_macro2::Span` or anything else the parser owns,
/// this file stops compiling — and a test that does not compile cannot be made
/// green by adjusting a number.
const INDEX_IS_SHAREABLE: () = only_if_send_and_sync::<Index>();

/// PIN — the index may be handed to another thread and read from two at once.
///
/// MUTATION: put a `syn::File` in `Index` and the `const` above fails to build.
#[test]
fn the_index_is_send_and_sync_and_therefore_holds_no_parser_object() {
    let () = INDEX_IS_SHAREABLE;
    let index = lowering_fixture();
    let borrowed = Arc::clone(&index);
    let counted = std::thread::spawn(move || borrowed.count_identifier("plain", View::Identifiers))
        .join()
        .expect("the index crossed a thread boundary");
    assert_eq!(counted, index.count_identifier("plain", View::Identifiers));
}

// ── the two universes these tests ask ─────────────────────────────────────

fn fixture_universe(name: &str) -> Universe {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    Universe::declare(
        format!("the {name} fixture"),
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
    .expect("the fixture is there")
}

fn lowering_fixture() -> Arc<Index> {
    Index::shared(&fixture_universe("lowering")).expect("the fixture lowers")
}

fn workspace() -> Workspace {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    Workspace::read(&root).expect("this workspace")
}

fn bt_app(workspace: &Workspace) -> &Package {
    workspace.package("bt-app").expect("bt-app")
}

/// `bt-app`'s own `src/` — the universe §5's measurement is written about.
fn bt_app_index(workspace: &Workspace) -> Arc<Index> {
    let universe =
        universes::crate_sources(bt_app(workspace), Vendor::Excluded).expect("bt-app's own src");
    Index::shared(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

// ── 2. the lowering is total ──────────────────────────────────────────────

/// RED — **every file of the enumeration is in the index, and every span in the
/// index is inside the file it came from.**
///
/// A span that pointed outside its file would be a query answering about the
/// wrong program, and it would do so silently: the slice would still be Rust
/// and the needle would still be found or not found. So the check is not "the
/// numbers look plausible", it is every record, one at a time, against the
/// boundaries recorded for its file.
///
/// MUTATION: drop the union base from one of the three lowerings and this goes
/// red on the first file after the first.
#[test]
fn the_lowering_leaves_nothing_of_the_universe_behind() {
    let workspace = workspace();
    let index = bt_app_index(&workspace);
    let universe =
        universes::crate_sources(bt_app(&workspace), Vendor::Excluded).expect("bt-app's own src");
    let (enumeration, unreached) = bt_source::enumerate(&universe).expect("bt-app enumerates");
    unreached.expect_none("bt-app/src holds no file its own declarations do not reach");

    let declared: Vec<&PathBuf> = enumeration.files().keys().collect();
    let lowered: Vec<&Path> = index
        .files()
        .iter()
        .map(bt_source::FileRecord::path)
        .collect();
    assert_eq!(
        declared.len(),
        lowered.len(),
        "the index holds {} of the enumeration's {} files",
        lowered.len(),
        declared.len()
    );
    for path in declared {
        assert!(
            index.file(path).is_some(),
            "{} is declared and not lowered",
            path.display()
        );
    }

    // The union is the files and nothing between them, in the index's own order.
    let mut expected_start = 0;
    let mut total_lines = 0;
    for file in index.files() {
        assert_eq!(
            file.span().start(),
            expected_start,
            "{} does not begin where the file before it ends",
            file.path().display()
        );
        let on_disk = std::fs::read_to_string(file.path()).expect("the file is still there");
        assert_eq!(
            index.text(file.span()),
            on_disk,
            "the union's slice for {} is not the file",
            file.path().display()
        );
        assert!(
            !file.owners().is_empty(),
            "{} is in the index with no declaration owning it",
            file.path().display()
        );
        expected_start = file.span().end();
        total_lines += file.lines();
    }
    assert_eq!(index.union().len(), expected_start);

    for item in index.items() {
        let file = index.file_of(item);
        assert!(
            item.whole().within(file.span()),
            "{} has a span outside {}",
            item.name(),
            file.path().display()
        );
        if let Some(body) = item.body() {
            assert!(
                body.within(item.whole()),
                "{}'s body escapes it",
                item.name()
            );
        }
        assert!(
            !item.module_paths().is_empty(),
            "{} has no module path to be named by",
            item.name()
        );
    }
    for span in index
        .tokens()
        .iter()
        .map(|token| token.span())
        .chain(index.literals().iter().map(|literal| literal.span()))
        .chain(index.comments().iter().map(|comment| comment.span()))
    {
        assert!(
            inside_one_file(&index, span),
            "a lowered span at {:?} is not inside any one file",
            span
        );
    }

    println!(
        "bt-app: {} files, {} lines, {} bytes, {} items, {} identifier tokens, {} literals, \
         {} comment masks",
        index.files().len(),
        total_lines,
        index.union().len(),
        index.items().len(),
        index.tokens().len(),
        index.literals().len(),
        index.comments().len()
    );
    println!(
        "{} macro invocations and definitions, {} unsupported shapes",
        index.macros().len(),
        index.unsupported_macro_shapes().len()
    );
}

fn inside_one_file(index: &Index, span: Span) -> bool {
    index
        .file_at(span.start())
        .is_some_and(|file| span.within(file.span()))
}

// ── 3. spans round-trip ───────────────────────────────────────────────────

/// RED — **the union sliced at an item's span is that item**, for every item in
/// `bt-app`, re-parsed rather than eyeballed.
///
/// This is the property every migrated body pin rests on: `body_of` hands back a
/// slice of a 23 MB string, and the only thing that makes that slice the method
/// somebody asked for is that the span is right.
///
/// MUTATION: start the span at the `fn` keyword instead of the first attribute
/// and the items carrying a `#[cfg]` still parse, but the ones whose attribute
/// is a doc comment lose it — take the closing brace off instead and every row
/// goes red at once.
#[test]
fn slicing_the_union_at_an_item_yields_that_item() {
    let workspace = workspace();
    let index = bt_app_index(&workspace);
    let mut checked = 0usize;
    for item in index.items() {
        let text = index.text(item.whole());
        // A method is parsed as a method, a trait's own declaration as one, and
        // a free function as an item: the shapes are not interchangeable, and
        // asking for the wrong one would be a round trip that proved nothing.
        let parsed_name = if item.type_owner().is_some() {
            syn::parse_str::<syn::ImplItemFn>(text)
                .ok()
                .map(|parsed| parsed.sig.ident.to_string())
        } else if item.trait_name().is_some() {
            syn::parse_str::<syn::TraitItemFn>(text)
                .ok()
                .map(|parsed| parsed.sig.ident.to_string())
        } else {
            syn::parse_str::<syn::Item>(text)
                .ok()
                .and_then(|parsed| match parsed {
                    syn::Item::Fn(function) => Some(function.sig.ident.to_string()),
                    _ => None,
                })
        };
        let at = index
            .locate(item.whole().start())
            .expect("an item is inside a file");
        assert_eq!(
            parsed_name.as_deref(),
            Some(item.name()),
            "the slice at {at} does not re-parse as `{}`:\n{}",
            item.name(),
            text.chars().take(200).collect::<String>()
        );
        checked += 1;
    }
    println!("{checked} item spans re-parsed as the item they name");
    assert!(checked > 1000, "bt-app has more callables than {checked}");
}

// ── 4. one index per process per universe ─────────────────────────────────

/// RED — the cache hands back **the same object**, and a different universe is a
/// different object.
///
/// MUTATION: key the cache on the universe's name and the two fixtures below
/// still differ; key it on nothing and the second assertion goes red.
#[test]
fn one_universe_is_lowered_once_and_shared() {
    let first = Index::shared(&fixture_universe("lowering")).expect("lowers");
    let again = Index::shared(&fixture_universe("lowering")).expect("lowers");
    assert!(
        Arc::ptr_eq(&first, &again),
        "the same universe was lowered twice"
    );
    let other = Index::shared(&fixture_universe("ownership")).expect("lowers");
    assert!(
        !Arc::ptr_eq(&first, &other),
        "two universes came back as one index"
    );
    assert_eq!(first.universe().name(), "the lowering fixture");
    assert_eq!(other.universe().name(), "the ownership fixture");
}

// ── the queries, on the fixture ───────────────────────────────────────────

/// RED — **a unique query that finds none, or more than it expects, refuses and
/// names every candidate** (§2.4).
///
/// Zero is the dangerous answer and the reason this crate exists: a pin whose
/// subject moved would otherwise read a smaller universe and stay green.
///
/// MUTATION: let `find` return the first candidate when there are two and the
/// second block goes green with the wrong body.
#[test]
fn a_query_that_is_not_answered_exactly_says_so() {
    let index = lowering_fixture();

    let missing = index
        .body_of(&ItemQuery::function("nothing_is_called_this"))
        .expect_err("no such function");
    let QueryFailure::Multiplicity { found, .. } = &missing else {
        panic!("a name nobody declares is a multiplicity of zero: {missing}");
    };
    assert!(found.is_empty());

    // Two arms of one identity, and a query that did not say so is refused.
    let both = index
        .find(&ItemQuery::function("only_one_arm"))
        .expect_err("two declarations, one expected");
    let QueryFailure::Multiplicity { found, .. } = &both else {
        panic!("{both}");
    };
    assert_eq!(found.len(), 2, "{both}");
    let predicates: Vec<String> = found
        .iter()
        .flat_map(|candidate| candidate.identity.variant.predicates().to_vec())
        .collect();
    assert_eq!(predicates, ["windows", "not(windows)"], "{both}");

    // Said out loud, the same query is answered.
    let arms = index
        .find(&ItemQuery::function("only_one_arm").expecting(2))
        .expect("two were expected and two are there");
    assert_eq!(arms.len(), 2);

    // A name that is near and not it: the guard prints what it ruled out.
    let near_miss = index
        .body_of(&ItemQuery::method("Latch", "spare"))
        .expect_err("`spare` belongs to no type");
    assert!(
        near_miss.to_string().contains("same name, ruled out"),
        "{near_miss}"
    );
}

/// RED — **identity is the tuple, not the name**: an inherent method, a trait
/// implementation and a trait's own default are three different things, and a
/// signature with no body says so rather than handing back something else.
#[test]
fn identity_separates_an_inherent_method_from_a_trait_one() {
    let index = lowering_fixture();

    assert_eq!(
        index
            .body_of(&ItemQuery::method("Door", "open"))
            .expect("one inherent `open`")
            .trim(),
        "{\n        self.name.len()\n    }"
    );
    // `Door<'_>` and `Door<'a>` are one type, so both inherent impls answer.
    assert!(index.body_of(&ItemQuery::method("Door", "shut")).is_ok());

    // The implementation and the default are not the same declaration.
    assert!(
        index
            .body_of(&ItemQuery::method("Door", "required"))
            .is_err(),
        "an inherent query must not reach into a trait impl"
    );
    assert!(
        index
            .body_of(&ItemQuery::method("Door", "required").of_trait("Latch"))
            .is_ok()
    );
    let bodyless = index
        .body_of(&ItemQuery::function("required").of_trait("Latch"))
        .expect_err("the trait declares it without one");
    assert!(
        matches!(bodyless, QueryFailure::NoBody { .. }),
        "{bodyless}"
    );
    assert_eq!(
        index
            .body_of(&ItemQuery::function("spare").of_trait("Latch"))
            .expect("the default has a body")
            .trim(),
        "{\n        7\n    }"
    );

    // An inline module is a component of the module path.
    let nested = index
        .find(&ItemQuery::function("nested").in_module("crate::inner"))
        .expect("one `nested`");
    assert_eq!(nested.len(), 1);
    assert!(
        index
            .find(&ItemQuery::function("nested").in_module("crate"))
            .is_err()
    );
}

/// RED — **a file reached two ways answers to two identities**, and the owner
/// set says which declaration paths those are (§2.3).
#[test]
fn owners_of_an_item_are_the_declaration_paths_that_reach_its_file() {
    let index = lowering_fixture();
    let owners = index
        .owners_of(&ItemQuery::function("tail"))
        .expect("one `tail`");
    let paths: Vec<&str> = owners
        .iter()
        .map(|owner| owner.module_path.as_str())
        .collect();
    assert_eq!(paths, ["crate::twin"]);
    assert!(
        owners[0]
            .steps
            .iter()
            .all(|step| step.predicates.is_empty()),
        "nothing conditional stands on the way to twin.rs"
    );
    let identities: Vec<String> = index
        .one(&ItemQuery::function("tail"))
        .expect("one `tail`")
        .identities()
        .map(|identity| identity.to_string())
        .collect();
    assert_eq!(identities, ["crate::twin::tail"]);
}

/// RED — **the four views are four different questions** (§2.1), and the two
/// no-crossing rules of §2.2 hold.
///
/// MUTATION: make `CodeKeepingLiterals` strip literals too and the second row
/// goes red — which is the mutation that would have turned five live
/// prohibitions in this tree quietly green.
#[test]
fn a_view_decides_what_is_there_and_a_match_crosses_nothing() {
    let index = lowering_fixture();

    // A needle that is only ever written in a comment.
    assert!(index.contains("hidden_only_in_a_comment", View::Raw));
    assert!(!index.contains("hidden_only_in_a_comment", View::CodeKeepingLiterals));
    assert!(index.contains("hidden_only_in_a_block", View::Raw));
    assert!(!index.contains("hidden_only_in_a_block", View::CodeKeepingLiterals));

    // An attribute that begins with `doc` is not documentation text: the doc
    // comment above it is masked and the attribute itself is code.
    assert!(index.contains("#[doc(hidden)]", View::CodeKeepingLiterals));
    assert!(index.contains("kept out of the rendered documentation", View::Raw));
    assert!(!index.contains(
        "kept out of the rendered documentation",
        View::CodeKeepingLiterals
    ));

    // A needle inside a string literal survives, because it is code's subject.
    assert!(index.contains("stand_in is not a comment", View::CodeKeepingLiterals));
    assert!(index.contains("stand_in is not a comment", View::LiteralValues));

    // Spelling is not value: `0xFF` is written one way and decodes to another.
    assert!(index.contains("0xFF", View::Raw));
    assert!(index.contains("255", View::LiteralValues));
    assert!(!index.contains("255", View::Raw));

    // A name inside a macro invocation is a name (§2.7's coverage floor).
    assert_eq!(
        index.count_identifier("stand_in_inside_a_macro", View::Identifiers),
        2,
        "the definition and the call inside `println!`"
    );
    // Boundary-checked on both sides: `stand_in` is not `stand_in_inside_a_macro`.
    assert_eq!(index.count_identifier("stand_in", View::Identifiers), 0);
    assert!(index.contains("stand_in", View::Raw));

    // No match may cross a file boundary, even one the union spells out.
    let files = index.files();
    let first = index.text(files[0].span());
    let second = index.text(files[1].span());
    let across = format!(
        "{}{}",
        &first[first.len() - 8..],
        &second[..8.min(second.len())]
    );
    assert!(
        index.union().contains(&across),
        "the fixture's union really does hold these bytes in a row"
    );
    assert!(
        !index.contains(&across, View::Raw),
        "a union must not manufacture an occurrence at a file join"
    );
}
