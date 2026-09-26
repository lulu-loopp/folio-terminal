//! **The whole application leaving, as one transaction** (multiwindow slice E2,
//! user ruling 2026-08-20).
//!
//! `Ctrl+Shift+Q` is not "close every window in turn". Closing a window is the
//! *user* closing a window, and this product files such a window away in Recent
//! so it can be reopened (§2.5, slice D ruling ②). Quit is the *process*
//! leaving with everything it had open: every window goes into `session.json`
//! and **not one seed goes into the vault**, so the next launch opens what was
//! left rather than offering it back one row at a time.
//!
//! # Why this is a state machine and not a function
//!
//! Three of the four phases can *fail or be refused*, and the fourth cannot be
//! written straight-line at all:
//!
//! 1. **The gate** is a question, and a question is a pause — the answer arrives
//!    on a later event-loop turn.
//! 2. **The photograph** is read-only and takes every window in turn.
//! 3. **The write** either reaches the disk or does not, and a quit that could
//!    not write must not leave.
//! 4. **The retirement** hides the windows and then *keeps pumping the loop*
//!    until every hosted page's browser has let go. `event_loop.exit()` before
//!    that is what makes the browser-exit deadline a dead letter: nothing drives
//!    the clock it is hung on once the loop has stopped (審 #14).
//!
//! So the transaction is a value the loop advances, and the effects belong to
//! whoever holds the windows. This module owns the **order** and the **verdicts**
//! and nothing else, which is what makes both testable without a GPU.
//!
//! # Why not `exiting`
//!
//! `FolioApp::exiting` is winit's after-the-fact callback: it runs once the loop
//! has already stopped, and its body is
//! `for_each_window(|runtime| runtime.close_window(true))`. That is the wrong
//! machine for this in three separate ways, and each of them is a defect and not
//! a preference:
//!
//! * `for_each_window` **stops at the first failure** ("Answer for every open
//!   window in turn, oldest first, stopping at the first failure"), so one
//!   window whose child refused to die would leave the windows after it
//!   unphotographed.
//! * `close_window` **interleaves the photograph with the teardown** — it marks
//!   the session dirty, then closes controllers, then shuts children, per window
//!   — so window three is photographed after window one's browser has already
//!   been told to go. The ruling is that every window is photographed before
//!   *anything* is torn down.
//! * The loop has stopped, so nothing pumps: the browser-exit wait
//!   (`WebSeat::tick`) is never turned again and `WebOutcome::Gone` never
//!   arrives.
//!
//! `exiting` stays exactly as it is — it is the backstop for a loop stopped by
//! something that is *not* a quit and not a window closing, and for that job its
//! shape is right.

use std::time::{Duration, Instant};

/// How long the process will wait for the hosted pages to let go before it
/// leaves anyway.
///
/// **A bound and not a promise.** [`crate::webhost::BROWSER_EXIT_DEADLINE`] is
/// the per-seat backstop for a `BrowserProcessExited` that measured late in one
/// shutdown of eight and absent in another; this is the whole application's, and
/// it is longer than one seat's because several seats retire in parallel and the
/// last of them starts its own clock only once it has been told to close. A quit
/// that hung here would be a terminal that will not close, which is worse than a
/// browser process that outlives it by a second.
///
/// **Derived from the seat's bound rather than written down beside it** (§7.35).
/// It used to be a flat six seconds while the seat's was ten, so the sentence
/// above was false: the application always gave up four seconds *before* the
/// backstop it was waiting on could open, and every shut whose
/// `BrowserProcessExited` did not arrive left with the engine still holding the
/// window. Two seconds of slack, because what the application is waiting for is
/// the last seat's own door plus one turn of the loop to carry the answer.
pub const PAGE_TEARDOWN_DEADLINE: Duration =
    crate::webhost::BROWSER_EXIT_DEADLINE.saturating_add(Duration::from_secs(2));

/// What a press on the summary card answers.
///
/// Three and not two, which is the one way this card differs from the dirty gate
/// it is drawn like: the gate's affirmative answer *is* the discard, and here
/// there is a third thing a reader can honestly want — to keep the work and
/// still leave.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuitAnswer {
    /// Write every dirty buffer back, commit every open editor, and quit only if
    /// all of it reached the disk.
    Save,
    /// Leave without them. **Only in-memory modifications go** — nothing on disk
    /// is touched and no browsing history is emptied.
    Discard,
    /// Change nothing. **The default**, and Esc's answer, on the dirty gate's own
    /// standing rule: a question about losing work that took silence for consent
    /// would be the thing §7.1.3 exists to forbid.
    Cancel,
}

/// Something on the summary card the pointer can be over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuitTarget {
    /// The dialog itself, away from every button. A press here does nothing.
    Panel,
    Save,
    Discard,
    Cancel,
}

/// The answer Enter gives, which is the one that changes nothing.
pub const QUIT_FOCUSED_ANSWER: QuitAnswer = QuitAnswer::Cancel;

/// What the save branch actually managed, item by item (v3, 复审 ④-a).
///
/// **Both halves are carried, and that is the point.** A save that half worked
/// leaves the application in a state no single sentence describes: some buffers
/// are now clean and on disk, others are still dirty and named. Reporting only
/// the failures would let the window claim afterwards that nothing happened,
/// which is a claim only `Cancel` is entitled to make.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveReport {
    /// What reached the disk, or was committed. These are honestly clean now.
    pub saved: Vec<String>,
    /// What did not, by name, each with its own reason.
    pub failed: Vec<(String, String)>,
}

impl SaveReport {
    /// Whether every item the card named is now safe.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.failed.is_empty()
    }

    /// The names of what did not go through, in the order they were tried.
    #[must_use]
    pub fn failed_names(&self) -> Vec<String> {
        self.failed.iter().map(|(name, _)| name.clone()).collect()
    }
}

/// What the transaction owes its driver right now.
///
/// The driver performs exactly one of these and reports back; it never decides
/// what comes next. That is what makes "every window is photographed before
/// anything is torn down" a property of this file rather than a habit of a
/// seventy-thousand-line one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuitStep {
    /// Keep the card up and wait for one answer.
    Ask,
    /// Save everything the card named, then report with [`Quit::saved`].
    Save,
    /// Throw the in-memory modifications away — **and only those** — then report
    /// with [`Quit::discarded`].
    ///
    /// Its own step rather than something the answer does on its way past,
    /// because it is a change to every window and the answer arrives inside one:
    /// the same reason the whole transaction is a value the loop advances.
    Discard,
    /// Take every window's picture and assemble the document. **Read-only** —
    /// nothing is torn down here. Report with [`Quit::photographed`].
    Photograph,
    /// Put the document on the disk atomically, and say whether it landed
    /// ([`Quit::written`]).
    Write,
    /// Hide every window and tell every page and every child to go, then
    /// [`Quit::retired`].
    Retire,
    /// Keep pumping the loop and ask [`Quit::pages`] on every turn.
    WaitForPages,
    /// Stop the loop.
    Exit,
    /// This quit is over and the application is exactly as it was. Drop it.
    Abandon,
}

/// **What the [`QuitStep::Write`] step found, and therefore what happens next**
/// (T-QUIT-TIMEOUT-PROCEEDS, release review X-8).
///
/// A `bool` said "landed or not", and that read a save which ran out of its budget as a save
/// that was refused — so a disk that had stopped answering kept every window open in front of
/// somebody who had asked to leave, which is the one thing the deadline exists to prevent.
/// There are three answers, and only one of them keeps the windows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteVerdict {
    /// The document is on the disk.
    Landed,
    /// The save did not finish inside its budget, and the writer is still inside it. The last
    /// completed save is what a reader would find, this run's document may yet land on top of
    /// it, and the quit goes on either way — with `session.lock` left standing, because this run
    /// never saw the save finish.
    TimedOut,
    /// The disk refused it. Nothing landed and nothing is on its way.
    Refused,
}

impl WriteVerdict {
    /// Whether the quit carries on to [`QuitStep::Retire`].
    #[must_use]
    pub fn leaves(self) -> bool {
        matches!(self, Self::Landed | Self::TimedOut)
    }
}

/// Where a quit has got to, and what it is asking about.
#[derive(Clone, Debug, PartialEq)]
pub struct Quit {
    phase: Phase,
    /// What the card names, collected once when the question was put.
    ///
    /// **Once, and not re-derived per frame** like the dirty gate's list is. The
    /// gate is one window's and a `Runtime` can see its own pool every frame;
    /// this list spans every window and there is no borrow in this program that
    /// holds them all *and* a renderer. It is also the honest reading: the
    /// question was asked about what was dirty when it was asked.
    names: Vec<String>,
    hover: Option<QuitTarget>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Asking,
    Saving,
    Discarding,
    Photographing,
    Writing,
    Retiring,
    Waiting { until: Instant },
    Leaving,
    Abandoned,
}

impl Quit {
    /// Begin one. `names` is everything dirty across every window; an empty list
    /// is a quit with nothing to ask about.
    ///
    /// **No confirmation when there is nothing to lose.** "对可逆动作不加确认仪式"
    /// is true exactly here and nowhere else in this transaction: a quit that put
    /// up 「确定退出?」 over a clean application would be asking a question whose
    /// only honest answer is "nothing is lost either way".
    #[must_use]
    pub fn begin(names: Vec<String>) -> Self {
        let phase = if names.is_empty() {
            Phase::Photographing
        } else {
            Phase::Asking
        };
        Self {
            phase,
            names,
            hover: None,
        }
    }

    /// What the driver owes right now.
    #[must_use]
    pub fn step(&self) -> QuitStep {
        match self.phase {
            Phase::Asking => QuitStep::Ask,
            Phase::Saving => QuitStep::Save,
            Phase::Discarding => QuitStep::Discard,
            Phase::Photographing => QuitStep::Photograph,
            Phase::Writing => QuitStep::Write,
            Phase::Retiring => QuitStep::Retire,
            Phase::Waiting { .. } => QuitStep::WaitForPages,
            Phase::Leaving => QuitStep::Exit,
            Phase::Abandoned => QuitStep::Abandon,
        }
    }

    /// Whether the summary card is on screen.
    #[must_use]
    pub fn is_asking(&self) -> bool {
        self.phase == Phase::Asking
    }

    /// Whether the windows have been told to go.
    ///
    /// The one fact the drawing side needs from the tail of the transaction: past
    /// this point the windows are hidden and there is nothing to compose.
    #[must_use]
    pub fn is_retiring(&self) -> bool {
        matches!(self.phase, Phase::Waiting { .. } | Phase::Leaving)
    }

    /// **Whether the document has already landed, so that nothing this run does
    /// from here on is a change a reader made** (M3-4, `docs/DESIGN.md` §13.34).
    ///
    /// [`QuitStep::Photograph`] takes every window's picture and
    /// [`QuitStep::Write`] puts it on the disk; everything after that is the
    /// application taking itself apart. A pane whose child has gone closes its
    /// tab, a tab that was the last one closes its window, and each of those is
    /// an ordinary edit that would ordinarily be recorded — so without this
    /// question the teardown writes a *second* document over the one the
    /// transaction just wrote, describing the windows as they are on the way out
    /// rather than as the reader left them. Measured on the Mac 2026-09-12: a
    /// window standing with three tabs was written with three, and the file on
    /// the disk after the quit held none of them.
    ///
    /// It is `true` from [`Phase::Retiring`] on and never for
    /// [`Phase::Abandoned`]: a quit that could not write is a quit the
    /// application carries on past, and its session has everything still to say.
    ///
    /// A quit that retired on [`WriteVerdict::TimedOut`] answers `true` as any
    /// other retirement does, and that is the answer it wants: the teardown's
    /// second document would be handed to a writer already inside a write nobody
    /// can bound, over windows that are leaving rather than as the reader left
    /// them. What the disk holds is the last completed save, which is what the
    /// line in the log says.
    #[must_use]
    pub fn document_is_written(&self) -> bool {
        matches!(
            self.phase,
            Phase::Retiring | Phase::Waiting { .. } | Phase::Leaving
        )
    }

    /// What the card names.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }

    #[must_use]
    pub fn hover(&self) -> Option<QuitTarget> {
        self.hover
    }

    /// Report where the pointer is. Returns whether the answer changed.
    pub fn set_hover(&mut self, hover: Option<QuitTarget>) -> bool {
        let hover = hover.filter(|_| self.phase == Phase::Asking);
        if self.hover == hover {
            return false;
        }
        self.hover = hover;
        true
    }

    /// Spend the card's one answer.
    pub fn answer(&mut self, answer: QuitAnswer) -> QuitStep {
        if self.phase != Phase::Asking {
            return self.step();
        }
        self.hover = None;
        self.phase = match answer {
            QuitAnswer::Save => Phase::Saving,
            QuitAnswer::Discard => Phase::Discarding,
            QuitAnswer::Cancel => Phase::Abandoned,
        };
        self.step()
    }

    /// Report what the save branch managed.
    ///
    /// **All of it or none of the leaving** (v3): one refusal and the process
    /// stays, with its windows where they were and the failures named. The items
    /// that did go through are not un-saved to make the sentence tidier — they
    /// are on the disk, and only `Cancel` ever promised otherwise.
    pub fn saved(&mut self, report: &SaveReport) -> QuitStep {
        if self.phase != Phase::Saving {
            return self.step();
        }
        self.phase = if report.is_complete() {
            Phase::Photographing
        } else {
            Phase::Abandoned
        };
        self.step()
    }

    /// The in-memory modifications have been thrown away.
    pub fn discarded(&mut self) -> QuitStep {
        if self.phase != Phase::Discarding {
            return self.step();
        }
        self.phase = Phase::Photographing;
        self.step()
    }

    /// Every window has been photographed and the document is assembled.
    pub fn photographed(&mut self) -> QuitStep {
        if self.phase != Phase::Photographing {
            return self.step();
        }
        self.phase = Phase::Writing;
        self.step()
    }

    /// What became of the document.
    ///
    /// A refusal ends the quit here, before a single window has been hidden and
    /// before the sentinel is dropped: `session.lock` left in place is this
    /// process saying it did not reach a clean exit, and a quit that could not
    /// write the file did not.
    ///
    /// **A save that ran out of its budget is not a refusal** (release review
    /// X-8). The window is standing in front of somebody who asked to leave, the
    /// disk still holds the last completed save, and the document in hand may
    /// yet land on top of it — none of which is a reason to stay. So
    /// [`WriteVerdict::TimedOut`] retires exactly as a landing does, and the
    /// sentinel it leaves behind is not this transaction's to drop: the store
    /// keeps it precisely because nobody here saw the save finish.
    pub fn written(&mut self, verdict: WriteVerdict) -> QuitStep {
        if self.phase != Phase::Writing {
            return self.step();
        }
        self.phase = if verdict.leaves() {
            Phase::Retiring
        } else {
            Phase::Abandoned
        };
        self.step()
    }

    /// The windows are hidden and everything has been told to go.
    pub fn retired(&mut self, now: Instant) -> QuitStep {
        if self.phase != Phase::Retiring {
            return self.step();
        }
        self.phase = Phase::Waiting {
            until: now + PAGE_TEARDOWN_DEADLINE,
        };
        self.step()
    }

    /// Ask whether the wait is over: either every page has let go, or the bound
    /// has run out.
    pub fn pages(&mut self, all_gone: bool, now: Instant) -> QuitStep {
        let Phase::Waiting { until } = self.phase else {
            return self.step();
        };
        if all_gone {
            self.phase = Phase::Leaving;
        } else if now >= until {
            // Said out loud rather than swallowed: a browser that outlived the
            // bound is a fact about this machine, and the picture on disk is
            // already safe either way.
            eprintln!("BT_WEB quit left with a page still holding its browser process");
            self.phase = Phase::Leaving;
        }
        self.step()
    }

    /// When the loop has to come back and look at the clock, if it does.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        match self.phase {
            Phase::Waiting { until } => Some(until),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        vec!["a.txt".to_owned(), "b.md".to_owned()]
    }

    /// Walk a quit to the point where it would hide a window, reporting every
    /// step it asked for on the way.
    ///
    /// The witness for the ordering claims below: what a driver was *told* to do,
    /// in the order it was told, with no window and no GPU anywhere near it.
    fn walk(answer: Option<QuitAnswer>, verdict: WriteVerdict) -> Vec<QuitStep> {
        let mut quit = Quit::begin(if answer.is_some() {
            names()
        } else {
            Vec::new()
        });
        let mut seen = vec![quit.step()];
        if let Some(answer) = answer {
            seen.push(quit.answer(answer));
            if seen.last() == Some(&QuitStep::Save) {
                seen.push(quit.saved(&SaveReport {
                    saved: names(),
                    failed: Vec::new(),
                }));
            }
            if seen.last() == Some(&QuitStep::Discard) {
                seen.push(quit.discarded());
            }
        }
        if seen.last() == Some(&QuitStep::Photograph) {
            seen.push(quit.photographed());
        }
        if seen.last() == Some(&QuitStep::Write) {
            seen.push(quit.written(verdict));
        }
        seen
    }

    /// PIN (multiwindow slice E2) — **every window is photographed before
    /// anything at all is torn down.**
    ///
    /// Acceptance gate 2, said about the machine that decides it rather than
    /// about a comment in the loop. `Retire` is the only step that closes a
    /// controller or shuts a child, and there is no sequence of answers that
    /// reaches it without `Photograph` and a landed `Write` behind it.
    ///
    /// Red gate: let `answer(Discard)` go straight to `Phase::Retiring` — the
    /// shape `close_window` has, where the picture and the teardown are one call
    /// per window — and the walk below no longer has a `Photograph` in it.
    #[test]
    fn nothing_is_torn_down_until_every_window_has_been_photographed() {
        for answer in [Some(QuitAnswer::Save), Some(QuitAnswer::Discard), None] {
            let seen = walk(answer, WriteVerdict::Landed);
            let photograph = seen
                .iter()
                .position(|step| *step == QuitStep::Photograph)
                .expect("a quit that is going to leave photographs first");
            let write = seen
                .iter()
                .position(|step| *step == QuitStep::Write)
                .expect("and writes what it photographed");
            let retire = seen
                .iter()
                .position(|step| *step == QuitStep::Retire)
                .expect("and only then retires");
            assert!(
                photograph < write && write < retire,
                "photograph, write, retire — in that order and no other: {seen:?}"
            );
        }
    }

    /// PIN — **a clean application is not asked whether it is sure.**
    #[test]
    fn a_quit_with_nothing_dirty_puts_no_card_up() {
        let quit = Quit::begin(Vec::new());
        assert!(!quit.is_asking());
        assert_eq!(quit.step(), QuitStep::Photograph);
    }

    /// PIN — **cancel moves nothing.**
    ///
    /// Acceptance gate 1's first third: the card's third answer ends the
    /// transaction before a single picture is taken, which is what makes "一扇窗
    /// 都没动" true by construction rather than by inspection.
    #[test]
    fn cancelling_the_card_ends_the_quit_before_anything_is_read() {
        let mut quit = Quit::begin(names());
        assert_eq!(quit.step(), QuitStep::Ask);
        assert_eq!(quit.answer(QuitAnswer::Cancel), QuitStep::Abandon);
        assert_eq!(quit.deadline(), None);
    }

    /// PIN — **one refusal in the save branch stops the quit**, and the items
    /// that did go through stay saved.
    ///
    /// Acceptance gate 1's second third (v3 复审 ④-a). Red gate: report only the
    /// failures and drop `saved`, and the window has no way to say which half of
    /// the list is now clean.
    #[test]
    fn a_save_that_did_not_all_go_through_does_not_leave() {
        let mut quit = Quit::begin(names());
        assert_eq!(quit.answer(QuitAnswer::Save), QuitStep::Save);
        let report = SaveReport {
            saved: vec!["a.txt".to_owned()],
            failed: vec![("b.md".to_owned(), "the file moved on disk".to_owned())],
        };
        assert!(!report.is_complete());
        assert_eq!(report.failed_names(), vec!["b.md".to_owned()]);
        assert_eq!(quit.saved(&report), QuitStep::Abandon);
        assert_eq!(
            report.saved,
            vec!["a.txt".to_owned()],
            "what reached the disk is not un-saved to make the sentence tidier"
        );
    }

    /// PIN — **a write the disk refused does not leave.**
    ///
    /// Acceptance gate 1's last third and the whole of ③: the store's final flush
    /// is judged, and a quit that could not write the file keeps its windows. Red
    /// gate: make `written` ignore its argument and the process leaves having
    /// silently lost the session it was quitting to preserve.
    #[test]
    fn a_document_the_disk_refused_keeps_the_windows_open() {
        let seen = walk(Some(QuitAnswer::Discard), WriteVerdict::Refused);
        assert_eq!(seen.last(), Some(&QuitStep::Abandon));
        assert!(
            !seen.contains(&QuitStep::Retire),
            "not one window is hidden: {seen:?}"
        );
    }

    /// RED (T-QUIT-TIMEOUT-PROCEEDS, release review X-8) — **a save that ran out
    /// of its budget leaves.**
    ///
    /// The deadline on the session write buys back the window thread; X-8 is
    /// that it was then spent on the wrong answer. `wait_for` reports a budget
    /// that ran out, the old `written(landed.is_ok())` read that as "refused",
    /// and a disk that had stopped answering therefore kept every window open —
    /// the exact hang the deadline exists to end, three seconds later and with a
    /// card on it. The transaction now walks the whole write-to-retire
    /// transition on this answer.
    ///
    /// Red gate: fold `TimedOut` back in with `Refused` — give `leaves` a single
    /// `matches!(self, Self::Landed)` — and this goes red at `Retire`, naming the
    /// windows that stayed.
    #[test]
    fn a_save_that_ran_out_of_its_budget_still_leaves() {
        let seen = walk(Some(QuitAnswer::Discard), WriteVerdict::TimedOut);
        assert!(
            seen.contains(&QuitStep::Retire),
            "the windows are hidden even though nobody saw the save finish: {seen:?}"
        );
        assert!(
            !seen.contains(&QuitStep::Abandon),
            "and the quit is not abandoned: {seen:?}"
        );

        // And the rest of the ladder is the ordinary one: retire, wait for the
        // pages, leave.
        let mut quit = Quit::begin(Vec::new());
        let start = Instant::now();
        assert_eq!(quit.photographed(), QuitStep::Write);
        assert_eq!(quit.written(WriteVerdict::TimedOut), QuitStep::Retire);
        assert!(
            quit.document_is_written(),
            "the teardown does not hand a second document to a writer that is still inside one"
        );
        assert_eq!(quit.retired(start), QuitStep::WaitForPages);
        assert_eq!(quit.pages(true, start), QuitStep::Exit);
    }

    /// PIN (審 #14) — **the wait for the pages is a bounded wait that the loop
    /// actually turns.**
    ///
    /// The deadline exists to be reachable: the transaction hands the loop an
    /// instant to wake at, and the wait ends either when every page has gone or
    /// when that instant arrives. Red gate: return `None` from `deadline` and a
    /// process whose browser never says it exited waits for ever with its windows
    /// invisible.
    #[test]
    fn the_wait_for_the_pages_ends_either_way() {
        let start = Instant::now();
        let mut quit = Quit::begin(Vec::new());
        quit.photographed();
        quit.written(WriteVerdict::Landed);
        assert_eq!(quit.retired(start), QuitStep::WaitForPages);
        assert_eq!(quit.deadline(), Some(start + PAGE_TEARDOWN_DEADLINE));
        assert_eq!(quit.pages(false, start), QuitStep::WaitForPages);
        assert_eq!(
            quit.pages(false, start + PAGE_TEARDOWN_DEADLINE),
            QuitStep::Exit,
            "the bound is a bound"
        );

        let mut early = Quit::begin(Vec::new());
        early.photographed();
        early.written(WriteVerdict::Landed);
        early.retired(start);
        assert_eq!(
            early.pages(true, start),
            QuitStep::Exit,
            "and a page that has gone is not waited for"
        );
    }

    /// PIN — **the card is answered once.**
    ///
    /// A second press while the save branch is running must not restart the
    /// question, on `raise_dirty_gate`'s own rule read for a transaction: the
    /// answer is spent, and the verb below it is under way.
    #[test]
    fn a_second_answer_lands_on_a_question_that_has_been_spent() {
        let mut quit = Quit::begin(names());
        assert_eq!(quit.answer(QuitAnswer::Save), QuitStep::Save);
        assert_eq!(
            quit.answer(QuitAnswer::Cancel),
            QuitStep::Save,
            "the save is already running"
        );
        assert!(!quit.set_hover(Some(QuitTarget::Cancel)));
    }

    /// RED (M3-4) — **once the document has landed, the teardown has nothing to
    /// say about it.**
    ///
    /// Measured on the Mac 2026-09-12 and it is why this question exists: a
    /// window standing with three tabs was photographed with three, written with
    /// three, and the file on the disk after the quit held **one** — the
    /// unanswered restore row folded back, and not one live tab. What overwrote
    /// it was the teardown itself: `Retire` tells every child to go, the loop is
    /// still turning while it waits for the pages, each pane whose child has
    /// gone closes its tab, and every one of those closes is an ordinary edit
    /// that records the document again.
    ///
    /// Two claims, and the second is the one that keeps the application usable:
    /// the answer is `false` for every phase up to and including the write, and
    /// `false` again for a quit that **could not** write — that quit is
    /// abandoned, the application carries on, and its session still has
    /// everything to say.
    ///
    /// MUTATION: make it `true` from `Phase::Writing` and the document is frozen
    /// one step early, before the step that assembles it has run; make it `true`
    /// for `Abandoned` and an application that failed to quit stops recording
    /// anything for the rest of the run.
    #[test]
    fn once_the_document_has_landed_the_teardown_cannot_write_over_it() {
        let mut quit = Quit::begin(names());
        assert!(!quit.document_is_written(), "with the card up");
        assert_eq!(quit.answer(QuitAnswer::Discard), QuitStep::Discard);
        assert!(!quit.document_is_written(), "while the changes are dropped");
        assert_eq!(quit.discarded(), QuitStep::Photograph);
        assert!(!quit.document_is_written(), "while the pictures are taken");
        assert_eq!(quit.photographed(), QuitStep::Write);
        assert!(!quit.document_is_written(), "while it is being written");
        assert_eq!(quit.written(WriteVerdict::Landed), QuitStep::Retire);
        assert!(quit.document_is_written(), "the moment it has landed");
        assert_eq!(quit.retired(Instant::now()), QuitStep::WaitForPages);
        assert!(quit.document_is_written(), "while the pages go");
        let mut abandoned = Quit::begin(Vec::new());
        assert_eq!(abandoned.photographed(), QuitStep::Write);
        assert_eq!(abandoned.written(WriteVerdict::Refused), QuitStep::Abandon);
        assert!(
            !abandoned.document_is_written(),
            "a quit that could not write leaves an application with everything still to say"
        );
    }

    /// Walk `quit` from `step` as `FolioApp::settle_quit` walks it, writing the window thread's
    /// phase where its arms write it — `exiting()` at the head of `Write`, `quit_abandoned()` at
    /// the head of `Abandon` — and answer the steps and the phase after each.
    ///
    /// `settle_quit` itself cannot run in a test: it takes winit's `ActiveEventLoop`, which exists
    /// only inside a running loop. What this drives is the real transaction, every step of it,
    /// against the real phase writers; the two writer calls are the two statements of
    /// `settle_quit` it stands in for.
    fn walk_with_the_phase(
        quit: &mut Quit,
        mut step: QuitStep,
        verdict: WriteVerdict,
    ) -> Vec<(QuitStep, Option<bt_platform::admission::Phase>)> {
        use bt_platform::admission;
        let mut seen = Vec::new();
        loop {
            match step {
                QuitStep::Write => assert!(admission::exiting(), "the write is on the way out"),
                QuitStep::Abandon => assert!(
                    admission::quit_abandoned(),
                    "giving a quit up is never a violation"
                ),
                _ => {}
            }
            seen.push((step, admission::phase()));
            step = match step {
                QuitStep::Save => quit.saved(&SaveReport {
                    saved: vec!["a.txt".to_owned()],
                    failed: vec![("b.md".to_owned(), "the disk is full".to_owned())],
                }),
                QuitStep::Discard => quit.discarded(),
                QuitStep::Photograph => quit.photographed(),
                QuitStep::Write => quit.written(verdict),
                QuitStep::Retire => quit.retired(Instant::now()),
                QuitStep::WaitForPages => quit.pages(true, Instant::now()),
                QuitStep::Ask | QuitStep::Exit | QuitStep::Abandon => return seen,
            };
        }
    }

    /// Run `body` on a thread of its own that has entered the window thread and taken the loop's
    /// first turn.
    fn on_a_running_window_thread(body: impl FnOnce() + Send) {
        std::thread::scope(|threads| {
            let answer = threads
                .spawn(|| {
                    assert!(bt_platform::admission::enter_window_thread());
                    assert!(bt_platform::admission::loop_running());
                    body();
                })
                .join();
            if let Err(panic) = answer {
                std::panic::resume_unwind(panic);
            }
        });
    }

    /// RED (A1a, revision (c)5) — **the window thread's phase follows the real quit: a quit
    /// cancelled at its card or whose saves did not all land never leaves `Running`; one whose
    /// write was refused goes to `Exiting` and comes back; one that retires stays `Exiting`, where
    /// the exit doors are admitted.**
    ///
    /// Cancel and an incomplete save reach `Abandon` without passing `Write`. If giving a quit up
    /// were admitted only from `Exiting`, every Cancel would count a violation for an ordinary
    /// gesture; if the write did not turn the phase, the exit doors (§5.3 rows 15–17) would be
    /// refused on the way out.
    ///
    /// MUTATION: admit `quit_abandoned` only from `Exiting` (`bt_platform::admission`) and the
    /// Cancel walk goes red.
    #[test]
    fn the_window_threads_phase_follows_the_real_quit() {
        use bt_platform::admission::{Phase, admitted, doors};
        on_a_running_window_thread(|| {
            // Cancel at the card.
            let mut quit = Quit::begin(names());
            let first = quit.answer(QuitAnswer::Cancel);
            assert_eq!(
                walk_with_the_phase(&mut quit, first, WriteVerdict::Landed),
                vec![(QuitStep::Abandon, Some(Phase::Running))]
            );
            // A save that did not all land.
            let mut quit = Quit::begin(names());
            let first = quit.answer(QuitAnswer::Save);
            assert_eq!(
                walk_with_the_phase(&mut quit, first, WriteVerdict::Landed),
                vec![
                    (QuitStep::Save, Some(Phase::Running)),
                    (QuitStep::Abandon, Some(Phase::Running)),
                ]
            );
            // A write the store refused.
            let mut quit = Quit::begin(Vec::new());
            assert_eq!(
                walk_with_the_phase(&mut quit, QuitStep::Photograph, WriteVerdict::Refused),
                vec![
                    (QuitStep::Photograph, Some(Phase::Running)),
                    (QuitStep::Write, Some(Phase::Exiting)),
                    (QuitStep::Abandon, Some(Phase::Running)),
                ]
            );
            // A quit that goes.
            let mut quit = Quit::begin(Vec::new());
            assert_eq!(
                walk_with_the_phase(&mut quit, QuitStep::Photograph, WriteVerdict::Landed),
                vec![
                    (QuitStep::Photograph, Some(Phase::Running)),
                    (QuitStep::Write, Some(Phase::Exiting)),
                    (QuitStep::Retire, Some(Phase::Exiting)),
                    (QuitStep::WaitForPages, Some(Phase::Exiting)),
                    (QuitStep::Exit, Some(Phase::Exiting)),
                ]
            );
            assert_eq!(
                admitted::<doors::PaneRetirementWait, _>(|_token| ()),
                Ok(())
            );
            assert_eq!(admitted::<doors::SessionWriteWait, _>(|_token| ()), Ok(()));
            assert_eq!(
                admitted::<doors::SessionWriterRetire, _>(|_token| ()),
                Ok(())
            );
            assert_eq!(admitted::<doors::TraceFlush, _>(|_token| ()), Ok(()));
            // `App::finish` on that road finds the thread already leaving.
            assert!(bt_platform::admission::exiting());
        });
    }

    /// RED (A1a, revisions (b)1 and (c)5) — **a loop that could not be built still leaves through
    /// the trace flush.**
    ///
    /// `fn main`'s event-loop build error arm calls `exiting()` before it returns, from
    /// `Starting`, so the trace guard's drop reaches row 17's flush admitted.
    ///
    /// MUTATION: refuse `Starting` in `admission::exiting` and the flush is refused.
    #[test]
    fn a_loop_that_could_not_be_built_still_flushes_the_trace() {
        use bt_platform::admission::{Phase, admitted, doors};
        std::thread::scope(|threads| {
            let answer = threads
                .spawn(|| {
                    assert!(bt_platform::admission::enter_window_thread());
                    assert_eq!(bt_platform::admission::phase(), Some(Phase::Starting));
                    assert!(bt_platform::admission::exiting());
                    assert_eq!(admitted::<doors::TraceFlush, _>(|_token| ()), Ok(()));
                })
                .join();
            if let Err(panic) = answer {
                std::panic::resume_unwind(panic);
            }
        });
    }
}
