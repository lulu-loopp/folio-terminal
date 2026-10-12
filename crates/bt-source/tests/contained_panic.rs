//! **No bare `catch_unwind` in the product** (T-PANIC-ROWS-HONEST): every catch of a panic in
//! `bt-app`, `bt-platform` and `bt-pty` is the one boundary, `contained_panic::contain`, or sits
//! inside a closure handed to it.
//!
//! The resident application's panic hook ends the process from inside the hook, before any
//! unwinding, unless its one decision — `the_hook_lets_this_panic_unwind` — says the panic is
//! contained. A bare `std::panic::catch_unwind` is therefore reached only where no such hook is
//! installed: in a test, never in Folio, so the failure value it builds is decorative there.
//! `contained_panic::contain` marks the thread for the length of its work, which is what the
//! hook asks; it is the only catch the hook lets a panic reach.
//!
//! # What it reads, exactly
//!
//! Each package's own sources ([`Index::of_package`]), and in them every **product** occurrence
//! of the identifier `catch_unwind` ([`bt_source::Occurrence::in_the_product`]: test files,
//! `#[cfg(test)]` items and inline test modules are not read) — a call, a path handed on as a
//! value, and a `use` of it under any name, so an import renamed out of sight is refused where
//! it is written. An occurrence is allowed when
//!
//! * its innermost item is the function `contain` of a module named `contained_panic` — the
//!   boundary itself; or
//! * it lies inside the arguments of a call whose path ends `contained_panic::contain` — a catch
//!   inside the closure handed to the boundary.
//!
//! Anything else is refused by `path:line:column`. There is no allowlist: a thread whose death
//! is meant to be the process's has no catch at all, and its `# completions` row says that a
//! panic ends the process under the resident hook (`crates/bt-app/src/window_waits.tsv`).
//!
//! # What it does not see
//!
//! A call to `contain` written inside a macro's token tree is not parsed, so a catch handed to
//! the boundary there is refused, never allowed; and `bt-math`'s render boundary, which the hook
//! honours through `render_panic_is_contained`, is outside the three packages.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use bt_source::{
    DiskScope, Index, ItemRecord, Pattern, Search, Span, TargetId, TargetKind, TargetRoot,
    Universe, Vendor, View, Workspace, needle, report,
};

/// The packages whose product code the guard reads.
const PACKAGES: [&str; 3] = ["bt-app", "bt-platform", "bt-pty"];

/// The boundary's module.
const MODULE: &str = "contained_panic";

/// The boundary's function.
const BOUNDARY: &str = "contain";

/// The byte ranges, within one file, of the argument list of every call whose path ends
/// `contained_panic::contain`.
#[derive(Default)]
struct HandedToTheBoundary {
    arguments: Vec<Range<usize>>,
}

impl<'ast> syn::visit::Visit<'ast> for HandedToTheBoundary {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*call.func {
            let segments: Vec<String> = path
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            if segments.ends_with(&[MODULE.to_owned(), BOUNDARY.to_owned()]) {
                let open = call.paren_token.span.open().byte_range();
                let close = call.paren_token.span.close().byte_range();
                self.arguments.push(open.start..close.end);
            }
        }
        syn::visit::visit_expr_call(self, call);
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

/// Whether `item` is the boundary itself: a free function `contain` in a module named
/// `contained_panic`.
fn is_the_boundary(item: &ItemRecord) -> bool {
    item.name() == BOUNDARY
        && item.type_owner().is_none()
        && item
            .module_paths()
            .iter()
            .any(|module| module.rsplit("::").next() == Some(MODULE))
}

/// Every product `catch_unwind` of `index`, judged; a refused site's path is written relative to
/// `root`.
fn read(index: &Index, root: &Path) -> Reading {
    let found = index
        .search(&Search::new(
            needle!(Pattern::identifier("catch_unwind")),
            View::Identifiers,
        ))
        .unwrap_or_else(|failure| panic!("{failure}"))
        .in_the_product(index);
    let mut handed: BTreeMap<usize, Vec<Range<usize>>> = BTreeMap::new();
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
        let arguments = handed.entry(base).or_insert_with(|| {
            let parsed = syn::parse_file(index.text(file.span()))
                .unwrap_or_else(|error| panic!("{}: {error}", file.path().display()));
            let mut calls = HandedToTheBoundary::default();
            syn::visit::Visit::visit_file(&mut calls, &parsed);
            calls.arguments
        });
        let handed_to_the_boundary = arguments
            .iter()
            .any(|range| base + range.start <= span.start() && span.end() <= base + range.end);
        if handed_to_the_boundary || innermost_item(index, span).is_some_and(is_the_boundary) {
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
/// boundary or inside it.**
///
/// MUTATION: put `pty_door::request`'s birth back on a bare `std::panic::catch_unwind` (or plant
/// any bare catch in the three packages' product code) and this names its line.
#[test]
fn no_product_catch_unwind_is_outside_the_contained_panic_boundary() {
    let workspace =
        Workspace::read(&workspace_root()).unwrap_or_else(|rejection| panic!("{rejection}"));
    let root = workspace.root();
    let mut refused = Vec::new();
    let mut allowed = 0;
    for package in PACKAGES {
        let index = Index::of_package(package);
        assert!(!index.files().is_empty(), "{package}: no file read");
        let reading = read(index, root);
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

/// The `contained_panic` fixture, indexed.
fn fixture() -> Index {
    let directory = fixture_directory();
    let universe = Universe::declare(
        "the contained-panic fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "contained_panic".to_owned(),
                kind: TargetKind::Library,
                name: "contained_panic".to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

/// RED (T-PANIC-ROWS-HONEST) — **a planted bare `catch_unwind` is refused, by its site; the
/// boundary's own catch, a catch inside a closure handed to it and a test's catch are not.**
///
/// The fixture plants three bare catches: a call in an ordinary function, an import renamed out
/// of sight, and a call in a function named `contain` in a module that is not the boundary.
///
/// MUTATIONS: ① allow every occurrence whose innermost item is named `contain` (drop the module
/// test in `is_the_boundary`) — the third plant is not refused; ② drop the closure rule —
/// `nested`'s catch is refused.
#[test]
fn a_planted_bare_catch_unwind_is_refused_by_its_site() {
    let index = fixture();
    let reading = read(&index, &fixture_directory());
    assert_eq!(
        reading.refused,
        ["lib.rs:18:17", "lib.rs:22:17", "lib.rs:31:21"],
        "the three plants, and nothing else"
    );
    assert_eq!(
        reading.allowed, 2,
        "the boundary's own catch and the one handed to it"
    );
}
