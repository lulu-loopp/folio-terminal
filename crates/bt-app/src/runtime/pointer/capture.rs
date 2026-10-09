//! **The capture** (`docs/plans/design/pointer-capture-2026-10-09.md` §2.2):
//! one record per latched gesture, in one application slot.
//!
//! In cut 2 the slot is a **mirror**: every site that sets or clears one of
//! the legacy latch fields (§1.2) writes the same fact here beside it, through
//! [`Runtime::capture_mirror_begin`] and [`Runtime::capture_mirror_end`]. It is
//! a list rather than one record because today's latches can overlap — a stale
//! latch beside a new one, a settings drag beside anything, a formula route
//! over a forwarded press — and the mirror records the overlap instead of
//! resolving it. Nothing reads it, so no behaviour changes.

use crate::Runtime;
use crate::TabId;
use winit::event::{ElementState, MouseButton};
use winit::window::WindowId;

use super::PointerHit;

/// **Which gesture holds the pointer** — one variant per row of §1.2. A
/// gesture whose latch lives on a tab names the tab.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CaptureOwner {
    /// C1, a divider drag.
    Divider,
    /// C2, a press on a tab, not yet a click or a drag.
    TabPress,
    /// C3, a press on a pane head.
    PanePress,
    /// C4, a press on a files row.
    RowPress,
    /// C5, a tab, pane or row in the hand.
    Drag,
    /// C6, a press on a peek's head.
    FloatHeadPress,
    /// C7, a floating window being moved or resized.
    FloatDrag,
    /// C8, a press on the glance card's head.
    GlanceHeadPress,
    /// C9, the glance card's thumb.
    GlanceThumb,
    /// C10, a video's scrubber or volume.
    VideoBar,
    /// C11, a preview body's thumb.
    PreviewBodyThumb(TabId),
    /// C12, a wide block's thumb.
    BlockThumb(TabId),
    /// C13, a picture being panned.
    PicturePan(TabId),
    /// C14, a selection on the edit surface.
    EditSelection(TabId),
    /// C15, a selection on a rendered page.
    RenderedSelection(TabId),
    /// C16, a terminal's thumb.
    TerminalThumb(TabId),
    /// C17, a terminal's foot mark.
    TerminalFootMark(TabId),
    /// C18, a terminal selection.
    TerminalSelection,
    /// C19, a press handed to the program in a pane.
    ForwardedPress,
    /// C20, a press on a formula block.
    FormulaBlock,
    /// C22, the settings sheet's slider.
    SettingsSlider,
    /// C22, the settings sheet's menu bar.
    SettingsMenuBar,
}

/// **One latched gesture** (§2.2): the window whose press latched it, which
/// gesture it is, the button, and what the router said was under the press.
#[derive(Clone, Debug)]
pub(crate) struct PointerCapture {
    pub(crate) window: WindowId,
    pub(crate) owner: CaptureOwner,
    #[expect(
        dead_code,
        reason = "T-POINTER-CAPTURE until 2026-12-31: the mirror is read by nothing until cut 3 makes it the slot"
    )]
    pub(crate) button: MouseButton,
    /// The router's answer for the event that latched it.
    #[expect(
        dead_code,
        reason = "T-POINTER-CAPTURE until 2026-12-31: the mirror is read by nothing until cut 3 makes it the slot"
    )]
    pub(crate) started: Option<PointerHit>,
}

/// **The application's capture slot, as a mirror of the legacy latches**
/// (cut 2): every live latch of every window, in the order they were taken.
#[derive(Default)]
pub(crate) struct CaptureMirror {
    held: Vec<PointerCapture>,
}

impl CaptureMirror {
    /// A latch was set. Setting one that is already held keeps the record it
    /// has: a latch field holds one gesture, and writing it again is the same
    /// gesture going on.
    pub(crate) fn begin(&mut self, capture: PointerCapture) {
        if !self.holds(capture.window, capture.owner) {
            self.held.push(capture);
        }
    }

    /// A latch was cleared (or was written empty while already empty).
    pub(crate) fn end(&mut self, window: WindowId, owner: CaptureOwner) {
        self.held
            .retain(|capture| !(capture.window == window && capture.owner == owner));
    }

    fn holds(&self, window: WindowId, owner: CaptureOwner) -> bool {
        self.held
            .iter()
            .any(|capture| capture.window == window && capture.owner == owner)
    }

    /// Every live latch, oldest first.
    #[cfg(test)]
    pub(crate) fn held(&self) -> &[PointerCapture] {
        &self.held
    }

    /// **How many latches stand beside another** — what cut 3's one slot
    /// cannot represent, and so must bring to zero.
    #[cfg(test)]
    pub(crate) fn overlaps(&self) -> usize {
        self.held.len().saturating_sub(1)
    }
}

impl Runtime<'_> {
    /// **Mirror a latch being set** (cut 2): written beside every site that
    /// sets one of the legacy latch fields on the left button's road.
    pub(crate) fn capture_mirror_begin(&mut self, owner: CaptureOwner) {
        self.capture_mirror_begin_pressed(owner, MouseButton::Left);
    }

    /// **Mirror a latch being set by the button that was pressed** (cut 2): for
    /// the latches a press of any button can take.
    pub(crate) fn capture_mirror_begin_pressed(
        &mut self,
        owner: CaptureOwner,
        button: MouseButton,
    ) {
        let started = self
            .window
            .pointer_memo
            .answer
            .as_ref()
            .and_then(|(_, hit)| hit.clone());
        let window = self.window.window.id();
        self.app.pointer_capture.begin(PointerCapture {
            window,
            owner,
            button,
            started,
        });
    }

    /// **Mirror a press handed to a program, or its release** (cut 2): the
    /// forwarded route is set by the press and cleared by the release, in one
    /// call that answers both.
    pub(crate) fn capture_mirror_forwarded(&mut self, state: ElementState, button: MouseButton) {
        match state {
            ElementState::Pressed => {
                self.capture_mirror_begin_pressed(CaptureOwner::ForwardedPress, button);
            }
            ElementState::Released => self.capture_mirror_end(CaptureOwner::ForwardedPress),
        }
    }

    /// **Mirror the pointer route being cleared** (cut 2): the one field holds
    /// a terminal selection, a forwarded press or a formula press, and a write of
    /// `None` ends whichever of them the mirror holds.
    pub(crate) fn capture_mirror_end_route(&mut self) {
        for owner in [
            CaptureOwner::TerminalSelection,
            CaptureOwner::ForwardedPress,
            CaptureOwner::FormulaBlock,
        ] {
            self.capture_mirror_end(owner);
        }
    }

    /// **Mirror a latch being cleared** (cut 2): written beside every site that
    /// clears one.
    pub(crate) fn capture_mirror_end(&mut self, owner: CaptureOwner) {
        let window = self.window.window.id();
        self.app.pointer_capture.end(window, owner);
    }
}
