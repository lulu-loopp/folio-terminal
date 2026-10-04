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
    use super::fresh_logon_environment;
    use crate::ThreadPriority;

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
