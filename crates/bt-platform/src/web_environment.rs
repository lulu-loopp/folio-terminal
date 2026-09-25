//! **The process's one web environment, and where it is in its life** (0.4.5
//! ticket 54, D-64).
//!
//! WebView2 has one environment per process (`plan.md` §0): two over one user
//! data folder with different options is `0x8007139F`, and two with the same
//! options is two browser process trees for no reason. Until ticket 54 the
//! only thing that asked for it was a page, so "is there one?" was a cache that
//! was either filled or not, and two pages opened before the first answer came
//! back made two creation calls.
//!
//! Ticket 54 gives the environment a second trigger source — the turn's warm-up
//! clock in `bt-app`, which asks for it on an idle turn after startup so the
//! first page does not pay for the runtime's start — and with two sources the
//! lifecycle has to be a fact with one owner rather than a cache two roads
//! fill. This is that owner's state, written once and generic over what the
//! environment is, so the rule is the same on every platform and testable on
//! all of them: the Windows arm keeps one of these for
//! `ICoreWebView2Environment` on the window thread, and a platform with no
//! process-wide environment keeps none.
//!
//! **The rule.** There is at most one creation call in flight. Whoever asks
//! while it is in flight waits for it, exactly as the one who made it does. It
//! answers every one of them, success or failure, once. A failure leaves the
//! state empty, so the next ask makes a new call; a success leaves the
//! environment for every later ask to be answered with on the spot.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

/// **Where the process's web environment is** — the three states a reader can
/// act on.
///
/// There is no `Failed`: a failure is an answer, delivered to everybody who was
/// waiting, and what it leaves behind is [`Self::None`] — the next page asks
/// again, as it did before the warm-up existed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebEnvironmentPhase {
    /// Nobody has asked, or the last ask failed, or it was forgotten.
    None,
    /// A creation call is in flight and has not answered.
    Requested,
    /// The environment exists.
    Ready,
}

/// **What a warm-up ask did** — the answer [`crate::warm_web_environment`]
/// gives its one caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebWarmUp {
    /// A creation call was made; its answer comes back through the closure the
    /// caller handed over.
    Asked,
    /// Somebody else already asked (a page, or an earlier warm-up) or the
    /// environment is already here. Nothing was done and the closure was
    /// dropped unanswered, because there is nothing for it to say.
    AlreadyAsked,
    /// This platform has no process-wide environment to warm (macOS makes a
    /// `WKWebView` on the spot; a Linux build has no engine).
    NothingToWarm,
}

/// **One waiter's answer**: `None` when the environment came up, `Some(error)`
/// when it did not. Called once, on the thread that owns the slot.
pub type EnvironmentAnswer = Box<dyn FnOnce(Option<String>)>;

/// What [`EnvironmentSlot::ask`] and [`EnvironmentSlot::warm`] tell their
/// caller to do next — outside the slot's borrow, because the creation call
/// can answer before it returns, and the answer writes the slot.
pub enum EnvironmentAsk {
    /// The environment is here: call the answer now, with `None`.
    Answer(EnvironmentAnswer),
    /// A creation call is already in flight and the answer has joined it.
    Joined,
    /// Make the creation call now. Its completion is [`EnvironmentSlot::arrived`]
    /// with this ticket; a call that fails where it stands is
    /// [`EnvironmentSlot::refused`] with it.
    Create(u64),
    /// [`EnvironmentSlot::warm`] only: somebody has already asked, or the
    /// environment is already here. The answer was dropped.
    AlreadyAsked,
}

enum State<E> {
    None,
    /// The ticket the creation call carries, and everybody waiting on it.
    Requested(u64, Vec<EnvironmentAnswer>),
    Ready(E),
}

/// **The one owner of the environment's lifecycle.**
///
/// `E` is the environment itself (a COM interface on Windows); the slot only
/// ever clones it out to a caller that asked.
pub struct EnvironmentSlot<E> {
    state: State<E>,
    /// The next creation call's ticket. A completion carrying any other ticket
    /// is for a call this slot has forgotten and is not adopted.
    next_ticket: u64,
    /// Waiters of a request that was forgotten while in flight, handed to the
    /// next creation call rather than dropped: they asked for an environment
    /// and the next one is the one they get.
    carried: Vec<EnvironmentAnswer>,
    /// **Which environment this is** (0.4.5 ticket 60): bumped by every [`Self::forget`] and by
    /// every environment that arrives, so a holder that recorded it can tell, later, whether the
    /// environment it was made under is still the one the process has.
    ///
    /// Read for the spare web controller, which is parked beside the pages and has no page's
    /// recovery road of its own: another seat's rebuild forgets the environment, and a spare made
    /// under the old one can no longer be handed to a page (`holds_the_process_environment`).
    epoch: u64,
}

impl<E: Clone> Default for EnvironmentSlot<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Clone> EnvironmentSlot<E> {
    /// An empty slot: nobody has asked.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: State::None,
            next_ticket: 1,
            carried: Vec::new(),
            epoch: 0,
        }
    }

    /// **Which environment this is** — see the field. Moves on every forget and on every arrival.
    #[must_use]
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Where the environment is.
    #[must_use]
    pub fn phase(&self) -> WebEnvironmentPhase {
        match self.state {
            State::None => WebEnvironmentPhase::None,
            State::Requested(..) => WebEnvironmentPhase::Requested,
            State::Ready(_) => WebEnvironmentPhase::Ready,
        }
    }

    /// The environment, when it is here.
    #[must_use]
    pub fn environment(&self) -> Option<E> {
        match &self.state {
            State::Ready(environment) => Some(environment.clone()),
            State::None | State::Requested(..) => None,
        }
    }

    /// **A page's ask**: answered on the spot when the environment is here,
    /// joined to the call in flight when there is one, and a new call
    /// otherwise.
    pub fn ask(&mut self, answer: EnvironmentAnswer) -> EnvironmentAsk {
        if let State::Requested(_, waiting) = &mut self.state {
            waiting.push(answer);
            return EnvironmentAsk::Joined;
        }
        if let State::Ready(_) = self.state {
            return EnvironmentAsk::Answer(answer);
        }
        let ticket = self.next_ticket;
        self.next_ticket += 1;
        let mut waiting = std::mem::take(&mut self.carried);
        waiting.push(answer);
        self.state = State::Requested(ticket, waiting);
        EnvironmentAsk::Create(ticket)
    }

    /// **The warm-up's ask**: a new call only when nobody has asked, and
    /// nothing at all otherwise — never a second request, and never a join,
    /// because a warm-up that joined a page's request would be a warm-up that
    /// did nothing but add a second reader of its answer.
    pub fn warm(&mut self, answer: EnvironmentAnswer) -> EnvironmentAsk {
        match self.state {
            State::None => self.ask(answer),
            State::Requested(..) | State::Ready(_) => EnvironmentAsk::AlreadyAsked,
        }
    }

    /// **The creation call answered.** Leaves the environment on success and
    /// the empty state on failure, and hands back everybody who was waiting, to
    /// be answered outside the borrow with the error (or `None`).
    ///
    /// A completion whose ticket is not the one in flight is for a request that
    /// was forgotten: it answers nobody and is not adopted.
    #[must_use]
    pub fn arrived(
        &mut self,
        ticket: u64,
        result: Result<E, String>,
    ) -> (Vec<EnvironmentAnswer>, Option<String>) {
        let waiting = match std::mem::replace(&mut self.state, State::None) {
            State::Requested(current, waiting) if current == ticket => waiting,
            other => {
                self.state = other;
                return (Vec::new(), None);
            }
        };
        match result {
            Ok(environment) => {
                self.state = State::Ready(environment);
                self.epoch += 1;
                (waiting, None)
            }
            Err(error) => (waiting, Some(error)),
        }
    }

    /// **The creation call failed where it stood**, before any callback.
    ///
    /// Answers whether the failure is the caller's to report: `true` when the
    /// request was still in flight — the state goes back to empty and its
    /// waiters, who can only be the caller itself (nothing else runs between
    /// the ask and the call returning), are dropped unanswered because the
    /// caller reports the failure through its return value. `false` when a
    /// completion already answered during the call, in which case every waiter
    /// has heard and there is nothing left to say.
    #[must_use]
    pub fn refused(&mut self, ticket: u64) -> bool {
        if matches!(self.state, State::Requested(current, _) if current == ticket) {
            self.state = State::None;
            true
        } else {
            false
        }
    }

    /// **Let the environment go** — the step a runtime update or a dead browser
    /// forces (see `forget_web_environment`).
    ///
    /// A ready environment is dropped. A request in flight is forgotten too,
    /// because a creation call made while an old browser still held the folder
    /// never calls back (`w0p-evidence.md` §3.4) and a slot that waited on it
    /// would never ask again; its waiters are carried to the next call.
    pub fn forget(&mut self) {
        self.epoch += 1;
        match std::mem::replace(&mut self.state, State::None) {
            State::Requested(_, waiting) => self.carried.extend(waiting),
            State::None | State::Ready(_) => {}
        }
    }
}

// ── The controller a creation call delivers, and the ones nobody came for ──

/// **Where one controller creation call is in its life** (R2-13; 0.4.5 ticket 60).
enum Delivery<C> {
    /// The call is out and has not answered.
    Waiting,
    /// It answered with a controller nobody has taken yet.
    Delivered(C),
    /// It answered, and what it delivered was taken or closed, or there was nothing.
    Spent,
}

/// **The completion's half of one controller creation call**: the callback delivers into it,
/// once, whether the call succeeded (`Some`) or not (`None`).
pub struct ControllerSink<C>(Rc<RefCell<Delivery<C>>>);

impl<C> ControllerSink<C> {
    /// The creation call answered.
    pub fn deliver(&self, controller: Option<C>) {
        *self.0.borrow_mut() = match controller {
            Some(controller) => Delivery::Delivered(controller),
            None => Delivery::Spent,
        };
    }
}

/// One controller creation call, by the generation it was made for.
struct Pending<C> {
    generation: u64,
    delivery: Rc<RefCell<Delivery<C>>>,
}

impl<C> Pending<C> {
    fn answered(&self) -> bool {
        !matches!(*self.delivery.borrow(), Delivery::Waiting)
    }

    /// What the call delivered, if it has answered with a controller nobody has taken.
    fn take(&self) -> Option<C> {
        let mut delivery = self.delivery.borrow_mut();
        match std::mem::replace(&mut *delivery, Delivery::Spent) {
            Delivery::Delivered(controller) => Some(controller),
            Delivery::Waiting => {
                *delivery = Delivery::Waiting;
                None
            }
            Delivery::Spent => None,
        }
    }
}

/// **A host's controller creation calls: the one it may adopt, and the ones nobody will come for**
/// (R2-13; the orphan fix of 0.4.5 ticket 60).
///
/// WebView2's creation callbacks cannot be cancelled. A host that stopped wanting the controller
/// of a call still in flight used to drop its slot; the callback kept a clone, delivered its
/// controller into it and was released, and the controller went with it — **dropped without
/// `Close()`**, a browser process tree with nobody pointing at it. The seat's own
/// `CloseOrphanController` then found an empty slot and closed nothing. So a slot nobody will
/// adopt is kept as an *orphan* until it answers, and what it delivers is closed.
///
/// Generic over the controller so the rule is the same on every platform and testable on all of
/// them; the Windows arm keeps one of these for `ICoreWebView2CompositionController`.
pub struct ControllerSlots<C> {
    pending: Option<Pending<C>>,
    orphans: Vec<Pending<C>>,
}

impl<C> Default for ControllerSlots<C> {
    fn default() -> Self {
        Self {
            pending: None,
            orphans: Vec::new(),
        }
    }
}

impl<C> ControllerSlots<C> {
    /// **A creation call is being made for `generation`.** A slot already standing belongs to an
    /// attempt this one supersedes and is orphaned first. Answers the sink the callback delivers
    /// into.
    pub fn open(&mut self, generation: u64, close: impl FnMut(C)) -> ControllerSink<C> {
        self.close_pending(close);
        let delivery = Rc::new(RefCell::new(Delivery::Waiting));
        self.pending = Some(Pending {
            generation,
            delivery: Rc::clone(&delivery),
        });
        ControllerSink(delivery)
    }

    /// The generation the pending slot was opened for, if there is one.
    #[must_use]
    pub fn pending_generation(&self) -> Option<u64> {
        self.pending.as_ref().map(|pending| pending.generation)
    }

    /// **Take the controller the callback delivered for `generation`.** A slot opened for another
    /// generation is not adopted: it is orphaned (and closed, if it has answered), and the answer
    /// is the refusal.
    pub fn take(&mut self, generation: u64, close: impl FnMut(C)) -> Result<C, String> {
        let Some(pending) = self.pending.take() else {
            return Err(String::from("no controller callback has been answered"));
        };
        if pending.generation != generation {
            let opened_for = pending.generation;
            self.pending = Some(pending);
            self.close_pending(close);
            return Err(format!(
                "the controller that answered was asked for by generation {opened_for}, not {generation}"
            ));
        }
        pending
            .take()
            .ok_or_else(|| String::from("the controller callback delivered no controller"))
    }

    /// **Nobody will come for the pending slot.** It becomes an orphan, and every orphan that has
    /// answered is let go of — what it delivered is handed to `close`, never dropped.
    pub fn close_pending(&mut self, mut close: impl FnMut(C)) {
        if let Some(pending) = self.pending.take() {
            self.orphans.push(pending);
        }
        self.orphans.retain(|orphan| {
            if !orphan.answered() {
                return true;
            }
            if let Some(controller) = orphan.take() {
                close(controller);
            }
            false
        });
    }

    /// Whether a call nobody will adopt has still not answered. A spare on its way out waits for
    /// these, so that what they deliver is closed rather than dropped.
    #[must_use]
    pub fn has_orphans(&self) -> bool {
        !self.orphans.is_empty()
    }
}

// ── The spare web controller's owner ───────────────────────────────────────

/// **Where the spare web controller is in its life** (0.4.5 ticket 60; the design note's
/// restated lifecycle table).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SparePhase {
    /// None made (and, once the clock's stage has passed, none ever will be).
    None,
    /// Made, and not yet parked: the engine is coming up or the blank page has not landed.
    Creating,
    /// The blank page has landed: the first eligible page may take it.
    Parked,
    /// A page is taking it, inside one call.
    Adopting,
    /// A page took it. Terminal, but for an old parent a lost handoff left to be released.
    Adopted,
    /// Closing, and waiting for its browser to let go.
    Retiring,
    /// Nothing is held. Terminal.
    Retired,
}

enum Spare<S, P> {
    None,
    Creating {
        seat: S,
        parent: P,
        epoch: u64,
    },
    Parked {
        seat: S,
        parent: P,
        epoch: u64,
    },
    Adopting {
        seat: S,
        parent: P,
        epoch: u64,
    },
    /// `old_parent` is the parent a lost handoff left behind, and when it may be let go of.
    Adopted {
        old_parent: Option<(P, Instant)>,
    },
    Retiring {
        seat: S,
        parent: P,
    },
    Retired,
}

/// **The one owner of the spare web controller** — its seat `S` and its never-shown parent `P`
/// (0.4.5 ticket 60).
///
/// A portable generic, so the lifecycle is one rule on every platform and testable on all of
/// them; `bt-app` keeps one for `WebSeat` and `SpareParent` on the window thread. **Every resource
/// has exactly one owner at every return**: the slot holds the seat and the parent until
/// [`Self::finish_moved`] or [`Self::finish_lost`] hands the seat to a page, and whatever a
/// handoff could not move stays here and is retired from here.
///
/// No path leads from `Adopted` or `Retired` back to `Creating`: [`Self::created`] answers only
/// from `None`, and the spare is never replenished.
pub struct SpareSlot<S, P> {
    spare: Spare<S, P>,
}

impl<S, P> Default for SpareSlot<S, P> {
    fn default() -> Self {
        Self { spare: Spare::None }
    }
}

impl<S, P> SpareSlot<S, P> {
    /// Where the spare is.
    #[must_use]
    pub fn phase(&self) -> SparePhase {
        match self.spare {
            Spare::None => SparePhase::None,
            Spare::Creating { .. } => SparePhase::Creating,
            Spare::Parked { .. } => SparePhase::Parked,
            Spare::Adopting { .. } => SparePhase::Adopting,
            Spare::Adopted { .. } => SparePhase::Adopted,
            Spare::Retiring { .. } => SparePhase::Retiring,
            Spare::Retired => SparePhase::Retired,
        }
    }

    /// **A spare was made**, while the environment was `epoch`. Only from `None`; from anywhere else the
    /// pair is handed back for the caller to let go of, because a second spare is never made.
    pub fn created(&mut self, seat: S, parent: P, epoch: u64) -> Result<(), (S, P)> {
        if !matches!(self.spare, Spare::None) {
            return Err((seat, parent));
        }
        self.spare = Spare::Creating {
            seat,
            parent,
            epoch,
        };
        Ok(())
    }

    /// The environment the spare was made under, while a page could still take it.
    #[must_use]
    pub fn epoch(&self) -> Option<u64> {
        match self.spare {
            Spare::Creating { epoch, .. }
            | Spare::Parked { epoch, .. }
            | Spare::Adopting { epoch, .. } => Some(epoch),
            _ => None,
        }
    }

    /// The seat and its parent, while the slot holds both.
    pub fn held_mut(&mut self) -> Option<(&mut S, &P)> {
        match &mut self.spare {
            Spare::Creating { seat, parent, .. }
            | Spare::Parked { seat, parent, .. }
            | Spare::Adopting { seat, parent, .. }
            | Spare::Retiring { seat, parent } => Some((seat, parent)),
            Spare::None | Spare::Adopted { .. } | Spare::Retired => None,
        }
    }

    /// **The blank page has landed**: `Creating` → `Parked`, under the environment `epoch` its
    /// controller was made under. Nothing from any other phase.
    pub fn parked(&mut self, epoch: u64) {
        if !matches!(self.spare, Spare::Creating { .. }) {
            return;
        }
        if let Spare::Creating { seat, parent, .. } =
            std::mem::replace(&mut self.spare, Spare::Retired)
        {
            self.spare = Spare::Parked {
                seat,
                parent,
                epoch,
            };
        }
    }

    /// **Retire the spare** — from any live phase to `Retiring`, keeping the seat and the parent
    /// until the seat has let go. From `None` it is `Retired` at once: there is nothing to wait
    /// for, and nothing will be made afterwards. Idempotent: `Retiring`, `Adopted` and `Retired`
    /// are left as they are.
    pub fn retire(&mut self) {
        self.spare = match std::mem::replace(&mut self.spare, Spare::Retired) {
            Spare::None => Spare::Retired,
            Spare::Creating { seat, parent, .. }
            | Spare::Parked { seat, parent, .. }
            | Spare::Adopting { seat, parent, .. } => Spare::Retiring { seat, parent },
            other => other,
        };
    }

    /// **A page asks for the spare.** `true`, and `Adopting`, only when the spare is `Parked`, its
    /// environment is still `epoch_now` and `fit` says the seat is still whole. A parked spare that
    /// is no longer fit is retired, and the answer is `false`. A spare still `Creating` answers
    /// `false` and carries on, for the next eligible page. **Nothing is taken away**: while
    /// `Adopting` the slot still owns the seat and the parent, so a second page in the same turn
    /// finds it not `Parked`.
    pub fn begin_adoption(&mut self, epoch_now: u64, fit: impl FnOnce(&S) -> bool) -> bool {
        let Spare::Parked { seat, epoch, .. } = &self.spare else {
            return false;
        };
        if *epoch != epoch_now || !fit(seat) {
            self.retire();
            return false;
        }
        if let Spare::Parked {
            seat,
            parent,
            epoch,
        } = std::mem::replace(&mut self.spare, Spare::Retired)
        {
            self.spare = Spare::Adopting {
                seat,
                parent,
                epoch,
            };
        }
        true
    }

    /// The seat and parent a page is taking, while `Adopting`.
    pub fn adopting_mut(&mut self) -> Option<(&mut S, &P)> {
        match &mut self.spare {
            Spare::Adopting { seat, parent, .. } => Some((seat, parent)),
            _ => None,
        }
    }

    /// **The handoff moved the controller.** The seat is the page's now and is handed back; the
    /// parent holds no controller and is let go of here.
    pub fn finish_moved(&mut self) -> Option<S> {
        match std::mem::replace(&mut self.spare, Spare::Retired) {
            Spare::Adopting { seat, parent, .. } => {
                self.spare = Spare::Adopted { old_parent: None };
                drop(parent);
                Some(seat)
            }
            other => {
                self.spare = other;
                None
            }
        }
    }

    /// **The handoff refused where it stood.** Nothing moved: the slot keeps the seat and the
    /// parent and retires them.
    pub fn finish_kept_source(&mut self) {
        if matches!(self.spare, Spare::Adopting { .. }) {
            self.retire();
        }
    }

    /// **The handoff could not be undone**: the controller was closed and the seat is rebuilding
    /// in the page's window. The seat is handed back; the old parent stays here until
    /// `release_at` — its closed controller's browser-exit wait — and never for the page's life.
    pub fn finish_lost(&mut self, release_at: Instant) -> Option<S> {
        match std::mem::replace(&mut self.spare, Spare::Retired) {
            Spare::Adopting { seat, parent, .. } => {
                self.spare = Spare::Adopted {
                    old_parent: Some((parent, release_at)),
                };
                Some(seat)
            }
            other => {
                self.spare = other;
                None
            }
        }
    }

    /// **The retiring seat has let go** (its browser has gone, or its wait ran out): the seat is
    /// dropped, then the parent. `Retired`.
    pub fn let_go(&mut self) {
        if !matches!(self.spare, Spare::Retiring { .. }) {
            return;
        }
        if let Spare::Retiring { seat, parent } = std::mem::replace(&mut self.spare, Spare::Retired)
        {
            drop(seat);
            drop(parent);
        }
    }

    /// Let go of an old parent whose time has come, or whose run's `bound` has.
    pub fn release_old_parent(&mut self, now: Instant, bound: Option<Instant>) {
        if let Spare::Adopted {
            old_parent: Some((_, at)),
        } = &self.spare
            && (now >= *at || bound.is_some_and(|bound| now >= bound))
        {
            self.spare = Spare::Adopted { old_parent: None };
        }
    }

    /// When an old parent is due to be let go of.
    #[must_use]
    pub fn old_parent_due(&self) -> Option<Instant> {
        match &self.spare {
            Spare::Adopted {
                old_parent: Some((_, at)),
            } => Some(*at),
            _ => None,
        }
    }

    /// **Whether the slot holds nothing a run's end has to wait for** — no seat, and no old
    /// parent.
    #[must_use]
    pub fn holds_nothing(&self) -> bool {
        matches!(
            self.spare,
            Spare::None | Spare::Retired | Spare::Adopted { old_parent: None }
        )
    }

    /// **An orderly stop, or a bound that ran out: give everything up at once.** Answers the seat
    /// (for its controller and orphans to be closed now, with no wait) and every parent (to be
    /// left to process exit — never destroyed by a thread that has stopped pumping). The slot
    /// holds nothing afterwards.
    pub fn abandon(&mut self) -> (Option<S>, Vec<P>) {
        match std::mem::replace(&mut self.spare, Spare::Retired) {
            Spare::Creating { seat, parent, .. }
            | Spare::Parked { seat, parent, .. }
            | Spare::Adopting { seat, parent, .. }
            | Spare::Retiring { seat, parent } => (Some(seat), vec![parent]),
            Spare::Adopted { old_parent } => {
                self.spare = Spare::Adopted { old_parent: None };
                (
                    None,
                    old_parent.map(|(parent, _)| parent).into_iter().collect(),
                )
            }
            Spare::None | Spare::Retired => (None, Vec::new()),
        }
    }
}

#[cfg(test)]
mod spare_ownership_tests {
    //! The spare's owner and the orphan rule, driven headless with stand-ins for the controller,
    //! the seat and the parent. What a stand-in cannot be is the COM call; the VM smoke is that.

    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use super::{ControllerSlots, EnvironmentSlot, SparePhase, SpareSlot};

    /// RED (60) — **a controller that arrives for a slot nobody will adopt is closed, not
    /// dropped.**
    ///
    /// A spare retired while its controller is pending walks `WebHost::close`, whose
    /// `PendingController` row used to drop the slot. The creation callback cannot be cancelled:
    /// it delivered into its own clone of the slot, and the controller was released with the
    /// callback — a browser process tree with no `Close()`. The seat's late `Controller` event then
    /// answered `CloseOrphanController`, which found nothing to close. A spare retired while
    /// `Creating` makes this common; pages reach it too.
    ///
    /// MUTATION: make `close_pending` drop the pending slot instead of keeping it as an orphan
    /// (the old `close_pending_controller`) and the late controller is never closed.
    #[test]
    fn a_spare_retired_while_its_controller_is_pending_closes_the_late_controller() {
        let closed: Rc<RefCell<Vec<u32>>> = Rc::default();
        let close = |controller| closed.borrow_mut().push(controller);
        let mut slots = ControllerSlots::<u32>::default();
        let sink = slots.open(4, close);
        // Retired during `ControllerPending`: the host closes, which orphans the slot.
        slots.close_pending(close);
        assert!(slots.has_orphans(), "the call has not answered yet");
        assert!(closed.borrow().is_empty());
        // The completion arrives on a later turn, into the orphan's sink.
        sink.deliver(Some(40));
        // The seat's late `Controller` event is `CloseOrphanController`: the host closes again.
        slots.close_pending(close);
        assert_eq!(*closed.borrow(), vec![40], "one orphan closed");
        assert!(!slots.has_orphans(), "and nothing is left to wait for");
        assert!(
            slots.take(4, close).is_err(),
            "nothing is installed from an orphan"
        );
        // A call that answers with an error leaves nothing to close and nothing to wait for.
        let failed = slots.open(5, close);
        slots.close_pending(close);
        failed.deliver(None);
        slots.close_pending(close);
        assert!(!slots.has_orphans());
        assert_eq!(*closed.borrow(), vec![40]);
    }

    /// **A slot opened for one generation is not adopted by another, and is closed when it
    /// answers** (R2-13, now through the orphan rule).
    #[test]
    fn a_superseded_slot_is_closed_when_it_answers_and_the_current_one_is_taken() {
        let closed: Rc<RefCell<Vec<u32>>> = Rc::default();
        let close = |controller| closed.borrow_mut().push(controller);
        let mut slots = ControllerSlots::<u32>::default();
        let old = slots.open(1, close);
        let new = slots.open(2, close);
        assert_eq!(slots.pending_generation(), Some(2));
        old.deliver(Some(10));
        new.deliver(Some(20));
        assert_eq!(slots.take(2, close), Ok(20));
        slots.close_pending(close);
        assert_eq!(*closed.borrow(), vec![10]);
        let refused = slots.open(3, close);
        refused.deliver(Some(30));
        assert!(slots.take(4, close).is_err());
        assert_eq!(*closed.borrow(), vec![10, 30]);
    }

    /// RED (60) — **the environment's epoch moves on every forget and every arrival**, and on
    /// nothing else.
    ///
    /// The spare records the epoch it was made under; another seat's rebuild forgets the
    /// environment, and the spare must see that before a page is handed a controller over a
    /// browser the process has let go of.
    ///
    /// MUTATION: drop the bump from `forget` and a rebuild that forgot and re-created nothing yet
    /// leaves the epoch where it was.
    #[test]
    fn the_environment_epoch_moves_on_every_forget_and_every_arrival() {
        let mut slot = EnvironmentSlot::<u32>::new();
        assert_eq!(slot.epoch(), 0);
        let super::EnvironmentAsk::Create(ticket) = slot.ask(Box::new(|_| {})) else {
            panic!("the first ask makes the call");
        };
        assert_eq!(slot.epoch(), 0, "asking is not an environment");
        let _ = slot.arrived(ticket, Ok(1));
        let made = slot.epoch();
        assert_eq!(made, 1);
        assert!(matches!(
            slot.ask(Box::new(|_| {})),
            super::EnvironmentAsk::Answer(_)
        ));
        assert_eq!(
            slot.epoch(),
            made,
            "a page answered on the spot changes nothing"
        );
        slot.forget();
        assert_ne!(slot.epoch(), made, "another seat's rebuild forgot it");
    }

    /// **The spare's phases, and one owner at every step.**
    #[test]
    fn the_spare_is_owned_by_the_slot_until_a_page_takes_it_and_never_comes_back() {
        let mut slot = SpareSlot::<&str, &str>::default();
        assert_eq!(slot.phase(), SparePhase::None);
        assert!(slot.created("seat", "parent", 3).is_ok());
        assert!(
            slot.created("second", "parent", 3).is_err(),
            "at most one spare"
        );
        assert!(!slot.begin_adoption(3, |_| true), "not while Creating");
        assert_eq!(slot.phase(), SparePhase::Creating, "and it carries on");
        slot.parked(3);
        assert_eq!(slot.phase(), SparePhase::Parked);
        assert!(slot.begin_adoption(3, |_| true));
        assert_eq!(slot.phase(), SparePhase::Adopting);
        assert!(slot.held_mut().is_some(), "still the slot's while Adopting");
        assert_eq!(slot.finish_moved(), Some("seat"));
        assert_eq!(slot.phase(), SparePhase::Adopted);
        assert!(slot.holds_nothing());
        slot.retire();
        assert_eq!(
            slot.phase(),
            SparePhase::Adopted,
            "retiring an adopted spare is nothing"
        );
        assert!(
            slot.created("again", "parent", 3).is_err(),
            "never replenished"
        );
    }

    /// **A lost handoff leaves the old parent here, for its own wait and no longer.**
    #[test]
    fn a_lost_handoff_keeps_the_old_parent_until_its_own_time() {
        let now = Instant::now();
        let mut slot = SpareSlot::<&str, &str>::default();
        let _ = slot.created("seat", "parent", 1);
        slot.parked(1);
        assert!(slot.begin_adoption(1, |_| true));
        let release = now + Duration::from_secs(10);
        assert_eq!(slot.finish_lost(release), Some("seat"));
        assert!(!slot.holds_nothing());
        assert_eq!(slot.old_parent_due(), Some(release));
        slot.release_old_parent(now, None);
        assert!(!slot.holds_nothing());
        slot.release_old_parent(now + Duration::from_secs(1), Some(now));
        assert!(slot.holds_nothing(), "the run's bound comes first");
    }
}

#[cfg(test)]
mod environment_slot_tests {
    //! The lifecycle rule, run through the slot the Windows arm keeps, with a
    //! stand-in for the one thing a test cannot do headless: create the engine.

    use std::cell::RefCell;
    use std::rc::Rc;

    use super::{EnvironmentAsk, EnvironmentSlot, WebEnvironmentPhase};

    type Heard = Rc<RefCell<Vec<(&'static str, Option<String>)>>>;

    fn listener(heard: &Heard, who: &'static str) -> super::EnvironmentAnswer {
        let heard = Rc::clone(heard);
        Box::new(move |error| heard.borrow_mut().push((who, error)))
    }

    fn answer_all(waiting: Vec<super::EnvironmentAnswer>, error: Option<String>) {
        for answer in waiting {
            answer(error.clone());
        }
    }

    /// RED (54) — **a page asked for while the warm-up's request is in flight
    /// waits for that request, and one creation call answers both.**
    ///
    /// Before ticket 54 every page that found no environment cached made its
    /// own `CreateCoreWebView2EnvironmentWithOptions`; with a warm-up in the
    /// process the first page would otherwise race it with a second call.
    ///
    /// MUTATION: make `ask` answer `Create` from `Requested` too (a second
    /// call per asker) and the creation count is two.
    #[test]
    fn a_page_that_asks_while_the_warm_up_is_in_flight_joins_it() {
        let heard: Heard = Rc::default();
        let mut slot = EnvironmentSlot::<u32>::new();
        let mut creations = Vec::new();
        match slot.warm(listener(&heard, "warm-up")) {
            EnvironmentAsk::Create(ticket) => creations.push(ticket),
            _ => panic!("the first ask makes the call"),
        }
        assert_eq!(slot.phase(), WebEnvironmentPhase::Requested);
        match slot.ask(listener(&heard, "page")) {
            EnvironmentAsk::Create(ticket) => creations.push(ticket),
            EnvironmentAsk::Joined => {}
            _ => panic!("nothing is here yet"),
        }
        assert_eq!(creations.len(), 1, "one creation call, not one per asker");
        let (waiting, error) = slot.arrived(creations[0], Ok(7));
        answer_all(waiting, error);
        assert_eq!(
            *heard.borrow(),
            vec![("warm-up", None), ("page", None)],
            "both heard the one answer"
        );
        assert_eq!(slot.environment(), Some(7));
        assert!(
            matches!(
                slot.ask(listener(&heard, "later")),
                EnvironmentAsk::Answer(_)
            ),
            "a later page is answered on the spot"
        );
    }

    /// RED (54) — **the warm-up asks only when nobody has.** A page's request
    /// in flight, or an environment already here, is a warm-up that does
    /// nothing.
    ///
    /// MUTATION: let `warm` fall through to `ask` whatever the state and it
    /// joins the page's request instead of answering `AlreadyAsked`.
    #[test]
    fn the_warm_up_does_nothing_once_anybody_has_asked() {
        let heard: Heard = Rc::default();
        let mut slot = EnvironmentSlot::<u32>::new();
        let EnvironmentAsk::Create(ticket) = slot.ask(listener(&heard, "page")) else {
            panic!("the page makes the call");
        };
        assert!(matches!(
            slot.warm(listener(&heard, "warm-up")),
            EnvironmentAsk::AlreadyAsked
        ));
        let (waiting, error) = slot.arrived(ticket, Ok(1));
        assert_eq!(waiting.len(), 1, "only the page was waiting");
        answer_all(waiting, error);
        assert!(matches!(
            slot.warm(listener(&heard, "warm-up")),
            EnvironmentAsk::AlreadyAsked
        ));
        assert_eq!(*heard.borrow(), vec![("page", None)]);
    }

    /// RED (54) — **a failed request leaves the state empty, answers everybody
    /// who waited with the failure, and the next ask makes a new call.**
    ///
    /// MUTATION: leave the state `Requested` after a failed completion and the
    /// next page joins a request that will never answer again.
    #[test]
    fn a_failed_request_leaves_the_state_empty_and_the_next_ask_calls_again() {
        let heard: Heard = Rc::default();
        let mut slot = EnvironmentSlot::<u32>::new();
        let EnvironmentAsk::Create(first) = slot.warm(listener(&heard, "warm-up")) else {
            panic!("the warm-up makes the call");
        };
        assert!(matches!(
            slot.ask(listener(&heard, "page")),
            EnvironmentAsk::Joined
        ));
        let (waiting, error) = slot.arrived(first, Err("0x8007139F".to_owned()));
        answer_all(waiting, error);
        assert_eq!(slot.phase(), WebEnvironmentPhase::None);
        assert_eq!(
            *heard.borrow(),
            vec![
                ("warm-up", Some("0x8007139F".to_owned())),
                ("page", Some("0x8007139F".to_owned())),
            ],
            "the failure is answered to everybody who waited, as their own would have been"
        );
        let EnvironmentAsk::Create(second) = slot.ask(listener(&heard, "next page")) else {
            panic!("the next page asks again");
        };
        assert_ne!(first, second);
    }

    /// RED (54) — **a creation call refused where it stands is the caller's to
    /// report, and the slot is empty after it**; a call whose completion
    /// already answered during the call is not reported twice.
    ///
    /// MUTATION: have `refused` leave the state `Requested` and the next ask
    /// joins a call that was never made.
    #[test]
    fn a_call_refused_where_it_stands_empties_the_slot() {
        let heard: Heard = Rc::default();
        let mut slot = EnvironmentSlot::<u32>::new();
        let EnvironmentAsk::Create(ticket) = slot.ask(listener(&heard, "page")) else {
            panic!("the call is made");
        };
        assert!(slot.refused(ticket), "the caller reports it");
        assert_eq!(slot.phase(), WebEnvironmentPhase::None);
        assert!(heard.borrow().is_empty(), "and nobody else heard anything");

        let EnvironmentAsk::Create(ticket) = slot.ask(listener(&heard, "page")) else {
            panic!("the call is made again");
        };
        let (waiting, error) = slot.arrived(ticket, Err("no runtime".to_owned()));
        answer_all(waiting, error);
        assert!(
            !slot.refused(ticket),
            "a completion that already answered is not reported a second time"
        );
    }

    /// RED (54) — **a forgotten request is not waited on, and its waiters get
    /// the next environment**; a late completion for it is not adopted.
    ///
    /// MUTATION: have `forget` drop the waiters of a request in flight and the
    /// page that was waiting never hears anything.
    #[test]
    fn a_forgotten_request_hands_its_waiters_to_the_next_call() {
        let heard: Heard = Rc::default();
        let mut slot = EnvironmentSlot::<u32>::new();
        let EnvironmentAsk::Create(stale) = slot.ask(listener(&heard, "page")) else {
            panic!("the call is made");
        };
        slot.forget();
        assert_eq!(slot.phase(), WebEnvironmentPhase::None);
        let EnvironmentAsk::Create(fresh) = slot.ask(listener(&heard, "rebuild")) else {
            panic!("the rebuild makes a new call");
        };
        let (late, _) = slot.arrived(stale, Ok(1));
        assert!(late.is_empty(), "the forgotten call answers nobody");
        assert_eq!(slot.phase(), WebEnvironmentPhase::Requested);
        let (waiting, error) = slot.arrived(fresh, Ok(2));
        answer_all(waiting, error);
        assert_eq!(*heard.borrow(), vec![("page", None), ("rebuild", None)]);
        assert_eq!(slot.environment(), Some(2));
        slot.forget();
        assert_eq!(slot.phase(), WebEnvironmentPhase::None);
    }
}
