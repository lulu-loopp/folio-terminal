//! **The second launch's door into the first, on a machine whose kernel names
//! doors with paths** — one well-known Unix socket per data directory, and the
//! other end of `folio` being started while a `folio` is already running
//! (`docs/DESIGN.md` §7.59, §7.59b; M3-5).
//!
//! # What travels unchanged, and what could not
//!
//! **The conversation is the product's and does not move.** [`Decision<T>`]
//! decided once and carried, the reply written back, and the client's
//! [`CONFIRM`] as the commit point — §7.59b's five rulings are about a launch
//! and not about a transport, and every one of them is held here in the same
//! order. What changes is the three things Win32 was answering along the way:
//!
//! * **A name.** A named pipe lives in a kernel namespace and a Unix socket
//!   lives in the filesystem, so the endpoint is a *path* — and a path has a
//!   length limit a name does not. See [`crate::instance::SOCKET_PATH_LIMIT`].
//! * **A boundary.** `\\.\pipe\…` carried a DACL naming the **logon session**,
//!   deliberately, "so a second session of the same user (a service, another
//!   desktop) is outside it". A socket carries file permissions, and file
//!   permissions name a **user**. `0600` inside a `0700` runtime directory is
//!   therefore a *wider* principal than the Windows door: a second `ssh` login,
//!   a launch agent and a second console of the same user are all inside it.
//!   **That is stated rather than substituted** (`docs/plans/port/
//!   macos-plan-2026-09-12.md` §R6), and it is the right boundary for this
//!   particular door: what is on the other side of it is one `$HOME`'s data
//!   directory, which every session of that user shares anyway. The sentence
//!   the Windows module ends on is unchanged and is the one that matters: this
//!   is **not** a defence against a hostile process running as you.
//! * **A framing.** `PIPE_TYPE_MESSAGE` made "one frame" a kernel fact. A
//!   `SOCK_STREAM` socket has no frames, so this module writes its own: four
//!   bytes of big-endian length and then that many bytes, bounded by
//!   [`crate::attention_pipe::MAX_MESSAGE_BYTES`] on the way in *and* on the
//!   way out. A grammar above it never sees the difference.
//!
//! # Who is on the other end, and the answer is not a DACL
//!
//! The Windows client asks the kernel which process is serving the pipe and
//! refuses to write a command line to an image that is not this program
//! (§7.59b, C-7). The same rule is kept and it is asked **both ways**, because
//! on this platform both ends can ask:
//!
//! * **The server** takes the peer's credentials off the connected socket —
//!   [`crate::peer::credentials`], which is the uid and the pid this kernel
//!   recorded when that peer connected — and refuses a peer whose uid is not its
//!   own or whose executable is not the same file as its own. A pid out of a
//!   frame would be a number the peer chose; these come from the kernel. (The
//!   two calls that answer it are not the same two on the two Unixes this
//!   workspace compiles, which is what that module is for.)
//! * **The client** reads the endpoint's own file before it connects — a
//!   socket, not a link, owned by this user, mode `0600` — and then asks the
//!   same two questions of the connected peer, before a byte of the command
//!   line is written. A folder somebody is standing in is not told to a process
//!   that is not this program.
//!
//! **The stale endpoint is not this module's to clean up.** A socket file
//! outlives the process that bound it, so the file being there means nothing;
//! what means something is the claim in [`crate::instance`], and that is where
//! the unlink happens — after the `flock` and never before it (§R5). A
//! `LaunchPipe` that finds the name already taken **refuses**, because the one
//! thing it must never do is take a door away from whoever is standing behind
//! it.

use std::{
    io::{self, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        io::{AsRawFd, RawFd},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use crate::attention_pipe::MAX_MESSAGE_BYTES;
use crate::instance::SOCKET_PATH_LIMIT;

/// **How long a second launch will wait for the running Folio before opening a
/// window of its own.** Wire policy, and the Windows module's own number for
/// the Windows module's own reasons — a person who double-clicked an icon is
/// waiting for a window, not for a syscall.
pub const HANDOVER_BUDGET: Duration = Duration::from_secs(2);

/// How long the server holds one connection open waiting for a step of the
/// conversation, and how long step 5 waits for the confirmation. A client that
/// read the reply confirms in microseconds; a client that does not is a client
/// whose launch is **not committed** (§7.59b).
const STEP_DEADLINE: Duration = Duration::from_millis(250);

/// **The client's one word back**, and the whole of step 4's second half. A
/// fixed token, because there is exactly one thing it can mean: *I read your
/// reply and I am not going to open a window*.
pub const CONFIRM: &str = "ok";

/// **What the first process decided about the second's request.**
///
/// `admitted` is `Some` only for a request that was let in, and it is dropped
/// un-committed on every path where the client does not confirm — which is what
/// makes a reservation held inside it safe (§7.59b).
pub struct Decision<T> {
    /// The one line written back to the client.
    pub reply: String,
    /// The launch itself, if this decision admitted one.
    pub admitted: Option<T>,
}

/// The path this process's launch endpoint takes for `directory`, or `None` for
/// a path this wire cannot address.
///
/// `None` is a real answer and not an error to paper over, and it is the
/// Windows arm's own rule: a launch that cannot find the door opens its own
/// window, which is exactly what every Folio did before this channel existed.
/// There are two ways to get it, and both are honest refusals rather than
/// failures — a runtime directory whose bytes are not text (so the two ends
/// could not agree on one spelling of the path), and a path longer than a
/// `sockaddr_un` (so the kernel would not hear the whole of it).
#[must_use]
pub fn endpoint_for(directory: &Path) -> Option<String> {
    let path = crate::instance::launch_socket_path(directory);
    if !crate::instance::fits_a_socket_path(&path) {
        return None;
    }
    path.to_str().map(str::to_owned)
}

/// **The endpoint.** Live from the moment [`LaunchPipe::start`] returns, closed
/// when this is dropped.
pub struct LaunchPipe {
    name: String,
    path: PathBuf,
    /// The write end of the self-pipe the listener also waits on — this
    /// module's `SetEvent`, and the only way to wake a thread parked in `poll`.
    stop: RawFd,
    listener: Option<JoinHandle<()>>,
}

impl LaunchPipe {
    /// Open this process's launch endpoint for `directory` and start listening.
    ///
    /// **It returns already listening**, and here that is a fact rather than a
    /// handshake: `bind` and `listen` both happen on this thread, before the
    /// listener thread is spawned, so there is no window in which a second
    /// launch could find no socket. The Windows arm needs a channel for the
    /// same promise because `CreateNamedPipeW` runs on the listener thread.
    ///
    /// **The caller is expected to hold the claim on `directory`**
    /// ([`crate::instance::claim_data_directory`]) and this does not re-take it
    /// — a `flock` is held by an open file description, so asking for it a
    /// second time from the same process would be refused by the kernel and
    /// would prove the opposite of what it looks like. What this does instead
    /// is refuse a name that is already bound: if somebody is behind that door,
    /// they are the writer and this process is not.
    ///
    /// The two closures are the two halves of step 3 and step 5, and the order
    /// between them is the contract this module exists to keep — `decide`
    /// answers a [`Decision`] on the listener thread, and `commit` is handed
    /// **that same admitted value** once the client has confirmed.
    pub fn start<T, D, C>(directory: &Path, decide: D, commit: C) -> io::Result<Self>
    where
        T: Send + 'static,
        D: Fn(&str) -> Option<Decision<T>> + Send + 'static,
        C: Fn(T) + Send + 'static,
    {
        let path = crate::instance::launch_socket_path(directory);
        let bytes = path.as_os_str().as_encoded_bytes().len();
        if !crate::instance::fits_a_socket_path(&path) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "a {bytes}-byte endpoint path does not fit {SOCKET_PATH_LIMIT} bytes of \
                     sun_path"
                ),
            ));
        }
        let Some(name) = path.to_str().map(str::to_owned) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the endpoint's path is not text, so the two ends of this wire could not agree \
                 on one spelling of it",
            ));
        };
        // The claim prepared this already. Preparing it again is idempotent and
        // is what lets the endpoint's promise — a private, owner-checked,
        // link-free directory — stand on this module's own work rather than on
        // a caller having done it.
        crate::instance::prepare_runtime_directory()?;
        let listener = UnixListener::bind(&path)?;
        // **The mode is set rather than left to the umask.** `bind` honours it,
        // and a process started under `umask 0` would otherwise stand a
        // world-writable door up. The directory above is `0700` either way, so
        // this is the second of two locks on the same gate rather than the
        // only one.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let (wake, stop) = match self_pipe() {
            Ok(pair) => pair,
            Err(error) => {
                let _ = std::fs::remove_file(&path);
                return Err(error);
            }
        };
        let listener = std::thread::Builder::new()
            .name("folio-launch-endpoint".to_owned())
            .spawn(move || {
                listen(&listener, wake, &decide, &commit);
            });
        match listener {
            Ok(listener) => Ok(Self {
                name,
                path,
                stop,
                listener: Some(listener),
            }),
            Err(error) => {
                // SAFETY: nothing else ever had either descriptor — the thread
                // that would have owned the read end was never started.
                unsafe {
                    libc::close(wake);
                    libc::close(stop);
                }
                let _ = std::fs::remove_file(&path);
                Err(error)
            }
        }
    }

    /// The name this endpoint is listening on — for a test, and for a trace.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for LaunchPipe {
    fn drop(&mut self) {
        let word = [1u8];
        // SAFETY: one byte into this process's own pipe, whose read end is held
        // by the thread joined on the next line.
        unsafe {
            libc::write(self.stop, word.as_ptr().cast(), 1);
        }
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
        // SAFETY: the only other user of this descriptor has been joined.
        unsafe {
            libc::close(self.stop);
        }
        // **The name goes with the listener**, wherever the listener is owned
        // by something that is dropped. In the product it is not: `bt-app`
        // parks the endpoint in a `OnceLock` static, and a static is never
        // dropped, so a real Folio leaves its name behind on every exit and the
        // claim's own cleanup in `crate::instance` is what clears it on the
        // next start. This line is therefore for a scope that owns one — every
        // test below, and anything that comes later — rather than the general
        // answer, which is why the general answer is under the lock and not
        // here.
        let _ = std::fs::remove_file(&self.path);
    }
}

/// One connection at a time, for as long as the endpoint is open.
///
/// **One at a time and not a pool**, which is the Windows arm's own choice for
/// its own reason: this endpoint serves a person double-clicking an icon, and
/// being served takes microseconds because nothing in the transaction touches
/// the window thread. A caller that arrives mid-conversation waits in the
/// kernel's accept queue rather than being refused.
fn listen<T>(
    listener: &UnixListener,
    wake: RawFd,
    decide: &(impl Fn(&str) -> Option<Decision<T>> + ?Sized),
    commit: &(impl Fn(T) + ?Sized),
) {
    loop {
        let mut waiting = [
            libc::pollfd {
                fd: listener.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: wake,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: both descriptors are open for the whole of this call — the
        // listener is borrowed and the read end is closed only below.
        let ready = unsafe { libc::poll(waiting.as_mut_ptr(), 2, -1) };
        if ready < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        // **The stop word is read first.** A `drop` that raced an incoming
        // connection must end the thread rather than serve one more caller
        // whose commit would reach a runtime that is on its way out.
        if waiting[1].revents != 0 {
            break;
        }
        if waiting[0].revents & libc::POLLIN == 0 {
            continue;
        }
        match listener.accept() {
            Ok((stream, _)) => serve(stream, decide, commit),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) => {}
            Err(_) => break,
        }
    }
    // SAFETY: this thread is the only owner of the read end, and it is ending.
    unsafe {
        libc::close(wake);
    }
}

/// **One whole conversation**, steps 1 to 5, and every one of them bounded.
///
/// Nothing here reports a failure anywhere: there is no log a launch would look
/// in and no reader to tell. A step that does not complete ends the connection,
/// the decision is dropped un-committed, and the client's own fallback — a
/// window of its own — is the report.
fn serve<T>(
    mut stream: UnixStream,
    decide: &(impl Fn(&str) -> Option<Decision<T>> + ?Sized),
    commit: &(impl Fn(T) + ?Sized),
) {
    if stream.set_read_timeout(Some(STEP_DEADLINE)).is_err()
        || stream.set_write_timeout(Some(STEP_DEADLINE)).is_err()
    {
        return;
    }
    // **Step 1 from this side.** A peer that is not this user, or not this
    // program, is not read from and not answered.
    if vetted_peer(&stream).is_err() {
        return;
    }
    let Ok(frame) = read_frame(&mut stream) else {
        return;
    };
    let line = String::from_utf8_lossy(&frame).into_owned();
    // **A line this build does not understand is dropped without a word** — the
    // attention wire's founding rule at the second door. There is no reply that
    // would help: a caller speaking a grammar this build has not got is not a
    // launch that arrived slightly wrong.
    let Some(decision) = decide(&line) else {
        return;
    };
    if write_frame(&mut stream, decision.reply.as_bytes()).is_err() {
        // A write that did not land is a client that has already given up and
        // gone. Returning here drops `decision` — and with it whatever
        // reservation the decision was holding.
        return;
    }
    // Step 4 happening on the other side. **That word is the commit point** —
    // a close, a timeout or anything that is not [`CONFIRM`] leaves the launch
    // un-committed, because in every one of those cases the client is about to
    // open a window of its own.
    let Ok(confirmation) = read_frame(&mut stream) else {
        return;
    };
    if String::from_utf8_lossy(&confirmation).trim() != CONFIRM {
        return;
    }
    let Some(admitted) = decision.admitted else {
        // A refusal the client acknowledged. There was never anything to park.
        return;
    };
    commit(admitted);
}

/// **A second launch's whole conversation with the Folio that is already
/// running.**
///
/// Look at the door, connect, ask who answered, write the request, read the
/// answer, let the caller act on it, confirm, close. Bounded end to end by
/// [`HANDOVER_BUDGET`], because what a person is waiting for is a window.
///
/// **`on_reply` runs while the socket is still open**, and that is the whole
/// reason it is a callback rather than a return value: the running process does
/// not act on the request until this end has confirmed, so whatever the caller
/// does with the answer has to be done *inside* the conversation.
///
/// **Its first argument is the server's process id as the kernel reports it**
/// (§7.59b, C-7) — `LOCAL_PEERPID` off the connected socket, never a number out
/// of the reply, because the thing being talked to must not be able to write
/// the answer to "who am I talking to".
///
/// The errors are the caller's fallback and not a report to anyone: `NotFound`
/// is nobody behind that name, `TimedOut` is the budget spent,
/// `PermissionDenied` is a door that is not this program's, and in every case
/// the answer is the same — this process opens the window itself. **A failure
/// after the request was written is safe by construction**: the server commits
/// on this end's [`CONFIRM`], which a failing handover never sends.
pub fn hand_over(
    endpoint: &str,
    request: &str,
    on_reply: impl FnOnce(u32, &str),
) -> io::Result<()> {
    if request.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "request longer than the launch endpoint's frame bound",
        ));
    }
    let began = Instant::now();
    let path = Path::new(endpoint);
    // **The door is read before it is opened.** `connect` on a path somebody
    // else stood up would be this process introducing itself to whoever is
    // there; the checks below are what make "nobody but this user could have
    // put this here" true before that happens.
    vetted_endpoint(path)?;
    let mut stream = UnixStream::connect(path)?;
    // **Step 1: who answered?** Before a byte of the command line is written —
    // the request names a folder somebody is standing in.
    let server = vetted_peer(&stream)?;
    let left = remaining(began);
    stream.set_read_timeout(Some(left))?;
    stream.set_write_timeout(Some(left))?;
    write_frame(&mut stream, request.as_bytes()).map_err(ran_out)?;
    let reply = read_frame(&mut stream).map_err(ran_out)?;
    on_reply(server, &String::from_utf8_lossy(&reply));
    // **Step 4's second half, and the server's commit point.** The last
    // statement of the conversation, written out rather than left implicit so
    // that the order of these lines is the order of the protocol.
    write_frame(&mut stream, CONFIRM.as_bytes()).map_err(ran_out)?;
    drop(stream);
    Ok(())
}

/// Whatever is left of the budget, and never zero.
///
/// `set_read_timeout(Some(Duration::ZERO))` is how the standard library spells
/// *block forever*, so a budget that has already run out has to be a very short
/// wait rather than no wait — the one shape this function refuses to produce is
/// the one that would hang.
fn remaining(began: Instant) -> Duration {
    HANDOVER_BUDGET
        .saturating_sub(began.elapsed())
        .max(Duration::from_millis(1))
}

/// **A socket timeout is `WouldBlock`, and this wire's word for it is
/// `TimedOut`.**
///
/// The caller reads these kinds — `bt_app::launch_wire` and the tests below —
/// and the promise is stated in the Windows arm's vocabulary: the budget being
/// spent is a timeout. Translating it here rather than at four call sites is
/// what keeps the two arms answering one word.
fn ran_out(error: io::Error) -> io::Error {
    if error.kind() == io::ErrorKind::WouldBlock {
        return io::Error::new(
            io::ErrorKind::TimedOut,
            "the running Folio did not answer inside the launch's allowance",
        );
    }
    error
}

/// **The endpoint's own file, read before anybody connects to it.**
///
/// Four questions and one refusal between them, and `symlink_metadata` rather
/// than `metadata` is the first of them: a link standing where the endpoint
/// should be is somebody redirecting this launch, and following it to find out
/// where would be taking their word for it.
fn vetted_endpoint(path: &Path) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    let refuse =
        |reason: &'static str| Err(io::Error::new(io::ErrorKind::PermissionDenied, reason));
    if metadata.file_type().is_symlink() || !metadata.file_type().is_socket() {
        return refuse("what is standing at the launch endpoint's path is not a socket");
    }
    // SAFETY: `geteuid` reads this process's own credentials and cannot fail.
    if metadata.uid() != unsafe { libc::geteuid() } {
        return refuse("the launch endpoint belongs to another user");
    }
    if metadata.permissions().mode() & 0o777 != 0o600 {
        return refuse("the launch endpoint is reachable by somebody who is not its owner");
    }
    Ok(())
}

/// **The process at the other end of this socket, if it is this program running
/// as this user** — and its process id, which is the kernel's answer and not
/// the peer's.
///
/// Asked by both ends, which is the difference from Windows worth reading
/// twice: there, only the client could ask (`GetNamedPipeServerProcessId`), and
/// the server's half of the boundary was the DACL. Here the DACL's replacement
/// is a mode bit on a file, which says who could have *created* the door rather
/// than who is standing at it — so the server asks too.
fn vetted_peer(stream: &UnixStream) -> io::Result<u32> {
    let peer = crate::peer::credentials(stream)?;
    // SAFETY: `geteuid` reads this process's own credentials and cannot fail.
    if peer.uid != unsafe { libc::geteuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the other end of the launch endpoint is running as another user",
        ));
    }
    let Some(pid) = peer.pid else {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the launch endpoint named a process id that is not a process",
        ));
    };
    vet_executable(&peer_executable(pid)?)?;
    Ok(pid)
}

/// **Is that executable this one?**
///
/// Device and inode, which is the filesystem's own identity: the two paths can
/// be spelled differently — a bundle reached through a symlink, a `..` — and
/// still be one file, and two builds in two folders are two files however alike
/// their names are.
///
/// **This is stricter than the Windows arm and deliberately so.** There the
/// comparison is on the file *name*, because a second Folio may be a newer
/// build in another folder and the wire's own version check is what answers
/// that difference. Here the peer's path comes from the kernel rather than from
/// an image name this program chose, and an application on this platform lives
/// at one path inside one bundle — so the stronger question is the one that can
/// be asked, and the cost of the difference is benign: two Folios that are
/// genuinely two files refuse each other, and the second one opens its own
/// window, which is the fallback every path here already takes.
fn vet_executable(theirs: &Path) -> io::Result<()> {
    let mine = std::env::current_exe()?;
    let (mine, theirs) = (std::fs::metadata(mine)?, std::fs::metadata(theirs)?);
    if mine.dev() == theirs.dev() && mine.ino() == theirs.ino() {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "the launch endpoint is held by a process that is not this program",
    ))
}

/// The executable one pid is running, from the kernel.
#[cfg(target_os = "macos")]
fn peer_executable(pid: u32) -> io::Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;

    /// `<sys/proc_info.h>`: `PROC_PIDPATHINFO_MAXSIZE`, the buffer
    /// `proc_pidpath` documents as the only size it will write into.
    const PATH_MAX_SIZE: usize = 4 * 1024;

    // `<libproc.h>`, which is in libSystem and needs no crate: the path of the
    // executable a process is running, which is a question `/proc` answers on
    // the other Unix and nothing in the standard library answers on this one.
    unsafe extern "C" {
        fn proc_pidpath(pid: libc::c_int, buffer: *mut libc::c_void, size: u32) -> libc::c_int;
    }

    let mut buffer = vec![0u8; PATH_MAX_SIZE];
    // SAFETY: the buffer is this frame's and its length is what is handed over.
    let written = unsafe {
        proc_pidpath(
            libc::pid_t::try_from(pid).unwrap_or(0),
            buffer.as_mut_ptr().cast(),
            u32::try_from(PATH_MAX_SIZE).unwrap_or(0),
        )
    };
    if written <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the other end of the launch endpoint has an image this process cannot read",
        ));
    }
    buffer.truncate(usize::try_from(written).unwrap_or(0));
    Ok(PathBuf::from(std::ffi::OsString::from_vec(buffer)))
}

/// The same question where `/proc` answers it.
#[cfg(not(target_os = "macos"))]
fn peer_executable(pid: u32) -> io::Result<PathBuf> {
    Ok(PathBuf::from(format!("/proc/{pid}/exe")))
}

/// The two descriptors a `drop` uses to reach a thread parked in `poll`.
///
/// A self-pipe rather than a flag, for the reason the Windows arm has an event:
/// the listener may be anywhere between two waits when `drop` fires, and a byte
/// already sitting in a pipe is read whenever the thread next looks. A flag
/// would be read only by a thread that had already woken up.
fn self_pipe() -> io::Result<(RawFd, RawFd)> {
    let mut ends = [0 as libc::c_int; 2];
    // SAFETY: `ends` is a live array of exactly the two integers `pipe` writes.
    if unsafe { libc::pipe(ends.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((ends[0], ends[1]))
}

/// **One frame out**, length first and bounded on the way out as well as on the
/// way in — `crate::attention_pipe`'s own rule: this end has no reason to trust
/// that the far end applied the bound, and neither has that one.
fn write_frame(stream: &mut UnixStream, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a frame longer than the launch endpoint's bound",
        ));
    }
    let Ok(length) = u32::try_from(payload.len()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a frame longer than the launch endpoint's bound",
        ));
    };
    stream.write_all(&length.to_be_bytes())?;
    stream.write_all(payload)
}

/// **One frame in**, and a frame that says it is longer than this endpoint will
/// take is refused before a byte of it is read.
///
/// `read_exact` and not `read`: a stream socket is allowed to hand over a
/// prefix, and a parser given a prefix of a JSON object is a parser that
/// answers the wrong question rather than no question. That is what
/// `PIPE_READMODE_MESSAGE` was buying on the other platform.
fn read_frame(stream: &mut UnixStream) -> io::Result<Vec<u8>> {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a frame longer than the launch endpoint's bound",
        ));
    }
    let mut payload = vec![0u8; length];
    stream.read_exact(&mut payload)?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// A data directory no other test in this process is using, so two of them
    /// can run at once — and one that really exists, because the endpoint's
    /// name is the filesystem's answer about it.
    fn scratch(line: u32) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("bt-platform-launch-{}-{line}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("make the data directory");
        directory
    }

    /// **RED — one request crosses, and it is answered.**
    ///
    /// End to end against a real socket, because what is worth pinning is a
    /// property of the real one: that a client can connect the instant `start`
    /// hands back, that the bytes arrive whole, and that the reply written on
    /// the listener thread reaches the caller. It also pins the peer id: the
    /// number handed to `on_reply` is the kernel's answer for the far end and
    /// not anything either side wrote into a frame.
    ///
    /// MUTATION: read the request with `read` instead of `read_exact` and a
    /// frame split across two packets arrives truncated.
    #[test]
    fn a_request_crosses_the_launch_endpoint_and_is_answered() {
        let directory = scratch(line!());
        let (sender, committed) = mpsc::channel();
        let endpoint = LaunchPipe::start(
            &directory,
            |line| {
                Some(Decision {
                    reply: format!("answer to {line}"),
                    admitted: Some(line.to_owned()),
                })
            },
            move |admitted: String| {
                let _ = sender.send(admitted);
            },
        )
        .expect("open the launch endpoint");
        let (heard, replies) = mpsc::channel();
        hand_over(endpoint.name(), "one request", |server, reply| {
            assert_eq!(
                server,
                std::process::id(),
                "the server this client vetted is the process it is actually talking to"
            );
            let _ = heard.send(reply.to_owned());
        })
        .expect("hand the request over");
        assert_eq!(
            replies.recv_timeout(Duration::from_secs(5)).ok(),
            Some("answer to one request".to_owned())
        );
        assert_eq!(
            committed.recv_timeout(Duration::from_secs(5)).ok(),
            Some("one request".to_owned()),
            "and the request is committed once the caller has let go"
        );
    }

    /// **RED — the endpoint is `0600` inside a `0700` directory, and it is gone
    /// when its owner is.**
    ///
    /// The whole of this door's boundary is two mode bits and a directory, so
    /// they are measured rather than assumed — and the second half is what
    /// keeps an orderly quit from leaving a name for the next start to trip on.
    #[test]
    fn the_endpoint_is_private_to_this_user_and_goes_when_the_listener_does() {
        let directory = scratch(line!());
        let endpoint = LaunchPipe::start(&directory, |_| None::<Decision<()>>, |()| {})
            .expect("open the launch endpoint");
        let path = PathBuf::from(endpoint.name());
        let metadata = std::fs::symlink_metadata(&path).expect("the endpoint is a real file");
        assert!(metadata.file_type().is_socket());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        let runtime = std::fs::metadata(path.parent().expect("it lives in the runtime directory"))
            .expect("and that directory is there");
        assert_eq!(runtime.permissions().mode() & 0o777, 0o700);
        drop(endpoint);
        assert!(
            !path.exists(),
            "a clean quit left its door standing: {}",
            path.display()
        );
    }

    /// **RED — a line this build does not understand is dropped without a reply
    /// and without effect.**
    ///
    /// The attention wire's founding rule at the second door, and the second
    /// assertion is the one that matters: a refused line must not reach the
    /// window thread.
    ///
    /// MUTATION: commit before the reply is written, or commit on a `None`
    /// verdict, and the second assertion goes red.
    #[test]
    fn a_line_the_grammar_refuses_is_dropped_without_a_reply_and_without_effect() {
        let directory = scratch(line!());
        let (sender, committed) = mpsc::channel();
        let endpoint = LaunchPipe::start(
            &directory,
            |line| {
                (line == "good").then(|| Decision {
                    reply: "ok".to_owned(),
                    admitted: Some(line.to_owned()),
                })
            },
            move |admitted: String| {
                let _ = sender.send(admitted);
            },
        )
        .expect("open the launch endpoint");
        let mut answered = false;
        let _ = hand_over(endpoint.name(), "rubbish", |_, _| answered = true);
        assert!(!answered, "a refused line is answered with silence");
        assert!(
            committed.recv_timeout(Duration::from_millis(500)).is_err(),
            "and it never reaches the window thread"
        );
    }

    /// **RED (§7.59b) — a client that read the reply and then went away leaves
    /// nothing behind.**
    ///
    /// The duplicate this closes: a server that committed on its own write
    /// would hand a launch to the window thread while the client — which is
    /// allowed eight times as long to collect that reply — opened a window of
    /// its own. One launch, a window **and** a tab.
    ///
    /// The client here is written by hand rather than through [`hand_over`],
    /// because what is being pinned is precisely the step `hand_over` always
    /// takes: it reads the reply and then stops.
    #[test]
    fn a_client_that_never_confirms_commits_nothing() {
        let directory = scratch(line!());
        let (sender, committed) = mpsc::channel();
        let endpoint = LaunchPipe::start(
            &directory,
            |line| {
                Some(Decision {
                    reply: format!("answer to {line}"),
                    admitted: Some(line.to_owned()),
                })
            },
            move |admitted: String| {
                let _ = sender.send(admitted);
            },
        )
        .expect("open the launch endpoint");
        {
            let mut stream = UnixStream::connect(endpoint.name()).expect("connect to the endpoint");
            write_frame(&mut stream, b"one request").expect("write the request");
            let reply = read_frame(&mut stream).expect("read the reply");
            assert_eq!(reply, b"answer to one request");
            // And now this client goes away without a word, which is every
            // client that lost the reply, crashed, or was killed between the
            // two statements above.
        }
        assert!(
            committed
                .recv_timeout(HANDOVER_BUDGET + Duration::from_millis(500))
                .is_err(),
            "the launch was handed to the window thread on the strength of a reply the client \
             never acted on"
        );
    }

    /// **RED — a request longer than the endpoint's frame bound never leaves
    /// this process.**
    #[test]
    fn a_request_past_the_frame_bound_is_refused_before_it_is_written() {
        let oversized = "x".repeat(MAX_MESSAGE_BYTES + 1);
        let refused = hand_over("/nowhere/folio.sock", &oversized, |_, _| {
            panic!("an oversized request must not reach a socket at all");
        })
        .expect_err("an oversized request is refused");
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);
    }

    /// **RED — a Folio that takes the request and never answers costs the
    /// launch its budget and nothing more.**
    ///
    /// MUTATION: leave the read timeout unset — which on a socket means *block
    /// forever* — and this test never returns.
    #[test]
    fn a_server_that_never_answers_costs_the_launch_its_budget_and_no_more() {
        let directory = scratch(line!());
        let endpoint = LaunchPipe::start(
            &directory,
            |_| {
                std::thread::sleep(HANDOVER_BUDGET * 3);
                Some(Decision {
                    reply: "far too late".to_owned(),
                    admitted: None::<()>,
                })
            },
            |()| {},
        )
        .expect("open the launch endpoint");
        let began = Instant::now();
        let refused = hand_over(endpoint.name(), "one request", |_, _| {
            panic!("a hung Folio has not answered");
        })
        .expect_err("a Folio that never answers cannot be handed anything");
        assert_eq!(refused.kind(), io::ErrorKind::TimedOut);
        assert!(
            began.elapsed() + Duration::from_millis(20) >= HANDOVER_BUDGET,
            "the budget is waited out before the launch gives up"
        );
        assert!(
            began.elapsed() < HANDOVER_BUDGET * 3,
            "and it is not waited past: the person is owed a window, not a syscall"
        );
    }

    /// **RED — a name with nobody behind it is a refusal and not a wait.**
    ///
    /// The ordinary case of a first launch: there is no Folio running, so there
    /// is no socket, and the answer has to come back immediately rather than
    /// after [`HANDOVER_BUDGET`].
    #[test]
    fn an_endpoint_nobody_is_listening_on_refuses_at_once() {
        let directory = scratch(line!());
        let name = endpoint_for(&directory).expect("the name is addressable");
        let began = Instant::now();
        let refused = hand_over(&name, "one request", |_, _| {
            panic!("there is nobody to answer");
        })
        .expect_err("a name with nobody behind it cannot be handed anything");
        assert!(
            began.elapsed() < HANDOVER_BUDGET,
            "a launch with no Folio running must not wait out the budget"
        );
        assert_eq!(refused.kind(), io::ErrorKind::NotFound);
    }

    /// **RED — a door that is not this program's is not written to.**
    ///
    /// The peer rule, asked of the one half of it a single process can stand
    /// both sides of: an executable that is a real file and is not this one is
    /// refused, and this one is not. The connected half of the rule is proved
    /// by every test above — each of them is a peer whose executable is this
    /// test binary, and each of them is served.
    ///
    /// MUTATION: compare file names instead of device and inode and the first
    /// half of this stays green while two different builds start trusting each
    /// other.
    #[test]
    fn an_executable_that_is_not_this_one_is_refused() {
        let mine = std::env::current_exe().expect("this process has an image");
        vet_executable(&mine).expect("this program is this program");
        let refused = vet_executable(Path::new("/bin/sh"))
            .expect_err("a shell is not a Folio, whatever it is called");
        assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied);
        assert!(
            vet_executable(Path::new("/nowhere/folio")).is_err(),
            "and an image this process cannot read is a peer it cannot vouch for"
        );
    }

    /// **RED — a door standing at the endpoint's path that is not a private
    /// socket of this user's is not connected to.**
    ///
    /// What this refuses is the shape of an attack the Windows DACL refused in
    /// the kernel: something else standing where the endpoint should be. A
    /// regular file and a symlink are the two cheapest ways to do it.
    #[test]
    fn the_door_is_read_before_it_is_opened() {
        let directory = scratch(line!());
        let path = PathBuf::from(endpoint_for(&directory).expect("the name is addressable"));
        crate::instance::prepare_runtime_directory().expect("the runtime directory is prepared");

        std::fs::write(&path, b"not a socket").expect("stand a plain file there");
        let refused = vetted_endpoint(&path).expect_err("a regular file is not an endpoint");
        assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied);
        std::fs::remove_file(&path).expect("clear it");

        std::os::unix::fs::symlink("/dev/null", &path).expect("stand a link there");
        let refused = vetted_endpoint(&path).expect_err("a link is not followed to find out");
        assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied);
        std::fs::remove_file(&path).expect("clear it");

        assert_eq!(
            vetted_endpoint(&path)
                .expect_err("and a name with nothing at it is nobody home")
                .kind(),
            io::ErrorKind::NotFound
        );
    }

    /// **PIN — two spellings of one data directory address one endpoint.**
    ///
    /// The isolation promise, and it is a property of the pair: a run under an
    /// isolated data directory must miss the reader's everyday Folio at *both*
    /// doors, and two spellings of one directory must find each other at both.
    #[test]
    fn one_directory_is_one_claim_and_one_endpoint_however_it_is_spelled() {
        let root = scratch(line!());
        let real = root.join("real");
        std::fs::create_dir_all(&real).expect("make the data directory");
        let link = root.join("link");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&real, &link).expect("point a link at it");

        assert_eq!(
            endpoint_for(&real),
            endpoint_for(&link),
            "two spellings of one directory are one door"
        );
        assert_ne!(
            endpoint_for(&real),
            endpoint_for(&root),
            "and a run under an isolated data directory finds a different one"
        );
        assert_eq!(
            endpoint_for(&real).expect("addressable"),
            crate::instance::claim_name(&real),
            "the claim and the endpoint are one name, so one cannot be taken without the other"
        );
    }
}
