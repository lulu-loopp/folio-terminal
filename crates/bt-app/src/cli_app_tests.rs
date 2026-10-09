//! **`cli`, as the application drives it.** Tests whose first assertion is about
//! `cli`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{LedgerPane, document_key, text_buffer, wired};

/// PIN — **`OSC 1337;RequestAttention=` on the wire becomes an episode accounted to `src=osc`.**
///
/// The two halves of this block meet here and nowhere else: `bt-term` mints a *generation* from the
/// bytes, and the ledger mints an *episode* from the generation. Pinning them separately leaves the
/// join untested, and the join is where a level would be read as an edge — a program restating its
/// request once a second would then mint an episode once a second, and the badge would re-arm
/// forever.
///
/// The withdrawal is the other half of what makes this sequence the one the plan chose over four
/// alternatives: the program can take its own sentence back, and the ledger writes that down as the
/// program's doing rather than as anybody's answer.
#[test]
fn the_bytes_of_a_standing_request_become_one_episode_charged_to_the_osc_lane() {
    fn wrote(session: &mut bt_term::DualPlaneSession, bytes: &[u8]) -> Option<u64> {
        session.feed(bytes).expect("the session accepts bytes");
        session.status().attention_request
    }

    let mut session = wired();
    let mut pane = LedgerPane::new();
    let level = wrote(&mut session, b"\x1b]1337;RequestAttention=yes\x07");
    let rose = pane.ledger.weak_edge(level).expect("a rising edge");
    assert_eq!(
        pane.at(rose),
        ["mint tab=1 seat=SeatId(2) episode=1 src=osc gen=1 grounds=requested prev=-"]
    );
    let level = wrote(&mut session, b"\x1b]1337;RequestAttention=yes\x07");
    assert_eq!(
        pane.ledger.weak_edge(level),
        None,
        "a restatement is one program saying one thing twice"
    );
    assert_eq!(
        pane.away(),
        ["admit tab=1 seat=SeatId(2) ticket=0 episode=1 grounds=requested active=0 focused=0"],
        "a program that wants you is not a program that is blocked on you: no interruption"
    );
    let level = wrote(&mut session, b"\x1b]1337;RequestAttention=no\x07");
    let fell = pane.ledger.weak_edge(level).expect("a falling edge");
    assert_eq!(
        pane.at(fell),
        ["withdraw tab=1 seat=SeatId(2) ticket=0 episode=1 reason=program src=osc"]
    );
    assert_eq!(pane.state(), attention::State::Idle);
}

/// PIN — **`once` and `fireworks` reach the ledger as nothing at all.**
///
/// Both are on iTerm2's own list beside `yes` and `no`, which is what makes them worth a pin: the
/// tempting reading is "four values of one sequence, so four values of one state". `once` is a
/// one-shot and takes the bell's path inside the session; `fireworks` is a gesture this terminal
/// does not have. Neither is a level, so neither can produce an edge.
#[test]
fn the_one_shot_and_the_unimplemented_never_reach_the_ledger() {
    for payload in [
        &b"\x1b]1337;RequestAttention=once\x07"[..],
        &b"\x1b]1337;RequestAttention=fireworks\x07"[..],
    ] {
        let mut session = wired();
        let ledger = attention::AttentionLedger::default();
        session.feed(payload).expect("the session accepts bytes");
        assert_eq!(
            ledger.weak_edge(session.status().attention_request),
            None,
            "{payload:?}"
        );
    }
}

/// **The highlighting is on the width-free side of the key** (#49).
///
/// A resize changes `body_width_px` and nothing else, and `PreviewParseKey`
/// is what a re-parse is gated on — so a drag cannot re-walk a grammar. An
/// edit changes the revision, which is in the parse key, so it can and must.
/// This is the same argument [`MarkdownBlockIntrinsic`] was created for, made
/// again for the thing that was added to it.
#[test]
fn a_resize_cannot_re_walk_a_grammar_and_an_edit_must() {
    let mut buffer = text_buffer("main.rs", "fn main() {}\n");
    let narrow = document_key(&buffer, false, 400.0, 1.0);
    let wide = document_key(&buffer, false, 1200.0, 1.0);
    assert_ne!(narrow, wide, "the two widths are two documents");
    assert_eq!(
        narrow.parse, wide.parse,
        "but one parse — which is the half the highlighting hangs off"
    );
    buffer.edit_content(|content| {
        content.insert(0, 'p');
        true
    });
    assert_ne!(
        document_key(&buffer, false, 400.0, 1.0).parse,
        narrow.parse,
        "and an edit re-walks it"
    );
}
