//! **The folder a process is standing in** — its current directory, read from outside it
//! (T-CWD-FROM-PROCESS).
//!
//! The one door through which `bt-app` learns where a pane's shell stands when the shell itself
//! has said nothing (no OSC 7: a shell without Folio's integration, a row whose arguments refused
//! the injection, a program that is not a shell). It answers the folder or a named refusal; it
//! never guesses one.
//!
//! * **Windows**: one `OpenProcess` for `PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ`,
//!   and on that one handle `QueryFullProcessImageNameW` (the bridge check),
//!   `NtQueryInformationProcess` (`ProcessWow64Information`, then `ProcessBasicInformation`), and
//!   three `ReadProcessMemory` reads — the PEB's `ProcessParameters` pointer,
//!   `RTL_USER_PROCESS_PARAMETERS.CurrentDirectory.DosPath`, and the path's characters. A 32-bit
//!   process on a 64-bit Windows (WOW64) keeps its current directory in its own 32-bit PEB, which
//!   `ProcessWow64Information` locates; it is read with the 32-bit layout rather than refused.
//! * **macOS**: `proc_pidpath` (the bridge check), then one `proc_pidinfo(PROC_PIDVNODEPATHINFO)`,
//!   whose `pvi_cdir` is the path of the process's current-directory vnode.
//!
//! The call is bounded by the calls it makes: nothing here waits on another thread or process, so
//! it may be asked on the window thread when a person asks for the folder.
//!
//! **A bridge to another machine is refused, not read** (the foreground-program door's boundary,
//! [`crate::foreground_program`]): the local folder of `ssh` or `wsl` says nothing about where the
//! shell on the far side stands.
//!
//! # And the command line this process's parent was started with
//!
//! [`parent_arguments`] (0.4.10 plan row 74): the words the operating system holds for the process
//! that started this one — asked by `folio attention`, whose parent is the agent that ran its hook,
//! so that a resumed session can be handed the flags the agent was started with. Read only, and
//! answered only while that very parent runs: the parent is taken by its pid **and** its start
//! instant ([`crate::install_flip::parent_of`]), and the start is asked again after the read, so a
//! parent that ended — and a pid reused since — is never answered for. And
//! [`arguments_of_parent_of`] the same, one generation up (row 74's review N1): the parent of a
//! process this one descends from, for a hook an agent runs through a shell, with the child it was
//! asked about held to the same rule — it must still run as recorded after the read too. Which
//! step to take, and whether to take it, is the caller's.
//!
//! * **Windows**: the same one `OpenProcess` as the folder's, and the same PEB walk, to
//!   `RTL_USER_PROCESS_PARAMETERS.CommandLine` (the `UNICODE_STRING` `winternl.h` names beside
//!   `ImagePathName`), whose characters are split into words by `CommandLineToArgvW`. That is the
//!   rule of the Windows shell, and nearly the MSVC C runtime's, which Rust's standard library
//!   follows too: the two part only on a doubled quote inside a quoted run (`"a""b c"`), which
//!   `CommandLineToArgvW` reads as a literal quote that also ends the quoted run, where the runtime
//!   since 2008 stays inside it, so a word holding `""` may be recorded otherwise than the agent
//!   read it (review N5, a named limit; re-review F3). A 32-bit parent is read from its own PEB.
//! * **macOS**: `sysctl(KERN_PROCARGS2)`, through [`crate::install_flip::arguments_of`].
//!
//! Bounded as the folder's read is: a `UNICODE_STRING` holds at most 32,767 characters, and
//! `KERN_PROCARGS2` answers at most `KERN_ARGMAX` bytes. Nothing waits on another thread or
//! process.

use std::ffi::OsString;
use std::path::PathBuf;

/// Why a process's folder could not be read. Each is said to the person as "folder unknown"; the
/// variants keep the cause for a diagnostic and for the tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FolderRefusal {
    /// This account may not read that process (another user's, or a protected one).
    NoAccess,
    /// The process has ended — between the moment its id was taken and the read, or before.
    Exited,
    /// The process is a bridge to another machine (`ssh`, `wsl`), whose local folder is not the
    /// far side's.
    FarSide,
    /// The process answered, but what it answered is not a folder (a read that came back short, a
    /// length that does not fit its own buffer, an empty path).
    Unreadable,
    /// This platform has no reading of another process's folder.
    Unsupported,
}

/// The current directory of the live process `pid`, or why it cannot be said.
///
/// # Errors
/// A [`FolderRefusal`] naming the reason.
pub fn current_directory(pid: u32) -> Result<PathBuf, FolderRefusal> {
    imp::current_directory(pid)
}

/// Why the parent's command line could not be read. A caller reads every one of them as "the
/// launch is not known"; the variants keep the cause for the tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArgumentsRefusal {
    /// This account may not read the parent (another user's, or a protected one).
    NoAccess,
    /// The parent has ended, or its pid now names a process that started after this one.
    Exited,
    /// The parent answered, but not with a command line (a short read, an empty line).
    Unreadable,
    /// This platform has no reading of another process's command line.
    Unsupported,
}

/// **A process this one descends from, and the words it was started with.**
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AncestorLine {
    /// The process, by its pid and start instant.
    pub process: crate::install_flip::Running,
    /// Its command line, its program's own word first.
    pub words: Vec<OsString>,
}

/// **The command line this process's parent was started with** — see the module header.
///
/// # Errors
/// An [`ArgumentsRefusal`] naming the reason.
pub fn parent_arguments() -> Result<AncestorLine, ArgumentsRefusal> {
    imp::parent_arguments()
}

/// **The command line `child`'s parent was started with** — `child` a process this one descends
/// from, by its pid and start instant: the parent taken as [`parent_arguments`] takes it, started
/// before `child`, and both still running as recorded before the open and after the read (row
/// 74's review N1).
///
/// # Errors
/// An [`ArgumentsRefusal`] naming the reason; `Exited` when `child` or its parent does not run as
/// recorded.
pub fn arguments_of_parent_of(
    child: crate::install_flip::Running,
) -> Result<AncestorLine, ArgumentsRefusal> {
    imp::arguments_of_parent_of(child)
}

/// A bridge to another machine, by its image name as the foreground-program door spells it.
#[cfg(any(windows, target_os = "macos", test))]
fn is_far_side(image: &std::path::Path) -> bool {
    crate::foreground_program::crosses_remote_boundary(&crate::foreground_program::canonical_image(
        &image.to_string_lossy(),
    ))
}

/// Where the process parameters, the current directory and the command line stand in one PEB
/// layout.
///
/// The offsets are the documented `PEB` and `RTL_USER_PROCESS_PARAMETERS` layouts, which Windows
/// has kept since NT for both widths: `PEB.ProcessParameters` at `0x20` (64-bit) or `0x10`
/// (32-bit); `RTL_USER_PROCESS_PARAMETERS.CurrentDirectory` — a `CURDIR` whose first member is the
/// `UNICODE_STRING DosPath` — at `0x38` or `0x24`; `RTL_USER_PROCESS_PARAMETERS.CommandLine`, the
/// `UNICODE_STRING` after `ImagePathName` (`winternl.h`: sixteen bytes and ten pointers, then the
/// two strings), at `0x70` or `0x40`; and in a `UNICODE_STRING` the byte length first and the
/// buffer pointer at `8` or `4`.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PebLayout {
    parameters_at: u64,
    current_directory_at: u64,
    command_line_at: u64,
    buffer_at: u64,
    pointer_bytes: usize,
}

#[cfg(any(windows, test))]
impl PebLayout {
    const NATIVE_64: Self = Self {
        parameters_at: 0x20,
        current_directory_at: 0x38,
        command_line_at: 0x70,
        buffer_at: 8,
        pointer_bytes: 8,
    };
    const WOW64: Self = Self {
        parameters_at: 0x10,
        current_directory_at: 0x24,
        command_line_at: 0x40,
        buffer_at: 4,
        pointer_bytes: 4,
    };
}

/// Another process's memory, as the PEB walk reads it: `true` when every byte of `into` was read
/// from `address`.
#[cfg(any(windows, test))]
trait ProcessMemory {
    fn read(&self, address: u64, into: &mut [u8]) -> bool;
}

/// A little-endian pointer of `bytes` width at `address`.
#[cfg(any(windows, test))]
fn read_pointer(memory: &dyn ProcessMemory, address: u64, bytes: usize) -> Option<u64> {
    let mut raw = [0u8; 8];
    let into = raw.get_mut(..bytes)?;
    memory.read(address, into).then(|| u64::from_le_bytes(raw))
}

/// **The current directory's UTF-16 code units**, walked from the PEB at `peb` in `layout`.
///
/// `None` for any read that does not come back whole, a null pointer, an odd or empty length:
/// each is a process that did not answer with a folder.
#[cfg(any(windows, test))]
fn current_directory_units(
    memory: &dyn ProcessMemory,
    peb: u64,
    layout: PebLayout,
) -> Option<Vec<u16>> {
    parameters_string_units(memory, peb, layout, layout.current_directory_at)
}

/// **The command line's UTF-16 code units**, walked from the PEB at `peb` in `layout`, by the
/// folder's rules: `None` for anything that is not a whole, non-empty string.
#[cfg(any(windows, test))]
fn command_line_units(memory: &dyn ProcessMemory, peb: u64, layout: PebLayout) -> Option<Vec<u16>> {
    parameters_string_units(memory, peb, layout, layout.command_line_at)
}

/// The `UNICODE_STRING` at `field` of the process parameters the PEB at `peb` points at.
#[cfg(any(windows, test))]
fn parameters_string_units(
    memory: &dyn ProcessMemory,
    peb: u64,
    layout: PebLayout,
    field: u64,
) -> Option<Vec<u16>> {
    let parameters = read_pointer(
        memory,
        peb.checked_add(layout.parameters_at)?,
        layout.pointer_bytes,
    )?;
    if parameters == 0 {
        return None;
    }
    let string = parameters.checked_add(field)?;
    let mut length = [0u8; 2];
    if !memory.read(string, &mut length) {
        return None;
    }
    let length = usize::from(u16::from_le_bytes(length));
    let buffer = read_pointer(
        memory,
        string.checked_add(layout.buffer_at)?,
        layout.pointer_bytes,
    )?;
    if length == 0 || length % 2 != 0 || buffer == 0 {
        return None;
    }
    let mut bytes = vec![0u8; length];
    if !memory.read(buffer, &mut bytes) {
        return None;
    }
    Some(
        bytes
            .chunks_exact(2)
            .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
            .collect(),
    )
}

/// The folder a `CurDir` DOS path names: Windows keeps it with a trailing separator
/// (`C:\work\`), which a folder's own spelling does not carry (`C:\work`); a root keeps its own
/// (`C:\`).
#[cfg(windows)]
fn folder_of_dos_path(units: &[u16]) -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    std::path::Path::new(&std::ffi::OsString::from_wide(units))
        .components()
        .collect()
}

/// The path in a NUL-ended C string of `bytes` (macOS's `vip_path`), or `None` when it is empty.
#[cfg(any(target_os = "macos", test))]
fn path_of_c_bytes(bytes: &[u8]) -> Option<PathBuf> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let path = bytes.get(..end)?;
    if path.is_empty() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(path)))
    }
    #[cfg(not(unix))]
    {
        Some(PathBuf::from(String::from_utf8_lossy(path).into_owned()))
    }
}

/// What a failed `proc_pidinfo` means, by the `errno` it left: no such process is an exit, a
/// permission refusal is no access, and anything else is a read that did not answer.
#[cfg(any(target_os = "macos", test))]
fn refusal_of_errno(errno: i32, no_such_process: i32, not_permitted: i32) -> FolderRefusal {
    if errno == no_such_process {
        FolderRefusal::Exited
    } else if errno == not_permitted {
        FolderRefusal::NoAccess
    } else {
        FolderRefusal::Unreadable
    }
}

#[cfg(windows)]
mod imp {
    use super::{AncestorLine, ArgumentsRefusal, FolderRefusal, PebLayout, ProcessMemory};
    use crate::install_flip::Running;
    use std::{
        ffi::OsString,
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        path::PathBuf,
    };
    use windows::{
        Wdk::System::Threading::{
            NtQueryInformationProcess, ProcessBasicInformation, ProcessWow64Information,
        },
        Win32::{
            Foundation::{
                ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, HANDLE,
                HLOCAL, LocalFree, STILL_ACTIVE,
            },
            System::{
                Diagnostics::Debug::ReadProcessMemory,
                Threading::{
                    GetExitCodeProcess, OpenProcess, PROCESS_BASIC_INFORMATION, PROCESS_NAME_WIN32,
                    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ, QueryFullProcessImageNameW,
                },
            },
            UI::Shell::CommandLineToArgvW,
        },
    };

    // The native walk reads the 64-bit layout: a 32-bit build of this crate would misread every
    // 64-bit process, so it does not build.
    const _: () = assert!(
        size_of::<usize>() == 8,
        "the native PEB walk is the 64-bit layout"
    );

    struct Process(OwnedHandle);

    impl Process {
        fn handle(&self) -> HANDLE {
            HANDLE(self.0.as_raw_handle())
        }

        /// Whether the process has ended: the one question that tells an exit from a read that
        /// failed on a live process.
        fn has_exited(&self) -> bool {
            let mut code = 0u32;
            // SAFETY: the handle is live for `self` and was opened with query access; `code` is
            // writable for the call.
            let asked = unsafe { GetExitCodeProcess(self.handle(), &raw mut code) };
            asked.is_ok() && code != STILL_ACTIVE.0.cast_unsigned()
        }

        /// **Which executable this process is running**, asked of the handle already held — the
        /// one open the read makes. The buffer starts at a path's usual length and doubles while
        /// the answer does not fit, up to the longest path Windows names (32,767 units); any other
        /// failure is no answer, and the bridge check then has nothing to refuse on.
        fn image(&self) -> Option<PathBuf> {
            use std::os::windows::ffi::OsStringExt;
            const LONGEST: usize = 32_768;
            let mut room = 260usize;
            loop {
                let mut buffer = vec![0u16; room];
                let mut length = u32::try_from(room).ok()?;
                // SAFETY: the handle is live for `self` and has query access; the buffer and the
                // length outlive the call, which writes at most `length` units and stores the count.
                let asked = unsafe {
                    QueryFullProcessImageNameW(
                        self.handle(),
                        PROCESS_NAME_WIN32,
                        windows::core::PWSTR(buffer.as_mut_ptr()),
                        &raw mut length,
                    )
                };
                match asked {
                    Ok(()) => {
                        let length = usize::try_from(length).ok()?;
                        return Some(PathBuf::from(std::ffi::OsString::from_wide(
                            buffer.get(..length)?,
                        )));
                    }
                    Err(error)
                        if error.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult()
                            && room < LONGEST =>
                    {
                        room = (room * 2).min(LONGEST);
                    }
                    Err(_) => return None,
                }
            }
        }

        /// One query of `class` into a value of `T`'s size.
        fn query<T: Default>(
            &self,
            class: windows::Wdk::System::Threading::PROCESSINFOCLASS,
        ) -> Option<T> {
            let mut value = T::default();
            let size = u32::try_from(size_of::<T>()).ok()?;
            let mut written = 0u32;
            // SAFETY: `value` is writable for `size` bytes, the handle has query access, and the
            // class's answer is exactly a `T` (the callers pair each class with its type).
            let status = unsafe {
                NtQueryInformationProcess(
                    self.handle(),
                    class,
                    (&raw mut value).cast(),
                    size,
                    &raw mut written,
                )
            };
            (status.is_ok() && written == size).then_some(value)
        }
    }

    impl ProcessMemory for Process {
        fn read(&self, address: u64, into: &mut [u8]) -> bool {
            let Ok(address) = usize::try_from(address) else {
                return false;
            };
            let mut read = 0usize;
            // SAFETY: `into` is writable for its length; the call copies at most that many bytes
            // and fails, writing nothing it reports, for an address the process does not map.
            let asked = unsafe {
                ReadProcessMemory(
                    self.handle(),
                    std::ptr::with_exposed_provenance::<core::ffi::c_void>(address),
                    into.as_mut_ptr().cast(),
                    into.len(),
                    Some(&raw mut read),
                )
            };
            asked.is_ok() && read == into.len()
        }
    }

    /// Why a process could not be opened for reading: the cause each door names in its own words.
    enum Unopened {
        NoAccess,
        Exited,
        Unreadable,
    }

    /// **One process, opened to be read**: `PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ`,
    /// the one open each door makes.
    fn opened_for_reading(pid: u32) -> Result<Process, Unopened> {
        // SAFETY: a call taking flags and an integer; it dereferences nothing and answers an error
        // for a process id that has gone or that this token may not open.
        let opened = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
                false,
                pid,
            )
        };
        match opened {
            // SAFETY: the handle was opened here and is owned by this value alone.
            Ok(handle) => Ok(Process(unsafe { OwnedHandle::from_raw_handle(handle.0) })),
            Err(error) if error.code() == ERROR_ACCESS_DENIED.to_hresult() => {
                Err(Unopened::NoAccess)
            }
            Err(error) if error.code() == ERROR_INVALID_PARAMETER.to_hresult() => {
                Err(Unopened::Exited)
            }
            Err(_) => Err(Unopened::Unreadable),
        }
    }

    impl Process {
        /// **One string of the process parameters, read from the PEB this process keeps them
        /// in**: a WOW64 process's own 32-bit PEB (which `ProcessWow64Information` locates), any
        /// other's native one. `walk` is the string's own walk
        /// ([`super::current_directory_units`], [`super::command_line_units`]), handed the PEB
        /// and the layout it is read in.
        fn parameters_string(
            &self,
            walk: fn(&dyn ProcessMemory, u64, PebLayout) -> Option<Vec<u16>>,
        ) -> Option<Vec<u16>> {
            match self.query::<usize>(ProcessWow64Information) {
                // A WOW64 process: the answer is its 32-bit PEB's address.
                Some(peb32) if peb32 != 0 => u64::try_from(peb32)
                    .ok()
                    .and_then(|peb32| walk(self, peb32, PebLayout::WOW64)),
                Some(_) => self
                    .query::<PROCESS_BASIC_INFORMATION>(ProcessBasicInformation)
                    .and_then(|basic| u64::try_from(basic.PebBaseAddress.addr()).ok())
                    .and_then(|peb| walk(self, peb, PebLayout::NATIVE_64)),
                None => None,
            }
        }
    }

    pub(super) fn current_directory(pid: u32) -> Result<PathBuf, FolderRefusal> {
        let process = opened_for_reading(pid).map_err(|unopened| match unopened {
            Unopened::NoAccess => FolderRefusal::NoAccess,
            Unopened::Exited => FolderRefusal::Exited,
            Unopened::Unreadable => FolderRefusal::Unreadable,
        })?;
        if let Some(image) = process.image()
            && super::is_far_side(&image)
        {
            return Err(FolderRefusal::FarSide);
        }
        match process.parameters_string(super::current_directory_units) {
            Some(units) => Ok(super::folder_of_dos_path(&units)),
            None if process.has_exited() => Err(FolderRefusal::Exited),
            None => Err(FolderRefusal::Unreadable),
        }
    }

    /// This process's parent's command line (module header).
    pub(super) fn parent_arguments() -> Result<AncestorLine, ArgumentsRefusal> {
        let this = crate::install_flip::this_running_process().ok_or(ArgumentsRefusal::Exited)?;
        arguments_of_parent_of(this)
    }

    /// `child`'s parent's command line (module header): asked only while the parent recorded by
    /// pid and start runs, and `child` too, before the open and again after the read — the handle
    /// held between keeps the process it opened, and a start that still matches after the read is
    /// that same process.
    pub(super) fn arguments_of_parent_of(child: Running) -> Result<AncestorLine, ArgumentsRefusal> {
        use crate::install_flip::{parent_of, still_running};
        let parent = parent_of(child).ok_or(ArgumentsRefusal::Exited)?;
        let both_run = || still_running(parent) && still_running(child);
        if !both_run() {
            return Err(ArgumentsRefusal::Exited);
        }
        let process = opened_for_reading(parent.pid).map_err(|unopened| match unopened {
            Unopened::NoAccess => ArgumentsRefusal::NoAccess,
            Unopened::Exited => ArgumentsRefusal::Exited,
            Unopened::Unreadable => ArgumentsRefusal::Unreadable,
        })?;
        let read = process.parameters_string(super::command_line_units);
        if !both_run() {
            return Err(ArgumentsRefusal::Exited);
        }
        match read.and_then(|units| words_of_command_line(&units)) {
            Some(words) => Ok(AncestorLine {
                process: parent,
                words,
            }),
            None if process.has_exited() => Err(ArgumentsRefusal::Exited),
            None => Err(ArgumentsRefusal::Unreadable),
        }
    }

    /// **A command line's words, split by `CommandLineToArgvW`**, the first word's included —
    /// the Windows shell's rule, which parts from the C runtime's only on a doubled quote inside a
    /// quoted run: a literal quote that also ends the run here, where the runtime since 2008 stays
    /// inside it (module header, review N5). `None` when it refuses.
    fn words_of_command_line(units: &[u16]) -> Option<Vec<OsString>> {
        use std::os::windows::ffi::OsStringExt;
        let line: Vec<u16> = units.iter().copied().chain(std::iter::once(0)).collect();
        let mut count = 0i32;
        // SAFETY: `line` is NUL-ended and outlives the call; `count` is writable for it.
        let words =
            unsafe { CommandLineToArgvW(windows::core::PCWSTR(line.as_ptr()), &raw mut count) };
        if words.is_null() {
            return None;
        }
        let count = usize::try_from(count).unwrap_or(0);
        let split = (0..count)
            .map(|index| {
                // SAFETY: the answer holds `count` NUL-ended strings, alive until it is freed below.
                OsString::from_wide(unsafe { (*words.add(index)).as_wide() })
            })
            .collect();
        // SAFETY: `CommandLineToArgvW` documents `LocalFree` as the matching release, and nothing
        // of the answer is read after it.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(words.cast())));
        }
        Some(split)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::FolderRefusal;
    use std::path::PathBuf;

    pub(super) fn current_directory(pid: u32) -> Result<PathBuf, FolderRefusal> {
        let pid = libc::c_int::try_from(pid).map_err(|_| FolderRefusal::Exited)?;
        let mut name = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: `name` is writable for its length, which is the size passed.
        let named = unsafe {
            libc::proc_pidpath(
                pid,
                name.as_mut_ptr().cast(),
                u32::try_from(name.len()).unwrap_or(u32::MAX),
            )
        };
        if named > 0
            && let Some(image) = super::path_of_c_bytes(&name)
            && super::is_far_side(&image)
        {
            return Err(FolderRefusal::FarSide);
        }
        // SAFETY: an all-zero `proc_vnodepathinfo` is a valid value of a plain C struct.
        let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
        let size = libc::c_int::try_from(size_of::<libc::proc_vnodepathinfo>())
            .map_err(|_| FolderRefusal::Unreadable)?;
        // SAFETY: `info` is `size` bytes and lives across the call.
        let written = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDVNODEPATHINFO,
                0,
                (&raw mut info).cast(),
                size,
            )
        };
        if written != size {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            return Err(super::refusal_of_errno(errno, libc::ESRCH, libc::EPERM));
        }
        // SAFETY: `vip_path` is a `[[c_char; 32]; 32]`, which is 1024 contiguous bytes.
        let bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(
                (&raw const info.pvi_cdir.vip_path).cast::<u8>(),
                size_of_val(&info.pvi_cdir.vip_path),
            )
        };
        super::path_of_c_bytes(bytes).ok_or(FolderRefusal::Unreadable)
    }

    /// This process's parent's command line (module header).
    pub(super) fn parent_arguments() -> Result<super::AncestorLine, super::ArgumentsRefusal> {
        let this =
            crate::install_flip::this_running_process().ok_or(super::ArgumentsRefusal::Exited)?;
        arguments_of_parent_of(this)
    }

    /// `child`'s parent's command line (module header), through `install_flip`'s read by pid and
    /// start: a parent that does not run as recorded, before or after the read, is an exit, and so
    /// is a `child` that no longer runs after it; a refused permission is no access; any other
    /// failure is a read that did not answer.
    pub(super) fn arguments_of_parent_of(
        child: crate::install_flip::Running,
    ) -> Result<super::AncestorLine, super::ArgumentsRefusal> {
        use super::ArgumentsRefusal;
        let parent = crate::install_flip::parent_of(child).ok_or(ArgumentsRefusal::Exited)?;
        let words =
            crate::install_flip::arguments_of(parent).map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => ArgumentsRefusal::Exited,
                std::io::ErrorKind::PermissionDenied => ArgumentsRefusal::NoAccess,
                _ => ArgumentsRefusal::Unreadable,
            })?;
        if !crate::install_flip::still_running(child) {
            return Err(ArgumentsRefusal::Exited);
        }
        Ok(super::AncestorLine {
            process: parent,
            words,
        })
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::FolderRefusal;
    use std::path::PathBuf;

    /// The named unsupported-platform refusal.
    pub(super) fn current_directory(_pid: u32) -> Result<PathBuf, FolderRefusal> {
        Err(FolderRefusal::Unsupported)
    }

    /// The named unsupported-platform refusal.
    pub(super) fn parent_arguments() -> Result<super::AncestorLine, super::ArgumentsRefusal> {
        Err(super::ArgumentsRefusal::Unsupported)
    }

    /// The named unsupported-platform refusal.
    pub(super) fn arguments_of_parent_of(
        _child: crate::install_flip::Running,
    ) -> Result<super::AncestorLine, super::ArgumentsRefusal> {
        Err(super::ArgumentsRefusal::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A scripted address space: each stretch of bytes at its address.
    #[derive(Default)]
    struct Script(BTreeMap<u64, Vec<u8>>);

    impl Script {
        fn put(&mut self, address: u64, bytes: &[u8]) {
            self.0.insert(address, bytes.to_vec());
        }
    }

    impl ProcessMemory for Script {
        fn read(&self, address: u64, into: &mut [u8]) -> bool {
            let Some((start, bytes)) = self.0.range(..=address).next_back() else {
                return false;
            };
            let Some(from) = usize::try_from(address - start).ok() else {
                return false;
            };
            match bytes.get(from..from + into.len()) {
                Some(found) => {
                    into.copy_from_slice(found);
                    true
                }
                None => false,
            }
        }
    }

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn utf16_bytes(text: &str) -> Vec<u8> {
        units(text)
            .iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect()
    }

    /// Where a process keeps its folder, written out from the documented layouts rather than read
    /// from [`PebLayout`], so that the walk is held to the offsets and not to itself.
    struct Placed {
        parameters_at: u64,
        current_directory_at: u64,
        command_line_at: u64,
        buffer_at: usize,
        width: usize,
    }

    const NATIVE_64_AT: Placed = Placed {
        parameters_at: 0x20,
        current_directory_at: 0x38,
        command_line_at: 0x70,
        buffer_at: 8,
        width: 8,
    };
    const WOW64_AT: Placed = Placed {
        parameters_at: 0x10,
        current_directory_at: 0x24,
        command_line_at: 0x40,
        buffer_at: 4,
        width: 4,
    };

    /// The process parameters every scripted PEB points at.
    const PARAMETERS: u64 = 0x7000;

    /// A `UNICODE_STRING` holding `text` at `string`, its characters at `characters`.
    fn put_unicode_string(
        script: &mut Script,
        string: u64,
        characters: u64,
        text: &str,
        at: &Placed,
    ) {
        let text = utf16_bytes(text);
        let length = u16::try_from(text.len()).unwrap();
        let mut header = vec![0u8; at.buffer_at + at.width];
        header[..2].copy_from_slice(&length.to_le_bytes());
        header[2..4].copy_from_slice(&length.to_le_bytes());
        header[at.buffer_at..at.buffer_at + at.width]
            .copy_from_slice(&characters.to_le_bytes()[..at.width]);
        script.put(string, &header);
        script.put(characters, &text);
    }

    /// A process whose PEB at `peb`, placed as `at`, points at parameters holding `folder`.
    fn process_standing_in(folder: &str, peb: u64, at: &Placed) -> Script {
        let mut script = Script::default();
        script.put(
            peb + at.parameters_at,
            &PARAMETERS.to_le_bytes()[..at.width],
        );
        put_unicode_string(
            &mut script,
            PARAMETERS + at.current_directory_at,
            0x9000,
            folder,
            at,
        );
        script
    }

    /// RED (row 74) — **the command line is read where each width keeps it, and is not the
    /// folder**: a process placed with both strings answers each walk its own — the 64-bit
    /// parameters' `CommandLine` at `0x70`, the WOW64 parameters' at `0x40`.
    ///
    /// MUTATIONS: `PebLayout::NATIVE_64.command_line_at` at `0x60` (`ImagePathName`, which this
    /// script does not map) — that arm reads nothing; `command_line_units` walking
    /// `current_directory_at` — it answers the folder.
    #[test]
    fn the_command_line_is_walked_from_either_peb_layout() {
        let folder = r"C:\工作 work\";
        let line =
            r#""C:\Program Files\nodejs\node.exe" C:\npm\claude-code\cli.js --model "工作 opus""#;
        for (layout, at) in [
            (PebLayout::NATIVE_64, NATIVE_64_AT),
            (PebLayout::WOW64, WOW64_AT),
        ] {
            let mut script = process_standing_in(folder, 0x1000, &at);
            put_unicode_string(
                &mut script,
                PARAMETERS + at.command_line_at,
                0xB000,
                line,
                &at,
            );
            assert_eq!(
                command_line_units(&script, 0x1000, layout),
                Some(units(line)),
                "{layout:?}"
            );
            assert_eq!(
                current_directory_units(&script, 0x1000, layout),
                Some(units(folder)),
                "{layout:?}: the folder is still the folder"
            );
        }
    }

    /// RED — **the folder is read where each width keeps it**: the 64-bit PEB's parameters at
    /// `0x20` and the directory at `0x38` with an 8-byte buffer pointer, the WOW64 PEB's at `0x10`,
    /// `0x24` and 4.
    ///
    /// MUTATIONS (observed red): `PebLayout::WOW64` with the native offsets, and
    /// `PebLayout::NATIVE_64.current_directory_at` at `0x30` — that arm reads nothing.
    #[test]
    fn the_current_directory_is_walked_from_either_peb_layout() {
        let folder = r"C:\工作 work\sub\";
        for (layout, at) in [
            (PebLayout::NATIVE_64, NATIVE_64_AT),
            (PebLayout::WOW64, WOW64_AT),
        ] {
            let script = process_standing_in(folder, 0x1000, &at);
            assert_eq!(
                current_directory_units(&script, 0x1000, layout),
                Some(units(folder)),
                "{layout:?}"
            );
        }
    }

    /// RED — **a process that does not answer a whole folder is refused, never half-read**: a null
    /// parameters pointer, a characters buffer that is not mapped, an odd byte length and an empty
    /// path each answer `None`.
    ///
    /// MUTATION (observed red): drop the length checks — the odd length and the empty one are
    /// answered.
    #[test]
    fn a_process_that_does_not_answer_a_whole_folder_is_unreadable() {
        let layout = PebLayout::NATIVE_64;
        let mut null_parameters = Script::default();
        null_parameters.put(0x1000 + NATIVE_64_AT.parameters_at, &0u64.to_le_bytes());
        assert_eq!(
            current_directory_units(&null_parameters, 0x1000, layout),
            None
        );

        let mut unmapped = process_standing_in(r"C:\a\", 0x1000, &NATIVE_64_AT);
        unmapped.0.remove(&0x9000);
        assert_eq!(current_directory_units(&unmapped, 0x1000, layout), None);

        for length in [0u16, 3] {
            let mut script = process_standing_in(r"C:\a\", 0x1000, &NATIVE_64_AT);
            let dos_path = script
                .0
                .get_mut(&(0x7000 + NATIVE_64_AT.current_directory_at))
                .unwrap();
            dos_path[..2].copy_from_slice(&length.to_le_bytes());
            assert_eq!(
                current_directory_units(&script, 0x1000, layout),
                None,
                "length {length}"
            );
        }
    }

    /// RED (macOS arm, scripted) — **`vip_path` is read up to its NUL**, and an empty path is no
    /// folder.
    #[test]
    fn the_vnode_path_is_read_up_to_its_terminator() {
        let mut path = [0u8; 1024];
        let folder = "/Users/alice/工作 work";
        path[..folder.len()].copy_from_slice(folder.as_bytes());
        assert_eq!(path_of_c_bytes(&path), Some(PathBuf::from(folder)));
        assert_eq!(path_of_c_bytes(&[0u8; 1024]), None);
    }

    /// RED (macOS arm, scripted) — **a failed `proc_pidinfo` is named by its `errno`**: no such
    /// process is an exit, a refused permission is no access, anything else unreadable.
    #[test]
    fn a_failed_vnode_read_is_named_by_its_errno() {
        const ESRCH: i32 = 3;
        const EPERM: i32 = 1;
        assert_eq!(refusal_of_errno(ESRCH, ESRCH, EPERM), FolderRefusal::Exited);
        assert_eq!(
            refusal_of_errno(EPERM, ESRCH, EPERM),
            FolderRefusal::NoAccess
        );
        assert_eq!(
            refusal_of_errno(22, ESRCH, EPERM),
            FolderRefusal::Unreadable
        );
    }

    #[cfg(any(windows, target_os = "macos"))]
    const HELPER_MODE: &str = "FOLIO_PROCESS_FOLDER_HELPER_MODE";

    /// The stand-in process the real reads are asked of: it says it is up, then waits for its
    /// standard input to close. Run only when the test that starts it asks.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn helper_stands_until_released() {
        use std::io::{BufRead as _, Write as _};
        if std::env::var(HELPER_MODE).as_deref() != Ok("stand") {
            return;
        }
        writeln!(std::io::stdout(), "ready").expect("publish readiness");
        std::io::stdout().flush().expect("flush readiness");
        let mut release = String::new();
        let _ = std::io::BufReader::new(std::io::stdin()).read_line(&mut release);
    }

    /// A process this test started, ended through its own handle when the test's scope ends — a
    /// panic included.
    #[cfg(any(windows, target_os = "macos"))]
    struct Started(std::process::Child);

    #[cfg(any(windows, target_os = "macos"))]
    impl Drop for Started {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// A scratch folder this test made, removed when the test's scope ends — a red included, so a
    /// failed run leaves no copy of the test executable behind. Declared before the processes
    /// that stand in it, so they end first.
    #[cfg(any(windows, target_os = "macos"))]
    struct Scratch(std::path::PathBuf);

    #[cfg(any(windows, target_os = "macos"))]
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// `program` (this test executable, or a copy of it under another name) standing in `folder`,
    /// once it has said it is up.
    #[cfg(any(windows, target_os = "macos"))]
    fn standing_helper(program: &std::path::Path, folder: &std::path::Path) -> Started {
        use std::io::BufRead as _;
        let mut child = crate::quiet_command(program)
            .args([
                "--exact",
                "process_folder::tests::helper_stands_until_released",
                "--nocapture",
            ])
            .env(HELPER_MODE, "stand")
            .current_dir(folder)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("start the stand-in");
        let mut output = std::io::BufReader::new(child.stdout.take().expect("stdout was piped"));
        let started = Started(child);
        loop {
            let mut line = String::new();
            assert_ne!(
                output.read_line(&mut line).expect("read readiness"),
                0,
                "the stand-in ended before it was up"
            );
            if line.trim_end() == "ready" {
                return started;
            }
        }
    }

    /// RED (real process) — **a process started in a folder is read standing in it**, a folder
    /// whose name is not ASCII and holds a space included.
    ///
    /// MUTATIONS: `PebLayout::NATIVE_64.current_directory_at` one pointer early (`0x30`, the
    /// standard error handle) — `Unreadable` (Windows, observed red); `pvi_rdir` for `pvi_cdir` —
    /// the root directory, `/` (macOS, the Mac gate's).
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_process_started_in_a_folder_is_read_standing_in_it() {
        let root = bt_testpath::temp_path("process-folder");
        let folder = root.join("工作 folder");
        std::fs::create_dir_all(&folder).expect("create the folder");
        let _scratch = Scratch(root.clone());
        let helper = standing_helper(&std::env::current_exe().unwrap(), &folder);
        let read = current_directory(helper.0.id());
        drop(helper);
        let read = read.expect("the stand-in's folder is readable");
        assert!(
            crate::same_file(&read, &folder),
            "read {read:?}, started in {folder:?}"
        );
    }

    /// RED (real process, Windows) — **a 32-bit process is read from its own PEB**: WOW64's
    /// `cmd.exe`, moved by `cd` into a subfolder, answers the subfolder. WOW64 is part of 64-bit
    /// Windows, so this test has no skip.
    ///
    /// MUTATION (observed red): read every process through `ProcessBasicInformation` and
    /// `PebLayout::NATIVE_64` — the 64-bit PEB's parameters are not where a 32-bit process keeps
    /// its folder (they named the Windows folder, not the subfolder `cd` moved it to).
    #[cfg(windows)]
    #[test]
    fn a_32_bit_process_is_read_from_its_own_peb() {
        use std::io::{BufRead as _, Write as _};
        let system = crate::program_on_path(std::path::Path::new("cmd.exe"))
            .expect("cmd.exe is part of Windows");
        let wow64 = system
            .parent()
            .and_then(std::path::Path::parent)
            .expect("System32's parent")
            .join("SysWOW64")
            .join("cmd.exe");
        let root = bt_testpath::temp_path("process-folder-wow64");
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).expect("create the folders");
        let _scratch = Scratch(root.clone());
        let mut child = crate::quiet_command(&wow64)
            .args(["/d", "/q", "/k", "prompt", "$G$_"])
            .current_dir(&root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("start WOW64's cmd.exe");
        let mut output = std::io::BufReader::new(child.stdout.take().expect("stdout was piped"));
        let mut input = child.stdin.take().expect("stdin was piped");
        let cmd = Started(child);
        input
            .write_all(b"cd sub\r\necho moved\r\n")
            .expect("tell cmd to move");
        input.flush().expect("flush the commands");
        loop {
            let mut line = Vec::new();
            assert_ne!(
                output
                    .read_until(b'\n', &mut line)
                    .expect("read cmd's output"),
                0,
                "cmd ended before it moved"
            );
            if String::from_utf8_lossy(&line).trim_end() == "moved" {
                break;
            }
        }
        let read = current_directory(cmd.0.id());
        drop(input);
        drop(cmd);
        let read = read.expect("WOW64's cmd.exe has a readable folder");
        assert!(
            crate::same_file(&read, &sub),
            "read {read:?}, moved to {sub:?}"
        );
    }

    /// RED (real process) — **a process that has ended is `Exited`**: on Windows its id is still
    /// held by this test's handle, so no other process can have taken it; on macOS the wait reaps
    /// it, and the id is read at once after.
    ///
    /// MUTATIONS: answer `Unreadable` for every failed read on Windows (drop the `has_exited`
    /// arm; observed red); on macOS, map no errno to `Exited` (the Mac gate's).
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_process_that_has_ended_is_exited() {
        let folder = bt_testpath::temp_path("process-folder-ended");
        std::fs::create_dir_all(&folder).expect("create the folder");
        let _scratch = Scratch(folder.clone());
        let mut helper = standing_helper(&std::env::current_exe().unwrap(), &folder);
        drop(helper.0.stdin.take());
        // Its exit status is not the subject: its standard output is already closed, so its
        // harness's last line may fail.
        helper.0.wait().expect("the stand-in ends");
        let read = current_directory(helper.0.id());
        drop(helper);
        assert_eq!(read, Err(FolderRefusal::Exited));
    }

    /// RED (real process) — **a process this account may not read is `NoAccess`**: the System
    /// process (pid 4) on Windows, `launchd` (pid 1) on macOS, neither readable by an
    /// unelevated reader.
    ///
    /// MUTATION (observed red, Windows): `ERROR_ACCESS_DENIED` answered as `Unreadable`.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_process_this_account_may_not_read_is_no_access() {
        let protected = if cfg!(windows) { 4 } else { 1 };
        assert_eq!(current_directory(protected), Err(FolderRefusal::NoAccess));
    }

    /// RED (real process) — **a bridge to another machine is refused before it is read**: this
    /// test executable copied as `ssh` stands in a folder that is readable, and is refused.
    ///
    /// MUTATION (observed red, Windows): drop the `is_far_side` check from `current_directory` —
    /// the copy's folder is answered.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_bridge_process_is_the_far_side() {
        let folder = bt_testpath::temp_path("process-folder-bridge");
        std::fs::create_dir_all(&folder).expect("create the folder");
        let _scratch = Scratch(folder.clone());
        let ssh = folder.join(if cfg!(windows) { "ssh.exe" } else { "ssh" });
        std::fs::copy(std::env::current_exe().unwrap(), &ssh).expect("copy the test executable");
        let helper = standing_helper(&ssh, &folder);
        let read = current_directory(helper.0.id());
        drop(helper);
        assert_eq!(read, Err(FolderRefusal::FarSide));
    }

    /// The full name of [`helper_parents`], as libtest is told to run it.
    #[cfg(any(windows, target_os = "macos"))]
    const PARENT_HELPER: &str = "process_folder::tests::helper_parents";

    /// The word the stand-in parent carries on its command line: a space, a quote and a name that
    /// is not ASCII, each a way a split could go wrong. libtest takes it as one more filter, which
    /// names no test.
    #[cfg(any(windows, target_os = "macos"))]
    const PARENT_MARKER: &str = "工作 \"quoted\" word";

    /// The word the stand-in shell carries on its command line, between the stand-in agent and the
    /// reading child (review N1's T10 variant).
    #[cfg(any(windows, target_os = "macos"))]
    const SHELL_MARKER: &str = "stand-in shell";

    /// **The stand-in parent and the child that reads it** (row 74, T10), run only when the test
    /// that starts it asks. `parent-waits` and `parent-leaves` start this executable again as
    /// `reads-parent`, with their own standard input and output, and either wait for it or end at
    /// once; `reads-parent` waits for a line (or the end) on its standard input, then reads its
    /// parent's command line and prints it — each word hex-encoded, so no word's own characters
    /// can move the line about. **And one generation more** (review N1): `agent-waits` and
    /// `agent-leaves` start a stand-in shell, `shell-hop`, carrying [`SHELL_MARKER`], and wait for
    /// it or end at once; the shell starts `reads-agent` and waits for it, which reads its parent
    /// (the shell) and then that parent's parent (the agent), and prints both.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn helper_parents() {
        use std::io::{BufRead as _, Write as _};
        let say = |label: &str, answer: Result<AncestorLine, ArgumentsRefusal>| {
            let line = match answer {
                Ok(line) => {
                    let mut said = format!("{label} ok");
                    for word in line.words {
                        said.push(' ');
                        for byte in word.to_string_lossy().as_bytes() {
                            said.push_str(&format!("{byte:02x}"));
                        }
                    }
                    said
                }
                Err(refusal) => format!("{label} err {refusal:?}"),
            };
            writeln!(std::io::stdout(), "{line}").expect("say what was read");
            std::io::stdout().flush().expect("flush what was read");
        };
        match std::env::var(HELPER_MODE).as_deref() {
            Ok(mode @ ("parent-waits" | "parent-leaves" | "agent-waits" | "agent-leaves")) => {
                let (args, next): (&[&str], _) = if mode.starts_with("parent") {
                    (&[], "reads-parent")
                } else {
                    (&[SHELL_MARKER], "shell-hop")
                };
                #[expect(
                    clippy::zombie_processes,
                    reason = "`parent-leaves` and `agent-leaves` end before their child on \
                              purpose: that child is the test of a parent that has ended, and an \
                              orphan is the system's to reap"
                )]
                let mut child = crate::quiet_command(std::env::current_exe().unwrap())
                    .args(["--exact", PARENT_HELPER, "--nocapture"])
                    .args(args)
                    .env(HELPER_MODE, next)
                    .spawn()
                    .expect("start the next generation");
                if mode.ends_with("-waits") {
                    child.wait().expect("the child ends");
                }
            }
            Ok("shell-hop") => {
                crate::quiet_command(std::env::current_exe().unwrap())
                    .args(["--exact", PARENT_HELPER, "--nocapture"])
                    .env(HELPER_MODE, "reads-agent")
                    .spawn()
                    .expect("start the reading child")
                    .wait()
                    .expect("the reading child ends");
            }
            Ok(mode @ ("reads-parent" | "reads-agent")) => {
                let mut release = String::new();
                let _ = std::io::BufReader::new(std::io::stdin()).read_line(&mut release);
                let parent = parent_arguments();
                if mode == "reads-agent" {
                    let agent = parent
                        .clone()
                        .and_then(|shell| arguments_of_parent_of(shell.process));
                    say("SHELL", parent);
                    say("PARENT", agent);
                } else {
                    say("PARENT", parent);
                }
            }
            _ => {}
        }
    }

    /// What the reading child said: its parent's words, or the refusal by name.
    #[cfg(any(windows, target_os = "macos"))]
    fn parent_read_back(output: &mut dyn std::io::BufRead) -> Result<Vec<String>, String> {
        line_read_back(output, "PARENT")
    }

    /// What the reading child said under `label`: the words it read, or the refusal by name.
    #[cfg(any(windows, target_os = "macos"))]
    fn line_read_back(
        output: &mut dyn std::io::BufRead,
        label: &str,
    ) -> Result<Vec<String>, String> {
        let (failed, read) = (format!("{label} err "), format!("{label} ok"));
        loop {
            let mut line = String::new();
            assert_ne!(
                output
                    .read_line(&mut line)
                    .expect("read the child's answer"),
                0,
                "the reading child ended without an answer"
            );
            let line = line.trim_end();
            if let Some(refusal) = line.strip_prefix(failed.as_str()) {
                return Err(refusal.to_owned());
            }
            if let Some(words) = line.strip_prefix(read.as_str()) {
                return Ok(words
                    .split_whitespace()
                    .map(|hex| {
                        let bytes: Vec<u8> = (0..hex.len())
                            .step_by(2)
                            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
                            .collect();
                        String::from_utf8(bytes).unwrap()
                    })
                    .collect());
            }
        }
    }

    /// The stand-in parent in `mode`, its standard input and output piped to this test.
    #[cfg(any(windows, target_os = "macos"))]
    fn stand_in_parent(mode: &str) -> std::process::Child {
        crate::quiet_command(std::env::current_exe().unwrap())
            .args(["--exact", PARENT_HELPER, "--nocapture", PARENT_MARKER])
            .env(HELPER_MODE, mode)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("start the stand-in parent")
    }

    /// RED (real process, row 74 T10) — **a child reads its parent's command line word for word**:
    /// the stand-in parent's own words come back, the one holding a space, quotes and a name that
    /// is not ASCII whole, and its program first.
    ///
    /// MUTATIONS: split the line at spaces instead of by `CommandLineToArgvW` — the marker comes
    /// back in pieces (Windows); read `current_directory_at` for the command line — the parent's
    /// folder comes back as one word (Windows); read this process's own arguments — the reading
    /// child's words come back, with no marker.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_child_reads_its_parents_command_line_word_for_word() {
        use std::io::Write as _;
        let mut parent = stand_in_parent("parent-waits");
        let mut output = std::io::BufReader::new(parent.stdout.take().expect("stdout was piped"));
        let mut input = parent.stdin.take().expect("stdin was piped");
        let parent = Started(parent);
        input.write_all(b"go\n").expect("release the reading child");
        input.flush().expect("flush the release");
        let words = parent_read_back(&mut output);
        drop(input);
        drop(parent);
        let words = words.expect("a running parent's command line is readable");
        assert_eq!(
            words.get(1..),
            Some(
                &[
                    "--exact".to_owned(),
                    PARENT_HELPER.to_owned(),
                    "--nocapture".to_owned(),
                    PARENT_MARKER.to_owned(),
                ][..]
            ),
            "the parent's words: {words:?}"
        );
        assert_eq!(
            words
                .first()
                .and_then(|program| std::path::Path::new(program).file_name()),
            std::env::current_exe().unwrap().file_name(),
            "its program first"
        );
    }

    /// RED (real process, row 74 T10) — **a parent that has ended is not read**: the stand-in
    /// parent starts the reading child and ends at once, and the child, asked only after it has
    /// ended, does not answer the parent's words. On Windows the answer is `Exited` (the parent's
    /// pid no longer runs as recorded); on macOS the orphan's parent is `launchd`, whose command
    /// line holds no marker either way.
    ///
    /// MUTATION (of the setup, which is what makes this a test of an ended parent): let
    /// `reads-parent` read before its release line — the parent is still running then, and its
    /// words come back.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_parent_that_has_ended_is_not_read() {
        use std::io::Write as _;
        let mut parent = stand_in_parent("parent-leaves");
        let mut output = std::io::BufReader::new(parent.stdout.take().expect("stdout was piped"));
        let mut input = parent.stdin.take().expect("stdin was piped");
        let mut parent = Started(parent);
        parent.0.wait().expect("the stand-in parent ends");
        input.write_all(b"go\n").expect("release the reading child");
        input.flush().expect("flush the release");
        let read = parent_read_back(&mut output);
        drop(input);
        drop(parent);
        if cfg!(windows) {
            assert_eq!(read, Err("Exited".to_owned()));
        } else {
            assert!(
                !matches!(&read, Ok(words) if words.iter().any(|word| word == PARENT_MARKER)),
                "an ended parent's words are never answered: {read:?}"
            );
        }
    }

    /// RED (real process, row 74 T10, review N1) — **through a shell between, the agent is read
    /// one generation up**: the stand-in agent starts a stand-in shell, which starts the reading
    /// child; the child reads the shell's words (its parent), then the agent's
    /// ([`arguments_of_parent_of`] the shell), each word for word. Which shell line earns that step
    /// is `bt-app`'s (`attention_codex::hook_shell_runs`); this is the door it steps through.
    ///
    /// MUTATIONS: `arguments_of_parent_of` reading `child` itself rather than its parent — the
    /// shell's words come back twice, with no agent marker; `install_flip::parent_of` answering
    /// this process's parent whatever it is asked — the same.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn through_a_shell_between_the_agent_is_read_one_generation_up() {
        use std::io::Write as _;
        let mut agent = stand_in_parent("agent-waits");
        let mut output = std::io::BufReader::new(agent.stdout.take().expect("stdout was piped"));
        let mut input = agent.stdin.take().expect("stdin was piped");
        let agent = Started(agent);
        input.write_all(b"go\n").expect("release the reading child");
        input.flush().expect("flush the release");
        let shell = line_read_back(&mut output, "SHELL");
        let read = line_read_back(&mut output, "PARENT");
        drop(input);
        drop(agent);
        let shell = shell.expect("a running shell's command line is readable");
        assert_eq!(
            shell.last().map(String::as_str),
            Some(SHELL_MARKER),
            "the shell's words: {shell:?}"
        );
        let read = read.expect("a running agent's command line is readable through its shell");
        assert_eq!(
            read.last().map(String::as_str),
            Some(PARENT_MARKER),
            "the agent's words, one generation up: {read:?}"
        );
    }

    /// RED (real process, row 74 T10, review N1) — **an agent that has ended is not read through
    /// its shell**: the stand-in agent starts the stand-in shell and ends at once, and the reading
    /// child, released only after, does not answer the agent's words. On Windows the answer is
    /// `Exited` (the snapshot still names the agent's pid, which no longer runs as recorded); on
    /// macOS the orphaned shell's parent is `launchd`, whose command line holds no marker either
    /// way.
    ///
    /// MUTATION (of the setup, which is what makes this a test of an ended agent): let
    /// `reads-agent` read before its release line — the agent is still running then, and its
    /// words come back.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn an_agent_that_has_ended_is_not_read_through_its_shell() {
        use std::io::Write as _;
        let mut agent = stand_in_parent("agent-leaves");
        let mut output = std::io::BufReader::new(agent.stdout.take().expect("stdout was piped"));
        let mut input = agent.stdin.take().expect("stdin was piped");
        let mut agent = Started(agent);
        agent.0.wait().expect("the stand-in agent ends");
        input.write_all(b"go\n").expect("release the reading child");
        input.flush().expect("flush the release");
        let shell = line_read_back(&mut output, "SHELL");
        let read = line_read_back(&mut output, "PARENT");
        drop(input);
        drop(agent);
        assert!(
            matches!(&shell, Ok(words) if words.last().map(String::as_str) == Some(SHELL_MARKER)),
            "the shell still runs and is read: {shell:?}"
        );
        if cfg!(windows) {
            assert_eq!(read, Err("Exited".to_owned()));
        } else {
            assert!(
                !matches!(&read, Ok(words) if words.iter().any(|word| word == PARENT_MARKER)),
                "an ended agent's words are never answered: {read:?}"
            );
        }
    }

    #[test]
    fn a_bridge_to_another_machine_is_the_far_side() {
        for bridge in [
            "ssh",
            "ssh.exe",
            r"C:\Windows\System32\wsl.exe",
            "/usr/bin/ssh",
        ] {
            assert!(is_far_side(std::path::Path::new(bridge)), "{bridge}");
        }
        assert!(!is_far_side(std::path::Path::new(
            r"C:\Windows\System32\cmd.exe"
        )));
    }
}
