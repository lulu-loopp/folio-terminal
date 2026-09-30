//! **The update card and the General row** — what the reader sees of the update
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
//!   state C9 draws no card for.
//! - **What a press on it asks** — [`CardVerb::asks`] (Escape and the close
//!   box are Later; a failed card's Close is Later too; Restart asks the
//!   application's quit, not the job alone) and [`spend`], which
//!   hands the job's one outside effect, Skip's, to the check's owner
//!   (`update::OfferState::skip`).
//! - **Where the keyboard's ring is** — [`Card`], the per-window hover and ring,
//!   which is the only state here and is about a window, not the job.
//! - **What the General row ends with** — [`row_foot`]: the releases page as
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
use crate::update_job::{Bytes, Effect, Failure, Job, NotEligible, State, Stop, Verb};

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
    /// finish (`Failure::Incomplete`, U-29).
    Incomplete { folder: PathBuf },
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
        State::Updated(version) => Some(Paint {
            heading: Some(format!("Folio {version}")),
            detail: Some(Text::UpdateCardUpdated.text().to_owned()),
            ..bare(vec![CardVerb::Close])
        }),
    }
}

/// **A failed card**: the reason, then what the failure did (C9's three
/// shapes; coordinator ruling 11 — the reasons stay U-20's, the suffix is
/// C9's).
#[must_use]
pub(crate) fn failed(reason: &str, outcome: &Outcome) -> Paint {
    let (detail, folder, first) = match outcome {
        Outcome::NothingChanged => (Text::UpdateCardNothingChanged, None, CardVerb::Releases),
        Outcome::Restored => (Text::UpdateCardRestored, None, CardVerb::Releases),
        Outcome::Incomplete { folder } => (
            Text::UpdateCardIncomplete,
            Some(folder.clone()),
            CardVerb::ShowFolder,
        ),
    };
    Paint {
        heading: Some(reason.to_owned()),
        bar: None,
        detail: Some(detail.text().to_owned()),
        folder,
        verbs: vec![first, CardVerb::Close],
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
        Failure::Stopped(Stop::Journal) => Text::UpdateFailedJournal,
        Failure::Stopped(Stop::Download | Stop::Cancelled) => Text::UpdateFailedStopped,
        Failure::Stopped(Stop::Sums) => Text::UpdateFailedSums,
        Failure::Stopped(Stop::Mount) => Text::UpdateFailedMount,
        Failure::Stopped(Stop::Identity) => Text::UpdateFailedIdentity,
        Failure::Stopped(Stop::Copy) => Text::UpdateFailedCopy,
        Failure::Stopped(Stop::Clone) => Text::UpdateFailedClone,
        Failure::RolledBack | Failure::Incomplete { .. } => Text::UpdateFailedTrial,
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
        Failure::Unsupported | Failure::Stopped(_) => Outcome::NothingChanged,
        Failure::RolledBack => Outcome::Restored,
        Failure::Incomplete { folder } => Outcome::Incomplete {
            folder: folder.clone(),
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

// ── the General row ────────────────────────────────────────────────────────

/// **What the General row's picker ends with** (C9, C2).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum RowFoot {
    /// `Open releases page`, as the row has always offered — also for a copy
    /// that is not ours, not known, or has no release file here (§D).
    #[default]
    ReleasesPage,
    /// `Restart to update`: a job waits at `Verified`, and the foot raises its
    /// card again in the window it is pressed in. `tag` is the offer's.
    Restart { tag: String },
    /// `Copy`: a package manager updates this copy, and this is its command.
    Copy { command: &'static str },
}

impl RowFoot {
    /// The foot's word.
    #[must_use]
    pub(crate) fn text(&self) -> &'static str {
        match self {
            Self::ReleasesPage => Text::OpenReleasesPage,
            Self::Restart { .. } => Text::UpdateRowRestart,
            Self::Copy { .. } => Text::UpdateRowCopy,
        }
        .text()
    }
}

/// **The General row's foot, read off the job** — the row says only what the
/// job already knows (coordinator ruling 6: no capability input is added).
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

/// **The General row's sentence for this foot**, in `lang`; `offered` is the
/// tag the day's check offers (`update::offer`).
///
/// The releases page keeps today's sentence ([`crate::update::row_description_in`]);
/// the other two name the tag and what their foot does. A managed copy whose
/// check offers nothing says the row's plain sentence, as it always has.
#[must_use]
pub(crate) fn row_description_in(
    lang: Lang,
    foot: &RowFoot,
    offered: Option<&str>,
) -> &'static str {
    match foot {
        RowFoot::ReleasesPage => crate::update::row_description_in(lang),
        RowFoot::Restart { tag } => i18n::intern(i18n::update_row_ready_in(lang, tag)),
        RowFoot::Copy { command } => match offered {
            Some(tag) => i18n::intern(i18n::update_row_managed_in(lang, tag, command)),
            None => Text::DescUpdateCheck.in_lang(lang),
        },
    }
}

/// **`Copy` on the row**: the manager's command through the window's one
/// clipboard write (`write`), **only on the reader's press** — nothing else
/// here writes the clipboard. Answers whether this foot copies at all.
///
/// # Errors
///
/// The write's.
pub(crate) fn copy_command(
    foot: &RowFoot,
    write: impl FnOnce(&str) -> anyhow::Result<()>,
) -> anyhow::Result<bool> {
    let RowFoot::Copy { command } = foot else {
        return Ok(false);
    };
    write(command)?;
    Ok(true)
}

// ── what the windows show, for the loop's once-a-turn comparison ───────────

/// **What the job shows this turn**: the card (its window and its paint) and
/// the row's foot. The loop keeps the last one and repaints only the windows
/// whose drawing it changed (`FolioApp::settle_update_card`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Shown<W> {
    pub(crate) card: Option<(W, Paint)>,
    pub(crate) foot: RowFoot,
}

impl<W> Default for Shown<W> {
    fn default() -> Self {
        Self {
            card: None,
            foot: RowFoot::default(),
        }
    }
}

/// What the job shows now.
#[must_use]
pub(crate) fn shown<W: Copy + Eq>(job: &Job<W>) -> Shown<W> {
    Shown {
        card: job
            .card_window()
            .and_then(|window| paint(job.state()).map(|paint| (window, paint))),
        foot: row_foot(job),
    }
}

/// **Every sentence the General row can say beyond today's**, for the settings
/// dialog's two-line budget (`settings::tests::no_settings_sentence_needs_a_third_line`):
/// the ready row and the managed row under each manager, with a tag longer than
/// any this product has shipped.
#[cfg(test)]
pub(crate) fn every_new_row_sentence_in(lang: Lang) -> Vec<&'static str> {
    const TAG: &str = "v10.10.10-preview";
    let mut sentences = vec![row_description_in(
        lang,
        &RowFoot::Restart {
            tag: TAG.to_owned(),
        },
        Some(TAG),
    )];
    for manager in [
        crate::install_channel::Manager::Scoop,
        crate::install_channel::Manager::Homebrew,
        crate::install_channel::Manager::Winget,
    ] {
        sentences.push(row_description_in(
            lang,
            &RowFoot::Copy {
                command: crate::update_job::manager_command(manager),
            },
            Some(TAG),
        ));
    }
    sentences
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;

    use bt_persist::UpdateCheckV1;
    use bt_platform::HostPlatform;

    use super::{
        Asks, Bar, Card, CardVerb, Key, Outcome, Paint, RowFoot, Target, UninstallVerb,
        copy_command, failed, key, paint, row_description_in, row_foot, shown, spend,
        uninstall_paint,
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
            check: Some((
                UpdateCheckV1 {
                    latest_tag: Some(latest.to_owned()),
                    ..UpdateCheckV1::default()
                },
                true,
            )),
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
                folder: folder.clone(),
            },
        );
        assert_eq!(drawn.detail.as_deref(), Some("Update incomplete."));
        assert_eq!(drawn.folder, Some(folder));
        assert_eq!(drawn.verbs, vec![CardVerb::ShowFolder, CardVerb::Close]);
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
                folder: folder.clone(),
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
    /// job, and leaves `Restart to update` on the General row, which raises
    /// the card again.**
    ///
    /// §B: "From `Verified`, **Later** keeps the staged transaction and leaves
    /// a **Restart to finish updating** entry on the General row's picker
    /// foot, so the work is not stranded and not silently discarded"; C9 names
    /// it **Restart to update**. The row's foot is read off the job, the
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
        assert_eq!(foot.text(), "Restart to update");
        assert_eq!(
            row_description_in(Lang::English, &foot, Some("v0.4.7")),
            "v0.4.7 is ready. Restart to update closes running programs."
        );
        let values = crate::settings::SettingsValues {
            update_row: foot,
            ..crate::settings::SettingsValues::sample()
        };
        assert_eq!(
            crate::settings::SettingsRow::UpdateCheck.menu_action(&values),
            Some("Restart to update"),
            "the picker's foot is the job's"
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
    /// MUTATION: send `NotEligible::Managed` to the releases page in
    /// `row_foot` and the row offers the page instead of the command.
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
            assert_eq!(shown(&job).card, None);
            let foot = row_foot(&job);
            assert_eq!(foot, RowFoot::Copy { command }, "{manager:?}");
            assert_eq!(foot.text(), "Copy");
            let sentence = row_description_in(Lang::English, &foot, Some("v0.4.7"));
            assert!(sentence.contains(command), "{sentence}");
            let written = RefCell::new(Vec::<String>::new());
            assert!(
                copy_command(&foot, |text| {
                    written.borrow_mut().push(text.to_owned());
                    Ok(())
                })
                .expect("the recording writer accepts")
            );
            assert_eq!(written.into_inner(), vec![command.to_owned()]);
        }
        // And the releases page is not a copy: nothing is written.
        let written = RefCell::new(0);
        assert!(
            !copy_command(&RowFoot::ReleasesPage, |_| {
                *written.borrow_mut() += 1;
                Ok(())
            })
            .expect("nothing to write")
        );
        assert_eq!(written.into_inner(), 0);
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

        let location =
            std::env::temp_dir().join(format!("bt-update-card-winget-{}", std::process::id()));
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
        let sentence = row_description_in(Lang::English, &foot, Some("v0.4.7"));
        assert!(sentence.contains(command), "{sentence}");
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
        let root = std::env::temp_dir().join(format!("bt-update-card-skip-{}", std::process::id()));
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
