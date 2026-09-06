//! `Clear screen`, driven through a real ConPTY, because the defect it answers only exists there.
//!
//! `bt-term`'s own tests pin what this window does to its own grid, and they cannot see the
//! question this one asks. On Windows the child is not drawing on the screen this window shows: it
//! is drawing on the console host's buffer, and ConPTY sends this window only the *difference*
//! against what the host believes is displayed. A window that clears its own grid and tells nobody
//! has not cleared the screen — it has desynchronised from the only thing that knows what is on
//! it, and there is no prompt coming back, because as far as the host is concerned one is already
//! there.
//!
//! That is what the 2026-09-05 report was: a pane wiped by the menu's `Clear screen`, no prompt,
//! and the next characters typed appearing alone at the column the prompt used to end at. Recorded
//! from a real pwsh with the first implementation in place, the child said nothing at all after
//! the clear, and one keystroke later said `ESC[?25l ESC[7;8H … x … ESC[7;9H ESC[?25h`: an
//! absolute `CUP` to row 7, column 8, where the seven-character prompt had ended.
//!
//! **What this file asserts, and what it deliberately does not (2026-09-06).** The first version
//! of this gate typed one character straight after the clear and required it to land on the kept
//! row. It passed here every time — thirty runs idle, twenty under load — and failed on the first
//! GitHub `windows-2025` run, with the screen the report describes. It cannot be otherwise: the
//! clear travels on the pseudoconsole's **signal** pipe and the keystroke on its **input** pipe,
//! two pipes with two readers and no ordering between them, and the loser state is permanent
//! because a host that has cleared emits nothing to correct the paint that beat it. A gate whose
//! answer depends on which of two threads a four-core runner schedules first is not a gate.
//!
//! So the ordering is gone from the *product* rather than papered over here: the host is asked
//! first, and **its answer picks what this window is allowed to move**
//! (`bt_term::HostScreen`). Told, the rows above the prompt scroll away and the prompt becomes the
//! top row; untold, nothing moves at all and the screen is cleared around the row the host still
//! believes in. Either way no keystroke can land on a row the two disagree about, so that is what
//! is asserted here — on both paths, whichever this machine has.
//!
//! **Nothing here touches the user's own shell.** `-NoProfile` means their `$PROFILE` is not read,
//! the line editor is told to save no history, and the shell is spawned the way a pane spawns one.

#![cfg(windows)]

use std::{
    num::{NonZeroU16, NonZeroU32},
    sync::Arc,
    time::{Duration, Instant},
};

use bt_pty::{ConPtySource, PtyCommand, PtySession, PtySize, WINDOWS_POWERSHELL, conpty_source};
use bt_term::{DualPlaneSession, HostScreen};

/// The prompt the probe installs, so "a prompt is on the screen" is read rather than assumed.
const PROMPT: &str = "BTCLR> ";
/// The same prompt as a terminal row holds it: the cells past the last written one are blank, not
/// spaces, so the needle is the prompt without its trailing separator.
const PROMPT_ON_SCREEN: &str = "BTCLR>";
/// What the reader has typed and not submitted — the thing a clear must not take away.
const TYPED: &str = "half-written";

/// `bt-pty`'s `PROBE_SILENCE_BUDGET` to the second, and for its reasons: a wall-clock total
/// measures the machine, a silence budget measures the child.
const SILENCE_BUDGET: Duration = Duration::from_secs(30);
/// The backstop for the one shape silence cannot catch — a child that talks forever.
const CEILING: Duration = Duration::from_secs(180);
/// How long the child must say nothing before a burst is finished.
const QUIET: Duration = Duration::from_millis(150);

const COLUMNS: u16 = 80;
const ROWS: u16 = 12;

struct Pane {
    pty: PtySession,
    session: DualPlaneSession,
    raw: Vec<u8>,
    started: Instant,
    last_output: Instant,
}

impl Pane {
    /// One Windows PowerShell on a real pty, with a terminal in front of it.
    ///
    /// `powershell.exe` rather than `pwsh.exe`: it is part of Windows, so this gate never turns
    /// into a gate that quietly passes on a machine that does not have its subject.
    fn spawn() -> Self {
        let columns = NonZeroU16::new(COLUMNS).unwrap();
        let rows = NonZeroU16::new(ROWS).unwrap();
        let command = PtyCommand::interactive_shell(WINDOWS_POWERSHELL)
            .arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-NoExit")
            .arg("-Command")
            .arg(format!(
                "Set-PSReadLineOption -HistorySaveStyle SaveNothing; \
                 function global:prompt {{ '{PROMPT}' }}"
            ));
        let pty = PtySession::spawn(command, PtySize::cells(columns, rows), Arc::new(|| {}))
            .unwrap_or_else(|error| {
                panic!("Windows PowerShell starts on a supported host: {error:?}")
            });
        Self {
            pty,
            session: DualPlaneSession::new(
                NonZeroU32::new(u32::from(COLUMNS)).unwrap(),
                NonZeroU32::new(u32::from(ROWS)).unwrap(),
            ),
            raw: Vec::new(),
            started: Instant::now(),
            last_output: Instant::now(),
        }
    }

    fn pump_once(&mut self) -> bool {
        let bytes = self.pty.read_output();
        if bytes.is_empty() {
            return false;
        }
        self.last_output = Instant::now();
        self.raw.extend_from_slice(&bytes);
        self.session.feed(&bytes).unwrap();
        for reply in self.session.take_pty_writes() {
            self.pty.write(&reply).unwrap();
        }
        true
    }

    fn rows(&self) -> Vec<String> {
        self.session.terminal().visible_text()
    }

    fn screen_has(&self, needle: &str) -> bool {
        self.rows().iter().any(|row| row.contains(needle))
    }

    fn wait_for_screen(&mut self, needle: &str) {
        loop {
            self.pump_once();
            if self.screen_has(needle) {
                return;
            }
            self.give_up_if_stalled(&format!("{needle:?} on the screen"));
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Pump until `needle` is **off** the screen.
    ///
    /// The other half of `wait_for_screen`, and the only honest way to wait for a clear: what a
    /// clear produces is an absence, and every needle that was on the screen before it is still
    /// there the instant after the request goes out. A `settle` alone cannot stand in for this —
    /// a child that has not started answering yet is already quiet.
    fn wait_for_screen_without(&mut self, needle: &str) {
        loop {
            self.pump_once();
            if !self.screen_has(needle) {
                return;
            }
            self.give_up_if_stalled(&format!("{needle:?} to leave the screen"));
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn settle(&mut self) {
        loop {
            self.pump_once();
            if self.last_output.elapsed() >= QUIET {
                return;
            }
            self.give_up_if_stalled("the child to fall quiet");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn give_up_if_stalled(&self, waiting_for: &str) {
        let silent_for = self.last_output.elapsed();
        if silent_for < SILENCE_BUDGET && self.started.elapsed() < CEILING {
            return;
        }
        panic!(
            "gave up waiting for {waiting_for} after {:?}, the last {:?} of it with the child \
             silent; screen {:?}",
            self.started.elapsed(),
            silent_for,
            self.rows()
        );
    }

    /// A prompt, two commands run through it, and a third line half typed and not submitted.
    fn reach_a_half_written_prompt(&mut self) {
        self.wait_for_screen(PROMPT_ON_SCREEN);
        self.settle();
        for run in 1..=2u32 {
            // Split in the request and joined in the answer, on `bt-pty`'s own marker discipline:
            // a marker spelled the same way in both would be matched by the echo of the request.
            self.pty
                .write(format!("Write-Output ('BT_CLR' + '_{run}')\r").as_bytes())
                .unwrap();
            self.wait_for_screen(&format!("BT_CLR_{run}"));
            self.settle();
        }
        self.pty.write(TYPED.as_bytes()).unwrap();
        self.wait_for_screen(TYPED);
        self.settle();
    }

    fn finish(mut self) {
        // Ask before killing: `Ctrl+C` abandons the half-written line, `exit` shuts the shell's
        // own console down, and `PtySession::drop` is then holding a process that has already gone.
        let _ = self.pty.write(b"\x03exit\r");
        let _ = self.pty.shutdown();
    }
}

/// RED GATE — **`Clear screen` leaves the prompt where it can be seen, and moves rows only when
/// the console host was told to move its own.**
///
/// Three claims, and all three are answered without racing anything. The pseudoconsole's answer
/// is checked against the implementation this process actually loaded, so neither path can pass by
/// being skipped: the packaged host exports the clear and must say so, the operating system's
/// inbox host exports none and must say that. Then the screen is read for the shape that answer
/// requires — the prompt at the top with what was above it in the transcript, or the prompt
/// exactly where it was with the screen cleared around it and the transcript untouched.
///
/// MUTATION: hand `HostScreen::Cleared` to the session regardless of the answer and the untold arm
/// goes red on a prompt that moved to the top of a screen the host still addresses by the old
/// numbers — which is the row the 2026-09-05 report's keystroke landed on.
#[test]
fn clear_screen_keeps_the_prompt_and_moves_rows_only_when_the_host_was_told() {
    let mut pane = Pane::spawn();
    pane.reach_a_half_written_prompt();

    let before = pane.session.scrollback_line_count();
    let cursor = pane.session.terminal().cursor();
    assert!(
        cursor.row > 0,
        "the fixture must leave the prompt below the top row, or there is nothing to scroll out"
    );
    let kept_row = cursor.row as usize;

    // Exactly what `Runtime::clear_pane_screen` does, in its order: the host first, because its
    // answer is what decides the shape.
    let source = conpty_source();
    let told = pane.pty.clear_host_buffer(true).unwrap();
    match &source {
        ConPtySource::Sidecar { .. } => assert!(
            told,
            "the packaged pseudoconsole exports ConptyClearPseudoConsole, and this is it: {source}"
        ),
        ConPtySource::System => assert!(
            !told,
            "kernel32 exports no clear at all, so there is no host to tell: {source}"
        ),
    }
    let host = if told {
        HostScreen::Cleared
    } else {
        eprintln!(
            "BT_CLEAR_PROBE host-half=absent {source} \
             (the inbox pseudoconsole has no clear call; the untold shape is under test instead)"
        );
        HostScreen::Untold
    };
    pane.session.clear_screen_keeping_cursor_row(host).unwrap();
    pane.settle();

    let rows = pane.rows();
    match host {
        HostScreen::Cleared => {
            assert!(
                rows[0].contains(PROMPT_ON_SCREEN) && rows[0].contains(TYPED),
                "the prompt and the line being typed on it are at the top now: {rows:?} ({source})"
            );
            assert!(
                rows[1..].iter().all(|row| row.trim_end().is_empty()),
                "and nothing else is on the screen: {rows:?}"
            );
            assert_eq!(
                pane.session.scrollback_line_count(),
                before + kept_row,
                "what was above it scrolled out; a clear deletes nothing"
            );
        }
        HostScreen::Untold => {
            assert!(
                rows[kept_row].contains(PROMPT_ON_SCREEN) && rows[kept_row].contains(TYPED),
                "the prompt is still on the row the host believes in: {rows:?} ({source})"
            );
            assert!(
                rows.iter()
                    .enumerate()
                    .all(|(row, text)| row == kept_row || text.trim_end().is_empty()),
                "and the screen around it is clear: {rows:?}"
            );
            assert_eq!(
                pane.session.scrollback_line_count(),
                before,
                "nothing moved, so nothing entered the transcript"
            );
        }
    }

    // Whichever path this machine took, the two sides agree about where the cursor is — which is
    // the whole of what the report was about, and the one thing a keystroke could have shown.
    let cursor = pane.session.terminal().cursor();
    assert_eq!(
        (cursor.row as usize, cursor.column),
        (
            match host {
                HostScreen::Cleared => 0,
                HostScreen::Untold => kept_row,
            },
            cursor.column
        ),
        "the cursor is on the kept row, at the coordinates the host also holds"
    );

    pane.finish();
}

/// RED GATE — **what a shell's own clear actually sends, and that this window obeys the half of
/// it that is a deletion.**
///
/// Recorded rather than assumed, because the answer is not what the row above it does and is not
/// even the same on both pseudoconsoles. Measured 2026-09-05/06 from Windows PowerShell's
/// `Clear-Host`:
///
/// * through the **packaged** host it arrives as `ESC[H` `ESC[2J` `ESC[3J` — home, erase the
///   display, and delete the scrollback;
/// * through the operating system's **inbox** host there is no `ESC[2J` at all: it repaints, a
///   `ESC[H` and then one `ESC[K` per row, and sends `ESC[3J` after it.
///
/// What both spellings share is the `ESC[3J`, and that is the byte that decides whether the reader
/// keeps their history: a `cls` at a prompt is a *deletion the shell asked for*, and the
/// transcript going empty is this window obeying it. So the shared claim is asserted for both and
/// the erase spelling for whichever host this process loaded — a gate that named only one would go
/// red on a machine that is merely configured differently.
///
/// The deterministic half of this — all three spellings of a clear, and the one difference between
/// them — is `bt-term`'s `only_the_ed3_in_a_shells_clear_deletes_anything`; what only a real
/// ConPTY can say is which of them a real shell picks, and it is this.
///
/// MUTATION: assert the erase alone and this passes while saying nothing — the ED3 is the byte the
/// reader's history hangs on, and a gate that does not name it would let a change to the erase
/// path look like a change to the deletion path.
#[test]
fn a_shells_own_clear_arrives_as_an_erase_and_a_delete_and_both_are_obeyed() {
    let mut pane = Pane::spawn();
    pane.wait_for_screen(PROMPT_ON_SCREEN);
    pane.settle();
    pane.pty
        .write(b"Write-Output ('BT_CLS' + '_RAN')\r")
        .unwrap();
    pane.wait_for_screen("BT_CLS_RAN");
    pane.settle();

    pane.raw.clear();
    pane.pty.write(b"Clear-Host\r").unwrap();
    // A clear produces an absence, so the wait is for one: the marker leaving the screen is the
    // only moment at which the request can be said to have arrived.
    pane.wait_for_screen_without("BT_CLS_RAN");
    pane.wait_for_screen(PROMPT_ON_SCREEN);
    pane.settle();

    let source = conpty_source();
    let sent = |needle: &[u8]| {
        pane.raw
            .windows(needle.len())
            .any(|window| window == needle)
    };
    let said = String::from_utf8_lossy(&pane.raw).into_owned();
    match &source {
        ConPtySource::Sidecar { .. } => assert!(
            sent(b"\x1b[2J"),
            "the packaged host passes the shell's erase through: {said:?}"
        ),
        ConPtySource::System => assert!(
            sent(b"\x1b[H") && sent(b"\x1b[K"),
            "the inbox host repaints instead of erasing, a home and one erase-to-end per row: \
             {said:?}"
        ),
    }
    assert!(
        sent(b"\x1b[3J"),
        "and both spellings carry the deletion the shell asked for ({source}): {said:?}"
    );
    assert_eq!(
        pane.session.scrollback_line_count(),
        0,
        "so the transcript goes, because that is what was asked for"
    );

    let rows = pane.rows();
    assert!(
        rows[0].contains(PROMPT_ON_SCREEN),
        "the shell drew its next prompt at the top, which is why nobody notices this one: {rows:?}"
    );
    assert!(
        !pane.screen_has("BT_CLS_RAN"),
        "and the screen it cleared is gone: {rows:?}"
    );

    pane.finish();
}
