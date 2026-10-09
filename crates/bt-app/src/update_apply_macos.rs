//! **`folio --update-apply <home> <txn> <nonce>` on macOS: the applier**
//! (0.4.6 tickets U-28 and U-29; `docs/plans/design/self-update-2026-09-16.md`
//! §C.4, §C.5, revision (b) §(b).2's M3–M11 and "Who may write what").
//!
//! O, on its way out, wrote `Handoff{applier: <nonce>}` durably and started
//! the rescue clone with this line (`update_handoff`, U-21). This process — P,
//! the rescue clone, a copy of O's own build — takes the transaction from
//! there to `Committed`, or back to the old bundle, or stops at a named phase
//! for the next lock holder:
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
//!    bundle's executable (`bt_platform::install_flip::running_from`) — one
//!    still there is waited for within the same window, as the Windows road
//!    waits for a held file (`update_apply::until_let_go`, U-40: a person's
//!    start that hands itself to the recovery build is gone within moments) →
//!    `Admitted`: the journal says `Moving` (the note's `Exchanging`), with
//!    both identities already in its layout since `Prepared`. Refused → the
//!    plist removed, `Reverted` to `Prepared`.
//! 4. **Exchange** (M5 → M6): the live bundle must still be the journal's
//!    old identity and the staged one its new identity (cdhash and version,
//!    read with `codesign` and `plutil`); then one
//!    `renamex_np(RENAME_SWAP)` (`bt_platform::install_flip::exchange`). The
//!    exclusive admission is let go right after, so the trial can take its
//!    shared hold. Either identity changed → nothing swapped, `Reverted`.
//! 5. **Trial** (M6 → M7): `open -n -W -a <bundle> --args --update-trial <txn>
//!    <nonce>` with a fresh nonce, held ([`Launch`]); then, polling through the
//!    wait door until [`Limits::trial_within_ms`] after the launch: the trial's
//!    pid — LaunchServices reports none, so it is the receipt's own, or the
//!    process whose image is the installed executable and whose arguments
//!    carry these very words (`update_apply::launched_trial`, 0.4.7 ticket
//!    U-40: never a process only because it started after the launch, which a
//!    person's start of the new build in that instant also does) — and its
//!    receipt `H/<txn>/health-<nonce>`. Found →
//!    `Trial{nonce, process, began_ms}`, durable. **The launch over and
//!    nothing seen** — `open -W` returns when
//!    the application it opened ends — is a trial that ended before it could
//!    be seen: no receipt, at once (0.4.7 ticket U-38; it used to cost the
//!    whole deadline with no window).
//! 6. **Commit** (M7 → M8): a receipt the journal accepts — this transaction's,
//!    this trial's nonce, while the journal says `Trial` — →
//!    `Committed{outcome: committed}`, durable; **then** the old bundle in
//!    `stage/` (checked to be the old identity) is removed, the plist removed,
//!    `Retired{Committed}` recorded, and `H/<txn>` removed with the rescue
//!    clone this process runs from (a Unix process may remove its own image).
//!    The journal itself is kept at `Retired{Committed}`: the trial's watch
//!    releases its held-back writes when it reads `committed` there, and would
//!    drop them if it found no journal; the next ordinary start retires it.
//! 7. **No receipt by the deadline, the trial died without one, or the
//!    exchange left the new bundle live without a trial** → `RollbackIntent`
//!    (`outcome: rolled_back`), durable, and the rollback (U-29, M9), decided
//!    at each step by `update_txn::decide` from what is on disk:
//!    - **the trial is stopped** when it still runs ([`stop_trial`]): asked to
//!      quit (`SIGTERM`, the road any application's quit takes on macOS; the
//!      trial writes nothing it held back, `update_trial`), given
//!      [`Limits::quit_within`], then ended (`SIGKILL`) and given
//!      [`Limits::end_within`] — each signal sent only after the process list
//!      shows the journal's pid, with its start instant, running from the new
//!      bundle's executable (at the launch path, or at `stage/` if a swap back
//!      already happened: `bt_platform::install_flip::ask`);
//!    - **the swap back**, only while the live identity is the new one and
//!      `stage/` holds the old one: exclusive admission within
//!      [`Limits::old_within`] (not had → nothing is recorded and the
//!      rollback waits for the next start: `RollbackWaits`), then the same
//!      one `RENAME_SWAP`. The live identity already old → no swap;
//!    - **the restored bundle verified** — the old identity at the launch
//!      path, and its signature against this process's own designated
//!      requirement (the rescue clone is a copy of the old bundle, U-16) —
//!      → `RolledBack`, durable;
//!    - then the plist removed, `Retired{RolledBack}` recorded, and `H/<txn>`
//!      removed (the staged new bundle and the rescue clone with it). The
//!      journal is kept, as after a commit: the relaunched old build reads its
//!      header for the card, and retires it.
//!
//!    Any step that fails → `RollbackFailed`: `Stuck{last_error, attempts}`,
//!    durable, and nothing removed — the journal, both bundles and the
//!    entrance stay (M10). A `Stuck` transaction is tried again by the next
//!    lock holder (the LaunchAgent at login, or a start through the rescue
//!    build), until [`update_txn::STUCK_ATTEMPT_LIMIT`] rollbacks have failed.
//!
//! **Relaunch** (the coordinator's ruling 1, U-29; rulings 2 and 3, U-29b).
//! The lock let go, the applier starts the installed bundle again through
//! LaunchServices: after a rollback, finished or not, with `--update-failed
//! <journal>` — the card rises at `Failed`, *Previous version restored.* or
//! *Update incomplete.* and the journal's folder; after a revert (`Reverted`
//! to `Prepared`: the old bundle is unchanged, and the transaction waits for
//! the deferred rule) with no word at all. After `Abandoned` nothing is
//! started: O never left, or the reader quit on purpose, and the next ordinary
//! start retires it. **After a failure** — a journal write that failed, a
//! lock that could not be opened — **what a person's start would owe**
//! (U-34): the live bundle with `--update-failed`, the journal and the
//! LaunchAgent kept for the next start or login. A trial started over
//! `Moving` whose start cannot be recorded is ended at once and the road
//! goes on as for a trial that did not start. **The new bundle, live and not committed, is never
//! started plainly**: a road that ends `Stuck` with it live starts it as a
//! trial over `Stuck` before the lock is let go — recorded (`RetrialBegan`),
//! so its receipt commits forward — and waits for it ([`Txn::retry_as_trial`]);
//! where no such trial can be recorded, the start is a trial with a nonce no
//! journal records ([`Opens::Trial`]).
//!
//! **Re-entry** (P started again over its own transaction): `Armed` continues
//! from step 3. `Moving` is decided by the live identity, as recovery decides
//! it (M5, M6): still the old bundle → the plist removed, `Reverted`; the new
//! one with the old in `stage/` → the trial nobody started (U-29b), else
//! `RollbackIntent` and the rollback. Anything else is refused and nothing is
//! touched.
//!
//! **Recovery** ([`recover`], `--update-recover <home>`, U-29 and U-29b): the
//! rescue build started by the LaunchAgent at login, or by an ordinary start
//! that found a destructive journal, finishes **every** phase a dead applier
//! can leave over the same code, each step `decide`'s answer for
//! `Asker::Rescue` ([`Txn::settle`]): `Handoff` and `Armed` back to
//! `Prepared`; `Moving` decided by the live identity (a trial when the new
//! bundle is live); `Trial` waited for, committed on its receipt or rolled
//! back; `RollbackIntent`, `Stuck` and `RolledBack` rolled back and retired as
//! above; `Committed` retired as after a commit (M11, "Committed-with-debt").
//! **A running trial the journal does not record** — over `Moving` with the
//! new bundle live, or a `Stuck` whose recorded trial is gone — is recorded
//! when its receipt is here (`Txn::before_deciding`, U-37, revision (h): only a receipt naming it by pid and start instant), and that receipt commits;
//! a second trial is never started beside it (the rehearsal's defect 9).
//! What opens after it is [`Recovered::opens`]: exactly one start.
//!
//! Every phase is recorded through `update_txn::Journal::advance` (which
//! refuses what the protocol does not allow) and written with
//! `update_apply::write_journal` (`install_txn::durable_write`; a macOS
//! rename is never refused for an open target, so it is never asked again
//! here) only after `update_txn::may_record` says this
//! actor may; every effect on a file, the entrance, the bundles or the trial
//! is asked of `update_txn::may` first. Headless: the main thread is a worker
//! (`admission::enter_standalone_main`), and its only sleeps are the wait
//! door's.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bt_platform::admission::WorkerCtx;
use bt_platform::file_reads::{self, Lane};
use bt_platform::install_flip::{self, Running};
use bt_platform::install_txn::{self, Held, Hold};
use bt_platform::{HostPlatform, launch_agent};

use crate::cli;
use crate::update_adapter::Layouts;
use crate::update_apply::{
    Ahead, BeforeDeciding, Deferral, ExitGuard, Leave, Recording, Watch, Watched, Window,
    stop_trial, trial_runs,
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
pub(crate) use crate::update_apply::{Opener, Opens, failed_words, trial_words};
use crate::update_txn::{
    Action, Actor, Asker, BundleIdentity, Class, Disk, Effect, Event, HeaderOutcome, Home, Journal,
    Layout, Located, Nonce, Phase, PhaseKind, Receipt, Restore, Role, Sight, TrialProcess, TxnId,
    decide,
};

/// `open`, by its absolute path: LaunchServices starts the trial as it starts
/// any application (§C.5), and the old build again after a rollback.
pub(crate) const OPEN: &str = "/usr/bin/open";

/// **How long the applier waits, and how often it looks.**
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    /// For O's lock, O's claim and the exclusive admission together (§C.4's
    /// 60 s); and, for a rollback, for the transaction lock and for the
    /// exclusive admission before the swap back.
    pub(crate) old_within: Duration,
    /// For the trial's pid and receipt, counted from the launch (§C.5's
    /// 90 s, `update_txn::TRIAL_DEADLINE_MS`).
    pub(crate) trial_within_ms: u64,
    /// Between two looks at the claim, at the trial, and at a trial asked to
    /// stop.
    pub(crate) poll: Duration,
    /// **How long a trial asked to quit has before it is ended** (W9's "after
    /// 5 s grace"; the coordinator's ruling 2, U-29).
    pub(crate) quit_within: Duration,
    /// How long an ended trial has to leave the process list before the
    /// rollback records that it would not end.
    pub(crate) end_within: Duration,
}

impl Limits {
    /// The product's.
    pub(crate) const PRODUCT: Self = Self {
        old_within: Duration::from_secs(60),
        trial_within_ms: crate::update_txn::TRIAL_DEADLINE_MS,
        poll: Duration::from_millis(250),
        quit_within: Duration::from_secs(5),
        end_within: Duration::from_secs(5),
    };
}

/// **A trial's launch, held while its trial is unseen** (0.4.7 ticket U-38):
/// LaunchServices reports no pid, so the trial is found in the process list,
/// and a trial that ends before the list shows it once left the applier
/// waiting out its whole deadline with no window (the rehearsal's defect 2:
/// 89.8 s). The launch is `open -n -W`, which returns when the application it
/// opened ends — or at once, when it could not be opened — so `open`'s own end
/// is the trial's, seen without its pid ([`Launch::over`]) — **only when
/// `open` exited** (design revision (h) H.5, rule L1): an `open` ended by a
/// signal says nothing about the application, which LaunchServices may still
/// be starting, so the launch is dropped from the watch and the trial is
/// waited for to its deadline as before. Ending `open` leaves the application
/// running (measured on macOS 26), so the launch is ended when it is let go —
/// by the applier's normal return and by an unwinding panic. **An applier
/// ended from outside** (a signal, which runs no `Drop`) leaves its `open -W`
/// waiting until the application it opened ends (rule L2): it starts nothing
/// and holds no lock, claim or admission.
pub(crate) struct Launch(Option<std::process::Child>);

impl Launch {
    /// The launch `child` (`open -W`, or a test's stand-in for it).
    pub(crate) fn of(child: std::process::Child) -> Self {
        Self(Some(child))
    }

    /// A launch nothing can be asked about: never over.
    pub(crate) fn untracked() -> Self {
        Self(None)
    }

    /// **Whether the launch has ended** — `open` exited: the application it
    /// opened has ended, or never started. A launch whose state cannot be read
    /// is not; an `open` ended by a signal is unknown (H.5, L1): the launch is
    /// dropped from the watch, and is never over from then on.
    pub(crate) fn over(&mut self) -> bool {
        let Some(child) = self.0.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(Some(status)) if status.code().is_some() => true,
            Ok(Some(_)) => {
                // Ended by a signal, and reaped by that look: nothing is left
                // to end, and nothing is known about the application.
                self.0 = None;
                false
            }
            _ => false,
        }
    }
}

impl Drop for Launch {
    /// `open -W` ended, by its own handle — never the application it opened
    /// — and reaped when it has already gone.
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.try_wait();
        }
    }
}

/// **One lock holder's road**: the home the line named, the transaction, the
/// applier's nonce O handed over (none for recovery, which applies nothing),
/// and where the data directory and the LaunchAgents folder are.
pub(crate) struct Road {
    pub(crate) home: Home,
    pub(crate) txn: TxnId,
    /// The nonce `Handoff` must name for this process to apply it; `None` for
    /// the rescue build as recovery ([`recover`]).
    pub(crate) nonce: Option<Nonce>,
    /// The data directory whose claim O held (the same storage rule O ran
    /// under: the applier inherits O's environment).
    pub(crate) data: PathBuf,
    /// `~/Library/LaunchAgents`, where the entrance goes; `None` when there is
    /// no home folder to find it from.
    pub(crate) agents: Option<PathBuf>,
    pub(crate) limits: Limits,
    /// **This process's starter** (U-37, H.3): its parent, by pid and an
    /// earlier start instant — an ordinary start that handed itself over is no
    /// candidate.
    pub(crate) starter: Option<Running>,
    /// **The trial that handed its transaction back** (`--from-trial`, H.4).
    pub(crate) handed_back: Option<crate::update_apply::HandedBack>,
    /// **The layouts this road calls**, by the adapter the journal names
    /// (managed-update R2): [`own_layouts`] in the product.
    pub(crate) layouts: Layouts<dyn ApplyPoints>,
}

/// **A layout's `Activate` and `Prove / Recover` on the macOS road** (0.4.7
/// ticket U-41a1; managed-update §1.1 R1): the road calls these, through the
/// adapter its journal names (`update_adapter::Layouts`), and nothing else
/// about it depends on the layout — the locks, the admission, the process
/// check, the identity checks before the exchange, the phases and their
/// rights, the trial, the commit and the rollback's decision
/// (`update_txn::decide`) are common.
///
/// * [`ApplyPoints::activate`] — after `Moving` is durable, before
///   `TrialBegan`: the staged bundle made the live one;
/// * [`ApplyPoints::activate_back`] — after `RollbackIntent` is durable,
///   under the admission: the old bundle made the live one again;
/// * [`ApplyPoints::locate`] — which bundle is live and which is staged, read
///   from the disk: every such answer after `Activate` begins, including what
///   `decide`, the trial watcher and the exit guard are handed.
pub(crate) trait ApplyPoints: Send + Sync {
    /// **Activate forward**, through `hands`' exchange.
    ///
    /// # Errors
    /// The exchange's refusal; the road decides what follows by what is live.
    fn activate(&self, places: &Places<'_>, hands: &mut dyn Hands) -> Result<(), String>;

    /// **Activate back**, through `hands`' exchange.
    ///
    /// # Errors
    /// The exchange's refusal; the road records `Stuck`.
    fn activate_back(&self, places: &Places<'_>, hands: &mut dyn Hands) -> Result<(), String>;

    /// **The identities of the live bundle and of the staged one**, each
    /// `None` where none can be read: `update_txn::Located::Bundle`'s two
    /// sides.
    fn locate(
        &self,
        worker: &WorkerCtx,
        places: &Places<'_>,
    ) -> (Option<BundleIdentity>, Option<BundleIdentity>);
}

/// **Folio's own layout** (`Layout::Bundle`, U-28/U-29): one
/// `renamex_np(RENAME_SWAP)` of the installed bundle with the staged one,
/// forward and back, and each side read by its identity.
pub(crate) struct Ours;

impl ApplyPoints for Ours {
    fn activate(&self, places: &Places<'_>, hands: &mut dyn Hands) -> Result<(), String> {
        hands.exchange(places.installed, places.stage)
    }

    fn activate_back(&self, places: &Places<'_>, hands: &mut dyn Hands) -> Result<(), String> {
        hands.exchange(places.installed, places.stage)
    }

    fn locate(
        &self,
        worker: &WorkerCtx,
        places: &Places<'_>,
    ) -> (Option<BundleIdentity>, Option<BundleIdentity>) {
        let live = crate::update_prepare_macos::identity(worker, places.installed).ok();
        let stage = crate::update_prepare_macos::identity(worker, places.stage).ok();
        (live, stage)
    }
}

/// **The layouts of this road** as the product has them: [`Ours`] alone.
#[must_use]
pub(crate) fn own_layouts() -> Layouts<dyn ApplyPoints> {
    Layouts::of(Arc::new(Ours))
}

/// **The effects of a lock holder that a test stands in for**: its lines, the
/// exchange, and the check of a restored bundle's signature. Everything else —
/// the journal, the locks, the claim, the entrance, the identities, the
/// process list, the signals, the receipt and the deletions — is the real
/// door in the product and in the tests.
pub(crate) trait Hands {
    /// One line of what happened.
    fn say(&mut self, line: &str);
    /// The one exchange of the installed bundle with the staged one, forward
    /// or back.
    ///
    /// # Errors
    /// The exchange's refusal, or a flush after it that failed.
    fn exchange(&mut self, live: &Path, staged: &Path) -> Result<(), String>;
    /// **Whether the bundle a rollback put back is this publisher's old
    /// build**: its signature valid, strictly, and this process's own
    /// designated requirement satisfied — the rescue clone is a copy of the
    /// old bundle (U-16's check).
    ///
    /// # Errors
    /// Why not, as a sentence.
    fn verify_restored(&mut self, worker: &WorkerCtx, bundle: &Path) -> Result<(), String>;
    /// Start the trial: `open -n -W -a <bundle> --args <args>`, detached — the
    /// applier's after its exchange, and a recovery's over an exchange or a
    /// `Stuck` whose new bundle is live (U-29b) — and hold the launch, which
    /// ends when the application it opened ends ([`Launch`], U-38).
    ///
    /// # Errors
    /// `open` could not be started.
    fn launch_trial(&mut self, bundle: &Path, args: &[OsString]) -> io::Result<Launch>;
    /// **Whether a start just made was acknowledged** (U-34, round 2): a Folio
    /// holds the data directory `data` within
    /// `update_apply::ACKNOWLEDGED_WITHIN`, asked through `worker`'s wait door
    /// (`update_apply::claimed_within`).
    fn acknowledged(&mut self, worker: Option<&WorkerCtx>, data: &Path) -> bool;
    /// **The failure window, in this process** (U-34, round 2): `text` in an
    /// alert.
    fn show_here(&mut self, text: &str);
}

/// **The applier's effects that a test stands in for**: a lock holder's, and
/// the start of the installed build after its road.
pub(crate) trait World: Hands {
    /// Start the installed build again after a rollback or a revert: `open -n
    /// -a <bundle>` with `args` after `--args` when there are any, detached.
    ///
    /// # Errors
    /// `open` could not be started.
    fn relaunch(&mut self, bundle: &Path, args: &[OsString]) -> io::Result<()>;
}

/// **Where a lock holder stopped.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ended {
    /// `Committed`, then the old bundle, the entrance and `H/<txn>` removed.
    Committed,
    /// `Committed` is durable; a deletion after it failed and is the next
    /// actor's debt (M11), never a rollback.
    CommittedWithDebt(String),
    /// `RolledBack`, then the entrance, `Retired{RolledBack}` and `H/<txn>`:
    /// the old bundle is live and verified.
    RolledBack,
    /// `RolledBack` is durable; the entrance, the retirement or `H/<txn>`
    /// failed and is the next start's to finish (M11).
    RolledBackWithDebt(String),
    /// A step of the rollback failed: `Stuck` is durable with this error, and
    /// everything is kept (M10).
    Stuck(String),
    /// `Stuck` at its bound ([`crate::update_txn::STUCK_ATTEMPT_LIMIT`]):
    /// nothing was tried; the last error.
    GaveUp(String),
    /// A running copy held the admission through the wait: the swap back
    /// was not tried, nothing was recorded, and the next lock holder tries.
    RollbackWaits(String),
    /// `Reverted` to `Prepared`: the entrance was found or removed, admission
    /// was refused, or the bundles were not the journal's.
    Reverted,
    /// `Abandoned` and cleared: O did not let go, or the entrance could not be
    /// armed.
    Abandoned,
    /// The transaction lock was still held at the end of the wait — by O,
    /// or by another lock holder: nothing was written.
    LockHeld,
    /// The line, the home or the journal is not this holder's to act on:
    /// nothing was touched.
    Refused(String),
    /// A write or a read failed; the journal holds its last durable phase.
    Failed(String),
    /// **Nothing was recorded, started or swapped** (U-37, H.3).
    Deferred(Deferral),
    /// **The transaction was already decided when this holder had it**
    /// (U-40): retired — its entrance removed if one was left — or back at
    /// `Prepared`, by the lock holder a recovery waited for. Nothing else is
    /// this holder's: the next ordinary start retires a retired transaction,
    /// and a prepared one is the running build's.
    Left(String),
    /// **The journal is one this build cannot read whole** (0.4.8 E1): this
    /// holder recorded, removed and ended nothing and let the lock go; the
    /// rescue build the journal names settles it.
    StoodAside(String),
}

impl Ended {
    /// **Whether this end deferred to a Folio proved to hold the data
    /// directory** (H.3): the exit guard then owes no start.
    pub(crate) fn deferred_to_a_holder(&self) -> bool {
        matches!(self, Ended::Deferred(Deferral::Held | Deferral::WindowDuty))
    }

    /// The process's exit code.
    pub(crate) fn code(&self) -> i32 {
        match self {
            Ended::Committed
            | Ended::CommittedWithDebt(_)
            | Ended::RolledBack
            | Ended::RolledBackWithDebt(_) => 0,
            Ended::Refused(_) => 2,
            _ => 1,
        }
    }

    /// **Whether this end leaves a rollback behind** — finished or not — so
    /// that the build started next carries `--update-failed` (U-29).
    pub(crate) fn rolled_back(&self) -> bool {
        matches!(
            self,
            Ended::RolledBack
                | Ended::RolledBackWithDebt(_)
                | Ended::Stuck(_)
                | Ended::GaveUp(_)
                | Ended::RollbackWaits(_)
        )
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
    /// at login only after a revert or a rollback it finished.
    pub(crate) waiting: bool,
    /// **Whom a person's start was deferred to** (0.4.8 E3), as
    /// `update_apply_windows::Recovered::deferred_to` says it.
    pub(crate) deferred_to: Option<Ahead>,
}

/// **How the macOS applier leaves** (`update_apply::ExitGuard`, U-34): the
/// installed bundle started again through LaunchServices with what the disk
/// names ([`opens_now`]). `worker` is `None` before the standalone main lent
/// one: which bundle is live cannot then be told, and a trial is safe for
/// either.
pub(crate) struct MacLeave<'a, W: World> {
    pub(crate) worker: Option<&'a WorkerCtx>,
    pub(crate) home: &'a Home,
    pub(crate) world: &'a mut W,
    /// The data directory a started Folio takes: the acknowledgement.
    pub(crate) data: &'a Path,
}

impl<W: World> Leave for MacLeave<'_, W> {
    fn say(&mut self, line: &str) {
        self.world.say(line);
    }

    fn opening(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
        let bundle = self.home.installed_bundle()?;
        let words = opens_now(self.worker, self.home).words(self.home);
        Some((bundle, words))
    }

    fn start(&mut self, program: &Path, words: &[OsString]) -> io::Result<()> {
        self.world.relaunch(program, words)
    }

    fn acknowledged(&mut self) -> bool {
        self.world.acknowledged(self.worker, self.data)
    }

    fn show_here(&mut self, why: &str) {
        self.world.say(&format!(
            "BT_UPDATE_EXIT no start was delivered ({why}); the failure window is shown here"
        ));
        self.world
            .show_here(&crate::update_apply::failure_text(Some(self.home)));
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
    // From here the home is known: every way out, a refusal included, leaves
    // through the exit guard (U-34).
    let refused = match (TxnId::parse(txn), Nonce::parse(nonce)) {
        (Ok(txn), Ok(nonce)) => {
            let road = Road {
                home: home.clone(),
                txn,
                nonce: Some(nonce),
                data: data.clone(),
                agents: launch_agents(),
                limits: Limits::PRODUCT,
                starter: install_flip::parent_of_this_process(),
                handed_back: None,
                layouts: own_layouts(),
            };
            match bt_platform::admission::enter_standalone_main("folio-update-apply", |worker| {
                apply(worker, &road, &mut world)
            }) {
                Ok(ended) => {
                    world.say(&format!("BT_UPDATE_APPLY transaction {txn}: {ended:?}"));
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
    world.say(&format!("BT_UPDATE_APPLY {refused}"));
    // Refused before it took the window's mark: the duty stays with O, which
    // armed it at the press and finds no mark (U-34, round 2).
    let mut guard = ExitGuard::new(MacLeave {
        worker: None,
        home: &home,
        world: &mut world,
        data: &data,
    });
    guard.not_mine(None);
    let left = guard.leave();
    drop(guard);
    world.say(&format!("BT_UPDATE_APPLY {}", left.said()));
    2
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
    apply_electing(worker, road, world, |worker, until| {
        crate::update_apply::take_the_window_for_applier(
            worker,
            &road.home,
            road.txn,
            crate::update_apply::this_process(),
            until,
            road.limits.poll,
        )
    })
}

/// [`apply`], with the window election `elect` asked by the road deadline it
/// is given: the product's is `update_apply::take_the_window_for_applier`.
fn apply_electing(
    worker: &WorkerCtx,
    road: &Road,
    world: &mut impl World,
    elect: impl FnOnce(&WorkerCtx, Instant) -> Window,
) -> Ended {
    let window = Instant::now() + road.limits.old_within;
    // Every way out of the road, a panic included, leaves through the guard
    // (U-34). It owes a window only once the election is won (0.4.8 E2):
    // before that the duty is O's, and an applier that ends or panics there
    // starts nothing.
    let mut guard = ExitGuard::contender(MacLeave {
        worker: Some(worker),
        home: &road.home,
        world,
        data: &road.data,
    });
    // **The window's duty first** (U-34, `update_apply::OWNER_FILE` and its
    // lock): taken before the wait for O's lock, while O still runs. An
    // applier that does not get it leaves the transaction untouched.
    // A live owner that is not this applier is O at its end, which leaves
    // anyway: wait for it to go (round 6), within the applier's own wait for
    // O, before giving up the transaction. **One deadline for the whole wait
    // for O** (round 7): the mark, the transaction lock, the claim and the
    // admission all spend `window` — the election's own wait for its lock
    // too (round 8).
    match elect(worker, window) {
        Window::Mine(duty) => {
            if let Some(warning) = duty.warning() {
                guard
                    .inner()
                    .world
                    .say(&format!("BT_UPDATE_APPLY {warning}"));
            }
            guard.owns_window(duty);
        }
        Window::StoodAside(why) => {
            // The journal is one this build cannot read whole (E1): no mark,
            // nothing recorded, and the window duty stays with the build
            // that armed it, as for any applier that proved none — the
            // Windows applier's own arm.
            let left = guard.leave();
            guard
                .inner()
                .world
                .say(&format!("BT_UPDATE_APPLY {why}; {}", left.said()));
            return Ended::StoodAside(why);
        }
        other => {
            // Another live owner, an election O still runs, or this
            // applier's own mark that did not land (0.4.8 E2): the duty is
            // the owner's or O's, which armed it.
            if let Window::Theirs(owner) = &other {
                guard.not_mine(Some(owner.pid));
            }
            let left = guard.leave();
            guard
                .inner()
                .world
                .say(&format!("BT_UPDATE_APPLY {other:?}; {}", left.said()));
            return Ended::Refused(format!("the window is not this applier's: {other:?}"));
        }
    }
    let (ended, successor) = {
        let world = &mut *guard.inner().world;
        match Txn::hold(road, worker, Asker::LockHolder, window) {
            Ok((mut txn, bundles)) => {
                let places = bundles.places();
                let mut ended = txn.run(worker, &places, window, world);
                if ended.rolled_back() {
                    ended = txn.retry_as_trial(worker, &places, world, ended, &[]);
                }
                world.say(&format!(
                    "BT_UPDATE_APPLY transaction {} wrote {:?}",
                    road.txn, txn.written
                ));
                // The lock is let go (`txn` dropped here) before the
                // installed build starts: its own start retires a finished
                // transaction, which needs it.
                (ended, txn.successor)
            }
            Err(ended) => (ended, None),
        }
    };
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

/// **The rescue build as recovery, over a macOS bundle's transaction in any
/// phase a dead applier can leave** (U-29 and U-29b, the coordinator's
/// ruling 1): the transaction lock within [`Limits::old_within`], then the
/// journal, and each step `update_txn::decide`'s answer for
/// [`Asker::Rescue`] from what is on disk:
///
/// - `Handoff` / `Armed` — nothing exchanged: the entrance removed, back to
///   `Prepared` (unless a live process holds the window's mark,
///   `update_apply::OWNER_FILE`: then it is left to it, as
///   [`Ended::LockHeld`]);
/// - `Moving` — decided by the live identity: the old one → `Prepared`; the
///   new one → the new build started as the trial, `Trial` recorded, and
///   waited for as below; a trial that cannot be started → `RollbackIntent`;
/// - `Trial` without a receipt — its recorded process, running from the new
///   bundle's executable, waited for through the wait door until the
///   deadline counted from `Trial.began_ms`; a receipt with its nonce →
///   `Committed` and the cleanup (W8/M8); otherwise `RollbackIntent` and the
///   rollback (M9);
/// - `RollbackIntent`, `Stuck`, `RolledBack` rolled back and retired (M9–M11),
///   `Committed` retired as after a commit (M11);
/// - `Retired`, `Abandoned` or `Prepared` — decided by the lock holder a
///   person's start waited for (U-40): its entrance removed if one is left,
///   and [`Ended::Left`]; the start then opens what the disk names.
///
/// Handed a person's start (`start`), a `Stuck` left with the new bundle
/// live has the new build started as a trial — recorded, and its receipt
/// commits forward (ruling 3) — with `start` after its words. What opens
/// after is [`Recovered::opens`].
pub(crate) fn recover(
    worker: &WorkerCtx,
    road: &Road,
    hands: &mut impl Hands,
    start: Option<&[OsString]>,
) -> Recovered {
    let opener = if start.is_some() {
        Opener::Start
    } else {
        Opener::Login
    };
    let handed = start.unwrap_or(&[]);
    let (ended, successor, deferred_to) = match Txn::hold(
        road,
        worker,
        Asker::Rescue,
        Instant::now() + road.limits.old_within,
    ) {
        Ok((mut txn, bundles)) => {
            let places = bundles.places();
            let mut ended = if txn.phase() == PhaseKind::Handoff {
                match crate::update_apply::window_holder(
                    &road.home,
                    road.txn,
                    crate::update_apply::this_process(),
                ) {
                    Ok(Some(crate::update_apply::WindowHolder::Marked(applier))) => {
                        hands.say(&format!(
                            "BT_UPDATE_RECOVER {} has the update's window; the handed-off update is left to it",
                            applier.pid
                        ));
                        txn.successor = Some(applier);
                        Ended::LockHeld
                    }
                    Ok(Some(crate::update_apply::WindowHolder::Unmarked)) => {
                        hands.say(
                            "BT_UPDATE_RECOVER an applier's window election is still in flight; the handed-off update is left to it",
                        );
                        Ended::Deferred(Deferral::WindowDuty)
                    }
                    Ok(None) => txn
                        .settle(worker, &places, None, hands, handed)
                        .unwrap_or_else(Ended::Failed),
                    Err(why) => Ended::Failed(why),
                }
            } else {
                txn.settle(worker, &places, None, hands, handed)
                    .unwrap_or_else(Ended::Failed)
            };
            if ended.rolled_back() && opener == Opener::Start {
                ended = txn.retry_as_trial(worker, &places, hands, ended, handed);
            }
            hands.say(&format!(
                "BT_UPDATE_RECOVER transaction {} wrote {:?}",
                road.txn, txn.written
            ));
            let deferred_to = match (&ended, txn.successor) {
                (_, Some(_)) if txn.successor_has_the_start => None,
                (_, Some(successor)) => Some(Ahead::Process(successor)),
                (Ended::Deferred(Deferral::WindowDuty), None) => Some(Ahead::Election {
                    home: road.home.clone(),
                    txn: road.txn,
                    me: crate::update_apply::this_process(),
                }),
                (Ended::Deferred(Deferral::Held), None) => Some(Ahead::DataHolder),
                _ => None,
            };
            (ended, txn.successor, deferred_to)
        }
        // Another holder kept the transaction lock through the wait: it opens
        // the window.
        Err(Ended::LockHeld) => (Ended::LockHeld, None, Some(Ahead::Lock(road.home.lock()))),
        Err(ended) => (ended, None, None),
    };
    // At login, every end that attempted the transaction owes a window; only
    // the no-op ends do not (U-34, round 2, blocker 4 — as
    // `update_apply::owed_at_logon`).
    let waiting = opener == Opener::Start
        || !matches!(
            ended,
            Ended::Committed
                | Ended::CommittedWithDebt(_)
                | Ended::Refused(_)
                | Ended::LockHeld
                | Ended::Abandoned
                | Ended::Deferred(_)
                | Ended::Left(_)
                | Ended::StoodAside(_)
        );
    Recovered {
        ended,
        successor,
        waiting,
        deferred_to: deferred_to.filter(|_| opener == Opener::Start),
    }
}

/// **What the disk names to start now** (the coordinator's rulings 2 and 3,
/// U-29b; since U-34 read only by an exit guard): the installed build — as a
/// trial while it is the new build and the transaction not `Committed` (or
/// when no journal says which it is but the header is still `destructive`),
/// and with `--update-failed` while the header is `destructive` or a retired
/// rollback. Without a worker the live bundle is not asked, and a
/// `destructive` header is answered with a trial, safe for either build.
///
/// **A journal this build cannot read whole** (0.4.8 E1,
/// [`Role::MacExit`]) is answered by its header's frozen class when the
/// header reads, and otherwise as a `destructive` transaction whose live set
/// is not known: a trial of the envelope's transaction, its writes held —
/// and when not even the envelope reads, so that no transaction can be named
/// for a trial, the installed build with `--update-failed`, which continues
/// past it with the card that says so. Never the installed build plainly.
pub(crate) fn opens_now(worker: Option<&WorkerCtx>, home: &Home) -> Opens {
    opens_now_with(worker, home, &own_layouts())
}

/// [`opens_now`] through `layouts`; separated so the layout recorder can pin
/// the exit guard's `Prove` call without replacing any product effect.
fn opens_now_with(
    worker: Option<&WorkerCtx>,
    home: &Home,
    layouts: &Layouts<dyn ApplyPoints>,
) -> Opens {
    // Only a journal that is not there opens the installed build plainly; a
    // file that could not be read is unreadable (E1 round 2).
    let Some(seen) =
        Role::MacExit.sight_of_read(file_reads::read(Lane::UpdateJournal, home.journal()))
    else {
        return Opens::Installed { failed: false };
    };
    let Some(header) = seen.acting_header() else {
        return Opens::Installed { failed: true };
    };
    let destructive = header.class == Class::Destructive;
    let failed = destructive || header.outcome == HeaderOutcome::RolledBack;
    let journal = match seen {
        Sight::Known(journal) => Some(journal),
        Sight::Header { .. } | Sight::Envelope { .. } | Sight::Unreadable(_) => None,
    };
    let new_live = match (journal.as_ref(), worker) {
        (Some(journal), Some(worker)) => exit_places(home, journal)
            .and_then(|bundles| {
                layouts
                    .named(journal.body.adapter)
                    .ok()
                    .map(|layout| (layout, bundles))
            })
            .map(|(layout, bundles)| {
                let places = bundles.places();
                layout.locate(worker, &places).0.as_ref() == Some(places.new)
            })
            .unwrap_or(destructive),
        // Which build is live cannot be told: a trial is safe for either.
        _ => destructive,
    };
    if new_live && header.outcome != HeaderOutcome::Committed {
        Opens::Trial { txn: header.txn }
    } else {
        Opens::Installed { failed }
    }
}

/// The paths and recorded identities the exit guard needs to ask a layout
/// which bundle is live. This is the same derivation [`Txn::hold`] uses.
fn exit_places(home: &Home, journal: &Journal) -> Option<Bundles> {
    let (installed, program, stage, rescue_program) = (
        home.installed_bundle()?,
        home.installed_program()?,
        home.stage_bundle(journal.txn)?,
        home.rescue_executable(journal.txn)?,
    );
    let Layout::Bundle { old, new } = journal.body.layout.clone() else {
        return None;
    };
    let inside = program.strip_prefix(&installed).unwrap_or(&program);
    let stage_program = stage.join(inside);
    Some(Bundles {
        installed,
        program,
        stage,
        stage_program,
        rescue_program,
        old,
        new,
    })
}

/// The paths and identities one road acts on, owned.
struct Bundles {
    installed: PathBuf,
    program: PathBuf,
    stage: PathBuf,
    stage_program: PathBuf,
    rescue_program: PathBuf,
    old: BundleIdentity,
    new: BundleIdentity,
}

impl Bundles {
    fn places(&self) -> Places<'_> {
        Places {
            installed: &self.installed,
            program: &self.program,
            stage: &self.stage,
            stage_program: &self.stage_program,
            rescue_program: &self.rescue_program,
            old: &self.old,
            new: &self.new,
        }
    }
}

/// The paths and identities one road acts on.
pub(crate) struct Places<'a> {
    installed: &'a Path,
    program: &'a Path,
    stage: &'a Path,
    /// The staged bundle's executable: where the new build's image is after a
    /// swap back, and the old build's before it.
    stage_program: &'a Path,
    /// The rescue clone's executable: what the applier and recovery run from.
    rescue_program: &'a Path,
    old: &'a BundleIdentity,
    new: &'a BundleIdentity,
}

/// **One transaction under its lock**: the journal as it stands durably, and
/// every phase this process wrote, in order.
struct Txn<'a> {
    road: &'a Road,
    /// The worker this holder runs on: a refused journal write sleeps through
    /// its wait door (`update_apply::write_journal`, U-34).
    worker: &'a WorkerCtx,
    /// Who this holder is to `decide`: the applier, or the rescue build as
    /// recovery.
    asker: Asker,
    journal: Journal,
    /// Held for the whole road; let go when the value is dropped.
    _lock: Held,
    plist: Option<PathBuf>,
    written: Vec<PhaseKind>,
    /// The layout the journal names, whose points this road calls.
    layout: Arc<dyn ApplyPoints>,
    /// **The successor this holder leaves behind**: the trial it started, once
    /// found and recorded (`TrialBegan`, `RetrialBegan`), or the applier found
    /// at `Handoff`. While it runs it is the window and the exit guard starts
    /// nothing (U-34; U-29b's ruling 2's "exactly one"); a trial a rollback
    /// stopped no longer runs.
    successor: Option<Running>,
    /// **A trial this holder started was handed the person's start** with its
    /// words ([`Txn::begin_trial`]): that start is delivered with it and
    /// nothing is carried (0.4.8 E3).
    successor_has_the_start: bool,
}

impl<'a> Txn<'a> {
    /// **The transaction lock by `until`** — the road's one deadline, counted
    /// from its start ([`Limits::old_within`]) — **then the journal**: this
    /// road's transaction, a bundle's.
    fn hold(
        road: &'a Road,
        worker: &'a WorkerCtx,
        asker: Asker,
        until: Instant,
    ) -> Result<(Self, Bundles), Ended> {
        let (Some(installed), Some(program), Some(stage), Some(rescue_program)) = (
            road.home.installed_bundle(),
            road.home.installed_program(),
            road.home.stage_bundle(road.txn),
            road.home.rescue_executable(road.txn),
        ) else {
            return Err(Ended::Refused(
                "the home is not a macOS bundle's".to_owned(),
            ));
        };
        let lock = match install_txn::hold_within(
            &road.home.lock(),
            Hold::Exclusive,
            until.saturating_duration_since(Instant::now()),
        ) {
            Ok(Some(held)) => held,
            Ok(None) => return Err(Ended::LockHeld),
            Err(failure) => return Err(Ended::Failed(failure.to_string())),
        };
        // A journal this build cannot read whole is stood aside from: nothing
        // recorded, the lock let go as this returns (E1, `Role::MacHolder`).
        let journal = match Role::MacHolder
            .sight_of_read(file_reads::read(Lane::UpdateJournal, road.home.journal()))
        {
            Some(Sight::Known(journal)) => journal,
            Some(beyond) => {
                return Err(Ended::StoodAside(beyond.said(Role::MacHolder)));
            }
            None => return Err(Ended::Refused("there is no journal".to_owned())),
        };
        if journal.txn != road.txn {
            return Err(Ended::Refused(format!(
                "the journal is transaction {}, not {}",
                journal.txn, road.txn
            )));
        }
        let Layout::Bundle { old, new } = journal.body.layout.clone() else {
            return Err(Ended::Refused("the journal is not a bundle's".to_owned()));
        };
        let layout = road
            .layouts
            .named(journal.body.adapter)
            .map_err(|not_built| Ended::Refused(not_built.to_string()))?;
        let inside = program.strip_prefix(&installed).unwrap_or(&program);
        let stage_program = stage.join(inside);
        let txn = Txn {
            road,
            worker,
            asker,
            journal,
            _lock: lock,
            plist: road
                .agents
                .as_ref()
                .map(|agents| agents.join(launch_agent::file_name(road.txn.bytes()))),
            written: Vec::new(),
            layout,
            successor: None,
            successor_has_the_start: false,
        };
        Ok((
            txn,
            Bundles {
                installed,
                program,
                stage,
                stage_program,
                rescue_program,
                old,
                new,
            },
        ))
    }

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
        crate::update_apply::write_journal(self.worker, &self.road.home.journal(), &next.encode())?;
        self.journal = next;
        self.written.push(phase);
        if let Event::TrialBegan { process, .. } | Event::RetrialBegan { process, .. } = event {
            // The trial this holder started, found: its successor.
            self.successor = Some(Running {
                pid: process.pid,
                started: process.started,
            });
        }
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
            Phase::Handoff { applier } if Some(applier) == self.road.nonce => {
                self.at_handoff(worker, places, window, world)
            }
            Phase::Handoff { .. } => {
                return Ended::Refused("the journal was handed to another applier".to_owned());
            }
            Phase::Armed => self.at_armed(worker, places, window, world),
            Phase::Moving => self.at_moving_again(worker, places, world),
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
            places.rescue_program,
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
        // A process of the installed executable that is leaving — a person's
        // start that hands itself to the recovery build is gone within moments
        // — is waited for within the window, as the Windows road waits for a
        // held file (U-40); only one still running when the window ends
        // reverts the transaction.
        let program = places.program;
        let mut said_waiting = false;
        let let_go = crate::update_apply::until_let_go(
            window,
            self.road.limits.poll,
            &mut Instant::now,
            &mut || {
                let running = install_flip::running_from(program)
                    .map_err(|error| format!("the process list: {error}"))?;
                let held: Vec<String> = running
                    .iter()
                    .map(|process| format!("{} (pid {})", program.display(), process.pid))
                    .collect();
                if !held.is_empty() && !said_waiting {
                    said_waiting = true;
                    world.say(&format!(
                        "BT_UPDATE_APPLY {} runs; the exchange waits for it to leave",
                        held.join(", ")
                    ));
                }
                Ok(held)
            },
            &mut |pause| bt_platform::wait::sleep_within(worker, pause),
        );
        if let Err(why) = let_go {
            world.say(&format!("BT_UPDATE_APPLY {why}"));
            return self.revert(Actor::Applier);
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
        // Activate, forward: the layout's (managed-update R1).
        let activated = self.layout.activate(places, world);
        if let Err(error) = activated {
            world.say(&format!("BT_UPDATE_APPLY the exchange: {error}"));
            // Decided by what is live, as recovery decides it (M5, M6).
            drop(admission);
            return self.after_activate_refusal(worker, places, world);
        }
        // The trial takes its shared hold at its start.
        drop(admission);
        self.begin_trial(worker, places, Actor::Applier, world, &[])
    }

    /// What follows an `Activate` error, from the named layout's proof of the
    /// live set: no effect reverts to `Prepared`; any partial or complete
    /// effect declares the rollback.
    fn after_activate_refusal(
        &mut self,
        worker: &WorkerCtx,
        places: &Places<'_>,
        world: &mut impl World,
    ) -> Result<Ended, String> {
        let (live, _) = self.layout.locate(worker, places);
        if live.as_ref() == Some(places.old) {
            self.revert(Actor::Applier)
        } else {
            self.declare_rollback(worker, places, Actor::Applier, world)
        }
    }

    /// M5 / M6, found by a process started again over its own transaction:
    /// decided by the live identity, never by the phase — as recovery decides
    /// it (U-29b: the new bundle live and the old one in `stage/` is decided
    /// by a trial).
    fn at_moving_again(
        &mut self,
        worker: &WorkerCtx,
        places: &Places<'_>,
        world: &mut impl World,
    ) -> Result<Ended, String> {
        let (live, stage) = self.layout.locate(worker, places);
        if live.as_ref() == Some(places.old) {
            self.revert(Actor::Recovery)
        } else if live.as_ref() == Some(places.new) && stage.as_ref() == Some(places.old) {
            // A trial of it that runs and has answered is recorded, never a
            // second one started beside it (U-37).
            match self.before_deciding(
                places,
                Actor::Recovery,
                (Some(places.new), Some(places.old)),
                None,
                world,
            )? {
                Pre::Again => return self.settle(worker, places, None, world, &[]),
                Pre::Stop(ended) => return Ok(ended),
                Pre::Decide => {}
            }
            self.begin_trial(worker, places, Actor::Recovery, world, &[])
        } else {
            self.declare_rollback(worker, places, Actor::Recovery, world)
        }
    }

    /// **Start the new build as the trial** (M6 → M7) — over `Moving` after
    /// the exchange, or over a `Stuck` whose new bundle is live (the
    /// coordinator's ruling 3, U-29b) — with `handed` after its words, then
    /// wait for it ([`Txn::watch_trial`]). A receipt the journal accepts →
    /// `Committed` and the cleanup (M8). None: over `Moving` or `Trial`,
    /// `RollbackIntent` and the rollback (M9); over `Stuck`, nothing more —
    /// the trial is left running, and the next holder decides.
    fn begin_trial(
        &mut self,
        worker: &WorkerCtx,
        places: &Places<'_>,
        actor: Actor,
        hands: &mut impl Hands,
        handed: &[OsString],
    ) -> Result<Ended, String> {
        let nonce = crate::update_job::mint_nonce();
        let began_ms = now_ms();
        let over_stuck = self.phase() == PhaseKind::Stuck;
        let mut args = trial_words(self.road.txn, &nonce).to_vec();
        if over_stuck {
            // Its card says the update is incomplete, as `Stuck`'s start's
            // always did (U-29).
            args.extend(failed_words(&self.road.home));
        }
        args.extend_from_slice(handed);
        let mut launch = match hands.launch_trial(places.installed, &args) {
            Ok(launch) => {
                self.successor_has_the_start = true;
                launch
            }
            Err(error) => {
                hands.say(&format!("BT_UPDATE_APPLY {OPEN} did not start: {error}"));
                return if over_stuck {
                    Ok(self.still_stuck())
                } else {
                    self.declare_rollback(worker, places, actor, hands)
                };
            }
        };
        let watched = self.watch_trial(
            worker,
            places,
            actor,
            hands,
            Some((nonce, began_ms)),
            &mut launch,
        );
        drop(launch);
        match watched? {
            Some(ended) => Ok(ended),
            None if self.phase() == PhaseKind::Stuck => Ok(self.still_stuck()),
            None => self.declare_rollback(worker, places, actor, hands),
        }
    }

    /// `Stuck`'s end as it stands: its last error.
    fn still_stuck(&self) -> Ended {
        match &self.journal.body.phase {
            Phase::Stuck { last_error, .. } => Ended::Stuck(last_error.clone()),
            other => Ended::Failed(format!("{:?} is not Stuck", other.kind())),
        }
    }

    /// **The trial, waited for** (M7, W7): the one this holder just started
    /// (`started`: its nonce and when), found by exact evidence — the
    /// receipt's own, or the process of the installed executable that carries
    /// its words (`update_apply::launched_trial`, U-40) — and recorded
    /// (`Trial` over `Moving`, the retrial over
    /// `Stuck`); or the one the journal records. Then, polling through the
    /// wait door until the deadline counted from its start: a receipt the
    /// journal accepts → `Committed` and the cleanup (`Some`); the process
    /// gone from the new bundle's executable, or the deadline → `None`. A
    /// trial this holder started over `Moving` whose start cannot be recorded
    /// is ended, and → `None`, as a trial that did not start (U-34).
    fn watch_trial(
        &mut self,
        worker: &WorkerCtx,
        places: &Places<'_>,
        actor: Actor,
        hands: &mut impl Hands,
        started: Option<(Nonce, u64)>,
        launch: &mut Launch,
    ) -> Result<Option<Ended>, String> {
        // The new build is live for the whole wait: nothing is exchanged
        // under this lock meanwhile.
        let (live, stage) = self.layout.locate(worker, places);
        let images: Vec<&Path> = [(&live, places.program), (&stage, places.stage_program)]
            .into_iter()
            .filter(|(identity, _)| identity.as_ref() == Some(places.new))
            .map(|(_, program)| program)
            .collect();
        let road = self.road;
        let watch = Watch {
            home: &road.home,
            actor,
            started,
            poll: road.limits.poll,
            trial_within_ms: road.limits.trial_within_ms,
        };
        // LaunchServices reports no pid: the trial is the receipt's own, or
        // the process of the installed executable that carries its words —
        // never one that merely started after the launch (U-40).
        let (txn, program) = (road.txn, places.program);
        let mut find = |nonce: Nonce, receipt: Option<&Receipt>| {
            crate::update_apply::launched_trial(txn, nonce, program, receipt)
        };
        let watched = crate::update_apply::watch_trial(
            worker,
            self,
            &watch,
            &mut find,
            &mut || launch.over(),
            &mut |process| trial_runs(process, &images),
            &mut |line| hands.say(line),
        )?;
        Ok(match watched {
            Watched::Committed => Some(self.commit(worker, places, actor)),
            Watched::NoReceipt => None,
            Watched::Unrecorded { process, why } => {
                hands.say(&format!(
                    "BT_UPDATE_APPLY the trial {} could not be recorded: {why}",
                    process.pid
                ));
                if self.phase() == PhaseKind::Stuck {
                    // A retrial carries `--update-failed`: unrecorded, it is
                    // what `Opens::Trial` starts over `Stuck`, and it runs on
                    // as the window (U-34).
                    self.successor = Some(Running {
                        pid: process.pid,
                        started: process.started,
                    });
                    return Err(why);
                }
                // Over `Moving` the journal does not know it (U-34): ended by
                // its pid, start instant and image, and the road goes on as
                // for a trial that did not start. One that will not end runs
                // on as the window.
                self.may(actor, Effect::EndTrial)?;
                let limits = &self.road.limits;
                stop_trial(
                    worker,
                    process,
                    &images,
                    (limits.quit_within, limits.end_within, limits.poll),
                    &mut |line| hands.say(line),
                )
                .map_err(|stop| {
                    // It will not end: it runs on as the window.
                    self.successor = Some(Running {
                        pid: process.pid,
                        started: process.started,
                    });
                    format!("{why}; {stop}")
                })?;
                None
            }
        })
    }

    /// **The coordinator's ruling 3 (U-29b)**: a road that left the
    /// transaction `Stuck` with the new bundle live, and no trial of it
    /// running, starts the new build as a trial over it — recorded, so its
    /// receipt commits forward (W8/M8) — with `handed` after its words, and
    /// waits for it under this lock. What the road ended as then: the commit,
    /// or `ended` as it was.
    fn retry_as_trial(
        &mut self,
        worker: &WorkerCtx,
        places: &Places<'_>,
        hands: &mut impl Hands,
        ended: Ended,
        handed: &[OsString],
    ) -> Ended {
        let Phase::Stuck { trial, .. } = &self.journal.body.phase else {
            return ended;
        };
        if trial.is_some_and(|process| trial_runs(process, &[places.program])) {
            // A trial of it runs already: it is the window.
            return ended;
        }
        let (live, _) = self.layout.locate(worker, places);
        if live.as_ref() != Some(places.new) {
            return ended;
        }
        let actor = match self.asker {
            Asker::LockHolder => Actor::Applier,
            _ => Actor::Recovery,
        };
        // Never a retrial beside a candidate, over a held or unaskable claim,
        // or when what runs cannot be read (H.3); a running trial its receipt
        // names is recorded instead and commits.
        match self.before_deciding(places, actor, (Some(places.new), None), None, hands) {
            Ok(Pre::Decide) => {}
            Ok(Pre::Again) => {
                return match self.settle(worker, places, None, hands, handed) {
                    Ok(committed @ (Ended::Committed | Ended::CommittedWithDebt(_))) => committed,
                    Ok(_) => ended,
                    Err(error) => {
                        hands.say(&format!("BT_UPDATE_ROLLBACK the trial over Stuck: {error}"));
                        ended
                    }
                };
            }
            // The deferral is the end (H.3): a held claim then starts
            // nothing beside its holder (`ExitGuard::window_elsewhere`).
            Ok(Pre::Stop(deferred)) => return deferred,
            Err(error) => {
                hands.say(&format!("BT_UPDATE_ROLLBACK the trial over Stuck: {error}"));
                return ended;
            }
        }
        match self.begin_trial(worker, places, actor, hands, handed) {
            Ok(committed @ (Ended::Committed | Ended::CommittedWithDebt(_))) => committed,
            Ok(_) => ended,
            Err(error) => {
                hands.say(&format!("BT_UPDATE_ROLLBACK the trial over Stuck: {error}"));
                ended
            }
        }
    }

    /// M8 and M11 after `Committed` is durable: the old bundle, the entrance,
    /// then `Retired{Committed}` and `H/<txn>`. Every failure here is debt.
    fn commit(&mut self, worker: &WorkerCtx, places: &Places<'_>, actor: Actor) -> Ended {
        let mut debt = Vec::new();
        let (_, stage) = self.layout.locate(worker, places);
        if stage.as_ref() == Some(places.old) {
            match self.may(actor, Effect::DeleteRollbackMaterial) {
                Ok(()) => {
                    if let Err(failure) = install_txn::durable_remove(places.stage) {
                        debt.push(failure.to_string());
                    }
                }
                Err(why) => debt.push(why),
            }
        }
        if let Err(why) = self.disarm(actor) {
            debt.push(why);
        }
        self.retire(actor, &mut debt);
        if debt.is_empty() {
            Ended::Committed
        } else {
            Ended::CommittedWithDebt(debt.join("; "))
        }
    }

    /// `Retired`, then `H/<txn>` with everything left in it — the rescue clone
    /// this process may run from included (a Unix process may remove its own
    /// image). The journal is kept: after a commit for the trial's watch,
    /// after a rollback for the relaunched build's card; the next ordinary
    /// start retires it.
    fn retire(&mut self, actor: Actor, debt: &mut Vec<String>) {
        if let Err(why) = self.record(actor, &Event::Retired) {
            debt.push(why);
            return;
        }
        match self.may(actor, Effect::DeleteTxnDir) {
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

    /// `RollbackDeclared` as `actor`, durable, then the rollback.
    fn declare_rollback(
        &mut self,
        worker: &WorkerCtx,
        places: &Places<'_>,
        actor: Actor,
        hands: &mut impl Hands,
    ) -> Result<Ended, String> {
        self.record(actor, &Event::RollbackDeclared)?;
        self.settle(worker, places, Some(actor), hands, &[])
    }

    /// **Every phase from what is on disk, one step at a time** (U-29b's one
    /// recovery road, and M9–M11 — see the module header, step 7). Each step
    /// is the one `update_txn::decide` names for this holder now, so a road
    /// cut short anywhere is finished by the next holder from wherever it
    /// stood. `tenure` is the actor every record is made as; `None` takes the
    /// one the phase gives this holder's asker. `handed` goes after the
    /// words of a trial started here.
    fn settle(
        &mut self,
        worker: &WorkerCtx,
        places: &Places<'_>,
        tenure: Option<Actor>,
        hands: &mut impl Hands,
        handed: &[OsString],
    ) -> Result<Ended, String> {
        // Waits aside, at most: declare, stop the trial, swap back, declare,
        // retire.
        for _ in 0..8 {
            let actor = tenure.unwrap_or_else(|| self.asker.actor(self.phase()));
            let (trial, nonce) = match &self.journal.body.phase {
                Phase::Trial { process, nonce, .. } => (Some(*process), Some(*nonce)),
                Phase::RollbackIntent { trial, .. } => (*trial, None),
                Phase::Stuck { trial, retrial, .. } => {
                    (*trial, retrial.map(|retrial| retrial.nonce))
                }
                _ => (None, None),
            };
            let (live, stage) = self.layout.locate(worker, places);
            // The trial's executable is the new bundle's, wherever it is now.
            let images: Vec<&Path> = [(&live, places.program), (&stage, places.stage_program)]
                .into_iter()
                .filter(|(identity, _)| identity.as_ref() == Some(places.new))
                .map(|(_, program)| program)
                .collect();
            let trial_alive = trial.is_some_and(|process| trial_runs(process, &images));
            let entrance = self
                .plist
                .as_ref()
                .is_some_and(|plist| std::fs::symlink_metadata(plist).is_ok());
            let receipt = nonce
                .and_then(|nonce| read_receipt(&self.road.home.receipt_path(self.road.txn, &nonce)))
                .and_then(Result::ok);
            match self.before_deciding(
                places,
                actor,
                (live.as_ref(), stage.as_ref()),
                trial.filter(|_| trial_alive),
                hands,
            )? {
                Pre::Again => continue,
                Pre::Stop(ended) => return Ok(ended),
                Pre::Decide => {}
            }
            let located = Located::Bundle { live, stage };
            let action = decide(&Disk {
                journal: &self.journal,
                asker: self.asker,
                entrance,
                located,
                receipt: receipt.clone(),
                trial_alive,
                now_ms: now_ms(),
            });
            match action {
                Action::Revert { .. } => return self.revert(actor),
                Action::BeginTrial => {
                    return self.begin_trial(worker, places, actor, hands, handed);
                }
                Action::AwaitReceipt { .. } => {
                    if let Some(ended) = self.watch_trial(
                        worker,
                        places,
                        actor,
                        hands,
                        None,
                        &mut Launch::untracked(),
                    )? {
                        return Ok(ended);
                    }
                    if self.phase() == PhaseKind::Stuck {
                        // The trial started over `Stuck` gave none: it is left
                        // to the next holder, which stops it first.
                        return Ok(self.still_stuck());
                    }
                    self.record(actor, &Event::RollbackDeclared)?;
                }
                Action::Commit => {
                    let Some(receipt) = receipt else {
                        return Err("a commit without its receipt".to_owned());
                    };
                    self.record(actor, &Event::ReceiptAccepted(receipt))?;
                    return Ok(self.commit(worker, places, actor));
                }
                Action::DeclareRollback => {
                    if let Some(process) = trial {
                        hands.say(&format!(
                            "BT_UPDATE_RECOVER the trial {} gave no receipt",
                            process.pid
                        ));
                    } else if self.phase() == PhaseKind::Moving {
                        // No trial was begun: the moves were cut short (W6; U-42a).
                        hands.say(
                            "BT_UPDATE_RECOVER the update was interrupted before the new version started",
                        );
                    }
                    self.record(actor, &Event::RollbackDeclared)?;
                }
                Action::FinishCommit { .. } => return Ok(self.commit(worker, places, actor)),
                Action::StopTrial(process) => {
                    self.may(actor, Effect::EndTrial)?;
                    let limits = &self.road.limits;
                    if let Err(why) = stop_trial(
                        worker,
                        process,
                        &images,
                        (limits.quit_within, limits.end_within, limits.poll),
                        &mut |line| hands.say(line),
                    ) {
                        return self.stuck(actor, why, hands);
                    }
                }
                Action::RollBack(Restore::SwapBack) => {
                    let admission = match install_txn::hold_within(
                        &self.road.home.admission(),
                        Hold::Exclusive,
                        self.road.limits.old_within,
                    ) {
                        Ok(Some(held)) => held,
                        Ok(None) => {
                            let why = "a running copy holds the admission".to_owned();
                            hands.say(&format!("BT_UPDATE_ROLLBACK {why}; the next start tries"));
                            return Ok(Ended::RollbackWaits(why));
                        }
                        Err(failure) => {
                            return self.stuck(actor, format!("the admission: {failure}"), hands);
                        }
                    };
                    self.may(actor, Effect::Swap)?;
                    // Activate, back: the layout's (managed-update R1).
                    let swapped = self.layout.activate_back(places, hands);
                    drop(admission);
                    if let Err(error) = swapped {
                        return self.stuck(actor, format!("the exchange back: {error}"), hands);
                    }
                }
                Action::DeclareRolledBack => {
                    if let Err(why) = hands.verify_restored(worker, places.installed) {
                        return self.stuck(actor, format!("the restored bundle: {why}"), hands);
                    }
                    self.record(actor, &Event::RolledBack)?;
                }
                Action::StayStuck { reason } => return self.stuck(actor, reason, hands),
                Action::GiveUp { last_error } => {
                    hands.say(&format!(
                        "BT_UPDATE_ROLLBACK the update is incomplete after {} attempts ({last_error}); see {}",
                        crate::update_txn::STUCK_ATTEMPT_LIMIT,
                        self.road.home.root().display()
                    ));
                    return Ok(Ended::GaveUp(last_error));
                }
                Action::FinishRollback { .. } => return Ok(self.finish_rollback(actor)),
                // Decided by the holder this recovery waited for (U-40):
                // complete evidence, and nothing left but what the next
                // ordinary start does — as the Windows recovery leaves them.
                Action::Retire { .. } => {
                    self.disarm(actor)?;
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
        let actor = tenure.unwrap_or_else(|| self.asker.actor(self.phase()));
        self.stuck(actor, "the rollback did not settle".to_owned(), hands)
    }

    /// **H.3, on macOS** (0.4.7 ticket U-37, design revision (h)): over
    /// `Moving` with the new bundle live and the old one in `stage/`, or over a
    /// `Stuck` whose recorded trial no longer runs with the new bundle live,
    /// before `decide` — `update_apply::before_deciding` over the installed
    /// executable, this road's data directory, the handed-back trial of
    /// `--from-trial` and this recovery's own starter. A trial its receipt
    /// names exactly is recorded (`Pre::Again`; `decide` then commits on its
    /// receipt) and is this holder's successor; a handed-back trial that never
    /// became ready is ended by M9's stop of that instance; any other process of
    /// the new bundle, a held or unaskable claim, or a process list that cannot
    /// be read ends the road `Ended::Deferred` with nothing recorded, started or
    /// swapped — never a second trial beside a candidate (the rehearsal's
    /// defect 9).
    ///
    /// # Errors
    /// The adoption's record failed; the adopted trial runs on as the window.
    fn before_deciding(
        &mut self,
        places: &Places<'_>,
        actor: Actor,
        (live, stage): (Option<&BundleIdentity>, Option<&BundleIdentity>),
        recorded: Option<TrialProcess>,
        hands: &mut impl Hands,
    ) -> Result<Pre, String> {
        let new_live = live == Some(places.new);
        let over_stuck = match &self.journal.body.phase {
            Phase::Moving if new_live && stage == Some(places.old) => false,
            Phase::Stuck { .. } if new_live && recorded.is_none() => true,
            _ => return Ok(Pre::Decide),
        };
        let stale = match &self.journal.body.phase {
            Phase::Stuck { trial, .. } => *trial,
            _ => None,
        };
        let road = self.road;
        let excluded: Vec<Running> = stale
            .map(|trial| Running {
                pid: trial.pid,
                started: trial.started,
            })
            .into_iter()
            .chain(road.starter)
            .collect();
        let worker = self.worker;
        let may_end = self.may(actor, Effect::EndTrial);
        let program = places.program;
        let mut end = |process: TrialProcess| {
            may_end.clone()?;
            stop_trial(
                worker,
                process,
                &[program],
                (
                    road.limits.quit_within,
                    road.limits.end_within,
                    road.limits.poll,
                ),
                &mut |line| hands.say(line),
            )
        };
        let what = crate::update_apply::Before {
            home: &road.home,
            txn: road.txn,
            program,
            over_stuck,
            excluded: &excluded,
            handed_back: road.handed_back,
            data: &road.data,
        };
        match crate::update_apply::before_deciding(&what, &mut end) {
            BeforeDeciding::Decide => Ok(Pre::Decide),
            BeforeDeciding::Defer(deferral) => {
                hands.say(&format!("BT_UPDATE_RECOVER deferred: {}", deferral.said()));
                if let Deferral::Candidate(candidate) = &deferral {
                    self.successor = Some(*candidate);
                }
                Ok(Pre::Stop(Ended::Deferred(deferral)))
            }
            BeforeDeciding::Adopt { event, successor } => {
                hands.say(&format!(
                    "BT_UPDATE_RECOVER the trial {} runs and its receipt names it, and the journal does not record it: it is recorded",
                    successor.pid
                ));
                self.successor = Some(successor);
                self.record(actor, &event)?;
                Ok(Pre::Again)
            }
        }
    }

    /// M11 after `RolledBack` is durable: the entrance, then `Retired` and
    /// `H/<txn>` (the staged new bundle with it). Every failure here is debt.
    fn finish_rollback(&mut self, actor: Actor) -> Ended {
        let mut debt = Vec::new();
        if let Err(why) = self.disarm(actor) {
            debt.push(why);
        }
        self.retire(actor, &mut debt);
        if debt.is_empty() {
            Ended::RolledBack
        } else {
            Ended::RolledBackWithDebt(debt.join("; "))
        }
    }

    /// W10, M10: `RollbackFailed` with `why` — `Stuck`, durable, everything
    /// kept — and the line naming the journal's folder.
    fn stuck(
        &mut self,
        actor: Actor,
        why: String,
        hands: &mut impl Hands,
    ) -> Result<Ended, String> {
        self.record(actor, &Event::RollbackFailed { error: why.clone() })?;
        hands.say(&format!(
            "BT_UPDATE_ROLLBACK the update is incomplete ({why}); see {}",
            self.road.home.root().display()
        ));
        Ok(Ended::Stuck(why))
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

impl Recording for Txn<'_> {
    fn journal(&self) -> &Journal {
        &self.journal
    }

    fn record(&mut self, actor: Actor, event: &Event) -> Result<(), String> {
        Txn::record(self, actor, event)
    }
}

/// **§C.4's authoritative test that O is gone**: the data directory's claim,
/// tried until it is had and let go at once, sleeping between tries through
/// the wait door, until `until`. A live holder or a transient sweep is asked
/// about again; a claim that cannot be asked about is not
/// (`ClaimRefusal::QueryDenied`: no answer is coming).
fn wait_for_the_claim(worker: &WorkerCtx, road: &Road, until: Instant) -> Result<(), String> {
    loop {
        match crate::persist::try_claim(&road.data) {
            Ok(claim) => {
                drop(claim);
                return Ok(());
            }
            Err(
                bt_platform::instance::ClaimRefusal::Held
                | bt_platform::instance::ClaimRefusal::Sweeping,
            ) => {
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
        Ok(bytes) => Some(crate::update_txn::receipt_sight(&bytes).known(Role::MacReceiptWatch)),
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

/// **The product's check of a restored bundle** ([`Hands::verify_restored`]):
/// `codesign --verify --strict --deep --all-architectures` against this
/// process's own designated requirement and the Developer ID requirement
/// (`bt_platform::macos_identity`, U-16).
///
/// # Errors
/// Why not, as a sentence.
pub(crate) fn verify_restored_here(bundle: &Path) -> Result<(), String> {
    use bt_platform::macos_identity;
    let requirement = macos_identity::running_requirement().map_err(|r| r.to_string())?;
    macos_identity::verify_bundle(bundle, &requirement)
        .map(drop)
        .map_err(|refusal| refusal.to_string())
}

/// `open -n -a <bundle> [--args <args>]`, detached, through the one door for
/// a child: `open` returns once LaunchServices has the launch, and is not
/// waited on.
pub(crate) fn open_bundle(bundle: &Path, args: &[OsString]) -> io::Result<()> {
    let mut open = bt_platform::quiet_command(OPEN);
    open.arg("-n").arg("-a").arg(bundle);
    if !args.is_empty() {
        open.arg("--args").args(args);
    }
    open.spawn().map(drop)
}

/// **The trial's launch** (U-38): `open -n -W -a <bundle> --args <args>`,
/// through the one door for a child, held ([`Launch`]) — `open -W` returns when
/// the application it opened ends.
///
/// # Errors
/// `open` could not be started.
pub(crate) fn open_trial(bundle: &Path, args: &[OsString]) -> io::Result<Launch> {
    let mut open = bt_platform::quiet_command(OPEN);
    open.arg("-n").arg("-W").arg("-a").arg(bundle);
    if !args.is_empty() {
        open.arg("--args").args(args);
    }
    open.spawn().map(Launch::of)
}

/// This process's own world.
struct Machine {
    /// Where each line is appended: the data directory's `diagnostics.log`,
    /// or `recover.log` in the home — standard error only while there is none.
    log: Option<(PathBuf, String)>,
}

impl Hands for Machine {
    /// **One writer per line** (0.4.7 ticket U-42d; 0.4.6's D-11): the log
    /// once one is named, standard error only before — the applier's standard
    /// error is the `diagnostics.log` of the Folio that started it, so both
    /// wrote every line twice.
    fn say(&mut self, line: &str) {
        match &self.log {
            Some((log, _)) => {
                let _ = crate::diagnostics::append_note(log, line);
            }
            None => {
                bt_platform::write_std_error(format!("{line}\n").as_bytes());
            }
        }
    }

    fn exchange(&mut self, live: &Path, staged: &Path) -> Result<(), String> {
        install_flip::exchange(live, staged).map_err(|failure| failure.to_string())
    }

    fn verify_restored(&mut self, _worker: &WorkerCtx, bundle: &Path) -> Result<(), String> {
        verify_restored_here(bundle)
    }

    fn launch_trial(&mut self, bundle: &Path, args: &[OsString]) -> io::Result<Launch> {
        open_trial(bundle, args)
    }

    fn acknowledged(&mut self, worker: Option<&WorkerCtx>, data: &Path) -> bool {
        crate::update_apply::claimed_within(worker, data, crate::update_apply::ACKNOWLEDGED_WITHIN)
    }

    fn show_here(&mut self, text: &str) {
        bt_platform::standalone_alert(crate::APP_NAME, text);
    }
}

impl World for Machine {
    fn relaunch(&mut self, bundle: &Path, args: &[OsString]) -> io::Result<()> {
        open_bundle(bundle, args)
    }
}

#[cfg(test)]
#[path = "update_apply_macos_tests.rs"]
mod tests;
