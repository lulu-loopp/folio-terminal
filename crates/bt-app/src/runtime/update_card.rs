//! `update_card` — the window's half of the update card and of the General
//! row's foot (0.4.6 ticket U-19): whether the card is up in this window, its
//! placement against the real font, and what a key, a press or the row's foot
//! does. What the card says is [`crate::update_card`]'s; every fact it reads is
//! the update job's (`App::update_job`).

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
    /// coordinator ruling 10). Offers are off (`Job::offers_enabled`), so no
    /// card carries a press yet. `Releases` and `Show folder` leave the window through the doors
    /// every such press uses and move nothing. The repaint is the loop's
    /// (`FolioApp::settle_update_card`), which sees what the job now shows.
    pub(in crate::runtime) fn answer_update_card(
        &mut self,
        verb: update_card::CardVerb,
    ) -> Result<()> {
        match verb.job_verb() {
            Some(job_verb) => {
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
            None => match verb {
                update_card::CardVerb::Releases => {
                    self.hand_url_to_the_browser(update::RELEASES_PAGE)?;
                }
                update_card::CardVerb::ShowFolder => {
                    if let Some(folder) = update_card::paint(self.app.update_job.state())
                        .and_then(|paint| paint.folder)
                    {
                        self.reveal_in_explorer(&folder);
                    }
                }
                _ => {}
            },
        }
        Ok(())
    }

    /// **The General row's foot** (0.4.6 U-19), answered by what the job
    /// says it is: the releases page as before; `Restart to update` raises the
    /// verified job's card again, in this window; `Copy` puts the package
    /// manager's command on the clipboard through the window's one clipboard
    /// write — on this press and on nothing else.
    pub(crate) fn press_update_row_foot(&mut self) -> Result<()> {
        match update_card::row_foot(&self.app.update_job) {
            update_card::RowFoot::ReleasesPage => {
                self.hand_url_to_the_browser(update::RELEASES_PAGE)?;
            }
            update_card::RowFoot::Restart { .. } => {
                let window = self.window.window.id();
                self.app.update_job.reopen(window);
            }
            foot @ update_card::RowFoot::Copy { .. } => {
                recoverable_clipboard_write(
                    update_card::copy_command(&foot, write_terminal_clipboard_text).map(drop),
                    "copy the package manager's update command",
                );
            }
        }
        Ok(())
    }
}
