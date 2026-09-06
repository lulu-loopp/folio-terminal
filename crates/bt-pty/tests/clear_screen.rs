//! `Clear screen`, driven through a real ConPTY, because the defect it answers only exists there.
//!
//! `bt-term`'s own tests pin what this window does to its own grid, and they cannot see the
//! question these two ask. On Windows the child is not drawing on the screen this window shows: it
//! is drawing on the console host's buffer, and ConPTY sends this window only the *difference*
//! against what the host believes is displayed. A window that clears its own grid and tells nobody
//! has not cleared the screen — it has desynchronised from the only thing that knows what is on
//! it, and there is no prompt coming back, because as far as the host is concerned one is already
//! there.
//!
//! That is what the 2026-09-05 report was: a pane wiped by the menu's `Clear screen`, no prompt,
//! and the next characters typed appearing alone at the column the prompt used to end at. Recorded
//! here from a real pwsh, with the old implementation in place, is exactly that — the child said
//! nothing at all after the clear, and one keystroke later said
//! `ESC[?25l ESC[7;8H … x … ESC[7;9H ESC[?25h`: an absolute `CUP` to row 7, column 8, which is
//! where the seven-character prompt had ended, on a screen this window had already blanked.
//!
//! So both halves of the operation are asserted here together, because either alone is the defect:
//! this window keeps the row the cursor is on (`clear_screen_keeping_cursor_row`) and the host is
//! asked to keep the same one (`clear_host_buffer`, ConPTY's `keepCursorRow`).
//!
//! **Nothing here touches the user's own shell.** `-NoProfile` means their `$PROFILE` is not read,
//! the line editor is told to save no history, and the shell is spawned the way a pane spawns one.

#![cfg(windows)]

use std::{
    num::{NonZeroU16, NonZeroU32},
    sync::Arc,
    time::{Duration, Instant},
};

use bt_pty::{PtyCommand, PtySession, PtySize, WINDOWS_POWERSHELL};
use bt_term::DualPlaneSession;

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

/// RED GATE — **`Clear screen` leaves the prompt on the screen, and the console host agrees about
/// where it is.**
///
/// Three claims, and the first two are the report: the kept row is *on the screen* (it was not:
/// the pane was blank), it is the *top* row with everything above it gone, and — the claim only a
/// real ConPTY can answer — the next keystroke lands **on that row**, because the host was told
/// the same thing this window did. Before the fix the third one arrived as an absolute `CUP` to
/// the row the prompt used to be on, six rows below a screen that had nothing on it.
///
/// MUTATION: replace the two halves with `session.feed(b"\x1b[2J\x1b[H")` — the implementation
/// this replaces — and the first assertion goes red on an empty screen. Keep the local half and
/// drop `clear_host_buffer` and the first two pass while the last one puts the keystroke back on
/// row 6: the desynchronisation, isolated.
#[test]
fn clear_screen_keeps_the_prompt_and_the_host_agrees_where_it_is() {
    let mut pane = Pane::spawn();
    pane.reach_a_half_written_prompt();

    let before = pane.session.scrollback_line_count();
    let cursor = pane.session.terminal().cursor();
    assert!(
        cursor.row > 0,
        "the fixture must leave the prompt below the top row, or there is nothing to scroll out"
    );
    let scrolled_out = cursor.row as usize;

    // Exactly what `Runtime::clear_pane_screen` does, both halves of it.
    pane.session.clear_screen_keeping_cursor_row().unwrap();
    assert!(
        pane.pty.clear_host_buffer(true).unwrap(),
        "the pinned ConPTY sidecar exports the clear call this row is built on"
    );
    pane.settle();

    let rows = pane.rows();
    assert!(
        rows[0].contains(PROMPT_ON_SCREEN) && rows[0].contains(TYPED),
        "the prompt and the line being typed on it are still on the screen, at the top: {rows:?}"
    );
    assert!(
        rows[1..].iter().all(|row| row.trim_end().is_empty()),
        "and nothing else is: {rows:?}"
    );
    assert_eq!(
        pane.session.scrollback_line_count(),
        before + scrolled_out,
        "what was above it scrolled out; a clear deletes nothing"
    );

    // The host's half, and the only witness to it: where does the next keystroke land?
    pane.pty.write(b"!").unwrap();
    pane.wait_for_screen(&format!("{TYPED}!"));
    pane.settle();
    let rows = pane.rows();
    assert!(
        rows[0].contains(&format!("{TYPED}!")),
        "the keystroke landed on the row both sides kept: {rows:?}"
    );
    assert!(
        rows[1..].iter().all(|row| row.trim_end().is_empty()),
        "and nowhere else: {rows:?}"
    );

    pane.finish();
}

/// RED GATE — **what a shell's own clear actually sends, and that this window obeys both halves
/// of it.**
///
/// Recorded here rather than assumed, because the answer is not what the row above it does.
/// Windows PowerShell's `Clear-Host` reaches this window as `ESC[H` `ESC[2J` `ESC[3J` — home,
/// erase the display, **and delete the scrollback**. So a `cls` at a prompt is a *deletion* the
/// shell asked for, and the transcript going empty is this window obeying it; the erase before it
/// is the part that scrolls the screen away, which is why a mark anchored in one of those rows is
/// gone afterwards rather than left pointing at a row nobody has.
///
/// PowerShell 7 spells the same cmdlet `clear` and sends the same three, and a `clear` reading its
/// terminfo sends them in the same order. The deterministic half of this — all three spellings,
/// and the one difference between them — is `bt-term`'s
/// `every_shells_own_clear_scrolls_out_and_only_ed3_deletes`; what only a real ConPTY can say is
/// which of them a real shell picks, and it is this one.
///
/// MUTATION: assert `ESC[2J` alone and this passes while saying nothing — the ED3 is the byte
/// that decides whether the reader keeps their history, and a gate that does not name it would let
/// a change to the erase path look like a change to the deletion path.
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

    let sent = |needle: &[u8]| {
        pane.raw
            .windows(needle.len())
            .any(|window| window == needle)
    };
    assert!(
        sent(b"\x1b[2J"),
        "a shell's clear erases the display: {:?}",
        String::from_utf8_lossy(&pane.raw)
    );
    assert!(
        sent(b"\x1b[3J"),
        "and Windows PowerShell's asks for the scrollback as well: {:?}",
        String::from_utf8_lossy(&pane.raw)
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
