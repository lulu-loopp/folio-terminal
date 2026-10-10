//! **The capture** (`docs/plans/design/pointer-capture-2026-10-09.md` §2.2):
//! one record per latched gesture, in one application slot.
//!
//! `App::pointer_capture` is the authority for the current gesture. Until cut
//! 4 decides what another button does, `App::pointer_capture_overlaps` keeps
//! the independent legacy-field overlaps cut 3 is not allowed to change.
//!
//! Every gesture is read and written through the accessors below, so a
//! window reads only its own capture and a tab's gesture is read only on that
//! tab. The release of the capture's button goes to its owner before any
//! layer is asked ([`Runtime::release_capture`], R-6), and a press of the held
//! button proves the capture stale and cancels it first
//! ([`press_against_the_slot`], R-5 rule 2).

use crate::{
    App, DividerDrag, Drag, FilePeekPress, FloatDrag, FloatHeadPress, ImageDrag, MouseRoute,
    PanePress, PreviewBlockDrag, PreviewBodyDrag, PreviewSurface, PreviewTextDrag, RowPress,
    Runtime, TabId, TabPress, TerminalColumnDrag, TerminalThumbDrag, settings,
};
use winit::event::{ElementState, MouseButton};
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

    /// **Where this legacy field's release stood before cut 3.** Several
    /// independent fields could be live together, and the first matching arm
    /// ended the release. The compatibility list keeps that order until cut 4
    /// replaces the other-button rule.
    const fn legacy_release_precedence(&self) -> u8 {
        match self {
            Self::Route(_) => 0,
            Self::SettingsSlider(_) => 1,
            Self::SettingsMenuBar(_) => 2,
            Self::GlanceThumb(_) => 3,
            Self::GlanceHeadPress(_) => 4,
            Self::FloatHeadPress(_) => 5,
            Self::FloatDrag(_) => 6,
            Self::VideoBar(_) => 7,
            Self::PreviewBodyThumb(..) => 8,
            Self::TerminalThumb(..) => 9,
            Self::TerminalFootMark(..) => 10,
            Self::BlockThumb(..) => 11,
            Self::PicturePan(..) => 12,
            Self::EditSelection(..) => 13,
            Self::RenderedSelection(..) => 14,
            Self::Drag(_) => 15,
            Self::Divider(_) => 16,
            Self::PanePress(..) => 17,
            Self::RowPress(_) => 18,
            Self::TabPress(_) => 19,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CaptureIndex {
    Primary,
    Overlap(usize),
}

fn same_legacy_field(left: &PointerCapture, right: &PointerCapture) -> bool {
    left.window == right.window
        && std::mem::discriminant(&left.owner) == std::mem::discriminant(&right.owner)
}

pub(crate) fn capture_index_for_button_event(
    primary: Option<&PointerCapture>,
    overlaps: &[PointerCapture],
    arrived_in: WindowId,
    state: ElementState,
    button: MouseButton,
) -> Option<CaptureIndex> {
    let matches = |capture: &PointerCapture| match state {
        ElementState::Pressed => capture.button == button,
        ElementState::Released => release_ends_the_capture(capture, button),
    };
    if state == ElementState::Pressed {
        if primary.is_some_and(matches) {
            return Some(CaptureIndex::Primary);
        }
        return overlaps
            .iter()
            .rposition(matches)
            .map(CaptureIndex::Overlap);
    }
    primary
        .into_iter()
        .map(|capture| (CaptureIndex::Primary, capture))
        .chain(
            overlaps
                .iter()
                .enumerate()
                .map(|(index, capture)| (CaptureIndex::Overlap(index), capture)),
        )
        .filter(|(_, capture)| matches(capture))
        .min_by_key(|(_, capture)| {
            (
                capture.owner.legacy_release_precedence(),
                capture.window != arrived_in,
            )
        })
        .map(|(index, _)| index)
}

pub(crate) fn latch_in(
    primary: &mut Option<PointerCapture>,
    overlaps: &mut Vec<PointerCapture>,
    capture: PointerCapture,
) {
    overlaps.retain(|held| !same_legacy_field(held, &capture));
    match primary.take() {
        Some(held) if held.button != capture.button && !same_legacy_field(&held, &capture) => {
            overlaps.retain(|older| !same_legacy_field(older, &held));
            overlaps.push(held);
        }
        Some(_) | None => {}
    }
    *primary = Some(capture);
}

pub(crate) fn cancel_then_route<S, T, E>(
    state: &mut S,
    cancel_owner: impl FnOnce(&mut S) -> Result<(), E>,
    route_press: impl FnOnce(&mut S) -> Result<T, E>,
) -> Result<T, E> {
    cancel_owner(state)?;
    route_press(state)
}

impl App {
    pub(crate) fn capture_index_where(
        &self,
        mut predicate: impl FnMut(&PointerCapture) -> bool,
    ) -> Option<CaptureIndex> {
        if self.pointer_capture.as_ref().is_some_and(&mut predicate) {
            return Some(CaptureIndex::Primary);
        }
        self.pointer_capture_overlaps
            .iter()
            .rposition(predicate)
            .map(CaptureIndex::Overlap)
    }

    pub(crate) fn focus_capture(&mut self, index: CaptureIndex) {
        let CaptureIndex::Overlap(index) = index else {
            return;
        };
        let Some(primary) = self.pointer_capture.as_mut() else {
            self.pointer_capture = Some(self.pointer_capture_overlaps.remove(index));
            return;
        };
        std::mem::swap(primary, &mut self.pointer_capture_overlaps[index]);
    }

    pub(crate) fn take_capture_at(&mut self, index: CaptureIndex) -> Option<PointerCapture> {
        match index {
            CaptureIndex::Primary => {
                let taken = self.pointer_capture.take();
                self.pointer_capture = self.pointer_capture_overlaps.pop();
                taken
            }
            CaptureIndex::Overlap(index) => (index < self.pointer_capture_overlaps.len())
                .then(|| self.pointer_capture_overlaps.remove(index)),
        }
    }

    fn take_capture_where(
        &mut self,
        predicate: impl FnMut(&PointerCapture) -> bool,
    ) -> Option<PointerCapture> {
        let index = self.capture_index_where(predicate)?;
        self.take_capture_at(index)
    }

    fn latch_capture(&mut self, capture: PointerCapture) {
        latch_in(
            &mut self.pointer_capture,
            &mut self.pointer_capture_overlaps,
            capture,
        );
    }
}

/// **One door body's access to a gesture.** The methods themselves are written
/// below so the source index can see every `Runtime` item; this macro expands
/// expressions only. Each body answers only for this window's capture and, for
/// a gesture that belongs to a tab, only while that tab is in front.
macro_rules! gesture_door {
    (@window held $this:ident $variant:ident) => {{
        match &$this.capture_where(|capture| matches!(capture.owner, CaptureOwner::$variant(..)))?.owner {
            CaptureOwner::$variant(value) => Some(value),
            _ => None,
        }
    }};
    (@window held_mut $this:ident $variant:ident) => {{
        match &mut $this.capture_where_mut(|capture| matches!(capture.owner, CaptureOwner::$variant(..)))?.owner {
            CaptureOwner::$variant(value) => Some(value),
            _ => None,
        }
    }};
    (@window take $this:ident $variant:ident) => {{
        let window = $this.window.window.id();
        let capture = $this.app.take_capture_where(|capture| {
            capture.window == window && matches!(capture.owner, CaptureOwner::$variant(..))
        })?;
        match capture.owner {
            CaptureOwner::$variant(value) => Some(value),
            _ => None,
        }
    }};
    (@window latch $this:ident $variant:ident $value:ident) => {{
        $this.latch_capture(CaptureOwner::$variant($value), MouseButton::Left);
    }};
    (@window drop $this:ident $variant:ident) => {{
        let window = $this.window.window.id();
        let _ = $this.app.take_capture_where(|capture| {
            capture.window == window && matches!(capture.owner, CaptureOwner::$variant(..))
        });
    }};
    (@tab held $this:ident $variant:ident) => {{
        let id = $this.id;
        match &$this.capture_where(|capture| matches!(capture.owner, CaptureOwner::$variant(tab, _) if tab == id))?.owner {
            CaptureOwner::$variant(tab, value) if *tab == $this.id => Some(value),
            _ => None,
        }
    }};
    (@tab held_mut $this:ident $variant:ident) => {{
        let id = $this.id;
        match &mut $this.capture_where_mut(|capture| matches!(capture.owner, CaptureOwner::$variant(tab, _) if tab == id))?.owner {
            CaptureOwner::$variant(tab, value) if *tab == id => Some(value),
            _ => None,
        }
    }};
    (@tab take $this:ident $variant:ident) => {{
        let (window, id) = ($this.window.window.id(), $this.id);
        let capture = $this.app.take_capture_where(|capture| {
            capture.window == window
                && matches!(capture.owner, CaptureOwner::$variant(tab, _) if tab == id)
        })?;
        match capture.owner {
            CaptureOwner::$variant(_, value) => Some(value),
            _ => None,
        }
    }};
    (@tab latch $this:ident $variant:ident $value:ident) => {{
        let tab = $this.id;
        $this.latch_capture(CaptureOwner::$variant(tab, $value), MouseButton::Left);
    }};
    (@tab drop $this:ident $variant:ident) => {{
        let (window, id) = ($this.window.window.id(), $this.id);
        let _ = $this.app.take_capture_where(|capture| {
            capture.window == window
                && matches!(capture.owner, CaptureOwner::$variant(tab, _) if tab == id)
        });
    }};
}

impl Runtime<'_> {
    pub(crate) fn held_divider_drag(&self) -> Option<&DividerDrag> {
        gesture_door!(@window held self Divider)
    }

    pub(crate) fn take_divider_drag(&mut self) -> Option<DividerDrag> {
        gesture_door!(@window take self Divider)
    }

    pub(crate) fn latch_divider_drag(&mut self, value: DividerDrag) {
        gesture_door!(@window latch self Divider value)
    }

    pub(crate) fn drop_divider_drag(&mut self) {
        gesture_door!(@window drop self Divider)
    }

    pub(crate) fn held_tab_press(&self) -> Option<&TabPress> {
        gesture_door!(@window held self TabPress)
    }

    pub(crate) fn held_tab_press_mut(&mut self) -> Option<&mut TabPress> {
        gesture_door!(@window held_mut self TabPress)
    }

    pub(crate) fn take_tab_press(&mut self) -> Option<TabPress> {
        gesture_door!(@window take self TabPress)
    }

    pub(crate) fn latch_tab_press(&mut self, value: TabPress) {
        gesture_door!(@window latch self TabPress value)
    }

    pub(crate) fn drop_tab_press(&mut self) {
        gesture_door!(@window drop self TabPress)
    }

    pub(crate) fn held_row_press(&self) -> Option<&RowPress> {
        gesture_door!(@window held self RowPress)
    }

    pub(crate) fn held_row_press_mut(&mut self) -> Option<&mut RowPress> {
        gesture_door!(@window held_mut self RowPress)
    }

    pub(crate) fn take_row_press(&mut self) -> Option<RowPress> {
        gesture_door!(@window take self RowPress)
    }

    pub(crate) fn latch_row_press(&mut self, value: RowPress) {
        gesture_door!(@window latch self RowPress value)
    }

    pub(crate) fn drop_row_press(&mut self) {
        gesture_door!(@window drop self RowPress)
    }

    pub(crate) fn held_float_head_press(&self) -> Option<&FloatHeadPress> {
        gesture_door!(@window held self FloatHeadPress)
    }

    pub(crate) fn held_float_head_press_mut(&mut self) -> Option<&mut FloatHeadPress> {
        gesture_door!(@window held_mut self FloatHeadPress)
    }

    pub(crate) fn latch_float_head_press(&mut self, value: FloatHeadPress) {
        gesture_door!(@window latch self FloatHeadPress value)
    }

    pub(crate) fn drop_float_head_press(&mut self) {
        gesture_door!(@window drop self FloatHeadPress)
    }

    pub(crate) fn held_float_drag(&self) -> Option<&FloatDrag> {
        gesture_door!(@window held self FloatDrag)
    }

    pub(crate) fn latch_float_drag(&mut self, value: FloatDrag) {
        gesture_door!(@window latch self FloatDrag value)
    }

    pub(crate) fn drop_float_drag(&mut self) {
        gesture_door!(@window drop self FloatDrag)
    }

    pub(crate) fn held_file_peek_press_mut(&mut self) -> Option<&mut FilePeekPress> {
        gesture_door!(@window held_mut self GlanceHeadPress)
    }

    pub(crate) fn take_file_peek_press(&mut self) -> Option<FilePeekPress> {
        gesture_door!(@window take self GlanceHeadPress)
    }

    pub(crate) fn latch_file_peek_press(&mut self, value: FilePeekPress) {
        gesture_door!(@window latch self GlanceHeadPress value)
    }

    pub(crate) fn drop_file_peek_press(&mut self) {
        gesture_door!(@window drop self GlanceHeadPress)
    }

    pub(crate) fn held_glance_thumb(&self) -> Option<&f32> {
        gesture_door!(@window held self GlanceThumb)
    }

    pub(crate) fn take_glance_thumb(&mut self) -> Option<f32> {
        gesture_door!(@window take self GlanceThumb)
    }

    pub(crate) fn drop_glance_thumb(&mut self) {
        gesture_door!(@window drop self GlanceThumb)
    }

    pub(crate) fn held_video_bar_drag(&self) -> Option<&PreviewSurface> {
        gesture_door!(@window held self VideoBar)
    }

    pub(crate) fn take_video_bar_drag(&mut self) -> Option<PreviewSurface> {
        gesture_door!(@window take self VideoBar)
    }

    pub(crate) fn latch_video_bar_drag(&mut self, value: PreviewSurface) {
        gesture_door!(@window latch self VideoBar value)
    }

    pub(crate) fn held_settings_slider_drag(&self) -> Option<&settings::SettingsRow> {
        gesture_door!(@window held self SettingsSlider)
    }

    pub(crate) fn latch_settings_slider_drag(&mut self, value: settings::SettingsRow) {
        gesture_door!(@window latch self SettingsSlider value)
    }

    pub(crate) fn held_settings_menu_bar_drag(&self) -> Option<&f32> {
        gesture_door!(@window held self SettingsMenuBar)
    }

    pub(crate) fn latch_settings_menu_bar_drag(&mut self, value: f32) {
        gesture_door!(@window latch self SettingsMenuBar value)
    }

    pub(crate) fn held_pane_press(&self) -> Option<&PanePress> {
        gesture_door!(@tab held self PanePress)
    }

    pub(crate) fn held_pane_press_mut(&mut self) -> Option<&mut PanePress> {
        gesture_door!(@tab held_mut self PanePress)
    }

    pub(crate) fn take_pane_press(&mut self) -> Option<PanePress> {
        gesture_door!(@tab take self PanePress)
    }

    pub(crate) fn latch_pane_press(&mut self, value: PanePress) {
        gesture_door!(@tab latch self PanePress value)
    }

    pub(crate) fn drop_pane_press(&mut self) {
        gesture_door!(@tab drop self PanePress)
    }

    pub(crate) fn held_preview_body_drag(&self) -> Option<&PreviewBodyDrag> {
        gesture_door!(@tab held self PreviewBodyThumb)
    }

    pub(crate) fn take_preview_body_drag(&mut self) -> Option<PreviewBodyDrag> {
        gesture_door!(@tab take self PreviewBodyThumb)
    }

    pub(crate) fn latch_preview_body_drag(&mut self, value: PreviewBodyDrag) {
        gesture_door!(@tab latch self PreviewBodyThumb value)
    }

    pub(crate) fn drop_preview_body_drag(&mut self) {
        gesture_door!(@tab drop self PreviewBodyThumb)
    }

    pub(crate) fn held_preview_block_drag(&self) -> Option<&PreviewBlockDrag> {
        gesture_door!(@tab held self BlockThumb)
    }

    pub(crate) fn take_preview_block_drag(&mut self) -> Option<PreviewBlockDrag> {
        gesture_door!(@tab take self BlockThumb)
    }

    pub(crate) fn latch_preview_block_drag(&mut self, value: PreviewBlockDrag) {
        gesture_door!(@tab latch self BlockThumb value)
    }

    pub(crate) fn drop_preview_block_drag(&mut self) {
        gesture_door!(@tab drop self BlockThumb)
    }

    pub(crate) fn held_preview_image_drag(&self) -> Option<&ImageDrag> {
        gesture_door!(@tab held self PicturePan)
    }

    pub(crate) fn take_preview_image_drag(&mut self) -> Option<ImageDrag> {
        gesture_door!(@tab take self PicturePan)
    }

    pub(crate) fn latch_preview_image_drag(&mut self, value: ImageDrag) {
        gesture_door!(@tab latch self PicturePan value)
    }

    pub(crate) fn held_preview_selecting(&self) -> Option<&PreviewSurface> {
        gesture_door!(@tab held self EditSelection)
    }

    pub(crate) fn take_preview_selecting(&mut self) -> Option<PreviewSurface> {
        gesture_door!(@tab take self EditSelection)
    }

    pub(crate) fn latch_preview_selecting(&mut self, value: PreviewSurface) {
        gesture_door!(@tab latch self EditSelection value)
    }

    pub(crate) fn drop_preview_selecting(&mut self) {
        gesture_door!(@tab drop self EditSelection)
    }

    pub(crate) fn held_preview_text_drag(&self) -> Option<&PreviewTextDrag> {
        gesture_door!(@tab held self RenderedSelection)
    }

    pub(crate) fn held_preview_text_drag_mut(&mut self) -> Option<&mut PreviewTextDrag> {
        gesture_door!(@tab held_mut self RenderedSelection)
    }

    pub(crate) fn take_preview_text_drag(&mut self) -> Option<PreviewTextDrag> {
        gesture_door!(@tab take self RenderedSelection)
    }

    pub(crate) fn latch_preview_text_drag(&mut self, value: PreviewTextDrag) {
        gesture_door!(@tab latch self RenderedSelection value)
    }

    pub(crate) fn drop_preview_text_drag(&mut self) {
        gesture_door!(@tab drop self RenderedSelection)
    }

    pub(crate) fn held_terminal_thumb_drag(&self) -> Option<&TerminalThumbDrag> {
        gesture_door!(@tab held self TerminalThumb)
    }

    pub(crate) fn take_terminal_thumb_drag(&mut self) -> Option<TerminalThumbDrag> {
        gesture_door!(@tab take self TerminalThumb)
    }

    pub(crate) fn latch_terminal_thumb_drag(&mut self, value: TerminalThumbDrag) {
        gesture_door!(@tab latch self TerminalThumb value)
    }

    pub(crate) fn held_terminal_column_drag(&self) -> Option<&TerminalColumnDrag> {
        gesture_door!(@tab held self TerminalFootMark)
    }

    pub(crate) fn take_terminal_column_drag(&mut self) -> Option<TerminalColumnDrag> {
        gesture_door!(@tab take self TerminalFootMark)
    }

    pub(crate) fn latch_terminal_column_drag(&mut self, value: TerminalColumnDrag) {
        gesture_door!(@tab latch self TerminalFootMark value)
    }
}

impl Runtime<'_> {
    /// **This window's most recently latched capture**, including the bounded
    /// legacy-overlap list retained until cut 4.
    pub(crate) fn own_capture(&self) -> Option<&PointerCapture> {
        let window = self.window.window.id();
        self.capture_where(|capture| capture.window == window)
    }

    fn capture_where(
        &self,
        mut predicate: impl FnMut(&PointerCapture) -> bool,
    ) -> Option<&PointerCapture> {
        let window = self.window.window.id();
        self.app
            .pointer_capture
            .as_ref()
            .filter(|capture| capture.window == window && predicate(capture))
            .or_else(|| {
                self.app
                    .pointer_capture_overlaps
                    .iter()
                    .rev()
                    .find(|capture| capture.window == window && predicate(capture))
            })
    }

    fn capture_where_mut(
        &mut self,
        mut predicate: impl FnMut(&PointerCapture) -> bool,
    ) -> Option<&mut PointerCapture> {
        let window = self.window.window.id();
        if self
            .app
            .pointer_capture
            .as_ref()
            .is_some_and(|capture| capture.window == window && predicate(capture))
        {
            return self.app.pointer_capture.as_mut();
        }
        self.app
            .pointer_capture_overlaps
            .iter_mut()
            .rev()
            .find(|capture| capture.window == window && predicate(capture))
    }

    /// **Whether a gesture of this window holds the pointer** — any of them,
    /// by the one slot ([`crate::Runtime::a_gesture_holds_the_pointer`]).
    pub(crate) fn a_capture_holds_the_pointer(&self) -> bool {
        let window = self.window.window.id();
        self.app
            .capture_index_where(|capture| capture.window == window)
            .is_some()
    }

    pub(crate) fn drop_own_capture(&mut self) {
        let window = self.window.window.id();
        let _ = self
            .app
            .take_capture_where(|capture| capture.window == window);
    }

    /// **Latch a gesture.** A gesture of the same button replaces the current
    /// stage (for example pane press → drag). A gesture of another button is
    /// retained beside it exactly as the independent legacy fields allowed;
    /// cut 4 removes or changes that overlap after the owner rules on R-8.
    pub(crate) fn latch_capture(&mut self, owner: CaptureOwner, button: MouseButton) {
        let started = self
            .window
            .pointer_memo
            .answer
            .as_ref()
            .and_then(|(_, hit)| hit.clone());
        let window = self.window.window.id();
        self.app.latch_capture(PointerCapture {
            window,
            owner,
            button,
            started,
        });
    }

    /// The drag in the hand.
    pub(crate) fn held_drag(&self) -> Option<&Drag> {
        match &self
            .capture_where(|capture| matches!(capture.owner, CaptureOwner::Drag(_)))?
            .owner
        {
            CaptureOwner::Drag(drag) => Some(drag),
            _ => None,
        }
    }

    pub(crate) fn held_drag_mut(&mut self) -> Option<&mut Drag> {
        match &mut self
            .capture_where_mut(|capture| matches!(capture.owner, CaptureOwner::Drag(_)))?
            .owner
        {
            CaptureOwner::Drag(drag) => Some(drag),
            _ => None,
        }
    }

    pub(crate) fn take_drag(&mut self) -> Option<Drag> {
        let window = self.window.window.id();
        match self
            .app
            .take_capture_where(|capture| {
                capture.window == window && matches!(capture.owner, CaptureOwner::Drag(_))
            })?
            .owner
        {
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
        match &self
            .capture_where(|capture| matches!(capture.owner, CaptureOwner::Route(_)))?
            .owner
        {
            CaptureOwner::Route(route) => Some(route),
            _ => None,
        }
    }

    pub(crate) fn take_mouse_route(&mut self) -> Option<MouseRoute> {
        let window = self.window.window.id();
        match self
            .app
            .take_capture_where(|capture| {
                capture.window == window && matches!(capture.owner, CaptureOwner::Route(_))
            })?
            .owner
        {
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
