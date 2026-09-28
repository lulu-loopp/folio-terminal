//! **A test certificate world for the updater's identity check** — U-15's
//! E-13 harness (`trust_harness_windows.rs`: a root, leaves and a time-stamp
//! authority made with ephemeral keys, small PE files signed by
//! `SignerSignEx3` and time-stamped by an RFC 3161 authority served on the
//! loopback interface, verified under an exclusive-root engine), and the one
//! surface of it another crate's tests use (0.4.6 ticket U-20: `bt-app`'s
//! Windows Prepare is tested against releases signed here).
//!
//! **Tests only.** It is compiled for this crate's own tests and, through the
//! `trust-harness` feature, for the tests of a crate that names the feature on
//! its *dev-dependency* on this one — `bt-app`'s. A build of the shipped
//! program never has it. Nothing here installs a certificate or opens a store
//! other than the memory stores `trust` itself makes; every file it signs is
//! under the system's temporary folder.
//!
//! Off Windows there is no world: [`TestCa::new`] answers `Unsupported`, and a
//! test that needs one says what the product does there instead.

use std::io;
use std::path::Path;

use crate::trust::{FileVersion, Policy};

/// The test publisher's subject, in the order Windows prints a subject.
pub const SUBJECT: &str =
    "CN=Folio Test Publisher, O=Folio Test Publisher, L=Example City, S=mi, C=US";
/// The test publisher's identity OID.
pub const IDENTITY: &str = "1.3.6.1.4.1.311.97.11111111.22222222.33333333.44444444";
/// Another identity OID under the same prefix.
pub const OTHER_IDENTITY: &str = "1.3.6.1.4.1.311.97.55555555.66666666.77777777.88888888";

/// **What a program the harness writes does when it is run** (0.4.6 ticket
/// U-23: the Windows applier's tests start the installed build, the trial and
/// the rescue copy as real processes). Neither imports anything or opens a
/// window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behaviour {
    /// `xor eax, eax; ret`: the process exits at once, with 0.
    Returns,
    /// `Sleep(INFINITE)` in a loop: the process stays up, using no processor,
    /// until it is ended — by the test that started it, by its handle or its
    /// recorded pid.
    StaysUp,
}

/// **Write an unsigned program at `path`**, which must not exist: `behaviour`'s
/// code, carrying `version` as its `VERSIONINFO`.
///
/// # Errors
/// `Unsupported` off Windows.
pub fn program(path: &Path, version: FileVersion, behaviour: Behaviour) -> io::Result<()> {
    #[cfg(windows)]
    {
        world::program_doing(path, version, &[], behaviour);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (path, version, behaviour);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the harness writes Windows programs only",
        ))
    }
}

/// **Open `path` for reading the way a scanner, an indexer or a backup tool
/// may: sharing reading and writing, but not deletion** (0.4.6 ticket U-34).
/// While the handle lives, a rename that replaces `path` is refused with
/// `ERROR_ACCESS_DENIED` — the refusal the applier's journal write met on the
/// clean VM (rows W4 and W12).
///
/// # Errors
/// The open's own error; `Unsupported` off Windows, where no open keeps a
/// rename out.
pub fn hold_without_delete_sharing(path: &Path) -> io::Result<std::fs::File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE).0)
            .open(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "only Windows keeps a rename out of an open file",
        ))
    }
}

/// **A data directory's claim name squatted** (0.4.6 ticket U-34, round 4):
/// `crate::instance::try_claim_data_directory` then answers
/// `ClaimRefusal::QueryDenied` — the question not answered — for as long as
/// the value lives, the shape `instance`'s own test of the two refusals uses:
/// on Windows an event under the claim's kernel name (another kind of named
/// object), on Unix a directory where the lock file goes.
pub struct Squat {
    /// The event, owned: dropping it closes it.
    #[cfg(windows)]
    _event: std::os::windows::io::OwnedHandle,
    /// The directory standing where the lock file goes, removed on drop.
    #[cfg(unix)]
    lock: std::path::PathBuf,
}

#[cfg(unix)]
impl Drop for Squat {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.lock);
    }
}

/// Squat `directory`'s claim name — see [`Squat`].
///
/// # Errors
/// The squatting object could not be made.
pub fn squat_the_claim(directory: &Path) -> io::Result<Squat> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::System::Threading::CreateEventW;
        use windows::core::PCWSTR;
        let name: Vec<u16> = std::ffi::OsStr::new(&crate::instance::claim_name(directory))
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: the name is NUL-terminated and lives across the call.
        let event = unsafe { CreateEventW(None, false, false, PCWSTR(name.as_ptr())) }
            .map_err(|error| io::Error::other(error.to_string()))?;
        use std::os::windows::io::{FromRawHandle, OwnedHandle};
        // SAFETY: the handle was just made by this call and is owned by
        // nothing else.
        Ok(Squat {
            _event: unsafe { OwnedHandle::from_raw_handle(event.0) },
        })
    }
    #[cfg(unix)]
    {
        let runtime = crate::instance::prepare_runtime_directory()?;
        let lock =
            crate::instance::lock_path_in(&runtime, &crate::instance::directory_tag(directory));
        std::fs::create_dir(&lock)?;
        Ok(Squat { lock })
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = directory;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no claim to squat here",
        ))
    }
}

#[cfg(windows)]
#[path = "trust_harness_windows.rs"]
pub mod world;

#[cfg(windows)]
type Inner = world::World;
#[cfg(not(windows))]
type Inner = std::convert::Infallible;

/// **A root and its time-stamp authority**, and the exclusive-root policy
/// that trusts only them.
pub struct TestCa(Inner);

impl TestCa {
    /// A new world: a root and a time-stamp authority, each with a key made
    /// for it and forgotten with it.
    ///
    /// # Errors
    /// `Unsupported` off Windows.
    pub fn new() -> io::Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self(world::World::new()))
        }
        #[cfg(not(windows))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the test certificate world is Windows'",
            ))
        }
    }

    /// The policy under which this world's root is the only trusted root,
    /// with a revocation list that revokes nothing.
    #[must_use]
    pub fn policy(&self) -> Policy {
        #[cfg(windows)]
        {
            self.0.policy()
        }
        #[cfg(not(windows))]
        {
            match self.0 {}
        }
    }

    /// **Write a signed program at `path`**, which must not exist and must be
    /// under the system's temporary folder: a small x64 PE carrying `version`
    /// as its `VERSIONINFO` and each of `resources` as an `RCDATA` resource of
    /// that name, signed by a new three-day leaf of this root for `subject`
    /// and `identity`, and time-stamped now.
    pub fn signed_program(
        &self,
        path: &Path,
        version: FileVersion,
        subject: &str,
        identity: &str,
        resources: &[(&str, &[u8])],
    ) {
        self.signed_program_that(
            path,
            version,
            subject,
            identity,
            resources,
            Behaviour::Returns,
        );
    }

    /// [`TestCa::signed_program`], whose code is `behaviour`'s.
    pub fn signed_program_that(
        &self,
        path: &Path,
        version: FileVersion,
        subject: &str,
        identity: &str,
        resources: &[(&str, &[u8])],
        behaviour: Behaviour,
    ) {
        #[cfg(windows)]
        {
            world::program_doing(path, version, resources, behaviour);
            let leaf = self.0.leaf(subject, identity);
            world::sign(
                path,
                &leaf,
                &[&self.0.root],
                Some(&self.0.stamping(world::now())),
            );
        }
        #[cfg(not(windows))]
        {
            let _ = (path, version, subject, identity, resources, behaviour);
            match self.0 {}
        }
    }
}
