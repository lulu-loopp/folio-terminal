//! What the kitty keyboard protocol and xterm's modifyOtherKeys can reach through a real ConPTY
//! (`docs/plans/design/keyboard-protocol-2026-09-29.md` §1 and §6.3).
//!
//! `bt-term`'s own tests pin the protocol against a `TerminalAdapter`; `bt-app`'s pin the bytes
//! the encoder writes. Neither can see the **transport**, and on Windows the transport decides
//! which programs the protocol can reach at all: between a program and Folio sits a console host
//! whose input parser knows no `CSI … u`. The design note reads the Windows Terminal source and
//! concludes four things; each is an assertion here, in its own test, because they fail
//! separately:
//!
//! 1. **The request crosses.** A child that writes `CSI ? u` and `CSI > 1 u` reaches Folio with
//!    both, the session's flags become 1, and the reply reaches the child.
//! 2. **A key-record reader** (PowerShell's `[Console]::ReadKey`, as PSReadLine, cmd, .NET and
//!    Codex on Windows read) sent `CSI 13;5u` reads seven characters and no `Enter`+`Control`;
//!    sent `CSI 27;5;13~` reads nothing; sent the win32-input-mode record pair for Ctrl+Enter
//!    reads `Enter` with `Control` — the transport T-KEYBOARD-RECORDS will use.
//! 3. **A VT-input reader** (the same host after `ENABLE_VIRTUAL_TERMINAL_INPUT`, as WSL's relay
//!    reads) sent either form reads exactly its bytes.
//! 4. **A native Node raw-stdin reader** — the stand-in for Claude Code on Windows — reads the
//!    reply to its query and, sent `CSI 13;5u`, exactly those seven bytes. This one needs `node`
//!    on `PATH` and **fails** without it, saying so; CI's `conpty` job selects it out by name
//!    (`--skip a_native_node_reader`), and the ticket's acceptance run is where it runs.
//!
//! Every child is spawned the way a pane spawns it, `PtyCommand` through `PtySession`, and every
//! PowerShell script crosses the command line as `-EncodedCommand`, so nothing in it is re-parsed
//! by the argument-quoting rules the colour probe has to write around.

#![cfg(windows)]
#![allow(clippy::disallowed_methods)]

use std::{
    ffi::OsString,
    num::{NonZeroU16, NonZeroU32},
    sync::Arc,
    time::{Duration, Instant},
};

use bt_pty::{PtyCommand, PtySession, PtySize, WINDOWS_POWERSHELL};
use bt_term::DualPlaneSession;

const SILENCE_BUDGET: Duration = Duration::from_secs(30);
const CEILING: Duration = Duration::from_secs(180);
/// How long a probe keeps pumping after its last write before it sends the next one, so that a
/// sequence ConPTY would deliver late is not mistaken for one it dropped.
const SETTLE: Duration = Duration::from_millis(400);

/// Wide enough that no marker line the children print ever wraps.
const COLUMNS: u16 = 250;
const ROWS: u16 = 20;

/// Folio's win32-input-mode record pair for Ctrl+Enter, built like `bt-app`'s
/// `SHIFT_ENTER_RECORDS`: `CSI Vk;Sc;Uc;Kd;Cs;Rc _`, down then up — `VK_RETURN` (13), scan code
/// 28, the character a real Ctrl+Enter produces (LF, 10), `LEFT_CTRL_PRESSED` (8), one repeat.
const CTRL_ENTER_RECORDS: &[u8] = b"\x1b[13;28;10;1;8;1_\x1b[13;28;10;0;8;1_";

/// PowerShell's `-EncodedCommand` wants the script as base64 of its UTF-16LE bytes.
fn encoded_command(script: &str) -> OsString {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if index <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out.into()
}

fn powershell(script: &str) -> PtyCommand {
    PtyCommand::interactive_shell(WINDOWS_POWERSHELL)
        .arg("-NoLogo")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-EncodedCommand")
        .arg(encoded_command(script))
}

/// Hex of every UTF-16 unit, as the PowerShell children print what they read.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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
    fn spawn(command: PtyCommand) -> Self {
        let columns = NonZeroU16::new(COLUMNS).unwrap();
        let rows = NonZeroU16::new(ROWS).unwrap();
        let pty = PtySession::spawn(command, PtySize::cells(columns, rows), Arc::new(|| {}))
            .expect("the child starts on a supported host");
        let session = DualPlaneSession::new(
            NonZeroU32::new(u32::from(COLUMNS)).unwrap(),
            NonZeroU32::new(u32::from(ROWS)).unwrap(),
        );
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

    /// The first visible row holding `marker`, with everything up to the end of the marker cut
    /// off — the value the child printed after it.
    fn value_after(&self, marker: &str) -> Option<String> {
        self.session
            .terminal()
            .visible_text()
            .iter()
            .find_map(|row| {
                row.find(marker)
                    .map(|at| row[at + marker.len()..].trim_end().to_owned())
            })
    }

    fn wait_for(&mut self, marker: &str) -> String {
        loop {
            self.pump_once();
            if let Some(value) = self.value_after(marker) {
                return value;
            }
            let silent_for = self.last_output.elapsed();
            assert!(
                silent_for < SILENCE_BUDGET && self.started.elapsed() < CEILING,
                "gave up waiting for {marker} after {:?}, the last {:?} of it silent, {} bytes \
                 read; screen {:?}",
                self.started.elapsed(),
                silent_for,
                self.raw.len(),
                self.session.terminal().visible_text()
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Write `bytes` to the child as keyboard input would reach it, then keep pumping for
    /// [`SETTLE`].
    fn send(&mut self, bytes: &[u8]) {
        self.pty.write(bytes).unwrap();
        let until = Instant::now() + SETTLE;
        while Instant::now() < until {
            self.pump_once();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn finish(mut self) {
        let _ = self.pty.shutdown();
    }
}

fn position(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Case 1 — **the request crosses ConPTY, Folio answers it, and the answer reaches the child.**
///
/// ConPTY parses a client's VT output and writes the same string to the terminal verbatim, and
/// returns early from its own handling of the kitty sequences when it runs as ConPTY (design note
/// §1). So `CSI ? u` and `CSI > 1 u` from a program that writes VT arrive in `bt-term` unchanged,
/// and the reply goes back through ConPTY's input parser as characters.
///
/// RED before T-KEYBOARD-PROTOCOL: the vendored terminal's switch was off, so the query went
/// unanswered, the child read nothing, and the flags stayed 0.
#[test]
fn a_keyboard_protocol_request_crosses_conpty_and_its_answer_reaches_the_child() {
    const SCRIPT: &str = r#"
$e = [char]27
[Console]::Out.Write($e + '[?u' + $e + '[>1u'); [Console]::Out.Flush()
$s = ''; $d = (Get-Date).AddSeconds(10)
while ((Get-Date) -lt $d -and -not $s.EndsWith('u')) {
  if ([Console]::KeyAvailable) { $s += [Console]::ReadKey($true).KeyChar } else { Start-Sleep -Milliseconds 10 }
}
Write-Output ('BT_KKP_' + 'READ=' + (($s.ToCharArray() | ForEach-Object { '{0:x2}' -f [int]$_ }) -join ''))
$d = (Get-Date).AddSeconds(60)
while ((Get-Date) -lt $d) {
  if ([Console]::KeyAvailable) { if ([Console]::ReadKey($true).KeyChar -eq 'q') { break } } else { Start-Sleep -Milliseconds 10 }
}
"#;
    let mut probe = Probe::spawn(powershell(SCRIPT));
    let read = probe.wait_for("BT_KKP_READ=");
    let flags = probe.session.terminal_modes().keyboard.kitty;
    eprintln!(
        "BT_KKP_CASE1 conpty={:?} query_reached_us={} push_reached_us={} flags={flags} \
         answered={:?} child_read={read:?}",
        bt_pty::conpty_source(),
        position(&probe.raw, b"\x1b[?u").is_some(),
        position(&probe.raw, b"\x1b[>1u").is_some(),
        String::from_utf8_lossy(&probe.answered),
    );
    assert!(
        position(&probe.raw, b"\x1b[?u").is_some() && position(&probe.raw, b"\x1b[>1u").is_some(),
        "the query and the push have to reach the terminal; raw {:?}",
        String::from_utf8_lossy(&probe.raw)
    );
    assert_eq!(flags, 1, "the push is in force in the session");
    assert!(
        position(&probe.answered, b"\x1b[?0u").is_some(),
        "the query, asked before the push, is answered 0; answered {:?}",
        String::from_utf8_lossy(&probe.answered)
    );
    assert_eq!(
        read,
        hex(b"\x1b[?0u"),
        "and the child reads the answer back"
    );
    probe.send(b"q");
    probe.finish();
}

/// Case 2 — **a key-record reader reads `CSI 13;5u` as seven characters, reads nothing of
/// `CSI 27;5;13~`, and reads a win32-input-mode record pair as `Enter` with `Control`.**
///
/// This is the fact that makes the design note's third row true: PowerShell, cmd, .NET and Codex
/// on Windows read console key records, never ask for the protocol, and could not read its bytes
/// if they did. ConPTY's input parser has no case for `CSI … u` and hands it over as characters;
/// its `~` handler has no key for 27 and drops the modifyOtherKeys form for such a client. The
/// record pair is what T-KEYBOARD-RECORDS will send (§7.3).
#[test]
fn a_key_record_reader_reads_csi_u_as_characters_nothing_of_modify_other_keys_and_records_as_keys()
{
    const SCRIPT: &str = r#"
Write-Output ('BT_KKP_' + 'READY')
$t = @()
while ($true) {
  $k = [Console]::ReadKey($true)
  if ($k.KeyChar -eq 'q') { break }
  $t += ('{0}/{1}/{2:x2}' -f $k.Key, [int]$k.Modifiers, [int]$k.KeyChar)
}
Write-Output ('BT_KKP_' + 'KEYS=' + ($t -join ' '))
"#;
    let mut probe = Probe::spawn(powershell(SCRIPT));
    probe.wait_for("BT_KKP_READY");
    probe.send(b"\x1b[13;5u");
    probe.send(b"x");
    probe.send(b"\x1b[27;5;13~");
    probe.send(b"x");
    probe.send(CTRL_ENTER_RECORDS);
    probe.send(b"q");
    let keys = probe.wait_for("BT_KKP_KEYS=");
    eprintln!("BT_KKP_CASE2 keys={keys:?}");
    let events = keys.split(' ').collect::<Vec<_>>();
    let segments = events
        .split(|event| event.ends_with("/78"))
        .map(<[&str]>::to_vec)
        .collect::<Vec<_>>();
    assert_eq!(
        segments.len(),
        3,
        "three sends between two `x` sentinels: {keys:?}"
    );

    let characters = segments[0]
        .iter()
        .map(|event| event.rsplit('/').next().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(
        characters,
        ["1b", "5b", "31", "33", "3b", "35", "75"],
        "`CSI 13;5u` reaches a key-record reader as its seven characters: {keys:?}"
    );
    assert!(
        !segments[0].iter().any(|event| event.starts_with("Enter/")),
        "and never as an Enter: {keys:?}"
    );
    assert!(
        segments[1].is_empty(),
        "`CSI 27;5;13~` reaches a key-record reader as nothing: {keys:?}"
    );
    assert_eq!(
        segments[2].len(),
        1,
        "the record pair is one key press: {keys:?}"
    );
    assert!(
        segments[2][0].starts_with("Enter/4/"),
        "and it is Enter with Control (ConsoleModifiers.Control = 4): {keys:?}"
    );
    probe.finish();
}

/// Case 3 — **a VT-input reader reads `CSI 13;5u` and `CSI 27;5;13~` as exactly their bytes.**
///
/// The same console host, after `SetConsoleMode(… | ENABLE_VIRTUAL_TERMINAL_INPUT)` with line
/// input, echo and processed input off, reading the console's input as characters
/// (`ReadConsoleW`, which is what `[Console]::In` reads through). This is what a program that
/// writes and reads VT on Windows — WSL's relay among them — sees, and why the design note's first
/// row gets the whole protocol.
#[test]
fn a_vt_input_reader_reads_both_forms_as_their_bytes() {
    const SCRIPT: &str = r#"
$sig = @'
[DllImport("kernel32.dll")] public static extern IntPtr GetStdHandle(int which);
[DllImport("kernel32.dll")] public static extern bool GetConsoleMode(IntPtr handle, out uint mode);
[DllImport("kernel32.dll")] public static extern bool SetConsoleMode(IntPtr handle, uint mode);
[DllImport("kernel32.dll", CharSet = CharSet.Unicode)] public static extern bool ReadConsoleW(IntPtr handle, [Out] char[] buffer, uint wanted, out uint read, IntPtr control);
'@
Add-Type -MemberDefinition $sig -Name Console -Namespace FolioProbe | Out-Null
$h = [FolioProbe.Console]::GetStdHandle(-10)
$m = [uint32]0
[void][FolioProbe.Console]::GetConsoleMode($h, [ref]$m)
$vt = ($m -bor 0x200) -band (-bnot 0x7)
if (-not [FolioProbe.Console]::SetConsoleMode($h, $vt)) { Write-Output ('BT_KKP_' + 'NOMODE') }
Write-Output ('BT_KKP_' + 'READY')
$buffer = New-Object char[] 256
$s = ''
while (-not $s.Contains('q')) {
  $n = [uint32]0
  if (-not [FolioProbe.Console]::ReadConsoleW($h, $buffer, 256, [ref]$n, [IntPtr]::Zero)) { break }
  if ($n -gt 0) { $s += -join $buffer[0..([int]$n - 1)] }
}
$s = $s.Substring(0, $s.IndexOf('q'))
Write-Output ('BT_KKP_' + 'VT=' + (($s.ToCharArray() | ForEach-Object { '{0:x2}' -f [int]$_ }) -join ''))
"#;
    let mut probe = Probe::spawn(powershell(SCRIPT));
    probe.wait_for("BT_KKP_READY");
    assert!(
        probe.value_after("BT_KKP_NOMODE").is_none(),
        "the console must accept ENABLE_VIRTUAL_TERMINAL_INPUT for this reader to be one"
    );
    probe.send(b"\x1b[13;5u");
    probe.send(b"x");
    probe.send(b"\x1b[27;5;13~");
    probe.send(b"q");
    let read = probe.wait_for("BT_KKP_VT=");
    eprintln!("BT_KKP_CASE3 read={read:?}");
    assert_eq!(
        read,
        format!(
            "{}{}{}",
            hex(b"\x1b[13;5u"),
            hex(b"x"),
            hex(b"\x1b[27;5;13~")
        ),
        "a VT-input reader reads both forms byte for byte"
    );
    probe.finish();
}

/// Case 4 — **a native Node raw-stdin reader reads the answer to its query, and `CSI 13;5u` as
/// exactly those seven bytes.**
///
/// The stand-in for Claude Code on Windows, whose transport this is: `process.stdin.setRawMode`
/// and libuv reassembling what the console hands it into bytes. If this fails the design note's
/// second row is wrong, and T-KEYBOARD-PROTOCOL stops before any release note names Claude Code
/// on Windows (§6.3).
///
/// It needs `node` on `PATH`, and **fails** rather than skipping when there is none: a green
/// result has to mean the reader ran. CI's `conpty` job selects it out by name.
#[test]
fn a_native_node_reader_reads_the_answer_and_csi_u_as_its_bytes() {
    let version = std::process::Command::new("node").arg("--version").output();
    let Ok(version) = version.as_ref().map(|output| output.stdout.clone()) else {
        panic!(
            "case 4 needs `node` on PATH (the stand-in for Claude Code on Windows) and there is \
             none: {version:?}"
        );
    };
    const SCRIPT: &str = "const out = process.stdout; \
        let buf = Buffer.alloc(0); let phase = 0; \
        process.stdin.setRawMode(true); process.stdin.resume(); \
        process.stdin.on('data', (d) => { buf = Buffer.concat([buf, d]); \
          if (phase === 0 && buf.includes(0x75)) { \
            out.write('BT_KKP_' + 'NODE_REPLY=' + buf.toString('hex') + '\\r\\n'); \
            buf = Buffer.alloc(0); phase = 1; \
          } else if (phase === 1 && buf.includes(0x71)) { \
            out.write('BT_KKP_' + 'NODE_KEYS=' + buf.subarray(0, buf.indexOf(0x71)).toString('hex') + '\\r\\n'); \
            setTimeout(() => process.exit(0), 200); \
          } }); \
        out.write('\\x1b[?u');";
    let mut probe = Probe::spawn(PtyCommand::new("node").arg("-e").arg(SCRIPT));
    let reply = probe.wait_for("BT_KKP_NODE_REPLY=");
    probe.send(b"\x1b[13;5u");
    probe.send(b"q");
    let keys = probe.wait_for("BT_KKP_NODE_KEYS=");
    eprintln!(
        "BT_KKP_CASE4 node={:?} reply={reply:?} keys={keys:?}",
        String::from_utf8_lossy(&version).trim()
    );
    assert_eq!(
        reply,
        hex(b"\x1b[?0u"),
        "the Node reader reads Folio's answer to its query"
    );
    assert_eq!(
        keys,
        hex(b"\x1b[13;5u"),
        "and reads `CSI 13;5u` as exactly its seven bytes"
    );
    probe.finish();
}
