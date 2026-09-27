//! **`folio --update-apply <home> <txn> <nonce>` on macOS: the applier**
//! (0.4.6 ticket U-28; `docs/plans/design/self-update-2026-09-16.md` §C.4,
//! §C.5, revision (b) §(b).2's M3–M8 and "Who may write what").
//!
//! O, on its way out, wrote `Handoff{applier: <nonce>}` durably and started
//! the rescue clone with this line (`update_handoff`, U-21). This process — P,
//! the rescue clone, a copy of O's own build — takes the transaction from
//! there to `Committed`, or stops at a named phase for the next lock holder:
//!
//! 1. **Wait for O** (§C.4). The transaction lock `H/lock`, which O holds
//!    until its process ends, is asked for through
//!    `install_txn::hold_within` for up to [`Limits::old_within`]; not had →
//!    nothing is written (the journal stays `Handoff`, M3: the first lock
//!    holder applies it). Then the authoritative test: the data directory's
//!    claim is tried (`persist::try_claim`) until it is had, and **let go at
//!    once** — P is not the writer and never becomes one — sleeping between
//!    tries through the worker's wait door (`bt_platform::wait::sleep_within`),
//!    within the same window. Still held when the window runs out, or the
//!    claim cannot be asked about at all → [`Event::OldStayed`]: `Abandoned`,
//!    and the transaction's folder and journal are cleared. Nothing moved.
//! 2. **Arm** (M3 → M4): the LaunchAgent plist for the rescue executable with
//!    `--update-recover <home>` (`bt_platform::launch_agent::arm`, which reads
//!    it back and only then makes the `install_txn::Armed` proof); journal
//!    `Armed`, durable. A plist already there from a dead attempt → removed,
//!    `Reverted` (W4). An entrance that cannot be made durable →
//!    `EntranceFailed`, `Abandoned`, cleared.
//! 3. **Admit** (M4 → M5): exclusive admission on `H/admission`, within what
//!    is left of the window, and no process running from the installed
//!    bundle's executable (`bt_platform::install_flip::running_from`) →
//!    `Admitted`: the journal says `Moving` (the note's `Exchanging`), with
//!    both identities already in its layout since `Prepared`. Refused → the
//!    plist removed, `Reverted` to `Prepared`.
//! 4. **Exchange** (M5 → M6): the live bundle must still be the journal's
//!    old identity and the staged one its new identity (cdhash and version,
//!    read with `codesign` and `plutil`); then one
//!    `renamex_np(RENAME_SWAP)` (`bt_platform::install_flip::exchange`). The
//!    exclusive admission is let go right after, so the trial can take its
//!    shared hold. Either identity changed → nothing swapped, `Reverted`.
//! 5. **Trial** (M6 → M7): `open -n -a <bundle> --args --update-trial <txn>
//!    <nonce>` with a fresh nonce, detached; then, polling through the wait
//!    door until [`Limits::trial_within_ms`] after the launch: the trial's pid,
//!    by listing the processes whose image is the installed executable and
//!    started after the launch (LaunchServices reports none), and its receipt
//!    `H/<txn>/health-<nonce>`. A pid (or a receipt, whose own pid is taken
//!    when the list has none yet) → `Trial{nonce, process, began_ms}`, durable.
//! 6. **Commit** (M7 → M8): a receipt the journal accepts — this transaction's,
//!    this trial's nonce, while the journal says `Trial` — →
//!    `Committed{outcome: committed}`, durable; **then** the old bundle in
//!    `stage/` (checked to be the old identity) is removed, the plist removed,
//!    `Retired{Committed}` recorded, and `H/<txn>` removed with the rescue
//!    clone this process runs from (a Unix process may remove its own image).
//!    The journal itself is kept at `Retired{Committed}`: the trial's watch
//!    releases its held-back writes when it reads `committed` there, and would
//!    drop them if it found no journal; the next ordinary start retires it.
//! 7. **No receipt by the deadline, or the trial died without one** →
//!    `RollbackIntent{trial}`, and P stops: the swap back and `Stuck` are
//!    U-29's. Until U-29, an ordinary start reads that `destructive` header
//!    and hands itself to the rescue build, which says the update is not
//!    finished and starts nothing (U-12, U-22); the plist stays armed for the
//!    same reason.
//!
//! **Re-entry** (P started again over its own transaction): `Armed` continues
//! from step 3. `Moving` is decided by the live identity, as recovery decides
//! it (M5, M6): still the old bundle → the plist removed, `Reverted`; the new
//! one → `RollbackIntent`. Anything else is refused and nothing is touched.
//!
//! Every phase is recorded through `update_txn::Journal::advance` (which
//! refuses what the protocol does not allow) and written with
//! `install_txn::durable_write` only after `update_txn::may_record` says this
//! actor may; every effect on a file, the entrance or the bundles is asked of
//! `update_txn::may` first. Headless: the main thread is a worker
//! (`admission::enter_standalone_main`), and its only sleeps are the wait
//! door's.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bt_platform::admission::WorkerCtx;
use bt_platform::file_reads::{self, Lane};
use bt_platform::install_flip::{self, Running};
use bt_platform::install_txn::{self, Held, Hold};
use bt_platform::{HostPlatform, launch_agent};

use crate::cli;
use crate::update_txn::{
    Actor, BundleIdentity, Effect, Event, Home, Journal, Layout, Nonce, Phase, PhaseKind, Receipt,
    TrialProcess, TxnId,
};

/// `open`, by its absolute path: LaunchServices starts the trial as it starts
/// any application (§C.5).
pub(crate) const OPEN: &str = "/usr/bin/open";

/// **How long the applier waits, and how often it looks.**
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    /// For O's lock, O's claim and the exclusive admission together (§C.4's
    /// 60 s).
    pub(crate) old_within: Duration,
    /// For the trial's pid and receipt, counted from the launch (§C.5's
    /// 90 s, `update_txn::TRIAL_DEADLINE_MS`).
    pub(crate) trial_within_ms: u64,
    /// Between two looks at the claim, and at the trial.
    pub(crate) poll: Duration,
}

impl Limits {
    /// The product's.
    pub(crate) const PRODUCT: Self = Self {
        old_within: Duration::from_secs(60),
        trial_within_ms: crate::update_txn::TRIAL_DEADLINE_MS,
        poll: Duration::from_millis(250),
    };
}

/// **One applier's road**: the home the line named, the transaction and the
/// applier's nonce it names, and where the data directory and the
/// LaunchAgents folder are.
pub(crate) struct Road {
    pub(crate) home: Home,
    pub(crate) txn: TxnId,
    pub(crate) nonce: Nonce,
    /// The data directory whose claim O held (the same storage rule O ran
    /// under: the applier inherits O's environment).
    pub(crate) data: PathBuf,
    /// `~/Library/LaunchAgents`, where the entrance goes; `None` when there is
    /// no home folder to find it from.
    pub(crate) agents: Option<PathBuf>,
    pub(crate) limits: Limits,
}

/// **The applier's effects that a test stands in for**: its lines, the
/// exchange, and the trial's launch. Everything else — the journal, the locks,
/// the claim, the entrance, the identities, the process list, the receipt and
/// the deletions — is the real door in the product and in the tests.
pub(crate) trait World {
    /// One line of what happened.
    fn say(&mut self, line: &str);
    /// The one exchange of the installed bundle with the staged one.
    ///
    /// # Errors
    /// The exchange's refusal, or a flush after it that failed.
    fn exchange(&mut self, live: &Path, staged: &Path) -> Result<(), String>;
    /// Start the trial: `open -n -a <bundle> --args <args>`, detached.
    ///
    /// # Errors
    /// `open` could not be started.
    fn launch_trial(&mut self, bundle: &Path, args: &[OsString]) -> io::Result<()>;
}

/// **Where the applier stopped.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ended {
    /// `Committed`, then the old bundle, the entrance and `H/<txn>` removed.
    Committed,
    /// `Committed` is durable; a deletion after it failed and is the next
    /// actor's debt (M11), never a rollback.
    CommittedWithDebt(String),
    /// `RollbackIntent` is durable; U-29's lock holder rolls back.
    RollbackIntent,
    /// `Reverted` to `Prepared`: the entrance was found or removed, admission
    /// was refused, or the bundles were not the journal's.
    Reverted,
    /// `Abandoned` and cleared: O did not let go, or the entrance could not be
    /// armed.
    Abandoned,
    /// O still held the transaction lock at the end of the wait: nothing was
    /// written.
    OldHeldTheLock,
    /// The line, the home or the journal is not an applier's to act on:
    /// nothing was touched.
    Refused(String),
    /// A write or a read failed; the journal holds its last durable phase.
    Failed(String),
}

impl Ended {
    /// The process's exit code.
    pub(crate) fn code(&self) -> i32 {
        match self {
            Ended::Committed | Ended::CommittedWithDebt(_) => 0,
            Ended::Refused(_) => 2,
            _ => 1,
        }
    }
}

/// **The door, for this process**: this executable must be a macOS rescue
/// clone, and `home` a locator's home.
pub(crate) fn run_here(home: &Path, txn: &str, nonce: &str) -> i32 {
    let mut world = Machine { log: None };
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            world.say(&format!(
                "BT_UPDATE_APPLY cannot name its own executable: {error}"
            ));
            return 2;
        }
    };
    let Some((home, _installed)) = Home::of_rescue_named(HostPlatform::MacOs, &exe, home) else {
        world.say(&format!(
            "BT_UPDATE_APPLY {} is not an installation home of {}; {} runs only from a rescue clone",
            home.display(),
            exe.display(),
            cli::UPDATE_APPLY_FLAG
        ));
        return 2;
    };
    let data = crate::persist::storage_dir_unmoved();
    world.log = Some(crate::update_recover::log_file(&home, &data));
    let (Ok(txn), Ok(nonce)) = (TxnId::parse(txn), Nonce::parse(nonce)) else {
        world.say(&format!(
            "BT_UPDATE_APPLY malformed transaction or nonce; {}",
            cli::UPDATE_APPLY_USAGE
        ));
        return 2;
    };
    let road = Road {
        home,
        txn,
        nonce,
        data,
        agents: launch_agents(),
        limits: Limits::PRODUCT,
    };
    match bt_platform::admission::enter_standalone_main("folio-update-apply", |worker| {
        apply(worker, &road, &mut world)
    }) {
        Ok(ended) => {
            world.say(&format!("BT_UPDATE_APPLY transaction {txn}: {ended:?}"));
            ended.code()
        }
        Err(refused) => {
            world.say(&format!("BT_UPDATE_APPLY {refused:?}"));
            2
        }
    }
}

/// **`~/Library/LaunchAgents`**, from `HOME`: where the update entrance lives
/// (F-3; `uninstall`'s row reads the same folder).
pub(crate) fn launch_agents() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/LaunchAgents"))
}

/// **An ordinary start's retirement of a finished transaction's LaunchAgent**
/// (`StartAction::Retire`'s `RemoveEntrance`, `update_startup`): the plist of
/// `txn` in `agents` removed and the folder flushed; none there, or no folder
/// to look in, is success.
///
/// # Errors
/// The door's refusal, which stops the retirement (the journal stays).
pub(crate) fn retire_entrance_in(agents: Option<&Path>, txn: TxnId) -> Result<(), String> {
    match agents {
        Some(agents) => {
            launch_agent::disarm(agents, txn.bytes()).map_err(|refusal| refusal.to_string())
        }
        None => Ok(()),
    }
}

/// **The applier, over any road** — see the module header.
pub(crate) fn apply(worker: &WorkerCtx, road: &Road, world: &mut impl World) -> Ended {
    let window = Instant::now() + road.limits.old_within;
    let (Some(installed), Some(program), Some(stage), Some(rescue)) = (
        road.home.installed_bundle(),
        road.home.installed_program(),
        road.home.stage_bundle(road.txn),
        road.home.rescue_executable(road.txn),
    ) else {
        return Ended::Refused("the home is not a macOS bundle's".to_owned());
    };
    let lock = match install_txn::hold_within(
        &road.home.lock(),
        Hold::Exclusive,
        road.limits.old_within,
    ) {
        Ok(Some(held)) => held,
        Ok(None) => return Ended::OldHeldTheLock,
        Err(failure) => return Ended::Failed(failure.to_string()),
    };
    let journal = match file_reads::read(Lane::UpdateJournal, road.home.journal()) {
        Ok(bytes) => match Journal::parse(&bytes) {
            Ok(journal) => journal,
            Err(refusal) => return Ended::Refused(format!("the journal: {refusal}")),
        },
        Err(error) => return Ended::Refused(format!("the journal: {error}")),
    };
    if journal.txn != road.txn {
        return Ended::Refused(format!(
            "the journal is transaction {}, not {}",
            journal.txn, road.txn
        ));
    }
    let Layout::Bundle { old, new } = journal.body.layout.clone() else {
        return Ended::Refused("the journal is not a bundle's".to_owned());
    };
    let mut txn = Txn {
        road,
        journal,
        _lock: lock,
        plist: road
            .agents
            .as_ref()
            .map(|agents| agents.join(launch_agent::file_name(road.txn.bytes()))),
        written: Vec::new(),
    };
    let places = Places {
        installed: &installed,
        program: &program,
        stage: &stage,
        rescue: &rescue,
        old: &old,
        new: &new,
    };
    let ended = txn.run(worker, &places, window, world);
    world.say(&format!(
        "BT_UPDATE_APPLY transaction {} wrote {:?}",
        road.txn, txn.written
    ));
    ended
}

/// The paths and identities one road acts on.
struct Places<'a> {
    installed: &'a Path,
    program: &'a Path,
    stage: &'a Path,
    rescue: &'a Path,
    old: &'a BundleIdentity,
    new: &'a BundleIdentity,
}

/// **One transaction under its lock**: the journal as it stands durably, and
/// every phase this process wrote, in order.
struct Txn<'a> {
    road: &'a Road,
    journal: Journal,
    /// Held for the whole road; let go when the process leaves.
    _lock: Held,
    plist: Option<PathBuf>,
    written: Vec<PhaseKind>,
}

impl Txn<'_> {
    fn phase(&self) -> PhaseKind {
        self.journal.body.phase.kind()
    }

    /// **Record `event` as `actor`**: the next phase by the protocol, allowed
    /// to this actor, then durable.
    fn record(&mut self, actor: Actor, event: &Event) -> Result<(), String> {
        let next = self
            .journal
            .advance(event)
            .map_err(|refusal| format!("{refusal:?}"))?;
        let phase = next.body.phase.kind();
        if !crate::update_txn::may_record(actor, phase) {
            return Err(format!("{actor:?} may not record {phase:?}"));
        }
        install_txn::durable_write(&self.road.home.journal(), &next.encode())
            .map_err(|failure| failure.to_string())?;
        self.journal = next;
        self.written.push(phase);
        Ok(())
    }

    /// Whether `actor` may do `effect` now.
    fn may(&self, actor: Actor, effect: Effect) -> Result<(), String> {
        if crate::update_txn::may(actor, effect, self.phase()) {
            Ok(())
        } else {
            Err(format!(
                "{actor:?} may not {effect:?} in {:?}",
                self.phase()
            ))
        }
    }

    fn run(
        &mut self,
        worker: &WorkerCtx,
        places: &Places<'_>,
        window: Instant,
        world: &mut impl World,
    ) -> Ended {
        let outcome = match self.journal.body.phase.clone() {
            Phase::Handoff { applier } if applier == self.road.nonce => {
                self.at_handoff(worker, places, window, world)
            }
            Phase::Handoff { .. } => {
                return Ended::Refused("the journal was handed to another applier".to_owned());
            }
            Phase::Armed => self.at_armed(worker, places, window, world),
            Phase::Moving => self.at_moving_again(worker, places),
            other => {
                return Ended::Refused(format!(
                    "the journal says {:?}, which is no applier's",
                    other.kind()
                ));
            }
        };
        outcome.unwrap_or_else(Ended::Failed)
    }

    /// M3: wait for O, then arm.
    fn at_handoff(
        &mut self,
        worker: &WorkerCtx,
        places: &Places<'_>,
        window: Instant,
        world: &mut impl World,
    ) -> Result<Ended, String> {
        if let Err(why) = wait_for_the_claim(worker, self.road, window) {
            world.say(&format!(
                "BT_UPDATE_APPLY the old build did not let go of {}: {why}",
                self.road.data.display()
            ));
            self.record(Actor::Applier, &Event::OldStayed)?;
            return self.clear();
        }
        let Some(plist) = self.plist.clone() else {
            world.say("BT_UPDATE_APPLY no home folder, so no LaunchAgents folder to arm");
            self.record(Actor::Applier, &Event::EntranceFailed)?;
            return self.clear();
        };
        if std::fs::symlink_metadata(&plist).is_ok() {
            // W4: an entrance from a dead attempt; nothing was moved.
            return self.revert(Actor::Applier);
        }
        let agents = plist.parent().unwrap_or(Path::new("."));
        self.may(Actor::Applier, Effect::WriteEntrance)?;
        match launch_agent::arm(
            agents,
            self.road.txn.bytes(),
            places.rescue,
            self.road.home.root(),
        ) {
            Ok(armed) => self.record(Actor::Applier, &Event::Armed(armed))?,
            Err(refusal) => {
                world.say(&format!("BT_UPDATE_APPLY the entrance: {refusal}"));
                self.record(Actor::Applier, &Event::EntranceFailed)?;
                return self.clear();
            }
        }
        self.at_armed(worker, places, window, world)
    }

    /// M4: admit, exchange, trial.
    fn at_armed(
        &mut self,
        worker: &WorkerCtx,
        places: &Places<'_>,
        window: Instant,
        world: &mut impl World,
    ) -> Result<Ended, String> {
        let left = window.saturating_duration_since(Instant::now());
        let admission =
            match install_txn::hold_within(&self.road.home.admission(), Hold::Exclusive, left) {
                Ok(Some(held)) => held,
                Ok(None) => {
                    world.say("BT_UPDATE_APPLY a running copy holds the admission");
                    return self.revert(Actor::Applier);
                }
                Err(failure) => {
                    world.say(&format!("BT_UPDATE_APPLY the admission: {failure}"));
                    return self.revert(Actor::Applier);
                }
            };
        match install_flip::running_from(places.program) {
            Ok(running) if running.is_empty() => {}
            Ok(running) => {
                world.say(&format!(
                    "BT_UPDATE_APPLY {} still runs from {}",
                    running
                        .iter()
                        .map(|process| process.pid.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    places.program.display()
                ));
                return self.revert(Actor::Applier);
            }
            Err(error) => {
                world.say(&format!("BT_UPDATE_APPLY the process list: {error}"));
                return self.revert(Actor::Applier);
            }
        }
        self.record(Actor::Applier, &Event::Admitted)?;
        let live = crate::update_prepare_macos::identity(worker, places.installed);
        let staged = crate::update_prepare_macos::identity(worker, places.stage);
        if live.as_ref() != Ok(places.old) || staged.as_ref() != Ok(places.new) {
            world.say(&format!(
                "BT_UPDATE_APPLY the bundles are not the journal's: live {live:?}, staged {staged:?}"
            ));
            return self.revert(Actor::Applier);
        }
        self.may(Actor::Applier, Effect::Swap)?;
        if let Err(error) = world.exchange(places.installed, places.stage) {
            world.say(&format!("BT_UPDATE_APPLY the exchange: {error}"));
            // Decided by what is live, as recovery decides it (M5, M6).
            return if crate::update_prepare_macos::identity(worker, places.installed).as_ref()
                == Ok(places.old)
            {
                self.revert(Actor::Applier)
            } else {
                self.record(Actor::Applier, &Event::RollbackDeclared)?;
                Ok(Ended::RollbackIntent)
            };
        }
        // The trial takes its shared hold at its start.
        drop(admission);
        self.trial(worker, places, world)
    }

    /// M5 / M6, found by a process started again over its own transaction:
    /// decided by the live identity, never by the phase.
    fn at_moving_again(
        &mut self,
        worker: &WorkerCtx,
        places: &Places<'_>,
    ) -> Result<Ended, String> {
        let live = crate::update_prepare_macos::identity(worker, places.installed);
        if live.as_ref() == Ok(places.old) {
            self.revert(Actor::Recovery)
        } else {
            self.record(Actor::Recovery, &Event::RollbackDeclared)?;
            Ok(Ended::RollbackIntent)
        }
    }

    /// M6 → M7 → M8, or `RollbackIntent`.
    fn trial(
        &mut self,
        worker: &WorkerCtx,
        places: &Places<'_>,
        world: &mut impl World,
    ) -> Result<Ended, String> {
        let nonce = crate::update_job::mint_nonce();
        let began_ms = now_ms();
        let args = [
            OsString::from(cli::UPDATE_TRIAL_FLAG),
            OsString::from(self.road.txn.to_string()),
            OsString::from(nonce.to_string()),
        ];
        if let Err(error) = world.launch_trial(places.installed, &args) {
            world.say(&format!("BT_UPDATE_APPLY {OPEN} did not start: {error}"));
            self.record(Actor::Applier, &Event::RollbackDeclared)?;
            return Ok(Ended::RollbackIntent);
        }
        let deadline = began_ms.saturating_add(self.road.limits.trial_within_ms);
        let receipt_path = self.road.home.receipt_path(self.road.txn, &nonce);
        let mut said_refusal = false;
        loop {
            let receipt = read_receipt(&receipt_path);
            if self.phase() == PhaseKind::Moving {
                let listed = install_flip::running_from(places.program)
                    .ok()
                    .and_then(|list| {
                        list.into_iter()
                            .filter(|process| process.started >= began_ms.saturating_mul(1000))
                            .min_by_key(|process| process.started)
                    });
                let process = listed.or_else(|| {
                    receipt
                        .as_ref()
                        .and_then(|found| found.as_ref().ok())
                        .map(|receipt| Running {
                            pid: receipt.pid,
                            started: install_flip::started_of(receipt.pid).unwrap_or(0),
                        })
                });
                if let Some(process) = process {
                    self.record(
                        Actor::Applier,
                        &Event::TrialBegan {
                            nonce,
                            process: TrialProcess {
                                pid: process.pid,
                                started: process.started,
                            },
                            began_ms,
                        },
                    )?;
                }
            }
            if let Phase::Trial { process, .. } = self.journal.body.phase.clone() {
                match receipt {
                    Some(Ok(receipt)) => {
                        let event = Event::ReceiptAccepted(receipt);
                        match self.journal.advance(&event) {
                            Ok(_) => {
                                self.record(Actor::Applier, &event)?;
                                return Ok(self.commit(worker, places));
                            }
                            Err(refusal) if !said_refusal => {
                                said_refusal = true;
                                world.say(&format!(
                                    "BT_UPDATE_APPLY the receipt at {} is refused: {refusal:?}",
                                    receipt_path.display()
                                ));
                            }
                            Err(_) => {}
                        }
                    }
                    Some(Err(error)) if !said_refusal => {
                        said_refusal = true;
                        world.say(&format!("BT_UPDATE_APPLY the receipt: {error}"));
                    }
                    _ => {}
                }
                let alive = install_flip::still_running(Running {
                    pid: process.pid,
                    started: process.started,
                });
                if !alive {
                    world.say(&format!(
                        "BT_UPDATE_APPLY the trial {} ended without a receipt",
                        process.pid
                    ));
                    self.record(Actor::Applier, &Event::RollbackDeclared)?;
                    return Ok(Ended::RollbackIntent);
                }
            }
            let now = now_ms();
            if now >= deadline {
                world.say("BT_UPDATE_APPLY no receipt by the trial's deadline");
                self.record(Actor::Applier, &Event::RollbackDeclared)?;
                return Ok(Ended::RollbackIntent);
            }
            bt_platform::wait::sleep_within(
                worker,
                self.road
                    .limits
                    .poll
                    .min(Duration::from_millis(deadline - now)),
            );
        }
    }

    /// M8, after `Committed` is durable: the old bundle, the entrance, then
    /// `Retired{Committed}` and `H/<txn>`. Every failure here is debt.
    fn commit(&mut self, worker: &WorkerCtx, places: &Places<'_>) -> Ended {
        let mut debt = Vec::new();
        if crate::update_prepare_macos::identity(worker, places.stage).as_ref() == Ok(places.old) {
            match self.may(Actor::Applier, Effect::DeleteRollbackMaterial) {
                Ok(()) => {
                    if let Err(failure) = install_txn::durable_remove(places.stage) {
                        debt.push(failure.to_string());
                    }
                }
                Err(why) => debt.push(why),
            }
        }
        if let Err(why) = self.disarm(Actor::Applier) {
            debt.push(why);
        }
        if let Err(why) = self.record(Actor::Applier, &Event::Retired) {
            debt.push(why);
        } else {
            match self.may(Actor::Applier, Effect::DeleteTxnDir) {
                Ok(()) => {
                    if let Err(failure) =
                        install_txn::durable_remove(&self.road.home.transaction(self.road.txn))
                    {
                        debt.push(failure.to_string());
                    }
                }
                Err(why) => debt.push(why),
            }
        }
        if debt.is_empty() {
            Ended::Committed
        } else {
            Ended::CommittedWithDebt(debt.join("; "))
        }
    }

    /// Remove the entrance if it is there.
    fn disarm(&self, actor: Actor) -> Result<(), String> {
        let Some(plist) = &self.plist else {
            return Ok(());
        };
        if std::fs::symlink_metadata(plist).is_err() {
            return Ok(());
        }
        self.may(actor, Effect::RemoveEntrance)?;
        let agents = plist.parent().unwrap_or(Path::new("."));
        launch_agent::disarm(agents, self.road.txn.bytes()).map_err(|refusal| refusal.to_string())
    }

    /// W4, W5, M5: the entrance removed, then `Reverted` to `Prepared`.
    fn revert(&mut self, actor: Actor) -> Result<Ended, String> {
        self.disarm(actor)?;
        self.record(actor, &Event::Reverted)?;
        Ok(Ended::Reverted)
    }

    /// W13 for a transaction this applier abandoned: `H/<txn>`, then the
    /// journal — kept when the folder could not be removed, so the next start
    /// retires both.
    fn clear(&mut self) -> Result<Ended, String> {
        self.may(Actor::Applier, Effect::DeleteTxnDir)?;
        install_txn::durable_remove(&self.road.home.transaction(self.road.txn))
            .map_err(|failure| failure.to_string())?;
        self.may(Actor::Applier, Effect::DeleteJournal)?;
        install_txn::durable_remove(&self.road.home.journal())
            .map_err(|failure| failure.to_string())?;
        Ok(Ended::Abandoned)
    }
}

/// **§C.4's authoritative test that O is gone**: the data directory's claim,
/// tried until it is had and let go at once, sleeping between tries through
/// the wait door, until `until`. A claim that cannot be asked about is not
/// asked again (`ClaimRefusal::QueryDenied`: no answer is coming).
fn wait_for_the_claim(worker: &WorkerCtx, road: &Road, until: Instant) -> Result<(), String> {
    loop {
        match crate::persist::try_claim(&road.data) {
            Ok(claim) => {
                drop(claim);
                return Ok(());
            }
            Err(bt_platform::instance::ClaimRefusal::Held) => {
                let left = until.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err("still held at the end of the wait".to_owned());
                }
                bt_platform::wait::sleep_within(worker, road.limits.poll.min(left));
            }
            Err(refusal) => return Err(format!("{refusal:?}")),
        }
    }
}

/// The receipt at `path`: `None` while there is none, else what it says.
fn read_receipt(path: &Path) -> Option<Result<Receipt, String>> {
    match file_reads::read(Lane::UpdateJournal, path) {
        Ok(bytes) => Some(Receipt::parse(&bytes).map_err(|refusal| refusal.to_string())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => Some(Err(error.to_string())),
    }
}

/// Wall-clock milliseconds, as `Trial`'s `began_ms` is recorded (recovery in
/// another process measures the same deadline from it).
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// This process's own world.
struct Machine {
    /// Where each line is appended besides standard error: the data
    /// directory's `diagnostics.log`, or `recover.log` in the home.
    log: Option<(PathBuf, String)>,
}

impl World for Machine {
    fn say(&mut self, line: &str) {
        bt_platform::write_std_error(format!("{line}\n").as_bytes());
        if let Some((log, _)) = &self.log {
            let _ = crate::diagnostics::append_note(log, line);
        }
    }

    fn exchange(&mut self, live: &Path, staged: &Path) -> Result<(), String> {
        install_flip::exchange(live, staged).map_err(|failure| failure.to_string())
    }

    fn launch_trial(&mut self, bundle: &Path, args: &[OsString]) -> io::Result<()> {
        // `quiet_command` is the one door for a child; `open` returns once
        // LaunchServices has the launch, and is not waited on.
        bt_platform::quiet_command(OPEN)
            .arg("-n")
            .arg("-a")
            .arg(bundle)
            .arg("--args")
            .args(args)
            .spawn()
            .map(drop)
    }
}

#[cfg(test)]
#[path = "update_apply_macos_tests.rs"]
mod tests;
