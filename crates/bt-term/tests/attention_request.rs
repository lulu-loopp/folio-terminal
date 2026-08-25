//! `OSC 1337;RequestAttention=` — the weak tier of the attention ledger, at the session seam
//! (`docs/plans/attention/plan.md` §11.1.1, §12.2).
//!
//! Everything here is about the **generation number** a session hands upward, because that number
//! is the whole reason this tier is not a bit. A bit can say "a program is asking"; it cannot say
//! "the program is asking *again*", and it cannot survive the sentence the whole plan turns on —
//! *the user has answered, and the program has not withdrawn*. The layer above records an answer
//! as a watermark, and a watermark is only meaningful against a number that never repeats.
//!
//! Nothing here decides anything about a dot, a ticket or a toast. That is `bt-app`'s ledger and
//! is pinned in its own suite; this file pins what the bytes did to this session.

use std::num::NonZeroU32;

use bt_term::DualPlaneSession;

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn session() -> DualPlaneSession {
    DualPlaneSession::new(nz(80), nz(8))
}

fn fed(session: &mut DualPlaneSession, bytes: &[u8]) -> Option<u64> {
    session.feed(bytes).unwrap();
    session.status().attention_request
}

/// PIN — **a generation is minted on the `None → Some` edge and on no other byte.**
///
/// A program that restates `yes` is saying one thing repeatedly. Minting on the restatement would
/// hand the layer above a fresh request on every repeat, and since the layer above mints an
/// *episode* whenever an unanswered credential appears, a program restating once a second would
/// produce a badge that re-arms once a second — the 2026-08-21 defect, moved to a new pillar.
///
/// Red gate: mint on every `yes` and the second assertion below reads `Some(2)`.
#[test]
fn request_attention_yes_mints_only_on_the_rising_edge() {
    let mut session = session();
    assert_eq!(
        fed(&mut session, b"\x1b]1337;RequestAttention=yes\x07"),
        Some(1)
    );
    assert_eq!(
        fed(&mut session, b"\x1b]1337;RequestAttention=yes\x07"),
        Some(1),
        "a restatement is the same request"
    );
    assert_eq!(
        fed(&mut session, b"\x1b]1337;RequestAttention=yes\x07"),
        Some(1),
        "and so is the next one"
    );
}

/// PIN — **withdrawal clears the live value and leaves the cursor alone.**
///
/// This is invariant I2 in the one place it is actually carried: after `yes, no, yes` the second
/// request must be generation **2**, never 1. If the counter wound back, an answer to the first
/// request would leave a watermark of 1 standing, and the second request — a genuinely new one —
/// would arrive at 1 and be read as already answered. **That is a swallowed request, the one class
/// of failure this whole block exists to remove.**
///
/// Red gate: reset the counter on `=no` and the last assertion reads `Some(1)`.
#[test]
fn request_attention_no_clears_the_value_without_winding_the_counter_back() {
    let mut session = session();
    assert_eq!(
        fed(&mut session, b"\x1b]1337;RequestAttention=yes\x07"),
        Some(1)
    );
    assert_eq!(
        fed(&mut session, b"\x1b]1337;RequestAttention=no\x07"),
        None
    );
    assert_eq!(
        fed(&mut session, b"\x1b]1337;RequestAttention=no\x07"),
        None,
        "a second withdrawal is idempotent"
    );
    assert_eq!(
        fed(&mut session, b"\x1b]1337;RequestAttention=yes\x07"),
        Some(2),
        "the next request is strictly younger than every number the last one was compared against"
    );
}

/// PIN — **`once` is an event and takes the bell's path; it never enters the ledger.**
///
/// iTerm2's `once` is by definition a one-shot, and a one-shot request for attention is what a
/// bell already is. Giving it a second implementation would put the same sentence in two voices,
/// and — worse — a one-shot that could mint a generation would be a ticket nobody can retire,
/// which is the original defect (§0) restated in the new vocabulary.
#[test]
fn request_attention_once_latches_the_bell_and_stays_out_of_the_ledger() {
    let mut session = session();
    let status = {
        session
            .feed(b"\x1b]1337;RequestAttention=once\x07")
            .unwrap();
        session.status()
    };
    assert!(status.bell_latched, "it rang");
    assert_eq!(status.attention_request, None, "and it asserted nothing");
}

/// PIN — **`fireworks` and every unknown value change nothing at all.**
///
/// Not the ledger, not the bell, not the grid. A value this terminal has no gesture for is a
/// request it did not implement, and implementing "something" instead is how a terminal starts
/// answering requests programs never made.
#[test]
fn unimplemented_request_attention_values_change_nothing() {
    for payload in [
        &b"\x1b]1337;RequestAttention=fireworks\x07"[..],
        &b"\x1b]1337;RequestAttention=maybe\x07"[..],
        &b"\x1b]1337;RequestAttention=\x07"[..],
    ] {
        let mut session = session();
        session.feed(payload).unwrap();
        let status = session.status();
        assert_eq!(status.attention_request, None, "{payload:?}");
        assert!(!status.bell_latched, "{payload:?}");
    }
}

/// PIN (`attention` plan §10.9, pin 2) — **a look spends the latches and does not unsay the
/// request.**
///
/// `clear_attention` is what "the user has seen this tab" does to a bell and to a failing exit
/// code: both are one-shot facts, and a look is a way of spending them. A standing
/// `RequestAttention=yes` is not one of those — it is a sentence the program is *still saying*.
/// Clearing it here would be this terminal withdrawing a request on the program's behalf, and the
/// pane would go quiet while the program was still waiting.
///
/// Red gate: add `attention_request = None` to `clear_attention` and the last assertion fails —
/// which is exactly the shortcut the plan names ("do not save a function by putting the new field
/// in the old clear").
#[test]
fn clearing_the_latches_does_not_withdraw_a_standing_request() {
    let mut session = session();
    session
        .feed(b"\x1b]1337;RequestAttention=yes\x07\x07")
        .unwrap();
    assert!(session.status().bell_latched);
    session.clear_attention();
    let status = session.status();
    assert!(!status.bell_latched, "the latch is spent by the look");
    assert_eq!(
        status.attention_request,
        Some(1),
        "and the program is still asking"
    );
}

/// PIN — **the request survives being cut anywhere in the byte stream.**
///
/// Feeds arrive in whatever sizes a pipe hands over, and the dispatch now turns on a key that can
/// be split down the middle. One sequence is one request however many reads it took.
#[test]
fn a_request_split_across_feeds_is_one_request() {
    let stream = b"\x1b]1337;RequestAttention=yes\x07";
    for split in 0..=stream.len() {
        let mut session = session();
        session.feed(&stream[..split]).unwrap();
        session.feed(&stream[split..]).unwrap();
        assert_eq!(
            session.status().attention_request,
            Some(1),
            "split at byte {split}"
        );
    }
}
