//! The low-cadence worker lane for T-PANE-COLUMNS E8 foreground provenance.

use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};

use crate::{LeafSession, ShellAddress};

pub(crate) const OBSERVATION_INTERVAL: Duration = Duration::from_secs(5);

pub(crate) struct Request {
    pub(crate) address: ShellAddress,
    pub(crate) incarnation: u64,
    pub(crate) shell: bt_pty::ShellProcessId,
}

pub(crate) struct Answer {
    pub(crate) address: ShellAddress,
    pub(crate) incarnation: u64,
    pub(crate) program: bt_platform::foreground_program::ForegroundProgram,
}

pub(crate) struct Worker {
    pub(crate) requests: mpsc::Sender<Request>,
    pub(crate) answers: mpsc::Receiver<Answer>,
}

impl Worker {
    pub(crate) fn spawn(mut wake: impl FnMut() + Send + 'static) -> Result<Self> {
        let (request_tx, request_rx) = mpsc::channel::<Request>();
        let (answer_tx, answer_rx) = mpsc::channel::<Answer>();
        bt_platform::spawn_at_priority(
            "bt-foreground-program-worker",
            bt_platform::ThreadPriority::BelowNormal,
            move |worker| run(worker, request_rx, answer_tx, &mut wake),
        )
        .context("spawn foreground-program worker")?;
        Ok(Self {
            requests: request_tx,
            answers: answer_rx,
        })
    }
}

/// The request channel is itself a worker-only wait, so the same capability that admits the
/// platform observation also owns this loop in the effect registry.
fn run(
    worker: &bt_platform::admission::WorkerCtx,
    request_rx: mpsc::Receiver<Request>,
    answer_tx: mpsc::Sender<Answer>,
    wake: &mut impl FnMut(),
) {
    while let Ok(request) = request_rx.recv() {
        let program =
            bt_platform::foreground_program::foreground_program(worker, request.shell.get());
        if answer_tx
            .send(Answer {
                address: request.address,
                incarnation: request.incarnation,
                program,
            })
            .is_err()
        {
            return;
        }
        wake();
    }
}

/// Ask the worker about `leaf`'s shell when its cadence says so: at once after an OSC 133 command
/// start, and every [`OBSERVATION_INTERVAL`] while the pane's last capture held a frame candidate.
/// A pane without a shell process (or a worker that has gone) books no request, so the cadence is
/// not left waiting for an answer that cannot come.
pub(crate) fn request(
    leaf: &mut LeafSession,
    address: ShellAddress,
    requests: &mpsc::Sender<Request>,
    now: Instant,
) {
    let command_started = leaf.session.take_foreground_program_probe_request();
    let candidate = leaf.session.foreground_program_probe_is_armed();
    if !leaf
        .foreground_program_cadence
        .should_request(now, command_started, candidate)
    {
        return;
    }
    let sent = leaf
        .pty
        .as_ref()
        .and_then(bt_pty::PtySession::shell_process_id)
        .is_some_and(|shell| {
            requests
                .send(Request {
                    address,
                    incarnation: leaf.incarnation,
                    shell,
                })
                .is_ok()
        });
    if !sent {
        leaf.foreground_program_cadence.answered();
    }
}

/// When `leaf`'s cadence next asks, for the window's wake-up deadline.
pub(crate) fn deadline(leaf: &LeafSession) -> Option<Instant> {
    leaf.foreground_program_cadence
        .deadline(leaf.session.foreground_program_probe_is_armed())
}

/// Land one answer only in the shell incarnation that asked. `None` is a stale answer; `Some`
/// says whether the compact session-owned fact changed.
pub(crate) fn apply_answer(
    session: &mut LeafSession,
    incarnation: u64,
    program: bt_platform::foreground_program::ForegroundProgram,
) -> Option<bool> {
    if session.incarnation != incarnation {
        return None;
    }
    session.foreground_program_cadence.answered();
    let program = match program {
        bt_platform::foreground_program::ForegroundProgram::Known(image) => {
            bt_detect::ForegroundProgram::known(image)
        }
        bt_platform::foreground_program::ForegroundProgram::Unknown => {
            bt_detect::ForegroundProgram::Unknown
        }
    };
    let changed = session.session.foreground_program() != &program;
    session.session.apply_foreground_program(program);
    Some(changed)
}

/// Per-session cadence and coalescing. The clock is supplied by the app, so tests move it without
/// sleeping.
#[derive(Default)]
pub(crate) struct Cadence {
    in_flight: bool,
    next_periodic: Option<Instant>,
}

impl Cadence {
    pub(crate) fn should_request(
        &mut self,
        now: Instant,
        command_started: bool,
        candidate: bool,
    ) -> bool {
        if !candidate {
            self.next_periodic = None;
        } else if self.next_periodic.is_none() {
            self.next_periodic = Some(now + OBSERVATION_INTERVAL);
        }
        let periodic = candidate && self.next_periodic.is_some_and(|due| now >= due);
        if self.in_flight || !(command_started || periodic) {
            return false;
        }
        self.in_flight = true;
        self.next_periodic = candidate.then_some(now + OBSERVATION_INTERVAL);
        true
    }

    pub(crate) fn answered(&mut self) {
        self.in_flight = false;
    }

    pub(crate) fn deadline(&self, candidate: bool) -> Option<Instant> {
        (candidate && !self.in_flight)
            .then_some(self.next_periodic)
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_start_asks_immediately() {
        let start = Instant::now();
        assert!(Cadence::default().should_request(start, true, false));
    }

    #[test]
    fn no_frame_candidate_makes_no_periodic_request() {
        let start = Instant::now();
        let mut cadence = Cadence::default();
        assert!(!cadence.should_request(start, false, false));
        assert!(!cadence.should_request(start + Duration::from_secs(50), false, false));
    }

    #[test]
    fn a_frame_candidate_asks_at_the_five_second_boundary() {
        const FIVE_SECONDS: Duration = Duration::from_secs(5);
        let start = Instant::now();
        let mut cadence = Cadence::default();
        assert!(!cadence.should_request(start, false, true));
        assert_eq!(cadence.deadline(true), Some(start + FIVE_SECONDS));
        assert!(!cadence.should_request(
            start + FIVE_SECONDS - Duration::from_nanos(1),
            false,
            true
        ));
        assert!(cadence.should_request(start + FIVE_SECONDS, false, true));
        assert_eq!(cadence.deadline(true), None);
    }

    #[test]
    fn redundant_requests_are_coalesced_until_the_answer_lands() {
        let start = Instant::now();
        let mut cadence = Cadence::default();
        assert!(cadence.should_request(start, true, true));
        assert!(!cadence.should_request(start + OBSERVATION_INTERVAL, true, true));
        cadence.answered();
        assert!(cadence.should_request(start + OBSERVATION_INTERVAL, true, true));
    }
}
