//! **The translated-touch road** — the sixth way pointer data enters Folio
//! (`docs/plans/design/pointer-capture-2026-10-09.md` §4.2, N-T).
//!
//! The system recognises a one-finger pan and hands each step to the window's
//! touch door; the door parks the step for the loop, and the loop spends it on
//! the wheel's road. Everything that names a [`bt_platform::PanStep`] lives
//! here, so the step's client point reaches the rest of the program only as
//! the pointer move and the wheel report this module makes of it.

use crate::{AppEvent, Runtime, diagnostics};
use anyhow::Result;
use std::cell::RefCell;
use std::rc::Rc;
use winit::dpi::PhysicalPosition;
use winit::event::MouseScrollDelta;
use winit::event_loop::EventLoopProxy;

/// **Hand this window's touch input to the system that already knows what to do
/// with it** (owner ruling 2026-09-21), and say so once when a finger arrives.
///
/// Folio writes no translation from touch to anything. Tap becomes click, drag
/// becomes scroll and press-and-hold becomes the menu because Windows makes
/// them so, with the system's own inertia and timings, the way every ordinary
/// program gets them — and `bt_platform::let_the_system_translate_touch` is the
/// one door that says it. On macOS the door does nothing, because a trackpad's
/// gestures already arrive as mouse and scroll events.
///
/// **The line is the self-report** (`docs/CONVENTIONS.md` §十 rule 2). Touch
/// reaches this program from a touch screen or from a remote-desktop tool, both
/// of which are somebody else's machine as far as this session is concerned, so
/// the road says once per window that it was walked: what is in
/// `diagnostics.log` afterwards separates "the finger never reached Folio" from
/// "it reached Folio and the system did something else with it". Once per
/// window, no position, nothing about what was touched.
///
/// Both window constructors call it, and neither lets it decide whether a
/// window opens: a refusal is a window that answers nothing to a finger, which
/// is precisely what it did before this door existed.
///
/// **And the one gesture the window answers** (0.4.4 ticket 11): a slide the
/// system recognised as a pan. The door hands each step of it to the closure
/// below from inside the window's message dispatch, where no `Runtime` can be
/// reached, so the closure parks the step and wakes the loop — one value and
/// one wake per `WM_GESTURE`, and nothing at all while no finger is down — and
/// [`Runtime::spend_parked_pans`] puts it on the wheel's road on the turn that
/// wake buys. The slot it parks in is returned, for the window runtime to own.
pub(crate) fn let_the_system_translate_touch(
    native: bt_platform::NativeWindow,
    proxy: &EventLoopProxy<AppEvent>,
) -> ParkedPans {
    let parked = ParkedPans::default();
    let panned = {
        let parked = Rc::clone(&parked);
        let proxy = proxy.clone();
        Box::new(move |step| {
            parked.borrow_mut().push(step);
            let _ = proxy.send_event(AppEvent::TouchPanned);
        })
    };
    if let Err(error) = bt_platform::let_the_system_translate_touch(
        native,
        Box::new(|| diagnostics::note("touch arrived; handed to the system")),
        panned,
    ) {
        diagnostics::note(&format!("touch door: {error}"));
    }
    parked
}

/// **The pans a window's touch door has answered, waiting for the loop**
/// (0.4.4 ticket 11).
///
/// `Rc<RefCell<_>>` and not a lock: the writer is the door's subclass and the
/// reader is [`Runtime::spend_parked_pans`], and both run on the window's own
/// thread — the subclass inside message dispatch, the reader on the turn after
/// it. A `Vec` rather than a sum, because a pan's opening step carries a point
/// and two pans in one turn would otherwise have one point between them.
pub(crate) type ParkedPans = Rc<RefCell<Vec<bt_platform::PanStep>>>;

/// **What one answered pan step is on the wheel's road** (0.4.4 ticket 11):
/// where the pointer is to be before the wheel turns, if the step opens a
/// pan, and the wheel report it makes, if it moved.
///
/// A pan is answered as a wheel turned under a still pointer. The pointer is
/// put where the pan went down because the wheel routes by where the pointer
/// is and a recognised pan is not promoted to mouse input — nothing else says
/// where the finger is — and it is put there **once**, so the pane under the
/// finger when it went down keeps the whole pan, the system's inertia
/// included, the way a pane under a still mouse keeps a spun wheel.
///
/// The report is **pixels, one for one**: the travel is physical pixels, which
/// is `PixelDelta`'s currency, and the sign is already the wheel's — a finger
/// moving down is positive `y`, which the wheel road reads as travel back up
/// the document, so the content follows the finger. It is the currency a
/// precision touchpad speaks, so every scroller that already answers a
/// trackpad answers a finger without learning anything.
pub(crate) fn pan_on_the_wheel_road(
    step: bt_platform::PanStep,
) -> (Option<PhysicalPosition<f64>>, Option<MouseScrollDelta>) {
    let pointer = step
        .began_at
        .map(|(x, y)| PhysicalPosition::new(f64::from(x), f64::from(y)));
    let (x, y) = step.travel;
    let wheel = ((x, y) != (0, 0))
        .then(|| MouseScrollDelta::PixelDelta(PhysicalPosition::new(f64::from(x), f64::from(y))));
    (pointer, wheel)
}

impl Runtime<'_> {
    /// **The pans the touch door parked, put on the wheel's road** (0.4.4
    /// ticket 11).
    ///
    /// Each step enters by the doors a mouse would use: a pan's opening point
    /// is [`Self::pointer_moved`] — after the wheel held so far is spent, as
    /// the dispatcher spends it before any pointer move — and its travel is
    /// [`Self::queue_wheel`], the wheel's own entrance, so the routing, the
    /// merging into one burst per turn, the scroll-back rules and every
    /// scroller that already answers a trackpad apply unchanged. There is no
    /// second scroll road, and nothing here recognises anything: the system
    /// said it was a pan, and [`pan_on_the_wheel_road`] only changes its
    /// currency.
    pub(crate) fn spend_parked_pans(&mut self) -> Result<()> {
        let steps = std::mem::take(&mut *self.window.parked_pans.borrow_mut());
        for step in steps {
            let (pointer, wheel) = pan_on_the_wheel_road(step);
            if let Some(position) = pointer {
                self.flush_wheel()?;
                self.pointer_moved(position)?;
            }
            if let Some(delta) = wheel {
                self.queue_wheel(delta)?;
            }
        }
        Ok(())
    }
}
