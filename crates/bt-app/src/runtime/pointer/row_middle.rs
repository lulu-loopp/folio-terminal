//! **A middle click on a file row** (owner's ruling 2026-10-23, 0.4.9 plan row 45; capture
//! C24): the browser's "middle click opens it in a new tab", in this window's words — the file
//! opens in a new preview pane beside the one a double click would replace, and that pane keeps
//! what it shows ([`Runtime::open_preview_beside`]).
//!
//! The press, the move and the release all live here, where pointer data is read: the press is
//! held as [`super::CaptureOwner::RowMiddlePress`], the one owner latched by the middle button, so its
//! release is the slot's and reaches it wherever the button comes up; the release decides.

use crate::window_seam::prelude::*;
use crate::{
    DragLatch, FilesEditRow, PointerTarget, RowActivation, RowHost, RowMiddlePress, Runtime, files,
    files_row_activation, float, seats,
};
use anyhow::Result;
use std::time::Instant;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton};

impl<B: Backend> Runtime<'_, B> {
    /// **A middle press on a file row**, on a column's tree or a floating one's — the two hosts
    /// whose second left press opens a file. Answers whether the press was taken; every other
    /// press, a folder row's included, is not, and goes on down the road it always took.
    ///
    /// **It decides nothing**: it is held, and its release decides
    /// ([`Self::release_files_row_middle`]), so a press that travels past the row drag's six
    /// pixels ([`DragLatch`], P81) is no click.
    ///
    /// **Nothing else about the column moves.** The press selects no row, takes no keyboard and
    /// focuses no pane: a middle click is aimed past the list, at the pane it opens, and the
    /// browser's own middle click leaves the page it was made on in front. What it does say is
    /// that it is not half of a double click — the chain breaks, as a folder press breaks it —
    /// and the glance card goes, for P149's reason: a press is the hand saying it is done
    /// glancing.
    pub(crate) fn press_files_row_middle(
        &mut self,
        state: ElementState,
        button: MouseButton,
    ) -> Result<bool> {
        if state != ElementState::Pressed || button != MouseButton::Middle {
            return Ok(false);
        }
        let Some(position) = self.window.pointer_position else {
            return Ok(false);
        };
        let (host, index) = match self.pointer_target_at(position) {
            Some(PointerTarget::Chrome(seats::ChromeTarget::FilesRow { seat, index })) => {
                (RowHost::Column(seat), index)
            }
            Some(PointerTarget::Float(id, float::FloatPart::Row(index))) => {
                (RowHost::Float(id), index)
            }
            _ => return Ok(false),
        };
        let Some((_, rows)) = self.host_rows(host) else {
            return Ok(false);
        };
        let Some(row) = rows
            .get(index)
            .filter(|row| row.kind == files::RowKind::File)
        else {
            return Ok(false);
        };
        // **The row a new entry is being named in opens nothing** — it is in the list because
        // the box has to stand somewhere, and it names no file on the disk until Enter is
        // pressed (the file menu's own refusal, `Runtime::file_row_under`).
        let pending = match host {
            RowHost::Column(seat) => {
                matches!(
                    self.files_edit_place(),
                    Some(place) if place.seat == seat && matches!(place.at, FilesEditRow::New { .. })
                ) && self
                    .files_trees(Instant::now())
                    .get(&seat)
                    .and_then(|tree| tree.edit.as_ref())
                    .is_some_and(|edit| edit.at == index)
            }
            RowHost::Float(_) | RowHost::Git(_) | RowHost::Terminal(_) => false,
        };
        if pending {
            return Ok(false);
        }
        let key = row.key.clone();
        self.window.files_row_clicks.interrupt();
        if self.hide_file_peek() && self.refresh_overlay() {
            self.present_chrome_change()?;
        }
        self.mouse_trace(|| format!("press_files_row_middle host={host:?} key={key}"));
        self.latch_row_middle_press(RowMiddlePress {
            host,
            key,
            latch: DragLatch::new(position),
        });
        Ok(true)
    }

    /// The pointer moved with a middle press on a file row held: past the row drag's six pixels
    /// it is no click, and a middle press carries no row. Told and not consumed, so every hover
    /// still hears the move.
    pub(crate) fn travel_files_row_middle(&mut self, position: PhysicalPosition<f64>) {
        let scale = self.window.renderer.scale_factor();
        if let Some(press) = self.held_row_middle_press_mut() {
            press.latch.travelled(position, scale);
        }
    }

    /// **The release of a middle press on a file row** — a click unless it travelled, and a
    /// click opens the file beside the preview. Always taken: the press was the row's.
    ///
    /// The file is the one the press named, resolved against its host's root as it stands now —
    /// the resolution a double click makes ([`files_row_activation`]). A host that went while
    /// the button was down never reaches here: its capture went with it (R-9).
    ///
    /// **The row is looked up again, by its key, before anything opens** — the left drag's
    /// `row_payload` asks the same question at the same moment. A double click has no time
    /// between reading its row and acting on it; a held press does, and in it a refresh can take
    /// the file off the disk, or a new root can leave the key naming nothing. A press whose file
    /// row is gone ends having opened nothing.
    pub(crate) fn release_files_row_middle(&mut self) -> Result<bool> {
        let Some(press) = self.take_row_middle_press() else {
            return Ok(false);
        };
        if press.latch.begun {
            return Ok(true);
        }
        // The host's root as it stands now, through the one door both hosts' rows are read by.
        let Some((root, rows)) = self.host_rows(press.host) else {
            return Ok(true);
        };
        if !rows
            .iter()
            .any(|row| row.key == press.key && row.kind == files::RowKind::File)
        {
            return Ok(true);
        }
        if let RowActivation::Preview(path) = files_row_activation(&root, &press.key) {
            self.open_preview_beside(path)?;
        }
        Ok(true)
    }
}
