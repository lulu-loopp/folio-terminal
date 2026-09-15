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
//! # Why a kernel object and not a file somebody has to judge
//!
//! A lock file **read as a file** is a question nobody can answer. A file left
//! behind by a process that was killed is indistinguishable from one a live
//! process is holding, so the reader has to be given a staleness rule — and
//! there is no honest one: a Folio open for a week is not stale, and a Folio
//! killed a second ago is. The `update` module's own claim file gets away with a
//! five-minute rule because what it guards is one HTTP request; a claim on a
//! whole data directory is held for the life of a window.
//!
//! A **kernel-owned** claim has no such question in it. The kernel knows which
//! claims are held, it drops every one of them when the process that took them
//! ends — killed, crashed, or quit — and "is somebody else running" is answered
//! by the same call that takes the claim, with no timestamp, no heartbeat and no
//! recovery path. Both arms below are that, and only the spelling differs:
//!
//! * **Windows: a named mutex.** The kernel owns the name, the name exists
//!   exactly while some handle to it is open, and every handle this process
//!   holds is closed when the process ends.
//! * **Unix: `flock(LOCK_EX | LOCK_NB)` on a file in a private runtime
//!   directory.** The lock is a property of the open file description rather
//!   than of the file's contents, so nothing in the file is ever read and the
//!   file being there means nothing at all. The kernel releases it when the last
//!   descriptor to that description is closed, which a killed process gets for
//!   free. **That is the whole of crash recovery for the claim** — what a crash
//!   does leave behind is the launch *socket*, and that is cleaned up below,
//!   under the lock (`docs/plans/port/macos-plan-2026-09-12.md` §R5).
//!
//! # The name
//!
//! **On Windows**, `Local\` — the session-local namespace — because two users
//! signed into one machine have two `%APPDATA%` directories and must not lock
//! each other out; and the directory's own spelling folded into the name,
//! because the claim is on a *directory* rather than on the product. A build run
//! with `APPDATA` pointed somewhere else (which is how this repository's own
//! tests and manual checks are run) claims a different name and does not collide
//! with the reader's everyday Folio.
//!
//! **On Unix** the isolation is not in the name but in the directory the name
//! lives in: `folio-<uid>/` under the per-user temporary directory the *system*
//! names — **not** `$TMPDIR`, which is an environment variable and therefore a
//! thing two processes of one user can disagree about (review RA-1, and
//! `runtime_directory` below carries the whole of that reason). It is created
//! `0700`, owner-checked and refused outright if anything but a real directory
//! of this user's is standing there.
//! And the fold is not the Windows one. `to_lowercase` on a path is correct on
//! NTFS and wrong on a case-sensitive APFS volume, and it says nothing at all
//! about symlinks — so the Unix arm asks the filesystem instead
//! (`canonical_path`), which answers case and symlinks and `..` in one, and
//! hashes *that*. **This is a name, not a secret**; what it has to do is not
//! collide between the handful of directories one machine has, and be short —
//! the same digest is the launch socket's file name, and `sockaddr_un` on macOS
//! has 104 bytes for a whole path.
//!
//! # What this is not, on either platform
//!
//! A defence against a hostile process running as you. On Windows the mutex name
//! is reachable by everything in the logon session; on Unix the runtime
//! directory is reachable by this **user**, which is a wider principal than a
//! logon session — a second `ssh` login, a launch agent, a second desktop are
//! all inside it, where on Windows they would be outside
//! (`docs/plans/port/macos-plan-2026-09-12.md` §R6). That difference is stated
//! rather than papered over: it is the correct boundary for this claim, because
//! the thing being guarded is one `$HOME`'s data directory and every session of
//! one user shares it.

use std::path::{Path, PathBuf};

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

/// **The digest every name for one directory is built out of.**
///
/// FNV-1a, and private to this module's arms rather than public: this is a name
/// and not a secret, and what it has to do is not collide between the handful of
/// directories one machine has, and be **short** — a Windows kernel object name
/// is capped at `MAX_PATH`, and a Unix socket path is capped at
/// [`SOCKET_PATH_LIMIT`] bytes for the whole path, digest and runtime directory
/// together.
fn digest(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// **One directory, reduced to the segment every name for it is built out of.**
///
/// Public because two names are derived from one directory — the claim below and
/// the launch endpoint in [`crate::launch_pipe`] — and the property that matters
/// is a property of *both*: a test window under an isolated data directory must
/// miss the reader's everyday Folio at every door, not at one of them. Two
/// spellings that claim one lock have to address one endpoint, and the only way
/// to guarantee that is for there to be one folding.
///
/// **The Windows folding is the Windows one and stays here** — case, and a
/// trailing separator — because that is what NTFS means by one path. It is not
/// what APFS means by one path, and this arm is gated rather than shared for
/// exactly that reason (`docs/plans/port/macos-plan-2026-09-12.md` §R5).
#[cfg(windows)]
#[must_use]
pub fn directory_tag(directory: &Path) -> String {
    let folded = directory
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase();
    digest(folded.as_bytes())
}

/// **The same segment where the filesystem, and not a case table, says what one
/// directory is.**
///
/// `to_lowercase` would be wrong twice over off Windows: a case-sensitive APFS
/// or ext4 volume has two directories where it sees one, and no amount of case
/// folding answers a symlink or a `..`. So this arm asks the filesystem —
/// [`canonical_path`] — and hashes the answer.
#[cfg(not(windows))]
#[must_use]
pub fn directory_tag(directory: &Path) -> String {
    digest(canonical_path(directory).as_os_str().as_encoded_bytes())
}

/// **One directory's canonical spelling**, as far as it exists.
///
/// `realpath` for the part of the path that is there, and the remaining
/// components appended as they were written. Two properties are wanted from it
/// and both are load-bearing:
///
/// * **It folds what the filesystem folds.** Symlinks, `..`, a trailing
///   separator, and — on a case-insensitive volume, where the kernel answers
///   with the on-disk spelling — case. That is the whole of §R5's complaint
///   against a lowercased path.
/// * **It does not change when the directory is created.** A name is asked for
///   before the data directory exists (a first run asks who the writer is before
///   anything has written), and the answer has to be the same one the next ask
///   gets. Canonicalising the longest *existing* prefix and appending the rest
///   gives that: the prefix does not move when a child of it is made, and the
///   appended components are the ones the caller wrote. An identity taken from
///   the directory's own inode would have answered one thing before `mkdir` and
///   another after, and the two asks would be two claims on one directory.
///
/// Not gated, because nothing in it names a platform: it is `std::path` and
/// `std::fs`, and the Windows arm above simply does not call it.
#[must_use]
pub fn canonical_path(directory: &Path) -> PathBuf {
    // Lexical, and deliberately so: this must not consult the filesystem before
    // the loop below decides how much of the path the filesystem knows about.
    let Ok(absolute) = std::path::absolute(directory) else {
        return directory.to_path_buf();
    };
    let mut trailing: Vec<std::ffi::OsString> = Vec::new();
    let mut probe = absolute.clone();
    loop {
        if let Ok(real) = std::fs::canonicalize(&probe) {
            let mut answer = real;
            for component in trailing.iter().rev() {
                answer.push(component);
            }
            return answer;
        }
        // A path with no file name is a root, or ends in `..` that `absolute`
        // could not remove. There is nothing left to step out of, so the lexical
        // answer is the whole answer.
        let (Some(name), Some(parent)) = (probe.file_name(), probe.parent()) else {
            return absolute;
        };
        trailing.push(name.to_owned());
        probe = parent.to_path_buf();
    }
}

/// **How many bytes a Unix socket path may be, terminator included.**
///
/// `sizeof(((struct sockaddr_un *)0)->sun_path)` is 104 on macOS and 108 on
/// Linux, and the smaller of the two is the one written down: a limit that is
/// right on one platform and generous on the other is a limit that fails on the
/// platform this port is for. Pinned rather than assumed, because the failure it
/// guards is quiet — a `sun_path` that does not fit is truncated by some
/// systems and refused by others, and a truncated endpoint is a door two
/// processes disagree about.
///
/// Nothing here is at risk of reaching it: [`socket_path_in`] puts a sixteen
/// character digest and a five character suffix under this user's runtime
/// directory — `folio-<uid>` under the per-user temporary directory the system
/// names — **whatever the data directory's own path length is**, and
/// [`attention_socket_path_in`] — the longer of the two, and therefore the one
/// the promise is really about — puts the same digest and a ten character
/// suffix there. That is the other half of why the name is a digest.
pub const SOCKET_PATH_LIMIT: usize = 104;

/// The launch endpoint's file name, inside a runtime directory.
///
/// Pure, and not gated, so that the length promise above is a claim a Windows
/// runner can check rather than one only a Mac could.
#[must_use]
pub fn socket_path_in(runtime: &Path, tag: &str) -> PathBuf {
    runtime.join(format!("{tag}.sock"))
}

/// **The attention endpoint's suffix**, and the whole of what tells the two
/// doors of one data directory apart (M4-7, `docs/DESIGN.md` §13.37).
///
/// A suffix on the same digest rather than a second digest: the two names have
/// to fold identically, because a run under an isolated data directory must
/// miss the reader's everyday Folio at *both* doors and two spellings of one
/// directory must find each other at both — which is §13.28 ②'s promise, now
/// made of three names instead of two.
pub const ATTENTION_SOCKET_SUFFIX: &str = ".attn.sock";

/// The attention endpoint's file name, inside a runtime directory.
///
/// Pure and not gated, for [`socket_path_in`]'s reason: the length promise
/// above is a claim a Windows runner can check rather than one only a Mac
/// could, and this is the longer of the two names that has to keep it.
#[must_use]
pub fn attention_socket_path_in(runtime: &Path, tag: &str) -> PathBuf {
    runtime.join(format!("{tag}{ATTENTION_SOCKET_SUFFIX}"))
}

/// **Whether a socket path and its terminator fit in `sun_path`.**
///
/// One function rather than the comparison written out at each of the three
/// places that ask — the server binding, the client addressing, and the pin in
/// this file's own tests — because the interesting half of it is the half that
/// is easiest to drop: `sun_path` holds a **C string**, so the terminator is
/// inside the field, and a path of exactly [`SOCKET_PATH_LIMIT`] bytes does not
/// fit in [`SOCKET_PATH_LIMIT`] bytes.
#[must_use]
pub fn fits_a_socket_path(path: &Path) -> bool {
    path.as_os_str().as_encoded_bytes().len() < SOCKET_PATH_LIMIT
}

/// The lock file's name, in the same directory and derived the same way.
///
/// A second file rather than a second use of the socket: `flock` on the socket
/// file would tie the claim's lifetime to the endpoint's, and the claim has to
/// outlive every endpoint it opens — it is the thing the endpoint is opened on
/// the strength of.
#[cfg(unix)]
#[must_use]
fn lock_path_in(runtime: &Path, tag: &str) -> PathBuf {
    runtime.join(format!("{tag}.lock"))
}

/// **Where this user's Folio runtime files live**, as a path and without
/// touching the disk.
///
/// # Why this is not `$TMPDIR` (review RA-1)
///
/// The claim on a data directory is a `flock` on a file in here, and both of
/// that directory's endpoints are files in here — so this path is half of what
/// "one data directory, one writer" means. Two Folios that compute two
/// different runtime directories take two different locks, **both** answer
/// `Some(DataDirectoryClaim)`, and both write `settings.json` and
/// `session.json` over each other; the second one cannot even find the first to
/// hand its command line over, because it is knocking on a door in the other
/// directory.
///
/// `$TMPDIR` is exactly that split, written into the product. It is an
/// environment variable: a Folio `launchd` started has it set to this user's
/// per-user directory, and a Folio started from an `ssh` session, from `env -i`
/// or by a daemon does not — which used to mean `/tmp`. Same user, same
/// `$HOME`, two runtime directories, two writers. The Windows arm cannot
/// diverge this way, because its claim is a kernel name derived from the data
/// directory alone.
///
/// # What is asked instead
///
/// The **system**, through [`per_user_temporary_directory`]: on macOS that is
/// `confstr(_CS_DARWIN_USER_TEMP_DIR)`, which is the directory `launchd` reads
/// `$TMPDIR` *out of*, so a process that inherited no environment at all gets
/// the same answer as one that inherited a full one. Nothing in a process's
/// environment can move it, which is the whole property this function needs.
///
/// The `<uid>` in the name stays, and it is what makes the `/tmp` answer as
/// private as the per-user one: two users on one machine must not meet in one
/// directory, and the one that got there first must not be able to decide what
/// the second one finds — which is [`prepare_runtime_directory`]'s three
/// refusals.
#[cfg(unix)]
#[must_use]
pub fn runtime_directory() -> PathBuf {
    // SAFETY: `geteuid` reads this process's own credentials and cannot fail.
    let uid = unsafe { libc::geteuid() };
    runtime_directory_from(per_user_temporary_directory(), uid)
}

/// **The same rule with its one impure input handed in.**
///
/// Pure and not gated, for [`socket_path_in`]'s reason: what it promises — that
/// one user's runtime directory is not another's, that a path that is not
/// rooted is refused rather than joined onto, and that what comes out of it
/// still leaves room for an endpoint inside `sun_path` — is a claim a Windows
/// runner can check rather than one only a Mac could.
///
/// Split out rather than inlined for `bt_app::attention_hooks::config_dir_from`'s
/// reason: a process-wide variable changed from a test is changed for every
/// other test running beside it, so the impure input is **named** instead of
/// being reached for. Here the named input is what the system answered, and the
/// finding this closes is that there used to be a second, unnamed one.
///
/// **`has_root` and not `is_absolute`**: on Unix the two are the same question,
/// and this function is also read on a Windows host, where `is_absolute` wants
/// a drive letter as well. A relative answer is refused rather than joined onto
/// a working directory — a runtime directory that moved with the directory a
/// Folio was started in would be the same split this function exists to close.
#[must_use]
pub fn runtime_directory_from(system_temporary_directory: Option<PathBuf>, uid: u32) -> PathBuf {
    system_temporary_directory
        .filter(|directory| directory.has_root())
        .unwrap_or_else(|| PathBuf::from(SHARED_TEMPORARY_DIRECTORY))
        .join(format!("folio-{uid}"))
}

/// **Where a runtime directory goes on a system that has no per-user temporary
/// directory to put it in.**
///
/// A constant and not a second environment read: `/tmp` is the same path in
/// every process on the machine, which is the property [`runtime_directory`] is
/// built out of, and it is what the `<uid>` in the name and
/// [`prepare_runtime_directory`]'s owner and symlink refusals are really for —
/// this is the one case where the directory's parent is shared with other
/// users.
const SHARED_TEMPORARY_DIRECTORY: &str = "/tmp";

/// **What macOS says this user's own temporary directory is**, asked of the
/// system rather than read out of the environment (RA-1).
///
/// `confstr(_CS_DARWIN_USER_TEMP_DIR)` answers `/var/folders/<xx>/<digest>/T/`
/// — per user and per boot, created by the system, owned by this user and
/// reachable by nobody else. It is the value `launchd` puts in `$TMPDIR`, and
/// asking for it is how a process that inherited no environment arrives at the
/// same directory as one that did. `confstr(3)` on macOS documents the
/// `_CS_DARWIN_USER_*` names as this user's own directories and is the whole
/// citation for that claim.
///
/// `None` rather than a guess when the system has none to give: the caller's
/// answer for that is a constant, and a second guess here would be a second way
/// for two processes of one user to disagree.
#[cfg(target_os = "macos")]
#[must_use]
fn per_user_temporary_directory() -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let name = libc::_CS_DARWIN_USER_TEMP_DIR;
    // SAFETY: a null destination with a zero length is how `confstr` is
    // specified to be asked for the room it needs; it writes nothing at all in
    // that call.
    let needed = unsafe { libc::confstr(name, std::ptr::null_mut(), 0) };
    if needed == 0 {
        return None;
    }
    let mut answer = vec![0_u8; needed];
    let room = answer.len();
    let into = answer.as_mut_ptr().cast::<libc::c_char>();
    // SAFETY: `into` is this vector's own buffer and `room` is that buffer's
    // length, so the call cannot write past it.
    let written = unsafe { libc::confstr(name, into, room) };
    // Zero is the failure. An answer that wanted more room than it was given
    // was truncated, and a truncated path is a *different* directory rather
    // than a shorter spelling of the same one — so it is refused too.
    if written == 0 || written > room {
        return None;
    }
    // The count includes the terminator, which is not part of the path.
    answer.truncate(written - 1);
    Some(PathBuf::from(OsString::from_vec(answer)))
}

/// **The same question on a Unix that is not macOS**, which has no per-user
/// temporary directory to ask about.
///
/// `XDG_RUNTIME_DIR` is the nearest thing and it is an environment variable,
/// which is the finding this answers rather than a way round it, and
/// `/run/user/<uid>` — what systemd sets it to — is not on every system. So the
/// answer is `None` and `/tmp/folio-<uid>` stands: one path, the same one in
/// every process of this user, which is the property that matters here.
#[cfg(all(unix, not(target_os = "macos")))]
#[must_use]
fn per_user_temporary_directory() -> Option<PathBuf> {
    None
}

/// **The runtime directory, made if it is not there and refused if it is not
/// ours.**
///
/// Three refusals and one repair, and the order is the point:
///
/// * `mkdir` is asked for `0700` **at creation**, not `chmod`-ed to it
///   afterwards, so there is no instant at which the directory exists and is
///   reachable by anybody else.
/// * What is standing there is then read with `lstat` and not `stat`: a
///   **symlink** is refused outright, because following one would be this
///   process putting its lock and its socket wherever somebody else pointed.
/// * The **owner** must be this user. A directory of somebody else's is refused
///   and never adopted, whatever its mode says.
/// * The **mode** is repaired rather than refused, and only once the owner check
///   above has passed: a directory this user owns is a directory this user may
///   set the mode of, and `0700` is what it must be. Refusing here instead would
///   be refusing the claim — and a refused claim is a Folio that hands over to
///   nobody and then opens a window that writes, which is the failure this whole
///   module exists to stop.
#[cfg(unix)]
pub(crate) fn prepare_runtime_directory() -> std::io::Result<PathBuf> {
    use std::io::{Error, ErrorKind};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    let directory = runtime_directory();
    match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = std::fs::symlink_metadata(&directory)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            "the Folio runtime directory is not a directory, and a link is not followed to find \
             out where it goes",
        ));
    }
    // SAFETY: `geteuid` reads this process's own credentials and cannot fail.
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            "the Folio runtime directory belongs to another user",
        ));
    }
    if metadata.permissions().mode() & 0o777 != 0o700 {
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(directory)
}

/// The name one directory's claim is taken under — a kernel object's name on
/// Windows, and the launch endpoint's path on Unix.
///
/// Public to this crate's tests and to `bt_app::persist`, because the two
/// properties that matter are properties of a string: two spellings of one
/// directory claim one name, and two different directories claim two. Both of
/// them are [`directory_tag`]'s, which is why the folding is there and not here.
#[cfg(windows)]
#[must_use]
pub fn claim_name(directory: &Path) -> String {
    format!("Local\\Folio.data.{}", directory_tag(directory))
}

/// **The same name where there is no kernel namespace to put it in: a path.**
///
/// It is the launch endpoint's path rather than the lock file's, and that is the
/// more useful of the two — the lock file has no reader, while this string is
/// what a trace, a test and `bt_app::persist`'s claim table mean when they name
/// "the door of that data directory".
#[cfg(unix)]
#[must_use]
pub fn claim_name(directory: &Path) -> String {
    socket_path_in(&runtime_directory(), &directory_tag(directory))
        .to_string_lossy()
        .into_owned()
}

/// The same name on a platform with neither a kernel namespace nor a socket.
#[cfg(all(not(windows), not(unix)))]
#[must_use]
pub fn claim_name(directory: &Path) -> String {
    format!("Folio.data.{}", directory_tag(directory))
}

/// **The launch endpoint's path for one data directory**, which is
/// [`claim_name`] as a path rather than as text.
///
/// `crate::launch_pipe` binds and connects through this rather than through the
/// string, so a runtime directory whose bytes are not text cannot turn into a
/// different path on the way to the kernel. The string is for reading; this is
/// for opening.
#[cfg(unix)]
#[must_use]
pub fn launch_socket_path(directory: &Path) -> PathBuf {
    socket_path_in(&runtime_directory(), &directory_tag(directory))
}

/// **The attention endpoint's path for one data directory** — the second door
/// of the same runtime directory (M4-7).
///
/// One folding and now three names built out of it, which is [`claim_name`]'s
/// own note with one more name in it: the claim, the launch endpoint and the
/// doorbell a hook rings are one directory's three files, so a run under an
/// isolated data directory misses the reader's everyday Folio at all three and
/// two spellings of one directory find each other at all three.
#[cfg(unix)]
#[must_use]
pub fn attention_socket_path(directory: &Path) -> PathBuf {
    attention_socket_path_in(&runtime_directory(), &directory_tag(directory))
}

/// A claim on one data directory, released when this value is dropped or when
/// the process ends, whichever comes first — **the Unix arm, where the guarantee
/// is built rather than preserved** (`docs/plans/port/macos-plan-2026-09-12.md`
/// §4.4 ④).
///
/// The field is an **open file description**, and that is the whole mechanism:
/// `flock` attaches to the description rather than to the file, so the lock is
/// held exactly while this descriptor is open, dropped by `File`'s own `Drop`
/// when this value goes, and dropped by the kernel when this process does —
/// quit, killed or crashed. There is no `Drop` written here because there is
/// nothing for one to do that closing the descriptor does not already do, and a
/// hand-written one that also unlinked the lock file would be a claim that
/// destroys the thing the next process is waiting on.
#[cfg(unix)]
pub struct DataDirectoryClaim {
    /// The descriptor the `flock` is held on. Never read from and never written
    /// to, which is the point rather than an oversight: what is in the file is
    /// not the claim — the descriptor being open is, and `File`'s own drop
    /// closing it is what releases it.
    #[expect(dead_code, reason = "the field is the mechanism rather than data")]
    lock: std::fs::File,
}

/// Take the claim on `directory`, or answer `None` because another live process
/// already holds it.
///
/// **`LOCK_NB`, so this asks rather than waits.** A blocking `flock` would turn
/// a second launch into a process that hangs until the first one quits, which is
/// the opposite of the answer the caller needs — the caller needs to be told it
/// is the second one, so that it can hand its command line over.
///
/// **The stale endpoint is cleaned up here, after the lock and never before**
/// (§R5). A socket file is the one thing a crashed holder does leave behind: the
/// kernel drops its `flock` and closes its listener, but the *name* stays in the
/// filesystem, and a client that connects to it is refused rather than told
/// there is nobody there. Unlinking it is safe on exactly one condition — that
/// nobody is listening on it — and holding this lock is what makes that true,
/// because the endpoint is only ever opened by the process that holds this
/// claim. Unlinking before the lock, or without it, would be one Folio deleting
/// a running Folio's front door.
///
/// **The claim is not released when this returns** — it is released when the
/// returned value is dropped, which for the product is when the process ends.
#[cfg(unix)]
#[must_use]
pub fn claim_data_directory(directory: &Path) -> Option<DataDirectoryClaim> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;

    let runtime = prepare_runtime_directory().ok()?;
    let tag = directory_tag(directory);
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(lock_path_in(&runtime, &tag))
        .ok()?;
    // SAFETY: the descriptor is this call's own and stays open for as long as
    // the value returned below lives.
    let taken = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    if !taken {
        return None;
    }
    // **Both doors, and for one reason** (M4-7). The launch endpoint and the
    // attention endpoint are two socket files bound by this one process on the
    // strength of this one lock, so they go stale together and they are safe to
    // unlink on exactly the same condition. A cleanup that took only the first
    // would leave a doorbell standing that every hook connects to and no
    // listener answers — a failure quieter than the one it half fixed, because
    // `folio attention` would keep exiting zero.
    for endpoint in [
        socket_path_in(&runtime, &tag),
        attention_socket_path_in(&runtime, &tag),
    ] {
        if std::fs::symlink_metadata(&endpoint).is_ok() {
            let _ = std::fs::remove_file(&endpoint);
        }
    }
    Some(DataDirectoryClaim { lock })
}

/// A machine with neither a kernel object nor a descriptor to hold always
/// answers "you are the one writer".
#[cfg(all(not(windows), not(unix)))]
pub struct DataDirectoryClaim;

#[cfg(all(not(windows), not(unix)))]
#[must_use]
pub fn claim_data_directory(_directory: &Path) -> Option<DataDirectoryClaim> {
    Some(DataDirectoryClaim)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — two spellings of one directory claim one name, and two directories
    /// claim two.
    #[cfg(windows)]
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

    /// **PIN — the launch socket fits in a `sockaddr_un` whatever the data
    /// directory's path is.**
    ///
    /// Runnable on every platform because both halves of it are pure: the
    /// digest is a fixed sixteen characters and the runtime directory is the
    /// system's, so the only number that can move is the length of the per-user
    /// temporary directory the system names — and a real macOS one is measured
    /// here, not imagined. That is the longer of the two answers
    /// [`runtime_directory_from`] can give, and it is built through that
    /// function rather than written out, so the two cannot drift apart.
    ///
    /// MUTATION: put the data directory's path in the socket's name instead of
    /// the digest and this goes red at the first long path.
    #[test]
    fn the_launch_socket_fits_a_sockaddr_un_however_long_the_data_directory_is() {
        let runtime = runtime_directory_from(
            Some(PathBuf::from("/var/folders/8x/_yq1234n5abc9xyz0000gn/T")),
            501,
        );
        let absurd = PathBuf::from(format!("/Users/{}/Folio", "d".repeat(3_000)));
        let tag = directory_tag(&absurd);
        assert_eq!(tag.len(), 16, "the digest is a fixed width: {tag}");
        let socket = socket_path_in(&runtime, &tag);
        let bytes = socket.as_os_str().as_encoded_bytes().len();
        assert!(
            fits_a_socket_path(&socket),
            "a {bytes}-byte path plus its terminator does not fit \
             {SOCKET_PATH_LIMIT} bytes of sun_path: {}",
            socket.display()
        );
        // **The attention endpoint is the longer of the two names** (M4-7), so
        // it is the one the promise is really about: five characters of suffix
        // became ten, and a length claim that only ever measured the shorter
        // name would be a claim that goes quiet exactly when it starts to
        // matter.
        let doorbell = attention_socket_path_in(&runtime, &tag);
        let bytes = doorbell.as_os_str().as_encoded_bytes().len();
        assert!(
            fits_a_socket_path(&doorbell),
            "a {bytes}-byte doorbell path plus its terminator does not fit \
             {SOCKET_PATH_LIMIT} bytes of sun_path: {}",
            doorbell.display()
        );
        assert_ne!(
            socket, doorbell,
            "the two doors of one directory are two names"
        );
        assert_eq!(
            SOCKET_PATH_LIMIT, 104,
            "macOS is the smaller of the two limits and is the one written down"
        );
        assert!(
            !fits_a_socket_path(&PathBuf::from("x".repeat(SOCKET_PATH_LIMIT))),
            "the terminator is inside sun_path, so a path exactly as wide as the field does not fit it"
        );
    }

    /// **PIN — the Unix arm exists, and it is the four things §R5 asked for.**
    ///
    /// This file's own text, because the arm it is about cannot be compiled by
    /// the machine most of this repository's work is done on, and the way a port
    /// arm rots is that somebody simplifies one of these four lines without a
    /// compiler anywhere that would notice. `update_check_transport_tests` is the
    /// precedent: a claim about a platform arm is a claim about the source when
    /// no runner can hold it.
    ///
    /// MUTATION: take the `symlink_metadata` out of `prepare_runtime_directory`,
    /// or make `DataDirectoryClaim`'s Unix body a unit struct again, and this
    /// goes red on Windows.
    #[test]
    fn the_unix_claim_is_a_flock_on_a_descriptor_in_a_private_runtime_directory() {
        let source = include_str!("instance.rs");

        let claim = source
            .split("#[cfg(unix)]\npub struct DataDirectoryClaim {")
            .nth(1)
            .expect("the Unix arm declares a DataDirectoryClaim with a body");
        let body = claim.split("\n}\n").next().unwrap_or_default();
        assert!(
            body.contains("lock: std::fs::File"),
            "the Unix claim holds an owned descriptor for the life of the process, \
             and a unit struct holds nothing: {body}"
        );

        let take = source
            .split("#[cfg(unix)]\n#[must_use]\npub fn claim_data_directory")
            .nth(1)
            .expect("the Unix arm has its own claim_data_directory");
        let take = take.split("\n}\n").next().unwrap_or_default();
        assert!(
            take.contains("libc::LOCK_EX | libc::LOCK_NB"),
            "the claim asks and does not wait, and it is exclusive: {take}"
        );
        assert!(
            take.find("flock").unwrap_or(usize::MAX) < take.find("remove_file").unwrap_or(0),
            "the stale endpoint is unlinked under the ownership lock and never before it (§R5)"
        );
        // **Both doors, or the cleanup is a half fix** (M4-7). A stale attention
        // socket that nobody unlinks is a doorbell every hook connects to and no
        // listener answers, which is quieter than the failure it half fixes:
        // `folio attention` keeps exiting zero.
        for door in [
            "socket_path_in(&runtime, &tag)",
            "attention_socket_path_in(&runtime, &tag)",
        ] {
            assert!(
                take.contains(door),
                "the claim clears one of this directory's two endpoints and leaves the other: \
                 {door} is not in {take}"
            );
        }

        let prepare = source
            .split("fn prepare_runtime_directory()")
            .nth(1)
            .expect("the Unix arm prepares its own runtime directory");
        let prepare = prepare.split("\n}\n").next().unwrap_or_default();
        assert!(
            prepare.contains(".mode(0o700)"),
            "the runtime directory is created 0700 rather than chmod-ed to it: {prepare}"
        );
        assert!(
            prepare.contains("symlink_metadata") && prepare.contains("is_symlink"),
            "a link standing where the runtime directory should be is refused, not followed"
        );
        assert!(
            prepare.contains("metadata.uid()"),
            "a runtime directory belonging to another user is refused"
        );
    }

    /// **RED — two claims on one canonical directory, and the second is `None`.**
    ///
    /// The guarantee §4.4 ④ found absent: before this ticket the Unix arm
    /// answered `Some` to everybody, so two Folios over one `$HOME` both wrote.
    ///
    /// MUTATION: drop `LOCK_NB`'s companion `LOCK_EX` for `LOCK_SH`, or answer
    /// `Some` without reading `flock`'s return, and the second claim succeeds.
    #[cfg(unix)]
    #[test]
    fn one_canonical_directory_is_claimed_once_and_released_on_drop() {
        let directory = scratch(line!());
        std::fs::create_dir_all(&directory).expect("make the data directory");
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
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// **RED — one directory reached through a symlink is one directory.**
    ///
    /// `to_lowercase` on a path says nothing about this, which is §R5's second
    /// complaint. A Folio started with `$HOME` pointed at a link — which is how
    /// half the home directories on a developer's Mac are spelled — must claim
    /// the same lock as one started at the target.
    #[cfg(unix)]
    #[test]
    fn a_symlink_to_one_directory_is_that_directory() {
        let root = scratch(line!());
        let real = root.join("real");
        let link = root.join("link");
        std::fs::create_dir_all(&real).expect("make the target");
        std::os::unix::fs::symlink(&real, &link).expect("point a link at it");

        assert_eq!(
            directory_tag(&real),
            directory_tag(&link),
            "two spellings of one directory are one claim"
        );
        assert_eq!(
            directory_tag(&real),
            directory_tag(&real.join("..").join("real")),
            "and so is a spelling that walks out and back"
        );

        let held = claim_data_directory(&real).expect("the first claim is taken");
        assert!(
            claim_data_directory(&link).is_none(),
            "the link must not be handed a second claim on the same directory"
        );
        drop(held);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **RED — case is the volume's question and the volume is asked.**
    ///
    /// Two spellings that differ only in case are one directory on a
    /// case-insensitive volume and two on a case-sensitive one, and the answer
    /// is not a property of the platform — an APFS volume can be either, and a
    /// Mac can have one of each mounted. So the test asks the volume it is
    /// standing on the same way the code does, and then holds the code to the
    /// volume's own answer in **both** directions.
    ///
    /// MUTATION: lowercase the path the Windows way and the case-sensitive half
    /// of this goes red; hash the path as written and the case-insensitive half
    /// does.
    #[cfg(unix)]
    #[test]
    fn case_folds_where_the_volume_folds_it_and_nowhere_else() {
        let root = scratch(line!());
        let lower = root.join("folio");
        std::fs::create_dir_all(&lower).expect("make the directory");
        let upper = root.join("FOLIO");
        let insensitive = upper.is_dir();

        if insensitive {
            assert_eq!(
                directory_tag(&lower),
                directory_tag(&upper),
                "this volume has one directory here, so Folio must have one claim"
            );
        } else {
            assert_ne!(
                directory_tag(&lower),
                directory_tag(&upper),
                "this volume has two directories here, so a folded name would have \
                 locked one of them out of its own data"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **RED — the runtime directory is private, and a link is not followed.**
    ///
    /// The private runtime directory is where the lock and the endpoint both
    /// live, so everything either of them promises rests on this: it is a real
    /// directory, it is this user's, and nobody else can reach into it.
    #[cfg(unix)]
    #[test]
    fn the_runtime_directory_is_this_users_and_is_0700() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let directory = prepare_runtime_directory().expect("the runtime directory is prepared");
        let metadata = std::fs::symlink_metadata(&directory).expect("it is there");
        assert!(!metadata.file_type().is_symlink(), "and it is not a link");
        assert!(metadata.is_dir());
        assert_eq!(
            metadata.permissions().mode() & 0o777,
            0o700,
            "nobody but this user may reach the lock or the endpoint"
        );
        // SAFETY: `geteuid` reads this process's own credentials.
        let uid = unsafe { libc::geteuid() };
        assert_eq!(metadata.uid(), uid);
        assert_eq!(
            directory.file_name().and_then(std::ffi::OsStr::to_str),
            Some(format!("folio-{uid}").as_str()),
            "the directory this machine actually prepared is this user's own: {}",
            directory.display()
        );
        // **And this machine's real runtime directory leaves room for an
        // endpoint** (RA-1). The pure test beside this one measures a per-user
        // temporary directory written down in this file; this measures the one
        // the machine running the test was actually given, which is the only
        // place the promise can be wrong.
        let doorbell = attention_socket_path_in(&directory, &digest(b"any data directory"));
        assert!(
            fits_a_socket_path(&doorbell),
            "this machine's runtime directory leaves no room for an endpoint inside \
             {SOCKET_PATH_LIMIT} bytes of sun_path: {}",
            doorbell.display()
        );
    }

    /// **PIN — one user's runtime directory is one path, and the process's
    /// environment is not one of the things it is made of** (RA-1).
    ///
    /// The rule with its one impure input handed in, which is the only way to
    /// state it without changing a process-wide variable out from under every
    /// other test running beside this one.
    ///
    /// MUTATION: read `$TMPDIR` again — the shipped behaviour before this
    /// ticket — and a Folio started by `launchd` and one started from an `ssh`
    /// session compute two of these, take two locks on one data directory, and
    /// both write.
    #[test]
    fn one_user_has_one_runtime_directory_and_it_is_short_enough_for_an_endpoint() {
        let per_user = PathBuf::from("/var/folders/8x/_yq1234n5abc9xyz0000gn/T/");
        let mine = runtime_directory_from(Some(per_user.clone()), 501);
        assert_eq!(
            mine,
            Path::new("/var/folders/8x/_yq1234n5abc9xyz0000gn/T/folio-501"),
            "the system's answer with this user's own name on a directory inside it \
             — and the separator the system writes at the end of it is not doubled"
        );
        assert_ne!(
            mine,
            runtime_directory_from(Some(per_user), 502),
            "two users on one machine are two runtime directories"
        );
        assert_eq!(
            runtime_directory_from(None, 501),
            Path::new("/tmp/folio-501"),
            "a system with no per-user directory to give still answers one path, \
             and it is the same one in every process of this user"
        );
        assert_eq!(
            runtime_directory_from(Some(PathBuf::from("T")), 501),
            Path::new("/tmp/folio-501"),
            "a runtime directory that is not rooted would be a different directory \
             for every directory a Folio was started in"
        );
        // **The length promise, at the location the lock and both endpoints
        // moved to.** The digest is the same sixteen characters wherever the
        // data directory is, and the doorbell is the longer of the two names.
        let doorbell = attention_socket_path_in(&mine, &digest(b"/Users/somebody/Folio"));
        assert!(
            fits_a_socket_path(&doorbell),
            "a per-user temporary directory plus folio-<uid> plus the doorbell does not \
             fit {SOCKET_PATH_LIMIT} bytes of sun_path: {}",
            doorbell.display()
        );
    }

    /// **PIN — the runtime directory is asked of the system and not of the
    /// environment** (RA-1).
    ///
    /// The half of the finding no value can state: that `$TMPDIR` is not read
    /// *anywhere* on the way to this path. It is this file's own text for
    /// `the_unix_claim_is_a_flock_on_a_descriptor_in_a_private_runtime_directory`'s
    /// reason — the arm cannot be compiled by the machine most of this
    /// repository's work is done on, and the way it rots is that somebody puts
    /// the variable back because it is one line shorter.
    ///
    /// MUTATION: answer `std::env::var_os("TMPDIR")` again and this goes red on
    /// Windows, naming the line.
    #[test]
    fn the_runtime_directory_is_asked_of_the_system_and_not_of_the_environment() {
        let source = include_str!("instance.rs");

        let rule = source
            .split("#[cfg(unix)]\n#[must_use]\npub fn runtime_directory()")
            .nth(1)
            .expect("the Unix arm says where this user's runtime files live");
        let rule = rule.split("\n}\n").next().unwrap_or_default();
        assert!(
            !rule.contains("TMPDIR") && !rule.contains("env::var"),
            "the runtime directory is read out of the environment again, so a Folio \
             started by launchd and one started from an ssh session take two different \
             locks on one data directory and both write it (RA-1): {rule}"
        );
        assert!(
            rule.contains("per_user_temporary_directory()"),
            "the system is no longer the thing being asked where this user's own \
             temporary directory is: {rule}"
        );

        let asked = source
            .split("#[cfg(target_os = \"macos\")]\n#[must_use]\nfn per_user_temporary_directory()")
            .nth(1)
            .expect("the macOS arm asks the system");
        let asked = asked.split("\n}\n").next().unwrap_or_default();
        assert!(
            asked.contains("libc::_CS_DARWIN_USER_TEMP_DIR"),
            "confstr(_CS_DARWIN_USER_TEMP_DIR) is the directory launchd sets $TMPDIR \
             from, and it is the one answer an ssh session and a launchd job agree \
             on: {asked}"
        );
    }

    /// **RED — a socket left behind by a crash is cleaned up by the next
    /// holder, and only once it holds the lock.**
    ///
    /// The one thing the kernel does not clean up. The `flock` is released by a
    /// crash, the listener is closed by a crash, and the *name* is not: it sits
    /// there refusing connections until somebody removes it.
    ///
    /// MUTATION: move the `remove_file` above the `flock` and the second half of
    /// this goes red — a Folio that was refused the claim would have deleted the
    /// running Folio's front door on its way out.
    #[cfg(unix)]
    #[test]
    fn a_stale_endpoint_is_removed_by_the_next_holder_and_only_under_the_lock() {
        let directory = scratch(line!());
        std::fs::create_dir_all(&directory).expect("make the data directory");
        let endpoint = launch_socket_path(&directory);

        // What a crashed holder leaves: a name in the filesystem with nobody
        // behind it.
        prepare_runtime_directory().expect("the runtime directory is prepared");
        std::fs::write(&endpoint, b"").expect("leave a stale endpoint behind");
        assert!(endpoint.exists());

        let held = claim_data_directory(&directory).expect("the claim is free and is taken");
        assert!(
            !endpoint.exists(),
            "the new holder cleared the name the dead one left: {}",
            endpoint.display()
        );

        // And now the other direction: while the lock is held, a process that is
        // refused the claim must not touch the endpoint on its way out.
        std::fs::write(&endpoint, b"").expect("stand a live endpoint up");
        assert!(
            claim_data_directory(&directory).is_none(),
            "the lock is still held"
        );
        assert!(
            endpoint.exists(),
            "a Folio that was refused the claim deleted the holder's front door"
        );
        drop(held);
        let _ = std::fs::remove_file(&endpoint);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A directory no other test in this process is using, so two of them can
    /// run at once.
    #[cfg(unix)]
    fn scratch(line: u32) -> PathBuf {
        std::env::temp_dir().join(format!(
            "bt-platform-instance-{}-{line}",
            std::process::id()
        ))
    }
}
