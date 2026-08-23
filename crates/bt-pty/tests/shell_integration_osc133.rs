//! The PowerShell half of shell integration, driven through a real ConPTY.
//!
//! `bt-term`'s `shell_integration_script.rs` already pins what
//! `scripts/shell-integration/folio.ps1` *returns*: it calls the wrapped `prompt`
//! by hand in a `-NonInteractive` host and reads the string back. That is a test of the
//! script's arithmetic, and it cannot see the question these two ask, which is a
//! transport question: **do the markers reach the terminal?** Between the string the
//! function returns and the bytes a terminal reads sit the console host that decides
//! when to draw a prompt at all, PSReadLine, and ConPTY — which renders a screen
//! rather than forwarding a stream, and passes an OSC through only because it was
//! taught to. Any one of those three could stop carrying `133;A`/`133;B` without the
//! script changing by a byte, and nothing in this repository would have gone red.
//!
//! So these spawn Windows PowerShell (and PowerShell 7 where the machine has it) the
//! way the product spawns it — `bt_pty::PtyCommand` through `bt_pty::PtySession`, a
//! real pty with a real line editor on the other end — let it draw a prompt, run one
//! command, and draw another, and then read the raw bytes that came back.
//!
//! **The pair is the point, and both halves stay.** The A/B experiment of 2026-08-21
//! (recorded in `docs/DESIGN.md` §7.1.5i, run by hand and never committed — this file
//! is that probe brought into the suite) established the shape: the same shell, the
//! same prompt, the same typed line, twice, with the presence of `. folio.ps1` as the
//! only difference. Without it a PowerShell is *silent* on OSC 133 — that is the red
//! form, and it is a standing test rather than a paragraph, because it is the only
//! thing that says the green one is measuring the script and not the shell.
//!
//! **Nothing here touches the user's own shell.** The script reaches the child as an
//! argument (`-NoProfile … -Command ". '<script>'"`), never through `$PROFILE`, which
//! is the one file this product refuses to edit on its own (`bt_app::shell_integration`
//! says why); `-NoProfile` also means the real one is not so much as read. The line
//! editor is told to save no history, so the user's own history file is not written
//! either.

#![cfg(windows)]

use std::{
    num::{NonZeroU16, NonZeroU32},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use bt_pty::{
    PtyCommand, PtySession, PtySize, SystemShellEnvironment, WINDOWS_POWERSHELL,
    resolve_powershell_seven,
};
use bt_term::DualPlaneSession;

/// The prompt the probe installs, so that "a prompt is on the screen" is something it
/// reads rather than something it assumes. Deliberately not a substring of anything
/// else these tests type or wait for.
const PROMPT: &str = "BTOSC133> ";
/// The same prompt as a row of the screen holds it. A terminal row is trailing-space
/// free — the cells past the last written one are blank, not spaces — so the needle a
/// screen is searched for is the prompt without the separator a real prompt ends in.
const PROMPT_ON_SCREEN: &str = "BTOSC133>";

/// What the one command prints, and what the probe waits for to know the command
/// region actually opened and closed.
///
/// Split in the command and joined in the output, on the same discipline as
/// `bt-pty`'s `ORACLE_HELD_MARKER`: the typed line is echoed back keystroke by
/// keystroke, so a marker spelled the same way in both would be matched by the child
/// *repeating the request* rather than by the child obeying it.
const RAN_MARKER: &str = "BT_OSC133_RAN";
const RAN_COMMAND: &str = "Write-Output ('BT_OSC133' + '_RAN')\r";

/// FinalTerm/iTerm2 `OSC 133`, BEL-terminated, exactly as `folio.ps1` writes it.
const PROMPT_START: &[u8] = b"\x1b]133;A\x07";
const COMMAND_START: &[u8] = b"\x1b]133;B\x07";
/// The introducer alone — what the red arm asserts the absence of, so that a build
/// which started sending some *other* member of the family from somewhere else would
/// be caught rather than quietly tolerated.
const ANY_MARKER: &[u8] = b"\x1b]133;";

/// How long the probe waits on a child that has gone **silent**, as opposed to one
/// that is merely slow — `bt-pty`'s `PROBE_SILENCE_BUDGET` and `bt-app`'s
/// `real_powershell_input_reaches_a_viewport_owned_frame` to the second, and for the
/// reasons written out at length there: a wall-clock total measures the machine, a
/// silence budget measures the child. A real PowerShell reaching a real prompt is
/// slow, and on a host running five of these lines at once it is slower still.
const SILENCE_BUDGET: Duration = Duration::from_secs(30);
/// The backstop for the one shape silence cannot catch: a child that talks forever
/// without ever producing what is being waited for.
const CEILING: Duration = Duration::from_secs(180);
/// How long the child must say nothing before the probe calls the burst finished —
/// the same hundred milliseconds `bt-pty`'s oracle calls quiet.
const QUIET: Duration = Duration::from_millis(100);

/// The script, as a path PowerShell can dot-source.
///
/// **Not `canonicalize`d**, and the reason is sharper than the tidiness one: on Windows
/// that returns the verbatim `\\?\D:\…` spelling, and PowerShell's execution policy
/// classifies a path beginning `\\` as a *remote* one. Under the RemoteSigned policy a
/// stock Windows carries, the same unsigned script that dot-sources fine from the
/// user's own `$PROFILE` is then refused for want of a digital signature — measured
/// here on 2026-08-23, which is how this comment came to be written. The refusal
/// arrives as ordinary text on the shell's error stream, the prompt appears anyway,
/// and the only symptom is markers that never come: precisely the failure this file
/// exists to catch, manufactured by the test itself.
///
/// Handing over the ordinary path is also what the product does, so the argument the
/// child receives here is the argument a real pane's `$PROFILE` line would carry.
fn script_path() -> PathBuf {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/shell-integration/folio.ps1");
    assert!(
        path.is_file(),
        "the integration script ships in the repository: {}",
        path.display()
    );
    path
}

/// The two PowerShell generations this product supports, as far as this machine has
/// them.
///
/// `powershell.exe` is **not** optional and is never skipped: it is part of Windows,
/// it is the floor `bt_pty::WINDOWS_POWERSHELL` documents every fallback against, and
/// two other test files in this repository already spawn it without asking. A gate
/// that quietly passes when its subject is missing is not a gate.
///
/// PowerShell 7 genuinely is optional — MSI, `winget` and the Store each put it
/// somewhere different and only one of them reliably puts it on `PATH` — so it is
/// resolved through the product's own `resolve_powershell_seven` and, when that
/// answers `None`, the arm is skipped with a line on stderr saying so. Asking the
/// resolver rather than composing a path is the 2026-08-21 lesson from `$PROFILE`
/// (`docs/HANDOFF-2026-08-21.md` §5): whether a machine has something is a question
/// for the machine.
fn generations() -> Vec<String> {
    let mut shells = vec![WINDOWS_POWERSHELL.to_owned()];
    match resolve_powershell_seven(&SystemShellEnvironment) {
        Some(pwsh) => shells.push(pwsh.to_string_lossy().into_owned()),
        None => eprintln!(
            "BT_OSC133_PROBE skipped=pwsh.exe reason=not-installed \
             (PATH, %ProgramFiles%\\PowerShell\\7 and the Store alias were all asked)"
        ),
    }
    shells
}

/// One PowerShell on the other end of a real pty, with a terminal in front of it.
///
/// The terminal is not decoration and not the subject: ConPTY opens every session by
/// asking the terminal who it is and where its cursor is, and a probe that reads bytes
/// without ever answering is a terminal that has stopped responding — which is a fact
/// about the probe that the child would then be measured through. So the bytes go into
/// a `DualPlaneSession` and its replies go back, exactly as a pane does it. What the
/// assertions read is the *raw* stream kept alongside, because the claim under test is
/// about bytes on the wire and a marker the parser consumed is still a marker that
/// arrived.
struct ShellProbe {
    pty: PtySession,
    session: DualPlaneSession,
    raw: Vec<u8>,
    started: Instant,
    last_output: Instant,
}

impl ShellProbe {
    /// `shell`, started the way a pane starts it, running `startup` and staying open.
    ///
    /// `-NoProfile` is the isolation and is load-bearing twice over: the user's own
    /// `$PROFILE` is not read, so nothing they have installed can make the red arm
    /// green, and nothing this test does can reach a file they own.
    fn spawn(shell: &str, startup: &str) -> Self {
        let columns = NonZeroU16::new(80).unwrap();
        let rows = NonZeroU16::new(20).unwrap();
        let command = PtyCommand::interactive_shell(shell)
            .arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-NoExit")
            .arg("-Command")
            .arg(startup);
        let pty = PtySession::spawn(command, PtySize::cells(columns, rows), Arc::new(|| {}))
            .unwrap_or_else(|error| panic!("{shell} starts on a supported host: {error:?}"));
        Self {
            pty,
            session: DualPlaneSession::new(
                NonZeroU32::new(u32::from(columns.get())).unwrap(),
                NonZeroU32::new(u32::from(rows.get())).unwrap(),
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

    fn screen_has(&self, needle: &str) -> bool {
        self.session
            .terminal()
            .visible_text()
            .iter()
            .any(|row| row.contains(needle))
    }

    /// Pump until `needle` is on the child's screen, giving up only once the child has
    /// stopped talking for [`SILENCE_BUDGET`].
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

    /// Pump until the child has said nothing for [`QUIET`].
    ///
    /// This is what makes "every prompt that opened also opened its input" a
    /// determinate question: a stream sampled mid-prompt can hold an `A` whose `B` is
    /// still in flight, and the difference between that and a lost `B` is only time.
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
             silent and {} bytes read in all; screen {:?}",
            self.started.elapsed(),
            silent_for,
            self.raw.len(),
            self.session.terminal().visible_text()
        );
    }

    /// Let the child reach a prompt, run one command, and reach the next prompt; then
    /// hand back everything it wrote.
    ///
    /// The command is not decoration either. `A` opens a prompt and `B` opens the
    /// input it is asking for, so a session that never submits anything has only ever
    /// been *at* its first prompt — the one place where a marker that is emitted late
    /// and a marker that is never emitted look the same. Driving a whole command
    /// region through and coming out at a second prompt is what tells them apart.
    fn run_one_command(mut self) -> Vec<u8> {
        self.wait_for_screen(PROMPT_ON_SCREEN);
        self.pty.write(RAN_COMMAND.as_bytes()).unwrap();
        self.wait_for_screen(RAN_MARKER);
        self.settle();
        let raw = std::mem::take(&mut self.raw);
        // Ask before killing: an `exit` the child obeys is a shell that shut its own
        // console down, and `PtySession::shutdown` (which `Drop` calls anyway) is left
        // holding a process that has already gone.
        let _ = self.pty.write(b"exit\r");
        let _ = self.pty.shutdown();
        raw
    }
}

/// A shell with a known prompt and a line editor that writes nothing to disk.
///
/// `SaveNothing` is isolation, not tidiness: PSReadLine's default is to append every
/// line typed in any session to one file in the user's own profile directory, and a
/// test suite has no business writing there.
fn bare_startup() -> String {
    format!(
        "Set-PSReadLineOption -HistorySaveStyle SaveNothing; \
         function global:prompt {{ '{PROMPT}' }}"
    )
}

/// The same shell with the integration dot-sourced — the single difference between the
/// two arms.
///
/// After the prompt definition, because that is where a user's own dot-source line
/// sits: the script wraps whatever `prompt` it finds, and one installed before the
/// prompt it is meant to wrap would be wrapping a different function.
fn integrated_startup() -> String {
    format!(
        "{}; . '{}'",
        bare_startup(),
        script_path().display().to_string().replace('\'', "''")
    )
}

fn occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

fn position(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// RED FORM — a PowerShell that has not been handed `folio.ps1` sends no OSC 133 at
/// all, so the green test below is measuring the script and not the shell.
///
/// This is the arm that makes the pair mean something, and it is also the state the
/// user's own machine was in until 2026-08-14: PowerShell integration is opt-in
/// because `pwsh` has one startup file and no argument that would name a second, so
/// the only automatic install would be writing into a file that belongs to the user
/// (`bt_app::shell_integration`, `docs/DESIGN.md` §7.1.6j). A shell in this state is
/// perfectly usable and looks completely normal, which is exactly why nobody notices:
/// every OSC 133 consequence this terminal ships — the prompt-start mouse retirement
/// of §7.1.5i, the busy breath, the exit-code dot — is simply absent.
///
/// The positive marker is asserted first on purpose. Without it this test could pass
/// by having done nothing at all, which is the failure mode every absence assertion
/// has.
#[test]
fn a_powershell_without_the_integration_script_sends_no_osc_133_at_all() {
    for shell in generations() {
        let raw = ShellProbe::spawn(&shell, &bare_startup()).run_one_command();
        assert!(
            position(&raw, RAN_MARKER.as_bytes()).is_some(),
            "{shell}: the bare arm has to have actually run its command, or its silence \
             on OSC 133 is the silence of a shell that never started"
        );
        assert_eq!(
            occurrences(&raw, ANY_MARKER),
            0,
            "{shell}: a shell nobody handed the integration to announces no prompt \
             regions; {:?}",
            String::from_utf8_lossy(&raw)
        );
    }
}

/// GREEN — dot-source `folio.ps1` into that same shell and both markers arrive on the
/// wire: `133;A` opening every prompt and `133;B` opening the input it asks for, in
/// that order, through a real console host and a real ConPTY.
///
/// The two counts are asserted equal rather than merely non-zero because A and B are
/// one statement in two halves — a prompt that announced itself and then never opened
/// an input region would leave this terminal waiting for a command that had already
/// been typed. `folio.ps1` emits both from the wrapped `prompt`, one before the
/// customizer chain and one after, and only at depth 0; the equality is that ruling
/// read back off the wire.
#[test]
fn the_integration_script_makes_a_real_powershell_announce_prompt_start_and_command_start() {
    for shell in generations() {
        let raw = ShellProbe::spawn(&shell, &integrated_startup()).run_one_command();
        let text = String::from_utf8_lossy(&raw);
        let prompt_starts = occurrences(&raw, PROMPT_START);
        let command_starts = occurrences(&raw, COMMAND_START);
        eprintln!(
            "BT_OSC133_PROBE shell={shell} conpty={:?} prompt_starts={prompt_starts} \
             command_starts={command_starts} bytes={}",
            bt_pty::conpty_source(),
            raw.len()
        );
        assert!(
            prompt_starts > 0,
            "{shell}: the prompt start never reached the pty; {text:?}"
        );
        assert!(
            command_starts > 0,
            "{shell}: the command start never reached the pty; {text:?}"
        );
        assert_eq!(
            prompt_starts, command_starts,
            "{shell}: every prompt that opened a region also opened its input; {text:?}"
        );
        assert!(
            position(&raw, PROMPT_START) < position(&raw, COMMAND_START),
            "{shell}: A opens the prompt whose input B opens, so A comes first; {text:?}"
        );
    }
}
