//! **The update job** — one per process, owned by the application and not by a
//! window (0.4.6 ticket U-18; `docs/plans/design/self-update-2026-09-16.md` §B,
//! §D, C11, and revision (b)'s U-18 row).
//!
//! The update check ([`crate::update`]) stops at a tag. This module starts
//! there: it holds the **offer** — `{ txn, tag, asset, hash_doc, to_version }`,
//! minted once when the card would be raised and never derived again — and the
//! state that offer is in:
//!
//! ```text
//! Pending ─both facts→ Idle ─offer→ Available ─Update→ Downloading → Staged → Verified
//!                        ↑              │                  │                     │ Restart
//!                        └─Later/Skip───┘      Cancel─→ Idle└─fail→ Failed       ↓
//!                                                                             Quitting → Committing
//! ```
//!
//! # A typed pending state until the evidence is in
//!
//! Two facts decide an offer and both arrive off the window thread: how this
//! copy was installed ([`crate::install_channel`], its own worker) and what the
//! day's check answered ([`crate::update`], its own worker). The job sits in
//! [`State::Pending`] — naming which of the two it is still waiting for — until
//! **both** have landed. No offer is derived from half of the evidence: a check
//! that answers a newer tag while the channel is still being read is not an
//! offer to a copy that turns out to be scoop's.
//!
//! # Eligibility is a pure function
//!
//! [`Evidence::eligibility`] reads the offer decision
//! ([`crate::update::should_offer`]: newer, and above a skipped tag by
//! precedence), the channel (§D: only [`Channel::Ours`]; a managed copy gets its
//! manager's command, `NotOurs` and `Unknown` the releases page), the build's
//! updater flag ([`crate::update::eligible`]) and whether this process is an
//! update's trial (which never offers). Every refusal names its reason
//! ([`NotEligible`]). Nothing in it touches a file: the channel was read once at
//! start and is only looked at here.
//!
//! # At most once per launch, and a suppressed offer spends nothing
//!
//! [`State::Available`] is entered at most once per launch, in the most recently
//! active ordinary window — never the summoned terminal (§7.59's rule,
//! `most_recently_active_window`). An answer that is not an offer (a skipped
//! tag, no newer release, a managed copy) leaves the gate unspent, so a newer
//! tag arriving later in the same launch still gets its card.
//!
//! # The drivers, and offers off
//!
//! A press reaches a [`Driver`] ([`driver_for_this_copy`]). On macOS it is the
//! Prepare (`update_prepare_macos`, U-27): the offer's two files downloaded
//! through the download door ([`ReleaseDownload`]), the image attached and its
//! bundle checked, copied and checked again, the rescue clone, the journal at
//! `Prepared`, each refusal a named [`Stop`]. On Windows it is the Prepare of
//! `update_prepare_windows` (U-20): the archive and its checksum document into
//! the installation's own folder, the space reserved, the archive expanded and
//! its signed files held to the running build's identity, the set copied into
//! `set\` and checked again there, the rescue copy of the running executable,
//! the inventories, the journal at `Prepared`. Elsewhere it is [`Unsupported`],
//! which refuses before any network, staging, flush or wait: the job goes
//! straight to [`State::Failed`]. The quit barrier is U-21 ([`Job::restart`], and the two answers the quit delivers
//! to [`Job::apply`] on the window thread). Whether a reader sees any of this
//! is [`Job::offers_enabled`], a build fact per platform: on for Windows since
//! U-31 and for macOS since U-32 (and off everywhere else, where no release is
//! built). With offers off the job decides and never leaves `Idle`. Either way
//! it says what it decided once per launch in `diagnostics.log`.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the drivers land after the card: U-20/U-27 drive and report, U-21 quits ((b).5)"
    )
)]

use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use bt_persist::UpdateCheckV1;
use bt_platform::HostPlatform;

use crate::install_channel::{Channel, Manager};
use crate::update::{Version, newer_than, should_offer};
use crate::update_handoff::Staged;
use crate::update_prepare::Resumer;
use crate::update_txn::{Home, Nonce, TxnId};

/// **Whether a Windows reader may be offered an update** — on since U-31, once
/// the Windows driver (U-20), the card (U-19), the quit barrier (U-21), the
/// apply and the rollback (U-23, U-24) and the clean-VM checklist (U-30) exist.
const OFFERS_ENABLED_WINDOWS: bool = true;

/// **Whether a macOS reader may be offered an update** — on since U-32, once
/// the macOS Prepare (U-27), the exchange, trial and commit (U-28), the
/// rollback and `Stuck` (U-29), the recovery of every phase (U-29b), the
/// launch pass (U-33) and the one exit guard (U-34) exist and the macOS
/// recovery contract has passed its experiments (owner ruling 2026-09-25, 2;
/// E-8's E1, the U-32 rehearsal).
const OFFERS_ENABLED_MACOS: bool = true;

/// The host the two files of an offer are fetched from (C11). GitHub
/// redirects to its asset host; the redirect rules are the download door's
/// (`bt_platform::https_download`).
pub(crate) const RELEASE_HOST: &str = "github.com";

/// Where a release's files are, by tag: `<this>/<tag>/<name>` (C11). **Never
/// `/releases/latest/download/`**: "latest" can move under an open offer.
pub(crate) const RELEASE_DOWNLOAD_PATH: &str = "/lulu-loopp/folio-terminal/releases/download";

/// **The one architecture a macOS release is built for** (`docs/RELEASING.md`:
/// `Folio-<version>-macos-arm64.dmg`): the name the image's asset carries, and
/// the slice the new bundle's main executable must have (C7: "the architecture
/// must match the offer").
pub(crate) const MACOS_ARCHITECTURE: &str = "arm64";

// ── the offer ───────────────────────────────────────────────────────────────

/// **The offer**: what a press fetches and what the transaction becomes,
/// captured once and never re-derived (§B). A later check moving `latest_tag`
/// does not touch it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Offer {
    /// The transaction this offer becomes, minted with the offer (§B). It names
    /// every progress event that may move the job ([`Progress::txn`]).
    txn: TxnId,
    /// The release's tag, verbatim (`v0.4.7-preview`).
    tag: String,
    /// The archive (Windows) or disk image (macOS), by its versioned name.
    asset: String,
    /// The checksum document beside it.
    hash_doc: String,
    /// The version the tag names (`0.4.7`): the tag without `v` and
    /// `-preview`, `docs/RELEASING.md` "The tag".
    to_version: String,
}

impl Offer {
    /// The offer for `tag` on `platform`, or `None` when the release grammar
    /// names no file for it: a tag that is not `v<version>` or
    /// `v<version>-preview`, or a platform no release is built for.
    #[must_use]
    pub(crate) fn mint(txn: TxnId, tag: &str, platform: HostPlatform) -> Option<Self> {
        let to_version = released_version(tag)?;
        let (asset, hash_doc) = match platform {
            HostPlatform::Windows => (
                format!("folio-{to_version}-windows-x64.zip"),
                "SHA256SUMS.txt".to_owned(),
            ),
            HostPlatform::MacOs => (
                format!("Folio-{to_version}-macos-{MACOS_ARCHITECTURE}.dmg"),
                "SHA256SUMS-macos.txt".to_owned(),
            ),
            HostPlatform::OtherUnix => return None,
        };
        Some(Self {
            txn,
            tag: tag.to_owned(),
            asset,
            hash_doc,
            to_version,
        })
    }

    /// The transaction this offer becomes.
    #[must_use]
    pub(crate) const fn txn(&self) -> TxnId {
        self.txn
    }

    /// The release's tag.
    #[must_use]
    pub(crate) fn tag(&self) -> &str {
        &self.tag
    }

    /// The version the offer installs.
    #[must_use]
    pub(crate) fn to_version(&self) -> &str {
        &self.to_version
    }

    /// The archive or disk image, by its versioned name.
    #[must_use]
    pub(crate) fn asset(&self) -> &str {
        &self.asset
    }

    /// **The two files a press fetches, and nothing else** (C11): the asset and
    /// its checksum document, both by the offer's own tag.
    #[must_use]
    pub(crate) fn requests(&self) -> [Request; 2] {
        [&self.asset, &self.hash_doc].map(|name| Request {
            host: RELEASE_HOST,
            path: format!("{RELEASE_DOWNLOAD_PATH}/{}/{name}", self.tag),
            tag: self.tag.clone(),
            file_name: name.clone(),
        })
    }
}

/// The version a release tag names: `v0.4.7` and `v0.4.7-preview` are `0.4.7`;
/// anything else names no release file.
fn released_version(tag: &str) -> Option<String> {
    let rest = tag.strip_prefix('v')?;
    let version = rest.strip_suffix("-preview").unwrap_or(rest);
    let well_formed = Version::parse(version).is_some()
        && version.split('.').count() == 3
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    well_formed.then(|| version.to_owned())
}

/// One file a press fetches: `https://{host}{path}` into `file_name` — or,
/// from a release feed, the file the feed lists as `file_name` of release
/// `tag` ([`FeedCopy`], U-30b).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub(crate) host: &'static str,
    pub(crate) path: String,
    /// The offer's tag, which `path` also spells.
    pub(crate) tag: String,
    pub(crate) file_name: String,
}

// ── the evidence, and eligibility ───────────────────────────────────────────

/// **What the job is still waiting for** before it may decide anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    clippy::enum_variant_names,
    reason = "the names the ticket gives the pending state (U-18: `Pending::AwaitingClassification` / `AwaitingCheck`)"
)]
pub(crate) enum Pending {
    /// Neither the channel nor the check has arrived.
    AwaitingBoth,
    /// The check has answered; how this copy was installed has not been read.
    AwaitingClassification,
    /// The channel is known; the day's check has not settled.
    AwaitingCheck,
    /// **An earlier launch's transaction is being settled** by this launch's
    /// job-owner pass ([`Job::after_start`], U-33): nothing is offered until
    /// it has swept, counted, resumed or discarded it.
    AwaitingTransaction,
}

/// **The two facts that arrive off the window thread, as far as they have**,
/// and the three that are known at start.
#[derive(Clone, Debug)]
pub(crate) struct Gathered {
    /// The check's state, once this launch's check has settled
    /// ([`crate::update::job_evidence`]).
    pub(crate) check: Option<UpdateCheckV1>,
    /// How this copy was installed, once read ([`crate::install_channel::channel`]).
    pub(crate) channel: Option<Channel>,
    /// The running build's version.
    pub(crate) running: &'static str,
    /// Whether this build was made with the updater flag ([`crate::update::eligible`]).
    pub(crate) capable: bool,
    /// Whether this process is an update's trial ([`crate::update_startup::trial`]).
    pub(crate) trial: bool,
    /// Which platform's release files an offer would name.
    pub(crate) platform: HostPlatform,
}

impl Gathered {
    /// What this process knows now — every read is of a fact already held in
    /// memory; nothing here touches a file.
    #[must_use]
    pub(crate) fn now() -> Self {
        Self {
            check: crate::update::job_evidence(),
            channel: crate::install_channel::channel(),
            running: crate::version::VERSION,
            capable: crate::update::eligible(),
            trial: crate::update_startup::trial().is_some(),
            platform: bt_platform::host_platform(),
        }
    }

    /// The whole evidence, or which half is still missing.
    ///
    /// # Errors
    /// The [`Pending`] naming what has not arrived.
    pub(crate) fn complete(self) -> Result<Evidence, Pending> {
        match (self.check, self.channel) {
            (Some(check), Some(channel)) => Ok(Evidence {
                check,
                running: self.running,
                channel,
                capable: self.capable,
                trial: self.trial,
                platform: self.platform,
            }),
            (None, None) => Err(Pending::AwaitingBoth),
            (Some(_), None) => Err(Pending::AwaitingClassification),
            (None, Some(_)) => Err(Pending::AwaitingCheck),
        }
    }
}

/// **Everything an offer is decided on**, complete.
#[derive(Clone, Debug)]
pub(crate) struct Evidence {
    /// `update-check.json` as this process holds it after the day's check.
    pub(crate) check: UpdateCheckV1,
    /// The running build's version.
    pub(crate) running: &'static str,
    /// How this copy was installed.
    pub(crate) channel: Channel,
    /// Whether this build was made with the updater flag.
    pub(crate) capable: bool,
    /// Whether this process is an update's trial.
    pub(crate) trial: bool,
    /// Which platform's release files an offer would name.
    pub(crate) platform: HostPlatform,
}

/// An offer may be made, for this tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Eligible {
    pub(crate) tag: String,
}

/// **Why no offer is made** — each variant is a reason U-19's row can name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NotEligible {
    /// This process is an update's trial: it is proving a build, not choosing one.
    Trial,
    /// Nothing newer than the running build is known.
    NoNewerRelease,
    /// The newest tag is at or below the one the reader skipped.
    Skipped { tag: String },
    /// A package manager owns this copy's updates (§D, C2): its command.
    Managed {
        manager: Manager,
        command: &'static str,
    },
    /// Unpacked by another account: the releases page (§D).
    NotOurs,
    /// How this copy was installed is not known: the releases page (§D —
    /// unknown fails safe).
    Unknown,
    /// This build was not made with the updater flag (U-8): check, mark and row
    /// unchanged (§D "not updater-capable").
    NotUpdaterBuild,
    /// The release grammar names no file for this tag on this platform.
    NoAsset { tag: String },
    /// This copy's folder cannot be written — a translocated bundle run from
    /// its read-only randomized mount, or a folder this account may not write
    /// (C7): the releases page. The macOS Prepare asks it first, on its
    /// worker, before it writes anything (U-27).
    NotWritable,
}

/// Where a reader who is not offered an update is sent (§D's behaviour column).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    /// Nothing to say: there is no newer release to go to, or the reader said no.
    Nothing,
    /// The manager's own command, for the row's **Copy** verb.
    Command(&'static str),
    /// The releases page, as the row has always offered.
    ReleasesPage,
}

impl NotEligible {
    /// Where this answer sends the reader.
    #[must_use]
    pub(crate) const fn route(&self) -> Route {
        match self {
            Self::Trial | Self::NoNewerRelease | Self::Skipped { .. } => Route::Nothing,
            Self::Managed { command, .. } => Route::Command(command),
            Self::NotOurs
            | Self::Unknown
            | Self::NotUpdaterBuild
            | Self::NoAsset { .. }
            | Self::NotWritable => Route::ReleasesPage,
        }
    }

    /// The reason, in a few words, for `diagnostics.log` (no path, no account).
    #[must_use]
    pub(crate) fn why(&self) -> String {
        match self {
            Self::Trial => "this start is an update's trial".to_owned(),
            Self::NoNewerRelease => "no newer release is known".to_owned(),
            Self::Skipped { tag } => format!("{tag} is at or below the skipped tag"),
            Self::Managed { manager, command } => {
                format!("{} owns this copy's updates (`{command}`)", manager.name())
            }
            Self::NotOurs => "this copy belongs to another account".to_owned(),
            Self::Unknown => "how this copy was installed is unknown".to_owned(),
            Self::NotUpdaterBuild => "this build was made without the updater flag".to_owned(),
            Self::NoAsset { tag } => format!("no release file is named for {tag} here"),
            Self::NotWritable => Stop::NotWritable.why().to_owned(),
        }
    }
}

/// The command that updates a copy `manager` owns (C2).
#[must_use]
pub(crate) const fn manager_command(manager: Manager) -> &'static str {
    match manager {
        Manager::Scoop => "scoop update folio",
        Manager::Homebrew => "brew upgrade --cask folio",
        Manager::Winget => "winget upgrade --id WeiyiShi.Folio --exact",
    }
}

impl Evidence {
    /// **Whether this copy may be offered an update now** — pure, and the whole
    /// of §B's "when all of" and §D's classification column.
    ///
    /// The order is the order a reason is named in: a trial first (it never
    /// offers), then whether there is anything newer, then who owns the copy,
    /// then the build, then the file. Automatic check is a schedule and is not
    /// eligibility.
    ///
    /// # Errors
    /// The first condition that does not hold.
    pub(crate) fn eligibility(&self) -> Result<Eligible, NotEligible> {
        if self.trial {
            return Err(NotEligible::Trial);
        }
        let Some(tag) = should_offer(&self.check, self.running) else {
            return Err(
                match newer_than(self.check.latest_tag.as_deref(), self.running) {
                    Some(tag) => NotEligible::Skipped {
                        tag: tag.to_owned(),
                    },
                    None => NotEligible::NoNewerRelease,
                },
            );
        };
        match self.channel {
            Channel::Ours => {}
            // A managed copy takes its manager's adapter only where that
            // road is built (managed-update §1.5); everywhere else, its row
            // keeps the manager's command.
            Channel::Managed { manager, .. } => {
                if !crate::update_adapter::built_on(
                    crate::update_adapter::of_manager(manager),
                    self.platform,
                ) {
                    return Err(NotEligible::Managed {
                        manager,
                        command: manager_command(manager),
                    });
                }
            }
            Channel::NotOurs => return Err(NotEligible::NotOurs),
            Channel::Unknown => return Err(NotEligible::Unknown),
        }
        if !self.capable {
            return Err(NotEligible::NotUpdaterBuild);
        }
        if released_version(tag).is_none() || self.platform == HostPlatform::OtherUnix {
            return Err(NotEligible::NoAsset {
                tag: tag.to_owned(),
            });
        }
        Ok(Eligible {
            tag: tag.to_owned(),
        })
    }
}

// ── the state, the verbs, and the table ─────────────────────────────────────

/// Bytes of the asset received, and the length if the server said it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Bytes {
    pub(crate) received: u64,
    pub(crate) total: Option<u64>,
}

/// Why a job failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// No driver exists for this build yet: nothing was fetched, staged or
    /// changed.
    Unsupported,
    /// The driver stopped before the files were verified, for this reason;
    /// nothing installed was changed.
    Stopped(Stop),
    /// **The new version did not prove itself and the previous one was put
    /// back** (U-29): a start sent with `--update-failed` found its
    /// transaction retired with the outcome `rolled_back`.
    RolledBack,
    /// **The update stopped before the new version ever ran, and the previous
    /// one was put back** (0.4.7 U-42a; 0.4.6's D-7): a start sent with
    /// `--update-failed` found its transaction retired `rolled_back` with no
    /// trial ever begun — a power cut during the moves (W6).
    Interrupted,
    /// **The new version did not prove itself and putting the previous one
    /// back did not finish** (U-29): the transaction is still `rolled_back`
    /// and destructive — `Stuck`, or not begun — and `folder` is where its
    /// journal is, which the card names and Show folder opens. `None` only for
    /// a report handed over from another start whose folder is not a local
    /// path (`launch_wire::accept`, U-36 round 2): the card then names no
    /// folder. `held`: this start continues with its writes held, because
    /// the rescue build could not be started (0.4.8 E1), and the card says
    /// that this session's changes are not kept. `untried`: the journal
    /// records no trial of the new build ever begun
    /// (`update_txn::Phase::trial_begun`, 0.4.8 E4) — the update stopped
    /// before the new version started, and the card says that rather than
    /// that it did not start.
    Incomplete {
        folder: Option<PathBuf>,
        held: bool,
        untried: bool,
    },
    /// **An update this build cannot read whole is not finished** (0.4.8
    /// E1): the start continued past a journal another Folio wrote —
    /// `version`, when the journal names a later build, and otherwise one
    /// whose record cannot be read. `folder` is where the journal is; `held`
    /// as for [`Failure::Incomplete`].
    Newer {
        folder: Option<PathBuf>,
        version: Option<String>,
        held: bool,
    },
    /// **This start is the unfinished transaction's trial: the new version
    /// runs here** — U-35's reserved trial, started because recovery could not
    /// be launched (0.4.7), or the trial an exit guard starts with a fresh
    /// nonce over a transaction whose new set is live (0.4.8 E4: W14long, a
    /// `Stuck` one's retrial). The update is still incomplete and `folder` is
    /// where its journal is; unlike [`Failure::Incomplete`], the card tells the
    /// reader that this session is the trial — never that the new version did
    /// not start.
    TrialIncomplete { folder: PathBuf },
    /// **Another program held the update's journal open past the applier's
    /// window for a refused write** (0.4.8 E4): `error` is the operating
    /// system's last refusal, `then` the failure the journal itself shows. The
    /// card names the hold and its error and says what `then` did; it never
    /// says that the new version did not start.
    JournalHeld { error: String, then: Box<Failure> },
    /// **The update was committed after its trial ended, and what the person
    /// changed in the trial was not kept** (0.4.8 E4, R3): the trial's writes
    /// were held until a commit it never saw, and the trial left its mark in
    /// the transaction's folder (`update_trial`). `version` is this build's.
    ChangesNotKept { version: String },
}

impl Failure {
    /// **The failure beneath a held journal's** — the one the journal itself
    /// shows ([`Failure::JournalHeld`]'s `then`), or this failure.
    pub(crate) fn beneath(&self) -> &Failure {
        match self {
            Failure::JournalHeld { then, .. } => then.beneath(),
            other => other,
        }
    }
}

/// **Why a driver stopped** (U-18 decision 11, grown by the macOS Prepare,
/// U-27): each is a reason the failed card names, and each stops before
/// anything installed moves — the transaction is abandoned, its image detached
/// and its folder removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    /// This copy cannot be written where it stands — a translocated bundle on
    /// its read-only mount, or a folder this account may not write
    /// ([`NotEligible::NotWritable`]): the releases page. Nothing was written.
    NotWritable,
    /// How this copy was installed does not let Folio update it (the channel
    /// is not [`Channel::Ours`]): nothing was written.
    NotOurs,
    /// Another transaction holds this installation: its lock is taken, or its
    /// journal is still there. Nothing was written.
    Busy,
    /// **The journal still there is one this build cannot read whole**
    /// (0.4.8 E1): another Folio's update is not finished, and the build that
    /// wrote it finishes it. Nothing was written.
    Newer,
    /// The installation home, the transaction's folders or its journal could
    /// not be written.
    Journal,
    /// A file of the offer did not arrive.
    Download,
    /// The image is not the one the checksum document names, or the document
    /// names none.
    Sums,
    /// The image could not be attached.
    Mount,
    /// The bundle on the image, or its copy, is not this publisher's Folio of
    /// the offered version for this machine's architecture.
    Identity,
    /// The bundle could not be copied off the image.
    Copy,
    /// The rescue copy of the running build could not be made.
    Clone,
    /// **The release needs a newer updater than this build** (its manifest's
    /// `min_updater`; 0.4.7 U-42c, 0.4.6's D-13): this build cannot update
    /// itself to it, and the new version is downloaded by hand. Nothing was
    /// changed.
    TooOld,
    /// The volume the installation is on has too little space for the
    /// expanded release, its staged copy and the rescue copy (Windows, §C.2
    /// step 3): this many bytes more are needed. Nothing was expanded.
    Space { short_by: u64 },
    /// The reader cancelled; the job has already moved on.
    Cancelled,
}

impl Stop {
    /// The reason, in a few words, for a log (no path, no account).
    #[must_use]
    pub(crate) const fn why(self) -> &'static str {
        match self {
            Self::NotWritable => "this copy's folder cannot be written",
            Self::NotOurs => "this copy is not updated by Folio",
            Self::Busy => "another update holds this installation",
            Self::Newer => "another Folio's unfinished update holds this installation",
            Self::Journal => "the update's folder or journal could not be written",
            Self::Download => "a file did not download",
            Self::Sums => "the image does not match its checksum",
            Self::Mount => "the image could not be attached",
            Self::Identity => "the new build is not the offered, signed Folio",
            Self::Copy => "the new build could not be copied",
            Self::Clone => "the running build could not be kept aside",
            Self::TooOld => "the release needs a newer updater than this build",
            Self::Space { .. } => "the disk has too little space for the update",
            Self::Cancelled => "the update was cancelled",
        }
    }
}

/// **The job's state** (§B).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum State {
    /// The evidence is not all in: nothing is decided.
    Pending(Pending),
    /// Decided, and no card: not eligible, gated off, answered, or cancelled.
    Idle,
    /// The card is up with this offer.
    Available(Offer),
    /// The press was taken; the driver is fetching.
    Downloading(Offer, Bytes),
    /// Both files are on disk; the driver is verifying.
    Staged(Offer),
    /// Verified and waiting for **Restart**.
    Verified(Offer),
    /// The ordinary quit is running on the update's behalf (U-21).
    Quitting(Offer),
    /// The session landed; the process is handing over and exiting.
    Committing(Offer),
    /// The job stopped, and says why: this launch's offer, or none when the
    /// failure is an earlier launch's transaction, reported by the rollback
    /// that sent this one (`--update-failed`, U-29).
    Failed(Option<Offer>, Failure),
    /// **The update this launch said was incomplete has completed** (U-32):
    /// this process is the trial a lock holder started over a `Stuck`
    /// transaction whose new build was live, its card stood at
    /// [`Failure::Incomplete`] or [`Failure::TrialIncomplete`] from the start,
    /// and the trial's watch then read `Committed` — the receipt committed it
    /// forward. The card follows the
    /// journal's final phase, not the phase at launch; the version is this
    /// build's own. Also **an update committed after its trial ended** (0.4.8
    /// E4, R3), raised at the next start: the changes made in that trial were
    /// not kept, and the card says so ([`TrialChanges::NotKept`]).
    Updated(String, TrialChanges),
}

/// **What a completed update kept of what was changed in its trial** (0.4.8
/// E4, R3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrialChanges {
    /// The trial saw its commit and wrote what it had held (U-32's road), or
    /// held nothing a person changed.
    Kept,
    /// The trial ended before its commit holding a person's change: it never
    /// reached the disk.
    NotKept,
}

/// A state's name, for the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    Pending,
    Idle,
    Available,
    Downloading,
    Staged,
    Verified,
    Quitting,
    Committing,
    Failed,
    Updated,
}

impl Kind {
    /// Every state.
    pub(crate) const ALL: [Self; 10] = [
        Self::Pending,
        Self::Idle,
        Self::Available,
        Self::Downloading,
        Self::Staged,
        Self::Verified,
        Self::Quitting,
        Self::Committing,
        Self::Failed,
        Self::Updated,
    ];
}

impl State {
    /// This state's name.
    #[must_use]
    pub(crate) const fn kind(&self) -> Kind {
        match self {
            Self::Pending(_) => Kind::Pending,
            Self::Idle => Kind::Idle,
            Self::Available(_) => Kind::Available,
            Self::Downloading(..) => Kind::Downloading,
            Self::Staged(_) => Kind::Staged,
            Self::Verified(_) => Kind::Verified,
            Self::Quitting(_) => Kind::Quitting,
            Self::Committing(_) => Kind::Committing,
            Self::Failed(..) => Kind::Failed,
            Self::Updated(..) => Kind::Updated,
        }
    }

    /// The offer this state carries, if any.
    #[must_use]
    pub(crate) const fn offer(&self) -> Option<&Offer> {
        match self {
            Self::Pending(_) | Self::Idle | Self::Updated(..) => None,
            Self::Available(offer)
            | Self::Downloading(offer, _)
            | Self::Staged(offer)
            | Self::Verified(offer)
            | Self::Quitting(offer)
            | Self::Committing(offer) => Some(offer),
            Self::Failed(offer, _) => offer.as_ref(),
        }
    }
}

/// **A card verb** (§B, C9). Escape and the close box are **Later**; C9's
/// **Close** on a failed card is **Later** too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Verb {
    Later,
    Skip,
    /// **Update** (C9) — the press.
    Press,
    Cancel,
    Restart,
}

impl Verb {
    /// Every verb.
    pub(crate) const ALL: [Self; 5] = [
        Self::Later,
        Self::Skip,
        Self::Press,
        Self::Cancel,
        Self::Restart,
    ];
}

/// **Why a verb does nothing in a state** — every one is listed in [`TABLE`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// There is no card in this state, so no verb can have been pressed on one.
    NoCard,
    /// The card in this state does not carry this verb (C9's verbs column).
    NotOnThisCard,
    /// The ordinary quit owns the window now; its own card answers (§B `Quitting`).
    TheQuitAnswers,
    /// The process is handing over and exiting.
    Exiting,
}

/// What a verb does in a state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Handling {
    /// The job moves to this state.
    Moves(Kind),
    /// The job is handed to the driver: [`Kind::Downloading`] if it starts,
    /// [`Kind::Failed`] if it refuses.
    Prepares,
    /// The verb is refused, for this reason.
    Refused(Refusal),
}

/// **The state × verb table** (§B: "there is no reachable state in which a card
/// verb exists with no handler"). One row per pair, 10 × 5;
/// `every_card_state_has_a_handler_for_every_verb` holds [`Job::answer`] to it.
pub(crate) const TABLE: [(Kind, Verb, Handling); 50] = {
    use Handling::{Moves, Prepares, Refused};
    use Kind as K;
    use Refusal::{Exiting, NoCard, NotOnThisCard, TheQuitAnswers};
    use Verb as V;
    [
        (K::Pending, V::Later, Refused(NoCard)),
        (K::Pending, V::Skip, Refused(NoCard)),
        (K::Pending, V::Press, Refused(NoCard)),
        (K::Pending, V::Cancel, Refused(NoCard)),
        (K::Pending, V::Restart, Refused(NoCard)),
        (K::Idle, V::Later, Refused(NoCard)),
        (K::Idle, V::Skip, Refused(NoCard)),
        (K::Idle, V::Press, Refused(NoCard)),
        (K::Idle, V::Cancel, Refused(NoCard)),
        (K::Idle, V::Restart, Refused(NoCard)),
        // Update · Later · Skip.
        (K::Available, V::Later, Moves(K::Idle)),
        (K::Available, V::Skip, Moves(K::Idle)),
        (K::Available, V::Press, Prepares),
        (K::Available, V::Cancel, Refused(NotOnThisCard)),
        (K::Available, V::Restart, Refused(NotOnThisCard)),
        // Cancel. Escape and the close box (Later) put the card away and the
        // download goes on (coordinator ruling 1, 2026-09-26, U-19).
        (K::Downloading, V::Later, Moves(K::Downloading)),
        (K::Downloading, V::Skip, Refused(NotOnThisCard)),
        (K::Downloading, V::Press, Refused(NotOnThisCard)),
        (K::Downloading, V::Cancel, Moves(K::Idle)),
        (K::Downloading, V::Restart, Refused(NotOnThisCard)),
        // Still the download's card: Cancel; Later puts it away, as above.
        (K::Staged, V::Later, Moves(K::Staged)),
        (K::Staged, V::Skip, Refused(NotOnThisCard)),
        (K::Staged, V::Press, Refused(NotOnThisCard)),
        (K::Staged, V::Cancel, Moves(K::Idle)),
        (K::Staged, V::Restart, Refused(NotOnThisCard)),
        // Restart · Later — Later keeps the staged transaction (§B).
        (K::Verified, V::Later, Moves(K::Verified)),
        (K::Verified, V::Skip, Refused(NotOnThisCard)),
        (K::Verified, V::Press, Refused(NotOnThisCard)),
        (K::Verified, V::Cancel, Refused(NotOnThisCard)),
        (K::Verified, V::Restart, Moves(K::Quitting)),
        (K::Quitting, V::Later, Refused(TheQuitAnswers)),
        (K::Quitting, V::Skip, Refused(TheQuitAnswers)),
        (K::Quitting, V::Press, Refused(TheQuitAnswers)),
        (K::Quitting, V::Cancel, Refused(TheQuitAnswers)),
        (K::Quitting, V::Restart, Refused(TheQuitAnswers)),
        (K::Committing, V::Later, Refused(Exiting)),
        (K::Committing, V::Skip, Refused(Exiting)),
        (K::Committing, V::Press, Refused(Exiting)),
        (K::Committing, V::Cancel, Refused(Exiting)),
        (K::Committing, V::Restart, Refused(Exiting)),
        // Releases · Close — Releases opens a page and moves nothing; Close is Later.
        (K::Failed, V::Later, Moves(K::Idle)),
        (K::Failed, V::Skip, Refused(NotOnThisCard)),
        (K::Failed, V::Press, Refused(NotOnThisCard)),
        (K::Failed, V::Cancel, Refused(NotOnThisCard)),
        (K::Failed, V::Restart, Refused(NotOnThisCard)),
        // Close — Close is Later (U-32).
        (K::Updated, V::Later, Moves(K::Idle)),
        (K::Updated, V::Skip, Refused(NotOnThisCard)),
        (K::Updated, V::Press, Refused(NotOnThisCard)),
        (K::Updated, V::Cancel, Refused(NotOnThisCard)),
        (K::Updated, V::Restart, Refused(NotOnThisCard)),
    ]
};

/// What a verb asks of somebody other than the job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    /// Nothing.
    None,
    /// **Skip** — write `skipped_tag` and `seen_tag` for this tag through the
    /// check's one owner (`update::OfferState::skip`, U-6).
    RecordSkip(String),
}

// ── the driver seam ─────────────────────────────────────────────────────────

/// **How a driver fetches a file** — on the driver's own worker, so it is
/// shared with that thread ([`SharedTransport`]). The real one is the download
/// door ([`ReleaseDownload`], `bt_platform::http::https_download`, wired by the
/// first driver that fetches, U-27); a test's is a stand-in that writes the
/// file it is asked for.
pub(crate) trait Transport: Send + Sync {
    /// Fetch one file into the folder `into`, which exists; answer where it
    /// is. Bytes are reported through `fetching` as they arrive, and the fetch
    /// stops when `fetching` says the job was cancelled.
    ///
    /// # Errors
    /// The door's refusal, as a sentence; nothing of the file is left.
    fn fetch(
        &self,
        request: &Request,
        into: &std::path::Path,
        fetching: &Fetching,
    ) -> Result<std::path::PathBuf, String>;
}

/// A transport, as the job hands it to a driver that takes it to its worker.
pub(crate) type SharedTransport = Arc<dyn Transport>;

/// **What a fetch reports to, and asks**: the bytes of the file so far, and
/// whether the job the fetch is for was cancelled.
pub(crate) struct Fetching {
    pub(crate) report: Arc<dyn Fn(Bytes) + Send + Sync>,
    pub(crate) cancelled: Arc<AtomicBool>,
}

/// **The download door, for a release file** (C11): one `GET` of
/// `https://{host}{path}` through `bt_platform::http::https_download`, into
/// `into` under the file's own name, under the door's own ceiling and
/// deadlines, with the check's `User-Agent`. Its monitor's wake reports the
/// bytes so far and passes a cancelled job's word on to the door, which stops
/// within its cancel latency and removes what it wrote.
pub(crate) struct ReleaseDownload;

impl Transport for ReleaseDownload {
    fn fetch(
        &self,
        request: &Request,
        into: &std::path::Path,
        fetching: &Fetching,
    ) -> Result<std::path::PathBuf, String> {
        use bt_platform::https_download::{
            DOWNLOAD_CEILING_LIMIT, DOWNLOAD_FLOOR_BYTES, DOWNLOAD_IDLE_TIMEOUT, DownloadMonitor,
            HttpsDownload,
        };
        let report = Arc::clone(&fetching.report);
        let cancelled = Arc::clone(&fetching.cancelled);
        let monitor = Arc::new_cyclic(|me: &std::sync::Weak<DownloadMonitor>| {
            let me = me.clone();
            DownloadMonitor::new(move || {
                if let Some(monitor) = me.upgrade() {
                    if cancelled.load(Ordering::SeqCst) {
                        monitor.cancel();
                    }
                    let seen = monitor.take();
                    report(Bytes {
                        received: seen.received,
                        total: seen.expected,
                    });
                }
            })
        });
        bt_platform::http::https_download(&HttpsDownload {
            host: request.host,
            path: &request.path,
            user_agent: crate::update::USER_AGENT,
            directory: into,
            file_name: &request.file_name,
            ceiling: DOWNLOAD_CEILING_LIMIT,
            idle_timeout: DOWNLOAD_IDLE_TIMEOUT,
            floor: DOWNLOAD_FLOOR_BYTES,
            monitor: &monitor,
        })
        .map(|downloaded| downloaded.path)
        .map_err(|error| error.to_string())
    }
}

/// **A release feed's file, copied** (U-30b): the file the feed
/// (`update::Feed`, `--update-feed`) lists under the request's tag and name,
/// read through `file_reads` on [`bt_platform::file_reads::Lane::Update`] and
/// copied durably into `into` under the file's own name — in place of
/// [`ReleaseDownload`], and nothing else changes: the driver holds what it
/// copied to the checksum document and to the running build's signer exactly
/// as it holds a download. Bytes are reported as they are copied, and a
/// cancelled job stops the copy and removes what it wrote.
pub(crate) struct FeedCopy(crate::update::Feed);

impl Transport for FeedCopy {
    fn fetch(
        &self,
        request: &Request,
        into: &std::path::Path,
        fetching: &Fetching,
    ) -> Result<std::path::PathBuf, String> {
        let (source, size) = self.0.asset(&request.tag, &request.file_name)?;
        let target = into.join(&request.file_name);
        copy_feed_file(&source, &target, size, fetching).map(|()| target)
    }
}

/// [`FeedCopy`]'s copy of `source` to `target`: the durable-copy door
/// (`install_txn::durable_copy` — a temporary beside the target, flushed,
/// renamed, nothing left under the name on a failure), fed by [`FeedBytes`].
fn copy_feed_file(
    source: &std::path::Path,
    target: &std::path::Path,
    size: u64,
    fetching: &Fetching,
) -> Result<(), String> {
    let from = bt_platform::file_reads::open(bt_platform::file_reads::Lane::Update, source)
        .map_err(|error| format!("{}: {error}", source.display()))?;
    let mut bytes = FeedBytes {
        from,
        received: 0,
        size,
        fetching,
    };
    bt_platform::install_txn::durable_copy(&mut bytes, target)
        .map(drop)
        .map_err(|failure| failure.to_string())
}

/// **A feed's file as the copy reads it**: the bytes so far reported as they
/// pass, and a cancelled job's word turned into a refused read, which ends the
/// copy with nothing left.
struct FeedBytes<'a> {
    from: bt_platform::file_reads::Reader<'a, std::fs::File>,
    received: u64,
    size: u64,
    fetching: &'a Fetching,
}

impl Read for FeedBytes<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.fetching.cancelled.load(Ordering::SeqCst) {
            return Err(std::io::Error::other("cancelled"));
        }
        let read = feed_chunk(&mut self.from, buffer)?;
        self.received += read as u64;
        (self.fetching.report)(Bytes {
            received: self.received,
            total: Some(self.size),
        });
        Ok(read)
    }
}

/// One chunk of a feed's file, through the `file_reads` reader it was opened
/// with.
fn feed_chunk(
    from: &mut bt_platform::file_reads::Reader<'_, std::fs::File>,
    buffer: &mut [u8],
) -> std::io::Result<usize> {
    from.read(buffer)
}

/// **The transport a press fetches with**: the feed's copy when this process
/// was given a release feed (U-30b), `page` — the download door — otherwise.
/// Never both: a feed that cannot deliver a file is a failed Prepare, not a
/// reason to ask the network.
pub(crate) fn transport_for(
    feed: Option<&crate::update::Feed>,
    page: SharedTransport,
) -> SharedTransport {
    match feed {
        Some(feed) => Arc::new(FeedCopy(feed.clone())),
        None => page,
    }
}

/// Why a driver would not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// This build has no driver.
    Unsupported,
    /// The driver's worker could not be started (the system refused a
    /// thread): nothing was fetched, staged or changed.
    NoWorker,
}

/// **What a press hands the offer to** — Prepare: fetch, stage, verify
/// (U-20 Windows, U-27 macOS). A driver that starts reports through the
/// [`Poster`] it is given, off the window thread.
pub(crate) trait Driver {
    /// Start Prepare for `offer`.
    ///
    /// # Errors
    /// The driver will not start; nothing was fetched, staged or changed.
    fn prepare(
        &self,
        offer: &Offer,
        transport: &SharedTransport,
        post: &Poster,
    ) -> Result<(), Refused>;
}

/// **The driver of a build or a copy that has none**: it refuses before any
/// network, staging, flush or wait — it does not look at its arguments at all.
/// A platform with no release, a macOS process that is not running from a
/// bundle (`update_prepare_macos::MacPrepare::of_this_copy`), and a Windows
/// process that cannot name its own executable
/// (`update_prepare_windows::WinPrepare::of_this_copy`).
pub(crate) struct Unsupported;

impl Driver for Unsupported {
    fn prepare(&self, _: &Offer, _: &SharedTransport, _: &Poster) -> Result<(), Refused> {
        Err(Refused::Unsupported)
    }
}

/// **A transport that fetches nothing** — for the verbs and the tests whose
/// driver never asks for a file; it says so if anything ever did.
pub(crate) struct NoDownloadDoor;

impl Transport for NoDownloadDoor {
    fn fetch(
        &self,
        request: &Request,
        _into: &std::path::Path,
        _fetching: &Fetching,
    ) -> Result<std::path::PathBuf, String> {
        Err(format!(
            "no download door is wired here ({})",
            request.file_name
        ))
    }
}

/// **The driver and the transport a press is handed on this copy**: the
/// Prepare of the running bundle on macOS (U-27) and of the running install
/// folder on Windows (U-20), each with the download door — or with the
/// release feed's copy when this process was given one ([`transport_for`],
/// U-30b); [`Unsupported`] everywhere else.
pub(crate) fn driver_for_this_copy() -> (Box<dyn Driver>, SharedTransport) {
    let transport = || transport_for(crate::update::feed(), Arc::new(ReleaseDownload));
    match bt_platform::host_platform() {
        HostPlatform::MacOs => (
            crate::update_prepare_macos::MacPrepare::of_this_copy(),
            transport(),
        ),
        HostPlatform::Windows => (
            crate::update_prepare_windows::WinPrepare::of_this_copy(),
            transport(),
        ),
        HostPlatform::OtherUnix => (Box::new(Unsupported), Arc::new(NoDownloadDoor)),
    }
}

/// **How this copy revalidates a staged transaction a later launch finds**
/// (U-33): the running install folder's on Windows
/// (`update_prepare_windows::resumer`, under the system's trust), the running
/// bundle's on macOS (`update_prepare_macos::resumer_of_this_copy`), and
/// nothing anywhere else (`update_prepare::no_resume`, which discards).
#[must_use]
pub(crate) fn resumer_for_this_copy() -> Resumer {
    match bt_platform::host_platform() {
        HostPlatform::Windows => match std::env::current_exe() {
            Ok(exe) => {
                crate::update_prepare_windows::resumer(exe, bt_platform::trust::Policy::System)
            }
            Err(_) => crate::update_prepare::no_resume(),
        },
        HostPlatform::MacOs => crate::update_prepare_macos::resumer_of_this_copy(),
        HostPlatform::OtherUnix => crate::update_prepare::no_resume(),
    }
}

// ── an earlier launch's transaction ─────────────────────────────────────────

/// **What this launch's job-owner pass decided** (U-33) —
/// `update_prepare::settle_at_launch`'s answer, carried from the job's worker
/// to the window thread.
pub(crate) enum Landed {
    /// Nothing is left of an earlier launch's transaction that is this
    /// launch's to show: nothing waited, or it was swept, counted with offers
    /// off, or discarded. The launch checks and offers as usual.
    Ordinary,
    /// Another holder has the transaction lock: nothing was touched, and this
    /// launch offers nothing.
    Busy,
    /// **A staged set passed revalidation**: the offer minted again from the
    /// set's own version under the transaction's identity, and the staged
    /// transaction, lock held — the verified card, as if its download had just
    /// finished.
    Resumed(Offer, Box<Staged>),
}

/// Where this launch's job-owner pass is.
enum Launch {
    /// None was due at this launch, or it has landed and been read.
    Done,
    /// The start left a transaction for the job owner; the pass starts once
    /// how this copy was installed is known (the revalidation holds the copy
    /// to it, as a press does).
    Due {
        home: Home,
        resume: Resumer,
        check: Box<dyn FnOnce() + Send>,
    },
    /// The pass runs on the job's worker and leaves its answer here.
    Running {
        landed: Arc<Mutex<Option<Landed>>>,
        check: Box<dyn FnOnce() + Send>,
    },
}

/// What [`Job::consider`] does after asking the launch pass.
enum Pass {
    /// The pass has not landed: nothing is decided.
    Waiting,
    /// The pass decided this launch (a resumed card, or `Busy`): the line to
    /// say, the first time.
    Decided(Option<String>),
    /// The launch is an ordinary one: consider the offer as usual.
    Ordinary,
}

// ── progress, and the stale-event rule ─────────────────────────────────────

/// **Why the quit gave the update up** (0.4.6 U-21, §C.3 and R-4): the reason
/// the verified card names when it comes back. The card's words are U-19's;
/// this is the fact they are drawn from, and [`Abandon::why`] is the line
/// `diagnostics.log` gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Abandon {
    /// The reader answered the quit's card with Cancel.
    Cancelled,
    /// **Save** was chosen and not everything it named reached the disk.
    SaveIncomplete,
    /// The session document was refused by the disk.
    SessionRefused,
    /// The session's receipt did not come back inside
    /// [`crate::quit::UPDATE_RECEIPT_DEADLINE`]. The quit went on; the update
    /// did not, because nobody saw the document land.
    SessionTimedOut,
}

impl Abandon {
    /// The reason, as the one line `diagnostics.log` gets.
    #[must_use]
    pub(crate) const fn why(self) -> &'static str {
        match self {
            Self::Cancelled => "the quit was cancelled",
            Self::SaveIncomplete => "not every unsaved file could be saved",
            Self::SessionRefused => "the session could not be written",
            Self::SessionTimedOut => "the session write did not finish in time",
        }
    }
}

/// What a driver (or the quit barrier) reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// More of the asset arrived.
    Received(Bytes),
    /// Both files are on disk.
    Staged,
    /// Both files passed verification.
    Verified,
    /// The driver stopped, for this reason; nothing installed was changed.
    Stopped(Stop),
    /// The quit gave the update up, and why: back to `Verified` (§B).
    QuitAbandoned(Abandon),
    /// The session landed: the process hands over (§B `Committing`).
    SessionLanded,
}

/// One report, **named by its transaction**.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Progress {
    pub(crate) txn: TxnId,
    pub(crate) step: Step,
}

/// Whether a report moved the job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Applied {
    Moved,
    /// Dropped: another transaction's, or one this job no longer runs (a
    /// cancelled or finished job is never revived).
    Stale,
}

/// How the loop is woken for progress — [`install_progress_wake`].
static PROGRESS_WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// How the loop is woken when one of the job's two facts lands — [`install_wake`].
static EVIDENCE_WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Install the wake a driver's report asks for, once per process
/// (`AppEvent::UpdateJobProgress`).
pub(crate) fn install_progress_wake(wake: impl Fn() + Send + Sync + 'static) {
    let _ = PROGRESS_WAKE.set(Box::new(wake));
}

/// Install the wake the job's evidence asks for, once per process
/// (`AppEvent::UpdateJobOffer`).
pub(crate) fn install_wake(wake: impl Fn() + Send + Sync + 'static) {
    let _ = EVIDENCE_WAKE.set(Box::new(wake));
}

/// **One of the job's two facts has landed**: ask the loop to consider an
/// offer. Called by the check when it settles (`update::begin`).
pub(crate) fn evidence_landed() {
    if let Some(wake) = EVIDENCE_WAKE.get() {
        wake();
    }
}

/// Where a driver leaves the staged transaction for the job, until the job
/// reads its `Verified` report: the transaction it belongs to, and the value.
type StagedSlot = Arc<Mutex<Option<(TxnId, Staged)>>>;

/// **A driver's way back to the job**: reports carry the transaction they
/// belong to, wait in the job's inbox, and wake the loop.
#[derive(Clone)]
pub(crate) struct Poster {
    txn: TxnId,
    inbox: Arc<Mutex<Vec<Progress>>>,
    staged: StagedSlot,
    /// Set by the job when the reader cancels this transaction's download: the
    /// driver stops at its next step.
    cancelled: Arc<AtomicBool>,
}

impl Poster {
    /// **Report `Verified`, handing the job what Prepare leaves behind** —
    /// the home, the journal at `Prepared` and the transaction lock (U-21's
    /// seam for U-20 / U-27): the quit's hand-over writes `Handoff` from it,
    /// under that lock, at the way out.
    ///
    /// # Errors
    /// The job was cancelled first: the staged transaction comes back, for the
    /// driver to abandon — a job that has moved on never holds it.
    pub(crate) fn verified(&self, staged: Staged) -> Result<(), Box<Staged>> {
        let mut slot = self
            .staged
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.cancelled() {
            return Err(Box::new(staged));
        }
        *slot = Some((self.txn, staged));
        drop(slot);
        self.post(Step::Verified);
        Ok(())
    }

    /// Whether the job has cancelled this poster's transaction.
    #[must_use]
    pub(crate) fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// The flag [`Self::cancelled`] reads, for a fetch to pass on to its door.
    #[must_use]
    pub(crate) fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }

    /// Report `step` for this poster's transaction.
    pub(crate) fn post(&self, step: Step) {
        self.inbox
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Progress {
                txn: self.txn,
                step,
            });
        if let Some(wake) = PROGRESS_WAKE.get() {
            wake();
        }
    }
}

/// A fresh transaction identity: 128 bits nobody can guess
/// (`bt_platform::attention_pipe::unguessable_bits`).
#[must_use]
pub(crate) fn mint_txn() -> TxnId {
    TxnId::new(bt_platform::attention_pipe::unguessable_bits().to_le_bytes())
}

/// **The applier's nonce** O records in `Handoff` and hands P (U-21): 256
/// bits nobody can guess, two draws of the same source.
#[must_use]
pub(crate) fn mint_nonce() -> Nonce {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(&bt_platform::attention_pipe::unguessable_bits().to_le_bytes());
    bytes[16..].copy_from_slice(&bt_platform::attention_pipe::unguessable_bits().to_le_bytes());
    Nonce::new(bytes)
}

// ── the job ─────────────────────────────────────────────────────────────────

/// The windows an offer can be raised in: [`crate::most_recently_active_window`]'s
/// three lists.
pub(crate) struct Presenters<'a, W> {
    /// The windows of this run, oldest visit first (`App::activated`).
    pub(crate) visited: &'a [W],
    /// The windows on the screen, in the order they opened.
    pub(crate) open: &'a [W],
    /// The summoned terminal, if this run has one.
    pub(crate) quake: Option<W>,
}

/// **The update job** — one per process, on `App` (§B).
pub(crate) struct Job<W> {
    state: State,
    /// Whether `Available` has been entered this launch (at most once).
    offered_this_launch: bool,
    /// The window the card is raised in. Kept while the card is put away, so
    /// the card comes back where it went (coordinator ruling 1, U-19).
    presenter: Option<W>,
    /// **The reader put the card away and the job went on** — Later (Escape,
    /// the close box) on the download's card or on a verified one. The card
    /// is not drawn until the download ends (`Verified` or `Failed`, once) or
    /// About → Version's control asks for it again ([`Self::reopen`]).
    put_away: bool,
    /// The last decision, for U-19's row.
    answer: Option<Result<Eligible, NotEligible>>,
    /// The platform whose release grammar the last decision used. Kept beside
    /// `answer` so an asked offer can mint exactly the offer that decision
    /// permits without re-running eligibility.
    offer_platform: Option<HostPlatform>,
    /// The last failure of this launch (R3). Closing its card moves the state
    /// to `Idle`; About → Version and the gear's mark keep naming it until a
    /// new attempt's download starts, or the update it named completes
    /// ([`Self::after_commit`]).
    last_failure: Option<(Option<Offer>, Failure)>,
    /// Whether the decision has been written to `diagnostics.log`.
    said: bool,
    /// [`Job::offers_enabled`] in the product.
    offers: bool,
    /// Reports from a driver, waiting for the window thread.
    inbox: Arc<Mutex<Vec<Progress>>>,
    /// What a driver left with its `Verified` report, until the job takes it.
    staged_slot: StagedSlot,
    /// **The staged transaction** — its home, its journal and the lock O holds
    /// on it — from `Verified` until the process leaves (U-21).
    staged: Option<Staged>,
    /// Why the last quit gave the update up, while the card is back at
    /// `Verified` (U-21; U-19 names it).
    abandoned: Option<Abandon>,
    /// The cancel flag of the driver working for this job's offer, from the
    /// press until the download ends ([`Poster::cancelled`]).
    running: Option<Arc<AtomicBool>>,
    /// **This launch's job-owner pass** over the transaction an earlier
    /// launch left ([`Self::after_start`], U-33).
    launch: Launch,
    /// **This launch was sent to say an update is incomplete** — its card
    /// started at [`Failure::Incomplete`] or [`Failure::TrialIncomplete`]
    /// ([`Self::after_rollback`]). Read by [`Self::after_commit`] (U-32,
    /// U-35).
    said_incomplete: bool,
    /// **The restart a press already asked for** (N10, the owner's ruling of
    /// 2026-10-06): About → Version's *Update and restart* names its
    /// transaction here, and when that transaction reaches `Verified` the
    /// application restarts as the Ready card's Restart would
    /// ([`Self::take_asked_restart`]). Held only while the job is downloading,
    /// verifying or verified for that transaction; any other state —
    /// a failure, a cancel, another offer — lets it go
    /// ([`Self::keep_the_asked_restart_in_its_transaction`]).
    restart_asked: Option<TxnId>,
}

impl<W: Copy + Eq> Default for Job<W> {
    fn default() -> Self {
        Self::for_platform(bt_platform::host_platform())
    }
}

impl<W: Copy + Eq> Job<W> {
    /// **Whether offers reach a reader of this build** — the gate of the
    /// platform this build is for ([`Self::offers_enabled_on`]).
    #[must_use]
    pub(crate) const fn offers_enabled() -> bool {
        Self::offers_enabled_on(bt_platform::host_platform())
    }

    /// **Whether offers reach a reader on `platform`**: Windows since U-31,
    /// macOS once U-32 turns its gate on, and nowhere else (no release is
    /// built there). A build fact — no setting, no variable, no flag moves it.
    /// Pure and taking the platform as a value, so that a test on one platform
    /// reads the other's gate.
    #[must_use]
    pub(crate) const fn offers_enabled_on(platform: HostPlatform) -> bool {
        match platform {
            HostPlatform::Windows => OFFERS_ENABLED_WINDOWS,
            HostPlatform::MacOs => OFFERS_ENABLED_MACOS,
            HostPlatform::OtherUnix => false,
        }
    }

    /// **The job a build for `platform` holds**: its gate is that platform's
    /// ([`Self::offers_enabled_on`]). The application's is the host's
    /// (`Job::default`).
    #[must_use]
    pub(crate) fn for_platform(platform: HostPlatform) -> Self {
        Self::with_offers(Self::offers_enabled_on(platform))
    }

    /// A job whose gate is `offers`; the product's is [`Self::offers_enabled`].
    #[must_use]
    pub(crate) fn with_offers(offers: bool) -> Self {
        Self {
            state: State::Pending(Pending::AwaitingBoth),
            offered_this_launch: false,
            presenter: None,
            put_away: false,
            answer: None,
            offer_platform: None,
            last_failure: None,
            said: false,
            offers,
            inbox: Arc::new(Mutex::new(Vec::new())),
            staged_slot: Arc::new(Mutex::new(None)),
            staged: None,
            abandoned: None,
            running: None,
            launch: Launch::Done,
            said_incomplete: false,
            restart_asked: None,
        }
    }

    /// **The download is given up** (Cancel): the driver is told to stop at
    /// its next step, and a staged transaction it may already have left is let
    /// go — its lock released, its journal left at `Prepared` for a later
    /// launch to count or resume ((b).1 F-17). A job that has moved on holds nothing of it.
    fn stop_the_driver(&mut self) {
        if let Some(flag) = self.running.take() {
            flag.store(true, Ordering::SeqCst);
        }
        let left = self
            .staged_slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(left);
    }

    /// **A launch that a rollback sent** (`--update-failed`, U-29;
    /// `update_startup::failed`): the card stands at `Failed` from the start
    /// with `failure` and no offer — the transaction was an earlier launch's —
    /// and this launch raises no offer by itself (its one unasked offer is
    /// spent). The reader may still ask from About → Version once the check
    /// lands ([`Self::offer_again`]): Update and restart or Retry, a new
    /// transaction; while an incomplete update may still be committed forward
    /// ([`Self::asked_offer`]) nothing can be asked. `None` is every other
    /// launch.
    #[must_use]
    pub(crate) fn after_rollback(mut self, failure: Option<Failure>) -> Self {
        if let Some(failure) = failure {
            self.told(failure);
        }
        self
    }

    /// **The card stands at `Failed` with an earlier transaction's
    /// `failure`**, and this launch raises no offer by itself — what a start a
    /// rollback sent is told, whether at its own start ([`Self::after_rollback`])
    /// or handed to this one ([`Self::told_by_a_launch`]). Being told an update
    /// is incomplete is kept until a commit says otherwise
    /// ([`Self::after_commit`]); a later report does not take it back.
    fn told(&mut self, failure: Failure) {
        // Not a failure of the update: it completed, and the card says what
        // its trial did not keep (0.4.8 E4, R3). Nothing to name in About.
        if let Failure::ChangesNotKept { version } = failure {
            self.state = State::Updated(version, TrialChanges::NotKept);
            self.offered_this_launch = true;
            return;
        }
        self.said_incomplete |= matches!(
            failure.beneath(),
            Failure::Incomplete { .. } | Failure::TrialIncomplete { .. } | Failure::Newer { .. }
        );
        self.last_failure = Some((None, failure.clone()));
        self.state = State::Failed(None, failure);
        self.offered_this_launch = true;
    }

    /// **A start a rollback sent handed itself to this running Folio**
    /// (`launch_wire`, U-36): its report reaches this job where the launch
    /// landed — `landed`, the window it opened or opened a tab in, or `None`
    /// when no window could be opened ([`Self::hand_over`] then seats the card
    /// in the next ordinary window). Answers whether the card was raised.
    ///
    /// **It is the card that start would have shown cold**, and it is that
    /// start's one report: raised once, in the window the reader is looking at
    /// now, over whatever this launch shows — no card, an offer nobody has
    /// pressed (its tag stays askable from About → Version), or an earlier
    /// failure, closed or not (the newest report wins). **A transaction this
    /// launch is running is not disturbed** (RULES §36): from the press to the
    /// quit, and while the launch pass settles an earlier launch's transaction,
    /// the report is kept as the launch's last failure, which About → Version
    /// names with Details once the job is back at `Idle`, and no card is
    /// raised over the transaction's.
    pub(crate) fn told_by_a_launch(&mut self, failure: Failure, landed: Option<W>) -> bool {
        let running = matches!(
            self.state,
            State::Downloading(..)
                | State::Staged(_)
                | State::Verified(_)
                | State::Quitting(_)
                | State::Committing(_)
        ) || !matches!(self.launch, Launch::Done);
        if running {
            // Kept, not queued: if this transaction then fails, its own failure
            // replaces the report here (`Self::apply`), and the report is no
            // longer named anywhere. The rule RULES §36 states.
            self.last_failure = Some((None, failure));
            return false;
        }
        self.told(failure);
        self.presenter = landed;
        true
    }

    /// **This process's trial was committed** — the trial's watch read
    /// `Committed` (`update_trial`, `AppEvent::TrialWritesReleased`), and
    /// `version` is this build's (U-32, the coordinator's ruling 2).
    ///
    /// A launch sent to say the update was incomplete — the trial a lock
    /// holder starts over a `Stuck` transaction whose new build is live
    /// carries `--update-failed` (U-29b) — can still be committed forward by
    /// its own receipt. Its card then follows the journal's final phase:
    /// *Update incomplete.* becomes [`State::Updated`], and a card the reader
    /// already closed comes back once to say so ([`Self::hand_over`] seats
    /// it). Every other launch is unchanged: a trial that was never told
    /// anything is not told this either. Answers whether the card changed.
    pub(crate) fn after_commit(&mut self, version: &str) -> bool {
        let standing = match &self.state {
            State::Failed(None, failure) => matches!(
                failure.beneath(),
                Failure::Incomplete { .. }
                    | Failure::TrialIncomplete { .. }
                    | Failure::Newer { .. }
            ),
            State::Idle => true,
            _ => false,
        };
        if !(self.said_incomplete && standing) {
            return false;
        }
        self.said_incomplete = false;
        self.last_failure = None;
        self.state = State::Updated(version.to_owned(), TrialChanges::Kept);
        true
    }

    /// **What the start left for this launch's job owner** (U-33; (b).1 F-17
    /// and (b).2's W1–W2, M1–M2): `waiting` is the home whose `preparing` /
    /// `deferred` transaction the start continued past
    /// (`update_startup::waiting`), `resume` how this copy revalidates a
    /// staged set ([`resumer_for_this_copy`]) and `check` the start of the
    /// day's update check (`update::begin`).
    ///
    /// With nothing waiting, `check` runs now, as every start has. Otherwise
    /// the job owner's pass (`update_prepare::settle_at_launch`) runs **once,
    /// on the job's worker** (`bt-update-job`), as soon as the channel is
    /// known, and **before any offer**: the job stays
    /// [`Pending::AwaitingTransaction`] until it lands, and [`Self::consider`]
    /// reads its answer on the window thread — `Swept`, a discard, anything
    /// that leaves no staged set → an ordinary launch; `Busy` → no offer this
    /// launch; a counted set that revalidates → `Verified` with the offer
    /// rebuilt from the set's own version, in the most recently active
    /// ordinary window, exactly as if the download had just finished.
    ///
    /// **The day's check and a resumed set** (the 24-hour rule): `check` runs
    /// only once the pass has landed, and **not at all when the pass resumed a
    /// staged set** — that launch's card is the staged version's, and a check
    /// run beside it would offer the same release a second time. A launch
    /// whose pass swept or discarded, or found the lock held, checks as usual
    /// (a check that is not due asks nothing, `update::due`). A kernel that
    /// will not give the pass a thread leaves the transaction for the next
    /// launch and makes this one ordinary.
    #[must_use]
    pub(crate) fn after_start(
        mut self,
        waiting: Option<Home>,
        resume: Resumer,
        check: impl FnOnce() + Send + 'static,
    ) -> Self {
        match waiting {
            None => check(),
            Some(home) => {
                self.launch = Launch::Due {
                    home,
                    resume,
                    check: Box::new(check),
                };
            }
        }
        self
    }

    /// **The launch pass, asked on the window thread** by [`Self::consider`]:
    /// started once the channel is known, read once it has landed.
    fn launch_pass(&mut self, gathered: &Gathered, presenters: &Presenters<'_, W>) -> Pass {
        let (landed, check) = match std::mem::replace(&mut self.launch, Launch::Done) {
            Launch::Done => return Pass::Ordinary,
            Launch::Due {
                home,
                resume,
                check,
            } => {
                let Some(channel) = gathered.channel else {
                    self.launch = Launch::Due {
                        home,
                        resume,
                        check,
                    };
                    self.state = State::Pending(Pending::AwaitingTransaction);
                    return Pass::Waiting;
                };
                let landed = Arc::new(Mutex::new(None));
                let (slot, offers, platform) =
                    (Arc::clone(&landed), self.offers, gathered.platform);
                let spawned = bt_platform::spawn_at_priority(
                    crate::update_prepare::WORKER,
                    bt_platform::ThreadPriority::BelowNormal,
                    move |worker| {
                        let answer = crate::update_prepare::settle_at_launch(
                            worker,
                            &home,
                            offers,
                            resume,
                            Some(channel),
                            platform,
                        );
                        *slot
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(answer);
                        evidence_landed();
                    },
                );
                if spawned.is_err() {
                    check();
                    return Pass::Ordinary;
                }
                self.launch = Launch::Running { landed, check };
                self.state = State::Pending(Pending::AwaitingTransaction);
                return Pass::Waiting;
            }
            Launch::Running { landed, check } => {
                let answer = landed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                match answer {
                    Some(answer) => (answer, check),
                    None => {
                        self.launch = Launch::Running { landed, check };
                        self.state = State::Pending(Pending::AwaitingTransaction);
                        return Pass::Waiting;
                    }
                }
            }
        };
        match landed {
            Landed::Ordinary => {
                check();
                Pass::Ordinary
            }
            Landed::Busy => {
                check();
                self.state = State::Idle;
                self.offered_this_launch = true;
                Pass::Decided((!self.said).then(|| {
                    self.said = true;
                    format!("Folio: update job — no offer: {}", Stop::Busy.why())
                }))
            }
            Landed::Resumed(offer, staged) => {
                let line = format!(
                    "Folio: update job — {} was prepared at an earlier launch and is verified again",
                    offer.tag()
                );
                self.presenter = crate::most_recently_active_window(
                    presenters.visited,
                    presenters.open,
                    presenters.quake,
                );
                self.staged = Some(*staged);
                self.state = State::Verified(offer);
                self.put_away = false;
                self.offered_this_launch = true;
                Pass::Decided((!self.said).then(|| {
                    self.said = true;
                    line
                }))
            }
        }
    }

    /// A job standing at `Verified` with `offer` — a test's way to the quit
    /// barrier without a driver (none exists before U-20).
    #[cfg(test)]
    pub(crate) fn verified_for_test(offer: Offer) -> Self {
        let mut job = Self::with_offers(true);
        job.state = State::Verified(offer);
        job
    }

    /// **Why the last quit gave the update up**, while the job is back at
    /// `Verified` — the reason the card names (§C.3, R-4).
    #[must_use]
    pub(crate) const fn quit_abandoned(&self) -> Option<Abandon> {
        self.abandoned
    }

    /// **The staged transaction** the quit hands to its applier: present from
    /// a driver's `Verified` report on, for as long as the process runs.
    #[must_use]
    pub(crate) const fn staged(&self) -> Option<&Staged> {
        self.staged.as_ref()
    }

    /// **Restart** on the verified card (§C.3): the job moves to `Quitting`
    /// and answers the reason the ordinary quit begins with — the quit
    /// carries the offer's transaction, so its two answers are refused as
    /// stale by any job that is not this one.
    ///
    /// # Errors
    /// [`TABLE`]'s refusal for Restart in the job's state; nothing moves.
    pub(crate) fn restart(&mut self) -> Result<crate::quit::Reason, Refusal> {
        // Restart's row never reaches a driver, so the one there is serves.
        let unfetched: SharedTransport = Arc::new(NoDownloadDoor);
        self.answer_verb(Verb::Restart, &Unsupported, &unfetched)?;
        let txn = self
            .state
            .offer()
            .map(Offer::txn)
            .expect("Restart moves only a verified job, to Quitting, which carries its offer");
        self.abandoned = None;
        Ok(crate::quit::Reason::UpdateRestart { txn })
    }

    /// The job's state.
    #[must_use]
    pub(crate) const fn state(&self) -> &State {
        &self.state
    }

    /// The window the card is raised in, while there is one.
    #[must_use]
    pub(crate) const fn presenter(&self) -> Option<W> {
        self.presenter
    }

    /// The last decision (`None` while pending).
    #[must_use]
    pub(crate) const fn answer(&self) -> Option<&Result<Eligible, NotEligible>> {
        self.answer.as_ref()
    }

    /// The failure About > Version keeps naming after its card was closed.
    #[must_use]
    pub(crate) fn last_failure(&self) -> Option<(&Option<Offer>, &Failure)> {
        self.last_failure
            .as_ref()
            .map(|(offer, failure)| (offer, failure))
    }

    /// **The window the card is drawn in now**, or `None` when no card is up
    /// (U-19).
    ///
    /// A card is up in the states C9 draws one for — `Available`, the
    /// download's two (`Downloading`, `Staged`), `Verified`, `Failed` and
    /// `Updated` (U-32) — in the presenting window, unless the reader put it
    /// away. `Quitting` has
    /// the quit's own card and `Committing` none.
    #[must_use]
    pub(crate) fn card_window(&self) -> Option<W> {
        let drawn = matches!(
            self.state,
            State::Available(_)
                | State::Downloading(..)
                | State::Staged(_)
                | State::Verified(_)
                | State::Failed(..)
                | State::Updated(..)
        );
        self.presenter.filter(|_| drawn && !self.put_away)
    }

    /// **About → Version asks for the card again** (`Restart…`, or `Update and
    /// restart` drawn before the job got to `Verified`), in `window` — the
    /// window the row was pressed in. Answers whether a card is now up there.
    pub(crate) fn reopen(&mut self, window: W) -> bool {
        if !matches!(self.state, State::Verified(_)) {
            return false;
        }
        self.presenter = Some(window);
        self.put_away = false;
        true
    }

    /// **The release an asked offer would name** (T-UPDATE-ON-ABOUT round 2,
    /// R1): the last decision's eligible tag, while it is the tag the check
    /// owner offers now (`known_tag`, [`crate::update::offer`]), offers reach
    /// this build ([`Self::offers_enabled`]), and no update this launch was
    /// told is incomplete may still be committed forward ([`Self::after_commit`]).
    /// The state is not read: [`Self::offer_again`] acts only from `Idle`.
    #[must_use]
    pub(crate) fn asked_offer(&self, known_tag: Option<&str>) -> Option<&str> {
        let Some(Ok(eligible)) = &self.answer else {
            return None;
        };
        (self.offers
            && !self.said_incomplete
            && self.offer_platform.is_some()
            && known_tag == Some(eligible.tag.as_str()))
        .then_some(eligible.tag.as_str())
    }

    /// **The reader asks for the offer from About → Version** (T-UPDATE-ON-ABOUT
    /// round 2, R1): from `Idle` — after Later, or after a failed card was
    /// closed — the job raises the same `Available` offer [`Self::consider`]
    /// raises for [`Self::asked_offer`]'s tag, in `window`, the window the row
    /// was pressed in. Independent of `offered_this_launch`, which stops a
    /// second *unasked* card; an asked one is not unasked. Answers whether the
    /// offer is up; the card's `Update` verb then runs unchanged.
    pub(crate) fn offer_again(&mut self, window: W, known_tag: Option<&str>) -> bool {
        if !matches!(self.state, State::Idle) {
            return false;
        }
        let (Some(tag), Some(platform)) = (self.asked_offer(known_tag), self.offer_platform) else {
            return false;
        };
        let Some(offer) = Offer::mint(mint_txn(), tag, platform) else {
            return false;
        };
        self.state = State::Available(offer);
        self.presenter = Some(window);
        self.put_away = false;
        true
    }

    /// **The press asked for the restart too** (N10): About → Version's
    /// *Update and restart* — the button's name is the reader's consent to the
    /// restart. The transaction the job is running now (downloading, verifying
    /// or verified) restarts when it is verified, without the Ready card asking
    /// again ([`Self::take_asked_restart`]). Nothing is asked in any other
    /// state: a press the driver refused has already failed.
    pub(crate) fn ask_restart_when_ready(&mut self) {
        self.restart_asked = Self::running_txn(&self.state);
    }

    /// **The asked restart, now due** (N10): `true` once, when the job stands
    /// at `Verified` for the transaction a press asked to restart — the
    /// caller then asks the application's quit exactly as the Ready card's
    /// Restart does (`App::restart_for_update`). The ask is spent either way:
    /// a restart that is refused leaves the job at `Verified` with its Ready
    /// card up ([`Self::apply`] raised it) and Version's `Restart…`, and the
    /// next restart is the reader's.
    pub(crate) fn take_asked_restart(&mut self) -> bool {
        let due =
            matches!(&self.state, State::Verified(offer) if Some(offer.txn) == self.restart_asked);
        if due {
            self.restart_asked = None;
        }
        due
    }

    /// Whether a press's restart is still asked for (N10).
    #[cfg(test)]
    pub(crate) const fn restart_is_asked(&self) -> bool {
        self.restart_asked.is_some()
    }

    /// The transaction the job is running toward a restart: downloading,
    /// verifying or verified.
    fn running_txn(state: &State) -> Option<TxnId> {
        match state {
            State::Downloading(offer, _) | State::Staged(offer) | State::Verified(offer) => {
                Some(offer.txn)
            }
            _ => None,
        }
    }

    /// **The asked restart does not outlive its transaction** (N10): once the
    /// job is anywhere but downloading, verifying or verified for the
    /// transaction it names — failed, cancelled, quitting, or on another
    /// offer — it is let go. Every move of the state ends here.
    fn keep_the_asked_restart_in_its_transaction(&mut self) {
        if self.restart_asked != Self::running_txn(&self.state) {
            self.restart_asked = None;
        }
    }

    /// **About → Version's `Details`**: the failed card in `window`. A failed
    /// card that was closed (`Idle`, the failure remembered for this launch,
    /// R3) is raised again with the same failure; Later closes it as before.
    pub(crate) fn show_failure(&mut self, window: W) -> bool {
        if matches!(self.state, State::Idle)
            && let Some((offer, failure)) = self.last_failure.clone()
        {
            self.state = State::Failed(offer, failure);
        }
        if !matches!(self.state, State::Failed(..)) {
            return false;
        }
        self.presenter = Some(window);
        self.put_away = false;
        true
    }

    /// **The card moves when its window goes** (§B: "closing the presenting
    /// window re-presents on the next active window"; coordinator ruling 5,
    /// U-19) — the card's one way of choosing a new presenter.
    ///
    /// A job holding an offer whose presenter is no longer among
    /// `presenters.open` takes the most recently active ordinary window
    /// ([`crate::most_recently_active_window`]; never the summoned terminal),
    /// or none while no ordinary window is open — and the next call, when one
    /// opens, seats it there. A card that was put away stays put away: the
    /// window moves, the reader's answer does not. Answers whether the
    /// presenter changed.
    pub(crate) fn hand_over(&mut self, presenters: &Presenters<'_, W>) -> bool {
        let has_a_card = self.state.offer().is_some()
            || matches!(self.state, State::Failed(..) | State::Updated(..));
        if !has_a_card
            || self
                .presenter
                .is_some_and(|window| presenters.open.contains(&window))
        {
            return false;
        }
        let next = crate::most_recently_active_window(
            presenters.visited,
            presenters.open,
            presenters.quake,
        );
        let moved = next != self.presenter;
        self.presenter = next;
        moved
    }

    /// **Consider an offer** on what has been gathered — the handler of
    /// `AppEvent::UpdateJobOffer`. An earlier launch's transaction is settled
    /// first ([`Self::after_start`]): until its pass lands nothing is decided,
    /// and a resumed set or a held lock decides this launch without an offer.
    ///
    /// Only a job that has not offered this launch is moved — and a job that
    /// has not offered holds no offer, so an offer, once minted, is never
    /// re-derived from a later answer. Returns the `diagnostics.log` line the
    /// first time the evidence is complete, and never again in this process.
    pub(crate) fn consider(
        &mut self,
        gathered: Gathered,
        presenters: &Presenters<'_, W>,
        mint: impl FnOnce() -> TxnId,
    ) -> Option<String> {
        // An earlier launch's transaction is settled before any offer (U-33).
        match self.launch_pass(&gathered, presenters) {
            Pass::Waiting => return None,
            Pass::Decided(line) => return line,
            Pass::Ordinary => {}
        }
        if self.offered_this_launch {
            // The launch's one unasked offer is spent, and the state does not
            // move; the decision is still kept current, so About → Version names
            // and asks for ([`Self::offer_again`]) what the evidence permits now
            // — a reader's Check can learn a newer tag after Later.
            if let Ok(evidence) = gathered.complete() {
                self.offer_platform = Some(evidence.platform);
                self.answer = Some(evidence.eligibility());
            }
            return None;
        }
        let evidence = match gathered.complete() {
            Ok(evidence) => evidence,
            Err(pending) => {
                self.state = State::Pending(pending);
                return None;
            }
        };
        let answer = evidence.eligibility();
        self.offer_platform = Some(evidence.platform);
        self.state = State::Idle;
        let line = (!self.said).then(|| {
            self.said = true;
            line(&answer, self.offers)
        });
        // **The offer does not wait for a window to be minted** (U-32, the
        // macOS rehearsal's first row): the window directory this reads is
        // published at each turn's head and as each window opens
        // (`FolioApp::publish_window_directory`), and a
        // check that settles before the first turn — a local release feed, a
        // fast network, the macOS loop delivering its first user events before
        // its first `about_to_wait` — found no window, left the job `Idle`,
        // and no later `UpdateJobOffer` came to ask again: the card was never
        // drawn although the line said it was offered. The card's one way of
        // choosing a window is [`Self::hand_over`], asked once a turn; an
        // offer minted with no presenter is seated there as soon as an
        // ordinary window is open.
        if let (Ok(eligible), true) = (&answer, self.offers)
            && let Some(offer) = Offer::mint(mint(), &eligible.tag, evidence.platform)
        {
            self.state = State::Available(offer);
            self.presenter = crate::most_recently_active_window(
                presenters.visited,
                presenters.open,
                presenters.quake,
            );
            self.offered_this_launch = true;
        }
        self.answer = Some(answer);
        line
    }

    /// **The reports waiting for the window thread, taken without applying
    /// them** — a test's way to read a driver's last word, which a job that
    /// has moved on drops as stale ([`Self::apply`] them after).
    #[cfg(test)]
    pub(crate) fn take_reports(&mut self) -> Vec<Progress> {
        std::mem::take(
            &mut *self
                .inbox
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// **Apply what the drivers reported**, in order — the handler of
    /// `AppEvent::UpdateJobProgress`. Returns how many were stale.
    pub(crate) fn drain_progress(&mut self) -> usize {
        let reports = std::mem::take(
            &mut *self
                .inbox
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        reports
            .into_iter()
            .filter(|report| self.apply(*report) == Applied::Stale)
            .count()
    }

    /// **One report**: it moves the job only if it names the job's own
    /// transaction and the job is in a state that report can come from.
    pub(crate) fn apply(&mut self, report: Progress) -> Applied {
        if self.state.offer().map(Offer::txn) != Some(report.txn) {
            return Applied::Stale;
        }
        let state = std::mem::replace(&mut self.state, State::Idle);
        let (next, applied) = match (state, report.step) {
            (State::Downloading(offer, _), Step::Received(bytes)) => {
                (State::Downloading(offer, bytes), Applied::Moved)
            }
            (State::Downloading(offer, _), Step::Staged) => (State::Staged(offer), Applied::Moved),
            (State::Staged(offer), Step::Verified) => {
                let staged = self
                    .staged_slot
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take_if(|(txn, _)| *txn == offer.txn)
                    .map(|(_, staged)| staged);
                if staged.is_some() {
                    self.staged = staged;
                }
                (State::Verified(offer), Applied::Moved)
            }
            (State::Downloading(offer, _) | State::Staged(offer), Step::Stopped(why)) => (
                State::Failed(Some(offer), Failure::Stopped(why)),
                Applied::Moved,
            ),
            (State::Quitting(offer), Step::QuitAbandoned(why)) => {
                self.abandoned = Some(why);
                (State::Verified(offer), Applied::Moved)
            }
            (State::Quitting(offer), Step::SessionLanded) => {
                (State::Committing(offer), Applied::Moved)
            }
            (state, _) => (state, Applied::Stale),
        };
        // **The download's end is said once** (coordinator ruling 1): a card
        // put away while the job downloaded or verified comes back, in the
        // window it was put away in, when the job reaches `Verified` or
        // `Failed` — and only on that move, so no state raises it twice.
        if applied == Applied::Moved && matches!(next, State::Verified(_) | State::Failed(..)) {
            self.put_away = false;
            // The driver is done: nothing is left to cancel.
            self.running = None;
        }
        if applied == Applied::Moved
            && let State::Failed(offer, failure) = &next
        {
            self.last_failure = Some((offer.clone(), failure.clone()));
        }
        self.state = next;
        self.keep_the_asked_restart_in_its_transaction();
        applied
    }
}

impl<W: Copy + Eq> Job<W> {
    /// **A card verb** — every (state, verb) pair is handled, as [`TABLE`] lists.
    ///
    /// # Errors
    /// The listed refusal; the job is unchanged.
    pub(crate) fn answer_verb(
        &mut self,
        verb: Verb,
        driver: &dyn Driver,
        transport: &SharedTransport,
    ) -> Result<Effect, Refusal> {
        let state = std::mem::replace(&mut self.state, State::Idle);
        let (next, outcome) = match (state, verb) {
            (state @ (State::Pending(_) | State::Idle), _) => (state, Err(Refusal::NoCard)),
            (State::Available(_), Verb::Later) => (State::Idle, Ok(Effect::None)),
            (State::Available(offer), Verb::Skip) => {
                (State::Idle, Ok(Effect::RecordSkip(offer.tag)))
            }
            (State::Available(offer), Verb::Press) => {
                let cancelled = Arc::new(AtomicBool::new(false));
                let post = Poster {
                    txn: offer.txn,
                    inbox: Arc::clone(&self.inbox),
                    staged: Arc::clone(&self.staged_slot),
                    cancelled: Arc::clone(&cancelled),
                };
                match driver.prepare(&offer, transport, &post) {
                    Ok(()) => {
                        self.last_failure = None;
                        self.running = Some(cancelled);
                        (
                            State::Downloading(offer, Bytes::default()),
                            Ok(Effect::None),
                        )
                    }
                    // A driver with no thread to run on is, to the reader, a
                    // copy that cannot update itself now: nothing moved.
                    Err(Refused::Unsupported | Refused::NoWorker) => (
                        State::Failed(Some(offer), Failure::Unsupported),
                        Ok(Effect::None),
                    ),
                }
            }
            (state @ State::Available(_), Verb::Cancel | Verb::Restart)
            | (
                state @ (State::Downloading(..) | State::Staged(_)),
                Verb::Skip | Verb::Press | Verb::Restart,
            )
            | (state @ State::Verified(_), Verb::Skip | Verb::Press | Verb::Cancel)
            | (
                state @ (State::Failed(..) | State::Updated(..)),
                Verb::Skip | Verb::Press | Verb::Cancel | Verb::Restart,
            ) => (state, Err(Refusal::NotOnThisCard)),
            (State::Downloading(..) | State::Staged(_), Verb::Cancel) => {
                self.stop_the_driver();
                (State::Idle, Ok(Effect::None))
            }
            // **Later on a card whose work goes on puts the card away** and
            // moves nothing (§B for `Verified`; coordinator ruling 1 for the
            // download's card): the row's foot is the way back to a verified
            // job, and a download's end raises the card again, once.
            (
                state @ (State::Downloading(..) | State::Staged(_) | State::Verified(_)),
                Verb::Later,
            ) => {
                self.put_away = true;
                (state, Ok(Effect::None))
            }
            (State::Verified(offer), Verb::Restart) => (State::Quitting(offer), Ok(Effect::None)),
            (state @ State::Quitting(_), _) => (state, Err(Refusal::TheQuitAnswers)),
            (state @ State::Committing(_), _) => (state, Err(Refusal::Exiting)),
            (State::Failed(..) | State::Updated(..), Verb::Later) => {
                (State::Idle, Ok(Effect::None))
            }
        };
        if matches!(next, State::Idle) {
            self.presenter = None;
            self.put_away = false;
        }
        if let State::Failed(offer, failure) = &next {
            self.last_failure = Some((offer.clone(), failure.clone()));
        }
        self.state = next;
        self.keep_the_asked_restart_in_its_transaction();
        outcome
    }
}

/// The one `diagnostics.log` line: the decision, with no path and no account.
fn line(answer: &Result<Eligible, NotEligible>, offers: bool) -> String {
    match answer {
        Ok(eligible) if offers => format!("Folio: update job — {} is offered", eligible.tag),
        Ok(eligible) => format!(
            "Folio: update job — {} would be offered; offers are off in this build",
            eligible.tag
        ),
        Err(why) => format!("Folio: update job — no offer: {}", why.why()),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::path::PathBuf;

    use bt_persist::UpdateCheckV1;
    use bt_platform::HostPlatform;

    use super::{
        Applied, Bytes, Driver, Effect, Evidence, Failure, Fetching, Gathered, Handling, Job, Kind,
        NotEligible, Offer, Pending, Poster, Presenters, Refused, Request, Route, SharedTransport,
        State, Step, Stop, TABLE, Transport, Unsupported, Verb,
    };
    use crate::install_channel::{self, Channel, Manager};
    use crate::update_txn::TxnId;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    /// The running build every fixture below is older than a tag it offers.
    const RUNNING: &str = "0.4.6";

    fn txn(byte: u8) -> TxnId {
        TxnId::new([byte; 16])
    }

    fn check(latest: Option<&str>, skipped: Option<&str>) -> UpdateCheckV1 {
        UpdateCheckV1 {
            latest_tag: latest.map(str::to_owned),
            skipped_tag: skipped.map(str::to_owned),
            ..UpdateCheckV1::default()
        }
    }

    /// Everything in: a newer tag, a flagged build, not a trial —
    /// with the channel as given.
    fn gathered(latest: &str, skipped: Option<&str>, channel: Option<Channel>) -> Gathered {
        Gathered {
            check: Some(check(Some(latest), skipped)),
            channel,
            running: RUNNING,
            capable: true,
            trial: false,
            platform: HostPlatform::Windows,
        }
    }

    /// One ordinary window, `1`.
    fn one_window() -> Presenters<'static, u32> {
        Presenters {
            visited: &[1],
            open: &[1],
            quake: None,
        }
    }

    /// A job whose offers are on, whatever the platform — the gate's own tests
    /// read the product's (`the_windows_gate_is_on`, `the_macos_gate_is_on`).
    fn job() -> Job<u32> {
        Job::with_offers(true)
    }

    /// A job with its card up for `tag`, transaction `txn(1)`.
    fn available(tag: &str) -> Job<u32> {
        let mut job = job();
        job.consider(
            gathered(tag, None, Some(Channel::Ours)),
            &one_window(),
            || txn(1),
        );
        assert!(
            matches!(job.state(), State::Available(offer) if offer.tag() == tag),
            "the fixture's card is up: {:?}",
            job.state()
        );
        job
    }

    /// A transport that records every request and fetches nothing; a clone
    /// shares the record, so the test keeps one while the job hands the other
    /// to its driver.
    #[derive(Clone, Default)]
    struct Recording(Arc<Mutex<Vec<Request>>>);

    impl Recording {
        /// This recording, as the job hands a transport to a driver.
        fn shared(&self) -> SharedTransport {
            Arc::new(self.clone())
        }

        fn requests(&self) -> Vec<Request> {
            self.0.lock().unwrap().clone()
        }
    }

    impl Transport for Recording {
        fn fetch(
            &self,
            request: &Request,
            into: &std::path::Path,
            _fetching: &Fetching,
        ) -> Result<std::path::PathBuf, String> {
            self.0.lock().unwrap().push(request.clone());
            Ok(into.join(&request.file_name))
        }
    }

    /// What a test's driver hands a fetch: no one listening, never cancelled.
    fn fetching() -> Fetching {
        Fetching {
            report: Arc::new(|_| {}),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// A driver that starts, and keeps the poster it was given so the test can
    /// report through it later — the shape of the drivers to come.
    #[derive(Default)]
    struct Starting(RefCell<Option<Poster>>);

    impl Driver for Starting {
        fn prepare(
            &self,
            offer: &Offer,
            transport: &SharedTransport,
            post: &Poster,
        ) -> Result<(), Refused> {
            for request in offer.requests() {
                transport
                    .fetch(&request, std::path::Path::new("."), &fetching())
                    .expect("the recording transport fetches");
            }
            *self.0.borrow_mut() = Some(post.clone());
            Ok(())
        }
    }

    /// RED (0.4.6 U-21) — **a driver's `Verified` report hands the job the
    /// staged transaction, and Restart begins the quit for that transaction.**
    ///
    /// The seam U-20 / U-27 report through: the home, the journal at
    /// `Prepared` and the transaction lock cross from the driver's thread with
    /// the report, and stay with the job — the lock held — until the quit's
    /// way out writes `Handoff` from them. Restart answers the reason the
    /// ordinary quit begins with, carrying the offer's transaction.
    ///
    /// MUTATION: in `Job::apply`'s `Verified` arm, leave the slot where it is —
    /// the job reaches `Verified` holding nothing to hand over.
    #[test]
    fn a_verified_report_hands_the_job_its_staged_transaction() {
        use bt_platform::install_txn::{self, Hold};

        use crate::update_handoff::Staged;
        use crate::update_txn::{Event, Home, Inventories, Journal, Layout, PhaseKind};

        let folder = bt_testpath::temp_path("bt-update-job-staged");
        let home = Home::at(folder.join(".folio-update"));
        std::fs::create_dir_all(home.root()).expect("the home");
        let mut job = available("v0.4.7");
        let driver = Starting::default();
        job.answer_verb(Verb::Press, &driver, &Recording::default().shared())
            .expect("the press is taken");
        let offer = job.state().offer().cloned().expect("an offer");
        let post = driver
            .0
            .borrow_mut()
            .take()
            .expect("the driver kept its poster");
        let journal = Journal::allocate(
            offer.txn(),
            folder.join("rescue.exe").to_string_lossy().into_owned(),
            Layout::Members(Inventories {
                old_shipped: Vec::new(),
                old_present: Vec::new(),
                new: Vec::new(),
            }),
        )
        .advance(&Event::Prepared)
        .expect("Allocated → Prepared");
        let lock = install_txn::try_hold(&home.lock(), Hold::Exclusive)
            .expect("the lock file opens")
            .expect("nobody else holds it");
        std::thread::scope(|threads| {
            threads.spawn(|| {
                post.post(Step::Staged);
                assert!(
                    post.verified(Staged {
                        home: home.clone(),
                        journal,
                        lock,
                    })
                    .is_ok(),
                    "the job was not cancelled"
                );
            });
        });
        assert_eq!(job.drain_progress(), 0, "no report is stale");
        assert!(matches!(job.state(), State::Verified(_)));
        let staged = job.staged().expect("the job holds the staged transaction");
        assert_eq!(staged.journal.txn, offer.txn());
        assert_eq!(staged.journal.body.phase.kind(), PhaseKind::Prepared);
        assert!(
            install_txn::try_hold(&home.lock(), Hold::Exclusive)
                .expect("the lock file opens")
                .is_none(),
            "and the transaction lock is still held"
        );
        assert_eq!(
            job.restart(),
            Ok(crate::quit::Reason::UpdateRestart { txn: offer.txn() })
        );
        assert!(matches!(job.state(), State::Quitting(_)));
        assert!(
            job.staged().is_some(),
            "the quit hands it over, not Restart"
        );
        drop(job);
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// RED (U-18) — **an offer is captured once and a later answer does not
    /// change it.**
    ///
    /// §B: the offer is "captured when the card is raised and never re-derived.
    /// A later check that moves `latest_tag` under an open card does not change
    /// the job." A card that changed its tag under the reader, or a download
    /// that switched releases mid-way, would be an offer nobody accepted.
    ///
    /// MUTATION: drop the `offered_this_launch` return at the top of
    /// `Job::consider` and the second answer mints `v0.4.8` over the open card.
    #[test]
    fn offered_tag_survives_latest_changes() {
        let mut job = available("v0.4.7");
        let before = job.state().offer().cloned().expect("an offer");
        assert_eq!(
            job.consider(
                gathered("v0.4.8", None, Some(Channel::Ours)),
                &one_window(),
                || txn(2)
            ),
            None
        );
        assert_eq!(job.state(), &State::Available(before.clone()));
        assert_eq!(before.txn(), txn(1));
        assert_eq!(before.to_version(), "0.4.7");

        // And under a download too: the job fetches the tag it offered.
        let driver = Starting::default();
        job.answer_verb(Verb::Press, &driver, &Recording::default().shared())
            .expect("the press is taken");
        job.consider(
            gathered("v0.4.9", None, Some(Channel::Ours)),
            &one_window(),
            || txn(3),
        );
        assert_eq!(
            job.state(),
            &State::Downloading(before, Bytes::default()),
            "a later answer moved the download to another release"
        );
    }

    /// RED (U-18) — **a report for a job that was cancelled, or for another
    /// transaction, is dropped and revives nothing.**
    ///
    /// A driver reports from its own thread, so its reports can arrive after
    /// the reader pressed Cancel. Each carries its transaction (§B, F-list
    /// `stale_progress_cannot_revive_a_cancelled_job`); the job applies a report
    /// only if it names the job's own transaction in a state that report can
    /// come from.
    ///
    /// MUTATION: make Cancel leave the state as it is in `Job::answer_verb`
    /// (`(state, Ok(Effect::None))`): the "cancelled" job is still
    /// downloading, and its late reports would carry it on to `Verified`.
    #[test]
    fn stale_progress_cannot_revive_a_cancelled_job() {
        let mut job = available("v0.4.7");
        let driver = Starting::default();
        job.answer_verb(Verb::Press, &driver, &Recording::default().shared())
            .expect("the press is taken");
        let post = driver
            .0
            .borrow_mut()
            .take()
            .expect("the driver was given a poster");
        post.post(Step::Received(Bytes {
            received: 1,
            total: Some(2),
        }));
        assert_eq!(job.drain_progress(), 0, "a report for this job is applied");
        assert_eq!(
            job.answer_verb(Verb::Cancel, &Unsupported, &Recording::default().shared()),
            Ok(Effect::None)
        );
        assert_eq!(job.state(), &State::Idle);
        assert_eq!(job.presenter(), None, "a cancelled job has no card");

        post.post(Step::Received(Bytes {
            received: 2,
            total: Some(2),
        }));
        post.post(Step::Staged);
        post.post(Step::Verified);
        assert_eq!(job.drain_progress(), 3, "all three late reports are stale");
        assert_eq!(job.state(), &State::Idle, "a cancelled job was revived");

        // Another transaction's report does not move a live download either.
        let mut live = available("v0.4.7");
        live.answer_verb(
            Verb::Press,
            &Starting::default(),
            &Recording::default().shared(),
        )
        .expect("the press is taken");
        let foreign = super::Progress {
            txn: txn(9),
            step: Step::Staged,
        };
        assert_eq!(live.apply(foreign), Applied::Stale);
        assert!(matches!(live.state(), State::Downloading(..)));
    }

    /// RED (U-18) — **a report moves the job only along §B's machine**:
    /// received bytes stay `Downloading`, then `Staged`, then `Verified`; a
    /// driver that stops fails the job; the quit barrier's two answers take
    /// `Quitting` back to `Verified` or on to `Committing`. Anything else —
    /// `Verified` reported to a job still downloading — is dropped.
    ///
    /// MUTATION: map `Step::Staged` on a downloading job to `Verified` in
    /// `Job::apply` and verification is skipped.
    #[test]
    fn a_report_moves_the_job_only_along_the_machine() {
        let mut job = available("v0.4.7");
        let driver = Starting::default();
        let transport = Recording::default();
        job.answer_verb(Verb::Press, &driver, &transport.shared())
            .expect("the press is taken");
        assert_eq!(
            transport.requests().len(),
            2,
            "a driver fetches the offer's two files"
        );
        let offer = job.state().offer().cloned().expect("an offer");
        let report = |step| super::Progress {
            txn: offer.txn(),
            step,
        };
        assert_eq!(job.apply(report(Step::Verified)), Applied::Stale);
        let bytes = Bytes {
            received: 12,
            total: None,
        };
        assert_eq!(job.apply(report(Step::Received(bytes))), Applied::Moved);
        assert_eq!(job.state(), &State::Downloading(offer.clone(), bytes));
        assert_eq!(job.apply(report(Step::Staged)), Applied::Moved);
        assert_eq!(job.state(), &State::Staged(offer.clone()));
        assert_eq!(job.apply(report(Step::Verified)), Applied::Moved);
        assert_eq!(job.state(), &State::Verified(offer.clone()));
        job.answer_verb(Verb::Restart, &Unsupported, &Recording::default().shared())
            .expect("Restart");
        assert_eq!(
            job.apply(report(Step::QuitAbandoned(super::Abandon::Cancelled))),
            Applied::Moved
        );
        assert_eq!(job.state(), &State::Verified(offer.clone()));
        job.answer_verb(Verb::Restart, &Unsupported, &Recording::default().shared())
            .expect("Restart");
        assert_eq!(job.apply(report(Step::SessionLanded)), Applied::Moved);
        assert_eq!(job.state(), &State::Committing(offer.clone()));

        let mut stopped = available("v0.4.7");
        stopped
            .answer_verb(
                Verb::Press,
                &Starting::default(),
                &Recording::default().shared(),
            )
            .expect("the press is taken");
        assert_eq!(
            stopped.apply(report(Step::Stopped(Stop::Download))),
            Applied::Moved
        );
        assert_eq!(
            stopped.state(),
            &State::Failed(Some(offer), Failure::Stopped(Stop::Download))
        );
    }

    /// A job standing in `kind`, with an offer where the state carries one.
    fn standing_in(kind: Kind) -> Job<u32> {
        let offer = Offer::mint(txn(1), "v0.4.7", HostPlatform::Windows).expect("a release tag");
        let mut job = job();
        job.state = match kind {
            Kind::Pending => State::Pending(Pending::AwaitingBoth),
            Kind::Idle => State::Idle,
            Kind::Available => State::Available(offer),
            Kind::Downloading => State::Downloading(offer, Bytes::default()),
            Kind::Staged => State::Staged(offer),
            Kind::Verified => State::Verified(offer),
            Kind::Quitting => State::Quitting(offer),
            Kind::Committing => State::Committing(offer),
            Kind::Failed => State::Failed(Some(offer), Failure::Unsupported),
            Kind::Updated => {
                State::Updated("0.4.7".to_owned(), crate::update_job::TrialChanges::Kept)
            }
        };
        job
    }

    /// RED (U-18) — **every verb on every state is handled: it moves the job
    /// or is a listed refusal.**
    ///
    /// §B: "There is no reachable state in which a card verb exists with no
    /// handler: the enumeration above is total and is gated by a test." The
    /// table has one row for each of the 50 pairs, and `Job::answer_verb` —
    /// the product's only driver in hand — does exactly what its row says; a
    /// refusal leaves the job as it was.
    ///
    /// MUTATION: make `Verified` + **Later** discard the staged job
    /// (`(State::Verified(_), Verb::Later) => (State::Idle, …)`) and its row
    /// (`Moves(Verified)`: Later keeps the staged transaction) goes red.
    #[test]
    fn every_card_state_has_a_handler_for_every_verb() {
        let pairs: HashSet<(Kind, Verb)> =
            TABLE.iter().map(|(kind, verb, _)| (*kind, *verb)).collect();
        assert_eq!(pairs.len(), TABLE.len(), "a pair is listed twice");
        for kind in Kind::ALL {
            for verb in Verb::ALL {
                assert!(
                    pairs.contains(&(kind, verb)),
                    "{kind:?} × {verb:?} has no row"
                );
            }
        }
        for (kind, verb, handling) in TABLE {
            let mut job = standing_in(kind);
            let before = job.state().clone();
            let transport = Recording::default();
            let outcome = job.answer_verb(verb, &Unsupported, &transport.shared());
            match handling {
                Handling::Refused(reason) => {
                    assert_eq!(outcome, Err(reason), "{kind:?} × {verb:?}");
                    assert_eq!(job.state(), &before, "a refused {verb:?} moved {kind:?}");
                }
                Handling::Moves(to) => {
                    assert!(
                        outcome.is_ok(),
                        "{kind:?} × {verb:?} was refused: {outcome:?}"
                    );
                    assert_eq!(job.state().kind(), to, "{kind:?} × {verb:?}");
                }
                Handling::Prepares => {
                    assert!(
                        outcome.is_ok(),
                        "{kind:?} × {verb:?} was refused: {outcome:?}"
                    );
                    assert_eq!(
                        job.state().kind(),
                        Kind::Failed,
                        "the only driver there is refuses"
                    );
                    let mut started = standing_in(kind);
                    started
                        .answer_verb(verb, &Starting::default(), &transport.shared())
                        .expect("a driver that starts");
                    assert_eq!(started.state().kind(), Kind::Downloading);
                }
            }
        }
        // Skip is the one verb that asks the check's owner to write.
        let mut job = standing_in(Kind::Available);
        assert_eq!(
            job.answer_verb(Verb::Skip, &Unsupported, &Recording::default().shared()),
            Ok(Effect::RecordSkip("v0.4.7".to_owned()))
        );
    }

    /// RED (U-18) — **a press with no driver fails at once and fetches
    /// nothing.**
    ///
    /// Until U-20 and U-27 there is no Prepare. The press must still land
    /// somewhere a card can draw — `Failed(Unsupported)`, "Nothing changed." —
    /// and it must not have touched the network, a staging folder or the
    /// disk to get there: the recording transport sees no request and no
    /// progress is posted.
    ///
    /// MUTATION: have `Unsupported::prepare` fetch the offer's
    /// `requests()` before it refuses, and the transport records two.
    #[test]
    fn an_unsupported_driver_fails_without_downloading() {
        let mut job = available("v0.4.7");
        let offer = job.state().offer().cloned().expect("an offer");
        let transport = Recording::default();
        assert_eq!(
            job.answer_verb(Verb::Press, &Unsupported, &transport.shared()),
            Ok(Effect::None)
        );
        assert_eq!(
            job.state(),
            &State::Failed(Some(offer), Failure::Unsupported)
        );
        assert!(
            transport.requests().is_empty(),
            "the press fetched {:?}",
            transport.requests()
        );
        assert_eq!(job.drain_progress(), 0);
        assert!(matches!(job.state(), State::Failed(..)), "nothing reported");
        assert_eq!(job.presenter(), Some(1), "the failure is said on the card");
    }

    /// RED (U-18) — **an answer that is not an offer leaves the launch's one
    /// offer unspent.**
    ///
    /// §B: "An offer suppressed because a cached tag was skipped does not
    /// consume the once-per-launch gate — a newer tag arriving later still
    /// gets its card." And the gate holds once it is spent: after Later, the
    /// same launch offers nothing more.
    ///
    /// MUTATION: set `offered_this_launch` whenever the evidence is complete in
    /// `Job::consider` (not only when a card goes up) and the newer tag gets no
    /// card.
    #[test]
    fn a_suppressed_offer_does_not_consume_the_launch_gate() {
        let mut job = job();
        job.consider(
            gathered("v0.4.7", Some("v0.4.7"), Some(Channel::Ours)),
            &one_window(),
            || txn(1),
        );
        assert_eq!(job.state(), &State::Idle);
        assert_eq!(
            job.answer(),
            Some(&Err(NotEligible::Skipped {
                tag: "v0.4.7".to_owned()
            }))
        );
        job.consider(
            gathered("v0.4.6", None, Some(Channel::Ours)),
            &one_window(),
            || txn(1),
        );
        assert_eq!(job.answer(), Some(&Err(NotEligible::NoNewerRelease)));
        job.consider(
            gathered("v0.4.8", Some("v0.4.7"), Some(Channel::Ours)),
            &one_window(),
            || txn(2),
        );
        assert!(
            matches!(job.state(), State::Available(offer) if offer.tag() == "v0.4.8"),
            "the newer tag got no card: {:?}",
            job.state()
        );
        job.answer_verb(Verb::Later, &Unsupported, &Recording::default().shared())
            .expect("Later");
        job.consider(
            gathered("v0.4.9", None, Some(Channel::Ours)),
            &one_window(),
            || txn(3),
        );
        assert_eq!(job.state(), &State::Idle, "a second card in one launch");
    }

    /// RED (U-18) — **no offer is derived from half the evidence: the job
    /// waits, typed, for how this copy was installed.**
    ///
    /// The check can answer before the channel's worker has read the install
    /// folder. Deciding then would offer an update to a copy scoop owns. The
    /// job sits in `Pending` naming what it waits for; the channel's arrival
    /// decides — `Ours` raises the card, `Managed` names the manager's command.
    ///
    /// MUTATION: have `Gathered::complete` read a missing channel as
    /// `Channel::Ours` and the first answer raises a card.
    #[test]
    fn no_offer_before_classification_arrives() {
        let mut job = job();
        let line = job.consider(gathered("v0.4.7", None, None), &one_window(), || txn(1));
        assert_eq!(
            job.state(),
            &State::Pending(Pending::AwaitingClassification)
        );
        assert_eq!(job.answer(), None);
        assert_eq!(line, None, "nothing is said before anything is decided");
        job.consider(
            gathered("v0.4.7", None, Some(Channel::Ours)),
            &one_window(),
            || txn(1),
        );
        assert!(matches!(job.state(), State::Available(_)));

        let mut managed = self::job();
        managed.consider(gathered("v0.4.7", None, None), &one_window(), || txn(1));
        managed.consider(
            gathered(
                "v0.4.7",
                None,
                Some(Channel::Managed {
                    manager: Manager::Scoop,
                    uninstall_hook: true,
                }),
            ),
            &one_window(),
            || txn(1),
        );
        assert_eq!(managed.state(), &State::Idle);
        let answer = managed.answer().cloned().expect("decided");
        assert_eq!(
            answer,
            Err(NotEligible::Managed {
                manager: Manager::Scoop,
                command: "scoop update folio"
            })
        );
        assert_eq!(
            answer.unwrap_err().route(),
            Route::Command("scoop update folio")
        );

        // And the other halves, each named.
        let mut waiting = self::job();
        waiting.consider(
            Gathered {
                check: None,
                ..gathered("v0.4.7", None, Some(Channel::Ours))
            },
            &one_window(),
            || txn(1),
        );
        assert_eq!(waiting.state(), &State::Pending(Pending::AwaitingCheck));
        waiting.consider(
            Gathered {
                check: None,
                ..gathered("v0.4.7", None, None)
            },
            &one_window(),
            || txn(1),
        );
        assert_eq!(waiting.state(), &State::Pending(Pending::AwaitingBoth));
    }

    /// RED (U-18) — **an update's trial never offers.**
    ///
    /// A trial (`update_startup::trial`) is the new build proving it starts;
    /// offering it the next release would stack a second transaction on an
    /// undecided first one.
    ///
    /// MUTATION: drop the `trial` clause of `Evidence::eligibility`.
    #[test]
    fn a_trial_process_never_offers() {
        let mut job = job();
        job.consider(
            Gathered {
                trial: true,
                ..gathered("v0.4.7", None, Some(Channel::Ours))
            },
            &one_window(),
            || txn(1),
        );
        assert_eq!(job.state(), &State::Idle);
        assert_eq!(job.answer(), Some(&Err(NotEligible::Trial)));
    }

    fn eligibility(
        channel: Channel,
        capable: bool,
        platform: HostPlatform,
    ) -> Result<super::Eligible, NotEligible> {
        Evidence {
            check: check(Some("v0.4.7"), None),
            running: RUNNING,
            channel,
            capable,
            trial: false,
            platform,
        }
        .eligibility()
    }

    /// RED (U-18) — **a copy that is not ours, or whose install is unknown, is
    /// sent to the releases page; a managed one gets its manager's command.**
    ///
    /// §D: "Unknown fails safe to the releases page"; C2's commands for the
    /// three managers; §D "not updater-capable" keeps today's row (the page).
    ///
    /// MUTATION: read `Channel::Unknown` as ours in `Evidence::eligibility`
    /// (`Channel::Ours | Channel::Unknown => {}`) and an unknown copy is offered.
    #[test]
    fn a_copy_that_is_not_ours_or_unknown_is_sent_to_the_releases_page() {
        let windows = HostPlatform::Windows;
        for (channel, expected) in [
            (Channel::NotOurs, NotEligible::NotOurs),
            (Channel::Unknown, NotEligible::Unknown),
        ] {
            let answer = eligibility(channel, true, windows);
            assert_eq!(answer, Err(expected.clone()));
            assert_eq!(expected.route(), Route::ReleasesPage);
        }
        for (manager, command) in [
            (Manager::Scoop, "scoop update folio"),
            (Manager::Homebrew, "brew upgrade --cask folio"),
            (
                Manager::Winget,
                "winget upgrade --id WeiyiShi.Folio --exact",
            ),
        ] {
            let channel = Channel::Managed {
                manager,
                uninstall_hook: false,
            };
            assert_eq!(
                eligibility(channel, true, windows),
                Err(NotEligible::Managed { manager, command })
            );
        }
        let unflagged = eligibility(Channel::Ours, false, windows);
        assert_eq!(unflagged, Err(NotEligible::NotUpdaterBuild));
        assert_eq!(unflagged.unwrap_err().route(), Route::ReleasesPage);
        assert_eq!(
            eligibility(Channel::Ours, true, HostPlatform::OtherUnix),
            Err(NotEligible::NoAsset {
                tag: "v0.4.7".to_owned()
            })
        );
        assert_eq!(
            eligibility(Channel::Ours, true, HostPlatform::MacOs),
            Ok(super::Eligible {
                tag: "v0.4.7".to_owned()
            })
        );
    }

    /// RED (U-18) — **the card is raised in the window the reader was last in,
    /// never in the summoned terminal.**
    ///
    /// §B, and §7.59's rule (`most_recently_active_window`): the summoned
    /// terminal is a companion that spends most of its life hidden. A run
    /// whose only window is the summoned one keeps its offer for a window,
    /// which `Job::hand_over` seats once one is open (U-32).
    ///
    /// MUTATION: pass `None` for `presenters.quake` in `Job::consider` and the
    /// card goes up in the summoned terminal, `9`.
    #[test]
    fn the_offer_is_minted_in_the_most_recently_active_ordinary_window() {
        let mut job = job();
        job.consider(
            gathered("v0.4.7", None, Some(Channel::Ours)),
            &Presenters {
                visited: &[1, 2, 9],
                open: &[1, 2, 9],
                quake: Some(9),
            },
            || txn(1),
        );
        assert_eq!(job.presenter(), Some(2));

        let mut alone = self::job();
        alone.consider(
            gathered("v0.4.7", None, Some(Channel::Ours)),
            &Presenters {
                visited: &[9],
                open: &[9],
                quake: Some(9),
            },
            || txn(1),
        );
        assert_eq!(alone.presenter(), None, "never the summoned terminal");
        assert_eq!(alone.card_window(), None, "no card is drawn yet");
        alone.hand_over(&Presenters {
            visited: &[9, 4],
            open: &[9, 4],
            quake: Some(9),
        });
        assert_eq!(alone.presenter(), Some(4), "the offer waited for a window");
    }

    /// RED (U-32, the macOS rehearsal's first row) — **an offer considered
    /// before the first turn has published any window is still drawn: the
    /// next turn's hand-over seats it in the one ordinary window, and its card
    /// is painted there.**
    ///
    /// The window thread publishes its window directory once a turn, in
    /// `about_to_wait`. On macOS, with a local feed, the check and the channel
    /// both landed before that first turn: the job considered with no window
    /// open (only the restored, hidden summoned terminal on its way), said
    /// "v0.4.7 is offered", stayed `Idle`, and nothing asked it again — the
    /// gear had its dot and no card ever appeared. This is the product's
    /// order: `consider` with the directory the first turn has not written
    /// yet, then `settle_update_card`'s `hand_over` with the one it writes.
    ///
    /// MUTATION: in `Job::consider`, mint the offer only when a window is
    /// found (the `let Some(window) = …` guard of before).
    #[test]
    fn an_offer_considered_before_any_window_is_published_is_seated_and_painted_at_the_next_turn() {
        let mut job: Job<u32> = Job::for_platform(HostPlatform::MacOs);
        let line = job.consider(
            Gathered {
                platform: HostPlatform::MacOs,
                ..gathered("v0.4.7", None, Some(Channel::Ours))
            },
            &Presenters {
                visited: &[1],
                open: &[],
                quake: None,
            },
            || txn(1),
        );
        assert_eq!(
            line.as_deref(),
            Some("Folio: update job — v0.4.7 is offered")
        );
        assert_eq!(job.card_window(), None, "no window is published yet");
        assert!(job.hand_over(&Presenters {
            visited: &[1, 9],
            open: &[1, 9],
            quake: Some(9),
        }));
        assert_eq!(
            job.card_window(),
            Some(1),
            "the ordinary window, not the summoned one"
        );
        let paint = crate::update_card::paint(job.state()).expect("a card is painted");
        assert_eq!(paint.heading.as_deref(), Some("Folio 0.4.7"));
        assert_eq!(
            paint.verbs,
            vec![
                crate::update_card::CardVerb::Update,
                crate::update_card::CardVerb::Later,
                crate::update_card::CardVerb::Skip
            ]
        );
    }

    /// RED (U-31) — **a Windows build offers: the job it holds raises the card
    /// for a newer release and says so once.**
    ///
    /// U-18 built the job with its gate shut; U-31 opens it for Windows only,
    /// once the Windows roads — the Prepare, the card, the quit barrier, the
    /// apply and the rollback — exist and the clean-VM checklist has run on
    /// this build. The gate is a build fact per platform, read here through
    /// the constructor the application uses (`Job::for_platform`, which
    /// `Job::default` calls with the host), so a Mac or Linux runner reads the
    /// Windows gate too. `diagnostics.log` names the offer, and the decision is
    /// still said once per launch.
    ///
    /// MUTATION: set `OFFERS_ENABLED_WINDOWS` to `false`.
    #[test]
    fn the_windows_gate_is_on() {
        assert!(
            Job::<u32>::offers_enabled_on(HostPlatform::Windows),
            "U-31: offers are on for Windows"
        );
        assert_eq!(
            Job::<u32>::offers_enabled(),
            Job::<u32>::offers_enabled_on(bt_platform::host_platform()),
            "the application's gate is its own platform's"
        );
        let mut job: Job<u32> = Job::for_platform(HostPlatform::Windows);
        let line = job.consider(
            gathered("v0.4.7", None, Some(Channel::Ours)),
            &one_window(),
            || txn(1),
        );
        assert!(
            matches!(job.state(), State::Available(offer) if offer.tag() == "v0.4.7"),
            "no card on Windows: {:?}",
            job.state()
        );
        assert_eq!(job.presenter(), Some(1));
        assert_eq!(
            line.as_deref(),
            Some("Folio: update job — v0.4.7 is offered")
        );
        assert_eq!(
            job.consider(
                gathered("v0.4.8", None, Some(Channel::Ours)),
                &one_window(),
                || txn(2)
            ),
            None,
            "the decision is said once per launch"
        );
    }

    /// RED (U-32) — **a macOS build offers: a copy that is ours gets the card
    /// for a newer release, said once, and a copy Homebrew owns gets
    /// `brew upgrade` on About → Version and no card.**
    ///
    /// U-18 built the job with its gate shut; U-31 opened it for Windows and
    /// U-32 opens it for macOS, once the macOS roads — the Prepare (U-27), the
    /// exchange, trial and commit (U-28), the rollback and `Stuck` (U-29), the
    /// recovery of every phase (U-29b), the launch pass (U-33) and the one exit
    /// guard (U-34) — exist and the rehearsal on two signed, notarised bundles
    /// has run. The gate is still a build fact, read through the constructor
    /// the application uses (`Job::for_platform`). The 2026-09-20 ruling holds
    /// after it: each copy's channel comes out of the real classifier over
    /// what a Mac reads (the marker is the bundle's extended attribute, there
    /// is no scoop receipt), and what each copy shows is read the way the
    /// window reads it — the card's paint and the row's foot. A platform no
    /// release is built for still has no gate to open.
    ///
    /// MUTATION: set `OFFERS_ENABLED_MACOS` to `false`.
    #[test]
    fn the_macos_gate_is_on() {
        use crate::install_channel::{
            Evidence, Marker, MarkerEvidence, OwnerEvidence, ReceiptEvidence, WingetEvidence,
        };
        use crate::update_card::{self, RowFoot};
        assert!(
            Job::<u32>::offers_enabled_on(HostPlatform::MacOs),
            "U-32: offers are on for macOS"
        );
        assert!(!Job::<u32>::offers_enabled_on(HostPlatform::OtherUnix));
        let read_on_a_mac = |marker: MarkerEvidence| {
            install_channel::classify(&Evidence {
                marker,
                receipt: ReceiptEvidence::NotApplicable,
                owner: OwnerEvidence::ThisAccount,
                winget: WingetEvidence::NotApplicable,
            })
        };
        let mac = |tag: &str, channel: Channel| Gathered {
            platform: HostPlatform::MacOs,
            ..gathered(tag, None, Some(channel))
        };

        let ours = read_on_a_mac(MarkerEvidence::Absent);
        assert_eq!(ours, Channel::Ours);
        let mut job: Job<u32> = Job::for_platform(HostPlatform::MacOs);
        let line = job.consider(mac("v0.4.7", ours), &one_window(), || txn(1));
        assert!(
            matches!(job.state(), State::Available(offer) if offer.tag() == "v0.4.7"),
            "no card on macOS: {:?}",
            job.state()
        );
        assert!(
            update_card::paint(job.state()).is_some(),
            "the card is drawn"
        );
        assert_eq!(job.presenter(), Some(1));
        assert_eq!(
            line.as_deref(),
            Some("Folio: update job — v0.4.7 is offered")
        );
        assert_eq!(
            job.consider(mac("v0.4.8", ours), &one_window(), || txn(2)),
            None,
            "the decision is said once per launch"
        );

        let homebrew = read_on_a_mac(MarkerEvidence::Present(Marker {
            manager: Manager::Homebrew,
            uninstall_hook: false,
        }));
        let mut managed: Job<u32> = Job::for_platform(HostPlatform::MacOs);
        managed.consider(mac("v0.4.7", homebrew), &one_window(), || txn(3));
        assert_eq!(managed.state(), &State::Idle, "a Homebrew copy got a card");
        assert_eq!(update_card::paint(managed.state()), None);
        assert_eq!(
            update_card::row_foot(&managed),
            RowFoot::Copy {
                command: "brew upgrade --cask folio"
            }
        );
    }

    /// RED (U-31) — **through the Windows gate, a copy that is ours gets the
    /// card and a copy scoop owns gets scoop's command on About → Version.**
    ///
    /// Opening the gate must not reach a managed copy: the 2026-09-20 ruling
    /// (managed installs do not self-update) holds after it. Each copy is a
    /// real folder read by the real channel reader and classifier — `ours`
    /// owned by this account with no marker, `scoop` with scoop's marker — the
    /// check's answer comes through the check's own owner, and the job is the
    /// one a Windows build holds (`Job::for_platform`). What each copy then
    /// shows is read the way the window reads it: the card's paint and the
    /// row's foot.
    ///
    /// MUTATION: set `OFFERS_ENABLED_WINDOWS` to `false` (the `ours` copy gets
    /// no card).
    #[test]
    fn an_ours_copy_sees_the_offer_and_a_managed_copy_sees_the_command() {
        use crate::update_card::{self, RowFoot};
        struct Answering;
        impl crate::update::Releases for Answering {
            fn latest_tag(&self) -> Result<String, String> {
                Ok("v99.0.1".to_owned())
            }
        }
        let base = bt_testpath::temp_path("bt-update-job-gate");
        let _ = std::fs::remove_dir_all(&base);
        let me = bt_platform::install_evidence::current_account().unwrap();
        for (folder, marker) in [
            ("ours", None),
            (
                "scoop",
                Some(&br#"{"v":1,"manager":"scoop","uninstall_hook":true}"#[..]),
            ),
        ] {
            let data = base.join(folder).join("data");
            let root = base.join(folder).join("install");
            std::fs::create_dir_all(&data).unwrap();
            std::fs::create_dir_all(&root).unwrap();
            if let Some(marker) = marker {
                std::fs::write(root.join(install_channel::MARKER_FILE_NAME), marker).unwrap();
            }
            let channel = install_channel::classify(&install_channel::read(
                &root,
                HostPlatform::Windows,
                Ok(&me),
                install_channel::WingetEvidence::None,
            ));
            let owner = crate::update::OfferState::load(&data, true);
            let _ = owner.run(crate::update::CHECK_INTERVAL_MS + 1, &Answering);
            let mut job: Job<u32> = Job::for_platform(HostPlatform::Windows);
            let line = job.consider(
                Gathered {
                    check: owner.job_evidence(),
                    channel: Some(channel),
                    running: crate::version::VERSION,
                    capable: true,
                    trial: false,
                    platform: HostPlatform::Windows,
                },
                &one_window(),
                || txn(1),
            );
            if marker.is_none() {
                assert!(
                    matches!(job.state(), State::Available(offer) if offer.tag() == "v99.0.1"),
                    "{folder}: {:?}",
                    job.state()
                );
                assert!(
                    update_card::paint(job.state()).is_some(),
                    "{folder}: the card is drawn"
                );
                assert_eq!(
                    line.as_deref(),
                    Some("Folio: update job — v99.0.1 is offered")
                );
                assert_eq!(update_card::row_foot(&job), RowFoot::ReleasesPage);
            } else {
                assert_eq!(
                    job.state(),
                    &State::Idle,
                    "{folder}: a managed copy got a card"
                );
                assert_eq!(update_card::paint(job.state()), None, "{folder}");
                assert_eq!(
                    update_card::row_foot(&job),
                    RowFoot::Copy {
                        command: "scoop update folio"
                    },
                    "{folder}"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// RED (U-18) — **a press fetches exactly two files, by the offer's own
    /// tag, and never "latest".**
    ///
    /// C11: `/releases/download/<tag>/folio-<version>-windows-x64.zip` and
    /// `SHA256SUMS.txt` (macOS: the versioned `.dmg` and
    /// `SHA256SUMS-macos.txt`); `<version>` is the tag without `v` and
    /// `-preview`. "latest" can move under an open offer (R-8).
    ///
    /// MUTATION: build the path on `/releases/latest/download` in
    /// `Offer::requests`.
    #[test]
    fn a_press_fetches_exactly_two_files_by_the_offers_own_tag() {
        let windows =
            Offer::mint(txn(1), "v0.4.7-preview", HostPlatform::Windows).expect("a release tag");
        assert_eq!(windows.to_version(), "0.4.7");
        let paths = windows
            .requests()
            .map(|request| (request.host, request.path));
        assert_eq!(
            paths,
            [
                (
                    "github.com",
                    "/lulu-loopp/folio-terminal/releases/download/v0.4.7-preview/folio-0.4.7-windows-x64.zip".to_owned()
                ),
                (
                    "github.com",
                    "/lulu-loopp/folio-terminal/releases/download/v0.4.7-preview/SHA256SUMS.txt".to_owned()
                ),
            ]
        );
        let mac = Offer::mint(txn(1), "v0.4.7", HostPlatform::MacOs).expect("a release tag");
        assert_eq!(
            mac.requests().map(|request| request.file_name),
            [
                "Folio-0.4.7-macos-arm64.dmg".to_owned(),
                "SHA256SUMS-macos.txt".to_owned()
            ]
        );
        for tag in ["0.4.7", "v0.4.7-rc.1", "v0.4", "vx.y.z", "v0.4.7+build"] {
            assert_eq!(
                Offer::mint(txn(1), tag, HostPlatform::Windows),
                None,
                "{tag}"
            );
        }
        assert_eq!(Offer::mint(txn(1), "v0.4.7", HostPlatform::OtherUnix), None);
    }

    /// RED (U-18) — **the job decides on the real channel and the real check.**
    ///
    /// The seam test: a real install folder read by `install_channel::read`
    /// and judged by `classify`, and a real `update-check.json` owner whose
    /// real `run` settles the check — into the job. Before the check settles
    /// the owner hands over nothing, and the job waits for it.
    ///
    /// MUTATION: have `OfferState::job_evidence` answer before the check
    /// settles, and the job decides on the cached `v99.0.0` instead of waiting
    /// for the check's `v99.0.1`.
    #[test]
    fn the_job_decides_on_the_real_channel_and_the_real_check() {
        struct Answering;
        impl crate::update::Releases for Answering {
            fn latest_tag(&self) -> Result<String, String> {
                Ok("v99.0.1".to_owned())
            }
        }
        let base = bt_testpath::temp_path("bt-update-job-seam");
        let _ = std::fs::remove_dir_all(&base);
        let data = base.join("data");
        std::fs::create_dir_all(&data).unwrap();
        bt_persist::write_update_check_atomic(
            &data.join(crate::update::STATE_FILE_NAME),
            &check(Some("v99.0.0"), None),
        )
        .unwrap();
        let me = bt_platform::install_evidence::current_account().unwrap();
        for (folder, marker, expected) in [
            ("ours", None, None),
            (
                "scoop",
                Some(&br#"{"v":1,"manager":"scoop","uninstall_hook":true}"#[..]),
                Some(NotEligible::Managed {
                    manager: Manager::Scoop,
                    command: "scoop update folio",
                }),
            ),
        ] {
            let root = base.join(folder);
            std::fs::create_dir_all(&root).unwrap();
            if let Some(marker) = marker {
                std::fs::write(root.join(install_channel::MARKER_FILE_NAME), marker).unwrap();
            }
            let channel = install_channel::classify(&install_channel::read(
                &root,
                HostPlatform::Windows,
                Ok(&me),
                install_channel::WingetEvidence::None,
            ));
            let owner = crate::update::OfferState::load(&data, true);
            let mut job = job();
            let now = |owner: &crate::update::OfferState| Gathered {
                check: owner.job_evidence(),
                channel: Some(channel),
                running: crate::version::VERSION,
                capable: true,
                trial: false,
                platform: HostPlatform::Windows,
            };
            job.consider(now(&owner), &one_window(), || txn(1));
            assert_eq!(
                job.state(),
                &State::Pending(Pending::AwaitingCheck),
                "{folder}"
            );
            // A day after the fixture's stamp of zero: the check asks.
            let _ = owner.run(crate::update::CHECK_INTERVAL_MS + 1, &Answering);
            job.consider(now(&owner), &one_window(), || txn(1));
            match expected {
                None => assert!(
                    matches!(job.state(), State::Available(offer) if offer.tag() == "v99.0.1"),
                    "{folder}: {:?}",
                    job.state()
                ),
                Some(why) => {
                    assert_eq!(job.state(), &State::Idle, "{folder}");
                    assert_eq!(job.answer(), Some(&Err(why)), "{folder}");
                }
            }
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// RED (U-18) — **the job's two events are each charged to a station of
    /// their own**, so a slow decision or a slow drain is named in a stall
    /// report rather than filed under a wake nobody named.
    ///
    /// MUTATION: charge `AppEvent::UpdateJobOffer` to `Station::Woken`.
    #[test]
    fn the_jobs_events_are_charged_to_stations_of_their_own() {
        use crate::hang_watch::Station;
        assert_eq!(
            crate::AppEvent::UpdateJobOffer.station(),
            Station::UpdateJobOffer
        );
        assert_eq!(
            crate::AppEvent::UpdateJobProgress.station(),
            Station::UpdateJobProgress
        );
        assert_eq!(
            Station::UpdateJobOffer.label(),
            "FolioApp::consider_update_offer"
        );
        assert_eq!(
            Station::UpdateJobProgress.label(),
            "update_job::Job::drain_progress"
        );
    }

    /// RED (U-19) — **Escape on the download's card hides the card and the
    /// download goes on.**
    ///
    /// Coordinator ruling 1 (2026-09-26): "Escape / close box on
    /// `Downloading` and `Staged` hides the card; the download goes on."
    /// Later is `Moves(same)` there: the job keeps its state, its offer and
    /// its presenter, its reports still move it, and no card is drawn.
    ///
    /// MUTATION: make **Later** on the download's card cancel it in
    /// `Job::answer_verb` (`(State::Downloading(..) | State::Staged(_),
    /// Verb::Later) => (State::Idle, …)`) and the job is gone.
    #[test]
    fn escape_during_download_hides_the_card_and_keeps_the_job() {
        let mut job = available("v0.4.7");
        let driver = Starting::default();
        job.answer_verb(Verb::Press, &driver, &Recording::default().shared())
            .expect("the press is taken");
        let post = driver.0.borrow_mut().take().expect("a poster");
        assert_eq!(job.card_window(), Some(1), "the download's card is up");
        assert_eq!(
            job.answer_verb(Verb::Later, &Unsupported, &Recording::default().shared()),
            Ok(Effect::None)
        );
        assert!(
            matches!(job.state(), State::Downloading(..)),
            "the download goes on: {:?}",
            job.state()
        );
        assert_eq!(job.card_window(), None, "the card is hidden");
        assert_eq!(job.presenter(), Some(1), "and keeps its window");
        let bytes = Bytes {
            received: 5,
            total: Some(10),
        };
        post.post(Step::Received(bytes));
        assert_eq!(job.drain_progress(), 0, "its reports still move it");
        assert!(matches!(job.state(), State::Downloading(_, got) if *got == bytes));
        assert_eq!(job.card_window(), None, "progress does not raise the card");
        post.post(Step::Staged);
        job.drain_progress();
        assert!(matches!(job.state(), State::Staged(_)));
        assert_eq!(job.card_window(), None);
        assert_eq!(
            job.answer_verb(Verb::Later, &Unsupported, &Recording::default().shared()),
            Ok(Effect::None),
            "Later on the staged card is the same"
        );
        assert!(matches!(job.state(), State::Staged(_)));
    }

    /// RED (U-19) — **a card hidden during the download comes back once,
    /// in its window, when the job reaches `Verified` or `Failed`.**
    ///
    /// Coordinator ruling 1: "the card is re-presented **once**, in the
    /// presenting window, when the job reaches `Verified` or `Failed`. No
    /// second card for the same state." Later on the re-presented verified
    /// card puts it away for good; the row's foot is the way back.
    ///
    /// MUTATION: drop the `put_away = false` on a move to `Verified` or
    /// `Failed` in `Job::apply` and the download's end is never said.
    #[test]
    fn the_card_is_re_presented_once_at_verified() {
        let mut job = available("v0.4.7");
        let driver = Starting::default();
        job.answer_verb(Verb::Press, &driver, &Recording::default().shared())
            .expect("the press is taken");
        let post = driver.0.borrow_mut().take().expect("a poster");
        job.answer_verb(Verb::Later, &Unsupported, &Recording::default().shared())
            .expect("Later hides the download's card");
        post.post(Step::Staged);
        job.drain_progress();
        assert_eq!(job.card_window(), None, "staged is not the end");
        post.post(Step::Verified);
        job.drain_progress();
        assert!(matches!(job.state(), State::Verified(_)));
        assert_eq!(job.card_window(), Some(1), "re-presented, in its window");
        job.answer_verb(Verb::Later, &Unsupported, &Recording::default().shared())
            .expect("Later on the verified card");
        assert!(matches!(job.state(), State::Verified(_)), "the job is kept");
        assert_eq!(job.card_window(), None);
        post.post(Step::Verified);
        assert_eq!(job.drain_progress(), 1, "a second Verified is stale");
        assert_eq!(job.card_window(), None, "no second card for the same state");

        // And a download that fails after its card was hidden says so.
        let mut failing = available("v0.4.7");
        let driver = Starting::default();
        failing
            .answer_verb(Verb::Press, &driver, &Recording::default().shared())
            .expect("the press is taken");
        let post = driver.0.borrow_mut().take().expect("a poster");
        failing
            .answer_verb(Verb::Later, &Unsupported, &Recording::default().shared())
            .expect("Later hides the download's card");
        post.post(Step::Stopped(Stop::Download));
        failing.drain_progress();
        assert!(matches!(failing.state(), State::Failed(..)));
        assert_eq!(failing.card_window(), Some(1), "the failure is said");
    }

    /// RED (U-19) — **when the presenting window closes, the card moves to
    /// the next active ordinary window — never the summoned terminal.**
    ///
    /// §B: "Closing the presenting window does not cancel a job that other
    /// windows can still be told about; it re-presents on the next active
    /// window"; coordinator ruling 5: the job keeps `presenter`, and
    /// [`Job::hand_over`] is the one method that moves it. Two ordinary
    /// windows and a summoned terminal that was visited after both: the card
    /// goes to the ordinary one. With no ordinary window left the card waits,
    /// and the next ordinary window to open takes it. A card the reader put
    /// away stays put away when it moves.
    ///
    /// MUTATION: pass `None` for `presenters.quake` in `Job::hand_over` and
    /// the card lands in the summoned terminal, 9.
    #[test]
    fn a_closed_presenter_hands_the_card_to_the_next_ordinary_window() {
        let mut job = job();
        job.consider(
            gathered("v0.4.7", None, Some(Channel::Ours)),
            &Presenters {
                visited: &[2, 9, 1],
                open: &[1, 2, 9],
                quake: Some(9),
            },
            || txn(1),
        );
        assert_eq!(job.card_window(), Some(1));
        let one_gone = Presenters {
            visited: &[2, 9],
            open: &[2, 9],
            quake: Some(9),
        };
        assert!(job.hand_over(&one_gone), "window 1 closed");
        assert_eq!(job.card_window(), Some(2), "not the summoned terminal");
        assert!(!job.hand_over(&one_gone), "a presenter still open stays");
        let only_summoned = Presenters {
            visited: &[9],
            open: &[9],
            quake: Some(9),
        };
        assert!(job.hand_over(&only_summoned), "window 2 closed");
        assert_eq!(
            job.card_window(),
            None,
            "no ordinary window: the card waits"
        );
        assert!(
            matches!(job.state(), State::Available(_)),
            "the job is kept"
        );
        assert!(job.hand_over(&Presenters {
            visited: &[9],
            open: &[9, 3],
            quake: Some(9),
        }));
        assert_eq!(job.card_window(), Some(3), "a new window takes it");

        // A hidden card moves hidden.
        let driver = Starting::default();
        job.answer_verb(Verb::Press, &driver, &Recording::default().shared())
            .expect("the press is taken");
        job.answer_verb(Verb::Later, &Unsupported, &Recording::default().shared())
            .expect("Later hides the download's card");
        assert!(job.hand_over(&one_gone), "window 3 closed");
        assert_eq!(job.presenter(), Some(2));
        assert_eq!(job.card_window(), None, "still put away");
        // And an idle job has no card to move.
        let mut idle = self::job();
        assert!(!idle.hand_over(&one_gone));
        assert_eq!(idle.presenter(), None);
    }

    /// RED (U-29) — **a launch a rollback sent stands at `Failed` from the
    /// start, with no offer: the card goes to the first ordinary window that
    /// opens, the evidence that lands later offers nothing, and Close puts it
    /// away.**
    ///
    /// §C.5: the restored old build is relaunched "with `--update-failed
    /// <journal>`, which raises the card at `Failed`". The failed transaction
    /// was an earlier launch's, so there is no offer to carry, and the card is
    /// this launch's one update card.
    ///
    /// MUTATION: in `Job::hand_over`, seat a card only for a state that
    /// carries an offer (the card never rises).
    #[test]
    fn a_launch_sent_by_a_rollback_raises_the_failed_card() {
        let folder = PathBuf::from("/Applications/.Folio.app.folio-update");
        let mut job = job().after_rollback(Some(Failure::Incomplete {
            folder: Some(folder.clone()),
            held: false,
            untried: false,
        }));
        assert_eq!(
            job.state(),
            &State::Failed(
                None,
                Failure::Incomplete {
                    folder: Some(folder),
                    held: false,
                    untried: false,
                }
            )
        );
        assert_eq!(job.card_window(), None, "no window yet");
        let first = Presenters {
            visited: &[],
            open: &[4],
            quake: None,
        };
        assert!(job.hand_over(&first));
        assert_eq!(job.card_window(), Some(4));
        assert_eq!(
            job.consider(
                gathered("v0.4.7", None, Some(Channel::Ours)),
                &first,
                || { txn(1) }
            ),
            None
        );
        assert!(matches!(job.state(), State::Failed(None, _)));
        job.answer_verb(Verb::Later, &Unsupported, &Recording::default().shared())
            .expect("Close is Later");
        assert_eq!(job.state(), &State::Idle);
        assert_eq!(job.card_window(), None);

        let plain = self::job().after_rollback(None);
        assert_eq!(plain.state(), &State::Pending(Pending::AwaitingBoth));
    }

    /// RED (U-35) — **a fallback trial's incomplete card follows its journal
    /// to `Updated` when that recorded trial commits**, just as the existing
    /// retrial over `Stuck` does.
    ///
    /// MUTATION: omit `Failure::TrialIncomplete` from `said_incomplete`.
    #[test]
    fn u35_a_fallback_trial_that_commits_reports_updated() {
        let mut job = job().after_rollback(Some(Failure::TrialIncomplete {
            folder: PathBuf::from("update-journal"),
        }));
        assert!(job.after_commit("0.4.7"));
        assert_eq!(
            job.state(),
            &State::Updated("0.4.7".to_owned(), crate::update_job::TrialChanges::Kept)
        );
    }

    /// An unfinished rollback's report, its folder not ASCII.
    fn incomplete() -> Failure {
        Failure::Incomplete {
            folder: Some(PathBuf::from(r"D:\工具\Folio 终端\.folio-update")),
            held: false,
            untried: false,
        }
    }

    /// RED (U-36) — **a report handed over raises its card in the window the
    /// launch landed in, over no card, an unpressed offer or an earlier
    /// failure, and the evidence that lands later does not take it down.**
    ///
    /// The receiving half of `launch_wire`'s report: the card the start would
    /// have shown cold, raised once, where the reader now is. An offer nobody
    /// pressed is not a transaction — its card gives way and its tag stays
    /// askable from About → Version once the failure is closed. The newest
    /// report wins over an earlier one, closed or not. And the launch's one
    /// unasked offer is spent, as at a start a rollback sent: a check landing
    /// after the report keeps the failure up.
    ///
    /// MUTATIONS: in `Job::told_by_a_launch`, leave `presenter` as it was (the
    /// card stays in window 1 or is not drawn); leave `offered_this_launch`
    /// unset in `told` (the later `consider` replaces the failure with an
    /// offer); assign `said_incomplete` in `told` instead of keeping it (a
    /// later report makes Update and restart askable over an incomplete one).
    #[test]
    fn a_report_handed_over_raises_its_card_where_the_launch_landed() {
        // No card yet: a launch still pending, and the evidence after it.
        let mut pending = job();
        assert!(pending.told_by_a_launch(Failure::RolledBack, Some(3)));
        assert_eq!(pending.card_window(), Some(3));
        assert_eq!(
            pending.consider(
                gathered("v0.4.8", None, Some(Channel::Ours)),
                &one_window(),
                || txn(9)
            ),
            None
        );
        assert_eq!(
            pending.state(),
            &State::Failed(None, Failure::RolledBack),
            "the check that lands after the report raises nothing over it"
        );

        // An offer nobody pressed gives way, and stays askable.
        let mut offered = available("v0.4.8");
        assert!(offered.told_by_a_launch(Failure::Interrupted, Some(2)));
        assert_eq!(offered.state(), &State::Failed(None, Failure::Interrupted));
        assert_eq!(offered.card_window(), Some(2));
        offered
            .answer_verb(Verb::Later, &Unsupported, &Recording::default().shared())
            .expect("Close is Later");
        assert!(
            offered.offer_again(2, Some("v0.4.8")),
            "the offer the report replaced is asked for from About → Version"
        );

        // An earlier report, its card closed: the newer report is raised.
        let mut earlier = available("v0.4.8");
        assert!(earlier.told_by_a_launch(incomplete(), Some(5)));
        earlier
            .answer_verb(Verb::Later, &Unsupported, &Recording::default().shared())
            .expect("Close is Later");
        assert_eq!(earlier.card_window(), None, "the earlier card was closed");
        assert!(earlier.told_by_a_launch(Failure::RolledBack, Some(6)));
        assert_eq!(earlier.state(), &State::Failed(None, Failure::RolledBack));
        assert_eq!(earlier.card_window(), Some(6));
        // And a third over the second, still up: the newest wins.
        assert!(earlier.told_by_a_launch(Failure::Interrupted, Some(5)));
        assert_eq!(earlier.state(), &State::Failed(None, Failure::Interrupted));
        assert_eq!(earlier.card_window(), Some(5));
        assert_eq!(
            earlier.asked_offer(Some("v0.4.8")),
            None,
            "being told an update is incomplete is not taken back by a later report"
        );

        // No window could be opened: the card waits for the next one.
        let mut nowhere = job();
        assert!(nowhere.told_by_a_launch(Failure::RolledBack, None));
        assert_eq!(nowhere.card_window(), None);
        assert!(nowhere.hand_over(&one_window()));
        assert_eq!(nowhere.card_window(), Some(1));
    }

    /// RED (U-36) — **a report handed over never disturbs a transaction this
    /// launch is running** (RULES §36): from the press to the quit, and while
    /// the launch pass settles an earlier launch's transaction, the state and
    /// its card stay; the report is kept as the launch's last failure, which
    /// About → Version names once the job is back at `Idle`.
    ///
    /// MUTATION: drop the `running` guard of `Job::told_by_a_launch` — the
    /// download's card is replaced by the failure and its driver is orphaned.
    #[test]
    fn a_report_handed_over_never_disturbs_a_running_transaction() {
        let mut job = available("v0.4.8");
        let driver = Starting::default();
        job.answer_verb(Verb::Press, &driver, &Recording::default().shared())
            .expect("the press is taken");
        assert!(matches!(job.state(), State::Downloading(..)));
        assert!(!job.told_by_a_launch(Failure::RolledBack, Some(4)));
        assert!(
            matches!(job.state(), State::Downloading(..)),
            "the download goes on"
        );
        assert_eq!(job.card_window(), Some(1), "its card stays where it was");
        assert_eq!(
            job.last_failure().map(|(_, failure)| failure),
            Some(&Failure::RolledBack)
        );
        job.answer_verb(Verb::Cancel, &Unsupported, &Recording::default().shared())
            .expect("Cancel");
        assert_eq!(job.state(), &State::Idle);
        assert!(
            job.show_failure(1),
            "About → Version's Details raises the report once the job is idle"
        );
        assert_eq!(job.state(), &State::Failed(None, Failure::RolledBack));

        // The launch pass settling an earlier launch's transaction.
        let home = crate::update_txn::Home::at(PathBuf::from(r"D:\工具\.folio-update"));
        let mut settling =
            self::job().after_start(Some(home), super::resumer_for_this_copy(), || {});
        assert!(!settling.told_by_a_launch(incomplete(), Some(4)));
        assert_eq!(settling.card_window(), None);
        assert_eq!(
            settling.last_failure().map(|(_, failure)| failure),
            Some(&incomplete())
        );
    }
}
