//! **`folio --update-apply <home> <txn> <nonce>` on Windows: the applier, and
//! the recovery of every phase a dead applier leaves** (0.4.6 ticket U-23;
//! `docs/plans/design/self-update-2026-09-16.md` §C.4–§C.7, revision (b)
//! §(b).2's W3–W8 and W12, "Who may write what", E-7).
//!
//! O, on its way out, wrote `Handoff{applier: <nonce>}` durably and started
//! the rescue copy `H\<txn>\rescue\folio.exe` with this line (`update_handoff`,
//! U-21). This process — P, a copy of O's own executable that no step moves —
//! takes the transaction from there to `Committed`, or stops at a named phase
//! for the next lock holder:
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
//!    `RollbackIntent`, and P stops (the rollback is U-24's).
//! 6. **Trial** (W6 → W7): the exclusive admission let go (the trial takes its
//!    shared hold at its start); `<install>\folio.exe --update-trial <txn>
//!    <nonce>` with a fresh nonce, detached, through `quiet_command`; its pid
//!    from the child and its start time from `GetProcessTimes` →
//!    `Trial{nonce, process, began_ms}`, durable. Not started →
//!    `RollbackIntent`.
//! 7. **Commit** (W7 → W8, `update_apply::await_receipt`): a receipt of this
//!    transaction and this trial's nonce while the journal says `Trial` →
//!    `Committed{outcome: committed}`, durable; any other receipt is said once
//!    and waited past. The trial gone without one, or 90 s → `RollbackIntent`.
//! 8. **Retire** (W8, W12; the coordinator's order): after `Committed` is
//!    durable, the `Run` value removed and flushed; then exactly the recorded
//!    old files found in `backup\` by their digests deleted — never a file
//!    the journal does not record; then `Retired{Committed}` — the class
//!    `terminal`. A step that fails is debt ([`Ended::CommittedWithDebt`]),
//!    finished by the next lock holder, never a rollback. **The rescue folder
//!    cannot be deleted by the process that runs from it**: the next ordinary
//!    start retires the terminal journal and deletes `H\<txn>` with it
//!    (`update_startup`, U-12's `Retire`).
//!
//! **Recovery** ([`recover`], the rescue build as `--update-recover`, R): the
//! same lock, then the phase a dead applier left, by the same code: `Handoff`
//! or `Armed` — nothing moved — → the `Run` value removed, `Reverted` to
//! `Prepared` (the coordinator's ruling of 2026-09-27: the old build opens);
//! `Moving` → `RollbackIntent` (W6; the rollback is U-24's); `Trial` → the
//! receipt awaited while the recorded trial process lives and its deadline has
//! not passed, then `Committed` and its retirement, or `RollbackIntent` (W7,
//! W8); `Committed` → the retirement finished (W12). A `Handoff` whose applier
//! may still be alive — a process of the rescue image started before this one,
//! which is how P looks while it waits for O — is not recovery's: the lock is
//! let go and asked for again, so the applier takes it.
//!
//! Every phase is recorded through `update_apply::Journaled` (the protocol's
//! refusal, the writer table, then `install_txn::durable_write`); every effect
//! is asked of `update_txn::may` first. Headless: the main thread is a worker
//! (`admission::enter_standalone_main`), and its only sleeps are the wait
//! door's (`bt_platform::wait::sleep_within`) and `install_txn::hold_within`'s.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bt_platform::HostPlatform;
use bt_platform::admission::WorkerCtx;
use bt_platform::file_reads::{self, Lane};
use bt_platform::install_flip::{self, Running};
use bt_platform::install_txn::{self, Armed, Held, Hold};
use bt_platform::trust::Policy;

use crate::cli;
use crate::install_channel::Channel;
use crate::update_apply::{Ended, Journaled, Limits, Verdict, await_receipt, now_ms};
use crate::update_prepare_windows::{Resume, staged_as_verified};
use crate::update_txn::{
    Action, Actor, Asker, Digest, Disk, Effect, Event, Home, Inventories, Journal, Layout, Located,
    Move, Nonce, Phase, Place, Seen, TrialProcess, TxnId, decide,
};

/// **How long recovery waits for another lock holder** — an applier that is
/// still taking the transaction to its end: §C.4's 60 s for the old build and
/// §C.5's 90 s for the trial, and room for the moves and the retirement
/// between them. The owner's ruling of 2026-09-25 (3): a start during an apply
/// waits for it, up to the apply's own deadline.
pub(crate) const RECOVER_WITHIN: Duration = Duration::from_secs(180);

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
    /// One move of the flip is done — the instant between two moves.
    fn moved(&mut self, _done: &Move) {}
}

/// **One Windows road**: the home, the installed program it replaces, the
/// rescue copy this process runs from, and what the checks and waits take.
pub(crate) struct Road {
    pub(crate) home: Home,
    /// `<install>\folio.exe`.
    pub(crate) installed: PathBuf,
    /// This process's own image, `H\<txn>\rescue\folio.exe`.
    pub(crate) rescue: PathBuf,
    /// The data directory whose claim O held (the storage rule O ran under:
    /// the applier inherits O's environment).
    pub(crate) data: PathBuf,
    /// Which roots a signature may end in: the system's in the product.
    pub(crate) policy: Policy,
    /// How the installed copy was installed, derived now.
    pub(crate) channel: Option<Channel>,
    pub(crate) limits: Limits,
    /// [`RECOVER_WITHIN`] in the product.
    pub(crate) recover_within: Duration,
    /// This process, by its pid and start time: an applier still alive is a
    /// process of the rescue image started before it.
    pub(crate) me: Running,
}

impl Road {
    /// The road of the rescue build at `rescue` over `home`, as the product
    /// runs it.
    pub(crate) fn of_this_copy(home: Home, installed: PathBuf, rescue: PathBuf) -> Self {
        let channel = Some(crate::install_channel::channel_of(&installed));
        let pid = std::process::id();
        Self {
            home,
            installed,
            rescue,
            data: crate::persist::storage_dir_unmoved(),
            policy: Policy::System,
            channel,
            limits: Limits::PRODUCT,
            recover_within: RECOVER_WITHIN,
            me: Running {
                pid,
                // Not known: nothing counts as started before this process.
                started: install_flip::started_of(pid).unwrap_or(0),
            },
        }
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
    let (Ok(txn), Ok(nonce)) = (TxnId::parse(txn), Nonce::parse(nonce)) else {
        World::say(
            &mut world,
            &format!(
                "BT_UPDATE_APPLY malformed transaction or nonce; {}",
                cli::UPDATE_APPLY_USAGE
            ),
        );
        return 2;
    };
    match bt_platform::admission::enter_standalone_main("folio-update-apply", |worker| {
        apply(worker, &road, txn, nonce, &mut world)
    }) {
        Ok(ended) => {
            World::say(
                &mut world,
                &format!("BT_UPDATE_APPLY transaction {txn}: {ended:?}"),
            );
            ended.code()
        }
        Err(refused) => {
            World::say(&mut world, &format!("BT_UPDATE_APPLY {refused:?}"));
            2
        }
    }
}

/// **The applier, over any road** — see the module header. After a revert
/// (steps 1 and 4) the old build is started again with no argument, once the
/// lock is let go; after every other end nothing is started: `Committed` —
/// the trial runs; `Abandoned` — O stayed, or the transaction is the next
/// start's to retire; `RollbackIntent` — the rollback's (U-24).
pub(crate) fn apply(
    worker: &WorkerCtx,
    road: &Road,
    txn: TxnId,
    nonce: Nonce,
    world: &mut impl World,
) -> Ended {
    let window = Instant::now() + road.limits.old_within;
    let lock = match install_txn::hold_within(
        &road.home.lock(),
        Hold::Exclusive,
        road.limits.old_within,
    ) {
        Ok(Some(held)) => held,
        Ok(None) => return Ended::OldHeldTheLock,
        Err(failure) => return Ended::Failed(failure.to_string()),
    };
    let journal = match read_journal(&road.home) {
        Ok(Some(journal)) => journal,
        Ok(None) => return Ended::Refused("there is no journal".to_owned()),
        Err(why) => return Ended::Refused(why),
    };
    if journal.txn != txn {
        return Ended::Refused(format!(
            "the journal is transaction {}, not {txn}",
            journal.txn
        ));
    }
    let ended = match Txn::of(road, journal, lock) {
        Ok(mut held) => {
            let ended = held.apply(worker, nonce, window, world);
            World::say(
                world,
                &format!(
                    "BT_UPDATE_APPLY transaction {txn} wrote {:?}",
                    held.j.written
                ),
            );
            ended
        }
        Err(ended) => ended,
    };
    if ended == Ended::Reverted {
        relaunch_the_old_build(road, world);
    }
    ended
}

/// A revert's relaunch: the installed build, unchanged, with no argument.
fn relaunch_the_old_build(road: &Road, world: &mut impl World) {
    let line = match world.spawn_detached(&road.installed, &[]) {
        Ok(()) => format!(
            "BT_UPDATE_APPLY nothing was moved; {} started again",
            road.installed.display()
        ),
        Err(error) => format!(
            "BT_UPDATE_APPLY nothing was moved; {} could not be started again: {error}",
            road.installed.display()
        ),
    };
    world.say(&line);
}

/// **Recovery over any road** (R) — see the module header: the transaction
/// lock within [`Road::recover_within`], then the phase a dead applier left.
pub(crate) fn recover(worker: &WorkerCtx, road: &Road, world: &mut impl World) -> Ended {
    let until = Instant::now() + road.recover_within;
    let mut said_waiting = false;
    let (lock, journal) = loop {
        let left = until.saturating_duration_since(Instant::now());
        let lock = match install_txn::hold_within(&road.home.lock(), Hold::Exclusive, left) {
            Ok(Some(held)) => held,
            Ok(None) => return Ended::LockHeld,
            Err(failure) => return Ended::Failed(failure.to_string()),
        };
        let journal = match read_journal(&road.home) {
            Ok(Some(journal)) => journal,
            Ok(None) => return Ended::Left("there is no transaction".to_owned()),
            Err(why) => return Ended::Left(why),
        };
        if matches!(journal.body.phase, Phase::Handoff { .. }) && applier_alive(road) {
            drop(lock);
            if !said_waiting {
                said_waiting = true;
                world.say("BT_UPDATE_RECOVER the applier is still alive; waiting for it");
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ended::LockHeld;
            }
            bt_platform::wait::sleep_within(worker, road.limits.poll.min(left));
            continue;
        }
        break (lock, journal);
    };
    let txn = journal.txn;
    match Txn::of(road, journal, lock) {
        Ok(mut held) => {
            let ended = held
                .recover_from(worker, world)
                .unwrap_or_else(Ended::Failed);
            world.say(&format!(
                "BT_UPDATE_RECOVER transaction {txn} wrote {:?}",
                held.j.written
            ));
            ended
        }
        Err(ended) => ended,
    }
}

/// **Whether an applier may still be alive**: a process of the rescue image
/// this process runs from, other than this one, started before it. O starts P
/// before it lets go of the transaction lock, so a P that has not taken the
/// lock yet is listed here whenever this process holds it. A list that cannot
/// be read is taken as no applier.
fn applier_alive(road: &Road) -> bool {
    install_flip::running_from(&road.rescue).is_ok_and(|running| {
        running
            .iter()
            .any(|process| process.pid != road.me.pid && process.started < road.me.started)
    })
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
    j: Journaled,
    /// Held for the whole road; let go when this is dropped.
    _lock: Held,
    inventories: Inventories,
}

impl<'a> Txn<'a> {
    fn of(road: &'a Road, journal: Journal, lock: Held) -> Result<Self, Ended> {
        let Layout::Members(inventories) = journal.body.layout.clone() else {
            return Err(Ended::Refused(
                "the journal is not a member set's".to_owned(),
            ));
        };
        Ok(Self {
            road,
            j: Journaled::of(&road.home, journal),
            _lock: lock,
            inventories,
        })
    }

    fn txn(&self) -> TxnId {
        self.j.journal.txn
    }

    /// `set\`, `backup\` or the install folder.
    fn folder(&self, place: Place) -> Result<PathBuf, String> {
        match place {
            Place::Install => self
                .road
                .installed
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| "the installed program has no folder".to_owned()),
            other => self
                .road
                .home
                .members_folder(self.txn(), other)
                .ok_or_else(|| "the home is not a member set's".to_owned()),
        }
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
            Phase::Moving | Phase::Trial { .. } | Phase::Committed => {
                self.recover_from(worker, world)
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
        for step in self.inventories.forward_moves() {
            let effect = match step.to {
                Place::Backup => Effect::MoveOldOut,
                _ => Effect::MoveNewIn,
            };
            self.j.may(Actor::Applier, effect)?;
            let from = self.folder(step.from)?.join(&step.name);
            let to = self.folder(step.to)?.join(&step.name);
            if let Err(failure) = install_txn::durable_move(&from, &to) {
                world.say(&format!(
                    "BT_UPDATE_APPLY the move of `{}` failed: {failure}",
                    step.name
                ));
                self.j.record(Actor::Applier, &Event::RollbackDeclared)?;
                return Ok(Ended::RollbackIntent);
            }
            world.moved(&step);
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
        loop {
            let mut held = Vec::new();
            for member in &self.inventories.old_present {
                match install_flip::held_open(&install.join(&member.name)) {
                    Ok(false) => {}
                    Ok(true) => held.push(member.name.as_str()),
                    Err(error) => {
                        return Err(format!("`{}` in the install: {error}", member.name));
                    }
                }
            }
            if held.is_empty() {
                return Ok(());
            }
            let left = window.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!("held open by another process: {}", held.join(", ")));
            }
            bt_platform::wait::sleep_within(worker, self.road.limits.poll.min(left));
        }
    }

    /// W6 → W7 → W8, or `RollbackIntent`.
    fn trial(&mut self, worker: &WorkerCtx, world: &mut impl World) -> Result<Ended, String> {
        let nonce = crate::update_job::mint_nonce();
        let began_ms = now_ms();
        let args = [
            OsString::from(cli::UPDATE_TRIAL_FLAG),
            OsString::from(self.txn().to_string()),
            OsString::from(nonce.to_string()),
        ];
        let pid = match world.launch_trial(&self.road.installed, &args) {
            Ok(pid) => pid,
            Err(error) => {
                world.say(&format!(
                    "BT_UPDATE_APPLY {} did not start: {error}",
                    self.road.installed.display()
                ));
                self.j.record(Actor::Applier, &Event::RollbackDeclared)?;
                return Ok(Ended::RollbackIntent);
            }
        };
        // A trial that has already gone has no start time: it is then taken
        // as not running, and only its receipt can commit.
        let started = install_flip::started_of(pid).unwrap_or(0);
        self.j.record(
            Actor::Applier,
            &Event::TrialBegan {
                nonce,
                process: TrialProcess { pid, started },
                began_ms,
            },
        )?;
        self.await_and_retire(worker, Actor::Applier, world)
    }

    /// W7 → W8, for P and for R alike.
    fn await_and_retire(
        &mut self,
        worker: &WorkerCtx,
        actor: Actor,
        world: &mut impl World,
    ) -> Result<Ended, String> {
        let verdict = await_receipt(
            worker,
            &mut self.j,
            &self.road.home,
            actor,
            &self.road.limits,
            &mut |process| {
                install_flip::still_running(Running {
                    pid: process.pid,
                    started: process.started,
                })
            },
            &mut |line| world.say(line),
        )?;
        Ok(match verdict {
            Verdict::Committed => self.retire_committed(actor, world),
            Verdict::RollbackIntent => Ended::RollbackIntent,
        })
    }

    /// **R's step, from the phase a dead applier left** (and P's, found again
    /// at a phase past its own).
    fn recover_from(
        &mut self,
        worker: &WorkerCtx,
        world: &mut impl World,
    ) -> Result<Ended, String> {
        let phase = self.j.journal.body.phase.clone();
        let actor = Asker::LockHolder.actor(phase.kind());
        match phase {
            // Nothing was moved: moves wait for `Moving`, which follows
            // `Armed` (W4, W5; the coordinator's ruling: the old build opens).
            Phase::Handoff { .. } | Phase::Armed => self.revert(actor, world),
            // W6: the trial never began, so there is no receipt and no roll
            // forward.
            Phase::Moving => {
                self.j.record(actor, &Event::RollbackDeclared)?;
                Ok(Ended::RollbackIntent)
            }
            Phase::Trial { .. } => self.await_and_retire(worker, actor, world),
            Phase::Committed => Ok(self.retire_committed(actor, world)),
            Phase::Abandoned | Phase::Retired { .. } => {
                self.j.may(actor, Effect::RemoveEntrance)?;
                world.disarm(self.txn())?;
                Ok(Ended::Left(
                    "the transaction is over; the next ordinary start deletes its folder and journal"
                        .to_owned(),
                ))
            }
            Phase::Allocated | Phase::Prepared { .. } => Ok(Ended::Left(
                "the transaction is the running build's to resume or discard".to_owned(),
            )),
            Phase::RollbackIntent { .. } | Phase::Stuck { .. } | Phase::RolledBack => {
                Ok(Ended::Left(
                    "the rollback is not in this build yet; the transaction is left as it is"
                        .to_owned(),
                ))
            }
        }
    }

    /// W4, W5: the entrance removed, then `Reverted` to `Prepared`.
    fn revert(&mut self, actor: Actor, world: &mut impl World) -> Result<Ended, String> {
        self.j.may(actor, Effect::RemoveEntrance)?;
        world.disarm(self.txn())?;
        self.j.record(actor, &Event::Reverted)?;
        Ok(Ended::Reverted)
    }

    /// **What is at each member's name, by digest**: in the install folder
    /// and in `backup\`.
    fn locate(&self) -> Result<Vec<Seen>, String> {
        let install = self.folder(Place::Install)?;
        let backup = self.folder(Place::Backup)?;
        let mut names: Vec<&str> = Vec::new();
        for member in self
            .inventories
            .old_present
            .iter()
            .chain(&self.inventories.new)
        {
            if !names.contains(&member.name.as_str()) {
                names.push(&member.name);
            }
        }
        Ok(names
            .into_iter()
            .map(|name| Seen {
                name: name.to_owned(),
                install: digest_at(&install.join(name)),
                backup: digest_at(&backup.join(name)),
            })
            .collect())
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
        let seen = self.locate()?;
        let entrance = world.is_armed(self.txn()).unwrap_or(true);
        let disk = Disk {
            journal: &self.j.journal,
            asker: Asker::LockHolder,
            entrance,
            located: Located::Members(seen),
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
/// processes, and lines on standard error and, when `log` names one, in a
/// file.
pub(crate) struct Machine {
    pub(crate) log: Option<PathBuf>,
}

impl World for Machine {
    fn say(&mut self, line: &str) {
        bt_platform::write_std_error(format!("{line}\n").as_bytes());
        if let Some(log) = &self.log {
            let _ = crate::diagnostics::append_note(log, line);
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
mod tests;
