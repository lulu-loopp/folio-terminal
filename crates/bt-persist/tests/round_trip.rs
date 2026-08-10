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
    DegradationReport, LayoutNodeV1, LeafNodeV1, ReadReport, RecentSeedV1, SETTINGS_SCHEMA_VERSION,
    SessionCursorStyleV1, SessionSidebarModeV1, SessionTabLayoutV1, SessionThemeV1, SessionV1,
    SettingsV1, TabV1, TermLeafV1, ThemeModeV1, read_session, read_settings, write_session_atomic,
    write_settings_atomic,
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
    assert_eq!(session.schema_version, 5);
    assert_eq!(session.theme, SessionThemeV1::Dark);
    assert_eq!(session.cursor_style, SessionCursorStyleV1::Bar);
    assert_eq!(session.tab_layout, SessionTabLayoutV1::Horizontal);
    assert_eq!(session.sidebar_mode, SessionSidebarModeV1::Expanded);
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
fn every_theme_mode_round_trips_through_the_public_session_api() {
    for theme in [
        SessionThemeV1::System,
        SessionThemeV1::Light,
        SessionThemeV1::Dark,
    ] {
        let dir = std::env::temp_dir().join(format!(
            "bt-persist-theme-roundtrip-{}-{theme:?}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.json");
        let session = SessionV1 {
            theme,
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
fn v3_dark_and_light_fixtures_migrate_and_round_trip_without_changing_mode() {
    for (fixture, expected_theme) in [
        ("session_v3_dark.json", SessionThemeV1::Dark),
        ("session_v3_light.json", SessionThemeV1::Light),
    ] {
        let (migrated, report, degradation) = read_session(&fixture_path(fixture));
        assert_eq!(report, ReadReport::Loaded);
        assert!(degradation.is_clean());
        assert_eq!(migrated.schema_version, 5);
        assert_eq!(migrated.theme, expected_theme);

        let dir = std::env::temp_dir().join(format!(
            "bt-persist-v3-theme-migration-{}-{expected_theme:?}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.json");
        write_session_atomic(&path, &migrated).unwrap();
        let (round_tripped, report, degradation) = read_session(&path);
        assert_eq!(report, ReadReport::Loaded);
        assert!(degradation.is_clean());
        assert_eq!(round_tripped, migrated);
        std::fs::remove_dir_all(&dir).unwrap();
    }
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
fn every_tab_layout_round_trips_through_the_public_session_api() {
    for tab_layout in [SessionTabLayoutV1::Horizontal, SessionTabLayoutV1::Vertical] {
        let dir = std::env::temp_dir().join(format!(
            "bt-persist-tab-layout-roundtrip-{}-{tab_layout:?}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.json");
        let session = SessionV1 {
            tab_layout,
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
fn every_sidebar_mode_round_trips_through_the_public_session_api() {
    for sidebar_mode in [SessionSidebarModeV1::Expanded, SessionSidebarModeV1::Icons] {
        let dir = std::env::temp_dir().join(format!(
            "bt-persist-sidebar-mode-roundtrip-{}-{sidebar_mode:?}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.json");
        let session = SessionV1 {
            sidebar_mode,
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

// --- settings.json: display-formula rendering switch (settings schema v2) ---

#[test]
fn display_formulas_round_trips_through_the_public_settings_api() {
    // Both states, not just the non-default one: a getter that ignored the
    // stored value and always answered `true` would still pass a one-sided
    // test (`CONVENTIONS.md` §三 "默认值会掩盖 bug").
    for display_formulas in [false, true] {
        let dir = std::env::temp_dir().join(format!(
            "bt-persist-display-formulas-roundtrip-{}-{display_formulas}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let settings = SettingsV1 {
            display_formulas,
            ..SettingsV1::default()
        };

        write_settings_atomic(&path, &settings).unwrap();
        let (loaded, report) = read_settings(&path);
        assert_eq!(report, ReadReport::Loaded);
        assert_eq!(loaded, settings);
        assert_eq!(loaded.display_formulas, display_formulas);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[test]
fn settings_defaults_render_formulas_at_the_current_schema_version() {
    let defaults = SettingsV1::default();
    assert_eq!(defaults.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(
        SETTINGS_SCHEMA_VERSION, 2,
        "adding the display-formula switch is the v1→v2 bump (§1.3)"
    );
    assert!(
        defaults.display_formulas,
        "formulas render by default — the switch exists to turn rendering off"
    );
}

#[test]
fn settings_v1_fixture_migrates_to_v2_preserving_theme_and_rendering_formulas() {
    // §1.3 rule 1 demands a *non-default* fixture: theme_mode is `Dark`, so a
    // migration that dropped or reordered fields could not hide behind the
    // `System` default. The unknown hand-edited key must vanish (ruling 4).
    let (migrated, report) = read_settings(&fixture_path("settings_v1_nondefault.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(migrated.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(
        migrated.theme_mode,
        ThemeModeV1::Dark,
        "migration must carry the pre-existing theme across untouched"
    );
    assert!(
        migrated.display_formulas,
        "every pre-v2 settings file was written by a build that always rendered \
         formulas — the migration must preserve that behaviour, not impose a new one"
    );

    let dir = std::env::temp_dir().join(format!(
        "bt-persist-settings-v1-migration-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    write_settings_atomic(&path, &migrated).unwrap();
    let (round_tripped, report) = read_settings(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(round_tripped, migrated);
    let on_disk = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
    assert!(
        !on_disk.contains("bt_unknown_hand_edit"),
        "unknown fields are dropped, never round-tripped (§1.3 ruling 4)"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
