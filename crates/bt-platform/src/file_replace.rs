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
/// Callers treat a failure here as "this much could not be carried", not as a
/// failed save: what cannot be carried is exactly what a plain rename has
/// always lost, and a document that used to save must not stop saving because
/// its metadata could not be copied.
#[cfg(windows)]
pub fn carry_metadata(replacement: &Path, replaced: &Path) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    set_file_attributes(
        replacement,
        fs::metadata(replaced)?.file_attributes() & !READ_ONLY,
    )
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
pub fn carry_metadata(replacement: &Path, replaced: &Path) -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt, chown};
    let metadata = fs::metadata(replaced)?;
    // Handing a file to another user is the privileged call. When the kernel
    // refuses it the replacement stays this process's — which is what every
    // plain rename before this rule left behind — rather than failing a save.
    let _ = chown(replacement, Some(metadata.uid()), Some(metadata.gid()));
    // chown may clear set-ID bits, so restore permissions after ownership.
    fs::set_permissions(replacement, metadata.permissions())?;
    carry_extended_attributes(replaced, replacement);
    Ok(())
}

#[cfg(not(any(windows, unix)))]
pub fn carry_metadata(replacement: &Path, replaced: &Path) -> io::Result<()> {
    let _ = (replacement, replaced);
    Ok(())
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

/// Replace an existing, single-link file. `recovery` is a fresh sibling name:
/// ReplaceFileW can move the original there before reporting an error. Keep it
/// recoverable, and never fall back to MoveFileEx when metadata merging fails.
#[cfg(windows)]
pub fn replace_file_preserving(temp: &Path, target: &Path, recovery: &Path) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    use windows::{
        Win32::Storage::FileSystem::{REPLACE_FILE_FLAGS, ReplaceFileW},
        core::PCWSTR,
    };
    let target_w = native_path(target)?;
    let temp_w = native_path(temp)?;
    let recovery_w = native_path(recovery)?;
    let attributes = fs::metadata(target)?.file_attributes();
    // Hidden/Archive/System flags are not in ReplaceFileW's documented merge
    // list. Set them on the replacement before committing; an error costs the
    // original nothing. Encryption/compression/streams/DACL merge in ReplaceFileW.
    // Read-only is withheld until after the commit — see [`READ_ONLY`].
    set_file_attributes(temp, attributes & !READ_ONLY)?;
    // SAFETY: terminated strings remain live throughout the synchronous call.
    let result = unsafe {
        ReplaceFileW(
            PCWSTR(target_w.as_ptr()),
            PCWSTR(temp_w.as_ptr()),
            PCWSTR(recovery_w.as_ptr()),
            REPLACE_FILE_FLAGS(0),
            None,
            None,
        )
    };
    if result.is_err() {
        let error = io::Error::last_os_error();
        // 1177 leaves the original under the requested backup name. Restore
        // only into a missing destination; do not overwrite a concurrent edit.
        if error.raw_os_error() == Some(1177) {
            if let Err(restore) = fs::hard_link(recovery, target) {
                return Err(io::Error::other(format!(
                    "{error}; original retained at {} (restore failed: {restore})",
                    recovery.display()
                )));
            }
            let _ = fs::remove_file(recovery);
        }
        return Err(error);
    }
    // ReplaceFileW sets Archive even when it was clear on both inputs. Restore
    // the original flags before retiring the recovery copy. If this fails,
    // put the original object back (not a newly created copy of its bytes).
    if let Err(error) = set_file_attributes(target, attributes) {
        if let Err(restore) = fs::rename(recovery, target) {
            return Err(io::Error::other(format!(
                "{error}; original retained at {} (restore failed: {restore})",
                recovery.display()
            )));
        }
        return Err(error);
    }
    // It is now just an extra metadata-preserving recovery copy of a file that
    // has been replaced successfully, so nothing is riding on it: the shell
    // profile's caller keeps a dated content backup of its own, and the editor
    // has the body in the window. Deletion is best effort.
    let _ = fs::remove_file(recovery);
    Ok(())
}

#[cfg(not(windows))]
pub fn replace_file_preserving(temp: &Path, target: &Path, _recovery: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        // Ownership, mode and every extended attribute — quarantine, Finder
        // tags, `user.*` — before the rename that makes the replacement the
        // file. There is no ReplaceFileW here and none is needed: on Unix
        // everything the old object carried can be put on the new one first.
        carry_metadata(temp, target)?;
        fs::File::open(temp)?.sync_all()?;
        fs::rename(temp, target)
    }
    #[cfg(not(unix))]
    {
        let _ = (temp, target);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "preserving replacement unavailable",
        ))
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

        carry_metadata(&temp, &target).unwrap();

        assert_eq!(fs::metadata(&temp).unwrap().file_attributes(), 0x2);
        fs::remove_dir_all(root).unwrap();
    }
}
