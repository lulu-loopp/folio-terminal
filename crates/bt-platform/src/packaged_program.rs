//! **Which installed package a program belongs to** (T-CHECK-NEVER-OVERLAPS-A-PACKAGED-BIRTH).
//!
//! Windows runs every process of one packaged app that an unpackaged parent starts — PowerShell 7
//! from the Microsoft Store is one — in one *desktop app container* per package and account: the
//! first such process creates it, every later one joins it while it lives, and it is destroyed
//! with its members (Microsoft-Windows-AppModel-Runtime events 210, 211 and 217 name it by the
//! package). So the identity two programs share a container by is their package, not their path:
//! the Store's app execution alias (`%LocalAppData%\Microsoft\WindowsApps\pwsh.exe`) and the
//! package's own `pwsh.exe` are one package, and PowerShell 7 and PowerShell 7 Preview are two.
//!
//! [`package_family_of`] answers it from what Windows itself keeps, never from how a path is
//! spelled: an alias is a reparse point whose data names its package family; any other program is
//! in a package when an ancestor directory is the one Windows says an installed package of that
//! name lives in. Off Windows there are no packages and every answer is `None`.

/// **A package family** — a packaged app's identity across its versions
/// (`Microsoft.PowerShell_8wekyb3d8bbwe`), held lowercased, as Windows compares it.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PackageFamily(String);

impl PackageFamily {
    /// The family named `name`, in any case.
    #[must_use]
    pub fn named(name: &str) -> Self {
        Self(name.to_lowercase())
    }

    /// The family's name, lowercased.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.0
    }
}

/// **The package `program` belongs to**, or `None` for a program that is in no package (or that
/// is not there). `program` is an absolute path. Reads the file system (one reparse-point read,
/// and one package lookup per ancestor directory whose name is a package's full name), so it is
/// asked on a worker.
///
/// Two ways in, each Windows' own record:
/// - **An app execution alias** — a reparse point tagged `IO_REPARSE_TAG_APPEXECLINK`, which is
///   what `CreateProcess` follows into the package. Its data (version 3) is a run of
///   NUL-terminated strings, the first of them the package family the alias starts.
/// - **A file inside an installed package**: an ancestor directory whose name is a package full
///   name (`PackageFamilyNameFromFullName` accepts it) and which is the very directory
///   `GetPackagePathByFullName` says that package is installed in. A directory that only looks
///   like one is not.
///
/// What it does not see: a program inside a package registered from a development folder whose
/// name is not the package's full name, reached by its path rather than through its alias; and a
/// symbolic link a person made to the alias or to the package's `pwsh.exe`, which is read as the
/// link it is, never followed. A `\\?\` verbatim spelling is compared as the plain one.
#[cfg(windows)]
#[must_use]
pub fn package_family_of(program: &std::path::Path) -> Option<PackageFamily> {
    windows_impl::alias_family(program).or_else(|| windows_impl::installed_family(program))
}

/// Off Windows no program is in a package.
#[cfg(not(windows))]
#[must_use]
pub fn package_family_of(program: &std::path::Path) -> Option<PackageFamily> {
    let _ = program;
    None
}

#[cfg(windows)]
mod windows_impl {
    use super::PackageFamily;
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
    use std::path::{Component, Path};
    use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::Storage::Packaging::Appx::{
        GetPackageFamilyName, GetPackagePathByFullName, PackageFamilyNameFromFullName,
    };
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::Win32::System::Ioctl::FSCTL_GET_REPARSE_POINT;
    use windows::Win32::System::SystemServices::IO_REPARSE_TAG_APPEXECLINK;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::core::{PCWSTR, PWSTR};

    /// The most a reparse point's data can be (`MAXIMUM_REPARSE_DATA_BUFFER_SIZE`).
    const REPARSE_ROOM: usize = 16 * 1024;
    /// The layout of an app execution alias's data this reader knows.
    const APPEXECLINK_VERSION: u32 = 3;

    /// The family an app execution alias at `program` names; `None` for anything else.
    pub(super) fn alias_family(program: &Path) -> Option<PackageFamily> {
        let data = reparse_data(program)?;
        appexeclink_family(&data)
    }

    /// The reparse point at `path` itself — tag and data, never followed — or `None` when there is
    /// none (a plain file answers `ERROR_NOT_A_REPARSE_POINT`).
    pub(super) fn reparse_data(path: &Path) -> Option<Vec<u8>> {
        let wide = crate::wide_with_nul(path.as_os_str()).ok()?;
        // SAFETY: `wide` is NUL-terminated and outlives the call; the handle moves to
        // `OwnedHandle` at once. No access is asked: the control code below needs none, and
        // `FILE_FLAG_OPEN_REPARSE_POINT` opens the link rather than what it leads to.
        let file = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        }
        .ok()?;
        // SAFETY: as above.
        let file = unsafe { OwnedHandle::from_raw_handle(file.0) };
        let mut data = vec![0_u8; REPARSE_ROOM];
        let mut written = 0_u32;
        // SAFETY: the handle is live; `data` is writable storage of the length the call is told,
        // and `written` receives how much of it was filled.
        unsafe {
            DeviceIoControl(
                HANDLE(file.as_raw_handle()),
                FSCTL_GET_REPARSE_POINT,
                None,
                0,
                Some(data.as_mut_ptr().cast()),
                u32::try_from(data.len()).expect("the reparse room fits u32"),
                Some(&raw mut written),
                None,
            )
        }
        .ok()?;
        data.truncate(usize::try_from(written).ok()?);
        Some(data)
    }

    /// The family in a reparse point's bytes (`REPARSE_DATA_BUFFER`: the tag, the data's length
    /// and a reserved word, then the data) when it is an app execution alias of the known layout.
    pub(super) fn appexeclink_family(buffer: &[u8]) -> Option<PackageFamily> {
        let word = |at: usize| -> Option<u32> {
            Some(u32::from_le_bytes(buffer.get(at..at + 4)?.try_into().ok()?))
        };
        if word(0)? != IO_REPARSE_TAG_APPEXECLINK {
            return None;
        }
        let length = usize::from(u16::from_le_bytes(buffer.get(4..6)?.try_into().ok()?));
        let data = buffer.get(8..8 + length)?;
        if u32::from_le_bytes(data.get(..4)?.try_into().ok()?) != APPEXECLINK_VERSION {
            return None;
        }
        let units: Vec<u16> = data[4..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let end = units.iter().position(|unit| *unit == 0)?;
        let family = String::from_utf16(&units[..end]).ok()?;
        (!family.is_empty()).then(|| PackageFamily::named(&family))
    }

    /// The family of the installed package one of `program`'s ancestor directories is, if any.
    pub(super) fn installed_family(program: &Path) -> Option<PackageFamily> {
        program.ancestors().skip(1).find_map(|directory| {
            let full_name = directory.file_name()?.to_str()?;
            let family = family_of_full_name(full_name)?;
            let installed = installed_path(full_name)?;
            same_directory(directory, Path::new(&installed)).then_some(family)
        })
    }

    /// The family of the running process `id`, asked of its own token.
    pub(super) fn process_family(id: u32) -> Option<PackageFamily> {
        // SAFETY: opens a process for a limited query; the handle moves to `OwnedHandle` at once.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, id) }.ok()?;
        // SAFETY: as above.
        let process = unsafe { OwnedHandle::from_raw_handle(process.0) };
        let name = two_calls(|length, out| {
            // SAFETY: the handle is live; `out` is `None` or writable storage of `*length` units.
            unsafe { GetPackageFamilyName(HANDLE(process.as_raw_handle()), length, out) }
        })?;
        Some(PackageFamily::named(&name))
    }

    /// The family of a package full name, or `None` when `full_name` is no full name.
    fn family_of_full_name(full_name: &str) -> Option<PackageFamily> {
        let wide = crate::wide_with_nul(std::ffi::OsStr::new(full_name)).ok()?;
        let name = two_calls(|length, out| {
            // SAFETY: `wide` is NUL-terminated; `out` is `None` or writable storage of `*length`
            // units, which is what the call is told.
            unsafe { PackageFamilyNameFromFullName(PCWSTR(wide.as_ptr()), length, out) }
        })?;
        Some(PackageFamily::named(&name))
    }

    /// Where the installed package `full_name` lives, or `None` when no such package is installed.
    fn installed_path(full_name: &str) -> Option<String> {
        let wide = crate::wide_with_nul(std::ffi::OsStr::new(full_name)).ok()?;
        two_calls(|length, out| {
            // SAFETY: as in `family_of_full_name`.
            unsafe { GetPackagePathByFullName(PCWSTR(wide.as_ptr()), length, out) }
        })
    }

    /// A string from an API that is asked its length first (no buffer: `ERROR_INSUFFICIENT_BUFFER`
    /// with the length, its NUL included), then asked again with room for it.
    fn two_calls(
        mut call: impl FnMut(*mut u32, Option<PWSTR>) -> windows::Win32::Foundation::WIN32_ERROR,
    ) -> Option<String> {
        let mut length = 0_u32;
        if call(&raw mut length, None) != ERROR_INSUFFICIENT_BUFFER {
            return None;
        }
        let mut units = vec![0_u16; usize::try_from(length).ok()?];
        if call(&raw mut length, Some(PWSTR(units.as_mut_ptr()))) != ERROR_SUCCESS {
            return None;
        }
        let end = units
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(units.len());
        String::from_utf16(&units[..end]).ok()
    }

    /// Whether two absolute directories are one, component by component, in Windows' case rule;
    /// a `\\?\` verbatim spelling is read as the plain one.
    fn same_directory(left: &Path, right: &Path) -> bool {
        let parts = |path: &Path| -> Vec<String> {
            crate::handoff::strip_verbatim_prefix(path)
                .components()
                .filter(|component| !matches!(component, Component::CurDir))
                .map(|component| component.as_os_str().to_string_lossy().to_lowercase())
                .collect()
        };
        parts(left) == parts(right)
    }
}

/// **The package a running process belongs to**, asked of the process itself
/// (`GetPackageFamilyName`); `None` for a process in no package, one that has exited, and one
/// this process may not ask.
#[cfg(windows)]
pub(crate) fn package_family_of_process(id: u32) -> Option<PackageFamily> {
    windows_impl::process_family(id)
}

#[cfg(all(test, windows))]
#[path = "packaged_program_tests_windows.rs"]
mod tests;
