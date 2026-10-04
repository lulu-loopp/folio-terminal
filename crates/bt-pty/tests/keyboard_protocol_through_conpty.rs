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
//! T-KEYBOARD-RECORDS (design note §7.3) adds the transport it uses for the programs above that
//! never ask — win32-input-mode key records — and its gates 1 and 2 as assertions here:
//! ConPTY asks for win32-input-mode at the head of the session and again after a child's `RIS`,
//! and the session records it; and for every chord the encoder writes as a record pair, a
//! key-record reader gets exactly that key, a VT-input reader gets what ConPTY's own encoder
//! writes for it and a native Node reader what libuv writes (both pinned as literals), with
//! Ctrl+Enter no worse than today's `\r`. The Node reader is named `a_native_node_reader…` so
//! CI's filter selects it out as it does case 4. The chords are every one the application's
//! encoder writes on a US layout, read from the file `bt-app`'s own test holds equal to the
//! encoder, and the byte literals are the ConPTY Folio ships (the vendored pair); under
//! `BT_CONPTY_FORCE_SYSTEM=1` the key-record reader still passes and the byte readers differ, as
//! the design note's revision (e) tabulates.
//!
//! Every child is spawned the way a pane spawns it, `PtyCommand` through `PtySession`, and every
//! PowerShell script crosses the command line as `-EncodedCommand`, so nothing in it is re-parsed
//! by the argument-quoting rules the colour probe has to write around.

#![cfg(windows)]
#![allow(clippy::disallowed_methods)]

use std::{
    ffi::OsString,
    num::{NonZeroU16, NonZeroU32},
    time::{Duration, Instant},
};

use bt_pty::test_shell::TestShell;
use bt_pty::{PtyCommand, PtySize, WINDOWS_POWERSHELL};
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
        .arg("-NonInteractive")
        .arg("-EncodedCommand")
        .arg(encoded_command(script))
}

/// Hex of every UTF-16 unit, as the PowerShell children print what they read.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

struct Probe {
    pty: TestShell,
    session: DualPlaneSession,
    raw: Vec<u8>,
    answered: Vec<u8>,
    started: Instant,
    last_output: Instant,
}

impl Probe {
    fn spawn(command: PtyCommand) -> Self {
        Self::spawn_with_rows(command, ROWS)
    }

    /// A probe tall enough for a child that prints one line per chord.
    fn spawn_with_rows(command: PtyCommand, rows: u16) -> Self {
        let columns = NonZeroU16::new(COLUMNS).unwrap();
        let pty = TestShell::spawn(
            command,
            PtySize::cells(columns, NonZeroU16::new(rows).unwrap()),
        )
        .expect("the child starts on a supported host");
        let session = DualPlaneSession::new(
            NonZeroU32::new(u32::from(COLUMNS)).unwrap(),
            NonZeroU32::new(u32::from(rows)).unwrap(),
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
            self.pty.reply(&reply).unwrap();
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
            if silent_for >= SILENCE_BUDGET || self.started.elapsed() >= CEILING {
                panic!(
                    "gave up waiting for {marker} after {:?}, the last {:?} of it silent, {} bytes \
                     read; {}; screen {:?}",
                    self.started.elapsed(),
                    silent_for,
                    self.raw.len(),
                    self.pty.account(),
                    self.session.terminal().visible_text()
                );
            }
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

// ── T-KEYBOARD-RECORDS (design note §7.3, gates 1 and 2) ──────────────────────────────────────
//
// **Scope of the byte literals.** The VT-input and Node readbacks below are what the ConPTY Folio
// ships answers: the vendored `conpty.dll`/`OpenConsole.exe` pair (`ConPtySource::Sidecar`), which
// these tests run against. `BT_CONPTY_FORCE_SYSTEM=1` switches a process to the ConPTY inbox in
// Windows; that is a test switch, not a product setting, and under it the key-record reader still
// passes while several byte readbacks differ (design note revision (e) has the comparison). The
// product falls back to the inbox ConPTY on its own only when the packaged pair is missing beside
// `folio.exe` or fails to load (`vendor/conpty/portable-pty/src/win/psuedocon.rs`, `load_conpty`),
// and a pane on it is sent no records at all (`ConPtyKind::Inbox`; coordinator's ruling,
// 2026-09-29): these literals never have to hold there.

/// Every chord the application's encoder writes as a record pair on Windows with a US layout —
/// `bt-app`'s `input::keyboard_bytes`, rendered by its test
/// `the_windows_us_records_file_is_what_the_encoder_writes`, which fails when this file is not
/// exactly the encoder's output. Rows: key, location, modifiers, bytes (`\e` is ESC).
const ENCODER_RECORDS: &str = include_str!("../../bt-app/src/key_records_windows_us.tsv");

/// One chord driven through ConPTY: the record pair and the key event it stands for.
struct Chord {
    name: String,
    records: Vec<u8>,
    virtual_key: u16,
    character: u16,
    /// `ConsoleModifiers`: Alt 1, Shift 2, Control 4, from the record's control-key state.
    console_modifiers: u8,
    /// Written by the encoder. The six chords of Escape with Ctrl or Alt are driven too and are
    /// not: ConPTY swallows their record (asserted below), which is why the encoder keeps their
    /// ESC. Their pairs are built here, in the encoder's form, because the encoder writes none.
    from_the_encoder: bool,
}

/// The fields of the first record of a pair: `(Vk, Uc, Cs)`.
fn record_fields(records: &[u8]) -> (u16, u16, u16) {
    let text = std::str::from_utf8(records).expect("a record is ASCII");
    let body = text
        .strip_prefix("\x1b[")
        .and_then(|rest| rest.split('_').next())
        .unwrap_or_else(|| panic!("not a record: {text:?}"));
    let fields = body
        .split(';')
        .map(|field| field.parse::<u16>().expect("a number"))
        .collect::<Vec<_>>();
    let [virtual_key, _scan, character, down, state, repeat] = fields[..] else {
        panic!("six fields: {text:?}");
    };
    assert_eq!((down, repeat), (1, 1), "down first, one repeat: {text:?}");
    (virtual_key, character, state)
}

fn chord(name: String, records: Vec<u8>, from_the_encoder: bool) -> Chord {
    let (virtual_key, character, state) = record_fields(&records);
    let console_modifiers =
        u8::from(state & 2 != 0) + 2 * u8::from(state & 16 != 0) + 4 * u8::from(state & 8 != 0);
    Chord {
        name,
        records,
        virtual_key,
        character,
        console_modifiers,
        from_the_encoder,
    }
}

/// The encoder's chords, then the six swallowed Escape chords.
fn record_chords() -> Vec<Chord> {
    let mut chords = ENCODER_RECORDS
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let cells = line.split('\t').collect::<Vec<_>>();
            let [key, location, mods, bytes] = cells[..] else {
                panic!("four columns: {line:?}");
            };
            let name = if location == "numpad" {
                format!("{mods}+Numpad{key}")
            } else {
                format!("{mods}+{key}")
            };
            chord(name, bytes.replace("\\e", "\x1b").into_bytes(), true)
        })
        .collect::<Vec<_>>();
    assert_eq!(chords.len(), 155, "every chord the encoder file holds");
    for (mods, state) in [
        ("A", 2),
        ("SA", 18),
        ("C", 8),
        ("SC", 24),
        ("AC", 10),
        ("SAC", 26),
    ] {
        let character = if mods == "C" {
            27
        } else if mods.contains('C') {
            0
        } else {
            27
        };
        let record = |down: u8| format!("\x1b[27;1;{character};{down};{state};1_");
        chords.push(chord(
            format!("{mods}+Escape"),
            format!("{}{}", record(1), record(0)).into_bytes(),
            false,
        ));
    }
    chords
}

/// `§` separates one chord's reading from the next and `¶` ends the run: no record of the set
/// types either on a US layout, and neither is a byte a VT encoder writes.
const SEPARATOR: &str = "\u{a7}";
const END: &str = "\u{b6}";

/// Send every chord's record pair and a separator, in order, then the end mark.
fn send_every_chord(probe: &mut Probe, chords: &[Chord]) {
    for chord in chords {
        probe.pty.write(&chord.records).unwrap();
        probe.pty.write(SEPARATOR.as_bytes()).unwrap();
        probe.pump_once();
    }
    probe.send(END.as_bytes());
}

/// **ConPTY asks for win32-input-mode at the head of every session, and the session records it;
/// what ConPTY writes after a child's `RIS` decides whether it is on afterwards** (gate 1).
///
/// The real bytes, asserted: `?9001h` arrives before the child's first output and the session's
/// `win32_input_mode` is on; the child's `ESC c` reaches the terminal, which resets every mode;
/// ConPTY follows it with `?1004h ?9001h` at once, so the session has the mode on again, which is
/// what the encoder reads.
#[test]
fn conpty_asks_for_win32_input_mode_and_the_session_records_it() {
    const SCRIPT: &str = r#"
Write-Output ('BT_KKR_' + 'READY')
while ($true) { $k = [Console]::ReadKey($true); if ($k.KeyChar -eq 'r') { break } }
$e = [char]27
[Console]::Out.Write($e + 'c'); [Console]::Out.Flush()
Write-Output ('BT_KKR_' + 'RESET')
while ($true) { $k = [Console]::ReadKey($true); if ($k.KeyChar -eq 'q') { break } }
"#;
    let mut probe = Probe::spawn(powershell(SCRIPT));
    probe.wait_for("BT_KKR_READY");
    let head = probe.raw.clone();
    let asked_at_head = position(&head, b"\x1b[?9001h");
    let on_at_head = probe.session.terminal_modes().keyboard.win32_input_mode;
    probe.send(b"r");
    probe.wait_for("BT_KKR_RESET");
    probe.send(b"");
    let after = probe.raw[head.len()..].to_vec();
    let ris = position(&after, b"\x1bc");
    let asked_after_ris = ris.and_then(|at| position(&after[at..], b"\x1b[?9001h"));
    let on_after_ris = probe.session.terminal_modes().keyboard.win32_input_mode;
    eprintln!(
        "BT_KKR_HEAD asked_at={asked_at_head:?} on={on_at_head} head={:?}",
        String::from_utf8_lossy(&head[..head.len().min(80)])
    );
    eprintln!(
        "BT_KKR_RIS ris_at={ris:?} asked_after={asked_after_ris:?} on={on_after_ris} after={:?}",
        String::from_utf8_lossy(&after[..after.len().min(200)])
    );
    assert!(
        asked_at_head.is_some(),
        "ConPTY asks for win32-input-mode at the head of the session: {:?}",
        String::from_utf8_lossy(&head)
    );
    assert!(on_at_head, "and the session records it");
    assert!(
        ris.is_some(),
        "the child's RIS reaches the terminal, which resets every mode: {:?}",
        String::from_utf8_lossy(&after)
    );
    assert!(
        asked_after_ris.is_some(),
        "ConPTY asks for win32-input-mode again right after a RIS: {:?}",
        String::from_utf8_lossy(&after)
    );
    assert!(
        on_after_ris,
        "so after a RIS the session has it on again, and records go on reaching the child"
    );
    probe.send(b"q");
    probe.finish();
}

/// RED (T-KEYBOARD-RECORDS, review round 3) — **a spawned pane knows which ConPTY it runs on,
/// and why when it is the inbox one.**
///
/// The key encoder writes win32-input-mode records only to the ConPTY Folio ships (coordinator's
/// ruling, 2026-09-29), reading this at the key; the application writes the reason to
/// `diagnostics.log` when a pane is born on the inbox one. In an ordinary run the test executable
/// has the packaged pair beside it, so the pane is `Shipped` with no reason; under
/// `BT_CONPTY_FORCE_SYSTEM=1` it is `Inbox`, and the reason names the switch — which is also how
/// this file's byte literals are shown not to apply there (the encoder writes no records to such a
/// pane: `bt-app`'s `input::tests::a_pane_on_the_inbox_conpty_gets_no_records`).
///
/// MUTATION: map `ConPtySource::System` to `ConPtyKind::Shipped` in `ConPtyKind::of`.
#[test]
fn a_spawned_pane_knows_which_conpty_it_runs_on() {
    let probe = Probe::spawn(powershell("exit"));
    let kind = probe.pty.conpty_kind();
    let reason = probe.pty.inbox_conpty_reason();
    eprintln!("BT_KKR_CONPTY kind={kind:?} reason={reason:?}");
    if std::env::var_os("BT_CONPTY_FORCE_SYSTEM").is_some() {
        assert_eq!(kind, bt_pty::ConPtyKind::Inbox);
        assert!(
            reason
                .as_deref()
                .is_some_and(|reason| reason.contains("BT_CONPTY_FORCE_SYSTEM")),
            "the reason names the switch: {reason:?}"
        );
    } else {
        assert_eq!(kind, bt_pty::ConPtyKind::Shipped);
        assert_eq!(reason, None);
    }
    probe.finish();
}

/// Rows enough for one `BT_KKR_<n>=` line per chord and the markers.
const CHORD_ROWS: u16 = 200;

/// Split what a reader printed, one `BT_KKR_<n>=` line per chord.
fn per_chord(probe: &mut Probe, chords: &[Chord], last: &str) -> Vec<String> {
    probe.wait_for(last);
    (0..chords.len())
        .map(|index| {
            probe
                .value_after(&format!("BT_KKR_{index}="))
                .unwrap_or_else(|| panic!("no line for chord {index} ({})", chords[index].name))
        })
        .collect()
}

/// Gate 2, **a key-record reader gets every chord the encoder writes as that key, with its
/// modifiers and its character** — `Enter` with `Control` for Ctrl+Enter, and likewise each of
/// the 155 — **and gets nothing for the six Escape chords ConPTY swallows.**
///
/// PowerShell's `[Console]::ReadKey`, as PSReadLine, cmd, .NET and Codex on Windows read. Each
/// chord is sent as the encoder's record pair, then a separator; the reader prints, per chord,
/// every key event it read as `ConsoleKey/ConsoleModifiers/KeyChar` in numbers. Exactly one
/// event per chord: its virtual key, its modifiers and its character, field for field.
#[test]
fn a_key_record_reader_reads_every_chord_the_encoder_writes_as_that_key() {
    const SCRIPT: &str = r#"
Write-Output ('BT_KKR_' + 'READY')
$t = @(); $cur = @()
while ($true) {
  $k = [Console]::ReadKey($true)
  if ($k.KeyChar -eq [char]0xB6) { break }
  if ($k.KeyChar -eq [char]0xA7) { $t += ,($cur -join ' '); $cur = @(); continue }
  $cur += ('{0}/{1}/{2:x2}' -f [int]$k.Key, [int]$k.Modifiers, [int]$k.KeyChar)
}
for ($i = 0; $i -lt $t.Count; $i++) { Write-Output ('BT_KKR_' + $i + '=' + $t[$i]) }
Write-Output ('BT_KKR_' + 'END')
"#;
    let chords = record_chords();
    let mut probe = Probe::spawn_with_rows(powershell(SCRIPT), CHORD_ROWS);
    probe.wait_for("BT_KKR_READY");
    send_every_chord(&mut probe, &chords);
    let read = per_chord(&mut probe, &chords, "BT_KKR_END");
    for (chord, events) in chords.iter().zip(&read) {
        eprintln!("BT_KKR_RECORD {} {events:?}", chord.name);
    }
    for (chord, events) in chords.iter().zip(&read) {
        if chord.from_the_encoder {
            assert_eq!(
                events,
                &format!(
                    "{}/{}/{:02x}",
                    chord.virtual_key, chord.console_modifiers, chord.character
                ),
                "{}: one key event, the chord itself",
                chord.name
            );
        } else {
            assert_eq!(
                events, "",
                "{}: ConPTY swallows the record, which is why the encoder keeps this chord's ESC",
                chord.name
            );
        }
    }
    probe.finish();
}

/// Gate 2, **a VT-input reader gets, for every chord, exactly what ConPTY's own encoder writes
/// for that key event** — asserted as the literals of [`READ_BACK`], so a change in the ConPTY
/// Folio ships is caught.
///
/// The same reader as case 3 (`ENABLE_VIRTUAL_TERMINAL_INPUT`, `ReadConsoleW`): what WSL's relay
/// reads. ConPTY parses the record into a key event and, for a client that asked for VT input,
/// writes that event back out as VT itself. Scoped to the vendored ConPTY (the note above).
#[test]
fn a_vt_input_reader_reads_what_conpty_encodes_for_every_record() {
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
if (-not [FolioProbe.Console]::SetConsoleMode($h, $vt)) { Write-Output ('BT_KKR_' + 'NOMODE') }
Write-Output ('BT_KKR_' + 'READY')
$buffer = New-Object char[] 256
$s = ''
while (-not $s.Contains([char]0xB6)) {
  $n = [uint32]0
  if (-not [FolioProbe.Console]::ReadConsoleW($h, $buffer, 256, [ref]$n, [IntPtr]::Zero)) { break }
  if ($n -gt 0) { $s += -join $buffer[0..([int]$n - 1)] }
}
$s = $s.Substring(0, $s.IndexOf([char]0xB6))
$parts = $s.Split([char]0xA7)
for ($i = 0; $i -lt $parts.Count - 1; $i++) {
  Write-Output ('BT_KKR_' + $i + '=' + (($parts[$i].ToCharArray() | ForEach-Object { '{0:x2}' -f [int]$_ }) -join ''))
}
Write-Output ('BT_KKR_' + 'END')
"#;
    let chords = record_chords();
    let mut probe = Probe::spawn_with_rows(powershell(SCRIPT), CHORD_ROWS);
    probe.wait_for("BT_KKR_READY");
    assert!(
        probe.value_after("BT_KKR_NOMODE").is_none(),
        "the console must accept ENABLE_VIRTUAL_TERMINAL_INPUT for this reader to be one"
    );
    send_every_chord(&mut probe, &chords);
    let read = per_chord(&mut probe, &chords, "BT_KKR_END");
    for (chord, bytes) in chords.iter().zip(&read) {
        eprintln!("BT_KKR_VT {:?} {bytes:?}", chord.name);
    }
    for (chord, bytes) in chords.iter().zip(&read) {
        assert_eq!(
            bytes,
            read_back(chord).0,
            "{}: what ConPTY encodes for the key event",
            chord.name
        );
    }
    probe.finish();
}

/// Gate 2, **a native Node raw-stdin reader gets, for every chord, bytes asserted exactly — and
/// for Ctrl+Enter nothing worse than today's `\r`.**
///
/// libuv reads the console's key events and turns them into bytes itself. Needs `node` on `PATH`
/// and fails without it, as case 4 does; CI's `conpty` job selects it out by name. Scoped to the
/// vendored ConPTY (the note above).
#[test]
fn a_native_node_reader_reads_every_record_no_worse_than_today() {
    let version = std::process::Command::new("node").arg("--version").output();
    let Ok(version) = version.as_ref().map(|output| output.stdout.clone()) else {
        panic!("this reader needs `node` on PATH and there is none: {version:?}");
    };
    // `§` is C2 A7 and `¶` C2 B6 in what libuv hands over.
    const SCRIPT: &str = "const out = process.stdout; \
        let buf = Buffer.alloc(0); \
        process.stdin.setRawMode(true); process.stdin.resume(); \
        process.stdin.on('data', (d) => { buf = Buffer.concat([buf, d]); \
          const end = buf.indexOf(Buffer.from([0xc2, 0xb6])); \
          if (end >= 0) { \
            const all = buf.subarray(0, end); \
            const parts = []; let from = 0; \
            for (let i = 0; i + 1 < all.length; i++) { if (all[i] === 0xc2 && all[i + 1] === 0xa7) { parts.push(all.subarray(from, i)); from = i + 2; i++; } } \
            parts.forEach((p, i) => out.write('BT_KKR_' + i + '=' + p.toString('hex') + '\\r\\n')); \
            out.write('BT_KKR_' + 'END\\r\\n'); \
            setTimeout(() => process.exit(0), 200); \
          } }); \
        out.write('BT_KKR_' + 'READY\\r\\n');";
    // Today's bytes for Ctrl+Enter first: the legacy `\r`.
    let mut all = vec![Chord {
        name: "today's Ctrl+Enter".to_owned(),
        records: b"\r".to_vec(),
        virtual_key: 13,
        character: 13,
        console_modifiers: 4,
        from_the_encoder: false,
    }];
    all.extend(record_chords());
    let mut probe =
        Probe::spawn_with_rows(PtyCommand::new("node").arg("-e").arg(SCRIPT), CHORD_ROWS);
    probe.wait_for("BT_KKR_READY");
    send_every_chord(&mut probe, &all);
    let read = per_chord(&mut probe, &all, "BT_KKR_END");
    eprintln!(
        "BT_KKR_NODE node={:?}",
        String::from_utf8_lossy(&version).trim()
    );
    for (chord, bytes) in all.iter().zip(&read) {
        eprintln!("BT_KKR_NODE {:?} {bytes:?}", chord.name);
    }
    assert_eq!(
        read[0], "0d",
        "today Ctrl+Enter reaches libuv as `\\r`, the same as Enter"
    );
    for (chord, bytes) in all[1..].iter().zip(&read[1..]) {
        assert_eq!(
            bytes,
            read_back(chord).1,
            "{}: what libuv reads for the key event",
            chord.name
        );
    }
    let ctrl_enter = all
        .iter()
        .position(|chord| chord.name == "C+Enter")
        .expect("Ctrl+Enter is driven");
    assert_eq!(
        read[ctrl_enter], "0a",
        "Ctrl+Enter as a record reaches libuv as LF — still a line end for a reader that took `\\r` \
         as one, and now told apart from Enter: no worse than today's `\\r`"
    );
    probe.finish();
}

/// The read-back row of `chord`, by name: `(VT-input reader, Node raw-stdin reader)`.
fn read_back(chord: &Chord) -> (&'static str, &'static str) {
    let (_, vt, node) = READ_BACK
        .iter()
        .find(|(name, _, _)| *name == chord.name)
        .unwrap_or_else(|| panic!("no read-back row for {}", chord.name));
    (vt, node)
}

/// What each reader got for each chord in the acceptance run through the vendored ConPTY, as hex:
/// `(chord, VT-input reader, Node raw-stdin reader)`. ConPTY parses the record into a key event
/// and, for a client that reads VT, turns it back into bytes with its own encoder; libuv does the
/// same for Node. These are those answers, pinned so a change in either is caught — the
/// translations Windows Terminal's users get from the same records (design note revision (e)).
/// An empty cell reads nothing.
const READ_BACK: [(&str, &str, &str); 161] = [
    ("S+Enter", "0d", "0d"),
    ("A+Enter", "1b0d", "1b0d"),
    ("SA+Enter", "1b0d", "1b0d"),
    ("C+Enter", "0a", "0a"),
    ("SC+Enter", "0a", "0a"),
    ("AC+Enter", "1b0a", "1b0a"),
    ("SAC+Enter", "1b0a", "1b0a"),
    ("S+Tab", "1b5b5a", "1b5b5a"),
    ("A+Tab", "1b09", "1b09"),
    ("SA+Tab", "1b1b5b5a", "1b1b5b5a"),
    ("AC+Tab", "1b09", "1b09"),
    ("SAC+Tab", "1b1b5b5a", "1b1b5b5a"),
    ("S+Backspace", "7f", "7f"),
    ("A+Backspace", "1b7f", "1b7f"),
    ("SA+Backspace", "1b7f", "1b7f"),
    ("C+Backspace", "08", "08"),
    ("SC+Backspace", "08", "08"),
    ("AC+Backspace", "1b08", "1b08"),
    ("SAC+Backspace", "1b08", "1b08"),
    ("S+Escape", "1b", "1b"),
    ("S+Space", "20", "20"),
    ("A+Space", "1b20", "1b20"),
    ("SA+Space", "1b20", "1b20"),
    ("C+Space", "00", ""),
    ("SC+Space", "00", ""),
    ("AC+Space", "1b00", "1b"),
    ("SAC+Space", "1b00", "1b"),
    ("AC+a", "1b01", "1b01"),
    ("SAC+a", "1b01", "1b01"),
    ("AC+b", "1b02", "1b02"),
    ("SAC+b", "1b02", "1b02"),
    ("AC+c", "1b03", "1b03"),
    ("SAC+c", "1b03", "1b03"),
    ("AC+d", "1b04", "1b04"),
    ("SAC+d", "1b04", "1b04"),
    ("AC+e", "1b05", "1b05"),
    ("SAC+e", "1b05", "1b05"),
    ("AC+f", "1b06", "1b06"),
    ("SAC+f", "1b06", "1b06"),
    ("AC+g", "1b07", "1b07"),
    ("SAC+g", "1b07", "1b07"),
    ("AC+h", "1b08", "1b08"),
    ("SAC+h", "1b08", "1b08"),
    ("AC+i", "1b09", "1b09"),
    ("SAC+i", "1b09", "1b09"),
    ("AC+j", "1b0a", "1b0a"),
    ("SAC+j", "1b0a", "1b0a"),
    ("AC+k", "1b0b", "1b0b"),
    ("SAC+k", "1b0b", "1b0b"),
    ("AC+l", "1b0c", "1b0c"),
    ("SAC+l", "1b0c", "1b0c"),
    ("AC+m", "1b0d", "1b0d"),
    ("SAC+m", "1b0d", "1b0d"),
    ("AC+n", "1b0e", "1b0e"),
    ("SAC+n", "1b0e", "1b0e"),
    ("AC+o", "1b0f", "1b0f"),
    ("SAC+o", "1b0f", "1b0f"),
    ("AC+p", "1b10", "1b10"),
    ("SAC+p", "1b10", "1b10"),
    ("AC+q", "1b11", "1b11"),
    ("SAC+q", "1b11", "1b11"),
    ("AC+r", "1b12", "1b12"),
    ("SAC+r", "1b12", "1b12"),
    ("AC+s", "1b13", "1b13"),
    ("SAC+s", "1b13", "1b13"),
    ("AC+t", "1b14", "1b14"),
    ("SAC+t", "1b14", "1b14"),
    ("AC+u", "1b15", "1b15"),
    ("SAC+u", "1b15", "1b15"),
    ("AC+v", "1b16", "1b16"),
    ("SAC+v", "1b16", "1b16"),
    ("AC+w", "1b17", "1b17"),
    ("SAC+w", "1b17", "1b17"),
    ("AC+x", "1b18", "1b18"),
    ("SAC+x", "1b18", "1b18"),
    ("AC+y", "1b19", "1b19"),
    ("SAC+y", "1b19", "1b19"),
    ("AC+z", "1b1a", "1b1a"),
    ("SAC+z", "1b1a", "1b1a"),
    ("C+0", "30", "30"),
    ("SC+0", "29", "29"),
    ("AC+0", "1b30", "1b30"),
    ("SAC+0", "1b29", "1b29"),
    ("C+1", "31", "31"),
    ("AC+1", "1b31", "1b31"),
    ("SAC+1", "1b21", "1b21"),
    ("C+2", "00", ""),
    ("AC+2", "1b00", "1b"),
    ("SAC+2", "1b00", "1b"),
    ("C+3", "1b", "1b"),
    ("AC+3", "1b1b", "1b1b"),
    ("SAC+3", "1b1b", "1b1b"),
    ("C+4", "1c", "1c"),
    ("AC+4", "1b1c", "1b1c"),
    ("SAC+4", "1b1c", "1b1c"),
    ("C+5", "1d", "1d"),
    ("AC+5", "1b1d", "1b1d"),
    ("SAC+5", "1b1d", "1b1d"),
    ("C+6", "1e", "1e"),
    ("AC+6", "1b1e", "1b1e"),
    ("SAC+6", "1b1e", "1b1e"),
    ("C+7", "1f", "1f"),
    ("AC+7", "1b1f", "1b1f"),
    ("SAC+7", "1b1f", "1b1f"),
    ("C+8", "7f", "7f"),
    ("AC+8", "1b7f", "1b7f"),
    ("SAC+8", "1b7f", "1b7f"),
    ("C+9", "39", "39"),
    ("AC+9", "1b39", "1b39"),
    ("SAC+9", "1b39", "1b39"),
    ("C+`", "00", ""),
    ("SC+`", "1e", "1e"),
    ("AC+`", "1b00", "1b"),
    ("SAC+`", "1b1e", "1b1e"),
    ("C+-", "2d", "2d"),
    ("AC+-", "1b2d", "1b2d"),
    ("SAC+-", "1b1f", "1b1f"),
    ("C+=", "3d", "3d"),
    ("SC+=", "2b", "2b"),
    ("AC+=", "1b3d", "1b3d"),
    ("SAC+=", "1b2b", "1b2b"),
    ("SC+[", "1b", "1b"),
    ("AC+[", "1b1b", "1b1b"),
    ("SAC+[", "1b1b", "1b1b"),
    ("SC+]", "1d", "1d"),
    ("AC+]", "1b1d", "1b1d"),
    ("SAC+]", "1b1d", "1b1d"),
    ("SC+\\", "1c", "1c"),
    ("AC+\\", "1b1c", "1b1c"),
    ("SAC+\\", "1b1c", "1b1c"),
    ("C+;", "3b", "3b"),
    ("SC+;", "3a", "3a"),
    ("AC+;", "1b3b", "1b3b"),
    ("SAC+;", "1b3a", "1b3a"),
    ("C+'", "27", "27"),
    ("SC+'", "22", "22"),
    ("AC+'", "1b27", "1b27"),
    ("SAC+'", "1b22", "1b22"),
    ("SC+,", "3c", "3c"),
    ("AC+,", "1b2c", "1b2c"),
    ("SAC+,", "1b3c", "1b3c"),
    ("C+.", "2e", "2e"),
    ("SC+.", "3e", "3e"),
    ("AC+.", "1b2e", "1b2e"),
    ("SAC+.", "1b3e", "1b3e"),
    ("C+/", "1f", "1f"),
    ("AC+/", "1b1f", "1b1f"),
    ("SAC+/", "1b7f", "1b7f"),
    ("S+NumpadEnter", "0d", "0d"),
    ("A+NumpadEnter", "1b0d", "1b0d"),
    ("SA+NumpadEnter", "1b0d", "1b0d"),
    ("C+NumpadEnter", "0a", "0a"),
    ("SC+NumpadEnter", "0a", "0a"),
    ("AC+NumpadEnter", "1b0a", "1b0a"),
    ("SAC+NumpadEnter", "1b0a", "1b0a"),
    ("A+Escape", "", ""),
    ("SA+Escape", "", ""),
    ("C+Escape", "", ""),
    ("SC+Escape", "", ""),
    ("AC+Escape", "", ""),
    ("SAC+Escape", "", ""),
];
