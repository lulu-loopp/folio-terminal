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
//!        `.parent()` chain; `env!("CARGO_MANIFEST_DIR")`;
//!      - `concat!` and `format!`, spliced piece by piece: a format string's
//!        holes are filled by what fills them (the next or a numbered
//!        argument, a named one, a captured name; a hole with a spec is not
//!        the value's own text). Text after a leading path continues the
//!        path; a path spelled into text after other text makes a string
//!        that names that path (`format!(". '{}'", script.display())`), judged
//!        by it;
//!      - the innermost binding of a name in scope at the call: a `let` is in
//!        scope over the rest of its block, a `for` loop's variable over the
//!        loop's body, and the latest one whose scope holds the call wins, so
//!        an inner loop's variable is gone once the inner loop ends. A loop
//!        over a literal list (an array, `&[…]`, `vec![…]`, `.iter()` over one,
//!        a name or constant bound to one; a tuple pattern picks its field) is
//!        each element in turn, every one judged;
//!      - a constant: by bare name, the file's constant of that name; through
//!        `self::`, `super::` and `crate::`, the module bt-source's module tree
//!        leads to — in this file or another of the same target — and the
//!        constant written there, brought there by a `use` or a `use …::*`, or
//!        declared there by `thread_local!` (never a path). Any other longer
//!        path, and one that runs through a type (`crate::x::Type::CONST`),
//!        names an item this reading cannot see. A constant name is
//!        SCREAMING_SNAKE_CASE, as the workspace's `non_upper_case_globals`
//!        lint makes every one;
//!      - a call of a free function of the same file: its tail, with its
//!        parameters bound to what the call hands it.
//!
//!      Constants and helpers are followed as far as their chains go; one
//!      that reaches itself again on the same resolution path is a cycle. Its
//!      base is the package directory (`CARGO_MANIFEST_DIR`, or a relative
//!      path, which `cargo test` resolves there), a scratch root (`temp_dir`,
//!      `temp_path`, `tempfile`), an absolute path (outside the checkout: a
//!      committed absolute path into it would be a machine path, which
//!      `scripts/check-machine-paths.ps1` refuses), or a base this reading
//!      cannot see (a parameter, a field, another file's function). A path
//!      under the package directory that lands on a committed document is a
//!      read of it; one that lands under `target/`, on Rust source or on an
//!      uncommitted file is not; a scratch path is not. A path on an unseen
//!      base reads a committed document only when its literal segments are a
//!      whole path to one from the workspace root (two segments or more,
//!      which a scratch file's bare name is not). So a function that reads its
//!      own output under `target/` and a committed `.tsv` is a reader of the
//!      `.tsv` alone.
//!
//!      **A path this reading gives up on is never dropped in silence.** A
//!      rooted path that keeps a piece it cannot reduce (`root.join(name)`
//!      with `name` a parameter, a `format!` hole it cannot fill, another
//!      crate's constant), a constant or helper that comes back to itself, and
//!      a `self::`/`super::`/`crate::` constant the module tree holds no
//!      constant, `use` or `thread_local!` of, is not judged: it is counted
//!      and listed as a diagnostic — file, line, item, callee and why, printed
//!      by the test and counted in its refusal — and not refused.
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
        let world = World::of(self.index);
        let mut readers = Vec::new();
        let mut unresolved = Vec::new();
        for (position, file) in self.index.files().iter().enumerate() {
            let span = file.span();
            let text = self.index.text(span);
            let syntax = Rc::new(FileSyntax::of(text, file.path()));
            for call in &syntax.calls {
                let at = span.start() + call.offset;
                let module = world.module_at(at);
                let evaluate = |expression: &syn::Expr| {
                    Evaluation {
                        world: &world,
                        syntax: Rc::clone(&syntax),
                        file: position,
                        module: module.clone(),
                        function: call.function,
                        position: call.offset,
                        parameters: BTreeMap::new(),
                        visiting: Vec::new(),
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
                    Judgement::Unresolved(why) => Some(*why),
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
            Value::Text(_) | Value::Opaque => vec![Judgement::NotADocument],
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

/// Why a path handed to a call could not be reduced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Why {
    /// Rooted in the package directory, with a piece after the root this
    /// reading cannot reduce to a literal: a parameter, a `format!`, another
    /// crate's constant.
    Computed,
    /// A constant or a helper that reaches itself on its own resolution path.
    Cycle,
    /// A constant a `self::`, `super::` or `crate::` path names that this
    /// reading cannot place in the module tree.
    Elsewhere,
}

impl Why {
    fn said(self) -> &'static str {
        match self {
            Self::Computed => "a rooted path with a piece it cannot reduce",
            Self::Cycle => "a constant or helper that reaches itself",
            Self::Elsewhere => "a constant of a module path it cannot place",
        }
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

impl UnresolvedRead {
    fn named(&self) -> String {
        format!(
            "{}:{} {} — `{}`, {}",
            self.file,
            self.line,
            self.owners.iter().cloned().collect::<Vec<_>>().join(" / "),
            self.callee,
            self.why.said()
        )
    }
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
    /// Text with paths spelled into it (`format!(". '{}'", script.display())`):
    /// it names each of them, and is judged by them.
    Within(Vec<Value>),
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
            Self::Opaque => Self::Path {
                base: Base::Unknown,
                segments: Vec::new(),
                open: false,
            },
            Self::Each(values) => Self::Each(values.into_iter().map(Self::into_path).collect()),
            path @ (Self::Path { .. } | Self::Unreduced(_) | Self::Within(_)) => path,
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
            // Text with paths in it is no path to join onto.
            Self::Within(_) => return Self::Opaque,
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
            Self::Path {
                base: Base::Unknown,
                ..
            }
            | Self::Opaque
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

/// One binding of a single name: a `let`, or a `for` loop's variable.
struct Binding {
    offset: usize,
    function: usize,
    /// The bytes the name is in scope over: the block a `let` is written in,
    /// or a `for` loop's body.
    scope: (usize, usize),
    name: String,
    /// What a `let` binds, or what a `for` loop iterates.
    value: syn::Expr,
    /// A loop variable: the name is, in turn, the part of each element of
    /// `value` this path of tuple positions picks — empty for the whole
    /// element (`for name in …`), `[0]` for `for (name, _) in …`.
    each: Option<Vec<usize>>,
}

/// Every name a loop pattern binds, with the tuple positions that reach it.
fn loop_names(pattern: &syn::Pat, at: &mut Vec<usize>, found: &mut Vec<(String, Vec<usize>)>) {
    match pattern {
        syn::Pat::Ident(name) => found.push((name.ident.to_string(), at.clone())),
        syn::Pat::Type(typed) => loop_names(&typed.pat, at, found),
        syn::Pat::Reference(reference) => loop_names(&reference.pat, at, found),
        syn::Pat::Paren(inner) => loop_names(&inner.pat, at, found),
        syn::Pat::Tuple(tuple) => {
            for (position, element) in tuple.elems.iter().enumerate() {
                at.push(position);
                loop_names(element, at, found);
                at.pop();
            }
        }
        _ => {}
    }
}

/// The part of a list element a loop variable is: the element itself, or
/// the tuple fields its positions pick.
fn projected<'e>(element: &'e syn::Expr, positions: &[usize]) -> Option<&'e syn::Expr> {
    let Some((first, rest)) = positions.split_first() else {
        return Some(element);
    };
    match element {
        syn::Expr::Tuple(tuple) => projected(tuple.elems.iter().nth(*first)?, rest),
        syn::Expr::Paren(inner) => projected(&inner.expr, positions),
        syn::Expr::Reference(reference) => projected(&reference.expr, positions),
        _ => None,
    }
}

/// A free function of the file: its parameters' names and its tail
/// expression.
struct Helper {
    function: usize,
    /// Where the tail starts: the point its bindings are seen from.
    tail_at: usize,
    parameters: Vec<Option<String>>,
    tail: Option<syn::Expr>,
}

/// A `const` or `static`: the inline modules of its file it is written in,
/// and its value.
struct Constant {
    inline: Vec<String>,
    value: syn::Expr,
}

/// What one file's syntax says about the paths its read calls are handed:
/// the calls, the bindings before them, the file's own constants and free
/// functions. Parsed from the index's own text of the file.
struct FileSyntax {
    calls: Vec<ReadCall>,
    bindings: Vec<Binding>,
    /// By bare name, file-wide: `None` where two items share the name, which
    /// a bare name cannot tell apart.
    constants: BTreeMap<String, Option<Rc<Constant>>>,
    /// By the inline modules they are written in and their name.
    scoped: BTreeMap<(Vec<String>, String), Rc<Constant>>,
    /// Each `use` binding of a name, by the inline modules it is written in:
    /// the path it names.
    uses: BTreeMap<(Vec<String>, String), Vec<String>>,
    /// Each `use …::*`, by the inline modules it is written in.
    globs: BTreeMap<Vec<String>, Vec<Vec<String>>>,
    /// The statics `thread_local!` declares: keys to a thread's own value,
    /// never a path.
    locals: BTreeSet<(Vec<String>, String)>,
    helpers: BTreeMap<String, Option<Helper>>,
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
            text,
            line_starts: std::iter::once(0)
                .chain(text.match_indices('\n').map(|(at, _)| at + 1))
                .collect(),
            functions: vec![0],
            blocks: vec![(0, text.len())],
            modules: Vec::new(),
            syntax: Self {
                calls: Vec::new(),
                bindings: Vec::new(),
                constants: BTreeMap::new(),
                scoped: BTreeMap::new(),
                uses: BTreeMap::new(),
                globs: BTreeMap::new(),
                locals: BTreeSet::new(),
                helpers: BTreeMap::new(),
            },
        };
        syn::visit::visit_file(&mut collector, &file);
        collector.syntax
    }
}

struct Collector<'a> {
    text: &'a str,
    line_starts: Vec<usize>,
    /// The start of each function the walk is inside, innermost last.
    functions: Vec<usize>,
    /// The span of each block the walk is inside, innermost last.
    blocks: Vec<(usize, usize)>,
    /// The inline modules the walk is inside, outermost first.
    modules: Vec<String>,
    syntax: FileSyntax,
}

impl Collector<'_> {
    /// A span's start as a byte offset into the file. The parser counts
    /// columns in characters.
    fn offset(&self, span: proc_macro2::Span) -> usize {
        self.at(span.start())
    }

    fn end(&self, span: proc_macro2::Span) -> usize {
        self.at(span.end())
    }

    fn at(&self, at: proc_macro2::LineColumn) -> usize {
        let line = self.line_starts[at.line - 1];
        let rest = &self.text[line..];
        line + rest
            .char_indices()
            .nth(at.column)
            .map_or(rest.len(), |(byte, _)| byte)
    }

    fn function(&self) -> usize {
        *self
            .functions
            .last()
            .expect("the file is the outermost scope")
    }

    fn record_constant(&mut self, name: String, value: &syn::Expr) {
        let constant = Rc::new(Constant {
            inline: self.modules.clone(),
            value: value.clone(),
        });
        self.syntax
            .constants
            .entry(name.clone())
            .and_modify(|held| *held = None)
            .or_insert_with(|| Some(Rc::clone(&constant)));
        self.syntax
            .scoped
            .insert((self.modules.clone(), name), constant);
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

fn bound_name(pattern: &syn::Pat) -> Option<String> {
    match pattern {
        syn::Pat::Ident(name) => Some(name.ident.to_string()),
        syn::Pat::Type(typed) => bound_name(&typed.pat),
        syn::Pat::Reference(reference) => bound_name(&reference.pat),
        _ => None,
    }
}

impl<'ast> syn::visit::Visit<'ast> for Collector<'_> {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if item.content.is_some() {
            self.modules.push(item.ident.to_string());
            syn::visit::visit_item_mod(self, item);
            self.modules.pop();
        }
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.blocks
            .push((self.offset(block.span()), self.end(block.span())));
        syn::visit::visit_block(self, block);
        self.blocks.pop();
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let start = self.offset(item.span());
        let tail = match item.block.stmts.last() {
            Some(syn::Stmt::Expr(expression, None)) => Some(expression.clone()),
            _ => None,
        };
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
            .and_modify(|held| *held = None)
            .or_insert(Some(helper));
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
        self.record_constant(item.ident.to_string(), &item.expr);
        syn::visit::visit_item_const(self, item);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        let mut names = Vec::new();
        let mut globs = Vec::new();
        use_bindings(&item.tree, &mut Vec::new(), &mut names, &mut globs);
        for (name, path) in names {
            self.syntax.uses.insert((self.modules.clone(), name), path);
        }
        self.syntax
            .globs
            .entry(self.modules.clone())
            .or_default()
            .extend(globs);
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        self.record_constant(item.ident.to_string(), &item.expr);
        syn::visit::visit_item_static(self, item);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let (Some(name), Some(init)) = (bound_name(&local.pat), &local.init) {
            self.syntax.bindings.push(Binding {
                offset: self.offset(local.span()),
                function: self.function(),
                scope: *self.blocks.last().expect("the file is the outermost block"),
                name,
                value: (*init.expr).clone(),
                each: None,
            });
        }
        syn::visit::visit_local(self, local);
    }

    fn visit_expr_for_loop(&mut self, looped: &'ast syn::ExprForLoop) {
        let mut names = Vec::new();
        loop_names(&looped.pat, &mut Vec::new(), &mut names);
        let body = (
            self.offset(looped.body.span()),
            self.end(looped.body.span()),
        );
        for (name, positions) in names {
            self.syntax.bindings.push(Binding {
                offset: self.offset(looped.span()),
                function: self.function(),
                scope: body,
                name,
                value: (*looped.expr).clone(),
                each: Some(positions),
            });
        }
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
    /// `assert_eq!`, `format!`, `vec![…]` — are walked like any other code.
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

/// Where every module of one index is written, and the files read so far for
/// a constant another file names.
struct World<'a> {
    index: &'a Index,
    /// Each module: the position of its file in `index.files()`, and the
    /// inline `mod`s inside that file that lead to it.
    modules: BTreeMap<ModuleKey, (usize, Vec<String>)>,
    /// The other way round: the module a file's inline path is, in the first
    /// target that compiles it.
    placed: BTreeMap<(usize, Vec<String>), ModuleKey>,
    read: RefCell<BTreeMap<usize, Rc<FileSyntax>>>,
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

impl<'a> World<'a> {
    fn of(index: &'a Index) -> Self {
        let mut modules = BTreeMap::new();
        let mut placed = BTreeMap::new();
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
                let key = (owner.target.clone(), declared.clone());
                placed
                    .entry((position, inline.clone()))
                    .or_insert_with(|| key.clone());
                modules.insert(key, (position, inline));
            }
        }
        Self {
            index,
            modules,
            placed,
            read: RefCell::new(BTreeMap::new()),
        }
    }

    /// The syntax of the file at a position, read once, its calls left out.
    fn syntax(&self, position: usize) -> Rc<FileSyntax> {
        if let Some(syntax) = self.read.borrow().get(&position) {
            return Rc::clone(syntax);
        }
        let file = &self.index.files()[position];
        let mut syntax = FileSyntax::of(self.index.text(file.span()), file.path());
        syntax.calls.clear();
        let syntax = Rc::new(syntax);
        self.read.borrow_mut().insert(position, Rc::clone(&syntax));
        syntax
    }

    /// Where a path of module segments leads from a module: `self`, `super`,
    /// `crate`, and child modules bt-source's module tree declares.
    fn walk(&self, from: &ModuleKey, segments: &[String]) -> Walk {
        let (target, mut path) = from.clone();
        for segment in segments {
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
                        return Walk::Item;
                    }
                    path = next;
                }
            }
        }
        Walk::Module((target, path))
    }

    /// The module a byte of the index is written in.
    fn module_at(&self, at: usize) -> Option<ModuleKey> {
        let module = self
            .index
            .modules()
            .iter()
            .filter(|module| module.span().holds(at))
            .min_by_key(|module| module.span().len())?;
        let file = self.index.file_at(at)?;
        let declared = module.module_paths().first()?;
        let owner = owner_of(file, declared)?;
        Some((owner.target.clone(), declared.clone()))
    }
}

/// A constant's name: every `const` and `static` of this workspace is
/// SCREAMING_SNAKE_CASE, which the `non_upper_case_globals` lint the
/// workspace denies makes a rule. A longer path ending in any other name is a
/// function, a type or a variant.
fn names_a_constant(name: &str) -> bool {
    name.chars().all(|character| {
        character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
    }) && name.chars().any(|character| character.is_ascii_uppercase())
}

/// Where a module path leads from a module: to a module of the same target,
/// through a type, function or other crate (an item this reading cannot see),
/// or nowhere (`super` past the crate root).
enum Walk {
    Module(ModuleKey),
    Item,
    Lost,
}

/// A constant found, with the scope its value is read in, or the value that
/// stands for one that was not.
enum Resolved<'a> {
    Found(Evaluation<'a>, Rc<Constant>),
    Missing(Value),
}

/// A file's syntax, its position in the index, and a module written in it.
struct Place {
    syntax: Rc<FileSyntax>,
    file: usize,
    module: Option<ModuleKey>,
}

/// One expression, evaluated as a path in the scope of one read call.
struct Evaluation<'a> {
    world: &'a World<'a>,
    syntax: Rc<FileSyntax>,
    /// The position of the syntax's file in the index.
    file: usize,
    /// The module the expression is written in.
    module: Option<ModuleKey>,
    /// The function whose bindings are in scope.
    function: usize,
    /// Only bindings written before this offset, and whose scope holds it,
    /// are in scope.
    position: usize,
    /// A helper's parameters, bound to what its caller handed it.
    parameters: BTreeMap<String, Value>,
    /// The constants and helpers on the current resolution path. A chain is
    /// followed as far as it goes; one that comes back to itself is a cycle.
    visiting: Vec<String>,
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
        })
    }

    /// The same scope, seen from an earlier point of the same function.
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
        }
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
            syn::Expr::Macro(mac) => self.of_macro(&mac.mac),
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
            syn::Expr::MethodCall(call) => {
                let receiver = || self.of(&call.receiver);
                match call.method.to_string().as_str() {
                    "join" => match call.args.first() {
                        Some(argument) => receiver().join(self.of(argument)),
                        None => Value::Opaque,
                    },
                    "parent" => receiver().parent(),
                    // `dir.parent().and_then(Path::parent)`: the parent's parent.
                    "and_then" | "map"
                        if call.args.len() == 1
                            && matches!(&call.args[0], syn::Expr::Path(path)
                                if path.path.segments.last().is_some_and(|last| last.ident == "parent")) =>
                    {
                        receiver().parent()
                    }
                    // A path spelled for `format!` is the path's own text.
                    "to_path_buf" | "as_path" | "clone" | "to_owned" | "into" | "as_ref"
                    | "unwrap" | "expect" | "borrow" | "as_os_str" | "display"
                    | "to_string_lossy" | "to_str" | "to_string" | "into_owned" => receiver(),
                    _ => Value::Opaque,
                }
            }
            syn::Expr::Call(call) => self.of_call(call),
            _ => Value::Opaque,
        }
    }

    fn of_call(&self, call: &syn::ExprCall) -> Value {
        let syn::Expr::Path(function) = &*call.func else {
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
            return Value::Path {
                base: Base::Package,
                segments: Vec::new(),
                open: false,
            };
        }
        if matches!(last, "temp_dir" | "temp_path") || segments.iter().any(|s| s == "tempfile") {
            return Value::Path {
                base: Base::Scratch,
                segments: Vec::new(),
                open: false,
            };
        }
        if converts(&segments) {
            return call
                .args
                .first()
                .map_or(Value::Opaque, |argument| self.of(argument));
        }
        if segments.len() == 1 {
            return self.helper(last, call);
        }
        Value::Opaque
    }

    fn of_macro(&self, mac: &syn::Macro) -> Value {
        let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        let Ok(arguments) = mac.parse_body_with(parser) else {
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
                })) if text.value() == MANIFEST_DIRECTORY => Value::Path {
                    base: Base::Package,
                    segments: Vec::new(),
                    open: false,
                },
                _ => Value::Opaque,
            },
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
            _ => Value::Opaque,
        }
    }

    /// A format string's pieces: its text, and each hole filled with the value
    /// of what fills it — the next or a numbered argument, a named one, or a
    /// name the string captures. A hole with a format spec (`{:?}`, `{:>8}`)
    /// is not the value's own text.
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
                    let value = if !spec.is_empty() {
                        Value::Opaque
                    } else if reference.is_empty() {
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
                    pieces.push(Value::Text(std::mem::take(&mut text)));
                    pieces.push(value);
                }
                other => text.push(other),
            }
        }
        pieces.push(Value::Text(text));
        pieces
    }

    /// The innermost binding of a name in scope here: written before this
    /// point, in this function, in a scope that holds this point.
    fn binding(&self, name: &str, loops: bool) -> Option<&Binding> {
        self.syntax
            .bindings
            .iter()
            .filter(|binding| {
                binding.function == self.function
                    && binding.offset < self.position
                    && binding.scope.0 <= self.position
                    && self.position < binding.scope.1
                    && binding.name == name
                    && (loops || binding.each.is_none())
            })
            .max_by_key(|binding| binding.offset)
    }

    /// A single name: a helper's parameter, then the innermost binding of it
    /// in scope here, then the file's constant.
    fn named(&self, name: &str) -> Value {
        if let Some(value) = self.parameters.get(name) {
            return value.clone();
        }
        let Some(binding) = self.binding(name, true) else {
            return self.constant(&[name.to_owned()]);
        };
        let scope = self.at(binding.offset);
        let Some(positions) = &binding.each else {
            return scope.of(&binding.value);
        };
        match scope.elements(&binding.value) {
            Some(elements) => Value::Each(
                elements
                    .iter()
                    .map(|element| {
                        projected(element, positions).map_or(Value::Opaque, |part| scope.of(part))
                    })
                    .collect(),
            ),
            None => Value::Opaque,
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
                {
                    return self.at(binding.offset).elements(&binding.value);
                }
                match self.resolve_constant(&segments) {
                    Resolved::Found(scope, constant) => scope.elements(&constant.value),
                    Resolved::Missing(_) => None,
                }
            }
            _ => None,
        }
    }

    /// A constant's value, followed as far as its chain goes.
    fn constant(&self, segments: &[String]) -> Value {
        match self.resolve_constant(segments) {
            Resolved::Found(scope, constant) => scope.of(&constant.value),
            Resolved::Missing(value) => value,
        }
    }

    /// The constant a path names. A bare name is the file's constant of that
    /// name. `self::`, `super::` and `crate::` lead from the module this is
    /// written in through bt-source's module tree to a module of the same
    /// target, in this file or another, and the constant written there; one
    /// this reading cannot place is unreduced, never silent. Any other longer
    /// path (`bt_render::EASE`, `Curve::EASE`) names an item of another crate
    /// or type, which this reading cannot see. A constant reached again on its
    /// own resolution path is a cycle.
    fn resolve_constant(&self, segments: &[String]) -> Resolved<'a> {
        let Some((name, modules)) = segments.split_last() else {
            return Resolved::Missing(Value::Opaque);
        };
        if modules.is_empty() {
            let Some(Some(constant)) = self.syntax.constants.get(name) else {
                return Resolved::Missing(Value::Opaque);
            };
            let module = self
                .world
                .placed
                .get(&(self.file, constant.inline.clone()))
                .cloned();
            let identity = format!("const {}:{}::{name}", self.file, constant.inline.join("::"));
            return match self.entering(
                identity,
                Place {
                    syntax: Rc::clone(&self.syntax),
                    file: self.file,
                    module,
                },
                usize::MAX,
                0,
                BTreeMap::new(),
            ) {
                Some(scope) => Resolved::Found(scope, Rc::clone(constant)),
                None => Resolved::Missing(Value::Unreduced(Why::Cycle)),
            };
        }
        let relative = matches!(modules[0].as_str(), "self" | "super" | "crate");
        if !relative || !names_a_constant(name) {
            return Resolved::Missing(Value::Opaque);
        }
        let Some(start) = self.module.clone() else {
            return Resolved::Missing(Value::Unreduced(Why::Elsewhere));
        };
        match self.world.walk(&start, modules) {
            Walk::Module(key) => self.constant_in(key, name, &mut BTreeSet::new()),
            Walk::Item => Resolved::Missing(Value::Opaque),
            Walk::Lost => Resolved::Missing(Value::Unreduced(Why::Elsewhere)),
        }
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
    ) -> Resolved<'a> {
        if !seen.insert((key.clone(), name.to_owned())) {
            return Resolved::Missing(Value::Unreduced(Why::Cycle));
        }
        let Some((file, inline)) = self.world.modules.get(&key).cloned() else {
            return Resolved::Missing(Value::Unreduced(Why::Elsewhere));
        };
        let syntax = if file == self.file {
            Rc::clone(&self.syntax)
        } else {
            self.world.syntax(file)
        };
        let here = (inline.clone(), name.to_owned());
        if let Some(constant) = syntax.scoped.get(&here).cloned() {
            let identity = format!("const {file}:{}::{name}", inline.join("::"));
            let place = Place {
                syntax,
                file,
                module: Some(key),
            };
            return match self.entering(identity, place, usize::MAX, 0, BTreeMap::new()) {
                Some(scope) => Resolved::Found(scope, constant),
                None => Resolved::Missing(Value::Unreduced(Why::Cycle)),
            };
        }
        if syntax.locals.contains(&here) {
            return Resolved::Missing(Value::Opaque);
        }
        if let Some(path) = syntax.uses.get(&here) {
            let Some((last, modules)) = path.split_last() else {
                return Resolved::Missing(Value::Unreduced(Why::Elsewhere));
            };
            return match self.world.walk(&key, modules) {
                Walk::Module(next) => self.constant_in(next, last, seen),
                Walk::Item => Resolved::Missing(Value::Opaque),
                Walk::Lost => Resolved::Missing(Value::Unreduced(Why::Elsewhere)),
            };
        }
        for glob in syntax.globs.get(&inline).into_iter().flatten() {
            if let Walk::Module(next) = self.world.walk(&key, glob)
                && let found @ Resolved::Found(..) = self.constant_in(next, name, seen)
            {
                return found;
            }
        }
        Resolved::Missing(Value::Unreduced(Why::Elsewhere))
    }

    /// A call of the file's own free function: its tail, with its parameters
    /// bound to what this call hands it.
    fn helper(&self, name: &str, call: &syn::ExprCall) -> Value {
        let Some(Some(helper)) = self.syntax.helpers.get(name) else {
            return Value::Opaque;
        };
        let Some(tail) = &helper.tail else {
            return Value::Opaque;
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
        match self.entering(
            format!("fn {}:{name}", self.file),
            Place {
                syntax: Rc::clone(&self.syntax),
                file: self.file,
                module: self.module.clone(),
            },
            helper.function,
            helper.tail_at,
            parameters,
        ) {
            Some(scope) => scope.of(tail),
            None => Value::Unreduced(Why::Cycle),
        }
    }
}

/// Pieces written one after another — `concat!`'s arguments, a format
/// string's text and holes — as one value: texts join into a text, a text
/// after a path continues the path (from its last segment, or past a
/// separator), and one of several values splices each in turn. A rooted path
/// that a piece this reading cannot reduce follows, or precedes, is
/// unreduced and diagnosed; with no rooted piece, it is an ordinary string.
fn splice(pieces: Vec<Value>) -> Value {
    if let Some(at) = pieces
        .iter()
        .position(|piece| matches!(piece, Value::Each(_)))
    {
        let Value::Each(choices) = pieces[at].clone() else {
            unreachable!("found as several")
        };
        return Value::Each(
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
    let rooted = |value: &Value| {
        matches!(
            value,
            Value::Path {
                base: Base::Package,
                ..
            }
        )
    };
    let mut spliced = Value::Text(String::new());
    for piece in pieces {
        spliced = match (spliced, piece) {
            (Value::Unreduced(why), _) | (_, Value::Unreduced(why)) => Value::Unreduced(why),
            (Value::Text(before), Value::Text(after)) => Value::Text(before + &after),
            (Value::Text(before), path @ Value::Path { .. }) if before.is_empty() => path,
            // A path after text is spelled into a string, which names it;
            // text after that is more string, not more path.
            (Value::Text(_) | Value::Opaque, path @ Value::Path { .. }) => {
                Value::Within(vec![path])
            }
            (Value::Within(mut paths), path @ Value::Path { .. }) => {
                paths.push(path);
                Value::Within(paths)
            }
            (within @ Value::Within(_), _) => within,
            (path @ Value::Path { .. }, Value::Text(after)) => continued(path, &after),
            (path, _) if rooted(&path) => {
                let Value::Path { base, segments, .. } = path else {
                    unreachable!("rooted is a path")
                };
                Value::Path {
                    base,
                    segments,
                    open: true,
                }
            }
            _ => Value::Opaque,
        };
    }
    spliced
}

/// A path with text written straight after it: past a separator the text
/// is more segments; otherwise it lengthens the last one. A rooted path
/// already open stays open.
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
        _ if base == Base::Package => return Value::Unreduced(Why::Computed),
        _ => return Value::Opaque,
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
    for (package, index) in indices {
        let scan = DocumentScan {
            index,
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

/// What the document half says when it fires: each item, its file, both
/// lists, and how many rooted reads it could not reduce.
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
         refused as well.\n\n{} path(s) handed to a call this scan could not reduce to a \
         committed file or rule out are not judged (a diagnostic, not a refusal; the test prints \
         each).",
        unresolved.len()
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
        "{} committed documents; {} readers of them; {} rows on the two lists; {} paths handed \
         to a call not reduced (the diagnostic):",
        documents,
        readers.len(),
        listed.len(),
        unresolved.len()
    );
    for read in &unresolved {
        println!("    {}", read.named());
    }
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
/// `tests::reads_a_formatted_name` leaves the diagnostic.
///
/// It also reads through a chain of nine constants to a committed `.tsv`,
/// hands two ledgers to a wrapper in a loop over a literal list of tuples,
/// hands a wrapper a rooted path with a computed name, and reads a constant
/// that names itself — the last two the diagnostic, with no overflow. And it
/// shadows a loop variable in an inner loop and hands the outer one on after
/// it, reads a path built by `format!` and one whose `format!` takes a
/// computed name, reads a parent module's constant through `super::`, and one
/// `super::super::` leads to no constant of.
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
            "tests::reads_its_output_and_a_ledger",
            Reading::AtRunTime,
            "docs/plans/POINTER-DEBT.tsv",
        ),
        (
            "tests::reads_the_ledger",
            Reading::AtRunTime,
            "docs/plans/structural-debt.md",
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
                read.why,
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
                "tests::inner::reads_a_constant_it_cannot_place".to_owned(),
                "std::fs::read_to_string".to_owned(),
                Why::Elsewhere
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
                Why::Cycle
            ),
        ],
        "the rooted reads with a computed name, handed to a read or to a wrapper, and the \
         self-named constant are the diagnostic; the computed reads under target/ and the \
         temporary directory are not"
    );

    let listed = listed_readers(&workspace_root());
    let strays = unlisted_documents(&readers, &listed);
    assert_eq!(
        strays.len(),
        12,
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
            && said.contains("5 path(s) handed to a call"),
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

/// A directory of our own under the machine's temporary one.
///
/// No `tempfile`: the name is `bt_testpath`'s, the one owner of a test's scratch
/// name, which no other call in this or any running process is given.
fn temporary_directory(name: &str) -> PathBuf {
    let path = bt_testpath::temp_path(&format!("bt-source-{name}"));
    std::fs::create_dir_all(&path).expect("a directory of our own");
    path
}
