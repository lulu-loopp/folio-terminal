//! Crash-vs-clean-exit sentinel — docs/M2-persistence-schema-v1.md §5.5.
//!
//! "启动时在存储目录创建一个哨兵文件…正常退出时删除它。下次启动若发现哨兵文件
//! 残留,判定上次是崩溃". The system-shutdown path is explicitly folded into
//! "clean exit" by the caller (via `WM_QUERYENDSESSION`/`WM_ENDSESSION`
//! running the same cleanup as a normal quit, per §5.5) — this crate has no
//! Windows message-loop dependency and does not need to know about that; it
//! only provides the three primitives named in the implementation brief.
//! Callers are expected to sequence them as: `probe` at startup (before
//! touching the sentinel further) to learn last session's fate, then
//! `create` for this session, then `remove` on clean exit.

use std::fs::File;
use std::io;
use std::path::Path;

/// Result of [`probe_sentinel`]: whether the previous run's sentinel file
/// was still present at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitState {
    /// No sentinel was found — the previous run (if any) removed it on a
    /// clean exit, or this is the first run ever.
    Normal,
    /// A sentinel from a previous run was still present — it was never
    /// removed, so that run did not reach its clean-exit path.
    Crashed,
}

/// Read-only check of whether a sentinel file is present, without creating
/// or removing anything. Call this **before** [`create_sentinel`] at
/// startup — creating first would make every subsequent probe report
/// `Crashed`.
pub fn probe_sentinel(path: &Path) -> io::Result<ExitState> {
    match path.try_exists()? {
        true => Ok(ExitState::Crashed),
        false => Ok(ExitState::Normal),
    }
}

/// Creates (or truncates, if one somehow already exists) the sentinel file
/// for this session. Content is irrelevant — presence is the entire signal
/// (§5.5: "内容不重要,存在性即信号").
pub fn create_sentinel(path: &Path) -> io::Result<()> {
    File::create(path)?;
    Ok(())
}

/// Removes the sentinel file on a clean exit. Removing an already-absent
/// sentinel is not an error — it means the previous step already happened,
/// or nothing was ever created, either of which is a fine end state.
pub fn remove_sentinel(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_dir() -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("bt-persist-sentinel-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn first_run_ever_probes_normal() {
        let dir = unique_dir();
        let sentinel = dir.join("session.lock");
        assert_eq!(probe_sentinel(&sentinel).unwrap(), ExitState::Normal);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn leftover_sentinel_probes_crashed() {
        let dir = unique_dir();
        let sentinel = dir.join("session.lock");
        create_sentinel(&sentinel).unwrap();
        // Simulate the next launch, without removing the sentinel first.
        assert_eq!(probe_sentinel(&sentinel).unwrap(), ExitState::Crashed);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn clean_exit_then_restart_probes_normal_again() {
        let dir = unique_dir();
        let sentinel = dir.join("session.lock");
        create_sentinel(&sentinel).unwrap();
        remove_sentinel(&sentinel).unwrap();
        assert_eq!(probe_sentinel(&sentinel).unwrap(), ExitState::Normal);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn removing_an_absent_sentinel_is_not_an_error() {
        let dir = unique_dir();
        let sentinel = dir.join("session.lock");
        remove_sentinel(&sentinel).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn full_lifecycle_two_sessions_one_crash_one_clean() {
        let dir = unique_dir();
        let sentinel = dir.join("session.lock");

        // Session 1: starts fresh, crashes (sentinel never removed).
        assert_eq!(probe_sentinel(&sentinel).unwrap(), ExitState::Normal);
        create_sentinel(&sentinel).unwrap();
        // (crash — no remove_sentinel call)

        // Session 2: starts, sees the crash, exits cleanly this time.
        assert_eq!(probe_sentinel(&sentinel).unwrap(), ExitState::Crashed);
        create_sentinel(&sentinel).unwrap();
        remove_sentinel(&sentinel).unwrap();

        // Session 3: starts, sees the clean exit.
        assert_eq!(probe_sentinel(&sentinel).unwrap(), ExitState::Normal);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
