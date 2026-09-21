//! Atomic file replacement — docs/M2-persistence-schema-v1.md §5.2.
//!
//! "统一模式:在目标文件同目录下写一个随机后缀的临时文件、`fsync`、再用平台原子
//! 替换…覆盖到最终文件名。**同目录**是必要条件——跨卷的『写临时文件再 rename』
//! 不是原子操作".
//!
//! `std::fs::rename` on Windows already goes through `MoveFileExW` with
//! `MOVEFILE_REPLACE_EXISTING` when the destination exists, which is the
//! platform atomic replace the spec asks for — no `unsafe` FFI needed here,
//! The preserving variant delegates native calls to `bt-platform`,
//! keeping this crate outside the workspace's one deliberate `unsafe`
//! boundary (`bt-platform`, per `docs/CONVENTIONS.md` §零).

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

/// Replace an existing user-owned file without discarding its metadata.
/// Windows merges DACLs, streams and creation time through ReplaceFileW;
/// Unix copies ownership and mode before rename. Multiple links are refused.
/// New files should use `atomic_write` instead.
pub fn atomic_replace_preserving(path: &Path, contents: &[u8]) -> Result<(), WriteError> {
    let tmp_path = temp_sibling_path(path)?;
    let recovery = temp_sibling_path(path)?;
    let result = (|| -> io::Result<()> {
        let file = File::open(path)?;
        if bt_platform::file_link_count(&file)? > 1 {
            return Err(io::Error::other("hard-linked files are not replaced"));
        }
        drop(file);
        write_temp(&tmp_path, contents)?;
        bt_platform::replace_file_preserving(&tmp_path, path, &recovery)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result.map_err(|source| WriteError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// **A save replaces a file's content and nothing else the file carried**
/// (audit 3 F-1, 2026-09-20). The file on the other end of this call is the
/// reader's own document, not one of ours: it can carry alternate data streams
/// (`Zone.Identifier` among them), an explicit DACL, hidden and system
/// attributes, a mode, an owner and extended attributes — quarantine and Finder
/// tags on macOS — and none of that belongs to the bytes being replaced.
///
/// Which replacement a file can take is decided by what the file is, and the
/// rule is *carry as much as this file allows*, never *refuse the save*:
///
/// * One name, not a symlink, and openable → [`atomic_replace_preserving`],
///   which on Windows is `ReplaceFileW` (streams, DACL, creation time, object
///   id, compression, encryption) and on Unix carries ownership, mode and every
///   extended attribute onto the replacement before the rename.
/// * More than one name — a hard link — → the writer this product has always
///   used, carrying what a rename can carry. **Breaking the other name is
///   today's behaviour and this is not the ticket that changes it**: the
///   preserving replacement refuses a hard-linked target, and a refusal would
///   turn a working save into a failed one for anybody whose notes are linked
///   into a dotfiles tree. Unruled, and named as such.
/// * A symlink → the same writer, for the same reason: a rename replaces the
///   link rather than what it points at, which is the gap `docs/DESIGN.md`
///   7.1.3v ④ already names, and `ReplaceFileW` has no documented answer for a
///   reparse point that could be relied on without a Windows to try it on.
/// * A file this process cannot even open → the same writer. A replacement
///   whose metadata cannot be read is one that cannot be carried, and a save
///   that used to land must not start failing because of what it could not copy.
pub fn atomic_replace_keeping_metadata(path: &Path, contents: &[u8]) -> Result<(), WriteError> {
    if can_be_preserved(path) {
        atomic_replace_preserving(path, contents)
    } else {
        atomic_write_carrying(path, contents)
    }
}

/// Can this file take the preserving replacement? One name, and its own name is
/// not a link to somebody else's file. Anything this cannot establish answers
/// no, because what it cannot establish it cannot preserve.
fn can_be_preserved(path: &Path) -> bool {
    let single_link = File::open(path)
        .and_then(|file| bt_platform::file_link_count(&file))
        .is_ok_and(|links| links == 1);
    let own_name = fs::symlink_metadata(path).is_ok_and(|metadata| !metadata.is_symlink());
    single_link && own_name
}

/// [`atomic_write`] plus everything a rename *can* bring across: the attribute
/// word on Windows, ownership, mode and extended attributes on Unix. The
/// carrying is best effort — what fails here is exactly what this writer has
/// always lost, and a document must not become unsaveable over it.
fn atomic_write_carrying(path: &Path, contents: &[u8]) -> Result<(), WriteError> {
    let tmp_path = temp_sibling_path(path)?;
    write_temp(&tmp_path, contents).map_err(|source| WriteError::Io {
        path: tmp_path.clone(),
        source,
    })?;
    let _ = bt_platform::carry_metadata(&tmp_path, path);
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
///
/// **A rename that fails takes its temp file with it** (review row R4-11).
/// The failure modes are all of the "this write is not going to work" family
/// — the target is a directory, the volume is full, the file is held open by
/// something that refuses a replace — and every one of them is retried by
/// the caller on its own clock. Until this line existed, each of those
/// retries left one more `session.json.tmp-…` in `%APPDATA%\Folio\`, so a
/// disk that had filled up was answered by filling it further, once every
/// debounce, for as long as the window stayed open. The removal is
/// best-effort for [`write_temp`]'s reason: if it also fails there is
/// nothing further to try that does not risk the real file, and the error
/// the caller is told about is the rename's, which is the one worth reading.
fn commit_rename(tmp_path: &Path, target: &Path) -> Result<(), WriteError> {
    match fs::rename(tmp_path, target) {
        Ok(()) => Ok(()),
        Err(source) => {
            let _ = fs::remove_file(tmp_path);
            Err(WriteError::Io {
                path: target.to_path_buf(),
                source,
            })
        }
    }
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
    fn preserving_replace_writes_content_and_refuses_hardlinks() {
        let dir = std::env::temp_dir().join(format!("bt-preserving-{}", unique_suffix()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("profile.ps1");
        fs::write(&target, b"before").unwrap();
        atomic_replace_preserving(&target, b"after").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"after");
        let linked = dir.join("dotfile.ps1");
        fs::hard_link(&target, &linked).unwrap();
        assert!(atomic_replace_preserving(&target, b"must not land").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"after");
        assert_eq!(fs::read(&linked).unwrap(), b"after");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        fs::remove_dir_all(dir).unwrap();
    }

    /// RED (audit 3, F-1) — **a hard-linked document still saves.**
    ///
    /// [`atomic_replace_preserving`] refuses a target with a second name, and
    /// the editor's save must not inherit that refusal: nobody has ruled that a
    /// linked file is unsaveable, and today's behaviour — the save lands and the
    /// other name silently keeps the old bytes — is the behaviour this pins
    /// until somebody does rule. What *is* new on this arm is that the metadata
    /// a rename can carry is carried.
    ///
    /// Red gate: route [`atomic_replace_keeping_metadata`] straight at
    /// [`atomic_replace_preserving`] and the first assertion is an `Err`.
    #[test]
    fn a_hard_linked_target_saves_the_way_it_always_has() {
        let dir = std::env::temp_dir().join(format!("bt-keeping-linked-{}", unique_suffix()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("notes.md");
        fs::write(&target, b"before").unwrap();
        let linked = dir.join("dotfiles-notes.md");
        fs::hard_link(&target, &linked).unwrap();

        atomic_replace_keeping_metadata(&target, b"after").unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"after", "the save landed");
        assert_eq!(
            fs::read(&linked).unwrap(),
            b"before",
            "and the other name kept the bytes it had — the link is broken, as it always was"
        );
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            2,
            "no staging or recovery file survives the write"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    /// The ordinary file — one name, not a link — takes the preserving arm, and
    /// the arm leaves nothing beside the file it replaced.
    #[test]
    fn a_single_named_file_takes_the_preserving_arm_and_leaves_nothing_beside_it() {
        let dir = std::env::temp_dir().join(format!("bt-keeping-plain-{}", unique_suffix()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("notes.md");
        fs::write(&target, b"before").unwrap();
        assert!(can_be_preserved(&target), "one name, and it is its own");

        atomic_replace_keeping_metadata(&target, b"after").unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"after");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "one entry, and it is the file that was saved"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn preserving_replace_missing_target_is_refused_without_temps() {
        let dir = std::env::temp_dir().join(format!("bt-preserving-missing-{}", unique_suffix()));
        fs::create_dir_all(&dir).unwrap();
        assert!(atomic_replace_preserving(&dir.join("missing"), b"no").is_err());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn preserving_replace_keeps_mode_and_ownership() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = std::env::temp_dir().join(format!("bt-preserving-mode-{}", unique_suffix()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("profile.ps1");
        fs::write(&target, b"before").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        let before = fs::metadata(&target).unwrap();
        atomic_replace_preserving(&target, b"after").unwrap();
        let after = fs::metadata(&target).unwrap();
        assert_eq!(
            (after.uid(), after.gid(), after.mode()),
            (before.uid(), before.gid(), before.mode())
        );
        fs::remove_dir_all(dir).unwrap();
    }

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

    /// RED (review row R4-11) — **a rename that could not happen leaves nothing
    /// behind.**
    ///
    /// The caller of this function is a debounce that retries, so a temp file
    /// that survives one failure survives every one of them: the reported
    /// failure was a full disk, and the answer to it was one more file in the
    /// same directory every 1.5 seconds. The target here is a *directory*, which
    /// is a rename Windows and Unix both refuse for a reason nothing in this
    /// process can fix, so the retry loop it stands for is the real one.
    ///
    /// Red gate: put `fs::rename(..).map_err(..)` back in `commit_rename` and
    /// the directory listing below holds the temp file.
    #[test]
    fn a_rename_that_fails_takes_its_temp_file_with_it() {
        let dir = std::env::temp_dir().join(format!("bt-persist-atomic-stuck-{}", unique_suffix()));
        fs::create_dir_all(&dir).unwrap();
        // A directory cannot be replaced by a file, and it is the one refusal
        // available without a full volume or a permission edit.
        let target = dir.join("session.json");
        fs::create_dir(&target).unwrap();

        let result = atomic_write(&target, b"NEW-CONTENT");
        assert!(result.is_err(), "renaming over a directory must fail");

        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name != "session.json")
            .collect();
        assert!(
            leftovers.is_empty(),
            "a failed rename must not leave its temp file behind: {leftovers:?}"
        );

        fs::remove_dir_all(&dir).unwrap();
    }
}
