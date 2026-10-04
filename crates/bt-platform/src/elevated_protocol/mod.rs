//! The platform-free protocol shared by an elevated pane's resident and host.
//!
//! This module owns bytes and pure transitions only. Pipe, process, clock, and
//! ConPTY effects are represented as data for the later transport tickets.

mod codec;
mod launch;
mod session;

pub use codec::{
    CONTROL_MAX_PAYLOAD, Capability, ConPtyKind, DecodeError, Decoder, DecoderAllocationAccounting,
    Direction, EncodeError, EnvironmentEntry, ErrorPayload, ExitStatus, FRAME_HEADER_LENGTH,
    FRAME_MAGIC, ForegroundProcess, Frame, FrameKind, Grid, HostProcessId, MalformedPayload,
    Message, OUTPUT_MAX_PAYLOAD, ParentStartId, ReaderRole, RequestId, SpawnRequest, SpawnSpec,
    WIRE_VERSION, WireString, decode, encode,
};
pub use launch::{
    LAUNCH_TIMEOUT, LaunchAction, LaunchEvent, LaunchInstant, LaunchState, LaunchTransitionError,
    transition_launch,
};
pub use session::{
    Effect, HostEvent, HostState, PaneFailure, ParentEvent, ParentState, StartKind,
    transition_host, transition_parent,
};
