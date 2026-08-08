//! Fixture round-trip gate: a v1 `session.json` with non-default values in
//! every field, differently key-ordered/whitespaced and carrying unknown
//! fields at every nesting level, must
//!   1. parse cleanly (no fallback, no degradation) via the public API,
//!   2. drop every unknown field (§1.3 ruling 4A), and
//!   3. re-serialize to *exactly* the checked-in canonical bytes — proving
//!      the on-disk form is deterministic ("bit-stable") and that field
//!      order is canonicalized to the struct's declaration order regardless
//!      of what order the source file used.
//!
//! Only default-valued fixtures would miss the field-order/drop bugs this
//! test exists to catch (`CONVENTIONS.md` §三 "默认值会掩盖 bug") — every
//! field below carries a non-default, distinguishable value.

use std::path::PathBuf;

use bt_persist::{
    DegradationReport, LayoutNodeV1, LeafNodeV1, ReadReport, RecentSeedV1, SessionCursorStyleV1,
    SessionThemeV1, SessionV1, TabV1, TermLeafV1, read_session, write_session_atomic,
};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn canonical_bytes() -> Vec<u8> {
    let mut bytes = std::fs::read(fixture_path("session_v1_nondefault_canonical.json"))
        .expect("canonical fixture must exist");
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    bytes
}

#[test]
fn messy_input_parses_clean_and_matches_canonical_struct() {
    let (session, report, degradation) =
        read_session(&fixture_path("session_v1_nondefault_input.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(
        degradation,
        DegradationReport::default(),
        "well-formed fixture must not need any degradation"
    );

    // Spot-check individual non-default fields end to end, not just an
    // opaque byte comparison — each of these would fail on its own if the
    // corresponding piece of parsing regressed.
    assert_eq!(session.schema_version, 3);
    assert_eq!(session.theme, SessionThemeV1::Dark);
    assert_eq!(session.cursor_style, SessionCursorStyleV1::Bar);
    assert_eq!(session.active_tab, 1);
    assert_eq!(session.window.dpi, 144);
    assert!(session.window.maximized);
    assert_eq!(session.window.bounds.x, -100);
    assert_eq!(session.window.bounds.width, 1920);
    assert_eq!(session.window.monitor_id.as_deref(), Some(r"\\.\DISPLAY2"));
    assert_eq!(session.tabs.len(), 2);
    assert!(session.tabs[0].pinned);
    assert!(!session.tabs[1].pinned);
    assert_eq!(session.recent.len(), 2);
    match &session.recent[1].seed {
        RecentSeedV1::Files { root } => assert_eq!(root, r"C:\Users\dev\docs"),
        other => panic!("expected a files seed, got {other:?}"),
    }
    let LayoutNodeV1::Split(root_split) = &session.tabs[0].root else {
        panic!("tab 0's root must be the split written in the fixture");
    };
    assert_eq!(root_split.ratio, 350_000);
    let LayoutNodeV1::Leaf(LeafNodeV1::Files(files_leaf)) = root_split.children[0].as_ref() else {
        panic!("tab 0's first child must be the files leaf");
    };
    assert_eq!(
        files_leaf.open,
        vec!["node-12".to_string(), "node-45".to_string()]
    );

    // The real assertion this test exists for: re-serializing must reproduce
    // the canonical bytes exactly, proving canonical field order + unknown
    // field drop together.
    let reserialized =
        serde_json::to_vec_pretty(&session).expect("SettingsV1/SessionV1 always serialize");
    assert_eq!(
        String::from_utf8(reserialized).unwrap(),
        String::from_utf8(canonical_bytes()).unwrap(),
        "re-serialized session must match the canonical fixture byte-for-byte"
    );
}

#[test]
fn light_theme_round_trips_through_the_public_session_api() {
    let dir =
        std::env::temp_dir().join(format!("bt-persist-theme-roundtrip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("session.json");
    let session = SessionV1 {
        theme: SessionThemeV1::Light,
        ..SessionV1::default()
    };

    write_session_atomic(&path, &session).unwrap();
    let (loaded, report, degradation) = read_session(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert!(degradation.is_clean());
    assert_eq!(loaded, session);

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn every_cursor_style_round_trips_through_the_public_session_api() {
    for cursor_style in [
        SessionCursorStyleV1::Bar,
        SessionCursorStyleV1::Block,
        SessionCursorStyleV1::Underline,
    ] {
        let dir = std::env::temp_dir().join(format!(
            "bt-persist-cursor-roundtrip-{}-{cursor_style:?}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.json");
        let session = SessionV1 {
            cursor_style,
            ..SessionV1::default()
        };
        write_session_atomic(&path, &session).unwrap();
        let (loaded, report, degradation) = read_session(&path);
        assert_eq!(report, ReadReport::Loaded);
        assert!(degradation.is_clean());
        assert_eq!(loaded, session);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[test]
fn multi_tab_trees_and_active_index_round_trip_together() {
    let dir = std::env::temp_dir().join(format!(
        "bt-persist-multi-tab-roundtrip-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("session.json");
    let tabs = (0..3)
        .map(|index| TabV1 {
            root: LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
                profile_id: "pwsh.exe".to_owned(),
                cwd: format!(r"C:\work\tab-{index}"),
                manual_name: Some(format!("tab {index}")),
            })),
            pinned: index == 0,
            focused_leaf: "leaf-0".to_owned(),
        })
        .collect();
    let session = SessionV1 {
        tabs,
        active_tab: 2,
        ..SessionV1::default()
    };

    write_session_atomic(&path, &session).unwrap();
    let (loaded, report, degradation) = read_session(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert!(degradation.is_clean());
    assert_eq!(loaded, session);

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn canonical_fixture_is_a_fixed_point_of_parse_then_serialize() {
    let (session, report, degradation) =
        read_session(&fixture_path("session_v1_nondefault_canonical.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert!(degradation.is_clean());

    let once = serde_json::to_vec_pretty(&session).unwrap();
    let twice = serde_json::to_vec_pretty(&session).unwrap();
    assert_eq!(
        once, twice,
        "serializing the same value twice must be byte-identical (deterministic output)"
    );
    assert_eq!(once, canonical_bytes());
}

#[test]
fn writing_the_parsed_session_produces_the_canonical_bytes_on_disk() {
    let (session, report, _) = read_session(&fixture_path("session_v1_nondefault_input.json"));
    assert_eq!(report, ReadReport::Loaded);

    let dir =
        std::env::temp_dir().join(format!("bt-persist-roundtrip-write-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out_path = dir.join("session.json");

    write_session_atomic(&out_path, &session).expect("write must succeed");
    let on_disk = std::fs::read(&out_path).unwrap();
    assert_eq!(
        String::from_utf8(on_disk).unwrap(),
        String::from_utf8(canonical_bytes()).unwrap(),
        "atomic_write must persist exactly the canonical bytes"
    );

    std::fs::remove_dir_all(&dir).unwrap();
}
