//! **A process-global that two tests of one binary reach, or that a test writes, is on a
//! register with the reason it is safe** — T-TEST-PROCESS-GLOBALS, the guard for shape (b) of
//! T-TEST-ORDER-STRAYS' census.
//!
//! Guard (source-reading by design): its subject is how the workspace's test code shares state
//! across the tests of one process, so it reads that code through `bt-source`'s index and call
//! graph and binds to no file.
//!
//! # What a process-global is
//!
//! A `static` written in a module or inside a function body, whose written type is, through
//! `Arc`/`Box`/`Rc` and through an array or a slice (`[AtomicU64; 5]` is its cell, element by
//! element):
//!
//! * an `Atomic*`;
//! * a `Mutex<T>` or `RwLock<T>` with a `T` other than `()`;
//! * a `OnceLock` or `OnceCell`;
//! * a `LazyLock`, `LazyCell` or `Lazy`, whose own cell is the type it holds;
//!
//! or any `static mut`. **Not** one, each for a stated reason:
//!
//! * `thread_local!` statics: libtest runs every test on a thread of its own, so the value is
//!   that test's. The index holds no record of one either, since a macro writes it.
//! * Unit locks (`Mutex<()>`, `RwLock<()>`) and the value-free primitives (`Condvar`, `Once`,
//!   `Barrier`): the lock is the remedy, not the shape. A `lock:<NAME>` row names one.
//! * A static declared inside a `#[test]` body: one test.
//! * A static of any other type, a first-party one with interior mutability included. That is
//!   a limit, stated below.
//!
//! # A test's reach
//!
//! A **test** is a `#[test]` function, or the crate-root `main` of a target whose manifest says
//! `harness = false`. Its **reach** is its own body plus every test-only callable of the test's
//! own target that it reaches through edges `bt_source::call_graph` resolves, transitively.
//! Test-only means no product build compiles it. This is `timing.rs`'s helper walk; a product
//! function ends the walk.
//!
//! **What runs without a call edge is taken in too**, to a fixed point:
//!
//! * a test-only first-party impl of a foreign trait of the target ([`Resolver::implicit`]: a
//!   `Drop` at a scope's end, winit's `ApplicationHandler::resumed`, a `Display` a `format!`
//!   runs), when a body of the reach names its self type: a path, a type, a struct literal or
//!   an unparsed macro argument that, placed from the body's module (its own module, its named
//!   and glob `use`s, `crate`/`self`/`super`), may be the type as placed from the impl's module,
//!   or a method of that type in the reach. Two types of one name in two modules are two types;
//! * a test-only `GlobalAlloc` impl of the target, in every test of the target: every
//!   allocation runs it;
//! * a function written in the tokens of an item-position macro of test code
//!   (`objc2::define_class!`), which the index holds no record of: the tokens are read as
//!   items, and a function in an `impl` block is taken in when a body of the reach names that
//!   block's type, a free one when a body names the function. Its paths are placed without a
//!   call graph, by its module, its named and glob `use`s, and `crate`/`self`/`super`.
//!
//! A test **reaches** a global when a body in its reach names it: a use of a module's `static`
//! is a [`Target::Value`] edge, and a body-declared static is named by its own name inside the
//! body that declares it. Each use is read by its syntax:
//!
//! | the global | read | written |
//! |---|---|---|
//! | `Atomic*` | `load` | `store`, `swap`, `fetch_*`, `compare_exchange*`, `compare_and_swap` |
//! | `Mutex`, `RwLock` | `read`, `try_read`, `is_poisoned` | `lock`, `try_lock`, `write`, `try_write`, `get_mut`, `clear_poison` |
//! | `OnceLock`, `OnceCell` | `get`, `wait` | `get_or_init`, `get_or_try_init`, `set` (once); `take`, `get_mut` |
//! | `LazyLock` of a plain value | every use (a deref; the initializer is its only write) | — |
//! | `LazyLock` of a cell | a method, read as that cell's | a method, as that cell's |
//! | `static mut` | a value, a field, an index, a deref, `&` | `=`, `op=`, `&mut`, `addr_of_mut!` |
//!
//! A field, an index or a parenthesis of the global under `=`, `op=`, `&mut`, `addr_of_mut!` or
//! a method call is the global in that place: `X.f = 1`, `X[i] += 1` and `&mut X.f` write a
//! `static mut`, `X.f.push(1)` is its method, and `X[i].fetch_add(1, …)` is the cell's.
//!
//! **Any other use is undecided, and is read as a write** (fail closed): the global handed by
//! reference to a call, used bare as a value, or a method not in the table (a `static mut`'s
//! included). So it is a G2 hit, its message says `undecided use`, and no class this guard
//! checks passes over it.
//!
//! # The grammar
//!
//! * **G1 — shared.** A global reached by two or more tests of one binary must be a row:
//!   `<file>:<line>\t<binary>\t<GLOBAL>\tG1\treached by <n> tests of <binary> (…) and on no row
//!   of docs/plans/TEST-GLOBALS.tsv`. Two `cfg` arms of one test are one test.
//! * **G2 — written.** A global that a body in one test's reach writes must be a row, even when
//!   no other test names it, because product code the other tests call may read it:
//!   `<file>:<line>\t<binary>\t<GLOBAL>\tG2\twritten (<op>) by <test> and on no row …`.
//! * **G3 — through the call graph.** The reach is G3. On the walk, an edge the call graph
//!   leaves unresolved is refused by file and line when a candidate is a test-only callable of
//!   the target that the caller may call and that reaches a global, or of which that cannot be
//!   told. Unresolved edges are `Dynamic`, `Ambiguous` and `OtherArm`; a `Path` whose last
//!   segment is a global's name is refused outright, and any other is read as every test-only
//!   callable of its last segment's name. A call in an item-macro function is read the same
//!   way. Also refused: a body on the walk that does not parse, a cut-off chain, a
//!   `macro_rules!` of the package whose definition names a global, macro arguments that do not
//!   parse and name a global, a use the call graph sees that this reading does not, an
//!   implicitly run impl or item-macro function that reaches a global and whose type no test's
//!   reach names, and an item-position macro of test code whose tokens do not parse as items
//!   (or hold an item other than a function) and name a global or a test-only callable that
//!   reaches one.
//!
//! # The register
//!
//! `docs/plans/TEST-GLOBALS.tsv` has the columns `file`, `global`, `binary`, `class` and
//! `reason`, and is keyed by the first three. The reason is `YYYY-MM-DD: why`. The rows equal
//! the hits both ways: an unlisted hit is red, and so is a row that is no longer a hit. These
//! classes **are checked here**:
//!
//! * `lock:<NAME>`: every test that reaches the global, each `cfg` arm on its own, also
//!   reaches the unit lock `<NAME>` names. `<NAME>` is the lock's name, or its module path's
//!   tail (`windows_impl::ONE_WATCHER_AT_A_TIME`), and must name exactly one unit lock of the
//!   binary;
//! * `single writer`: exactly one test's reach writes it;
//! * `one main`: the binary runs without the harness;
//! * `run=alone`, `run=serial`: every test that reaches it carries `#[ignore]` and is on
//!   `scripts/ci/ignored-tests.txt` with that run condition (the list names a test by its bare
//!   name, so the attribute is what ties the line to this package's test).
//!
//! These classes are **by hand**: `per-test key`, `monotone`, `write-once`, `not on a test
//! thread` and `serial helpers`. Of `write-once`, the guard holds the writes: every write is a
//! once-write (`get_or_init`, `get_or_try_init`, `set`), or the global is a `LazyLock` of a
//! plain value. That the once-written value is the same for every test is a person's reading.
//! `scripts/ci/check-test-globals.ps1` admits an added row of a checked class with its dated
//! reason. It admits an added row of a by-hand class only under a new `# widened` line, as the
//! timing list does.
//!
//! # What it does not see
//!
//! * **A global reached only through product functions.** The walk ends at a product function.
//!   A test reaching product code reaches most of the workspace's globals, and about twelve
//!   thousand unresolved edges in bt-app's product graph alone, so following product code
//!   decides nothing. The `i18n` language switch the CJK timing probe makes is one; it was found
//!   by reading. [`the_product_reach_of_the_tests_is_measured`] prints how many such globals
//!   there are, as a measure, not a gate; CI prints it on every run.
//! * An unresolved edge whose candidates are all product code.
//! * A global of a first-party type with interior mutability, a `Sync` wrapper around an
//!   `UnsafeCell`, and a cell reached through a `static` holding a reference.
//! * A body-declared static named by another body that a `use` brought it into.
//! * An implicitly run impl whose type one test's reach names while another test builds a
//!   value of it without naming it (by inference alone, or through a path the module-local
//!   placing above does not follow, as `super::super::T` through a re-export): the second test
//!   is not given it. An impl no test's reach names at all is refused.
//! * A `#[cfg]` written on an item-position macro invocation itself: the macro is read as test
//!   code only when its file or its module is.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use bt_source::call_graph::{Arms, CallGraph, ItemId, Node, Resolver, Target, Unresolved};
use bt_source::{
    DiskScope, Index, ItemKind, ItemRecord, MacroKind, Package, Pattern, Search, Span, TargetId,
    TargetKind, TargetRoot, Universe, ValueKind, Vendor, View, Workspace, is_vendored, needle,
    normalized, report, universes,
};

/// The register, from the workspace root.
const LIST: &str = "docs/plans/TEST-GLOBALS.tsv";

/// The register's column header.
const HEADER: &str = "file\tglobal\tbinary\tclass\treason";

/// The run conditions of the ignored tests, from the workspace root.
const IGNORED: &str = "scripts/ci/ignored-tests.txt";

/// The classes this guard checks itself, beside `lock:<NAME>`.
const CHECKED: [&str; 4] = ["single writer", "one main", "run=alone", "run=serial"];

/// The classes reviewed by hand. The guard holds `write-once`'s writes to once-writes, but
/// that a once-written value is the same for every test is a person's reading.
const BY_HAND: [&str; 5] = [
    "per-test key",
    "monotone",
    "write-once",
    "not on a test thread",
    "serial helpers",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The whole of a first-party package; `bt-source`'s `tests/fixtures/` are declared out.
fn universe_for(package: &Package) -> Universe {
    if package.name() == "bt-source" {
        let scopes = vec![
            DiskScope::under(package.directory().join("src")),
            DiskScope::under(package.directory().join("tests")).excluding(&["fixtures"]),
        ];
        return Universe::declare(
            "the whole of bt-source, fixtures aside",
            package.targets().to_vec(),
            scopes,
            Vendor::Excluded,
        )
        .expect("this crate is where it says it is");
    }
    universes::whole_package(package, Vendor::Excluded).expect("a package of this workspace")
}

/// Every span of `search`, sorted.
fn spans(index: &Index, search: &Search) -> Vec<Span> {
    let mut found = index
        .search(search)
        .unwrap_or_else(|failure| panic!("{failure}"))
        .spans();
    found.sort();
    found
}

/// Every `#[test]` attribute of `index`, sorted.
fn test_attributes(index: &Index) -> Vec<Span> {
    spans(
        index,
        &Search::new(needle!(Pattern::text("#[test]")), View::CodeKeepingLiterals),
    )
}

/// Whether `item` is a callable carrying `#[test]`.
fn is_test(attributes: &[Span], item: &ItemRecord) -> bool {
    item.body().is_some() && item.kind().is_callable() && {
        let outer = item.declaration();
        let first = attributes.partition_point(|span| span.start() < outer.start());
        attributes[first..]
            .iter()
            .take_while(|span| span.start() < outer.end())
            .any(|span| span.within(outer))
    }
}

/// Whether callable `item` carries one of `attributes` among those written before its body.
fn carries(attributes: &[Span], item: &ItemRecord) -> bool {
    let outer = item.declaration();
    let body = item.body().map_or(outer.end(), Span::start);
    let first = attributes.partition_point(|span| span.start() < outer.start());
    attributes[first..]
        .iter()
        .take_while(|span| span.start() < body)
        .any(|span| span.within(outer))
}

/// Whether no product build compiles `record`: its file, or every arm it stands on, is
/// test-gated.
fn test_only(index: &Index, record: &ItemRecord) -> bool {
    !index.file_of(record).permits_product()
        || !record
            .identities()
            .any(|identity| identity.variant.permits_product())
}

/// The targets that compile `record`'s file, as `package:kind:name`.
fn targets(index: &Index, record: &ItemRecord) -> BTreeSet<String> {
    index
        .file_of(record)
        .owners()
        .iter()
        .map(|owner| owner.target.to_string())
        .collect()
}

/// The name a test answers to in `cargo test --list` of its target: its module path with the
/// crate root's `crate` left off. `main` for a target without the harness.
fn test_name(record: &ItemRecord) -> String {
    let declared = record.module_paths().into_iter().next().unwrap_or("crate");
    let path = declared
        .strip_prefix("crate")
        .map_or(declared, |rest| rest.trim_start_matches("::"));
    if path.is_empty() {
        record.name().to_owned()
    } else {
        format!("{path}::{}", record.name())
    }
}

/// `path` relative to the workspace root (or `root`), with forward slashes.
fn relative(path: &Path, root: &Path) -> String {
    normalized(path)
        .strip_prefix(normalized(root))
        .map_or_else(|_| path.to_path_buf(), Path::to_path_buf)
        .to_string_lossy()
        .replace('\\', "/")
}

/// `file:line` of a union offset of `index`, the file relative to `root`.
fn place(index: &Index, offset: usize, root: &Path) -> String {
    index.locate(offset).map_or_else(
        || format!("offset {offset}"),
        |location| format!("{}:{}", relative(location.file, root), location.line),
    )
}

// ── what a static is ──────────────────────────────────────────────────────────

/// The cell a process-global is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Store {
    Atomic,
    Lock,
    Once,
}

/// What a `static`'s written type makes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Plain(Store),
    /// A `LazyLock` of a plain value (`None`) or of a cell.
    Lazy(Option<Store>),
    StaticMut,
}

/// What a `static` is, by its written type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Global(Kind),
    /// A unit lock or a value-free primitive: what a `lock:` row names.
    UnitLock,
    NotMutable,
}

/// The shape of a `static` of type `ty`, `mutable` when it is a `static mut`.
fn shape_of(ty: &syn::Type, mutable: bool) -> Shape {
    if mutable {
        return Shape::Global(Kind::StaticMut);
    }
    let path = match ty {
        syn::Type::Path(path) => path,
        // An array or a slice of cells is the cell, element by element: `X[i].fetch_add(…)`
        // reads as the cell's method.
        syn::Type::Array(array) => return shape_of(&array.elem, false),
        syn::Type::Slice(slice) => return shape_of(&slice.elem, false),
        syn::Type::Paren(inner) => return shape_of(&inner.elem, false),
        syn::Type::Group(inner) => return shape_of(&inner.elem, false),
        _ => return Shape::NotMutable,
    };
    let Some(last) = path.path.segments.last() else {
        return Shape::NotMutable;
    };
    let name = last.ident.to_string();
    let argument = match &last.arguments {
        syn::PathArguments::AngleBracketed(arguments) => {
            arguments.args.iter().find_map(|argument| match argument {
                syn::GenericArgument::Type(ty) => Some(ty),
                _ => None,
            })
        }
        _ => None,
    };
    let unit = |ty: Option<&syn::Type>| matches!(ty, Some(syn::Type::Tuple(tuple)) if tuple.elems.is_empty());
    match name.as_str() {
        "Arc" | "Box" | "Rc" => argument.map_or(Shape::NotMutable, |inner| shape_of(inner, false)),
        "Mutex" | "RwLock" | "ReentrantMutex" => {
            if unit(argument) {
                Shape::UnitLock
            } else {
                Shape::Global(Kind::Plain(Store::Lock))
            }
        }
        "Condvar" | "Once" | "Barrier" => Shape::UnitLock,
        "OnceLock" | "OnceCell" => Shape::Global(Kind::Plain(Store::Once)),
        "LazyLock" | "LazyCell" | "Lazy" => {
            let inner = argument.map(|inner| shape_of(inner, false));
            Shape::Global(Kind::Lazy(match inner {
                Some(Shape::Global(Kind::Plain(cell))) => Some(cell),
                _ => None,
            }))
        }
        atomic if atomic.starts_with("Atomic") => Shape::Global(Kind::Plain(Store::Atomic)),
        _ => Shape::NotMutable,
    }
}

/// How one use of a global reads.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Access {
    Read,
    Write { op: String, once: bool },
    Undecided(String),
}

impl Access {
    fn writes(&self) -> bool {
        !matches!(self, Self::Read)
    }

    fn said(&self) -> String {
        match self {
            Self::Read => "read".to_owned(),
            Self::Write { op, .. } => op.clone(),
            Self::Undecided(what) => format!("undecided use {what}"),
        }
    }
}

/// How a method call on a cell reads.
fn method_of(cell: Store, method: &str, name: &str) -> Access {
    let write = |once: bool| Access::Write {
        op: format!(".{method}()"),
        once,
    };
    match (cell, method) {
        (Store::Atomic, "load")
        | (Store::Lock, "read" | "try_read" | "is_poisoned")
        | (Store::Once, "get" | "wait") => Access::Read,
        (
            Store::Atomic,
            "store"
            | "swap"
            | "compare_exchange"
            | "compare_exchange_weak"
            | "compare_and_swap"
            | "fetch_update",
        )
        | (Store::Lock, "lock" | "try_lock" | "write" | "try_write" | "get_mut" | "clear_poison")
        | (Store::Once, "take" | "get_mut") => write(false),
        (Store::Atomic, fetch) if fetch.starts_with("fetch_") => write(false),
        (Store::Once, "get_or_init" | "get_or_try_init" | "set") => write(true),
        _ => Access::Undecided(format!(
            "`{name}.{method}()`, a method the grammar does not read"
        )),
    }
}

/// How one use of a global of `kind` named `name` reads.
fn access(kind: Kind, context: &reading::Context, name: &str) -> Access {
    use reading::Context;
    let undecided = || {
        Access::Undecided(format!(
            "`{name}` {}: neither a read nor a write of the grammar",
            context.said()
        ))
    };
    match (kind, context) {
        (Kind::Plain(cell) | Kind::Lazy(Some(cell)), Context::Method(method)) => {
            method_of(cell, method, name)
        }
        (Kind::Lazy(None), Context::Assign | Context::MutRef) => undecided(),
        (Kind::Lazy(None), _) => Access::Read,
        (Kind::StaticMut, Context::Assign | Context::MutRef) => Access::Write {
            op: context.said(),
            once: false,
        },
        (Kind::StaticMut, Context::Method(_)) => undecided(),
        (Kind::StaticMut, _) => Access::Read,
        _ => undecided(),
    }
}

// ── the globals and the locks ─────────────────────────────────────────────────

/// One process-global, or one unit lock.
#[derive(Clone, Debug)]
struct Global {
    /// Its module path and name; for a body-declared one, its body's path and name.
    key: String,
    name: String,
    /// Its package's position.
    package: usize,
    /// The file it is written in, relative to the root.
    file: String,
    /// Where it is written, `file:line`.
    at: String,
    shape: Shape,
    /// The module it is written in, for a module's `static`; `None` for a body-declared one.
    module: Option<String>,
    /// The targets that compile its file: each has a copy of its own, and a test of one target
    /// reaches only that target's.
    targets: BTreeSet<String>,
}

/// The globals and unit locks of the packages handed in.
#[derive(Default)]
struct Globals {
    all: Vec<Global>,
    /// A module's `static`, by its package's position and its position in that package's
    /// values (a `ValueId`'s two halves).
    by_value: HashMap<(usize, usize), usize>,
    /// The statics declared in a body: the package's position, the body's whole span, and the
    /// global. A body written inside that body (an `impl` in a function) names it too.
    in_body: Vec<(usize, Span, usize)>,
    /// Every static declared in a body, of any type and any owner (a `#[test]` included), by
    /// package position and the owner's whole span: a name in its scope is that static.
    scoped: Vec<(usize, Span, String)>,
    /// The names of each package's globals, for what a macro's tokens name.
    names: HashMap<String, BTreeSet<String>>,
    /// Each package's names a module-level `use` binds, by module, with the absolute path each
    /// names: what makes a module's global nameable by its bare name outside its own module.
    imported: HashMap<(usize, String), BTreeMap<String, String>>,
    /// The modules each module's glob `use`s open (`use super::*;`), absolute.
    globbed: HashMap<(usize, String), BTreeSet<String>>,
    /// The packages' names, by position.
    packages: Vec<String>,
    /// Every `thread_local!` static, by package position and module path and name: not a
    /// global, and the index holds no record of it.
    thread_locals: BTreeSet<(usize, String)>,
}

/// `segments`, written in module `module`, as an absolute `crate::…` path: `crate`, `self` and
/// `super` are read; any other first segment is read as a child of `module`, which is what
/// it is unless it names another crate (whose globals are not this package's, so no key
/// matches it).
fn absolute(module: &str, segments: &[String]) -> String {
    let mut at: Vec<&str> = module.split("::").collect();
    let mut rest = segments;
    match rest.first().map(String::as_str) {
        Some("crate") => {
            at = vec!["crate"];
            rest = &rest[1..];
        }
        Some("self") => rest = &rest[1..],
        _ => {}
    }
    while rest.first().map(String::as_str) == Some("super") {
        if at.len() > 1 {
            at.pop();
        }
        rest = &rest[1..];
    }
    at.extend(rest.iter().map(String::as_str));
    at.join("::")
}

impl Globals {
    fn read(indices: &[(String, Index)], root: &Path) -> Self {
        let mut globals = Self {
            packages: indices.iter().map(|(name, _)| name.clone()).collect(),
            ..Self::default()
        };
        for (position, (package, index)) in indices.iter().enumerate() {
            let attributes = test_attributes(index);
            for (value_position, value) in index.values().iter().enumerate() {
                if value.kind() != ValueKind::Static || value.type_owner().is_some() {
                    continue;
                }
                let text = index.text(value.whole());
                let Ok(parsed) = syn::parse_str::<syn::ItemStatic>(text) else {
                    panic!(
                        "{}: a static whose text does not parse",
                        place(index, value.whole().start(), root)
                    );
                };
                let shape = shape_of(
                    &parsed.ty,
                    matches!(parsed.mutability, syn::StaticMutability::Mut(_)),
                );
                if shape == Shape::NotMutable {
                    continue;
                }
                let module = value
                    .module_paths()
                    .into_iter()
                    .next()
                    .unwrap_or("crate")
                    .to_owned();
                let file = index
                    .file_at(value.whole().start())
                    .expect("a value stands in a file of its index");
                globals
                    .by_value
                    .insert((position, value_position), globals.all.len());
                globals.add(
                    package,
                    Global {
                        key: format!("{module}::{}", value.name()),
                        name: value.name().to_owned(),
                        package: position,
                        file: relative(file.path(), root),
                        at: place(index, value.whole().start(), root),
                        shape,
                        module: Some(module),
                        targets: file
                            .owners()
                            .iter()
                            .map(|owner| owner.target.to_string())
                            .collect(),
                    },
                );
            }
            for import in index.uses() {
                for module in import.module_paths() {
                    globals
                        .imported
                        .entry((position, module.to_owned()))
                        .or_default()
                        .extend(
                            import
                                .names()
                                .iter()
                                .map(|(bound, path)| (bound.clone(), absolute(module, path))),
                        );
                    globals
                        .globbed
                        .entry((position, module.to_owned()))
                        .or_default()
                        .extend(import.globs().iter().map(|path| absolute(module, path)));
                }
            }
            // `thread_local!` statics written in a module, read from the macro's tokens.
            for record in index.macros() {
                if record.kind() != MacroKind::Invocation || record.name() != "thread_local" {
                    continue;
                }
                let Ok(file) = syn::parse_str::<syn::File>(index.text(record.tokens())) else {
                    continue;
                };
                let module = innermost_module(index, record.span());
                for item in &file.items {
                    if let syn::Item::Static(declared) = item {
                        globals
                            .thread_locals
                            .insert((position, format!("{module}::{}", declared.ident)));
                    }
                }
            }
            // Statics declared in a body: read from the body's own syntax.
            let mut owned: BTreeMap<usize, (Span, Global)> = BTreeMap::new();
            for record in index.items() {
                let Some(body) = record.body() else { continue };
                if !record.kind().is_callable() || !index.text(body).contains("static") {
                    continue;
                }
                let in_a_test = is_test(&attributes, record);
                let Ok(read) = reading::callable(
                    index.text(record.whole()),
                    record.kind() == ItemKind::Function,
                    record.type_owner().is_some(),
                ) else {
                    continue;
                };
                let base = record.whole().start();
                let owner_path = format!(
                    "{}::{}",
                    record.module_paths().into_iter().next().unwrap_or("crate"),
                    record.type_owner().map_or_else(
                        || record.name().to_owned(),
                        |owner| format!("{owner}::{}", record.name())
                    )
                );
                for declared in &read.statics {
                    globals
                        .scoped
                        .push((position, record.whole(), declared.name.clone()));
                    let shape = shape_of(&declared.ty, declared.mutable);
                    if shape == Shape::NotMutable || in_a_test {
                        continue;
                    }
                    let at = base + declared.at;
                    let global = Global {
                        key: format!("{owner_path}::{}", declared.name),
                        name: declared.name.clone(),
                        package: position,
                        file: relative(index.file_of(record).path(), root),
                        at: place(index, at, root),
                        shape,
                        module: None,
                        targets: targets(index, record),
                    };
                    match owned.get(&at) {
                        Some((known, _)) if known.len() <= record.whole().len() => {}
                        _ => {
                            owned.insert(at, (record.whole(), global));
                        }
                    }
                }
            }
            // Statics declared in a `#[test]` body belong to that test and are not globals; they
            // stay in `scoped`, so a name in their scope is theirs and nobody else's.
            for (_, (owner, global)) in owned {
                globals.in_body.push((position, owner, globals.all.len()));
                globals.add(package, global);
            }
        }
        globals
    }

    /// The body-declared globals in whose scope `record` (of the package at `position`) stands.
    fn declared_around(&self, position: usize, record: &ItemRecord) -> Vec<usize> {
        self.in_body
            .iter()
            .filter(|(at, owner, _)| *at == position && record.whole().within(*owner))
            .map(|(_, _, global)| *global)
            .collect()
    }

    /// Whether `name`, written bare in `record`, is a static declared in a body around it.
    fn scoped_around(&self, position: usize, record: &ItemRecord, name: &str) -> bool {
        self.scoped.iter().any(|(at, owner, declared)| {
            *at == position && declared == name && record.whole().within(*owner)
        })
    }

    /// Whether a module's global named `name` of the package at `position` can be what a bare
    /// `name` in module `module` names: it is written in that module, or a `use` there binds
    /// the name.
    fn nameable(&self, position: usize, module: &str, name: &str, package: &str) -> bool {
        self.names
            .get(package)
            .is_some_and(|names| names.contains(name))
            && (self.all.iter().any(|global| {
                global.package == position
                    && global.name == name
                    && global.module.as_deref() == Some(module)
            }) || self
                .imported
                .get(&(position, module.to_owned()))
                .is_some_and(|bound| bound.contains_key(name))
                || self.globbed_global(position, module, name).is_some())
    }

    /// The module-level global named `name` that a glob `use` of `module` brings in.
    fn globbed_global(&self, position: usize, module: &str, name: &str) -> Option<usize> {
        let opened = self.globbed.get(&(position, module.to_owned()))?;
        self.all.iter().position(|global| {
            global.package == position
                && global.name == name
                && global
                    .module
                    .as_ref()
                    .is_some_and(|declared| opened.contains(declared))
        })
    }

    /// The absolute paths a path written in `module` of the package at `position` may name,
    /// read without a call graph: for a bare name, its own module's, a named `use`'s and each
    /// glob `use`'s; for a longer path, through a `use` of its first segment, or as an absolute
    /// or relative path.
    fn places(&self, position: usize, module: &str, segments: &[String]) -> Vec<String> {
        let imported = self.imported.get(&(position, module.to_owned()));
        match segments {
            [] => Vec::new(),
            [name] => {
                let mut found = vec![format!("{module}::{name}")];
                found.extend(imported.and_then(|bound| bound.get(name)).cloned());
                found.extend(
                    self.globbed
                        .get(&(position, module.to_owned()))
                        .into_iter()
                        .flatten()
                        .map(|opened| format!("{opened}::{name}")),
                );
                found
            }
            [first, rest @ ..] => {
                let mut found = vec![imported.and_then(|bound| bound.get(first)).map_or_else(
                    || absolute(module, segments),
                    |path| format!("{path}::{}", rest.join("::")),
                )];
                // The call graph writes a path it cannot place without its leading `crate`, so a
                // path of plain segments is also read from the crate root.
                if !matches!(first.as_str(), "crate" | "self" | "super") {
                    found.push(format!("crate::{}", segments.join("::")));
                }
                found
            }
        }
    }

    /// The module-level global or unit lock a path written in `module` of the package at
    /// `position` names ([`Self::places`]).
    fn resolve(&self, position: usize, module: &str, segments: &[String]) -> Option<usize> {
        self.places(position, module, segments)
            .iter()
            .find_map(|key| {
                self.all.iter().position(|global| {
                    global.package == position && global.module.is_some() && global.key == *key
                })
            })
    }

    /// What the paths `written` in `module` of the package at `position` may name, as
    /// `<position>:<absolute path>` keys ([`Self::places`]): how a body names a type.
    fn named_keys<'p>(
        &self,
        position: usize,
        module: &str,
        written: impl IntoIterator<Item = &'p Vec<String>>,
    ) -> BTreeSet<String> {
        written
            .into_iter()
            .flat_map(|segments| self.places(position, module, segments))
            .map(|place| format!("{position}:{place}"))
            .collect()
    }

    /// Whether a path written in `module` names a `thread_local!` static of the package.
    fn names_a_thread_local(&self, position: usize, module: &str, segments: &[String]) -> bool {
        self.places(position, module, segments)
            .into_iter()
            .any(|key| self.thread_locals.contains(&(position, key)))
    }

    fn add(&mut self, package: &str, global: Global) {
        if matches!(global.shape, Shape::Global(_)) {
            self.names
                .entry(package.to_owned())
                .or_default()
                .insert(global.name.clone());
        }
        self.all.push(global);
    }
}

// ── one body, read ────────────────────────────────────────────────────────────

/// One use of a global in a body.
#[derive(Clone, Debug)]
struct Touch {
    global: usize,
    access: Access,
    site: String,
}

/// What one body says about globals.
#[derive(Clone, Debug, Default)]
struct Facts {
    touches: Vec<Touch>,
    /// The unit locks it names, by key.
    locks: BTreeSet<String>,
    refusals: Vec<String>,
    /// What the paths it writes may name, and its own type for a method, as
    /// [`Globals::named_keys`].
    named: BTreeSet<String>,
}

/// The call graph, the globals, and every body's facts, read once.
struct Reader<'r, 'i> {
    resolver: &'r Resolver<'i>,
    graph: CallGraph<'r, 'i>,
    globals: &'r Globals,
    root: &'r Path,
    facts: RefCell<HashMap<ItemId, std::rc::Rc<Facts>>>,
    reaches: RefCell<HashMap<ItemId, Reach>>,
    asking: RefCell<Vec<ItemId>>,
    leaned_on_open: Cell<bool>,
    test_callables: RefCell<HashMap<String, Vec<ItemId>>>,
    /// The test-only first-party impls of a foreign trait (`Drop`, winit's `ApplicationHandler`,
    /// `Display`), by every key their self type's name may be placed at from their module
    /// ([`Globals::named_keys`]): what an implicit call or a foreign callback runs
    /// ([`Resolver::implicit`]).
    implicit: HashMap<String, Vec<ItemId>>,
    /// The test-only `GlobalAlloc` impls: every allocation of every test of their target runs
    /// them.
    allocators: Vec<ItemId>,
    /// The functions of item-position macros of test code.
    macro_bodies: Vec<MacroBody>,
}

/// Whether a test-only callable reaches a global, through test-only callables of the target.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Reach {
    Yes,
    No,
    CannotTell(Vec<String>),
}

/// What one test's walk found.
#[derive(Default)]
struct Walk {
    touches: Vec<Touch>,
    /// The unit locks its reach names, by key.
    locks: BTreeSet<String>,
    refusals: Vec<String>,
    /// The implicitly run impls it took in.
    ran: BTreeSet<ItemId>,
    /// The item-macro functions it took in, by position.
    ran_macros: BTreeSet<usize>,
    /// The unresolved calls met: their candidates, the caller (for visibility) and what to say.
    /// Judged once the reach is whole, so a candidate already in the reach adds nothing.
    pending: Vec<(Vec<ItemId>, Option<ItemId>, String)>,
}

impl<'r, 'i> Reader<'r, 'i> {
    fn new(
        resolver: &'r Resolver<'i>,
        globals: &'r Globals,
        root: &'r Path,
        macro_bodies: Vec<MacroBody>,
    ) -> Self {
        let mut implicit: HashMap<String, Vec<ItemId>> = HashMap::new();
        let mut allocators = Vec::new();
        for (path, ids) in resolver.implicit() {
            for id in ids {
                let record = resolver.record(id);
                if !test_only(resolver.index_of(id), record) {
                    continue;
                }
                if path.rsplit("::").next() == Some("GlobalAlloc") {
                    allocators.push(id);
                } else if let Some(owner) = record.type_owner() {
                    let module = record.module_paths().into_iter().next().unwrap_or("crate");
                    for key in globals.named_keys(id.package(), module, &[vec![owner.to_owned()]]) {
                        implicit.entry(key).or_default().push(id);
                    }
                }
            }
        }
        Self {
            resolver,
            graph: CallGraph::new(resolver),
            globals,
            root,
            facts: RefCell::default(),
            reaches: RefCell::default(),
            asking: RefCell::default(),
            leaned_on_open: Cell::new(false),
            test_callables: RefCell::default(),
            implicit,
            allocators,
            macro_bodies,
        }
    }

    fn place(&self, id: ItemId, offset: usize) -> String {
        place(self.resolver.index_of(id), offset, self.root)
    }

    /// What body `id` says about globals and locks.
    fn facts(&self, id: ItemId) -> std::rc::Rc<Facts> {
        if let Some(known) = self.facts.borrow().get(&id) {
            return std::rc::Rc::clone(known);
        }
        let facts = std::rc::Rc::new(self.read_facts(id));
        self.facts
            .borrow_mut()
            .insert(id, std::rc::Rc::clone(&facts));
        facts
    }

    fn read_facts(&self, id: ItemId) -> Facts {
        let index = self.resolver.index_of(id);
        let record = self.resolver.record(id);
        let base = record.whole().start();
        let mut facts = Facts::default();
        let read = match reading::callable(
            index.text(record.whole()),
            record.kind() == ItemKind::Function,
            record.type_owner().is_some(),
        ) {
            Ok(read) => read,
            Err(why) => {
                facts.refusals.push(format!(
                    "{}: the text of `{}` does not parse ({why}), so the globals it names cannot be read",
                    self.place(id, base),
                    record.name()
                ));
                return facts;
            }
        };
        let module = record.module_paths().into_iter().next().unwrap_or("crate");
        facts.named = self.globals.named_keys(id.package(), module, &read.named);
        if let Some(owner) = record.type_owner() {
            facts.named.extend(self.globals.named_keys(
                id.package(),
                module,
                &[vec![owner.to_owned()]],
            ));
        }
        let uses: HashMap<usize, &reading::Use> =
            read.uses.iter().map(|used| (used.at, used)).collect();
        let mut seen = BTreeSet::new();
        let mut valued = BTreeSet::new();
        for edge in self.graph.edges(Node::Item(id)).iter() {
            let Target::Value(value) = &edge.to else {
                continue;
            };
            let at = edge.site.span.start();
            if let Some(offset) = at.checked_sub(base) {
                valued.insert(offset);
            }
            let Some(global) = self
                .globals
                .by_value
                .get(&(value.package(), value.value()))
                .copied()
            else {
                continue;
            };
            let site = self.place(id, at);
            let global_record = &self.globals.all[global];
            let Some(used) = at.checked_sub(base).and_then(|offset| uses.get(&offset)) else {
                if matches!(global_record.shape, Shape::Global(_)) {
                    facts.refusals.push(format!(
                        "{site}: the call graph reads a use of `{}` here that this reading does not",
                        global_record.name
                    ));
                }
                continue;
            };
            seen.insert(used.at);
            match global_record.shape {
                Shape::Global(kind) => facts.touches.push(Touch {
                    global,
                    access: access(kind, &used.context, &global_record.name),
                    site,
                }),
                Shape::UnitLock => {
                    facts.locks.insert(global_record.key.clone());
                }
                Shape::NotMutable => {}
            }
        }
        // The statics declared in this body, or in a body around it, named by their own names.
        let position = id.package();
        let declared: Vec<usize> = self.globals.declared_around(position, record);
        for used in &read.uses {
            if used.segments != 1 || seen.contains(&used.at) {
                continue;
            }
            if declared
                .iter()
                .any(|global| self.globals.all[*global].name == used.last)
            {
                seen.insert(used.at);
            }
            for global in &declared {
                let global_record = &self.globals.all[*global];
                if global_record.name != used.last {
                    continue;
                }
                match global_record.shape {
                    Shape::Global(kind) => facts.touches.push(Touch {
                        global: *global,
                        access: access(kind, &used.context, &global_record.name),
                        site: self.place(id, base + used.at),
                    }),
                    Shape::UnitLock => {
                        facts.locks.insert(global_record.key.clone());
                    }
                    Shape::NotMutable => {}
                }
            }
        }
        // A path the reading sees, named like a module's global, where the call graph places no
        // value: a use the graph does not read (inside a macro it does not parse, say).
        let package = self.resolver.package(id);
        let module = record.module_paths().into_iter().next().unwrap_or("crate");
        let nameable = |name: &str| {
            !read.local_names.contains(name)
                && !self.globals.scoped_around(position, record, name)
                && self.globals.nameable(position, module, name, package)
        };
        for used in &read.uses {
            if valued.contains(&used.at)
                || seen.contains(&used.at)
                || used.segments != 1
                || !nameable(&used.last)
            {
                continue;
            }
            facts.refusals.push(format!(
                "{}: `{}` is a process-global's name, and the call graph places no value here",
                self.place(id, base + used.at),
                used.last
            ));
        }
        // Macro arguments that do not parse, naming a global.
        for unparsed in &read.unparsed {
            for (at, ident) in &unparsed.idents {
                let named = nameable(ident)
                    || declared
                        .iter()
                        .any(|global| self.globals.all[*global].name == *ident);
                if named {
                    facts.refusals.push(format!(
                        "{}: the arguments of `{}!` do not parse as expressions, and they name `{ident}`, a process-global's name",
                        self.place(id, base + at),
                        unparsed.name
                    ));
                }
            }
        }
        facts
    }

    fn test_only(&self, id: ItemId) -> bool {
        test_only(self.resolver.index_of(id), self.resolver.record(id))
    }

    fn of_target(&self, id: ItemId, within: &BTreeSet<String>) -> bool {
        !targets(self.resolver.index_of(id), self.resolver.record(id)).is_disjoint(within)
    }

    /// Whether the language lets `from`'s body call `id`: a function or an inherent method
    /// written without `pub` is private to the module that declares it and its descendants.
    fn visible_from(&self, id: ItemId, from: ItemId) -> bool {
        let index = self.resolver.index_of(id);
        let record = self.resolver.record(id);
        if record.trait_name().is_some()
            || !reading::private(
                index.text(record.whole()),
                record.kind() == ItemKind::Function,
                record.type_owner().is_some(),
            )
        {
            return true;
        }
        let caller = self.resolver.record(from);
        record.module_paths().iter().any(|declared| {
            caller
                .module_paths()
                .iter()
                .any(|calling| calling == declared || calling.starts_with(&format!("{declared}::")))
        })
    }

    fn name(&self, id: ItemId) -> String {
        let record = self.resolver.record(id);
        record.type_owner().map_or_else(
            || record.name().to_owned(),
            |owner| format!("{owner}::{}", record.name()),
        )
    }

    fn test_callables(&self, package: &str) -> Vec<ItemId> {
        if let Some(known) = self.test_callables.borrow().get(package) {
            return known.clone();
        }
        let found: Vec<ItemId> = self
            .resolver
            .callables(package)
            .into_iter()
            .filter(|id| self.test_only(*id))
            .collect();
        self.test_callables
            .borrow_mut()
            .insert(package.to_owned(), found.clone());
        found
    }

    /// Whether test-only `id` reaches a global through test-only callables of `within`.
    fn reach(&self, id: ItemId, within: &BTreeSet<String>) -> Reach {
        if let Some(known) = self.reaches.borrow().get(&id) {
            return known.clone();
        }
        if self.asking.borrow().contains(&id) {
            self.leaned_on_open.set(true);
            return Reach::No;
        }
        let outer = self.leaned_on_open.replace(false);
        self.asking.borrow_mut().push(id);
        let walk = self.walk(id, within, false);
        let answer = if walk
            .touches
            .iter()
            .any(|touch| matches!(self.globals.all[touch.global].shape, Shape::Global(_)))
        {
            Reach::Yes
        } else if walk.refusals.is_empty() {
            Reach::No
        } else {
            Reach::CannotTell(walk.refusals)
        };
        self.asking.borrow_mut().pop();
        let leaned = self.leaned_on_open.get();
        if !leaned || self.asking.borrow().is_empty() {
            self.reaches.borrow_mut().insert(id, answer.clone());
        }
        self.leaned_on_open.set(outer || leaned);
        answer
    }

    /// A refusal for `what` when a candidate the caller may call reaches a global, or whether it
    /// does cannot be told.
    fn refuse_candidates(
        &self,
        candidates: impl IntoIterator<Item = ItemId>,
        from: Option<ItemId>,
        within: &BTreeSet<String>,
        what: &str,
    ) -> Option<String> {
        let mut reaching = Vec::new();
        let mut unknown = Vec::new();
        for candidate in candidates {
            if !self.test_only(candidate)
                || !self.of_target(candidate, within)
                || from.is_some_and(|from| !self.visible_from(candidate, from))
            {
                continue;
            }
            match self.reach(candidate, within) {
                Reach::Yes => reaching.push(self.name(candidate)),
                Reach::No => {}
                Reach::CannotTell(why) => {
                    unknown.push(format!("{} ({})", self.name(candidate), why.join("; ")));
                }
            }
        }
        if !reaching.is_empty() {
            return Some(format!(
                "{what}, and a candidate reaches a process-global: {}",
                reaching.join(", ")
            ));
        }
        if !unknown.is_empty() {
            return Some(format!(
                "{what}, and whether a candidate reaches a process-global cannot be told: {}",
                unknown.join(", ")
            ));
        }
        None
    }

    /// From `root`, every global and lock its reach names, and what the walk cannot decide.
    ///
    /// The reach grows to a fixed point: once the call edges are walked, every test-only impl
    /// of a foreign trait of the target whose self type a body of the reach names (an implicit
    /// call or a foreign callback runs it: a `Drop` at a scope's end, winit's `resumed`), every
    /// `GlobalAlloc` impl of the target, and every item-macro function whose type a body of the
    /// reach names are taken in, and walked in turn. The allocators are a test's alone
    /// (`from_a_test`): every test of the target runs them, so a callable asked whether it
    /// reaches a global does not answer for them. An unresolved call's candidates are judged
    /// when the reach is whole, and a candidate already in the reach adds nothing to it.
    fn walk(&self, root: ItemId, within: &BTreeSet<String>, from_a_test: bool) -> Walk {
        let mut stack = vec![root];
        let mut seen = BTreeSet::from([root]);
        let mut walk = Walk::default();
        let mut named: BTreeSet<String> = BTreeSet::new();
        let mut implicit: Vec<ItemId> = if from_a_test {
            self.allocators.clone()
        } else {
            Vec::new()
        };
        loop {
            while let Some(at) = stack.pop() {
                self.walk_one(at, within, &mut walk, &mut named, &mut stack, &mut seen);
            }
            for name in &named {
                if let Some(ids) = self.implicit.get(name) {
                    implicit.extend(ids.iter().copied());
                }
            }
            for id in implicit.drain(..) {
                if self.of_target(id, within) && seen.insert(id) {
                    walk.ran.insert(id);
                    stack.push(id);
                }
            }
            let mut grew = false;
            for (position, body) in self.macro_bodies.iter().enumerate() {
                if walk.ran_macros.contains(&position)
                    || body.named_by_keys.is_disjoint(&named)
                    || body.targets.is_disjoint(within)
                {
                    continue;
                }
                walk.ran_macros.insert(position);
                grew = true;
                walk.touches.extend(body.facts.touches.iter().cloned());
                walk.locks.extend(body.facts.locks.iter().cloned());
                walk.refusals.extend(body.facts.refusals.iter().cloned());
                named.extend(body.facts.named.iter().cloned());
                let package = &self.globals.packages[body.package];
                for (site, callee) in &body.calls {
                    let candidates = self
                        .test_callables(package)
                        .into_iter()
                        .filter(|id| self.resolver.record(*id).name() == callee)
                        .collect();
                    let what = format!(
                        "{site}: a call to `{callee}` in {}, which no call graph follows, read as \
                         any test-only callable of that name",
                        body.what
                    );
                    walk.pending.push((candidates, None, what));
                }
            }
            if stack.is_empty() && !grew {
                for (candidates, from, what) in std::mem::take(&mut walk.pending) {
                    let outside = candidates.into_iter().filter(|id| !seen.contains(id));
                    walk.refusals
                        .extend(self.refuse_candidates(outside, from, within, &what));
                }
                return walk;
            }
        }
    }

    /// One body of the walk: its facts, and its call edges followed or judged.
    fn walk_one(
        &self,
        at: ItemId,
        within: &BTreeSet<String>,
        walk: &mut Walk,
        named: &mut BTreeSet<String>,
        stack: &mut Vec<ItemId>,
        seen: &mut BTreeSet<ItemId>,
    ) {
        let facts = self.facts(at);
        walk.touches.extend(facts.touches.iter().cloned());
        walk.locks.extend(facts.locks.iter().cloned());
        walk.refusals.extend(facts.refusals.iter().cloned());
        named.extend(facts.named.iter().cloned());
        let index = self.resolver.index_of(at);
        let package = self.resolver.package(at);
        for edge in self.graph.edges(Node::Item(at)).iter() {
            let site = || place(index, edge.site.span.start(), self.root);
            match &edge.to {
                Target::Item(to) => {
                    if !self.test_only(*to) || !self.of_target(*to, within) {
                        continue;
                    }
                    if seen.insert(*to) {
                        stack.push(*to);
                    }
                }
                Target::Unresolved(
                    Unresolved::Dynamic { candidates, .. }
                    | Unresolved::Ambiguous { candidates, .. }
                    | Unresolved::OtherArm { candidates },
                ) => {
                    let what = format!(
                        "{}: a call to `{}` the call graph does not resolve",
                        site(),
                        edge.site.name
                    );
                    walk.pending.push((candidates.clone(), Some(at), what));
                }
                Target::Unresolved(Unresolved::Path { written, .. }) => {
                    let last = written.rsplit("::").next().unwrap_or(written);
                    let segments: Vec<String> = written.split("::").map(str::to_owned).collect();
                    let module = self
                        .resolver
                        .record(at)
                        .module_paths()
                        .into_iter()
                        .next()
                        .unwrap_or("crate");
                    if self
                        .globals
                        .names
                        .get(package)
                        .is_some_and(|names| names.contains(last))
                        && (self
                            .globals
                            .resolve(at.package(), module, &segments)
                            .is_some()
                            || !self
                                .globals
                                .names_a_thread_local(at.package(), module, &segments))
                    {
                        walk.refusals.push(format!(
                            "{}: the path `{written}` names a process-global's name, and the call \
                             graph places no value here",
                            site()
                        ));
                        continue;
                    }
                    let candidates = self
                        .test_callables(package)
                        .into_iter()
                        .filter(|id| self.resolver.record(*id).name() == last)
                        .collect();
                    let what = format!(
                        "{}: the path `{written}` names nothing the call graph places, and is \
                         read as any test-only callable of that name",
                        site()
                    );
                    walk.pending.push((candidates, Some(at), what));
                }
                Target::Unresolved(Unresolved::Unparsed { why }) => walk.refusals.push(format!(
                    "{}: a body on the walk does not parse ({why})",
                    site()
                )),
                Target::Unresolved(Unresolved::Cutoff { what }) => walk
                    .refusals
                    .push(format!("{}: the walk was cut off at {what}", site())),
                Target::Unresolved(Unresolved::Macro { name }) => {
                    let names = self.globals.names.get(package);
                    let defined = index.macros().iter().any(|record| {
                        record.kind() == MacroKind::Definition
                            && defined_macro(index, record) == name.rsplit("::").next()
                            && {
                                let tokens = index.text(record.tokens());
                                reading::words(tokens)
                                    .any(|word| names.is_some_and(|names| names.contains(word)))
                            }
                    });
                    if defined {
                        walk.refusals.push(format!(
                            "{}: the macro `{name}!`'s expansion is not followed, and its definition here names a process-global",
                            site()
                        ));
                    }
                }
                _ => {}
            }
        }
    }
}

/// The name a `macro_rules!` definition defines.
fn defined_macro<'i>(index: &'i Index, record: &bt_source::MacroRecord) -> Option<&'i str> {
    let written = index.text(record.span());
    let rest = written.split_once('!')?.1.trim_start();
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

// ── item-position macros ──────────────────────────────────────────────────────

/// One function written in the tokens of an item-position macro of test code
/// (`objc2::define_class!`): the index holds no record of it and the call graph no edge, so it
/// is read here, its paths placed by [`Globals::resolve`].
struct MacroBody {
    package: usize,
    targets: BTreeSet<String>,
    /// The type of its `impl` block, or its own name: a reach that names it runs it.
    named_by: String,
    /// `named_by` as its module places it ([`Globals::named_keys`]).
    named_by_keys: BTreeSet<String>,
    /// `file:line` and name, for a refusal.
    what: String,
    facts: Facts,
    /// Its calls, `file:line` and callee name: no call graph follows them.
    calls: Vec<(String, String)>,
}

/// An item-position macro of test code whose tokens do not parse as items.
struct UnreadMacro {
    package: usize,
    targets: BTreeSet<String>,
    at: String,
    name: String,
    words: BTreeSet<String>,
}

/// The module path of the innermost module `span` is written in.
fn innermost_module(index: &Index, span: Span) -> &str {
    index
        .modules()
        .iter()
        .filter(|module| span.within(module.span()))
        .min_by_key(|module| module.span().len())
        .and_then(|module| module.module_paths().first())
        .map_or("crate", String::as_str)
}

/// Every span of `spans` merged into disjoint runs, sorted.
fn merged(mut spans: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    spans.sort_unstable();
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for (start, end) in spans {
        match runs.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => runs.push((start, end)),
        }
    }
    runs
}

/// Whether `span` lies inside one of the disjoint sorted `runs`.
fn inside(runs: &[(usize, usize)], span: Span) -> bool {
    let after = runs.partition_point(|run| run.0 <= span.start());
    after > 0 && span.end() <= runs[after - 1].1
}

/// Every item-position macro invocation of test code — written outside every body, every
/// `const`/`static` initializer and every other macro — read as items: its functions, and the
/// macros whose tokens do not parse as items, and the refusals of items in them that are not
/// functions and name a global.
fn item_macros(
    indices: &[(String, Index)],
    globals: &Globals,
    root: &Path,
) -> (Vec<MacroBody>, Vec<UnreadMacro>, Vec<String>) {
    let mut bodies = Vec::new();
    let mut unread = Vec::new();
    let mut refusals = Vec::new();
    for (position, (package, index)) in indices.iter().enumerate() {
        let mut covered: Vec<(usize, usize)> = index
            .items()
            .iter()
            .filter_map(|record| record.body().map(|body| (body.start(), body.end())))
            .collect();
        covered.extend(
            index
                .values()
                .iter()
                .map(|value| (value.whole().start(), value.whole().end())),
        );
        covered.extend(
            index
                .macros()
                .iter()
                .map(|record| (record.tokens().start(), record.tokens().end())),
        );
        let covered = merged(covered);
        let names = globals.names.get(package.as_str());
        for record in index.macros() {
            if record.kind() != MacroKind::Invocation || inside(&covered, record.span()) {
                continue;
            }
            let Some(file) = index.file_at(record.span().start()) else {
                continue;
            };
            let module = index
                .modules()
                .iter()
                .filter(|module| record.span().within(module.span()))
                .min_by_key(|module| module.span().len());
            let module_path = innermost_module(index, record.span());
            let in_product =
                file.permits_product() && module.is_none_or(|module| module.permits_product());
            if in_product {
                continue;
            }
            let targets: BTreeSet<String> = file
                .owners()
                .iter()
                .map(|owner| owner.target.to_string())
                .collect();
            let tokens = record.tokens();
            let text = index.text(tokens);
            let at = |offset: usize| place(index, tokens.start() + offset, root);
            let Ok((fns, others)) = reading::items(text) else {
                unread.push(UnreadMacro {
                    package: position,
                    targets,
                    at: at(0),
                    name: record.name().to_owned(),
                    words: reading::words(text).map(str::to_owned).collect(),
                });
                continue;
            };
            let global_name = |word: &str| names.is_some_and(|names| names.contains(word));
            for range in others {
                if let Some(word) =
                    reading::words(&text[range.clone()]).find(|word| global_name(word))
                {
                    refusals.push(format!(
                        "{}: an item in `{}!` that is not a function names `{word}`, a process-global's name, and is not read",
                        at(range.start),
                        record.name()
                    ));
                }
            }
            for function in fns {
                let mut facts = Facts::default();
                let read = &function.reading;
                for used in &read.uses {
                    if used.segments == 1 && read.local_names.contains(&used.last) {
                        continue;
                    }
                    let site = at(used.at);
                    match globals.resolve(position, module_path, &used.path) {
                        Some(global) => match globals.all[global].shape {
                            Shape::Global(kind) => facts.touches.push(Touch {
                                global,
                                access: access(kind, &used.context, &globals.all[global].name),
                                site,
                            }),
                            Shape::UnitLock => {
                                facts.locks.insert(globals.all[global].key.clone());
                            }
                            Shape::NotMutable => {}
                        },
                        None if global_name(&used.last) => facts.refusals.push(format!(
                            "{site}: `{}` in `{}!` is a process-global's name, and this reading cannot place it",
                            used.path.join("::"),
                            record.name()
                        )),
                        None => {}
                    }
                }
                for unparsed in &read.unparsed {
                    for (offset, ident) in &unparsed.idents {
                        if global_name(ident) {
                            facts.refusals.push(format!(
                                "{}: the arguments of `{}!` do not parse as expressions, and they name `{ident}`, a process-global's name",
                                at(*offset),
                                unparsed.name
                            ));
                        }
                    }
                }
                facts.named = globals.named_keys(position, module_path, &read.named);
                let named_by_keys =
                    globals.named_keys(position, module_path, &[vec![function.named_by.clone()]]);
                bodies.push(MacroBody {
                    named_by_keys,
                    package: position,
                    targets: targets.clone(),
                    what: format!(
                        "{}: `{}::{}` in `{}!`",
                        at(function.at),
                        function.named_by,
                        function.name,
                        record.name()
                    ),
                    named_by: function.named_by,
                    calls: read
                        .calls
                        .iter()
                        .map(|(offset, name)| (at(*offset), name.clone()))
                        .collect(),
                    facts,
                });
            }
        }
    }
    (bodies, unread, refusals)
}

// ── the scan ──────────────────────────────────────────────────────────────────

/// One test's reach of one global.
#[derive(Debug)]
struct Reached {
    /// The accesses of its reach.
    touches: Vec<Touch>,
    /// The unit locks its reach names, by key.
    locks: BTreeSet<String>,
    /// Whether the test carries `#[ignore]`.
    ignored: bool,
}

/// One global of one binary that a test reaches: who reaches it, who writes it, and how.
#[derive(Default, Debug)]
struct Shared {
    /// Each test that reaches it, by its name and where it is written: two `cfg` arms of one
    /// test are one test of a build, each held to a row on its own.
    tests: BTreeMap<(String, String), Reached>,
}

impl Shared {
    /// The tests that reach it, by name.
    fn names(&self) -> BTreeSet<&str> {
        self.tests.keys().map(|(name, _)| name.as_str()).collect()
    }

    /// The tests whose reach writes it, by name, with the writes.
    fn writers(&self) -> BTreeMap<&str, Vec<&Touch>> {
        let mut writers: BTreeMap<&str, Vec<&Touch>> = BTreeMap::new();
        for ((test, _), reached) in &self.tests {
            let writes: Vec<&Touch> = reached
                .touches
                .iter()
                .filter(|touch| touch.access.writes())
                .collect();
            if !writes.is_empty() {
                writers.entry(test.as_str()).or_default().extend(writes);
            }
        }
        writers
    }
}

/// A register key: file, global, binary.
type Key = (String, String, String);

/// What the scan found.
struct Scan {
    globals: Globals,
    /// Every global some test of a binary reaches, by key.
    reached: BTreeMap<Key, Shared>,
    refusals: BTreeSet<String>,
    /// How many test-code functions were read.
    tests: usize,
}

impl Scan {
    /// The keys of the grammar: G1 (two tests or more) or G2 (written).
    fn hits(&self) -> BTreeMap<&Key, &'static str> {
        self.reached
            .iter()
            .filter_map(|(key, shared)| {
                if shared.names().len() >= 2 {
                    Some((key, "G1"))
                } else if !shared.writers().is_empty() {
                    Some((key, "G2"))
                } else {
                    None
                }
            })
            .collect()
    }

    /// What an unlisted hit says.
    fn said(&self, key: &Key, shape: &str) -> String {
        let shared = &self.reached[key];
        let at = self
            .globals
            .all
            .iter()
            .find(|global| global.key == key.1 && global.file == key.0)
            .map_or_else(|| key.0.clone(), |global| global.at.clone());
        let tests: Vec<&str> = shared.names().into_iter().collect();
        if shape == "G1" {
            format!(
                "{at}\t{}\t{}\tG1\treached by {} tests of {} ({}) and on no row of {LIST}",
                key.2,
                key.1,
                tests.len(),
                key.2,
                tests.join(", ")
            )
        } else {
            let (test, writes) = shared
                .writers()
                .into_iter()
                .next()
                .expect("a G2 hit is written");
            format!(
                "{at}\t{}\t{}\tG2\twritten ({}) by {test} and on no row of {LIST}",
                key.2,
                key.1,
                writes
                    .iter()
                    .map(|touch| format!("{} at {}", touch.access.said(), touch.site))
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        }
    }
}

/// Every test root of `index`: the `#[test]` callables, and the crate-root `main` of a target
/// without the harness.
fn test_roots<'a>(index: &'a Index, without_harness: &BTreeSet<String>) -> Vec<&'a ItemRecord> {
    let attributes = test_attributes(index);
    index
        .items()
        .iter()
        .filter(|record| {
            is_test(&attributes, record)
                || (record.kind() == ItemKind::Function
                    && record.name() == "main"
                    && record.body().is_some()
                    && index.file_of(record).owners().iter().any(|owner| {
                        owner.module_path == "crate"
                            && record.module_paths().contains(&"crate")
                            && without_harness.contains(&owner.target.to_string())
                    }))
        })
        .collect()
}

/// Whether every declaration of `item` is test code: out of every product build, or on a `cfg`
/// that names `test` (`test_order.rs`'s rule, so the floor counts what it counts).
fn is_test_code(item: &ItemRecord) -> bool {
    let mut identities = item.identities().peekable();
    identities.peek().is_some()
        && identities.all(|identity| {
            !identity.variant.permits_product()
                || identity
                    .variant
                    .predicates()
                    .iter()
                    .any(|predicate| names_test(predicate))
        })
}

/// Whether a `cfg` predicate, as written, names the `test` configuration — a whole word outside
/// any string, so `feature = "test-shell"` does not.
fn names_test(predicate: &str) -> bool {
    let mut outside = String::with_capacity(predicate.len());
    let mut quoted = false;
    for character in predicate.chars() {
        if character == '"' {
            quoted = !quoted;
            outside.push(' ');
        } else if !quoted {
            outside.push(character);
        }
    }
    outside
        .split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .any(|word| word == "test")
}

/// Scan `indices`: every global each test reaches, by binary.
fn scan(indices: &[(String, Index)], without_harness: &BTreeSet<String>, root: &Path) -> Scan {
    let packages: Vec<(&str, &Index)> = indices
        .iter()
        .map(|(name, index)| (name.as_str(), index))
        .collect();
    let resolver = Resolver::new(&packages, Arms::filtered(|_| true))
        .unwrap_or_else(|refused| panic!("{refused:?}"));
    let globals = Globals::read(indices, root);
    let (bodies, unread, mut refusals) = item_macros(indices, &globals, root);
    let reader = Reader::new(&resolver, &globals, root, bodies);
    let mut refusals: BTreeSet<String> = refusals.drain(..).collect();
    let mut reached: BTreeMap<Key, Shared> = BTreeMap::new();
    let mut ran: BTreeSet<ItemId> = BTreeSet::new();
    let mut ran_macros: BTreeSet<usize> = BTreeSet::new();
    let mut tests = 0;
    for (package, index) in indices {
        tests += index
            .items()
            .iter()
            .filter(|item| item.body().is_some() && item.kind().is_callable() && is_test_code(item))
            .count();
        let ignores = spans(
            index,
            &Search::new(
                needle!(Pattern::text("#[ignore")),
                View::CodeKeepingLiterals,
            ),
        );
        for record in test_roots(index, without_harness) {
            let id = resolver
                .id_of(package, record)
                .expect("a record of this index is the resolver's");
            let within = targets(index, record);
            let walk = reader.walk(id, &within, true);
            refusals.extend(walk.refusals);
            ran.extend(walk.ran);
            ran_macros.extend(walk.ran_macros);
            let name = test_name(record);
            let written_at = place(index, record.whole().start(), root);
            let ignored = carries(&ignores, record);
            let mut by_global: BTreeMap<usize, Vec<Touch>> = BTreeMap::new();
            for touch in walk.touches {
                if matches!(globals.all[touch.global].shape, Shape::Global(_)) {
                    by_global.entry(touch.global).or_default().push(touch);
                }
            }
            for (global, touches) in by_global {
                let global_record = &globals.all[global];
                for binary in within.intersection(&global_record.targets) {
                    reached
                        .entry((
                            global_record.file.clone(),
                            global_record.key.clone(),
                            binary.clone(),
                        ))
                        .or_default()
                        .tests
                        .insert(
                            (name.clone(), written_at.clone()),
                            Reached {
                                touches: touches.clone(),
                                locks: walk.locks.clone(),
                                ignored,
                            },
                        );
                }
            }
        }
    }
    // An impl an implicit call or a foreign callback runs, reaching a global, that no test's
    // reach names: what runs it cannot be told.
    let mut implicit: Vec<ItemId> = reader.allocators.clone();
    implicit.extend(reader.implicit.values().flatten().copied());
    for id in implicit {
        if ran.contains(&id) {
            continue;
        }
        let record = resolver.record(id);
        let within = targets(resolver.index_of(id), record);
        let why = match reader.reach(id, &within) {
            Reach::No => continue,
            Reach::Yes => "reaches a process-global".to_owned(),
            Reach::CannotTell(why) => format!(
                "and whether it reaches a process-global cannot be told ({})",
                why.join("; ")
            ),
        };
        refusals.insert(format!(
            "{}: `{}` runs by an implicit call or a foreign callback, {why}, and no test's reach \
             names `{}`",
            reader.place(id, record.whole().start()),
            reader.name(id),
            record.type_owner().unwrap_or("its type")
        ));
    }
    for (position, body) in reader.macro_bodies.iter().enumerate() {
        let touches = body
            .facts
            .touches
            .iter()
            .any(|touch| matches!(globals.all[touch.global].shape, Shape::Global(_)));
        if ran_macros.contains(&position) || (!touches && body.facts.refusals.is_empty()) {
            continue;
        }
        refusals.insert(format!(
            "{}, run by a foreign callback, names a process-global, and no test's reach names `{}`",
            body.what, body.named_by
        ));
    }
    // An item-position macro of test code whose tokens do not parse as items: refused when they
    // name a global, or a test-only callable of its target that reaches one.
    for macro_record in &unread {
        let package = &globals.packages[macro_record.package];
        let names = globals.names.get(package);
        if let Some(word) = macro_record
            .words
            .iter()
            .find(|word| names.is_some_and(|names| names.contains(*word)))
        {
            refusals.insert(format!(
                "{}: the tokens of `{}!` do not parse as items, and they name `{word}`, a \
                 process-global's name",
                macro_record.at, macro_record.name
            ));
            continue;
        }
        let candidates = reader
            .test_callables(package)
            .into_iter()
            .filter(|id| macro_record.words.contains(resolver.record(*id).name()));
        let what = format!(
            "{}: the tokens of `{}!` do not parse as items and name a test-only callable",
            macro_record.at, macro_record.name
        );
        refusals.extend(reader.refuse_candidates(candidates, None, &macro_record.targets, &what));
    }
    drop(reader);
    Scan {
        globals,
        reached,
        refusals,
        tests,
    }
}

// ── the register ──────────────────────────────────────────────────────────────

/// One row of the register.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    class: String,
    reason: String,
}

/// The rows of a register's text, refusing a malformed line, an unknown class, an undated
/// reason, and a row listed twice, by number.
fn rows_of(text: &str) -> BTreeMap<Key, Row> {
    let mut rows = BTreeMap::new();
    let mut header = false;
    for (number, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        let number = number + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !header {
            assert_eq!(line, HEADER, "{LIST}:{number}: the header");
            header = true;
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let [file, global, binary, class, reason] = fields[..] else {
            panic!("{LIST}:{number}: five tab-separated fields: {line}");
        };
        let known = class
            .strip_prefix("lock:")
            .is_some_and(|name| !name.is_empty())
            || CHECKED.contains(&class)
            || BY_HAND.contains(&class);
        assert!(known, "{LIST}:{number}: no class {class:?}");
        assert!(
            dated(reason),
            "{LIST}:{number}: a reason is `YYYY-MM-DD: why`: {reason:?}"
        );
        let fresh = rows
            .insert(
                (file.to_owned(), global.to_owned(), binary.to_owned()),
                Row {
                    class: class.to_owned(),
                    reason: reason.to_owned(),
                },
            )
            .is_none();
        assert!(fresh, "{LIST}:{number}: listed twice: {line}");
    }
    assert!(header, "{LIST} has its header");
    rows
}

/// Whether `reason` begins `YYYY-MM-DD: ` and says something after it.
fn dated(reason: &str) -> bool {
    let bytes = reason.as_bytes();
    bytes.len() > 12
        && bytes[..10].iter().enumerate().all(|(at, byte)| {
            if at == 4 || at == 7 {
                *byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
        && &reason[10..12] == ": "
        && !reason[12..].trim().is_empty()
}

/// The run conditions of `ignored-tests.txt`, by test name.
fn run_conditions(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let (name, condition) = line.split_once('\t')?;
            Some((name.to_owned(), condition.trim().to_owned()))
        })
        .collect()
}

/// What a row's class says that the tree does not hold, if anything.
fn class_broken(
    scan: &Scan,
    key: &Key,
    row: &Row,
    without_harness: &BTreeSet<String>,
    runs: &BTreeMap<String, String>,
) -> Option<String> {
    let shared = &scan.reached[key];
    let writers = shared.writers();
    if let Some(lock) = row.class.strip_prefix("lock:") {
        // The unit locks of this binary the row's name or path ends: exactly one, or the row
        // spells the lock by more of its module path.
        let suffix = format!("::{lock}");
        let named: Vec<&str> = scan
            .globals
            .all
            .iter()
            .filter(|global| {
                global.shape == Shape::UnitLock
                    && global.targets.contains(&key.2)
                    && (global.key == lock || global.key.ends_with(&suffix))
            })
            .map(|global| global.key.as_str())
            .collect();
        let [held] = named[..] else {
            return Some(if named.is_empty() {
                format!("no unit lock of {} is named `{lock}`", key.2)
            } else {
                format!(
                    "`{lock}` names {} unit locks of {}: {}; spell the one by its module path",
                    named.len(),
                    key.2,
                    named.join(", ")
                )
            });
        };
        let without: BTreeSet<&str> = shared
            .tests
            .iter()
            .filter(|(_, reached)| !reached.locks.contains(held))
            .map(|((test, _), _)| test.as_str())
            .collect();
        return (!without.is_empty()).then(|| {
            format!(
                "{} reach it without `{held}`",
                without.into_iter().collect::<Vec<_>>().join(", ")
            )
        });
    }
    match row.class.as_str() {
        "single writer" => (writers.len() != 1).then(|| {
            format!(
                "written by {} tests: {}",
                writers.len(),
                writers.keys().copied().collect::<Vec<_>>().join(", ")
            )
        }),
        "one main" => {
            (!without_harness.contains(&key.2)).then(|| format!("{} runs under the harness", key.2))
        }
        "run=alone" | "run=serial" => {
            // The list names a test by its bare name, so the test itself must carry `#[ignore]`:
            // another package's ignored test of the same name says nothing of this one.
            let off: BTreeSet<&str> = shared
                .tests
                .iter()
                .filter(|((test, _), reached)| {
                    !reached.ignored || runs.get(test) != Some(&row.class)
                })
                .map(|((test, _), _)| test.as_str())
                .collect();
            (!off.is_empty()).then(|| {
                format!(
                    "not an ignored test {} on {IGNORED}: {}",
                    row.class,
                    off.into_iter().collect::<Vec<_>>().join(", ")
                )
            })
        }
        "write-once" => {
            let other: Vec<String> = writers
                .values()
                .flatten()
                .filter(|touch| !matches!(touch.access, Access::Write { once: true, .. }))
                .map(|touch| format!("{} at {}", touch.access.said(), touch.site))
                .collect();
            (!other.is_empty()).then(|| format!("written other than once: {}", other.join("; ")))
        }
        _ => None,
    }
}

/// Hold `scan` to the register `rows`: the unlisted hits, the stale rows, and the rows whose
/// checked class the tree does not hold.
fn judge(
    scan: &Scan,
    rows: &BTreeMap<Key, Row>,
    without_harness: &BTreeSet<String>,
    runs: &BTreeMap<String, String>,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let hits = scan.hits();
    let unlisted: Vec<String> = hits
        .iter()
        .filter(|(key, _)| !rows.contains_key(**key))
        .map(|(key, shape)| scan.said(key, shape))
        .collect();
    let stale: Vec<String> = rows
        .keys()
        .filter(|key| !hits.contains_key(key))
        .map(|key| {
            format!(
                "{}\t{}\t{}: no longer reached by two tests or written by one",
                key.0, key.1, key.2
            )
        })
        .collect();
    let broken: Vec<String> = rows
        .iter()
        .filter(|(key, _)| hits.contains_key(key))
        .filter_map(|(key, row)| {
            class_broken(scan, key, row, without_harness, runs)
                .map(|why| format!("{}\t{}\t{}\t{}: {why}", key.0, key.1, key.2, row.class))
        })
        .collect();
    (unlisted, stale, broken)
}

// ── the workspace ─────────────────────────────────────────────────────────────

/// Every first-party package's index, and the targets that run without the harness.
fn workspace() -> (Vec<(String, Index)>, BTreeSet<String>) {
    let workspace = Workspace::read(&workspace_root()).expect("this workspace");
    let mut without_harness = BTreeSet::new();
    let indices = workspace
        .packages()
        .iter()
        .filter(|package| !is_vendored(package.directory()))
        .map(|package| {
            without_harness.extend(
                package
                    .targets()
                    .iter()
                    .filter(|target| package.runs_without_harness(&target.id))
                    .map(|target| target.id.to_string()),
            );
            let index = Index::build(&universe_for(package))
                .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
            (package.name().to_owned(), index)
        })
        .collect();
    (indices, without_harness)
}

/// The register.
fn register() -> BTreeMap<Key, Row> {
    rows_of(
        &std::fs::read_to_string(workspace_root().join(LIST)).expect("the register is committed"),
    )
}

/// The ignored tests' run conditions.
fn ignored() -> BTreeMap<String, String> {
    run_conditions(
        &std::fs::read_to_string(workspace_root().join(IGNORED))
            .expect("the ignored-test list is committed"),
    )
}

/// RED (T-TEST-PROCESS-GLOBALS) — **every process-global that two tests of one binary reach, or
/// that a test writes, is a row of the register with a class the tree holds; every row still is
/// such a global; and nothing the walk cannot decide is passed over.**
///
/// MUTATION: drop `let _writers = WRITERS…` from one of bt-effects' admission refusal tests and
/// its `lock:WRITERS` rows name that test; delete a row and this names the global under G1 or
/// G2; add a row for a global no test reaches and this names it as stale.
#[test]
fn every_process_global_tests_share_or_write_is_on_the_register() {
    let (indices, without_harness) = workspace();
    let root = normalized(&workspace_root());
    let scan = scan(&indices, &without_harness, &root);
    assert!(
        scan.tests > 1000,
        "the scan read {} test functions; the workspace has thousands",
        scan.tests
    );
    assert!(
        scan.refusals.is_empty(),
        "the process-globals scan cannot decide these, and does not pass over them ({}):\n  {}",
        scan.refusals.len(),
        scan.refusals
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    let rows = register();
    let (unlisted, stale, broken) = judge(&scan, &rows, &without_harness, &ignored());
    println!(
        "{} process-globals reached by tests, {} hits, {} rows",
        scan.reached.len(),
        scan.hits().len(),
        rows.len()
    );
    assert!(
        unlisted.is_empty() && stale.is_empty() && broken.is_empty(),
        "{LIST} and the tree disagree.\n\n\
         Process-globals two tests share or a test writes, on no row ({}) — make the state the \
         test's own (a value handed in, a key per test), take one lock in every test that \
         reaches it, or add a row with its class and a dated reason:\n  {}\n\n\
         Rows that are no longer a hit ({}) — remove the row:\n  {}\n\n\
         Rows whose class the tree does not hold ({}):\n  {}",
        unlisted.len(),
        unlisted.join("\n  "),
        stale.len(),
        stale.join("\n  "),
        broken.len(),
        broken.join("\n  "),
    );
}

/// A row listed twice is refused by its line, even with another reason.
///
/// MUTATION: drop the `fresh` assertion in `rows_of` and this does not panic.
#[test]
#[should_panic(expected = "4: listed twice")]
fn a_row_listed_twice_is_refused() {
    let _ = rows_of(&format!(
        "# a register\n{HEADER}\na.rs\tcrate::X\tp:lib:p\tmonotone\t2026-10-24: one\n\
         a.rs\tcrate::X\tp:lib:p\tmonotone\t2026-10-24: another\n"
    ));
}

/// An undated reason and an unknown class are refused by their line.
///
/// MUTATION: drop `dated` from `rows_of` and the first does not panic.
#[test]
fn a_row_needs_a_dated_reason_and_a_known_class() {
    let undated = std::panic::catch_unwind(|| {
        rows_of(&format!(
            "{HEADER}\na.rs\tcrate::X\tp:lib:p\tmonotone\tit only grows\n"
        ))
    });
    assert!(undated.is_err(), "an undated reason is refused");
    let unknown = std::panic::catch_unwind(|| {
        rows_of(&format!(
            "{HEADER}\na.rs\tcrate::X\tp:lib:p\tfine\t2026-10-24: it is\n"
        ))
    });
    assert!(unknown.is_err(), "an unknown class is refused");
    let lock = rows_of(&format!(
        "{HEADER}\na.rs\tcrate::X\tp:lib:p\tlock:TURN\t2026-10-24: it is\n"
    ));
    assert_eq!(lock.len(), 1);
}

// ── the fixture ───────────────────────────────────────────────────────────────

/// The `test_globals` fixture: the library `globals`, and its test target `main_probe`, which
/// runs without the harness.
fn fixture() -> (Vec<(String, Index)>, BTreeSet<String>, PathBuf) {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("test_globals");
    let probe = TargetId {
        package: "globals".to_owned(),
        kind: TargetKind::IntegrationTest,
        name: "main_probe".to_owned(),
    };
    let without_harness = BTreeSet::from([probe.to_string()]);
    let universe = Universe::declare(
        "the test_globals fixture",
        vec![
            TargetRoot {
                id: TargetId {
                    package: "globals".to_owned(),
                    kind: TargetKind::Library,
                    name: "globals".to_owned(),
                },
                file: directory.join("lib.rs"),
            },
            TargetRoot {
                id: probe,
                file: directory.join("main_probe.rs"),
            },
        ],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    let index =
        Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
    (
        vec![("globals".to_owned(), index)],
        without_harness,
        directory,
    )
}

/// The fixture's own register: each checked class held and broken, and a stale row.
const FIXTURE_ROWS: &str = "file\tglobal\tbinary\tclass\treason
lib.rs\tcrate::tests::LOCKED\tglobals:lib:globals\tlock:TURN\t2026-10-24: every test takes TURN
lib.rs\tcrate::tests::HALF_LOCKED\tglobals:lib:globals\tlock:TURN\t2026-10-24: one test forgets TURN
lib.rs\tcrate::tests::ARMED_TWICE\tglobals:lib:globals\tlock:TURN\t2026-10-24: one arm of a test forgets TURN
lib.rs\tcrate::tests::GATED\tglobals:lib:globals\tlock:GATE_TURN\t2026-10-24: two locks answer to GATE_TURN
lib.rs\tcrate::tests::GATED_TOO\tglobals:lib:globals\tlock:gated::GATE_TURN\t2026-10-24: spelled by its module path
lib.rs\tcrate::tests::ONE_WRITER\tglobals:lib:globals\tsingle writer\t2026-10-24: one test writes it
lib.rs\tcrate::tests::TWO_WRITERS\tglobals:lib:globals\tsingle writer\t2026-10-24: two tests write it
lib.rs\tcrate::tests::GUARDED\tglobals:lib:globals\tsingle writer\t2026-10-24: a drop guard is a second writer
lib.rs\tcrate::tests::FAMILIES\tglobals:lib:globals\twrite-once\t2026-10-24: a LazyLock of a fixed list
lib.rs\tcrate::tests::RESET\tglobals:lib:globals\twrite-once\t2026-10-24: stored, so not write-once
lib.rs\tcrate::tests::ALONE\tglobals:lib:globals\trun=alone\t2026-10-24: listed, but its tests carry no #[ignore]
lib.rs\tcrate::tests::ALONE_HELD\tglobals:lib:globals\trun=alone\t2026-10-24: ignored and listed run=alone
lib.rs\tcrate::tests::WRITTEN\tglobals:lib:globals\tone main\t2026-10-24: the library runs under the harness
main_probe.rs\tcrate::PROBED\tglobals:test:main_probe\tone main\t2026-10-24: written by its main
main_probe.rs\tcrate::RESUMED\tglobals:test:main_probe\tone main\t2026-10-24: written by a callback for a type main names
main_probe.rs\tcrate::COUNTED\tglobals:test:main_probe\tone main\t2026-10-24: written by its global allocator
lib.rs\tcrate::tests::GROWS\tglobals:lib:globals\tmonotone\t2026-10-24: by hand
lib.rs\tcrate::tests::READ_ONCE\tglobals:lib:globals\tmonotone\t2026-10-24: one test reads it, so stale
";

/// RED (T-TEST-PROCESS-GLOBALS) — **each shape of the grammar is found, its accepted twin is
/// not, a resolved helper, an implicit call (a drop guard, a global allocator) and a foreign
/// callback (winit's `resumed`, a function in `define_class!`'s tokens) carry a global to the
/// tests whose reach runs them, what the walk cannot decide is refused by file and line, each
/// checked class is held and broken, and a global reached only through a product function is
/// not seen (the blind spot, pinned).**
///
/// MUTATIONS: in `Reader::walk`, follow no test-only callable — `HELPED` and the helper's
/// `CACHE` are not found; take in no implicit impl — `GUARDED`'s `single writer` row holds
/// and `RESUMED`/`COUNTED` are stale; take in no item-macro function — `PRESSED` is not found;
/// read `lock` as a read in `method_of` — `WRITTEN_BY_LOCK` is not found; skip the `Ambiguous`
/// arm — the `poke` refusal is gone; `place` for `place_written` — `SPOT` and `TALLIES` are
/// not found; drop the `Path` arm's global-name refusal — `LOST`'s refusal is gone; drop
/// `globbed` from `nameable` — `GLOBBED`'s refusal is gone; key `Shared::tests` by name alone
/// — `ARMED_TWICE`'s row holds; match a lock row by bare name — `GATED`'s row holds; drop the
/// `#[ignore]` requirement — `ALONE`'s row holds; read arrays as not cells — `SLOTS` is not
/// found.
#[test]
fn the_grammar_refuses_each_shape_and_accepts_its_twin() {
    let (indices, without_harness, directory) = fixture();
    let scan = scan(&indices, &without_harness, &directory);
    let found: Vec<String> = scan
        .hits()
        .iter()
        .map(|(key, shape)| format!("{} {}", key.1, shape))
        .collect();
    assert_eq!(
        found,
        [
            "crate::tests::ALONE G1",
            "crate::tests::ALONE_HELD G1",
            "crate::tests::ARMED_TWICE G1",
            "crate::tests::FAMILIES G1",
            "crate::tests::GATED G1",
            "crate::tests::GATED_TOO G1",
            "crate::tests::GROWS G2",
            "crate::tests::GUARDED G1",
            "crate::tests::HALF_LOCKED G1",
            "crate::tests::HELPED G1",
            "crate::tests::LOCKED G1",
            "crate::tests::ONE_WRITER G1",
            "crate::tests::PRESSED G2",
            "crate::tests::RESET G2",
            "crate::tests::SHARED G1",
            "crate::tests::SLOTS G2",
            "crate::tests::SPOT G2",
            "crate::tests::TALLIES G2",
            "crate::tests::TWO_WRITERS G1",
            "crate::tests::UNDECIDED G2",
            "crate::tests::WRITTEN G2",
            "crate::tests::WRITTEN_BY_LOCK G2",
            "crate::tests::cached::CACHE G1",
            "crate::COUNTED G2",
            "crate::PROBED G2",
            "crate::RESUMED G2",
        ],
        "{found:#?}"
    );
    let refusals: Vec<&String> = scan.refusals.iter().collect();
    let refused = |needle: &str| {
        refusals
            .iter()
            .any(|refusal| refusal.starts_with("lib.rs:") && refusal.contains(needle))
    };
    assert_eq!(refusals.len(), 4, "{refusals:#?}");
    assert!(
        refused("`poke`") && refused("a candidate reaches a process-global: Poker::poke"),
        "{refusals:#?}"
    );
    assert!(
        refused("`Orphan::drop` runs by an implicit call or a foreign callback")
            && refused("no test's reach names `Orphan`"),
        "{refusals:#?}"
    );
    assert!(
        refused("the path `missing::LOST` names a process-global's name"),
        "{refusals:#?}"
    );
    assert!(
        refused(
            "the arguments of `odd_macro!` do not parse as expressions, and they name `GLOBBED`"
        ),
        "{refusals:#?}"
    );
    let undecided = scan.said(
        &(
            "lib.rs".to_owned(),
            "crate::tests::UNDECIDED".to_owned(),
            "globals:lib:globals".to_owned(),
        ),
        "G2",
    );
    assert!(
        undecided.contains("undecided use `UNDECIDED` handed by reference"),
        "{undecided}"
    );

    let rows = rows_of(FIXTURE_ROWS);
    let runs: BTreeMap<String, String> = [
        "tests::alone_once",
        "tests::alone_twice",
        "tests::alone_held_once",
        "tests::alone_held_twice",
    ]
    .into_iter()
    .map(|test| (test.to_owned(), "run=alone".to_owned()))
    .collect();
    let (unlisted, stale, broken) = judge(&scan, &rows, &without_harness, &runs);
    let unlisted: Vec<&str> = unlisted
        .iter()
        .map(|line| line.split('\t').nth(2).unwrap_or(line))
        .collect();
    assert_eq!(
        unlisted,
        [
            "crate::tests::HELPED",
            "crate::tests::PRESSED",
            "crate::tests::SHARED",
            "crate::tests::SLOTS",
            "crate::tests::SPOT",
            "crate::tests::TALLIES",
            "crate::tests::UNDECIDED",
            "crate::tests::WRITTEN_BY_LOCK",
            "crate::tests::cached::CACHE",
        ]
    );
    assert_eq!(
        stale,
        [
            "lib.rs\tcrate::tests::READ_ONCE\tglobals:lib:globals: no longer reached by two tests or written by one"
        ]
    );
    let broken_globals: Vec<&str> = broken
        .iter()
        .map(|line| line.split('\t').nth(1).unwrap_or(line))
        .collect();
    assert_eq!(
        broken_globals,
        [
            "crate::tests::ALONE",
            "crate::tests::ARMED_TWICE",
            "crate::tests::GATED",
            "crate::tests::GUARDED",
            "crate::tests::HALF_LOCKED",
            "crate::tests::RESET",
            "crate::tests::TWO_WRITERS",
            "crate::tests::WRITTEN",
        ],
        "{broken:#?}"
    );
    let says = |global: &str, needle: &str| {
        broken
            .iter()
            .any(|line| line.contains(&format!("\t{global}\t")) && line.contains(needle))
    };
    assert!(
        says("crate::tests::ALONE", "not an ignored test run=alone"),
        "{broken:#?}"
    );
    assert!(
        says("crate::tests::ARMED_TWICE", "on_each_arm reach it without"),
        "{broken:#?}"
    );
    assert!(
        says("crate::tests::GATED", "spell the one by its module path"),
        "{broken:#?}"
    );
    assert!(
        says(
            "crate::tests::GUARDED",
            "written by 2 tests: tests::guarded_is_reset_by_a_drop, tests::guarded_writes"
        ),
        "{broken:#?}"
    );
    assert!(
        says(
            "crate::tests::WRITTEN",
            "globals:lib:globals runs under the harness"
        ),
        "{broken:#?}"
    );
}

// ── product reach, measured ───────────────────────────────────────────────────

/// **What the blind spot holds, as a number** (T-TEST-PROCESS-GLOBALS ruling 1): the product
/// functions' writes of process-globals that two tests of one binary reach through resolved
/// edges, product code included, and the unresolved edges met on the way. A measure, printed as
/// `TEST_GLOBALS_PRODUCT_REACH` (CI's step "the tests' product reach of process-globals,
/// measured" runs it with `--nocapture`), for a ticket that would gate product reach; never a
/// gate, and never compared. It reads the workspace on its own: run beside the gate in one test
/// binary, the second index costs no wall time (2026-10-24, this machine: 22.6 s for the binary,
/// 24.0 s for the gate alone, 18.9 s for this alone).
#[test]
fn the_product_reach_of_the_tests_is_measured() {
    let (indices, without_harness) = workspace();
    let root = normalized(&workspace_root());
    let packages: Vec<(&str, &Index)> = indices
        .iter()
        .map(|(name, index)| (name.as_str(), index))
        .collect();
    let resolver = Resolver::new(&packages, Arms::filtered(|_| true))
        .unwrap_or_else(|refused| panic!("{refused:?}"));
    let globals = Globals::read(&indices, &root);
    let reader = Reader::new(&resolver, &globals, &root, Vec::new());

    // Every callable's resolved callees, reversed.
    let mut callers: HashMap<ItemId, Vec<ItemId>> = HashMap::new();
    let mut writers: BTreeMap<usize, BTreeSet<ItemId>> = BTreeMap::new();
    for (package, _) in &indices {
        for id in resolver.callables(package) {
            for edge in reader.graph.edges(Node::Item(id)).iter() {
                if let Target::Item(to) = &edge.to {
                    callers.entry(*to).or_default().push(id);
                }
            }
            if reader.test_only(id) {
                continue;
            }
            let touches_any = reader.graph.edges(Node::Item(id)).iter().any(|edge| {
                matches!(&edge.to, Target::Value(value)
                    if globals.by_value.contains_key(&(value.package(), value.value())))
            }) || !globals
                .declared_around(id.package(), resolver.record(id))
                .is_empty();
            if !touches_any {
                continue;
            }
            for touch in &reader.facts(id).touches {
                if matches!(globals.all[touch.global].shape, Shape::Global(_))
                    && touch.access.writes()
                {
                    writers.entry(touch.global).or_default().insert(id);
                }
            }
        }
    }
    // Every test, by its id, with its binaries.
    let mut tests: HashMap<ItemId, BTreeSet<String>> = HashMap::new();
    for (package, index) in &indices {
        for record in test_roots(index, &without_harness) {
            let id = resolver
                .id_of(package, record)
                .expect("a record of this index is the resolver's");
            tests.insert(id, targets(index, record));
        }
    }
    let mut written = 0;
    for bodies in writers.values() {
        let mut seen: BTreeSet<ItemId> = bodies.iter().copied().collect();
        let mut stack: Vec<ItemId> = seen.iter().copied().collect();
        let mut by_binary: BTreeMap<&str, usize> = BTreeMap::new();
        while let Some(at) = stack.pop() {
            if let Some(binaries) = tests.get(&at) {
                for binary in binaries {
                    *by_binary.entry(binary.as_str()).or_default() += 1;
                }
            }
            for caller in callers.get(&at).into_iter().flatten() {
                if seen.insert(*caller) {
                    stack.push(*caller);
                }
            }
        }
        if by_binary.values().any(|count| *count >= 2) {
            written += 1;
        }
    }
    let roots: Vec<ItemId> = tests.keys().copied().collect();
    let reach = reader.graph.walk(&roots, &BTreeSet::new());
    let unresolved: BTreeSet<(usize, String)> = reach
        .unresolved
        .iter()
        .map(|(edge, _)| (edge.site.span.start(), edge.site.name.clone()))
        .collect();
    println!(
        "TEST_GLOBALS_PRODUCT_REACH\twritten by product {}\treached so by two tests of a binary \
         {written}\tunresolved edges on the walk {}",
        writers.len(),
        unresolved.len()
    );
}

// ── the reading ───────────────────────────────────────────────────────────────

/// A callable's text, read for the paths it names and the statics it declares.
mod reading {
    use proc_macro2::{TokenStream, TokenTree};
    use syn::punctuated::Punctuated;
    use syn::visit::{self, Visit};
    use syn::{Expr, Token};

    /// Where a path in value position stands.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Context {
        /// The receiver of a method call, by the method's name.
        Method(String),
        /// The place an `=` or an `op=` writes.
        Assign,
        /// Under `&mut`, `&raw mut` or `addr_of_mut!`.
        MutRef,
        /// Under `&`, `&raw const` or `addr_of!`.
        Ref,
        Field,
        Index,
        Deref,
        /// Anything else: an argument, a binding's value, a call's callee.
        Bare,
    }

    impl Context {
        pub fn said(&self) -> String {
            match self {
                Self::Method(method) => format!("`.{method}()`"),
                Self::Assign => "assigned".to_owned(),
                Self::MutRef => "borrowed `&mut`".to_owned(),
                Self::Ref => "handed by reference".to_owned(),
                Self::Field => "read by a field".to_owned(),
                Self::Index => "indexed".to_owned(),
                Self::Deref => "dereferenced".to_owned(),
                Self::Bare => "used as a value".to_owned(),
            }
        }
    }

    /// One path in value position.
    #[derive(Debug)]
    pub struct Use {
        /// The offset of its last segment, in the text handed to [`callable`].
        pub at: usize,
        pub last: String,
        /// Its segments as written, `super::X` as `["super", "X"]`.
        pub path: Vec<String>,
        pub segments: usize,
        pub context: Context,
    }

    /// One `static` declared in the body.
    pub struct Declared {
        pub at: usize,
        pub name: String,
        pub ty: syn::Type,
        pub mutable: bool,
    }

    /// One macro whose arguments do not parse as expressions: its identifiers.
    pub struct Unparsed {
        pub name: String,
        pub idents: Vec<(usize, String)>,
    }

    #[derive(Default)]
    pub struct Reading {
        pub uses: Vec<Use>,
        pub statics: Vec<Declared>,
        pub unparsed: Vec<Unparsed>,
        /// The `const`s and `static`s written in the body, by name: a bare path of one of
        /// those names is the body's own.
        pub local_names: std::collections::BTreeSet<String>,
        /// Every prefix of every path the body writes (a type, a value, a struct literal, a
        /// pattern), as segments, and every identifier of a macro argument it cannot parse:
        /// what names a type whose impl an implicit call or a foreign callback runs.
        pub named: std::collections::BTreeSet<Vec<String>>,
        /// Every call the body writes, by its offset and the callee's last name: a path call's
        /// last segment, a method call's method. Read only for a body no call graph walks.
        pub calls: Vec<(usize, String)>,
    }

    /// Read a callable's whole text.
    pub fn callable(text: &str, function: bool, owned: bool) -> Result<Reading, syn::Error> {
        let mut reader = Reader::default();
        if function {
            let parsed = syn::parse_str::<syn::ItemFn>(text)?;
            reader.visit_signature(&parsed.sig);
            reader.visit_block(&parsed.block);
        } else if owned {
            let parsed = syn::parse_str::<syn::ImplItemFn>(text)?;
            reader.visit_signature(&parsed.sig);
            reader.visit_block(&parsed.block);
        } else {
            let parsed = syn::parse_str::<syn::TraitItemFn>(text)?;
            reader.visit_signature(&parsed.sig);
            if let Some(block) = &parsed.default {
                reader.visit_block(block);
            }
        }
        Ok(reader.reading)
    }

    /// One function written inside an item-position macro's tokens.
    pub struct MacroFn {
        /// What runs it when written: the type of its `impl` block, or its own name for a free
        /// function.
        pub named_by: String,
        pub name: String,
        /// Its offset in the tokens.
        pub at: usize,
        pub reading: Reading,
    }

    /// The tokens of an item-position macro, read as items: every function in them, through
    /// `impl` blocks and inline modules, and the byte ranges of every other item.
    pub fn items(text: &str) -> Result<(Vec<MacroFn>, Vec<std::ops::Range<usize>>), syn::Error> {
        fn read_items(
            items: &[syn::Item],
            fns: &mut Vec<MacroFn>,
            others: &mut Vec<std::ops::Range<usize>>,
        ) {
            use syn::spanned::Spanned;
            for item in items {
                match item {
                    syn::Item::Fn(function) => {
                        let mut reader = Reader::default();
                        reader.visit_signature(&function.sig);
                        reader.visit_block(&function.block);
                        fns.push(MacroFn {
                            named_by: function.sig.ident.to_string(),
                            name: function.sig.ident.to_string(),
                            at: function.sig.ident.span().byte_range().start,
                            reading: reader.reading,
                        });
                    }
                    syn::Item::Impl(block) => {
                        let named_by = match &*block.self_ty {
                            syn::Type::Path(path) => path
                                .path
                                .segments
                                .last()
                                .map(|segment| segment.ident.to_string()),
                            _ => None,
                        };
                        for member in &block.items {
                            match (member, &named_by) {
                                (syn::ImplItem::Fn(function), Some(named_by)) => {
                                    let mut reader = Reader::default();
                                    reader.visit_signature(&function.sig);
                                    reader.visit_block(&function.block);
                                    fns.push(MacroFn {
                                        named_by: named_by.clone(),
                                        name: function.sig.ident.to_string(),
                                        at: function.sig.ident.span().byte_range().start,
                                        reading: reader.reading,
                                    });
                                }
                                _ => others.push(member.span().byte_range()),
                            }
                        }
                    }
                    syn::Item::Mod(module) => match &module.content {
                        Some((_, inner)) => read_items(inner, fns, others),
                        None => others.push(item.span().byte_range()),
                    },
                    _ => others.push(item.span().byte_range()),
                }
            }
        }
        let file = syn::parse_str::<syn::File>(text)?;
        let mut fns = Vec::new();
        let mut others = Vec::new();
        read_items(&file.items, &mut fns, &mut others);
        Ok((fns, others))
    }

    /// Whether a callable is written without `pub`.
    pub fn private(text: &str, function: bool, owned: bool) -> bool {
        if function {
            syn::parse_str::<syn::ItemFn>(text)
                .is_ok_and(|parsed| matches!(parsed.vis, syn::Visibility::Inherited))
        } else if owned {
            syn::parse_str::<syn::ImplItemFn>(text)
                .is_ok_and(|parsed| matches!(parsed.vis, syn::Visibility::Inherited))
        } else {
            false
        }
    }

    /// The identifier-like words of a text.
    pub fn words(text: &str) -> impl Iterator<Item = &str> {
        text.split(|character: char| !(character.is_alphanumeric() || character == '_'))
            .filter(|word| !word.is_empty())
    }

    #[derive(Default)]
    struct Reader {
        reading: Reading,
    }

    /// The path an expression is, through parentheses and invisible groups.
    fn path_of(expr: &Expr) -> Option<&syn::ExprPath> {
        match expr {
            Expr::Path(path) if path.qself.is_none() => Some(path),
            Expr::Paren(inner) => path_of(&inner.expr),
            Expr::Group(inner) => path_of(&inner.expr),
            _ => None,
        }
    }

    impl Reader {
        fn record(&mut self, path: &syn::ExprPath, context: Context) {
            let Some(last) = path.path.segments.last() else {
                return;
            };
            self.visit_path(&path.path);
            self.reading.uses.push(Use {
                at: last.ident.span().byte_range().start,
                last: last.ident.to_string(),
                path: path
                    .path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect(),
                segments: path.path.segments.len(),
                context,
            });
        }

        /// `expr` in `context` when it is a path; read as any expression otherwise.
        fn place(&mut self, expr: &Expr, context: Context) {
            if let Some(path) = path_of(expr) {
                self.record(path, context);
            } else {
                self.visit_expr(expr);
            }
        }

        /// `expr` in a writing context (`=`, `op=`, `&mut`, a method's receiver): a field, an
        /// index or a parenthesis of a path writes that path, so `X.f = 1`, `X[i] += 1`,
        /// `&mut X.f` and `X.f.push(1)` are read as `X` in `context`. An index's own
        /// expression is read as any expression.
        fn place_written(&mut self, expr: &Expr, context: Context) {
            match expr {
                Expr::Field(field) => self.place_written(&field.base, context),
                Expr::Index(index) => {
                    self.visit_expr(&index.index);
                    self.place_written(&index.expr, context);
                }
                Expr::Paren(inner) => self.place_written(&inner.expr, context),
                Expr::Group(inner) => self.place_written(&inner.expr, context),
                _ => self.place(expr, context),
            }
        }
    }

    /// Every identifier of `tokens`, groups opened, with its offset.
    fn identifiers(tokens: TokenStream, into: &mut Vec<(usize, String)>) {
        for token in tokens {
            match token {
                TokenTree::Ident(ident) => {
                    into.push((ident.span().byte_range().start, ident.to_string()));
                }
                TokenTree::Group(group) => identifiers(group.stream(), into),
                _ => {}
            }
        }
    }

    /// `matches!(value, pattern if guard)`: its value and its guard.
    fn matches_body(input: syn::parse::ParseStream<'_>) -> syn::Result<Vec<Expr>> {
        let value: Expr = input.parse()?;
        input.parse::<Token![,]>()?;
        let _ = syn::Pat::parse_multi_with_leading_vert(input)?;
        let mut found = vec![value];
        if input.peek(Token![if]) {
            input.parse::<Token![if]>()?;
            found.push(input.parse()?);
        }
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }
        Ok(found)
    }

    /// `[value; count]`-shaped arguments: `vec![x; n]`.
    fn repeat_body(input: syn::parse::ParseStream<'_>) -> syn::Result<Vec<Expr>> {
        let value: Expr = input.parse()?;
        input.parse::<Token![;]>()?;
        let count: Expr = input.parse()?;
        Ok(vec![value, count])
    }

    impl<'ast> Visit<'ast> for Reader {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            match expr {
                Expr::MethodCall(call) => {
                    self.reading.calls.push((
                        call.method.span().byte_range().start,
                        call.method.to_string(),
                    ));
                    if let Some(turbofish) = &call.turbofish {
                        self.visit_angle_bracketed_generic_arguments(turbofish);
                    }
                    self.place_written(&call.receiver, Context::Method(call.method.to_string()));
                    for argument in &call.args {
                        self.visit_expr(argument);
                    }
                }
                Expr::Call(call) => {
                    if let Some(path) = path_of(&call.func)
                        && let Some(last) = path.path.segments.last()
                    {
                        self.reading
                            .calls
                            .push((last.ident.span().byte_range().start, last.ident.to_string()));
                    }
                    visit::visit_expr_call(self, call);
                }
                Expr::Assign(assign) => {
                    self.place_written(&assign.left, Context::Assign);
                    self.visit_expr(&assign.right);
                }
                Expr::Binary(binary)
                    if matches!(
                        binary.op,
                        syn::BinOp::AddAssign(_)
                            | syn::BinOp::SubAssign(_)
                            | syn::BinOp::MulAssign(_)
                            | syn::BinOp::DivAssign(_)
                            | syn::BinOp::RemAssign(_)
                            | syn::BinOp::BitXorAssign(_)
                            | syn::BinOp::BitAndAssign(_)
                            | syn::BinOp::BitOrAssign(_)
                            | syn::BinOp::ShlAssign(_)
                            | syn::BinOp::ShrAssign(_)
                    ) =>
                {
                    self.place_written(&binary.left, Context::Assign);
                    self.visit_expr(&binary.right);
                }
                Expr::Reference(reference) => {
                    if reference.mutability.is_some() {
                        self.place_written(&reference.expr, Context::MutRef);
                    } else {
                        self.place(&reference.expr, Context::Ref);
                    }
                }
                Expr::RawAddr(raw) => {
                    if matches!(raw.mutability, syn::PointerMutability::Mut(_)) {
                        self.place_written(&raw.expr, Context::MutRef);
                    } else {
                        self.place(&raw.expr, Context::Ref);
                    }
                }
                Expr::Field(field) => self.place(&field.base, Context::Field),
                Expr::Index(index) => {
                    self.place(&index.expr, Context::Index);
                    self.visit_expr(&index.index);
                }
                Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
                    self.place(&unary.expr, Context::Deref);
                }
                Expr::Path(path) if path.qself.is_none() => self.record(path, Context::Bare),
                _ => visit::visit_expr(self, expr),
            }
        }

        fn visit_path(&mut self, path: &'ast syn::Path) {
            let segments: Vec<String> = path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            for end in 1..=segments.len() {
                self.reading.named.insert(segments[..end].to_vec());
            }
            visit::visit_path(self, path);
        }

        fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
            self.reading.local_names.insert(item.ident.to_string());
            visit::visit_item_const(self, item);
        }

        fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
            self.reading.local_names.insert(item.ident.to_string());
            self.reading.statics.push(Declared {
                at: item.ident.span().byte_range().start,
                name: item.ident.to_string(),
                ty: (*item.ty).clone(),
                mutable: matches!(item.mutability, syn::StaticMutability::Mut(_)),
            });
            visit::visit_item_static(self, item);
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            let name = mac
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .unwrap_or_default();
            if matches!(name.as_str(), "addr_of_mut" | "addr_of")
                && let Ok(expr) = mac.parse_body::<Expr>()
            {
                if name == "addr_of_mut" {
                    self.place_written(&expr, Context::MutRef);
                } else {
                    self.place(&expr, Context::Ref);
                }
                return;
            }
            let parsed = mac
                .parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated)
                .map(|list| list.into_iter().collect::<Vec<_>>())
                .or_else(|_| mac.parse_body_with(repeat_body))
                .or_else(|_| mac.parse_body_with(matches_body));
            match parsed {
                Ok(arguments) => {
                    for argument in &arguments {
                        self.visit_expr(argument);
                    }
                }
                Err(_) => {
                    let mut idents = Vec::new();
                    identifiers(mac.tokens.clone(), &mut idents);
                    self.reading
                        .named
                        .extend(idents.iter().map(|(_, ident)| vec![ident.clone()]));
                    if !idents.is_empty() {
                        self.reading.unparsed.push(Unparsed {
                            name: name.clone(),
                            idents,
                        });
                    }
                }
            }
        }
    }
}
