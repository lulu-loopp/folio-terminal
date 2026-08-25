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
//! # The measured limit, written down rather than discovered later
//!
//! Forty frames written back to back from one thread lost **one** of them, on a build whose pool
//! and read loop are otherwise sound, with every refusal counter at zero — so it was not refused,
//! it was written to an instance and never read. Eight in a row (twice the pool, and twice what a
//! whole turn produces) is solid over repeated runs, and that is what
//! [`a_whole_turn_of_hooks_arrives_with_none_of_them_lost`] pins.
//!
//! So the honest contract is: **an endpoint under a flood past its pool's width can drop a frame
//! that a client believed it had written.** Two reasons it is recorded here instead of chased:
//! a hook fires a handful of times per turn and [`MAX_FRAMES_PER_SECOND`] would be refusing long
//! before this, and the thing lost is one *idempotent* ring of a doorbell — the ledger's watermark
//! is what carries "the next real request is seen", not this channel's delivery (`attention` plan
//! §11.4.3, which sorts these two failure modes by weight and puts this one second).
//!
//! What would close it, when there is a reason to: a completion port with a read posted on every
//! instance from the moment it is armed, so that no arrival ever waits for this thread to notice
//! it. That is a different loop, not a bigger constant, which is why it is not being written on
//! the way past.

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
        CloseHandle, ERROR_IO_PENDING, ERROR_MORE_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
        GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, LocalFree, WAIT_OBJECT_0, WAIT_TIMEOUT,
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
            SetEvent, WaitForMultipleObjects,
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

/// How many instances of the endpoint may exist at once.
///
/// Four, and none of them is for concurrency: the listener reads one client at a time. They are
/// for the **gap**. An endpoint that closed one instance and then created the next would have a
/// window, however short, in which the name did not exist — and a caller that arrived in that
/// window is not told "busy", it is told *there is no such pipe*, and gives up. Two hooks firing
/// back to back land in exactly that window, which is how the defect was found. So the successor
/// is armed **before** the current client is read, and the name is continuously answerable.
const MAX_INSTANCES: u32 = 4;

/// One instance of the endpoint, with a connect outstanding on it.
///
/// The `OVERLAPPED` is boxed because an overlapped operation owns the address it was handed until
/// it completes, and this one outlives the call that issued it — it is armed in one place and
/// waited on in another, which is the whole point of arming the successor early.
struct Instance {
    pipe: OwnedHandle,
    event: Overlapped,
    /// Held, never read. The kernel owns this address until the connect completes or is cancelled,
    /// so the box's whole job is to still be there when that happens.
    #[allow(
        dead_code,
        reason = "the kernel holds this address until the operation completes"
    )]
    overlapped: Box<OVERLAPPED>,
    /// Set when `ConnectNamedPipe` completed on the spot — a client that was already there.
    ready: bool,
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

/// Create one instance and put a connect on it.
///
/// `first` asks for `FILE_FLAG_FIRST_PIPE_INSTANCE`, which is a **check** rather than a flag: it
/// fails if the name already exists, so a process that squatted this name before us cannot end up
/// being the thing our own children talk to.
fn arm(wide_name: &[u16], attributes: &SECURITY_ATTRIBUTES, first: bool) -> io::Result<Instance> {
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
    let pipe = OwnedHandle(pipe);
    let event = Overlapped::new()?;
    let mut overlapped = Box::new(event.overlapped());
    // SAFETY: the boxed `OVERLAPPED` is owned by the `Instance` returned below and stays at this
    // address until the operation is waited on or cancelled by that value's `Drop`.
    let issued = unsafe { ConnectNamedPipe(pipe.0, Some(&raw mut *overlapped)) };
    let ready = match issued {
        // A client that was already waiting. That is an arrival, not a failure.
        Ok(()) => true,
        Err(error) if win32_of(&error) == ERROR_PIPE_CONNECTED.0 => true,
        Err(error) if win32_of(&error) == ERROR_IO_PENDING.0 => false,
        Err(error) => return Err(win32_io_error(error)),
    };
    Ok(Instance {
        pipe,
        event,
        overlapped,
        ready,
    })
}

/// The listener thread: **[`MAX_INSTANCES`] listening at once, and none of them ever thrown away
/// with a client attached.**
///
/// Two earlier shapes of this loop each lost a frame, and both losses were the same mistake in
/// different clothes: an instance whose life was decided by *this* thread's position in a loop
/// rather than by whether a caller had already been let in.
///
/// * One instance at a time, closed and reopened — the name stopped existing between the two, and a
///   caller arriving in that window is not told "busy", it is told **there is no such pipe**, and
///   gives up. The red form was `ERROR_FILE_NOT_FOUND` at the client.
/// * One instance plus a successor armed before the read — no gap in the *name*, but still a gap in
///   the *chain*: only ever two instances exist, so a third caller arriving while one is being read
///   and one is spoken for finds nothing free, and the retry window is where a frame went missing.
///   The red form was `frame 1` absent from a run of five, with `delivered: 4` and every refusal
///   counter at zero — the endpoint had not refused it, it had never seen it.
///
/// What is here instead has one invariant, and it is enough: **an instance is replaced only after
/// its client has been read.** Four are armed at the start and the loop waits on all of them at
/// once, so at every instant at least three are listening; the one being served is the fourth, and
/// its slot is re-armed the moment its line has been taken. There is no ordering to get wrong,
/// because there is no chain.
///
/// Still one reader, which is what keeps two callers' frames from interleaving; [`READ_DEADLINE`]
/// is still what keeps one stalled caller from holding that reader.
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
    let mut buffer = vec![0u8; MAX_MESSAGE_BYTES];
    // The first instance carries the check that this name is ours, and its success is the word that
    // makes `start` mean what it says: a connect is outstanding on a pipe that exists, before
    // anybody is told the endpoint is open.
    let mut pool = match arm(&wide_name, &attributes, true) {
        Ok(instance) => {
            let _ = armed.send(Ok(()));
            vec![instance]
        }
        Err(error) => {
            let _ = armed.send(Err(error));
            return;
        }
    };
    while pool.len() < MAX_INSTANCES as usize {
        match arm(&wide_name, &attributes, false) {
            Ok(instance) => pool.push(instance),
            // Fewer than the full four is a smaller endpoint, not a broken one — one is enough to
            // work, and the rest are headroom.
            Err(_) => break,
        }
    }
    loop {
        // A slot whose client was already there when it was armed needs no wait at all.
        let served = match pool.iter().position(|instance| instance.ready) {
            Some(index) => index,
            None => {
                let mut handles = pool
                    .iter()
                    .map(|instance| instance.event.handle())
                    .collect::<Vec<_>>();
                handles.push(stop.0);
                // SAFETY: every handle in the list is owned by this thread's pool, or is the stop
                // event, which outlives the listener.
                let answer = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
                let Some(index) = answer.0.checked_sub(WAIT_OBJECT_0.0) else {
                    return;
                };
                let index = index as usize;
                if index >= pool.len() {
                    // The stop event, or a wait that failed. Either way this thread is finished,
                    // and every instance's `Drop` cancels and closes it.
                    return;
                }
                index
            }
        };
        match read_one(pool[served].pipe.0, stop, &mut buffer) {
            Frame::Line(bytes) => {
                let mut counts = counts.lock().unwrap_or_else(PoisonError::into_inner);
                if rate.admit(Instant::now()) {
                    counts.delivered += 1;
                    drop(counts);
                    deliver(String::from_utf8_lossy(bytes).into_owned());
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
            Frame::Stopped => return,
        }
        // **Only now.** The slot is re-armed after its client has been read, and the old instance
        // is dropped only once the new one is listening — so the number of instances answering this
        // name never falls, and no instance is ever closed with a line still in it.
        match arm(&wide_name, &attributes, false) {
            Ok(replacement) => pool[served] = replacement,
            // The kernel would not give another. Drop the spent slot rather than keep a dead one,
            // and carry on with a smaller pool; an endpoint that has run out entirely is finished.
            Err(_) => {
                pool.remove(served);
                if pool.is_empty() {
                    return;
                }
            }
        }
    }
}

enum Frame<'a> {
    Line(&'a [u8]),
    Oversize,
    Silent,
    Stopped,
}

/// Read one message from a connected client, or decide it is not going to send one.
fn read_one<'a>(pipe: HANDLE, stop: SendHandle, buffer: &'a mut [u8]) -> Frame<'a> {
    let Ok(event) = Overlapped::new() else {
        return Frame::Silent;
    };
    let mut overlapped = event.overlapped();
    // SAFETY: `buffer` and `overlapped` both outlive the wait below.
    let issued = unsafe { ReadFile(pipe, Some(buffer), None, Some(&raw mut overlapped)) };
    let pending = match issued {
        Ok(()) => false,
        Err(error) if win32_of(&error) == ERROR_IO_PENDING.0 => true,
        Err(error) if win32_of(&error) == ERROR_MORE_DATA.0 => {
            return Frame::Oversize;
        }
        Err(_) => return Frame::Silent,
    };
    if pending {
        match wait_for(event.handle(), stop, Some(READ_DEADLINE)) {
            Waited::Signalled => {}
            Waited::Stopped => {
                cancel(pipe);
                return Frame::Stopped;
            }
            Waited::TimedOut | Waited::Failed => {
                cancel(pipe);
                return Frame::Silent;
            }
        }
    }
    let mut read = 0u32;
    // SAFETY: the overlapped structure and the pipe are both still alive.
    let completed = unsafe {
        windows::Win32::System::IO::GetOverlappedResult(
            pipe,
            &raw const overlapped,
            &raw mut read,
            false,
        )
    };
    match completed {
        Ok(()) => {}
        Err(error) if win32_of(&error) == ERROR_MORE_DATA.0 => return Frame::Oversize,
        Err(_) => return Frame::Silent,
    }
    let read = read as usize;
    if read == 0 {
        return Frame::Silent;
    }
    Frame::Line(&buffer[..read.min(buffer.len())])
}

enum Waited {
    Signalled,
    Stopped,
    TimedOut,
    Failed,
}

fn wait_for(event: HANDLE, stop: SendHandle, deadline: Option<Duration>) -> Waited {
    let handles = [event, stop.0];
    let millis = deadline.map_or(INFINITE, |deadline| {
        u32::try_from(deadline.as_millis()).unwrap_or(INFINITE)
    });
    // SAFETY: both handles are alive for the duration of the wait.
    let answer = unsafe { WaitForMultipleObjects(&handles, false, millis) };
    if answer == WAIT_OBJECT_0 {
        Waited::Signalled
    } else if answer.0 == WAIT_OBJECT_0.0 + 1 {
        Waited::Stopped
    } else if answer == WAIT_TIMEOUT {
        Waited::TimedOut
    } else {
        Waited::Failed
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
    /// counter at zero.
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
        assert_eq!(pipe.counts().delivered, FRAMES as u64);
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
