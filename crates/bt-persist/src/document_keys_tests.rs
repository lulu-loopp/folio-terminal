//! **The persisted documents keep their keys at their versions** (0.4.10 plan row 68d, ruling R9
//! of T-DOWNGRADE-SAYS-SO-AND-OFFERS-THE-COPY-BACK).
//!
//! A build reads a document at its own version and refuses one written at a higher version, so a
//! key added without a new version is handed to every build of that version unknowingly: a
//! dropped key comes back as its default the next time the older build writes, and a record that
//! refuses unknown keys refuses the file. `docs/plans/DOCUMENT-KEYS.tsv` pins, for every document
//! and version, the shape of every type a document's root reaches; this module holds the six
//! documents this crate reads to it, and `scripts/ci/check-document-keys.ps1` holds the register
//! to the merge base's, so a row is never edited in place.
//!
//! **A shape** is the type's declaration and its body as `bt_source` reads them, squeezed: every
//! `//` comment and all whitespace taken out, the attributes, the field and variant names and the
//! types kept. `bt-app`'s `the_integration_records_keep_their_keys` reads its three records by the
//! same rule.

use std::collections::{BTreeMap, BTreeSet};

use bt_source::call_graph::{Arms, Resolver};
use bt_source::{Index, ItemQuery};

use crate::{
    KEYBINDINGS_SCHEMA_VERSION, PINS_SCHEMA_VERSION, PROFILES_SCHEMA_VERSION,
    SESSION_SCHEMA_VERSION, SETTINGS_SCHEMA_VERSION, UPDATE_CHECK_SCHEMA_VERSION,
};

/// The register, as committed.
const DOCUMENT_KEYS_REGISTER: &str = include_str!("../../../docs/plans/DOCUMENT-KEYS.tsv");

/// The register's column header.
const DOCUMENT_KEYS_HEADER: &str = "document\tversion\ttype\tshape\treason";

/// The documents of the register that `bt-app` reads and holds
/// (`profile_marks::tests::the_integration_records_keep_their_keys`).
const RECORDS_OF_BT_APP: [&str; 3] = [
    "integration-marks",
    "integration-profile-files",
    "update-homes",
];

/// One document this crate reads through the fallback chain: its name in the register, its root
/// type, and the version constant its reads pass, by name and by value.
struct PinnedDocument {
    name: &'static str,
    root: &'static str,
    constant: &'static str,
    version: u32,
}

const PINNED_DOCUMENTS: [PinnedDocument; 6] = [
    PinnedDocument {
        name: "session",
        root: "SessionV1",
        constant: "SESSION_SCHEMA_VERSION",
        version: SESSION_SCHEMA_VERSION,
    },
    PinnedDocument {
        name: "settings",
        root: "SettingsV1",
        constant: "SETTINGS_SCHEMA_VERSION",
        version: SETTINGS_SCHEMA_VERSION,
    },
    PinnedDocument {
        name: "keybindings",
        root: "KeybindingsV1",
        constant: "KEYBINDINGS_SCHEMA_VERSION",
        version: KEYBINDINGS_SCHEMA_VERSION,
    },
    PinnedDocument {
        name: "profiles",
        root: "ProfilesV1",
        constant: "PROFILES_SCHEMA_VERSION",
        version: PROFILES_SCHEMA_VERSION,
    },
    PinnedDocument {
        name: "pins",
        root: "PinsV1",
        constant: "PINS_SCHEMA_VERSION",
        version: PINS_SCHEMA_VERSION,
    },
    PinnedDocument {
        name: "update-check",
        root: "UpdateCheckV1",
        constant: "UPDATE_CHECK_SCHEMA_VERSION",
        version: UPDATE_CHECK_SCHEMA_VERSION,
    },
];

/// One row of the register.
struct PinnedKeyRow<'r> {
    document: &'r str,
    version: u32,
    type_name: &'r str,
    shape: &'r str,
}

/// The register's rows, each read whole: five non-empty columns under the header, a dated reason,
/// and no (document, version, type) named twice.
fn key_register_rows() -> Vec<PinnedKeyRow<'static>> {
    let mut rows = Vec::new();
    let mut header = false;
    let mut keys = BTreeSet::new();
    for line in DOCUMENT_KEYS_REGISTER.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !header {
            assert_eq!(line, DOCUMENT_KEYS_HEADER, "the register's column header");
            header = true;
            continue;
        }
        let columns: Vec<&str> = line.split('\t').collect();
        let [document, version, type_name, shape, reason] = columns[..] else {
            panic!("a register row is five columns: {line}");
        };
        assert!(
            columns.iter().all(|column| !column.is_empty()),
            "a register row has no empty column: {line}"
        );
        assert!(
            pin_reason_is_dated(reason),
            "a register row's reason is dated (YYYY-MM-DD: why): {line}"
        );
        let version: u32 = version
            .parse()
            .unwrap_or_else(|_| panic!("a register row's version is a number: {line}"));
        assert!(
            keys.insert((document, version, type_name)),
            "the register names {document} v{version} {type_name} twice"
        );
        rows.push(PinnedKeyRow {
            document,
            version,
            type_name,
            shape,
        });
    }
    assert!(header, "the register has its column header");
    rows
}

/// Whether `reason` reads `YYYY-MM-DD: why`.
fn pin_reason_is_dated(reason: &str) -> bool {
    let bytes = reason.as_bytes();
    bytes.len() > 12
        && bytes[..10].iter().enumerate().all(|(at, byte)| match at {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        })
        && reason[10..].starts_with(": ")
}

/// One item's text with its `//` comments and its whitespace taken out (the register's rule).
fn squeezed_shape(text: &str) -> String {
    text.lines()
        .map(|line| match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<String>()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

/// This crate, indexed once per process.
fn persist_source() -> &'static Index {
    Index::of_package("bt-persist")
}

/// **Every type `root` reaches in this crate, by name, with its shape** — the type, then the
/// types its fields and its variants' payloads name, walked to a fixed point
/// (`Resolver::data_types_reached`).
fn shapes_reached(resolver: &Resolver<'_>, root: &str) -> BTreeMap<String, String> {
    let index = persist_source();
    let record = index
        .one(&ItemQuery::type_item(root))
        .unwrap_or_else(|failure| panic!("{failure}"));
    let id = resolver
        .id_of("bt-persist", record)
        .expect("the root is an item of bt-persist");
    let reached = resolver.data_types_reached(id).unwrap_or_else(|unread| {
        panic!("{root} reaches a type the resolver cannot read: {unread:?}")
    });
    let mut shapes = BTreeMap::new();
    for id in reached {
        let record = resolver.record(id);
        let body = record.body().map_or("", |body| index.text(body));
        let shape = squeezed_shape(index.text(record.declaration())) + &squeezed_shape(body);
        if let Some(other) = shapes.insert(record.name().to_owned(), shape.clone()) {
            assert_eq!(
                other,
                shape,
                "{root} reaches two types named {}, and a register row names a type by its name",
                record.name()
            );
        }
    }
    shapes
}

/// RED (0.4.10 plan row 68d, D9) — **every document keeps its keys at its version**: for each of
/// the six documents this crate reads, the types its root reaches are exactly the register's rows
/// at its current version, each shape equal to the source's, and the register names no version
/// above it. A key added, an optional key added, a type newly reached: red until the version is
/// bumped, its migration step added and the new version's rows written.
///
/// MUTATION: a required key added to `SettingsV1` (`plant-68d-key-added.py`); a
/// `skip_serializing_if` key added (`plant-68d-optional-key-added.py`).
#[test]
fn every_document_keeps_its_keys_at_its_version() {
    let rows = key_register_rows();
    let index = persist_source();
    let resolver = Resolver::new(&[("bt-persist", index)], Arms::product())
        .unwrap_or_else(|refusals| panic!("{refusals:?}"));
    let mut wrong = Vec::new();
    for document in &PINNED_DOCUMENTS {
        let highest = rows
            .iter()
            .filter(|row| row.document == document.name)
            .map(|row| row.version)
            .max();
        if highest != Some(document.version) {
            wrong.push(format!(
                "{}: the register's highest version is {highest:?}, and this build writes {}",
                document.name, document.version
            ));
        }
        let pinned: BTreeMap<&str, &str> = rows
            .iter()
            .filter(|row| row.document == document.name && row.version == document.version)
            .map(|row| (row.type_name, row.shape))
            .collect();
        let source = shapes_reached(&resolver, document.root);
        for (type_name, shape) in &source {
            match pinned.get(type_name.as_str()) {
                Some(row) if row == shape => {}
                Some(row) => wrong.push(format!(
                    "{} v{}: {type_name} is not the shape its row pins\n  row: {row}\n  \
                   source: {shape}",
                    document.name, document.version
                )),
                None => wrong.push(format!(
                    "{} v{}: {type_name} has no row; the source's row would be\n  \
                   {}\t{}\t{type_name}\t{shape}\t<date>: <ticket>; <why>",
                    document.name, document.version, document.name, document.version
                )),
            }
        }
        for type_name in pinned.keys() {
            if !source.contains_key(*type_name) {
                wrong.push(format!(
                    "{} v{}: the row of {type_name} names a type the root no longer reaches",
                    document.name, document.version
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "a document's keys changed without a new version; bump the version, add its migration \
       step and add the new version's rows to docs/plans/DOCUMENT-KEYS.tsv:\n{}",
        wrong.join("\n")
    );
}

/// The version constants `body` passes to the fallback chain's three entries, each the second
/// argument of a call of `read_with_fallback`, `read_with_fallback_bytes` or `parse_document`.
fn versions_passed_to_the_chain(body: &str, found: &mut BTreeSet<String>) {
    let text = squeezed_shape(body);
    for entry in [
        "read_with_fallback",
        "read_with_fallback_bytes",
        "parse_document",
    ] {
        let mut from = 0;
        while let Some(at) = text[from..].find(entry) {
            let start = from + at;
            let end = start + entry.len();
            from = end;
            let identifier = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
            if start > 0 && identifier(text.as_bytes()[start - 1]) {
                continue;
            }
            let mut rest = &text[end..];
            if rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            if let Some(turbofish) = rest.strip_prefix("::<") {
                let mut depth = 1;
                let close = turbofish
                    .char_indices()
                    .find(|&(_, c)| {
                        depth += match c {
                            '<' => 1,
                            '>' => -1,
                            _ => 0,
                        };
                        depth == 0
                    })
                    .map(|(close, _)| close)
                    .expect("a turbofish closes");
                rest = &turbofish[close + 1..];
            }
            let Some(arguments) = rest.strip_prefix('(') else {
                continue;
            };
            let mut depth = 0;
            let mut argument = 0;
            let mut constant = String::new();
            for c in arguments.chars() {
                match c {
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' if depth == 0 => break,
                    ')' | ']' | '}' => depth -= 1,
                    ',' if depth == 0 => argument += 1,
                    _ if argument == 1 && depth == 0 => constant.push(c),
                    _ => {}
                }
            }
            assert!(
                !constant.is_empty()
                    && constant
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
                "a call of `{entry}` passes its version as a named constant, not `{constant}`"
            );
            found.insert(constant);
        }
    }
}

/// RED (0.4.10 plan row 68d, D10) — **every versioned document has its register rows**: the
/// version constants `lib.rs` passes to `read_with_fallback`, `read_with_fallback_bytes` and
/// `parse_document` are exactly the six documents' of this test, and the register names no
/// document but those six and `bt-app`'s three records. A seventh document cannot land unpinned.
///
/// MUTATION: a seventh `read_with_fallback` call with a version constant of its own
/// (`plant-68d-document-without-rows.py`).
#[test]
fn every_versioned_document_has_its_register_rows() {
    let index = persist_source();
    let mut passed = BTreeSet::new();
    for record in index.items() {
        if record.kind().is_callable()
            && record.module_paths().contains(&"crate")
            && let Some(body) = record.body()
        {
            versions_passed_to_the_chain(index.text(body), &mut passed);
        }
    }
    let documents: BTreeSet<String> = PINNED_DOCUMENTS
        .iter()
        .map(|document| document.constant.to_owned())
        .collect();
    assert_eq!(
        passed, documents,
        "the documents lib.rs reads through the fallback chain are the six this test pins; a new \
       one needs its rows in docs/plans/DOCUMENT-KEYS.tsv and its entry in PINNED_DOCUMENTS"
    );
    let named: BTreeSet<&str> = key_register_rows().iter().map(|row| row.document).collect();
    let expected: BTreeSet<&str> = PINNED_DOCUMENTS
        .iter()
        .map(|document| document.name)
        .chain(RECORDS_OF_BT_APP)
        .collect();
    assert_eq!(
        named, expected,
        "the register's documents are this crate's six and bt-app's three"
    );
}
