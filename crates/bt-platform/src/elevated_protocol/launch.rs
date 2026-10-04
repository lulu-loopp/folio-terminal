use std::time::Duration;

/// The absolute launch/connect/hello/start bound from the administrator design.
pub const LAUNCH_TIMEOUT: Duration = Duration::from_secs(15);

/// A caller-supplied monotonic instant. The model never reads a clock.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LaunchInstant(pub Duration);

/// The complete parent-side launch-attempt state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchState {
    Waiting { began: LaunchInstant },
    Connected,
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
            Self::Waiting { .. } | Self::Connected => None,
        }
    }
}

/// Facts injected by the launcher and transport.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchEvent {
    Tick(LaunchInstant),
    Connected,
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
        (LaunchState::Waiting { began }, LaunchEvent::Tick(now)) => {
            let elapsed = now.0.saturating_sub(began.0);
            if elapsed >= LAUNCH_TIMEOUT {
                Ok(LaunchState::TimedOut)
            } else {
                Ok(LaunchState::Waiting { began })
            }
        }
        (LaunchState::Waiting { .. }, LaunchEvent::Connected) => Ok(LaunchState::Connected),
        (LaunchState::Waiting { .. }, LaunchEvent::Canceled) => Ok(LaunchState::Canceled),
        (LaunchState::Waiting { .. }, LaunchEvent::Failed(reason)) => {
            Ok(LaunchState::Failed(reason))
        }
        (LaunchState::Waiting { .. } | LaunchState::Connected, LaunchEvent::Stopped(reason)) => {
            Ok(LaunchState::Stopped(reason))
        }
        (LaunchState::Connected, LaunchEvent::Tick(_)) => Ok(LaunchState::Connected),
        (state, event) => Err(LaunchTransitionError {
            state: state_name(&state),
            event: event_name(&event),
        }),
    }
}

const fn state_name(state: &LaunchState) -> &'static str {
    match state {
        LaunchState::Waiting { .. } => "waiting",
        LaunchState::Connected => "connected",
        LaunchState::Canceled => "canceled",
        LaunchState::TimedOut => "timed out",
        LaunchState::Failed(_) => "failed",
        LaunchState::Stopped(_) => "stopped",
    }
}

const fn event_name(event: &LaunchEvent) -> &'static str {
    match event {
        LaunchEvent::Tick(_) => "tick",
        LaunchEvent::Connected => "connected",
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
        let waiting = LaunchState::Waiting {
            began: at_millis(1_000),
        };
        assert_eq!(waiting.action(), None);
        assert_eq!(
            transition_launch(waiting.clone(), LaunchEvent::Connected),
            Ok(LaunchState::Connected)
        );
        assert_eq!(
            transition_launch(waiting.clone(), LaunchEvent::Canceled),
            Ok(LaunchState::Canceled)
        );
        assert_eq!(
            transition_launch(
                waiting.clone(),
                LaunchEvent::Failed("access denied".to_owned())
            ),
            Ok(LaunchState::Failed("access denied".to_owned()))
        );
        assert_eq!(
            transition_launch(waiting, LaunchEvent::Stopped(Some("pipe broke".to_owned()))),
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
        assert_eq!(LaunchState::Connected.action(), None);
    }

    /// RED MUTATION: change the timeout comparison from `>=` to `>`; exactly
    /// fifteen seconds remains Waiting.
    #[test]
    fn launch_timeout_uses_the_exact_fifteen_second_boundary() {
        let waiting = LaunchState::Waiting {
            began: at_millis(2_000),
        };
        assert_eq!(
            transition_launch(waiting.clone(), LaunchEvent::Tick(at_millis(16_999))),
            Ok(waiting.clone())
        );
        assert_eq!(
            transition_launch(waiting.clone(), LaunchEvent::Tick(at_millis(17_000))),
            Ok(LaunchState::TimedOut)
        );
        assert_eq!(
            transition_launch(waiting, LaunchEvent::Tick(at_millis(17_001))),
            Ok(LaunchState::TimedOut)
        );
    }
}
