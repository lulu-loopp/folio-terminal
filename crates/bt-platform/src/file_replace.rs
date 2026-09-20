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

/// Replace an existing, single-link file. `recovery` is a fresh sibling name:
/// ReplaceFileW can move the original there before reporting an error. Keep it
/// recoverable, and never fall back to MoveFileEx when metadata merging fails.
#[cfg(windows)]
pub fn replace_file_preserving(temp: &Path, target: &Path, recovery: &Path) -> io::Result<()> {
    use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
    use windows::{
        Win32::Storage::FileSystem::{
            FILE_FLAGS_AND_ATTRIBUTES, REPLACE_FILE_FLAGS, ReplaceFileW, SetFileAttributesW,
        },
        core::PCWSTR,
    };
    fn wide(path: &Path) -> io::Result<Vec<u16>> {
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
    let target_w = wide(target)?;
    let temp_w = wide(temp)?;
    let recovery_w = wide(recovery)?;
    let attributes = fs::metadata(target)?.file_attributes();
    // Hidden/Archive/System flags are not in ReplaceFileW's documented merge
    // list. Set them on the replacement before committing; an error costs the
    // original nothing. Encryption/compression/streams/DACL merge in ReplaceFileW.
    // SAFETY: terminated strings remain live throughout the synchronous calls.
    unsafe {
        SetFileAttributesW(
            PCWSTR(temp_w.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(attributes),
        )
    }
    .map_err(|_| io::Error::last_os_error())?;
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
    // SAFETY: target_w is still a live, terminated native path.
    if unsafe {
        SetFileAttributesW(
            PCWSTR(target_w.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(attributes),
        )
    }
    .is_err()
    {
        let error = io::Error::last_os_error();
        if let Err(restore) = fs::rename(recovery, target) {
            return Err(io::Error::other(format!(
                "{error}; original retained at {} (restore failed: {restore})",
                recovery.display()
            )));
        }
        return Err(error);
    }
    // It is now just an extra metadata-preserving recovery copy. The caller
    // already keeps its dated content backup; deletion here is best effort.
    let _ = fs::remove_file(recovery);
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
    use windows::{
        Win32::Storage::FileSystem::{FILE_FLAGS_AND_ATTRIBUTES, SetFileAttributesW},
        core::PCWSTR,
    };

    #[test]
    fn preserving_replacement_keeps_hidden_and_archive_set_or_clear() {
        let root = std::env::temp_dir().join(format!(
            "bt-file-replace-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        for attributes in [0x2, 0x2 | 0x20] {
            // Hidden; Hidden + Archive
            let target = root.join(format!("profile-{attributes}"));
            let temp = root.join("replacement");
            let recovery = root.join("recovery");
            fs::write(&target, b"old").unwrap();
            fs::write(&temp, b"new").unwrap();
            let wide: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
            // SAFETY: this test owns the file and the terminated path buffer.
            unsafe {
                SetFileAttributesW(PCWSTR(wide.as_ptr()), FILE_FLAGS_AND_ATTRIBUTES(attributes))
            }
            .unwrap();
            replace_file_preserving(&temp, &target, &recovery).unwrap();
            assert_eq!(fs::metadata(&target).unwrap().file_attributes(), attributes);
            assert_eq!(fs::read(&target).unwrap(), b"new");
            assert!(!recovery.exists());
            assert!(!temp.exists());
        }
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(not(windows))]
pub fn replace_file_preserving(temp: &Path, target: &Path, _recovery: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, chown};
        let metadata = fs::metadata(target)?;
        // chown may clear set-ID bits, so restore permissions after ownership.
        chown(temp, Some(metadata.uid()), Some(metadata.gid()))?;
        fs::set_permissions(temp, metadata.permissions())?;
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
