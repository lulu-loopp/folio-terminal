//! **A put-away run that works holds an activity, so that macOS does not nap it** (0.4.10 plan
//! row 87, part 87c; T-TRAY-87C-MACOS; the coordinator's R14 of 2026-11-12, item 19 of 2026-11-14
//! and R6-R8 of 2026-11-20).
//!
//! App Nap slows an application none of whose windows is on the glass. A run whose last window is
//! put away is that application, and its shells and agents are still writing into their PTYs:
//! napped, the drains that read them run late, and a writer faster than a late drain blocks on a
//! full pipe. So the run holds one activity (`bt_platform::process_activity`) **while a window is
//! put away and work runs** — an agent's turn under way, or a command an integrated shell says is
//! running: the one reader of running work, [`crate::running_work`], which the quit's question
//! reads too (item 19). A put-away run with nothing running holds none: it has nothing to be late
//! for, and App Nap is the system's to give it.
//!
//! **Only while put away** (R7): a shown window that is covered, minimised or hidden with `Cmd+H`
//! is left to App Nap as before this part; the hand check measures that state beside the put-away
//! one, and a later row widens this on a measured stall only.
//!
//! **Compare and record, once a turn** (`FolioApp::settle_the_put_away_activity`, after every
//! window's turn, beside the run's icon): begun on the turn it is first wanted, let go on the turn
//! it is no longer — the window brought back, or the work ended — and **let go for good** by
//! `App::finish`, after which no turn begins it again (the loop's last turns run after the exit was
//! asked, as the icon's do).

/// **Whether the run wants the activity**: a window is put away, and work runs.
#[must_use]
pub(crate) const fn the_activity_is_wanted(
    a_window_is_put_away: bool,
    work: crate::running_work::RunningWork,
) -> bool {
    a_window_is_put_away && work.any()
}

/// **The activity the run holds, if any, and whether it has ended for good.**
pub(crate) struct ActivityStand<A> {
    held: Option<A>,
    ended_for_good: bool,
}

impl<A> Default for ActivityStand<A> {
    fn default() -> Self {
        Self {
            held: None,
            ended_for_good: false,
        }
    }
}

impl<A> ActivityStand<A> {
    /// **This turn's answer, compared with the last one's**: `begin` is asked only on the turn
    /// the activity is first wanted, and the held one dropped (ended) on the turn it is not. A
    /// refused beginning is said through `note` and asked again on the next turn that wants it.
    pub(crate) fn answer_the_turn(
        &mut self,
        wanted: bool,
        begin: impl FnOnce() -> Result<A, String>,
        note: impl FnOnce(&str),
    ) {
        if !wanted || self.ended_for_good {
            self.held = None;
            return;
        }
        if self.held.is_some() {
            return;
        }
        match begin() {
            Ok(activity) => self.held = Some(activity),
            Err(error) => note(&format!("BT_PUT_AWAY the activity: {error}")),
        }
    }

    /// **Let it go, and begin none again** — `App::finish`, after the run's last write.
    pub(crate) fn let_go_for_good(&mut self) {
        self.held = None;
        self.ended_for_good = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::running_work::RunningWork;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// An activity that says when it ends.
    struct Told(Rc<RefCell<Vec<&'static str>>>);

    impl Drop for Told {
        fn drop(&mut self) {
            self.0.borrow_mut().push("ended");
        }
    }

    const WORK: RunningWork = RunningWork {
        agents: 1,
        commands: 0,
    };

    /// RED (R7 of 2026-11-20) — **wanted only while a window is put away and work runs**: a
    /// put-away run with nothing running, and a shown run with work, want none.
    ///
    /// PLANT 87c-P55: drop the put-away term from `the_activity_is_wanted` — a shown run that
    /// works wants it.
    #[test]
    fn the_activity_is_wanted_only_while_put_away_with_work() {
        assert!(the_activity_is_wanted(true, WORK));
        assert!(!the_activity_is_wanted(true, RunningWork::NONE));
        assert!(!the_activity_is_wanted(false, WORK));
        assert!(!the_activity_is_wanted(false, RunningWork::NONE));
    }

    /// RED — **begun once while wanted, ended once when not, and never again after the end for
    /// good**; a refused beginning is said and asked again on the next turn.
    #[test]
    fn the_stand_begins_once_ends_once_and_stays_ended_for_good() {
        let told = Rc::new(RefCell::new(Vec::new()));
        let begin = |told: &Rc<RefCell<Vec<&'static str>>>| {
            told.borrow_mut().push("begun");
            Ok(Told(Rc::clone(told)))
        };
        let mut stand = ActivityStand::default();
        stand.answer_the_turn(true, || begin(&told), |_| panic!("nothing refused"));
        stand.answer_the_turn(true, || begin(&told), |_| panic!("nothing refused"));
        assert!(stand.held.is_some());
        stand.answer_the_turn(false, || begin(&told), |_| panic!("nothing refused"));
        assert!(stand.held.is_none());
        assert_eq!(*told.borrow(), ["begun", "ended"]);
        let mut said = Vec::new();
        stand.answer_the_turn(
            true,
            || Err("refused".to_owned()),
            |line| said.push(line.to_owned()),
        );
        assert!(stand.held.is_none(), "a refusal holds nothing");
        assert_eq!(said, ["BT_PUT_AWAY the activity: refused"]);
        stand.answer_the_turn(true, || begin(&told), |_| panic!("nothing refused"));
        assert!(stand.held.is_some(), "and the next turn asks again");
        stand.let_go_for_good();
        stand.answer_the_turn(true, || begin(&told), |_| panic!("nothing refused"));
        assert!(
            stand.held.is_none(),
            "nothing begins after the end for good"
        );
        assert_eq!(*told.borrow(), ["begun", "ended", "begun", "ended"]);
    }
}
