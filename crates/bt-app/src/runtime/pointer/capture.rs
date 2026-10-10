//! **The capture** (`docs/plans/design/pointer-capture-2026-10-09.md` §2.2):
//! one record per latched gesture, in one application slot.
//!
//! `App::pointer_capture` holds at most one [`PointerCapture`]: the window
//! whose press latched it, the gesture with its own payload ([`CaptureOwner`],
//! one variant per row of §1.2, a tab's own gesture naming its tab), the
//! button, and the router's answer at the press. Two latches at once, two
//! windows at once and a latch nobody owns are not representable.
//!
//! Every gesture is read and written through the accessors below, so a
//! window reads only its own capture and a tab's gesture is read only on that
//! tab. The release of the capture's button goes to its owner before any
//! layer is asked ([`Runtime::release_capture`], R-6), and a press of the held
//! button proves the capture stale and cancels it first
//! ([`press_against_the_slot`], R-5 rule 2).

use crate::{
    DividerDrag, Drag, FilePeekPress, FloatDrag, FloatHeadPress, ImageDrag, MouseRoute, PanePress,
    PreviewBlockDrag, PreviewBodyDrag, PreviewSurface, PreviewTextDrag, RowPress, Runtime, TabId,
    TabPress, TerminalColumnDrag, TerminalThumbDrag, settings,
};
use winit::event::MouseButton;
use winit::window::WindowId;

use super::PointerHit;

/// **Which gesture holds the pointer, and its payload** — one variant per row
/// of §1.2. A gesture whose surface belongs to a tab names the tab, so it is
/// read only while that tab is in front.
#[derive(Clone)]
pub(crate) enum CaptureOwner {
    /// C1, a divider drag.
    Divider(DividerDrag),
    /// C2, a press on a tab, not yet a click or a drag.
    TabPress(TabPress),
    /// C3, a press on a pane head; the tab it was pressed in beside it.
    PanePress(TabId, PanePress),
    /// C4, a press on a files row.
    RowPress(RowPress),
    /// C5, a tab, pane or row in the hand.
    Drag(Box<Drag>),
    /// C6, a press on a peek's head.
    FloatHeadPress(FloatHeadPress),
    /// C7, a floating window being moved or resized.
    FloatDrag(FloatDrag),
    /// C8, a press on the glance card's head.
    GlanceHeadPress(FilePeekPress),
    /// C9, the glance card's thumb, and where in it the hand took hold.
    GlanceThumb(f32),
    /// C10, a video's scrubber or volume.
    VideoBar(PreviewSurface),
    /// C11, a preview body's thumb.
    PreviewBodyThumb(TabId, PreviewBodyDrag),
    /// C12, a wide block's thumb.
    BlockThumb(TabId, PreviewBlockDrag),
    /// C13, a picture being panned.
    PicturePan(TabId, ImageDrag),
    /// C14, a selection on the edit surface.
    EditSelection(TabId, PreviewSurface),
    /// C15, a selection on a rendered page.
    RenderedSelection(TabId, PreviewTextDrag),
    /// C16, a terminal's thumb.
    TerminalThumb(TabId, TerminalThumbDrag),
    /// C17, a terminal's foot mark.
    TerminalFootMark(TabId, TerminalColumnDrag),
    /// C18–C20, a press on a terminal pane's cells: a selection, a press
    /// handed to the program, or a formula block.
    Route(MouseRoute),
    /// C22, the settings sheet's slider.
    SettingsSlider(settings::SettingsRow),
    /// C22, the settings sheet's menu bar, and where on it the hand took hold.
    SettingsMenuBar(f32),
}

impl CaptureOwner {
    /// **Which release ends this gesture** (until cut 4 rules on the other
    /// button): the glance card's, a floating window's, the settings sheet's
    /// and a formula block's end on the release of any button, as they always
    /// have; every other one on the release of the button that latched it.
    pub(crate) const fn ends_on_any_release(&self) -> bool {
        match self {
            Self::GlanceHeadPress(_)
            | Self::GlanceThumb(_)
            | Self::FloatHeadPress(_)
            | Self::FloatDrag(_)
            | Self::SettingsSlider(_)
            | Self::SettingsMenuBar(_)
            | Self::Route(MouseRoute::MathBlock) => true,
            Self::Divider(_)
            | Self::TabPress(_)
            | Self::PanePress(..)
            | Self::RowPress(_)
            | Self::Drag(_)
            | Self::VideoBar(_)
            | Self::PreviewBodyThumb(..)
            | Self::BlockThumb(..)
            | Self::PicturePan(..)
            | Self::EditSelection(..)
            | Self::RenderedSelection(..)
            | Self::TerminalThumb(..)
            | Self::TerminalFootMark(..)
            | Self::Route(MouseRoute::Local(_) | MouseRoute::Forward { .. }) => false,
        }
    }
}

/// **One latched gesture** (§2.2).
#[derive(Clone)]
pub(crate) struct PointerCapture {
    /// The window whose press latched it.
    pub(crate) window: WindowId,
    pub(crate) owner: CaptureOwner,
    /// The button that latched it, after the macOS secondary-click translation.
    pub(crate) button: MouseButton,
    /// The router's answer for the event that latched it.
    #[expect(
        dead_code,
        reason = "T-POINTER-CAPTURE until 2026-12-31: the press road reads it once it dispatches on the router (cut 8a)"
    )]
    pub(crate) started: Option<PointerHit>,
}

/// **What a press does with the slot** (R-5, cut 3): routed, or routed after
/// the held capture is cancelled. A press of another button keeps today's
/// routing until cut 4 rules on it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PressVerdict {
    Route,
    /// The held capture's own button went down again: its release was lost,
    /// so the capture is stale. It is cancelled, then the press is routed.
    CancelThenRoute,
}

/// **The press precedence** (R-5): compared with the slot by button, never by
/// window, so a press arriving in another window than the capture's answers
/// exactly as one arriving in it.
pub(crate) fn press_against_the_slot(
    slot: Option<&PointerCapture>,
    button: MouseButton,
) -> PressVerdict {
    match slot {
        Some(capture) if capture.button == button => PressVerdict::CancelThenRoute,
        Some(_) | None => PressVerdict::Route,
    }
}

/// **Whether a release ends the held capture** (R-6): the release of the
/// capture's own button, or of any button for the gestures that have always
/// ended on any ([`CaptureOwner::ends_on_any_release`]). The window plays no
/// part, and neither does what is under the pointer.
pub(crate) fn release_ends_the_capture(capture: &PointerCapture, button: MouseButton) -> bool {
    capture.button == button || capture.owner.ends_on_any_release()
}

/// **The doors of one gesture** — read, read to change in place, take, latch,
/// drop — each answering only for this window's capture, and, for a gesture
/// that belongs to a tab, only while that tab is in front. Each gesture names
/// the doors its road uses.
macro_rules! gesture_doors {
    ($scope:ident $variant:ident $ty:ty { $($door:ident $name:ident),* $(,)? }) => {
        impl Runtime<'_> {
            $(gesture_doors!(@$scope $door $name $variant $ty);)*
        }
    };
    (@window held $name:ident $variant:ident $ty:ty) => {
        pub(crate) fn $name(&self) -> Option<&$ty> {
            match &self.own_capture()?.owner {
                CaptureOwner::$variant(value) => Some(value),
                _ => None,
            }
        }
    };
    (@window held_mut $name:ident $variant:ident $ty:ty) => {
        pub(crate) fn $name(&mut self) -> Option<&mut $ty> {
            match &mut self.own_capture_mut()?.owner {
                CaptureOwner::$variant(value) => Some(value),
                _ => None,
            }
        }
    };
    (@window take $name:ident $variant:ident $ty:ty) => {
        pub(crate) fn $name(&mut self) -> Option<$ty> {
            let window = self.window.window.id();
            let capture = self.app.pointer_capture.take_if(|capture| {
                capture.window == window && matches!(capture.owner, CaptureOwner::$variant(..))
            })?;
            match capture.owner {
                CaptureOwner::$variant(value) => Some(value),
                _ => None,
            }
        }
    };
    (@window latch $name:ident $variant:ident $ty:ty) => {
        pub(crate) fn $name(&mut self, value: $ty) {
            self.latch_capture(CaptureOwner::$variant(value), MouseButton::Left);
        }
    };
    (@window drop $name:ident $variant:ident $ty:ty) => {
        pub(crate) fn $name(&mut self) {
            let window = self.window.window.id();
            let _ = self.app.pointer_capture.take_if(|capture| {
                capture.window == window && matches!(capture.owner, CaptureOwner::$variant(..))
            });
        }
    };
    (@tab held $name:ident $variant:ident $ty:ty) => {
        pub(crate) fn $name(&self) -> Option<&$ty> {
            match &self.own_capture()?.owner {
                CaptureOwner::$variant(tab, value) if *tab == self.id => Some(value),
                _ => None,
            }
        }
    };
    (@tab held_mut $name:ident $variant:ident $ty:ty) => {
        pub(crate) fn $name(&mut self) -> Option<&mut $ty> {
            let id = self.id;
            match &mut self.own_capture_mut()?.owner {
                CaptureOwner::$variant(tab, value) if *tab == id => Some(value),
                _ => None,
            }
        }
    };
    (@tab take $name:ident $variant:ident $ty:ty) => {
        pub(crate) fn $name(&mut self) -> Option<$ty> {
            let (window, id) = (self.window.window.id(), self.id);
            let capture = self.app.pointer_capture.take_if(|capture| {
                capture.window == window
                    && matches!(capture.owner, CaptureOwner::$variant(tab, _) if tab == id)
            })?;
            match capture.owner {
                CaptureOwner::$variant(_, value) => Some(value),
                _ => None,
            }
        }
    };
    (@tab latch $name:ident $variant:ident $ty:ty) => {
        pub(crate) fn $name(&mut self, value: $ty) {
            let tab = self.id;
            self.latch_capture(CaptureOwner::$variant(tab, value), MouseButton::Left);
        }
    };
    (@tab drop $name:ident $variant:ident $ty:ty) => {
        pub(crate) fn $name(&mut self) {
            let (window, id) = (self.window.window.id(), self.id);
            let _ = self.app.pointer_capture.take_if(|capture| {
                capture.window == window
                    && matches!(capture.owner, CaptureOwner::$variant(tab, _) if tab == id)
            });
        }
    };
}

gesture_doors!(window Divider DividerDrag {
    held held_divider_drag, take take_divider_drag, latch latch_divider_drag, drop drop_divider_drag,
});
gesture_doors!(window TabPress TabPress {
    held held_tab_press, held_mut held_tab_press_mut, take take_tab_press,
    latch latch_tab_press, drop drop_tab_press,
});
gesture_doors!(window RowPress RowPress {
    held held_row_press, held_mut held_row_press_mut, take take_row_press,
    latch latch_row_press, drop drop_row_press,
});
gesture_doors!(window FloatHeadPress FloatHeadPress {
    held held_float_head_press, held_mut held_float_head_press_mut,
    latch latch_float_head_press, drop drop_float_head_press,
});
gesture_doors!(window FloatDrag FloatDrag {
    held held_float_drag, latch latch_float_drag, drop drop_float_drag,
});
gesture_doors!(window GlanceHeadPress FilePeekPress {
    held_mut held_file_peek_press_mut, take take_file_peek_press,
    latch latch_file_peek_press, drop drop_file_peek_press,
});
gesture_doors!(window GlanceThumb f32 {
    held held_glance_thumb, take take_glance_thumb, drop drop_glance_thumb,
});
gesture_doors!(window VideoBar PreviewSurface {
    held held_video_bar_drag, take take_video_bar_drag, latch latch_video_bar_drag,
});
gesture_doors!(window SettingsSlider settings::SettingsRow {
    held held_settings_slider_drag, latch latch_settings_slider_drag,
});
gesture_doors!(window SettingsMenuBar f32 {
    held held_settings_menu_bar_drag, latch latch_settings_menu_bar_drag,
});
gesture_doors!(tab PanePress PanePress {
    held held_pane_press, held_mut held_pane_press_mut, take take_pane_press,
    latch latch_pane_press, drop drop_pane_press,
});
gesture_doors!(tab PreviewBodyThumb PreviewBodyDrag {
    held held_preview_body_drag, take take_preview_body_drag,
    latch latch_preview_body_drag, drop drop_preview_body_drag,
});
gesture_doors!(tab BlockThumb PreviewBlockDrag {
    held held_preview_block_drag, take take_preview_block_drag,
    latch latch_preview_block_drag, drop drop_preview_block_drag,
});
gesture_doors!(tab PicturePan ImageDrag {
    held held_preview_image_drag, take take_preview_image_drag, latch latch_preview_image_drag,
});
gesture_doors!(tab EditSelection PreviewSurface {
    held held_preview_selecting, take take_preview_selecting,
    latch latch_preview_selecting, drop drop_preview_selecting,
});
gesture_doors!(tab RenderedSelection PreviewTextDrag {
    held held_preview_text_drag, held_mut held_preview_text_drag_mut,
    take take_preview_text_drag, latch latch_preview_text_drag, drop drop_preview_text_drag,
});
gesture_doors!(tab TerminalThumb TerminalThumbDrag {
    held held_terminal_thumb_drag, take take_terminal_thumb_drag, latch latch_terminal_thumb_drag,
});
gesture_doors!(tab TerminalFootMark TerminalColumnDrag {
    held held_terminal_column_drag, take take_terminal_column_drag,
    latch latch_terminal_column_drag,
});

impl Runtime<'_> {
    /// **This window's capture**, if the slot holds one.
    pub(crate) fn own_capture(&self) -> Option<&PointerCapture> {
        let window = self.window.window.id();
        self.app
            .pointer_capture
            .as_ref()
            .filter(|capture| capture.window == window)
    }

    fn own_capture_mut(&mut self) -> Option<&mut PointerCapture> {
        let window = self.window.window.id();
        self.app
            .pointer_capture
            .as_mut()
            .filter(|capture| capture.window == window)
    }

    /// **Whether a gesture of this window holds the pointer** — any of them,
    /// by the one slot ([`crate::Runtime::a_gesture_holds_the_pointer`]).
    pub(crate) fn a_capture_holds_the_pointer(&self) -> bool {
        self.own_capture().is_some()
    }

    /// **Latch a gesture**: it takes the application's one slot. A slot that
    /// still held another gesture held one whose release was lost (R-5); the
    /// press door has already cancelled a stale capture of the same button,
    /// and what a press of another button replaces is ruled in cut 4.
    pub(crate) fn latch_capture(&mut self, owner: CaptureOwner, button: MouseButton) {
        let started = self
            .window
            .pointer_memo
            .answer
            .as_ref()
            .and_then(|(_, hit)| hit.clone());
        let window = self.window.window.id();
        self.app.pointer_capture = Some(PointerCapture {
            window,
            owner,
            button,
            started,
        });
    }

    /// The drag in the hand.
    pub(crate) fn held_drag(&self) -> Option<&Drag> {
        match &self.own_capture()?.owner {
            CaptureOwner::Drag(drag) => Some(drag),
            _ => None,
        }
    }

    pub(crate) fn held_drag_mut(&mut self) -> Option<&mut Drag> {
        match &mut self.own_capture_mut()?.owner {
            CaptureOwner::Drag(drag) => Some(drag),
            _ => None,
        }
    }

    pub(crate) fn take_drag(&mut self) -> Option<Drag> {
        self.held_drag()?;
        match self.app.pointer_capture.take()?.owner {
            CaptureOwner::Drag(drag) => Some(*drag),
            _ => None,
        }
    }

    pub(crate) fn latch_drag(&mut self, drag: Drag) {
        self.latch_capture(CaptureOwner::Drag(Box::new(drag)), MouseButton::Left);
    }

    pub(crate) fn drop_drag(&mut self) {
        let _ = self.take_drag();
    }

    /// The route a press on a terminal pane's cells latched.
    pub(crate) fn held_mouse_route(&self) -> Option<&MouseRoute> {
        match &self.own_capture()?.owner {
            CaptureOwner::Route(route) => Some(route),
            _ => None,
        }
    }

    pub(crate) fn take_mouse_route(&mut self) -> Option<MouseRoute> {
        self.held_mouse_route()?;
        match self.app.pointer_capture.take()?.owner {
            CaptureOwner::Route(route) => Some(route),
            _ => None,
        }
    }

    /// Latch a route, recording the button that pressed it.
    pub(crate) fn latch_mouse_route(&mut self, route: MouseRoute, button: MouseButton) {
        self.latch_capture(CaptureOwner::Route(route), button);
    }

    /// Latch a terminal selection, which only the left button begins.
    pub(crate) fn latch_selection_route(&mut self, route: MouseRoute) {
        self.latch_mouse_route(route, MouseButton::Left);
    }

    pub(crate) fn drop_mouse_route(&mut self) {
        let _ = self.take_mouse_route();
    }
}
