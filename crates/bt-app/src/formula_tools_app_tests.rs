//! **`formula_tools`, as the application drives it.** Tests whose first assertion is about
//! `formula_tools`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;

/// PIN — the turn pays the strip's frame debt in the mark's own quantized
/// angles, so it draws every step it has and stops the moment it lands.
///
/// Two failures this stands against, and they pull opposite ways. Compare
/// the raw fraction and every wake-up of the 140ms owes a present, including
/// the long tail where `.2,0,0,1` is crawling through less than a degree and
/// the rasterized arrow is byte-identical. Compare nothing at all and the
/// arrow strands on whatever frame the last present happened to catch —
/// which is the failure `tab_owes_frame` was written for, in its original
/// half-faded-icon form.
#[test]
fn the_chevron_s_frame_debt_is_paid_in_drawn_angles() {
    let now = Instant::now();
    let mut turn = ChevronTurn::default();
    turn.retarget(true, now, Motion::Full);

    // The strip wakes on its own beat; what it draws each time is the mark.
    let mut last_drawn: Option<marks::ChromeMark> = None;
    let mut presents = 0_u32;
    let mut wakes = 0_u32;
    let mut drawn = Vec::new();
    let mut at = now;
    loop {
        let (fraction, moving) = turn.sample(at, Motion::Full);
        let showing = marks::ChromeMark::chevron(fraction);
        if tab_owes_frame(last_drawn, showing) {
            last_drawn = Some(showing);
            presents += 1;
            drawn.push(showing);
        }
        wakes += 1;
        if !moving {
            break;
        }
        at += pace::DEFAULT_FRAME_INTERVAL;
    }

    assert!(
        wakes > presents,
        "every single wake-up presented a frame ({presents} of {wakes}) — the debt \
             is being measured on something finer than the arrow is drawn at"
    );
    assert!(
        presents >= 5,
        "only {presents} frames of the turn were ever drawn — that is a swap \
             wearing an animation's clothes"
    );
    assert!(
        presents <= u32::from(marks::CHEVRON_TURN_STEPS),
        "a single turn asked for {presents} rasters, more than the quantum allows"
    );
    assert_eq!(
        drawn.first().copied(),
        Some(marks::ChromeMark::chevron(0.0)),
        "the turn is drawn from the arrow's resting angle"
    );
    assert_eq!(
        drawn.last().copied(),
        Some(marks::ChromeMark::chevron(1.0)),
        "and the frame it settles on is the terminal one — an arrow left at 175° is \
             the stranded-mid-breath bug wearing a different mark"
    );
    for pair in drawn.windows(2) {
        assert_ne!(pair[0], pair[1], "a present that redrew the same angle");
    }
}
