//! **Guard: the chain tables of `docs/ARCHITECTURE.md` §7 name items the code has**
//! (T-CHAINS-72; `docs/ARCHITECTURE-GUARDS.md`, "Cross-crate chains").
//!
//! §7 is hand-written ("adding a hop edits this list"), and a hand-written table of function names
//! goes stale the day a hop moves: §7.2's first pass named functions that had moved files, crates or
//! names within three weeks. This guard reads every table of §7 and resolves every name written
//! between backticks in it to an item identity of a first-party crate, through
//! [`bt_source::Index`] — the identity a move keeps, never a file — and every thread name to the
//! string the product spawns it with. Its subject is how the document is written against the code,
//! which is why it reads source at all.
//!
//! # The grammar, and nothing else
//!
//! Every run of `|` lines in §7 is a table, named in a refusal by the heading above it. Its first
//! line is the header, which has a `crate` column and an `entry` column; its second line is the
//! separator (`|---|`, a colon allowed at either end of a cell); at least one row follows. In a row:
//!
//! * the **`crate` cell** spells exactly one backticked workspace package, `` `bt-term` ``;
//! * the **`entry` cell** holds at least one backticked span, and every other cell any number;
//!   prose around them is free, and every backticked span in a cell other than `crate` is one of:
//!   * `` `item` `` — a free function or a type written at the root of the row's crate;
//!   * `` `module::item` ``, `` `module::inner::item` `` — a free function or a type written in that
//!     module of the row's crate, the module path being the one the item is **declared** in (a
//!     re-export is not followed);
//!   * `` `Type::member` `` — a method, an associated function, a field or a variant whose owning
//!     type is `Type` (an `impl` block's self type by its last segment, as [`bt_source::ItemQuery`]
//!     names it), written anywhere in the row's crate;
//!   * `` `module::Type::member` `` — the same, where `Type` is also declared in that module;
//!   * any of these after a leading package segment, `` `bt_platform::handoff::resolved_for_a_door` ``
//!     or `` `bt-platform::handoff::resolved_for_a_door` ``, which names that package instead of the
//!     row's;
//!   * `` `a-thread-name` `` — lowercase letters and digits joined by hyphens, at least one hyphen,
//!     and no package's name: a thread, which resolves when a string literal of exactly that text
//!     stands in the product code of some first-party package (the name the thread door or `std`
//!     is handed).
//!
//! Each segment of a name is a Rust identifier, and a segment that starts with a capital is a type:
//! it is the last segment or the one before it, never earlier. A name resolves when such an item
//! exists in a product build; one that stands only on `cfg(test)` is its own refusal. **Every other
//! shape is a refusal naming the row** — a call spelled with parentheses, a generic argument, a
//! constant, a static, a trait method named by its trait, a module named alone, a cell that is not
//! a name at all — and so is: a row with the wrong number of cells, a `crate` cell that is not one
//! package, an `entry` cell with no span, an unpaired backtick, a table whose second line is not the
//! separator, a table with no rows, a table without the two columns, and a document with no §7 or a
//! §7 with no table. Nothing is skipped: what the grammar does not read is written out of the
//! backticks, or the grammar is widened by its own change with a planted case.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bt_source::{Index, ItemKind, ItemRecord, LiteralValue, Workspace, is_vendored};

/// The document, relative to the workspace root.
const DOCUMENT: &str = "docs/ARCHITECTURE.md";

/// The heading that opens the section, and the prefix of the one after it.
const SECTION: &str = "## 7. ";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// What one reading found: how many spans resolved, and every refusal.
#[derive(Debug, Default)]
struct Reading {
    resolved: usize,
    refusals: Vec<String>,
}

/// Which way a code name is asked for.
#[derive(Debug, PartialEq, Eq)]
enum Shape<'a> {
    /// A free function or a type, in a module of the package (`crate` for its root).
    Free { module: String, name: &'a str },
    /// A member of a type, the type optionally pinned to the module it is declared in.
    Member {
        module: Option<String>,
        owner: &'a str,
        name: &'a str,
    },
}

/// Whether an item a name asks for is there.
#[derive(Debug, PartialEq, Eq)]
enum Found {
    InTheProduct,
    OnlyInTests,
    Nowhere,
}

/// Where the indices come from, and what the thread names already answered.
struct Reader<'w, 'i> {
    workspace: &'w Workspace,
    index_of: &'i dyn Fn(&str) -> &'static Index,
    threads: BTreeMap<String, bool>,
}

fn is_identifier(segment: &str) -> bool {
    let mut characters = segment.chars();
    characters
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && characters.all(|rest| rest == '_' || rest.is_ascii_alphanumeric())
}

fn is_type_name(segment: &str) -> bool {
    segment.starts_with(|first: char| first.is_ascii_uppercase())
}

fn is_thread_shaped(span: &str) -> bool {
    span.contains('-')
        && span.split('-').all(|piece| {
            !piece.is_empty()
                && piece
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

fn is_separator(line: &str) -> bool {
    cells(line).iter().all(|cell| {
        let inner = cell.strip_prefix(':').unwrap_or(cell);
        let inner = inner.strip_suffix(':').unwrap_or(inner);
        !inner.is_empty() && inner.chars().all(|c| c == '-')
    })
}

/// The package a leading segment spells, in either spelling, when the workspace has it.
fn package_named(workspace: &Workspace, segment: &str) -> Option<String> {
    let name = segment.replace('_', "-");
    workspace.package(&name).ok().map(|_| name)
}

/// The shape of one backticked code name, or why it has none.
fn shape<'a>(workspace: &Workspace, span: &'a str) -> Result<(Option<String>, Shape<'a>), String> {
    let segments: Vec<&str> = span.split("::").collect();
    let (package, rest) = match segments.split_first() {
        Some((first, rest)) if !rest.is_empty() => match package_named(workspace, first) {
            Some(package) => (Some(package), rest),
            None => (None, segments.as_slice()),
        },
        _ => (None, segments.as_slice()),
    };
    if let Some(bad) = rest.iter().find(|segment| !is_identifier(segment)) {
        return Err(format!(
            "`{span}`: `{bad}` is not an identifier; a name is `module::item`, `Type::member`, \
             `item` or a thread name, optionally after a package"
        ));
    }
    let types: Vec<usize> = (0..rest.len())
        .filter(|at| is_type_name(rest[*at]))
        .collect();
    let module_of = |modules: &[&str]| {
        std::iter::once("crate")
            .chain(modules.iter().copied())
            .collect::<Vec<_>>()
            .join("::")
    };
    let found = match types.as_slice() {
        // `module::function` or a bare `function`.
        [] => Shape::Free {
            module: module_of(&rest[..rest.len() - 1]),
            name: rest[rest.len() - 1],
        },
        // `module::Type`.
        [at] if *at == rest.len() - 1 => Shape::Free {
            module: module_of(&rest[..*at]),
            name: rest[*at],
        },
        // `module::Type::member`.
        [at] if *at == rest.len() - 2 => Shape::Member {
            module: (*at > 0).then(|| module_of(&rest[..*at])),
            owner: rest[*at],
            name: rest[*at + 1],
        },
        // `Type::Variant`.
        [at, last] if *at == rest.len() - 2 && *last == rest.len() - 1 => Shape::Member {
            module: (*at > 0).then(|| module_of(&rest[..*at])),
            owner: rest[*at],
            name: rest[*last],
        },
        _ => {
            return Err(format!(
                "`{span}` is not a shape the guard reads: a type segment is the last one, or the \
                 one before the last"
            ));
        }
    };
    Ok((package, found))
}

fn in_the_product(record: &ItemRecord) -> bool {
    record
        .identities()
        .any(|identity| identity.variant.permits_product())
}

/// Whether `index` holds the item `found` names, and whether a product build has it.
fn resolve(index: &Index, found: &Shape<'_>) -> Found {
    let matching: Vec<&ItemRecord> = match found {
        Shape::Free { module, name } => index
            .items()
            .iter()
            .filter(|record| {
                record.name() == *name
                    && record.type_owner().is_none()
                    && matches!(
                        record.kind(),
                        ItemKind::Function | ItemKind::Struct | ItemKind::Enum | ItemKind::Union
                    )
                    && record.module_paths().contains(&module.as_str())
            })
            .collect(),
        Shape::Member {
            module,
            owner,
            name,
        } => {
            let type_is_there = module.as_ref().is_none_or(|module| {
                index.items().iter().any(|record| {
                    record.name() == *owner
                        && record.kind().is_type()
                        && record.module_paths().contains(&module.as_str())
                })
            });
            if !type_is_there {
                return Found::Nowhere;
            }
            index
                .items()
                .iter()
                .filter(|record| {
                    record.name() == *name
                        && record.type_owner() == Some(*owner)
                        && matches!(
                            record.kind(),
                            ItemKind::AssociatedFunction | ItemKind::Field | ItemKind::Variant
                        )
                })
                .collect()
        }
    };
    if matching.iter().any(|record| in_the_product(record)) {
        Found::InTheProduct
    } else if matching.is_empty() {
        Found::Nowhere
    } else {
        Found::OnlyInTests
    }
}

/// Whether a string literal of exactly `name` stands in the product code of `index`.
fn spawns(index: &Index, name: &str) -> bool {
    index.literals().iter().any(|literal| {
        matches!(literal.value(), LiteralValue::Str(text) if text == name)
            && index
                .file_at(literal.span().start())
                .is_some_and(bt_source::FileRecord::permits_product)
            && index
                .items()
                .iter()
                .filter(|record| literal.span().within(record.whole()))
                .all(in_the_product)
    })
}

impl Reader<'_, '_> {
    /// Judge one backticked span of a row whose crate is `package`.
    fn judge(&mut self, span: &str, package: &str) -> Result<(), String> {
        if is_thread_shaped(span) {
            if package_named(self.workspace, span).is_some() {
                return Err(format!(
                    "`{span}` is a package; a package is named in the crate cell or as the first \
                     segment of a path"
                ));
            }
            return if self.thread_is_spawned(span) {
                Ok(())
            } else {
                Err(format!(
                    "`{span}` is thread-shaped and no first-party product code spells it"
                ))
            };
        }
        let (named, found) = shape(self.workspace, span)?;
        let package = named.as_deref().unwrap_or(package);
        match resolve((self.index_of)(package), &found) {
            Found::InTheProduct => Ok(()),
            Found::OnlyInTests => Err(format!(
                "`{span}` names an item of `{package}` that stands only in a test build"
            )),
            Found::Nowhere => Err(format!(
                "`{span}` names no product item of `{package}` ({found:?})"
            )),
        }
    }

    fn thread_is_spawned(&mut self, name: &str) -> bool {
        if let Some(known) = self.threads.get(name) {
            return *known;
        }
        let index_of = self.index_of;
        let found = self
            .workspace
            .packages()
            .iter()
            .filter(|package| !is_vendored(package.directory()))
            .any(|package| spawns(index_of(package.name()), name));
        self.threads.insert(name.to_owned(), found);
        found
    }
}

/// The backticked spans of a cell, or why the cell's backticks do not pair.
fn spans(cell: &str) -> Result<Vec<&str>, String> {
    let pieces: Vec<&str> = cell.split('`').collect();
    if pieces.len().is_multiple_of(2) {
        return Err("an unpaired backtick".to_owned());
    }
    Ok(pieces.iter().skip(1).step_by(2).copied().collect())
}

fn cells(line: &str) -> Vec<&str> {
    let inner = line.trim();
    let inner = inner.strip_prefix('|').unwrap_or(inner);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    inner.split('|').map(str::trim).collect()
}

/// Read `document`'s §7 against the packages of `workspace`, each package's index from
/// `index_of` (`Index::of_package`, which lowers a package once per process).
fn read(
    document: &str,
    workspace: &Workspace,
    index_of: &dyn Fn(&str) -> &'static Index,
) -> Reading {
    let mut reading = Reading::default();
    let mut reader = Reader {
        workspace,
        index_of,
        threads: BTreeMap::new(),
    };
    let lines: Vec<&str> = document.lines().collect();
    let Some(start) = lines.iter().position(|line| line.starts_with(SECTION)) else {
        reading
            .refusals
            .push(format!("{DOCUMENT} has no `{SECTION}` heading"));
        return reading;
    };
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.starts_with("## "))
        .map_or(lines.len(), |at| start + 1 + at);
    let mut heading = lines[start].trim_start_matches('#').trim().to_owned();
    let mut tables = 0;
    let mut at = start + 1;
    while at < end {
        let line = lines[at];
        if line.starts_with('#') {
            heading = line.trim_start_matches('#').trim().to_owned();
            at += 1;
            continue;
        }
        if !line.trim_start().starts_with('|') {
            at += 1;
            continue;
        }
        let first = at;
        while at < end && lines[at].trim_start().starts_with('|') {
            at += 1;
        }
        tables += 1;
        read_table(&heading, &lines[first..at], &mut reader, &mut reading);
    }
    if tables == 0 {
        reading
            .refusals
            .push(format!("{DOCUMENT} §7 holds no table"));
    }
    reading
}

fn read_table(heading: &str, table: &[&str], reader: &mut Reader<'_, '_>, reading: &mut Reading) {
    if table.len() < 2 || !is_separator(table[1]) {
        reading.refusals.push(format!(
            "{heading}: a table whose second line is not the separator ({})",
            table.get(1).copied().unwrap_or("nothing")
        ));
        return;
    }
    let header = cells(table[0]);
    let column = |name: &str| header.iter().position(|cell| *cell == name);
    let (Some(crate_at), Some(entry_at)) = (column("crate"), column("entry")) else {
        reading.refusals.push(format!(
            "{heading}: a table whose header has no `crate` and `entry` columns ({})",
            table[0]
        ));
        return;
    };
    if table.len() == 2 {
        reading
            .refusals
            .push(format!("{heading}: a table with no rows"));
        return;
    }
    for line in table.iter().skip(2) {
        let row = cells(line);
        let label = format!("{heading}, row `{}`", row.first().copied().unwrap_or(""));
        if row.len() != header.len() {
            reading.refusals.push(format!(
                "{label}: {} cells under a header of {}",
                row.len(),
                header.len()
            ));
            continue;
        }
        let package = match spans(row[crate_at]).as_deref() {
            Ok([one]) => match package_named(reader.workspace, one) {
                Some(package) => package,
                None => {
                    reading.refusals.push(format!(
                        "{label}: the crate cell `{one}` is no package of the workspace"
                    ));
                    continue;
                }
            },
            _ => {
                reading.refusals.push(format!(
                    "{label}: the crate cell spells one backticked package, not `{}`",
                    row[crate_at]
                ));
                continue;
            }
        };
        for (at, cell) in row.iter().enumerate() {
            if at == crate_at {
                continue;
            }
            let names = match spans(cell) {
                Ok(names) => names,
                Err(why) => {
                    reading.refusals.push(format!("{label}: {why} in `{cell}`"));
                    continue;
                }
            };
            if at == entry_at && names.is_empty() {
                reading
                    .refusals
                    .push(format!("{label}: the entry cell names no item"));
            }
            for name in names {
                match reader.judge(name, &package) {
                    Ok(()) => reading.resolved += 1,
                    Err(why) => reading.refusals.push(format!("{label}: {why}")),
                }
            }
        }
    }
}

fn workspace() -> Workspace {
    Workspace::read(&workspace_root()).expect("this workspace")
}

/// The planted section: one row that resolves, and one row for every refusal a row can earn.
const PLANTED: &str = "\
## 7. Cross-crate chains

### 7.9 A planted chain

| hop | crate | entry | carries | lane |
|---|---|---|---|---|
| found | `bt-source` | `Index::of_package`, `query::ItemQuery::method` | `index::ItemRecord` | `bt-dir-watch` |
| moved | `bt-source` | `Index::no_such_entry` | - | any |
| malformed | `bt-source` | `Index::of_package()` | - | any |
| elsewhere | `no-such-crate` | `Index::of_package` | - | any |
| short | `bt-source` | `Index::of_package` | - |
| nameless | `bt-source` | an entry with no name | - | any |
| unpaired | `bt-source` | `Index::of_package | - | any |
| misplaced | `bt-source` | `Index::Inner::of_package` | - | any |
| tests only | `bt-source` | `universe::tests::a_directory_the_walk_cannot_list_is_a_refusal` | - | any |
| no thread | `bt-source` | `Index::of_package` | - | `no-such-thread` |
| a package | `bt-source` | `Index::of_package` | - | `bt-source` |
| a module | `bt-source` | `bt_source::index` | - | any |

### 7.10 A table without its separator

| hop | crate | entry | carries | lane |
| found | `bt-source` | `Index::of_package` | - | any |

### 7.11 A table without rows

| hop | crate | entry | carries | lane |
|---|---|---|---|---|

## 8. After

| hop | crate | entry | carries | lane |
|---|---|---|---|---|
| after | `bt-source` | `nothing_after_section_seven_is_read` | - | any |
";

/// RED (T-CHAINS-72) — the fixture: the names a crate has and a thread the product spawns
/// resolve; every other row, and both malformed tables, are refused by name, and nothing after §7
/// is read.
#[test]
fn a_chain_row_names_an_item_or_is_refused_by_name() {
    let workspace = workspace();
    let reading = read(PLANTED, &workspace, &Index::of_package);
    let refused = |row: &str, what: &str| {
        reading
            .refusals
            .iter()
            .any(|refusal| refusal.contains(&format!("row `{row}`")) && refusal.contains(what))
    };
    let planted = [
        (
            "moved",
            "`Index::no_such_entry` names no product item of `bt-source`",
        ),
        ("malformed", "`of_package()` is not an identifier"),
        ("elsewhere", "`no-such-crate` is no package"),
        ("short", "4 cells under a header of 5"),
        ("nameless", "the entry cell names no item"),
        ("unpaired", "an unpaired backtick"),
        ("misplaced", "is not a shape the guard reads"),
        ("tests only", "stands only in a test build"),
        ("no thread", "no first-party product code spells it"),
        ("a package", "`bt-source` is a package"),
        ("a module", "`bt_source::index` names no product item"),
    ];
    for (row, what) in planted {
        assert!(refused(row, what), "row `{row}`: {what}\n{reading:#?}");
    }
    let tables = [
        "7.10 A table without its separator: a table whose second line is not the separator",
        "7.11 A table without rows: a table with no rows",
    ];
    for table in tables {
        assert!(
            reading
                .refusals
                .iter()
                .any(|refusal| refusal.starts_with(table)),
            "{table}\n{reading:#?}"
        );
    }
    assert_eq!(
        reading.refusals.len(),
        planted.len() + tables.len(),
        "{reading:#?}"
    );
    // `found`'s three names and its thread, and the unrefused names of the rows refused for
    // something else: `nameless` has none, `short` is refused whole.
    assert_eq!(reading.resolved, 4 + 2, "{reading:#?}");
}

/// RED (T-CHAINS-72) — a document without §7, a §7 without a table, and a table without the two
/// columns, are refusals.
#[test]
fn a_section_the_guard_cannot_read_is_refused() {
    let workspace = workspace();
    let headless = read("## 6. Doors\n\nprose\n", &workspace, &Index::of_package);
    assert_eq!(
        headless.refusals,
        [format!("{DOCUMENT} has no `{SECTION}` heading")]
    );
    let empty = read(
        "## 7. Cross-crate chains\n\nprose\n",
        &workspace,
        &Index::of_package,
    );
    assert_eq!(empty.refusals, [format!("{DOCUMENT} §7 holds no table")]);
    let columnless = read(
        "## 7. Chains\n### 7.1 One\n| hop | where |\n|---|---|\n| a | `b` |\n",
        &workspace,
        &Index::of_package,
    );
    assert_eq!(columnless.refusals.len(), 1, "{columnless:#?}");
    assert!(
        columnless.refusals[0].contains("no `crate` and `entry` columns"),
        "{columnless:#?}"
    );
}

/// The guard itself: every name §7's tables spell is an item of its crate, and every thread name a
/// thread the product spawns.
#[test]
fn every_name_in_the_chain_tables_is_an_item_of_its_crate() {
    let workspace = workspace();
    let document = std::fs::read_to_string(workspace_root().join(DOCUMENT))
        .expect("the architecture document is committed");
    let reading = read(&document, &workspace, &Index::of_package);
    assert!(
        reading.refusals.is_empty(),
        "{DOCUMENT} §7: {} names resolved, {} refused:\n{}",
        reading.resolved,
        reading.refusals.len(),
        reading.refusals.join("\n")
    );
    assert!(reading.resolved > 0, "{DOCUMENT} §7 resolved no name");
}
