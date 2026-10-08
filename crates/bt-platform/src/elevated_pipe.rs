//! **The elevated pane's pipe and launch primitives** (design note
//! `docs/plans/design/admin-terminal-2026-10-04.md` §2, spikes report decisions
//! (c) and (d)).
//!
//! The unelevated resident is always the server and the elevated host always
//! the client. The resident creates one attempt's pipe before it asks Windows
//! to elevate ([`ElevatedEndpoint::create`]), starts the host through
//! `ShellExecuteExW` with the verb `runas` ([`launch_elevated_host`]), and then
//! admits only the process that call returned ([`ElevatedEndpoint::accept`]).
//! The host opens the pipe named on its command line and admits only the
//! process named there as its parent ([`connect_to_parent`]).
//!
//! # What admits a peer
//!
//! * **The descriptor**: a protected DACL granting the parent's logon SID and
//!   Builtin Administrators read and write, nothing else. Administrators are needed because a
//!   standard user may answer UAC with another administrator's credentials, and
//!   that host's token carries neither the parent's logon SID nor its user.
//! * **The kernel's pipe peer ids**: `GetNamedPipeClientProcessId` must equal
//!   the pid `ShellExecuteExW` returned, and `GetNamedPipeServerProcessId` must
//!   equal the parent pid on the host's command line. These are correct across
//!   integrity levels and accounts; token integrity and `OpenProcess` identity
//!   are not used for the handshake.
//! * **The capability**: 256 bits drawn for one attempt, repeated in `Hello`
//!   and in `Authenticate`. The checks are the pure session model's
//!   (`crate::elevated_protocol::transition_parent` / `transition_host`), which
//!   moves the capability out of its state at the first `Hello`.
//!
//! `PIPE_REJECT_REMOTE_CLIENTS` keeps the pipe off the network and
//! `FILE_FLAG_FIRST_PIPE_INSTANCE` refuses a name somebody already holds. The
//! host opens the pipe at the identification level, so the medium-integrity
//! parent can read who connected and cannot act as the elevated token.
//!
//! This is not a defence against a hostile process already running as the same
//! user or as an administrator.
//!
//! # Deadlines
//!
//! Every wait takes the attempt's one absolute deadline (`LAUNCH_TIMEOUT`
//! after launch). The parent's waits also end when the launched host process
//! ends, so a host that dies before it connects fails the attempt at once.

use std::{ffi::OsStr, os::windows::ffi::OsStrExt as _, path::Path, time::Instant};

use windows::Win32::{
    Foundation::{
        ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_MORE_DATA, ERROR_NO_DATA,
        ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, GENERIC_READ,
        GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    },
    Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom},
    Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX, ReadFile,
        WriteFile,
    },
    System::{
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId,
            GetNamedPipeServerProcessId, PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_TYPE_MESSAGE, PIPE_WAIT, SetNamedPipeHandleState,
        },
        Threading::{GetProcessId, WaitForMultipleObjects},
    },
    UI::{
        Shell::{
            SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
            ShellExecuteExW,
        },
        WindowsAndMessaging::SW_HIDE,
    },
};
use windows::core::PCWSTR;

use crate::admission::WorkerCtx;
use crate::attention_pipe::{
    Overlapped, OwnedHandle, SecurityDescriptor, logon_sid, open_client, session_tag, wide,
    win32_of,
};
use crate::elevated_protocol::{
    CONTROL_MAX_PAYLOAD, Capability, Effect, EndpointError, FRAME_HEADER_LENGTH, Frame,
    HandshakeFailure, HandshakePhase, HostEvent, HostLine, HostProcessId, HostState, LaunchRefusal,
    PaneFailure, ParentEvent, ParentStartId, ParentState, ReaderRole, decode,
    elevated_endpoint_name, encode, transition_host, transition_parent,
};

/// Each direction's pipe buffer, the largest output frame plus its header.
const PIPE_BUFFER_BYTES: u32 = 64 * 1024 + 32;

/// The largest message a reader assembles: one control frame.
const MAX_MESSAGE_BYTES: usize = FRAME_HEADER_LENGTH + CONTROL_MAX_PAYLOAD;

/// One read's chunk; a longer message continues through `ERROR_MORE_DATA`.
const READ_CHUNK_BYTES: usize = 4096;

/// The descriptor of an elevated pane's pipe: protected, the parent's logon
/// SID and Builtin Administrators, read and write, nothing else.
///
/// Both entries admit only the connecting host, which opens the pipe for
/// reading and writing; the parent's own handle comes from creating the only
/// instance and is not granted through this list. Neither entry carries
/// `WRITE_DAC` or `WRITE_OWNER`. `P` is what design note §2 names; a pipe
/// inherits nothing, so it changes no access here.
#[must_use]
pub fn elevated_descriptor_sddl(logon_sid: &str) -> String {
    format!("D:P(A;;GRGW;;;{logon_sid})(A;;GRGW;;;BA)")
}

/// **One attempt's listening pipe**, created before the host is launched.
pub struct ElevatedEndpoint {
    line: HostLine,
    pipe: OwnedHandle,
}

impl ElevatedEndpoint {
    /// Mint the attempt's name and capability and create the pipe's only
    /// instance. It listens from the moment this returns: a client that
    /// connects before [`ElevatedEndpoint::accept`] is waiting is admitted by it.
    pub fn create() -> Result<Self, EndpointError> {
        let logon = logon_sid().ok_or(EndpointError::NoLogonSid)?;
        let parent_pid = std::process::id();
        let parent_start =
            crate::install_flip::started_of(parent_pid).ok_or_else(|| EndpointError::Os {
                call: "GetProcessTimes",
                code: last_error(),
            })?;
        let name = elevated_endpoint_name(&session_tag(&logon), parent_pid, random_bytes()?);
        let capability = Capability::new(random_bytes()?);
        let pipe = create_first_instance(&name, &elevated_descriptor_sddl(&logon))?;
        Ok(Self {
            line: HostLine {
                pipe_name: name,
                parent_pid,
                parent_start: ParentStartId(parent_start),
                capability,
            },
            pipe,
        })
    }

    /// The words the host is started with.
    #[must_use]
    pub fn host_line(&self) -> &HostLine {
        &self.line
    }

    /// **Admit the launched host, and only it.** Waits for a connection, asks
    /// the kernel which process made it, reads `Hello`, answers
    /// `Authenticate`. Consumes the endpoint: an attempt is accepted once.
    pub fn accept(
        self,
        host: &LaunchedHost,
        deadline: Instant,
    ) -> Result<ParentConnection, HandshakeFailure> {
        connect(&self.pipe, deadline, Some(host.process.0))?;
        self.accept_connected(host, deadline)
    }

    /// [`ElevatedEndpoint::accept`] after the connection has been made.
    fn accept_connected(
        self,
        host: &LaunchedHost,
        deadline: Instant,
    ) -> Result<ParentConnection, HandshakeFailure> {
        let Self { line, pipe } = self;
        let actual = peer_process_id(&pipe, Peer::Client)?;
        if actual != host.pid {
            return Err(HandshakeFailure::WrongPeer {
                expected: host.pid,
                actual,
            });
        }
        let state = ParentState::new(line.capability, HostProcessId(host.pid), line.parent_start);
        let hello = read_frame(
            &pipe,
            ReaderRole::Parent,
            deadline,
            Some(host.process.0),
            HandshakePhase::Hello,
        )?;
        let (state, effects) = transition_parent(state, ParentEvent::Receive(hello));
        let ready = matches!(state, ParentState::Ready { generation: 0 });
        settle_handshake(
            &pipe,
            effects,
            ready,
            deadline,
            Some(host.process.0),
            HandshakePhase::Hello,
        )?;
        Ok(ParentConnection {
            pipe,
            host_pid: host.pid,
            state,
        })
    }
}

/// The parent's authenticated pipe to its host.
pub struct ParentConnection {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "T-ADMIN-4 until 2026-11-30: the resident's relay reads and writes this pipe"
        )
    )]
    pipe: OwnedHandle,
    host_pid: u32,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "T-ADMIN-4 until 2026-11-30: the resident's relay continues the \
                      authenticated session from this state"
        )
    )]
    state: ParentState,
}

impl ParentConnection {
    /// The host's process id, as both the launch and the kernel named it.
    #[must_use]
    pub fn host_pid(&self) -> u32 {
        self.host_pid
    }
}

/// The host's authenticated pipe to its parent.
pub struct HostConnection {
    #[expect(
        dead_code,
        reason = "T-ADMIN-4 until 2026-11-30: the host's relay reads and writes this pipe"
    )]
    pipe: OwnedHandle,
    #[expect(
        dead_code,
        reason = "T-ADMIN-4 until 2026-11-30: the host's relay continues the authenticated \
                  session from this state"
    )]
    state: HostState,
}

/// **The host side of the handshake.** Opens the named pipe at the
/// identification level, asks the kernel which process is serving it, says
/// `Hello`, and admits only the `Authenticate` that repeats the line's
/// capability and parent start identity.
pub fn connect_to_parent(
    line: &HostLine,
    deadline: Instant,
) -> Result<HostConnection, HandshakeFailure> {
    let pipe =
        open_client(&wide(&line.pipe_name), (GENERIC_READ | GENERIC_WRITE).0).map_err(|error| {
            match error.raw_os_error().map(|code| code as u32) {
                Some(code) if code == ERROR_FILE_NOT_FOUND.0 => HandshakeFailure::NoEndpoint,
                // `open_client` answers without an OS code only when the pipe stayed busy.
                code => HandshakeFailure::Os {
                    call: "CreateFileW",
                    code: code.unwrap_or(ERROR_PIPE_BUSY.0),
                },
            }
        })?;
    let mode = PIPE_READMODE_MESSAGE;
    // SAFETY: `pipe` is this call's client handle and `mode` a live local.
    unsafe { SetNamedPipeHandleState(pipe.0, Some(&raw const mode), None, None) }.map_err(
        |error| HandshakeFailure::Os {
            call: "SetNamedPipeHandleState",
            code: win32_of(&error),
        },
    )?;
    let actual = peer_process_id(&pipe, Peer::Server)?;
    if actual != line.parent_pid {
        return Err(HandshakeFailure::WrongPeer {
            expected: line.parent_pid,
            actual,
        });
    }
    let state = HostState::new(
        line.capability,
        HostProcessId(std::process::id()),
        line.parent_start,
    );
    let (state, effects) = transition_host(state, HostEvent::Begin);
    let saying_hello = matches!(state, HostState::AwaitingAuthenticate { .. });
    settle_handshake(
        &pipe,
        effects,
        saying_hello,
        deadline,
        None,
        HandshakePhase::Authenticate,
    )?;
    let authenticate = read_frame(
        &pipe,
        ReaderRole::Host,
        deadline,
        None,
        HandshakePhase::Authenticate,
    )?;
    let (state, effects) = transition_host(state, HostEvent::Receive(authenticate));
    let ready = matches!(state, HostState::Ready { generation: 0 });
    settle_handshake(
        &pipe,
        effects,
        ready,
        deadline,
        None,
        HandshakePhase::Authenticate,
    )?;
    Ok(HostConnection { pipe, state })
}

/// The process `ShellExecuteExW` started: its pid and the handle
/// `SEE_MASK_NOCLOSEPROCESS` returned.
pub struct LaunchedHost {
    pid: u32,
    process: OwnedHandle,
}

impl LaunchedHost {
    /// The pid the launch returned, which the kernel must name at the pipe.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.pid
    }
}

/// **Start `program` elevated as `endpoint`'s host**: `ShellExecuteExW`, verb
/// `runas`, `SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI`,
/// hidden, with the exact host line. Windows shows its UAC surface; this call
/// waits for it, so it runs on a worker.
pub fn launch_elevated_host(
    worker: &WorkerCtx,
    program: &Path,
    endpoint: &ElevatedEndpoint,
) -> Result<LaunchedHost, LaunchRefusal> {
    launch_through(program, endpoint.host_line(), |program, parameters| {
        shell_execute_runas(worker, program, parameters)
    })
}

/// The launch with its shell call injected: the parameters it is given and
/// the error table it answers through are the product's.
fn launch_through(
    program: &Path,
    line: &HostLine,
    shell: impl FnOnce(&Path, &str) -> Result<LaunchedHost, u32>,
) -> Result<LaunchedHost, LaunchRefusal> {
    // No word of a host line needs quoting (see `HostLine::arguments`).
    let parameters = line.arguments().join(" ");
    shell(program, &parameters).map_err(|code| LaunchRefusal::from_win32(code, system_message))
}

fn shell_execute_runas(
    _worker: &WorkerCtx,
    program: &Path,
    parameters: &str,
) -> Result<LaunchedHost, u32> {
    let file = wide_os(program.as_os_str());
    let verb = wide("runas");
    let parameters = wide(parameters);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: u32::try_from(size_of::<SHELLEXECUTEINFOW>()).unwrap_or(0),
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY: `info` is sized and initialised, and the three strings it points at are
    // NUL-terminated locals that outlive the call.
    unsafe { ShellExecuteExW(&raw mut info) }.map_err(|error| win32_of(&error))?;
    let process = OwnedHandle(info.hProcess);
    // SAFETY: `SEE_MASK_NOCLOSEPROCESS` returned this process handle to this call.
    let pid = unsafe { GetProcessId(process.0) };
    Ok(LaunchedHost { pid, process })
}

fn system_message(code: u32) -> String {
    std::io::Error::from_raw_os_error(code as i32).to_string()
}

fn create_first_instance(name: &str, sddl: &str) -> Result<OwnedHandle, EndpointError> {
    let descriptor = SecurityDescriptor::from_sddl(sddl).map_err(|error| EndpointError::Os {
        call: "ConvertStringSecurityDescriptorToSecurityDescriptorW",
        code: error.raw_os_error().map_or(0, |code| code as u32),
    })?;
    let attributes = descriptor.attributes();
    let wide_name = wide(name);
    // SAFETY: `wide_name` is NUL-terminated and `attributes` points at `descriptor`; both outlive
    // the call.
    let pipe = unsafe {
        CreateNamedPipeW(
            PCWSTR(wide_name.as_ptr()),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            PIPE_BUFFER_BYTES,
            PIPE_BUFFER_BYTES,
            0,
            Some(&raw const attributes),
        )
    };
    if pipe == INVALID_HANDLE_VALUE {
        // `FILE_FLAG_FIRST_PIPE_INSTANCE` on a name that already has an instance is refused
        // with `ERROR_ACCESS_DENIED`.
        let code = last_error();
        return Err(if code == 5 {
            EndpointError::NameTaken
        } else {
            EndpointError::Os {
                call: "CreateNamedPipeW",
                code,
            }
        });
    }
    Ok(OwnedHandle(pipe))
}

fn random_bytes<const N: usize>() -> Result<[u8; N], EndpointError> {
    let mut bytes = [0u8; N];
    // SAFETY: a writable buffer of exactly its own length; no algorithm handle is passed with
    // the system-preferred flag.
    unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }
        .ok()
        .map_err(|error| EndpointError::Os {
            call: "BCryptGenRandom",
            code: error.code().0 as u32,
        })?;
    Ok(bytes)
}

/// Write what the handshake transition asked to be written, and answer
/// whether it reached the state the caller needs. A refusal carries the
/// model's reason; a transition that ended without one (a `Shutdown` before
/// authentication) is the other side leaving.
fn settle_handshake(
    pipe: &OwnedHandle,
    effects: Vec<Effect>,
    reached: bool,
    deadline: Instant,
    watched: Option<HANDLE>,
    phase: HandshakePhase,
) -> Result<(), HandshakeFailure> {
    let mut refusal: Option<PaneFailure> = None;
    for effect in effects {
        match effect {
            Effect::WriteFrame(frame) => write_frame(pipe, &frame, deadline, watched, phase)?,
            Effect::FailPane(reason) => refusal = Some(reason),
            _ => {}
        }
    }
    match (reached, refusal) {
        (_, Some(reason)) => Err(HandshakeFailure::Refused(reason)),
        (true, None) => Ok(()),
        (false, None) => Err(HandshakeFailure::PeerLeft { phase }),
    }
}

#[derive(Clone, Copy)]
enum Peer {
    Client,
    Server,
}

fn peer_process_id(pipe: &OwnedHandle, peer: Peer) -> Result<u32, HandshakeFailure> {
    let mut pid = 0u32;
    // SAFETY: `pipe` is a connected pipe handle this call borrows and `pid` a live local.
    let asked = unsafe {
        match peer {
            Peer::Client => GetNamedPipeClientProcessId(pipe.0, &raw mut pid),
            Peer::Server => GetNamedPipeServerProcessId(pipe.0, &raw mut pid),
        }
    };
    asked.map_err(|error| HandshakeFailure::Os {
        call: match peer {
            Peer::Client => "GetNamedPipeClientProcessId",
            Peer::Server => "GetNamedPipeServerProcessId",
        },
        code: win32_of(&error),
    })?;
    Ok(pid)
}

/// Wait, within `deadline`, for the one client this instance will admit.
fn connect(
    pipe: &OwnedHandle,
    deadline: Instant,
    watched: Option<HANDLE>,
) -> Result<(), HandshakeFailure> {
    let event = overlapped_event()?;
    let mut overlapped = Box::new(event.overlapped());
    let mut ignored = 0u32;
    // SAFETY: the boxed structure outlives the operation, which `finish` collects before
    // returning.
    let issued = unsafe { ConnectNamedPipe(pipe.0, Some(&raw mut *overlapped)) };
    match issued {
        Err(error) if win32_of(&error) == ERROR_PIPE_CONNECTED.0 => return Ok(()),
        Err(error) if win32_of(&error) == ERROR_NO_DATA.0 => {
            return Err(HandshakeFailure::PeerLeft {
                phase: HandshakePhase::Connect,
            });
        }
        _ => {}
    }
    match finish(
        pipe.0,
        issued,
        &event,
        &overlapped,
        &mut ignored,
        deadline,
        watched,
    ) {
        Finished::Io(Ok(())) => Ok(()),
        Finished::Io(Err(code)) => Err(HandshakeFailure::Os {
            call: "ConnectNamedPipe",
            code,
        }),
        Finished::TimedOut => Err(HandshakeFailure::TimedOut {
            phase: HandshakePhase::Connect,
        }),
        Finished::Watched => Err(HandshakeFailure::HostExited),
    }
}

fn read_frame(
    pipe: &OwnedHandle,
    role: ReaderRole,
    deadline: Instant,
    watched: Option<HANDLE>,
    phase: HandshakePhase,
) -> Result<Frame, HandshakeFailure> {
    let message = read_message(pipe, deadline, watched, phase)?;
    decode(&message, role).map_err(|_| HandshakeFailure::Malformed)
}

/// One whole pipe message, assembled across `ERROR_MORE_DATA`.
fn read_message(
    pipe: &OwnedHandle,
    deadline: Instant,
    watched: Option<HANDLE>,
    phase: HandshakePhase,
) -> Result<Vec<u8>, HandshakeFailure> {
    let mut message = Vec::new();
    let mut chunk = vec![0u8; READ_CHUNK_BYTES];
    loop {
        let event = overlapped_event()?;
        let mut overlapped = Box::new(event.overlapped());
        let mut read = 0u32;
        // SAFETY: the buffer and the boxed structure outlive the operation, which `finish`
        // collects before returning.
        let issued = unsafe {
            ReadFile(
                pipe.0,
                Some(&mut chunk),
                Some(&raw mut read),
                Some(&raw mut *overlapped),
            )
        };
        let finished = finish(
            pipe.0,
            issued,
            &event,
            &overlapped,
            &mut read,
            deadline,
            watched,
        );
        let taken = chunk.get(..read as usize).unwrap_or_default();
        match finished {
            Finished::Io(Ok(())) => {
                message.extend_from_slice(taken);
                return Ok(message);
            }
            Finished::Io(Err(code)) if code == ERROR_MORE_DATA.0 => {
                message.extend_from_slice(taken);
                if message.len() > MAX_MESSAGE_BYTES {
                    return Err(HandshakeFailure::Malformed);
                }
            }
            Finished::Io(Err(code)) if is_broken(code) => {
                return Err(HandshakeFailure::PeerLeft { phase });
            }
            Finished::Io(Err(code)) => {
                return Err(HandshakeFailure::Os {
                    call: "ReadFile",
                    code,
                });
            }
            Finished::TimedOut => return Err(HandshakeFailure::TimedOut { phase }),
            Finished::Watched => return Err(HandshakeFailure::HostExited),
        }
    }
}

fn write_frame(
    pipe: &OwnedHandle,
    frame: &Frame,
    deadline: Instant,
    watched: Option<HANDLE>,
    phase: HandshakePhase,
) -> Result<(), HandshakeFailure> {
    // A handshake frame is a fixed-size capability and two integers; the codec cannot refuse it.
    let bytes = encode(frame).map_err(|_| HandshakeFailure::Malformed)?;
    let event = overlapped_event()?;
    let mut overlapped = Box::new(event.overlapped());
    let mut written = 0u32;
    // SAFETY: the bytes and the boxed structure outlive the operation, which `finish` collects
    // before returning.
    let issued = unsafe {
        WriteFile(
            pipe.0,
            Some(&bytes),
            Some(&raw mut written),
            Some(&raw mut *overlapped),
        )
    };
    match finish(
        pipe.0,
        issued,
        &event,
        &overlapped,
        &mut written,
        deadline,
        watched,
    ) {
        Finished::Io(Ok(())) => Ok(()),
        Finished::Io(Err(code)) if is_broken(code) => Err(HandshakeFailure::PeerLeft { phase }),
        Finished::Io(Err(code)) => Err(HandshakeFailure::Os {
            call: "WriteFile",
            code,
        }),
        Finished::TimedOut => Err(HandshakeFailure::TimedOut { phase }),
        Finished::Watched => Err(HandshakeFailure::HostExited),
    }
}

fn is_broken(code: u32) -> bool {
    code == ERROR_BROKEN_PIPE.0 || code == ERROR_PIPE_NOT_CONNECTED.0 || code == ERROR_NO_DATA.0
}

/// How an overlapped operation ended.
enum Finished {
    /// It completed, with its Win32 result.
    Io(Result<(), u32>),
    /// The deadline passed first; the operation was cancelled and collected.
    TimedOut,
    /// The watched process ended first; the operation was cancelled and collected.
    Watched,
}

/// Wait for an issued overlapped operation within `deadline`, also ending
/// when `watched` is signalled, and always collect it before returning: the
/// buffer and the structure are the caller's again afterwards.
fn finish(
    pipe: HANDLE,
    issued: windows::core::Result<()>,
    event: &Overlapped,
    overlapped: &OVERLAPPED,
    transferred: &mut u32,
    deadline: Instant,
    watched: Option<HANDLE>,
) -> Finished {
    // A pending operation and one that completed at once (wholly, or with `ERROR_MORE_DATA`)
    // both signal the event and are collected below, which is where the transferred count is
    // read; any other answer is the operation's whole result.
    if let Err(error) = issued {
        let code = win32_of(&error);
        if code != ERROR_IO_PENDING.0 && code != ERROR_MORE_DATA.0 {
            return Finished::Io(Err(code));
        }
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    // Rounded up: the deadline is the one number this module promises.
    let milliseconds = u32::try_from(
        remaining.as_millis() + u128::from(!remaining.subsec_nanos().is_multiple_of(1_000_000)),
    )
    .unwrap_or(u32::MAX);
    let mut handles = vec![event.handle()];
    handles.extend(watched);
    // SAFETY: both handles outlive the wait.
    let answer = unsafe { WaitForMultipleObjects(&handles, false, milliseconds) };
    let completed = answer == WAIT_OBJECT_0;
    if !completed {
        // SAFETY: cancelling this thread's own outstanding operation on a handle it borrows.
        unsafe {
            let _ = CancelIoEx(pipe, Some(&raw const *overlapped));
        }
    }
    // SAFETY: the structure is still this operation's; `bWait` makes the buffer the caller's
    // again on the way out.
    let collected = unsafe { GetOverlappedResult(pipe, &raw const *overlapped, transferred, true) };
    match collected {
        // An operation that completed while the wait was ending is a completion.
        Ok(()) => Finished::Io(Ok(())),
        Err(error) if completed => Finished::Io(Err(win32_of(&error))),
        Err(_) if answer.0 == WAIT_OBJECT_0.0 + 1 => Finished::Watched,
        Err(_) => Finished::TimedOut,
    }
}

fn overlapped_event() -> Result<Overlapped, HandshakeFailure> {
    Overlapped::new().map_err(|error| HandshakeFailure::Os {
        call: "CreateEventW",
        code: error.raw_os_error().map_or(0, |code| code as u32),
    })
}

fn last_error() -> u32 {
    std::io::Error::last_os_error()
        .raw_os_error()
        .map_or(0, |code| code as u32)
}

fn wide_os(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elevated_protocol::{LAUNCH_TIMEOUT, Message, PaneFailure, ParentStartId};
    use std::path::PathBuf;
    use std::process::Child;
    use std::time::Duration;
    use windows::Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::{
            Authorization::{
                ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
                GetSecurityInfo, SDDL_REVISION_1, SE_KERNEL_OBJECT,
            },
            DACL_SECURITY_INFORMATION, GetTokenInformation, PSECURITY_DESCRIPTOR, PSID,
            RevertToSelf, SECURITY_IMPERSONATION_LEVEL, SecurityIdentification,
            TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TOKEN_USER,
            TokenImpersonationLevel, TokenIntegrityLevel, TokenUser,
        },
        System::{
            Pipes::{ImpersonateNamedPipeClient, PIPE_UNLIMITED_INSTANCES},
            Threading::{
                GetCurrentProcess, GetCurrentThread, OpenProcess, OpenProcessToken,
                OpenThreadToken, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            },
        },
    };

    /// The honest host child: the test binary re-run on this selector, told
    /// its host line in [`LINE_ENV`] and where to write its outcome in
    /// [`ANSWER_ENV`].
    const CHILD_SELECTOR: &str =
        "elevated_pipe::tests::an_honest_host_and_parent_authenticate_by_kernel_pid_and_capability";
    const LINE_ENV: &str = "BT_ELEVATED_HOST_TEST_LINE";
    const ANSWER_ENV: &str = "BT_ELEVATED_HOST_TEST_ANSWER";
    const AUTHENTICATED: &str = "authenticated";

    fn far_deadline() -> Instant {
        Instant::now() + LAUNCH_TIMEOUT
    }

    fn open_process(pid: u32) -> OwnedHandle {
        try_open_process(pid).expect("the process runs")
    }

    /// A launched host whose handle is this test process (never signalled
    /// while the test runs) and whose pid is whatever the test claims.
    fn this_process_claiming(pid: u32) -> LaunchedHost {
        LaunchedHost {
            pid,
            process: open_process(std::process::id()),
        }
    }

    /// Launch the honest host child through the production launch path with
    /// the shell call replaced by an ordinary process start.
    fn launch_honest_child(line: &HostLine, answer: &Path) -> (Child, LaunchedHost, String) {
        let mut seen = String::new();
        let mut started = None;
        let host = launch_through(
            &std::env::current_exe().expect("the test binary"),
            line,
            |program, parameters| {
                seen = parameters.to_owned();
                let child = crate::quiet_command(program)
                    .args(["--exact", CHILD_SELECTOR, "--nocapture"])
                    .env(LINE_ENV, parameters)
                    .env(ANSWER_ENV, answer)
                    .spawn()
                    .expect("the honest host child starts");
                let host = LaunchedHost {
                    pid: child.id(),
                    process: open_process(child.id()),
                };
                started = Some(child);
                Ok(host)
            },
        )
        .expect("the injected launch succeeds");
        (started.expect("the child was started"), host, seen)
    }

    fn answer_of(mut child: Child, answer: &Path) -> String {
        let status = child.wait().expect("the child ends");
        let said = std::fs::read_to_string(answer).unwrap_or_default();
        let _ = std::fs::remove_file(answer);
        assert!(status.success(), "{status}; said {said:?}");
        said
    }

    /// The host side of a test, on a thread of this process.
    fn host_thread(
        line: HostLine,
        deadline: Instant,
    ) -> std::thread::JoinHandle<Result<(), HandshakeFailure>> {
        std::thread::spawn(move || connect_to_parent(&line, deadline).map(|_| ()))
    }

    fn write_raw(pipe: &OwnedHandle, frame: &Frame) {
        write_frame(pipe, frame, far_deadline(), None, HandshakePhase::Hello)
            .expect("the test side writes");
    }

    fn wide_to_string(text: windows::core::PWSTR) -> String {
        // SAFETY: the conversion that produced `text` wrote a NUL-terminated string, which
        // `LocalFree` releases.
        unsafe {
            let owned = text.to_string().expect("UTF-16");
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
            owned
        }
    }

    fn sid_text(sid: PSID) -> String {
        let mut text = windows::core::PWSTR::null();
        // SAFETY: `sid` points into a token buffer the caller keeps alive.
        unsafe { ConvertSidToStringSidW(sid, &raw mut text) }.expect("a SID prints");
        wide_to_string(text)
    }

    /// One token information class, read into an 8-aligned buffer.
    fn token_information(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Vec<u64> {
        let mut needed = 0u32;
        // SAFETY: the documented two-call shape.
        let _ = unsafe { GetTokenInformation(token, class, None, 0, &raw mut needed) };
        let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
        // SAFETY: the buffer is at least `needed` bytes.
        unsafe {
            GetTokenInformation(
                token,
                class,
                Some(buffer.as_mut_ptr().cast()),
                needed,
                &raw mut needed,
            )
        }
        .expect("the token answers");
        buffer
    }

    /// What the server learns by impersonating its connected client after a
    /// read: the lent level, the client's user SID and its integrity SID.
    struct ClientIdentity {
        level: SECURITY_IMPERSONATION_LEVEL,
        user: String,
        integrity: String,
    }

    fn client_identity(pipe: &OwnedHandle) -> ClientIdentity {
        // SAFETY: `pipe` is a connected server end that has read from its client.
        unsafe { ImpersonateNamedPipeClient(pipe.0) }.expect("impersonation after a read");
        let mut token = HANDLE::default();
        // SAFETY: this thread now holds the client's token; `token` is a live local.
        let opened =
            unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &raw mut token) };
        // SAFETY: ends this thread's impersonation.
        unsafe { RevertToSelf() }.expect("revert");
        opened.expect("the impersonation token opens");
        let token = OwnedHandle(token);
        let level = token_information(token.0, TokenImpersonationLevel);
        // SAFETY: the buffer holds one `SECURITY_IMPERSONATION_LEVEL`.
        let level = unsafe { *level.as_ptr().cast::<SECURITY_IMPERSONATION_LEVEL>() };
        let user = token_information(token.0, TokenUser);
        // SAFETY: the buffer holds a `TOKEN_USER` whose SID points inside it.
        let user = sid_text(unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid });
        let label = token_information(token.0, TokenIntegrityLevel);
        // SAFETY: the buffer holds a `TOKEN_MANDATORY_LABEL` whose SID points inside it.
        let integrity =
            sid_text(unsafe { (*label.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid });
        ClientIdentity {
            level,
            user,
            integrity,
        }
    }

    /// **Two unelevated processes, the whole road**: the production launch
    /// path (its shell call replaced by an ordinary process start), the
    /// parent's accept, and in the child the production host handshake.
    ///
    /// Run with [`LINE_ENV`] set, this test *is* that child: it parses the
    /// host line exactly as the launch wrote it, connects, and writes its
    /// outcome for the parent.
    ///
    /// RED MUTATION: in `accept_connected`, construct the session model with
    /// the parent pid instead of the launched pid; the honest `Hello` is
    /// refused as `WrongHostProcess`.
    #[test]
    fn an_honest_host_and_parent_authenticate_by_kernel_pid_and_capability() {
        if let Some(parameters) = std::env::var_os(LINE_ENV) {
            let answer = std::env::var_os(ANSWER_ENV).expect("the answer path");
            let words = parameters
                .to_str()
                .expect("a host line is ASCII")
                .split(' ')
                .map(std::ffi::OsString::from);
            let said = match HostLine::parse(words) {
                Some(Ok(line)) => match connect_to_parent(&line, far_deadline()) {
                    Ok(_) => AUTHENTICATED.to_owned(),
                    Err(failure) => format!("{failure:?}"),
                },
                other => format!("not a host line: {other:?}"),
            };
            std::fs::write(answer, said).expect("the answer is written");
            return;
        }
        let endpoint = ElevatedEndpoint::create().expect("an endpoint");
        let line = endpoint.host_line().clone();
        let answer = bt_testpath::temp_path("bt-elevated-host-answer");
        let (child, host, parameters) = launch_honest_child(&line, &answer);
        assert_eq!(parameters, line.arguments().join(" "));
        assert!(parameters.starts_with(r"--elevated-host 1 \\.\pipe\folio-elevated-"));
        let connection = endpoint.accept(&host, far_deadline());
        assert_eq!(
            connection.as_ref().map(ParentConnection::host_pid).ok(),
            Some(child.id()),
            "{:?}",
            connection.as_ref().err()
        );
        assert_eq!(answer_of(child, &answer), AUTHENTICATED);
    }

    /// The descriptor read back from the kernel object: protected, exactly
    /// two allow entries, the logon SID's and Builtin Administrators', each
    /// with read and write and nothing more (`GRGW` is stored as `0x12019f`,
    /// `FILE_GENERIC_READ | FILE_GENERIC_WRITE`: no `WRITE_DAC`, no
    /// `WRITE_OWNER`).
    ///
    /// RED MUTATIONS: drop the `(A;;GRGW;;;BA)` entry from
    /// `elevated_descriptor_sddl`, and the read-back descriptor names one SID;
    /// grant the logon SID `GA` again, and its entry reads `FA`.
    #[test]
    fn the_endpoint_descriptor_is_protected_and_grants_the_logon_sid_and_administrators_only() {
        let endpoint = ElevatedEndpoint::create().expect("an endpoint");
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: the pipe handle is live; `descriptor` receives a `LocalAlloc`ed buffer.
        let read = unsafe {
            GetSecurityInfo(
                endpoint.pipe.0,
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                Some(&raw mut descriptor),
            )
        };
        assert!(read.is_ok(), "{read:?}");
        let mut text = windows::core::PWSTR::null();
        // SAFETY: `descriptor` is the buffer the call above returned.
        let printed = unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &raw mut text,
                None,
            )
        };
        // SAFETY: `GetSecurityInfo` documents `LocalFree` as the release.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        }
        printed.expect("the descriptor prints");
        let logon = logon_sid().expect("a logon SID");
        assert_eq!(
            wide_to_string(text),
            format!("D:P(A;;0x12019f;;;{logon})(A;;0x12019f;;;BA)")
        );
    }

    /// `FILE_FLAG_FIRST_PIPE_INSTANCE`: a name somebody already holds is
    /// refused, not joined as a further instance of their pipe.
    ///
    /// RED MUTATION: drop `FILE_FLAG_FIRST_PIPE_INSTANCE`; the creation
    /// succeeds as a second instance of the squatter's pipe.
    #[test]
    fn a_name_somebody_already_holds_is_refused_not_joined() {
        let name = elevated_endpoint_name("0123456789abcdef", std::process::id(), [0x5a; 32]);
        let wide_name = wide(&name);
        // SAFETY: a NUL-terminated name; default security; any number of instances.
        let squatter = unsafe {
            CreateNamedPipeW(
                PCWSTR(wide_name.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                1024,
                1024,
                0,
                None,
            )
        };
        assert_ne!(squatter, INVALID_HANDLE_VALUE);
        let _squatter = OwnedHandle(squatter);
        let logon = logon_sid().expect("a logon SID");
        assert_eq!(
            create_first_instance(&name, &elevated_descriptor_sddl(&logon)).err(),
            Some(EndpointError::NameTaken)
        );
    }

    /// The kernel names the connecting process, and only the launched one is
    /// admitted: here the client is this process while the launch named
    /// another pid.
    ///
    /// RED MUTATION: remove the `actual != host.pid` comparison in
    /// `accept_connected`; the client's own `Hello` is refused later and for a
    /// different reason (`WrongHostProcess`), not as the wrong peer.
    #[test]
    fn a_connection_from_any_process_but_the_launched_one_is_refused_by_its_kernel_pid() {
        let endpoint = ElevatedEndpoint::create().expect("an endpoint");
        let host = host_thread(endpoint.host_line().clone(), far_deadline());
        let me = std::process::id();
        let claimed = me + 4;
        let refused = endpoint.accept(&this_process_claiming(claimed), far_deadline());
        assert_eq!(
            refused.err(),
            Some(HandshakeFailure::WrongPeer {
                expected: claimed,
                actual: me
            })
        );
        assert!(matches!(
            host.join().expect("the host thread ends"),
            Err(HandshakeFailure::PeerLeft { .. })
        ));
    }

    /// The host names its parent and admits only that server.
    ///
    /// RED MUTATION: remove the `actual != line.parent_pid` comparison in
    /// `connect_to_parent`; the host goes on to wait for `Authenticate` and
    /// reports the deadline instead.
    #[test]
    fn a_host_refuses_a_server_that_is_not_its_named_parent() {
        let endpoint = ElevatedEndpoint::create().expect("an endpoint");
        let mut line = endpoint.host_line().clone();
        line.parent_pid = std::process::id() + 4;
        assert_eq!(
            connect_to_parent(&line, Instant::now()).err(),
            Some(HandshakeFailure::WrongPeer {
                expected: std::process::id() + 4,
                actual: std::process::id()
            })
        );
    }

    /// A `Hello` that does not repeat the attempt's capability is refused by
    /// the session model, and no `Authenticate` is written.
    ///
    /// RED MUTATION: in `accept_connected`, build the model with the
    /// capability the `Hello` carries; the forged `Hello` is admitted.
    #[test]
    fn a_parent_refuses_a_hello_with_another_capability() {
        let endpoint = ElevatedEndpoint::create().expect("an endpoint");
        let name = endpoint.host_line().pipe_name.clone();
        let forger = std::thread::spawn(move || {
            let pipe = open_client(&wide(&name), (GENERIC_READ | GENERIC_WRITE).0)
                .expect("the forger connects");
            let mode = PIPE_READMODE_MESSAGE;
            // SAFETY: a connected client handle and a live local.
            unsafe { SetNamedPipeHandleState(pipe.0, Some(&raw const mode), None, None) }
                .expect("message mode");
            write_raw(
                &pipe,
                &Frame {
                    generation: 0,
                    message: Message::Hello {
                        capability: Capability::new([0xee; 32]),
                        host_pid: HostProcessId(std::process::id()),
                    },
                },
            );
            read_message(&pipe, far_deadline(), None, HandshakePhase::Authenticate)
        });
        let refused = endpoint.accept(&this_process_claiming(std::process::id()), far_deadline());
        assert_eq!(
            refused.err(),
            Some(HandshakeFailure::Refused(PaneFailure::WrongCapability))
        );
        assert!(matches!(
            forger.join().expect("the forger ends"),
            Err(HandshakeFailure::PeerLeft { .. })
        ));
    }

    /// The capability is spent by the first `Hello`: the authenticated state
    /// no longer holds it, so a replayed `Hello` is an out-of-order frame.
    ///
    /// RED MUTATION: return the connection with the state from before the
    /// `Hello`; the replay is admitted a second time.
    #[test]
    fn the_capability_is_spent_by_the_first_hello() {
        let endpoint = ElevatedEndpoint::create().expect("an endpoint");
        let line = endpoint.host_line().clone();
        let host = host_thread(line.clone(), far_deadline());
        let connection = endpoint
            .accept(&this_process_claiming(std::process::id()), far_deadline())
            .expect("the honest host is admitted");
        assert_eq!(host.join().expect("the host ends"), Ok(()));
        let replay = Frame {
            generation: 0,
            message: Message::Hello {
                capability: line.capability,
                host_pid: HostProcessId(std::process::id()),
            },
        };
        let (_, effects) = transition_parent(connection.state, ParentEvent::Receive(replay));
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::FailPane(PaneFailure::UnexpectedFrame { .. })
            )),
            "{effects:?}"
        );
    }

    /// The host admits only an `Authenticate` that repeats its line's
    /// capability and its parent's start identity.
    ///
    /// RED MUTATION: in `connect_to_parent`, answer `Ok` once a frame has been
    /// read, without passing it to the session model; both forgeries are
    /// admitted.
    #[test]
    fn a_host_refuses_an_authenticate_with_another_capability_or_start_identity() {
        for forge_the_capability in [true, false] {
            let endpoint = ElevatedEndpoint::create().expect("an endpoint");
            let line = endpoint.host_line().clone();
            let host = host_thread(line.clone(), far_deadline());
            connect(&endpoint.pipe, far_deadline(), None).expect("the host connects");
            read_message(&endpoint.pipe, far_deadline(), None, HandshakePhase::Hello)
                .expect("the host says hello");
            let (capability, parent_start, expected) = if forge_the_capability {
                (
                    Capability::new([0x11; 32]),
                    line.parent_start,
                    PaneFailure::WrongCapability,
                )
            } else {
                let forged = ParentStartId(line.parent_start.0 + 1);
                (
                    line.capability,
                    forged,
                    PaneFailure::WrongParentStartIdentity {
                        expected: line.parent_start,
                        received: forged,
                    },
                )
            };
            write_raw(
                &endpoint.pipe,
                &Frame {
                    generation: 0,
                    message: Message::Authenticate {
                        capability,
                        parent_start_id: parent_start,
                    },
                },
            );
            assert_eq!(
                host.join().expect("the host ends"),
                Err(HandshakeFailure::Refused(expected))
            );
        }
    }

    /// The parent's wait ends at the attempt's deadline when no host comes.
    ///
    /// RED MUTATION: in `finish`, report a cancelled wait as the operation's
    /// own result; the parent reports `ERROR_OPERATION_ABORTED` instead of the
    /// deadline.
    #[test]
    fn the_parent_gives_up_at_the_deadline_when_no_host_connects() {
        let endpoint = ElevatedEndpoint::create().expect("an endpoint");
        assert_eq!(
            endpoint
                .accept(&this_process_claiming(1), Instant::now())
                .err(),
            Some(HandshakeFailure::TimedOut {
                phase: HandshakePhase::Connect
            })
        );
    }

    /// The host's wait for `Authenticate` ends at its deadline when the
    /// parent connects and never answers.
    ///
    /// RED MUTATION: in `finish`, report a cancelled wait as the operation's
    /// own result; the host reports `ERROR_OPERATION_ABORTED`.
    #[test]
    fn the_host_gives_up_at_the_deadline_when_the_parent_never_answers() {
        let endpoint = ElevatedEndpoint::create().expect("an endpoint");
        let line = endpoint.host_line().clone();
        let host = host_thread(line, Instant::now());
        connect(&endpoint.pipe, far_deadline(), None).expect("the host connects");
        assert_eq!(
            host.join().expect("the host ends"),
            Err(HandshakeFailure::TimedOut {
                phase: HandshakePhase::Authenticate
            })
        );
    }

    /// A launched host that ends before it connects fails the attempt at
    /// once rather than at the deadline: the wait also watches its process.
    ///
    /// RED MUTATION: pass `None` instead of the host's process to `connect`;
    /// the attempt waits out its deadline and reports `TimedOut`.
    #[test]
    fn a_host_that_ends_before_connecting_fails_the_attempt_at_once() {
        let endpoint = ElevatedEndpoint::create().expect("an endpoint");
        let mut child = crate::quiet_command(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "elevated_pipe::tests::no_such_test_at_all",
                "--list",
            ])
            .spawn()
            .expect("a short-lived child");
        let host = LaunchedHost {
            pid: child.id(),
            process: open_process(child.id()),
        };
        assert_eq!(
            endpoint.accept(&host, far_deadline()).err(),
            Some(HandshakeFailure::HostExited)
        );
        child.wait().expect("the child is reaped");
    }

    /// A host told a pipe that does not exist says so, distinctly.
    ///
    /// RED MUTATION: map every open failure to `Os`; the missing endpoint is
    /// reported as code 2.
    #[test]
    fn a_host_told_a_pipe_that_does_not_exist_reports_no_endpoint() {
        let line = HostLine {
            pipe_name: elevated_endpoint_name("0123456789abcdef", std::process::id(), [0x33; 32]),
            parent_pid: std::process::id(),
            parent_start: ParentStartId(1),
            capability: Capability::new([1; 32]),
        };
        assert_eq!(
            connect_to_parent(&line, far_deadline()).err(),
            Some(HandshakeFailure::NoEndpoint)
        );
    }

    /// The elevated host lends the medium-integrity parent an
    /// identification-level token: the parent can read who connected and
    /// cannot act as it.
    ///
    /// RED MUTATION: drop `SECURITY_SQOS_PRESENT` from `open_client`'s flags;
    /// the server is lent an impersonation-level token.
    #[test]
    fn the_host_lends_the_parent_an_identification_token_only() {
        let endpoint = ElevatedEndpoint::create().expect("an endpoint");
        let line = endpoint.host_line().clone();
        let host = host_thread(line.clone(), far_deadline());
        connect(&endpoint.pipe, far_deadline(), None).expect("the host connects");
        read_message(&endpoint.pipe, far_deadline(), None, HandshakePhase::Hello)
            .expect("the host says hello");
        let identity = client_identity(&endpoint.pipe);
        assert_eq!(identity.level, SecurityIdentification);
        assert!(identity.user.starts_with("S-1-5-"), "{}", identity.user);
        assert!(
            identity.integrity.starts_with("S-1-16-"),
            "{}",
            identity.integrity
        );
        write_raw(
            &endpoint.pipe,
            &Frame {
                generation: 0,
                message: Message::Authenticate {
                    capability: line.capability,
                    parent_start_id: line.parent_start,
                },
            },
        );
        assert_eq!(host.join().expect("the host ends"), Ok(()));
    }

    /// The launch hands the shell call the exact program and host line, and
    /// answers each Win32 code through the error table with the system's own
    /// reason.
    ///
    /// RED MUTATION: in `launch_through`, classify every non-zero code as
    /// `Cancelled`; the access-denied row becomes a cancellation.
    #[test]
    fn the_launch_answers_every_shell_code_through_the_error_table() {
        let endpoint = ElevatedEndpoint::create().expect("an endpoint");
        let program = PathBuf::from(r"C:\Program Files\Folio Terminal\folio.exe");
        for (code, expected) in [
            (1223u32, LaunchRefusal::Cancelled),
            (0x5b3, LaunchRefusal::CouldNotStart(system_message(0x5b3))),
            (5, LaunchRefusal::CouldNotStart(system_message(5))),
            (2, LaunchRefusal::CouldNotStart(system_message(2))),
        ] {
            let mut asked = None;
            let refused = launch_through(&program, endpoint.host_line(), |program, parameters| {
                asked = Some((program.to_owned(), parameters.to_owned()));
                Err(code)
            });
            assert_eq!(refused.err(), Some(expected), "code {code:#x}");
            assert_eq!(
                asked,
                Some((program.clone(), endpoint.host_line().arguments().join(" ")))
            );
        }
        assert!(!system_message(5).is_empty());
    }

    /// `OpenProcess` for waiting and querying, answered rather than expected: an observer may be
    /// refused a process it did not start (another user's elevated host, `ERROR_ACCESS_DENIED`).
    fn try_open_process(pid: u32) -> windows::core::Result<OwnedHandle> {
        // SAFETY: opening a process by id for waiting and querying only.
        unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
        }
        .map(OwnedHandle)
    }

    /// **The `observe` row's body**: wait until `waited` for a client the harness started,
    /// name it by the kernel's pid and put it through the real handshake
    /// ([`ElevatedEndpoint::accept_connected`]); answer the row's lines.
    ///
    /// The observer did not launch this client, so it holds no launch handle. The production
    /// path never opens its host by pid — it keeps the handle `ShellExecuteExW` returned — and an
    /// observer running as one user may be refused another user's elevated process. So the
    /// client's process is opened with `open`, and when that is refused the row says
    /// `process=unavailable (<code>)` and the handshake's waits watch this process's own handle,
    /// which is never signalled while the row runs: they end at the deadline or when the pipe
    /// breaks, and the admission (kernel pid, `Hello`, `Authenticate`, the lent identity) is the
    /// same either way.
    fn observe(
        endpoint: ElevatedEndpoint,
        waited: Instant,
        open: impl FnOnce(u32) -> windows::core::Result<OwnedHandle>,
    ) -> Vec<String> {
        let mut said = Vec::new();
        if let Err(failure) = connect(&endpoint.pipe, waited, None) {
            said.push(format!("connect=Err({failure:?})"));
            return said;
        }
        let pid =
            peer_process_id(&endpoint.pipe, Peer::Client).expect("the kernel names the client");
        said.push(format!("kernel_client_pid={pid}"));
        let process = match open(pid) {
            Ok(process) => {
                said.push("process=opened".to_owned());
                process
            }
            Err(error) => {
                said.push(format!("process=unavailable ({:#x})", win32_of(&error)));
                open_process(std::process::id())
            }
        };
        let host = LaunchedHost { pid, process };
        match endpoint.accept_connected(&host, far_deadline()) {
            Ok(connection) => {
                said.push("accept=Ok".to_owned());
                let identity = client_identity(&connection.pipe);
                said.push(format!(
                    "host_pid={} host_user={} host_integrity={} lent_level={}",
                    connection.host_pid(),
                    identity.user,
                    identity.integrity,
                    identity.level.0
                ));
            }
            Err(failure) => said.push(format!("accept=Err({failure:?})")),
        }
        said
    }

    /// RED — **the `observe` row completes when the observer may not open the client's
    /// process**, as an observer of another user's elevated host may not (T-ADMIN-2's row 1 on
    /// the clean guest): the client is admitted by its kernel pid, the row says the process was
    /// unavailable with the system's code, and the handshake and the lent identity are reported.
    ///
    /// MUTATION (observed red): `observe` taking the process as `open(pid).expect("the process
    /// runs")` — the row panics after the pipe accepted the client, as it did on the guest.
    #[test]
    fn the_observe_row_completes_without_the_clients_process_handle() {
        let endpoint = ElevatedEndpoint::create().expect("an endpoint");
        let host = host_thread(endpoint.host_line().clone(), far_deadline());
        let said = observe(endpoint, far_deadline(), |_| {
            Err(windows::core::Error::from(
                windows::Win32::Foundation::ERROR_ACCESS_DENIED.to_hresult(),
            ))
        });
        assert_eq!(host.join().expect("the host ends"), Ok(()));
        assert_eq!(said[0], format!("kernel_client_pid={}", std::process::id()));
        assert_eq!(said[1], "process=unavailable (0x5)");
        assert_eq!(said[2], "accept=Ok");
        // The host is this process, so its user and integrity are this process's token's.
        let mut token = HANDLE::default();
        // SAFETY: the current process's pseudo-handle; `token` is a live local.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }
            .expect("this process's token opens");
        let token = OwnedHandle(token);
        let user = token_information(token.0, TokenUser);
        // SAFETY: the buffer holds a `TOKEN_USER` whose SID points inside it.
        let user = sid_text(unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid });
        let label = token_information(token.0, TokenIntegrityLevel);
        // SAFETY: the buffer holds a `TOKEN_MANDATORY_LABEL` whose SID points inside it.
        let integrity =
            sid_text(unsafe { (*label.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid });
        assert!(integrity.starts_with("S-1-16-"), "{integrity}");
        assert_eq!(
            said[3],
            format!(
                "host_pid={} host_user={user} host_integrity={integrity} lent_level=1",
                std::process::id()
            )
        );
        assert_eq!(said.len(), 4, "{said:?}");
    }

    /// **A row of the clean-VM record (T-ADMIN-2), never run elsewhere**: it
    /// performs a real `runas` launch, which on an ordinary machine would show
    /// a UAC prompt. The guest harness sets `BT_ELEVATED_PIPE_VM_ROW` to
    /// `launch` (the real launch of `BT_ELEVATED_PIPE_VM_PROGRAM`, a
    /// `folio.exe`, then the real accept, or the refusal the launch answered)
    /// or `observe` (an endpoint whose line is written to `line.txt` for a host
    /// started by the harness under another account; the client the kernel
    /// names is then put through the real handshake), and
    /// `BT_ELEVATED_PIPE_VM_DIR` to where `row.txt` is written.
    #[test]
    #[ignore = "clean-VM row: a real runas launch; run only by the guest harness"]
    fn vm_row() {
        let row = std::env::var("BT_ELEVATED_PIPE_VM_ROW").expect("the row");
        let dir = PathBuf::from(std::env::var_os("BT_ELEVATED_PIPE_VM_DIR").expect("the dir"));
        let mut said = vec![format!("row={row} parent_pid={}", std::process::id())];
        let endpoint = ElevatedEndpoint::create().expect("an endpoint");
        said.push(format!("pipe={}", endpoint.host_line().pipe_name));
        let identify = |connection: &ParentConnection, said: &mut Vec<String>| {
            let identity = client_identity(&connection.pipe);
            said.push(format!(
                "host_pid={} host_user={} host_integrity={} lent_level={}",
                connection.host_pid(),
                identity.user,
                identity.integrity,
                identity.level.0
            ));
        };
        match row.as_str() {
            "launch" => {
                let program = PathBuf::from(
                    std::env::var_os("BT_ELEVATED_PIPE_VM_PROGRAM").expect("the program"),
                );
                let launched =
                    crate::admission::enter_standalone_main("elevated-vm-row", |worker| {
                        launch_elevated_host(worker, &program, &endpoint)
                    })
                    .expect("one standalone entry");
                match launched {
                    Ok(host) => {
                        said.push(format!("launched pid={}", host.pid()));
                        match endpoint.accept(&host, far_deadline()) {
                            Ok(connection) => {
                                said.push("accept=Ok".to_owned());
                                identify(&connection, &mut said);
                            }
                            Err(failure) => said.push(format!("accept=Err({failure:?})")),
                        }
                    }
                    Err(refusal) => said.push(format!("launch=Err({refusal:?})")),
                }
            }
            "observe" => {
                std::fs::write(
                    dir.join("line.txt"),
                    endpoint.host_line().arguments().join(" "),
                )
                .expect("the line is written");
                let waited = Instant::now() + Duration::from_secs(240);
                said.extend(observe(endpoint, waited, try_open_process));
            }
            other => said.push(format!("unknown row {other}")),
        }
        let text = said.join("\n") + "\n";
        std::fs::write(dir.join("row.txt"), &text).expect("the row is written");
        print!("{text}");
    }
}
