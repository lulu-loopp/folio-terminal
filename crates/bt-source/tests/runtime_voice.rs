//! **A runtime road says its `diagnostics.log` line through the window's door**
//! (T-DIAGNOSTICS-TEST-SINK; `docs/ARCHITECTURE-GUARDS.md`, "Runtime voice"). A guard: its
//! subject is how the code is written.
//!
//! `diagnostics::note` writes to the resident log and, on a console run, to the trace sink's
//! queue — both process-wide, neither of them anything a test can read. A `Runtime` method has
//! the window in hand, and the window seam's `WindowHost::note` is the same line said through the
//! window: its native arm calls `diagnostics::note`, the headless backend's records the exact
//! text on the window's ledger (`WindowEffect::Noted`), so a headless test reads back the line
//! the road wrote rather than the line a builder would have written. This guard holds the roads
//! to that door: every mention of the voice inside an `impl` block of `Runtime` (inherent and
//! trait blocks alike, on every arm, in the product) is a **direct line**, and the direct lines
//! are exactly the rows of `docs/plans/runtime-voice-direct-lines.tsv` — the roads no headless
//! test drives yet, each with its reason. The list only shrinks (`scripts/ci/check-runtime-voice.ps1`
//! holds it to the merge base's): a new road says its line through the door, and a road a
//! headless test comes to drive moves to the door with that test.
//!
//! # What it reads, and what it refuses by name
//!
//! * **A direct call** — `diagnostics::note(…)`, the path written with any prefix
//!   (`crate::diagnostics::note`), in a closure or a macro's tokens of a method — counted by the
//!   innermost item holding it.
//! * **The voice handed on as a value** — the same path not followed by `(`: given to a helper as
//!   its `say`, or named by a `use` inside a body. Counted the same way, as its own shape.
//! * **A bare `note`** in a runtime road that is neither the tail of that path nor a method or
//!   field after `.` (the door is `self.window.window.note(…)`): a name brought in by a `use`, a
//!   renamed module's `d::note`, a local of that name. The guard cannot tell it from the voice,
//!   so it is refused by its line — noise, never silence: spell the call through the door.
//! * **A `use` anywhere in the package that names the voice or opens `diagnostics` by a glob**
//!   (module level, the index's `use` records): a call through the name it gives is a call this
//!   guard cannot see, so the `use` itself is refused by its line.
//!
//! Not read, as limits: a free function a runtime method calls that itself says a line (the line
//! is that function's, and the function has no window — `exit_diagnostics`' helpers take the voice
//! as a parameter, which is how a road hands them the door), a stored closure or function pointer
//! holding the voice, and a type other than `Runtime` that holds a backend (`FolioApp`, `App`:
//! neither has a window in hand).
//!
//! The fixture under `tests/fixtures/runtime_voice/` plants each refusal; the last test holds
//! `bt-app` itself.

use std::collections::BTreeMap;
use std::path::Path;

use bt_source::{
    Certainty, DiskScope, Index, Occurrence, Pattern, Scope, Search, Span, TargetId, TargetKind,
    TargetRoot, Universe, Vendor, View, needle, report,
};

/// How a direct line names the voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Shape {
    /// `diagnostics::note(…)`.
    Call,
    /// `diagnostics::note` handed on, or named by a body's `use`.
    Value,
}

/// One row of a direct-line list: the road (the item, as `bt-source` prints its identity), how
/// it names the voice, how many times, and why it is not through the door yet.
type Row = (String, Shape, usize, String);

/// **`bt-app`'s runtime roads that still say their line directly**, each with the reason, read
/// from its committed file (`docs/plans/runtime-voice-direct-lines.tsv`). The list only shrinks:
/// `scripts/ci/check-runtime-voice.ps1` holds it to the merge base's.
fn bt_app_direct_lines() -> Vec<Row> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("plans")
        .join("runtime-voice-direct-lines.tsv");
    let text = std::fs::read_to_string(&path).expect("the direct-line list is committed");
    let mut rows = Vec::new();
    let mut header = false;
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !header {
            assert_eq!(
                line, "road\tshape\tcount\treason",
                "the direct-line list's header"
            );
            header = true;
            continue;
        }
        let columns: Vec<&str> = line.split('\t').collect();
        let [road, shape, count, reason] = columns[..] else {
            panic!("a direct-line row is four columns: {line}");
        };
        let shape = match shape {
            "call" => Shape::Call,
            "value" => Shape::Value,
            _ => panic!("a direct-line row's shape is call or value: {line}"),
        };
        let count = count
            .parse()
            .unwrap_or_else(|_| panic!("a direct-line row's count is a number: {line}"));
        rows.push((road.to_owned(), shape, count, reason.to_owned()));
    }
    rows
}

/// What the guard found: the direct lines by road and shape, and every refusal of a shape it does
/// not read.
struct Census {
    lines: BTreeMap<(String, Shape), usize>,
    unread: Vec<String>,
}

/// **The census of one package**: every mention of `voice` (`diagnostics::note`) in the product
/// `impl` blocks of `holder`, and every shape the guard refuses.
fn census(index: &Index, holder: &str, voice: &str) -> Census {
    let found = |pattern: Pattern, scope: Scope| {
        index
            .search(&Search::new(needle!(pattern), View::Identifiers).in_scope(scope))
            .unwrap_or_else(|failure| panic!("{failure}"))
            .in_the_product(index)
    };
    let tail = voice
        .rsplit("::")
        .next()
        .expect("a path has a last segment");
    let roads = Scope::Impls(holder.to_owned());
    let mentions = found(Pattern::path(voice), roads.clone());
    let mut lines = BTreeMap::new();
    let mut tails = Vec::new();
    for occurrence in mentions.occurrences() {
        let shape = if next_char(index, occurrence.span) == Some('(') {
            Shape::Call
        } else {
            Shape::Value
        };
        let owners = mentions_owner(index, *occurrence);
        assert!(
            !owners.is_empty(),
            "a mention inside an impl block is inside one of its items"
        );
        for owner in owners {
            *lines.entry((owner, shape)).or_insert(0) += 1;
        }
        tails.push(occurrence.span.end());
    }
    let mut unread = Vec::new();
    for occurrence in found(Pattern::identifier(tail), roads).occurrences() {
        if tails.contains(&occurrence.span.end())
            || previous_char(index, occurrence.span) == Some('.')
        {
            continue;
        }
        unread.push(format!(
            "{}: a bare `{tail}` in a runtime road — the guard reads `{voice}` by its path, and a \
             name a `use`, a renamed module or a local gives it is one it cannot tell from the \
             voice; say the line through the window's door (`WindowHost::note`)",
            where_is(index, occurrence.span)
        ));
    }
    let segments: Vec<&str> = voice.split("::").collect();
    let module = &segments[..segments.len() - 1];
    for record in index.uses() {
        let product = Occurrence {
            span: record.whole(),
            certainty: Certainty::Resolved,
        }
        .in_the_product(index);
        if !product {
            continue;
        }
        let names_the_voice = record.names().iter().any(|(_, path)| {
            path.ends_with(&segments.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
        });
        let opens_its_module = record.globs().iter().any(|path| {
            path.ends_with(&module.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
        });
        if names_the_voice || opens_its_module {
            unread.push(format!(
                "{}: a `use` that names `{voice}` or opens its module by a glob — a call through \
                 the name it gives is one the guard cannot see; spell the path where it is said",
                where_is(index, record.whole())
            ));
        }
    }
    Census { lines, unread }
}

/// The items holding an occurrence, as `bt-source` prints their identities.
fn mentions_owner(index: &Index, occurrence: Occurrence) -> Vec<String> {
    let found = BTreeMap::from_iter(
        index
            .items()
            .iter()
            .filter(|record| occurrence.span.within(record.whole()))
            .min_by_key(|record| record.whole().len())
            .into_iter()
            .flat_map(|record| {
                record
                    .identities()
                    .map(|identity| (identity.to_string(), ()))
            }),
    );
    found.into_keys().collect()
}

/// The first character after `span` that is not whitespace, within its file.
fn next_char(index: &Index, span: Span) -> Option<char> {
    let file = index.file_at(span.start())?;
    let text = index.text(file.span());
    text[span.end() - file.span().start()..]
        .chars()
        .find(|c| !c.is_whitespace())
}

/// The last character before `span` that is not whitespace, within its file.
fn previous_char(index: &Index, span: Span) -> Option<char> {
    let file = index.file_at(span.start())?;
    let text = index.text(file.span());
    text[..span.start() - file.span().start()]
        .chars()
        .rev()
        .find(|c| !c.is_whitespace())
}

fn where_is(index: &Index, span: Span) -> String {
    index
        .locate(span.start())
        .map_or_else(|| format!("{span:?}"), |location| location.to_string())
}

/// **The census against a list**: each direct line not on it (or more of them than it says), each
/// row whose road says fewer (or none), and each shape the guard does not read.
fn judge(census: &Census, list: &[Row]) -> Vec<String> {
    let mut refusals = census.unread.clone();
    let listed: BTreeMap<(String, Shape), usize> = list
        .iter()
        .map(|(owner, shape, count, reason)| {
            assert!(!reason.is_empty(), "a row says why: {owner}");
            ((owner.clone(), *shape), *count)
        })
        .collect();
    assert_eq!(listed.len(), list.len(), "a road and shape is one row");
    for ((owner, shape), count) in &census.lines {
        match listed.get(&(owner.clone(), *shape)) {
            Some(rows) if rows == count => {}
            Some(rows) => refusals.push(format!(
                "{owner}: {count} direct line(s) ({shape:?}) where its row says {rows} — a line \
                 added to a runtime road is said through the window's door (`WindowHost::note`); \
                 one taken away takes its count down"
            )),
            None => refusals.push(format!(
                "{owner}: {count} direct line(s) ({shape:?}) in a runtime road that is on no row \
                 — say it through the window's door (`WindowHost::note`), where a headless test \
                 reads it back"
            )),
        }
    }
    for ((owner, shape), _) in listed {
        if !census.lines.contains_key(&(owner.clone(), shape)) {
            refusals.push(format!(
                "{owner}: a row ({shape:?}) for a road that says no direct line — take it off"
            ));
        }
    }
    refusals
}

/// The fixture crate.
fn fixture() -> Index {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("runtime_voice");
    let universe = Universe::declare(
        "the runtime-voice fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "runtime-voice".to_owned(),
                kind: TargetKind::Library,
                name: "runtime_voice".to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

/// RED (planted) — **each direct line off the list, and each shape the guard does not read, is
/// named, and nothing else is.** The fixture plants a direct call inside a closure of a method, the
/// voice handed on as a value, the voice brought in by a body's `use` and called bare, the voice
/// reached through a renamed module, a module-level `use` renaming it and a module's glob over
/// the voice's module; the list holds a row
/// whose road says one line where it says two, and a row for a road that says none. A line
/// through the window's door, a free function's line, a trait block's listed lines and a test
/// block's line pass.
#[test]
fn every_direct_line_in_a_runtime_road_is_named() {
    let index = fixture();
    let census = census(&index, "Runtime", "diagnostics::note");
    let list: Vec<Row> = [
        ("crate::Runtime::listed", 1, "the fixture's listed road"),
        (
            "crate::<Runtime as Clone>::clone",
            1,
            "the fixture's trait road, listed one short",
        ),
        (
            "crate::Runtime::gone",
            1,
            "a road that says nothing any more",
        ),
    ]
    .into_iter()
    .map(|(road, count, reason)| (road.to_owned(), Shape::Call, count, reason.to_owned()))
    .collect();
    let refusals = judge(&census, &list);
    let named = |needle: &str| {
        refusals
            .iter()
            .filter(|refusal| refusal.contains(needle))
            .count()
    };
    assert_eq!(
        named(
            "crate::Runtime::planted_call: 1 direct line(s) (Call) in a runtime road that is on no row"
        ),
        1,
        "{refusals:#?}"
    );
    assert_eq!(
        named("crate::Runtime::planted_value: 1 direct line(s) (Value)"),
        1,
        "{refusals:#?}"
    );
    assert_eq!(
        named("crate::Runtime::planted_bare: 1 direct line(s) (Value)"),
        1,
        "the body's `use` is the voice named as a value: {refusals:#?}"
    );
    assert_eq!(
        named("crate::<Runtime as Clone>::clone: 2 direct line(s) (Call) where its row says 1"),
        1,
        "{refusals:#?}"
    );
    assert_eq!(
        named("crate::Runtime::gone: a row (Call) for a road that says no direct line"),
        1,
        "{refusals:#?}"
    );
    assert_eq!(
        named("a bare `note` in a runtime road"),
        2,
        "the bare call and `d::note`: {refusals:#?}"
    );
    assert_eq!(
        named("a `use` that names `diagnostics::note`"),
        2,
        "the renaming `use` and the glob over its module: {refusals:#?}"
    );
    assert_eq!(refusals.len(), 9, "nothing else is refused: {refusals:#?}");
    assert!(
        !census
            .lines
            .keys()
            .any(|(owner, _)| owner.contains("through_the_door")
                || owner.contains("hand_on")
                || owner.contains("in_a_test")
                || owner.contains("Window::note")),
        "the door, a free function and a test block say no direct line: {:#?}",
        census.lines
    );
}

/// GUARD — **`bt-app`'s runtime roads say their lines through the window's door**, but for the
/// listed ones.
///
/// MUTATION: say `diagnostics::note(&line)` again in `Runtime::ime_input` — an unlisted direct
/// line, red; take a listed road's line through the door without taking its row off — a stale
/// row, red.
#[test]
fn bt_app_runtime_roads_say_their_lines_through_the_window() {
    let index = Index::of_package("bt-app");
    let census = census(index, "Runtime", "diagnostics::note");
    let refusals = judge(&census, &bt_app_direct_lines());
    assert!(
        refusals.is_empty(),
        "{}\n\nthe census: {:#?}",
        refusals.join("\n"),
        census.lines
    );
}
