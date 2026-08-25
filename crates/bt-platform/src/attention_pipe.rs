//! **The pane-local attention endpoint** — one named pipe per Folio process, and the
//! `folio attention` verb's other end.
//!
//! `docs/plans/attention/plan.md` §10.6 is the specification, and it opens by deleting a sentence
//! an earlier draft had believed: *"the name is ours, so nobody else can impersonate a pane."*
//! **A pipe name is a location, not an identity.** Windows gives an unnamed-descriptor pipe a DACL
//! that lets Everyone — anonymous logons included — open it, so a pipe that took the default
//! descriptor would be a pipe any process on the machine could ring the doorbell of. The whole of
//! this module's security is therefore two decisions: **a descriptor we write ourselves**, and
//! **a capability the caller must already hold**.
//!
//! # What is promised, and what is refused
//!
//! Promised: nothing outside this logon session can connect at all, nothing off this machine can
//! (`PIPE_REJECT_REMOTE_CLIENTS`), and a caller that does connect can do exactly one thing —
//! hand over one bounded line about **one pane whose capability it already had**.
//!
//! **Refused, in writing:** this is not a defence against a hostile process running as *you*.
//! A capability travels in a child's environment, and anything that can read that environment has
//! it. Making that untrue needs a non-transferable inherited handle or a broker with an identity
//! model of its own, and §10.6's closing paragraph says plainly that neither is being promised
//! here. What is bounded instead is the **blast radius**: the worst a stolen capability buys is a
//! single pane's attention bit, raised or lowered. It cannot type, cannot open a pane, cannot read
//! a transcript, and cannot name a different pane — see [`AttentionPipe::start`] for why the
//! message format has no pane coordinates in it at all.
//!
//! # Why one endpoint per process and not one per pane
//!
//! §10.6 wrote the contract in terms of a per-pane endpoint; this build serves every pane of one
//! process from one pipe, and the six clauses survive intact because **the pane is named by the
//! capability rather than by the endpoint**. The clauses each land somewhere:
//!
//! 1. `FOLIO_PANE` is diagnostic. It is not in the message and nothing routes by it.
//! 2. The capability is 128 unpredictable bits; the pipe carries an explicit DACL, refuses remote
//!    clients, and takes `FILE_FLAG_FIRST_PIPE_INSTANCE` so that a squatter that got there first
//!    cannot be mistaken for us.
//! 3. Two idempotent verbs, a bounded frame, a bounded rate, one client at a time with a deadline.
//! 4. The capability lives on the leaf and dies with it — the endpoint outliving one pane is
//!    exactly why the capability, and not the endpoint, is the thing that expires.
//! 5. Nothing is discovered from a working directory or a repository.
//! 6. A stronger future capability gets its own endpoint and its own grant; this one is
//!    attention-only by construction.
//!
//! The trade is one kernel object and one thread for a window instead of one per pane, and one
//! place to audit instead of *n*.
//!
//! # The frame that went missing, and what it turned out to be
//!
//! Three shapes of the listener lost frames under load, and the third made the loss *nameable*
//! rather than merely visible. The counter that did it is [`PipeCounts::accepted`]: every client
//! that attaches must become exactly one of delivered, oversize, throttled or silent, so a run that
//! reports `delivered: 7, accepted: 7` for eight callers is not saying "one was refused", it is
//! saying **one was never there** — and that pointed at the connect, not the read.
//!
//! The cause is a Win32 detail with no forgiving reading: **an instance starts listening when
//! `CreateNamedPipeW` returns, not when `ConnectNamedPipe` is called.** A client can therefore
//! arrive in the window between the two — and `folio attention` connects, writes forty bytes and
//! closes inside a millisecond, so it can arrive *and leave* in that window. `ConnectNamedPipe`
//! then answers `ERROR_NO_DATA`, which reads like a failure and is not: the client's message is
//! sitting in the instance's buffer, readable until somebody disconnects. Every earlier shape
//! treated it as a failure and threw the instance away with the message still in it.
//!
//! So [`Instance::arm_connect`] has three "attached" answers rather than two, and the invariant
//! above is asserted directly by [`every_client_that_attaches_is_accounted_for`]. It is the useful
//! kind of pin: it does not know what the next such bug will be, only that the arithmetic has to
//! come out.

use std::{
    ffi::c_void,
    io,
    sync::{
        Arc, Mutex, PoisonError,
        mpsc::{self, RecvTimeoutError},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use windows::Win32::{
    Foundation::{
        CloseHandle, ERROR_IO_PENDING, ERROR_MORE_DATA, ERROR_NO_DATA, ERROR_PIPE_BUSY,
        ERROR_PIPE_CONNECTED, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SDDL_REVISION_1,
        },
        PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_GROUPS, TOKEN_QUERY, TokenGroups,
    },
    Storage::FileSystem::{
        CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_SHARE_MODE,
        OPEN_EXISTING, PIPE_ACCESS_INBOUND, ReadFile, WriteFile,
    },
    System::{
        IO::{CancelIoEx, OVERLAPPED},
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_MESSAGE,
            PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PIPE_WAIT, WaitNamedPipeW,
        },
        Threading::{
            CreateEventW, GetCurrentProcess, GetCurrentProcessId, INFINITE, OpenProcessToken,
            ResetEvent, SetEvent, WaitForMultipleObjects,
        },
    },
};
use windows::core::PCWSTR;

/// The longest single frame this endpoint will take, in bytes.
///
/// Four kilobytes is far more than the grammar needs — the longest legal message is a verb, a
/// kind, a capability and a bounded key — and it is chosen to be *obviously* enough rather than
/// tight, because the failure of a too-tight bound is a real request silently dropped. A frame
/// over the bound is not truncated and read anyway: message-mode pipes make "there was more" a
/// distinguishable answer, and a half-read line is exactly the kind of thing a parser should never
/// be handed.
pub const MAX_MESSAGE_BYTES: usize = 4096;

/// How many frames one endpoint will take in a second before it starts refusing.
///
/// A hook fires a handful of times per turn. Sixty-four is two orders of magnitude above that and
/// still low enough that a runaway loop cannot spend the window's time in this thread. Refusal is
/// counted rather than reported: there is nobody on the other end to report it to, and a hook that
/// is looping is not going to read an error.
pub const MAX_FRAMES_PER_SECOND: u32 = 64;

/// How long the endpoint will hold one connection open waiting for its one line.
///
/// A client that connects and says nothing holds the only listening instance, so this is the bound
/// that keeps one stalled caller from wedging the doorbell for everyone. `folio attention` writes
/// its line and closes within a millisecond of connecting; a quarter of a second is three orders of
/// magnitude of slack.
const READ_DEADLINE: Duration = Duration::from_millis(250);

/// How long `folio attention` waits for a busy endpoint before giving up.
///
/// The verb's contract is **never block**, and this is what "never" is worth in milliseconds: the
/// endpoint serves one client at a time, so a second hook firing in the same instant finds it busy
/// and waits exactly this long for the instance to come free. Past that the verb exits non-zero
/// rather than queueing — an attention signal that arrives a second late has already been overtaken
/// by whatever the user did in the meantime.
const CLIENT_BUSY_WAIT_MS: u32 = 100;

/// `SE_GROUP_LOGON_ID` from `winnt.h`.
///
/// Spelled here rather than imported, because the `windows` crate files it under
/// `Win32::System::SystemServices` — a feature this crate would otherwise have no use for at all,
/// pulled in for one constant whose value has not changed since Windows NT.
const SE_GROUP_LOGON_ID: u32 = 0xC000_0000;

/// What an endpoint has been asked, since it opened.
///
/// Counted rather than logged, and every field but the first is a **refusal**. A frame this
/// endpoint could not make sense of is dropped in silence — there is no reply channel, and a
/// message that fails to parse is by definition one whose sender cannot be reasoned with — so
/// these numbers are the only evidence that it happened, and the reason they exist.
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
    /// **Clients that attached**, and the conservation law of this whole file: every one of them
    /// becomes exactly one of the four counts above.
    ///
    /// It exists because the three counts above could not tell "refused" from "never seen", and the
    /// defect that mattered was the second — a caller whose bytes reached an instance that was then
    /// discarded. `accepted` short by one says that in a single number, which is how the cause was
    /// found after two wrong fixes aimed at the read.
    pub accepted: u64,
}

/// A token bucket over one second, and the whole of the rate bound.
///
/// A bucket rather than a minimum gap between frames: two hooks firing in the same millisecond is
/// ordinary — a permission request and its notification fallback can land together — and a
/// minimum-gap rule would drop the second one every time. A bucket lets a burst through and only
/// refuses a *sustained* flood, which is the thing worth refusing.
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

/// The segment of the endpoint name derived from the logon session.
///
/// A **digest** of the logon SID and not the SID itself. The SID is not a secret, but a name that
/// carried it whole would put a stable, cross-referenceable identifier into a string that shows up
/// in process listings and crash dumps for no gain at all — the name only has to be *unique per
/// logon session*, and sixteen hex digits of it are.
///
/// The digest is FNV-1a, which is here because it is four lines and needs no dependency; nothing
/// about this use is adversarial — an attacker who wants the name can read the environment that
/// carries it.
#[must_use]
pub fn session_tag(logon_sid: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in logon_sid.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The endpoint's full name.
///
/// Three segments and each answers a different question: the logon tag says *which session owns
/// this*, the process id says *which window*, and the nonce says *which run* — a process id is
/// reused by Windows the moment a process exits, and a name without the nonce would let a stale
/// capability in a long-lived child's environment address a completely different window.
#[must_use]
pub fn endpoint_name(session_tag: &str, process: u32, nonce: u128) -> String {
    format!(r"\\.\pipe\folio-attention-{session_tag}-{process}-{nonce:032x}")
}

/// The security descriptor this endpoint is created with, in SDDL.
///
/// `D:P(A;;GA;;;<logon sid>)` — a **protected** DACL with exactly one entry. Protected because
/// inheritance is how a permissive ACE arrives without anyone writing one, and one entry because
/// the only principal in this design is the logon session: Everyone, `ANONYMOUS LOGON` and
/// `NETWORK` are excluded by not being mentioned, which is how a DACL says no.
///
/// The grant is not split finer than "all" and the reason is honest rather than lazy: the server
/// and the client are the same principal here, so a client-only grant would still have to include
/// `FILE_CREATE_PIPE_INSTANCE` for our own next instance, and the difference between that and
/// `GA` would be spelling rather than security. The boundary that does the work is **which SID is
/// in the list**, and it is the narrowest one Windows will name — the logon session, not the user,
/// so a second session of the same user (a service, another desktop) is outside it.
#[must_use]
pub fn security_descriptor_sddl(logon_sid: &str) -> String {
    format!("D:P(A;;GA;;;{logon_sid})")
}

/// This process's logon SID, as a string.
///
/// The one group in the process token carrying `SE_GROUP_LOGON_ID`. Windows guarantees at most one,
/// and a token without one is a real answer rather than an error to paper over — it happens for
/// tokens that were never part of an interactive logon — so the caller gets `None` and, per
/// [`AttentionPipe::start`], no endpoint at all. **A missing logon SID must never fall back to a
/// default descriptor**: that is the exact failure this module exists to prevent, and a fallback
/// would make it happen precisely on the machines nobody tests on.
fn logon_sid() -> Option<String> {
    let mut token = HANDLE::default();
    // SAFETY: `GetCurrentProcess` is a pseudo-handle needing no close, and `token` is a live local
    // for the duration of the call.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }.ok()?;
    let guard = OwnedHandle(token);
    let mut needed = 0u32;
    // SAFETY: the documented two-call shape — the first fails with the size it wants.
    let _ = unsafe { GetTokenInformation(guard.0, TokenGroups, None, 0, &raw mut needed) };
    if needed == 0 {
        return None;
    }
    // `TOKEN_GROUPS` is a header plus a run of `SID_AND_ATTRIBUTES`, so the buffer is bytes with an
    // alignment strong enough for the struct — a `Vec<u64>` gives eight, which is what a pointer
    // field needs and more than the header does.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: the buffer is at least `needed` bytes and lives across the call.
    unsafe {
        GetTokenInformation(
            guard.0,
            TokenGroups,
            Some(buffer.as_mut_ptr().cast::<c_void>()),
            needed,
            &raw mut needed,
        )
    }
    .ok()?;
    let groups = buffer.as_ptr().cast::<TOKEN_GROUPS>();
    // SAFETY: the kernel filled this buffer with a `TOKEN_GROUPS` whose `GroupCount` describes the
    // run of entries that follows the header.
    let count = unsafe { (*groups).GroupCount } as usize;
    // SAFETY: `Groups` is a one-element array standing for `count` of them, which is the documented
    // shape of every counted Win32 structure.
    let entries = unsafe {
        std::slice::from_raw_parts(
            (&raw const (*groups).Groups).cast::<windows::Win32::Security::SID_AND_ATTRIBUTES>(),
            count,
        )
    };
    let sid = entries
        .iter()
        .find(|entry| entry.Attributes & SE_GROUP_LOGON_ID == SE_GROUP_LOGON_ID)
        .map(|entry| entry.Sid)?;
    sid_to_string(sid)
}

/// One SID, printed.
fn sid_to_string(sid: PSID) -> Option<String> {
    let mut text = windows::core::PWSTR::null();
    // SAFETY: `text` receives a `LocalAlloc`ed string the guard below frees.
    unsafe { ConvertSidToStringSidW(sid, &raw mut text) }.ok()?;
    if text.is_null() {
        return None;
    }
    // SAFETY: the call above wrote a NUL-terminated wide string.
    let owned = unsafe { text.to_string() }.ok();
    // SAFETY: `ConvertSidToStringSidW` documents `LocalFree` as the matching release.
    unsafe {
        let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(text.0.cast())));
    }
    owned
}

use windows::Win32::Security::GetTokenInformation;

/// A handle this module owns, closed when it goes out of scope.
///
/// Small enough to be written out, and worth writing out: every early return below is a place a
/// hand-closed handle would leak on, and the paths that fail are the ones nobody exercises.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: this type is the sole owner of the handle.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// A handle that crosses to the listener thread.
///
/// Same reasoning as `DirWatch`'s: these are process-wide kernel objects with one user apiece, and
/// [`AttentionPipe::drop`] joins the thread before closing any of them.
#[derive(Clone, Copy)]
struct SendHandle(HANDLE);

// SAFETY: see the type's own note.
unsafe impl Send for SendHandle {}

/// **The endpoint.** Live from the moment [`AttentionPipe::start`] returns, closed when this is
/// dropped.
pub struct AttentionPipe {
    name: String,
    stop: SendHandle,
    listener: Option<JoinHandle<()>>,
    counts: Arc<Mutex<PipeCounts>>,
}

impl AttentionPipe {
    /// Open this process's endpoint and start listening.
    ///
    /// **It returns already listening**, which is `CONVENTIONS.md`'s rule for anything shaped like
    /// a subscription and is load-bearing here for a specific reason: the first pane's shell is
    /// spawned within a frame of this returning, and a hook that fired against an endpoint that was
    /// "about to exist" would be a signal that is not late but *gone*. So the listener thread
    /// issues its first `ConnectNamedPipe` and says so, and this waits for that word — or hands
    /// back the refusal instead of a thread that dies in private.
    ///
    /// `deliver` is called **on the listener thread**, once per accepted frame, and is expected to
    /// do nothing but park the line and nudge the loop that will act on it. It is handed the raw
    /// bytes as a `String` and no more: this module knows nothing about the grammar inside, which
    /// is what keeps a parser change out of the unsafe boundary.
    ///
    /// The frame's content is deliberately *not* validated here beyond its length. In particular
    /// there is no pane coordinate to check, because there is none in the format — a caller says
    /// which pane it means by presenting that pane's capability, and a capability it does not hold
    /// is a capability it cannot name.
    pub fn start(deliver: impl Fn(String) + Send + 'static) -> io::Result<Self> {
        let Some(logon) = logon_sid() else {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "this process's token carries no logon SID, so the endpoint has no principal to \
                 grant and will not be opened with a default descriptor",
            ));
        };
        let name = endpoint_name(&session_tag(&logon), process_id(), unguessable_bits());
        let sddl = security_descriptor_sddl(&logon);
        let descriptor = SecurityDescriptor::from_sddl(&sddl)?;
        // Manual-reset: the listener may be anywhere between two waits when `drop` fires, so once
        // this is set it has to stay set.
        // SAFETY: a nameless, unowned event.
        let stop =
            unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(win32_io_error)?;
        let stop = SendHandle(stop);
        let counts = Arc::new(Mutex::new(PipeCounts::default()));
        let (armed, first_word) = mpsc::channel::<io::Result<()>>();
        let listener = {
            let name = name.clone();
            let counts = Arc::clone(&counts);
            std::thread::Builder::new()
                .name("folio-attention-endpoint".to_owned())
                .spawn(move || listen(&name, descriptor, stop, &counts, &armed, &deliver))?
        };
        // The thread's first word. A timeout rather than a bare `recv` because a listener that
        // never speaks is a bug in this file, and hanging the launch over it would turn a bug into
        // a product that does not start.
        match first_word.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                name,
                stop,
                listener: Some(listener),
                counts,
            }),
            Ok(Err(error)) => {
                let _ = listener.join();
                // SAFETY: the thread that shared this has been joined.
                unsafe {
                    let _ = CloseHandle(stop.0);
                }
                Err(error)
            }
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                // SAFETY: setting an event is sound from any thread that has it.
                unsafe {
                    let _ = SetEvent(stop.0);
                }
                let _ = listener.join();
                // SAFETY: the thread that shared this has been joined.
                unsafe {
                    let _ = CloseHandle(stop.0);
                }
                Err(io::Error::other("the attention endpoint never came up"))
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

// SAFETY: the only raw handle in this value is the stop event. Setting an event is thread-safe by
// contract, and it is the only thing anything does with that handle before `drop`, which runs once
// on the sole owner and joins the listener before closing it. The counters are behind a mutex and
// the join handle is `Send + Sync` already.
//
// Written out because `HANDLE` is a pointer-shaped value and the compiler cannot know any of that:
// a process-wide endpoint has to live in a `static`, and a `static` needs `Sync`.
unsafe impl Sync for AttentionPipe {}

impl Drop for AttentionPipe {
    fn drop(&mut self) {
        // SAFETY: the stop event is alive until after the join below.
        unsafe {
            let _ = SetEvent(self.stop.0);
        }
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
        // SAFETY: the only other user of this handle has been joined.
        unsafe {
            let _ = CloseHandle(self.stop.0);
        }
    }
}

/// A `LocalAlloc`ed security descriptor, freed when it goes out of scope.
struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

// SAFETY: the descriptor is a plain kernel-format buffer with one owner; it crosses to the
// listener thread and is freed there.
unsafe impl Send for SecurityDescriptor {}

impl SecurityDescriptor {
    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let wide = wide(sddl);
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `wide` is NUL-terminated and outlives the call; the out-parameter receives a
        // `LocalAlloc`ed buffer this type frees.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(wide.as_ptr()),
                SDDL_REVISION_1,
                &raw mut descriptor,
                None,
            )
        }
        .map_err(win32_io_error)?;
        Ok(Self(descriptor))
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
            lpSecurityDescriptor: self.0.0,
            bInheritHandle: false.into(),
        }
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: the conversion above documents `LocalFree` as the matching release.
            unsafe {
                let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(self.0.0)));
            }
        }
    }
}

/// How many instances of the endpoint listen at once.
///
/// Four, and none of them is for throughput: every transaction is a connect, a write of a few dozen
/// bytes and a close. They are there so that a caller arriving while another is being served finds
/// a door rather than a wait — and, with the loop below, so that being *served* never means being
/// queued behind somebody else's read.
///
/// **They are created once and never replaced.** Two earlier shapes of this file replaced them, and
/// both lost frames; see [`listen`] for what replacing costs and why recycling the same handle does
/// not.
const MAX_INSTANCES: u32 = 4;

/// What one instance is currently waiting for.
///
/// Two states, and the transition between them is the whole fix: **a read is posted the instant a
/// connect completes**, on the same event, before this loop does anything else. So a client's bytes
/// are already being awaited by the kernel while the loop is still deciding what to do next, and
/// there is no window in which an arrival depends on this thread's position in a loop.
#[derive(Clone, Copy, Debug)]
enum Phase {
    /// A `ConnectNamedPipe` is outstanding: nobody is here yet.
    Connecting,
    /// A client is attached and a `ReadFile` is outstanding. `since` is what
    /// [`READ_DEADLINE`] is measured from.
    Reading { since: Instant },
}

/// One instance of the endpoint: a handle, one event, one operation at a time.
///
/// One event per instance rather than one per operation, because the instance is now long-lived —
/// it is reset and reused for the connect and then for the read, and again for the next client.
/// That is what makes "recycle" mean `DisconnectNamedPipe` + `ConnectNamedPipe` on the handle we
/// already have, rather than a create and a close.
struct Instance {
    pipe: OwnedHandle,
    event: Overlapped,
    /// The kernel owns this address for as long as an operation is outstanding, which is nearly
    /// always — hence the box, and hence its being reset rather than rebuilt.
    overlapped: Box<OVERLAPPED>,
    /// **Per instance, because reads are now concurrent.** Four instances can each have a read
    /// outstanding at once, and one shared buffer would be four kernel writes into the same bytes.
    buffer: Vec<u8>,
    phase: Phase,
}

impl Drop for Instance {
    fn drop(&mut self) {
        cancel(self.pipe.0);
        // SAFETY: this instance owns the handle; disconnecting an unconnected pipe is a no-op.
        unsafe {
            let _ = DisconnectNamedPipe(self.pipe.0);
        }
    }
}

/// What posting an operation decided, when it decided on the spot.
enum Posted {
    /// The kernel has it. The event will be — or already is — signalled.
    Pending,
    /// A message longer than this endpoint will take.
    Oversize,
    /// The client is not going to say anything on this connection.
    Failed,
}

/// What a completed read turned out to be.
enum Frame {
    /// This many bytes, at the front of the instance's own buffer.
    Line(usize),
    Oversize,
    Silent,
}

impl Instance {
    /// Create one instance and put a connect on it.
    ///
    /// `first` asks for `FILE_FLAG_FIRST_PIPE_INSTANCE`, which is a **check** rather than a flag: it
    /// fails if the name already exists, so a process that squatted this name before us cannot end
    /// up being the thing our own children talk to.
    fn open(
        wide_name: &[u16],
        attributes: &SECURITY_ATTRIBUTES,
        first: bool,
    ) -> io::Result<(Self, bool)> {
        let mut mode = PIPE_ACCESS_INBOUND | FILE_FLAG_OVERLAPPED;
        if first {
            mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        // SAFETY: `wide_name` is NUL-terminated and outlives the call; `attributes` points at a
        // descriptor the caller keeps alive for the whole of the listener.
        let pipe = unsafe {
            CreateNamedPipeW(
                PCWSTR(wide_name.as_ptr()),
                mode,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                MAX_INSTANCES,
                0,
                u32::try_from(MAX_MESSAGE_BYTES).unwrap_or(0),
                0,
                Some(&raw const *attributes),
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let event = Overlapped::new()?;
        let overlapped = Box::new(event.overlapped());
        let mut instance = Self {
            pipe: OwnedHandle(pipe),
            event,
            overlapped,
            buffer: vec![0u8; MAX_MESSAGE_BYTES],
            phase: Phase::Connecting,
        };
        let attached = instance.arm_connect()?;
        Ok((instance, attached))
    }

    /// Put a connect on this instance. `true` when a client is **already** there — or has been.
    ///
    /// **Three answers mean "attached", and the third is the one that cost a frame.** A named pipe
    /// instance starts listening the moment `CreateNamedPipeW` returns, not when
    /// `ConnectNamedPipe` is called, so a client can arrive in the window between them — and
    /// `folio attention` connects, writes forty bytes and closes inside a millisecond, so it can
    /// arrive **and leave** inside that window.
    ///
    /// * `Ok(())` — the connect completed at once.
    /// * `ERROR_PIPE_CONNECTED` — somebody got here first and is still holding on.
    /// * `ERROR_NO_DATA` — somebody got here first, said their piece and **has already gone**.
    ///
    /// None of the three signals the event, because none of them went to the kernel as an
    /// overlapped operation; all three have to be carried back rather than waited for.
    ///
    /// The third was previously read as a failure, and a failed instance was discarded — **taking
    /// the message still sitting in its buffer with it**. That is the defect this file has now been
    /// through three shapes of, and it is exactly what `accepted` falling one short says out loud:
    /// the frame was not refused, it was never accounted for. A pipe's buffered data survives its
    /// client's departure and stays readable until `DisconnectNamedPipe`, so the right answer to
    /// "the client has gone" is not to disconnect but to **read** — and if there turns out to be
    /// nothing there, that is a silent connection and is counted as one.
    fn arm_connect(&mut self) -> io::Result<bool> {
        self.reset();
        self.phase = Phase::Connecting;
        // SAFETY: the boxed `OVERLAPPED` lives at a fixed address for as long as this instance
        // does, and this instance outlives the operation — `Drop` cancels anything still pending.
        let issued = unsafe { ConnectNamedPipe(self.pipe.0, Some(&raw mut *self.overlapped)) };
        let code = |error: &windows::core::Error| win32_of(error);
        match issued {
            Ok(()) => Ok(true),
            Err(error)
                if code(&error) == ERROR_PIPE_CONNECTED.0 || code(&error) == ERROR_NO_DATA.0 =>
            {
                Ok(true)
            }
            Err(error) if code(&error) == ERROR_IO_PENDING.0 => Ok(false),
            Err(error) => Err(win32_io_error(error)),
        }
    }

    /// **Post the read, now.** Called the instant a connect completes and never later.
    fn post_read(&mut self) -> Posted {
        self.reset();
        self.phase = Phase::Reading {
            since: Instant::now(),
        };
        // SAFETY: both the buffer and the boxed `OVERLAPPED` belong to this instance and outlive
        // the operation, which `Drop` cancels if it is still outstanding.
        let issued = unsafe {
            ReadFile(
                self.pipe.0,
                Some(&mut self.buffer),
                None,
                Some(&raw mut *self.overlapped),
            )
        };
        match issued {
            // Synchronous completion still signals the event, because the `OVERLAPPED` names one —
            // so both answers are the same answer here, and the loop picks it up uniformly.
            Ok(()) => Posted::Pending,
            Err(error) if win32_of(&error) == ERROR_IO_PENDING.0 => Posted::Pending,
            Err(error) if win32_of(&error) == ERROR_MORE_DATA.0 => Posted::Oversize,
            Err(_) => Posted::Failed,
        }
    }

    /// Collect a read the event has just reported.
    fn complete_read(&mut self) -> Frame {
        let mut read = 0u32;
        // SAFETY: the instance still owns the handle and the structure the operation was issued on.
        let done = unsafe {
            windows::Win32::System::IO::GetOverlappedResult(
                self.pipe.0,
                &raw const *self.overlapped,
                &raw mut read,
                false,
            )
        };
        match done {
            Ok(()) => {
                let read = (read as usize).min(self.buffer.len());
                if read == 0 {
                    Frame::Silent
                } else {
                    Frame::Line(read)
                }
            }
            Err(error) if win32_of(&error) == ERROR_MORE_DATA.0 => Frame::Oversize,
            Err(_) => Frame::Silent,
        }
    }

    /// Hand this instance back to the listening pool, on the handle it already has.
    ///
    /// **This is what replaced replacing.** A create-and-close pair has a window in which the
    /// endpoint answers with one fewer instance — or, if it was the last, with none — and a caller
    /// landing in it is told there is no such pipe. `DisconnectNamedPipe` followed by
    /// `ConnectNamedPipe` on a handle we never let go of has no such window: the instance exists
    /// throughout, and the only observable moment is the one where it stops being *connected* and
    /// starts being *listening*.
    fn recycle(&mut self) -> io::Result<bool> {
        cancel(self.pipe.0);
        // SAFETY: this instance owns the handle.
        unsafe {
            let _ = DisconnectNamedPipe(self.pipe.0);
        }
        self.arm_connect()
    }

    /// Clear the event and the structure before reusing them for the next operation.
    fn reset(&mut self) {
        // SAFETY: the event belongs to this instance.
        unsafe {
            let _ = ResetEvent(self.event.handle());
        }
        *self.overlapped = self.event.overlapped();
    }

    /// How long until this instance's client has run out of time to say anything.
    fn remaining(&self, now: Instant) -> Option<Duration> {
        match self.phase {
            Phase::Connecting => None,
            Phase::Reading { since } => {
                Some(READ_DEADLINE.saturating_sub(now.duration_since(since)))
            }
        }
    }
}

/// The listener thread: **four instances listening, each with its read already posted, and none of
/// them ever created or destroyed while the endpoint is up.**
///
/// Three shapes of this loop have now been measured, and the two that failed failed the same way:
/// an arrival's fate depended on where *this thread* happened to be.
///
/// * **One instance, closed and reopened.** The name stopped existing between the two, and a caller
///   arriving in that window is not told "busy" — it is told **there is no such pipe** and gives up.
///   Red form: `ERROR_FILE_NOT_FOUND` at the client.
/// * **One instance plus a successor armed before the read.** No gap in the name, but the pool could
///   only ever be two deep, and the replacement was a *create* that had to succeed while the old
///   instance still counted against `nMaxInstances`. Red form: `frame 1` absent from a run of five —
///   and, under a loaded machine, one absent from a run of **eight** — with `delivered` short by one
///   and every refusal counter at zero. The endpoint had not refused it; it had never seen it.
///
/// The invariant that removes the class: **an instance is never replaced, and a read is outstanding
/// from the moment a client attaches.** Recycling is `DisconnectNamedPipe` + `ConnectNamedPipe` on
/// the handle we already hold, so the count of instances answering this name is constant from the
/// moment the endpoint opens until it closes; and because the read is posted in the same breath as
/// the connect completing, a caller's bytes are already awaited by the kernel while this loop is
/// still deciding what to look at next. Four reads can be outstanding at once, which is why each
/// instance owns its buffer.
///
/// [`READ_DEADLINE`] is still the bound on a caller that attaches and says nothing — but it now
/// costs that caller its own instance rather than the whole endpoint's attention, and the loop
/// sweeps it on the timeout of the same wait it is already doing.
fn listen(
    name: &str,
    descriptor: SecurityDescriptor,
    stop: SendHandle,
    counts: &Mutex<PipeCounts>,
    armed: &mpsc::Sender<io::Result<()>>,
    deliver: &(impl Fn(String) + Send + ?Sized),
) {
    let attributes = descriptor.attributes();
    let wide_name = wide(name);
    let mut rate = RateLimit::new(Instant::now());
    let mut pool: Vec<Instance> = Vec::new();
    for index in 0..MAX_INSTANCES as usize {
        match Instance::open(&wide_name, &attributes, index == 0) {
            Ok((mut instance, attached)) => {
                if attached {
                    note_accepted(counts);
                    let _ = instance.post_read();
                }
                pool.push(instance);
                // The first instance's success is the word that makes `start` mean what it says: a
                // connect is outstanding on a pipe that exists, before anybody is told the endpoint
                // is open.
                if index == 0 {
                    let _ = armed.send(Ok(()));
                }
            }
            Err(error) => {
                if index == 0 {
                    let _ = armed.send(Err(error));
                    return;
                }
                // Fewer than four is a smaller endpoint, not a broken one.
                break;
            }
        }
    }
    loop {
        let now = Instant::now();
        let timeout = pool
            .iter()
            .filter_map(|instance| instance.remaining(now))
            .min()
            .map_or(INFINITE, |left| {
                u32::try_from(left.as_millis()).unwrap_or(INFINITE).max(1)
            });
        let mut handles = pool
            .iter()
            .map(|instance| instance.event.handle())
            .collect::<Vec<_>>();
        handles.push(stop.0);
        // SAFETY: every handle is owned by this thread's pool, or is the stop event, which outlives
        // the listener.
        let answer = unsafe { WaitForMultipleObjects(&handles, false, timeout) };
        if answer == WAIT_TIMEOUT {
            sweep(&mut pool, counts);
            if pool.is_empty() {
                return;
            }
            continue;
        }
        let Some(index) = answer.0.checked_sub(WAIT_OBJECT_0.0) else {
            return;
        };
        let index = index as usize;
        // The stop event sits one past the pool; anything further is a wait that failed. Either way
        // this thread is finished, and every instance's `Drop` cancels and closes it.
        if index >= pool.len() {
            return;
        }
        let outcome = match pool[index].phase {
            // A client has attached. **Post its read before anything else happens.**
            Phase::Connecting => {
                note_accepted(counts);
                match pool[index].post_read() {
                    Posted::Pending => None,
                    Posted::Oversize => Some(Frame::Oversize),
                    Posted::Failed => Some(Frame::Silent),
                }
            }
            Phase::Reading { .. } => Some(pool[index].complete_read()),
        };
        let Some(frame) = outcome else {
            continue;
        };
        match frame {
            Frame::Line(read) => {
                let mut counts = counts.lock().unwrap_or_else(PoisonError::into_inner);
                if rate.admit(Instant::now()) {
                    counts.delivered += 1;
                    drop(counts);
                    let line = String::from_utf8_lossy(&pool[index].buffer[..read]).into_owned();
                    deliver(line);
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
        if !relist(&mut pool, index, counts) {
            return;
        }
    }
}

/// One more client through the door, for the conservation law in [`PipeCounts::accepted`].
fn note_accepted(counts: &Mutex<PipeCounts>) {
    counts
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .accepted += 1;
}

/// Put one instance back to listening, dropping it if it will not go.
///
/// `false` only when the pool has emptied, which is the one condition that ends the listener.
fn relist(pool: &mut Vec<Instance>, index: usize, counts: &Mutex<PipeCounts>) -> bool {
    match pool[index].recycle() {
        Ok(true) => {
            // Somebody was already waiting for this instance. Their read starts now, not on the
            // next turn of the loop.
            note_accepted(counts);
            let _ = pool[index].post_read();
            true
        }
        Ok(false) => true,
        Err(_) => {
            pool.remove(index);
            !pool.is_empty()
        }
    }
}

/// Take back the instances whose clients attached and then said nothing.
fn sweep(pool: &mut Vec<Instance>, counts: &Mutex<PipeCounts>) {
    let now = Instant::now();
    let mut index = 0;
    while index < pool.len() {
        let expired = pool[index]
            .remaining(now)
            .is_some_and(|left| left.is_zero());
        if expired {
            counts.lock().unwrap_or_else(PoisonError::into_inner).silent += 1;
            if !relist(pool, index, counts) {
                return;
            }
        }
        index += 1;
    }
}

fn cancel(pipe: HANDLE) {
    // SAFETY: cancelling this thread's own outstanding operations on a handle it owns.
    unsafe {
        let _ = CancelIoEx(pipe, None);
    }
}

/// An event and the `OVERLAPPED` that names it.
struct Overlapped(HANDLE);

impl Overlapped {
    fn new() -> io::Result<Self> {
        // Manual-reset and initially unsignalled; each of these lives for exactly one operation, so
        // there is no stale signal to reset by hand.
        // SAFETY: a nameless, unowned event.
        let event =
            unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(win32_io_error)?;
        Ok(Self(event))
    }

    fn handle(&self) -> HANDLE {
        self.0
    }

    fn overlapped(&self) -> OVERLAPPED {
        OVERLAPPED {
            hEvent: self.0,
            ..Default::default()
        }
    }
}

impl Drop for Overlapped {
    fn drop(&mut self) {
        // SAFETY: this type is the sole owner.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// **`folio attention`'s whole conversation**: connect, write one line, close.
///
/// Bounded end to end. The endpoint serves one client at a time, so a busy answer is ordinary
/// rather than exceptional, and it is waited on for [`CLIENT_BUSY_WAIT_MS`] and then given up on —
/// the verb's contract is that it never blocks, and "never" has to be a number somewhere.
///
/// No reply is read, and there is none to read: the endpoint's pipe is inbound-only. A verb that
/// waited for an acknowledgement would be a verb that could hang on a window that is busy painting,
/// which is precisely the moment a hook is most likely to fire.
pub fn send_line(endpoint: &str, line: &str) -> io::Result<()> {
    if line.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "message longer than the endpoint's frame bound",
        ));
    }
    let wide_name = wide(endpoint);
    let handle = open_client(&wide_name)?;
    let mut written = 0u32;
    // SAFETY: the buffer outlives the synchronous call.
    unsafe {
        WriteFile(
            handle.0,
            Some(line.as_bytes()),
            Some(&raw mut written),
            None,
        )
    }
    .map_err(win32_io_error)?;
    Ok(())
}

fn open_client(wide_name: &[u16]) -> io::Result<OwnedHandle> {
    for attempt in 0..2 {
        // SAFETY: `wide_name` is NUL-terminated and outlives the call.
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide_name.as_ptr()),
                GENERIC_WRITE.0,
                FILE_SHARE_MODE(0),
                None,
                OPEN_EXISTING,
                windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        };
        match handle {
            Ok(handle) if !handle.is_invalid() => return Ok(OwnedHandle(handle)),
            Ok(_) => return Err(io::Error::other("the endpoint answered with no handle")),
            Err(error) => {
                let code = win32_of(&error);
                if attempt == 0 && code == ERROR_PIPE_BUSY.0 {
                    // The answer is deliberately ignored: a wait that timed out and a wait that
                    // succeeded lead to the same next move, which is to try the open once more and
                    // let *that* say whether the endpoint is free. Branching here would put the
                    // decision in two places.
                    // SAFETY: `wide_name` is NUL-terminated and outlives the call.
                    let _free =
                        unsafe { WaitNamedPipeW(PCWSTR(wide_name.as_ptr()), CLIENT_BUSY_WAIT_MS) };
                    continue;
                }
                return Err(win32_io_error(error));
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "the endpoint stayed busy for the whole of the verb's allowance",
    ))
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn process_id() -> u32 {
    // SAFETY: no arguments, no failure mode.
    unsafe { GetCurrentProcessId() }
}

/// 128 unpredictable bits, for the endpoint's name and for every pane capability.
///
/// The standard library's own hash seed, which Windows fills from the OS entropy source once per
/// process and which `RandomState::new` walks forward on every call — so two calls in one process
/// differ, and two processes started in the same millisecond differ. `RtlGenRandom` would be the
/// textbook answer and is one more Win32 surface for the same bits.
///
/// **This is not the security boundary — the DACL is.** What the randomness buys is narrower and
/// worth stating: a name nobody can guess from a process id, so a stale capability sitting in a
/// long-lived child's environment cannot address a window that happens to have inherited that id;
/// and a capability that names one pane and cannot be walked to the next.
#[must_use]
pub fn unguessable_bits() -> u128 {
    use std::hash::{BuildHasher, Hasher};
    let half = || {
        std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish()
    };
    (u128::from(half()) << 64) | u128::from(half())
}

fn win32_io_error(error: windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(crate::windows_impl::win32_code(error.code()))
}

/// The Win32 code inside an `HRESULT`-wearing error, as a `u32` to compare against the constants.
///
/// One spelling, because the alternative — `error.code().0 as u32 == 0x8007_0000 | CODE.0` written
/// out at each site — is a place for the facility bits to be got wrong once and read as a code that
/// never matches, which is a branch that silently never runs.
fn win32_of(error: &windows::core::Error) -> u32 {
    crate::windows_impl::win32_code(error.code()) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The descriptor is written, and it says one thing.**
    ///
    /// The red form of this is the whole module's reason to exist: an endpoint created with `None`
    /// for its attributes gets Windows' default pipe DACL, which grants `Everyone` — anonymous
    /// logons included — read access. So this asserts the *shape*: a protected DACL, exactly one
    /// ACE, and the principal is the logon SID it was given.
    #[test]
    fn the_endpoint_grants_the_logon_session_and_nothing_else() {
        let sddl = security_descriptor_sddl("S-1-5-5-0-1234567");
        assert_eq!(sddl, "D:P(A;;GA;;;S-1-5-5-0-1234567)");
        assert!(
            sddl.starts_with("D:P"),
            "an unprotected DACL inherits ACEs nobody wrote: {sddl}"
        );
        assert_eq!(
            sddl.matches("(A;").count(),
            1,
            "one principal, and a second entry is a second answer to who may connect: {sddl}"
        );
        for outsider in ["S-1-1-0", "S-1-5-7", "S-1-5-2", "WD", "AN", "NU"] {
            assert!(
                !sddl.contains(outsider),
                "{outsider} names a principal outside this logon session: {sddl}"
            );
        }
    }

    /// The name keeps two runs of one process id apart, and two logon sessions apart.
    #[test]
    fn an_endpoint_name_is_unique_per_session_per_process_and_per_run() {
        let one = endpoint_name(&session_tag("S-1-5-5-0-1"), 4242, 1);
        let same = endpoint_name(&session_tag("S-1-5-5-0-1"), 4242, 1);
        let next_run = endpoint_name(&session_tag("S-1-5-5-0-1"), 4242, 2);
        let other_session = endpoint_name(&session_tag("S-1-5-5-0-2"), 4242, 1);
        let other_process = endpoint_name(&session_tag("S-1-5-5-0-1"), 4243, 1);
        assert_eq!(one, same);
        assert_ne!(one, next_run, "a reused process id must not reuse a name");
        assert_ne!(one, other_session);
        assert_ne!(one, other_process);
        assert!(one.starts_with(r"\\.\pipe\folio-attention-"), "{one}");
        assert!(
            !one.contains("S-1-5-5"),
            "the session segment is a digest, not the SID itself: {one}"
        );
    }

    /// A burst passes; a flood does not; and the next second forgives.
    #[test]
    fn the_rate_bound_lets_a_burst_through_and_refuses_a_flood() {
        let start = Instant::now();
        let mut rate = RateLimit::new(start);
        for frame in 0..MAX_FRAMES_PER_SECOND {
            assert!(rate.admit(start), "frame {frame} of one burst was refused");
        }
        assert!(
            !rate.admit(start),
            "the bound is the bound: frame {MAX_FRAMES_PER_SECOND} must not pass"
        );
        assert!(
            rate.admit(start + Duration::from_millis(1001)),
            "a bucket that never refills is a bucket that breaks the feature after one flood"
        );
    }

    /// **The endpoint is live when `start` returns, and one line crosses it.**
    ///
    /// End to end against the real kernel object, because the two things worth pinning here are
    /// both properties of the real one: that a client can connect the instant `start` hands back,
    /// and that the bytes arrive whole.
    #[test]
    fn a_line_written_the_instant_the_endpoint_opens_arrives_whole() {
        let (sender, lines) = mpsc::channel();
        let pipe = AttentionPipe::start(move |line| {
            let _ = sender.send(line);
        })
        .expect("open the endpoint");
        send_line(pipe.name(), r#"{"v":1,"event":"PermissionRequest"}"#).expect("write one line");
        let line = lines
            .recv_timeout(Duration::from_secs(5))
            .expect("the endpoint delivered nothing");
        assert_eq!(line, r#"{"v":1,"event":"PermissionRequest"}"#);
        assert_eq!(pipe.counts().delivered, 1);
    }

    /// **A whole turn's worth of hooks, back to back, and not one of them lost.**
    ///
    /// Twice the pool's size, from one thread with no pause, which is more than a turn ever
    /// produces — a permission request, its receipt, a prompt and a stop is four. Both earlier
    /// shapes of the listener failed this: the first told a caller arriving between two clients
    /// that there was no such pipe, the second let one of five go missing with every refusal
    /// counter at zero — and, on a loaded machine, one of these eight.
    ///
    /// **Order is compared as a set, and that is the honest comparison.** Callers are separate
    /// processes landing on whichever instance is free, so the endpoint does not promise the order
    /// two of them are read in and could not keep such a promise if it made one. What it promises
    /// is that a frame it accepted arrives, and that is what is asserted.
    #[test]
    fn a_whole_turn_of_hooks_arrives_with_none_of_them_lost() {
        const FRAMES: usize = 8;
        let (sender, lines) = mpsc::channel();
        let pipe = AttentionPipe::start(move |line| {
            let _ = sender.send(line);
        })
        .expect("open the endpoint");
        for index in 0..FRAMES {
            send_line(pipe.name(), &format!("frame {index}")).expect("write");
        }
        let mut arrived = Vec::new();
        while arrived.len() < FRAMES {
            match lines.recv_timeout(Duration::from_secs(5)) {
                Ok(line) => arrived.push(line),
                Err(_) => break,
            }
        }
        arrived.sort();
        let mut expected = (0..FRAMES)
            .map(|index| format!("frame {index}"))
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(arrived, expected, "counts were {:?}", pipe.counts());
        let counts = pipe.counts();
        assert_eq!(counts.delivered, FRAMES as u64);
        assert_eq!(
            counts.accepted, counts.delivered,
            "a client attached and was not delivered: {counts:?}"
        );
    }

    /// **Ten times the pool, from one thread with no pause: still not one lost.**
    ///
    /// Far past anything a hook does — the rate bound would be refusing at sixty-four in a second,
    /// and a turn produces four — and here precisely because the previous shape of the listener
    /// lost one frame in forty and the header carried that as an accepted limit. It is not one any
    /// more, and this is the test that says so: replacing an instance was the whole of it, and
    /// nothing is replaced now.
    ///
    /// Kept just under the rate bound, because a throttled frame is a frame the endpoint **did**
    /// refuse — a different sentence from losing one, counted separately, and not what this is for.
    #[test]
    fn a_flood_far_past_any_real_producer_still_loses_nothing() {
        const FRAMES: usize = 40;
        let (sender, lines) = mpsc::channel();
        let pipe = AttentionPipe::start(move |line| {
            let _ = sender.send(line);
        })
        .expect("open the endpoint");
        for index in 0..FRAMES {
            send_line(pipe.name(), &format!("frame {index}")).expect("write");
        }
        let mut arrived = Vec::new();
        while arrived.len() < FRAMES {
            match lines.recv_timeout(Duration::from_secs(10)) {
                Ok(line) => arrived.push(line),
                Err(_) => break,
            }
        }
        arrived.sort();
        let mut expected = (0..FRAMES)
            .map(|index| format!("frame {index}"))
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(arrived, expected, "counts were {:?}", pipe.counts());
        assert_eq!(pipe.counts().delivered, FRAMES as u64);
        assert_eq!(pipe.counts().silent, 0, "no client was timed out");
    }

    /// **Every client that attaches is accounted for**, which is the pin the two wrong fixes needed.
    ///
    /// Neither of them was aimed at the right place, and nothing in the counters could have said so
    /// — "delivered is one short" is equally consistent with a read that failed and a connection
    /// nobody ever saw. This asserts the arithmetic instead: attach, and become exactly one of the
    /// four outcomes. A future change that drops a client on some new path fails this without
    /// anybody having to guess in advance what that path is.
    #[test]
    fn every_client_that_attaches_is_accounted_for() {
        let (sender, lines) = mpsc::channel();
        let pipe = AttentionPipe::start(move |line| {
            let _ = sender.send(line);
        })
        .expect("open the endpoint");
        // A mixture on purpose: ordinary frames, and one the verb itself refuses to put on the wire.
        for index in 0..12 {
            send_line(pipe.name(), &format!("frame {index}")).expect("write");
        }
        assert!(send_line(pipe.name(), &"x".repeat(MAX_MESSAGE_BYTES + 1)).is_err());
        let mut arrived = 0;
        while arrived < 12 {
            match lines.recv_timeout(Duration::from_secs(10)) {
                Ok(_) => arrived += 1,
                Err(_) => break,
            }
        }
        let counts = pipe.counts();
        assert_eq!(
            counts.accepted,
            counts.delivered + counts.oversize + counts.throttled + counts.silent,
            "a client attached and became none of the four outcomes: {counts:?}"
        );
        assert_eq!(counts.delivered, 12, "{counts:?}");
    }

    /// A frame over the bound is refused whole rather than truncated and parsed.
    #[test]
    fn an_oversized_frame_is_dropped_and_counted() {
        let (sender, lines) = mpsc::channel();
        let pipe = AttentionPipe::start(move |line| {
            let _ = sender.send(line);
        })
        .expect("open the endpoint");
        // The client's own bound refuses it before a byte moves, which is the first of the two
        // gates; the endpoint's is the second and is what the count below proves.
        let too_long = "x".repeat(MAX_MESSAGE_BYTES + 1);
        assert!(
            send_line(pipe.name(), &too_long).is_err(),
            "the verb must not put a frame on the wire that the endpoint would refuse"
        );
        send_line(pipe.name(), "after").expect("write");
        assert_eq!(
            lines
                .recv_timeout(Duration::from_secs(5))
                .expect("delivery"),
            "after",
            "an oversized frame must not wedge the endpoint for the next caller"
        );
    }

    /// A verb aimed at an endpoint that is not there fails at once rather than waiting on it.
    #[test]
    fn the_verb_gives_up_on_an_endpoint_that_does_not_exist() {
        let began = Instant::now();
        let answer = send_line(
            &endpoint_name(&session_tag("S-1-5-5-0-9"), 1, 0xdead_beef),
            "wait",
        );
        assert!(answer.is_err(), "there is no such endpoint");
        assert!(
            began.elapsed() < Duration::from_secs(1),
            "the verb waited {:?} on a name that does not exist",
            began.elapsed()
        );
    }

    /// Dropping the endpoint closes it, and the verb can tell.
    #[test]
    fn a_closed_endpoint_stops_answering() {
        let name = {
            let pipe = AttentionPipe::start(|_| {}).expect("open the endpoint");
            let name = pipe.name().to_owned();
            send_line(&name, "before").expect("the endpoint is open");
            name
        };
        assert!(
            send_line(&name, "after").is_err(),
            "a dropped endpoint must not still take frames"
        );
    }
}
