//! **The system's pan gesture, answered as travel** (owner ruling 2026-09-21,
//! refined by the entry of 2026-09-23 in `docs/DESIGN.md`).
//!
//! Windows recognises a finger sliding over a window as `GID_PAN` and reports
//! it as a stream of `WM_GESTURE` messages, each carrying *where the pan is
//! now* in `GESTUREINFO::ptsLocation` — never how far it went. The first
//! message of a pan is flagged `GF_BEGIN` and *"indicates a pan start but does
//! not perform any panning"*; every later one, including the ones the system
//! makes up after the finger has lifted (*"the `GID_PAN` gesture has built-in
//! inertia. At the end of a pan gesture, additional pan gesture messages are
//! created by the operating system"*), is a new position; the last is flagged
//! `GF_END`
//! (<https://learn.microsoft.com/en-us/windows/win32/wintouch/wm-gesture>,
//! <https://learn.microsoft.com/en-us/windows/win32/wintouch/windows-touch-gestures-overview>).
//!
//! So the answer is a subtraction and nothing else: **the travel of one
//! message is its position minus the position of the message before it in the
//! same pan.** That is the whole of what this module is. It recognises
//! nothing — the system decided that this was a pan, and it decides when the
//! pan ends and how long the inertia runs; there is no threshold, no timer and
//! no velocity here, because a pan's feel belongs to the system (the ruling
//! this refines: *"Folio writes no gesture translation of its own"*).
//!
//! Platform-free on purpose: the arithmetic is the same question on every
//! machine, so it is tested on every machine, and only the Windows door
//! (`let_the_system_translate_touch`'s subclass) feeds it.

/// **One answered pan message, as the wheel road reads it.**
///
/// Both halves are in the window's own physical pixels — the currency of
/// `WM_MOUSEMOVE` and of a precision touchpad's `PixelDelta`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanStep {
    /// **Where the pan went down**, in client coordinates — `Some` on the step
    /// that opens a pan and on no other.
    ///
    /// The wheel is routed by where the pointer is, and a recognised pan is a
    /// gesture the system does *not* promote to mouse input, so nothing else
    /// tells the window where the finger is. The pan carries its own point for
    /// that reason, once: a wheel turned under a still pointer is what a pan
    /// is answered as, so the pane under the finger when it went down keeps
    /// the whole pan, inertia included.
    pub began_at: Option<(i32, i32)>,
    /// **How far the pan moved since the previous message of the same pan**,
    /// `(x, y)`; positive `y` is the finger moving down the screen and
    /// positive `x` is it moving right. The content follows the finger, so a
    /// finger moving down shows what is above — which is the direction a wheel
    /// turned away from the hand already means on the wheel road.
    pub travel: (i32, i32),
}

/// **The one pan a window is in the middle of**, as the last position it
/// reported. Nothing about the finger: the system's positions are the only
/// input, and forgetting them at `GF_END` is the only state change it makes
/// on its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PanTrack {
    last: Option<(i32, i32)>,
}

impl PanTrack {
    /// A new pan message: `begins` / `ends` are its `GF_BEGIN` / `GF_END`
    /// flags, `screen` its `ptsLocation` and `client` the same point in the
    /// window's client coordinates.
    ///
    /// Travel is measured in screen coordinates, because those are what the
    /// system reported and a window that moves under a pan must not turn its
    /// own movement into scrolling; the client point is used only to say where
    /// the pan went down.
    ///
    /// **A pan message with no pan open is a pan opening**, whatever its flags
    /// say — the window may have come into being, or had its door installed,
    /// after the `GF_BEGIN` it would have seen. Its position is the only fact
    /// there is, so it is where the travel is measured from.
    ///
    /// `None` when there is nothing to answer: a step that neither opens a pan
    /// nor moves one.
    pub fn step(
        &mut self,
        begins: bool,
        ends: bool,
        screen: (i32, i32),
        client: (i32, i32),
    ) -> Option<PanStep> {
        let previous = if begins { None } else { self.last };
        self.last = (!ends).then_some(screen);
        let Some(previous) = previous else {
            return Some(PanStep {
                began_at: Some(client),
                travel: (0, 0),
            });
        };
        let travel = (screen.0 - previous.0, screen.1 - previous.1);
        (travel != (0, 0)).then_some(PanStep {
            began_at: None,
            travel,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{PanStep, PanTrack};

    /// One `WM_GESTURE` / `GID_PAN` message, as the door reads it:
    /// `(GF_BEGIN, GF_INERTIA, GF_END, ptsLocation)`. The inertia flag is in
    /// the record because the system sets it, and the tests assert that it
    /// changes nothing — inertia is more positions, not a different rule.
    type Record = (bool, bool, bool, (i32, i32));

    /// Screen and client differ by the window's origin, as they do on a real
    /// desktop; the synthetic window stands at (300, 200).
    fn client_of(screen: (i32, i32)) -> (i32, i32) {
        (screen.0 - 300, screen.1 - 200)
    }

    fn run(records: &[Record]) -> Vec<Option<PanStep>> {
        let mut track = PanTrack::default();
        records
            .iter()
            .map(|&(begins, _inertia, ends, at)| track.step(begins, ends, at, client_of(at)))
            .collect()
    }

    fn moved(x: i32, y: i32) -> Option<PanStep> {
        Some(PanStep {
            began_at: None,
            travel: (x, y),
        })
    }

    fn opened_at(x: i32, y: i32) -> Option<PanStep> {
        Some(PanStep {
            began_at: Some((x, y)),
            travel: (0, 0),
        })
    }

    /// RED (0.4.4 ticket 11) — **a pan's travel is the difference between
    /// successive points.**
    ///
    /// The system reports where a pan *is*, never how far it went, and the
    /// first message of a pan *"does not perform any panning"*. A synthetic
    /// flick with the shape the documentation gives it: a begin, three moves
    /// with the finger down, three the system made up after it lifted (the
    /// first of them flagged `GF_INERTIA`), and the end at the last position.
    /// Every move is the step from the one before it — not from the begin,
    /// which would scroll the distance of the whole pan again on every
    /// message — and the begin carries the point the pan went down at, in
    /// client coordinates, and no travel.
    ///
    /// MUTATION: measure from the begin instead of from the previous message
    /// (leave `last` at the begin's point) and the third move reads (1, 60).
    #[test]
    fn a_pans_travel_is_the_difference_between_successive_points() {
        let steps = run(&[
            (true, false, false, (400, 500)),
            (false, false, false, (400, 512)),
            (false, false, false, (401, 530)),
            (false, false, false, (401, 560)),
            (false, true, false, (401, 580)),
            (false, false, false, (401, 590)),
            (false, false, false, (401, 594)),
            (false, false, true, (401, 594)),
        ]);
        assert_eq!(
            steps,
            vec![
                opened_at(100, 300),
                moved(0, 12),
                moved(1, 18),
                moved(0, 30),
                moved(0, 20),
                moved(0, 10),
                moved(0, 4),
                None,
            ],
            "begin → a point and no travel; moves and inertia → deltas; an end where the pan \
             already was → nothing"
        );
        let total = steps.iter().flatten().fold((0, 0), |sum, step| {
            (sum.0 + step.travel.0, sum.1 + step.travel.1)
        });
        assert_eq!(
            total,
            (1, 94),
            "the travel of a whole pan is its last point minus its first — nothing is spent twice"
        );
    }

    /// RED (0.4.4 ticket 11) — **a pan ends where the system says it ends, and
    /// the next one starts from its own point.**
    ///
    /// An end that moved is still travel (the system reported a position, and
    /// dropping it would lose it); what follows an end is measured from the
    /// next pan's own begin, never from where the last pan stopped. And a pan
    /// message with no pan open — the door installed mid-gesture — opens one
    /// rather than inventing travel from nowhere.
    ///
    /// MUTATION: keep `last` across `GF_END` and ignore `begins`, and the
    /// second pan opens with a travel of (0, -85) instead of at its own point.
    #[test]
    fn a_pan_that_ended_leaves_nothing_behind_for_the_next() {
        let steps = run(&[
            (false, false, false, (50, 60)),
            (false, false, false, (50, 70)),
            (false, false, true, (50, 75)),
            (true, false, false, (50, -10)),
            (false, false, false, (50, -15)),
        ]);
        assert_eq!(
            steps,
            vec![
                opened_at(-250, -140),
                moved(0, 10),
                moved(0, 5),
                opened_at(-250, -210),
                moved(0, -5),
            ]
        );
    }
}
