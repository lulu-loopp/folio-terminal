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
        #[cfg(windows)]
        {
            world::program_at(path, version, resources);
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
            let _ = (path, version, subject, identity, resources);
            match self.0 {}
        }
    }
}
