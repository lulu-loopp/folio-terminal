//! **The effects of an update transaction: durable writes, durable moves, the
//! registry flush and the two locks** (0.4.6 ticket U-11;
//! `docs/plans/design/self-update-2026-09-16.md` revision (b), §(b).2
//! "Durability", F-3, F-6 and experiment E-15).
//!
//! `bt-app`'s `update_txn` says *what* a transaction records and *who* may do
//! what in which phase; it has no effects. This door is the effects, and only
//! them: which journal state is written when is the caller's (U-12, U-13 and
//! the job tickets after them), never this module's.
//!
//! * **A durable write** of a small file ([`durable_write`]): a temporary name
//!   in the same directory, the bytes, a flush of the file (`FlushFileBuffers`
//!   / `F_FULLFSYNC`), an atomic rename over the target, then a flush of the
//!   directory (Windows: `FlushFileBuffers` on a directory handle opened with
//!   `FILE_FLAG_BACKUP_SEMANTICS`; macOS: `F_FULLFSYNC` on the directory's
//!   descriptor). It returns only after the directory flush. A reader after a
//!   power cut finds the old file or the new one, whole, and never a torn one.
//! * **A durable create** ([`durable_create`]): the same steps, with a rename
//!   that never replaces — the trial's receipt (U-13), written once and never
//!   over an existing one.
//! * **A durable move** ([`durable_move`]): Windows `MoveFileExW(…,
//!   MOVEFILE_WRITE_THROUGH)`, macOS `renamex_np(…, RENAME_EXCL)`; then the
//!   same directory flush, on the destination's directory and on the source's
//!   when it is another one. **A move never replaces a file** (see
//!   [`durable_move`]).
//! * **A durable remove** ([`durable_remove`]): a file or a whole directory
//!   tree is removed, then the directory it was in is flushed, so a removal the
//!   caller orders before another one reaches the device first (a retired
//!   transaction's folder before its journal, U-12). Nothing there already is
//!   an answer, not a failure.
//! * **The registry flush** (Windows only, [`flush_current_user_key`]):
//!   `RegFlushKey` on a key under `HKEY_CURRENT_USER`, for the entrance value a
//!   later ticket writes (U-22).
//! * **The two locks** ([`try_hold`], [`hold_within`]): a byte-range lock
//!   (`LockFileEx`) on Windows, `flock` on macOS, held by a [`Held`] whose drop
//!   releases it. The admission file `H\admission` is held [`Hold::Shared`] by
//!   every running copy of the install (many holders; the file is opened for
//!   reading only, because F-6 lets any account that may run `folio.exe` there
//!   take part, and such an account has read access through the inherited ACL)
//!   and [`Hold::Exclusive`] by the applier while files move (refused while any
//!   shared holder exists, and it refuses new shared holders while held). The
//!   transaction lock `H\lock` is the same primitive held exclusive.
//!
//! **Three arms.** Windows and macOS are real. Every other platform has no arm:
//! each call is refused with an error that names this door and the operation
//! (`io::ErrorKind::Unsupported`), never answered as if it had happened.
//!
//! **The OS calls come through a small trait, `Surface`,** so the one order
//! that makes a write durable — write, flush, rename, directory flush — is
//! asserted over a recording fake as well as run for real.
//!
//! **Worker only.** Every call here blocks on the disk (a flush waits for the
//! device), and [`hold_within`] sleeps until its deadline. None of it may run
//! on a window thread. Today that is this sentence; the thread door's
//! `WorkerCtx` (A1b) and its prohibitions (A1e) are what will make it a type.
//! **The one exception is the start** (U-12, `bt-app::update_startup`): in
//! `fn main`, before the event loop exists, the window thread takes the
//! admission and asks for the transaction lock with [`try_hold`] (never
//! [`hold_within`]) and retires a finished transaction with
//! [`durable_remove`] — the design's startup-recovery row, whose reason is
//! §5.3 row 18's: there is no loop yet to be blocked.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// **The step of an effect that failed.** Every failure of this door names
/// one, so a journal that did not become durable says where it stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Creating the temporary file beside the target.
    CreateTemp,
    /// Writing the bytes into the temporary file.
    Write,
    /// Flushing the temporary file to the device.
    FlushFile,
    /// The rename (a durable write's) or the move (a durable move's).
    Rename,
    /// Opening a directory to flush it.
    OpenDirectory,
    /// Flushing a directory to the device.
    FlushDirectory,
    /// Opening a lock file.
    OpenLockFile,
    /// Asking for the lock itself (never "somebody else holds it": that is an
    /// answer, not a failure).
    Lock,
    /// Opening a registry key.
    OpenKey,
    /// Flushing a registry key.
    FlushKey,
    /// Removing a file or a directory tree (a durable remove's).
    Remove,
}

impl Stage {
    /// The stage's name, as a failure prints it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CreateTemp => "create-temp",
            Self::Write => "write",
            Self::FlushFile => "flush-file",
            Self::Rename => "rename",
            Self::OpenDirectory => "open-directory",
            Self::FlushDirectory => "flush-directory",
            Self::OpenLockFile => "open-lock-file",
            Self::Lock => "lock",
            Self::OpenKey => "open-key",
            Self::FlushKey => "flush-key",
            Self::Remove => "remove",
        }
    }
}

/// **An effect that did not happen, with the step it stopped at**, the path
/// that step was acting on, and the operating system's error.
#[derive(Debug)]
pub struct Failure {
    pub stage: Stage,
    pub path: PathBuf,
    pub error: io::Error,
}

impl Failure {
    fn at(stage: Stage, path: &Path, error: io::Error) -> Self {
        Self {
            stage,
            path: path.to_path_buf(),
            error,
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "install_txn {} {}: {}",
            self.stage.name(),
            self.path.display(),
            self.error
        )
    }
}

impl std::error::Error for Failure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// Whether a rename may take the name of a file that is already there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Replace {
    /// A durable write: the new journal takes the old one's name.
    Existing,
    /// A durable move or a durable create: an existing destination is a refusal.
    Never,
}

/// **The operating system calls a durable write and a durable move are made
/// of.** One implementation per arm (`arm::Os`), and a recording fake in the
/// tests, so the order is a fact a test reads rather than a comment.
trait Surface {
    type Handle;
    /// Create `path`, which must not exist, for writing.
    fn create_new(&mut self, path: &Path) -> io::Result<Self::Handle>;
    fn write_all(&mut self, handle: &mut Self::Handle, bytes: &[u8]) -> io::Result<()>;
    /// Flush everything written through `handle` to the device:
    /// `FlushFileBuffers` on Windows, `F_FULLFSYNC` on macOS — for a file and
    /// for a directory handle alike.
    fn flush(&mut self, handle: &mut Self::Handle) -> io::Result<()>;
    fn close(&mut self, handle: Self::Handle);
    /// Open a directory so that it can be flushed (Windows: write access and
    /// `FILE_FLAG_BACKUP_SEMANTICS`; macOS: a read-only descriptor).
    fn open_directory(&mut self, path: &Path) -> io::Result<Self::Handle>;
    fn rename(&mut self, from: &Path, to: &Path, replace: Replace) -> io::Result<()>;
    /// Remove a temporary file this door made, on a failure. Best effort: the
    /// failure being reported is the one that matters.
    fn remove(&mut self, path: &Path);
    /// Remove `path` — a file, or a directory with everything in it. Nothing
    /// at `path` is `NotFound`.
    fn remove_entry(&mut self, path: &Path) -> io::Result<()>;
}

/// **Write `bytes` to `target` so that after a power cut `target` holds either
/// its old bytes or these, whole** — §(b).2's durable journal write.
///
/// The temporary file is `.<name>.<pid>.<n>.tmp` beside the target, so the
/// rename never crosses a volume. It is removed on every failure before the
/// rename takes it. `target` is replaced if it exists. A failure at
/// [`Stage::FlushDirectory`] or [`Stage::OpenDirectory`] comes after the rename:
/// the new bytes are in place but not known to be durable, and the caller must
/// not act as if they were.
///
/// # Errors
/// A [`Failure`] naming the stage that failed; on a platform with no arm, one
/// at [`Stage::CreateTemp`] whose error is `Unsupported` and names this door.
pub fn durable_write(target: &Path, bytes: &[u8]) -> Result<(), Failure> {
    durable_write_with(&mut arm::Os, target, bytes, Replace::Existing)
}

/// **Write `bytes` to `target` durably, and only if nothing is there yet** —
/// the trial's receipt (F-14: "create-new, flush file and directory"; U-13).
///
/// [`durable_write`]'s steps in its order — a temporary beside the target, the
/// bytes, a flush of the file, a rename, a flush of the directory — with the
/// one difference that the rename **never replaces**: a target that already
/// exists is refused at [`Stage::Rename`] with the operating system's "already
/// exists" error, the existing file is left byte for byte, and the temporary is
/// removed. So a receipt, once written, is never written over.
///
/// # Errors
/// A [`Failure`] naming the stage that failed; on a platform with no arm, one
/// at [`Stage::CreateTemp`] whose error is `Unsupported` and names this door.
pub fn durable_create(target: &Path, bytes: &[u8]) -> Result<(), Failure> {
    durable_write_with(&mut arm::Os, target, bytes, Replace::Never)
}

/// **Move `from` to `to`, durably, and never over an existing file.**
///
/// The contract: a move in a transaction takes a file from one place to
/// another (install → backup, set → install, and back on a rollback), and the
/// recovery invariant is that every file is in exactly one place (I1′). A
/// destination that already exists would be a file the inventories count
/// being destroyed by a move, so it is refused at [`Stage::Rename`] with the
/// operating system's "already exists" error and neither file is changed.
/// Replacing a file whole is what [`durable_write`] is for.
///
/// After the rename the destination's directory is flushed, and the source's
/// directory too when it is another one: a rename changes both.
///
/// # Errors
/// A [`Failure`] naming the stage that failed; on a platform with no arm, one
/// at [`Stage::Rename`] whose error is `Unsupported` and names this door.
pub fn durable_move(from: &Path, to: &Path) -> Result<(), Failure> {
    durable_move_with(&mut arm::Os, from, to)
}

/// **Remove `path` — a file or a directory tree — and flush the directory it
/// was in**, so that after a power cut the removal is either on the device or
/// the entry is whole where it was, and a later removal the caller makes is
/// never on the device before this one.
///
/// The contract that makes a retirement safe to repeat: **nothing at `path`
/// is success** (the directory is still flushed, because an earlier removal
/// may not have been). A tree that cannot be removed whole — a running
/// executable inside it on Windows — is a failure at [`Stage::Remove`], and
/// whatever part of it was removed stays removed; the caller removes nothing
/// that depended on it and tries again at the next start.
///
/// # Errors
/// A [`Failure`] at [`Stage::Remove`], [`Stage::OpenDirectory`] or
/// [`Stage::FlushDirectory`]; on a platform with no arm, one at
/// [`Stage::Remove`] whose error is `Unsupported` and names this door.
pub fn durable_remove(path: &Path) -> Result<(), Failure> {
    durable_remove_with(&mut arm::Os, path)
}

/// A temporary file's number within this process, so two writes to one
/// target from two threads never share a temporary name.
static TEMP_SERIAL: AtomicU64 = AtomicU64::new(0);

/// The directory a path is in; `.` for a bare file name.
fn directory_of(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

fn temporary_beside(target: &Path) -> io::Result<PathBuf> {
    let name = target.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "a durable write needs a file name",
        )
    })?;
    let serial = TEMP_SERIAL.fetch_add(1, Ordering::Relaxed);
    let mut temporary = std::ffi::OsString::from(".");
    temporary.push(name);
    temporary.push(format!(".{}.{serial}.tmp", std::process::id()));
    Ok(directory_of(target).join(temporary))
}

fn durable_write_with<S: Surface>(
    surface: &mut S,
    target: &Path,
    bytes: &[u8],
    replace: Replace,
) -> Result<(), Failure> {
    let temporary =
        temporary_beside(target).map_err(|error| Failure::at(Stage::CreateTemp, target, error))?;
    let mut file = surface
        .create_new(&temporary)
        .map_err(|error| Failure::at(Stage::CreateTemp, &temporary, error))?;
    let written = match surface.write_all(&mut file, bytes) {
        Err(error) => Err(Failure::at(Stage::Write, &temporary, error)),
        Ok(()) => surface
            .flush(&mut file)
            .map_err(|error| Failure::at(Stage::FlushFile, &temporary, error)),
    };
    surface.close(file);
    if let Err(failure) = written {
        surface.remove(&temporary);
        return Err(failure);
    }
    if let Err(error) = surface.rename(&temporary, target, replace) {
        surface.remove(&temporary);
        return Err(Failure::at(Stage::Rename, target, error));
    }
    flush_directory(surface, directory_of(target))
}

fn durable_move_with<S: Surface>(surface: &mut S, from: &Path, to: &Path) -> Result<(), Failure> {
    surface
        .rename(from, to, Replace::Never)
        .map_err(|error| Failure::at(Stage::Rename, to, error))?;
    let destination = directory_of(to);
    flush_directory(surface, destination)?;
    let source = directory_of(from);
    if source != destination {
        flush_directory(surface, source)?;
    }
    Ok(())
}

fn durable_remove_with<S: Surface>(surface: &mut S, path: &Path) -> Result<(), Failure> {
    match surface.remove_entry(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(Failure::at(Stage::Remove, path, error)),
    }
    flush_directory(surface, directory_of(path))
}

fn flush_directory<S: Surface>(surface: &mut S, directory: &Path) -> Result<(), Failure> {
    let mut handle = surface
        .open_directory(directory)
        .map_err(|error| Failure::at(Stage::OpenDirectory, directory, error))?;
    let flushed = surface.flush(&mut handle);
    surface.close(handle);
    flushed.map_err(|error| Failure::at(Stage::FlushDirectory, directory, error))
}

/// **Flush a key under `HKEY_CURRENT_USER` to disk** (`RegFlushKey`).
///
/// A registry write is visible to every reader at once and reaches the disk
/// later; §(b).2 (F-2) requires the entrance value to be on disk before the
/// journal records `Armed`. The caller writes the value and then calls this
/// with the same key's path relative to `HKEY_CURRENT_USER`. The key is opened
/// for reading only; the flush writes nothing of its own.
///
/// # Errors
/// A [`Failure`] at [`Stage::OpenKey`] (a missing key is `NotFound`) or at
/// [`Stage::FlushKey`]; its path is `HKCU\<subkey>`.
#[cfg(windows)]
pub fn flush_current_user_key(subkey: &str) -> Result<(), Failure> {
    arm::flush_current_user_key(subkey)
}

/// **The two ways the lock files are held.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    /// A running copy's hold on `admission`: many holders at once; the file
    /// is opened for reading only and must already exist (a copy started
    /// before the installation home existed holds nothing).
    Shared,
    /// The applier's hold on `admission` while files move, and every holder's
    /// hold on the transaction lock: one holder, and none while any shared
    /// holder exists. The file is opened for reading and writing and created
    /// if it is missing.
    Exclusive,
}

/// **A lock held on one file**, released when this value is dropped or when
/// the process ends, whichever comes first.
///
/// The operating system owns the lock; it is a property of the open handle
/// (Windows) or open file description (macOS), never of the file's contents,
/// so nothing is ever written into the file and a holder that was killed
/// leaves nothing to clean up.
#[derive(Debug)]
pub struct Held {
    hold: Hold,
    file: std::fs::File,
}

impl Held {
    /// Which way this lock is held.
    #[must_use]
    pub const fn hold(&self) -> Hold {
        self.hold
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        arm::unlock(&self.file);
    }
}

/// How often [`hold_within`] asks again while somebody else holds the lock.
pub const LOCK_POLL: Duration = Duration::from_millis(25);

/// **Take the lock on `path` now, or answer `Ok(None)` because somebody else
/// holds it in a way this hold cannot share** (`LOCKFILE_FAIL_IMMEDIATELY` /
/// `LOCK_NB`).
///
/// # Errors
/// A [`Failure`] at [`Stage::OpenLockFile`] (for [`Hold::Shared`], a missing
/// file is `NotFound`) or at [`Stage::Lock`]; on a platform with no arm, one at
/// [`Stage::OpenLockFile`] whose error is `Unsupported` and names this door.
pub fn try_hold(path: &Path, hold: Hold) -> Result<Option<Held>, Failure> {
    hold_until(path, hold, None)
}

/// **The same, asking again every [`LOCK_POLL`] until `wait` has passed** —
/// the applier's bounded wait for the running copies to quit. `Ok(None)` means
/// the lock was still held by somebody else at the deadline.
///
/// # Errors
/// As [`try_hold`].
pub fn hold_within(path: &Path, hold: Hold, wait: Duration) -> Result<Option<Held>, Failure> {
    hold_until(path, hold, Some(Instant::now() + wait))
}

fn hold_until(path: &Path, hold: Hold, deadline: Option<Instant>) -> Result<Option<Held>, Failure> {
    let file = arm::open_lock_file(path, hold)
        .map_err(|error| Failure::at(Stage::OpenLockFile, path, error))?;
    loop {
        if arm::try_lock(&file, hold).map_err(|error| Failure::at(Stage::Lock, path, error))? {
            return Ok(Some(Held { hold, file }));
        }
        let Some(deadline) = deadline else {
            return Ok(None);
        };
        let now = Instant::now();
        if now >= deadline {
            return Ok(None);
        }
        std::thread::sleep(LOCK_POLL.min(deadline - now));
    }
}

/// The Windows and macOS removal: a directory goes with everything in it,
/// anything else (a file, a link) alone — a link to a directory is removed,
/// never followed.
#[cfg(any(windows, target_os = "macos"))]
fn remove_entry(path: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// **The Windows arm.**
#[cfg(windows)]
mod arm {
    use super::{Failure, Hold, Replace, Stage, Surface};
    use std::fs::{File, OpenOptions};
    use std::io::{self, Write};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::{Path, PathBuf};
    use windows::Win32::Foundation::{ERROR_LOCK_VIOLATION, ERROR_SUCCESS, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FlushFileBuffers, LOCK_FILE_FLAGS, LOCKFILE_EXCLUSIVE_LOCK,
        LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        MoveFileExW, UnlockFileEx,
    };
    use windows::Win32::System::IO::OVERLAPPED;
    use windows::core::PCWSTR;

    /// The byte range every lock covers: the first byte. Locking past the end
    /// of a file is allowed, so the file stays empty; the range has to be the
    /// same in `LockFileEx` and `UnlockFileEx`.
    const LOCKED_BYTES: u32 = 1;

    fn wide(text: &std::ffi::OsStr) -> io::Result<Vec<u16>> {
        use std::os::windows::ffi::OsStrExt;
        let mut value: Vec<u16> = text.encode_wide().collect();
        if value.contains(&0) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "NUL in a path"));
        }
        value.push(0);
        Ok(value)
    }

    fn raw(file: &File) -> HANDLE {
        HANDLE(file.as_raw_handle())
    }

    /// The operating system's error as `io::Error` with its kind: a Win32 code
    /// wrapped in an `HRESULT` (`0x8007xxxx`) is unwrapped first, because
    /// `io::Error` reads a raw code as a Win32 code and the `windows` crate's
    /// own conversion hands it the whole `HRESULT`.
    fn os_error(error: &windows::core::Error) -> io::Error {
        let code = error.code().0.cast_unsigned();
        if code & 0xFFFF_0000 == 0x8007_0000 {
            io::Error::from_raw_os_error((code & 0xFFFF).cast_signed())
        } else {
            io::Error::from_raw_os_error(code.cast_signed())
        }
    }

    pub(super) struct Os;

    impl Surface for Os {
        type Handle = File;

        fn create_new(&mut self, path: &Path) -> io::Result<File> {
            OpenOptions::new().write(true).create_new(true).open(path)
        }

        fn write_all(&mut self, handle: &mut File, bytes: &[u8]) -> io::Result<()> {
            handle.write_all(bytes)
        }

        fn flush(&mut self, handle: &mut File) -> io::Result<()> {
            // SAFETY: the handle is live for the whole call and owned by
            // `handle`, which outlives it.
            unsafe { FlushFileBuffers(raw(handle)) }.map_err(|error| os_error(&error))
        }

        fn close(&mut self, handle: File) {
            drop(handle);
        }

        fn open_directory(&mut self, path: &Path) -> io::Result<File> {
            // `FlushFileBuffers` needs `GENERIC_WRITE` on the handle, and a
            // directory can only be opened at all with backup semantics.
            OpenOptions::new()
                .write(true)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0)
                .open(path)
        }

        fn rename(&mut self, from: &Path, to: &Path, replace: Replace) -> io::Result<()> {
            let from = wide(from.as_os_str())?;
            let to = wide(to.as_os_str())?;
            let flags = match replace {
                Replace::Existing => MOVEFILE_WRITE_THROUGH | MOVEFILE_REPLACE_EXISTING,
                Replace::Never => MOVEFILE_WRITE_THROUGH,
            };
            // SAFETY: both strings are NUL-terminated and live across the call.
            unsafe { MoveFileExW(PCWSTR(from.as_ptr()), PCWSTR(to.as_ptr()), flags) }
                .map_err(|error| os_error(&error))
        }

        fn remove(&mut self, path: &Path) {
            let _ = std::fs::remove_file(path);
        }

        fn remove_entry(&mut self, path: &Path) -> io::Result<()> {
            super::remove_entry(path)
        }
    }

    pub(super) fn open_lock_file(path: &Path, hold: Hold) -> io::Result<File> {
        match hold {
            Hold::Shared => OpenOptions::new().read(true).open(path),
            Hold::Exclusive => OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path),
        }
    }

    pub(super) fn try_lock(file: &File, hold: Hold) -> io::Result<bool> {
        let mode = match hold {
            Hold::Shared => LOCK_FILE_FLAGS(0),
            Hold::Exclusive => LOCKFILE_EXCLUSIVE_LOCK,
        };
        let mut overlapped = OVERLAPPED::default();
        // SAFETY: the handle is live for the call; the handle is synchronous,
        // so with `LOCKFILE_FAIL_IMMEDIATELY` the call returns before
        // `overlapped`, which only carries the offset 0, goes out of scope.
        let taken = unsafe {
            LockFileEx(
                raw(file),
                mode | LOCKFILE_FAIL_IMMEDIATELY,
                None,
                LOCKED_BYTES,
                0,
                &raw mut overlapped,
            )
        };
        match taken {
            Ok(()) => Ok(true),
            Err(error) if error.code() == ERROR_LOCK_VIOLATION.to_hresult() => Ok(false),
            Err(error) => Err(os_error(&error)),
        }
    }

    pub(super) fn unlock(file: &File) {
        let mut overlapped = OVERLAPPED::default();
        // SAFETY: as in `try_lock`; the range is the one it locked. Closing
        // the handle right after would release the lock anyway, but not at a
        // time the documentation promises.
        unsafe {
            let _ = UnlockFileEx(raw(file), None, LOCKED_BYTES, 0, &raw mut overlapped);
        }
    }

    pub(super) fn flush_current_user_key(subkey: &str) -> Result<(), Failure> {
        use windows::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, RegCloseKey, RegFlushKey, RegOpenKeyExW,
        };
        let place = PathBuf::from(format!("HKCU\\{subkey}"));
        let name = wide(std::ffi::OsStr::new(subkey))
            .map_err(|error| Failure::at(Stage::OpenKey, &place, error))?;
        let mut key = HKEY::default();
        // SAFETY: the name is NUL-terminated and lives across the call; `key`
        // is written only on success.
        let opened = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(name.as_ptr()),
                None,
                KEY_QUERY_VALUE,
                &raw mut key,
            )
        };
        if opened != ERROR_SUCCESS {
            return Err(Failure::at(
                Stage::OpenKey,
                &place,
                io::Error::from_raw_os_error(opened.0.cast_signed()),
            ));
        }
        // SAFETY: `key` was opened above and is closed right after.
        let flushed = unsafe { RegFlushKey(key) };
        // SAFETY: as above; nothing uses `key` after this.
        unsafe {
            let _ = RegCloseKey(key);
        }
        if flushed == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(Failure::at(
                Stage::FlushKey,
                &place,
                io::Error::from_raw_os_error(flushed.0.cast_signed()),
            ))
        }
    }
}

/// **The macOS arm.**
#[cfg(target_os = "macos")]
mod arm {
    use super::{Hold, Replace, Surface};
    use std::ffi::CString;
    use std::fs::{File, OpenOptions};
    use std::io::{self, Write};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::AsRawFd;
    use std::path::Path;

    fn c_path(path: &Path) -> io::Result<CString> {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in a path"))
    }

    pub(super) struct Os;

    impl Surface for Os {
        type Handle = File;

        fn create_new(&mut self, path: &Path) -> io::Result<File> {
            OpenOptions::new().write(true).create_new(true).open(path)
        }

        fn write_all(&mut self, handle: &mut File, bytes: &[u8]) -> io::Result<()> {
            handle.write_all(bytes)
        }

        fn flush(&mut self, handle: &mut File) -> io::Result<()> {
            // `fsync` on macOS leaves the data in the drive's cache;
            // `F_FULLFSYNC` asks the drive to write it to the medium.
            // SAFETY: the descriptor is live for the call and owned by `handle`.
            if unsafe { libc::fcntl(handle.as_raw_fd(), libc::F_FULLFSYNC) } == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        fn close(&mut self, handle: File) {
            drop(handle);
        }

        fn open_directory(&mut self, path: &Path) -> io::Result<File> {
            OpenOptions::new().read(true).open(path)
        }

        fn rename(&mut self, from: &Path, to: &Path, replace: Replace) -> io::Result<()> {
            match replace {
                Replace::Existing => std::fs::rename(from, to),
                Replace::Never => {
                    let from = c_path(from)?;
                    let to = c_path(to)?;
                    // SAFETY: both strings are NUL-terminated and live across
                    // the call.
                    if unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) }
                        == -1
                    {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                }
            }
        }

        fn remove(&mut self, path: &Path) {
            let _ = std::fs::remove_file(path);
        }

        fn remove_entry(&mut self, path: &Path) -> io::Result<()> {
            super::remove_entry(path)
        }
    }

    pub(super) fn open_lock_file(path: &Path, hold: Hold) -> io::Result<File> {
        match hold {
            Hold::Shared => OpenOptions::new().read(true).open(path),
            Hold::Exclusive => OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path),
        }
    }

    pub(super) fn try_lock(file: &File, hold: Hold) -> io::Result<bool> {
        let mode = match hold {
            Hold::Shared => libc::LOCK_SH,
            Hold::Exclusive => libc::LOCK_EX,
        };
        // SAFETY: the descriptor is live for the call.
        if unsafe { libc::flock(file.as_raw_fd(), mode | libc::LOCK_NB) } == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
            Ok(false)
        } else {
            Err(error)
        }
    }

    pub(super) fn unlock(file: &File) {
        // SAFETY: the descriptor is live for the call. Closing it right after
        // would release the lock as well; this says so at the drop.
        unsafe {
            libc::flock(file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

/// **The arm of every other platform: each effect is refused by name.**
#[cfg(not(any(windows, target_os = "macos")))]
mod arm {
    use super::{Hold, Replace, Surface};
    use std::convert::Infallible;
    use std::fs::File;
    use std::io;
    use std::path::Path;

    fn refused(operation: &str) -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            format!("install_txn has no {operation} on this platform"),
        )
    }

    pub(super) struct Os;

    impl Surface for Os {
        type Handle = Infallible;

        fn create_new(&mut self, _path: &Path) -> io::Result<Infallible> {
            Err(refused("durable write"))
        }

        fn write_all(&mut self, handle: &mut Infallible, _bytes: &[u8]) -> io::Result<()> {
            match *handle {}
        }

        fn flush(&mut self, handle: &mut Infallible) -> io::Result<()> {
            match *handle {}
        }

        fn close(&mut self, handle: Infallible) {
            match handle {}
        }

        fn open_directory(&mut self, _path: &Path) -> io::Result<Infallible> {
            Err(refused("directory flush"))
        }

        fn rename(&mut self, _from: &Path, _to: &Path, _replace: Replace) -> io::Result<()> {
            Err(refused("durable move"))
        }

        fn remove(&mut self, _path: &Path) {}

        fn remove_entry(&mut self, _path: &Path) -> io::Result<()> {
            Err(refused("durable remove"))
        }
    }

    pub(super) fn open_lock_file(_path: &Path, _hold: Hold) -> io::Result<File> {
        Err(refused("lock"))
    }

    pub(super) fn try_lock(_file: &File, _hold: Hold) -> io::Result<bool> {
        Err(refused("lock"))
    }

    pub(super) fn unlock(_file: &File) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One call the door made, as the recording fake saw it. A handle is named
    /// by the path it was opened on and whether it was opened as a directory.
    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Call {
        Create(PathBuf),
        Write(PathBuf, Vec<u8>),
        Flush { path: PathBuf, directory: bool },
        Close(PathBuf),
        OpenDirectory(PathBuf),
        Rename(PathBuf, PathBuf, Replace),
        Remove(PathBuf),
        RemoveEntry(PathBuf),
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Fail {
        Create,
        Write,
        FlushFile,
        Rename,
        OpenDirectory,
        FlushDirectory,
        /// The entry to remove is not there (`NotFound`).
        RemoveMissing,
        /// The entry cannot be removed (any other error).
        RemoveEntry,
    }

    struct FakeHandle {
        path: PathBuf,
        directory: bool,
    }

    /// **The recording fake of the OS surface**: it does nothing but write
    /// down each call, and fails the one call it is told to.
    #[derive(Default)]
    struct Recorder {
        calls: Vec<Call>,
        fail: Option<Fail>,
    }

    impl Recorder {
        fn failing(fail: Fail) -> Self {
            Self {
                calls: Vec::new(),
                fail: Some(fail),
            }
        }

        fn answer(&self, at: Fail) -> io::Result<()> {
            if self.fail == Some(at) {
                Err(io::Error::other(format!("{at:?} refused by the fake")))
            } else {
                Ok(())
            }
        }
    }

    impl Surface for Recorder {
        type Handle = FakeHandle;

        fn create_new(&mut self, path: &Path) -> io::Result<FakeHandle> {
            self.calls.push(Call::Create(path.to_path_buf()));
            self.answer(Fail::Create)?;
            Ok(FakeHandle {
                path: path.to_path_buf(),
                directory: false,
            })
        }

        fn write_all(&mut self, handle: &mut FakeHandle, bytes: &[u8]) -> io::Result<()> {
            self.calls
                .push(Call::Write(handle.path.clone(), bytes.to_vec()));
            self.answer(Fail::Write)
        }

        fn flush(&mut self, handle: &mut FakeHandle) -> io::Result<()> {
            self.calls.push(Call::Flush {
                path: handle.path.clone(),
                directory: handle.directory,
            });
            self.answer(if handle.directory {
                Fail::FlushDirectory
            } else {
                Fail::FlushFile
            })
        }

        fn close(&mut self, handle: FakeHandle) {
            self.calls.push(Call::Close(handle.path));
        }

        fn open_directory(&mut self, path: &Path) -> io::Result<FakeHandle> {
            self.calls.push(Call::OpenDirectory(path.to_path_buf()));
            self.answer(Fail::OpenDirectory)?;
            Ok(FakeHandle {
                path: path.to_path_buf(),
                directory: true,
            })
        }

        fn rename(&mut self, from: &Path, to: &Path, replace: Replace) -> io::Result<()> {
            self.calls
                .push(Call::Rename(from.to_path_buf(), to.to_path_buf(), replace));
            self.answer(Fail::Rename)
        }

        fn remove(&mut self, path: &Path) {
            self.calls.push(Call::Remove(path.to_path_buf()));
        }

        fn remove_entry(&mut self, path: &Path) -> io::Result<()> {
            self.calls.push(Call::RemoveEntry(path.to_path_buf()));
            if self.fail == Some(Fail::RemoveMissing) {
                return Err(io::Error::from(io::ErrorKind::NotFound));
            }
            self.answer(Fail::RemoveEntry)
        }
    }

    /// Journal-shaped bytes: a header v1 as `bt-app`'s `update_txn` encodes it.
    /// Spelled out rather than encoded, because `bt-platform` sits below
    /// `bt-app` and cannot name its types; the door never reads what it writes.
    const JOURNAL: &[u8] = br#"{"v":1,"txn":"00112233445566778899aabbccddeeff","rescue":"rescue\\folio.exe","class":"deferred","outcome":"none","body":{"phase":"Prepared"}}"#;

    fn home() -> PathBuf {
        PathBuf::from("home").join(".folio-update")
    }

    /// The temporary file of the one write in a recording.
    fn temporary_of(calls: &[Call]) -> PathBuf {
        match calls.first() {
            Some(Call::Create(path)) => path.clone(),
            other => panic!("a durable write starts by creating its temporary file, not {other:?}"),
        }
    }

    /// RED (U-11) — **a journal write is renamed only after its bytes are
    /// flushed, and the call returns only after the directory is flushed.**
    ///
    /// §(b).2 "Durability": write to a temp file, flush it, atomically rename,
    /// then flush the directory. A rename before the flush can be on disk
    /// while the bytes are not: after a power cut the journal's name holds a
    /// torn or empty file, and recovery reads a state nobody wrote. A return
    /// before the directory flush lets the caller take a destructive step
    /// whose naming journal state the next boot never sees.
    ///
    /// MUTATION: swap the flush and the rename in `durable_write_with` (rename
    /// the temporary file, then flush it).
    #[test]
    fn a_journal_write_is_renamed_only_after_its_flush() {
        let target = home().join("journal.json");
        let mut fake = Recorder::default();
        durable_write_with(&mut fake, &target, JOURNAL, Replace::Existing).unwrap();
        let temporary = temporary_of(&fake.calls);
        assert_eq!(temporary.parent(), Some(home().as_path()), "same directory");
        assert_eq!(
            fake.calls,
            vec![
                Call::Create(temporary.clone()),
                Call::Write(temporary.clone(), JOURNAL.to_vec()),
                Call::Flush {
                    path: temporary.clone(),
                    directory: false
                },
                Call::Close(temporary.clone()),
                Call::Rename(temporary, target, Replace::Existing),
                Call::OpenDirectory(home()),
                Call::Flush {
                    path: home(),
                    directory: true
                },
                Call::Close(home()),
            ],
        );
    }

    /// RED (U-11) — **the directory flush opens the target's directory and
    /// flushes that handle, after the rename.**
    ///
    /// On Windows a directory is flushed through `FlushFileBuffers` on a handle
    /// to the directory itself (opened with `FILE_FLAG_BACKUP_SEMANTICS`,
    /// E-15); flushing the renamed file again does not make the rename, which
    /// is a change to the directory, durable. The same holds for a move, whose
    /// rename changes two directories.
    ///
    /// MUTATION: in `flush_directory`, get the handle from
    /// `create_new(directory)` instead of `open_directory(directory)`.
    #[test]
    fn the_directory_flush_opens_the_directory_and_flushes_that_handle() {
        let target = home().join("journal.json");
        let mut fake = Recorder::default();
        durable_write_with(&mut fake, &target, b"x", Replace::Existing).unwrap();
        let rename = fake
            .calls
            .iter()
            .position(|call| matches!(call, Call::Rename(..)))
            .unwrap();
        assert_eq!(fake.calls[rename + 1], Call::OpenDirectory(home()));
        assert_eq!(
            fake.calls[rename + 2],
            Call::Flush {
                path: home(),
                directory: true
            }
        );

        let backup = home().join("txn").join("backup");
        let mut fake = Recorder::default();
        durable_move_with(
            &mut fake,
            &PathBuf::from("install").join("a.dll"),
            &backup.join("a.dll"),
        )
        .unwrap();
        let flushed: Vec<&Call> = fake
            .calls
            .iter()
            .filter(|call| matches!(call, Call::OpenDirectory(_) | Call::Flush { .. }))
            .collect();
        assert_eq!(
            flushed,
            vec![
                &Call::OpenDirectory(backup.clone()),
                &Call::Flush {
                    path: backup,
                    directory: true
                },
                &Call::OpenDirectory(PathBuf::from("install")),
                &Call::Flush {
                    path: PathBuf::from("install"),
                    directory: true
                },
            ],
            "a move flushes the destination's directory, then the source's",
        );
        assert_eq!(
            fake.calls.first(),
            Some(&Call::Rename(
                PathBuf::from("install").join("a.dll"),
                home().join("txn").join("backup").join("a.dll"),
                Replace::Never
            )),
            "the rename comes first, and never replaces",
        );
    }

    /// RED (U-11) — **every failure of a durable write names its stage, and a
    /// failure before the rename removes the temporary file.**
    ///
    /// A journal that did not become durable has to say where it stopped, and
    /// must not leave `.journal.json.*.tmp` files behind in the installation
    /// home for every retry.
    ///
    /// MUTATION: drop the `surface.remove(&temporary)` after a failed flush.
    #[test]
    fn every_failure_of_a_durable_write_names_its_stage_and_leaves_no_temp() {
        let target = home().join("journal.json");
        for (fail, stage, removes) in [
            (Fail::Create, Stage::CreateTemp, false),
            (Fail::Write, Stage::Write, true),
            (Fail::FlushFile, Stage::FlushFile, true),
            (Fail::Rename, Stage::Rename, true),
            (Fail::OpenDirectory, Stage::OpenDirectory, false),
            (Fail::FlushDirectory, Stage::FlushDirectory, false),
        ] {
            let mut fake = Recorder::failing(fail);
            let failure =
                durable_write_with(&mut fake, &target, JOURNAL, Replace::Existing).unwrap_err();
            assert_eq!(failure.stage, stage, "{fail:?}");
            assert!(
                failure
                    .to_string()
                    .starts_with(&format!("install_txn {} ", stage.name())),
                "{failure}"
            );
            let temporary = temporary_of(&fake.calls);
            assert_eq!(
                fake.calls.last() == Some(&Call::Remove(temporary.clone())),
                removes,
                "{fail:?}: {:?}",
                fake.calls
            );
            if matches!(fail, Fail::OpenDirectory | Fail::FlushDirectory) {
                assert!(
                    fake.calls
                        .iter()
                        .any(|call| matches!(call, Call::Rename(..))),
                    "a directory failure comes after the rename"
                );
            } else {
                assert!(
                    !fake
                        .calls
                        .iter()
                        .any(|call| matches!(call, Call::OpenDirectory(_))),
                    "{fail:?}: nothing is flushed after a failure before the rename"
                );
            }
        }
    }

    /// RED (U-12) — **a durable remove flushes the directory the entry was in,
    /// after the removal, and nothing there already is done, not a failure.**
    ///
    /// A retired transaction's folder is removed before its journal (U-10's
    /// "journal deleted last"): if the journal's removal reached the device
    /// and the folder's did not, the folder would be left behind with nothing
    /// that names it. Flushing the parent after each removal is what orders the
    /// two on the device. A start that finds the folder already gone (an
    /// earlier start removed it and stopped before the journal) must still be
    /// able to finish, so `NotFound` is success — and the directory is flushed
    /// anyway, because that earlier removal may not have been.
    ///
    /// MUTATION: in `durable_remove_with`, return `Ok(())` straight after the
    /// removal, without `flush_directory`.
    #[test]
    fn a_durable_remove_flushes_the_directory_after_the_removal() {
        let folder = home().join("00112233445566778899aabbccddeeff");
        for fail in [None, Some(Fail::RemoveMissing)] {
            let mut fake = Recorder {
                calls: Vec::new(),
                fail,
            };
            durable_remove_with(&mut fake, &folder).unwrap();
            assert_eq!(
                fake.calls,
                vec![
                    Call::RemoveEntry(folder.clone()),
                    Call::OpenDirectory(home()),
                    Call::Flush {
                        path: home(),
                        directory: true
                    },
                    Call::Close(home()),
                ],
                "{fail:?}"
            );
        }
        let mut fake = Recorder::failing(Fail::RemoveEntry);
        let failure = durable_remove_with(&mut fake, &folder).unwrap_err();
        assert_eq!(failure.stage, Stage::Remove);
        assert_eq!(
            fake.calls,
            vec![Call::RemoveEntry(folder)],
            "a removal that failed flushes nothing"
        );
    }

    #[cfg(any(windows, target_os = "macos"))]
    fn scratch(tag: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("bt-install-txn-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[cfg(any(windows, target_os = "macos"))]
    fn names_in(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// RED (U-11) — **a durable write replaces its target whole and leaves
    /// nothing else in the directory.**
    ///
    /// The real arm end to end: the journal is rewritten at every phase, so the
    /// rename must take an existing name (`MOVEFILE_REPLACE_EXISTING` /
    /// `rename`), and the real directory flush (E-15's first half: an NTFS
    /// directory opened with backup semantics accepts `FlushFileBuffers`;
    /// `F_FULLFSYNC` on an APFS directory) must succeed, or no journal write
    /// could ever return.
    ///
    /// MUTATION: open the directory in `Os::open_directory` without
    /// `FILE_FLAG_BACKUP_SEMANTICS` (Windows) — the open is refused.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_durable_write_replaces_its_target_whole_and_leaves_no_temp() {
        let home = scratch("write");
        let journal = home.join("journal.json");
        durable_write(&journal, b"old").unwrap();
        durable_write(&journal, JOURNAL).unwrap();
        assert_eq!(std::fs::read(&journal).unwrap(), JOURNAL);
        assert_eq!(names_in(&home), vec!["journal.json".to_string()]);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// RED (U-13) — **a durable create writes a new file whole, and refuses an
    /// existing one at the rename, leaving it byte for byte and no temporary
    /// beside it.**
    ///
    /// The trial's receipt is written create-new (F-14): once `health-<nonce>`
    /// exists, nothing replaces it, so the lock holder that reads it reads the
    /// first receipt the trial wrote and never a second one that raced it.
    ///
    /// MUTATION: pass `Replace::Existing` from `durable_create` (the receipt is
    /// written over).
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_durable_create_writes_a_new_file_and_never_an_existing_one() {
        let home = scratch("create");
        let receipt = home.join("health-00");
        durable_create(&receipt, b"first").unwrap();
        assert_eq!(std::fs::read(&receipt).unwrap(), b"first");
        let refused = durable_create(&receipt, b"second").unwrap_err();
        assert_eq!(refused.stage, Stage::Rename, "{refused}");
        assert_eq!(
            refused.error.kind(),
            io::ErrorKind::AlreadyExists,
            "{refused}"
        );
        assert_eq!(std::fs::read(&receipt).unwrap(), b"first");
        assert_eq!(names_in(&home), vec!["health-00".to_string()]);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// RED (U-12) — **a durable remove takes a whole tree, leaves its
    /// siblings, and a second remove of the same path is done, not a
    /// failure.**
    ///
    /// The real removal behind the fake's order: a transaction's folder holds
    /// nested folders (`rescue`, `set`, `backup`), so the removal is of a tree,
    /// and the journal and the two lock files beside it must survive it.
    ///
    /// MUTATION: make `remove_entry` call `std::fs::remove_dir` (the folder
    /// alone, which refuses a folder that is not empty).
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_durable_remove_takes_a_whole_tree_and_leaves_its_siblings() {
        let home = scratch("remove");
        let folder = home.join("00112233445566778899aabbccddeeff");
        std::fs::create_dir_all(folder.join("rescue")).unwrap();
        std::fs::write(folder.join("rescue").join("folio.exe"), b"old").unwrap();
        std::fs::write(folder.join("inventory"), b"x").unwrap();
        std::fs::write(home.join("journal.json"), JOURNAL).unwrap();
        std::fs::write(home.join("lock"), b"").unwrap();
        durable_remove(&folder).unwrap();
        assert_eq!(names_in(&home), vec!["journal.json", "lock"]);
        durable_remove(&folder).unwrap();
        durable_remove(&home.join("journal.json")).unwrap();
        assert_eq!(names_in(&home), vec!["lock"]);
        std::fs::remove_dir_all(&home).unwrap();
    }

    /// RED (U-11) — **a durable move never takes the name of an existing
    /// file: it is refused at the rename and neither file changes.**
    ///
    /// The contract (see [`durable_move`]): every file is in exactly one
    /// place across a flip (I1′), so a move onto an existing name would
    /// destroy a file an inventory counts. A move to a free name succeeds, the
    /// source is gone and the destination holds its bytes.
    ///
    /// MUTATION: pass `Replace::Existing` in `durable_move_with`.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_durable_move_never_replaces_an_existing_file() {
        let root = scratch("move");
        let install = root.join("install");
        let backup = root.join("backup");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(install.join("a.dll"), b"old member").unwrap();
        std::fs::write(backup.join("a.dll"), b"already here").unwrap();

        let refused = durable_move(&install.join("a.dll"), &backup.join("a.dll")).unwrap_err();
        assert_eq!(refused.stage, Stage::Rename);
        assert_eq!(
            refused.error.kind(),
            io::ErrorKind::AlreadyExists,
            "{refused}"
        );
        assert_eq!(std::fs::read(install.join("a.dll")).unwrap(), b"old member");
        assert_eq!(
            std::fs::read(backup.join("a.dll")).unwrap(),
            b"already here"
        );

        durable_move(&install.join("a.dll"), &backup.join("b.dll")).unwrap();
        assert!(!install.join("a.dll").exists());
        assert_eq!(std::fs::read(backup.join("b.dll")).unwrap(), b"old member");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The child half of `admission_shared_blocks_exclusive_across_processes`.
    ///
    /// Run by that test as a second process of this same test binary, with the
    /// three paths in its environment; run any other way (no environment), it
    /// has nothing to do and returns. It takes shared admission, says so by
    /// creating the ready file, and holds it until the go file appears (or a
    /// minute passes, so an abandoned child still ends by itself).
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn admission_child_holds_shared_until_told() {
        let (Some(admission), Some(ready), Some(go)) = (
            std::env::var_os(CHILD_ADMISSION),
            std::env::var_os(CHILD_READY),
            std::env::var_os(CHILD_GO),
        ) else {
            return;
        };
        let held = try_hold(Path::new(&admission), Hold::Shared)
            .unwrap()
            .expect("the child is the first holder");
        std::fs::write(&ready, b"held").unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while !Path::new(&go).exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(held);
    }

    #[cfg(any(windows, target_os = "macos"))]
    const CHILD_ADMISSION: &str = "BT_INSTALL_TXN_CHILD_ADMISSION";
    #[cfg(any(windows, target_os = "macos"))]
    const CHILD_READY: &str = "BT_INSTALL_TXN_CHILD_READY";
    #[cfg(any(windows, target_os = "macos"))]
    const CHILD_GO: &str = "BT_INSTALL_TXN_CHILD_GO";

    /// RED (U-11) — **a running copy's shared admission, held in another
    /// process, refuses the applier's exclusive admission at once; the
    /// exclusive admission is taken as soon as that process has exited.**
    ///
    /// F-6: every Folio started from an install holds `admission` shared for
    /// its lifetime, and the mover holds it exclusive only while files move.
    /// The lock is the operating system's, so it has to hold across processes
    /// and has to be gone when its holder is: a second process is the only
    /// honest test of either. The child is this test binary run again (see
    /// `admission_child_holds_shared_until_told`); its pid is recorded and it
    /// is waited on, never ended.
    ///
    /// MUTATION: take the exclusive lock as shared (`Hold::Exclusive =>
    /// LOCK_FILE_FLAGS(0)` / `libc::LOCK_SH` in `try_lock`).
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn admission_shared_blocks_exclusive_across_processes() {
        let home = scratch("admission-processes");
        let admission = home.join("admission");
        let ready = home.join("ready");
        let go = home.join("go");
        std::fs::write(&admission, b"").unwrap();

        let mut child = crate::quiet_command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "install_txn::tests::admission_child_holds_shared_until_told",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(CHILD_ADMISSION, &admission)
            .env(CHILD_READY, &ready)
            .env(CHILD_GO, &go)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let child_pid = child.id();
        let deadline = Instant::now() + Duration::from_secs(60);
        while !ready.exists() {
            assert!(
                Instant::now() < deadline,
                "child {child_pid} never took the lock"
            );
            std::thread::sleep(Duration::from_millis(20));
        }

        let refused = try_hold(&admission, Hold::Exclusive).unwrap();
        let shared_beside = try_hold(&admission, Hold::Shared).unwrap();

        std::fs::write(&go, b"go").unwrap();
        let status = child.wait().unwrap();
        assert!(status.success(), "child {child_pid}: {status}");

        assert!(
            refused.is_none(),
            "exclusive admission was granted while process {child_pid} held it shared"
        );
        assert!(
            shared_beside.is_some(),
            "a second running copy shares admission with the first"
        );
        drop(shared_beside);
        let applier = try_hold(&admission, Hold::Exclusive).unwrap();
        assert_eq!(
            applier.as_ref().map(Held::hold),
            Some(Hold::Exclusive),
            "the holder {child_pid} has exited, so the applier is admitted"
        );
        drop(applier);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// RED (U-11) — **while the applier holds admission exclusive, no new
    /// running copy is admitted; once it lets go, several are.**
    ///
    /// F-6's other direction: a Folio started while files move must not run
    /// from a half-replaced install (owner ruling 3, 2026-09-25: it waits). Two
    /// handles in one process conflict exactly as two processes do, for both
    /// `LockFileEx` and `flock`.
    ///
    /// MUTATION: take the exclusive lock as shared (as above).
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn admission_exclusive_blocks_a_new_shared_holder() {
        let home = scratch("admission-exclusive");
        let admission = home.join("admission");
        let applier = try_hold(&admission, Hold::Exclusive)
            .unwrap()
            .expect("the file is created and nobody holds it");
        assert!(try_hold(&admission, Hold::Shared).unwrap().is_none());
        assert!(try_hold(&admission, Hold::Exclusive).unwrap().is_none());
        drop(applier);
        let first = try_hold(&admission, Hold::Shared).unwrap();
        let second = try_hold(&admission, Hold::Shared).unwrap();
        assert!(
            first.is_some() && second.is_some(),
            "shared has many holders"
        );
        drop((first, second));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// RED (U-11) — **the bounded wait asks until its deadline and not past
    /// it, and is answered at once when the lock is free.**
    ///
    /// The applier waits for the running copies to quit for a bounded time
    /// (F-6: 60 s, then `Prepared`, deferred). A wait that gave up at the first
    /// refusal would defer every update a quitting copy was slow to leave;
    /// one that ignored its deadline would hang the job.
    ///
    /// MUTATION: return `Ok(None)` after the first refusal in `hold_until`
    /// whatever the deadline.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_durable_lock_wait_gives_up_at_its_deadline_and_not_before() {
        let home = scratch("admission-deadline");
        let lock = home.join("lock");
        let holder = try_hold(&lock, Hold::Exclusive).unwrap().unwrap();
        let wait = Duration::from_millis(300);
        let began = Instant::now();
        assert!(hold_within(&lock, Hold::Exclusive, wait).unwrap().is_none());
        let waited = began.elapsed();
        assert!(waited >= wait, "gave up after {waited:?}, before {wait:?}");
        assert!(waited < Duration::from_secs(5), "waited {waited:?}");
        drop(holder);
        let began = Instant::now();
        let taken = hold_within(&lock, Hold::Exclusive, Duration::from_secs(30)).unwrap();
        assert!(taken.is_some());
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "a free lock is taken at once"
        );
        drop(taken);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// RED (U-11) — **a running copy takes shared admission on a file it may
    /// only read, and one that does not exist yet is `NotFound` and is not
    /// created.**
    ///
    /// F-6 / E-15: another account that may run `folio.exe` from the install
    /// has read access to the admission file through the folder's inherited
    /// ACL, and nothing more; its shared hold must need nothing more. A
    /// read-only file stands in for that account here (no unprivileged test
    /// can make another account). A copy started before the installation home
    /// existed holds nothing (F-6), so the shared hold never creates the file.
    ///
    /// MUTATION: open the shared hold for writing in `open_lock_file`.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn admission_shared_needs_only_read_access_and_creates_nothing() {
        let home = scratch("admission-read-only");
        let admission = home.join("admission");

        let missing = try_hold(&admission, Hold::Shared).unwrap_err();
        assert_eq!(missing.stage, Stage::OpenLockFile);
        assert_eq!(missing.error.kind(), io::ErrorKind::NotFound, "{missing}");
        assert!(!admission.exists());

        std::fs::write(&admission, b"").unwrap();
        let mut permissions = std::fs::metadata(&admission).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&admission, permissions.clone()).unwrap();
        let held = try_hold(&admission, Hold::Shared);
        #[allow(
            clippy::permissions_set_readonly_false,
            reason = "the test clears the bit it set so its folder can be removed"
        )]
        permissions.set_readonly(false);
        std::fs::set_permissions(&admission, permissions).unwrap();
        let held = held.unwrap();
        assert_eq!(held.as_ref().map(Held::hold), Some(Hold::Shared));
        drop(held);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// RED (U-11) — **the registry flush opens a key under `HKEY_CURRENT_USER`
    /// and flushes it, and a key that is not there fails at the open, by
    /// name.**
    ///
    /// Writes nothing to the registry: `Software` always exists, and the
    /// missing key's name carries this process's id. Whether the flushed value
    /// survives a hard reset is experiment E-7, before U-22.
    #[cfg(windows)]
    #[test]
    fn a_durable_registry_flush_opens_its_key_and_names_a_missing_one() {
        flush_current_user_key("Software").unwrap();
        let missing = format!(
            "Software\\bt-install-txn-no-such-key-{}",
            std::process::id()
        );
        let failure = flush_current_user_key(&missing).unwrap_err();
        assert_eq!(failure.stage, Stage::OpenKey);
        assert_eq!(failure.error.kind(), io::ErrorKind::NotFound, "{failure}");
        assert_eq!(failure.path, PathBuf::from(format!("HKCU\\{missing}")));
    }

    /// RED (U-11) — **on a platform with no arm every effect is refused, by
    /// the door's name and the operation's, at its first stage.**
    ///
    /// A refusal that looked like success would let a caller believe a journal
    /// was durable; one that said only "unsupported" would not say which door.
    ///
    /// MUTATION: answer `Ok(())` from the portable `rename`.
    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn the_portable_arm_refuses_by_name() {
        let target = std::env::temp_dir()
            .join("bt-install-txn-portable")
            .join("journal.json");
        let write = durable_write(&target, JOURNAL).unwrap_err();
        assert_eq!(write.stage, Stage::CreateTemp);
        let moved = durable_move(&target, &target.with_extension("moved")).unwrap_err();
        assert_eq!(moved.stage, Stage::Rename);
        let held = try_hold(&target, Hold::Shared).unwrap_err();
        assert_eq!(held.stage, Stage::OpenLockFile);
        let removed = durable_remove(&target).unwrap_err();
        assert_eq!(removed.stage, Stage::Remove);
        for (failure, operation) in [
            (write, "durable write"),
            (moved, "durable move"),
            (held, "lock"),
            (removed, "durable remove"),
        ] {
            assert_eq!(failure.error.kind(), io::ErrorKind::Unsupported);
            assert!(
                failure
                    .to_string()
                    .contains(&format!("install_txn has no {operation} on this platform")),
                "{failure}"
            );
        }
        assert!(!target.exists());
    }
}
