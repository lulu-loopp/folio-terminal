//! **One data directory, one writer** (review row R4-5).
//!
//! Two Folio processes sharing `%APPDATA%\Folio\` had no lock and no re-read
//! between them: each held the whole of `settings.json` and `session.json` in
//! memory from the moment it started, each wrote the whole document back, and
//! the second one to write erased everything the first had done since. Nobody
//! saw it happen — both windows kept showing what they thought was true, and the
//! loss appeared on the launch after that.
//!
//! What this module provides is the primitive that answers it and nothing
//! beyond: a claim on one directory, held for as long as the process that took
//! it is alive. Who takes it, what the process that does not get it may still
//! do, and what the reader is told are `bt-app`'s decisions, and they are made
//! in `bt_app::persist`.
//!
//! # Why a named mutex and not a lock file
//!
//! A lock file is a file, and every question about it is the wrong one. A file
//! left behind by a process that was killed is indistinguishable from one a live
//! process is holding, so the reader has to be given a staleness rule — and
//! there is no honest one: a Folio open for a week is not stale, and a Folio
//! killed a second ago is. The `update` module's own claim file gets away with a
//! five-minute rule because what it guards is one HTTP request; a claim on a
//! whole data directory is held for the life of a window.
//!
//! A named kernel object has no such question in it. The kernel owns the name,
//! the name exists exactly while some handle to it is open, and every handle
//! this process holds is closed when the process ends — killed, crashed, or
//! quit. So "is somebody else running" is answered by the same call that takes
//! the claim, with no timestamp, no heartbeat and no recovery path.
//!
//! # The name
//!
//! `Local\` — the session-local namespace — because two users signed into one
//! machine have two `%APPDATA%` directories and must not lock each other out;
//! and the directory's own spelling folded into the name, because the claim is
//! on a *directory* rather than on the product. A build run with `APPDATA`
//! pointed somewhere else (which is how this repository's own tests and manual
//! checks are run) claims a different name and does not collide with the reader's
//! everyday Folio.

use std::path::Path;

/// A claim on one data directory, released when this value is dropped or when
/// the process ends, whichever comes first.
///
/// There is deliberately no way to ask it anything. It either exists — this
/// process is the one writer — or [`claim_data_directory`] answered `None` and
/// somebody else is.
#[cfg(windows)]
pub struct DataDirectoryClaim {
    handle: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl Drop for DataDirectoryClaim {
    fn drop(&mut self) {
        // The kernel would do this at process exit anyway; doing it here is what
        // makes a claim taken and dropped inside one test not outlive the test.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

// A handle is a kernel object identifier, not a pointer into this process, and
// the only thing done with it is `CloseHandle` on drop.
#[cfg(windows)]
unsafe impl Send for DataDirectoryClaim {}
#[cfg(windows)]
unsafe impl Sync for DataDirectoryClaim {}

/// Take the claim on `directory`, or answer `None` because another live process
/// already holds it.
///
/// **The claim is not released when this returns** — it is released when the
/// returned value is dropped, which for the product is when the process ends.
/// Holding the returned value for less than the life of the process would be a
/// claim that says nothing.
#[cfg(windows)]
#[must_use]
pub fn claim_data_directory(directory: &Path) -> Option<DataDirectoryClaim> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::core::PCWSTR;

    let name: Vec<u16> = std::ffi::OsStr::new(&claim_name(directory))
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // `bInitialOwner` is false: nothing here ever waits on the mutex or signals
    // it. What is being used is the *name*, whose existence is the claim, so
    // ownership — and with it the abandoned-mutex state a killed owner would
    // leave — is never entered at all.
    let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }.ok()?;
    // `CreateMutexW` succeeds and hands back a handle to the *existing* object
    // when the name is taken, which is why the error has to be read even on the
    // success path. The handle is closed rather than kept: holding it would keep
    // the name alive past this process's own claim being refused.
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if already {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(handle);
        }
        return None;
    }
    Some(DataDirectoryClaim { handle })
}

/// The kernel name one directory's claim is taken under.
///
/// Pure, and public to this crate's tests, because the two properties that
/// matter are properties of a string: two spellings of one directory claim one
/// name, and two different directories claim two.
///
/// The spelling is folded the way Windows folds a path — case, and a trailing
/// separator — and then reduced to a fixed-length digest, because a kernel object
/// name is capped at `MAX_PATH` and a `%APPDATA%` under a long user name plus a
/// prefix is not guaranteed to fit. The digest is FNV-1a: this is a name, not a
/// secret, and what it has to do is not collide between the handful of
/// directories one machine has.
#[must_use]
pub fn claim_name(directory: &Path) -> String {
    let folded = directory
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in folded.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("Local\\Folio.data.{hash:016x}")
}

/// A machine with no kernel to ask always answers "you are the one writer",
/// which is true of every platform this build runs on that is not Windows.
#[cfg(not(windows))]
pub struct DataDirectoryClaim;

#[cfg(not(windows))]
#[must_use]
pub fn claim_data_directory(_directory: &Path) -> Option<DataDirectoryClaim> {
    Some(DataDirectoryClaim)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — two spellings of one directory claim one name, and two directories
    /// claim two.
    #[test]
    fn the_name_is_the_directory_folded_the_way_windows_folds_it() {
        let one = claim_name(Path::new(r"C:\Users\Me\AppData\Roaming\Folio"));
        let same = claim_name(Path::new(r"c:\users\me\appdata\roaming\folio\"));
        let other = claim_name(Path::new(r"D:\scratch\Folio"));
        assert_eq!(
            one, same,
            "case and a trailing separator are not a difference"
        );
        assert_ne!(one, other, "two directories are two claims");
        assert!(
            one.starts_with("Local\\Folio.data."),
            "session-local, so two signed-in users do not lock each other out: {one}"
        );
    }

    /// RED (review row R4-5) — **the second process is told it is the second.**
    ///
    /// One process here rather than two, which is the whole point of the
    /// primitive: the claim is a kernel name, so a second attempt at it from
    /// anywhere — this thread, another thread, another process — is refused while
    /// the first is held, and released the moment the first is dropped.
    ///
    /// Red gate: answer `Some` without reading `ERROR_ALREADY_EXISTS` and the
    /// second claim below succeeds.
    #[cfg(windows)]
    #[test]
    fn one_directory_is_claimed_once_and_released_on_drop() {
        let directory = std::env::temp_dir().join(format!(
            "bt-platform-instance-{}-{}",
            std::process::id(),
            line!()
        ));
        let first = claim_data_directory(&directory).expect("the first claim is taken");
        assert!(
            claim_data_directory(&directory).is_none(),
            "a second process must not be handed the same directory to write"
        );
        drop(first);
        assert!(
            claim_data_directory(&directory).is_some(),
            "and the claim is gone the moment its holder is"
        );
    }
}
