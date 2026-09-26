//! **The rescue clone of a macOS bundle** (0.4.6 ticket U-26;
//! `docs/plans/design/self-update-2026-09-16.md` revision (b), F-3, F-8 and
//! experiment E-11).
//!
//! The rescue build that applies an update and recovers from an interrupted
//! one is a copy of the **old** bundle, kept at `H/<txn>/rescue/<Bundle>.app`
//! in the installation home beside the bundle: after the exchange the old
//! bundle's own path holds the new build, so whatever finishes or undoes the
//! transaction has to run from somewhere the exchange does not touch.
//! [`rescue_clone`] makes that copy and proves it is the same signed code:
//!
//! 1. the old bundle's identity is read with `codesign --display` — its
//!    designated requirement and its code-directory hash (cdhash);
//! 2. the bundle is cloned with `clonefile(2)` (APFS: the copy shares its
//!    blocks and costs no space until one side changes). A file system that
//!    cannot clone (`ENOTSUP`, `EXDEV`) is copied with `/usr/bin/ditto`
//!    instead, within [`COPY_WITHIN`];
//! 3. the clone is verified with `codesign --verify --deep --strict` against
//!    the old bundle's designated requirement — the requirement the running
//!    code carries, since the running code *is* the old bundle — which checks
//!    every sealed file of the clone;
//! 4. the clone's cdhash must equal the old bundle's, so it is this build and
//!    not merely one the same team signed.
//!
//! The answer is the clone's main executable, as `codesign` names it: the
//! program the LaunchAgent entrance (`crate::launch_agent`) runs at login.
//!
//! **Children only through the quiet door, by absolute path**
//! (`crate::quiet_command`): `/usr/bin/codesign` and `/usr/bin/ditto` are
//! where the system keeps them, so nothing is looked up on `PATH`. Each one is
//! waited for within a deadline and ended by the pid this call started it
//! with if it overruns. **Off macOS the call is refused by name**
//! ([`Refusal::NotHere`]). Worker only: a clone and a verification read the
//! whole bundle.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::{Duration, Instant};

use crate::HostPlatform;
use crate::file_reads::{self, Lane};

/// The system's code-signing tool.
pub const CODESIGN: &str = "/usr/bin/codesign";

/// The system's bundle copier, for a file system that cannot clone.
pub const DITTO: &str = "/usr/bin/ditto";

/// How long one `codesign` run may take. A display is instant; a deep
/// verification reads every sealed file of a bundle of some tens of
/// megabytes.
pub const SIGNING_WITHIN: Duration = Duration::from_secs(60);

/// How long a `ditto` copy of the bundle may take.
pub const COPY_WITHIN: Duration = Duration::from_secs(120);

/// How often a child that has not finished is asked again.
const POLL: Duration = Duration::from_millis(20);

/// **Why no verified rescue clone was made.**
#[derive(Debug)]
pub enum Refusal {
    /// This platform has no bundles to clone.
    NotHere,
    /// Something already stands where the clone would go.
    Exists(PathBuf),
    /// `clonefile` failed for a reason other than "this file system cannot".
    Clone(io::Error),
    /// A child did not start, did not finish within its deadline, or said no;
    /// `detail` is what it said, or why it was ended.
    Program {
        program: &'static str,
        detail: String,
    },
    /// `codesign` did not name the identity this call reads: the bundle is not
    /// signed, or its answer had no designated requirement, cdhash or
    /// executable.
    NoIdentity { bundle: PathBuf, detail: String },
    /// The clone verified, but it is not the same code as the old bundle.
    Differs { old: String, clone: String },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotHere => write!(f, "macos_update: no bundle to clone on this platform"),
            Self::Exists(path) => {
                write!(f, "macos_update: {} already exists", path.display())
            }
            Self::Clone(error) => write!(f, "macos_update clonefile: {error}"),
            Self::Program { program, detail } => {
                write!(f, "macos_update {program}: {}", detail.trim())
            }
            Self::NoIdentity { bundle, detail } => write!(
                f,
                "macos_update: no code identity for {}: {}",
                bundle.display(),
                detail.trim()
            ),
            Self::Differs { old, clone } => write!(
                f,
                "macos_update: the clone's cdhash {clone} is not the old bundle's {old}"
            ),
        }
    }
}

impl std::error::Error for Refusal {}

/// **What `codesign --display --verbose=3 -r-` says of a bundle**: the three
/// facts [`rescue_clone`] reads.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    /// The designated requirement's text, as `-R=` takes it back.
    designated: String,
    /// The code-directory hash, lowercase hex.
    cdhash: String,
    /// The main executable's path.
    executable: PathBuf,
}

/// Reads `codesign --display --verbose=3 -r-`'s answer (its standard output
/// and standard error together). The designated requirement is written
/// `designated => …`, or `# designated => …` when it is the implicit one.
fn identity(said: &str) -> Option<Identity> {
    let mut designated = None;
    let mut cdhash = None;
    let mut executable = None;
    for line in said.lines() {
        let line = line.trim_end();
        if let Some(text) = line
            .strip_prefix("# designated => ")
            .or_else(|| line.strip_prefix("designated => "))
        {
            designated = Some(text.to_owned());
        } else if let Some(hash) = line.strip_prefix("CDHash=") {
            cdhash = Some(hash.to_ascii_lowercase());
        } else if let Some(path) = line.strip_prefix("Executable=") {
            executable = Some(PathBuf::from(path));
        }
    }
    Some(Identity {
        designated: designated.filter(|text| !text.is_empty())?,
        cdhash: cdhash.filter(|hash| !hash.is_empty())?,
        executable: executable?,
    })
}

/// **Run `program` with `arguments` and wait for it within `within`**, ending
/// it by the pid this call started if it overruns. The output is charged to
/// `file_reads`' `Lane::Update`.
fn run(program: &'static str, arguments: &[&OsStr], within: Duration) -> Result<Output, Refusal> {
    let refused = |detail: String| Refusal::Program { program, detail };
    let mut child = crate::quiet_command(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| refused(format!("did not start: {error}")))?;
    let deadline = Instant::now() + within;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
            Ok(None) => {
                // Only the child this call started is ended.
                let _ = child.kill();
                let _ = child.wait();
                return Err(refused(format!("did not finish within {within:?}")));
            }
            Err(error) => return Err(refused(format!("could not be waited for: {error}"))),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| refused(format!("could not be read: {error}")))?;
    file_reads::pipe_output(Lane::Update, &output);
    Ok(output)
}

/// The text of a finished child's two streams, standard output first.
fn said(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

/// **The identity of the signed bundle at `bundle`.**
fn identity_of(bundle: &Path) -> Result<Identity, Refusal> {
    let output = run(
        CODESIGN,
        &[
            OsStr::new("--display"),
            OsStr::new("--verbose=3"),
            OsStr::new("-r-"),
            bundle.as_os_str(),
        ],
        SIGNING_WITHIN,
    )?;
    let text = said(&output);
    if !output.status.success() {
        return Err(Refusal::NoIdentity {
            bundle: bundle.to_path_buf(),
            detail: text,
        });
    }
    identity(&text).ok_or_else(|| Refusal::NoIdentity {
        bundle: bundle.to_path_buf(),
        detail: text,
    })
}

/// **Clone `from` to `to`**, or copy it with `ditto` where the file system
/// cannot clone.
fn clone_bundle(from: &Path, to: &Path) -> Result<(), Refusal> {
    match clone_tree(from, to) {
        Ok(()) => Ok(()),
        Err(error) if cannot_clone_here(&error) => {
            let output = run(DITTO, &[from.as_os_str(), to.as_os_str()], COPY_WITHIN)?;
            if output.status.success() {
                Ok(())
            } else {
                Err(Refusal::Program {
                    program: DITTO,
                    detail: said(&output),
                })
            }
        }
        Err(error) => Err(Refusal::Clone(error)),
    }
}

/// `<sys/clonefile.h>`'s `CLONE_NOFOLLOW`: a link at the source is cloned
/// as a link. The `libc` crate declares `clonefile` but not its flags.
#[cfg(target_os = "macos")]
const CLONE_NOFOLLOW: u32 = 0x0001;

/// `<sys/clonefile.h>`'s `CLONE_NOOWNERCOPY`: the clone is owned by the
/// caller, as a copy would be.
#[cfg(target_os = "macos")]
const CLONE_NOOWNERCOPY: u32 = 0x0002;

/// `clonefile(2)` with `CLONE_NOFOLLOW` (a link at `from` is cloned as a
/// link, never followed) and `CLONE_NOOWNERCOPY` (the clone is the caller's,
/// as a copy would be). A directory is cloned with everything in it.
#[cfg(target_os = "macos")]
fn clone_tree(from: &Path, to: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c_path = |path: &Path| {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in a path"))
    };
    let (from, to) = (c_path(from)?, c_path(to)?);
    // SAFETY: both strings are NUL-terminated and live across the call.
    let cloned = unsafe {
        libc::clonefile(
            from.as_ptr(),
            to.as_ptr(),
            CLONE_NOFOLLOW | CLONE_NOOWNERCOPY,
        )
    };
    if cloned == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Off macOS there is no `clonefile`; [`rescue_clone`] refuses before it
/// would be asked.
#[cfg(not(target_os = "macos"))]
fn clone_tree(_from: &Path, _to: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "macos_update has no clonefile on this platform",
    ))
}

/// Whether `clonefile` said "not on this file system" (`ENOTSUP`) or "not
/// across volumes" (`EXDEV`) — the two answers `ditto` stands in for.
fn cannot_clone_here(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::CrossesDevices || is_not_supported(error)
}

#[cfg(unix)]
fn is_not_supported(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(code) if code == libc::ENOTSUP || code == libc::EOPNOTSUPP)
}

#[cfg(not(unix))]
fn is_not_supported(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::Unsupported
}

/// **Clone the old bundle to `clone` and prove the clone is the same signed
/// code**; the answer is the clone's main executable.
///
/// `clone` is the rescue bundle's path in the installation home
/// (`update_txn::Home::rescue_bundle` in `bt-app`); its parent must exist and
/// nothing may stand at `clone` itself. A clone that fails its verification is
/// removed again before the refusal is returned, so a retry starts from
/// nothing.
///
/// # Errors
/// [`Refusal::NotHere`] off macOS; otherwise the step that refused.
pub fn rescue_clone(old_bundle: &Path, clone: &Path) -> Result<PathBuf, Refusal> {
    if crate::host_platform() != HostPlatform::MacOs {
        return Err(Refusal::NotHere);
    }
    if std::fs::symlink_metadata(clone).is_ok() {
        return Err(Refusal::Exists(clone.to_path_buf()));
    }
    let old = identity_of(old_bundle)?;
    clone_bundle(old_bundle, clone)?;
    let verified = verify_clone(&old, clone);
    if verified.is_err() {
        let _ = std::fs::remove_dir_all(clone);
    }
    verified
}

/// Steps 3 and 4: the clone's seal against the old bundle's designated
/// requirement, then its cdhash against the old one's.
fn verify_clone(old: &Identity, clone: &Path) -> Result<PathBuf, Refusal> {
    let mut requirement = OsString::from("-R=");
    requirement.push(&old.designated);
    let output = run(
        CODESIGN,
        &[
            OsStr::new("--verify"),
            OsStr::new("--deep"),
            OsStr::new("--strict"),
            &requirement,
            clone.as_os_str(),
        ],
        SIGNING_WITHIN,
    )?;
    if !output.status.success() {
        return Err(Refusal::Program {
            program: CODESIGN,
            detail: said(&output),
        });
    }
    let cloned = identity_of(clone)?;
    if cloned.cdhash != old.cdhash {
        return Err(Refusal::Differs {
            old: old.cdhash.clone(),
            clone: cloned.cdhash,
        });
    }
    // `codesign` names the executable by the clone's real path (`/var` is
    // `/private/var`); the answer is given in the caller's own spelling.
    let real = std::fs::canonicalize(clone).map_err(|error| Refusal::NoIdentity {
        bundle: clone.to_path_buf(),
        detail: error.to_string(),
    })?;
    match cloned.executable.strip_prefix(&real) {
        Ok(inside) => Ok(clone.join(inside)),
        Err(_) => Err(Refusal::NoIdentity {
            bundle: clone.to_path_buf(),
            detail: format!(
                "its executable {} is outside it",
                cloned.executable.display()
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `codesign --display --verbose=3 -r-` printed for an ad-hoc signed
    /// universal bundle on macOS 26 (paths made up, hashes kept in shape).
    const AD_HOC: &str = "Executable=/tmp/x/T.app/Contents/MacOS/t\n\
        Identifier=test.u26\n\
        Format=bundle with Mach-O universal (x86_64 arm64e)\n\
        CodeDirectory v=20400 size=257 flags=0x2(adhoc) hashes=2+3 location=embedded\n\
        # designated => cdhash H\"618d5e3368be265abd51472976742fa902819c8e\" or cdhash H\"1452da50f42f5cbc30828b02498d29a6e4721d4c\"\n\
        CDHash=618d5e3368be265abd51472976742fa902819c8e\n\
        Signature=adhoc\n";

    /// RED (U-26) — **the identity is read from `codesign`'s own words: the
    /// designated requirement (explicit or implicit), the cdhash and the
    /// executable; an answer missing any of them is no identity.**
    ///
    /// MUTATION: in `identity`, accept only `designated => ` (drop the `# `
    /// form).
    #[test]
    fn the_identity_is_read_from_codesigns_display() {
        assert_eq!(
            identity(AD_HOC),
            Some(Identity {
                designated: String::from(
                    "cdhash H\"618d5e3368be265abd51472976742fa902819c8e\" or cdhash H\"1452da50f42f5cbc30828b02498d29a6e4721d4c\""
                ),
                cdhash: String::from("618d5e3368be265abd51472976742fa902819c8e"),
                executable: PathBuf::from("/tmp/x/T.app/Contents/MacOS/t"),
            })
        );
        let explicit = AD_HOC.replace(
            "# designated => cdhash",
            "designated => identifier \"x\" and cdhash",
        );
        assert!(
            identity(&explicit)
                .unwrap()
                .designated
                .starts_with("identifier")
        );
        for missing in ["# designated", "CDHash=", "Executable="] {
            let text: String = AD_HOC
                .lines()
                .filter(|line| !line.starts_with(missing))
                .map(|line| format!("{line}\n"))
                .collect();
            assert_eq!(identity(&text), None, "without {missing}");
        }
        assert_eq!(identity("T.app: code object is not signed at all\n"), None);
    }

    /// RED (U-26) — **only "cannot clone here" falls back to `ditto`; any
    /// other `clonefile` failure is a refusal.**
    ///
    /// MUTATION: `cannot_clone_here` answers `true` for every error.
    #[test]
    fn only_a_file_system_that_cannot_clone_falls_back_to_a_copy() {
        assert!(cannot_clone_here(&io::Error::from(
            io::ErrorKind::CrossesDevices
        )));
        assert!(!cannot_clone_here(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
        assert!(!cannot_clone_here(&io::Error::from(
            io::ErrorKind::AlreadyExists
        )));
    }

    /// RED (U-26) — **off macOS the rescue clone is refused by name.**
    ///
    /// MUTATION: drop the platform check at the top of `rescue_clone`.
    #[test]
    fn off_macos_the_rescue_clone_is_refused_by_name() {
        if crate::host_platform() == HostPlatform::MacOs {
            return;
        }
        let refused = rescue_clone(Path::new("Folio.app"), Path::new("clone/Folio.app"));
        assert!(matches!(refused, Err(Refusal::NotHere)), "{refused:?}");
        assert!(refused.unwrap_err().to_string().starts_with("macos_update"));
    }

    /// The macOS half: a tiny synthetic bundle, built and ad-hoc signed by the
    /// test in a temporary folder. No installed Folio is read or touched.
    #[cfg(target_os = "macos")]
    mod on_apfs {
        use super::*;

        fn scratch(tag: &str) -> PathBuf {
            let path =
                std::env::temp_dir().join(format!("bt-macos-update-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            path
        }

        const INFO: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
            <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
            <plist version=\"1.0\"><dict>\
            <key>CFBundleExecutable</key><string>tiny</string>\
            <key>CFBundleIdentifier</key><string>io.github.lulu-loopp.folio.u26-test</string>\
            <key>CFBundleShortVersionString</key><string>0.0.1</string>\
            </dict></plist>\n";

        /// A bundle whose executable is a copy of `/usr/bin/true` (a real
        /// Mach-O, so it can be signed), with one resource, ad-hoc signed.
        fn tiny_bundle(root: &Path, name: &str) -> PathBuf {
            let bundle = root.join(name);
            std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
            std::fs::create_dir_all(bundle.join("Contents/Resources")).unwrap();
            std::fs::copy("/usr/bin/true", bundle.join("Contents/MacOS/tiny")).unwrap();
            std::fs::write(bundle.join("Contents/Info.plist"), INFO).unwrap();
            std::fs::write(
                bundle.join("Contents/Resources/notice.txt"),
                b"a sealed file",
            )
            .unwrap();
            sign(&bundle);
            bundle
        }

        fn sign(bundle: &Path) {
            let output = crate::quiet_command(CODESIGN)
                .args([OsStr::new("--force"), OsStr::new("--sign"), OsStr::new("-")])
                .arg(bundle)
                .output()
                .unwrap();
            assert!(output.status.success(), "{}", said(&output));
        }

        /// RED (U-26) — **a rescue clone on APFS is a bundle whose main
        /// executable is byte-identical to the old one's, whose seal verifies
        /// against the old bundle's designated requirement, and whose cdhash
        /// is the old one's**; the answer is that executable, inside the
        /// clone.
        ///
        /// E-11's first half. The second (the clone runs headless after the
        /// original is swapped) needs a real Folio and is the experiment's.
        ///
        /// MUTATION: in `rescue_clone`, answer the old bundle's executable
        /// (`Ok(old.executable)`) instead of verifying the clone.
        #[test]
        fn rescue_clone_on_apfs_is_the_same_signed_code() {
            let root = scratch("clone");
            let old = tiny_bundle(&root, "Folio.app");
            std::fs::create_dir_all(root.join("home/txn/rescue")).unwrap();
            let clone = root.join("home/txn/rescue/Folio.app");
            let executable = rescue_clone(&old, &clone).unwrap();
            assert_eq!(executable, clone.join("Contents/MacOS/tiny"));
            assert_eq!(
                std::fs::read(&executable).unwrap(),
                std::fs::read(old.join("Contents/MacOS/tiny")).unwrap(),
                "the clone's main executable is byte-identical"
            );
            assert_eq!(
                std::fs::read(clone.join("Contents/Resources/notice.txt")).unwrap(),
                b"a sealed file"
            );
            let refused = rescue_clone(&old, &clone);
            assert!(matches!(refused, Err(Refusal::Exists(_))), "{refused:?}");
            let _ = std::fs::remove_dir_all(&root);
        }

        /// RED (U-26) — **a bundle that is another build fails the clone's
        /// check**, even though the same identity signed it: the rescue must
        /// be the running build itself. And an unsigned bundle is never
        /// cloned: it has no identity to hold a clone to.
        ///
        /// A second bundle, signed the same way but with a different
        /// resource, stands in for the clone step producing the wrong code;
        /// its seal is valid, so only the requirement and the cdhash can tell.
        ///
        /// MUTATION: drop the cdhash comparison in `verify_clone` (and pass
        /// no `-R=`).
        #[test]
        fn a_bundle_that_is_not_the_old_build_fails_the_clone_check() {
            let root = scratch("differs");
            let old = tiny_bundle(&root, "Folio.app");
            let other = tiny_bundle(&root, "Other.app");
            std::fs::write(
                other.join("Contents/Resources/notice.txt"),
                b"another build",
            )
            .unwrap();
            sign(&other);
            let old_identity = identity_of(&old).unwrap();
            let refused = verify_clone(&old_identity, &other);
            assert!(
                matches!(
                    refused,
                    Err(Refusal::Program {
                        program: CODESIGN,
                        ..
                    } | Refusal::Differs { .. })
                ),
                "{refused:?}"
            );

            let unsigned = root.join("Unsigned.app");
            std::fs::create_dir_all(unsigned.join("Contents/MacOS")).unwrap();
            std::fs::copy("/usr/bin/true", unsigned.join("Contents/MacOS/tiny")).unwrap();
            std::fs::write(unsigned.join("Contents/Info.plist"), INFO).unwrap();
            let refused = rescue_clone(&unsigned, &root.join("clone.app"));
            assert!(
                matches!(
                    refused,
                    Err(Refusal::Program { .. } | Refusal::NoIdentity { .. })
                ),
                "{refused:?}"
            );
            assert!(
                !root.join("clone.app").exists(),
                "nothing is cloned from an unsigned bundle"
            );
            let _ = std::fs::remove_dir_all(&root);
        }
    }
}
