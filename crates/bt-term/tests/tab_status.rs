use std::{num::NonZeroU32, time::Instant};

use bt_term::{DualPlaneSession, ProgressState};

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn session() -> DualPlaneSession {
    DualPlaneSession::new(nz(80), nz(8))
}

#[test]
fn osc_9_4_progress_matrix_accepts_bel_and_st_clamps_and_ignores_malformed_reports() {
    let mut session = session();
    let cases = [
        (b"0".as_slice(), None),
        (b"1;42".as_slice(), Some(ProgressState::Normal(42))),
        (b"2".as_slice(), Some(ProgressState::Error(None))),
        (b"3".as_slice(), Some(ProgressState::Indeterminate)),
        (b"4".as_slice(), Some(ProgressState::Paused(None))),
    ];
    for (payload, expected) in cases {
        for terminator in [b"\x07".as_slice(), b"\x1b\\".as_slice()] {
            let mut report = b"\x1b]9;4;".to_vec();
            report.extend_from_slice(payload);
            report.extend_from_slice(terminator);
            session.feed(&report).unwrap();
            assert_eq!(session.status().progress, expected, "report {report:?}");
        }
    }

    session.feed(b"\x1b]9;4;1;101\x1b\\").unwrap();
    assert_eq!(session.status().progress, Some(ProgressState::Normal(100)));
    session.feed(b"\x1b]9;4;2;250\x07").unwrap();
    assert_eq!(
        session.status().progress,
        Some(ProgressState::Error(Some(100)))
    );
    session.feed(b"\x1b]9;4;4;-9\x1b\\").unwrap();
    assert_eq!(
        session.status().progress,
        Some(ProgressState::Paused(Some(0)))
    );

    session.feed(b"\x1b]9;4;1;37\x07").unwrap();
    for malformed in [
        b"\x1b]9;4\x07".as_slice(),
        b"\x1b]9;4;\x07".as_slice(),
        b"\x1b]9;4;1\x1b\\".as_slice(),
        b"\x1b]9;4;x;50\x07".as_slice(),
        b"\x1b]9;4;5;50\x1b\\".as_slice(),
        b"\x1b]9;4;2;oops\x07".as_slice(),
    ] {
        session.feed(malformed).unwrap();
        assert_eq!(
            session.status().progress,
            Some(ProgressState::Normal(37)),
            "malformed report {malformed:?} changed state"
        );
    }
    session.feed(b"before\x1b]9;4;bogus\x07after").unwrap();
    assert!(session.terminal().visible_text()[0].contains("beforeafter"));

    session
        .feed(b"\x1b]133;C\x07\x1b]9;4;1;55\x07\x1b]133;D;0\x07")
        .unwrap();
    assert_eq!(session.status().progress, Some(ProgressState::Normal(55)));
    session.feed(b"\x1b]133;C\x07").unwrap();
    assert_eq!(session.status().progress, None);
}

#[test]
fn bare_bell_latches_attention_but_osc_bel_terminators_do_not() {
    let mut session = session();
    session.feed(b"\x1b]0;title\x07").unwrap();
    assert!(!session.status().bell_latched);

    session.feed(b"\x07").unwrap();
    assert!(session.status().bell_latched);

    session.clear_attention();
    session.feed(b"\x1bPqpayload\x07more\x1b\\").unwrap();
    assert!(!session.status().bell_latched);
}

#[test]
fn osc_133_failure_latch_tracks_exit_code_and_clears_on_next_command() {
    let mut session = session();
    session.feed(b"\x1b]133;D\x07").unwrap();
    assert_eq!(session.status().failure_exit_code, None);

    session.feed(b"\x1b]133;C\x07\x1b]133;D;17\x1b\\").unwrap();
    assert_eq!(session.status().failure_exit_code, Some(17));
    session.feed(b"\x1b]133;D\x07").unwrap();
    assert_eq!(session.status().failure_exit_code, Some(17));

    session.feed(b"\x1b]133;C\x07").unwrap();
    assert_eq!(session.status().failure_exit_code, None);

    session.feed(b"\x1b]133;D;0\x07").unwrap();
    assert_eq!(session.status().failure_exit_code, None);
}

#[test]
fn osc_133_working_follows_the_full_b_to_c_to_d_trajectory() {
    let mut session = session();
    assert!(!session.status().working);

    session.feed(b"\x1b]133;A\x07").unwrap();
    assert!(!session.status().working);
    session.feed(b"prompt\x1b]133;B\x07").unwrap();
    assert!(!session.status().working);
    session.feed(b"command\x1b]133;C\x07").unwrap();
    assert!(session.status().working);
    session.feed(b"output\x1b]133;D;0\x07").unwrap();
    assert!(!session.status().working);
}

#[test]
fn clear_attention_clears_only_latches_and_preserves_progress() {
    let mut session = session();
    session
        .feed(b"\x1b]9;4;4;63\x07\x07\x1b]133;D;9\x07")
        .unwrap();
    assert_eq!(
        session.status().progress,
        Some(ProgressState::Paused(Some(63)))
    );
    assert!(session.status().bell_latched);
    assert_eq!(session.status().failure_exit_code, Some(9));

    session.clear_attention();
    assert_eq!(
        session.status().progress,
        Some(ProgressState::Paused(Some(63)))
    );
    assert!(!session.status().bell_latched);
    assert_eq!(session.status().failure_exit_code, None);
}

#[test]
fn published_revision_strictly_increases_for_every_published_frame() {
    let mut session = session();
    let mut projection = session.new_projection(session.layout_key());
    let frame = session.viewport_frame(&mut projection).unwrap();
    let initial = session.published_revision();

    session.record_published_frame(&frame, Instant::now());
    let first = session.published_revision();
    assert_eq!(first, initial + 1);
    assert_eq!(session.status().published_revision, first);

    session.record_published_frame(&frame, Instant::now());
    assert_eq!(session.published_revision(), first + 1);
}

/// **§7.1.5b/c — `alt-screen 一律不记`, applied to the busy channel.**
///
/// The command-mark ledger has always refused to record the `A/B/C/D` a full-screen program emits
/// on the alternate screen: those markers describe *its* canvas, not this session's command
/// history. `working` was the one field in the taxonomy that still listened to them, so a TUI that
/// says `C` on its own screen could set the whole session running, and one that says `D` there
/// could declare the shell's real, still-unfinished command over.
///
/// The corner this pins deliberately: **`D` counts only on the primary screen.** A command started
/// at the shell's prompt and then handed to a full-screen program stays `working` for as long as
/// the program holds the screen — including across a `D` the program itself emits — and is retired
/// by the shell's own `D`, back on the primary screen, where the command really ends. The
/// alternative (letting an alt-screen `D` clear it) would have any TUI that speaks shell
/// integration silently close a command it did not open.
#[test]
fn working_is_not_flipped_from_the_alternate_screen() {
    let mut session = session();
    assert!(!session.status().alternate_screen);

    // A full-screen program takes the grid and runs its own command cycle on it.
    session.feed(b"\x1b[?1049h").unwrap();
    assert!(
        session.status().alternate_screen,
        "the mode bit is read off the vendor terminal, not remembered here"
    );
    session
        .feed(b"\x1b]133;A\x07\x1b]133;B\x07\x1b]133;C\x07")
        .unwrap();
    assert!(
        !session.status().working,
        "a `C` on the alternate screen describes the TUI's canvas, not this session's command"
    );
    session.feed(b"\x1b]133;D;0\x07").unwrap();
    assert!(!session.status().working);

    // Back on the primary screen the shell's own markers are authoritative again.
    session.feed(b"\x1b[?1049l").unwrap();
    assert!(!session.status().alternate_screen);
    session
        .feed(b"\x1b]133;A\x07PS> \x1b]133;B\x07claude\x1b]133;C\x07")
        .unwrap();
    assert!(
        session.status().working,
        "the shell said this command started"
    );

    // The command it started is a full-screen program. It holds the screen for its whole life,
    // and nothing it does there ends the command the shell is still running.
    session.feed(b"\x1b[?1049h").unwrap();
    assert!(session.status().working);
    assert!(session.status().alternate_screen);
    session.feed(b"\x1b]133;C\x07\x1b]133;D;0\x07").unwrap();
    assert!(
        session.status().working,
        "`D` is only accepted on the primary screen, so a TUI cannot close the shell's command"
    );

    // It exits, and the shell — on the primary screen, where the command really ends — retires it.
    session.feed(b"\x1b[?1049l").unwrap();
    assert!(session.status().working);
    assert!(!session.status().alternate_screen);
    session.feed(b"\x1b]133;D;0\x07").unwrap();
    assert!(
        !session.status().working,
        "the primary screen's `D` still clears a command that spent its life on the alternate one"
    );
}
