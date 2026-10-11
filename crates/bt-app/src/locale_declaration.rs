//! **The locale a new pane is told** (`LANG`, `LC_CTYPE`), as the system last said it
//! (T-OS-SIGNALS-MAC; B5 design note §3.3, revisions (b)11, (c)4 and (c)5).
//!
//! The answer used to be a `OnceLock` filled by two child processes on the window thread when the
//! first pane was born (window-waits row 7), and a region changed in System Settings never reached
//! a new pane until Folio restarted. Now it is a cell with one writer, its probe worker:
//!
//! * **asked** at launch ([`LocaleTrigger::Launch`]), when the system says the locale changed
//!   ([`LocaleTrigger::LocaleChanged`]), and by a pane's birth when the newest reading failed and
//!   none is out ([`LocaleTrigger::Birth`]) — one probe at a time, requests made meanwhile merged
//!   into the one after it (`attention_copilot::CopilotProbe`'s shape);
//! * **read** by a birth without waiting ([`held`]): the newest reading that completed, and while
//!   none has, the newest reading's own choice — what the machine said before it failed, which is
//!   today's answer for a reading that fails;
//! * **waited for** only by a cold first pane, in birth ([`at_birth`] answers
//!   [`AtBirth::Waiting`] while no reading has finished at all): owner Q5 keeps that pane's
//!   outcome, and the reading's one ten-second budget bounds the wait.
//!
//! A changed locale reaches **new panes only** (owner Q4): a running shell keeps the environment
//! it was born with, because nothing here writes to one.

use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock, PoisonError};

use bt_platform::admission::WorkerCtx;
use bt_platform::{LocaleDeclaration, LocaleReading};

/// **Why a reading was asked for** — said in `diagnostics.log`, and nothing decides on it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocaleTrigger {
    /// The application started (`Runtime::create`).
    Launch,
    /// The system said the user's locale changed (`SystemNews::Locale`).
    LocaleChanged,
    /// A pane's birth found the newest reading failed, or none asked for, and none out.
    Birth,
}

impl LocaleTrigger {
    const fn name(self) -> &'static str {
        match self {
            Self::Launch => "launch",
            Self::LocaleChanged => "the system's locale changed",
            Self::Birth => "a pane's birth",
        }
    }
}

/// **What a pane's birth may do about its locale** ([`LocaleCell::at_birth`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AtBirth {
    /// Be born now with [`held`]'s declaration.
    Ready,
    /// Wait in birth: no reading has finished yet, so there is nothing to be born with.
    Waiting,
}

/// The requests, the reading out, and the answers.
#[derive(Default)]
struct CellState {
    /// The number of the newest request; counted from `1`.
    requested: u64,
    /// The newest request not yet started: its number and why.
    waiting: Option<(u64, LocaleTrigger)>,
    /// The reading out now: its number, why, and how many requests were made while it was out.
    out: Option<(u64, LocaleTrigger, u64)>,
    /// A worker is running (started by a request, gone when nothing waits).
    worker: bool,
    /// The newest reading that completed, by its request's number.
    last_success: Option<(u64, Option<LocaleDeclaration>)>,
    /// The newest reading of all, a failed one included, by its request's number.
    newest: Option<(u64, LocaleReading)>,
}

impl CellState {
    /// Whether a reading is out or waiting to start: a request made now would only be merged.
    const fn asked(&self) -> bool {
        self.out.is_some() || self.waiting.is_some()
    }

    /// **File a reading unless a newer one is held** — the rule that keeps a slow reading from
    /// putting back what a later one replaced, for the newest reading and for the newest success.
    fn publish(&mut self, generation: u64, reading: LocaleReading) {
        if reading.failure.is_none()
            && self
                .last_success
                .as_ref()
                .is_none_or(|(held, _)| *held < generation)
        {
            self.last_success = Some((generation, reading.declaration.clone()));
        }
        if self
            .newest
            .as_ref()
            .is_none_or(|(held, _)| *held < generation)
        {
            self.newest = Some((generation, reading));
        }
    }

    /// What a birth is told: the newest completed reading's declaration, and while none has
    /// completed, the newest reading's own choice (`None` before any).
    fn held(&self) -> Option<LocaleDeclaration> {
        match (&self.last_success, &self.newest) {
            (Some((_, declaration)), _) => declaration.clone(),
            (None, Some((_, reading))) => reading.declaration.clone(),
            (None, None) => None,
        }
    }
}

/// What a reading runs: read the system's locale, on the cell's worker.
type ReadLocale = dyn Fn(&WorkerCtx) -> LocaleReading + Send + Sync;

/// **The locale declaration cell** — a type rather than statics so a test runs one with a
/// reading it writes.
pub(crate) struct LocaleCell {
    /// Whether this platform declares a locale at all (`bt_platform::system_locale_is_declared`):
    /// where it does not, nothing is ever read and no birth waits.
    declared: bool,
    state: Mutex<CellState>,
    run: Box<ReadLocale>,
    wake: OnceLock<Box<dyn Fn() + Send + Sync>>,
    note: Box<dyn Fn(&str) + Send + Sync>,
}

impl LocaleCell {
    /// A cell that reads through `run` when `declared`, saying its diagnostics lines through
    /// `note`.
    pub(crate) fn new(
        declared: bool,
        run: impl Fn(&WorkerCtx) -> LocaleReading + Send + Sync + 'static,
        note: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        Self {
            declared,
            state: Mutex::new(CellState::default()),
            run: Box::new(run),
            wake: OnceLock::new(),
            note: Box::new(note),
        }
    }

    /// Teach the worker how to bring the event loop round. Once; a second call is ignored.
    pub(crate) fn install_wake(&self, wake: impl Fn() + Send + Sync + 'static) {
        let _ = self.wake.set(Box::new(wake));
    }

    /// **Number a request and see that a reading answers it.** Never waits for one: a lock, and —
    /// when no worker is running — a thread start. A request made while a reading is out replaces
    /// any request still waiting; the reading after the one out answers them all. Nothing on a
    /// platform that declares no locale.
    pub(crate) fn ask(&'static self, trigger: LocaleTrigger) {
        if !self.declared {
            return;
        }
        let state = self.lock();
        self.ask_locked(state, trigger);
    }

    /// [`Self::ask`] with the lock already held; lets it go before any line is said or thread
    /// started.
    fn ask_locked(&'static self, mut state: MutexGuard<'_, CellState>, trigger: LocaleTrigger) {
        state.requested += 1;
        let generation = state.requested;
        let replaced = state.waiting.replace((generation, trigger));
        if let Some((_, _, joined)) = state.out.as_mut() {
            *joined += 1;
        }
        let start_a_worker = !state.worker;
        state.worker = true;
        drop(state);
        // Said after the lock is let go: the line is a file write.
        if let Some((older, older_trigger)) = replaced {
            (self.note)(&format!(
                "locale reading request {older} ({}) was replaced before it started by request \
                 {generation} ({}), which answers both",
                older_trigger.name(),
                trigger.name()
            ));
        }
        if !start_a_worker {
            return;
        }
        // In the workers' band: two short child processes, never the reason a frame was late.
        let started = bt_platform::spawn_at_priority(
            "locale-reading",
            bt_platform::ThreadPriority::BelowNormal,
            move |ctx| self.serve(ctx),
        );
        if let Err(error) = started {
            // Answered as a failed reading, so a pane waiting for the first one is born (with
            // nothing to be told), and the next birth asks again.
            let reason = format!("its worker would not start ({error})");
            {
                let mut state = self.lock();
                state.worker = false;
                if let Some((waiting, _)) = state.waiting.take() {
                    state.publish(waiting, failed(&reason));
                }
            }
            (self.note)(&format!("locale reading request {generation}: {reason}"));
            self.wake();
        }
    }

    /// **What a pane's birth may do about its locale**, and the asking a birth owes: a birth that
    /// finds the newest reading failed and none out asks again — whether or not an older success
    /// is held (revision (c)5) — and is born now with [`Self::held`]; a birth that finds no
    /// reading finished at all waits, asking first if nobody has.
    pub(crate) fn at_birth(&'static self) -> AtBirth {
        if !self.declared {
            return AtBirth::Ready;
        }
        let state = self.lock();
        let (answer, ask, line) = match &state.newest {
            None => (AtBirth::Waiting, !state.asked(), None),
            Some((generation, reading)) if reading.failure.is_some() && !state.asked() => (
                AtBirth::Ready,
                true,
                Some(format!(
                    "locale reading {generation} did not complete ({}); asked again by a pane's \
                     birth",
                    reading.failure.as_deref().unwrap_or_default()
                )),
            ),
            Some(_) => (AtBirth::Ready, false, None),
        };
        if ask {
            self.ask_locked(state, LocaleTrigger::Birth);
        } else {
            drop(state);
        }
        if let Some(line) = line {
            (self.note)(&line);
        }
        answer
    }

    /// **What a new pane is told**: the newest reading that completed, and while none has, the
    /// newest reading's own choice. Never waits: a lock.
    pub(crate) fn held(&self) -> Option<LocaleDeclaration> {
        self.lock().held()
    }

    /// Whether a pane waiting in birth for its locale may be born: some reading has finished,
    /// or this platform declares no locale.
    pub(crate) fn settled(&self) -> bool {
        !self.declared || self.lock().newest.is_some()
    }

    /// **The worker's whole body**: take the newest request, read, publish, and go when nothing
    /// is waiting.
    fn serve(&self, ctx: &WorkerCtx) {
        loop {
            let (generation, trigger) = {
                let mut state = self.lock();
                let Some((generation, trigger)) = state.waiting.take() else {
                    state.worker = false;
                    return;
                };
                state.out = Some((generation, trigger, 0));
                (generation, trigger)
            };
            // A reading that unwinds leaves the cell askable and a waiting birth unblocked.
            let guard = ReadingOut {
                cell: self,
                generation,
            };
            let reading = (self.run)(ctx);
            std::mem::forget(guard);
            let failure = reading.failure.clone();
            let joined = {
                let mut state = self.lock();
                let joined = state.out.take().map_or(0, |(_, _, joined)| joined);
                state.publish(generation, reading);
                joined
            };
            if let Some(failure) = failure {
                (self.note)(&format!(
                    "locale reading {generation} ({}) did not complete: {failure}",
                    trigger.name()
                ));
            }
            if joined > 0 {
                (self.note)(&format!(
                    "locale reading {generation} ({}): {joined} request(s) made while it was out \
                     are answered by the next reading",
                    trigger.name()
                ));
            }
            // After the answer is published, never before: a wake that raced the publication
            // would send the loop to read an answer that is still missing.
            self.wake();
        }
    }

    fn wake(&self) {
        if let Some(wake) = self.wake.get() {
            wake();
        }
    }

    fn lock(&self) -> MutexGuard<'_, CellState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A reading that did not happen, for `reason`.
fn failed(reason: &str) -> LocaleReading {
    LocaleReading {
        declaration: None,
        failure: Some(reason.to_owned()),
    }
}

/// **A reading on its way**: dropped only by an unwind (a finished reading forgets it), when it
/// files a failed reading — so a pane waiting for the first one is born — and marks the worker
/// gone so the next request starts another.
struct ReadingOut<'a> {
    cell: &'a LocaleCell,
    generation: u64,
}

impl Drop for ReadingOut<'_> {
    fn drop(&mut self) {
        {
            let mut state = self.cell.lock();
            state.worker = false;
            state.out = None;
            state.publish(
                self.generation,
                failed("the reading ended before its answer"),
            );
        }
        (self.cell.note)(&format!(
            "locale reading {} ended before its answer; the next pane's birth asks again",
            self.generation
        ));
        self.cell.wake();
    }
}

/// The product's cell, reading this machine and writing its lines to `diagnostics.log`.
static CELL: LazyLock<LocaleCell> = LazyLock::new(|| {
    LocaleCell::new(
        bt_platform::system_locale_is_declared(),
        bt_platform::read_system_locale_declaration,
        crate::diagnostics::note,
    )
});

/// Teach the cell how to bring the event loop round when a reading lands: a pane waiting in birth
/// for the first one is born on that turn.
pub(crate) fn install_wake(wake: impl Fn() + Send + Sync + 'static) {
    CELL.install_wake(wake);
}

/// **Ask for a reading** ([`LocaleCell::ask`] on the product's cell).
pub(crate) fn ask(trigger: LocaleTrigger) {
    CELL.ask(trigger);
}

/// **What a pane's birth may do about its locale** ([`LocaleCell::at_birth`]).
pub(crate) fn at_birth() -> AtBirth {
    CELL.at_birth()
}

/// **What a new pane is told** ([`LocaleCell::held`]). Never waits.
#[must_use]
pub(crate) fn held() -> Option<LocaleDeclaration> {
    CELL.held()
}

/// Whether a pane waiting in birth for its locale may be born ([`LocaleCell::settled`]).
#[must_use]
pub(crate) fn settled() -> bool {
    CELL.settled()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Condvar, mpsc};

    /// **A system a test writes**: what each reading answers, in order, a gate that holds a
    /// reading inside its run until the test lets it go, and how many readings ran and how many
    /// ran at once.
    #[derive(Clone, Default)]
    struct FakeSystem {
        answers: Arc<Mutex<Vec<LocaleReading>>>,
        runs: Arc<Mutex<u64>>,
        running: Arc<Mutex<(u64, u64)>>,
        gate: Arc<(Mutex<bool>, Condvar)>,
    }

    impl FakeSystem {
        fn read(&self) -> LocaleReading {
            {
                let mut running = self.running.lock().unwrap();
                running.0 += 1;
                running.1 = running.1.max(running.0);
            }
            let (held, released) = &*self.gate;
            let mut held = held.lock().unwrap();
            while *held {
                held = released.wait(held).unwrap();
            }
            drop(held);
            *self.runs.lock().unwrap() += 1;
            self.running.lock().unwrap().0 -= 1;
            let mut answers = self.answers.lock().unwrap();
            if answers.len() > 1 {
                answers.remove(0)
            } else {
                answers[0].clone()
            }
        }

        fn answering(answers: Vec<LocaleReading>) -> Self {
            let system = Self::default();
            *system.answers.lock().unwrap() = answers;
            system
        }

        fn hold(&self) {
            *self.gate.0.lock().unwrap() = true;
        }

        fn release(&self) {
            *self.gate.0.lock().unwrap() = false;
            self.gate.1.notify_all();
        }

        fn runs(&self) -> u64 {
            *self.runs.lock().unwrap()
        }

        /// The most readings that ran at once.
        fn most_at_once(&self) -> u64 {
            self.running.lock().unwrap().1
        }
    }

    /// A cell of the test's own over `system`, leaked for the `'static` its worker needs; its
    /// wakes and its diagnostics lines come back on channels.
    fn cell_over(
        system: &FakeSystem,
        declared: bool,
    ) -> (
        &'static LocaleCell,
        mpsc::Receiver<()>,
        mpsc::Receiver<String>,
    ) {
        let (noted, notes) = mpsc::channel();
        let noted = Mutex::new(noted);
        let asked = system.clone();
        let cell: &'static LocaleCell = Box::leak(Box::new(LocaleCell::new(
            declared,
            move |_ctx| asked.read(),
            move |line: &str| {
                let _ = noted.lock().unwrap().send(line.to_owned());
            },
        )));
        let (woke, wakes) = mpsc::channel();
        let woke = Mutex::new(woke);
        cell.install_wake(move || {
            let _ = woke.lock().unwrap().send(());
        });
        (cell, wakes, notes)
    }

    fn said(locale: &str) -> LocaleReading {
        LocaleReading {
            declaration: Some(LocaleDeclaration::lang(locale)),
            failure: None,
        }
    }

    fn failed_with(declaration: Option<LocaleDeclaration>, why: &str) -> LocaleReading {
        LocaleReading {
            declaration,
            failure: Some(why.to_owned()),
        }
    }

    /// Wait until no reading is out or waiting and the worker has gone.
    fn until_idle(cell: &LocaleCell) {
        while {
            let state = cell.lock();
            state.asked() || state.worker
        } {
            std::thread::yield_now();
        }
    }

    /// RED (T-OS-SIGNALS-MAC) — **a changed locale is read again, and reaches the panes born
    /// after it** (owner Q4): what a pane was told stays what it was told; the next pane is told
    /// the new locale. The answer used to be a `OnceLock`.
    ///
    /// MUTATION: `publish` keeping the first success it filed (`last_success` written only when
    /// `None`) — the pane born after the change is told the old locale.
    #[test]
    fn a_changed_locale_is_read_again_and_reaches_new_panes_only() {
        let system = FakeSystem::answering(vec![said("ja_JP.UTF-8"), said("zh_CN.UTF-8")]);
        let (cell, wakes, _) = cell_over(&system, true);
        cell.ask(LocaleTrigger::Launch);
        crate::lane::wait_for_a_wake(&wakes, "the launch's reading");
        let born_before = cell.held();
        assert_eq!(born_before, Some(LocaleDeclaration::lang("ja_JP.UTF-8")));

        cell.ask(LocaleTrigger::LocaleChanged);
        crate::lane::wait_for_a_wake(&wakes, "the reading the change asked for");
        assert_eq!(
            cell.held(),
            Some(LocaleDeclaration::lang("zh_CN.UTF-8")),
            "a pane born after the change is told the new locale"
        );
        assert_eq!(
            born_before,
            Some(LocaleDeclaration::lang("ja_JP.UTF-8")),
            "and one born before keeps what it was told"
        );
        assert_eq!(system.runs(), 2);
    }

    /// RED (T-OS-SIGNALS-MAC) — **an older reading never lands over a newer one**, for the
    /// newest reading and for the newest success alike (revision (b)10, E-3's stale ordering).
    ///
    /// MUTATION: `publish` filing every reading whatever its number — the older one answering
    /// last replaces the newer.
    #[test]
    fn an_older_reading_never_lands_over_a_newer() {
        let mut state = CellState::default();
        state.publish(2, said("de_DE.UTF-8"));
        state.publish(1, said("fr_FR.UTF-8"));
        assert_eq!(state.held(), Some(LocaleDeclaration::lang("de_DE.UTF-8")));
        assert_eq!(state.newest.as_ref().map(|(n, _)| *n), Some(2));
        state.publish(
            3,
            failed_with(None, "`locale -a` ended with exit status: 1"),
        );
        state.publish(1, said("fr_FR.UTF-8"));
        assert_eq!(
            state.newest.as_ref().map(|(n, _)| *n),
            Some(3),
            "the newest is the failure, and an older success does not replace it"
        );
        assert_eq!(state.held(), Some(LocaleDeclaration::lang("de_DE.UTF-8")));
    }

    /// RED (revision (c)5) — **a failed refresh after a success is retried by the next birth, and
    /// the old success is used meanwhile**: success, then the locale changes, then that reading
    /// fails. The birth is not held, it is told the old locale, and it asks for a third reading;
    /// one line says so.
    ///
    /// MUTATION: the retry predicate of revision (b)11, "no success held" — no third reading is
    /// asked for, and the failure is kept as the answer for as long as Folio runs.
    #[test]
    fn a_failed_refresh_after_a_success_is_retried_by_the_next_birth_and_the_old_success_is_used_meanwhile()
     {
        let system = FakeSystem::answering(vec![
            said("ko_KR.UTF-8"),
            failed_with(
                Some(LocaleDeclaration::lang("C.UTF-8")),
                "`defaults read -g AppleLocale` ended with exit status: 1",
            ),
            said("zh_TW.UTF-8"),
        ]);
        let (cell, wakes, notes) = cell_over(&system, true);
        cell.ask(LocaleTrigger::Launch);
        crate::lane::wait_for_a_wake(&wakes, "the launch's reading");
        cell.ask(LocaleTrigger::LocaleChanged);
        crate::lane::wait_for_a_wake(&wakes, "the failed refresh");
        until_idle(cell);

        system.hold();
        assert_eq!(cell.at_birth(), AtBirth::Ready, "the birth is not held");
        assert_eq!(
            cell.held(),
            Some(LocaleDeclaration::lang("ko_KR.UTF-8")),
            "and is told the old success"
        );
        system.release();
        crate::lane::wait_for_a_wake(&wakes, "the reading the birth asked for");
        assert_eq!(system.runs(), 3, "the birth asked again");
        assert_eq!(cell.held(), Some(LocaleDeclaration::lang("zh_TW.UTF-8")));
        let lines: Vec<String> = notes.try_iter().collect();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("asked again by a pane's birth")),
            "{lines:?}"
        );
    }

    /// RED (T-OS-SIGNALS-MAC) — **one reading at a time**: while one is out, births and changes
    /// only ask, and every request made meanwhile is answered by exactly one more reading.
    ///
    /// MUTATION: start a worker for every request (`start_a_worker` always true) — two readings
    /// run at once.
    #[test]
    fn one_reading_at_a_time_and_requests_meanwhile_are_one_more() {
        let system = FakeSystem::answering(vec![failed_with(None, "the budget was spent")]);
        let (cell, wakes, _) = cell_over(&system, true);
        system.hold();
        cell.ask(LocaleTrigger::Launch);
        while cell.lock().out.is_none() {
            std::thread::yield_now();
        }
        assert_eq!(cell.at_birth(), AtBirth::Waiting, "nothing has finished");
        cell.ask(LocaleTrigger::LocaleChanged);
        assert_eq!(cell.at_birth(), AtBirth::Waiting);
        cell.ask(LocaleTrigger::LocaleChanged);
        system.release();
        crate::lane::wait_for_a_wake(&wakes, "the held reading");
        crate::lane::wait_for_a_wake(&wakes, "the one reading after it");
        until_idle(cell);
        assert_eq!(
            system.runs(),
            2,
            "the requests while it was out are one more"
        );
        assert_eq!(system.most_at_once(), 1, "never two at once");
        // Failed and none out: one birth asks, the births after it find that request standing.
        system.hold();
        assert_eq!(cell.at_birth(), AtBirth::Ready);
        assert_eq!(cell.at_birth(), AtBirth::Ready);
        assert_eq!(cell.at_birth(), AtBirth::Ready);
        system.release();
        crate::lane::wait_for_a_wake(&wakes, "the one retry");
        until_idle(cell);
        assert_eq!(system.runs(), 3, "three births, one retry");
        assert_eq!(system.most_at_once(), 1);
    }

    /// RED (revision (b)11, owner Q5) — **a cold first pane waits in birth until a reading has
    /// finished, and a failed one ends the wait too**, with the failed reading's own choice: the
    /// encoding the machine has, today's answer for a reading that fails. A birth that finds
    /// nothing asked asks itself.
    ///
    /// MUTATION: `at_birth` answering `Ready` while no reading has finished — the cold first pane
    /// is born with no declaration.
    #[test]
    fn a_cold_first_pane_waits_until_a_reading_finishes_even_a_failed_one() {
        let fallback = LocaleDeclaration::apple_fallback(true, true);
        let system = FakeSystem::answering(vec![failed_with(
            fallback.clone(),
            "`defaults read -g AppleLocale` ended with exit status: 1",
        )]);
        let (cell, wakes, _) = cell_over(&system, true);
        system.hold();
        assert_eq!(
            cell.at_birth(),
            AtBirth::Waiting,
            "nothing asked, nothing held"
        );
        assert!(!cell.settled());
        assert!(
            cell.lock().asked(),
            "the birth asked for the reading itself"
        );
        system.release();
        crate::lane::wait_for_a_wake(&wakes, "the first reading");
        assert!(cell.settled(), "the waiting pane may be born");
        assert_eq!(cell.held(), fallback, "with what the failed reading chose");
    }

    /// RED (T-OS-SIGNALS-MAC) — **window-waits row 7 stays retired: no pane's birth reads the
    /// system's locale; every product road asks the cell for what it holds.** Both product call
    /// sites of the declaration — `shell_integration::shell_command`, on the window thread, and
    /// `derive_environment_for_birth`, on the birth worker — are handed
    /// `crate::locale_declaration::held()`, a lock; the reading itself is named in the product
    /// nowhere but this module, whose worker runs it; and nothing fills a `OnceLock` from it.
    ///
    /// MUTATION: make one call site blocking — `shell_command` handed a declaration it reads
    /// itself, on a worker it waits for — the call site no longer takes `held()`, and the
    /// reading is named outside the cell.
    #[test]
    fn row_7_no_birth_reads_the_locale_itself() {
        use bt_source::{Index, ModuleSpec, Pattern, Scope, Search, View, needle};

        let app = Index::of_package("bt-app");
        let found = |index: &'static Index, pattern: Pattern, view: View, scope: Scope| {
            index
                .search(&Search::new(needle!(pattern), view).in_scope(scope))
                .unwrap_or_else(|failure| panic!("{failure}"))
                .in_the_product(index)
        };
        let shell_integration =
            || Scope::Modules(vec![ModuleSpec::tree("crate::shell_integration")]);
        let call_sites = found(
            app,
            Pattern::text("extend(locale_declaration("),
            View::CodeKeepingLiterals,
            shell_integration(),
        );
        let handed_the_cell = found(
            app,
            Pattern::text("crate::locale_declaration::held().as_ref()"),
            View::CodeKeepingLiterals,
            shell_integration(),
        );
        assert_eq!(call_sites.len(), 2, "{}", call_sites.report(app));
        assert_eq!(
            handed_the_cell.len(),
            2,
            "a product call site of the declaration is not handed what the cell holds:\n{}",
            call_sites.report(app)
        );

        let reading = |scope: Scope| {
            found(
                app,
                Pattern::identifier("read_system_locale_declaration"),
                View::Identifiers,
                scope,
            )
        };
        let everywhere = reading(Scope::Everything);
        let in_the_cell = reading(Scope::Modules(vec![ModuleSpec::tree(
            "crate::locale_declaration",
        )]));
        assert!(
            !in_the_cell.is_empty(),
            "the cell no longer reads, so this reads nothing"
        );
        assert_eq!(
            everywhere.len(),
            in_the_cell.len(),
            "the system's locale is read outside the cell's worker:\n{}",
            everywhere.report(app)
        );

        let platform = Index::of_package("bt-platform");
        let filled_once = found(
            platform,
            Pattern::text("get_or_init(read_system_locale_declaration"),
            View::CodeKeepingLiterals,
            Scope::Everything,
        );
        assert!(
            filled_once.is_empty(),
            "a `OnceLock` fills itself from the reading again:\n{}",
            filled_once.report(platform)
        );
    }

    /// PIN — **a platform that declares no locale never reads one and never holds a birth**
    /// (Windows: a console child is told its encoding by the code page).
    #[test]
    fn a_platform_that_declares_no_locale_never_reads_and_never_waits() {
        let system = FakeSystem::answering(vec![said("en_US.UTF-8")]);
        let (cell, _, _) = cell_over(&system, false);
        cell.ask(LocaleTrigger::Launch);
        assert_eq!(cell.at_birth(), AtBirth::Ready);
        assert!(cell.settled());
        assert_eq!(cell.held(), None);
        assert_eq!(system.runs(), 0);
        assert!(!cell.lock().worker);
    }
}
