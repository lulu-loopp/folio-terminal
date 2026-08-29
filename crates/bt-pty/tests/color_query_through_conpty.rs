//! What a program inside a real ConPTY learns when it asks the terminal what
//! colour it is standing on.
//!
//! `bt-term`'s own tests already pin the *protocol*: feed `OSC 11;?` to a
//! `TerminalAdapter` carrying a palette and the right bytes come back out of
//! `take_pty_writes`. `bt-app`'s `the_colours_a_program_is_told_are_the_ones_the_
//! glass_is_wearing` pins the *mapping*, on all three canvases a window can wear
//! — light, dark, and a `BT_BG` override. What neither can see is the
//! **transport**, and the transport is where this question actually lives:
//! between the program's `OSC 11;?` and our answer sits a console host that
//! renders a screen rather than forwarding a stream, and every sequence that
//! survives that crossing survives because somebody taught it to
//! (`shell_integration_osc133.rs` says the same thing about `OSC 133`).
//!
//! So this spawns Windows PowerShell the way a pane spawns it, tells the
//! terminal in front of it that the window is wearing a light canvas, has the
//! child ask, then **moves the window to the dark canvas and has it ask again**.
//! Four facts are read off one run, because they fail separately:
//!
//! 1. **The question arrives.** `\x1b]11;?` appears in the bytes the pty handed
//!    us. If the console host had answered on its own behalf this would be
//!    absent, and no palette we hold could have mattered.
//! 2. **The answer is the canvas in force** — not a constant, and not the scheme
//!    this process was born in.
//! 3. **A second question after a theme change is answered with the new colour.**
//!    A program that asks again gets today's answer; one that asked once and
//!    never subscribed to DEC 2031 keeps the answer it was given, and that is the
//!    whole of what a terminal can do for it (`docs/DESIGN.md` §7.46).
//! 4. **The terminator is the asker's.** The first query is BEL-terminated and
//!    the second `ST`-terminated, and each answer comes back wearing the one it
//!    was asked with — which is what a program reading with a fixed-terminator
//!    parser depends on. Real askers use both: `codex`'s
//!    `tui/src/terminal_probe.rs` writes `\x1b]10;?\x1b\\\x1b]11;?\x1b\\`.

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
const LIGHT_BACKGROUND: [u8; 3] = [0xfa, 0xf9, 0xf5];
/// And its opposite number, for the same reason.
const DARK_BACKGROUND: [u8; 3] = [0x14, 0x16, 0x1a];

const SILENCE_BUDGET: Duration = Duration::from_secs(30);
const CEILING: Duration = Duration::from_secs(180);

/// What the child prints after each round, so the probe knows where it is.
///
/// Split in the command and joined in the output for
/// `shell_integration_osc133.rs`'s reason: a marker spelled the same way in both
/// is matched by the child repeating the request rather than by it obeying.
const ONE_MARKER: &str = "BT_OSC11_ONE=";
const TWO_MARKER: &str = "BT_OSC11_TWO=";

/// The byte the probe sends to say "the window has moved, ask again".
const GO: &[u8] = b"g";

fn palette(canvas: TerminalCanvas, background: [u8; 3]) -> TerminalPalette {
    TerminalPalette {
        canvas,
        background,
        foreground: [0x37, 0x35, 0x2f],
        cursor: [0x21, 0x20, 0x1e],
        ansi: [[0x11, 0x22, 0x33]; 16],
    }
}

/// The child: ask for the background, listen for the answer, print it as hex;
/// wait to be told the window moved; ask again with the other terminator.
///
/// The listening is `[Console]::ReadKey` and not `Console.In`, because the reply
/// reaches a console client as input records rather than as a line, and a
/// line-oriented read would sit waiting for an `Enter` that is never coming.
///
/// **No double quotes anywhere in it**: this whole script crosses a Windows
/// command line as one argument, and the backslash-quote rules that argument is
/// re-parsed by would eat the `ESC \` the second query is terminated with.
const ASKING_STARTUP: &str = "\
$e=[char]27; $bel=[char]7; $st=$e+'\\'; \
[Console]::Out.Write($e+']11;?'+$bel); [Console]::Out.Flush(); \
$s=''; $d=(Get-Date).AddSeconds(8); \
while((Get-Date) -lt $d -and -not ($s.Contains($bel) -or $s.Contains($st))) { \
  if([Console]::KeyAvailable){$s+=[Console]::ReadKey($true).KeyChar} else {Start-Sleep -Milliseconds 10} }; \
Write-Output ('BT_OSC11' + '_ONE=' + ((($s.ToCharArray() | ForEach-Object { '{0:x2}' -f [int]$_ }) -join ''))); \
$go=$false; $d=(Get-Date).AddSeconds(120); \
while(-not $go -and (Get-Date) -lt $d) { \
  if([Console]::KeyAvailable){ if([Console]::ReadKey($true).KeyChar -eq 'g'){$go=$true} } else {Start-Sleep -Milliseconds 10} }; \
[Console]::Out.Write($e+']11;?'+$st); [Console]::Out.Flush(); \
$s=''; $d=(Get-Date).AddSeconds(8); \
while((Get-Date) -lt $d -and -not ($s.Contains($bel) -or $s.Contains($st))) { \
  if([Console]::KeyAvailable){$s+=[Console]::ReadKey($true).KeyChar} else {Start-Sleep -Milliseconds 10} }; \
Write-Output ('BT_OSC11' + '_TWO=' + ((($s.ToCharArray() | ForEach-Object { '{0:x2}' -f [int]$_ }) -join '')))";

struct Probe {
    pty: PtySession,
    session: DualPlaneSession,
    raw: Vec<u8>,
    answered: Vec<u8>,
    started: Instant,
    last_output: Instant,
}

/// Everything one run produced: the child's own bytes, ours, and its screen.
struct Run {
    raw: Vec<u8>,
    answered: Vec<u8>,
    screen: Vec<String>,
}

impl Run {
    fn line(&self, marker: &str) -> String {
        self.screen
            .iter()
            .find(|row| row.contains(marker))
            .cloned()
            .unwrap_or_default()
    }
}

impl Probe {
    fn spawn() -> Self {
        let columns = NonZeroU16::new(80).unwrap();
        let rows = NonZeroU16::new(20).unwrap();
        let command = PtyCommand::interactive_shell(WINDOWS_POWERSHELL)
            .arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-NoExit")
            .arg("-Command")
            .arg(ASKING_STARTUP);
        let pty = PtySession::spawn(command, PtySize::cells(columns, rows), Arc::new(|| {}))
            .expect("Windows PowerShell starts on a supported host");
        let mut session = DualPlaneSession::new(
            NonZeroU32::new(u32::from(columns.get())).unwrap(),
            NonZeroU32::new(u32::from(rows.get())).unwrap(),
        );
        session.set_color_palette(palette(TerminalCanvas::Light, LIGHT_BACKGROUND));
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
        if !bytes.is_empty() {
            self.last_output = Instant::now();
            self.raw.extend_from_slice(&bytes);
            self.session.feed(&bytes).unwrap();
        }
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

    fn wait_for(&mut self, needle: &str) {
        loop {
            self.pump_once();
            if self.screen_has(needle) {
                return;
            }
            let silent_for = self.last_output.elapsed();
            assert!(
                silent_for < SILENCE_BUDGET && self.started.elapsed() < CEILING,
                "gave up waiting for {needle} after {:?}, the last {:?} of it silent, \
                 {} bytes read; screen {:?}",
                self.started.elapsed(),
                silent_for,
                self.raw.len(),
                self.session.terminal().visible_text()
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn run(mut self) -> Run {
        self.wait_for(ONE_MARKER);
        // The window moves to the other canvas, exactly as a theme switch moves
        // it: one call on the drain path, and then the child is told to ask
        // again.
        self.session
            .set_color_palette(palette(TerminalCanvas::Dark, DARK_BACKGROUND));
        self.pty.write(GO).unwrap();
        self.wait_for(TWO_MARKER);
        let screen = self.session.terminal().visible_text();
        let raw = std::mem::take(&mut self.raw);
        let answered = std::mem::take(&mut self.answered);
        let _ = self.pty.write(b"exit\r");
        let _ = self.pty.shutdown();
        Run {
            raw,
            answered,
            screen,
        }
    }
}

fn position(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The answer bytes for one background, without the terminator.
fn answer_for(background: [u8; 3]) -> String {
    format!(
        "\x1b]11;rgb:{0:02x}{0:02x}/{1:02x}{1:02x}/{2:02x}{2:02x}",
        background[0], background[1], background[2]
    )
}

fn as_hex(bytes: &str) -> String {
    bytes.bytes().map(|byte| format!("{byte:02x}")).collect()
}

/// RED FORM of the 2026-08-29 report: a light window whose programs are told
/// they are standing on a dark one.
#[test]
fn the_background_a_program_asks_for_is_the_one_in_force() {
    let run = Probe::spawn().run();
    let one = run.line(ONE_MARKER);
    let two = run.line(TWO_MARKER);
    eprintln!(
        "BT_OSC11_PROBE conpty={:?} query_reached_us={} answered={:?} one={one:?} two={two:?}",
        bt_pty::conpty_source(),
        position(&run.raw, b"\x1b]11;?").is_some(),
        String::from_utf8_lossy(&run.answered),
    );
    assert!(
        position(&run.raw, b"\x1b]11;?").is_some(),
        "the child's colour query has to reach the terminal, or nothing we hold could \
         answer it; raw {:?}",
        String::from_utf8_lossy(&run.raw)
    );

    // The canvas in force, and its own terminator: BEL asked, BEL answered.
    let light = format!("{}\x07", answer_for(LIGHT_BACKGROUND));
    assert!(
        position(&run.answered, light.as_bytes()).is_some(),
        "the first answer has to spell the light canvas, BEL-terminated as it was asked; \
         answered {:?}",
        String::from_utf8_lossy(&run.answered)
    );
    assert!(
        one.contains(&as_hex(&light)),
        "and the child has to read it back; it read {one:?}, we sent {:?}",
        as_hex(&light)
    );

    // The window moved. A program that asks again is told where it is now — and
    // `ST` asked is `ST` answered.
    let dark = format!("{}\x1b\\", answer_for(DARK_BACKGROUND));
    assert!(
        position(&run.answered, dark.as_bytes()).is_some(),
        "the second answer has to spell the canvas the window moved to, ST-terminated as it \
         was asked; answered {:?}",
        String::from_utf8_lossy(&run.answered)
    );
    assert!(
        two.contains(&as_hex(&dark)),
        "and the child has to read that back too; it read {two:?}, we sent {:?}",
        as_hex(&dark)
    );
}
