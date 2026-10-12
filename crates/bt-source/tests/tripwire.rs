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
//!      as far as its own spelling goes: a string literal, a
//!      `Path::new`/`PathBuf::from`/`.join(…)`/`.parent()` chain, `env!` and
//!      `concat!`, a `let` binding of the same function written before the
//!      call, a `const` or `static` of the same file, and a call of a free
//!      function of the same file (its tail, with its parameters bound to what
//!      the call hands it). Its base is the package directory
//!      (`CARGO_MANIFEST_DIR`, or a relative path, which `cargo test` resolves
//!      there), a scratch root (`temp_dir`, `temp_path`, `tempfile`) or a base
//!      this reading cannot see (a parameter, a field, a loop variable, another
//!      file's function). A path under the package directory that lands on a
//!      committed document is a read of it; one that lands under `target/`,
//!      on Rust source or on an uncommitted file is not; a scratch path is
//!      not. A path on an unseen base reads a committed document only when
//!      its literal segments are a whole path to one from the workspace root
//!      (two segments or more, which a scratch file's bare name is not). So a
//!      function that reads its own output under `target/` and a committed
//!      `.tsv` is a reader of the `.tsv` alone.
//!
//!      A rooted call whose path keeps a piece this reading cannot reduce
//!      (`root.join(name)` with `name` a loop variable, a `format!`, another
//!      crate's constant) is not judged: it is counted and listed as a
//!      diagnostic — printed by the test and in its refusal — and not refused.
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
//!      resolve one from the package directory, and an unreduced rooted path
//!      is no diagnostic.
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
//! inside a macro whose arguments do not parse as expressions; and the rooted
//! reads it lists as unreduced. Rows are not checked the other way: a row whose item no
//! longer reads is not refused.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

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
        let mut readers = Vec::new();
        let mut unresolved = Vec::new();
        for file in self.index.files() {
            let span = file.span();
            let text = self.index.text(span);
            let syntax = FileSyntax::of(text, file.path());
            for call in &syntax.calls {
                let at = span.start() + call.offset;
                let evaluate = |expression: &syn::Expr| {
                    Evaluation {
                        syntax: &syntax,
                        function: call.function,
                        position: call.offset,
                        parameters: BTreeMap::new(),
                        depth: 0,
                    }
                    .of(expression)
                };
                if reads.contains_key(&at) {
                    let value = call.arguments.first().map_or(Value::Opaque, evaluate);
                    match self.judge(value, true) {
                        Judgement::NotADocument => {}
                        Judgement::Document(subject) => readers.push(DocumentReader {
                            file: self.relative(file),
                            line: self.line(at),
                            owners: self.owners_of(at),
                            readings: BTreeSet::from([Reading::AtRunTime]),
                            subjects: BTreeSet::from([subject]),
                        }),
                        Judgement::Unresolved => unresolved.push(UnresolvedRead {
                            file: self.relative(file),
                            line: self.line(at),
                            owners: self.owners_of(at),
                        }),
                    }
                    continue;
                }
                let subjects: BTreeSet<String> = call
                    .receiver
                    .iter()
                    .chain(&call.arguments)
                    .filter_map(|expression| match self.judge(evaluate(expression), false) {
                        Judgement::Document(subject) => Some(subject),
                        Judgement::NotADocument | Judgement::Unresolved => None,
                    })
                    .collect();
                if subjects.is_empty() || self.token_in_the_product(at) {
                    continue;
                }
                readers.push(DocumentReader {
                    file: self.relative(file),
                    line: self.line(at),
                    owners: self.owners_of(at),
                    readings: BTreeSet::from([Reading::HandedTo(call.callee.clone())]),
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
    fn judge(&self, value: Value, read: bool) -> Judgement {
        let Value::Path {
            base,
            segments,
            open,
        } = value.into_path()
        else {
            return Judgement::NotADocument;
        };
        match base {
            Base::Scratch => Judgement::NotADocument,
            Base::Relative if !read => Judgement::NotADocument,
            Base::Package | Base::Relative => {
                let mut path = self.package.to_path_buf();
                path.extend(&segments);
                let from = from_root(&normalized(&path), &normalized(self.root));
                if from == "target" || from.starts_with("target/") {
                    Judgement::NotADocument
                } else if open {
                    Judgement::Unresolved
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
    /// Rooted in the package directory, with a piece this reading cannot
    /// reduce to a literal after the root.
    Unresolved,
}

/// A rooted read whose path this reading could not reduce to a committed
/// file: a diagnostic, not a refusal.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct UnresolvedRead {
    file: String,
    line: usize,
    owners: BTreeSet<String>,
}

impl UnresolvedRead {
    fn named(&self) -> String {
        format!(
            "{}:{} {}",
            self.file,
            self.line,
            self.owners.iter().cloned().collect::<Vec<_>>().join(" / ")
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
    /// A base this reading cannot see: a parameter, a field, a loop variable,
    /// a call into another file.
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
    /// Nothing this reading can reduce.
    Opaque,
}

impl Value {
    /// The value used as a path. A text is relative; an absolute one is
    /// outside the checkout.
    fn into_path(self) -> Self {
        match self {
            Self::Text(text) => match segments_of(&text) {
                Some(segments) => Self::Path {
                    base: Base::Relative,
                    segments,
                    open: false,
                },
                None => Self::Opaque,
            },
            Self::Opaque => Self::Path {
                base: Base::Unknown,
                segments: Vec::new(),
                open: false,
            },
            path @ Self::Path { .. } => path,
        }
    }

    /// `self.join(other)`: a relative piece adds its segments, a rooted one
    /// replaces the path, and one this reading cannot reduce leaves it open.
    fn join(self, other: Self) -> Self {
        let Self::Path {
            base,
            mut segments,
            mut open,
        } = self.into_path()
        else {
            // An absolute text: a path outside the checkout, whatever follows.
            return Self::Opaque;
        };
        match other {
            Self::Text(text) => match segments_of(&text) {
                Some(more) => segments.extend(more),
                None => return Self::Opaque,
            },
            Self::Path {
                base: Base::Unknown,
                ..
            }
            | Self::Opaque => open = true,
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

/// One `let` binding of a single name.
struct Binding {
    offset: usize,
    function: usize,
    name: String,
    value: syn::Expr,
}

/// A free function of the file: its parameters' names and its tail
/// expression.
struct Helper {
    function: usize,
    end: usize,
    parameters: Vec<Option<String>>,
    tail: Option<syn::Expr>,
}

/// What one file's syntax says about the paths its read calls are handed:
/// the calls, the bindings before them, the file's own constants and free
/// functions. Parsed from the index's own text of the file.
struct FileSyntax {
    calls: Vec<ReadCall>,
    bindings: Vec<Binding>,
    /// `None` where two items share the name, which this reading cannot tell
    /// apart.
    constants: BTreeMap<String, Option<syn::Expr>>,
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
            syntax: Self {
                calls: Vec::new(),
                bindings: Vec::new(),
                constants: BTreeMap::new(),
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
    syntax: FileSyntax,
}

impl Collector<'_> {
    /// A span's start as a byte offset into the file. The parser counts
    /// columns in characters.
    fn offset(&self, span: proc_macro2::Span) -> usize {
        let at = span.start();
        let line = self.line_starts[at.line - 1];
        let rest = &self.text[line..];
        line + rest
            .char_indices()
            .nth(at.column)
            .map_or(rest.len(), |(byte, _)| byte)
    }

    fn end(&self, span: proc_macro2::Span) -> usize {
        let at = span.end();
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
}

fn bound_name(pattern: &syn::Pat) -> Option<String> {
    match pattern {
        syn::Pat::Ident(name) => Some(name.ident.to_string()),
        syn::Pat::Type(typed) => bound_name(&typed.pat),
        _ => None,
    }
}

impl<'ast> syn::visit::Visit<'ast> for Collector<'_> {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let start = self.offset(item.span());
        let tail = match item.block.stmts.last() {
            Some(syn::Stmt::Expr(expression, None)) => Some(expression.clone()),
            _ => None,
        };
        let helper = Helper {
            function: start,
            end: self.end(item.span()),
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
        self.syntax
            .constants
            .entry(item.ident.to_string())
            .and_modify(|held| *held = None)
            .or_insert(Some((*item.expr).clone()));
        syn::visit::visit_item_const(self, item);
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        self.syntax
            .constants
            .entry(item.ident.to_string())
            .and_modify(|held| *held = None)
            .or_insert(Some((*item.expr).clone()));
        syn::visit::visit_item_static(self, item);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let (Some(name), Some(init)) = (bound_name(&local.pat), &local.init) {
            self.syntax.bindings.push(Binding {
                offset: self.offset(local.span()),
                function: self.function(),
                name,
                value: (*init.expr).clone(),
            });
        }
        syn::visit::visit_local(self, local);
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
        let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        if let Ok(arguments) = mac.parse_body_with(parser) {
            for argument in &arguments {
                self.visit_expr(argument);
            }
        }
    }
}

/// One expression, evaluated as a path in the scope of one read call.
struct Evaluation<'a> {
    syntax: &'a FileSyntax,
    /// The function whose bindings are in scope.
    function: usize,
    /// Only bindings written before this offset are in scope.
    position: usize,
    /// A helper's parameters, bound to what its caller handed it.
    parameters: BTreeMap<String, Value>,
    depth: usize,
}

impl Evaluation<'_> {
    /// How deep a chain of helpers this reading follows.
    const DEEPEST: usize = 8;

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
            syn::Expr::Path(path) => match path.path.segments.last() {
                Some(last) if path.path.segments.len() == 1 => self.named(&last.ident.to_string()),
                Some(last) => self.constant(&last.ident.to_string()),
                None => Value::Opaque,
            },
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
                    "to_path_buf" | "as_path" | "clone" | "to_owned" | "into" | "as_ref"
                    | "unwrap" | "expect" | "borrow" | "as_os_str" => receiver(),
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
            Some("concat") => {
                let mut values = arguments.iter().map(|argument| self.of(argument));
                let first = values.next().unwrap_or(Value::Text(String::new()));
                let mut rest = String::new();
                for value in values {
                    let Value::Text(text) = value else {
                        return Value::Opaque;
                    };
                    rest.push_str(&text);
                }
                // `concat!(env!("CARGO_MANIFEST_DIR"), "/../x")` appends to the
                // directory's text, so its leading separator is not a root.
                match first {
                    Value::Text(text) => Value::Text(text + &rest),
                    path @ Value::Path { .. } => {
                        path.join(Value::Text(rest.trim_start_matches(['/', '\\']).to_owned()))
                    }
                    Value::Opaque => Value::Opaque,
                }
            }
            Some("format") => match arguments.first() {
                Some(syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(text),
                    ..
                })) if arguments.len() == 1 && !text.value().contains('{') => {
                    Value::Text(text.value())
                }
                _ => Value::Opaque,
            },
            _ => Value::Opaque,
        }
    }

    /// A single name: a helper's parameter, then the latest binding of it
    /// before this point in the same function, then the file's constant.
    fn named(&self, name: &str) -> Value {
        if let Some(value) = self.parameters.get(name) {
            return value.clone();
        }
        let binding = self
            .syntax
            .bindings
            .iter()
            .filter(|binding| {
                binding.function == self.function
                    && binding.offset < self.position
                    && binding.name == name
            })
            .max_by_key(|binding| binding.offset);
        match binding {
            Some(binding) => Evaluation {
                syntax: self.syntax,
                function: self.function,
                position: binding.offset,
                parameters: self.parameters.clone(),
                depth: self.depth,
            }
            .of(&binding.value),
            None => self.constant(name),
        }
    }

    /// The file's constant of that name. A constant can name itself through
    /// a longer path (`const A: &str = other::A;`), so the chain is bounded
    /// like a helper's.
    fn constant(&self, name: &str) -> Value {
        match self.syntax.constants.get(name) {
            Some(Some(expression)) if self.depth < Self::DEEPEST => Evaluation {
                syntax: self.syntax,
                function: usize::MAX,
                position: 0,
                parameters: BTreeMap::new(),
                depth: self.depth + 1,
            }
            .of(expression),
            _ => Value::Opaque,
        }
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
        if self.depth >= Self::DEEPEST {
            return Value::Opaque;
        }
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
        Evaluation {
            syntax: self.syntax,
            function: helper.function,
            position: helper.end,
            parameters,
            depth: self.depth + 1,
        }
        .of(tail)
    }
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
         refused as well.\n\n{} rooted read(s) whose path this scan cannot reduce to a literal \
         file are not judged (a diagnostic, not a refusal).",
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
        "{} committed documents; {} readers of them; {} rows on the two lists; {} rooted reads \
         not reduced:",
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
/// delete the `target` arm of
/// `DocumentScan::judge` and `tests::reads_its_own_output_under_target` joins
/// the diagnostic.
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
            "tests::opens_a_json",
            Reading::AtRunTime,
            "packaging/scoop/folio.json",
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
    ]
    .map(|(owner, reading, subject)| (owner.to_owned(), vec![reading], vec![subject.to_owned()]));
    assert_eq!(found, expected, "the scan found {readers:#?}");
    assert_eq!(
        unresolved
            .iter()
            .map(|read| read.owners.iter().cloned().collect::<Vec<_>>().join(" / "))
            .collect::<Vec<_>>(),
        ["tests::reads_a_name_it_is_handed"],
        "the one rooted read with a computed name is the diagnostic, and the computed reads under target/ and the temporary directory are not"
    );

    let listed = listed_readers(&workspace_root());
    let strays = unlisted_documents(&readers, &listed);
    assert_eq!(
        strays.len(),
        7,
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
            && said.contains("1 rooted read(s)"),
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
