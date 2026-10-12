//! **No source reader names a file outside the three lists** — the plan's §6.1
//! tripwire, `no_source_reader_names_a_file_outside_the_two_lists` — **and no
//! reader of a committed document is on neither list**
//! (T-SOURCE-READERS-TSV-TRIPWIRE,
//! `every_reader_of_a_committed_document_is_on_a_list`; see *The document
//! readers*).
//!
//! Three lists classify every reader bound to a file.
//! `docs/plans/MIGRATION-DEBT.tsv` is the temporary one: every reader blocked on
//! a missing identity seam, one row each, shrinking and never growing —
//! `scripts/ci/check-migration-debt.ps1` compares it with the merge base and
//! refuses an added row. `docs/plans/SOURCE-READERS.tsv` records tests whose
//! subject intentionally is non-source bytes. [`bt_source::FileScoped`] is the
//! permanent source-reader list: the few readers whose concern really *is* a
//! source file, a variant each, with the reason in the variant's own doc
//! comment. This test is what makes the classification mean something: a
//! reader on none of them is a red test naming the hit and all three lists.
//!
//! # Written without the query layer, on purpose
//!
//! The Rust-source scan below is plain text matching. It does not call
//! `enumerate`, it does not build a [`bt_source::Universe`], it does not parse.
//! That is §6.1's instruction and the reason is simple: this is the guard
//! *against* the mechanism, and a bug in the mechanism must not be able to
//! switch it off. It is also why the crate's own allowlist type is here on the
//! allowlist — see [`bt_source::FileScoped::TheTripwireItself`]. (The document
//! half below is not a guard against the mechanism, and reads through it.)
//!
//! # Why it lives here
//!
//! The two cross-crate guards in this tree today, `bt_platform`'s
//! `native_window_door_tests` and `quiet_door_tests`, are inline
//! `#[cfg(test)] mod`s in `crates/bt-platform/src/lib.rs`, and they are right to
//! be: each guards a *door declared in that file* — `NativeWindow::stand_in`,
//! `bt_platform::quiet` — and walks the workspace only to ask who else names it.
//! This guard has no such home. Its subject is the workspace's source-reading
//! itself, and both of the lists it enforces are `bt-source`'s: the allowlist is
//! a type in this crate, and the debt list is the plan's, which this crate is
//! the mechanism for. An integration test and not a `src/` module, because an
//! integration target links this crate from outside — the same relationship
//! every reader it guards will have with it.
//!
//! # What it catches, and what it does not
//!
//! Four lexical shapes, in `.rs` and `.ps1` files, everywhere under the
//! workspace root except `target/`, `.git/`, `vendor/` and `node_modules/`:
//!
//! 1. an `include_str!` or `include_bytes!` whose argument names a `.rs` file;
//! 2. a path-shaped `.rs` literal handed to a path or read call, in a function
//!    that reads a file (a script is taken whole, having no function to speak
//!    of);
//! 3. a `Scope::File` construction;
//! 4. a directory read rooted at the manifest directory that picks files out by
//!    the `rs` extension.
//!
//! **It matches by file, not by reader**, because that is the granularity both
//! lists have: the allowlist is the *file-scoped* one, and a debt row's owning
//! item is a test name no lexical scan can attribute to a byte with a straight
//! face. So a second reader planted in a file that already has a row does not
//! fire, and the debt row is what tracks it. It is a tripwire and not a
//! completeness proof — the list's own header says the same thing about itself.
//!
//! Three limits, written down rather than discovered later: comments are not
//! stripped, so prose naming a file counts (deliberate — an over-approximation
//! is the safe direction for a guard); a reader split across two functions, one
//! building the path and another reading it, is seen only if both are in the
//! same file and the reading one names the path; and the scan reads no Python,
//! so `scripts/dev/*.py` are on the debt list and out of its reach.
//!
//! # The document readers
//!
//! The four shapes are about Rust source. A test that reads a committed
//! *document* — a `.tsv` ledger, a `.md` page, a fixture, a packaging manifest
//! — is the other half of what the two lists classify, and
//! `every_reader_of_a_committed_document_is_on_a_list` holds it: **every item
//! of test or build-support code that reads a committed non-Rust file is a row
//! of `SOURCE-READERS.tsv` or of `MIGRATION-DEBT.tsv`**, keyed by its file and
//! its item (the `owner` column), and one on neither is refused naming the
//! item, the file and both lists. A reader on both is refused by
//! `every_source_reader_names_a_reason_and_is_not_also_debt`.
//!
//! * **The subject set** is every file `git ls-files` names that does not end
//!   in `.rs`: `docs/`, `scripts/`, `packaging/`, `assets/`, `tests/`,
//!   `licenses/`, `vendor/`, the root's files and every crate's non-Rust files
//!   (`src/**/*.tsv`, `tests/fixtures/`, `tests/fonts/`, the manifests).
//!   Generated files under `target/` are not committed, and so not subjects.
//! * **The readers** are the items of every first-party package's `src/`,
//!   `tests/`, `examples/`, `benches/` and build script (`bt-source`'s
//!   `tests/fixtures/` aside) whose bytes no build of the shipped program
//!   contains ([`bt_source::Occurrence::in_the_product`]) — a bundled asset is
//!   the product's, not a reader's — and that read in one of three ways:
//!   1. an `include_str!` or `include_bytes!` whose literal, taken from the
//!      including file's directory, is a committed document;
//!   2. a **call** of `read_to_string`, `fs::read` or `File::open` — each
//!      found through bt-source's call pattern, the last two only where the
//!      called name ends the path `fs::read` or `File::open`, so a bare
//!      mention (`let read = std::fs::read;`) is no read — judged **one call
//!      at a time by the path that call is handed**. The argument is reduced
//!      as far as its own spelling goes:
//!      - a string literal; a `Path::new`/`PathBuf::from`/`.join(…)`/
//!        `.parent()`/`.with_file_name(…)`/`.with_extension(…)` chain;
//!        `env!("CARGO_MANIFEST_DIR")`; `&`, `*`, `?`, and a block's, an
//!        `if`'s or a `match`'s value (each branch, seen from inside it);
//!      - `concat!` and `format!`, spliced piece by piece. Placeholders take
//!        their arguments as `std` does: an implicit one (`{}`, `{:?}`) the
//!        next positional argument, `{0}` and `{name}` (a named argument or a
//!        captured name) the one they name, a `.*` precision one positional
//!        argument more. A placeholder with a spec puts no path text in: it
//!        is words, or a string naming the path it renders. Text after a
//!        leading path continues the path; a path after other text makes a
//!        string that names that path (`format!(". '{}'", script.display())`),
//!        judged by it; a tuple, array or struct literal holding paths names
//!        them the same way;
//!      - the innermost binding of a name in scope at the call: a `let` from
//!        the end of its statement — after its initialiser, so `let path =
//!        read(path)` reads the earlier `path` — to the end of its block; an
//!        `if let`'s or `while let`'s body; a `match` arm's body; a `for`
//!        loop's body. The one whose scope starts last wins, so an inner
//!        loop's variable is gone once the inner loop ends. `Some(x)` and
//!        `Ok(x)` bind `x` to what they unwrap; a tuple pattern picks its
//!        field. A loop over a literal list (an array, `&[…]`, `vec![…]`,
//!        `.iter()` over one, a name or constant bound to one) is each element
//!        in turn, every one judged;
//!      - a constant, as Rust resolves the path: a bare name, the
//!        constant of that name the module it is written in declares, then
//!        what the module brings in by `use` and `use …::*`; `self::`, `super::`, `crate::`, a child module or a
//!        first-party crate's name, through bt-source's module trees — in any
//!        file of any first-party package, every file a module is written in
//!        under its `cfg`s — to the constant there, its `use`s and globs, or a
//!        `thread_local!` (never a path); `Self::NAME`, `Type::NAME` and
//!        `<Type as Trait>::NAME`, the constants `impl` blocks of that type
//!        declare, or a first-party trait's default for one the block does
//!        not declare — the named trait's impl only; unqualified, the inherent
//!        impl, else the one trait impl, and several are unreduced. A trait
//!        path is followed through the module trees, `use`s (an alias to what
//!        it names) and globs to a first-party trait; another crate's trait is
//!        its full path as written after first-party `use`s are followed
//!        (`use dep::Trait as A` makes `A` `dep::Trait`; `a::Same` is not
//!        `b::Same`); one that cannot be followed (a glob of another crate's
//!        module may bring it in) is unreduced.
//!
//!        The tree compiles, so every path the reading code names is one
//!        Rust let it name: a `pub use` of a `pub(crate)` item is E0364, a
//!        glob re-exports only what it may, and a type or trait the code
//!        names is visible to it. Visibility therefore does one thing here:
//!        where two ways of bringing a name in (two globs) lead to two
//!        constants, the one visible to the module the path is written in —
//!        carried through every re-export; visible when every `use` on its
//!        way admits that module and so does the constant — is the constant; several visible,
//!        or none, is unreduced. A lone candidate is the constant even when it
//!        is invisible to the reader: code that named it otherwise could not
//!        have compiled. That is the rule, not a hole. Another crate's item or a primitive's (`u32::MAX`, a
//!        `windows` constant, a glob of another crate's module) is never a
//!        path into this checkout. A name found nowhere that is spelled
//!        SCREAMING_SNAKE_CASE is unreduced; any other is a function, a type
//!        or a variant — CI's workspace-wide `clippy -D warnings` makes
//!        `non_upper_case_globals` refuse a constant spelled otherwise, and a
//!        snake_case constant that is there is found by its name all the same;
//!      - a call of a free function of the same file: what it returns (its
//!        tail, or a `return` that ends it), with its parameters bound to what
//!        the call hands it; several of one name under exclusive `cfg`s are
//!        each followed, and one that calls itself adds nothing of its own to
//!        what its other branches return.
//!
//!      Constants are followed as far as their chains go; one that reaches
//!      itself again on the same resolution path is a cycle. Its base is the
//!      package directory (`CARGO_MANIFEST_DIR`, or a relative path, which
//!      `cargo test` resolves there), a scratch root (`temp_dir`,
//!      `temp_path`, `tempfile`), an absolute path (outside the checkout: a
//!      committed absolute path into it would be a machine path, which
//!      `scripts/check-machine-paths.ps1` refuses), or a base this reading
//!      cannot see (a parameter, a field, another function's result: the
//!      function or caller that hands that base on is the call judged for
//!      it). A path under the package directory that lands on a committed
//!      document is a read of it; one that lands under `target/`, on Rust
//!      source, on a directory or on an uncommitted file is not; a scratch
//!      path is not. A path on an unseen base reads a committed document only
//!      when its literal segments are a whole path to one from the workspace
//!      root (two segments or more, which a scratch file's bare name is not).
//!      So a function that reads its own output under `target/` and a
//!      committed `.tsv` is a reader of the `.tsv` alone.
//!
//!      **A path this reading gives up on is never dropped in silence.** It
//!      is a diagnostic — grouped by cause, its exact total and a few
//!      examples (file, line, item) per cause printed by the test and in its
//!      refusal — and not refused, when:
//!      - a rooted path keeps a piece it cannot reduce (`root.join(name)`
//!        with `name` a parameter, another crate's constant it cannot read);
//!      - a `concat!` or `format!` holds a rooted piece and, anywhere, a
//!        piece it cannot reduce (`format!("{}{}/x", unknown, root)`) —
//!        and each rooted piece it does reduce to a committed document is a
//!        reader all the same (both outcomes, never one);
//!      - a string naming a rooted path is joined onto, or joined onto a
//!        path; a list or a tuple it cannot pick a part of holds a rooted
//!        path; a macro it does not expand is handed one;
//!      - a constant reaches itself again;
//!      - a SCREAMING_SNAKE_CASE name leads to no constant, `use`, glob or
//!        `thread_local!` of the module tree (a constant a macro declares).
//!   3. **any other call or method handed a committed document**: every call
//!      of a path (`read_session(…)`, `Engine::open(…)`, `std::fs::copy(…)`)
//!      and every method call is judged by each of its arguments and by the
//!      value it is called on, reduced the same way, and is a reader of the
//!      committed documents they land on, named by its callee (the `mechanism`
//!      column). A test that names a committed file and hands it to code
//!      depends on that file, whatever the callee does with it. Two things are
//!      not such a call: a macro (`assert_eq!(path, …)` compares; its own
//!      arguments are walked, and a call inside them counts), and the calls
//!      that build or describe a path rather than hand it on — `Path::new`,
//!      `PathBuf::from`, `String::from`, and the methods in [`PATH_BUILDING`]
//!      (`join`, `parent`, `display`, `to_str`, `file_name`…). Here a relative
//!      string is a string, not a path into the checkout: only the three reads
//!      resolve one from the package directory. An unreduced rooted path, or a
//!      cycle, handed to any of these calls joins the same diagnostic.
//! * **The item** is the innermost function holding the call; outside every
//!   function it is the `const` or `static` the include initialises, or the
//!   module itself. The name is the module path without `crate`, with an
//!   integration test's, example's, bench's or build script's target name in
//!   front (`census::committed_census`).
//!
//! This half reads Rust through `bt-source`'s index — the call sites, the
//! items, and the text of each file it parses for a call's argument — because
//! its key is an item and only the index knows which module a byte is in.
//! What it does not see, written down rather than discovered: a path handed in
//! as a parameter (`bt-app`'s `build.rs` `release_manifest`, and `scan` above,
//! which reads `.ps1` files); a root reached through a method, a closure or
//! another file's function; a path a function hands on from its own
//! parameter (`bt_persist::read_settings` is the reader of what it is
//! handed; its caller, which names the fixture, is the row); a read call
//! renamed by `use … as …`, since bt-source does not resolve aliases; a call
//! inside a macro whose arguments do not parse as expressions; a method that
//! reads under a name [`PATH_BUILDING`] excludes, since that list is of names
//! (and a new path-describing method is a reader until it is added there); a
//! loop over anything but a literal list; and the paths it lists as
//! unreduced. Rows are not checked the other way: a row whose item no longer
//! reads is not refused.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use bt_source::{
    Certainty, DiskScope, FileRecord, FileScoped, Index, ItemRecord, LiteralValue, MacroKind,
    Occurrence, Package, Pattern, Scope, Search, Span, TargetId, TargetKind, TargetRoot, TokenKind,
    Universe, Vendor, View, Workspace, is_vendored, needle, normalized, report, universes,
};
use syn::spanned::Spanned as _;

/// The temporary list, from the workspace root.
const DEBT_LIST: &str = "docs/plans/MIGRATION-DEBT.tsv";

/// Intentional readers of fixtures, documents, assets, and parser corpora.
const SOURCE_LIST: &str = "docs/plans/SOURCE-READERS.tsv";

/// The columns the list is written in, in order.
const DEBT_COLUMNS: [&str; 8] = [
    "file",
    "owner",
    "mechanism",
    "subject",
    "ticket",
    "disturbed_by_2a",
    "needed_seam",
    "verdict",
];

/// The columns of the intentional-reader list, in order.
const SOURCE_COLUMNS: [&str; 7] = [
    "file",
    "owner",
    "mechanism",
    "subject",
    "ticket",
    "disturbed_by_2a",
    "reason",
];

/// Directory names the scan never descends into.
///
/// `target/` is not source; `vendor/` is upstream's code held to upstream's
/// choices, and a vendored crate reading its own source is not this project's
/// migration debt.
const NOT_SCANNED: [&str; 4] = ["target", ".git", "vendor", "node_modules"];

/// A macro that takes a file name and hands back its bytes.
const INCLUDING: [&str; 2] = ["include_str!(", "include_bytes!("];

/// Call heads that turn a string into a path or into the bytes behind one.
const PATH_HEADS: [&str; 9] = [
    "join",
    "push",
    "new",
    "from",
    "open",
    "read",
    "read_to_string",
    "canonicalize",
    "with_file_name",
];

/// Reading a file, in each of the two languages the gates of this workspace are
/// written in.
///
/// `fs::read(` carries its parenthesis because `fs::read_dir` contains
/// `fs::read`, and a function that lists a directory is not a function that
/// reads a file — without it, `bt_source::manifest::roots_under`'s `main.rs`,
/// which is cargo's target-discovery rule and names no reader's subject, reads
/// as a source pin.
const RUST_READS: [&str; 3] = ["read_to_string", "fs::read(", "File::open"];
const SHELL_READS: [&str; 5] = [
    "Get-Content",
    "Select-String",
    "ReadAllText",
    "ReadAllLines",
    "ReadAllBytes",
];

/// The three tokens that together are a walk of a crate's own sources.
const DIRECTORY_READ: &str = "read_dir";
const MANIFEST_DIRECTORY: &str = "CARGO_MANIFEST_DIR";
const RUST_EXTENSION: &str = "\"rs\"";

/// A scope that names a file, built.
const FILE_SCOPE: &str = "Scope::File(";

/// What the scan found a reader doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    IncludedSource,
    ReadOfANamedSource,
    FileScope,
    ManifestDirectoryWalk,
}

impl Kind {
    fn said(self) -> &'static str {
        match self {
            Self::IncludedSource => "includes the text of a named source file",
            Self::ReadOfANamedSource => "reads a named source file at run time",
            Self::FileScope => "builds a scope that names a file",
            Self::ManifestDirectoryWalk => "walks its own manifest directory for `.rs` files",
        }
    }
}

/// One reader, where it is and what it names.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Hit {
    /// From the workspace root, forward slashes — the key both lists use.
    file: String,
    line: usize,
    kind: Kind,
    subject: String,
}

impl Hit {
    fn named(&self) -> String {
        format!(
            "{}:{} {} — {}",
            self.file,
            self.line,
            self.kind.said(),
            self.subject
        )
    }
}

// ──────────────────────────────────────────────────────────────── the scan ──

/// Every hit under `root`, in path order.
fn scan(root: &Path) -> Vec<Hit> {
    let mut files = Vec::new();
    gather(root, &mut files);
    files.sort();
    let mut hits = Vec::new();
    for file in &files {
        let rust = file.extension().is_some_and(|kind| kind == "rs");
        let text = std::fs::read_to_string(file).unwrap_or_else(|error| {
            panic!(
                "{} is in the tree and this scan could not read it: {error}. A file the guard \
                 skips is a file the guard does not cover.",
                file.display()
            )
        });
        hits.extend(hits_in(&from_root(file, root), &text, rust));
    }
    hits
}

fn gather(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            let skipped = entry
                .file_name()
                .to_str()
                .is_some_and(|name| NOT_SCANNED.contains(&name));
            if !skipped {
                gather(&path, found);
            }
        } else if path
            .extension()
            .is_some_and(|kind| kind == "rs" || kind == "ps1")
        {
            found.push(path);
        }
    }
}

fn from_root(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The four shapes, in one file's text.
fn hits_in(file: &str, text: &str, rust: bool) -> Vec<Hit> {
    let mut found = Vec::new();
    let items = if rust { function_starts(text) } else { vec![0] };

    for (open, close) in named_source_literals(text) {
        let named = &text[open + 1..close];
        let before = &text[..open];
        let kind = if ends_with_any(strip_raw_marker(before.trim_end()), &INCLUDING) {
            Kind::IncludedSource
        } else if reads_a_file(&text[body_of(&items, text.len(), open)], rust)
            && (!rust || ends_with_a_path_head(before))
        {
            Kind::ReadOfANamedSource
        } else {
            continue;
        };
        found.push(Hit {
            file: file.to_owned(),
            line: line_of(text, open),
            kind,
            subject: named.to_owned(),
        });
    }

    if rust && let Some(at) = text.find(FILE_SCOPE) {
        found.push(Hit {
            file: file.to_owned(),
            line: line_of(text, at),
            kind: Kind::FileScope,
            subject: FILE_SCOPE.to_owned(),
        });
    }

    if rust
        && let Some(at) = text.find(DIRECTORY_READ)
        && text.contains(MANIFEST_DIRECTORY)
        && text.contains(RUST_EXTENSION)
    {
        found.push(Hit {
            file: file.to_owned(),
            line: line_of(text, at),
            kind: Kind::ManifestDirectoryWalk,
            subject: format!("{DIRECTORY_READ} under {MANIFEST_DIRECTORY}"),
        });
    }

    found
}

/// Every string literal in `text` whose value is a path ending in `.rs`, as the
/// byte positions of its two quotes.
///
/// The search starts from the *closing* quote and walks back to the nearest one
/// on the same line rather than pairing quotes from the top of the file. A
/// forward pairing desynchronises on the first `'"'` character literal and then
/// reads the rest of the file inside out; this cannot, because every candidate
/// is decided within one line.
fn named_source_literals(text: &str) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    for (at, _) in text.match_indices(".rs\"") {
        let close = at + 3;
        let line_start = text[..at].rfind('\n').map_or(0, |end| end + 1);
        let Some(open) = text[line_start..at].rfind('"').map(|at| line_start + at) else {
            continue;
        };
        if path_shaped(&text[open + 1..close]) {
            found.push((open, close));
        }
    }
    found
}

/// A name a file could have: no spaces, no quotes, and something before the dot.
fn path_shaped(value: &str) -> bool {
    value.len() > ".rs".len()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '/' | '\\' | '-')
        })
}

/// `text` with a raw-string marker (`r`, `r#`, `r##`…) taken off the end.
fn strip_raw_marker(text: &str) -> &str {
    let text = text.trim_end_matches('#');
    text.strip_suffix('r').unwrap_or(text)
}

fn ends_with_any(text: &str, endings: &[&str]) -> bool {
    endings.iter().any(|ending| text.ends_with(ending))
}

/// Whether the text just before a literal is `…some_call(` with a path or read
/// call at its head.
fn ends_with_a_path_head(before: &str) -> bool {
    let before = strip_raw_marker(before.trim_end());
    let before = before.trim_end().strip_suffix('&').unwrap_or(before);
    let Some(before) = before.trim_end().strip_suffix('(') else {
        return false;
    };
    let before = before.trim_end();
    let head_at = before
        .rfind(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .map_or(0, |at| at + 1);
    PATH_HEADS.contains(&&before[head_at..])
}

fn reads_a_file(body: &str, rust: bool) -> bool {
    let verbs: &[&str] = if rust { &RUST_READS } else { &SHELL_READS };
    verbs.iter().any(|verb| body.contains(verb))
}

/// The span of the function `at` is written in, as a range over `text`.
fn body_of(starts: &[usize], length: usize, at: usize) -> std::ops::Range<usize> {
    let start = starts
        .iter()
        .rev()
        .find(|&&start| start <= at)
        .copied()
        .unwrap_or(0);
    let end = starts
        .iter()
        .find(|&&start| start > at)
        .copied()
        .unwrap_or(length);
    start..end
}

/// The byte position of every line that declares a function.
fn function_starts(text: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut at = 0usize;
    for line in text.split_inclusive('\n') {
        if declares_a_function(line) {
            starts.push(at);
        }
        at += line.len();
    }
    starts
}

fn declares_a_function(line: &str) -> bool {
    let mut rest = line.trim_start();
    // `pub(in crate::x) const unsafe extern "C" fn` is six qualifiers and the
    // longest this language writes; the bound is what keeps this a loop.
    for _ in 0..6 {
        if rest.starts_with("fn ") || rest.starts_with("fn\t") {
            return true;
        }
        match strip_one_qualifier(rest) {
            Some(next) => rest = next.trim_start(),
            None => return false,
        }
    }
    false
}

fn strip_one_qualifier(rest: &str) -> Option<&str> {
    for word in ["pub", "const", "async", "unsafe", "default"] {
        let Some(tail) = rest.strip_prefix(word) else {
            continue;
        };
        if tail.starts_with(char::is_whitespace) {
            return Some(tail);
        }
        if word == "pub" && tail.starts_with('(') {
            return tail.find(')').map(|close| &tail[close + 1..]);
        }
    }
    let tail = rest.strip_prefix("extern")?;
    let tail = tail.trim_start();
    match tail.strip_prefix('"') {
        Some(name) => name.find('"').map(|close| &name[close + 1..]),
        None => Some(tail),
    }
}

fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

// ─────────────────────────────────────────────────────────────── the lists ──

/// Two directories above this crate.
///
/// Deliberately **not** `canonicalize`, for `bt_source::normalized`'s reason one
/// crate over: on Windows it returns a `\\?\` verbatim path, and a verbatim path
/// does not accept the forward slashes both lists are keyed by, so a joined
/// script path would stop naming a file that is plainly there. The `..`
/// components are left in; every path the scan
/// reports is built by walking down from this one, so they cancel out of every
/// comparison.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Every row of the debt list, as the fields the header names.
fn debt_rows(root: &Path) -> Vec<BTreeMap<&'static str, String>> {
    let path = root.join(DEBT_LIST);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{DEBT_LIST} is in the tree: {error}"));
    let mut rows = Vec::new();
    let mut seen_header = false;
    for (number, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(
            fields.len(),
            DEBT_COLUMNS.len(),
            "{DEBT_LIST}:{} has {} fields and the list has {}. The CI check parses this file by \
             splitting on tabs, so a row it cannot split is a row it cannot compare.",
            number + 1,
            fields.len(),
            DEBT_COLUMNS.len()
        );
        if !seen_header {
            assert_eq!(
                fields, DEBT_COLUMNS,
                "{DEBT_LIST} does not begin with the columns it is read by"
            );
            seen_header = true;
            continue;
        }
        rows.push(
            DEBT_COLUMNS
                .iter()
                .zip(fields)
                .map(|(column, value)| (*column, value.to_owned()))
                .collect(),
        );
    }
    assert!(seen_header, "{DEBT_LIST} has no header row");
    rows
}

fn debt_files(root: &Path) -> BTreeSet<String> {
    debt_rows(root)
        .into_iter()
        .map(|row| row["file"].clone())
        .collect()
}

/// Every intentional non-source reader, as the fields its header names.
fn source_rows(root: &Path) -> Vec<BTreeMap<&'static str, String>> {
    let path = root.join(SOURCE_LIST);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{SOURCE_LIST} is in the tree: {error}"));
    let mut rows = Vec::new();
    let mut seen_header = false;
    for (number, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(
            fields.len(),
            SOURCE_COLUMNS.len(),
            "{SOURCE_LIST}:{} has {} fields and the list has {}",
            number + 1,
            fields.len(),
            SOURCE_COLUMNS.len()
        );
        if !seen_header {
            assert_eq!(
                fields, SOURCE_COLUMNS,
                "{SOURCE_LIST} does not begin with the columns it is read by"
            );
            seen_header = true;
            continue;
        }
        rows.push(
            SOURCE_COLUMNS
                .iter()
                .zip(fields)
                .map(|(column, value)| (*column, value.to_owned()))
                .collect(),
        );
    }
    assert!(seen_header, "{SOURCE_LIST} has no header row");
    rows
}

fn source_files(root: &Path) -> BTreeSet<String> {
    source_rows(root)
        .into_iter()
        .map(|row| row["file"].clone())
        .collect()
}

fn allowlist() -> BTreeMap<&'static str, FileScoped> {
    FileScoped::ALL
        .iter()
        .map(|entry| (entry.path(), *entry))
        .collect()
}

/// The hits that are on neither list.
fn unlisted<'a>(
    hits: &'a [Hit],
    debt: &BTreeSet<String>,
    sources: &BTreeSet<String>,
    allowed: &BTreeMap<&str, FileScoped>,
) -> Vec<&'a Hit> {
    hits.iter()
        .filter(|hit| {
            !debt.contains(&hit.file)
                && !sources.contains(&hit.file)
                && !allowed.contains_key(hit.file.as_str())
        })
        .collect()
}

/// What the test says when it fires: the hit, and both lists.
fn complaint(
    strays: &[&Hit],
    debt: &BTreeSet<String>,
    sources: &BTreeSet<String>,
    allowed: &BTreeMap<&str, FileScoped>,
) -> String {
    let mut said = String::new();
    let _ = writeln!(
        said,
        "{} source reader(s) name a file and are on none of the three lists:\n",
        strays.len()
    );
    for stray in strays {
        let _ = writeln!(said, "    {}", stray.named());
    }
    let _ = writeln!(
        said,
        "\nThe three lists are:\n\n  * {DEBT_LIST} — the readers blocked on a missing seam, {} files today. \
         It only shrinks: a row is removed by the ticket that migrates its reader, and \
         scripts/ci/check-migration-debt.ps1 refuses an added one.\n  * {SOURCE_LIST} — \
         intentional readers of fixtures, documents, assets, and corpora, {} files today.\n  * \
         bt_source::FileScoped — \
         the readers whose concern really is a file, {} entries today:",
        debt.len(),
        sources.len(),
        allowed.len()
    );
    for (path, entry) in allowed {
        let _ = writeln!(said, "      - {path}: {}", entry.reason());
    }
    let _ = write!(
        said,
        "\nA new reader belongs on neither. Ask the crate about the item instead \
         (docs/plans/bt-app-split-prep.md §2); and if the subject really is a file, add a \
         FileScoped variant whose doc comment says why."
    );
    said
}

// ─────────────────────────────────────────────────────────────── the tests ──

/// RED — **every reader in this workspace that names a file is on one of the two
/// lists.**
///
/// MUTATION: `a_planted_reader_that_is_on_neither_list_makes_the_tripwire_fire`
/// below is this test's mutation, run against a tree of its own rather than
/// against this one.
#[test]
fn no_source_reader_names_a_file_outside_the_two_lists() {
    let root = workspace_root();
    let hits = scan(&root);
    // The independent traversal floor: a walk that lost its way reads a handful
    // of files, not hundreds, and the list comparison below cannot tell — a
    // lone listed reader is no stray. The floor is on the files the walk read
    // and not on the readers it found, because the readers are what every
    // migration ticket removes: 718 `.rs` and `.ps1` files on 2026-10-09.
    let mut walked = Vec::new();
    gather(&root, &mut walked);
    assert!(
        walked.len() > 400,
        "the scan read {} files, which is not this workspace — it read {} as its root",
        walked.len(),
        root.display()
    );

    let debt = debt_files(&root);
    let sources = source_files(&root);
    let allowed = allowlist();
    let strays = unlisted(&hits, &debt, &sources, &allowed);
    println!(
        "{} hits over {} files; {} debt files, {} intentional-reader files, {} on the allowlist",
        hits.len(),
        hits.iter()
            .map(|hit| &hit.file)
            .collect::<BTreeSet<_>>()
            .len(),
        debt.len(),
        sources.len(),
        allowed.len()
    );
    assert!(
        strays.is_empty(),
        "{}",
        complaint(&strays, &debt, &sources, &allowed)
    );
}

/// RED — **plant a reader of each of the four shapes in a tree of its own and
/// the scan finds every one of them, and the check refuses all four.**
///
/// The plant is in a temporary directory the scan is pointed at, never in this
/// repository: a fixture under `tests/` would be a reader in the real tree, and
/// the test above would then need a row for it — which is the list growing to
/// accommodate its own guard. The four readers are assembled from halves for the
/// same reason `bt_platform::quiet_door_tests` assembles its needles: spelled
/// whole they would be readers in *this* file.
#[test]
fn a_planted_reader_that_is_on_neither_list_makes_the_tripwire_fire() {
    let plot = temporary_directory("planted");
    let module = plot.join("crates").join("planted").join("src");
    std::fs::create_dir_all(&module).expect("a tree of our own");
    let written = module.join("lib.rs");
    std::fs::write(&written, planted_reader()).expect("the plant is written");

    let hits = scan(&plot);
    let kinds: BTreeSet<Kind> = hits.iter().map(|hit| hit.kind).collect();
    assert_eq!(
        kinds,
        BTreeSet::from([
            Kind::IncludedSource,
            Kind::ReadOfANamedSource,
            Kind::FileScope,
            Kind::ManifestDirectoryWalk,
        ]),
        "the scan missed one of the four shapes it is written for; it found {hits:#?}"
    );
    assert!(
        hits.iter()
            .all(|hit| hit.file == "crates/planted/src/lib.rs"),
        "the scan named a file that is not the plant: {hits:#?}"
    );

    // The real lists, against the planted tree: the plant is on neither.
    let root = workspace_root();
    let debt = debt_files(&root);
    let sources = source_files(&root);
    let allowed = allowlist();
    let strays = unlisted(&hits, &debt, &sources, &allowed);
    assert_eq!(
        strays.len(),
        hits.len(),
        "a planted reader was taken for a listed one"
    );

    let said = complaint(&strays, &debt, &sources, &allowed);
    assert!(
        said.contains("crates/planted/src/lib.rs")
            && said.contains(DEBT_LIST)
            && said.contains(SOURCE_LIST)
            && said.contains("bt_source::FileScoped")
            && said.contains(FileScoped::VendoredAddedFiles.path()),
        "the refusal has to name the hit and both lists; it said:\n{said}"
    );

    // And the same plant *on* a list is not a stray — which is the other half of
    // the claim, and the reason a migration ticket can delete a row and see this
    // test start covering the file it emptied.
    let listed = BTreeSet::from(["crates/planted/src/lib.rs".to_owned()]);
    assert!(unlisted(&hits, &listed, &BTreeSet::new(), &BTreeMap::new()).is_empty());

    std::fs::remove_dir_all(&plot).expect("the plant is taken away again");
}

/// PIN — **every allowlist entry names one file, one reason, and a file the scan
/// still reaches.**
///
/// The last of those is what keeps the permanent list shrinking too: an entry
/// whose reader has been migrated or deleted stops being hit, and this says so
/// rather than leaving a variant nobody can account for.
///
/// MUTATION: give two variants the same path, empty a reason, or point one at a
/// file with no reader in it, and this goes red naming the entry.
#[test]
fn each_allowlist_entry_names_one_file_a_reason_and_a_reader() {
    let mut paths: Vec<&str> = FileScoped::ALL.iter().map(|entry| entry.path()).collect();
    paths.sort_unstable();
    let distinct = paths.len();
    paths.dedup();
    assert_eq!(
        paths.len(),
        distinct,
        "two allowlist entries name the same file, so one of them says nothing"
    );

    let root = workspace_root();
    let reached: BTreeSet<String> = scan(&root).into_iter().map(|hit| hit.file).collect();
    for entry in FileScoped::ALL {
        assert!(
            !entry.path().is_empty() && !entry.reason().is_empty(),
            "{entry:?} is on the allowlist without saying where it is or why"
        );
        assert!(
            !entry.path().contains('\\'),
            "{entry:?} is spelled with a backslash; both lists are keyed by forward slashes so \
             that one spelling works on both platforms"
        );
        assert!(
            root.join(entry.path()).is_file(),
            "{entry:?} names {}, which is not in the tree",
            entry.path()
        );
        assert!(
            reached.contains(entry.path()),
            "{entry:?} names {}, and the scan finds no reader in it any more — the allowlist only \
             shrinks, so the entry goes",
            entry.path()
        );
        assert_eq!(Scope::File(entry).named_file(), Some(entry.path()));
        assert_eq!(Scope::File(entry).entry(), Some(entry));
    }
}

/// PIN — **the debt list is machine-readable in the shape the CI check reads
/// it**: eight tab-separated fields a row, a known ticket, a yes-or-no answer
/// about Step 2a, a needed seam, and the DEBT verdict.
///
/// `debt_rows` does the splitting and the arity, so what is left here is the
/// vocabulary. The CI check compares whole rows as text and never parses a
/// field, which is why a malformed row would otherwise go unnoticed until a
/// ticket tried to use the list.
///
/// MUTATION: write a ticket the plan does not have, or a third answer in the
/// last column, and this names the row.
#[test]
fn every_debt_row_names_a_ticket_of_the_plan_and_an_answer_about_the_move() {
    let tickets: BTreeSet<&str> = BTreeSet::from([
        "P0", "P3", "P4", "P5", "P6", "P7", "P8", "P9", "P10", "P12", "P13", "P14", "P15", "P16",
        "P17", "P18", "P19",
    ]);
    // No floor on the row count: the list only shrinks, by design, and a
    // truncated list is caught by `no_source_reader_names_a_file_outside_the_two_lists`
    // (a reader whose row went missing is on neither list) and by
    // `scripts/ci/check-migration-debt.ps1` (which refuses growth). A floor
    // written from the seed census would go red on the batch that migrates
    // the row under it, which is exactly the work this list exists to record.
    let rows = debt_rows(&workspace_root());
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for row in &rows {
        assert!(
            tickets.contains(row["ticket"].as_str()),
            "no ticket of docs/plans/bt-app-split-prep.md is called {:?}: {row:?}",
            row["ticket"]
        );
        assert!(
            row["disturbed_by_2a"] == "yes" || row["disturbed_by_2a"] == "no",
            "whether Step 2a disturbs a row is yes or no: {row:?}"
        );
        assert_eq!(row["verdict"], "DEBT", "the residue list carries only debt");
        for column in DEBT_COLUMNS {
            assert!(
                !row[column].is_empty(),
                "{column} is empty in {row:?}, and a row with a hole in it is a row nobody can act \
                 on"
            );
        }
        *counts.entry(row["ticket"].clone()).or_default() += 1;
    }
    println!("rows by ticket: {counts:?}");
}

/// PIN — the intentional-reader list is complete in itself and disjoint from
/// the debt list by reader identity.
#[test]
fn every_source_reader_names_a_reason_and_is_not_also_debt() {
    let root = workspace_root();
    let debt: BTreeSet<(String, String)> = debt_rows(&root)
        .into_iter()
        .map(|row| (row["file"].clone(), row["owner"].clone()))
        .collect();
    let rows = source_rows(&root);
    let mut seen = BTreeSet::new();
    for row in rows {
        let identity = (row["file"].clone(), row["owner"].clone());
        assert!(
            seen.insert(identity.clone()),
            "SOURCE-READERS repeats {identity:?}"
        );
        assert!(
            !debt.contains(&identity),
            "{identity:?} is both intentional and debt"
        );
        assert!(!row["reason"].is_empty(), "{identity:?} has no KEEP reason");
    }
}

// ──────────────────────────────────────────────────── the document readers ──

/// The macros that read a file's bytes into the program at compile time.
const INCLUDE_MACROS: [&str; 2] = ["include_str", "include_bytes"];

/// What a document reader does with the file.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Reading {
    /// `include_str!` or `include_bytes!` of a committed path.
    Included,
    /// `read_to_string`, `fs::read` or `File::open` on a path the item roots in
    /// the checkout or spells out.
    AtRunTime,
    /// Any other call or method handed a path that resolves to a committed
    /// document — a wrapper of this crate, a function of another, a method
    /// called on the path — named by its callee.
    HandedTo(String),
}

impl Reading {
    fn said(&self) -> String {
        match self {
            Self::Included => "includes a committed document".to_owned(),
            Self::AtRunTime => "reads a committed document at run time".to_owned(),
            Self::HandedTo(callee) => format!("hands a committed document to `{callee}`"),
        }
    }
}

/// One item of test or build-support code that reads a committed document.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DocumentReader {
    /// The Rust file the item is written in, from the workspace root.
    file: String,
    line: usize,
    /// Every name the item answers to — one per way its file is reached — in
    /// the spelling both lists' `owner` column is written in.
    owners: BTreeSet<String>,
    readings: BTreeSet<Reading>,
    /// The committed documents its literals name, from the workspace root.
    /// Empty when the path is computed and no literal names a file.
    subjects: BTreeSet<String>,
}

impl DocumentReader {
    fn named(&self) -> String {
        let subjects = if self.subjects.is_empty() {
            "a path it computes under the checkout".to_owned()
        } else {
            self.subjects.iter().cloned().collect::<Vec<_>>().join(", ")
        };
        format!(
            "{}:{} {} {} — {}",
            self.file,
            self.line,
            self.owners.iter().cloned().collect::<Vec<_>>().join(" / "),
            self.readings
                .iter()
                .map(Reading::said)
                .collect::<Vec<_>>()
                .join(" and "),
            subjects
        )
    }
}

/// Every file the repository commits, from the workspace root, forward slashes.
///
/// `git ls-files` and not a directory walk, because the subject is what is
/// *committed*: a walk would also find `target/`'s generated files and a
/// developer's untracked ones, and neither is a document a test may depend on.
fn committed_files(root: &Path) -> BTreeSet<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .unwrap_or_else(|error| {
            panic!("git ls-files names what the repository commits, and it did not start: {error}")
        });
    assert!(
        output.status.success(),
        "git ls-files failed in {}: {}",
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("the repository's paths are UTF-8")
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Whether a committed path is a document: any committed file that is not
/// Rust source. Rust source keeps the four shapes above.
fn is_document(path: &str) -> bool {
    !path.ends_with(".rs")
}

/// The whole of a first-party package; `bt-source`'s `tests/fixtures/` are
/// source *about* compilation and are declared out, as `timing.rs` does.
fn package_universe(package: &Package) -> Universe {
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

/// Every first-party package's directory and index.
fn first_party_indices(root: &Path) -> Vec<(PathBuf, Index)> {
    let workspace = Workspace::read(root).expect("this workspace");
    workspace
        .packages()
        .iter()
        .filter(|package| !is_vendored(package.directory()))
        .map(|package| {
            let index = Index::build(&package_universe(package))
                .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
            (package.directory().to_path_buf(), index)
        })
        .collect()
}

/// One index, read for its document readers.
struct DocumentScan<'a> {
    index: &'a Index,
    /// The index's position among the scan's indices.
    number: usize,
    world: &'a World<'a>,
    root: &'a Path,
    /// The package's own directory, where `cargo test` runs its tests.
    package: &'a Path,
    committed: &'a BTreeSet<String>,
}

impl DocumentScan<'_> {
    /// The decoded value of every string literal inside `[start, end)`.
    fn strings_in(&self, start: usize, end: usize) -> Vec<&str> {
        let literals = self.index.literals();
        let first = literals.partition_point(|literal| literal.span().start() < start);
        literals[first..]
            .iter()
            .take_while(|literal| literal.span().end() <= end)
            .filter_map(|literal| match literal.value() {
                LiteralValue::Str(value) => Some(value.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The identifier tokens inside `[start, end)`, with where each starts.
    fn names_in(&self, start: usize, end: usize) -> Vec<(usize, &str)> {
        let tokens = self.index.tokens();
        let first = tokens.partition_point(|token| token.span().start() < start);
        tokens[first..]
            .iter()
            .take_while(|token| token.span().end() <= end)
            .filter(|token| token.kind() == TokenKind::Identifier)
            .map(|token| (token.span().start(), self.index.text(token.name_span())))
            .collect()
    }

    /// Where a statement beginning at `start` ends: after its `;` at depth
    /// zero, or at the `}` that closes the block it is written in, literals and
    /// comments stepped over.
    fn statement_end(&self, start: usize, limit: usize) -> usize {
        let union = self.index.union().as_bytes();
        let literals = self.index.literals();
        let comments = self.index.comments();
        let mut depth = 0usize;
        let mut position = start;
        while position < limit {
            if let Some(literal) = literals
                .get(literals.partition_point(|literal| literal.span().end() <= position))
                .filter(|literal| literal.span().holds(position))
            {
                position = literal.span().end();
                continue;
            }
            if let Some(comment) = comments
                .get(comments.partition_point(|comment| comment.span().end() <= position))
                .filter(|comment| comment.span().holds(position))
            {
                position = comment.span().end();
                continue;
            }
            match union[position] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' if depth > 0 => depth -= 1,
                b'}' => return position,
                b';' if depth == 0 => return position + 1,
                _ => {}
            }
            position += 1;
        }
        limit
    }

    /// Every `const` and `static` a file declares: its name and the bytes of
    /// its statement, from the keyword to the `;`.
    fn value_statements(&self, file: &FileRecord) -> Vec<(&str, usize, usize)> {
        let span = file.span();
        let names = self.names_in(span.start(), span.end());
        let mut found = Vec::new();
        for (at, (start, keyword)) in names.iter().enumerate() {
            if *keyword != "const" && *keyword != "static" {
                continue;
            }
            // `const fn`, `const unsafe fn` and `const extern` declare a
            // function, not a value.
            let Some((_, name)) = names[at + 1..].iter().find(|(_, name)| *name != "mut") else {
                continue;
            };
            if matches!(*name, "fn" | "unsafe" | "extern" | "async") {
                continue;
            }
            found.push((*name, *start, self.statement_end(*start, span.end())));
        }
        found
    }

    /// The innermost `const` or `static` whose statement holds `at`.
    fn declared_value(&self, at: usize) -> Option<&str> {
        let file = self.index.file_at(at).expect("a byte of the index");
        self.value_statements(file)
            .into_iter()
            .filter(|(_, start, end)| *start <= at && at < *end)
            .min_by_key(|(_, start, end)| end - start)
            .map(|(name, _, _)| name)
    }

    fn innermost_callable(&self, at: usize) -> Option<&ItemRecord> {
        self.index
            .items()
            .iter()
            .filter(|item| item.kind().is_callable())
            .filter(|item| item.body().is_some_and(|body| body.holds(at)))
            .min_by_key(|item| item.whole().len())
    }

    /// A module path as the lists write it: `crate` left off, and an
    /// integration test's, example's, bench's or build script's target name in
    /// front, which is what tells `census::x` from a library's `x`.
    fn qualified(file: &FileRecord, declared: &str) -> String {
        let owner = file
            .owners()
            .iter()
            .filter(|owner| {
                declared == owner.module_path
                    || declared.starts_with(&format!("{}::", owner.module_path))
            })
            .max_by_key(|owner| owner.module_path.len())
            .expect("a module path extends its file's");
        let path = declared
            .strip_prefix("crate")
            .map_or(declared, |rest| rest.trim_start_matches("::"));
        match owner.target.kind {
            TargetKind::Library | TargetKind::Binary => path.to_owned(),
            _ if path.is_empty() => owner.target.name.clone(),
            _ => format!("{}::{path}", owner.target.name),
        }
    }

    fn joined(module: &str, name: &str) -> String {
        if module.is_empty() {
            name.to_owned()
        } else {
            format!("{module}::{name}")
        }
    }

    fn item_names(&self, item: &ItemRecord) -> BTreeSet<String> {
        let file = self.index.file_of(item);
        let name = match item.type_owner() {
            Some(owner) => format!("{owner}::{}", item.name()),
            None => item.name().to_owned(),
        };
        item.module_paths()
            .into_iter()
            .map(|declared| Self::joined(&Self::qualified(file, declared), &name))
            .collect()
    }

    /// The names the bytes at `at` answer to: the innermost function around
    /// them, or — outside every function — the `const` or `static` they
    /// initialise, or their module itself when the statement declares neither
    /// (an attribute's argument).
    fn owners_of(&self, at: usize) -> BTreeSet<String> {
        if let Some(item) = self.innermost_callable(at) {
            return self.item_names(item);
        }
        let file = self.index.file_at(at).expect("a byte of the index");
        let module = self
            .index
            .modules()
            .iter()
            .filter(|module| module.span().holds(at))
            .min_by_key(|module| module.span().len())
            .expect("every byte is in a module");
        let value = self.declared_value(at);
        module
            .module_paths()
            .iter()
            .map(|path| {
                let module_name = Self::qualified(file, path);
                match value {
                    Some(value) => Self::joined(&module_name, value),
                    None => module_name,
                }
            })
            .collect()
    }

    fn line(&self, at: usize) -> usize {
        self.index.locate(at).expect("a byte of the index").line
    }

    fn relative(&self, file: &FileRecord) -> String {
        from_root(&normalized(file.path()), &normalized(self.root))
    }

    fn in_the_product(&self, span: Span) -> bool {
        Occurrence {
            span,
            certainty: Certainty::Resolved,
        }
        .in_the_product(self.index)
    }

    /// Every `include_str!` and `include_bytes!` of a committed document
    /// outside the product.
    fn included(&self) -> Vec<DocumentReader> {
        let root = normalized(self.root);
        let mut found = Vec::new();
        for invocation in self.index.macros() {
            if invocation.kind() != MacroKind::Invocation
                || !INCLUDE_MACROS.contains(&invocation.name())
                || self.in_the_product(invocation.span())
            {
                continue;
            }
            let file = self
                .index
                .file_at(invocation.span().start())
                .expect("a byte of the index");
            let directory = file.path().parent().expect("a file is in a directory");
            let subjects: BTreeSet<String> = self
                .strings_in(invocation.tokens().start(), invocation.tokens().end())
                .into_iter()
                .map(|value| from_root(&normalized(&directory.join(value)), &root))
                .filter(|path| self.committed.contains(path) && is_document(path))
                .collect();
            if subjects.is_empty() {
                continue;
            }
            found.push(DocumentReader {
                file: self.relative(file),
                line: self.line(invocation.span().start()),
                owners: self.owners_of(invocation.span().start()),
                readings: BTreeSet::from([Reading::Included]),
                subjects,
            });
        }
        found
    }

    /// The union offsets of every call of the three run-time reads outside the
    /// product, each the call's name token.
    ///
    /// All three are calls through bt-source's call pattern: `read_to_string(`,
    /// and `read(` and `open(` only where they end the paths `fs::read` and
    /// `File::open`. A bare mention — `let read = std::fs::read;` — calls
    /// nothing and is not one.
    fn read_calls(&self) -> BTreeMap<usize, Span> {
        let mut found = BTreeMap::new();
        for (name, path) in READ_CALLS {
            let calls = self
                .index
                .search(&Search::new(
                    needle!(Pattern::call(name)),
                    View::Identifiers,
                ))
                .unwrap_or_else(|failure| panic!("{failure}"))
                .spans();
            let path_ends: Option<BTreeSet<usize>> = path.map(|path| {
                self.index
                    .search(&Search::new(
                        needle!(Pattern::path(path)),
                        View::Identifiers,
                    ))
                    .unwrap_or_else(|failure| panic!("{failure}"))
                    .spans()
                    .into_iter()
                    .map(Span::end)
                    .collect()
            });
            for call in calls {
                let ends_the_path = path_ends
                    .as_ref()
                    .is_none_or(|ends| ends.contains(&call.end()));
                if ends_the_path && !self.in_the_product(call) {
                    found.insert(call.start(), call);
                }
            }
        }
        found
    }

    /// Every call outside the product that reads a committed document or is
    /// handed one, judged one call at a time by the paths that call is handed:
    /// the three run-time reads by their path argument, every other call and
    /// method by each argument and by the value it is called on.
    fn read_at_run_time(&self) -> (Vec<DocumentReader>, Vec<UnresolvedRead>) {
        let reads = self.read_calls();
        let world = self.world;
        let mut readers = Vec::new();
        let mut unresolved = Vec::new();
        for (position, file) in self.index.files().iter().enumerate() {
            let span = file.span();
            let text = self.index.text(span);
            let syntax = Rc::new(FileSyntax::of(text, file.path()));
            for call in &syntax.calls {
                let at = span.start() + call.offset;
                let file_id = (self.number, position);
                let module = world.module_at(file_id, call.offset);
                let evaluate = |expression: &syn::Expr| {
                    Evaluation {
                        world,
                        syntax: Rc::clone(&syntax),
                        file: file_id,
                        module: module.clone(),
                        function: call.function,
                        position: call.offset,
                        parameters: BTreeMap::new(),
                        visiting: Vec::new(),
                        admitted: true,
                    }
                    .of(expression)
                };
                // The three reads are judged by their path argument and
                // resolve a relative one; every other call by each argument
                // and by what it is called on.
                let read = reads.contains_key(&at);
                let handed: Vec<&syn::Expr> = if read {
                    call.arguments.iter().take(1).collect()
                } else {
                    call.receiver.iter().chain(&call.arguments).collect()
                };
                let judgements: Vec<Judgement> = handed
                    .into_iter()
                    .flat_map(|expression| self.judge(evaluate(expression), read))
                    .collect();
                if judgements
                    .iter()
                    .all(|judgement| matches!(judgement, Judgement::NotADocument))
                    || (!read && self.token_in_the_product(at))
                {
                    continue;
                }
                let subjects: BTreeSet<String> = judgements
                    .iter()
                    .filter_map(|judgement| match judgement {
                        Judgement::Document(subject) => Some(subject.clone()),
                        Judgement::NotADocument | Judgement::Unresolved(_) => None,
                    })
                    .collect();
                for why in judgements.iter().filter_map(|judgement| match judgement {
                    Judgement::Unresolved(why) => Some(why.clone()),
                    Judgement::NotADocument | Judgement::Document(_) => None,
                }) {
                    unresolved.push(UnresolvedRead {
                        file: self.relative(file),
                        line: self.line(at),
                        owners: self.owners_of(at),
                        callee: call.callee.clone(),
                        why,
                    });
                }
                if subjects.is_empty() {
                    continue;
                }
                let reading = if read {
                    Reading::AtRunTime
                } else {
                    Reading::HandedTo(call.callee.clone())
                };
                readers.push(DocumentReader {
                    file: self.relative(file),
                    line: self.line(at),
                    owners: self.owners_of(at),
                    readings: BTreeSet::from([reading]),
                    subjects,
                });
            }
        }
        (readers, unresolved)
    }

    /// Whether a build of the shipped program contains the name token that
    /// starts at `at`.
    fn token_in_the_product(&self, at: usize) -> bool {
        let tokens = self.index.tokens();
        let token = tokens[tokens.partition_point(|token| token.span().start() < at)];
        self.in_the_product(token.span())
    }

    /// What one path handed to a call is. `read` says whether the call is one
    /// of the three run-time reads, which resolve a relative path from the
    /// package directory.
    fn judge(&self, value: Value, read: bool) -> Vec<Judgement> {
        match value.into_path() {
            Value::Each(values) => values
                .into_iter()
                .flat_map(|value| self.judge(value, read))
                .collect(),
            Value::Unreduced(why) => vec![Judgement::Unresolved(why)],
            Value::Within(paths) => paths
                .into_iter()
                .flat_map(|path| self.judge(path, read))
                .collect(),
            Value::Path {
                base,
                segments,
                open,
            } => vec![self.judge_path(base, &segments, open, read)],
            Value::Text(_) | Value::Opaque | Value::Words => {
                unreachable!("into_path makes a path of text, nothing and words")
            }
        }
    }

    fn judge_path(&self, base: Base, segments: &[String], open: bool, read: bool) -> Judgement {
        match base {
            Base::Scratch | Base::Absolute => Judgement::NotADocument,
            Base::Relative if !read => Judgement::NotADocument,
            Base::Package | Base::Relative => {
                let mut path = self.package.to_path_buf();
                path.extend(segments);
                let from = from_root(&normalized(&path), &normalized(self.root));
                if from == "target" || from.starts_with("target/") {
                    Judgement::NotADocument
                } else if open {
                    Judgement::Unresolved(Why::Computed)
                } else if self.committed.contains(&from) && is_document(&from) {
                    Judgement::Document(from)
                } else {
                    Judgement::NotADocument
                }
            }
            // A base this reading cannot see reaches the checkout only as a
            // whole path from the workspace root: two segments or more, which
            // a scratch file's bare name is not.
            Base::Unknown => {
                let from = from_root(
                    &normalized(&segments.iter().collect::<PathBuf>()),
                    Path::new(""),
                );
                let whole = !open
                    && !segments.iter().any(|segment| segment == "..")
                    && from.split('/').count() >= 2;
                if whole && self.committed.contains(&from) && is_document(&from) {
                    Judgement::Document(from)
                } else {
                    Judgement::NotADocument
                }
            }
        }
    }
}

/// The three run-time reads: the name the call pattern finds, and the path
/// that name must end, if any.
const READ_CALLS: [(&str, Option<&str>); 3] = [
    ("read_to_string", None),
    ("read", Some("fs::read")),
    ("open", Some("File::open")),
];

/// What one read call's path turned out to be.
enum Judgement {
    /// Scratch, `target/`, Rust source, a file the repository does not
    /// commit, or a base this reading cannot see.
    NotADocument,
    /// A committed document, from the workspace root.
    Document(String),
    /// A path this reading had to give up on, and why.
    Unresolved(Why),
}

/// Why a path handed to a call could not be reduced, and — for a constant —
/// the path it was written as.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Why {
    /// Rooted in the package directory, with a piece this reading cannot
    /// reduce to a literal beside or after the root: a parameter, a
    /// `format!` placeholder it cannot fill, another crate's constant.
    Computed,
    /// A constant that reaches itself on its own resolution path.
    Cycle(String),
    /// A constant-spelled path this reading cannot place: no constant, `use`,
    /// glob or `thread_local!` of the module trees holds it, it is not
    /// visible from where it is written, or several `impl`s offer it.
    Elsewhere(String),
}

impl Why {
    fn said(&self) -> String {
        match self {
            Self::Computed => "a rooted path with a piece it cannot reduce".to_owned(),
            Self::Cycle(path) => format!("a constant that reaches itself: {path}"),
            Self::Elsewhere(path) => format!("a constant it cannot place: {path}"),
        }
    }

    /// The same cause, with the path it was written as where none is said.
    fn written(self, path: &str) -> Self {
        match self {
            Self::Cycle(said) if said.is_empty() => Self::Cycle(path.to_owned()),
            Self::Elsewhere(said) if said.is_empty() => Self::Elsewhere(path.to_owned()),
            other => other,
        }
    }

    fn elsewhere() -> Self {
        Self::Elsewhere(String::new())
    }

    fn cycle() -> Self {
        Self::Cycle(String::new())
    }
}

/// A path handed to a call that this reading could not reduce to a committed
/// file or rule out: a diagnostic, not a refusal.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct UnresolvedRead {
    file: String,
    line: usize,
    owners: BTreeSet<String>,
    callee: String,
    why: Why,
}

// ─────────────────────────────────────────────── one read call's path ──

/// Where a path starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Base {
    /// The package directory: `CARGO_MANIFEST_DIR`.
    Package,
    /// A relative path spelled as a string. A run-time read resolves it from
    /// the package directory, where `cargo test` runs; handed to anything
    /// else it is a string, not a path into the checkout.
    Relative,
    /// The machine's temporary directory or a test's scratch name
    /// (`temp_dir`, `temp_path`, `tempfile`).
    Scratch,
    /// An absolute path spelled as a string (`C:\Users\alice`, `/srv`): fully
    /// reduced, and outside the checkout — a committed absolute path into it
    /// would be a machine path, which `scripts/check-machine-paths.ps1`
    /// refuses.
    Absolute,
    /// A base this reading cannot see: a parameter, a field, a loop over
    /// anything but a literal list, a call into another file.
    Unknown,
}

/// What an expression handed to a read is, as far as its own spelling says.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    /// A string whose whole text is known.
    Text(String),
    /// A path: its base, the literal segments after it, and whether a piece
    /// this reading cannot reduce follows them.
    Path {
        base: Base,
        segments: Vec<String>,
        open: bool,
    },
    /// Nothing this reading can reduce, and nothing rooted in the checkout.
    Opaque,
    /// One of several values: a loop variable over a literal list.
    Each(Vec<Value>),
    /// A path this reading gave up on, and why: a diagnostic wherever it is
    /// handed.
    Unreduced(Why),
    /// Text with paths spelled into it (`format!(". '{}'", script.display())`),
    /// or a tuple, array or struct holding paths: it names each of them, and
    /// is judged by them.
    Within(Vec<Value>),
    /// Text whose content is not known and holds no path: a `{:?}` of a
    /// label, a file's contents, another variable's value at build time.
    Words,
}

impl Value {
    /// The value used as a path. A text is relative; an absolute one is
    /// outside the checkout, and judged nothing until it is joined.
    fn into_path(self) -> Self {
        match self {
            Self::Text(text) => match segments_of(&text) {
                Some(segments) => Self::Path {
                    base: Base::Relative,
                    segments,
                    open: false,
                },
                None => Self::Path {
                    base: Base::Absolute,
                    segments: Vec::new(),
                    open: false,
                },
            },
            Self::Opaque | Self::Words => Self::Path {
                base: Base::Unknown,
                segments: Vec::new(),
                open: false,
            },
            Self::Each(values) => Self::Each(values.into_iter().map(Self::into_path).collect()),
            path @ (Self::Path { .. } | Self::Unreduced(_) | Self::Within(_)) => path,
        }
    }

    fn package() -> Self {
        Self::Path {
            base: Base::Package,
            segments: Vec::new(),
            open: false,
        }
    }

    /// One value, several, or none.
    fn several(mut values: Vec<Self>) -> Self {
        match values.len() {
            0 => Self::Opaque,
            1 => values.pop().expect("one value"),
            _ => Self::Each(values),
        }
    }

    /// Whether this is, or names, a path rooted in the package directory, or
    /// a path already unreduced.
    fn holds_root(&self) -> bool {
        match self {
            // A path that leads only up and then into `target/` is built, not
            // committed.
            Self::Path {
                base: Base::Package,
                segments,
                ..
            } => {
                segments
                    .iter()
                    .find(|segment| *segment != "..")
                    .map(String::as_str)
                    != Some("target")
            }
            Self::Unreduced(_) => true,
            Self::Within(values) | Self::Each(values) => values.iter().any(Self::holds_root),
            _ => false,
        }
    }

    /// Whether this is, or names, a path of a base this reading sees.
    fn names_a_path(&self) -> bool {
        match self {
            Self::Path { base, .. } => *base != Base::Unknown,
            Self::Unreduced(_) => true,
            Self::Within(values) | Self::Each(values) => values.iter().any(Self::names_a_path),
            _ => false,
        }
    }

    /// Whether this is a piece this reading cannot reduce: anything at all.
    fn unreducible(&self) -> bool {
        matches!(
            self,
            Self::Opaque
                | Self::Path {
                    base: Base::Unknown,
                    ..
                }
        )
    }

    /// What a part of this value is, when the part cannot be picked out (an
    /// element of a list this reading cannot read, a field of a value not
    /// written as a tuple): unreduced the same way if this is, unreduced if
    /// this holds a rooted path, and nothing otherwise.
    fn unreachable_part(self) -> Self {
        match self {
            unreduced @ Self::Unreduced(_) => unreduced,
            value if value.holds_root() => Self::Unreduced(Why::Computed),
            _ => Self::Opaque,
        }
    }

    fn unreduced(&self) -> Option<Why> {
        match self {
            Self::Unreduced(why) => Some(why.clone()),
            _ => None,
        }
    }

    /// `self.with_extension(extension)`: the last segment's extension
    /// replaced, where both are known; the path left open where the
    /// extension is not.
    fn with_extension(self, extension: Self) -> Self {
        match (self, extension) {
            (Self::Each(values), extension) => Self::several(
                values
                    .into_iter()
                    .map(|value| value.with_extension(extension.clone()))
                    .collect(),
            ),
            (
                Self::Path {
                    base,
                    mut segments,
                    open: false,
                },
                Self::Text(extension),
            ) if segments.last().is_some_and(|last| last != "..") => {
                if let Some(last) = segments.last_mut() {
                    if let Some((stem, _)) = last.rsplit_once('.')
                        && !stem.is_empty()
                    {
                        *last = stem.to_owned();
                    }
                    if !extension.is_empty() {
                        last.push('.');
                        last.push_str(&extension);
                    }
                }
                Self::Path {
                    base,
                    segments,
                    open: false,
                }
            }
            (Self::Path { base, segments, .. }, _) => Self::Path {
                base,
                segments,
                open: true,
            },
            (other, _) => other,
        }
    }

    /// `self.join(other)`: a relative piece adds its segments, a rooted one
    /// replaces the path, one this reading cannot reduce leaves it open, and
    /// each of several values is joined in turn.
    fn join(self, other: Self) -> Self {
        if let Self::Each(values) = self {
            return Self::Each(
                values
                    .into_iter()
                    .map(|value| value.join(other.clone()))
                    .collect(),
            );
        }
        if let Self::Each(values) = other {
            return Self::Each(
                values
                    .into_iter()
                    .map(|value| self.clone().join(value))
                    .collect(),
            );
        }
        let (base, mut segments, mut open) = match self.into_path() {
            Self::Path {
                base,
                segments,
                open,
            } => (base, segments, open),
            Self::Unreduced(why) => return Self::Unreduced(why),
            // Text naming paths is no path to join onto: unreduced if it names
            // a rooted one, and no path into the checkout otherwise.
            within @ Self::Within(_) => {
                return if within.holds_root() {
                    Self::Unreduced(Why::Computed)
                } else {
                    Self::Opaque
                };
            }
            _ => unreachable!("into_path is a path, several, unreduced or text with paths"),
        };
        match other {
            Self::Text(text) => match segments_of(&text) {
                Some(more) => segments.extend(more),
                // Joining an absolute path replaces the path with it.
                None => {
                    return Self::Path {
                        base: Base::Absolute,
                        segments: Vec::new(),
                        open: false,
                    };
                }
            },
            Self::Unreduced(why) => return Self::Unreduced(why),
            Self::Each(_) => unreachable!("several values are joined one by one above"),
            within @ Self::Within(_) if within.holds_root() => {
                return Self::Unreduced(Why::Computed);
            }
            Self::Path {
                base: Base::Unknown,
                ..
            }
            | Self::Opaque
            | Self::Words
            | Self::Within(_) => open = true,
            rooted @ Self::Path { .. } => return rooted,
        }
        Self::Path {
            base,
            segments,
            open,
        }
    }

    fn parent(self) -> Self {
        self.join(Self::Text("..".to_owned()))
    }
}

/// A relative path's segments, `.` dropped and `..` kept; `None` for an
/// absolute path.
fn segments_of(text: &str) -> Option<Vec<String>> {
    let absolute = text.starts_with(['/', '\\'])
        || text
            .split(['/', '\\'])
            .next()
            .is_some_and(|first| first.contains(':'));
    (!absolute).then(|| {
        text.split(['/', '\\'])
            .filter(|segment| !segment.is_empty() && *segment != ".")
            .map(ToOwned::to_owned)
            .collect()
    })
}

/// One read call as the parser sees it.
struct ReadCall {
    /// The call's name token — the last segment of a called path, a method's
    /// name — from the start of the file.
    offset: usize,
    /// The callee as written: `read_session`, `bt_persist::read_session`,
    /// `read_link`.
    callee: String,
    arguments: Vec<syn::Expr>,
    /// What a method is called on.
    receiver: Option<syn::Expr>,
    /// The start of the function the call is written in.
    function: usize,
}

/// The methods that build or describe a path rather than hand it to anything:
/// a path they are called on, or handed, is not consumed by them.
const PATH_BUILDING: [&str; 29] = [
    "join",
    "parent",
    "and_then",
    "map",
    "to_path_buf",
    "as_path",
    "clone",
    "to_owned",
    "into",
    "as_ref",
    "unwrap",
    "expect",
    "borrow",
    "as_os_str",
    "display",
    "to_str",
    "to_string_lossy",
    "to_string",
    "file_name",
    "file_stem",
    "extension",
    "components",
    "strip_prefix",
    "starts_with",
    "ends_with",
    "with_extension",
    "with_file_name",
    "push",
    "iter",
];

/// Whether a called path builds a path or a string out of its argument
/// (`Path::new`, `PathBuf::from`, `String::from`) rather than consuming it.
fn converts(segments: &[String]) -> bool {
    let last = segments.last().map_or("", String::as_str);
    let owner = segments
        .len()
        .checked_sub(2)
        .map_or("", |at| segments[at].as_str());
    matches!(
        (owner, last),
        ("Path" | "OsStr", "new") | ("PathBuf" | "String" | "OsString" | "From", "from")
    )
}

/// One binding of a single name: a `let` (or `let … else`), an `if let` or
/// `while let`, a `match` arm, or a `for` loop's variable.
struct Binding {
    /// Where the binding is written: its value is read as of here.
    offset: usize,
    function: usize,
    /// The bytes the name is in scope over: from the end of a `let`'s
    /// statement to the end of its block; an `if let`'s branch, a
    /// `while let`'s or a `for` loop's body; a `match` arm's body.
    scope: (usize, usize),
    name: String,
    /// What the pattern is matched against: a `let`'s value, a `match`'s
    /// scrutinee, what a `for` loop iterates.
    value: syn::Expr,
    /// The steps the pattern takes the name from — empty for the whole
    /// value; `Some(x)` and `Ok(x)` take the value itself.
    projection: Vec<Step>,
    /// A loop variable: the name is each element of `value` in turn.
    each: bool,
}

/// One step into a value: a tuple's or tuple struct's position, or a
/// struct's field.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Step {
    Index(usize),
    Field(String),
}

/// Every name a pattern binds, with the steps that reach it. `Some(x)` and
/// `Ok(x)` bind `x` to the value they unwrap; a slice or a `|` pattern binds
/// nothing this reading follows, and a name it binds is a parameter of the
/// pattern (the unseen base).
fn pattern_names(pattern: &syn::Pat, at: &mut Vec<Step>, found: &mut Vec<(String, Vec<Step>)>) {
    match pattern {
        syn::Pat::Ident(name) => found.push((name.ident.to_string(), at.clone())),
        syn::Pat::Type(typed) => pattern_names(&typed.pat, at, found),
        syn::Pat::Reference(reference) => pattern_names(&reference.pat, at, found),
        syn::Pat::Paren(inner) => pattern_names(&inner.pat, at, found),
        syn::Pat::Tuple(tuple) => {
            for (position, element) in tuple.elems.iter().enumerate() {
                at.push(Step::Index(position));
                pattern_names(element, at, found);
                at.pop();
            }
        }
        syn::Pat::TupleStruct(wrapped)
            if wrapped.elems.len() == 1
                && wrapped
                    .path
                    .segments
                    .last()
                    .is_some_and(|last| last.ident == "Some" || last.ident == "Ok") =>
        {
            pattern_names(&wrapped.elems[0], at, found);
        }
        syn::Pat::TupleStruct(wrapped) => {
            for (position, element) in wrapped.elems.iter().enumerate() {
                at.push(Step::Index(position));
                pattern_names(element, at, found);
                at.pop();
            }
        }
        syn::Pat::Struct(record) => {
            for field in &record.fields {
                at.push(match &field.member {
                    syn::Member::Named(name) => Step::Field(name.to_string()),
                    syn::Member::Unnamed(position) => Step::Index(position.index as usize),
                });
                pattern_names(&field.pat, at, found);
                at.pop();
            }
        }
        _ => {}
    }
}

/// The part of a value a pattern's steps pick, where the value is written as
/// tuples, tuple structs and struct literals.
fn projected<'e>(element: &'e syn::Expr, steps: &[Step]) -> Option<&'e syn::Expr> {
    let Some((first, rest)) = steps.split_first() else {
        return Some(element);
    };
    match (element, first) {
        (syn::Expr::Paren(inner), _) => projected(&inner.expr, steps),
        (syn::Expr::Reference(reference), _) => projected(&reference.expr, steps),
        (syn::Expr::Tuple(tuple), Step::Index(position)) => {
            projected(tuple.elems.iter().nth(*position)?, rest)
        }
        // A tuple struct's or variant's constructor — capitalised, as
        // rustc's `non_camel_case_types` has every type and variant — and not
        // a function, whose result is no tuple of its arguments.
        (syn::Expr::Call(call), Step::Index(position))
            if matches!(&*call.func, syn::Expr::Path(path)
            if path.path.segments.last().is_some_and(|last| {
                last.ident.to_string().starts_with(|first: char| first.is_ascii_uppercase())
            })) =>
        {
            projected(call.args.iter().nth(*position)?, rest)
        }
        (syn::Expr::Struct(record), step) => {
            let field = record
                .fields
                .iter()
                .find(|field| match (&field.member, step) {
                    (syn::Member::Named(name), Step::Field(wanted)) => name == wanted,
                    (syn::Member::Unnamed(position), Step::Index(wanted)) => {
                        position.index as usize == *wanted
                    }
                    _ => false,
                })?;
            projected(&field.expr, rest)
        }
        _ => None,
    }
}

/// A free function of the file: its parameters' names and the expression it
/// returns.
struct Helper {
    function: usize,
    /// Where the returned expression starts: the point its bindings are seen
    /// from.
    tail_at: usize,
    parameters: Vec<Option<String>>,
    tail: Option<syn::Expr>,
}

/// A `const` or `static`: the inline modules of its file it is written in,
/// and its value.
struct Constant {
    inline: Vec<String>,
    value: syn::Expr,
    visibility: Visibility,
}

/// Who may name an item, as written on it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Visibility {
    /// `pub`: anyone who can reach its module.
    Public,
    /// `pub(crate)`: its own crate.
    Crate,
    /// `pub(super)`, `pub(self)`, `pub(in path)`: the module the path leads
    /// to from the item's, and its descendants.
    Within(Vec<String>),
    /// No `pub`: its own module and its descendants.
    Private,
}

impl Visibility {
    fn of(written: &syn::Visibility) -> Self {
        match written {
            syn::Visibility::Public(_) => Self::Public,
            syn::Visibility::Inherited => Self::Private,
            syn::Visibility::Restricted(restricted) => {
                let path: Vec<String> = restricted
                    .path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect();
                if path == ["crate"] {
                    Self::Crate
                } else {
                    Self::Within(path)
                }
            }
        }
    }

    /// Whether code in `viewer` may name an item with this visibility that
    /// belongs to the module `owner`, as Rust decides: across a crate's
    /// boundary only `pub`; inside it, the module the visibility opens.
    fn admits(&self, owner: &ModuleKey, viewer: Option<&ModuleKey>) -> bool {
        let Some(viewer) = viewer else {
            return true;
        };
        let same_crate = owner.0 == viewer.0;
        let inside = |scope: &str| viewer.1 == scope || viewer.1.starts_with(&format!("{scope}::"));
        match self {
            Self::Public => true,
            Self::Crate => same_crate,
            Self::Private => same_crate && inside(&owner.1),
            Self::Within(path) => {
                let mut scope = owner.1.clone();
                for segment in path {
                    match segment.as_str() {
                        "self" | "in" => {}
                        "crate" => scope = "crate".to_owned(),
                        "super" => {
                            if let Some((parent, _)) = scope.rsplit_once("::") {
                                scope = parent.to_owned();
                            }
                        }
                        child => scope = format!("{scope}::{child}"),
                    }
                }
                same_crate && inside(&scope)
            }
        }
    }
}

/// An `impl` block: the inline modules it is written in, the type it is
/// for, the trait it implements (as written), if any, and the constants it
/// declares by name.
struct ImplBlock {
    inline: Vec<String>,
    owner: String,
    implemented: Option<Vec<String>>,
    constants: BTreeMap<String, Vec<Rc<Constant>>>,
}

/// Which trait a trait path names, once followed: a trait a first-party
/// module declares, or another crate's, by its full path as written after
/// following first-party `use`s.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum TraitIdentity {
    FirstParty(ModuleKey, String),
    Dependency(String),
}

/// The constants one `impl` block offers under a name, each in the file it
/// is written in.
type Offered = Vec<(FileId, Rc<Constant>)>;

/// What an `impl` block implements: nothing (inherent), a trait it can tell,
/// or a trait whose path it cannot follow (a name a glob of another crate's
/// module may bring in).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Implemented {
    Inherent,
    Trait(TraitIdentity),
    Unfollowed,
}

/// What one file's syntax says about the paths its calls are handed: the
/// calls, the bindings, the file's constants, `use`s and free functions, and
/// its text, to place a span. Parsed from the index's own text of the file.
struct FileSyntax {
    text: String,
    line_starts: Vec<usize>,
    calls: Vec<ReadCall>,
    bindings: Vec<Binding>,
    /// By bare name, file-wide: every constant of that name (two under
    /// exclusive `cfg`s are both kept, and both judged).
    constants: BTreeMap<String, Vec<Rc<Constant>>>,
    /// By the inline modules they are written in and their name.
    scoped: BTreeMap<(Vec<String>, String), Vec<Rc<Constant>>>,
    /// The `impl` blocks of the file, by the type they are for.
    impl_blocks: BTreeMap<String, Vec<Rc<ImplBlock>>>,
    /// The traits the file declares, by the inline modules they are written
    /// in.
    traits: BTreeSet<(Vec<String>, String)>,
    /// The constants a trait gives a default value, by the inline modules
    /// the trait is written in, its name and theirs.
    defaults: BTreeMap<(Vec<String>, String, String), Rc<Constant>>,
    /// Each `use` binding of a name, by the inline modules it is written in:
    /// the path it names, and who the `use` item admits.
    uses: BTreeMap<(Vec<String>, String), (Vec<String>, Visibility)>,
    /// Each `use …::*`, by the inline modules it is written in, and who the
    /// `use` item admits.
    globs: BTreeMap<Vec<String>, Vec<(Vec<String>, Visibility)>>,
    /// The statics `thread_local!` declares: keys to a thread's own value,
    /// never a path.
    locals: BTreeSet<(Vec<String>, String)>,
    helpers: BTreeMap<String, Vec<Helper>>,
}

impl FileSyntax {
    fn of(text: &str, path: &Path) -> Self {
        let file = syn::parse_file(text).unwrap_or_else(|error| {
            panic!(
                "{} is Rust the source index already accepted: {error}",
                path.display()
            )
        });
        let mut collector = Collector {
            functions: vec![0],
            blocks: vec![(0, text.len())],
            modules: Vec::new(),
            impls: Vec::new(),
            syntax: Self {
                text: text.to_owned(),
                line_starts: std::iter::once(0)
                    .chain(text.match_indices('\n').map(|(at, _)| at + 1))
                    .collect(),
                calls: Vec::new(),
                bindings: Vec::new(),
                constants: BTreeMap::new(),
                scoped: BTreeMap::new(),
                impl_blocks: BTreeMap::new(),
                traits: BTreeSet::new(),
                defaults: BTreeMap::new(),
                uses: BTreeMap::new(),
                globs: BTreeMap::new(),
                locals: BTreeSet::new(),
                helpers: BTreeMap::new(),
            },
        };
        syn::visit::visit_file(&mut collector, &file);
        collector.syntax
    }

    /// A position as a byte offset into the file. The parser counts columns
    /// in characters.
    fn at(&self, at: proc_macro2::LineColumn) -> usize {
        let line = self.line_starts[at.line - 1];
        let rest = &self.text[line..];
        line + rest
            .char_indices()
            .nth(at.column)
            .map_or(rest.len(), |(byte, _)| byte)
    }

    fn offset(&self, span: proc_macro2::Span) -> usize {
        self.at(span.start())
    }

    fn end(&self, span: proc_macro2::Span) -> usize {
        self.at(span.end())
    }
}

struct Collector {
    /// The start of each function the walk is inside, innermost last.
    functions: Vec<usize>,
    /// The span of each block the walk is inside, innermost last.
    blocks: Vec<(usize, usize)>,
    /// The inline modules the walk is inside, outermost first.
    modules: Vec<String>,
    /// Each `impl` block the walk is inside, innermost last.
    impls: Vec<ImplBlock>,
    syntax: FileSyntax,
}

impl Collector {
    fn offset(&self, span: proc_macro2::Span) -> usize {
        self.syntax.offset(span)
    }

    fn end(&self, span: proc_macro2::Span) -> usize {
        self.syntax.end(span)
    }

    fn function(&self) -> usize {
        *self
            .functions
            .last()
            .expect("the file is the outermost scope")
    }

    fn record_constant(&mut self, name: String, value: &syn::Expr, visibility: Visibility) {
        let constant = Rc::new(Constant {
            inline: self.modules.clone(),
            value: value.clone(),
            visibility,
        });
        self.syntax
            .constants
            .entry(name.clone())
            .or_default()
            .push(Rc::clone(&constant));
        self.syntax
            .scoped
            .entry((self.modules.clone(), name))
            .or_default()
            .push(constant);
    }

    /// Bindings for every name a pattern binds, matched against `value`, in
    /// scope over `scope`.
    fn bind(
        &mut self,
        pattern: &syn::Pat,
        value: &syn::Expr,
        at: usize,
        scope: (usize, usize),
        each: bool,
    ) {
        let mut names = Vec::new();
        pattern_names(pattern, &mut Vec::new(), &mut names);
        for (name, projection) in names {
            self.syntax.bindings.push(Binding {
                offset: at,
                function: self.function(),
                scope,
                name,
                value: value.clone(),
                projection,
                each,
            });
        }
    }

    /// The `let`s of an `if`'s or `while`'s condition, `&&`-chained or alone.
    fn bind_condition(&mut self, condition: &syn::Expr, at: usize, scope: (usize, usize)) {
        match condition {
            syn::Expr::Let(binding) => {
                self.bind(&binding.pat, &binding.expr, at, scope, false);
            }
            syn::Expr::Binary(chain) if matches!(chain.op, syn::BinOp::And(_)) => {
                self.bind_condition(&chain.left, at, scope);
                self.bind_condition(&chain.right, at, scope);
            }
            syn::Expr::Paren(inner) => self.bind_condition(&inner.expr, at, scope),
            _ => {}
        }
    }
}

fn bound_name(pattern: &syn::Pat) -> Option<String> {
    match pattern {
        syn::Pat::Ident(name) => Some(name.ident.to_string()),
        syn::Pat::Type(typed) => bound_name(&typed.pat),
        syn::Pat::Reference(reference) => bound_name(&reference.pat),
        _ => None,
    }
}

/// Every name a `use` tree binds, with the path it names, and every glob.
fn use_bindings(
    tree: &syn::UseTree,
    prefix: &mut Vec<String>,
    names: &mut Vec<(String, Vec<String>)>,
    globs: &mut Vec<Vec<String>>,
) {
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            use_bindings(&path.tree, prefix, names, globs);
            prefix.pop();
        }
        syn::UseTree::Name(name) => {
            let mut path = prefix.clone();
            path.push(name.ident.to_string());
            names.push((name.ident.to_string(), path));
        }
        syn::UseTree::Rename(rename) => {
            let mut path = prefix.clone();
            path.push(rename.ident.to_string());
            names.push((rename.rename.to_string(), path));
        }
        syn::UseTree::Glob(_) => globs.push(prefix.clone()),
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                use_bindings(tree, prefix, names, globs);
            }
        }
    }
}

/// `vec![element; count]`'s element.
fn repeated(mac: &syn::Macro) -> Option<syn::Expr> {
    if !mac.path.is_ident("vec") {
        return None;
    }
    mac.parse_body_with(|input: syn::parse::ParseStream<'_>| {
        let element: syn::Expr = input.parse()?;
        input.parse::<syn::Token![;]>()?;
        input.parse::<syn::Expr>()?;
        Ok(element)
    })
    .ok()
}

/// The expression a function returns: its tail, or a `return` that ends it.
fn returned(block: &syn::Block) -> Option<&syn::Expr> {
    match block.stmts.last()? {
        syn::Stmt::Expr(syn::Expr::Return(early), _) => early.expr.as_deref(),
        syn::Stmt::Expr(tail, None) => Some(tail),
        _ => None,
    }
}

impl<'ast> syn::visit::Visit<'ast> for Collector {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if item.content.is_some() {
            self.modules.push(item.ident.to_string());
            syn::visit::visit_item_mod(self, item);
            self.modules.pop();
        }
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let name = match &*item.self_ty {
            syn::Type::Path(path) => path.path.segments.last().map(|last| last.ident.to_string()),
            _ => None,
        };
        let implemented = item.trait_.as_ref().map(|(_, path, _)| {
            path.segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect()
        });
        self.impls.push(ImplBlock {
            inline: self.modules.clone(),
            owner: name.unwrap_or_default(),
            implemented,
            constants: BTreeMap::new(),
        });
        syn::visit::visit_item_impl(self, item);
        let block = self.impls.pop().expect("the block just entered");
        if !block.owner.is_empty() {
            self.syntax
                .impl_blocks
                .entry(block.owner.clone())
                .or_default()
                .push(Rc::new(block));
        }
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        let name = item.ident.to_string();
        for member in &item.items {
            if let syn::TraitItem::Const(constant) = member
                && let Some((_, value)) = &constant.default
            {
                self.syntax.defaults.insert(
                    (
                        self.modules.clone(),
                        name.clone(),
                        constant.ident.to_string(),
                    ),
                    Rc::new(Constant {
                        inline: self.modules.clone(),
                        value: value.clone(),
                        visibility: Visibility::Public,
                    }),
                );
            }
        }
        self.syntax.traits.insert((self.modules.clone(), name));
        syn::visit::visit_item_trait(self, item);
    }

    fn visit_impl_item_const(&mut self, item: &'ast syn::ImplItemConst) {
        if let Some(block) = self.impls.last_mut() {
            // A trait's item is as visible as the trait; an inherent one as
            // written.
            let visibility = if block.implemented.is_some() {
                Visibility::Public
            } else {
                Visibility::of(&item.vis)
            };
            block
                .constants
                .entry(item.ident.to_string())
                .or_default()
                .push(Rc::new(Constant {
                    inline: self.modules.clone(),
                    value: item.expr.clone(),
                    visibility,
                }));
        }
        syn::visit::visit_impl_item_const(self, item);
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.blocks
            .push((self.offset(block.span()), self.end(block.span())));
        syn::visit::visit_block(self, block);
        self.blocks.pop();
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let start = self.offset(item.span());
        let tail = returned(&item.block).cloned();
        let helper = Helper {
            function: start,
            tail_at: tail.as_ref().map_or(start, |tail| self.offset(tail.span())),
            parameters: item
                .sig
                .inputs
                .iter()
                .map(|input| match input {
                    syn::FnArg::Typed(typed) => bound_name(&typed.pat),
                    syn::FnArg::Receiver(_) => None,
                })
                .collect(),
            tail,
        };
        self.syntax
            .helpers
            .entry(item.sig.ident.to_string())
            .or_default()
            .push(helper);
        self.functions.push(start);
        syn::visit::visit_item_fn(self, item);
        self.functions.pop();
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.functions.push(self.offset(item.span()));
        syn::visit::visit_impl_item_fn(self, item);
        self.functions.pop();
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        self.functions.push(self.offset(item.span()));
        syn::visit::visit_trait_item_fn(self, item);
        self.functions.pop();
    }

    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        self.record_constant(
            item.ident.to_string(),
            &item.expr,
            Visibility::of(&item.vis),
        );
        syn::visit::visit_item_const(self, item);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        let mut names = Vec::new();
        let mut globs = Vec::new();
        use_bindings(&item.tree, &mut Vec::new(), &mut names, &mut globs);
        let visibility = Visibility::of(&item.vis);
        for (name, path) in names {
            self.syntax
                .uses
                .insert((self.modules.clone(), name), (path, visibility.clone()));
        }
        self.syntax
            .globs
            .entry(self.modules.clone())
            .or_default()
            .extend(globs.into_iter().map(|glob| (glob, visibility.clone())));
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        self.record_constant(
            item.ident.to_string(),
            &item.expr,
            Visibility::of(&item.vis),
        );
        syn::visit::visit_item_static(self, item);
    }

    /// A `let` is in scope from the end of its statement — after its
    /// initialiser, so `let path = path.join(…)` reads the earlier `path` —
    /// to the end of its block.
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let Some(init) = &local.init {
            let block = *self.blocks.last().expect("the file is the outermost block");
            let scope = (self.end(local.span()), block.1);
            self.bind(
                &local.pat,
                &init.expr,
                self.offset(local.span()),
                scope,
                false,
            );
        }
        syn::visit::visit_local(self, local);
    }

    fn visit_expr_if(&mut self, branch: &'ast syn::ExprIf) {
        let scope = (
            self.offset(branch.then_branch.span()),
            self.end(branch.then_branch.span()),
        );
        self.bind_condition(&branch.cond, self.offset(branch.span()), scope);
        syn::visit::visit_expr_if(self, branch);
    }

    fn visit_expr_while(&mut self, looped: &'ast syn::ExprWhile) {
        let scope = (
            self.offset(looped.body.span()),
            self.end(looped.body.span()),
        );
        self.bind_condition(&looped.cond, self.offset(looped.span()), scope);
        syn::visit::visit_expr_while(self, looped);
    }

    fn visit_expr_match(&mut self, matched: &'ast syn::ExprMatch) {
        for arm in &matched.arms {
            let scope = (self.offset(arm.body.span()), self.end(arm.body.span()));
            self.bind(
                &arm.pat,
                &matched.expr,
                self.offset(arm.span()),
                scope,
                false,
            );
        }
        syn::visit::visit_expr_match(self, matched);
    }

    fn visit_expr_for_loop(&mut self, looped: &'ast syn::ExprForLoop) {
        let scope = (
            self.offset(looped.body.span()),
            self.end(looped.body.span()),
        );
        self.bind(
            &looped.pat,
            &looped.expr,
            self.offset(looped.span()),
            scope,
            true,
        );
        syn::visit::visit_expr_for_loop(self, looped);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*call.func
            && let Some(last) = path.path.segments.last()
        {
            let segments: Vec<String> = path
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            if !converts(&segments) {
                self.syntax.calls.push(ReadCall {
                    offset: self.offset(last.ident.span()),
                    callee: segments.join("::"),
                    arguments: call.args.iter().cloned().collect(),
                    receiver: None,
                    function: self.function(),
                });
            }
        } else {
            // A call of a value — `(self.open)(path)`, `handlers[0](path)` —
            // is handed its arguments all the same.
            self.syntax.calls.push(ReadCall {
                offset: self.offset(call.func.span()),
                callee: "(callable)".to_owned(),
                arguments: call.args.iter().cloned().collect(),
                receiver: None,
                function: self.function(),
            });
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let method = call.method.to_string();
        if !PATH_BUILDING.contains(&method.as_str()) {
            self.syntax.calls.push(ReadCall {
                offset: self.offset(call.method.span()),
                callee: method,
                arguments: call.args.iter().cloned().collect(),
                receiver: Some((*call.receiver).clone()),
                function: self.function(),
            });
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    /// A macro's arguments, where they parse as expressions — `assert!`,
    /// `assert_eq!`, `format!`, `vec![…]` — are walked like any other code;
    /// `thread_local!`'s statics are recorded as never a path.
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac
            .path
            .segments
            .last()
            .is_some_and(|last| last.ident == "thread_local")
        {
            let statics = mac.parse_body_with(|input: syn::parse::ParseStream<'_>| {
                let mut statics = Vec::new();
                while !input.is_empty() {
                    statics.push(input.parse::<syn::ItemStatic>()?);
                }
                Ok(statics)
            });
            for item in statics.into_iter().flatten() {
                self.syntax
                    .locals
                    .insert((self.modules.clone(), item.ident.to_string()));
            }
            return;
        }
        let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        if let Ok(arguments) = mac.parse_body_with(parser) {
            for argument in &arguments {
                self.visit_expr(argument);
            }
        }
    }
}

/// A module of one target: the target that compiles it, and the module path
/// bt-source gives it there.
type ModuleKey = (TargetId, String);

/// A file of the scan: the position of its package's index, and its position
/// in that index's files.
type FileId = (usize, usize);

/// Where every module of every first-party index is written, the library
/// each first-party crate name stands for, and the files read so far for a
/// constant another file names.
struct World<'a> {
    indices: &'a [(PathBuf, Index)],
    /// Each module: its file, and the inline `mod`s inside that file that
    /// lead to it.
    /// One module may be written in several files, each under its own `cfg`
    /// (`#[cfg(windows)] #[path = "x.rs"] mod x;` beside an `x_unsupported.rs`).
    modules: BTreeMap<ModuleKey, Vec<(FileId, Vec<String>)>>,
    /// The other way round: the module a file's inline path is, in the first
    /// target that compiles it.
    placed: BTreeMap<(FileId, Vec<String>), ModuleKey>,
    /// Each first-party library, by the crate name a path spells it with.
    crates: BTreeMap<String, TargetId>,
    read: RefCell<BTreeMap<FileId, Rc<FileSyntax>>>,
}

/// The owning declaration path of a file a module path extends.
fn owner_of<'f>(file: &'f FileRecord, declared: &str) -> Option<&'f bt_source::FileOwner> {
    file.owners()
        .iter()
        .filter(|owner| {
            declared == owner.module_path
                || declared.starts_with(&format!("{}::", owner.module_path))
        })
        .max_by_key(|owner| owner.module_path.len())
}

/// Where a module path leads from a module: to a module of the same target;
/// to a segment that is no module there — a type, a function, an enum —
/// with the module it was looked up in; or nowhere (`super` past the root).
enum Walk {
    Module(ModuleKey),
    Item { module: ModuleKey, at: usize },
    Lost,
}

impl<'a> World<'a> {
    fn of(indices: &'a [(PathBuf, Index)]) -> Self {
        let mut modules = BTreeMap::new();
        let mut placed = BTreeMap::new();
        let mut crates = BTreeMap::new();
        for (number, (_, index)) in indices.iter().enumerate() {
            for module in index.modules() {
                let start = module.span().start();
                let position = index
                    .files()
                    .partition_point(|file| file.span().end() <= start);
                let file = &index.files()[position];
                for declared in module.module_paths() {
                    let Some(owner) = owner_of(file, declared) else {
                        continue;
                    };
                    let inline: Vec<String> = declared[owner.module_path.len()..]
                        .split("::")
                        .filter(|segment| !segment.is_empty())
                        .map(ToOwned::to_owned)
                        .collect();
                    if owner.target.kind == TargetKind::Library {
                        crates
                            .entry(owner.target.name.replace('-', "_"))
                            .or_insert_with(|| owner.target.clone());
                    }
                    let key = (owner.target.clone(), declared.clone());
                    placed
                        .entry(((number, position), inline.clone()))
                        .or_insert_with(|| key.clone());
                    let placements: &mut Vec<(FileId, Vec<String>)> =
                        modules.entry(key).or_default();
                    if !placements.contains(&((number, position), inline.clone())) {
                        placements.push(((number, position), inline));
                    }
                }
            }
        }
        Self {
            indices,
            modules,
            placed,
            crates,
            read: RefCell::new(BTreeMap::new()),
        }
    }

    fn index(&self, file: FileId) -> &'a Index {
        &self.indices[file.0].1
    }

    /// The syntax of a file, read once, its calls left out.
    fn syntax(&self, file: FileId) -> Rc<FileSyntax> {
        if let Some(syntax) = self.read.borrow().get(&file) {
            return Rc::clone(syntax);
        }
        let index = self.index(file);
        let record = &index.files()[file.1];
        let mut syntax = FileSyntax::of(index.text(record.span()), record.path());
        syntax.calls.clear();
        let syntax = Rc::new(syntax);
        self.read.borrow_mut().insert(file, Rc::clone(&syntax));
        syntax
    }

    /// The module a byte of a file is written in.
    fn module_at(&self, file: FileId, offset: usize) -> Option<ModuleKey> {
        let index = self.index(file);
        let record = &index.files()[file.1];
        let at = record.span().start() + offset;
        let module = index
            .modules()
            .iter()
            .filter(|module| module.span().holds(at))
            .min_by_key(|module| module.span().len())?;
        let declared = module.module_paths().first()?;
        let owner = owner_of(record, declared)?;
        Some((owner.target.clone(), declared.clone()))
    }

    /// The type of the `impl` block a byte of a file is written in.
    fn impl_type_at(&self, file: FileId, offset: usize) -> Option<String> {
        let index = self.index(file);
        let at = index.files()[file.1].span().start() + offset;
        index
            .impls()
            .iter()
            .filter(|block| block.body().holds(at))
            .min_by_key(|block| block.body().len())
            .map(|block| block.type_owner().to_owned())
    }

    /// Whether an index declares a `struct`, `enum` or `union` of that name.
    fn declares_type(&self, number: usize, name: &str) -> bool {
        self.indices[number]
            .1
            .items()
            .iter()
            .any(|item| item.kind().is_type() && item.name() == name)
    }

    /// Every `impl` block of the named type, in one index.
    fn impl_blocks(&self, number: usize, owner: &str) -> Vec<(FileId, Rc<ImplBlock>)> {
        let index = &self.indices[number].1;
        let mut files: BTreeSet<FileId> = BTreeSet::new();
        for block in index.impls() {
            if block.type_owner() == owner {
                let start = block.body().start();
                files.insert((
                    number,
                    index
                        .files()
                        .partition_point(|file| file.span().end() <= start),
                ));
            }
        }
        files
            .into_iter()
            .flat_map(|file| {
                self.syntax(file)
                    .impl_blocks
                    .get(owner)
                    .into_iter()
                    .flatten()
                    .map(|block| (file, Rc::clone(block)))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Where a path of module segments leads from a module: a first-party
    /// crate's name, `self`, `super`, `crate`, and the child modules
    /// bt-source's module tree declares. The tree compiles, so a module a
    /// path names is one Rust let it name: no visibility is asked here.
    fn walk(&self, from: Option<&ModuleKey>, segments: &[String]) -> Walk {
        let (target, mut path, rest) = match segments.first() {
            Some(first) if self.crates.contains_key(first) => (
                self.crates[first].clone(),
                "crate".to_owned(),
                &segments[1..],
            ),
            _ => match from {
                Some((target, path)) => (target.clone(), path.clone(), segments),
                None => return Walk::Lost,
            },
        };
        for (at, segment) in rest.iter().enumerate() {
            match segment.as_str() {
                "self" => {}
                "crate" => path = "crate".to_owned(),
                "super" => match path.rsplit_once("::") {
                    Some((parent, _)) => path = parent.to_owned(),
                    None => return Walk::Lost,
                },
                child => {
                    let next = format!("{path}::{child}");
                    if !self.modules.contains_key(&(target.clone(), next.clone())) {
                        return Walk::Item {
                            module: (target, path),
                            at: at + segments.len() - rest.len(),
                        };
                    }
                    path = next;
                }
            }
        }
        Walk::Module((target, path))
    }
}

/// What a `use` leads to, each constant's scope marked as reached through
/// a `use` that does, or does not, admit the reader.
fn through(resolved: Resolved<'_>, admits: bool) -> Resolved<'_> {
    match resolved {
        Resolved::Found(found) => Resolved::Found(
            found
                .into_iter()
                .map(|(mut scope, constant)| {
                    scope.admitted &= admits;
                    (scope, constant)
                })
                .collect(),
        ),
        missing => missing,
    }
}

/// A constant's name: SCREAMING_SNAKE_CASE. `non_upper_case_globals` warns on
/// any other, and CI runs clippy with `-D warnings` over the whole workspace
/// (the workspace's `Cargo.toml` does not list it: it is rustc's default
/// warn, and CI makes it a refusal), so every
/// constant is spelled so; a path ending in another name that leads to no
/// constant is a function, a type or a variant.
fn names_a_constant(name: &str) -> bool {
    name.chars().all(|character| {
        character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
    }) && name.chars().any(|character| character.is_ascii_uppercase())
}

/// The constants found, each with the scope its value is read in, or the
/// value that stands for none.
enum Resolved<'a> {
    Found(Vec<(Evaluation<'a>, Rc<Constant>)>),
    Missing(Value),
}

/// A file's syntax, its place in the scan, and a module written in it.
struct Place {
    syntax: Rc<FileSyntax>,
    file: FileId,
    module: Option<ModuleKey>,
}

/// One expression, evaluated as a path in the scope of one call.
struct Evaluation<'a> {
    world: &'a World<'a>,
    syntax: Rc<FileSyntax>,
    file: FileId,
    /// The module the expression is written in.
    module: Option<ModuleKey>,
    /// The function whose bindings are in scope.
    function: usize,
    /// Only bindings whose scope holds this offset are in scope.
    position: usize,
    /// A helper's parameters, bound to what its caller handed it.
    parameters: BTreeMap<String, Value>,
    /// The constants and helpers on the current resolution path. A chain is
    /// followed as far as it goes; one that comes back to itself is a cycle.
    visiting: Vec<String>,
    /// Whether every `use` the resolution went through to reach this
    /// constant's scope admits the module the path is written in: only
    /// [`Evaluation::disambiguated`] asks.
    admitted: bool,
}

impl<'a> Evaluation<'a> {
    /// The same evaluation in another scope, one step further along the
    /// resolution path; `None` when that step comes back to itself.
    fn entering(
        &self,
        identity: String,
        place: Place,
        function: usize,
        position: usize,
        parameters: BTreeMap<String, Value>,
    ) -> Option<Evaluation<'a>> {
        if self.visiting.contains(&identity) {
            return None;
        }
        let mut visiting = self.visiting.clone();
        visiting.push(identity);
        Some(Evaluation {
            world: self.world,
            syntax: place.syntax,
            file: place.file,
            module: place.module,
            function,
            position,
            parameters,
            visiting,
            admitted: true,
        })
    }

    /// The same scope, seen from another point of the same function.
    fn at(&self, position: usize) -> Evaluation<'a> {
        Evaluation {
            world: self.world,
            syntax: Rc::clone(&self.syntax),
            file: self.file,
            module: self.module.clone(),
            function: self.function,
            position,
            parameters: self.parameters.clone(),
            visiting: self.visiting.clone(),
            admitted: self.admitted,
        }
    }

    /// The same scope, seen from where an expression of this file starts.
    fn from(&self, expression: &syn::Expr) -> Evaluation<'a> {
        self.at(self.syntax.offset(expression.span()))
    }

    fn of(&self, expression: &syn::Expr) -> Value {
        match expression {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(text),
                ..
            }) => Value::Text(text.value()),
            syn::Expr::Reference(reference) => self.of(&reference.expr),
            syn::Expr::Paren(inner) => self.of(&inner.expr),
            syn::Expr::Group(inner) => self.of(&inner.expr),
            syn::Expr::Try(inner) => self.of(&inner.expr),
            syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
                self.of(&unary.expr)
            }
            syn::Expr::Macro(mac) => self.of_macro(&mac.mac),
            // `<Type as Trait>::NAME`: the type's associated constant.
            syn::Expr::Path(path) if path.qself.is_some() => {
                let owner = path.qself.as_ref().and_then(|qself| match &*qself.ty {
                    syn::Type::Path(owner) => owner
                        .path
                        .segments
                        .last()
                        .map(|last| last.ident.to_string()),
                    // `<[u8]>::len`, `<dyn Trait>::f`: no type this reading
                    // names.
                    _ => None,
                });
                let trait_named: Vec<String> = path
                    .path
                    .segments
                    .iter()
                    .take(path.qself.as_ref().map_or(0, |qself| qself.position))
                    .map(|segment| segment.ident.to_string())
                    .collect();
                let named = (!trait_named.is_empty()).then_some(trait_named.as_slice());
                match (owner, path.path.segments.last()) {
                    (Some(owner), Some(name)) => {
                        let written =
                            format!("<{owner} as {}>::{}", trait_named.join("::"), name.ident);
                        match self.associated(self.file.0, &owner, &name.ident.to_string(), named) {
                            Resolved::Found(found) => Value::several(
                                found
                                    .iter()
                                    .map(|(scope, constant)| scope.of(&constant.value))
                                    .collect(),
                            ),
                            Resolved::Missing(Value::Unreduced(why)) => {
                                Value::Unreduced(why.written(&written))
                            }
                            Resolved::Missing(value) => value,
                        }
                    }
                    _ => Value::Opaque,
                }
            }
            syn::Expr::Path(path) => {
                let segments: Vec<String> = path
                    .path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect();
                match segments.as_slice() {
                    [name] => self.named(name),
                    _ => self.constant(&segments),
                }
            }
            syn::Expr::MethodCall(call) => self.of_method(call),
            syn::Expr::Call(call) => self.of_call(call),
            syn::Expr::Block(block) => self.of_block(&block.block),
            syn::Expr::Unsafe(block) => self.of_block(&block.block),
            syn::Expr::If(branch) => {
                let mut values = vec![self.of_block(&branch.then_branch)];
                if let Some((_, otherwise)) = &branch.else_branch {
                    values.push(self.from(otherwise).of(otherwise));
                }
                Value::several(values)
            }
            syn::Expr::Match(matched) => Value::several(
                matched
                    .arms
                    .iter()
                    .map(|arm| self.from(&arm.body).of(&arm.body))
                    .collect(),
            ),
            syn::Expr::Index(indexed) => match self.of(&indexed.expr) {
                Value::Within(values) => Value::several(values),
                value => value,
            },
            syn::Expr::Field(field) => {
                let step = match &field.member {
                    syn::Member::Named(name) => Step::Field(name.to_string()),
                    syn::Member::Unnamed(position) => Step::Index(position.index as usize),
                };
                self.member(&field.base, step)
            }
            syn::Expr::Tuple(tuple) => self.composite(tuple.elems.iter()),
            syn::Expr::Array(array) => self.composite(array.elems.iter()),
            syn::Expr::Struct(record) => {
                self.composite(record.fields.iter().map(|field| &field.expr))
            }
            // Not a path value: a closure, a number, a comparison, an
            // arithmetic, a range, a cast, a loop, an assignment.
            _ => Value::Opaque,
        }
    }

    /// A field of a value: of a literal written here, or of one a name in
    /// scope is bound to. A field of a value from elsewhere — a parameter,
    /// `self`, a call's result — is the unseen base, unless that value is
    /// itself rooted or unreduced.
    fn member(&self, base: &syn::Expr, step: Step) -> Value {
        if let Some(part) = projected(base, std::slice::from_ref(&step)) {
            return self.of(part);
        }
        if let syn::Expr::Path(path) = base
            && let Some(name) = path.path.get_ident()
            && !self.parameters.contains_key(&name.to_string())
            && let Some(binding) = self.binding(&name.to_string(), true)
        {
            let scope = self.at(binding.offset);
            let mut steps = binding.projection.clone();
            steps.push(step);
            if !binding.each {
                return scope.part(&binding.value, &steps);
            }
            return match scope.elements(&binding.value) {
                Some(elements) => Value::several(
                    elements
                        .iter()
                        .map(|element| scope.part(element, &steps))
                        .collect(),
                ),
                None => scope.of(&binding.value).unreachable_part(),
            };
        }
        self.of(base).unreachable_part()
    }

    /// A tuple, array or struct literal: a value that names the paths among
    /// its parts, or nothing.
    fn composite<'e>(&self, parts: impl Iterator<Item = &'e syn::Expr>) -> Value {
        let paths: Vec<Value> = parts
            .map(|part| self.of(part))
            .filter(Value::names_a_path)
            .collect();
        if paths.is_empty() {
            // Numbers, flags, text: no path among the parts.
            Value::Opaque
        } else {
            Value::Within(paths)
        }
    }

    /// A block's value: the expression it ends with, seen from inside it.
    fn of_block(&self, block: &syn::Block) -> Value {
        match block.stmts.last() {
            Some(syn::Stmt::Expr(tail, None)) => self.from(tail).of(tail),
            // A block that ends in a statement is `()`.
            _ => Value::Opaque,
        }
    }

    fn of_method(&self, call: &syn::ExprMethodCall) -> Value {
        let receiver = || self.of(&call.receiver);
        let argument = || {
            call.args
                .first()
                .map_or(Value::Opaque, |argument| self.of(argument))
        };
        match call.method.to_string().as_str() {
            "join" => receiver().join(argument()),
            "parent" => receiver().parent(),
            "with_file_name" => receiver().parent().join(argument()),
            "with_extension" => receiver().with_extension(argument()),
            // `dir.parent().and_then(Path::parent)`: the parent's parent.
            "and_then" | "map"
                if call.args.len() == 1
                    && matches!(&call.args[0], syn::Expr::Path(path)
                        if path.path.segments.last().is_some_and(|last| last.ident == "parent")) =>
            {
                receiver().parent()
            }
            // A path spelled for `format!` is the path's own text.
            "to_path_buf" | "as_path" | "clone" | "to_owned" | "into" | "as_ref" | "unwrap"
            | "expect" | "borrow" | "as_os_str" | "display" | "to_string_lossy" | "to_str"
            | "to_string" | "into_owned" => receiver(),
            // Any other method is a call of its own, judged by what it is
            // handed (`read_at_run_time`); what it returns is not a path this
            // reading can follow, and the paths it was handed are not lost.
            // The path-describing ones (`file_name`, `extension`,
            // `starts_with`, `components`…) answer a name, a flag or an
            // iterator, not a path.
            _ => Value::Opaque,
        }
    }

    fn of_call(&self, call: &syn::ExprCall) -> Value {
        let syn::Expr::Path(function) = &*call.func else {
            // A call of a value: a call of its own, judged by what it is
            // handed.
            return Value::Opaque;
        };
        let segments: Vec<String> = function
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        let last = segments.last().map_or("", String::as_str);
        let names_the_manifest_directory = call.args.iter().any(|argument| {
            matches!(argument, syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(text), .. })
                if text.value() == MANIFEST_DIRECTORY)
        });
        if names_the_manifest_directory {
            return Value::package();
        }
        if matches!(last, "temp_dir" | "temp_path") || segments.iter().any(|s| s == "tempfile") {
            return Value::Path {
                base: Base::Scratch,
                segments: Vec::new(),
                open: false,
            };
        }
        // A conversion, or a wrapper that holds its argument as it is.
        let wraps = matches!(last, "Some" | "Ok")
            || matches!(
                segments.as_slice(),
                [.., owner, new] if new == "new" && matches!(owner.as_str(), "Box" | "Rc" | "Arc")
            );
        if converts(&segments) || wraps {
            return call
                .args
                .first()
                .map_or(Value::Opaque, |argument| self.of(argument));
        }
        if segments.len() == 1 {
            return self.helper(last, call);
        }
        // Another function: a call of its own, judged by what it is handed.
        Value::Opaque
    }

    fn of_macro(&self, mac: &syn::Macro) -> Value {
        if let Some(element) = repeated(mac) {
            return self.composite(std::iter::once(&element));
        }
        let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        let Ok(arguments) = mac.parse_body_with(parser) else {
            // A pattern or bare tokens, not values (`matches!(x, Some(_))`,
            // a `macro_rules!` body): nothing a path is handed to.
            return Value::Opaque;
        };
        let arguments: Vec<syn::Expr> = arguments.into_iter().collect();
        let name = mac
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string());
        match name.as_deref() {
            Some("env") => match arguments.first() {
                Some(syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(text),
                    ..
                })) if text.value() == MANIFEST_DIRECTORY => Value::package(),
                // Another variable's value at build time: a string of the
                // build's, not a path into the checkout.
                _ => Value::Words,
            },
            // A file's contents, not its path.
            Some("include_str" | "include_bytes") => Value::Words,
            Some("concat") => splice(arguments.iter().map(|argument| self.of(argument)).collect()),
            Some("format") => match arguments.split_first() {
                Some((
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(template),
                        ..
                    }),
                    rest,
                )) => splice(self.format_pieces(&template.value(), rest)),
                _ => Value::Opaque,
            },
            // A macro this reading does not expand: a path handed to it is
            // unreduced; with none, nothing.
            _ => {
                let values: Vec<Value> =
                    arguments.iter().map(|argument| self.of(argument)).collect();
                match values.iter().find_map(Value::unreduced) {
                    Some(why) => Value::Unreduced(why),
                    None if values.iter().any(Value::holds_root) => Value::Unreduced(Why::Computed),
                    None => Value::Opaque,
                }
            }
        }
    }

    /// A format string's pieces: its text, and what each placeholder puts in
    /// it. Placeholders take their arguments as `std` does: an implicit one
    /// (`{}`, `{:?}`) the next positional argument, `{0}` and `{name}` (a
    /// named argument or a captured name) the one they name, and a `.*`
    /// precision one more positional argument first. A placeholder with a
    /// spec is not the value's own text: it is words, or a string that names
    /// the path it renders.
    fn format_pieces(&self, template: &str, arguments: &[syn::Expr]) -> Vec<Value> {
        let mut positional = Vec::new();
        let mut named = BTreeMap::new();
        for argument in arguments {
            match argument {
                syn::Expr::Assign(assign) => {
                    if let syn::Expr::Path(path) = &*assign.left
                        && let Some(name) = path.path.get_ident()
                    {
                        named.insert(name.to_string(), &*assign.right);
                    }
                }
                other => positional.push(other),
            }
        }
        let mut pieces = Vec::new();
        let mut text = String::new();
        let mut next = 0usize;
        let mut characters = template.chars().peekable();
        while let Some(character) = characters.next() {
            match character {
                '{' if characters.peek() == Some(&'{') => {
                    characters.next();
                    text.push('{');
                }
                '}' if characters.peek() == Some(&'}') => {
                    characters.next();
                    text.push('}');
                }
                '{' => {
                    let hole: String = characters.by_ref().take_while(|c| *c != '}').collect();
                    let (reference, spec) = hole.split_once(':').unwrap_or((&hole, ""));
                    if spec.contains(".*") {
                        next += 1;
                    }
                    let value = if reference.is_empty() {
                        next += 1;
                        positional
                            .get(next - 1)
                            .map_or(Value::Opaque, |argument| self.of(argument))
                    } else if let Ok(at) = reference.parse::<usize>() {
                        positional
                            .get(at)
                            .map_or(Value::Opaque, |argument| self.of(argument))
                    } else if let Some(argument) = named.get(reference) {
                        self.of(argument)
                    } else {
                        self.named(reference)
                    };
                    let value = if spec.is_empty() {
                        value
                    } else if value.names_a_path() {
                        Value::Within(vec![value])
                    } else {
                        Value::Words
                    };
                    pieces.push(Value::Text(std::mem::take(&mut text)));
                    pieces.push(value);
                }
                other => text.push(other),
            }
        }
        pieces.push(Value::Text(text));
        pieces
    }

    /// The innermost binding of a name in scope here: in this function, with
    /// a scope that holds this point; the one whose scope starts last wins.
    fn binding(&self, name: &str, loops: bool) -> Option<&Binding> {
        self.syntax
            .bindings
            .iter()
            .filter(|binding| {
                binding.function == self.function
                    && binding.scope.0 <= self.position
                    && self.position < binding.scope.1
                    && binding.name == name
                    && (loops || !binding.each)
            })
            .max_by_key(|binding| binding.scope.0)
    }

    /// A single name: a helper's parameter, then the innermost binding of it
    /// in scope here, then a constant.
    fn named(&self, name: &str) -> Value {
        if let Some(value) = self.parameters.get(name) {
            return value.clone();
        }
        let Some(binding) = self.binding(name, true) else {
            return self.constant(&[name.to_owned()]);
        };
        let scope = self.at(binding.offset);
        if !binding.each {
            return scope.part(&binding.value, &binding.projection);
        }
        match scope.elements(&binding.value) {
            Some(elements) => Value::several(
                elements
                    .iter()
                    .map(|element| scope.part(element, &binding.projection))
                    .collect(),
            ),
            // A loop over a list this reading cannot read: the list is a
            // value of its own, handed to whatever made it; a rooted one is
            // unreduced.
            None => scope.of(&binding.value).unreachable_part(),
        }
    }

    /// The part of a value a pattern's positions pick. One not written as
    /// tuples is unreduced if it holds a rooted path, and nothing otherwise.
    fn part(&self, value: &syn::Expr, positions: &[Step]) -> Value {
        match projected(value, positions) {
            Some(part) => self.of(part),
            None => self.of(value).unreachable_part(),
        }
    }

    /// The elements of a literal list a loop iterates: an array, a reference
    /// to one, `vec![…]`, `.iter()` and its kin over one, or a name or a
    /// constant bound to one. `None` for anything else.
    fn elements(&self, expression: &syn::Expr) -> Option<Vec<syn::Expr>> {
        match expression {
            syn::Expr::Array(array) => Some(array.elems.iter().cloned().collect()),
            syn::Expr::Reference(reference) => self.elements(&reference.expr),
            syn::Expr::Paren(inner) => self.elements(&inner.expr),
            syn::Expr::Group(inner) => self.elements(&inner.expr),
            syn::Expr::Macro(mac) if mac.mac.path.is_ident("vec") => {
                if let Some(element) = repeated(&mac.mac) {
                    return Some(vec![element]);
                }
                let parser =
                    syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
                mac.mac
                    .parse_body_with(parser)
                    .ok()
                    .map(|elements| elements.into_iter().collect())
            }
            syn::Expr::MethodCall(call)
                if call.args.is_empty()
                    && matches!(
                        call.method.to_string().as_str(),
                        "iter" | "into_iter" | "copied" | "cloned"
                    ) =>
            {
                self.elements(&call.receiver)
            }
            syn::Expr::Path(path) => {
                let segments: Vec<String> = path
                    .path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect();
                if let [name] = segments.as_slice()
                    && let Some(binding) = self.binding(name, false)
                    && binding.projection.is_empty()
                {
                    return self.at(binding.offset).elements(&binding.value);
                }
                match self.resolve(&segments) {
                    Resolved::Found(found) => {
                        let lists: Vec<Vec<syn::Expr>> = found
                            .iter()
                            .filter_map(|(scope, constant)| scope.elements(&constant.value))
                            .collect();
                        (!lists.is_empty()).then(|| lists.into_iter().flatten().collect())
                    }
                    Resolved::Missing(_) => None,
                }
            }
            _ => None,
        }
    }

    /// A constant's value, followed as far as its chain goes; several
    /// constants of one name are each followed.
    fn constant(&self, segments: &[String]) -> Value {
        match self.resolve(segments) {
            Resolved::Found(found) => Value::several(
                found
                    .iter()
                    .map(|(scope, constant)| scope.of(&constant.value))
                    .collect(),
            ),
            Resolved::Missing(value) => value,
        }
    }

    /// The constants a path names, as Rust resolves a path written here:
    /// - a bare name: the module this is written in's constant of that
    ///   name, then what that module brings in by `use` and `use …::*`;
    /// - `Self::NAME`: the constants the `impl` blocks of the type this is
    ///   written in declare;
    /// - a longer path: from a first-party crate's name, `self`, `super`,
    ///   `crate` or a child module, through bt-source's module tree, to the
    ///   module's constant (or its `use`s and globs), or — where the path
    ///   ends at a type of this workspace — that type's associated constant.
    ///
    /// A path through another crate's items or a primitive (`u32::MAX`,
    /// `std::path::MAIN_SEPARATOR`) is never rooted in this checkout. A
    /// constant-spelled name this reading cannot place is unreduced; any
    /// other name it cannot place is a function, a type or a variant.
    fn resolve(&self, segments: &[String]) -> Resolved<'a> {
        let Some((name, modules)) = segments.split_last() else {
            return Resolved::Missing(Value::Opaque);
        };
        let resolved = match modules {
            // A bare name is what the module it is written in holds: its own
            // constant, else its `use`s and globs. A file no module tree
            // places has only its own constants of the name.
            [] => match self.module.clone() {
                Some(module) => {
                    let viewer = module.clone();
                    self.constant_in(module, name, &mut BTreeSet::new(), Some(&viewer))
                }
                None => {
                    let here: Vec<(FileId, Rc<Constant>)> = self
                        .syntax
                        .constants
                        .get(name)
                        .into_iter()
                        .flatten()
                        .map(|constant| (self.file, Rc::clone(constant)))
                        .collect();
                    self.found(name, &here)
                }
            },
            [owner] if owner == "Self" => match self.world.impl_type_at(self.file, self.position) {
                Some(owner) => self.associated(self.file.0, &owner, name, None),
                None => Resolved::Missing(Value::Unreduced(Why::elsewhere())),
            },
            _ => self.locate(
                self.module.clone(),
                modules,
                name,
                &mut BTreeSet::new(),
                self.module.as_ref(),
            ),
        };
        match resolved {
            Resolved::Missing(Value::Unreduced(Why::Elsewhere(_))) if !names_a_constant(name) => {
                Resolved::Missing(Value::Opaque)
            }
            Resolved::Missing(Value::Unreduced(why)) => {
                Resolved::Missing(Value::Unreduced(why.written(&segments.join("::"))))
            }
            other => other,
        }
    }

    /// The constants found, each entered on the resolution path.
    fn found(&self, name: &str, constants: &[(FileId, Rc<Constant>)]) -> Resolved<'a> {
        let mut found = Vec::new();
        if constants.is_empty() {
            return Resolved::Missing(Value::Unreduced(Why::elsewhere()));
        }
        for (number, (file, constant)) in constants.iter().enumerate() {
            let syntax = if *file == self.file {
                Rc::clone(&self.syntax)
            } else {
                self.world.syntax(*file)
            };
            let module = self
                .world
                .placed
                .get(&(*file, constant.inline.clone()))
                .cloned();
            let identity = format!(
                "const {file:?}:{}::{name}#{number}",
                constant.inline.join("::")
            );
            let place = Place {
                syntax,
                file: *file,
                module,
            };
            match self.entering(identity, place, usize::MAX, 0, BTreeMap::new()) {
                Some(scope) => found.push((scope, Rc::clone(constant))),
                None => return Resolved::Missing(Value::Unreduced(Why::cycle())),
            }
        }
        Resolved::Found(found)
    }

    /// A constant a path of module segments leads to from a module, named
    /// from `viewer` (which only tells same-named candidates apart).
    fn locate(
        &self,
        from: Option<ModuleKey>,
        modules: &[String],
        name: &str,
        seen: &mut BTreeSet<(ModuleKey, String)>,
        viewer: Option<&ModuleKey>,
    ) -> Resolved<'a> {
        match self.world.walk(from.as_ref(), modules) {
            Walk::Module(key) => self.constant_in(key, name, seen, viewer),
            Walk::Item { module, at } if at + 1 == modules.len() => {
                let Some(((number, _), _)) = self
                    .world
                    .modules
                    .get(&module)
                    .and_then(|placed| placed.first())
                else {
                    return Resolved::Missing(Value::Unreduced(Why::elsewhere()));
                };
                self.associated(*number, &modules[at], name, None)
            }
            // A path through an item further down, or through another crate.
            Walk::Item { .. } => Resolved::Missing(Value::Opaque),
            Walk::Lost => Resolved::Missing(Value::Unreduced(Why::elsewhere())),
        }
    }

    /// The constant the `impl` blocks of a type offer under a name, as Rust
    /// picks it: with a trait named (`<Type as Trait>::NAME`), the block
    /// whose trait is that trait; with none, the inherent block, else the one
    /// trait that offers it. A block offers what it declares, and a
    /// first-party trait's default for a constant it does not declare.
    /// Several traits offering it, a trait path that cannot be followed, or
    /// a block whose trait cannot be followed beside the one named, is
    /// ambiguous: unreduced. A type this workspace does not declare is
    /// another crate's or a primitive, never rooted in the checkout.
    fn associated(
        &self,
        number: usize,
        owner: &str,
        name: &str,
        trait_named: Option<&[String]>,
    ) -> Resolved<'a> {
        if !self.world.declares_type(number, owner) {
            return Resolved::Missing(Value::Opaque);
        }
        let mut offered: Vec<(Implemented, Offered)> = Vec::new();
        for (file, block) in self.world.impl_blocks(number, owner) {
            let module = self
                .world
                .placed
                .get(&(file, block.inline.clone()))
                .cloned();
            let implemented = match &block.implemented {
                None => Implemented::Inherent,
                Some(path) => {
                    match self.trait_identity(module.as_ref(), path, &mut BTreeSet::new()) {
                        Some(identity) => Implemented::Trait(identity),
                        None => Implemented::Unfollowed,
                    }
                }
            };
            let mut constants: Vec<(FileId, Rc<Constant>)> = block
                .constants
                .get(name)
                .into_iter()
                .flatten()
                .map(|constant| (file, Rc::clone(constant)))
                .collect();
            if constants.is_empty()
                && let Implemented::Trait(TraitIdentity::FirstParty(key, declared)) = &implemented
            {
                constants = self.default_of(key, declared, name);
            }
            if !constants.is_empty() || implemented == Implemented::Unfollowed {
                offered.push((implemented, constants));
            }
        }
        let chosen: Vec<(FileId, Rc<Constant>)> = match trait_named {
            Some(path) => {
                let Some(wanted) =
                    self.trait_identity(self.module.as_ref(), path, &mut BTreeSet::new())
                else {
                    return Resolved::Missing(Value::Unreduced(Why::elsewhere()));
                };
                let wanted = Implemented::Trait(wanted);
                let exact: Vec<(FileId, Rc<Constant>)> = offered
                    .iter()
                    .filter(|(implemented, _)| *implemented == wanted)
                    .flat_map(|(_, constants)| constants.iter().cloned())
                    .collect();
                if exact.is_empty()
                    && offered
                        .iter()
                        .any(|(implemented, _)| *implemented == Implemented::Unfollowed)
                {
                    return Resolved::Missing(Value::Unreduced(Why::elsewhere()));
                }
                exact
            }
            None => {
                let inherent: Vec<(FileId, Rc<Constant>)> = offered
                    .iter()
                    .filter(|(implemented, _)| *implemented == Implemented::Inherent)
                    .flat_map(|(_, constants)| constants.iter().cloned())
                    .collect();
                if inherent.is_empty() {
                    let traits = offered
                        .iter()
                        .filter(|(_, constants)| !constants.is_empty())
                        .map(|(implemented, _)| implemented)
                        .collect::<BTreeSet<_>>();
                    let unfollowed = offered
                        .iter()
                        .any(|(implemented, _)| *implemented == Implemented::Unfollowed);
                    if traits.len() > 1 || unfollowed {
                        return Resolved::Missing(Value::Unreduced(Why::elsewhere()));
                    }
                    offered
                        .iter()
                        .flat_map(|(_, constants)| constants.iter().cloned())
                        .collect()
                } else {
                    inherent
                }
            }
        };
        self.found(name, &chosen)
    }

    /// The default a first-party trait gives a constant, in every file its
    /// module is written in.
    fn default_of(
        &self,
        key: &ModuleKey,
        declared: &str,
        name: &str,
    ) -> Vec<(FileId, Rc<Constant>)> {
        self.world
            .modules
            .get(key)
            .into_iter()
            .flatten()
            .filter_map(|(file, inline)| {
                self.world
                    .syntax(*file)
                    .defaults
                    .get(&(inline.clone(), declared.to_owned(), name.to_owned()))
                    .map(|constant| (*file, Rc::clone(constant)))
            })
            .collect()
    }

    /// Which trait a trait path written in a module names, as Rust resolves
    /// it: a trait a first-party module declares, reached through the module
    /// trees, `use`s (an alias is followed to what it names) and globs; else
    /// another crate's trait, identified by its full path as written once
    /// the first-party `use`s it goes through are followed (`use dep::Trait
    /// as Alias` makes `Alias` `dep::Trait`; `a::Same` is not `b::Same`); a
    /// bare name no module holds and no glob can bring is the prelude's. A
    /// path that cannot be followed (a name a glob of another crate's module
    /// may bring in, a `super` past the root) is `None`.
    fn trait_identity(
        &self,
        from: Option<&ModuleKey>,
        path: &[String],
        seen: &mut BTreeSet<(Option<ModuleKey>, Vec<String>)>,
    ) -> Option<TraitIdentity> {
        if !seen.insert((from.cloned(), path.to_vec())) {
            return None;
        }
        let (name, modules) = path.split_last()?;
        let Some(from) = from else {
            return Some(TraitIdentity::Dependency(path.join("::")));
        };
        match self.world.walk(Some(from), modules) {
            Walk::Module(key) => self.trait_held(&key, name, seen),
            Walk::Item { module, at } => {
                if let Some(used) = self.use_of(&module, &path[at]) {
                    let mut followed = used;
                    followed.extend(path[at + 1..].iter().cloned());
                    return self.trait_identity(Some(&module), &followed, seen);
                }
                let special = matches!(path[0].as_str(), "crate" | "self" | "super")
                    || self.world.crates.contains_key(&path[0]);
                // Another crate's path (`dep::Trait`, `std::fmt::Display`);
                // a path through a first-party type is no trait.
                (at == 0 && !special).then(|| TraitIdentity::Dependency(path.join("::")))
            }
            Walk::Lost => None,
        }
    }

    /// The path a `use` in a module binds a name to, in any file it is
    /// written in.
    fn use_of(&self, key: &ModuleKey, name: &str) -> Option<Vec<String>> {
        self.world
            .modules
            .get(key)
            .into_iter()
            .flatten()
            .find_map(|(file, inline)| {
                self.world
                    .syntax(*file)
                    .uses
                    .get(&(inline.clone(), name.to_owned()))
                    .map(|(path, _)| path.clone())
            })
    }

    /// The trait a module holds under a name, a bare name no module holds
    /// and no glob can bring in being the prelude's.
    fn trait_held(
        &self,
        key: &ModuleKey,
        name: &str,
        seen: &mut BTreeSet<(Option<ModuleKey>, Vec<String>)>,
    ) -> Option<TraitIdentity> {
        self.trait_held_in(key, name, seen)
            .unwrap_or_else(|| Some(TraitIdentity::Dependency(name.to_owned())))
    }

    /// The trait a module holds under a name: declared there, brought there
    /// by a `use` (followed), or by a glob of a first-party module that holds
    /// it — `Some(None)` where a glob of another crate's module may bring it
    /// in (it cannot be followed), `None` where nothing brings it in.
    fn trait_held_in(
        &self,
        key: &ModuleKey,
        name: &str,
        seen: &mut BTreeSet<(Option<ModuleKey>, Vec<String>)>,
    ) -> Option<Option<TraitIdentity>> {
        // Globs may lead back to a module already asked.
        if !seen.insert((Some(key.clone()), vec!["*".to_owned(), name.to_owned()])) {
            return None;
        }
        let mut beyond = false;
        for (file, inline) in self.world.modules.get(key).cloned().into_iter().flatten() {
            let syntax = self.world.syntax(file);
            let here = (inline.clone(), name.to_owned());
            if syntax.traits.contains(&here) {
                return Some(Some(TraitIdentity::FirstParty(
                    key.clone(),
                    name.to_owned(),
                )));
            }
            if let Some((path, _)) = syntax.uses.get(&here) {
                return Some(self.trait_identity(Some(key), path, seen));
            }
            for (glob, _) in syntax.globs.get(&inline).into_iter().flatten() {
                match self.world.walk(Some(key), glob) {
                    Walk::Module(next) => {
                        if let Some(held) = self.trait_held_in(&next, name, seen) {
                            return Some(held);
                        }
                    }
                    Walk::Item { .. } | Walk::Lost => beyond = true,
                }
            }
        }
        beyond.then_some(None)
    }

    /// The constant a module holds under a name: written there, brought
    /// there by a `use` or a `use …::*`, or declared there by
    /// `thread_local!` (which is never a path). One it holds by none of
    /// these is unreduced.
    fn constant_in(
        &self,
        key: ModuleKey,
        name: &str,
        seen: &mut BTreeSet<(ModuleKey, String)>,
        viewer: Option<&ModuleKey>,
    ) -> Resolved<'a> {
        // Glob imports may lead back to a module already asked (`use
        // super::*` in a child, `pub use child::*` in its parent): the name is
        // not held along that way.
        if !seen.insert((key.clone(), name.to_owned())) {
            return Resolved::Missing(Value::Unreduced(Why::elsewhere()));
        }
        let Some(placements) = self.world.modules.get(&key).cloned() else {
            return Resolved::Missing(Value::Unreduced(Why::elsewhere()));
        };
        // Each file the module is written in answers for its own `cfg`; what
        // any of them holds is held.
        let mut found = Vec::new();
        let mut missing = None;
        for (file, inline) in placements {
            match self.held_in(&key, file, &inline, name, seen, viewer) {
                Resolved::Found(more) => found.extend(more),
                Resolved::Missing(Value::Unreduced(Why::Elsewhere(_))) => {}
                Resolved::Missing(value) => missing = missing.or(Some(value)),
            }
        }
        if !found.is_empty() {
            Resolved::Found(found)
        } else {
            Resolved::Missing(missing.unwrap_or(Value::Unreduced(Why::elsewhere())))
        }
    }

    /// What one file of a module holds under a name: a constant written
    /// there, or a `use` or glob followed from the module that holds it. The
    /// tree compiles, so a path the reading code names is one Rust let it
    /// name; `viewer` — the module the path is written in, carried through
    /// every re-export — only tells same-named candidates apart (see
    /// [`Evaluation::disambiguated`]).
    fn held_in(
        &self,
        key: &ModuleKey,
        file: FileId,
        inline: &[String],
        name: &str,
        seen: &mut BTreeSet<(ModuleKey, String)>,
        viewer: Option<&ModuleKey>,
    ) -> Resolved<'a> {
        let syntax = if file == self.file {
            Rc::clone(&self.syntax)
        } else {
            self.world.syntax(file)
        };
        let here = (inline.to_vec(), name.to_owned());
        if let Some(constants) = syntax.scoped.get(&here) {
            let constants: Vec<(FileId, Rc<Constant>)> = constants
                .iter()
                .map(|constant| (file, Rc::clone(constant)))
                .collect();
            return self.found(name, &constants);
        }
        if syntax.locals.contains(&here) {
            return Resolved::Missing(Value::Opaque);
        }
        if let Some((path, visibility)) = syntax.uses.get(&here) {
            let Some((last, modules)) = path.split_last() else {
                return Resolved::Missing(Value::Unreduced(Why::elsewhere()));
            };
            return through(
                self.locate(Some(key.clone()), modules, last, seen, viewer),
                visibility.admits(key, viewer),
            );
        }
        // Every glob that brings the name in is a candidate; the viewer
        // tells them apart.
        let mut beyond = false;
        let mut candidates = Vec::new();
        let mut missing = None;
        for (glob, visibility) in syntax.globs.get(inline).into_iter().flatten() {
            match self.world.walk(Some(key), glob) {
                Walk::Module(next) => match through(
                    self.constant_in(next, name, seen, viewer),
                    visibility.admits(key, viewer),
                ) {
                    Resolved::Missing(Value::Unreduced(Why::Elsewhere(_))) => {}
                    Resolved::Found(found) => candidates.push(found),
                    Resolved::Missing(value) => missing = missing.or(Some(value)),
                },
                Walk::Item { .. } | Walk::Lost => beyond = true,
            }
        }
        if !candidates.is_empty() {
            return self.disambiguated(candidates, viewer);
        }
        if let Some(value) = missing {
            return Resolved::Missing(value);
        }
        // A glob of another crate's module (`use windows::Win32::…::*`) may
        // bring the name in from outside the workspace, which is never a path
        // into this checkout.
        if beyond {
            Resolved::Missing(Value::Opaque)
        } else {
            Resolved::Missing(Value::Unreduced(Why::elsewhere()))
        }
    }

    /// Several candidates for one name, each what one way of bringing it in
    /// leads to: the one candidate, if there is one (an item invisible to
    /// the viewer is still it — the tree compiles, so the code could not
    /// name it otherwise); else the one visible to the viewer — every `use`
    /// on its way admits the viewer, and so does the constant; several
    /// visible, or none, is ambiguous: unreduced. Ways that lead to the
    /// same constants are one candidate.
    fn disambiguated(
        &self,
        candidates: Vec<Vec<(Evaluation<'a>, Rc<Constant>)>>,
        viewer: Option<&ModuleKey>,
    ) -> Resolved<'a> {
        let mut distinct: Vec<Vec<(Evaluation<'a>, Rc<Constant>)>> = Vec::new();
        for candidate in candidates {
            let same = |other: &Vec<(Evaluation<'a>, Rc<Constant>)>| {
                other.len() == candidate.len()
                    && other
                        .iter()
                        .zip(&candidate)
                        .all(|((_, left), (_, right))| Rc::ptr_eq(left, right))
            };
            if !distinct.iter().any(same) {
                distinct.push(candidate);
            }
        }
        if distinct.len() == 1 {
            return Resolved::Found(distinct.remove(0));
        }
        let mut visible: Vec<Vec<(Evaluation<'a>, Rc<Constant>)>> = distinct
            .into_iter()
            .filter(|candidate| {
                candidate.iter().any(|(scope, constant)| {
                    scope.admitted
                        && scope
                            .module
                            .as_ref()
                            .is_none_or(|owner| constant.visibility.admits(owner, viewer))
                })
            })
            .collect();
        if visible.len() == 1 {
            Resolved::Found(visible.remove(0))
        } else {
            Resolved::Missing(Value::Unreduced(Why::elsewhere()))
        }
    }

    /// A call of the file's own free function: what it returns, with its
    /// parameters bound to what this call hands it. Several functions of the
    /// name (under exclusive `cfg`s) are each followed.
    fn helper(&self, name: &str, call: &syn::ExprCall) -> Value {
        let Some(helpers) = self.syntax.helpers.get(name) else {
            // Another file's or crate's function: a call of its own, judged
            // by what it is handed.
            return Value::Opaque;
        };
        let mut values = Vec::new();
        for (number, helper) in helpers.iter().enumerate() {
            let Some(tail) = &helper.tail else {
                // A function that returns nothing a path can be read from.
                continue;
            };
            let parameters = helper
                .parameters
                .iter()
                .zip(&call.args)
                .filter_map(|(parameter, argument)| {
                    parameter
                        .as_ref()
                        .map(|parameter| (parameter.clone(), self.of(argument)))
                })
                .collect();
            // The helper's body is read in the module it is written in.
            let place = Place {
                syntax: Rc::clone(&self.syntax),
                file: self.file,
                module: self.world.module_at(self.file, helper.function),
            };
            values.push(
                match self.entering(
                    format!("fn {:?}:{name}#{number}", self.file),
                    place,
                    helper.function,
                    helper.tail_at,
                    parameters,
                ) {
                    Some(scope) => scope.of(tail),
                    // A function that calls itself: what it returns comes from
                    // the branches that do not, which are followed; the
                    // recursion adds no value of its own.
                    None => Value::Opaque,
                },
            );
        }
        if values.is_empty() {
            // Functions of the name that return `()`: no value.
            Value::Opaque
        } else {
            Value::several(values)
        }
    }
}

/// Pieces written one after another — `concat!`'s arguments, a format
/// string's text and placeholders — as one value. A piece this reading cannot
/// reduce anywhere among pieces that hold a rooted path makes the whole
/// unreduced: never a silent drop, and no later piece is discarded.
/// Otherwise texts join into a text, text after a leading path continues the
/// path (past a separator or on its last segment), and a path after other
/// text makes a string that names the path. With no rooted piece and an
/// unreducible one, it is an ordinary string.
fn splice(pieces: Vec<Value>) -> Value {
    if let Some(at) = pieces
        .iter()
        .position(|piece| matches!(piece, Value::Each(_)))
    {
        let Value::Each(choices) = pieces[at].clone() else {
            unreachable!("found as several")
        };
        return Value::several(
            choices
                .into_iter()
                .map(|choice| {
                    let mut pieces = pieces.clone();
                    pieces[at] = choice;
                    splice(pieces)
                })
                .collect(),
        );
    }
    // Every rooted piece written whole is a path the string spells, judged
    // on its own; a piece this reading cannot reduce beside one is the
    // Computed diagnostic; a piece already unreduced keeps its cause. All of
    // them are kept: a value yields every outcome it holds.
    let unreducible = pieces.iter().any(Value::unreducible);
    let mut rooted: Vec<Value> = pieces
        .iter()
        .filter(|piece| piece.holds_root())
        .cloned()
        .collect();
    if unreducible && !rooted.is_empty() {
        rooted.push(Value::Unreduced(Why::Computed));
        return Value::Within(rooted);
    }
    if rooted.iter().any(|piece| piece.unreduced().is_some()) {
        return Value::Within(rooted);
    }
    if unreducible {
        // No rooted piece: an ordinary string.
        return Value::Opaque;
    }
    let mut spliced = Value::Text(String::new());
    for piece in pieces {
        spliced = match (spliced, piece) {
            (Value::Text(before), Value::Text(after)) => Value::Text(before + &after),
            (Value::Text(before), path @ Value::Path { .. }) if before.is_empty() => path,
            (Value::Text(_) | Value::Words, Value::Text(_) | Value::Words) => Value::Words,
            (Value::Text(_) | Value::Words, path @ Value::Path { .. }) => Value::Within(vec![path]),
            (Value::Text(_) | Value::Words, Value::Within(paths)) => Value::Within(paths),
            (Value::Within(mut paths), path @ Value::Path { .. }) => {
                paths.push(path);
                Value::Within(paths)
            }
            (Value::Within(mut paths), Value::Within(more)) => {
                paths.extend(more);
                Value::Within(paths)
            }
            (within @ Value::Within(_), Value::Text(_) | Value::Words) => within,
            (path @ Value::Path { .. }, Value::Text(after)) => continued(path, &after),
            // Unknown text right after a path: the path's own text is no
            // longer known.
            (Value::Path { base, segments, .. }, Value::Words) => Value::Path {
                base,
                segments,
                open: true,
            },
            // Two paths, or a path and a string naming paths, side by side.
            (first, second) => {
                if first.holds_root() || second.holds_root() {
                    Value::Unreduced(Why::Computed)
                } else {
                    // Scratch, absolute or relative text side by side: no
                    // path into the checkout.
                    Value::Opaque
                }
            }
        };
    }
    spliced
}

/// A path with text written straight after it: past a separator the text
/// is more segments; otherwise it lengthens the last one. A path already
/// open stays open, and text that lengthens no segment leaves it open.
fn continued(path: Value, after: &str) -> Value {
    let Value::Path {
        base,
        mut segments,
        open,
    } = path
    else {
        unreachable!("a path")
    };
    if after.is_empty() || open {
        return Value::Path {
            base,
            segments,
            open,
        };
    }
    if after.starts_with(['/', '\\']) {
        return Value::Path {
            base,
            segments,
            open,
        }
        .join(Value::Text(
            after.trim_start_matches(['/', '\\']).to_owned(),
        ));
    }
    let (first, rest) = after.split_once(['/', '\\']).unwrap_or((after, ""));
    match segments.last_mut() {
        Some(last) if last != ".." => last.push_str(first),
        _ => {
            return Value::Path {
                base,
                segments,
                open: true,
            };
        }
    }
    Value::Path {
        base,
        segments,
        open,
    }
    .join(Value::Text(rest.to_owned()))
}

/// What the scan found in `indices`: every reader of a committed document,
/// one per item, in file and item order, and every rooted read it could not
/// reduce.
fn document_readers(
    indices: &[(PathBuf, Index)],
    root: &Path,
    committed: &BTreeSet<String>,
) -> (Vec<DocumentReader>, Vec<UnresolvedRead>) {
    let mut by_item: BTreeMap<(String, BTreeSet<String>), DocumentReader> = BTreeMap::new();
    let mut unresolved = Vec::new();
    let world = World::of(indices);
    for (number, (package, index)) in indices.iter().enumerate() {
        let scan = DocumentScan {
            index,
            number,
            world: &world,
            root,
            package,
            committed,
        };
        let (read, unreduced) = scan.read_at_run_time();
        unresolved.extend(unreduced);
        for reader in scan.included().into_iter().chain(read) {
            match by_item.entry((reader.file.clone(), reader.owners.clone())) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(reader);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let held = entry.get_mut();
                    held.line = held.line.min(reader.line);
                    held.readings.extend(reader.readings);
                    held.subjects.extend(reader.subjects);
                }
            }
        }
    }
    unresolved.sort();
    // A call judged once per value a loop gives it is one entry.
    unresolved.dedup();
    (by_item.into_values().collect(), unresolved)
}

/// Every `(file, owner)` the two lists carry.
fn listed_readers(root: &Path) -> BTreeSet<(String, String)> {
    debt_rows(root)
        .into_iter()
        .chain(source_rows(root))
        .map(|row| (row["file"].clone(), row["owner"].clone()))
        .collect()
}

/// The document readers on neither list.
fn unlisted_documents<'a>(
    readers: &'a [DocumentReader],
    listed: &BTreeSet<(String, String)>,
) -> Vec<&'a DocumentReader> {
    readers
        .iter()
        .filter(|reader| {
            !reader
                .owners
                .iter()
                .any(|owner| listed.contains(&(reader.file.clone(), owner.clone())))
        })
        .collect()
}

/// The diagnostic, grouped by cause — a constant it cannot place, one that
/// reaches itself, or a computed piece handed to one callee — each cause one
/// line with its count and a few examples, under the exact total.
fn diagnostic_summary(unresolved: &[UnresolvedRead]) -> String {
    const EXAMPLES: usize = 3;
    let mut causes: BTreeMap<String, Vec<&UnresolvedRead>> = BTreeMap::new();
    for read in unresolved {
        let cause = match &read.why {
            Why::Computed => format!("{} — `{}`", read.why.said(), read.callee),
            other => other.said(),
        };
        causes.entry(cause).or_default().push(read);
    }
    let mut causes: Vec<(String, Vec<&UnresolvedRead>)> = causes.into_iter().collect();
    causes.sort_by(|left, right| right.1.len().cmp(&left.1.len()).then(left.0.cmp(&right.0)));
    let mut said = String::new();
    let _ = writeln!(
        said,
        "{} path(s) handed to a call this scan could not reduce to a committed file or rule out \
         are not judged (a diagnostic, not a refusal), in {} cause(s):",
        unresolved.len(),
        causes.len()
    );
    for (cause, reads) in &causes {
        let _ = writeln!(said, "  {} × {cause}", reads.len());
        for read in reads.iter().take(EXAMPLES) {
            let _ = writeln!(
                said,
                "      {}:{} {}",
                read.file,
                read.line,
                read.owners.iter().cloned().collect::<Vec<_>>().join(" / ")
            );
        }
        if reads.len() > EXAMPLES {
            let _ = writeln!(said, "      … and {} more", reads.len() - EXAMPLES);
        }
    }
    said
}

/// What the document half says when it fires: each item, its file, both
/// lists, and the diagnostic, grouped.
fn document_complaint(strays: &[&DocumentReader], unresolved: &[UnresolvedRead]) -> String {
    let mut said = String::new();
    let _ = writeln!(
        said,
        "{} reader(s) of a committed document are on neither list:\n",
        strays.len()
    );
    for stray in strays {
        let _ = writeln!(said, "    {}", stray.named());
    }
    let _ = write!(
        said,
        "\nEach needs a row keyed by its file and its item (the `owner` column) on one of the \
         two lists:\n\n  * {SOURCE_LIST} — a test whose subject really is the document's bytes: \
         file, owner, mechanism, subject, ticket, disturbed_by_2a and the reason.\n  * \
         {DEBT_LIST} — a reader waiting on a seam. It only shrinks: \
         scripts/ci/check-migration-debt.ps1 refuses an added row.\n\nA reader on both lists is \
         refused as well.\n\n{}",
        diagnostic_summary(unresolved)
    );
    said
}

/// RED (T-SOURCE-READERS-TSV-TRIPWIRE) — **every item of test or
/// build-support code that reads a committed document is a row of one of the
/// two lists.**
///
/// MUTATION: add `#[test] fn planted() { std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plans/structural-debt.md")).unwrap(); }`
/// to any crate's tests, or a `const` that `include_str!`s a committed `.tsv`,
/// and this names the item, its file and both lists.
/// `a_planted_document_reader_on_neither_list_makes_the_tripwire_fire` is the
/// same mutation on a tree of its own.
#[test]
fn every_reader_of_a_committed_document_is_on_a_list() {
    let root = workspace_root();
    let committed = committed_files(&root);
    let documents = committed
        .iter()
        .filter(|path| is_document(path.as_str()))
        .count();
    // The floor is on what git names, not on the readers: a `git` that ran in
    // the wrong place names nothing, and an empty subject set finds no reader.
    assert!(
        documents > 500,
        "git names {documents} committed documents under {}, which is not this repository",
        root.display()
    );
    let (readers, unresolved) = document_readers(&first_party_indices(&root), &root, &committed);
    let listed = listed_readers(&root);
    let strays = unlisted_documents(&readers, &listed);
    println!(
        "{} committed documents; {} readers of them; {} rows on the two lists; {}",
        documents,
        readers.len(),
        listed.len(),
        diagnostic_summary(&unresolved)
    );
    assert!(
        strays.is_empty(),
        "{}",
        document_complaint(&strays, &unresolved)
    );
}

/// RED — **each read call is judged by its own path**: a planted crate's
/// reads of committed documents through all three calls and an include are
/// found, named by their item and refused while neither list has a row; its
/// reads of `target/`, of the temporary directory and its bare mentions of a
/// read are not readers; a mixed function is a reader of its committed path
/// only; and a rooted read it cannot reduce is a diagnostic.
///
/// The plant is a crate of its own under the machine's temporary directory,
/// with the real repository's committed paths as the subject set; written
/// whole, it would be readers in this file.
///
/// It also hands a committed fixture to a same-file wrapper and calls a
/// method on one — readers, named by their callee — and compares a committed
/// path inside `assert_eq!`, which is not.
///
/// MUTATION: drop `"include_str"` from [`INCLUDE_MACROS`] and `tests::LIST` is
/// not found; skip every call that is not one of the three reads in
/// `DocumentScan::read_at_run_time` (`continue` before its subjects are
/// gathered) and `tests::hands_a_fixture_to_a_method` and
/// `tests::hands_a_fixture_to_a_wrapper` are not; search `read_` instead of
/// `read` in [`READ_CALLS`] and `tests::reads_a_text_with_fs_read` is not;
/// delete the `target` arm of `DocumentScan::judge_path` and
/// `tests::reads_its_own_output_under_target` joins the diagnostic; bind a
/// loop variable to nothing (`elements` answers `None`) and
/// `tests::hands_two_ledgers_in_a_loop` is not found; stop a constant chain
/// after eight steps and `tests::reads_through_nine_constants` is not; drop
/// the diagnostic from calls other than the three reads and
/// `tests::hands_a_computed_ledger` leaves it; let a binding's scope not
/// matter (drop the scope test in `Evaluation::binding`) and the inner loop's
/// `structural-debt.md` joins `tests::hands_the_outer_fixtures_after_an_inner_loop`;
/// read `format!` as nothing (`Some("format") => Value::Opaque`) and
/// `tests::reads_a_formatted_path` is not found and
/// `tests::reads_a_formatted_name` leaves the diagnostic; start a `let`'s
/// scope at the `let` (`self.offset(local.span())` for the scope's start in
/// `Collector::visit_local`) and `tests::reads_through_a_shadowing_let`
/// reads its own result; let a spec'd placeholder take no argument and
/// `tests::formats_a_label_before_the_path` reads nothing; splice an
/// unreducible piece before a rooted one into a string that names the path
/// and `tests::reads_after_an_unknown_prefix` leaves the diagnostic, which
/// the count shows; let a splice yield one outcome (only the Computed
/// diagnostic) and `tests::hands_a_script_with_a_computed_word` is no
/// reader; follow every `impl`'s constant whatever trait is named and
/// `tests::reads_a_trait_qualified_constant` reads both ledgers; identify a
/// dependency's trait by its last segment and
/// `tests::reads_one_of_two_same_named_traits` reads two documents and
/// `tests::reads_through_a_trait_alias` none; leave a trait's default out and
/// `tests::reads_a_default_associated_constant` is no reader.
///
/// It also reads through a chain of nine constants to a committed `.tsv`,
/// hands two ledgers to a wrapper in a loop over a literal list of tuples,
/// hands a wrapper a rooted path with a computed name, and reads a constant
/// that names itself — the last two the diagnostic, with no overflow. And it
/// shadows a loop variable in an inner loop and hands the outer one on after
/// it, reads a path built by `format!` and one whose `format!` takes a
/// computed name, reads a parent module's constant through `super::`, and one
/// `super::super::` leads to no constant of. And it shadows a `let` with one
/// whose initialiser reads the first, formats a path after a `{:?}` label,
/// and formats an unknown prefix before a rooted path. And it hands on a
/// string that spells a script and a computed word — a reader of the script
/// and a diagnostic both — and reads an associated constant named through
/// one of two traits, and one named through neither; one named through a
/// `use … as` alias of a dependency's trait, one through one of two
/// dependency traits of the same last name, and a trait's default.
#[test]
fn a_planted_document_reader_on_neither_list_makes_the_tripwire_fire() {
    let plot = temporary_directory("planted-documents");
    let module = plot.join("crates").join("planted").join("src");
    std::fs::create_dir_all(&module).expect("a tree of our own");
    std::fs::write(module.join("lib.rs"), planted_document_reader()).expect("the plant");
    let universe = Universe::declare(
        "the planted document reader",
        vec![TargetRoot {
            id: TargetId {
                package: "planted".to_owned(),
                kind: TargetKind::Library,
                name: "planted".to_owned(),
            },
            file: module.join("lib.rs"),
        }],
        vec![DiskScope::under(&module)],
        Vendor::Excluded,
    )
    .expect("the plant is there");
    let index =
        Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
    let committed = committed_files(&workspace_root());

    let indices = [(plot.join("crates").join("planted"), index)];
    let (readers, unresolved) = document_readers(&indices, &plot, &committed);
    let found: Vec<(String, Vec<Reading>, Vec<String>)> = readers
        .iter()
        .map(|reader| {
            (
                reader
                    .owners
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" / "),
                reader.readings.iter().cloned().collect(),
                reader.subjects.iter().cloned().collect(),
            )
        })
        .collect();
    let expected = [
        (
            "tests::LIST",
            Reading::Included,
            "crates/bt-app/src/window_waits.tsv",
        ),
        (
            "tests::formats_a_label_before_the_path",
            Reading::HandedTo("read_fixture".to_owned()),
            "docs/plans/SOURCE-READERS.tsv",
        ),
        (
            "tests::hands_a_fixture_to_a_method",
            Reading::HandedTo("read_link".to_owned()),
            "packaging/macos/Info.plist.in",
        ),
        (
            "tests::hands_a_fixture_to_a_wrapper",
            Reading::HandedTo("read_fixture".to_owned()),
            "tests/assets/folio-pdf-test.pdf",
        ),
        (
            "tests::hands_a_script_with_a_computed_word",
            Reading::HandedTo("read_fixture".to_owned()),
            "scripts/shell-integration/folio.ps1",
        ),
        (
            "tests::hands_the_outer_fixtures_after_an_inner_loop",
            Reading::HandedTo("read_fixture".to_owned()),
            "docs/plans/MIGRATION-DEBT.tsv;docs/plans/POINTER-DEBT.tsv",
        ),
        (
            "tests::hands_two_ledgers_in_a_loop",
            Reading::HandedTo("read_fixture".to_owned()),
            "docs/plans/MIGRATION-DEBT.tsv;docs/plans/POINTER-DEBT.tsv",
        ),
        (
            "tests::inner::reads_a_parent_constant",
            Reading::AtRunTime,
            "docs/RULES.md",
        ),
        (
            "tests::opens_a_json",
            Reading::AtRunTime,
            "packaging/scoop/folio.json",
        ),
        (
            "tests::reads_a_default_associated_constant",
            Reading::AtRunTime,
            "docs/plans/MIGRATION-DEBT.tsv",
        ),
        (
            "tests::reads_a_formatted_path",
            Reading::AtRunTime,
            "docs/DESIGN.md",
        ),
        (
            "tests::reads_a_text_with_fs_read",
            Reading::AtRunTime,
            "crates/bt-app/src/file_reads_doors.txt",
        ),
        (
            "tests::reads_a_trait_qualified_constant",
            Reading::AtRunTime,
            "docs/plans/SOURCE-READERS.tsv",
        ),
        (
            "tests::reads_its_output_and_a_ledger",
            Reading::AtRunTime,
            "docs/plans/POINTER-DEBT.tsv",
        ),
        (
            "tests::reads_one_of_two_same_named_traits",
            Reading::AtRunTime,
            "docs/plans/TIMING-BOUND-TESTS.tsv",
        ),
        (
            "tests::reads_the_ledger",
            Reading::AtRunTime,
            "docs/plans/structural-debt.md",
        ),
        (
            "tests::reads_through_a_shadowing_let",
            Reading::AtRunTime,
            "docs/plans/TIMING-BOUND-TESTS.tsv",
        ),
        (
            "tests::reads_through_a_trait_alias",
            Reading::AtRunTime,
            "docs/plans/POINTER-DEBT.tsv",
        ),
        (
            "tests::reads_through_nine_constants",
            Reading::AtRunTime,
            "docs/plans/TIMING-BOUND-TESTS.tsv",
        ),
    ]
    .map(|(owner, reading, subjects)| {
        (
            owner.to_owned(),
            vec![reading],
            subjects.split(';').map(ToOwned::to_owned).collect(),
        )
    });
    assert_eq!(found, expected, "the scan found {readers:#?}");
    let mut diagnosed: Vec<(String, String, Why)> = unresolved
        .iter()
        .map(|read| {
            (
                read.owners.iter().cloned().collect::<Vec<_>>().join(" / "),
                read.callee.clone(),
                read.why.clone(),
            )
        })
        .collect();
    diagnosed.sort();
    assert_eq!(
        diagnosed,
        [
            (
                "tests::hands_a_computed_ledger".to_owned(),
                "read_fixture".to_owned(),
                Why::Computed
            ),
            (
                "tests::hands_a_script_with_a_computed_word".to_owned(),
                "read_fixture".to_owned(),
                Why::Computed
            ),
            (
                "tests::inner::reads_a_constant_it_cannot_place".to_owned(),
                "std::fs::read_to_string".to_owned(),
                Why::Elsewhere("super::super::NOWHERE".to_owned())
            ),
            (
                "tests::reads_a_formatted_name".to_owned(),
                "std::fs::read_to_string".to_owned(),
                Why::Computed
            ),
            (
                "tests::reads_a_name_it_is_handed".to_owned(),
                "std::fs::read_to_string".to_owned(),
                Why::Computed
            ),
            (
                "tests::reads_a_self_named_constant".to_owned(),
                "std::fs::read_to_string".to_owned(),
                Why::Cycle("self::SELF_NAMED".to_owned())
            ),
            (
                "tests::reads_after_an_unknown_prefix".to_owned(),
                "std::fs::read_to_string".to_owned(),
                Why::Computed
            ),
            (
                "tests::reads_an_ambiguous_associated_constant".to_owned(),
                "std::fs::read_to_string".to_owned(),
                Why::Elsewhere("Fixture::NAME".to_owned())
            ),
        ],
        "the rooted reads with a computed name, handed to a read or to a wrapper, and the \
         self-named constant are the diagnostic; the computed reads under target/ and the \
         temporary directory are not"
    );

    // The count on its own: a planted shape the scan stops seeing — dropped
    // in silence instead of diagnosed — changes it.
    assert_eq!(
        unresolved.len(),
        8,
        "the planted crate's diagnostic is exactly eight paths"
    );

    let listed = listed_readers(&workspace_root());
    let strays = unlisted_documents(&readers, &listed);
    assert_eq!(
        strays.len(),
        19,
        "a planted reader was taken for a listed one"
    );
    let said = document_complaint(&strays, &unresolved);
    assert!(
        said.contains("crates/planted/src/lib.rs")
            && said.contains("tests::reads_the_ledger")
            && said.contains("docs/plans/structural-debt.md")
            && said.contains("tests::LIST")
            && said.contains(DEBT_LIST)
            && said.contains(SOURCE_LIST)
            && said.contains("8 path(s) handed to a call"),
        "the refusal names each item, its file, both lists and the diagnostic; it said:\n{said}"
    );
    assert!(
        !said.contains("out.tsv"),
        "the mixed function is refused for its committed read only; it said:\n{said}"
    );

    // With a row each, none is a stray.
    let rows: BTreeSet<(String, String)> = expected
        .iter()
        .map(|(owner, _, _)| ("crates/planted/src/lib.rs".to_owned(), owner.clone()))
        .collect();
    assert!(unlisted_documents(&readers, &rows).is_empty());

    std::fs::remove_dir_all(&plot).expect("the plant is taken away again");
}

/// RED (T-SOURCE-READERS-TSV-TRIPWIRE rounds 7–8) — **visibility tells
/// same-named candidates apart, and nothing else**. The tree compiles, so a
/// path the reading code names is one Rust let it name; where two ways of
/// bringing a name in lead to two constants — `pub use hidden::*` and `pub
/// use shared::*` at a crate's root, `hidden`'s constant private — the one
/// visible to the reading module is the constant. A crate's own test reads
/// its private constant; another crate reads a `pub` one and a `pub use` of
/// one in a private module.
///
/// Two `pub` constants of one name behind `pub(crate) use left::*` and
/// `pub use right::*` are, to another crate, `right`'s alone: a candidate is
/// visible only when every `use` on its way admits the reader too.
///
/// MUTATION: let `Evaluation::disambiguated` take the first candidate and
/// `tests::reads_one_of_two_glob_constants` reads `structural-debt.md`; let
/// it take every candidate and it reads both; let it ignore the `use`s'
/// visibility (`scope.admitted`) and `tests::reads_through_the_public_glob`
/// is a false ambiguity in the diagnostic.
#[test]
fn a_name_two_globs_bring_in_is_the_constant_visible_to_the_reader() {
    let plot = temporary_directory("planted-crates");
    let committed = committed_files(&workspace_root());
    let mut indices = Vec::new();
    for (name, text) in [
        ("planted_lib", planted_library()),
        ("planted_user", planted_user()),
    ] {
        let package = plot.join("crates").join(name);
        let module = package.join("src");
        std::fs::create_dir_all(&module).expect("a tree of our own");
        std::fs::write(module.join("lib.rs"), text).expect("the plant");
        let universe = Universe::declare(
            name,
            vec![TargetRoot {
                id: TargetId {
                    package: name.to_owned(),
                    kind: TargetKind::Library,
                    name: name.to_owned(),
                },
                file: module.join("lib.rs"),
            }],
            vec![DiskScope::under(&module)],
            Vendor::Excluded,
        )
        .expect("the plant is there");
        let index =
            Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
        indices.push((package, index));
    }
    let (readers, unresolved) = document_readers(&indices, &plot, &committed);
    let found: Vec<(String, String, Vec<String>)> = readers
        .iter()
        .map(|reader| {
            (
                reader.file.clone(),
                reader
                    .owners
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" / "),
                reader.subjects.iter().cloned().collect(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            (
                "crates/planted_lib/src/lib.rs",
                "tests::reads_its_own_private_constant",
                "docs/plans/MIGRATION-DEBT.tsv",
            ),
            (
                "crates/planted_user/src/lib.rs",
                "tests::reads_a_public_constant",
                "docs/plans/TIMING-BOUND-TESTS.tsv",
            ),
            (
                "crates/planted_user/src/lib.rs",
                "tests::reads_a_reexported_constant",
                "docs/plans/POINTER-DEBT.tsv",
            ),
            (
                "crates/planted_user/src/lib.rs",
                "tests::reads_one_of_two_glob_constants",
                "docs/plans/SOURCE-READERS.tsv",
            ),
            (
                "crates/planted_user/src/lib.rs",
                "tests::reads_through_the_public_glob",
                "docs/RULES.md",
            ),
        ]
        .map(|(file, owner, subject)| (
            file.to_owned(),
            owner.to_owned(),
            vec![subject.to_owned()]
        )),
        "the scan found {readers:#?}"
    );
    assert!(
        unresolved.is_empty(),
        "every name the planted crates use is one constant: {unresolved:#?}"
    );
    std::fs::remove_dir_all(&plot).expect("the plant is taken away again");
}

// ───────────────────────────────────────────────────────────── the fixture ──

/// A file that reads its own tree in all four of the shapes the scan knows.
///
/// Assembled rather than spelled: written out whole, this function would be four
/// readers in `tripwire.rs`.
fn planted_reader() -> String {
    let neighbour = concat!("neighbour", ".rs");
    let including = concat!("include_", "str!");
    let scope = concat!("Scope::", "File(");
    format!(
        "//! A planted reader, on neither list.\n\
         const TEXT: &str = {including}(\"{neighbour}\");\n\
         fn read_one() -> String {{\n    \
             std::fs::read_to_string(std::path::Path::new(\".\").join(\"{neighbour}\")).unwrap()\n\
         }}\n\
         fn scoped() -> bt_source::Scope {{ {scope}bt_source::FileScoped::VendoredAddedFiles) }}\n\
         fn walk() {{\n    \
             let root = std::path::Path::new(env!(\"{MANIFEST_DIRECTORY}\")).join(\"src\");\n    \
             for entry in std::fs::{DIRECTORY_READ}(&root).unwrap().flatten() {{\n        \
                 if entry.path().extension().is_some_and(|kind| kind == {RUST_EXTENSION}) {{}}\n    \
             }}\n\
         }}\n"
    )
}

/// A crate whose test module reads committed documents through an include
/// and all three run-time calls, reads its own output under `target/` and a
/// temporary file, names a read without calling it, mixes a scratch read with
/// a committed one, and reads a rooted path with a name it is handed — on
/// neither list.
///
/// A raw string: none of it is a `.rs` name, so it is no reader of the four
/// shapes, and a string literal is no call or include of this file's.
fn planted_document_reader() -> String {
    r#"//! A planted document reader, on neither list.
#[cfg(test)]
mod tests {
    const LIST: &str = include_str!("../../bt-app/src/window_waits.tsv");

    const C1: &str = C2;
    const C2: &str = C3;
    const C3: &str = C4;
    const C4: &str = C5;
    const C5: &str = C6;
    const C6: &str = C7;
    const C7: &str = C8;
    const C8: &str = C9;
    const C9: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/plans/TIMING-BOUND-TESTS.tsv");

    const SELF_NAMED: &str = self::SELF_NAMED;

    const RULES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/RULES.md");

    fn repository_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
    }

    #[test]
    fn reads_the_ledger() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let _ = std::fs::read_to_string(root.join("../../docs/plans/structural-debt.md"));
    }

    #[test]
    fn reads_a_text_with_fs_read() {
        let _ = std::fs::read(repository_root().join("crates/bt-app/src/file_reads_doors.txt"));
    }

    #[test]
    fn opens_a_json() {
        let path = repository_root().join("packaging").join("scoop").join("folio.json");
        let _ = std::fs::File::open(&path);
    }

    #[test]
    fn reads_its_own_output_under_target() {
        let name = std::process::id().to_string();
        let _ = std::fs::read(repository_root().join("target").join(name));
    }

    #[test]
    fn reads_a_temporary_file() {
        assert!(!env!("CARGO_MANIFEST_DIR").is_empty());
        let name = std::process::id().to_string();
        let scratch = std::env::temp_dir().join(name);
        let _ = std::fs::read_to_string(&scratch);
    }

    #[test]
    fn reads_its_output_and_a_ledger() {
        let out = repository_root().join("target").join("planted").join("out.tsv");
        let _ = std::fs::read_to_string(&out);
        let ledger = repository_root().join("docs").join("plans").join("POINTER-DEBT.tsv");
        let _ = std::fs::read_to_string(ledger);
    }

    #[test]
    fn names_a_read_without_calling_it() {
        let _read = std::fs::read;
        let _open = std::fs::File::open;
        let _ = repository_root().join("docs").join("plans").join("MIGRATION-DEBT.tsv");
    }

    fn reads_a_name_it_is_handed(name: &str) -> String {
        std::fs::read_to_string(repository_root().join("docs").join(name)).unwrap()
    }

    fn read_fixture(path: &std::path::Path) -> Vec<u8> {
        std::fs::read(path).unwrap()
    }

    #[test]
    fn hands_a_fixture_to_a_wrapper() {
        let _ = read_fixture(&repository_root().join("tests/assets/folio-pdf-test.pdf"));
    }

    #[test]
    fn hands_a_fixture_to_a_method() {
        let _ = repository_root().join("packaging/macos/Info.plist.in").read_link();
    }

    #[test]
    fn reads_through_nine_constants() {
        let _ = std::fs::read_to_string(C1);
    }

    #[test]
    fn reads_a_self_named_constant() {
        let _ = std::fs::read_to_string(SELF_NAMED);
    }

    #[test]
    fn hands_two_ledgers_in_a_loop() {
        for (name, _rows) in [("POINTER-DEBT.tsv", 540), ("MIGRATION-DEBT.tsv", 7)] {
            let _ = read_fixture(&repository_root().join("docs/plans").join(name));
        }
    }

    #[test]
    fn hands_the_outer_fixtures_after_an_inner_loop() {
        for f in ["POINTER-DEBT.tsv", "MIGRATION-DEBT.tsv"] {
            for f in ["structural-debt.md"] {
                let _ = f;
            }
            let _ = read_fixture(&repository_root().join("docs/plans").join(f));
        }
    }

    #[test]
    fn reads_a_formatted_path() {
        let _ = std::fs::read_to_string(format!("{}/../../docs/DESIGN.md", env!("CARGO_MANIFEST_DIR")));
    }

    #[test]
    fn reads_through_a_shadowing_let() {
        let path = repository_root().join("docs/plans/TIMING-BOUND-TESTS.tsv");
        let path = std::fs::read_to_string(path);
        assert!(path.is_ok());
    }

    #[test]
    fn formats_a_label_before_the_path() {
        let label = "ledger";
        let ledger = repository_root().join("docs/plans/SOURCE-READERS.tsv");
        let said = format!("{:?} {}", label, ledger.display());
        let _ = read_fixture(std::path::Path::new(&said));
    }

    fn reads_after_an_unknown_prefix(unknown: &str) -> String {
        std::fs::read_to_string(format!("{}{}/docs/DESIGN.md", unknown, env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    trait Ledger {
        const NAME: &'static str;
    }

    trait Corpus {
        const NAME: &'static str;
    }

    struct Fixture;

    impl Ledger for Fixture {
        const NAME: &'static str = "docs/plans/SOURCE-READERS.tsv";
    }

    impl Corpus for Fixture {
        const NAME: &'static str = "docs/plans/MIGRATION-DEBT.tsv";
    }

    #[test]
    fn reads_a_trait_qualified_constant() {
        let _ = std::fs::read_to_string(repository_root().join(<Fixture as Ledger>::NAME));
    }

    use dep_c::Trait as Alias;

    impl dep_c::Trait for Fixture {
        const NAME: &'static str = "docs/plans/POINTER-DEBT.tsv";
    }

    impl dep_a::Same for Fixture {
        const NAME: &'static str = "docs/plans/TIMING-BOUND-TESTS.tsv";
    }

    impl dep_b::Same for Fixture {
        const NAME: &'static str = "docs/plans/structural-debt.md";
    }

    trait Defaulted {
        const NAME: &'static str = "docs/plans/MIGRATION-DEBT.tsv";
    }

    impl Defaulted for Fixture {}

    #[test]
    fn reads_through_a_trait_alias() {
        let _ = std::fs::read_to_string(repository_root().join(<Fixture as Alias>::NAME));
    }

    #[test]
    fn reads_one_of_two_same_named_traits() {
        let _ = std::fs::read_to_string(repository_root().join(<Fixture as dep_a::Same>::NAME));
    }

    #[test]
    fn reads_a_default_associated_constant() {
        let _ = std::fs::read_to_string(repository_root().join(<Fixture as Defaulted>::NAME));
    }

    #[test]
    fn reads_an_ambiguous_associated_constant() {
        let _ = std::fs::read_to_string(repository_root().join(Fixture::NAME));
    }

    fn hands_a_script_with_a_computed_word(word: &str) -> Vec<u8> {
        let script = repository_root().join("scripts/shell-integration/folio.ps1");
        read_fixture(std::path::Path::new(&format!("{} {}", script.display(), word)))
    }

    fn reads_a_formatted_name(name: &str) -> String {
        std::fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), name)).unwrap()
    }

    mod inner {
        #[test]
        fn reads_a_parent_constant() {
            let _ = std::fs::read_to_string(super::RULES);
        }

        #[test]
        fn reads_a_constant_it_cannot_place() {
            let _ = std::fs::read_to_string(super::super::NOWHERE);
        }
    }

    fn hands_a_computed_ledger(name: &str) -> Vec<u8> {
        read_fixture(&repository_root().join("docs").join(name))
    }

    #[test]
    fn compares_a_fixture_path() {
        assert_eq!(repository_root().join("docs/DESIGN.md"), std::path::PathBuf::new());
    }
}
"#
    .to_owned()
}

/// A library with a private constant and its own test that reads it, a
/// `pub` one, a `pub use` of one in a private module, and two globs that
/// bring in a constant of one name each — one private, so not re-exported.
fn planted_library() -> String {
    r#"//! A planted library.
const DOC: &str = "docs/plans/MIGRATION-DEBT.tsv";
pub const OPEN_DOC: &str = "docs/plans/TIMING-BOUND-TESTS.tsv";

mod secret {
    pub const SECRET_DOC: &str = "docs/plans/POINTER-DEBT.tsv";
}

pub use secret::SECRET_DOC;

mod hidden {
    #[allow(dead_code)]
    const SHARED_DOC: &str = "docs/plans/structural-debt.md";
}

pub mod shared {
    pub const SHARED_DOC: &str = "docs/plans/SOURCE-READERS.tsv";
}

pub use hidden::*;
pub use shared::*;

mod left {
    pub const PAIRED_DOC: &str = "docs/DESIGN.md";
}

mod right {
    pub const PAIRED_DOC: &str = "docs/RULES.md";
}

pub(crate) use left::*;
pub use right::*;

#[cfg(test)]
mod tests {
    fn root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
    }

    #[test]
    fn reads_its_own_private_constant() {
        let _ = std::fs::read_to_string(root().join(super::DOC));
    }
}
"#
    .to_owned()
}

/// A crate whose tests name the library's public constants.
fn planted_user() -> String {
    r#"//! A planted user of the planted library.
#[cfg(test)]
mod tests {
    fn root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
    }

    #[test]
    fn reads_through_the_public_glob() {
        let _ = std::fs::read_to_string(root().join(planted_lib::PAIRED_DOC));
    }

    #[test]
    fn reads_one_of_two_glob_constants() {
        let _ = std::fs::read_to_string(root().join(planted_lib::SHARED_DOC));
    }

    #[test]
    fn reads_a_reexported_constant() {
        let _ = std::fs::read_to_string(root().join(planted_lib::SECRET_DOC));
    }

    #[test]
    fn reads_a_public_constant() {
        let _ = std::fs::read_to_string(root().join(planted_lib::OPEN_DOC));
    }
}
"#
    .to_owned()
}

/// A directory of our own under the machine's temporary one.
///
/// No `tempfile`: the name is `bt_testpath`'s, the one owner of a test's scratch
/// name, which no other call in this or any running process is given.
fn temporary_directory(name: &str) -> PathBuf {
    let path = bt_testpath::temp_path(&format!("bt-source-{name}"));
    std::fs::create_dir_all(&path).expect("a directory of our own");
    path
}
