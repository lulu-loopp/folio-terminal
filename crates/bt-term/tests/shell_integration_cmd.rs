//! The `cmd.exe` half of shell integration, run for real.
//!
//! The third of the round trips: `shell_integration_script.rs` pins `folio.ps1`
//! against a real Windows PowerShell, `shell_integration_bash.rs` pins
//! `folio.bash` against a real Git Bash, and this one pins the `PROMPT` string
//! against a real `cmd.exe`. There is no script here to point at — `cmd.exe` has
//! no startup file and no hook, so its whole integration is a format string
//! (`bt_app::profiles::Integration::CmdPrompt`) — and the round trip is what
//! makes that string checkable at all: the shell's own expansion of it goes into
//! a session, so the format string and the decoder are held against each other
//! and neither can drift alone.
//!
//! **Why the string is spelled again here.** `bt-app` is a binary crate with no
//! library target, so an integration test cannot call into it. The literal below
//! is therefore a copy, and the copy is guarded rather than trusted: the last
//! assertion of `the_prompt_this_test_runs_is_the_prompt_the_product_sets` reads
//! `crates/bt-app/src/shell_integration.rs` and requires the two marker
//! constants and the report to be in it, character for character. A rewrite of
//! either half turns this file red rather than leaving it quietly testing a
//! string nothing ships.

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use bt_term::DualPlaneSession;

fn nz(value: u32) -> std::num::NonZeroU32 {
    std::num::NonZeroU32::new(value).unwrap()
}

/// What Folio puts in a Command Prompt child's environment.
///
/// `$e` is the escape character and `$P` the current drive and path — two of the
/// dozen substitutions `cmd.exe` performs on this string, and the only two that
/// matter. `$e\` is `ESC \`, the string terminator, used because `PROMPT` has no
/// code that produces a `BEL` byte.
const PROMPT: &str = r"$e]133;D$e\$e]7;file:///$P$e\$e]133;A$e\$P$G";

/// The product's own source, so that the copy above cannot drift away from it.
fn shell_integration_source() -> String {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bt-app/src/shell_integration.rs");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is in the repository: {error}", path.display()))
}

fn temporary_directory() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("folio-cmd-{}-{unique}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    directory
}

/// Everything a `cmd.exe` started the way Folio starts it writes, while running
/// `commands`.
///
/// `/d` skips the `AutoRun` registry value, which is somebody's own machine
/// setup and not part of what is under test — the same discipline every other
/// probe in this repository applies to a real user's configuration.
fn session_bytes(directory: &Path, commands: &str) -> Vec<u8> {
    let output = Command::new(
        std::env::var_os("ComSpec").unwrap_or_else(|| r"C:\Windows\System32\cmd.exe".into()),
    )
    .arg("/d")
    .current_dir(directory)
    .env("PROMPT", PROMPT)
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::null())
    .spawn()
    .and_then(|mut child| {
        use std::io::Write;
        child
            .stdin
            .take()
            .expect("stdin was piped")
            .write_all(commands.as_bytes())?;
        child.wait_with_output()
    })
    .expect("cmd.exe ships with Windows");
    output.stdout
}

/// PIN — the string this file runs is the string the product sets.
///
/// Red gate: without it, editing `cmd_prompt` to emit anything else leaves every
/// assertion below passing about a prompt no pane is ever given.
#[test]
fn the_prompt_this_test_runs_is_the_prompt_the_product_sets() {
    let source = shell_integration_source();
    for piece in [
        r#"const CMD_MARKS_BEFORE_REPORT: &str = r"$e]133;D$e\";"#,
        r#"const CMD_MARKS_AFTER_REPORT: &str = r"$e]133;A$e\";"#,
        r#"const CMD_OSC7: &str = r"$e]7;file:///$P$e\";"#,
    ] {
        assert!(
            source.contains(piece),
            "bt-app must still declare {piece:?} — this file's copy of the prompt is stale"
        );
    }
}

/// PIN — a real `cmd.exe` expands the marks, and a session fed its exact bytes
/// ends up with one command record per prompt and the directory it was standing
/// in.
///
/// Red gate on each half separately: with the markers dropped the rail has
/// nothing to draw for the one profile whose reader cannot install a script
/// instead; with the report dropped every relative image path in a `cmd` pane
/// goes undetected. Neither is visible by hand until somebody looks for a tick
/// that was never there.
#[test]
fn command_prompt_marks_every_prompt_and_reports_its_working_directory() {
    let directory = temporary_directory();
    let bytes = session_bytes(&directory, "echo one\r\necho two\r\nexit\r\n");
    let text = String::from_utf8_lossy(&bytes).into_owned();

    assert!(
        text.contains("\u{1b}]133;D\u{1b}\\"),
        "the previous command's end: {text:?}"
    );
    assert!(
        text.contains("\u{1b}]133;A\u{1b}\\"),
        "this prompt's start: {text:?}"
    );
    assert!(
        !text.contains("\u{1b}]133;B") && !text.contains("\u{1b}]133;C"),
        "and neither of the two that would open a region cmd can never close: {text:?}"
    );

    let mut session = DualPlaneSession::new(nz(120), nz(24));
    session.feed(&bytes).unwrap();
    assert_eq!(
        session.working_directory(),
        Some(directory.as_path()),
        "the session holds the directory the shell was actually in, spelled the one way \
         `PROMPT` can spell it"
    );

    // Three prompts were printed — one before `echo one`, one before `echo two`,
    // one before `exit` — so three records, each with a prompt row to jump to.
    let marks = session.command_marks();
    assert!(
        marks.len() >= 3,
        "one record per prompt: {} records for three prompts",
        marks.len()
    );
    assert!(
        marks.iter().all(|mark| mark.prompt.is_some()),
        "every record was opened by an `A`, so every tick has a prompt row to land on"
    );
    assert!(
        marks[0].finished.is_some() && marks[0].exit_code.is_none(),
        "the next prompt's `D` ended the first command, and said nothing about how — \
         `PROMPT` cannot read ERRORLEVEL"
    );

    std::fs::remove_dir(&directory).unwrap();
}
