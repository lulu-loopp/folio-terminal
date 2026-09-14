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
        aim_card_skip(&s, &mut p, 4, 1);
    }
    let top = transcript_tail(&s, 40, 4, p);
    let at_top = p;
    aim_card_skip(&s, &mut p, 4, -1);
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
    aim_card_skip(&s, &mut p, 4, 16);
    clamp_card_skip(&s, &mut p, 12);
    let grown = transcript_tail(&s, 40, 12, p);
    assert_eq!(p, 8, "a taller card must discard stored overshoot");
    clamp_card_skip(&s, &mut p, 12);
    aim_card_skip(&s, &mut p, 12, -1);
    let down = transcript_tail(&s, 40, 12, p);
    println!("grown first reverse stored={} visible={down:?}", p);
    assert_eq!(p, 7);
    assert_ne!(
        grown, down,
        "first reverse should move, even when resize left an unreachable pin"
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
    aim_card_skip(&s, &mut p, 4, 16);
    let grown = transcript_tail(&s, 40, 12, p).0;
    aim_card_skip(&s, &mut p, 12, -1);
    assert_eq!(p, 7, "reverse starts at visible maximum 8, not stored 16");
    assert_eq!(transcript_tail(&s, 40, 12, p).0[0], "row-01");
    assert_ne!(transcript_tail(&s, 40, 12, p).0, grown);
}

#[test]
fn projection_gate_clamps_height_growth_before_publishing() {
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
    thumbs.clamp_terminal_skip(tab, &demand(12, skip), &mut skip, start);
    assert_eq!(skip, 16);
    let ready = start + MIN_INTERVAL;
    thumbs.clamp_terminal_skip(tab, &demand(12, skip), &mut skip, ready);
    assert_eq!(skip, 8);
    thumbs.project(tab, &[demand(12, skip)], ready);
    let MiniSeatContent::Transcript { lines, .. } = &thumbs.seats(tab).unwrap()[&seat] else {
        panic!("terminal card must publish transcript rows");
    };
    assert_eq!(lines.first().unwrap(), "row-00");
    assert_eq!(lines.last().unwrap(), "row-11");
    aim_card_skip(&s, &mut skip, 12, -1);
    assert_eq!(skip, 7);
}
