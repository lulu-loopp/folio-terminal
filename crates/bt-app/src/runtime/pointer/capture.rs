//! **The capture** (`docs/plans/design/pointer-capture-2026-10-09.md` §2.2):
//! one record per latched gesture, in one application slot.
//!
//! `App::pointer_capture` is the one gesture that holds the pointer, and
//! `App::swallowed` the buttons pressed while it was held (R-8, cut 4).
//!
//! Every gesture is read and written through the accessors below, so a
//! window reads only its own capture and a tab's gesture is read only on that
//! tab. Every button event is first compared with the slot, by button and
//! never by window ([`press_against_the_slot`], [`release_against_the_slot`]):
//! the release of the capture's button goes to its owner before any layer is
//! asked ([`Runtime::release_capture`], R-6), a press of the held button
//! proves the capture stale and cancels it first (R-5 rule 2), and a press of
//! any other button goes nowhere, nor does its release (R-5 rule 3, R-8).

use crate::window_seam::prelude::*;
use crate::{
    App, DividerDrag, Drag, FilePeekPress, FloatDrag, FloatHeadPress, ImageDrag, MouseRoute,
    PanePress, PreviewBlockDrag, PreviewBodyDrag, PreviewSurface, PreviewTextDrag, RowPress,
    Runtime, TabId, TabPress, TerminalColumnDrag, TerminalThumbDrag, settings,
};
use winit::event::MouseButton;
use winit::keyboard::ModifiersState;
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
    /// **Which window held the loop thread's mouse capture when the gesture began** — the
    /// divider's own sample (Codex review 2026-09-17), taken for every gesture (R-9, cut 5): a
    /// capture that is no longer the one taken at the press is a gesture the system took away
    /// with no button-up and no blur ([`Runtime::end_a_capture_that_lost_its_pointer`]). `None`
    /// where the platform has no per-thread capture (macOS), so there it never fires.
    pub(crate) os: Option<bt_platform::NativeWindow>,
    /// **The last report a press handed to the program was given** (§2.2's release snapshot),
    /// for the one owner whose ending is a message to somebody else: a forwarded press that ends
    /// without its release tells the program its button came up, at this cell and with these
    /// modifiers — never the keys held when the ending happened. `None` for every other owner.
    pub(crate) last: Option<ReleaseSnapshot>,
}

/// **What a forwarded press last told its program** (§2.2): the cell and the modifiers of the
/// last mouse report actually sent, press or motion. The program, the encoding and the button
/// are the route's own ([`MouseRoute::Forward`]), so they are not copied here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReleaseSnapshot {
    pub(crate) cell: bt_render::GridHit,
    pub(crate) modifiers: ModifiersState,
}

/// **The buttons that went down while a capture was held** (R-5 rule 3):
/// each was delivered to nothing, and so is its release, even one that comes
/// after the capture has ended.
///
/// A button's bit is the platform's own number for it: winit's five named
/// buttons are numbers 0–4 on Windows and in AppKit alike (primary, secondary,
/// middle, back, forward), and `Other(n)` is AppKit's `buttonNumber` n.
/// Windows has those five buttons and AppKit reports the buttons down as one
/// 64-bit mask (`NSEvent.pressedMouseButtons`), so a button number that has
/// no bit here is one neither platform can report as down; it is never
/// remembered.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ButtonSet(u64);

impl ButtonSet {
    fn bit(button: MouseButton) -> u64 {
        let number = match button {
            MouseButton::Left => 0,
            MouseButton::Right => 1,
            MouseButton::Middle => 2,
            MouseButton::Back => 3,
            MouseButton::Forward => 4,
            MouseButton::Other(number) => u32::from(number),
        };
        1_u64.checked_shl(number).unwrap_or(0)
    }

    pub(crate) fn contains(self, button: MouseButton) -> bool {
        self.0 & Self::bit(button) != 0
    }

    pub(crate) fn insert(&mut self, button: MouseButton) {
        self.0 |= Self::bit(button);
    }

    pub(crate) fn remove(&mut self, button: MouseButton) {
        self.0 &= !Self::bit(button);
    }
}

/// **What a press does with the slot** (R-5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PressVerdict {
    /// No gesture holds the pointer: the press is routed as it always was.
    Route,
    /// The held capture's own button went down again: its release was lost,
    /// so the capture is stale. It is cancelled, then the press is routed.
    CancelThenRoute,
    /// Another button, while a gesture holds the pointer (R-8): the press is
    /// delivered to nothing and its button is remembered, so its release is
    /// delivered to nothing too.
    Swallow,
}

/// **What a release does with the slot** (R-5 rule 3, R-6, R-8).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReleaseVerdict {
    /// No gesture holds the pointer and its press was not swallowed: the
    /// release is routed as it always was.
    Route,
    /// The release of the held capture's own button: it goes to the owner
    /// before any layer is asked, and the gesture ends.
    ToOwner,
    /// The release of a swallowed press, or of another button while a gesture
    /// holds the pointer: delivered to nothing.
    Swallow,
}

/// **The press precedence** (R-5): compared with the slot by button, never by
/// window, so a press arriving in another window than the capture's answers
/// exactly as one arriving in it.
pub(crate) fn press_against_the_slot(
    slot: Option<&PointerCapture>,
    button: MouseButton,
) -> PressVerdict {
    match slot {
        None => PressVerdict::Route,
        Some(capture) if capture.button == button => PressVerdict::CancelThenRoute,
        Some(_) => PressVerdict::Swallow,
    }
}

/// **The release precedence** (R-5 rule 3, R-6, R-8): a release is compared
/// with the swallowed buttons first — a swallowed press's release is
/// swallowed whatever has happened to the capture meanwhile — and only then
/// with the slot. As for a press, the window plays no part, and neither does
/// what is under the pointer.
pub(crate) fn release_against_the_slot(
    slot: Option<&PointerCapture>,
    swallowed: ButtonSet,
    button: MouseButton,
) -> ReleaseVerdict {
    if swallowed.contains(button) {
        return ReleaseVerdict::Swallow;
    }
    match slot {
        None => ReleaseVerdict::Route,
        Some(capture) if capture.button == button => ReleaseVerdict::ToOwner,
        Some(_) => ReleaseVerdict::Swallow,
    }
}

pub(crate) fn cancel_then_route<S, T, E>(
    state: &mut S,
    cancel_owner: impl FnOnce(&mut S) -> Result<(), E>,
    route_press: impl FnOnce(&mut S) -> Result<T, E>,
) -> Result<T, E> {
    cancel_owner(state)?;
    route_press(state)
}

impl<B: Backend> App<B> {
    /// **A press, answered by the slot and spent on the swallowed buttons.**
    /// A remembered bit of this button is stale — its release was lost to
    /// another process — so the press clears it, exactly as rule 2 proves a
    /// stale capture, before the slot answers; a swallowed press is remembered.
    pub(crate) fn answer_press(&mut self, button: MouseButton) -> PressVerdict {
        self.swallowed.remove(button);
        let verdict = press_against_the_slot(self.pointer_capture.as_ref(), button);
        if verdict == PressVerdict::Swallow {
            self.swallowed.insert(button);
        }
        verdict
    }

    /// **A release, answered by the slot and spent on the swallowed
    /// buttons**: the release of a remembered button clears its bit.
    pub(crate) fn answer_release(&mut self, button: MouseButton) -> ReleaseVerdict {
        let verdict =
            release_against_the_slot(self.pointer_capture.as_ref(), self.swallowed, button);
        self.swallowed.remove(button);
        verdict
    }

    pub(super) fn take_capture_where(
        &mut self,
        mut predicate: impl FnMut(&PointerCapture) -> bool,
    ) -> Option<PointerCapture> {
        self.pointer_capture.take_if(|capture| predicate(capture))
    }

    /// **A latch replaces what the slot holds.** Nothing else can be there: a
    /// press of the held button cancelled it first, a press of another button
    /// was swallowed and latched nothing, and every other latch is a later
    /// stage of this gesture (a pane press becoming a drag).
    fn latch_capture(&mut self, capture: PointerCapture) {
        self.pointer_capture = Some(capture);
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

impl<B: Backend> Runtime<'_, B> {
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

impl<B: Backend> Runtime<'_, B> {
    /// **This window's capture**, when the slot holds one of this window's.
    pub(crate) fn own_capture(&self) -> Option<&PointerCapture> {
        self.capture_where(|_| true)
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
    }

    fn capture_where_mut(
        &mut self,
        mut predicate: impl FnMut(&PointerCapture) -> bool,
    ) -> Option<&mut PointerCapture> {
        let window = self.window.window.id();
        self.app
            .pointer_capture
            .as_mut()
            .filter(|capture| capture.window == window && predicate(capture))
    }

    /// **Whether a gesture of this window holds the pointer** — any of them,
    /// by the one slot ([`crate::Runtime::a_gesture_holds_the_pointer`]).
    pub(crate) fn a_capture_holds_the_pointer(&self) -> bool {
        self.own_capture().is_some()
    }

    pub(crate) fn drop_own_capture(&mut self) {
        let window = self.window.window.id();
        let _ = self
            .app
            .take_capture_where(|capture| capture.window == window);
    }

    /// **Latch a gesture**, replacing what the slot holds
    /// ([`App::latch_capture`]'s reason).
    ///
    /// **The system's capture is sampled once per gesture.** What the slot already holds here is
    /// an earlier stage of this same gesture (a pane press becoming a drag), so its sample is the
    /// one taken at the press and is kept; a gesture's first latch takes it now.
    pub(crate) fn latch_capture(&mut self, owner: CaptureOwner, button: MouseButton) {
        let started = self
            .window
            .pointer_memo
            .answer
            .as_ref()
            .and_then(|(_, hit)| hit.clone());
        let window = self.window.window.id();
        let os = match self.app.pointer_capture.as_ref() {
            Some(held) if held.window == window && held.button == button => held.os,
            _ => bt_platform::thread_mouse_capture(),
        };
        self.app.latch_capture(PointerCapture {
            window,
            owner,
            button,
            started,
            os,
            last: None,
        });
    }

    /// **Remember what a forwarded press last told its program** (§2.2), on the route's own
    /// capture: written by the press that latched it and by every move reported under it, so an
    /// ending that owes the program a release sends it there ([`Self::end_capture`]).
    pub(crate) fn note_the_last_report_sent(
        &mut self,
        cell: bt_render::GridHit,
        modifiers: ModifiersState,
    ) {
        if let Some(capture) = self.capture_where_mut(|capture| {
            matches!(
                capture.owner,
                CaptureOwner::Route(MouseRoute::Forward { .. })
            )
        }) {
            capture.last = Some(ReleaseSnapshot { cell, modifiers });
        }
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
