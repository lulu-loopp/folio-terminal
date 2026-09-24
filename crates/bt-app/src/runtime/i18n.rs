//! `i18n` — moved out of `main.rs`'s `impl Runtime` blocks by
//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.

use crate::{ApplicationChange, Runtime, i18n, resolved_language};
use anyhow::Result;
use bt_render::{FrameSource, FrameTrigger};
use std::time::Instant;

impl Runtime<'_> {
    /// **The window's language, changed while the window is up** (§7.1.6c-3c).
    ///
    /// It used to be the one setting whose effect was not on screen when it was
    /// chosen, and it used to raise a card saying so. Both of those are gone:
    /// [`crate::i18n`]'s header records what had to exist first — a revision, so
    /// that a measurement made in one language cannot be handed back in another
    /// — and what turned out not to need doing, which is most of it.
    ///
    /// **No card.** The row's tick moves and every word in the window moves with
    /// it, so there is nothing left for a card to report that the reader is not
    /// already looking at.
    ///
    /// Silent when the value did not change, for the reason every confirmation in
    /// this window is: pressing the item that already wears the tick is not
    /// something happening. Note that the *stored* value is what is compared —
    /// `System` and `English` are two different answers on an English machine
    /// even though they resolve to one language, and the file has to record which
    /// question the user answered.
    pub(crate) fn apply_language(&mut self, language: bt_persist::LanguageV1) -> Result<bool> {
        if self.app.settings_store.loaded().language == language {
            return Ok(false);
        }
        let mut settings = self.app.settings_store.loaded().clone();
        settings.language = language;
        if !self.app.settings_store.store(settings) {
            return Ok(false);
        }
        self.adopt_new_language()?;
        Ok(true)
    }

    /// **Everything a new language costs this window.**
    ///
    /// Deliberately short, and the shortness is the finding rather than an
    /// omission — see [`crate::i18n`]'s header, which lists what was audited to
    /// arrive at it. A language change is not a font change: the face of the grid
    /// has not moved, so every glyph in it is still the right size, and none of
    /// the renderer's shaping, composed-row or texture caches describes anything
    /// that has changed. What *has* changed is the window's own words, and those
    /// are re-derived from the table on the frame that draws them.
    ///
    /// So there are three steps and each answers for one of the three things
    /// that outlive a repaint:
    ///
    /// 1. `i18n::install` — the answer itself, and the revision behind it. A
    ///    `false` here means the resolved language did not move (`System` chosen
    ///    on a machine already resolving to the same language), and nothing below
    ///    is owed.
    /// 2. [`Self::sync_math_layout_key`] — carries the new `lang_rev` into every
    ///    session's `LayoutKey`, which is the channel every artefact keyed on the
    ///    layout is invalidated through.
    /// 3. [`Self::refresh_chrome`] and a publish — the strip, the tooltip
    ///    anchors, the files feet, `preview_button_width` and the whole overlay
    ///    stack, all rebuilt from the words that are in force now.
    ///
    /// The PSReadLine row's cached line needs no step of its own: it keeps one
    /// slot per language rather than one slot, so step 3 asks it in the new
    /// language and gets the new language back.
    fn adopt_new_language(&mut self) -> Result<()> {
        if !i18n::install(resolved_language(self.app.settings_store.loaded().language)) {
            return Ok(());
        }
        // `i18n::install` is a process-wide switch, so every other window is now
        // drawing yesterday's words until it re-derives its chrome.
        self.note_application_change(ApplicationChange {
            font: false,
            look: true,
            caret: false,
            option: false,
            paid_by: Some(self.window.window.id()),
        });
        self.sync_math_layout_key();
        self.refresh_chrome();
        self.publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })?;
        Ok(())
    }
}
