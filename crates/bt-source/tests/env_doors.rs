//! **No library reads a `BT_` door from the environment** — T-DIAGNOSTIC-DOORS-AT-THE-ENTRY, the
//! guard half of "the entry reads every door once and hands the answer down".
//!
//! Guard (source-reading by design): its subject is where the workspace's product code reads the
//! process environment, so it reads that code through `bt-source`'s index and binds to no file.
//!
//! # Why
//!
//! A `BT_` variable is the person's shell speaking to one run of the program: a system boundary.
//! Read by library code at the moment of use, it is honoured by every binary that reaches that
//! code — a test binary as readily as the product — and on 2026-10-15/17 a shell that still had
//! `BT_PTY_DUMP` and `BT_CARD_TRACE` set made every test that started a real pane write about
//! six gigabytes of recordings into the folder they named. So the product's entry reads them
//! (`bt_app::entry_doors`), and this guard holds every other item to reading none.
//!
//! # What is read
//!
//! **Product code** of every first-party package (the vendored crates are not the workspace's to
//! hold; `vendor/portable-pty` reads `BT_CONPTY_FORCE_SYSTEM` and is out of scope, as a limit):
//! every item a shipped build compiles — not `cfg(test)`, not a module under it, not an
//! integration test, not a build script, not an item standing on a `cfg` that names `test`
//! (`any(test, feature = "test-shell")` is a test helper a feature also exposes). A test reads
//! its own child-process markers, which are the test's own channel and not a door of the
//! person's.
//!
//! # The grammar
//!
//! Every identifier token `var`, `var_os`, `vars` or `vars_os` is one of these, judged by the
//! tokens around it:
//!
//! 1. **`std::env::var_os(<string literal>)`** or **`std::env::var(<string literal>)`** — a read
//!    of the name the literal spells. Also written `::std::env::…`.
//! 2. **`env::var_os(…)`** / **`env::var(…)`** in a file with a module-level `use std::env;`.
//! 3. **The bare call `var_os(…)`** / **`var(…)`** in a file whose module-level `use std::env::…`
//!    brings that name in under its own name (`use std::env::{var, var_os};`).
//! 4. In 1–3, the argument may instead be **`<IDENT>`**, a `const <IDENT>: &str = "<literal>";`
//!    (or `&'static str`) written in the same file, exactly one value for that name there.
//! 5. **Not an environment read at all**, and not judged: a method call (`.var_os(`), a
//!    declaration (`fn var_os(`), a bare name the file does not import from `std::env` (a local
//!    `vars`), and the name inside a module-level `use` that imports it under its own name.
//!
//! **Everything else that names one of the four is a refusal**, by file and line, never a silent
//! skip: a computed argument (a parameter, a `format!`, a constant of another file or module, a
//! macro's `$name`), `vars()`/`vars_os()` (an enumeration of the whole environment), the function
//! handed on as a value (`.map(std::env::var_os)`), a path through another module
//! (`other::var_os(…)`, `env::var_os` in a file with no `use std::env;`), a comment between the
//! tokens, and a `use` that renames one of them or globs `std::env::*`. Extending the grammar is
//! another ticket.
//!
//! # The list
//!
//! `docs/plans/ENV-DOORS.tsv` names every product item that reads a `BT_` name in a supported
//! shape or holds a refused shape, keyed by file and item, and the list is an **equality**: every
//! such item has exactly its row, the row's names are exactly the `BT_` names the item reads, its
//! `refused` count is exactly the refusals in it, and a row whose item reads nothing is stale.
//! Its categories: `entry` (a program's entry: `bt_app::entry_doors` and the tools' own entries),
//! `door` (a behaviour or test door not moved to the entry yet; the ticket column names the
//! follow-up that moves it) and `unjudged` (a refused shape reading names that are not `BT_` doors;
//! the reason says what it reads). **The list only shrinks**: a new reader is moved to the entry
//! rather than listed, and review refuses an added row.
//!
//! # What it does not see
//!
//! A read in a `use` inside a function body (`use std::env;` written in the body) is judged as a
//! path through another module and refused — noise, never silence. An alias of `std` itself
//! (`use std as s;`) and a re-export of `std::env` under another name from another module reach
//! the environment through a path this grammar refuses as "another module" only when the final
//! segment is still one of the four names. Review owns those.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use bt_source::{
    DiskScope, Index, ItemRecord, LiteralValue, Package, Span, TargetId, TargetKind, TargetRoot,
    TokenRecord, Universe, Vendor, Workspace, is_vendored, report, universes,
};

/// The list, from the workspace root.
const LIST: &str = "docs/plans/ENV-DOORS.tsv";

/// The list's column header.
const HEADER: &str = "file\titem\tcategory\tnames\trefused\tticket\treason";

/// The four names of `std::env` that read the environment.
const READERS: [&str; 4] = ["var", "var_os", "vars", "vars_os"];

/// The categories a row may have.
const CATEGORIES: [&str; 3] = ["entry", "door", "unjudged"];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The whole of a first-party package; `bt-source`'s `tests/fixtures/` are source *about*
/// compilation and are declared out.
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

/// Whether every declaration of `item` is test code (see the module header).
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

/// One environment read the scan found, or one shape it refuses.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Finding {
    /// A read of this name in a supported shape.
    Read(String),
    /// A shape the grammar does not read, and why.
    Refused(&'static str),
}

/// What one product item reads: its `BT_` names, and its refusals with their sites.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Reads {
    names: BTreeSet<String>,
    refused: Vec<String>,
}

/// The scan of one index: `(file, item)` → what it reads, for every product item that reads a
/// `BT_` name or holds a refused shape. `file` is relative to `root`, with `/`.
fn scan(index: &Index, root: &Path) -> BTreeMap<(String, String), Reads> {
    let tokens = index.tokens();
    let literals = index.literals();
    let mut found: BTreeMap<(String, String), Reads> = BTreeMap::new();
    for (at, token) in tokens.iter().enumerate() {
        let name = index.text(token.name_span());
        if !READERS.contains(&name) {
            continue;
        }
        let Some(file) = index.file_at(token.span().start()) else {
            continue;
        };
        let item = innermost_item(index, token.span());
        let in_test = match item {
            Some(item) => is_test_code(item),
            None => file.is_wholly_test() || !file.permits_product(),
        };
        if in_test {
            continue;
        }
        let Some(finding) = judge(index, tokens, literals, at, file.span()) else {
            continue;
        };
        let key = (
            relative(file.path(), root),
            item.map_or_else(|| "-".to_owned(), identity_of),
        );
        match finding {
            Finding::Read(read) if read.starts_with("BT_") => {
                found.entry(key).or_default().names.insert(read);
            }
            Finding::Read(_) => {}
            Finding::Refused(why) => {
                let place = index
                    .locate(token.span().start())
                    .map_or_else(String::new, |location| {
                        format!("{}:{}", relative(location.file, root), location.line)
                    });
                found
                    .entry(key)
                    .or_default()
                    .refused
                    .push(format!("{place} {why}"));
            }
        }
    }
    found
}

/// What the token at `at` is, by the grammar of the module header; `None` when it is no
/// environment read at all (rule 5).
fn judge(
    index: &Index,
    tokens: &[TokenRecord],
    literals: &[bt_source::LiteralRecord],
    at: usize,
    file: Span,
) -> Option<Finding> {
    let token = tokens[at];
    let name = index.text(token.name_span());
    let before = gap_before(index, tokens, at, file);
    let previous = at
        .checked_sub(1)
        .map(|index_of| tokens[index_of])
        .filter(|previous| previous.span().start() >= file.start());
    let previous_name = previous.map(|previous| index.text(previous.name_span()));
    if before.ends_with('.') || (previous_name == Some("fn") && before.is_empty()) {
        return None;
    }
    if let Some(finding) = in_a_use(index, token, name) {
        return finding;
    }
    let supported_path = if before == "::" {
        match previous_name {
            Some("env") => {
                let env_at = at - 1;
                let before_env = gap_before(index, tokens, env_at, file);
                let qualified = before_env == "::"
                    && env_at
                        .checked_sub(1)
                        .map(|std_at| index.text(tokens[std_at].name_span()) == "std")
                        .unwrap_or(false);
                if qualified {
                    // `std` itself a segment of a longer path (`x::std::env`) is another module.
                    gap_before(index, tokens, env_at - 1, file) != "::"
                } else {
                    before_env != "::" && imports(index, file, "env", &["std", "env"])
                }
            }
            _ => false,
        }
    } else {
        if !imports(index, file, name, &["std", "env", name]) {
            // A bare name this file does not bring in from `std::env` is not the standard
            // library's (rule 5) — unless a glob could have brought it in, which no reading of
            // the name alone can tell.
            return globs_std_env(index, file)
                .then_some(Finding::Refused("a bare reader under a glob of `std::env`"));
        }
        true
    };
    if !supported_path {
        return Some(Finding::Refused("a path this grammar does not read"));
    }
    if name == "vars" || name == "vars_os" {
        return Some(Finding::Refused("an enumeration of the whole environment"));
    }
    let after = &index.union()[token.span().end()..file.end()];
    let trimmed = after.trim_start();
    if !trimmed.starts_with('(') {
        return Some(Finding::Refused("the reader handed on as a value"));
    }
    let open = token.span().end() + (after.len() - trimmed.len());
    let argument_start = open + 1;
    let rest = &index.union()[argument_start..file.end()];
    let argument_at = argument_start + (rest.len() - rest.trim_start().len());
    if let Some(literal) = literals
        .iter()
        .find(|literal| literal.span().start() == argument_at)
    {
        return Some(match literal.value() {
            LiteralValue::Str(value) if closes(index, literal.span().end(), file) => {
                Finding::Read(value.clone())
            }
            _ => Finding::Refused("an argument that is not one string literal"),
        });
    }
    if let Some(next) = tokens.get(at + 1)
        && next.span().start() == argument_at
        && closes(index, next.span().end(), file)
    {
        let constant = index.text(next.name_span());
        return Some(
            match constant_in_file(index, tokens, literals, constant, file) {
                Some(value) => Finding::Read(value),
                None => Finding::Refused("a name that is no single string constant of this file"),
            },
        );
    }
    Some(Finding::Refused("a computed argument"))
}

/// The bytes between the token before `at` (in this file) and the token at `at`, trimmed — or
/// the bytes from the file's start when it is the file's first token.
fn gap_before<'a>(index: &'a Index, tokens: &[TokenRecord], at: usize, file: Span) -> &'a str {
    let start = at
        .checked_sub(1)
        .map(|previous| tokens[previous].span().end())
        .filter(|end| *end >= file.start())
        .unwrap_or(file.start());
    index.union()[start..tokens[at].span().start()].trim()
}

/// Whether a module-level `use` of the file holding `file` globs `std::env::*`.
fn globs_std_env(index: &Index, file: Span) -> bool {
    index.uses().iter().any(|record| {
        record.whole().within(file)
            && record
                .globs()
                .iter()
                .any(|glob| glob.as_slice() == ["std", "env"])
    })
}

/// Whether the bytes from `end` close the call: `)` after any whitespace.
fn closes(index: &Index, end: usize, file: Span) -> bool {
    index.union()[end..file.end()].trim_start().starts_with(')')
}

/// The innermost item whose bytes hold `span`: a callable before any other kind.
fn innermost_item(index: &Index, span: Span) -> Option<&ItemRecord> {
    let holding = || {
        index
            .items()
            .iter()
            .filter(move |item| span.within(item.whole()))
    };
    holding()
        .filter(|item| item.kind().is_callable())
        .min_by_key(|item| item.whole().len())
        .or_else(|| holding().min_by_key(|item| item.whole().len()))
}

/// An item's first identity, as `module::Type::name`.
fn identity_of(item: &ItemRecord) -> String {
    let identity = item
        .identities()
        .min_by(|left, right| left.module_path.cmp(&right.module_path))
        .expect("an item has an identity");
    match identity.type_owner {
        Some(owner) => format!("{}::{owner}::{}", identity.module_path, identity.name),
        None => format!("{}::{}", identity.module_path, identity.name),
    }
}

/// Whether a module-level `use` of the file holding `file` brings `name` in as `path`.
fn imports(index: &Index, file: Span, name: &str, path: &[&str]) -> bool {
    index.uses().iter().any(|record| {
        record.whole().within(file)
            && record
                .names()
                .iter()
                .any(|(brought, written)| brought == name && written == path)
    })
}

/// `Some(answer)` when `token` is inside a module-level `use`: nothing for one that imports a
/// reader under its own name (rule 5), a refusal for one that renames it or globs `std::env`.
fn in_a_use(index: &Index, token: TokenRecord, name: &str) -> Option<Option<Finding>> {
    let record = index
        .uses()
        .iter()
        .find(|record| token.span().within(record.whole()))?;
    let renames = record
        .names()
        .iter()
        .any(|(brought, written)| brought != name && written.as_slice() == ["std", "env", name]);
    Some(renames.then_some(Finding::Refused("a use that renames a reader")))
}

/// The value of the one `const <name>: &str = "<literal>";` written in `file`, if there is
/// exactly one value for that name there.
fn constant_in_file(
    index: &Index,
    tokens: &[TokenRecord],
    literals: &[bt_source::LiteralRecord],
    name: &str,
    file: Span,
) -> Option<String> {
    let mut values = BTreeSet::new();
    for (at, token) in tokens.iter().enumerate() {
        if !token.span().within(file) || index.text(token.name_span()) != name || at == 0 {
            continue;
        }
        if index.text(tokens[at - 1].name_span()) != "const"
            || !gap_before(index, tokens, at, file).is_empty()
        {
            continue;
        }
        let after = &index.union()[token.span().end()..file.end()];
        let Some(equals) = after.find('=') else {
            continue;
        };
        let written: String = after[..equals].split_whitespace().collect();
        if written != ":&str" && written != ":&'staticstr" {
            continue;
        }
        let value_from = token.span().end() + equals + 1;
        let rest = &index.union()[value_from..file.end()];
        let value_at = value_from + (rest.len() - rest.trim_start().len());
        let literal = literals
            .iter()
            .find(|literal| literal.span().start() == value_at)?;
        let LiteralValue::Str(value) = literal.value() else {
            return None;
        };
        if !index.union()[literal.span().end()..file.end()]
            .trim_start()
            .starts_with(';')
        {
            return None;
        }
        values.insert(value.clone());
    }
    (values.len() == 1).then(|| values.into_iter().next().expect("one value"))
}

/// `path` relative to `root`, with `/`.
fn relative(path: &Path, root: &Path) -> String {
    let root = bt_source::normalized(root);
    let path = bt_source::normalized(path);
    path.strip_prefix(&root)
        .unwrap_or(&path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Every first-party package's scan, merged.
fn workspace_scan() -> BTreeMap<(String, String), Reads> {
    let root = workspace_root();
    let workspace = Workspace::read(&root).expect("this workspace");
    let mut found = BTreeMap::new();
    for package in workspace
        .packages()
        .iter()
        .filter(|package| !is_vendored(package.directory()))
    {
        let index = Index::build(&universe_for(package))
            .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
        found.extend(scan(&index, &root));
    }
    found
}

/// One row of the list: what it says the item reads. Its category, ticket and reason are checked
/// for shape as the list is read.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    names: BTreeSet<String>,
    refused: usize,
}

/// The committed list, by `(file, item)`.
fn committed() -> BTreeMap<(String, String), Row> {
    let text = std::fs::read_to_string(workspace_root().join(LIST)).expect("the list is committed");
    let mut lines = text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    assert_eq!(lines.next(), Some(HEADER), "{LIST}'s column header");
    let mut rows = BTreeMap::new();
    for line in lines {
        let columns: Vec<&str> = line.split('\t').collect();
        assert_eq!(columns.len(), 7, "{LIST}: a row is seven columns: {line}");
        assert!(
            CATEGORIES.contains(&columns[2]),
            "{LIST}: `{}` is not one of {CATEGORIES:?}: {line}",
            columns[2]
        );
        assert!(
            !columns[5].is_empty() && !columns[6].is_empty(),
            "{LIST}: a row names its ticket and its reason: {line}"
        );
        let names = if columns[3] == "-" {
            BTreeSet::new()
        } else {
            columns[3].split(',').map(str::to_owned).collect()
        };
        let row = Row {
            names,
            refused: columns[4]
                .parse()
                .unwrap_or_else(|_| panic!("{LIST}: `refused` is a count: {line}")),
        };
        assert!(
            columns[2] != "unjudged" || row.names.is_empty(),
            "{LIST}: an `unjudged` row reads no `BT_` name in a supported shape: {line}"
        );
        assert!(
            rows.insert((columns[0].to_owned(), columns[1].to_owned()), row)
                .is_none(),
            "{LIST} names {} {} twice",
            columns[0],
            columns[1]
        );
    }
    rows
}

/// The differences between a scan and a list, one sentence each.
fn differences(
    found: &BTreeMap<(String, String), Reads>,
    rows: &BTreeMap<(String, String), Row>,
) -> Vec<String> {
    let mut said = Vec::new();
    for ((file, item), reads) in found {
        let names = if reads.names.is_empty() {
            "-".to_owned()
        } else {
            reads.names.iter().cloned().collect::<Vec<_>>().join(",")
        };
        match rows.get(&(file.clone(), item.clone())) {
            None => said.push(format!(
                "{file} {item} reads {names} with {} refused shape(s) and has no row:\n    {}",
                reads.refused.len(),
                reads.refused.join("\n    ")
            )),
            Some(row) if row.names != reads.names || row.refused != reads.refused.len() => {
                said.push(format!(
                    "{file} {item} reads {names} with {} refused shape(s); its row says {} \
                     with {}:\n    {}",
                    reads.refused.len(),
                    row.names.iter().cloned().collect::<Vec<_>>().join(","),
                    row.refused,
                    reads.refused.join("\n    ")
                ));
            }
            Some(_) => {}
        }
    }
    for (file, item) in rows.keys() {
        if !found.contains_key(&(file.clone(), item.clone())) {
            said.push(format!(
                "{file} {item} has a row and reads no `BT_` name and holds no refused shape: \
                 the row is stale; remove it"
            ));
        }
    }
    said
}

/// The fixture, indexed as a library of its own.
fn fixture() -> (Index, PathBuf) {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("env_doors");
    let universe = Universe::declare(
        "the env_doors fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "env_doors".to_owned(),
                kind: TargetKind::Library,
                name: "env_doors".to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    (
        Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections))),
        directory,
    )
}

/// RED (T-DIAGNOSTIC-DOORS-AT-THE-ENTRY) — **every product item that reads a `BT_` name from the
/// environment, or holds a shape this guard cannot read, is exactly its row of
/// `docs/plans/ENV-DOORS.tsv`.**
///
/// MUTATION: put `std::env::var_os("BT_CARD_TRACE")` back into `bt_app::trace`'s `Gate::get` —
/// red, naming `crates/bt-app/src/trace.rs crate::trace::Gate::get` as a reader with no row;
/// put `std::env::var_os(PTY_DUMP_ENV)` back into `bt_pty`'s `PtyDump::for_request` — red the
/// same way, the constant resolved to `BT_PTY_DUMP`.
#[test]
fn no_library_reads_a_bt_door_outside_the_entry_list() {
    let found = workspace_scan();
    let rows = committed();
    let said = differences(&found, &rows);
    if !said.is_empty() {
        // What the tree reads, row by row, for the author who has to answer: every item with its
        // names, its refusal count and the refused sites.
        let out = workspace_root().join("target").join("env-doors");
        std::fs::create_dir_all(&out).expect("the target folder");
        let text: String = found
            .iter()
            .map(|((file, item), reads)| {
                let names = if reads.names.is_empty() {
                    "-".to_owned()
                } else {
                    reads.names.iter().cloned().collect::<Vec<_>>().join(",")
                };
                format!(
                    "{file}\t{item}\t{names}\t{}\t{}\n",
                    reads.refused.len(),
                    reads.refused.join(" | ")
                )
            })
            .collect();
        std::fs::write(out.join("found.tsv"), text).expect("written");
    }
    assert!(
        said.is_empty(),
        "{LIST} and the product's environment reads disagree. A `BT_` door is read once, at the \
         program's entry (`bt_app::entry_doors`), and handed to the code that writes it as a \
         parameter or an installed answer; a library never reads one. Move the read to the \
         entry rather than adding a row (the list only shrinks); a row whose reader has gone is \
         removed. What the tree reads is in target/env-doors/found.tsv:\n  {}",
        said.join("\n  ")
    );
}

/// RED (T-DIAGNOSTIC-DOORS-AT-THE-ENTRY) — **the grammar reads every supported shape and refuses
/// every other one, by line.** The fixture has one function per shape; the scan answers exactly
/// this table.
///
/// MUTATION: make `judge` answer `None` for a computed argument — `computed`, `formatted` and
/// `through_another_module` vanish from the table; make `constant_in_file` answer `None` —
/// `through_a_constant` turns into a refusal.
#[test]
fn the_grammar_reads_each_supported_shape_and_refuses_each_other_one() {
    let (index, directory) = fixture();
    let found: BTreeMap<String, (Vec<String>, Vec<String>)> = scan(&index, &directory)
        .into_iter()
        .map(|((_, item), reads)| {
            let refused = reads
                .refused
                .iter()
                .map(|site| {
                    site.split_once(' ')
                        .map_or(site.clone(), |(_, why)| why.to_owned())
                })
                .collect();
            (item, (reads.names.into_iter().collect(), refused))
        })
        .collect();
    let row = |names: &[&str], refused: &[&str]| -> (Vec<String>, Vec<String>) {
        (
            names.iter().map(|name| (*name).to_owned()).collect(),
            refused.iter().map(|why| (*why).to_owned()).collect(),
        )
    };
    let expected: BTreeMap<String, (Vec<String>, Vec<String>)> = [
        (
            "-",
            row(&[], &["a use that renames a reader", "a computed argument"]),
        ),
        (
            "crate::ambiguous",
            row(
                &[],
                &["a name that is no single string constant of this file"],
            ),
        ),
        ("crate::bare", row(&["BT_FIXTURE_BARE"], &[])),
        (
            "crate::computed",
            row(
                &[],
                &["a name that is no single string constant of this file"],
            ),
        ),
        (
            "crate::enumerated",
            row(&[], &["an enumeration of the whole environment"]),
        ),
        ("crate::formatted", row(&[], &["a computed argument"])),
        (
            "crate::globbed::under_a_glob",
            row(&[], &["a bare reader under a glob of `std::env`"]),
        ),
        (
            "crate::handed_on",
            row(&[], &["the reader handed on as a value"]),
        ),
        ("crate::literal", row(&["BT_FIXTURE_LITERAL"], &[])),
        ("crate::literal_os", row(&["BT_FIXTURE_LITERAL_OS"], &[])),
        (
            "crate::other_module_path",
            row(&[], &["a path this grammar does not read"]),
        ),
        (
            "crate::through_another_module",
            row(&[], &["a computed argument"]),
        ),
        (
            "crate::through_a_constant",
            row(&["BT_FIXTURE_CONSTANT"], &[]),
        ),
        ("crate::through_use", row(&["BT_FIXTURE_USE"], &[])),
    ]
    .into_iter()
    .map(|(item, row)| (item.to_owned(), row))
    .collect();
    assert_eq!(found, expected, "{found:#?}");
}

/// RED (T-DIAGNOSTIC-DOORS-AT-THE-ENTRY) — **the list is an equality**: a reader with no row, a
/// row whose names or count are not the reader's, and a row whose item reads nothing are each
/// named.
///
/// MUTATION: drop the stale-row loop from `differences` — the third sentence is lost.
#[test]
fn a_reader_without_its_row_a_wrong_row_and_a_stale_row_are_each_named() {
    let key = |item: &str| ("crates/x/src/lib.rs".to_owned(), item.to_owned());
    let reads = |names: &[&str], refused: usize| Reads {
        names: names.iter().map(|name| (*name).to_owned()).collect(),
        refused: (0..refused)
            .map(|line| format!("x:{line} refused"))
            .collect(),
    };
    let row = |names: &[&str], refused: usize| Row {
        names: names.iter().map(|name| (*name).to_owned()).collect(),
        refused,
    };
    let found = BTreeMap::from([
        (key("crate::listed"), reads(&["BT_A"], 0)),
        (key("crate::unlisted"), reads(&["BT_B"], 0)),
        (key("crate::miscounted"), reads(&[], 2)),
    ]);
    let rows = BTreeMap::from([
        (key("crate::listed"), row(&["BT_A"], 0)),
        (key("crate::miscounted"), row(&[], 1)),
        (key("crate::gone"), row(&["BT_C"], 0)),
    ]);
    let said = differences(&found, &rows);
    assert_eq!(said.len(), 3, "{said:#?}");
    assert!(said[0].contains("crate::miscounted") && said[0].contains("its row says"));
    assert!(said[1].contains("crate::unlisted") && said[1].contains("has no row"));
    assert!(said[2].contains("crate::gone") && said[2].contains("stale"));
}
