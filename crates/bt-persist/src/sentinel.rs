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

use std::fs::OpenOptions;
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

/// Creates the sentinel file for this session. Content is irrelevant —
/// presence is the entire signal (§5.5: "内容不重要,存在性即信号").
///
/// **It creates, and it never truncates** (review row R4-8). This used to be
/// `File::create`, which opens the name with `TRUNCATE_EXISTING` and follows
/// whatever the name resolves to — so a hard link or a symbolic link planted
/// at `session.lock` emptied the file it pointed at, on every launch,
/// silently, without ever needing the sentinel to be read. Since presence is
/// the whole of the signal there was never anything to write, which makes the
/// truncation pure cost.
///
/// So the name is inspected before it is opened, with
/// [`std::fs::symlink_metadata`] — the one query that reports the link itself
/// rather than what it points at:
///
/// * a **link** is refused outright. Nothing this crate wrote is a link, so
///   one standing at this name is somebody else's doing, and a launch that
///   quietly opened it would be acting on that somebody's behalf. The refusal
///   costs this run its clean-exit claim (the caller leaves the sentinel
///   unarmed) and costs nothing else;
/// * a **directory** is refused for the same reason and with the same cost;
/// * an ordinary **file already there** is a sentinel a previous run left
///   behind — the crash case this whole module exists to detect — and it is
///   left exactly as it is. Presence is the signal and it is already present;
/// * **nothing there** is created with `create_new`, so a second process that
///   reached the name first keeps its file rather than having it emptied.
pub fn create_sentinel(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the sentinel name is a link, and this is not a name anything may follow",
            ));
        }
        Ok(metadata) if metadata.is_dir() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the sentinel name is a directory",
            ));
        }
        // Already standing: the previous run's, or this run's own second call.
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(_) => Ok(()),
        // Somebody created it between the query above and this line. Presence
        // is the signal, and it is present.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
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

    /// RED (review row R4-8) — **creating the sentinel empties nothing.**
    ///
    /// A hard link rather than a symbolic one, and not for convenience: a hard
    /// link needs no privilege on Windows, so it is the version of this an
    /// ordinary account can plant, and it is invisible to every check that asks
    /// what a path *is* — the name is a second directory entry for one file.
    /// `File::create` opens it with `TRUNCATE_EXISTING` and follows it, so the
    /// file it names is emptied on every launch, with nothing written in its
    /// place because presence is the whole of the signal.
    ///
    /// Red gate: put `File::create(path)?` back and `victim` reads empty.
    #[test]
    fn a_link_planted_at_the_sentinel_name_does_not_empty_what_it_names() {
        let dir = unique_dir();
        let victim = dir.join("something-of-mine.txt");
        std::fs::write(&victim, b"CONTENT-THAT-MUST-SURVIVE").unwrap();
        let sentinel = dir.join("session.lock");
        std::fs::hard_link(&victim, &sentinel).expect("a hard link needs no privilege");

        // The launch does what it always does.
        create_sentinel(&sentinel).unwrap();

        assert_eq!(
            std::fs::read(&victim).unwrap(),
            b"CONTENT-THAT-MUST-SURVIVE",
            "the file the sentinel name pointed at must not be emptied"
        );
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
