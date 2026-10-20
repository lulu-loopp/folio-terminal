//! **The call graph's grammar, a fixture per row and per unresolved kind** (T-BT-SOURCE-CALL-GRAPH,
//! `bt_source::call_graph`'s module table). Each fixture under `tests/fixtures/call_graph/` is a
//! small crate; each test names the mutation of the engine that turns it red. The last test runs
//! the engine over `bt-app` and its first-party dependencies and prints what it met.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_source::call_graph::{
    Arms, CallGraph, Edge, ItemId, Node, Resolver, Target, Unresolved, Via,
};
use bt_source::{
    DiskScope, Index, TargetId, TargetKind, TargetRoot, Universe, Vendor, Workspace, is_vendored,
    report, universes,
};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("call_graph")
}

/// The fixture crate in `directory`, as the package `package`.
fn fixture_as(directory: &str, package: &str) -> Arc<Index> {
    let root = fixtures().join(directory);
    let universe = Universe::declare(
        format!("the call graph's {directory} fixture"),
        vec![TargetRoot {
            id: TargetId {
                package: package.to_owned(),
                kind: TargetKind::Library,
                name: package.replace('-', "_"),
            },
            file: root.join("lib.rs"),
        }],
        vec![DiskScope::under(&root)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::shared(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

fn package_of(directory: &str) -> String {
    format!("cg-{directory}")
}

fn fixture(directory: &str) -> Arc<Index> {
    fixture_as(directory, &package_of(directory))
}

/// The resolver over one fixture, under the product's arms.
fn resolver<'i>(directory: &str, index: &'i Index) -> Resolver<'i> {
    let package: &'static str = Box::leak(package_of(directory).into_boxed_str());
    Resolver::new(&[(package, index)], Arms::product())
        .unwrap_or_else(|refusals| panic!("{refusals:?}"))
}

/// The callable named `name` (the first, when several are).
fn item(resolver: &Resolver<'_>, package: &str, name: &str) -> ItemId {
    resolver
        .callables(package)
        .into_iter()
        .find(|id| resolver.record(*id).name() == name)
        .unwrap_or_else(|| panic!("`{name}` is a callable of {package}"))
}

/// One edge's target, spelled.
fn spelled(resolver: &Resolver<'_>, edge: &Edge) -> String {
    match &edge.to {
        Target::Item(id) => resolver.key(*id),
        Target::Constructor => "constructor".to_owned(),
        Target::Value(id) => format!("value {}", resolver.node_key(Node::Value(*id))),
        Target::Closure(_) => "closure".to_owned(),
        Target::Nested(_) => "nested".to_owned(),
        Target::Foreign(path) => format!("foreign {}", path.join("::")),
        Target::Unresolved(kind) => match kind {
            Unresolved::Dynamic { candidates, .. } => {
                format!("dynamic {}", keys(resolver, candidates).join(", "))
            }
            Unresolved::Ambiguous { method, candidates } => {
                format!(
                    "ambiguous {method}: {}",
                    keys(resolver, candidates).join(", ")
                )
            }
            Unresolved::Macro { name } => format!("macro {name}"),
            Unresolved::OtherArm { candidates } => {
                format!("other arm {}", keys(resolver, candidates).join(", "))
            }
            Unresolved::Generated { by } => format!("generated {by}"),
            Unresolved::Path { written, .. } => format!("path {written}"),
            Unresolved::Cutoff { .. } => "cutoff".to_owned(),
            Unresolved::Unparsed { .. } => "unparsed".to_owned(),
        },
    }
}

fn keys(resolver: &Resolver<'_>, ids: &[ItemId]) -> Vec<String> {
    let mut keys: Vec<String> = ids.iter().map(|id| resolver.key(*id)).collect();
    keys.sort();
    keys
}

/// What the calls named `site` in `caller`'s body (in the first package) go to, spelled and
/// sorted.
fn at(resolver: &Resolver<'_>, caller: &str, site: &str) -> Vec<String> {
    let package = resolver.packages()[0];
    let from = item(resolver, package, caller);
    let mut found: Vec<String> = resolver
        .resolve_callees(from)
        .iter()
        .filter(|edge| edge.site.name == site)
        .map(|edge| spelled(resolver, edge))
        .collect();
    found.sort();
    found
}

// ── the note's four ───────────────────────────────────────────────────────

/// RED — **a helper two calls deep is reached, and the foreign call it makes is reported with the
/// path that reached it.**
///
/// MUTATION: in `CallGraph::walk_following`, expand only the roots (stop at depth 1) — `helper2` is
/// not reached and `std::fs::write` is not met.
#[test]
fn a_helper_two_calls_deep_is_reached() {
    let index = fixture("two_deep");
    let resolver = resolver("two_deep", &index);
    let graph = CallGraph::new(&resolver);
    let root = item(&resolver, "cg-two_deep", "root");
    let reach = graph.walk(&[root], &BTreeSet::new());
    let reached: Vec<String> = keys(
        &resolver,
        &reach.reached.iter().copied().collect::<Vec<_>>(),
    );
    assert_eq!(
        reached,
        [
            "cg-two_deep crate::helper",
            "cg-two_deep crate::helper2",
            "cg-two_deep crate::root",
        ]
    );
    let [(edge, path)] = reach.foreign.as_slice() else {
        panic!("one foreign call: {:?}", reach.foreign);
    };
    assert_eq!(spelled(&resolver, edge), "foreign std::fs::write");
    assert_eq!(
        path,
        &[
            Node::Item(root),
            Node::Item(item(&resolver, "cg-two_deep", "helper")),
            Node::Item(item(&resolver, "cg-two_deep", "helper2")),
        ],
        "the path starts at the root"
    );
}

/// RED — **a method found through two first-party `Deref` steps is the target's, and each step's
/// `deref` is an edge.**
///
/// MUTATION: in `Tables::method_on`, do not take the `Deref` step (`deref_of` answers nothing) —
/// `Inner::only_on_inner` and both `deref` edges are missing.
#[test]
fn a_method_through_deref_is_the_targets() {
    let index = fixture("deref");
    let resolver = resolver("deref", &index);
    assert_eq!(
        at(&resolver, "root", "only_on_inner"),
        [
            "cg-deref crate::<Middle as Deref>::deref",
            "cg-deref crate::<Outer as Deref>::deref",
            "cg-deref crate::Inner::only_on_inner",
        ]
    );
}

/// RED — **a call on a first-party trait object reaches every implementation and nothing else of
/// the name; one no implementation answers is a dynamic edge, and so is a foreign trait object,
/// whose candidates are the first-party implementations.**
///
/// MUTATIONS: in `Tables::dispatch`, answer an empty set where no implementation answers — the
/// `Lonely` call has no edge; unite the dispatch with every first-party method of the name (by
/// name) — `Cache::prepare` appears.
#[test]
fn a_trait_object_call_is_its_implementations_or_dynamic() {
    let index = fixture("trait_object");
    let resolver = resolver("trait_object", &index);
    let both = [
        "cg-trait_object crate::<Mac as Driver>::prepare",
        "cg-trait_object crate::<Win as Driver>::prepare",
    ];
    assert_eq!(at(&resolver, "borrowed", "prepare"), both);
    assert_eq!(at(&resolver, "boxed", "prepare"), both);
    assert_eq!(at(&resolver, "path", "prepare"), both);
    assert_eq!(
        at(&resolver, "bound", "settle"),
        [
            "cg-trait_object crate::<Mac as Driver>::settle",
            "cg-trait_object crate::Driver::settle",
        ],
        "the default runs for `Win`, which does not override it"
    );
    assert_eq!(at(&resolver, "lonely", "alone"), ["dynamic "]);
    assert_eq!(
        at(&resolver, "standard", "flush"),
        ["dynamic cg-trait_object crate::<Sink as std::io::Write>::flush"]
    );
}

/// RED — **a macro invocation is an `Unresolved::Macro` edge, and a call among its expression
/// arguments is walked, carrying the macro.**
///
/// MUTATION: in `Walker::mac`, record the macro as no unresolved edge (a constructor) — the test
/// demands the edge.
#[test]
fn a_macro_is_an_unresolved_edge() {
    let index = fixture("macros");
    let resolver = resolver("macros", &index);
    let root = item(&resolver, "cg-macros", "root");
    let edges = resolver.resolve_callees(root);
    let macros: Vec<&Edge> = edges
        .iter()
        .filter(|edge| matches!(&edge.to, Target::Unresolved(Unresolved::Macro { name }) if name == "my_macro"))
        .collect();
    assert_eq!(macros.len(), 2, "both invocations are edges: {edges:?}");
    let helper: Vec<&Edge> = edges
        .iter()
        .filter(|edge| edge.site.name == "helper")
        .collect();
    let [helper] = helper.as_slice() else {
        panic!("the argument's call is walked: {edges:?}");
    };
    assert_eq!(spelled(&resolver, helper), "cg-macros crate::helper");
    assert_eq!(helper.site.macro_arg.as_deref(), Some("my_macro"));
}

// ── one per grammar row ───────────────────────────────────────────────────

/// RED (R1) — **a path call is placed through the module tree**: bare, `self::`, `crate::`, a
/// child module, a rename, a re-export, a glob, another crate's path and its re-export, the
/// prelude, a foreign path, a function-body `use`; a single foreign glob supplies its name; two
/// foreign globs are refused; a name nothing holds is a path edge.
///
/// MUTATIONS: in `Names::in_scope_once`, drop the `use` names step — `alias_name`,
/// `reexport::deep` and the crate re-export name nothing; take the first of two foreign globs —
/// `mystery` is foreign.
#[test]
fn a_path_call_is_placed_through_the_module_tree() {
    let own = fixture("paths");
    let other = fixture_as("other_crate", "cg-other");
    let resolver = Resolver::new(
        &[("cg-paths", &*own), ("cg-other", &*other)],
        Arms::product(),
    )
    .unwrap_or_else(|refusals| panic!("{refusals:?}"));
    let root = item(&resolver, "cg-paths", "root");
    let edges = resolver.resolve_callees(root);
    let by_name = |name: &str| -> Vec<String> {
        edges
            .iter()
            .filter(|edge| edge.site.name == name)
            .map(|edge| spelled(&resolver, edge))
            .collect()
    };
    assert_eq!(by_name("local"), ["cg-paths crate::local"; 3]);
    assert_eq!(by_name("deep"), ["cg-paths crate::inner::deep"; 2]);
    assert_eq!(by_name("deeper"), ["cg-paths crate::inner::nested::deeper"]);
    assert_eq!(by_name("alias_name"), ["cg-paths crate::renamed::original"]);
    assert_eq!(by_name("by_glob"), ["cg-paths crate::opened::by_glob"]);
    assert_eq!(by_name("far"), ["cg-other crate::tools::far"]);
    assert_eq!(by_name("again"), ["cg-other crate::tools::far"]);
    assert_eq!(by_name("drop"), ["foreign std::mem::drop"]);
    assert_eq!(by_name("remove_file"), ["foreign std::fs::remove_file"]);
    assert_eq!(by_name("here"), ["cg-paths crate::inner::nested::deeper"]);
    assert_eq!(by_name("nowhere"), ["path nowhere"]);
    let one = item(&resolver, "cg-paths", "caller");
    let callers: Vec<ItemId> = resolver
        .callables("cg-paths")
        .into_iter()
        .filter(|id| resolver.record(*id).name() == "caller")
        .collect();
    assert_eq!(callers.len(), 2);
    let mut answers: Vec<String> = callers
        .iter()
        .flat_map(|id| resolver.resolve_callees(*id))
        .map(|edge| spelled(&resolver, &edge))
        .collect();
    answers.sort();
    assert_eq!(
        answers,
        ["foreign rayon::prelude::par_bridge", "path mystery"],
        "one foreign glob supplies its name; two are refused ({one:?})"
    );
}

/// RED (R2) — **a tuple-struct or tuple-variant constructor is resolved, and not walked.**
///
/// MUTATION: in `Walker::module_call`, drop the constructor answer — `Wrap(1)` is a path edge.
#[test]
fn a_constructor_is_resolved_and_not_walked() {
    let index = fixture("constructors");
    let resolver = resolver("constructors", &index);
    assert_eq!(at(&resolver, "root", "Wrap"), ["constructor"]);
    assert_eq!(at(&resolver, "root", "One"), ["constructor"]);
    assert_eq!(at(&resolver, "root", "Some"), ["constructor"]);
}

/// RED (R3) — **an associated function is the type's**: inherent, its trait impl's, a trait
/// default it does not override, and `Self::`.
///
/// MUTATION: in `Tables::members`, skip the trait defaults — `Thing::made()` is a path edge.
#[test]
fn an_associated_function_is_the_types() {
    let index = fixture("associated");
    let resolver = resolver("associated", &index);
    assert_eq!(
        at(&resolver, "root", "new"),
        ["cg-associated crate::Thing::new"]
    );
    assert_eq!(
        at(&resolver, "root", "make"),
        ["cg-associated crate::<Thing as Make>::make"]
    );
    assert_eq!(
        at(&resolver, "root", "made"),
        ["cg-associated crate::Make::made"]
    );
    assert_eq!(
        at(&resolver, "new", "helper"),
        ["cg-associated crate::Thing::helper"]
    );
}

/// RED (R4) — **a method on a stated receiver is its type's**: a parameter, a written `let`, a
/// `let` carried from its initialiser, `self` and `self.field`, a `Box`, an `Rc`, an `Arc`; a
/// shadowing binding retypes the name.
///
/// MUTATION: in `Walker::lookup`, look a name up outermost scope first (the earlier binding
/// wins) — the shadowed call is `Part::spin`.
#[test]
fn a_method_on_a_stated_receiver_is_its_types() {
    let index = fixture("receivers");
    let resolver = resolver("receivers", &index);
    let run = ["cg-receivers crate::Engine::run"];
    assert_eq!(at(&resolver, "parameter", "run"), run);
    assert_eq!(at(&resolver, "annotated", "run"), run);
    assert_eq!(at(&resolver, "carried", "run"), run);
    assert_eq!(
        at(&resolver, "run", "spin"),
        ["cg-receivers crate::Part::spin"]
    );
    assert_eq!(
        at(&resolver, "run", "idle"),
        ["cg-receivers crate::Engine::idle"]
    );
    assert_eq!(
        at(&resolver, "pointers", "spin"),
        ["cg-receivers crate::Part::spin"; 3]
    );
    assert_eq!(
        at(&resolver, "shadowed", "spin"),
        ["cg-receivers crate::Other::spin"]
    );
}

/// RED (R6) — **`<X as Tr>::m` is X's impl of Tr, or Tr's default where it does not override it;
/// on a bound generic it is the dispatch.**
///
/// MUTATION: in `Tables::qualified_on`, skip the trait's default — `<Dog as Speak>::hum` is a path
/// edge.
#[test]
fn a_qualified_call_is_the_impl_of_the_trait() {
    let index = fixture("ufcs");
    let resolver = resolver("ufcs", &index);
    let calls = at(&resolver, "root", "speak");
    assert_eq!(calls, ["cg-ufcs crate::<Dog as Speak>::speak"]);
    assert_eq!(at(&resolver, "root", "hum"), ["cg-ufcs crate::Speak::hum"]);
    assert_eq!(
        at(&resolver, "generic", "speak"),
        [
            "cg-ufcs crate::<Cat as Speak>::speak",
            "cg-ufcs crate::<Dog as Speak>::speak"
        ]
    );
}

/// RED (R7) — **a type written through an alias of an alias is the type it names**, and an alias
/// is an index record: one per `type` item written in a module, with its arm, and none for an
/// associated type or one written inside a body.
///
/// MUTATIONS: in `Tables::expand`, follow one alias only — `value.act()` on `Shorter` is a cutoff;
/// in `lower.rs`, lower no `syn::Item::Type` — the index has no alias records.
#[test]
fn a_type_through_an_alias_is_the_type_it_names() {
    let index = fixture("alias");
    let names: Vec<(&str, Vec<String>)> = index
        .aliases()
        .iter()
        .map(|alias| (alias.name(), alias.variant().predicates().to_vec()))
        .collect();
    assert_eq!(
        names,
        [
            ("Short", Vec::new()),
            ("Shorter", vec!["windows".to_owned()])
        ]
    );
    assert_eq!(index.aliases()[0].module_paths(), ["crate"]);
    let resolver = resolver("alias", &index);
    assert_eq!(at(&resolver, "root", "act"), ["cg-alias crate::Real::act"]);
}

/// RED (R8) — **an inline closure call is the closure, a closure's body is its definer's with the
/// closure on its edges, and a stored closure called is dynamic.**
///
/// MUTATION: in `Walker::closure`, do not push the closure's span — `handed`'s edge carries no
/// closure.
#[test]
fn a_closure_is_its_definers_and_a_stored_one_is_dynamic() {
    let index = fixture("closures");
    let resolver = resolver("closures", &index);
    let root = item(&resolver, "cg-closures", "root");
    let edges = resolver.resolve_callees(root);
    assert!(
        edges
            .iter()
            .any(|edge| matches!(edge.to, Target::Closure(_)))
    );
    let handed: Vec<&Edge> = edges
        .iter()
        .filter(|edge| edge.site.name == "handed")
        .collect();
    let [handed] = handed.as_slice() else {
        panic!("the handed closure's call is walked: {edges:?}");
    };
    assert_eq!(spelled(&resolver, handed), "cg-closures crate::handed");
    assert!(handed.site.closure.is_some(), "it is written in a closure");
    assert_eq!(at(&resolver, "root", "stored"), ["dynamic "]);
    assert_eq!(at(&resolver, "spawn", "work"), ["dynamic "]);
    assert_eq!(
        at(&resolver, "root", "inside"),
        ["cg-closures crate::inside"; 2]
    );
}

/// RED (R9, R10) — **a function named as a value is an edge to it, and a `fn` written inside a
/// body is walked as the body's.**
///
/// MUTATION: in `Walker::path_value`, push no value edge — `spawn(worker)` names nothing.
#[test]
fn a_function_value_and_a_nested_fn_are_edges() {
    let index = fixture("values");
    let resolver = resolver("values", &index);
    let root = item(&resolver, "cg-values", "root");
    let edges = resolver.resolve_callees(root);
    let worker: Vec<(String, Via)> = edges
        .iter()
        .filter(|edge| edge.site.name == "worker")
        .map(|edge| (spelled(&resolver, edge), edge.via.clone()))
        .collect();
    assert_eq!(
        worker,
        [
            ("cg-values crate::worker".to_owned(), Via::Value),
            ("cg-values crate::worker".to_owned(), Via::Direct),
        ]
    );
    assert_eq!(at(&resolver, "root", "local_helper"), ["nested"]);
}

/// RED (R11) — **a foreign callee is its full path**: through `use`, `Arc`'s deref and a guard.
///
/// MUTATION: in `Tables::method_at`, answer a foreign type's method by its written name instead of
/// the followed path — `Mutex::lock` is not `std::sync::Mutex::lock`.
#[test]
fn a_foreign_callee_is_its_full_path() {
    let index = fixture("foreign");
    let resolver = resolver("foreign", &index);
    assert_eq!(
        at(&resolver, "touch", "lock"),
        ["foreign std::sync::Mutex::lock"]
    );
    assert_eq!(at(&resolver, "root", "read"), ["foreign std::fs::read"]);
}

// ── one per unresolved kind ───────────────────────────────────────────────

/// RED (`Ambiguous`) — **an untyped receiver's call lists every first-party method of the name and
/// picks none.**
///
/// MUTATION: in `Tables::untyped`, keep only the first candidate — `shared_name` lists one.
#[test]
fn an_untyped_receiver_is_ambiguous_with_its_candidates() {
    let index = fixture("ambiguous");
    let resolver = resolver("ambiguous", &index);
    assert_eq!(
        at(&resolver, "root", "shared_name"),
        [
            "ambiguous shared_name: cg-ambiguous crate::A::shared_name, cg-ambiguous crate::B::shared_name"
        ]
    );
    assert_eq!(
        at(&resolver, "root", "only_a"),
        ["ambiguous only_a: cg-ambiguous crate::A::only_a"]
    );
    assert_eq!(
        at(&resolver, "root", "no_such_method"),
        ["ambiguous no_such_method: "]
    );
}

/// RED (`OtherArm`) — **a callee only another arm has, and a site under a statement `#[cfg]` the
/// arms rule out, are other-arm edges**; a fixed platform rules out the other's functions.
///
/// MUTATION: in `Walker::ruled_out`, rule nothing out — the `#[cfg(test)]` block's call is an item
/// edge.
#[test]
fn another_arms_code_is_an_other_arm_edge() {
    let index = fixture("other_arm");
    let resolver = resolver("other_arm", &index);
    let mut calls = at(&resolver, "root", "on_windows");
    calls.sort();
    assert_eq!(
        calls,
        [
            "cg-other_arm crate::on_windows",
            "other arm cg-other_arm crate::on_windows"
        ]
    );
    assert_eq!(
        at(&resolver, "root", "test_only"),
        ["other arm cg-other_arm crate::test_only"]
    );
    let windows = Resolver::new(
        &[("cg-other_arm", &*index)],
        Arms::filtered(|query| {
            query.compilation.permits_product()
                && !query
                    .predicates
                    .iter()
                    .any(|predicate| predicate.contains("macos"))
        }),
    )
    .unwrap_or_else(|refusals| panic!("{refusals:?}"));
    assert_eq!(
        at(&windows, "root", "on_macos"),
        ["other arm cg-other_arm crate::on_macos"]
    );
    assert_eq!(
        at(&resolver, "root", "on_macos"),
        ["cg-other_arm crate::on_macos"]
    );
}

/// RED (`Generated`) — **a derive's method and `#[from]`'s `From::from` are generated edges.**
///
/// MUTATION: in `Tables::generated`, answer nothing — `clone` is a foreign prelude call and
/// `Failure::from` a path edge.
#[test]
fn a_generated_callee_is_a_generated_edge() {
    let index = fixture("generated");
    let resolver = resolver("generated", &index);
    assert_eq!(
        at(&resolver, "root", "clone"),
        ["generated #[derive(Clone)]"]
    );
    assert_eq!(at(&resolver, "root", "from"), ["generated #[from]"]);
}

/// RED (`Cutoff`) — **a path chained past the hop limit is a cutoff edge, never "names nothing"**;
/// a short chain is followed.
///
/// MUTATION: in `Walker::module_call`, ignore `resolution.cut` — `link20` is a path edge.
#[test]
fn a_chain_past_the_hop_limit_is_a_cutoff() {
    let index = fixture("cutoff");
    let resolver = resolver("cutoff", &index);
    assert_eq!(at(&resolver, "near", "link3"), ["cg-cutoff crate::target"]);
    assert_eq!(at(&resolver, "far", "link20"), ["cutoff"]);
}

// `Unresolved::Unparsed` has no fixture: the index refuses a file `syn` does not parse, and an
// item's text, cut from a file that parsed, parses on its own. The kind is there so that a body
// that ever did not would be an edge rather than a panic or a silence.

// ── the walk ──────────────────────────────────────────────────────────────

/// RED — **the walk visits each item once (a cycle ends), stops at a boundary item without
/// entering it, and reports an edge met with the first path that reached it.**
///
/// MUTATION: in `CallGraph::walk_following`, expand a boundary item — `beyond` is reached.
#[test]
fn the_walk_stops_at_the_boundary_and_visits_once() {
    let index = fixture("walk");
    let resolver = resolver("walk", &index);
    let graph = CallGraph::new(&resolver);
    let root = item(&resolver, "cg-walk", "root");
    let boundary = item(&resolver, "cg-walk", "boundary");
    let reach = graph.walk(&[root], &BTreeSet::from([boundary]));
    assert_eq!(
        keys(
            &resolver,
            &reach.reached.iter().copied().collect::<Vec<_>>()
        ),
        [
            "cg-walk crate::ping",
            "cg-walk crate::pong",
            "cg-walk crate::root"
        ]
    );
    assert_eq!(reach.stopped_at, BTreeSet::from([boundary]));
    let [(edge, path)] = reach.foreign.as_slice() else {
        panic!("one foreign call: {:?}", reach.foreign);
    };
    assert_eq!(spelled(&resolver, edge), "foreign std::fs::read");
    assert_eq!(
        path,
        &[
            Node::Item(root),
            Node::Item(item(&resolver, "cg-walk", "ping")),
            Node::Item(item(&resolver, "cg-walk", "pong")),
        ]
    );
}

/// RED — **the first-party impls only an implicit call runs are listed** (`Drop` here), not
/// reached.
///
/// MUTATION: in `Resolver::implicit`, list nothing — `Lane`'s `drop` is missing.
#[test]
fn an_implicit_callees_impl_is_listed() {
    let index = fixture("implicit");
    let resolver = resolver("implicit", &index);
    let graph = CallGraph::new(&resolver);
    let root = item(&resolver, "cg-implicit", "root");
    let reach = graph.walk(&[root], &BTreeSet::new());
    let drops: Vec<ItemId> = reach
        .implicit
        .get("std::ops::Drop")
        .into_iter()
        .flatten()
        .copied()
        .collect();
    assert_eq!(
        keys(&resolver, &drops),
        ["cg-implicit crate::<Lane as Drop>::drop"]
    );
    assert!(
        !reach.reached.contains(&drops[0]),
        "an implicit callee is listed, not reached"
    );
}

// ── the real tree ─────────────────────────────────────────────────────────

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// **The walk over `bt-app` and its first-party packages, from every product callable of
/// `bt-app` as a root** — the upper bound of any door walk — with the counts of what it met,
/// printed; and the product holds no cutoff and no body that does not parse. Not a timing: the
/// timing-bound list only shrinks, so the cost is measured outside the suite (the ticket's
/// report).
#[test]
fn the_whole_product_walk_is_counted() {
    let workspace = Workspace::read(&workspace_root()).expect("this workspace");
    let held: Vec<(String, Arc<Index>)> = workspace
        .packages()
        .iter()
        .filter(|package| package.name() != "bt-source")
        .map(|package| {
            let vendor = if is_vendored(package.directory()) {
                Vendor::Included
            } else {
                Vendor::Excluded
            };
            let universe = universes::crate_sources(package, vendor)
                .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
            let index = Index::shared(&universe)
                .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
            (package.name().to_owned(), index)
        })
        .collect();
    let packages: Vec<(&str, &Index)> = held
        .iter()
        .map(|(name, index)| (name.as_str(), &**index))
        .collect();
    let resolver =
        Resolver::new(&packages, Arms::product()).unwrap_or_else(|refusals| panic!("{refusals:?}"));
    let roots: Vec<ItemId> = resolver
        .callables("bt-app")
        .into_iter()
        .filter(|id| resolver.admitted(*id))
        .collect();
    let graph = CallGraph::new(&resolver);
    let reach = graph.walk(&roots, &BTreeSet::new());
    let mut kinds: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for (edge, _) in &reach.unresolved {
        let kind = match &edge.to {
            Target::Unresolved(Unresolved::Dynamic { .. }) => "dynamic",
            Target::Unresolved(Unresolved::Ambiguous { .. }) => "ambiguous",
            Target::Unresolved(Unresolved::Macro { .. }) => "macro",
            Target::Unresolved(Unresolved::OtherArm { .. }) => "other arm",
            Target::Unresolved(Unresolved::Generated { .. }) => "generated",
            Target::Unresolved(Unresolved::Path { .. }) => "path",
            Target::Unresolved(Unresolved::Cutoff { .. }) => "cutoff",
            Target::Unresolved(Unresolved::Unparsed { .. }) => "unparsed",
            _ => "other",
        };
        *kinds.entry(kind).or_default() += 1;
    }
    eprintln!(
        "CALL_GRAPH_REAL_TREE\tpackages {}\troots {}\treached {}\tforeign {}\tunresolved {:?}\t\
         implicit traits {}\tunparsed declarations {}",
        packages.len(),
        roots.len(),
        reach.reached.len(),
        reach.foreign.len(),
        kinds,
        reach.implicit.len(),
        resolver.unparsed().len()
    );
    assert!(
        !kinds.contains_key("cutoff") && !kinds.contains_key("unparsed"),
        "the product has no cutoff and no unparsed body: {kinds:?}"
    );
    eprintln!(
        "CALL_GRAPH_UNPLACED_IMPLS\t{}\n  {}",
        resolver.unplaced_impls().len(),
        resolver
            .unplaced_impls()
            .iter()
            .map(|unplaced| unplaced.what.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    assert!(
        resolver.unparsed().is_empty(),
        "every declaration of the product parses for the call graph: {:?}",
        resolver.unparsed()
    );
}

// ── round 2 ───────────────────────────────────────────────────────────────

/// What the sites named `site` in `caller`'s body go to, with how each is reached.
fn at_via(resolver: &Resolver<'_>, caller: &str, site: &str) -> Vec<(String, Via)> {
    let package = resolver.packages()[0];
    let from = item(resolver, package, caller);
    let mut found: Vec<(String, Via)> = resolver
        .resolve_callees(from)
        .iter()
        .filter(|edge| edge.site.name == site)
        .map(|edge| (spelled(resolver, edge), edge.via.clone()))
        .collect();
    found.sort();
    found
}

/// RED (R9, round 2) — **a function named as a value whose path is no one item is an edge of its
/// kind, with `Via::Value`, never dropped**: a derive's `default` (generated), `Self::f` with two
/// traits declaring `f` (ambiguous), a `cfg(test)` function (other arm), a macro-generated one
/// (path), a trait no implementation answers (dynamic); and data — a `const`, a `static`, a unit
/// struct, an associated `const` — is no call (a value's use is an edge to its initializer, a unit
/// struct's no edge), while a method called on a `static` is
/// typed by its declaration (through a `LazyLock` to its value).
///
/// MUTATION: in `Walker::path_value`, push only the resolved, nested, foreign and cutoff targets
/// (round 1's drop) — the generated, ambiguous, other-arm, path and dynamic edges are missing.
#[test]
fn a_function_value_that_is_no_one_item_is_an_edge_of_its_kind() {
    let index = fixture("value_kinds");
    let resolver = resolver("value_kinds", &index);
    let value = |text: &str| vec![(text.to_owned(), Via::Value)];
    assert_eq!(
        at_via(&resolver, "root", "default"),
        value("generated #[derive(Default)]")
    );
    assert_eq!(
        at_via(&resolver, "root", "f"),
        value(
            "ambiguous f: cg-value_kinds crate::<Both as Again>::f, \
             cg-value_kinds crate::<Both as Twice>::f"
        )
    );
    assert_eq!(
        at_via(&resolver, "root", "worker"),
        value("other arm cg-value_kinds crate::worker")
    );
    assert_eq!(
        at_via(&resolver, "root", "generated_fn"),
        value("path generated_fn")
    );
    assert_eq!(at_via(&resolver, "generic", "alone"), value("dynamic "));
    // Data is no call: a `const`'s or `static`'s use is an edge to its initializer only (round
    // 3), and a unit struct is no edge at all.
    for (data, initializer) in [
        ("LIMIT", "value cg-value_kinds crate::LIMIT"),
        ("NAMES", "value cg-value_kinds crate::NAMES"),
        ("CAP", "value cg-value_kinds crate::Both::CAP"),
    ] {
        assert_eq!(
            at_via(&resolver, "root", data),
            value(initializer),
            "`{data}` is data: its use reaches its initializer and calls nothing"
        );
    }
    assert_eq!(
        at_via(&resolver, "root", "Unit"),
        Vec::new(),
        "a unit struct is no edge"
    );
    assert_eq!(
        at(&resolver, "root", "lock"),
        ["foreign std::sync::Mutex::lock"],
        "a `static` is typed by its declaration"
    );
    assert_eq!(
        at(&resolver, "root", "ask"),
        ["cg-value_kinds crate::Lane::ask"],
        "a `static` behind a `LazyLock` derefs to its value"
    );
}

/// RED (R7, round 2) — **one alias on two arms naming two types is neither of them taken alone**:
/// over both arms the call is untyped and names both methods; with one arm fixed it is that arm's.
///
/// MUTATION: in `Tables::read_aliases`, keep one declaration per alias (the last read wins) — the
/// call is one arm's type over both arms.
#[test]
fn an_alias_on_two_arms_is_neither_arm_alone() {
    let index = fixture("alias_arms");
    let resolver = resolver("alias_arms", &index);
    assert_eq!(
        at(&resolver, "root", "show"),
        ["ambiguous show: cg-alias_arms crate::Narrow::show, cg-alias_arms crate::Wide::show"]
    );
    let windows = Resolver::new(
        &[("cg-alias_arms", &*index)],
        Arms::filtered(|query| {
            query.compilation.permits_product()
                && !query
                    .predicates
                    .iter()
                    .any(|predicate| predicate == "not(windows)")
        }),
    )
    .unwrap_or_else(|refusals| panic!("{refusals:?}"));
    assert_eq!(
        at(&windows, "root", "show"),
        ["cg-alias_arms crate::Wide::show"]
    );
}

/// RED (R10, round 2) — **a `fn` written in a body sees the block's items and `use`s**: its call
/// of a sibling `fn` is the sibling, never the module's function of that name, and its call
/// through the block's `use` is the function the `use` names.
///
/// MUTATION: in `Walker::nested_item`, walk the nested item with a fresh scope stack (round 1) —
/// `sibling()` is the module's `crate::sibling` and `from_use()` names nothing.
#[test]
fn a_nested_fn_sees_the_blocks_items_and_uses() {
    let index = fixture("nested_scope");
    let resolver = resolver("nested_scope", &index);
    assert_eq!(at(&resolver, "root", "sibling"), ["nested"]);
    assert_eq!(
        at(&resolver, "root", "from_use"),
        ["cg-nested_scope crate::other::helper"]
    );
}

/// RED (`Cutoff`, round 2) — **a name a body's glob opens past the hop limit is a cutoff**.
///
/// MUTATION: in `Walker::module_call`, discard the body globs' cutoff — `link20()` is a path edge.
#[test]
fn a_body_glob_past_the_hop_limit_is_a_cutoff() {
    let index = fixture("cutoff");
    let resolver = resolver("cutoff", &index);
    assert_eq!(at(&resolver, "body_glob", "link20"), ["cutoff"]);
}

/// RED (round 2) — **an `impl` block whose self type the module tree does not hold (a type a macro
/// generates) is listed with its methods**, never filed under nothing; only a self type chained
/// past the hop limit refuses the reading (a refusal).
///
/// MUTATION: in `Tables::read_impls`, skip such a block without listing it (round 1 filed it under
/// `SelfKey::Other`) — `unplaced_impls` is empty.
#[test]
fn an_impl_for_a_type_nothing_holds_is_listed() {
    let index = fixture("refused");
    let resolver = Resolver::new(&[("cg-refused", &*index)], Arms::product())
        .unwrap_or_else(|refusals| panic!("{refusals:?}"));
    let [unplaced] = resolver.unplaced_impls() else {
        panic!("one unplaced impl: {:?}", resolver.unplaced_impls());
    };
    assert!(
        unplaced.what.contains("`Missing`") && unplaced.what.contains("names nothing"),
        "{}",
        unplaced.what
    );
    assert_eq!(
        keys(&resolver, &unplaced.methods),
        ["cg-refused crate::Missing::lost"]
    );
}

/// RED (round 2) — **an `impl` on an alias declared in a later package is the alias's type**: the
/// types and aliases of every package are read before any `impl` block.
///
/// MUTATION: in `Tables::read`, read each package's `impl` blocks right after its own types (round
/// 1's single pass) — the alias of the later package is not read yet and the block is refused.
#[test]
fn an_impl_on_a_later_packages_alias_is_its_type() {
    let first = fixture_as("alias_later_a", "cg-alias_later_a");
    let later = fixture_as("alias_later_b", "cg-alias_later_b");
    let resolver = Resolver::new(
        &[("cg-alias_later_a", &*first), ("cg-alias_later_b", &*later)],
        Arms::product(),
    )
    .unwrap_or_else(|refusals| panic!("{refusals:?}"));
    assert_eq!(
        at(&resolver, "root", "speak"),
        ["cg-alias_later_a crate::<Short as Speak>::speak"]
    );
}

/// RED (round 2) — **a `const` and a `static` are index records**: a module's, an `impl` block's
/// (with its owner) and a trait's (with the trait), and none for one written in a body.
///
/// MUTATION: in `lower.rs`, lower no `syn::Item::Static` — `NAMES` and `INBOX` are missing.
#[test]
fn a_const_and_a_static_are_index_records() {
    let index = fixture("value_kinds");
    let mut values: Vec<(String, Option<String>, bt_source::ValueKind)> = index
        .values()
        .iter()
        .map(|value| {
            (
                value.name().to_owned(),
                value.type_owner().map(ToOwned::to_owned),
                value.kind(),
            )
        })
        .collect();
    values.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        values,
        [
            (
                "CAP".to_owned(),
                Some("Both".to_owned()),
                bt_source::ValueKind::Const
            ),
            ("INBOX".to_owned(), None, bt_source::ValueKind::Static),
            ("LANE".to_owned(), None, bt_source::ValueKind::Static),
            ("LIMIT".to_owned(), None, bt_source::ValueKind::Const),
            ("NAMES".to_owned(), None, bt_source::ValueKind::Static),
        ]
    );
}

// ── round 3 ───────────────────────────────────────────────────────────────

/// RED (round 3) — **a `static`'s and a `const`'s initializer is a body of the graph**: a use of a
/// `LazyLock` static reaches its initializer with `Via::Deref` (it runs on the first deref), and
/// the walk follows the initializer's calls and the functions it names as values, inside its
/// closure too; a `const` holding a function pointer, named or called, reaches the function, and
/// the call is dynamic with the initializer's function as its candidate.
///
/// MUTATIONS: in `Walker::path_value`, push no edge for a value's use (round 2's data rule) —
/// `probe`, `run` and `note` are not reached from `root`; in `Walker::module_call`, answer a
/// called `const` with the dynamic edge alone — `target` is not reached from `calls_const`.
#[test]
fn a_static_or_const_initializer_is_walked_from_its_use() {
    let index = fixture("lazy_init");
    let resolver = resolver("lazy_init", &index);
    let graph = CallGraph::new(&resolver);
    let package = "cg-lazy_init";
    let root = item(&resolver, package, "root");
    let edges = resolver.resolve_callees(root);
    let cell: Vec<(&Target, &Via)> = edges
        .iter()
        .filter(|edge| edge.site.name == "CELL")
        .map(|edge| (&edge.to, &edge.via))
        .collect();
    assert!(
        matches!(cell.as_slice(), [(Target::Value(_), Via::Deref)]),
        "a use of a `LazyLock` static reaches its initializer by its deref: {cell:?}"
    );
    let reached = |from: ItemId| -> Vec<String> {
        let reach = graph.walk(&[from], &BTreeSet::new());
        keys(
            &resolver,
            &reach.reached.iter().copied().collect::<Vec<_>>(),
        )
    };
    let from_root = reached(root);
    for name in ["probe", "run", "note", "Lane::ask"] {
        assert!(
            from_root.contains(&format!("{package} crate::{name}")),
            "`{name}` is reached through `CELL`'s initializer: {from_root:?}"
        );
    }
    let calls = item(&resolver, package, "calls_const");
    assert!(reached(calls).contains(&format!("{package} crate::target")));
    let call: Vec<String> = resolver
        .resolve_callees(calls)
        .iter()
        .filter(|edge| edge.site.name == "F")
        .map(|edge| spelled(&resolver, edge))
        .collect();
    assert!(
        call.contains(&format!("dynamic {package} crate::target")),
        "the call of `F` is dynamic with the initializer's function as its candidate: {call:?}"
    );
    let names = item(&resolver, package, "names_const");
    assert!(reached(names).contains(&format!("{package} crate::target")));
    let initializer = resolver
        .resolve_callees(root)
        .iter()
        .find_map(|edge| match edge.to {
            Target::Value(id) => Some(id),
            _ => None,
        })
        .expect("`CELL` is a value edge");
    let closure_edges: Vec<String> = resolver
        .value_callees(initializer)
        .iter()
        .filter(|edge| edge.site.closure.is_some())
        .map(|edge| edge.site.name.clone())
        .collect();
    assert!(
        closure_edges.contains(&"probe".to_owned()) && closure_edges.contains(&"note".to_owned()),
        "the initializer's closure carries its edges: {closure_edges:?}"
    );
}

/// The macro-generated types whose `impl` blocks the module tree cannot place, by package, module
/// and type, once per block — a shrink-only list: a block that joins it is refused here (its type
/// is to be written where the tree can read it, or the list widened in a change that says why),
/// and a row whose block is gone is refused as stale. Their methods are reachable only as
/// `Ambiguous` candidates, never through a typed call.
const UNPLACED_IMPLS: [&str; 24] = [
    "alacritty_terminal crate::term TermMode",
    "alacritty_terminal crate::term TermMode",
    "bt-app crate::i18n Text",
    "bt-platform crate::explorer_command Command_Impl",
    "bt-platform crate::explorer_command Factory_Impl",
    "bt-platform crate::http DownloadTransport",
    "bt-platform crate::http DownloadTransport",
    "bt-platform crate::http DownloadTransport",
    "bt-platform crate::http Transport",
    "bt-platform crate::http Transport",
    "bt-platform crate::http Transport",
    "bt-platform crate::macos_dialogs DeferredFormulaMenu",
    "bt-platform crate::macos_impl SystemSettingsObserver",
    "bt-platform crate::macos_impl WindowButtonsObserver",
    "bt-platform crate::macos_menu MenuTarget",
    "bt-platform crate::macos_notify NotificationDelegate",
    "bt-platform crate::macos_services ServicesProvider",
    "bt-platform crate::video::engine Notify_Impl",
    "bt-platform crate::video::macos_player EndWatch",
    "bt-platform crate::webview::macos Gate",
    "bt-pty crate ConPtySource",
    "mitex-parser crate::syntax SyntaxKind",
    "mitex-parser crate::syntax SyntaxKind",
    "mitex-parser crate::syntax SyntaxKind",
];

/// The resolver over every workspace package but this one (bt-app's first-party packages and
/// the vendored ones as what they are).
fn real_tree() -> Vec<(String, Arc<Index>)> {
    let workspace = Workspace::read(&workspace_root()).expect("this workspace");
    workspace
        .packages()
        .iter()
        .filter(|package| package.name() != "bt-source")
        .map(|package| {
            let vendor = if is_vendored(package.directory()) {
                Vendor::Included
            } else {
                Vendor::Excluded
            };
            let universe = universes::crate_sources(package, vendor)
                .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
            let index = Index::shared(&universe)
                .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
            (package.name().to_owned(), index)
        })
        .collect()
}

/// RED (round 3) — **the `impl` blocks of types the module tree does not hold are exactly the
/// named list** ([`UNPLACED_IMPLS`]): a 25th is refused, and so is a row whose block is gone.
///
/// MUTATION: remove one row of the list — the block it named is refused as unlisted.
#[test]
fn the_unplaced_impls_are_the_named_list() {
    let held = real_tree();
    let packages: Vec<(&str, &Index)> = held
        .iter()
        .map(|(name, index)| (name.as_str(), &**index))
        .collect();
    let resolver =
        Resolver::new(&packages, Arms::product()).unwrap_or_else(|refusals| panic!("{refusals:?}"));
    let mut found: Vec<&str> = resolver
        .unplaced_impls()
        .iter()
        .map(|unplaced| unplaced.key.as_str())
        .collect();
    found.sort_unstable();
    let mut listed: Vec<&str> = UNPLACED_IMPLS.to_vec();
    listed.sort_unstable();
    assert_eq!(
        found, listed,
        "the impl blocks of types the module tree does not hold are the named list (UNPLACED_IMPLS)"
    );
}

/// One row of the unfollowed-edges list: a site's key and how many calls stand behind it.
type SiteKey = (String, String, String, String);

/// **The unfollowed edges of the real tree, by site**: walking from every product callable of
/// bt-app, every `Dynamic` edge and every `Ambiguous` edge with other than one candidate — the
/// calls the door reach does not follow (`window_waits_tests::followed`, whose `.into()` to the
/// product's `From` impls is followed and left out here too) — keyed by its file, the item (or
/// value) it is written in, its kind and the call's name.
fn unfollowed_sites(resolver: &Resolver<'_>) -> BTreeMap<SiteKey, usize> {
    let roots: Vec<ItemId> = resolver
        .callables("bt-app")
        .into_iter()
        .filter(|id| resolver.admitted(*id))
        .collect();
    let graph = CallGraph::new(resolver);
    let reach = graph.walk(&roots, &BTreeSet::new());
    let root = workspace_root()
        .canonicalize()
        .expect("the workspace root resolves");
    let mut sites: BTreeMap<SiteKey, usize> = BTreeMap::new();
    for (edge, _) in &reach.unresolved {
        let kind = match &edge.to {
            Target::Unresolved(Unresolved::Dynamic { .. }) => "Dynamic",
            Target::Unresolved(Unresolved::Ambiguous { candidates, .. })
                if candidates.len() != 1 && edge.site.name != "into" =>
            {
                "Ambiguous"
            }
            _ => continue,
        };
        let file = resolver
            .node_index(edge.from)
            .file_at(edge.site.span.start())
            .map(|file| {
                let path = file
                    .path()
                    .canonicalize()
                    .unwrap_or_else(|_| file.path().to_path_buf());
                path.strip_prefix(&root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .unwrap_or_default();
        *sites
            .entry((
                file,
                resolver.node_key(edge.from),
                kind.to_owned(),
                edge.site.name.clone(),
            ))
            .or_default() += 1;
    }
    sites
}

/// The committed list's path, from the workspace root.
fn unfollowed_list() -> PathBuf {
    workspace_root()
        .join("docs")
        .join("plans")
        .join("UNFOLLOWED-EDGES.tsv")
}

/// RED (round 3) — **the committed `docs/plans/UNFOLLOWED-EDGES.tsv` is the real tree's unfollowed
/// edges, site by site** ([`unfollowed_sites`]): a key the tree has and the list does not, a key
/// the list has and the tree does not, and a key whose `sites` differ are each named. Across
/// commits `scripts/ci/check-unfollowed-edges.ps1` holds the list to the merge base's (no key added
/// or grown without a dated reason). Every run leaves the rendering, the committed reasons kept,
/// in `target/unfollowed-edges/UNFOLLOWED-EDGES.tsv`; `scripts/generate-unfollowed-edges.ps1`
/// copies it.
///
/// MUTATION: delete one row of the committed list — the test names its key as in the tree and not
/// on the list.
#[test]
fn the_unfollowed_edges_are_the_committed_list() {
    let held = real_tree();
    let packages: Vec<(&str, &Index)> = held
        .iter()
        .map(|(name, index)| (name.as_str(), &**index))
        .collect();
    let resolver =
        Resolver::new(&packages, Arms::product()).unwrap_or_else(|refusals| panic!("{refusals:?}"));
    let sites = unfollowed_sites(&resolver);
    let committed_text = std::fs::read_to_string(unfollowed_list()).unwrap_or_default();
    let mut committed: BTreeMap<SiteKey, (usize, String)> = BTreeMap::new();
    for line in committed_text.lines() {
        if line.is_empty() || line.starts_with('#') || line.starts_with("file\t") {
            continue;
        }
        let cells: Vec<&str> = line.split('\t').collect();
        assert_eq!(
            cells.len(),
            6,
            "UNFOLLOWED-EDGES.tsv: a row of six columns: {line}"
        );
        let count: usize = cells[4]
            .parse()
            .unwrap_or_else(|_| panic!("UNFOLLOWED-EDGES.tsv: a count in `sites`: {line}"));
        let key = (
            cells[0].to_owned(),
            cells[1].to_owned(),
            cells[2].to_owned(),
            cells[3].to_owned(),
        );
        assert!(
            !committed.contains_key(&key),
            "UNFOLLOWED-EDGES.tsv carries {key:?} on two rows; each key has one row"
        );
        committed.insert(key, (count, cells[5].to_owned()));
    }
    let mut rendering = String::from(
        "# UNFOLLOWED-EDGES - generated by scripts/generate-unfollowed-edges.ps1 from bt_source::call_graph\n\
         # (T-BT-SOURCE-CALL-GRAPH round 3): every Dynamic edge and every Ambiguous edge with other than one\n\
         # candidate met walking from every product callable of bt-app, by site (file, item, kind, the call's\n\
         # name) with the number of calls behind it. Held equal to the tree by bt-source's call_graph test;\n\
         # shrink-only against the merge base by scripts/ci/check-unfollowed-edges.ps1 - a key added or grown\n\
         # carries a reason 'YYYY-MM-DD: <why>'. Never edit a key or a count by hand.\n\
         file\titem\tkind\tname\tsites\treason\n",
    );
    for (key, count) in &sites {
        let reason = committed.get(key).map_or("", |(_, reason)| reason.as_str());
        rendering.push_str(&format!(
            "{}\t{}\t{}\t{}\t{count}\t{reason}\n",
            key.0, key.1, key.2, key.3
        ));
    }
    let out = workspace_root().join("target").join("unfollowed-edges");
    std::fs::create_dir_all(&out).expect("target/unfollowed-edges");
    std::fs::write(out.join("UNFOLLOWED-EDGES.tsv"), &rendering).expect("the rendering is written");
    let mut differences = Vec::new();
    for (key, count) in &sites {
        match committed.get(key) {
            None => differences.push(format!("in the tree, not on the list: {key:?} ({count})")),
            Some((listed, _)) if listed != count => {
                differences.push(format!(
                    "{key:?}: {count} in the tree, {listed} on the list"
                ));
            }
            Some(_) => {}
        }
    }
    for key in committed.keys() {
        if !sites.contains_key(key) {
            differences.push(format!("on the list, not in the tree: {key:?}"));
        }
    }
    eprintln!(
        "UNFOLLOWED_SITES\tkeys {}\tcalls {}",
        sites.len(),
        sites.values().sum::<usize>()
    );
    assert!(
        differences.is_empty(),
        "docs/plans/UNFOLLOWED-EDGES.tsv and the tree disagree ({} differences; the rendering is in \
         target/unfollowed-edges/, regenerate with scripts/generate-unfollowed-edges.ps1):\n  {}",
        differences.len(),
        differences
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

// ── round 4 ───────────────────────────────────────────────────────────────

/// RED (round 4) — **a function-pointer value whose initializer names the value again is read
/// once**: `const F: fn() = { fn g() { F() } g };` and `static S: fn() = || S();` resolve without
/// recursing (the call inside its own initializer is dynamic, its candidates the outer reading's),
/// `F()` names `g` and the walk ends; and an associated function-pointer `const` called as
/// `Self::H()` or `Holder::H()` reaches its initializer's function, the call dynamic with it as
/// the candidate.
///
/// MUTATION: in `Walker::called_values`, drop the guard (read every value whatever is open) — the
/// self-referential readings recurse until the stack overflows and the test process dies.
#[test]
fn a_self_referential_function_pointer_value_is_read_once() {
    let index = fixture("value_cycle");
    let resolver = resolver("value_cycle", &index);
    let graph = CallGraph::new(&resolver);
    let package = "cg-value_cycle";
    let root = item(&resolver, package, "root");
    let f = at(&resolver, "root", "F");
    assert!(
        f.iter().any(|spelled| spelled.starts_with("value ")),
        "`F()` reaches `F`'s initializer: {f:?}"
    );
    assert!(
        f.iter().any(|spelled| spelled.starts_with("dynamic ")),
        "`F()` is dynamic: {f:?}"
    );
    let s = at(&resolver, "root", "S");
    assert!(
        s.iter().any(|spelled| spelled.starts_with("value ")),
        "{s:?}"
    );
    let reach = graph.walk(&[root], &BTreeSet::new());
    assert_eq!(reach.values.len(), 2, "both initializers are reached once");
    let call = item(&resolver, package, "call");
    let spelled = at(&resolver, "call", "H");
    assert_eq!(
        spelled
            .iter()
            .filter(|spelled| *spelled == "dynamic cg-value_cycle crate::target")
            .count(),
        2,
        "`Self::H()` and `Holder::H()` are dynamic with the initializer's function: {spelled:?}"
    );
    let reached = graph.walk(&[call], &BTreeSet::new());
    assert!(
        keys(
            &resolver,
            &reached.reached.iter().copied().collect::<Vec<_>>()
        )
        .contains(&"cg-value_cycle crate::target".to_owned()),
        "the associated value's initializer reaches `target`"
    );
}
