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
