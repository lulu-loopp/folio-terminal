//! **`video_seat`, as the application drives it.** Tests whose first assertion is about
//! `video_seat`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{engines_settling_to, ledger_gate};

/// RED — **one recording is one seat, and a seat is at home on any of the
/// three surfaces** (user ruling 2026-08-28: *「视频在 hover 卡、固定浮窗、侧边
/// 预览 pane 三个表面用同一个引擎与同一张画、同一套手势」*; §7.44 ①).
///
/// The whole of "one model, three surfaces", stated as the two halves it
/// actually decomposes into:
///
/// ① **The model does not know which surface it is on.** The same door —
/// `VideoSeats::open` — is taken for a docked pane, a floating window and
/// the glance card, and what comes back answers the same questions with the
/// same types. There is no per-surface branch to get wrong because there is
/// no per-surface type.
///
/// ② **And they are still three pictures.** Three surfaces over one file
/// are three seats with three engines and three texture names, because they
/// are three rectangles that may be at three playheads. A build that
/// "shared" the seat between surfaces would show one picture in three places
/// and stop one of them stopping all three.
///
/// RED GATE ①: key the map by path instead of by surface and the second
/// `open` returns the first seat — the count is one, the keys are equal, and
/// two of the three surfaces are drawing somebody else's playhead. RED GATE
/// ②: leave the engines to `Drop` at the end of the test instead of
/// `shutdown_all` and `engines_outstanding` never comes back to where it
/// started, which is §7.42 ⑦'s counter noticing a leak this slice could
/// introduce three times over.
#[test]
fn a_video_is_one_seat_on_three_surfaces() {
    use bt_platform::video::engine::engines_outstanding;
    let _ledger = ledger_gate();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/assets/folio-video-test.mp4");
    let before = engines_outstanding();
    let mut seats = video_seat::VideoSeats::default();
    let surfaces = [
        PreviewSurface::Seat(LeafId {
            tab: TabId(1),
            seat: SeatId(2),
        }),
        PreviewSurface::Float(9),
        PreviewSurface::Peek,
    ];
    let now = Instant::now();
    for surface in surfaces {
        seats
            .open(surface, &fixture, now)
            .unwrap_or_else(|error| panic!("{surface:?} opens the fixture: {error:?}"));
    }
    // ① one door, one shape of answer, three surfaces.
    let mut keys = std::collections::BTreeSet::new();
    for surface in surfaces {
        let seat = seats
            .get(surface)
            .unwrap_or_else(|| panic!("{surface:?} holds a seat"));
        assert_eq!(seat.path(), fixture, "{surface:?}");
        // The same questions, answered for every surface alike.
        let _ = seat.state();
        let _ = seat.is_sounding();
        let _ = seat.presence(now, Motion::Full);
        keys.insert(seat.key().to_owned());
    }
    // ② three pictures, not one shared between three boxes.
    assert_eq!(keys.len(), 3, "three surfaces are three textures: {keys:?}");
    assert_eq!(
        engines_settling_to(before + 3),
        before + 3,
        "three surfaces are three decoders"
    );
    // And closing one closes exactly one.
    assert!(seats.close(surfaces[1]));
    assert!(seats.get(surfaces[1]).is_none());
    assert!(
        seats.get(surfaces[0]).is_some(),
        "and leaves the others alone"
    );
    assert_eq!(engines_outstanding(), before + 2);
    seats.shutdown_all();
    assert_eq!(
        engines_outstanding(),
        before,
        "and no engine outlives the surfaces it was opened for"
    );
}
