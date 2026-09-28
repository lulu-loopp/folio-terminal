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
//!    `none`, the design's "journals `Failed`": nothing has moved, and the
//!    next start retires it (W13).
//!
//! A `Handoff` that could not be written leaves the journal `Prepared` (the
//! write is atomic) and starts nothing: the next start offers the update
//! again (W2).
//!
//! **And then O leaves through its exit guard** (0.4.6 ticket U-34, the one
//! guard every road process leaves through: `update_apply::ExitGuard`). Who
//! opens the window is decided by one mark, never by who is alive
//! (`update_apply::OWNER_FILE`, `H\<txn>\owner`): the hand-over clears it
//! before `Handoff` is written ([`perform`]); the applier takes it as soon as
//! it knows its transaction; O, at the process's very end — after the loop,
//! once it has let go of its data directory's claim ([`leave_armed`]) — takes
//! it too, and starts Folio only if it got it ([`Leaving`]). So an applier
//! that took the mark opens the window itself, and O starts nothing; an
//! applier that never took it (not started, not yet running, refused before
//! its road) leaves it to O, and a late one that finds O's mark touches
//! nothing. O's start reads the header the way a lock holder's does: a
//! `destructive` journal (`Handoff`, nothing moved) is started with
//! `--update-failed`, so it continues — with *Update incomplete.* — and never
//! hands itself back; `Prepared` or `Abandoned` plainly (W2, W13). **An answer
//! that does not come within [`crate::quit::HANDOFF_DEADLINE`]** (W9) changes
//! nothing of this: the mark decides.
//!
//! # Where it runs
//!
//! On the storage worker (`persist`'s writer thread, [`perform`]), never on
//! the window thread: a durable write waits for the device. The window thread
//! builds the [`HandoffJob`] — pure: the two journals it may write are
//! encoded before anything is sent — hands it over, and reads the answer on
//! its turns under [`crate::quit::HANDOFF_DEADLINE`] ([`look`]). The worker
//! says how long the write and the start each took, in one line.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use bt_platform::install_flip::Running;
use bt_platform::install_txn::{self, Held};

use bt_platform::file_reads::{self, Lane};

use crate::cli;
use crate::update_apply::{ExitGuard, Leave, Left, Window};
use crate::update_txn::{
    Class, Event, Header, HeaderOutcome, Home, Journal, Nonce, Refusal, TxnId,
};

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
    /// Start `program` with `args`, detached: never waited on. Answers the
    /// child by its pid and start instant (`started` 0 when it cannot be read:
    /// nothing then proves it runs).
    ///
    /// # Errors
    /// The operating system would not start it.
    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<Running>;
}

/// **The product's spawner**: `bt_platform::quiet_command` — the door for a
/// program named by an absolute path (`quiet_command_named` is for a bare name
/// looked up on `PATH`, which this is not) — and the child dropped at once.
pub(crate) struct Detached;

impl Spawner for Detached {
    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<Running> {
        let child = bt_platform::quiet_command(program).args(args).spawn()?;
        let pid = child.id();
        Ok(Running {
            pid,
            started: bt_platform::install_flip::started_of(pid).unwrap_or(0),
        })
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
    txn: TxnId,
    journal: PathBuf,
    /// `H\<txn>\owner`, cleared before `Handoff` (`update_apply::OWNER_FILE`).
    owner: PathBuf,
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
            txn: staged.journal.txn,
            journal: staged.home.journal(),
            owner: crate::update_apply::owner_path(&staged.home, staged.journal.txn),
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
    /// `Handoff` is durable and the applier was started: this process.
    Started { applier: Running },
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
            Self::Started { applier } => {
                format!(
                    "Folio: update {txn} handed to its applier ({})",
                    applier.pid
                )
            }
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
/// `Abandoned`. One diagnostics line says how long the write and the start
/// each took (U-34: a first start of the new rescue executable that a scanner
/// holds is measured apart from the durable write).
pub(crate) fn perform(mut job: HandoffJob) -> HandedOff {
    let began = Instant::now();
    // No window's mark of an earlier attempt may stand once `Handoff` is on
    // the disk: the applier this hand-over starts takes a fresh one (U-34).
    if let Err(why) = crate::update_apply::clear_the_window(&job.owner) {
        return HandedOff::NotRecorded(format!("the window's mark: {why}"));
    }
    if let Err(failure) = install_txn::durable_write(&job.journal, &job.handoff) {
        crate::diagnostics::note(&took_line(job.txn, began.elapsed(), None));
        return HandedOff::NotRecorded(failure.to_string());
    }
    let written = began.elapsed();
    let spawned = Instant::now();
    let started = job.spawner.spawn_detached(&job.program, &job.args);
    crate::diagnostics::note(&took_line(job.txn, written, Some(spawned.elapsed())));
    match started {
        Ok(applier) => HandedOff::Started { applier },
        Err(error) => HandedOff::NotStarted {
            error: error.to_string(),
            abandoned: install_txn::durable_write(&job.journal, &job.abandoned)
                .map_err(|failure| failure.to_string()),
        },
    }
}

/// **The hand-over's timing line**: the durable write of `Handoff`, and the
/// start of the applier (`None`: not tried, the write failed).
#[must_use]
pub(crate) fn took_line(
    txn: TxnId,
    write: std::time::Duration,
    spawn: Option<std::time::Duration>,
) -> String {
    match spawn {
        Some(spawn) => format!(
            "Folio: update {txn}'s hand-over: Handoff written in {} ms, the applier's start took {} ms",
            write.as_millis(),
            spawn.as_millis()
        ),
        None => format!(
            "Folio: update {txn}'s hand-over: Handoff not written, after {} ms",
            write.as_millis()
        ),
    }
}

/// **What O's way out owes after *Restart to update*** (U-34): the
/// transaction it hands over — whose window's mark decides who opens Folio —
/// or none, when nothing was staged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Leaving {
    pub(crate) window: Option<(Home, TxnId)>,
}

impl Leaving {
    /// The transaction `txn` in `home`, handed over or about to be.
    #[must_use]
    pub(crate) fn over(home: &Home, txn: TxnId) -> Self {
        Self {
            window: Some((home.clone(), txn)),
        }
    }

    /// No transaction to read: O starts Folio plainly.
    #[must_use]
    pub(crate) fn nothing_staged() -> Self {
        Self { window: None }
    }

    /// **Leave, through the exit guard** (`update_apply::ExitGuard`), as `me`:
    /// the window's mark taken ([`crate::update_apply::take_the_window`]) — a
    /// live applier that has it opens the window, and nothing is started
    /// here; otherwise `program`, the installed build (this process's own
    /// executable), is started with what the header names.
    pub(crate) fn leave(self, me: Running, program: &Path, spawner: &mut dyn Spawner) -> Left {
        let home = self.window.as_ref().map(|(home, _)| home.clone());
        let mut guard = ExitGuard::new(OldLeave {
            program,
            spawner,
            home: home.as_ref(),
        });
        if let Some((home, txn)) = &self.window
            && let Window::Theirs(owner) = crate::update_apply::take_the_window(home, *txn, me)
        {
            guard.not_mine(Some(owner.pid));
        }
        guard.leave()
    }
}

/// **How O leaves** (`update_apply::Leave`): its own executable. O is not the
/// lock holder and nothing has moved while the journal is O's (`Prepared`,
/// `Handoff`, `Abandoned`): the header alone names the words — a
/// `destructive` one `--update-failed`, so the start continues past it with
/// *Update incomplete.* and never hands itself to the recovery build; any
/// other plainly.
struct OldLeave<'a> {
    program: &'a Path,
    spawner: &'a mut dyn Spawner,
    home: Option<&'a Home>,
}

impl Leave for OldLeave<'_> {
    fn say(&mut self, line: &str) {
        crate::diagnostics::note(line);
    }

    fn opening(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
        let words = self
            .home
            .filter(|home| {
                file_reads::read(Lane::UpdateJournal, home.journal())
                    .ok()
                    .and_then(|bytes| Header::parse(&bytes).ok())
                    .is_some_and(|header| {
                        header.class == Class::Destructive
                            || header.outcome == HeaderOutcome::RolledBack
                    })
            })
            .map(|home| crate::update_apply::failed_words(home).to_vec())
            .unwrap_or_default();
        Some((self.program.to_path_buf(), words))
    }

    fn start(&mut self, program: &Path, words: &[OsString]) -> io::Result<()> {
        self.spawner.spawn_detached(program, words).map(drop)
    }
}

/// **What the loop reads of a hand-over it sent** (U-34): nothing yet, or its
/// end and the line to note.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Looked {
    /// No answer, and the deadline has not passed: look again next turn.
    Waiting,
    /// The answer came, or the deadline passed without one.
    Over { line: String },
}

/// **One look at the hand-over's answer** for transaction `txn`, `overdue`
/// once [`crate::quit::HANDOFF_DEADLINE`] has passed. Never waits.
pub(crate) fn look(answer: Option<&Receiver<HandedOff>>, txn: TxnId, overdue: bool) -> Looked {
    match answer.and_then(|answer| answer.try_recv().ok()) {
        Some(handed) => Looked::Over {
            line: handed.line(txn),
        },
        None if overdue => Looked::Over {
            line: format!(
                "Folio: update {txn}'s hand-over did not answer in time; the window's mark decides \
                 who opens Folio as this process leaves"
            ),
        },
        None => Looked::Waiting,
    }
}

/// **What this process's way out owes after *Restart to update*** — set by
/// the loop ([`arm`]), spent once at the process's end or in its panic
/// ([`leave_armed`]).
static ARMED: Mutex<Option<Leaving>> = Mutex::new(None);

/// **Arm O's exit guard** with the transaction it hands over.
pub(crate) fn arm(leaving: Leaving) {
    if let Ok(mut armed) = ARMED.lock() {
        *armed = Some(leaving);
    }
}

/// **O's exit guard, spent** (U-34): at the very end of the process — the loop
/// has returned, the session's sentinel is gone — or in a panic after the
/// hand-over. First this process lets go of its data directory's claim, so the
/// start it makes is the writer and not a launch handed back to a Folio that
/// is leaving; then the exit guard. `None` when no update was handed over.
pub(crate) fn leave_armed() -> Option<Left> {
    // Never waits: this also runs in the panic hook.
    let leaving = ARMED.try_lock().ok()?.take()?;
    crate::persist::let_go_of_every_claim();
    let left = match std::env::current_exe() {
        Ok(program) => leaving.leave(crate::update_apply::this_process(), &program, &mut Detached),
        Err(error) => Left::NotStarted(PathBuf::new(), error.to_string()),
    };
    crate::diagnostics::note(&format!("Folio: leaving after an update: {}", left.said()));
    Some(left)
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
    use bt_platform::install_flip::Running;
    use bt_platform::install_txn::{self, Hold};

    use super::{
        HandedOff, HandoffJob, Leaving, Looked, Spawner, Staged, apply_command_line, look, perform,
    };
    use crate::persist::SessionStore;
    use crate::quit::{
        Handoff, Quit, QuitAnswer, QuitStep, Reason, SaveReport, UPDATE_RECEIPT_DEADLINE,
        WriteVerdict,
    };
    use crate::update_apply::Left;
    use crate::update_apply::Window;
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
        fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<Running> {
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
                // Nothing is started: this test's own process stands for the
                // applier, alive for as long as anything looks.
                Ok(crate::update_apply::this_process())
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
        assert_eq!(
            answered,
            HandedOff::Started {
                applier: crate::update_apply::this_process()
            }
        );
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
                sent_by_rollback: false,
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
                sent_by_rollback: false,
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
                assert!(
                    matches!(answered, HandedOff::Started { .. }),
                    "{answered:?}"
                );
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

    /// A spawner that starts nothing and records every start it is asked
    /// for: O's exit guard's.
    #[derive(Default)]
    struct Starts {
        calls: Vec<(PathBuf, Vec<OsString>)>,
    }

    impl Spawner for Starts {
        fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<Running> {
            self.calls.push((program.to_path_buf(), args.to_vec()));
            Ok(Running { pid: 0, started: 0 })
        }
    }

    /// **A spawner held on its first start until the test lets it go** — the
    /// first start of a new executable that a scanner reads whole (W9).
    struct Held {
        gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
        asked: Arc<Mutex<Asked>>,
        journal: PathBuf,
    }

    impl Spawner for Held {
        fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<Running> {
            let (open, turn) = &*self.gate;
            let give_up = Instant::now() + Duration::from_secs(60);
            let mut opened = open.lock().expect("the gate");
            while !*opened && Instant::now() < give_up {
                opened = turn
                    .wait_timeout(opened, Duration::from_millis(50))
                    .expect("the gate")
                    .0;
            }
            drop(opened);
            let journal = Journal::parse(&std::fs::read(&self.journal)?)
                .map_err(|refusal| io::Error::other(refusal.to_string()))?;
            self.asked.lock().expect("the record").calls.push((
                program.to_path_buf(),
                args.to_vec(),
                journal.body.phase,
            ));
            Ok(crate::update_apply::this_process())
        }
    }

    /// The loop's looks at the answer, as `hand_the_update_over` makes them,
    /// with `now` read as `at(turn)`, until the hand-over is over.
    fn look_until_over(
        answer: &std::sync::mpsc::Receiver<HandedOff>,
        quit: &Quit,
        at: impl Fn(u32) -> Instant,
    ) -> String {
        for turn in 0..2_000 {
            match look(
                Some(answer),
                TxnId::new(TXN),
                quit.handoff_is_overdue(at(turn)),
            ) {
                Looked::Over { line } => return line,
                Looked::Waiting => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        panic!("the hand-over never ended");
    }

    /// RED (U-34, W9) — **a hand-over with no answer by its deadline still
    /// leaves a Folio: O's exit guard, finding no applier holding the
    /// window's mark, takes it and starts the installed build once — with
    /// `--update-failed`, `Handoff` being destructive — and a late applier then
    /// finds O's mark and starts nothing; when the applier did take the mark,
    /// O starts nothing.**
    ///
    /// The clean VM's W9: the storage worker was still inside the applier's
    /// start (a first start of a new 79 MB executable) when the 3 s budget ran
    /// out; O left, no applier ran, and nothing opened until the next logon.
    /// Through the real road: the quit walked to its way out, the job's staged
    /// transaction, the real storage worker holding the first start past the
    /// deadline, the loop's look at the answer — the deadline read on the
    /// quit's own clock, [`crate::quit::HANDOFF_DEADLINE`] later — and O's
    /// exit guard. The start it makes is recorded, never made; the late
    /// applier's start is let through afterwards and changes nothing.
    ///
    /// MUTATION: in `look`, answer `Looked::Waiting` past the deadline too, or
    /// in `OldLeave::opening` answer `None` (no start by the deadline); in
    /// `Leaving::leave`, start whatever the mark says (a second start beside
    /// the applier's).
    #[test]
    fn a_hand_over_with_no_answer_in_time_still_opens_folio_once() {
        let folder = Folder::new("overdue");
        let staged = staged(&folder);
        let (mut job, reason) = quitting_job();
        let mut quit = Quit::begin_for(Vec::new(), reason);
        walk_to_the_way_out(&mut quit, &mut job, |_, _| {});
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let asked = Arc::new(Mutex::new(Asked::default()));
        let held = Held {
            gate: Arc::clone(&gate),
            asked: Arc::clone(&asked),
            journal: staged.home.journal(),
        };
        let handoff =
            HandoffJob::new(&staged, nonce(), Box::new(held)).expect("Prepared → Handoff");
        let mut store =
            SessionStore::at(folder.0.join("session.json"), folder.0.join("session.lock"));
        let answer = store
            .hand_off(handoff)
            .expect("the storage worker takes the hand-over");
        let sent = Instant::now();
        quit.handoff_sent(sent);
        assert_eq!(
            look(
                Some(&answer),
                TxnId::new(TXN),
                quit.handoff_is_overdue(sent)
            ),
            Looked::Waiting
        );
        let line = look_until_over(&answer, &quit, |_| sent + crate::quit::HANDOFF_DEADLINE);
        assert!(line.contains("did not answer in time"), "{line}");
        quit.handed_off();
        assert_eq!(
            (quit.step(), quit.handoff()),
            (QuitStep::Exit, Handoff::Done)
        );

        // O leaves: nobody took the window's mark, so O takes it and starts.
        let txn = TxnId::new(TXN);
        let old = crate::update_apply::this_process();
        let installed = folder.0.join("folio.exe");
        let mut starts = Starts::default();
        let left = Leaving::over(&staged.home, txn).leave(old, &installed, &mut starts);
        assert_eq!(left, Left::Started(installed.clone()));
        let failed = crate::update_apply::failed_words(&staged.home).to_vec();
        assert_eq!(starts.calls, vec![(installed.clone(), failed)]);

        // The late start goes through; the applier finds O's live mark.
        *gate.0.lock().expect("the gate") = true;
        gate.1.notify_all();
        let late = answer
            .recv_timeout(Duration::from_secs(20))
            .expect("the worker answers at last");
        assert!(matches!(late, HandedOff::Started { .. }), "{late:?}");
        assert_eq!(asked.lock().expect("the record").calls.len(), 1);
        let late_applier = Running { pid: 1, started: 1 };
        assert_eq!(
            crate::update_apply::take_the_window(&staged.home, txn, late_applier),
            Window::Theirs(old),
            "a late applier finds O's mark and touches nothing"
        );
        assert_eq!(starts.calls.len(), 1, "exactly one start");

        // A prompt hand-over whose applier took the mark: O starts nothing.
        install_txn::durable_write(&staged.home.journal(), &staged.journal.encode())
            .expect("back to Prepared for the second road");
        let (spawner, _) = recording(&staged, false);
        let handoff = HandoffJob::new(&staged, nonce(), spawner).expect("Prepared → Handoff");
        let answer = store.hand_off(handoff).expect("the worker takes it");
        let sent = Instant::now();
        let line = look_until_over(&answer, &quit, |_| sent);
        assert!(line.contains("handed to its applier"), "{line}");
        assert_eq!(
            crate::update_apply::window_owner(&staged.home, txn),
            None,
            "the hand-over cleared the earlier mark"
        );
        let applier = crate::update_apply::this_process();
        assert_eq!(
            crate::update_apply::take_the_window(&staged.home, txn, applier),
            Window::Mine
        );
        let mut starts = Starts::default();
        assert_eq!(
            Leaving::over(&staged.home, txn).leave(
                Running { pid: 1, started: 1 },
                &installed,
                &mut starts
            ),
            Left::NotMine(Some(applier.pid))
        );
        assert!(starts.calls.is_empty(), "{:?}", starts.calls);
    }

    /// RED (U-34, round 2) — **the window's mark is taken by exactly one live
    /// process**: created for the first taker; refused to a second while the
    /// first runs (by pid and start instant); taken over from an owner that no
    /// longer runs; and unknown — nobody proven — when it can be neither made
    /// nor read.
    ///
    /// MUTATION: in `update_apply::take_the_window`, replace the mark whatever
    /// it names (a second live taker then gets it too).
    #[test]
    fn the_windows_mark_is_taken_by_exactly_one_live_process() {
        let folder = Folder::new("mark");
        let staged = staged(&folder);
        let txn = TxnId::new(TXN);
        let me = crate::update_apply::this_process();
        let other = Running { pid: 1, started: 1 };
        let take = |who| crate::update_apply::take_the_window(&staged.home, txn, who);
        assert_eq!(take(me), Window::Mine);
        assert_eq!(take(me), Window::Mine);
        assert_eq!(take(other), Window::Theirs(me));
        std::fs::write(
            crate::update_apply::owner_path(&staged.home, txn),
            format!("{}:{}", me.pid, me.started.wrapping_add(1)),
        )
        .unwrap();
        assert_eq!(
            take(other),
            Window::Mine,
            "an owner that no longer runs is taken over"
        );
        assert_eq!(
            crate::update_apply::window_owner(&staged.home, txn),
            Some(other)
        );
        assert!(matches!(
            crate::update_apply::take_the_window(&staged.home, TxnId::new([0x77; 16]), me),
            Window::Unknown(_)
        ));
    }

    /// RED (U-34) — **the hand-over's budget is a program's first start, not a
    /// session write**: at least twice W9's measured 5.9 s, and no more than a
    /// quarter of the applier's own wait for O's lock, so an applier that
    /// does start is never timed out by O's slowness.
    ///
    /// MUTATION: set `quit::HANDOFF_DEADLINE` back to 3 s.
    #[test]
    fn the_hand_over_budget_covers_a_first_start_of_a_new_executable() {
        let w9 = Duration::from_millis(5_900);
        assert!(crate::quit::HANDOFF_DEADLINE >= w9 * 2);
        assert!(
            crate::quit::HANDOFF_DEADLINE * 4 <= crate::update_apply::Limits::PRODUCT.old_within
        );
    }
}
