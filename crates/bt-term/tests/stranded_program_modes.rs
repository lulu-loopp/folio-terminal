//! **A full-screen program killed from outside, under a real shell with Folio's integration,
//! through a real pty** (T-RESET-MODES, ledger #19/#20).
//!
//! `bt-term`'s own tests feed the bytes; these make a real shell and a real pty produce them. The
//! program enters the alternate screen and turns on any-motion mouse tracking, SGR reports, the
//! kitty keyboard protocol and modifyOtherKeys, prints its process id and sleeps; the test ends it
//! by that id — a process this test started — and pumps until the shell's next prompt.
//!
//! **What the probe recorded (2026-10-08) and these tests now hold.** Nothing between the program
//! and Folio leaves the alternate screen for a dead program: ConPTY (Windows PowerShell 5.1 and
//! PowerShell 7.6 over the shipped pseudoconsole) sends, after the kill, the window title, then
//! `133;D;1`, `OSC 7`, `133;A`, the prompt and `133;B`; zsh over a Unix pty sends
//! `zsh: killed …`, its `PROMPT_SP` line, `133;D;137`, `OSC 7`, `133;A`, the prompt, `133;B`
//! and `?2004h`. Neither sends `?1049l` or turns a mode off. So every mode the program set is
//! still on when the shell speaks, and the shell speaks on the alternate screen — the order the
//! session's `D` handler reads as a stranded alternate screen.
//!
//! The assertion is the outcome the person sees: back on the primary screen, the shell's prompt
//! there, and none of the program's modes left on. Without the stranded-screen rule the pane
//! stays on the alternate screen and the wait for a prompt on the primary one gives up, saying
//! what the screen holds.
//!
//! **A stopped program is the other half** (round 2): the same program stopped from outside under
//! zsh is alive, and the shell's `D` for it carries 128 + the stop signal. Its screen and modes
//! stay, so `fg` resumes it where it was.
//!
//! **Nothing here touches the user's own shell**: `bt_pty::test_shell` starts it without the
//! user's startup files, with a line editor that saves no history and a temporary home.

#![allow(clippy::disallowed_methods)]

use std::{
    num::{NonZeroU16, NonZeroU32},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use bt_pty::test_shell::TestShell;
use bt_pty::{PtyCommand, PtySize};
use bt_term::{DualPlaneSession, ModifyOtherKeys, MouseTracking};

/// The prompt as a row of the screen holds it (the probe's prompt is this and a space).
const PROMPT_ON_SCREEN: &str = "BTSTR>";
/// What the program prints before its process id. Spelled in two halves in every command that
/// asks for it, so the echo of the typed line cannot be read as the program's answer.
const PID_MARKER: &str = "BTPID=";
/// How long the probe waits on a child that has gone silent, and the backstop for one that talks
/// without ever producing what is waited for — `bt-pty`'s `shell_integration_osc133` budgets.
const SILENCE_BUDGET: Duration = Duration::from_secs(30);
const CEILING: Duration = Duration::from_secs(180);
/// How long the child must say nothing before a burst is called finished.
const QUIET: Duration = Duration::from_millis(300);
const COLUMNS: u16 = 100;
const ROWS: u16 = 20;

/// The modes the program turns on: the alternate screen, any-motion tracking, SGR reports, the
/// kitty disambiguate flag (a push) and modifyOtherKeys 2.
const PROGRAM_MODES: &str = "\x1b[?1049h\x1b[?1003h\x1b[?1006h\x1b[>1u\x1b[>4;2m";

fn script(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/shell-integration")
        .join(name);
    assert!(
        path.is_file(),
        "the integration script ships: {}",
        path.display()
    );
    path
}

/// One shell on the other end of a real pty, with a session in front of it answering its
/// queries the way a pane does.
struct Probe {
    pty: TestShell,
    session: DualPlaneSession,
    raw: Vec<u8>,
    started: Instant,
    last_output: Instant,
}

impl Probe {
    fn spawn(command: PtyCommand) -> Self {
        let columns = NonZeroU16::new(COLUMNS).unwrap();
        let rows = NonZeroU16::new(ROWS).unwrap();
        let pty = TestShell::spawn(command, PtySize::cells(columns, rows))
            .expect("the shell starts on a supported host");
        let mut session = DualPlaneSession::new(
            NonZeroU32::new(u32::from(COLUMNS)).unwrap(),
            NonZeroU32::new(u32::from(ROWS)).unwrap(),
        );
        session.set_pty_transport(if cfg!(windows) {
            bt_term::PtyTransport::ConPty
        } else {
            bt_term::PtyTransport::Unix
        });
        Self {
            pty,
            session,
            raw: Vec::new(),
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
            self.pty.reply(&reply).unwrap();
        }
    }

    fn rows_holding(&self, needle: &str) -> usize {
        self.session
            .terminal()
            .visible_text()
            .iter()
            .filter(|row| row.contains(needle))
            .count()
    }

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

    fn give_up_if_stalled(&self, waiting_for: &str) {
        let silent_for = self.last_output.elapsed();
        assert!(
            silent_for < SILENCE_BUDGET && self.started.elapsed() < CEILING,
            "gave up waiting for {waiting_for} after {:?}, the last {silent_for:?} silent; \
             modes {:?}; screen {:?}; the last bytes {}",
            self.started.elapsed(),
            self.session.terminal_modes(),
            self.session.terminal().visible_text(),
            self.raw[self.raw.len().saturating_sub(400)..].escape_ascii()
        );
    }

    fn wait_until(&mut self, waiting_for: &str, done: impl Fn(&Self) -> bool) {
        loop {
            self.pump_once();
            if done(self) {
                return;
            }
            self.give_up_if_stalled(waiting_for);
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn settle(&mut self) {
        self.wait_until("the child to fall quiet", |probe| {
            probe.last_output.elapsed() >= QUIET
        });
    }

    /// Run `program_command` at the prompt and wait until the program it starts has set its
    /// modes; its process id.
    fn start_a_full_screen_program(&mut self, program_command: &str) -> String {
        self.wait_until("the first prompt", |probe| {
            probe.rows_holding(PROMPT_ON_SCREEN) == 1
        });
        self.settle();
        self.pty.write(program_command.as_bytes()).unwrap();
        self.wait_until("the program's process id", |probe| {
            probe.value_after(PID_MARKER).is_some()
        });
        self.settle();
        assert_program_modes_on(&self.session);
        self.value_after(PID_MARKER).unwrap()
    }

    /// Run `program_command` at the prompt, end the program it starts with `end` once it has
    /// set its modes, and check the pane the shell comes back to.
    fn kill_a_full_screen_program(mut self, program_command: &str, end: impl Fn(&str)) {
        let pid = self.start_a_full_screen_program(program_command);
        end(&pid);
        // The prompt the command was typed at, and the one the shell draws after it — both on the
        // primary screen.
        self.wait_until("the shell's next prompt on the primary screen", |probe| {
            !probe.session.terminal_modes().alternate_screen
                && probe.rows_holding(PROMPT_ON_SCREEN) >= 2
        });
        self.settle();
        let modes = self.session.terminal_modes();
        assert!(!modes.alternate_screen);
        assert_eq!(modes.mouse_tracking, MouseTracking::Off);
        assert!(!modes.sgr_mouse);
        assert_eq!(modes.keyboard.kitty, 0);
        assert_eq!(modes.keyboard.modify_other_keys, ModifyOtherKeys::Off);
        assert!(self.session.shell_prompt_opened_in_order());

        let _ = self.pty.write(b"exit\r");
        let _ = self.pty.shutdown();
    }
}

fn assert_program_modes_on(session: &DualPlaneSession) {
    let modes = session.terminal_modes();
    assert!(modes.alternate_screen, "the program's screen is up");
    assert_eq!(modes.mouse_tracking, MouseTracking::Motion);
    assert!(modes.sgr_mouse);
    assert_eq!(modes.keyboard.kitty, 1);
    assert_eq!(modes.keyboard.modify_other_keys, ModifyOtherKeys::Two);
}

/// A program file under the temporary directory, removed when dropped.
struct ProgramFile(PathBuf);

impl ProgramFile {
    fn new(extension: &str, text: &str) -> Self {
        let path = bt_testpath::temp_path("bt-stranded-program").with_extension(extension);
        std::fs::write(&path, text).unwrap();
        Self(path)
    }
}

impl Drop for ProgramFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// RED (T-RESET-MODES, ruling 2026-10-08 (2) and (3)) — **a full-screen program ended from
/// outside under Windows PowerShell 5.1, and PowerShell 7 where the machine has it, with
/// `folio.ps1` loaded: the pane comes back to the primary screen with the shell's prompt on it and
/// none of the program's modes on.**
///
/// MUTATION: drop the stranded-screen arm from the session's `D` handler — the wait for the prompt
/// on the primary screen gives up with the pane still on the alternate screen.
#[cfg(windows)]
#[test]
fn a_killed_full_screen_program_under_powershell_leaves_the_prompt_on_the_primary_screen() {
    let program = ProgramFile::new(
        "ps1",
        &format!(
            "[Console]::Out.Write(\"{}\")\n[Console]::Out.Flush()\n\
             Write-Output ('BTPI' + 'D=' + $PID)\nStart-Sleep -Seconds 600\n",
            PROGRAM_MODES.replace('\x1b', "$([char]27)")
        ),
    );
    let mut shells = vec![bt_pty::WINDOWS_POWERSHELL.to_owned()];
    match bt_pty::resolve_powershell_seven(&bt_pty::SystemShellEnvironment) {
        Some(pwsh) => shells.push(pwsh.to_string_lossy().into_owned()),
        None => eprintln!("BT_STRANDED skipped=pwsh.exe reason=not-installed"),
    }
    for shell in shells {
        let startup = format!(
            "{} function global:prompt {{ '{PROMPT_ON_SCREEN} ' }}; \
             . ([scriptblock]::Create([IO.File]::ReadAllText('{}')))",
            bt_pty::test_shell::HYGIENE,
            script("folio.ps1").display()
        );
        let command = PtyCommand::interactive_shell(&shell)
            .arg("-NoLogo")
            .arg("-NoExit")
            .arg("-Command")
            .arg(startup);
        Probe::spawn(command).kill_a_full_screen_program(
            &format!(
                "& '{}' -NoProfile -NoLogo -File '{}'\r",
                bt_pty::WINDOWS_POWERSHELL,
                program.0.display()
            ),
            |pid| {
                let status = Command::new("taskkill")
                    .args(["/F", "/PID", pid])
                    .output()
                    .unwrap()
                    .status;
                assert!(status.success(), "{shell}: the program {pid} is ended");
            },
        );
    }
}

/// The program for the zsh arms: its modes, its process id, then a long sleep in its place.
#[cfg(unix)]
fn a_unix_full_screen_program() -> ProgramFile {
    ProgramFile::new(
        "sh",
        &format!(
            "printf '{}'\necho \"BTPI\"\"D=$$\"\nexec sleep 600\n",
            PROGRAM_MODES.replace('\x1b', "\\033")
        ),
    )
}

/// zsh, started the way a test starts a shell, with `folio.zsh` wrapped around the probe's prompt.
#[cfg(unix)]
fn zsh_with_the_integration() -> Probe {
    let mut probe = Probe::spawn(PtyCommand::interactive_shell("/bin/zsh").arg("-i"));
    // zsh's own first prompt, then the probe's.
    probe.settle();
    probe
        .pty
        .write(
            format!(
                "PS1='BTS''TR> '; source '{}'\r",
                script("folio.zsh").display()
            )
            .as_bytes(),
        )
        .unwrap();
    probe
}

#[cfg(unix)]
fn send_signal(signal: &str, pid: &str) {
    let status = Command::new("kill")
        .args([signal, pid])
        .output()
        .unwrap()
        .status;
    assert!(status.success(), "kill {signal} {pid}");
}

/// RED (T-RESET-MODES, ruling 2026-10-08 (2) and (3)) — **the same under zsh with `folio.zsh`
/// over a Unix pty, the program killed with `SIGKILL`.**
///
/// MUTATION: drop the stranded-screen arm from the session's `D` handler — the wait for the prompt
/// on the primary screen gives up with the pane still on the alternate screen.
#[cfg(unix)]
#[test]
fn a_killed_full_screen_program_under_zsh_leaves_the_prompt_on_the_primary_screen() {
    let program = a_unix_full_screen_program();
    zsh_with_the_integration().kill_a_full_screen_program(
        &format!("sh '{}'\r", program.0.display()),
        |pid| {
            send_signal("-KILL", pid);
        },
    );
}

/// RED (T-RESET-MODES round 2) — **a full-screen program stopped from outside under zsh keeps its
/// screen and its modes**: zsh reports the stopped job (`$?` = 128 + `SIGSTOP`) and draws its
/// prompt on the program's screen, as before the stranded-screen rule, so `fg` can resume the
/// program where it was. The program is then ended with `SIGKILL`.
///
/// MUTATION: drop the `JOB_STOPPED_EXIT_CODES` check from the stranded arm — the pane returns to
/// the primary screen and the wait for the prompt on the program's screen gives up.
#[cfg(unix)]
#[test]
fn a_stopped_full_screen_program_under_zsh_keeps_its_screen() {
    let program = a_unix_full_screen_program();
    let mut probe = zsh_with_the_integration();
    let pid = probe.start_a_full_screen_program(&format!("sh '{}'\r", program.0.display()));
    send_signal("-STOP", &pid);
    probe.wait_until(
        "the shell's prompt on the stopped program's screen",
        |probe| {
            probe.session.terminal_modes().alternate_screen
                && probe.rows_holding(PROMPT_ON_SCREEN) >= 1
        },
    );
    probe.settle();
    assert_program_modes_on(&probe.session);
    send_signal("-KILL", &pid);
    let _ = probe.pty.write(b"exit\r");
    let _ = probe.pty.shutdown();
}
