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
//!   [`MAX_FRAME_BYTES`] on the way in *and* on the
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
//!   [`crate::peer::credentials`], the uid and the pid the kernel names for
//!   that peer — and refuses a peer whose uid is not its own or whose
//!   executable is not a Folio: an image whose file name is not this
//!   executable's ([`vet_executable`]). A pid out of a frame would be a number
//!   the peer chose; these come from the kernel. (The two calls that answer it
//!   are not the same two on the two Unixes this workspace compiles, which is
//!   what that module is for.) **A peer it refuses keeps its connection until
//!   its own first move**, because the client is asking the same question at
//!   the same moment and Darwin answers the pid only while the connection
//!   stands.
//! * **The client** reads the endpoint's own file before it connects — a
//!   socket, not a link, owned by this user, mode `0600` — and then asks the
//!   same two questions of the connected peer the moment it has connected,
//!   before a byte of the command line is written
//!   (`crate::launch_conversation`, the order both arms speak). A folder
//!   somebody is standing in is not told to a process that is not this
//!   program, and a server that refuses this client is heard as a refusal
//!   rather than as a connection that went away.
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

use crate::instance::SOCKET_PATH_LIMIT;

/// **The most bytes one frame on this endpoint may carry** — 256 KiB (F-SWEEP-2-048).
///
/// Its own bound and not the doorbell's four kilobytes, because a launch may carry the
/// environment it was started in (`folio --with-environment`): a full environment block is a few
/// kilobytes on a plain account and tens of kilobytes on a developer's — a long `PATH`, an
/// activated virtual environment or toolchain prompt — and it crosses as JSON, which doubles every
/// backslash of a Windows path. A quarter of a mebibyte is several times the largest such block
/// and still one read into one buffer. A request past it is refused whole before it is written,
/// never cut. An earlier build reading with the doorbell's bound refuses a frame past four
/// kilobytes, and the launch that sent it opens a window of its own.
pub const MAX_FRAME_BYTES: usize = 256 * 1024;

/// **How long a second launch will wait for the running Folio before opening a
/// window of its own.** Wire policy, and the Windows module's own number for
/// the Windows module's own reasons — a person who double-clicked an icon is
/// waiting for a window, not for a syscall.
pub const HANDOVER_BUDGET: Duration = Duration::from_secs(2);

/// How long the server holds one connection open waiting for a step of the
/// conversation, how long step 5 waits for the confirmation, and how long a
/// refused peer is held until its first move ([`answer`]). A client that read
/// the reply confirms in microseconds; a client that does not is a client whose
/// launch is **not committed** (§7.59b). The same bound fits the third use
/// because a client's first move follows its connect by a few local syscalls —
/// the peer read and the image lookup — and nothing that waits. 250 ms is a
/// rationale (orders of magnitude over those steps, short beside
/// [`HANDOVER_BUDGET`]), not a measurement.
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
/// by [`LaunchPipe::retire_within`] — its door (T-DROP-DOORS, D-79): *an endpoint is
/// retired through an explicit door, not by its drop.*
///
/// The product's endpoint lives in a `static` for the process's life and is
/// never retired; the door is the road for a scope that owns one (every test,
/// anything later). A drop that finds the door was not called waits for
/// nothing: it hangs up the stop pipe, takes the name down, lets the listener
/// go and says so once ([`crate::door_skipped`]).
#[must_use = "an endpoint is retired through `LaunchPipe::retire_within`, never by its drop"]
pub struct LaunchPipe {
    name: String,
    path: PathBuf,
    /// The write end of the self-pipe the listener also waits on — this
    /// module's `SetEvent`, and the only way to wake a thread parked in `poll`.
    stop: RawFd,
    listener: Option<JoinHandle<()>>,
    /// The listener's end: its body holds the sender, so this hears `Disconnected`
    /// once the listener has left (the door's bounded wait).
    ended: std::sync::mpsc::Receiver<()>,
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
        // The listener's end, heard by the door: the sender goes with the listener's body.
        let (alive, ended) = std::sync::mpsc::channel::<()>();
        let (wake, stop) = match self_pipe() {
            Ok(pair) => pair,
            Err(error) => {
                let _ = std::fs::remove_file(&path);
                return Err(error);
            }
        };
        let listener = crate::spawn_at_priority(
            "folio-launch-endpoint",
            crate::ThreadPriority::Normal,
            move |_ctx| {
                let _alive = alive;
                listen(&listener, wake, &decide, &commit);
            },
        );
        match listener {
            Ok(listener) => Ok(Self {
                name,
                path,
                stop,
                listener: Some(listener),
                ended,
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

    /// **The door: stop listening, wait — up to `bound` — for the listener to leave,
    /// and take the name down.** Answers whether it left in time.
    ///
    /// The one road on which this endpoint waits, and its wait is bounded by the
    /// caller: the listener leaves at its next `poll`, which the hung-up stop pipe
    /// ends — after the client it is serving has had its frame or its deadline,
    /// and after the caller's own closure has returned, which no bound written
    /// here could cover. So the door waits on the listener's end (the sender its
    /// body holds goes with it) for `bound`, and a listener still standing past
    /// it is let go. Called on whatever thread owns the endpoint; nothing in the
    /// product does (its endpoint is a `static`).
    pub fn retire_within(mut self, bound: std::time::Duration) -> bool {
        self.hang_up();
        let left = match self.listener.take() {
            Some(listener) => match crate::listener_says_within(&self.ended, bound) {
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = listener.join();
                    true
                }
                Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => false,
            },
            None => true,
        };
        // **The name goes with the listener**, wherever the listener is owned
        // by something that retires it. In the product it is not: `bt-app`
        // parks the endpoint in a `OnceLock` static, and a static is never
        // dropped, so a real Folio leaves its name behind on every exit and the
        // claim's own cleanup in `crate::instance` is what clears it on the
        // next start. This line is therefore for a scope that owns one — every
        // test below, and anything that comes later — rather than the general
        // answer, which is why the general answer is under the lock and not
        // here.
        let _ = std::fs::remove_file(&self.path);
        left
    }

    /// One byte into the stop pipe and its write end closed: the listener's
    /// `poll` wakes on either. Needs no wait — this process's own pipe, whose
    /// read end the listener holds and closes on its way out.
    fn hang_up(&self) {
        let word = [1u8];
        // SAFETY: the write end is this value's own descriptor, written and
        // closed once, here, on the one road (door or drop) that reaches it.
        unsafe {
            libc::write(self.stop, word.as_ptr().cast(), 1);
            libc::close(self.stop);
        }
    }
}

impl Drop for LaunchPipe {
    /// Waits for nothing. After [`LaunchPipe::retire_within`] there is nothing left to
    /// do; without it, the stop pipe is hung up, the name taken down and the
    /// listener let go of, and the drop says so once.
    fn drop(&mut self) {
        if let Some(listener) = self.listener.take() {
            self.hang_up();
            drop(listener);
            let _ = std::fs::remove_file(&self.path);
            crate::door_skipped::say(
                "the launch endpoint (`LaunchPipe`)",
                "its retire door",
                "its listener was told to stop and let go of",
            );
        }
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
    stream: UnixStream,
    decide: &(impl Fn(&str) -> Option<Decision<T>> + ?Sized),
    commit: &(impl Fn(T) + ?Sized),
) {
    if stream.set_read_timeout(Some(STEP_DEADLINE)).is_err()
        || stream.set_write_timeout(Some(STEP_DEADLINE)).is_err()
    {
        return;
    }
    answer(Served(stream), decide, commit);
}

/// **The server's half of one conversation, on a connection it has accepted** —
/// [`serve`]'s body over any [`Accepted`], so that its order is the thing a
/// test can watch; the connection is closed when this returns, on every path.
fn answer<T>(
    mut connection: impl Accepted,
    decide: &(impl Fn(&str) -> Option<Decision<T>> + ?Sized),
    commit: &(impl Fn(T) + ?Sized),
) {
    // **Step 1 from this side.** A peer that is not this user, or not this
    // program, is not listened to and not answered — **and its end is not
    // closed before its own first move**. The client asks the same question
    // of this end the moment it has connected, and on Darwin the kernel
    // answers the process id half of it only while the connection stands
    // (`crate::peer`), so a refusal that closed at once could reach the client
    // as `NotConnected` instead of as its own refusal. The client moves —
    // writes, or hangs up — only after it has its answer, so the close waits
    // on that and on [`STEP_DEADLINE`], whichever comes first.
    if connection.vetted_peer().is_err() {
        connection.outwait_first_move();
        return;
    }
    let Ok(frame) = connection.hear() else {
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
    if connection.say(decision.reply.as_bytes()).is_err() {
        // A write that did not land is a client that has already given up and
        // gone. Returning here drops `decision` — and with it whatever
        // reservation the decision was holding.
        return;
    }
    // Step 4 happening on the other side. **That word is the commit point** —
    // a close, a timeout or anything that is not [`CONFIRM`] leaves the launch
    // un-committed, because in every one of those cases the client is about to
    // open a window of its own.
    let Ok(confirmation) = connection.hear() else {
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

/// **The server's end of one accepted connection**: the three things the client's
/// end does ([`crate::launch_conversation::Connection`]), and one more.
trait Accepted: crate::launch_conversation::Connection {
    /// Wait for the peer's first move — a byte, or its hang-up — and no longer
    /// than [`STEP_DEADLINE`]. What it wrote is not read as anything.
    fn outwait_first_move(&mut self);
}

/// **An accepted socket**, its every step bounded by [`STEP_DEADLINE`] (set by
/// [`serve`] before this is made).
struct Served(UnixStream);

impl crate::launch_conversation::Connection for Served {
    fn vetted_peer(&mut self) -> io::Result<u32> {
        vetted_peer(&self.0)
    }

    fn say(&mut self, frame: &[u8]) -> io::Result<()> {
        write_frame(&self.0, frame)
    }

    fn hear(&mut self) -> io::Result<Vec<u8>> {
        read_frame(&self.0)
    }
}

impl Accepted for Served {
    fn outwait_first_move(&mut self) {
        let mut byte = [0u8; 1];
        while let Err(error) = (&self.0).read(&mut byte) {
            if error.kind() != io::ErrorKind::Interrupted {
                break;
            }
        }
    }
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
    if request.len() > MAX_FRAME_BYTES {
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
    let stream = UnixStream::connect(path)?;
    // **The conversation is the one both arms speak**
    // (`crate::launch_conversation`): who answered is asked the moment the
    // connection stands and before a byte of the command line is written — the
    // request names a folder somebody is standing in — and [`CONFIRM`] goes
    // last.
    crate::launch_conversation::hand_over_on(Client { stream, began }, request, on_reply)
}

/// **The client's end of one connected socket**, every step bounded by what is
/// left of the budget the handover began with.
struct Client {
    stream: UnixStream,
    began: Instant,
}

impl crate::launch_conversation::Connection for Client {
    fn vetted_peer(&mut self) -> io::Result<u32> {
        vetted_peer(&self.stream)
    }

    fn say(&mut self, frame: &[u8]) -> io::Result<()> {
        self.stream.set_write_timeout(Some(remaining(self.began)))?;
        write_frame(&self.stream, frame).map_err(ran_out)
    }

    fn hear(&mut self) -> io::Result<Vec<u8>> {
        self.stream.set_read_timeout(Some(remaining(self.began)))?;
        read_frame(&self.stream).map_err(ran_out)
    }
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

/// **Is that executable a Folio?** — the Windows arm's rule (`vetted_server`),
/// and one rule for both platforms.
///
/// **The file name and not the file** (0.4.8 D3). Two installed copies of Folio
/// that share one data directory — a second `Folio.app` in another folder, the
/// copy an update restored beside one that kept running — are two files, and
/// the launch wire is keyed by the data directory, not by the bundle: a second
/// copy reaches the running one exactly as a second start of the same copy
/// does. A version difference between them is the wire's own version check's
/// to answer, as it is on Windows. What this rules out is a program that is
/// not this program, under the uid check beside it and inside the private
/// runtime folder the endpoint lives in — the door's boundary is still this
/// user, and the module's own sentence still stands: this is not a defence
/// against a hostile process running as you.
///
/// ASCII case is folded, as the Windows arm folds it: the default volume on
/// this platform does not tell the two spellings of one name apart.
fn vet_executable(theirs: &Path) -> io::Result<()> {
    let mine = std::env::current_exe()?;
    let named = |path: &Path| path.file_name().map(std::ffi::OsStr::to_ascii_lowercase);
    match (named(&mine), named(theirs)) {
        (Some(mine), Some(theirs)) if mine == theirs => Ok(()),
        _ => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "the launch endpoint is held by {}, which is not a Folio",
                theirs.display()
            ),
        )),
    }
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

/// The same question where `/proc` answers it: the link `/proc/<pid>/exe` names
/// the image, and the name [`vet_executable`] reads is the image's, not the
/// link's.
#[cfg(not(target_os = "macos"))]
fn peer_executable(pid: u32) -> io::Result<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).map_err(|_| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the other end of the launch endpoint has an image this process cannot read",
        )
    })
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
fn write_frame(mut stream: &UnixStream, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_FRAME_BYTES {
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
fn read_frame(mut stream: &UnixStream) -> io::Result<Vec<u8>> {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length > MAX_FRAME_BYTES {
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
        let directory = bt_testpath::temp_path(&format!("bt-platform-launch-{line}"));
        std::fs::create_dir_all(&directory).expect("make the data directory");
        directory
    }

    /// What one recorded server conversation did, in the order it did it.
    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Step {
        AskedWhoConnected,
        OutwaitedFirstMove,
        Heard,
        Said(String),
        Closed,
    }

    /// **An accepted connection that writes down what is done to it** — a
    /// client whose kernel answer is given, whose frames are `frames` in order,
    /// and whose close is the moment it is dropped.
    struct Recording {
        steps: std::rc::Rc<std::cell::RefCell<Vec<Step>>>,
        folio: bool,
        frames: std::collections::VecDeque<&'static str>,
    }

    impl crate::launch_conversation::Connection for Recording {
        fn vetted_peer(&mut self) -> io::Result<u32> {
            self.steps.borrow_mut().push(Step::AskedWhoConnected);
            if self.folio {
                Ok(4242)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "the launch endpoint is held by 另一个程序, which is not a Folio",
                ))
            }
        }

        fn say(&mut self, frame: &[u8]) -> io::Result<()> {
            self.steps
                .borrow_mut()
                .push(Step::Said(String::from_utf8_lossy(frame).into_owned()));
            Ok(())
        }

        fn hear(&mut self) -> io::Result<Vec<u8>> {
            self.steps.borrow_mut().push(Step::Heard);
            self.frames
                .pop_front()
                .map(|frame| frame.as_bytes().to_vec())
                .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))
        }
    }

    impl Accepted for Recording {
        fn outwait_first_move(&mut self) {
            self.steps.borrow_mut().push(Step::OutwaitedFirstMove);
        }
    }

    impl Drop for Recording {
        fn drop(&mut self) {
            self.steps.borrow_mut().push(Step::Closed);
        }
    }

    /// One server conversation with a client that is (`folio`) or is not a
    /// Folio and says the request and the confirmation: what it did, and what
    /// it committed.
    fn answered(folio: bool) -> (Vec<Step>, Vec<String>) {
        let steps = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let committed = std::cell::RefCell::new(Vec::new());
        answer(
            Recording {
                steps: std::rc::Rc::clone(&steps),
                folio,
                frames: ["打开 folder", CONFIRM].into(),
            },
            &|line: &str| {
                Some(Decision {
                    reply: format!("answer to {line}"),
                    admitted: Some(line.to_owned()),
                })
            },
            &|admitted: String| committed.borrow_mut().push(admitted),
        );
        let steps = steps.borrow().clone();
        (steps, committed.into_inner())
    }

    /// **RED (T-LAUNCH-WIRE-PEER-READ) — the server asks who connected before
    /// it hears a word, and a peer it refuses keeps its connection until its own
    /// first move.**
    ///
    /// The client asks the same question of this end at the same moment
    /// (`crate::launch_conversation`), and Darwin answers the pid half of it
    /// only while the connection stands (`crate::peer`); a refusal that closed
    /// at once could beat that question and reach the client as `NotConnected`
    /// rather than as its own refusal. Through a recording connection, so the
    /// order is what is asked on every kernel — Linux keeps the pid after a
    /// close and would never show the race on a real socket.
    ///
    /// MUTATION: return on a refused peer without `outwait_first_move` — the
    /// first conversation closes straight after the question; or move
    /// `vetted_peer` after the first `hear` — the first conversation hears the
    /// request of a peer it refuses, the second asks after a word.
    #[test]
    fn the_peer_is_read_before_any_word_is_exchanged() {
        let (steps, committed) = answered(false);
        assert_eq!(
            steps,
            vec![
                Step::AskedWhoConnected,
                Step::OutwaitedFirstMove,
                Step::Closed
            ],
            "a refused peer is asked, not listened to, and its end is closed only after its \
             first move"
        );
        assert!(committed.is_empty(), "and nothing is committed for it");

        let (steps, committed) = answered(true);
        assert_eq!(
            steps,
            vec![
                Step::AskedWhoConnected,
                Step::Heard,
                Step::Said("answer to 打开 folder".to_owned()),
                Step::Heard,
                Step::Closed,
            ],
            "a Folio's request is heard only after it has been asked who it is"
        );
        assert_eq!(committed, vec!["打开 folder".to_owned()]);
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

    /// **RED (F-SWEEP-2-048) — a request far past the doorbell's four kilobytes crosses whole.**
    ///
    /// A launch carrying its environment is tens of kilobytes; the frame bound is this endpoint's
    /// own ([`MAX_FRAME_BYTES`]).
    ///
    /// MUTATION: bound the endpoint by `attention_pipe::MAX_MESSAGE_BYTES` again and the request is
    /// refused before it is written.
    #[test]
    fn a_request_of_an_environments_size_crosses_whole() {
        let directory = scratch(line!());
        // The reply is the server's count of what it read, so it is the whole of the proof: no
        // waiting on the commit, which `a_request_crosses_the_launch_endpoint_and_is_answered` pins.
        let endpoint = LaunchPipe::start(
            &directory,
            |line| {
                Some(Decision {
                    reply: format!("{} bytes", line.len()),
                    admitted: Some(()),
                })
            },
            |(): ()| {},
        )
        .expect("open the launch endpoint");
        let request = "环境".repeat(20_000);
        let mut reply = None;
        hand_over(endpoint.name(), &request, |_, line| {
            reply = Some(line.to_owned());
        })
        .expect("hand the request over");
        assert_eq!(reply, Some(format!("{} bytes", request.len())));
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
        assert!(
            endpoint.retire_within(crate::door_skipped::PATIENCE),
            "the listener left"
        );
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
            let stream = UnixStream::connect(endpoint.name()).expect("connect to the endpoint");
            write_frame(&stream, b"one request").expect("write the request");
            let reply = read_frame(&stream).expect("read the reply");
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
        let oversized = "x".repeat(MAX_FRAME_BYTES + 1);
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

    /// **RED (0.4.8 D3) — a door held by a program that is not a Folio is not
    /// written to, and a door held by another copy of Folio is.**
    ///
    /// The peer rule, asked of the one half of it a single process can stand
    /// both sides of. This program is a Folio; so is another copy of it in
    /// another folder — another bundle, another file, the same name; a shell is
    /// not, and its refusal names it. The connected half of the rule is proved
    /// by every test above — each of them is a peer whose executable is this
    /// test binary, and each of them is served — and by `bt-app`'s
    /// `launch_wire` tests across two real copies.
    ///
    /// MUTATION: compare device and inode (this bundle, not a Folio) and the
    /// other copy is refused.
    #[test]
    fn a_program_that_is_not_a_folio_is_refused_and_another_copy_is_not() {
        let mine = std::env::current_exe().expect("this process has an image");
        vet_executable(&mine).expect("this program is this program");
        let name = mine.file_name().expect("an image has a name");
        let other = scratch(line!())
            .join("其他 副本")
            .join("Folio.app/Contents/MacOS");
        std::fs::create_dir_all(&other).expect("make the other copy's folder");
        let other = other.join(name);
        std::fs::write(&other, b"another build of the same program").expect("stand it there");
        vet_executable(&other).expect("another copy of this program is a Folio");
        let refused = vet_executable(Path::new("/bin/sh"))
            .expect_err("a shell is not a Folio, whatever it is called");
        assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied);
        assert!(
            refused.to_string().contains("/bin/sh"),
            "the refusal names what holds the door: {refused}"
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

    /// An endpoint whose listener stands inside `decide` until the test lets it
    /// go, and the flag `decide` raises on its way out. The request that holds
    /// it is handed over on a thread of its own, which gives up at
    /// [`HANDOVER_BUDGET`] whatever the test does.
    fn a_held_endpoint(
        directory: &Path,
    ) -> (
        LaunchPipe,
        mpsc::Sender<()>,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        let (entered, inside) = mpsc::channel();
        let (release, held) = mpsc::channel::<()>();
        let left = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let leaving = std::sync::Arc::clone(&left);
        let endpoint = LaunchPipe::start(
            directory,
            move |_line| {
                let _ = entered.send(());
                let _ = held.recv();
                leaving.store(true, std::sync::atomic::Ordering::SeqCst);
                None::<Decision<()>>
            },
            |()| {},
        )
        .expect("open the launch endpoint");
        let name = endpoint.name().to_owned();
        std::thread::spawn(move || {
            let _ = hand_over(&name, "hold", |_, _| {});
        });
        crate::door_skipped::received(&inside, "the listener inside `decide`");
        (endpoint, release, left)
    }

    /// RED (T-DROP-DOORS, D-79) — **the door waits for the listener to leave,
    /// takes the name down, and says no skipped door.**
    ///
    /// The listener is held inside `decide`; the door is called on a thread of
    /// its own, and the hold let go once that thread is on its way into the
    /// door. When the door returns, `decide` has returned and the name is gone.
    ///
    /// MUTATION: let the listener go in `retire_within` instead of joining it and the
    /// door returns while `decide` still stands — `left` reads false (unless the
    /// release overtakes the door, which the order below makes the rare case).
    #[test]
    fn the_door_waits_for_the_listener_to_leave() {
        let directory = scratch(line!());
        let (endpoint, release, left) = a_held_endpoint(&directory);
        let path = PathBuf::from(endpoint.name());
        let (going, on_its_way) = mpsc::channel();
        let (retired, heard) = mpsc::channel();
        let leaving = std::sync::Arc::clone(&left);
        std::thread::spawn(move || {
            let _ = going.send(());
            assert!(
                endpoint.retire_within(crate::door_skipped::PATIENCE),
                "the listener left"
            );
            let _ = retired.send((
                leaving.load(std::sync::atomic::Ordering::SeqCst),
                crate::door_skipped::skipped_on_this_thread(),
            ));
        });
        crate::door_skipped::received(&on_its_way, "the retiring thread");
        let _ = release.send(());
        let (had_left, skipped) =
            crate::door_skipped::received(&heard, "the door, once the listener is let go");
        assert!(had_left, "the door returned before the listener left");
        assert_eq!(skipped, 0, "a retired endpoint's drop says nothing");
        assert!(!path.exists(), "a retired endpoint left its name standing");
    }

    /// RED (T-DROP-DOORS, D-79) — **an endpoint dropped without its door waits
    /// for nothing, and says so once.**
    ///
    /// The listener is held inside `decide` for the whole of the drop: a drop
    /// that joined it would stand until the test let go, and the test lets go
    /// only after the drop has answered.
    ///
    /// MUTATION: put the join back in `Drop` and the drop's thread does not
    /// answer within the suite's patience; take the saying out and the count is
    /// zero.
    #[test]
    fn an_endpoint_dropped_without_its_door_waits_for_nothing_and_says_so_once() {
        let directory = scratch(line!());
        let (endpoint, release, _left) = a_held_endpoint(&directory);
        let skipped =
            crate::door_skipped::dropped_alone(endpoint, "a drop while the listener is held");
        assert_eq!(skipped, 1, "the drop says its skipped door once");
        let _ = release.send(());
    }

    /// RED (T-DROP-DOORS, D-79, the review's bound) — **the door's wait ends at its bound**: a
    /// listener held inside the caller's closure has not left within no time at all, and the door
    /// answers so and lets it go.
    ///
    /// MUTATION: join the listener in `retire_within` without the bounded wait and the door's
    /// thread does not answer within the suite's patience.
    #[test]
    fn the_door_lets_a_listener_go_past_its_bound() {
        let directory = scratch(line!());
        let (endpoint, release, _left) = a_held_endpoint(&directory);
        let (answered, heard) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = answered.send(endpoint.retire_within(Duration::ZERO));
        });
        let left = crate::door_skipped::received(&heard, "the door with no time at all");
        assert!(
            !left,
            "a listener held in its closure has not left within no time at all"
        );
        drop(release);
    }
}
