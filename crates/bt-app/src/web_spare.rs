//! **The spare web controller** (0.4.5 ticket 60, D-64; `docs/ARCHITECTURE.md` §5.3 row 21;
//! `docs/plans/design/spare-web-controller-2026-09-25.md`).
//!
//! The first web page of a run paid for its controller inside the gesture that asked for it:
//! 2.3 s median on the clean VM from cold, 0.6 s with the environment warmed (ticket 54), because
//! the browser's processes start at the first *controller*, not at the environment. The ruling of
//! 2026-09-25 (option A) is to make one controller ahead of time, on a quiet turn, for a profile
//! that has opened a page before, park it on a never-shown window, and hand it to the first
//! eligible page through the road tear-out already uses (`WebSeat::rehost`): spike 59 measured
//! the first page at 146 ms median that way.
//!
//! This module is the application's side of that: the one owner ([`WebSpare`], held by `App`, on
//! the window thread), its clock at the three places it is turned, the adoption transaction, the
//! run's end, and the receipt that decides who gets a spare at all. The lifecycle itself is
//! [`bt_platform::SpareSlot`]'s, and the seat is an ordinary [`WebSeat`] under
//! [`webhost::RecoveryPolicy::Parked`].
//!
//! **What is generic here, and why.** The owner and the transaction are written over two small
//! traits — [`SpareSeat`] and [`Handoff`] — so the event sequences the design note names can be
//! driven headless through the real `WebMachine`, `recovery_under`, `EnvironmentSlot`,
//! `SpareSlot` and `WebWarmup`, with callbacks delivered on later simulated turns. The product
//! implements them with `WebSeat` and `bt_platform::SpareParent`; the COM calls underneath are the
//! VM smoke's to prove.

use std::time::Instant;

use bt_persist::{
    PreviewSourceV1, RecentPreviewV1, RecentSeedV1, SessionV1, SettingsV1, WebPagesUsedV1,
};
use bt_platform::{SparePhase, SpareSlot};

use crate::webhost::{self, WebOutcome, WebSeat};

/// The one line `diagnostics.log` gets when the run's bound runs out with the spare's browser
/// still holding on.
pub(crate) const OUTLIVED_LINE: &str = "the spare web page's browser outlived the run's bound";

/// The line a retired spare leaves, naming why.
pub(crate) fn retired_line(why: &str) -> String {
    format!("the spare web page was retired: {why}")
}

/// **The spare's size on its never-shown parent**, physical pixels: the parent's own client
/// area, as spike 59's harness had it (SW-5).
pub(crate) const PARKED_BOUNDS: webhost::WebBounds = webhost::WebBounds {
    x: 0,
    y: 0,
    width: 800,
    height: 600,
};

/// **The tab number the spare's page visual is filed under** in its parent's compositor. The
/// parent's tree holds nothing else, so any key would do; this one is no `TabId` the process mints.
pub(crate) const SPARE_TAB: u64 = u64::MAX;

// ── The seat, as the owner sees it ─────────────────────────────────────────

/// **What the spare's owner needs from its seat.** One product implementation, `WebSeat` over
/// `bt_platform::SpareParent`; a test drives the owner through a recorded stand-in.
pub(crate) trait SpareSeat<P> {
    /// Read everything the engine has said and act on it, **then** turn its clocks — so an answer
    /// that has already arrived is never read as silence by a deadline.
    fn advance(&mut self, parent: &P, now: Instant) -> Vec<WebOutcome>;
    /// Close it and start the wait for its browser (`WebSeat::close`).
    fn retire(&mut self, parent: &P) -> Vec<WebOutcome>;
    /// Close the controller and every answered orphan now, and wait for nothing.
    fn close_now(&mut self);
    /// When its own clocks next need turning.
    fn next_deadline(&self) -> Option<Instant>;
    /// Whether its engine has said something nobody has read.
    fn has_events(&self) -> bool;
    /// Whether a creation call nobody will adopt has still not answered.
    fn has_orphans(&self) -> bool;
    /// Whether its own blank page has landed on its current generation.
    fn landed_on_blank(&self) -> bool;
    /// Whether a page may still take it.
    fn fit_for_adoption(&self) -> bool;
    /// The environment epoch its controller was asked for under.
    fn made_under(&self) -> Option<u64>;
}

impl SpareSeat<bt_platform::SpareParent> for WebSeat {
    fn advance(&mut self, parent: &bt_platform::SpareParent, now: Instant) -> Vec<WebOutcome> {
        let compositor = parent.compositor();
        let mut outcomes = self.drive(compositor);
        outcomes.extend(self.tick(now, compositor));
        // The parent's tree is committed by nobody else: no frame is ever drawn for it.
        if let Err(error) = compositor.commit() {
            outcomes.push(WebOutcome::Fault(error));
        }
        outcomes
    }

    fn retire(&mut self, parent: &bt_platform::SpareParent) -> Vec<WebOutcome> {
        let outcomes = self.close(parent.compositor());
        let _ = parent.compositor().commit();
        outcomes
    }

    fn close_now(&mut self) {
        WebSeat::close_now(self);
    }

    fn next_deadline(&self) -> Option<Instant> {
        WebSeat::next_deadline(self)
    }

    fn has_events(&self) -> bool {
        WebSeat::has_events(self)
    }

    fn has_orphans(&self) -> bool {
        WebSeat::has_orphans(self)
    }

    fn landed_on_blank(&self) -> bool {
        WebSeat::landed_on_blank(self)
    }

    fn fit_for_adoption(&self) -> bool {
        WebSeat::fit_for_adoption(self)
    }

    fn made_under(&self) -> Option<u64> {
        WebSeat::made_under(self)
    }
}

// ── The owner ──────────────────────────────────────────────────────────────

/// **The application's one spare** (SW-1, SW-2): the slot, the parents left to process exit, and
/// the clock's own bookkeeping.
pub(crate) struct WebSpare<S = WebSeat, P = bt_platform::SpareParent> {
    slot: SpareSlot<S, P>,
    /// **Parents an orderly stop or an outrun bound left behind**, never destroyed by a thread
    /// that has stopped pumping (§7.35). Held for the life of the process; `main` leaves by
    /// `leave_process`, which runs no destructor.
    abandoned: Vec<P>,
    /// Whether the retiring seat has said `Gone`. It is let go of once, in addition, no creation
    /// call it made is still out.
    gone: bool,
    /// The deadline the last advance left.
    next: Option<Instant>,
}

impl<S, P> Default for WebSpare<S, P> {
    fn default() -> Self {
        Self {
            slot: SpareSlot::default(),
            abandoned: Vec::new(),
            gone: false,
            next: None,
        }
    }
}

impl<S: SpareSeat<P>, P> WebSpare<S, P> {
    /// Where the spare is.
    pub(crate) fn phase(&self) -> SparePhase {
        self.slot.phase()
    }

    /// The slot itself, for a test to look inside.
    #[cfg(test)]
    pub(crate) fn slot_mut(&mut self) -> &mut SpareSlot<S, P> {
        &mut self.slot
    }

    /// **The spare was made.** From the clock's own stage only, which runs once.
    pub(crate) fn created(&mut self, seat: S, parent: P, epoch: u64) {
        if let Err((seat, parent)) = self.slot.created(seat, parent, epoch) {
            // Unreachable from the clock, which makes at most one; let go of rather than kept.
            drop(seat);
            drop(parent);
        }
    }

    /// **One turn of the spare's own clock** (SW-2's `advance`): drain and tick the seat, act on
    /// what it said and on the owner's own causes, and answer when it next needs a turn.
    ///
    /// `drain_creating` is whether this turn may advance a spare that is still being made: only a
    /// turn the warm-up clock calls quiet may (SW-6). `bound` is the run's retirement bound, once
    /// retirement has started. At the bound the spare takes the abandonment road with one
    /// diagnostics line, and the run's end may go on.
    pub(crate) fn advance(
        &mut self,
        now: Instant,
        drain_creating: bool,
        epoch_now: u64,
        bound: Option<Instant>,
        say: &mut dyn FnMut(&str),
    ) -> Option<Instant> {
        if bound.is_some_and(|bound| now >= bound) && !self.slot.holds_nothing() {
            say(OUTLIVED_LINE);
            self.abandon();
            self.next = None;
            return None;
        }
        self.slot.release_old_parent(now, bound);
        let phase = self.slot.phase();
        if phase == SparePhase::Creating && !drain_creating {
            return self.next;
        }
        let mut retire_for: Option<String> = None;
        let mut park_under: Option<u64> = None;
        let mut orphans = false;
        if let Some((seat, parent)) = self.slot.held_mut() {
            for outcome in seat.advance(parent, now) {
                match outcome {
                    WebOutcome::Retired(why) => retire_for = Some(why.to_owned()),
                    WebOutcome::Fault(text) if phase != SparePhase::Retiring => {
                        retire_for = Some(text);
                    }
                    WebOutcome::Gone => self.gone = true,
                    _ => {}
                }
            }
            if matches!(phase, SparePhase::Creating | SparePhase::Parked) && retire_for.is_none() {
                // **Another seat's rebuild let the environment go** (SW-1 item 3).
                if seat.made_under().is_some_and(|made| made != epoch_now) {
                    retire_for = Some(String::from(
                        "another page's rebuild let the web environment go",
                    ));
                } else if phase == SparePhase::Creating && seat.landed_on_blank() {
                    park_under = seat.made_under();
                }
            }
            if let Some(why) = &retire_for {
                say(&retired_line(why));
                // A seat that retired itself is already closing; this is the owner's own cause.
                let _ = seat.retire(parent);
            }
            orphans = seat.has_orphans();
        }
        if retire_for.is_some() {
            self.slot.retire();
        } else if let Some(epoch) = park_under {
            self.slot.parked(epoch);
            crate::web_trace::line(|| String::from("spare parked"));
        }
        if self.slot.phase() == SparePhase::Retiring && self.gone && !orphans {
            self.slot.let_go();
        }
        self.next = self.own_deadline(bound);
        self.next
    }

    /// When the spare next needs a turn, from its seat's clocks, an old parent and the run's
    /// bound.
    fn own_deadline(&mut self, bound: Option<Instant>) -> Option<Instant> {
        let seat = self
            .slot
            .held_mut()
            .and_then(|(seat, _)| seat.next_deadline());
        let live = !self.slot.holds_nothing();
        [seat, self.slot.old_parent_due(), bound.filter(|_| live)]
            .into_iter()
            .flatten()
            .min()
    }

    /// **The instant the event loop has to come back for the spare** — the fold's
    /// `"spare web controller"` owner. While it is being made and its engine has said something,
    /// that is the next quiet instant (`quiet_at`, `None` while the restore card stands);
    /// otherwise its own clocks.
    pub(crate) fn deadline(&mut self, quiet_at: Option<Instant>) -> Option<Instant> {
        if self.slot.phase() == SparePhase::Creating
            && self
                .slot
                .held_mut()
                .is_some_and(|(seat, _)| seat.has_events())
        {
            return quiet_at;
        }
        self.next
    }

    /// **The run is ending, or the owner has a cause of its own: retire the spare** — close its
    /// seat and wait for its browser, from any live phase; from `None`, nothing is ever made.
    /// Idempotent.
    pub(crate) fn retire(&mut self) {
        if matches!(
            self.slot.phase(),
            SparePhase::Creating | SparePhase::Parked | SparePhase::Adopting
        ) && let Some((seat, parent)) = self.slot.held_mut()
        {
            let _ = seat.retire(parent);
        }
        self.slot.retire();
    }

    /// **An orderly stop** (SW-2): the controller and its answered orphans closed at once, no
    /// wait, and every parent left to process exit.
    pub(crate) fn abandon(&mut self) {
        let (seat, parents) = self.slot.abandon();
        if let Some(mut seat) = seat {
            seat.close_now();
        }
        self.abandoned.extend(parents);
        self.gone = false;
    }

    /// **Whether the run may end as far as the spare is concerned**: nothing held that its end
    /// has to wait for.
    pub(crate) fn has_let_go(&self) -> bool {
        self.slot.holds_nothing()
    }

    /// How many parents are waiting for process exit.
    #[cfg(test)]
    pub(crate) fn abandoned(&self) -> usize {
        self.abandoned.len()
    }
}

// ── The run's end (SW-2) ───────────────────────────────────────────────────

/// **Start the run's retirement bound, once** (SW-2): `now` + the teardown deadline, the first
/// time it is asked, and never again — a poll never restarts it.
pub(crate) fn start_retiring(bound: &mut Option<Instant>, now: Instant) -> Instant {
    *bound.get_or_insert(now + crate::quit::PAGE_TEARDOWN_DEADLINE)
}

/// What the event loop does once every window has left.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RunControl {
    /// The run ends now.
    Exit,
    /// Come back at this instant: something is still letting go.
    WaitUntil(Instant),
    /// Nothing is owed; sleep until something wakes the loop.
    Wait,
}

/// **The empty registry's control flow** (SW-2, extracted so it can be driven with no window).
///
/// `run_ends` is `a_run_ends_with_its_last_visible_window` of the registry. The run ends only
/// when it ends **and** the spare has let go; while the spare holds anything the loop waits for
/// the spare's own instant — its browser-exit wait, or the run's bound — and never plainly, which
/// with no window left to wake it would be a run that never ends.
pub(crate) fn after_the_last_window(
    run_ends: bool,
    spare_let_go: bool,
    spare_next: Option<Instant>,
    bound: Option<Instant>,
) -> RunControl {
    if spare_let_go {
        return if run_ends {
            RunControl::Exit
        } else {
            RunControl::Wait
        };
    }
    match spare_next.or(bound) {
        Some(at) => RunControl::WaitUntil(at),
        None => RunControl::Wait,
    }
}

// ── The adoption transaction (SW-3) ────────────────────────────────────────

/// How the platform handoff ended, in the transaction's words.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HandedOff {
    /// The controller moved to the page's window.
    Moved,
    /// Nothing moved; the handoff refused where it stood.
    SourceKept(String),
    /// The handoff could not be undone: the controller was closed and the seat is rebuilding in
    /// the page's window.
    Lost(String),
}

/// **The handoff, as the transaction sees it**: park the seat for it, then walk
/// `WebSeat::rehost`. One product implementation over the page's window; a recorded door in the
/// tests.
pub(crate) trait Handoff<S, P> {
    /// Hide the seat and forget the spare's own rectangle (SW-5).
    fn park(&mut self, seat: &mut S);
    /// Move it from `parent` into the page's window.
    fn rehost(&mut self, seat: &mut S, parent: &P) -> HandedOff;
}

/// What the page ends up with.
#[derive(Debug)]
pub(crate) enum Adoption<S> {
    /// No spare for this page: it builds its own controller, as every page did before.
    BuildYourOwn,
    /// The spare's controller is the page's; navigate it once it has its bounds.
    Moved(S),
    /// The spare's seat is the page's, and is rebuilding its controller in the page's window.
    Lost(S),
}

/// **Adopt the spare, as one transaction** (SW-3).
///
/// `begin_adoption` moves the slot to `Adopting` **before** any platform call, so a second page in
/// the same turn finds nothing; the slot still owns the seat and the parent while the handoff runs.
/// Every return leaves each resource one owner: `Moved` and `Lost` hand the seat to the page (the
/// slot keeps a lost handoff's old parent until `lost_release_at`), and `SourceKept` leaves both
/// with the slot, retiring.
pub(crate) fn adopt<S: SpareSeat<P>, P>(
    spare: &mut WebSpare<S, P>,
    epoch_now: u64,
    door: &mut dyn Handoff<S, P>,
    lost_release_at: Instant,
) -> Adoption<S> {
    if !spare
        .slot
        .begin_adoption(epoch_now, SpareSeat::fit_for_adoption)
    {
        return Adoption::BuildYourOwn;
    }
    let Some((seat, parent)) = spare.slot.adopting_mut() else {
        return Adoption::BuildYourOwn;
    };
    door.park(seat);
    match door.rehost(seat, parent) {
        HandedOff::Moved => spare
            .slot
            .finish_moved()
            .map_or(Adoption::BuildYourOwn, Adoption::Moved),
        HandedOff::SourceKept(_) => {
            // The slot keeps both and retires them; the page builds its own.
            spare.slot.finish_kept_source();
            if let Some((seat, parent)) = spare.slot.held_mut() {
                let _ = seat.retire(parent);
            }
            Adoption::BuildYourOwn
        }
        HandedOff::Lost(_) => spare
            .slot
            .finish_lost(lost_release_at)
            .map_or(Adoption::BuildYourOwn, Adoption::Lost),
    }
}

// ── The receipt (SW-4) ─────────────────────────────────────────────────────

/// **The receipt's one writer** (0.4.5 ticket 60): hand the store a copy that says `Used`, and let
/// `SettingsStore::store` decide whether that is a write — a change, or a retry after a write that
/// did not land. Two callers: a page's `Committed`, and the startup reconciliation.
pub(crate) fn note_a_web_page_committed(store: &mut crate::persist::SettingsStore) {
    let settings = SettingsV1 {
        web_pages_used: WebPagesUsedV1::Used,
        ..store.loaded().clone()
    };
    store.store(settings);
}

/// **Whether a saved session holds a page** — a typed page record in any window, tab, pane, pool
/// entry, Recent seed or Recent preview (SW-4). Never a string search: an `.html` file previewed
/// is a file, not a page.
pub(crate) fn saved_pages_in(session: &SessionV1) -> bool {
    fn seed_holds_a_page(seed: &RecentSeedV1) -> bool {
        match seed {
            RecentSeedV1::Preview { source, .. } => *source == PreviewSourceV1::Url,
            RecentSeedV1::Window { seeds } => seeds.iter().any(seed_holds_a_page),
            RecentSeedV1::Term { .. } | RecentSeedV1::Files { .. } => false,
        }
    }
    let open = session
        .windows
        .iter()
        .flat_map(|window| &window.tabs)
        .filter_map(|tab| tab.preview.as_ref())
        .any(|preview| {
            preview
                .panes
                .iter()
                .any(|pane| pane.cur_source == PreviewSourceV1::Url)
                || preview
                    .pool
                    .iter()
                    .any(|entry| entry.source == PreviewSourceV1::Url)
        });
    open || session.recent.iter().any(|entry| {
        seed_holds_a_page(&entry.seed)
            || entry
                .previews
                .iter()
                .any(|preview| matches!(preview, RecentPreviewV1::Page { .. }))
    })
}

/// **Complete the v39 upgrade from the session already in hand** (SW-4): while the receipt says
/// `Never`, a saved session holding a page writes `Used` through the one writer. A session that
/// was missing, would not read or came from a newer build was loaded as the empty document, so it
/// is no evidence. Never writes `Never`.
pub(crate) fn reconcile_web_pages_used(
    store: &mut crate::persist::SettingsStore,
    session: &SessionV1,
) {
    if store.loaded().web_pages_used == WebPagesUsedV1::Never && saved_pages_in(session) {
        note_a_web_page_committed(store);
    }
}

#[cfg(test)]
mod spare_lifecycle_tests {
    //! The design note's event sequences (Revision (b), tests 1–11), headless. The seat is a
    //! stand-in whose machine is the real [`webhost::WebMachine`], whose policy filter is the real
    //! [`webhost::recovery_under`], and whose environment is a real
    //! [`bt_platform::EnvironmentSlot`]; callbacks are queued and delivered on a later simulated
    //! turn, never inside the call that caused them. The executor records what it would have
    //! asked the engine for — environment creations and forgets, controller requests, closes and
    //! orphan closes — which is the one thing a test cannot do headless.

    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use bt_platform::{EnvironmentAsk, EnvironmentSlot, SparePhase};

    use super::{
        Adoption, HandedOff, Handoff, RunControl, SpareSeat, WebSpare, adopt,
        after_the_last_window, start_retiring,
    };
    use crate::quit::PAGE_TEARDOWN_DEADLINE;
    use crate::web_warmup::{
        SpareDue, WEB_ENGINE_WARMUP_AFTER, WEB_ENGINE_WARMUP_QUIET, WebWarmup,
    };
    use crate::webhost::{
        BROWSER_EXIT_DEADLINE, Recovered, RecoveryPolicy, WebEffect, WebMachine, WebOutcome,
        WebState, recovery_under,
    };

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// What the executor asked the engine for, in order.
    #[derive(Clone, Debug, Eq, PartialEq)]
    enum Asked {
        /// A creation call for the process's environment.
        CreateEnvironment,
        /// `forget_web_environment`.
        Forget,
        /// A controller creation call, by who asked.
        Controller(&'static str),
        /// The install burst.
        Install(&'static str),
        /// A navigation.
        Navigate(&'static str, String),
        /// `ICoreWebView2Controller::Close`.
        Close(&'static str),
        /// A late controller closed as an orphan.
        OrphanClose(&'static str),
    }

    /// One process's engine, as the executor sees it.
    #[derive(Default)]
    struct Engine {
        environment: EnvironmentSlot<u32>,
        asked: Vec<Asked>,
        /// Tickets the environment creation calls carry, in order.
        tickets: Vec<u64>,
    }

    type Shared = Rc<RefCell<Engine>>;

    /// Something a callback will say, and on which turn.
    #[derive(Clone, Debug)]
    enum Said {
        Environment,
        Controller(u64),
        BlankLanded(u64),
        BrowserExited,
        NewVersion,
    }

    /// **A seat, with the real machine and policy filter and a recorded executor.**
    struct Recorded {
        who: &'static str,
        engine: Shared,
        machine: WebMachine,
        policy: RecoveryPolicy,
        /// Callbacks queued for later turns.
        queue: Rc<RefCell<VecDeque<Said>>>,
        controller: bool,
        pending: Option<u64>,
        landed: Option<u64>,
        made_under: Option<u64>,
        waiting: Option<Instant>,
        /// Whether `waiting` is a rebuild's wait for the old browser (`BrowserWait::Rebuild`)
        /// rather than a close's.
        rebuilding: bool,
        now: Instant,
    }

    impl Recorded {
        fn open(who: &'static str, engine: &Shared, url: &str, policy: RecoveryPolicy) -> Self {
            let mut seat = Self {
                who,
                engine: Rc::clone(engine),
                machine: WebMachine::new(),
                policy,
                queue: Rc::default(),
                controller: false,
                pending: None,
                landed: None,
                made_under: None,
                waiting: None,
                rebuilding: false,
                now: Instant::now(),
            };
            let _ = seat.machine.request(url);
            seat.start_environment();
            seat
        }

        /// `WebSeat::start_environment`: the process slot's `ask`, answered on a later turn.
        fn start_environment(&mut self) {
            let queue = Rc::clone(&self.queue);
            let asked = self.engine.borrow_mut().environment.ask(Box::new(move |_| {
                queue.borrow_mut().push_back(Said::Environment)
            }));
            let mut engine = self.engine.borrow_mut();
            match asked {
                EnvironmentAsk::Answer(answer) => {
                    drop(engine);
                    answer(None);
                }
                EnvironmentAsk::Create(ticket) => {
                    engine.asked.push(Asked::CreateEnvironment);
                    engine.tickets.push(ticket);
                }
                EnvironmentAsk::Joined | EnvironmentAsk::AlreadyAsked => {}
            }
        }

        /// `WebSeat::step`, recorded.
        fn step(&mut self, effect: WebEffect) {
            let generation = self.machine.generation();
            let next = match effect {
                WebEffect::Ignore => None,
                WebEffect::CreateController => {
                    self.made_under = Some(self.engine.borrow().environment.epoch());
                    self.engine
                        .borrow_mut()
                        .asked
                        .push(Asked::Controller(self.who));
                    self.pending = Some(generation);
                    None
                }
                WebEffect::InstallEvents => {
                    self.engine
                        .borrow_mut()
                        .asked
                        .push(Asked::Install(self.who));
                    self.controller = true;
                    self.pending = None;
                    Some(self.machine.on_events_installed(generation))
                }
                WebEffect::Navigate(url) => {
                    self.engine
                        .borrow_mut()
                        .asked
                        .push(Asked::Navigate(self.who, url.clone()));
                    if url == crate::webnav::BLANK_PAGE {
                        self.queue
                            .borrow_mut()
                            .push_back(Said::BlankLanded(generation));
                    }
                    None
                }
                WebEffect::Reload => None,
                WebEffect::CloseOrphanController => {
                    self.engine
                        .borrow_mut()
                        .asked
                        .push(Asked::OrphanClose(self.who));
                    None
                }
                WebEffect::RebuildFromScratch => {
                    self.close_controller();
                    self.engine.borrow_mut().environment.forget();
                    self.engine.borrow_mut().asked.push(Asked::Forget);
                    self.start_environment();
                    None
                }
                WebEffect::AwaitBrowserExitBeforeRebuild => {
                    if !self.rebuilding {
                        self.close_controller();
                        self.waiting = Some(self.now + BROWSER_EXIT_DEADLINE);
                        self.rebuilding = true;
                    }
                    None
                }
                WebEffect::RebuildForNewVersion => {
                    self.engine.borrow_mut().environment.forget();
                    self.engine.borrow_mut().asked.push(Asked::Forget);
                    self.start_environment();
                    None
                }
                WebEffect::AwaitBrowserExitBeforeCleanup => {
                    self.close_controller();
                    self.waiting = Some(self.now + BROWSER_EXIT_DEADLINE);
                    None
                }
                WebEffect::ReleaseUserDataFolder => {
                    self.waiting = None;
                    None
                }
            };
            if let Some(next) = next {
                self.step(next);
            }
        }

        fn close_controller(&mut self) {
            if self.controller {
                self.engine.borrow_mut().asked.push(Asked::Close(self.who));
            }
            self.controller = false;
        }

        /// One callback, digested and filtered as `WebSeat::drive` does.
        fn digest(&mut self, said: Said, outcomes: &mut Vec<WebOutcome>) {
            let failed_before = self.machine.state() == WebState::Failed;
            let generation = self.machine.generation();
            let effect = match said {
                Said::Environment => self.machine.on_environment(generation, true),
                Said::Controller(asked_for) => {
                    let effect = self.machine.on_controller(asked_for, true);
                    if effect == WebEffect::CloseOrphanController {
                        // The orphan rule: what a late call delivers is closed.
                        self.engine
                            .borrow_mut()
                            .asked
                            .push(Asked::OrphanClose(self.who));
                        WebEffect::Ignore
                    } else {
                        effect
                    }
                }
                Said::BlankLanded(landed) => {
                    if landed == generation && self.machine.state() == WebState::Ready {
                        self.landed = Some(landed);
                    }
                    WebEffect::Ignore
                }
                // `WebSeat::browser_is_gone`: the obituary of a browser a rebuild asked to go ends
                // that wait.
                Said::BrowserExited if self.rebuilding => {
                    self.rebuilding = false;
                    self.waiting = None;
                    WebEffect::RebuildForNewVersion
                }
                Said::BrowserExited => self.machine.on_browser_process_exited(),
                Said::NewVersion => self.machine.on_new_browser_version_available(),
            };
            let fell = !failed_before && self.machine.state() == WebState::Failed;
            match recovery_under(self.policy, effect, fell) {
                Recovered::Apply(effect) => {
                    if effect == WebEffect::ReleaseUserDataFolder {
                        outcomes.push(WebOutcome::Gone);
                    }
                    self.step(effect);
                }
                Recovered::Retire(why) => {
                    if self.machine.state() != WebState::Closing {
                        let effect = self.machine.close();
                        self.step(effect);
                        outcomes.push(WebOutcome::Retired(why));
                    }
                }
            }
            // A controller request answers on a later turn.
            if let Some(asked_for) = self.pending.take() {
                self.queue
                    .borrow_mut()
                    .push_back(Said::Controller(asked_for));
            }
        }

        /// Deliver one callback now, as the engine would between turns.
        fn hear(&mut self, said: Said) {
            self.queue.borrow_mut().push_back(said);
        }
    }

    impl SpareSeat<&'static str> for Recorded {
        fn advance(&mut self, _parent: &&'static str, now: Instant) -> Vec<WebOutcome> {
            self.now = now;
            let mut outcomes = Vec::new();
            // Only what was queued before this turn: an answer raised now arrives next turn.
            let due: Vec<Said> = self.queue.borrow_mut().drain(..).collect();
            for said in due {
                self.digest(said, &mut outcomes);
            }
            if self.waiting.is_some_and(|at| now >= at) {
                self.waiting = None;
                if std::mem::take(&mut self.rebuilding) {
                    self.step(WebEffect::RebuildForNewVersion);
                } else if self.machine.on_cleanup_deadline() == WebEffect::ReleaseUserDataFolder {
                    outcomes.push(WebOutcome::Gone);
                }
            }
            outcomes
        }

        fn retire(&mut self, _parent: &&'static str) -> Vec<WebOutcome> {
            if self.machine.state() == WebState::Closing {
                return Vec::new();
            }
            let effect = self.machine.close();
            self.step(effect);
            Vec::new()
        }

        fn close_now(&mut self) {
            self.close_controller();
            self.engine
                .borrow_mut()
                .asked
                .push(Asked::Close("close_now"));
        }

        fn next_deadline(&self) -> Option<Instant> {
            self.waiting
        }

        fn has_events(&self) -> bool {
            !self.queue.borrow().is_empty()
        }

        fn has_orphans(&self) -> bool {
            false
        }

        fn landed_on_blank(&self) -> bool {
            self.landed == Some(self.machine.generation())
                && self.machine.state() == WebState::Ready
        }

        fn fit_for_adoption(&self) -> bool {
            self.policy == RecoveryPolicy::Parked
                && self.machine.state() == WebState::Ready
                && self.controller
                && self.landed_on_blank()
        }

        fn made_under(&self) -> Option<u64> {
            self.made_under
        }
    }

    /// A process: its engine, the warm-up clock, and the spare's owner.
    struct Process {
        engine: Shared,
        clock: WebWarmup,
        spare: WebSpare<Recorded, &'static str>,
        start: Instant,
        said: Vec<String>,
    }

    impl Process {
        fn new() -> Self {
            let start = Instant::now();
            let mut clock = WebWarmup::default();
            clock.saw_frame(Some(start));
            Self {
                engine: Shared::default(),
                clock,
                spare: WebSpare::default(),
                start,
                said: Vec::new(),
            }
        }

        fn epoch(&self) -> u64 {
            self.engine.borrow().environment.epoch()
        }

        /// The environment's creation call answers (on a later turn than it was made).
        fn environment_answers(&mut self) {
            let ticket = *self
                .engine
                .borrow()
                .tickets
                .last()
                .expect("a call was made");
            let (waiting, error) = self
                .engine
                .borrow_mut()
                .environment
                .arrived(ticket, Ok(ticket as u32));
            for answer in waiting {
                answer(error.clone());
            }
        }

        /// The turn of the window that runs the application's clocks, as `Runtime::turn` and
        /// `Runtime::warm_web_engine` run it.
        fn turn(&mut self, now: Instant, pages_used: bool, page_open: bool) {
            struct Door<'a>(&'a Shared);
            impl crate::web_warmup::EngineDoor for Door<'_> {
                fn warm(
                    &mut self,
                    answered: bt_platform::EnvironmentAnswer,
                ) -> Result<bt_platform::WebWarmUp, String> {
                    let asked = self.0.borrow_mut().environment.warm(answered);
                    match asked {
                        EnvironmentAsk::AlreadyAsked => Ok(bt_platform::WebWarmUp::AlreadyAsked),
                        EnvironmentAsk::Create(ticket) => {
                            let mut engine = self.0.borrow_mut();
                            engine.asked.push(Asked::CreateEnvironment);
                            engine.tickets.push(ticket);
                            Ok(bt_platform::WebWarmUp::Asked)
                        }
                        EnvironmentAsk::Answer(answer) => {
                            answer(None);
                            Ok(bt_platform::WebWarmUp::Asked)
                        }
                        EnvironmentAsk::Joined => Ok(bt_platform::WebWarmUp::Asked),
                    }
                }
            }
            let _ = self.clock.turn(now, false, &mut Door(&self.engine), |_| {});
            if self.clock.spare_turn(now, false, pages_used, page_open) == Some(SpareDue::Make) {
                let seat = Recorded::open(
                    "spare",
                    &self.engine,
                    crate::webnav::BLANK_PAGE,
                    RecoveryPolicy::Parked,
                );
                let epoch = self.epoch();
                self.spare.created(seat, "parent", epoch);
            }
            let quiet = self.clock.is_quiet(now, false);
            let epoch = self.epoch();
            let said = &mut self.said;
            let _ = self.spare.advance(now, quiet, epoch, None, &mut |line| {
                said.push(line.to_owned())
            });
        }

        /// A `WebPageSpoke`: the spare is advanced unless it is still being made (SW-6).
        fn spoke(&mut self, now: Instant) {
            let epoch = self.epoch();
            let said = &mut self.said;
            let _ = self.spare.advance(now, false, epoch, None, &mut |line| {
                said.push(line.to_owned())
            });
        }

        fn asked(&self) -> Vec<Asked> {
            self.engine.borrow().asked.clone()
        }

        fn controllers(&self, who: &str) -> usize {
            self.asked()
                .iter()
                .filter(|asked| matches!(asked, Asked::Controller(by) if *by == who))
                .count()
        }

        /// Turns every 100 ms from the first frame until `until`, with a stir at each of `stirs`.
        fn run(&mut self, until: Duration, stirs: &[Duration], pages_used: bool) {
            let mut at = Duration::ZERO;
            while at <= until {
                let now = self.start + at;
                if stirs.contains(&at) {
                    self.clock.stir(now);
                }
                if at == WEB_ENGINE_WARMUP_AFTER + ms(100) {
                    self.environment_answers();
                }
                self.turn(now, pages_used, false);
                at += ms(100);
            }
        }
    }

    /// RED (60) — **a profile whose history has no page never asks for a controller.**
    ///
    /// The receipt is the whole of who pays the spare's 91 MB: a fresh profile, a profile upgraded
    /// without pages, one whose navigations were blank or failed, and one that imported a document
    /// saying `Used` from another machine (an import never moves the receipt — see
    /// `settings_bundle`'s `the_web_pages_receipt_is_no_row_and_no_import_moves_it`). The
    /// environment is still warmed (ticket 54), and nothing more.
    ///
    /// MUTATION: drop the receipt condition from `WebWarmup::spare_turn` and a spare is made.
    #[test]
    fn a_profile_whose_history_has_no_page_never_asks_for_a_controller() {
        let mut process = Process::new();
        process.run(ms(20_000), &[], false);
        assert_eq!(
            process.asked(),
            vec![Asked::CreateEnvironment],
            "the environment is warmed and nothing else is asked for"
        );
        assert_eq!(process.spare.phase(), SparePhase::None);
        // A blank page and a failed one never move the page's identity, which is what the
        // receipt's writer is driven by (`WebOutcome::Committed`).
        let mut machine = WebMachine::new();
        let _ = machine.request("https://example.test/");
        let _ = machine.on_environment(1, true);
        let _ = machine.on_controller(1, true);
        let _ = machine.on_events_installed(1);
        let _ = machine.on_navigation_completed(1, crate::webnav::BLANK_PAGE, true);
        let _ = machine.on_navigation_completed(1, "https://example.test/", false);
        assert_eq!(
            machine.recoverable_url(),
            None,
            "no commit, so no receipt write"
        );
    }

    /// RED (60) — **the spare is made on a quiet turn after the environment's, and its controller
    /// call waits for a quiet turn even when the environment answers late.**
    ///
    /// The environment's answer is delayed past a stir (typing). The spare's seat drains it only on
    /// the next turn the clock calls quiet, so the controller call — up to 590 ms on the window
    /// thread, measured — is never made inside the burst, and it is made once.
    ///
    /// MUTATION: advance a `Creating` spare on the spoke (`drain_creating` true in `spoke`) and the
    /// controller call lands on the busy turn.
    #[test]
    fn the_spare_waits_for_a_quiet_turn_even_when_the_environment_answers_late() {
        let mut process = Process::new();
        let grace = WEB_ENGINE_WARMUP_AFTER;
        // The environment's turn at the grace; the spare's turn a quiet second later.
        let mut at = Duration::ZERO;
        while at <= grace + WEB_ENGINE_WARMUP_QUIET {
            process.turn(process.start + at, true, false);
            at += ms(100);
        }
        assert_eq!(
            process.spare.phase(),
            SparePhase::Creating,
            "made once, on a quiet turn"
        );
        assert_eq!(
            process.controllers("spare"),
            0,
            "the environment has not answered"
        );
        // Typing starts; the environment answers in the middle of it and wakes the loop.
        let typing = process.start + at;
        process.clock.stir(typing);
        process.environment_answers();
        process.spoke(typing + ms(10));
        for step in 1..=5 {
            let now = typing + ms(100 * step);
            process.clock.stir(now);
            process.spoke(now);
            process.turn(now, true, false);
        }
        assert_eq!(
            process.controllers("spare"),
            0,
            "no controller call while the reader is typing"
        );
        let quiet = typing + ms(500) + WEB_ENGINE_WARMUP_QUIET;
        process.turn(quiet, true, false);
        assert_eq!(
            process.controllers("spare"),
            1,
            "on the next quiet turn, once"
        );
        for step in 1..=30 {
            process.turn(quiet + ms(100 * step), true, false);
        }
        assert_eq!(process.controllers("spare"), 1);
        assert_eq!(process.spare.phase(), SparePhase::Parked);
        assert_eq!(
            process
                .asked()
                .iter()
                .filter(|asked| **asked == Asked::CreateEnvironment)
                .count(),
            1,
            "one environment for the warm-up and the spare"
        );
    }

    /// RED (60) — **a page opened while the spare is being made neither waits for it nor takes
    /// it**; one environment serves both, the page makes its own controller call, and the spare
    /// later parks for the next eligible page.
    ///
    /// MUTATION: let `begin_adoption` answer from `Creating` and the page takes a spare that has
    /// no controller yet.
    #[test]
    fn a_page_opened_while_the_spare_is_being_made_neither_waits_nor_takes_it() {
        let mut process = Process::new();
        let spare_turn = WEB_ENGINE_WARMUP_AFTER + WEB_ENGINE_WARMUP_QUIET;
        let mut at = Duration::ZERO;
        while at <= spare_turn {
            process.turn(process.start + at, true, false);
            at += ms(100);
        }
        assert_eq!(process.spare.phase(), SparePhase::Creating);
        let epoch = process.epoch();
        struct NeverCalled;
        impl Handoff<Recorded, &'static str> for NeverCalled {
            fn park(&mut self, _: &mut Recorded) {
                panic!("nothing to park: the spare is not parked");
            }
            fn rehost(&mut self, _: &mut Recorded, _: &&'static str) -> HandedOff {
                panic!("nothing to hand over");
            }
        }
        let now = process.start + at;
        let adoption = adopt(&mut process.spare, epoch, &mut NeverCalled, now);
        assert!(matches!(adoption, Adoption::BuildYourOwn));
        assert_eq!(process.spare.phase(), SparePhase::Creating, "it carries on");
        let mut page = Recorded::open(
            "page",
            &process.engine,
            "https://a.test/",
            RecoveryPolicy::Page,
        );
        process.environment_answers();
        let mut outcomes = Vec::new();
        let queued: Vec<_> = page.queue.borrow_mut().drain(..).collect();
        for said in queued {
            page.digest(said, &mut outcomes);
        }
        for step in 0..40 {
            process.turn(now + WEB_ENGINE_WARMUP_QUIET + ms(100 * step), true, true);
        }
        assert_eq!(process.controllers("page"), 1, "the page made its own");
        assert_eq!(process.controllers("spare"), 1);
        assert_eq!(
            process
                .asked()
                .iter()
                .filter(|asked| **asked == Asked::CreateEnvironment)
                .count(),
            1,
            "one environment creation for both"
        );
        assert_eq!(
            process.spare.phase(),
            SparePhase::Parked,
            "parked for the next page"
        );
    }

    /// A process whose spare is parked.
    fn parked() -> Process {
        let mut process = Process::new();
        process.run(ms(12_000), &[], true);
        assert_eq!(
            process.spare.phase(),
            SparePhase::Parked,
            "{:?}",
            process.asked()
        );
        process
    }

    /// RED (60) — **a page and the spare hearing one browser exit rebuild once** — the page's
    /// rebuild, one forget and one new environment creation — and the spare's own effects hold no
    /// forget and no ask: it retires. The same holds for a new browser version, and in either
    /// delivery order.
    ///
    /// MUTATION: make `recovery_under` the identity and the spare forgets the environment and asks
    /// for another.
    #[test]
    fn a_page_and_the_spare_hearing_one_browser_exit_rebuild_once() {
        for (event, spare_first) in [(0, true), (0, false), (1, true), (1, false)] {
            let mut process = parked();
            let mut page = Recorded::open(
                "page",
                &process.engine,
                "https://a.test/",
                RecoveryPolicy::Page,
            );
            let now = process.start + ms(12_100);
            // The page is up on the same environment.
            let mut outcomes = Vec::new();
            for _ in 0..4 {
                let queued: Vec<_> = page.queue.borrow_mut().drain(..).collect();
                for said in queued {
                    page.digest(said, &mut outcomes);
                }
            }
            assert_eq!(page.machine.state(), WebState::Ready);
            let before = process.asked().len();
            // A new version reaches each seat as two events: the notice, and then the obituary
            // of the old browser every seat on it was asked to let go of (ticket 68).
            let said = || {
                if event == 0 {
                    vec![Said::BrowserExited]
                } else {
                    vec![Said::NewVersion, Said::BrowserExited]
                }
            };
            let deliver_to_page = |page: &mut Recorded| {
                for said in said() {
                    page.hear(said);
                }
                let queued: Vec<_> = page.queue.borrow_mut().drain(..).collect();
                let mut outcomes = Vec::new();
                for said in queued {
                    page.digest(said, &mut outcomes);
                }
            };
            if !spare_first {
                deliver_to_page(&mut page);
            }
            if let Some((seat, _)) = process.spare.slot_mut().held_mut() {
                for said in said() {
                    seat.hear(said);
                }
            }
            process.spoke(now);
            if spare_first {
                deliver_to_page(&mut page);
            }
            let after: Vec<Asked> = process.asked()[before..].to_vec();
            assert_eq!(
                after
                    .iter()
                    .filter(|asked| **asked == Asked::Forget)
                    .count(),
                1,
                "one forget, the page's: {after:?}"
            );
            assert_eq!(
                after
                    .iter()
                    .filter(|asked| **asked == Asked::CreateEnvironment)
                    .count(),
                1,
                "one new environment, the page's: {after:?}"
            );
            assert!(
                after.contains(&Asked::Close("spare")),
                "the spare closed its controller: {after:?}"
            );
            // A new version's second event is the obituary of the browser the spare retired from,
            // which ends its wait; a lone browser exit is what started the retirement.
            let retired = if event == 0 {
                SparePhase::Retiring
            } else {
                SparePhase::Retired
            };
            assert_eq!(process.spare.phase(), retired);
        }
    }

    /// RED (60) — **another seat forgetting the environment retires a parked spare**, and a page
    /// that asks afterwards is told to build its own.
    ///
    /// MUTATION: drop the epoch comparison from `WebSpare::advance` (and the slot's) and the spare
    /// stays parked over an environment the process has let go of.
    #[test]
    fn another_seat_forgetting_the_environment_retires_a_parked_spare() {
        let mut process = parked();
        process.engine.borrow_mut().environment.forget();
        let now = process.start + ms(12_100);
        process.spoke(now);
        assert_eq!(process.spare.phase(), SparePhase::Retiring);
        assert!(
            process
                .said
                .iter()
                .any(|line| line.contains("rebuild let the web environment go"))
        );
        struct NeverCalled;
        impl Handoff<Recorded, &'static str> for NeverCalled {
            fn park(&mut self, _: &mut Recorded) {
                panic!("a retired spare is not handed over");
            }
            fn rehost(&mut self, _: &mut Recorded, _: &&'static str) -> HandedOff {
                panic!("a retired spare is not handed over");
            }
        }
        let epoch = process.epoch();
        assert!(matches!(
            adopt(&mut process.spare, epoch, &mut NeverCalled, now),
            Adoption::BuildYourOwn
        ));
    }

    /// The handoff door, recorded: what it answers, and what it was asked.
    struct Door {
        answer: HandedOff,
        parked: usize,
        handed: usize,
    }

    impl Handoff<Recorded, &'static str> for Door {
        fn park(&mut self, _seat: &mut Recorded) {
            self.parked += 1;
        }
        fn rehost(&mut self, seat: &mut Recorded, _parent: &&'static str) -> HandedOff {
            self.handed += 1;
            if matches!(self.answer, HandedOff::Lost(_)) {
                // `WebSeat::rehost`'s lost branch: the controller was closed.
                seat.close_controller();
            }
            self.answer.clone()
        }
    }

    /// RED (60) — **every handoff result leaves each resource one owner.** `Moved`: the seat is the
    /// page's and the parent is gone, with no new controller request. `SourceKept`: the slot keeps
    /// the seat and the parent, retiring them, and the page builds its own — exactly one new
    /// request. `Lost`: the seat is the page's, rebuilding, and the slot keeps the old parent until
    /// its own wait ends. A setter failing after `Moved` finds the seat already the page's.
    ///
    /// MUTATION: take the seat out of the slot in `begin_adoption` (before the handoff) and a
    /// `SourceKept` leaves it with no owner: the retiring slot holds nothing.
    #[test]
    fn every_handoff_result_leaves_each_resource_one_owner() {
        for answer in [
            HandedOff::Moved,
            HandedOff::SourceKept(String::from("put_ParentWindow")),
            HandedOff::Lost(String::from("SetRootVisualTarget; compensation failed")),
        ] {
            let mut process = parked();
            let now = process.start + ms(12_100);
            let before = process.controllers("page");
            let mut door = Door {
                answer: answer.clone(),
                parked: 0,
                handed: 0,
            };
            let epoch = process.epoch();
            let release = now + BROWSER_EXIT_DEADLINE;
            let adoption = adopt(&mut process.spare, epoch, &mut door, release);
            assert_eq!(
                (door.parked, door.handed),
                (1, 1),
                "parked, then handed over"
            );
            match (&answer, adoption) {
                (HandedOff::Moved, Adoption::Moved(seat)) => {
                    assert!(seat.controller, "the page holds the live controller");
                    assert_eq!(process.spare.phase(), SparePhase::Adopted);
                    assert!(process.spare.has_let_go(), "and the parent is gone");
                    assert_eq!(process.controllers("page"), before, "no new request");
                    // A setter that fails now is a fault on a seat that is already the page's:
                    // nothing here can take it back out of the page's hands.
                    drop(seat);
                }
                (HandedOff::SourceKept(_), Adoption::BuildYourOwn) => {
                    assert_eq!(process.spare.phase(), SparePhase::Retiring);
                    assert!(
                        process.spare.slot_mut().held_mut().is_some(),
                        "the slot still owns the seat and the parent"
                    );
                    assert!(process.asked().contains(&Asked::Close("spare")));
                    // The page's own road: one controller request.
                    let mut page = Recorded::open(
                        "page",
                        &process.engine,
                        "https://a.test/",
                        RecoveryPolicy::Page,
                    );
                    let mut outcomes = Vec::new();
                    let queued: Vec<_> = page.queue.borrow_mut().drain(..).collect();
                    for said in queued {
                        page.digest(said, &mut outcomes);
                    }
                    assert_eq!(process.controllers("page"), before + 1);
                }
                (HandedOff::Lost(_), Adoption::Lost(seat)) => {
                    assert!(!seat.controller, "the handed controller was closed");
                    assert_eq!(process.spare.phase(), SparePhase::Adopted);
                    assert!(!process.spare.has_let_go(), "the old parent waits");
                    let epoch = process.epoch();
                    let _ = process
                        .spare
                        .advance(release, false, epoch, None, &mut |_| {});
                    assert!(
                        process.spare.has_let_go(),
                        "at its own time, not the page's life"
                    );
                }
                (answer, _) => panic!("{answer:?} ended some other way"),
            }
        }
    }

    /// RED (60) — **two pages in one turn share one spare**: the first moves the slot to
    /// `Adopting` before any platform call, so the second finds it not parked and builds its own.
    ///
    /// MUTATION: move to `Adopting` only after the handoff (answer `Parked` from `begin_adoption`
    /// until then) and the second page is handed the same seat.
    #[test]
    fn two_pages_in_one_turn_share_one_spare() {
        let mut process = parked();
        let now = process.start + ms(12_100);
        let epoch = process.epoch();
        let mut door = Door {
            answer: HandedOff::Moved,
            parked: 0,
            handed: 0,
        };
        let first = adopt(&mut process.spare, epoch, &mut door, now);
        assert!(matches!(first, Adoption::Moved(_)));
        let again = process.spare.slot_mut().begin_adoption(epoch, |_| true);
        assert!(!again, "the second page finds nothing to take");
        assert_eq!(process.spare.phase(), SparePhase::Adopted);
        // And while the first is inside its handoff, the slot already says so.
        let mut process = parked();
        assert!(process.spare.slot_mut().begin_adoption(epoch, |_| true));
        assert!(!process.spare.slot_mut().begin_adoption(epoch, |_| true));
        assert_eq!(process.spare.phase(), SparePhase::Adopting);
    }

    /// RED (60) — **the run ends after the spare lets go, with no window left to wake it.**
    ///
    /// The last window closes with a parked spare and no page: the spare is retired in the same
    /// branch, the run's bound is started once, the registry empties, and the loop is driven
    /// through [`after_the_last_window`] with **no exit notification** — `WebHost::close` took the
    /// handlers off — on an advancing fake clock. It ends at the spare's browser-exit deadline
    /// from retirement, and never later than the bound; polling does not restart the bound.
    ///
    /// MUTATION: answer `RunControl::Wait` while the spare holds on (the empty arm's old
    /// `ControlFlow::Wait`) and the loop sleeps with nothing to wake it; or restart the bound on
    /// every poll and the exit moves.
    #[test]
    fn the_run_ends_after_the_spare_lets_go_with_no_window_left_to_wake_it() {
        let mut process = parked();
        // The last window closes on the turn the spare was last advanced on.
        let closed_at = process.start + ms(12_000);
        let mut bound = None;
        let first = start_retiring(&mut bound, closed_at);
        process.spare.retire();
        assert_eq!(process.spare.phase(), SparePhase::Retiring);
        let mut now = closed_at;
        let mut turns = 0;
        loop {
            turns += 1;
            assert!(turns < 100, "the loop never ended");
            assert_eq!(
                start_retiring(&mut bound, now),
                first,
                "a poll never restarts the bound"
            );
            let epoch = process.epoch();
            let next = process.spare.advance(now, false, epoch, bound, &mut |_| {});
            match after_the_last_window(true, process.spare.has_let_go(), next, bound) {
                RunControl::Exit => break,
                RunControl::WaitUntil(at) => {
                    assert!(at > now, "a wake in the past is a spin");
                    now = at;
                }
                RunControl::Wait => panic!("the loop waits with nothing left to wake it"),
            }
        }
        assert_eq!(
            now,
            closed_at + BROWSER_EXIT_DEADLINE,
            "at the browser-exit deadline"
        );
        assert!(
            now <= closed_at + PAGE_TEARDOWN_DEADLINE,
            "and inside the run's bound"
        );
        assert!(
            process.asked().contains(&Asked::Close("spare")),
            "the controller was closed"
        );
        // A spare whose browser outlives even the bound is abandoned at the bound, said once.
        let mut process = parked();
        let mut bound = None;
        let _ = start_retiring(&mut bound, closed_at);
        process.spare.retire();
        if let Some((seat, _)) = process.spare.slot_mut().held_mut() {
            seat.waiting = Some(closed_at + PAGE_TEARDOWN_DEADLINE * 2);
        }
        let epoch = process.epoch();
        let at_bound = closed_at + PAGE_TEARDOWN_DEADLINE;
        let _ = process
            .spare
            .advance(at_bound, false, epoch, bound, &mut |line| {
                assert_eq!(line, super::OUTLIVED_LINE);
            });
        assert!(process.spare.has_let_go());
        assert_eq!(
            process.spare.abandoned(),
            1,
            "its parent is left to process exit"
        );
    }

    /// RED (60) — **an orderly stop abandons the spare at once**: the controller closed with no
    /// wait, and the parent held for process exit.
    ///
    /// MUTATION: route `abandon` through `retire` and the stop waits for the browser: the spare
    /// is still `Retiring` and holding its parent.
    #[test]
    fn an_orderly_stop_abandons_the_spare_at_once() {
        let mut process = parked();
        process.spare.abandon();
        assert!(process.spare.has_let_go(), "nothing left to wait for");
        assert_eq!(
            process.spare.abandoned(),
            1,
            "the parent is left to process exit"
        );
        assert!(process.asked().contains(&Asked::Close("close_now")));
        assert_eq!(
            after_the_last_window(true, process.spare.has_let_go(), None, None),
            RunControl::Exit
        );
    }

    /// **An adopted page navigates only on the target's bounds, and only once** (SW-5): the
    /// machine's `adopt` without bounds waits, a request meanwhile is last-write-wins, and the
    /// first bounds release one `Navigate`; bounds after that release nothing.
    #[test]
    fn an_adopted_page_navigates_only_on_the_targets_bounds_and_only_once() {
        let mut machine = WebMachine::new();
        let _ = machine.request(crate::webnav::BLANK_PAGE);
        let _ = machine.on_environment(1, true);
        let _ = machine.on_controller(1, true);
        assert_eq!(
            machine.on_events_installed(1),
            WebEffect::Navigate(crate::webnav::BLANK_PAGE.to_owned())
        );
        // Stale spare bounds were cleared with the address, so the page arrives unsized.
        assert_eq!(machine.adopt("https://a.test/", false), WebEffect::Ignore);
        assert_eq!(
            machine.request("https://b.test/"),
            WebEffect::Ignore,
            "a hidden or background target with no layout yet still waits"
        );
        assert_eq!(
            machine.release_on_bounds(),
            WebEffect::Navigate("https://b.test/".to_owned()),
            "the first target bounds release the last address asked for"
        );
        assert_eq!(machine.release_on_bounds(), WebEffect::Ignore, "once");
        assert_eq!(
            machine.request("https://c.test/"),
            WebEffect::Navigate("https://c.test/".to_owned()),
            "and from then on the page is an ordinary page"
        );
        // An adopted controller never asks for one of its own.
        let mut sized = WebMachine::new();
        let _ = sized.request(crate::webnav::BLANK_PAGE);
        let _ = sized.on_environment(1, true);
        let _ = sized.on_controller(1, true);
        let _ = sized.on_events_installed(1);
        assert_eq!(
            sized.adopt("https://a.test/", true),
            WebEffect::Navigate("https://a.test/".to_owned())
        );
        assert_eq!(
            sized.state(),
            WebState::Ready,
            "no CreateController, no InstallEvents"
        );
    }
}

#[cfg(test)]
mod receipt_tests {
    //! The receipt that decides who gets a spare, through the real store and the real session
    //! reader.

    use bt_persist::{
        PreviewPaneV1, PreviewPoolEntryV1, PreviewSourceV1, RecentEntryV1, RecentPreviewV1,
        RecentSeedV1, SessionV1, WebPagesUsedV1,
    };

    use super::{note_a_web_page_committed, reconcile_web_pages_used, saved_pages_in};
    use crate::persist::SettingsStore;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("folio-web-receipt-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        dir
    }

    fn on_disk(path: &std::path::Path) -> WebPagesUsedV1 {
        bt_persist::read_settings(path).0.web_pages_used
    }

    /// RED (60) — **the receipt is written by a real commit, once, and retried after a write that
    /// did not land.**
    ///
    /// A real `SettingsStore` over a temp folder. The first write fails (the file's place is
    /// taken by a folder); the receipt is `Used` in memory, and the next commit — the same value —
    /// is still written, because the store's own retry decides and not a "nothing to do" check in
    /// front of it. After that, commits write nothing.
    ///
    /// MUTATION: write only when the loaded receipt is `Never` (the design note's first form) and
    /// the retry never reaches the disk.
    #[test]
    fn the_receipt_is_written_by_a_real_commit_and_retried_after_a_failed_write() {
        let dir = scratch("retry");
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&path).expect("a folder where the file would go");
        let mut store = SettingsStore::at(path.clone());
        note_a_web_page_committed(&mut store);
        assert_eq!(store.loaded().web_pages_used, WebPagesUsedV1::Used);
        assert!(!path.is_file(), "the first write did not land");
        std::fs::remove_dir_all(&path).expect("the folder goes");
        note_a_web_page_committed(&mut store);
        assert_eq!(on_disk(&path), WebPagesUsedV1::Used, "the retry landed");
        let written = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        std::thread::sleep(std::time::Duration::from_millis(20));
        note_a_web_page_committed(&mut store);
        assert_eq!(
            std::fs::metadata(&path).and_then(|m| m.modified()).ok(),
            written,
            "and a later commit writes nothing"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn page_pane() -> PreviewPaneV1 {
        PreviewPaneV1 {
            leaf: String::from("1"),
            cur: Some(String::from("https://a.test/")),
            cur_source: PreviewSourceV1::Url,
            graph: None,
        }
    }

    fn session_with(
        windows: Vec<bt_persist::SessionWindowV1>,
        recent: Vec<RecentEntryV1>,
    ) -> SessionV1 {
        SessionV1 {
            windows,
            recent,
            ..SessionV1::default()
        }
    }

    fn window_with(tab: bt_persist::TabV1) -> bt_persist::SessionWindowV1 {
        bt_persist::SessionWindowV1 {
            tabs: vec![tab],
            ..bt_persist::SessionWindowV1::default()
        }
    }

    fn tab_with(panes: Vec<PreviewPaneV1>, pool: Vec<PreviewPoolEntryV1>) -> bt_persist::TabV1 {
        bt_persist::TabV1 {
            root: bt_persist::LayoutNodeV1::Leaf(bt_persist::LeafNodeV1::Unknown),
            pinned: false,
            focused_leaf: String::from("1"),
            preview: Some(bt_persist::TabPreviewV1 { panes, pool }),
        }
    }

    fn recent(seed: RecentSeedV1, previews: Vec<RecentPreviewV1>) -> RecentEntryV1 {
        RecentEntryV1 {
            key: String::from("k"),
            seed,
            timestamp: String::from("2026-09-25T00:00:00Z"),
            previews,
        }
    }

    /// RED (60) — **the upgrade reads typed saved pages and nothing else**: each of the four page
    /// records is evidence and writes `Used`; an `.html` file previewed, an empty (missing, corrupt
    /// or newer) session and a plain terminal history are not, and leave `Never`.
    ///
    /// MUTATION: drop any one of the four record kinds from `saved_pages_in` and its case stays
    /// `Never`; search strings for `http` and the `.html` file becomes a page.
    #[test]
    fn the_upgrade_reads_typed_saved_pages_and_nothing_else() {
        let html_file = PreviewPaneV1 {
            cur: Some(String::from(r"C:\docs\report.html")),
            cur_source: PreviewSourceV1::File,
            ..page_pane()
        };
        let evidence = [
            session_with(
                vec![window_with(tab_with(vec![page_pane()], vec![]))],
                vec![],
            ),
            session_with(
                vec![window_with(tab_with(
                    vec![],
                    vec![PreviewPoolEntryV1 {
                        path: String::from("https://a.test/"),
                        name: String::from("a"),
                        source: PreviewSourceV1::Url,
                    }],
                ))],
                vec![],
            ),
            session_with(
                vec![],
                vec![recent(
                    RecentSeedV1::Window {
                        seeds: vec![RecentSeedV1::Preview {
                            path: String::from("https://a.test/"),
                            source: PreviewSourceV1::Url,
                        }],
                    },
                    vec![],
                )],
            ),
            session_with(
                vec![],
                vec![recent(
                    RecentSeedV1::Files {
                        root: String::from(r"C:\x"),
                    },
                    vec![RecentPreviewV1::Page {
                        url: String::from("https://a.test/"),
                    }],
                )],
            ),
        ];
        for (index, session) in evidence.iter().enumerate() {
            assert!(saved_pages_in(session), "record kind {index} is a page");
        }
        let no_evidence = [
            SessionV1::default(),
            session_with(vec![window_with(tab_with(vec![html_file], vec![]))], vec![]),
            session_with(
                vec![],
                vec![recent(
                    RecentSeedV1::Preview {
                        path: String::from(r"C:\docs\https-guide.html"),
                        source: PreviewSourceV1::File,
                    },
                    vec![RecentPreviewV1::File(String::from(r"C:\docs\https.html"))],
                )],
            ),
        ];
        for (index, session) in no_evidence.iter().enumerate() {
            assert!(!saved_pages_in(session), "case {index} holds no page");
        }
        // Through the real store: evidence writes `Used`, and nothing ever writes `Never`.
        let dir = scratch("upgrade");
        let path = dir.join("settings.json");
        let mut store = SettingsStore::at(path.clone());
        reconcile_web_pages_used(&mut store, &no_evidence[1]);
        assert_eq!(store.loaded().web_pages_used, WebPagesUsedV1::Never);
        assert!(!path.exists(), "no evidence, no write");
        reconcile_web_pages_used(&mut store, &evidence[3]);
        assert_eq!(on_disk(&path), WebPagesUsedV1::Used);
        reconcile_web_pages_used(&mut store, &SessionV1::default());
        assert_eq!(on_disk(&path), WebPagesUsedV1::Used, "never written back");
        // A corrupt session and one from a newer build are read as the empty document.
        let corrupt = dir.join("corrupt.json");
        std::fs::write(&corrupt, b"{ not json").expect("a corrupt session");
        assert!(!saved_pages_in(&bt_persist::read_session(&corrupt).0));
        let newer = dir.join("newer.json");
        let mut future = serde_json::to_value(&evidence[0]).expect("serialises");
        future["schema_version"] = serde_json::json!(bt_persist::SESSION_SCHEMA_VERSION + 1);
        std::fs::write(&newer, future.to_string()).expect("a newer session");
        assert!(!saved_pages_in(&bt_persist::read_session(&newer).0));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod spare_wiring_tests {
    //! Wiring pins beside the behaviour tests above (Revision (b), test 13): where the spare is
    //! taken, the one bookkeeping tail, the one window product code creates. Read through
    //! `bt_source`, by item and not by file.

    use bt_source::{Index, ItemQuery, Pattern, Search, View, needle};

    fn source() -> &'static Index {
        Index::of_package("bt-app")
    }

    fn method_body(owner: &str, name: &str) -> &'static str {
        source()
            .body_of(&ItemQuery::method(owner, name))
            .unwrap_or_else(|failure| panic!("{failure}"))
    }

    /// PIN (60) — **the spare is taken only where a pane has no engine**, and the seat enters the
    /// window through the one bookkeeping tail on both roads.
    #[test]
    fn the_spare_is_taken_only_in_the_arm_with_no_engine_and_seated_by_the_one_tail() {
        let door = method_body("Runtime", "open_web_page_on");
        let navigated = door
            .find("self.window.web.contains_key(&leaf)")
            .expect("the door forks on this pane's engine");
        let adopted = door
            .find("self.adopt_spare_web_page(")
            .expect("the door asks for the spare");
        let built = door
            .find("webhost::WebSeat::open(")
            .expect("and builds its own otherwise");
        assert!(
            navigated < adopted && adopted < built,
            "the spare is asked for after the navigated arm and before a controller is built:\n{door}"
        );
        let adoption = method_body("Runtime", "adopt_spare_web_page");
        assert_eq!(
            adoption.matches("self.seat_a_web_page(").count(),
            2,
            "both adopted roads seat the page through the tail:\n{adoption}"
        );
        let inserts = source()
            .search(&Search::new(
                needle!(Pattern::text("self.window.web.insert(leaf")),
                View::CodeKeepingLiterals,
            ))
            .unwrap_or_else(|failure| panic!("{failure}"))
            .in_the_product(source());
        let owners: Vec<String> = inserts
            .owners(source())
            .into_keys()
            .map(|identity| identity.name)
            .collect();
        assert_eq!(owners, vec![String::from("seat_a_web_page")]);
    }

    /// PIN (60) — **the spare parent is the one window product code creates outside winit**
    /// (`docs/ARCHITECTURE.md` §6).
    #[test]
    fn the_spare_parent_is_the_one_window_product_code_creates() {
        let platform = Index::of_package("bt-platform");
        let created = platform
            .search(&Search::new(
                needle!(Pattern::text("CreateWindowExW(")),
                View::CodeKeepingLiterals,
            ))
            .unwrap_or_else(|failure| panic!("{failure}"))
            .in_the_product(platform);
        let owners: Vec<String> = created
            .owners(platform)
            .into_keys()
            .map(|identity| identity.name)
            .collect();
        assert_eq!(
            owners,
            vec![String::from("spare_parent")],
            "{}",
            created.report(platform)
        );
    }

    /// PIN (60) — **the run's end asks the spare**: the quit's wait and the empty registry.
    #[test]
    fn the_runs_end_asks_the_spare() {
        assert!(method_body("FolioApp", "every_page_has_gone").contains("has_let_go()"));
        let reap = method_body("FolioApp", "reap_leaving_windows");
        assert!(reap.contains("self.run_end(waking)"));
        assert!(
            reap.contains("if spare_holds && !done.is_empty() && done.len() == self.windows.len()"),
            "the last closed window waits for the spare, as it waits for its own pages"
        );
        assert!(method_body("FolioApp", "run_end").contains("after_the_last_window("));
        assert!(method_body("FolioApp", "fail").contains("web_spare.abandon()"));
    }
}
