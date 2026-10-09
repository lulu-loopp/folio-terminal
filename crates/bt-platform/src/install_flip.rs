//! **The installed program's replacement: the macOS exchange of a bundle, the
//! processes that run from an executable, and whether another process holds a
//! file open** (0.4.6 tickets U-28 and U-23;
//! `docs/plans/design/self-update-2026-09-16.md` §C.4, revision (b) §(b).2's
//! M4–M8 and W3–W8, experiments E-7 and E-12).
//!
//! * **[`exchange`]**: one `renamex_np(live, staged, RENAME_SWAP)` — the
//!   installed bundle and the staged one trade places in a single call, so
//!   there is no instant at which the launch path holds no bundle (§C.4, E5)
//!   — then a flush (`F_FULLFSYNC`) of each directory, through
//!   [`crate::install_txn`]'s durable steps. Both names must exist and sit on
//!   one volume, which the installation home beside the bundle gives by
//!   construction (C.1). Windows has no such call and refuses it; so does
//!   every platform without an arm. (The Windows flip is one
//!   `install_txn::durable_move` per file.)
//! * **[`running_from`]**: the processes whose image is a given executable,
//!   matched by the file itself, never by spelling — read only. macOS:
//!   `proc_listallpids`, then `proc_pidpath` of each, compared by device and
//!   inode. Windows: `K32EnumProcesses`, then `QueryFullProcessImageNameW` of
//!   each, compared by volume serial number and file index. The macOS
//!   applier's process check before the exchange (M4), the trial's pid after
//!   its launch through LaunchServices (`open` reports no pid), and the
//!   Windows recovery's look for an applier still alive (W3) are this list.
//! * **[`arguments_of`]** (0.4.7 ticket U-40, macOS): the command line a
//!   process recorded by its pid *and* its start time was started with — read
//!   only, through `sysctl(KERN_PROCARGS2)`, and answered only while that
//!   very process runs (its start time is asked before and after the read, so
//!   a pid reused in between is never answered for). `open` reports no pid, so
//!   a trial LaunchServices started is told from any other process of the same
//!   executable by the words it carries. `Unsupported` elsewhere: the Windows
//!   trial's pid is its launch's own.
//! * **[`still_running`]**: whether a process recorded by its pid *and* its
//!   start time still runs — a pid alone may have been reused (F-7). The start
//!   time is `proc_pidinfo`'s on macOS (microseconds since the epoch) and
//!   `GetProcessTimes`'s creation time on Windows (100 ns since 1601); a
//!   Windows process that has exited while a handle keeps its record is not
//!   running. **A Windows process runs until its process object is
//!   signalled** — after its handles are closed and its image unmapped — and
//!   not merely until its exit code can be read, which comes first (0.4.7
//!   uninstall fix: the door that waits for its asker took the said exit code
//!   for the end and met the asker's data-directory claim still held).
//!   Linux reads field 22 of `/proc/PID/stat` in clock ticks since boot.
//!   Process listing and signaling remain unavailable on Linux.
//! * **[`ask`]** (U-29; Windows U-24): a recorded process asked to quit, or
//!   ended — only after the process list shows that very process (pid *and*
//!   start instant) running from one of the given executables. The rollback
//!   stops the trial this way (§(b).2 W9/M9): its stopper is not its parent
//!   (the trial was started by LaunchServices, or by an applier that has since
//!   died) and has only the journal's record to go by. macOS: `SIGTERM` to
//!   ask, `SIGKILL` to end. Windows: the process is opened and its creation
//!   time read again **from that handle**, so a pid reused between the look
//!   and the act is never touched; to ask, `WM_CLOSE` is posted to each of its
//!   visible, unowned top-level windows that are not tool windows — the close
//!   a person makes, and the one `scripts/release/smoke.ps1` ends a run with
//!   (Folio listens to no other quit road; a process with no such window is
//!   asked by nothing, and its grace runs out); to end, `TerminateProcess` on
//!   the same handle. Refused by name elsewhere.
//! * **[`held_open`]** (Windows): whether another process holds a file open,
//!   asked by opening it for reading and writing with no sharing at all —
//!   E-7's process check before the first move. Any other handle on the file
//!   refuses that open, and so does a running image or a loaded library (the
//!   section the loader maps refuses a writer), with a sharing violation. A
//!   rename alone would not find a running image: Windows lets one be moved.
//!   Nothing is written, and the handle is closed at once, so the check never
//!   stands in a move's way. Refused by name elsewhere.
//!
//! Worker only: the exchange flushes to the device. Linux supplies only the
//! process-start read; unsupported operations return a named refusal or their
//! documented empty answer.

use std::ffi::OsString;
use std::io;
use std::path::Path;

use crate::install_txn::{self, Failure};

/// **A process, by its pid and the instant it started** (macOS: microseconds
/// since the epoch as `proc_pidinfo` reports it; Windows: `GetProcessTimes`
/// creation time in 100 ns since 1601; Linux: `/proc/PID/stat` field 22, clock
/// ticks since boot): together they name one process for its whole life, where
/// a pid alone may be reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Running {
    pub pid: u32,
    pub started: u64,
}

/// **Exchange the installed bundle at `live` with the one at `staged`**, in one
/// call, then flush both directories — see the module header.
///
/// # Errors
/// A [`Failure`] at [`install_txn::Stage::Rename`] when the exchange is
/// refused (neither name changed; `Unsupported` off macOS), or at a
/// directory's open or flush after it (the exchange happened and is not known
/// to be durable).
pub fn exchange(live: &Path, staged: &Path) -> Result<(), Failure> {
    install_txn::durable_exchange_with(&mut install_txn::Os, live, staged)
}

/// **Every process whose image is the file at `executable`** — see the module
/// header. A process that ends while it is being asked about is left out.
///
/// # Errors
/// The process list could not be read, or `executable` cannot be looked at;
/// `Unsupported` where there is no arm (neither macOS nor Windows).
pub fn running_from(executable: &Path) -> io::Result<Vec<Running>> {
    imp::running_from(executable)
}

/// **Whether `process` still runs**: its pid names a live process that started
/// at the same instant. `false` where there is no arm.
#[must_use]
pub fn still_running(process: Running) -> bool {
    imp::started_of(process.pid) == Some(process.started)
}

/// **The start instant of the live process `pid`**, or `None` when there is
/// none (or no arm).
#[must_use]
pub fn started_of(pid: u32) -> Option<u64> {
    imp::started_of(pid)
}

/// **The arguments `process` was started with**, its program's own name
/// first — see the module header.
///
/// # Errors
/// `NotFound` when `process` (its pid with its start instant) does not run,
/// before or after the read; the read's own error; `Unsupported` off macOS.
pub fn arguments_of(process: Running) -> io::Result<Vec<OsString>> {
    let gone = || {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} does not run as recorded", process.pid),
        )
    };
    if !still_running(process) {
        return Err(gone());
    }
    let arguments = imp::arguments_of(process.pid)?;
    if !still_running(process) {
        return Err(gone());
    }
    Ok(arguments)
}

/// **The arguments in a `KERN_PROCARGS2` answer**: a native `int` count, the
/// executable's path and the NULs that pad it, then that many NUL-ended
/// arguments (the environment follows, and is not read). `None` for bytes
/// that do not hold as many arguments as they count. An empty first argument
/// cannot be told from the padding — the format's own limit, as `ps` meets
/// it; a program started through LaunchServices or a shell names itself.
#[cfg(any(target_os = "macos", test))]
fn procargs2_arguments(bytes: &[u8]) -> Option<Vec<Vec<u8>>> {
    let (count, rest) = bytes.split_first_chunk::<4>()?;
    let count = usize::try_from(i32::from_ne_bytes(*count)).ok()?;
    let path_end = rest.iter().position(|byte| *byte == 0)?;
    let mut rest = &rest[path_end..];
    let start = rest.iter().position(|byte| *byte != 0)?;
    rest = &rest[start..];
    // The count is the answer's own claim: room is reserved for no more
    // arguments than the bytes after it could hold (each ends in a NUL).
    let mut arguments = Vec::with_capacity(count.min(rest.len()));
    for _ in 0..count {
        let end = rest.iter().position(|byte| *byte == 0)?;
        arguments.push(rest[..end].to_vec());
        rest = &rest[end + 1..];
    }
    Some(arguments)
}

/// **The image's file name for this exact live process**, or `None` after it
/// has ended, its pid has been reused, or its image cannot be inspected.
/// This is display evidence only; waits continue to use pid plus start time.
#[must_use]
pub fn image_name(process: Running) -> Option<OsString> {
    still_running(process)
        .then(|| imp::image_name(process.pid))
        .flatten()
}

/// **This process's parent, by its pid and start instant, when it started
/// before this process did** (0.4.7 ticket U-37): the process that started
/// this one, still running — `None` when it has gone, or when its pid now
/// names a process that started after this one (a reused pid: a parent always
/// starts first), or where there is no arm. macOS: `getppid`, which names
/// `launchd` once the parent has gone (it reparents the orphan). Windows: the
/// process snapshot's parent pid (`CreateToolhelp32Snapshot`), which Windows
/// never updates when the parent exits. Read only.
#[must_use]
pub fn parent_of_this_process() -> Option<Running> {
    let me = std::process::id();
    let mine = imp::started_of(me)?;
    let parent = imp::parent_pid(me)?;
    let started = imp::started_of(parent)?;
    (started < mine).then_some(Running {
        pid: parent,
        started,
    })
}

/// **What [`ask`] asks of a process.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// The ordinary request to quit, which a process may answer in its own
    /// time (or ignore): `SIGTERM` on macOS, `WM_CLOSE` to its windows on
    /// Windows.
    Quit,
    /// The end, which no process can refuse: `SIGKILL` on macOS,
    /// `TerminateProcess` on Windows.
    End,
}

/// **Whether `process` is running from one of `images`**: the process list of
/// each executable ([`running_from`]) names its pid with the same start
/// instant. An executable that cannot be looked at (a bundle that is not
/// there) names nothing.
///
/// # Errors
/// The process list could not be read; `Unsupported` where there is no arm.
pub fn runs_from(process: Running, images: &[&Path]) -> io::Result<bool> {
    for image in images {
        match running_from(image) {
            Ok(list) if list.contains(&process) => return Ok(true),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

/// **Ask `process` to quit, or end it, if it is still that process running
/// from one of `images`** — see the module header. Answers whether it was that
/// process and was asked: `false` when the process list does not show it (it
/// has ended, its pid was reused, or it runs some other program), and nothing
/// is sent then.
///
/// # Errors
/// The process list could not be read, or the process could not be asked for
/// a reason other than its having just ended; `Unsupported` where there is no
/// arm (neither macOS nor Windows).
pub fn ask(process: Running, images: &[&Path], ask: Ask) -> io::Result<bool> {
    if !runs_from(process, images)? {
        return Ok(false);
    }
    imp::signal(process, ask)
}

/// **Whether another process holds the file at `path` open** — see the module
/// header. `Ok(false)`: it was opened with no sharing, and closed again.
///
/// # Errors
/// The file could not be opened for another reason (it is not there, access
/// is denied); `Unsupported` off Windows.
pub fn held_open(path: &Path) -> io::Result<bool> {
    imp::held_open(path)
}

/// **Hold the file at `path` open with no sharing, as another program
/// would**, until the answer is dropped — a test's stand-in for the clean
/// machine's W10 hold (`[IO.File]::Open(path, 'Open', 'Read', 'None')`;
/// 0.4.7 ticket U-42b). Tests only: this crate's, and those of a crate that
/// names the `trust-harness` feature on its dev-dependency (`bt-app`'s).
///
/// # Errors
/// The file could not be opened; `Unsupported` off Windows, which has no
/// sharing modes.
#[cfg(any(test, feature = "trust-harness"))]
#[doc(hidden)]
pub fn hold_unshared(path: &Path) -> io::Result<std::fs::File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "sharing modes are a Windows file system's",
        ))
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{Ask, Running};
    use std::ffi::OsStr;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    /// Room for the processes that start between the count and the list.
    const SLACK: usize = 64;

    pub(super) fn running_from(executable: &Path) -> io::Result<Vec<Running>> {
        let wanted = std::fs::symlink_metadata(executable)?;
        let name = executable.file_name();
        let mut found = Vec::new();
        for pid in all_pids()? {
            let Some(path) = image_of(pid) else {
                continue;
            };
            let path = Path::new(OsStr::from_bytes(&path));
            // The name first, so only the few candidates are looked at.
            if path.file_name() != name {
                continue;
            }
            let Ok(seen) = std::fs::symlink_metadata(path) else {
                continue;
            };
            if (seen.dev(), seen.ino()) != (wanted.dev(), wanted.ino()) {
                continue;
            }
            let Ok(pid) = u32::try_from(pid) else {
                continue;
            };
            if let Some(started) = started_of(pid) {
                found.push(Running { pid, started });
            }
        }
        Ok(found)
    }

    fn all_pids() -> io::Result<Vec<libc::c_int>> {
        // SAFETY: a null buffer asks only for the count.
        let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        let count = usize::try_from(count).map_err(|_| io::Error::last_os_error())?;
        let mut pids: Vec<libc::c_int> = vec![0; count + SLACK];
        let bytes = libc::c_int::try_from(pids.len() * size_of::<libc::c_int>())
            .map_err(|_| io::Error::other("the process list does not fit an int"))?;
        // SAFETY: the buffer is `bytes` long and lives across the call.
        let listed = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        let listed = usize::try_from(listed).map_err(|_| io::Error::last_os_error())?;
        pids.truncate(listed.min(pids.len()));
        pids.retain(|pid| *pid > 0);
        Ok(pids)
    }

    /// The path of `pid`'s image as the kernel names it now (a moved file is
    /// named where it is), or `None` for a process that is gone or not ours
    /// to ask about.
    fn image_of(pid: libc::c_int) -> Option<Vec<u8>> {
        let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        let size = u32::try_from(buffer.len()).ok()?;
        // SAFETY: the buffer is `size` bytes and lives across the call.
        let written = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), size) };
        let written = usize::try_from(written).ok().filter(|n| *n > 0)?;
        buffer.truncate(written);
        Some(buffer)
    }

    pub(super) fn started_of(pid: u32) -> Option<u64> {
        let pid = libc::c_int::try_from(pid).ok()?;
        // SAFETY: an all-zero `proc_bsdinfo` is a valid value of a plain C
        // struct.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = libc::c_int::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
        // SAFETY: `info` is `size` bytes and lives across the call.
        let written = unsafe {
            libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size)
        };
        (written == size).then(|| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
    }

    pub(super) fn arguments_of(pid: u32) -> io::Result<Vec<std::ffi::OsString>> {
        use std::os::unix::ffi::OsStringExt;
        let pid = libc::c_int::try_from(pid)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a pid past c_int"))?;
        let mut most: libc::c_int = 0;
        let mut size = size_of::<libc::c_int>();
        let mut name = [libc::CTL_KERN, libc::KERN_ARGMAX];
        // SAFETY: `name` holds two integers; `most` is `size` bytes and lives
        // across the call; nothing is written to the kernel.
        let asked = unsafe {
            libc::sysctl(
                name.as_mut_ptr(),
                2,
                (&raw mut most).cast(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if asked != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0u8; usize::try_from(most).map_err(|_| io::Error::other("ARG_MAX"))?];
        let mut size = buffer.len();
        let mut name = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        // SAFETY: `name` holds three integers; the buffer is `size` bytes and
        // lives across the call, which writes at most that many and says how
        // many; nothing is written to the kernel.
        let asked = unsafe {
            libc::sysctl(
                name.as_mut_ptr(),
                3,
                buffer.as_mut_ptr().cast(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if asked != 0 {
            return Err(io::Error::last_os_error());
        }
        buffer.truncate(size);
        super::procargs2_arguments(&buffer)
            .map(|arguments| {
                arguments
                    .into_iter()
                    .map(std::ffi::OsString::from_vec)
                    .collect()
            })
            .ok_or_else(|| io::Error::other("the process's arguments could not be read whole"))
    }

    pub(super) fn image_name(pid: u32) -> Option<std::ffi::OsString> {
        let pid = libc::c_int::try_from(pid).ok()?;
        let path = image_of(pid)?;
        Path::new(OsStr::from_bytes(&path))
            .file_name()
            .map(OsStr::to_os_string)
    }

    pub(super) fn parent_pid(me: u32) -> Option<u32> {
        debug_assert_eq!(me, std::process::id());
        // SAFETY: `getppid` takes nothing and cannot fail.
        u32::try_from(unsafe { libc::getppid() }).ok()
    }

    pub(super) fn signal(process: Running, ask: Ask) -> io::Result<bool> {
        let pid = libc::pid_t::try_from(process.pid)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a pid past pid_t"))?;
        let signal = match ask {
            Ask::Quit => libc::SIGTERM,
            Ask::End => libc::SIGKILL,
        };
        // SAFETY: `kill` takes two integers and touches no memory of ours.
        if unsafe { libc::kill(pid, signal) } == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            // It ended between the look and the signal.
            Ok(false)
        } else {
            Err(error)
        }
    }

    pub(super) fn held_open(_path: &Path) -> io::Result<bool> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "install_flip asks whether a file is held open on Windows only",
        ))
    }
}

#[cfg(windows)]
mod imp {
    use super::Running;
    use std::ffi::OsStr;
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::path::Path;
    use windows::Win32::Foundation::{
        ERROR_INVALID_PARAMETER, ERROR_SHARING_VIOLATION, FILETIME, HANDLE, HWND, LPARAM,
        STILL_ACTIVE, WAIT_TIMEOUT, WPARAM,
    };
    use windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    use windows::Win32::System::ProcessStatus::K32EnumProcesses;
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongW, GetWindowThreadProcessId,
        IsWindowVisible, PostMessageW, WM_CLOSE, WS_EX_TOOLWINDOW,
    };
    use windows::core::BOOL;

    /// Room for the processes that start between two asks.
    const SLACK: usize = 256;

    pub(super) fn running_from(executable: &Path) -> io::Result<Vec<Running>> {
        let wanted = identity(executable)?;
        let name = lowered(executable.file_name());
        let mut found = Vec::new();
        for pid in all_pids()? {
            let Some(path) = crate::process_image_path(pid) else {
                continue;
            };
            // The name first, so only the few candidates are opened.
            if lowered(path.file_name()) != name {
                continue;
            }
            if identity(&path).ok() != Some(wanted) {
                continue;
            }
            if let Some(started) = started_of(pid) {
                found.push(Running { pid, started });
            }
        }
        Ok(found)
    }

    fn lowered(name: Option<&OsStr>) -> Option<String> {
        name.map(|name| name.to_string_lossy().to_lowercase())
    }

    /// The file's own identity: its volume's serial number and its index.
    fn identity(path: &Path) -> io::Result<(u32, u32, u32)> {
        let file = File::open(path)?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: `file` owns a live handle for the call; `info` is writable.
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &raw mut info) }
            .map_err(|_| io::Error::last_os_error())?;
        Ok((
            info.dwVolumeSerialNumber,
            info.nFileIndexHigh,
            info.nFileIndexLow,
        ))
    }

    fn all_pids() -> io::Result<Vec<u32>> {
        let mut pids: Vec<u32> = vec![0; 1024];
        loop {
            let bytes = u32::try_from(pids.len() * size_of::<u32>())
                .map_err(|_| io::Error::other("the process list does not fit a u32"))?;
            let mut returned = 0u32;
            // SAFETY: the buffer is `bytes` long and lives across the call,
            // which writes at most that many bytes and says how many.
            unsafe { K32EnumProcesses(pids.as_mut_ptr(), bytes, &raw mut returned) }
                .ok()
                .map_err(|_| io::Error::last_os_error())?;
            let listed = returned as usize / size_of::<u32>();
            if listed < pids.len() {
                pids.truncate(listed);
                pids.retain(|pid| *pid != 0);
                return Ok(pids);
            }
            pids.resize(pids.len() * 2 + SLACK, 0);
        }
    }

    pub(super) fn started_of(pid: u32) -> Option<u64> {
        // SAFETY: a call taking two flags and an integer; it answers an error
        // for a pid that has gone or that this token may not open.
        let process = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
        }
        .ok()?;
        // SAFETY: the handle was opened here, is owned by nothing else, and is
        // closed when `owned` is dropped.
        let owned = unsafe { OwnedHandle::from_raw_handle(process.0) };
        creation_of(HANDLE(owned.as_raw_handle()))
    }

    pub(super) fn arguments_of(_pid: u32) -> io::Result<Vec<std::ffi::OsString>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "install_flip reads a process's arguments on macOS only",
        ))
    }

    pub(super) fn image_name(pid: u32) -> Option<std::ffi::OsString> {
        crate::process_image_path(pid)?
            .file_name()
            .map(OsStr::to_os_string)
    }

    /// The parent pid the process snapshot records for `me`.
    pub(super) fn parent_pid(me: u32) -> Option<u32> {
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        };
        // SAFETY: a call taking a flag and a pid; it answers an error when no
        // snapshot can be taken.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.ok()?;
        // SAFETY: the snapshot was opened here, is owned by nothing else, and
        // is closed when `owned` is dropped.
        let owned = unsafe { OwnedHandle::from_raw_handle(snapshot.0) };
        let snapshot = HANDLE(owned.as_raw_handle());
        let mut entry = PROCESSENTRY32W {
            dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).ok()?,
            ..Default::default()
        };
        // SAFETY: `snapshot` is live for the call; `entry` is writable and its
        // `dwSize` says how large it is.
        let mut found = unsafe { Process32FirstW(snapshot, &raw mut entry) }.is_ok();
        while found {
            if entry.th32ProcessID == me {
                return Some(entry.th32ParentProcessID);
            }
            // SAFETY: as above.
            found = unsafe { Process32NextW(snapshot, &raw mut entry) }.is_ok();
        }
        None
    }

    /// The creation time of the process behind `process` while it runs, or
    /// `None` once it has ended (its record outlives it while any handle is
    /// open). `process` was opened with `PROCESS_SYNCHRONIZE`.
    ///
    /// **Ended is the process object signalled**, which the kernel does after
    /// it has closed every handle the process held and unmapped its image —
    /// not the exit code being readable, which comes first: a process leaving
    /// by `ExitProcess` or by `TerminateProcess` on itself (Folio's way out)
    /// has said its code while its handles — a data directory's claim among
    /// them — and its image are still its own, for as long as its threads take
    /// to leave the kernel. Whoever waits for a process to end waits for what
    /// it held, so that stretch is still running.
    fn creation_of(process: HANDLE) -> Option<u64> {
        // SAFETY: `process` is live for the call; a zero timeout only asks.
        if unsafe { WaitForSingleObject(process, 0) } != WAIT_TIMEOUT {
            return None;
        }
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: `process` is live for the call; the four are writable.
        unsafe {
            GetProcessTimes(
                process,
                &raw mut created,
                &raw mut exited,
                &raw mut kernel,
                &raw mut user,
            )
        }
        .ok()?;
        Some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }

    /// Whether the process behind `process` has said its exit code: it has
    /// ended, or it is leaving and its end can no longer be refused.
    fn leaving(process: HANDLE) -> bool {
        let mut code = 0u32;
        // SAFETY: `process` is live for the call; `code` is writable.
        unsafe { GetExitCodeProcess(process, &raw mut code) }
            .is_ok_and(|()| code != STILL_ACTIVE.0.cast_unsigned())
    }

    /// **Ask the process, or end it, through a handle that is that process**:
    /// its creation time is read again from the handle this opens, so a pid
    /// reused since the list was read is never touched. `false`: it has ended
    /// (or its pid is another process's now), and nothing was done.
    pub(super) fn signal(process: Running, ask: super::Ask) -> io::Result<bool> {
        // SAFETY: a call taking two flags and an integer; it answers an error
        // for a pid that has gone or that this token may not open.
        let opened = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
                false,
                process.pid,
            )
        };
        let handle = match opened {
            Ok(handle) => handle,
            // No process has this pid any more.
            Err(error) if error.code() == ERROR_INVALID_PARAMETER.to_hresult() => {
                return Ok(false);
            }
            Err(error) => return Err(io::Error::other(error)),
        };
        // SAFETY: the handle was opened here, is owned by nothing else, and is
        // closed when `owned` is dropped.
        let owned = unsafe { OwnedHandle::from_raw_handle(handle.0) };
        let handle = HANDLE(owned.as_raw_handle());
        if creation_of(handle) != Some(process.started) {
            return Ok(false);
        }
        match ask {
            super::Ask::Quit => {
                for window in closable_windows_of(process.pid) {
                    // SAFETY: a post takes the window and two integers; a
                    // window that has gone since it was listed refuses it.
                    let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
                }
                Ok(true)
            }
            super::Ask::End => {
                // SAFETY: `handle` is live for the call and was opened with
                // `PROCESS_TERMINATE`.
                match unsafe { TerminateProcess(handle, ENDED_BY_A_ROLLBACK) } {
                    Ok(()) => Ok(true),
                    // It ended, or began leaving, on its own between the look
                    // and the end: a process already leaving refuses to be
                    // ended, and has said its exit code.
                    Err(_) if leaving(handle) => Ok(false),
                    Err(error) => Err(io::Error::other(error)),
                }
            }
        }
    }

    /// The exit code of a process [`signal`] ends.
    const ENDED_BY_A_ROLLBACK: u32 = 1;

    /// **The windows of the process `pid` a person could close**: its
    /// top-level windows that are visible, have no owner and are not tool
    /// windows — the ones a person's `×` or `Alt+F4` closes.
    fn closable_windows_of(pid: u32) -> Vec<HWND> {
        /// What the enumeration looks for, and what it found.
        struct Look {
            pid: u32,
            found: Vec<HWND>,
        }

        /// Keep each window of `look.pid` a person could close. `state` is the
        /// `&mut Look` below, which outlives the enumeration because
        /// `EnumWindows` is synchronous.
        unsafe extern "system" fn keep_closable(window: HWND, state: LPARAM) -> BOOL {
            // SAFETY: `state` is the `&mut Look` of `closable_windows_of`,
            // live for the whole enumeration and touched by nothing else.
            let look = unsafe { &mut *(state.0 as *mut Look) };
            let mut owner_pid = 0u32;
            // SAFETY: `window` is the one being enumerated; `owner_pid` is
            // writable.
            unsafe { GetWindowThreadProcessId(window, Some(&raw mut owner_pid)) };
            if owner_pid != look.pid {
                return true.into();
            }
            // SAFETY: three reads of the enumerated window's own state.
            let visible = unsafe { IsWindowVisible(window) }.as_bool();
            let owned =
                unsafe { GetWindow(window, GW_OWNER) }.is_ok_and(|owner| !owner.is_invalid());
            let style = unsafe { GetWindowLongW(window, GWL_EXSTYLE) };
            let tool = style & WS_EX_TOOLWINDOW.0.cast_signed() != 0;
            if visible && !owned && !tool {
                look.found.push(window);
            }
            true.into()
        }

        let mut look = Look {
            pid,
            found: Vec::new(),
        };
        // SAFETY: the callback reads `look` only during this synchronous call.
        let _ = unsafe { EnumWindows(Some(keep_closable), LPARAM((&raw mut look) as isize)) };
        look.found
    }

    pub(super) fn held_open(path: &Path) -> io::Result<bool> {
        match OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(path)
        {
            Ok(file) => {
                drop(file);
                Ok(false)
            }
            Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION.0.cast_signed()) => {
                Ok(true)
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod imp {
    use super::{Ask, Running};
    use std::io;
    use std::path::Path;

    pub(super) fn running_from(_executable: &Path) -> io::Result<Vec<Running>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "install_flip reads the process list on macOS and Windows only",
        ))
    }

    #[cfg(not(target_os = "linux"))]
    pub(super) fn started_of(_pid: u32) -> Option<u64> {
        None
    }

    #[cfg(target_os = "linux")]
    pub(super) fn started_of(pid: u32) -> Option<u64> {
        let stat = crate::file_reads::read(
            crate::file_reads::Lane::Install,
            format!("/proc/{pid}/stat"),
        )
        .ok()?;
        start_ticks(&stat)
    }

    #[cfg(target_os = "linux")]
    fn start_ticks(stat: &[u8]) -> Option<u64> {
        let close = stat.iter().rposition(|byte| *byte == b')')?;
        if stat.get(close + 1) != Some(&b' ') {
            return None;
        }
        let mut fields = stat
            .get(close + 2..)?
            .split(|byte| byte.is_ascii_whitespace())
            .filter(|field| !field.is_empty());
        let state = fields.next()?;
        if state.len() != 1 || state == b"Z" || state == b"X" || state == b"x" {
            return None;
        }
        let start_ticks = fields.nth(18)?;
        std::str::from_utf8(start_ticks).ok()?.parse().ok()
    }

    #[cfg(all(test, target_os = "linux"))]
    mod linux_tests {
        use super::start_ticks;

        #[test]
        fn start_ticks_reads_field_22_after_the_last_comm_parenthesis() {
            let mut stat = b"42 (cmd ) S 1 2 (nested) \xff) R ".to_vec();
            stat.extend_from_slice(b"1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 731 20\n");
            assert_eq!(start_ticks(&stat), Some(731));
        }

        #[test]
        fn start_ticks_rejects_dead_and_malformed_records() {
            let fields = b"1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 731 20";
            for state in [b'Z', b'X', b'x'] {
                let mut stat = b"42 (worker) ".to_vec();
                stat.push(state);
                stat.push(b' ');
                stat.extend_from_slice(fields);
                assert_eq!(start_ticks(&stat), None);
            }
            assert_eq!(start_ticks(b"malformed"), None);
            assert_eq!(start_ticks(b"42 (short) S 1 2"), None);
            assert_eq!(
                start_ticks(b"42 (bad) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 nope"),
                None
            );
        }
    }

    pub(super) fn image_name(_pid: u32) -> Option<std::ffi::OsString> {
        None
    }

    pub(super) fn arguments_of(_pid: u32) -> io::Result<Vec<std::ffi::OsString>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "install_flip reads a process's arguments on macOS only",
        ))
    }

    pub(super) fn parent_pid(_me: u32) -> Option<u32> {
        None
    }

    pub(super) fn signal(_process: Running, _ask: Ask) -> io::Result<bool> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "install_flip asks a process to quit on macOS and Windows only",
        ))
    }

    pub(super) fn held_open(_path: &Path) -> io::Result<bool> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "install_flip asks whether a file is held open on Windows only",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_txn::Replace;
    use crate::install_txn::recording::{Call, Recorder};
    use std::path::PathBuf;

    /// RED (U-28) — **an exchange is one call that swaps the two names, then
    /// a flush of each directory, the first directory first** — and a refused
    /// exchange flushes nothing.
    ///
    /// MUTATION: flush only `a`'s directory in `durable_exchange_with`.
    #[test]
    fn an_exchange_is_one_swap_then_both_directories_flushed() {
        let live = PathBuf::from("parent").join("Folio.app");
        let staged = PathBuf::from("parent")
            .join(".Folio.app.folio-update")
            .join("t")
            .join("stage")
            .join("Folio.app");
        let mut recorder = Recorder::default();
        install_txn::durable_exchange_with(&mut recorder, &live, &staged).unwrap();
        let calls = recorder.calls;
        assert_eq!(
            calls.first(),
            Some(&Call::Rename(live.clone(), staged.clone(), Replace::Swap))
        );
        let opened: Vec<&PathBuf> = calls
            .iter()
            .filter_map(|call| match call {
                Call::OpenDirectory(path) => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(
            opened,
            vec![
                &PathBuf::from("parent"),
                &staged.parent().unwrap().to_path_buf()
            ]
        );

        let mut refusing = Recorder::failing(crate::install_txn::recording::Fail::Rename);
        let failure =
            install_txn::durable_exchange_with(&mut refusing, &live, &staged).unwrap_err();
        assert_eq!(failure.stage, install_txn::Stage::Rename);
        assert_eq!(refusing.calls.len(), 1, "{:?}", refusing.calls);
    }

    /// RED (U-28) — **a real exchange on macOS swaps two folders' contents in
    /// place; elsewhere it is refused and changes nothing.**
    ///
    /// MUTATION: `RENAME_EXCL` in place of `RENAME_SWAP` in the macOS arm.
    #[test]
    fn a_real_exchange_swaps_two_folders_or_is_refused() {
        let root = bt_testpath::temp_path("bt-install-flip");
        let _ = std::fs::remove_dir_all(&root);
        let (a, b) = (
            root.join("a").join("One.app"),
            root.join("b").join("One.app"),
        );
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("which"), b"old").unwrap();
        std::fs::write(b.join("which"), b"new").unwrap();
        let answer = exchange(&a, &b);
        if crate::host_platform() == crate::HostPlatform::MacOs {
            answer.unwrap();
            assert_eq!(std::fs::read(a.join("which")).unwrap(), b"new");
            assert_eq!(std::fs::read(b.join("which")).unwrap(), b"old");
        } else {
            assert_eq!(answer.unwrap_err().error.kind(), io::ErrorKind::Unsupported);
            assert_eq!(std::fs::read(a.join("which")).unwrap(), b"old");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-28; Windows by U-23) — **the process list finds this very test
    /// process by its image, and only while it runs**; a pid with another
    /// start instant is not it.
    ///
    /// MUTATION: `still_running` answers from the pid alone.
    #[test]
    fn the_process_list_finds_a_process_by_its_image() {
        if crate::host_platform() == crate::HostPlatform::OtherUnix {
            assert!(running_from(Path::new("anything")).is_err());
            assert!(!still_running(Running { pid: 1, started: 0 }));
            return;
        }
        let me = std::env::current_exe().unwrap();
        let found = running_from(&me).unwrap();
        let pid = std::process::id();
        let mine = found
            .iter()
            .find(|running| running.pid == pid)
            .copied()
            .expect("this process runs from its own executable");
        assert!(still_running(mine));
        assert!(!still_running(Running {
            pid,
            started: mine.started + 1
        }));
        let elsewhere = bt_testpath::temp_path("bt-flip-none");
        std::fs::write(&elsewhere, b"").unwrap();
        assert!(running_from(&elsewhere).unwrap().is_empty());
        let _ = std::fs::remove_file(&elsewhere);
    }

    /// RED (U-29) — **a signal reaches a recorded process only while the
    /// process list shows that very process running from one of the given
    /// executables**: another start instant, another image or a process that
    /// has ended gets nothing, and `Quit` is `SIGTERM`, `End` is `SIGKILL`.
    ///
    /// Coordinator ruling 2 (U-29): "never touch a process whose image is not
    /// the trial's executable and whose start time is not the journal's". The
    /// processes are `/bin/sleep` on macOS and a synthetic program on Windows
    /// (U-24: the Windows arm is real), started and reaped by this test.
    ///
    /// MUTATION: in `ask`, signal without asking `runs_from` first.
    #[test]
    fn a_process_is_signalled_only_while_its_image_and_start_instant_match() {
        #[cfg(target_os = "macos")]
        signals_reach_only_the_recorded_process();
        #[cfg(windows)]
        a_windows_process_is_asked_only_by_its_identity();
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            // Refused by name where there is no process list.
            let nobody = Running { pid: 1, started: 0 };
            assert!(ask(nobody, &[Path::new("/bin/sleep")], Ask::Quit).is_err());
        }
    }

    /// RED (U-24) — **on Windows a recorded process is asked to quit, and
    /// ended, only while it is that very process — its pid, its creation time
    /// and its image all the recorded ones**: another creation time or another
    /// image gets nothing and runs on; `Quit` leaves a process with no window to
    /// close running (nothing a person could close was there to ask); `End`
    /// ends it, and asking again after it has ended sends nothing.
    ///
    /// W9: "stop the trial process (quits itself; after 5 s grace, ended by its
    /// starter, or by R from the recorded pid and start time)". The programs
    /// are synthetic (they open no window), started from a folder of the
    /// test's own and reaped by their handles.
    ///
    /// MUTATION: in `ask`, call `imp::signal` without asking `runs_from` first
    /// (the handle's creation time still refuses another start, but the image
    /// goes unchecked, and the process asked through another image is ended).
    #[cfg(windows)]
    fn a_windows_process_is_asked_only_by_its_identity() {
        let scratch = Scratch::new("ask");
        let program = scratch.0.join("folio.exe");
        crate::trust_harness::program(
            &program,
            crate::trust::FileVersion([0, 4, 7, 0]),
            crate::trust_harness::Behaviour::StaysUp,
        )
        .unwrap();
        let elsewhere = scratch.0.join("copy").join("folio.exe");
        std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
        std::fs::copy(&program, &elsewhere).unwrap();
        let start = |image: &Path| {
            crate::quiet_command(image)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("the synthetic program starts")
        };
        let mut child = start(&program);
        let me = Running {
            pid: child.id(),
            started: started_of(child.id()).expect("a process just started has a start time"),
        };
        let other_start = Running {
            pid: me.pid,
            started: me.started + 1,
        };
        let asked = [
            ask(other_start, &[&program], Ask::End),
            ask(me, &[&elsewhere], Ask::End),
            ask(me, &[&scratch.0.join("nowhere.exe")], Ask::End),
        ];
        let untouched = still_running(me);
        let quit = ask(me, &[&elsewhere, &program], Ask::Quit);
        let after_quit = still_running(me);
        let ended = ask(me, &[&program], Ask::End);
        let status = child.wait();
        let again = ask(me, &[&program], Ask::End);
        let mut stranger = start(&elsewhere);
        let theirs = Running {
            pid: stranger.id(),
            started: started_of(stranger.id()).unwrap(),
        };
        let not_theirs = ask(theirs, &[&program], Ask::End);
        let stranger_runs = still_running(theirs);
        let _ = stranger.kill();
        let _ = stranger.wait();
        for answer in asked {
            assert!(
                !answer.unwrap(),
                "nothing is sent to a process that is not it"
            );
        }
        assert!(untouched, "the process runs on");
        assert!(quit.unwrap(), "that process is asked");
        assert!(
            after_quit,
            "with no window to close, asking does not end it"
        );
        assert!(ended.unwrap(), "that process is ended");
        assert_eq!(status.unwrap().code(), Some(1));
        assert!(!again.unwrap(), "an ended process is asked nothing");
        assert!(
            !not_theirs.unwrap(),
            "another image's process is asked nothing"
        );
        assert!(stranger_runs, "and runs on");
    }

    #[cfg(target_os = "macos")]
    fn signals_reach_only_the_recorded_process() {
        use std::os::unix::process::ExitStatusExt;

        let sleep = Path::new("/bin/sleep");
        let mut quits = crate::quiet_command(sleep).arg("30").spawn().unwrap();
        let pid = quits.id();
        let started = started_of(pid).expect("a process just started has a start instant");
        let me = Running { pid, started };
        let other_start = Running {
            pid,
            started: started + 1,
        };
        let elsewhere = bt_testpath::temp_path("bt-flip-image");
        std::fs::write(&elsewhere, b"").unwrap();
        assert!(!ask(other_start, &[sleep], Ask::Quit).unwrap());
        assert!(!ask(me, &[&elsewhere], Ask::Quit).unwrap());
        assert!(!ask(me, &[Path::new("/nowhere/at/all")], Ask::Quit).unwrap());
        assert!(runs_from(me, &[&elsewhere, sleep]).unwrap());
        assert!(ask(me, &[&elsewhere, sleep], Ask::Quit).unwrap());
        assert_eq!(quits.wait().unwrap().signal(), Some(libc::SIGTERM));
        assert!(!ask(me, &[sleep], Ask::End).unwrap(), "it has ended");

        let mut ends = crate::quiet_command(sleep).arg("30").spawn().unwrap();
        let pid = ends.id();
        let me = Running {
            pid,
            started: started_of(pid).unwrap(),
        };
        assert!(ask(me, &[sleep], Ask::End).unwrap());
        assert_eq!(ends.wait().unwrap().signal(), Some(libc::SIGKILL));
        let _ = std::fs::remove_file(&elsewhere);
    }

    /// A folder of a test's own under the temporary directory, removed when
    /// dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let root = bt_testpath::temp_path(&format!("bt-u23-flip-{tag}"));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// RED (U-40) — **a `KERN_PROCARGS2` answer gives exactly its counted
    /// arguments**: the count, the executable's path and its padding are
    /// skipped, the environment after them is not read, and an answer that
    /// holds fewer arguments than it counts gives none.
    ///
    /// The macOS updater tells the trial it launched through LaunchServices
    /// from any other process of the same executable by these words.
    ///
    /// MUTATION: in `procargs2_arguments`, start the arguments right after
    /// the path's own NUL (the padding is read as empty arguments). The
    /// boastful count's bound on what is reserved has no observable red here:
    /// a 64-bit host grants the unbounded reservation (tried on Windows).
    #[test]
    fn a_procargs2_answer_gives_exactly_its_counted_arguments() {
        let words: [&[u8]; 4] = [
            b"/Applications/Folio.app/Contents/MacOS/folio",
            b"--update-trial",
            "\u{6587}\u{4ef6}\u{5939} mixed \u{0627}".as_bytes(),
            b"",
        ];
        let mut answer = 4i32.to_ne_bytes().to_vec();
        answer.extend_from_slice(b"/Applications/Folio.app/Contents/MacOS/folio\0\0\0\0\0");
        for word in words {
            answer.extend_from_slice(word);
            answer.push(0);
        }
        answer.extend_from_slice(b"HOME=/Users/someone\0LANG=zh_CN.UTF-8\0");
        let read = procargs2_arguments(&answer).expect("the answer holds its four");
        assert_eq!(read, words.map(<[u8]>::to_vec).to_vec());

        let mut short = 6i32.to_ne_bytes().to_vec();
        short.extend_from_slice(&answer[4..answer.len() - 37]);
        assert_eq!(procargs2_arguments(&short), None, "fewer than it counts");
        assert_eq!(procargs2_arguments(&answer[..3]), None, "no count");

        // A count the bytes cannot hold reserves nothing it cannot use.
        let mut boastful = i32::MAX.to_ne_bytes().to_vec();
        boastful.extend_from_slice(&answer[4..]);
        assert_eq!(
            procargs2_arguments(&boastful),
            None,
            "far fewer than it counts"
        );
    }

    /// RED (U-40) — **a process's arguments are answered only while it is
    /// that very process**: read by its pid and start instant, refused for the
    /// same pid at another instant and once it has ended; off macOS the read
    /// is refused by name.
    ///
    /// MUTATION: in `arguments_of`, read without asking `still_running` first
    /// and after.
    #[test]
    fn a_processs_arguments_are_read_only_while_it_is_that_process() {
        #[cfg(target_os = "macos")]
        {
            // `cat` reads its standard input first, which stays open until
            // the test ends it, so it never reaches the file it names.
            let named = "\u{6587}\u{4ef6} trial";
            let mut child = crate::quiet_command("/bin/cat")
                .args(["-u", "-", named])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let me = Running {
                pid: child.id(),
                started: started_of(child.id()).expect("a process just started runs"),
            };
            let read = arguments_of(me);
            let other_start = arguments_of(Running {
                pid: me.pid,
                started: me.started + 1,
            });
            let _ = child.kill();
            let _ = child.wait();
            let after = arguments_of(me);
            assert_eq!(
                read.unwrap(),
                ["/bin/cat", "-u", "-", named].map(OsString::from),
            );
            assert_eq!(other_start.unwrap_err().kind(), io::ErrorKind::NotFound);
            assert_eq!(after.unwrap_err().kind(), io::ErrorKind::NotFound);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let me = Running {
                pid: std::process::id(),
                started: started_of(std::process::id()).unwrap_or(0),
            };
            let refused = arguments_of(me).unwrap_err().kind();
            assert!(
                matches!(
                    refused,
                    io::ErrorKind::Unsupported | io::ErrorKind::NotFound
                ),
                "{refused:?}"
            );
        }
    }

    /// RED (U-37) — **this process's parent is named by its pid and its start
    /// instant, which comes before this process's own**; a child this process
    /// starts names this process as its parent.
    ///
    /// The update's recovery build excludes its own starter — an ordinary
    /// start that handed itself over — from the trials it counts, by exactly
    /// this identity.
    ///
    /// MUTATION: in `parent_of_this_process`, answer `None`.
    #[test]
    #[cfg(any(windows, target_os = "macos"))]
    fn this_process_names_its_parent_by_pid_and_an_earlier_start() {
        let me = super::Running {
            pid: std::process::id(),
            started: super::started_of(std::process::id()).expect("this process runs"),
        };
        let parent = super::parent_of_this_process().expect("the test runner's parent runs");
        assert_ne!(parent.pid, me.pid);
        assert!(parent.started < me.started, "{parent:?} before {me:?}");
        assert!(super::still_running(parent));
        if let Ok(named) = std::env::var("BT_U37_PARENT_TEST_CHILD") {
            // The child: say what it finds, for the parent to compare.
            std::fs::write(named, format!("{}:{}", parent.pid, parent.started)).unwrap();
            return;
        }
        let answer = bt_testpath::temp_path("bt-u37-parent");
        let _ = std::fs::remove_file(&answer);
        let status = crate::quiet_command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "install_flip::tests::this_process_names_its_parent_by_pid_and_an_earlier_start",
                "--nocapture",
            ])
            .env("BT_U37_PARENT_TEST_CHILD", &answer)
            .status()
            .unwrap();
        assert!(status.success());
        let said = std::fs::read_to_string(&answer).unwrap();
        let _ = std::fs::remove_file(&answer);
        assert_eq!(said, format!("{}:{}", me.pid, me.started));
    }

    /// RED (U-23) — **a synthetic program started from a folder of the
    /// test's own is found by its image while it runs, by its pid and start
    /// time, and is gone from both once it is ended**; a copy of it at
    /// another path, never started, lists nothing.
    ///
    /// The Windows recovery tells an applier that is still alive from one that
    /// died by this list (W3), and waits on a recorded trial by its pid and
    /// start time (W7).
    ///
    /// MUTATION: in the Windows arm, `creation_of` answers the creation time
    /// without asking whether the process is still active.
    #[test]
    fn a_started_program_is_listed_while_it_runs_and_not_after() {
        if crate::host_platform() != crate::HostPlatform::Windows {
            return;
        }
        let scratch = Scratch::new("list");
        let program = scratch.0.join("folio.exe");
        crate::trust_harness::program(
            &program,
            crate::trust::FileVersion([0, 4, 7, 0]),
            crate::trust_harness::Behaviour::StaysUp,
        )
        .unwrap();
        let unstarted = scratch.0.join("copy").join("folio.exe");
        std::fs::create_dir_all(unstarted.parent().unwrap()).unwrap();
        std::fs::copy(&program, &unstarted).unwrap();
        let mut child = crate::quiet_command(&program)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the synthetic program starts");
        let pid = child.id();
        let found = running_from(&program).unwrap();
        let listed = found.iter().find(|running| running.pid == pid).copied();
        let ended = child.kill().and_then(|()| child.wait());
        let listed = listed.expect("the running program is listed by its image");
        assert_eq!(started_of(pid), None, "an ended process has no start time");
        assert!(!still_running(listed), "an ended process does not run");
        ended.unwrap();
        assert!(running_from(&program).unwrap().is_empty());
        assert!(running_from(&unstarted).unwrap().is_empty());
    }

    /// RED (0.4.7 uninstall fix) — **a process runs until its handles are
    /// closed, not until its exit code is said**: held at its end — the exit
    /// code set, the handle table not yet run down — the process that holds
    /// a data directory's claim still runs by its pid and start instant, and
    /// the claim is still refused to anybody else; once it has gone, it does
    /// not run and the claim is free. A process it started while it held the
    /// claim, still running, does not hold the claim: the claim is not
    /// inherited across a spawn.
    ///
    /// The clean-VM rehearsal of 0.4.7 met that stretch: the uninstall door
    /// waits for the Folio that asked for it to end, saw its exit code, took
    /// it for gone, and was refused the claim its asker had not yet let go of.
    ///
    /// MUTATION: in the Windows arm, `creation_of` asks the exit code instead
    /// of whether the process object is signalled (the first assertion goes
    /// red).
    #[test]
    #[cfg(windows)]
    fn a_process_runs_until_its_handles_are_closed_not_until_its_exit_code_is_said() {
        if let Ok(directory) = std::env::var("BT_EXIT_CLAIM_CHILD") {
            // The child: take the claim, start a process that outlives it,
            // name that process, and leave by Folio's own way out.
            let claim = crate::instance::claim_data_directory(Path::new(&directory))
                .expect("the child is the first to claim its folder");
            let program = std::env::var("BT_EXIT_CLAIM_GRANDCHILD").unwrap();
            let grandchild = crate::quiet_command(&program)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            // The stand-in's identity first, so the test can end it whatever
            // happens after this line.
            let stand_in = grandchild.id();
            std::fs::write(
                Path::new(&directory).join("stand-in"),
                format!("{stand_in} {}", started_of(stand_in).unwrap()),
            )
            .unwrap();
            let me = std::process::id();
            std::fs::write(
                Path::new(&directory).join("asker"),
                format!("{me} {}", started_of(me).unwrap()),
            )
            .unwrap();
            // The stand-in outlives this process, and the test ends it by its
            // identity; nothing here waits for it.
            std::mem::forget(grandchild);
            std::mem::forget(claim);
            crate::leave_process(7);
        }
        let scratch = Scratch::new("exit");
        let data = scratch.0.join("Folio");
        std::fs::create_dir(&data).unwrap();
        let program = scratch.0.join("stays.exe");
        crate::trust_harness::program(
            &program,
            crate::trust::FileVersion([0, 4, 7, 0]),
            crate::trust_harness::Behaviour::StaysUp,
        )
        .unwrap();
        // **The stand-in ends with the test, on every road out of it** — an
        // assertion that fails, here or inside the held exit, included: it is
        // not debugged, so nothing else would end it. By its recorded pid and
        // start instant, from its own image, so nothing else is touched.
        struct EndsTheStandIn<'a> {
            data: &'a Path,
            program: &'a Path,
        }
        impl EndsTheStandIn<'_> {
            fn running(&self) -> Option<Running> {
                let said = std::fs::read_to_string(self.data.join("stand-in")).ok()?;
                let (pid, started) = said.split_once(' ')?;
                Some(Running {
                    pid: pid.parse().ok()?,
                    started: started.parse().ok()?,
                })
            }
            fn end(&self) -> bool {
                self.running()
                    .is_some_and(|running| ask(running, &[self.program], Ask::End).unwrap_or(false))
            }
        }
        impl Drop for EndsTheStandIn<'_> {
            fn drop(&mut self) {
                self.end();
            }
        }
        let stand_in = EndsTheStandIn {
            data: &data,
            program: &program,
        };
        let mut command = crate::quiet_command(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "install_flip::tests::a_process_runs_until_its_handles_are_closed_not_until_its_exit_code_is_said",
                "--nocapture",
            ])
            .env("BT_EXIT_CLAIM_CHILD", &data)
            .env("BT_EXIT_CLAIM_GRANDCHILD", &program)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let asker = || -> Running {
            let said = std::fs::read_to_string(data.join("asker")).unwrap();
            let (pid, started) = said.split_once(' ').unwrap();
            Running {
                pid: pid.parse().unwrap(),
                started: started.parse().unwrap(),
            }
        };
        let (at_exit, status) = crate::trust_harness::stopped_at_exit(&mut command, |pid, code| {
            let child = asker();
            assert_eq!(child.pid, pid);
            (
                code,
                crate::instance::claim_data_directory(&data).is_none(),
                still_running(child),
            )
        })
        .unwrap();
        let child = asker();
        assert!(
            stand_in.running().is_some_and(still_running),
            "the process the child started runs"
        );
        // Asked while the process the child started still runs, which is
        // then ended before anything is asserted.
        let claim = crate::instance::claim_data_directory(&data);
        assert!(stand_in.end());
        assert_eq!(
            at_exit,
            (7, true, true),
            "held at its end: (its exit code, the claim still held, still running)"
        );
        assert_eq!(status.code(), Some(7));
        assert!(
            !still_running(child),
            "a process that has gone does not run"
        );
        assert_eq!(started_of(child.pid), None);
        assert!(
            claim.is_some(),
            "a process started while the claim was held does not hold it"
        );
    }

    /// RED (U-23) — **a file whose image a running process maps is held open,
    /// and the same file is not once the process is ended; a file nobody has
    /// open is not held open, and the look leaves it as it was.**
    ///
    /// E-7's process check, on a real held-open file: the Windows applier
    /// refuses before its first move when any file it would move is held
    /// open by another process.
    ///
    /// MUTATION: in the Windows arm, open for reading only (a running image
    /// then opens: the loader keeps no handle, only its section).
    #[test]
    fn a_file_a_running_process_maps_is_held_open() {
        let scratch = Scratch::new("held");
        let quiet = scratch.0.join("uninstall.cmd");
        std::fs::write(&quiet, b"@rem nobody has this open").unwrap();
        if crate::host_platform() != crate::HostPlatform::Windows {
            assert_eq!(
                held_open(&quiet).unwrap_err().kind(),
                io::ErrorKind::Unsupported
            );
            return;
        }
        assert!(!held_open(&quiet).unwrap());
        assert_eq!(std::fs::read(&quiet).unwrap(), b"@rem nobody has this open");
        let program = scratch.0.join("folio.exe");
        crate::trust_harness::program(
            &program,
            crate::trust::FileVersion([0, 4, 6, 0]),
            crate::trust_harness::Behaviour::StaysUp,
        )
        .unwrap();
        let mut child = crate::quiet_command(&program)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the synthetic program starts");
        let while_running = held_open(&program);
        let ended = child.kill().and_then(|()| child.wait());
        drop(child);
        assert!(while_running.unwrap(), "a running image is held open");
        ended.unwrap();
        // The loader's section goes with the process's last reference, a
        // moment after the process is signalled.
        let ended_at = std::time::Instant::now();
        while held_open(&program).unwrap() {
            assert!(
                ended_at.elapsed() < std::time::Duration::from_secs(10),
                "an ended process's image is let go"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        println!(
            "U-23: the image was let go {:?} after the process ended",
            ended_at.elapsed()
        );
        assert_eq!(
            held_open(&scratch.0.join("absent")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
