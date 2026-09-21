//! **Which machine a path's volume stands on** — the one question a drive letter cannot answer
//! about itself.
//!
//! `Z:\work\notes.md` and `C:\work\notes.md` are the same spelling as far as every lexical reader
//! in this workspace is concerned: both are [`std::path::Prefix::Disk`] with a root under them,
//! which is what `bt_transcript::paths::may_read_unasked` admits and is right to admit, because it
//! is a function of the text and nothing else. But `net use Z: \\server\share` makes `Z:` a name
//! for somebody else's machine, and a reader that stopped at the spelling hands the redirector a
//! session setup to a host that may be gone — twenty-one seconds of TCP retransmission, on whatever
//! thread asked.
//!
//! `GetDriveTypeW` is the answer and it is a cheap one: it resolves the `\??\Z:` object-manager
//! symlink out of this session's device map and reports what kind of device is on the other end.
//! It touches no network and opens no file, so the crate's rule — a platform question lives behind
//! this crate's interface — costs the caller one lookup.
//!
//! `docs/DESIGN.md` §3.4 is the ruling this serves: 「网络路径（UNC/映射盘）默认不自动预览（显式确认）」
//! — a mapped drive is a network path, by name, and this is how a caller can tell.

use std::path::Path;

/// Whose disk a path stands on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Volume {
    /// A disk this machine holds: fixed, removable, optical or a RAM disk. A path on one is this
    /// window's to read on its own terms.
    ThisMachine,
    /// A mapped network drive — a letter standing for `\\server\share`. Reading one is a request
    /// to a stranger's machine made with the user's credentials, which is a click nobody made.
    AnotherMachine,
    /// Nothing is mounted there, or the platform would not say. Treated as `AnotherMachine` by
    /// every caller that has to choose, because an unanswered question is not a yes.
    Unknown,
}

/// The three-character root of a drive-letter path — `C:\` for `C:\a\b`, and `None` for every
/// spelling that is not one.
///
/// Read off the text rather than through [`std::path::Component`] so that it answers the same way
/// on every platform: this is the argument `GetDriveTypeW` takes, and a unit test of it must not
/// need a Windows path parser to state its own case.
#[must_use]
pub fn drive_letter_root(path: &Path) -> Option<String> {
    let text = path.as_os_str().to_string_lossy();
    let bytes = text.as_bytes();
    let drive_rooted = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    drive_rooted.then(|| format!("{}:\\", bytes[0] as char))
}

/// Whose disk `path` stands on.
///
/// A path with no drive letter answers [`Volume::ThisMachine`]: the only rootless spellings that
/// reach this crate's callers are `\\wsl.localhost\<distro>\…`, which is this machine's own
/// (§7.30), and a POSIX absolute path, where there is one root and no letter to ask about. Every
/// spelling that names a *stranger's* machine outright — `\\server\share`, `\\.\pipe\…` — was
/// refused by the lexical gate before it reached here.
#[cfg(windows)]
#[must_use]
pub fn volume_of(path: &Path) -> Volume {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt};
    use windows::{Win32::Storage::FileSystem::GetDriveTypeW, core::PCWSTR};

    // `GetDriveTypeW`'s own return codes, spelled here because the generated bindings do not carry
    // them as constants. The numbers are the API's documented contract and have not moved since
    // Windows NT: 0 unknown, 1 no root directory, 2 removable, 3 fixed, 4 remote, 5 CD-ROM,
    // 6 RAM disk.
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    const DRIVE_REMOTE: u32 = 4;
    const DRIVE_CDROM: u32 = 5;
    const DRIVE_RAMDISK: u32 = 6;

    let Some(root) = drive_letter_root(path) else {
        return Volume::ThisMachine;
    };
    let wide: Vec<u16> = OsStr::new(&root)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 buffer that outlives the call, which is the whole
    // of this function's contract with the API.
    match unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) } {
        DRIVE_REMOTE => Volume::AnotherMachine,
        DRIVE_FIXED | DRIVE_REMOVABLE | DRIVE_RAMDISK | DRIVE_CDROM => Volume::ThisMachine,
        // `DRIVE_NO_ROOT_DIR` and `DRIVE_UNKNOWN`. Neither says "this machine", and the caller's
        // rule is that only a yes is a yes.
        _ => Volume::Unknown,
    }
}

/// The same question on a filesystem with one root and no volumes to tell apart.
#[cfg(not(windows))]
#[must_use]
pub fn volume_of(path: &Path) -> Volume {
    let _ = path;
    Volume::ThisMachine
}

#[cfg(test)]
mod tests {
    use super::{Volume, drive_letter_root, volume_of};
    use std::path::Path;

    #[test]
    fn a_drive_letter_path_names_the_root_its_kind_is_asked_of() {
        assert_eq!(
            drive_letter_root(Path::new(r"C:\a\b")).as_deref(),
            Some("C:\\")
        );
        assert_eq!(
            drive_letter_root(Path::new("Z:/work/a.txt")).as_deref(),
            Some("Z:\\")
        );
        assert_eq!(drive_letter_root(Path::new(r"C:notes.md")), None);
        assert_eq!(
            drive_letter_root(Path::new(r"\\wsl.localhost\Ubuntu\home")),
            None
        );
        assert_eq!(drive_letter_root(Path::new("/usr/share")), None);
    }

    /// A spelling with no letter to ask about is this machine's — the WSL share and the POSIX
    /// root, which are the only two that reach here.
    #[test]
    fn a_path_with_no_drive_letter_is_this_machines() {
        assert_eq!(
            volume_of(Path::new(r"\\wsl.localhost\Ubuntu\home\a")),
            Volume::ThisMachine
        );
        assert_eq!(volume_of(Path::new("/usr/share/a")), Volume::ThisMachine);
    }

    /// The system drive is a disk this machine holds, whatever else is mounted on this runner.
    #[cfg(windows)]
    #[test]
    fn the_system_drive_is_this_machines() {
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
        assert_eq!(
            volume_of(Path::new(&format!("{system}\\Windows"))),
            Volume::ThisMachine
        );
    }
}
