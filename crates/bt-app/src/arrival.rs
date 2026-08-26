//! **How a layer arrives, and how it leaves** — one register for every popup,
//! menu, submenu, dialog and strip this window raises.
//!
//! # Why it is one register and not one field per popup
//!
//! The 2026-08-25 audits found eight menus, a dialog and a notice strip all
//! appearing in a single frame, and the obvious repair is to give each of the
//! ten a pair of `Instant`s. That repair is ten state machines, ten chances to
//! forget the reduced-motion branch, and — the part that actually bites — ten
//! answers to the question a departure asks: *what do you draw for a menu whose
//! state you have just thrown away?*
//!
//! Because that is the shape of the problem. A popup in this window is a
//! `Some(state)` that every hit test, every key ladder rung and every layout
//! reads; `close_popup` sets it to `None` and the menu is gone from all of them
//! at once, which is exactly right — **a menu that is leaving must not be
//! pressable**. A fade-out cannot be a state that lingers, or the lingering
//! state is a menu you can still click. What may linger is the *picture*.
//!
//! So this register keeps pictures. Each frame a band of the overlay is handed
//! to [`Passages::stage`]; while the band has layers in it they are remembered,
//! and on the frame it comes back empty the remembered ones are handed back,
//! fading, until the fast span is up. The popup's own state died on the frame it
//! was closed, so nothing can be pressed, hovered, focused or scrolled through
//! the ghost — there is nothing there but ink.
//!
//! It also means an entrance costs no opener a line: a band that was empty and
//! is not is a surface that has just arrived, whoever raised it and by whatever
//! door. The eight popups, their two submenus, the settings scrim, the dialog on
//! it and the notice strip all come through one piece of code, and a ninth popup
//! joins them by being drawn.
//!
//! # The two halves are not symmetrical
//!
//! An entrance is [`bt_render::POPUP_ENTER`] (the base span) on
//! [`bt_render::GRAB_EASE`], with [`bt_render::MOTION_TRAVEL_LOGICAL_PX`] of
//! travel from the direction of the control that raised it. A departure is
//! [`bt_render::POPUP_EXIT`] (the fast span) on [`bt_render::EASE`], and it does
//! not travel at all — see the archive for both arguments.
//!
//! # Reduced motion
//!
//! Under [`Motion::Reduced`] this register holds nothing and answers nothing: a
//! band arrives at full strength in its final place on its first frame, and a
//! band that goes away is simply gone. No entry is kept, so
//! [`Passages::moving`] is `false` and the window asks for no frames at all —
//! and because what is being suppressed is only ever a fade and four pixels, the
//! menu, the dialog and the strip are the same menu, dialog and strip they were,
//! in the same place, with the same border and the same shadow.

use std::time::Instant;

use bt_render::{EASE, GRAB_EASE, POPUP_ENTER, POPUP_EXIT, Travel};

use crate::marks::OverlayLayer;
use crate::{Motion, cubic_bezier};

/// Which half of its life on the glass a surface is in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Half {
    /// Coming: opacity and travel, on the base span.
    Arriving,
    /// **Here, and none of this register's doing** — the state a surface handed
    /// through [`Passages::stage_departure`] sits in for as long as it is up.
    ///
    /// It owes no frames and moves nothing; the only reason a register entry
    /// exists at all is that a departure needs a picture to fade, and the
    /// picture has to have been kept while the surface was still real.
    Standing,
    /// Going: opacity alone, on the fast span, from a picture nothing can press.
    Leaving,
}

/// How solid a passing layer is drawn this frame, and how far it still has to
/// travel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Passing {
    /// `0.0 ..= 1.0`, multiplied into every layer's own opacity.
    pub opacity: f32,
    /// Physical pixels the whole band is displaced by, `[dx, dy]`.
    pub offset: [f32; 2],
    /// Whether another frame is owed.
    pub moving: bool,
}

/// One surface's passage.
#[derive(Clone, Debug)]
struct Passage {
    half: Half,
    started: Instant,
    /// **Which way it grew, or `None` for a layer that did not grow out of
    /// anything.**
    ///
    /// Read at the entrance and kept, because a departure must not consult it
    /// (nothing travels on the way out) and a re-entrance takes a fresh reading
    /// anyway.
    ///
    /// `None` is one surface in this window and it is the settings scrim: the
    /// dimming is not a layer that came from somewhere, it is the window itself
    /// going dark, and a scrim that slid four pixels would be the whole window
    /// sliding under the dialog standing on it.
    travel: Option<Travel>,
    /// **The picture, as it was last painted at full strength.**
    ///
    /// Untransformed: the fade and the travel are applied on the way out of
    /// [`Passages::stage`], so what is kept is the band itself and a departure
    /// starting mid-entrance does not inherit a four-pixel displacement it would
    /// then never work off.
    ghost: Vec<OverlayLayer>,
    /// How solid it was on the frame it began to leave.
    ///
    /// A menu dismissed 40ms into its own entrance is 40ms solid, and fading it
    /// from a full 1.0 would be a flash on the way out — the same reversal rule
    /// [`crate::RevealTween::retarget`] keeps, for the same reason.
    left_at: f32,
}

impl Passage {
    /// Where this passage is now, with the travel already in physical pixels.
    ///
    /// Reduced motion never reaches here: [`Passages::stage`] retires every entry
    /// rather than sampling one, so there is no branch on it to forget.
    fn sample(&self, now: Instant, scale: f32) -> Passing {
        let (span, curve) = match self.half {
            Half::Arriving => (POPUP_ENTER, GRAB_EASE),
            Half::Leaving => (POPUP_EXIT, EASE),
            Half::Standing => {
                return Passing {
                    opacity: 1.0,
                    offset: [0.0, 0.0],
                    moving: false,
                };
            }
        };
        let elapsed = now.saturating_duration_since(self.started);
        let progress = (elapsed.as_secs_f32() / span.as_secs_f32()).clamp(0.0, 1.0);
        let eased = cubic_bezier(progress, curve);
        let moving = progress < 1.0;
        match self.half {
            Half::Arriving => Passing {
                opacity: eased,
                offset: self
                    .travel
                    .map_or([0.0, 0.0], |travel| travel.offset(1.0 - eased, scale)),
                moving,
            },
            Half::Leaving | Half::Standing => Passing {
                opacity: self.left_at * (1.0 - eased),
                offset: [0.0, 0.0],
                moving,
            },
        }
    }
}

/// **Every surface currently arriving or leaving**, keyed by whatever the window
/// calls its bands.
///
/// Generic over the key so that the list of surfaces stays in `main.rs`, where
/// the popups are: this file owns the *rhythm*, and which bands there are is a
/// fact about the window.
///
/// A `Vec` and not a map because the whole of it is at most three entries — one
/// popup is up at a time (E61), a submenu hangs off it, and the dialog and its
/// scrim pass together — and a three-entry linear scan is cheaper than hashing
/// even once.
#[derive(Clone, Debug)]
pub struct Passages<K> {
    entries: Vec<(K, Passage)>,
}

impl<K> Default for Passages<K> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<K: Copy + PartialEq> Passages<K> {
    /// **Put one band of the overlay through its passage**, and answer with the
    /// layers to draw.
    ///
    /// `layers` is the band as its builders left it — empty when the surface is
    /// not there. `travel` is which way it grew, and is consulted only on the
    /// frame an entrance begins.
    ///
    /// Three answers and no fourth:
    ///
    /// * layers, and the surface was already here — remember them, and draw them
    ///   through whatever is left of the entrance.
    /// * layers, and it was not — the entrance begins now.
    /// * none, and it was here — the departure begins now (or continues), and
    ///   what is drawn is the picture kept from the last frame it was real.
    #[must_use]
    pub fn stage(
        &mut self,
        key: K,
        layers: Vec<OverlayLayer>,
        travel: Option<Travel>,
        now: Instant,
        motion: Motion,
        scale: f32,
    ) -> Vec<OverlayLayer> {
        if motion == Motion::Reduced {
            // Nothing is kept and nothing is owed: the band is what it is, on
            // the first frame, in its final place.
            self.entries.retain(|(held, _)| *held != key);
            return layers;
        }
        if layers.is_empty() {
            return self.depart(key, now, scale);
        }
        self.arrive(key, layers, travel, now, scale)
    }

    /// **The departure alone** — for a surface whose entrance is its own.
    ///
    /// The tip and the key hint each fade themselves in over the fast span and
    /// have done since they were written; what they never had is the other half.
    /// Handing their bands through here gives them a departure without taking
    /// away the entrance they already had, and without briefly running two fades
    /// over one card.
    #[must_use]
    pub fn stage_departure(
        &mut self,
        key: K,
        layers: Vec<OverlayLayer>,
        now: Instant,
        motion: Motion,
    ) -> Vec<OverlayLayer> {
        if motion == Motion::Reduced {
            self.entries.retain(|(held, _)| *held != key);
            return layers;
        }
        if layers.is_empty() {
            return self.depart(key, now, 1.0);
        }
        // **Remembered exactly as it drew itself**, which is what makes "leaves
        // from where it had got to" fall out rather than be arranged: the layers
        // carry the surface's own fade in their own opacity, and a departure
        // multiplies whatever that was down to nothing. The direction is the one
        // no departure ever consults — this surface has no entrance of this
        // register's to travel on.
        self.remember(key, &layers, None, Half::Standing, now);
        layers
    }

    /// Begin or continue an entrance.
    fn arrive(
        &mut self,
        key: K,
        layers: Vec<OverlayLayer>,
        travel: Option<Travel>,
        now: Instant,
        scale: f32,
    ) -> Vec<OverlayLayer> {
        let started = match self.find(key) {
            // Already coming: the clock it started on, not this frame's.
            Some(passage) if passage.half == Half::Arriving => passage.started,
            // Coming back before it had finished going — the entrance starts
            // over rather than reversing, because what is on the glass is a
            // picture of a menu that is no longer open and the one arriving may
            // be a different menu entirely.
            _ => now,
        };
        self.remember(key, &layers, travel, Half::Arriving, started);
        let passing = self
            .find(key)
            .expect("the entry was just written")
            .sample(now, scale);
        passed(layers, passing)
    }

    /// Begin or continue a departure, and answer with what is left of the
    /// picture.
    fn depart(&mut self, key: K, now: Instant, scale: f32) -> Vec<OverlayLayer> {
        let Some(index) = self.entries.iter().position(|(held, _)| *held == key) else {
            return Vec::new();
        };
        if self.entries[index].1.half != Half::Leaving {
            let left_at = self.entries[index].1.sample(now, scale).opacity;
            let passage = &mut self.entries[index].1;
            passage.half = Half::Leaving;
            passage.started = now;
            passage.left_at = left_at;
        }
        let passing = self.entries[index].1.sample(now, scale);
        if !passing.moving {
            // The picture is spent. Dropping the entry here rather than leaving
            // a transparent one is what makes [`Passages::moving`] fall silent
            // and the window go genuinely idle.
            self.entries.remove(index);
            return Vec::new();
        }
        // Never travels — a layer that is ending is not going anywhere.
        passed(self.entries[index].1.ghost.clone(), passing)
    }

    /// Write down the picture and the clock this passage runs on.
    fn remember(
        &mut self,
        key: K,
        layers: &[OverlayLayer],
        travel: Option<Travel>,
        half: Half,
        started: Instant,
    ) {
        let passage = Passage {
            half,
            started,
            travel,
            ghost: layers.to_vec(),
            // Full strength until a departure says otherwise: an entrance is
            // measured from nothing, and a standing picture is already drawn at
            // whatever strength its own fade left in its layers.
            left_at: 1.0,
        };
        match self.entries.iter_mut().find(|(held, _)| *held == key) {
            Some((_, held)) => *held = passage,
            None => self.entries.push((key, passage)),
        }
    }

    fn find(&self, key: K) -> Option<&Passage> {
        self.entries
            .iter()
            .find(|(held, _)| *held == key)
            .map(|(_, passage)| passage)
    }

    /// Whether anything here still owes a frame.
    ///
    /// `false` for a window under reduced motion however many popups it has
    /// raised, because nothing was ever written down.
    #[must_use]
    pub fn moving(&self, now: Instant, motion: Motion) -> bool {
        motion == Motion::Full
            && self
                .entries
                .iter()
                .any(|(_, passage)| passage.sample(now, 1.0).moving)
    }

    /// **What would be drawn this frame, quantised to what can reach the glass**
    /// — the frame-debt reading, on the pin's and the rail's own terms.
    ///
    /// The opacity in the 1/255 a layer's alpha resolves to and the offset in
    /// whole physical pixels, so a curve settling through its long tail stops
    /// owing presents once the picture has stopped changing.
    #[must_use]
    pub fn drawn(&self, now: Instant, motion: Motion, scale: f32) -> Vec<(K, u8, i32, i32)> {
        if motion == Motion::Reduced {
            return Vec::new();
        }
        self.entries
            .iter()
            .map(|(key, passage)| {
                let passing = passage.sample(now, scale);
                (
                    *key,
                    (passing.opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
                    passing.offset[0].round() as i32,
                    passing.offset[1].round() as i32,
                )
            })
            .collect()
    }
}

/// Apply one passage to a band: fade every layer, and move the whole of it.
///
/// The whole of it, in one place, because a band that faded and travelled in
/// pieces would be a menu whose shadow arrived before its face.
#[must_use]
fn passed(layers: Vec<OverlayLayer>, passing: Passing) -> Vec<OverlayLayer> {
    let [dx, dy] = passing.offset;
    layers
        .into_iter()
        .map(|layer| nudged(layer, passing.opacity, dx, dy))
        .collect()
}

/// One layer, faded and moved.
///
/// Destructured rather than mutated field by field so that a field added to
/// [`OverlayLayer`] fails to compile here: the day a band carries a new kind of
/// geometry is the day this function has to say whether it moves with the rest,
/// and a `..` would answer "no" silently.
fn nudged(layer: OverlayLayer, opacity: f32, dx: f32, dy: f32) -> OverlayLayer {
    let OverlayLayer {
        mut grounds,
        mut quads,
        mut labels,
        mut sprites,
        opacity: own,
        // A scrolled document, and no band that passes through here has one:
        // the preview float is its only tenant and a float runs its own
        // entrance (`float::fade`). Left alone rather than moved, so that the
        // day one does arrive here the picture is wrong in a way a reader can
        // see rather than subtly stale.
        body,
        mut images,
    } = layer;
    let shift = |rect: &mut [f32; 4]| {
        rect[0] += dx;
        rect[1] += dy;
        rect[2] += dx;
        rect[3] += dy;
    };
    for ground in &mut grounds {
        shift(&mut ground.rect);
    }
    for quad in &mut quads {
        shift(&mut quad.rect);
    }
    for label in &mut labels {
        shift(&mut label.rect);
        if let Some(clip) = label.clip.as_mut() {
            shift(clip);
        }
    }
    for sprite in &mut sprites {
        shift(&mut sprite.rect);
    }
    for image in &mut images {
        shift(&mut image.rect);
        if let Some(clip) = image.clip.as_mut() {
            shift(clip);
        }
    }
    OverlayLayer {
        grounds,
        quads,
        labels,
        sprites,
        opacity: own * opacity,
        body,
        images,
    }
}

#[cfg(test)]
mod tests {
    use bt_render::OverlayQuad;

    use super::*;

    /// One band with one quad at a known place, so a shift and a fade are both
    /// readable off the answer.
    fn band(left: f32, top: f32) -> Vec<OverlayLayer> {
        vec![OverlayLayer {
            quads: vec![OverlayQuad {
                rect: [left, top, left + 100.0, top + 40.0],
                color: [0, 0, 0],
                alpha: 1.0,
            }],
            ..OverlayLayer::default()
        }]
    }

    fn only_rect(layers: &[OverlayLayer]) -> [f32; 4] {
        layers[0].quads[0].rect
    }

    /// RED — **a popup arrives over the base span, from four pixels toward the
    /// thing that raised it**, and it is not simply there on the first frame.
    ///
    /// Mutation: return the layers untouched from `stage` and the first frame is
    /// a solid menu in its final place — the hard cut both audits filed as the
    /// window's largest rhythm gap.
    #[test]
    fn a_band_that_was_not_there_arrives_over_the_base_span_and_travels_four_pixels() {
        let mut passages = Passages::<u8>::default();
        let now = Instant::now();
        let first = passages.stage(
            1,
            band(200.0, 100.0),
            Some(Travel::Down),
            now,
            Motion::Full,
            1.0,
        );
        assert!(
            first[0].opacity < 0.01,
            "a menu that is fully solid on its first frame did not arrive, it appeared"
        );
        assert_eq!(
            only_rect(&first),
            [200.0, 96.0, 300.0, 136.0],
            "it has to start four pixels up, against the control it dropped out of"
        );

        // Half way along the curve it is neither, and still owes frames.
        let middle = passages.stage(
            1,
            band(200.0, 100.0),
            Some(Travel::Down),
            now + POPUP_ENTER / 2,
            Motion::Full,
            1.0,
        );
        assert!(middle[0].opacity > 0.5 && middle[0].opacity < 1.0);
        assert!(passages.moving(now + POPUP_ENTER / 2, Motion::Full));

        // And at the end it is exactly the band its builder handed over.
        let landed = passages.stage(
            1,
            band(200.0, 100.0),
            Some(Travel::Down),
            now + POPUP_ENTER,
            Motion::Full,
            1.0,
        );
        assert_eq!(landed[0].opacity, 1.0);
        assert_eq!(only_rect(&landed), [200.0, 100.0, 300.0, 140.0]);
        assert!(!passages.moving(now + POPUP_ENTER, Motion::Full));
    }

    /// RED — **a band with no direction deepens and does not move**, which is
    /// the settings scrim's whole rule.
    ///
    /// Mutation: give the scrim a direction and the dimming travels four pixels
    /// with the dialog standing on it, which is the whole window appearing to
    /// slide. Caught on the glass exactly that way (`BT_CHROME_DUMP`,
    /// 2026-08-26): `op=0.275 top=-5.8` on a quad spanning the window.
    #[test]
    fn a_band_that_grew_out_of_nothing_deepens_without_travelling() {
        let mut passages = Passages::<u8>::default();
        let now = Instant::now();
        let first = passages.stage(1, band(0.0, 0.0), None, now, Motion::Full, 2.0);
        assert!(
            first[0].opacity < 0.01,
            "it still fades in over the base span"
        );
        assert_eq!(
            only_rect(&first),
            [0.0, 0.0, 100.0, 40.0],
            "and it is exactly where its builder put it, on the first frame"
        );
        let half = passages.stage(
            1,
            band(0.0, 0.0),
            None,
            now + POPUP_ENTER / 2,
            Motion::Full,
            2.0,
        );
        assert_eq!(only_rect(&half), [0.0, 0.0, 100.0, 40.0]);
        assert!(half[0].opacity > 0.5 && half[0].opacity < 1.0);
    }

    /// RED — **the picture outlives the state, and only the picture.**
    ///
    /// The band handed in is empty from the instant the popup closed — that is
    /// what a closed popup is — and what comes back is the last thing it drew,
    /// fading, in the place it stood. Nothing travels.
    ///
    /// Mutation: keep the popup's state alive for the fade instead and the menu
    /// is pressable while it disappears.
    #[test]
    fn a_band_that_has_gone_leaves_its_picture_behind_for_the_fast_span_only() {
        let mut passages = Passages::<u8>::default();
        let now = Instant::now();
        let _ = passages.stage(
            1,
            band(200.0, 100.0),
            Some(Travel::Down),
            now,
            Motion::Full,
            1.0,
        );
        let landed = now + POPUP_ENTER;
        let _ = passages.stage(
            1,
            band(200.0, 100.0),
            Some(Travel::Down),
            landed,
            Motion::Full,
            1.0,
        );

        // The popup is closed: its builders have nothing to hand over.
        let going = passages.stage(1, Vec::new(), Some(Travel::Down), landed, Motion::Full, 1.0);
        assert_eq!(going.len(), 1, "the picture it drew is still on the glass");
        assert_eq!(
            only_rect(&going),
            [200.0, 100.0, 300.0, 140.0],
            "and it is where it stood: nothing travels on the way out"
        );

        let half = landed + POPUP_EXIT / 2;
        let fading = passages.stage(1, Vec::new(), Some(Travel::Down), half, Motion::Full, 1.0);
        assert!(fading[0].opacity > 0.0 && fading[0].opacity < 1.0);
        assert_eq!(only_rect(&fading), [200.0, 100.0, 300.0, 140.0]);

        // And when the fast span is up it is gone, and owes nothing.
        let after = landed + POPUP_EXIT;
        let gone = passages.stage(1, Vec::new(), Some(Travel::Down), after, Motion::Full, 1.0);
        assert!(gone.is_empty());
        assert!(
            !passages.moving(after, Motion::Full),
            "a register with nothing left in it must let the window go idle"
        );
    }

    /// RED — a popup dismissed **during** its own entrance fades from where it
    /// had got to, not from a solidity it never reached.
    #[test]
    fn a_popup_dismissed_mid_entrance_leaves_from_where_it_was() {
        let mut passages = Passages::<u8>::default();
        let now = Instant::now();
        let _ = passages.stage(
            1,
            band(0.0, 0.0),
            Some(Travel::Down),
            now,
            Motion::Full,
            1.0,
        );
        let third = now + POPUP_ENTER / 3;
        let partway = passages.stage(
            1,
            band(0.0, 0.0),
            Some(Travel::Down),
            third,
            Motion::Full,
            1.0,
        );
        let reached = partway[0].opacity;
        assert!(reached > 0.0 && reached < 1.0);

        let leaving = passages.stage(1, Vec::new(), Some(Travel::Down), third, Motion::Full, 1.0);
        assert!(
            (leaving[0].opacity - reached).abs() < 1e-3,
            "the departure has to begin at {reached}, not at a full 1.0 it never had"
        );
    }

    /// RED — **reduced motion asks for no frames at all**, and loses nothing.
    ///
    /// The band arrives whole, in its final place, on its first frame; it goes
    /// away on the frame it is closed; and the register holds nothing that could
    /// wake the loop. This is the red line the whole animation block is written
    /// under.
    ///
    /// Mutation: sample the curve under `Reduced` and the window animates for
    /// somebody who asked it not to, and — worse — keeps asking to be woken.
    #[test]
    fn reduced_motion_takes_the_band_whole_and_asks_for_nothing() {
        let mut passages = Passages::<u8>::default();
        let now = Instant::now();
        let first = passages.stage(
            1,
            band(200.0, 100.0),
            Some(Travel::Down),
            now,
            Motion::Reduced,
            1.0,
        );
        assert_eq!(first[0].opacity, 1.0, "it is simply there");
        assert_eq!(
            only_rect(&first),
            [200.0, 100.0, 300.0, 140.0],
            "and there is where it belongs — no four pixels to work off"
        );
        assert!(!passages.moving(now, Motion::Reduced));
        assert!(passages.drawn(now, Motion::Reduced, 1.0).is_empty());

        // And it goes the instant it is closed, leaving nothing behind.
        let gone = passages.stage(1, Vec::new(), Some(Travel::Down), now, Motion::Reduced, 1.0);
        assert!(gone.is_empty());
        assert!(!passages.moving(now, Motion::Reduced));
    }

    /// RED — a surface handed through [`Passages::stage_departure`] keeps the
    /// entrance it drew for itself and gains only the way out.
    ///
    /// Mutation: send the tip through `stage` instead and it fades in twice —
    /// its own 90ms curve multiplied by this register's 140.
    #[test]
    fn a_departure_only_band_is_handed_back_exactly_as_its_own_fade_left_it() {
        let mut passages = Passages::<u8>::default();
        let now = Instant::now();
        let mut half_faded = band(10.0, 10.0);
        half_faded[0].opacity = 0.4;
        let shown = passages.stage_departure(7, half_faded, now, Motion::Full);
        assert_eq!(shown[0].opacity, 0.4, "the tip's own fade is untouched");
        assert_eq!(only_rect(&shown), [10.0, 10.0, 110.0, 50.0]);

        // Taken down: it leaves from the 0.4 it had reached.
        let leaving = passages.stage_departure(7, Vec::new(), now, Motion::Full);
        assert!((leaving[0].opacity - 0.4).abs() < 1e-3);
        let gone = passages.stage_departure(7, Vec::new(), now + POPUP_EXIT, Motion::Full);
        assert!(gone.is_empty());
    }

    /// RED — two bands pass independently: a submenu opening does not restart
    /// the menu it hangs off.
    #[test]
    fn each_key_keeps_its_own_clock() {
        let mut passages = Passages::<u8>::default();
        let now = Instant::now();
        let _ = passages.stage(
            1,
            band(0.0, 0.0),
            Some(Travel::Down),
            now,
            Motion::Full,
            1.0,
        );
        let later = now + POPUP_ENTER;
        let parent = passages.stage(
            1,
            band(0.0, 0.0),
            Some(Travel::Down),
            later,
            Motion::Full,
            1.0,
        );
        assert_eq!(parent[0].opacity, 1.0, "the parent has long since landed");
        let child = passages.stage(
            2,
            band(100.0, 0.0),
            Some(Travel::Right),
            later,
            Motion::Full,
            1.0,
        );
        assert!(child[0].opacity < 0.01, "the child is only now arriving");
        assert_eq!(
            only_rect(&child),
            [96.0, 0.0, 196.0, 40.0],
            "and it comes out of its parent's row, from the left"
        );
    }

    /// RED — the frame-debt reading falls silent once the picture has stopped
    /// changing, and says nothing at all under reduced motion.
    #[test]
    fn the_drawn_reading_is_quantised_and_empties_when_the_passage_ends() {
        let mut passages = Passages::<u8>::default();
        let now = Instant::now();
        let _ = passages.stage(
            1,
            band(0.0, 0.0),
            Some(Travel::Down),
            now,
            Motion::Full,
            1.0,
        );
        let opening = passages.drawn(now, Motion::Full, 1.0);
        assert_eq!(opening, vec![(1, 0, 0, -4)]);
        let _ = passages.stage(
            1,
            band(0.0, 0.0),
            Some(Travel::Down),
            now + POPUP_ENTER,
            Motion::Full,
            1.0,
        );
        assert_eq!(
            passages.drawn(now + POPUP_ENTER, Motion::Full, 1.0),
            vec![(1, 255, 0, 0)]
        );
    }
}
