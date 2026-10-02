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

    /// About > Version's state control. Update enters through the card's exact
    /// `Update` verb, Copy uses the existing clipboard road, and Check/Retry use
    /// the daily check's worker door with its timestamp gate bypassed.
    pub(crate) fn press_update_row_foot(&mut self) -> Result<()> {
        match self.settings_values().version_update.control {
            update_card::VersionControl::Check { enabled: true }
            | update_card::VersionControl::Retry => {
                let _ = update::begin_now();
            }
            update_card::VersionControl::UpdateAndRestart => {
                let verb = if matches!(self.app.update_job.state(), update_job::State::Verified(_))
                {
                    update_card::CardVerb::Restart
                } else {
                    update_card::CardVerb::Update
                };
                self.answer_update_card(verb)?;
            }
            update_card::VersionControl::CopyCommand { command } => {
                recoverable_clipboard_write(
                    write_terminal_clipboard_text(command),
                    "copy the package manager's update command",
                );
            }
            update_card::VersionControl::Check { enabled: false }
            | update_card::VersionControl::Progress(_) => {}
        }
        Ok(())
    }
}
