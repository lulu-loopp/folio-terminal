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
//!   sealed bundle;
//! * **whether a folder may be written** ([`may_write_into`], U-27) — the
//!   read-only mount flag and the process's write access, on Unix: the macOS
//!   updater's test of the folder its bundle stands in;
//! * **the uninstall records of this account** ([`uninstall_records`], U-4) —
//!   the subkeys of [`UNINSTALL_KEY`] under `HKEY_CURRENT_USER`, where winget
//!   records a portable install; a registry read, with no door.
//!
//! All are read-only. Neither takes a lock, creates a file or follows a
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

/// **Whether this process may create entries in the folder at `path`** — the
/// updater's "not writable" test of the folder a bundle stands in (0.4.6 U-27,
/// C7: "a translocated bundle … sits on a read-only randomized mount: Not
/// writable").
///
/// Two questions, both answered by the system without writing anything: is the
/// file system the folder is on mounted read-only (`statvfs`'s `ST_RDONLY` — a
/// translocated bundle's nullfs mount, a disk image attached read-only), and
/// does the folder grant this process write access (`access(2)` with `W_OK`,
/// which reads the permissions and the ACL for the real ids). `false` for
/// either; `true` only when both say yes. A read-only mount or a refused access
/// (`EROFS`, `EACCES`, `EPERM`) is an answer, `false`; any other error is
/// returned — a folder that cannot be asked is not "writable", and the caller
/// does not treat it as one.
///
/// # Errors
/// The folder could not be asked (it is missing, or the call failed);
/// `ErrorKind::Unsupported` off Unix.
pub fn may_write_into(path: &Path) -> io::Result<bool> {
    imp::may_write_into(path)
}

/// **Where winget keeps a portable package's uninstall record**, under
/// `HKEY_CURRENT_USER` (E2, 2026-09-27: HKCU only; HKLM and the 32-bit view are
/// untouched by a portable install, and are not read).
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

/// The most bytes one value of an uninstall record may carry before it is
/// refused as malformed — longer than any path Windows can open.
pub const RECORD_VALUE_MAX_BYTES: usize = 64 * 1024;

/// **One named value of one uninstall record, as it was found.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordValue {
    /// The record has no value of this name.
    Absent,
    /// A `REG_SZ`, without its terminator.
    Text(String),
    /// A value of another type, one that is not UTF-16, or one longer than
    /// [`RECORD_VALUE_MAX_BYTES`]: there, and not a string this reader reads.
    Malformed,
}

/// **One subkey of the uninstall key**: its name, and the values asked for, in
/// the order they were asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UninstallRecord {
    pub key: String,
    pub values: Vec<RecordValue>,
}

/// **The uninstall records of this account** — every subkey of
/// [`UNINSTALL_KEY`] under `HKEY_CURRENT_USER`, each with the values named in
/// `names` (0.4.6 U-4: winget's own record of a portable install is how a
/// winget copy is told, since winget runs no hook that could write a marker).
///
/// Read-only and bounded: the subkeys present, `names.len()` values each, none
/// longer than [`RECORD_VALUE_MAX_BYTES`]. A registry read has no door
/// (`docs/ARCHITECTURE.md` §6): it is not file content, and it waits on
/// nothing but the registry. An uninstall key that is not there is no records;
/// a subkey that vanishes between the listing and its opening is skipped.
///
/// # Errors
/// The key could not be opened or listed, or a subkey could not be opened or
/// read, for any reason but its absence — a record that could not be read is
/// not "no record"; `ErrorKind::Unsupported` off Windows.
pub fn uninstall_records(names: &[&str]) -> io::Result<Vec<UninstallRecord>> {
    imp::uninstall_records(UNINSTALL_KEY, names)
}

#[cfg(windows)]
mod imp {
    use super::{Account, Principal, RecordValue, UninstallRecord};
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

    /// The Windows updater asks its own question of its install folder (U-20);
    /// this one is the macOS road's.
    pub(super) fn may_write_into(_path: &Path) -> io::Result<bool> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "install_evidence asks whether a folder may be written only on Unix",
        ))
    }

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

    /// An open key under `HKEY_CURRENT_USER`, closed when dropped.
    struct Key(windows::Win32::System::Registry::HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful open and is closed
            // once, here.
            unsafe {
                let _ = windows::Win32::System::Registry::RegCloseKey(self.0);
            }
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    fn os_error(code: windows::Win32::Foundation::WIN32_ERROR) -> io::Error {
        io::Error::from_raw_os_error(code.0.cast_signed())
    }

    /// `parent\name` opened for reading, `None` when it is not there.
    fn open_key(
        parent: windows::Win32::System::Registry::HKEY,
        name: &str,
    ) -> io::Result<Option<Key>> {
        use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
        use windows::Win32::System::Registry::{HKEY, KEY_READ, RegOpenKeyExW};
        let units = wide(name);
        let mut opened = HKEY::default();
        // SAFETY: the name is NUL-terminated and lives across the call;
        // `opened` is written only on success and closed by `Key`.
        let code = unsafe {
            RegOpenKeyExW(
                parent,
                PCWSTR(units.as_ptr()),
                None,
                KEY_READ,
                &raw mut opened,
            )
        };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if code != ERROR_SUCCESS {
            return Err(os_error(code));
        }
        Ok(Some(Key(opened)))
    }

    /// One named value as [`RecordValue`], through `buffer` (whose length is
    /// the cap).
    fn record_value(key: &Key, name: &str, buffer: &mut [u16]) -> io::Result<RecordValue> {
        use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
        use windows::Win32::System::Registry::{REG_SZ, REG_VALUE_TYPE, RegQueryValueExW};
        let units = wide(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut size = u32::try_from(std::mem::size_of_val(buffer)).unwrap_or(u32::MAX);
        // SAFETY: the name is NUL-terminated; `buffer` holds `size` bytes and
        // lives across the call, and `size` is updated to what was written.
        let code = unsafe {
            RegQueryValueExW(
                key.0,
                PCWSTR(units.as_ptr()),
                None,
                Some(&raw mut kind),
                Some(buffer.as_mut_ptr().cast::<u8>()),
                Some(&raw mut size),
            )
        };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(RecordValue::Absent);
        }
        if code == ERROR_MORE_DATA {
            return Ok(RecordValue::Malformed);
        }
        if code != ERROR_SUCCESS {
            return Err(os_error(code));
        }
        if kind != REG_SZ || !size.is_multiple_of(2) {
            return Ok(RecordValue::Malformed);
        }
        let mut text = &buffer[..(size as usize / 2).min(buffer.len())];
        // A `REG_SZ` carries its terminator, or (written by hand) does not.
        while let Some((&0, rest)) = text.split_last() {
            text = rest;
        }
        Ok(String::from_utf16(text).map_or(RecordValue::Malformed, RecordValue::Text))
    }

    pub(super) fn uninstall_records(
        under: &str,
        names: &[&str],
    ) -> io::Result<Vec<UninstallRecord>> {
        use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
        use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RegEnumKeyExW};
        use windows::core::PWSTR;
        let Some(root) = open_key(HKEY_CURRENT_USER, under)? else {
            return Ok(Vec::new());
        };
        let mut subkeys = Vec::new();
        // 255 characters is the documented longest key name, plus the
        // terminator the call writes.
        let mut name = [0u16; 256];
        for index in 0u32.. {
            let mut length = name.len() as u32;
            // SAFETY: `name` holds `length` units and lives across the call;
            // every other out-parameter is null.
            let code = unsafe {
                RegEnumKeyExW(
                    root.0,
                    index,
                    Some(PWSTR(name.as_mut_ptr())),
                    &raw mut length,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if code == ERROR_NO_MORE_ITEMS {
                break;
            }
            if code != ERROR_SUCCESS {
                return Err(os_error(code));
            }
            let units = &name[..(length as usize).min(name.len())];
            subkeys.push(String::from_utf16(units).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "a subkey name is not UTF-16")
            })?);
        }
        let mut buffer = vec![0u16; super::RECORD_VALUE_MAX_BYTES / 2];
        let mut records = Vec::with_capacity(subkeys.len());
        for key in subkeys {
            let Some(opened) = open_key(root.0, &key)? else {
                continue;
            };
            let values = names
                .iter()
                .map(|name| record_value(&opened, name, &mut buffer))
                .collect::<io::Result<Vec<_>>>()?;
            records.push(UninstallRecord { key, values });
        }
        Ok(records)
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

    pub(super) fn may_write_into(path: &Path) -> io::Result<bool> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let c_path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // SAFETY: an all-zero `statvfs` is a valid value for the call to
        // overwrite.
        let mut volume: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: the path is NUL-terminated and outlives the call; `volume`
        // is a live local the call fills.
        if unsafe { libc::statvfs(c_path.as_ptr(), &raw mut volume) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if volume.f_flag & libc::ST_RDONLY != 0 {
            return Ok(false);
        }
        // SAFETY: as above; `access` reads the path and the process's ids.
        if unsafe { libc::access(c_path.as_ptr(), libc::W_OK) } == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EROFS | libc::EACCES | libc::EPERM) => Ok(false),
            _ => Err(error),
        }
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

    pub(super) fn uninstall_records(
        _under: &str,
        _names: &[&str],
    ) -> io::Result<Vec<super::UninstallRecord>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "uninstall records are a Windows registry key",
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
    pub(super) fn may_write_into(_path: &Path) -> io::Result<bool> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
    pub(super) fn uninstall_records(
        _under: &str,
        _names: &[&str],
    ) -> io::Result<Vec<super::UninstallRecord>> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = bt_testpath::temp_path(&format!("bt-install-evidence-{tag}"));
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

    /// RED (U-27) — **a folder this process made may be written into; a folder
    /// whose permissions refuse it may not; a missing folder is an error, never
    /// an answer.**
    ///
    /// The read-only-mount half runs against a real read-only disk image in
    /// `bt-app`'s `update_prepare_macos` tests
    /// (`a_translocated_bundle_is_not_writable`).
    ///
    /// MUTATION: answer `Ok(true)` when `access` refuses in `may_write_into`.
    #[cfg(unix)]
    #[test]
    fn install_evidence_a_folder_may_be_written_only_where_the_system_says_so() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("writable");
        assert!(may_write_into(&dir).unwrap());
        let closed = dir.join("closed");
        std::fs::create_dir(&closed).unwrap();
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o555)).unwrap();
        // SAFETY: `geteuid` has no preconditions. Root is granted write access
        // whatever the mode says, so the refusal is only asserted for others.
        if unsafe { libc::geteuid() } != 0 {
            assert!(!may_write_into(&closed).unwrap());
        }
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(may_write_into(&dir.join("missing")).is_err());
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

    /// A key of the test's own, `HKCU\Software\Folio-test-<pid>-<ordinal>`, deleted with
    /// everything in it when dropped. Never the real uninstall key.
    #[cfg(windows)]
    struct TestRoot(String);

    #[cfg(windows)]
    impl TestRoot {
        fn new() -> Self {
            let root = format!(r"Software\{}", bt_testpath::unique_name("Folio-test"));
            assert_ne!(root, UNINSTALL_KEY);
            Self(root)
        }

        /// Write one value under `root\sub`, creating the keys on the way.
        fn set(&self, sub: &str, name: &str, kind: u32, data: &[u8]) {
            use windows::Win32::System::Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_VALUE_TYPE,
                RegCloseKey, RegCreateKeyExW, RegSetValueExW,
            };
            use windows::core::PCWSTR;
            let key: Vec<u16> = format!(r"{}\{sub}", self.0)
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            let mut opened = HKEY::default();
            // SAFETY: both names are NUL-terminated and live across the calls;
            // `opened` is closed below.
            unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    PCWSTR(key.as_ptr()),
                    None,
                    PCWSTR::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE,
                    None,
                    &raw mut opened,
                    None,
                )
                .ok()
                .unwrap();
                RegSetValueExW(
                    opened,
                    PCWSTR(name.as_ptr()),
                    None,
                    REG_VALUE_TYPE(kind),
                    Some(data),
                )
                .ok()
                .unwrap();
                let _ = RegCloseKey(opened);
            }
        }

        fn text(&self, sub: &str, name: &str, value: &str) {
            let bytes: Vec<u8> = value
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect();
            self.set(
                sub,
                name,
                windows::Win32::System::Registry::REG_SZ.0,
                &bytes,
            );
        }
    }

    #[cfg(windows)]
    impl Drop for TestRoot {
        fn drop(&mut self) {
            use windows::Win32::System::Registry::{
                HKEY_CURRENT_USER, RegDeleteKeyW, RegDeleteTreeW,
            };
            use windows::core::PCWSTR;
            let key: Vec<u16> = self.0.encode_utf16().chain(Some(0)).collect();
            // SAFETY: the name is NUL-terminated and lives across the calls.
            unsafe {
                let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()));
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()));
            }
        }
    }

    /// RED (U-4) — **the real reader lists every subkey of an uninstall key
    /// with the values asked for: a `REG_SZ` as its text, a value that is not
    /// there as absent, and a value of any other type as malformed.**
    ///
    /// The record is E2's (2026-09-27) as winget wrote it for a portable zip,
    /// under a key root the test creates (`HKCU\Software\Folio-test-<pid>`) and
    /// deletes — never the real uninstall key. A second subkey carries the
    /// names with the wrong types, and a root that is not there is no records.
    ///
    /// MUTATION: drop the `kind != REG_SZ` refusal in `record_value` — the
    /// `REG_DWORD` and `REG_EXPAND_SZ` values read as text.
    #[cfg(windows)]
    #[test]
    fn install_evidence_the_real_reader_finds_a_record_under_a_test_root() {
        use windows::Win32::System::Registry::{REG_DWORD, REG_EXPAND_SZ};
        let test = TestRoot::new();
        let uninstall = format!(r"{}\Uninstall", test.0);
        let winget = r"Uninstall\WeiyiShi.Folio__DefaultSource";
        test.text(winget, "WinGetPackageIdentifier", "WeiyiShi.Folio");
        test.text(winget, "WinGetSourceIdentifier", "*DefaultSource");
        test.text(winget, "WinGetInstallerType", "portable");
        test.text(winget, "DisplayName", "Folio");
        test.text(
            winget,
            "InstallLocation",
            r"C:\WinGet\Packages\WeiyiShi.Folio__DefaultSource",
        );
        test.set(
            winget,
            "InstallDirectoryCreated",
            REG_DWORD.0,
            &1u32.to_le_bytes(),
        );
        let odd = r"Uninstall\Another program";
        test.set(
            odd,
            "WinGetPackageIdentifier",
            REG_DWORD.0,
            &7u32.to_le_bytes(),
        );
        let expand: Vec<u8> = "%LOCALAPPDATA%\\Programs\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        test.set(odd, "InstallLocation", REG_EXPAND_SZ.0, &expand);

        let names = [
            "WinGetPackageIdentifier",
            "WinGetInstallerType",
            "InstallLocation",
            "WinGetSourceIdentifier",
        ];
        let mut records = imp::uninstall_records(&uninstall, &names).unwrap();
        records.sort_by(|one, other| one.key.cmp(&other.key));
        assert_eq!(
            records,
            vec![
                UninstallRecord {
                    key: "Another program".to_owned(),
                    values: vec![
                        RecordValue::Malformed,
                        RecordValue::Absent,
                        RecordValue::Malformed,
                        RecordValue::Absent,
                    ],
                },
                UninstallRecord {
                    key: "WeiyiShi.Folio__DefaultSource".to_owned(),
                    values: vec![
                        RecordValue::Text("WeiyiShi.Folio".to_owned()),
                        RecordValue::Text("portable".to_owned()),
                        RecordValue::Text(
                            r"C:\WinGet\Packages\WeiyiShi.Folio__DefaultSource".to_owned()
                        ),
                        RecordValue::Text("*DefaultSource".to_owned()),
                    ],
                },
            ]
        );
        assert_eq!(
            imp::uninstall_records(&format!(r"{}\Missing", test.0), &names).unwrap(),
            Vec::new()
        );
    }

    /// **Off Windows there are no uninstall records, and the door says so as
    /// an error** — so the caller never reads the silence as "no record".
    #[cfg(not(windows))]
    #[test]
    fn install_evidence_no_uninstall_records_off_windows_is_an_error() {
        let error = uninstall_records(&["InstallLocation"]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }
}
