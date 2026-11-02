//! **A press and a drag inside one of the window's one-line fields** (0.4.9 plan rows 179 and
//! 180; capture C25): the caret goes to the seam under the pointer, a `Shift`+press carries the
//! selection's far end there, and a drag selects what it crosses — in the name editor on every
//! surface it is drawn on (a tab's name, a preview head's file name, the breadcrumb's last
//! segment, a page's address, a files row, a new entry's box), in the find bar, in a commit
//! graph's search, in each text field of the settings dialog, in the palette's box and in a git
//! menu's branch prompt.
//!
//! Each field's own press handler decides that a press is inside the field and does what a press
//! there always did — the name editor's guard consumes it whole (J103), the find bar and the
//! graph's search take the keyboard, the settings dialog moves its focus, the palette and the git
//! menu keep the press inside their box — and then hands the press here. What the press does to
//! the text, the moves under it and its release are read here, where pointer data enters, so
//! `POINTER-DEBT.tsv` gains no row. Where the seams are is the run the field's painter drew the
//! text as ([`crate::text_field::DrawnRun`], kept in [`crate::DrawnFields`]), never a second
//! measurement.

use crate::text_field::{DrawnRun, TextField};
use crate::window_seam::prelude::*;
use crate::{LeafId, PreviewSurface, RenameSubject, Runtime, TabId, settings};
use anyhow::Result;
use std::time::Instant;
use winit::dpi::PhysicalPosition;

/// **Which one-line field a press went into**, and what the field is drawn on — the payload of
/// [`super::CaptureOwner::FieldSelection`], so a drag across it ends when what it is drawn on
/// goes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FieldDrag {
    /// The name editor, and the subject it is open on (row 179).
    Name(RenameSubject),
    /// The find bar, and the pane its capsule stands on.
    FindBar(LeafId),
    /// A commit graph's search, on the surface the graph is drawn on and with the tab whose plane
    /// holds that graph — what a selection drawn on that surface is kept with.
    GraphSearch(TabId, PreviewSurface),
    /// A text field of the settings dialog.
    Settings(settings::SettingsTarget),
    /// The command palette's box.
    Palette,
    /// The branch prompt a git context menu turned into.
    GitPrompt,
}

/// Whether the pointer is pressing or dragging — the two ways a pointer names a seam
/// ([`DrawnRun::press_at`], [`DrawnRun::drag_to`]).
#[derive(Clone, Copy)]
enum Pointer {
    Press,
    Drag,
}

/// **Put a field's caret on the seam `x` names in the run it was drawn as**, carrying the
/// selection's far end when `extend`. `None` while a composition is up — the IME is editing the
/// text at the caret, and moving the caret from under it would part the composition from what it
/// is composing (row 179 R5) — and otherwise whether the caret or the selection moved.
///
/// With no composition, the string on the glass is the field's own text, so every offset the run
/// answers is an offset into it.
fn place(
    field: &mut TextField,
    run: &DrawnRun,
    x: f32,
    pointer: Pointer,
    extend: bool,
) -> Option<bool> {
    if !field.preedit().is_empty() {
        return None;
    }
    let at = match pointer {
        Pointer::Press => run.press_at(x),
        Pointer::Drag => run.drag_to(x),
    };
    let was = (field.caret(), field.selection());
    field.place(at, extend);
    Some((field.caret(), field.selection()) != was)
}

impl<B: Backend> Runtime<'_, B> {
    /// **A press inside one of the window's one-line fields**: the caret to the seam under the
    /// pointer, the selection's far end with it under `Shift`, and the drag that follows held as
    /// [`super::CaptureOwner::FieldSelection`]. Asked by the field's own press handler, after it
    /// has done whatever a press on the field did before (taken the keyboard, moved the focus).
    ///
    /// **A press while the field is composing moves nothing and holds nothing** (row 179 R5): the
    /// press is still the field's, and the caret stays where the IME is composing.
    ///
    /// A press left of the drawn text is the drawn window's start and one right of it is the
    /// nearest seam out there, the text's end for a text that fits (row 179 R6).
    pub(crate) fn press_field(
        &mut self,
        field: FieldDrag,
        position: PhysicalPosition<f64>,
    ) -> Result<()> {
        let extend = self.window.modifiers.shift_key();
        let Some(moved) = self.place_in_field(&field, position.x as f32, Pointer::Press, extend)
        else {
            return Ok(());
        };
        self.latch_field_selection(field.clone());
        if moved {
            self.show_the_placed_caret(&field)?;
        }
        Ok(())
    }

    /// **A press in a commit graph's search field**, after the field has taken the keyboard
    /// (`Runtime::press_graph_tool`): the caret to the seam under the pointer, on the surface the
    /// graph is drawn on and kept with the tab whose plane holds it.
    pub(crate) fn press_graph_search(
        &mut self,
        surface: PreviewSurface,
        position: PhysicalPosition<f64>,
    ) -> Result<()> {
        let tab = self.window.tabs[self.preview_tab_index(surface)].id;
        self.press_field(FieldDrag::GraphSearch(tab, surface), position)
    }

    /// **A move under a press that began in a field**: the caret follows the pointer and the
    /// selection's anchor stays where the press put it — outside the box as well, for the reason
    /// every selection drag in this window is asked outside its own body: a drag that stopped
    /// the moment the hand left a box one line tall would make the last letter a matter of aim.
    /// Past either edge of a box scrolled along its text, the seam out there is reached and the
    /// box follows the caret (row 179's review F5). Answers whether a drag of a field holds the
    /// pointer.
    pub(crate) fn drag_field_selection(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let Some(field) = self.held_field_selection().cloned() else {
            return Ok(false);
        };
        if self.place_in_field(&field, position.x as f32, Pointer::Drag, true) == Some(true) {
            self.show_the_placed_caret(&field)?;
        }
        Ok(true)
    }

    /// **The drag let go**, wherever the button comes up: the selection it drew is the answer.
    pub(crate) fn release_field_selection(&mut self) -> Result<bool> {
        self.drop_field_selection();
        Ok(true)
    }

    /// **A drag across the name editor goes with the editor** (row 179): every exit of the
    /// editor ([`Runtime::finish_rename`]) asks this, and a drag across any other field is left
    /// where it is.
    pub(crate) fn let_go_of_the_name_selection(&mut self) {
        if matches!(self.held_field_selection(), Some(FieldDrag::Name(_))) {
            self.drop_field_selection();
        }
    }

    /// The field a press or a drag is in, with the run it was drawn as, and the caret put — or
    /// `None` when the field is no longer up, was never drawn, or is composing.
    fn place_in_field(
        &mut self,
        field: &FieldDrag,
        x: f32,
        pointer: Pointer,
        extend: bool,
    ) -> Option<bool> {
        let runs = &self.window.drawn_fields;
        match field {
            FieldDrag::Name(_) => {
                let editor = self.window.rename.as_mut()?;
                let run = runs.name.as_ref()?;
                if editor.composing() {
                    return None;
                }
                let at = match pointer {
                    Pointer::Press => run.press_at(x),
                    Pointer::Drag => run.drag_to(x),
                };
                Some(editor.place_at(at, extend))
            }
            FieldDrag::FindBar(_) => place(
                self.window.search.field_mut(),
                runs.find_bar.as_ref()?,
                x,
                pointer,
                extend,
            ),
            FieldDrag::GraphSearch(_, surface) => {
                let run = runs.graph_search.get(surface)?;
                let tab = self.preview_tab_index(*surface);
                let view = self.window.tabs[tab].git_graph_view.get_mut(surface)?;
                place(&mut view.search, run, x, pointer, extend)
            }
            FieldDrag::Settings(target) => {
                let (_, run) = runs.settings.iter().find(|(drawn, _)| drawn == target)?;
                place(
                    self.window.settings.text_field_mut(*target)?,
                    run,
                    x,
                    pointer,
                    extend,
                )
            }
            FieldDrag::Palette => place(
                self.window.palette.as_mut()?.field_mut(),
                runs.palette.as_ref()?,
                x,
                pointer,
                extend,
            ),
            FieldDrag::GitPrompt => place(
                &mut self.window.git_menu.as_mut()?.prompt.as_mut()?.field,
                runs.git_prompt.as_ref()?,
                x,
                pointer,
                extend,
            ),
        }
    }

    /// A caret the pointer moved is drawn where it now stands — and the name editor's, which
    /// blinks, is shown lit, as a typed one is (a caret that blinks out from under the place it
    /// was just put reads as a press that did nothing).
    fn show_the_placed_caret(&mut self, field: &FieldDrag) -> Result<()> {
        if matches!(field, FieldDrag::Name(_)) {
            self.window
                .rename_blink
                .reset(Instant::now(), self.app.motion);
        }
        self.refresh_chrome();
        self.present_chrome_change()
    }
}

#[cfg(test)]
#[path = "field_caret_tests.rs"]
mod tests;
