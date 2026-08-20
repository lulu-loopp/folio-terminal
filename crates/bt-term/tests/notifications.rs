//! `OSC 9` and `OSC 777;notify` — the two desktop-notification sequences (DESIGN §7.6).
//!
//! Everything here is about *what the terminal understood*, and nothing here raises anything: a
//! `DualPlaneSession` files requests and `take_notifications` hands them over. Whether one becomes
//! a toast is `bt-app`'s question and is pinned in its own suite.

use std::num::NonZeroU32;

use bt_term::{DualPlaneSession, ProgressState, TerminalNotification};

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn session() -> DualPlaneSession {
    DualPlaneSession::new(nz(80), nz(8))
}

fn notified(reports: &[&[u8]]) -> Vec<TerminalNotification> {
    let mut session = session();
    for report in reports {
        session.feed(report).unwrap();
    }
    session.take_notifications()
}

fn one(report: &[u8]) -> Option<TerminalNotification> {
    let mut all = notified(&[report]);
    assert!(all.len() <= 1, "expected at most one notification: {all:?}");
    all.pop()
}

fn titled(title: Option<&str>, body: &str) -> Option<TerminalNotification> {
    Some(TerminalNotification {
        title: title.map(str::to_owned),
        body: body.to_owned(),
    })
}

/// PIN — **`OSC 9` is two protocols sharing a number, and the number decides which.**
///
/// ConEmu put a numbered subcommand slot on `OSC 9`; iTerm2 put a free-text notification on the
/// same sequence. The rule is Ghostty's, the only terminal that ships both: a first field that is
/// entirely digits is ConEmu's, everything else is a message.
///
/// Red gate: read the whole body as a message and `9;4;1;42` raises a toast saying `4;1;42` while
/// the progress ring never moves; read the whole body as a subcommand and no `OSC 9` notification
/// can ever be sent at all.
#[test]
fn osc_9_reads_a_leading_number_as_conemu_and_everything_else_as_a_message() {
    assert_eq!(
        one(b"\x1b]9;build finished\x07"),
        titled(None, "build finished"),
        "the plain iTerm2 shape"
    );
    assert_eq!(
        one(b"\x1b]9;\xe5\xae\x8c\xe6\x88\x90 \xe2\x9c\x93\x1b\\"),
        titled(None, "完成 ✓"),
        "ST terminates it and the body is UTF-8"
    );

    // `9;4` stays progress, and is not also a notification.
    let mut session = session();
    session.feed(b"\x1b]9;4;1;42\x07").unwrap();
    assert_eq!(session.status().progress, Some(ProgressState::Normal(42)));
    assert!(
        session.take_notifications().is_empty(),
        "the progress report is not also a message"
    );

    // Every other ConEmu number is dropped rather than read as text. `9;9;<cwd>` is a shell
    // saying where it is; a toast reading "9;C:\\src" would be a message nobody wrote.
    for quiet in [
        b"\x1b]9;1;500\x07".as_slice(),
        b"\x1b]9;3;tab name\x07".as_slice(),
        b"\x1b]9;9;C:\\src\x07".as_slice(),
        b"\x1b]9;12\x07".as_slice(),
        // Nothing to say at all.
        b"\x1b]9;\x07".as_slice(),
    ] {
        assert_eq!(one(quiet), None, "report {quiet:?}");
    }

    // A body that merely *contains* a digit, or starts with one that is not followed by a
    // separator, is text — the slot is "digits then `;`", not "starts with a digit".
    assert_eq!(
        one(b"\x1b]9;3 tests failed\x07"),
        titled(None, "3 tests failed")
    );
    assert_eq!(
        one(b"\x1b]9;;4;1;42\x07"),
        titled(None, ";4;1;42"),
        "an empty first field is not a number"
    );
}

/// PIN — **`OSC 777;notify` splits title from body at the FIRST semicolon and keeps the rest.**
///
/// foot's rule, and the only one anybody implements: "neither title nor body is escaped … split
/// title from body at the first ';', with any remaining ';' characters treated as part of body".
/// A body with a semicolon in it is the ordinary case — `make: *** [all] Error 2; see log`.
///
/// Red gate: split on every separator and the second half of every such body disappears.
#[test]
fn osc_777_notify_splits_once_and_ignores_every_other_verb() {
    assert_eq!(
        one(b"\x1b]777;notify;cargo;build finished\x07"),
        titled(Some("cargo"), "build finished")
    );
    assert_eq!(
        one(b"\x1b]777;notify;make;Error 2; see the log\x1b\\"),
        titled(Some("make"), "Error 2; see the log"),
        "the second semicolon belongs to the body"
    );
    assert_eq!(
        one(b"\x1b]777;notify;title only\x07"),
        titled(Some("title only"), ""),
        "a title with no body is still a message"
    );
    assert_eq!(
        one(b"\x1b]777;notify;;body only\x07"),
        titled(None, "body only"),
        "an empty title falls back to the pane's own name, exactly as OSC 9's absent one does"
    );

    for quiet in [
        // Somebody else's 777 extension. Its second field is not a name for anything.
        b"\x1b]777;precmd;zsh\x07".as_slice(),
        b"\x1b]777;notify\x07".as_slice(),
        b"\x1b]777;notify;;\x07".as_slice(),
        b"\x1b]777;\x07".as_slice(),
    ] {
        assert_eq!(one(quiet), None, "report {quiet:?}");
    }
}

/// PIN — **`OSC 7` and `OSC 777` share their first byte and do not collide.**
///
/// The one thing the prefix table can get wrong. `7` alone is a candidate for both; `7;` is
/// exactly the working directory; `77` has already ruled the working directory out.
///
/// Red gate: decide on the leading byte instead of the whole prefix and one of the two sequences
/// swallows the other.
#[test]
fn osc_7_and_osc_777_do_not_take_each_others_bytes() {
    let mut session = session();
    session
        .feed(b"\x1b]7;file:///D:/Developer\x07\x1b]777;notify;t;b\x07\x1b]7;file:///C:/\x07")
        .unwrap();
    assert_eq!(
        session.take_notifications(),
        vec![TerminalNotification {
            title: Some("t".to_owned()),
            body: "b".to_owned(),
        }]
    );
    assert_eq!(
        session
            .working_directory()
            .map(std::path::Path::to_path_buf),
        Some(std::path::PathBuf::from("C:\\"))
    );
}

/// PIN — **a bare BEL is not a notification.**
///
/// No terminal surveyed raises a system toast from `0x07`: Windows Terminal's `bellStyle` plays a
/// sound or flashes the taskbar, foot's BEL notify command ships as `none`, and iTerm2's bell is
/// an in-app cue. Folio's answer is the same one it already gave — the bell latches the tab's own
/// attention dot and reaches the operating system through nothing.
#[test]
fn a_bell_latches_attention_and_asks_for_no_notification() {
    let mut session = session();
    session.feed(b"\x07").unwrap();
    assert!(session.status().bell_latched);
    assert!(session.take_notifications().is_empty());
}

/// PIN — **what is not a message is dropped, not repaired.**
///
/// Three refusals, each for its own reason: a payload past the limit is dropped whole rather than
/// truncated (half a sentence is a sentence nobody wrote), bytes that are not UTF-8 have no lossy
/// reading worth showing a person, and the surrounding text stream is untouched in every case.
#[test]
fn oversized_and_undecodable_payloads_are_dropped_whole() {
    let long = format!("\x1b]9;{}\x07", "a".repeat(4096));
    assert_eq!(one(long.as_bytes()), None);

    let mut malformed = b"\x1b]777;notify;t;".to_vec();
    malformed.extend_from_slice(&[0xff, 0xfe]);
    malformed.push(0x07);
    assert_eq!(one(&malformed), None);

    let mut session = session();
    session
        .feed(b"before\x1b]9;hello\x07middle\x1b]777;notify;a;b\x1b\\after")
        .unwrap();
    assert!(session.terminal().visible_text()[0].contains("beforemiddleafter"));
    assert_eq!(session.take_notifications().len(), 2);
}

/// PIN — **the same bytes decide the same way however the pipe chops them.**
///
/// A PTY read can end anywhere, including between `77` and `7`, and a scanner that made a
/// decision on a partial prefix would answer differently on a slow machine.
#[test]
fn notification_decisions_are_invariant_at_every_chunk_boundary() {
    let stream =
        b"a\x1b]9;one\x07b\x1b]777;notify;two;three\x1b\\c\x1b]7;file:///D:/x\x07d\x1b]9;4;1;7\x07";
    let whole = {
        let mut session = session();
        session.feed(stream).unwrap();
        (session.take_notifications(), session.status().progress)
    };
    assert_eq!(whole.0.len(), 2, "fixture carries two messages");
    for split in 0..=stream.len() {
        let mut session = session();
        session.feed(&stream[..split]).unwrap();
        session.feed(&stream[split..]).unwrap();
        assert_eq!(
            (session.take_notifications(), session.status().progress),
            whole,
            "split at byte {split}"
        );
    }
    let mut session = session();
    for byte in stream {
        session.feed(std::slice::from_ref(byte)).unwrap();
    }
    assert_eq!(
        (session.take_notifications(), session.status().progress),
        whole,
        "one byte at a time"
    );
}

/// PIN — **a burst is bounded and a drain is a drain.**
///
/// The queue is emptied once per turn of the application's loop, so what this bounds is one
/// program shouting inside one read. Sixteen are kept, the rest of the burst is dropped, and the
/// oldest are the ones kept — the first thing a program said is the thing it said first.
#[test]
fn a_flood_is_capped_and_taking_empties_the_queue() {
    let mut session = session();
    let flood: Vec<u8> = (0..64)
        .flat_map(|index| format!("\x1b]9;message {index}\x07").into_bytes())
        .collect();
    session.feed(&flood).unwrap();
    let taken = session.take_notifications();
    assert_eq!(taken.len(), 16);
    assert_eq!(taken[0].body, "message 0", "the oldest are the ones kept");
    assert_eq!(taken[15].body, "message 15");
    assert!(
        session.take_notifications().is_empty(),
        "a notification is spent by being read"
    );

    session.feed(b"\x1b]9;after\x07").unwrap();
    assert_eq!(session.take_notifications().len(), 1, "the queue re-opens");
}
