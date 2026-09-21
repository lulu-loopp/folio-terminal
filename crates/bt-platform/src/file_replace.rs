//! Native metadata needed when replacing a file owned by the user.
use std::{fs, io, path::Path};

/// Query the opened file, not a path inferred from its name.
pub fn file_link_count(file: &fs::File) -> io::Result<u64> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: file owns a live handle; info is writable for the call.
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
            .map_err(|_| io::Error::last_os_error())?;
        Ok(u64::from(info.nNumberOfLinks))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(file.metadata()?.nlink())
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = file;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "file link count unavailable",
        ))
    }
}

/// The names in what `listxattr` answers: every extended attribute the file
/// carries, NUL-terminated, one after another in a single buffer. An empty
/// answer is no names, and the terminator after the last name is a terminator
/// and not an empty name.
///
/// Pure, and outside every `cfg`, so the parse that decides *which* attributes
/// a save carries is read by a test on every platform and not only on the one
/// whose libc has the call.
pub fn extended_attribute_names(list: &[u8]) -> Vec<&[u8]> {
    list.split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .collect()
}

/// Carry onto `replacement` what `replaced` holds and a rename cannot bring
/// across by itself — a renamed file is a different object wearing the same
/// name, and everything the old object carried stays with the old object.
///
/// Windows: the attribute word (hidden, system, not-content-indexed, …). The
/// alternate data streams and the DACL are `ReplaceFileW`'s to merge and there
/// is no other way to carry them, which is why the preserving replacement is
/// the path a save takes whenever the file can take it.
///
/// Unix: ownership, mode and every extended attribute — `com.apple.quarantine`
/// and the Finder's tags on macOS, `user.*` on Linux.
///
/// **It cannot fail, by construction** — the rule is *content is guaranteed,
/// what the file carried is best effort*, and a function that could return an
/// error here is a function some caller will one day fail a save on. What
/// cannot be carried is exactly what a plain rename has always lost, and a
/// document that used to save must not stop saving because its metadata could
/// not be copied.
#[cfg(windows)]
pub fn carry_metadata(replacement: &Path, replaced: &Path) {
    use std::os::windows::fs::MetadataExt;
    if let Ok(metadata) = fs::metadata(replaced) {
        let _ = set_file_attributes(replacement, metadata.file_attributes() & !READ_ONLY);
    }
}

/// `FILE_ATTRIBUTE_READONLY`, and **the one attribute a replacement is never
/// given before it is committed.** A read-only file cannot be renamed over or
/// replaced at all, so the bit is only ever read off a target whose save is
/// about to fail — and a read-only staging file is one this process can no
/// longer delete, which turns that clean failure into a temp file left in the
/// reader's folder for every retry.
#[cfg(windows)]
const READ_ONLY: u32 = 0x1;

#[cfg(unix)]
pub fn carry_metadata(replacement: &Path, replaced: &Path) {
    use std::os::unix::fs::{MetadataExt, chown};
    let Ok(metadata) = fs::metadata(replaced) else {
        return;
    };
    // Handing a file to another user is the privileged call. When the kernel
    // refuses it the replacement stays this process's — which is what every
    // plain rename before this rule left behind — rather than failing a save.
    let _ = chown(replacement, Some(metadata.uid()), Some(metadata.gid()));
    // chown may clear set-ID bits, so restore permissions after ownership. A
    // mode that cannot be widened leaves the replacement at the `0600` it was
    // born with, which is the safe direction to fail in.
    let _ = fs::set_permissions(replacement, metadata.permissions());
    carry_extended_attributes(replaced, replacement);
}

#[cfg(not(any(windows, unix)))]
pub fn carry_metadata(replacement: &Path, replaced: &Path) {
    let _ = (replacement, replaced);
}

/// A NUL-terminated native path, or a refusal for the one string it cannot be.
#[cfg(any(windows, unix))]
fn native_path(path: &Path) -> io::Result<NativePath> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let mut value: Vec<_> = path.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "NUL in file path",
            ));
        }
        value.push(0);
        Ok(value)
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in file path"))
    }
}

#[cfg(windows)]
type NativePath = Vec<u16>;
#[cfg(unix)]
type NativePath = std::ffi::CString;

/// The Win32 attribute word, set whole. Public because it is one of the native
/// calls this crate exists to be the only home of.
#[cfg(windows)]
pub fn set_file_attributes(path: &Path, attributes: u32) -> io::Result<()> {
    use windows::{
        Win32::Storage::FileSystem::{FILE_FLAGS_AND_ATTRIBUTES, SetFileAttributesW},
        core::PCWSTR,
    };
    let path_w = native_path(path)?;
    // SAFETY: the terminated string stays live throughout the synchronous call.
    unsafe {
        SetFileAttributesW(
            PCWSTR(path_w.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(attributes),
        )
    }
    .map_err(|_| io::Error::last_os_error())
}

/// Every extended attribute `from` carries, onto `to`.
///
/// macOS: one `copyfile` with `COPYFILE_XATTR | COPYFILE_ACL` — the call the
/// platform gives for exactly this, and the only one that also brings the
/// resource fork and the access-control list across.
///
/// Linux: `listxattr`/`getxattr`/`setxattr`, name by name. `security.*` and
/// `trusted.*` are the kernel's to grant and an unprivileged `setxattr` of them
/// is refused; a refusal skips that name and carries the rest, because losing
/// one label is smaller than losing the save.
#[cfg(target_os = "macos")]
fn carry_extended_attributes(from: &Path, to: &Path) {
    let (Ok(from), Ok(to)) = (native_path(from), native_path(to)) else {
        return;
    };
    // SAFETY: both paths are live, terminated native strings; the null state
    // pointer is copyfile's documented "no state of my own" argument.
    unsafe {
        libc::copyfile(
            from.as_ptr(),
            to.as_ptr(),
            std::ptr::null_mut(),
            libc::COPYFILE_XATTR | libc::COPYFILE_ACL,
        );
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn carry_extended_attributes(from: &Path, to: &Path) {
    let (Ok(from), Ok(to)) = (native_path(from), native_path(to)) else {
        return;
    };
    // SAFETY: every call below is given a live terminated path and a buffer
    // whose length is the one being passed; a negative answer is read as the
    // failure it is and nothing is copied out of an unwritten buffer.
    let size = unsafe { libc::listxattr(from.as_ptr(), std::ptr::null_mut(), 0) };
    let Ok(size) = usize::try_from(size) else {
        return;
    };
    let mut list = vec![0_u8; size];
    let read = unsafe { libc::listxattr(from.as_ptr(), list.as_mut_ptr().cast(), list.len()) };
    let Ok(read) = usize::try_from(read) else {
        return;
    };
    list.truncate(read.min(list.len()));
    for name in extended_attribute_names(&list) {
        let Ok(name) = std::ffi::CString::new(name) else {
            continue;
        };
        let size = unsafe { libc::getxattr(from.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0) };
        let Ok(size) = usize::try_from(size) else {
            continue;
        };
        let mut value = vec![0_u8; size];
        let read = unsafe {
            libc::getxattr(
                from.as_ptr(),
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
            )
        };
        let Ok(read) = usize::try_from(read) else {
            continue;
        };
        value.truncate(read.min(value.len()));
        unsafe {
            libc::setxattr(
                to.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            );
        }
    }
}

/// Why a preserving replacement did not happen — and, the part the caller
/// needs, whether the document is provably still under its own name.
///
/// **The rule this type exists for: content is guaranteed, what the file
/// carried is best effort.** A volume that cannot do a preserving replacement
/// at all must not cost the reader their save, and a replacement that was
/// *attempted* must never be answered by a second write on top of whatever it
/// left behind.
#[derive(Debug)]
pub enum ReplaceRefusal {
    /// **This volume does not do preserving replacement**, and it changed
    /// nothing: the document is still under its own name and no backup was
    /// materialised — both asked of the filesystem, not assumed from the error
    /// code. A caller may answer this one by writing the file the plain way.
    VolumeCannot(io::Error),
    /// Everything else: denied, held open, out of space, or a replacement that
    /// got part of the way. Whatever it left is what the message describes, and
    /// no other writer may be pointed at the same file after it.
    Refused(io::Error),
}

impl ReplaceRefusal {
    /// The error to report, whichever kind of refusal this was.
    pub fn into_io(self) -> io::Error {
        match self {
            Self::VolumeCannot(error) | Self::Refused(error) => error,
        }
    }
}

/// The Win32 answers that mean *this volume does not do preserving replacement
/// at all*, as against *this one replacement did not work*.
///
/// `ERROR_INVALID_FUNCTION` (1) and `ERROR_CALL_NOT_IMPLEMENTED` (120) are what
/// a redirector or a user-mode filesystem driver answers for an operation it
/// never implemented; `ERROR_NOT_SUPPORTED` (50) is the documented "this volume
/// cannot". Everything else — access denied, sharing violation, a full disk —
/// is about this file or this moment, and a caller that answered it by writing
/// the file another way would be retrying a failure that is about to repeat.
///
/// Pure, and outside every `cfg`, so the classification is read by a test on
/// every platform rather than only where the codes come from.
pub fn volume_cannot_replace(code: Option<i32>) -> bool {
    matches!(code, Some(1 | 50 | 120))
}

/// `ERROR_UNABLE_TO_MOVE_REPLACEMENT_2`: the one documented partial state.
/// ReplaceFileW has moved the replaced file to the backup name and could not
/// move the replacement into the name it vacated — so the document's own name
/// is free, and the document is under the backup name.
#[cfg(windows)]
const ERROR_UNABLE_TO_MOVE_REPLACEMENT_2: i32 = 1177;

/// Replace an existing, single-link file. `recovery` is a fresh sibling name:
/// ReplaceFileW can move the original there before reporting an error. Keep it
/// recoverable, and never fall back to MoveFileEx when the *replacement* fails
/// — [`ReplaceRefusal::VolumeCannot`] is the only refusal a caller may answer
/// with a writer of its own, and it is given only when the filesystem itself
/// says nothing was changed.
#[cfg(windows)]
pub fn replace_file_preserving(
    temp: &Path,
    target: &Path,
    recovery: &Path,
) -> Result<(), ReplaceRefusal> {
    replace_file_preserving_with(
        temp,
        target,
        recovery,
        replace_file_w,
        rename_path,
        link_path,
    )
}

/// The replacement, the restore and the backup's retirement, with the three
/// operations that touch the disk passed in — so every branch below is reached
/// by a test on a filesystem that does all of them, instead of only on the
/// volumes that happen to fail.
#[cfg(windows)]
fn replace_file_preserving_with(
    temp: &Path,
    target: &Path,
    recovery: &Path,
    replace: impl Fn(&Path, &Path, &Path) -> io::Result<()>,
    rename: impl Fn(&Path, &Path) -> io::Result<()>,
    link: impl Fn(&Path, &Path) -> io::Result<()>,
) -> Result<(), ReplaceRefusal> {
    use std::os::windows::fs::MetadataExt;
    let attributes = fs::metadata(target).ok().map(|data| data.file_attributes());
    // Hidden/Archive/System flags are not in ReplaceFileW's documented merge
    // list, so they are staged on the replacement; encryption, compression,
    // streams and the DACL merge inside ReplaceFileW. Read-only is withheld
    // until after the commit (see [`READ_ONLY`]), and the whole step is best
    // effort: an attribute is not the document.
    if let Some(word) = attributes {
        let _ = set_file_attributes(temp, word & !READ_ONLY);
    }
    if let Err(error) = replace(temp, target, recovery) {
        // Asked of the filesystem rather than assumed from the error code: the
        // document is still under its own name, and no backup was materialised.
        // Read before anything is cleaned up, because the cleanup would change
        // the answer.
        let untouched = target.exists() && !recovery.exists();
        if error.raw_os_error() == Some(ERROR_UNABLE_TO_MOVE_REPLACEMENT_2) {
            return Err(ReplaceRefusal::Refused(restore_replaced(
                error, recovery, target, rename, link,
            )));
        }
        // Classified from the code the volume gave, before any message is built
        // around it.
        let cannot = untouched && volume_cannot_replace(error.raw_os_error());
        let error = retiring(error, recovery, target);
        return Err(if cannot {
            ReplaceRefusal::VolumeCannot(error)
        } else {
            ReplaceRefusal::Refused(error)
        });
    }
    // ReplaceFileW sets Archive even when it was clear on both inputs. Put the
    // original word back — best effort, because the content has landed and
    // rolling a successful save back over an attribute would be choosing the
    // metadata over the document it belongs to.
    if let Some(word) = attributes {
        let _ = set_file_attributes(target, word);
    }
    let _ = retire_recovery(recovery, target);
    Ok(())
}

/// The backup ReplaceFileW was given is a copy of the original, and it matters
/// only while the original is not under its own name. `true` when it is gone
/// (or was never made); `false` when it was kept — either because it is the
/// only copy of the document, or because the filesystem would not delete it.
#[cfg(windows)]
fn retire_recovery(recovery: &Path, target: &Path) -> bool {
    if !recovery.exists() {
        return true;
    }
    if !target.exists() {
        // It is the document. Keeping it is the whole point of asking for it.
        return false;
    }
    fs::remove_file(recovery).is_ok()
}

/// [`retire_recovery`], and an error that says where the copy is when one was
/// kept. A leftover nobody is told about is the leftover this rule exists for.
#[cfg(windows)]
fn retiring(error: io::Error, recovery: &Path, target: &Path) -> io::Error {
    if retire_recovery(recovery, target) {
        return error;
    }
    io::Error::other(format!(
        "{error}; the file's previous content is at {}",
        recovery.display()
    ))
}

/// Put the document back under its own name after
/// `ERROR_UNABLE_TO_MOVE_REPLACEMENT_2`.
///
/// **A rename, because that is the one restore every filesystem has.** The
/// documented state of this error leaves the document's own name free, so the
/// rename is the move the API stopped short of. `hard_link` came first here
/// once, on the grounds that it cannot replace a file created at that name in
/// the microseconds since — but links do not exist on FAT, exFAT or most
/// network redirectors, which are exactly the volumes this ladder is for, and a
/// document left under a `.tmp-…` name is a worse answer than a race nobody has
/// seen. It stays as the second try, for a filesystem that refuses a rename.
///
/// If both are refused the document is not lost, and the error says where it is.
#[cfg(windows)]
fn restore_replaced(
    error: io::Error,
    recovery: &Path,
    target: &Path,
    rename: impl Fn(&Path, &Path) -> io::Result<()>,
    link: impl Fn(&Path, &Path) -> io::Result<()>,
) -> io::Error {
    if rename(recovery, target).is_ok() {
        return error;
    }
    if link(recovery, target).is_ok() {
        let _ = fs::remove_file(recovery);
        return error;
    }
    io::Error::other(format!(
        "{error}; the file was not lost — its content is at {}",
        recovery.display()
    ))
}

#[cfg(windows)]
fn replace_file_w(temp: &Path, target: &Path, recovery: &Path) -> io::Result<()> {
    use windows::{
        Win32::Storage::FileSystem::{REPLACE_FILE_FLAGS, ReplaceFileW},
        core::PCWSTR,
    };
    let target_w = native_path(target)?;
    let temp_w = native_path(temp)?;
    let recovery_w = native_path(recovery)?;
    // SAFETY: terminated strings remain live throughout the synchronous call.
    unsafe {
        ReplaceFileW(
            PCWSTR(target_w.as_ptr()),
            PCWSTR(temp_w.as_ptr()),
            PCWSTR(recovery_w.as_ptr()),
            REPLACE_FILE_FLAGS(0),
            None,
            None,
        )
    }
    .map_err(|_| io::Error::last_os_error())
}

#[cfg(windows)]
fn rename_path(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn link_path(from: &Path, to: &Path) -> io::Result<()> {
    fs::hard_link(from, to)
}

#[cfg(not(windows))]
pub fn replace_file_preserving(
    temp: &Path,
    target: &Path,
    _recovery: &Path,
) -> Result<(), ReplaceRefusal> {
    #[cfg(unix)]
    {
        // Ownership, mode and every extended attribute — quarantine, Finder
        // tags, `user.*` — before the rename that makes the replacement the
        // file. There is no ReplaceFileW here and none is needed: on Unix
        // everything the old object carried can be put on the new one first,
        // and the commit is the single atomic rename it has always been.
        carry_metadata(temp, target);
        fs::File::open(temp)
            .and_then(|file| file.sync_all())
            .map_err(ReplaceRefusal::Refused)?;
        fs::rename(temp, target).map_err(ReplaceRefusal::Refused)
    }
    #[cfg(not(unix))]
    {
        let _ = (temp, target);
        // Nothing was touched, so the caller's plain writer is free to land the
        // content — which is the whole of what this platform can promise.
        Err(ReplaceRefusal::VolumeCannot(io::Error::new(
            io::ErrorKind::Unsupported,
            "preserving replacement unavailable",
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "bt-file-replace-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// **Which attributes are carried is read out of one buffer**, and the
    /// reading is the same arithmetic on every platform even though only one of
    /// them has the call that fills the buffer. A terminator read as an empty
    /// name would hand `CString::new` an empty attribute name and ask the
    /// kernel for it once per file.
    ///
    /// Mutation: drop the `filter` and the last two cases each grow a name.
    #[test]
    fn the_names_in_a_listxattr_answer_are_its_names_and_not_its_terminators() {
        assert!(extended_attribute_names(b"").is_empty());
        assert_eq!(
            extended_attribute_names(b"user.folio.test\0"),
            vec![&b"user.folio.test"[..]]
        );
        assert_eq!(
            extended_attribute_names(b"com.apple.quarantine\0user.tag\0"),
            vec![&b"com.apple.quarantine"[..], &b"user.tag"[..]]
        );
        // A buffer that was filled to exactly its length has no room for the
        // final terminator, and the last name is still a name.
        assert_eq!(
            extended_attribute_names(b"user.a\0user.b"),
            vec![&b"user.a"[..], &b"user.b"[..]]
        );
        assert!(extended_attribute_names(b"\0\0").is_empty());
    }

    /// **Which refusal may be answered by a second writer**, read off the code
    /// the volume gave rather than off a volume nobody in this project owns.
    ///
    /// The three that mean *this filesystem has no such operation* are the
    /// three a save may answer by writing the file the plain way; a denial, a
    /// sharing violation and a full disk are about this file or this moment and
    /// would fail the same way twice.
    ///
    /// Mutation: add `Some(5)` (access denied) to the set and a locked file
    /// starts being written by a second writer after the first was refused.
    #[test]
    fn only_a_volume_without_the_operation_may_be_answered_another_way() {
        for code in [1, 50, 120] {
            assert!(
                volume_cannot_replace(Some(code)),
                "{code} means this volume does not do preserving replacement"
            );
        }
        for code in [5, 32, 39, 87, 1175, 1176, 1177] {
            assert!(
                !volume_cannot_replace(Some(code)),
                "{code} is about this file or this moment"
            );
        }
        assert!(
            !volume_cannot_replace(None),
            "and a non-OS error is not one"
        );
    }

    /// A replacer that did not run, and left the filesystem as it found it.
    #[cfg(windows)]
    fn refuses(code: i32) -> impl Fn(&Path, &Path, &Path) -> io::Result<()> {
        move |_temp: &Path, _target: &Path, _recovery: &Path| {
            Err(io::Error::from_raw_os_error(code))
        }
    }

    #[cfg(windows)]
    fn never(_from: &Path, _to: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(1))
    }

    /// RED (closure review R1) — **a volume that cannot do a preserving
    /// replacement must not cost the reader their save.**
    ///
    /// Each of the three codes, with the filesystem untouched: the answer is
    /// `VolumeCannot`, which is the one refusal `bt-persist` may answer with the
    /// plain writer. Without it, one mapped drive or one user-mode cloud folder
    /// without `ReplaceFile` makes every Ctrl+S on that volume fail where it
    /// landed before this branch existed.
    ///
    /// Red gate: return `Refused` for every code and all three assertions go.
    #[cfg(windows)]
    #[test]
    fn a_volume_without_the_operation_refuses_without_changing_anything() {
        let root = temp_root("unsupported");
        for code in [1, 50, 120] {
            let target = root.join(format!("notes-{code}.md"));
            let temp = root.join(format!("notes-{code}.md.tmp"));
            let recovery = root.join(format!("notes-{code}.md.rec"));
            fs::write(&target, b"as it was").unwrap();
            fs::write(&temp, b"as it would be").unwrap();

            let refusal = replace_file_preserving_with(
                &temp,
                &target,
                &recovery,
                refuses(code),
                never,
                never,
            )
            .expect_err("the volume refused");

            assert!(
                matches!(refusal, ReplaceRefusal::VolumeCannot(_)),
                "{code}: the document is untouched, so the plain writer may have it"
            );
            assert_eq!(fs::read(&target).unwrap(), b"as it was");
            assert!(!recovery.exists(), "and no backup was left beside it");
        }
        fs::remove_dir_all(root).unwrap();
    }

    /// **"Untouched" is asked of the filesystem, not of the error code.** Same
    /// unsupported code, but this replacer got as far as moving the document to
    /// the backup name; answering it with a second writer would write a new file
    /// over a name whose content is no longer there. It is `Refused`.
    ///
    /// Red gate: drop the `untouched` conjunct and this returns `VolumeCannot`.
    #[cfg(windows)]
    #[test]
    fn an_unsupported_code_after_the_document_moved_is_not_answerable() {
        let root = temp_root("moved");
        let target = root.join("notes.md");
        let temp = root.join("notes.md.tmp");
        let recovery = root.join("notes.md.rec");
        fs::write(&target, b"as it was").unwrap();
        fs::write(&temp, b"as it would be").unwrap();
        let moved = |_temp: &Path, target: &Path, recovery: &Path| {
            fs::rename(target, recovery)?;
            Err(io::Error::from_raw_os_error(50))
        };

        let refusal = replace_file_preserving_with(&temp, &target, &recovery, moved, never, never)
            .expect_err("the replacement did not happen");

        assert!(
            matches!(refusal, ReplaceRefusal::Refused(_)),
            "a replacement that got part of the way is nobody's to retry"
        );
        assert!(
            recovery.exists(),
            "and the only copy of the document is kept"
        );
        assert!(
            refusal.into_io().to_string().contains("notes.md.rec"),
            "and the reader is told where it is"
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// RED (closure review R3) — **no permanent leftovers.**
    ///
    /// A replacement that materialised the backup and then failed used to leave
    /// it in the reader's folder for ever. The document is under its own name,
    /// so the backup is a redundant copy and it goes.
    ///
    /// Red gate: return the error without calling `retiring` and the assertion
    /// finds `notes.md.rec` still there.
    #[cfg(windows)]
    #[test]
    fn a_backup_nothing_is_riding_on_does_not_survive_the_failure() {
        let root = temp_root("backup");
        let target = root.join("notes.md");
        let temp = root.join("notes.md.tmp");
        let recovery = root.join("notes.md.rec");
        fs::write(&target, b"as it was").unwrap();
        fs::write(&temp, b"as it would be").unwrap();
        let copied = |_temp: &Path, target: &Path, recovery: &Path| {
            fs::copy(target, recovery)?;
            Err(io::Error::from_raw_os_error(5))
        };

        let refusal = replace_file_preserving_with(&temp, &target, &recovery, copied, never, never)
            .expect_err("access denied");

        assert!(matches!(refusal, ReplaceRefusal::Refused(_)));
        assert_eq!(fs::read(&target).unwrap(), b"as it was");
        assert!(
            !recovery.exists(),
            "the copy nobody needs is not left in the reader's folder"
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// RED (closure review R2) — **after the one documented partial state, the
    /// document goes back under its own name, and the restore does not depend
    /// on hard links.**
    ///
    /// `ERROR_UNABLE_TO_MOVE_REPLACEMENT_2` leaves the document under the backup
    /// name with its own name free. Three rungs are exercised: the rename that
    /// every filesystem has, the hard link for a filesystem that refuses one,
    /// and both refused — where the document is still not lost and the error
    /// says where it is.
    ///
    /// Red gate: restore with `hard_link` alone and the middle case is the only
    /// one that passes; restore with nothing and the document stays under
    /// `notes.md.rec` in all three.
    #[cfg(windows)]
    #[test]
    fn the_documented_partial_state_puts_the_file_back_under_its_own_name() {
        let root = temp_root("partial");
        let moved_to_backup = |_temp: &Path, target: &Path, recovery: &Path| {
            fs::rename(target, recovery)?;
            Err(io::Error::from_raw_os_error(
                ERROR_UNABLE_TO_MOVE_REPLACEMENT_2,
            ))
        };

        // ① The rename rung.
        let target = root.join("renamed.md");
        let recovery = root.join("renamed.md.rec");
        fs::write(&target, b"the document").unwrap();
        fs::write(root.join("renamed.md.tmp"), b"the replacement").unwrap();
        let refusal = replace_file_preserving_with(
            &root.join("renamed.md.tmp"),
            &target,
            &recovery,
            moved_to_backup,
            rename_path,
            never,
        )
        .expect_err("the replacement did not complete");
        assert!(matches!(refusal, ReplaceRefusal::Refused(_)));
        assert_eq!(
            fs::read(&target).unwrap(),
            b"the document",
            "back under its own name, and it is the original object"
        );
        assert!(!recovery.exists(), "with nothing left beside it");

        // ② The hard-link rung, for a filesystem that refuses the rename.
        let target = root.join("linked.md");
        let recovery = root.join("linked.md.rec");
        fs::write(&target, b"the document").unwrap();
        fs::write(root.join("linked.md.tmp"), b"the replacement").unwrap();
        let refusal = replace_file_preserving_with(
            &root.join("linked.md.tmp"),
            &target,
            &recovery,
            moved_to_backup,
            never,
            link_path,
        )
        .expect_err("the replacement did not complete");
        assert!(matches!(refusal, ReplaceRefusal::Refused(_)));
        assert_eq!(fs::read(&target).unwrap(), b"the document");
        assert!(!recovery.exists());

        // ③ Neither: the document is not lost, and the message names it.
        let target = root.join("stranded.md");
        let recovery = root.join("stranded.md.rec");
        fs::write(&target, b"the document").unwrap();
        fs::write(root.join("stranded.md.tmp"), b"the replacement").unwrap();
        let refusal = replace_file_preserving_with(
            &root.join("stranded.md.tmp"),
            &target,
            &recovery,
            moved_to_backup,
            never,
            never,
        )
        .expect_err("the replacement did not complete");
        assert!(!target.exists());
        assert_eq!(fs::read(&recovery).unwrap(), b"the document");
        let said = refusal.into_io().to_string();
        assert!(
            said.contains("stranded.md.rec") && said.contains("not lost"),
            "the reader is told where their file is: {said}"
        );

        fs::remove_dir_all(root).unwrap();
    }

    /// A replacement that works leaves the document, and nothing else.
    #[cfg(windows)]
    #[test]
    fn a_replacement_that_works_retires_the_backup_it_asked_for() {
        let root = temp_root("clean");
        let target = root.join("notes.md");
        let temp = root.join("notes.md.tmp");
        let recovery = root.join("notes.md.rec");
        fs::write(&target, b"as it was").unwrap();
        fs::write(&temp, b"as it is now").unwrap();

        replace_file_preserving(&temp, &target, &recovery).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"as it is now");
        assert!(!recovery.exists() && !temp.exists());
        assert_eq!(
            fs::read_dir(&root).unwrap().count(),
            1,
            "one entry, and it is the document"
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// A user extended attribute onto a path, or `false` when the filesystem
    /// under the fixture cannot hold one (a pre-6.6 `tmpfs`, for instance).
    #[cfg(unix)]
    fn set_extended_attribute(path: &Path, name: &str, value: &[u8]) -> bool {
        let (Ok(path), Ok(name)) = (native_path(path), std::ffi::CString::new(name)) else {
            return false;
        };
        // SAFETY: live terminated path and name, and a buffer whose length is
        // the one passed beside it.
        #[cfg(target_os = "macos")]
        let set = unsafe {
            libc::setxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        #[cfg(not(target_os = "macos"))]
        let set = unsafe {
            libc::setxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        };
        set == 0
    }

    #[cfg(unix)]
    fn extended_attribute(path: &Path, name: &str) -> Option<Vec<u8>> {
        let (Ok(path), Ok(name)) = (native_path(path), std::ffi::CString::new(name)) else {
            return None;
        };
        let mut value = vec![0_u8; 512];
        // SAFETY: as above; a negative answer is read as the failure it is.
        #[cfg(target_os = "macos")]
        let read = unsafe {
            libc::getxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        #[cfg(not(target_os = "macos"))]
        let read = unsafe {
            libc::getxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
            )
        };
        let read = usize::try_from(read).ok()?;
        value.truncate(read.min(512));
        Some(value)
    }

    /// RED (audit 3, F-1) — **a save replaces the content and keeps what the
    /// file carried**, on the platform where the thing carried is an extended
    /// attribute: `com.apple.quarantine` and the Finder's tags on macOS, and
    /// `user.*` here. Mode and ownership were already carried; the attributes
    /// were not, so a downloaded file came out of a save de-quarantined.
    ///
    /// Red gate: take `carry_extended_attributes` out of `carry_metadata` and
    /// the attribute assertion goes red while the mode one stays green.
    #[cfg(unix)]
    #[test]
    fn a_preserving_replacement_carries_mode_and_extended_attributes() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_root("carry");
        let target = root.join("notes.md");
        fs::write(&target, b"downloaded\n").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        let carried = set_extended_attribute(&target, "user.folio.test", b"kept");
        #[cfg(target_os = "macos")]
        assert!(
            carried,
            "APFS holds extended attributes: a fixture that cannot be set here is a broken test"
        );
        let temp = root.join("notes.md.tmp");
        fs::write(&temp, b"edited\n").unwrap();

        replace_file_preserving(&temp, &target, &root.join("notes.md.rec")).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"edited\n");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600,
            "the mode the file carried"
        );
        if carried {
            assert_eq!(
                extended_attribute(&target, "user.folio.test").as_deref(),
                Some(&b"kept"[..]),
                "and the extended attribute the file carried"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn preserving_replacement_keeps_hidden_and_archive_set_or_clear() {
        use std::os::windows::fs::MetadataExt;
        let root = temp_root("attributes");
        for attributes in [0x2, 0x2 | 0x20] {
            // Hidden; Hidden + Archive
            let target = root.join(format!("profile-{attributes}"));
            let temp = root.join("replacement");
            let recovery = root.join("recovery");
            fs::write(&target, b"old").unwrap();
            fs::write(&temp, b"new").unwrap();
            set_file_attributes(&target, attributes).unwrap();
            replace_file_preserving(&temp, &target, &recovery).unwrap();
            assert_eq!(fs::metadata(&target).unwrap().file_attributes(), attributes);
            assert_eq!(fs::read(&target).unwrap(), b"new");
            assert!(!recovery.exists());
            assert!(!temp.exists());
        }
        fs::remove_dir_all(root).unwrap();
    }

    /// **What a rename can carry, it carries** — the arm a hard-linked file
    /// still takes (`bt_persist::atomic_replace_keeping_metadata`). The streams
    /// and the DACL are `ReplaceFileW`'s alone and cannot come this way; the
    /// attribute word can, and before this it did not.
    #[cfg(windows)]
    #[test]
    fn a_replacement_carries_the_attribute_word_of_the_file_it_replaces() {
        use std::os::windows::fs::MetadataExt;
        let root = temp_root("word");
        let target = root.join("notes.md");
        let temp = root.join("notes.md.tmp");
        fs::write(&target, b"old").unwrap();
        fs::write(&temp, b"new").unwrap();
        set_file_attributes(&target, 0x2).unwrap();

        carry_metadata(&temp, &target);

        assert_eq!(fs::metadata(&temp).unwrap().file_attributes(), 0x2);
        fs::remove_dir_all(root).unwrap();
    }
}
