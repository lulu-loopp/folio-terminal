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
        }
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
        match std::mem::replace(&mut self.state, State::None) {
            State::Requested(_, waiting) => self.carried.extend(waiting),
            State::None | State::Ready(_) => {}
        }
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
