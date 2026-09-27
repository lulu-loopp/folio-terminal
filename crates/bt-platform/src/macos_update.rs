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
//!
//! **The image mount** (0.4.6 ticket U-17; C7 step 2, revision (b) F-17 and
//! M1): [`attach`] mounts the downloaded image read-only with
//! `/usr/bin/hdiutil attach -nobrowse -readonly -noautoopen -mountrandom
//! <H>/<txn>/mnt`, bounded, and answers a [`Mount`] that only [`detach`]
//! consumes; [`with_image`] is the road that attaches, hands the mount point
//! to its body and detaches whatever the body answers. Because the mount
//! point lies under the installation home, [`mounts_under`] finds it from the
//! mount table (`getfsstat`) with no record at all, and [`detach_all_under`]
//! is M1's step before `H/<txn>` is deleted. The same door, absolute path and
//! bounded wait as the clone's children; worker only (`&WorkerCtx`).

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
    /// This platform has no bundles to clone and no disk images to mount.
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
    /// A child did not finish within its deadline and was ended by the pid
    /// this call started it with.
    TimedOut {
        program: &'static str,
        within: Duration,
    },
    /// The directory an image was to be mounted under could not be resolved
    /// (it must exist: `hdiutil -mountrandom` makes its mount point inside it).
    MountDir { path: PathBuf, error: io::Error },
    /// `hdiutil attach` succeeded, but not exactly one of the mount points it
    /// reported lies under `mount_dir`. An image already attached elsewhere is
    /// answered with its existing mount point, which is somebody else's and is
    /// never detached here.
    MountPoints {
        mount_dir: PathBuf,
        reported: Vec<PathBuf>,
    },
    /// The mount table could not be read.
    MountTable(io::Error),
    /// These mount points are still mounted: their detach was refused. They
    /// lie under the installation home, so the next sweep finds them again.
    LeftMounted(Vec<PathBuf>),
    /// An attach failed (`cause`), and detaching what it may have mounted
    /// under its mount directory failed too (`sweep`).
    Undetached {
        cause: Box<Refusal>,
        sweep: Box<Refusal>,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotHere => write!(
                f,
                "macos_update: no bundle to clone and no disk image to mount on this platform"
            ),
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
            Self::TimedOut { program, within } => {
                write!(
                    f,
                    "macos_update {program}: did not finish within {within:?}"
                )
            }
            Self::MountDir { path, error } => write!(
                f,
                "macos_update: no mount directory at {}: {error}",
                path.display()
            ),
            Self::MountPoints {
                mount_dir,
                reported,
            } => write!(
                f,
                "macos_update hdiutil: not exactly one of the mount points {reported:?} lies under {}",
                mount_dir.display()
            ),
            Self::MountTable(error) => write!(f, "macos_update: the mount table: {error}"),
            Self::LeftMounted(points) => {
                write!(
                    f,
                    "macos_update: still mounted after a refused detach: {points:?}"
                )
            }
            Self::Undetached { cause, sweep } => {
                write!(f, "{cause}; and what it may have mounted: {sweep}")
            }
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
    run_at(program, Path::new(program), arguments, within)
}

/// [`run`], for a program named `name` in refusals and found at `at` — the
/// system's place, or a test's stand-in.
fn run_at(
    name: &'static str,
    at: &Path,
    arguments: &[&OsStr],
    within: Duration,
) -> Result<Output, Refusal> {
    let refused = |detail: String| Refusal::Program {
        program: name,
        detail,
    };
    let mut child = crate::quiet_command(at)
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
                return Err(Refusal::TimedOut {
                    program: name,
                    within,
                });
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

// ───────────────────────── the image mount (U-17) ─────────────────────────
//
// C7 step 2 and revision (b) F-17: the downloaded image is attached with
// `hdiutil attach -nobrowse -readonly -noautoopen -mountrandom <H>/<txn>/mnt`,
// so its mount point lies under the installation home. **A mount under the
// home is ours even when the attach result was never recorded**: the mount
// table lists it, and M1's next actor detaches it from there before it deletes
// `H/<txn>`.

/// The system's disk-image tool.
pub const HDIUTIL: &str = "/usr/bin/hdiutil";

/// How long one `hdiutil attach` may take. It checksums every block of a
/// compressed image before it mounts it, which for the release image (some
/// tens of megabytes) takes seconds, not minutes.
pub const ATTACH_WITHIN: Duration = Duration::from_secs(120);

/// How long one `hdiutil detach` may take.
pub const DETACH_WITHIN: Duration = Duration::from_secs(30);

/// How long a detach refused as busy waits before its one `-force` retry.
pub const BUSY_RETRY_AFTER: Duration = Duration::from_secs(2);

/// `hdiutil detach`'s exit status when a file on the volume is still open
/// (`EBUSY`; the message beside it is in the system's language).
const BUSY: i32 = 16;

/// **An image attached by [`attach`], and the one thing that detaches it.**
///
/// No `Clone`, no public constructor, and **no `Drop`** (A1e's closed `Drop`
/// inventory: a detach waits on a child, and no new `Drop` may wait). It is
/// consumed by [`detach`] on every road; [`with_image`] is the road that does
/// so by construction, and `macos_update::mount_tests` drives each of its exit
/// roads with a stand-in `hdiutil` that records every attach and detach. A
/// `Mount` lost anyway (a panic) leaves its mount point under the home, where
/// [`mounts_under`] finds it.
#[must_use = "an attached image is detached by `detach`, on every road"]
#[derive(Debug, PartialEq, Eq)]
pub struct Mount {
    /// The mount point, as `hdiutil` reported it.
    point: PathBuf,
}

impl Mount {
    /// Where the image's volume is mounted.
    #[must_use]
    pub fn point(&self) -> &Path {
        &self.point
    }
}

/// **What [`with_image`] did**: the body's answer or why the image never
/// reached it, and whether the image came off again.
#[must_use = "an image's use says both what the body answered and whether the image came off again"]
#[derive(Debug)]
pub struct Used<T, E> {
    /// What the body answered, or the attach's refusal.
    pub outcome: Result<T, Failed<E>>,
    /// Whether the image was detached afterwards. `Ok` when the attach itself
    /// failed (it detaches what it may have mounted before it answers). A
    /// refusal here is debt (R-11), never a reason to undo the body's work:
    /// the mount point lies under the mount directory, so the next sweep of the
    /// home finds it in the mount table.
    pub detached: Result<(), Refusal>,
}

/// Why [`with_image`]'s body gave no answer.
#[derive(Debug)]
pub enum Failed<E> {
    /// The image was not attached (anything it may have mounted is detached).
    Attach(Refusal),
    /// The body refused (the image is detached afterwards all the same).
    Body(E),
}

/// The deadlines one mount road runs under.
#[derive(Clone, Copy, Debug)]
struct Within {
    attach: Duration,
    detach: Duration,
    busy: Duration,
}

/// The system's deadlines.
const SYSTEM_WITHIN: Within = Within {
    attach: ATTACH_WITHIN,
    detach: DETACH_WITHIN,
    busy: BUSY_RETRY_AFTER,
};

/// Where the mount road finds its tool and its mount table: the system's, or
/// a test's stand-ins.
struct Tools<'a> {
    hdiutil: &'a Path,
    table: &'a dyn Fn(&Path) -> Result<Vec<PathBuf>, Refusal>,
    within: Within,
}

/// The system's tools.
fn system() -> Tools<'static> {
    Tools {
        hdiutil: Path::new(HDIUTIL),
        table: &points_under,
        within: SYSTEM_WITHIN,
    }
}

/// **Attach `image` read-only, mounted at a fresh directory inside
/// `mount_dir`** (`update_txn::Home::mount_point(txn)` = `H/<txn>/mnt`), with
/// `hdiutil attach -nobrowse -readonly -noautoopen -mountrandom <mount_dir>
/// <image>`, both paths resolved to absolute ones first, within
/// [`ATTACH_WITHIN`].
///
/// `mount_dir` must exist (`hdiutil` refuses a missing one). The answer is
/// the one mount point `hdiutil` reported under `mount_dir`. **Every refusal
/// after the child started first detaches whatever is mounted under
/// `mount_dir`, found in the mount table** — `hdiutil` that failed, overran
/// its deadline (it is ended by its pid, and may have mounted before it was),
/// or said something this does not read — so an attach that is refused leaves
/// nothing mounted there; a detach that is refused too is named in
/// [`Refusal::Undetached`].
///
/// A worker's call (`&WorkerCtx`): it waits on a child.
///
/// # Errors
/// [`Refusal::NotHere`] off macOS; otherwise the step that refused.
pub fn attach(
    _worker: &crate::admission::WorkerCtx,
    image: &Path,
    mount_dir: &Path,
) -> Result<Mount, Refusal> {
    if crate::host_platform() != HostPlatform::MacOs {
        return Err(Refusal::NotHere);
    }
    attach_with(&system(), image, mount_dir)
}

/// **Detach `mount`** with `hdiutil detach <mount point>`, within
/// [`DETACH_WITHIN`]; refused as busy (exit 16), it waits
/// [`BUSY_RETRY_AFTER`] and tries once more with `-force`. `hdiutil` detaches
/// the whole image a mount point belongs to.
///
/// # Errors
/// [`Refusal::NotHere`] off macOS; otherwise the detach's refusal — debt
/// (R-11): the mount point is still under the home, where [`mounts_under`]
/// finds it.
pub fn detach(_worker: &crate::admission::WorkerCtx, mount: Mount) -> Result<(), Refusal> {
    if crate::host_platform() != HostPlatform::MacOs {
        return Err(Refusal::NotHere);
    }
    detach_with(&system(), mount)
}

/// **Every mount point strictly below `root`**, from the mount table
/// (`getfsstat(2)`, the table `getmntinfo(3)` reads, with `MNT_NOWAIT` so a
/// file system that does not answer cannot hold the call), compared with
/// `root`'s real path. A `root` that does not exist has nothing under it.
///
/// No record is needed: M1's next actor finds the mount an attach left even
/// when the attach's answer was never written down.
///
/// # Errors
/// [`Refusal::NotHere`] off macOS; [`Refusal::MountTable`] when the table or
/// `root` cannot be read.
pub fn mounts_under(root: &Path) -> Result<Vec<PathBuf>, Refusal> {
    if crate::host_platform() != HostPlatform::MacOs {
        return Err(Refusal::NotHere);
    }
    points_under(root)
}

/// **Detach every mount below `root`** — M1's step before `H/<txn>` is
/// deleted: each mount point [`mounts_under`] lists, detached as [`detach`]
/// does.
///
/// # Errors
/// [`Refusal::NotHere`] off macOS; [`Refusal::MountTable`]; or
/// [`Refusal::LeftMounted`] naming every mount point whose detach was refused
/// (the others are detached all the same).
pub fn detach_all_under(_worker: &crate::admission::WorkerCtx, root: &Path) -> Result<(), Refusal> {
    if crate::host_platform() != HostPlatform::MacOs {
        return Err(Refusal::NotHere);
    }
    detach_all_with(&system(), root)
}

/// **Attach `image` under `mount_dir`, hand its mount point to `body`, and
/// detach it again — whatever `body` answers.** The road the macOS Prepare
/// (U-27) takes for C7 step 3: verify the mounted bundle, copy it out, verify
/// the copy. The [`Mount`] never leaves this function.
pub fn with_image<T, E>(
    _worker: &crate::admission::WorkerCtx,
    image: &Path,
    mount_dir: &Path,
    body: impl FnOnce(&Path) -> Result<T, E>,
) -> Used<T, E> {
    if crate::host_platform() != HostPlatform::MacOs {
        return Used {
            outcome: Err(Failed::Attach(Refusal::NotHere)),
            detached: Ok(()),
        };
    }
    with_image_with(&system(), image, mount_dir, body)
}

fn with_image_with<T, E>(
    tools: &Tools<'_>,
    image: &Path,
    mount_dir: &Path,
    body: impl FnOnce(&Path) -> Result<T, E>,
) -> Used<T, E> {
    let mount = match attach_with(tools, image, mount_dir) {
        Ok(mount) => mount,
        Err(refusal) => {
            return Used {
                outcome: Err(Failed::Attach(refusal)),
                detached: Ok(()),
            };
        }
    };
    let outcome = body(mount.point()).map_err(Failed::Body);
    Used {
        outcome,
        detached: detach_with(tools, mount),
    }
}

/// **The mount points in `hdiutil attach`'s answer** (its standard output,
/// without `-plist`). Measured on macOS 26: before the table come the
/// checksum lines, in the system's language; the table has one line per
/// device, `<device><spaces>\t<content hint><spaces>\t<mount point>`, and the
/// third column is empty for a device with no file system mounted. So a line
/// that begins `/dev/` and has a third tab-separated column beginning `/`
/// names a mount point: that column to the end of the line, unpadded (a tab
/// or a space inside the path is kept). An APFS image lists a synthesized
/// container disk as well; only its volume has a mount point.
fn mount_points(said: &str) -> Vec<PathBuf> {
    said.lines()
        .filter(|line| line.starts_with("/dev/"))
        .filter_map(|line| line.splitn(3, '\t').nth(2))
        .filter(|point| point.starts_with('/'))
        .map(PathBuf::from)
        .collect()
}

/// Whether `point` is strictly below `root` (both real paths).
fn is_below(point: &Path, root: &Path) -> bool {
    point != root && point.starts_with(root)
}

fn attach_with(tools: &Tools<'_>, image: &Path, mount_dir: &Path) -> Result<Mount, Refusal> {
    let dir = std::fs::canonicalize(mount_dir).map_err(|error| Refusal::MountDir {
        path: mount_dir.to_path_buf(),
        error,
    })?;
    let image = std::fs::canonicalize(image).map_err(|error| Refusal::Program {
        program: HDIUTIL,
        detail: format!("no image at {}: {error}", image.display()),
    })?;
    let answered = run_at(
        HDIUTIL,
        tools.hdiutil,
        &[
            OsStr::new("attach"),
            OsStr::new("-nobrowse"),
            OsStr::new("-readonly"),
            OsStr::new("-noautoopen"),
            OsStr::new("-mountrandom"),
            dir.as_os_str(),
            image.as_os_str(),
        ],
        tools.within.attach,
    );
    // Every refusal from here on first detaches what is mounted under `dir`.
    let refused = |cause: Refusal| match detach_all_with(tools, &dir) {
        Ok(()) => cause,
        Err(sweep) => Refusal::Undetached {
            cause: Box::new(cause),
            sweep: Box::new(sweep),
        },
    };
    let output = answered.map_err(refused)?;
    if !output.status.success() {
        return Err(refused(Refusal::Program {
            program: HDIUTIL,
            detail: said(&output),
        }));
    }
    let reported = mount_points(&String::from_utf8_lossy(&output.stdout));
    let ours: Vec<&PathBuf> = reported
        .iter()
        .filter(|point| std::fs::canonicalize(point).is_ok_and(|real| is_below(&real, &dir)))
        .collect();
    if let [point] = ours.as_slice() {
        return Ok(Mount {
            point: PathBuf::clone(point),
        });
    }
    Err(refused(Refusal::MountPoints {
        mount_dir: dir.clone(),
        reported,
    }))
}

fn detach_with(tools: &Tools<'_>, mount: Mount) -> Result<(), Refusal> {
    detach_point(tools, &mount.point)
}

fn detach_point(tools: &Tools<'_>, point: &Path) -> Result<(), Refusal> {
    let detach = |force: bool| {
        let mut arguments = vec![OsStr::new("detach")];
        if force {
            arguments.push(OsStr::new("-force"));
        }
        arguments.push(point.as_os_str());
        run_at(HDIUTIL, tools.hdiutil, &arguments, tools.within.detach)
    };
    let mut output = detach(false)?;
    if output.status.code() == Some(BUSY) {
        std::thread::sleep(tools.within.busy);
        output = detach(true)?;
    }
    if output.status.success() {
        Ok(())
    } else {
        Err(Refusal::Program {
            program: HDIUTIL,
            detail: said(&output),
        })
    }
}

fn detach_all_with(tools: &Tools<'_>, root: &Path) -> Result<(), Refusal> {
    let left: Vec<PathBuf> = (tools.table)(root)?
        .into_iter()
        .filter(|point| detach_point(tools, point).is_err())
        .collect();
    if left.is_empty() {
        Ok(())
    } else {
        Err(Refusal::LeftMounted(left))
    }
}

/// [`mounts_under`] without the platform check: the table's mount points
/// strictly below `root`'s real path.
fn points_under(root: &Path) -> Result<Vec<PathBuf>, Refusal> {
    let root = match std::fs::canonicalize(root) {
        Ok(root) => root,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Refusal::MountTable(error)),
    };
    Ok(mounted()
        .map_err(Refusal::MountTable)?
        .into_iter()
        .filter(|point| is_below(point, &root))
        .collect())
}

/// **Every mount point in the mount table**, from `getfsstat(2)` into a
/// buffer this call owns (`getmntinfo(3)` reads the same table into one it
/// keeps between calls). A table that grew between the count and the read is
/// read again.
#[cfg(target_os = "macos")]
fn mounted() -> io::Result<Vec<PathBuf>> {
    use std::os::unix::ffi::OsStrExt;
    loop {
        // SAFETY: a null buffer of size 0 asks only for the number of mounts;
        // nothing is written.
        let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
        let count = usize::try_from(count).map_err(|_| io::Error::last_os_error())?;
        let room = count + 4;
        let bytes = libc::c_int::try_from(room * std::mem::size_of::<libc::statfs>())
            .map_err(|_| io::Error::other("the mount table does not fit a buffer size"))?;
        let mut table: Vec<libc::statfs> = Vec::with_capacity(room);
        // SAFETY: `table` has room for `room` records and `bytes` is exactly
        // that many records' size; the call writes at most that and answers
        // how many it wrote.
        let filled = unsafe { libc::getfsstat(table.as_mut_ptr(), bytes, libc::MNT_NOWAIT) };
        let filled = usize::try_from(filled).map_err(|_| io::Error::last_os_error())?;
        if filled >= room {
            continue;
        }
        // SAFETY: the call initialised the first `filled` records, and
        // `filled` is below the capacity `room`.
        unsafe { table.set_len(filled) };
        return Ok(table
            .iter()
            .map(|record| {
                let name: Vec<u8> = record
                    .f_mntonname
                    .iter()
                    .map(|&c| c as u8)
                    .take_while(|&byte| byte != 0)
                    .collect();
                PathBuf::from(OsStr::from_bytes(&name))
            })
            .collect());
    }
}

/// Off macOS [`mounts_under`] refuses before it would be asked.
#[cfg(not(target_os = "macos"))]
fn mounted() -> io::Result<Vec<PathBuf>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "macos_update has no mount table on this platform",
    ))
}

#[cfg(test)]
#[path = "macos_update_mount_tests.rs"]
mod mount_tests;

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
