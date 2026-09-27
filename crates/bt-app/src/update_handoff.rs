//! **The hand-over: O records `Handoff` and starts its applier** (0.4.6
//! ticket U-21; `docs/plans/design/self-update-2026-09-16.md` §C.3 and
//! revision (b) §(b).2, *The recovery contract*).
//!
//! An update's Restart is the ordinary quit run to completion
//! ([`crate::quit::Reason::UpdateRestart`]). Only when that quit's session
//! has landed — a receipt for the very document it photographed — and the
//! pages have gone does the way out owe the update anything, and what it owes
//! is this, in this order:
//!
//! 1. **`Handoff{applier}` durably**, while this process still holds the
//!    transaction lock ([`Staged::lock`], taken by Prepare and held since):
//!    the journal written through `bt_platform::install_txn::durable_write`
//!    (temporary file, flush, rename, flush of the folder), with the header's
//!    `class` and `outcome` computed by [`crate::update_txn::Journal::encode`]
//!    from the phase — `destructive`, `none`. O writes `outcome: none` and
//!    nothing else (coordinator ruling, 2026-09-27).
//! 2. **Then the applier**, started detached from the rescue copy the header
//!    names, and never waited on. A durable `Handoff` with no applier is W3:
//!    the first lock holder performs the apply, so a death between the two
//!    loses nothing.
//! 3. **A start that failed is journalled** `Abandoned` — terminal, outcome
//!    `none`, the design's "journals `Failed`" — and the process leaves
//!    anyway: nothing has moved, and the next start retires it (W13).
//!
//! A `Handoff` that could not be written leaves the journal `Prepared` (the
//! write is atomic), starts nothing, and the process leaves: the next start
//! offers the update again (W2).
//!
//! # Where it runs
//!
//! On the storage worker (`persist`'s writer thread, [`perform`]), never on
//! the window thread: a durable write waits for the device. The window thread
//! builds the [`HandoffJob`] — pure: the two journals it may write are
//! encoded before anything is sent — hands it over, and reads the answer on
//! its turns under [`crate::quit::HANDOFF_DEADLINE`].

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use bt_platform::install_txn::{self, Held};

use crate::cli;
use crate::update_txn::{Event, Home, Journal, Nonce, Refusal, TxnId};

/// **What Prepare leaves the job holding** (U-20 / U-27 make it; the job keeps
/// it from its `Verified` report until the process leaves): the installation
/// home, the journal as it stands durably at `Prepared`, and the transaction
/// lock, held since `Allocated`.
pub(crate) struct Staged {
    pub(crate) home: Home,
    pub(crate) journal: Journal,
    /// Never read: holding it is its whole job, and the process's exit is
    /// what lets it go — after `Handoff` is on the disk.
    #[expect(
        dead_code,
        reason = "a lock is held by owning it; nothing reads it (install_txn::Held's drop releases it)"
    )]
    pub(crate) lock: Held,
}

/// **How a process is started and let go** — the one effect of the hand-over
/// that is not a file. The product's is [`Detached`]; a test's records.
pub(crate) trait Spawner: Send {
    /// Start `program` with `args`, detached: never waited on.
    ///
    /// # Errors
    /// The operating system would not start it.
    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()>;
}

/// **The product's spawner**: `bt_platform::quiet_command` — the door for a
/// program named by an absolute path (`quiet_command_named` is for a bare name
/// looked up on `PATH`, which this is not) — and the child dropped at once.
pub(crate) struct Detached;

impl Spawner for Detached {
    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()> {
        bt_platform::quiet_command(program)
            .args(args)
            .spawn()
            .map(drop)
    }
}

/// **The applier's command line** after its program: `--update-apply <home>
/// <txn> <nonce>` (U-28's grammar, `cli::update_door`): the installation home,
/// named because the macOS rescue clone cannot find it from its own path
/// alone, then the two values as the journal writes them (lowercase hex).
#[must_use]
pub(crate) fn apply_command_line(home: &Path, txn: TxnId, applier: &Nonce) -> Vec<OsString> {
    vec![
        OsString::from(cli::UPDATE_APPLY_FLAG),
        home.as_os_str().to_os_string(),
        OsString::from(txn.to_string()),
        OsString::from(applier.to_string()),
    ]
}

/// **One hand-over, decided on the window thread and performed on the storage
/// worker**: both journals it may write, already encoded, and the start.
pub(crate) struct HandoffJob {
    journal: PathBuf,
    handoff: Vec<u8>,
    abandoned: Vec<u8>,
    program: PathBuf,
    args: Vec<OsString>,
    spawner: Box<dyn Spawner>,
}

impl HandoffJob {
    /// The hand-over of `staged` to the applier whose nonce is `applier`.
    ///
    /// # Errors
    /// The journal is not in a phase `Handoff` may follow (only `Prepared`
    /// is): nothing is built, nothing will be written.
    pub(crate) fn new(
        staged: &Staged,
        applier: Nonce,
        spawner: Box<dyn Spawner>,
    ) -> Result<Self, Refusal> {
        let handoff = staged.journal.advance(&Event::HandedOff { applier })?;
        let abandoned = handoff.advance(&Event::ApplierNotStarted)?;
        Ok(Self {
            journal: staged.home.journal(),
            handoff: handoff.encode(),
            abandoned: abandoned.encode(),
            program: staged.home.rescue_program(&staged.journal.rescue),
            args: apply_command_line(staged.home.root(), staged.journal.txn, &applier),
            spawner,
        })
    }
}

/// **What became of a hand-over.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HandedOff {
    /// `Handoff` is durable and the applier was started.
    Started,
    /// `Handoff` could not be written: the journal is still `Prepared` and
    /// nothing was started.
    NotRecorded(String),
    /// `Handoff` is durable and the applier could not be started; the journal
    /// was then written `Abandoned`, or could not be (and stays `Handoff`,
    /// which the next lock holder applies).
    NotStarted {
        error: String,
        abandoned: Result<(), String>,
    },
}

impl HandedOff {
    /// The one line `diagnostics.log` gets about transaction `txn`.
    #[must_use]
    pub(crate) fn line(&self, txn: TxnId) -> String {
        match self {
            Self::Started => format!("Folio: update {txn} handed to its applier"),
            Self::NotRecorded(error) => {
                format!("Folio: update {txn} was not handed over, and stays prepared: {error}")
            }
            Self::NotStarted {
                error,
                abandoned: Ok(()),
            } => format!(
                "Folio: update {txn} was abandoned: its applier could not be started ({error})"
            ),
            Self::NotStarted {
                error,
                abandoned: Err(written),
            } => format!(
                "Folio: update {txn}'s applier could not be started ({error}), and the journal \
                 could not say so ({written})"
            ),
        }
    }
}

/// **Perform one hand-over**, on the thread that calls this — the storage
/// worker: `Handoff` durably, **then** the applier; a failed start journals
/// `Abandoned`.
pub(crate) fn perform(mut job: HandoffJob) -> HandedOff {
    if let Err(failure) = install_txn::durable_write(&job.journal, &job.handoff) {
        return HandedOff::NotRecorded(failure.to_string());
    }
    match job.spawner.spawn_detached(&job.program, &job.args) {
        Ok(()) => HandedOff::Started,
        Err(error) => HandedOff::NotStarted {
            error: error.to_string(),
            abandoned: install_txn::durable_write(&job.journal, &job.abandoned)
                .map_err(|failure| failure.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    //! Every test here builds an installation home in a temporary folder, with
    //! the journal written durably at `Prepared` and the transaction lock
    //! really held, and runs the real quit, the real job and the real
    //! hand-over against it. **No applier is ever started**: the spawner is a
    //! recording fake that reads the journal off the disk at the instant it is
    //! asked to start one.

    use std::ffi::OsString;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use bt_platform::HostPlatform;
    use bt_platform::install_txn::{self, Hold};

    use super::{HandedOff, HandoffJob, Spawner, Staged, apply_command_line, perform};
    use crate::persist::SessionStore;
    use crate::quit::{
        Handoff, Quit, QuitAnswer, QuitStep, Reason, SaveReport, UPDATE_RECEIPT_DEADLINE,
        WriteVerdict,
    };
    use crate::update_job::{Abandon, Applied, Job, Offer, State};
    use crate::update_txn::{
        Asker, Class, Disk, HeaderOutcome, Home, Inventories, Journal, JournalRead, Layout,
        Located, Nonce, Phase, PhaseKind, StartAction, StartView, TxnId, at_start, decide,
    };

    const TXN: [u8; 16] = [0x21; 16];

    fn nonce() -> Nonce {
        Nonce::new([0x5a; 32])
    }

    /// A folder of its own under the temporary directory, removed on drop.
    struct Folder(PathBuf);

    impl Folder {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "bt-update-handoff-{tag}-{}-{}",
                std::process::id(),
                bt_platform::attention_pipe::unguessable_bits()
            ));
            std::fs::create_dir_all(&path).expect("a scratch folder");
            Self(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An installation home with a transaction staged in it: the journal
    /// durably at `Prepared`, the transaction lock held.
    fn staged(folder: &Folder) -> Staged {
        let home = Home::at(folder.0.join(".folio-update"));
        std::fs::create_dir_all(home.transaction(TxnId::new(TXN)).join("rescue"))
            .expect("the transaction's folder");
        let rescue = home
            .transaction(TxnId::new(TXN))
            .join("rescue")
            .join("folio.exe");
        let allocated = Journal::allocate(
            TxnId::new(TXN),
            rescue.to_string_lossy().into_owned(),
            Layout::Members(Inventories {
                old_shipped: Vec::new(),
                old_present: Vec::new(),
                new: Vec::new(),
            }),
        );
        let journal = allocated
            .advance(&crate::update_txn::Event::Prepared)
            .expect("Allocated → Prepared");
        install_txn::durable_write(&home.journal(), &journal.encode())
            .expect("the journal is written durably");
        let lock = install_txn::try_hold(&home.lock(), Hold::Exclusive)
            .expect("the lock file opens")
            .expect("nobody else holds the lock");
        Staged {
            home,
            journal,
            lock,
        }
    }

    /// The journal as a start would read it off the disk now.
    fn on_disk(home: &Home) -> Journal {
        Journal::parse(&std::fs::read(home.journal()).expect("the journal is there"))
            .expect("the journal reads")
    }

    /// Everything a spawner was asked, and the journal's phase at that instant.
    #[derive(Default)]
    struct Asked {
        calls: Vec<(PathBuf, Vec<OsString>, Phase)>,
    }

    /// A spawner that reads the journal off the disk when asked, records it,
    /// and starts nothing — or fails, when told to.
    struct Recording {
        journal: PathBuf,
        asked: Arc<Mutex<Asked>>,
        fail: bool,
    }

    impl Spawner for Recording {
        fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()> {
            let journal = Journal::parse(&std::fs::read(&self.journal)?)
                .map_err(|refusal| io::Error::other(refusal.to_string()))?;
            self.asked.lock().expect("the record").calls.push((
                program.to_path_buf(),
                args.to_vec(),
                journal.body.phase,
            ));
            if self.fail {
                Err(io::Error::other("the system would not start it"))
            } else {
                Ok(())
            }
        }
    }

    fn recording(staged: &Staged, fail: bool) -> (Box<Recording>, Arc<Mutex<Asked>>) {
        let asked = Arc::new(Mutex::new(Asked::default()));
        (
            Box::new(Recording {
                journal: staged.home.journal(),
                asked: Arc::clone(&asked),
                fail,
            }),
            asked,
        )
    }

    /// A job at `Quitting` for transaction [`TXN`] — Restart pressed on a
    /// verified card — and the reason its quit begins with.
    fn quitting_job() -> (Job<u32>, Reason) {
        let mut job = Job::<u32>::verified_for_test(
            Offer::mint(TxnId::new(TXN), "v0.4.7", HostPlatform::Windows).expect("a release tag"),
        );
        let reason = job.restart().expect("Restart on a verified card");
        assert!(matches!(job.state(), State::Quitting(_)));
        (job, reason)
    }

    /// Hand whatever the quit owes the job to it, as the window thread does.
    fn deliver(quit: &mut Quit, job: &mut Job<u32>) {
        if let Some(report) = quit.take_update_report() {
            assert_eq!(job.apply(report), Applied::Moved, "{report:?}");
        }
    }

    /// Walk an update's quit whose session lands, from its card (answered
    /// `Discard`) to the way out, answering each step as `settle_quit` does,
    /// and call `at` with the step and the quit before each is answered.
    fn walk_to_the_way_out(
        quit: &mut Quit,
        job: &mut Job<u32>,
        mut at: impl FnMut(QuitStep, &Quit),
    ) {
        let start = Instant::now();
        let mut step = quit.step();
        loop {
            at(step, quit);
            step = match step {
                QuitStep::Ask => quit.answer(QuitAnswer::Discard),
                QuitStep::Discard => quit.discarded(),
                QuitStep::Photograph => quit.photographed(),
                QuitStep::Write if quit.awaited_generation().is_none() => quit.requested(7, start),
                QuitStep::Write => quit.written(WriteVerdict::Landed),
                QuitStep::Retire => quit.retired(start),
                QuitStep::WaitForPages => quit.pages(true, start),
                QuitStep::Exit | QuitStep::Save | QuitStep::Abandon => return,
            };
            deliver(quit, job);
        }
    }

    /// RED (U-21) — **`Handoff` is on the disk before the applier is started,
    /// and only after the session landed**: the spawner, reading the journal
    /// at the instant it is asked, finds `Handoff` naming the applier's
    /// nonce; before the hand-over the journal is still `Prepared`.
    ///
    /// Run through the real road: a quit for the update, walked to its way
    /// out; the job's staged transaction; the job built on the window thread's
    /// side and performed by the real storage worker (`SessionStore::hand_off`)
    /// with the real durable write.
    ///
    /// MUTATION: in `perform`, start the applier before the durable write —
    /// the spawner then reads `Prepared`.
    #[test]
    fn handoff_is_durable_before_the_spawn() {
        let folder = Folder::new("durable");
        let staged = staged(&folder);
        let (mut job, reason) = quitting_job();
        let mut quit = Quit::begin_for(vec!["notes.md".to_owned()], reason);
        walk_to_the_way_out(&mut quit, &mut job, |_, _| {});
        assert!(matches!(job.state(), State::Committing(_)));
        assert_eq!(quit.handoff(), Handoff::Owed);
        assert_eq!(
            on_disk(&staged.home).body.phase.kind(),
            PhaseKind::Prepared,
            "nothing is written before the way out"
        );

        let (spawner, asked) = recording(&staged, false);
        let handoff = HandoffJob::new(&staged, nonce(), spawner).expect("Prepared → Handoff");
        let mut store =
            SessionStore::at(folder.0.join("session.json"), folder.0.join("session.lock"));
        let answer = store
            .hand_off(handoff)
            .expect("the storage worker takes the hand-over");
        quit.handoff_sent(Instant::now());
        let answered = answer
            .recv_timeout(Duration::from_secs(20))
            .expect("the storage worker answers");
        assert_eq!(answered, HandedOff::Started);
        quit.handed_off();
        assert_eq!(quit.handoff(), Handoff::Done);
        assert_eq!(quit.step(), QuitStep::Exit);

        let asked = asked.lock().expect("the record");
        let [(program, args, phase)] = asked.calls.as_slice() else {
            panic!("one start, not {}", asked.calls.len());
        };
        assert_eq!(
            phase,
            &Phase::Handoff { applier: nonce() },
            "the applier is started only once `Handoff` is durable"
        );
        assert_eq!(
            program,
            &staged.home.rescue_program(&staged.journal.rescue),
            "from the rescue copy the header names"
        );
        assert_eq!(
            args,
            &apply_command_line(staged.home.root(), TxnId::new(TXN), &nonce())
        );
        let header = on_disk(&staged.home).header();
        assert_eq!(header.class, Class::Destructive);
        assert_eq!(
            header.outcome,
            HeaderOutcome::None,
            "O writes `outcome: none` (coordinator ruling 2026-09-27)"
        );
    }

    /// RED (U-21) — **an applier that could not be started journals the
    /// transaction abandoned — terminal, outcome `none` — and the process
    /// still leaves.**
    ///
    /// §C.3: "A spawn failure there journals `Failed` and exits anyway —
    /// nothing has moved, and the next start says so." The next start finds a
    /// terminal class and retires it (W13).
    ///
    /// MUTATION: in `perform`, answer `NotStarted` without writing the
    /// abandoned journal — the disk still says `Handoff`.
    #[test]
    fn a_spawn_failure_journals_failed_and_still_exits() {
        let folder = Folder::new("spawn-fails");
        let staged = staged(&folder);
        let (mut job, reason) = quitting_job();
        let mut quit = Quit::begin_for(Vec::new(), reason);
        walk_to_the_way_out(&mut quit, &mut job, |_, _| {});
        let (spawner, asked) = recording(&staged, true);
        quit.handoff_sent(Instant::now());
        let answered =
            perform(HandoffJob::new(&staged, nonce(), spawner).expect("Prepared → Handoff"));
        assert!(
            matches!(
                &answered,
                HandedOff::NotStarted {
                    abandoned: Ok(()),
                    ..
                }
            ),
            "{answered:?}"
        );
        assert_eq!(asked.lock().expect("the record").calls.len(), 1);
        let journal = on_disk(&staged.home);
        assert_eq!(journal.body.phase, Phase::Abandoned);
        let header = journal.header();
        assert_eq!(header.class, Class::Terminal);
        assert_eq!(header.outcome, HeaderOutcome::None);
        assert_eq!(
            at_start(&StartView {
                journal: JournalRead::Read(header),
                lock_free: true,
                own_image: None,
                rescue_image: None,
                trial_of: None,
            }),
            StartAction::Retire,
            "the next start retires it (W13)"
        );
        quit.handed_off();
        assert_eq!(
            (quit.step(), quit.handoff()),
            (QuitStep::Exit, Handoff::Done),
            "and the process leaves anyway"
        );
    }

    /// RED (U-21) — **a Cancel, a save that did not all go through, or a
    /// session the disk refused abandons the update with the quit**: the job
    /// is back at `Verified` naming the reason, no hand-over is ever owed, and
    /// the journal on the disk is still `Prepared` (R-4).
    ///
    /// MUTATION: in `Quit::answer`, let `Cancel` abandon the quit without
    /// giving the update up — the job stays `Quitting`.
    #[test]
    fn quit_cancel_or_failed_save_prevents_swap() {
        let folder = Folder::new("cancel");
        let staged = staged(&folder);
        let roads: [(&str, Abandon); 3] = [
            ("cancel", Abandon::Cancelled),
            ("save", Abandon::SaveIncomplete),
            ("write", Abandon::SessionRefused),
        ];
        for (road, why) in roads {
            let (mut job, reason) = quitting_job();
            let mut quit = Quit::begin_for(vec!["notes.md".to_owned()], reason);
            let mut steps = vec![quit.step()];
            let mut step = match road {
                "cancel" => quit.answer(QuitAnswer::Cancel),
                "save" => {
                    steps.push(quit.answer(QuitAnswer::Save));
                    quit.saved(&SaveReport {
                        saved: Vec::new(),
                        failed: vec![("notes.md".to_owned(), "the disk is full".to_owned())],
                    })
                }
                _ => {
                    steps.push(quit.answer(QuitAnswer::Discard));
                    steps.push(quit.discarded());
                    steps.push(quit.photographed());
                    steps.push(quit.requested(3, Instant::now()));
                    quit.written(WriteVerdict::Refused)
                }
            };
            steps.push(step);
            deliver(&mut quit, &mut job);
            assert_eq!(step, QuitStep::Abandon, "{road}: {steps:?}");
            assert_eq!(quit.handoff(), Handoff::NotOwed, "{road}");
            assert!(
                !steps.contains(&QuitStep::Exit),
                "{road}: the way out is never reached: {steps:?}"
            );
            assert!(
                matches!(job.state(), State::Verified(_)),
                "{road}: {:?}",
                job.state()
            );
            assert_eq!(job.quit_abandoned(), Some(why), "{road}");
            step = quit.step();
            assert_eq!(step, QuitStep::Abandon);
            assert_eq!(
                on_disk(&staged.home).body.phase.kind(),
                PhaseKind::Prepared,
                "{road}: the journal stays `Prepared`"
            );
        }
    }

    /// RED (U-21) — **a session receipt that does not come back in time
    /// abandons the update and not the quit**: the quit retires and leaves as
    /// any timed-out quit does, the job is back at `Verified` naming the
    /// timeout, the way out owes no hand-over, and the journal stays
    /// `Prepared`, so the next start offers the update again.
    ///
    /// The store's half is real: the overdue receipt is booked as the
    /// synchronous wait's expiry is, a refusal the quit may proceed past.
    ///
    /// MUTATION: in `Quit::written`, hand the update on for `TimedOut` as for
    /// `Landed` — the way out then owes a hand-over.
    #[test]
    fn a_session_timeout_abandons_the_update_and_still_quits() {
        let folder = Folder::new("timeout");
        let staged = staged(&folder);
        let (mut job, reason) = quitting_job();
        let mut quit = Quit::begin_for(Vec::new(), reason);
        assert_eq!(quit.step(), QuitStep::Photograph);
        quit.photographed();
        let mut store =
            SessionStore::at(folder.0.join("session.json"), folder.0.join("session.lock"));
        let start = Instant::now();
        let generation = store
            .hand_over_final(start)
            .expect("handed over")
            .expect("this store writes");
        assert_eq!(quit.requested(generation, start), QuitStep::Write);
        assert!(!quit.receipt_is_overdue(start + UPDATE_RECEIPT_DEADLINE / 2));
        assert_eq!(
            quit.wake_at(start),
            Some(start + crate::quit::ANSWER_LOOK),
            "the loop comes back to look, and waits for nothing"
        );
        let late = start + UPDATE_RECEIPT_DEADLINE;
        assert!(quit.receipt_is_overdue(late), "the bound is a bound");
        let refusal = store.receipt_overdue(generation, late);
        assert!(refusal.quit_may_proceed(), "{refusal:?}");
        assert_eq!(quit.written(WriteVerdict::TimedOut), QuitStep::Retire);
        deliver(&mut quit, &mut job);
        assert!(matches!(job.state(), State::Verified(_)));
        assert_eq!(job.quit_abandoned(), Some(Abandon::SessionTimedOut));
        assert_eq!(quit.retired(late), QuitStep::WaitForPages);
        assert_eq!(
            quit.pages(true, late),
            QuitStep::Exit,
            "and the quit leaves"
        );
        assert_eq!(quit.handoff(), Handoff::NotOwed, "with no applier");
        assert_eq!(on_disk(&staged.home).body.phase.kind(), PhaseKind::Prepared);
    }

    /// RED (U-21) — **closing the process at any step of an update's quit
    /// leaves a journal the recovery table names**: `Prepared` (W2: a start
    /// continues and the job resumes or discards) until the way out, and
    /// `Handoff` (W3: a start hands itself to the rescue build, and the lock
    /// holder that finds it performs the apply) once the hand-over has been
    /// performed — with its applier started or not.
    ///
    /// "Closing" is read the way the next process reads it: the journal off
    /// the disk, the lock free (the dead process held it), through the
    /// ordinary start's rule and the lock holder's.
    ///
    /// MUTATION: make `Handoff`'s class `deferred` in `PhaseKind::class` — a
    /// start then carries on past a transaction its applier owns.
    #[test]
    fn close_during_each_phase_preserves_recovery() {
        let folder = Folder::new("close");
        let staged = staged(&folder);
        let (mut job, reason) = quitting_job();
        let mut quit = Quit::begin_for(vec!["notes.md".to_owned()], reason);
        let row = |journal: &Journal| {
            let start = at_start(&StartView {
                journal: JournalRead::Read(journal.header()),
                lock_free: true,
                own_image: None,
                rescue_image: None,
                trial_of: None,
            });
            let holder = decide(&Disk {
                journal,
                asker: Asker::LockHolder,
                entrance: false,
                located: Located::Members(Vec::new()),
                receipt: None,
                trial_alive: false,
                now_ms: 0,
            });
            (journal.body.phase.kind(), start, holder)
        };
        let mut seen = Vec::new();
        walk_to_the_way_out(&mut quit, &mut job, |step, _| {
            seen.push((step, row(&on_disk(&staged.home))));
        });
        let before = seen.len();
        assert!(before >= 7, "every step of the quit: {seen:?}");
        for (step, (phase, start, holder)) in &seen {
            assert_eq!(
                (*phase, *start),
                (PhaseKind::Prepared, StartAction::Continue),
                "W2 at {step:?}"
            );
            assert_eq!(*holder, crate::update_txn::Action::Leave, "W2 at {step:?}");
        }
        for fail in [false, true] {
            if fail {
                install_txn::durable_write(&staged.home.journal(), &staged.journal.encode())
                    .expect("back to Prepared for the second road");
            }
            let (spawner, _) = recording(&staged, fail);
            let answered =
                perform(HandoffJob::new(&staged, nonce(), spawner).expect("Prepared → Handoff"));
            let (phase, start, holder) = row(&on_disk(&staged.home));
            if fail {
                assert!(matches!(answered, HandedOff::NotStarted { .. }));
                assert_eq!(
                    (phase, start),
                    (PhaseKind::Abandoned, StartAction::Retire),
                    "W13 when no applier was started"
                );
            } else {
                assert_eq!(answered, HandedOff::Started);
                assert_eq!(
                    (phase, start, holder),
                    (
                        PhaseKind::Handoff,
                        StartAction::HandToRescue,
                        crate::update_txn::Action::Apply
                    ),
                    "W3 once the hand-over is durable"
                );
            }
        }
    }
}
