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
    DegradationReport, LayoutNodeV1, LeafNodeV1, PreviewLeafV1, PreviewPaneV1, PreviewPoolEntryV1,
    ReadReport, RecentSeedV1, SESSION_SCHEMA_VERSION, SETTINGS_SCHEMA_VERSION,
    SessionCursorStyleV1, SessionSidebarModeV1, SessionTabLayoutV1, SessionThemeV1, SessionV1,
    SettingsV1, TabPreviewV1, TabV1, TermLeafV1, ThemeModeV1, read_session, read_settings,
    write_session_atomic, write_settings_atomic,
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
    assert_eq!(session.schema_version, SESSION_SCHEMA_VERSION);
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

    // **v5 -> v6: every `profile_id` in the document arrives as a slug.**
    //
    // This fixture is the strongest case available for it, because it is written
    // throughout in the *other* convention — §3.3's "shell executable path"
    // spelling, and two different PowerShell installs at that. A migration that
    // walked only the top-level object, or only the first leaf, or that mapped
    // only the one path it happened to be written against, fails here rather
    // than in somebody's real session file.
    let LayoutNodeV1::Split(inner) = root_split.children[1].as_ref() else {
        panic!("tab 0's second child must be the nested split");
    };
    // The two children were written as two different executables, and they
    // arrive as two different slugs: the v1 document recorded which PowerShell
    // actually ran, and this build has a profile for each of them.
    for (index, expected_cwd, expected_id) in [
        (0usize, r"C:\Users\dev\project", "pwsh"),
        (1, r"C:\Users\dev", "winps"),
    ] {
        let LayoutNodeV1::Leaf(LeafNodeV1::Term(term)) = inner.children[index].as_ref() else {
            panic!("the nested split's children are both term leaves");
        };
        assert_eq!(
            term.profile_id, expected_id,
            "a term leaf two splits deep must arrive slugged"
        );
        // The step renamed the profile and nothing standing beside it.
        assert_eq!(term.cwd, expected_cwd);
    }
    let LayoutNodeV1::Leaf(LeafNodeV1::Term(lone)) = &session.tabs[1].root else {
        panic!("tab 1's root must be the lone term leaf");
    };
    assert_eq!(lone.profile_id, "pwsh");
    match &session.recent[0].seed {
        RecentSeedV1::Term { profile_id, .. } => assert_eq!(
            profile_id, "pwsh",
            "a Recent seed carries the same field and migrates with it"
        ),
        other => panic!("expected a term seed, got {other:?}"),
    }

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

/// N43 — **the preview leaf and the tab's content section, on the wire.**
///
/// The block's red line L1 splits one pane's state across two places on purpose:
/// `pinned` is *geometry* and rides in the layout tree, while *which file the
/// pane was showing* and *which buffers the tab had open* are content and ride
/// in `tab.preview`. A fixture that carried only one of the two would let the
/// other be dropped silently, which is exactly the failure the split invites —
/// so both are here, in one document, with a pool entry that no pane is showing
/// so "the pool is a history, not a list of what is on screen" cannot collapse
/// into "the pool is the panes" without this failing.
///
/// Every field is non-default (`CONVENTIONS.md` §三): light theme, block cursor,
/// a vertical strip, an icon rail — a preview fixture written in defaults would
/// pass while the reader dropped four fields on the way past.
#[test]
fn a_preview_pane_keeps_its_pin_in_the_tree_and_its_file_in_the_content_section() {
    let (session, report, degradation) = read_session(&fixture_path("session_v6_preview.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(degradation, DegradationReport::default());
    assert_eq!(session.schema_version, SESSION_SCHEMA_VERSION);
    assert_eq!(session.theme, SessionThemeV1::Light);
    assert_eq!(session.cursor_style, SessionCursorStyleV1::Block);
    assert_eq!(session.tab_layout, SessionTabLayoutV1::Vertical);
    assert_eq!(session.sidebar_mode, SessionSidebarModeV1::Icons);

    let LayoutNodeV1::Split(root) = &session.tabs[0].root else {
        panic!("tab 0's root is the split the fixture writes");
    };
    let LayoutNodeV1::Split(column) = root.children[1].as_ref() else {
        panic!("the tab's right-hand child is the column of two previews");
    };
    // Two preview leaves in one tab is the state the pin exists to create, and
    // it is the state a "there is one preview per tab" reader would flatten.
    let pins: Vec<bool> = column
        .children
        .iter()
        .map(|child| match child.as_ref() {
            LayoutNodeV1::Leaf(LeafNodeV1::Preview(preview)) => preview.pinned,
            other => panic!("expected a preview leaf, got {other:?}"),
        })
        .collect();
    assert_eq!(
        pins,
        vec![true, false],
        "the pin is per leaf, and it is geometry"
    );

    let content = session.tabs[0]
        .preview
        .as_ref()
        .expect("the fixture's first tab carries a content section");
    assert_eq!(
        content.panes,
        vec![
            PreviewPaneV1 {
                leaf: "leaf-1".to_owned(),
                cur: Some(r"C:\Users\dev\project\README.md".to_owned()),
            },
            PreviewPaneV1 {
                leaf: "leaf-2".to_owned(),
                cur: None,
            },
        ],
        "each preview leaf is named by the same positional token `focused_leaf` uses, \
         and a pane showing nothing says so rather than being left out"
    );
    assert_eq!(
        content.pool,
        vec![
            PreviewPoolEntryV1 {
                path: r"C:\Users\dev\project\README.md".to_owned(),
                name: "README.md".to_owned(),
            },
            PreviewPoolEntryV1 {
                path: r"C:\Users\dev\project\src\main.rs".to_owned(),
                name: "main.rs".to_owned(),
            },
        ],
        "the pool is the tab's history: it holds a buffer no pane is showing"
    );
    assert_eq!(
        session.recent[0].previews,
        vec![r"C:\Users\dev\notes\todo.md".to_owned()],
        "裁决 10 — a closed tab's preview goes into Recent with it, so undo-close \
         and the restore prompt are not two doors with one of them broken"
    );

    // The same fixed-point gate the canonical fixture gets: reading and writing
    // this document must reproduce it byte for byte.
    let reserialized = serde_json::to_vec_pretty(&session).expect("SessionV1 always serializes");
    let expected = std::fs::read(fixture_path("session_v6_preview.json")).unwrap();
    assert_eq!(
        String::from_utf8(reserialized).unwrap(),
        String::from_utf8(expected).unwrap().trim_end().to_owned(),
        "the preview fixture must be a fixed point of parse-then-serialize"
    );
}

/// The other half of "additive": a document written before the content section
/// existed must read as *no previews*, not as a parse failure and not as an
/// invented empty pane list.
#[test]
fn a_document_written_before_the_content_section_reads_as_no_preview_at_all() {
    let (session, report, degradation) =
        read_session(&fixture_path("session_v1_nondefault_canonical.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert!(degradation.is_clean());
    assert!(
        session.tabs.iter().all(|tab| tab.preview.is_none()),
        "no content section on disk is no content section in memory"
    );
    assert!(
        session.recent.iter().all(|entry| entry.previews.is_empty()),
        "and a Recent entry from before 裁决 10 brings back a tab with no preview"
    );
}

/// And the field must not appear on the way back out for a tab that has no
/// preview: the canonical fixture is a fixed point, and an `Option` that
/// serialized as `null` would have rewritten every session file on disk.
#[test]
fn a_tab_with_no_preview_writes_no_content_section() {
    let dir =
        std::env::temp_dir().join(format!("bt-persist-preview-absent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("session.json");
    let session = SessionV1 {
        tabs: vec![TabV1 {
            root: LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
                profile_id: "pwsh".to_owned(),
                cwd: r"C:\work".to_owned(),
                manual_name: None,
            })),
            pinned: false,
            focused_leaf: "leaf-0".to_owned(),
            preview: None,
        }],
        ..SessionV1::default()
    };
    write_session_atomic(&path, &session).unwrap();
    let on_disk = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
    assert!(
        !on_disk.contains("preview"),
        "a tab with nothing to say about previews says nothing"
    );
    let (loaded, report, _) = read_session(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(loaded, session);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The content section survives the public write/read API, not only the fixture
/// — including the pane that is showing nothing, which is the one a
/// `filter_map` over `cur` would quietly drop.
#[test]
fn the_content_section_round_trips_through_the_public_session_api() {
    let dir = std::env::temp_dir().join(format!(
        "bt-persist-preview-roundtrip-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("session.json");
    let session = SessionV1 {
        tabs: vec![TabV1 {
            root: LayoutNodeV1::Leaf(LeafNodeV1::Preview(PreviewLeafV1 { pinned: true })),
            pinned: false,
            focused_leaf: "leaf-0".to_owned(),
            preview: Some(TabPreviewV1 {
                panes: vec![PreviewPaneV1 {
                    leaf: "leaf-0".to_owned(),
                    cur: None,
                }],
                pool: vec![PreviewPoolEntryV1 {
                    path: r"C:\work\notes.md".to_owned(),
                    name: "notes.md".to_owned(),
                }],
            }),
        }],
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
        assert_eq!(migrated.schema_version, SESSION_SCHEMA_VERSION);
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
            preview: None,
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
        SETTINGS_SCHEMA_VERSION, 4,
        "the display-formula switch was the v1→v2 bump, the inline one the v2→v3, \
         and the default profile the v3→v4 (§1.3)"
    );
    assert!(
        defaults.display_formulas,
        "formulas render by default — the switch exists to turn rendering off"
    );
    assert!(
        defaults.inline_formulas,
        "inline formulas render by default too; the site gate is what keeps that safe"
    );
}

/// PIN — v3→v4 adds the default profile as **unchosen**, and touches nothing.
///
/// The fixture is non-default in all three of its older fields (§1.3 rule 1), so
/// a step that rewrote a sibling while inserting its own field could not hide
/// behind a matching default on the way back out. And a file this old must not
/// come back claiming its owner picked PowerShell: they were never asked.
#[test]
fn settings_v3_fixture_migrates_to_v4_with_no_profile_chosen() {
    let (v4, report) = read_settings(&fixture_path("settings_v3_inline_off.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(v4.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(
        v4.default_profile, "",
        "a v3 user was never offered the question, and a migration that answered \
         it for them would be indistinguishable from one they had answered"
    );
    assert_eq!(
        v4.theme_mode,
        ThemeModeV1::Dark,
        "v3→v4 is structural: the theme crosses untouched"
    );
    assert!(!v4.display_formulas, "and so does the display switch");
    assert!(!v4.inline_formulas, "and so does the inline one");

    // A file that *does* name a profile keeps naming it — the id is opaque here,
    // and this crate must not decide that a spelling it does not recognise is
    // wrong. Which profile it means is the reading build's question.
    let dir = std::env::temp_dir().join(format!(
        "bt-persist-settings-v3-migration-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    let chosen = SettingsV1 {
        default_profile: "a-profile-this-build-never-heard-of".to_owned(),
        ..SettingsV1::default()
    };
    write_settings_atomic(&path, &chosen).unwrap();
    let (round_tripped, report) = read_settings(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(round_tripped, chosen);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn settings_v1_fixture_migrates_to_v2_preserving_theme_and_rendering_formulas() {
    // §1.3 rule 1 demands a *non-default* fixture: theme_mode is `Dark`, so a
    // migration that dropped or reordered fields could not hide behind the
    // `System` default. The unknown hand-edited key must vanish (ruling 4).
    let (v2, report) = read_settings(&fixture_path("settings_v2_formulas_off.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(v2.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(
        v2.theme_mode,
        ThemeModeV1::Light,
        "v2→v3 carries the pre-existing theme across untouched"
    );
    assert!(
        !v2.display_formulas,
        "v2→v3 is structural: it adds a field and must not disturb the sibling \
         switch this user had deliberately turned off"
    );
    assert!(
        v2.inline_formulas,
        "the inline switch arrives on. A v2 build never rendered an inline run at \
         all — the detector was disabled outright — so carrying that absence \
         forward as `false` would be freezing a missing feature, not preserving a \
         choice the user had made"
    );

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
    assert!(
        migrated.inline_formulas,
        "a file this old predates the inline switch entirely, so it arrives at the \
         product default rather than at a preserved behaviour — see migrate_settings_v2_to_v3"
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
