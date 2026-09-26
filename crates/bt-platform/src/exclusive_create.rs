//! **Files created only where they did not exist, under a directory held open**
//! — the door the updater's archive reader writes a release's members through
//! (`docs/plans/design/self-update-2026-09-16.md`, revision (b), F-12; 0.4.6
//! ticket U-14).
//!
//! F-12 asks for "exclusive creation under the staging directory handle, never
//! following an existing link". That is three promises, and this module keeps
//! each by construction rather than by a check made before the act:
//!
//! * **under the directory's handle.** [`Directory::open`] opens the staging
//!   directory once and holds it, and every later create and rename names a
//!   file *relative to that handle*: `NtCreateFile` with the handle as
//!   `OBJECT_ATTRIBUTES::RootDirectory` on Windows, `openat` / `linkat` /
//!   `unlinkat` on the descriptor on Unix. A path is resolved once, at the
//!   open, so re-pointing a junction or a symlink anywhere above the staging
//!   directory afterwards does not move a single write. On Windows the handle
//!   is also held without `FILE_SHARE_DELETE`, so the directory itself cannot
//!   be renamed or removed while it is held.
//! * **never following an existing link.** The directory is opened with
//!   `FILE_FLAG_OPEN_REPARSE_POINT` / `O_NOFOLLOW` and refused if what is
//!   there is a link or a junction rather than a directory. Each file is
//!   created with `FILE_CREATE` / `O_CREAT | O_EXCL`, which fail when *any*
//!   entry of that name exists — a file, a directory, a junction, or a
//!   symlink, dangling or not — so there is nothing to follow; the create
//!   options add `FILE_OPEN_REPARSE_POINT` / `O_NOFOLLOW` as well.
//! * **exclusive.** Because the create fails on an existing name, a file this
//!   returns is one this call made. [`Directory::rename_new`] keeps the same
//!   rule for the one rename a reader needs — a finished member moved from its
//!   temporary name to its final one — and never replaces an existing name.
//!
//! Every name is **one component**: not empty, not `.` or `..`, with no
//! separator, no `:` (which on NTFS names a stream of another file) and no NUL.
//! A name outside that is [`io::ErrorKind::InvalidInput`]; deciding which names
//! an archive may carry is the reader's grammar, not this door's.
//!
//! Worker only: a create is a disk write. Nothing here decides anything; each
//! failure is the operating system's, with its kind (`AlreadyExists` for a
//! name that exists).

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

/// **A directory held open**, which files are created in by handle.
#[derive(Debug)]
pub struct Directory {
    path: PathBuf,
    held: arm::Held,
}

impl Directory {
    /// **Open the directory at `path` and hold it.**
    ///
    /// # Errors
    ///
    /// The operating system's error when it cannot be opened;
    /// [`io::ErrorKind::InvalidInput`] when what stands at `path` is a link, a
    /// junction or another reparse point, or is not a directory.
    pub fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            held: arm::open_directory(path)?,
        })
    }

    /// The path the directory was opened at. A reader that needs to hand a
    /// file inside it to another API (the manifest read, U-14) names it by
    /// this path joined with the file's name.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// **Create `name` in the directory, for writing, only if nothing of that
    /// name exists.**
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::AlreadyExists`] when an entry of that name — of any
    /// kind — exists; [`io::ErrorKind::InvalidInput`] when `name` is not one
    /// component; otherwise the operating system's error.
    pub fn create_new(&self, name: &str) -> io::Result<File> {
        one_component(name)?;
        arm::create_new(&self.held, name)
    }

    /// **Rename `from` to `to` inside the directory, only if nothing is
    /// called `to`.** `from` is opened without following a link.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::AlreadyExists`] when `to` exists;
    /// [`io::ErrorKind::InvalidInput`] when either name is not one component;
    /// otherwise the operating system's error.
    pub fn rename_new(&self, from: &str, to: &str) -> io::Result<()> {
        one_component(from)?;
        one_component(to)?;
        arm::rename_new(&self.held, from, to)
    }
}

/// `name` is one component of a path, spelled the same on every platform.
fn one_component(name: &str) -> io::Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':', '\0']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("`{name}` is not one component of a path"),
        ));
    }
    Ok(())
}

/// **The Windows arm**: `CreateFileW` for the directory, `NtCreateFile`
/// relative to it for each file, and `NtSetInformationFile` renaming relative
/// to it.
#[cfg(windows)]
mod arm {
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::path::Path;
    use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows::Wdk::Storage::FileSystem::{
        FILE_CREATE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
        FILE_RENAME_INFORMATION, FILE_SYNCHRONOUS_IO_NONALERT, FileRenameInformation,
        NTCREATEFILE_CREATE_DISPOSITION, NTCREATEFILE_CREATE_OPTIONS, NtCreateFile,
        NtSetInformationFile,
    };
    use windows::Win32::Foundation::{
        HANDLE, NTSTATUS, OBJ_CASE_INSENSITIVE, RtlNtStatusToDosError,
        STATUS_OBJECT_NAME_COLLISION, UNICODE_STRING,
    };
    use windows::Win32::Storage::FileSystem::{
        DELETE, FILE_ACCESS_RIGHTS, FILE_ADD_FILE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_GENERIC_WRITE, FILE_LIST_DIRECTORY, FILE_SHARE_MODE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FILE_TRAVERSE, SYNCHRONIZE,
    };
    use windows::Win32::System::IO::IO_STATUS_BLOCK;
    use windows::core::PWSTR;

    /// The directory's handle. Opened without `FILE_SHARE_DELETE`, so while it
    /// is held the directory cannot be renamed or removed.
    #[derive(Debug)]
    pub(super) struct Held(File);

    pub(super) fn open_directory(path: &Path) -> io::Result<Held> {
        let directory = OpenOptions::new()
            .access_mode((FILE_LIST_DIRECTORY | FILE_ADD_FILE | FILE_TRAVERSE | SYNCHRONIZE).0)
            .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE).0)
            .custom_flags((FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT).0)
            .open(path)?;
        let attributes = directory.metadata()?.file_attributes();
        if attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            || attributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is a link or not a directory", path.display()),
            ));
        }
        Ok(Held(directory))
    }

    /// `NtCreateFile` for `name` relative to the held directory, never
    /// following a reparse point.
    fn nt_create(
        directory: &Held,
        name: &str,
        access: FILE_ACCESS_RIGHTS,
        share: FILE_SHARE_MODE,
        disposition: NTCREATEFILE_CREATE_DISPOSITION,
        options: NTCREATEFILE_CREATE_OPTIONS,
    ) -> io::Result<OwnedHandle> {
        let mut wide: Vec<u16> = std::ffi::OsStr::new(name).encode_wide().collect();
        let bytes = u16::try_from(wide.len() * 2)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a name too long"))?;
        let object_name = UNICODE_STRING {
            Length: bytes,
            MaximumLength: bytes,
            Buffer: PWSTR(wide.as_mut_ptr()),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: u32::try_from(std::mem::size_of::<OBJECT_ATTRIBUTES>())
                .expect("OBJECT_ATTRIBUTES is a few dozen bytes"),
            RootDirectory: HANDLE(directory.0.as_raw_handle()),
            ObjectName: &raw const object_name,
            // What Win32 asks for: names compare as Windows compares them, so
            // `CONPTY.DLL` collides with `conpty.dll`.
            Attributes: OBJ_CASE_INSENSITIVE,
            ..OBJECT_ATTRIBUTES::default()
        };
        let mut handle = HANDLE::default();
        let mut status_block = IO_STATUS_BLOCK::default();
        // SAFETY: every pointer is to a local that outlives the call; the name
        // buffer is `wide`, whose length in bytes is `Length`; the root is the
        // held directory's live handle. `handle` is written only on success.
        let status = unsafe {
            NtCreateFile(
                &raw mut handle,
                access | SYNCHRONIZE,
                &raw const attributes,
                &raw mut status_block,
                None,
                FILE_ATTRIBUTE_NORMAL,
                share,
                disposition,
                options | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
                None,
                0,
            )
        };
        status_error(status, name)?;
        // SAFETY: `NtCreateFile` succeeded, so `handle` is a new handle this
        // function owns and hands to `OwnedHandle`, which closes it once.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
    }

    pub(super) fn create_new(directory: &Held, name: &str) -> io::Result<File> {
        // No `FILE_NON_DIRECTORY_FILE`: without `FILE_DIRECTORY_FILE` the
        // create makes a file, and an existing directory of the name is then
        // the same collision as an existing file rather than a refusal of
        // another kind.
        nt_create(
            directory,
            name,
            FILE_GENERIC_WRITE,
            FILE_SHARE_READ,
            FILE_CREATE,
            NTCREATEFILE_CREATE_OPTIONS(0),
        )
        .map(File::from)
    }

    pub(super) fn rename_new(directory: &Held, from: &str, to: &str) -> io::Result<()> {
        let file = nt_create(
            directory,
            from,
            DELETE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_OPEN,
            FILE_NON_DIRECTORY_FILE,
        )?;
        let name: Vec<u16> = std::ffi::OsStr::new(to).encode_wide().collect();
        let header = std::mem::offset_of!(FILE_RENAME_INFORMATION, FileName);
        let length = header + name.len() * 2;
        // `u64` words, so the buffer is aligned for the structure's handle.
        let mut buffer = vec![0u64; length.div_ceil(8) + 1];
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        // SAFETY: `buffer` is at least `length` bytes and 8-byte aligned, which
        // is the structure's alignment; the name is copied into the space
        // after the fixed part, which `FileNameLength` describes in bytes.
        unsafe {
            (*info).Anonymous.ReplaceIfExists = false;
            // The new name is relative to the held directory, as every create
            // is. (`SetFileInformationByHandle` resolves a relative name
            // against the process's current directory instead, which is why
            // this is the native call.)
            (*info).RootDirectory = HANDLE(directory.0.as_raw_handle());
            (*info).FileNameLength = u32::try_from(name.len() * 2)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a name too long"))?;
            std::ptr::copy_nonoverlapping(
                name.as_ptr(),
                buffer.as_mut_ptr().cast::<u8>().add(header).cast::<u16>(),
                name.len(),
            );
        }
        let mut status_block = IO_STATUS_BLOCK::default();
        // SAFETY: the handle is live and opened with `DELETE`; the buffer is
        // the structure built above, `length` bytes long; the root is the held
        // directory's live handle.
        let status = unsafe {
            NtSetInformationFile(
                HANDLE(file.as_raw_handle()),
                &raw mut status_block,
                info.cast_const().cast(),
                u32::try_from(length).expect("a name of one component"),
                FileRenameInformation,
            )
        };
        status_error(status, to)
    }

    /// An `NTSTATUS` as `io::Error`: a collision is `AlreadyExists` naming
    /// `name`, anything else the Win32 code it translates to.
    fn status_error(status: NTSTATUS, name: &str) -> io::Result<()> {
        if status == STATUS_OBJECT_NAME_COLLISION {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{name} already exists"),
            ));
        }
        if status.is_err() {
            // SAFETY: a pure translation of a status code.
            let code = unsafe { RtlNtStatusToDosError(status) };
            return Err(io::Error::from_raw_os_error(code.cast_signed()));
        }
        Ok(())
    }
}

/// **The Unix arm**: a descriptor for the directory and the `*at` calls on it.
#[cfg(unix)]
mod arm {
    use std::ffi::CString;
    use std::fs::File;
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    #[derive(Debug)]
    pub(super) struct Held(OwnedFd);

    fn c_string(bytes: &[u8]) -> io::Result<CString> {
        CString::new(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in a name"))
    }

    pub(super) fn open_directory(path: &Path) -> io::Result<Held> {
        let path = c_string(path.as_os_str().as_bytes())?;
        // SAFETY: a NUL-terminated path; the descriptor, if any, is owned below.
        let fd = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            let error = io::Error::last_os_error();
            // `O_NOFOLLOW` on a symlink is `ELOOP`; `O_DIRECTORY` on anything
            // else is `ENOTDIR`. Both are "not a directory we will hold".
            return Err(
                if matches!(error.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR)) {
                    io::Error::new(io::ErrorKind::InvalidInput, "a link or not a directory")
                } else {
                    error
                },
            );
        }
        // SAFETY: `fd` is a new descriptor this function owns.
        Ok(Held(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    pub(super) fn create_new(directory: &Held, name: &str) -> io::Result<File> {
        let name = c_string(name.as_bytes())?;
        // SAFETY: a live directory descriptor and a NUL-terminated name.
        let fd = unsafe {
            libc::openat(
                directory.0.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o644 as libc::c_uint,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` is a new descriptor this function owns.
        Ok(File::from(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    pub(super) fn rename_new(directory: &Held, from: &str, to: &str) -> io::Result<()> {
        let from = c_string(from.as_bytes())?;
        let to = c_string(to.as_bytes())?;
        let at = directory.0.as_raw_fd();
        // A new name that fails when `to` exists, then the old name removed —
        // POSIX's rename that never replaces, the same on Linux and macOS.
        // SAFETY: a live descriptor and two NUL-terminated names; flags 0 does
        // not follow `from` if it were a link.
        if unsafe { libc::linkat(at, from.as_ptr(), at, to.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: as above.
        if unsafe { libc::unlinkat(at, from.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// **Neither**: no build of Folio runs here, and the door says so.
#[cfg(not(any(windows, unix)))]
mod arm {
    use std::fs::File;
    use std::io;
    use std::path::Path;

    #[derive(Debug)]
    pub(super) struct Held;

    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "bt_platform::exclusive_create has no arm on this platform",
        )
    }

    pub(super) fn open_directory(_: &Path) -> io::Result<Held> {
        Err(unsupported())
    }

    pub(super) fn create_new(_: &Held, _: &str) -> io::Result<File> {
        Err(unsupported())
    }

    pub(super) fn rename_new(_: &Held, _: &str, _: &str) -> io::Result<()> {
        Err(unsupported())
    }
}

#[cfg(test)]
#[path = "exclusive_create_tests.rs"]
mod tests;
