//! **The ownership census is a query with a narrow judgement gate** — ticket census-1,
//! the census note's revision (b)2 "Ticket A, re-specified"
//! (`docs/plans/design/ownership-census-2026-09-25.md`).
//!
//! Two kinds of test live here. The fixtures under `tests/fixtures/census/`
//! prove the rules one at a time: a writer moved, a writer added, a method
//! name shared between a writing and a reading type, `get_mut` alone, each
//! receiver shape, `Runtime`'s own fields before its `Deref`, and the gate's
//! two refusals. The last two tests run the query over `bt-app` itself and hold
//! it over `bt-app` itself:
//!
//! * inventory and sites are query output under `target/ownership-census/`;
//! * `ownership-census-unknowns` is the committed shrink-only unresolved set;
//! * `ownership-census-annotations` is the hand-edited class and proposed
//!   owner of every proven multi-writer fact.
//!
//! Every run leaves all renderings in `target/ownership-census/`. Only the
//! unknown list is copied, and only when it has not grown.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use bt_source::{
    Column, Committed, CommittedFile, Difference, DiskScope, FieldCensus, Index, ItemKind, SiteRow,
    TargetId, TargetKind, TargetRoot, Universe, Vendor, Workspace, is_vendored, report, universes,
};

/// The census's four structs, as the note's §1 names them. `Runtime` is not
/// one of them: its two fields are the way to `App` and `WindowRuntime`, and
/// its `Deref` target is `TabState`.
const STRUCTS: [&str; 4] = ["App", "WindowRuntime", "TabState", "LeafSession"];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

// ── the fixtures ──────────────────────────────────────────────────────────

/// The fixture program `name`, from its crate root, with the shared files
/// beside it in scope.
fn fixture(name: &str) -> Arc<Index> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("census");
    let universe = Universe::declare(
        format!("the census {name} fixture"),
        vec![TargetRoot {
            id: TargetId {
                package: format!("census-{name}"),
                kind: TargetKind::Library,
                name: format!("census_{name}"),
            },
            file: root.join(name).join("lib.rs"),
        }],
        vec![DiskScope::under(&root)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::shared(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

fn census_of(name: &str) -> FieldCensus {
    let index = fixture(name);
    let census = FieldCensus::take(&index, &STRUCTS, &[&index]).expect("the fixture's four");
    assert!(
        census.unparsed().is_empty(),
        "every declaration of the fixture parses: {:?}",
        census.unparsed()
    );
    census
}

fn rows<'c>(census: &'c FieldCensus, fact: &str, column: Column) -> Vec<&'c SiteRow> {
    census
        .rows()
        .iter()
        .filter(|row| row.fact == fact && row.column == column)
        .collect()
}

fn functions(rows: &[&SiteRow]) -> Vec<String> {
    rows.iter().map(|row| row.function.clone()).collect()
}

/// The committed files as the fixture's own renders say them.
fn committed_as_rendered(census: &FieldCensus) -> Committed {
    Committed {
        inventory: census.render_inventory(),
        sites: census.render_sites(),
        unknowns: census.render_unknowns(),
        annotations: annotations_for(census),
    }
}

/// An annotation file covering exactly the proven multi-writer facts.
fn annotations_for(census: &FieldCensus) -> String {
    let mut text = String::from("fact\tpart\tclass\tproposed_owner\tnote\n");
    for fact in census.multi_writer_facts() {
        text.push_str(&format!("{fact}\t-\tVIEW\tfixture\t-\n"));
    }
    text
}

/// RED (census-1) — **a writer moved into a newly declared module changes the
/// module column of its rows and nothing else.**
///
/// The census note's §1 split `main.rs` by manifest topic, which is a file's
/// identity: a move that left the manifest behind moved the writer without the
/// census noticing, and a move that did not leave it made two rows look like
/// different facts. The module column is now the declaring item's module path,
/// so the gate's diff after a move is exactly the moved rows, and each of them
/// differs in that one column.
///
/// MUTATION: key a row's module on the file its item is written in
/// (`FileRecord::path`) instead of the identity's module path, and the moved
/// rows differ in the module column by a file name instead — or, for two moves
/// into one file, not at all.
#[test]
fn a_writer_moved_into_a_new_module_changes_only_the_module_column() {
    let before = census_of("rules");
    let after = census_of("moved");
    let without_module = |census: &FieldCensus| -> Vec<String> {
        census
            .rows()
            .iter()
            .map(|row| {
                format!(
                    "{}\t{}\t{}\t{}\t{}",
                    row.fact,
                    row.column.name(),
                    row.function,
                    row.kinds.join(","),
                    row.sites
                )
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    assert_eq!(
        without_module(&before),
        without_module(&after),
        "the same rows, whatever module"
    );
    let moved = |census: &FieldCensus| -> Vec<(String, String)> {
        census
            .rows()
            .iter()
            .filter(|row| row.function == "Runtime::raise")
            .map(|row| (row.fact.clone(), row.module.clone()))
            .collect()
    };
    assert_eq!(
        moved(&before),
        [(
            "WindowRuntime.dirty_gate".to_owned(),
            "crate::writers".to_owned()
        )]
    );
    assert_eq!(
        moved(&after),
        [(
            "WindowRuntime.dirty_gate".to_owned(),
            "crate::doors".to_owned()
        )]
    );
    assert_eq!(
        before.render_inventory(),
        after.render_inventory(),
        "the inventory does not move"
    );
}

/// RED (census-1) — **a new writer adds one site row, and the gate names it.**
///
/// The gate is the committed file against the code. A function added in a new
/// module that writes `WindowRuntime.title` is one row the committed file does
/// not carry, and the message says which, in the words of the row.
///
/// MUTATION: have `FieldCensus::judge` compare row counts instead of rows, and
/// the added row is not named (and a moved one would pass).
#[test]
fn a_new_writer_adds_a_row_and_the_gate_names_it() {
    let before = census_of("rules");
    let after = census_of("grown");
    let differences = after.judge(&committed_as_rendered(&before));
    let added: Vec<&Difference> = differences
        .iter()
        .filter(|difference| {
            matches!(
                difference,
                Difference::NotCommitted {
                    file: CommittedFile::Sites,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(added.len(), 1, "one new site row: {differences:#?}");
    let message = added[0].to_string();
    assert!(
        message
            .contains("WindowRuntime.title\twrite\tcrate::titles\tname_the_window\tcall:push_str"),
        "the message names the row: {message}"
    );
    // The title now has writers in two modules, so the gate also asks for its
    // annotation — and nothing else differs.
    assert!(
        differences.iter().all(|difference| match difference {
            Difference::NotCommitted { row, .. } | Difference::NoLongerTrue { row, .. } =>
                row.starts_with("WindowRuntime.title\t"),
            Difference::MissingAnnotation { fact } => fact == "WindowRuntime.title",
            _ => false,
        }),
        "nothing but the title's rows differ: {differences:#?}"
    );
    assert!(differences.contains(&Difference::MissingAnnotation {
        fact: "WindowRuntime.title".to_owned()
    }));
}

/// RED (census-1) — **a call is a write when its method resolves, by the
/// field's declared type, to a declaration taking `&mut self` — whatever other
/// type declares a method of that name with `&self`.**
///
/// This is the defect the Codex review found in the note's TSV: `raise_dirty_gate`
/// calls `self.window.dirty_gate.open(request)`, and the census dropped it
/// because `open` is declared with `&self` elsewhere. Here `DirtyGate::open`
/// writes and `Door::open` looks; the first is a proven write, the second is
/// nothing.
///
/// MUTATION: resolve a method by its name across every declaration (require
/// every declaration of `open` to take `&mut self`), and the gate's opener
/// disappears from the write column.
#[test]
fn a_method_shared_by_name_with_a_reading_type_is_proven_by_the_declared_type() {
    let census = census_of("rules");
    let writes = rows(&census, "WindowRuntime.dirty_gate", Column::Write);
    assert_eq!(functions(&writes), ["Runtime::alias", "Runtime::raise"]);
    assert!(
        writes.iter().all(|row| row.kinds == ["call:open"]),
        "{writes:#?}"
    );
    assert!(
        census.rows().iter().all(|row| row.function != "knock"),
        "`Door::open` reads: {:#?}",
        census.rows()
    );
}

/// RED (census-1) — **`get_mut` alone is mutable access, not a write.**
///
/// The note's "148 other than lending" counted `get_mut`, `iter_mut`,
/// `values_mut` and `as_mut` as mutating calls. They grant access and prove
/// nothing; the access column records them and the write column does not.
///
/// MUTATION: class `get_mut` on the fixed list as `Class::Write`, and `peek`
/// becomes a membership change of `WindowRuntime.tabs`.
#[test]
fn get_mut_alone_is_access_and_not_a_write() {
    let census = census_of("rules");
    let peeks: Vec<&SiteRow> = census
        .rows()
        .iter()
        .filter(|row| row.function == "peek")
        .collect();
    assert_eq!(peeks.len(), 1, "{peeks:#?}");
    assert_eq!(peeks[0].fact, "WindowRuntime.tabs");
    assert_eq!(peeks[0].column, Column::Access);
    assert_eq!(peeks[0].kinds, ["call:get_mut"]);
}

/// RED (census-1) — **each receiver shape is resolved or listed unknown, and
/// none is guessed.**
///
/// An explicit receiver (a typed parameter), a local alias, an indexed
/// element, a same-named field on another struct, a `Deref` field — each is
/// resolved by a stated rule. A receiver no rule reaches (a closure parameter
/// behind a `map` the rules cannot type) is an unknown row with its item and
/// reason, and the field it spells is `incomplete`.
///
/// MUTATION: attribute an unresolved `.title` to the one struct that declares
/// a field of that name (the note's §1 fallback), and the unknown row becomes
/// a write of `WindowRuntime.title` by `untyped`.
#[test]
fn each_receiver_shape_is_resolved_or_listed_unknown() {
    let census = census_of("rules");
    // An explicit receiver.
    assert_eq!(
        functions(&rows(&census, "WindowRuntime.title", Column::Write)),
        ["retitle"]
    );
    // A local alias: the lend where it is taken, the write where it is used.
    assert!(
        functions(&rows(&census, "WindowRuntime.dirty_gate", Column::Access))
            .contains(&"Runtime::alias".to_owned())
    );
    // An indexed element: the element's field is written; the list is reached.
    let pinned = functions(&rows(&census, "TabState.pinned", Column::Write));
    assert!(pinned.contains(&"pin_first".to_owned()), "{pinned:?}");
    let reached: Vec<&SiteRow> = rows(&census, "WindowRuntime.tabs", Column::Access)
        .into_iter()
        .filter(|row| row.function == "pin_first")
        .collect();
    assert_eq!(reached.len(), 1);
    assert_eq!(reached[0].kinds, ["index"]);
    // A same-named field on another struct: not a fact, and not unknown.
    assert!(
        census
            .rows()
            .iter()
            .chain(std::iter::empty())
            .all(|row| row.function != "file")
    );
    assert!(census.unknowns().iter().all(|row| row.function != "file"));
    // A `Deref` field.
    assert!(pinned.contains(&"Runtime::pin".to_owned()), "{pinned:?}");
    // A hub's membership and inner mutability, in their own columns.
    assert_eq!(
        functions(&rows(&census, "WindowRuntime.tabs", Column::Membership)),
        ["adopt"]
    );
    assert_eq!(
        functions(&rows(&census, "App.minimized", Column::Inner)),
        ["minimize"]
    );
    // The receiver no rule reaches.
    let unknown: Vec<_> = census
        .unknowns()
        .iter()
        .map(|row| (row.site.as_str(), row.function.as_str()))
        .collect();
    assert_eq!(
        unknown,
        [("?.title", "untyped")],
        "{:#?}",
        census.unknowns()
    );
    assert!(
        census.unknowns()[0].reason.contains("does not resolve"),
        "{:#?}",
        census.unknowns()
    );
    let title = census
        .facts()
        .iter()
        .find(|fact| fact.fact == "WindowRuntime.title")
        .expect("a fact");
    assert!(
        !title.complete(),
        "a field an unknown site spells is incomplete"
    );
}

/// RED (census-1) — **`Runtime.{app, window}` are the runtime's own fields,
/// looked up before its `Deref` to `TabState`.**
///
/// `TabState` in the fixture declares a field called `window`, as a tab might.
/// `self.window.hover` inside `impl Runtime` is the window's hover and nothing
/// of the tab's; `self.pinned` is the tab's, because `Runtime` declares no
/// `pinned`.
///
/// MUTATION: in `Resolver::field`, try the `Deref` target before the type's
/// own fields, and `self.window` becomes the tab's `u32`, so `hover` is an
/// unknown site instead of a write of `WindowRuntime.hover`.
#[test]
fn runtime_app_and_window_are_resolved_before_the_deref_to_tab_state() {
    let census = census_of("rules");
    assert_eq!(
        functions(&rows(&census, "WindowRuntime.hover", Column::Write)),
        ["Runtime::hover"]
    );
    assert!(
        census
            .rows()
            .iter()
            .all(|row| row.fact != "TabState.window"),
        "{:#?}",
        census.rows()
    );
    assert!(
        census
            .unknowns()
            .iter()
            .all(|row| row.function != "Runtime::hover")
    );
}

/// RED (census-1) — **the gate refuses an unknown the committed list does not
/// carry, and lets the list shrink.**
///
/// Unknowns are output, not dropped, and like `docs/plans/MIGRATION-DEBT.tsv`
/// the list moves one way: a committed row the code no longer produces is
/// allowed (it is removed by the copier), and a row the code produces that the
/// committed list does not carry is refused, by name.
///
/// MUTATION: have `FieldCensus::judge` skip the unknown list, and the grown
/// set passes.
#[test]
fn the_gate_refuses_a_grown_unknown_set_and_lets_it_shrink() {
    let census = census_of("rules");
    let mut committed = committed_as_rendered(&census);
    assert!(
        census.judge(&committed).is_empty(),
        "{:#?}",
        census.judge(&committed)
    );

    committed.unknowns = "site\tmodule\tfunction\treason\tsites\n".to_owned();
    let differences = census.judge(&committed);
    assert_eq!(differences.len(), 1, "{differences:#?}");
    let message = differences[0].to_string();
    assert!(
        message.contains("the unknown list grew: ?.title in crate::others::untyped"),
        "{message}"
    );
    assert_eq!(
        census.render_unknowns_if_not_grown(&committed.unknowns),
        None,
        "a grown list is not copied"
    );

    committed.unknowns = format!(
        "{}?.gone\tcrate::others\tretired\ta resolved site\t3\n",
        census.render_unknowns()
    );
    assert!(census.judge(&committed).is_empty(), "a shrunk list passes");
    assert!(
        census
            .render_unknowns_if_not_grown(&committed.unknowns)
            .is_some()
    );
}

/// RED (census-1) — **every proven multi-writer fact has an annotation row,
/// and no annotation row is stale.**
///
/// The class and proposed-owner columns are hand-edited judgements the code
/// cannot regenerate (revision (b)2 §7), so the gate asks only two things of
/// them: that the facts the query proves are written from more than one module
/// are all annotated, and that nothing else is.
///
/// MUTATION: drop the coverage loop from `FieldCensus::judge`, and `App.gpu`
/// — written in `crate::gate` and `crate::others` — passes unannotated.
#[test]
fn the_gate_refuses_a_missing_annotation_and_a_stale_one() {
    let census = census_of("rules");
    assert_eq!(
        census.multi_writer_facts().into_iter().collect::<Vec<_>>(),
        ["App.gpu", "WindowRuntime.dirty_gate"]
    );
    let mut committed = committed_as_rendered(&census);
    committed.annotations = "fact\tpart\tclass\tproposed_owner\tnote\nWindowRuntime.dirty_gate\tthe pending request\tDUR\tgate\t-\nApp.notes\t-\tVIEW\tnotes\t-\n".to_owned();
    let messages: Vec<String> = census
        .judge(&committed)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        messages,
        [
            "the annotation row for App.notes is stale: it is not a proven multi-writer fact",
            "App.gpu has proven writers in more than one module and no annotation row",
        ]
    );
}

// ── bt-app ────────────────────────────────────────────────────────────────

/// The universe the rules stand on: every workspace member but this crate,
/// each package's own sources, vendored ones included as what they are.
fn declarations() -> Vec<Arc<Index>> {
    let workspace = Workspace::read(&workspace_root()).expect("this workspace");
    workspace
        .packages()
        .iter()
        .filter(|package| package.name() != "bt-source")
        .map(|package| {
            let vendor = if is_vendored(package.directory()) {
                Vendor::Included
            } else {
                Vendor::Excluded
            };
            let universe = universes::crate_sources(package, vendor)
                .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
            Index::shared(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
        })
        .collect()
}

fn the_census() -> &'static FieldCensus {
    static CENSUS: OnceLock<FieldCensus> = OnceLock::new();
    CENSUS.get_or_init(|| {
        let held = declarations();
        let indexes: Vec<&Index> = held.iter().map(|index| &**index).collect();
        FieldCensus::take(Index::of_package("bt-app"), &STRUCTS, &indexes)
            .unwrap_or_else(|failure| panic!("{failure}"))
    })
}

/// The two committed judgements, with query-only renderings filled from this
/// run so ordinary writer movement is report data, not a gate.
fn committed_census(census: &FieldCensus) -> Committed {
    let design = workspace_root().join("docs").join("plans").join("design");
    let text = |name: &str| {
        std::fs::read_to_string(design.join(format!("ownership-census-{name}.tsv")))
            .unwrap_or_default()
    };
    Committed {
        inventory: census.render_inventory(),
        sites: census.render_sites(),
        unknowns: text("unknowns"),
        annotations: text("annotations"),
    }
}

/// The annotation file's shape: census-1's header, and five columns a row with
/// something in the owner column. Whether a proposal is confirmed is not this
/// test's to say — the owner rules on it — and a row *added* against the merge
/// base that names no owner or says "proposed" is refused by
/// `scripts/ci/check-census-unknowns.ps1`, which is the half that can see the
/// merge base.
fn annotation_rows_are_well_formed(text: &str) -> Result<(), String> {
    let mut rows = text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let header = rows.next().unwrap_or_default();
    if header != "fact\tpart\tclass\tproposed_owner\tnote" {
        return Err(format!(
            "the annotation header is not census-1's: {header:?}"
        ));
    }
    for row in rows {
        let columns: Vec<&str> = row.split('\t').collect();
        if columns.len() != 5 || columns[3].trim().is_empty() {
            return Err(format!(
                "an annotation row is not five columns with an owner column: {row}"
            ));
        }
    }
    Ok(())
}

/// Leave the renderings where the copier looks for them.
fn leave_renderings(census: &FieldCensus, committed: &Committed) {
    let out = workspace_root().join("target").join("ownership-census");
    std::fs::create_dir_all(&out).expect("target is writable");
    let unknowns = out.join("ownership-census-unknowns.tsv");
    let _ = std::fs::remove_file(&unknowns);
    std::fs::write(
        out.join("ownership-census-inventory.tsv"),
        census.render_inventory(),
    )
    .expect("written");
    std::fs::write(
        out.join("ownership-census-sites.tsv"),
        census.render_sites(),
    )
    .expect("written");
    let grown_or_first = if committed.unknowns.is_empty() {
        Some(census.render_unknowns())
    } else {
        census.render_unknowns_if_not_grown(&committed.unknowns)
    };
    if let Some(text) = grown_or_first {
        std::fs::write(unknowns, text).expect("written");
    }
}

/// RED (T-GATES-047) — **the census gates judgements, not snapshots**: the
/// unknowns may shrink but not grow, every proven multi-writer fact has an
/// annotation row, and no row names a fact that is not one. Inventory and site
/// rows are query output under `target/ownership-census/`.
///
/// A moved or added resolved writer changes the report and needs no committed
/// edit. A new multi-writer fact or a new unknown still needs a judgement.
///
/// MUTATION: delete the annotation row of one proven multi-writer fact
/// (`App.quit`) and this names it as missing.
#[test]
fn the_committed_census_has_annotations_and_no_new_unknowns() {
    let census = the_census();
    let committed = committed_census(census);
    leave_renderings(census, &committed);
    assert!(
        census.unparsed().is_empty(),
        "every declaration parses: {:#?}",
        census.unparsed()
    );
    annotation_rows_are_well_formed(&committed.annotations)
        .unwrap_or_else(|failure| panic!("{failure}"));
    let differences = census.judge(&committed);
    assert!(
        differences.is_empty(),
        "{} ownership judgement difference(s): resolve a new unknown, or add/remove the \
         annotation row of the multi-writer fact it names (a new row names its owner):\n{}",
        differences.len(),
        differences
            .iter()
            .map(|difference| format!("  {difference}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// RED (census-1) — **every field of the four structs has an inventory row**,
/// counted apart from the query by the index's own field identities.
///
/// The census note's figure was 436 at `f7826bd4`. The number is not pinned
/// here, because a field added or removed is a row of the committed inventory
/// already; what is pinned is that the query's facts are exactly the fields
/// the index says the four declare, so no field is left out of the gate.
///
/// MUTATION: skip a struct's `cfg`-gated fields when the query reads the
/// declarations, and the two lists differ by those fields.
#[test]
fn every_field_of_the_four_structs_has_an_inventory_row() {
    let census = the_census();
    let index = Index::of_package("bt-app");
    let mut declared: Vec<String> = index
        .items()
        .iter()
        .filter(|item| item.kind() == ItemKind::Field)
        .filter(|item| {
            item.type_owner()
                .is_some_and(|owner| STRUCTS.contains(&owner))
        })
        .map(|item| format!("{}.{}", item.type_owner().unwrap_or_default(), item.name()))
        .collect();
    declared.sort();
    declared.dedup();
    let mut facts: Vec<String> = census
        .facts()
        .iter()
        .map(|fact| fact.fact.clone())
        .collect();
    facts.sort();
    assert_eq!(facts, declared);
}
