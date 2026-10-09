//! **The update card and About → Version** — what the reader sees of the update
//! job (0.4.6 ticket U-19; `docs/plans/design/self-update-2026-09-16.md` §B,
//! C2, C9 and the revision's "U-14 — The card and the row").
//!
//! The job ([`crate::update_job`]) owns every fact: the offer, the state, the
//! window the card is raised in, whether the reader put the card away. This
//! module owns none of them. It **reads** the job and answers four questions
//! about it, each as a pure function a test can hold without a window:
//!
//! - **What the card says** — [`paint`]: one line per state, C9's verbs, and
//!   for the download a bar (determinate with `12 / 41 MB` beside it, or the
//!   indeterminate bar alone when the length is unknown). `None` in every
//!   state C9 draws no card for. A job's card is [`paint_of`]: the state's,
//!   and the Ready card of a restart that did not happen says so first.
//! - **What a press on it asks** — [`CardVerb::asks`] (Escape and the close
//!   box are Later; a failed card's Close is Later too; Restart asks the
//!   application's quit, not the job alone) and [`spend`], which
//!   hands the job's one outside effect, Skip's, to the check's owner
//!   (`update::OfferState::skip`).
//! - **Where the keyboard's ring is** — [`Card`], the per-window hover and ring,
//!   which is the only state here and is about a window, not the job.
//! - **What the Version row ends with** — [`row_foot`]: the releases page as
//!   before, **Restart to update** while a job waits at `Verified`, and on a copy
//!   a package manager owns that manager's command with one **Copy** (C2: a
//!   managed copy gets no card).
//!
//! The geometry and the drawing are `restore.rs`'s, where every dialog of this
//! craft keeps its constants and its `.btn` (`restore::update_card_layout`); the
//! window's half — which rung of the key ladder, which window draws it — is
//! `runtime/update_card.rs`.
//!
//! **Who sees the card** is `update_job::Job::offers_enabled`: a Windows build
//! since U-31, a macOS build since U-32.

use std::path::PathBuf;

use crate::i18n::{self, Lang, Text};
use crate::update_job::{
    Bytes, Effect, Failure, Job, NotEligible, Offer, State, Stop, TrialChanges, Verb,
};

// ── the verbs ──────────────────────────────────────────────────────────────

/// **A button on the card** (C9's verbs column).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CardVerb {
    /// The press — C9's first verb.
    Update,
    Later,
    Skip,
    Cancel,
    Restart,
    /// A failed card's releases page. Opens a page and moves nothing.
    Releases,
    /// The card after a rollback that failed: the journal's folder.
    ShowFolder,
    /// A failed card's dismissal — Later, in the job's words.
    Close,
}

/// **What a card's verbs owe the chassis they are drawn in** — the update card's
/// [`CardVerb`], and the uninstaller's confirmation card's [`UninstallVerb`]
/// (T-UNINSTALL-UX): their words, which of them the card recommends, and which
/// one the `×` and `Escape` are.
pub(crate) trait Verbs: Copy + Eq + std::fmt::Debug {
    /// The word on the button.
    fn text(self) -> &'static str;
    /// **The verb drawn in the accent**, of the verbs a card carries, if any.
    fn recommended(verbs: &[Self]) -> Option<Self>;
    /// **What the `×` and `Escape` are** on this card.
    fn put_away() -> Self;
}

impl Verbs for CardVerb {
    fn text(self) -> &'static str {
        CardVerb::text(self)
    }

    /// C9's first, except `Cancel`: a card that recommended stopping its own
    /// download would be recommending the one press nobody raised it for.
    fn recommended(verbs: &[Self]) -> Option<Self> {
        verbs.first().copied().filter(|verb| *verb != Self::Cancel)
    }

    /// Later (§B).
    fn put_away() -> Self {
        Self::Later
    }
}

/// **A button on the uninstaller's confirmation card** (T-UNINSTALL-UX): two
/// verbs, the one the reader came for and the one that changes nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UninstallVerb {
    /// Quit, and uninstall as the process's last act.
    Uninstall,
    /// Put the card away; nothing changes.
    Cancel,
}

impl Verbs for UninstallVerb {
    fn text(self) -> &'static str {
        match self {
            Self::Uninstall => Text::UninstallCardUninstall,
            Self::Cancel => Text::UninstallCardCancel,
        }
        .text()
    }

    /// *Uninstall*, the verb the reader pressed `Uninstall…` for; the ring is
    /// not lit when the card rises, so `Enter` presses nothing until a key has
    /// lit it ([`Card`]).
    fn recommended(verbs: &[Self]) -> Option<Self> {
        verbs.iter().copied().find(|verb| *verb == Self::Uninstall)
    }

    /// Cancel: the `×` and `Escape` change nothing.
    fn put_away() -> Self {
        Self::Cancel
    }
}

/// **What the uninstaller's confirmation card says** (T-UNINSTALL-UX): the
/// question, then what happens to settings and data — the switch's answer —
/// and the two verbs, *Uninstall* first.
#[must_use]
pub(crate) fn uninstall_paint(remove_data: bool) -> Paint<UninstallVerb> {
    Paint {
        heading: Some(Text::UninstallCardHeading.text().to_owned()),
        bar: None,
        detail: Some(
            if remove_data {
                Text::UninstallCardRemoves
            } else {
                Text::UninstallCardKeeps
            }
            .text()
            .to_owned(),
        ),
        folder: None,
        verbs: vec![UninstallVerb::Uninstall, UninstallVerb::Cancel],
    }
}

impl CardVerb {
    /// The word on the button.
    #[must_use]
    pub(crate) fn text(self) -> &'static str {
        match self {
            Self::Update => Text::UpdateCardUpdate,
            Self::Later => Text::UpdateCardLater,
            Self::Skip => Text::UpdateCardSkip,
            Self::Cancel => Text::UpdateCardCancel,
            Self::Restart => Text::UpdateCardRestart,
            Self::Releases => Text::UpdateCardReleases,
            Self::ShowFolder => Text::UpdateCardShowFolder,
            Self::Close => Text::UpdateCardClose,
        }
        .text()
    }

    /// **What a press on this button asks, and of whom.** Close is Later
    /// (`update_job::Verb`'s own note). **Restart is the quit's**, not a verb
    /// the job answers alone: the job's Restart only moves it to `Quitting`,
    /// and the Folio it was pressed in must then be asked to quit with the
    /// update's reason (`App::restart_for_update`), or nothing leaves.
    #[must_use]
    pub(crate) const fn asks(self) -> Asks {
        match self {
            Self::Update => Asks::Job(Verb::Press),
            Self::Later | Self::Close => Asks::Job(Verb::Later),
            Self::Skip => Asks::Job(Verb::Skip),
            Self::Cancel => Asks::Job(Verb::Cancel),
            Self::Restart => Asks::Quit,
            Self::Releases => Asks::Releases,
            Self::ShowFolder => Asks::ShowFolder,
        }
    }
}

/// **Who answers a press on the card** ([`CardVerb::asks`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Asks {
    /// The job, with this verb (`Job::answer_verb`).
    Job(Verb),
    /// The application's quit, carrying the update (`App::restart_for_update`,
    /// which moves the job through `Job::restart`).
    Quit,
    /// The browser, with the releases page. Moves nothing.
    Releases,
    /// The file manager, at the journal's folder. Moves nothing.
    ShowFolder,
}

/// What a point on the card is over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Target<V = CardVerb> {
    /// The face, or the window beside it: answers nothing.
    Panel,
    /// The `×` — Later (§B); Cancel on the uninstaller's card.
    Close,
    Verb(V),
}

impl<V: Verbs> Target<V> {
    /// The verb a press on this target is, with the close box read as the
    /// card's own way of putting it away ([`Verbs::put_away`]).
    #[must_use]
    pub(crate) fn verb(self) -> Option<V> {
        match self {
            Self::Panel => None,
            Self::Close => Some(V::put_away()),
            Self::Verb(verb) => Some(verb),
        }
    }
}

// ── the paint model ────────────────────────────────────────────────────────

/// The download's bar — **new drawing**; nothing in this window drew progress
/// before it (§B).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Bar {
    /// The share received, `0.0 ..= 1.0`.
    Determinate(f32),
    /// The length is not known: the bar alone, no share and no number.
    Indeterminate,
}

/// **What a failed job did to the installed copy** — the line C9 puts after the
/// reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    /// `Nothing changed.` — the job stopped before anything installed moved.
    NothingChanged,
    /// `Previous version restored.` — the flip was rolled back
    /// (`Failure::RolledBack`, U-29).
    Restored,
    /// `Update incomplete.` and the journal's folder — the rollback did not
    /// finish (`Failure::Incomplete`, U-29). No folder when a report handed
    /// over named none this side accepts (U-36 round 2). `held`: and this
    /// session's changes are not kept (0.4.8 E1).
    Incomplete { folder: Option<PathBuf>, held: bool },
    /// An update this build cannot read whole is not finished, and the
    /// build that wrote it finishes it — at the next sign-in, or when the
    /// later `version` it names is started (`Failure::Newer`, 0.4.8 E1).
    /// `held` as for [`Outcome::Incomplete`].
    Newer {
        folder: Option<PathBuf>,
        version: Option<String>,
        held: bool,
    },
    /// The unfinished update's new build is running as its recorded trial
    /// because neither recovery launch could be made (U-35).
    Trial { folder: PathBuf },
}

/// **Everything the card draws, in the order it draws it** — decided here, from
/// the job alone, and measured and placed by `restore::update_card_layout`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Paint<V = CardVerb> {
    /// The state's one line, drawn in the title's weight. `None` on the
    /// download's card, whose first row is the bar.
    pub(crate) heading: Option<String>,
    pub(crate) bar: Option<Bar>,
    /// The quieter line under it: the megabytes, or what a failure did.
    pub(crate) detail: Option<String>,
    /// The journal's folder, after a rollback that did not finish.
    pub(crate) folder: Option<PathBuf>,
    /// C9's verbs, in C9's order; the first is the one the card recommends
    /// ([`Self::primary`]).
    pub(crate) verbs: Vec<V>,
}

impl<V: Verbs> Paint<V> {
    /// **The verb drawn in the accent** ([`Verbs::recommended`]) — on the update
    /// card C9's first, except `Cancel`.
    #[must_use]
    pub(crate) fn primary(&self) -> Option<V> {
        V::recommended(&self.verbs)
    }
}

/// **What the card says in this state**, or `None` where C9 draws no card:
/// `Pending`, `Idle`, `Quitting` (the ordinary quit's own card asks, unsaved
/// documents included) and `Committing`.
#[must_use]
pub(crate) fn paint(state: &State) -> Option<Paint> {
    let bare = |verbs: Vec<CardVerb>| Paint {
        heading: None,
        bar: None,
        detail: None,
        folder: None,
        verbs,
    };
    match state {
        State::Pending(_) | State::Idle | State::Quitting(_) | State::Committing(_) => None,
        State::Available(offer) => Some(Paint {
            heading: Some(format!("Folio {}", offer.to_version())),
            ..bare(vec![CardVerb::Update, CardVerb::Later, CardVerb::Skip])
        }),
        State::Downloading(_, bytes) => Some(match megabytes(*bytes) {
            Some((line, share)) => Paint {
                bar: Some(Bar::Determinate(share)),
                detail: Some(line),
                ..bare(vec![CardVerb::Cancel])
            },
            None => Paint {
                bar: Some(Bar::Indeterminate),
                ..bare(vec![CardVerb::Cancel])
            },
        }),
        // Both files are on disk and being verified: still the download's card,
        // with no number left to give.
        State::Staged(_) => Some(Paint {
            bar: Some(Bar::Indeterminate),
            ..bare(vec![CardVerb::Cancel])
        }),
        State::Verified(_) => Some(Paint {
            heading: Some(Text::UpdateCardReady.text().to_owned()),
            ..bare(vec![CardVerb::Restart, CardVerb::Later])
        }),
        State::Failed(_, failure) => Some(failed(&reason(failure), &outcome(failure))),
        // The trial over `Stuck` committed forward after its card said the
        // update was incomplete (U-32): the version this build is, and one
        // word.
        // Committed after its trial ended (0.4.8 E4, R3): the same card, and
        // the sentence that says the trial's changes were not kept.
        State::Updated(version, changes) => Some(Paint {
            heading: Some(format!("Folio {version}")),
            detail: Some(match changes {
                TrialChanges::Kept => Text::UpdateCardUpdated.text().to_owned(),
                TrialChanges::NotKept => {
                    i18n::update_card_trial_not_kept(Text::UpdateCardUpdated.text())
                }
            }),
            ..bare(vec![CardVerb::Close])
        }),
    }
}

/// **What the card of `job` says** — [`paint`] of its state, except the Ready
/// card of a staged set whose restart did not happen (0.4.8 E3,
/// `Job::restart_missed`): its heading is *The restart did not happen.*, the
/// Ready line follows as its detail, and the same Restart · Later are offered.
#[must_use]
pub(crate) fn paint_of<W: Copy + Eq>(job: &Job<W>) -> Option<Paint> {
    let paint = paint(job.state())?;
    if !job.restart_missed() {
        return Some(paint);
    }
    Some(Paint {
        heading: Some(Text::UpdateCardRestartMissed.text().to_owned()),
        detail: paint.heading,
        ..paint
    })
}

/// **A failed card**: the reason, then what the failure did (C9's three
/// shapes; coordinator ruling 11 — the reasons stay U-20's, the suffix is
/// C9's).
#[must_use]
pub(crate) fn failed(reason: &str, outcome: &Outcome) -> Paint {
    let (detail, folder, first) = match outcome {
        Outcome::NothingChanged => (
            Text::UpdateCardNothingChanged.text().to_owned(),
            None,
            CardVerb::Releases,
        ),
        Outcome::Restored => (
            Text::UpdateCardRestored.text().to_owned(),
            None,
            CardVerb::Releases,
        ),
        Outcome::Incomplete { folder, held } => (
            not_kept(Text::UpdateCardIncomplete.text().to_owned(), *held),
            folder.clone(),
            folder_verb(folder.as_ref()),
        ),
        Outcome::Newer {
            folder,
            version,
            held,
        } => (
            not_kept(
                match version {
                    Some(version) => i18n::update_card_newer(version),
                    None => Text::UpdateCardNewerUnnamed.text().to_owned(),
                },
                *held,
            ),
            folder.clone(),
            folder_verb(folder.as_ref()),
        ),
        Outcome::Trial { folder } => (
            Text::UpdateCardTrial.text().to_owned(),
            Some(folder.clone()),
            CardVerb::ShowFolder,
        ),
    };
    Paint {
        heading: Some(reason.to_owned()),
        bar: None,
        detail: Some(detail),
        folder,
        verbs: vec![first, CardVerb::Close],
    }
}

/// Show folder only with a folder to show: a report handed over without one
/// offers the releases page, as the other failures do (U-36 round 2).
fn folder_verb(folder: Option<&PathBuf>) -> CardVerb {
    if folder.is_some() {
        CardVerb::ShowFolder
    } else {
        CardVerb::Releases
    }
}

/// `detail`, and when this session's writes are held (0.4.8 E1) the
/// sentence that says its changes are not kept.
fn not_kept(detail: String, held: bool) -> String {
    if held {
        i18n::update_card_not_kept(&detail)
    } else {
        detail
    }
}

/// The reason a failure gives: one line per driver's reason (U-27 names the
/// macOS Prepare's, U-20 the Windows Prepare's shortfall).
fn reason(failure: &Failure) -> String {
    let text = match failure {
        Failure::Unsupported | Failure::Stopped(Stop::NotWritable | Stop::NotOurs) => {
            Text::UpdateFailedUnsupported
        }
        Failure::Stopped(Stop::Busy) => Text::UpdateFailedBusy,
        Failure::Stopped(Stop::Newer) => Text::UpdateFailedNewer,
        Failure::Stopped(Stop::Journal) => Text::UpdateFailedJournal,
        Failure::Stopped(Stop::Download | Stop::Cancelled) => Text::UpdateFailedStopped,
        Failure::Stopped(Stop::Sums) => Text::UpdateFailedSums,
        Failure::Stopped(Stop::Mount) => Text::UpdateFailedMount,
        Failure::Stopped(Stop::Identity) => Text::UpdateFailedIdentity,
        Failure::Stopped(Stop::Copy) => Text::UpdateFailedCopy,
        Failure::Stopped(Stop::Clone) => Text::UpdateFailedClone,
        Failure::Stopped(Stop::TooOld) => Text::UpdateFailedTooOld,
        Failure::RolledBack | Failure::Incomplete { untried: false, .. } => Text::UpdateFailedTrial,
        Failure::Incomplete { untried: true, .. } => Text::UpdateFailedUntried,
        Failure::JournalHeld { error, .. } => return i18n::update_failed_journal_held(error),
        // Never a failed card: the job raises it as `State::Updated`.
        Failure::ChangesNotKept { version } => return format!("Folio {version}"),
        Failure::TrialIncomplete { .. } | Failure::BesideTheTrial { .. } => {
            Text::UpdateFailedTrialRunning
        }
        Failure::Interrupted => Text::UpdateFailedInterrupted,
        Failure::Newer {
            version: Some(_), ..
        } => Text::UpdateFailedNewer,
        Failure::Newer { version: None, .. } => Text::UpdateFailedUnreadable,
        Failure::Stopped(Stop::Space { short_by }) => {
            return i18n::update_failed_space(&needed_megabytes(*short_by));
        }
    };
    text.text().to_owned()
}

/// **A shortfall in whole megabytes, rounded up** — the decimal unit the
/// progress line counts in ([`MEGABYTE`]); a shortfall of a single byte still
/// reads `1`, because it is still a shortfall.
fn needed_megabytes(bytes: u64) -> String {
    bytes.div_ceil(MEGABYTE).max(1).to_string()
}

/// What each failure did to the installed copy: a driver's stop and a missing
/// driver moved nothing (`update_job::Failure`'s own notes); a rollback put
/// the previous version back, or did not finish (U-29).
fn outcome(failure: &Failure) -> Outcome {
    match failure {
        // Another Folio's update holds the installation: it finishes it.
        Failure::Stopped(Stop::Newer) => Outcome::Newer {
            folder: None,
            version: None,
            held: false,
        },
        Failure::Unsupported | Failure::Stopped(_) => Outcome::NothingChanged,
        Failure::RolledBack | Failure::Interrupted => Outcome::Restored,
        Failure::Incomplete { folder, held, .. } => Outcome::Incomplete {
            folder: folder.clone(),
            held: *held,
        },
        Failure::JournalHeld { then, .. } => outcome(then),
        // Never a failed card ([`reason`]); the update was committed.
        Failure::ChangesNotKept { .. } => Outcome::NothingChanged,
        Failure::Newer {
            folder,
            version,
            held,
        } => Outcome::Newer {
            folder: folder.clone(),
            version: version.clone(),
            held: *held,
        },
        Failure::TrialIncomplete { folder } => Outcome::Trial {
            folder: folder.clone(),
        },
        // The update's reserved trial runs in another process; this session's
        // writes are held for its life (0.4.8 E3).
        Failure::BesideTheTrial { folder } => Outcome::Incomplete {
            folder: Some(folder.clone()),
            held: true,
        },
    }
}

/// One megabyte, as the line counts it: the decimal unit a download dialog
/// and a release page both use.
const MEGABYTE: u64 = 1_000_000;

/// **`12 / 41 MB` and the share the bar fills**, or `None` when the length is
/// unknown (the bar alone, indeterminate).
///
/// Received is rounded down and the total to the nearest megabyte, and the
/// received figure never reads past the total, so the line cannot say the
/// download is done before the job does.
#[must_use]
pub(crate) fn megabytes(bytes: Bytes) -> Option<(String, f32)> {
    let total = bytes.total.filter(|total| *total > 0)?;
    let total_mb = (total + MEGABYTE / 2) / MEGABYTE;
    let received_mb = (bytes.received / MEGABYTE).min(total_mb);
    let share = (bytes.received.min(total) as f64 / total as f64) as f32;
    Some((
        i18n::update_card_progress(&received_mb.to_string(), &total_mb.to_string()),
        share,
    ))
}

// ── the card in one window ─────────────────────────────────────────────────

/// **The card as one window holds it**: what the pointer is over and where
/// the keyboard's ring stands. Nothing about the job lives here.
///
/// **The ring is not lit when the card arrives**, and `Enter` presses only a
/// verb the ring stands on: the card rises by itself, while the reader may be
/// typing, and a return key already on its way must not start a download
/// (the PSReadLine invitation's rule for a card that writes).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Card<V = CardVerb> {
    hover: Option<Target<V>>,
    ring: Option<V>,
}

impl<V> Default for Card<V> {
    fn default() -> Self {
        Self {
            hover: None,
            ring: None,
        }
    }
}

impl<V: Verbs> Card<V> {
    #[must_use]
    pub(crate) const fn hover(&self) -> Option<Target<V>> {
        self.hover
    }

    /// Returns whether the drawing has to change.
    pub(crate) fn set_hover(&mut self, hover: Option<Target<V>>) -> bool {
        let changed = self.hover != hover;
        self.hover = hover;
        changed
    }

    /// The verb the ring stands on, if it stands on one of `verbs`.
    #[must_use]
    pub(crate) fn ring(&self, verbs: &[V]) -> Option<V> {
        self.ring.filter(|verb| verbs.contains(verb))
    }

    /// `Tab` / `Shift+Tab`: the ring lights, or moves, over the verbs in the
    /// order they are drawn (C9's order reversed: the recommended verb stands
    /// on the right).
    pub(crate) fn step_ring(&mut self, verbs: &[V], forward: bool) {
        let drawn: Vec<V> = verbs.iter().rev().copied().collect();
        if drawn.is_empty() {
            return;
        }
        let at = self
            .ring(verbs)
            .and_then(|verb| drawn.iter().position(|drawn| *drawn == verb));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => drawn.len() - 1,
            (Some(at), true) => (at + 1) % drawn.len(),
            (Some(at), false) => (at + drawn.len() - 1) % drawn.len(),
        };
        self.ring = Some(drawn[next]);
    }

    /// The card left this window, or changed its verbs: nothing is hovered
    /// and the ring is out.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}

/// **A key on the card.** Everything else is swallowed: the card holds the
/// keyboard while it is up, as the restore card does (0.4.5 ticket 57).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Key {
    /// `Escape` — Later (§B).
    Later,
    /// `Tab`, or `Shift+Tab` when `forward` is false.
    Step { forward: bool },
    /// `Enter` or `Space` — the verb the ring stands on, if it is lit.
    Press,
}

/// **The verb a key answers**, after the ring has moved (`Step`), or `None`.
pub(crate) fn key<V: Verbs>(card: &mut Card<V>, verbs: &[V], key: Key) -> Option<V> {
    match key {
        Key::Later => Some(V::put_away()),
        Key::Step { forward } => {
            card.step_ring(verbs, forward);
            None
        }
        Key::Press => card.ring(verbs),
    }
}

// ── Skip's one outside effect ──────────────────────────────────────────────

/// **Spend what a verb asked of somebody other than the job** — Skip's write,
/// handed to the check's one owner (`update::OfferState::skip`, U-6;
/// coordinator ruling 10). The card writes nothing itself.
///
/// # Errors
///
/// The owner's write.
pub(crate) fn spend(
    effect: Effect,
    skip: impl FnOnce(&str) -> Result<(), bt_persist::WriteError>,
) -> Result<(), bt_persist::WriteError> {
    match effect {
        Effect::None => Ok(()),
        Effect::RecordSkip(tag) => skip(&tag),
    }
}

// ── About > Version ────────────────────────────────────────────────────────

/// The control in About's Version row. This is deliberately a view model: the
/// job remains the owner and every press is routed back to its existing card
/// entry or to the check owner's shared worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum VersionControl {
    Check {
        enabled: bool,
    },
    UpdateAndRestart,
    /// A verified update waits for **Restart** (`State::Verified`): the press raises the card
    /// again (`Job::reopen`), whose own `Restart` then quits — so the verb wears the `…` of a
    /// verb that asks first.
    Restart,
    Progress(Bytes),
    /// The download is complete and the job is past it — verifying, or quitting to apply
    /// (`State::Staged`, `Quitting`, `Committing`): the bar, full. No verb.
    Downloaded,
    CopyCommand {
        command: &'static str,
    },
    /// Enabled while the job can raise the failed release's offer again
    /// ([`Job::asked_offer`]); a recovery that may still complete cannot be
    /// raced by a second attempt.
    Retry {
        enabled: bool,
    },
    /// The releases page, for a copy the job will not update itself (§D's
    /// adoption route, [`crate::update_job::Route::ReleasesPage`]).
    OpenReleases,
}

/// The stable names used by the six-state table test.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VersionControlKind {
    Check,
    UpdateAndRestart,
    Restart,
    Progress,
    Downloaded,
    CopyCommand,
    Retry,
    OpenReleases,
}

impl VersionControl {
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn kind(&self) -> VersionControlKind {
        match self {
            Self::Check { .. } => VersionControlKind::Check,
            Self::UpdateAndRestart => VersionControlKind::UpdateAndRestart,
            Self::Restart => VersionControlKind::Restart,
            Self::Progress(_) => VersionControlKind::Progress,
            Self::Downloaded => VersionControlKind::Downloaded,
            Self::CopyCommand { .. } => VersionControlKind::CopyCommand,
            Self::Retry { .. } => VersionControlKind::Retry,
            Self::OpenReleases => VersionControlKind::OpenReleases,
        }
    }

    #[must_use]
    pub(crate) fn text(&self) -> &'static str {
        match self {
            Self::Check { .. } => Text::VersionCheck,
            Self::UpdateAndRestart => Text::VersionUpdateAndRestart,
            Self::Restart => Text::VersionRestart,
            Self::Progress(_) | Self::Downloaded => return "",
            Self::CopyCommand { .. } => Text::VersionCopyCommand,
            Self::Retry { .. } => Text::VersionRetry,
            Self::OpenReleases => Text::VersionOpenReleases,
        }
        .text()
    }

    #[must_use]
    pub(crate) const fn enabled(&self) -> bool {
        !matches!(
            self,
            Self::Check { enabled: false }
                | Self::Retry { enabled: false }
                | Self::Progress(_)
                | Self::Downloaded
        )
    }

    /// Whether the control is answered outside this window.
    #[must_use]
    pub(crate) const fn leaves_window(&self) -> bool {
        matches!(self, Self::OpenReleases)
    }
}

/// The inline link beside the Version value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum VersionLink {
    WhatsNew { tag: String },
    Details,
}

impl VersionLink {
    /// The words the link is drawn as: `What's new` wears the `↗` of a page
    /// that opens outside the window (once, whichever column already carries
    /// it); `Details` reopens a card in this window and wears none.
    #[must_use]
    pub(crate) fn label(&self) -> String {
        match self {
            Self::WhatsNew { .. } => {
                let text = Text::VersionWhatsNew.text();
                if text.ends_with('↗') {
                    text.to_owned()
                } else {
                    format!("{text} ↗")
                }
            }
            Self::Details => Text::VersionDetails.text().to_owned(),
        }
    }
}

/// Everything About's Version row reads from the owners this frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VersionRow {
    pub(crate) value: String,
    pub(crate) control: VersionControl,
    pub(crate) link: Option<VersionLink>,
}

impl Default for VersionRow {
    fn default() -> Self {
        Self {
            value: format!(
                "{} · {}",
                crate::version::banner(),
                Text::VersionUpToDate.text()
            ),
            control: VersionControl::Check { enabled: true },
            link: None,
        }
    }
}

/// Derive the Version row from the existing check and job owners. No state is
/// cached here: a frame that sees new evidence or progress sees new words.
#[must_use]
pub(crate) fn version_row<W: Copy + Eq>(
    job: &Job<W>,
    check: crate::update::CheckView,
    offered: Option<&str>,
    now_ms: u64,
    lang: Lang,
) -> VersionRow {
    let running = crate::version::VERSION;
    let line = |state: String| format!("{} · {state}", crate::version::banner());

    if let Some((offer, outcome)) = version_failure(job) {
        let tag = offer
            .as_ref()
            .map(Offer::tag)
            .or(offered)
            .unwrap_or(running);
        return VersionRow {
            value: line(version_failed_in(lang, &outcome, tag)),
            control: VersionControl::Retry {
                enabled: job.asked_offer(offered).is_some(),
            },
            link: Some(VersionLink::Details),
        };
    }
    if let State::Downloading(offer, bytes) = job.state() {
        return VersionRow {
            value: line(crate::i18n::version_downloading_in(lang, offer.tag())),
            control: VersionControl::Progress(*bytes),
            link: None,
        };
    }
    // **Past the download, the row says what the job is doing now** (047-EXPERIENCE): the
    // files are being checked, or Folio is quitting to put them in place. The bar stays full —
    // every byte is on disk — and there is no verb, as there is none on the card.
    if let State::Staged(offer) = job.state() {
        return VersionRow {
            value: line(crate::i18n::version_verifying_in(lang, offer.tag())),
            control: VersionControl::Downloaded,
            link: None,
        };
    }
    if let State::Quitting(_) | State::Committing(_) = job.state() {
        return VersionRow {
            value: line(Text::VersionRestarting.in_lang(lang).to_owned()),
            control: VersionControl::Downloaded,
            link: None,
        };
    }
    // **Downloaded and verified, waiting for the restart**: the row says so, and its verb raises
    // the Ready card again rather than offering a download that already happened.
    if let State::Verified(offer) = job.state() {
        return VersionRow {
            value: line(crate::i18n::version_ready_in(lang, offer.tag())),
            control: VersionControl::Restart,
            link: Some(VersionLink::WhatsNew {
                tag: offer.tag().to_owned(),
            }),
        };
    }
    if matches!(job.state(), State::Pending(_)) {
        return VersionRow {
            value: line(Text::VersionChecking.in_lang(lang).to_owned()),
            control: VersionControl::Check { enabled: false },
            link: None,
        };
    }
    if let Some(tag) = job.state().offer().map(Offer::tag) {
        return VersionRow {
            value: line(crate::i18n::version_available_in(lang, tag)),
            control: VersionControl::UpdateAndRestart,
            link: Some(VersionLink::WhatsNew {
                tag: tag.to_owned(),
            }),
        };
    }
    // Idle with a decision: the control is the decision's actionable route
    // (R2) — the asked offer (R1), a manager's command, or the releases page
    // for every answer that kept that adoption route. The check's offer alone
    // never makes a verb.
    if let (Some(answer), Some(tag)) = (job.answer(), offered) {
        let control = match answer {
            Err(NotEligible::Managed { command, .. }) => {
                return VersionRow {
                    value: line(crate::i18n::version_managed_in(lang, tag, command)),
                    control: VersionControl::CopyCommand { command },
                    link: Some(VersionLink::WhatsNew {
                        tag: tag.to_owned(),
                    }),
                };
            }
            Ok(_) if job.asked_offer(offered).is_some() => Some(VersionControl::UpdateAndRestart),
            Ok(eligible) => (eligible.tag == tag).then_some(VersionControl::OpenReleases),
            Err(reason) => (reason.route() == crate::update_job::Route::ReleasesPage)
                .then_some(VersionControl::OpenReleases),
        };
        if let Some(control) = control {
            return VersionRow {
                value: line(crate::i18n::version_available_in(lang, tag)),
                control,
                link: Some(VersionLink::WhatsNew {
                    tag: tag.to_owned(),
                }),
            };
        }
    }
    if check.answered == Some(false) {
        return VersionRow {
            value: line(crate::i18n::version_last_checked_in(
                lang,
                check.checked_at_ms,
                now_ms,
            )),
            control: VersionControl::Check {
                enabled: !check.checking,
            },
            link: None,
        };
    }
    VersionRow {
        value: line(Text::VersionUpToDate.in_lang(lang).to_owned()),
        control: VersionControl::Check {
            enabled: !check.checking,
        },
        link: None,
    }
}

/// **The failure About → Version names, and what it did to the installed
/// copy**: the launch's last failure ([`Job::last_failure`]) while the job is
/// `Failed` or `Idle`, read through the same [`outcome`] the failed card draws
/// its second line from — nothing changed, the previous version restored, or
/// the update incomplete (T-UPDATE-FAILURE-COPY).
fn version_failure<W: Copy + Eq>(job: &Job<W>) -> Option<(&Option<Offer>, Outcome)> {
    if !matches!(job.state(), State::Failed(..) | State::Idle) {
        return None;
    }
    job.last_failure()
        .map(|(offer, failure)| (offer, outcome(failure)))
}

/// **Version's failed value**, one sentence per [`Outcome`]: a stop before
/// anything moved says only that `version` was not installed; a rollback says
/// the previous version was put back as well; an unfinished rollback says the
/// update is incomplete, as the card does.
#[must_use]
pub(crate) fn version_failed_in(lang: Lang, outcome: &Outcome, version: &str) -> String {
    match outcome {
        Outcome::NothingChanged => Text::VersionFailed,
        Outcome::Restored => Text::VersionFailedRestored,
        Outcome::Incomplete { .. } | Outcome::Newer { .. } | Outcome::Trial { .. } => {
            Text::VersionFailedIncomplete
        }
    }
    .in_lang(lang)
    .replace("{version}", version)
}

// ── the job posture watched by the window loop ────────────────────────────

/// **What the Version row's control asks for** (C9, C2).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum RowFoot {
    /// `Open releases page`, as the row has always offered — also for a copy
    /// that is not ours, not known, or has no release file here (§D).
    #[default]
    ReleasesPage,
    /// A job waits at `Verified`; the row's control is `Restart…`
    /// ([`VersionControl::Restart`]), whose press raises the Ready card again in
    /// the window it is pressed in (`dispatch_version_control`). `tag` is the offer's.
    Restart { tag: String },
    /// `Copy`: a package manager updates this copy, and this is its command.
    Copy { command: &'static str },
}

/// The job posture which can change About's Version row, read off the owner.
/// This stays language-free so the application loop can cheaply decide which
/// windows need a settings repaint.
#[must_use]
pub(crate) fn row_foot<W: Copy + Eq>(job: &Job<W>) -> RowFoot {
    if let State::Verified(offer) = job.state() {
        return RowFoot::Restart {
            tag: offer.tag().to_owned(),
        };
    }
    match job.answer() {
        Some(Err(NotEligible::Managed { command, .. })) => RowFoot::Copy { command },
        _ => RowFoot::ReleasesPage,
    }
}

// ── what the windows show, for the loop's once-a-turn comparison ───────────

/// **What the job shows this turn**: the card (its window and its paint) and
/// the row's foot. The loop keeps the last one and repaints only the windows
/// whose drawing it changed (`FolioApp::settle_update_card`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Shown<W> {
    pub(crate) card: Option<(W, Paint)>,
    pub(crate) foot: RowFoot,
    pub(crate) version: VersionPosture,
}

/// The language-free facts About > Version and the shared update mark are
/// drawn from ([`version_row`], `update::gear_mark_is_lit`), as the window loop
/// compares them once a turn: a change in any of them repaints every window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VersionPosture {
    pub(crate) state: crate::update_job::Kind,
    pub(crate) progress_megabytes: Option<(u64, Option<u64>)>,
    pub(crate) mark_lit: bool,
    /// The tag the check owner offers (`update::offer`).
    pub(crate) offered: Option<String>,
    /// Whether the job can raise that offer when asked ([`Job::asked_offer`]):
    /// Update and restart, or Retry enabled.
    pub(crate) asked: bool,
    /// A reader's Check is in flight (Check disabled).
    pub(crate) checking: bool,
    /// The "Last checked" form, while the last question got no answer — the
    /// displayed bucket, not the timestamp.
    pub(crate) last_checked: Option<crate::i18n::LastChecked>,
    /// What the failure Version names did to the installed copy
    /// ([`version_failure`]), while it names one.
    pub(crate) failed: Option<Outcome>,
}

impl<W> Default for Shown<W> {
    fn default() -> Self {
        Self {
            card: None,
            foot: RowFoot::default(),
            version: VersionPosture {
                state: crate::update_job::Kind::Pending,
                progress_megabytes: None,
                mark_lit: false,
                offered: None,
                asked: false,
                checking: false,
                last_checked: None,
                failed: None,
            },
        }
    }
}

/// What the job shows now, with the check facts Version reads beside it:
/// `mark_lit` (`update::gear_mark_is_lit`), `check` (`update::check_view`),
/// `offered` (`update::offer`) and the wall clock `now_ms`.
#[must_use]
pub(crate) fn shown<W: Copy + Eq>(
    job: &Job<W>,
    mark_lit: bool,
    check: crate::update::CheckView,
    offered: Option<&str>,
    now_ms: u64,
) -> Shown<W> {
    let progress_megabytes = match job.state() {
        State::Downloading(_, bytes) => Some((
            bytes.received / 1_000_000,
            bytes.total.map(|total| total / 1_000_000),
        )),
        State::Staged(_) | State::Quitting(_) | State::Committing(_) => Some((0, None)),
        _ => None,
    };
    Shown {
        card: job
            .card_window()
            .and_then(|window| paint(job.state()).map(|paint| (window, paint))),
        foot: row_foot(job),
        version: VersionPosture {
            state: job.state().kind(),
            progress_megabytes,
            mark_lit,
            offered: offered.map(str::to_owned),
            asked: job.asked_offer(offered).is_some(),
            checking: check.checking,
            last_checked: (check.answered == Some(false))
                .then(|| crate::i18n::LastChecked::at(check.checked_at_ms, now_ms)),
            failed: version_failure(job).map(|(_, outcome)| outcome),
        },
    }
}

/// Whether every open window must repaint its Version row and shared update
/// marks this turn.
#[must_use]
pub(crate) fn version_changed<W>(before: &Shown<W>, now: &Shown<W>) -> bool {
    before.foot != now.foot || before.version != now.version
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;

    use bt_persist::UpdateCheckV1;
    use bt_platform::HostPlatform;

    use super::{
        Asks, Bar, Card, CardVerb, Key, Outcome, Paint, RowFoot, Target, UninstallVerb,
        VersionControl, VersionControlKind, failed, key, paint, row_foot, shown, spend,
        uninstall_paint, version_changed, version_row,
    };
    use crate::i18n::{Lang, Text};
    use crate::install_channel::{Channel, Manager};
    use crate::update_job::{
        Bytes, Driver, Effect, Failure, Gathered, Job, NoDownloadDoor, Offer, Poster, Presenters,
        Refused, SharedTransport, State, Step, Unsupported, Verb,
    };

    /// The transport the verbs below never ask for a file.
    fn no_door() -> SharedTransport {
        std::sync::Arc::new(NoDownloadDoor)
    }
    use crate::update_txn::TxnId;

    /// Every fixture below runs this build and is offered a newer one.
    const RUNNING: &str = "0.4.6";

    fn gathered(latest: &str, channel: Channel) -> Gathered {
        Gathered {
            check: Some(UpdateCheckV1 {
                latest_tag: Some(latest.to_owned()),
                ..UpdateCheckV1::default()
            }),
            channel: Some(channel),
            running: RUNNING,
            capable: true,
            trial: false,
            platform: HostPlatform::Windows,
        }
    }

    /// Two ordinary windows, `1` visited last.
    fn windows() -> Presenters<'static, u32> {
        Presenters {
            visited: &[2, 1],
            open: &[1, 2],
            quake: None,
        }
    }

    /// A job with offers on (the enabling tickets' job) that has considered
    /// `latest` for a copy installed through `channel`.
    fn considered(latest: &str, channel: Channel) -> Job<u32> {
        let mut job = Job::with_offers(true);
        job.consider(gathered(latest, channel), &windows(), || {
            TxnId::new([7; 16])
        });
        job
    }

    /// A driver that starts and keeps its poster, so the test can report
    /// through it — the shape of the drivers to come (U-20, U-27).
    #[derive(Default)]
    struct Starting(RefCell<Option<Poster>>);

    impl Driver for Starting {
        fn prepare(&self, _: &Offer, _: &SharedTransport, post: &Poster) -> Result<(), Refused> {
            *self.0.borrow_mut() = Some(post.clone());
            Ok(())
        }
    }

    /// A job whose download of `v0.4.7` was pressed, and the poster its
    /// driver reports through.
    fn downloading() -> (Job<u32>, Poster) {
        let mut job = considered("v0.4.7", Channel::Ours);
        let driver = Starting::default();
        job.answer_verb(Verb::Press, &driver, &no_door())
            .expect("the press is taken");
        let post = driver.0.borrow_mut().take().expect("a poster");
        (job, post)
    }

    /// A job whose download of `v0.4.7` was verified.
    fn verified() -> Job<u32> {
        let (mut job, post) = downloading();
        post.post(Step::Staged);
        post.post(Step::Verified);
        assert_eq!(job.drain_progress(), 0);
        assert!(matches!(job.state(), State::Verified(_)));
        job
    }

    /// RED (T-UPDATE-ON-ABOUT) — the Version row has exactly six visible
    /// states, each with its exact value line and one control kind.
    ///
    /// Every value line starts with `version::banner()` — the same
    /// `Folio <version> (<commit>)` that `--version`, `diagnostics.log` and the
    /// hang reports print (round 2, R5).
    ///
    /// MUTATION: return `Check` for `VersionControl::UpdateAndRestart`; the
    /// `available` row goes red while the other five remain green. MUTATION
    /// (R5): build the line from `Folio {VERSION}` instead of the banner; all
    /// six rows go red.
    #[test]
    fn about_version_has_the_six_ruled_states() {
        use super::{VersionControlKind as Kind, version_row};
        use crate::update::CheckView;

        let banner = crate::version::banner();
        let up_to_date = considered(RUNNING, Channel::Ours);
        let available = considered("v0.4.7", Channel::Ours);
        let (downloading_job, _) = downloading();
        let managed = considered(
            "v0.4.7",
            Channel::Managed {
                manager: Manager::Scoop,
                uninstall_hook: true,
            },
        );
        let (mut failed_job, failed_post) = downloading();
        failed_post.post(Step::Stopped(crate::update_job::Stop::Download));
        assert_eq!(failed_job.drain_progress(), 0);
        let no_answer = considered(RUNNING, Channel::Ours);
        let now = 10 * 86_400_000;

        let rows = [
            (
                "up to date",
                &up_to_date,
                CheckView {
                    answered: Some(true),
                    ..CheckView::default()
                },
                None,
                format!("{banner} · Up to date"),
                Kind::Check,
            ),
            (
                "available",
                &available,
                CheckView::default(),
                Some("v0.4.7"),
                format!("{banner} · v0.4.7 available"),
                Kind::UpdateAndRestart,
            ),
            (
                "downloading",
                &downloading_job,
                CheckView::default(),
                Some("v0.4.7"),
                format!("{banner} · Downloading v0.4.7"),
                Kind::Progress,
            ),
            (
                "managed",
                &managed,
                CheckView::default(),
                Some("v0.4.7"),
                format!("{banner} · v0.4.7 available · scoop update folio"),
                Kind::CopyCommand,
            ),
            (
                "failed",
                &failed_job,
                CheckView::default(),
                Some("v0.4.7"),
                format!("{banner} · v0.4.7 was not installed."),
                Kind::Retry,
            ),
            (
                "no answer",
                &no_answer,
                CheckView {
                    checked_at_ms: now,
                    answered: Some(false),
                    ..CheckView::default()
                },
                None,
                format!("{banner} · Last checked: just now"),
                Kind::Check,
            ),
        ];
        for (name, job, check, offered, value, control) in rows {
            let row = version_row(job, check, offered, now, Lang::English);
            assert_eq!(row.value, value, "{name}");
            assert!(row.value.starts_with(&banner), "{name}: {}", row.value);
            assert_eq!(row.control.kind(), control, "{name}");
        }
    }

    /// RED (047-EXPERIENCE) — **About ▸ Version says what the update job is doing in every
    /// state, and offers only a verb that state has.** Each job is driven to its state through
    /// the job's own entries (the press, the driver's posts, `restart`, a rollback's report, a
    /// commit), never by writing the state.
    ///
    /// The rows that were wrong before: verifying and quitting said `Downloading` with a 35 %
    /// bar, and a verified update said `available · Update and restart` as though nothing had
    /// been downloaded.
    ///
    /// MUTATIONS, each observed red: (1) answer `State::Staged` with `version_downloading_in`
    /// again — the verifying row; (2) delete the `State::Verified` arm of `version_row` — the
    /// ready row (it falls to `available` and `UpdateAndRestart`); (3) give `Quitting` and
    /// `Committing` `VersionControl::Progress(Bytes::default())` again — both restarting rows.
    #[test]
    fn about_version_says_what_every_job_state_is_doing() {
        use super::{VersionControlKind as Kind, version_row};
        use crate::update::CheckView;

        let banner = crate::version::banner();
        let now = 10 * 86_400_000;
        let answered = CheckView {
            answered: Some(true),
            ..CheckView::default()
        };
        let unanswered = CheckView {
            checked_at_ms: now,
            answered: Some(false),
            ..CheckView::default()
        };
        let (mut verifying, post) = downloading();
        post.post(Step::Staged);
        assert_eq!(verifying.drain_progress(), 0);
        let (mut quitting, post) = downloading();
        post.post(Step::Staged);
        post.post(Step::Verified);
        assert_eq!(quitting.drain_progress(), 0);
        quitting.restart().expect("a verified job restarts");
        let (mut committing, post) = downloading();
        post.post(Step::Staged);
        post.post(Step::Verified);
        assert_eq!(committing.drain_progress(), 0);
        committing.restart().expect("a verified job restarts");
        post.post(Step::SessionLanded);
        assert_eq!(committing.drain_progress(), 0);
        let (mut stopped, post) = downloading();
        post.post(Step::Stopped(crate::update_job::Stop::Download));
        assert_eq!(stopped.drain_progress(), 0);
        let after =
            |failure: Failure| considered("v0.4.7", Channel::Ours).after_rollback(Some(failure));
        let mut updated =
            considered(RUNNING, Channel::Ours).after_rollback(Some(Failure::Incomplete {
                folder: Some(PathBuf::from("journal")),
                held: false,
                untried: false,
            }));
        assert!(
            updated.after_commit(RUNNING),
            "the commit replaces the card"
        );

        /// A state's name, its job, the check beside it, the offered tag, the line and the control.
        type Row = (
            &'static str,
            Job<u32>,
            CheckView,
            Option<&'static str>,
            String,
            Kind,
        );
        let rows: Vec<Row> = vec![
            (
                "checking",
                Job::with_offers(true),
                CheckView::default(),
                Some("v0.4.7"),
                format!("{banner} · Checking…"),
                Kind::Check,
            ),
            (
                "up to date",
                considered(RUNNING, Channel::Ours),
                answered,
                None,
                format!("{banner} · Up to date"),
                Kind::Check,
            ),
            (
                "no answer",
                considered(RUNNING, Channel::Ours),
                unanswered,
                None,
                format!("{banner} · Last checked: just now"),
                Kind::Check,
            ),
            (
                "offer",
                considered("v0.4.7", Channel::Ours),
                CheckView::default(),
                Some("v0.4.7"),
                format!("{banner} · v0.4.7 available"),
                Kind::UpdateAndRestart,
            ),
            (
                "downloading",
                downloading().0,
                CheckView::default(),
                Some("v0.4.7"),
                format!("{banner} · Downloading v0.4.7"),
                Kind::Progress,
            ),
            (
                "verifying",
                verifying,
                CheckView::default(),
                Some("v0.4.7"),
                format!("{banner} · Verifying v0.4.7"),
                Kind::Downloaded,
            ),
            (
                "ready to restart",
                verified(),
                CheckView::default(),
                Some("v0.4.7"),
                format!("{banner} · v0.4.7 ready"),
                Kind::Restart,
            ),
            (
                "quitting to apply",
                quitting,
                CheckView::default(),
                Some("v0.4.7"),
                format!("{banner} · Restarting…"),
                Kind::Downloaded,
            ),
            (
                "handing over",
                committing,
                CheckView::default(),
                Some("v0.4.7"),
                format!("{banner} · Restarting…"),
                Kind::Downloaded,
            ),
            (
                "failed, nothing changed",
                stopped,
                CheckView::default(),
                Some("v0.4.7"),
                format!("{banner} · v0.4.7 was not installed."),
                Kind::Retry,
            ),
            (
                "failed, restored",
                after(Failure::RolledBack),
                CheckView::default(),
                Some("v0.4.7"),
                format!("{banner} · v0.4.7 was not installed. The previous version was restored."),
                Kind::Retry,
            ),
            (
                "failed, incomplete",
                after(Failure::Incomplete {
                    folder: Some(PathBuf::from("journal")),
                    held: false,
                    untried: false,
                }),
                CheckView::default(),
                Some("v0.4.7"),
                format!("{banner} · The update to v0.4.7 is incomplete."),
                Kind::Retry,
            ),
            (
                "trial",
                after(Failure::TrialIncomplete {
                    folder: PathBuf::from("journal"),
                }),
                CheckView::default(),
                Some("v0.4.7"),
                format!("{banner} · The update to v0.4.7 is incomplete."),
                Kind::Retry,
            ),
            (
                "updated by the trial's commit",
                updated,
                answered,
                None,
                format!("{banner} · Up to date"),
                Kind::Check,
            ),
        ];
        for (name, job, check, offered, value, control) in &rows {
            let row = version_row(job, *check, *offered, now, Lang::English);
            assert_eq!(&row.value, value, "{name}");
            assert_eq!(row.control.kind(), *control, "{name}");
        }
        // Every job state is on the table.
        let kinds: std::collections::HashSet<_> =
            rows.iter().map(|(_, job, ..)| job.state().kind()).collect();
        for kind in crate::update_job::Kind::ALL {
            assert!(kinds.contains(&kind), "{kind:?} has no row");
        }
    }

    /// RED (T-UPDATE-FAILURE-COPY; the owner's ruling of 2026-10-04) —
    /// **Version's failed line says what the failure did to the installed
    /// copy**, read from the same `outcome` the failed card draws its second
    /// line from: a stop before anything moved says only that the version was
    /// not installed; a rollback (after a trial, or after the moves were
    /// interrupted) says the previous version was restored; a rollback that
    /// did not finish says the update is incomplete, with Retry disabled.
    ///
    /// MUTATIONS, each observed red: (1) draw `Text::VersionFailed` for every
    /// outcome in `version_failed_in` — the three restored and incomplete rows
    /// fail; (2) draw `VersionFailedRestored` for `Outcome::Incomplete` — the
    /// incomplete row fails; (3) map `Failure::Interrupted` to
    /// `Outcome::NothingChanged` in `outcome` — the interrupted row fails.
    #[test]
    fn about_version_says_what_each_failure_did() {
        use crate::update::CheckView;

        let banner = crate::version::banner();
        let (mut stopped, post) = downloading();
        post.post(Step::Stopped(crate::update_job::Stop::Sums));
        assert_eq!(stopped.drain_progress(), 0);
        let after =
            |failure: Failure| considered("v0.4.7", Channel::Ours).after_rollback(Some(failure));
        let rows = [
            (
                "stopped",
                stopped,
                format!("{banner} · v0.4.7 was not installed."),
                true,
            ),
            (
                "rolled back",
                after(Failure::RolledBack),
                format!("{banner} · v0.4.7 was not installed. The previous version was restored."),
                true,
            ),
            (
                "interrupted",
                after(Failure::Interrupted),
                format!("{banner} · v0.4.7 was not installed. The previous version was restored."),
                true,
            ),
            (
                "incomplete",
                after(Failure::Incomplete {
                    folder: Some(PathBuf::from("journal")),
                    held: false,
                    untried: false,
                }),
                format!("{banner} · The update to v0.4.7 is incomplete."),
                false,
            ),
        ];
        for (name, job, value, retry) in rows {
            assert!(matches!(job.state(), State::Failed(..)), "{name}");
            let row = version_row(&job, CheckView::default(), Some("v0.4.7"), 0, Lang::English);
            assert_eq!(row.value, value, "{name}");
            assert_eq!(
                row.control,
                VersionControl::Retry { enabled: retry },
                "{name}"
            );
        }
    }

    /// RED (T-UPDATE-FAILURE-COPY) — **what a failure did reaches the
    /// once-a-turn comparison**: two failed jobs that differ only in it (same
    /// state kind, offer, asked offer, mark and check) draw different Version
    /// lines, so the posture differs.
    ///
    /// MUTATION: build `failed: None` in `shown`; observed red.
    #[test]
    fn version_posture_carries_what_the_failure_did() {
        use crate::update::CheckView;

        let (mut stopped, post) = downloading();
        post.post(Step::Stopped(crate::update_job::Stop::Sums));
        assert_eq!(stopped.drain_progress(), 0);
        let rolled_back =
            considered("v0.4.7", Channel::Ours).after_rollback(Some(Failure::RolledBack));
        let before = shown(&stopped, true, CheckView::default(), Some("v0.4.7"), 0);
        let now = shown(&rolled_back, true, CheckView::default(), Some("v0.4.7"), 0);
        assert_eq!(before.version.state, now.version.state);
        assert_eq!(before.version.asked, now.version.asked);
        assert_eq!(before.foot, now.foot);
        assert!(version_changed(&before, &now));
    }

    /// RED (T-UPDATE-ON-ABOUT round 2, R2) — **Version derives its control
    /// from the job's actionable state and route, never from the check's offer
    /// by itself.** Pending names its state with no verb; a check in flight
    /// disables Check; every non-manager ineligible answer that kept the
    /// releases-page adoption route — and an eligible answer in a build whose
    /// offers are off — opens that page.
    ///
    /// MUTATION: restore round 1's `job.state().offer().map(...).or(offered)`
    /// projection ahead of the answer; Pending and every ineligible row
    /// become `UpdateAndRestart` and fail this table.
    #[test]
    fn pending_and_ineligible_jobs_keep_their_actionable_version_routes() {
        use crate::update::CheckView;

        let pending = Job::<u32>::with_offers(true);
        let pending_row = version_row(
            &pending,
            CheckView::default(),
            Some("v0.4.7"),
            0,
            Lang::English,
        );
        assert!(
            pending_row.value.ends_with(" · Checking…"),
            "{}",
            pending_row.value
        );
        assert_eq!(
            pending_row.control,
            VersionControl::Check { enabled: false }
        );
        assert!(!pending_row.control.enabled());

        let current = considered(RUNNING, Channel::Ours);
        let in_flight = version_row(
            &current,
            CheckView {
                checking: true,
                ..CheckView::default()
            },
            None,
            0,
            Lang::English,
        );
        assert_eq!(in_flight.control, VersionControl::Check { enabled: false });

        let cases = [
            (
                "not ours",
                Gathered {
                    channel: Some(Channel::NotOurs),
                    ..gathered("v0.4.7", Channel::Ours)
                },
                true,
            ),
            (
                "unknown",
                Gathered {
                    channel: Some(Channel::Unknown),
                    ..gathered("v0.4.7", Channel::Ours)
                },
                true,
            ),
            (
                "not an updater build",
                Gathered {
                    capable: false,
                    ..gathered("v0.4.7", Channel::Ours)
                },
                true,
            ),
            (
                "no asset",
                Gathered {
                    platform: HostPlatform::OtherUnix,
                    ..gathered("v0.4.7", Channel::Ours)
                },
                true,
            ),
            ("offers off", gathered("v0.4.7", Channel::Ours), false),
        ];
        for (name, evidence, offers) in cases {
            let mut job = Job::with_offers(offers);
            job.consider(evidence, &windows(), || TxnId::new([9; 16]));
            assert!(matches!(job.state(), State::Idle), "{name}");
            let row = version_row(&job, CheckView::default(), Some("v0.4.7"), 0, Lang::English);
            assert_eq!(row.control, VersionControl::OpenReleases, "{name}");
            assert!(row.control.leaves_window(), "{name}");
            assert_eq!(row.control.text(), "Open the release page", "{name}");
            assert!(
                row.value.ends_with("v0.4.7 available"),
                "{name}: {}",
                row.value
            );
            assert!(
                !job.offer_again(2, Some("v0.4.7")),
                "{name}: no asked offer"
            );
        }
    }

    /// RED (T-UPDATE-ON-ABOUT round 2, blocker 4) — **the once-per-turn
    /// comparison sends Version changes to every window, not only the card's
    /// presenter.** Window A (`1`) presents the card; window B has About open.
    /// A 1 MB → 20 MB move and Downloading → Failed each change what B draws —
    /// its Version row and control, and the one lit fact its gear, its About
    /// entry and its Version title wear — while the row's foot stays the
    /// releases page, so the comparison must see the Version posture.
    ///
    /// MUTATION: compare only `foot` in `version_changed` (round 1's
    /// comparison); both "window B must repaint" assertions go red.
    #[test]
    fn another_windows_about_repaints_for_progress_failure_and_all_update_marks() {
        let row = |job: &Job<u32>| {
            version_row(
                job,
                crate::update::CheckView::default(),
                Some("v0.4.7"),
                0,
                Lang::English,
            )
        };
        let (mut job, post) = downloading();
        assert_eq!(job.card_window(), Some(1), "window A presents the card");
        post.post(Step::Received(Bytes {
            received: 1_000_000,
            total: Some(40_000_000),
        }));
        assert_eq!(job.drain_progress(), 0);
        let one = shown(
            &job,
            crate::update::gear_mark_is_lit(&job),
            crate::update::CheckView::default(),
            Some("v0.4.7"),
            0,
        );
        let one_row = row(&job);

        post.post(Step::Received(Bytes {
            received: 20_000_000,
            total: Some(40_000_000),
        }));
        assert_eq!(job.drain_progress(), 0);
        let twenty = shown(
            &job,
            crate::update::gear_mark_is_lit(&job),
            crate::update::CheckView::default(),
            Some("v0.4.7"),
            0,
        );
        let twenty_row = row(&job);
        assert_eq!(one.foot, twenty.foot, "the foot alone would not repaint B");
        assert!(version_changed(&one, &twenty), "window B must repaint");
        assert_ne!(one_row.control, twenty_row.control);

        post.post(Step::Stopped(crate::update_job::Stop::Download));
        assert_eq!(job.drain_progress(), 0);
        let failed = shown(
            &job,
            crate::update::gear_mark_is_lit(&job),
            crate::update::CheckView::default(),
            Some("v0.4.7"),
            0,
        );
        let failed_row = row(&job);
        assert_eq!(
            twenty.foot, failed.foot,
            "the foot alone would not repaint B"
        );
        assert!(version_changed(&twenty, &failed), "window B must repaint");
        assert_eq!(failed_row.control, VersionControl::Retry { enabled: true });
        assert!(
            !twenty.version.mark_lit && failed.version.mark_lit,
            "B's gear, About entry and Version title wear the one lit fact"
        );
    }

    /// RED (T-UPDATE-ON-ABOUT round 3, B1) — **every fact Version is drawn
    /// from reaches the once-per-turn comparison, not only the job's state.**
    /// Window B has About open while each of these lands elsewhere:
    ///
    /// 1. a launch sent by a finished rollback (`--update-failed`, U-29) shows
    ///    the failure with Retry disabled; the startup check lands, the job's
    ///    refreshed decision lets it raise the offer, and Retry enables — the
    ///    state stays `Failed` and the mark stays lit;
    /// 2. after Later, a reader's Check learns a newer tag — the job stays
    ///    `Idle` and can still raise an offer, but B must name the new tag;
    /// 3. a reader's Check pressed in window A disables B's Check while in
    ///    flight;
    /// 4. the "Last checked" line moves from one displayed form to the next,
    ///    while a clock tick inside one form repaints nothing.
    ///
    /// MUTATIONS (each drops one field from the comparison by building it as a
    /// constant in `shown`): `asked: false` reddens 1; `offered: None`
    /// reddens 2; `checking: false` reddens 3; `last_checked: None` reddens 4.
    #[test]
    fn another_windows_about_repaints_for_the_decision_and_the_check() {
        use crate::update::CheckView;

        let posture = |job: &Job<u32>, offered: Option<&str>, check: CheckView, now: u64| {
            shown(
                job,
                crate::update::gear_mark_is_lit(job),
                check,
                offered,
                now,
            )
        };
        let row = |job: &Job<u32>, offered: Option<&str>, check: CheckView, now: u64| {
            version_row(job, check, offered, now, Lang::English)
        };
        let quiet = CheckView::default();

        // 1. `--update-failed` → the check lands → Retry enables.
        let mut rolled_back = Job::with_offers(true).after_rollback(Some(Failure::RolledBack));
        let before = posture(&rolled_back, Some("v0.4.7"), quiet, 0);
        assert_eq!(
            row(&rolled_back, Some("v0.4.7"), quiet, 0).control,
            VersionControl::Retry { enabled: false }
        );
        rolled_back.consider(gathered("v0.4.7", Channel::Ours), &windows(), || {
            TxnId::new([5; 16])
        });
        let after = posture(&rolled_back, Some("v0.4.7"), quiet, 0);
        assert_eq!(
            row(&rolled_back, Some("v0.4.7"), quiet, 0).control,
            VersionControl::Retry { enabled: true }
        );
        assert_eq!(before.version.state, after.version.state);
        assert_eq!(before.version.mark_lit, after.version.mark_lit);
        assert!(
            version_changed(&before, &after),
            "B must repaint: Retry enabled"
        );

        // 2. After Later, a Check learns a newer tag.
        let mut later = considered("v0.4.7", Channel::Ours);
        later
            .answer_verb(Verb::Later, &Starting::default(), &no_door())
            .expect("Later");
        let before = posture(&later, Some("v0.4.7"), quiet, 0);
        later.consider(gathered("v0.4.8", Channel::Ours), &windows(), || {
            TxnId::new([6; 16])
        });
        let after = posture(&later, Some("v0.4.8"), quiet, 0);
        assert!(before.version.asked && after.version.asked);
        assert!(
            row(&later, Some("v0.4.8"), quiet, 0)
                .value
                .ends_with("v0.4.8 available")
        );
        assert!(
            version_changed(&before, &after),
            "B must repaint: the newer tag"
        );

        // 3. A Check in flight from window A.
        let current = considered(RUNNING, Channel::Ours);
        let asking = CheckView {
            checking: true,
            ..quiet
        };
        let before = posture(&current, None, quiet, 0);
        let after = posture(&current, None, asking, 0);
        assert_eq!(
            row(&current, None, asking, 0).control,
            VersionControl::Check { enabled: false }
        );
        assert!(
            version_changed(&before, &after),
            "B must repaint: Check in flight"
        );

        // 4. "Last checked" moves by its displayed form, not by the clock.
        let unanswered = CheckView {
            checked_at_ms: 1_000_000,
            answered: Some(false),
            ..quiet
        };
        let minute = 60_000;
        let one = posture(&current, None, unanswered, 1_000_000 + minute + 1);
        let one_later = posture(&current, None, unanswered, 1_000_000 + 2 * minute - 1);
        let two = posture(&current, None, unanswered, 1_000_000 + 2 * minute);
        assert_eq!(
            row(&current, None, unanswered, 1_000_000 + 2 * minute).value,
            format!("{} · Last checked: 2 minutes ago", crate::version::banner())
        );
        assert!(
            !version_changed(&one, &one_later),
            "a tick inside one form repaints nothing"
        );
        assert!(
            version_changed(&one, &two),
            "B must repaint: 1 → 2 minutes ago"
        );
    }

    /// RED (T-UPDATE-ON-ABOUT round 2, review 7) — **the lit gear's tip names
    /// the release the job failed to install, after its card is closed too**:
    /// the tag the chrome's tip is built from (`update::gear_mark_tag`, read by
    /// `runtime/tooltips.rs`) is the failed offer's, in both languages.
    ///
    /// MUTATION: make `gear_mark_tag` answer `None` for a remembered failure;
    /// the tip falls back to the bare "Settings" and goes red.
    #[test]
    fn the_lit_gear_tip_names_the_failed_release() {
        let (mut job, post) = downloading();
        post.post(Step::Stopped(crate::update_job::Stop::Download));
        assert_eq!(job.drain_progress(), 0);
        for close in [false, true] {
            if close {
                job.answer_verb(Verb::Later, &Starting::default(), &no_door())
                    .expect("close the failed card");
            }
            assert!(crate::update::gear_mark_is_lit(&job), "closed: {close}");
            let tag = crate::update::gear_mark_tag(&job);
            for (lang, tip) in [
                (Lang::English, "Settings · v0.4.7 available"),
                (Lang::Chinese, "设置 · 有新版 v0.4.7"),
            ] {
                assert_eq!(
                    crate::i18n::version_settings_tip_in(lang, tag.as_deref()),
                    tip,
                    "closed: {close}"
                );
            }
        }
    }

    /// RED (U-19) — **the offer's card names the version, and its verbs are
    /// C9's: Update, Later, Skip — Update recommended.**
    ///
    /// C9: "A state is a word or a number; no sentence the reader did not
    /// need." The line is the product and the version the offer carries (the
    /// tag's `to_version`, never the raw tag); the highlight line (R-17) is
    /// dropped.
    ///
    /// MUTATION: draw the offer's tag instead of its `to_version` in `paint`
    /// and the heading reads `Folio v0.4.7-preview`.
    #[test]
    fn the_offer_card_names_the_version_and_offers_update_later_skip() {
        let job = considered("v0.4.7-preview", Channel::Ours);
        let drawn = paint(job.state()).expect("an offer has a card");
        assert_eq!(
            drawn,
            Paint {
                heading: Some("Folio 0.4.7".to_owned()),
                bar: None,
                detail: None,
                folder: None,
                verbs: vec![CardVerb::Update, CardVerb::Later, CardVerb::Skip],
            }
        );
        assert_eq!(drawn.primary(), Some(CardVerb::Update));
        assert_eq!(CardVerb::Update.text(), "Update");
    }

    /// RED (U-19) — **the download's card is a bar and `12 / 41 MB`, and its
    /// one verb is Cancel.**
    ///
    /// §B: "There is no progress surface to reuse … The determinate bar is new
    /// drawing." The bar's share is what was received of what the server said
    /// it would send, and the line never reads the total before the job is
    /// done. Cancel is not recommended: nobody raised the card to stop it.
    ///
    /// MUTATION: round the received megabytes to the nearest in `megabytes`
    /// and the line reads `13 / 41 MB` for 12.6 MB.
    #[test]
    fn the_download_card_draws_a_determinate_bar_and_the_megabytes() {
        let (mut job, post) = downloading();
        post.post(Step::Received(Bytes {
            received: 12_600_000,
            total: Some(41_000_000),
        }));
        job.drain_progress();
        let drawn = paint(job.state()).expect("a download has a card");
        let Some(Bar::Determinate(share)) = drawn.bar else {
            panic!("a download of known length draws a determinate bar: {drawn:?}");
        };
        assert!((share - 12.6 / 41.0).abs() < 1e-4, "{share}");
        assert_eq!(drawn.detail.as_deref(), Some("12 / 41 MB"));
        assert_eq!(drawn.heading, None, "the bar is the first row");
        assert_eq!(drawn.verbs, vec![CardVerb::Cancel]);
        assert_eq!(drawn.primary(), None, "Cancel is never recommended");

        // Nearly done still does not say done.
        post.post(Step::Received(Bytes {
            received: 40_999_999,
            total: Some(41_000_000),
        }));
        job.drain_progress();
        assert_eq!(
            paint(job.state()).and_then(|drawn| drawn.detail).as_deref(),
            Some("40 / 41 MB")
        );
    }

    /// RED (U-19) — **a download whose length is unknown draws the bar alone,
    /// indeterminate, with no number.**
    ///
    /// C9: "bar alone, indeterminate, when the length is unknown". A share
    /// invented for a stream with no length would be a number the card does not
    /// have; the staged job (files on disk, being verified) has none left to
    /// give either.
    ///
    /// MUTATION: read a missing total as the bytes received so far in
    /// `megabytes` and the card draws a full determinate bar and `12 / 12 MB`.
    #[test]
    fn a_download_of_unknown_length_draws_the_bar_alone() {
        let (mut job, post) = downloading();
        post.post(Step::Received(Bytes {
            received: 12_000_000,
            total: None,
        }));
        job.drain_progress();
        let alone = Paint {
            heading: None,
            bar: Some(Bar::Indeterminate),
            detail: None,
            folder: None,
            verbs: vec![CardVerb::Cancel],
        };
        assert_eq!(paint(job.state()), Some(alone.clone()));
        post.post(Step::Staged);
        job.drain_progress();
        assert!(matches!(job.state(), State::Staged(_)));
        assert_eq!(paint(job.state()), Some(alone));
    }

    /// RED (U-19) — **the verified card says `Ready. Running programs will
    /// close.` and offers Restart and Later.**
    ///
    /// C9's third row: the restart is asked for here, not at the press, so the
    /// reader is told what it costs before they spend it.
    ///
    /// MUTATION: give the verified card the offer's verbs in `paint` and it
    /// offers Update again.
    #[test]
    fn the_verified_card_says_ready_and_offers_restart_and_later() {
        let drawn = paint(verified().state()).expect("a verified job has a card");
        assert_eq!(
            drawn,
            Paint {
                heading: Some("Ready. Running programs will close.".to_owned()),
                bar: None,
                detail: None,
                folder: None,
                verbs: vec![CardVerb::Restart, CardVerb::Later],
            }
        );
        assert_eq!(drawn.primary(), Some(CardVerb::Restart));
    }

    /// RED (U-31) — **Restart on the verified card asks the application to
    /// quit with the update's reason; it is not a verb the job answers
    /// alone.**
    ///
    /// The job's own Restart only moves it to `Quitting`: the quit that
    /// photographs the session and hands the installation to the applier is
    /// the application's (`App::restart_for_update`, U-21). Before U-31 the
    /// card's Restart went to `Job::answer_verb`, which left the job at
    /// `Quitting` with no quit asked for — the card gone, Folio still running,
    /// and every later verb refused as `TheQuitAnswers`. That was unseen while
    /// offers were off; with the Windows gate on it is the happy path. Every
    /// other verb still asks what it asked.
    ///
    /// MUTATION: map `CardVerb::Restart` to `Asks::Job(Verb::Restart)` in
    /// `CardVerb::asks`.
    #[test]
    fn restart_on_the_card_asks_the_quit_and_not_the_job_alone() {
        assert_eq!(CardVerb::Restart.asks(), Asks::Quit);
        for (verb, asks) in [
            (CardVerb::Update, Asks::Job(Verb::Press)),
            (CardVerb::Later, Asks::Job(Verb::Later)),
            (CardVerb::Close, Asks::Job(Verb::Later)),
            (CardVerb::Skip, Asks::Job(Verb::Skip)),
            (CardVerb::Cancel, Asks::Job(Verb::Cancel)),
            (CardVerb::Releases, Asks::Releases),
            (CardVerb::ShowFolder, Asks::ShowFolder),
        ] {
            assert_eq!(verb.asks(), asks, "{verb:?}");
        }
        // What the quit's entrance does to the job it is pressed on.
        let mut job = verified();
        let txn = job.state().offer().map(Offer::txn).expect("an offer");
        assert_eq!(
            job.restart(),
            Ok(crate::quit::Reason::UpdateRestart { txn })
        );
        assert!(matches!(job.state(), State::Quitting(_)));
    }

    /// RED (U-19) — **a failure that moved nothing gives its reason, then
    /// `Nothing changed.`, and offers Releases and Close.**
    ///
    /// Through the real press and the only driver there is: `Unsupported`
    /// refuses and the job is `Failed(Unsupported)`, which is what an enabled
    /// build with no driver would show (coordinator ruling 11: the reason as
    /// the job knows it, then C9's suffix).
    ///
    /// MUTATION: map `Failure::Unsupported` to `Outcome::Restored` in
    /// `outcome` and the card claims a rollback that never happened.
    #[test]
    fn a_failure_that_changed_nothing_says_so_after_its_reason() {
        let mut job = considered("v0.4.7", Channel::Ours);
        job.answer_verb(Verb::Press, &Unsupported, &no_door())
            .expect("the press is taken");
        let drawn = paint(job.state()).expect("a failed job has a card");
        assert_eq!(
            drawn,
            Paint {
                heading: Some(Text::UpdateFailedUnsupported.text().to_owned()),
                bar: None,
                detail: Some("Nothing changed.".to_owned()),
                folder: None,
                verbs: vec![CardVerb::Releases, CardVerb::Close],
            }
        );
        assert_eq!(CardVerb::Close.asks(), Asks::Job(Verb::Later));
        assert_eq!(
            CardVerb::Releases.asks(),
            Asks::Releases,
            "a page moves nothing"
        );
    }

    /// RED (U-19) — **a rolled-back failure gives its reason, then `Previous
    /// version restored.`**
    ///
    /// C9's fifth row. No failure the job knows today rolls back — the flip is
    /// U-20/U-21's — so the shape is held on the paint model itself.
    ///
    /// MUTATION: say `Nothing changed.` for `Outcome::Restored` in `failed`.
    #[test]
    fn a_rolled_back_failure_says_the_previous_version_was_restored() {
        let drawn = failed("The new version did not start.", &Outcome::Restored);
        assert_eq!(
            drawn,
            Paint {
                heading: Some("The new version did not start.".to_owned()),
                bar: None,
                detail: Some("Previous version restored.".to_owned()),
                folder: None,
                verbs: vec![CardVerb::Releases, CardVerb::Close],
            }
        );
    }

    /// RED (U-19) — **a rollback that did not finish says `Update
    /// incomplete.`, names the journal's folder, and offers Show folder.**
    ///
    /// C9's sixth row: the one failure the reader has something to do about,
    /// so its first verb opens the folder rather than a web page.
    ///
    /// MUTATION: give `Outcome::Incomplete` the Releases verb in `failed`.
    #[test]
    fn a_rollback_that_did_not_finish_names_the_journal_folder() {
        let folder = PathBuf::from("update-journal");
        let drawn = failed(
            "The previous version could not be put back.",
            &Outcome::Incomplete {
                folder: Some(folder.clone()),
                held: false,
            },
        );
        assert_eq!(drawn.detail.as_deref(), Some("Update incomplete."));
        assert_eq!(drawn.folder, Some(folder));
        assert_eq!(drawn.verbs, vec![CardVerb::ShowFolder, CardVerb::Close]);
        assert_eq!(drawn.primary(), Some(CardVerb::ShowFolder));
    }

    /// RED (E1) — **the card of an update this build cannot read whole, and
    /// of a session whose writes are held**: a newer Folio's unfinished
    /// update names that Folio's version and when it finishes; one whose
    /// record names no later build says the record cannot be read; a start
    /// that continues with its writes held adds that this session's changes
    /// are not kept, to the newer card and to *Update incomplete.* alike. The
    /// buttons are the incomplete card's: Show folder with a folder, the
    /// releases page without one, and Close.
    ///
    /// MUTATION: in `not_kept`, answer `detail` whatever `held` says (the held
    /// sentence is never drawn).
    #[test]
    fn an_unfinished_update_by_another_folio_and_a_held_session_say_so() {
        let folder = PathBuf::from(r"D:\工具\Folio 终端\.folio-update");
        let drawn = |failure: Failure| {
            paint(&State::Failed(None, failure)).expect("a failed job has a card")
        };
        for (failure, heading, detail) in [
            (
                Failure::Newer {
                    folder: Some(folder.clone()),
                    version: Some("0.4.9".to_owned()),
                    held: false,
                },
                "An update by a newer Folio is not finished.",
                "It finishes when you next sign in, or when you start Folio 0.4.9.",
            ),
            (
                Failure::Newer {
                    folder: Some(folder.clone()),
                    version: None,
                    held: false,
                },
                "The update record cannot be read.",
                "It finishes when you next sign in.",
            ),
            (
                Failure::Newer {
                    folder: Some(folder.clone()),
                    version: Some("0.4.9".to_owned()),
                    held: true,
                },
                "An update by a newer Folio is not finished.",
                "It finishes when you next sign in, or when you start Folio 0.4.9. Changes made in this session are not kept.",
            ),
            (
                Failure::Incomplete {
                    folder: Some(folder.clone()),
                    held: true,
                    untried: false,
                },
                "The new version did not start.",
                "Update incomplete. Changes made in this session are not kept.",
            ),
        ] {
            let card = drawn(failure.clone());
            assert_eq!(card.heading.as_deref(), Some(heading), "{failure:?}");
            assert_eq!(card.detail.as_deref(), Some(detail), "{failure:?}");
            assert_eq!(card.folder.as_ref(), Some(&folder), "{failure:?}");
            assert_eq!(card.verbs, vec![CardVerb::ShowFolder, CardVerb::Close]);
        }
        let unnamed = drawn(Failure::Newer {
            folder: None,
            version: None,
            held: false,
        });
        assert_eq!(unnamed.verbs, vec![CardVerb::Releases, CardVerb::Close]);
    }

    /// RED (U-35 round 2) — **the fallback trial's card says what happened,
    /// the held-write consequence and what to do, without protocol jargon.**
    /// It keeps the unfinished transaction's Show folder action.
    ///
    /// MUTATION: map `Failure::TrialIncomplete` to the ordinary incomplete
    /// texts; either sentence below changes.
    #[test]
    fn u35_the_fallback_trial_card_names_the_update_and_the_session() {
        let folder = PathBuf::from("update-journal");
        let drawn = paint(&State::Failed(
            None,
            Failure::TrialIncomplete {
                folder: folder.clone(),
            },
        ))
        .expect("the fallback trial has a card");
        assert_eq!(drawn.heading.as_deref(), Some("The update did not finish."));
        assert_eq!(
            drawn.detail.as_deref(),
            Some(
                "Changes made now may not be kept until Folio confirms the update. Keep Folio open until then."
            )
        );
        assert_eq!(drawn.folder, Some(folder));
        assert_eq!(drawn.primary(), Some(CardVerb::ShowFolder));
    }

    /// RED (U-29) — **the two failures a rollback reports are C9's fifth and
    /// sixth rows, from the job's own state**: the reason that the new
    /// version did not start, then *Previous version restored.* with the
    /// releases page, or *Update incomplete.* with the journal's folder.
    ///
    /// Until U-29 no failure reached these shapes (`Outcome::Restored` and
    /// `Outcome::Incomplete` were drawn only by the tests).
    ///
    /// MUTATION: map `Failure::RolledBack` to `Outcome::NothingChanged` in
    /// `outcome`.
    #[test]
    fn the_failures_a_rollback_reports_draw_restored_and_incomplete() {
        let restored =
            paint(&State::Failed(None, Failure::RolledBack)).expect("a failed job has a card");
        assert_eq!(
            restored,
            Paint {
                heading: Some(Text::UpdateFailedTrial.text().to_owned()),
                bar: None,
                detail: Some("Previous version restored.".to_owned()),
                folder: None,
                verbs: vec![CardVerb::Releases, CardVerb::Close],
            }
        );
        let folder = PathBuf::from("/Applications/.Folio.app.folio-update");
        let incomplete = paint(&State::Failed(
            None,
            Failure::Incomplete {
                folder: Some(folder.clone()),
                held: false,
                untried: false,
            },
        ))
        .expect("a failed job has a card");
        assert_eq!(incomplete.heading, restored.heading);
        assert_eq!(incomplete.detail.as_deref(), Some("Update incomplete."));
        assert_eq!(incomplete.folder, Some(folder));
        assert_eq!(incomplete.primary(), Some(CardVerb::ShowFolder));
    }

    /// RED (U-19) — **no card where C9 draws none**: a pending or idle job,
    /// and a job whose quit is running (the quit's own card asks, unsaved
    /// documents included).
    ///
    /// MUTATION: draw the verified card for `State::Quitting` in `paint`.
    #[test]
    fn no_card_is_drawn_where_c9_draws_none() {
        assert_eq!(paint(Job::<u32>::with_offers(true).state()), None);
        let mut quitting = verified();
        quitting
            .answer_verb(Verb::Restart, &Unsupported, &no_door())
            .expect("Restart");
        assert!(matches!(quitting.state(), State::Quitting(_)));
        assert_eq!(paint(quitting.state()), None);
        assert_eq!(quitting.card_window(), None);
        let idle = considered("v0.4.6", Channel::Ours);
        assert_eq!(idle.state(), &State::Idle);
        assert_eq!(paint(idle.state()), None);
    }

    /// RED (U-19) — **Later on a verified job puts the card away, keeps the
    /// job, and leaves `Update and restart` on About → Version, which raises
    /// the card again.**
    ///
    /// §B: "From `Verified`, **Later** keeps the staged transaction and leaves
    /// an **Update and restart** control on the Version row
    /// foot, so the work is not stranded and not silently discarded"; C9 names
    /// it the same restart action. The row's control is read off the job, the
    /// row's sentence names the tag, and pressing the foot re-opens the card
    /// in the window it was pressed in.
    ///
    /// MUTATION: answer `RowFoot::ReleasesPage` for a verified job in
    /// `row_foot` and the verified job has no way back.
    #[test]
    fn later_from_verified_leaves_a_resume_entry() {
        let mut job = verified();
        assert_eq!(job.card_window(), Some(1), "the verified card is up");
        assert_eq!(
            job.answer_verb(Verb::Later, &Unsupported, &no_door()),
            Ok(Effect::None)
        );
        assert!(matches!(job.state(), State::Verified(_)), "the job is kept");
        assert_eq!(job.card_window(), None, "Later put the card away");
        let foot = row_foot(&job);
        assert_eq!(
            foot,
            RowFoot::Restart {
                tag: "v0.4.7".to_owned()
            }
        );
        assert!(job.reopen(2), "the row's foot raises the card again");
        assert_eq!(
            job.card_window(),
            Some(2),
            "in the window it was pressed in"
        );
        assert_eq!(
            paint(job.state()).map(|drawn| drawn.verbs),
            Some(vec![CardVerb::Restart, CardVerb::Later])
        );
    }

    /// RED (U-19) — **a copy a package manager owns gets no card, and its row
    /// names the manager's command with one `Copy`, which writes the command
    /// on the press.**
    ///
    /// C2: "Managed copies get no card … the row names the manager's command
    /// with one **Copy** verb". Through the real job on each of the three
    /// managers, and the press through a recording writer — the product's
    /// writer is the window's one clipboard write, and no test writes the real
    /// clipboard.
    ///
    /// MUTATION: return `VersionControl::UpdateAndRestart` for the managed
    /// branch of `version_row`; its one control stops being Copy and this goes
    /// red.
    #[test]
    fn a_managed_copy_shows_its_command_and_no_card() {
        for (manager, command) in [
            (Manager::Scoop, "scoop update folio"),
            (Manager::Homebrew, "brew upgrade --cask folio"),
            (
                Manager::Winget,
                "winget upgrade --id WeiyiShi.Folio --exact",
            ),
        ] {
            let job = considered(
                "v0.4.7",
                Channel::Managed {
                    manager,
                    uninstall_hook: true,
                },
            );
            assert_eq!(job.card_window(), None, "{manager:?} gets no card");
            assert_eq!(
                shown(&job, false, crate::update::CheckView::default(), None, 0).card,
                None
            );
            let foot = row_foot(&job);
            assert_eq!(foot, RowFoot::Copy { command }, "{manager:?}");
            let row = version_row(
                &job,
                crate::update::CheckView::default(),
                Some("v0.4.7"),
                0,
                Lang::English,
            );
            assert!(row.value.contains(command), "{}", row.value);
            assert_eq!(row.control, VersionControl::CopyCommand { command });
        }
    }

    /// A job that considered `latest` for a copy installed as `channel`,
    /// running on `platform`.
    fn considered_on(latest: &str, channel: Channel, platform: HostPlatform) -> Job<u32> {
        let mut job = Job::with_offers(true);
        job.consider(
            Gathered {
                platform,
                ..gathered(latest, channel)
            },
            &windows(),
            || TxnId::new([7; 16]),
        );
        job
    }

    /// PIN (U-41d, managed-update §2.4 design (a)) — **a winget copy never
    /// raises the card while winget's road is off: on every platform, its
    /// row keeps `winget upgrade` with one Copy, and no transaction is
    /// offered.**
    ///
    /// The road is off by a constant (`update_adapter::WINGET_ROAD`), not by
    /// a precondition: no pin is read and no winget process runs, so the
    /// regression is the whole of it — the job's answer, the card and the
    /// row, through the real job.
    ///
    /// MUTATION: `WINGET_ROAD = true` — the winget copy on Windows is offered
    /// the card.
    #[test]
    fn a_winget_copy_never_raises_the_card_while_the_road_is_off() {
        let command = "winget upgrade --id WeiyiShi.Folio --exact";
        for platform in [HostPlatform::Windows, HostPlatform::MacOs] {
            let job = considered_on(
                "v0.4.7",
                Channel::Managed {
                    manager: Manager::Winget,
                    uninstall_hook: false,
                },
                platform,
            );
            assert_eq!(job.card_window(), None, "{platform:?}: no card");
            assert_eq!(
                shown(&job, false, crate::update::CheckView::default(), None, 0).card,
                None,
                "{platform:?}"
            );
            assert_eq!(
                job.answer(),
                Some(&Err(crate::update_job::NotEligible::Managed {
                    manager: Manager::Winget,
                    command
                })),
                "{platform:?}"
            );
            assert_eq!(row_foot(&job), RowFoot::Copy { command }, "{platform:?}");
        }
    }

    /// PIN (U-41a1, managed-update §1.5) — **a Homebrew copy on macOS and a
    /// scoop copy on Windows, whose adapters are not built yet, keep their
    /// manager's command with Copy and no card, on the platform whose road
    /// their adapter would take.**
    ///
    /// The journal can name `Homebrew` and `Scoop` (`update_txn::Adapter`),
    /// and eligibility now asks the adapter whether its road is built
    /// (`update_adapter::built_on`) instead of refusing every managed copy;
    /// until U-41b and U-41c build them, the answer must stay the row it was.
    ///
    /// MUTATION: `HOMEBREW_ROAD = true` or `SCOOP_ROAD = true` — that copy
    /// is offered the card on its platform.
    #[test]
    fn a_managed_copy_whose_adapter_is_not_built_keeps_the_copy_row() {
        for (manager, platform, command) in [
            (
                Manager::Homebrew,
                HostPlatform::MacOs,
                "brew upgrade --cask folio",
            ),
            (Manager::Scoop, HostPlatform::Windows, "scoop update folio"),
        ] {
            let adapter = crate::update_adapter::of_manager(manager);
            assert!(
                !crate::update_adapter::built_on(adapter, platform),
                "{manager:?}: this proof is for an unbuilt manager road"
            );
            let job = considered_on(
                "v0.4.7",
                Channel::Managed {
                    manager,
                    uninstall_hook: true,
                },
                platform,
            );
            assert_eq!(job.card_window(), None, "{manager:?}: no card");
            assert_eq!(row_foot(&job), RowFoot::Copy { command }, "{manager:?}");
            let row = version_row(
                &job,
                crate::update::CheckView::default(),
                Some("v0.4.7"),
                0,
                Lang::English,
            );
            assert_eq!(
                row.control,
                VersionControl::CopyCommand { command },
                "{manager:?}: the Version state copies the manager command"
            );
            assert_ne!(
                row.control.kind(),
                VersionControlKind::UpdateAndRestart,
                "{manager:?}: an unbuilt road never offers Folio's updater"
            );
        }
        // And ours, on the same two platforms, is offered it.
        for platform in [HostPlatform::Windows, HostPlatform::MacOs] {
            let job = considered_on("v0.4.7", Channel::Ours, platform);
            assert!(job.card_window().is_some(), "{platform:?}: ours is offered");
        }
    }

    /// RED (U-4) — **a copy winget's own record names is offered no card, and
    /// its row names `winget upgrade --id WeiyiShi.Folio --exact` with one
    /// `Copy`.**
    ///
    /// The 0.4.6 gate (the owner's ruling 2026-09-20: managed installs do not
    /// self-update) holds for winget only if a winget copy is ever classified
    /// as one. So the channel here is not spelled: a package folder shaped as
    /// E2 found it, winget's record of it, and the executable inside go through
    /// `install_channel`'s own read and classification, and the channel that
    /// comes out goes through the real job to the row.
    ///
    /// MUTATION: in `install_channel::classify`, `WingetEvidence::Record(_) =>
    /// false` — the copy reads as ours and is offered the card.
    #[test]
    fn the_managed_winget_row_names_the_upgrade_command() {
        use bt_platform::install_evidence::{RecordValue, UninstallRecord};

        use crate::install_channel::{self, WINGET_PACKAGE_ID, WINGET_PORTABLE};

        let location = bt_testpath::temp_path("bt-update-card-winget");
        let _ = std::fs::remove_dir_all(&location);
        let version = location.join("folio-0.4.6");
        std::fs::create_dir_all(&version).unwrap();
        let exe = version.join("folio.exe");
        std::fs::write(&exe, b"").unwrap();
        let record = UninstallRecord {
            key: "WeiyiShi.Folio_Microsoft.Winget.Source_8wekyb3d8bbwe".to_owned(),
            values: vec![
                RecordValue::Text(WINGET_PACKAGE_ID.to_owned()),
                RecordValue::Text(WINGET_PORTABLE.to_owned()),
                RecordValue::Text(location.to_string_lossy().into_owned()),
                RecordValue::Text("Microsoft.Winget.Source_8wekyb3d8bbwe".to_owned()),
            ],
        };
        let me = bt_platform::install_evidence::current_account().unwrap();
        let winget = install_channel::winget(&exe, HostPlatform::Windows, || Ok(vec![record]));
        let channel = install_channel::classify(&install_channel::read(
            &version,
            HostPlatform::Windows,
            Ok(&me),
            winget,
        ));
        assert_eq!(
            channel,
            Channel::Managed {
                manager: Manager::Winget,
                uninstall_hook: false,
            }
        );

        let job = considered("v0.4.7", channel);
        assert_eq!(job.card_window(), None);
        let command = "winget upgrade --id WeiyiShi.Folio --exact";
        let foot = row_foot(&job);
        assert_eq!(foot, RowFoot::Copy { command });
        let row = version_row(
            &job,
            crate::update::CheckView::default(),
            Some("v0.4.7"),
            0,
            Lang::English,
        );
        assert!(row.value.contains(command), "{}", row.value);
        assert_eq!(row.control, VersionControl::CopyCommand { command });
        std::fs::remove_dir_all(&location).unwrap();
    }

    /// RED (U-19) — **Skip on the card records the tag through the check's
    /// one owner.**
    ///
    /// Coordinator ruling 10: "Skip hands `Effect::RecordSkip(tag)` to
    /// `OfferState::skip` (U-6's owner)". The real job's Skip, spent through
    /// the real owner on a private `update-check.json`: the file then skips
    /// the tag and has seen it, and the card is gone.
    ///
    /// MUTATION: make `spend` answer `Ok(())` for `Effect::RecordSkip` without
    /// calling the owner, and the file never learns the tag.
    #[test]
    fn skip_records_the_tag_through_offer_state() {
        let root = bt_testpath::temp_path("bt-update-card-skip");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private directory for this test");
        let owner = crate::update::OfferState::load(&root, true);

        let mut job = considered("v0.4.7", Channel::Ours);
        let effect = job
            .answer_verb(Verb::Skip, &Unsupported, &no_door())
            .expect("Skip is on the offer's card");
        spend(effect, |tag| owner.skip(tag)).expect("the Skip is written");
        assert_eq!(job.card_window(), None, "the card went with the Skip");
        let known = owner.known();
        assert_eq!(known.skipped_tag.as_deref(), Some("v0.4.7"));
        assert_eq!(known.seen_tag.as_deref(), Some("v0.4.7"));
        let reread = bt_persist::read_update_check(&root.join(crate::update::STATE_FILE_NAME)).0;
        assert_eq!(reread.skipped_tag.as_deref(), Some("v0.4.7"), "on disk");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-19) — **Enter presses nothing until a key has lit the ring, and
    /// then presses the verb it stands on; Escape is Later.**
    ///
    /// The card rises by itself while the reader may be typing: a return key
    /// already on its way must not start a download (the PSReadLine
    /// invitation's rule for a card that writes). `Tab` walks the verbs in the
    /// order they are drawn, left to right.
    ///
    /// MUTATION: have `Key::Press` answer the recommended verb when no ring is
    /// lit in `key`.
    #[test]
    fn enter_presses_nothing_until_the_ring_is_lit() {
        let verbs = [CardVerb::Update, CardVerb::Later, CardVerb::Skip];
        let mut card = Card::default();
        assert_eq!(key(&mut card, &verbs, Key::Press), None);
        assert_eq!(key(&mut card, &verbs, Key::Step { forward: true }), None);
        assert_eq!(
            card.ring(&verbs),
            Some(CardVerb::Skip),
            "drawn left to right"
        );
        key(&mut card, &verbs, Key::Step { forward: false });
        assert_eq!(card.ring(&verbs), Some(CardVerb::Update));
        assert_eq!(key(&mut card, &verbs, Key::Press), Some(CardVerb::Update));
        assert_eq!(key(&mut card, &verbs, Key::Later), Some(CardVerb::Later));
        assert_eq!(
            Target::Close.verb(),
            Some(CardVerb::Later),
            "the × is Later"
        );
        assert_eq!(Target::<CardVerb>::Panel.verb(), None);
        // A ring on a verb the card no longer carries is out.
        assert_eq!(card.ring(&[CardVerb::Cancel]), None);
    }

    /// RED (U-19) — **every verb and the `×` answer where they are drawn, and
    /// the recommended verb stands on the right.**
    ///
    /// The card's geometry, measured with a fixed advance: the press routes by
    /// `restore::update_card_hit`, so a verb drawn in one place and hit-tested
    /// in another would be a button that presses its neighbour.
    ///
    /// MUTATION: lay the verbs out left to right from the content's left edge
    /// in `restore::update_card_layout` and Update is no longer rightmost.
    #[test]
    fn every_verb_answers_where_it_is_drawn() {
        let drawn = paint(considered("v0.4.7", Channel::Ours).state()).expect("a card");
        let content =
            crate::restore::update_card_content(&drawn, 1200.0, 1.0, &mut |text, size, _| {
                text.chars().count() as f32 * size * 0.5
            });
        let layout = crate::restore::update_card_layout(&content, 1200.0, 800.0, 1.0);
        let centre = |rect: [f32; 4]| {
            (
                f64::from((rect[0] + rect[2]) / 2.0),
                f64::from((rect[1] + rect[3]) / 2.0),
            )
        };
        let mut lefts = Vec::new();
        for verb in &drawn.verbs {
            let rect = layout.button(*verb).expect("every verb is drawn");
            let (x, y) = centre(rect);
            assert_eq!(
                crate::restore::update_card_hit(&layout, x, y),
                Target::Verb(*verb)
            );
            lefts.push(rect[0]);
        }
        assert!(
            lefts[0] > lefts[1] && lefts[1] > lefts[2],
            "C9's order runs right to left: {lefts:?}"
        );
        let (x, y) = centre(layout.close_box());
        assert_eq!(
            crate::restore::update_card_hit(&layout, x, y),
            Target::Close
        );
        assert_eq!(
            crate::restore::update_card_hit(&layout, 1.0, 1.0),
            Target::Panel,
            "beside the card is the card's, and answers nothing"
        );
    }

    /// RED (T-UNINSTALL-UX) — **the uninstaller's confirmation card says whether settings and
    /// data stay, carries Uninstall then Cancel, and presses nothing on `Enter` until a key has
    /// lit the ring; `Escape` and the `×` are Cancel.**
    ///
    /// MUTATION: in `UninstallVerb::put_away`, answer `Uninstall`, and `Escape` uninstalls.
    #[test]
    fn the_uninstall_card_names_what_stays_and_escape_cancels() {
        let keeps = uninstall_paint(false);
        let removes = uninstall_paint(true);
        assert_eq!(
            keeps.detail.as_deref(),
            Some(Text::UninstallCardKeeps.text())
        );
        assert_eq!(
            removes.detail.as_deref(),
            Some(Text::UninstallCardRemoves.text())
        );
        assert_eq!(
            keeps.verbs,
            [UninstallVerb::Uninstall, UninstallVerb::Cancel]
        );
        assert_eq!(keeps.primary(), Some(UninstallVerb::Uninstall));
        let mut card = Card::<UninstallVerb>::default();
        assert_eq!(
            key(&mut card, &keeps.verbs, Key::Press),
            None,
            "the ring is unlit"
        );
        assert_eq!(
            key(&mut card, &keeps.verbs, Key::Later),
            Some(UninstallVerb::Cancel)
        );
        assert_eq!(
            Target::<UninstallVerb>::Close.verb(),
            Some(UninstallVerb::Cancel)
        );
        key(&mut card, &keeps.verbs, Key::Step { forward: true });
        assert_eq!(
            key(&mut card, &keeps.verbs, Key::Press),
            Some(UninstallVerb::Cancel),
            "the first Tab lands on the leftmost button, Cancel"
        );
    }
}
