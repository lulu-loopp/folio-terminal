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
//!   the product's, not a reader's — and that read in one of two ways:
//!   1. an `include_str!` or `include_bytes!` whose literal, taken from the
//!      including file's directory, is a committed document;
//!   2. a function whose body calls `read_to_string`, `fs::read` or
//!      `File::open` on a path that is either **rooted in the checkout** — the
//!      body names `CARGO_MANIFEST_DIR`, or calls a free function of its own
//!      file that does (transitively), with no literal leading into `target/`
//!      — or **spelled out**: a literal of the body, or of a `const` or
//!      `static` of its file the body names, that is a whole path from the
//!      workspace root or the package directory to a committed document. A
//!      rooted body is a reader of whatever committed file a literal piece of
//!      its path ends; one that names none computes its path, and is a
//!      reader of what it computes. A read that names only Rust source keeps
//!      the four shapes' rules.
//! * **The item** is the innermost function holding the read; outside every
//!   function it is the `const` or `static` the include initialises, or the
//!   module itself. The name is the module path without `crate`, with an
//!   integration test's, example's, bench's or build script's target name in
//!   front (`census::committed_census`).
//!
//! This half reads Rust through `bt-source`'s index, because its key is an
//! item and only the index knows which module a byte is in. What it does not
//! see, written down rather than discovered: a path handed in as a parameter
//! by a rooted caller (the reader is the function that reads, and it neither
//! roots the path nor spells it — `scan` above, which reads `.ps1` files, is
//! one); a root helper in another file, or reached through a method; a bare
//! file name in an unrooted body (`README.md` is as often a scratch file's
//! name); and a read through any other call than the three. Rows are not
//! checked the other way: a row whose item no longer reads is not refused.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use bt_source::{
    Certainty, DiskScope, FileRecord, FileScoped, Index, ItemKind, ItemRecord, LiteralValue,
    MacroKind, Occurrence, Package, Pattern, Scope, Search, Span, TargetId, TargetKind, TargetRoot,
    TokenKind, Universe, Vendor, View, Workspace, is_vendored, needle, normalized, report,
    universes,
};

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Reading {
    /// `include_str!` or `include_bytes!` of a committed path.
    Included,
    /// `read_to_string`, `fs::read` or `File::open` on a path the item roots in
    /// the checkout or spells out.
    AtRunTime,
}

impl Reading {
    fn said(self) -> &'static str {
        match self {
            Self::Included => "includes a committed document",
            Self::AtRunTime => "reads a committed document at run time",
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
                .map(|reading| reading.said())
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

/// The committed files a literal's value names: the value, its `.` and `..`
/// segments dropped, as a whole trailing run of a committed path's segments.
/// `docs/plans/X.tsv` names one file; `X.tsv` names every committed file called
/// that.
fn named_by(value: &str, committed: &BTreeSet<String>) -> BTreeSet<String> {
    let segments: Vec<&str> = value
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty() && *segment != "." && *segment != "..")
        .collect();
    if segments.is_empty() {
        return BTreeSet::new();
    }
    let tail = segments.join("/");
    committed
        .iter()
        .filter(|path| {
            path.as_str() == tail
                || path
                    .strip_suffix(tail.as_str())
                    .is_some_and(|head| head.ends_with('/'))
        })
        .cloned()
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

    /// The free functions a body calls by name: an identifier with an opening
    /// parenthesis after it and neither `.` nor `fn` before it.
    fn calls_in(&self, body: Span) -> BTreeSet<&str> {
        let union = self.index.union();
        self.names_in(body.start(), body.end())
            .into_iter()
            .filter(|(at, name)| {
                let after = union[at + name.len()..].trim_start();
                let before = union[..*at].trim_end();
                after.starts_with('(') && !before.ends_with('.') && !before.ends_with("fn")
            })
            .map(|(_, name)| name)
            .collect()
    }

    /// Whether a body roots a path in the checkout itself: it names the crate's
    /// own directory, and no literal of it leads into `target/`, whose files
    /// are built, not committed.
    fn roots_in_the_checkout(&self, body: Span) -> bool {
        let strings = self.strings_in(body.start(), body.end());
        let names_the_manifest_directory = strings.contains(&MANIFEST_DIRECTORY)
            || self
                .names_in(body.start(), body.end())
                .iter()
                .any(|(_, name)| *name == MANIFEST_DIRECTORY);
        let leads_into_target = strings.iter().any(|value| {
            value
                .split(['/', '\\'])
                .find(|segment| !segment.is_empty() && *segment != "." && *segment != "..")
                == Some("target")
        });
        names_the_manifest_directory && !leads_into_target
    }

    /// The free functions that root a path in the checkout, each by its file
    /// and its name: it roots one itself, or calls one of its own file's that
    /// does. A call is resolved within the caller's file because a name is all
    /// a call carries, and `root` or `scratch` is a helper of a dozen files.
    fn root_helpers(&self) -> BTreeSet<(usize, &str)> {
        let free: Vec<(usize, &str, Span)> = self
            .index
            .items()
            .iter()
            .filter(|item| item.kind() == ItemKind::Function && item.type_owner().is_none())
            .filter_map(|item| {
                Some((
                    self.index.file_of(item).span().start(),
                    item.name(),
                    item.body()?,
                ))
            })
            .collect();
        let calls: Vec<BTreeSet<&str>> = free
            .iter()
            .map(|(_, _, body)| self.calls_in(*body))
            .collect();
        let mut helpers: BTreeSet<(usize, &str)> = free
            .iter()
            .filter(|(_, _, body)| self.roots_in_the_checkout(*body))
            .map(|(file, name, _)| (*file, *name))
            .collect();
        loop {
            let before = helpers.len();
            for ((file, name, _), called) in free.iter().zip(&calls) {
                if called
                    .iter()
                    .any(|callee| helpers.contains(&(*file, *callee)))
                {
                    helpers.insert((*file, *name));
                }
            }
            if helpers.len() == before {
                return helpers;
            }
        }
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

    /// The decoded string values of every `const` and `static` a file
    /// declares, by name.
    fn file_values(&self, file: &FileRecord) -> BTreeMap<&str, Vec<&str>> {
        let mut values: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (name, start, end) in self.value_statements(file) {
            values
                .entry(name)
                .or_default()
                .extend(self.strings_in(start, end));
        }
        values
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

    /// The committed file a literal's value names as a whole path from the
    /// workspace root or from the package directory, if it names one. A bare
    /// file name is not a path from anywhere: `README.md` is what a test calls
    /// the file it writes into its own scratch directory as often as not.
    fn anchored(&self, value: &str) -> BTreeSet<String> {
        if !value.contains(['/', '\\']) {
            return BTreeSet::new();
        }
        let root = normalized(self.root);
        let value = value.replace('\\', "/");
        [self.root, self.package]
            .into_iter()
            .map(|base| from_root(&normalized(&base.join(&value)), &root))
            .filter(|path| self.committed.contains(path))
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

    /// Every function outside the product whose body reads a file at run time
    /// on a path it roots in the checkout, or on a path one of its literals
    /// spells out to a committed document.
    fn read_at_run_time(&self) -> Vec<DocumentReader> {
        let helpers = self.root_helpers();
        let mut sites = Vec::new();
        for pattern in [
            needle!(Pattern::call("read_to_string")),
            needle!(Pattern::path("fs::read")),
            needle!(Pattern::path("File::open")),
        ] {
            sites.extend(
                self.index
                    .search(&Search::new(pattern, View::Identifiers))
                    .unwrap_or_else(|failure| panic!("{failure}"))
                    .spans(),
            );
        }
        sites.sort();
        // The first read of each function, by the function's own bytes.
        let mut first_read: BTreeMap<Span, usize> = BTreeMap::new();
        for site in sites {
            if self.in_the_product(site) {
                continue;
            }
            if let Some(item) = self.innermost_callable(site.start()) {
                first_read.entry(item.whole()).or_insert(site.start());
            }
        }
        let mut values_by_file: BTreeMap<String, BTreeMap<&str, Vec<&str>>> = BTreeMap::new();
        let mut found = Vec::new();
        for (whole, site) in first_read {
            let item = self
                .innermost_callable(site)
                .filter(|item| item.whole() == whole)
                .expect("the function the read was found in");
            let body = item
                .body()
                .expect("a function with a read in it has a body");
            let file = self.index.file_of(item);
            let values = values_by_file
                .entry(self.relative(file))
                .or_insert_with(|| self.file_values(file));
            let referenced: Vec<&str> = self
                .names_in(body.start(), body.end())
                .into_iter()
                .filter_map(|(_, name)| values.get(name))
                .flatten()
                .copied()
                .collect();
            let rooted = self.roots_in_the_checkout(body)
                || self
                    .calls_in(body)
                    .iter()
                    .any(|callee| helpers.contains(&(file.span().start(), *callee)));
            // A rooted path is built a piece at a time, so a piece names every
            // committed file it ends; an unrooted one reaches the checkout only
            // as a whole path from the workspace root or from the package
            // directory, where `cargo test` runs a test.
            let named: BTreeSet<String> = self
                .strings_in(body.start(), body.end())
                .into_iter()
                .chain(referenced)
                .flat_map(|value| {
                    if rooted {
                        named_by(value, self.committed)
                    } else {
                        self.anchored(value)
                    }
                })
                .collect();
            let documents: BTreeSet<String> = named
                .iter()
                .filter(|path| is_document(path.as_str()))
                .cloned()
                .collect();
            // A read that names only Rust source is a Rust-source reader, which
            // the four shapes above hold; a rooted one that names no committed
            // file computes its path, and reads whatever it computes.
            let reads_a_document = !documents.is_empty() || (rooted && named.is_empty());
            if reads_a_document {
                found.push(DocumentReader {
                    file: self.relative(file),
                    line: self.line(site),
                    owners: self.item_names(item),
                    readings: BTreeSet::from([Reading::AtRunTime]),
                    subjects: documents,
                });
            }
        }
        found
    }
}

/// Every reader of a committed document in `indices`, one per item, in file
/// and item order.
fn document_readers(
    indices: &[(PathBuf, Index)],
    root: &Path,
    committed: &BTreeSet<String>,
) -> Vec<DocumentReader> {
    let mut by_item: BTreeMap<(String, BTreeSet<String>), DocumentReader> = BTreeMap::new();
    for (package, index) in indices {
        let scan = DocumentScan {
            index,
            root,
            package,
            committed,
        };
        for reader in scan.included().into_iter().chain(scan.read_at_run_time()) {
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
    by_item.into_values().collect()
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

/// What the document half says when it fires: each item, its file, both lists.
fn document_complaint(strays: &[&DocumentReader]) -> String {
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
         refused as well."
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
    let readers = document_readers(&first_party_indices(&root), &root, &committed);
    let listed = listed_readers(&root);
    let strays = unlisted_documents(&readers, &listed);
    println!(
        "{} committed documents; {} readers of them; {} rows on the two lists",
        documents,
        readers.len(),
        listed.len()
    );
    assert!(strays.is_empty(), "{}", document_complaint(&strays));
}

/// RED — **a planted test that reads a committed document at run time, and a
/// planted `const` that includes one, are each found, named by their item, and
/// refused while neither list has a row for them.**
///
/// The plant is a crate of its own under the machine's temporary directory,
/// with the real repository's committed paths as the subject set; written
/// whole, it would be two readers in this file.
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
    let readers = document_readers(&indices, &plot, &committed);
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
                reader.readings.iter().copied().collect(),
                reader.subjects.iter().cloned().collect(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            (
                "tests::LIST".to_owned(),
                vec![Reading::Included],
                vec!["crates/bt-app/src/window_waits.tsv".to_owned()]
            ),
            (
                "tests::reads_the_ledger".to_owned(),
                vec![Reading::AtRunTime],
                vec!["docs/plans/structural-debt.md".to_owned()]
            ),
        ],
        "the scan found {readers:#?}"
    );

    let listed = listed_readers(&workspace_root());
    let strays = unlisted_documents(&readers, &listed);
    assert_eq!(
        strays.len(),
        2,
        "a planted reader was taken for a listed one"
    );
    let said = document_complaint(&strays);
    assert!(
        said.contains("crates/planted/src/lib.rs")
            && said.contains("tests::reads_the_ledger")
            && said.contains("docs/plans/structural-debt.md")
            && said.contains("tests::LIST")
            && said.contains(DEBT_LIST)
            && said.contains(SOURCE_LIST),
        "the refusal names each item, its file and both lists; it said:\n{said}"
    );

    // With a row each, neither is a stray.
    let rows: BTreeSet<(String, String)> = ["tests::LIST", "tests::reads_the_ledger"]
        .into_iter()
        .map(|owner| ("crates/planted/src/lib.rs".to_owned(), owner.to_owned()))
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

/// A crate whose test module reads a committed document at run time and
/// includes another in a `const`, on neither list.
///
/// Assembled rather than spelled, for `planted_reader`'s reason.
fn planted_document_reader() -> String {
    let including = concat!("include_", "str!");
    let reading = concat!("read_", "to_string");
    format!(
        "//! A planted document reader, on neither list.\n\
         #[cfg(test)]\n\
         mod tests {{\n    \
             const LIST: &str = {including}(\"../../bt-app/src/window_waits.tsv\");\n    \
             #[test]\n    \
             fn reads_the_ledger() {{\n        \
                 let root = std::path::Path::new(env!(\"{MANIFEST_DIRECTORY}\"));\n        \
                 let text = std::fs::{reading}(root.join(\"../../docs/plans/structural-debt.md\"));\n        \
                 assert!(text.is_ok());\n    \
             }}\n\
         }}\n"
    )
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
