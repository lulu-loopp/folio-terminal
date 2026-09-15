// T-CARD-RESTORE-NEXT59. Deterministic replay; no PTY child.
use super::*;
use std::{
    num::NonZeroU32,
    time::{Duration, Instant},
};
fn nz(n: u32) -> NonZeroU32 {
    NonZeroU32::new(n).unwrap()
}
// Fixture reused from card-anchor-audit-probe/audit.rs.
fn fixture(width: u32, height: u32) -> DualPlaneSession {
    let mut s = DualPlaneSession::new(nz(width), nz(height));
    for n in 0..60 {
        s.feed(
            format!(
                "conversation-{n:03}: {} END-{n:03}\r\n",
                "the user asks and the assistant explains a concrete step. ".repeat(7)
            )
            .as_bytes(),
        )
        .unwrap();
    }
    s.feed(b"status: working\r\ninput: ready\x1b[1A\r\x1b[2Kstatus: idle\x1b[1B\r")
        .unwrap();
    s
}
fn settle(s: &mut DualPlaneSession, width: u32, height: u32, at: Instant) {
    s.resize_at(nz(width), nz(height), at).unwrap();
    s.mark_pty_resize_requested_at(nz(width), nz(height), at);
    assert!(
        s.finish_resize_if_quiescent(at + Duration::from_millis(500))
            .unwrap()
    );
}
#[test]
fn wheel_at_top_reverses_on_first_notch() {
    let mut s = DualPlaneSession::new(nz(40), nz(30));
    for n in 0..20 {
        s.feed(format!("row-{n:02}\r\n").as_bytes()).unwrap();
    }
    let mut p = 0;
    clamp_card_skip(&s, &mut p, 4);
    for _ in 0..50 {
        aim_card_skip(&s, &mut p, 4, 1, card_trace::Card::untraced());
    }
    let top = transcript_tail(&s, 40, 4, p);
    let at_top = p;
    aim_card_skip(&s, &mut p, 4, -1, card_trace::Card::untraced());
    let down = transcript_tail(&s, 40, 4, p);
    println!(
        "top stored={at_top} reverse stored={} top={top:?} down={down:?}",
        p
    );
    assert_eq!(at_top, 16);
    assert_eq!(p, 15);
    assert_eq!(top.0, ["row-00", "row-01", "row-02", "row-03"]);
    assert_eq!(down.0, ["row-01", "row-02", "row-03", "row-04"]);
}
#[test]
fn height_growth_overshoot_and_reversal() {
    let mut s = DualPlaneSession::new(nz(40), nz(30));
    for n in 0..20 {
        s.feed(format!("row-{n:02}\r\n").as_bytes()).unwrap();
    }
    let mut p = 0;
    aim_card_skip(&s, &mut p, 4, 16, card_trace::Card::untraced());
    // The card grows to twelve rows. Nothing but the hand writes the number
    // (T-CARD-NO-PASSIVE-CLAMP), so the overshoot is spent by the draw and kept
    // by the leaf.
    let grown = transcript_tail(&s, 40, 12, p);
    assert_eq!(p, 16, "a taller card leaves the stored number alone");
    assert_eq!(
        grown,
        transcript_tail(&s, 40, 12, 8),
        "and shows the highest rows it can reach"
    );
    aim_card_skip(&s, &mut p, 12, -1, card_trace::Card::untraced());
    let down = transcript_tail(&s, 40, 12, p);
    println!("grown first reverse stored={} visible={down:?}", p);
    assert_eq!(
        p, 7,
        "the notch's entry clamp reverses from the visible eight"
    );
    assert_ne!(
        grown, down,
        "first reverse should move, even when a taller card left an unreachable pin"
    );
}
#[test]
fn replay_prior_real_conpty_bytes() {
    let mut s = DualPlaneSession::new(nz(240), nz(24));
    s.feed(include_bytes!(
        "../tests/fixtures/card-restore/child-raw-initial.bin"
    ))
    .unwrap();
    let mut p = 5;
    clamp_card_skip(&s, &mut p, 8);
    let before = transcript_tail(&s, 59, 8, p);
    let start = Instant::now();
    for (i, width) in [160, 240].into_iter().enumerate() {
        let at = start + Duration::from_secs(i as u64 * 2);
        s.resize_at(nz(width), nz(24), at).unwrap();
        clamp_card_skip(&s, &mut p, 8);
        s.mark_pty_resize_requested_at(nz(width), nz(24), at);
        clamp_card_skip(&s, &mut p, 8);
        s.feed_at(
            if width == 160 {
                include_bytes!("../tests/fixtures/card-restore/child-raw-160.bin")
            } else {
                include_bytes!("../tests/fixtures/card-restore/child-raw-240.bin")
            },
            at + Duration::from_millis(20),
        )
        .unwrap();
        clamp_card_skip(&s, &mut p, 8);
        assert!(
            s.finish_resize_if_quiescent(at + Duration::from_secs(1))
                .unwrap()
        );
        clamp_card_skip(&s, &mut p, 8);
    }
    let after = transcript_tail(&s, 59, 8, p);
    println!("replay skip={} before={before:?} after={after:?}", p);
    assert_eq!(
        before, after,
        "recorded 240 -> 160 -> 240 must restore every card row"
    );
}
#[test]
fn identical_repaint_with_intermediate_card_read() {
    let mut s = fixture(240, 24);
    let mut p = 5;
    clamp_card_skip(&s, &mut p, 8);
    let before = transcript_tail(&s, 59, 8, p);
    let at = Instant::now();
    s.resize_at(nz(231), nz(24), at).unwrap();
    s.mark_pty_resize_requested_at(nz(231), nz(24), at);
    clamp_card_skip(&s, &mut p, 8);
    let live = s.live_rows();
    let mut repaint = String::from("\x1b[H");
    for (i, r) in live.iter().enumerate() {
        let text = row_text(r);
        repaint += if r.continues { &text } else { text.trim_end() };
        if !r.continues && i + 1 < live.len() {
            repaint += "\r\n";
        }
    }
    let erase = (1..=24)
        .map(|r| format!("\x1b[{r};1H\x1b[2K"))
        .collect::<String>();
    s.feed_at(erase.as_bytes(), at + Duration::from_millis(10))
        .unwrap();
    clamp_card_skip(&s, &mut p, 8);
    s.feed_at(repaint.as_bytes(), at + Duration::from_millis(160))
        .unwrap();
    clamp_card_skip(&s, &mut p, 8);
    assert_eq!(live, s.live_rows(), "repaint restored captured source rows");
    settle(&mut s, 240, 24, at + Duration::from_millis(500));
    clamp_card_skip(&s, &mut p, 8);
    let after = transcript_tail(&s, 59, 8, p);
    println!("transient skip={} before={before:?} after={after:?}", p);
    assert_eq!(
        before, after,
        "a transient empty view must not latch a new position"
    );
}

#[test]
fn settled_round_trip_resting_card_keeps_tail() {
    let mut s = fixture(240, 24);
    let mut p = 0;
    clamp_card_skip(&s, &mut p, 8);
    let before = transcript_tail(&s, 59, 8, p);
    let at = Instant::now();
    for (i, w) in [160, 240].into_iter().enumerate() {
        settle(&mut s, w, 24, at + Duration::from_secs(i as u64));
        clamp_card_skip(&s, &mut p, 8);
        assert_eq!(p, 0);
    }
    assert_eq!(transcript_tail(&s, 59, 8, p), before);
    s.feed(b"\r\nnewest after resize").unwrap();
    clamp_card_skip(&s, &mut p, 8);
    assert_eq!(
        transcript_tail(&s, 59, 8, p).0.last().unwrap(),
        "newest after resize"
    );
}

#[test]
fn height_growth_first_reverse_before_next_draw_moves() {
    let mut s = DualPlaneSession::new(nz(40), nz(30));
    for n in 0..20 {
        s.feed(format!("row-{n:02}\r\n").as_bytes()).unwrap();
    }
    let mut p = 0;
    aim_card_skip(&s, &mut p, 4, 16, card_trace::Card::untraced());
    let grown = transcript_tail(&s, 40, 12, p).0;
    aim_card_skip(&s, &mut p, 12, -1, card_trace::Card::untraced());
    assert_eq!(p, 7, "reverse starts at visible maximum 8, not stored 16");
    assert_eq!(transcript_tail(&s, 40, 12, p).0[0], "row-01");
    assert_ne!(transcript_tail(&s, 40, 12, p).0, grown);
}

/// A taller card publishes the rows it can reach, and the leaf keeps its number
/// for the hand to spend (T-CARD-NO-PASSIVE-CLAMP).
///
/// The stored offset goes into the per-frame station by value, so "a frame never
/// moves it" is the signature and not an assertion; what is asserted here is the
/// half a reader sees — the picture the projection publishes, and where the next
/// notch starts from.
#[test]
fn a_taller_card_publishes_what_it_can_reach_and_keeps_its_number() {
    let mut s = DualPlaneSession::new(nz(40), nz(30));
    for n in 0..20 {
        s.feed(format!("row-{n:02}\r\n").as_bytes()).unwrap();
    }
    let mut thumbs = FocusThumbnails::default();
    let tab = TabId(1);
    let seat = SeatId(1);
    let mut skip = 16;
    let start = Instant::now();
    let demand = |rows, skip| SeatDemand {
        id: seat,
        columns: 40,
        rows,
        source: SeatSource::Terminal { session: &s, skip },
    };
    thumbs.project(tab, &[demand(4, skip)], start);
    // The old picture remains until the normal projection clock admits it.
    thumbs.trace_card_walk(
        tab,
        &demand(12, skip),
        skip,
        start,
        card_trace::Card::untraced(),
    );
    let ready = start + MIN_INTERVAL;
    thumbs.trace_card_walk(
        tab,
        &demand(12, skip),
        skip,
        ready,
        card_trace::Card::untraced(),
    );
    thumbs.project(tab, &[demand(12, skip)], ready);
    let MiniSeatContent::Transcript { lines, .. } = &thumbs.seats(tab).unwrap()[&seat] else {
        panic!("terminal card must publish transcript rows");
    };
    assert_eq!(lines.first().unwrap(), "row-00");
    assert_eq!(lines.last().unwrap(), "row-11");
    assert_eq!(
        skip, 16,
        "the published clamp is the draw's, not the leaf's"
    );
    aim_card_skip(&s, &mut skip, 12, -1, card_trace::Card::untraced());
    assert_eq!(skip, 7);
}

/// **The owner's own recording, as a test** (T-CARD-NO-PASSIVE-CLAMP;
/// `card.log` 150926.864 — `grid=24x21 rows=20 skip_before=7 skip_after=1
/// max=1`, and the same seat back at `35x29` a fifth of a second later).
///
/// The pane is an alternate screen because the recorded ones were: a TUI keeps
/// its whole transcript on the live grid, so the grid a window wears for an
/// instant on its way to a display of another scale is the *whole* of what the
/// card can reach in that instant. A frame that walked there used to write that
/// instant's maximum into the leaf, and the number never came back — which is
/// the owner's "off by a few rows" when it was four, and "not anchored at all"
/// when it was seven.
#[test]
fn a_transient_grid_moves_the_drawn_row_and_never_the_stored_one() {
    fn paint(session: &mut DualPlaneSession) {
        session.feed(b"\x1b[2J\x1b[H").unwrap();
        let screen = (1..=28)
            .map(|n| format!("A{n:03}"))
            .collect::<Vec<_>>()
            .join("\r\n");
        session.feed(screen.as_bytes()).unwrap();
    }
    let mut s = DualPlaneSession::new(nz(40), nz(30));
    s.feed(b"\x1b[?1049h").unwrap();
    paint(&mut s);
    let thumbs = FocusThumbnails::default();
    // Seven rows above the tail: the recording's own number, and a number this
    // pane can reach twenty times over.
    let skip = 7;
    let before = transcript_tail(&s, 24, 4, skip).0;
    assert_eq!(before, ["A018", "A019", "A020", "A021"]);
    let start = Instant::now();
    // The window is on its way to the other display, and the pane is given a
    // grid that belongs to no display.
    s.resize_at(nz(24), nz(5), start).unwrap();
    s.mark_pty_resize_requested_at(nz(24), nz(5), start);
    {
        let demand = SeatDemand {
            id: SeatId(1),
            columns: 24,
            rows: 4,
            source: SeatSource::Terminal { session: &s, skip },
        };
        // The frame that used to cost the reader their place. It takes the
        // number by value: there is no longer anything for it to write.
        thumbs.trace_card_walk(TabId(1), &demand, skip, start, card_trace::Card::untraced());
    }
    let transient = transcript_tail(&s, 24, 4, skip).0;
    assert_eq!(
        transient,
        transcript_tail(&s, 24, 4, 1).0,
        "the draw clamps to the one row this grid can reach"
    );
    assert_ne!(
        transient, before,
        "the transient grid really did take the picture away"
    );
    // And the window arrives, at a grid the pane can hold its screen on again.
    s.resize_at(nz(40), nz(30), start + Duration::from_millis(400))
        .unwrap();
    paint(&mut s);
    assert_eq!(
        transcript_tail(&s, 24, 4, skip).0,
        before,
        "the card comes back showing what it showed, because nothing wrote its number"
    );
}

/// **The first frames after a restore** (T-CARD-NO-PASSIVE-CLAMP; `card.log`
/// 5404.664 — `rows=20 skip_before=14 skip_after=0 max=0` over a pane whose
/// shell had not printed a byte yet).
///
/// A restored card's number is the reader's from the last run, and an empty pane
/// is not an argument against it: the walk can reach nothing, the draw shows
/// nothing, and both of those are true only until the shell speaks.
#[test]
fn a_walk_over_a_pane_that_has_not_spoken_keeps_the_persisted_number() {
    let mut s = DualPlaneSession::new(nz(40), nz(30));
    let thumbs = FocusThumbnails::default();
    let skip = 14;
    {
        let demand = SeatDemand {
            id: SeatId(1),
            columns: 40,
            rows: 6,
            source: SeatSource::Terminal { session: &s, skip },
        };
        thumbs.trace_card_walk(
            TabId(1),
            &demand,
            skip,
            Instant::now(),
            card_trace::Card::untraced(),
        );
    }
    assert!(
        transcript_tail(&s, 40, 6, skip).0.is_empty(),
        "a pane with nothing on it draws nothing"
    );
    for n in 0..25 {
        s.feed(format!("row-{n:02}\r\n").as_bytes()).unwrap();
    }
    let arrived = transcript_tail(&s, 40, 6, skip).0;
    assert_eq!(
        arrived.first().unwrap(),
        "row-05",
        "the restored card is where the reader left it: {arrived:?}"
    );
    assert_eq!(arrived.last().unwrap(), "row-10");
}
