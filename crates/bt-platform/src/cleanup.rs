//! Platform I/O for the uninstall door. No process enumeration or termination.
use std::{fs, io, path::Path};

pub fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

/// A read handle with no sharing detects outstanding handles within the resolved roots.
/// This is intentionally conservative: lineage cannot be recovered after Folio exits.
/// No bytes are read. The handle is closed before deletion; a later sharing race is an
/// ordinary deletion refusal. macOS has no equivalent mandatory sharing-mode probe.
pub fn probe_file(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(path)
            .map(drop)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(())
    }
}

/// Decision/effect seam: tests inject the registry reading and deletion, never HKCU.
pub fn remove_toast_with(
    reading: Result<bool, String>,
    delete: impl FnOnce() -> Result<(), String>,
) -> Result<bool, String> {
    if !reading? {
        return Ok(false);
    }
    delete()?;
    Ok(true)
}

pub fn remove_toast_identity() -> Result<bool, String> {
    #[cfg(windows)]
    {
        use windows::{
            Win32::{
                Foundation::ERROR_FILE_NOT_FOUND,
                System::Registry::{HKEY, HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegOpenKeyExW},
            },
            core::PCWSTR,
        };
        let key = crate::notification_aumid_key();
        let wide: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
        let mut opened = HKEY::default();
        // SAFETY: fixed HKCU product key; buffer lives through the synchronous call.
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(wide.as_ptr()),
                None,
                KEY_READ,
                &mut opened,
            )
        };
        let reading = if status == ERROR_FILE_NOT_FOUND {
            Ok(false)
        } else if status.is_err() {
            Err(format!("RegOpenKeyExW({key}): {}", status.0))
        } else {
            // SAFETY: this handle was opened above and is no longer used.
            unsafe {
                let _ = RegCloseKey(opened);
            }
            Ok(true)
        };
        remove_toast_with(reading, || crate::windows_impl::delete_registry_tree(&key))
    }
    #[cfg(not(windows))]
    {
        remove_toast_with(Ok(false), || Ok(()))
    }
}
