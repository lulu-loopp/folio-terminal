use std::time::Duration;

/// The absolute launch/connect/hello/start bound from the administrator design.
pub const LAUNCH_TIMEOUT: Duration = Duration::from_secs(15);

/// A caller-supplied monotonic instant. The model never reads a clock.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LaunchInstant(pub Duration);

/// The complete parent-side launch-attempt state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchState {
    Launching { began: LaunchInstant },
    Connecting { began: LaunchInstant },
    AwaitingHello { began: LaunchInstant },
    Starting { began: LaunchInstant },
    Started,
    Canceled,
    TimedOut,
    Failed(String),
    Stopped(Option<String>),
}

/// The only inline action a terminal body may offer for a terminal state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchAction {
    TryAgain,
    RestartShell,
}

impl LaunchState {
    #[must_use]
    pub const fn action(&self) -> Option<LaunchAction> {
        match self {
            Self::Canceled | Self::TimedOut | Self::Failed(_) => Some(LaunchAction::TryAgain),
            Self::Stopped(_) => Some(LaunchAction::RestartShell),
            Self::Launching { .. }
            | Self::Connecting { .. }
            | Self::AwaitingHello { .. }
            | Self::Starting { .. }
            | Self::Started => None,
        }
    }
}

/// Facts injected by the launcher and transport.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchEvent {
    Tick(LaunchInstant),
    HostLaunched,
    TransportConnected,
    HelloReceived,
    ChildStarted,
    Canceled,
    Failed(String),
    Stopped(Option<String>),
}

/// A duplicate or nonsensical launch outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchTransitionError {
    pub state: &'static str,
    pub event: &'static str,
}

/// Apply one injected launch fact without a timer, sleep, process, or pipe.
pub fn transition_launch(
    state: LaunchState,
    event: LaunchEvent,
) -> Result<LaunchState, LaunchTransitionError> {
    match (state, event) {
        (
            state @ (LaunchState::Launching { began }
            | LaunchState::Connecting { began }
            | LaunchState::AwaitingHello { began }
            | LaunchState::Starting { began }),
            LaunchEvent::Tick(now),
        ) => {
            let elapsed = now.0.saturating_sub(began.0);
            if elapsed >= LAUNCH_TIMEOUT {
                Ok(LaunchState::TimedOut)
            } else {
                Ok(state)
            }
        }
        (LaunchState::Launching { began }, LaunchEvent::HostLaunched) => {
            Ok(LaunchState::Connecting { began })
        }
        (LaunchState::Connecting { began }, LaunchEvent::TransportConnected) => {
            Ok(LaunchState::AwaitingHello { began })
        }
        (LaunchState::AwaitingHello { began }, LaunchEvent::HelloReceived) => {
            Ok(LaunchState::Starting { began })
        }
        (LaunchState::Starting { .. }, LaunchEvent::ChildStarted) => Ok(LaunchState::Started),
        (LaunchState::Launching { .. }, LaunchEvent::Canceled) => Ok(LaunchState::Canceled),
        (
            LaunchState::Launching { .. }
            | LaunchState::Connecting { .. }
            | LaunchState::AwaitingHello { .. }
            | LaunchState::Starting { .. },
            LaunchEvent::Failed(reason),
        ) => Ok(LaunchState::Failed(reason)),
        (
            LaunchState::Launching { .. }
            | LaunchState::Connecting { .. }
            | LaunchState::AwaitingHello { .. }
            | LaunchState::Starting { .. },
            LaunchEvent::Stopped(reason),
        ) => Ok(LaunchState::Failed(reason.unwrap_or_default())),
        (LaunchState::Started, LaunchEvent::Stopped(reason)) => Ok(LaunchState::Stopped(reason)),
        (state, event) => Err(LaunchTransitionError {
            state: state_name(&state),
            event: event_name(&event),
        }),
    }
}

const fn state_name(state: &LaunchState) -> &'static str {
    match state {
        LaunchState::Launching { .. } => "launching",
        LaunchState::Connecting { .. } => "connecting",
        LaunchState::AwaitingHello { .. } => "awaiting hello",
        LaunchState::Starting { .. } => "starting",
        LaunchState::Started => "started",
        LaunchState::Canceled => "canceled",
        LaunchState::TimedOut => "timed out",
        LaunchState::Failed(_) => "failed",
        LaunchState::Stopped(_) => "stopped",
    }
}

const fn event_name(event: &LaunchEvent) -> &'static str {
    match event {
        LaunchEvent::Tick(_) => "tick",
        LaunchEvent::HostLaunched => "host launched",
        LaunchEvent::TransportConnected => "transport connected",
        LaunchEvent::HelloReceived => "hello received",
        LaunchEvent::ChildStarted => "child started",
        LaunchEvent::Canceled => "canceled",
        LaunchEvent::Failed(_) => "failed",
        LaunchEvent::Stopped(_) => "stopped",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_millis(value: u64) -> LaunchInstant {
        LaunchInstant(Duration::from_millis(value))
    }

    /// RED MUTATION: map `Canceled` to `Failed`; the exact canceled row and
    /// its Try-again action differ.
    #[test]
    fn every_launch_table_row_has_its_exact_state_and_single_action() {
        let launching = LaunchState::Launching {
            began: at_millis(1_000),
        };
        assert_eq!(launching.action(), None);
        assert_eq!(
            transition_launch(launching.clone(), LaunchEvent::HostLaunched),
            Ok(LaunchState::Connecting {
                began: at_millis(1_000)
            })
        );
        assert_eq!(
            transition_launch(launching.clone(), LaunchEvent::Canceled),
            Ok(LaunchState::Canceled)
        );
        assert_eq!(
            transition_launch(
                launching.clone(),
                LaunchEvent::Failed("access denied".to_owned())
            ),
            Ok(LaunchState::Failed("access denied".to_owned()))
        );
        assert_eq!(
            transition_launch(
                LaunchState::Started,
                LaunchEvent::Stopped(Some("pipe broke".to_owned()))
            ),
            Ok(LaunchState::Stopped(Some("pipe broke".to_owned())))
        );
        assert_eq!(LaunchState::Canceled.action(), Some(LaunchAction::TryAgain));
        assert_eq!(LaunchState::TimedOut.action(), Some(LaunchAction::TryAgain));
        assert_eq!(
            LaunchState::Failed(String::new()).action(),
            Some(LaunchAction::TryAgain)
        );
        assert_eq!(
            LaunchState::Stopped(None).action(),
            Some(LaunchAction::RestartShell)
        );
        assert_eq!(LaunchState::Started.action(), None);
    }

    /// RED MUTATION: change the timeout comparison from `>=` to `>`; every
    /// pre-start phase remains live at exactly fifteen seconds.
    #[test]
    fn launch_timeout_uses_the_exact_fifteen_second_boundary() {
        let began = at_millis(2_000);
        for state in [
            LaunchState::Launching { began },
            LaunchState::Connecting { began },
            LaunchState::AwaitingHello { began },
            LaunchState::Starting { began },
        ] {
            assert_eq!(
                transition_launch(state.clone(), LaunchEvent::Tick(at_millis(16_999))),
                Ok(state.clone())
            );
            assert_eq!(
                transition_launch(state, LaunchEvent::Tick(at_millis(17_000))),
                Ok(LaunchState::TimedOut)
            );
        }
    }

    /// RED MUTATION: keep `Connecting` after `TransportConnected`; a host that
    /// connects without saying Hello is not represented by the bounded phase.
    #[test]
    fn launch_connect_hello_and_start_are_distinct_bounded_phases() {
        let began = at_millis(2_000);
        let phases = [
            (LaunchState::Launching { began }, LaunchEvent::HostLaunched),
            (
                LaunchState::Connecting { began },
                LaunchEvent::TransportConnected,
            ),
            (
                LaunchState::AwaitingHello { began },
                LaunchEvent::HelloReceived,
            ),
            (LaunchState::Starting { began }, LaunchEvent::ChildStarted),
        ];
        let expected = [
            LaunchState::Connecting { began },
            LaunchState::AwaitingHello { began },
            LaunchState::Starting { began },
            LaunchState::Started,
        ];
        for ((state, event), expected) in phases.into_iter().zip(expected) {
            assert_eq!(transition_launch(state, event), Ok(expected));
        }
    }

    /// RED MUTATION: map a pre-start disappearance to `Stopped`; it offers
    /// Restart-shell instead of the ruled Try-again action.
    #[test]
    fn disappearance_is_a_start_failure_until_the_child_has_started() {
        let began = at_millis(2_000);
        for state in [
            LaunchState::Launching { began },
            LaunchState::Connecting { began },
            LaunchState::AwaitingHello { began },
            LaunchState::Starting { began },
        ] {
            let (failed, action) = transition_launch(
                state,
                LaunchEvent::Stopped(Some("host disappeared".to_owned())),
            )
            .map(|state| {
                let action = state.action();
                (state, action)
            })
            .expect("pre-start disappearance has a ruled outcome");
            assert_eq!(failed, LaunchState::Failed("host disappeared".to_owned()));
            assert_eq!(action, Some(LaunchAction::TryAgain));
        }

        let stopped = transition_launch(LaunchState::Started, LaunchEvent::Stopped(None))
            .expect("post-start disappearance has a ruled outcome");
        assert_eq!(stopped, LaunchState::Stopped(None));
        assert_eq!(stopped.action(), Some(LaunchAction::RestartShell));
    }
}
