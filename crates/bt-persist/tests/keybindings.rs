//! `keybindings.json` against the public API — the file the shortcut table's
//! departures live in (user ruling Q7 = B, 2026-08-17).
//!
//! Three properties and one non-property. The round trip, because the file is
//! the thing that outlives the process; the empty file, because "no departures"
//! is the ordinary state of nearly every machine and must not be an error; the
//! `null`, because "unbound" and "absent" are different sentences and only one
//! of them survives a change of default. And the non-property: a damaged file is
//! read as *no overrides* and **left on disk**, because it is the user's own
//! words and a build that rewrote what it could not parse would destroy the one
//! copy they could have fixed by hand.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use bt_persist::{
    BindingOverrideV1, FallbackReason, KEYBINDINGS_SCHEMA_VERSION, KeybindingsV1, ReadReport,
    read_keybindings, write_keybindings_atomic,
};

fn unique_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bt-persist-keybindings-{tag}-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// §5.4 case 1 — most machines never customise a chord, and that is not a
/// failure to report.
#[test]
fn a_machine_that_has_never_customised_a_chord_reports_nothing_at_all() {
    let dir = unique_dir("missing");
    let (file, report) = read_keybindings(&dir.join("keybindings.json"));
    assert_eq!(report, ReadReport::NotFound);
    assert!(file.bindings.is_empty());
    assert_eq!(file.schema_version, KEYBINDINGS_SCHEMA_VERSION);
}

/// PIN — what was written is what comes back, `null` included.
#[test]
fn the_departures_survive_a_round_trip_through_the_disk() {
    let dir = unique_dir("round-trip");
    let path = dir.join("keybindings.json");
    let written = KeybindingsV1 {
        schema_version: KEYBINDINGS_SCHEMA_VERSION,
        bindings: vec![
            BindingOverrideV1 {
                action: "new-tab".to_owned(),
                chord: Some("Ctrl+Shift+Y".to_owned()),
            },
            BindingOverrideV1 {
                action: "open-search-alias".to_owned(),
                chord: None,
            },
        ],
    };
    write_keybindings_atomic(&path, &written).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains('\n'),
        "the one file this product invites a person to edit is written for a person"
    );
    assert!(
        text.contains("\"chord\": null"),
        "an explicitly cleared row keeps its key: {text}"
    );

    let (read, report) = read_keybindings(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(read, written);
}

/// PIN — **only the departures are in the file**, which is what lets a later
/// build retune a chord for everyone who never touched it.
///
/// A file holding every row would freeze today's defaults into every user's disk
/// for ever; the reader would then honour them and the retune would reach
/// nobody. The rule lives in `bt-app`'s `Shortcuts::overrides`, and this is the
/// half of it that is about the file: an empty list is a legal, ordinary
/// document.
#[test]
fn a_table_that_departs_nowhere_writes_an_empty_list_and_reads_back_as_one() {
    let dir = unique_dir("empty");
    let path = dir.join("keybindings.json");
    write_keybindings_atomic(&path, &KeybindingsV1::default()).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"bindings\": []"), "{text}");

    let (read, report) = read_keybindings(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert!(read.bindings.is_empty());
}

/// §5.4 case 2 — a damaged file is reported, read as nothing, and **left where
/// it is**.
#[test]
fn a_damaged_file_falls_back_to_no_overrides_and_is_left_on_disk() {
    let dir = unique_dir("corrupt");
    let path = dir.join("keybindings.json");
    let original = "{ this is not json";
    std::fs::write(&path, original).unwrap();

    let (read, report) = read_keybindings(&path);
    assert!(read.bindings.is_empty(), "the defaults are in force");
    assert!(
        matches!(
            report,
            ReadReport::FellBackToDefaults {
                reason: FallbackReason::ParseError(_)
            }
        ),
        "{report:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        original,
        "the read path never touches the file it could not use"
    );
}

/// §1.3 rule 2 — a file from a newer build is not partially parsed.
#[test]
fn a_file_from_a_future_build_is_refused_whole_rather_than_read_in_part() {
    let dir = unique_dir("future");
    let path = dir.join("keybindings.json");
    std::fs::write(
        &path,
        r#"{ "schema_version": 99, "bindings": [ { "action": "new-tab", "chord": "Ctrl+Shift+Y" } ] }"#,
    )
    .unwrap();

    let (read, report) = read_keybindings(&path);
    assert!(read.bindings.is_empty());
    assert!(
        matches!(
            report,
            ReadReport::FellBackToDefaults {
                reason: FallbackReason::FutureSchemaVersion { found: 99, .. }
            }
        ),
        "{report:?}"
    );
}
