//! **That a process has ended, as the kernel signals it** (T-INTEGRATION-INJECT-2-BUILD round 4).
//!
//! A test that must act only once a child it started has really gone — its image unmapped, its
//! files free to delete — waits on the process's own exit signal: a handle opened for
//! synchronization while the child is still alive, waited on with `WaitForSingleObject`, bounded
//! by the patience the caller hands in. A completion signal, never a sleep. No product road names
//! it; `bt-pty`'s test shell hands one out (`TestShell::exit_witness`).

use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::time::Duration;

/// A process's exit signal, held open on a handle of its own.
#[derive(Debug)]
pub struct ExitWitness(OwnedHandle);

impl ExitWitness {
    /// Open process `pid` for synchronization. Taken while the process is alive, the handle is
    /// that process's for as long as it is held, whatever ids the system hands out later.
    ///
    /// # Errors
    /// The system's refusal to open it (an id no process holds, or one this account may not see).
    pub fn of(pid: u32) -> std::io::Result<Self> {
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
        // SAFETY: `OpenProcess` asks for a synchronization handle only; on success the handle is
        // moved into an `OwnedHandle`, which closes it exactly once.
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        // SAFETY: `handle` was just returned open by `OpenProcess` and nothing else owns it.
        Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle.0) }))
    }

    /// Whether the process ended within `patience`: `true` once its exit is signalled, `false`
    /// when the patience runs out first.
    #[must_use]
    pub fn ended_within(&self, patience: Duration) -> bool {
        use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::WaitForSingleObject;
        let milliseconds = u32::try_from(patience.as_millis()).unwrap_or(u32::MAX - 1);
        // SAFETY: the handle is live and owned by this witness; the wait neither closes nor
        // changes it, and it is bounded by `milliseconds`.
        let waited = unsafe { WaitForSingleObject(HANDLE(self.0.as_raw_handle()), milliseconds) };
        waited == WAIT_OBJECT_0
    }
}

#[cfg(test)]
mod tests {
    use super::ExitWitness;
    use std::time::Duration;

    /// RED (T-INTEGRATION-INJECT-2-BUILD round 4) — **the witness says a process ended only once
    /// the kernel signals its exit**: a child standing in a wait is not ended at once, and the
    /// same child, killed, is ended within the patience — the signal, not a guess from time.
    ///
    /// MUTATION: answer `ended_within` with `true` without waiting — the live child reads as ended.
    #[test]
    fn a_witness_waits_for_the_exit_signal() {
        let mut child = crate::quiet_command("cmd.exe")
            .args(["/d", "/c", "pause"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("cmd starts");
        let witness = ExitWitness::of(child.id()).expect("the live child opens");
        assert!(
            !witness.ended_within(Duration::ZERO),
            "a child standing in a wait has not ended"
        );
        child.kill().expect("the child is ended");
        assert!(
            witness.ended_within(Duration::from_secs(10)),
            "its exit is signalled"
        );
        let _ = child.wait();
    }
}
