//! **The contract, asserted on the tree it exists for.**
//!
//! A fixture proves a rule; only the real tree proves the rule is the one this
//! workspace is written in. Three things live here, and all three are facts
//! about `main` today that go red the day they stop being true:
//!
//! 1. **The ten duplicated conditional identities** of §2.4, *regenerated*
//!    from the index and compared with the plan's list — so an eleventh is a red
//!    test and not a surprise in P3.
//! 2. **The macro facts of §2.7**: four `macro_rules!` definitions in
//!    `bt-app`, one of them constructing items — exactly the `Text` enum and
//!    its test list, nothing else in that arm; no source inclusion, no
//!    `module_path!`, no `compile_error!`, and no line-number invocation.
//! 3. **Needle provenance** (§2.6), in both of its cases, with the needles
//!    written in this file: one query whose caller is inside the universe being
//!    read, and one whose caller is outside it — which is the shape of the two
//!    `bt-term` integration tests that read `bt-app`'s source.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use bt_source::{
    DiskScope, Index, MacroKind, MacroShape, ModuleSpec, Needle, Package, Pattern, Provenance,
    QueryFailure, Scope, Search, Site, Universe, Vendor, View, Why, Workspace, needle, report,
    universes,
};

fn workspace() -> Workspace {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    Workspace::read(&root).expect("this workspace")
}

/// `bt-app`'s own `src/` — the universe §5's measurement is written about, and
/// the one the ten and the macro facts are facts about.
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

// ── §2.4 — the ten, regenerated ───────────────────────────────────────────

/// RED — **`bt-app` declares exactly ten callable identities twice**, and
/// this set is computed from the index rather than copied from the plan.
///
/// §2.4 counted eleven. Ticket 72 (2026-09-26) made `explorer_menu::read_state`
/// one portable declaration — its Windows arm already asked nothing the other
/// platforms lack — so the set shrank by that one row, which is the only way
/// it may change without a finding.
///
/// "The full module path is unique" is false in this tree, and these are the
/// rows that make it false. A query for any of them by name alone is a refusal
/// (§2.4), which is why P3 needs the list to be a thing that goes red rather
/// than a paragraph somebody read once. `run_probe` is declared four times
/// across two modules, so the key is the module path and never the name.
///
/// **The kind is part of the key, and has to be.** The index holds types and
/// their members beside the callables, and `Toast::anchor` is a field and a
/// method of one name — two identities, not two arms of one. A key without the
/// kind made every such pair in `bt-app` look like a duplicated identity, which
/// is the reading §2.4 forbids in the other direction: two different things
/// taken for one.
///
/// MUTATION: add a second `#[cfg(unix)]` arm to any function in `bt-app` and an
/// eleventh row appears here; take `#[cfg(debug_assertions)]` off
/// `panic_selftest_if_due`'s pair and one disappears; drop the kind from the
/// key and the field/method pairs flood the first list.
#[test]
fn the_identities_bt_app_declares_twice_are_the_ten() {
    let index = bt_app();
    let mut by_identity: BTreeMap<(&'static str, String), Vec<String>> = BTreeMap::new();
    for record in index.items() {
        let sort = if record.kind().is_callable() {
            "callable"
        } else if record.kind().is_type() {
            "type"
        } else {
            "member"
        };
        for identity in record.identities() {
            let owner = match (&identity.type_owner, &identity.trait_name) {
                (Some(owner), Some(trait_name)) => format!("<{owner} as {trait_name}>::"),
                (Some(owner), None) => format!("{owner}::"),
                (None, Some(trait_name)) => format!("{trait_name}::"),
                (None, None) => String::new(),
            };
            by_identity
                .entry((
                    sort,
                    format!("{}::{owner}{}", identity.module_path, identity.name),
                ))
                .or_default()
                .push(identity.variant.to_string());
        }
    }
    let duplicated: BTreeMap<(&str, String), Vec<String>> = by_identity
        .into_iter()
        .filter(|(_, variants)| variants.len() > 1)
        .collect();
    println!("declared more than once:\n{duplicated:#?}");

    let callables: Vec<&str> = duplicated
        .keys()
        .filter(|(sort, _)| *sort == "callable")
        .map(|(_, path)| path.as_str())
        .collect();
    assert_eq!(
        callables,
        [
            "crate::FolioApp::surface_selftest_if_due",
            "crate::attention_copilot::run_probe",
            "crate::files::is_concealed",
            "crate::hang_watch::run_selftest_if_due",
            "crate::panic_selftest_if_due",
            "crate::psreadline::run_probe",
            "crate::shell_integration::installed_powershells",
            "crate::shell_integration::run_profile_path_probe",
            "crate::wsl::<CurrentUser as Registry>::string",
            "crate::wsl::<CurrentUser as Registry>::subkeys",
        ],
        "the ten of `docs/plans/bt-app-split-prep.md` §2.4 (eleven until ticket 72), \
         regenerated — an eleventh is a finding about the tree, never a row added to make \
         this green"
    );

    // The same claim for the data the program keeps, whose identities arrived
    // with the struct and field ticket. `bt-app` declares none of them twice:
    // the day it does, the row below is a finding about the tree in exactly the
    // way an eleventh callable would be.
    let data: Vec<String> = duplicated
        .keys()
        .filter(|(sort, _)| *sort != "callable")
        .map(|(sort, path)| format!("{sort} {path}"))
        .collect();
    assert_eq!(
        data,
        Vec::<String>::new(),
        "no type and no field of `bt-app` is declared under two conditions"
    );

    for (identity, variants) in &duplicated {
        assert_eq!(
            variants.len(),
            2,
            "{identity:?} has {} arms",
            variants.len()
        );
        let distinct: BTreeSet<&String> = variants.iter().collect();
        assert_eq!(
            distinct.len(),
            2,
            "{identity:?}'s two declarations stand on the same predicate"
        );
    }
}

// ── §2.7 — the macro facts about today's tree ─────────────────────────────

/// RED — **the five `macro_rules!` definitions in `bt-app`, and the shapes the
/// traversal cannot classify.**
///
/// §2.7's claim is that the mechanism outlives 2a, so the facts it rests on are
/// asserted rather than remembered: `i18n::text_entries` constructs exactly
/// the `Text` enum and an `impl Text` holding only its test list `ALL` (checked
/// token by token in [`the_text_declaration_and_nothing_else`]), while
/// `marks::folder_body`, `psreadline::asset`, pointer capture's
/// expression-only `gesture_door`, and
/// `shell_integration::profile_marks::managed_line` construct no item — so no
/// macro can be making a `Runtime` method that this index does not hold; the
/// only invocation shapes reported are the ones listed below.
///
/// MUTATION: write a `macro_rules!` arm in `bt-app` that expands to a `fn` and
/// the `ItemConstructingArm` assertion goes red; add an `impl Runtime { … }` (or
/// a `fn` inside `impl Text`) to `text_entries!`'s own arm and the exception's
/// check does; add an `include!` and the source-inclusion one does.
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
        [
            "text_entries",
            "folder_body",
            "asset",
            "gesture_door",
            "managed_line"
        ],
        "bt-app has exactly the five named `macro_rules!` definitions"
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
        MacroShape::LineNumber,
    ] {
        assert!(
            !by_shape.contains_key(&format!("{absent:?}")),
            "bt-app has no {absent:?} today: {by_shape:#?}"
        );
    }

    let item_arms: Vec<&bt_source::UnsupportedMacroShape> = index
        .unsupported_macro_shapes()
        .iter()
        .filter(|reported| reported.shape == MacroShape::ItemConstructingArm)
        .collect();
    assert_eq!(
        item_arms.len(),
        1,
        "`text_entries!` is the one item-constructing arm: {:#?}",
        by_shape.get(&format!("{:?}", MacroShape::ItemConstructingArm))
    );
    if let Err(why) = the_text_declaration_and_nothing_else(&item_arms[0].spelling) {
        panic!(
            "the one admitted item-constructing arm emits more than the `Text` enum and its \
             test list ({why}):\n{}",
            item_arms[0].spelling
        );
    }
}

/// `Ok` when `arm` — an item-constructing arm's expansion as the index spells
/// it, braces included — emits exactly two items: `pub enum Text { … }` and an
/// `impl Text { … }` whose body is one item, the `ALL` constant.
///
/// **This is the whole of the exception.** `ItemConstructingArm` is reported
/// because the item a macro makes is not in the index, so a query for it
/// answers "not declared" and the census cannot see its writes. The `Text` enum
/// and its test list are admitted by shape: another item written beside them in
/// the same arm is an item the index does not hold, and it is refused here as it
/// would be in an arm of its own.
fn the_text_declaration_and_nothing_else(arm: &str) -> Result<(), String> {
    use proc_macro2::{Delimiter, TokenStream, TokenTree};
    use std::str::FromStr;

    /// One item at the top level of a token stream: the tokens before its body,
    /// and its brace body if it has one. Attributes — doc comments included,
    /// which the lexer turns into `#[doc = …]` — are skipped.
    struct Item {
        head: Vec<String>,
        body: Option<TokenStream>,
    }

    fn items(stream: TokenStream) -> Result<Vec<Item>, String> {
        let trees: Vec<TokenTree> = stream.into_iter().collect();
        let mut found = Vec::new();
        let mut at = 0;
        while at < trees.len() {
            if matches!(&trees[at], TokenTree::Punct(hash) if hash.as_char() == '#') {
                match trees.get(at + 1) {
                    Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Bracket => {
                        at += 2;
                        continue;
                    }
                    _ => return Err("a `#` that opens no attribute".to_owned()),
                }
            }
            let mut head = Vec::new();
            let mut body = None;
            while at < trees.len() {
                let tree = &trees[at];
                at += 1;
                match tree {
                    TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => {
                        body = Some(group.stream());
                        break;
                    }
                    TokenTree::Punct(semicolon) if semicolon.as_char() == ';' => break,
                    other => head.push(other.to_string()),
                }
            }
            found.push(Item { head, body });
        }
        Ok(found)
    }

    let stream = TokenStream::from_str(arm).map_err(|error| format!("it does not lex: {error}"))?;
    let trees: Vec<TokenTree> = stream.into_iter().collect();
    let inside = match trees.as_slice() {
        [TokenTree::Group(group)] if group.delimiter() == Delimiter::Brace => group.stream(),
        _ => return Err("it is not one braced expansion".to_owned()),
    };
    let emitted = items(inside)?;
    let heads: Vec<String> = emitted.iter().map(|item| item.head.join(" ")).collect();
    let [enumeration, list] = emitted.as_slice() else {
        return Err(format!("it emits {} items: {heads:?}", emitted.len()));
    };
    if enumeration.head != ["pub", "enum", "Text"] || enumeration.body.is_none() {
        return Err(format!(
            "its first item is not `pub enum Text {{ … }}`: {heads:?}"
        ));
    }
    if list.head != ["impl", "Text"] {
        return Err(format!(
            "its second item is not `impl Text {{ … }}`: {heads:?}"
        ));
    }
    let members = items(list.body.clone().unwrap_or_default())?;
    let member_heads: Vec<String> = members.iter().map(|item| item.head.join(" ")).collect();
    match members.as_slice() {
        [constant]
            if constant.body.is_none()
                && constant.head.len() >= 3
                && constant.head[..3] == ["pub", "const", "ALL"] =>
        {
            Ok(())
        }
        _ => Err(format!(
            "`impl Text` holds more than the `ALL` list: {member_heads:?}"
        )),
    }
}

/// RED (T-GATES-047) — **an item planted inside the admitted arm is refused**,
/// beside the `Text` enum or inside its `impl`.
///
/// The exception for `text_entries!` is a statement about what the arm emits,
/// so the check runs on the real arm and on the real arm with one more item in
/// it. An `impl Runtime` written there would make a method, with writes the
/// census cannot see, that no query of this index could find.
///
/// MUTATION: in `the_text_declaration_and_nothing_else`, match
/// `[enumeration, list, ..]` instead of `[enumeration, list]` and the first
/// planted arm is accepted.
#[test]
fn an_extra_item_inside_the_admitted_arm_is_refused() {
    let index = bt_app();
    let arm = &index
        .unsupported_macro_shapes()
        .iter()
        .find(|reported| reported.shape == MacroShape::ItemConstructingArm)
        .expect("`text_entries!` is an item-constructing arm")
        .spelling;
    assert_eq!(the_text_declaration_and_nothing_else(arm), Ok(()));

    let close = arm.rfind('}').expect("the expansion is braced");
    let beside = format!(
        "{}\n        impl Runtime {{ fn planted(&mut self) {{ self.window.title = String::new(); }} }}\n{}",
        &arm[..close],
        &arm[close..]
    );
    let refused = the_text_declaration_and_nothing_else(&beside)
        .expect_err("an `impl Runtime` beside the enum is an item the index does not hold");
    assert!(refused.contains("3 items"), "{refused}");

    let list = arm
        .find("pub const ALL")
        .expect("the test list is in the arm");
    let inside = format!(
        "{}fn planted() {{}}\n            {}",
        &arm[..list],
        &arm[list..]
    );
    let refused = the_text_declaration_and_nothing_else(&inside)
        .expect_err("a `fn` inside `impl Text` is an item the index does not hold");
    assert!(refused.contains("more than the `ALL` list"), "{refused}");
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

/// RED — **self-exclusion works inside a union of modules too**, and it removes
/// a construction only where the scope reached it.
///
/// A union narrows which bytes are read; it does not change what a reader is
/// allowed to find of itself. This file is one of the universe's own and its
/// module path is `crate` — one member of the union below — so the needle here
/// is inside the scope and its own construction is what the exclusion takes.
///
/// MUTATION: apply the scope after the removals and the first block's
/// construction stops being excluded and starts being counted; exclude by file
/// instead of by construction span and the second block's occurrences in
/// `src/query.rs` go with it.
#[test]
fn a_needle_built_inside_a_union_of_modules_excludes_its_own_construction() {
    let index = bt_source();
    let union = || {
        Scope::Modules(vec![
            ModuleSpec::exact("crate"),
            ModuleSpec::tree("crate::query"),
        ])
    };
    let found = index
        .search(
            &Search::new(
                needle!(Pattern::text(
                    "a_spelling_written_only_where_this_needle_is_built"
                )),
                View::Raw,
            )
            .in_scope(union()),
        )
        .expect("the construction is locatable");
    assert!(
        found.is_empty(),
        "the only occurrence is the needle itself:\n{}",
        found.report(&index)
    );
    assert_eq!(found.excluded().len(), 1, "{}", found.report(&index));
    assert_eq!(found.excluded()[0].why, Why::NeedleConstruction);
    let Provenance::Excluded { file, .. } = found.provenance() else {
        panic!(
            "this file is one of the universe's own: {:?}",
            found.provenance()
        );
    };
    assert!(file.ends_with("contract.rs"), "{}", file.display());

    // And a union that does not reach this file still records where the needle
    // came from, while removing nothing: the exclusion is a span, not a name.
    let elsewhere = index
        .search(
            &Search::new(
                needle!(Pattern::identifier("ModuleSpec")),
                View::Identifiers,
            )
            .in_scope(Scope::Modules(vec![ModuleSpec::tree("crate::query")])),
        )
        .expect("the construction is locatable");
    assert!(
        elsewhere.len() >= 2,
        "the type is declared and resolved there:\n{}",
        elsewhere.report(&index)
    );
    assert!(
        elsewhere.excluded().is_empty(),
        "{:?}",
        elsewhere.excluded()
    );
    assert!(matches!(
        elsewhere.provenance(),
        Provenance::Excluded { .. }
    ));
}

/// RED — **a needle built outside the universe being read is a recorded answer,
/// not a failure** (§2.6 rule 1).
///
/// This is the shape of `bt-pty/tests/shell_integration_cmd.rs` and
/// `shell_integration_wsl.rs`: both ask `bt-app`'s index for the product's own
/// spelling, and their caller file will never resolve into `bt-app`'s
/// enumeration. Resolution goes through the
/// caller's own crate — this file is found through `bt-source`'s manifest
/// directory — and the exclusion then applies to nothing, because the queried
/// source does not contain the site.
///
/// MUTATION: panic when the site is outside and both `bt-pty` readers become
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
