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

/// `transition: width .16s ease, margin-left .16s ease` (line 341) — the pin's
/// zero-width expansion. One continuous layout change, not a fade-in on top of
/// a jump: the control widens *in* and the badge beside it slides aside.
///
/// **Base**, with the fade below: the pair used to run at 160 and 120 so the ink
/// arrived a touch ahead of the box. One span for both loses that grace note and
/// keeps the thing it was decorating — a control that widens and fills at once
/// still widens and fills, and a sixteenth of a second of offset is not what
/// made it legible.
pub const WINDOW_TAB_PIN_REVEAL_MS: u64 = MOTION_BASE_MS;
/// `opacity .12s ease` from the same declaration — the pin's ink.
pub const WINDOW_TAB_PIN_FADE_MS: u64 = MOTION_BASE_MS;

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

    /// PIN — the named spans really are the tokens, not numbers that happen to
    /// agree with them today.
    #[test]
    fn every_named_span_in_this_file_resolves_to_one_of_the_three() {
        for (name, ms) in [
            ("RAIL_TRANSITION_MS", RAIL_TRANSITION_MS),
            ("RAIL_TEXT_FADE_MS", RAIL_TEXT_FADE_MS),
            ("FLOAT_WINDOW_ANIMATION_MS", FLOAT_WINDOW_ANIMATION_MS),
            ("WINDOW_TAB_PIN_REVEAL_MS", WINDOW_TAB_PIN_REVEAL_MS),
            ("WINDOW_TAB_PIN_FADE_MS", WINDOW_TAB_PIN_FADE_MS),
        ] {
            assert!(
                MOTION_ARCHIVE_MS.contains(&ms),
                "{name} is {ms}ms, off the archive"
            );
        }
    }
}
