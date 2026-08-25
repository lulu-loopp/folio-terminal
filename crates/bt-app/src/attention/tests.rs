//! The arrival grid of `attention` plan §11.1.4, cell by cell, plus the invariants that hold
//! across every path through it.
//!
//! **Why cell by cell.** The grid is four states by nine arrivals, and the defects the plan spent
//! four review rounds finding were all *single cells*: a restatement read as a new request, a new
//! request read as a restatement, a late confirmation that never interrupted, a receipt that
//! removed the wrong credential. None of them is visible in a walk-through of the happy path, and
//! all of them are one assertion here.
//!
//! **Zero-trace cells are assertions too.** A machine that wrote a line whenever it *saw* a
//! standing request would write sixty lines a second while a program held one up, and the file
//! would be useless for the one thing it exists for. So every idempotent cell below asserts the
//! empty vector, and that is the pin.

use super::*;

fn site() -> Site {
    Site {
        tab: 1,
        seat: SeatId(2),
    }
}

/// One pane's ledger and the window serial it draws places from.
struct Pane {
    ledger: AttentionLedger,
    next_ticket: u64,
    reach: Reach,
}

impl Pane {
    fn new() -> Self {
        Self {
            ledger: AttentionLedger::default(),
            // The window's counter starts where the running build's does.
            next_ticket: 0,
            reach: Reach::Flash,
        }
    }

    /// One arrival, and the lines it decided.
    fn at(&mut self, event: Event) -> Vec<String> {
        self.ledger
            .apply(site(), self.reach, event, &mut self.next_ticket)
            .lines
    }

    /// One arrival, and everything it decided.
    fn outcome(&mut self, event: Event) -> Outcome {
        self.ledger
            .apply(site(), self.reach, event, &mut self.next_ticket)
    }

    fn state(&self) -> State {
        self.ledger.state()
    }

    fn away(&mut self) -> Vec<String> {
        self.at(Event::Settle {
            active: false,
            focused: false,
        })
    }

    fn watched(&mut self) -> Vec<String> {
        self.at(Event::Settle {
            active: true,
            focused: true,
        })
    }
}

fn strong_wait(kind: WaitKind) -> Event {
    Event::StrongWait(WaitSlot::Level(kind))
}

fn keyed_wait(kind: WaitKind, key: &str) -> Event {
    Event::StrongWait(WaitSlot::Keyed {
        kind,
        key: key.to_owned(),
    })
}

fn clear_all(reason: ClearReason) -> Event {
    Event::StrongClear {
        selector: ClearSelector::All,
        class: ClearClass::Boundary,
        reason,
        begins_turn: false,
    }
}

/// A pane whose weak tier is asserting, unanswered, with no place taken.
fn requested() -> Pane {
    let mut pane = Pane::new();
    assert_eq!(
        pane.at(Event::WeakYes(1)),
        ["mint tab=1 seat=SeatId(2) episode=1 src=osc gen=1 grounds=requested prev=-"]
    );
    pane
}

/// The same, having taken place 0.
fn queued() -> Pane {
    let mut pane = requested();
    assert_eq!(
        pane.away(),
        ["admit tab=1 seat=SeatId(2) ticket=0 episode=1 grounds=requested active=0 focused=0"]
    );
    pane
}

/// The same, answered — the state a bit cannot represent.
fn acknowledged() -> Pane {
    let mut pane = requested();
    assert_eq!(
        pane.at(Event::Answer(AnswerKind::Keyboard)),
        ["answer tab=1 seat=SeatId(2) episode=1 by=keyboard weak=1 strong=0"]
    );
    assert_eq!(pane.state(), State::Acknowledged(1));
    pane
}

// ---------------------------------------------------------------------------
// The grid, one row per arrival
// ---------------------------------------------------------------------------

/// GRID — `WeakYes`, all four states.
///
/// The rising edge mints; a restatement decides nothing and therefore writes nothing. The second
/// half is the flood guard: `bt-term` reports the level it holds, and a program restating `yes`
/// every second must not produce a request per second here.
#[test]
fn weak_yes_mints_on_the_rise_and_is_silent_on_a_restatement() {
    let mut idle = Pane::new();
    assert_eq!(
        idle.at(Event::WeakYes(1)),
        ["mint tab=1 seat=SeatId(2) episode=1 src=osc gen=1 grounds=requested prev=-"]
    );
    assert_eq!(idle.state(), State::Requested(1));

    let mut pane = requested();
    assert!(pane.at(Event::WeakYes(1)).is_empty());
    assert_eq!(pane.state(), State::Requested(1));

    let mut pane = queued();
    assert!(pane.at(Event::WeakYes(1)).is_empty());
    assert_eq!(
        pane.state(),
        State::Queued {
            episode: 1,
            ticket: 0
        },
        "a restatement does not re-stamp the place"
    );

    let mut pane = acknowledged();
    assert!(pane.at(Event::WeakYes(1)).is_empty());
    assert_eq!(pane.state(), State::Acknowledged(1));
}

/// GRID — `WeakNo`, all four states, and the three shapes of "a credential left".
///
/// The shapes are what `attention` plan §11.1.5 closed a hole with: `withdraw` carries a `ticket=`
/// and two of the four states have none, so an unqueued destruction says `drop` and a source
/// leaving a still-live episode says `clear`.
#[test]
fn weak_no_writes_the_shape_that_matches_what_was_held() {
    let mut idle = Pane::new();
    assert!(idle.at(Event::WeakNo).is_empty());

    let mut pane = requested();
    assert_eq!(
        pane.at(Event::WeakNo),
        [
            "clear tab=1 seat=SeatId(2) episode=1 src=osc gen=1 reason=program",
            "drop tab=1 seat=SeatId(2) episode=1 reason=program",
        ]
    );
    assert_eq!(pane.state(), State::Idle);

    let mut pane = queued();
    assert_eq!(
        pane.at(Event::WeakNo),
        ["withdraw tab=1 seat=SeatId(2) ticket=0 episode=1 reason=program src=osc"]
    );
    assert_eq!(pane.state(), State::Idle);

    let mut pane = acknowledged();
    assert_eq!(
        pane.at(Event::WeakNo),
        [
            "clear tab=1 seat=SeatId(2) episode=1 src=osc gen=1 reason=program",
            "drop tab=1 seat=SeatId(2) episode=1 reason=program",
        ],
        "the credential the user already answered still has to be taken down"
    );
    assert_eq!(pane.state(), State::Idle);
}

/// PIN — **the wording is read from what is *unanswered*, not from what is standing.**
///
/// A strong credential the user has already dealt with is still in the table — nothing withdrew
/// it, and nothing here withdraws it on the program's behalf. So when the weak tier asks again, the
/// new request has to be worded from the evidence that is actually outstanding: "attention
/// requested", not "waiting for you". Reading the wording off "is a strong credential present"
/// instead says the second sentence on the strength of a credential you already answered — and,
/// because the wording is also what opens the door to the desktop, it interrupts you about it.
///
/// This is the same error the plan corrected twice already, in miniature: the weaker fact promoted
/// to the stronger claim.
#[test]
fn a_new_request_is_worded_from_what_is_still_unanswered() {
    let mut pane = Pane::new();
    pane.at(strong_wait(WaitKind::Permission));
    pane.at(Event::Answer(AnswerKind::Keyboard));
    assert_eq!(
        pane.ledger.grounds(),
        Grounds::Requested,
        "an answered credential is not evidence of anything outstanding"
    );
    assert_eq!(
        pane.at(Event::WeakYes(1)),
        ["mint tab=1 seat=SeatId(2) episode=2 src=osc gen=1 grounds=requested prev=1"]
    );
    assert_eq!(
        pane.away(),
        ["admit tab=1 seat=SeatId(2) ticket=0 episode=2 grounds=requested active=0 focused=0"],
        "and it does not reach the desktop on the strength of one"
    );
}

/// GRID — a `WeakNo` while the strong tier is still unanswered keeps the episode alive.
///
/// One source withdrawing is not the request ending. This is the cell that distinguishes `clear`
/// from `withdraw`, and it is the reason the two verbs exist.
#[test]
fn weak_no_under_a_standing_strong_credential_only_clears_its_own_layer() {
    let mut pane = requested();
    assert_eq!(
        pane.at(strong_wait(WaitKind::Permission)),
        ["upgrade tab=1 seat=SeatId(2) episode=1 grounds=awaiting src=pipe gen=1"]
    );
    assert_eq!(
        pane.at(Event::WeakNo),
        ["clear tab=1 seat=SeatId(2) episode=1 src=osc gen=1 reason=program"]
    );
    assert_eq!(pane.state(), State::Requested(1), "still the same request");
    assert_eq!(pane.ledger.grounds(), Grounds::AwaitingInput);
}

/// GRID — `StrongWait` that raises, all four states.
///
/// The fourth cell is the narrow review's counterexample ①: a pane that has been answered, on
/// which a hook then reports a **real** permission request, must be able to queue again. Reading
/// that arrival as a restatement is how an agent goes dark on you.
#[test]
fn a_raising_strong_wait_mints_upgrades_or_re_queues() {
    let mut idle = Pane::new();
    assert_eq!(
        idle.at(strong_wait(WaitKind::Permission)),
        ["mint tab=1 seat=SeatId(2) episode=1 src=pipe gen=1 grounds=awaiting prev=-"]
    );

    let mut pane = requested();
    assert_eq!(
        pane.at(strong_wait(WaitKind::Permission)),
        ["upgrade tab=1 seat=SeatId(2) episode=1 grounds=awaiting src=pipe gen=1"],
        "no ticket is carried, because none is held"
    );

    let mut pane = queued();
    assert_eq!(
        pane.at(strong_wait(WaitKind::Permission)),
        [
            "upgrade tab=1 seat=SeatId(2) ticket=0 episode=1 grounds=awaiting src=pipe gen=1",
            "toast tab=1 seat=SeatId(2) why=awaiting ticket=0 episode=1 reach=flash",
        ]
    );

    let mut pane = acknowledged();
    assert_eq!(
        pane.at(strong_wait(WaitKind::Permission)),
        ["mint tab=1 seat=SeatId(2) episode=2 src=pipe gen=1 grounds=awaiting prev=1"],
        "an answered pane can be asked again"
    );
    assert_eq!(pane.state(), State::Requested(2));
    assert_eq!(
        pane.away(),
        [
            "admit tab=1 seat=SeatId(2) ticket=0 episode=2 grounds=awaiting active=0 focused=0",
            "toast tab=1 seat=SeatId(2) why=awaiting ticket=0 episode=2 reach=flash"
        ],
        "and it takes a place"
    );
}

/// GRID — `StrongWait` that restates, in the mode where restatement is possible.
///
/// With an identifier the producer's evidence outranks ours: the same id is the same request even
/// after it has been answered. Without one the watermark decides, and a level going up again after
/// an answer is a new thing — pinned separately in the level walk-through below.
#[test]
fn a_keyed_strong_wait_restated_decides_nothing() {
    let mut pane = Pane::new();
    assert_eq!(pane.at(keyed_wait(WaitKind::Permission, "A")).len(), 1);
    assert!(pane.at(keyed_wait(WaitKind::Permission, "A")).is_empty());
    assert_eq!(
        pane.away().len(),
        2,
        "it is admitted, and interrupted about once"
    );
    assert!(pane.at(keyed_wait(WaitKind::Permission, "A")).is_empty());
    assert_eq!(pane.at(Event::Answer(AnswerKind::Keyboard)).len(), 1);
    assert!(
        pane.at(keyed_wait(WaitKind::Permission, "A")).is_empty(),
        "answered or not, the same key is the same request"
    );
    assert_eq!(pane.state(), State::Acknowledged(1));
}

/// GRID — `StrongClear`, all four states, and the narrow review's counterexample ②.
///
/// The third cell is the one the plan rewrote a rule for: the strong tier withdrawing while the
/// weak tier keeps asserting **lowers the wording and keeps the place**. Keeping the wording would
/// leave the pane saying "waiting for you" on the strength of a credential that no longer exists;
/// dropping the place would lose a request the weak tier is still making; interrupting again would
/// be a second interruption for one request.
#[test]
fn a_strong_clear_takes_down_its_own_layer_and_nothing_else() {
    let mut idle = Pane::new();
    assert!(idle.at(clear_all(ClearReason::Hook)).is_empty());

    let mut pane = Pane::new();
    pane.at(strong_wait(WaitKind::Permission));
    assert_eq!(
        pane.at(clear_all(ClearReason::Hook)),
        [
            "clear tab=1 seat=SeatId(2) episode=1 src=pipe gen=1 reason=hook",
            "drop tab=1 seat=SeatId(2) episode=1 reason=hook",
        ]
    );
    assert_eq!(pane.state(), State::Idle);

    let mut pane = Pane::new();
    pane.at(strong_wait(WaitKind::Permission));
    pane.away();
    assert_eq!(
        pane.at(clear_all(ClearReason::Hook)),
        ["withdraw tab=1 seat=SeatId(2) ticket=0 episode=1 reason=program src=pipe"]
    );
    assert_eq!(pane.state(), State::Idle);

    // Counterexample ②: weak still up, strong withdrawn.
    let mut pane = requested();
    pane.at(strong_wait(WaitKind::Permission));
    let admitted = pane.away();
    assert_eq!(
        admitted,
        [
            "admit tab=1 seat=SeatId(2) ticket=0 episode=1 grounds=awaiting active=0 focused=0",
            "toast tab=1 seat=SeatId(2) why=awaiting ticket=0 episode=1 reach=flash",
        ]
    );
    assert_eq!(
        pane.at(clear_all(ClearReason::Hook)),
        [
            "clear tab=1 seat=SeatId(2) episode=1 src=pipe gen=1 reason=hook",
            "downgrade tab=1 seat=SeatId(2) ticket=0 episode=1 grounds=requested src=pipe reason=clear",
        ]
    );
    assert_eq!(
        pane.state(),
        State::Queued {
            episode: 1,
            ticket: 0
        },
        "the place is kept"
    );
    assert_eq!(
        pane.ledger.grounds(),
        Grounds::Requested,
        "the wording falls"
    );
    assert!(
        pane.at(strong_wait(WaitKind::Permission))
            .iter()
            .all(|line| !line.starts_with("toast")),
        "and there is no second interruption for one request"
    );
}

/// GRID — `Settle`, all four states, both answers.
///
/// The fourth cell is the whole reason this machine is not a bit: **`Acknowledged` is a fixed
/// point.** With a bit, the frame after an answer sees "still asserted" and hands out a fresh
/// place — the badge outliving the thing it reports, which is the 2026-08-21 defect.
#[test]
fn settle_admits_once_refuses_once_and_leaves_everything_else_alone() {
    let mut idle = Pane::new();
    assert!(idle.away().is_empty());
    assert!(idle.watched().is_empty());

    let mut pane = requested();
    assert_eq!(
        pane.away(),
        ["admit tab=1 seat=SeatId(2) ticket=0 episode=1 grounds=requested active=0 focused=0"]
    );
    assert!(pane.away().is_empty(), "a held place is a fixed point");
    assert!(pane.watched().is_empty(), "looking at it changes nothing");

    let mut pane = acknowledged();
    assert!(pane.away().is_empty(), "answered is a fixed point");
    assert!(pane.watched().is_empty());
    assert_eq!(pane.state(), State::Acknowledged(1));
}

/// GRID — a refusal is decided once, not on every frame of being watched.
///
/// The pass runs on every turn of the event loop and a standing request does not go away while you
/// look at it, so a station that spoke from the *level* would write `refuse` sixty times a second.
/// Being still refused is not a decision.
#[test]
fn a_watched_request_is_refused_once_per_episode() {
    let mut pane = requested();
    assert_eq!(
        pane.watched(),
        ["refuse tab=1 seat=SeatId(2) episode=1 reason=watched active=1 focused=1"]
    );
    assert!(pane.watched().is_empty());
    assert!(pane.watched().is_empty());
    assert_eq!(pane.state(), State::Requested(1), "and it is still asking");

    // A new request is a new decision.
    pane.at(Event::Answer(AnswerKind::Keyboard));
    pane.at(strong_wait(WaitKind::Permission));
    assert_eq!(
        pane.watched(),
        ["refuse tab=1 seat=SeatId(2) episode=2 reason=watched active=1 focused=1"]
    );
}

/// GRID — `Answer`, all four states.
///
/// An answer moves **both** watermarks, because "I answered" is a statement about the pane and not
/// about one credential. That is why a watermark is enough and a set of answered identifiers is
/// not needed.
#[test]
fn answering_answers_everything_on_the_table() {
    let mut idle = Pane::new();
    assert!(idle.at(Event::Answer(AnswerKind::Keyboard)).is_empty());

    let mut pane = requested();
    assert_eq!(
        pane.at(Event::Answer(AnswerKind::Paste)),
        ["answer tab=1 seat=SeatId(2) episode=1 by=paste weak=1 strong=0"],
        "no place was held, so no place is named"
    );

    let mut pane = queued();
    pane.at(strong_wait(WaitKind::Permission));
    assert_eq!(
        pane.at(Event::Answer(AnswerKind::MouseButton)),
        ["answer tab=1 seat=SeatId(2) ticket=0 episode=1 by=mouse-button weak=1 strong=1"]
    );
    assert_eq!(pane.state(), State::Acknowledged(1));
    assert_eq!(pane.ledger.ticket(), None, "the place is given up");

    let mut pane = acknowledged();
    assert!(pane.at(Event::Answer(AnswerKind::Keyboard)).is_empty());
}

/// GRID — `LeafGone`, all four states, and the serial that does not come back.
#[test]
fn a_vanished_pane_expires_its_place_and_drops_everything_else() {
    let mut idle = Pane::new();
    assert!(idle.at(Event::LeafGone).is_empty());

    let mut pane = requested();
    assert_eq!(
        pane.at(Event::LeafGone),
        ["drop tab=1 seat=SeatId(2) episode=1 reason=leaf-gone"]
    );

    let mut pane = queued();
    assert_eq!(
        pane.at(Event::LeafGone),
        ["expire tab=1 seat=SeatId(2) ticket=0 episode=1 reason=leaf-gone"]
    );
    assert_eq!(
        pane.next_ticket, 1,
        "the window's serial does not come back"
    );

    let mut pane = acknowledged();
    assert_eq!(
        pane.at(Event::LeafGone),
        ["drop tab=1 seat=SeatId(2) episode=1 reason=leaf-gone"]
    );
}

/// PIN (`attention` plan §4 B4) — **a tab dragged into another window leaves its place behind and
/// takes everything else with it.**
///
/// The half that is easy to get wrong is not the surrender, it is what survives it: the pane is
/// still being asked for, so the window it lands in has to admit it again — with a place drawn from
/// *that* window's serial, because two windows' places are two orderings and a number carried
/// across would belong to both.
///
/// Red gate: clear the credentials along with the place and the pane arrives silent, which is a
/// standing request the user never sees again; wind the serial back and the two panes' places
/// become indistinguishable in the one file that is supposed to tell them apart.
#[test]
fn a_pane_carried_to_another_window_gives_up_its_place_and_keeps_its_request() {
    let mut idle = Pane::new();
    assert!(idle.ledger.surrender_place(site()).lines.is_empty());

    let mut pane = requested();
    assert!(
        pane.ledger.surrender_place(site()).lines.is_empty(),
        "a pane with nothing to give up gives nothing up, and says nothing"
    );
    assert_eq!(pane.state(), State::Requested(1));

    let mut pane = queued();
    assert_eq!(
        pane.ledger.surrender_place(site()).lines,
        ["expire tab=1 seat=SeatId(2) ticket=0 episode=1 reason=torn-out"]
    );
    assert_eq!(pane.state(), State::Requested(1), "and it is still asking");
    assert_eq!(
        pane.next_ticket, 1,
        "the serial the window issued does not come back"
    );

    // The window it landed in admits it again, out of its own serial — which starts wherever that
    // window's own counter stands, and here is simply the next one.
    assert_eq!(
        pane.away(),
        ["admit tab=1 seat=SeatId(2) ticket=1 episode=1 grounds=requested active=0 focused=0"],
        "the same episode, a new place"
    );
}

/// GRID — `MarkSeen`, all four states (`attention` plan §10.9, edge e17).
///
/// Switching to a tab spends every bell and every failing exit code in it, deliberately and for a
/// reason that is written down: a tab's dot is an assertion about the whole fleet under its lid.
/// **A standing request is not one of those.** It is a sentence a program is still saying, and a
/// glance does not unsay it.
#[test]
fn being_looked_at_does_not_touch_the_ledger() {
    for mut pane in [Pane::new(), requested(), queued(), acknowledged()] {
        let before = pane.state();
        assert!(pane.at(Event::MarkSeen).is_empty());
        assert_eq!(pane.state(), before);
    }
}

// ---------------------------------------------------------------------------
// The one door to the desktop (§11.2)
// ---------------------------------------------------------------------------

/// PIN — **the late confirmation interrupts exactly once, and does not re-order the queue.**
///
/// A weak credential admitted first raises nothing, correctly: "a program wants you" is not "a
/// program is blocked on you". Six seconds later a hook confirms it, and *that* is the moment the
/// pane is really waiting. Pinning the interruption to the admit edge alone loses it entirely —
/// which is the hole this gate was rewritten to close.
///
/// The place is not re-stamped: first come, first served must not be reshuffled because the
/// evidence for one request got stronger.
#[test]
fn a_late_strong_credential_interrupts_once_without_moving_the_place() {
    let mut pane = requested();
    assert_eq!(
        pane.away(),
        ["admit tab=1 seat=SeatId(2) ticket=0 episode=1 grounds=requested active=0 focused=0"],
        "a weak request alone does not reach the desktop"
    );
    let out = pane.outcome(strong_wait(WaitKind::Permission));
    assert_eq!(
        out.raised,
        Some(Raised {
            why: Why::Awaiting,
            reach: Reach::Flash,
            ticket: Some(0),
            episode: Some(1),
        })
    );
    assert_eq!(
        pane.state(),
        State::Queued {
            episode: 1,
            ticket: 0
        },
        "same place, same number"
    );
    let again = pane.outcome(keyed_wait(WaitKind::Permission, "B"));
    assert_eq!(again.raised, None, "one request, one interruption");
}

/// PIN — **a request refused for being watched is not interrupted about, and is not owed one
/// later.**
///
/// You are looking at the pane. When you look away the gate is evaluated again on its own terms;
/// what it must not do is deliver the interruption that was declined earlier, because that is a
/// notification queued and played back — the thing this build refuses to do anywhere.
#[test]
fn a_watched_request_is_not_interrupted_about_and_nothing_is_owed() {
    let mut pane = Pane::new();
    pane.at(strong_wait(WaitKind::Permission));
    assert_eq!(
        pane.watched(),
        ["refuse tab=1 seat=SeatId(2) episode=1 reason=watched active=1 focused=1"]
    );
    let out = pane.outcome(Event::Settle {
        active: false,
        focused: false,
    });
    assert_eq!(
        out.lines,
        [
            "admit tab=1 seat=SeatId(2) ticket=0 episode=1 grounds=awaiting active=0 focused=0",
            "toast tab=1 seat=SeatId(2) why=awaiting ticket=0 episode=1 reach=flash",
        ],
        "one interruption, taken when the door was actually open"
    );
}

// ---------------------------------------------------------------------------
// The level path — no identifier anywhere (§12.1.4)
// ---------------------------------------------------------------------------

/// PIN (`attention` plan §12.1.4, t0–t6) — **the whole walk-through, on a family with no
/// identifier at all.**
///
/// This is the main road, not the fallback: today every kind of every family runs here, because a
/// field path may only be written when it has been quoted verbatim from upstream and none has been.
/// The walk-through is the plan's, line for line.
#[test]
fn two_permission_requests_across_a_turn_end_on_the_level_path() {
    let mut pane = Pane::new();

    // t0 — a request arrives.
    assert_eq!(
        pane.at(strong_wait(WaitKind::Permission)),
        ["mint tab=1 seat=SeatId(2) episode=1 src=pipe gen=1 grounds=awaiting prev=-"]
    );
    // t1 — it takes a place, and interrupts once.
    assert_eq!(
        pane.away(),
        [
            "admit tab=1 seat=SeatId(2) ticket=0 episode=1 grounds=awaiting active=0 focused=0",
            "toast tab=1 seat=SeatId(2) why=awaiting ticket=0 episode=1 reach=flash",
        ]
    );
    // t1.5 — the late duplicate the fixed-key design could not absorb.
    assert!(
        pane.at(strong_wait(WaitKind::Permission)).is_empty(),
        "an unanswered level going up again is the same wait"
    );
    // t2 — you answer.
    assert_eq!(
        pane.at(Event::Answer(AnswerKind::Keyboard)),
        ["answer tab=1 seat=SeatId(2) ticket=0 episode=1 by=keyboard weak=0 strong=1"]
    );
    // t3 — no receipt comes; this family has none.
    // t4 — the turn ends and clears everything.
    assert_eq!(
        pane.at(Event::StrongClear {
            selector: ClearSelector::All,
            class: ClearClass::Boundary,
            reason: ClearReason::Hook,
            begins_turn: false,
        }),
        [
            "clear tab=1 seat=SeatId(2) episode=1 src=pipe gen=1 reason=hook",
            "drop tab=1 seat=SeatId(2) episode=1 reason=hook",
        ]
    );
    assert_eq!(pane.state(), State::Idle);
    // t5/t6 — the second request is a second episode, pointing back at the first.
    assert_eq!(
        pane.at(strong_wait(WaitKind::Permission)),
        ["mint tab=1 seat=SeatId(2) episode=2 src=pipe gen=2 grounds=awaiting prev=1"]
    );
    assert_eq!(
        pane.away(),
        [
            "admit tab=1 seat=SeatId(2) ticket=1 episode=2 grounds=awaiting active=0 focused=0",
            "toast tab=1 seat=SeatId(2) why=awaiting ticket=1 episode=2 reach=flash",
        ]
    );
}

/// PIN (`attention` plan §12.1.4, the t4-absent branch) — **two approvals inside one turn, with no
/// clear in between.**
///
/// This is the branch a fixed per-kind key fails on, and it fails silently: the second request
/// looks like a restatement of the first, decides nothing, and the pane never lights up while a
/// person waits at an approval box. **The change of generation is carried by the watermark, not by
/// the key being different** — which is precisely why the level path works at all.
#[test]
fn a_second_request_inside_one_turn_is_a_second_episode() {
    let mut pane = Pane::new();
    pane.at(strong_wait(WaitKind::Permission));
    pane.away();
    pane.at(Event::Answer(AnswerKind::Keyboard));
    // t4′: nothing clears. The answered level is still in the table.
    assert_eq!(
        pane.at(strong_wait(WaitKind::Permission)),
        ["mint tab=1 seat=SeatId(2) episode=2 src=pipe gen=2 grounds=awaiting prev=1"],
        "an answered level going up again can only be a new thing"
    );
    assert_eq!(
        pane.away(),
        [
            "admit tab=1 seat=SeatId(2) ticket=1 episode=2 grounds=awaiting active=0 focused=0",
            "toast tab=1 seat=SeatId(2) why=awaiting ticket=1 episode=2 reach=flash",
        ]
    );
}

/// PIN — **an unanswered level that goes up again is absorbed, and it costs nothing.**
///
/// The two readings of that arrival — a duplicate, or a genuine second request — have identical
/// observable consequences: still asking, still `awaiting`, same episode, same place, already
/// interrupted about. Merging them is therefore free, and it is what makes the level path lose
/// nothing a user could see.
#[test]
fn an_unanswered_level_absorbs_repeats_with_no_observable_cost() {
    let mut pane = Pane::new();
    pane.at(strong_wait(WaitKind::Permission));
    pane.away();
    for _ in 0..5 {
        assert!(pane.at(strong_wait(WaitKind::Permission)).is_empty());
    }
    assert_eq!(
        pane.state(),
        State::Queued {
            episode: 1,
            ticket: 0
        }
    );
    assert_eq!(
        pane.at(Event::Answer(AnswerKind::Keyboard)).len(),
        1,
        "and one answer still answers it"
    );
}

// ---------------------------------------------------------------------------
// The three classes of clear (§13.1)
// ---------------------------------------------------------------------------

/// PIN (`attention` plan §13.4 ㉔) — **a wait whose only exit arrives with nobody having answered
/// still gets out.**
///
/// `quota_auto_resume_fired` says "the wait for this kind is over"; by construction it arrives when
/// *nobody* answered, so it always sits above the watermark. Gating it on the watermark — which the
/// single-gate design did — makes the one documented exit for that wait unreachable, and the
/// credential can then only leave by a boundary or a ten-minute timer.
///
/// The second half is the other side of the same coin: a receipt that can only name a *kind* must
/// **not** be able to take down an unanswered credential, because it might be a late echo of the
/// previous one. Both halves in one run, because they are one rule.
#[test]
fn the_authoritative_end_of_a_wait_is_not_gated_on_an_answer() {
    let mut pane = Pane::new();
    pane.at(strong_wait(WaitKind::Quota));
    pane.away();
    assert_eq!(pane.ledger.grounds(), Grounds::AwaitingInput);

    // A kind-only receipt on an unanswered credential: refused, and silent about it.
    assert!(
        pane.at(Event::StrongClear {
            selector: ClearSelector::Kind(WaitKind::Quota),
            class: ClearClass::Receipt,
            reason: ClearReason::Hook,
            begins_turn: false,
        })
        .is_empty(),
        "a late echo may not erase the request that is standing now"
    );
    assert_eq!(
        pane.state(),
        State::Queued {
            episode: 1,
            ticket: 0
        }
    );

    // The program's own end of the wait: through, and it says why.
    assert_eq!(
        pane.at(Event::StrongClear {
            selector: ClearSelector::Kind(WaitKind::Quota),
            class: ClearClass::BoundaryKind,
            reason: ClearReason::AutoResume,
            begins_turn: false,
        }),
        ["withdraw tab=1 seat=SeatId(2) ticket=0 episode=1 reason=program src=pipe"]
    );
    assert_eq!(pane.state(), State::Idle);
}

/// PIN (`attention` plan §13.4 ㉖) — **a receipt that names one credential is not gated; the same
/// sequence with no name is.**
///
/// One kind, one sequence, two modes, two results. The gate exists for "I cannot say which one
/// ended"; pointing it at a receipt that *can* say leaves the identified path unable to do the one
/// thing identifying it was for.
#[test]
fn a_named_receipt_removes_what_it_names_and_an_unnamed_one_waits_its_turn() {
    let mut keyed = Pane::new();
    keyed.at(keyed_wait(WaitKind::Permission, "A"));
    keyed.away();
    assert_eq!(
        keyed.at(Event::StrongClear {
            selector: ClearSelector::Key {
                kind: WaitKind::Permission,
                key: "A".to_owned(),
            },
            class: ClearClass::Receipt,
            reason: ClearReason::Hook,
            begins_turn: false,
        }),
        ["withdraw tab=1 seat=SeatId(2) ticket=0 episode=1 reason=program src=pipe"],
        "it can only reach the one it names"
    );

    let mut level = Pane::new();
    level.at(strong_wait(WaitKind::Permission));
    level.away();
    assert!(
        level
            .at(Event::StrongClear {
                selector: ClearSelector::Kind(WaitKind::Permission),
                class: ClearClass::Receipt,
                reason: ClearReason::Hook,
                begins_turn: false,
            })
            .is_empty(),
        "the same receipt with no name may not guess"
    );

    // And a key that names nothing is a no-op with nothing to say.
    let mut keyed = Pane::new();
    keyed.at(keyed_wait(WaitKind::Permission, "A"));
    assert!(
        keyed
            .at(Event::StrongClear {
                selector: ClearSelector::Key {
                    kind: WaitKind::Permission,
                    key: "gone".to_owned(),
                },
                class: ClearClass::Receipt,
                reason: ClearReason::Hook,
                begins_turn: false,
            })
            .is_empty()
    );
}

// ---------------------------------------------------------------------------
// The event door (§11.7, §13.3)
// ---------------------------------------------------------------------------

/// PIN (`attention` plan §13.4 ㉕) — **a turn end is decided once, and `Nothing` is a decision.**
///
/// The reported defect: with the window focused the first source answers "do not interrupt" and,
/// on the "set the bit when something was actually raised" reading, leaves the bit down. Walk away,
/// let the second source of the same turn arrive, and a flash appears about a turn that ended while
/// you were watching. That is a notification queued and played back, and it is forbidden twice
/// over — by the rule against replay, and by this section's own "there must be a new turn in
/// between".
#[test]
fn a_turn_end_is_decided_once_even_when_the_decision_was_to_say_nothing() {
    let mut pane = Pane::new();
    let first =
        pane.ledger
            .announce_turn_end(site(), Reach::Nothing, true, Transport::Pipe, Via::Stop);
    assert_eq!(
        first.lines,
        ["toast tab=1 seat=SeatId(2) why=turn-end episode=- reach=nothing src=pipe via=stop"],
        "a decision leaves a mark even when the decision was to add nothing"
    );

    let second =
        pane.ledger
            .announce_turn_end(site(), Reach::Flash, true, Transport::Bel, Via::Bel);
    assert_eq!(second.lines, Vec::<String>::new());
    assert_eq!(
        second.raised, None,
        "no flash for a turn that is already over"
    );

    // Your next turn re-arms it.
    pane.at(Event::Answer(AnswerKind::Keyboard));
    let next =
        pane.ledger
            .announce_turn_end(site(), Reach::Toast, true, Transport::Osc, Via::Osc777);
    assert_eq!(
        next.lines,
        ["toast tab=1 seat=SeatId(2) why=turn-end episode=- reach=toast src=osc via=osc-777"]
    );
}

/// PIN — **with the setting off the door is shut, and it leaves no half state behind.**
///
/// Not evaluated, not written, and the bit is not set. Setting it would mean that turning the
/// setting on mid-turn produced a silence nobody could explain.
#[test]
fn a_disabled_turn_end_door_records_nothing_at_all() {
    let mut pane = Pane::new();
    let off =
        pane.ledger
            .announce_turn_end(site(), Reach::Toast, false, Transport::Pipe, Via::Stop);
    assert_eq!(off, Outcome::default());
    let on = pane
        .ledger
        .announce_turn_end(site(), Reach::Toast, true, Transport::Pipe, Via::Stop);
    assert_eq!(
        on.lines.len(),
        1,
        "the bit was not left set by the closed door"
    );
}

/// PIN (red line 14) — **the event tier mints nothing and queues nothing.**
///
/// Four sources say "a turn ended" and none of them may put a pane in the queue. Every regression
/// this block is about started with an event being promoted to a state.
#[test]
fn the_event_door_never_mints_an_episode_or_takes_a_place() {
    let mut pane = Pane::new();
    for (source, via) in [
        (Transport::Pipe, Via::Stop),
        (Transport::Bel, Via::Bel),
        (Transport::Osc, Via::Osc9),
        (Transport::Osc, Via::Osc99),
        (Transport::Pipe, Via::Notify),
        (Transport::Pipe, Via::AgentSettled),
        (Transport::Pipe, Via::StopFailure),
        (Transport::Osc, Via::Osc1337),
    ] {
        pane.ledger
            .announce_turn_end(site(), Reach::Toast, true, source, via);
        pane.at(Event::Answer(AnswerKind::Keyboard));
    }
    assert_eq!(pane.state(), State::Idle);
    assert_eq!(pane.ledger.ticket(), None);
    assert_eq!(pane.next_ticket, 0, "not one place was handed out");
}

// ---------------------------------------------------------------------------
// The trace's field contract (§11.1.5, six clauses)
// ---------------------------------------------------------------------------

/// Everything a long run decided, for the contract sweep below.
fn a_long_run() -> Vec<String> {
    let mut pane = Pane::new();
    let mut lines = Vec::new();
    lines.extend(pane.at(Event::WeakYes(1)));
    lines.extend(pane.watched());
    lines.extend(pane.away());
    lines.extend(pane.at(strong_wait(WaitKind::Permission)));
    lines.extend(pane.at(Event::StrongClear {
        selector: ClearSelector::Kind(WaitKind::Permission),
        class: ClearClass::BoundaryKind,
        reason: ClearReason::AutoResume,
        begins_turn: false,
    }));
    lines.extend(pane.at(Event::Answer(AnswerKind::MouseWheel)));
    lines.extend(pane.at(Event::WeakNo));
    lines.extend(
        pane.ledger
            .announce_turn_end(site(), Reach::Flash, true, Transport::Osc, Via::Osc9)
            .lines,
    );
    lines.extend(pane.at(keyed_wait(WaitKind::Elicitation, "e-1")));
    lines.extend(pane.away());
    lines.extend(pane.at(Event::LeafGone));
    lines.push(claim_line(
        1,
        pane.ledger.claim_episode(true, false),
        "Awaiting",
        "Silent",
    ));
    lines.push(claim_line(
        1,
        pane.ledger.claim_episode(false, false),
        "Bell",
        "Unread",
    ));
    lines
}

/// PIN (`attention` plan §11.1.5, and §13.4's sweep) — **the six field clauses, over a real run.**
///
/// The contract's whole value is that it has no exception: a reader scanning the file must never
/// have to stop and remember which verb is the one that does not name its request. `claim` was the
/// last line standing outside it, and the fix was to give it the field rather than to give the
/// contract an exception.
#[test]
fn every_line_of_a_run_keeps_the_field_contract() {
    let lines = a_long_run();
    assert!(!lines.is_empty());
    for line in &lines {
        let verb = line.split_whitespace().next().unwrap_or_default();
        assert!(
            line.contains(" episode="),
            "every line names its request, or says it has none: {line}"
        );
        let has_ticket = line.contains(" ticket=");
        match verb {
            "withdraw" | "expire" => assert!(has_ticket, "a place was held: {line}"),
            "drop" | "clear" | "mint" | "refuse" => {
                assert!(!has_ticket, "no place was held: {line}");
            }
            _ => {}
        }
        assert!(
            !line.contains("by=mouse-motion"),
            "a pointer crossing a pane is not an answer: {line}"
        );
    }
    let turn_end = lines
        .iter()
        .filter(|line| line.contains("why=turn-end"))
        .collect::<Vec<_>>();
    assert_eq!(turn_end.len(), 1);
    for line in turn_end {
        assert!(line.contains(" episode=-"), "{line}");
        assert!(!line.contains(" ticket="), "{line}");
        assert!(line.contains(" src="), "{line}");
        assert!(line.contains(" via="), "{line}");
    }
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("claim tab=1 episode=2 ")),
        "the claim that dropped an attention dot names the request that drove it, \
         and it reads that request out of the cursor a drop does not clear"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("claim tab=1 episode=- ")),
        "and one between two other claims honestly names none"
    );
}

/// PIN (§13.2.2) — **`src=` says how it arrived and `via=` says who said it, and a message that
/// came in over an OSC is never recorded as a bell.**
///
/// The two are separate fields because one cannot answer both questions: the value set for "how"
/// is closed and has three members, the value set for "who" grows with every CLI. Recording an
/// `OSC 9` turn end as `bel` would make it impossible to check whether the adapter that produces
/// the `OSC 9` is actually installed — which is the exact thing that request to upstream is about.
#[test]
fn a_turn_end_reported_over_an_osc_is_never_written_as_a_bell() {
    for (source, via, expected) in [
        (Transport::Osc, Via::Osc9, "src=osc via=osc-9"),
        (Transport::Osc, Via::Osc777, "src=osc via=osc-777"),
        (Transport::Osc, Via::Osc99, "src=osc via=osc-99"),
        (Transport::Bel, Via::Bel, "src=bel via=bel"),
        (Transport::Pipe, Via::Stop, "src=pipe via=stop"),
        (
            Transport::Pipe,
            Via::StopFailure,
            "src=pipe via=stop-failure",
        ),
        (Transport::Pipe, Via::Notify, "src=pipe via=notify"),
        (
            Transport::Pipe,
            Via::AgentSettled,
            "src=pipe via=agent-settled",
        ),
    ] {
        let mut pane = Pane::new();
        let out = pane
            .ledger
            .announce_turn_end(site(), Reach::Flash, true, source, via);
        assert!(
            out.lines[0].ends_with(expected),
            "{:?}/{:?}: {}",
            source,
            via,
            out.lines[0]
        );
    }
}

/// PIN (red line 13) — **an association key never leaves this process.**
///
/// A key can carry a tool name or a path. The trace records the generation it minted instead, which
/// answers every question a reader of the file actually has and none of the ones a key would leak.
#[test]
fn an_association_key_never_appears_in_the_trace() {
    let mut pane = Pane::new();
    let mut lines = pane.at(keyed_wait(WaitKind::Permission, "C.-secret_tool:99"));
    lines.extend(pane.away());
    lines.extend(pane.at(Event::StrongClear {
        selector: ClearSelector::Key {
            kind: WaitKind::Permission,
            key: "C.-secret_tool:99".to_owned(),
        },
        class: ClearClass::Receipt,
        reason: ClearReason::Hook,
        begins_turn: false,
    }));
    assert!(lines.len() >= 3);
    for line in lines {
        assert!(!line.contains("secret_tool"), "{line}");
    }
}

/// PIN — the key contract itself: bounded, and from an alphabet that cannot be read back as two
/// fields.
#[test]
fn a_wait_key_is_bounded_and_separator_free() {
    assert!(wait_key_is_well_formed("toolu_01ABC.-:9"));
    assert!(!wait_key_is_well_formed(""));
    assert!(!wait_key_is_well_formed("has space"));
    assert!(!wait_key_is_well_formed("has=equals"));
    assert!(!wait_key_is_well_formed(
        &"a".repeat(MAX_WAIT_KEY_BYTES + 1)
    ));
    assert!(wait_key_is_well_formed(&"a".repeat(MAX_WAIT_KEY_BYTES)));
}

// ---------------------------------------------------------------------------
// Invariants, over random paths (§11.1.6)
// ---------------------------------------------------------------------------

/// A deterministic sequence generator. No dependency, and a failure is reproducible from its seed.
struct Roll(u64);

impl Roll {
    fn next(&mut self) -> u64 {
        // A plain 64-bit LCG. Any full-period generator does; what matters is that a failing seed
        // replays exactly.
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        self.0 >> 33
    }

    fn pick(&mut self, count: u64) -> u64 {
        self.next() % count
    }
}

fn random_event(roll: &mut Roll, weak_generation: &mut u64) -> Event {
    let kinds = [
        WaitKind::Permission,
        WaitKind::Elicitation,
        WaitKind::Agent,
        WaitKind::Quota,
    ];
    let kind = kinds[roll.pick(4) as usize];
    match roll.pick(10) {
        0 => {
            *weak_generation += 1;
            Event::WeakYes(*weak_generation)
        }
        1 => Event::WeakNo,
        2 | 3 => Event::StrongWait(WaitSlot::Level(kind)),
        4 => Event::StrongWait(WaitSlot::Keyed {
            kind,
            key: format!("k{}", roll.pick(3)),
        }),
        5 => Event::StrongClear {
            selector: match roll.pick(2) {
                0 => ClearSelector::All,
                _ => ClearSelector::Kind(kind),
            },
            class: match roll.pick(3) {
                0 => ClearClass::Boundary,
                1 => ClearClass::BoundaryKind,
                _ => ClearClass::Receipt,
            },
            reason: ClearReason::Hook,
            begins_turn: roll.pick(2) == 0,
        },
        6 | 7 => Event::Settle {
            active: roll.pick(2) == 0,
            focused: roll.pick(2) == 0,
        },
        8 => Event::Answer(AnswerKind::Keyboard),
        _ => Event::MarkSeen,
    }
}

/// PIN (invariant I3) — **a place never outlives the thing it reports.**
///
/// `ticket.is_some() ⇒ asking`, written as an inequality a random walk can falsify. It is the
/// general form of 2026-08-21's defect — the badge that outlived its cause — and the general form
/// is what a property test can hold on paths nobody would think to write by hand.
#[test]
fn a_held_place_always_has_something_unanswered_behind_it() {
    for seed in 0..64 {
        let mut roll = Roll(seed * 2_654_435_761 + 1);
        let mut pane = Pane::new();
        let mut weak = 0;
        for step in 0..300 {
            let event = random_event(&mut roll, &mut weak);
            pane.at(event.clone());
            let held = pane.ledger.ticket().is_some();
            let asking = matches!(pane.state(), State::Queued { .. });
            assert_eq!(
                held, asking,
                "seed {seed} step {step} after {event:?}: a place with nothing behind it"
            );
        }
    }
}

/// PIN (invariant I2) — **numbers only go up, and `prev=` chains through however many times the
/// account fell idle.**
///
/// This is the property the earlier field sketch could not carry at all: with the live episode as
/// the only storage there is nowhere to read "the last one" from once it has been dropped. Separate
/// cursors are what make it answerable, and the chain is the observable proof that they are
/// separate.
#[test]
fn episodes_and_generations_only_ever_climb() {
    for seed in 0..64 {
        let mut roll = Roll(seed * 40_503 + 7);
        let mut pane = Pane::new();
        let mut weak = 0;
        let mut minted = Vec::new();
        for _ in 0..300 {
            let event = random_event(&mut roll, &mut weak);
            for line in pane.at(event) {
                if let Some(rest) = line.strip_prefix("mint tab=1 seat=SeatId(2) episode=") {
                    let mut fields = rest.split_whitespace();
                    let episode = fields.next().unwrap().parse::<u64>().unwrap();
                    let previous = fields
                        .find_map(|field| field.strip_prefix("prev="))
                        .unwrap();
                    let expected = minted
                        .last()
                        .map_or_else(|| "-".to_owned(), |last: &u64| last.to_string());
                    assert_eq!(previous, expected, "seed {seed}: the chain skipped");
                    if let Some(last) = minted.last() {
                        assert!(episode > *last, "seed {seed}: a number came back");
                    }
                    minted.push(episode);
                }
            }
        }
        assert!(
            !minted.is_empty(),
            "seed {seed} exercised no request at all"
        );
    }
}

/// PIN (invariant I4) — **the four states are a function of the fields and are stored nowhere.**
///
/// Asserted the only way it can be: over a random walk, recomputing the derivation independently
/// and comparing. Two producers each keeping a copy of "are we asking" is how the two would drift.
#[test]
fn the_state_is_only_ever_derived() {
    for seed in 0..32 {
        let mut roll = Roll(seed * 97 + 3);
        let mut pane = Pane::new();
        let mut weak = 0;
        for _ in 0..200 {
            let event = random_event(&mut roll, &mut weak);
            pane.at(event);
            let held = pane.ledger.ticket();
            match pane.state() {
                State::Idle | State::Acknowledged(_) => assert_eq!(held, None),
                State::Requested(_) => assert_eq!(held, None),
                State::Queued { ticket, .. } => assert_eq!(held, Some(ticket)),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The mapping table (§12.1 R1–R3)
// ---------------------------------------------------------------------------

/// Claude Code's `permission` kind as it is installed today: the zero-delay event, no identifier
/// quoted for it yet, and the three exits §11.4.2 gives it.
const CLAUDE_PERMISSION: &[MappingRow] = &[
    MappingRow {
        family: "claude-code",
        event: "PermissionRequest",
        kind: WaitKind::Permission,
        id: IdSource::None,
        action: MappedAction::Wait {
            tier: Tier::Primary,
        },
    },
    MappingRow {
        family: "claude-code",
        event: "PostToolUse",
        kind: WaitKind::Permission,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::Receipt,
            scope: ClearScope::ThisKind,
            reason: ClearReason::Hook,
            begins_turn: false,
        },
    },
    MappingRow {
        family: "claude-code",
        event: "UserPromptSubmit",
        kind: WaitKind::Permission,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::Boundary,
            scope: ClearScope::All,
            reason: ClearReason::Hook,
            begins_turn: true,
        },
    },
    MappingRow {
        family: "claude-code",
        event: "Stop",
        kind: WaitKind::Permission,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::Boundary,
            scope: ClearScope::All,
            reason: ClearReason::Hook,
            begins_turn: false,
        },
    },
];

/// codex's approval gate. Its payload carries a session and a turn but nothing that names one
/// approval, and a turn can hold several — so the kind is born without an identifier and there is
/// no upstream change that would give it one.
const CODEX_PERMISSION: &[MappingRow] = &[
    MappingRow {
        family: "codex",
        event: "hooks.permission-request",
        kind: WaitKind::Permission,
        id: IdSource::None,
        action: MappedAction::Wait {
            tier: Tier::Primary,
        },
    },
    MappingRow {
        family: "codex",
        event: "user-prompt-submit",
        kind: WaitKind::Permission,
        id: IdSource::None,
        action: MappedAction::Clear {
            class: ClearClass::Boundary,
            scope: ClearScope::All,
            reason: ClearReason::Hook,
            begins_turn: true,
        },
    },
];

/// PIN (R3) — **a kind is identified only when everything that must agree does, and the default is
/// not.**
///
/// A kind that is half keyed and half not produces both failure directions at once: a stale fixed
/// key swallowing the next real request, and two layers of one request minting two credentials.
/// So one undeclared row puts the whole kind on the level path, and the level path is what both
/// tables above are on today — which is the honest state of the evidence, not a limitation.
#[test]
fn a_kind_is_identified_only_when_every_row_declares_a_path() {
    assert_eq!(
        kind_mode(CLAUDE_PERMISSION, "claude-code", WaitKind::Permission),
        Mode::Level
    );
    assert_eq!(
        kind_mode(CODEX_PERMISSION, "codex", WaitKind::Permission),
        Mode::Level
    );

    // Quote both field paths and the kind moves over, receipts included.
    let identified = [
        MappingRow {
            id: IdSource::Path("tool_use_id"),
            ..CLAUDE_PERMISSION[0]
        },
        MappingRow {
            id: IdSource::Path("tool_use_id"),
            ..CLAUDE_PERMISSION[1]
        },
        CLAUDE_PERMISSION[2],
        CLAUDE_PERMISSION[3],
    ];
    assert_eq!(
        kind_mode(&identified, "claude-code", WaitKind::Permission),
        Mode::Id
    );

    // The wait declares one and the receipt does not: back to the level path, whole.
    let half = [
        MappingRow {
            id: IdSource::Path("tool_use_id"),
            ..CLAUDE_PERMISSION[0]
        },
        CLAUDE_PERMISSION[1],
        CLAUDE_PERMISSION[2],
        CLAUDE_PERMISSION[3],
    ];
    assert_eq!(
        kind_mode(&half, "claude-code", WaitKind::Permission),
        Mode::Level
    );

    // A kind with no wait row at all is not identified either — there is nothing to identify.
    assert_eq!(
        kind_mode(CLAUDE_PERMISSION, "claude-code", WaitKind::Quota),
        Mode::Level
    );
}

/// PIN (R2) — **one kind, one tier, in one installed configuration.**
///
/// The zero-delay event and the six-second notification describe the same request. Installing both
/// is how one request becomes two credentials — two dots, two interruptions — and no association
/// key can merge them, because nobody has ever shown that the two layers carry the same identifier.
/// The rule removes the problem at the source instead of trying to repair it downstream.
#[test]
fn a_kind_may_not_have_both_of_its_layers_installed() {
    assert_eq!(duplicated_tier(CLAUDE_PERMISSION), None);
    let mut both = CLAUDE_PERMISSION.to_vec();
    both.push(MappingRow {
        family: "claude-code",
        event: "Notification:permission_prompt",
        kind: WaitKind::Permission,
        id: IdSource::None,
        action: MappedAction::Wait {
            tier: Tier::Fallback,
        },
    });
    assert_eq!(
        duplicated_tier(&both),
        Some(("claude-code", WaitKind::Permission))
    );

    // Two families each with their own layer is not a clash; the rule is per family and kind.
    let mut apart = CLAUDE_PERMISSION.to_vec();
    apart.extend_from_slice(CODEX_PERMISSION);
    assert_eq!(duplicated_tier(&apart), None);
}

/// PIN (R4's other half) — **the mode decides the slot, and a row without a payload identifier
/// lands on the level even in an identified kind.**
///
/// The endpoint does not get to improvise. A malformed key is not "close enough" either: it falls
/// back to the level, where the watermark is still a correct answer.
#[test]
fn a_row_takes_the_slot_its_mode_and_payload_allow() {
    let row = CLAUDE_PERMISSION[0];
    assert_eq!(
        row.slot(Mode::Id, Some("toolu_01")),
        WaitSlot::Keyed {
            kind: WaitKind::Permission,
            key: "toolu_01".to_owned()
        }
    );
    assert_eq!(
        row.slot(Mode::Id, None),
        WaitSlot::Level(WaitKind::Permission)
    );
    assert_eq!(
        row.slot(Mode::Id, Some("has space")),
        WaitSlot::Level(WaitKind::Permission)
    );
    assert_eq!(
        row.slot(Mode::Level, Some("toolu_01")),
        WaitSlot::Level(WaitKind::Permission),
        "a level kind does not become keyed because one message happened to carry an id"
    );
}

/// PIN — **one pane holds a bounded number of outstanding credentials, and eviction takes the
/// oldest.**
///
/// The newest is the one that is actually happening. And on the level path the bound is structural
/// rather than enforced: four kinds, one slot each.
#[test]
fn a_pane_holds_a_bounded_number_of_waits_and_drops_the_oldest_first() {
    let mut pane = Pane::new();
    for index in 0..MAX_OUTSTANDING_WAITS {
        pane.at(keyed_wait(WaitKind::Permission, &format!("k{index}")));
    }
    let overflow = pane.at(keyed_wait(WaitKind::Permission, "k99"));
    assert!(
        overflow.iter().any(|line| line.contains("reason=overflow")),
        "{overflow:?}"
    );

    let mut level = Pane::new();
    for _ in 0..50 {
        for kind in [
            WaitKind::Permission,
            WaitKind::Elicitation,
            WaitKind::Agent,
            WaitKind::Quota,
        ] {
            level.at(strong_wait(kind));
        }
        level.at(Event::Answer(AnswerKind::Keyboard));
    }
    assert!(
        level
            .at(strong_wait(WaitKind::Permission))
            .iter()
            .all(|line| !line.contains("overflow")),
        "one kind is one slot, so the bound is out of reach"
    );
}

// ---------------------------------------------------------------------------
// The weak level, and the edge it becomes
// ---------------------------------------------------------------------------

/// PIN — **a polled level becomes at most one edge, and the translation lives next to the mirror
/// it compares against.**
#[test]
fn the_weak_level_turns_into_edges_exactly_once_each() {
    let mut pane = Pane::new();
    assert_eq!(pane.ledger.weak_edge(None), None);
    assert_eq!(pane.ledger.weak_edge(Some(1)), Some(Event::WeakYes(1)));
    pane.at(Event::WeakYes(1));
    assert_eq!(
        pane.ledger.weak_edge(Some(1)),
        None,
        "the level is unchanged"
    );
    assert_eq!(pane.ledger.weak_edge(None), Some(Event::WeakNo));
    assert_eq!(
        pane.ledger.weak_edge(Some(2)),
        Some(Event::WeakYes(2)),
        "a withdrawal and a fresh request inside one poll is still one edge"
    );
    pane.at(Event::WeakNo);
    assert_eq!(pane.ledger.weak_edge(None), None);
}
