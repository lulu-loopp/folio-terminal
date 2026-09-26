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
//! - **What a press on it asks** — [`CardVerb::job_verb`] (Escape and the close
//!   box are Later; a failed card's Close is Later too) and [`spend`], which
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
//! **Offers stay off** (`update_job::Job::offers_enabled`): no shipped build
//! reaches a state with a card until the enabling tickets (U-31, U-32). The card
//! and the row are reached by the tests and by the job's own events.

use std::path::PathBuf;

use crate::i18n::{self, Lang, Text};
use crate::update_job::{Bytes, Effect, Failure, Job, NotEligible, State, Verb};

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

    /// **The job's verb this button is**, or `None` for the two that leave
    /// the window and move nothing (Releases, Show folder). Close is Later
    /// (`update_job::Verb`'s own note).
    #[must_use]
    pub(crate) const fn job_verb(self) -> Option<Verb> {
        match self {
            Self::Update => Some(Verb::Press),
            Self::Later | Self::Close => Some(Verb::Later),
            Self::Skip => Some(Verb::Skip),
            Self::Cancel => Some(Verb::Cancel),
            Self::Restart => Some(Verb::Restart),
            Self::Releases | Self::ShowFolder => None,
        }
    }
}

/// What a point on the card is over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Target {
    /// The face, or the window beside it: answers nothing.
    Panel,
    /// The `×` — Later (§B).
    Close,
    Verb(CardVerb),
}

impl Target {
    /// The verb a press on this target is, with the close box read as Later.
    #[must_use]
    pub(crate) const fn verb(self) -> Option<CardVerb> {
        match self {
            Self::Panel => None,
            Self::Close => Some(CardVerb::Later),
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
    /// `Previous version restored.` — the flip was rolled back (U-20/U-21's).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no failure rolls back until the flip exists: U-20 / U-21 (b).5"
        )
    )]
    Restored,
    /// `Update incomplete.` and the journal's folder — the rollback did not
    /// finish (U-20/U-21's).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no failure leaves a journal behind until the flip exists: U-20 / U-21 (b).5"
        )
    )]
    Incomplete { folder: PathBuf },
}

/// **Everything the card draws, in the order it draws it** — decided here, from
/// the job alone, and measured and placed by `restore::update_card_layout`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Paint {
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
    pub(crate) verbs: Vec<CardVerb>,
}

impl Paint {
    /// **The verb drawn in the accent** — C9's first, except `Cancel`: a card
    /// that recommended stopping its own download would be recommending the one
    /// press nobody raised it for.
    #[must_use]
    pub(crate) fn primary(&self) -> Option<CardVerb> {
        self.verbs
            .first()
            .copied()
            .filter(|verb| *verb != CardVerb::Cancel)
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
        State::Failed(_, failure) => Some(failed(reason(failure), &outcome(failure))),
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

/// The reason a failure gives, as the job knows it today.
fn reason(failure: &Failure) -> &'static str {
    match failure {
        Failure::Unsupported => Text::UpdateFailedUnsupported,
        Failure::Stopped => Text::UpdateFailedStopped,
    }
    .text()
}

/// What each failure the job knows today did: both stop before anything
/// installed moves (`update_job::Failure`'s own notes).
const fn outcome(failure: &Failure) -> Outcome {
    match failure {
        Failure::Unsupported | Failure::Stopped => Outcome::NothingChanged,
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
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Card {
    hover: Option<Target>,
    ring: Option<CardVerb>,
}

impl Card {
    #[must_use]
    pub(crate) const fn hover(&self) -> Option<Target> {
        self.hover
    }

    /// Returns whether the drawing has to change.
    pub(crate) fn set_hover(&mut self, hover: Option<Target>) -> bool {
        let changed = self.hover != hover;
        self.hover = hover;
        changed
    }

    /// The verb the ring stands on, if it stands on one of `verbs`.
    #[must_use]
    pub(crate) fn ring(&self, verbs: &[CardVerb]) -> Option<CardVerb> {
        self.ring.filter(|verb| verbs.contains(verb))
    }

    /// `Tab` / `Shift+Tab`: the ring lights, or moves, over the verbs in the
    /// order they are drawn (C9's order reversed: the recommended verb stands
    /// on the right).
    pub(crate) fn step_ring(&mut self, verbs: &[CardVerb], forward: bool) {
        let drawn: Vec<CardVerb> = verbs.iter().rev().copied().collect();
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
pub(crate) fn key(card: &mut Card, verbs: &[CardVerb], key: Key) -> Option<CardVerb> {
    match key {
        Key::Later => Some(CardVerb::Later),
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
        Bar, Card, CardVerb, Key, Outcome, Paint, RowFoot, Target, copy_command, failed, key,
        paint, row_description_in, row_foot, shown, spend,
    };
    use crate::i18n::{Lang, Text};
    use crate::install_channel::{Channel, Manager};
    use crate::update_job::{
        Bytes, Driver, Effect, Gathered, Job, NoDownloadDoor, Offer, Poster, Presenters, Refused,
        State, Step, Transport, Unsupported, Verb,
    };
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
        fn prepare(&self, _: &Offer, _: &dyn Transport, post: &Poster) -> Result<(), Refused> {
            *self.0.borrow_mut() = Some(post.clone());
            Ok(())
        }
    }

    /// A job whose download of `v0.4.7` was pressed, and the poster its
    /// driver reports through.
    fn downloading() -> (Job<u32>, Poster) {
        let mut job = considered("v0.4.7", Channel::Ours);
        let driver = Starting::default();
        job.answer_verb(Verb::Press, &driver, &NoDownloadDoor)
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
        job.answer_verb(Verb::Press, &Unsupported, &NoDownloadDoor)
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
        assert_eq!(CardVerb::Close.job_verb(), Some(Verb::Later));
        assert_eq!(CardVerb::Releases.job_verb(), None, "a page moves nothing");
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
            .answer_verb(Verb::Restart, &Unsupported, &NoDownloadDoor)
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
            job.answer_verb(Verb::Later, &Unsupported, &NoDownloadDoor),
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
            .answer_verb(Verb::Skip, &Unsupported, &NoDownloadDoor)
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
        assert_eq!(Target::Panel.verb(), None);
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
}
