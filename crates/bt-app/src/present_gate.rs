//! CPU-only bookkeeping for the picture actually committed to one window.

use bt_render::{RendererPresentState, SeatViewport};

pub(crate) fn pictures_match(
    previous: &bt_viewport::ViewportFrame,
    next: &bt_viewport::ViewportFrame,
) -> bool {
    std::ptr::eq(previous, next)
        || (crate::presentation_equivalent(previous, next)
            && previous.math_failures == next.math_failures)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SeatSignature {
    pub owner: (u64, u64),
    pub picture_revision: u64,
    pub viewport: SeatViewport,
    pub clip: SeatViewport,
    pub focused: bool,
    /// **The metrics the seat's picture is drawn at** (ticket 37).
    ///
    /// A pane's text size is part of what is on the glass: a size step that leaves the integer
    /// grid unchanged is still a new picture, and a gate that compared everything but the cell
    /// would call it the old one and present nothing. The frame's own `layout_key` carries the
    /// face size too, so [`pictures_match`] already tells the two apart; this names the metrics
    /// the renderer is handed beside the frame, so the record of what reached the glass is a
    /// record of both halves of the pair. An observation, like the rest of this record — the
    /// pane's rung has one owner and it is not here.
    pub metrics: bt_render::CellMetrics,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PresentSignature {
    pub renderer: RendererPresentState,
    // A hidden pre-clear cannot pay the first presentation after ShowWindow.
    pub window_visible: bool,
    pub seats: Vec<SeatSignature>,
    pub native_pages: Vec<NativePageSignature>,
    pub window_size: (u32, u32),
    pub dpi: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativePageSignature {
    pub owner: (u64, u64),
    pub state: (
        u64,
        crate::webhost::WebState,
        bool,
        crate::webhost::WebPresence,
    ),
}

#[derive(Clone, Copy)]
pub(crate) struct PresentConditions {
    pub visible: bool,
    pub resize_pending: bool,
    pub skirt_pending: bool,
}

#[derive(Default)]
pub(crate) struct PresentGate {
    last: Option<PresentSignature>,
    pub unchanged: u64,
}

impl PresentGate {
    // The caller compares the complete projection, including selection, hover
    // marks and viewport origin, with the retained picture of this same seat.
    // No cell or pixel hashing is needed on the retained path.
    pub fn picture_revision(&self, owner: (u64, u64), same_picture: bool) -> u64 {
        let previous = self
            .last
            .as_ref()
            .and_then(|last| last.seats.iter().find(|seat| seat.owner == owner));
        match previous {
            Some(seat) if same_picture => seat.picture_revision,
            Some(seat) => seat
                .picture_revision
                .checked_add(1)
                .expect("picture revision exhausted"),
            None => 1,
        }
    }

    pub fn unchanged(&self, next: &PresentSignature, conditions: PresentConditions) -> bool {
        conditions.visible
            && !conditions.resize_pending
            && !conditions.skirt_pending
            && self.last.as_ref() == Some(next)
    }

    pub fn presented(&mut self, signature: PresentSignature) {
        self.last = Some(signature);
    }

    // An attempted present can alter the surface even when no complete picture
    // is returned. A textless frame must also be retried, never called equal.
    pub fn invalidate(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_render::{CursorStyle, FrameSource, FrameTrigger, LatestFrameSlot};
    use bt_term::DualPlaneSession;
    use bt_viewport::{FrameViewportOrigin, SelectionSpan};
    use std::{num::NonZeroU32, time::Instant};

    fn visible() -> PresentConditions {
        PresentConditions {
            visible: true,
            resize_pending: false,
            skirt_pending: false,
        }
    }

    fn signature() -> PresentSignature {
        let viewport = SeatViewport::whole(800, 600);
        PresentSignature {
            renderer: RendererPresentState {
                retained_revision: 0,
                surface_generation: 0,
                size: (800, 600),
                scale_factor: 1.0_f64.to_bits(),
                font_revision: 1,
                theme_revision: 1,
                cursor_style: CursorStyle::Bar,
                cursor_blink_visible: true,
                window_focused: true,
                seat: viewport,
            },
            seats: vec![SeatSignature {
                owner: (1, 1),
                picture_revision: 1,
                viewport,
                clip: viewport,
                focused: true,
                metrics: crate::fixture_cell_metrics(1.0, 16.0),
            }],
            window_visible: true,
            native_pages: Vec::new(),
            window_size: (800, 600),
            dpi: 1.0_f64.to_bits(),
        }
    }

    #[test]
    fn equal_picture_needs_no_second_pass_but_resize_and_skirt_always_do() {
        let mut gate = PresentGate::default();
        let picture = signature();
        assert!(
            !gate.unchanged(&picture, visible()),
            "first frame must draw"
        );
        gate.presented(picture.clone());
        assert!(gate.unchanged(&picture, visible()));
        for conditions in [
            PresentConditions {
                resize_pending: true,
                ..visible()
            },
            PresentConditions {
                skirt_pending: true,
                ..visible()
            },
            PresentConditions {
                visible: false,
                ..visible()
            },
        ] {
            assert!(!gate.unchanged(&picture, conditions));
        }
    }

    #[test]
    fn a_hidden_preclear_does_not_replace_the_first_visible_present() {
        let mut picture = signature();
        picture.window_visible = false;
        let mut gate = PresentGate::default();
        gate.presented(picture);
        assert!(!gate.unchanged(&signature(), visible()));
    }

    #[test]
    fn every_sampled_drawing_input_invalidates_the_signature() {
        let picture = signature();
        let mut gate = PresentGate::default();
        gate.presented(picture.clone());
        let changes: &[fn(&mut PresentSignature)] = &[
            |p| p.renderer.retained_revision += 1,
            |p| p.renderer.surface_generation += 1,
            |p| p.renderer.size.0 += 1,
            |p| p.renderer.scale_factor = 2.0_f64.to_bits(),
            |p| p.renderer.font_revision += 1,
            |p| p.renderer.theme_revision += 1,
            |p| p.renderer.cursor_style = CursorStyle::Block,
            |p| p.renderer.cursor_blink_visible = false,
            |p| p.renderer.window_focused = false,
            |p| p.renderer.seat.x += 1,
            |p| p.seats[0].owner.0 += 1,
            |p| p.seats[0].picture_revision += 1,
            |p| p.seats[0].viewport.x += 1,
            |p| p.seats[0].clip.width -= 1,
            |p| p.seats[0].focused = false,
            |p| p.seats.clear(),
            |p| p.window_visible = false,
            |p| p.window_size.1 += 1,
            |p| p.dpi = 2.0_f64.to_bits(),
        ];
        for (index, change) in changes.iter().enumerate() {
            let mut next = picture.clone();
            change(&mut next);
            assert!(!gate.unchanged(&next, visible()), "drawing input {index}");
        }
    }

    #[test]
    fn a_native_page_arriving_below_the_same_hole_still_needs_a_commit() {
        use crate::webhost::{WebPresence, WebState};
        let mut picture = signature();
        picture.native_pages.push(NativePageSignature {
            owner: (1, 2),
            state: (1, WebState::Uninitialized, false, WebPresence::Hidden),
        });
        let mut gate = PresentGate::default();
        gate.presented(picture.clone());
        picture.native_pages[0].state.2 = true;
        assert!(!gate.unchanged(&picture, visible()));
    }

    #[test]
    fn a_tab_without_a_shell_can_skip_only_its_unchanged_chrome() {
        let mut picture = signature();
        picture.seats.clear();
        let mut gate = PresentGate::default();
        assert!(!gate.unchanged(&picture, visible()));
        gate.presented(picture.clone());
        assert!(gate.unchanged(&picture, visible()));
        picture.renderer.retained_revision += 1;
        assert!(!gate.unchanged(&picture, visible()));
    }

    #[test]
    fn each_seat_keeps_its_own_picture_revision() {
        let mut picture = signature();
        let mut sibling = picture.seats[0].clone();
        sibling.owner.1 = 2;
        sibling.picture_revision = 9;
        sibling.focused = false;
        picture.seats.push(sibling);
        let mut gate = PresentGate::default();
        gate.presented(picture.clone());
        assert_eq!(gate.picture_revision((1, 1), true), 1);
        assert_eq!(gate.picture_revision((1, 2), true), 9);
        picture.seats[1].picture_revision = gate.picture_revision((1, 2), false);
        assert_eq!(picture.seats[1].picture_revision, 10);
        assert!(!gate.unchanged(&picture, visible()));
    }

    #[test]
    fn failed_or_textless_present_cannot_authorize_a_skip() {
        let picture = signature();
        let mut gate = PresentGate::default();
        gate.presented(picture.clone());
        gate.invalidate();
        assert!(!gate.unchanged(&picture, visible()));
        // Merely attempting another frame does not restore the permission.
        assert!(!gate.unchanged(&picture, visible()));
        gate.presented(picture.clone());
        assert!(gate.unchanged(&picture, visible()));
    }

    fn frame() -> bt_viewport::ViewportFrame {
        let session =
            DualPlaneSession::new(NonZeroU32::new(8).unwrap(), NonZeroU32::new(2).unwrap());
        let mut projection = session.new_projection(session.layout_key());
        session.viewport_frame(&mut projection).unwrap()
    }

    #[test]
    fn selection_hover_caret_notice_and_viewport_are_picture_changes() {
        let before = frame();
        let changes: &[fn(&mut bt_viewport::ViewportFrame)] = &[
            |p| {
                p.selection_spans.push(SelectionSpan {
                    row: 0,
                    start_column: 0,
                    end_column: 1,
                })
            },
            |p| {
                p.underline_cells(&[0], true);
            },
            |p| p.cursor.visible = !p.cursor.visible,
            |p| p.status_text = Some("notice".into()),
            |p| p.viewport_origin = FrameViewportOrigin::LiveOverflow { rows_below: 1 },
            |p| p.presentation_offset_subpixels = 1,
            |p| p.scroll_offset_rows = 1,
        ];
        assert!(pictures_match(&before, &before.clone()));
        for (index, change) in changes.iter().enumerate() {
            let mut after = before.clone();
            change(&mut after);
            assert!(!pictures_match(&before, &after), "picture input {index}");
        }
        let mut generation_only = before.clone();
        generation_only.view_generation.0 += 1;
        assert!(pictures_match(&before, &generation_only));
    }

    #[test]
    fn two_publications_only_offer_the_latest_picture_to_the_gate() {
        let mut slot = LatestFrameSlot::default();
        let first = frame();
        let mut latest = first.clone();
        latest.status_text = Some("latest".into());
        let trigger = FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        };
        slot.publish(first, trigger).unwrap();
        slot.publish(latest.clone(), trigger).unwrap();
        assert_eq!(slot.overwrites(), 1);
        assert_eq!(slot.take().unwrap().0, latest);
        assert!(
            slot.take().is_none(),
            "no superseded frame can start a GPU pass"
        );
    }
}
