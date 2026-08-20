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
    BackgroundFitV1, DegradationReport, FilesViewV1, LanguageV1, LayoutNodeV1, LeafNodeV1,
    MinimumContrastV1, PreviewLeafV1, PreviewPaneV1, PreviewPoolEntryV1, PsReadLineInviteV1,
    ReadReport, RecentSeedV1, SESSION_SCHEMA_VERSION, SETTINGS_SCHEMA_VERSION,
    SessionCursorStyleV1, SessionSidebarModeV1, SessionTabLayoutV1, SessionThemeV1, SessionV1,
    SettingsV1, SplitDirectionV1, TabPreviewV1, TabV1, TermLeafV1, ThemeModeV1, read_session,
    read_settings, write_session_atomic, write_settings_atomic,
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
    // this document must reproduce it byte for byte. The bytes compared against
    // are the **v8** copy of the same document, because the v7 one is now a
    // migration source and a migrated document is by definition not what it was
    // read from — see `a_v7_vault_arrives_at_v8_with_nothing_but_its_version
    // _changed`, which is the other half of this pair.
    let reserialized = serde_json::to_vec_pretty(&session).expect("SessionV1 always serializes");
    let expected = std::fs::read(fixture_path("session_v8_preview.json")).unwrap();
    assert_eq!(
        String::from_utf8(reserialized).unwrap(),
        String::from_utf8(expected).unwrap().trim_end().to_owned(),
        "the preview fixture must be a fixed point of parse-then-serialize"
    );
}

/// **v7 → v8 is a version and nothing else, and the vault is where that has to
/// be proved.**
///
/// The bump exists for a variant that no v7 document can contain, so the honest
/// step touches no field — and "touches no field" is exactly the claim a
/// migration test is for. The fixture carries a non-default value in every older
/// field (`CONVENTIONS.md` §三) and a `term` vault entry with a `previews` list,
/// which is the row a step that rewrote seeds would most easily damage.
#[test]
fn a_v7_vault_arrives_at_v8_with_nothing_but_its_version_changed() {
    let (migrated, report, degradation) = read_session(&fixture_path("session_v7_preview.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(degradation, DegradationReport::default());
    assert_eq!(migrated.schema_version, 8);

    let (native, report, _) = read_session(&fixture_path("session_v8_preview.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(
        migrated, native,
        "a v7 document read at v8 is the v8 document, field for field"
    );
    assert_eq!(
        migrated.recent[0].seed,
        RecentSeedV1::Term {
            profile_id: "pwsh".to_owned(),
            cwd: r"C:\Users\dev\notes".to_owned(),
            manual_name: None,
        },
        "an old vault entry is already what it should be at v8"
    );
}

/// **T5/§7.1.6h — a tab made of a folder and a tab made of a file survive a
/// restart, and so do the two vault rows that reopen them.**
///
/// The whole of what the sessionless slice asks of the disk, in one document,
/// because the interesting failure is that they are *four* facts that look like
/// one: a tree whose only leaf is a `files` leaf, a tree whose only leaf is a
/// `preview` leaf, the content section that says which file that preview was on,
/// and the two vault seeds. A reader that quietly required a `term` leaf
/// somewhere would pass three of them.
///
/// **No field is new here except the vault's third seed shape.** That is the
/// finding this slice recorded rather than a gap it closed: `files` leaves have
/// carried their root since v1 and `preview` panes their file since the content
/// section landed, so a term-less tab was *expressible* on disk long before it
/// was constructible in the program. What was missing was upstream — the reader
/// that answered "no terminal leaf" by handing back a lone terminal — and no
/// schema change could have fixed that.
///
/// Fixed-point gated like every other fixture: read, write, compare bytes.
#[test]
fn a_folder_tab_and_a_file_tab_round_trip_with_the_seeds_that_reopen_them() {
    let (session, report, degradation) = read_session(&fixture_path("session_v8_sessionless.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(degradation, DegradationReport::default());
    assert_eq!(session.schema_version, SESSION_SCHEMA_VERSION);

    let LayoutNodeV1::Leaf(LeafNodeV1::Files(column)) = &session.tabs[0].root else {
        panic!("the first tab is one files leaf and nothing else");
    };
    assert_eq!(column.root, r"D:\work\folio");
    assert_eq!(column.view, FilesViewV1::Git);
    assert!(column.remotes_open);
    assert_eq!(
        session.tabs[0].focused_leaf, "leaf-0",
        "the token names the column, which is a seat this build now lets it name"
    );
    assert!(
        session.tabs[0].pinned,
        "a folder tab pins like any other tab"
    );

    let LayoutNodeV1::Leaf(LeafNodeV1::Preview(preview)) = &session.tabs[1].root else {
        panic!("the second tab is one preview leaf and nothing else");
    };
    assert!(preview.pinned);
    assert_eq!(
        session.tabs[1]
            .preview
            .as_ref()
            .expect("a preview tab carries the file it is on")
            .panes,
        vec![PreviewPaneV1 {
            leaf: "leaf-0".to_owned(),
            cur: Some(r"D:\work\folio\docs\DESIGN.md".to_owned()),
            graph: None,
        }],
    );

    assert_eq!(
        session.recent[0].seed,
        RecentSeedV1::Files {
            root: r"D:\other".to_owned()
        },
    );
    assert_eq!(
        session.recent[1].seed,
        RecentSeedV1::Preview {
            path: r"D:\work\folio\README.md".to_owned()
        },
        "§7.1.6h's third seed shape — without it, closing a file tab puts nothing \
         in the vault and Ctrl+Shift+T is a door onto an empty store"
    );

    let reserialized = serde_json::to_vec_pretty(&session).expect("SessionV1 always serializes");
    let expected = std::fs::read(fixture_path("session_v8_sessionless.json")).unwrap();
    assert_eq!(
        String::from_utf8(reserialized).unwrap(),
        String::from_utf8(expected).unwrap().trim_end().to_owned(),
        "the sessionless fixture must be a fixed point of parse-then-serialize"
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
        SETTINGS_SCHEMA_VERSION, 17,
        "the display-formula switch was the v1→v2 bump, the inline one the v2→v3, \
         the default profile the v3→v4, the Git panel's master switch the v4→v5, \
         the direction-less split's direction the v5→v6, the interface \
         language the v6→v7, the grid's face, its size and the PSReadLine \
         invitation's state the v7→v8 — three keys in one bump because all three \
         arrive with their readers in one change (§1.3) — and the two colour \
         schemes the v8→v9, two keys in one bump because they are one decision's \
         two halves and `theme_mode` still decides which of them is in force — \
         and the window's ground the v9→v10: a picture, its fit, how much of it \
         comes through, how much of the desktop comes through behind it, whether \
         Windows blurs that, and whether the window stays in front. Six keys in \
         one bump because four of them describe one ground nobody can set a \
         quarter of, and because a schema version is a file format rather than a \
         changelog — and the Advanced disclosure's own list the v10→v11, one key \
         on its own day, the Tables switch the v11-to-v12, and the Rendered \
         blocks page's own Maximum height the v12-to-v13, and the Terminal page's \
         own Scrollback the v13-to-v14, and the Appearance page's own Focus mode \
         the v14-to-v15, and the Appearance page's own Minimum contrast the \
         v15-to-v16, and the Terminal page's own Notifications the v16-to-v17 —          one key on one day, seven times running"
    );
    assert_eq!(
        defaults.minimum_contrast,
        MinimumContrastV1::Off,
        "the one row in this document that overrides a colour a program asked \
         for defaults to doing nothing, and its migration carries that forward: \
         a build that repainted yesterday's output on the day it learned how \
         would be this feature pointed backwards"
    );
    assert!(
        !defaults.focus_mode,
        "no build before v15 had a focus mode to open in, and a migration that \
         switched a layout mode on for a reader who has never met the row would \
         be this crate redecorating somebody's window on upgrade"
    );
    assert!(
        defaults.terminal_notifications,
        "the two rulings look alike and land opposite ways. Focus mode is off \
         because it replaces the window somebody opens every morning; this is on \
         because it changes nothing anybody can see until a program asks for it, \
         and a switch that has to be found before it works once is a feature most \
         of its users never learn they have"
    );
    assert_eq!(
        defaults.scrollback_lines, 100_000,
        "the capacity every build has kept since M0-alpha, written down rather \
         than changed: a row that shipped by quietly shrinking somebody's history \
         would be answering a question they had not been asked"
    );
    assert!(
        defaults.advanced_open.is_empty(),
        "progressive disclosure that arrived already disclosed would be a longer \
         page with a triangle on it"
    );
    assert_eq!(
        defaults.background_image, "",
        "no picture, and the empty string is the value rather than a stand-in \
         for one: there is no wallpaper this build could have meant"
    );
    assert_eq!(
        defaults.background_fit,
        BackgroundFitV1::Fill,
        "the only fit that both covers the window and leaves the picture looking \
         like itself; unreachable anyway until a picture is named"
    );
    assert_eq!(defaults.background_image_opacity, 100);
    assert_eq!(
        defaults.background_opacity, 100,
        "an opaque window is what every build before v10 drew, and a default \
         that made somebody's terminal see-through on upgrade would be this \
         crate deciding something it was never asked"
    );
    assert!(!defaults.acrylic);
    assert!(
        !defaults.always_on_top,
        "a window that arrives in front of everything else has taken a decision \
         about the whole desktop on the strength of being launched"
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

/// PIN (the appearance pack, 2026-08-17) — a v9 settings file migrates to v10
/// with the ground v9 already drew, and every one of the twelve older fields
/// crosses untouched.
///
/// The fixture is non-default in all twelve (§1.3 rule 1), the two v9 scheme
/// keys deliberately among them: they are the siblings added one version ago
/// and therefore the ones a copy-paste of the step above would most plausibly
/// reset while inserting its own six.
///
/// The half worth pinning is that **nothing visible changes**. Six keys appearing
/// at once looks like `v3_to_v4`'s kind of step — a build filling in an answer
/// on the user's behalf — and it is not: a v9 build drew no picture at an opaque
/// ground with no backdrop and no topmost bit, so these six values are that
/// behaviour recorded rather than a new one imposed. `background_fit` is the one
/// exception and it is a safe one: a v9 build had no fit at all, and this one is
/// unreachable until somebody names a picture.
#[test]
fn settings_v9_fixture_migrates_to_v10_with_the_ground_v9_drew_and_disturbs_nothing() {
    let (v10, report) = read_settings(&fixture_path("settings_v9_ground_absent.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(v10.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(
        v10.background_image, "",
        "a v9 build never asked for a picture, so the migration must not invent \
         one — and an empty name is also the state a file keeps while the drive \
         its wallpaper lives on is unplugged"
    );
    assert_eq!(v10.background_fit, BackgroundFitV1::Fill);
    assert_eq!(v10.background_image_opacity, 100);
    assert_eq!(
        v10.background_opacity, 100,
        "the one value here whose wrong answer would be visible on every \
         upgraded machine at once"
    );
    assert!(!v10.acrylic);
    assert!(!v10.always_on_top);
    assert_eq!(v10.theme_mode, ThemeModeV1::Dark);
    assert!(!v10.display_formulas);
    assert!(!v10.inline_formulas);
    assert_eq!(v10.default_profile, "cmd");
    assert!(!v10.git_panel);
    assert_eq!(v10.split_direction, SplitDirectionV1::Right);
    assert_eq!(v10.language, LanguageV1::English);
    assert_eq!(v10.terminal_font_family, "Lucida Console");
    assert_eq!(v10.terminal_font_size, 12);
    assert_eq!(v10.psreadline_invite, PsReadLineInviteV1::Dismissed);
    assert_eq!(v10.light_scheme, "One Half Light");
    assert_eq!(
        v10.dark_scheme, "Gruvbox Dark",
        "v9→v10 is structural: every sibling crosses untouched"
    );
}

/// PIN — a v10 file that describes a whole ground is loaded without migration
/// and hands every part of it back exactly as written.
///
/// The companion to the migration test above, pinning the opposite failure:
/// that one proves an unasked user is not answered for, this one proves an
/// answered user is not overruled. The path in the fixture carries a space and
/// Windows separators on purpose — a wallpaper lives wherever its owner keeps
/// their pictures, and this crate stores the name it was given rather than a
/// name it has tidied.
#[test]
fn a_v10_settings_file_that_describes_a_ground_keeps_every_part_of_it() {
    let (v10, report) = read_settings(&fixture_path("settings_v10_pictured_ground.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(v10.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(v10.background_image, r"D:\pictures\ridge line.jpg");
    assert_eq!(v10.background_fit, BackgroundFitV1::Tile);
    assert_eq!(v10.background_image_opacity, 45);
    assert_eq!(v10.background_opacity, 65);
    assert!(v10.acrylic);
    assert!(v10.always_on_top);
    assert_eq!(v10.light_scheme, "Solarized Light");
    assert_eq!(v10.dark_scheme, "Dracula");
    assert_eq!(v10.terminal_font_size, 22);

    // And back out again, byte-for-byte in meaning: the ground a user chose is
    // the ground the next launch reads.
    let dir = std::env::temp_dir().join(format!(
        "bt-persist-settings-v10-ground-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    write_settings_atomic(&path, &v10).unwrap();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.contains(r#""background_image": "D:\\pictures\\ridge line.jpg""#),
        "a picture is written as the path its owner gave, escaped for JSON and \
         not rewritten: {on_disk}"
    );
    assert!(
        on_disk.contains(r#""background_fit": "Tile""#)
            && on_disk.contains(r#""background_image_opacity": 45"#)
            && on_disk.contains(r#""background_opacity": 65"#),
        "a percentage is a whole number on the wire, never a float whose last \
         digit depends on which machine wrote it: {on_disk}"
    );
    let (round_tripped, report) = read_settings(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(round_tripped, v10);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// PIN (§7.1.6c-5) — a v10 file migrates to v11 with every Advanced group shut,
/// and a v11 file that names one keeps it.
///
/// Both halves in one test because they are the two ways one field can be got
/// wrong. The migration half pins that nothing is disclosed on the user's
/// behalf: a v10 build had no Advanced group at all, so the honest reading of
/// its file is "no page has been opened" — and a step that wrote
/// `["appearance"]` would greet every upgrading user with the eight rows the
/// ruling exists to fold away. The v11 half pins the opposite failure, and the
/// fixture carries a key this build has no page for on purpose: §5.4's
/// per-leaf degradation says an unknown page is dropped and every page beside
/// it still honoured, which is a claim about the reader in `bt-app` and, here,
/// a claim that this crate hands the list over as written rather than tidying
/// it.
#[test]
fn settings_v10_migrates_with_every_group_shut_and_v11_keeps_the_pages_it_names() {
    let (v11, report) = read_settings(&fixture_path("settings_v10_pictured_ground.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(v11.schema_version, SETTINGS_SCHEMA_VERSION);
    assert!(
        v11.advanced_open.is_empty(),
        "a v10 build had no disclosure, so the migration must not open one: {:?}",
        v11.advanced_open
    );
    assert_eq!(
        v11.background_image, r"D:\pictures\ridge line.jpg",
        "v10→v11 is structural: every sibling crosses untouched"
    );
    assert!(v11.acrylic && v11.always_on_top);
    assert_eq!(v11.terminal_font_size, 22);

    let (named, report) = read_settings(&fixture_path("settings_v11_advanced_open.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(named.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(
        named.advanced_open,
        vec![
            "appearance".to_owned(),
            "a-page-this-build-retired".to_owned()
        ],
        "this crate stores the keys it was given; deciding which of them name a          page is the reader's job"
    );

    // And back out again: the pages a reader opened are the pages the next
    // launch opens.
    let dir = std::env::temp_dir().join(format!(
        "bt-persist-settings-v11-advanced-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    write_settings_atomic(&path, &named).unwrap();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.contains(r#""advanced_open""#) && on_disk.contains(r#""appearance""#),
        "an open group is written as a list of page keys: {on_disk}"
    );
    let (round_tripped, report) = read_settings(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(round_tripped, named);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// PIN (the Tables switch) - a v11 file migrates to v12 with tables on, and a v12 file that turns
/// them off is read as turning them off.
///
/// The migration half pins that the step carries a behaviour forward rather than choosing a new
/// default. A v11 build drew no tables at all, so its file records no preference about them; what
/// it records is silence, and the honest reading of silence is the product's own answer, which is
/// the same `true` a fresh install gets. Writing `false` would be inventing a refusal nobody made.
/// The v12 half pins the opposite failure: a reader who *did* say no must be heard, through the
/// write and the read both.
#[test]
fn settings_v11_migrates_with_tables_on_and_v12_keeps_the_answer_it_was_given() {
    let (migrated, report) = read_settings(&fixture_path("settings_v11_advanced_open.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(migrated.schema_version, SETTINGS_SCHEMA_VERSION);
    assert!(
        migrated.tables,
        "a v11 file never answered this question, so the product answers it"
    );
    assert!(
        !migrated.display_formulas,
        "one key crosses; every sibling crosses untouched"
    );
    assert_eq!(migrated.terminal_font_size, 22);

    let (off, report) = read_settings(&fixture_path("settings_v12_tables_off.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(off.schema_version, SETTINGS_SCHEMA_VERSION);
    assert!(!off.tables, "a reader who said no is heard");

    let dir = std::env::temp_dir().join(format!(
        "bt-persist-settings-v12-tables-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    write_settings_atomic(&path, &off).unwrap();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.contains(r#""tables": false"#),
        "the switch is written as its own key: {on_disk}"
    );
    let (round_tripped, report) = read_settings(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(round_tripped, off);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// PIN (the Maximum height row) - a v12 file migrates to v13 with no cap at all, and a v13 file
/// naming a height is read as naming it.
///
/// The migration half pins the third running of the same shape: a v12 build drew every block at
/// its full height because it had no control that could say otherwise, so `0` is that behaviour
/// written down rather than a new default chosen on the reader's behalf. The second half pins
/// that the number survives the write and the read - and that it survives *as a number*, because
/// `0` is a legal value of this key and a serializer that dropped it, or a reader that read a
/// missing key as "no opinion", would silently uncap a reader who had capped.
#[test]
fn settings_v12_migrates_uncapped_and_v13_keeps_the_height_it_was_given() {
    let (migrated, report) = read_settings(&fixture_path("settings_v12_tables_off.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(migrated.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(
        migrated.block_max_height, 0,
        "a v12 file drew every block whole, and that is what it keeps doing"
    );
    assert!(
        !migrated.tables,
        "one key crosses; every sibling crosses untouched"
    );

    let (capped, report) = read_settings(&fixture_path("settings_v13_block_max_height.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(capped.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(capped.block_max_height, 240, "a reader who capped is heard");

    let dir = std::env::temp_dir().join(format!(
        "bt-persist-settings-v13-blockmax-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    write_settings_atomic(&path, &capped).unwrap();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.contains(r#""block_max_height": 240"#),
        "the height is written as its own key: {on_disk}"
    );
    let (round_tripped, report) = read_settings(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(round_tripped, capped);

    // And zero is a value, not an absence.
    let uncapped = SettingsV1 {
        block_max_height: 0,
        ..capped
    };
    write_settings_atomic(&path, &uncapped).unwrap();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.contains(r#""block_max_height": 0"#),
        "no limit is written down rather than left out: {on_disk}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// PIN (the Scrollback row) — a v13 file migrates to v14 keeping **the capacity every
/// build before it kept**, and a v14 file naming a smaller one is read as naming it.
///
/// The migration half is the fourth running of the same shape, and here the behaviour being
/// carried forward is a literal constant rather than an absence: every build up to v13 held
/// 100,000 frozen lines per pane because `M0_FROZEN_LINE_QUOTA` said so and nothing could say
/// otherwise. Writing a smaller number here would be shrinking a stranger's history on the
/// strength of a control they have not seen; writing a larger one would be spending their
/// memory the same way.
///
/// The second half pins that the number survives the write and the read as a number. There is
/// no `0` sentinel on this key — unlike `block_max_height`, "no limit" is a thing this product
/// has ruled it does not do (P2-9: 真·无限 = 输出必须写盘), so every value of this key is a
/// real capacity and a reader who chose one must get it back.
#[test]
fn settings_v13_migrates_to_the_capacity_it_always_had_and_v14_keeps_the_number_it_was_given() {
    let (migrated, report) = read_settings(&fixture_path("settings_v13_block_max_height.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(migrated.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(
        migrated.scrollback_lines, 100_000,
        "a v13 build kept a hundred thousand lines a pane, and that is what it keeps doing"
    );
    assert_eq!(
        migrated.block_max_height, 240,
        "one key crosses; every sibling crosses untouched"
    );

    let (chosen, report) = read_settings(&fixture_path("settings_v14_scrollback.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(chosen.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(
        chosen.scrollback_lines, 25_000,
        "a reader who asked for less is heard"
    );

    let dir = std::env::temp_dir().join(format!(
        "bt-persist-settings-v14-scrollback-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    write_settings_atomic(&path, &chosen).unwrap();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.contains(r#""scrollback_lines": 25000"#),
        "the capacity is written as its own key: {on_disk}"
    );
    let (round_tripped, report) = read_settings(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(round_tripped, chosen);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// PIN (§7.1.6b′, 2026-08-19) — a v14 settings file migrates to v15 with focus mode off, and a
/// v15 file that says the reader lives in it gets that answer back through a write and a read.
///
/// The first half is the whole of the migration's judgement. No build before v15 could be in
/// focus mode, so `false` is not a policy this step chooses but the behaviour it records; a
/// step that shipped the new mode *on* — tempting precisely because it is the reason the
/// version moved — would redecorate the window of every reader who upgrades without asking.
/// The sibling assertion is §1.3 rule 1: the fixture is non-default in its older fields, and a
/// copy-pasted step that resets one while inserting its own is the failure this shape catches.
///
/// The second half is what the row is *for*. Focus mode's other half lives on the window and
/// dies with it; this key is the reason a window closed with the card column up opens with it
/// up, so a `true` that did not survive the file would leave the Appearance row promising
/// something no restart delivers.
#[test]
fn settings_v14_migrates_with_focus_mode_off_and_v15_keeps_the_shape_it_was_left_in() {
    let (migrated, report) = read_settings(&fixture_path("settings_v14_scrollback.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(migrated.schema_version, SETTINGS_SCHEMA_VERSION);
    assert!(
        !migrated.focus_mode,
        "a v14 build had no focus mode to be in, and that is the shape it goes on opening in"
    );
    assert_eq!(
        migrated.scrollback_lines, 25_000,
        "one key crosses; every sibling crosses untouched"
    );
    assert_eq!(migrated.block_max_height, 480);

    let (chosen, report) = read_settings(&fixture_path("settings_v15_focus_mode.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(chosen.schema_version, SETTINGS_SCHEMA_VERSION);
    assert!(
        chosen.focus_mode,
        "a reader who lives in the card column is heard"
    );

    let dir = std::env::temp_dir().join(format!(
        "bt-persist-settings-v15-focus-mode-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    write_settings_atomic(&path, &chosen).unwrap();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.contains(r#""focus_mode": true"#),
        "the window's shape is written as its own key: {on_disk}"
    );
    let (round_tripped, report) = read_settings(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(round_tripped, chosen);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// PIN (DESIGN §2.6, 2026-08-19) — a v15 settings file migrates to v16 with the contrast floor
/// `Off`, and a v16 file that names a rung gets that rung back through a write and a read.
///
/// **The first half is the strongest case in this file for the rule every step here follows**,
/// and it is worth having its own test rather than an entry in a table. `focus_mode`'s
/// migration only had to avoid opening a different window; this one has to avoid **re-inking
/// output the reader has already read**. Every rung above `Off` overrides colours a program
/// asked for, in a scheme the reader deliberately chose, so a step that carried anything but
/// `Off` forward would repaint a stranger's terminal on the strength of a row they have not
/// seen. The sibling assertions are §1.3 rule 1 again: the fixture is non-default in its older
/// fields, and a copy-pasted step that resets one while inserting its own is what this shape
/// catches.
///
/// The second half is what the row is *for*: the floor has to survive the file, or the picker
/// is a verb somebody re-presses every morning.
#[test]
fn settings_v15_migrates_with_the_contrast_floor_off_and_v16_keeps_the_rung_it_was_given() {
    let (migrated, report) = read_settings(&fixture_path("settings_v15_focus_mode.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(migrated.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(
        migrated.minimum_contrast,
        MinimumContrastV1::Off,
        "a v15 build drew every colour as the program named it, and that is what it goes on doing"
    );
    assert!(
        migrated.focus_mode,
        "one key crosses; every sibling crosses untouched"
    );
    assert_eq!(migrated.scrollback_lines, 25_000);
    assert_eq!(migrated.dark_scheme, "Nord");

    let (chosen, report) = read_settings(&fixture_path("settings_v16_minimum_contrast.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(chosen.schema_version, SETTINGS_SCHEMA_VERSION);
    assert_eq!(
        chosen.minimum_contrast,
        MinimumContrastV1::Ratio45,
        "a reader who asked for the WCAG AA bar is heard"
    );

    let dir = std::env::temp_dir().join(format!(
        "bt-persist-settings-v16-minimum-contrast-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    write_settings_atomic(&path, &chosen).unwrap();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.contains(r#""minimum_contrast": "Ratio45""#),
        "the floor is written as its own key, PascalCase like every other enum here: {on_disk}"
    );
    let (round_tripped, report) = read_settings(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(round_tripped, chosen);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// PIN (§7.6, 2026-08-20) — a v16 settings file migrates to v17 with notifications **on**, and a
/// v17 file that says no gets that answer back through a write and a read.
///
/// The first half is this step's judgement, and it is the opposite of the step before it for a
/// reason worth having in the suite rather than only in a comment: `v14→v15` writes `false`
/// because focus mode replaces the window somebody opens every morning, and this one writes
/// `true` because nothing here changes until a program asks. Two migrations that look identical
/// in shape and land opposite ways are exactly what a copy-paste gets wrong.
///
/// The second half is the switch being a switch. A `false` that did not survive the file would
/// be a row that promises silence and delivers it until the next launch — the worst way for a
/// notification setting to fail, because the reader finds out by being interrupted.
#[test]
fn settings_v16_migrates_with_notifications_on_and_v17_keeps_the_silence_it_was_asked_for() {
    let (migrated, report) = read_settings(&fixture_path("settings_v16_minimum_contrast.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(migrated.schema_version, SETTINGS_SCHEMA_VERSION);
    assert!(
        migrated.terminal_notifications,
        "a v16 build could raise no notification at all, so `false` would freeze an absence          rather than preserve a choice — the feature takes the product's default"
    );
    assert_eq!(
        migrated.minimum_contrast,
        MinimumContrastV1::Ratio45,
        "one key crosses; every sibling crosses untouched"
    );

    let (silent, report) = read_settings(&fixture_path("settings_v17_notifications_off.json"));
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(silent.schema_version, SETTINGS_SCHEMA_VERSION);
    assert!(
        !silent.terminal_notifications,
        "a reader who asked for silence is heard"
    );

    let dir = std::env::temp_dir().join(format!(
        "bt-persist-settings-v17-notifications-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    write_settings_atomic(&path, &silent).unwrap();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.contains(r#""terminal_notifications": false"#),
        "the answer is written as its own key: {on_disk}"
    );
    let (round_tripped, report) = read_settings(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(round_tripped, silent);
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
