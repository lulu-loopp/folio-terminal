//! The account environment Windows would give a process started now.

use std::{ffi::OsString, io};

use crate::admission::WorkerCtx;

/// An environment block after its native ordering has been preserved as pairs.
pub type Environment = Vec<(OsString, OsString)>;

/// Ask the host for the current user's fresh logon environment.
///
/// Windows rebuilds it from the account and machine sources. Other platforms
/// have no corresponding door, so their spawns continue to inherit normally.
#[cfg(windows)]
pub fn fresh_logon_environment(worker: &WorkerCtx) -> io::Result<Option<Environment>> {
    let _ = worker;
    platform::fresh_logon_environment().map(Some)
}

#[cfg(not(windows))]
pub fn fresh_logon_environment(worker: &WorkerCtx) -> io::Result<Option<Environment>> {
    let _ = worker;
    Ok(None)
}

/// **The machine's own value of the environment variable `name`**, as the system
/// environment block stores it (`HKLM\SYSTEM\CurrentControlSet\Control\Session
/// Manager\Environment`), expanded the way a process reads it; `None` when the machine sets
/// none, and on a platform with no such block.
///
/// Windows PowerShell 5.1 keys one decision on it (T-INTEGRATION-INJECT-2-BUILD): a session whose
/// inherited `PSModulePath` equals the machine's value is given the account's Documents modules
/// folder in front of it, and any other inherited value is kept as it is. Folio's composition of
/// a pane's module path asks the same question so it adds that folder only where 5.1 would have.
#[cfg(windows)]
pub fn machine_variable(name: &str) -> Option<OsString> {
    platform::machine_variable(name)
}

#[cfg(not(windows))]
pub fn machine_variable(name: &str) -> Option<OsString> {
    let _ = name;
    None
}

#[cfg(windows)]
mod platform {
    use super::Environment;
    use std::{
        ffi::OsString,
        io,
        os::windows::{
            ffi::OsStringExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
        ptr,
    };
    use windows::Win32::{
        Foundation::HANDLE,
        Security::TOKEN_QUERY,
        System::{
            Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock},
            Threading::{GetCurrentProcess, OpenProcessToken},
        },
    };

    struct OwnedBlock(*mut core::ffi::c_void);

    impl Drop for OwnedBlock {
        fn drop(&mut self) {
            // SAFETY: CreateEnvironmentBlock returned this pointer and this guard owns it.
            let _ = unsafe { DestroyEnvironmentBlock(self.0) };
        }
    }

    pub(super) fn machine_variable(name: &str) -> Option<OsString> {
        use windows::Win32::Foundation::ERROR_SUCCESS;
        use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
        use windows::core::PCWSTR;

        let wide = |text: &str| text.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        let key = wide(r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment");
        let value = wide(name);
        // `RRF_RT_REG_SZ` without `RRF_NOEXPAND`: a `REG_EXPAND_SZ` value — which is how Windows
        // stores `PSModulePath` — comes back expanded, as a process sees it.
        let mut size = 0u32;
        // SAFETY: the key and value names are live and NUL-terminated; a null buffer asks only
        // for the size, written to `size`.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                PCWSTR(key.as_ptr()),
                PCWSTR(value.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&raw mut size),
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        // An expanded value can be longer than the stored one the first answer measured; the
        // second call says so, and is asked again at the size it reports.
        for _ in 0..4 {
            let mut buffer = vec![0u16; (size as usize).div_ceil(2).max(1)];
            let mut length = u32::try_from(buffer.len() * 2).ok()?;
            // SAFETY: as above, with a live buffer of `length` bytes.
            let status = unsafe {
                RegGetValueW(
                    HKEY_LOCAL_MACHINE,
                    PCWSTR(key.as_ptr()),
                    PCWSTR(value.as_ptr()),
                    RRF_RT_REG_SZ,
                    None,
                    Some(buffer.as_mut_ptr().cast()),
                    Some(&raw mut length),
                )
            };
            if status == ERROR_SUCCESS {
                let units = (length as usize / 2).min(buffer.len());
                let text = &buffer[..units];
                let text = text.split(|unit| *unit == 0).next().unwrap_or_default();
                return Some(OsString::from_wide(text));
            }
            if status != windows::Win32::Foundation::ERROR_MORE_DATA {
                return None;
            }
            size = length;
        }
        None
    }

    pub(super) fn fresh_logon_environment() -> io::Result<Environment> {
        let mut token = HANDLE::default();
        // SAFETY: the process pseudo-handle needs no close; `token` is writable for the call.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }
            .map_err(io::Error::other)?;
        // SAFETY: OpenProcessToken returned a real owned handle, not a pseudo-handle.
        let token = unsafe { OwnedHandle::from_raw_handle(token.0) };
        let mut raw = ptr::null_mut();
        // FALSE is deliberate: this is the account's block now, without this process's block.
        // SAFETY: `raw` is writable and the queried token remains live through the call.
        unsafe { CreateEnvironmentBlock(&raw mut raw, Some(HANDLE(token.as_raw_handle())), false) }
            .map_err(io::Error::other)?;
        let block = OwnedBlock(raw);

        let mut environment = Vec::new();
        let mut cursor = block.0.cast::<u16>();
        loop {
            // SAFETY: the API returns a double-NUL-terminated UTF-16 block.
            if unsafe { *cursor } == 0 {
                break;
            }
            let mut length = 0usize;
            // SAFETY: each entry is NUL-terminated within that block.
            while unsafe { *cursor.add(length) } != 0 {
                length += 1;
            }
            // SAFETY: `length` was found by walking this entry to its terminator.
            let entry = unsafe { std::slice::from_raw_parts(cursor, length) };
            let separator = if entry.first() == Some(&u16::from(b'=')) {
                entry[1..]
                    .iter()
                    .position(|unit| *unit == u16::from(b'='))
                    .map(|at| at + 1)
            } else {
                entry.iter().position(|unit| *unit == u16::from(b'='))
            };
            if let Some(separator) = separator {
                environment.push((
                    OsString::from_wide(&entry[..separator]),
                    OsString::from_wide(&entry[separator + 1..]),
                ));
            }
            // SAFETY: advance past this entry and its NUL to the next entry or final NUL.
            cursor = unsafe { cursor.add(length + 1) };
        }
        Ok(environment)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::{fresh_logon_environment, machine_variable};
    use crate::ThreadPriority;

    /// The machine block names `OS` and `windir`, as every Windows does: the one a plain string,
    /// the other stored as `%SystemRoot%` and answered expanded, as a process sees it. A name no
    /// machine sets is `None`.
    ///
    /// RED (mutation: pass `RRF_NOEXPAND`) — `windir` comes back as `%SystemRoot%`.
    #[test]
    fn the_machine_block_answers_a_variable_it_sets_and_not_one_it_does_not() {
        assert_eq!(
            machine_variable("OS").as_deref(),
            Some(std::ffi::OsStr::new("Windows_NT"))
        );
        let windir = machine_variable("windir").expect("the machine block names windir");
        assert!(!windir.is_empty());
        assert!(
            !windir.to_string_lossy().contains('%'),
            "expanded: {windir:?}"
        );
        assert_eq!(machine_variable("FOLIO_NO_MACHINE_SETS_THIS_NAME"), None);
    }

    #[test]
    fn the_windows_logon_block_contains_system_root() {
        let worker =
            crate::spawn_at_priority("bt-environment-test", ThreadPriority::BelowNormal, |ctx| {
                fresh_logon_environment(ctx)
            })
            .expect("environment worker");
        let environment = worker
            .join()
            .expect("environment worker panicked")
            .expect("environment block")
            .expect("Windows block");
        assert!(environment.iter().any(|(name, value)| {
            name.to_string_lossy().eq_ignore_ascii_case("SystemRoot") && !value.is_empty()
        }));
    }
}
