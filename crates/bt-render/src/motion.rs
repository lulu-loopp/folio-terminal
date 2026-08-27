//! The window's rhythm: three spans, one travel, three curves — and the rule
//! that keeps a fourth span from ever being added.
//!
//! # Why this file exists
//!
//! Before it, a duration was written wherever the surface that needed one was
//! written: `main.rs` held eight, `theme.rs` nine, and `float.rs`, `toast.rs`,
//! `tooltip.rs`, `keyhint.rs`, `seats.rs`, `cmdrail.rs` and `termscroll.rs` one
//! or two each. Every one of them was defensible on its own page and no page
//! could see the others, so the window drew fades at ninety, a hundred, a
//! hundred and twenty, a hundred and forty, a hundred and sixty, a hundred and
//! eighty, two hundred, two hundred and twenty and four hundred and twenty
//! milliseconds. That is not nine decisions; it is one decision nobody took.
//!
//! So the spans are named here, once, and every surface asks for one of them by
//! name. What a surface still chooses for itself is *which* span and *which*
//! curve — that is design. What it may no longer choose is a number.
//!
//! # The three spans
//!
//! | Span | Length | What wears it |
//! |---|---|---|
//! | [`MOTION_FAST_MS`] | 90ms | opacity and colour on secondary chrome: tips, key hints, rail labels, hover states, a card pulling in under a divider |
//! | [`MOTION_BASE_MS`] | 140ms | **one interaction**: every popup, menu, sheet and dialog arriving and leaving, a toast, a float, a chevron turning over |
//! | [`MOTION_SLOW_MS`] | 200ms | motion that moves the *layout*: the rail's width, a pane FLIP, a tab landing, a fresh pane arriving |
//!
//! 140 is the base because the product had already chosen it twice without
//! saying so: `.chevbtn svg { transition: transform 140ms }` is the arrow over
//! the profile list, and the list it points at is the other half of that same
//! action. A menu that appeared in one frame under an arrow that took 140ms to
//! turn was one gesture with a rhythm on one side of it only.
//!
//! # The travel
//!
//! [`MOTION_TRAVEL_LOGICAL_PX`] is four logical pixels, and it is the *whole*
//! of the distance rule: **a layer arrives from the direction of the thing that
//! summoned it**, and it arrives from four pixels away. Not eight (a toast's
//! old number, which reads as a slide) and not five (a float's, which was four
//! rounded by nobody). Four pixels is provenance — enough for the eye to catch
//! where a thing came from, too little to be a journey.
//!
//! # What is *not* rhythm
//!
//! Two families of duration live outside the archive on purpose, and conflating
//! either with a transition is the mistake this file is meant to end:
//!
//! * **Waits.** How long a hand must rest before a tip is summoned, how long a
//!   peek is forgiven for a pointer that strayed, how long a scroll bar stays up
//!   after the scrolling stopped. These are *intent* and *discoverability*
//!   rules. A tooltip's 380ms delay and its 90ms fade are not two numbers in one
//!   family that failed to agree; they are the answer to two different
//!   questions. They keep their own names — `…_DELAY`, `…_DWELL`, `…_GRACE`,
//!   `…_HOLD`, `…_REST`, `…_INTENT…` — and they are registered as
//!   [`MotionKind::Intent`] so a reader can see they were considered and set
//!   aside rather than missed.
//! * **Periods and holds.** A caret's blink, a spinner's turn, a breath, the
//!   ease of a *number* arriving at a new reading, the second a receipt stands
//!   before it becomes a verb again. These are spans, but they are not the span
//!   of one interaction, and forcing them into 90/140/200 would change what they
//!   say. They are [`MotionKind::Exempt`] and each one carries its reason in the
//!   register.
//!
//! # The gate
//!
//! [`unarchived`] is the rule itself, and `bt-app` holds the one register that
//! names every span in the product and runs it. The register is written out by
//! hand rather than discovered by scanning the source, deliberately: a scan
//! finds what it knows how to match and reports silence for a duration spelled a
//! way it did not expect, which is the failure mode a red gate may not have. A
//! hand-written table is wrong loudly — a span that is not in it is a span
//! nobody wrote a line about.

use std::time::Duration;

// ── the archive ────────────────────────────────────────────────────────────

/// **Fast** — 90ms. Opacity and colour on chrome that is not the subject.
///
/// The shortest span that still reads as a transition rather than a flicker,
/// and the one the tip and the key hint already ran on before there was an
/// archive to put them in.
pub const MOTION_FAST_MS: u64 = 90;
/// **Base** — 140ms. One interaction: something appearing, or going away.
pub const MOTION_BASE_MS: u64 = 140;
/// **Slow** — 200ms. Motion that moves the layout rather than a layer over it.
pub const MOTION_SLOW_MS: u64 = 200;

/// [`MOTION_FAST_MS`] as the type the samplers actually take.
pub const MOTION_FAST: Duration = Duration::from_millis(MOTION_FAST_MS);
/// [`MOTION_BASE_MS`] as the type the samplers actually take.
pub const MOTION_BASE: Duration = Duration::from_millis(MOTION_BASE_MS);
/// [`MOTION_SLOW_MS`] as the type the samplers actually take.
pub const MOTION_SLOW: Duration = Duration::from_millis(MOTION_SLOW_MS);

/// The three spans, in order, as the gate reads them.
pub const MOTION_ARCHIVE_MS: [u64; 3] = [MOTION_FAST_MS, MOTION_BASE_MS, MOTION_SLOW_MS];

/// How far a layer travels as it arrives, in logical pixels.
///
/// One distance for every surface that travels at all, and always *away from*
/// the thing that summoned it: a menu slides four pixels off its anchor, a toast
/// four off the edge it hangs from, a float four off the trigger that was
/// pressed. Nothing travels on the way out — leaving toward an edge says "going
/// somewhere", and a layer that is ending is not going anywhere.
pub const MOTION_TRAVEL_LOGICAL_PX: f32 = 4.0;

/// **Which way a layer travels as it arrives** — the direction half of the rule
/// [`MOTION_TRAVEL_LOGICAL_PX`] states the distance half of.
///
/// The name is the direction the layer *finishes* moving in, which is the
/// direction it grew in: a menu that drops out of the button above it travels
/// [`Travel::Down`], and it therefore starts four pixels *up*, against that
/// button. So a reader of a call site sees where the thing came from, and
/// [`Travel::offset`] is the one place the sign is worked out.
///
/// Four directions and no diagonals. Every layer in this window hangs off an
/// edge of the control that raised it — under a chevron, beside a row, off a
/// pane's head — and a diagonal entrance would say the thing came from a
/// corner, which nothing here does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Travel {
    /// Grew downward: it starts against the control above it.
    Down,
    /// Grew upward — a menu that had no room below its button.
    Up,
    /// Grew to the right: a submenu off its parent's row, the icon rail's own
    /// list off the panel beside it.
    Right,
    /// Grew to the left — the same, with no room on the right.
    Left,
}

impl Travel {
    /// Every direction, for a test that means to walk them all.
    pub const ALL: [Self; 4] = [Self::Down, Self::Up, Self::Right, Self::Left];

    /// **Which way a box at `frame` grew out of the control at `anchor`** — the
    /// derivation, so that a layer that had to flip is never drawn arriving from
    /// the side it did not come from.
    ///
    /// Both rectangles are `[left, top, right, bottom]` in the same space, and
    /// the answer is the axis their centres differ on most: a menu hanging under
    /// a button is far below it and barely beside it, a submenu beside a row is
    /// the reverse, and the larger difference is the one the eye reads as "it
    /// came out of there". Ties fall to the vertical, which is the way all but
    /// one family of this window's popups open.
    #[must_use]
    pub fn away_from(anchor: [f32; 4], frame: [f32; 4]) -> Self {
        let dx = (frame[0] + frame[2]) / 2.0 - (anchor[0] + anchor[2]) / 2.0;
        let dy = (frame[1] + frame[3]) / 2.0 - (anchor[1] + anchor[3]) / 2.0;
        if dx.abs() > dy.abs() {
            if dx < 0.0 { Self::Left } else { Self::Right }
        } else if dy < 0.0 {
            Self::Up
        } else {
            Self::Down
        }
    }

    /// **Where the layer stands when it is `remaining` of the way from its
    /// origin**, in physical pixels, at `scale`.
    ///
    /// `remaining` is `1.0` on the first frame of an entrance and `0.0` when it
    /// has landed — the complement of the eased progress — so the displacement
    /// is always *back toward the anchor* and always reaches zero.
    #[must_use]
    pub fn offset(self, remaining: f32, scale: f32) -> [f32; 2] {
        let distance = remaining * MOTION_TRAVEL_LOGICAL_PX * scale;
        match self {
            Self::Down => [0.0, -distance],
            Self::Up => [0.0, distance],
            Self::Right => [-distance, 0.0],
            Self::Left => [distance, 0.0],
        }
    }
}

// ── the curves ─────────────────────────────────────────────────────────────

/// CSS `ease` — the keyword the mock-up writes for nearly every transition it
/// declares.
pub const EASE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
/// CSS `ease-in-out`, worn by `@keyframes breathe` and the waiting halo.
pub const EASE_IN_OUT: [f32; 4] = [0.42, 0.0, 0.58, 1.0];
/// `cubic-bezier(.2, 0, 0, 1)` — leave immediately, arrive gently.
///
/// The curve of everything a hand is holding or has just let go of: a tab
/// FLIPping into its slot, a pane changing shape, an arrow turning over. Its
/// character is the point — it starts at full speed, so the picture answers the
/// press on the first frame rather than a fortieth of a second later.
pub const GRAB_EASE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];

// ── the spans, by the name each surface knows them by ──────────────────────

/// The rail's open/close (`width .18s ease, padding .18s ease, opacity .18s
/// ease`, and the shade's `left`) — P168.
///
/// **Slow**, and no longer the mock-up's 180: the rail is a panel whose width
/// re-solves the whole seat tree, which is the definition of the slow span. The
/// stylesheet's own eighteen hundredths was inside the twentieth this file's
/// slow span states, and reading it as "two hundred, spelled loosely" is nearer
/// the truth than treating it as a fourth number.
pub const RAIL_TRANSITION_MS: u64 = MOTION_SLOW_MS;
/// The label/title/badge/`×` fade in icon mode (`opacity .1s ease`) — Q183.
///
/// **Fast.** The text fades rather than being removed, so the layout is
/// identical in both states and the icons never jump; the rail's own overflow
/// does the clipping while the width animates.
pub const RAIL_TEXT_FADE_MS: u64 = MOTION_FAST_MS;
/// `transition-delay: .06s` on the way *open* only — the panel gets a moment to
/// be wide enough to hold words before the words arrive.
///
/// **A wait, not a span**, and therefore not in the archive: it is the reason
/// the two halves of one gesture do not start together, and it would say
/// something else at any length the archive offers.
pub const RAIL_TEXT_FADE_OPEN_DELAY_MS: u64 = 60;

/// `@keyframes flyIn/flyOut` — a float arriving and its reverse (§7.1.2).
///
/// **Base**: a float is a popup, and popups are one interaction.
pub const FLOAT_WINDOW_ANIMATION_MS: u64 = MOTION_BASE_MS;

/// **Every popup arriving** — the eight menus, their submenus, the settings
/// dialog and its scrim, the notice strip.
///
/// **Base**, and it is the span the base was chosen *from*: `.chevbtn svg {
/// transition: transform 140ms }` is the arrow over the profile list, and the
/// list is the other half of that same press. The arrow turned over 140ms while
/// the menu it pointed at appeared in one frame — one gesture with a rhythm on
/// one side of it only — and this is the number that ends that.
pub const POPUP_ENTER_MS: u64 = MOTION_BASE_MS;
/// **Every popup leaving.**
///
/// **Fast**, and asymmetric on purpose. An entrance is read: the eye is told
/// where a thing came from and what is now in front of it. A departure is not —
/// what a hand is waiting for after the press that dismissed a menu is the thing
/// *underneath* it, and a menu leaving at the base span is a menu in the way for
/// half again as long as it needs to be. Nothing travels on the way out, for
/// [`MOTION_TRAVEL_LOGICAL_PX`]'s stated reason.
pub const POPUP_EXIT_MS: u64 = MOTION_FAST_MS;
/// [`POPUP_ENTER_MS`] as the type the samplers take.
pub const POPUP_ENTER: Duration = Duration::from_millis(POPUP_ENTER_MS);
/// [`POPUP_EXIT_MS`] as the type the samplers take.
pub const POPUP_EXIT: Duration = Duration::from_millis(POPUP_EXIT_MS);

/// `transition: width .16s ease, margin-left .16s ease` (line 341) — the pin's
/// zero-width expansion. **The room**, not the ink: the control widens *in* and
/// the badge beside it slides aside, which is a layout change and therefore one
/// interaction.
///
/// **Base**, and the pair below it is now 140 and 90 rather than the mock-up's
/// 160 and 120. See [`WINDOW_TAB_PIN_FADE_MS`] for why the two did not collapse
/// onto one number after all.
pub const WINDOW_TAB_PIN_REVEAL_MS: u64 = MOTION_BASE_MS;
/// `opacity .12s ease` from the same declaration — the pin's ink.
///
/// **Fast, by the rule the whole window's hover-revealed chrome now keeps**
/// (user ruling 2026-08-26, the animation block's second slice): *the ink of a
/// control a hover reveals is fast; the room it needs to be revealed into is one
/// interaction.* The pane head's `×`, the files head's chevron, the preview
/// head's controls and the preview rail's tools are all pure ink — their head is
/// already on screen and only the mark appears — so all of them are 90, and a
/// pin whose ink ran at 140 while the identical marks two rows below ran at 90
/// would be one rule with an exception nobody could see the reason for.
///
/// This overrides the previous slice's collapse of the pair onto a single span.
/// That collapse read the two declarations as one gesture measured twice; the
/// rule above reads them as *two* facts — a box making room, and ink arriving in
/// it — which is also what the mock-up's own two numbers were saying before they
/// were rounded to one. What is dropped is not the grace note but the mock-up's
/// arbitrary sixteenth: the ink still lands ahead of the box, at a span this
/// window uses everywhere else for exactly this.
pub const WINDOW_TAB_PIN_FADE_MS: u64 = MOTION_FAST_MS;

/// **Every control a hover reveals**, arriving and going: the pane head's run,
/// the files head's chevron and its pill, the preview head's own furniture and
/// the preview rail's tools.
///
/// **Fast**, and it is the span the fast rung was written for: this is chrome
/// that is not the subject, its room is already on the glass, and what changes
/// is one alpha. The rule it states — *ink is fast, room is one interaction* —
/// is what decides [`WINDOW_TAB_PIN_FADE_MS`] against
/// [`WINDOW_TAB_PIN_REVEAL_MS`] beside it.
///
/// **What it does not touch is the hit test.** A control is pressable from the
/// frame its reveal *begins*, not from the frame it reaches full strength: a
/// button that could be seen and not pressed for a sixteenth of a second is the
/// same broken promise as one that could be pressed and not seen, which is the
/// bug a layout probe found on a files head on 2026-08-26. Going the other way
/// the promise reverses with the picture: a run whose pane the hand has left is
/// ink on its way out and answers nothing, exactly as a departing menu does.
pub const HOVER_CHROME_FADE_MS: u64 = MOTION_FAST_MS;
/// [`HOVER_CHROME_FADE_MS`] as the type the samplers take.
pub const HOVER_CHROME_FADE: Duration = Duration::from_millis(HOVER_CHROME_FADE_MS);

/// **A tab's chrome changing hands** — the ground, the ring and the label of the
/// tab that has just become the active one, and of the one that has just
/// stopped being it.
///
/// **Fast**: three colours on furniture, which is precisely the fast rung's
/// remit. It is the *colour* alone — see the note below on what stays hard.
///
/// **The content does not cross-fade, and that is a rule rather than an
/// omission** (§7.18 ⑦, restated here where the colour now moves). What a tab
/// press asks for is the other terminal, and the answer to that question is the
/// other terminal — whole, on the first frame, at full contrast. Two grids of
/// glyphs dissolved through one another for a tenth of a second is unreadable
/// text pretending to be a transition, and a reader who pressed a tab to *read*
/// something would be made to wait for the picture to stop lying. So the body
/// switches in one frame and the furniture around it takes ninety milliseconds
/// to say which tab is now the one you are in.
pub const TAB_ACTIVATION_MS: u64 = MOTION_FAST_MS;
/// [`TAB_ACTIVATION_MS`] as the type the samplers take.
pub const TAB_ACTIVATION: Duration = Duration::from_millis(TAB_ACTIVATION_MS);

/// **A row of the Git panel dimming while a write about it is in flight** —
/// `git add` on one file, `git checkout` on one branch.
///
/// **Fast.** The row does not go anywhere and nothing appears; one opacity
/// changes on a list that is still the truth. It was an instant jump to 0.45 and
/// back, on a list that is otherwise perfectly still, which read as the row
/// flickering rather than as the row waiting.
pub const GIT_PENDING_FADE_MS: u64 = MOTION_FAST_MS;
/// [`GIT_PENDING_FADE_MS`] as the type the samplers take.
pub const GIT_PENDING_FADE_SPAN: Duration = Duration::from_millis(GIT_PENDING_FADE_MS);

/// **A foot's receipt trading places with the fact it stands in front of** —
/// `Revealed`, `Opened`, `Saved`, `140%`.
///
/// **Fast**, and a *cross*-fade rather than a swap: the two phrases share one
/// box, so the outgoing one thins as the incoming one thickens and the strip is
/// never blank. The hold in front of it is untouched — see the register's
/// `FOOT_REVEAL_FEEDBACK`, which is 1300ms of standing still and is not this.
pub const FOOT_RECEIPT_CROSSFADE_MS: u64 = MOTION_FAST_MS;
/// [`FOOT_RECEIPT_CROSSFADE_MS`] as the type the samplers take.
pub const FOOT_RECEIPT_CROSSFADE: Duration = Duration::from_millis(FOOT_RECEIPT_CROSSFADE_MS);

/// **The ghost that hangs off the pointer once a drag has really begun.**
///
/// **Fast, and the opacity only.** The ghost's *position* is the pointer's, on
/// every frame, with no easing of any kind — that is the red line the plan
/// states and this constant must never be read as softening it: a picture of the
/// thing in your hand that lagged the hand would be a picture of somebody else's
/// hand. What fades is whether the ghost is there at all, over the frames after
/// the pointer crosses the drag threshold, so the moment a press becomes a carry
/// is a moment rather than a flicker.
pub const DRAG_GHOST_FADE_MS: u64 = MOTION_FAST_MS;
/// [`DRAG_GHOST_FADE_MS`] as the type the samplers take.
pub const DRAG_GHOST_FADE: Duration = Duration::from_millis(DRAG_GHOST_FADE_MS);

/// **The settings page's `Advanced` group opening and shutting** — the height
/// its rows take, not their ink.
///
/// **Slow**, because it is the definition of the slow rung: rows below it move,
/// the page's own height changes and the scroll bar re-solves. A *clip* reveal
/// and not a spring — the group's rows stand still in their final places and the
/// band they are cut to grows down over them, so nothing on the page is ever
/// drawn at a position it is not also pressable at.
pub const DISCLOSURE_REVEAL_MS: u64 = MOTION_SLOW_MS;
/// [`DISCLOSURE_REVEAL_MS`] as the type the samplers take.
pub const DISCLOSURE_REVEAL: Duration = Duration::from_millis(DISCLOSURE_REVEAL_MS);

// ── the exemptions: spans that are not the span of one interaction ─────────

/// `.ticon.working { animation: breathe 1.7s ease-in-out infinite }` (line 245).
///
/// **Exempt: a period, not a transition.** Nothing arrives or leaves; a mark
/// that is already there says it is still working. Seventeen hundred
/// milliseconds is a breathing rate, and 140 would be a strobe.
pub const WINDOW_TAB_BREATHE_PERIOD_MS: u64 = 1_700;
/// `.pring.indeterminate { animation: pring-spin 1.1s linear infinite }` (282).
///
/// **Exempt: a period, not a transition.** One turn of an indeterminate arc.
pub const WINDOW_TAB_RING_SPIN_PERIOD_MS: u64 = 1_100;
/// `.pring .arc { transition: stroke-dashoffset .3s ease }` (line 279) — a
/// progress report jumps, and the arc that reports it must not.
///
/// **Exempt, and the user ruled it so (2026-08-26).** This is a *number*
/// arriving at a new reading rather than a thing appearing on the glass, and the
/// eye reads the arrival as the measurement. It is also the one place in this
/// product that deliberately runs under reduced motion, for the same reason: the
/// value is the content.
pub const WINDOW_TAB_RING_SWEEP_TRANSITION_MS: u64 = 300;

// ── the gate ───────────────────────────────────────────────────────────────

/// What one registered span is, as far as the rule is concerned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionKind {
    /// A transition, and therefore one of [`MOTION_ARCHIVE_MS`].
    Archived,
    /// A span that is not the span of one interaction, with the reason it is
    /// not. Periods, blinks, holds and value arrivals live here.
    Exempt(&'static str),
    /// Not a transition at all: a wait before one, or a grace after one, with
    /// what it is waiting for.
    Intent(&'static str),
}

/// One row of the register: a duration the product actually holds, under the
/// name a reader will find it by.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionSpan {
    /// The constant's own name, spelled as the source spells it.
    pub name: &'static str,
    /// Its value, read *from the constant* rather than repeated — a register
    /// that restates numbers is a second source for them.
    pub ms: u64,
    /// Which of the three things it is.
    pub kind: MotionKind,
}

/// The words that mark a name as a wait rather than a span.
///
/// A duration that gates when something appears must say so where it is used,
/// not only where it is registered: `FLY_OPEN` is what let a 180ms hover-intent
/// be read, twice, by two independent audits, as a 180ms entrance animation.
const INTENT_WORDS: [&str; 7] = ["DELAY", "DWELL", "GRACE", "HOLD", "INTENT", "LIFE", "REST"];

/// Every complaint the register earns, empty when the rhythm holds.
///
/// Three rules, and the second and third are what stop the first from being
/// decorative:
///
/// 1. An [`MotionKind::Archived`] span is one of the three. This is the gate.
/// 2. An [`MotionKind::Exempt`] span carries a reason **and is not one of the
///    three**. A number that already agrees with the archive is not an
///    exception to it, and filing it as one hides a span that is in fact
///    governed — so the next person to change the archive would miss it.
/// 3. An [`MotionKind::Intent`] wait carries a reason **and says in its own
///    name** that it is a wait. Anything else is a duration that will be read as
///    a transition by the next person who greps for one.
#[must_use]
pub fn unarchived(spans: &[MotionSpan]) -> Vec<String> {
    let mut complaints = Vec::new();
    for span in spans {
        match span.kind {
            MotionKind::Archived => {
                if !MOTION_ARCHIVE_MS.contains(&span.ms) {
                    complaints.push(format!(
                        "{} is {}ms, which is not one of the three spans {MOTION_ARCHIVE_MS:?}",
                        span.name, span.ms
                    ));
                }
            }
            MotionKind::Exempt(reason) => {
                if reason.trim().is_empty() {
                    complaints.push(format!("{} is exempt with no reason given", span.name));
                }
                if MOTION_ARCHIVE_MS.contains(&span.ms) {
                    complaints.push(format!(
                        "{} is {}ms, which is an archived span — it does not need an exemption",
                        span.name, span.ms
                    ));
                }
            }
            MotionKind::Intent(reason) => {
                if reason.trim().is_empty() {
                    complaints.push(format!("{} is a wait with no reason given", span.name));
                }
                if !INTENT_WORDS.iter().any(|word| span.name.contains(word)) {
                    complaints.push(format!(
                        "{} is a wait, so its name has to say so — one of {INTENT_WORDS:?}",
                        span.name
                    ));
                }
            }
        }
    }
    complaints
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — the three spans, and the distance between them.
    ///
    /// Mutation: move any of the three and every surface in the product moves
    /// with it, which is the whole point; move two of them together and the
    /// ladder stops being a ladder.
    #[test]
    fn the_archive_is_three_rungs_and_they_are_ninety_a_hundred_and_forty_and_two_hundred() {
        assert_eq!(MOTION_ARCHIVE_MS, [90, 140, 200]);
        const { assert!(MOTION_FAST_MS < MOTION_BASE_MS && MOTION_BASE_MS < MOTION_SLOW_MS) };
        assert_eq!(MOTION_FAST, Duration::from_millis(90));
        assert_eq!(MOTION_BASE, Duration::from_millis(140));
        assert_eq!(MOTION_SLOW, Duration::from_millis(200));
        assert!((MOTION_TRAVEL_LOGICAL_PX - 4.0).abs() < f32::EPSILON);
    }

    /// RED — **a layer arrives from the direction of the thing that summoned
    /// it**, including when that thing is below it.
    ///
    /// Mutation: return a constant direction and the flipped menu below arrives
    /// from the side it did not come from — the four-pixel lie the derivation
    /// exists to prevent.
    #[test]
    fn a_box_grows_away_from_its_anchor_on_whichever_axis_it_grew_most() {
        let button = [100.0, 40.0, 140.0, 64.0];
        // The ordinary drop: a menu under the button that raised it.
        assert_eq!(
            Travel::away_from(button, [100.0, 64.0, 300.0, 300.0]),
            Travel::Down
        );
        // The same menu with no room below it, standing on the button instead.
        assert_eq!(
            Travel::away_from(button, [100.0, -200.0, 300.0, 40.0]),
            Travel::Up
        );
        // A submenu off a row: far beside it, barely below it.
        let row = [100.0, 200.0, 300.0, 224.0];
        assert_eq!(
            Travel::away_from(row, [300.0, 200.0, 500.0, 400.0]),
            Travel::Right
        );
        assert_eq!(
            Travel::away_from(row, [-100.0, 200.0, 100.0, 400.0]),
            Travel::Left
        );
    }

    /// RED — the displacement is **toward** the anchor and it reaches zero.
    ///
    /// Mutation: drop the minus signs and every popup in the window arrives from
    /// the far side of itself, sliding *away* from the control that raised it.
    #[test]
    fn the_travel_starts_against_the_anchor_and_lands_at_nothing() {
        for travel in Travel::ALL {
            assert_eq!(
                travel.offset(0.0, 2.0),
                [0.0, 0.0],
                "{travel:?} never lands"
            );
        }
        assert_eq!(Travel::Down.offset(1.0, 1.0), [0.0, -4.0]);
        assert_eq!(Travel::Up.offset(1.0, 1.0), [0.0, 4.0]);
        assert_eq!(Travel::Right.offset(1.0, 1.0), [-4.0, 0.0]);
        assert_eq!(Travel::Left.offset(1.0, 1.0), [4.0, 0.0]);
        // The distance is logical, so a 200% window travels twice as far in
        // pixels and exactly as far to the eye.
        assert_eq!(Travel::Down.offset(1.0, 2.0), [0.0, -8.0]);
        assert_eq!(Travel::Down.offset(0.5, 1.0), [0.0, -2.0]);
    }

    /// PIN — the gate passes a well-formed register and nothing else.
    #[test]
    fn a_registered_span_is_archived_exempt_with_a_reason_or_a_named_wait() {
        let good = [
            MotionSpan {
                name: "TOAST_EXIT",
                ms: MOTION_FAST_MS,
                kind: MotionKind::Archived,
            },
            MotionSpan {
                name: "CURSOR_BLINK_PHASE",
                ms: 550,
                kind: MotionKind::Exempt("a blink is a period"),
            },
            MotionSpan {
                name: "TOOLTIP_DELAY",
                ms: 380,
                kind: MotionKind::Intent("how long a hand rests before a tip is summoned"),
            },
        ];
        assert_eq!(unarchived(&good), Vec::<String>::new());
    }

    /// PIN — each of the three ways a register can be wrong is caught, and the
    /// complaint names the constant.
    ///
    /// Mutation: drop rule 2 and a governed span can hide behind the word
    /// "exempt"; drop rule 3 and `FLY_OPEN` comes back.
    #[test]
    fn a_fourth_span_an_idle_exemption_and_an_unnamed_wait_are_all_refused() {
        let fourth = [MotionSpan {
            name: "SOME_NEW_FADE",
            ms: 120,
            kind: MotionKind::Archived,
        }];
        let complaints = unarchived(&fourth);
        assert_eq!(complaints.len(), 1);
        assert!(complaints[0].contains("SOME_NEW_FADE") && complaints[0].contains("120ms"));

        let idle = [MotionSpan {
            name: "ALREADY_FINE",
            ms: MOTION_BASE_MS,
            kind: MotionKind::Exempt("no it is not"),
        }];
        assert_eq!(unarchived(&idle).len(), 1);

        let reasonless = [MotionSpan {
            name: "MYSTERY_HOLD",
            ms: 700,
            kind: MotionKind::Exempt("   "),
        }];
        assert_eq!(unarchived(&reasonless).len(), 1);

        let unnamed = [MotionSpan {
            name: "FLY_OPEN",
            ms: 180,
            kind: MotionKind::Intent("hover intent"),
        }];
        let complaints = unarchived(&unnamed);
        assert_eq!(complaints.len(), 1);
        assert!(complaints[0].contains("FLY_OPEN"));
    }

    /// RED — **ink is fast and room is one interaction**, the rule the second
    /// animation slice put every hover-revealed control in this window under.
    ///
    /// Mutation: put any of the ink spans back on the base rung and the window
    /// draws two speeds of the same reveal — a pin filling at 140 beside a `×`
    /// filling at 90 — which is the drift the archive exists to end. Mutation the
    /// other way: pull the pin's *width* down to the fast rung and a layout
    /// change is being drawn at chrome's speed.
    #[test]
    fn the_ink_a_hover_reveals_is_fast_and_the_room_it_needs_is_one_interaction() {
        for (name, ms) in [
            ("HOVER_CHROME_FADE_MS", HOVER_CHROME_FADE_MS),
            ("WINDOW_TAB_PIN_FADE_MS", WINDOW_TAB_PIN_FADE_MS),
            ("TAB_ACTIVATION_MS", TAB_ACTIVATION_MS),
            ("GIT_PENDING_FADE_MS", GIT_PENDING_FADE_MS),
            ("FOOT_RECEIPT_CROSSFADE_MS", FOOT_RECEIPT_CROSSFADE_MS),
            ("DRAG_GHOST_FADE_MS", DRAG_GHOST_FADE_MS),
        ] {
            assert_eq!(ms, MOTION_FAST_MS, "{name} is ink and ink is fast");
        }
        assert_eq!(WINDOW_TAB_PIN_REVEAL_MS, MOTION_BASE_MS);
        assert_eq!(DISCLOSURE_REVEAL_MS, MOTION_SLOW_MS);
    }

    /// PIN — the named spans really are the tokens, not numbers that happen to
    /// agree with them today.
    #[test]
    fn every_named_span_in_this_file_resolves_to_one_of_the_three() {
        for (name, ms) in [
            ("RAIL_TRANSITION_MS", RAIL_TRANSITION_MS),
            ("RAIL_TEXT_FADE_MS", RAIL_TEXT_FADE_MS),
            ("FLOAT_WINDOW_ANIMATION_MS", FLOAT_WINDOW_ANIMATION_MS),
            ("POPUP_ENTER_MS", POPUP_ENTER_MS),
            ("POPUP_EXIT_MS", POPUP_EXIT_MS),
            ("WINDOW_TAB_PIN_REVEAL_MS", WINDOW_TAB_PIN_REVEAL_MS),
            ("WINDOW_TAB_PIN_FADE_MS", WINDOW_TAB_PIN_FADE_MS),
            ("HOVER_CHROME_FADE_MS", HOVER_CHROME_FADE_MS),
            ("TAB_ACTIVATION_MS", TAB_ACTIVATION_MS),
            ("GIT_PENDING_FADE_MS", GIT_PENDING_FADE_MS),
            ("FOOT_RECEIPT_CROSSFADE_MS", FOOT_RECEIPT_CROSSFADE_MS),
            ("DRAG_GHOST_FADE_MS", DRAG_GHOST_FADE_MS),
            ("DISCLOSURE_REVEAL_MS", DISCLOSURE_REVEAL_MS),
        ] {
            assert!(
                MOTION_ARCHIVE_MS.contains(&ms),
                "{name} is {ms}ms, off the archive"
            );
        }
    }
}
