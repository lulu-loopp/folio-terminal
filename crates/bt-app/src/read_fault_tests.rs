//! `persist::read_fault`'s own tests (0.4.10 plan row 68d, review N1, N6): which refusal the
//! start says, and which it leaves to the carry pass's refused copies.

use std::path::PathBuf;

use bt_persist::{FallbackReason, ReadReport};

use super::{newer_refusal_to_note, read_fault_noting};
use crate::rejected_copies::KeptDocument;

/// A newer `settings.json`: the version above this build's.
fn newer() -> FallbackReason {
    FallbackReason::FutureSchemaVersion {
        found: bt_persist::SETTINGS_SCHEMA_VERSION + 1,
        current: bt_persist::SETTINGS_SCHEMA_VERSION,
    }
}

const COPY: &str = "settings.json.rejected-20261115-010203";

/// RED (0.4.10 plan row 68d, R4; review N1, N6) — **only a newer file kept as a copy, of a live
/// document, in the writer, is left to the refused copies**: a damaged file kept as a copy, a
/// newer file with no copy, update-check.json, and a process that is not the writer are each
/// said by the start.
///
/// MUTATION: any refusal that kept a copy is noted, whatever its reason
/// (`plant-68d-any-refusal-noted.py`); a process that is not the writer notes it too
/// (`plant-68d-noted-outside-the-writer.py`).
#[test]
fn only_a_newer_file_kept_in_the_writer_is_left_to_the_copies() {
    assert_eq!(
        newer_refusal_to_note(&newer(), Some(COPY), "settings.json", true),
        Some((KeptDocument::Settings, COPY.to_owned())),
        "a newer settings.json kept as a copy, in the writer"
    );
    for (reason, said) in [
        (
            FallbackReason::ParseError("expected value".to_owned()),
            "damaged",
        ),
        (FallbackReason::NoMigrationPath { found: 0 }, "no migration"),
        (
            FallbackReason::TooLarge {
                bytes: u64::MAX,
                cap: 1,
            },
            "too large",
        ),
    ] {
        assert_eq!(
            newer_refusal_to_note(&reason, Some(COPY), "settings.json", true),
            None,
            "a {said} file kept as a copy is said by the start"
        );
    }
    assert_eq!(
        newer_refusal_to_note(&newer(), None, "settings.json", true),
        None,
        "a newer file with no copy kept is said by the start"
    );
    assert_eq!(
        newer_refusal_to_note(&newer(), Some(COPY), "update-check.json", true),
        None,
        "update-check.json is said by the start"
    );
    assert_eq!(
        newer_refusal_to_note(&newer(), Some(COPY), "settings.json", false),
        None,
        "a process that is not the writer says it at start"
    );
}

/// RED (row 68d, R4; review N1) — **the start's read of a newer file kept as a copy raises no
/// sentence and notes the copy for the refused copies**, by its name; a damaged file kept as a
/// copy raises the sentence naming the copy, and notes nothing.
///
/// MUTATION: the note is dropped and the sentence still is (`plant-68d-refusal-noted-nowhere.py`).
#[test]
fn a_newer_file_kept_as_a_copy_is_noted_and_not_said_at_start() {
    let kept = PathBuf::from("C:/data").join(COPY);
    let mut noted = Vec::new();
    let said = read_fault_noting(
        &ReadReport::FellBackToDefaults {
            reason: newer(),
            kept: Some(kept.clone()),
        },
        "settings.json",
        true,
        |file| format!("{file} could not be read"),
        |document, copy| noted.push((document, copy)),
    );
    assert_eq!(said, None, "the start says nothing");
    assert_eq!(noted, [(KeptDocument::Settings, COPY.to_owned())]);

    let mut noted = Vec::new();
    let said = read_fault_noting(
        &ReadReport::FellBackToDefaults {
            reason: FallbackReason::ParseError("expected value".to_owned()),
            kept: Some(kept),
        },
        "settings.json",
        true,
        |file| format!("{file} could not be read"),
        |document, copy| noted.push((document, copy)),
    );
    assert!(
        said.as_deref().is_some_and(|said| said.contains(COPY)),
        "a damaged file is said, naming its copy: {said:?}"
    );
    assert!(noted.is_empty(), "and noted nowhere");
}
