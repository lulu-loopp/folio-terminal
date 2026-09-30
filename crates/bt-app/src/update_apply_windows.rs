//! **`folio --update-apply <home> <txn> <nonce>` on Windows: the applier, its
//! rollback, and the recovery of every phase a dead applier leaves** (0.4.6
//! tickets U-23 and U-24; `docs/plans/design/self-update-2026-09-16.md`
//! §C.4–§C.7, revision (b) §(b).2's W3–W12, "Who may write what", E-7).
//!
//! O, on its way out, wrote `Handoff{applier: <nonce>}` durably and started
//! the rescue copy `H\<txn>\rescue\folio.exe` with this line (`update_handoff`,
//! U-21). This process — P, a copy of O's own executable that no step moves —
//! takes the transaction from there to `Committed`, or back to the old set, or
//! stops at a named phase for the next lock holder:
//!
//! 1. **Wait for O** (§C.4): the transaction lock `H\lock`, which O holds
//!    until its process ends (`install_txn::hold_within`, 60 s; not had →
//!    nothing written, [`Ended::OldHeldTheLock`], W3 stays for the next lock
//!    holder), then the data directory's claim tried until had and let go at
//!    once (`update_apply::wait_for_the_claim`); still held → `OldStayed`,
//!    `Abandoned`. An entrance already there from a dead attempt (W4) → the
//!    `Run` value removed, `Reverted`.
//! 2. **Revalidate** (U-20's decision 1, taken here):
//!    `update_prepare_windows::staged_as_verified` — the channel of the
//!    installed copy, the installed `folio.exe` still the recorded old image,
//!    the staged set at its recorded digests and its identity at the staged
//!    `folio.exe`'s own version, the rescue copy still the old image. Refused
//!    → `Unverified`, `Abandoned`. Nothing has been armed.
//! 3. **Arm** (W3 → W4 → W5): the `Run` value `FolioUpdate-<txn8>` =
//!    `"<rescue>" --update-recover` (`bt_platform::logon_hook::arm`: written,
//!    flushed, read back, and only then the `Armed` proof) → `Armed`, durable.
//!    The entrance could not be made durable → `EntranceFailed`, `Abandoned`.
//! 4. **Admit and check** (W5, E-7): exclusive admission on `H\admission`
//!    within what is left of the 60 s; then, before any move, every name the
//!    flip touches must be what the journal says — every old file still a
//!    regular file at its name (a folder or a link there, or nothing, refuses:
//!    U-20's decision 2), no name only the new set brings already taken by
//!    something the journal did not record (a move never replaces) — and
//!    **no old file held open
//!    by another process** (`bt_platform::install_flip::held_open`: opened for
//!    reading and writing with no sharing; a running image refuses it). A file
//!    still held is asked about again until the window runs out, because the
//!    old build's image outlives its process by a moment. Any refusal → the
//!    `Run` value removed, `Reverted` to `Prepared`, and **the old build is
//!    started again** with no argument (its journal is `Prepared` again, so it
//!    starts as usual).
//! 5. **Move** (W6, I1′): `backup\` made, then `Moving` durable (`Admitted`)
//!    **before the first move**; then one `MoveFileExW(…,
//!    MOVEFILE_WRITE_THROUGH)` per file (`install_txn::durable_move`, never over
//!    a file, each directory flushed after it): every old file present to
//!    `backup\<name>` first, then every new file from `set\<name>` into the
//!    install folder (`update_txn::Inventories::forward_moves`). A rename is
//!    atomic, so after every prefix of that list each old file is in exactly
//!    one of install and `backup\`, and each new file in exactly one of `set\`
//!    and install. A move that fails leaves exactly such a prefix: →
//!    `RollbackIntent`, and the rollback (step 8).
//! 6. **Trial** (W6 → W7): the exclusive admission let go (the trial takes its
//!    shared hold at its start); `<install>\folio.exe --update-trial <txn>
//!    <nonce>` with a fresh nonce, detached, through `quiet_command`; its pid
//!    from the child and its start time from `GetProcessTimes` →
//!    `Trial{nonce, process, began_ms}`, durable. Not started →
//!    `RollbackIntent`, and the rollback. **Started, and `Trial` not
//!    recorded** (U-34: the pid exists only once the process does, so the
//!    record comes after the start) → the trial ended at once, by its pid,
//!    creation time and image (`EndTrial` is the applier's over `Moving` for
//!    this one case), and the road goes on as for a trial that did not
//!    start: the journal never outlives its knowledge of a running trial.
//! 7. **Commit** (W7 → W8, `update_apply::watch_trial`): a receipt of this
//!    transaction and this trial's nonce while the journal says `Trial` →
//!    `Committed{outcome: committed}`, durable; any other receipt is said once
//!    and waited past. The trial gone without one, or 90 s → `RollbackIntent`,
//!    and the rollback. **After `Committed`** (W8, W12; the coordinator's
//!    order of U-23): the `Run` value removed and flushed; then exactly the
//!    recorded old files found in `backup\` by their digests deleted — never a
//!    file the journal does not record; then `Retired{Committed}` — the class
//!    `terminal`. A step that fails is debt ([`Ended::CommittedWithDebt`]),
//!    finished by the next lock holder, never a rollback.
//! 8. **Roll back** (W9–W11, U-24), each step `update_txn::decide`'s answer
//!    from what is on disk at that moment — the install folder and `backup\`
//!    read by digest every time, never the last phase inverted:
//!    - **the trial stopped** while the process list shows the journal's pid,
//!      with its creation time, running from `<install>\folio.exe`
//!      (`update_apply::stop_trial` → `bt_platform::install_flip::ask`): asked
//!      to quit (`WM_CLOSE` to the windows a person could close), 5 s of grace
//!      through the wait door, then ended (`TerminateProcess` on a handle
//!      whose creation time is read again), 5 s more; a process that is not
//!      all three is never touched;
//!    - **the moves back**, under exclusive admission within 60 s (not had →
//!      nothing recorded, [`Ended::RollbackWaits`]: the next holder tries):
//!      `rolledout\` made; every install file whose digest is a **new**
//!      member's → `rolledout\<name>`, then every old file in `backup\` →
//!      the install (`update_txn::rollback_moves`), each one
//!      `install_txn::durable_move`. New files go out first because a rename
//!      never replaces: after every prefix each old file is in exactly one of
//!      install and `backup\`, each new file in exactly one of `set\`, install
//!      and `rolledout\` (I1′);
//!    - **verified**: the next look finds every old file at its recorded
//!      digest in the install and no new file there → `RolledBack`
//!      (`outcome: rolled_back`), durable;
//!    - **then** the `Run` value removed and flushed, and `Retired{RolledBack}`
//!      — the class `terminal`; `H\<txn>`, with the rescue folder this process
//!      runs from, is the next ordinary start's to delete (U-12's `Retire`).
//!
//!    Any step that fails → `RollbackFailed`: `Stuck{last_error, attempts}`,
//!    durable, and nothing removed — the journal, `backup\`, `rolledout\` and
//!    the `Run` value stay (W10): the entrance runs this road again at every
//!    logon, and every start hands itself here, until
//!    `update_txn::STUCK_ATTEMPT_LIMIT` rollbacks have failed.
//!
//! **Leaving: one exit guard** (U-34; U-29b's rulings 2 and 3, adopted here by
//! U-24, now applied by it): every way out of the applier and of the
//! recovery — a normal end, any refusal once the home is known, a panic
//! unwinding — goes through `update_apply::ExitGuard` ([`WindowsLeave`]). The
//! applier has the duty only once it has taken the window's mark
//! (`update_apply::OWNER_FILE`), before it waits for O's lock; before that, and
//! when another live process holds the mark, it starts nothing. A successor
//! it leaves behind still running — the trial it started, the mark's holder
//! found at `Handoff` — opens Folio; otherwise one start of what the disk
//! names once the lock is let go ([`opens_now`]), counted only when a Folio
//! acknowledges it by holding the data directory, else the rescue copy, else
//! the failure window shown by this process: the old build
//! plainly after a revert or an abandon; after a rollback, finished or not,
//! and while the journal is `destructive` with the old set whole — a journal
//! write refused past [`crate::update_apply::JOURNAL_WRITE_WITHIN`], a lock
//! never had — the installed build with `--update-failed <journal>` (its
//! card: *Previous version restored.* or *Update incomplete.* and the folder),
//! the journal and the `Run` value staying for the next start or logon.
//! **The new set, live and not committed, is only ever started as a trial**: a road that ends `Stuck` with
//! every new file installed at its digest starts it as a trial over `Stuck` —
//! recorded (`RetrialBegan`), so its receipt commits forward (W8) — and waits
//! for it under the lock ([`Txn::retry_as_trial`]). Where the install holds
//! neither whole set, **the rescue copy** (O's own image, whose home holds no
//! journal) opens with `--update-failed` — the fallback when even the
//! recovery could not finish.
//!
//! **Recovery** ([`recover`], the rescue build as `--update-recover`, R): the
//! same lock within 60 s, then every phase a dead applier can leave, by the
//! same code, each step `decide`'s answer for `Asker::Rescue`: `Handoff` or
//! `Armed` — nothing moved — → the `Run` value removed, `Reverted` to
//! `Prepared`; `Moving` → `RollbackIntent` (W6) and the rollback; `Trial` →
//! waited for while its recorded process lives and its deadline has not passed,
//! then `Committed` and its retirement, or the rollback (W7); `RollbackIntent`,
//! `Stuck`, `RolledBack` → the rollback and its retirement (W9–W11);
//! `Committed` → its retirement (W12). **A `Handoff` whose window's mark names a
//! live process is left to it** — the one rule for both platforms
//! (`update_apply::the_window_is_theirs`, U-34): R writes nothing, waits for
//! nothing and opens nothing; that process opens Folio. Any other process of
//! the rescue image — an applier that never took the mark — is not waited
//! for. **A running trial the journal does not record** (0.4.7 ticket U-37)
//! — over `Moving`, or a `Stuck` whose recorded trial is gone, with the new
//! set installed — is recorded by any lock holder when its receipt is here
//! (`Txn::before_deciding`; revision (h): a receipt naming it by pid and start
//! instant), and that receipt commits: the guard's trial with a nonce
//! no journal records (`Opens::Trial`) is kept rather than rolled back under.
//!
//! Every phase is recorded through `update_apply::Journaled` (the protocol's
//! refusal, the writer table, then `install_txn::durable_write`, asked again
//! through the wait door while another program holds `journal.json` open —
//! `update_apply::write_journal`, U-34); every effect
//! is asked of `update_txn::may` first. Headless: the main thread is a worker
//! (`admission::enter_standalone_main`), and its only sleeps are the wait
//! door's (`bt_platform::wait::sleep_within`) and `install_txn::hold_within`'s.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use bt_platform::HostPlatform;
use bt_platform::admission::WorkerCtx;
use bt_platform::file_reads::{self, Lane};
use bt_platform::install_flip::{self, Running};
use bt_platform::install_txn::{self, Armed, Held, Hold};
use bt_platform::trust::Policy;

use crate::cli;
use crate::install_channel::Channel;
use crate::update_adapter::Layouts;
use crate::update_apply::{
    BeforeDeciding, Deferral, Ended, ExitGuard, HandedBack, Journaled, Leave, Limits, Opener,
    Opens, Watch, Watched, Window, failed_words, now_ms, owed_at_logon, read_receipt, stop_trial,
    trial_runs, trial_words,
};

/// **What H.3 answered before `decide`** (U-37).
enum Pre {
    /// A trial was adopted: take the next step from the disk again.
    Again,
    /// The road ends here: `Ended::Deferred`.
    Stop(Ended),
    /// `decide` as before.
    Decide,
}
use crate::update_prepare_windows::{Resume, staged_as_verified};
use crate::update_txn::{
    Action, Actor, Asker, Class, Digest, Disk, Effect, Event, Header, HeaderOutcome, Home,
    Inventories, Journal, Layout, Located, Move, Nonce, Phase, PhaseKind, Place, Restore, Seen,
    TrialProcess, TxnId, decide,
};

/// **The applier's and the recovery's effects that a test stands in for**:
/// the entrance (the `Run` value, through `bt_platform::logon_hook` over the
/// current user's registry in the product, and over a registry of the test's
/// own in a test), the trial's launch, a look after each move, the lines, and
/// the start of a Folio.
pub(crate) trait World {
    /// One line of what happened.
    fn say(&mut self, line: &str);
    /// Start `program` with `args`, detached: never waited on.
    ///
    /// # Errors
    /// It could not be started.
    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()>;
    /// Write the entrance for `txn` to start `rescue`, flush it and read it
    /// back: the `Armed` proof.
    ///
    /// # Errors
    /// The door's refusal, as a sentence.
    fn arm(&mut self, txn: TxnId, rescue: &Path) -> Result<Armed, String>;
    /// Remove the entrance of `txn` and flush; none there is success.
    ///
    /// # Errors
    /// The door's refusal, as a sentence.
    fn disarm(&mut self, txn: TxnId) -> Result<(), String>;
    /// Whether the entrance of `txn` is there.
    ///
    /// # Errors
    /// The registry could not be read.
    fn is_armed(&mut self, txn: TxnId) -> Result<bool, String>;
    /// Start the trial, `program args`, detached: its pid.
    ///
    /// # Errors
    /// It could not be started.
    fn launch_trial(&mut self, program: &Path, args: &[OsString]) -> io::Result<u32>;
    /// One move of the flip, or of its rollback, is done — the instant
    /// between two moves.
    fn moved(&mut self, _done: &Move) {}
    /// **Whether a start just made was acknowledged**: a Folio holds the data
    /// directory `data` within `update_apply::ACKNOWLEDGED_WITHIN`, asked
    /// through `worker`'s wait door (`update_apply::claimed_within`).
    fn acknowledged(&mut self, worker: Option<&WorkerCtx>, data: &Path) -> bool;
    /// **The failure window, in this process**: `text` in a message box.
    fn show_here(&mut self, text: &str);
}

// ── the layout's points ─────────────────────────────────────────────────────

/// **Where a layout acts on the Windows road**: the home, the installed
/// program, the transaction, and the member inventories the journal records.
pub(crate) struct Site<'a> {
    pub(crate) home: &'a Home,
    /// `<install>\folio.exe`.
    pub(crate) installed: &'a Path,
    pub(crate) txn: TxnId,
    pub(crate) inventories: &'a Inventories,
}

impl Site<'_> {
    /// `set\`, `backup\`, `rolledout\` or the install folder.
    ///
    /// # Errors
    /// The installed program has no folder, or the home is not a member
    /// set's.
    pub(crate) fn folder(&self, place: Place) -> Result<PathBuf, String> {
        match place {
            Place::Install => self
                .installed
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| "the installed program has no folder".to_owned()),
            other => self
                .home
                .members_folder(self.txn, other)
                .ok_or_else(|| "the home is not a member set's".to_owned()),
        }
    }
}

/// **How an `Activate` ended.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Activated {
    /// Every step is done.
    Done,
    /// A step failed, leaving what every prefix of the steps leaves (I1′);
    /// why, as a sentence. The road decides what follows from the disk.
    Cut(String),
}

/// **A layout's `Activate` and `Prove / Recover` on the Windows road** (0.4.7
/// ticket U-41a1; managed-update §1.1 R1): the road calls these, through the
/// adapter its journal names (`update_adapter::Layouts`), and nothing else
/// about it depends on the layout — the locks, the admission, the checks
/// before a move, the phases and their rights, the trial, the commit and the
/// rollback's decision (`update_txn::decide`) are common.
///
/// * [`ApplyPoints::activate`] — after `Moving` is durable, before
///   `TrialBegan`: the staged set made the live one;
/// * [`ApplyPoints::activate_back`] — after `RollbackIntent` is durable,
///   under the admission: the restore `decide` named;
/// * [`ApplyPoints::locate`] and [`ApplyPoints::live`] — which set is live,
///   read from the disk: what `decide` is handed at every step, and which
///   whole set the install holds (H.3, a retrial over `Stuck`, what opens).
///
/// Every effect is asked of the journal's rights first
/// (`Journaled::may`).
pub(crate) trait ApplyPoints: Send + Sync {
    /// **Activate forward**, as the applier.
    ///
    /// # Errors
    /// An effect the phase does not allow the applier.
    fn activate(
        &self,
        site: &Site<'_>,
        journal: &Journaled<'_>,
        world: &mut dyn World,
    ) -> Result<Activated, String>;

    /// **Activate back**, as `actor`: `moves`, `decide`'s restore.
    ///
    /// # Errors
    /// An effect the phase does not allow `actor`.
    fn activate_back(
        &self,
        site: &Site<'_>,
        journal: &Journaled<'_>,
        actor: Actor,
        moves: &[Move],
        world: &mut dyn World,
    ) -> Result<Activated, String>;

    /// **What is where**, for `decide`.
    ///
    /// # Errors
    /// A folder the site cannot name.
    fn locate(&self, site: &Site<'_>) -> Result<Located, String>;

    /// **Which whole set is installed**, or `None` for a mix or a site
    /// whose install cannot be named.
    fn live(&self, site: &Site<'_>) -> Option<Live>;
}

/// **Folio's own layout** (`Layout::Members`, U-23/U-24): one
/// `install_txn::durable_move` per file, forward and back, and the install
/// folder and `backup\` read by digest.
pub(crate) struct Ours;

impl ApplyPoints for Ours {
    /// Every old file present to `backup\`, then every new file from `set\`
    /// into the install (`update_txn::Inventories::forward_moves`).
    fn activate(
        &self,
        site: &Site<'_>,
        journal: &Journaled<'_>,
        world: &mut dyn World,
    ) -> Result<Activated, String> {
        for step in site.inventories.forward_moves() {
            let effect = match step.to {
                Place::Backup => Effect::MoveOldOut,
                _ => Effect::MoveNewIn,
            };
            journal.may(Actor::Applier, effect)?;
            let from = site.folder(step.from)?.join(&step.name);
            let to = site.folder(step.to)?.join(&step.name);
            if let Err(failure) = install_txn::durable_move(&from, &to) {
                return Ok(Activated::Cut(format!(
                    "the move of `{}` failed: {failure}",
                    step.name
                )));
            }
            world.moved(&step);
        }
        Ok(Activated::Done)
    }

    /// Each of `moves` — the new files out to `rolledout\`, then the old ones
    /// back — one `install_txn::durable_move`, with a look after each.
    fn activate_back(
        &self,
        site: &Site<'_>,
        journal: &Journaled<'_>,
        actor: Actor,
        moves: &[Move],
        world: &mut dyn World,
    ) -> Result<Activated, String> {
        for step in moves {
            let effect = match step.to {
                Place::RolledOut => Effect::MoveNewOut,
                _ => Effect::MoveOldBack,
            };
            journal.may(actor, effect)?;
            let from = site.folder(step.from)?.join(&step.name);
            let to = site.folder(step.to)?.join(&step.name);
            if let Err(failure) = install_txn::durable_move(&from, &to) {
                return Ok(Activated::Cut(format!(
                    "the move of `{}` to {:?}: {failure}",
                    step.name, step.to
                )));
            }
            world.moved(step);
        }
        Ok(Activated::Done)
    }

    /// What is at each member's name, by digest: in the install folder and
    /// in `backup\`.
    fn locate(&self, site: &Site<'_>) -> Result<Located, String> {
        let install = site.folder(Place::Install)?;
        let backup = site.folder(Place::Backup)?;
        let mut names: Vec<&str> = Vec::new();
        for member in site
            .inventories
            .old_present
            .iter()
            .chain(&site.inventories.new)
        {
            if !names.contains(&member.name.as_str()) {
                names.push(&member.name);
            }
        }
        Ok(Located::Members(
            names
                .into_iter()
                .map(|name| {
                    let mut unread = Vec::new();
                    let mut at =
                        |place: Place, folder: &Path| match read_digest_at(&folder.join(name)) {
                            Ok(digest) => digest,
                            Err(why) => {
                                unread.push((place, why));
                                None
                            }
                        };
                    let install = at(Place::Install, &install);
                    let backup = at(Place::Backup, &backup);
                    Seen {
                        name: name.to_owned(),
                        install,
                        backup,
                        unread,
                    }
                })
                .collect(),
        ))
    }

    fn live(&self, site: &Site<'_>) -> Option<Live> {
        let install = site.folder(Place::Install).ok()?;
        live_set(site.inventories, &install)
    }
}

/// **The layouts of this road** as the product has them: [`Ours`] alone.
#[must_use]
pub(crate) fn own_layouts() -> Layouts<dyn ApplyPoints> {
    Layouts::of(Arc::new(Ours))
}

/// **One Windows road**: the home, the installed program it replaces, the
/// rescue copy this process runs from, and what the checks and waits take.
pub(crate) struct Road {
    pub(crate) home: Home,
    /// `<install>\folio.exe`.
    pub(crate) installed: PathBuf,
    /// This process's own image, `H\<txn>\rescue\folio.exe`: the rescue copy,
    /// which is also what opens when neither whole set is installed.
    pub(crate) rescue: PathBuf,
    /// The data directory whose claim O held (the storage rule O ran under:
    /// the applier inherits O's environment).
    pub(crate) data: PathBuf,
    /// Which roots a signature may end in: the system's in the product.
    pub(crate) policy: Policy,
    /// How the installed copy was installed, derived now.
    pub(crate) channel: Option<Channel>,
    pub(crate) limits: Limits,
    /// This process, by its pid and start time: what the window's mark names
    /// when this process takes it (`update_apply::OWNER_FILE`).
    pub(crate) me: Running,
    /// **This process's starter** (U-37, H.3): its parent, by pid and an
    /// earlier start instant (`install_flip::parent_of_this_process`, read at
    /// entry) — an ordinary start that handed itself over is no candidate.
    pub(crate) starter: Option<Running>,
    /// **The trial that handed its transaction back** (`--from-trial`, H.4).
    pub(crate) handed_back: Option<HandedBack>,
    /// **The layouts this road calls**, by the adapter the journal names
    /// (managed-update R2): [`own_layouts`] in the product.
    pub(crate) layouts: Layouts<dyn ApplyPoints>,
    /// **A rescue build older than 0.4.7, for the direct-hop test** (design
    /// revision (h), the rollout contract): no adoption, no deferral — the road
    /// every update from 0.4.6 takes, whose rescue build is 0.4.6.
    #[cfg(test)]
    pub(crate) as_046: bool,
}

impl Road {
    /// The road of the rescue build at `rescue` over `home`, as the product
    /// runs it.
    pub(crate) fn of_this_copy(home: Home, installed: PathBuf, rescue: PathBuf) -> Self {
        let channel = Some(crate::install_channel::channel_of(&installed));
        Self {
            home,
            installed,
            rescue,
            data: crate::persist::storage_dir_unmoved(),
            policy: Policy::System,
            channel,
            limits: Limits::PRODUCT,
            me: crate::update_apply::this_process(),
            starter: install_flip::parent_of_this_process(),
            handed_back: None,
            layouts: own_layouts(),
            #[cfg(test)]
            as_046: false,
        }
    }

    /// **The program [`Opens`] starts, and its words** before a handed
    /// command line.
    pub(crate) fn opening(&self, opens: &Opens) -> (&Path, Vec<OsString>) {
        let program = match opens {
            Opens::Rescue => &self.rescue,
            _ => &self.installed,
        };
        (program, opens.words(&self.home))
    }
}

/// **What recovery did, and whom it leaves behind.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Recovered {
    pub(crate) ended: Ended,
    /// A successor that opens Folio while it runs: the applier found at
    /// `Handoff`, or the trial this run started with the handed command line.
    pub(crate) successor: Option<Running>,
    /// Whether anybody waits for a window: a person's start always; the run
    /// at logon only after a revert or a rollback it finished.
    pub(crate) waiting: bool,
}

/// **How a Windows road process leaves** (`update_apply::ExitGuard`, U-34):
/// what the disk names from `road` ([`opens_now`]), with `handed` after its
/// words, started through the world.
pub(crate) struct WindowsLeave<'a, W: World> {
    pub(crate) road: &'a Road,
    pub(crate) world: &'a mut W,
    pub(crate) handed: &'a [OsString],
    /// The worker whose wait door the acknowledgement's wait sleeps through;
    /// `None` where none was lent (then it is asked once).
    pub(crate) worker: Option<&'a WorkerCtx>,
}

impl<W: World> Leave for WindowsLeave<'_, W> {
    fn say(&mut self, line: &str) {
        self.world.say(line);
    }

    fn opening(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
        let (program, mut words) = self.road.opening(&opens_now(self.road));
        words.extend_from_slice(self.handed);
        Some((program.to_path_buf(), words))
    }

    fn start(&mut self, program: &Path, words: &[OsString]) -> io::Result<()> {
        self.world.spawn_detached(program, words)
    }

    /// The rescue copy — O's own image, which has run — with
    /// `--update-failed`: whose own home holds no journal.
    fn fallback(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
        let (program, mut words) = self.road.opening(&Opens::Rescue);
        words.extend_from_slice(self.handed);
        Some((program.to_path_buf(), words))
    }

    fn acknowledged(&mut self) -> bool {
        self.world.acknowledged(self.worker, &self.road.data)
    }

    fn show_here(&mut self, why: &str) {
        self.world.say(&format!(
            "BT_UPDATE_EXIT no start was delivered ({why}); the failure window is shown here"
        ));
        self.world
            .show_here(&crate::update_apply::failure_text(Some(&self.road.home)));
    }
}

/// **The door, for this process**: this executable must be the rescue copy
/// of the home the line names.
pub(crate) fn run_here(home: &Path, txn: &str, nonce: &str) -> i32 {
    let mut world = Machine { log: None };
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            World::say(
                &mut world,
                &format!("BT_UPDATE_APPLY cannot name its own executable: {error}"),
            );
            return 2;
        }
    };
    let Some((home, installed)) = Home::of_rescue_named(HostPlatform::Windows, &exe, home) else {
        World::say(
            &mut world,
            &format!(
                "BT_UPDATE_APPLY {} is not the installation home of {}; {} runs only from a rescue copy",
                home.display(),
                exe.display(),
                cli::UPDATE_APPLY_FLAG
            ),
        );
        return 2;
    };
    let road = Road::of_this_copy(home, installed, exe);
    world.log = Some(crate::update_recover::log_file(&road.home, &road.data).0);
    // From here the home is known: every way out, a refusal included, leaves
    // through the exit guard (U-34).
    let refused = match (TxnId::parse(txn), Nonce::parse(nonce)) {
        (Ok(txn), Ok(nonce)) => {
            match bt_platform::admission::enter_standalone_main("folio-update-apply", |worker| {
                apply(worker, &road, txn, nonce, &mut world)
            }) {
                Ok(ended) => {
                    World::say(
                        &mut world,
                        &format!("BT_UPDATE_APPLY transaction {txn}: {ended:?}"),
                    );
                    return ended.code();
                }
                Err(refused) => format!("{refused:?}"),
            }
        }
        _ => format!(
            "malformed transaction or nonce; {}",
            cli::UPDATE_APPLY_USAGE
        ),
    };
    World::say(&mut world, &format!("BT_UPDATE_APPLY {refused}"));
    // Refused before it took the window's mark: the duty stays with O, which
    // armed it at the press and finds no mark (U-34, round 2).
    let mut guard = ExitGuard::new(WindowsLeave {
        road: &road,
        world: &mut world,
        handed: &[],
        worker: None,
    });
    guard.not_mine(None);
    let left = guard.leave();
    drop(guard);
    World::say(&mut world, &format!("BT_UPDATE_APPLY {}", left.said()));
    2
}

/// **The applier, over any road** — see the module header. It takes the
/// window's mark first (`update_apply::take_the_window`) and runs its road only
/// if it got it; once the lock is let go, it leaves through its exit guard
/// (`update_apply::ExitGuard`): the trial it started, still running, is the
/// window; otherwise the start the disk names ([`opens_now`]), acknowledged,
/// else the rescue copy, else the failure window shown here.
pub(crate) fn apply(
    worker: &WorkerCtx,
    road: &Road,
    txn: TxnId,
    nonce: Nonce,
    world: &mut impl World,
) -> Ended {
    // Every way out of the road, a panic included, leaves through the guard
    // (U-34); the lock is let go before a build starts — its own start
    // retires a finished transaction, which needs it.
    let mut guard = ExitGuard::new(WindowsLeave {
        road,
        world,
        handed: &[],
        worker: Some(worker),
    });
    // **The window's duty first** (U-34, `update_apply::OWNER_FILE`): taken
    // before the wait for O's lock, while O still runs. An applier that does
    // not get it leaves the transaction untouched: the process that has the
    // duty opens Folio, and the next start or logon finishes the update.
    // A live owner that is not this applier is O at its end, which leaves
    // anyway: wait for it to go (round 6), within the applier's own wait for
    // O, before giving up the transaction. **One deadline for the whole wait
    // for O** (round 7): the mark, the transaction lock, the claim and the
    // admission all spend the same `old_within` — the election's own wait for
    // its lock too (round 8).
    let until = Instant::now() + road.limits.old_within;
    let window = loop {
        match crate::update_apply::take_the_window(&road.home, txn, road.me, until) {
            Window::Theirs(_) if Instant::now() < until => {
                bt_platform::wait::sleep_within(
                    worker,
                    road.limits
                        .poll
                        .min(until.saturating_duration_since(Instant::now())),
                );
            }
            decided => break decided,
        }
    };
    match window {
        Window::Mine => {}
        other => {
            let owner = match &other {
                Window::Theirs(owner) => Some(owner.pid),
                _ => None,
            };
            guard.not_mine(owner);
            let left = guard.leave();
            guard
                .inner()
                .world
                .say(&format!("BT_UPDATE_APPLY {other:?}; {}", left.said()));
            return Ended::Refused(format!("the window is not this applier's: {other:?}"));
        }
    }
    let (ended, successor) =
        apply_under_the_lock(worker, road, txn, nonce, until, &mut *guard.inner().world);
    guard.succeeded_by(successor);
    if ended.deferred_to_a_holder() {
        guard.window_elsewhere();
    }
    let left = guard.leave();
    guard
        .inner()
        .world
        .say(&format!("BT_UPDATE_APPLY {}", left.said()));
    ended
}

/// **The applier's road under the transaction lock**: where it ended, and
/// the trial it started, which opens Folio while it runs.
fn apply_under_the_lock(
    worker: &WorkerCtx,
    road: &Road,
    txn: TxnId,
    nonce: Nonce,
    window: Instant,
    world: &mut impl World,
) -> (Ended, Option<Running>) {
    let lock = match install_txn::hold_within(
        &road.home.lock(),
        Hold::Exclusive,
        window.saturating_duration_since(Instant::now()),
    ) {
        Ok(Some(held)) => held,
        Ok(None) => return (Ended::OldHeldTheLock, None),
        Err(failure) => return (Ended::Failed(failure.to_string()), None),
    };
    let journal = match read_journal(&road.home) {
        Ok(Some(journal)) => journal,
        Ok(None) => return (Ended::Refused("there is no journal".to_owned()), None),
        Err(why) => return (Ended::Refused(why), None),
    };
    if journal.txn != txn {
        return (
            Ended::Refused(format!(
                "the journal is transaction {}, not {txn}",
                journal.txn
            )),
            None,
        );
    }
    match Txn::of(road, worker, journal, lock, Asker::LockHolder) {
        Ok(mut held) => {
            let mut ended = held.apply(worker, nonce, window, world);
            if ended.rolled_back() {
                ended = held.retry_as_trial(worker, world, ended, &[]);
            }
            World::say(
                world,
                &format!(
                    "BT_UPDATE_APPLY transaction {txn} wrote {:?}",
                    held.j.written
                ),
            );
            (ended, held.successor)
        }
        Err(ended) => (ended, None),
    }
}

/// **Recovery over any road** (R) — see the module header: the transaction
/// lock within [`Limits::old_within`], then each step `decide`'s answer for
/// `Asker::Rescue`. Handed a person's start (`start`), a road that ends
/// `Stuck` with the new set installed starts it as a trial over `Stuck`, with
/// `start` after its words. What opens after is [`Recovered::opens`].
pub(crate) fn recover(
    worker: &WorkerCtx,
    road: &Road,
    world: &mut impl World,
    start: Option<&[OsString]>,
) -> Recovered {
    let opener = if start.is_some() {
        Opener::Start
    } else {
        Opener::Login
    };
    let handed = start.unwrap_or(&[]);
    let (ended, successor) = match hold(road) {
        Ok((lock, journal)) => {
            let txn = journal.txn;
            match Txn::of(road, worker, journal, lock, Asker::Rescue) {
                Ok(mut held) => {
                    let mut ended = if held.j.phase() == PhaseKind::Handoff
                        && let Some(applier) =
                            crate::update_apply::the_window_is_theirs(&road.home, txn, road.me)
                    {
                        world.say(&format!(
                            "BT_UPDATE_RECOVER {} has the update's window; the handed-off update is left to it",
                            applier.pid
                        ));
                        held.successor = Some(applier);
                        Ended::LockHeld
                    } else {
                        held.settle(worker, None, world)
                            .unwrap_or_else(Ended::Failed)
                    };
                    if ended.rolled_back() && opener == Opener::Start {
                        ended = held.retry_as_trial(worker, world, ended, handed);
                    }
                    world.say(&format!(
                        "BT_UPDATE_RECOVER transaction {txn} wrote {:?}",
                        held.j.written
                    ));
                    (ended, held.successor)
                }
                Err(ended) => (ended, None),
            }
        }
        Err(ended) => (ended, None),
    };
    let waiting = opener == Opener::Start || owed_at_logon(&ended);
    Recovered {
        ended,
        successor,
        waiting,
    }
}

/// The transaction lock within [`Limits::old_within`], then the journal.
fn hold(road: &Road) -> Result<(Held, Journal), Ended> {
    let lock = match install_txn::hold_within(
        &road.home.lock(),
        Hold::Exclusive,
        road.limits.old_within,
    ) {
        Ok(Some(held)) => held,
        Ok(None) => return Err(Ended::LockHeld),
        Err(failure) => return Err(Ended::Failed(failure.to_string())),
    };
    match read_journal(&road.home) {
        Ok(Some(journal)) => Ok((lock, journal)),
        Ok(None) => Err(Ended::Left("there is no transaction".to_owned())),
        Err(why) => Err(Ended::Left(why)),
    }
}

/// **What the disk names to start now** (U-29b's rulings 2 and 3, adopted by
/// U-24; since U-34 read only by the exit guard, [`WindowsLeave`]): while the
/// header is not `destructive`, the installed build — with `--update-failed`
/// after a retired rollback; while it is, by what the install folder holds,
/// read by digest: every new file at its digest and no commit → the new build
/// **only as a trial**; the whole new set committed, or the whole old set →
/// the installed build with `--update-failed`; neither whole set, or a journal
/// whose layout cannot be read → the rescue copy with `--update-failed`.
pub(crate) fn opens_now(road: &Road) -> Opens {
    let bytes = file_reads::read(Lane::UpdateJournal, road.home.journal()).ok();
    let Some(header) = bytes.as_deref().and_then(|bytes| Header::parse(bytes).ok()) else {
        return Opens::Installed { failed: false };
    };
    if header.class != Class::Destructive {
        return Opens::Installed {
            failed: header.outcome == HeaderOutcome::RolledBack,
        };
    }
    let Some(journal) = bytes
        .as_deref()
        .and_then(|bytes| Journal::parse(bytes).ok())
    else {
        return Opens::Rescue;
    };
    let Layout::Members(inventories) = &journal.body.layout else {
        return Opens::Rescue;
    };
    let Ok(layout) = road.layouts.named(journal.body.adapter) else {
        return Opens::Rescue;
    };
    let site = Site {
        home: &road.home,
        installed: &road.installed,
        txn: journal.txn,
        inventories,
    };
    match layout.live(&site) {
        Some(Live::New) if header.outcome != HeaderOutcome::Committed => {
            Opens::Trial { txn: header.txn }
        }
        Some(_) => Opens::Installed { failed: true },
        None => Opens::Rescue,
    }
}

/// Which whole set the install folder holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Live {
    /// Every new member at its digest.
    New,
    /// Every old file at its digest, and no member only the new set brings.
    Old,
}

/// **Which whole set is installed, by digest**, or `None` for a mix.
fn live_set(inventories: &Inventories, install: &Path) -> Option<Live> {
    let at = |name: &str| digest_at(&install.join(name));
    if inventories
        .new
        .iter()
        .all(|member| at(&member.name) == Some(member.digest))
    {
        return Some(Live::New);
    }
    let old_whole = inventories
        .old_present
        .iter()
        .all(|member| at(&member.name) == Some(member.digest));
    let no_new_only = inventories
        .new
        .iter()
        .filter(|member| {
            !inventories
                .old_present
                .iter()
                .any(|old| old.name == member.name)
        })
        .all(|member| at(&member.name) != Some(member.digest));
    (old_whole && no_new_only).then_some(Live::Old)
}

/// The journal at `H\journal.json`: `None` when there is none.
fn read_journal(home: &Home) -> Result<Option<Journal>, String> {
    match file_reads::read(Lane::UpdateJournal, home.journal()) {
        Ok(bytes) => Journal::parse(&bytes)
            .map(Some)
            .map_err(|refusal| format!("the journal: {refusal}")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("the journal: {error}")),
    }
}

/// **One transaction under its lock**, with its inventories.
struct Txn<'a> {
    road: &'a Road,
    /// Who this holder is to `decide`: the applier, or the rescue build as
    /// recovery.
    asker: Asker,
    j: Journaled<'a>,
    /// Held for the whole road; let go when this is dropped.
    _lock: Held,
    inventories: Inventories,
    /// The layout the journal names, whose points this road calls.
    layout: Arc<dyn ApplyPoints>,
    /// **The successor this holder leaves behind**: the trial it started — the
    /// applier's own, or a retrial over `Stuck` — or the applier found at
    /// `Handoff`. While it runs it is the window, and the exit guard starts
    /// nothing (U-34; U-29b's ruling 2's "exactly one"); a trial stopped by a
    /// rollback no longer runs.
    successor: Option<Running>,
}

impl<'a> Txn<'a> {
    fn of(
        road: &'a Road,
        worker: &'a WorkerCtx,
        journal: Journal,
        lock: Held,
        asker: Asker,
    ) -> Result<Self, Ended> {
        let Layout::Members(inventories) = journal.body.layout.clone() else {
            return Err(Ended::Refused(
                "the journal is not a member set's".to_owned(),
            ));
        };
        let layout = road
            .layouts
            .named(journal.body.adapter)
            .map_err(|not_built| Ended::Refused(not_built.to_string()))?;
        Ok(Self {
            road,
            asker,
            j: Journaled::of(&road.home, worker, journal),
            _lock: lock,
            inventories,
            layout,
            successor: None,
        })
    }

    fn txn(&self) -> TxnId {
        self.j.journal.txn
    }

    /// Where the layout acts: this road's home and program, this
    /// transaction and its inventories.
    fn site(&self) -> Site<'_> {
        Site {
            home: &self.road.home,
            installed: &self.road.installed,
            txn: self.txn(),
            inventories: &self.inventories,
        }
    }

    /// `set\`, `backup\`, `rolledout\` or the install folder.
    fn folder(&self, place: Place) -> Result<PathBuf, String> {
        self.site().folder(place)
    }

    /// P's road, by the phase it finds.
    fn apply(
        &mut self,
        worker: &WorkerCtx,
        nonce: Nonce,
        window: Instant,
        world: &mut impl World,
    ) -> Ended {
        let outcome = match self.j.journal.body.phase.clone() {
            Phase::Handoff { applier } if applier == nonce => {
                self.at_handoff(worker, window, world)
            }
            Phase::Handoff { .. } => {
                return Ended::Refused("the journal was handed to another applier".to_owned());
            }
            Phase::Armed => self.at_armed(worker, window, world),
            // Found again past its own steps: decided from the disk, as
            // recovery decides it.
            Phase::Moving | Phase::Trial { .. } | Phase::Committed => {
                self.settle(worker, None, world)
            }
            other => {
                return Ended::Refused(format!(
                    "the journal says {:?}, which is no applier's",
                    other.kind()
                ));
            }
        };
        outcome.unwrap_or_else(Ended::Failed)
    }

    /// W3: wait for O, revalidate, arm.
    fn at_handoff(
        &mut self,
        worker: &WorkerCtx,
        window: Instant,
        world: &mut impl World,
    ) -> Result<Ended, String> {
        if world.is_armed(self.txn())? {
            // W4: an entrance from a dead attempt; nothing was moved.
            world.say("BT_UPDATE_APPLY an entrance from an earlier attempt is there");
            return self.revert(Actor::Applier, world);
        }
        if let Err(why) = crate::update_apply::wait_for_the_claim(
            worker,
            &self.road.data,
            self.road.limits.poll,
            window,
        ) {
            world.say(&format!(
                "BT_UPDATE_APPLY the old build did not let go of {}: {why}",
                self.road.data.display()
            ));
            self.j.record(Actor::Applier, &Event::OldStayed)?;
            return Ok(Ended::Abandoned);
        }
        let resume = Resume {
            exe: &self.road.installed,
            channel: self.road.channel,
            policy: &self.road.policy,
        };
        if let Err(stop) = staged_as_verified(&self.road.home, &self.j.journal, &resume) {
            world.say(&format!(
                "BT_UPDATE_APPLY the staged update is not what was verified: {stop:?}"
            ));
            self.j.record(Actor::Applier, &Event::Unverified)?;
            return Ok(Ended::Abandoned);
        }
        self.j.may(Actor::Applier, Effect::WriteEntrance)?;
        let rescue = self.road.home.rescue_program(&self.j.journal.rescue);
        match world.arm(self.txn(), &rescue) {
            Ok(armed) => self.j.record(Actor::Applier, &Event::Armed(armed))?,
            Err(why) => {
                world.say(&format!("BT_UPDATE_APPLY the entrance: {why}"));
                self.j.record(Actor::Applier, &Event::EntranceFailed)?;
                return Ok(Ended::Abandoned);
            }
        }
        self.at_armed(worker, window, world)
    }

    /// W5 → W6 → W7: admit, check, move, start the trial.
    fn at_armed(
        &mut self,
        worker: &WorkerCtx,
        window: Instant,
        world: &mut impl World,
    ) -> Result<Ended, String> {
        let left = window.saturating_duration_since(Instant::now());
        let admission =
            match install_txn::hold_within(&self.road.home.admission(), Hold::Exclusive, left) {
                Ok(Some(held)) => held,
                Ok(None) => {
                    world.say("BT_UPDATE_APPLY a running copy holds the admission");
                    return self.revert(Actor::Applier, world);
                }
                Err(failure) => {
                    world.say(&format!("BT_UPDATE_APPLY the admission: {failure}"));
                    return self.revert(Actor::Applier, world);
                }
            };
        if let Err(why) = self.ready_to_move(worker, window) {
            world.say(&format!("BT_UPDATE_APPLY nothing is moved: {why}"));
            return self.revert(Actor::Applier, world);
        }
        let backup = self.folder(Place::Backup)?;
        match install_txn::durable_create_dir(&backup) {
            Ok(()) => {}
            Err(failure) if failure.error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(failure) => {
                world.say(&format!("BT_UPDATE_APPLY nothing is moved: {failure}"));
                return self.revert(Actor::Applier, world);
            }
        }
        // `Moving` is durable before the first move.
        self.j.record(Actor::Applier, &Event::Admitted)?;
        // Activate, forward: the layout's (managed-update R1).
        let layout = Arc::clone(&self.layout);
        let activated = layout.activate(&self.site(), &self.j, world)?;
        if let Activated::Cut(why) = activated {
            world.say(&format!("BT_UPDATE_APPLY {why}"));
            drop(admission);
            return self.declare_rollback(worker, Actor::Applier, world);
        }
        // The trial takes its shared hold at its start.
        drop(admission);
        self.trial(worker, world)
    }

    /// **Before any move** (W5, E-7, U-20's decision 2): every old file the
    /// journal records is a regular file at its name; no name the new set
    /// moves into is taken by anything the journal does not record; and no old
    /// file is held open by another process — asked about again every poll
    /// until `window`.
    fn ready_to_move(&self, worker: &WorkerCtx, window: Instant) -> Result<(), String> {
        let install = self.folder(Place::Install)?;
        for member in &self.inventories.old_present {
            match std::fs::symlink_metadata(install.join(&member.name)) {
                Ok(meta) if meta.is_file() => {}
                Ok(_) => {
                    return Err(format!(
                        "`{}` in the install is a folder or a link",
                        member.name
                    ));
                }
                Err(error) => return Err(format!("`{}` in the install: {error}", member.name)),
            }
        }
        for member in &self.inventories.new {
            let recorded = self
                .inventories
                .old_present
                .iter()
                .any(|old| old.name == member.name);
            if !recorded && std::fs::symlink_metadata(install.join(&member.name)).is_ok() {
                return Err(format!(
                    "`{}` in the install is not what the journal recorded",
                    member.name
                ));
            }
        }
        until_let_go(
            window,
            self.road.limits.poll,
            &mut Instant::now,
            &mut || {
                let mut held = Vec::new();
                for member in &self.inventories.old_present {
                    match install_flip::held_open(&install.join(&member.name)) {
                        Ok(false) => {}
                        Ok(true) => held.push(member.name.clone()),
                        Err(error) => {
                            return Err(format!("`{}` in the install: {error}", member.name));
                        }
                    }
                }
                Ok(held)
            },
            &mut |pause| bt_platform::wait::sleep_within(worker, pause),
        )
    }

    /// W6 → W7 → W8, or the rollback.
    fn trial(&mut self, worker: &WorkerCtx, world: &mut impl World) -> Result<Ended, String> {
        let nonce = crate::update_job::mint_nonce();
        let began_ms = now_ms();
        let args = trial_words(self.txn(), &nonce);
        let pid = match world.launch_trial(&self.road.installed, &args) {
            Ok(pid) => pid,
            Err(error) => {
                world.say(&format!(
                    "BT_UPDATE_APPLY {} did not start: {error}",
                    self.road.installed.display()
                ));
                return self.declare_rollback(worker, Actor::Applier, world);
            }
        };
        // A trial that has already gone has no start time: it is then taken
        // as not running, and only its receipt can commit.
        let started = install_flip::started_of(pid).unwrap_or(0);
        let process = TrialProcess { pid, started };
        self.successor = Some(Running { pid, started });
        if let Err(why) = self.j.record(
            Actor::Applier,
            &Event::TrialBegan {
                nonce,
                process,
                began_ms,
            },
        ) {
            // The journal does not know this trial (U-34): it is ended, and
            // the road goes on as for a trial that did not start.
            world.say(&format!(
                "BT_UPDATE_APPLY the trial {pid} could not be recorded: {why}"
            ));
            self.end_unrecorded(worker, process, why, world)?;
            return self.declare_rollback(worker, Actor::Applier, world);
        }
        if self.watch(worker, Actor::Applier, world)? {
            Ok(self.retire_committed(Actor::Applier, world))
        } else {
            self.declare_rollback(worker, Actor::Applier, world)
        }
    }

    /// **End the trial this holder launched over `Moving` whose start the
    /// journal could not record** (U-34): asked to quit, then ended, only by
    /// its pid, creation time and image, as W9 stops a recorded one. A trial
    /// that will not end runs on as the window (the holder's successor), and
    /// nothing else is started.
    ///
    /// # Errors
    /// `why`, and why the trial could not be ended.
    fn end_unrecorded(
        &mut self,
        worker: &WorkerCtx,
        process: TrialProcess,
        why: String,
        world: &mut impl World,
    ) -> Result<(), String> {
        self.j.may(Actor::Applier, Effect::EndTrial)?;
        let limits = &self.road.limits;
        let installed = self.road.installed.clone();
        let stopped = stop_trial(
            worker,
            process,
            &[installed.as_path()],
            (limits.quit_within, limits.end_within, limits.poll),
            &mut |line| world.say(line),
        );
        stopped.map_err(|stop| format!("{why}; {stop}"))
    }

    /// **The trial the journal records, waited for** (W7): its receipt, while
    /// the recorded process runs from `<install>\folio.exe` — its pid, its
    /// creation time and its image — and its deadline has not passed. `true`
    /// once `Committed` is durable.
    fn watch(
        &mut self,
        worker: &WorkerCtx,
        actor: Actor,
        world: &mut impl World,
    ) -> Result<bool, String> {
        let road = self.road;
        let watch = Watch {
            home: &road.home,
            actor,
            started: None,
            poll: road.limits.poll,
            trial_within_ms: road.limits.trial_within_ms,
        };
        let images = [road.installed.as_path()];
        let watched = crate::update_apply::watch_trial(
            worker,
            &mut self.j,
            &watch,
            // The Windows trial's pid is the child's own, recorded at its
            // launch: there is never one to find, so never one unrecorded,
            // and never a launch to watch instead (U-38).
            &mut |_, _| None,
            &mut || false,
            &mut |process| trial_runs(process, &images),
            &mut |line| world.say(line),
        )?;
        Ok(watched == Watched::Committed)
    }

    /// `RollbackDeclared` as `actor`, durable, then the rollback.
    fn declare_rollback(
        &mut self,
        worker: &WorkerCtx,
        actor: Actor,
        world: &mut impl World,
    ) -> Result<Ended, String> {
        self.j.record(actor, &Event::RollbackDeclared)?;
        self.settle(worker, Some(actor), world)
    }

    /// **Every phase from what is on disk, one step at a time** (W6–W12 — see
    /// the module header, step 8). Each step is the one `update_txn::decide`
    /// names for this holder now, from the install folder and `backup\` read
    /// by digest, the entrance, the receipt and the recorded trial, so a road
    /// cut short anywhere is finished by the next holder from wherever it
    /// stood. `tenure` is the actor every record is made as; `None` takes the
    /// one the phase gives this holder's asker.
    fn settle(
        &mut self,
        worker: &WorkerCtx,
        tenure: Option<Actor>,
        world: &mut impl World,
    ) -> Result<Ended, String> {
        // Waits aside, at most: declare, stop the trial, move back, declare,
        // retire.
        for _ in 0..8 {
            let actor = tenure.unwrap_or_else(|| self.asker.actor(self.j.phase()));
            let (trial, nonce) = match &self.j.journal.body.phase {
                Phase::Trial { process, nonce, .. } => (Some(*process), Some(*nonce)),
                Phase::RollbackIntent { trial } => (*trial, None),
                Phase::Stuck { trial, retrial, .. } => {
                    (*trial, retrial.map(|retrial| retrial.nonce))
                }
                _ => (None, None),
            };
            let installed = self.road.installed.clone();
            let images = [installed.as_path()];
            let trial_alive = trial.is_some_and(|process| trial_runs(process, &images));
            let entrance = world.is_armed(self.txn()).unwrap_or(true);
            let receipt = nonce
                .and_then(|nonce| read_receipt(&self.road.home.receipt_path(self.txn(), &nonce)))
                .and_then(Result::ok);
            match self.before_deciding(worker, actor, trial.filter(|_| trial_alive), world)? {
                Pre::Again => continue,
                Pre::Stop(ended) => return Ok(ended),
                Pre::Decide => {}
            }
            let located = self.layout.locate(&self.site())?;
            let action = decide(&Disk {
                journal: &self.j.journal,
                asker: self.asker,
                entrance,
                located,
                receipt: receipt.clone(),
                trial_alive,
                now_ms: now_ms(),
            });
            match action {
                Action::Revert { .. } => return self.revert(actor, world),
                Action::AwaitReceipt { .. } => {
                    if self.watch(worker, actor, world)? {
                        return Ok(self.retire_committed(actor, world));
                    }
                    if self.j.phase() == PhaseKind::Stuck {
                        // The trial started over `Stuck` gave none: it is left
                        // to the next holder, which stops it first.
                        return Ok(self.still_stuck());
                    }
                    self.j.record(actor, &Event::RollbackDeclared)?;
                }
                Action::Commit => {
                    let Some(receipt) = receipt else {
                        return Err("a commit without its receipt".to_owned());
                    };
                    self.j.record(actor, &Event::ReceiptAccepted(receipt))?;
                    return Ok(self.retire_committed(actor, world));
                }
                Action::DeclareRollback => {
                    if let Some(process) = trial {
                        world.say(&format!(
                            "BT_UPDATE_RECOVER the trial {} gave no receipt",
                            process.pid
                        ));
                    } else if self.j.phase() == PhaseKind::Moving {
                        // No trial was begun: the moves were cut short (W6; U-42a).
                        world.say(
                            "BT_UPDATE_RECOVER the update was interrupted before the new version started",
                        );
                    }
                    self.j.record(actor, &Event::RollbackDeclared)?;
                }
                Action::FinishCommit { .. } => return Ok(self.retire_committed(actor, world)),
                Action::StopTrial(process) => {
                    self.j.may(actor, Effect::EndTrial)?;
                    let limits = &self.road.limits;
                    if let Err(why) = stop_trial(
                        worker,
                        process,
                        &images,
                        (limits.quit_within, limits.end_within, limits.poll),
                        &mut |line| world.say(line),
                    ) {
                        return self.stuck(actor, why, world);
                    }
                }
                Action::RollBack(Restore::Moves(moves)) => {
                    if let Some(ended) = self.move_back(actor, &moves, world)? {
                        return Ok(ended);
                    }
                }
                Action::DeclareRolledBack => self.j.record(actor, &Event::RolledBack)?,
                Action::StayStuck { reason } => return self.stuck(actor, reason, world),
                Action::GiveUp { last_error } => {
                    world.say(&format!(
                        "BT_UPDATE_ROLLBACK the update is incomplete after {} attempts ({last_error}); see {}",
                        crate::update_txn::STUCK_ATTEMPT_LIMIT,
                        self.road.home.root().display()
                    ));
                    return Ok(Ended::GaveUp(last_error));
                }
                Action::FinishRollback { .. } => return Ok(self.finish_rollback(actor, world)),
                Action::Retire { .. } => {
                    self.j.may(actor, Effect::RemoveEntrance)?;
                    world.disarm(self.txn())?;
                    return Ok(Ended::Left(
                        "the transaction is over; the next ordinary start deletes its folder and journal"
                            .to_owned(),
                    ));
                }
                Action::Leave => {
                    return Ok(Ended::Left(
                        "the transaction is the running build's to resume or discard".to_owned(),
                    ));
                }
                other => return Err(format!("the recovery met {other:?}")),
            }
        }
        let actor = tenure.unwrap_or_else(|| self.asker.actor(self.j.phase()));
        self.stuck(actor, "the rollback did not settle".to_owned(), world)
    }

    /// **H.3, on Windows** (0.4.7 ticket U-37, design revision (h)): over
    /// `Moving`, or over a `Stuck` whose recorded trial no longer runs, with
    /// every new file installed at its digest, before `decide` —
    /// `update_apply::before_deciding` over `<install>\folio.exe`, this road's
    /// data directory, the handed-back trial of `--from-trial` and this
    /// recovery's own starter. A trial its receipt names exactly is recorded
    /// (`Pre::Again`: the next step is `decide`'s, whose answer for it is the
    /// commit) and is this holder's successor; a handed-back trial that never
    /// became ready is ended by W9's stop of that instance; any other process
    /// of the new build, a held or unaskable claim, or a process list that
    /// cannot be read ends the road `Ended::Deferred` with nothing recorded,
    /// started or moved.
    ///
    /// # Errors
    /// The adoption's record failed; the adopted trial runs on as the window.
    fn before_deciding(
        &mut self,
        worker: &WorkerCtx,
        actor: Actor,
        recorded: Option<TrialProcess>,
        world: &mut impl World,
    ) -> Result<Pre, String> {
        #[cfg(test)]
        if self.road.as_046 {
            return Ok(Pre::Decide);
        }
        let over_stuck = match &self.j.journal.body.phase {
            Phase::Moving => false,
            Phase::Stuck { .. } if recorded.is_none() => true,
            _ => return Ok(Pre::Decide),
        };
        if self.layout.live(&self.site()) != Some(Live::New) {
            return Ok(Pre::Decide);
        }
        let stale = match &self.j.journal.body.phase {
            Phase::Stuck { trial, .. } => *trial,
            _ => None,
        };
        let excluded: Vec<Running> = stale
            .map(|trial| Running {
                pid: trial.pid,
                started: trial.started,
            })
            .into_iter()
            .chain(self.road.starter)
            .collect();
        let road = self.road;
        let installed = road.installed.clone();
        let may_end = self.j.may(actor, Effect::EndTrial);
        let mut end = |process: TrialProcess| {
            may_end.clone()?;
            stop_trial(
                worker,
                process,
                &[installed.as_path()],
                (
                    road.limits.quit_within,
                    road.limits.end_within,
                    road.limits.poll,
                ),
                &mut |line| world.say(line),
            )
        };
        let what = crate::update_apply::Before {
            home: &road.home,
            txn: self.j.journal.txn,
            program: &road.installed,
            over_stuck,
            excluded: &excluded,
            handed_back: road.handed_back,
            data: &road.data,
        };
        match crate::update_apply::before_deciding(&what, &mut end) {
            BeforeDeciding::Decide => Ok(Pre::Decide),
            BeforeDeciding::Defer(deferral) => {
                world.say(&format!("BT_UPDATE_RECOVER deferred: {}", deferral.said()));
                if let Deferral::Candidate(candidate) = &deferral {
                    self.successor = Some(*candidate);
                }
                Ok(Pre::Stop(Ended::Deferred(deferral)))
            }
            BeforeDeciding::Adopt { event, successor } => {
                world.say(&format!(
                    "BT_UPDATE_RECOVER the trial {} runs and its receipt names it, and the journal does not record it: it is recorded",
                    successor.pid
                ));
                self.successor = Some(successor);
                self.j.record(actor, &event)?;
                Ok(Pre::Again)
            }
        }
    }

    /// **W9's moves** under exclusive admission: `rolledout\` made, then each
    /// of `moves` — the new files out, then the old ones back — one
    /// `install_txn::durable_move`, with a look after each. `Some` when the
    /// road stops here: the admission not had ([`Ended::RollbackWaits`],
    /// nothing recorded), or a move that failed (`Stuck`, everything kept).
    fn move_back(
        &mut self,
        actor: Actor,
        moves: &[Move],
        world: &mut impl World,
    ) -> Result<Option<Ended>, String> {
        let admission = match install_txn::hold_within(
            &self.road.home.admission(),
            Hold::Exclusive,
            self.road.limits.old_within,
        ) {
            Ok(Some(held)) => held,
            Ok(None) => {
                let why = "a running copy holds the admission".to_owned();
                world.say(&format!("BT_UPDATE_ROLLBACK {why}; the next start tries"));
                return Ok(Some(Ended::RollbackWaits(why)));
            }
            Err(failure) => {
                return self
                    .stuck(actor, format!("the admission: {failure}"), world)
                    .map(Some);
            }
        };
        let rolledout = self.folder(Place::RolledOut)?;
        match install_txn::durable_create_dir(&rolledout) {
            Ok(()) => {}
            Err(failure) if failure.error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(failure) => {
                return self
                    .stuck(actor, format!("`rolledout`: {failure}"), world)
                    .map(Some);
            }
        }
        // Activate, back: the layout's (managed-update R1).
        let layout = Arc::clone(&self.layout);
        let activated = layout.activate_back(&self.site(), &self.j, actor, moves, world)?;
        if let Activated::Cut(why) = activated {
            drop(admission);
            return self.stuck(actor, why, world).map(Some);
        }
        Ok(None)
    }

    /// **The coordinator's ruling 3 (U-29b), on Windows**: a road that left
    /// the transaction `Stuck` with every new file installed at its digest,
    /// and no trial of it running, starts the new build as a trial over it —
    /// recorded (`RetrialBegan`), so its receipt commits forward (W8) — with
    /// `--update-failed <journal>` and `handed` after its words, and waits for
    /// it under this lock. What the road ended as then: the commit, or `ended`
    /// as it was.
    fn retry_as_trial(
        &mut self,
        worker: &WorkerCtx,
        world: &mut impl World,
        ended: Ended,
        handed: &[OsString],
    ) -> Ended {
        let Phase::Stuck { trial, .. } = &self.j.journal.body.phase else {
            return ended;
        };
        let installed = self.road.installed.clone();
        if trial.is_some_and(|process| trial_runs(process, &[installed.as_path()])) {
            // A trial of it runs already: it is the window.
            return ended;
        }
        if self.layout.live(&self.site()) != Some(Live::New) {
            return ended;
        }
        let actor = match self.asker {
            Asker::LockHolder => Actor::Applier,
            _ => Actor::Recovery,
        };
        // Never a retrial beside a candidate, over a held or unaskable claim,
        // or when what runs cannot be read (H.3); a running trial its receipt
        // names is recorded instead and commits.
        match self.before_deciding(worker, actor, None, world) {
            Ok(Pre::Decide) => {}
            Ok(Pre::Again) => {
                return match self.settle(worker, None, world) {
                    Ok(committed @ (Ended::Committed | Ended::CommittedWithDebt(_))) => committed,
                    Ok(_) => ended,
                    Err(error) => {
                        world.say(&format!("BT_UPDATE_ROLLBACK the trial over Stuck: {error}"));
                        ended
                    }
                };
            }
            // The deferral is the end (H.3): a held claim then starts
            // nothing beside its holder (`ExitGuard::window_elsewhere`).
            Ok(Pre::Stop(deferred)) => return deferred,
            Err(error) => {
                world.say(&format!("BT_UPDATE_ROLLBACK the trial over Stuck: {error}"));
                return ended;
            }
        }
        match self.begin_retrial(worker, actor, world, handed) {
            Ok(committed @ (Ended::Committed | Ended::CommittedWithDebt(_))) => committed,
            Ok(_) => ended,
            Err(error) => {
                world.say(&format!("BT_UPDATE_ROLLBACK the trial over Stuck: {error}"));
                ended
            }
        }
    }

    /// The new build started as a trial over `Stuck`, recorded, and waited
    /// for.
    fn begin_retrial(
        &mut self,
        worker: &WorkerCtx,
        actor: Actor,
        world: &mut impl World,
        handed: &[OsString],
    ) -> Result<Ended, String> {
        let nonce = crate::update_job::mint_nonce();
        let began_ms = now_ms();
        let mut args = trial_words(self.txn(), &nonce).to_vec();
        // Its card says the update is incomplete, as `Stuck`'s start's
        // always does (U-29).
        args.extend(failed_words(&self.road.home));
        args.extend_from_slice(handed);
        let pid = match world.launch_trial(&self.road.installed, &args) {
            Ok(pid) => pid,
            Err(error) => {
                world.say(&format!(
                    "BT_UPDATE_APPLY {} did not start: {error}",
                    self.road.installed.display()
                ));
                return Ok(self.still_stuck());
            }
        };
        let started = install_flip::started_of(pid).unwrap_or(0);
        self.successor = Some(Running { pid, started });
        // A retrial whose start cannot be recorded runs on as the window: it
        // carries `--update-failed`, and a trial of this transaction that no
        // journal records is what `Opens::Trial` starts over `Stuck` (U-34).
        self.j.record(
            actor,
            &Event::RetrialBegan {
                nonce,
                process: TrialProcess { pid, started },
                began_ms,
            },
        )?;
        Ok(if self.watch(worker, actor, world)? {
            self.retire_committed(actor, world)
        } else {
            self.still_stuck()
        })
    }

    /// `Stuck`'s end as it stands: its last error.
    fn still_stuck(&self) -> Ended {
        match &self.j.journal.body.phase {
            Phase::Stuck { last_error, .. } => Ended::Stuck(last_error.clone()),
            other => Ended::Failed(format!("{:?} is not Stuck", other.kind())),
        }
    }

    /// W10: `RollbackFailed` with `why` — `Stuck`, durable, everything kept —
    /// and the line naming the journal's folder.
    fn stuck(
        &mut self,
        actor: Actor,
        why: String,
        world: &mut impl World,
    ) -> Result<Ended, String> {
        self.j
            .record(actor, &Event::RollbackFailed { error: why.clone() })?;
        world.say(&format!(
            "BT_UPDATE_ROLLBACK the update is incomplete ({why}); see {}",
            self.road.home.root().display()
        ));
        Ok(Ended::Stuck(why))
    }

    /// W11 after `RolledBack` is durable: the entrance removed and flushed,
    /// then `Retired{RolledBack}`. `H\<txn>` — `backup\`'s leftovers,
    /// `rolledout\` and the rescue folder this process runs from — is the next
    /// ordinary start's. Every failure here is debt.
    fn finish_rollback(&mut self, actor: Actor, world: &mut impl World) -> Ended {
        let steps = self
            .j
            .may(actor, Effect::RemoveEntrance)
            .and_then(|()| world.disarm(self.txn()))
            .and_then(|()| self.j.record(actor, &Event::Retired));
        match steps {
            Ok(()) => Ended::RolledBack,
            Err(debt) => Ended::RolledBackWithDebt(debt),
        }
    }

    /// W4, W5: the entrance removed, then `Reverted` to `Prepared`.
    fn revert(&mut self, actor: Actor, world: &mut impl World) -> Result<Ended, String> {
        self.j.may(actor, Effect::RemoveEntrance)?;
        world.disarm(self.txn())?;
        self.j.record(actor, &Event::Reverted)?;
        Ok(Ended::Reverted)
    }

    /// **W8 after the commit, and W12** (the coordinator's order): `Committed`
    /// is durable; the entrance removed and flushed; then exactly the recorded
    /// old files `backup\` holds, by their digests (`update_txn::decide`'s
    /// `FinishCommit`) — never a file the journal does not record; then
    /// `Retired{Committed}`. Every failure is debt, never a rollback; `H\<txn>`
    /// — the emptied `backup\`, and the rescue folder this process runs from —
    /// is the next ordinary start's.
    fn retire_committed(&mut self, actor: Actor, world: &mut impl World) -> Ended {
        match self.retire_committed_steps(actor, world) {
            Ok(()) => Ended::Committed,
            Err(debt) => Ended::CommittedWithDebt(debt),
        }
    }

    fn retire_committed_steps(
        &mut self,
        actor: Actor,
        world: &mut impl World,
    ) -> Result<(), String> {
        let located = self.layout.locate(&self.site())?;
        let entrance = world.is_armed(self.txn()).unwrap_or(true);
        let disk = Disk {
            journal: &self.j.journal,
            asker: Asker::LockHolder,
            entrance,
            located,
            receipt: None,
            trial_alive: false,
            now_ms: now_ms(),
        };
        let Action::FinishCommit {
            delete_backup,
            remove_entrance,
            ..
        } = decide(&disk)
        else {
            return Err(format!(
                "{:?} is not a committed transaction",
                self.j.phase()
            ));
        };
        if remove_entrance {
            self.j.may(actor, Effect::RemoveEntrance)?;
            world.disarm(self.txn())?;
        }
        let backup = self.folder(Place::Backup)?;
        for name in &delete_backup {
            self.j.may(actor, Effect::DeleteRollbackMaterial)?;
            install_txn::durable_remove(&backup.join(name))
                .map_err(|failure| failure.to_string())?;
        }
        self.j.record(actor, &Event::Retired)
    }
}

/// **The SHA-256 of the regular file at `path`**, `None` where there is none,
/// or why a file that is there could not be read — "held open by another
/// program" when another process holds it with no sharing (U-42b; W10's
/// hold), the read's own error otherwise.
fn read_digest_at(path: &Path) -> Result<Option<Digest>, String> {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(None);
    };
    if !meta.is_file() {
        return Ok(None);
    }
    match crate::update_prepare::digest_of(path) {
        Ok((digest, _)) => Ok(Digest::parse(&digest).ok()),
        Err(error) => Err(match install_flip::held_open(path) {
            Ok(true) => String::from("held open by another program"),
            _ => error.to_string(),
        }),
    }
}

/// **Wait until nothing `held` names is held any more, within `window`**
/// (E-7): ask, and while something is held sleep one `poll` (never past
/// `window`) and ask again — so a hold let go is seen at the next poll, and a
/// hold that outlasts `window` refuses with the names still held. `now` is the
/// clock and `sleep` the pause (the worker's wait door in the product), so a
/// test can count the polls (U-42d, review finding 5).
///
/// # Errors
/// What `held` refused with, or the files still held when `window` passed.
fn until_let_go(
    window: Instant,
    poll: std::time::Duration,
    now: &mut dyn FnMut() -> Instant,
    held: &mut dyn FnMut() -> Result<Vec<String>, String>,
    sleep: &mut dyn FnMut(std::time::Duration),
) -> Result<(), String> {
    loop {
        let names = held()?;
        if names.is_empty() {
            return Ok(());
        }
        let left = window.saturating_duration_since(now());
        if left.is_zero() {
            return Err(format!(
                "held open by another process: {}",
                names.join(", ")
            ));
        }
        sleep(poll.min(left));
    }
}

/// The SHA-256 of the regular file at `path`, or `None` where there is none.
fn digest_at(path: &Path) -> Option<Digest> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let (digest, _) = crate::update_prepare::digest_of(path).ok()?;
    Digest::parse(&digest).ok()
}

/// **This process's own world**: the current user's `Run` key, the system's
/// processes, and lines in the file `log` names — or on standard error while
/// it names none.
pub(crate) struct Machine {
    pub(crate) log: Option<PathBuf>,
}

impl World for Machine {
    /// **One writer per line** (0.4.7 ticket U-42d; 0.4.6's D-11): the log
    /// once one is named, standard error only before. The applier's standard
    /// error is the one it inherited from the Folio that started it, whose
    /// streams are that Folio's `diagnostics.log` — the same file `log`
    /// names — so writing both put every line in the file twice.
    fn say(&mut self, line: &str) {
        match &self.log {
            Some(log) => {
                let _ = crate::diagnostics::append_note(log, line);
            }
            None => {
                bt_platform::write_std_error(format!("{line}\n").as_bytes());
            }
        }
    }

    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()> {
        // `quiet_command` is the one door for a child; it is dropped at once,
        // never waited on or ended.
        bt_platform::quiet_command(program)
            .args(args)
            .spawn()
            .map(drop)
    }

    fn arm(&mut self, txn: TxnId, rescue: &Path) -> Result<Armed, String> {
        bt_platform::logon_hook::arm(txn.bytes(), rescue).map_err(|refusal| refusal.to_string())
    }

    fn disarm(&mut self, txn: TxnId) -> Result<(), String> {
        bt_platform::logon_hook::disarm(txn.bytes()).map_err(|refusal| refusal.to_string())
    }

    fn acknowledged(&mut self, worker: Option<&WorkerCtx>, data: &Path) -> bool {
        crate::update_apply::claimed_within(worker, data, crate::update_apply::ACKNOWLEDGED_WITHIN)
    }

    fn show_here(&mut self, text: &str) {
        bt_platform::standalone_alert(crate::APP_NAME, text);
    }

    fn is_armed(&mut self, txn: TxnId) -> Result<bool, String> {
        use bt_platform::logon_hook::{self, Registry};
        logon_hook::CurrentUser
            .get(logon_hook::RUN_KEY, &logon_hook::value_name(txn.bytes()))
            .map(|value| value.is_some())
            .map_err(|error| error.to_string())
    }

    fn launch_trial(&mut self, program: &Path, args: &[OsString]) -> io::Result<u32> {
        // Detached: the child's handle is dropped, never waited on; the pid
        // and its start time are what the journal keeps.
        bt_platform::quiet_command(program)
            .args(args)
            .spawn()
            .map(|child| child.id())
    }
}

#[cfg(test)]
#[path = "update_apply_windows_tests.rs"]
pub(crate) mod tests;
