//! **The second launch's door into the first** — one well-known named pipe per data directory,
//! and the other end of `folio.exe` being started while a `folio.exe` is already running
//! (`docs/DESIGN.md` §7.59).
//!
//! # Why a second pipe and not a second IPC design
//!
//! [`crate::attention_pipe`] already answers every question this channel asks — which principal may
//! connect, how long a stalled caller may hold the door, what a frame longer than the grammar is
//! worth — and it answers them in unsafe code that took three shapes to get right. So this module
//! **shares that module's primitives** rather than restating them: the descriptor, the owned
//! handle, the overlapped event, the bounded write, the client open with its impersonation level
//! and its one retry past a busy endpoint are all `attention_pipe`'s, lent out at `pub(crate)`.
//! What is written here is only the two things that are genuinely different.
//!
//! **It is well known rather than minted.** The attention endpoint's name carries a nonce because
//! the only callers that may find it are children this process told; the name below has to be
//! computed by a process that has never met this one, so there is nothing in it that either side
//! could not derive from the data directory alone — see [`endpoint_name`].
//!
//! **It answers.** The attention endpoint is inbound-only and deliberately has no reply channel: a
//! hook is ringing a doorbell and leaving. A launch is a person waiting for a window, and it needs
//! to know whether the running Folio took the request — because if it did not, this process has to
//! open a window itself rather than leave the person with nothing.
//!
//! # The conversation, in four steps, and the order is the whole of the safety
//!
//! 1. The client connects and writes **one** request frame.
//! 2. The server reads it, decides, and writes **one** reply frame carrying its own process id.
//! 3. The client reads the reply, grants that process id the foreground
//!    ([`crate::hotkey::allow_foreground_for`]), and closes the pipe.
//! 4. The server sees the close — or gives up waiting for it — and **only then** hands the request
//!    on to the window thread.
//!
//! Step 4 after step 3 is not tidiness. `SetForegroundWindow` is refused unless the process that
//! owns the foreground has said otherwise first, and the process that owns the foreground is the
//! one the user just started; a request acted on before its acknowledgement had reached that
//! process would open a tab in a window that could not come to the front. It also makes the reply
//! the **commit point**: a client that gave up and closed the pipe before the reply was written
//! leaves no request behind, which is what keeps [`HANDOVER_BUDGET`] from producing a window *and*
//! a tab.
//!
//! # What is promised
//!
//! The same two sentences the attention endpoint promises, for the same reason and by the same
//! means. Nothing outside this logon session can connect ([`crate::attention_pipe::
//! security_descriptor_sddl`]), and nothing off this machine can (`PIPE_REJECT_REMOTE_CLIENTS`).
//! And, as there: this is **not** a defence against a hostile process running as you. What such a
//! process could do with this channel is ask a Folio it could already have started to open a tab in
//! a folder it could already have named on a command line.

use std::{
    io,
    path::Path,
    sync::mpsc::{self, RecvTimeoutError},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use windows::Win32::{
    Foundation::{
        CloseHandle, ERROR_IO_PENDING, ERROR_MORE_DATA, ERROR_NO_DATA, ERROR_PIPE_BUSY,
        ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
        WAIT_OBJECT_0,
    },
    Security::SECURITY_ATTRIBUTES,
    Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX, ReadFile,
    },
    System::{
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_MESSAGE,
            PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PIPE_WAIT,
        },
        Threading::{CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects},
    },
};
use windows::core::PCWSTR;

use crate::attention_pipe::{
    MAX_MESSAGE_BYTES, Overlapped, OwnedHandle, SecurityDescriptor, SendHandle, cancel, logon_sid,
    open_client, security_descriptor_sddl, session_tag, wide, win32_io_error, win32_of,
    write_bounded,
};

/// **How long a second launch will wait for the running Folio before opening a window of its own.**
///
/// Two seconds, and the number is a product decision rather than a network estimate. The running
/// Folio answers this pipe from its **listener thread**, which does not touch the window thread —
/// so a Folio that is merely busy painting, reflowing a paste or rebuilding a device answers inside
/// a millisecond, and the only way to spend the whole of this budget is a process that is stuck as
/// a whole. `hang_watch` calls the window thread hung at five seconds; waiting that long here would
/// mean a person who double-clicked the taskbar icon watched nothing happen for five seconds, which
/// is a launch they will double-click again. Two seconds is three orders of magnitude of slack over
/// the answer this channel actually takes, and short enough that the fallback still reads as a slow
/// start rather than as a program that ignored them.
///
/// It bounds the **whole** handover — finding the door, writing the request, reading the answer —
/// because what a person is waiting for is a window, not a syscall.
pub const HANDOVER_BUDGET: Duration = Duration::from_secs(2);

/// How long the server holds one connection open waiting for a step of the conversation.
///
/// [`crate::attention_pipe`]'s own read deadline, for its reason: a client that connects and says
/// nothing holds the only listening instance, and this endpoint serves one caller at a time. It is
/// also what step 4 waits out — a client that neither closes nor speaks — and the request is still
/// committed then, because by that point the reply has been delivered and the person is owed the
/// window they asked for.
const STEP_DEADLINE_MS: u32 = 250;

/// **The endpoint's full name.**
///
/// Two segments, and neither of them is a secret: this name has to be computed by a process that
/// has never met the one listening, out of the one fact both of them start from — which data
/// directory this Folio is a Folio *of*.
///
/// * The **logon tag** is [`crate::attention_pipe::session_tag`]'s digest of the logon SID. It is
///   here because the pipe namespace has no `Local\`: the claim in [`crate::instance`] gets its
///   per-session isolation from the kernel by asking for it in the name, and a pipe has to write it
///   down. Without it two sessions of one user — a console and a remote desktop — would compute one
///   name for two `%APPDATA%` directories that happen to be the same directory, and the second
///   session's launch would be answered by a Folio on a desktop nobody is looking at.
/// * The **directory tag** is [`crate::instance::directory_tag`], which is the very folding the
///   mutex name is built out of. That is the point of it being shared: a test window run with
///   `APPDATA` pointed under a worktree claims a different mutex *and* addresses a different pipe,
///   so it can never speak to the reader's everyday Folio, and two spellings of one directory can
///   never fail to find each other.
#[must_use]
pub fn endpoint_name(session_tag: &str, directory_tag: &str) -> String {
    format!(r"\\.\pipe\folio-launch-{session_tag}-{directory_tag}")
}

/// The name this process's launch endpoint takes for `directory`, or `None` on a token with no
/// logon SID.
///
/// `None` is a real answer and not an error to paper over — it is [`crate::attention_pipe`]'s own
/// rule, and it has the same consequence here: a process that cannot name its own logon session
/// cannot write a descriptor that grants it, and **a missing logon SID must never fall back to a
/// default descriptor**. A launch that cannot find the door opens its own window, which is exactly
/// what every Folio did before this channel existed.
#[must_use]
pub fn endpoint_for(directory: &Path) -> Option<String> {
    let logon = logon_sid()?;
    Some(endpoint_name(
        &session_tag(&logon),
        &crate::instance::directory_tag(directory),
    ))
}

/// **The endpoint.** Live from the moment [`LaunchPipe::start`] returns, closed when this is
/// dropped.
pub struct LaunchPipe {
    name: String,
    stop: SendHandle,
    listener: Option<JoinHandle<()>>,
}

// SAFETY: the only raw handle in this value is the stop event. Setting an event is thread-safe by
// contract and is the only thing done with it before `drop`, which runs once on the sole owner and
// joins the listener before closing it.
unsafe impl Sync for LaunchPipe {}

impl LaunchPipe {
    /// Open this process's launch endpoint for `directory` and start listening.
    ///
    /// **It returns already listening**, which is `CONVENTIONS.md`'s rule for anything shaped like
    /// a subscription and is load-bearing here for a specific reason: the process is about to
    /// finish starting, and a second launch fired at it in that window would find no pipe and open
    /// a window of its own. So the listener thread issues its first `ConnectNamedPipe` and says so,
    /// and this waits for that word — or hands back the refusal instead of a thread that dies in
    /// private.
    ///
    /// The two closures are the two halves of step 2 and step 4, and they are separate because the
    /// order between them is the contract this module exists to keep:
    ///
    /// * `decide` is called **on the listener thread** with the request line exactly as it arrived.
    ///   It answers the reply line to write back, or `None` for a line that is not a request at all
    ///   — which is dropped without a word and without effect, because there is nobody on the other
    ///   end who would understand one.
    /// * `commit` is called **after** that reply has reached the client and the client has let go.
    ///   It is expected to do nothing but park the request and nudge the loop that will act on it.
    ///
    /// The listener is not given the request's grammar, its bounds or its meaning — this module
    /// knows nothing about what crosses it, which is what keeps a message-format change out of the
    /// unsafe boundary.
    pub fn start<D, C>(directory: &Path, decide: D, commit: C) -> io::Result<Self>
    where
        D: Fn(&str) -> Option<String> + Send + 'static,
        C: Fn(&str) + Send + 'static,
    {
        let Some(logon) = logon_sid() else {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "this process's token carries no logon SID, so the launch endpoint has no \
                 principal to grant and will not be opened with a default descriptor",
            ));
        };
        let name = endpoint_name(
            &session_tag(&logon),
            &crate::instance::directory_tag(directory),
        );
        let descriptor = SecurityDescriptor::from_sddl(&security_descriptor_sddl(&logon))?;
        // Manual-reset: the listener may be anywhere between two waits when `drop` fires, so once
        // this is set it has to stay set. Owned from the moment it exists — the spawn below can
        // fail, and a stop event held only as a `Copy` integer would be a kernel object this
        // process never closes on the one path nobody exercises.
        // SAFETY: a nameless, unowned event.
        let stop = OwnedHandle(
            unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(win32_io_error)?,
        );
        let stop_for_thread = SendHandle(stop.0);
        let (armed, first_word) = mpsc::channel::<io::Result<()>>();
        let listener = {
            let name = name.clone();
            std::thread::Builder::new()
                .name("folio-launch-endpoint".to_owned())
                .spawn(move || {
                    listen(&name, descriptor, stop_for_thread, &armed, &decide, &commit);
                })?
        };
        // The listener is running and shares the event, so the guard's job is over: from here the
        // handle is closed by whichever of the three arms below is taken, and by `Drop` on the arm
        // that hands the endpoint back.
        let stop = SendHandle(stop.into_raw());
        match first_word.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                name,
                stop,
                listener: Some(listener),
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
                Err(io::Error::other("the launch endpoint never came up"))
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

/// One connection at a time, for as long as the endpoint is open.
///
/// **One instance and not four**, which is the one place this listener is deliberately smaller than
/// the attention endpoint's. That one serves a hook that fires several times a turn while an agent
/// is running; this one serves a person double-clicking an icon. A caller that arrives while
/// another is being served finds the instance busy and waits for it — [`hand_over`] retries until
/// its budget is spent — and being served takes microseconds, because nothing in the transaction
/// touches the window thread.
fn listen(
    name: &str,
    descriptor: SecurityDescriptor,
    stop: SendHandle,
    armed: &mpsc::Sender<io::Result<()>>,
    decide: &(impl Fn(&str) -> Option<String> + ?Sized),
    commit: &(impl Fn(&str) + ?Sized),
) {
    let attributes = descriptor.attributes();
    let wide_name = wide(name);
    let mut instance = match Instance::open(&wide_name, &attributes) {
        Ok(instance) => {
            let _ = armed.send(Ok(()));
            instance
        }
        Err(error) => {
            let _ = armed.send(Err(error));
            return;
        }
    };
    loop {
        match instance.wait_for_a_caller(stop) {
            Caller::Stopped => return,
            Caller::Here => {}
        }
        serve(&mut instance, decide, commit);
        if instance.recycle().is_err() {
            return;
        }
    }
}

/// **One whole conversation**, steps 1 to 4, and every one of them bounded.
///
/// Nothing here reports a failure anywhere: there is no log a launch would look in and no reader to
/// tell. A step that does not complete ends the connection, and the client's own fallback — a
/// window of its own — is the report.
fn serve(
    instance: &mut Instance,
    decide: &(impl Fn(&str) -> Option<String> + ?Sized),
    commit: &(impl Fn(&str) + ?Sized),
) {
    let Ok(read) = instance.read_one() else {
        return;
    };
    let line = String::from_utf8_lossy(&instance.buffer[..read]).into_owned();
    // **A line this build does not understand is dropped without a word** — the attention wire's
    // founding rule at the second door. There is no reply that would help: a caller speaking a
    // grammar this build has not got is not a launch that arrived slightly wrong.
    let Some(reply) = decide(&line) else {
        return;
    };
    if write_bounded(instance.pipe.0, reply.as_bytes()).is_err() {
        // **The reply is the commit point.** A write that did not land is a client that has already
        // given up and gone, and a request committed for it would be the tab that arrives beside
        // the window the client opened instead.
        return;
    }
    // Step 3 happening on the other side: the client grants this process the foreground and lets
    // go. The read is expected to fail — that failure *is* the client closing — and its deadline is
    // what keeps a client that does neither from holding the door.
    let _ = instance.read_one();
    commit(&line);
}

/// One instance of the endpoint: a handle, one event, one operation at a time.
struct Instance {
    pipe: OwnedHandle,
    event: Overlapped,
    /// The kernel owns this address for as long as an operation is outstanding, which is why it is
    /// boxed and reset rather than rebuilt.
    overlapped: Box<OVERLAPPED>,
    buffer: Vec<u8>,
    /// **Whether the kernel still owns [`Self::overlapped`] and [`Self::buffer`]**, which is
    /// `attention_pipe`'s R2-1 held at this door: `CancelIoEx` asks for a cancellation and does not
    /// deliver one, so until `GetOverlappedResult` says the operation is over both are addresses
    /// the kernel may still write to.
    outstanding: bool,
}

impl Drop for Instance {
    fn drop(&mut self) {
        cancel(self.pipe.0);
        // **Before the buffer and the structure are freed** (R2-1).
        self.settle();
        // SAFETY: this instance owns the handle; disconnecting an unconnected pipe is a no-op.
        unsafe {
            let _ = DisconnectNamedPipe(self.pipe.0);
        }
    }
}

/// What the wait at the top of one turn ended on.
enum Caller {
    Here,
    Stopped,
}

impl Instance {
    /// Create the instance and put the first connect on it.
    ///
    /// `FILE_FLAG_FIRST_PIPE_INSTANCE` is a **check** rather than a flag: it fails if the name
    /// already exists, so a process that squatted this name before us cannot end up being the thing
    /// a second launch hands its command line to. This is the one endpoint where that matters most
    /// — the name is public by construction.
    fn open(wide_name: &[u16], attributes: &SECURITY_ATTRIBUTES) -> io::Result<Self> {
        // SAFETY: `wide_name` is NUL-terminated and outlives the call; `attributes` points at a
        // descriptor the caller keeps alive for the whole of the listener.
        let pipe = unsafe {
            CreateNamedPipeW(
                PCWSTR(wide_name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                u32::try_from(MAX_MESSAGE_BYTES).unwrap_or(0),
                u32::try_from(MAX_MESSAGE_BYTES).unwrap_or(0),
                0,
                Some(&raw const *attributes),
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // **Owned before the next thing that can fail**: `Overlapped::new` creates a kernel object
        // and can refuse, and a handle still living in a local at that moment is one the `?` walks
        // away from.
        let pipe = OwnedHandle(pipe);
        let event = Overlapped::new()?;
        let overlapped = Box::new(event.overlapped());
        let mut instance = Self {
            pipe,
            event,
            overlapped,
            buffer: vec![0u8; MAX_MESSAGE_BYTES],
            outstanding: false,
        };
        instance.arm_connect()?;
        Ok(instance)
    }

    /// Put a connect on this instance.
    ///
    /// The three "somebody is already here" answers are `attention_pipe::Instance::arm_connect`'s
    /// and are kept for its reason: an instance starts listening when `CreateNamedPipeW` returns
    /// rather than when `ConnectNamedPipe` is called, so a client can arrive in the window between
    /// them, and `ERROR_NO_DATA` means one arrived and has already gone — with its bytes still in
    /// the buffer, readable until somebody disconnects.
    fn arm_connect(&mut self) -> io::Result<()> {
        self.reset();
        // SAFETY: the boxed `OVERLAPPED` lives at a fixed address for as long as this instance
        // does, and this instance outlives the operation — `Drop` cancels anything still pending.
        let issued = unsafe { ConnectNamedPipe(self.pipe.0, Some(&raw mut *self.overlapped)) };
        match issued {
            Ok(()) => Ok(()),
            Err(error)
                if win32_of(&error) == ERROR_PIPE_CONNECTED.0
                    || win32_of(&error) == ERROR_NO_DATA.0 =>
            {
                // Already attached, so nothing is outstanding and the event will never be
                // signalled. Signalling it by hand is what makes the wait below one wait rather
                // than two shapes of one.
                // SAFETY: this instance owns the event.
                unsafe {
                    let _ = SetEvent(self.event.handle());
                }
                Ok(())
            }
            Err(error) if win32_of(&error) == ERROR_IO_PENDING.0 => {
                self.outstanding = true;
                Ok(())
            }
            Err(error) => Err(win32_io_error(error)),
        }
    }

    /// Wait until somebody connects, or until the endpoint is told to stop.
    fn wait_for_a_caller(&mut self, stop: SendHandle) -> Caller {
        let handles = [self.event.handle(), stop.0];
        // SAFETY: the first handle is this instance's own and the second outlives the listener.
        let answer = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
        if answer != WAIT_OBJECT_0 {
            return Caller::Stopped;
        }
        self.settle();
        Caller::Here
    }

    /// **One message off the wire, under [`STEP_DEADLINE_MS`].**
    ///
    /// A message-mode read, so "there was more" is a distinguishable answer rather than a silently
    /// truncated line: `ERROR_MORE_DATA` is refused outright, because a half-read frame is exactly
    /// the kind of thing a parser should never be handed.
    fn read_one(&mut self) -> io::Result<usize> {
        self.reset();
        let mut read = 0u32;
        // SAFETY: the buffer and the boxed structure both outlive this call, which does not return
        // until the operation has been collected below.
        let issued = unsafe {
            ReadFile(
                self.pipe.0,
                Some(self.buffer.as_mut_slice()),
                Some(&raw mut read),
                Some(&raw mut *self.overlapped),
            )
        };
        match issued {
            Ok(()) => return Ok(read as usize),
            Err(error) if win32_of(&error) == ERROR_IO_PENDING.0 => self.outstanding = true,
            Err(error) => return Err(win32_io_error(error)),
        }
        let handles = [self.event.handle()];
        // SAFETY: the event belongs to this instance and outlives the wait.
        let answer = unsafe { WaitForMultipleObjects(&handles, false, STEP_DEADLINE_MS) };
        let timed_out = answer != WAIT_OBJECT_0;
        if timed_out {
            cancel(self.pipe.0);
        }
        // SAFETY: the handle and the structure are both still this instance's, and `bWait` is what
        // makes the buffer this instance's again on the way out.
        let done = unsafe {
            GetOverlappedResult(
                self.pipe.0,
                &raw const *self.overlapped,
                &raw mut read,
                true,
            )
        };
        self.outstanding = false;
        if timed_out {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the caller held the launch endpoint without finishing its turn",
            ));
        }
        match done {
            Ok(()) if read > 0 => Ok(read as usize),
            // A connection that closed with nothing on it, and a frame longer than this endpoint
            // will take, are both "there is no request here" — and neither is answered, because
            // there is nothing left to answer on.
            Ok(()) => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the caller connected and said nothing",
            )),
            Err(error) if win32_of(&error) == ERROR_MORE_DATA.0 => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a frame longer than the launch endpoint's bound",
            )),
            Err(error) => Err(win32_io_error(error)),
        }
    }

    /// Put this instance back to listening for the next launch.
    fn recycle(&mut self) -> io::Result<()> {
        cancel(self.pipe.0);
        self.settle();
        // SAFETY: this instance owns the handle.
        unsafe {
            let _ = DisconnectNamedPipe(self.pipe.0);
        }
        self.arm_connect()
    }

    /// **Wait out whatever the kernel still owns** (R2-1), so the buffer and the structure are this
    /// process's again before either is reused or freed.
    fn settle(&mut self) {
        if !self.outstanding {
            return;
        }
        let mut moved = 0u32;
        // SAFETY: the handle and the structure are this instance's, and `bWait` is the whole point.
        let _ = unsafe {
            GetOverlappedResult(
                self.pipe.0,
                &raw const *self.overlapped,
                &raw mut moved,
                true,
            )
        };
        self.outstanding = false;
    }

    /// Ready the event and the structure for one more operation.
    fn reset(&mut self) {
        self.settle();
        *self.overlapped = self.event.overlapped();
        // SAFETY: this instance owns the event; a manual-reset event is reset by hand or not at
        // all, and every operation below waits on it.
        unsafe {
            let _ = ResetEvent(self.event.handle());
        }
    }
}

/// **A second launch's whole conversation with the Folio that is already running.**
///
/// Connect, write the request, read the answer, let the caller act on it, close. Bounded end to end
/// by [`HANDOVER_BUDGET`]: every step of it talks to a process that may be stuck, and a launch that
/// hung here would be a person double-clicking an icon and watching nothing happen at all.
///
/// **`on_reply` runs while the pipe is still open**, and that is the whole reason it is a callback
/// rather than a return value. What the caller does with the answer is grant the running process
/// the foreground, and the running process does not act on the request until it has seen this end
/// let go — so the grant has to be made *inside* the conversation, not after it.
///
/// The errors are the caller's fallback and not a report to anyone: `NotFound` is nobody listening
/// on that name, `TimedOut` is the budget spent, and either way the answer is the same — this
/// process opens the window itself, exactly as every Folio did before this channel existed.
pub fn hand_over(endpoint: &str, request: &str, on_reply: impl FnOnce(&str)) -> io::Result<()> {
    if request.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "request longer than the launch endpoint's frame bound",
        ));
    }
    let began = Instant::now();
    let wide_name = wide(endpoint);
    let handle = loop {
        match open_client(&wide_name, GENERIC_READ.0 | GENERIC_WRITE.0) {
            Ok(handle) => break handle,
            // **Busy is not refused, it is waited on.** The endpoint serves one caller at a time,
            // so two launches in the same instant is an ordinary collision rather than an error;
            // `open_client` has already spent its own hundred milliseconds inside
            // `WaitNamedPipeW` before answering this, so the loop is a retry and not a spin.
            Err(error)
                if matches!(error.kind(), io::ErrorKind::WouldBlock)
                    || error.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) =>
            {
                if began.elapsed() >= HANDOVER_BUDGET {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "the running Folio stayed busy for the whole of the launch's allowance",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
    };
    write_bounded(handle.0, request.as_bytes())?;
    let mut buffer = vec![0u8; MAX_MESSAGE_BYTES];
    let left = HANDOVER_BUDGET.saturating_sub(began.elapsed());
    let read = read_reply(handle.0, &mut buffer, left)?;
    on_reply(&String::from_utf8_lossy(&buffer[..read]));
    // The close is step 3's second half and is what the server is waiting for. Written out rather
    // than left to the end of the function so that the order of the last two statements is the
    // order of the protocol.
    drop(handle);
    Ok(())
}

/// **One reply off the wire, under whatever is left of the budget.**
///
/// Overlapped for the reason [`crate::attention_pipe`]'s write is: a synchronous read on a pipe
/// returns when the other end speaks and there is no argument that says otherwise, and the other
/// end here is a process that may be stuck. A read that runs out of time is cancelled and then
/// **collected** — the buffer is this stack's and the kernel has to be finished with it before this
/// returns.
fn read_reply(pipe: HANDLE, buffer: &mut [u8], budget: Duration) -> io::Result<usize> {
    let event = Overlapped::new()?;
    let mut overlapped = Box::new(event.overlapped());
    let mut read = 0u32;
    // SAFETY: the buffer and the boxed structure both outlive this function, which does not return
    // until the operation has been collected below.
    let issued = unsafe {
        ReadFile(
            pipe,
            Some(buffer),
            Some(&raw mut read),
            Some(&raw mut *overlapped),
        )
    };
    match issued {
        Ok(()) => return Ok(read as usize),
        Err(error) if win32_of(&error) == ERROR_IO_PENDING.0 => {}
        Err(error) => return Err(win32_io_error(error)),
    }
    let handles = [event.handle()];
    // Rounded **up**: a budget of 1999.6 ms waited as 1999 ms is a launch that gave up a hair
    // before its allowance, and the allowance is the one number this module promises.
    let milliseconds = u32::try_from(
        budget.as_millis() + u128::from(!budget.subsec_nanos().is_multiple_of(1_000_000)),
    )
    .unwrap_or(u32::MAX);
    // SAFETY: the event belongs to this call and outlives the wait.
    let answer = unsafe { WaitForMultipleObjects(&handles, false, milliseconds) };
    let timed_out = answer != WAIT_OBJECT_0;
    if timed_out {
        // SAFETY: cancelling this thread's own outstanding operation on a handle it owns.
        unsafe {
            let _ = CancelIoEx(pipe, Some(&raw const *overlapped));
        }
    }
    // SAFETY: the handle and the structure are both still this call's, and `bWait` is what makes
    // the buffer the caller's again on the way out.
    let done = unsafe { GetOverlappedResult(pipe, &raw const *overlapped, &raw mut read, true) };
    if timed_out {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "the running Folio did not answer inside the launch's allowance",
        ));
    }
    done.map_err(win32_io_error)?;
    Ok(read as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// **The two names one directory produces are folded the same way.**
    ///
    /// The whole of the isolation promise, and it is a property of two strings rather than of a
    /// kernel: a test window run with `APPDATA` pointed under a worktree must miss the reader's
    /// everyday Folio at *both* doors, and two spellings of one directory must find each other at
    /// both. The red form is a second folding written out in this file — case, or a trailing
    /// separator, handled differently here than in `instance` — and it would show up as a launch
    /// that took the claim of one directory and the pipe of another.
    ///
    /// MUTATION: build the tag out of the path itself instead of `instance::directory_tag`, and the
    /// two spellings below stop agreeing.
    #[test]
    fn one_directory_is_one_claim_and_one_endpoint_however_it_is_spelled() {
        let tag = crate::instance::directory_tag;
        let one = endpoint_name(
            "abcdef0123456789",
            &tag(Path::new(r"C:\Users\Me\AppData\Folio")),
        );
        let same = endpoint_name(
            "abcdef0123456789",
            &tag(Path::new(r"c:\users\me\appdata\folio\")),
        );
        let other = endpoint_name("abcdef0123456789", &tag(Path::new(r"D:\wt\isolated\Folio")));
        assert_eq!(
            one, same,
            "case and a trailing separator are not a difference"
        );
        assert_ne!(
            one, other,
            "a window under an isolated APPDATA must not find the reader's own Folio"
        );
        assert_ne!(
            one,
            endpoint_name(
                "fedcba9876543210",
                &tag(Path::new(r"C:\Users\Me\AppData\Folio"))
            ),
            "two logon sessions of one user share a directory and must not share an endpoint"
        );
        assert!(one.starts_with(r"\\.\pipe\folio-launch-"), "{one}");
        assert_eq!(
            crate::instance::claim_name(Path::new(r"C:\Users\Me\AppData\Folio")),
            crate::instance::claim_name(Path::new(r"c:\users\me\appdata\folio\")),
            "and the claim the endpoint stands beside folds the same way"
        );
    }

    /// **The descriptor this endpoint is opened with is the attention endpoint's.**
    ///
    /// Asserted here as well as there, because a second channel is exactly where a second, weaker
    /// answer to "who may connect" gets written by accident — and this one's name is *public* by
    /// construction, so the DACL is the whole of its boundary.
    #[test]
    fn the_launch_endpoint_grants_the_logon_session_and_nothing_else() {
        let sddl = security_descriptor_sddl("S-1-5-5-0-1234567");
        assert_eq!(sddl, "D:P(A;;GA;;;S-1-5-5-0-1234567)");
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

    /// A directory no other test in this process is using, so two of them can run at once.
    fn scratch(line: u32) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("bt-platform-launch-{}-{line}", std::process::id()))
    }

    /// **RED — one request crosses, and it is answered.**
    ///
    /// End to end against the real kernel object, because what is worth pinning is a property of
    /// the real one: that a client can connect the instant `start` hands back, that the bytes
    /// arrive whole, and that the reply written on the listener thread reaches the caller. Before
    /// this slice there was no second pipe at all and `hand_over` had nothing to open.
    #[test]
    fn a_request_crosses_the_launch_endpoint_and_is_answered() {
        let directory = scratch(line!());
        let (sender, committed) = mpsc::channel();
        let pipe = LaunchPipe::start(
            &directory,
            |line| Some(format!("answer to {line}")),
            move |line| {
                let _ = sender.send(line.to_owned());
            },
        )
        .expect("open the launch endpoint");
        let (heard, replies) = mpsc::channel();
        hand_over(pipe.name(), "one request", |reply| {
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

    /// **RED — a line this build does not understand is dropped without a word and without
    /// effect.**
    ///
    /// The attention wire's founding rule at the second door. The client is told nothing, because
    /// there is nothing to tell a caller speaking a grammar this build has not got — and, more to
    /// the point, **nothing is committed**: a refused line must not reach the window thread.
    ///
    /// MUTATION: commit before the reply is written, or commit on a `None` verdict, and the second
    /// assertion goes red.
    #[test]
    fn a_line_the_grammar_refuses_is_dropped_without_a_reply_and_without_effect() {
        let directory = scratch(line!());
        let (sender, committed) = mpsc::channel();
        let pipe = LaunchPipe::start(
            &directory,
            |line| (line == "good").then(|| "ok".to_owned()),
            move |line| {
                let _ = sender.send(line.to_owned());
            },
        )
        .expect("open the launch endpoint");
        let mut answered = false;
        let _ = hand_over(pipe.name(), "rubbish", |_| answered = true);
        assert!(!answered, "a refused line is answered with silence");
        assert!(
            committed.recv_timeout(Duration::from_millis(500)).is_err(),
            "and it never reaches the window thread"
        );
    }

    /// **RED — a request longer than the endpoint's frame bound never leaves this process.**
    ///
    /// Checked on the way out as well as on the way in, which is `attention_pipe`'s own rule: this
    /// end has no reason to trust that the far end applied it, and neither has that one.
    #[test]
    fn a_request_past_the_frame_bound_is_refused_before_it_is_written() {
        let oversized = "x".repeat(MAX_MESSAGE_BYTES + 1);
        let refused = hand_over(r"\\.\pipe\folio-launch-nobody", &oversized, |_| {
            panic!("an oversized request must not reach a pipe at all");
        })
        .expect_err("an oversized request is refused");
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);
    }

    /// **RED — a Folio that takes the request and never answers costs the launch its budget and
    /// nothing more.**
    ///
    /// The hung-server fallback, which is the whole reason [`HANDOVER_BUDGET`] is a number rather
    /// than an `INFINITE`: the caller has to get an answer it can act on — and the action is to
    /// open its own window — instead of standing at a pipe held by a process that has stopped.
    ///
    /// MUTATION: pass `INFINITE` to the wait in `read_reply` and this test never returns.
    #[test]
    fn a_server_that_never_answers_costs_the_launch_its_budget_and_no_more() {
        let directory = scratch(line!());
        let pipe = LaunchPipe::start(
            &directory,
            |_| {
                std::thread::sleep(HANDOVER_BUDGET * 3);
                Some("far too late".to_owned())
            },
            |_| {},
        )
        .expect("open the launch endpoint");
        let began = Instant::now();
        let refused = hand_over(pipe.name(), "one request", |_| {
            panic!("a hung Folio has not answered");
        })
        .expect_err("a Folio that never answers cannot be handed anything");
        assert_eq!(refused.kind(), io::ErrorKind::TimedOut);
        // **One timer tick of slack.** The kernel wait counts in interrupt time and may return up
        // to a tick (15.6 ms on a machine nobody has asked for a finer clock) before the
        // monotonic clock this test reads says the budget is spent; a CI runner is such a
        // machine. The promise under test is that the launch does not give up *early*, and a
        // tick is not early — it is the resolution of the wait.
        const TICK: Duration = Duration::from_millis(20);
        assert!(
            began.elapsed() + TICK >= HANDOVER_BUDGET,
            "the budget is waited out before the launch gives up"
        );
        assert!(
            began.elapsed() < HANDOVER_BUDGET * 3,
            "and it is not waited past: the person is owed a window, not a syscall"
        );
    }

    /// **RED — a name with nobody behind it is a refusal and not a wait.**
    ///
    /// The ordinary case of a first launch: there is no Folio running, so there is no pipe, and the
    /// answer has to come back immediately rather than after [`HANDOVER_BUDGET`] — a first launch
    /// must not pay two seconds for the existence of this feature.
    #[test]
    fn an_endpoint_nobody_is_listening_on_refuses_at_once() {
        let began = Instant::now();
        let refused = hand_over(
            &endpoint_name("0123456789abcdef", "fedcba9876543210"),
            "one request",
            |_| panic!("there is nobody to answer"),
        )
        .expect_err("a name with nobody behind it cannot be handed anything");
        assert!(
            began.elapsed() < HANDOVER_BUDGET,
            "a launch with no Folio running must not wait out the budget"
        );
        assert_ne!(refused.kind(), io::ErrorKind::TimedOut);
    }
}
