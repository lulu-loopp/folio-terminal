//! **The pane-local attention endpoint, where the kernel names doors with
//! paths** — one Unix socket per data directory, and the `folio attention`
//! verb's other end (`docs/plans/attention/plan.md` §10.6, `docs/DESIGN.md`
//! §7.1.5m and §13.37; M4-7).
//!
//! # What travels unchanged, and what could not
//!
//! **The contract is the product's and does not move.** Six clauses, the same
//! four-kilobyte frame, the same token bucket over a second, the same quarter
//! second a caller has to say its line, the same conservation law over
//! [`PipeCounts`], and the same message with **no pane coordinate in it at
//! all** — a caller says which pane it means by presenting that pane's
//! capability, and a capability it does not hold is a capability it cannot
//! name. What changes is the three things Win32 was answering along the way,
//! which is [`crate::launch_pipe`]'s own list at the other door of the same
//! runtime directory (`docs/DESIGN.md` §13.28 ⑤):
//!
//! * **A name.** A named pipe lives in a kernel namespace and a Unix socket
//!   lives in the filesystem, so the endpoint is a *path* — and a path has a
//!   length a name does not ([`crate::instance::SOCKET_PATH_LIMIT`]). The
//!   Windows name carries a logon tag, a process id and a 128-bit nonce; this
//!   one carries the **data directory's digest** and nothing else, because the
//!   name has to be a string the next holder of the claim can compute in order
//!   to unlink it. What the nonce was buying is bought here by the claim: the
//!   socket is opened only by the process that holds the data directory's
//!   `flock`, and a stale file at that name is removed under that lock before
//!   a new one is bound.
//! * **A framing.** `PIPE_TYPE_MESSAGE` made "one frame" a kernel fact. A
//!   `SOCK_STREAM` socket has no frames — but this conversation needs none: a
//!   caller connects, writes one line and closes, and there is no reply
//!   channel to keep the connection open for. So **the frame is the
//!   connection**: bytes are accumulated until the peer's half of it closes,
//!   bounded at [`MAX_MESSAGE_BYTES`] on the way in, and a connection that
//!   carries more than the bound is refused whole rather than truncated and
//!   parsed. That is the answer message mode gave, and it is why this module
//!   does not borrow the launch wire's length header — which exists there
//!   because that door is a five-step conversation on one socket.
//! * **A wake-up.** `WaitForMultipleObjects` over four pipe instances and a
//!   stop event becomes `poll` over the listener, the read end of a self-pipe
//!   and up to [`MAX_IN_FLIGHT`] accepted connections. The pool survives for
//!   the half of its reason that survives: a caller that attaches and says
//!   nothing costs **its own** slot for [`READ_DEADLINE`] rather than the
//!   endpoint's whole attention. The other half — that a pipe instance starts
//!   listening before `ConnectNamedPipe` is called, which is the defect
//!   `accepted` was added to name — has no counterpart here, because a
//!   listener's backlog holds a caller the kernel already took in for it.
//!
//! # **The boundary changes meaning, and this is the sentence about it**
//!
//! The Windows endpoint carries a descriptor written by hand,
//! `D:P(A;;GA;;;<logon sid>)` — protected, one ACE, and the principal in it is
//! the **logon session**, deliberately, "so a second session of the same user
//! (a service, another desktop) is outside it". A Unix socket carries file
//! permissions, and **file permissions name a user**. `0600` inside a `0700`
//! runtime directory is therefore a *wider* principal than the Windows door:
//! a second `ssh` login, a `launchd` agent, a fast-user-switched second
//! console of the same user — every one of them **can** post attention to this
//! Folio. That is written down as a decision rather than substituted and
//! called equivalent (`docs/plans/port/macos-plan-2026-09-12.md` §R6).
//!
//! **What the runtime directory does and does not buy.** It is `0700` and it is
//! per-uid, and on macOS `$TMPDIR` is itself `/var/folders/<xx>/<digest>/T/` —
//! which is **per user and per boot, not per session**. Every login session of
//! one user is handed the same one, so it adds nothing at all to the session
//! question, and saying that it did would be the substitution this module is
//! refusing. What it does buy is against a *different* user: the directory
//! another user would have to reach is both unguessable and unreadable, so the
//! socket's own `0600` is the second of two locks on that gate rather than the
//! only one.
//!
//! **And the sentence the Windows module ends on is unchanged**, because it is
//! the one that actually bounds this: **this is not a defence against a hostile
//! process running as you.** A capability travels in a child's environment and
//! anything that can read that environment has it. What is bounded is the blast
//! radius — the worst a stolen capability buys is a single pane's attention
//! bit, raised or lowered. It cannot type, cannot open a pane, cannot read a
//! transcript and cannot name a different pane.
//!
//! # The peer is checked for its user and **not** for its executable
//!
//! [`crate::launch_pipe`]'s server refuses a peer whose executable is not the
//! same file as its own, and that is right *there*: the only thing that ever
//! speaks the launch wire is a second Folio. **Nothing of the sort is true
//! here, and the check is deliberately absent rather than forgotten.** The
//! programs on the other end of this socket are other people's — `claude`,
//! `codex`, `node`, `copilot`, and, where a hook is spelled as a shell command,
//! `sh` and whatever it spawned. An executable check at this door would refuse
//! every real caller it has. So what is asked is the one question that means
//! something: [`crate::peer::uid`], and the uid must be this process's own. A
//! pid out of a frame would be a number the peer chose; this comes from the
//! kernel.
//!
//! **And the uid is the whole of what is asked for.** That module has a second
//! door which also answers the peer's process id, and this one deliberately
//! does not take it: the launch wire wants a pid in order to look an executable
//! up, and this door does not look one up.
//!
//! A peer of another uid is closed on without a byte read and **without a
//! count**, which is [`PipeCounts`]'s shape kept honest: on Windows such a
//! caller is refused by the descriptor and never attaches at all, so it appears
//! in none of the five numbers, and it appears in none of them here either.

use std::{
    io::{self, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        io::{AsRawFd, RawFd},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use crate::instance::SOCKET_PATH_LIMIT;

/// The longest single frame this endpoint will take, in bytes.
///
/// The product's bound and not the transport's, so it is the Windows arm's
/// number for the Windows arm's reasons: far more than the grammar needs — the
/// longest legal message is a verb, a kind, a capability and a bounded key —
/// and chosen to be *obviously* enough rather than tight, because the failure
/// of a too-tight bound is a real request silently dropped.
pub const MAX_MESSAGE_BYTES: usize = 4096;

/// **The backstop, and not the bound that protects one pane from another**
/// (R2-9).
///
/// Charged before a frame is parsed, because parsing is the caller's and this
/// module knows nothing about the grammar. The bound that tells one pane's
/// frames from another's is charged per pane, after the capability has been
/// checked, by the layer that knows which pane a capability names:
/// `bt_app::attention::AttentionLedger::admits_a_frame`.
pub const MAX_FRAMES_PER_SECOND: u32 = 512;

/// How long the endpoint will hold one connection open waiting for its one
/// line.
///
/// A caller that connects and says nothing costs its own slot and nothing more
/// — see [`MAX_IN_FLIGHT`] — but it costs that slot for this long, so the
/// number is still the bound that keeps a stalled caller from taking the
/// doorbell apart. `folio attention` writes its line and closes within a
/// millisecond of connecting; a quarter of a second is three orders of
/// magnitude of slack.
const READ_DEADLINE: Duration = Duration::from_millis(250);

/// How long the verb will spend putting one line on the wire.
///
/// The Windows arm's reasoning without the Windows arm's machinery: there a
/// bounded write needs overlapped I/O, because a write to a pipe returns when
/// the reader has taken the bytes and there is no argument that says otherwise;
/// here a socket has `SO_SNDTIMEO` and the standard library spells it
/// [`UnixStream::set_write_timeout`]. The contract is the one that matters and
/// it is unchanged — **the verb never blocks**, and "never" has to be a number
/// somewhere.
const WRITE_DEADLINE: Duration = Duration::from_millis(250);

/// How many connections the endpoint serves at once.
///
/// Four, which is [`crate::attention_pipe`]'s `MAX_INSTANCES` and is here for
/// the half of that constant's reason that survives a listener with a backlog:
/// being *served* must not mean being queued behind somebody else's read. A
/// fifth caller waits in the kernel's accept queue — a wait of microseconds
/// unless one of the four is a caller that attached and went quiet, and that
/// one is swept at [`READ_DEADLINE`].
const MAX_IN_FLIGHT: usize = 4;

/// What an endpoint has been asked, since it opened.
///
/// Counted rather than logged, and every field but the first is a **refusal**.
/// A frame this endpoint could not make sense of is dropped in silence — there
/// is no reply channel, and a message that fails to parse is by definition one
/// whose sender cannot be reasoned with — so these numbers are the only
/// evidence that it happened, and the reason they exist.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PipeCounts {
    /// Frames handed to the caller's sink.
    pub delivered: u64,
    /// Frames longer than [`MAX_MESSAGE_BYTES`].
    pub oversize: u64,
    /// Frames refused because this second's allowance was spent.
    pub throttled: u64,
    /// Connections that closed without saying anything.
    pub silent: u64,
    /// **Clients that attached**, and the conservation law this file keeps with
    /// the Windows one: every one of them becomes exactly one of the four
    /// counts above.
    ///
    /// A peer of another uid is not one of them — it is refused where a
    /// descriptor refuses it on Windows, which is before anything has attached.
    /// See the module header.
    pub accepted: u64,
}

/// A token bucket over one second, and the whole of the rate bound.
///
/// A bucket rather than a minimum gap between frames: two hooks firing in the
/// same millisecond is ordinary — a permission request and its notification
/// fallback can land together — and a minimum-gap rule would drop the second
/// one every time. A bucket lets a burst through and only refuses a *sustained*
/// flood, which is the thing worth refusing.
#[derive(Clone, Copy, Debug)]
struct RateLimit {
    window_started: Instant,
    used: u32,
}

impl RateLimit {
    fn new(now: Instant) -> Self {
        Self {
            window_started: now,
            used: 0,
        }
    }

    /// Whether one more frame may pass, charging it if so.
    fn admit(&mut self, now: Instant) -> bool {
        if now.duration_since(self.window_started) >= Duration::from_secs(1) {
            self.window_started = now;
            self.used = 0;
        }
        if self.used >= MAX_FRAMES_PER_SECOND {
            return false;
        }
        self.used += 1;
        true
    }
}

/// The path this process's attention endpoint takes for `directory`, or `None`
/// for a path this wire cannot address.
///
/// [`crate::launch_pipe::endpoint_for`]'s twin at the second door of the same
/// runtime directory, and `None` is a real answer here for the two reasons it
/// is one there: a runtime directory whose bytes are not text, so the two ends
/// could not agree on one spelling, and a path longer than a `sockaddr_un`, so
/// the kernel would not hear the whole of it. The caller's fallback is what
/// every machine had before this channel existed — no endpoint, and hooks that
/// reach nobody.
#[must_use]
pub fn endpoint_for(directory: &Path) -> Option<String> {
    let path = crate::instance::attention_socket_path(directory);
    if !crate::instance::fits_a_socket_path(&path) {
        return None;
    }
    path.to_str().map(str::to_owned)
}

/// **The endpoint.** Live from the moment [`AttentionPipe::start`] returns,
/// closed when this is dropped.
pub struct AttentionPipe {
    name: String,
    path: PathBuf,
    /// The write end of the self-pipe the listener also waits on — this
    /// module's `SetEvent`, and the only way to reach a thread parked in
    /// `poll`.
    stop: RawFd,
    listener: Option<JoinHandle<()>>,
    counts: Arc<Mutex<PipeCounts>>,
}

impl AttentionPipe {
    /// Open this process's endpoint for `directory` and start listening.
    ///
    /// **It returns already listening**, which is `docs/CONVENTIONS.md`'s rule for
    /// anything shaped like a subscription and is load-bearing here for the
    /// Windows arm's reason: the first pane's shell is spawned within a frame
    /// of this returning, and a hook that fired against an endpoint that was
    /// "about to exist" would be a signal that is not late but *gone*. Here it
    /// is a fact rather than a handshake — `bind` and `listen` both happen on
    /// this thread before the listener thread is spawned, so there is no window
    /// at all, where the Windows arm needs a channel because
    /// `CreateNamedPipeW` runs on the listener.
    ///
    /// **`directory` is the data directory whose claim this process holds**
    /// ([`crate::instance::claim_data_directory`]), and the claim is what makes
    /// the name safe to take: the socket is unlinked by the next holder of that
    /// lock and by nobody else, so binding over a name this process had not
    /// cleared would be one Folio taking a running Folio's doorbell away. This
    /// does not re-take the claim — a `flock` is held by an open file
    /// description, so asking for it a second time from one process would be
    /// refused by the kernel and would prove the opposite of what it looks like
    /// — it refuses a name that is already bound.
    ///
    /// **The parameter is spare on Windows and stays in the signature**
    /// (`docs/plans/port/macos-plan-2026-09-12.md` §4.4 ②, and
    /// [`crate::handoff`]'s own rule for the window it does not need): a door in
    /// this crate has one signature on every platform, and
    /// `bt_app::attention_wire` names this one with no `cfg` anywhere near it.
    ///
    /// `deliver` is called **on the listener thread**, once per accepted frame,
    /// and is expected to do nothing but park the line and nudge the loop that
    /// will act on it. It is handed the raw bytes as a `String` and no more:
    /// this module knows nothing about the grammar inside, which is what keeps
    /// a parser change out of the transport.
    ///
    /// The frame's content is deliberately *not* validated here beyond its
    /// length. In particular there is no pane coordinate to check, because
    /// there is none in the format.
    pub fn start(directory: &Path, deliver: impl Fn(String) + Send + 'static) -> io::Result<Self> {
        let path = crate::instance::attention_socket_path(directory);
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
        // world-writable doorbell up. The directory above is `0700` either way,
        // so this is the second of two locks on the same gate rather than the
        // only one.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let (wake, stop) = match self_pipe() {
            Ok(pair) => pair,
            Err(error) => {
                let _ = std::fs::remove_file(&path);
                return Err(error);
            }
        };
        let counts = Arc::new(Mutex::new(PipeCounts::default()));
        let started = {
            let counts = Arc::clone(&counts);
            std::thread::Builder::new()
                .name("folio-attention-endpoint".to_owned())
                .spawn(move || {
                    listen(&listener, wake, &counts, &deliver);
                })
        };
        match started {
            Ok(listener) => Ok(Self {
                name,
                path,
                stop,
                listener: Some(listener),
                counts,
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

    /// The name a child is told to write to.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What this endpoint has been asked since it opened.
    #[must_use]
    pub fn counts(&self) -> PipeCounts {
        *self.counts.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for AttentionPipe {
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
        // **The name goes with the listener**, wherever the listener is owned by
        // something that is dropped. In the product it is not: `bt-app` parks
        // the endpoint in a `OnceLock` static, a static is never dropped, and so
        // a real Folio leaves its name behind on every exit — which is
        // [`crate::launch_pipe`]'s own note at the other door, and why the
        // general answer is the unlink under the claim's lock and not this line.
        // This one is for a scope that owns one: every test below, and anything
        // that comes later.
        let _ = std::fs::remove_file(&self.path);
    }
}

/// One connection in flight: the socket, what has arrived on it, and when it
/// started running out of time.
struct Client {
    stream: UnixStream,
    buffer: Vec<u8>,
    since: Instant,
}

/// What one connection turned out to be.
///
/// Three outcomes, and with [`PipeCounts::throttled`] splitting the first they
/// are the four the conservation law names — written as a type rather than as a
/// comment.
enum Frame {
    /// A line, whole, inside the bound.
    Line(Vec<u8>),
    /// More bytes than [`MAX_MESSAGE_BYTES`] — refused whole, never truncated
    /// and parsed.
    Oversize,
    /// A connection that closed, or ran out of time, without saying anything.
    Silent,
}

/// Four connections at a time, for as long as the endpoint is open.
fn listen(
    listener: &UnixListener,
    wake: RawFd,
    counts: &Mutex<PipeCounts>,
    deliver: &(impl Fn(String) + Send + ?Sized),
) {
    let mut rate = RateLimit::new(Instant::now());
    let mut clients: Vec<Client> = Vec::new();
    loop {
        let now = Instant::now();
        let timeout = clients
            .iter()
            .map(|client| READ_DEADLINE.saturating_sub(now.duration_since(client.since)))
            .min()
            .map_or(-1, |left| {
                i32::try_from(left.as_millis()).unwrap_or(i32::MAX).max(1)
            });
        let mut waiting = vec![
            libc::pollfd {
                fd: wake,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                // **The listener leaves the set when the pool is full**, which
                // is what makes a fifth caller wait in the kernel's backlog
                // rather than be taken into a slot that does not exist. A
                // negative descriptor is `poll`'s own spelling for "ignore this
                // entry", and it keeps the two indices below fixed.
                fd: if clients.len() < MAX_IN_FLIGHT {
                    listener.as_raw_fd()
                } else {
                    -1
                },
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        waiting.extend(clients.iter().map(|client| libc::pollfd {
            fd: client.stream.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        }));
        let count = libc::nfds_t::try_from(waiting.len()).unwrap_or(0);
        // SAFETY: every descriptor is open for the whole of this call — the
        // listener is borrowed, the clients are owned by the vector above, and
        // the read end is closed only after this loop ends.
        let ready = unsafe { libc::poll(waiting.as_mut_ptr(), count, timeout) };
        if ready < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        // **The stop word is read first.** A `drop` that raced an incoming
        // connection must end the thread rather than serve one more caller
        // whose line would be parked in an inbox nobody is going to read.
        if waiting[0].revents != 0 {
            break;
        }
        if waiting[1].revents & libc::POLLIN != 0 {
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Some(client) = admit(stream) {
                        note_accepted(counts);
                        clients.push(client);
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                    ) => {}
                Err(_) => break,
            }
        }
        // **Walked backwards, because serving a client takes it out of the
        // pool.** Every other order would move the next one's index out from
        // under the walk, and the entry in `waiting` that belongs to it with
        // it — the two lists are parallel, and the pairing is what says which
        // descriptor the kernel was talking about.
        let mut index = clients.len();
        while index > 0 {
            index -= 1;
            let readable = waiting
                .get(index + 2)
                .is_some_and(|entry| entry.revents != 0);
            let outcome = if readable {
                collect(&mut clients[index])
            } else if Instant::now().duration_since(clients[index].since) >= READ_DEADLINE {
                Some(Frame::Silent)
            } else {
                None
            };
            let Some(frame) = outcome else {
                continue;
            };
            clients.remove(index);
            account(frame, &mut rate, counts, deliver);
        }
    }
    // SAFETY: this thread is the only owner of the read end, and it is ending.
    unsafe {
        libc::close(wake);
    }
}

/// **Whose connection is this?** — the one question this door asks its peer.
///
/// `None` closes on it without a byte read and without a count, which is the
/// module header's rule: a caller a descriptor would have refused on Windows
/// never attached there, and does not attach here.
fn admit(stream: UnixStream) -> Option<Client> {
    if stream.set_nonblocking(true).is_err() {
        return None;
    }
    if !peer_is_this_user(crate::peer::uid(&stream).ok()?) {
        return None;
    }
    Some(Client {
        stream,
        buffer: Vec::new(),
        since: Instant::now(),
    })
}

/// One more client through the door, for the conservation law in
/// [`PipeCounts::accepted`].
fn note_accepted(counts: &Mutex<PipeCounts>) {
    counts
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .accepted += 1;
}

/// Read whatever has arrived, and say whether this connection is finished.
///
/// **The frame is the connection**, so `Ok(0)` — the peer's half of it closing
/// — is the terminator, and it is the only thing that turns accumulated bytes
/// into a line. A read that would block leaves the client in the pool with its
/// deadline running.
fn collect(client: &mut Client) -> Option<Frame> {
    let mut chunk = [0u8; 1024];
    loop {
        match client.stream.read(&mut chunk) {
            Ok(0) => return Some(finished(client)),
            Ok(read) => {
                // **Refused whole rather than truncated and parsed**, which is
                // what message mode bought on the other platform: a half-read
                // line is exactly the kind of thing a parser should never be
                // handed.
                if client.buffer.len() + read > MAX_MESSAGE_BYTES {
                    return Some(Frame::Oversize);
                }
                client.buffer.extend_from_slice(&chunk[..read]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return None,
            // A connection that failed mid-read is a caller that went away.
            // Whatever it had already said is still what it said.
            Err(_) => return Some(finished(client)),
        }
    }
}

/// What a connection that is over turned out to have carried.
fn finished(client: &mut Client) -> Frame {
    if client.buffer.is_empty() {
        Frame::Silent
    } else {
        Frame::Line(std::mem::take(&mut client.buffer))
    }
}

/// Charge one finished connection to exactly one counter, and deliver the one
/// outcome that has somewhere to go.
fn account(
    frame: Frame,
    rate: &mut RateLimit,
    counts: &Mutex<PipeCounts>,
    deliver: &(impl Fn(String) + Send + ?Sized),
) {
    match frame {
        Frame::Line(bytes) => {
            let mut counts = counts.lock().unwrap_or_else(PoisonError::into_inner);
            if rate.admit(Instant::now()) {
                counts.delivered += 1;
                drop(counts);
                deliver(String::from_utf8_lossy(&bytes).into_owned());
            } else {
                counts.throttled += 1;
            }
        }
        Frame::Oversize => {
            counts
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .oversize += 1;
        }
        Frame::Silent => {
            counts.lock().unwrap_or_else(PoisonError::into_inner).silent += 1;
        }
    }
}

/// **`folio attention`'s whole conversation**: connect, write one line, close.
///
/// Bounded end to end, and there is no reply to read — this endpoint has no
/// reply channel, exactly as the Windows pipe is inbound-only. A verb that
/// waited for an acknowledgement would be a verb that could hang on a window
/// that is busy painting, which is precisely the moment a hook is most likely
/// to fire.
///
/// **The close is the frame's end and is written out** rather than left to the
/// drop: the server reads until the peer's half of the connection goes, so the
/// shutdown is a statement of the protocol and not a tidying-up.
///
/// There is no busy wait here and no `CLIENT_BUSY_WAIT_MS`, because there is
/// nothing to be busy: a named pipe serves one client per instance and can
/// answer `ERROR_PIPE_BUSY`, while a listener has a backlog and `connect`
/// succeeds against a live endpoint whatever it is in the middle of. The
/// deadline that is left is the one on the write.
pub fn send_line(endpoint: &str, line: &str) -> io::Result<()> {
    if line.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "message longer than the endpoint's frame bound",
        ));
    }
    // **The name is checked before it is opened** (R2-27). It arrives in this
    // process's own environment, which anything that spawned this process wrote,
    // and `connect` addresses whatever it is handed. What this refuses is not an
    // impersonation of our endpoint, which a name cannot prevent; it is a
    // *different kind of object* being written a capability line by a verb that
    // thought it was ringing a doorbell.
    if !names_an_endpoint(endpoint) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the endpoint variable does not name a Folio attention socket",
        ));
    }
    let path = Path::new(endpoint);
    // **The door is read before it is opened**, [`crate::launch_pipe`]'s rule at
    // the other door of the same directory: a link standing where the endpoint
    // should be is somebody redirecting this line, and following it to find out
    // where would be taking their word for it.
    vetted_endpoint(path)?;
    let mut stream = UnixStream::connect(path)?;
    stream.set_write_timeout(Some(WRITE_DEADLINE))?;
    stream.write_all(line.as_bytes())?;
    stream.shutdown(std::net::Shutdown::Write)
}

/// **Whether a string is the name of one of this build's attention endpoints**
/// — see [`endpoint_for`], which is the only thing that writes one.
///
/// The grammar and not merely the shape, which is the Windows arm's rule for
/// the Windows arm's reason: a name that got here cannot address anything
/// outside this user's own runtime directory whatever else is wrong with it.
/// Three questions — the directory is this process's own
/// [`crate::instance::runtime_directory`], the file name is a hexadecimal
/// digest with this wire's suffix, and the whole path fits a `sockaddr_un`. A
/// `..` cannot survive the first of them, because a parent with one in it is not
/// the runtime directory's spelling.
#[must_use]
pub fn names_an_endpoint(name: &str) -> bool {
    let path = Path::new(name);
    if !crate::instance::fits_a_socket_path(path) {
        return false;
    }
    if path.parent() != Some(crate::instance::runtime_directory().as_path()) {
        return false;
    }
    let Some(file) = path.file_name().and_then(|file| file.to_str()) else {
        return false;
    };
    let Some(tag) = file.strip_suffix(crate::instance::ATTENTION_SOCKET_SUFFIX) else {
        return false;
    };
    !tag.is_empty() && tag.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// **The endpoint's own file, read before anybody connects to it.**
///
/// `symlink_metadata` rather than `metadata` is the first of the questions, for
/// the reason [`crate::launch_pipe`] gives at the other door: a link standing at
/// the path is somebody redirecting this, and following it to find out where is
/// taking their word for it.
fn vetted_endpoint(path: &Path) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    let refuse =
        |reason: &'static str| Err(io::Error::new(io::ErrorKind::PermissionDenied, reason));
    if metadata.file_type().is_symlink() || !metadata.file_type().is_socket() {
        return refuse("what is standing at the attention endpoint's path is not a socket");
    }
    if !peer_is_this_user(metadata.uid()) {
        return refuse("the attention endpoint belongs to another user");
    }
    if metadata.permissions().mode() & 0o777 != 0o600 {
        return refuse("the attention endpoint is reachable by somebody who is not its owner");
    }
    Ok(())
}

/// **Whether a uid is the one this endpoint serves** — the whole of the
/// boundary, in one comparison.
///
/// Split out rather than written twice because it is asked at both ends, and
/// because it is the only half of the boundary a test on one machine can reach:
/// a test process cannot become a second user, so the *refusal* is proved
/// against this predicate and the *admission* is proved by every case that
/// connects, each of which is a peer whose uid really is this process's own.
/// That is `docs/DESIGN.md` §13.28's own written-down gap at the other door of
/// this directory, kept in the same words rather than quietly widened.
#[must_use]
pub(crate) fn peer_is_this_user(uid: u32) -> bool {
    // SAFETY: `geteuid` reads this process's own credentials and cannot fail.
    uid == unsafe { libc::geteuid() }
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

/// **Bits nobody can guess**, and this one is real on every platform.
///
/// The capability a pane hands its children carries 128 unpredictable bits, so
/// that a second process cannot compute a pane's capability. That is arithmetic
/// and entropy rather than a platform question, and `getrandom` is in the
/// standard library's own hasher: `RandomState` seeds itself from the operating
/// system, and two of its hashes of a fixed key are a hundred and twenty-eight
/// bits nobody can predict without the seed.
///
/// **Not a cryptographic generator**, and it does not need to be: what the bits
/// buy is that another process of the same user cannot *guess* a capability, and
/// the security boundary itself is the socket's own permissions — which is this
/// module's subject and §R6's warning.
#[must_use]
pub fn unguessable_bits() -> u128 {
    use std::hash::{BuildHasher, Hasher};

    let mut high = std::collections::hash_map::RandomState::new().build_hasher();
    high.write_u8(0);
    let mut low = std::collections::hash_map::RandomState::new().build_hasher();
    low.write_u8(1);
    (u128::from(high.finish()) << 64) | u128::from(low.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// A data directory no other test in this process is using, so two of them
    /// can run at once — and one that really exists, because the endpoint's name
    /// is the filesystem's answer about it.
    fn scratch(line: u32) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "bt-platform-attention-{}-{line}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("make the data directory");
        directory
    }

    /// Everything `folio attention` does, without this process's own
    /// environment: one connection, one line, closed.
    fn post(endpoint: &str, line: &str) {
        send_line(endpoint, line).expect("the endpoint took the line");
    }

    /// **RED — a line crosses the endpoint and arrives as the same value.**
    ///
    /// End to end against a real socket, because what is worth pinning is a
    /// property of the real one: that a client can connect the instant `start`
    /// hands back, and that the bytes reach the sink whole.
    ///
    /// MUTATION: deliver on the first read rather than on the peer's close and a
    /// line split across two writes arrives in halves.
    #[test]
    fn a_line_crosses_the_endpoint_and_the_same_value_arrives() {
        let directory = scratch(line!());
        let (sender, heard) = mpsc::channel();
        let endpoint = AttentionPipe::start(&directory, move |line| {
            let _ = sender.send(line);
        })
        .expect("open the attention endpoint");
        post(
            endpoint.name(),
            "raise claude-code:PermissionRequest cap=abc",
        );
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).ok().as_deref(),
            Some("raise claude-code:PermissionRequest cap=abc")
        );
        assert_eq!(
            endpoint.counts().delivered,
            1,
            "the frame is counted where it was delivered"
        );
    }

    /// **RED — every client that attaches becomes exactly one of the four
    /// counts.**
    ///
    /// The Windows module's conservation law, asserted here for the same reason
    /// it is asserted there: it does not know what the next such defect will be,
    /// only that the arithmetic has to come out. A connection that says nothing
    /// and one that says too much are the two refusals a single process can
    /// stand both sides of.
    #[test]
    fn every_client_that_attaches_is_accounted_for() {
        let directory = scratch(line!());
        let (sender, heard) = mpsc::channel();
        let endpoint = AttentionPipe::start(&directory, move |line| {
            let _ = sender.send(line);
        })
        .expect("open the attention endpoint");
        post(endpoint.name(), "one");
        assert!(heard.recv_timeout(Duration::from_secs(5)).is_ok());

        // A caller that connects and closes without a word.
        drop(UnixStream::connect(endpoint.name()).expect("connect and say nothing"));

        // A caller that says more than the bound. `send_line` refuses this
        // before a socket is touched, so the oversize frame is written by hand —
        // which is the case the endpoint has to survive, because the bound on
        // the far end is not a bound this end can rely on.
        {
            let mut stream = UnixStream::connect(endpoint.name()).expect("connect");
            let _ = stream.write_all(&vec![b'x'; MAX_MESSAGE_BYTES + 1]);
            let _ = stream.shutdown(std::net::Shutdown::Write);
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        let counts = loop {
            let counts = endpoint.counts();
            if counts.accepted >= 3 && counts.silent >= 1 && counts.oversize >= 1 {
                break counts;
            }
            assert!(
                Instant::now() < deadline,
                "the endpoint lost a caller: {counts:?}"
            );
            std::thread::yield_now();
        };
        assert_eq!(
            counts.accepted,
            counts.delivered + counts.oversize + counts.throttled + counts.silent,
            "a client attached and became none of the four: {counts:?}"
        );
    }

    /// **RED — the endpoint is `0600` inside a `0700` directory, and it is gone
    /// when its owner is.**
    ///
    /// The whole of this door's boundary is two mode bits and a directory, so
    /// they are measured rather than assumed.
    #[test]
    fn the_endpoint_is_private_to_this_user_and_goes_when_the_listener_does() {
        let directory = scratch(line!());
        let endpoint = AttentionPipe::start(&directory, |_| {}).expect("open the endpoint");
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
            "a clean quit left its doorbell standing: {}",
            path.display()
        );
    }

    /// **RED — a peer that is not this user is refused.**
    ///
    /// The predicate, which is the half of the boundary a test on one machine
    /// can reach: a test process cannot become a second user. The connected half
    /// is exercised by every other case here — each is a peer whose uid really
    /// is this process's own, and each is served.
    ///
    /// MUTATION: drop the peer's-user check out of `admit` and the second
    /// assertion stays green while the first stops meaning anything, which is
    /// why the source pin in `lib.rs` stands beside this.
    #[test]
    fn a_peer_of_another_user_is_not_this_endpoints_caller() {
        // SAFETY: `geteuid` reads this process's own credentials and cannot
        // fail.
        let mine = unsafe { libc::geteuid() };
        assert!(peer_is_this_user(mine), "this process is this user");
        assert!(
            !peer_is_this_user(mine.wrapping_add(1)),
            "another uid is another principal, whatever else it can reach"
        );
    }

    /// **RED — a stale socket left by a crash is removed by the next holder of
    /// the claim, and the endpoint binds over it.**
    ///
    /// The name outlives the process that bound it, and a client that connects
    /// to a name nobody is listening on is *refused* rather than told there is
    /// nobody home. Unlinking it is safe on exactly one condition — that nobody
    /// is listening — and holding the claim is what makes that true.
    ///
    /// MUTATION: move the `remove_file` in `instance::claim_data_directory`
    /// above the `flock` and this still passes, which is why
    /// `instance`'s own source pin asserts the order; take the unlink out
    /// altogether and `start` fails with `AddrInUse` here.
    #[test]
    fn a_stale_socket_is_cleared_by_the_next_holder_of_the_claim() {
        let directory = scratch(line!());
        let path = crate::instance::attention_socket_path(&directory);
        crate::instance::prepare_runtime_directory().expect("the runtime directory is prepared");
        let _ = std::fs::remove_file(&path);
        // What a crashed holder leaves: a socket file with nobody behind it.
        drop(UnixListener::bind(&path).expect("stand a socket there"));
        assert!(
            std::fs::symlink_metadata(&path).is_ok(),
            "the stale name is standing"
        );
        let claim =
            crate::instance::claim_data_directory(&directory).expect("this process is the writer");
        assert!(
            std::fs::symlink_metadata(&path).is_err(),
            "the claim left a stale doorbell standing: {}",
            path.display()
        );
        let endpoint =
            AttentionPipe::start(&directory, |_| {}).expect("open over the cleared name");
        assert_eq!(endpoint.name(), path.to_str().expect("a text path"));
        drop(endpoint);
        drop(claim);
    }

    /// **RED — a name that is not one of this build's endpoints is not connected
    /// to.**
    ///
    /// The grammar, and what it refuses is the shape of a redirection: a
    /// capability line written into somebody else's object by a verb that read
    /// its endpoint out of an environment somebody else wrote.
    #[test]
    fn only_this_users_runtime_directory_can_be_addressed() {
        let directory = scratch(line!());
        let real = crate::instance::attention_socket_path(&directory);
        assert!(names_an_endpoint(real.to_str().expect("a text path")));
        for forged in [
            "/tmp/folio.sock",
            "/etc/passwd",
            "folio.attn.sock",
            "",
            "/tmp/../etc/x.attn.sock",
        ] {
            assert!(!names_an_endpoint(forged), "{forged} is not an endpoint");
        }
        let outside = crate::instance::runtime_directory().join("zzzz.sock");
        assert!(
            !names_an_endpoint(outside.to_str().expect("a text path")),
            "the suffix is part of the grammar"
        );
        let refused = send_line("/tmp/folio.sock", "raise")
            .expect_err("a name outside the grammar never reaches a socket");
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);
    }

    /// **RED — a door standing at the endpoint's path that is not a private
    /// socket of this user's is not written to.**
    ///
    /// What this refuses is the shape of an attack the Windows descriptor
    /// refused in the kernel: something else standing where the endpoint should
    /// be. A regular file and a symlink are the two cheapest ways to do it.
    #[test]
    fn the_door_is_read_before_a_capability_is_written_to_it() {
        let directory = scratch(line!());
        let path = crate::instance::attention_socket_path(&directory);
        crate::instance::prepare_runtime_directory().expect("the runtime directory is prepared");
        let _ = std::fs::remove_file(&path);

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

    /// **RED — a message past the frame bound never leaves this process.**
    #[test]
    fn a_message_past_the_frame_bound_is_refused_before_it_is_written() {
        let oversized = "x".repeat(MAX_MESSAGE_BYTES + 1);
        let directory = scratch(line!());
        let name = endpoint_for(&directory).expect("the name is addressable");
        let refused =
            send_line(&name, &oversized).expect_err("an oversized message is refused here");
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);
    }

    /// **PIN — two spellings of one data directory address one doorbell, and the
    /// doorbell is not the launch door.**
    ///
    /// The isolation promise at the second door: a run under an isolated data
    /// directory must miss the reader's everyday Folio here as well, and the two
    /// doors of one directory must be two names — a socket cannot be bound
    /// twice.
    #[test]
    fn one_directory_is_one_doorbell_however_it_is_spelled() {
        let root = scratch(line!());
        let real = root.join("real");
        std::fs::create_dir_all(&real).expect("make the data directory");
        let link = root.join("link");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&real, &link).expect("point a link at it");

        assert_eq!(
            endpoint_for(&real),
            endpoint_for(&link),
            "two spellings of one directory are one doorbell"
        );
        assert_ne!(
            endpoint_for(&real),
            endpoint_for(&root),
            "and a run under an isolated data directory finds a different one"
        );
        assert_ne!(
            endpoint_for(&real).expect("addressable"),
            crate::instance::claim_name(&real),
            "the doorbell and the launch door are two names in one directory"
        );
    }
}
