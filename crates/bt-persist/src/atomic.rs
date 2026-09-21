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

use bt_platform::ReplaceRefusal;

use crate::error::WriteError;

/// Writes `contents` to `path` via the temp-file-same-dir-then-rename
/// pattern. `path`'s original content is untouched until the final rename
/// succeeds — a failure at any earlier step (temp file creation, write,
/// fsync) leaves `path` exactly as it was.
pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), WriteError> {
    let tmp_path = temp_sibling_path(path).map_err(|source| WriteError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    write_temp(&tmp_path, contents, TempBirth::Umask).map_err(|source| WriteError::Io {
        path: tmp_path.clone(),
        source,
    })?;
    commit_rename(&tmp_path, path)
}

/// Replace an existing user-owned file without discarding its metadata.
/// Windows merges DACLs, streams and creation time through ReplaceFileW;
/// Unix carries ownership, mode and extended attributes before rename.
/// Multiple links are refused. New files should use `atomic_write` instead.
pub fn atomic_replace_preserving(path: &Path, contents: &[u8]) -> Result<(), WriteError> {
    preserving_replace(path, contents).map_err(|refusal| WriteError::Io {
        path: path.to_path_buf(),
        source: refusal.into_io(),
    })
}

/// The preserving replacement with its refusal kept whole, so the writer above
/// can tell the one refusal that may be answered by a second writer from every
/// other — see [`may_be_written_the_plain_way`].
fn preserving_replace(path: &Path, contents: &[u8]) -> Result<(), ReplaceRefusal> {
    let tmp_path = temp_sibling_path(path).map_err(ReplaceRefusal::Refused)?;
    let recovery = temp_sibling_path(path).map_err(ReplaceRefusal::Refused)?;
    let result = (|| -> Result<(), ReplaceRefusal> {
        let file = File::open(path).map_err(ReplaceRefusal::Refused)?;
        if bt_platform::file_link_count(&file).map_err(ReplaceRefusal::Refused)? > 1 {
            return Err(ReplaceRefusal::Refused(io::Error::other(
                "hard-linked files are not replaced",
            )));
        }
        drop(file);
        write_temp(&tmp_path, contents, TempBirth::OwnerOnly).map_err(ReplaceRefusal::Refused)?;
        bt_platform::replace_file_preserving(&tmp_path, path, &recovery)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

/// **Content is guaranteed; what the file carried is best effort.** The one
/// refusal a save may answer with the plain writer is the volume saying it does
/// not do preserving replacement at all — which it gives only after the
/// filesystem has confirmed that nothing was changed. Every other refusal is
/// about this file or this moment, and pointing a second writer at the same
/// path would be writing on top of whatever the first one left.
///
/// Pure, so the rule is read by a test rather than by a volume nobody has.
fn may_be_written_the_plain_way(refusal: &ReplaceRefusal) -> bool {
    matches!(refusal, ReplaceRefusal::VolumeCannot(_))
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
/// * A volume that answers the preserving replacement with "this is not an
///   operation I have" → the same writer, **after** the filesystem has
///   confirmed that nothing was changed. A network redirector or a
///   user-mode cloud drive without `ReplaceFile` would otherwise make every
///   Ctrl+S on that volume fail where it used to land, which is the rule read
///   backwards: content is guaranteed, what the file carried is best effort.
pub fn atomic_replace_keeping_metadata(path: &Path, contents: &[u8]) -> Result<(), WriteError> {
    if !can_be_preserved(path) {
        return atomic_write_carrying(path, contents);
    }
    match preserving_replace(path, contents) {
        Ok(()) => Ok(()),
        Err(refusal) if may_be_written_the_plain_way(&refusal) => {
            atomic_write_carrying(path, contents)
        }
        Err(refusal) => Err(WriteError::Io {
            path: path.to_path_buf(),
            source: refusal.into_io(),
        }),
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
    let tmp_path = temp_sibling_path(path).map_err(|source| WriteError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    write_temp(&tmp_path, contents, TempBirth::OwnerOnly).map_err(|source| WriteError::Io {
        path: tmp_path.clone(),
        source,
    })?;
    bt_platform::carry_metadata(&tmp_path, path);
    commit_rename(&tmp_path, path)
}

/// How a staging file is born, in the moments before the rename that makes it
/// the document.
///
/// **`OwnerOnly`** is for a file that is replacing somebody's own: it is
/// unreadable to anybody but this user until [`bt_platform::carry_metadata`]
/// widens it to the mode the replaced file carried. A `0600` note must not be
/// world-readable even for the length of one write, and an ordering that got
/// that right only because the `chmod` came soon enough is not the invariant.
///
/// **`Umask`** is for Folio's own files under Folio's own configuration
/// directory: they have no mode to inherit and are born the way every version
/// of this writer has borne them.
#[derive(Clone, Copy)]
enum TempBirth {
    Umask,
    OwnerOnly,
}

/// Step 1: write `contents` into `tmp_path` and `fsync` it. Does not touch
/// the eventual target file at all — this is the step that can fail without
/// endangering any existing file.
///
/// **`create_new`, so the staging name is a file this call made.** A symlink
/// planted at the exact name would otherwise be followed and the document
/// written through it; the name mixes a pid, a nanosecond clock and a counter,
/// so this closes a question rather than a known hole, and it costs a flag.
fn write_temp(tmp_path: &Path, contents: &[u8], birth: TempBirth) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if matches!(birth, TempBirth::OwnerOnly) {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = birth;
    // Opened before the cleanup below exists, so the only file this function
    // can ever delete is the one it just created — a name that was already
    // taken is refused here and left exactly as it was found.
    let mut file = options.open(tmp_path)?;
    let result = file.write_all(contents).and_then(|()| file.sync_all());
    if result.is_err() {
        // Best-effort: don't let a half-written temp file linger. If this
        // also fails there is nothing more we can do without risking the
        // real target file, so it is deliberately ignored.
        drop(file);
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

fn temp_sibling_path(path: &Path) -> io::Result<PathBuf> {
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
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

    /// A downloaded document: an alternate data stream, an access control entry
    /// somebody set on the file itself, and the hidden bit.
    #[cfg(windows)]
    fn downloaded(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, b"# downloaded\n").unwrap();
        fs::write(dir.join(format!("{name}:Zone.Identifier")), ZONE).unwrap();
        // Everyone:(R) by SID, so the fixture does not depend on what this
        // machine's Windows calls its accounts. It grants and never denies: the
        // writer must still be able to write the file this asserts about.
        let granted = bt_platform::quiet_command("icacls")
            .arg(&path)
            .args(["/grant", "*S-1-1-0:(R)"])
            .output();
        assert!(
            granted.is_ok_and(|done| done.status.success()),
            "the fixture's explicit ACE was not set"
        );
        // Hidden + Archive: the two an ordinary user can set, and the two the
        // preserving replacement has to put back by hand.
        bt_platform::set_file_attributes(&path, 0x2 | 0x20).unwrap();
        path
    }

    #[cfg(windows)]
    const ZONE: &str = "[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://example.com/notes.md\r\n";

    #[cfg(windows)]
    fn access_control(path: &Path) -> String {
        let listed = bt_platform::quiet_command("icacls")
            .arg(path)
            .output()
            .expect("icacls runs");
        String::from_utf8_lossy(&listed.stdout).into_owned()
    }

    /// RED (audit 3, F-1) — **a save replaces the content and nothing else the
    /// file carried.**
    ///
    /// The document belongs to the reader, not to this product. A
    /// `File::create` of a sibling and a rename over the name makes a *new file
    /// object* and throws the old one away with everything on it: the alternate
    /// data streams (`Zone.Identifier` — the Mark of the Web, which is what
    /// SmartScreen and Office read before they trust a file), every access
    /// control entry set on the file itself rather than inherited from the
    /// folder, and the attribute word. Editing one line of a downloaded note
    /// silently de-quarantined it.
    ///
    /// The editor's own side of this — that `PreviewBuffer::save` is the caller
    /// of the writer below — is pinned in `bt-app`
    /// (`a_save_is_written_by_the_writer_that_keeps_what_the_file_carried`),
    /// which is the crate that may not ask what platform it is on.
    ///
    /// Red gate: call [`atomic_write`] here instead and the stream is gone, the
    /// explicit ACE is gone and the hidden bit is gone — three assertions, one
    /// cause.
    #[cfg(windows)]
    #[test]
    fn a_save_keeps_the_stream_the_ace_and_the_attributes_the_file_carried() {
        use std::os::windows::fs::MetadataExt;
        let dir = std::env::temp_dir().join(format!("bt-carried-{}", unique_suffix()));
        fs::create_dir_all(&dir).unwrap();
        let path = downloaded(&dir, "notes.md");
        assert!(
            access_control(&path).contains("Everyone:(R)"),
            "the fixture starts with an explicit entry"
        );

        atomic_replace_keeping_metadata(&path, b"# downloaded\nand edited here\n").unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# downloaded\nand edited here\n",
            "the content is the only thing a save replaces"
        );
        assert_eq!(
            fs::read_to_string(dir.join("notes.md:Zone.Identifier")).unwrap(),
            ZONE,
            "the mark of the web is still on the file the reader edited"
        );
        assert!(
            access_control(&path).contains("Everyone:(R)"),
            "and the entry somebody set on this file by hand"
        );
        assert_eq!(
            fs::metadata(&path).unwrap().file_attributes() & 0x2,
            0x2,
            "and the hidden bit it was carrying"
        );
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "and neither the staging file nor the replacement's backup is left behind"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    /// **A refusal is still a refusal, and it costs the file nothing** (audit 3,
    /// F-1). Two of them, because the writer opens the target before it writes
    /// and these two answer that open differently: a read-only file refuses the
    /// replace at the end, and a file another program holds with no sharing at
    /// all refuses the open at the start. Either way the write is reported as a
    /// failure and everything the file carried — its bytes and its stream — is
    /// exactly where it was.
    #[cfg(windows)]
    #[test]
    fn a_read_only_or_locked_file_is_refused_and_loses_nothing() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = std::env::temp_dir().join(format!("bt-refused-{}", unique_suffix()));
        fs::create_dir_all(&dir).unwrap();
        let path = downloaded(&dir, "notes.md");

        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).unwrap();
        assert!(
            atomic_replace_keeping_metadata(&path, b"mine now\n").is_err(),
            "a read-only file is not written over"
        );
        // Back to the fixture's own word — hidden and archive, and no read-only
        // bit — through the native call rather than through
        // `set_readonly(false)`, which means "world writable" one platform over.
        bt_platform::set_file_attributes(&path, 0x2 | 0x20).unwrap();

        // No sharing at all: what an editor that opened the file exclusively
        // leaves the rest of the machine looking at.
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .expect("this test's own exclusive handle");
        assert!(
            atomic_replace_keeping_metadata(&path, b"mine now\n").is_err(),
            "and neither is one somebody else is holding"
        );
        drop(held);

        assert_eq!(fs::read_to_string(&path).unwrap(), "# downloaded\n");
        assert_eq!(
            fs::read_to_string(dir.join("notes.md:Zone.Identifier")).unwrap(),
            ZONE,
            "and a refused save took nothing off the file"
        );
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "and staged nothing into the folder"
        );
        fs::remove_dir_all(&dir).unwrap();
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

    /// RED (closure review R1) — **the one refusal a save answers with the
    /// plain writer.**
    ///
    /// `VolumeCannot` is given only after the filesystem has confirmed that the
    /// document is still under its own name and no backup was made, so writing
    /// it the plain way cannot be a second write on top of a first. Every other
    /// refusal — denied, held open, out of space, a replacement that got part of
    /// the way — is reported as it stands.
    ///
    /// Red gate: match on `_` instead and a locked file gets a second writer
    /// pointed at it after the first was refused.
    #[test]
    fn only_the_volumes_refusal_is_answered_by_the_plain_writer() {
        assert!(may_be_written_the_plain_way(&ReplaceRefusal::VolumeCannot(
            io::Error::from_raw_os_error(50)
        )));
        assert!(!may_be_written_the_plain_way(&ReplaceRefusal::Refused(
            io::Error::from_raw_os_error(5)
        )));
    }

    /// The staging file is born unreadable to anybody else and is a file this
    /// call made — not a name something else planted (closure review R9, R10).
    #[test]
    fn a_staging_file_is_this_calls_own_and_nobody_elses_to_read() {
        let dir = std::env::temp_dir().join(format!("bt-birth-{}", unique_suffix()));
        fs::create_dir_all(&dir).unwrap();
        let tmp_path = temp_sibling_path(&dir.join("notes.md")).unwrap();
        write_temp(&tmp_path, b"private", TempBirth::OwnerOnly).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&tmp_path).unwrap().permissions().mode() & 0o777,
                0o600,
                "a document's replacement is never world-readable, not even briefly"
            );
        }
        assert!(
            write_temp(&tmp_path, b"again", TempBirth::OwnerOnly).is_err(),
            "a name that already exists is not written through"
        );
        assert_eq!(
            fs::read(&tmp_path).unwrap(),
            b"private",
            "and the refusal left what was there"
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
        write_temp(&tmp_path, b"NEW-CONTENT", TempBirth::Umask).unwrap();

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
        let result = write_temp(&bogus_tmp, b"NEW-CONTENT", TempBirth::Umask);
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
