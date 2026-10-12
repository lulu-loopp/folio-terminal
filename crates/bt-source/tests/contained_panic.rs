//! **No bare `catch_unwind` in the product** (T-PANIC-ROWS-HONEST): every catch of a panic in
//! `bt-app`, `bt-platform` and `bt-pty` is the one boundary, `bt_platform::contained_panic::contain`,
//! or sits in a closure handed to it.
//!
//! The resident application's panic hook ends the process from inside the hook, before any
//! unwinding, unless its one decision — `the_hook_lets_this_panic_unwind` — says the panic is
//! contained. A bare `std::panic::catch_unwind` is therefore reached only where no such hook is
//! installed: in a test, never in Folio, so the failure value it builds is decorative there.
//! `contain` marks the thread for the length of its work, which is what the hook asks; it is the
//! only catch the hook lets a panic reach.
//!
//! # What it reads, exactly
//!
//! Each package's own sources ([`Index::of_package`]), and in them every **product** occurrence
//! of the identifier `catch_unwind` ([`bt_source::Occurrence::in_the_product`]: test files,
//! `#[cfg(test)]` items and inline test modules are not read) — a call, a path handed on as a
//! value, and a `use` of it under any name, so an import renamed out of sight is refused where it
//! is written. An occurrence is allowed when
//!
//! * its innermost item **is** the boundary — the item identity [`BOUNDARY`] (package, module
//!   path, name), never a spelling; or
//! * the innermost closure around it is an argument of a call whose path
//!   [`FunctionPaths::functions_named`] follows — through the module's `use` items and the crates'
//!   re-exports, `bt-app`'s `contained_panic` included — to exactly that item: **a call edge**.
//!
//! So a function named `contain` in a module named `contained_panic` that is not the item is
//! refused, its body and its callers alike; an alias of the real item is followed to it; and a
//! closure handed to some other function that then calls the boundary is refused — the rule is a
//! direct edge from the closure's call to the boundary, not what the callee does with it.
//! Anything else is refused by `path:line:column`. There is no allowlist: a thread whose death is
//! meant to be the process's has no catch at all, and its `# completions` row says that a panic
//! ends the process under the resident hook (`crates/bt-app/src/window_waits.tsv`).
//!
//! # What it does not see
//!
//! A call written inside a macro's token tree is not parsed, and a `use` written inside a function
//! body is not followed, so a catch handed to the boundary that way is refused, never allowed; and
//! `bt-math`'s render boundary, which the hook honours separately through
//! `render_panic_is_contained`, is outside the three packages.
//!
//! # Joined threads (T-JOINED-PANIC-VALUES)
//!
//! The same hook decides what a `JoinHandle::join` can answer: a thread that panics outside
//! `contain` ends the process before its joiner returns, so a join's `Err` is real only for a
//! thread whose body is contained. [`no_product_join_makes_a_value_of_an_uncontained_panic`] reads
//! every product occurrence of the identifier `join` in the seven product packages
//! ([`JOIN_PACKAGES`]), and **every occurrence is one of the shapes below or is refused** — an
//! unsupported shape is a diagnostic, never a silent pass:
//!
//! * `h.join()` (any zero-argument method `join`) and `JoinHandle::join(h)`: **a thread's join**.
//!   Taken by its name and arity alone, which is sound only because of what follows: every such
//!   join is traced to a thread start or refused, so a `join()` that is not a thread's is refused
//!   too. Its result thrown away (`let _ =`, a statement, `drop`) or carried on as a panic
//!   (`unwrap`, `expect`) is not judged further. Anything else makes a value of the `Err`, and is
//!   allowed only when the handle follows to a thread start whose closure is a **call edge** to
//!   the boundary — the closure's whole body one call that [`FunctionPaths::functions_named`]
//!   follows to exactly [`BOUNDARY`].
//! * A `join` that takes arguments other than one handle (`Path::join`, a slice's `join`):
//!   `JoinHandle::join` takes none, so no thread's. A function named `join`, where it is
//!   declared, is no join either.
//! * `T::join(x)` through a type path that is not `JoinHandle`'s: a foreign crate's operation only
//!   when the path is written **in full from that crate's root** — its first segment is no
//!   keyword root, no `std`/`core`/`alloc`, no workspace crate, no name a `use` brings into scope
//!   there (and no glob `use` is in scope there), no item or module of the package; the tree compiling,
//!   that leaves a crate of the extern prelude. Any other path-called `join` is refused.
//!   (`bt-platform`'s WinRT deployment waits are written so:
//!   `windows_future::IAsyncOperationWithProgress::join(&operation)`.)
//! * A `join` in a macro's tokens, which no parser reads: with a non-empty argument group, no
//!   thread's; any other is refused.
//! * Any other occurrence — `JoinHandle::join` handed on as a value, a binding named `join` — is
//!   refused.
//!
//! A handle is followed through `let` bindings (whose pattern is the whole value, `Some(x)` or
//! `Ok(x)`), the values given to a `self` field anywhere in its package (struct literals and
//! assignments), the tail of a free function the path names, and the value-keeping adapters of
//! `Option` and `Result` (`unwrap`, `expect`, `take`, `map_err`, `?`). A thread start is `std`'s
//! `thread::spawn` written by that path (`std` is not indexed), a `spawn` method on a
//! `thread::Builder::new()`, or the thread door ([`DOORS`], by identity). Everything else is
//! **unfollowed and refused, naming the site**: a parameter, a match arm's or a tuple's part, a
//! field of anything but `self`, any other method's value (a collection's `pop`), a call that
//! follows to no workspace free function and starts no thread (an associated function, a renamed
//! `use` of `spawn`, a foreign crate's call).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

use bt_source::{
    DiskScope, FunctionItem, FunctionPaths, Index, ItemKind, ItemRecord, Pattern, Search, Span,
    TargetId, TargetKind, TargetRoot, Universe, Vendor, View, Workspace, needle, report,
};
use syn::spanned::Spanned as _;

/// The packages whose product code the guard reads.
const PACKAGES: [&str; 3] = ["bt-app", "bt-platform", "bt-pty"];

/// The boundary's identity: package, module path, name.
const BOUNDARY: (&str, &str, &str) = ("bt-platform", "crate::contained_panic", "contain");

fn boundary() -> FunctionItem {
    FunctionItem {
        package: BOUNDARY.0.to_owned(),
        module: BOUNDARY.1.to_owned(),
        name: BOUNDARY.2.to_owned(),
    }
}

/// One closure of a file: its byte range there, and — when it is an argument of a call written
/// as a path — that path and where the call is written.
struct Closure {
    range: Range<usize>,
    handed_to: Option<(Vec<String>, usize)>,
}

/// Every closure of one file.
#[derive(Default)]
struct Closures {
    found: Vec<Closure>,
    /// The closures that are arguments of a path call, by their start, with that call.
    arguments: BTreeMap<usize, (Vec<String>, usize)>,
}

impl<'ast> syn::visit::Visit<'ast> for Closures {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*call.func
            && path.qself.is_none()
        {
            let segments: Vec<String> = path
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            let at = call.span().byte_range().start;
            for argument in &call.args {
                if let syn::Expr::Closure(closure) = argument {
                    self.arguments
                        .insert(closure.span().byte_range().start, (segments.clone(), at));
                }
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
        let range = closure.span().byte_range();
        let handed_to = self.arguments.get(&range.start).cloned();
        self.found.push(Closure { range, handed_to });
        syn::visit::visit_expr_closure(self, closure);
    }
}

/// What one index answers: how many catches stand at the boundary, and every other one by
/// `path:line:column`, sorted.
struct Reading {
    allowed: usize,
    refused: Vec<String>,
}

/// The smallest item whose bytes hold `span`.
fn innermost_item(index: &Index, span: Span) -> Option<&ItemRecord> {
    index
        .items()
        .iter()
        .filter(|record| span.within(record.whole()))
        .min_by_key(|record| record.whole().len())
}

/// Whether `item` is the boundary itself, by identity.
fn is_the_boundary(index: &Index, item: &ItemRecord) -> bool {
    let package = &index.file_of(item).owners()[0].target.package;
    item.kind() == ItemKind::Function
        && item.type_owner().is_none()
        && package == BOUNDARY.0
        && item.name() == BOUNDARY.2
        && item.module_paths().contains(&BOUNDARY.1)
}

/// Every product `catch_unwind` of `index`, judged with `paths`; a refused site's path is written
/// relative to `root`.
fn read(index: &Index, paths: &FunctionPaths, root: &Path) -> Reading {
    let found = index
        .search(&Search::new(
            needle!(Pattern::identifier("catch_unwind")),
            View::Identifiers,
        ))
        .unwrap_or_else(|failure| panic!("{failure}"))
        .in_the_product(index);
    let mut closures: BTreeMap<usize, Vec<Closure>> = BTreeMap::new();
    let mut reading = Reading {
        allowed: 0,
        refused: Vec::new(),
    };
    for occurrence in found.occurrences() {
        let span = occurrence.span;
        let file = index
            .file_at(span.start())
            .expect("an occurrence is in a file");
        let base = file.span().start();
        let in_file = closures.entry(base).or_insert_with(|| {
            let parsed = syn::parse_file(index.text(file.span()))
                .unwrap_or_else(|error| panic!("{}: {error}", file.path().display()));
            let mut visitor = Closures::default();
            syn::visit::Visit::visit_file(&mut visitor, &parsed);
            visitor.found
        });
        let innermost_closure = in_file
            .iter()
            .filter(|closure| {
                base + closure.range.start <= span.start() && span.end() <= base + closure.range.end
            })
            .min_by_key(|closure| closure.range.len());
        let handed_to_the_boundary = innermost_closure
            .and_then(|closure| closure.handed_to.as_ref())
            .is_some_and(|(path, at)| {
                paths.functions_named(index, base + at, path) == BTreeSet::from([boundary()])
            });
        let in_the_boundary =
            innermost_item(index, span).is_some_and(|item| is_the_boundary(index, item));
        if handed_to_the_boundary || in_the_boundary {
            reading.allowed += 1;
            continue;
        }
        let location = index
            .locate(span.start())
            .expect("an occurrence is in a file");
        let relative = location
            .file
            .strip_prefix(root)
            .unwrap_or(location.file)
            .to_string_lossy()
            .replace('\\', "/");
        reading
            .refused
            .push(format!("{relative}:{}:{}", location.line, location.column));
    }
    reading.refused.sort();
    reading
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// RED (T-PANIC-ROWS-HONEST) — **every catch of a panic in the product is the contained-panic
/// boundary or in a closure handed to it.**
///
/// MUTATION: put `pty_door::request`'s birth back on a bare `std::panic::catch_unwind` (or plant
/// any bare catch in the three packages' product code) and this names its line.
#[test]
fn no_product_catch_unwind_is_outside_the_contained_panic_boundary() {
    let workspace =
        Workspace::read(&workspace_root()).unwrap_or_else(|rejection| panic!("{rejection}"));
    let indexes: Vec<&Index> = PACKAGES
        .iter()
        .map(|package| Index::of_package(package))
        .collect();
    let paths = FunctionPaths::read(&indexes);
    let platform = Index::of_package(BOUNDARY.0);
    assert!(
        platform
            .items()
            .iter()
            .any(|item| is_the_boundary(platform, item)),
        "the boundary {BOUNDARY:?} is not an item of the product — the guard has nothing to allow"
    );
    let mut refused = Vec::new();
    let mut allowed = 0;
    for (package, index) in PACKAGES.iter().zip(&indexes) {
        assert!(!index.files().is_empty(), "{package}: no file read");
        let reading = read(index, &paths, workspace.root());
        println!(
            "contained-panic guard: {package}: {} file(s), {} catch(es) at the boundary, {} \
             refused",
            index.files().len(),
            reading.allowed,
            reading.refused.len()
        );
        allowed += reading.allowed;
        refused.extend(reading.refused);
    }
    assert!(
        allowed > 0,
        "the boundary's own catch was not found — a guard that found nothing has proved nothing"
    );
    assert!(
        refused.is_empty(),
        "a bare `catch_unwind` in the product ({}): under the resident panic hook the process \
         ends before it is reached. Run the work inside `contained_panic::contain`, or, where \
         the panic is meant to end the process, catch nothing and say so in the protocol's \
         `# completions` row:\n  {}",
        refused.len(),
        refused.join("\n  ")
    );
}

/// The `contained_panic` fixture's directory.
fn fixture_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("contained_panic")
}

/// The `contained_panic` fixture, indexed as a crate called `lookalike`.
fn fixture() -> Index {
    let directory = fixture_directory();
    let universe = Universe::declare(
        "the contained-panic fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "lookalike".to_owned(),
                kind: TargetKind::Library,
                name: "lookalike".to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

/// RED (T-PANIC-ROWS-HONEST round 2) — **a catch is allowed by a call edge to the real boundary,
/// never by a name.** The fixture, read beside `bt-platform`, hands two catches to the real item —
/// through a renamed `use` and by its path — and plants five the guard refuses: the body of a
/// lookalike `contained_panic::contain` and a closure handed to it, a closure handed to a helper
/// that itself calls the boundary, a bare call, and an import renamed out of sight (a test's catch
/// is not read).
///
/// MUTATIONS: ① judge the call by its spelling (a path ending `contained_panic::contain`, as round
/// 1 did) — the lookalike's caller is allowed; ② follow no `use` item (judge only paths written in
/// full) — the alias is refused.
#[test]
fn a_catch_is_allowed_by_a_call_edge_to_the_boundary_never_by_a_name() {
    let index = fixture();
    let paths = FunctionPaths::read(&[&index, Index::of_package(BOUNDARY.0)]);
    let reading = read(&index, &paths, &fixture_directory());
    assert_eq!(
        reading.refused,
        [
            "lib.rs:10:29",
            "lib.rs:28:52",
            "lib.rs:38:27",
            "lib.rs:43:17",
            "lib.rs:5:17"
        ],
        "the lookalike's body and its caller, the helper's closure, the bare call and the renamed \
         import"
    );
    assert_eq!(
        reading.allowed, 2,
        "the alias and the path, each a call edge to the real item"
    );
}

// ── Joined threads ──────────────────────────────────────────────────────────────────────────────

/// The packages whose product joins the join guard reads: the audit's (T-JOINED-PANIC-VALUES).
const JOIN_PACKAGES: [&str; 7] = [
    "bt-app",
    "bt-platform",
    "bt-pty",
    "bt-term",
    "bt-render",
    "bt-corpus",
    "bt-persist",
];

/// The thread door's two items (`bt_platform::spawn_at_priority`, `…_with_stack`), by identity:
/// a call that follows to one of them starts a thread whose body is its last argument.
const DOORS: [(&str, &str, &str); 2] = [
    ("bt-platform", "crate::admission", "spawn_at_priority"),
    (
        "bt-platform",
        "crate::admission",
        "spawn_at_priority_with_stack",
    ),
];

/// How many steps — a `let`, a field, a function's tail, a method's receiver — one handle is
/// followed through.
const ORIGIN_STEPS: usize = 12;

fn is_door(item: &FunctionItem) -> bool {
    DOORS.iter().any(|(package, module, name)| {
        item.package == *package && item.module == *module && item.name == *name
    })
}

/// What a join's result is made into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JoinUse {
    /// Thrown away: `let _ = h.join();`, `h.join();`, `drop(h.join())`.
    Discarded,
    /// Carried on as a panic: `h.join().unwrap()`, `h.join().expect(…)`.
    CarriedOn,
    /// Anything else — a `match`, an `if let`, a `map_err`, an `unwrap_or_default`, a `?`: the
    /// join's `Err` becomes a value.
    Value,
}

/// A name bound in a function, the bytes it is in scope over, and what it was bound to — `None`
/// for a binding whose value is not one expression (a parameter, a match arm, a tuple's part).
struct Binding {
    name: String,
    at: usize,
    scope: Range<usize>,
    origin: Option<syn::Expr>,
}

/// One function body of a file: its bytes, the type of the `impl` it is in, its bindings, and —
/// for a free function — its name with where it is written, and its tail expression.
struct Function {
    range: Range<usize>,
    self_type: Option<String>,
    free_name: Option<(String, usize)>,
    tail: Option<syn::Expr>,
    bindings: Vec<Binding>,
}

/// A value given to a field: a struct literal's field, or an assignment to `self.<field>`.
struct FieldValue {
    type_name: String,
    field: String,
    value: syn::Expr,
}

/// One join of a file: its receiver, and what its result is made into.
struct Join {
    receiver: syn::Expr,
    used: JoinUse,
}

/// What an occurrence of the identifier `join` is when it is not a thread's join to judge.
#[derive(Clone, Debug)]
enum Shape {
    /// A `join` called with arguments other than one handle (`Path::join`, a slice's `join`):
    /// `JoinHandle::join` takes none, so no thread's.
    Arguments,
    /// A function named `join`, where it is declared.
    Definition,
    /// A name that is not called: a binding, a field, a local's value (`let (join, run) = …`,
    /// `layout.join`).
    Name,
    /// A `join` called through a type path (`T::join(x)`) that is not `JoinHandle`'s: a foreign
    /// crate's only when the path is written in full from that crate's root (judged by the reader).
    PathCall(Vec<String>),
    /// A shape the reading refuses, with why.
    Refused(String),
}

/// What one file holds for the join guard, at the file's own byte offsets.
#[derive(Default)]
struct FileJoins {
    functions: Vec<Function>,
    fields: Vec<FieldValue>,
    /// By the offset of the `join` identifier.
    joins: BTreeMap<usize, Join>,
    /// Every other parsed occurrence of the identifier `join`, by its offset.
    shapes: BTreeMap<usize, Shape>,
    /// Every `use` item's names (an alias by its new name, `*` for a glob): the bytes of the
    /// module or block it is written in, and the start of the module that holds it.
    uses: Vec<(Range<usize>, usize, String)>,
    /// The bytes of every inline module of the file.
    modules: Vec<Range<usize>>,
}

impl FileJoins {
    /// The start of the innermost module around byte `at` (the file's: 0).
    fn module_at(&self, at: usize) -> usize {
        self.modules
            .iter()
            .filter(|module| module.contains(&at))
            .min_by_key(|module| module.len())
            .map_or(0, |module| module.start)
    }

    /// The names `use` items bring into scope at byte `at`: those written in the innermost module
    /// around it, or in a block of that module around it — a parent module's `use` is not in
    /// scope in a child module.
    fn names_in_scope(&self, at: usize) -> BTreeSet<&str> {
        let module = self.module_at(at);
        self.uses
            .iter()
            .filter(|(scope, held_by, _)| scope.contains(&at) && *held_by == module)
            .map(|(_, _, name)| name.as_str())
            .collect()
    }
}

impl FileJoins {
    /// The innermost function holding byte `at`.
    fn function_at(&self, at: usize) -> Option<&Function> {
        self.functions
            .iter()
            .filter(|function| function.range.contains(&at))
            .min_by_key(|function| function.range.len())
    }
}

/// The visitor that fills a [`FileJoins`].
#[derive(Default)]
struct JoinVisitor {
    found: FileJoins,
    /// Joins whose result is thrown away or carried on, by their `join`'s offset.
    uses: BTreeMap<usize, JoinUse>,
    impl_types: Vec<String>,
    functions: Vec<usize>,
    blocks: Vec<Range<usize>>,
    /// The scopes a `use` can be written in, innermost last: the file, inline modules, blocks.
    scopes: Vec<Range<usize>>,
    /// The starts of the modules being visited, innermost last (the file's: 0).
    module_starts: Vec<usize>,
}

/// A path's last segment.
fn last_segment(path: &syn::Path) -> Option<String> {
    path.segments
        .last()
        .map(|segment| segment.ident.to_string())
}

/// The path of a call written as a path, segment by segment.
fn call_path(call: &syn::ExprCall) -> Option<Vec<String>> {
    match &*call.func {
        syn::Expr::Path(path) if path.qself.is_none() => Some(
            path.path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect(),
        ),
        _ => None,
    }
}

/// `expression` without the parentheses around it.
fn unparenthesised(expression: &syn::Expr) -> &syn::Expr {
    match expression {
        syn::Expr::Paren(inner) => unparenthesised(&inner.expr),
        other => other,
    }
}

/// When `expression` is a join — `h.join()`, or `JoinHandle::join(h)` — the offset of its `join`
/// identifier, and its receiver.
fn as_join(expression: &syn::Expr) -> Option<(usize, &syn::Expr)> {
    match unparenthesised(expression) {
        syn::Expr::MethodCall(call) if call.method == "join" && call.args.is_empty() => {
            Some((call.method.span().byte_range().start, &call.receiver))
        }
        syn::Expr::Call(call) if call.args.len() == 1 => {
            let syn::Expr::Path(path) = &*call.func else {
                return None;
            };
            let segments: Vec<&syn::PathSegment> = path.path.segments.iter().collect();
            match segments.as_slice() {
                [.., owner, join] if owner.ident == "JoinHandle" && join.ident == "join" => {
                    Some((join.ident.span().byte_range().start, &call.args[0]))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Every name `pattern` binds, with where it is written and whether it is bound to the whole
/// value (`x`, `mut x`, `Some(x)`, `Ok(x)`, `x: T`) — only then is the name the value's.
fn bound_names(pattern: &syn::Pat, out: &mut Vec<(String, usize, bool)>, whole: bool) {
    struct Names<'a>(&'a mut Vec<(String, usize, bool)>);
    impl<'ast> syn::visit::Visit<'ast> for Names<'_> {
        fn visit_pat_ident(&mut self, ident: &'ast syn::PatIdent) {
            self.0.push((
                ident.ident.to_string(),
                ident.ident.span().byte_range().start,
                false,
            ));
            syn::visit::visit_pat_ident(self, ident);
        }
    }
    match pattern {
        syn::Pat::Ident(ident) if ident.subpat.is_none() => out.push((
            ident.ident.to_string(),
            ident.ident.span().byte_range().start,
            whole,
        )),
        syn::Pat::Type(typed) => bound_names(&typed.pat, out, whole),
        syn::Pat::Paren(inner) => bound_names(&inner.pat, out, whole),
        syn::Pat::TupleStruct(tuple)
            if tuple.elems.len() == 1
                && tuple
                    .path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "Some" || segment.ident == "Ok") =>
        {
            bound_names(&tuple.elems[0], out, whole);
        }
        other => syn::visit::Visit::visit_pat(&mut Names(out), other),
    }
}

/// Every `join` in a macro's tokens, which no parser reads: one followed by a non-empty
/// parenthesised group takes arguments and is no thread's; any other — `.join()` above all — is
/// refused, since the reading cannot judge what its result is made into.
fn macro_joins(tokens: proc_macro2::TokenStream, shapes: &mut BTreeMap<usize, Shape>) {
    let trees: Vec<proc_macro2::TokenTree> = tokens.into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        match tree {
            proc_macro2::TokenTree::Group(group) => macro_joins(group.stream(), shapes),
            proc_macro2::TokenTree::Ident(ident) if ident == "join" => {
                let arguments = match trees.get(at + 1) {
                    Some(proc_macro2::TokenTree::Group(group))
                        if group.delimiter() == proc_macro2::Delimiter::Parenthesis =>
                    {
                        Some(!group.stream().is_empty())
                    }
                    _ => None,
                };
                let after_a_path = at >= 2
                    && matches!((&trees[at - 2], &trees[at - 1]),
                        (proc_macro2::TokenTree::Punct(a), proc_macro2::TokenTree::Punct(b))
                            if a.as_char() == ':' && b.as_char() == ':');
                let shape = if arguments == Some(true) {
                    Shape::Arguments
                } else if arguments.is_none() && !after_a_path {
                    Shape::Name
                } else {
                    Shape::Refused(
                        "a `join` in a macro's tokens, which the reading does not parse".to_owned(),
                    )
                };
                shapes.insert(ident.span().byte_range().start, shape);
            }
            _ => {}
        }
    }
}

impl JoinVisitor {
    /// `ident`, when it is `join`, as a name that is not called — unless a call already took it.
    fn name(&mut self, ident: &syn::Ident) {
        if ident == "join" {
            self.found
                .shapes
                .entry(ident.span().byte_range().start)
                .or_insert(Shape::Name);
        }
    }

    fn bind(&mut self, pattern: &syn::Pat, scope: &Range<usize>, origin: Option<&syn::Expr>) {
        let Some(&function) = self.functions.last() else {
            return;
        };
        let mut names = Vec::new();
        bound_names(pattern, &mut names, true);
        for (name, at, whole) in names {
            self.found.functions[function].bindings.push(Binding {
                name,
                at,
                scope: scope.clone(),
                origin: origin.filter(|_| whole).cloned(),
            });
        }
    }

    fn enter_function(
        &mut self,
        range: Range<usize>,
        free_name: Option<(String, usize)>,
        inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::Token![,]>,
        body: &syn::Block,
    ) {
        let tail = match body.stmts.last() {
            Some(syn::Stmt::Expr(tail, None)) => Some(tail.clone()),
            _ => None,
        };
        self.found.functions.push(Function {
            range: range.clone(),
            self_type: self.impl_types.last().cloned(),
            free_name,
            tail,
            bindings: Vec::new(),
        });
        self.functions.push(self.found.functions.len() - 1);
        for input in inputs {
            if let syn::FnArg::Typed(typed) = input {
                self.bind(&typed.pat, &range, None);
            }
        }
    }

    /// The `let`s of a condition (a `let` chain's too), bound over `scope`.
    fn bind_condition(&mut self, condition: &syn::Expr, scope: &Range<usize>) {
        match condition {
            syn::Expr::Let(binding) => self.bind(&binding.pat, scope, Some(&binding.expr)),
            syn::Expr::Binary(binary) if matches!(binary.op, syn::BinOp::And(_)) => {
                self.bind_condition(&binary.left, scope);
                self.bind_condition(&binary.right, scope);
            }
            syn::Expr::Paren(inner) => self.bind_condition(&inner.expr, scope),
            _ => {}
        }
    }
}

impl<'ast> syn::visit::Visit<'ast> for JoinVisitor {
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let name = match &*item.self_ty {
            syn::Type::Path(path) => last_segment(&path.path).unwrap_or_default(),
            _ => String::new(),
        };
        self.impl_types.push(name);
        syn::visit::visit_item_impl(self, item);
        self.impl_types.pop();
    }

    fn visit_signature(&mut self, signature: &'ast syn::Signature) {
        if signature.ident == "join" {
            self.found
                .shapes
                .insert(signature.ident.span().byte_range().start, Shape::Definition);
        }
        syn::visit::visit_signature(self, signature);
    }

    fn visit_file(&mut self, file: &'ast syn::File) {
        self.scopes.push(0..usize::MAX);
        self.module_starts.push(0);
        syn::visit::visit_file(self, file);
        self.module_starts.pop();
        self.scopes.pop();
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        let range = module.span().byte_range();
        self.found.modules.push(range.clone());
        self.module_starts.push(range.start);
        self.scopes.push(range);
        syn::visit::visit_item_mod(self, module);
        self.scopes.pop();
        self.module_starts.pop();
    }

    fn visit_use_tree(&mut self, tree: &'ast syn::UseTree) {
        let scope = self.scopes.last().cloned().unwrap_or(0..usize::MAX);
        let module = self.module_starts.last().copied().unwrap_or(0);
        let name = match tree {
            syn::UseTree::Name(name) => Some(name.ident.to_string()),
            syn::UseTree::Rename(rename) => Some(rename.rename.to_string()),
            syn::UseTree::Glob(_) => Some("*".to_owned()),
            syn::UseTree::Path(_) | syn::UseTree::Group(_) => None,
        };
        if let Some(name) = name {
            self.found.uses.push((scope, module, name));
        }
        syn::visit::visit_use_tree(self, tree);
    }

    fn visit_pat_ident(&mut self, ident: &'ast syn::PatIdent) {
        self.name(&ident.ident);
        syn::visit::visit_pat_ident(self, ident);
    }

    fn visit_field(&mut self, field: &'ast syn::Field) {
        if let Some(ident) = &field.ident {
            self.name(ident);
        }
        syn::visit::visit_field(self, field);
    }

    fn visit_member(&mut self, member: &'ast syn::Member) {
        if let syn::Member::Named(ident) = member {
            self.name(ident);
        }
        syn::visit::visit_member(self, member);
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        if path.qself.is_none()
            && let Some(ident) = path.path.get_ident()
        {
            self.name(ident);
        }
        syn::visit::visit_expr_path(self, path);
    }

    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        macro_joins(invocation.tokens.clone(), &mut self.found.shapes);
        syn::visit::visit_macro(self, invocation);
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let name = (
            item.sig.ident.to_string(),
            item.sig.ident.span().byte_range().start,
        );
        // A function item written inside a method belongs to no `impl`.
        let impl_types = std::mem::take(&mut self.impl_types);
        self.enter_function(
            item.span().byte_range(),
            Some(name),
            &item.sig.inputs,
            &item.block,
        );
        syn::visit::visit_item_fn(self, item);
        self.functions.pop();
        self.impl_types = impl_types;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.enter_function(
            item.span().byte_range(),
            None,
            &item.sig.inputs,
            &item.block,
        );
        syn::visit::visit_impl_item_fn(self, item);
        self.functions.pop();
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        if let Some(body) = &item.default {
            self.enter_function(item.span().byte_range(), None, &item.sig.inputs, body);
            syn::visit::visit_trait_item_fn(self, item);
            self.functions.pop();
        }
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.blocks.push(block.span().byte_range());
        self.scopes.push(block.span().byte_range());
        syn::visit::visit_block(self, block);
        self.scopes.pop();
        self.blocks.pop();
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        let start = local.span().byte_range().start;
        let end = self.blocks.last().map_or(usize::MAX, |block| block.end);
        let init = local.init.as_ref().map(|init| &*init.expr);
        if let (syn::Pat::Wild(_), Some(init)) = (&local.pat, init)
            && let Some((at, _)) = as_join(init)
        {
            self.uses.insert(at, JoinUse::Discarded);
        }
        self.bind(&local.pat, &(start..end), init);
        syn::visit::visit_local(self, local);
    }

    fn visit_stmt(&mut self, statement: &'ast syn::Stmt) {
        if let syn::Stmt::Expr(expression, Some(_)) = statement
            && let Some((at, _)) = as_join(expression)
        {
            self.uses.insert(at, JoinUse::Discarded);
        }
        syn::visit::visit_stmt(self, statement);
    }

    fn visit_expr_if(&mut self, branch: &'ast syn::ExprIf) {
        let scope = branch.then_branch.span().byte_range();
        self.bind_condition(&branch.cond, &scope);
        syn::visit::visit_expr_if(self, branch);
    }

    fn visit_expr_while(&mut self, looped: &'ast syn::ExprWhile) {
        let scope = looped.body.span().byte_range();
        self.bind_condition(&looped.cond, &scope);
        syn::visit::visit_expr_while(self, looped);
    }

    fn visit_expr_for_loop(&mut self, looped: &'ast syn::ExprForLoop) {
        self.bind(&looped.pat, &looped.body.span().byte_range(), None);
        syn::visit::visit_expr_for_loop(self, looped);
    }

    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        self.bind(&arm.pat, &arm.span().byte_range(), None);
        syn::visit::visit_arm(self, arm);
    }

    fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
        let scope = closure.span().byte_range();
        for input in &closure.inputs {
            self.bind(input, &scope, None);
        }
        syn::visit::visit_expr_closure(self, closure);
    }

    fn visit_expr_struct(&mut self, literal: &'ast syn::ExprStruct) {
        let named = last_segment(&literal.path);
        let type_name = match named.as_deref() {
            Some("Self") => self.impl_types.last().cloned(),
            _ => named,
        };
        if let Some(type_name) = type_name {
            for field in &literal.fields {
                if let syn::Member::Named(name) = &field.member {
                    self.found.fields.push(FieldValue {
                        type_name: type_name.clone(),
                        field: name.to_string(),
                        value: field.expr.clone(),
                    });
                }
            }
        }
        syn::visit::visit_expr_struct(self, literal);
    }

    fn visit_expr_assign(&mut self, assignment: &'ast syn::ExprAssign) {
        if let syn::Expr::Field(field) = &*assignment.left
            && let syn::Expr::Path(base) = &*field.base
            && base.path.is_ident("self")
            && let syn::Member::Named(name) = &field.member
            && let Some(type_name) = self.impl_types.last()
        {
            self.found.fields.push(FieldValue {
                type_name: type_name.clone(),
                field: name.to_string(),
                value: (*assignment.right).clone(),
            });
        }
        syn::visit::visit_expr_assign(self, assignment);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if (call.method == "unwrap" || call.method == "expect")
            && let Some((at, _)) = as_join(&call.receiver)
        {
            self.uses.insert(at, JoinUse::CarriedOn);
        }
        if call.method == "join" {
            let at = call.method.span().byte_range().start;
            if call.args.is_empty() {
                self.found.joins.insert(
                    at,
                    Join {
                        receiver: (*call.receiver).clone(),
                        used: JoinUse::Value,
                    },
                );
            } else {
                self.found.shapes.insert(at, Shape::Arguments);
            }
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if call_path(call).is_some_and(|path| path == ["drop"])
            && let Some(argument) = call.args.first()
            && let Some((at, _)) = as_join(argument)
        {
            self.uses.insert(at, JoinUse::Discarded);
        }
        if let syn::Expr::Path(path) = &*call.func
            && path.qself.is_none()
            && let Some(join) = path.path.segments.last()
            && join.ident == "join"
        {
            let at = join.ident.span().byte_range().start;
            let segments: Vec<String> = path
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            if call.args.len() == 1
                && segments.len() >= 2
                && segments[segments.len() - 2] == "JoinHandle"
            {
                self.found.joins.insert(
                    at,
                    Join {
                        receiver: call.args[0].clone(),
                        used: JoinUse::Value,
                    },
                );
            } else {
                self.found.shapes.insert(at, Shape::PathCall(segments));
            }
        }
        syn::visit::visit_expr_call(self, call);
    }
}

/// Where a joined handle came from.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Origin {
    /// A thread whose closure is a call edge to the boundary: under the resident hook its panic
    /// unwinds to the boundary, so the error its join is made into is real.
    Contained,
    /// A thread whose closure is not: its panic ends the process under the resident hook.
    Bare(String),
    /// A handle the reading cannot follow to where it was made.
    Unfollowed(String),
}

/// The workspace the join guard reads: its indexes, the paths over them, and each file's reading.
struct JoinReader<'a> {
    indexes: Vec<&'a Index>,
    paths: &'a FunctionPaths,
    /// The workspace's own crates, by the name a path writes (`bt_platform`).
    crates: BTreeSet<String>,
    files: std::cell::RefCell<BTreeMap<(usize, usize), std::rc::Rc<FileJoins>>>,
}

/// A file of the reader: its index's position, and the file's first byte in that index.
#[derive(Clone, Copy)]
struct Place {
    index: usize,
    base: usize,
}

impl<'a> JoinReader<'a> {
    fn new(indexes: Vec<&'a Index>, paths: &'a FunctionPaths, workspace: &Workspace) -> Self {
        Self {
            indexes,
            paths,
            crates: workspace
                .packages()
                .iter()
                .map(|package| package.name().replace('-', "_"))
                .collect(),
            files: std::cell::RefCell::default(),
        }
    }

    /// Whether `path`, a `join` called through a type path in the file at `place`, is written in
    /// full from a foreign crate's root: its first segment is none of `crate`, `self`, `super`,
    /// `Self`, `std`, `core`, `alloc`, a workspace crate, a name a `use` brings into scope there (and
    /// no glob `use` is in scope there), or an item or module of the package — which, the tree
    /// compiling, leaves only a crate of the extern prelude: a dependency's type, named.
    fn is_foreign_path(&self, place: Place, at: usize, path: &[String]) -> bool {
        let [first, _, .., _] = path else {
            return false;
        };
        let reading = self.file(place);
        let index = self.indexes[place.index];
        let in_scope = reading.names_in_scope(at);
        !["crate", "self", "super", "Self", "std", "core", "alloc"].contains(&first.as_str())
            && !self.crates.contains(first)
            && !in_scope.contains("*")
            && !in_scope.contains(first.as_str())
            && !index.items().iter().any(|item| item.name() == first)
            && !index.modules().iter().any(|module| {
                module
                    .module_paths()
                    .iter()
                    .any(|path| path.rsplit("::").next() == Some(first.as_str()))
            })
    }

    /// Whether `expression` — the receiver of a `spawn` method — is a `std::thread::Builder`:
    /// a `thread::Builder::new()` call, through the builder's own setters.
    fn is_a_thread_builder(expression: &syn::Expr) -> bool {
        match unparenthesised(expression) {
            syn::Expr::MethodCall(call) => Self::is_a_thread_builder(&call.receiver),
            syn::Expr::Call(call) => call_path(call).is_some_and(|path| {
                path.ends_with(&["thread".to_owned(), "Builder".to_owned(), "new".to_owned()])
            }),
            _ => false,
        }
    }

    /// The reading of the file at `place`.
    fn file(&self, place: Place) -> std::rc::Rc<FileJoins> {
        let mut files = self.files.borrow_mut();
        let reading = files.entry((place.index, place.base)).or_insert_with(|| {
            let index = self.indexes[place.index];
            let file = index.file_at(place.base).expect("a file starts here");
            let parsed = syn::parse_file(index.text(file.span()))
                .unwrap_or_else(|error| panic!("{}: {error}", file.path().display()));
            let mut visitor = JoinVisitor::default();
            syn::visit::Visit::visit_file(&mut visitor, &parsed);
            let mut found = visitor.found;
            for (at, used) in visitor.uses {
                if let Some(join) = found.joins.get_mut(&at) {
                    join.used = used;
                }
            }
            std::rc::Rc::new(found)
        });
        std::rc::Rc::clone(reading)
    }

    /// A site of the reader, `path:line:column` with the path relative to `root`.
    fn site(&self, index: usize, offset: usize, root: &Path) -> String {
        let location = self.indexes[index]
            .locate(offset)
            .expect("a site is in a file");
        let relative = location
            .file
            .strip_prefix(root)
            .unwrap_or(location.file)
            .to_string_lossy()
            .replace('\\', "/");
        format!("{relative}:{}:{}", location.line, location.column)
    }

    /// The thread whose body is `body`, an argument of the start at `start`: contained when
    /// `body` is a closure whose whole body is a call that follows to the boundary.
    fn judge_body(
        &self,
        place: Place,
        start: usize,
        body: Option<&syn::Expr>,
        root: &Path,
    ) -> Origin {
        let refused = || {
            Origin::Bare(format!(
                "the thread started at {} does not run its body inside the boundary",
                self.site(place.index, place.base + start, root)
            ))
        };
        let Some(syn::Expr::Closure(closure)) = body.map(unparenthesised) else {
            return refused();
        };
        let mut work = unparenthesised(&closure.body);
        while let syn::Expr::Block(block) = work
            && let [syn::Stmt::Expr(tail, None)] = block.block.stmts.as_slice()
        {
            work = unparenthesised(tail);
        }
        if let syn::Expr::Call(call) = work
            && let Some(path) = call_path(call)
            && self.paths.functions_named(
                self.indexes[place.index],
                place.base + call.span().byte_range().start,
                &path,
            ) == BTreeSet::from([boundary()])
        {
            return Origin::Contained;
        }
        refused()
    }

    /// The free function `item` of the reader's indexes: its file's place, the file's reading,
    /// and its position there.
    fn function_of(&self, item: &FunctionItem) -> Option<(Place, std::rc::Rc<FileJoins>, usize)> {
        for (at, index) in self.indexes.iter().enumerate() {
            for record in index.items() {
                let file = index.file_of(record);
                if record.kind() != ItemKind::Function
                    || record.type_owner().is_some()
                    || file.owners()[0].target.package != item.package
                    || record.name() != item.name
                    || !record.module_paths().contains(&item.module.as_str())
                {
                    continue;
                }
                let place = Place {
                    index: at,
                    base: file.span().start(),
                };
                let reading = self.file(place);
                let whole = record.whole();
                let found = reading.functions.iter().position(|function| {
                    function.free_name.as_ref().is_some_and(|(name, written)| {
                        *name == item.name
                            && whole.start() <= place.base + written
                            && place.base + written < whole.start() + whole.len()
                    })
                });
                if let Some(function) = found {
                    return Some((place, reading, function));
                }
            }
        }
        None
    }

    /// Every origin `expression`, written in the file at `place`, can have, followed at most
    /// `steps` more steps.
    fn origins(
        &self,
        place: Place,
        expression: &syn::Expr,
        steps: usize,
        root: &Path,
    ) -> Vec<Origin> {
        let offset = expression.span().byte_range().start;
        let unfollowed = |what: &str| {
            vec![Origin::Unfollowed(format!(
                "{what} at {}",
                self.site(place.index, place.base + offset, root)
            ))]
        };
        let Some(steps) = steps.checked_sub(1) else {
            return unfollowed("a handle followed past the reading's steps");
        };
        match unparenthesised(expression) {
            syn::Expr::Try(inner) => self.origins(place, &inner.expr, steps, root),
            syn::Expr::Reference(inner) => self.origins(place, &inner.expr, steps, root),
            syn::Expr::Block(block) => match block.block.stmts.last() {
                Some(syn::Stmt::Expr(tail, None)) => self.origins(place, tail, steps, root),
                _ => unfollowed("a block with no value"),
            },
            syn::Expr::MethodCall(call) if call.method == "spawn" => {
                if Self::is_a_thread_builder(&call.receiver) {
                    vec![self.judge_body(place, offset, call.args.last(), root)]
                } else {
                    unfollowed("a `spawn` method not of a `std::thread::Builder`")
                }
            }
            // The value-keeping adapters of `Option` and `Result`; any other method's value is
            // one the reading cannot tie to its receiver.
            syn::Expr::MethodCall(call)
                if ["unwrap", "expect", "take", "map_err"]
                    .iter()
                    .any(|name| call.method == name) =>
            {
                self.origins(place, &call.receiver, steps, root)
            }
            syn::Expr::MethodCall(_) => unfollowed("a method's value"),
            syn::Expr::Call(call) => {
                let Some(path) = call_path(call) else {
                    return unfollowed("a call of an expression");
                };
                let named = self.paths.functions_named(
                    self.indexes[place.index],
                    place.base + call.span().byte_range().start,
                    &path,
                );
                if named.iter().any(is_door) {
                    return vec![self.judge_body(place, offset, call.args.last(), root)];
                }
                if !named.is_empty() {
                    return named
                        .iter()
                        .flat_map(|item| match self.function_of(item) {
                            Some((there, reading, function)) => {
                                match &reading.functions[function].tail {
                                    Some(tail) => self.origins(there, tail, steps, root),
                                    None => unfollowed("a function with no tail expression"),
                                }
                            }
                            None => unfollowed("a function the reading does not hold"),
                        })
                        .collect();
                }
                // `std` is not indexed: its thread start is known by its path.
                if path.ends_with(&["thread".to_owned(), "spawn".to_owned()]) {
                    return vec![self.judge_body(place, offset, call.args.first(), root)];
                }
                if (path == ["Some"] || path == ["Ok"]) && call.args.len() == 1 {
                    return self.origins(place, &call.args[0], steps, root);
                }
                unfollowed(
                    "a call that follows to no free function of the workspace and starts no thread",
                )
            }
            syn::Expr::Path(path) if path.path.segments.len() == 1 => {
                let name = path.path.segments[0].ident.to_string();
                let reading = self.file(place);
                let bound = reading.function_at(offset).and_then(|function| {
                    function
                        .bindings
                        .iter()
                        .filter(|binding| {
                            binding.name == name
                                && binding.at < offset
                                && binding.scope.contains(&offset)
                        })
                        .max_by_key(|binding| binding.at)
                });
                match bound.and_then(|binding| binding.origin.as_ref()) {
                    Some(origin) => self.origins(place, origin, steps, root),
                    None => unfollowed(&format!("`{name}`, bound to no one expression,")),
                }
            }
            syn::Expr::Field(field) => {
                let reading = self.file(place);
                let owner = reading
                    .function_at(offset)
                    .and_then(|function| function.self_type.clone());
                let (syn::Expr::Path(base), syn::Member::Named(name), Some(owner)) =
                    (&*field.base, &field.member, owner)
                else {
                    return unfollowed("a field not of `self`");
                };
                if !base.path.is_ident("self") {
                    return unfollowed("a field not of `self`");
                }
                let index = self.indexes[place.index];
                let mut origins = Vec::new();
                for file in index.files() {
                    let there = Place {
                        index: place.index,
                        base: file.span().start(),
                    };
                    for value in &self.file(there).fields {
                        if value.type_name == owner && *name == value.field {
                            origins.extend(self.origins(there, &value.value, steps, root));
                        }
                    }
                }
                if origins.is_empty() {
                    return unfollowed(&format!(
                        "`{owner}::{name}`, given no value in its package,"
                    ));
                }
                origins
            }
            _ => unfollowed("an expression the reading does not follow"),
        }
    }
}

/// What the join guard answers for one index.
#[derive(Default)]
struct JoinReading {
    /// Joins whose result is thrown away or carried on as a panic.
    not_values: usize,
    /// Joins whose `Err` is a value, of a contained thread.
    contained: usize,
    /// Occurrences of `join` that are no thread's join by their shape: one that takes arguments,
    /// a function's declaration.
    not_joins: usize,
    /// Joins of a foreign crate's type, by the full path written.
    foreign: Vec<String>,
    /// Every other join whose `Err` is a value, and every shape the reading refuses: its site,
    /// and why.
    refused: Vec<(String, String)>,
}

/// Every product join of the reader's index at `at`, judged; sites relative to `root`.
fn read_joins(reader: &JoinReader<'_>, at: usize, root: &Path) -> JoinReading {
    let index = reader.indexes[at];
    let found = index
        .search(&Search::new(
            needle!(Pattern::identifier("join")),
            View::Identifiers,
        ))
        .unwrap_or_else(|failure| panic!("{failure}"))
        .in_the_product(index);
    let mut reading = JoinReading::default();
    for occurrence in found.occurrences() {
        let start = occurrence.span.start();
        let file = index.file_at(start).expect("an occurrence is in a file");
        let place = Place {
            index: at,
            base: file.span().start(),
        };
        let joins = reader.file(place);
        let Some(join) = joins.joins.get(&(start - place.base)) else {
            match joins.shapes.get(&(start - place.base)) {
                Some(Shape::Arguments | Shape::Definition | Shape::Name) => reading.not_joins += 1,
                Some(Shape::PathCall(path)) if reader.is_foreign_path(place, start - place.base, path) => {
                    reading.foreign.push(path.join("::"));
                }
                Some(Shape::PathCall(path)) => reading.refused.push((
                    reader.site(at, start, root),
                    format!(
                        "`{}`: a `join` called through a path the reading cannot tie to `JoinHandle` or to a foreign crate's type written in full",
                        path.join("::")
                    ),
                )),
                Some(Shape::Refused(why)) => {
                    reading.refused.push((reader.site(at, start, root), why.clone()));
                }
                None => reading.refused.push((
                    reader.site(at, start, root),
                    "an occurrence of `join` in a shape the reading does not parse".to_owned(),
                )),
            }
            continue;
        };
        if join.used != JoinUse::Value {
            reading.not_values += 1;
            continue;
        }
        let origins = reader.origins(place, &join.receiver, ORIGIN_STEPS, root);
        let refusals: Vec<String> = origins
            .iter()
            .filter_map(|origin| match origin {
                Origin::Bare(why) | Origin::Unfollowed(why) => Some(why.clone()),
                Origin::Contained => None,
            })
            .collect();
        if refusals.is_empty() {
            reading.contained += 1;
        } else {
            reading
                .refused
                .push((reader.site(at, start, root), refusals.join("; ")));
        }
    }
    reading.refused.sort();
    reading
}

/// RED (T-JOINED-PANIC-VALUES) — **a product join's `Err` is a value only where the joined
/// thread's body is contained.**
///
/// Under the resident panic hook a panicking thread ends the process from inside the hook, before
/// it unwinds, so `JoinHandle::join` answers `Err` only for a thread whose body runs inside
/// `contained_panic::contain`; any other join whose `Err` is made into a value — a variant, an
/// error, a default — names a behaviour Folio does not have. Such a join may throw its result
/// away or carry the panic on (`unwrap`, `expect`); one that makes a value of it is refused unless
/// its handle follows to a thread start whose closure is a call edge to the boundary, and every
/// occurrence of `join` the reading cannot place in a supported shape is refused (the module
/// doc's list).
///
/// MUTATION: give `bt-pty`'s bounded join its value arm back (`match reader.join() { …,
/// Err(_) => ReaderExit::Panicked }` in `join_within`) and this names that join: its handle is a
/// parameter, which is no contained thread.
#[test]
fn no_product_join_makes_a_value_of_an_uncontained_panic() {
    let workspace =
        Workspace::read(&workspace_root()).unwrap_or_else(|rejection| panic!("{rejection}"));
    let indexes: Vec<&Index> = JOIN_PACKAGES
        .iter()
        .map(|package| Index::of_package(package))
        .collect();
    let paths = FunctionPaths::read(&indexes);
    let reader = JoinReader::new(indexes, &paths, &workspace);
    let mut refused = Vec::new();
    let mut read = 0;
    for (at, package) in JOIN_PACKAGES.iter().enumerate() {
        let reading = read_joins(&reader, at, workspace.root());
        println!(
            "joined-panic guard: {package}: {} join(s) thrown away or carried on, {} of a \
             contained thread, {} foreign {:?}, {} no join by shape, {} refused",
            reading.not_values,
            reading.contained,
            reading.foreign.len(),
            reading.foreign,
            reading.not_joins,
            reading.refused.len()
        );
        read += reading.not_values + reading.contained;
        refused.extend(
            reading
                .refused
                .into_iter()
                .map(|(site, why)| format!("{site} ({why})")),
        );
    }
    assert!(
        read > 0,
        "no product join was read — a guard that found nothing has proved nothing"
    );
    assert!(
        refused.is_empty(),
        "a join makes a value of a panic the resident hook never lets it see ({}): run the \
         thread's body inside `contained_panic::contain`, or carry the panic on (`expect`) and \
         say that the thread's panic ends the process:\n  {}",
        refused.len(),
        refused.join("\n  ")
    );
}

/// The `joined_panic` fixture's directory.
fn join_fixture_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("joined_panic")
}

/// The `joined_panic` fixture, indexed as a crate called `joined`.
fn join_fixture() -> Index {
    let directory = join_fixture_directory();
    let universe = Universe::declare(
        "the joined-panic fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "joined".to_owned(),
                kind: TargetKind::Library,
                name: "joined".to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

/// The fixture's refused joins and shapes, by the site of their `join`.
const FIXTURE_REFUSED: [&str; 11] = [
    "lib.rs:118:15",
    "lib.rs:125:12",
    "lib.rs:132:47",
    "lib.rs:138:27",
    "lib.rs:143:41",
    "lib.rs:154:15",
    "lib.rs:24:18",
    "lib.rs:46:12",
    "lib.rs:75:14",
    "lib.rs:80:12",
    "lib.rs:91:12",
];

/// RED (T-JOINED-PANIC-VALUES, rounds 1 and 2) — **a join's `Err` is a value by a call edge from
/// its thread's closure to the real boundary, the handle followed to its thread by item, never by
/// a name; and what the reading cannot follow is refused, never passed.** The fixture, read beside
/// `bt-platform`, allows two joins — a thread started through the thread door whose body is
/// handed to the boundary by an alias, and a contained handle given to a `self` field and joined
/// by a method — and refuses eleven: a bare thread joined with an `Err → Panicked` arm, a thread
/// whose body is the lookalike `contained_panic::contain`, a free function's bare handle made a
/// default, a handle handed in as a parameter, a body that calls a helper that calls the boundary,
/// a handle an associated function returns, a contained body started through a renamed `use` of
/// `spawn`, a handle taken out of a collection, a `join()` in a macro's tokens and
/// `JoinHandle::join` handed on as a value, and a foreign type's `join` called through an imported
/// name. A foreign crate's `join` named by its full type path is no thread's; two joins of bare
/// threads are not judged (one thrown away, one carried on); five occurrences are no join by their
/// shape (a declaration, a binding and its value, two `join`s with arguments); a test's join is
/// not read.
///
/// MUTATIONS: ① judge the thread's closure by its spelling (a call path ending
/// `contained_panic::contain`) — the lookalike's thread is allowed; ② follow no `self` field —
/// the drain's join is refused; ③ allow a call that follows to nothing (round 1's "not a
/// thread") — the associated function's and the renamed spawn's joins pass; ④ skip an occurrence
/// the reading did not parse (round 1's `continue`) — the macro's join and the join handed on
/// pass.
#[test]
fn a_join_is_a_value_by_a_call_edge_from_its_thread_to_the_boundary() {
    let index = join_fixture();
    let platform = Index::of_package(BOUNDARY.0);
    let paths = FunctionPaths::read(&[&index, platform]);
    let workspace =
        Workspace::read(&workspace_root()).unwrap_or_else(|rejection| panic!("{rejection}"));
    let reader = JoinReader::new(vec![&index, platform], &paths, &workspace);
    let reading = read_joins(&reader, 0, &join_fixture_directory());
    let sites: Vec<&str> = reading
        .refused
        .iter()
        .map(|(site, _)| site.as_str())
        .collect();
    assert_eq!(
        sites, FIXTURE_REFUSED,
        "the associated function's handle, the renamed spawn, the collection, the macro's join, the join handed on as a value, the foreign type's join through an imported name, the bare thread's panic arm, the lookalike's thread, the free function's bare handle, \
         the parameter and the helper's thread: {:#?}",
        reading.refused
    );
    assert_eq!(
        reading.contained, 2,
        "the door's thread through the alias, and the drain's field"
    );
    assert_eq!(
        reading.not_values, 2,
        "one join thrown away, one carried on"
    );
    assert_eq!(
        reading.foreign,
        ["windows_future::IAsyncOperation::join"],
        "the foreign operation's join, named in full"
    );
    assert_eq!(
        reading.not_joins, 5,
        "`Operation::join`'s declaration, a binding named `join` and its value, `Path::join` and the macro's slice `join`"
    );
}
