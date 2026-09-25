//! **The web engine, warmed on an idle turn after startup** (0.4.5 ticket 54,
//! D-64; `docs/ARCHITECTURE.md` §5.3 row 21).
//!
//! The first web page a process opened paid for the WebView2 runtime's start —
//! the environment's creation call and the browser, GPU and utility processes
//! it launches — inside the gesture that asked for the page, on the window
//! thread. Ticket 43 measured that this cannot move to a lane (WebView2 answers
//! `0x802A000C` to an environment used from a thread other than its creator's),
//! and the ruling of 2026-09-24 is to ask for it earlier instead: on a turn
//! where nothing else is owed, a few seconds after the first frame, through the
//! same door a page uses. The controller is still made per page.
//!
//! This module is the clock's state and its one decision, kept apart from the
//! runtime so the decision can be driven through turns without a window. The
//! process's environment itself is [`bt_platform::EnvironmentSlot`]'s, not
//! this clock's: the clock asks only when the slot says nobody has, and a page
//! that asks while the warm-up's request is in flight waits for it there.
//!
//! **What counts as quiet**, reusing the turn's own notions rather than
//! inventing new ones:
//! * a gesture — the same classifier the file-read ledger already uses for
//!   "the reader did something" ([`crate::file_reads::is_user_input`]);
//! * output drained from any shell (`Runtime::drain_pty`);
//! * a window not at rest — the idle test the turn's own `BT_PERF_TRACE
//!   idle_wake` line uses (`Runtime::window_at_rest`), or a resize still owed
//!   to a shell;
//! * the restore card up.
//!
//! The first three are *stirs*: each moves the earliest instant the warm-up may
//! fire to [`WEB_ENGINE_WARMUP_QUIET`] after it, so a quiet turn is one that
//! follows a quiet stretch, and the clock always has a future instant to wake
//! for. The restore card is not a stir: it can stand for as long as the reader
//! leaves it, and whatever takes it down is a gesture, which is.
//!
//! **A second stage, the spare** (0.4.5 ticket 60; owner's ruling 2026-09-25, option A). After
//! the environment's turn, the same clock may make one spare web controller for the first
//! eligible page to adopt — on its own quiet turn, counted afresh from the environment's, so the
//! two never share one; only for a profile whose history holds a page (`web_pages_used`); and only
//! while no window holds a page. At most one per process, attempted once, never replenished.

use std::time::{Duration, Instant};

use bt_platform::{EnvironmentAnswer, WebWarmUp};

/// **How long after the first presented frame the engine may be warmed.**
///
/// The launch's own work is done in the first seconds: the shells start and
/// print their first prompts, a restored session's panes are born, the font
/// list is walked, and each of those is a burst of work on this thread or
/// beside it. Five seconds is after that burst on the development machine and
/// before the owner's acceptance road (a web pane opened about ten seconds
/// after launch). A pane opened sooner than this still makes the request
/// itself, exactly as before ticket 54.
pub(crate) const WEB_ENGINE_WARMUP_AFTER: Duration = Duration::from_secs(5);

/// **How long nothing may have stirred before the warm-up fires.**
///
/// The creation call itself holds this thread for 10–38 ms on the first ask
/// (ticket 43's measurement), and the processes it launches compete for the
/// machine while they start. A second with no gesture and no output is a pause
/// between bursts, not a gap between keystrokes: typing keeps its inter-key
/// gaps well under it, so the warm-up does not land inside a word.
pub(crate) const WEB_ENGINE_WARMUP_QUIET: Duration = Duration::from_secs(1);

/// **Whether a window event is the reader doing something** — a gesture (the file-read ledger's
/// own classifier, [`crate::file_reads::is_user_input`]) or the window resized, moved or carried
/// to another scale (ticket 60, F1).
pub(crate) fn event_stirs(event: &winit::event::WindowEvent) -> bool {
    use winit::event::WindowEvent;
    crate::file_reads::is_user_input(event)
        || matches!(
            event,
            WindowEvent::Resized(_)
                | WindowEvent::Moved(_)
                | WindowEvent::ScaleFactorChanged { .. }
        )
}

/// **What one window's turn says about whether anything is happening in it** (tickets 54 and
/// 60) — the one derivation both the environment's warm-up and the spare's stage read.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TurnFacts {
    /// Everything in motion, Folio's own periodics included — the pacer's lanes.
    pub(crate) running: crate::pace::Lanes,
    /// The motion a gesture set going: `running` less the periodics.
    pub(crate) travelling: crate::pace::Lanes,
    /// A frame owed to the glass: a refused animation frame, a shell frame not yet presented, a
    /// chrome present pending (a caret's phase among them), a picture or a card owed one.
    pub(crate) repaint_owed: bool,
    /// A resize still owed to a shell, or one still settling.
    pub(crate) resize_owed: bool,
}

impl TurnFacts {
    /// **Nothing moving and nothing owed to the glass** — the turn's `BT_PERF_TRACE idle_wake`
    /// test.
    pub(crate) fn at_rest(&self) -> bool {
        !self.running.any() && !self.repaint_owed
    }

    /// **Whether this turn stirs the warm-up** (ticket 60, F1; supersedes ticket 54's
    /// "a window not at rest").
    ///
    /// A gesture and a shell's output are stirred where they arrive; what a turn adds is a window
    /// mid-way through motion a gesture set going, or a resize still owed. **A repaint Folio
    /// scheduled for itself is not a stir**: a blinking caret presents twice a second and carries
    /// nothing new, and counted as one it held the warm-up's quiet stretch shut for as long as a
    /// shell had the keyboard (measured on the clean VM: neither the environment nor the spare was
    /// ever asked for at a prompt). Content arriving on its own — a shell frame, a picture — is
    /// already a stir where it arrives, or nobody's doing.
    pub(crate) fn stirs(&self) -> bool {
        self.travelling.any() || self.resize_owed
    }
}

/// The one line `diagnostics.log` gets when the warm-up's request fails.
pub(crate) fn warm_up_failed_line(error: &str) -> String {
    format!("the web engine's warm-up failed: {error}; the first page will ask again")
}

/// **The engine's door, as the clock sees it.** One implementation in the
/// product ([`ThisProcess`], the process's own environment); a test drives the
/// clock through a real [`bt_platform::EnvironmentSlot`] with the creation call
/// recorded instead of made.
pub(crate) trait EngineDoor {
    /// Ask for the environment, only if nobody has — the slot's own
    /// [`bt_platform::EnvironmentSlot::warm`], the one place that judges it.
    fn warm(&mut self, answered: EnvironmentAnswer) -> Result<WebWarmUp, String>;
}

/// **The process's own environment**, over the profile folder every web seat
/// is given ([`crate::webhost::user_data_folder`], the same function
/// `WebSeat::open` asks). A machine where that folder cannot be named has no
/// seat that could use an environment either, so there is nothing to warm.
pub(crate) struct ThisProcess;

impl EngineDoor for ThisProcess {
    fn warm(&mut self, answered: EnvironmentAnswer) -> Result<WebWarmUp, String> {
        match crate::webhost::user_data_folder() {
            Some(folder) => bt_platform::warm_web_environment(&folder, answered),
            None => Ok(WebWarmUp::NothingToWarm),
        }
    }
}

/// **The warm-up clock's state** — the application's, one per process, because
/// the environment is.
#[derive(Debug, Default)]
pub(crate) struct WebWarmup {
    /// The first presented frame this clock was told of: the grace is counted
    /// from it.
    first_frame_at: Option<Instant>,
    /// The last instant something stirred: a gesture, output drained, a window
    /// not at rest.
    stirred_at: Option<Instant>,
    /// **Which of its two turns the clock is waiting for** (tickets 54 and 60). Only ever moves
    /// forward: the environment's turn, then the spare's, then nothing. Never back: a failed
    /// warm-up leaves the environment's state empty and the next page asks again, as it did before
    /// ticket 54, and a spare is attempted once.
    stage: Stage,
    /// When the environment's turn fired: the spare's quiet stretch is counted from it too.
    environment_turn_at: Option<Instant>,
}

/// The clock's stages, in the only order they run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Stage {
    /// The environment has not been asked for.
    #[default]
    Environment,
    /// The environment's turn has fired; the spare's has not.
    Spare,
    /// Nothing more is owed.
    Done,
}

/// **What the spare's turn decided** (ticket 60).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SpareDue {
    /// Make the spare now, on this quiet turn.
    Make,
    /// Not for this process: the profile has never opened a page, or a page is already open.
    Declined,
}

impl WebWarmup {
    /// **Something stirred at `at`**: the quiet stretch starts again from here.
    pub(crate) fn stir(&mut self, at: Instant) {
        self.stirred_at = Some(self.stirred_at.map_or(at, |stirred| stirred.max(at)));
    }

    /// **A frame has been presented**, at `presented_at` if there is one yet.
    /// Only the first is kept.
    pub(crate) fn saw_frame(&mut self, presented_at: Option<Instant>) {
        if self.first_frame_at.is_none() {
            self.first_frame_at = presented_at;
        }
    }

    /// The earliest instant the warm-up may fire, from the two clocks: the
    /// grace after the first frame, and the quiet stretch after the last stir.
    /// `None` before the first frame.
    fn ready_at(&self) -> Option<Instant> {
        let grace = self.first_frame_at? + WEB_ENGINE_WARMUP_AFTER;
        Some(match self.stirred_at {
            Some(stirred) => grace.max(stirred + WEB_ENGINE_WARMUP_QUIET),
            None => grace,
        })
    }

    /// **The instant this clock wants a turn at**, for the turn's wake fold.
    ///
    /// `None` once it has fired, before the first frame (the frame's own turn
    /// comes regardless), and while the restore card is up (whatever takes the
    /// card down is a gesture, and the gesture books the next instant).
    pub(crate) fn deadline(&self, restore_card_up: bool) -> Option<Instant> {
        if restore_card_up {
            return None;
        }
        match self.stage {
            Stage::Environment => self.ready_at(),
            Stage::Spare => self.spare_ready_at(),
            Stage::Done => None,
        }
    }

    /// The earliest instant the spare's turn may fire: a quiet stretch after the environment's
    /// turn and after the last stir, and never before the grace.
    fn spare_ready_at(&self) -> Option<Instant> {
        let fired = self.environment_turn_at? + WEB_ENGINE_WARMUP_QUIET;
        Some(self.ready_at()?.max(fired))
    }

    /// **The instant from which a turn is quiet** — the end of the quiet stretch after the last
    /// stir, and never before the grace — or `None` while the restore card stands (ticket 60,
    /// SW-6). The spare, while it is being made, is advanced only on a turn at or after this, so
    /// its controller call — up to 590 ms on the window thread, measured — never lands inside a
    /// burst of typing, however late the environment's answer arrived.
    pub(crate) fn quiet_at(&self, restore_card_up: bool) -> Option<Instant> {
        if restore_card_up {
            return None;
        }
        self.ready_at()
    }

    /// Whether `now` is a quiet turn — see [`Self::quiet_at`].
    pub(crate) fn is_quiet(&self, now: Instant, restore_card_up: bool) -> bool {
        self.quiet_at(restore_card_up).is_some_and(|at| now >= at)
    }

    /// **The spare's turn** (ticket 60): `Some` on the first quiet turn after the environment's
    /// turn — `Make` when `pages_used` (the profile's receipt) and no window holds a page,
    /// `Declined` otherwise — and `None` on every other turn. Either answer ends the clock: at
    /// most one spare per process, attempted once.
    pub(crate) fn spare_turn(
        &mut self,
        now: Instant,
        restore_card_up: bool,
        pages_used: bool,
        page_open: bool,
    ) -> Option<SpareDue> {
        if self.stage != Stage::Spare || restore_card_up {
            return None;
        }
        if now < self.spare_ready_at()? {
            return None;
        }
        self.stage = Stage::Done;
        Some(if pages_used && !page_open {
            SpareDue::Make
        } else {
            SpareDue::Declined
        })
    }

    /// **One turn of the clock.** Asks the door for the environment when the
    /// grace has passed, nothing has stirred for the quiet stretch and the
    /// restore card is down — and the door asks only if nobody has yet (a page
    /// that asked first answers `AlreadyAsked`); answers what the door said, or
    /// `None` on a turn that asked nothing.
    ///
    /// Never waits: the answer arrives through the closure, on a later turn of
    /// the message pump or before the door returns, and a failure — there, or
    /// where the call stands — is one line through `say` and nothing else.
    pub(crate) fn turn(
        &mut self,
        now: Instant,
        restore_card_up: bool,
        door: &mut dyn EngineDoor,
        say: fn(&str),
    ) -> Option<Result<WebWarmUp, String>> {
        if self.stage != Stage::Environment || restore_card_up {
            return None;
        }
        if now < self.ready_at()? {
            return None;
        }
        let asked = door.warm(Box::new(move |error| {
            if let Some(error) = error {
                say(&warm_up_failed_line(&error));
            }
        }));
        if let Err(error) = &asked {
            say(&warm_up_failed_line(error));
        }
        // A platform with nothing to warm has no controller to make ahead of time either.
        self.stage = if asked == Ok(WebWarmUp::NothingToWarm) {
            Stage::Done
        } else {
            Stage::Spare
        };
        self.environment_turn_at = Some(now);
        Some(asked)
    }
}

#[cfg(test)]
mod web_warmup_tests {
    //! The clock driven through turns, headless. The door is the real
    //! [`bt_platform::EnvironmentSlot`] — the lifecycle the Windows arm keeps
    //! for the process's environment — with the one thing a test cannot do
    //! headless, the creation call, recorded instead of made.

    use std::cell::RefCell;
    use std::time::{Duration, Instant};

    use bt_platform::{
        EnvironmentAnswer, EnvironmentAsk, EnvironmentSlot, WebEnvironmentPhase, WebWarmUp,
    };

    use super::{
        EngineDoor, WEB_ENGINE_WARMUP_AFTER, WEB_ENGINE_WARMUP_QUIET, WebWarmup,
        warm_up_failed_line,
    };

    thread_local! {
        /// What the clock said to `diagnostics.log`, per test thread.
        static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn say(line: &str) {
        SAID.with(|said| said.borrow_mut().push(line.to_owned()));
    }

    fn said() -> Vec<String> {
        SAID.with(|said| said.borrow().clone())
    }

    /// The process's slot, with every creation call it asks for recorded.
    #[derive(Default)]
    struct Recorded {
        slot: EnvironmentSlot<u32>,
        creations: Vec<u64>,
    }

    impl Recorded {
        fn carry_out(&mut self, asked: EnvironmentAsk) {
            match asked {
                EnvironmentAsk::Answer(answer) => answer(None),
                EnvironmentAsk::Create(ticket) => self.creations.push(ticket),
                EnvironmentAsk::Joined | EnvironmentAsk::AlreadyAsked => {}
            }
        }

        /// A page's `request_environment`: the same `ask` the Windows arm makes.
        fn page_asks(&mut self) {
            let asked = self.slot.ask(Box::new(|_| {}));
            self.carry_out(asked);
        }

        /// The creation call answers.
        fn answer(&mut self, result: Result<u32, String>) {
            let ticket = *self.creations.last().expect("a call was made");
            let (waiting, error) = self.slot.arrived(ticket, result);
            for answer in waiting {
                answer(error.clone());
            }
        }
    }

    impl Recorded {
        fn phase(&self) -> WebEnvironmentPhase {
            self.slot.phase()
        }
    }

    impl EngineDoor for Recorded {
        fn warm(&mut self, answered: EnvironmentAnswer) -> Result<WebWarmUp, String> {
            match self.slot.warm(answered) {
                EnvironmentAsk::AlreadyAsked => Ok(WebWarmUp::AlreadyAsked),
                asked => {
                    self.carry_out(asked);
                    Ok(WebWarmUp::Asked)
                }
            }
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// RED (54) — **The web engine is asked for once, on a quiet turn after the
    /// startup grace, before any page.**
    ///
    /// Before ticket 54 nothing asked for the environment but a page, so the
    /// first page paid for the runtime's start inside its gesture (§5.3 row
    /// 21). The clock is driven through turns every 100 ms from the first
    /// frame: nothing before the grace, exactly one creation call on the first
    /// quiet turn after it, and nothing on any turn after that — including
    /// after the environment has arrived and been let go of again (the rebuild
    /// road's `forget_web_environment`), when only a page may ask.
    ///
    /// MUTATION: drop the once-per-process guard (the stage moving on in
    /// `turn`) and a second creation call appears after the environment is
    /// let go of. Or drop the grace from `ready_at` and the request lands on
    /// the first turn.
    #[test]
    fn the_web_engine_is_asked_for_once_on_a_quiet_turn_after_the_startup_grace() {
        let start = Instant::now();
        let mut clock = WebWarmup::default();
        let mut door = Recorded::default();
        clock.saw_frame(Some(start));
        let mut asked_at = Vec::new();
        for step in 0..100 {
            let now = start + ms(100 * step);
            clock.saw_frame(Some(now));
            if let Some(answer) = clock.turn(now, false, &mut door, say) {
                asked_at.push((now - start, answer));
            }
            // The spare's turn (ticket 60), for a profile that has never opened a page: declined,
            // and the clock has nothing left to ask for.
            let _ = clock.spare_turn(now, false, false, false);
            if step == 60 {
                door.answer(Ok(1));
                clock.stir(now);
            }
            if step == 70 {
                door.slot.forget();
            }
        }
        assert_eq!(
            asked_at,
            vec![(WEB_ENGINE_WARMUP_AFTER, Ok(WebWarmUp::Asked))],
            "one ask, on the first turn at the end of the grace, and none after"
        );
        assert_eq!(door.creations.len(), 1, "one creation call in the process");
        assert_eq!(door.phase(), WebEnvironmentPhase::None);
        assert_eq!(
            clock.deadline(false),
            None,
            "and no wake-up is booked after it"
        );
        assert!(said().is_empty());
    }

    /// RED (54) — **A page asked for before the warm-up fires makes the only
    /// request, and the warm-up then makes none.**
    ///
    /// The page's ask is the slot's own `ask`, the call
    /// `WebHost::request_environment` makes. The warm-up that comes due while
    /// that request is in flight, and after it has arrived, must neither make a
    /// second call nor join it.
    ///
    /// MUTATION: let the warm-up ask when the state is already `Requested`
    /// (`EnvironmentSlot::warm` falling through to `ask`) and it joins the
    /// page's request: the turn answers `Asked`, a second reader of the one call.
    #[test]
    fn a_page_asked_for_before_the_warm_up_fires_makes_the_only_request() {
        let start = Instant::now();
        let mut clock = WebWarmup::default();
        let mut door = Recorded::default();
        clock.saw_frame(Some(start));
        door.page_asks();
        assert_eq!(door.phase(), WebEnvironmentPhase::Requested);
        let due = start + WEB_ENGINE_WARMUP_AFTER;
        assert_eq!(
            clock.turn(due, false, &mut door, say),
            Some(Ok(WebWarmUp::AlreadyAsked)),
            "the warm-up finds the page's request and asks nothing"
        );
        door.answer(Ok(3));
        for step in 1..20 {
            assert_eq!(
                clock.turn(due + ms(100 * step), false, &mut door, say),
                None
            );
            // The page is open, so the spare's turn declines (ticket 60).
            let _ = clock.spare_turn(due + ms(100 * step), false, true, true);
        }
        assert_eq!(door.creations.len(), 1, "the page's call is the only call");
        assert_eq!(clock.deadline(false), None);
    }

    /// RED (54) — **A busy turn is not a quiet one.** PTY output drained, a
    /// gesture inside the quiet window, a window not at rest, or the restore
    /// card up: no request that turn.
    ///
    /// Each of the first three is a stir, and moves the earliest instant to
    /// [`WEB_ENGINE_WARMUP_QUIET`] after it; the card holds the clock with no
    /// wake-up booked for as long as it stands.
    ///
    /// MUTATION: drop the stir from `ready_at` (ignore `stirred_at`), or drop
    /// the `restore_card_up` condition from `turn`, and the request lands on a
    /// busy turn.
    #[test]
    fn a_busy_turn_is_not_a_quiet_one() {
        let start = Instant::now();
        let due = start + WEB_ENGINE_WARMUP_AFTER;

        // Output drained (or a gesture, or a window not at rest: one stir) on
        // the very turn the grace ends.
        let mut clock = WebWarmup::default();
        let mut door = Recorded::default();
        clock.saw_frame(Some(start));
        clock.stir(due);
        assert_eq!(clock.turn(due, false, &mut door, say), None);
        assert_eq!(
            clock.deadline(false),
            Some(due + WEB_ENGINE_WARMUP_QUIET),
            "the clock books the end of the quiet stretch, never an instant already past"
        );
        assert_eq!(
            clock.turn(due + WEB_ENGINE_WARMUP_QUIET - ms(1), false, &mut door, say),
            None,
            "still inside the quiet window"
        );
        assert!(door.creations.is_empty());
        assert_eq!(
            clock.turn(due + WEB_ENGINE_WARMUP_QUIET, false, &mut door, say),
            Some(Ok(WebWarmUp::Asked))
        );

        // A gesture well before the grace ends does not delay it.
        let mut clock = WebWarmup::default();
        let mut door = Recorded::default();
        clock.saw_frame(Some(start));
        clock.stir(start + ms(500));
        assert_eq!(
            clock.turn(due, false, &mut door, say),
            Some(Ok(WebWarmUp::Asked))
        );

        // The restore card up: no request, and no wake-up booked while it stands.
        let mut clock = WebWarmup::default();
        let mut door = Recorded::default();
        clock.saw_frame(Some(start));
        for step in 0..30 {
            assert_eq!(clock.turn(due + ms(100 * step), true, &mut door, say), None);
        }
        assert_eq!(clock.deadline(true), None);
        assert!(door.creations.is_empty());
        // The answer that takes it down is a gesture.
        let answered = due + ms(3_000);
        clock.stir(answered);
        assert_eq!(clock.turn(answered, false, &mut door, say), None);
        assert_eq!(
            clock.turn(answered + WEB_ENGINE_WARMUP_QUIET, false, &mut door, say),
            Some(Ok(WebWarmUp::Asked))
        );

        // And before the first frame there is nothing to count from.
        let mut clock = WebWarmup::default();
        let mut door = Recorded::default();
        assert_eq!(clock.turn(due, false, &mut door, say), None);
        assert_eq!(clock.deadline(false), None);
    }

    /// RED (54) — **A failed warm-up leaves the state empty and the next page
    /// asks again**, and says so in one line.
    ///
    /// The failure (no runtime, `0x8007139F` from a second Folio whose options
    /// differ, anything) is answered to the warm-up's closure, which writes one
    /// `diagnostics.log` line and nothing else; the slot is empty afterwards, so
    /// a page's request makes a new creation call rather than joining a request
    /// that has already answered. The clock does not ask a second time.
    ///
    /// MUTATION: leave the slot `Requested` after a failed completion
    /// (`EnvironmentSlot::arrived`), and the page joins a request that will
    /// never answer: no second creation call.
    #[test]
    fn a_failed_warm_up_leaves_the_state_empty_and_the_next_page_asks_again() {
        let start = Instant::now();
        let mut clock = WebWarmup::default();
        let mut door = Recorded::default();
        clock.saw_frame(Some(start));
        let due = start + WEB_ENGINE_WARMUP_AFTER;
        assert_eq!(
            clock.turn(due, false, &mut door, say),
            Some(Ok(WebWarmUp::Asked))
        );
        door.answer(Err(
            "CreateCoreWebView2Environment failed (0x8007139F)".to_owned()
        ));
        assert_eq!(door.phase(), WebEnvironmentPhase::None);
        assert_eq!(
            said(),
            vec![warm_up_failed_line(
                "CreateCoreWebView2Environment failed (0x8007139F)"
            )],
            "one line, naming the failure"
        );
        for step in 1..20 {
            assert_eq!(
                clock.turn(due + ms(100 * step), false, &mut door, say),
                None
            );
        }
        door.page_asks();
        assert_eq!(
            door.creations.len(),
            2,
            "the page makes its own call, as it did before ticket 54"
        );
        assert_eq!(door.phase(), WebEnvironmentPhase::Requested);
    }

    /// RED (60, F1) — **a window whose shell caret blinks and nothing else stays quiet, and the
    /// warm-up, then the spare, fires after the grace.**
    ///
    /// The caret's phase is a chrome present Folio schedules for itself twice a second; the turn
    /// used to read it as "not at rest" and stir, so the quiet stretch never ran out while a shell
    /// held the keyboard. The turn's facts here are the ones a blinking window has: nothing
    /// running, nothing a gesture set going, no resize owed, and a repaint owed on every other
    /// turn. A travelling tween and a resize still owed do stir.
    ///
    /// MUTATION: count the blink as a stir (`|| self.repaint_owed` in `TurnFacts::stirs`) and
    /// neither the environment nor the spare is ever asked for.
    #[test]
    fn a_blinking_caret_is_not_a_stir_and_the_warm_up_and_the_spare_fire_after_the_grace() {
        use super::{SpareDue, TurnFacts};
        use crate::pace::Lanes;
        let still = Lanes::default();
        let start = Instant::now();
        let mut clock = WebWarmup::default();
        let mut door = Recorded::default();
        clock.saw_frame(Some(start));
        let mut asked = None;
        let mut spare = None;
        for step in 0..100_u32 {
            let now = start + ms(100 * u64::from(step));
            let facts = TurnFacts {
                running: still,
                travelling: still,
                // The caret flips every 500 ms: a chrome present owed on those turns.
                repaint_owed: step % 5 == 0,
                resize_owed: false,
            };
            assert!(
                !facts.stirs(),
                "turn {step}: a blink is not the reader doing anything"
            );
            if facts.stirs() {
                clock.stir(now);
            }
            if let Some(answer) = clock.turn(now, false, &mut door, say) {
                asked.get_or_insert((now - start, answer));
            }
            if let Some(due) = clock.spare_turn(now, false, true, false) {
                spare.get_or_insert((now - start, due));
            }
        }
        assert_eq!(asked, Some((WEB_ENGINE_WARMUP_AFTER, Ok(WebWarmUp::Asked))));
        assert_eq!(
            spare,
            Some((
                WEB_ENGINE_WARMUP_AFTER + WEB_ENGINE_WARMUP_QUIET,
                SpareDue::Make
            ))
        );
        let travelling = TurnFacts {
            running: Lanes {
                chrome: true,
                overlay: false,
            },
            travelling: Lanes {
                chrome: true,
                overlay: false,
            },
            repaint_owed: false,
            resize_owed: false,
        };
        assert!(travelling.stirs(), "a tween a gesture set going is a stir");
        let spinning = TurnFacts {
            travelling: still,
            ..travelling
        };
        assert!(
            !spinning.stirs() && !spinning.at_rest(),
            "a status spinner is motion, not a stir"
        );
        assert!(
            TurnFacts {
                resize_owed: true,
                ..spinning
            }
            .stirs()
        );
    }

    /// **A platform with nothing to warm is told once and never again.**
    #[test]
    fn a_platform_with_nothing_to_warm_is_asked_once() {
        struct Nothing(usize);
        impl EngineDoor for Nothing {
            fn warm(&mut self, _: EnvironmentAnswer) -> Result<WebWarmUp, String> {
                self.0 += 1;
                Ok(WebWarmUp::NothingToWarm)
            }
        }
        let start = Instant::now();
        let mut clock = WebWarmup::default();
        let mut door = Nothing(0);
        clock.saw_frame(Some(start));
        for step in 0..100 {
            let _ = clock.turn(start + ms(100 * step), false, &mut door, say);
        }
        assert_eq!(door.0, 1);
    }
}
