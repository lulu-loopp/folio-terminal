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
//!
//! The runtime dependency map (J3, the ownership contract's §3.1) is taken
//! from the same census of the same structs, so its guard is here too: the
//! committed files under `docs/plans/design/runtime-dep-map/` are the map of
//! this tree, and `scripts/generate-runtime-dep-map.ps1` copies its renderings
//! from `target/depmap/`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use bt_source::call_graph::{Arms, Resolver, Target, Unresolved};

use bt_source::{
    Column, Committed, CommittedFile, DependencyMap, Difference, DiskScope, FieldCensus, Index,
    ItemKind, MEASURED_AT, SiteRow, TargetId, TargetKind, TargetRoot, Universe, Vendor, Workspace,
    is_vendored, report, universes,
};

/// The census note's four structs, as its §1 names them — the fixtures declare
/// these. `Runtime` is not one of them: its two fields are the way to `App`
/// and `WindowRuntime`, and its `Deref` target is `TabState`.
const FIXTURE_STRUCTS: [&str; 4] = ["App", "WindowRuntime", "TabState", "LeafSession"];

/// The structs whose fields the census holds over `bt-app`: the note's four,
/// and the document and its view (ticket O-7; the ownership contract's §2.2
/// invariant 1), so a write into a document is a row of its own and not only
/// a lend of `TabState.preview_pool` — the view being `PreviewPane` and the two
/// contents that hold its state, a view of a pool buffer and a picture (ticket
/// O-8, invariant 2) — and a terminal pane's view, `TermView`, nested in
/// the session record (ticket O-2, the contract's M0 and M1), and the facts of a
/// pane's launch, `LaunchFacts`, nested beside it (ticket O-4a, M5). The session
/// record is `ShellSession`, the contract's M0 name, since M8's rename (ticket
/// O-5a, 2026-10-27; `LeafSession` before it): the census refuses a struct the
/// package does not declare (`CensusFailure::NoSuchStruct`), so the name came in
/// with the rename and replaced the old one, and the count stays ten. M7's
/// `AttentionAccount` is not declared: its nest, `ShellSession.attention`, is the
/// fact, as the contract's §3.3 counts it; M6's `ShellLife` (ticket O-4b) is not
/// declared either, for the same reason. M8's second half (ticket O-5b,
/// 2026-10-28, ruling E2) adds the pane, `TerminalPane`: `TabState.sessions`
/// holds panes, and the session and the view are two fields of each, so the
/// pane is declared for the map to stay a hub and the two halves to be facts —
/// eleven.
const STRUCTS: [&str; 11] = [
    "App",
    "WindowRuntime",
    "TabState",
    "TerminalPane",
    "ShellSession",
    "TermView",
    "LaunchFacts",
    "PreviewBuffer",
    "PreviewPane",
    "DocumentView",
    "PictureView",
];

/// The fields M1 moved from `ShellSession` into `TermView` (the ownership
/// contract's §3.3).
const TERM_VIEW_M1: [&str; 5] = [
    "thumb_awake",
    "column_awake",
    "card_skip",
    "presented_metrics",
    "frame_image_references",
];

/// The fields M2–M4 moved from `ShellSession` into `TermView` (the ownership
/// contract's §3.3; ticket O-3).
const TERM_VIEW_M2_M4: [&str; 4] = [
    "text_scale",
    "metrics",
    "last_presented_frame",
    "projection",
];

/// The fields M5 moved from `ShellSession` into `LaunchFacts` (the ownership
/// contract's §3.3; ticket O-4a), with their one writer each.
const LAUNCH_FACTS: [(&str, &str); 7] = [
    ("profile", "finish_leaf_birth"),
    ("paste_recipient", "finish_leaf_birth"),
    ("integration", "finish_leaf_birth"),
    ("program", "finish_leaf_birth"),
    ("program_kind", "finish_leaf_birth"),
    ("spawn_place", "place_leaf"),
    ("born_named", "place_leaf"),
];

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
    let census =
        FieldCensus::take(&index, &FIXTURE_STRUCTS, &[&index]).expect("the fixture's four");
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

/// RED (O-2) — **a field moved into a nested struct changes only its fact key.**
///
/// The ownership contract's M1 moves five fields of `ShellSession` under
/// `ShellSession.view: TermView`, and its proof is the census: the moved
/// facts' rows, writes and reads, are the rows they had, with `Leaf.x`
/// spelled `View.x`. The field the nest adds is reached only on the way to
/// the fields under it, so it is read wherever one of them is read and is
/// written nowhere — a write or a lend under it is the inner field's, not
/// the nest's.
///
/// MUTATION: in the walk, visit the base of an attributed field with the
/// pending use instead of `Pending::Attributed`, and `Leaf.view` gains a
/// membership and an access row (it is a hub: its type is one of the facts'
/// structs) — the first assertion names them.
#[test]
fn a_field_moved_into_a_nested_struct_changes_only_its_fact_key() {
    let take = |name: &str, structs: &[&str]| {
        let index = fixture(name);
        let census = FieldCensus::take(&index, structs, &[&index]).expect("the fixture's structs");
        assert!(census.unparsed().is_empty(), "{:?}", census.unparsed());
        census
    };
    let before = take("nested_before", &["Leaf"]);
    let after = take("nested_after", &["Leaf", "View"]);
    let spelled = |rows: &[SiteRow], rename: bool| -> Vec<String> {
        let mut out: Vec<String> = rows
            .iter()
            .filter(|row| row.fact != "Leaf.view")
            .map(|row| {
                let fact = if rename {
                    row.fact
                        .replace("Leaf.clock", "View.clock")
                        .replace("Leaf.skip", "View.skip")
                } else {
                    row.fact.clone()
                };
                format!(
                    "{fact}\t{}\t{}\t{}\t{}\t{}",
                    row.column.name(),
                    row.module,
                    row.function,
                    row.kinds.join(","),
                    row.sites
                )
            })
            .collect();
        out.sort();
        out
    };
    assert!(
        after.rows().iter().all(|row| row.fact != "Leaf.view"),
        "the nest is written nowhere: {:?}",
        after
            .rows()
            .iter()
            .filter(|row| row.fact == "Leaf.view")
            .collect::<Vec<_>>()
    );
    assert_eq!(
        spelled(before.rows(), true),
        spelled(after.rows(), false),
        "the same write, access, membership and inner rows, keyed by the view"
    );
    assert_eq!(
        spelled(before.reads(), true),
        spelled(after.reads(), false),
        "the same reads, keyed by the view"
    );
    let mut nest_reads: Vec<&str> = after
        .reads()
        .iter()
        .filter(|row| row.fact == "Leaf.view")
        .map(|row| row.function.as_str())
        .collect();
    nest_reads.sort_unstable();
    assert_eq!(
        nest_reads,
        ["glance", "note"],
        "the nest is read exactly where a view field is read"
    );
    assert!(
        after
            .facts()
            .iter()
            .any(|fact| fact.fact == "Leaf.view" && fact.hub),
        "the nest is a fact of its own, and a hub"
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

/// RED (census-1) — **every field of the census's structs has an inventory row**,
/// counted apart from the query by the index's own field identities.
///
/// The census note's figure was 436 at `f7826bd4`. The number is not pinned
/// here, because a field added or removed is a row of the committed inventory
/// already; what is pinned is that the query's facts are exactly the fields
/// the index says the structs declare, so no field is left out of the gate.
///
/// MUTATION: skip a struct's `cfg`-gated fields when the query reads the
/// declarations, and the two lists differ by those fields.
#[test]
fn every_field_of_the_declared_structs_has_an_inventory_row() {
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

/// RED (O-2, O-3, O-5b) — **the nine view fields M1–M4 moved are `TermView`'s, nested
/// in the pane beside the session record, and their writers are the ones they had.**
///
/// The ownership contract's M1 (§3.3) moves `thumb_awake`, `column_awake`, `card_skip`,
/// `presented_metrics` and `frame_image_references` under `ShellSession.view: TermView`, and M2–M4
/// move `text_scale`, `metrics`, `last_presented_frame` and `projection` after them; no writer
/// changes module (§3.4: the rows' functions stay equal, only the fact column moves). So each is a
/// fact of `TermView` and none of `ShellSession`; the nest (`ShellSession.view` until O-5b,
/// `TerminalPane.view` since) is the one field whose type is `TermView`, and it is written nowhere after construction (two destructures lend
/// it, to reach the projection); and each moved field's proven writes and lends are exactly the
/// items that wrote and lent it on the leaf, by item and whatever module K2 puts the item in. Its
/// reads are not pinned: they move with K2's batches and are held by the runtime dependency map's
/// guard.
///
/// The writer table below is the writers as of M4 (2026-10-27). One row changed
/// function in O-3, as the contract's M3 asks: the presented pair is written by
/// the view's own `TermView::commit_presented` (in `runtime::frame`, the module
/// `Runtime::redraw` wrote it from), which `redraw` calls.
///
/// Since O-5b (M8's second half, 2026-10-28, rulings E2 and E14) the nest is
/// `TerminalPane.view`, the session record's sibling under `TabState.sessions`,
/// and is lent by the same two destructures. `card_skip` has one door, the
/// pane's `TerminalPane::aim_card_skip`, which lends it to
/// `focus_thumb::aim_card_skip`; `aim_card_window` calls the door and lends
/// nothing (E14).
///
/// MUTATION: leave `card_skip` or `projection` on `ShellSession`, and the first
/// assertion names it; drop `wake_terminal_column`'s write, write `leaf.view`
/// whole, write `leaf.view.last_presented_frame` in `redraw` beside the commit,
/// give `runtime::clipboard` a write of `projection` (pass the view to
/// `paste_spelled`), or lend `&mut leaf.view.card_skip` from `aim_card_window`
/// again (observed red at O-5b), and the writer sets differ.
#[test]
fn the_view_fields_are_term_views() {
    let census = the_census();
    let facts: std::collections::BTreeSet<&str> = census
        .facts()
        .iter()
        .map(|fact| fact.fact.as_str())
        .collect();
    for field in TERM_VIEW_M1.iter().chain(TERM_VIEW_M2_M4.iter()) {
        assert!(
            facts.contains(format!("TermView.{field}").as_str())
                && !facts.contains(format!("ShellSession.{field}").as_str()),
            "`{field}` is `TermView`'s and not `ShellSession`'s"
        );
    }
    let nests: Vec<&str> = census
        .facts()
        .iter()
        .filter(|fact| fact.fact.starts_with("TerminalPane.") && fact.declared.contains("TermView"))
        .map(|fact| fact.fact.as_str())
        .collect();
    assert_eq!(nests, ["TerminalPane.view"], "one nest");
    let writers = |fact: &str| -> Vec<(&'static str, String)> {
        let mut out: Vec<(&'static str, String)> = census
            .rows()
            .iter()
            .filter(|row| row.fact == fact)
            .map(|row| (row.column.name(), row.function.clone()))
            .collect();
        out.sort();
        out
    };
    let expected = |pairs: &[(&'static str, &str)]| -> Vec<(&'static str, String)> {
        pairs
            .iter()
            .map(|(column, function)| (*column, (*function).to_owned()))
            .collect()
    };
    // The nest is written nowhere after construction. It is lent only where a
    // destructure of the leaf takes the projection through it
    // (`view: TermView { projection, .. }`): the census counts every step of a
    // struct pattern, so those two lend the nest as well as `TermView.projection`.
    assert_eq!(
        writers("TerminalPane.view"),
        expected(&[
            ("access", "Runtime::insert_path_into_terminal"),
            ("access", "Runtime::send_paste"),
        ]),
        "the nest"
    );
    assert_eq!(
        writers("TermView.thumb_awake"),
        expected(&[("write", "Runtime::wake_terminal_thumb")])
    );
    assert_eq!(
        writers("TermView.column_awake"),
        expected(&[("write", "Runtime::wake_terminal_column")])
    );
    assert_eq!(
        writers("TermView.card_skip"),
        expected(&[("access", "TerminalPane::aim_card_skip")])
    );
    assert_eq!(
        writers("TermView.presented_metrics"),
        expected(&[("write", "TermView::commit_presented")])
    );
    assert_eq!(
        writers("TermView.last_presented_frame"),
        expected(&[("write", "TermView::commit_presented")])
    );
    assert_eq!(
        writers("TermView.text_scale"),
        expected(&[("write", "step_leaf_text_scale")])
    );
    assert_eq!(
        writers("TermView.metrics"),
        expected(&[("write", "apply_leaf_metrics")])
    );
    assert_eq!(
        writers("TermView.projection"),
        expected(&[
            ("access", "Runtime::apply_clipboard_write_effect"),
            ("access", "Runtime::insert_path_into_terminal"),
            ("access", "Runtime::publish_frame_inner"),
            ("access", "Runtime::redraw"),
            ("access", "Runtime::send_paste"),
            ("access", "apply_leaf_metrics"),
            ("write", "Runtime::clear_search_highlights"),
            ("write", "Runtime::install_search_highlights"),
            ("write", "Runtime::jump_to_command_mark"),
            ("write", "Runtime::keyboard_input"),
            ("write", "Runtime::return_to_live_for_input"),
            ("write", "Runtime::reveal_current_search_hit"),
            ("write", "Runtime::scroll_seat_by_columns"),
            ("write", "Runtime::scroll_seat_to_column"),
            ("write", "Runtime::scroll_seat_to_subpixels"),
            ("write", "Runtime::scroll_view"),
            ("write", "Runtime::scroll_view_exact_in"),
            ("write", "TabState::clear_leaf_selection"),
            ("write", "finish_leaf_birth"),
        ])
    );
    assert_eq!(
        writers("TermView.frame_image_references"),
        expected(&[
            ("write", "Runtime::publish_frame_inner"),
            ("write", "Runtime::rescan_pane_references"),
        ])
    );
}

/// RED (O-4a) — **the seven launch facts M5 moved are `LaunchFacts`', nested in
/// `ShellSession` with its lifetime, and each is written by the one item that
/// wrote it on the leaf.**
///
/// The ownership contract's M5 (§3.3) moves `profile`, `paste_recipient`,
/// `integration`, `program`, `program_kind`, `spawn_place` and `born_named`
/// under `ShellSession.launch: LaunchFacts`. Each is a fact the shell's start
/// alone knew: `finish_leaf_birth` writes the first five where the shell lands
/// and `place_leaf` the place and whether it was named, and nothing writes or
/// lends any of them after. So each is a fact of `LaunchFacts` and none of
/// `ShellSession`; `ShellSession.launch` is the one field of the leaf whose type
/// is `LaunchFacts`, and it is written and lent nowhere after construction.
/// Reads are not pinned (they move with K2's batches and are held by the
/// runtime dependency map's guard).
///
/// MUTATION: leave `born_named` on `ShellSession`, and the first assertion names
/// it; drop `place_leaf`'s write of `spawn_place`, write `leaf.launch` whole, or
/// write `leaf.launch.profile` from `restart_shell`, and the writer sets differ.
#[test]
fn the_launch_facts_are_launch_facts() {
    let census = the_census();
    let facts: BTreeSet<&str> = census
        .facts()
        .iter()
        .map(|fact| fact.fact.as_str())
        .collect();
    for (field, _) in LAUNCH_FACTS {
        assert!(
            facts.contains(format!("LaunchFacts.{field}").as_str())
                && !facts.contains(format!("ShellSession.{field}").as_str()),
            "`{field}` is `LaunchFacts`' and not `ShellSession`'s"
        );
    }
    let nests: Vec<&str> = census
        .facts()
        .iter()
        .filter(|fact| {
            fact.fact.starts_with("ShellSession.") && fact.declared.contains("LaunchFacts")
        })
        .map(|fact| fact.fact.as_str())
        .collect();
    assert_eq!(nests, ["ShellSession.launch"], "one nest");
    let writers = |fact: &str| -> Vec<(&'static str, String)> {
        let mut out: Vec<(&'static str, String)> = census
            .rows()
            .iter()
            .filter(|row| row.fact == fact)
            .map(|row| (row.column.name(), row.function.clone()))
            .collect();
        out.sort();
        out
    };
    assert_eq!(writers("ShellSession.launch"), [], "the nest");
    for (field, writer) in LAUNCH_FACTS {
        assert_eq!(
            writers(&format!("LaunchFacts.{field}")),
            [("write", writer.to_owned())],
            "`{field}`'s one writer"
        );
    }
}

/// RED (O-4a) — **a pane's attention account is one fact, `ShellSession.attention`,
/// written by the attention roads that wrote its four parts.**
///
/// The ownership contract's M7 (§3.3) nests the ledger, the bell's edge, the
/// credentials' clock and the capability as `ShellSession.attention:
/// AttentionAccount`. The account is not one of the census's structs: the nest
/// is the fact (the contract counts "4 rows → 1"), so a write of any part is a
/// row of `ShellSession.attention`. The three facts that merged into it had
/// writers that were all already its writers, so its writer set is the one it
/// had: seven items in the root and `runtime::attention`. Whether a road that
/// ends the pane has given its place up, `attention_ended`, stays on the leaf;
/// its writer, `end_leaves_of` in `runtime::panes`, is not one of these.
///
/// MUTATION: leave `bell_reported` on `ShellSession`, and the first assertion
/// names it; reset `leaf.attention.bell_reported` in `end_leaves_of` (a new
/// writer module), or drop `transfer_tab`'s re-point of the ledger, and the
/// writer set differs.
#[test]
fn the_attention_account_is_one_fact() {
    let census = the_census();
    let facts: BTreeSet<&str> = census
        .facts()
        .iter()
        .map(|fact| fact.fact.as_str())
        .collect();
    for gone in ["bell_reported", "attention_clock", "attention_capability"] {
        assert!(
            !facts.contains(format!("ShellSession.{gone}").as_str()),
            "`{gone}` is a part of the account, not a field of the leaf"
        );
    }
    let account: Vec<&str> = census
        .facts()
        .iter()
        .filter(|fact| {
            fact.fact.starts_with("ShellSession.") && fact.declared.contains("AttentionAccount")
        })
        .map(|fact| fact.fact.as_str())
        .collect();
    assert_eq!(account, ["ShellSession.attention"], "one account");
    let writers: BTreeSet<(&'static str, &str)> = census
        .rows()
        .iter()
        .filter(|row| row.fact == "ShellSession.attention")
        .map(|row| (row.column.name(), row.function.as_str()))
        .collect();
    assert_eq!(
        writers,
        BTreeSet::from([
            ("write", "FolioApp::transfer_tab"),
            ("write", "Runtime::mark_attention_seen"),
            ("write", "answer_attention_in"),
            ("write", "deliver_attention"),
            ("write", "deliver_osc_attention"),
            ("write", "expire_leaf_attention"),
            ("write", "settle_attention"),
        ])
    );
}

/// `ShellLife`'s transitions (O-4b): the only calls that may write `ShellSession.life`.
const LIFE_TRANSITIONS: [&str; 13] = [
    "begin_birth",
    "take_birth",
    "record_spawn_fallback",
    "take_spawn_fallback",
    "land",
    "owe_landing_size",
    "exit_draining",
    "exited",
    "release_dead_shell",
    "fall_back",
    "take_fallback",
    "keep_birth_death",
    "retire",
];

/// The methods that move a *Restart shell* successor (O-4b, ruling E5): the session record's
/// until O-5b, the pane's since (`TerminalPane.successor` is a whole pane, E5).
const SUCCESSOR_MOVES: [&str; 3] = ["await_successor", "successor_mut", "take_successor"];

/// RED (O-4b; the ownership contract's M6, rulings D2, D5, D9 and E5) — **the shell's life is
/// one fact, written only by its transitions.**
///
/// The seven fields M6 folded are gone from the census, and `ShellSession.life` is the one fact of
/// type `ShellLife`. Every write row of it calls only a transition — or, in the reaper, a call into
/// the shell through the one lend (`try_wait`, `take_birth_fallback` on what `pty_mut` lent) — and
/// every access row is one of today's lends kept as they stood: `pty_mut` in the reaper and the two
/// resize roads, and the two destructures of the input roads. The functions that touch it other
/// than by reading are these sixteen (§2's 20 → 16: D10 folded the owed-resize roads; E5 kept the
/// successor's three on the record). And `TerminalPane.successor`'s every non-read site is in the
/// pane's three methods that move it (the record's until O-5b moved the successor to the pane).
///
/// MUTATION: `leaf.life.phase = Phase::Kept` written in `keep_birth_death_face` (an assign row);
/// `match &mut leaf.life.phase` in the reaper (an unlisted access row); `leaf.successor =
/// Some(..)` written in `restart_shell` — each is named.
#[test]
fn the_shell_life_is_one_fact_written_only_by_its_transitions() {
    const GONE: [&str; 7] = [
        "pty",
        "shell_exit_age",
        "shell_exit_said",
        "birth_death_fallback",
        "birth_death_kept",
        "spawn_fallback",
        "birth",
    ];
    const THROUGH_THE_LEND: [(&str, &str); 2] = [
        ("Runtime::reap_exited_tabs", "call:take_birth_fallback"),
        ("Runtime::reap_exited_tabs", "call:try_wait"),
    ];
    const LENDS: [(&str, &str); 5] = [
        ("apply_owed_shell_resize", "call:pty_mut"),
        ("release_due_leaf_resize", "call:pty_mut"),
        ("Runtime::reap_exited_tabs", "call:pty_mut"),
        ("Runtime::send_paste", "pattern"),
        ("Runtime::insert_path_into_terminal", "pattern"),
    ];
    let census = the_census();
    let facts: BTreeSet<&str> = census
        .facts()
        .iter()
        .map(|fact| fact.fact.as_str())
        .collect();
    for gone in GONE {
        assert!(
            !facts.contains(format!("ShellSession.{gone}").as_str()),
            "`{gone}` is a part of the shell's life, not a field of the leaf"
        );
    }
    let life: Vec<&str> = census
        .facts()
        .iter()
        .filter(|fact| {
            fact.fact.starts_with("ShellSession.") && fact.declared.contains("ShellLife")
        })
        .map(|fact| fact.fact.as_str())
        .collect();
    assert_eq!(life, ["ShellSession.life"], "one life");
    let rows: Vec<&SiteRow> = census
        .rows()
        .iter()
        .filter(|row| row.fact == "ShellSession.life")
        .collect();
    let mut stray = Vec::new();
    for row in &rows {
        for kind in &row.kinds {
            let admitted = match row.column.name() {
                "write" => {
                    kind.strip_prefix("call:")
                        .is_some_and(|name| LIFE_TRANSITIONS.contains(&name))
                        || THROUGH_THE_LEND.contains(&(row.function.as_str(), kind.as_str()))
                }
                "access" => LENDS.contains(&(row.function.as_str(), kind.as_str())),
                _ => false,
            };
            if !admitted {
                stray.push(format!(
                    "{} {} {} {kind}",
                    row.column.name(),
                    row.module,
                    row.function
                ));
            }
        }
    }
    assert!(
        stray.is_empty(),
        "the shell's life written other than by a transition: {stray:?}"
    );
    let writers: BTreeSet<(&str, &str)> = rows
        .iter()
        .map(|row| (row.module.as_str(), row.function.as_str()))
        .collect();
    assert_eq!(
        writers,
        BTreeSet::from([
            ("crate", "TabState::retire_all_shells"),
            ("crate", "apply_owed_shell_resize"),
            ("crate", "await_shell"),
            ("crate", "create_leaf_session"),
            ("crate", "finish_leaf_birth"),
            ("crate", "land_birth_death_fallback_shell"),
            ("crate", "land_shell_birth"),
            ("crate", "release_due_leaf_resize"),
            ("crate::runtime::clipboard", "Runtime::send_paste"),
            ("crate::runtime::panes", "Runtime::land_own_shell"),
            ("crate::runtime::panes", "Runtime::land_restart"),
            ("crate::runtime::panes", "end_leaves_of"),
            ("crate::runtime::tabs", "Runtime::reap_exited_tabs"),
            ("crate::runtime::tabs", "keep_birth_death_face"),
            ("crate::runtime::tabs", "land_birth_death_fallback"),
            (
                "crate::runtime::terminal",
                "Runtime::insert_path_into_terminal"
            ),
        ]),
        "the life's sixteen writers"
    );
    let moves: BTreeSet<&str> = census
        .rows()
        .iter()
        .filter(|row| row.fact == "TerminalPane.successor")
        .map(|row| row.function.as_str())
        .collect();
    let expected: BTreeSet<String> = SUCCESSOR_MOVES
        .iter()
        .map(|name| format!("TerminalPane::{name}"))
        .collect();
    assert_eq!(
        moves,
        expected.iter().map(String::as_str).collect(),
        "a successor is moved only by the record's own methods"
    );
}

/// RED (O-2; the ownership contract's (b)1 and (c)1) — **the drain takes the
/// keyboard's focus as a report and touches no field of the view.**
///
/// `drain_leaf_pty` is the session's turn: it hears the shell's bytes and
/// answers for them. Its one view-derived input is its `holds_the_keyboard`
/// parameter, computed by the caller (the future `report_keyboard_focus`
/// door), so the census has no row of any column — read, write, access,
/// membership or inner — of any `TermView` fact whose function is the drain,
/// nor of the nest `TerminalPane.view` itself: a drain that handed the whole
/// view to a helper (`&leaf.view`) would show only as a row of the nest.
/// The assertion grows with `TermView`: every field O-3 moves under it is
/// covered without an edit here. Since O-3 the drain reports through
/// `ShellSession::report_keyboard_focus`, whose body is the drain's too: a view
/// read written there is the drain's read, so the door is held to the same
/// rule. Each one's own rows are asserted present, so a renamed drain or door
/// cannot pass by matching nothing.
///
/// MUTATION: have `drain_leaf_pty` read `leaf.view.card_skip`, or take
/// `&leaf.view`, or have `report_keyboard_focus` read `self.view.projection`,
/// and its row is named.
#[test]
fn the_drain_reads_no_view_field() {
    const DRAIN: [&str; 2] = ["drain_leaf_pty", "ShellSession::report_keyboard_focus"];
    let census = the_census();
    let name = |function: &str| function.split(' ').next().unwrap_or_default().to_owned();
    let is_drain = |function: &str| DRAIN.contains(&name(function).as_str());
    let is_view = |fact: &str| fact.starts_with("TermView.") || fact == "TerminalPane.view";
    let all = || census.rows().iter().chain(census.reads());
    for drain in DRAIN {
        assert!(
            all().any(|row| name(&row.function) == drain
                && row.fact.starts_with("ShellSession.")
                && !is_view(&row.fact)),
            "`{drain}` is found by its name"
        );
    }
    let touched: Vec<String> = all()
        .filter(|row| is_drain(&row.function) && is_view(&row.fact))
        .map(|row| format!("{} {}", row.fact, row.column.name()))
        .collect();
    assert!(
        touched.is_empty(),
        "the drain touches the view: {touched:?}"
    );
}

/// RED (O-5b, ruling E10; the census half of the ownership contract's pin 1) — **a session ends
/// without reading its view.**
///
/// The three roads that end a pane's shell — `end_leaves_of` (a close: the attention it held
/// expired, its shell retired), `expire_leaf_attention` (that expiry, per leaf) and
/// `TabState::retire_all_shells` (a window's teardown) — are the session's turn and nobody
/// else's: they walk the record through the session accessors (`leaves_mut`, one leaf lent), so
/// the census has no row of any column of any `TermView` fact, nor of the nest
/// `TerminalPane.view`, whose function is one of them. A road that took the pane to end it
/// (`panes_mut`) and read what the view holds would show as such a row. Each road's own rows are
/// asserted present, so a renamed road cannot pass by matching nothing. The behaviour half — the
/// view's ending observed from a running pane — is O-6's.
///
/// MUTATION (observed red): have `end_leaves_of` walk `tab.panes_mut()` and read
/// `pane.view.card_skip` beside the retire, and its row is named.
#[test]
fn a_session_ends_without_reading_its_view() {
    const ENDINGS: [&str; 3] = [
        "end_leaves_of",
        "expire_leaf_attention",
        "TabState::retire_all_shells",
    ];
    let census = the_census();
    let name = |function: &str| function.split(' ').next().unwrap_or_default().to_owned();
    let is_view = |fact: &str| fact.starts_with("TermView.") || fact == "TerminalPane.view";
    let all = || census.rows().iter().chain(census.reads());
    for ending in ENDINGS {
        assert!(
            all().any(|row| name(&row.function) == ending && row.fact.starts_with("ShellSession.")),
            "`{ending}` is found by its name"
        );
    }
    let touched: Vec<String> = all()
        .filter(|row| ENDINGS.contains(&name(&row.function).as_str()) && is_view(&row.fact))
        .map(|row| format!("{} {} {}", row.function, row.fact, row.column.name()))
        .collect();
    assert!(
        touched.is_empty(),
        "a session's ending touches its view: {touched:?}"
    );
}

// ── the session record's name and its doors (O-5a) ─────────────────────────

/// The fields of the session record: the 22 fields `LeafSession` had at O-4b's head, each
/// `ShellSession`'s by the contract's M8 rename (O-5a, 2026-10-27), less the two O-5b moved to
/// the pane (2026-10-28, rulings E2 and E5): `view` and `successor`. Twenty.
const SHELL_SESSION_FIELDS: [&str; 20] = [
    "attention",
    "attention_ended",
    "conpty_grid",
    "foreground_program_cadence",
    "grid",
    "has_rail",
    "incarnation",
    "last_finished_command",
    "last_seen_revision",
    "launch",
    "life",
    "output_revision",
    "paste_hold",
    "pending_paste",
    "pending_psreadline_resize_reanchor",
    "pending_pty_resize",
    "pending_typing",
    "process_folder",
    "session",
    "wake",
];

/// The pane's fields (O-5b, ruling E2): the session record, its view, and a *Restart shell*
/// successor, which is a whole pane (E5).
const TERMINAL_PANE_FIELDS: [&str; 3] = ["session", "successor", "view"];

/// The proven writes and lends (every row that is not a read) of one census fact, sorted, as
/// `(column, function)`.
fn non_read_rows(census: &FieldCensus, fact: &str) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = census
        .rows()
        .iter()
        .filter(|row| row.fact == fact)
        .map(|row| (row.column.name(), row.function.clone()))
        .collect();
    out.sort();
    out
}

/// RED (O-5a, ruling E1; O-5b, E2) — **the session record is `ShellSession`: the M8 rename moved
/// every fact of `LeafSession` to the same field of `ShellSession` and changed no writer — and
/// the pane, `TerminalPane`, holds the record, its view and its successor.**
///
/// The ownership contract's M8 (§3.3) renames the struct and §3.4 asks that the census differ
/// only in the `fact` column. So no `LeafSession.*` fact is left, the record's facts are exactly
/// the 22 fields it had at O-4b's head, and each fact's writes and lends are the ones it had
/// there, by function (the table below is O-4b's head's `fields.tsv`, re-keyed). The two
/// deliberate changes are O-5a's doors: `last_seen_revision` is written only by
/// `ShellSession::mark_seen` (E8; it was `mark_leaf_painted` and `mark_leaf_seen`, which now
/// call it), and `pending_typing` only by `ShellSession::{queue_typing, take_typing}` (E9; it
/// was `Runtime::queue_typing_in_the_focused_pane` and `drain_leaf_pty`, which now call them).
/// `life`, `attention`, `launch` and `view` are held by their own pins
/// (`the_shell_life_is_one_fact_written_only_by_its_transitions`,
/// `the_attention_account_is_one_fact`, `the_launch_facts_are_launch_facts`,
/// `the_view_fields_are_term_views`), so they are named here but their rows are not repeated;
/// `session`'s 47 writer functions are listed with the rest. The reads are not pinned, as O-2's
/// pin does not pin them: the rename's base-to-head diff of `fields.tsv` shows them equal (the
/// `fact` column only; O-5a's report §2), and the runtime dependency map's guard holds them
/// since.
///
/// O-5b (2026-10-28) took two fields off the record: `view` and `successor` are the pane's
/// (E2, E5), so the record's facts are twenty and the pane's are exactly its three; the
/// successor's three moves are the pane's methods now, and its rows are asserted under
/// `TerminalPane.successor`. `session`'s writer functions are the ones they were but for the
/// re-pathing O-5b did (the list below).
///
/// MUTATION (observed red): keep the old name on the struct, and the first assertion names every
/// field; drop `keep_birth_death_face`'s `leaf.wake = None`, and `wake`'s rows differ; write
/// `leaf.has_rail = true` in `drain_leaf_pty`, and `has_rail`'s rows differ; leave `view` on
/// `ShellSession` beside the pane's (O-5b), and the record's facts differ.
#[test]
fn the_session_record_is_shell_session() {
    let census = the_census();
    let stale: Vec<&str> = census
        .facts()
        .iter()
        .map(|fact| fact.fact.as_str())
        .filter(|fact| fact.starts_with("LeafSession."))
        .collect();
    assert!(
        stale.is_empty(),
        "facts still keyed by the old name: {stale:?}"
    );
    let mut fields: Vec<&str> = census
        .facts()
        .iter()
        .filter_map(|fact| fact.fact.strip_prefix("ShellSession."))
        .collect();
    fields.sort_unstable();
    assert_eq!(fields, SHELL_SESSION_FIELDS, "the record's facts");
    let mut pane_fields: Vec<&str> = census
        .facts()
        .iter()
        .filter_map(|fact| fact.fact.strip_prefix("TerminalPane."))
        .collect();
    pane_fields.sort_unstable();
    assert_eq!(pane_fields, TERMINAL_PANE_FIELDS, "the pane's facts");

    let expected = |pairs: &[(&'static str, &str)]| -> Vec<(&'static str, String)> {
        let mut out: Vec<(&'static str, String)> = pairs
            .iter()
            .map(|(column, function)| (*column, (*function).to_owned()))
            .collect();
        out.sort();
        out
    };
    let table: [(&str, Vec<(&'static str, String)>); 16] = [
        ("attention_ended", expected(&[("write", "end_leaves_of")])),
        (
            "conpty_grid",
            expected(&[("write", "release_due_leaf_resize")]),
        ),
        (
            "foreground_program_cadence",
            expected(&[
                ("write", "apply_answer"),
                ("write", "dispatch"),
                ("write", "request"),
            ]),
        ),
        (
            "grid",
            expected(&[
                ("write", "ShellSession::propose_size"),
                ("write", "release_due_leaf_resize"),
            ]),
        ),
        (
            "has_rail",
            expected(&[("write", "ShellSession::hear_first_mark")]),
        ),
        ("incarnation", expected(&[])),
        (
            "last_finished_command",
            expected(&[("write", "drain_leaf_pty")]),
        ),
        (
            "last_seen_revision",
            expected(&[("write", "ShellSession::mark_seen")]),
        ),
        ("output_revision", expected(&[("write", "drain_leaf_pty")])),
        (
            "paste_hold",
            expected(&[
                ("access", "Runtime::insert_path_into_terminal"),
                ("access", "Runtime::send_paste"),
                ("write", "Runtime::advance_the_hold"),
                ("write", "Runtime::answer_paste_card"),
                ("write", "Runtime::ask_for_a_paste"),
            ]),
        ),
        (
            "pending_paste",
            expected(&[
                ("access", "Runtime::press_paste_card_key"),
                ("write", "stage_paste"),
                ("write", "take_pending_paste"),
            ]),
        ),
        (
            "pending_psreadline_resize_reanchor",
            expected(&[
                ("access", "Runtime::finish_resize_if_quiescent"),
                ("access", "release_due_leaf_resize"),
            ]),
        ),
        (
            "pending_pty_resize",
            expected(&[
                ("access", "ShellSession::propose_size"),
                ("access", "release_due_leaf_resize"),
            ]),
        ),
        (
            "pending_typing",
            expected(&[
                ("write", "ShellSession::queue_typing"),
                ("write", "ShellSession::take_typing"),
            ]),
        ),
        ("process_folder", expected(&[])),
        (
            "wake",
            expected(&[
                ("write", "finish_leaf_birth"),
                ("write", "keep_birth_death_face"),
            ]),
        ),
    ];
    for (field, rows) in table {
        assert_eq!(
            non_read_rows(census, &format!("ShellSession.{field}")),
            rows,
            "`ShellSession.{field}`'s writes and lends"
        );
    }
    // The successor's three moves, the pane's since O-5b (E5).
    assert_eq!(
        non_read_rows(census, "TerminalPane.successor"),
        expected(&[
            ("access", "TerminalPane::successor_mut"),
            ("membership", "TerminalPane::await_successor"),
            ("membership", "TerminalPane::take_successor"),
        ]),
        "`TerminalPane.successor`'s writes and lends"
    );
    // The hub's writes and lends: the 47 functions it had at O-4b's head (49 rows; two write
    // and lend it both).
    assert_eq!(
        non_read_rows(census, "ShellSession.session"),
        expected(&[
            ("access", "Runtime::advance_live_math_if_due"),
            ("access", "Runtime::apply_clipboard_write_effect"),
            ("access", "Runtime::insert_path_into_terminal"),
            ("access", "Runtime::publish_frame_inner"),
            ("access", "Runtime::redraw"),
            ("access", "Runtime::send_paste"),
            ("access", "dispatch_tab_decoration_tasks"),
            ("access", "finish_leaf_birth"),
            ("access", "leaf_session_mut"),
            ("access", "release_due_leaf_resize"),
            ("write", "Runtime::apply_block_max_height"),
            ("write", "Runtime::apply_display_formulas"),
            ("write", "Runtime::apply_inline_formulas"),
            ("write", "Runtime::apply_repair_row_breaks"),
            ("write", "Runtime::apply_scrollback_lines"),
            ("write", "Runtime::apply_tables"),
            ("write", "Runtime::ask_the_worker_about_a_link_target"),
            ("write", "Runtime::clear_pane_screen"),
            ("write", "Runtime::clear_pane_scrollback"),
            ("write", "Runtime::drain_pty"),
            ("write", "Runtime::finish_resize_if_quiescent"),
            ("write", "Runtime::finish_synchronized_update_if_due"),
            ("write", "Runtime::mouse_wheel"),
            ("write", "Runtime::note_user_typing"),
            ("write", "Runtime::present_math_toggle"),
            ("write", "Runtime::press_math_toggle"),
            ("write", "Runtime::publish_frame_inner"),
            ("write", "Runtime::re_ask_the_worker_about_a_link_target"),
            ("write", "Runtime::reset_terminal_modes"),
            ("write", "Runtime::return_to_live_for_input"),
            ("write", "Runtime::sample_math_toggle"),
            ("write", "Runtime::set_hovered_math"),
            ("write", "Runtime::settle_math_toggle"),
            ("write", "Runtime::switch_math_source_now"),
            ("write", "ShellSession::propose_cell"),
            ("write", "ShellSession::propose_layout"),
            ("write", "ShellSession::propose_size"),
            ("write", "ShellSession::report_keyboard_focus"),
            ("write", "TabState::clear_leaf_selection"),
            ("write", "TabState::set_leaf_selection"),
            ("write", "apply_answer"),
            ("write", "drain_leaf_pty"),
            ("write", "finish_leaf_birth"),
            ("write", "forget_work_in_flight_for_seat"),
            ("write", "keep_birth_death_face"),
            ("write", "mark_leaf_seen"),
            ("write", "place_leaf"),
            ("write", "request"),
            ("write", "settle_attention"),
        ]),
        "`ShellSession.session`'s writes and lends"
    );
}

/// RED (O-5a, ruling E8; the ownership contract's §3.4 M8 pin, its census half) — **what a view
/// has seen reaches the session through one door.** `ShellSession.last_seen_revision` is written
/// by `ShellSession::mark_seen` and by nothing else; the two reporters, `mark_leaf_seen` and
/// `mark_leaf_painted`, call it. Its behaviour half, that the door keeps the larger, is bt-app's
/// `a_view_reports_seen_and_unread_never_goes_back`: a maximum is arithmetic, which the census
/// cannot see.
///
/// MUTATION (observed red): have `mark_leaf_painted` assign `leaf.last_seen_revision` again, and
/// the writer set names it.
#[test]
fn the_seen_revision_has_one_door() {
    let census = the_census();
    assert_eq!(
        non_read_rows(census, "ShellSession.last_seen_revision"),
        [("write", "ShellSession::mark_seen".to_owned())],
        "one door"
    );
    let callers = bt_app_callers();
    let door = app("ShellSession::mark_seen");
    let mut reporters: Vec<&str> = callers
        .by_callee
        .get(&door)
        .into_iter()
        .flatten()
        .map(|caller| last_segment(caller))
        .collect();
    reporters.sort_unstable();
    assert_eq!(
        reporters,
        ["mark_leaf_painted", "mark_leaf_seen"],
        "the reporters"
    );
}

// ── the doors between a view and its session (O-3) ─────────────────────────

/// **Who calls what in `bt-app`'s product**, through `bt_source::call_graph`
/// over the workspace's packages: for every call edge out of a product callable
/// of `bt-app` that resolves to an item, the callee's key and the caller's
/// (`Resolver::key`: `package module::Owner::name`). Calls written inside a
/// closure of an item are the item's. An edge the rules cannot follow is kept
/// with its candidates and the names it spells (the site's name, and the macro,
/// path, generator or cut-off item the edge names), so a guard can refuse one
/// that could reach what it pins instead of passing it in silence.
struct Callers {
    by_callee: BTreeMap<String, BTreeSet<String>>,
    unfollowed: Vec<Unfollowed>,
}

/// One call edge the rules cannot follow, out of `caller`.
struct Unfollowed {
    caller: String,
    what: String,
    candidates: Vec<String>,
    spelled: Vec<String>,
}

/// The last `::` segment of a key or a written path: the name an edge spells.
fn last_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

impl Callers {
    /// The callers of `callee`, sorted; empty when nothing calls it.
    fn of(&self, callee: &str) -> Vec<String> {
        self.by_callee
            .get(callee)
            .map(|callers| callers.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Every unfollowed edge that could reach one of `callees`: one of its
    /// candidates is one of them, or it spells one of their names.
    fn unfollowed_into(&self, callees: &[&str]) -> Vec<String> {
        self.unfollowed
            .iter()
            .filter(|edge| {
                edge.candidates
                    .iter()
                    .any(|candidate| callees.contains(&candidate.as_str()))
                    || edge.spelled.iter().any(|spelled| {
                        callees
                            .iter()
                            .any(|callee| last_segment(callee) == last_segment(spelled))
                    })
            })
            .map(|edge| format!("{} -> `{}` (unfollowed)", edge.caller, edge.what))
            .collect()
    }
}

/// The callers as the tree holds them now, built by each test that asks: the
/// test's own value, not a process-global its tests share (docs/plans/TEST-GLOBALS.tsv).
fn bt_app_callers() -> Callers {
    let workspace = Workspace::read(&workspace_root()).expect("this workspace");
    let held: Vec<(String, Arc<Index>)> = workspace
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
            let index = Index::shared(&universe)
                .unwrap_or_else(|rejections| panic!("{}", report(&rejections)));
            (package.name().to_owned(), index)
        })
        .collect();
    let packages: Vec<(&str, &Index)> = held
        .iter()
        .map(|(name, index)| (name.as_str(), &**index))
        .collect();
    let resolver =
        Resolver::new(&packages, Arms::product()).unwrap_or_else(|refusals| panic!("{refusals:?}"));
    let mut by_callee: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut unfollowed = Vec::new();
    for caller in resolver.callables("bt-app") {
        if !resolver.admitted(caller) {
            continue;
        }
        let from = resolver.key(caller);
        for edge in resolver.resolve_callees(caller) {
            let Target::Unresolved(unresolved) = &edge.to else {
                if let Target::Item(callee) = &edge.to {
                    by_callee
                        .entry(resolver.key(*callee))
                        .or_default()
                        .insert(from.clone());
                }
                continue;
            };
            let (what, candidates) = match unresolved {
                Unresolved::Ambiguous { method, candidates } => {
                    (method.clone(), candidates.clone())
                }
                Unresolved::Dynamic { what, candidates } => (what.clone(), candidates.clone()),
                Unresolved::OtherArm { candidates } => (edge.site.name.clone(), candidates.clone()),
                Unresolved::Macro { name } => (name.clone(), Vec::new()),
                Unresolved::Generated { by } => (by.clone(), Vec::new()),
                Unresolved::Path { written, .. } => (written.clone(), Vec::new()),
                Unresolved::Cutoff { what } => (what.clone(), Vec::new()),
                Unresolved::Unparsed { .. } => (edge.site.name.clone(), Vec::new()),
            };
            unfollowed.push(Unfollowed {
                caller: from.clone(),
                spelled: vec![edge.site.name.clone(), what.clone()],
                what,
                candidates: candidates.iter().map(|id| resolver.key(*id)).collect(),
            });
        }
    }
    Callers {
        by_callee,
        unfollowed,
    }
}

/// `bt-term`'s session item `name`, keyed as the call graph keys it.
fn term(name: &str) -> String {
    format!("bt-term crate::session::DualPlaneSession::{name}")
}

/// `bt-app`'s item `path` (under `crate::`), keyed as the call graph keys it.
fn app(path: &str) -> String {
    format!("bt-app crate::{path}")
}

/// RED (O-3; the ownership contract's (b)1) — **every sizing write is a size
/// proposal**: the session's size changes after its birth only through the
/// three stages of its one door, `ShellSession::propose_cell` (the cell size),
/// `propose_layout` (the layout key) and `propose_size` (the grid).
///
/// Two halves, because each sees what the other cannot.
/// - **The census**: every proven write of `ShellSession.session` that calls one
///   of the session's sizing setters is in a stage, and `ShellSession.grid`'s
///   writers (and `pending_pty_resize`'s lenders) are the grid stage and the
///   session's own release, `release_due_leaf_resize`.
/// - **The call graph**, which also sees a call on a `&mut DualPlaneSession`
///   handed to a helper (no field site at all): in `bt-app`'s product the
///   setters' callers are the stages and the constructor, `bare_leaf`, which
///   sets them on a session before there is a leaf; `resize_at`'s are the grid
///   stage and `commit_leaf_resize`, the release's local reflow; and
///   `DualPlaneSession::resize`, bt-term's own `resize_at` at the present
///   instant, has none (its call into `resize_at` is bt-term's, which this graph
///   does not walk, so it is pinned by its own name). An edge the graph cannot
///   follow is refused when a candidate or a name it spells is one of them,
///   never passed.
///
/// MUTATION: have `apply_line_wrapping` call `leaf.session.set_layout_key`
/// again, or `apply_leaf_metrics` call `set_font_size_subpixels`; have a
/// panes walk write `leaf.grid` or call `leaf.session.resize`; or call
/// `set_layout_key` from a helper that is handed `&mut leaf.session` — each is
/// named.
#[test]
fn every_sizing_write_is_a_size_proposal() {
    const SETTERS: [&str; 7] = [
        "set_cell_height_subpixels",
        "set_cell_width_subpixels",
        "set_ascii_baseline_subpixels",
        "set_font_size_subpixels",
        "set_layout_key",
        "resize_at",
        "resize",
    ];
    const STAGES: [&str; 3] = [
        "ShellSession::propose_cell",
        "ShellSession::propose_layout",
        "ShellSession::propose_size",
    ];
    let census = the_census();
    let bypassing: Vec<String> = census
        .rows()
        .iter()
        .filter(|row| row.fact == "ShellSession.session" && row.column.name() == "write")
        .filter(|row| {
            row.kinds.iter().any(|kind| {
                kind.strip_prefix("call:")
                    .is_some_and(|name| SETTERS.contains(&name))
            })
        })
        .filter(|row| !STAGES.contains(&row.function.as_str()))
        .map(|row| format!("{}::{} {:?}", row.module, row.function, row.kinds))
        .collect();
    assert!(
        bypassing.is_empty(),
        "a sizing write that is not a size proposal: {bypassing:?}"
    );
    let functions = |fact: &str, column: &str| -> Vec<String> {
        let mut out: Vec<String> = census
            .rows()
            .iter()
            .filter(|row| row.fact == fact && row.column.name() == column)
            .map(|row| row.function.clone())
            .collect();
        out.sort();
        out
    };
    assert_eq!(
        functions("ShellSession.grid", "write"),
        ["ShellSession::propose_size", "release_due_leaf_resize"],
        "the grid moves by the grid stage and the session's release"
    );
    assert_eq!(
        functions("ShellSession.pending_pty_resize", "access"),
        ["ShellSession::propose_size", "release_due_leaf_resize"],
        "and so does the resize it owes the child"
    );

    let callers = bt_app_callers();
    let pinned: Vec<String> = SETTERS.iter().map(|name| term(name)).collect();
    let pinned: Vec<&str> = pinned.iter().map(String::as_str).collect();
    let unfollowed = callers.unfollowed_into(&pinned);
    assert!(
        unfollowed.is_empty(),
        "a call the graph cannot follow may reach a sizing setter: {unfollowed:?}"
    );
    let cell = app("ShellSession::propose_cell");
    let birth = app("bare_leaf");
    for (setter, expected) in [
        ("set_cell_height_subpixels", vec![cell.clone()]),
        (
            "set_cell_width_subpixels",
            vec![cell.clone(), birth.clone()],
        ),
        (
            "set_ascii_baseline_subpixels",
            vec![cell.clone(), birth.clone()],
        ),
        ("set_font_size_subpixels", vec![cell.clone(), birth.clone()]),
        (
            "set_layout_key",
            vec![app("ShellSession::propose_layout"), birth.clone()],
        ),
        (
            "resize_at",
            vec![app("ShellSession::propose_size"), app("commit_leaf_resize")],
        ),
        ("resize", vec![]),
    ] {
        let mut expected = expected;
        expected.sort();
        assert_eq!(
            callers.of(&term(setter)),
            expected,
            "`{setter}` is reached only by a size proposal"
        );
    }
}

/// RED (O-3; the ownership contract's (b)1, group 3) — **each `bt-term`
/// view-state setter is reached only through its port**, and each port's
/// callers are these.
///
/// The selection, the math hover, a toggle's presentation and source, a math
/// block's own scroll and the presentation hold are a view's state that lives in
/// `DualPlaneSession` until a `bt-term` note splits it out. `TermViewPorts`
/// names the road, so the road can be counted: in `bt-app`'s product each
/// setter's only caller is its port, and each port's callers are the list
/// below, which a new caller or a lost one changes. An edge the graph cannot
/// follow is refused when a candidate or a name it spells is a setter or a port.
///
/// MUTATION: have `Runtime::set_hovered_math` call `set_math_hover` directly,
/// or call `port_clear_selection` from a new item — each is named.
#[test]
fn each_view_state_port_is_the_only_road_to_its_setter() {
    let port = |name: &str| app(&format!("<DualPlaneSession as TermViewPorts>::{name}"));
    let callers = bt_app_callers();
    let table: [(&str, &[&str]); 6] = [
        (
            "set_view_selection",
            &["port_clear_selection", "port_set_selection"],
        ),
        ("set_math_hover", &["port_math_hover"]),
        (
            "set_math_toggle_presentation",
            &["port_math_toggle_presentation"],
        ),
        ("toggle_math_source", &["port_toggle_math_source"]),
        ("scroll_math_block", &["port_scroll_math_block"]),
        (
            "release_presentation_hold_for_user_input",
            &["port_release_presentation_hold"],
        ),
    ];
    let mut every: Vec<String> = Vec::new();
    for (setter, ports) in table {
        every.push(term(setter));
        let mut expected: Vec<String> = ports.iter().map(|name| port(name)).collect();
        expected.sort();
        assert_eq!(
            callers.of(&term(setter)),
            expected,
            "`{setter}` is reached only through its port"
        );
    }
    let port_callers: [(&str, &[&str]); 7] = [
        ("port_set_selection", &["TabState::set_leaf_selection"]),
        (
            "port_clear_selection",
            &[
                "TabState::clear_leaf_selection",
                "runtime::clipboard::clear_copied_selection",
                "runtime::clipboard::paste_spelled",
            ],
        ),
        (
            "port_math_hover",
            &["runtime::math::Runtime::set_hovered_math"],
        ),
        (
            "port_math_toggle_presentation",
            &[
                "runtime::math::Runtime::present_math_toggle",
                "runtime::math::Runtime::sample_math_toggle",
                "runtime::math::Runtime::settle_math_toggle",
            ],
        ),
        (
            "port_toggle_math_source",
            &[
                "runtime::math::Runtime::press_math_toggle",
                "runtime::math::Runtime::settle_math_toggle",
                "runtime::math::Runtime::switch_math_source_now",
            ],
        ),
        (
            "port_scroll_math_block",
            &["runtime::mouse::Runtime::mouse_wheel"],
        ),
        (
            "port_release_presentation_hold",
            &[
                "Runtime::return_to_live_for_input",
                "runtime::frame::Runtime::publish_frame_inner",
            ],
        ),
    ];
    for (name, expected) in port_callers {
        every.push(port(name));
        let mut expected: Vec<String> = expected.iter().map(|path| app(path)).collect();
        expected.sort();
        assert_eq!(callers.of(&port(name)), expected, "`{name}`'s callers");
    }
    let every: Vec<&str> = every.iter().map(String::as_str).collect();
    let unfollowed = callers.unfollowed_into(&every);
    assert!(
        unfollowed.is_empty(),
        "a call the graph cannot follow may reach a view-state setter or its port: {unfollowed:?}"
    );
}

/// RED (O-3; the ownership contract's (b)1 and (c)1) — **the session hears where
/// the keyboard is through one door, `ShellSession::report_keyboard_focus`, and
/// only the drain reports it.**
///
/// The window and the view compute the truth and the session consumes it
/// (`DualPlaneSession::set_keyboard_focus` keeps the DEC 1004 state and queues
/// the child's reports). So in `bt-app`'s product the setter's one caller is the
/// door, and the door's one caller is `drain_leaf_pty`, which asks it after the
/// child's bytes are fed.
///
/// MUTATION: have the drain call `leaf.session.set_keyboard_focus` directly,
/// or report from a second item — each is named.
#[test]
fn the_keyboard_focus_is_one_report() {
    let callers = bt_app_callers();
    let setter = term("set_keyboard_focus");
    let door = app("ShellSession::report_keyboard_focus");
    assert_eq!(callers.of(&setter), std::slice::from_ref(&door), "one door");
    assert_eq!(callers.of(&door), [app("drain_leaf_pty")], "one reporter");
    let unfollowed = callers.unfollowed_into(&[setter.as_str(), door.as_str()]);
    assert!(
        unfollowed.is_empty(),
        "a call the graph cannot follow may reach the focus report: {unfollowed:?}"
    );
}

// ── the shell's life (O-4b) ──────────────────────────────────────────────

/// `bt-app`'s `ShellLife::name`, keyed as the call graph keys it.
fn life(name: &str) -> String {
    app(&format!("ShellLife::{name}"))
}

/// `bt-app`'s `TerminalPane::name`, keyed as the call graph keys it: the successor's methods,
/// which O-5b moved from the session record to the pane (ruling E5).
fn pane(name: &str) -> String {
    app(&format!("TerminalPane::{name}"))
}

impl Callers {
    /// The callers of `callee`, sorted, counting an edge the graph could not follow when its one
    /// candidate is `callee`: a method call through a receiver the rules do not type (a binding
    /// of a `?`'s value, a box bound by a pattern) whose name only `callee` declares.
    fn of_counting_sole(&self, callee: &str) -> Vec<String> {
        let mut out: BTreeSet<String> = self.of(callee).into_iter().collect();
        out.extend(
            self.unfollowed
                .iter()
                .filter(|edge| edge.candidates == [callee])
                .map(|edge| edge.caller.clone()),
        );
        out.into_iter().collect()
    }

    /// What `caller` calls, sorted, counting the unfollowed edges [`Self::of_counting_sole`]
    /// counts.
    fn callees_of(&self, caller: &str) -> Vec<String> {
        let mut out: BTreeSet<String> = self
            .by_callee
            .iter()
            .filter(|(_, callers)| callers.contains(caller))
            .map(|(callee, _)| callee.clone())
            .collect();
        out.extend(
            self.unfollowed
                .iter()
                .filter(|edge| edge.caller == caller && edge.candidates.len() == 1)
                .map(|edge| edge.candidates[0].clone()),
        );
        out.into_iter().collect()
    }

    /// [`Self::unfollowed_into`], less the edges whose one candidate is one of `callees`: those
    /// [`Self::of_counting_sole`] counts as calls.
    fn unfollowed_into_beyond_sole(&self, callees: &[&str]) -> Vec<String> {
        self.unfollowed
            .iter()
            .filter(|edge| {
                !(edge.candidates.len() == 1 && callees.contains(&edge.candidates[0].as_str()))
            })
            .filter(|edge| {
                edge.candidates
                    .iter()
                    .any(|candidate| callees.contains(&candidate.as_str()))
                    || edge.spelled.iter().any(|spelled| {
                        callees
                            .iter()
                            .any(|callee| last_segment(callee) == last_segment(spelled))
                    })
            })
            .map(|edge| format!("{} -> `{}` (unfollowed)", edge.caller, edge.what))
            .collect()
    }
}

/// RED (O-4b; the ownership contract's M6, rulings D8 and E5) — **each life transition has its
/// callers**, and each of the record's successor moves its.
///
/// The census proves which functions write the life; this proves which call each transition, so
/// a road that takes another road's edge — the landing keeping a pane itself, a close road
/// releasing a dead shell — changes a row here. An edge the graph cannot follow is refused when a
/// candidate or a name it spells is one of them.
///
/// MUTATION: have `land_birth_death_fallback` call `leaf.life.keep_birth_death()` itself instead of going
/// through `keep_birth_death_face`; have `end_leaves_of` call `release_dead_shell`; have
/// `restart_shell` call `take_successor` — each is named.
#[test]
fn each_life_transition_has_its_callers() {
    let callers = bt_app_callers();
    let reaper = "runtime::tabs::Runtime::reap_exited_tabs";
    let table: [(String, &[&str]); 17] = [
        (life("shellless"), &["bare_leaf"]),
        (
            life("begin_birth"),
            &["await_shell", "create_leaf_session", "land_shell_birth"],
        ),
        (life("take_birth"), &["land_shell_birth"]),
        (life("record_spawn_fallback"), &["finish_leaf_birth"]),
        (
            life("take_spawn_fallback"),
            &[
                "land_birth_death_fallback_shell",
                "runtime::panes::Runtime::land_own_shell",
                "runtime::panes::Runtime::land_restart",
            ],
        ),
        (life("land"), &["finish_leaf_birth"]),
        (life("owe_landing_size"), &["release_due_leaf_resize"]),
        (life("exit_draining"), &[reaper]),
        (life("exited"), &[reaper]),
        (life("release_dead_shell"), &[reaper]),
        (
            life("fall_back"),
            &[reaper, "runtime::tabs::land_birth_death_fallback"],
        ),
        (
            life("take_fallback"),
            &["runtime::tabs::land_birth_death_fallback"],
        ),
        (
            life("keep_birth_death"),
            &["runtime::tabs::keep_birth_death_face"],
        ),
        (
            life("retire"),
            &[
                "TabState::retire_all_shells",
                "runtime::panes::end_leaves_of",
            ],
        ),
        (
            pane("await_successor"),
            &[
                "runtime::panes::Runtime::land_pane_birth",
                "runtime::terminal::Runtime::restart_shell",
            ],
        ),
        (pane("successor_mut"), &["runtime::panes::land_successor"]),
        (pane("take_successor"), &["runtime::panes::land_successor"]),
    ];
    let mut every: Vec<String> = Vec::new();
    for (callee, expected) in table {
        let mut expected: Vec<String> = expected.iter().map(|path| app(path)).collect();
        expected.sort();
        assert_eq!(
            callers.of_counting_sole(&callee),
            expected,
            "`{callee}`'s callers"
        );
        every.push(callee);
    }
    let every: Vec<&str> = every.iter().map(String::as_str).collect();
    let unfollowed = callers.unfollowed_into_beyond_sole(&every);
    assert!(
        unfollowed.is_empty(),
        "a call the graph cannot follow may reach a life transition: {unfollowed:?}"
    );
}

/// RED (O-4b; the ownership contract's M6, ruling D9) — **a life transition performs no
/// effect**: no method of `ShellLife`, and none of the successor moves (the pane's since O-5b),
/// calls any item of the workspace but `ShellLife`'s own — so none reaches
/// `bt_pty::retire_session`, `pty_door`, `DualPlaneSession::feed` or any effect of
/// `window_waits.tsv`. Those stay on the roads that take the edge, in the order they had. Nor does
/// any of them make a call the graph cannot follow.
///
/// MUTATION: move `bt_pty::retire_session(pty)` into `ShellLife::retire`; move the
/// `pty_door::request` into `fall_back` — each is named.
#[test]
fn a_life_transition_performs_no_effect() {
    let callers = bt_app_callers();
    let pure = |key: &str| {
        key.starts_with(&life(""))
            || SUCCESSOR_MOVES
                .iter()
                .any(|name| key == pane(name).as_str())
    };
    let mut reached: Vec<String> = Vec::new();
    let mut walked = 0;
    for (callee, from) in &callers.by_callee {
        for caller in from.iter().filter(|caller| pure(caller)) {
            walked += 1;
            if !callee.starts_with(&life("")) {
                reached.push(format!("{caller} -> {callee}"));
            }
        }
    }
    assert!(walked > 0, "the transitions' own calls are found");
    assert!(
        reached.is_empty(),
        "a life transition calls out of the life: {reached:?}"
    );
    let unfollowed: Vec<String> = callers
        .unfollowed
        .iter()
        .filter(|edge| pure(&edge.caller))
        // `matches!` is the standard library's, and expands to a `match` with no call in it.
        // `debug_assert!` is the standard library's too: the entry transitions' refusal of a wrong
        // phase (review F2), a panic in a debug build only and compiled out of a release one.
        .filter(|edge| {
            !(edge.candidates.is_empty()
                && matches!(edge.what.as_str(), "matches" | "debug_assert"))
        })
        .map(|edge| format!("{} -> `{}`", edge.caller, edge.what))
        .collect();
    assert!(
        unfollowed.is_empty(),
        "a life transition makes a call the graph cannot follow: {unfollowed:?}"
    );
}

/// RED (O-4b; the ownership contract's (c)3, ruling E5) — **a restart asks only whether one is in
/// flight**: of the successor methods (the pane's since O-5b), `restart_shell` calls only
/// `restart_in_flight` and `await_successor`, and of `ShellLife` only `birth` — the replacement's
/// question, whether it waits — never a phase of the pane it replaces; `land_pane_birth`'s
/// successor road asks the same of the replacement it made.
///
/// The graph cannot tell the replacement's life from the old pane's, so a restart that asked the
/// old pane's phase through `birth` would pass; one that asks it through any other read or
/// transition (`leaf.life.pty().is_none()` → refuse) adds a callee and is named.
///
/// MUTATION: have `restart_shell` refuse unless `leaf.life.pty().is_some()` — named.
#[test]
fn a_restart_asks_only_whether_one_is_in_flight() {
    let callers = bt_app_callers();
    let asked = |caller: &str| -> (Vec<String>, Vec<String>) {
        let callees = callers.callees_of(&app(caller));
        let of_life = callees
            .iter()
            .filter(|callee| callee.starts_with(&life("")))
            .cloned()
            .collect();
        let of_record = callees
            .iter()
            .filter(|callee| {
                [
                    "restart_in_flight",
                    "await_successor",
                    "successor_mut",
                    "take_successor",
                    "birth_at",
                ]
                .iter()
                .any(|name| callee.as_str() == pane(name))
            })
            .cloned()
            .collect();
        (of_life, of_record)
    };
    assert_eq!(
        asked("runtime::terminal::Runtime::restart_shell"),
        (
            vec![life("birth")],
            vec![pane("await_successor"), pane("restart_in_flight")]
        ),
        "a restart asks the record whether one is in flight, and the replacement whether it waits"
    );
    let (of_life, _) = asked("runtime::panes::Runtime::land_pane_birth");
    assert_eq!(
        of_life,
        [life("birth")],
        "a successor made again is asked only whether it waits"
    );
}

// ── the runtime dependency map (J3) ───────────────────────────────────────

/// The lines the map's guard compares: all of them but the commit line the
/// generating script writes.
fn compared(text: &str) -> Vec<&str> {
    text.lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.starts_with(MEASURED_AT))
        .collect()
}

/// What differs between a committed file of the map and its rendering,
/// named by row.
fn map_differences(name: &str, committed: &str, rendered: &str) -> Vec<String> {
    let committed = compared(committed);
    let rendered = compared(rendered);
    let then: std::collections::BTreeSet<&str> = committed.iter().copied().collect();
    let now: std::collections::BTreeSet<&str> = rendered.iter().copied().collect();
    let mut out: Vec<String> = rendered
        .iter()
        .filter(|line| !then.contains(*line))
        .map(|line| format!("{name}: the code says a row the committed file does not: {line}"))
        .collect();
    out.extend(
        committed
            .iter()
            .filter(|line| !now.contains(*line))
            .map(|line| format!("{name}: the committed file says a row the code does not: {line}")),
    );
    if out.is_empty() && committed != rendered {
        out.push(format!("{name}: the same rows in another order"));
    }
    out
}

/// RED (J3) — **the committed runtime dependency map is the code's map**:
/// each of the four files under `docs/plans/design/runtime-dep-map/` equals
/// the rendering of `bt-app` at this tree, line for line, but for the line
/// naming the commit it was measured at. The renderings are left in
/// `target/depmap/` for `scripts/generate-runtime-dep-map.ps1` to copy.
///
/// MUTATION: change a count in one committed row of `doors.tsv` (or delete a
/// row), and this names the row on both sides.
#[test]
fn the_committed_runtime_dep_map_is_the_code() {
    let census = the_census();
    assert!(
        census.unparsed().is_empty(),
        "every declaration parses: {:#?}",
        census.unparsed()
    );
    let map = DependencyMap::of(census, Index::of_package("bt-app"));
    let out = workspace_root().join("target").join("depmap");
    std::fs::create_dir_all(&out).expect("target is writable");
    let committed_dir = workspace_root()
        .join("docs")
        .join("plans")
        .join("design")
        .join("runtime-dep-map");
    let mut found = Vec::new();
    for (name, rendered) in map.render() {
        std::fs::write(out.join(name), &rendered).expect("written");
        let committed = std::fs::read_to_string(committed_dir.join(name)).unwrap_or_default();
        found.extend(map_differences(name, &committed, &rendered));
    }
    assert!(
        found.is_empty(),
        "{} difference(s) between the committed runtime dependency map and the code; \
         regenerate it with scripts/generate-runtime-dep-map.ps1 (never edit a row by hand):\n{}",
        found.len(),
        found
            .iter()
            .take(40)
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
