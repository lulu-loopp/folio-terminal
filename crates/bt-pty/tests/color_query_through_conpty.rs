//! What a program inside a real ConPTY learns when it asks the terminal what
//! colour it is standing on.
//!
//! `bt-term`'s own tests already pin the *protocol*: feed `OSC 11;?` to a
//! `TerminalAdapter` carrying a palette and the right bytes come back out of
//! `take_pty_writes`. What they cannot see is the transport, and the transport
//! is where this question actually lives: between the program's `OSC 11;?` and
//! our answer sits a console host that renders a screen rather than forwarding a
//! stream, and every sequence that survives that crossing survives because
//! somebody taught it to (`shell_integration_osc133.rs` says the same thing about
//! `OSC 133`).
//!
//! So this spawns Windows PowerShell the way a pane spawns it, tells the terminal
//! in front of it that the window is wearing a *light* canvas, and has the child
//! ask. Two facts are read off one run, because they fail separately:
//!
//! 1. **The question arrives.** `\x1b]11;?` appears in the bytes the pty handed
//!    us. If the console host answered on its own behalf this is absent, and no
//!    palette we hold could have mattered.
//! 2. **Our answer is the canvas in force.** The bytes queued for the child spell
//!    the background the palette carries — not a constant, not the scheme this
//!    process was born in.

#![cfg(windows)]

use std::{
    num::{NonZeroU16, NonZeroU32},
    sync::Arc,
    time::{Duration, Instant},
};

use bt_pty::{PtyCommand, PtySession, PtySize, WINDOWS_POWERSHELL};
use bt_term::{DualPlaneSession, TerminalCanvas, TerminalPalette};

/// A background no scheme in this product wears, so an answer carrying it can
/// only have come from the palette this test handed over.
const PROBE_BACKGROUND: [u8; 3] = [0xfa, 0xf9, 0xf5];
const PROBE_FOREGROUND: [u8; 3] = [0x37, 0x35, 0x2f];
const PROBE_CURSOR: [u8; 3] = [0x21, 0x20, 0x1e];

const SILENCE_BUDGET: Duration = Duration::from_secs(30);
const CEILING: Duration = Duration::from_secs(180);

/// What the child prints once it has finished listening, so the probe knows the
/// run is over. Split in the command and joined in the output for
/// `shell_integration_osc133.rs`'s reason: a marker spelled the same way in both
/// is matched by the child repeating the request rather than obeying it.
const DONE_MARKER: &str = "BT_OSC11_DONE";

fn light_palette() -> TerminalPalette {
    TerminalPalette {
        canvas: TerminalCanvas::Light,
        background: PROBE_BACKGROUND,
        foreground: PROBE_FOREGROUND,
        cursor: PROBE_CURSOR,
        ansi: [[0x11, 0x22, 0x33]; 16],
    }
}

/// The child: ask for the background, listen for the answer, print it as hex.
///
/// The listening is `[Console]::ReadKey` and not `Console.In`, because the reply
/// reaches a console client as input records rather than as a line, and a
/// line-oriented read would sit waiting for an `Enter` that is never coming.
fn asking_startup() -> String {
    format!(
        "$e=[char]27; $bel=[char]7; \
         [Console]::Out.Write(\"$e]11;?$bel\"); [Console]::Out.Flush(); \
         $s=''; $deadline=(Get-Date).AddSeconds(8); \
         while((Get-Date) -lt $deadline -and -not $s.Contains($bel)) {{ \
           if([Console]::KeyAvailable) {{ $s += [Console]::ReadKey($true).KeyChar }} \
           else {{ Start-Sleep -Milliseconds 10 }} }}; \
         $hex=($s.ToCharArray() | ForEach-Object {{ '{{0:x2}}' -f [int]$_ }}) -join ''; \
         Write-Output ('BT_OSC11' + '_DONE=' + $hex)"
    )
}

struct Probe {
    pty: PtySession,
    session: DualPlaneSession,
    raw: Vec<u8>,
    answered: Vec<u8>,
    started: Instant,
    last_output: Instant,
}

impl Probe {
    fn spawn(palette: TerminalPalette) -> Self {
        let columns = NonZeroU16::new(80).unwrap();
        let rows = NonZeroU16::new(20).unwrap();
        let command = PtyCommand::interactive_shell(WINDOWS_POWERSHELL)
            .arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-NoExit")
            .arg("-Command")
            .arg(asking_startup());
        let pty = PtySession::spawn(command, PtySize::cells(columns, rows), Arc::new(|| {}))
            .expect("Windows PowerShell starts on a supported host");
        let mut session = DualPlaneSession::new(
            NonZeroU32::new(u32::from(columns.get())).unwrap(),
            NonZeroU32::new(u32::from(rows.get())).unwrap(),
        );
        session.set_color_palette(palette);
        Self {
            pty,
            session,
            raw: Vec::new(),
            answered: Vec::new(),
            started: Instant::now(),
            last_output: Instant::now(),
        }
    }

    fn pump_once(&mut self) {
        let bytes = self.pty.read_output();
        if bytes.is_empty() {
            return;
        }
        self.last_output = Instant::now();
        self.raw.extend_from_slice(&bytes);
        self.session.feed(&bytes).unwrap();
        for reply in self.session.take_pty_writes() {
            self.answered.extend_from_slice(&reply);
            self.pty.write(&reply).unwrap();
        }
    }

    fn screen_has(&self, needle: &str) -> bool {
        self.session
            .terminal()
            .visible_text()
            .iter()
            .any(|row| row.contains(needle))
    }

    fn run(mut self) -> (Vec<u8>, Vec<u8>, Vec<String>) {
        loop {
            self.pump_once();
            if self.screen_has(DONE_MARKER) {
                break;
            }
            let silent_for = self.last_output.elapsed();
            assert!(
                silent_for < SILENCE_BUDGET && self.started.elapsed() < CEILING,
                "gave up waiting for {DONE_MARKER} after {:?}, the last {:?} of it silent, \
                 {} bytes read; screen {:?}",
                self.started.elapsed(),
                silent_for,
                self.raw.len(),
                self.session.terminal().visible_text()
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let screen = self.session.terminal().visible_text();
        let raw = std::mem::take(&mut self.raw);
        let answered = std::mem::take(&mut self.answered);
        let _ = self.pty.write(b"exit\r");
        let _ = self.pty.shutdown();
        (raw, answered, screen)
    }
}

fn position(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// RED FORM of the 2026-08-29 report: a light window whose programs are told they
/// are standing on a dark one.
#[test]
fn the_background_a_program_asks_for_is_the_one_in_force() {
    let (raw, answered, screen) = Probe::spawn(light_palette()).run();
    let done = screen
        .iter()
        .find(|row| row.contains(DONE_MARKER))
        .cloned()
        .unwrap_or_default();
    eprintln!(
        "BT_OSC11_PROBE conpty={:?} query_reached_us={} answered={:?} child_read={done:?}",
        bt_pty::conpty_source(),
        position(&raw, b"\x1b]11;?").is_some(),
        String::from_utf8_lossy(&answered),
    );
    assert!(
        position(&raw, b"\x1b]11;?").is_some(),
        "the child's colour query has to reach the terminal, or nothing we hold could \
         answer it; raw {:?}",
        String::from_utf8_lossy(&raw)
    );
    let expected = format!(
        "\x1b]11;rgb:{0:02x}{0:02x}/{1:02x}{1:02x}/{2:02x}{2:02x}",
        PROBE_BACKGROUND[0], PROBE_BACKGROUND[1], PROBE_BACKGROUND[2]
    );
    assert!(
        position(&answered, expected.as_bytes()).is_some(),
        "the answer has to spell the canvas in force; answered {:?}",
        String::from_utf8_lossy(&answered)
    );
    let expected_hex: String = expected
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert!(
        done.contains(&expected_hex),
        "and the child has to read it back; it read {done:?}, we sent {expected_hex:?}"
    );
}
