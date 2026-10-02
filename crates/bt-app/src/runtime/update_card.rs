//! `update_card` — the window's half of the update card and of About → Version's
//! control (0.4.6 ticket U-19): whether the card is up in this window, its
//! placement against the real font, and what a key, a press or the row's control
//! does. What the card says is [`crate::update_card`]'s; every fact it reads is
//! the update job's (`App::update_job`).
//!
//! And the same half of the **uninstaller's confirmation card**, which is the
//! update card's chassis with two verbs of its own, raised over the Settings
//! dialog by `Uninstall…` (T-UNINSTALL-UX); its facts are the dialog's own
//! (`SettingsPanel::uninstall_card`, `SettingsPanel::uninstall_remove_data`).

use crate::{
    Runtime, recoverable_clipboard_write, restore, update, update_card, update_job,
    write_terminal_clipboard_text,
};
use anyhow::Result;
use winit::keyboard::{Key, NamedKey};

/// What is left for the window after About → Version's control has driven the
/// update job: a press that leaves the process, or nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VersionEffect {
    None,
    Check,
    Releases,
    Copy(&'static str),
}

/// **About → Version's control, answered through the update job's own entries**
/// (T-UPDATE-ON-ABOUT round 2, R1–R3).
///
/// *Update and restart* raises the card again for a job waiting at `Verified`
/// (`Job::reopen`, as the row's foot always did), presses the card's `Update`
/// on an `Available` offer, and from `Idle` first raises the asked offer
/// (`Job::offer_again`) in `window`. *Retry* closes a failed card that is still
/// up (its Later), raises the asked offer and presses `Update` — one new
/// attempt, and nothing at all while the job cannot raise it. A free function
/// over the job, the driver and the transport, so the dispatcher's roads run
/// in a test with a recording driver; the product passes
/// `update_job::driver_for_this_copy`'s.
pub(crate) fn dispatch_version_control<W: Copy + Eq>(
    job: &mut update_job::Job<W>,
    window: W,
    control: &update_card::VersionControl,
    known_offer: Option<&str>,
    driver: &dyn update_job::Driver,
    transport: &update_job::SharedTransport,
) -> VersionEffect {
    match control {
        update_card::VersionControl::Check { enabled: true } => VersionEffect::Check,
        update_card::VersionControl::UpdateAndRestart => {
            if matches!(job.state(), update_job::State::Verified(_)) {
                job.reopen(window);
                return VersionEffect::None;
            }
            job.offer_again(window, known_offer);
            if matches!(job.state(), update_job::State::Available(_)) {
                let _ = job.answer_verb(update_job::Verb::Press, driver, transport);
            }
            VersionEffect::None
        }
        update_card::VersionControl::Retry { enabled: true } => {
            if job.asked_offer(known_offer).is_none() {
                return VersionEffect::None;
            }
            if matches!(job.state(), update_job::State::Failed(..)) {
                let _ = job.answer_verb(update_job::Verb::Later, driver, transport);
            }
            if job.offer_again(window, known_offer) {
                let _ = job.answer_verb(update_job::Verb::Press, driver, transport);
            }
            VersionEffect::None
        }
        update_card::VersionControl::OpenReleases => VersionEffect::Releases,
        update_card::VersionControl::CopyCommand { command } => VersionEffect::Copy(command),
        update_card::VersionControl::Check { enabled: false }
        | update_card::VersionControl::Retry { enabled: false }
        | update_card::VersionControl::Progress(_) => VersionEffect::None,
    }
}

impl Runtime<'_> {
    /// **Whether the update card is up in this window** — the job's
    /// [`update_job::Job::card_window`] naming this one. It is also whether the
    /// card holds this window's keyboard and pointer: it does not dim, and
    /// nothing under it answers while it is up (the restore card's
    /// arrangement, 0.4.5 ticket 57).
    pub(crate) fn update_card_is_up(&self) -> bool {
        self.app.update_job.card_window() == Some(self.window.window.id())
    }

    /// The update card, measured against a real font, or nothing while it is
    /// not up here.
    pub(in crate::runtime) fn update_card_layout(&mut self) -> Option<restore::UpdateCardLayout> {
        if !self.update_card_is_up() {
            return None;
        }
        let paint = update_card::paint(self.app.update_job.state())?;
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (width, height) = (width as f32, height as f32);
        let scale = self.window.renderer.scale_factor() as f32;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let content =
            restore::update_card_content(&paint, width, scale, &mut |text, size, weight| {
                renderer.measure_chrome_label(gpu, text, size, weight, 0.0, false)
            });
        Some(restore::update_card_layout(&content, width, height, scale))
    }

    /// The verbs the card that is up here carries, in C9's order.
    fn update_card_verbs(&self) -> Vec<update_card::CardVerb> {
        update_card::paint(self.app.update_job.state())
            .map(|paint| paint.verbs)
            .unwrap_or_default()
    }

    /// **A key on the card.** `Escape` is Later (§B), `Tab` and `Shift+Tab`
    /// move the ring over the verbs, and `Enter` or `Space` press the verb the
    /// ring stands on — nothing until a key has lit it. Every other key is
    /// swallowed.
    pub(in crate::runtime) fn press_update_card_key(&mut self, key: &Key) -> Result<()> {
        let key = match key {
            Key::Named(NamedKey::Escape) => update_card::Key::Later,
            Key::Named(NamedKey::Tab) => update_card::Key::Step {
                forward: !self.window.modifiers.shift_key(),
            },
            Key::Named(NamedKey::Enter | NamedKey::Space) => update_card::Key::Press,
            _ => return Ok(()),
        };
        let verbs = self.update_card_verbs();
        match update_card::key(&mut self.window.update_card, &verbs, key) {
            Some(verb) => self.answer_update_card(verb),
            None => {
                if self.refresh_overlay() {
                    self.present_chrome_change()?;
                }
                Ok(())
            }
        }
    }

    /// A press on the card, answered by what it landed on. The face and the
    /// window beside the card answer nothing; the `×` is Later.
    pub(in crate::runtime) fn press_update_card(
        &mut self,
        target: update_card::Target,
    ) -> Result<()> {
        match target.verb() {
            Some(verb) => self.answer_update_card(verb),
            None => Ok(()),
        }
    }

    /// **Spend a verb of the card.**
    ///
    /// A job verb goes to the job (`Job::answer_verb`), with this copy's
    /// driver and transport (`update_job::driver_for_this_copy`: the macOS
    /// Prepare and the download door, U-27; Windows' driver is U-20's), and
    /// Skip's write is handed to the check's one owner ([`update::skip`],
    /// coordinator ruling 10). **Restart** is the application's quit with the
    /// update's reason (`App::restart_for_update`, which moves the job), not a
    /// job verb (U-31). A card is up only where offers are on
    /// (`Job::offers_enabled`: Windows since U-31). `Releases` and `Show folder`
    /// leave the window through the doors every such press uses and move
    /// nothing. The repaint is the loop's
    /// (`FolioApp::settle_update_card`), which sees what the job now shows.
    pub(in crate::runtime) fn answer_update_card(
        &mut self,
        verb: update_card::CardVerb,
    ) -> Result<()> {
        match verb.asks() {
            update_card::Asks::Job(job_verb) => {
                let (driver, transport) = update_job::driver_for_this_copy();
                let answered =
                    self.app
                        .update_job
                        .answer_verb(job_verb, driver.as_ref(), &transport);
                if let Ok(effect) = answered
                    && let Err(error) = update_card::spend(effect, update::skip)
                {
                    eprintln!("BT_UPDATE the skipped version was not written: {error}");
                }
            }
            update_card::Asks::Quit => {
                // A refusal is the job's own for its state (a quit already
                // under way answers instead); nothing moves, as for a refused
                // job verb.
                let _ = self.app.restart_for_update();
            }
            update_card::Asks::Releases => {
                self.hand_url_to_the_browser(update::RELEASES_PAGE)?;
            }
            update_card::Asks::ShowFolder => {
                if let Some(folder) =
                    update_card::paint(self.app.update_job.state()).and_then(|paint| paint.folder)
                {
                    self.reveal_in_explorer(&folder);
                }
            }
        }
        Ok(())
    }

    /// **Whether the uninstaller's confirmation card is up in this window**
    /// (T-UNINSTALL-UX): raised over the Settings dialog, and only while the
    /// dialog is. It holds the keyboard and the pointer while it is up, and the
    /// dialog is not drawn behind it (the update card's arrangement).
    pub(crate) fn uninstall_card_is_up(&self) -> bool {
        self.window.settings.is_open() && self.window.settings.uninstall_card().is_some()
    }

    /// The confirmation card, measured against a real font, or nothing while it
    /// is not up here.
    pub(in crate::runtime) fn uninstall_card_layout(
        &mut self,
    ) -> Option<restore::UpdateCardLayout<update_card::UninstallVerb>> {
        if !self.uninstall_card_is_up() {
            return None;
        }
        let paint = update_card::uninstall_paint(self.window.settings.uninstall_remove_data());
        let (width, height) = self.window.renderer.presentation_geometry().swapchain_size;
        let (width, height) = (width as f32, height as f32);
        let scale = self.window.renderer.scale_factor() as f32;
        let (gpu, renderer) = (&mut self.app.gpu, &mut self.window.renderer);
        let content =
            restore::update_card_content(&paint, width, scale, &mut |text, size, weight| {
                renderer.measure_chrome_label(gpu, text, size, weight, 0.0, false)
            });
        Some(restore::update_card_layout(&content, width, height, scale))
    }

    /// The confirmation card as one overlay layer, with its hover and ring.
    pub(in crate::runtime) fn uninstall_card_build(
        &self,
        layout: &restore::UpdateCardLayout<update_card::UninstallVerb>,
    ) -> Vec<crate::marks::OverlayLayer> {
        let card = self
            .window
            .settings
            .uninstall_card()
            .copied()
            .unwrap_or_default();
        let verbs =
            update_card::uninstall_paint(self.window.settings.uninstall_remove_data()).verbs;
        restore::update_card_build(layout, card.hover(), card.ring(&verbs))
    }

    /// **A key on the confirmation card**: the update card's keys — `Escape` is
    /// Cancel, `Tab` and `Shift+Tab` move the ring, `Enter` and `Space` press
    /// the verb the ring stands on and nothing until a key has lit it. Every
    /// other key is swallowed.
    pub(in crate::runtime) fn press_uninstall_card_key(&mut self, key: &Key) -> Result<()> {
        let key = match key {
            Key::Named(NamedKey::Escape) => update_card::Key::Later,
            Key::Named(NamedKey::Tab) => update_card::Key::Step {
                forward: !self.window.modifiers.shift_key(),
            },
            Key::Named(NamedKey::Enter | NamedKey::Space) => update_card::Key::Press,
            _ => return Ok(()),
        };
        let verbs =
            update_card::uninstall_paint(self.window.settings.uninstall_remove_data()).verbs;
        let answered = self
            .window
            .settings
            .uninstall_card_mut()
            .and_then(|card| update_card::key(card, &verbs, key));
        match answered {
            Some(verb) => self.answer_uninstall_card(verb),
            None => self.repaint_uninstall_card(),
        }
    }

    /// **The pointer over the confirmation card**; answers whether it was the
    /// card's — which, while it is up, is always.
    pub(in crate::runtime) fn hover_uninstall_card(&mut self, x: f64, y: f64) -> Result<bool> {
        let Some(layout) = self.uninstall_card_layout() else {
            return Ok(false);
        };
        let over = Some(restore::update_card_hit(&layout, x, y));
        let moved = self
            .window
            .settings
            .uninstall_card_mut()
            .is_some_and(|card| card.set_hover(over));
        if moved {
            self.repaint_uninstall_card()?;
        }
        Ok(true)
    }

    /// A press on the confirmation card, answered by what it landed on. The
    /// face and the window beside the card answer nothing; the `×` is Cancel.
    pub(in crate::runtime) fn press_uninstall_card(
        &mut self,
        target: update_card::Target<update_card::UninstallVerb>,
    ) -> Result<()> {
        match target.verb() {
            Some(verb) => self.answer_uninstall_card(verb),
            None => Ok(()),
        }
    }

    /// **Spend a verb of the confirmation card.** *Uninstall* asks the
    /// application's ordinary quit and arms its way out to start the
    /// uninstaller as its last act (`App::uninstall_on_quit`), with the
    /// switch's answer; *Cancel* puts the card away and changes nothing.
    fn answer_uninstall_card(&mut self, verb: update_card::UninstallVerb) -> Result<()> {
        let remove_data = self.window.settings.uninstall_remove_data();
        self.window.settings.put_uninstall_card_away();
        if verb == update_card::UninstallVerb::Uninstall {
            self.app.uninstall_on_quit(remove_data);
        }
        self.repaint_uninstall_card()
    }

    /// The card, or the dialog under it, drawn again.
    fn repaint_uninstall_card(&mut self) -> Result<()> {
        if self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        Ok(())
    }

    /// About > Version's state control ([`dispatch_version_control`]) in this
    /// window. Copy and the releases page keep their existing roads, and Check
    /// uses the shared check worker with its timestamp gate bypassed. The
    /// repaint is the loop's (`FolioApp::settle_update_card`).
    pub(crate) fn press_update_row_foot(&mut self) -> Result<()> {
        let control = self.settings_values().version_update.control;
        let known_offer = update::offer();
        let (driver, transport) = update_job::driver_for_this_copy();
        match dispatch_version_control(
            &mut self.app.update_job,
            self.window.window.id(),
            &control,
            known_offer.as_deref(),
            driver.as_ref(),
            &transport,
        ) {
            VersionEffect::Check => {
                let _ = update::begin_now();
            }
            VersionEffect::Releases => {
                self.hand_url_to_the_browser(update::RELEASES_PAGE)?;
            }
            VersionEffect::Copy(command) => {
                recoverable_clipboard_write(
                    write_terminal_clipboard_text(command),
                    "copy the package manager's update command",
                );
            }
            VersionEffect::None => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::sync::Arc;

    use bt_persist::UpdateCheckV1;
    use bt_platform::HostPlatform;

    use super::{VersionEffect, dispatch_version_control};
    use crate::i18n::Lang;
    use crate::install_channel::Channel;
    use crate::update::CheckView;
    use crate::update_card::{VersionControl, VersionRow, version_row};
    use crate::update_job::{
        Driver, Failure, Gathered, Job, NoDownloadDoor, Offer, Poster, Presenters, Refused,
        SharedTransport, State, Step, Stop, Verb,
    };
    use crate::update_txn::TxnId;

    /// A driver that records every Prepare it is asked for, and the poster
    /// each one was handed.
    #[derive(Default)]
    struct Starts {
        count: Cell<usize>,
        posters: RefCell<Vec<Poster>>,
    }

    impl Driver for Starts {
        fn prepare(&self, _: &Offer, _: &SharedTransport, post: &Poster) -> Result<(), Refused> {
            self.count.set(self.count.get() + 1);
            self.posters.borrow_mut().push(post.clone());
            Ok(())
        }
    }

    fn evidence(latest: &str) -> Gathered {
        Gathered {
            check: Some(UpdateCheckV1 {
                latest_tag: Some(latest.to_owned()),
                ..UpdateCheckV1::default()
            }),
            channel: Some(Channel::Ours),
            running: "0.4.6",
            capable: true,
            trial: false,
            platform: HostPlatform::Windows,
        }
    }

    fn consider(job: &mut Job<u32>, latest: &str) {
        job.consider(
            evidence(latest),
            &Presenters {
                visited: &[1],
                open: &[1],
                quake: None,
            },
            || TxnId::new([1; 16]),
        );
    }

    fn offered() -> Job<u32> {
        let mut job = Job::with_offers(true);
        consider(&mut job, "v0.4.7");
        job
    }

    fn no_door() -> SharedTransport {
        Arc::new(NoDownloadDoor)
    }

    fn row(job: &Job<u32>, offered: &str) -> VersionRow {
        version_row(job, CheckView::default(), Some(offered), 0, Lang::English)
    }

    /// RED (T-UPDATE-ON-ABOUT round 2, R1) — **Available → Later → About →
    /// Update starts the download**: the offer put away with Later is raised
    /// again by the press, in the window it was pressed in, and the card's
    /// own Update verb starts exactly one Prepare.
    ///
    /// MUTATION: make `Job::offer_again` return false when
    /// `offered_this_launch` is set; the job stays Idle and no Prepare starts.
    #[test]
    fn available_later_about_update_starts_the_download() {
        let mut job = offered();
        assert!(matches!(job.state(), State::Available(_)));
        job.answer_verb(Verb::Later, &Starts::default(), &no_door())
            .expect("Later");
        assert!(matches!(job.state(), State::Idle));
        let shown = row(&job, "v0.4.7");
        assert_eq!(shown.control, VersionControl::UpdateAndRestart);

        let driver = Starts::default();
        let effect = dispatch_version_control(
            &mut job,
            2,
            &shown.control,
            Some("v0.4.7"),
            &driver,
            &no_door(),
        );
        assert_eq!(effect, VersionEffect::None);
        assert_eq!(driver.count.get(), 1, "one Prepare");
        assert!(
            matches!(job.state(), State::Downloading(offer, _) if offer.tag() == "v0.4.7"),
            "{:?}",
            job.state()
        );
        assert_eq!(job.presenter(), Some(2), "in the pressed window");
    }

    /// RED (T-UPDATE-ON-ABOUT round 2, R1) — **a reader's Check after Later
    /// that learns a newer release is what the row names and asks for**: the
    /// job keeps its decision current after the launch's one unasked offer,
    /// without raising a second unasked card.
    ///
    /// MUTATION: drop the decision refresh from `Job::consider`'s
    /// `offered_this_launch` branch; the row falls back to "Up to date" and
    /// the press starts nothing.
    #[test]
    fn a_newer_answer_after_later_is_the_offer_the_row_raises() {
        let mut job = offered();
        job.answer_verb(Verb::Later, &Starts::default(), &no_door())
            .expect("Later");
        consider(&mut job, "v0.4.8");
        assert!(matches!(job.state(), State::Idle), "no second unasked card");
        let shown = row(&job, "v0.4.8");
        assert_eq!(shown.control, VersionControl::UpdateAndRestart);
        assert!(shown.value.ends_with("v0.4.8 available"), "{}", shown.value);

        let driver = Starts::default();
        dispatch_version_control(
            &mut job,
            1,
            &shown.control,
            Some("v0.4.8"),
            &driver,
            &no_door(),
        );
        assert_eq!(driver.count.get(), 1);
        assert!(matches!(job.state(), State::Downloading(offer, _) if offer.tag() == "v0.4.8"));
    }

    /// RED (T-UPDATE-ON-ABOUT round 2, R3) — **Downloading → Failed → (close
    /// the card) → Retry starts exactly one new download.** The failure stays
    /// on Version after its card is closed; Retry raises the asked offer and
    /// presses Update once; the successful start clears the remembered
    /// failure; a stale second Retry while that attempt runs starts nothing.
    ///
    /// MUTATION: route `VersionControl::Retry` to `VersionEffect::Check` (the
    /// round-1 road); the second Prepare never starts.
    #[test]
    fn downloading_failed_closed_retry_starts_exactly_one_new_download() {
        let mut job = offered();
        let driver = Starts::default();
        dispatch_version_control(
            &mut job,
            1,
            &VersionControl::UpdateAndRestart,
            Some("v0.4.7"),
            &driver,
            &no_door(),
        );
        assert_eq!(driver.count.get(), 1);
        driver.posters.borrow()[0].post(Step::Stopped(Stop::Download));
        assert_eq!(job.drain_progress(), 0);
        assert!(matches!(job.state(), State::Failed(..)));
        job.answer_verb(Verb::Later, &driver, &no_door())
            .expect("close the failed card");
        assert!(matches!(job.state(), State::Idle));

        let failed = row(&job, "v0.4.7");
        assert_eq!(failed.control, VersionControl::Retry { enabled: true });
        assert!(
            failed
                .value
                .ends_with("v0.4.7 wasn't installed. This version was restored."),
            "{}",
            failed.value
        );
        let effect = dispatch_version_control(
            &mut job,
            2,
            &failed.control,
            Some("v0.4.7"),
            &driver,
            &no_door(),
        );
        assert_eq!(effect, VersionEffect::None);
        assert_eq!(driver.count.get(), 2, "one retry starts one new Prepare");
        assert!(matches!(job.state(), State::Downloading(..)));
        assert!(job.last_failure().is_none(), "a successful start clears it");

        dispatch_version_control(
            &mut job,
            2,
            &VersionControl::Retry { enabled: true },
            Some("v0.4.7"),
            &driver,
            &no_door(),
        );
        assert_eq!(driver.count.get(), 2, "a running attempt is not duplicated");
    }

    /// RED (T-UPDATE-ON-ABOUT round 2, R3) — **a recovery that may still
    /// complete cannot be raced by Retry**: a launch a rollback sent to say
    /// the update is incomplete (its trial can still be committed forward)
    /// shows the failure with Retry disabled, and even a Retry press starts
    /// nothing — before and after its card is closed.
    ///
    /// MUTATION: drop `!self.said_incomplete` from `Job::asked_offer`; Retry is
    /// enabled and the press starts a Prepare.
    #[test]
    fn an_incomplete_recovery_cannot_be_retried() {
        let mut job = Job::with_offers(true).after_rollback(Some(Failure::Incomplete {
            folder: PathBuf::from("txn-更新-1"),
        }));
        consider(&mut job, "v0.4.7");
        let driver = Starts::default();
        for close in [false, true] {
            if close {
                job.answer_verb(Verb::Later, &driver, &no_door())
                    .expect("close the failed card");
            }
            let failed = row(&job, "v0.4.7");
            assert_eq!(
                failed.control,
                VersionControl::Retry { enabled: false },
                "closed: {close}"
            );
            dispatch_version_control(
                &mut job,
                1,
                &VersionControl::Retry { enabled: true },
                Some("v0.4.7"),
                &driver,
                &no_door(),
            );
            assert_eq!(driver.count.get(), 0, "closed: {close}");
        }
    }

    /// RED (T-UPDATE-ON-ABOUT round 2, R2) — **a job waiting at Verified is
    /// raised again, not restarted, by Update and restart** (the row's foot's
    /// road before this ticket): the card comes up in the pressed window and
    /// the job stays Verified.
    ///
    /// MUTATION: drop the `Verified` arm so the press falls through to
    /// `offer_again`; the card stays where it was.
    #[test]
    fn update_and_restart_on_a_verified_job_reopens_its_card() {
        let offer =
            Offer::mint(TxnId::new([4; 16]), "v0.4.7", HostPlatform::Windows).expect("an offer");
        let mut job = Job::<u32>::verified_for_test(offer);
        let driver = Starts::default();
        let effect = dispatch_version_control(
            &mut job,
            3,
            &VersionControl::UpdateAndRestart,
            Some("v0.4.7"),
            &driver,
            &no_door(),
        );
        assert_eq!(effect, VersionEffect::None);
        assert!(matches!(job.state(), State::Verified(_)));
        assert_eq!(job.card_window(), Some(3));
        assert_eq!(driver.count.get(), 0);
    }
}
