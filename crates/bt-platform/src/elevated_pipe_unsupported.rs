//! **The elevated pane's pipe and launch primitives, where there is no UAC.**
//!
//! An administrator terminal is a Windows feature (design note
//! `docs/plans/design/admin-terminal-2026-10-04.md`; on macOS and Linux `sudo`
//! in the shell is the idiom). This arm keeps the module's shape so callers
//! carry no `cfg`, and answers every entrance with `Unsupported`. No endpoint
//! can exist here, so the calls that need one cannot be reached.

use std::convert::Infallible;
use std::path::Path;
use std::time::Instant;

use crate::admission::WorkerCtx;
use crate::elevated_protocol::{EndpointError, HandshakeFailure, HostLine, LaunchRefusal};

/// One attempt's listening pipe. Never constructed on this platform.
pub struct ElevatedEndpoint {
    never: Infallible,
}

impl ElevatedEndpoint {
    /// Refused: this platform has no elevated host.
    pub fn create() -> Result<Self, EndpointError> {
        Err(EndpointError::Unsupported)
    }

    /// The words the host is started with.
    #[must_use]
    pub fn host_line(&self) -> &HostLine {
        match self.never {}
    }

    /// Admit the launched host.
    pub fn accept(
        self,
        _host: &LaunchedHost,
        _deadline: Instant,
    ) -> Result<ParentConnection, HandshakeFailure> {
        match self.never {}
    }
}

/// The parent's authenticated pipe. Never constructed on this platform.
pub struct ParentConnection {
    never: Infallible,
}

impl ParentConnection {
    /// The host's process id.
    #[must_use]
    pub fn host_pid(&self) -> u32 {
        match self.never {}
    }
}

/// The host's authenticated pipe. Never constructed on this platform.
pub struct HostConnection {
    _never: Infallible,
}

/// The launched host. Never constructed on this platform.
pub struct LaunchedHost {
    never: Infallible,
}

impl LaunchedHost {
    /// The pid the launch returned.
    #[must_use]
    pub fn pid(&self) -> u32 {
        match self.never {}
    }
}

/// Refused: this platform has no elevated host.
pub fn connect_to_parent(
    _line: &HostLine,
    _deadline: Instant,
) -> Result<HostConnection, HandshakeFailure> {
    Err(HandshakeFailure::Unsupported)
}

/// Start `program` elevated as `endpoint`'s host; no endpoint exists here.
pub fn launch_elevated_host(
    _worker: &WorkerCtx,
    _program: &Path,
    endpoint: &ElevatedEndpoint,
) -> Result<LaunchedHost, LaunchRefusal> {
    match endpoint.never {}
}

#[cfg(test)]
mod tests {
    use crate::elevated_protocol::{
        Capability, EndpointError, HandshakeFailure, HostLine, ParentStartId,
        elevated_endpoint_name,
    };
    use std::time::Instant;

    /// RED MUTATION: let `connect_to_parent` attempt a connection (or let
    /// `create` succeed); the answer is no longer `Unsupported`.
    #[test]
    fn this_platform_answers_unsupported_at_both_ends() {
        assert!(matches!(
            super::ElevatedEndpoint::create(),
            Err(EndpointError::Unsupported)
        ));
        let line = HostLine {
            pipe_name: elevated_endpoint_name("0123456789abcdef", 1, [7; 32]),
            parent_pid: 1,
            parent_start: ParentStartId(1),
            capability: Capability::new([9; 32]),
        };
        assert!(matches!(
            super::connect_to_parent(&line, Instant::now()),
            Err(HandshakeFailure::Unsupported)
        ));
    }
}
