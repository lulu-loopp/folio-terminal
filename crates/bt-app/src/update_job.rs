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
//! [`Evidence::eligibility`] reads the switch, the offer decision
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
//! # No driver yet, and offers off
//!
//! A press reaches a [`Driver`]. The only one is [`Unsupported`], which refuses
//! before any network, staging, flush or wait: the job goes straight to
//! [`State::Failed`]. The drivers are U-20 (Windows) and U-27 (macOS); the quit
//! barrier is U-21. And no user sees any of this: [`Job::offers_enabled`] is a
//! constant `false` until the enabling tickets (U-31, U-32) turn it on, so the
//! job never leaves `Idle` in a shipped build. What it does do is decide, and
//! say what it decided once per launch in `diagnostics.log`.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the card's verbs and the drivers land after the job: U-19 presses, U-20/U-27 drive, U-21 quits ((b).5)"
    )
)]

use std::sync::{Arc, Mutex, OnceLock};

use bt_persist::UpdateCheckV1;
use bt_platform::HostPlatform;

use crate::install_channel::{Channel, Manager};
use crate::update::{Version, newer_than, should_offer};
use crate::update_txn::TxnId;

/// **Whether a reader may be offered an update at all** (U-18: off).
///
/// Turned on by the enabling tickets, one per platform (U-31 Windows, U-32
/// macOS), once a driver, the card and the recovery contract exist. Until then
/// the job decides and never offers.
const OFFERS_ENABLED: bool = false;

/// The host the two files of an offer are fetched from (C11). GitHub
/// redirects to its asset host; the redirect rules are the download door's
/// (`bt_platform::https_download`).
pub(crate) const RELEASE_HOST: &str = "github.com";

/// Where a release's files are, by tag: `<this>/<tag>/<name>` (C11). **Never
/// `/releases/latest/download/`**: "latest" can move under an open offer.
pub(crate) const RELEASE_DOWNLOAD_PATH: &str = "/lulu-loopp/folio-terminal/releases/download";

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
                format!("Folio-{to_version}-macos-arm64.dmg"),
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

    /// **The two files a press fetches, and nothing else** (C11): the asset and
    /// its checksum document, both by the offer's own tag.
    #[must_use]
    pub(crate) fn requests(&self) -> [Request; 2] {
        [&self.asset, &self.hash_doc].map(|name| Request {
            host: RELEASE_HOST,
            path: format!("{RELEASE_DOWNLOAD_PATH}/{}/{name}", self.tag),
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

/// One file a press fetches: `https://{host}{path}` into `file_name`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub(crate) host: &'static str,
    pub(crate) path: String,
    pub(crate) file_name: String,
}

// ── the evidence, and eligibility ───────────────────────────────────────────

/// **What the job is still waiting for** before it may decide anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pending {
    /// Neither the channel nor the check has arrived.
    AwaitingBoth,
    /// The check has answered; how this copy was installed has not been read.
    AwaitingClassification,
    /// The channel is known; the day's check has not settled.
    AwaitingCheck,
}

/// **The two facts that arrive off the window thread, as far as they have**,
/// and the three that are known at start.
#[derive(Clone, Debug)]
pub(crate) struct Gathered {
    /// The check's state and the switch, once this launch's check has settled
    /// ([`crate::update::job_evidence`]).
    pub(crate) check: Option<(UpdateCheckV1, bool)>,
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
            (Some((check, switch)), Some(channel)) => Ok(Evidence {
                switch,
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
    /// The reader's switch (Settings > General > update check).
    pub(crate) switch: bool,
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
    /// The reader turned the check off.
    SwitchOff,
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
            Self::Trial | Self::SwitchOff | Self::NoNewerRelease | Self::Skipped { .. } => {
                Route::Nothing
            }
            Self::Managed { command, .. } => Route::Command(command),
            Self::NotOurs | Self::Unknown | Self::NotUpdaterBuild | Self::NoAsset { .. } => {
                Route::ReleasesPage
            }
        }
    }

    /// The reason, in a few words, for `diagnostics.log` (no path, no account).
    #[must_use]
    pub(crate) fn why(&self) -> String {
        match self {
            Self::Trial => "this start is an update's trial".to_owned(),
            Self::SwitchOff => "the check is switched off".to_owned(),
            Self::NoNewerRelease => "no newer release is known".to_owned(),
            Self::Skipped { tag } => format!("{tag} is at or below the skipped tag"),
            Self::Managed { manager, command } => {
                format!("{} owns this copy's updates (`{command}`)", manager.name())
            }
            Self::NotOurs => "this copy belongs to another account".to_owned(),
            Self::Unknown => "how this copy was installed is unknown".to_owned(),
            Self::NotUpdaterBuild => "this build was made without the updater flag".to_owned(),
            Self::NoAsset { tag } => format!("no release file is named for {tag} here"),
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
    /// offers), then the switch, then whether there is anything newer, then who
    /// owns the copy, then the build, then the file.
    ///
    /// # Errors
    /// The first condition that does not hold.
    pub(crate) fn eligibility(&self) -> Result<Eligible, NotEligible> {
        if self.trial {
            return Err(NotEligible::Trial);
        }
        if !self.switch {
            return Err(NotEligible::SwitchOff);
        }
        let Some(tag) = should_offer(&self.check, self.running, self.switch) else {
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
            Channel::Managed { manager, .. } => {
                return Err(NotEligible::Managed {
                    manager,
                    command: manager_command(manager),
                });
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
    /// The driver stopped before the files were verified; nothing installed
    /// was changed.
    Stopped,
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
    /// The job stopped, and says why.
    Failed(Offer, Failure),
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
}

impl Kind {
    /// Every state.
    pub(crate) const ALL: [Self; 9] = [
        Self::Pending,
        Self::Idle,
        Self::Available,
        Self::Downloading,
        Self::Staged,
        Self::Verified,
        Self::Quitting,
        Self::Committing,
        Self::Failed,
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
        }
    }

    /// The offer this state carries, if any.
    #[must_use]
    pub(crate) const fn offer(&self) -> Option<&Offer> {
        match self {
            Self::Pending(_) | Self::Idle => None,
            Self::Available(offer)
            | Self::Downloading(offer, _)
            | Self::Staged(offer)
            | Self::Verified(offer)
            | Self::Quitting(offer)
            | Self::Committing(offer)
            | Self::Failed(offer, _) => Some(offer),
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
/// verb exists with no handler"). One row per pair, 9 × 5;
/// `every_card_state_has_a_handler_for_every_verb` holds [`Job::answer`] to it.
pub(crate) const TABLE: [(Kind, Verb, Handling); 45] = {
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
        // Cancel.
        (K::Downloading, V::Later, Refused(NotOnThisCard)),
        (K::Downloading, V::Skip, Refused(NotOnThisCard)),
        (K::Downloading, V::Press, Refused(NotOnThisCard)),
        (K::Downloading, V::Cancel, Moves(K::Idle)),
        (K::Downloading, V::Restart, Refused(NotOnThisCard)),
        // Still the download's card: Cancel.
        (K::Staged, V::Later, Refused(NotOnThisCard)),
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

/// How a driver fetches a file. The real one is the download door
/// (`bt_platform::https_download`), wired by the first driver (U-20).
pub(crate) trait Transport {
    /// Fetch one file.
    ///
    /// # Errors
    /// The door's refusal, as a sentence.
    fn fetch(&self, request: &Request) -> Result<(), String>;
}

/// Why a driver would not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// This build has no driver.
    Unsupported,
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
        transport: &dyn Transport,
        post: &Poster,
    ) -> Result<(), Refused>;
}

/// **The only driver there is**: it refuses before any network, staging,
/// flush or wait — it does not look at its arguments at all.
pub(crate) struct Unsupported;

impl Driver for Unsupported {
    fn prepare(&self, _: &Offer, _: &dyn Transport, _: &Poster) -> Result<(), Refused> {
        Err(Refused::Unsupported)
    }
}

// ── progress, and the stale-event rule ─────────────────────────────────────

/// What a driver (or the quit barrier) reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// More of the asset arrived.
    Received(Bytes),
    /// Both files are on disk.
    Staged,
    /// Both files passed verification.
    Verified,
    /// The driver stopped; nothing installed was changed.
    Stopped,
    /// The quit was cancelled or its save failed: back to `Verified` (§B).
    QuitAbandoned,
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

/// **A driver's way back to the job**: reports carry the transaction they
/// belong to, wait in the job's inbox, and wake the loop.
#[derive(Clone)]
pub(crate) struct Poster {
    txn: TxnId,
    inbox: Arc<Mutex<Vec<Progress>>>,
}

impl Poster {
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
    /// The window the card is raised in.
    presenter: Option<W>,
    /// The last decision, for U-19's row.
    answer: Option<Result<Eligible, NotEligible>>,
    /// Whether the decision has been written to `diagnostics.log`.
    said: bool,
    /// [`OFFERS_ENABLED`] in the product.
    offers: bool,
    /// Reports from a driver, waiting for the window thread.
    inbox: Arc<Mutex<Vec<Progress>>>,
}

impl<W: Copy + Eq> Default for Job<W> {
    fn default() -> Self {
        Self::with_offers(Self::offers_enabled())
    }
}

impl<W: Copy + Eq> Job<W> {
    /// **Whether offers reach anybody** — `false` until U-31 / U-32.
    #[must_use]
    pub(crate) const fn offers_enabled() -> bool {
        OFFERS_ENABLED
    }

    /// A job whose gate is `offers`; the product's is [`Self::offers_enabled`].
    #[must_use]
    fn with_offers(offers: bool) -> Self {
        Self {
            state: State::Pending(Pending::AwaitingBoth),
            offered_this_launch: false,
            presenter: None,
            answer: None,
            said: false,
            offers,
            inbox: Arc::new(Mutex::new(Vec::new())),
        }
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

    /// **Consider an offer** on what has been gathered — the handler of
    /// `AppEvent::UpdateJobOffer`.
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
        if self.offered_this_launch {
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
        self.state = State::Idle;
        let line = (!self.said).then(|| {
            self.said = true;
            line(&answer, self.offers)
        });
        if let (Ok(eligible), true) = (&answer, self.offers)
            && let Some(window) = crate::most_recently_active_window(
                presenters.visited,
                presenters.open,
                presenters.quake,
            )
            && let Some(offer) = Offer::mint(mint(), &eligible.tag, evidence.platform)
        {
            self.state = State::Available(offer);
            self.presenter = Some(window);
            self.offered_this_launch = true;
        }
        self.answer = Some(answer);
        line
    }

    /// **The reader turned the check off** (§B): an offer on the card is put
    /// away and a download is cancelled. A staged, verified or quitting job is
    /// not the switch's to undo.
    pub(crate) fn switch_off(&mut self) {
        if matches!(
            self.state,
            State::Available(_) | State::Downloading(..) | State::Staged(_)
        ) {
            self.state = State::Idle;
            self.presenter = None;
        }
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
            (State::Staged(offer), Step::Verified) => (State::Verified(offer), Applied::Moved),
            (State::Downloading(offer, _) | State::Staged(offer), Step::Stopped) => {
                (State::Failed(offer, Failure::Stopped), Applied::Moved)
            }
            (State::Quitting(offer), Step::QuitAbandoned) => {
                (State::Verified(offer), Applied::Moved)
            }
            (State::Quitting(offer), Step::SessionLanded) => {
                (State::Committing(offer), Applied::Moved)
            }
            (state, _) => (state, Applied::Stale),
        };
        self.state = next;
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
        transport: &dyn Transport,
    ) -> Result<Effect, Refusal> {
        let state = std::mem::replace(&mut self.state, State::Idle);
        let (next, outcome) = match (state, verb) {
            (state @ (State::Pending(_) | State::Idle), _) => (state, Err(Refusal::NoCard)),
            (State::Available(_), Verb::Later) => (State::Idle, Ok(Effect::None)),
            (State::Available(offer), Verb::Skip) => {
                (State::Idle, Ok(Effect::RecordSkip(offer.tag)))
            }
            (State::Available(offer), Verb::Press) => {
                let post = Poster {
                    txn: offer.txn,
                    inbox: Arc::clone(&self.inbox),
                };
                match driver.prepare(&offer, transport, &post) {
                    Ok(()) => (
                        State::Downloading(offer, Bytes::default()),
                        Ok(Effect::None),
                    ),
                    Err(Refused::Unsupported) => {
                        (State::Failed(offer, Failure::Unsupported), Ok(Effect::None))
                    }
                }
            }
            (state @ State::Available(_), Verb::Cancel | Verb::Restart)
            | (
                state @ (State::Downloading(..) | State::Staged(_)),
                Verb::Later | Verb::Skip | Verb::Press | Verb::Restart,
            )
            | (state @ State::Verified(_), Verb::Skip | Verb::Press | Verb::Cancel)
            | (
                state @ State::Failed(..),
                Verb::Skip | Verb::Press | Verb::Cancel | Verb::Restart,
            ) => (state, Err(Refusal::NotOnThisCard)),
            (State::Downloading(..) | State::Staged(_), Verb::Cancel) => {
                (State::Idle, Ok(Effect::None))
            }
            (state @ State::Verified(_), Verb::Later) => (state, Ok(Effect::None)),
            (State::Verified(offer), Verb::Restart) => (State::Quitting(offer), Ok(Effect::None)),
            (state @ State::Quitting(_), _) => (state, Err(Refusal::TheQuitAnswers)),
            (state @ State::Committing(_), _) => (state, Err(Refusal::Exiting)),
            (State::Failed(..), Verb::Later) => (State::Idle, Ok(Effect::None)),
        };
        if matches!(next, State::Idle) {
            self.presenter = None;
        }
        self.state = next;
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

    use bt_persist::UpdateCheckV1;
    use bt_platform::HostPlatform;

    use super::{
        Applied, Bytes, Driver, Effect, Evidence, Failure, Gathered, Handling, Job, Kind,
        NotEligible, Offer, Pending, Poster, Presenters, Refused, Request, Route, State, Step,
        TABLE, Transport, Unsupported, Verb,
    };
    use crate::install_channel::{self, Channel, Manager};
    use crate::update_txn::TxnId;

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

    /// Everything in: a newer tag, switch on, a flagged build, not a trial —
    /// with the channel as given.
    fn gathered(latest: &str, skipped: Option<&str>, channel: Option<Channel>) -> Gathered {
        Gathered {
            check: Some((check(Some(latest), skipped), true)),
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

    /// A job whose offers are on — the enabling tickets' job; the product's
    /// stays off (`offers_stay_off_until_the_enabling_tickets`).
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

    /// A transport that records every request and fetches nothing.
    #[derive(Default)]
    struct Recording(RefCell<Vec<Request>>);

    impl Transport for Recording {
        fn fetch(&self, request: &Request) -> Result<(), String> {
            self.0.borrow_mut().push(request.clone());
            Ok(())
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
            transport: &dyn Transport,
            post: &Poster,
        ) -> Result<(), Refused> {
            for request in offer.requests() {
                transport
                    .fetch(&request)
                    .expect("the recording transport fetches");
            }
            *self.0.borrow_mut() = Some(post.clone());
            Ok(())
        }
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
        job.answer_verb(Verb::Press, &driver, &Recording::default())
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
        job.answer_verb(Verb::Press, &driver, &Recording::default())
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
            job.answer_verb(Verb::Cancel, &Unsupported, &Recording::default()),
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
        live.answer_verb(Verb::Press, &Starting::default(), &Recording::default())
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
        job.answer_verb(Verb::Press, &driver, &transport)
            .expect("the press is taken");
        assert_eq!(
            transport.0.borrow().len(),
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
        job.answer_verb(Verb::Restart, &Unsupported, &Recording::default())
            .expect("Restart");
        assert_eq!(job.apply(report(Step::QuitAbandoned)), Applied::Moved);
        assert_eq!(job.state(), &State::Verified(offer.clone()));
        job.answer_verb(Verb::Restart, &Unsupported, &Recording::default())
            .expect("Restart");
        assert_eq!(job.apply(report(Step::SessionLanded)), Applied::Moved);
        assert_eq!(job.state(), &State::Committing(offer.clone()));

        let mut stopped = available("v0.4.7");
        stopped
            .answer_verb(Verb::Press, &Starting::default(), &Recording::default())
            .expect("the press is taken");
        assert_eq!(stopped.apply(report(Step::Stopped)), Applied::Moved);
        assert_eq!(stopped.state(), &State::Failed(offer, Failure::Stopped));
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
            Kind::Failed => State::Failed(offer, Failure::Unsupported),
        };
        job
    }

    /// RED (U-18) — **every verb on every state is handled: it moves the job
    /// or is a listed refusal.**
    ///
    /// §B: "There is no reachable state in which a card verb exists with no
    /// handler: the enumeration above is total and is gated by a test." The
    /// table has one row for each of the 45 pairs, and `Job::answer_verb` —
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
            let outcome = job.answer_verb(verb, &Unsupported, &transport);
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
                        .answer_verb(verb, &Starting::default(), &transport)
                        .expect("a driver that starts");
                    assert_eq!(started.state().kind(), Kind::Downloading);
                }
            }
        }
        // Skip is the one verb that asks the check's owner to write.
        let mut job = standing_in(Kind::Available);
        assert_eq!(
            job.answer_verb(Verb::Skip, &Unsupported, &Recording::default()),
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
            job.answer_verb(Verb::Press, &Unsupported, &transport),
            Ok(Effect::None)
        );
        assert_eq!(job.state(), &State::Failed(offer, Failure::Unsupported));
        assert!(
            transport.0.borrow().is_empty(),
            "the press fetched {:?}",
            transport.0.borrow()
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
        job.answer_verb(Verb::Later, &Unsupported, &Recording::default())
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
            switch: true,
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
    /// whose only window is the summoned one keeps its offer for a window.
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
        assert_eq!(
            alone.state(),
            &State::Idle,
            "a card in the summoned terminal"
        );
        alone.consider(
            gathered("v0.4.7", None, Some(Channel::Ours)),
            &Presenters {
                visited: &[9, 4],
                open: &[9, 4],
                quake: Some(9),
            },
            || txn(1),
        );
        assert_eq!(alone.presenter(), Some(4), "the offer waited for a window");
    }

    /// RED (U-18) — **offers stay off until the enabling tickets turn them on,
    /// and the decision is still said once.**
    ///
    /// No user sees an update card from this ticket: the job the application
    /// holds (`Job::default`) decides, writes one `diagnostics.log` line naming
    /// the answer — no path, no account — and stays `Idle`. U-31 (Windows) and
    /// U-32 (macOS) change the constant.
    ///
    /// MUTATION: set `OFFERS_ENABLED` to `true`.
    #[test]
    fn offers_stay_off_until_the_enabling_tickets() {
        assert!(
            !Job::<u32>::offers_enabled(),
            "U-18: offers stay off until U-31 / U-32 enable them"
        );
        let mut job: Job<u32> = Job::default();
        let line = job.consider(
            gathered("v0.4.7", None, Some(Channel::Ours)),
            &one_window(),
            || txn(1),
        );
        assert_eq!(job.state(), &State::Idle, "a card went up with offers off");
        assert_eq!(
            line.as_deref(),
            Some("Folio: update job — v0.4.7 would be offered; offers are off in this build")
        );
        assert_eq!(
            job.consider(
                gathered("v0.4.8", None, Some(Channel::Ours)),
                &one_window(),
                || txn(1)
            ),
            None,
            "the decision is said once per launch"
        );
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
        let base = std::env::temp_dir().join(format!("bt-update-job-seam-{}", std::process::id()));
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
}
