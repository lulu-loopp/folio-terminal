//! The bash half of shell integration, run for real.
//!
//! The companion of `shell_integration_script.rs`: that one pins the PowerShell
//! script against a real Windows PowerShell, this one pins
//! `scripts/shell-integration/folio.bash` against a real Git Bash. Both
//! are round trips rather than shape matches — the script's own bytes go into a
//! session, so the encoder and the decoder are held against each other and
//! neither can drift alone.
//!
//! Git Bash is found the way the product finds it: `git.exe` on `PATH`, then
//! `<root>\bin\bash.exe` beside it. A machine that can clone this repository has
//! `git.exe`, and Git for Windows has never shipped without its bash, so this is
//! a real gate rather than one that quietly passes when the tool is missing.

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use bt_term::DualPlaneSession;

fn nz(value: u32) -> std::num::NonZeroU32 {
    std::num::NonZeroU32::new(value).unwrap()
}

/// The script, as a path bash can open.
///
/// **Not `canonicalize`d**, unlike the PowerShell test's: on Windows that
/// returns the verbatim `\\?\D:\…` spelling, which the Win32 API accepts and
/// MSYS's own path layer does not — bash is handed a filename it cannot open,
/// reads no startup file at all, and the shell that comes back looks almost
/// right. The product hands over an ordinary path for the same reason.
fn script_path() -> PathBuf {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/shell-integration/folio.bash");
    assert!(
        path.is_file(),
        "the integration script ships in the repository: {}",
        path.display()
    );
    path
}

/// `<git root>\bin\bash.exe`, reached through the `git.exe` the machine already
/// has on its path — `profiles::ProgramCandidate::BesideOnPath`'s own rule.
fn git_bash() -> PathBuf {
    let listed = Command::new("where.exe")
        .arg("git.exe")
        .output()
        .expect("where.exe is part of Windows");
    // PATH may name git through `<root>\cmd\git.exe` (plain shells) or through
    // `<root>\mingw64\bin\git.exe` (a shell Git Bash itself set up), so the
    // install root is *some* ancestor of *some* hit — walk them all and take
    // the first ancestor that truly carries `bin\bash.exe`, mirroring the
    // production `BesideOnPath` rule.
    let listing = String::from_utf8_lossy(&listed.stdout).into_owned();
    listing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .flat_map(|hit| {
            hit.ancestors()
                .skip(1)
                .map(|ancestor| ancestor.join(r"bin\bash.exe"))
                .collect::<Vec<_>>()
        })
        .find(|candidate| candidate.is_file())
        .expect("Git for Windows ships bin\\bash.exe under an ancestor of git.exe")
}

/// A directory whose name carries a space and CJK, so the script's own
/// percent-encoder is exercised on the byte that must become `%20` and on
/// multi-byte characters that must become their UTF-8 escapes.
fn temporary_directory() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "betterterminal 图 片-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    directory
}

/// Everything a Git Bash started the way Folio starts it writes, while
/// running `commands`.
///
/// The argument list is the product's: `--init-file <script> -i`, with
/// `BT_SHELL_INTEGRATION` set — which is the whole of the injection, so a
/// mistake in it fails here rather than only on a real machine.
///
/// **Both streams, joined by the shell itself.** `PS1` is written by readline,
/// whose output stream is *stderr*, while the `printf`s in `PROMPT_COMMAND` go
/// to stdout — so the A and B markers and the C, D and OSC 7 reports arrive on
/// two different pipes here. In a terminal there is only one: a pty is one
/// device that both file descriptors are opened on, and the order they interleave
/// in is the order the terminal sees. Reading two pipes would lose exactly that
/// ordering, which is half of what these tests assert, so the redirection is done
/// *inside* the shell — `2>&1` before `exec` — which reproduces the single
/// device rather than reassembling it afterwards.
fn session_bytes(directory: &Path, commands: &str) -> Vec<u8> {
    let bash = git_bash();
    let output = Command::new(&bash)
        .arg("-c")
        .arg(format!(
            "exec '{}' --init-file '{}' -i 2>&1",
            bash.display(),
            script_path().display()
        ))
        .current_dir(directory)
        .env("BT_SHELL_INTEGRATION", "1")
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
        .expect("Git Bash starts");
    output.stdout
}

/// PIN — the script reports where the shell is, in the spelling this terminal
/// speaks, and a session fed its exact bytes ends up holding that directory.
///
/// **The Windows spelling, from `pwd -W`.** Git Bash's `$PWD` says
/// `/c/Users/...`, which is a third path namespace that nothing else in this
/// product speaks and that no `is_dir` check, no relative-image resolution and
/// no inheritance into a PowerShell tab could read. Its *process* stands in a
/// Win32 directory, and that is what it reports. Red gate: reporting `$PWD`
/// passes any shape test of the URI and produces a session whose working
/// directory is a path that does not exist.
#[test]
fn git_bash_reports_its_working_directory_as_the_windows_directory_it_is_in() {
    let directory = temporary_directory();
    let bytes = session_bytes(&directory, "exit\n");
    let text = String::from_utf8_lossy(&bytes).into_owned();

    let report_start = text.find("\u{1b}]7;").expect("one OSC 7 report per prompt");
    let report_end = report_start
        + text[report_start..]
            .find('\u{7}')
            .expect("BEL terminates the report");
    let uri = &text[report_start + 4..report_end];
    assert!(
        uri.starts_with("file:///"),
        "an empty authority is the file-URI spelling of this host: {uri:?}"
    );
    assert!(
        uri.contains("%20") && uri.contains("%E5%9B%BE"),
        "a space is %20 and CJK is its UTF-8 escapes, encoded byte by byte: {uri:?}"
    );
    assert!(
        !uri.contains(' ') && uri.is_ascii(),
        "a URI on the wire is ASCII with no literal space: {uri:?}"
    );

    let mut session = DualPlaneSession::new(nz(120), nz(8));
    session.feed(&bytes).unwrap();
    assert_eq!(
        session.working_directory(),
        Some(directory.as_path()),
        "the session holds the Windows directory the shell was actually in — \
         not the MSYS spelling of it, which names nothing here"
    );

    std::fs::remove_dir(&directory).unwrap();
}

/// PIN — A, B, C and D all arrive, in that order, and D carries the exit code
/// the command actually returned.
///
/// Red gate for each half separately: without the `PS1` wrapping there is no A
/// or B and every prompt is indistinguishable from output; without the `DEBUG`
/// trap there is no C and the command line is never closed; without the
/// `PROMPT_COMMAND` hook there is no D and nothing ever reports success or
/// failure. Each of those is a shell that still works perfectly for the person
/// typing in it, which is why none of them would be noticed by hand.
#[test]
fn git_bash_marks_every_command_region_and_reports_the_exit_code() {
    let directory = temporary_directory();
    let bytes = session_bytes(&directory, "true\nfalse\nexit\n");
    let text = String::from_utf8_lossy(&bytes).into_owned();

    for marker in [
        "\u{1b}]133;A\u{7}",
        "\u{1b}]133;B\u{7}",
        "\u{1b}]133;C\u{7}",
    ] {
        assert!(text.contains(marker), "{marker:?} never arrived: {text:?}");
    }
    let first_prompt = text.find("\u{1b}]133;A\u{7}").unwrap();
    assert!(
        text.find("\u{1b}]7;").unwrap() < first_prompt,
        "the directory is reported before the prompt region it describes"
    );
    assert!(
        first_prompt < text.find("\u{1b}]133;B\u{7}").unwrap(),
        "B opens the input the prompt A opened is asking for"
    );
    // `true` then `false`: the exit code is the command's own and not a constant.
    assert!(
        text.contains("\u{1b}]133;D;0\u{7}"),
        "a command that succeeded closes with 0: {text:?}"
    );
    assert!(
        text.contains("\u{1b}]133;D;1\u{7}"),
        "a command that failed closes with its own code, not with 0: {text:?}"
    );

    std::fs::remove_dir(&directory).unwrap();
}

/// PIN — the script leaves the user's own startup files in charge.
///
/// `--init-file` displaces `~/.bashrc` and, because Folio also drops
/// `--login` to make the flag take effect at all, `/etc/profile` with it. On Git
/// for Windows `/etc/profile` is what puts `/mingw64/bin` on the path, so a
/// shell that skipped it is a Git Bash that cannot find git — the one thing that
/// profile exists to provide. Red gate: an injection that only adds the init
/// file and never runs the chain passes every marker test above and hands the
/// user a shell with a broken `PATH`.
#[test]
fn the_startup_chain_the_init_file_displaced_is_put_back() {
    let directory = temporary_directory();
    let bytes = session_bytes(&directory, "command -v git; echo MSYSTEM=$MSYSTEM\nexit\n");
    let text = String::from_utf8_lossy(&bytes).into_owned();
    assert!(
        text.contains("/mingw64/bin/git") || text.contains("/bin/git"),
        "`/etc/profile` ran and git is on the path: {text:?}"
    );
    assert!(
        text.contains("MSYSTEM=MINGW"),
        "the MSYS environment the login chain sets up is present: {text:?}"
    );
    std::fs::remove_dir(&directory).unwrap();
}
