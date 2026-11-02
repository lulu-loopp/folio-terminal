//! **A press and a drag inside the open name editor** (0.4.9 plan row 179; capture C25): the
//! caret goes to the seam under the pointer, a `Shift`+press carries the selection's far end
//! there, and a drag selects what it crosses — on every surface the one editor is drawn on, a
//! tab's name, a preview head's file name, the breadcrumb's last segment, a page's address, a
//! files row and a new entry's box.
//!
//! The press is the rename guard's in `chrome_mouse_input`, which decides that a press is inside
//! the editor and consumes it whole (J103: no drag of the tab armed, no click recorded); what the
//! press then does to the draft, the moves under it and its release are read here, where pointer
//! data enters, so `POINTER-DEBT.tsv` gains no row. Where the seams are is the run the painter
//! drew the draft as ([`crate::RenameDrawn`]), never a second measurement.

use crate::Runtime;
use crate::window_seam::prelude::*;
use anyhow::Result;
use std::time::Instant;
use winit::dpi::PhysicalPosition;

impl<B: Backend> Runtime<'_, B> {
    /// **A press inside the open editor**: the caret to the seam under the pointer, the
    /// selection's far end with it under `Shift`, and the drag that follows held as
    /// [`super::CaptureOwner::RenameSelection`].
    ///
    /// **A press while a composition is up moves nothing and holds nothing** (row 179 R5): the
    /// press is still the editor's, as it always was, and the caret stays where the IME is
    /// composing.
    ///
    /// A press left of the drawn text is the drawn window's start and one right of it is the
    /// draft's end (R6) — the editor's extent is the whole tab body, and a press on its padding or
    /// its mark is still a press in the editor, never a blur.
    pub(crate) fn press_rename_draft(&mut self, position: PhysicalPosition<f64>) -> Result<()> {
        let extend = self.window.modifiers.shift_key();
        let (Some(editor), Some(drawn)) = (
            self.window.rename.as_mut(),
            self.window.rename_drawn.as_ref(),
        ) else {
            return Ok(());
        };
        if editor.composing() {
            return Ok(());
        }
        let moved = editor.place_at(drawn, position.x as f32, extend);
        let subject = editor.subject.clone();
        self.latch_rename_selection(subject);
        if moved {
            self.show_the_placed_caret()?;
        }
        Ok(())
    }

    /// **A move under a press that began in the editor**: the caret follows the pointer and the
    /// selection's anchor stays where the press put it — outside the box as well, for the reason
    /// every selection drag in this window is asked outside its own body: a drag that stopped
    /// the moment the hand left a box one line tall would make the last letter a matter of aim.
    /// Answers whether a drag of the editor holds the pointer.
    pub(crate) fn drag_rename_selection(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        if self.held_rename_selection().is_none() {
            return Ok(false);
        }
        let (Some(editor), Some(drawn)) = (
            self.window.rename.as_mut(),
            self.window.rename_drawn.as_ref(),
        ) else {
            return Ok(true);
        };
        if !editor.composing() && editor.place_at(drawn, position.x as f32, true) {
            self.show_the_placed_caret()?;
        }
        Ok(true)
    }

    /// **The drag let go**, wherever the button comes up: the selection it drew is the answer.
    pub(crate) fn release_rename_selection(&mut self) -> Result<bool> {
        self.drop_rename_selection();
        Ok(true)
    }

    /// A caret the pointer moved is shown lit, as a typed one is (a caret that blinks out from
    /// under the place it was just put reads as a press that did nothing), and drawn.
    fn show_the_placed_caret(&mut self) -> Result<()> {
        self.window
            .rename_blink
            .reset(Instant::now(), self.app.motion);
        self.refresh_chrome();
        self.present_chrome_change()
    }
}

#[cfg(test)]
#[path = "rename_caret_tests.rs"]
mod tests;
