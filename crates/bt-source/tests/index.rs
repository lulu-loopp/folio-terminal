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
//!    universe and a different one for a different universe — and the
//!    one-call entry for a package shares that object rather than lowering a
//!    universe of its own.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_source::{
    DiskScope, Index, ItemKind, ItemQuery, Needle, Package, Pattern, QueryFailure, Scope, Search,
    Span, TargetId, TargetKind, TargetRoot, Universe, Vendor, View, Workspace, report, universes,
};
use syn::parse::Parser as _;

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

    let mut by_kind: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for item in index.items() {
        *by_kind.entry(format!("{:?}", item.kind())).or_default() += 1;
    }
    println!("bt-app items by kind: {by_kind:?}");

    // **What §2.3 adds to §2.4, counted on the real tree.** The second number is
    // the items whose own file writes a gate on them; the first is the items
    // that stand on one at all, the declarations that reach their file
    // included. The difference is what an identity built from a file's own text
    // could not see.
    let out_of_the_product = index
        .items()
        .iter()
        .filter(|item| !item.identities().any(|it| it.variant.permits_product()))
        .count();
    let written_in_the_file = index
        .items()
        .iter()
        .filter(|item| !item.variant().permits_product())
        .count();
    println!(
        "bt-app items no product build contains: {out_of_the_product}, of which \
         {written_in_the_file} say so in their own file"
    );
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
        // A method is parsed as a method, a trait's own declaration as one, a
        // free function and a data type as items, a field as a field and a
        // variant as a variant: the shapes are not interchangeable, and asking
        // for the wrong one would be a round trip that proved nothing.
        let parsed_name = match item.kind() {
            ItemKind::AssociatedFunction if item.type_owner().is_some() => {
                syn::parse_str::<syn::ImplItemFn>(text)
                    .ok()
                    .map(|parsed| parsed.sig.ident.to_string())
            }
            ItemKind::AssociatedFunction => syn::parse_str::<syn::TraitItemFn>(text)
                .ok()
                .map(|parsed| parsed.sig.ident.to_string()),
            ItemKind::Function | ItemKind::Struct | ItemKind::Enum | ItemKind::Union => {
                syn::parse_str::<syn::Item>(text)
                    .ok()
                    .and_then(|parsed| match parsed {
                        syn::Item::Fn(function) => Some(function.sig.ident.to_string()),
                        syn::Item::Struct(structure) => Some(structure.ident.to_string()),
                        syn::Item::Enum(enumeration) => Some(enumeration.ident.to_string()),
                        syn::Item::Union(union) => Some(union.ident.to_string()),
                        _ => None,
                    })
            }
            // A tuple field's name is its position, which the text it is
            // written as does not carry; that the slice parses **as an unnamed
            // field** is the whole of what the span can be held to, and the
            // named case carries its name like everything else.
            ItemKind::Field => syn::Field::parse_named
                .parse_str(text)
                .ok()
                .and_then(|field| field.ident.map(|name| name.to_string()))
                .or_else(|| {
                    syn::Field::parse_unnamed
                        .parse_str(text)
                        .ok()
                        .map(|_| item.name().to_owned())
                }),
            ItemKind::Variant => syn::parse_str::<syn::Variant>(text)
                .ok()
                .map(|variant| variant.ident.to_string()),
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

/// RED — **one call names a package and gets the index the long way builds**,
/// the same object and not a second lowering of the same files.
///
/// `Index::of_package` is the fifteen lines every consumer batch would otherwise
/// write out — read the workspace, find the member, declare
/// `universes::crate_sources`, lower it — and the claim that matters is that it
/// is those lines and not a universe of its own: a second universe would answer
/// the same questions from a second 23 MB index, and the two would drift the
/// day one of them was changed.
///
/// MUTATION: declare `whole_package` instead of `crate_sources` inside the
/// entry, or build rather than share, and the pointers differ.
#[test]
fn one_call_names_a_package_and_shares_the_index_the_long_way_builds() {
    let entry = Index::of_package("bt-app");
    let long_way = bt_app_index(&workspace());
    assert!(
        std::ptr::eq(entry, Arc::as_ptr(&long_way)),
        "the entry lowered a universe of its own: it holds {} file(s) against the long way's {}",
        entry.files().len(),
        long_way.files().len()
    );
    assert!(
        std::ptr::eq(entry, Index::of_package("bt-app")),
        "two asks for one package are one answer"
    );
    assert_eq!(entry.universe().name(), "bt-app's own sources");
}

/// RED — **a package the workspace does not have is loud**, not an index of no
/// files.
///
/// The one refusal a consumer can reach by writing the wrong word, and the
/// answer to it is [`bt_source::Rejection::NoSuchPackage`]'s own sentence.
#[test]
#[should_panic(expected = "is not a package of this workspace")]
fn a_package_this_workspace_does_not_have_is_loud() {
    let _ = Index::of_package("bt-nothing-is-called-this");
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

// ── what the declaration that reaches a file stands on ────────────────────

fn declaration_variant_fixture() -> Arc<Index> {
    Index::shared(&fixture_universe("declaration_variant")).expect("the fixture lowers")
}

/// Every identity the items called `name` answer to, printed.
fn identities_of(index: &Index, name: &str) -> Vec<String> {
    index
        .items()
        .iter()
        .filter(|record| record.name() == name)
        .flat_map(|record| record.identities())
        .map(|identity| identity.to_string())
        .collect()
}

/// RED — **a `#[cfg]` written on the declaration that reaches a file stands on
/// every item in that file**, exactly as one written on an inline `mod` does
/// (§2.3 into §2.4).
///
/// `#[cfg(test)] mod t;` and `#[cfg(test)] mod t { … }` are the same statement
/// written two ways. The inline spelling pushes its predicates onto the walk's
/// own stack and always has; the out-of-line one writes nothing in `t.rs` at
/// all, so an identity built from that file's text alone called every item in it
/// unconditional — and the difference was invisible at the call site, which is
/// what made it worth a ticket rather than a note.
///
/// MUTATION: build the identities from the file's text alone and the three
/// gated rows lose their `#[cfg(test)]` while the inline row keeps it — which
/// is the tree as it read before this.
#[test]
fn a_declaration_that_reaches_a_file_stands_on_every_item_in_it() {
    let index = declaration_variant_fixture();

    assert_eq!(
        identities_of(&index, "reached_by_a_gate"),
        ["crate::gate::reached_by_a_gate #[cfg(test)]"],
        "`#[cfg(test)] mod gate;` is written in lib.rs and stands on gate.rs"
    );
    assert_eq!(
        identities_of(&index, "reached_by_a_path"),
        ["crate::by_path::reached_by_a_path #[cfg(test)]"],
        "a `#[path]` declaration carries its gate the same way"
    );
    assert_eq!(
        identities_of(&index, "inside_the_braces"),
        ["crate::inline_gate::inside_the_braces #[cfg(test)]"],
        "the inline spelling of the same statement"
    );
    assert_eq!(
        identities_of(&index, "at_the_root"),
        ["crate::at_the_root"],
        "nothing stands on the crate root"
    );

    // **The two halves are kept apart and joined in one order**: what the
    // declarations outside the file write, then what the file writes itself.
    assert_eq!(
        identities_of(&index, "gated_twice"),
        ["crate::gate::gated_twice #[cfg(test)]#[cfg(windows)]"]
    );
    let gated_twice = index
        .one(&ItemQuery::function("gated_twice").in_module("crate::gate"))
        .unwrap_or_else(|failure| panic!("{failure}"));
    assert_eq!(
        gated_twice.variant().predicates(),
        ["windows"],
        "the record's own variant is the half written inside the file, which is \
         what `in_variant` narrows by"
    );
    assert_eq!(
        gated_twice.declaration_paths()[0].predicates(),
        ["test"],
        "and the other half is on the path that reaches it"
    );
}

/// RED — **a file reached two ways answers to two identities, and the gate is
/// on one of them** (§2.3: reachability is a property of a path to the byte).
///
/// `plain.rs` is declared unconditionally by `lib.rs` and again, through the
/// test gate, by a `#[path]` in `gate.rs`. A reading that put the gate on the
/// bytes rather than on the path would take a product file out of every guard
/// that skips test code the moment a test module happened to name it.
///
/// MUTATION: fold the paths' predicates together and the first row grows a
/// `#[cfg(test)]` it has no business carrying.
#[test]
fn a_file_reached_two_ways_carries_the_gate_on_the_gated_path_only() {
    let index = declaration_variant_fixture();
    assert_eq!(
        identities_of(&index, "reached_two_ways"),
        [
            "crate::gate::plain_again::reached_two_ways #[cfg(test)]",
            "crate::plain::reached_two_ways",
        ],
        "one set of bytes, one identity per path to them"
    );

    let record = index
        .one(&ItemQuery::function("reached_two_ways"))
        .unwrap_or_else(|failure| panic!("{failure}"));
    assert_eq!(
        record.module_paths(),
        ["crate::gate::plain_again", "crate::plain"]
    );
    assert!(
        record.variant().is_unconditional(),
        "the file itself writes nothing on it"
    );

    // **What the predicates say, three-valued and read once** (§2.3). The gated
    // path is out of the product; the other is in it; and the file permits
    // product compilation because one path does.
    let permits: Vec<bool> = record
        .identities()
        .map(|identity| identity.variant.permits_product())
        .collect();
    assert_eq!(permits, [false, true]);
    assert!(index.file_of(record).permits_product());

    // And a gate that is not about `test` is not a gate: `#[cfg(windows)]`
    // leaves the door open, which is the answer the evaluator gives and a
    // comparison against the spelling `"test"` cannot.
    let gated_twice = index
        .one(&ItemQuery::function("gated_twice").in_module("crate::gate"))
        .unwrap_or_else(|failure| panic!("{failure}"));
    assert!(gated_twice.variant().permits_product());
    assert!(
        !gated_twice
            .identities()
            .any(|identity| identity.variant.permits_product()),
        "the whole arm is `test` and `windows`, and `test` closes it"
    );
}

/// RED — **"how many times does the *product* do this" is one predicate the
/// crate owns**, and it takes both grains to answer.
///
/// Six copies of this rule were written out by hand in `bt-app`, four of them
/// word for word, and each of the two shapes was wrong in a way the other was
/// not: the file-grained one counted an inline `#[cfg(test)] mod` and a
/// `#[cfg(test)]` function as product, and the item-grained one counted a file
/// reached by `#[cfg(test)] mod x;` as product. The fixture writes one needle
/// once in each of those places, plus the case neither grain alone can answer —
/// bytes standing in no item at all.
///
/// MUTATION: drop the file half and the two rows in `crate::gate` come back;
/// drop the item half and `only_in_tests` and `inside_the_braces` do; read the
/// item's own file text instead of its identity and `reached_by_a_gate`
/// does — which is the tree as it read before the commit before this one.
#[test]
fn what_a_product_build_contains_is_answered_at_the_file_and_at_the_item() {
    let index = declaration_variant_fixture();
    let counted = |scope: Scope| {
        let found = index
            .search(
                &Search::new(
                    Needle::new(Pattern::text("the_needle_this_fixture_counts")),
                    View::Raw,
                )
                .in_scope(scope),
            )
            .unwrap_or_else(|failure| panic!("{failure}"));
        (found.len(), found.in_the_product(&index).len())
    };

    assert_eq!(counted(Scope::Everything), (7, 3));
    assert_eq!(
        counted(Scope::Item(ItemQuery::function("at_the_root"))),
        (1, 1),
        "product bytes in a product file"
    );
    assert_eq!(
        counted(Scope::Item(ItemQuery::function("only_in_tests"))),
        (1, 0),
        "a `#[cfg(test)]` function in a file a product build compiles"
    );
    assert_eq!(
        counted(Scope::Item(
            ItemQuery::function("inside_the_braces").in_module("crate::inline_gate")
        )),
        (1, 0),
        "an inline `#[cfg(test)] mod`, which is not a file and does not move \
         the file's answer"
    );
    assert_eq!(
        counted(Scope::Item(ItemQuery::function("reached_by_a_gate"))),
        (1, 0),
        "a file reached by `#[cfg(test)] mod gate;`, which writes no gate of \
         its own anywhere in it"
    );
    assert_eq!(
        counted(Scope::Module("crate::gate".to_owned())),
        (2, 0),
        "and the bytes of that file that stand in no item at all, which only \
         the file grain can answer for"
    );
    assert_eq!(
        counted(Scope::Module("crate".to_owned())),
        (4, 2),
        "lib.rs: the `const` and the free function, and neither of the two \
         gated items beside them"
    );
    assert_eq!(
        counted(Scope::Item(ItemQuery::function("reached_two_ways"))),
        (1, 1),
        "a file reached both ways is product code through the path that is \
         (§2.3), and so is an item in it"
    );

    // The narrowed answer is a `Found` and goes on answering as one: the owners
    // of §4.1, what was excluded, where the needle came from, and a report that
    // says which question it is the answer to.
    let found = index
        .search(&Search::new(
            Needle::new(Pattern::text("the_needle_this_fixture_counts")),
            View::Raw,
        ))
        .expect("a spelling of the fixture");
    assert_eq!(found.outside_items(&index), 2, "the two `const`s");
    let product = found.in_the_product(&index);
    assert_eq!(product.outside_items(&index), 1, "one of them is product");
    let mut names: Vec<String> = product
        .owners(&index)
        .into_keys()
        .map(|identity| identity.name)
        .collect();
    names.sort();
    names.dedup();
    assert_eq!(names, ["at_the_root", "reached_two_ways"]);
    assert!(
        product.report(&index).contains("in the product"),
        "{}",
        product.report(&index)
    );
}

// ── every impl of one type ────────────────────────────────────────────────

fn impls_fixture() -> Arc<Index> {
    Index::shared(&fixture_universe("impls")).expect("the fixture lowers")
}

/// RED — **a scope over every `impl` of a type reads that type's blocks,
/// wherever they are written and whatever arm they stand on** — and reads
/// nothing else.
///
/// `journeys_tests` holds two prohibitions of this shape — "no `impl` of
/// `PaneMotion` declares a renewable deadline" — and both settle for
/// `Scope::Module("crate")`, with a note saying a scope over a type's blocks is
/// what they mean and that this crate has none. A module is right only while
/// every block stays in it, which is the binding Step 2a breaks, and it is too
/// wide meanwhile: the needle a prohibition forbids is a spelling other types
/// legitimately carry, so the free function below would invert the guard.
///
/// MUTATION: resolve the scope to the whole of each block's file and both
/// scoped rows become five; take the trait blocks out and the first becomes
/// three; filter the `#[cfg]` arm out and it becomes three the other way.
#[test]
fn a_scope_over_a_types_impls_reads_every_block_of_it_and_nothing_else() {
    let index = impls_fixture();
    let counted = |scope: Scope| {
        index
            .search(
                &Search::new(Needle::new(Pattern::text("renewable_deadline")), View::Raw)
                    .in_scope(scope),
            )
            .unwrap_or_else(|failure| panic!("{failure}"))
            .len()
    };

    assert_eq!(
        counted(Scope::Everything),
        5,
        "four blocks and the free function"
    );
    assert_eq!(
        counted(Scope::Impls("Gate".to_owned())),
        4,
        "the two inherent blocks, the trait one, and the `#[cfg(windows)]` arm \
         — and not the free function, which is the occurrence that would \
         invert the prohibition"
    );
    assert_eq!(
        counted(Scope::Module("crate".to_owned())),
        4,
        "the module the blocks happen to be written in today answers about \
         `second.rs` not at all, and about the free function as though it were \
         one of them"
    );

    // The blocks the scope is built from: one record each, in union order,
    // named by the type and not by the path a file spells it with.
    let blocks: Vec<(&str, Option<&str>, String)> = index
        .impls()
        .iter()
        .map(|block| {
            (
                block.type_owner(),
                block.trait_name(),
                block.variant().to_string(),
            )
        })
        .collect();
    assert_eq!(
        blocks,
        [
            ("Gate", None, String::new()),
            ("Gate", Some("fmt::Display"), String::new()),
            ("Gate", None, "#[cfg(windows)]".to_owned()),
            ("Gate", None, String::new()),
        ],
        "`impl super::Gate` in the second file is the same owner (§2.4)"
    );
    let body = index.text(index.impls()[0].body());
    assert!(
        body.starts_with('{') && body.ends_with('}') && body.contains("fn open"),
        "a block's body is its braces and what is between them: {body}"
    );

    // **A type with no block at all is a refusal naming it**, because a
    // prohibition over no bytes holds about everything.
    let empty = index
        .search(
            &Search::new(Needle::new(Pattern::text("renewable_deadline")), View::Raw)
                .in_scope(Scope::Impls("Lonely".to_owned())),
        )
        .expect_err("`Lonely` is declared and implemented nowhere");
    let QueryFailure::EmptyScope { scope } = &empty else {
        panic!("{empty}");
    };
    assert_eq!(scope, "every `impl` of `Lonely`");
    assert!(empty.to_string().contains("would answer zero"), "{empty}");

    // And a scope over the type **itself** is unchanged: an item is its own
    // bytes, which is a different question from where its methods are written.
    let seat = |scope: Scope| {
        index
            .search(
                &Search::new(
                    Needle::new(Pattern::identifier("gate_seat")),
                    View::Identifiers,
                )
                .in_scope(scope),
            )
            .unwrap_or_else(|failure| panic!("{failure}"))
            .len()
    };
    assert_eq!(seat(Scope::Everything), 3);
    assert_eq!(
        seat(Scope::Item(ItemQuery::type_item("Gate"))),
        1,
        "the field's declaration, and neither of the two readers of it"
    );
    assert_eq!(
        seat(Scope::Impls("Gate".to_owned())),
        1,
        "and the blocks hold the one inside `open`"
    );
}

// ── one owner, however the move spelled it ────────────────────────────────

fn self_type_fixture() -> Arc<Index> {
    Index::shared(&fixture_universe("self_type")).expect("the fixture lowers")
}

/// RED — **a method that moved into a newly declared submodule, and had its self
/// type qualified on the way, is still the same owner** (§2.4).
///
/// This is the shape every relocation of P2a makes: the block comes out of
/// `main.rs` into `src/runtime/mod.rs`, where `Runtime` is no longer in scope
/// and is written `crate::Runtime<'_>`. Identity is stable across a move, so the
/// pin that named the owner `Runtime` before the move names it after — an owner
/// that carried the qualification would answer zero here, and answering zero
/// about a subject that moved is the failure this crate exists to remove.
///
/// MUTATION: join the whole self-type path in `lower::Parsed::type_owner` and
/// this goes red, with the candidate printed as
/// `crate::runtime::crate::Runtime::file_peek_promotes`.
#[test]
fn a_self_type_qualified_by_a_move_is_the_owner_it_was_before() {
    let index = self_type_fixture();
    let body = index
        .body_of(&ItemQuery::method("Runtime", "file_peek_promotes"))
        .expect("the moved method answers to the owner it had before the move");
    assert_eq!(body.trim(), "{\n        !self.name.is_empty()\n    }");

    let identities: Vec<String> = index
        .one(&ItemQuery::method("Runtime", "file_peek_promotes"))
        .expect("one declaration")
        .identities()
        .map(|identity| identity.to_string())
        .collect();
    assert_eq!(identities, ["crate::runtime::Runtime::file_peek_promotes"]);
}

/// RED — **two files spelling one owner two ways each answer for their own
/// method**, and neither query is refused.
///
/// `impl Runtime<'_>` in `lib.rs` and `impl crate::Runtime<'_>` in
/// `runtime/mod.rs` hold differently named methods; `super::Runtime<'_>` in
/// `peek.rs` is the third spelling the same move writes. One owner, three
/// modules, three answers — the module path is what tells the declarations
/// apart, exactly as §2.4's tuple says.
///
/// MUTATION: keep the qualification in the owner and the first row still
/// answers while the other two refuse — which is how the defect looked: a move
/// took two of three methods out of reach without a red test anywhere near them.
#[test]
fn one_owner_spelled_three_ways_answers_three_times() {
    let index = self_type_fixture();
    let mut modules: Vec<&str> = Vec::new();
    for method in [
        "turn_stays_in_the_root",
        "file_peek_promotes",
        "peek_card_names_its_folder",
    ] {
        let record = index
            .one(&ItemQuery::method("Runtime", method))
            .unwrap_or_else(|failure| panic!("{failure}"));
        assert_eq!(record.type_owner(), Some("Runtime"));
        assert_eq!(record.trait_name(), None);
        modules.push(record.module_paths()[0]);
    }
    assert_eq!(modules, ["crate", "crate::runtime", "crate::peek"]);

    // And the owner is the type, never the path to it: a query that named the
    // move's own spelling is a refusal rather than a second way to ask.
    assert!(
        index
            .find(&ItemQuery::method("crate::Runtime", "file_peek_promotes"))
            .is_err(),
        "`crate::Runtime` is a path and not an owner"
    );
}

/// RED — **the qualified path a refusal prints is well-formed**: the module the
/// item is written in, then the owner, then the name.
///
/// The report is the whole value of a loud refusal, and a reader who is told the
/// candidate is `crate::runtime::crate::Runtime::file_peek_promotes` is told
/// something that is not a Rust path and cannot be looked up. The module path
/// appears once, the owner is a single segment, and no `crate::` is glued inside.
///
/// MUTATION: join the whole self-type path in `lower::Parsed::type_owner` and
/// the doubled segment is back in this message.
#[test]
fn a_ruled_out_candidate_prints_one_module_path_and_one_owner() {
    let index = self_type_fixture();
    let refused = index
        .body_of(&ItemQuery::function("file_peek_promotes"))
        .expect_err("`file_peek_promotes` belongs to a type");
    let QueryFailure::Multiplicity { found, near, .. } = &refused else {
        panic!("{refused}");
    };
    assert!(found.is_empty(), "{refused}");
    let ruled_out: Vec<String> = near
        .iter()
        .map(|candidate| candidate.identity.to_string())
        .collect();
    assert_eq!(ruled_out, ["crate::runtime::Runtime::file_peek_promotes"]);

    let printed = refused.to_string();
    assert!(printed.contains("same name, ruled out"), "{printed}");
    assert!(
        printed.contains("crate::runtime::Runtime::file_peek_promotes"),
        "{printed}"
    );
    assert!(
        !printed.contains("crate::runtime::crate::"),
        "a `crate::` segment is glued inside the candidate path:\n{printed}"
    );
}

// ── types, and the members they carry ─────────────────────────────────────

fn type_members_fixture() -> Arc<Index> {
    Index::shared(&fixture_universe("type_members")).expect("the fixture lowers")
}

/// RED — **a `struct` is an identity, found by its own name wherever it is
/// written** (§2.4, extended past the callables).
///
/// A guard that says "the counter lives on `App`" has to be able to name `App`.
/// Before this, `bt-source` held callables only, so the nearest a reader could
/// get was a spelling somewhere in the package — a reading that stays green when
/// the field moves to another struct, which is the failure this crate exists to
/// remove.
///
/// MUTATION: drop the `Item::Struct` arm from `lower::Parsed::walk` and every
/// row here is a multiplicity of zero.
#[test]
fn a_struct_is_an_identity_found_by_name_in_a_submodule() {
    let index = type_members_fixture();
    let record = index
        .one(&ItemQuery::type_item("App"))
        .unwrap_or_else(|failure| panic!("{failure}"));
    assert_eq!(record.kind(), ItemKind::Struct);
    assert_eq!(record.type_owner(), None, "a type's own name is its name");
    assert_eq!(
        record
            .identities()
            .map(|it| it.to_string())
            .collect::<Vec<_>>(),
        ["crate::panes::App"],
        "the module it is written in is the identity's first component, and the query \
         did not have to know it"
    );

    // The body of a type is the list its members are written in.
    let body = index
        .body_of(&ItemQuery::type_item("App"))
        .expect("a named field list is a body");
    assert!(body.starts_with('{') && body.ends_with('}'), "{body}");
    assert!(body.contains("pane_seat"), "{body}");

    // A unit struct has no list, so it has no body — and its whole text is its
    // declaration, semicolon included.
    assert_eq!(
        index
            .declaration_of(&ItemQuery::type_item("Nothing"))
            .expect("a unit struct is still an item"),
        "pub struct Nothing;"
    );
    assert!(matches!(
        index.body_of(&ItemQuery::type_item("Nothing")),
        Err(QueryFailure::NoBody { .. })
    ));

    // The three kinds are one constructor and three answers.
    for (name, kind) in [
        ("Edge", ItemKind::Enum),
        ("Word", ItemKind::Union),
        ("Wrapper", ItemKind::Struct),
    ] {
        assert_eq!(
            index
                .one(&ItemQuery::type_item(name))
                .unwrap_or_else(|failure| panic!("{failure}"))
                .kind(),
            kind
        );
    }
}

/// RED — **a field's bytes are its declaration and stop before the comma.**
///
/// The comma separates two fields and belongs to neither; a span that swallowed
/// it would make the last field of a list a different shape from every other
/// field, and a pin comparing two of them would be comparing two shapes.
///
/// MUTATION: end the field at the comma in `lower::Parsed::members` and the two
/// `ends_with` rows go red; start it at the type instead of the name and the
/// named rows lose their names.
#[test]
fn a_fields_span_is_its_declaration_and_the_comma_is_not_part_of_it() {
    let index = type_members_fixture();
    assert_eq!(
        index
            .declaration_of(&ItemQuery::field("App", "edge"))
            .expect("a field of App"),
        "pub(crate) edge: Edge",
        "the visibility, the name and the type, and nothing after them"
    );

    // Attributes are part of a declaration for a field exactly as they are for
    // a callable, and a doc comment is an attribute.
    let counter = index
        .declaration_of(&ItemQuery::field("App", "tab_ids"))
        .expect("the counter is a field of App");
    assert!(counter.starts_with("/// **The one counter"), "{counter}");
    assert!(counter.ends_with("pub tab_ids: TabIds"), "{counter}");

    // A tuple field is named by its position, which is the name the language
    // gives it, and there is nothing in front of its type but its visibility.
    assert_eq!(
        index
            .declaration_of(&ItemQuery::field("Wrapper", "0"))
            .expect("the one field of a tuple struct"),
        "pub u32"
    );

    // A field has no body, and asking for one is a refusal rather than the
    // declaration handed back under the wrong name.
    assert!(matches!(
        index.body_of(&ItemQuery::field("App", "edge")),
        Err(QueryFailure::NoBody { .. })
    ));

    // A union's fields are fields.
    assert_eq!(
        index
            .declaration_of(&ItemQuery::field("Word", "bytes"))
            .expect("a union carries fields like a struct"),
        "pub bytes: [u8; 4]"
    );
}

/// RED — **a field and a method of one name are two identities**, told apart by
/// the kind and by nothing else, and both printed as a Rust path a reader can
/// look up.
///
/// MUTATION: leave the kind out of `ItemQuery::selects` and each query finds two
/// declarations and refuses — which is the better half of that mistake; the
/// worse half is a query for the method answering with the field's bytes.
#[test]
fn a_field_and_a_method_of_one_name_are_two_identities() {
    let index = type_members_fixture();
    let field = index
        .one(&ItemQuery::field("App", "tab_ids"))
        .unwrap_or_else(|failure| panic!("{failure}"));
    let method = index
        .one(&ItemQuery::method("App", "tab_ids"))
        .unwrap_or_else(|failure| panic!("{failure}"));
    assert_eq!(field.kind(), ItemKind::Field);
    assert_eq!(method.kind(), ItemKind::AssociatedFunction);
    assert!(
        !field.whole().overlaps(method.whole()),
        "two declarations, and the field is not inside the impl block"
    );
    for record in [field, method] {
        assert_eq!(
            record
                .identities()
                .map(|it| it.to_string())
                .collect::<Vec<_>>(),
            ["crate::panes::App::tab_ids"],
            "the module, the type, the name — the shape §2.4 prints a method in"
        );
    }
}

/// RED — **a member the type does not carry is a refusal naming the members it
/// does**, and a member of a type nobody declares says that instead.
///
/// "Not found" plus the names beside it is a diagnosis; "not found" alone is the
/// answer a guard reading a moved subject gets, and the whole preparation exists
/// to stop that being an answer.
///
/// MUTATION: return an empty `find` instead of the refusal and the first block
/// becomes a multiplicity of zero with nothing to read in it.
#[test]
fn a_member_nobody_declares_names_the_members_the_type_does_carry() {
    let index = type_members_fixture();
    let refused = index
        .declaration_of(&ItemQuery::field("App", "next_tab_id"))
        .expect_err("`App` carries no such field");
    let QueryFailure::Member {
        owner,
        declarations,
        ..
    } = &refused
    else {
        panic!("{refused}");
    };
    assert_eq!(owner, "App");
    assert_eq!(declarations.len(), 1, "{refused}");
    assert!(!declarations[0].carries);
    assert_eq!(
        declarations[0].members,
        ["tab_ids", "edge", "pane_seat"],
        "in the order they are written"
    );
    let printed = refused.to_string();
    assert!(
        printed.contains("carries no field called `next_tab_id`"),
        "{printed}"
    );
    assert!(printed.contains("tab_ids, edge, pane_seat"), "{printed}");

    // A type that is not declared at all is a different diagnosis, and saying
    // "no such field" about it would send the reader looking in the wrong place.
    let no_type = index
        .one(&ItemQuery::field("NoSuchTypeIsDeclared", "anything"))
        .expect_err("the type is not there");
    let QueryFailure::Member { declarations, .. } = &no_type else {
        panic!("{no_type}");
    };
    assert!(declarations.is_empty(), "{no_type}");
    assert!(
        no_type
            .to_string()
            .contains("no `struct`, `enum` or `union` called `NoSuchTypeIsDeclared` is declared"),
        "{no_type}"
    );

    // A variant's own fields are not identities, and the refusal says which
    // members the enum really has rather than pretending there are none.
    let variant_field = index
        .one(&ItemQuery::field("Edge", "inset"))
        .expect_err("a field of a variant is not an identity of this index");
    assert!(
        variant_field.to_string().contains("Top, Bottom, Corner"),
        "{variant_field}"
    );
}

/// RED — **a field carried by one `#[cfg]` arm of a type declared twice is a
/// refusal, not one declaration as expected.**
///
/// This is the quiet one. The reading is `cfg`-blind on purpose (§2.4), so both
/// arms are in the index; a query asking for one declaration and getting one
/// would be told "yes, `Split` has `only_on_windows`" about a type that has it
/// on one platform. The refusal names the arm that carries it and the arm that
/// does not, and the arm can then be pinned on purpose.
///
/// MUTATION: take `Index::member_gap` out and the first block answers with the
/// Windows arm's field and says nothing.
#[test]
fn a_field_carried_by_one_arm_of_a_split_type_names_both_arms() {
    let index = type_members_fixture();
    let refused = index
        .one(&ItemQuery::field("Split", "only_on_windows"))
        .expect_err("one arm of two carries it");
    let QueryFailure::Member { declarations, .. } = &refused else {
        panic!("{refused}");
    };
    assert_eq!(declarations.len(), 2, "{refused}");
    assert_eq!(
        declarations.iter().filter(|site| site.carries).count(),
        1,
        "{refused}"
    );
    let printed = refused.to_string();
    assert!(
        printed.contains("carried by 1 of the 2 declarations of `Split`"),
        "{printed}"
    );
    assert!(
        printed.contains("#[cfg(windows)]") && printed.contains("#[cfg(not(windows))]"),
        "the arms are named in the words they are written in:\n{printed}"
    );

    // Said out loud, the arm is an identity like any other.
    let pinned = index
        .one(&ItemQuery::field("Split", "only_on_windows").in_variant(&["windows"]))
        .unwrap_or_else(|failure| panic!("{failure}"));
    assert_eq!(index.text(pinned.whole()), "pub only_on_windows: u8");

    // The field both arms carry is two declarations, and a query expecting one
    // is refused by the machinery the callables use (§2.4).
    assert!(index.find(&ItemQuery::field("Split", "shared")).is_err());
    assert_eq!(
        index
            .find(&ItemQuery::field("Split", "shared").one_per_variant())
            .expect("one per arm")
            .len(),
        2
    );
}

/// RED — **a variant is a member the way a field is**, down to the span and the
/// refusals, and its body is its own field list or the discriminant it is fixed
/// to.
#[test]
fn a_variant_carries_its_fields_or_its_discriminant_as_its_body() {
    let index = type_members_fixture();
    assert_eq!(
        index.text(
            index
                .one(&ItemQuery::variant("Edge", "Bottom"))
                .unwrap_or_else(|failure| panic!("{failure}"))
                .whole()
        ),
        "Bottom { inset: u8 }",
        "the name and its list, and not the comma after it"
    );
    assert_eq!(
        index
            .body_of(&ItemQuery::variant("Edge", "Corner"))
            .expect("a tuple variant's list is its body"),
        "(u8, u8)"
    );
    assert!(
        matches!(
            index.body_of(&ItemQuery::variant("Edge", "Top")),
            Err(QueryFailure::NoBody { .. })
        ),
        "a fieldless variant fixed to nothing has no body"
    );
    assert_eq!(
        index
            .body_of(&ItemQuery::variant("Numbered", "Third"))
            .expect("the discriminant is what this variant carries"),
        "= 3"
    );

    // A variant asked of a type that has none is the same refusal a missing
    // field is, in the words a variant is asked for in.
    let refused = index
        .one(&ItemQuery::variant("App", "Top"))
        .expect_err("`App` is a struct");
    assert!(
        refused
            .to_string()
            .contains("carries no variant called `Top`"),
        "{refused}"
    );
}

/// RED — **a search scoped to a struct reads that struct's bytes and no
/// others** (§4.1: where the concern is genuinely one item, the scope says so
/// and the reading does not widen).
///
/// This is what a guard about a type's own contents gets instead of a
/// whole-package count: the same needle is written once inside `App` and once
/// outside every type, and the scoped reading sees one of them.
///
/// MUTATION: resolve `Scope::Item` to the file the item is in and both numbers
/// become two.
#[test]
fn a_scope_over_a_struct_reads_its_own_bytes_and_no_others() {
    let index = type_members_fixture();
    let needle = || Needle::new(Pattern::identifier("pane_seat"));
    let everywhere = index
        .search(&Search::new(needle(), View::Identifiers))
        .expect("a name of the fixture");
    assert_eq!(
        everywhere.len(),
        2,
        "the field and the free function:\n{}",
        everywhere.report(&index)
    );
    let inside = index
        .search(
            &Search::new(needle(), View::Identifiers)
                .in_scope(Scope::Item(ItemQuery::type_item("App"))),
        )
        .expect("a struct is a scope");
    assert_eq!(inside.len(), 1, "{}", inside.report(&index));
    assert_eq!(
        inside
            .owners(&index)
            .into_keys()
            .map(|identity| identity.to_string())
            .collect::<Vec<_>>(),
        ["crate::panes::App::pane_seat"],
        "and the occurrence inside a type is owned by the member it declares"
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
