//! **The OS hand-off lane** — every hand-off that leaves the window runs here, never on the
//! window thread (`docs/ARCHITECTURE.md` §5.1 and §5.3 row 1; `docs/DESIGN.md`, 2026-09-22 —
//! *a hand-off to the system runs on its own lane*).
//!
//! `ShellExecuteW` loads the shell extensions registered for what it is handed, in this process,
//! before it returns: a `Ctrl`+click on a printed `.md` held the window for 1447 ms, 1219 ms of it
//! the window thread's own CPU. The doors in `bt_platform::handoff` stay exactly as they are and
//! stay synchronous; this module is the one place that calls them, from one below-normal thread,
//! and the window hears the answer through its event loop.
//!
//! # The contract
//!
//! * **Identity.** Every request gets a [`HandoffId`] minted here, unique for the life of the
//!   process. The window that asked keeps what it owes the answer in its own [`Pending`], under
//!   that id — which is the target incarnation: a window that closes takes its `Pending` with it,
//!   and a new window that happens to reuse a platform handle starts with an empty one.
//! * **Ordering.** One thread, one FIFO queue: hand-offs are executed in press order, across
//!   every window, and answered in that order.
//! * **Capacity.** [`CAPACITY`] requests may wait. Nothing is coalesced — each press is a
//!   distinct request, and two presses on one link are two hand-offs, as they always were. A
//!   press that finds the queue full is not waited on: it is answered with [`LANE_FULL`] as its
//!   refusal, through the same wake and the same drain as every other answer.
//! * **Completion.** `Ok(())` — the operating system accepted it — or `Err(reason)` — the door's
//!   own refusal or the system's, byte for byte what the door returned. Never "the other program
//!   finished": the lane returns when `ShellExecuteW` or `NSWorkspace` does.
//! * **Abandonment.** A completion is raised only in the window whose [`Pending`] holds its id. A
//!   window that has closed holds nothing, so its completions are dropped — never raised in
//!   another window.
//! * **Wake.** The lane publishes the completion first and then wakes the loop
//!   (`AppEvent::HandoffAnswered`); the loop drains with `try_recv` and never blocks on the lane.
//!
//! Results come back the first of `ARCHITECTURE` §5.1's three ways — an `AppEvent` through the
//! event-loop proxy — because this lane is `bt-app`'s and may name `AppEvent`.

use std::collections::HashMap;
use std::sync::mpsc;

use anyhow::{Context, Result};
use bt_platform::admission::WorkerCtx;
use bt_platform::{Handoff, NativeWindow};

/// How many hand-offs may wait behind the one running.
///
/// Far beyond a person: each is one press. The bound exists so that a lane stuck inside a shell
/// extension that never returns is an accounted-for queue and not an unbounded one.
pub(crate) const CAPACITY: usize = 32;

/// The refusal a press meets when [`CAPACITY`] hand-offs are already waiting.
pub(crate) const LANE_FULL: &str = "the hand-off lane is full";

/// The refusal a press meets when the lane's thread is gone.
pub(crate) const LANE_GONE: &str = "the hand-off lane has stopped";

/// **One hand-off's name**, minted by [`HandoffLane::submit`] and unique in this process.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub(crate) struct HandoffId(u64);

/// What the window thread puts on the queue.
struct Request {
    id: HandoffId,
    window: NativeWindow,
    handoff: Handoff,
}

/// **One hand-off, answered** — the id it was asked under and the door's own `Result`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Completion {
    pub(crate) id: HandoffId,
    pub(crate) outcome: Result<(), String>,
}

/// **The lane's owner, held by the application** — the sending half, the answers, and the
/// counter that names requests.
pub(crate) struct HandoffLane {
    requests: mpsc::SyncSender<Request>,
    answers: mpsc::Receiver<Completion>,
    /// Presses the queue could not take, answered with their refusal on the next drain.
    turned_away: Vec<Completion>,
    /// The loop's wake, for those: a refusal nobody drains is a press with no answer.
    wake: Box<dyn Fn() + Send>,
    next: u64,
}

impl HandoffLane {
    /// **Start the lane** — one thread, `bt-os-handoff`, below normal, for the life of the
    /// process.
    ///
    /// The thread enters its [`bt_platform::ShellThread`] first, which on Windows is the COM
    /// apartment `ShellExecuteW` wants, and then hands each request to the door it names. The
    /// entry takes the capability the thread door lent this thread's body — the hand-off is a
    /// worker-only door, and this lane is the one worker that holds it.
    pub(crate) fn spawn(wake: impl Fn() + Clone + Send + 'static) -> Result<Self> {
        Self::start(
            |ctx| {
                let shell = bt_platform::ShellThread::enter(ctx);
                move |window: NativeWindow, handoff: &Handoff| shell.hand_over(window, handoff)
            },
            wake,
        )
    }

    /// The lane with the executor made on its own thread, from the capability the door lent it —
    /// the production one above, or a recording one in a test.
    fn start<M, E, W>(make_executor: M, wake: W) -> Result<Self>
    where
        M: FnOnce(&WorkerCtx) -> E + Send + 'static,
        E: FnMut(NativeWindow, &Handoff) -> Result<(), String>,
        W: Fn() + Clone + Send + 'static,
    {
        let (request_tx, request_rx) = mpsc::sync_channel::<Request>(CAPACITY);
        let (answer_tx, answer_rx) = mpsc::channel::<Completion>();
        let lane_wake = wake.clone();
        bt_platform::spawn_at_priority(
            "bt-os-handoff",
            bt_platform::ThreadPriority::BelowNormal,
            move |ctx| run_handoff_lane(request_rx, answer_tx, make_executor(ctx), lane_wake),
        )
        .context("spawn the OS hand-off lane")?;
        Ok(Self {
            requests: request_tx,
            answers: answer_rx,
            turned_away: Vec::new(),
            wake: Box::new(wake),
            next: 0,
        })
    }

    fn mint(&mut self) -> HandoffId {
        let id = HandoffId(self.next);
        self.next += 1;
        id
    }

    /// **Put one hand-off on the lane and return at once** with its id.
    ///
    /// A press the lane cannot take — [`CAPACITY`] already waiting, or the thread gone — is
    /// answered with that refusal on the next drain, like every other answer, so the caller never
    /// has two ways to hear one.
    pub(crate) fn submit(&mut self, window: NativeWindow, handoff: Handoff) -> HandoffId {
        let id = self.mint();
        let refused = match self.requests.try_send(Request {
            id,
            window,
            handoff,
        }) {
            Ok(()) => return id,
            Err(mpsc::TrySendError::Full(_)) => LANE_FULL,
            Err(mpsc::TrySendError::Disconnected(_)) => LANE_GONE,
        };
        self.turn_away(id, refused.to_owned());
        id
    }

    /// **A hand-off that never reached the lane**, answered with `reason` on the next drain — the
    /// press whose window could not be named, as well as the two refusals of [`Self::submit`].
    pub(crate) fn refuse(&mut self, reason: String) -> HandoffId {
        let id = self.mint();
        self.turn_away(id, reason);
        id
    }

    fn turn_away(&mut self, id: HandoffId, reason: String) {
        self.turned_away.push(Completion {
            id,
            outcome: Err(reason),
        });
        (self.wake)();
    }

    /// Every answer that has arrived, without waiting for one — the presses turned away first,
    /// then the lane's own in the order it answered them.
    pub(crate) fn answers(&mut self) -> Vec<Completion> {
        let mut all = std::mem::take(&mut self.turned_away);
        all.extend(self.answers.try_iter());
        all
    }
}

/// **The lane's body**: one request, one door, one answer, then the wake — `run_path_verify_worker`'s
/// shape, and nothing else runs here.
fn run_handoff_lane(
    requests: mpsc::Receiver<Request>,
    answers: mpsc::Sender<Completion>,
    mut execute: impl FnMut(NativeWindow, &Handoff) -> Result<(), String>,
    wake: impl Fn(),
) {
    while let Ok(Request {
        id,
        window,
        handoff,
    }) = requests.recv()
    {
        let outcome = execute(window, &handoff);
        if answers.send(Completion { id, outcome }).is_err() {
            return;
        }
        wake();
    }
}

/// **What one window owes the answers to its own hand-offs**, by id.
///
/// Held by the window, so that closing the window is what abandons them.
pub(crate) struct Pending<D> {
    owed: HashMap<HandoffId, D>,
}

impl<D> Default for Pending<D> {
    fn default() -> Self {
        Self {
            owed: HashMap::new(),
        }
    }
}

impl<D> Pending<D> {
    /// Remember what the answer to `id` is owed.
    pub(crate) fn owe(&mut self, id: HandoffId, duty: D) {
        self.owed.insert(id, duty);
    }

    /// The duty for `id`, to change what the answer will do — `None` when this window did not
    /// ask it or it has already been answered.
    pub(crate) fn duty_mut(&mut self, id: HandoffId) -> Option<&mut D> {
        self.owed.get_mut(&id)
    }

    /// **Claim an answer**: the duty when this window asked for it, and `None` otherwise — which
    /// is how a completion finds its window and no other.
    pub(crate) fn claim(&mut self, id: HandoffId) -> Option<D> {
        self.owed.remove(&id)
    }
}

/// **What one window owes one of its hand-offs' answers** — the refusal's words, and what a
/// surface asked to happen on either answer.
pub(crate) struct Duty {
    pub(crate) refusal: Refusal,
    pub(crate) accepted: Option<OnAccepted>,
    pub(crate) refused: Option<OnRefused>,
}

/// What a surface draws when the system has taken its hand-off.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OnAccepted {
    /// A foot's `Revealed` — it claims the reveal happened, so it waits for the answer.
    Revealed(crate::RevealedFoot),
    /// The no-preview card's `Opened`.
    PreviewOpened,
}

/// What a surface says, beyond the refusal's own words, when the system declines.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OnRefused {
    /// The hover line under the address a `Ctrl`+click was on.
    HyperlinkBlocked(bt_viewport::HyperlinkHit),
    /// The settings dialog's card under `Install fonts…`, holding the door's words.
    FontsToast,
    /// The notice a refused address raises on the preview surface a link was pressed on — a
    /// document's link to a scheme the machine has no handler for (ticket 14).
    PreviewAddressRefused(crate::PreviewSurface, String),
}

/// **What a refusal says and where**, fixed per surface when the press is made.
///
/// Each field is what that surface's code said on the window thread before the lane existed, so
/// a refusal that now arrives later arrives in the same words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Refusal {
    /// The stderr line's lead and the context the door's reason was wrapped in —
    /// `eprintln!("{lead}: {context}: {reason}")`, which is what `anyhow`'s `{:#}` printed.
    /// `None` for a surface that wrote no line.
    pub(crate) stderr: Option<(&'static str, &'static str)>,
    /// Whether the door's program refusal ([`bt_platform::PROGRAM_REFUSED`]) is told to the
    /// reader in the files notice — the surfaces where the user picked the file.
    pub(crate) program_notice: bool,
}

/// What a refusal produces, as values — the stderr line and the notice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RefusalWords {
    pub(crate) line: Option<String>,
    pub(crate) notice: Option<&'static str>,
}

impl Refusal {
    /// **The words a refusal with `reason` produces** — the one function the window's answer
    /// and the test both read.
    pub(crate) fn words(self, reason: &str) -> RefusalWords {
        RefusalWords {
            line: self
                .stderr
                .map(|(lead, context)| format!("{lead}: {context}: {reason}")),
            notice: (self.program_notice && reason.contains(bt_platform::PROGRAM_REFUSED))
                .then(crate::files_program_refused_notice),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use super::*;

    /// A lane whose executor writes down what it was handed and answers `answer`, so the lane's
    /// own queue, thread and wake are the real ones and only the door is a stand-in.
    fn recording_lane(
        answer: impl Fn(&Handoff) -> Result<(), String> + Send + 'static,
        delay: impl Fn(usize) -> Duration + Send + 'static,
    ) -> (HandoffLane, Arc<Mutex<Vec<Handoff>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let lane = HandoffLane::start(
            move |_ctx| {
                move |_window: NativeWindow, handoff: &Handoff| {
                    let count = {
                        let mut log = log.lock().expect("the log");
                        log.push(handoff.clone());
                        log.len()
                    };
                    std::thread::sleep(delay(count));
                    answer(handoff)
                }
            },
            || {},
        )
        .expect("the lane starts");
        (lane, seen)
    }

    /// Every answer to `count` requests, waited for with a deadline rather than forever.
    fn answers(lane: &mut HandoffLane, count: usize) -> Vec<Completion> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut all = Vec::new();
        while all.len() < count {
            all.extend(lane.answers());
            assert!(Instant::now() < deadline, "the lane answered {all:?}");
            std::thread::sleep(Duration::from_millis(2));
        }
        all
    }

    fn window() -> NativeWindow {
        NativeWindow::stand_in(0)
    }

    /// RED (ticket 10) — **hand-offs complete in press order.**
    ///
    /// Two presses in a row are two things the reader asked for in a sequence — a reveal and then
    /// an open, say — and a lane that ran them out of order would put the second window behind
    /// the first. The executor here takes longer over the first request than the later ones, so a
    /// lane that ran requests concurrently, or answered whichever finished first, is caught.
    ///
    /// MUTATION: spawn a thread per request in `run_handoff_lane` (`std::thread::spawn` around
    /// `execute`), and the answers arrive shortest-first.
    #[test]
    fn handoffs_complete_in_press_order() {
        let (mut lane, seen) = recording_lane(
            |_| Ok(()),
            |count| Duration::from_millis(if count == 1 { 60 } else { 1 }),
        );
        let requests: Vec<Handoff> = (0..5)
            .map(|at| Handoff::Address(format!("https://example.invalid/{at}")))
            .collect();
        let ids: Vec<HandoffId> = requests
            .iter()
            .map(|request| lane.submit(window(), request.clone()))
            .collect();
        let answered: Vec<HandoffId> = answers(&mut lane, ids.len())
            .into_iter()
            .map(|completion| completion.id)
            .collect();
        assert_eq!(answered, ids, "answered in the order pressed");
        assert_eq!(
            *seen.lock().expect("the log"),
            requests,
            "executed in the order pressed"
        );
    }

    /// RED (ticket 10) — **a completion for a closed window is dropped.**
    ///
    /// The answer to a hand-off belongs to the window that asked: a confirmation or a refusal
    /// raised in a different window would be a sentence about something that reader never did.
    /// A window's duties live in its own [`Pending`], so closing the window drops them; the ids
    /// are the lane's and unique in the process, so no other window can claim the answer by
    /// accident — including a window opened afterwards, which starts empty.
    ///
    /// MUTATION: make `Pending::claim` answer the first duty it holds whatever the id
    /// (`self.owed.drain().next().map(|(_, duty)| duty)`), and the second window claims the first
    /// window's answer.
    #[test]
    fn a_completion_for_a_closed_window_is_dropped() {
        let (mut lane, _) = recording_lane(|_| Ok(()), |_| Duration::ZERO);
        let mut first: Pending<&str> = Pending::default();
        let mut second: Pending<&str> = Pending::default();
        let asked = lane.submit(window(), Handoff::FontsPage);
        first.owe(asked, "the first window's confirmation");
        let other = lane.submit(window(), Handoff::FontsPage);
        second.owe(other, "the second window's confirmation");
        // The first window closes before its answer arrives.
        drop(first);
        let mut third: Pending<&str> = Pending::default();

        let all = answers(&mut lane, 2);
        let theirs = all
            .iter()
            .find(|completion| completion.id == asked)
            .expect("the lane answered the closed window's request");
        assert_eq!(
            second.claim(theirs.id),
            None,
            "not raised in another window"
        );
        assert_eq!(third.claim(theirs.id), None, "nor in one opened since");
        assert_eq!(
            second.claim(other),
            Some("the second window's confirmation"),
            "and the window that asked still hears its own"
        );
    }

    /// PIN (ticket 10) — **a flood is refused at the door of the queue, never waited on.**
    ///
    /// The window thread may not block on the lane. With the executor held, the queue fills to
    /// [`CAPACITY`] behind the one running, and every press past that is answered with
    /// [`LANE_FULL`] on the next drain — each press is its own request with its own id, so nothing
    /// is merged to make room.
    ///
    /// MUTATION: `send` instead of `try_send` in `submit`, and this test hangs past its deadline.
    #[test]
    fn a_flood_of_presses_is_refused_rather_than_waited_on() {
        let gate = Arc::new(Mutex::new(()));
        let held = gate.lock().expect("the gate");
        let inside = Arc::clone(&gate);
        let (mut lane, _) = recording_lane(
            move |_| {
                drop(inside.lock().expect("the gate"));
                Ok(())
            },
            |_| Duration::ZERO,
        );
        let started = Instant::now();
        let ids: Vec<HandoffId> = (0..CAPACITY + 3)
            .map(|at| lane.submit(window(), Handoff::Open(PathBuf::from(format!("{at}")))))
            .collect();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the press never waits"
        );
        assert!(
            ids.windows(2).all(|pair| pair[0] < pair[1]),
            "every press is its own request"
        );
        let refused: Vec<Completion> = lane.answers();
        assert!(
            refused.len() >= 2
                && refused
                    .iter()
                    .all(|answer| answer.outcome == Err(LANE_FULL.to_owned())),
            "the presses past the bound are answered with the refusal: {refused:?}"
        );
        drop(held);
    }

    /// RED (ticket 10) — **a refused hand-off raises the same words it did before.**
    ///
    /// The refusal used to be read off the door's `Result` on the window thread, wrapped in an
    /// `anyhow` context and printed with `{:#}`; the files notice was raised when that text held
    /// the program refusal. The door now answers on the lane, so the words are rebuilt from a
    /// [`Completion`] — and a reader must not be able to tell. Run through the **real** lane with
    /// the **real** [`bt_platform::ShellThread`] over the **real** verifier's answer for a real
    /// folder, against the old code's own spelling of the same line. The door's own answer for that
    /// spelling is asked through the only road to it that is left (A1b made the verbs private to
    /// `bt_platform::handoff`): a `ShellThread` entered on a second thread the thread door started,
    /// handed the same two requests — not through the lane under test.
    ///
    /// The program is a folder so that one fixture is a program on both machines without touching
    /// a mode bit: `payload.exe` is one by name to Windows, `Payload.app` is one by bundle to
    /// macOS, and `bt_term::verify_path` answers both. A third platform's door refuses everything
    /// with its own sentence, and the line is held to that sentence the same way.
    ///
    /// MUTATION: format the line as `"{lead}: {reason}"` in [`Refusal::words`], or drop the
    /// `contains(PROGRAM_REFUSED)` test, and the words differ.
    #[test]
    fn a_refused_handoff_raises_the_same_words_it_did_before() {
        let scratch =
            std::env::temp_dir().join(format!("folio-handoff-refusal-{}", std::process::id()));
        let program = scratch.join(match bt_platform::host_platform() {
            bt_platform::HostPlatform::MacOs => "Payload.app",
            _ => "payload.exe",
        });
        std::fs::create_dir_all(&program).expect("a scratch program");
        let missing = scratch.join("gone.md");
        let facts = crate::verified_target_of(Some(&bt_term::verify_path(&program)));
        assert!(facts.exists, "the verifier saw the fixture");

        let mut lane = HandoffLane::spawn(|| {}).expect("the lane starts");
        let requests = [
            Handoff::OpenVerified(program.clone(), facts.clone()),
            Handoff::OpenVerified(missing.clone(), bt_platform::VerifiedTarget::absent()),
        ];
        for request in &requests {
            lane.submit(window(), request.clone());
        }
        let answered = answers(&mut lane, requests.len());

        // The old surface: `Runtime::open_local_path_verified` as it stood on the window thread.
        let refusal = Refusal {
            stderr: Some((
                "recoverable reference open failure",
                "open a printed reference with its default handler",
            )),
            program_notice: true,
        };
        let before = |door: Result<(), String>| {
            let error = door
                .map_err(|error| anyhow::anyhow!(error))
                .context("open a printed reference with its default handler")
                .expect_err("the door refuses this");
            let text = format!("{error:#}");
            RefusalWords {
                line: Some(format!("recoverable reference open failure: {text}")),
                notice: text
                    .contains(bt_platform::PROGRAM_REFUSED)
                    .then(crate::files_program_refused_notice),
            }
        };
        let doors = bt_platform::spawn_at_priority(
            "bt-test-handoff-door",
            bt_platform::ThreadPriority::BelowNormal,
            move |ctx| {
                let shell = bt_platform::ShellThread::enter(ctx);
                requests.map(|request| shell.hand_over(window(), &request))
            },
        )
        .expect("the door starts a thread")
        .join()
        .expect("the door answered");
        let old = doors.map(before);
        for (completion, old) in answered.iter().zip(old) {
            let reason = completion
                .outcome
                .as_ref()
                .expect_err("the lane carried the door's refusal");
            assert_eq!(refusal.words(reason), old, "{completion:?}");
        }
        if bt_platform::host_platform() != bt_platform::HostPlatform::OtherUnix {
            assert_eq!(
                refusal
                    .words(answered[0].outcome.as_ref().unwrap_err())
                    .notice,
                Some(crate::files_program_refused_notice()),
                "a program is refused in the reader's words"
            );
        }

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// RED GATE (ticket 10) — **no hand-off runs on the window thread**: nothing in the product
    /// names a `bt_platform::handoff` door, or the thread type that calls them, except this
    /// module — so every one of them is reached through the lane and none from a `Runtime`
    /// method.
    ///
    /// Read through `bt_source` rather than as text of a file, so a door call moved into any file
    /// of this crate is still found, and its owner named. Both spellings of each door are asked:
    /// the crate-root re-export (`bt_platform::open_local_path`) and the module path
    /// (`handoff::open_local_path`), and a `use` that imports either — which would let a bare call
    /// hide from the first two — is an occurrence outside every callable and is refused as well.
    ///
    /// MUTATION: make `Runtime::open_local_path` call
    /// `bt_platform::open_local_path(native, path)` directly again, and this names it.
    #[test]
    fn no_handoff_runs_on_the_window_thread() {
        use bt_source::{Index, Pattern, Search, View, needle};

        let index = Index::of_package("bt-app");
        let mut names: Vec<String> = [
            "open_local_path",
            "open_local_path_verified",
            "reveal_in_explorer",
            "reveal_verified",
            "shell_execute",
            "open_local_file",
            "open_system_fonts_page",
        ]
        .iter()
        .flat_map(|door| [format!("bt_platform::{door}"), format!("handoff::{door}")])
        .collect();
        names.push("bt_platform::ShellThread".to_owned());
        let mut lane_callers = 0;
        for name in &names {
            let found = index
                .search(&Search::new(
                    needle!(Pattern::path(name)),
                    View::Identifiers,
                ))
                .unwrap_or_else(|failure| panic!("{failure}"))
                .in_the_product(index);
            assert_eq!(
                found.outside_items(index),
                0,
                "`{name}` is imported or named outside any function:\n{}",
                found.report(index)
            );
            for (owner, count) in found.owners(index) {
                assert!(
                    owner.module_path == "crate::handoff_lane"
                        || owner.module_path.starts_with("crate::handoff_lane::"),
                    "`{name}` is called from {owner} ({count}×), outside the OS hand-off lane — \
                     a door called there runs on the window thread:\n{}",
                    found.report(index)
                );
                lane_callers += count;
            }
        }
        assert!(
            lane_callers >= 1,
            "the lane itself no longer reaches the doors, so this gate is reading nothing"
        );
    }
}

/// **The lane contract's adapter** (`crate::lane`, `lane_contract_tests`): the real lane — its
/// queue, its thread, its drain and each window's [`Pending`] — with the suite's
/// [`crate::lane::Gate`] standing in for the door and its [`crate::lane::WakeProbe`] for the
/// loop's wake. Nothing of the lane is copied.
#[cfg(test)]
pub(crate) mod contract_adapter {
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Arc;

    use bt_platform::{Handoff, NativeWindow};

    use super::{Completion, HandoffId, HandoffLane, Pending};
    use crate::lane::{
        Admission, Contract, Delivered, Gate, HANDOFF, LaneUnderTest, Outcome, WakeProbe,
    };

    /// The address a question is handed over as, so the executor can say which one it runs.
    const ADDRESS: &str = "https://lane-contract.invalid/";

    fn question_of(handoff: &Handoff) -> Option<u64> {
        match handoff {
            Handoff::Address(address) => address.strip_prefix(ADDRESS)?.parse().ok(),
            _ => None,
        }
    }

    struct HandoffAdapter {
        lane: HandoffLane,
        gate: Arc<Gate>,
        probe: Arc<WakeProbe>,
        /// One `Pending` per open window; a closed window's is dropped, as `WindowRuntime`'s is.
        windows: BTreeMap<u32, Pending<u64>>,
        /// The suite's own record of which question each id carried, for its tally.
        questions: HashMap<HandoffId, u64>,
    }

    /// A fresh lane on its own `bt-os-handoff` thread.
    pub(crate) fn make() -> Box<dyn LaneUnderTest> {
        let gate = Arc::new(Gate::default());
        let probe = Arc::new(WakeProbe::default());
        let door = Arc::clone(&gate);
        let wake = Arc::clone(&probe);
        let lane = HandoffLane::start(
            move |_ctx| {
                move |_window: NativeWindow, handoff: &Handoff| {
                    door.pass(question_of(handoff));
                    Ok(())
                }
            },
            move || wake.woke(),
        )
        .expect("the hand-off lane starts");
        Box::new(HandoffAdapter {
            lane,
            gate,
            probe,
            windows: BTreeMap::new(),
            questions: HashMap::new(),
        })
    }

    impl LaneUnderTest for HandoffAdapter {
        fn contract(&self) -> &'static Contract {
            &HANDOFF
        }

        fn gate(&self) -> &Gate {
            &self.gate
        }

        fn probe(&self) -> &WakeProbe {
            &self.probe
        }

        fn submit(&mut self, target: u32, question: u64) -> Admission {
            let id = self.lane.submit(
                NativeWindow::stand_in(0),
                Handoff::Address(format!("{ADDRESS}{question}")),
            );
            self.questions.insert(id, question);
            self.windows.entry(target).or_default().owe(id, question);
            Admission {
                ticket: Some(id.0),
                question,
                refused: None,
            }
        }

        fn close_target(&mut self, target: u32) {
            self.windows.remove(&target);
        }

        /// `FolioApp::answer_handoffs`: every answer offered to every open window, claimed by the
        /// one whose `Pending` holds its id.
        fn drain(&mut self) -> Vec<Delivered> {
            let answers = self.lane.answers();
            answers
                .into_iter()
                .map(|Completion { id, outcome }| Delivered {
                    target: self
                        .windows
                        .iter_mut()
                        .find_map(|(window, pending)| pending.claim(id).map(|_| *window)),
                    ticket: Some(id.0),
                    question: self.questions.get(&id).copied(),
                    outcome: match outcome {
                        Ok(()) => Outcome::Answered,
                        Err(reason) => Outcome::Refused(reason),
                    },
                })
                .collect()
        }

        fn offer_stale(&mut self, _ticket: u64) {
            unreachable!("the hand-off lane's target is the asking window, not the application");
        }
    }
}
