//! **What is not at home, and how far it still has to come back** — one
//! register for every fade this window runs on a surface it did not raise.
//!
//! # Why there is a second register beside [`crate::arrival`]
//!
//! `arrival` answers a question about *pictures*: a menu's state dies on the
//! frame it closes, so what fades has to be the ink it left behind. Nothing in
//! here is like that. A pane head's `×`, a Git row waiting on a write, the tab
//! that has just become the active one, the ghost under a pointer that has just
//! begun to carry something — every one of these is a thing that **stays**, and
//! what changes is one number about it: how solid, how dim, how far open.
//!
//! Those are transitions in the plainest sense, and [`crate::RevealTween`] has
//! drawn them since the rail first slid: aim at a target, keep the current
//! position as the new origin so a reversal turns around from where it actually
//! is, report the target and no frames at all under [`Motion::Reduced`]. What
//! the window did not have was anywhere to *keep* one per thing. There is one
//! rail and one chevron, so those tweens are fields; there are as many head runs
//! as there are panes and as many dimmed rows as a repository has files, and a
//! field cannot be one per row.
//!
//! # Home, and the rule that keeps this register small
//!
//! Every value in here has a **home**: the number it wears when nothing is
//! happening to it. A head run's home is invisible, a Git row's is full
//! strength, a disclosure's is shut. [`Settling::settle`] is told the home and
//! the target on every frame, and it keeps an entry **only while the two
//! differ** — an entry that has finished coming home is dropped on the frame it
//! arrives.
//!
//! That is not tidiness, it is what makes the register correct without a sweep.
//! A key that goes away — a pane torn out, a file that stopped being modified, a
//! settings page nobody is on — simply stops being asked about, and the entry it
//! left behind is either still on its way home (in which case it is still being
//! drawn, and is still asked about) or already gone. There is no generation
//! counter to bump and no stale entry to reap, and the register of a window
//! sitting still is empty.
//!
//! # What a reader must not take from this
//!
//! **The fade is never the state.** Every surface that reads a number out of
//! here has a *static* answer for the same question — the run is on screen or it
//! is not, the row is dim or it is not, the group is open or it is not — and the
//! number is a gain over that answer, never the answer itself. Under
//! [`Motion::Reduced`] this register holds nothing, answers the target on the
//! first frame and asks for no frames at all, and every one of those surfaces is
//! exactly the surface it was: same box, same ink, same border.
//!
//! **And the fade is never the hit test.** A control is pressable from the frame
//! its reveal *begins*, not from the frame the reveal finishes — see
//! [`bt_render::HOVER_CHROME_FADE`], which is where that rule is written down.
//! Nothing in this file is reachable from a hit test, and
//! `only_the_paint_and_the_frame_clock_can_read_the_settling_register` in
//! `main.rs` is what keeps it that way.

use std::time::{Duration, Instant};

use crate::{Motion, RevealTween};

/// **Where a value is being pulled to, and on what curve.**
///
/// The target rides with the span and the curve rather than being passed beside
/// them, because the three are one decision: "this control's ink, arriving" is a
/// value *and* ninety milliseconds *and* the house ease, and a call site free to
/// pair one surface's target with another's span is a call site free to invent a
/// fourth rhythm one argument at a time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Toward {
    /// The number the value belongs at this frame.
    pub value: f32,
    /// How long it takes to get there.
    pub span: Duration,
    /// The curve it travels on.
    pub curve: [f32; 4],
}

impl Toward {
    /// A pull on the house `EASE`, which is every fade this register holds:
    /// they are all ink and colour, and `EASE` is what this window draws those
    /// with. The one transition in the second slice that is *not* ink — the
    /// settings page's Advanced group, which moves rows — is a field on the
    /// window rather than an entry here, and it takes `GRAB_EASE` at its own
    /// call site because a hand has just let go of it.
    #[must_use]
    pub fn eased(value: f32, span: Duration) -> Self {
        Self {
            value,
            span,
            curve: bt_render::EASE,
        }
    }
}

/// **Every value in this window that is away from home**, keyed by whatever the
/// surface that owns it calls its parts.
///
/// Generic over the key for [`crate::arrival::Passages`]' reason: this file owns
/// the rhythm, and *what there is* — which seats, which rows, which pages — is a
/// fact about the window and stays in `main.rs`.
///
/// A `Vec` and not a map because of the home rule above: the entries are the
/// things that are not at rest, and a window has single digits of those. A hand
/// crossing three panes in ninety milliseconds leaves three, and they leave by
/// themselves.
#[derive(Clone, Debug)]
pub struct Settling<K> {
    entries: Vec<(K, RevealTween)>,
}

impl<K> Default for Settling<K> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<K: Clone + Eq> Settling<K> {
    /// **Where `key` stands this frame**, having been told where home is and
    /// where it belongs.
    ///
    /// Called once per frame per key, by the code that is about to draw with the
    /// answer. Retargeting is idempotent — [`RevealTween::retarget`] returns at
    /// once when the target has not moved — so a frame that changes nothing
    /// costs one comparison, and a frame that reverses a fade mid-flight turns it
    /// around from where it actually is.
    ///
    /// Under [`Motion::Reduced`] the entry is dropped and the target is the
    /// answer: first frame, final value, no frames owed.
    pub fn settle(
        &mut self,
        key: &K,
        home: f32,
        toward: Toward,
        now: Instant,
        motion: Motion,
    ) -> f32 {
        if motion == Motion::Reduced {
            self.entries.retain(|(held, _)| held != key);
            return toward.value;
        }
        let Some(index) = self.entries.iter().position(|(held, _)| held == key) else {
            // Nothing to remember about a value that is where it belongs and
            // where it rests. This is the other half of the home rule: without
            // it every key ever asked about would earn a permanent entry saying
            // "still nothing".
            if toward.value == home {
                return home;
            }
            let mut tween = RevealTween::resting_on(home, toward.span, toward.curve);
            tween.retarget(toward.value, now, motion);
            let value = tween.sample(now, motion).0;
            self.entries.push((key.clone(), tween));
            return value;
        };
        let tween = &mut self.entries[index].1;
        tween.retarget(toward.value, now, motion);
        let (value, moving) = tween.sample(now, motion);
        if !moving && value == home {
            self.entries.remove(index);
        }
        value
    }

    /// **Every key this register is still holding something for**, so a caller
    /// can ask about the ones its own state has stopped mentioning.
    ///
    /// The hand leaves a pane and the pane stops being hovered; something still
    /// has to say "and that run is on its way out" for the ninety milliseconds it
    /// takes. This is how: the caller walks what it knows *plus* this, and pulls
    /// everything it no longer knows about back home.
    #[must_use]
    pub fn held(&self) -> Vec<K> {
        self.entries.iter().map(|(key, _)| key.clone()).collect()
    }

    /// **Drop what is left of a key whose surface has gone**, without easing it
    /// anywhere.
    ///
    /// The one case [`Self::settle`]'s home rule cannot reach: a value can only
    /// come home by being asked about, and a Git row that has been staged out of
    /// its own group is a row nobody will ask about again. Easing it home would
    /// be ninety milliseconds of frames spent on a fade with nothing on the glass
    /// to fade; this is the honest answer — the row is not dim, the row is gone.
    ///
    /// Never for a surface that is still *drawn* on its way out. A pane head's
    /// run leaving is a run this window is still painting, and it comes home the
    /// ordinary way.
    pub fn forget(&mut self, key: &K) {
        self.entries.retain(|(held, _)| held != key);
    }

    /// Whether anything here still owes a frame.
    ///
    /// `false` under reduced motion however much has been settled through it,
    /// because nothing was ever written down.
    #[must_use]
    pub fn moving(&self, now: Instant, motion: Motion) -> bool {
        motion == Motion::Full
            && self
                .entries
                .iter()
                .any(|(_, tween)| tween.sample(now, motion).1)
    }

    /// **What would be drawn this frame, quantised to what can reach the
    /// glass** — the frame-debt reading, on `Passages::drawn`'s own terms.
    ///
    /// A thousandth, which is finer than any surface this register feeds can
    /// show: a colour resolves to 1/255, and the tallest thing it opens is a
    /// settings group under a thousand pixels high. Quantising *finer* than the
    /// glass is the safe direction — the window can owe a frame that turns out to
    /// draw the same picture, which costs one present, but it can never miss the
    /// frame on which the picture really did change, which is a fade that stops
    /// one step short and stays there.
    #[must_use]
    pub fn drawn(&self, now: Instant, motion: Motion) -> Vec<(K, u16)> {
        if motion == Motion::Reduced {
            return Vec::new();
        }
        self.entries
            .iter()
            .map(|(key, tween)| {
                let (value, _) = tween.sample(now, motion);
                (key.clone(), (value.clamp(0.0, 1.0) * 1000.0).round() as u16)
            })
            .collect()
    }
}

// ── one box, two phrases ───────────────────────────────────────────────────

/// **A strip that has changed its mind about what it says**, and the ninety
/// milliseconds it takes to say the other thing.
///
/// The window's feet each carry a receipt: `Revealed`, `Opened`, `Saved`,
/// `140%`. A receipt stands for [`crate::FOOT_REVEAL_FEEDBACK`] — a hold, and
/// this does not touch it — and then the strip goes back to the fact it stands
/// in front of, which used to happen between two frames with nothing in between.
///
/// # Why it dissolves through the ground rather than one phrase over the other
///
/// A [`bt_render::ChromeLabel`] carries a colour and no alpha: this pipeline
/// composites in linear light, so a translucent ink is mixed over the surface it
/// lands on before it is handed to the blender (see `bt_render::ink_over`, which
/// is what every pre-mixed palette entry in this product already is). Two
/// phrases drawn over one another at complementary strengths would therefore be
/// two sets of glyphs both mixed toward the same ground and both drawn — which
/// on a foot is a path and a receipt overlapping for a sixteenth of a second,
/// and reads as neither.
///
/// So the box holds one phrase at a time and the *ink* crosses: the outgoing
/// phrase thins to the ground over the first half, the incoming one thickens out
/// of it over the second. The swap happens at the midpoint, where nothing is
/// legible anyway, which is the whole reason a dissolve can be a swap at all.
///
/// # What it is not
///
/// It is not a second clock. The receipt's own hold, and the instant it expires,
/// are exactly the clocks the window already keeps; this is told *what the strip
/// wants to say* on every frame and notices when the answer changes. So there is
/// no way for the fade and the receipt to disagree about when the word comes
/// back, because there is only one of them that knows.
#[derive(Clone, Debug)]
pub struct Crossfade<K> {
    entries: Vec<(K, Saying)>,
}

/// What one box is saying, what it was saying, and when it changed its mind.
#[derive(Clone, Debug)]
struct Saying {
    showing: Option<String>,
    /// **What it was saying**, which is the whole of what an entry is for: after
    /// the dissolve has run out this is the only field that still matters, and
    /// it is the reason an entry cannot be dropped when it settles the way
    /// [`Settling`]'s are. A box with no entry has no memory of a phrase, and a
    /// change it cannot see is a change it cannot dissolve.
    previous: Option<String>,
    since: Instant,
}

impl<K> Default for Crossfade<K> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<K: Clone + Eq> Crossfade<K> {
    /// **What this box is saying this frame, and how solid.**
    ///
    /// `wanted` is what the window would put there with no transition at all —
    /// `Some` for a receipt, `None` for the standing fact underneath it. The
    /// answer is the same thing except during the ninety milliseconds after it
    /// changes, when it is briefly the *old* phrase on its way out.
    ///
    /// A box asked for the first time simply says what it was asked to: there is
    /// no phrase to dissolve out of, and a receipt that faded up out of nothing
    /// on the frame its pane was drawn would be a strip announcing itself.
    pub fn say(
        &mut self,
        key: &K,
        wanted: Option<&str>,
        now: Instant,
        motion: Motion,
    ) -> (Option<String>, f32) {
        let span = bt_render::FOOT_RECEIPT_CROSSFADE;
        let half = span / 2;
        let wanted_owned = wanted.map(str::to_owned);
        let Some(index) = self.entries.iter().position(|(held, _)| held == key) else {
            self.entries.push((
                key.clone(),
                Saying {
                    showing: wanted_owned.clone(),
                    previous: wanted_owned.clone(),
                    // Settled: `now - span` would underflow on a monotonic clock
                    // that has not run for ninety milliseconds yet, so the
                    // settled state is spelled as its own instant.
                    since: now,
                },
            ));
            return (wanted_owned, 1.0);
        };
        let saying = &mut self.entries[index].1;
        if saying.showing != wanted_owned {
            saying.previous = saying.showing.clone();
            saying.showing = wanted_owned;
            saying.since = now;
        }
        if motion == Motion::Reduced {
            // The word changes on the frame it changes, and nothing is owed. The
            // entry stays, because what it holds is a memory and not a fade.
            saying.previous = saying.showing.clone();
            return (saying.showing.clone(), 1.0);
        }
        let elapsed = now.saturating_duration_since(saying.since);
        if elapsed >= span || saying.previous == saying.showing {
            return (saying.showing.clone(), 1.0);
        }
        if elapsed < half {
            let left = 1.0 - elapsed.as_secs_f32() / half.as_secs_f32();
            return (saying.previous.clone(), left);
        }
        let arrived = (elapsed - half).as_secs_f32() / half.as_secs_f32();
        (saying.showing.clone(), arrived)
    }

    /// Whether any box here is still mid-dissolve.
    #[must_use]
    pub fn moving(&self, now: Instant, motion: Motion) -> bool {
        motion == Motion::Full
            && self.entries.iter().any(|(_, saying)| {
                saying.previous != saying.showing
                    && now.saturating_duration_since(saying.since)
                        < bt_render::FOOT_RECEIPT_CROSSFADE
            })
    }

    /// The frame-debt reading — [`Settling::drawn`]'s, on the phrases.
    #[must_use]
    pub fn drawn(&self, now: Instant, motion: Motion) -> Vec<(K, u16)> {
        if motion == Motion::Reduced {
            return Vec::new();
        }
        let span = bt_render::FOOT_RECEIPT_CROSSFADE;
        self.entries
            .iter()
            .filter(|(_, saying)| {
                saying.previous != saying.showing
                    && now.saturating_duration_since(saying.since) < span
            })
            .map(|(key, saying)| {
                let elapsed = now.saturating_duration_since(saying.since);
                let progress = elapsed.as_secs_f32() / span.as_secs_f32();
                (key.clone(), (progress * 1000.0).round() as u16)
            })
            .collect()
    }

    /// **Drop the boxes that are not there any more** — a column torn out, a
    /// float shut.
    ///
    /// An entry is a *memory* rather than a fade, so it cannot retire itself the
    /// way [`Settling`]'s entries do; the window sweeps it exactly as it sweeps
    /// the command rails of seats that have gone.
    pub fn retain(&mut self, keep: impl Fn(&K) -> bool) {
        self.entries.retain(|(key, _)| keep(key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAST: Duration = bt_render::HOVER_CHROME_FADE;

    fn ink(value: f32) -> Toward {
        Toward::eased(value, FAST)
    }

    /// RED — **a value that is asked to leave home eases there over the span,
    /// and is not simply there on the first frame.**
    ///
    /// Mutation: return `toward.value` from `settle` and every hover-revealed
    /// control in this window snaps back to the hard cut both audits filed.
    #[test]
    fn a_value_pulled_away_from_home_arrives_over_the_span_and_not_in_one_frame() {
        let mut settling = Settling::<u8>::default();
        let now = Instant::now();
        let first = settling.settle(&1, 0.0, ink(1.0), now, Motion::Full);
        assert!(
            first < 0.01,
            "a control fully solid on its first frame did not appear, it was already there"
        );
        let middle = settling.settle(&1, 0.0, ink(1.0), now + FAST / 2, Motion::Full);
        assert!(middle > 0.0 && middle < 1.0, "{middle}");
        assert!(settling.moving(now + FAST / 2, Motion::Full));
        let landed = settling.settle(&1, 0.0, ink(1.0), now + FAST, Motion::Full);
        assert_eq!(landed, 1.0);
        assert!(!settling.moving(now + FAST, Motion::Full));
    }

    /// RED — **the register keeps only what is away from home**, which is what
    /// makes it need no sweep.
    ///
    /// Mutation: keep the entry after it lands home and a window that has been
    /// pointed at every pane it has carries one entry per pane for the rest of
    /// the session — and `moving` walks all of them on every frame for ever.
    #[test]
    fn an_entry_that_has_come_all_the_way_home_stops_being_an_entry() {
        let mut settling = Settling::<u8>::default();
        let now = Instant::now();
        // Never asked away: nothing is written down at all.
        assert_eq!(settling.settle(&1, 0.0, ink(0.0), now, Motion::Full), 0.0);
        assert!(settling.held().is_empty());

        let _ = settling.settle(&1, 0.0, ink(1.0), now, Motion::Full);
        let up = now + FAST;
        let _ = settling.settle(&1, 0.0, ink(1.0), up, Motion::Full);
        assert_eq!(settling.held(), vec![1], "held while it is away from home");

        // Sent home: the entry lives exactly as long as the journey, which
        // begins on the frame it is told to turn round and not before.
        let _ = settling.settle(&1, 0.0, ink(0.0), up, Motion::Full);
        let midway = settling.settle(&1, 0.0, ink(0.0), up + FAST / 2, Motion::Full);
        assert!(midway > 0.0 && midway < 1.0, "{midway}");
        assert_eq!(settling.held(), vec![1]);
        assert_eq!(
            settling.settle(&1, 0.0, ink(0.0), up + FAST, Motion::Full),
            0.0
        );
        assert!(
            settling.held().is_empty(),
            "home is not a state worth keeping"
        );
        assert!(!settling.moving(up + FAST, Motion::Full));
    }

    /// RED — a reversal mid-flight **turns around from where it is**, and does
    /// not snap to an end it never reached.
    ///
    /// Mutation: restart from `home` on every retarget and a pointer brushed
    /// across a head makes the run flash to full before falling back.
    #[test]
    fn a_fade_reversed_halfway_carries_on_from_where_it_actually_got_to() {
        let mut settling = Settling::<u8>::default();
        let now = Instant::now();
        let _ = settling.settle(&1, 0.0, ink(1.0), now, Motion::Full);
        let third = now + FAST / 3;
        let reached = settling.settle(&1, 0.0, ink(1.0), third, Motion::Full);
        assert!(reached > 0.0 && reached < 1.0);
        let turning = settling.settle(&1, 0.0, ink(0.0), third, Motion::Full);
        assert!(
            (turning - reached).abs() < 1e-3,
            "the way back has to begin at {reached}, not at a 1.0 it never had"
        );
    }

    /// RED — a value whose home is **not** zero comes back to its own home.
    ///
    /// The Git row is the case: full strength is home and 0.45 is away, so this
    /// register runs one of its fades downward. Mutation: assume zero is home
    /// anywhere in `settle` and a row that finished staging fades to invisible.
    #[test]
    fn a_row_whose_home_is_full_strength_dims_away_and_comes_back_to_full() {
        let mut settling = Settling::<u8>::default();
        let now = Instant::now();
        let dimming = settling.settle(&1, 1.0, ink(0.45), now, Motion::Full);
        assert!(
            (dimming - 1.0).abs() < 0.01,
            "it starts from full: {dimming}"
        );
        let dim = settling.settle(&1, 1.0, ink(0.45), now + FAST, Motion::Full);
        assert!((dim - 0.45).abs() < 1e-6, "{dim}");
        assert_eq!(settling.held(), vec![1], "0.45 is away from home");
        let turning = settling.settle(&1, 1.0, ink(1.0), now + FAST, Motion::Full);
        assert!(
            (turning - 0.45).abs() < 1e-6,
            "it leaves from the dim: {turning}"
        );
        let back = settling.settle(&1, 1.0, ink(1.0), now + 2 * FAST, Motion::Full);
        assert_eq!(back, 1.0);
        assert!(settling.held().is_empty());
    }

    /// RED — **reduced motion takes the value whole and asks for nothing.**
    ///
    /// This is the red line the whole animation block is written under, on this
    /// register's terms. Mutation: sample the curve under `Reduced` and the
    /// window animates for somebody who asked it not to — and keeps asking to be
    /// woken while it does.
    #[test]
    fn reduced_motion_takes_the_target_on_the_first_frame_and_keeps_nothing() {
        let mut settling = Settling::<u8>::default();
        let now = Instant::now();
        assert_eq!(settling.settle(&1, 0.0, ink(1.0), now, Motion::Full), 0.0);
        // The preference changes mid-fade: the entry goes and the value is final.
        assert_eq!(
            settling.settle(&1, 0.0, ink(1.0), now, Motion::Reduced),
            1.0
        );
        assert!(settling.held().is_empty());
        assert!(!settling.moving(now, Motion::Reduced));
        assert!(settling.drawn(now, Motion::Reduced).is_empty());
        assert_eq!(
            settling.settle(&1, 0.0, ink(0.0), now, Motion::Reduced),
            0.0
        );
        assert!(!settling.moving(now, Motion::Reduced));
    }

    /// RED — two keys keep two clocks: one pane's run going out does not
    /// restart the next pane's coming in.
    #[test]
    fn each_key_keeps_its_own_clock() {
        let mut settling = Settling::<u8>::default();
        let now = Instant::now();
        let _ = settling.settle(&1, 0.0, ink(1.0), now, Motion::Full);
        let later = now + FAST;
        assert_eq!(settling.settle(&1, 0.0, ink(1.0), later, Motion::Full), 1.0);
        let child = settling.settle(&2, 0.0, ink(1.0), later, Motion::Full);
        assert!(child < 0.01, "the second key is only now arriving: {child}");
        assert_eq!(settling.held().len(), 2);
    }

    /// RED — **a foot that changes its mind dissolves through the ground: the
    /// old phrase thins out, the new one thickens in, and the swap happens where
    /// nothing is legible.**
    ///
    /// Mutation: return `(wanted, 1.0)` always and the receipt goes back to
    /// being a path between two frames, which is the hard cut the audit filed.
    #[test]
    fn a_foot_that_changes_its_mind_thins_the_old_phrase_out_and_the_new_one_in() {
        let span = bt_render::FOOT_RECEIPT_CROSSFADE;
        let half = span / 2;
        let mut feet = Crossfade::<u8>::default();
        let now = Instant::now();
        // The first thing a box is asked, it simply says.
        assert_eq!(
            feet.say(&1, Some("D:\\work"), now, Motion::Full),
            (Some("D:\\work".to_owned()), 1.0)
        );

        // The receipt arrives: for the first half the *path* is still there,
        // thinning.
        let (said, ink) = feet.say(&1, Some("Revealed"), now, Motion::Full);
        assert_eq!(said.as_deref(), Some("D:\\work"));
        assert!(
            (ink - 1.0).abs() < 1e-6,
            "it leaves from full strength: {ink}"
        );
        let (said, ink) = feet.say(&1, Some("Revealed"), now + half / 2, Motion::Full);
        assert_eq!(said.as_deref(), Some("D:\\work"));
        assert!(ink > 0.0 && ink < 1.0, "{ink}");

        // Past the midpoint the new phrase is the one on the glass, arriving.
        let (said, ink) = feet.say(&1, Some("Revealed"), now + half, Motion::Full);
        assert_eq!(said.as_deref(), Some("Revealed"));
        assert!(ink < 0.01, "the new phrase begins at nothing: {ink}");
        let (said, ink) = feet.say(&1, Some("Revealed"), now + span, Motion::Full);
        assert_eq!(said.as_deref(), Some("Revealed"));
        assert_eq!(ink, 1.0);
        assert!(!feet.moving(now + span, Motion::Full));
        assert!(feet.drawn(now + span, Motion::Full).is_empty());
    }

    /// RED — **reduced motion swaps the word and asks for nothing**, and the
    /// strip is never blank for a frame.
    ///
    /// The hold in front of it is untouched either way: this register is told
    /// what the strip wants to say and never decides when it wants to say it.
    #[test]
    fn a_reduced_foot_swaps_the_word_in_one_frame() {
        let mut feet = Crossfade::<u8>::default();
        let now = Instant::now();
        let _ = feet.say(&1, Some("D:\\work"), now, Motion::Reduced);
        assert_eq!(
            feet.say(&1, Some("Revealed"), now, Motion::Reduced),
            (Some("Revealed".to_owned()), 1.0)
        );
        assert!(!feet.moving(now, Motion::Reduced));
        assert!(feet.drawn(now, Motion::Reduced).is_empty());
        // And back, still in one frame.
        assert_eq!(feet.say(&1, None, now, Motion::Reduced), (None, 1.0));
        assert!(!feet.moving(now, Motion::Reduced));
    }

    /// RED — a box nobody asks about any more is swept, and one that is still
    /// being asked about is not.
    ///
    /// The memory cannot retire itself the way a fade can: an entry that has
    /// settled still holds the phrase the *next* change has to dissolve out of.
    #[test]
    fn a_foot_whose_surface_has_gone_is_swept_and_the_others_are_kept() {
        let mut feet = Crossfade::<u8>::default();
        let now = Instant::now();
        let _ = feet.say(&1, None, now, Motion::Full);
        let _ = feet.say(&2, Some("Saved"), now, Motion::Full);
        feet.retain(|key| *key == 2);
        // The swept one has no memory left, so its next phrase is simply said.
        assert_eq!(
            feet.say(&1, Some("Revealed"), now, Motion::Full),
            (Some("Revealed".to_owned()), 1.0)
        );
    }

    /// RED — the frame-debt reading is quantised, and it empties when the last
    /// thing settles.
    #[test]
    fn the_drawn_reading_is_quantised_and_empties_when_everything_is_home() {
        let mut settling = Settling::<u8>::default();
        let now = Instant::now();
        let _ = settling.settle(&1, 0.0, ink(1.0), now, Motion::Full);
        assert_eq!(settling.drawn(now, Motion::Full), vec![(1, 0)]);
        let _ = settling.settle(&1, 0.0, ink(1.0), now + FAST, Motion::Full);
        assert_eq!(settling.drawn(now + FAST, Motion::Full), vec![(1, 1000)]);
        let _ = settling.settle(&1, 0.0, ink(0.0), now + FAST, Motion::Full);
        let _ = settling.settle(&1, 0.0, ink(0.0), now + 2 * FAST, Motion::Full);
        assert!(settling.drawn(now + 2 * FAST, Motion::Full).is_empty());
    }
}
