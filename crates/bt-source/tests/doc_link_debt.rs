//! **Every broken intra-doc link in bt-app's documentation is a row of
//! `docs/plans/DOC-LINK-DEBT.tsv`, and every row is one** — T-DOC-LINK-DEBT (0.4.9 plan row 78).
//!
//! The list's other half, `scripts/ci/check-doc-link-debt.ps1`, refuses a row the merge base did
//! not have. This half holds the list equal to what rustdoc reports: a broken link the list does
//! not hold is red here, and so is a row no broken link is (a fixed link leaves the list in the
//! commit that fixes it). Between them the list only shrinks.
//!
//! # How it runs
//!
//! **Only by name, after the measurement.** The manifest declares this target `test = false`,
//! so `cargo test -p bt-source` neither builds nor runs it. `scripts/ci/measure-doc-links.ps1`
//! runs `cargo doc -p bt-app --no-deps --locked --message-format=json`, writes each warning it
//! keeps to `target/doc-links/measured.tsv`, then runs `cargo test -p bt-source --locked --test
//! doc_link_debt`. The test that reads that file fails when it is not there: it is never run
//! without the measurement, and a check that read nothing must not look like one that agreed.
//! The fixture tests below need no measurement and run with it.
//!
//! # The grammar
//!
//! **Which warnings.** The measuring script keeps the `compiler-message`s whose lint code
//! (`message.code.code`) is `rustdoc::broken_intra_doc_links`, and nothing else; the other rustdoc
//! lints it counts and prints, ungated (ruling R6). Of that lint, three message forms are read
//! here, each naming its target in its first backtick pair:
//!
//! * `unresolved link to `X``: the shape `unresolved`;
//! * `` `X` is both a … and a … ``: the shape `ambiguous`;
//! * `incompatible link kind for `X``: the shape `incompatible`.
//!
//! Any other message under that code is refused by name: a form this test does not read is not
//! a row.
//!
//! **Where a warning stands.** Its primary span's file, line and column (one-based, the column in
//! characters), read as a byte of bt-app's index (`Index::of_package("bt-app")`). The byte must be
//! inside a doc comment (`///`, `//!`, `/** */`, `/*! */`) or a written `#[doc = …]`: rustdoc
//! points at the link, or at the first line of the doc block when it cannot place the link inside
//! it, and either is doc text. The owner is the **innermost record whose whole span holds the
//! byte** — attributes included, so an item's outer doc is inside it:
//!
//! * a function, an associated function, a struct, an enum, a union, a field or a variant
//!   (`ItemRecord`), in its declaration (before its body);
//! * a `const` or `static` of a module, an `impl` block or a trait (`ValueRecord`);
//! * a type alias (`AliasRecord`);
//! * an `impl` block or a `trait`, in its header (before its braces);
//! * **no record at all**: `<module> (outside any item)`, the module being the innermost inline
//!   module holding the byte, else the file's own. That is a `//!` module doc, the outer doc of a
//!   `mod x;` declaration (it documents the child but stands in the parent), and the doc of a
//!   module-level item the index keeps no record for (`macro_rules!`, `use`, `extern`).
//!
//! **Refused, never a silent row** — each by its file, line and why:
//!
//! * a file the index did not lower, or one under no `crates/bt-app/src`;
//! * a file reached by no product declaration path, or by more than one (two owners);
//! * a line or column past the file's end;
//! * a byte inside no doc comment or doc attribute;
//! * a byte inside a macro's token tree (the parse does not see the items a macro makes);
//! * a byte inside a function's body (a local item's doc), inside a data type's or a variant's
//!   body but in no field, or inside an `impl` block's or a trait's braces but in no member the
//!   index records (an associated type, a macro call);
//! * a record whose module paths name none, or more than one, under the file's product module.
//!
//! # The key
//!
//! A row is `owner<TAB>target<TAB>shape<TAB>module<TAB>file`, one per warning, sorted: two
//! warnings of one owner and target are two rows. `owner` is the module path, then the item as
//! `ItemIdentity` prints it (`Type::name`, `<Type as Trait>::name`, `Trait::name`), an `impl`
//! block as `impl Type` or `impl Trait for Type`, then the predicates written inside the file on
//! the item, its `impl` block and its inline modules (` #[cfg(windows)]`) when there are any.
//! `module` is the module the owner is written in; `file` is where it is written today, under
//! `crates/bt-app/src`, for the reader. No line numbers: a move inside a file changes no row,
//! and a move to another module is a row moved (T-MOVED-NOT-ADDED-RULE), which the shrink check
//! reads by `module`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bt_source::{
    CommentKind, ConditionalVariant, DiskScope, FileRecord, Index, ModuleShape, Span, TargetId,
    TargetKind, TargetRoot, Universe, Vendor, normalized, report,
};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The committed list, from the workspace root.
const LIST: &str = "docs/plans/DOC-LINK-DEBT.tsv";

/// The header of the list as this test renders it. The generator writes the stamp line,
/// `# measured at <sha>`, under its first two lines; the comparison skips it.
const HEADER: &str = "\
# DOC-LINK-DEBT - every broken intra-doc link in bt-app's documentation, one row per warning
# (T-DOC-LINK-DEBT, 0.4.9 plan row 78).
#
# Measured by scripts/ci/measure-doc-links.ps1: `cargo doc -p bt-app --no-deps --locked
# --message-format=json` on Windows, every warning whose lint code is
# `rustdoc::broken_intra_doc_links`, each keyed to the item whose documentation holds it by
# bt-source's `doc_link_debt` test target (its module doc states the grammar). Written by
# scripts/dev/generate-doc-link-debt.ps1; do not edit by hand.
#
# **This list only shrinks.** scripts/ci/check-doc-link-debt.ps1 refuses a row the merge base
# does not have; the `docs` CI job refuses a broken link the list does not hold and a row no
# broken link is. A link is fixed (or reworded) and its row leaves in the same commit.
# A row is `owner`, `target` and `shape`; `module` is the module the owner is written in, so an
# owner moved to another module is its rows moved (T-MOVED-NOT-ADDED-RULE); `file` is where it is
# written today and is not compared. When the list is empty, rustdoc's lint is denied in the
# workspace and the list retires.
owner\ttarget\tshape\tmodule\tfile
";

/// The stamp line's prefix, which the comparison does not read.
const STAMP: &str = "# measured at ";

/// The spelling of a warning in no record.
const OUTSIDE: &str = " (outside any item)";

/// One warning as the measuring script wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Warning {
    /// As rustdoc names it: relative to the workspace root, or absolute.
    file: String,
    line: usize,
    column: usize,
    message: String,
}

/// One row of the list.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Row {
    owner: String,
    target: String,
    shape: &'static str,
    module: String,
    file: String,
}

impl Row {
    fn line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}",
            self.owner, self.target, self.shape, self.module, self.file
        )
    }
}

/// The shape and the target of a message of `rustdoc::broken_intra_doc_links`, or why it is not
/// one of the three forms.
fn shape_of(message: &str) -> Result<(&'static str, String), String> {
    let first_pair = |text: &str| -> Option<String> {
        let open = text.find('`')?;
        let rest = &text[open + 1..];
        let close = rest.find('`')?;
        Some(rest[..close].to_owned()).filter(|target| !target.is_empty())
    };
    let shape = if message.starts_with("unresolved link to `") {
        "unresolved"
    } else if message.starts_with("incompatible link kind for `") {
        "incompatible"
    } else if message.starts_with('`') && message.contains("` is both ") {
        "ambiguous"
    } else {
        return Err(format!(
            "a message of rustdoc::broken_intra_doc_links in no form this test reads: {message}"
        ));
    };
    first_pair(message)
        .map(|target| (shape, target))
        .ok_or_else(|| format!("a message with no target in backticks: {message}"))
}

/// The union offset of a one-based line and character column of `file`.
fn offset_of(index: &Index, file: &FileRecord, line: usize, column: usize) -> Option<usize> {
    let text = index.text(file.span());
    let mut start = 0;
    for _ in 1..line {
        start += text[start..].find('\n')? + 1;
    }
    let rest = &text[start..];
    let end = rest.find('\n').unwrap_or(rest.len());
    let at = match column.checked_sub(1)? {
        0 => 0,
        skip => rest[..end]
            .char_indices()
            .nth(skip)
            .map(|(at, _)| at)
            .or((rest[..end].chars().count() == skip).then_some(end))?,
    };
    Some(file.span().start() + start + at)
}

/// A record that can hold a warning, as the grammar names them.
enum Holder<'a> {
    Item(&'a bt_source::ItemRecord),
    Value(&'a bt_source::ValueRecord),
    Alias(&'a bt_source::AliasRecord),
    Impl(&'a bt_source::ImplRecord),
    Trait(&'a bt_source::TraitRecord),
}

impl Holder<'_> {
    fn whole(&self) -> Span {
        match self {
            Holder::Item(record) => record.whole(),
            Holder::Value(record) => record.whole(),
            Holder::Alias(record) => record.whole(),
            Holder::Impl(record) => record.whole(),
            Holder::Trait(record) => record.whole(),
        }
    }

    fn module_paths(&self) -> Vec<&str> {
        match self {
            Holder::Item(record) => record.module_paths(),
            Holder::Value(record) => record.module_paths(),
            Holder::Alias(record) => record.module_paths(),
            Holder::Impl(record) => record.module_paths(),
            Holder::Trait(record) => record.module_paths(),
        }
    }

    fn variant(&self) -> &ConditionalVariant {
        match self {
            Holder::Item(record) => record.variant(),
            Holder::Value(record) => record.variant(),
            Holder::Alias(record) => record.variant(),
            Holder::Impl(record) => record.variant(),
            Holder::Trait(record) => record.variant(),
        }
    }

    /// The braces (or parentheses) a byte inside of which is in no member of this record.
    fn body(&self) -> Option<(Span, &'static str)> {
        match self {
            Holder::Item(record) => record.body().map(|body| {
                let why = if record.kind().is_callable() {
                    "inside a function's body (a local item's doc)"
                } else {
                    "inside a data type's or a variant's body but in no field"
                };
                (body, why)
            }),
            Holder::Impl(record) => Some((
                record.body(),
                "inside an impl block's braces but in no member the index records",
            )),
            Holder::Trait(record) => Some((
                record.body(),
                "inside a trait's braces but in no member the index records",
            )),
            Holder::Value(_) | Holder::Alias(_) => None,
        }
    }

    /// The item inside its module, as `ItemIdentity` prints it.
    fn tail(&self) -> String {
        let member =
            |owner: Option<&str>, trait_name: Option<&str>, name: &str| match (owner, trait_name) {
                (Some(owner), Some(trait_name)) => format!("<{owner} as {trait_name}>::{name}"),
                (Some(owner), None) => format!("{owner}::{name}"),
                (None, Some(trait_name)) => format!("{trait_name}::{name}"),
                (None, None) => name.to_owned(),
            };
        match self {
            Holder::Item(record) => member(record.type_owner(), record.trait_name(), record.name()),
            Holder::Value(record) => {
                member(record.type_owner(), record.trait_name(), record.name())
            }
            Holder::Alias(record) => record.name().to_owned(),
            Holder::Impl(record) => match record.trait_name() {
                Some(trait_name) => format!("impl {trait_name} for {}", record.type_owner()),
                None => format!("impl {}", record.type_owner()),
            },
            Holder::Trait(record) => record.name().to_owned(),
        }
    }
}

/// Whether `path` is `module` or a module inside it.
fn is_under(path: &str, module: &str) -> bool {
    path == module
        || path
            .strip_prefix(module)
            .is_some_and(|rest| rest.starts_with("::"))
}

/// The row of one warning, or why it is refused. `root` is what a relative file name is
/// relative to; `source` is the folder the `file` column is relative to.
fn key(index: &Index, root: &Path, source: &Path, warning: &Warning) -> Result<Row, String> {
    let at = format!("{}:{}:{}", warning.file, warning.line, warning.column);
    let refuse = |why: &str| Err(format!("{at}: {why}"));
    let (shape, target) = shape_of(&warning.message).map_err(|why| format!("{at}: {why}"))?;
    let path = normalized(&root.join(warning.file.replace('\\', "/")));
    let Some(file) = index.file(&path) else {
        return refuse("a file the index did not lower");
    };
    let Ok(relative) = path.strip_prefix(normalized(source)) else {
        return refuse("a file outside the crate's sources");
    };
    let relative = relative.to_string_lossy().replace('\\', "/");
    let products: Vec<&str> = file
        .owners()
        .iter()
        .filter(|owner| owner.compilation.permits_product())
        .map(|owner| owner.module_path.as_str())
        .collect();
    let [product] = products[..] else {
        return refuse(&format!(
            "a file reached by {} product declaration paths, not one",
            products.len()
        ));
    };
    let Some(offset) = offset_of(index, file, warning.line, warning.column) else {
        return refuse("a line or column past the file's end");
    };
    let in_doc = index.comments().iter().any(|comment| {
        matches!(
            comment.kind(),
            CommentKind::DocComment | CommentKind::DocAttribute
        ) && comment.span().holds(offset)
    });
    if !in_doc {
        return refuse("a position in no doc comment and no doc attribute");
    }
    if index
        .macros()
        .iter()
        .any(|record| record.tokens().holds(offset))
    {
        return refuse("a position inside a macro's token tree");
    }
    let holders = index
        .items()
        .iter()
        .map(Holder::Item)
        .chain(index.values().iter().map(Holder::Value))
        .chain(index.aliases().iter().map(Holder::Alias))
        .chain(index.impls().iter().map(Holder::Impl))
        .chain(index.traits().iter().map(Holder::Trait));
    let innermost = holders
        .filter(|holder| holder.whole().holds(offset))
        .min_by_key(|holder| holder.whole().len());
    let (owner, module) = match innermost {
        Some(holder) => {
            if let Some((body, why)) = holder.body()
                && body.holds(offset)
            {
                return refuse(why);
            }
            let modules: Vec<&str> = holder
                .module_paths()
                .into_iter()
                .filter(|path| is_under(path, product))
                .collect();
            let [module] = modules[..] else {
                return refuse(&format!(
                    "a record with {} module paths under {product}, not one",
                    modules.len()
                ));
            };
            let variant = holder.variant();
            let suffix = if variant.is_unconditional() {
                String::new()
            } else {
                format!(" {variant}")
            };
            (
                format!("{module}::{}{suffix}", holder.tail()),
                module.to_owned(),
            )
        }
        None => {
            let module = index
                .modules()
                .iter()
                .filter(|record| {
                    record.body() == ModuleShape::Inline && record.span().holds(offset)
                })
                .min_by_key(|record| record.span().len())
                .map_or(Ok(product), |record| {
                    let paths: Vec<&str> = record
                        .module_paths()
                        .iter()
                        .map(String::as_str)
                        .filter(|path| is_under(path, product))
                        .collect();
                    match paths[..] {
                        [path] => Ok(path),
                        _ => Err(format!(
                            "{at}: an inline module with {} module paths under {product}, not one",
                            paths.len()
                        )),
                    }
                })?;
            (format!("{module}{OUTSIDE}"), module.to_owned())
        }
    };
    Ok(Row {
        owner,
        target,
        shape,
        module,
        file: relative,
    })
}

/// The list as this test renders it: the header, then one row per key, sorted.
fn render(rows: &[Row]) -> String {
    let mut sorted: Vec<String> = rows.iter().map(Row::line).collect();
    sorted.sort();
    let mut text = HEADER.to_owned();
    for line in sorted {
        text.push_str(&line);
        text.push('\n');
    }
    text
}

/// What differs between the committed list and the rendering, one sentence per line, with
/// multiplicity; the stamp line is not compared.
fn differences(committed: &str, rendered: &str) -> Vec<String> {
    let lines = |text: &str| -> Vec<String> {
        text.replace("\r\n", "\n")
            .lines()
            .filter(|line| !line.starts_with(STAMP))
            .map(str::to_owned)
            .collect()
    };
    let committed = lines(committed);
    let rendered = lines(rendered);
    let count = |lines: &[String]| {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for line in lines {
            *counts.entry(line.clone()).or_default() += 1;
        }
        counts
    };
    let held = count(&committed);
    let found = count(&rendered);
    let mut out = Vec::new();
    for (line, &times) in &found {
        let before = held.get(line).copied().unwrap_or(0);
        for _ in before..times {
            out.push(if line.starts_with('#') || !line.contains('\t') {
                format!("the header says a line the committed list does not: {line}")
            } else {
                format!("a broken link the list does not hold: {line}")
            });
        }
    }
    for (line, &times) in &held {
        let now = found.get(line).copied().unwrap_or(0);
        for _ in now..times {
            out.push(if line.starts_with('#') || !line.contains('\t') {
                format!("the committed list says a header line the rendering does not: {line}")
            } else {
                format!("a row no broken link is (fixed: the row leaves the list): {line}")
            });
        }
    }
    if out.is_empty() && committed != rendered {
        out.push("the same lines in another order".to_owned());
    }
    out
}

/// The measuring script's file: a `file<TAB>line<TAB>column<TAB>message` header, then one
/// warning per line.
fn read_measured(text: &str) -> Result<Vec<Warning>, String> {
    let mut warnings = Vec::new();
    let mut header = false;
    for (number, line) in text.replace("\r\n", "\n").lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !header {
            if line != "file\tline\tcolumn\tmessage" {
                return Err(format!("line {}: not the header: {line}", number + 1));
            }
            header = true;
            continue;
        }
        let fields: Vec<&str> = line.splitn(4, '\t').collect();
        let [file, at, column, message] = fields[..] else {
            return Err(format!("line {}: not four fields: {line}", number + 1));
        };
        let parse = |field: &str| {
            field
                .parse::<usize>()
                .map_err(|_| format!("line {}: not a number: {field}", number + 1))
        };
        warnings.push(Warning {
            file: file.to_owned(),
            line: parse(at)?,
            column: parse(column)?,
            message: message.to_owned(),
        });
    }
    if header {
        Ok(warnings)
    } else {
        Err("no header".to_owned())
    }
}

/// RED (T-DOC-LINK-DEBT) — **every broken intra-doc link in bt-app is a row, and every row is
/// one.** Reads `target/doc-links/measured.tsv`, which `scripts/ci/measure-doc-links.ps1` writes
/// before it runs this; keys each warning by the grammar above; writes the rendering to
/// `target/doc-link-debt.tsv` for `scripts/dev/generate-doc-link-debt.ps1` to copy; and fails on
/// every difference from the committed list. A warning the grammar refuses fails it by name and
/// leaves no rendering: a list with a warning left out is not written.
///
/// MUTATION: write `/// [`planted_no_such_item`]` on any bt-app function, measure, and this
/// names that function and `planted_no_such_item`.
#[test]
fn every_broken_doc_link_is_a_row_and_every_row_a_broken_link() {
    let root = workspace_root();
    let measured = root.join("target").join("doc-links").join("measured.tsv");
    let rendering = root.join("target").join("doc-link-debt.tsv");
    if rendering.exists() {
        std::fs::remove_file(&rendering).expect("target/ is writable");
    }
    let text = std::fs::read_to_string(&measured).unwrap_or_else(|error| {
        panic!(
            "{}: {error} - this target is run by scripts/ci/measure-doc-links.ps1, after it \
             measures; it reads nothing else",
            measured.display()
        )
    });
    let warnings =
        read_measured(&text).unwrap_or_else(|why| panic!("{}: {why}", measured.display()));
    let index = Index::of_package("bt-app");
    let source = root.join("crates").join("bt-app").join("src");
    let mut rows = Vec::new();
    let mut refused = Vec::new();
    for warning in &warnings {
        match key(index, &root, &source, warning) {
            Ok(row) => rows.push(row),
            Err(why) => refused.push(why),
        }
    }
    assert!(
        refused.is_empty(),
        "{} of {} warning(s) stand where the grammar does not read (this target's module doc); \
         no rendering is written:\n{}",
        refused.len(),
        warnings.len(),
        refused
            .iter()
            .map(|why| format!("  {why}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let rendered = render(&rows);
    std::fs::write(&rendering, rendered.as_bytes()).expect("target/ is writable");
    let committed = std::fs::read_to_string(root.join(LIST)).unwrap_or_default();
    let found = differences(&committed, &rendered);
    println!(
        "{} broken intra-doc link(s) measured, {} row(s) rendered",
        warnings.len(),
        rows.len()
    );
    assert!(
        found.is_empty(),
        "{} difference(s) between {LIST} and the broken links rustdoc reports; a fixed link's \
         row leaves the list, a new broken link is fixed (never listed), and a moved owner's \
         rows are regenerated with scripts/dev/generate-doc-link-debt.ps1:\n{}",
        found.len(),
        found
            .iter()
            .take(60)
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// ── the fixture ────────────────────────────────────────────────────────────────────────────

/// `tests/fixtures/doc_link_debt/`, indexed as a library of its own, and its folder.
fn fixture() -> (Index, PathBuf) {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("doc_link_debt");
    let universe = Universe::declare(
        "the doc_link_debt fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "doc_link_debt".to_owned(),
                kind: TargetKind::Library,
                name: "doc_link_debt".to_owned(),
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

/// A warning at the link to `marker` in the fixture's one file, as rustdoc would name it from
/// `directory`: the line and column of the marker's first character.
fn warning_at(index: &Index, directory: &Path, marker: &str, message: &str) -> Warning {
    let file = &index.files()[0];
    let text = index.text(file.span());
    let link = format!("[`{marker}`]");
    let at = text
        .find(&link)
        .unwrap_or_else(|| panic!("the fixture links {marker}"))
        + 2;
    let line = text[..at].matches('\n').count() + 1;
    let line_start = text[..at].rfind('\n').map_or(0, |newline| newline + 1);
    let column = text[line_start..at].chars().count() + 1;
    let name = normalized(file.path());
    let relative = name
        .strip_prefix(normalized(directory))
        .expect("the fixture's file is in its folder");
    Warning {
        file: relative.to_string_lossy().into_owned(),
        line,
        column,
        message: message.to_owned(),
    }
}

/// The row of an unresolved link to `marker`, keyed in the fixture.
fn keyed(index: &Index, directory: &Path, marker: &str) -> Result<Row, String> {
    let message = format!("unresolved link to `{marker}`");
    key(
        index,
        directory,
        directory,
        &warning_at(index, directory, marker, &message),
    )
}

/// Every supported position names the owner the grammar says, and its module.
///
/// MUTATION: drop the attributes from an item's whole span in the index, and the outer-doc
/// rows here become `crate (outside any item)`.
#[test]
fn each_supported_position_names_its_owner_and_module() {
    let (index, directory) = fixture();
    let cases = [
        ("crate_doc_marker", "crate (outside any item)", "crate"),
        ("free_fn_marker", "crate::free", "crate"),
        ("struct_marker", "crate::Pane", "crate"),
        ("field_marker", "crate::Pane::width", "crate"),
        ("variant_marker", "crate::Seat::Pane", "crate"),
        ("method_marker", "crate::Pane::grow", "crate"),
        ("assoc_const_marker", "crate::Pane::LIMIT", "crate"),
        ("impl_marker", "crate::impl Default for Pane", "crate"),
        ("trait_marker", "crate::Grow", "crate"),
        ("default_method_marker", "crate::Grow::grow_by", "crate"),
        ("const_marker", "crate::WIDTH", "crate"),
        ("static_marker", "crate::NAME", "crate"),
        ("alias_marker", "crate::Width", "crate"),
        ("gated_marker", "crate::gated #[cfg(windows)]", "crate"),
        ("outer_module_marker", "crate (outside any item)", "crate"),
        (
            "inner_module_marker",
            "crate::inner (outside any item)",
            "crate::inner",
        ),
        ("inner_fn_marker", "crate::inner::inside", "crate::inner"),
        ("macro_doc_marker", "crate (outside any item)", "crate"),
        ("second_line_marker", "crate::two_lines", "crate"),
    ];
    let mut wrong = Vec::new();
    for (marker, owner, module) in cases {
        match keyed(&index, &directory, marker) {
            Ok(row) if row.owner == owner && row.module == module && row.target == marker => {}
            Ok(row) => wrong.push(format!(
                "{marker}: wanted {owner} in {module}, got {} in {} (target {})",
                row.owner, row.module, row.target
            )),
            Err(why) => wrong.push(format!("{marker}: wanted {owner}, refused: {why}")),
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Every unsupported position is refused, each for its own reason.
///
/// MUTATION: delete the function-body refusal, and `body_marker` is keyed to `crate::free`.
#[test]
fn each_unsupported_position_is_refused_by_name() {
    let (index, directory) = fixture();
    let cases = [
        ("body_marker", "inside a function's body"),
        (
            "assoc_type_marker",
            "inside a trait's braces but in no member",
        ),
        ("macro_tokens_marker", "inside a macro's token tree"),
        (
            "plain_comment_marker",
            "in no doc comment and no doc attribute",
        ),
    ];
    let mut wrong = Vec::new();
    for (marker, reason) in cases {
        match keyed(&index, &directory, marker) {
            Err(why) if why.contains(reason) => {}
            Err(why) => wrong.push(format!("{marker}: refused for another reason: {why}")),
            Ok(row) => wrong.push(format!(
                "{marker}: wanted refused ({reason}), got {}",
                row.line()
            )),
        }
    }
    let elsewhere = Warning {
        file: "elsewhere/nothing".to_owned(),
        line: 1,
        column: 1,
        message: "unresolved link to `x`".to_owned(),
    };
    match key(&index, &directory, &directory, &elsewhere) {
        Err(why) if why.contains("a file the index did not lower") => {}
        other => wrong.push(format!("a file not lowered: {other:?}")),
    }
    let mut past = warning_at(
        &index,
        &directory,
        "free_fn_marker",
        "unresolved link to `x`",
    );
    past.line = 100_000;
    match key(&index, &directory, &directory, &past) {
        Err(why) if why.contains("past the file's end") => {}
        other => wrong.push(format!("a line past the end: {other:?}")),
    }
    let mut outside = warning_at(
        &index,
        &directory,
        "free_fn_marker",
        "unresolved link to `x`",
    );
    outside.file = format!("../doc_link_debt/{}", outside.file);
    match key(&index, &directory, &directory.join("nested"), &outside) {
        Err(why) if why.contains("outside the crate's sources") => {}
        other => wrong.push(format!("a file outside the sources: {other:?}")),
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The three message forms are read with their targets, and any other is refused.
#[test]
fn the_three_message_forms_are_read_and_any_other_is_refused() {
    assert_eq!(
        shape_of("unresolved link to `Self::redraw`"),
        Ok(("unresolved", "Self::redraw".to_owned()))
    );
    assert_eq!(
        shape_of("`unit` is both a function and a primitive type"),
        Ok(("ambiguous", "unit".to_owned()))
    );
    assert_eq!(
        shape_of("incompatible link kind for `Seat`"),
        Ok(("incompatible", "Seat".to_owned()))
    );
    for other in [
        "this URL is not a hyperlink",
        "unresolved link to ``",
        "`unit` names nothing",
    ] {
        assert!(shape_of(other).is_err(), "{other} was read as a form");
    }
}

/// A link the list lacks and a row no link is are both red, with multiplicity; the stamp line
/// is not compared, and the same rows in another order are.
///
/// MUTATION: compare membership instead of counts, and the doubled row passes.
#[test]
fn a_new_link_and_a_fixed_row_are_both_red() {
    let row = |owner: &str, target: &str| Row {
        owner: owner.to_owned(),
        target: target.to_owned(),
        shape: "unresolved",
        module: "crate".to_owned(),
        file: "main".to_owned(),
    };
    let two = render(&[row("crate::a", "x"), row("crate::b", "y")]);
    let stamped = two.replacen('\n', "\n# measured at 0123abcd\n", 1);
    assert!(
        differences(&stamped, &two).is_empty(),
        "the stamp line is compared"
    );

    let three = render(&[
        row("crate::a", "x"),
        row("crate::b", "y"),
        row("crate::c", "planted"),
    ]);
    let found = differences(&two, &three);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("a broken link the list does not hold: crate::c\tplanted"),
        "{found:?}"
    );

    let one = render(&[row("crate::a", "x")]);
    let found = differences(&two, &one);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("a row no broken link is"), "{found:?}");

    let doubled = render(&[
        row("crate::a", "x"),
        row("crate::a", "x"),
        row("crate::b", "y"),
    ]);
    assert_eq!(
        differences(&two, &doubled).len(),
        1,
        "a second warning is a second row"
    );

    let swapped = two.replace("crate::a\tx", "crate::z\tx");
    let reordered = format!(
        "{HEADER}{}\n{}\n",
        row("crate::b", "y").line(),
        row("crate::z", "x").line()
    );
    assert_eq!(
        differences(&swapped, &reordered),
        ["the same lines in another order"]
    );
}

/// The measuring script's file is read by its header, and anything else is refused.
#[test]
fn the_measured_file_is_read_by_its_header() {
    let text =
        "file\tline\tcolumn\tmessage\ncrates\\bt-app\\src\\x\t3\t9\tunresolved link to `a`\tb`\n";
    let read = read_measured(text).expect("a measured file");
    assert_eq!(
        read,
        [Warning {
            file: "crates\\bt-app\\src\\x".to_owned(),
            line: 3,
            column: 9,
            message: "unresolved link to `a`\tb`".to_owned(),
        }]
    );
    assert!(read_measured("file\tline\n").is_err());
    assert!(read_measured("file\tline\tcolumn\tmessage\nx\ty\t1\tm\n").is_err());
    assert!(read_measured("").is_err());
}
