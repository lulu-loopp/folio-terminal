//! **No source reader names a file outside the two lists** — the plan's §6.1
//! tripwire, `no_source_reader_names_a_file_outside_the_two_lists`.
//!
//! Two lists stand between this workspace and a new reader bound to a file.
//! `docs/plans/MIGRATION-DEBT.tsv` is the temporary one: every reader that still
//! names a file, one row each, shrinking to zero at P20 and never growing —
//! `scripts/ci/check-migration-debt.ps1` compares it with the merge base and
//! refuses an added row. [`bt_source::FileScoped`] is the permanent one: the few
//! readers whose concern really *is* a file, a variant each, with the reason in
//! the variant's own doc comment. This test is what makes the pair mean
//! something: a reader on neither list is a red test naming the hit and both
//! lists.
//!
//! # Written without the query layer, on purpose
//!
//! The scan below is plain text matching. It does not call `enumerate`, it does
//! not build a [`bt_source::Universe`], it does not parse. That is §6.1's
//! instruction and the reason is simple: this is the guard *against* the
//! mechanism, and a bug in the mechanism must not be able to switch it off. It
//! is also why the crate's own allowlist type is here on the allowlist — see
//! [`bt_source::FileScoped::TheTripwireItself`].
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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use bt_source::{FileScoped, Scope};

/// The temporary list, from the workspace root.
const DEBT_LIST: &str = "docs/plans/MIGRATION-DEBT.tsv";

/// The columns the list is written in, in order.
const DEBT_COLUMNS: [&str; 6] = [
    "file",
    "owner",
    "mechanism",
    "subject",
    "ticket",
    "disturbed_by_2a",
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
/// does not accept the forward slashes both lists are keyed by, so
/// `root.join("scripts/check-portable-core.ps1")` would stop naming a file that
/// is plainly there. The `..` components are left in; every path the scan
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
    allowed: &BTreeMap<&str, FileScoped>,
) -> Vec<&'a Hit> {
    hits.iter()
        .filter(|hit| !debt.contains(&hit.file) && !allowed.contains_key(hit.file.as_str()))
        .collect()
}

/// What the test says when it fires: the hit, and both lists.
fn complaint(
    strays: &[&Hit],
    debt: &BTreeSet<String>,
    allowed: &BTreeMap<&str, FileScoped>,
) -> String {
    let mut said = String::new();
    let _ = writeln!(
        said,
        "{} source reader(s) name a file and are on neither list:\n",
        strays.len()
    );
    for stray in strays {
        let _ = writeln!(said, "    {}", stray.named());
    }
    let _ = writeln!(
        said,
        "\nThe two lists are:\n\n  * {DEBT_LIST} — the readers not yet migrated, {} files today. \
         It only shrinks: a row is removed by the ticket that migrates its reader, and \
         scripts/ci/check-migration-debt.ps1 refuses an added one.\n  * bt_source::FileScoped — \
         the readers whose concern really is a file, {} entries today:",
        debt.len(),
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
    assert!(
        hits.len() > 200,
        "the scan found {} readers, which is not this workspace — it read {} as its root",
        hits.len(),
        root.display()
    );

    let debt = debt_files(&root);
    let allowed = allowlist();
    let strays = unlisted(&hits, &debt, &allowed);
    println!(
        "{} hits over {} files; {} on the debt list, {} on the allowlist",
        hits.len(),
        hits.iter()
            .map(|hit| &hit.file)
            .collect::<BTreeSet<_>>()
            .len(),
        debt.len(),
        allowed.len()
    );
    assert!(strays.is_empty(), "{}", complaint(&strays, &debt, &allowed));
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
    let allowed = allowlist();
    let strays = unlisted(&hits, &debt, &allowed);
    assert_eq!(
        strays.len(),
        hits.len(),
        "a planted reader was taken for a listed one"
    );

    let said = complaint(&strays, &debt, &allowed);
    assert!(
        said.contains("crates/planted/src/lib.rs")
            && said.contains(DEBT_LIST)
            && said.contains("bt_source::FileScoped")
            && said.contains(FileScoped::PortableCoreArray.path()),
        "the refusal has to name the hit and both lists; it said:\n{said}"
    );

    // And the same plant *on* a list is not a stray — which is the other half of
    // the claim, and the reason a migration ticket can delete a row and see this
    // test start covering the file it emptied.
    let listed = BTreeSet::from(["crates/planted/src/lib.rs".to_owned()]);
    assert!(unlisted(&hits, &listed, &BTreeMap::new()).is_empty());

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
/// it**: six tab-separated fields a row, a known ticket, a yes-or-no answer
/// about Step 2a.
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
    let rows = debt_rows(&workspace_root());
    assert!(
        rows.len() > 500,
        "the debt list has {} rows, which is not the census it was seeded from",
        rows.len()
    );
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
         fn scoped() -> bt_source::Scope {{ {scope}bt_source::FileScoped::PortableCoreArray) }}\n\
         fn walk() {{\n    \
             let root = std::path::Path::new(env!(\"{MANIFEST_DIRECTORY}\")).join(\"src\");\n    \
             for entry in std::fs::{DIRECTORY_READ}(&root).unwrap().flatten() {{\n        \
                 if entry.path().extension().is_some_and(|kind| kind == {RUST_EXTENSION}) {{}}\n    \
             }}\n\
         }}\n"
    )
}

/// A directory of our own under the machine's temporary one.
///
/// No `tempfile`: this crate has no dev-dependencies and one test does not earn
/// the first. The shape is `bt_term`'s own `temporary_directory`, process id and
/// clock, which is enough for a directory two runs of one suite will not share.
fn temporary_directory(name: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock set after 1970")
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("bt-source-{name}-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&path).expect("a directory of our own");
    path
}
