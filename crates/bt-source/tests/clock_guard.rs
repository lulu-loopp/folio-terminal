//! **The clock guard (gate G3, `clock-guard`)** — no product code that a
//! `wasm32-unknown-unknown` build compiles names the standard library's
//! `Instant` or `SystemTime`.
//!
//! `std::time::Instant::now()` panics on `wasm32-unknown-unknown`, and the
//! target is `panic=abort`, so a clock read on the frame path ends the page.
//! The crates below read time through `web_time::Instant`, which on every
//! native target *is* `std::time::Instant` (a re-export) and in a browser reads
//! `performance.now()`. `Duration` has no clock in it and is allowed.
//!
//! # The source set, exactly
//!
//! The product items of these, and nothing else (see [`SOURCE_SET`]):
//!
//! * `crates/{bt-unicode,bt-transcript,bt-doc,bt-layout,bt-viewport,bt-detect,
//!   bt-effects,bt-math,bt-render,bt-term}/src` — every target the package roots
//!   there;
//! * `vendor/vte/src`;
//! * `vendor/alacritty_terminal/src` minus the modules `crate::event_loop` and
//!   `crate::tty`, which are not compiled for `wasm32`.
//!
//! **Out of scope, by name:** `bt-platform` (whose `http`, `install_txn` and
//! `instance` modules read `std::time` today; its admission vocabulary, the one
//! part a browser build reads, is `bt-effects`' since CC-3), `bt-app`,
//! `bt-pty`, `bt-persist`, `bt-corpus`, `bt-winres`, `bt-workbench`,
//! `bt-source`, `bt-lint-probe`, `vendor/mitex` and `vendor/mitex-parser`. None
//! of them is in the wasm32 graph the browser build is heading for; each joins
//! this list only in the ticket that brings it into that graph, and nothing
//! outside the list joins silently (the `gates-can-fail` canary plants a clock in
//! `bt-platform`'s `http` module and requires this to stay green).
//!
//! "Product" is bt-source's production view, [`Occurrence::in_the_product`]:
//! test files, `#[cfg(test)]` items and inline `#[cfg(test)]` modules are not
//! read.
//!
//! # Every spelling it refuses
//!
//! * a qualified path, `std::time::Instant::now()` or `::std::time::SystemTime`,
//!   anywhere — inside a macro's arguments too;
//! * a flat import, `use std::time::Instant;`, `use std::time::{Duration,
//!   Instant};`, with or without `as`;
//! * a nested import, `use std::{fmt, time::{Duration, Instant}};`;
//! * a glob that brings them, `use std::time::*;`;
//! * each import above written from the crate root, `use ::std::{time::Instant};`
//!   — a leading `::` names the crate, so its first segment is matched against
//!   `std` and its `extern crate` renames, never against a `use` alias;
//! * an alias of the module or of `std` itself — `use std::time as t;` then
//!   `t::Instant`, `use std as s;` then `s::time::Instant` — and a `pub`
//!   re-export of either, which this file-by-file reading could not follow into
//!   the module that uses it.
//!
//! An import is read from the parse; a `use` written inside a macro's token
//! tree is not, which no file of the set has.
//!
//! The run prints, per package, how many files it read and how many `use`
//! declarations it followed, and it refuses to pass having read no file of a
//! package on the list: a guard that read nothing has proved nothing.
//! `scripts/ci/check-clock-guard.ps1` runs this test and is the gate's name in
//! CI.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use bt_source::{
    Index, Occurrence, Pattern, Search, View, Workspace, is_vendored, needle, report, universes,
};

/// Which modules of a package the guard reads.
#[derive(Clone, Copy)]
enum Modules {
    /// Every module.
    All,
    /// Every module but these trees.
    AllBut(&'static [&'static str]),
}

impl Modules {
    fn admits(self, module_path: &str) -> bool {
        let under = |tree: &str| {
            module_path == tree
                || module_path
                    .strip_prefix(tree)
                    .is_some_and(|rest| rest.starts_with("::"))
        };
        match self {
            Self::All => true,
            Self::AllBut(trees) => !trees.iter().any(|tree| under(tree)),
        }
    }
}

/// The source set of the module documentation, package by package.
const SOURCE_SET: [(&str, Modules); 12] = [
    ("bt-unicode", Modules::All),
    ("bt-transcript", Modules::All),
    ("bt-doc", Modules::All),
    ("bt-layout", Modules::All),
    ("bt-viewport", Modules::All),
    ("bt-detect", Modules::All),
    ("bt-effects", Modules::All),
    ("bt-math", Modules::All),
    ("bt-render", Modules::All),
    ("bt-term", Modules::All),
    ("vte", Modules::All),
    (
        "alacritty_terminal",
        Modules::AllBut(&["crate::event_loop", "crate::tty"]),
    ),
];

/// The standard library's clocks.
const CLOCKS: [&str; 2] = ["Instant", "SystemTime"];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// One `use` tree, flattened: the path it names and what it binds.
struct Imported {
    /// The segments as written, without a leading `::`.
    path: Vec<syn::Ident>,
    /// Written with a leading `::` (`use ::std::time::Instant;`), so the first
    /// segment names a crate and never a `use` alias.
    global: bool,
    /// An `extern crate`, whose name is a crate name a leading `::` reaches.
    extern_crate: bool,
    leaf: Leaf,
    public: bool,
}

enum Leaf {
    /// The last segment of `path`, bound under this name.
    Name(String),
    /// Every public name of `path`.
    Glob,
}

fn flatten(
    tree: &syn::UseTree,
    prefix: &mut Vec<syn::Ident>,
    global: bool,
    public: bool,
    out: &mut Vec<Imported>,
) {
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(path.ident.clone());
            flatten(&path.tree, prefix, global, public, out);
            prefix.pop();
        }
        syn::UseTree::Name(name) => out.push(imported(prefix, &name.ident, None, global, public)),
        syn::UseTree::Rename(rename) => {
            out.push(imported(
                prefix,
                &rename.ident,
                Some(&rename.rename),
                global,
                public,
            ));
        }
        syn::UseTree::Glob(_) => out.push(Imported {
            path: prefix.clone(),
            global,
            extern_crate: false,
            leaf: Leaf::Glob,
            public,
        }),
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                flatten(tree, prefix, global, public, out);
            }
        }
    }
}

/// `a::b::{self}` names `a::b` itself.
fn imported(
    prefix: &[syn::Ident],
    ident: &syn::Ident,
    rename: Option<&syn::Ident>,
    global: bool,
    public: bool,
) -> Imported {
    let mut path = prefix.to_vec();
    if ident != "self" {
        path.push(ident.clone());
    }
    let bound = rename.unwrap_or_else(|| path.last().unwrap_or(ident));
    Imported {
        leaf: Leaf::Name(bound.to_string()),
        path,
        global,
        extern_crate: false,
        public,
    }
}

/// Every `use` and `extern crate` of one file, flattened — in function bodies
/// and inline modules too.
#[derive(Default)]
struct Imports {
    uses: Vec<Imported>,
    declarations: usize,
}

impl<'ast> syn::visit::Visit<'ast> for Imports {
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.declarations += 1;
        let public = !matches!(item.vis, syn::Visibility::Inherited);
        flatten(
            &item.tree,
            &mut Vec::new(),
            item.leading_colon.is_some(),
            public,
            &mut self.uses,
        );
    }

    fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
        self.declarations += 1;
        if let Some((_, rename)) = &item.rename {
            self.uses.push(Imported {
                path: vec![item.ident.clone()],
                global: false,
                extern_crate: true,
                leaf: Leaf::Name(rename.to_string()),
                public: !matches!(item.vis, syn::Visibility::Inherited),
            });
        }
    }
}

/// One refused spelling, before the production view is asked about it.
struct Site {
    /// File-relative byte range of the identifier that names the clock (or the
    /// module it is reached through).
    at: std::ops::Range<usize>,
    what: String,
}

/// What one file's imports bind and which of them name a clock.
struct Reading {
    /// Names that are `std` in this file.
    std_names: BTreeSet<String>,
    /// Names that are `std::time` in this file.
    time_names: BTreeSet<String>,
    sites: Vec<Site>,
}

/// The canonical `std`-rooted path an import names, if it is one.
///
/// A path written with a leading `::` starts at a crate name, so its first
/// segment is looked up among the crates that are `std` and never among the
/// file's `use` aliases; any other path may start at either.
fn rooted(
    import: &Imported,
    std_crates: &BTreeSet<String>,
    std_names: &BTreeSet<String>,
    time_names: &BTreeSet<String>,
) -> Option<Vec<String>> {
    let (first, rest) = import.path.split_first()?;
    let first = first.to_string();
    let mut canonical = if import.global {
        if !std_crates.contains(&first) {
            return None;
        }
        vec!["std".to_owned()]
    } else if std_names.contains(&first) {
        vec!["std".to_owned()]
    } else if time_names.contains(&first) {
        vec!["std".to_owned(), "time".to_owned()]
    } else {
        return None;
    };
    canonical.extend(rest.iter().map(ToString::to_string));
    Some(canonical)
}

fn read_imports(imports: &Imports) -> Reading {
    let mut std_crates = BTreeSet::from(["std".to_owned()]);
    for import in &imports.uses {
        if let (true, [krate], Leaf::Name(bound)) =
            (import.extern_crate, import.path.as_slice(), &import.leaf)
            && krate == "std"
        {
            std_crates.insert(bound.clone());
        }
    }
    let mut std_names = std_crates.clone();
    let mut time_names = BTreeSet::new();
    // Aliases can be written in any order and can chain (`use std as s; use
    // s::time as t;`), so the bindings are read to a fixed point first.
    loop {
        let before = (std_names.len(), time_names.len());
        for import in &imports.uses {
            let Some(canonical) = rooted(import, &std_crates, &std_names, &time_names) else {
                continue;
            };
            match (&import.leaf, canonical.as_slice()) {
                (Leaf::Name(bound), [std]) if std == "std" => {
                    std_names.insert(bound.clone());
                }
                (Leaf::Name(bound), [std, time]) if std == "std" && time == "time" => {
                    time_names.insert(bound.clone());
                }
                (Leaf::Glob, [std]) if std == "std" => {
                    time_names.insert("time".to_owned());
                }
                _ => {}
            }
        }
        if (std_names.len(), time_names.len()) == before {
            break;
        }
    }

    let mut sites = Vec::new();
    for import in &imports.uses {
        let Some(canonical) = rooted(import, &std_crates, &std_names, &time_names) else {
            continue;
        };
        let spelled: Vec<String> = import.path.iter().map(ToString::to_string).collect();
        let spelled = if import.global {
            format!("::{}", spelled.join("::"))
        } else {
            spelled.join("::")
        };
        let last = import.path.last().expect("a rooted path has a segment");
        let site = |what: String| Site {
            at: last.span().byte_range(),
            what,
        };
        match (&import.leaf, canonical.as_slice()) {
            (Leaf::Name(_), [std, time, clock])
                if std == "std" && time == "time" && CLOCKS.contains(&clock.as_str()) =>
            {
                sites.push(site(format!("`use {spelled}` imports std::time::{clock}")));
            }
            (Leaf::Glob, [std, time]) if std == "std" && time == "time" => {
                sites.push(site(format!(
                    "`use {spelled}::*` imports std::time::{{Instant, SystemTime}}"
                )));
            }
            (Leaf::Name(bound), [std, time]) if import.public && std == "std" && time == "time" => {
                sites.push(site(format!(
                    "a public re-export of std::time as `{bound}` reaches its clocks from other modules"
                )));
            }
            (Leaf::Name(bound), [std]) if import.public && std == "std" => {
                sites.push(site(format!(
                    "a public re-export of std as `{bound}` reaches std::time's clocks from other modules"
                )));
            }
            _ => {}
        }
    }
    Reading {
        std_names,
        time_names,
        sites,
    }
}

/// One package's answer.
struct Scanned {
    files: usize,
    declarations: usize,
    /// `path:line:column` to what is named there, sorted.
    refused: BTreeMap<String, String>,
}

/// Every product occurrence of `pattern` in `index`, by its end offset.
fn product_occurrences(index: &Index, pattern: Pattern) -> BTreeMap<usize, Occurrence> {
    index
        .search(&Search::new(needle!(pattern), View::Identifiers))
        .unwrap_or_else(|failure| panic!("{failure}"))
        .in_the_product(index)
        .occurrences()
        .iter()
        .map(|occurrence| (occurrence.span.end(), *occurrence))
        .collect()
}

fn scan(package: &str, modules: Modules) -> Scanned {
    let root = workspace_root();
    let workspace = Workspace::read(&root).unwrap_or_else(|rejection| panic!("{rejection}"));
    let member = workspace
        .package(package)
        .unwrap_or_else(|rejection| panic!("{rejection}"));
    let vendor = if is_vendored(member.directory()) {
        bt_source::Vendor::Included
    } else {
        bt_source::Vendor::Excluded
    };
    let universe = universes::crate_sources(member, vendor)
        .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
    let index =
        Index::shared(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)));

    // The names a refused import ends in, in the product, by where they end.
    let mut named: BTreeMap<usize, Occurrence> = BTreeMap::new();
    for name in CLOCKS.iter().chain(["time", "std"].iter()) {
        named.extend(product_occurrences(&index, Pattern::identifier(name)));
    }

    let mut files = 0;
    let mut declarations = 0;
    let mut refused = BTreeMap::new();
    let mut refuse = |end: usize, what: &str| {
        let location = index.locate(end - 1).expect("an occurrence is in a file");
        let relative = location
            .file
            .strip_prefix(workspace.root())
            .unwrap_or(location.file)
            .to_string_lossy()
            .replace('\\', "/");
        refused
            .entry(format!("{relative}:{}:{}", location.line, location.column))
            .or_insert_with(|| what.to_owned());
    };
    for file in index.files() {
        if !file
            .owners()
            .iter()
            .any(|owner| modules.admits(&owner.module_path))
        {
            continue;
        }
        files += 1;
        let base = file.span().start();
        let parsed = syn::parse_file(index.text(file.span()))
            .unwrap_or_else(|error| panic!("{}: {error}", file.path().display()));
        let mut imports = Imports::default();
        syn::visit::Visit::visit_file(&mut imports, &parsed);
        declarations += imports.declarations;
        let reading = read_imports(&imports);

        for site in &reading.sites {
            let end = base + site.at.end;
            if named.contains_key(&end) {
                refuse(end, &site.what);
            }
        }

        // Qualified paths, through every name `std` and `std::time` have in this
        // file — read lexically, so a path inside a macro's arguments counts.
        let mut spellings = Vec::new();
        for std_name in &reading.std_names {
            spellings.extend(
                CLOCKS
                    .iter()
                    .map(|clock| format!("{std_name}::time::{clock}")),
            );
        }
        for time_name in &reading.time_names {
            spellings.extend(CLOCKS.iter().map(|clock| format!("{time_name}::{clock}")));
        }
        for spelling in spellings {
            for (end, _) in product_occurrences(&index, Pattern::path(&spelling)) {
                if file.span().holds(end - 1) {
                    refuse(
                        end,
                        &format!("`{spelling}` names the standard library's clock"),
                    );
                }
            }
        }
    }
    Scanned {
        files,
        declarations,
        refused,
    }
}

/// RED (CC-2) — **nothing in the clock guard's source set names
/// `std::time::Instant` or `std::time::SystemTime`**, in any of the spellings
/// the module documentation lists.
///
/// MUTATION: add `fn probe() { let _ = std::time::Instant::now(); }` to
/// `crates/bt-doc/src/lib.rs` and this names that line; add
/// `use std::{time::{SystemTime}};` there and it names that one; add either to
/// `crates/bt-platform/src/http.rs` and it stays green (out of scope by name).
#[test]
fn no_clock_of_the_standard_library_is_named_where_a_browser_build_reads() {
    let mut refused = Vec::new();
    let mut empty = Vec::new();
    let mut total = 0;
    for (package, modules) in SOURCE_SET {
        let scanned = scan(package, modules);
        println!(
            "clock-guard: {package}: {} file(s) read, {} use declaration(s) followed, {} refused",
            scanned.files,
            scanned.declarations,
            scanned.refused.len()
        );
        if scanned.files == 0 {
            empty.push(package);
        }
        total += scanned.files;
        refused.extend(
            scanned
                .refused
                .into_iter()
                .map(|(at, what)| format!("{at}: {what}")),
        );
    }
    println!(
        "clock-guard: {total} file(s) read over {} package(s)",
        SOURCE_SET.len()
    );
    assert!(
        empty.is_empty(),
        "the clock guard read no file of {} — a package on the list it did not read is not a \
         package it checked",
        empty.join(", ")
    );
    assert!(
        refused.is_empty(),
        "the standard library's clocks are named where a wasm32 build reads ({}); use \
         `web_time::Instant` / `web_time::SystemTime`, which are std's on every native target:\n  {}",
        refused.len(),
        refused.join("\n  ")
    );
}
