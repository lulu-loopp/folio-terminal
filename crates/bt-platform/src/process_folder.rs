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

/// A bridge to another machine, by its image name as the foreground-program door spells it.
#[cfg(any(windows, target_os = "macos", test))]
fn is_far_side(image: &std::path::Path) -> bool {
    crate::foreground_program::crosses_remote_boundary(&crate::foreground_program::canonical_image(
        &image.to_string_lossy(),
    ))
}

/// Where the process parameters and the current directory stand in one PEB layout.
///
/// The offsets are the documented `PEB` and `RTL_USER_PROCESS_PARAMETERS` layouts, which Windows
/// has kept since NT for both widths: `PEB.ProcessParameters` at `0x20` (64-bit) or `0x10`
/// (32-bit); `RTL_USER_PROCESS_PARAMETERS.CurrentDirectory` — a `CURDIR` whose first member is the
/// `UNICODE_STRING DosPath` — at `0x38` or `0x24`; and in a `UNICODE_STRING` the byte length first
/// and the buffer pointer at `8` or `4`.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PebLayout {
    parameters_at: u64,
    current_directory_at: u64,
    buffer_at: u64,
    pointer_bytes: usize,
}

#[cfg(any(windows, test))]
impl PebLayout {
    const NATIVE_64: Self = Self {
        parameters_at: 0x20,
        current_directory_at: 0x38,
        buffer_at: 8,
        pointer_bytes: 8,
    };
    const WOW64: Self = Self {
        parameters_at: 0x10,
        current_directory_at: 0x24,
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
    let parameters = read_pointer(
        memory,
        peb.checked_add(layout.parameters_at)?,
        layout.pointer_bytes,
    )?;
    if parameters == 0 {
        return None;
    }
    let dos_path = parameters.checked_add(layout.current_directory_at)?;
    let mut length = [0u8; 2];
    if !memory.read(dos_path, &mut length) {
        return None;
    }
    let length = usize::from(u16::from_le_bytes(length));
    let buffer = read_pointer(
        memory,
        dos_path.checked_add(layout.buffer_at)?,
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
    use super::{FolderRefusal, PebLayout, ProcessMemory};
    use std::{
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
                STILL_ACTIVE,
            },
            System::{
                Diagnostics::Debug::ReadProcessMemory,
                Threading::{
                    GetExitCodeProcess, OpenProcess, PROCESS_BASIC_INFORMATION, PROCESS_NAME_WIN32,
                    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ, QueryFullProcessImageNameW,
                },
            },
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

    pub(super) fn current_directory(pid: u32) -> Result<PathBuf, FolderRefusal> {
        // SAFETY: a call taking flags and an integer; it dereferences nothing and answers an error
        // for a process id that has gone or that this token may not open.
        let opened = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
                false,
                pid,
            )
        };
        let handle = match opened {
            Ok(handle) => handle,
            Err(error) if error.code() == ERROR_ACCESS_DENIED.to_hresult() => {
                return Err(FolderRefusal::NoAccess);
            }
            Err(error) if error.code() == ERROR_INVALID_PARAMETER.to_hresult() => {
                return Err(FolderRefusal::Exited);
            }
            Err(_) => return Err(FolderRefusal::Unreadable),
        };
        // SAFETY: the handle was opened above and is owned by this value alone.
        let process = Process(unsafe { OwnedHandle::from_raw_handle(handle.0) });
        if let Some(image) = process.image()
            && super::is_far_side(&image)
        {
            return Err(FolderRefusal::FarSide);
        }
        let read = match process.query::<usize>(ProcessWow64Information) {
            // A WOW64 process: the answer is its 32-bit PEB's address.
            Some(peb32) if peb32 != 0 => u64::try_from(peb32).ok().and_then(|peb32| {
                super::current_directory_units(&process, peb32, PebLayout::WOW64)
            }),
            Some(_) => process
                .query::<PROCESS_BASIC_INFORMATION>(ProcessBasicInformation)
                .and_then(|basic| u64::try_from(basic.PebBaseAddress.addr()).ok())
                .and_then(|peb| {
                    super::current_directory_units(&process, peb, PebLayout::NATIVE_64)
                }),
            None => None,
        };
        match read {
            Some(units) => Ok(super::folder_of_dos_path(&units)),
            None if process.has_exited() => Err(FolderRefusal::Exited),
            None => Err(FolderRefusal::Unreadable),
        }
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
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::FolderRefusal;
    use std::path::PathBuf;

    /// The named unsupported-platform refusal.
    pub(super) fn current_directory(_pid: u32) -> Result<PathBuf, FolderRefusal> {
        Err(FolderRefusal::Unsupported)
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
        buffer_at: usize,
        width: usize,
    }

    const NATIVE_64_AT: Placed = Placed {
        parameters_at: 0x20,
        current_directory_at: 0x38,
        buffer_at: 8,
        width: 8,
    };
    const WOW64_AT: Placed = Placed {
        parameters_at: 0x10,
        current_directory_at: 0x24,
        buffer_at: 4,
        width: 4,
    };

    /// A process whose PEB at `peb`, placed as `at`, points at parameters holding `folder`.
    fn process_standing_in(folder: &str, peb: u64, at: &Placed) -> Script {
        let parameters = 0x7000_u64;
        let characters = 0x9000_u64;
        let mut script = Script::default();
        script.put(
            peb + at.parameters_at,
            &parameters.to_le_bytes()[..at.width],
        );
        let text = utf16_bytes(folder);
        let length = u16::try_from(text.len()).unwrap();
        let mut dos_path = vec![0u8; at.buffer_at + at.width];
        dos_path[..2].copy_from_slice(&length.to_le_bytes());
        dos_path[2..4].copy_from_slice(&length.to_le_bytes());
        dos_path[at.buffer_at..at.buffer_at + at.width]
            .copy_from_slice(&characters.to_le_bytes()[..at.width]);
        script.put(parameters + at.current_directory_at, &dos_path);
        script.put(characters, &text);
        script
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
