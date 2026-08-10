//! End-to-end exercise of the rest of the public API surface that
//! `round_trip.rs`/`failure_paths.rs` don't already cover: settings
//! write-then-read, the sentinel lifecycle, and the debounce/write-failure
//! helpers composed together the way a real call site would.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bt_persist::{
    Debouncer, ExitState, ReadReport, SETTINGS_SCHEMA_VERSION, SettingsV1, ThemeModeV1,
    WriteAlertAction, WriteFailureTracker, create_sentinel, probe_sentinel, read_settings,
    remove_sentinel, write_settings_atomic,
};

fn unique_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("bt-persist-smoke-{tag}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn settings_write_then_read_round_trips_a_non_default_value() {
    let dir = unique_dir("settings");
    let path = dir.join("settings.json");

    // Every field carries a non-default value, so a serializer that dropped one
    // could not hide behind a matching default on the way back in.
    let settings = SettingsV1 {
        schema_version: SETTINGS_SCHEMA_VERSION,
        theme_mode: ThemeModeV1::Dark,
        display_formulas: false,
        inline_formulas: false,
    };
    write_settings_atomic(&path, &settings).unwrap();

    let (loaded, report) = read_settings(&path);
    assert_eq!(report, ReadReport::Loaded);
    assert_eq!(loaded, settings);

    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.contains("\"theme_mode\": \"Dark\""),
        "written file must be human-readable JSON: {on_disk}"
    );
    assert!(
        on_disk.contains("\"display_formulas\": false"),
        "written file must be human-readable JSON: {on_disk}"
    );
    assert!(
        on_disk.contains("\"inline_formulas\": false"),
        "written file must be human-readable JSON: {on_disk}"
    );
}

#[test]
fn sentinel_lifecycle_across_three_simulated_launches() {
    let dir = unique_dir("sentinel");
    let sentinel = dir.join("session.lock");

    // Launch 1: fresh machine.
    assert_eq!(probe_sentinel(&sentinel).unwrap(), ExitState::Normal);
    create_sentinel(&sentinel).unwrap();
    // ... process "crashes" here, sentinel never removed.

    // Launch 2: sees the crash.
    assert_eq!(probe_sentinel(&sentinel).unwrap(), ExitState::Crashed);
    create_sentinel(&sentinel).unwrap();
    remove_sentinel(&sentinel).unwrap(); // clean exit this time

    // Launch 3: sees the clean exit.
    assert_eq!(probe_sentinel(&sentinel).unwrap(), ExitState::Normal);
}

/// Simulates the intended call-site composition: mark dirty on changes,
/// flush once the debounce window elapses, and only alert once per failure
/// streak — all driven by an explicit fake clock, no real sleeping.
#[test]
fn debounce_and_write_failure_tracker_compose_the_way_a_call_site_would() {
    let dir = unique_dir("debounce-compose");
    let path = dir.join("session.json");
    let debounce_window = Duration::from_millis(1500);

    let mut debouncer = Debouncer::new();
    let mut tracker = WriteFailureTracker::new();
    let mut alerts = 0u32;

    let t0 = Instant::now();
    debouncer.mark_dirty(t0); // e.g. user resized a pane

    // Not enough time has passed yet — nothing to do.
    assert!(!debouncer.should_flush(t0 + Duration::from_millis(200), debounce_window));

    // Debounce window elapses: flush.
    let flush_time = t0 + debounce_window;
    assert!(debouncer.should_flush(flush_time, debounce_window));
    let settings = bt_persist::SettingsV1::default();
    let write_result = write_settings_atomic(&path, &settings);
    if tracker.record(write_result.is_ok()) == WriteAlertAction::AlertOnce {
        alerts += 1;
    }
    debouncer.mark_flushed();
    assert!(write_result.is_ok());
    assert_eq!(alerts, 0, "a successful write must never alert");

    // A later change, followed by a real write failure (the target's parent
    // directory doesn't exist, so `atomic_write` cannot create the temp
    // file there) — three consecutive failed flushes must alert exactly
    // once, not three times.
    let bogus_path = dir.join("does-not-exist").join("session.json"); // parent was never created
    for i in 0..3u32 {
        let t = flush_time + Duration::from_secs(u64::from(i) + 1) * 2;
        debouncer.mark_dirty(t);
        assert!(debouncer.should_flush(t, Duration::from_secs(0)));
        let result = write_settings_atomic(&bogus_path, &settings);
        assert!(
            result.is_err(),
            "writing under a nonexistent directory must fail"
        );
        if tracker.record(false) == WriteAlertAction::AlertOnce {
            alerts += 1;
        }
        debouncer.mark_flushed();
    }
    assert_eq!(
        alerts, 1,
        "three failures in the same streak must alert exactly once"
    );
}
