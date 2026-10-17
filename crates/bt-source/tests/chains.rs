//! **Guard: the chain tables of `docs/ARCHITECTURE.md` §7 name items the code has**
//! (T-CHAINS-72; `docs/ARCHITECTURE-GUARDS.md`, "Cross-crate chains").
//!
//! §7 is hand-written ("adding a hop edits this list"), and a hand-written table of function names
//! goes stale the day a hop moves: §7.2's first pass named functions that had moved files, crates or
//! names within three weeks. This guard reads every table of §7 and resolves the names its `entry`
//! and `carries` cells spell to item identities of the crate the row's `crate` cell names, through
//! [`bt_source::Index`] — the identity a move keeps, never a file. Its subject is how the document is
//! written against the code, which is why it reads source at all.
//!
//! # The grammar, and nothing else
//!
//! Every run of `|` lines in §7 is a table, named in a refusal by the heading above it, and its
//! header has a `crate` column and an `entry` column. In a table:
//!
//! * the **`crate` cell** spells exactly one backticked workspace package, `` `bt-term` ``;
//! * the **`entry` cell** holds at least one backticked name, and the **`carries` cell** any number;
//!   prose around them is free, and every backticked span in either cell is a name in one of these
//!   shapes, each `segment` a Rust identifier:
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
//!     row's.
//!
//! A name resolves when such an item exists in a product build (an item that stands only on
//! `cfg(test)` does not). **Every other shape is a refusal naming the row** — a call spelled with
//! parentheses, a generic argument, a constant, a static, a trait method named by its trait, a
//! module named alone, a cell that is not a name at all — and so is a row with the wrong number of
//! cells, a `crate` cell that is not one package, an `entry` cell with no name, and a §7 with no
//! table. Nothing is skipped: what the grammar does not read is written out of the backticks, or
//! the grammar is widened by its own change.

use std::path::{Path, PathBuf};

use bt_source::{Index, ItemKind, ItemRecord, Workspace};

/// The document, relative to the workspace root.
const DOCUMENT: &str = "docs/ARCHITECTURE.md";

/// The heading that opens the section, and the prefix of the one after it.
const SECTION: &str = "## 7. ";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// What one reading found: how many names resolved, and every refusal.
#[derive(Debug, Default)]
struct Reading {
    resolved: usize,
    refusals: Vec<String>,
}

/// Which way a name is asked for.
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

/// The package a leading segment spells, in either spelling, when the workspace has it.
fn package_named(workspace: &Workspace, segment: &str) -> Option<String> {
    let name = segment.replace('_', "-");
    workspace.package(&name).ok().map(|_| name)
}

/// The shape of one backticked span, or why it has none.
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
            "`{span}`: `{bad}` is not an identifier; a name is `module::item`, `Type::member` or \
             `item`, optionally after a package"
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

/// Whether `index` holds the item `found` names.
fn resolves(index: &Index, found: &Shape<'_>) -> bool {
    match found {
        Shape::Free { module, name, .. } => index.items().iter().any(|record| {
            record.name() == *name
                && record.type_owner().is_none()
                && matches!(
                    record.kind(),
                    ItemKind::Function | ItemKind::Struct | ItemKind::Enum | ItemKind::Union
                )
                && record.module_paths().contains(&module.as_str())
                && in_the_product(record)
        }),
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
                        && in_the_product(record)
                })
            });
            type_is_there
                && index.items().iter().any(|record| {
                    record.name() == *name
                        && record.type_owner() == Some(*owner)
                        && matches!(
                            record.kind(),
                            ItemKind::AssociatedFunction | ItemKind::Field | ItemKind::Variant
                        )
                        && in_the_product(record)
                })
        }
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
fn read<'w>(
    document: &str,
    workspace: &Workspace,
    index_of: &dyn Fn(&str) -> &'w Index,
) -> Reading {
    let mut reading = Reading::default();
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
        read_table(
            &heading,
            &lines[first..at],
            workspace,
            index_of,
            &mut reading,
        );
    }
    if tables == 0 {
        reading
            .refusals
            .push(format!("{DOCUMENT} §7 holds no table"));
    }
    reading
}

fn read_table<'w>(
    heading: &str,
    table: &[&str],
    workspace: &Workspace,
    index_of: &dyn Fn(&str) -> &'w Index,
    reading: &mut Reading,
) {
    let header = cells(table[0]);
    let column = |name: &str| header.iter().position(|cell| *cell == name);
    let (Some(crate_at), Some(entry_at)) = (column("crate"), column("entry")) else {
        reading.refusals.push(format!(
            "{heading}: a table whose header has no `crate` and `entry` columns ({})",
            table[0]
        ));
        return;
    };
    let carries_at = column("carries");
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
            Ok([one]) => match package_named(workspace, one) {
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
        let mut judged = vec![(row[entry_at], true)];
        if let Some(carries_at) = carries_at {
            judged.push((row[carries_at], false));
        }
        for (cell, needs_a_name) in judged {
            let names = match spans(cell) {
                Ok(names) => names,
                Err(why) => {
                    reading.refusals.push(format!("{label}: {why} in `{cell}`"));
                    continue;
                }
            };
            if needs_a_name && names.is_empty() {
                reading
                    .refusals
                    .push(format!("{label}: the entry cell names no item"));
            }
            for name in names {
                match shape(workspace, name) {
                    Err(why) => reading.refusals.push(format!("{label}: {why}")),
                    Ok((named, found)) => {
                        let package = named.as_deref().unwrap_or(&package);
                        if resolves(index_of(package), &found) {
                            reading.resolved += 1;
                        } else {
                            reading.refusals.push(format!(
                                "{label}: `{name}` names no product item of `{package}` ({found:?})"
                            ));
                        }
                    }
                }
            }
        }
    }
}

fn workspace() -> Workspace {
    Workspace::read(&workspace_root()).expect("this workspace")
}

/// The planted section: one row that resolves, one whose name the crate does not have, one whose
/// name is no shape the guard reads, and one whose crate cell is no package.
const PLANTED: &str = "\
## 7. Cross-crate chains

### 7.9 A planted chain

| hop | crate | entry | carries | lane |
|---|---|---|---|---|
| found | `bt-source` | `Index::of_package`, `query::ItemQuery::method` | `index::ItemRecord` | any |
| moved | `bt-source` | `Index::no_such_entry` | - | any |
| malformed | `bt-source` | `Index::of_package()` | - | any |
| elsewhere | `no-such-crate` | `Index::of_package` | - | any |

## 8. After
";

/// RED (T-CHAINS-72) — the fixture: a name the crate has resolves; a moved name, a malformed name
/// and an unknown crate are each refused by their row, and nothing after §7 is read.
#[test]
fn a_chain_row_names_an_item_or_is_refused_by_name() {
    let workspace = workspace();
    let reading = read(PLANTED, &workspace, &Index::of_package);
    assert_eq!(reading.resolved, 3, "{reading:#?}");
    let refused = |row: &str, what: &str| {
        reading
            .refusals
            .iter()
            .any(|refusal| refusal.contains(&format!("row `{row}`")) && refusal.contains(what))
    };
    assert!(
        refused(
            "moved",
            "`Index::no_such_entry` names no product item of `bt-source`"
        ),
        "{reading:#?}"
    );
    assert!(
        refused("malformed", "`of_package()` is not an identifier"),
        "{reading:#?}"
    );
    assert!(
        refused("elsewhere", "`no-such-crate` is no package"),
        "{reading:#?}"
    );
    assert_eq!(reading.refusals.len(), 3, "{reading:#?}");
}

/// RED (T-CHAINS-72) — a §7 without a table, and a table without the two columns, are refusals.
#[test]
fn a_section_the_guard_cannot_read_is_refused() {
    let workspace = workspace();
    let empty = read(
        "## 7. Cross-crate chains\n\nprose\n",
        &workspace,
        &Index::of_package,
    );
    assert_eq!(empty.refusals, [format!("{DOCUMENT} §7 holds no table")]);
    let headless = read(
        "## 7. Chains\n### 7.1 One\n| hop | where |\n|---|---|\n| a | `b` |\n",
        &workspace,
        &Index::of_package,
    );
    assert_eq!(headless.refusals.len(), 1, "{headless:#?}");
    assert!(
        headless.refusals[0].contains("no `crate` and `entry` columns"),
        "{headless:#?}"
    );
}

/// The guard itself: every name §7's tables spell is an item of the crate the row names.
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
