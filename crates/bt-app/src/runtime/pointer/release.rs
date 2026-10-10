//! **The release goes to the capture's owner** (`docs/plans/design/pointer-capture-2026-10-09.md`
//! R-6, cut 3): while a capture is held, the release that ends it is delivered to its owner
//! before any layer is asked, wherever the pointer is and over whatever is drawn there.
//!
//! One door, [`Runtime::release_capture`], replaces the five roads a release used to take — the
//! cell route's own release, the chrome router's release ladder, the floating window's carry, the
//! glance card's two and the settings sheet's — each of which a layer above it could pre-empt.

use crate::{
    MouseRoute, PressedCellTarget, Runtime, TabClick, UserInputKind, mouse_trace,
    protocol_mouse_button, route_forwarded_mouse_button, seats, settings,
};
use anyhow::Result;
use std::time::Instant;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton};

use super::capture::{
    CaptureOwner, PressVerdict, press_against_the_slot, release_ends_the_capture,
};

impl Runtime<'_> {
    /// **A press against the slot** (R-5): a press of the held capture's own button proves the
    /// capture stale — its release was lost — so the capture is cancelled before the press is
    /// routed, whichever window holds it. A press of another button is routed as it always was
    /// (cut 4 rules on it).
    pub(crate) fn press_against_the_capture(&mut self, button: MouseButton) -> Result<()> {
        match press_against_the_slot(self.app.pointer_capture.as_ref(), button) {
            PressVerdict::Route => Ok(()),
            PressVerdict::CancelThenRoute => self.cancel_stale_capture(),
        }
    }

    /// **Cancel a capture whose release was lost.** This window's own is ended by its owner's
    /// cancel: a divider puts its ratio back, a drag goes home, a rendered selection drops its
    /// link, everything else lets go keeping what it already wrote. Another window's is handed to
    /// that window (`App::stale_capture`), which runs the same ending on its next turn; the slot
    /// is empty either way before the press is routed.
    fn cancel_stale_capture(&mut self) -> Result<()> {
        if self.own_capture().is_none() {
            self.app.stale_capture = self.app.pointer_capture.take();
            return Ok(());
        }
        self.cancel_own_capture()
    }

    /// End this window's capture with its owner's cancel.
    pub(crate) fn cancel_own_capture(&mut self) -> Result<()> {
        let Some(owner) = self.own_capture().map(|capture| capture.owner.clone()) else {
            return Ok(());
        };
        match owner {
            CaptureOwner::Divider(_) => {
                self.cancel_divider_drag()?;
            }
            CaptureOwner::Drag(_) => {
                self.cancel_drag()?;
            }
            CaptureOwner::RenderedSelection(..) => self.cancel_preview_text_drag()?,
            CaptureOwner::TabPress(_)
            | CaptureOwner::PanePress(..)
            | CaptureOwner::RowPress(_)
            | CaptureOwner::FloatHeadPress(_)
            | CaptureOwner::FloatDrag(_)
            | CaptureOwner::GlanceHeadPress(_)
            | CaptureOwner::GlanceThumb(_)
            | CaptureOwner::VideoBar(_)
            | CaptureOwner::PreviewBodyThumb(..)
            | CaptureOwner::BlockThumb(..)
            | CaptureOwner::PicturePan(..)
            | CaptureOwner::EditSelection(..)
            | CaptureOwner::TerminalThumb(..)
            | CaptureOwner::TerminalFootMark(..)
            | CaptureOwner::Route(_)
            | CaptureOwner::SettingsSlider(_)
            | CaptureOwner::SettingsMenuBar(_) => {
                self.app.pointer_capture = None;
            }
        }
        Ok(())
    }

    /// **The release of the capture's button, delivered to its owner** (R-6) — asked first of
    /// every release, so no layer drawn where the button comes up can take it. Answers whether
    /// the release was consumed; a release no capture owns, or one the owner hands on (the glance
    /// card's thumb, a peek's head press), goes on down the ordinary road.
    pub(crate) fn release_capture(
        &mut self,
        button: MouseButton,
        position: Option<PhysicalPosition<f64>>,
    ) -> Result<bool> {
        let Some(capture) = self.own_capture() else {
            return Ok(false);
        };
        if !release_ends_the_capture(capture, button) {
            return Ok(false);
        }
        match &capture.owner {
            CaptureOwner::Route(_) => self.release_cell_route(button),
            // Letting go of the thumb pays for a frame and consumes nothing: the same button
            // coming up still reaches whatever else was waiting for it.
            CaptureOwner::GlanceThumb(_) => {
                self.release_file_peek_thumb()?;
                Ok(false)
            }
            // The head's other meaning, and it *is* claimed: the press was consumed on the way
            // down so that the hand could still choose, and the release is where it is spent.
            CaptureOwner::GlanceHeadPress(_) => self.release_file_peek_press(),
            // "Pressed and let go without moving" is the absence of a gesture.
            CaptureOwner::FloatHeadPress(_) => {
                self.drop_float_head_press();
                Ok(false)
            }
            // A float carry ends wherever it lands, and the hand opens onto whatever the release
            // left it over — asked, because the hover underneath is as old as the gesture.
            CaptureOwner::FloatDrag(_) => {
                self.drop_float_drag();
                if let Some(position) = self.window.pointer_position {
                    self.drive_float_hover(position)?;
                }
                self.apply_pointer_cursor();
                Ok(true)
            }
            // A slider or a menu bar of the settings sheet ends wherever the button comes up.
            CaptureOwner::SettingsSlider(_) | CaptureOwner::SettingsMenuBar(_) => {
                self.app.pointer_capture = None;
                if let (Some(layout), Some(position)) =
                    (self.settings_layout(), self.window.pointer_position)
                {
                    let hover =
                        settings::hit(&layout, &self.settings_values(), position.x, position.y);
                    self.window.settings.set_hover(Some(hover));
                }
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
                Ok(true)
            }
            CaptureOwner::Divider(_)
            | CaptureOwner::TabPress(_)
            | CaptureOwner::PanePress(..)
            | CaptureOwner::RowPress(_)
            | CaptureOwner::Drag(_)
            | CaptureOwner::VideoBar(_)
            | CaptureOwner::PreviewBodyThumb(..)
            | CaptureOwner::BlockThumb(..)
            | CaptureOwner::PicturePan(..)
            | CaptureOwner::EditSelection(..)
            | CaptureOwner::RenderedSelection(..)
            | CaptureOwner::TerminalThumb(..)
            | CaptureOwner::TerminalFootMark(..) => match position {
                Some(position) => self.release_chrome_capture(button, position),
                // A window the pointer has never been in has nowhere to land a release.
                None => Ok(false),
            },
        }
    }

    /// **The chrome's latched gestures, let go** — the release ladder that stood in
    /// `chrome_mouse_input`, now reached only for the capture that owns the release.
    fn release_chrome_capture(
        &mut self,
        button: MouseButton,
        position: PhysicalPosition<f64>,
    ) -> Result<bool> {
        let state = ElementState::Released;
        let traced_target = mouse_trace::is_on().then(|| self.chrome_target_at(position));
        // **A held scrubber or volume, first of everything** (route B slice
        // ②; §7.44 ②): the fraction it wrote on the way is already the
        // answer, so letting go only puts the dot away and restarts the
        // dwell that will take the bar off the glass.
        if let Some(surface) = self.take_video_bar_drag() {
            if let Some(seat) = self.window.video.get_mut(surface) {
                seat.release(Instant::now());
            }
            self.refresh_chrome();
            self.present_chrome_change()?;
            self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-video-bar-track state={state:?} button={button:?} target={traced_target:?}"));
            return Ok(true);
        }
        // A thumb is let go wherever the hand lets go of it — the offset it
        // wrote on the way is already the answer, so this only puts the
        // accent out. The body's bar first, the order everything else about
        // these two is in.
        if self.take_preview_body_drag().is_some() {
            self.note_preview_body_hover(Some(position))?;
            self.repaint_preview()?;
            self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-preview-body-thumb state={state:?} button={button:?} target={traced_target:?}"));
            return Ok(true);
        }
        // The terminal's own thumb, on the same terms: the offset it wrote
        // on the way is the answer, so letting go only settles which ink it
        // wears and restarts the clock that will take it off the glass.
        if let Some(drag) = self.take_terminal_thumb_drag() {
            self.wake_terminal_thumb(drag.seat);
            self.note_terminal_thumb_hover(Some(position))?;
            self.repaint_pane_change(drag.seat)?;
            self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-terminal-thumb state={state:?} button={button:?} target={traced_target:?}"));
            return Ok(true);
        }
        // And the foot's, on those same terms.
        if let Some(drag) = self.take_terminal_column_drag() {
            self.wake_terminal_column(drag.seat);
            self.note_terminal_column_hover(Some(position))?;
            self.repaint_pane_change(drag.seat)?;
            self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-terminal-column-thumb state={state:?} button={button:?} target={traced_target:?}"));
            return Ok(true);
        }
        if self.take_preview_block_drag().is_some() {
            self.note_preview_block_hover(Some(position))?;
            self.repaint_preview()?;
            self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-preview-block-thumb state={state:?} button={button:?} target={traced_target:?}"));
            return Ok(true);
        }
        // A carried picture is let go wherever the hand lets go of it, and
        // the pan it wrote on the way is already the answer — this only puts
        // the closed hand away.
        if self.take_preview_image_drag().is_some() {
            self.apply_pointer_cursor();
            self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-preview-image-drag state={state:?} button={button:?} target={traced_target:?}"));
            return Ok(true);
        }
        // A selection drawn across the edit surface ends wherever the button
        // comes up. The release is consumed because the press was: a gesture
        // belongs to the surface it began on, whatever it is let go over.
        if self.take_preview_selecting().is_some() {
            self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-preview-selecting state={state:?} button={button:?} target={traced_target:?}"));
            return Ok(true);
        }
        // **And one drawn across a rendered page**, on the same terms and
        // with one more decision to make: a press that never travelled was a
        // click, and a click is the link's or nobody's.
        if self.release_preview_text(position)? {
            self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-preview-text state={state:?} button={button:?} target={traced_target:?}"));
            return Ok(true);
        }
        // Ahead of the press: a gesture that has become a drag answers with
        // its drop, and the press that started it is no longer a click.
        if self.release_drag()? {
            // A press that travelled is not half of a double click, and a
            // pane drag starts on the very head the zoom gesture lives on
            // (J99's rule, at this window's other double-click surface).
            self.window.pane_head_clicks.interrupt();
            self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-drag-drop state={state:?} button={button:?} target={traced_target:?}"));
            return Ok(true);
        }
        if self.take_divider_drag().is_some() {
            self.window.seat_pointer.dragging = None;
            self.apply_pointer_cursor();
            if self.refresh_chrome() {
                self.present_chrome_change()?;
            }
            // The end of a drag is a meaningful change (§5.1): the ratio that
            // was being explored is now the ratio the user chose.
            self.mark_session_dirty(Instant::now());
            self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-divider-drag state={state:?} button={button:?} target={traced_target:?}"));
            return Ok(true);
        }
        let target = self.chrome_target_at(position);
        // A pane press that never travelled has nothing to settle: D40 moved
        // the focus on the way down and that is all a press on a head has
        // ever meant. Dropping it is the whole of letting go.
        let pane_press = self.take_pane_press();
        let held_pane = pane_press.is_some() | self.take_row_press().is_some();
        // **A double-click on a pane head zooms it, and lets it go again**
        // (§7.1.6l, 2026-08-24). This is the seat §7.1.6b′ kept warm.
        //
        // The gesture carried focus mode for one day and was withdrawn on
        // 2026-08-19, on an argument that named its rightful owner in the
        // same breath: a double-click on a title bar means "make this thing
        // bigger" everywhere in this operating system, focus mode made the
        // pane *smaller*, and single-pane zoom is the verb whose shape this
        // is. So the gesture was left empty rather than repurposed, and the
        // layout primitives it needed (`bt-layout`'s `LayoutMode::Focus` /
        // `solve_focused`) were kept unused for exactly this.
        //
        // **Both clicks must land on the same pane's head**, and the pairing
        // is keyed by the seat rather than by a pixel neighbourhood, which is
        // this window's rule at its three other double-click surfaces: the
        // head a re-solve moved between the two clicks is still the same
        // head. A release on anything else breaks the chain (J99), which is
        // the `else` below and not a list of interrupts sprinkled through
        // the press arms — one place decides, so one place can be wrong.
        let doubled = match (pane_press, target) {
            (Some(press), Some(seats::ChromeTarget::PaneHeader(seat))) if seat == press.seat => {
                self.window.pane_head_clicks.register(seat, Instant::now()) == TabClick::Double
            }
            _ => {
                self.window.pane_head_clicks.interrupt();
                false
            }
        };
        if doubled && let Some(press) = pane_press {
            self.toggle_pane_zoom(press.seat)?;
            self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-pane-head-zoom state={state:?} button={button:?} target={traced_target:?}"));
            return Ok(true);
        }
        if let Some(press) = self.take_tab_press() {
            self.release_tab_press(press, target)?;
            self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-tab-press state={state:?} button={button:?} target={traced_target:?}"));
            return Ok(true);
        }
        if held_pane {
            self.mouse_trace(|| format!("chrome_mouse_input taken=1 at=release-held-pane state={state:?} button={button:?} target={traced_target:?}"));
            return Ok(true);
        }
        Ok(false)
    }

    /// **End the gesture a press on a cell latched, wherever the button comes
    /// up** — and answer whether there was one (T-STRIP-HOVER-THROUGH,
    /// confirmation review 2026-10-04).
    ///
    /// Every [`MouseRoute`] is a press that a pane took, and its release is
    /// owed to that pane whatever is under the pointer now:
    ///
    /// * a formula mark's press ink comes off with the button (owner's ruling
    ///   2026-09-14 ②);
    /// * a selection drag is finished in the pane it began in — over the pane
    ///   next door, over the chrome, past the window's edge — because a release
    ///   left unanswered left the route latched and the next move went on
    ///   selecting;
    /// * a press handed to the program gets its release, at the cell the
    ///   pointer stands over clamped into that pane's body
    ///   ([`Self::forwarded_gesture_hit`]), in the encoding the press was sent
    ///   in, and the route comes off. A child given a press and never its
    ///   release holds a button down for ever.
    ///
    /// A button the mouse protocol has no name for ends nothing here, as it
    /// forwarded nothing.
    fn release_cell_route(&mut self, button: MouseButton) -> Result<bool> {
        match self.held_mouse_route().cloned() {
            None => Ok(false),
            Some(MouseRoute::MathBlock) => {
                self.drop_mouse_route();
                if self.window.math_tool_pressed.take().is_some() {
                    self.repaint_hovered_pane()?;
                }
                Ok(true)
            }
            // A selection is only ever begun by the left button, so only the
            // left button's release ends it; any other button's release goes on
            // down the ordinary road as an event of its own.
            Some(MouseRoute::Local(drag)) => {
                if button != MouseButton::Left {
                    return Ok(false);
                }
                // Its shell gone, the selection is let go with nothing done.
                if self.live_paste_target(drag.owner).is_none() {
                    self.drop_mouse_route();
                    return Ok(true);
                }
                self.finish_local_selection(*drag)?;
                Ok(true)
            }
            Some(MouseRoute::Forward {
                button: latched,
                owner,
                ..
            }) => {
                // **Only the button that started the gesture ends it.** Any
                // other button's release is not this gesture's: it is not
                // forwarded under this route, it does not clear it, and it
                // goes on down the ordinary road as an event of its own.
                if protocol_mouse_button(button) != Some(latched) {
                    return Ok(false);
                }
                // **To the shell the press was handed to, and to no other.** A
                // shell that is gone (or whose tab is no longer on top) is owed
                // nothing: the route comes off with no byte sent anywhere.
                if self.live_paste_target(owner).is_none() {
                    self.drop_mouse_route();
                    return Ok(true);
                }
                let seat = owner.seat;
                let Some(hit) = self.forwarded_gesture_hit(seat) else {
                    // A pane with no frame to name a cell in has no child to
                    // tell; the latch still has to come off.
                    self.drop_mouse_route();
                    return Ok(true);
                };
                let modes = self.leaf_terminal_modes(seat);
                let mut route = self.take_mouse_route();
                if let Some(bytes) = route_forwarded_mouse_button(
                    &mut route,
                    ElementState::Released,
                    latched,
                    hit,
                    modes,
                    self.window.modifiers,
                    PressedCellTarget::Ordinary,
                    owner,
                ) {
                    self.mouse_trace(|| {
                        format!(
                            "pane_release forwarded=1 bytes={} cell={},{}",
                            bytes.len(),
                            hit.row,
                            hit.column
                        )
                    });
                    self.answer_attention(seat, UserInputKind::MouseButton);
                    self.send_mouse_input_to(seat, &bytes, "forward mouse button event to PTY")?;
                }
                Ok(true)
            }
        }
    }
}
