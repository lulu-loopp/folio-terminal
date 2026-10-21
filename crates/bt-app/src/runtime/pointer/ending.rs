//! **Every capture can end without its release** (`docs/plans/design/pointer-capture-2026-10-09.md`
//! R-9, cut 5).
//!
//! A gesture ends by its own button coming up ([`Runtime::release_capture`]) — or by one of the
//! lifecycle doors below, which take away what the gesture was about while the button is still
//! down. Each door calls [`Runtime::end_capture`] with its ending **before** it changes the state
//! the owner names, and each owner answers each ending by an exhaustive match:
//!
//! * **cancel** ([`Ending::Cancel`]) — the window blurred, the system took the pointer away
//!   ([`Runtime::end_a_capture_that_lost_its_pointer`]), the window was hidden or closed; and,
//!   for an owner holding a pixel quantity taken at the press, a scale change
//!   ([`Ending::ScaleChange`]);
//! * **owner off the glass** ([`Ending::OffTheGlass`]) — the tab a gesture belongs to is no longer
//!   in front, or its pane is zoomed out of view, while it still exists;
//! * **owner gone** ([`OwnerGone`]) — the tab, pane, floating window, surface, glance card or
//!   settings sheet it names is closed or replaced.
//!
//! What each ending does is the owner's: a divider puts its ratio back, a drag goes home, a press
//! handed to a program tells it the button came up at the last cell it was told about, and every
//! other gesture keeps what it already wrote and drops its release's verb. An owner that is gone
//! is owed nothing, so that ending cannot fail and its type says so.
//!
//! **The lifecycle doors** (the matrix of cut 5; `every_lifecycle_door_ends_the_capture` holds
//! one row per door to calling [`Runtime::end_capture`]):
//!
//! | door | ending |
//! |---|---|
//! | `Runtime::window_blurred` | cancel |
//! | `Runtime::flush_pending_pty_resize`, through `end_a_capture_that_lost_its_pointer` | cancel |
//! | `Runtime::scale_factor_changed`, its first statement | cancel, for pixel-quantity owners |
//! | `Runtime::hide_quake_window` | cancel |
//! | `Runtime::close_window` | cancel |
//! | `Runtime::activate_tab`, before the tab in front changes | off the glass |
//! | `Runtime::close_tab` | owner gone |
//! | `Runtime::close_pane` | owner gone |
//! | `Runtime::toggle_pane_zoom` | off the glass |
//! | `Runtime::sweep_preview_panes` | owner gone |
//! | `Runtime::leave_preview_buffer_in` | owner gone |
//! | `Runtime::rebuild_preview_document` | owner gone (its blocks) |
//! | `Runtime::dismiss_float` | owner gone |
//! | `Runtime::hide_file_peek` | owner gone |
//! | `Runtime::settings_closed` | owner gone |
//!
//! Not yet items, and so not rows: `Runtime::drag_out_to_the_system` (the OS drag-out, §7) lands
//! with its feature, and its ending `OsDragHandoff` with it; `WebSeat::place` and
//! `WebSeat::park_for_handoff`, the seat's own button-up before a page leaves the glass, are
//! cut 7's, with the page's own capture.

use crate::window_seam::prelude::*;
use crate::{
    FloatDragKind, LeafId, MouseRoute, PasteTarget, PreviewSurface, RowHost, Runtime, TabId,
    UserInputKind, a_capture_still_holds_its_pointer, float, input,
};
use anyhow::Result;
use bt_layout::SeatId;
use std::time::Instant;

use super::capture::{CaptureOwner, PointerCapture, ReleaseSnapshot};

/// **An ending that may owe somebody something** (R-9): a ratio put back, a drag sent home, a
/// program told its button came up — so it can fail, as those can.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Ending {
    /// The capturing window blurred, was hidden or closed, or the system took the pointer away.
    Cancel,
    /// **The window's scale changed**: a cancel, for the owners whose payload is a pixel quantity
    /// taken at the press ([`CaptureOwner::holds_pixels_from_the_press`]) — measured in a space
    /// the window no longer has.
    ScaleChange,
    /// The surface a tab-bound gesture names left the glass while it still exists.
    OffTheGlass(Leaving),
}

/// **What leaves the glass**: every pane of `tab` but `kept` — the whole tab when the tab in
/// front changes, every other pane when one is zoomed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Leaving {
    pub(crate) tab: TabId,
    pub(crate) kept: Option<SeatId>,
}

impl Leaving {
    /// Whether the pane `seat` of `tab` leaves the glass.
    fn takes(self, tab: TabId, seat: SeatId) -> bool {
        tab == self.tab && self.kept != Some(seat)
    }

    /// Whether a gesture on `surface`, kept with `tab`, leaves the glass: a pane's surface with
    /// its pane, a floating window's or the glance card's only with the whole tab.
    fn takes_the_surface(self, tab: TabId, surface: PreviewSurface) -> bool {
        match surface {
            PreviewSurface::Seat(leaf) => self.takes(leaf.tab, leaf.seat),
            PreviewSurface::Float(_) | PreviewSurface::Peek => {
                tab == self.tab && self.kept.is_none()
            }
        }
    }
}

/// **An ending that owes nobody anything** (R-9's *owner gone*): the thing a gesture names is
/// closed or replaced, so there is no ratio to put back on it and no program to tell, and the
/// gesture is simply let go of.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OwnerGone(pub(crate) Gone);

/// What went.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Gone {
    /// A tab of this window.
    Tab(TabId),
    /// A pane of the tab in front.
    Pane(LeafId),
    /// A floating window.
    Float(float::FloatId),
    /// A preview surface's document: the surface left the layout, or its buffer was left.
    Surface(PreviewSurface),
    /// A preview surface's blocks, which a new parse renumbers.
    Blocks(PreviewSurface),
    /// The glance card.
    GlanceCard,
    /// The settings sheet.
    SettingsSheet,
}

/// **One of R-9's endings**, and what ending a capture with it answers.
pub(crate) trait CaptureEnding: Copy {
    /// What the door is handed back: a result for an ending that may owe something, nothing for
    /// one that cannot.
    type Done;

    /// Whether this ending takes `owner` away, in a window whose tab in front is `active`.
    fn takes(self, owner: &CaptureOwner, active: TabId) -> bool;

    /// The ending's name, for the trace.
    fn name(self) -> &'static str;

    /// End `capture`, already out of the slot, on `runtime`.
    fn end<B: Backend>(self, runtime: &mut Runtime<'_, B>, capture: PointerCapture) -> Self::Done;

    /// The answer when nothing was held that this ending takes.
    fn nothing_held() -> Self::Done;
}

impl CaptureEnding for Ending {
    type Done = Result<()>;

    fn takes(self, owner: &CaptureOwner, active: TabId) -> bool {
        match self {
            Self::Cancel => true,
            Self::ScaleChange => owner.holds_pixels_from_the_press(),
            Self::OffTheGlass(glass) => owner.leaves_the_glass(glass, active),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Cancel => "cancel",
            Self::ScaleChange => "scale change",
            Self::OffTheGlass(_) => "off the glass",
        }
    }

    fn end<B: Backend>(self, runtime: &mut Runtime<'_, B>, capture: PointerCapture) -> Result<()> {
        runtime.cancel_a_released_capture(capture)
    }

    fn nothing_held() -> Result<()> {
        Ok(())
    }
}

impl CaptureEnding for OwnerGone {
    type Done = ();

    fn takes(self, owner: &CaptureOwner, active: TabId) -> bool {
        owner.is_gone_with(self.0, active)
    }

    fn name(self) -> &'static str {
        "owner gone"
    }

    fn end<B: Backend>(self, runtime: &mut Runtime<'_, B>, capture: PointerCapture) {
        runtime.let_go_of_a_gone_owner(capture);
    }

    fn nothing_held() {}
}

impl CaptureOwner {
    /// **The gesture's name**, as a trace line and a test say it.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Divider(_) => "Divider",
            Self::TabPress(_) => "TabPress",
            Self::PanePress(..) => "PanePress",
            Self::RowPress(_) => "RowPress",
            Self::Drag(_) => "Drag",
            Self::FloatHeadPress(_) => "FloatHeadPress",
            Self::FloatDrag(_) => "FloatDrag",
            Self::GlanceHeadPress(_) => "GlanceHeadPress",
            Self::GlanceThumb(_) => "GlanceThumb",
            Self::VideoBar(_) => "VideoBar",
            Self::PreviewBodyThumb(..) => "PreviewBodyThumb",
            Self::BlockThumb(..) => "BlockThumb",
            Self::PicturePan(..) => "PicturePan",
            Self::EditSelection(..) => "EditSelection",
            Self::RenderedSelection(..) => "RenderedSelection",
            Self::TerminalThumb(..) => "TerminalThumb",
            Self::TerminalFootMark(..) => "TerminalFootMark",
            Self::Route(MouseRoute::Local(_)) => "Route::Local",
            Self::Route(MouseRoute::Forward { .. }) => "Route::Forward",
            Self::Route(MouseRoute::MathBlock) => "Route::MathBlock",
            Self::SettingsSlider(_) => "SettingsSlider",
            Self::SettingsMenuBar(_) => "SettingsMenuBar",
        }
    }

    /// **The preview surface the gesture is held on**, for the gestures that are held on one.
    pub(crate) fn preview_surface(&self) -> Option<PreviewSurface> {
        match self {
            Self::VideoBar(surface) | Self::EditSelection(_, surface) => Some(*surface),
            Self::PreviewBodyThumb(_, drag) => Some(drag.surface),
            Self::BlockThumb(_, drag) => Some(drag.surface),
            Self::PicturePan(_, drag) => Some(drag.surface),
            Self::RenderedSelection(_, drag) => Some(drag.surface),
            Self::Divider(_)
            | Self::TabPress(_)
            | Self::PanePress(..)
            | Self::RowPress(_)
            | Self::Drag(_)
            | Self::FloatHeadPress(_)
            | Self::FloatDrag(_)
            | Self::GlanceHeadPress(_)
            | Self::GlanceThumb(_)
            | Self::TerminalThumb(..)
            | Self::TerminalFootMark(..)
            | Self::Route(_)
            | Self::SettingsSlider(_)
            | Self::SettingsMenuBar(_) => None,
        }
    }

    /// **Whether the payload holds a pixel quantity taken at the press** (R-9's scale change): a
    /// press's origin, which its six-pixel travel is measured from; a grab inside a thumb, a
    /// floating window's head or a carried tab; a picture's last point. Each is a distance in the
    /// window's physical pixels, so a new scale makes it a distance in a space the window no
    /// longer has. A divider, a scrubber, a selection on the edit surface and a press on a cell
    /// are read again from the pointer on every move, or are cells, and hold none.
    pub(crate) fn holds_pixels_from_the_press(&self) -> bool {
        match self {
            Self::TabPress(_)
            | Self::PanePress(..)
            | Self::RowPress(_)
            | Self::FloatHeadPress(_)
            | Self::GlanceHeadPress(_)
            | Self::GlanceThumb(_)
            | Self::PreviewBodyThumb(..)
            | Self::BlockThumb(..)
            | Self::PicturePan(..)
            | Self::RenderedSelection(..)
            | Self::TerminalThumb(..)
            | Self::TerminalFootMark(..)
            | Self::SettingsMenuBar(_) => true,
            Self::Drag(drag) => drag.tab_carry().is_some(),
            Self::FloatDrag(drag) => matches!(drag.kind, FloatDragKind::Move { .. }),
            Self::Divider(_)
            | Self::VideoBar(_)
            | Self::EditSelection(..)
            | Self::Route(_)
            | Self::SettingsSlider(_) => false,
        }
    }

    /// **Whether this gesture leaves the glass with `glass`** (R-9's *owner off the glass*), in
    /// a window whose tab in front is `active`.
    ///
    /// Only a gesture whose payload names a tab-bound surface does. A drag does not: its owner is
    /// the payload in the hand, drawn over whichever tab is in front, and the spring that
    /// switches tabs under it is the gesture's own aim. Nor does a gesture on the window's own
    /// surfaces — a tab of the strip, a floating window, the glance card, the settings sheet —
    /// which end when they are gone. A divider and a formula block are gestures on the tab in
    /// front that name no pane: a divider leaves with any pane, since a zoomed pane shows no
    /// divider; a formula block, whose pane the route does not record, leaves with the tab.
    fn leaves_the_glass(&self, glass: Leaving, active: TabId) -> bool {
        match self {
            Self::Divider(_) => active == glass.tab,
            Self::PanePress(tab, press) => glass.takes(*tab, press.seat),
            Self::RowPress(press) => match press.host {
                RowHost::Column(seat) | RowHost::Git(seat) | RowHost::Terminal(seat) => {
                    glass.takes(active, seat)
                }
                RowHost::Float(_) => false,
            },
            Self::VideoBar(surface) => match surface {
                PreviewSurface::Seat(leaf) => glass.takes(leaf.tab, leaf.seat),
                PreviewSurface::Float(_) | PreviewSurface::Peek => false,
            },
            Self::PreviewBodyThumb(tab, drag) => glass.takes_the_surface(*tab, drag.surface),
            Self::BlockThumb(tab, drag) => glass.takes_the_surface(*tab, drag.surface),
            Self::PicturePan(tab, drag) => glass.takes_the_surface(*tab, drag.surface),
            Self::EditSelection(tab, surface) => glass.takes_the_surface(*tab, *surface),
            Self::RenderedSelection(tab, drag) => glass.takes_the_surface(*tab, drag.surface),
            Self::TerminalThumb(tab, drag) => glass.takes(*tab, drag.seat),
            Self::TerminalFootMark(tab, drag) => glass.takes(*tab, drag.seat),
            Self::Route(MouseRoute::Local(selection)) => {
                glass.takes(selection.owner.tab, selection.owner.seat)
            }
            Self::Route(MouseRoute::Forward { owner, .. }) => glass.takes(owner.tab, owner.seat),
            Self::Route(MouseRoute::MathBlock) => active == glass.tab && glass.kept.is_none(),
            Self::Drag(_)
            | Self::TabPress(_)
            | Self::FloatHeadPress(_)
            | Self::FloatDrag(_)
            | Self::GlanceHeadPress(_)
            | Self::GlanceThumb(_)
            | Self::SettingsSlider(_)
            | Self::SettingsMenuBar(_) => false,
        }
    }

    /// **Whether this gesture's owner went with `gone`** (R-9's *owner gone*), in a window whose
    /// tab in front is `active`. A gesture on the tab in front that names no tab of its own — a
    /// divider, a files row, a formula block — goes with that tab.
    fn is_gone_with(&self, gone: Gone, active: TabId) -> bool {
        let pane_of = |tab: TabId, seat: SeatId| matches!(gone, Gone::Pane(leaf) if leaf == LeafId { tab, seat });
        let the_tab = |tab: TabId| matches!(gone, Gone::Tab(gone) if gone == tab);
        let on_surface =
            |tab: TabId, surface: PreviewSurface| the_tab(tab) || surface_goes(gone, surface);
        match self {
            Self::Divider(_) | Self::Route(MouseRoute::MathBlock) => the_tab(active),
            Self::TabPress(press) => the_tab(press.tab),
            Self::PanePress(tab, press) => the_tab(*tab) || pane_of(*tab, press.seat),
            Self::RowPress(press) => match press.host {
                RowHost::Column(seat) | RowHost::Git(seat) | RowHost::Terminal(seat) => {
                    the_tab(active) || pane_of(active, seat)
                }
                RowHost::Float(id) => matches!(gone, Gone::Float(gone) if gone == id),
            },
            // The drag of a tab that is closed: its payload stopped existing. A pane or a row in
            // the hand is the drag's own to settle when its source goes.
            Self::Drag(drag) => drag.tab().is_some_and(the_tab),
            Self::FloatHeadPress(press) => matches!(gone, Gone::Float(id) if id == press.win),
            Self::FloatDrag(drag) => matches!(gone, Gone::Float(id) if id == drag.win),
            Self::GlanceHeadPress(_) | Self::GlanceThumb(_) => gone == Gone::GlanceCard,
            Self::VideoBar(surface) => {
                surface_goes(gone, *surface)
                    || matches!(surface, PreviewSurface::Seat(leaf) if the_tab(leaf.tab))
            }
            Self::PreviewBodyThumb(tab, drag) => on_surface(*tab, drag.surface),
            Self::PicturePan(tab, drag) => on_surface(*tab, drag.surface),
            Self::EditSelection(tab, surface) => on_surface(*tab, *surface),
            Self::RenderedSelection(tab, drag) => on_surface(*tab, drag.surface),
            Self::BlockThumb(tab, drag) => {
                on_surface(*tab, drag.surface)
                    || matches!(gone, Gone::Blocks(surface) if surface == drag.surface)
            }
            Self::TerminalThumb(tab, drag) => the_tab(*tab) || pane_of(*tab, drag.seat),
            Self::TerminalFootMark(tab, drag) => the_tab(*tab) || pane_of(*tab, drag.seat),
            Self::Route(MouseRoute::Local(selection)) => goes_with_its_shell(gone, selection.owner),
            Self::Route(MouseRoute::Forward { owner, .. }) => goes_with_its_shell(gone, *owner),
            Self::SettingsSlider(_) | Self::SettingsMenuBar(_) => gone == Gone::SettingsSheet,
        }
    }
}

/// Whether a gesture on `surface` went with `gone`: its document, its floating window, its pane,
/// or — the glance card's surface — the card.
fn surface_goes(gone: Gone, surface: PreviewSurface) -> bool {
    match gone {
        Gone::Surface(went) => went == surface,
        Gone::Float(id) => surface == PreviewSurface::Float(id),
        Gone::Pane(leaf) => surface == PreviewSurface::Seat(leaf),
        Gone::GlanceCard => surface == PreviewSurface::Peek,
        Gone::Tab(_) | Gone::Blocks(_) | Gone::SettingsSheet => false,
    }
}

/// Whether a press on a shell's cells went with `gone`: its tab or its pane.
fn goes_with_its_shell(gone: Gone, shell: PasteTarget) -> bool {
    match gone {
        Gone::Tab(tab) => tab == shell.tab,
        Gone::Pane(leaf) => leaf.tab == shell.tab && leaf.seat == shell.seat,
        Gone::Float(_)
        | Gone::Surface(_)
        | Gone::Blocks(_)
        | Gone::GlanceCard
        | Gone::SettingsSheet => false,
    }
}

impl<B: Backend> Runtime<'_, B> {
    /// **End this window's capture with `ending`, if the ending takes it** (R-9) — the one call
    /// every lifecycle door makes, before it changes what the owner names.
    pub(crate) fn end_capture<E: CaptureEnding>(&mut self, ending: E) -> E::Done {
        let (window, active) = (self.window.window.id(), self.id);
        let Some(capture) = self.app.take_capture_where(|capture| {
            capture.window == window && ending.takes(&capture.owner, active)
        }) else {
            return E::nothing_held();
        };
        self.mouse_trace(|| {
            format!(
                "capture ended owner={} by={}",
                capture.owner.name(),
                ending.name()
            )
        });
        #[cfg(test)]
        self.app
            .capture_endings
            .push((capture.owner.name(), ending.name()));
        ending.end(self, capture)
    }

    /// **End a gesture the system has taken the pointer away from** (Codex review 2026-09-17,
    /// for the divider; R-9's cancel, for every gesture): the capture's sample at the press
    /// ([`crate::runtime::pointer::PointerCapture::os`]) against the one taken now, only while
    /// this window holds a capture — one `GetCapture` a turn while a gesture is in the air, and
    /// none at all otherwise. The whole of the test is [`a_capture_still_holds_its_pointer`].
    pub(crate) fn end_a_capture_that_lost_its_pointer(&mut self) -> Result<bool> {
        let Some(at_the_press) = self.own_capture().map(|capture| capture.os) else {
            return Ok(false);
        };
        if a_capture_still_holds_its_pointer(at_the_press, bt_platform::thread_mouse_capture()) {
            return Ok(false);
        }
        self.end_capture(Ending::Cancel)?;
        Ok(true)
    }

    /// **A gesture cancelled, already out of the slot** (R-9's cancel and off-the-glass): what it
    /// is owed, by an exhaustive match on its owner.
    fn cancel_a_released_capture(&mut self, capture: PointerCapture) -> Result<()> {
        match capture.owner {
            // Its ratio put back, read off the tab whose seats it moved — which is why every door
            // that changes the tab ends the capture first.
            CaptureOwner::Divider(drag) => {
                self.put_the_ratio_back(drag)?;
                if let Some(position) = self.window.pointer_position {
                    self.update_chrome_hover(position)?;
                }
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
                Ok(())
            }
            CaptureOwner::Drag(drag) => self.send_the_drag_home(&drag).map(|_| ()),
            // A selection drawn across a page lets go of the span it was holding, and its link is
            // not followed: a cancelled click is no click.
            CaptureOwner::RenderedSelection(..)
            | CaptureOwner::PreviewBodyThumb(..)
            | CaptureOwner::BlockThumb(..) => self.repaint_preview(),
            CaptureOwner::Route(MouseRoute::Forward { button, sgr, owner }) => {
                self.tell_the_program_its_button_came_up(button, sgr, owner, capture.last)
            }
            // The ink comes off with the gesture, as it does with the button.
            CaptureOwner::Route(MouseRoute::MathBlock) => {
                if self.window.math_tool_pressed.take().is_some() {
                    self.repaint_hovered_pane()?;
                }
                Ok(())
            }
            // The scrubber keeps the fraction it wrote and puts its dot away.
            CaptureOwner::VideoBar(surface) => {
                self.put_the_scrubber_away(surface);
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
                Ok(())
            }
            CaptureOwner::TerminalThumb(_, drag) => {
                self.wake_terminal_thumb(drag.seat);
                self.repaint_pane_change(drag.seat)
            }
            CaptureOwner::TerminalFootMark(_, drag) => {
                self.wake_terminal_column(drag.seat);
                self.repaint_pane_change(drag.seat)
            }
            CaptureOwner::PicturePan(..) | CaptureOwner::FloatDrag(_) => {
                self.apply_pointer_cursor();
                Ok(())
            }
            CaptureOwner::GlanceThumb(_)
            | CaptureOwner::SettingsSlider(_)
            | CaptureOwner::SettingsMenuBar(_) => {
                if self.refresh_chrome() {
                    self.present_chrome_change()?;
                }
                Ok(())
            }
            // A selection on the cells or the edit surface keeps what it selected; a press that
            // had not become anything is dropped.
            CaptureOwner::Route(MouseRoute::Local(_))
            | CaptureOwner::EditSelection(..)
            | CaptureOwner::TabPress(_)
            | CaptureOwner::PanePress(..)
            | CaptureOwner::RowPress(_)
            | CaptureOwner::FloatHeadPress(_)
            | CaptureOwner::GlanceHeadPress(_) => Ok(()),
        }
    }

    /// **A gesture whose owner is gone, already out of the slot**: nobody is owed anything, so
    /// all that is left is what the gesture had put up beside itself.
    fn let_go_of_a_gone_owner(&mut self, capture: PointerCapture) {
        match capture.owner {
            CaptureOwner::Divider(_) => self.window.seat_pointer.dragging = None,
            // F2: the payload stopped existing, so the application's pointer has nothing left to
            // broker either.
            CaptureOwner::Drag(_) => self.app.drag_broker = None,
            CaptureOwner::Route(MouseRoute::MathBlock) => self.window.math_tool_pressed = None,
            CaptureOwner::VideoBar(surface) => self.put_the_scrubber_away(surface),
            CaptureOwner::TabPress(_)
            | CaptureOwner::PanePress(..)
            | CaptureOwner::RowPress(_)
            | CaptureOwner::FloatHeadPress(_)
            | CaptureOwner::FloatDrag(_)
            | CaptureOwner::GlanceHeadPress(_)
            | CaptureOwner::GlanceThumb(_)
            | CaptureOwner::PreviewBodyThumb(..)
            | CaptureOwner::BlockThumb(..)
            | CaptureOwner::PicturePan(..)
            | CaptureOwner::EditSelection(..)
            | CaptureOwner::RenderedSelection(..)
            | CaptureOwner::TerminalThumb(..)
            | CaptureOwner::TerminalFootMark(..)
            | CaptureOwner::Route(MouseRoute::Local(_) | MouseRoute::Forward { .. })
            | CaptureOwner::SettingsSlider(_)
            | CaptureOwner::SettingsMenuBar(_) => {}
        }
    }

    /// A held scrubber or volume put away: the dot comes down and the dwell that takes the bar
    /// off the glass starts again, as a release does.
    fn put_the_scrubber_away(&mut self, surface: PreviewSurface) {
        if let Some(seat) = self.window.video.get_mut(surface) {
            seat.release(Instant::now());
        }
    }

    /// **A press handed to a program, told its button came up** (Q2's recommendation, R-9): at
    /// the last cell it was told about, in the encoding the press was sent in, with the
    /// modifiers of that last report — never the keys held when the ending happened, so a
    /// release a `Ctrl+Tab` caused does not report `Ctrl`. Sent to the shell the press was handed
    /// to and to no other, while it is still the one standing there; a shell that is gone is
    /// owed nothing.
    fn tell_the_program_its_button_came_up(
        &mut self,
        button: input::MouseProtocolButton,
        sgr: bool,
        owner: PasteTarget,
        last: Option<ReleaseSnapshot>,
    ) -> Result<()> {
        let Some(last) = last else {
            return Ok(());
        };
        if self.live_paste_target(owner).is_none() {
            return Ok(());
        }
        let bytes = input::mouse_bytes(
            sgr,
            button,
            input::MouseProtocolEvent::Release,
            last.cell.row,
            last.cell.column,
            last.modifiers,
        );
        self.mouse_trace(|| {
            format!(
                "capture_release forwarded=1 bytes={} cell={},{}",
                bytes.len(),
                last.cell.row,
                last.cell.column
            )
        });
        self.answer_attention(owner.seat, UserInputKind::MouseButton);
        self.send_mouse_input_to(owner.seat, &bytes, "forward mouse button release to PTY")
    }
}

#[cfg(test)]
#[path = "ending_tests.rs"]
mod tests;
