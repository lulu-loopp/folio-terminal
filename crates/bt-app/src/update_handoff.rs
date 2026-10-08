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
//! opens the window is decided by one election, never by who is alive
//! (`H\<txn>\owner.lock`, with `update_apply::OWNER_FILE` as its durable
//! mark): the hand-over clears the mark
//! before `Handoff` is written ([`perform`]); the applier takes it as soon as
//! it knows its transaction; O, at the process's very end — after the loop,
//! once it has let go of its data directory's claim ([`leave_armed`]) — takes
//! it too, and starts Folio only if it got it ([`Leaving`]). A holder whose
//! mark write fails retains the election lock and still owns. So an applier
//! that took the election opens the window itself, and O starts nothing; an
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
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use bt_platform::admission::{WaitToken, WorkerCtx, doors};

use bt_platform::install_flip::Running;
use bt_platform::install_txn::{self, Held};

use bt_platform::file_reads::{self, Lane};

use crate::cli;
use crate::update_apply::{ExitGuard, Leave, Left, Outgoing, Window};
use crate::update_txn::{Class, Event, HeaderOutcome, Home, Journal, Nonce, Refusal, Role, TxnId};

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
    /// **Whether a start just made was acknowledged** (U-34, round 2): a
    /// Folio holds the data directory `data` within
    /// `update_apply::ACKNOWLEDGED_WITHIN`, asked through `worker`'s wait door
    /// (`update_apply::claimed_within`).
    fn acknowledged(&mut self, worker: Option<&WorkerCtx>, data: &Path) -> bool;
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

    fn acknowledged(&mut self, worker: Option<&WorkerCtx>, data: &Path) -> bool {
        crate::update_apply::claimed_within(worker, data, crate::update_apply::ACKNOWLEDGED_WITHIN)
    }
}

/// **The panic hook's spawner** (round 2, finding 5): the same start, taken as
/// delivered at once — the hook waits for nothing, enumerates nothing and
/// opens no log; the hook's own message box is the window this crash owes.
struct Unwaited;

impl Spawner for Unwaited {
    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<Running> {
        Detached.spawn_detached(program, args)
    }

    fn acknowledged(&mut self, _worker: Option<&WorkerCtx>, _data: &Path) -> bool {
        true
    }
}

/// **Which copy of Folio a start names** (U-39): the rescue copy under the
/// installation home, or the installed one — the two a hand-over and O's exit
/// guard start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FolioCopy {
    Rescue,
    Installed,
}

impl FolioCopy {
    /// The copy `program` is: under `home`, the rescue copy; anywhere else
    /// (or with no home to compare), the installed one.
    #[must_use]
    pub(crate) fn of(program: &Path, home: Option<&Home>) -> Self {
        if home.is_some_and(|home| program.starts_with(home.root())) {
            Self::Rescue
        } else {
            Self::Installed
        }
    }

    fn said(self) -> &'static str {
        match self {
            Self::Rescue => "the rescue copy",
            Self::Installed => "the installed copy",
        }
    }
}

/// **This process's starts through the update's spawner, counted** (U-39):
/// how many were made and how many of them were refused. One per process in
/// the product ([`STARTS`]); a test keeps its own.
#[derive(Debug, Default)]
pub(crate) struct StartCount {
    made: u32,
    refused: u32,
}

impl StartCount {
    /// No start made yet.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            made: 0,
            refused: 0,
        }
    }
}

/// The product's count: every start of a hand-over and of O's exit guard.
static STARTS: Mutex<StartCount> = Mutex::new(StartCount::new());

/// **One start through `spawner`, counted in `starts`, and said when it
/// matters** (U-39; 0.4.6's D-5: `os error 50` from the applier's start, and
/// from every later start of the same process, with no cause on record).
///
/// A refused start says one line with everything a recurrence needs: which of
/// this process's starts it was, the program and which copy it is, the exact
/// error with its raw code, the image's state on the disk (whether it is
/// there, its size and attributes, what its signature says), and this
/// process's own state a start depends on
/// ([`bt_platform::start_conditions`]: console, the inherited standard
/// handles, working directory, creation flags). Once a start was refused,
/// every later start says one line too, whatever its outcome, so "every later
/// start failed" reads off the log. A start before any refusal says nothing.
/// Nothing is changed about the start itself.
pub(crate) fn counted_start(
    starts: &Mutex<StartCount>,
    spawner: &mut dyn Spawner,
    program: &Path,
    args: &[OsString],
    copy: FolioCopy,
    say: &mut dyn FnMut(&str),
) -> io::Result<Running> {
    let started = spawner.spawn_detached(program, args);
    let (made, refused_before) = match starts.lock() {
        Ok(mut starts) => {
            starts.made = starts.made.saturating_add(1);
            let before = starts.refused;
            if started.is_err() {
                starts.refused = starts.refused.saturating_add(1);
            }
            (starts.made, before)
        }
        // A panic elsewhere poisoned the count: the start is not said.
        Err(_) => return started,
    };
    match &started {
        Err(error) => say(&refused_line(made, refused_before, program, copy, error)),
        Ok(child) if refused_before > 0 => say(&format!(
            "BT_UPDATE_SPAWN start {made} of this process succeeded after {refused_before} refused: {} ({}), pid {}",
            program.display(),
            copy.said(),
            child.pid
        )),
        Ok(_) => {}
    }
    started
}

/// **The line a refused start says** — see [`counted_start`].
fn refused_line(
    made: u32,
    refused_before: u32,
    program: &Path,
    copy: FolioCopy,
    error: &io::Error,
) -> String {
    let code = error.raw_os_error().map_or_else(
        || String::from("no system code"),
        |code| format!("system code {code}"),
    );
    let image = match std::fs::symlink_metadata(program) {
        Err(error) => format!("not readable ({error})"),
        Ok(meta) => {
            let signature = match bt_platform::trust::verify_sidecar(program) {
                Ok(signed) => format!("signed by {}", signed.subject),
                Err(refusal) => format!("signature {refusal}"),
            };
            format!(
                "{}, {} bytes, {}, {signature}",
                if meta.file_type().is_symlink() {
                    "a link"
                } else if meta.is_file() {
                    "a file"
                } else {
                    "not a file"
                },
                meta.len(),
                bt_platform::attributes_of(&meta)
            )
        }
    };
    format!(
        "BT_UPDATE_SPAWN start {made} of this process was refused ({refused_before} refused before it): {} ({}): {error} [{:?}, {code}]; the image: {image}; this process: {}",
        program.display(),
        copy.said(),
        error.kind(),
        bt_platform::start_conditions()
    )
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
pub(crate) fn perform(job: HandoffJob) -> HandedOff {
    perform_counted(job, &STARTS, &mut |line| crate::diagnostics::note(line))
}

/// [`perform`], with the start counted in `starts` and every line said to
/// `say` — the product's are this process's count and `diagnostics.log`.
pub(crate) fn perform_counted(
    mut job: HandoffJob,
    starts: &Mutex<StartCount>,
    say: &mut dyn FnMut(&str),
) -> HandedOff {
    let began = Instant::now();
    // No window's mark of an earlier attempt may stand once `Handoff` is on
    // the disk: the applier this hand-over starts takes a fresh one (U-34).
    if let Err(why) = crate::update_apply::clear_the_window(&job.owner) {
        return HandedOff::NotRecorded(format!("the window's mark: {why}"));
    }
    if let Err(failure) = install_txn::durable_write(&job.journal, &job.handoff) {
        say(&took_line(job.txn, began.elapsed(), None));
        return HandedOff::NotRecorded(failure.to_string());
    }
    let written = began.elapsed();
    let spawned = Instant::now();
    let started = counted_start(
        starts,
        job.spawner.as_mut(),
        &job.program,
        &job.args,
        FolioCopy::Rescue,
        say,
    );
    say(&took_line(job.txn, written, Some(spawned.elapsed())));
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
    pub(crate) transaction: Option<(Home, TxnId)>,
    /// The data directory O held: the Folio it starts takes it, which is the
    /// start's acknowledgement.
    pub(crate) data: PathBuf,
    /// **The applier O started**, by pid and start instant, once the
    /// hand-over answered `Started` ([`record_the_applier`]): O's end waits for
    /// its decision before O's own election (round 6).
    pub(crate) applier: Option<Running>,
    /// How long O's end waits for that decision ([`APPLIER_MARK_WITHIN`]).
    pub(crate) applier_within: Duration,
}

/// **How long O's end waits for the applier it started to take the window's
/// mark** (U-34, round 6): until the mark names that applier, or it is gone,
/// or this passes. The clean VM measured 0.8 s from the applier's start to its
/// mark (the start itself, 839 ms, had already returned: the first scan of the
/// new rescue executable is behind it); 15 s is the hand-over's own budget for
/// a first start, so an applier that started within it is waited for as long
/// again for its first decision. Only an applier alive without a mark past it
/// loses the duty to O.
pub(crate) const APPLIER_MARK_WITHIN: Duration = Duration::from_secs(15);

impl Leaving {
    /// The transaction `txn` in `home`, handed over or about to be, by a
    /// process whose data directory is `data`.
    #[must_use]
    pub(crate) fn over(home: &Home, txn: TxnId, data: &Path) -> Self {
        Self {
            transaction: Some((home.clone(), txn)),
            data: data.to_path_buf(),
            applier: None,
            applier_within: APPLIER_MARK_WITHIN,
        }
    }

    /// The same, after the hand-over started `applier`: O's end waits up to
    /// `within` for its decision.
    #[must_use]
    pub(crate) fn after_applier(mut self, applier: Running, within: Duration) -> Self {
        self.applier = Some(applier);
        self.applier_within = within;
        self
    }

    /// No transaction to read: O starts Folio plainly.
    #[must_use]
    pub(crate) fn nothing_staged(data: &Path) -> Self {
        Self {
            transaction: None,
            data: data.to_path_buf(),
            applier: None,
            applier_within: APPLIER_MARK_WITHIN,
        }
    }

    /// **Leave, through the exit guard** (`update_apply::ExitGuard`), as `me`:
    /// the window's mark taken ([`crate::update_apply::take_the_window`]) — a
    /// live applier that has it opens the window, and nothing is started
    /// here; otherwise `program`, the installed build (this process's own
    /// executable), is started with what the header names.
    pub(crate) fn leave(
        self,
        me: Running,
        program: &Path,
        spawner: &mut dyn Spawner,
        worker: Option<&WorkerCtx>,
    ) -> Left {
        let home = self.transaction.as_ref().map(|(home, _)| home.clone());
        let mut guard = ExitGuard::new(OldLeave {
            program,
            spawner,
            home: home.as_ref(),
            data: &self.data,
            worker,
            election_failure: None,
        });
        // **The applier O started decides first** (round 6): O's end waits,
        // on its worker's wait door, until the mark names that applier, or it
        // is gone, or [`Leaving::applier_within`] passes — O must not take the
        // duty from an applier it has just started and that is still coming.
        // The panic road (no worker) waits for nothing.
        if let (Some((home, txn)), Some(applier), Some(worker)) =
            (&self.transaction, self.applier, worker)
        {
            let until = Instant::now() + self.applier_within;
            loop {
                if crate::update_apply::window_owner(home, *txn) == Some(applier)
                    || !bt_platform::install_flip::still_running(applier)
                {
                    break;
                }
                let left = until.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    crate::diagnostics::note(&format!(
                        "Folio: the applier {} took no window mark within {} s; this process takes the duty",
                        applier.pid,
                        self.applier_within.as_secs()
                    ));
                    break;
                }
                bt_platform::wait::sleep_within(worker, Duration::from_millis(50).min(left));
            }
        }
        if let Some((home, txn)) = &self.transaction {
            let window = crate::update_apply::take_the_window_within(
                worker,
                home,
                *txn,
                me,
                // The panic road (no worker) waits for nothing.
                if worker.is_some() {
                    crate::update_apply::ELECTION_WITHIN
                } else {
                    Duration::ZERO
                },
                Outgoing,
            );
            match window {
                Window::Mine(duty) => {
                    if let Some(warning) = duty.warning() {
                        crate::diagnostics::note(&format!(
                            "Folio: the outgoing build owns the window duty; {warning}"
                        ));
                    }
                    guard.owns_window(duty);
                }
                Window::Theirs(owner) => guard.not_mine(Some(owner.pid)),
                Window::Refused(refusal) => {
                    crate::diagnostics::note(&format!(
                        "Folio: the outgoing build's window election was refused: {}",
                        refusal.why()
                    ));
                    if refusal.outgoing_keeps_duty() {
                        guard.inner().election_failure = Some(refusal.why().to_owned());
                    } else {
                        guard.not_mine(None);
                    }
                }
            }
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
    data: &'a Path,
    worker: Option<&'a WorkerCtx>,
    /// An election O could not enter. O keeps its armed duty and the old
    /// build is told the update is incomplete even when the journal's class
    /// alone would not have selected the failure words.
    election_failure: Option<String>,
}

impl Leave for OldLeave<'_> {
    fn say(&mut self, line: &str) {
        crate::diagnostics::note(line);
    }

    /// A journal this build cannot read whole is read by its header's frozen
    /// class when the header reads, and otherwise as `destructive` (0.4.8 E1,
    /// [`crate::update_txn::Role::OutgoingExit`]): the start then continues past it with the
    /// card, never plainly.
    fn opening(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
        let words =
            self.home
                .filter(|home| {
                    self.election_failure.is_some()
                        || file_reads::read(Lane::UpdateJournal, home.journal())
                            .ok()
                            .is_some_and(|bytes| {
                                Role::OutgoingExit.sight(&bytes).acting_header().is_none_or(
                                    |header| {
                                        header.class == Class::Destructive
                                            || header.outcome == HeaderOutcome::RolledBack
                                    },
                                )
                            })
                })
                .map(|home| crate::update_apply::failed_words(home).to_vec())
                .unwrap_or_default();
        Some((self.program.to_path_buf(), words))
    }

    /// Counted and said as the hand-over's start is ([`counted_start`]) —
    /// except on the panic road (no worker), which opens no log.
    fn start(&mut self, program: &Path, words: &[OsString]) -> io::Result<()> {
        if self.worker.is_none() {
            return self.spawner.spawn_detached(program, words).map(drop);
        }
        counted_start(
            &STARTS,
            self.spawner,
            program,
            words,
            FolioCopy::of(program, self.home),
            &mut |line| crate::diagnostics::note(line),
        )
        .map(drop)
    }

    /// The rescue copy the journal names, with `--update-failed`: O's own
    /// image, copied, whose own home holds no journal — the envelope's, when
    /// this build cannot read the journal whole (0.4.8 E1).
    fn fallback(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
        let home = self.home?;
        let bytes = file_reads::read(Lane::UpdateJournal, home.journal()).ok()?;
        let header = Role::OutgoingExit.sight(&bytes).acting_header()?;
        Some((
            home.rescue_program(&header.rescue),
            crate::update_apply::failed_words(home).to_vec(),
        ))
    }

    fn acknowledged(&mut self) -> bool {
        self.spawner.acknowledged(self.worker, self.data)
    }

    /// Nothing is shown from the worker the guard runs on: the process's main
    /// thread shows the failure window once the guard has answered
    /// ([`leave_armed`]), so the window outlives no wait.
    fn show_here(&mut self, why: &str) {
        crate::diagnostics::note(&format!(
            "Folio: no start after the update was delivered ({why})"
        ));
    }
}

/// **What the loop reads of a hand-over it sent** (U-34): nothing yet, or its
/// end and the line to note.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Looked {
    /// No answer, and the deadline has not passed: look again next turn.
    Waiting,
    /// The answer came, or the deadline passed without one; the applier it
    /// started, if it said so.
    Over {
        line: String,
        applier: Option<Running>,
    },
}

/// **One look at the hand-over's answer** for transaction `txn`, `overdue`
/// once [`crate::quit::HANDOFF_DEADLINE`] has passed. Never waits.
pub(crate) fn look(answer: Option<&Receiver<HandedOff>>, txn: TxnId, overdue: bool) -> Looked {
    match answer.and_then(|answer| answer.try_recv().ok()) {
        Some(handed) => Looked::Over {
            line: handed.line(txn),
            applier: match handed {
                HandedOff::Started { applier } => Some(applier),
                _ => None,
            },
        },
        None if overdue => Looked::Over {
            line: format!(
                "Folio: update {txn}'s hand-over did not answer in time; the window's mark decides \
                 who opens Folio as this process leaves"
            ),
            applier: None,
        },
        None => Looked::Waiting,
    }
}

/// **What this process's way out owes after *Restart to update*** — set by
/// the loop ([`arm`]), spent once at the process's end or in its panic
/// ([`leave_armed`]).
static ARMED: Mutex<Option<Leaving>> = Mutex::new(None);

/// **Disarm O's exit guard**: the quit after *Restart to update* was
/// abandoned, and this process stays with its windows.
pub(crate) fn disarm() {
    if let Ok(mut armed) = ARMED.lock() {
        *armed = None;
    }
}

/// **Record the applier the hand-over started** in O's armed exit guard, so
/// O's end waits for its decision (round 6).
pub(crate) fn record_the_applier(applier: Running) {
    if let Ok(mut armed) = ARMED.lock()
        && let Some(leaving) = armed.take()
    {
        *armed = Some(leaving.after_applier(applier, APPLIER_MARK_WITHIN));
    }
}

/// **Arm O's exit guard** with the transaction it hands over.
pub(crate) fn arm(leaving: Leaving) {
    if let Ok(mut armed) = ARMED.lock() {
        *armed = Some(leaving);
    }
}

/// **How long the process's end waits for its exit guard** (§5.3 row 29): the
/// wait for the applier's decision ([`APPLIER_MARK_WITHIN`], 15 s), the
/// election's lock (5 s), the guard's two starts each with its acknowledgement
/// (20 s each), and a margin.
pub(crate) const LEAVE_WITHIN: Duration = Duration::from_secs(75);

/// **O's exit guard, spent** (U-34) — an owner-thread door (`doors::UpdateLeave`,
/// §5.3 row 29), admitted once at the very end of `fn main`: the loop has
/// returned and the session's sentinel is gone. First this process lets go of
/// its data directory's claim, so the start it makes is the writer and not a
/// launch handed back to a Folio that is leaving; then the exit guard runs on
/// a worker of its own — its acknowledgement waits through that worker's wait
/// door — and this thread waits for its answer, bounded by [`LEAVE_WITHIN`].
/// When no start was delivered, this thread shows the failure window itself
/// before the process ends. `None` when nothing was armed.
pub(crate) fn leave_armed(_token: WaitToken<'_, doors::UpdateLeave>) -> Option<Left> {
    let leaving = ARMED.lock().ok()?.take()?;
    crate::persist::let_go_of_every_claim();
    let home = leaving.transaction.as_ref().map(|(home, _)| home.clone());
    let left = match std::env::current_exe() {
        Err(error) => Left::ShownHere(format!("this program cannot be named: {error}")),
        Ok(program) => {
            let me = crate::update_apply::this_process();
            let (answer, answered) = mpsc::channel();
            match bt_platform::spawn_at_priority(
                "folio-update-leave",
                bt_platform::ThreadPriority::BelowNormal,
                move |worker| {
                    let _ = answer.send(leaving.leave(me, &program, &mut Detached, Some(worker)));
                },
            ) {
                Ok(_) => answered.recv_timeout(LEAVE_WITHIN).unwrap_or_else(|_| {
                    Left::ShownHere(String::from("the exit guard did not answer in time"))
                }),
                Err(error) => Left::ShownHere(format!("no worker for the exit guard: {error}")),
            }
        }
    };
    if matches!(left, Left::ShownHere(_)) {
        bt_platform::standalone_alert(
            crate::APP_NAME,
            &crate::update_apply::failure_text(home.as_ref()),
        );
    }
    crate::diagnostics::note(&format!("Folio: leaving after an update: {}", left.said()));
    Some(left)
}

/// **O's exit guard in its panic hook** (U-34, round 2, finding 5): the
/// deliberately small road — never waits for a lock another thread holds (the
/// armed slot and the claim table are only tried), takes the window's mark,
/// makes one start and takes it as delivered: no wait, no process list, no
/// log. The hook's own message box is this crash's window; the process's
/// ordinary end ([`leave_armed`]) carries the guarantee. `None` when nothing
/// was armed, or the slot is held.
pub(crate) fn leave_in_panic() -> Option<Left> {
    let leaving = ARMED.try_lock().ok()?.take()?;
    crate::persist::let_go_of_every_claim();
    let program = std::env::current_exe().ok()?;
    Some(leaving.leave(
        crate::update_apply::this_process(),
        &program,
        &mut Unwaited,
        None,
    ))
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
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::{Duration, Instant};

    use bt_platform::HostPlatform;
    use bt_platform::install_flip::Running;
    use bt_platform::install_txn::{self, Hold};

    #[cfg(windows)]
    use super::Detached;
    use super::{
        FolioCopy, HandedOff, HandoffJob, Leaving, Looked, Spawner, Staged, StartCount,
        apply_command_line, counted_start, look, perform, perform_counted,
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

    #[cfg(windows)]
    #[test]
    fn detached_handoff_helper_waits() {
        if !std::env::args_os().any(|argument| argument == "--exact") {
            return;
        }
        std::thread::park();
    }

    /// RED mutation: make the product's `Detached::spawn_detached` retain a
    /// `ProbeChild`; dropping that owner ends this deliberately handed-off
    /// process before the zero-duration kernel observation.
    #[cfg(windows)]
    #[test]
    fn a_real_update_handoff_is_not_probe_contained() {
        struct Started {
            running: Running,
            image: PathBuf,
        }
        impl Drop for Started {
            fn drop(&mut self) {
                let _ = bt_platform::install_flip::ask(
                    self.running,
                    &[&self.image],
                    bt_platform::install_flip::Ask::End,
                );
            }
        }

        let image = std::env::current_exe().expect("this test executable");
        let arguments = [
            OsString::from("--exact"),
            OsString::from("update_handoff::tests::detached_handoff_helper_waits"),
            OsString::from("--nocapture"),
        ];
        let running = Detached
            .spawn_detached(&image, &arguments)
            .expect("start through the product update hand-off");
        let started = Started { running, image };
        assert!(
            bt_platform::install_flip::still_running(started.running),
            "the product hand-off dropped a containment owner over its result"
        );
    }

    /// PIN (U-32) — **every road process shows its failure window with the box
    /// a process with no application can raise, and none of them with the
    /// application's box.**
    ///
    /// U-34's last resort — no start was delivered, so the guard's own process
    /// shows *Update incomplete.* and the folder — is `Leave::show_here` in the
    /// applier and the recovery build, and `leave_armed` in the old build. On
    /// Windows both boxes are `MessageBoxW`. On macOS `bt_platform::message_box`
    /// is AppKit's alert, which a windowless process (no `NSApplication`, a
    /// worker thread) cannot raise: it writes two lines to the log instead, so
    /// the macOS twin of the checklist's D-5 row — every start refused — ended
    /// with no window at all. `bt_platform::standalone_alert` is Core
    /// Foundation's box there, raised from any thread of any process.
    ///
    /// MUTATION: call `bt_platform::message_box` in any road `show_here` (or in
    /// `leave_armed`) again.
    #[test]
    fn every_road_process_shows_its_failure_window_without_an_application() {
        use bt_source::{Found, Index, Search, View, needle};
        let index = Index::of_package("bt-app");
        let callers = |found: Found| -> Vec<String> {
            let mut names: Vec<String> = found
                .in_the_product(index)
                .owners(index)
                .into_keys()
                .filter(|identity| {
                    [
                        "crate::update_apply_macos",
                        "crate::update_apply_windows",
                        "crate::update_handoff",
                        "crate::update_recover",
                    ]
                    .contains(&identity.module_path.as_str())
                })
                .map(|identity| identity.name)
                .collect();
            names.sort();
            names.dedup();
            names
        };
        let search = |needle| {
            index
                .search(&Search::new(needle, View::CodeKeepingLiterals))
                .unwrap_or_else(|failure| panic!("{failure}"))
        };
        let application_boxes = callers(search(needle!("bt_platform::message_box(")));
        for road in ["show_here", "leave_armed"] {
            assert!(
                !application_boxes.iter().any(|name| name == road),
                "`{road}` raises the application's box, which a road process on macOS cannot: \
                 {application_boxes:?}"
            );
        }
        assert_eq!(
            callers(search(needle!("bt_platform::standalone_alert("))),
            ["leave_armed", "show_here"],
            "the road processes' failure windows"
        );
    }

    const TXN: [u8; 16] = [0x21; 16];

    fn nonce() -> Nonce {
        Nonce::new([0x5a; 32])
    }

    /// A folder of its own under the temporary directory, removed on drop.
    struct Folder(PathBuf);

    impl Folder {
        fn new(tag: &str) -> Self {
            let path = bt_testpath::temp_path(&format!("bt-update-handoff-{tag}"));
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

        fn acknowledged(
            &mut self,
            _worker: Option<&bt_platform::admission::WorkerCtx>,
            _data: &Path,
        ) -> bool {
            true
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

    /// A spawner that stands for the system: it refuses every start with
    /// `code` until told to accept, and then answers this test's own process.
    struct Refusing {
        code: Option<i32>,
    }

    impl Spawner for Refusing {
        fn spawn_detached(&mut self, _program: &Path, _args: &[OsString]) -> io::Result<Running> {
            match self.code {
                Some(code) => Err(io::Error::from_raw_os_error(code)),
                None => Ok(crate::update_apply::this_process()),
            }
        }

        fn acknowledged(
            &mut self,
            _worker: Option<&bt_platform::admission::WorkerCtx>,
            _data: &Path,
        ) -> bool {
            true
        }
    }

    /// RED (U-39) — **an applier's start the system refuses says one line
    /// that names the exact error, the program and which copy it is, the
    /// image's state on the disk, this process's own state a start depends
    /// on, and which of this process's starts it was; and every later start
    /// of the same process says how it went.**
    ///
    /// 0.4.6's clean machine saw `os error 50` from the applier's start on A3–A5
    /// (D-5), and from every later start of that process, and nothing on
    /// record could say why: the hand-over's line held the error's text alone.
    /// The start here is refused by the test's stand-in with the same code,
    /// through the real hand-over, over a real rescue image on the disk.
    ///
    /// MUTATION: in `counted_start`, say nothing for a refused start (drop the
    /// `refused_line` arm) — no `BT_UPDATE_SPAWN` line is said.
    #[test]
    fn a_refused_applier_start_says_the_error_the_image_and_this_process_and_every_later_start_says_how_it_went()
     {
        let folder = Folder::new("spawn-refused");
        let staged = staged(&folder);
        let rescue = staged.home.rescue_program(&staged.journal.rescue);
        std::fs::write(&rescue, b"MZ not really a program").expect("the rescue image");
        let starts = Mutex::new(StartCount::new());
        let mut said = Vec::new();
        let answered = perform_counted(
            HandoffJob::new(&staged, nonce(), Box::new(Refusing { code: Some(50) }))
                .expect("Prepared → Handoff"),
            &starts,
            &mut |line| said.push(line.to_owned()),
        );
        assert!(
            matches!(&answered, HandedOff::NotStarted { .. }),
            "{answered:?}"
        );
        let refused: Vec<&String> = said
            .iter()
            .filter(|line| line.starts_with("BT_UPDATE_SPAWN"))
            .collect();
        assert_eq!(refused.len(), 1, "one line at the refusal: {said:?}");
        let line = refused[0];
        for field in [
            "start 1 of this process was refused (0 refused before it)",
            &rescue.display().to_string(),
            "(the rescue copy)",
            &io::Error::from_raw_os_error(50).to_string(),
            "system code 50",
            "the image: a file, 23 bytes,",
            "signature ",
            "this process: ",
            "standard handles inherited",
            "working directory ",
            "creation flags ",
        ] {
            assert!(line.contains(field), "{field:?} in {line}");
        }
        if bt_platform::host_platform() == HostPlatform::Windows {
            for field in ["attributes 0x", "creation flags 0x08000000", "(in "] {
                assert!(line.contains(field), "{field:?} in {line}");
            }
            assert!(
                line.contains("console attached") || line.contains("no console"),
                "{line}"
            );
        }

        // The same process's later starts: refused again, then accepted.
        let installed = folder.0.join("folio.exe");
        for (code, expected) in [
            (
                Some(50),
                "start 2 of this process was refused (1 refused before it)",
            ),
            (None, "start 3 of this process succeeded after 2 refused"),
        ] {
            said.clear();
            let _ = counted_start(
                &starts,
                &mut Refusing { code },
                &installed,
                &[],
                FolioCopy::of(&installed, Some(&staged.home)),
                &mut |line| said.push(line.to_owned()),
            );
            assert_eq!(said.len(), 1, "{said:?}");
            assert!(said[0].contains(expected), "{expected:?} in {}", said[0]);
            assert!(said[0].contains("(the installed copy)"), "{}", said[0]);
        }
    }

    /// PIN (U-39) — **a start before any refusal says nothing**: the count
    /// is kept, and the log is not.
    ///
    /// MUTATION: in `counted_start`, drop the `refused_before > 0` guard — the
    /// first, accepted start says a line.
    #[test]
    fn a_start_before_any_refusal_says_nothing() {
        let starts = Mutex::new(StartCount::new());
        let mut said = Vec::new();
        let program = std::env::temp_dir().join("folio.exe");
        assert!(
            counted_start(
                &starts,
                &mut Refusing { code: None },
                &program,
                &[],
                FolioCopy::Installed,
                &mut |line| said.push(line.to_owned()),
            )
            .is_ok()
        );
        assert!(said.is_empty(), "{said:?}");
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
        /// Every start dies before it takes the data directory.
        die: bool,
    }

    impl Spawner for Starts {
        fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<Running> {
            self.calls.push((program.to_path_buf(), args.to_vec()));
            Ok(Running { pid: 0, started: 0 })
        }

        fn acknowledged(
            &mut self,
            _worker: Option<&bt_platform::admission::WorkerCtx>,
            _data: &Path,
        ) -> bool {
            !self.die
        }
    }

    /// **A spawner held on its first start until the test lets it go** — the
    /// first start of a new executable that a scanner reads whole (W9).
    struct Held {
        gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
        asked: Arc<Mutex<Asked>>,
        journal: PathBuf,
        entered: mpsc::Sender<()>,
    }

    impl Spawner for Held {
        fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<Running> {
            self.entered.send(()).expect("the test is listening");
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

        fn acknowledged(
            &mut self,
            _worker: Option<&bt_platform::admission::WorkerCtx>,
            _data: &Path,
        ) -> bool {
            true
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
                Looked::Over { line, .. } => return line,
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
        let (entered, start_entered) = mpsc::channel();
        let held = Held {
            gate: Arc::clone(&gate),
            asked: Arc::clone(&asked),
            journal: staged.home.journal(),
            entered,
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
        let line = match look(
            Some(&answer),
            TxnId::new(TXN),
            quit.handoff_is_overdue(sent + crate::quit::HANDOFF_DEADLINE),
        ) {
            Looked::Over { line, .. } => line,
            other => panic!("an overdue hand-over did not end: {other:?}"),
        };
        assert!(line.contains("did not answer in time"), "{line}");
        quit.handed_off();
        assert_eq!(
            (quit.step(), quit.handoff()),
            (QuitStep::Exit, Handoff::Done)
        );

        // W9's instant: `Handoff` has landed and the applier's start is stuck.
        start_entered
            .recv()
            .expect("the held start says Handoff is durable before it begins");
        assert_eq!(on_disk(&staged.home).body.phase.kind(), PhaseKind::Handoff);
        // O leaves: nobody took the window's mark, so O takes it and starts.
        let txn = TxnId::new(TXN);
        let old = crate::update_apply::this_process();
        let installed = folder.0.join("folio.exe");
        let mut starts = Starts::default();
        let left =
            Leaving::over(&staged.home, txn, &folder.0).leave(old, &installed, &mut starts, None);
        assert_eq!(left, Left::Started(installed.clone()));
        let failed = crate::update_apply::failed_words(&staged.home).to_vec();
        assert_eq!(starts.calls, vec![(installed.clone(), failed)]);

        // The late start goes through; the applier finds O's live mark.
        *gate.0.lock().expect("the gate") = true;
        gate.1.notify_all();
        let late = answer
            .recv()
            .expect("the worker answers after it is released");
        assert!(matches!(late, HandedOff::Started { .. }), "{late:?}");
        assert_eq!(asked.lock().expect("the record").calls.len(), 1);
        let late_applier = Running { pid: 1, started: 1 };
        assert_eq!(
            crate::update_apply::take_the_window(
                None,
                &staged.home,
                txn,
                late_applier,
                Instant::now() + crate::update_apply::ELECTION_WITHIN
            ),
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
        assert!(
            crate::update_apply::take_the_window(
                None,
                &staged.home,
                txn,
                applier,
                Instant::now() + crate::update_apply::ELECTION_WITHIN
            )
            .is_mine()
        );
        let mut starts = Starts::default();
        assert_eq!(
            Leaving::over(&staged.home, txn, &folder.0).leave(
                Running { pid: 1, started: 1 },
                &installed,
                &mut starts,
                None,
            ),
            Left::NotMine(Some(applier.pid))
        );
        assert!(starts.calls.is_empty(), "{:?}", starts.calls);
    }

    /// RED (U-34, round 2) — **the window's mark is taken by exactly one live
    /// process**: created for the first taker; refused to a second while the
    /// first runs (by pid and start instant); taken over from an owner that no
    /// longer runs. A contender that cannot enter refuses; a holder whose
    /// mark cannot be read or replaced remains the one owner by retaining
    /// `owner.lock`.
    ///
    /// MUTATION: in `update_apply::take_the_window_within`, replace the mark
    /// whatever it names (a second live taker then gets it too).
    #[test]
    fn the_windows_mark_is_taken_by_exactly_one_live_process() {
        let folder = Folder::new("mark");
        let staged = staged(&folder);
        let txn = TxnId::new(TXN);
        let me = crate::update_apply::this_process();
        let other = Running { pid: 1, started: 1 };
        let take = |who| {
            crate::update_apply::take_the_window(
                None,
                &staged.home,
                txn,
                who,
                Instant::now() + crate::update_apply::ELECTION_WITHIN,
            )
        };
        assert!(take(me).is_mine());
        assert!(take(me).is_mine());
        assert_eq!(take(other), Window::Theirs(me));
        std::fs::write(
            crate::update_apply::owner_path(&staged.home, txn),
            format!("{}:{}", me.pid, me.started.wrapping_add(1)),
        )
        .unwrap();
        assert!(
            take(other).is_mine(),
            "an owner that no longer runs is taken over"
        );
        assert_eq!(
            crate::update_apply::window_owner(&staged.home, txn),
            Some(other)
        );

        std::fs::write(
            crate::update_apply::owner_path(&staged.home, txn),
            format!("{}:{}", me.pid, me.started.wrapping_add(1)),
        )
        .unwrap();
        let mark = crate::update_apply::owner_path(&staged.home, txn);
        if let Ok(scanner) = bt_platform::trust_harness::hold_without_delete_sharing(&mark) {
            let answer = take(me);
            let Window::Mine(duty) = answer else {
                panic!("the holder owns despite a replacement refusal: {answer:?}");
            };
            assert!(
                duty.warning()
                    .is_some_and(|warning| warning.contains("could not be replaced")),
                "the failed mark is named"
            );
            drop(scanner);
        }

        assert!(
            matches!(
                crate::update_apply::take_the_window(
                    None,
                    &staged.home,
                    TxnId::new([0x77; 16]),
                    me,
                    Instant::now() + crate::update_apply::ELECTION_WITHIN
                ),
                Window::Refused(_)
            ),
            "an election that cannot be held proves no owner"
        );
        assert_eq!(
            crate::update_apply::window_holder(&staged.home, TxnId::new([0x77; 16]), me),
            Ok(None),
            "recovery falls back to the mark when owner.lock cannot be opened"
        );

        let mark = crate::update_apply::owner_path(&staged.home, txn);
        std::fs::remove_file(&mark).unwrap();
        std::fs::create_dir(&mark).unwrap();
        let answer = take(me);
        let Window::Refused(refusal) = answer else {
            panic!("an unreadable mark cannot be claimed through: {answer:?}");
        };
        assert!(
            refusal.why().contains("could not be read"),
            "the exhausted read refusal is named: {refusal:?}"
        );
    }

    /// RED (T-UPDATE-LOCK-RACE round 3, R2-2) — **a transient owner-mark
    /// read failure is asked again under the same election lock and budget;
    /// only the successful second read is allowed to decide the duty.** The
    /// injected pause returns immediately, so no clock chooses the result.
    ///
    /// MUTATION: return `Mine` on the first read error, or remove the read
    /// retry in `take_the_window_within_using` (one attempt is observed).
    #[test]
    fn the_window_mark_is_retried_after_a_transient_read_failure() {
        let folder = Folder::new("mark-read-retry");
        let staged = staged(&folder);
        let txn = TxnId::new(TXN);
        let me = crate::update_apply::this_process();
        let mut attempts = 0;
        let answer = crate::update_apply::take_the_window_within_reads_at(
            &staged.home,
            txn,
            me,
            crate::update_apply::JOURNAL_WRITE_WITHIN,
            || {
                attempts += 1;
                if attempts == 1 {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "the scanner still has the mark",
                    ))
                } else {
                    Ok(None)
                }
            },
        );
        assert!(answer.is_mine(), "{answer:?}");
        assert_eq!(attempts, 2);
        assert_eq!(
            crate::update_apply::window_owner(&staged.home, txn),
            Some(me)
        );
    }

    /// RED (T-UPDATE-LOCK-RACE round 2) — **the owner mark uses the journal
    /// writer's retry discipline: a scanner may refuse it repeatedly and the
    /// fourth attempt still records this holder.** The injected pause returns
    /// immediately; no clock or sleep chooses the interleaving.
    ///
    /// MUTATION: remove the `refused_while_open` arm from
    /// `update_apply::retry_within`.
    #[test]
    fn the_window_mark_is_retried_after_three_scanner_refusals() {
        let folder = Folder::new("mark-retry");
        let staged = staged(&folder);
        let txn = TxnId::new(TXN);
        let me = crate::update_apply::this_process();
        let mut attempts = 0;
        let answer = crate::update_apply::take_the_window_within_writes_at(
            &staged.home,
            txn,
            me,
            crate::update_apply::JOURNAL_WRITE_WITHIN,
            |mark, bytes| {
                attempts += 1;
                if attempts <= 3 {
                    Err(
                        crate::update_apply::MarkWriteFailure::before_rename_for_test(
                            "scanner", true,
                        ),
                    )
                } else {
                    std::fs::write(mark, bytes).unwrap();
                    Ok(())
                }
            },
        );
        assert!(answer.is_mine(), "{answer:?}");
        assert_eq!(attempts, 4);
        assert_eq!(
            crate::update_apply::window_owner(&staged.home, txn),
            Some(me)
        );
    }

    /// RED (T-UPDATE-LOCK-RACE round 2) — **a holder whose pre-rename mark
    /// write fails still owns by its held election lock, so a simultaneous
    /// contender refuses and the pair can never be `[Refused, Refused]`.**
    ///
    /// MUTATION: map the injected write failure back to `Window::Refused`, or
    /// drop the duty instead of moving it into `ExitGuard::owns_window`.
    #[test]
    fn a_holder_that_cannot_write_still_owns_and_the_contender_refuses() {
        let folder = Folder::new("mark-write-fails");
        let staged = staged(&folder);
        let txn = TxnId::new(TXN);
        let me = crate::update_apply::this_process();
        let holder = crate::update_apply::take_the_window_within_writes_at(
            &staged.home,
            txn,
            me,
            crate::update_apply::JOURNAL_WRITE_WITHIN,
            |_mark, _bytes| {
                Err(
                    crate::update_apply::MarkWriteFailure::before_rename_for_test(
                        "disk refused the write",
                        false,
                    ),
                )
            },
        );
        let Window::Mine(duty) = holder else {
            panic!("the holder did not own: {holder:?}");
        };

        struct Finished;
        impl crate::update_apply::Leave for Finished {
            fn say(&mut self, _line: &str) {}
            fn opening(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
                None
            }
            fn start(&mut self, _program: &Path, _words: &[OsString]) -> io::Result<()> {
                unreachable!("a live successor means no start")
            }
            fn fallback(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
                None
            }
            fn acknowledged(&mut self) -> bool {
                false
            }
            fn show_here(&mut self, _why: &str) {}
        }

        let mut guard = crate::update_apply::ExitGuard::new(Finished);
        guard.owns_window(duty);
        guard.succeeded_by(Some(me));
        let contender = crate::update_apply::take_the_window_within(
            None,
            &staged.home,
            txn,
            Running { pid: 1, started: 1 },
            Duration::ZERO,
            crate::update_apply::Applier,
        );
        assert_eq!(
            crate::update_apply::window_holder(&staged.home, txn, Running { pid: 1, started: 1 },),
            Ok(Some(crate::update_apply::WindowHolder::Unmarked)),
            "recovery defers to the same live lock authority"
        );
        assert!(
            matches!(&contender, Window::Refused(refusal) if refusal.contended()),
            "{contender:?}"
        );
        assert_eq!(guard.leave(), Left::Succeeded(me.pid));
        assert!(
            matches!(
                crate::update_apply::take_the_window_within(
                    None,
                    &staged.home,
                    txn,
                    Running { pid: 1, started: 1 },
                    Duration::ZERO,
                    crate::update_apply::Applier,
                ),
                Window::Refused(refusal) if refusal.contended()
            ),
            "the product guard retains the lock even after its final delivery"
        );
        drop(guard);
        assert!(
            crate::update_apply::take_the_window_within(
                None,
                &staged.home,
                txn,
                Running { pid: 1, started: 1 },
                Duration::ZERO,
                crate::update_apply::Applier,
            )
            .is_mine(),
            "only dropping the process's exit guard releases the retained lock"
        );
    }

    /// RED (T-UPDATE-LOCK-RACE round 4) — **the outgoing build never reads the
    /// journal to elect: an applier that could not be started leaves the
    /// journal `Abandoned` in O's own hand, and O still opens Folio exactly
    /// once.** O holds the transaction lock while it asks, so no applier road
    /// can have moved the journal; `Abandoned` here is O's record, not a road
    /// taken. An applier asking at the same state does read the journal, and
    /// stands down.
    ///
    /// MUTATION: in `Leaving::leave`, elect as `Applier` (O would read
    /// `Abandoned` as a road taken and start nothing): since E1-a2 the match
    /// over its `Window<Applier>` no longer compiles, because that answer can
    /// say the road was taken.
    #[test]
    fn an_outgoing_build_whose_applier_never_started_still_opens_folio() {
        let folder = Folder::new("applier-never-started");
        let staged = staged(&folder);
        let txn = TxnId::new(TXN);
        let (spawner, _) = recording(&staged, true);
        let answered =
            perform(HandoffJob::new(&staged, nonce(), spawner).expect("Prepared → Handoff"));
        assert!(
            matches!(answered, HandedOff::NotStarted { .. }),
            "{answered:?}"
        );
        assert_eq!(
            on_disk(&staged.home).body.phase.kind(),
            PhaseKind::Abandoned
        );
        assert_eq!(
            crate::update_apply::take_the_window_within(
                None,
                &staged.home,
                txn,
                Running { pid: 2, started: 2 },
                Duration::ZERO,
                crate::update_apply::Applier,
            ),
            Window::RoadTaken(PhaseKind::Abandoned),
            "an applier reads the journal"
        );

        let installed = folder.0.join("folio.exe");
        let mut starts = Starts::default();
        let left = Leaving::over(&staged.home, txn, &folder.0).leave(
            Running { pid: 1, started: 1 },
            &installed,
            &mut starts,
            None,
        );
        assert_eq!(left, Left::Started(installed.clone()));
        assert_eq!(starts.calls.len(), 1, "{:?}", starts.calls);
        assert_eq!(starts.calls[0].0, installed);
    }

    /// RED (T-UPDATE-LOCK-RACE round 3, R2-1) — **every journal phase is on
    /// exactly one side of an applier's unmarked election:** allocation,
    /// preparation and hand-off have not taken the applier road; every later
    /// or terminal phase has.
    ///
    /// MUTATION: classify any phase after `Handoff` as open.
    #[test]
    fn every_journal_phase_decides_whether_an_unmarked_duty_is_open() {
        for phase in PhaseKind::ALL {
            assert_eq!(
                crate::update_apply::window_duty_is_open(phase),
                matches!(
                    phase,
                    PhaseKind::Allocated | PhaseKind::Prepared | PhaseKind::Handoff
                ),
                "{phase:?}"
            );
        }
    }

    /// RED (T-UPDATE-LOCK-RACE round 3, R2-4) — **the election's own lock
    /// wait cannot outlive the applier's existing deadline.** The supplied
    /// clock makes an expired deadline exactly zero and a distant one exactly
    /// `ELECTION_WITHIN`; no elapsed-time assertion is involved.
    ///
    /// MUTATION: return `ELECTION_WITHIN` without taking the minimum with the
    /// applier deadline.
    #[test]
    fn the_window_election_is_bounded_by_the_applier_deadline() {
        let now = Instant::now();
        assert_eq!(
            crate::update_apply::election_within(now - Duration::from_millis(1), now),
            Duration::ZERO
        );
        assert_eq!(
            crate::update_apply::election_within(
                now + crate::update_apply::ELECTION_WITHIN.saturating_mul(2),
                now,
            ),
            crate::update_apply::ELECTION_WITHIN
        );
    }

    /// RED (T-UPDATE-LOCK-RACE round 2) — **both platform appliers re-ask a
    /// contended election inside the same deadline**. The injected pause
    /// releases the first holder, so the second ask takes the duty without a
    /// sleep or a fresh budget.
    ///
    /// MUTATION: in `update_apply::ask_for_the_window_until`, return the first
    /// contended `Window::Refused`.
    #[test]
    fn both_appliers_reask_a_contended_election_within_the_same_budget() {
        let folder = Folder::new("mark-contention-retry");
        let staged = staged(&folder);
        let txn = TxnId::new(TXN);
        let me = crate::update_apply::this_process();
        let mut holder = Some(
            install_txn::try_hold(
                &crate::update_apply::owner_lock_path(&staged.home, txn),
                Hold::Exclusive,
            )
            .unwrap()
            .unwrap(),
        );
        let mut asks = 0;
        let answer = crate::update_apply::take_the_window_for_applier_at(
            Instant::now() + crate::update_apply::ELECTION_WITHIN,
            Duration::from_millis(1),
            || {
                asks += 1;
                crate::update_apply::take_the_window_within(
                    None,
                    &staged.home,
                    txn,
                    me,
                    Duration::ZERO,
                    crate::update_apply::Applier,
                )
            },
            |_| drop(holder.take()),
        );
        assert!(answer.is_mine(), "{answer:?}");
        assert_eq!(asks, 2);
    }

    /// RED (T-UPDATE-LOCK-RACE round 2) — **a directory-flush failure after
    /// rename is an election success**: the visible mark names the holder and
    /// the next contender answers `Theirs`.
    ///
    /// MUTATION: map an `after_rename` failure to `Window::Refused`.
    #[test]
    fn a_mark_that_landed_before_the_directory_flush_still_wins() {
        let folder = Folder::new("mark-landed");
        let staged = staged(&folder);
        let txn = TxnId::new(TXN);
        let me = crate::update_apply::this_process();
        let answer = crate::update_apply::take_the_window_within_writes_at(
            &staged.home,
            txn,
            me,
            crate::update_apply::JOURNAL_WRITE_WITHIN,
            |mark, bytes| {
                std::fs::write(mark, bytes).unwrap();
                Err(
                    crate::update_apply::MarkWriteFailure::after_rename_for_test(
                        "directory flush failed",
                    ),
                )
            },
        );
        assert!(answer.is_mine(), "{answer:?}");
        assert_eq!(
            crate::update_apply::take_the_window_within(
                None,
                &staged.home,
                txn,
                Running { pid: 1, started: 1 },
                Duration::ZERO,
                crate::update_apply::Applier,
            ),
            Window::Theirs(me)
        );
    }

    /// RED (T-UPDATE-LOCK-RACE round 2) — **when no process can even open the
    /// election lock, O still carries the duty it armed and starts the old
    /// build's failure road.** An unentered applier may defer to O; O may not
    /// turn its own refusal into silence.
    ///
    /// MUTATION: in `Leaving::leave`, call `guard.not_mine(None)` for every
    /// `Window::Refused`.
    #[test]
    fn os_unentered_election_still_ends_in_the_failure_road() {
        let folder = Folder::new("old-election-open-fails");
        let staged = staged(&folder);
        let installed = folder.0.join("folio.exe");
        let mut starts = Starts::default();
        let left = Leaving::over(&staged.home, TxnId::new([0x77; 16]), &folder.0).leave(
            crate::update_apply::this_process(),
            &installed,
            &mut starts,
            None,
        );
        assert_eq!(left, Left::Started(installed.clone()));
        assert_eq!(
            starts.calls,
            vec![(
                installed,
                crate::update_apply::failed_words(&staged.home).to_vec()
            )]
        );
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

    /// RED (U-34, round 2) — **a start is acknowledged only by a Folio holding
    /// the data directory**: while a claim on it is held, at once; while none
    /// is, not before the bound runs out.
    ///
    /// MUTATION: in `update_apply::claimed_within`, answer `true` for a claim
    /// this process could take.
    #[test]
    fn a_start_is_acknowledged_only_by_a_folio_holding_the_data_directory() {
        let folder = Folder::new("ack");
        let data = folder.0.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let (held, free) = bt_platform::spawn_at_priority(
            "bt-u34-ack",
            bt_platform::ThreadPriority::BelowNormal,
            move |worker| {
                let within = Duration::from_millis(400);
                let claim = crate::persist::try_claim(&data).unwrap();
                let held = crate::update_apply::claimed_within(Some(worker), &data, within);
                drop(claim);
                // Let go once the platform says so: a child another test is
                // starting may hold a copy of the lock until its exec
                // (`persist::tests::claim_once_let_go`).
                drop(crate::persist::tests::claim_once_let_go(&data));
                let began = Instant::now();
                let free = crate::update_apply::claimed_within(Some(worker), &data, within);
                (held, (free, began.elapsed() >= within))
            },
        )
        .unwrap()
        .join()
        .unwrap();
        assert!(held, "a Folio holds it: acknowledged");
        assert_eq!(free, (false, true), "nobody took it within the bound");
    }

    /// RED (U-34, round 2) — **O's exit guard falls back and, when nothing is
    /// delivered, says so to the process's end**: its own executable started
    /// and never acknowledged, then the rescue copy the journal names, then
    /// `Left::ShownHere` — which `leave_armed` answers with the failure window
    /// on the main thread.
    ///
    /// MUTATION: in `OldLeave::fallback`, answer `None`.
    #[test]
    fn the_old_builds_guard_falls_back_to_the_rescue_copy_and_then_to_a_window_here() {
        let folder = Folder::new("o-falls-back");
        let staged = staged(&folder);
        let txn = TxnId::new(TXN);
        let handoff = staged
            .journal
            .advance(&crate::update_txn::Event::HandedOff { applier: nonce() })
            .unwrap();
        install_txn::durable_write(&staged.home.journal(), &handoff.encode()).unwrap();
        let installed = folder.0.join("folio.exe");
        let mut starts = Starts {
            die: true,
            ..Starts::default()
        };
        let left = Leaving::over(&staged.home, txn, &folder.0).leave(
            crate::update_apply::this_process(),
            &installed,
            &mut starts,
            None,
        );
        assert!(matches!(left, Left::ShownHere(_)), "{left:?}");
        let failed = crate::update_apply::failed_words(&staged.home).to_vec();
        assert_eq!(
            starts.calls,
            vec![
                (installed, failed.clone()),
                (staged.home.rescue_program(&staged.journal.rescue), failed),
            ]
        );
    }

    /// RED (E1; role #15, O's exit, sites H7 `OldLeave::opening` and J10
    /// `OldLeave::fallback`) — **O leaves a journal it cannot read whole by the
    /// header it acts on, and never opens itself plainly over it**: its own
    /// executable with `--update-failed` (it continues past the journal with
    /// the card), then the rescue copy the header names — the envelope's for
    /// an unknown header word — and, when nothing reads, no rescue to name.
    /// The journal is byte for byte as it was.
    ///
    /// MUTATION: in `OldLeave::opening`, read the header alone again
    /// (`Header::parse(&bytes).ok()`): over an unknown header word, or bytes
    /// of which nothing reads, O starts itself plainly.
    #[test]
    fn the_old_build_leaves_what_it_cannot_read_whole_by_its_header() {
        let folder = Folder::new("o-beyond");
        let staged = staged(&folder);
        let txn = TxnId::new(TXN);
        let handoff = staged
            .journal
            .advance(&crate::update_txn::Event::HandedOff { applier: nonce() })
            .unwrap();
        let installed = folder.0.join("folio.exe");
        let failed = crate::update_apply::failed_words(&staged.home).to_vec();
        let rescue = staged.home.rescue_program(&staged.journal.rescue);
        for (index, (what, bytes)) in crate::update_txn::beyond_inputs(&handoff.encode())
            .into_iter()
            .enumerate()
        {
            install_txn::durable_write(&staged.home.journal(), &bytes).unwrap();
            let mut starts = Starts {
                die: true,
                ..Starts::default()
            };
            let left = Leaving::over(&staged.home, txn, &folder.0).leave(
                crate::update_apply::this_process(),
                &installed,
                &mut starts,
                None,
            );
            assert!(matches!(left, Left::ShownHere(_)), "{what}: {left:?}");
            let mut expected = vec![(installed.clone(), failed.clone())];
            if index < 2 {
                expected.push((rescue.clone(), failed.clone()));
            }
            assert_eq!(starts.calls, expected, "{what}");
            assert_eq!(
                std::fs::read(staged.home.journal()).unwrap(),
                bytes,
                "{what}"
            );
        }
    }

    /// RED (U-34, round 2; Codex's review, finding 5) — **O's panic road never
    /// waits for the armed slot another thread holds**: it answers `None` at
    /// once — the hook's own message box is then the crash's window — and
    /// leaves the slot as it was.
    ///
    /// MUTATION: in `leave_in_panic`, `lock()` the slot instead of trying it.
    #[test]
    fn the_panic_road_never_waits_for_a_held_armed_slot() {
        let (held, release) = std::sync::mpsc::channel::<()>();
        let (holding, is_held) = std::sync::mpsc::channel::<()>();
        let holder = bt_platform::spawn_at_priority(
            "bt-u34-armed",
            bt_platform::ThreadPriority::BelowNormal,
            move |_worker| {
                let _slot = super::ARMED.lock().expect("the slot");
                holding.send(()).unwrap();
                let _ = release.recv_timeout(Duration::from_secs(10));
            },
        )
        .unwrap();
        is_held.recv().unwrap();
        let began = Instant::now();
        let left = super::leave_in_panic();
        let took = began.elapsed();
        held.send(()).unwrap();
        holder.join().unwrap();
        assert_eq!(left, None);
        assert!(took < Duration::from_secs(2), "{took:?}");
    }

    /// RED (U-34, round 4; Codex's finding 12) — **a claim the platform will
    /// not answer for is no acknowledgement**: with the data directory's claim
    /// name squatted — `ClaimRefusal::QueryDenied`, "no evidence anybody holds
    /// anything" — `claimed_within` answers `false` at the bound, and a guard
    /// whose start is acknowledged that way falls back and then shows the
    /// failure window itself.
    ///
    /// The squat is the platform's own shape for this refusal
    /// (`bt_platform::trust_harness::squat_the_claim`: another kind of named
    /// kernel object under the claim's name on Windows, a directory where the
    /// lock file goes on Unix).
    ///
    /// MUTATION: in `update_apply::claimed_within`, count every refusal as
    /// held (`Err(_) => return true`).
    #[test]
    fn a_claim_the_platform_will_not_answer_for_is_no_acknowledgement() {
        let folder = Folder::new("query-denied");
        let data = folder.0.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let Ok(squat) = bt_platform::trust_harness::squat_the_claim(&data) else {
            return;
        };
        assert!(
            matches!(
                crate::persist::try_claim(&data),
                Err(bt_platform::instance::ClaimRefusal::QueryDenied(_))
            ),
            "the squat is the refusal that answers nothing"
        );

        struct ByTheClaim<'a> {
            data: &'a Path,
            worker: &'a bt_platform::admission::WorkerCtx,
            starts: usize,
            shown: Vec<String>,
        }
        impl crate::update_apply::Leave for ByTheClaim<'_> {
            fn say(&mut self, _line: &str) {}
            fn opening(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
                Some((PathBuf::from("installed"), Vec::new()))
            }
            fn start(&mut self, _program: &Path, _words: &[OsString]) -> io::Result<()> {
                self.starts += 1;
                Ok(())
            }
            fn fallback(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
                Some((PathBuf::from("rescue"), Vec::new()))
            }
            fn acknowledged(&mut self) -> bool {
                crate::update_apply::claimed_within(
                    Some(self.worker),
                    self.data,
                    Duration::from_millis(300),
                )
            }
            fn show_here(&mut self, why: &str) {
                self.shown.push(why.to_owned());
            }
        }

        let (answered, starts, shown) = bt_platform::spawn_at_priority(
            "bt-u34-denied",
            bt_platform::ThreadPriority::BelowNormal,
            move |worker| {
                let answered = crate::update_apply::claimed_within(
                    Some(worker),
                    &data,
                    Duration::from_millis(300),
                );
                let mut guard = crate::update_apply::ExitGuard::new(ByTheClaim {
                    data: &data,
                    worker,
                    starts: 0,
                    shown: Vec::new(),
                });
                let left = guard.leave();
                assert!(matches!(left, Left::ShownHere(_)), "{left:?}");
                let leave = guard.inner();
                (answered, leave.starts, std::mem::take(&mut leave.shown))
            },
        )
        .unwrap()
        .join()
        .unwrap();
        drop(squat);
        assert!(!answered, "QueryDenied is not a delivery");
        assert_eq!(starts, 2, "the start, then the fallback");
        assert_eq!(shown.len(), 1, "then the failure window, here");
    }

    /// RED (U-34, round 5; Codex's finding 14) — **the election is one
    /// exclusive lock around read-check-replace: a contender that arrives while
    /// another is inside it waits, and then reads what the other left — a live
    /// owner that appeared between its arrival and its turn is `Theirs`, never
    /// overwritten.**
    ///
    /// The deterministic interleaving: the mark names a dead process; the test
    /// takes `owner.lock` (a contender inside the election); a second
    /// contender starts and blocks on the lock; inside, the test writes a live
    /// owner (this process) and lets the lock go; the second contender then
    /// answers `Theirs(live owner)` and the mark still names it. Under the
    /// round-4 ballot scheme the second contender read the dead value without
    /// waiting and overwrote it.
    ///
    /// MUTATION: in `update_apply::take_the_window_within`, skip the lock.
    #[test]
    fn a_contender_waits_for_the_election_and_never_overwrites_a_live_owner() {
        let folder = Folder::new("election-order");
        let staged = staged(&folder);
        let txn = TxnId::new(TXN);
        let me = crate::update_apply::this_process();
        let mark = crate::update_apply::owner_path(&staged.home, txn);
        std::fs::write(&mark, format!("{}:{}", me.pid, me.started.wrapping_add(1))).unwrap();
        let inside = install_txn::try_hold(
            &crate::update_apply::owner_lock_path(&staged.home, txn),
            Hold::Exclusive,
        )
        .unwrap()
        .expect("the test is inside the election");
        let home = staged.home.clone();
        let late = Running { pid: 1, started: 1 };
        let contender = std::thread::spawn(move || {
            crate::update_apply::take_the_window(
                None,
                &home,
                txn,
                late,
                Instant::now() + crate::update_apply::ELECTION_WITHIN,
            )
        });
        std::thread::sleep(Duration::from_millis(300));
        assert!(!contender.is_finished(), "the contender waits for the lock");
        std::fs::write(&mark, format!("{}:{}", me.pid, me.started)).unwrap();
        drop(inside);
        assert_eq!(contender.join().unwrap(), Window::Theirs(me));
        assert_eq!(
            crate::update_apply::window_owner(&staged.home, txn),
            Some(me)
        );
    }

    /// RED (T-UPDATE-LOCK-RACE) — **a contender delayed anywhere inside the
    /// election remains the only winner**: a second contender whose bounded
    /// lock ask ends while the first is paused must refuse the duty, then the
    /// first writes its identity and answers `Mine`.
    ///
    /// The pause is a channel at the seam immediately after the first
    /// contender acquires `owner.lock`; no clock decides the interleaving.
    /// Both contenders see the same stale mark. Before this fix, a bounded
    /// lock refusal was `Mine`, so the deterministic answers were two winners.
    ///
    /// MUTATION: in `update_apply::take_the_window_within_after_lock`, map an
    /// untaken election lock to `Window::Mine`.
    #[test]
    fn a_delayed_contender_inside_the_election_is_the_only_winner() {
        let folder = Folder::new("delayed-election");
        let staged = staged(&folder);
        let txn = TxnId::new(TXN);
        let me = crate::update_apply::this_process();
        let stale = Running {
            pid: me.pid,
            started: me.started.wrapping_add(1),
        };
        std::fs::write(
            crate::update_apply::owner_path(&staged.home, txn),
            format!("{}:{}", stale.pid, stale.started),
        )
        .unwrap();

        let (inside, entered) = std::sync::mpsc::channel();
        let (release, proceed) = std::sync::mpsc::channel();
        let home = staged.home.clone();
        let first = std::thread::spawn(move || {
            crate::update_apply::take_the_window_within_at(
                &home,
                txn,
                me,
                crate::update_apply::ELECTION_WITHIN,
                || {
                    inside.send(()).unwrap();
                    proceed.recv().unwrap();
                },
            )
        });
        entered.recv().unwrap();
        let second = crate::update_apply::take_the_window_within(
            None,
            &staged.home,
            txn,
            Running { pid: 1, started: 1 },
            Duration::ZERO,
            crate::update_apply::Applier,
        );
        release.send(()).unwrap();
        let first = first.join().unwrap();

        assert!(first.is_mine(), "{first:?}");
        assert!(matches!(second, Window::Refused(_)), "{second:?}");
        assert_eq!(
            crate::update_apply::window_owner(&staged.home, txn),
            Some(me)
        );
    }

    /// RED (U-34, round 5; Codex's finding 14) — **a contender killed inside
    /// the election leaves nothing behind that stops the next one**: the
    /// operating system lets its lock go with its process, and the next
    /// contender takes the stale mark — `Mine`, never a third answer.
    ///
    /// The killed contender is a copy of this test binary
    /// (`BT_U34_ELECTION_CHILD` names the transaction's folder): it takes
    /// `owner.lock`, and — as the round-4 scheme's ballot winner would have —
    /// leaves `owner.takeover.<stale value>` naming itself, then says so and
    /// waits to be ended. The test ends it by its own handle.
    ///
    /// MUTATION: restore the round-4 ballot election (the next contender then
    /// finds a ballot naming a dead winner and answers `Unknown`).
    #[test]
    fn a_contender_killed_inside_the_election_releases_it() {
        const CHILD: &str = "BT_U34_ELECTION_CHILD";
        const NAME: &str =
            "update_handoff::tests::a_contender_killed_inside_the_election_releases_it";
        if let Some(folder) = std::env::var_os(CHILD) {
            let folder = PathBuf::from(folder);
            let _held = install_txn::try_hold(&folder.join("owner.lock"), Hold::Exclusive)
                .unwrap()
                .expect("the child is inside the election");
            let stale = std::fs::read_to_string(folder.join("owner")).unwrap();
            let key: String = stale
                .chars()
                .filter_map(|c| match c {
                    '0'..='9' => Some(c),
                    ':' => Some('-'),
                    _ => None,
                })
                .collect();
            let me = crate::update_apply::this_process();
            std::fs::write(
                folder.join(format!("owner.takeover.{key}")),
                format!("{}:{}", me.pid, me.started),
            )
            .unwrap();
            println!("u34 elector inside");
            std::thread::sleep(Duration::from_secs(60));
            return;
        }
        let folder = Folder::new("election-killed");
        let staged = staged(&folder);
        let txn = TxnId::new(TXN);
        let me = crate::update_apply::this_process();
        let transaction = staged.home.transaction(txn);
        std::fs::write(
            crate::update_apply::owner_path(&staged.home, txn),
            format!("{}:{}", me.pid, me.started.wrapping_add(1)),
        )
        .unwrap();
        let mut child =
            bt_platform::quiet_command(std::env::current_exe().expect("this test binary"))
                .args(["--exact", NAME, "--test-threads=1", "--nocapture"])
                .env(CHILD, &transaction)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("the child runs");
        let mut lines =
            std::io::BufRead::lines(std::io::BufReader::new(child.stdout.take().unwrap()));
        let inside = lines.any(|line| line.is_ok_and(|line| line.contains("u34 elector inside")));
        assert!(inside, "the child got inside the election");
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            crate::update_apply::take_the_window(
                None,
                &staged.home,
                txn,
                me,
                Instant::now() + crate::update_apply::ELECTION_WITHIN
            )
            .is_mine(),
            "the killed contender's lock went with it"
        );
        assert_eq!(
            crate::update_apply::window_owner(&staged.home, txn),
            Some(me)
        );
    }
}
