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
//! * **[`still_running`]**: whether a process recorded by its pid *and* its
//!   start time still runs — a pid alone may have been reused (F-7). The start
//!   time is `proc_pidinfo`'s on macOS (microseconds since the epoch) and
//!   `GetProcessTimes`'s creation time on Windows (100 ns since 1601); a
//!   Windows process that has exited while a handle keeps its record is not
//!   running.
//! * **[`ask`]** (U-29): a signal to a recorded process — `SIGTERM` to ask it
//!   to quit, `SIGKILL` to end it — sent only after the process list shows
//!   that very process (pid *and* start instant) running from one of the
//!   given executables. The rollback stops the trial this way (§(b).2 W9/M9):
//!   the trial was started by LaunchServices, so its stopper is not its
//!   parent and has only the journal's record to go by. macOS only; on
//!   Windows the signal is refused by name (the Windows rollback is U-24's).
//! * **[`held_open`]** (Windows): whether another process holds a file open,
//!   asked by opening it for reading and writing with no sharing at all —
//!   E-7's process check before the first move. Any other handle on the file
//!   refuses that open, and so does a running image or a loaded library (the
//!   section the loader maps refuses a writer), with a sharing violation. A
//!   rename alone would not find a running image: Windows lets one be moved.
//!   Nothing is written, and the handle is closed at once, so the check never
//!   stands in a move's way. Refused by name elsewhere.
//!
//! Worker only: the exchange flushes to the device. Refused by name (or an
//! empty answer, for the reads) where there is no arm.

use std::io;
use std::path::Path;

use crate::install_txn::{self, Failure};

/// **A process, by its pid and the instant it started** (macOS: microseconds
/// since the epoch, as `proc_pidinfo`'s `PROC_PIDTBSDINFO` reports it;
/// Windows: the creation time `GetProcessTimes` reports, in 100 ns since
/// 1601): together they name one process for its whole life, where a pid alone
/// may be reused.
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
/// `Unsupported` off macOS.
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

/// **What [`ask`] asks of a process.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// `SIGTERM`: the ordinary request to quit, which a process may answer
    /// in its own time (or ignore).
    Quit,
    /// `SIGKILL`: the end, which no process can refuse.
    End,
}

/// **Whether `process` is running from one of `images`**: the process list of
/// each executable ([`running_from`]) names its pid with the same start
/// instant. An executable that cannot be looked at (a bundle that is not
/// there) names nothing.
///
/// # Errors
/// The process list could not be read; `Unsupported` off macOS.
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

/// **Send `ask` to `process`, if it is still that process running from one of
/// `images`** — see the module header. Answers whether the signal was sent:
/// `false` when the process list does not show it (it has ended, its pid was
/// reused, or it runs some other program), and nothing is sent then.
///
/// # Errors
/// The process list could not be read, or the signal was refused for a reason
/// other than the process having just ended; `Unsupported` off macOS.
pub fn ask(process: Running, images: &[&Path], ask: Ask) -> io::Result<bool> {
    if !runs_from(process, images)? {
        return Ok(false);
    }
    imp::signal(process.pid, ask)
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

    pub(super) fn signal(pid: u32, ask: Ask) -> io::Result<bool> {
        let pid = libc::pid_t::try_from(pid)
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
    use windows::Win32::Foundation::{ERROR_SHARING_VIOLATION, FILETIME, HANDLE, STILL_ACTIVE};
    use windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    use windows::Win32::System::ProcessStatus::K32EnumProcesses;
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

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
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
        // SAFETY: the handle was opened here, is owned by nothing else, and is
        // closed when `owned` is dropped.
        let owned = unsafe { OwnedHandle::from_raw_handle(process.0) };
        creation_of(HANDLE(owned.as_raw_handle()))
    }

    /// The creation time of the live process behind `process`, or `None` once
    /// it has exited (its record outlives it while any handle is open).
    fn creation_of(process: HANDLE) -> Option<u64> {
        let mut code = 0u32;
        // SAFETY: `process` is live for the call; `code` is writable.
        unsafe { GetExitCodeProcess(process, &raw mut code) }.ok()?;
        if code != STILL_ACTIVE.0.cast_unsigned() {
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

    pub(super) fn signal(_pid: u32, _ask: super::Ask) -> io::Result<bool> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "install_flip signals a process on macOS only",
        ))
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

    pub(super) fn started_of(_pid: u32) -> Option<u64> {
        None
    }

    pub(super) fn signal(_pid: u32, _ask: Ask) -> io::Result<bool> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "install_flip signals a process on macOS only",
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
        let root = std::env::temp_dir().join(format!("bt-install-flip-{}", std::process::id()));
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
        let elsewhere = std::env::temp_dir().join(format!("bt-flip-none-{pid}"));
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
    /// processes are `/bin/sleep`, started and reaped by this test.
    ///
    /// MUTATION: in `ask`, signal without asking `runs_from` first.
    #[test]
    fn a_process_is_signalled_only_while_its_image_and_start_instant_match() {
        #[cfg(target_os = "macos")]
        signals_reach_only_the_recorded_process();
        #[cfg(not(target_os = "macos"))]
        {
            // No signal is sent: refused by name where there is no process
            // list, and on Windows (a real list since U-23) the program is not
            // there, so nothing runs from it.
            let nobody = Running { pid: 1, started: 0 };
            assert!(!matches!(
                ask(nobody, &[Path::new("/bin/sleep")], Ask::Quit),
                Ok(true)
            ));
        }
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
        let elsewhere = std::env::temp_dir().join(format!("bt-flip-image-{pid}"));
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
            let root = std::env::temp_dir().join(format!(
                "bt-u23-flip-{tag}-{}-{}",
                std::process::id(),
                crate::attention_pipe::unguessable_bits() % 1_000_000
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
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
