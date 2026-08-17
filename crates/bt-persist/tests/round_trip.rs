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
    DegradationReport, FilesViewV1, LanguageV1, LayoutNodeV1, LeafNodeV1, PreviewLeafV1,
    PreviewPaneV1, PreviewPoolEntryV1, PsReadLineInviteV1, ReadReport, RecentSeedV1,
    SESSION_SCHEMA_VERSION, SETTINGS_SCHEMA_VERSION, SessionCursorStyleV1, SessionSidebarModeV1,
    SessionTabLayoutV1, SessionThemeV1, SessionV1, SettingsV1, SplitDirectionV1, TabPreviewV1,
    TabV1, TermLeafV1, ThemeModeV1, read_session, read_settings, write_session_atomic,
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
    let (session, report, degradation) = read_session(&fixture_path("session_v7_preview.json"));
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
                graph: None,
            },
            PreviewPaneV1 {
                leaf: "leaf-2".to_owned(),
                cur: None,
                graph: None,
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
    let expected = std::fs::read(fixture_path("session_v7_preview.json")).unwrap();
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
                    graph: None,
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
        SETTINGS_SCHEMA_VERSION, 9,
        "the display-formula switch was the v1→v2 bump, the inline one the v2→v3, \
         the default profile the v3→v4, the Git panel's master switch the v4→v5, \
         the direction-less split's direction the v5→v6, the interface \
         language the v6→v7, the grid's face, its size and the PSReadLine \
         invitation's state the v7→v8 — three keys in one bump because all three \
         arrive with their readers in one change (§1.3) — and the two colour \
         schemes the v8→v9, two keys in one bump because they are one decision's \
         two halves and `theme_mode` still decides which of them is in force"
    );
    assert!(
        defaults.display_formulas,
        "formulas render by default — the switch exists to turn rendering off"
    );
    assert!(
        defaults.inline_formulas,
        "inline formulas render by default too; the site gate is what keeps that safe"
    );
    assert!(
        defaults.git_panel,
        "the Git page is on by default — a feature that arrives switched off is a \
         feature nobody finds. Turning it off is what stops the repository being read"
    );
    assert_eq!(
        defaults.split_direction,
        SplitDirectionV1::Auto,
        "a direction-less split has always cut across the pane's longer side; the \
         setting writes that answer down rather than changing it"
    );
}

/// PIN (the ⌄ ruling, 2026-08-16) — a v5 settings file migrates to v6 with the
/// split direction at `Auto`, and every sibling crosses untouched.
///
/// The fixture is non-default in all five of its older fields (§1.3 rule 1),
/// `git_panel` deliberately among them: a step that reset a sibling to its
/// default while inserting its own field is the one failure this shape of test
/// exists to catch, and `git_panel: false` is the sibling most recently added
/// and therefore the one most likely to be clobbered by a copy-paste of the step
/// above it.
#[test]
fn settings_v5_fixture_migrates_to_v6_with_the_longer_edge_and_disturbs_nothing() {
    let (v6, report) = read_settings(&fixture_path("settings_v5_git_panel_off.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(v6.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(
        v6.split_direction,
        SplitDirectionV1::Auto,
        "a v5 build cut every direction-less split across the pane's longer side \
         and offered no way to ask for anything else — `Auto` carries that \
         behaviour forward rather than imposing a new one"
    );
    assert_eq!(
        v6.theme_mode,
        ThemeModeV1::Dark,
        "v5→v6 is structural: every sibling crosses untouched"
    );
    assert!(!v6.display_formulas);
    assert!(v6.inline_formulas);
    assert_eq!(v6.default_profile, "wsl");
    assert!(
        !v6.git_panel,
        "and the switch this user deliberately turned off stays off"
    );
}

/// PIN (the Language row, 2026-08-17) — a v6 settings file migrates to v7 with
/// the language following the operating system, and every sibling crosses
/// untouched.
///
/// The fixture is non-default in all six of its older fields (§1.3 rule 1),
/// `split_direction: "Down"` deliberately among them: it is the sibling added
/// one version ago, and therefore the one a copy-paste of the step above would
/// most plausibly reset while inserting its own field.
///
/// `System` and not `English` is the half of this worth pinning. Every build
/// before v7 drew English and read nothing to decide that, so `English` would
/// look right on the machine the migration runs on and be wrong on a Chinese
/// Windows — a decision pinned into the file that its owner never made.
#[test]
fn settings_v6_fixture_migrates_to_v7_following_the_system_and_disturbs_nothing() {
    let (v7, report) = read_settings(&fixture_path("settings_v6_split_down.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(v7.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(
        v7.language,
        LanguageV1::System,
        "a v6 build never asked which language to speak, so the migration must          write the answer that defers rather than one that decides"
    );
    assert_eq!(v7.theme_mode, ThemeModeV1::Light);
    assert!(!v7.display_formulas);
    assert!(v7.inline_formulas);
    assert_eq!(v7.default_profile, "gitbash");
    assert!(!v7.git_panel);
    assert_eq!(
        v7.split_direction,
        SplitDirectionV1::Down,
        "v6→v7 is structural: every sibling crosses untouched"
    );
}

/// PIN (the font rows and the PSReadLine invitation, 2026-08-17) — a v7 settings
/// file migrates to v8 with an unnamed face at 16 logical pixels and an
/// invitation nobody has been shown, and every sibling crosses untouched.
///
/// The fixture is non-default in all seven of its older fields (§1.3 rule 1),
/// `language: "Chinese"` deliberately among them: it is the sibling added one
/// version ago, and therefore the one a copy-paste of the step above would most
/// plausibly reset while inserting its own keys.
///
/// `""` and not `"Consolas"` is the half worth pinning, and it is `v3_to_v4`'s
/// ruling reappearing. Every v7 build drew Consolas because it was the only face
/// it had; naming it here would be indistinguishable from a user who opened the
/// list and picked it, and would go on being written into files long after the
/// build's default face had moved.
#[test]
fn settings_v7_fixture_migrates_to_v8_with_an_unnamed_face_and_disturbs_nothing() {
    let (v8, report) = read_settings(&fixture_path("settings_v7_chinese_right.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(v8.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(
        v8.terminal_font_family, "",
        "a v7 build never asked which face to draw the grid in, so the migration \
         must write the answer that defers rather than one that decides"
    );
    assert_eq!(
        v8.terminal_font_size, 16,
        "16 is the size every v7 build drew at — the step writes that answer \
         down rather than changing it"
    );
    assert_eq!(
        v8.psreadline_invite,
        PsReadLineInviteV1::NotAsked,
        "a v7 build had no invitation, so this user is owed the offer once"
    );
    assert_eq!(v8.theme_mode, ThemeModeV1::Dark);
    assert!(!v8.display_formulas);
    assert!(!v8.inline_formulas);
    assert_eq!(v8.default_profile, "wsl");
    assert!(!v8.git_panel);
    assert_eq!(v8.split_direction, SplitDirectionV1::Right);
    assert_eq!(
        v8.language,
        LanguageV1::Chinese,
        "v7→v8 is structural: every sibling crosses untouched"
    );
}

/// PIN (the colour-scheme row, 2026-08-17) — a v8 settings file migrates to v9
/// with neither side of the theme naming a palette, and every sibling crosses
/// untouched.
///
/// The fixture is non-default in all ten of its older fields (§1.3 rule 1), the
/// three v8 keys deliberately among them: they are the siblings added one
/// version ago, and therefore the ones a copy-paste of the step above would most
/// plausibly reset while inserting its own pair.
///
/// Two empty strings and not this build's two default palette names is the half
/// worth pinning, and it is `v3_to_v4`'s ruling arriving a third time. A v8
/// build painted one light palette and one dark one because they were the only
/// two it had; naming them here would be indistinguishable from a user who
/// opened the list and picked them, and would go on being written into files
/// long after the built-in palette had been renamed or improved around them.
/// That *both* are empty is the other half: a step that filled one side in would
/// leave the other for the reader to guess at, which is what having two fields
/// exists to prevent.
#[test]
fn settings_v8_fixture_migrates_to_v9_with_no_scheme_named_and_disturbs_nothing() {
    let (v9, report) = read_settings(&fixture_path("settings_v8_scheme_absent.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(v9.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(
        v9.light_scheme, "",
        "a v8 build never asked which palette to paint the grid in, so the \
         migration must write the answer that defers rather than one that decides"
    );
    assert_eq!(
        v9.dark_scheme, "",
        "and it must defer on both sides — filling one in would leave the other \
         to be guessed at the first time the theme flipped"
    );
    assert_eq!(v9.theme_mode, ThemeModeV1::Light);
    assert!(!v9.display_formulas);
    assert!(v9.inline_formulas);
    assert_eq!(v9.default_profile, "gitbash");
    assert!(!v9.git_panel);
    assert_eq!(v9.split_direction, SplitDirectionV1::Down);
    assert_eq!(v9.language, LanguageV1::Chinese);
    assert_eq!(v9.terminal_font_family, "Cascadia Mono");
    assert_eq!(v9.terminal_font_size, 20);
    assert_eq!(
        v9.psreadline_invite,
        PsReadLineInviteV1::Installed,
        "v8→v9 is structural: every sibling crosses untouched"
    );
}

/// PIN — a v9 file that *names* both schemes is loaded without migration and
/// hands both names back exactly as written.
///
/// The companion to the migration test above, and it pins the opposite failure.
/// That one proves an unasked user is not answered for; this one proves an
/// answered user is not overruled. The names go through this crate opaquely: it
/// does not know whether "Solarized Light" resolves to anything on this machine,
/// and must not decide that a spelling it cannot resolve is wrong — resolving is
/// the reading build's question, and its answer is §5.4 逐叶降级.
#[test]
fn a_v9_settings_file_that_names_both_schemes_keeps_both_names() {
    let (v9, report) = read_settings(&fixture_path("settings_v9_solarized_pair.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(v9.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(v9.light_scheme, "Solarized Light");
    assert_eq!(v9.dark_scheme, "Solarized Dark");
    assert_eq!(
        v9.theme_mode,
        ThemeModeV1::System,
        "a user who follows the system is exactly who needs both sides named"
    );
    assert!(v9.display_formulas);
    assert!(!v9.inline_formulas);
    assert_eq!(v9.default_profile, "wsl");
    assert!(!v9.git_panel);
    assert_eq!(v9.split_direction, SplitDirectionV1::Right);
    assert_eq!(v9.language, LanguageV1::English);
    assert_eq!(v9.terminal_font_family, "MS Gothic");
    assert_eq!(v9.terminal_font_size, 14);
    assert_eq!(v9.psreadline_invite, PsReadLineInviteV1::Declined);

    // And back out again, byte-for-byte in meaning: the pair a user chose is the
    // pair the next launch reads.
    let dir = std::env::temp_dir().join(format!(
        "bt-persist-settings-v9-schemes-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    write_settings_atomic(&path, &v9).unwrap();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.contains("\"light_scheme\": \"Solarized Light\"")
            && on_disk.contains("\"dark_scheme\": \"Solarized Dark\""),
        "a scheme is written as its name, never as an index into a list that is \
         part built-in and part user-supplied: {on_disk}"
    );
    let (round_tripped, report) = read_settings(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(round_tripped, v9);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// PIN (R1 / the master switch, 2026-08-15) — a v4 settings file migrates to v5
/// with the Git panel on, and a v6 session gives every `files` leaf the page it
/// was provably on.
///
/// Both halves in one test because they are one ruling arriving in two files, and
/// because the interesting failure is the same for both: a step that inserted its
/// field while disturbing a sibling. The fixtures are non-default in their older
/// fields for exactly that reason (§1.3 rule 1).
#[test]
fn the_git_page_migrates_on_and_every_files_column_arrives_on_its_tree() {
    let (settings, report) = read_settings(&fixture_path("settings_v4_profile_chosen.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(settings.schema_version, SETTINGS_SCHEMA_VERSION);
    assert!(
        settings.git_panel,
        "a v4 build had no Git page at all, so `off` would freeze an absence \
         rather than preserve a choice — the feature takes the product's default"
    );
    assert_eq!(
        settings.default_profile, "gitbash",
        "v4→v5 is structural: every sibling crosses untouched"
    );
    assert!(!settings.inline_formulas);
    assert_eq!(settings.theme_mode, ThemeModeV1::Light);

    // A `files` leaf under a split. The vault's own seed is deliberately *not*
    // given a page — see `migrate_session_v6_to_v7` — and the assertion below
    // pins that: a closed tab's column is `{ root }` and nothing more.
    let (session, report, degradation) =
        read_session(&fixture_path("session_v6_files_column.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(degradation, DegradationReport::default());
    assert_eq!(session.schema_version, SESSION_SCHEMA_VERSION);
    let LayoutNodeV1::Split(split) = &session.tabs[0].root else {
        panic!("the fixture's tab is a split");
    };
    let LayoutNodeV1::Leaf(LeafNodeV1::Files(column)) = split.children[0].as_ref() else {
        panic!("the fixture's first child is a files column");
    };
    assert_eq!(
        column.view,
        FilesViewV1::Files,
        "every column written before v7 was written by a build with one page"
    );
    assert_eq!(
        column.open,
        vec!["crates".to_owned()],
        "the step adds a key and disturbs nothing beside it"
    );
    assert_eq!(column.width, 260);
    assert_eq!(
        session.recent[0].seed,
        RecentSeedV1::Files {
            root: "D:\\other".to_owned()
        },
        "a vault seed is the whole of what a closed tab is rebuilt from, and a \
         page is not part of it — the column it reopens is born on its tree"
    );

    // And the page a user actually chose survives a round trip, which is the
    // whole of what the field is for.
    let dir = std::env::temp_dir().join(format!("bt-persist-git-view-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("session.json");
    let mut chosen = session.clone();
    let LayoutNodeV1::Split(split) = &mut chosen.tabs[0].root else {
        unreachable!()
    };
    let LayoutNodeV1::Leaf(LeafNodeV1::Files(column)) = split.children[0].as_mut() else {
        unreachable!()
    };
    column.view = FilesViewV1::Git;
    write_session_atomic(&path, &chosen).unwrap();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.contains("\"view\": \"git\""),
        "the page is written in the design's own word, lower case: {on_disk}"
    );
    let (round_tripped, report, _) = read_session(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(round_tripped, chosen);
    std::fs::remove_dir_all(&dir).unwrap();
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
