use std::collections::BTreeSet;

use super::codec::{
    Capability, ConPtyKind, ErrorPayload, ExitStatus, ForegroundProcess, Frame, FrameKind, Grid,
    HostProcessId, Message, ParentStartId, RequestId, SpawnRequest, WireString,
};

/// Whether a child birth begins the session or replaces its current child.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartKind {
    Spawn,
    Restart,
}

/// A pane-local failure. No transition in this model can fail another pane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaneFailure {
    WrongCapability,
    WrongHostProcess {
        expected: HostProcessId,
        received: HostProcessId,
    },
    WrongParentStartIdentity {
        expected: ParentStartId,
        received: ParentStartId,
    },
    UnexpectedFrame {
        state: &'static str,
        kind: FrameKind,
    },
    UnexpectedEvent {
        state: &'static str,
        event: &'static str,
    },
    FutureGeneration {
        current: u64,
        received: u64,
    },
    RestartGeneration {
        current: u64,
        received: u64,
    },
    DuplicateRequest(RequestId),
    UnknownRequest(RequestId),
    GenerationExhausted,
    ChildStart(ErrorPayload),
}

/// Every side effect requested by either role's pure transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    WriteFrame(Frame),
    StartChild {
        generation: u64,
        request: SpawnRequest,
        kind: StartKind,
    },
    DeliverInput(Vec<u8>),
    ResizeChild(Grid),
    ObserveForeground(RequestId),
    ApplyOutput(Vec<u8>),
    ApplyExit(ExitStatus),
    ApplyForeground {
        request_id: RequestId,
        process: ForegroundProcess,
    },
    DropFrame(FrameKind),
    StopChild,
    CloseTransport,
    FailPane(PaneFailure),
}

/// Parent-side session state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParentState {
    AwaitingHello {
        capability: Capability,
        host_pid: HostProcessId,
        parent_start_id: ParentStartId,
    },
    Ready {
        generation: u64,
    },
    Starting {
        generation: u64,
        kind: StartKind,
    },
    Running {
        generation: u64,
        child_pid: HostProcessId,
        conpty: ConPtyKind,
        pending_foreground: BTreeSet<RequestId>,
    },
    Exited {
        generation: u64,
        status: ExitStatus,
    },
    ShuttingDown {
        generation: u64,
    },
    Ended,
    Failed,
}

impl ParentState {
    #[must_use]
    pub const fn new(
        capability: Capability,
        host_pid: HostProcessId,
        parent_start_id: ParentStartId,
    ) -> Self {
        Self::AwaitingHello {
            capability,
            host_pid,
            parent_start_id,
        }
    }
}

/// Facts from the parent transport or the pane owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParentEvent {
    Receive(Frame),
    Spawn(SpawnRequest),
    Input(Vec<u8>),
    Resize(Grid),
    ForegroundQuery(RequestId),
    Restart(SpawnRequest),
    Shutdown(WireString),
}

/// Host-side session state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostState {
    Dormant {
        capability: Capability,
        host_pid: HostProcessId,
        parent_start_id: ParentStartId,
    },
    AwaitingAuthenticate {
        capability: Capability,
        parent_start_id: ParentStartId,
    },
    Ready {
        generation: u64,
    },
    Starting {
        generation: u64,
        kind: StartKind,
    },
    Running {
        generation: u64,
        pending_foreground: BTreeSet<RequestId>,
    },
    Exited {
        generation: u64,
    },
    Ended,
    Failed,
}

impl HostState {
    #[must_use]
    pub const fn new(
        capability: Capability,
        host_pid: HostProcessId,
        parent_start_id: ParentStartId,
    ) -> Self {
        Self::Dormant {
            capability,
            host_pid,
            parent_start_id,
        }
    }
}

/// Facts from the parent transport or the host's child owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostEvent {
    Begin,
    Receive(Frame),
    ChildStarted {
        child_pid: HostProcessId,
        conpty: ConPtyKind,
    },
    ChildStartFailed(ErrorPayload),
    ChildOutput(Vec<u8>),
    ChildExited(ExitStatus),
    ForegroundObserved {
        request_id: RequestId,
        process: ForegroundProcess,
    },
    TransportClosed,
}

/// Apply one parent-side event.
#[must_use]
pub fn transition_parent(state: ParentState, event: ParentEvent) -> (ParentState, Vec<Effect>) {
    let event = match event {
        ParentEvent::Shutdown(reason) => return shutdown_parent(state, reason),
        event => event,
    };
    match (state, event) {
        (
            ParentState::AwaitingHello {
                capability,
                host_pid,
                parent_start_id,
            },
            ParentEvent::Receive(frame),
        ) => match frame {
            Frame {
                generation: 0,
                message:
                    Message::Hello {
                        capability: received_capability,
                        host_pid: _,
                    },
            } if received_capability != capability => fail(PaneFailure::WrongCapability),
            Frame {
                generation: 0,
                message:
                    Message::Hello {
                        capability: _,
                        host_pid: received_pid,
                    },
            } if received_pid != host_pid => fail(PaneFailure::WrongHostProcess {
                expected: host_pid,
                received: received_pid,
            }),
            Frame {
                generation: 0,
                message: Message::Hello { .. },
            } => (
                ParentState::Ready { generation: 0 },
                vec![Effect::WriteFrame(Frame {
                    generation: 0,
                    message: Message::Authenticate {
                        capability,
                        parent_start_id,
                    },
                })],
            ),
            frame => unexpected_parent_frame(
                ParentState::AwaitingHello {
                    capability,
                    host_pid,
                    parent_start_id,
                },
                frame,
            ),
        },
        (ParentState::Ready { generation: 0 }, ParentEvent::Spawn(request)) => (
            ParentState::Starting {
                generation: 1,
                kind: StartKind::Spawn,
            },
            vec![Effect::WriteFrame(Frame {
                generation: 1,
                message: Message::Spawn(request),
            })],
        ),
        (state, ParentEvent::Receive(frame)) => receive_parent(state, frame),
        (
            ParentState::Running {
                generation,
                child_pid,
                conpty,
                pending_foreground,
            },
            ParentEvent::Input(bytes),
        ) => (
            ParentState::Running {
                generation,
                child_pid,
                conpty,
                pending_foreground,
            },
            vec![Effect::WriteFrame(Frame {
                generation,
                message: Message::Input(bytes),
            })],
        ),
        (
            ParentState::Running {
                generation,
                child_pid,
                conpty,
                pending_foreground,
            },
            ParentEvent::Resize(grid),
        ) => (
            ParentState::Running {
                generation,
                child_pid,
                conpty,
                pending_foreground,
            },
            vec![Effect::WriteFrame(Frame {
                generation,
                message: Message::Resize(grid),
            })],
        ),
        (
            ParentState::Running {
                generation,
                child_pid,
                conpty,
                mut pending_foreground,
            },
            ParentEvent::ForegroundQuery(request_id),
        ) => {
            if !pending_foreground.insert(request_id) {
                return fail(PaneFailure::DuplicateRequest(request_id));
            }
            (
                ParentState::Running {
                    generation,
                    child_pid,
                    conpty,
                    pending_foreground,
                },
                vec![Effect::WriteFrame(Frame {
                    generation,
                    message: Message::ForegroundQuery(request_id),
                })],
            )
        }
        (ParentState::Running { generation, .. }, ParentEvent::Restart(request)) => {
            let Some(next_generation) = generation.checked_add(1) else {
                return fail(PaneFailure::GenerationExhausted);
            };
            (
                ParentState::Starting {
                    generation: next_generation,
                    kind: StartKind::Restart,
                },
                vec![Effect::WriteFrame(Frame {
                    generation: next_generation,
                    message: Message::Restart(request),
                })],
            )
        }
        (state, event) => unexpected_parent_event(state, event_name_parent(&event)),
    }
}

fn receive_parent(state: ParentState, frame: Frame) -> (ParentState, Vec<Effect>) {
    if let Some(current) = parent_generation(&state) {
        if is_host_operational(frame.message.kind()) && frame.generation < current {
            return (state, vec![Effect::DropFrame(frame.message.kind())]);
        }
        if is_host_operational(frame.message.kind()) && frame.generation > current {
            return fail(PaneFailure::FutureGeneration {
                current,
                received: frame.generation,
            });
        }
    }
    match (state, frame) {
        (
            ParentState::Starting { generation, .. },
            Frame {
                generation: received,
                message: Message::Started { child_pid, conpty },
            },
        ) if received == generation => (
            ParentState::Running {
                generation,
                child_pid,
                conpty,
                pending_foreground: BTreeSet::new(),
            },
            Vec::new(),
        ),
        (
            ParentState::Starting { generation, .. },
            Frame {
                generation: received,
                message: Message::StartFailed(error),
            },
        ) if received == generation => fail(PaneFailure::ChildStart(error)),
        (
            ParentState::Running {
                generation,
                child_pid,
                conpty,
                pending_foreground,
            },
            Frame {
                generation: received,
                message: Message::Output(bytes),
            },
        ) if received == generation => (
            ParentState::Running {
                generation,
                child_pid,
                conpty,
                pending_foreground,
            },
            vec![Effect::ApplyOutput(bytes)],
        ),
        (
            ParentState::Running { generation, .. },
            Frame {
                generation: received,
                message: Message::Exit(status),
            },
        ) if received == generation => (
            ParentState::Exited { generation, status },
            vec![Effect::ApplyExit(status)],
        ),
        (
            ParentState::Running {
                generation,
                child_pid,
                conpty,
                mut pending_foreground,
            },
            Frame {
                generation: received,
                message:
                    Message::ForegroundResult {
                        request_id,
                        process,
                    },
            },
        ) if received == generation => {
            if !pending_foreground.remove(&request_id) {
                return fail(PaneFailure::UnknownRequest(request_id));
            }
            (
                ParentState::Running {
                    generation,
                    child_pid,
                    conpty,
                    pending_foreground,
                },
                vec![Effect::ApplyForeground {
                    request_id,
                    process,
                }],
            )
        }
        (
            ParentState::ShuttingDown { generation },
            Frame {
                generation: received,
                message: Message::Exit(status),
            },
        ) if received == generation => (
            ParentState::ShuttingDown { generation },
            vec![Effect::ApplyExit(status)],
        ),
        (
            ParentState::ShuttingDown { generation },
            Frame {
                generation: received,
                message:
                    message @ (Message::Started { .. }
                    | Message::StartFailed(_)
                    | Message::Output(_)
                    | Message::ForegroundResult { .. }),
            },
        ) if received == generation => (
            ParentState::ShuttingDown { generation },
            vec![Effect::DropFrame(message.kind())],
        ),
        (
            ParentState::ShuttingDown { generation },
            Frame {
                generation: received,
                message: Message::ShutdownAck,
            },
        ) if received == generation => (ParentState::Ended, vec![Effect::CloseTransport]),
        (state, frame) => unexpected_parent_frame(state, frame),
    }
}

fn shutdown_parent(state: ParentState, reason: WireString) -> (ParentState, Vec<Effect>) {
    match parent_generation(&state) {
        Some(generation) if !matches!(state, ParentState::ShuttingDown { .. }) => (
            ParentState::ShuttingDown { generation },
            vec![Effect::WriteFrame(Frame {
                generation,
                message: Message::Shutdown(reason),
            })],
        ),
        Some(_) => (state, Vec::new()),
        None if matches!(&state, ParentState::Ended) => (state, Vec::new()),
        None => (ParentState::Ended, vec![Effect::CloseTransport]),
    }
}

fn unexpected_parent_frame(state: ParentState, frame: Frame) -> (ParentState, Vec<Effect>) {
    fail(PaneFailure::UnexpectedFrame {
        state: parent_state_name(&state),
        kind: frame.message.kind(),
    })
}

fn unexpected_parent_event(state: ParentState, event: &'static str) -> (ParentState, Vec<Effect>) {
    fail(PaneFailure::UnexpectedEvent {
        state: parent_state_name(&state),
        event,
    })
}

/// Apply one host-side event.
#[must_use]
pub fn transition_host(state: HostState, event: HostEvent) -> (HostState, Vec<Effect>) {
    if matches!(&event, HostEvent::TransportClosed) {
        let stop = matches!(
            &state,
            HostState::Starting { .. } | HostState::Running { .. }
        );
        let mut effects = Vec::new();
        if stop {
            effects.push(Effect::StopChild);
        }
        effects.push(Effect::CloseTransport);
        return (HostState::Ended, effects);
    }
    if let HostEvent::Receive(Frame {
        generation,
        message: Message::Shutdown(_),
    }) = &event
    {
        return shutdown_host(state, *generation);
    }
    match (state, event) {
        (
            HostState::Dormant {
                capability,
                host_pid,
                parent_start_id,
            },
            HostEvent::Begin,
        ) => (
            HostState::AwaitingAuthenticate {
                capability,
                parent_start_id,
            },
            vec![Effect::WriteFrame(Frame {
                generation: 0,
                message: Message::Hello {
                    capability,
                    host_pid,
                },
            })],
        ),
        (
            HostState::AwaitingAuthenticate {
                capability,
                parent_start_id,
            },
            HostEvent::Receive(frame),
        ) => match frame {
            Frame {
                generation: 0,
                message:
                    Message::Authenticate {
                        capability: received_capability,
                        parent_start_id: _,
                    },
            } if received_capability != capability => fail_host(PaneFailure::WrongCapability),
            Frame {
                generation: 0,
                message:
                    Message::Authenticate {
                        capability: _,
                        parent_start_id: received,
                    },
            } if received != parent_start_id => fail_host(PaneFailure::WrongParentStartIdentity {
                expected: parent_start_id,
                received,
            }),
            Frame {
                generation: 0,
                message: Message::Authenticate { .. },
            } => (HostState::Ready { generation: 0 }, Vec::new()),
            frame => unexpected_host_frame(
                HostState::AwaitingAuthenticate {
                    capability,
                    parent_start_id,
                },
                frame,
            ),
        },
        (
            HostState::Ready { generation: 0 },
            HostEvent::Receive(Frame {
                generation: 1,
                message: Message::Spawn(request),
            }),
        ) => (
            HostState::Starting {
                generation: 1,
                kind: StartKind::Spawn,
            },
            vec![Effect::StartChild {
                generation: 1,
                request,
                kind: StartKind::Spawn,
            }],
        ),
        (state, HostEvent::Receive(frame)) => receive_host(state, frame),
        (
            HostState::Starting {
                generation,
                kind: _,
            },
            HostEvent::ChildStarted { child_pid, conpty },
        ) => (
            HostState::Running {
                generation,
                pending_foreground: BTreeSet::new(),
            },
            vec![Effect::WriteFrame(Frame {
                generation,
                message: Message::Started { child_pid, conpty },
            })],
        ),
        (HostState::Starting { generation, .. }, HostEvent::ChildStartFailed(error)) => (
            HostState::Failed,
            vec![Effect::WriteFrame(Frame {
                generation,
                message: Message::StartFailed(error),
            })],
        ),
        (
            HostState::Running {
                generation,
                pending_foreground,
            },
            HostEvent::ChildOutput(bytes),
        ) => (
            HostState::Running {
                generation,
                pending_foreground,
            },
            vec![Effect::WriteFrame(Frame {
                generation,
                message: Message::Output(bytes),
            })],
        ),
        (HostState::Running { generation, .. }, HostEvent::ChildExited(status)) => (
            HostState::Exited { generation },
            vec![Effect::WriteFrame(Frame {
                generation,
                message: Message::Exit(status),
            })],
        ),
        (
            HostState::Running {
                generation,
                mut pending_foreground,
            },
            HostEvent::ForegroundObserved {
                request_id,
                process,
            },
        ) => {
            if !pending_foreground.remove(&request_id) {
                return fail_host(PaneFailure::UnknownRequest(request_id));
            }
            (
                HostState::Running {
                    generation,
                    pending_foreground,
                },
                vec![Effect::WriteFrame(Frame {
                    generation,
                    message: Message::ForegroundResult {
                        request_id,
                        process,
                    },
                })],
            )
        }
        (state, event) => unexpected_host_event(state, event_name_host(&event)),
    }
}

fn receive_host(state: HostState, frame: Frame) -> (HostState, Vec<Effect>) {
    let kind = frame.message.kind();
    if let Some(current) = host_generation(&state) {
        if is_parent_operational(kind) && frame.generation < current {
            return (state, vec![Effect::DropFrame(kind)]);
        }
        if matches!(&frame.message, Message::Restart(_)) {
            if matches!(&state, HostState::Running { .. })
                && current.checked_add(1) == Some(frame.generation)
                && let Message::Restart(request) = frame.message
            {
                return (
                    HostState::Starting {
                        generation: frame.generation,
                        kind: StartKind::Restart,
                    },
                    vec![Effect::StartChild {
                        generation: frame.generation,
                        request,
                        kind: StartKind::Restart,
                    }],
                );
            }
            return fail_host(PaneFailure::RestartGeneration {
                current,
                received: frame.generation,
            });
        }
        if is_parent_operational(kind) && frame.generation > current {
            return fail_host(PaneFailure::FutureGeneration {
                current,
                received: frame.generation,
            });
        }
    }
    match (state, frame) {
        (
            HostState::Running {
                generation,
                pending_foreground,
            },
            Frame {
                generation: received,
                message: Message::Input(bytes),
            },
        ) if received == generation => (
            HostState::Running {
                generation,
                pending_foreground,
            },
            vec![Effect::DeliverInput(bytes)],
        ),
        (
            HostState::Running {
                generation,
                pending_foreground,
            },
            Frame {
                generation: received,
                message: Message::Resize(grid),
            },
        ) if received == generation => (
            HostState::Running {
                generation,
                pending_foreground,
            },
            vec![Effect::ResizeChild(grid)],
        ),
        (
            HostState::Running {
                generation,
                mut pending_foreground,
            },
            Frame {
                generation: received,
                message: Message::ForegroundQuery(request_id),
            },
        ) if received == generation => {
            if !pending_foreground.insert(request_id) {
                return fail_host(PaneFailure::DuplicateRequest(request_id));
            }
            (
                HostState::Running {
                    generation,
                    pending_foreground,
                },
                vec![Effect::ObserveForeground(request_id)],
            )
        }
        (state, frame) => unexpected_host_frame(state, frame),
    }
}

fn shutdown_host(state: HostState, generation: u64) -> (HostState, Vec<Effect>) {
    match state {
        HostState::Dormant { .. } | HostState::AwaitingAuthenticate { .. } => {
            (HostState::Ended, vec![Effect::CloseTransport])
        }
        HostState::Ready {
            generation: current,
        }
        | HostState::Exited {
            generation: current,
        } if generation == current => (
            HostState::Ended,
            vec![
                Effect::WriteFrame(Frame {
                    generation,
                    message: Message::ShutdownAck,
                }),
                Effect::CloseTransport,
            ],
        ),
        HostState::Starting {
            generation: current,
            ..
        }
        | HostState::Running {
            generation: current,
            ..
        } if generation == current => (
            HostState::Ended,
            vec![
                Effect::StopChild,
                Effect::WriteFrame(Frame {
                    generation,
                    message: Message::ShutdownAck,
                }),
                Effect::CloseTransport,
            ],
        ),
        HostState::Ended => (HostState::Ended, Vec::new()),
        HostState::Failed => (HostState::Ended, vec![Effect::CloseTransport]),
        state => {
            let current = host_generation(&state).unwrap_or(0);
            if generation < current {
                (state, vec![Effect::DropFrame(FrameKind::Shutdown)])
            } else {
                fail_host(PaneFailure::FutureGeneration {
                    current,
                    received: generation,
                })
            }
        }
    }
}

fn fail(reason: PaneFailure) -> (ParentState, Vec<Effect>) {
    (
        ParentState::Failed,
        vec![Effect::FailPane(reason), Effect::CloseTransport],
    )
}

fn fail_host(reason: PaneFailure) -> (HostState, Vec<Effect>) {
    (
        HostState::Failed,
        vec![Effect::FailPane(reason), Effect::CloseTransport],
    )
}

fn unexpected_host_frame(state: HostState, frame: Frame) -> (HostState, Vec<Effect>) {
    fail_host(PaneFailure::UnexpectedFrame {
        state: host_state_name(&state),
        kind: frame.message.kind(),
    })
}

fn unexpected_host_event(state: HostState, event: &'static str) -> (HostState, Vec<Effect>) {
    fail_host(PaneFailure::UnexpectedEvent {
        state: host_state_name(&state),
        event,
    })
}

const fn parent_generation(state: &ParentState) -> Option<u64> {
    match state {
        ParentState::Ready { generation }
        | ParentState::Starting { generation, .. }
        | ParentState::Running { generation, .. }
        | ParentState::Exited { generation, .. }
        | ParentState::ShuttingDown { generation } => Some(*generation),
        ParentState::AwaitingHello { .. } | ParentState::Ended | ParentState::Failed => None,
    }
}

const fn host_generation(state: &HostState) -> Option<u64> {
    match state {
        HostState::Ready { generation }
        | HostState::Starting { generation, .. }
        | HostState::Running { generation, .. }
        | HostState::Exited { generation } => Some(*generation),
        HostState::Dormant { .. }
        | HostState::AwaitingAuthenticate { .. }
        | HostState::Ended
        | HostState::Failed => None,
    }
}

const fn is_host_operational(kind: FrameKind) -> bool {
    matches!(
        kind,
        FrameKind::Started
            | FrameKind::StartFailed
            | FrameKind::Output
            | FrameKind::Exit
            | FrameKind::ForegroundResult
    )
}

const fn is_parent_operational(kind: FrameKind) -> bool {
    matches!(
        kind,
        FrameKind::Spawn
            | FrameKind::Input
            | FrameKind::Resize
            | FrameKind::ForegroundQuery
            | FrameKind::Restart
            | FrameKind::Shutdown
    )
}

const fn parent_state_name(state: &ParentState) -> &'static str {
    match state {
        ParentState::AwaitingHello { .. } => "awaiting hello",
        ParentState::Ready { .. } => "ready",
        ParentState::Starting { .. } => "starting",
        ParentState::Running { .. } => "running",
        ParentState::Exited { .. } => "exited",
        ParentState::ShuttingDown { .. } => "shutting down",
        ParentState::Ended => "ended",
        ParentState::Failed => "failed",
    }
}

const fn host_state_name(state: &HostState) -> &'static str {
    match state {
        HostState::Dormant { .. } => "dormant",
        HostState::AwaitingAuthenticate { .. } => "awaiting authenticate",
        HostState::Ready { .. } => "ready",
        HostState::Starting { .. } => "starting",
        HostState::Running { .. } => "running",
        HostState::Exited { .. } => "exited",
        HostState::Ended => "ended",
        HostState::Failed => "failed",
    }
}

const fn event_name_parent(event: &ParentEvent) -> &'static str {
    match event {
        ParentEvent::Receive(_) => "receive",
        ParentEvent::Spawn(_) => "spawn",
        ParentEvent::Input(_) => "input",
        ParentEvent::Resize(_) => "resize",
        ParentEvent::ForegroundQuery(_) => "foreground query",
        ParentEvent::Restart(_) => "restart",
        ParentEvent::Shutdown(_) => "shutdown",
    }
}

const fn event_name_host(event: &HostEvent) -> &'static str {
    match event {
        HostEvent::Begin => "begin",
        HostEvent::Receive(_) => "receive",
        HostEvent::ChildStarted { .. } => "child started",
        HostEvent::ChildStartFailed(_) => "child start failed",
        HostEvent::ChildOutput(_) => "child output",
        HostEvent::ChildExited(_) => "child exited",
        HostEvent::ForegroundObserved { .. } => "foreground observed",
        HostEvent::TransportClosed => "transport closed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capability(tag: u8) -> Capability {
        Capability::new([tag; 32])
    }

    fn request() -> SpawnRequest {
        SpawnRequest {
            spec: super::super::codec::SpawnSpec {
                program: WireString::from_text(r"C:\Program Files\pwsh.exe"),
                arguments: vec![WireString::from_text("-NoLogo")],
            },
            cwd: Some(WireString::from_text(r"C:\A folder")),
            grid: Grid {
                rows: 24,
                columns: 80,
            },
            environment: vec![super::super::codec::EnvironmentEntry {
                name: WireString::from_text("TERM"),
                value: WireString::from_text("xterm-256color"),
            }],
        }
    }

    fn one_written(effects: Vec<Effect>) -> Frame {
        assert_eq!(effects.len(), 1, "{effects:#?}");
        let Effect::WriteFrame(frame) = effects.into_iter().next().expect("one effect") else {
            panic!("the one effect was not a frame")
        };
        frame
    }

    fn begin_running() -> (ParentState, HostState) {
        let secret = capability(7);
        let host_pid = HostProcessId(401);
        let parent_start = ParentStartId(9001);
        let parent = ParentState::new(secret, host_pid, parent_start);
        let host = HostState::new(secret, host_pid, parent_start);

        let (host, effects) = transition_host(host, HostEvent::Begin);
        let hello = one_written(effects);
        let (parent, effects) = transition_parent(parent, ParentEvent::Receive(hello));
        let authenticate = one_written(effects);
        let (host, effects) = transition_host(host, HostEvent::Receive(authenticate));
        assert!(effects.is_empty());

        let (parent, effects) = transition_parent(parent, ParentEvent::Spawn(request()));
        let spawn = one_written(effects);
        let (host, effects) = transition_host(host, HostEvent::Receive(spawn));
        assert!(matches!(effects.as_slice(), [Effect::StartChild { .. }]));
        let (host, effects) = transition_host(
            host,
            HostEvent::ChildStarted {
                child_pid: HostProcessId(402),
                conpty: ConPtyKind::Shipped,
            },
        );
        let started = one_written(effects);
        let (parent, effects) = transition_parent(parent, ParentEvent::Receive(started));
        assert!(effects.is_empty());
        (parent, host)
    }

    /// RED MUTATION: omit `pending_foreground.insert` on the host; the observed
    /// result fails instead of completing the full two-role happy path.
    #[test]
    fn both_roles_follow_the_full_happy_path() {
        let (mut parent, mut host) = begin_running();

        let (next, effects) = transition_parent(parent, ParentEvent::Input(vec![0, 0xff]));
        parent = next;
        let input = one_written(effects);
        let (next, effects) = transition_host(host, HostEvent::Receive(input));
        host = next;
        assert_eq!(effects, vec![Effect::DeliverInput(vec![0, 0xff])]);

        let resized = Grid {
            rows: 48,
            columns: 160,
        };
        let (next, effects) = transition_parent(parent, ParentEvent::Resize(resized));
        parent = next;
        let resize = one_written(effects);
        let (next, effects) = transition_host(host, HostEvent::Receive(resize));
        host = next;
        assert_eq!(effects, vec![Effect::ResizeChild(resized)]);

        let request_id = RequestId(55);
        let (next, effects) = transition_parent(parent, ParentEvent::ForegroundQuery(request_id));
        parent = next;
        let query = one_written(effects);
        let (next, effects) = transition_host(host, HostEvent::Receive(query));
        host = next;
        assert_eq!(effects, vec![Effect::ObserveForeground(request_id)]);
        let process = ForegroundProcess::Known(WireString::from_text("agent.exe"));
        let (next, effects) = transition_host(
            host,
            HostEvent::ForegroundObserved {
                request_id,
                process: process.clone(),
            },
        );
        host = next;
        let result = one_written(effects);
        let (next, effects) = transition_parent(parent, ParentEvent::Receive(result));
        parent = next;
        assert_eq!(
            effects,
            vec![Effect::ApplyForeground {
                request_id,
                process,
            }]
        );

        let (next, effects) = transition_host(host, HostEvent::ChildOutput(vec![3, 1, 4]));
        host = next;
        let output = one_written(effects);
        let (next, effects) = transition_parent(parent, ParentEvent::Receive(output));
        parent = next;
        assert_eq!(effects, vec![Effect::ApplyOutput(vec![3, 1, 4])]);

        let (next, effects) = transition_parent(parent, ParentEvent::Restart(request()));
        parent = next;
        let restart = one_written(effects);
        assert_eq!(restart.generation, 2, "parent advances before writing");
        let (next, effects) = transition_host(host, HostEvent::Receive(restart));
        host = next;
        assert!(matches!(
            effects.as_slice(),
            [Effect::StartChild {
                generation: 2,
                kind: StartKind::Restart,
                ..
            }]
        ));
        let (next, effects) = transition_host(
            host,
            HostEvent::ChildStarted {
                child_pid: HostProcessId(403),
                conpty: ConPtyKind::Inbox,
            },
        );
        host = next;
        let started = one_written(effects);
        let (next, effects) = transition_parent(parent, ParentEvent::Receive(started));
        parent = next;
        assert!(effects.is_empty());

        let (next, effects) = transition_host(host, HostEvent::ChildExited(Some(0)));
        host = next;
        let exit = one_written(effects);
        let (next, effects) = transition_parent(parent, ParentEvent::Receive(exit));
        parent = next;
        assert_eq!(effects, vec![Effect::ApplyExit(Some(0))]);

        let (next, effects) = transition_parent(
            parent,
            ParentEvent::Shutdown(WireString::from_text("pane closed")),
        );
        parent = next;
        let shutdown = one_written(effects);
        let (next, effects) = transition_host(host, HostEvent::Receive(shutdown));
        host = next;
        assert!(matches!(host, HostState::Ended));
        let ack = effects
            .into_iter()
            .find_map(|effect| match effect {
                Effect::WriteFrame(frame) => Some(frame),
                _ => None,
            })
            .expect("shutdown acknowledgement");
        let (parent, effects) = transition_parent(parent, ParentEvent::Receive(ack));
        assert!(matches!(parent, ParentState::Ended));
        assert_eq!(effects, vec![Effect::CloseTransport]);
    }

    /// RED MUTATION: compare only the capability in either handshake; one of
    /// the identity-mismatch transitions reaches Ready.
    #[test]
    fn a_wrong_capability_host_pid_or_parent_identity_ends_the_session() {
        let secret = capability(1);
        let parent = ParentState::new(secret, HostProcessId(10), ParentStartId(20));
        let (state, effects) = transition_parent(
            parent,
            ParentEvent::Receive(Frame {
                generation: 0,
                message: Message::Hello {
                    capability: capability(2),
                    host_pid: HostProcessId(10),
                },
            }),
        );
        assert!(matches!(state, ParentState::Failed));
        assert!(matches!(
            effects[0],
            Effect::FailPane(PaneFailure::WrongCapability)
        ));

        let parent = ParentState::new(secret, HostProcessId(10), ParentStartId(20));
        let (state, effects) = transition_parent(
            parent,
            ParentEvent::Receive(Frame {
                generation: 0,
                message: Message::Hello {
                    capability: secret,
                    host_pid: HostProcessId(11),
                },
            }),
        );
        assert!(matches!(state, ParentState::Failed));
        assert!(matches!(
            effects[0],
            Effect::FailPane(PaneFailure::WrongHostProcess { .. })
        ));

        let (host, _) = transition_host(
            HostState::new(secret, HostProcessId(10), ParentStartId(20)),
            HostEvent::Begin,
        );
        let (state, effects) = transition_host(
            host,
            HostEvent::Receive(Frame {
                generation: 0,
                message: Message::Authenticate {
                    capability: capability(2),
                    parent_start_id: ParentStartId(20),
                },
            }),
        );
        assert!(matches!(state, HostState::Failed));
        assert!(matches!(
            effects[0],
            Effect::FailPane(PaneFailure::WrongCapability)
        ));

        let (host, _) = transition_host(
            HostState::new(secret, HostProcessId(10), ParentStartId(20)),
            HostEvent::Begin,
        );
        let (state, effects) = transition_host(
            host,
            HostEvent::Receive(Frame {
                generation: 0,
                message: Message::Authenticate {
                    capability: secret,
                    parent_start_id: ParentStartId(21),
                },
            }),
        );
        assert!(matches!(state, HostState::Failed));
        assert!(matches!(
            effects[0],
            Effect::FailPane(PaneFailure::WrongParentStartIdentity { .. })
        ));
    }

    /// RED MUTATION: let the generic host receive arm ignore unexpected
    /// messages; the second Spawn and Input-before-Started cease to fail.
    #[test]
    fn every_out_of_order_frame_is_a_pane_local_protocol_error() {
        let secret = capability(3);
        let parent = ParentState::new(secret, HostProcessId(30), ParentStartId(40));
        let invalid_for_parent = [
            Message::Started {
                child_pid: HostProcessId(1),
                conpty: ConPtyKind::Shipped,
            },
            Message::StartFailed(ErrorPayload {
                code: 1,
                message: WireString::default(),
            }),
            Message::Output(Vec::new()),
            Message::Exit(None),
            Message::ForegroundResult {
                request_id: RequestId(1),
                process: ForegroundProcess::Unknown,
            },
            Message::ShutdownAck,
        ];
        for message in invalid_for_parent {
            let (state, effects) = transition_parent(
                parent.clone(),
                ParentEvent::Receive(Frame {
                    generation: 0,
                    message,
                }),
            );
            assert!(matches!(state, ParentState::Failed));
            assert!(matches!(
                effects.as_slice(),
                [
                    Effect::FailPane(PaneFailure::UnexpectedFrame { .. }),
                    Effect::CloseTransport
                ]
            ));
        }

        let (_, host) = begin_running();
        let (state, effects) = transition_host(
            host,
            HostEvent::Receive(Frame {
                generation: 1,
                message: Message::Spawn(request()),
            }),
        );
        assert!(matches!(state, HostState::Failed));
        assert!(matches!(
            effects[0],
            Effect::FailPane(PaneFailure::UnexpectedFrame {
                kind: FrameKind::Spawn,
                ..
            })
        ));

        let (parent, host) = begin_running();
        let (starting_parent, effects) = transition_parent(parent, ParentEvent::Restart(request()));
        let restart = one_written(effects);
        let (starting_host, _) = transition_host(host, HostEvent::Receive(restart));
        let (state, effects) = transition_host(
            starting_host,
            HostEvent::Receive(Frame {
                generation: 2,
                message: Message::Input(vec![1]),
            }),
        );
        assert!(matches!(state, HostState::Failed));
        assert!(matches!(
            effects[0],
            Effect::FailPane(PaneFailure::UnexpectedFrame {
                kind: FrameKind::Input,
                ..
            })
        ));
        let (state, effects) = transition_parent(starting_parent, ParentEvent::Input(vec![1]));
        assert!(matches!(state, ParentState::Failed));
        assert!(matches!(
            effects[0],
            Effect::FailPane(PaneFailure::UnexpectedEvent { event: "input", .. })
        ));
    }

    /// RED MUTATION: compare stale frames with `<=` reversed; an old Output,
    /// Exit, or ForegroundResult is applied to generation two.
    #[test]
    fn stale_generation_frames_after_restart_are_dropped_by_both_roles() {
        let (parent, host) = begin_running();
        let (parent, effects) = transition_parent(parent, ParentEvent::Restart(request()));
        let restart = one_written(effects);
        let (host, _) = transition_host(host, HostEvent::Receive(restart));

        for message in [
            Message::Output(vec![1]),
            Message::Exit(Some(9)),
            Message::ForegroundResult {
                request_id: RequestId(4),
                process: ForegroundProcess::Unknown,
            },
        ] {
            let kind = message.kind();
            let (same, effects) = transition_parent(
                parent.clone(),
                ParentEvent::Receive(Frame {
                    generation: 1,
                    message,
                }),
            );
            assert_eq!(same, parent);
            assert_eq!(effects, vec![Effect::DropFrame(kind)]);
        }

        for message in [
            Message::Input(vec![1]),
            Message::Resize(Grid {
                rows: 25,
                columns: 81,
            }),
            Message::ForegroundQuery(RequestId(4)),
            Message::Restart(request()),
            Message::Shutdown(WireString::default()),
        ] {
            let kind = message.kind();
            let (same, effects) = transition_host(
                host.clone(),
                HostEvent::Receive(Frame {
                    generation: 1,
                    message,
                }),
            );
            assert_eq!(same, host);
            assert_eq!(effects, vec![Effect::DropFrame(kind)]);
        }
    }

    /// RED MUTATION: route every current-generation frame received while
    /// shutting down through `unexpected_parent_frame`; an in-flight Exit no
    /// longer reaches the pane and legitimate pre-Shutdown frames fail it.
    #[test]
    fn shutting_down_pins_every_frame_kind() {
        let generation = 7;
        let shutting_down = ParentState::ShuttingDown { generation };
        let dropped = [
            Message::Started {
                child_pid: HostProcessId(40),
                conpty: ConPtyKind::Shipped,
            },
            Message::StartFailed(ErrorPayload {
                code: 5,
                message: WireString::from_text("could not start"),
            }),
            Message::Output(vec![1, 2, 3]),
            Message::ForegroundResult {
                request_id: RequestId(9),
                process: ForegroundProcess::Unknown,
            },
        ];
        for message in dropped {
            let kind = message.kind();
            let (state, effects) = transition_parent(
                shutting_down.clone(),
                ParentEvent::Receive(Frame {
                    generation,
                    message,
                }),
            );
            assert_eq!(state, shutting_down);
            assert_eq!(effects, vec![Effect::DropFrame(kind)]);
        }

        let (state, effects) = transition_parent(
            shutting_down.clone(),
            ParentEvent::Receive(Frame {
                generation,
                message: Message::Exit(Some(23)),
            }),
        );
        assert_eq!(state, shutting_down);
        assert_eq!(effects, vec![Effect::ApplyExit(Some(23))]);

        let (state, effects) = transition_parent(
            shutting_down.clone(),
            ParentEvent::Receive(Frame {
                generation,
                message: Message::ShutdownAck,
            }),
        );
        assert_eq!(state, ParentState::Ended);
        assert_eq!(effects, vec![Effect::CloseTransport]);

        let rejected = [
            Message::Hello {
                capability: capability(1),
                host_pid: HostProcessId(2),
            },
            Message::Authenticate {
                capability: capability(1),
                parent_start_id: ParentStartId(2),
            },
            Message::Spawn(request()),
            Message::Input(vec![1]),
            Message::Resize(Grid {
                rows: 24,
                columns: 80,
            }),
            Message::ForegroundQuery(RequestId(9)),
            Message::Restart(request()),
            Message::Shutdown(WireString::from_text("duplicate")),
        ];
        for message in rejected {
            let kind = message.kind();
            let (state, effects) = transition_parent(
                shutting_down.clone(),
                ParentEvent::Receive(Frame {
                    generation,
                    message,
                }),
            );
            assert_eq!(state, ParentState::Failed, "{kind:?}");
            assert_eq!(
                effects,
                vec![
                    Effect::FailPane(PaneFailure::UnexpectedFrame {
                        state: "shutting down",
                        kind,
                    }),
                    Effect::CloseTransport,
                ],
                "{kind:?}"
            );
        }
    }

    /// RED MUTATION: accept an operational frame whose generation is greater
    /// than the current generation; one role no longer reports FutureGeneration.
    #[test]
    fn future_generation_is_a_pane_local_error_for_both_roles() {
        let (parent, host) = begin_running();
        let (state, effects) = transition_parent(
            parent,
            ParentEvent::Receive(Frame {
                generation: 2,
                message: Message::Output(vec![1]),
            }),
        );
        assert_eq!(state, ParentState::Failed);
        assert!(matches!(
            effects.first(),
            Some(Effect::FailPane(PaneFailure::FutureGeneration {
                current: 1,
                received: 2,
            }))
        ));

        let (state, effects) = transition_host(
            host,
            HostEvent::Receive(Frame {
                generation: 2,
                message: Message::Input(vec![1]),
            }),
        );
        assert_eq!(state, HostState::Failed);
        assert!(matches!(
            effects.first(),
            Some(Effect::FailPane(PaneFailure::FutureGeneration {
                current: 1,
                received: 2,
            }))
        ));
    }

    /// RED MUTATION: accept any Restart generation while Running; generation
    /// three starts instead of reporting RestartGeneration from generation one.
    #[test]
    fn restart_requires_exactly_the_next_generation() {
        let (_, host) = begin_running();
        let (state, effects) = transition_host(
            host,
            HostEvent::Receive(Frame {
                generation: 3,
                message: Message::Restart(request()),
            }),
        );
        assert_eq!(state, HostState::Failed);
        assert!(matches!(
            effects.first(),
            Some(Effect::FailPane(PaneFailure::RestartGeneration {
                current: 1,
                received: 3,
            }))
        ));
    }

    /// RED MUTATION: let insertion of an already-live request id succeed; the
    /// parent or host accepts the same request twice.
    #[test]
    fn duplicate_live_request_ids_fail_both_roles() {
        let (parent, host) = begin_running();
        let request_id = RequestId(44);
        let (parent, _) = transition_parent(parent, ParentEvent::ForegroundQuery(request_id));
        let (state, effects) = transition_parent(parent, ParentEvent::ForegroundQuery(request_id));
        assert_eq!(state, ParentState::Failed);
        assert!(matches!(
            effects.first(),
            Some(Effect::FailPane(PaneFailure::DuplicateRequest(RequestId(
                44
            ))))
        ));

        let query = Frame {
            generation: 1,
            message: Message::ForegroundQuery(request_id),
        };
        let (host, _) = transition_host(host, HostEvent::Receive(query.clone()));
        let (state, effects) = transition_host(host, HostEvent::Receive(query));
        assert_eq!(state, HostState::Failed);
        assert!(matches!(
            effects.first(),
            Some(Effect::FailPane(PaneFailure::DuplicateRequest(RequestId(
                44
            ))))
        ));
    }

    /// RED MUTATION: ignore a missing pending request id; an unasked result is
    /// applied or written instead of reporting UnknownRequest.
    #[test]
    fn unasked_foreground_results_fail_both_roles() {
        let (parent, host) = begin_running();
        let request_id = RequestId(81);
        let (state, effects) = transition_parent(
            parent,
            ParentEvent::Receive(Frame {
                generation: 1,
                message: Message::ForegroundResult {
                    request_id,
                    process: ForegroundProcess::Unknown,
                },
            }),
        );
        assert_eq!(state, ParentState::Failed);
        assert!(matches!(
            effects.first(),
            Some(Effect::FailPane(PaneFailure::UnknownRequest(RequestId(81))))
        ));

        let (state, effects) = transition_host(
            host,
            HostEvent::ForegroundObserved {
                request_id,
                process: ForegroundProcess::Unknown,
            },
        );
        assert_eq!(state, HostState::Failed);
        assert!(matches!(
            effects.first(),
            Some(Effect::FailPane(PaneFailure::UnknownRequest(RequestId(81))))
        ));
    }

    /// RED MUTATION: replace checked generation advance with wrapping addition;
    /// a restart at u64::MAX writes generation zero.
    #[test]
    fn restart_rejects_generation_exhaustion() {
        let state = ParentState::Running {
            generation: u64::MAX,
            child_pid: HostProcessId(8),
            conpty: ConPtyKind::Inbox,
            pending_foreground: BTreeSet::new(),
        };
        let (state, effects) = transition_parent(state, ParentEvent::Restart(request()));
        assert_eq!(state, ParentState::Failed);
        assert_eq!(
            effects,
            vec![
                Effect::FailPane(PaneFailure::GenerationExhausted),
                Effect::CloseTransport,
            ]
        );
    }

    /// RED MUTATION: make pre-authentication Shutdown emit an acknowledgement;
    /// the AwaitingAuthenticate row performs more than transport close.
    #[test]
    fn shutdown_has_a_defined_transition_from_every_state() {
        let secret = capability(5);
        let (pre_auth, effects) = transition_host(
            HostState::AwaitingAuthenticate {
                capability: secret,
                parent_start_id: ParentStartId(2),
            },
            HostEvent::Receive(Frame {
                generation: 0,
                message: Message::Shutdown(WireString::from_text("close")),
            }),
        );
        assert_eq!(pre_auth, HostState::Ended);
        assert_eq!(effects, vec![Effect::CloseTransport]);

        let parent_states = vec![
            ParentState::new(secret, HostProcessId(1), ParentStartId(2)),
            ParentState::Ready { generation: 0 },
            ParentState::Starting {
                generation: 1,
                kind: StartKind::Spawn,
            },
            ParentState::Running {
                generation: 1,
                child_pid: HostProcessId(3),
                conpty: ConPtyKind::Shipped,
                pending_foreground: BTreeSet::new(),
            },
            ParentState::Exited {
                generation: 1,
                status: None,
            },
            ParentState::ShuttingDown { generation: 1 },
            ParentState::Failed,
            ParentState::Ended,
        ];
        for state in parent_states {
            let (next, effects) =
                transition_parent(state, ParentEvent::Shutdown(WireString::from_text("close")));
            assert!(
                !matches!(effects.first(), Some(Effect::FailPane(_))),
                "{next:#?} {effects:#?}"
            );
        }

        let host_states = vec![
            HostState::new(secret, HostProcessId(1), ParentStartId(2)),
            HostState::AwaitingAuthenticate {
                capability: secret,
                parent_start_id: ParentStartId(2),
            },
            HostState::Ready { generation: 1 },
            HostState::Starting {
                generation: 1,
                kind: StartKind::Spawn,
            },
            HostState::Running {
                generation: 1,
                pending_foreground: BTreeSet::new(),
            },
            HostState::Exited { generation: 1 },
            HostState::Failed,
            HostState::Ended,
        ];
        for state in host_states {
            let (next, effects) = transition_host(
                state,
                HostEvent::Receive(Frame {
                    generation: 1,
                    message: Message::Shutdown(WireString::from_text("close")),
                }),
            );
            assert!(matches!(next, HostState::Ended), "{next:#?} {effects:#?}");
            assert!(
                !matches!(effects.first(), Some(Effect::FailPane(_))),
                "{effects:#?}"
            );
        }
    }
}
