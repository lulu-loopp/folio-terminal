//! **The two reads of an install folder that are not file content** — who owns
//! the folder, and one extended attribute on it (`docs/plans/design/self-update-2026-09-16.md`
//! revision (b), F-2 and F-10; ticket U-1).
//!
//! `bt-app`'s `install_channel` owns the fact *how this copy was installed* and
//! reads the install marker file and scoop's receipt through
//! [`crate::file_reads`] on its own lane. What it cannot read that way lives
//! here, because both are native calls with no portable spelling:
//!
//! * **the folder's owner** — the owner SID of the folder's security descriptor
//!   on Windows (`GetNamedSecurityInfoW`), the owning uid on Unix (`stat`);
//! * **the marker attribute** — on macOS the install marker is an extended
//!   attribute on the bundle directory (`getxattr`), never a file inside the
//!   sealed bundle.
//!
//! Both are read-only. Neither takes a lock, creates a file or follows a
//! decision: they answer, and the error they meet is returned, never folded into
//! an answer — an unreadable owner is not "somebody else", and an unreadable
//! attribute is not "no marker". The caller turns every error into `Unknown`.

use std::io;
use std::path::Path;

/// **One security principal, as the platform names it** — a SID string on
/// Windows (`S-1-5-…`), `uid:<n>` on Unix.
///
/// Opaque on purpose: the only question anybody asks of it is whether an
/// [`Account`] is it, and it is never printed (a SID names a machine and a
/// person).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Principal(String);

/// **The principals that count as "this account"** for a folder's owner.
///
/// On Windows that is two SIDs: the token's user, and the token's default owner
/// — the principal Windows stamps as the owner of whatever this process creates.
/// They differ for an elevated administrator, whose new files are owned by
/// `BUILTIN\Administrators` unless the machine's policy says otherwise; a
/// folder that account unpacked is still the account's own. On Unix it is the
/// effective uid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account(Vec<Principal>);

impl Account {
    /// An account made of principals named by the caller — the test seam for
    /// "a folder owned by another account", which no unprivileged test can
    /// create for real (taking ownership needs a privilege the test does not
    /// hold). The owner read stays real; only who is asking changes.
    #[must_use]
    pub fn named(principals: impl IntoIterator<Item = String>) -> Self {
        Self(principals.into_iter().map(Principal).collect())
    }

    /// Whether `owner` is one of this account's principals.
    #[must_use]
    pub fn owns(&self, owner: &Principal) -> bool {
        self.0.contains(owner)
    }
}

/// **The account this process runs as.**
///
/// # Errors
/// The token or the uid could not be read.
pub fn current_account() -> io::Result<Account> {
    imp::current_account()
}

/// **Who owns the folder (or file) at `path`.**
///
/// # Errors
/// The security descriptor or the metadata could not be read, or the
/// filesystem keeps no owner (FAT, exFAT) — an owner that is not there is not
/// an answer.
pub fn owner_of(path: &Path) -> io::Result<Principal> {
    imp::owner_of(path)
}

/// The most bytes an attribute value may carry before it is refused as
/// something other than a marker.
pub const ATTRIBUTE_MAX_BYTES: usize = 4096;

/// **One extended attribute of `path`**: `Ok(None)` when the attribute is not
/// there, its value otherwise. The bytes count on
/// [`crate::file_reads::Lane::Install`], because they are the marker's content.
///
/// # Errors
/// The attribute could not be read, is longer than [`ATTRIBUTE_MAX_BYTES`], or
/// the filesystem (or the platform) keeps no extended attributes —
/// `ErrorKind::Unsupported` off macOS.
pub fn attribute(path: &Path, name: &str) -> io::Result<Option<Vec<u8>>> {
    let value = imp::attribute(path, name)?;
    if let Some(bytes) = &value {
        crate::file_reads::LEDGER.add(
            crate::file_reads::Lane::Install,
            bytes.len() as u64,
            1,
            Some(path),
        );
    }
    Ok(value)
}

#[cfg(windows)]
mod imp {
    use super::{Account, Principal};
    use std::ffi::c_void;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree},
        Security::{
            Authorization::{ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT},
            GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
            TOKEN_INFORMATION_CLASS, TOKEN_OWNER, TOKEN_QUERY, TOKEN_USER, TokenOwner, TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    use windows::core::PCWSTR;

    pub(super) fn current_account() -> io::Result<Account> {
        let mut token = HANDLE::default();
        // SAFETY: `GetCurrentProcess` is a pseudo-handle needing no close, and
        // `token` is a live local for the duration of the call.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }
            .map_err(io::Error::other)?;
        let result = account_of(token);
        // SAFETY: `token` was opened above and is closed exactly once.
        unsafe {
            let _ = CloseHandle(token);
        }
        result
    }

    fn account_of(token: HANDLE) -> io::Result<Account> {
        let user = token_information(token, TokenUser)?;
        // SAFETY: the kernel filled the buffer with a `TOKEN_USER`, and the SID
        // it points at lives inside the same buffer.
        let user = sid_string(unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid })?;
        let owner = token_information(token, TokenOwner)?;
        // SAFETY: as above, a `TOKEN_OWNER` whose SID lives in the buffer.
        let owner = sid_string(unsafe { (*owner.as_ptr().cast::<TOKEN_OWNER>()).Owner })?;
        let mut principals = vec![Principal(user)];
        if principals[0].0 != owner {
            principals.push(Principal(owner));
        }
        Ok(Account(principals))
    }

    /// The documented two-call shape; the buffer is `u64`s so that the
    /// structure at its head is aligned for its pointer field.
    fn token_information(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<u64>> {
        let mut needed = 0u32;
        // SAFETY: the first call fails with the size it wants.
        let _ = unsafe { GetTokenInformation(token, class, None, 0, &raw mut needed) };
        if needed == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
        // SAFETY: the buffer is at least `needed` bytes and lives across the call.
        unsafe {
            GetTokenInformation(
                token,
                class,
                Some(buffer.as_mut_ptr().cast::<c_void>()),
                needed,
                &raw mut needed,
            )
        }
        .map_err(io::Error::other)?;
        Ok(buffer)
    }

    pub(super) fn owner_of(path: &Path) -> io::Result<Principal> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut owner = PSID::default();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `wide` is NUL-terminated and outlives the call; the owner
        // pointer points into `descriptor`, which is freed below.
        let status = unsafe {
            GetNamedSecurityInfoW(
                PCWSTR(wide.as_ptr()),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                Some(&raw mut owner),
                None,
                None,
                None,
                &raw mut descriptor,
            )
        };
        if status.is_err() {
            return Err(io::Error::from_raw_os_error(status.0 as i32));
        }
        let result = if owner.is_invalid() {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the filesystem keeps no owner",
            ))
        } else {
            sid_string(owner).map(Principal)
        };
        // SAFETY: `GetNamedSecurityInfoW` documents `LocalFree` as the release
        // of the descriptor it allocated.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        }
        result
    }

    fn sid_string(sid: PSID) -> io::Result<String> {
        let mut text = windows::core::PWSTR::null();
        // SAFETY: `text` receives a `LocalAlloc`ed string freed below.
        unsafe { ConvertSidToStringSidW(sid, &raw mut text) }.map_err(io::Error::other)?;
        // SAFETY: the call above wrote a NUL-terminated wide string.
        let owned = unsafe { text.to_string() };
        // SAFETY: `ConvertSidToStringSidW` documents `LocalFree` as the release.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        }
        owned.map_err(io::Error::other)
    }

    pub(super) fn attribute(_path: &Path, _name: &str) -> io::Result<Option<Vec<u8>>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the install marker is a file on Windows",
        ))
    }
}

#[cfg(unix)]
mod imp {
    use super::{Account, Principal};
    use std::io;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    fn uid(uid: u32) -> Principal {
        Principal(format!("uid:{uid}"))
    }

    #[allow(clippy::unnecessary_wraps)]
    pub(super) fn current_account() -> io::Result<Account> {
        // SAFETY: `geteuid` has no preconditions and cannot fail.
        Ok(Account(vec![uid(unsafe { libc::geteuid() })]))
    }

    pub(super) fn owner_of(path: &Path) -> io::Result<Principal> {
        std::fs::metadata(path).map(|metadata| uid(metadata.uid()))
    }

    #[cfg(target_os = "macos")]
    pub(super) fn attribute(path: &Path, name: &str) -> io::Result<Option<Vec<u8>>> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let c_path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let c_name =
            CString::new(name).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let mut buffer = vec![0u8; super::ATTRIBUTE_MAX_BYTES];
        // SAFETY: both strings are NUL-terminated and outlive the call; the
        // buffer is `buffer.len()` bytes. Position 0 and no options: the value
        // from its start, following a symbolic link to the bundle it names.
        let read = unsafe {
            libc::getxattr(
                c_path.as_ptr(),
                c_name.as_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                0,
                0,
            )
        };
        if read < 0 {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(libc::ENOATTR) {
                Ok(None)
            } else {
                Err(error)
            };
        }
        buffer.truncate(read as usize);
        Ok(Some(buffer))
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn attribute(_path: &Path, _name: &str) -> io::Result<Option<Vec<u8>>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no install marker is defined here",
        ))
    }
}

#[cfg(not(any(windows, unix)))]
mod imp {
    use super::{Account, Principal};
    use std::io;
    use std::path::Path;

    pub(super) fn current_account() -> io::Result<Account> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
    pub(super) fn owner_of(_path: &Path) -> io::Result<Principal> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
    pub(super) fn attribute(_path: &Path, _name: &str) -> io::Result<Option<Vec<u8>>> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bt-install-evidence-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// RED (U-1) — **a folder this process creates is owned by this account.**
    ///
    /// The door's two halves have to agree about the one folder whose owner is
    /// known without asking: the one just made. On Windows that is the token's
    /// default owner, which is why [`Account`] carries it beside the user.
    ///
    /// MUTATION: drop `TokenOwner` from `current_account` — red on an elevated
    /// administrator (a CI runner), whose new folders belong to Administrators.
    #[test]
    fn install_evidence_a_folder_this_process_made_is_owned_by_this_account() {
        let dir = scratch("mine");
        let owner = owner_of(&dir).unwrap();
        assert!(current_account().unwrap().owns(&owner));
        assert!(!Account::named(["not this account".to_owned()]).owns(&owner));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// RED (U-1) — **a missing folder's owner is an error, never an answer.**
    ///
    /// MUTATION: map the error of `owner_of` to a placeholder principal.
    #[test]
    fn install_evidence_a_missing_folder_has_no_owner() {
        let dir = scratch("gone");
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(owner_of(&dir).is_err());
    }

    /// RED (U-1) — **the marker attribute is read back as written, and its
    /// absence is `None`, not an error** (macOS only: the attribute is the
    /// macOS marker; elsewhere the door answers `Unsupported`).
    ///
    /// MUTATION: return `Err` for `ENOATTR`.
    #[cfg(target_os = "macos")]
    #[test]
    fn install_evidence_the_marker_attribute_reads_back_and_its_absence_is_none() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let dir = scratch("xattr");
        let name = "io.github.lulu-loopp.folio.install";
        assert_eq!(attribute(&dir, name).unwrap(), None);
        let value = br#"{"v":1,"manager":"homebrew","uninstall_hook":false}"#;
        let c_path = CString::new(dir.as_os_str().as_bytes()).unwrap();
        let c_name = CString::new(name).unwrap();
        // SAFETY: NUL-terminated strings and a live buffer of `value.len()` bytes.
        let set = unsafe {
            libc::setxattr(
                c_path.as_ptr(),
                c_name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        assert_eq!(set, 0, "{}", io::Error::last_os_error());
        assert_eq!(attribute(&dir, name).unwrap().as_deref(), Some(&value[..]));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// **Off macOS there is no attribute marker, and the door says so as an
    /// error** — so the caller's reading is `Unknown`, never "no marker".
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn install_evidence_no_attribute_marker_off_macos_is_an_error() {
        let dir = scratch("noattr");
        let error = attribute(&dir, "io.github.lulu-loopp.folio.install").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
