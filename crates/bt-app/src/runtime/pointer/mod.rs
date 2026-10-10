//! **Pointer ownership: one router over the whole stack, one capture per
//! latched gesture** (T-POINTER-CAPTURE,
//! `docs/plans/design/pointer-capture-2026-10-09.md`).
//!
//! **This module is where pointer data enters Folio.** It names the pointer
//! kinds of a window event and answers them ([`is_pointer_event`],
//! [`Runtime::pointer_event`]), names them for the hang watch
//! ([`station_of`]), owns the translated-touch road ([`touch`]) and the
//! router ([`router`]). Every other item of the crate that reads a pointer
//! field, holds a pointer position, names a button or wheel type, reads the
//! system cursor, names a pointer event kind or a touch step is a row of
//! `docs/plans/POINTER-DEBT.tsv`, which only shrinks
//! (`every_pointer_read_is_the_routers_or_a_captures`, §4.2).

mod capture;
mod release;
mod router;
pub(crate) mod touch;

#[cfg(test)]
pub(crate) use capture::{
    CaptureIndex, PressVerdict, cancel_then_route, capture_index_for_button_event, latch_in,
    press_against_the_slot, release_ends_the_capture,
};
pub(crate) use capture::{CaptureOwner, PointerCapture};

#[cfg(test)]
pub(crate) use router::{BANDS_THAT_TAKE_NO_POINTER, POINTER_LAYERS_TOP_FIRST, Visits};
pub(crate) use router::{
    FloatFacts, Plane, PointerFacts, PointerHit, PointerScene, walk_pointer_layers,
};

use crate::{
    Drag, PasteTarget, PreviewSurface, Runtime, float, git_panel, hang_watch,
    paste_offer_is_kept, risen_frame, seats,
};
#[cfg(not(target_os = "linux"))]
use crate::{native_window, platform_pointer_of};
#[cfg(not(target_os = "linux"))]
use bt_platform::NativeWindow;
use anyhow::Result;
use std::time::Instant;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{MouseButton, WindowEvent};

/// **Whether a window event is a pointer event** — the dispatcher's one
/// question about the pointer, answered here so that it names no pointer kind
/// of its own.
pub(crate) fn is_pointer_event(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::CursorMoved { .. }
            | WindowEvent::CursorEntered { .. }
            | WindowEvent::CursorLeft { .. }
            | WindowEvent::MouseInput { .. }
            | WindowEvent::MouseWheel { .. }
    )
}

/// Whether a window event is a wheel notch — the one kind the dispatcher lets
/// past the held burst unspent ([`crate::WheelBurst`]).
pub(crate) fn is_wheel_event(event: &WindowEvent) -> bool {
    matches!(event, WindowEvent::MouseWheel { .. })
}

#[cfg(target_os = "linux")]
fn linux_resize_direction(
    position: PhysicalPosition<f64>,
    size: PhysicalSize<u32>,
    scale: f64,
) -> Option<winit::window::ResizeDirection> {
    use winit::window::ResizeDirection;
    let width = f64::from(size.width);
    let height = f64::from(size.height);
    if position.x < 0.0 || position.y < 0.0 || position.x >= width || position.y >= height {
        return None;
    }
    let edge = 4.0 * scale;
    let west = position.x < edge;
    let east = position.x >= width - edge;
    let north = position.y < edge;
    let south = position.y >= height - edge;
    match (west, east, north, south) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (_, true, true, _) => Some(ResizeDirection::NorthEast),
        (true, _, _, true) => Some(ResizeDirection::SouthWest),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, _, _, _) => Some(ResizeDirection::West),
        (_, true, _, _) => Some(ResizeDirection::East),
        (_, _, true, _) => Some(ResizeDirection::North),
        (_, _, _, true) => Some(ResizeDirection::South),
        _ => None,
    }
}

/// **The hang watch's name for a pointer event**, and `None` for every other
/// kind — the pointer rows of [`crate::window_event_station`].
///
/// Exhaustive over every kind winit has, so a kind winit adds stops this
/// compiling until somebody says whether it is the pointer's.
pub(crate) fn station_of(event: &WindowEvent) -> Option<hang_watch::Station> {
    use hang_watch::Station;
    match event {
        WindowEvent::CursorMoved { .. } => Some(Station::EventPointer),
        WindowEvent::CursorLeft { .. } => Some(Station::EventCursorLeft),
        WindowEvent::MouseInput { .. } => Some(Station::EventMouse),
        WindowEvent::MouseWheel { .. } => Some(Station::EventWheel),
        WindowEvent::CursorEntered { .. } => Some(Station::EventCursorEntered),
        WindowEvent::CloseRequested
        | WindowEvent::KeyboardInput { .. }
        | WindowEvent::Ime(_)
        | WindowEvent::ModifiersChanged(_)
        | WindowEvent::Resized(_)
        | WindowEvent::ScaleFactorChanged { .. }
        | WindowEvent::Moved(_)
        | WindowEvent::ThemeChanged(_)
        | WindowEvent::Occluded(_)
        | WindowEvent::RedrawRequested
        | WindowEvent::Focused(_)
        | WindowEvent::DroppedFile(_)
        | WindowEvent::ActivationTokenDone { .. }
        | WindowEvent::Destroyed
        | WindowEvent::HoveredFile(_)
        | WindowEvent::HoveredFileCancelled
        | WindowEvent::PinchGesture { .. }
        | WindowEvent::PanGesture { .. }
        | WindowEvent::DoubleTapGesture { .. }
        | WindowEvent::RotationGesture { .. }
        | WindowEvent::TouchpadPressure { .. }
        | WindowEvent::AxisMotion { .. }
        | WindowEvent::Touch(_) => None,
    }
}

/// **The router's answer for the event being handled** (R-4).
///
/// Written once, at the event's door ([`Runtime::open_pointer_event`]), and
/// replaced by the next event's; every reader of an event reads this one
/// answer rather than walking again (the readers move onto it in cut 8).
#[derive(Default)]
pub(crate) struct PointerMemo {
    answer: Option<(PhysicalPosition<f64>, Option<PointerHit>)>,
}

impl crate::quake::SummonScreen {
    /// **The display the reader is working on**, which is the display the pointer is on.
    ///
    /// The pointer and not the window, because the pointer is the only thing on the desk that says
    /// which screen the person is at. When Windows will not say where the pointer is, the window's
    /// own display answers, and when it will not say that either the virtual screen does — in that
    /// order, because each fallback is one step further from the question actually asked.
    ///
    /// This is the machine-reading half of the one summon door; the deciding half is
    /// [`crate::quake::Quake::placement`], which is pure and is where the rules live.
    #[cfg(not(target_os = "linux"))]
    #[must_use]
    pub(crate) fn under_the_pointer(window: NativeWindow, cached_dpi: u32) -> Self {
        let pointer = bt_platform::pointer_position();
        let work = pointer
            .and_then(|(x, y)| bt_platform::work_area_at(x, y).ok())
            .or_else(|| bt_platform::get_work_area(window).ok())
            .unwrap_or_else(bt_platform::virtual_screen_rect);
        Self {
            work,
            monitor_id: pointer.and_then(|(x, y)| bt_platform::monitor_id_at(x, y)),
            dpi: pointer.map_or(cached_dpi, |(x, y)| bt_platform::dpi_at(x, y)),
        }
    }
}

impl Runtime<'_> {
    /// **Where the cursor is, this instant, in this window's own pixels**
    /// (GitHub issue #1 ②, owner's ruling 2026-09-16: a drop lands in the pane
    /// under the cursor).
    ///
    /// **Two readers, and they are the two gestures that put a path on a command
    /// line.** [`Self::collect_dropped_file`] asks it as an external drop
    /// arrives, and [`Self::keep_the_paste_offer`] asks it as an internal drag is
    /// let go of (review 2026-09-17). Both for one reason: there is only one
    /// instant at which "where is the cursor" and "where was this let go of" are
    /// the same question, and it is the one this process is standing in while
    /// the platform delivers the release. Nothing else in this window reads it,
    /// and the name is the platform's rather than either gesture's so that
    /// neither road can grow a second door.
    ///
    /// **The units are `CursorMoved`'s and no conversion happens here**, which
    /// was checked rather than assumed. On Windows a pointer event is
    /// `WM_MOUSEMOVE`'s `lParam` — physical pixels from the client area's
    /// top-left — which is precisely what `GetCursorPos` put through
    /// `ScreenToClient` answers. On macOS winit takes its view's point and
    /// multiplies by the window's backing scale, which is precisely what the
    /// AppKit arm does with `NSEvent.mouseLocation` after the same two
    /// conversions. So the platform's answer is already in the window's physical
    /// pixels and is used as it stands; scaling it again here would square the
    /// factor on every Retina and every 150% display.
    ///
    /// Neither `pointer_position` nor [`WindowRuntime::pointer_last_seen`] is
    /// read: both say where the hand was *before* the drag, which is not where
    /// this drop landed, and a routing built on either would be a guess wearing
    /// a measurement's clothes.
    #[cfg(not(target_os = "linux"))]
    pub(in crate::runtime) fn platform_pointer_now(&self) -> Option<PhysicalPosition<f64>> {
        platform_pointer_of(
            native_window(&self.window.window)
                .ok()
                .and_then(bt_platform::pointer_position_in_window),
        )
    }

    /// Recheck a path offer at the actual release point.
    ///
    /// Linux supplies the asynchronous X11 query's answer; the native release path supplies its
    /// cursor query. The original offer, target shell, and plan still have to agree before bytes
    /// are written.
    pub(in crate::runtime) fn paste_offer_kept_at(
        &self,
        drag: &Drag,
        plan: &seats::DropPlan,
        released_at: PhysicalPosition<f64>,
    ) -> Option<PasteTarget> {
        let mut seam = drag.seam;
        let at_release = self.survey_drop(&drag.source, drag.home, released_at, &mut seam);
        paste_offer_is_kept(
            self.glass_here(released_at),
            drag.paste_offer,
            self.paste_offer_at(at_release),
            plan.fits(),
            self.a_modal_holds_the_window(),
        )
    }

    /// **The one door every pointer event of a window comes through**
    /// (T-POINTER-CAPTURE cut 1). The dispatcher hands over each event
    /// [`is_pointer_event`] names; a pointer entering the window asks nothing,
    /// because the first move inside it is what says where it is.
    pub(crate) fn pointer_event(&mut self, event: WindowEvent) -> Result<()> {
        match event {
            WindowEvent::CursorMoved { position, .. } => self.pointer_moved(position),
            WindowEvent::CursorLeft { .. } => self.pointer_left(),
            WindowEvent::MouseInput { state, button, .. } => self.mouse_input(state, button),
            WindowEvent::MouseWheel { delta, .. } => self.queue_wheel(delta),
            _ => Ok(()),
        }
    }

    /// Start Linux's native client-edge resize from this button event.
    #[cfg(target_os = "linux")]
    pub(in crate::runtime) fn try_begin_linux_border_resize(
        &mut self,
        button: MouseButton,
    ) -> bool {
        if button == MouseButton::Left
            && self.window_maximized_state() == Some(false)
            && self.window.window.fullscreen().is_none()
            && let Some(position) = self.window.pointer_position
            && let Some(direction) = linux_resize_direction(
                position,
                self.client_size(),
                self.window.window.scale_factor(),
            )
            && self.window.window.drag_resize_window(direction).is_ok()
        {
            return true;
        }
        false
    }

    /// **Open one pointer event**: forget the last event's answer and walk the
    /// router once for this one (R-4). Called by each door — a move, a button,
    /// a burst of notches being spent — at its top, and by nothing else.
    pub(in crate::runtime) fn open_pointer_event(&mut self, position: PhysicalPosition<f64>) {
        let hit = self.pointer_layer_at(&self.window.pointer_facts, position);
        self.window.pointer_memo.answer = Some((position, hit));
    }

    /// **What is under a point** (§2.1): the router's walk over this frame's
    /// facts.
    fn pointer_layer_at(
        &self,
        facts: &PointerFacts,
        position: PhysicalPosition<f64>,
    ) -> Option<PointerHit> {
        walk_pointer_layers(&self.pointer_scene(), facts, position, |plane, position| {
            self.plane_at(plane, position)
        })
    }

    /// One of the planes beneath the overlay, by the hit tests those planes
    /// have today.
    fn plane_at(&self, plane: Plane, position: PhysicalPosition<f64>) -> Option<PointerHit> {
        match plane {
            Plane::DockedChrome => self
                .docked_chrome_target_at(position)
                .map(PointerHit::Chrome),
            Plane::PaneFurniture => self
                .terminal_bar_under(position)
                .map(|(seat, _)| router::Furniture::TerminalLane(seat))
                .or_else(|| {
                    self.terminal_column_bar_under(position)
                        .map(|(seat, _)| router::Furniture::TerminalFootMark(seat))
                })
                .or_else(|| {
                    self.command_rail_at(position)
                        .map(|(seat, tick)| router::Furniture::CommandRail(seat, tick))
                })
                .map(PointerHit::Furniture),
            Plane::HostedPage => self.web_page_shown_at(position).map(PointerHit::Page),
            Plane::PaneBody => {
                seats::pane_at(&self.seat_layout, position.x, position.y).map(PointerHit::Body)
            }
        }
    }

    /// What the walk borrows from this window for one event.
    fn pointer_scene(&self) -> PointerScene<'_> {
        PointerScene {
            toasts: &self.window.toast_layouts,
            palette: self.window.palette_layout.as_ref(),
            profile_programs: &self.app.profile_programs,
            recent: self.app.recent.entries(),
            float_git_pages: &self.window.float_git_pages_shown,
            graphs: &self.window.git_graphs_shown,
            float_hover: self.window.float_hover,
            notices: &self.window.notice_layouts,
            web_sheets: &self.window.web_sheet_layouts,
            search: self.window.search_layout.as_ref(),
        }
    }

    /// **Measure this frame's facts for the router** (§2.1) — called where the
    /// overlay is built, so a menu's layout, a floating window's captions and
    /// its body's list are measured once per frame rather than once per hit
    /// test. The lists are refilled in place.
    pub(in crate::runtime) fn refresh_pointer_facts(&mut self, now: Instant) {
        let mut facts = std::mem::take(&mut self.window.pointer_facts);
        self.measure_pointer_facts(&mut facts, now);
        self.window.pointer_facts = facts;
    }

    /// Fill `facts` for this frame, its lists refilled in place.
    fn measure_pointer_facts(&mut self, facts: &mut PointerFacts, now: Instant) {
        let scale = self.window.renderer.scale_factor() as f32;
        facts.scale = scale;
        facts.glance = self
            .window
            .file_peek
            .as_ref()
            .filter(|peek| peek.clock.is_shown())
            .and_then(|peek| peek.frame);
        facts.tab_menu = self.tab_menu_layout();
        facts.term_menu = self.term_menu_layout();
        facts.git_menu = self.git_menu_layout();
        facts.pane_menu = self.pane_menu_layout();
        facts.file_menu = self.file_menu_layout();
        facts.modal_covers_window = self.a_modal_covers_the_window();
        facts.profile_menu = self.profile_menu_layout();
        facts.root_menu = self.root_menu_layout();
        facts.graph_filter_menu = if self.window.graph_filter_menu.is_some() {
            self.graph_filter_menu_layout()
        } else {
            None
        };
        facts.preview_menu_items.clear();
        facts.preview_menu = match self.preview_menu_seat() {
            Some(seat) => {
                facts
                    .preview_menu_items
                    .extend(self.preview_menu_items(seat));
                self.preview_menu_layout()
            }
            None => None,
        };
        self.refresh_float_facts(&mut facts.floats, &mut facts.float_order, scale, now);
    }

    /// Every live floating window as this frame lays it out, front first —
    /// what `float_hit_at` measures on every call, measured here once.
    fn refresh_float_facts(
        &mut self,
        floats: &mut Vec<FloatFacts>,
        order: &mut Vec<float::FloatId>,
        scale: f32,
        now: Instant,
    ) {
        floats.clear();
        order.clear();
        if self.window.float.hit_order().next().is_none() {
            return;
        }
        let dock_label = self.float_dock_label_width(scale);
        let open_label = self.preview_open_button_label(now);
        let open_button_px = self.window.renderer.measure_chrome_text(
            &mut self.app.gpu,
            open_label,
            seats::PREVIEW_CARD_BUTTON_FONT_LOGICAL_PX * scale,
        );
        order.extend(self.window.float.hit_order().map(|win| win.epoch));
        for &id in order.iter() {
            let tools = self.float_head_tools(id);
            let rail = self.rail_geometry(PreviewSurface::Float(id), scale);
            let Some(win) = self.window.float.drawn().find(|win| win.epoch == id) else {
                continue;
            };
            let geometry = float::float_geometry(
                risen_frame(win.frame, self.float_fade_of(win, now, scale)),
                win.mode,
                scale,
                dock_label,
                tools,
            );
            let page = self.window.float_git_pages_shown.get(&id);
            let tree = win.files().filter(|_| page.is_none()).map(|files| {
                let rows = crate::files::tree_view(&files.files, &files.cache)
                    .rows
                    .len();
                seats::files_tree_geometry(geometry.body, rows, files.cache.scroll_px, scale)
            });
            let git = page.map(|page| git_panel::git_panel_geometry(geometry.body, page, scale));
            let card_button = self
                .float_refusal_words(PreviewSurface::Float(id), open_label)
                .filter(|words| words.verb.is_some())
                .and_then(|_| {
                    seats::preview_card_geometry(
                        geometry.body,
                        Some(open_button_px),
                        false,
                        0,
                        scale,
                    )
                    .button
                });
            floats.push(FloatFacts {
                id,
                geometry,
                rail,
                card_button,
                tree,
                git,
            });
        }
    }
}
