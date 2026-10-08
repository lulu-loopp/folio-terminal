//! The platform-free half of an elevated pane's handshake: the host's exact
//! command line, the launch refusal table, and the typed outcomes the pipe
//! arms report.
//!
//! `crate::elevated_pipe` performs the effects; everything here is data and
//! pure functions, so both arms and both directions share one spelling.

use std::ffi::OsString;
use std::fmt;

use super::codec::{Capability, ParentStartId, WIRE_VERSION};
use super::launch::LaunchEvent;
use super::session::PaneFailure;

/// The private first word of the elevated host's command line.
pub const ELEVATED_HOST_FLAG: &str = "--elevated-host";

/// The usage line of the private host door.
pub const ELEVATED_HOST_USAGE: &str = "usage: folio --elevated-host <wire-version> <pipe-name> \
     <parent-pid> <parent-start-id> <capability>";

const PIPE_PREFIX: &str = r"\\.\pipe\folio-elevated-";
const SESSION_TAG_DIGITS: usize = 16;
const ATTEMPT_TAG_BYTES: usize = 32;
const CAPABILITY_BYTES: usize = 32;

/// The name of one attempt's pipe: the logon-session tag, the parent pid and a
/// 256-bit tag drawn for this attempt alone.
#[must_use]
pub fn elevated_endpoint_name(
    session_tag: &str,
    parent_pid: u32,
    attempt_tag: [u8; ATTEMPT_TAG_BYTES],
) -> String {
    format!(
        "{PIPE_PREFIX}{session_tag}-{parent_pid}-{}",
        lower_hex(&attempt_tag)
    )
}

/// Everything the parent hands the elevated host on its command line. The
/// line carries no program, environment, profile or path to run; those cross
/// the authenticated pipe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostLine {
    pub pipe_name: String,
    pub parent_pid: u32,
    pub parent_start: ParentStartId,
    pub capability: Capability,
}

/// Why words that begin with [`ELEVATED_HOST_FLAG`] are not a host line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostLineFault {
    /// A well-formed version this build does not speak.
    UnknownWireVersion(u16),
    /// Anything else: a missing, extra or malformed word.
    Malformed,
}

impl HostLine {
    /// The words after the program, in the order [`HostLine::parse`] reads
    /// them. Every word is drawn from ASCII digits, lowercase hexadecimal,
    /// `-`, `.`, `\` and the fixed prefix, so none needs quoting on a Windows
    /// command line.
    #[must_use]
    pub fn arguments(&self) -> Vec<String> {
        vec![
            ELEVATED_HOST_FLAG.to_owned(),
            WIRE_VERSION.to_string(),
            self.pipe_name.clone(),
            self.parent_pid.to_string(),
            self.parent_start.0.to_string(),
            lower_hex(&self.capability.bytes()),
        ]
    }

    /// The exact grammar. `None` when the first word is not the private flag;
    /// otherwise the line or its fault.
    pub fn parse(
        args: impl IntoIterator<Item = OsString>,
    ) -> Option<Result<HostLine, HostLineFault>> {
        let mut args = args.into_iter();
        let first = args.next()?;
        if first.to_str()? != ELEVATED_HOST_FLAG {
            return None;
        }
        let words: Vec<OsString> = args.collect();
        Some(Self::parse_words(&words))
    }

    fn parse_words(words: &[OsString]) -> Result<HostLine, HostLineFault> {
        let [version, pipe_name, parent_pid, parent_start, capability] = words else {
            return Err(HostLineFault::Malformed);
        };
        fn text(word: &OsString) -> Result<&str, HostLineFault> {
            word.to_str().ok_or(HostLineFault::Malformed)
        }
        let version = decimal::<u16>(text(version)?).ok_or(HostLineFault::Malformed)?;
        if version != WIRE_VERSION {
            return Err(HostLineFault::UnknownWireVersion(version));
        }
        let pipe_name = text(pipe_name)?;
        if !names_an_elevated_endpoint(pipe_name) {
            return Err(HostLineFault::Malformed);
        }
        let parent_pid = decimal::<u32>(text(parent_pid)?)
            .filter(|pid| *pid != 0)
            .ok_or(HostLineFault::Malformed)?;
        let parent_start = decimal::<u64>(text(parent_start)?).ok_or(HostLineFault::Malformed)?;
        let capability = from_lower_hex::<CAPABILITY_BYTES>(text(capability)?)
            .ok_or(HostLineFault::Malformed)?;
        Ok(HostLine {
            pipe_name: pipe_name.to_owned(),
            parent_pid,
            parent_start: ParentStartId(parent_start),
            capability: Capability::new(capability),
        })
    }
}

/// Whether `name` has the exact shape [`elevated_endpoint_name`] writes. The
/// grammar keeps a name from addressing anything outside the local pipe
/// namespace.
#[must_use]
pub fn names_an_elevated_endpoint(name: &str) -> bool {
    let Some(tail) = name.strip_prefix(PIPE_PREFIX) else {
        return false;
    };
    let mut segments = tail.split('-');
    let (Some(session), Some(pid), Some(tag), None) = (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) else {
        return false;
    };
    session.len() == SESSION_TAG_DIGITS
        && is_lower_hex(session)
        && decimal::<u32>(pid).is_some_and(|pid| pid != 0)
        && tag.len() == ATTEMPT_TAG_BYTES * 2
        && is_lower_hex(tag)
}

/// One step of the handshake a deadline or a failure can stop at.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandshakePhase {
    /// The parent waiting for the host to connect, or the host opening the pipe.
    Connect,
    /// The parent waiting for the host's `Hello`.
    Hello,
    /// The host waiting for the parent's `Authenticate`.
    Authenticate,
}

/// Why an endpoint could not be created.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EndpointError {
    /// The process token has no logon SID, so no descriptor can name it.
    NoLogonSid,
    /// Another pipe already holds the name (`FILE_FLAG_FIRST_PIPE_INSTANCE`).
    NameTaken,
    /// A Win32 call failed with this code.
    Os { call: &'static str, code: u32 },
    /// This platform has no elevated host.
    Unsupported,
}

/// Why one side of the handshake ended without an authenticated pipe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HandshakeFailure {
    /// The absolute deadline passed during `phase`.
    TimedOut { phase: HandshakePhase },
    /// The kernel names a different process at the other end of the pipe.
    WrongPeer { expected: u32, actual: u32 },
    /// The launched host process ended before the handshake finished.
    HostExited,
    /// The other side closed the pipe during `phase`.
    PeerLeft { phase: HandshakePhase },
    /// The pipe the host was told to open does not exist.
    NoEndpoint,
    /// A frame that the codec refuses.
    Malformed,
    /// A frame the session model refuses: wrong capability, wrong host pid,
    /// wrong parent start identity, or an out-of-order frame.
    Refused(PaneFailure),
    /// A Win32 call failed with this code.
    Os { call: &'static str, code: u32 },
    /// This platform has no elevated host.
    Unsupported,
}

impl fmt::Display for HandshakeFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "elevated host handshake failed: {self:?}")
    }
}

/// `ERROR_CANCELLED`: the person declined the UAC prompt.
pub const WIN32_ERROR_CANCELLED: u32 = 1223;

/// The launch error table (spikes report, decision (d)): only
/// `ERROR_CANCELLED` is a cancellation; every other failure, including
/// `ERROR_NOT_INTERACTIVE_WINDOW_STATION`, `ERROR_ACCESS_DENIED` and
/// `ERROR_FILE_NOT_FOUND`, is a failure to start that carries its reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchRefusal {
    Cancelled,
    CouldNotStart(String),
}

impl LaunchRefusal {
    /// Classify the Win32 code `ShellExecuteExW` reported. `reason` is that
    /// code's system message, which the caller supplies so this stays pure.
    #[must_use]
    pub fn from_win32(code: u32, reason: impl FnOnce(u32) -> String) -> Self {
        if code == WIN32_ERROR_CANCELLED {
            Self::Cancelled
        } else {
            Self::CouldNotStart(reason(code))
        }
    }

    /// The fact the launch-attempt model is told.
    #[must_use]
    pub fn into_launch_event(self) -> LaunchEvent {
        match self {
            Self::Cancelled => LaunchEvent::Canceled,
            Self::CouldNotStart(reason) => LaunchEvent::Failed(reason),
        }
    }
}

fn decimal<T: std::str::FromStr>(word: &str) -> Option<T> {
    // `str::parse` also takes a leading `+`; the grammar is digits only.
    if word.is_empty() || !word.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    word.parse().ok()
}

fn is_lower_hex(word: &str) -> bool {
    word.bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

fn from_lower_hex<const N: usize>(word: &str) -> Option<[u8; N]> {
    if word.len() != N * 2 || !is_lower_hex(word) {
        return None;
    }
    let mut bytes = [0u8; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(word.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::super::launch::{LaunchAction, LaunchInstant, LaunchState, transition_launch};
    use super::*;
    use std::time::Duration;

    fn line() -> HostLine {
        let mut capability = [0u8; CAPABILITY_BYTES];
        for (index, byte) in capability.iter_mut().enumerate() {
            *byte = u8::try_from(index * 7 + 3).unwrap();
        }
        HostLine {
            pipe_name: elevated_endpoint_name("0123456789abcdef", 4242, [0xa5; 32]),
            parent_pid: 4242,
            parent_start: ParentStartId(133_456_789_012_345_678),
            capability: Capability::new(capability),
        }
    }

    fn words(line: &[String]) -> Vec<OsString> {
        line.iter().map(OsString::from).collect()
    }

    /// RED MUTATION: in `HostLine::arguments`, write the capability before the
    /// parent start identity; the round trip and the exact order both differ.
    #[test]
    fn the_host_line_round_trips_in_the_designs_exact_order() {
        let line = line();
        let arguments = line.arguments();
        assert_eq!(arguments[0], "--elevated-host");
        assert_eq!(arguments[1], "1");
        assert_eq!(arguments[2], line.pipe_name);
        assert_eq!(arguments[3], "4242");
        assert_eq!(arguments[4], "133456789012345678");
        assert_eq!(arguments[5].len(), 64);
        assert_eq!(HostLine::parse(words(&arguments)), Some(Ok(line)));
    }

    /// RED MUTATION: accept any first word starting with `--elevated`; the
    /// near-miss and the misplaced flag open the door.
    #[test]
    fn the_host_door_opens_only_on_its_exact_first_word() {
        let mut arguments = line().arguments();
        arguments[0] = "--elevated-hosts".to_owned();
        assert_eq!(HostLine::parse(words(&arguments)), None);
        let mut prefixed = vec!["--cwd".to_owned()];
        prefixed.extend(line().arguments());
        assert_eq!(HostLine::parse(words(&prefixed)), None);
        assert_eq!(HostLine::parse(Vec::<OsString>::new()), None);
    }

    /// RED MUTATION: drop the wire-version comparison; a version-2 line is
    /// accepted by a version-1 host.
    #[test]
    fn a_host_line_of_another_wire_version_is_refused_by_its_version() {
        let mut arguments = line().arguments();
        arguments[1] = "2".to_owned();
        assert_eq!(
            HostLine::parse(words(&arguments)),
            Some(Err(HostLineFault::UnknownWireVersion(2)))
        );
    }

    /// RED MUTATION: let `decimal` accept a leading `+`, or let the capability
    /// accept uppercase hexadecimal; one of the malformed lines parses.
    #[test]
    fn every_malformed_word_refuses_the_whole_line() {
        let good = line().arguments();
        let replace = |index: usize, word: &str| {
            let mut arguments = good.clone();
            arguments[index] = word.to_owned();
            arguments
        };
        let mut extra = good.clone();
        extra.push("extra".to_owned());
        let mut short = good.clone();
        short.pop();
        let upper = good[5].to_uppercase();
        for arguments in [
            extra,
            short,
            replace(1, "+1"),
            replace(1, "版本"),
            replace(2, r"\\.\pipe\folio-attention-0123456789abcdef-4242-00"),
            replace(2, r"\\server\pipe\folio-elevated-0123456789abcdef-4242-00"),
            replace(3, "0"),
            replace(3, "+4242"),
            replace(3, "四二"),
            replace(4, "-1"),
            replace(5, &upper),
            replace(5, &good[5][..62]),
            replace(5, "能力令牌"),
        ] {
            assert_eq!(
                HostLine::parse(words(&arguments)),
                Some(Err(HostLineFault::Malformed)),
                "{arguments:?}"
            );
        }
    }

    /// RED MUTATION: drop the segment count in `names_an_elevated_endpoint`; a
    /// name with a fourth segment addresses a different pipe.
    #[test]
    fn an_endpoint_name_has_exactly_the_shape_the_parent_writes() {
        let name = elevated_endpoint_name("0123456789abcdef", 77, [0x0f; 32]);
        assert!(names_an_elevated_endpoint(&name));
        assert!(!names_an_elevated_endpoint(&format!("{name}-x")));
        assert!(!names_an_elevated_endpoint(&name.replace("-77-", "-0-")));
        assert!(!names_an_elevated_endpoint(
            &name.replace("0123456789abcdef", "0123456789abcdeF")
        ));
        assert!(!names_an_elevated_endpoint(
            &name.replace("folio", "管理员")
        ));
    }

    fn reason(code: u32) -> String {
        format!("reason {code} 原因")
    }

    /// The launch error table as states: each code `ShellExecuteExW` can
    /// report, classified and applied to a launching attempt.
    ///
    /// RED MUTATION: classify `ERROR_ACCESS_DENIED` (or the window-station
    /// code) as `Cancelled`; that row's state becomes `Canceled`.
    #[test]
    fn only_error_cancelled_is_a_cancellation_and_every_other_code_carries_its_reason() {
        let launching = LaunchState::Launching {
            began: LaunchInstant(Duration::from_millis(5)),
        };
        let table: [(u32, LaunchState); 6] = [
            (1223, LaunchState::Canceled),
            (0x5b3, LaunchState::Failed(reason(0x5b3))),
            (5, LaunchState::Failed(reason(5))),
            (2, LaunchState::Failed(reason(2))),
            (1155, LaunchState::Failed(reason(1155))),
            (0x8000_4005, LaunchState::Failed(reason(0x8000_4005))),
        ];
        for (code, expected) in table {
            let event = LaunchRefusal::from_win32(code, reason).into_launch_event();
            let state = transition_launch(launching.clone(), event).expect("a ruled outcome");
            assert_eq!(state, expected, "code {code:#x}");
            assert_eq!(
                state.action(),
                Some(LaunchAction::TryAgain),
                "code {code:#x}"
            );
        }
    }
}
