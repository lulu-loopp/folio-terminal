//! **The crate root and every module outside `runtime/` reach runtime's items only through
//! `runtime/mod.rs`'s re-exports, never by a path into a topic module**
//! (T-ROOT-NEVER-IMPORTS-A-RUNTIME-TOPIC, plan row 80; `docs/ARCHITECTURE-GUARDS.md`, "Runtime
//! topics from outside"). A guard: its subject is how the code is written.
//!
//! Plan row 59 made `bt-app`'s runtime topic modules `pub(crate) mod` so that rustdoc could
//! resolve the re-pathed doc links, and from then on a path `runtime::<topic>::item` compiles at
//! the crate root and in any module outside `runtime/`, as a `use` or as a full path with no `use`
//! at all. K2's rule is that the root's one road into `runtime/` is what `runtime/mod.rs`
//! re-exports (`runtime::AfterTheWait`, `use runtime::platform_pointer_of;`). This guard holds
//! that rule: every path into a topic module written in a product build outside `runtime/` is a
//! finding, and `bt-app` has none.
//!
//! # The grammar
//!
//! A **topic module** is a child module of `crate::runtime`, read from the index's modules —
//! private ones included, so a later widening is caught the same way.
//!
//! | # | Shape | Read by |
//! |---|---|---|
//! | G1 | A path in an expression, a type, a pattern, an `impl` header, a generic argument or after a `<` (`<runtime::x::T as Tr>::m`), its head spelled `runtime`, `crate::runtime`, `self::runtime` or `super::…::runtime` | `Pattern::path("runtime::<topic>")` over [`View::Identifiers`]; the tokens before the match are read, and a head other than `crate`, `self` or `super` is refused by its line |
//! | G2 | A module-level `use` in any tree shape: `pub use`, a nested group, `self`, a rename, a glob | [`Index::uses`]: each name and glob with the path it was written as. A path into a topic is a finding; a `use` that names `runtime` itself under another name, or opens its names by a glob, is refused by its line — the paths written through the name it gives are paths G1 cannot see — and so is a `runtime` segment followed by a topic (or ending a glob or a rename) behind a head other than `crate`, `self` or `super` (`use c::runtime::pointer::X;` after `use crate as c;`) |
//! | G3 | A `use` inside a body (not an [`Index::uses`] record) | G1 finds `use runtime::x::y;`; a group or a glob after `runtime::`, and `runtime as …`, are refused by kind, naming the line, whatever the head |
//! | — | A comment inside a path (`runtime::/* … */pointer::X`) | G1's search compares the bytes between tokens and does not match it; the `runtime` token followed by `::` and a topic with anything else between is refused by its line |
//! | — | `runtime/mod.rs`'s own `use` items | the road itself: an item re-export is accepted; one whose path names a module in the index (`pub(crate) use pointer::touch;`), and any glob, re-exports a module under a new name and is refused by its line |
//! | G4 | A macro's arguments | G1's search, [`Certainty::Candidate`]: counted as a finding, its certainty printed |
//! | G5 | Doc comments and string literals | not code, not read: the identifier view masks them |
//!
//! **Product** is [`Occurrence::in_the_product`] (the file, the innermost item, the innermost
//! inline module) and, for a module-level `use`, **that `use`'s own `cfg`**
//! ([`bt_source::UseRecord::variant`]): a `use` stands in no item, so the item grain cannot see
//! its gate (plan row 77), and this guard reads the gate itself. A field's gate is the field
//! item's own. What no grain reads is a `#[cfg]` on a statement or a match arm inside a body: a
//! path under one is refused by kind — gate the item, or spell the path through `runtime/mod.rs`.
//!
//! The fixture under `tests/fixtures/root_topic_paths/` plants each shape; the last test holds
//! `bt-app` itself.

use std::collections::BTreeSet;
use std::path::Path;

use bt_source::{
    Certainty, DiskScope, Index, Occurrence, Pattern, Search, Span, TargetId, TargetKind,
    TargetRoot, TokenRecord, Universe, Vendor, View, needle, report,
};

/// What the guard found: each path into a topic module, and each shape it refuses to read.
#[derive(Debug, Default)]
struct Reading {
    findings: Vec<String>,
    refused: Vec<String>,
}

/// How the product is decided — the guard's own reading, and the variant-blind one that row 77
/// describes, kept so that the planted test-gated `use` proves the difference.
#[derive(Clone, Copy, PartialEq, Eq)]
enum UseGate {
    /// The `use`'s own `cfg` is read.
    Read,
    /// Only [`Occurrence::in_the_product`]: a `#[cfg(test)] use` at the root counts as product.
    Blind,
}

const RUNTIME: &str = "crate::runtime";

/// The child modules of `crate::runtime`, by name.
fn topics(index: &Index) -> BTreeSet<String> {
    index
        .modules()
        .iter()
        .flat_map(|module| module.module_paths())
        .filter_map(|path| path.strip_prefix("crate::runtime::"))
        .filter(|rest| !rest.contains("::"))
        .map(ToOwned::to_owned)
        .collect()
}

/// The module paths of the innermost module holding `offset`.
fn modules_at(index: &Index, offset: usize) -> Vec<String> {
    let innermost = index
        .modules()
        .iter()
        .filter(|module| module.span().holds(offset))
        .min_by_key(|module| module.span().len());
    match innermost {
        Some(module) => module.module_paths().to_vec(),
        None => index
            .file_at(offset)
            .map(|file| {
                file.owners()
                    .iter()
                    .map(|owner| owner.module_path.clone())
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Whether a module path is `runtime` or inside it.
fn inside_runtime(module: &str) -> bool {
    module == RUNTIME
        || module
            .strip_prefix(RUNTIME)
            .is_some_and(|rest| rest.starts_with("::"))
}

/// Whether any module holding `offset` is outside `runtime/`.
fn outside_runtime(index: &Index, offset: usize) -> bool {
    modules_at(index, offset)
        .iter()
        .any(|module| !inside_runtime(module))
}

fn where_is(index: &Index, offset: usize) -> String {
    index
        .locate(offset)
        .map_or_else(|| format!("{offset}"), |location| location.to_string())
}

/// The removed regions (comments and literals), sorted, so that the bytes between two tokens can
/// be read without them.
fn masks(index: &Index) -> Vec<Span> {
    let mut masks: Vec<Span> = index
        .comments()
        .iter()
        .map(|comment| comment.span())
        .chain(index.literals().iter().map(|literal| literal.span()))
        .collect();
    masks.sort_by_key(|span| span.start());
    masks
}

/// The bytes between two offsets of one file, with every removed region blanked.
fn between(index: &Index, masks: &[Span], from: usize, to: usize) -> String {
    let Some(file) = index.file_at(from) else {
        return String::new();
    };
    if to <= from || to > file.span().end() {
        return String::new();
    }
    let base = file.span().start();
    let first = masks.partition_point(|mask| mask.end() <= from);
    let near: Vec<Span> = masks[first..]
        .iter()
        .take_while(|mask| mask.start() < to)
        .copied()
        .collect();
    index.text(file.span())[from - base..to - base]
        .char_indices()
        .map(|(offset, character)| {
            let masked = near.iter().any(|mask| mask.holds(from + offset));
            if masked && !character.is_whitespace() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

/// The token whose span starts at `offset`.
fn token_at(tokens: &[TokenRecord], offset: usize) -> usize {
    let at = tokens.partition_point(|token| token.span().start() < offset);
    assert!(
        tokens
            .get(at)
            .is_some_and(|token| token.span().start() == offset),
        "a path match starts at a token"
    );
    at
}

/// The names of the `::`-joined tokens written before token `at`, outermost first.
fn head_before(index: &Index, masks: &[Span], tokens: &[TokenRecord], at: usize) -> Vec<String> {
    let mut head = Vec::new();
    let mut here = at;
    while here > 0 {
        let previous = tokens[here - 1];
        let gap = between(
            index,
            masks,
            previous.span().end(),
            tokens[here].span().start(),
        );
        if gap.trim() != "::" {
            break;
        }
        head.push(index.text(previous.name_span()).to_owned());
        here -= 1;
    }
    head.reverse();
    head
}

/// Whether a path head leads to the crate's own `runtime`: nothing, or `crate`, `self` and
/// `super` alone.
fn our_head(head: &[String]) -> bool {
    head.iter()
        .all(|segment| matches!(segment.as_str(), "crate" | "self" | "super"))
}

/// Whether the statement or match arm holding token `at`, inside the body of the innermost item,
/// carries its own `#[cfg(…)]` — a gate no grain of the product test reads.
fn under_a_statement_gate(
    index: &Index,
    masks: &[Span],
    tokens: &[TokenRecord],
    at: usize,
) -> bool {
    let offset = tokens[at].span().start();
    let Some(body) = index
        .items()
        .iter()
        .filter(|item| item.whole().holds(offset))
        .min_by_key(|item| item.whole().len())
        .and_then(|item| item.body())
    else {
        return false;
    };
    let mut here = at;
    while here > 0 && tokens[here - 1].span().start() >= body.start() {
        let previous = tokens[here - 1];
        let gap = between(
            index,
            masks,
            previous.span().end(),
            tokens[here].span().start(),
        );
        if gap.contains([';', '{', '}', ',']) {
            return false;
        }
        if index.text(previous.name_span()) == "cfg" && here >= 2 {
            let before = between(
                index,
                masks,
                tokens[here - 2].span().end(),
                previous.span().start(),
            );
            if before.trim_end().ends_with("#[") {
                return true;
            }
        }
        here -= 1;
    }
    false
}

/// The module path a `use` path written in `runtime/mod.rs` names: from the crate with `crate`,
/// from `runtime` with `self`, a bare name, or one `super` per step out.
fn from_runtime(path: &[String]) -> String {
    let mut resolved = vec!["crate".to_owned(), "runtime".to_owned()];
    let mut rest = path;
    match path.first().map(String::as_str) {
        Some("crate") => {
            resolved.truncate(1);
            rest = &path[1..];
        }
        Some("self") => rest = &path[1..],
        _ => {}
    }
    while rest.first().is_some_and(|segment| segment == "super") {
        resolved.pop();
        rest = &rest[1..];
    }
    resolved.extend(rest.iter().cloned());
    resolved.join("::")
}

/// Whether `span` stands in a module-level `use` item.
fn in_a_use(index: &Index, span: Span) -> bool {
    index
        .uses()
        .iter()
        .any(|record| span.within(record.whole()))
}

/// **The reading of one package.**
fn read(index: &Index, gate: UseGate) -> Reading {
    let topics = topics(index);
    let modules: BTreeSet<String> = index
        .modules()
        .iter()
        .flat_map(|module| module.module_paths())
        .map(ToString::to_string)
        .collect();
    let masks = masks(index);
    let tokens = index.tokens();
    let mut reading = Reading::default();
    let search = |pattern: Pattern| {
        index
            .search(&Search::new(needle!(pattern), View::Identifiers))
            .unwrap_or_else(|failure| panic!("{failure}"))
            .in_the_product(index)
    };
    // G1 and G4: paths written outside a module-level `use`.
    for topic in &topics {
        for occurrence in search(Pattern::path(&format!("runtime::{topic}"))).occurrences() {
            let start = occurrence.span.start();
            if !outside_runtime(index, start) || in_a_use(index, occurrence.span) {
                continue;
            }
            let at = token_at(tokens, start);
            let head = head_before(index, &masks, tokens, at);
            if !our_head(&head) {
                reading.refused.push(format!(
                    "{}: `{}::runtime::{topic}` — a path whose head is not the crate's own \
                     `runtime`, which the guard does not resolve; spell the crate's path from \
                     `crate`, `self` or `super`",
                    where_is(index, start),
                    head.join("::")
                ));
                continue;
            }
            if under_a_statement_gate(index, &masks, tokens, at) {
                reading.refused.push(format!(
                    "{}: `runtime::{topic}` under a statement's own `#[cfg]`, a gate the guard \
                     does not read; gate the item, or spell the path through `runtime/mod.rs`",
                    where_is(index, start)
                ));
                continue;
            }
            let certainty = match occurrence.certainty {
                Certainty::Resolved => "",
                Certainty::Candidate => " (in a macro's arguments)",
            };
            reading.findings.push(format!(
                "{}: `{}runtime::{topic}`{certainty} — a path into a runtime topic module; \
                 re-export the item from `runtime/mod.rs` and reach it as `runtime::<item>`",
                where_is(index, start),
                head.iter()
                    .map(|segment| format!("{segment}::"))
                    .collect::<String>()
            ));
        }
    }
    // G3: `runtime::{…}`, `runtime::*` and `runtime as …` outside a module-level `use`, under
    // any head (a foreign one is refused here as G1 refuses it), and a comment inside a path into
    // a topic, which G1's search compares byte for byte and so does not match.
    for occurrence in search(Pattern::identifier("runtime")).occurrences() {
        let start = occurrence.span.start();
        if !outside_runtime(index, start) || in_a_use(index, occurrence.span) {
            continue;
        }
        let at = token_at(tokens, start);
        let head = head_before(index, &masks, tokens, at)
            .iter()
            .map(|segment| format!("{segment}::"))
            .collect::<String>();
        let next = tokens
            .get(at + 1)
            .filter(|next| index.file_at(next.span().start()) == index.file_at(start));
        let (after, raw) = next.map_or_else(Default::default, |next| {
            (
                between(index, &masks, occurrence.span.end(), next.span().start()),
                between(index, &[], occurrence.span.end(), next.span().start()),
            )
        });
        let joined = after.trim_start().strip_prefix("::").map(str::trim_start);
        let shape = match joined {
            Some(rest) if rest.starts_with('{') => Some("::{…}"),
            Some(rest) if rest.starts_with('*') => Some("::*"),
            _ if after.trim().is_empty()
                && next.is_some_and(|next| index.text(next.name_span()) == "as") =>
            {
                Some(" as …")
            }
            _ => None,
        };
        if let Some(shape) = shape {
            reading.refused.push(format!(
                "{}: `{head}runtime{shape}` in a body — `runtime` opened by a group, a glob or a \
                 rename, and a path written through it is one the guard cannot see; spell each \
                 path whole from `crate`, `self` or `super`",
                where_is(index, start)
            ));
            continue;
        }
        let into_a_topic = next.filter(|next| topics.contains(index.text(next.name_span())));
        if let Some(topic) = into_a_topic
            && joined.is_some_and(str::is_empty)
            && raw.trim() != "::"
        {
            reading.refused.push(format!(
                "{}: `{head}runtime::{}` with a comment inside the path, which the guard's path \
                 search does not read; write the path without it",
                where_is(index, start),
                index.text(topic.name_span())
            ));
        }
    }
    // G2: module-level `use` items.
    for record in index.uses() {
        let product = Occurrence {
            span: record.whole(),
            certainty: Certainty::Resolved,
        }
        .in_the_product(index)
            && (gate == UseGate::Blind || record.variant().permits_product());
        if !product {
            continue;
        }
        let place = where_is(index, record.whole().start());
        // F4: `runtime/mod.rs`'s own `use` items are the road; one that re-exports a module, or
        // a glob (which re-exports the modules it reaches), opens a module under a new name.
        if record.module_paths().contains(&RUNTIME) {
            for (name, path) in record.names() {
                let target = from_runtime(path);
                if modules.contains(&target) {
                    reading.refused.push(format!(
                        "{place}: `use {}` (as `{name}`) re-exports the module `{target}` — a \
                         path written through `runtime::{name}::` is one into a module the guard \
                         does not count as a topic; re-export the items, not the module",
                        path.join("::")
                    ));
                }
            }
            for path in record.globs() {
                reading.refused.push(format!(
                    "{place}: `use {}::*` in `runtime/mod.rs` — a glob re-exports whatever it \
                     reaches, modules included, which the guard does not resolve; re-export the \
                     items by name",
                    path.join("::")
                ));
            }
            continue;
        }
        if record
            .module_paths()
            .iter()
            .all(|module| inside_runtime(module))
        {
            continue;
        }
        let past_head = |path: &[String]| -> Option<Vec<String>> {
            let k = path
                .iter()
                .position(|segment| !matches!(segment.as_str(), "crate" | "self" | "super"))?;
            (path[k] == "runtime").then(|| path[k + 1..].to_vec())
        };
        // F1: a `runtime` segment behind a head the guard does not resolve, followed by a topic
        // (or ending the path, for a glob or a rename): refused, as G1 refuses the same head.
        let foreign = |path: &[String], ending: bool| -> bool {
            path.iter().enumerate().any(|(k, segment)| {
                segment == "runtime"
                    && !our_head(&path[..k])
                    && path.get(k + 1).map_or(ending, |next| topics.contains(next))
            })
        };
        let refuse_foreign = |shown: String| {
            format!(
                "{place}: `use {shown}` — a path into `runtime` behind a head the guard does not \
                 resolve; spell the crate's path from `crate`, `self` or `super`"
            )
        };
        for (name, path) in record.names() {
            let Some(rest) = past_head(path) else {
                let renames_runtime =
                    path.last().is_some_and(|last| last == "runtime") && name != "runtime";
                if foreign(path, renames_runtime) {
                    reading.refused.push(refuse_foreign(path.join("::")));
                }
                continue;
            };
            match rest.first() {
                None if name != "runtime" => reading.refused.push(format!(
                    "{place}: `use … runtime as {name}` — a path written through `{name}::` is \
                     one the guard cannot see; spell `runtime::<item>`"
                )),
                Some(first) if topics.contains(first) => reading.findings.push(format!(
                    "{place}: `use {}` — a path into a runtime topic module; re-export the item \
                     from `runtime/mod.rs` and reach it as `runtime::<item>`",
                    path.join("::")
                )),
                _ => {}
            }
        }
        for path in record.globs() {
            let Some(rest) = past_head(path) else {
                if foreign(path, true) {
                    reading
                        .refused
                        .push(refuse_foreign(format!("{}::*", path.join("::"))));
                }
                continue;
            };
            match rest.first() {
                None => reading.refused.push(format!(
                    "{place}: `use {}::*` — a glob over runtime's names brings the topic modules \
                     in bare, and a path written through one is one the guard cannot see",
                    path.join("::")
                )),
                Some(first) if topics.contains(first) => reading.findings.push(format!(
                    "{place}: `use {}::*` — a glob into a runtime topic module; re-export the \
                     items from `runtime/mod.rs`",
                    path.join("::")
                )),
                _ => {}
            }
        }
    }
    reading.findings.sort();
    reading.refused.sort();
    reading
}

/// The fixture crate.
fn fixture() -> Index {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("root_topic_paths");
    let universe = Universe::declare(
        "the root-topic-paths fixture",
        vec![TargetRoot {
            id: TargetId {
                package: "root-topic-paths".to_owned(),
                kind: TargetKind::Library,
                name: "root_topic_paths".to_owned(),
            },
            file: directory.join("lib.rs"),
        }],
        vec![DiskScope::under(&directory)],
        Vendor::Excluded,
    )
    .expect("the fixture is there");
    Index::build(&universe).unwrap_or_else(|rejections| panic!("{}", report(&rejections)))
}

fn named(lines: &[String], needle: &str) -> usize {
    lines.iter().filter(|line| line.contains(needle)).count()
}

/// RED (planted, T1–T12, the review's F1–F4) — **every path into a topic module written outside
/// `runtime/` in a product build is a finding, each shape the guard cannot read is refused by its
/// line, and nothing else is named.**
///
/// The fixture plants a module-level `use` (T1), a full path with no `use` (T2, and the same into
/// a private topic), one arm of a group (T5), a field type, an `impl` header and UFCS in another
/// module (T6), a path among a macro's arguments (T9), a glob into a topic and a nested module's
/// `use super::runtime::…` (F2); it plants a rename of `runtime` and a glob over it (T7), a path
/// under a statement's own gate (T8), a group, a glob and a rename in a body (F2), a foreign head
/// in a module-level `use`, a body group and a path (F1), a comment inside a path (F3) and a
/// module re-exported by `runtime/mod.rs` (F4), each refused. A test-gated `use` (T3), the
/// re-export road as a `use` and as a path (T4, and `runtime/mod.rs`'s item re-export), a doc
/// link and a string (T10), a sibling's path inside `runtime/` (T11), a file reached by
/// `#[cfg(test)] mod t;` (T12), a field gated to tests and a test module pass.
///
/// Every row is checked before the test fails, so a red run names each plant it misses.
#[test]
fn every_path_into_a_runtime_topic_from_outside_is_named() {
    let index = fixture();
    let reading = read(&index, UseGate::Read);
    let findings: &[(&str, usize, &str)] = &[
        (
            "lib.rs:23:1: `use runtime::clipboard::copy`",
            1,
            "T1, the `use`",
        ),
        ("lib.rs:42:17: `runtime::clipboard`", 1, "T2, the full path"),
        (
            "`use runtime::pointer::Press`",
            1,
            "T5, the group's arm into a topic",
        ),
        (
            "other.rs:",
            3,
            "T6, the field type, the `impl` header and UFCS",
        ),
        (
            "`super::runtime::pointer`",
            1,
            "T6's `impl` header, its head read",
        ),
        ("(in a macro's arguments)", 1, "T9"),
        ("`runtime::quiet`", 1, "a private topic is a topic"),
        (
            "`use runtime::pointer::*`",
            1,
            "F2, a module-level glob into a topic",
        ),
        (
            "`use super::runtime::pointer::Press`",
            1,
            "F2, a nested module's `use`",
        ),
    ];
    let refused: &[(&str, usize, &str)] = &[
        ("`use … runtime as rt`", 1, "T7, the rename"),
        ("`use crate::runtime::*`", 1, "T7, the glob"),
        ("under a statement's own `#[cfg]`", 1, "T8"),
        ("`runtime::{…}` in a body", 1, "F2, a group in a body"),
        ("`runtime::*` in a body", 1, "F2, a glob in a body"),
        (
            "`crate::runtime as …` in a body",
            1,
            "F2, a rename in a body",
        ),
        (
            "`use c::runtime::pointer::Press`",
            1,
            "F1, a foreign head in a module-level `use`",
        ),
        (
            "`c::runtime::{…}` in a body",
            1,
            "F1, a foreign head in a body group",
        ),
        (
            "`c::runtime::pointer` — a path whose head",
            1,
            "F1, a foreign head in a path (G1)",
        ),
        (
            "`runtime::pointer` with a comment inside",
            1,
            "F3, a comment inside a path",
        ),
        (
            "re-exports the module `crate::runtime::pointer::inner`",
            1,
            "F4, a module re-export",
        ),
    ];
    let mut missed = Vec::new();
    for (kind, lines, rows) in [
        ("finding", &reading.findings, findings),
        ("refusal", &reading.refused, refused),
    ] {
        for (needle, expected, what) in rows {
            let got = named(lines, needle);
            if got != *expected {
                missed.push(format!(
                    "{what}: {got} {kind}(s) naming {needle:?}, not {expected}"
                ));
            }
        }
    }
    for (kind, lines, expected) in [
        ("finding", &reading.findings, 10),
        ("refusal", &reading.refused, 11),
    ] {
        if lines.len() != expected {
            missed.push(format!("{} {kind}s, not {expected}", lines.len()));
        }
    }
    if reading.findings.iter().chain(&reading.refused).any(|line| {
        line.contains("t.rs:")
            || line.contains("clipboard.rs:")
            || line.contains("pointer.rs:")
            || line.contains("paste")
            || line.contains("exported")
    }) {
        missed.push("T3, T4, T11 and T12 are named, and pass".to_owned());
    }
    assert!(missed.is_empty(), "{}\n{reading:#?}", missed.join("\n"));
}

/// RED (planted, T3) — **the test-gated `use` is out of the product only because the guard reads
/// the `use`'s own gate.** Read the way [`Occurrence::in_the_product`] alone reads it (plan row
/// 77: a `use` stands in no item), the root's `#[cfg(test)] use runtime::clipboard::paste;` is a
/// finding — the false one this guard does not inherit.
#[test]
fn a_test_gated_use_is_out_of_the_product_by_its_own_gate() {
    let index = fixture();
    let blind = read(&index, UseGate::Blind);
    assert_eq!(
        named(&blind.findings, "`use runtime::clipboard::paste`"),
        1,
        "the variant-blind reading counts the gated `use`: {:#?}",
        blind.findings
    );
    let read_ = read(&index, UseGate::Read);
    assert_eq!(
        named(&read_.findings, "paste"),
        0,
        "the guard's reading does not: {:#?}",
        read_.findings
    );
}

/// GUARD (T13) — **`bt-app` reaches its runtime topics from outside `runtime/` only through
/// `runtime/mod.rs`'s re-exports**: no finding, and no shape refused.
///
/// MUTATION: write `runtime::pointer::ButtonSet` again in `App`'s field — a finding naming the
/// line; write `#[cfg(test)] use runtime::pointer::ButtonSet;` at the root — no finding.
#[test]
fn bt_app_reaches_runtime_topics_only_through_the_re_exports() {
    let index = Index::of_package("bt-app");
    let reading = read(index, UseGate::Read);
    assert!(
        reading.findings.is_empty() && reading.refused.is_empty(),
        "{}",
        reading
            .findings
            .iter()
            .chain(&reading.refused)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
