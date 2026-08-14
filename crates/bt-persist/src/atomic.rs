//! Atomic file replacement — docs/M2-persistence-schema-v1.md §5.2.
//!
//! "统一模式:在目标文件同目录下写一个随机后缀的临时文件、`fsync`、再用平台原子
//! 替换…覆盖到最终文件名。**同目录**是必要条件——跨卷的『写临时文件再 rename』
//! 不是原子操作".
//!
//! `std::fs::rename` on Windows already goes through `MoveFileExW` with
//! `MOVEFILE_REPLACE_EXISTING` when the destination exists, which is the
//! platform atomic replace the spec asks for — no `unsafe` FFI needed here,
//! keeping this crate outside the workspace's one deliberate `unsafe`
//! boundary (`bt-platform`, per `CONVENTIONS.md` §零).

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::WriteError;

/// Writes `contents` to `path` via the temp-file-same-dir-then-rename
/// pattern. `path`'s original content is untouched until the final rename
/// succeeds — a failure at any earlier step (temp file creation, write,
/// fsync) leaves `path` exactly as it was.
pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), WriteError> {
    let tmp_path = temp_sibling_path(path)?;
    write_temp(&tmp_path, contents).map_err(|source| WriteError::Io {
        path: tmp_path.clone(),
        source,
    })?;
    commit_rename(&tmp_path, path)
}

/// Step 1: write `contents` into `tmp_path` and `fsync` it. Does not touch
/// the eventual target file at all — this is the step that can fail without
/// endangering any existing file.
fn write_temp(tmp_path: &Path, contents: &[u8]) -> io::Result<()> {
    let result = (|| -> io::Result<()> {
        let mut file = File::create(tmp_path)?;
        file.write_all(contents)?;
        file.sync_all()
    })();
    if result.is_err() {
        // Best-effort: don't let a half-written temp file linger. If this
        // also fails there is nothing more we can do without risking the
        // real target file, so it is deliberately ignored.
        let _ = fs::remove_file(tmp_path);
    }
    result
}

/// Step 2: atomically replace `target` with the already-written `tmp_path`.
/// This is the only step that touches `target`'s directory entry, and it is
/// a single filesystem operation (`MoveFileExW`/`MOVEFILE_REPLACE_EXISTING`
/// on Windows, `rename(2)` on Unix) — there is no window in which `target`
/// is observably partial.
fn commit_rename(tmp_path: &Path, target: &Path) -> Result<(), WriteError> {
    fs::rename(tmp_path, target).map_err(|source| WriteError::Io {
        path: target.to_path_buf(),
        source,
    })
}

fn temp_sibling_path(path: &Path) -> Result<PathBuf, WriteError> {
    let file_name = path.file_name().ok_or_else(|| WriteError::Io {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"),
    })?;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut name = file_name.to_os_string();
    name.push(format!(".tmp-{}", unique_suffix()));
    Ok(dir.join(name))
}

/// Unique-enough-within-this-process suffix for temp file names: a
/// monotonic per-process counter (guarantees uniqueness across calls in the
/// same process even if the clock doesn't advance) combined with wall-clock
/// nanoseconds and the process ID (guards against collisions with another
/// Folio process writing the same directory). Not cryptographic
/// randomness — collision avoidance, not secrecy, is the requirement.
fn unique_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    format!("{pid:x}-{nanos:x}-{n:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_replaces_content_and_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join(format!("bt-persist-atomic-{}", unique_suffix()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("session.json");

        atomic_write(&target, b"first").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"first");

        atomic_write(&target, b"second").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"second");

        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|name| name != "session.json")
            .collect();
        assert!(
            leftovers.is_empty(),
            "no temp files should survive a successful write: {leftovers:?}"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    /// Red-check: atomicity. Simulates a crash between "temp file written"
    /// and "rename committed" by calling the two internal phases separately
    /// and stopping after phase 1 — exactly the crash window the spec's
    /// temp-then-rename pattern exists to make safe. The original file at
    /// `target` must be byte-for-byte untouched at that point; only the
    /// later, successful `commit_rename` may change it.
    #[test]
    fn interrupted_write_leaves_old_file_intact() {
        let dir = std::env::temp_dir().join(format!("bt-persist-atomic-crash-{}", unique_suffix()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("session.json");
        fs::write(&target, b"OLD-CONTENT").unwrap();

        // Phase 1 only: write (and fsync) the temp file. Do NOT rename —
        // this is the "process died right here" point.
        let tmp_path = temp_sibling_path(&target).unwrap();
        write_temp(&tmp_path, b"NEW-CONTENT").unwrap();

        // Assert the crash-window invariant: target is exactly the old bytes.
        assert_eq!(
            fs::read(&target).unwrap(),
            b"OLD-CONTENT",
            "target must be untouched while only the temp file has been written"
        );

        // Now "recover" by completing the commit, and verify it takes effect.
        commit_rename(&tmp_path, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"NEW-CONTENT");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_failure_before_rename_never_touches_target() {
        let dir = std::env::temp_dir().join(format!("bt-persist-atomic-fail-{}", unique_suffix()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("session.json");
        fs::write(&target, b"OLD-CONTENT").unwrap();

        // Force phase 1 to fail: point the "temp file" at a path that is
        // actually an existing directory, so `File::create` cannot succeed.
        let bogus_tmp = dir.join("this-is-a-directory");
        fs::create_dir(&bogus_tmp).unwrap();
        let result = write_temp(&bogus_tmp, b"NEW-CONTENT");
        assert!(
            result.is_err(),
            "creating a file where a directory exists must fail"
        );

        assert_eq!(
            fs::read(&target).unwrap(),
            b"OLD-CONTENT",
            "target must be untouched by a failed temp write"
        );

        fs::remove_dir_all(&dir).unwrap();
    }
}
