//! §5.4 failure-path red-checks against the public API (`read_settings`/
//! `read_session`), plus §5.4-case-3 per-leaf degradation with real illegal
//! data (not just the generic fixture type `migrate.rs`'s unit tests use).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use bt_persist::{
    FallbackReason, LayoutNodeV1, LeafNodeV1, ReadReport, SESSION_SCHEMA_VERSION,
    SETTINGS_SCHEMA_VERSION, ThemeModeV1, read_session, read_settings,
};

fn unique_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bt-persist-failure-paths-{tag}-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn missing_settings_file_defaults_to_system_theme_without_alerting() {
    let dir = unique_dir("settings-missing");
    let (settings, report) = read_settings(&dir.join("settings.json"));
    assert_eq!(report, ReadReport::NotFound);
    assert_eq!(settings.theme_mode, ThemeModeV1::System);
    assert_eq!(settings.schema_version, SETTINGS_SCHEMA_VERSION);
}

#[test]
fn corrupt_settings_json_falls_back_and_leaves_the_bad_file_alone() {
    let dir = unique_dir("settings-corrupt");
    let path = dir.join("settings.json");
    std::fs::write(&path, "{ this is not json").unwrap();

    let (settings, report) = read_settings(&path);
    assert_eq!(settings.theme_mode, ThemeModeV1::System);
    assert!(matches!(
        report,
        ReadReport::FellBackToDefaults {
            reason: FallbackReason::ParseError(_)
        }
    ));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "{ this is not json",
        "§5.4: original file must not be overwritten or deleted by a read"
    );
}

#[test]
fn settings_from_a_future_schema_version_refuses_and_does_not_partially_parse() {
    let dir = unique_dir("settings-future");
    let path = dir.join("settings.json");
    // A hypothetical future settings.json with a field this build has never
    // heard of, at a schema_version this build cannot understand.
    std::fs::write(
        &path,
        r##"{"schema_version": 7, "theme_mode": "Dark", "accent_color": "#ff00ff"}"##,
    )
    .unwrap();

    let (settings, report) = read_settings(&path);
    // Must fall all the way back to defaults, not adopt theme_mode="Dark"
    // just because that one field happens to still parse — §1.3 rule 2:
    // "不尝试部分解析".
    assert_eq!(settings.theme_mode, ThemeModeV1::System);
    assert_eq!(
        report,
        ReadReport::FellBackToDefaults {
            reason: FallbackReason::FutureSchemaVersion {
                found: 7,
                current: SETTINGS_SCHEMA_VERSION
            }
        }
    );
}

#[test]
fn session_from_a_future_schema_version_refuses_and_does_not_partially_parse() {
    let dir = unique_dir("session-future");
    let path = dir.join("session.json");
    std::fs::write(
        &path,
        r#"{"schema_version": 999, "active_tab": 3, "future_field": "opaque"}"#,
    )
    .unwrap();

    let (session, report, degradation) = read_session(&path);
    assert!(
        session.tabs.is_empty(),
        "must be the full default, not a partially-populated value"
    );
    assert_eq!(
        session.active_tab, 0,
        "the future file's active_tab must NOT leak through"
    );
    assert!(degradation.is_clean());
    assert_eq!(
        report,
        ReadReport::FellBackToDefaults {
            reason: FallbackReason::FutureSchemaVersion {
                found: 999,
                current: SESSION_SCHEMA_VERSION
            }
        }
    );
}

#[test]
fn illegal_ratio_and_unknown_leaf_kind_degrade_without_losing_the_rest_of_the_tree() {
    let dir = unique_dir("session-degrade");
    let path = dir.join("session.json");
    // A split with a ratio far outside the valid fraction range, next to a
    // leaf kind this build has never heard of (as if written by a future
    // Folio) — §5.4 case 3: neither should take down the whole tab.
    std::fs::write(
        &path,
        r#"{
            "schema_version": 1,
            "window": {"bounds": {"x": 0, "y": 0, "width": 800, "height": 600}, "dpi": 96, "maximized": false, "monitor_id": null},
            "tabs": [
                {
                    "root": {
                        "dir": "row",
                        "ratio": 5000000,
                        "children": [
                            {"kind": "term", "profile_id": "pwsh.exe", "cwd": "C:\\Users\\dev", "manual_name": null},
                            {"kind": "quake_overlay", "some_future_field": true}
                        ]
                    },
                    "pinned": false,
                    "focused_leaf": "leaf-0"
                }
            ],
            "active_tab": 0,
            "recent": []
        }"#,
    )
    .unwrap();

    let (session, report, degradation) = read_session(&path);
    assert_eq!(
        report,
        ReadReport::Loaded,
        "structurally valid JSON must still load, even with bad invariants"
    );
    assert_eq!(degradation.clamped_ratios, 1);
    assert_eq!(degradation.unknown_leaves, 1);

    let LayoutNodeV1::Split(split) = &session.tabs[0].root else {
        panic!("tree shape must survive degradation");
    };
    assert_eq!(
        split.ratio,
        bt_persist::RATIO_PPM_MAX,
        "out-of-range ratio must be clamped, not rejected"
    );
    assert!(
        matches!(
            split.children[0].as_ref(),
            LayoutNodeV1::Leaf(LeafNodeV1::Term(_))
        ),
        "sibling leaf must be unaffected"
    );
    assert!(
        matches!(
            split.children[1].as_ref(),
            LayoutNodeV1::Leaf(LeafNodeV1::Unknown)
        ),
        "unrecognized kind must become a placeholder, not fail the whole tab"
    );
}

#[test]
fn invalid_cwd_is_passed_through_unchanged_fs_checks_are_not_this_crates_job() {
    let dir = unique_dir("session-cwd");
    let path = dir.join("session.json");
    std::fs::write(
        &path,
        r#"{
            "schema_version": 1,
            "window": {"bounds": {"x": 0, "y": 0, "width": 800, "height": 600}, "dpi": 96, "maximized": false, "monitor_id": null},
            "tabs": [
                {
                    "root": {"kind": "term", "profile_id": "pwsh.exe", "cwd": "Z:\\this\\drive\\does\\not\\exist", "manual_name": null},
                    "pinned": false,
                    "focused_leaf": "leaf-0"
                }
            ],
            "active_tab": 0,
            "recent": []
        }"#,
    )
    .unwrap();

    let (session, report, degradation) = read_session(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert!(
        degradation.is_clean(),
        "cwd validity is not this crate's concern, so nothing should be flagged"
    );
    let LayoutNodeV1::Leaf(LeafNodeV1::Term(term)) = &session.tabs[0].root else {
        panic!("expected a term leaf");
    };
    assert_eq!(
        term.cwd, r"Z:\this\drive\does\not\exist",
        "cwd must be passed through verbatim"
    );
}
