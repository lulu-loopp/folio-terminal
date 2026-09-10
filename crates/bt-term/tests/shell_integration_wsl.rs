//! The WSL half of shell integration: the question the pane puts to its own
//! distribution, run through a real POSIX `sh`.
//!
//! The fourth of the round trips. `shell_integration_script.rs` pins
//! `folio.ps1` against a real Windows PowerShell, `shell_integration_bash.rs`
//! pins `folio.bash` against a real Git Bash, `shell_integration_cmd.rs` pins
//! the `PROMPT` string against a real `cmd.exe`, and this one pins
//! `bt_app::shell_integration::WSL_LOGIN_SHELL` — the six-word script that
//! decides, *inside the distribution*, which shell a WSL pane becomes.
//!
//! **Why that script needs a test of its own.** Until 2026-09-07 the decision
//! was made on the Windows side, from an answer a second `wsl.exe` brought back
//! — and nothing waited for it, so the first WSL pane of every run composed its
//! command line before the answer existed and was started with no init file at
//! all (`docs/plans/shell-matrix-2026-09-07.md` T-2). Moving the decision into
//! the pane removes the race and puts the whole of it in one string of shell,
//! where a typo has exactly two symptoms and both are silent: every WSL pane
//! loses its markers, or a reader whose login shell is zsh has it replaced by
//! bash every time they open a tab.
//!
//! **The shell here is a real `sh`, and the distribution is written down.**
//! `getent`, the login shell and the init file are all fixtures on a temporary
//! `PATH`, so what runs is the product's own script against a password database
//! this test controls — every branch of it, on a machine that need not have WSL
//! installed. `sh.exe` is found the way `shell_integration_bash.rs` finds bash:
//! `git.exe` on `PATH`, then `<root>\bin\` beside it. A machine that can clone
//! this repository has `git.exe`, and Git for Windows has never shipped without
//! its POSIX shell, so this is a real gate rather than one that quietly passes
//! when the tool is missing.
//!
//! **Why the script is spelled again here.** `bt-app` is a binary crate with no
//! library target, so an integration test cannot call into it — the same
//! constraint `shell_integration_cmd.rs` is under, and the same answer: the
//! fragments below are a copy, and the copy is guarded rather than trusted.
//! `the_question_this_test_runs_is_the_question_the_product_asks` reads
//! `crates/bt-app/src/shell_integration.rs` and requires every one of them to be
//! in it, character for character.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

/// The product's question, fragment by fragment, in the spelling its source
/// carries.
///
/// Split the way the constant is split so that each piece can be found in the
/// source on its own: a rewrite that moves a branch turns
/// `the_question_this_test_runs_is_the_question_the_product_asks` red rather
/// than leaving this file quietly exercising a script nothing ships.
const QUESTION: [&str; 11] = [
    r#"shell=$(getent passwd "$(id -u)" 2>/dev/null | cut -d: -f7); "#,
    r#"[ -n "$shell" ] || shell=/bin/sh; "#,
    r#"case "${shell##*/}" in "#,
    r#"bash) [ -n "$1" ] || exec "$shell" -l; "#,
    r#"BT_SHELL_INTEGRATION=login; export BT_SHELL_INTEGRATION; "#,
    r#"exec "$shell" --init-file "$1" -i;; "#,
    r#"zsh) [ -n "$2" ] || exec "$shell" -l; "#,
    r#"[ -n "${ZDOTDIR:-}" ] && { BT_USER_ZDOTDIR=$ZDOTDIR; export BT_USER_ZDOTDIR; }; "#,
    r#"ZDOTDIR="$2"; export ZDOTDIR; exec "$shell" -l;; "#,
    r#"*) exec "$shell" -l;; "#,
    "esac",
];

/// What `wsl.exe` is handed as one argument.
fn question() -> String {
    QUESTION.concat()
}

/// The product's own source, so that the copy above cannot drift away from it.
fn shell_integration_source() -> String {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bt-app/src/shell_integration.rs");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is in the repository: {error}", path.display()))
}

/// `<git root>\bin\sh.exe`, reached through the `git.exe` the machine already
/// has on its path — `profiles::ProgramCandidate::BesideOnPath`'s own rule, and
/// the rule `shell_integration_bash.rs` reaches `bash.exe` by.
fn git_sh() -> PathBuf {
    let listed = Command::new("where.exe")
        .arg("git.exe")
        .output()
        .expect("where.exe is part of Windows");
    let listing = String::from_utf8_lossy(&listed.stdout).into_owned();
    listing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .flat_map(|hit| {
            Path::new(hit)
                .ancestors()
                .map(|root| root.join("bin").join("sh.exe"))
                .collect::<Vec<_>>()
        })
        .find(|candidate| candidate.is_file())
        .expect("Git for Windows ships a POSIX sh beside git.exe")
}

/// **Unique per call within one process, not merely per instant.** Two tests in one test
/// binary run on two threads, and a clock that answers the same nanosecond to both — the CI
/// runner did, once — handed them one directory and one `AlreadyExists`. The counter is
/// the part of the name the clock cannot be trusted with.
fn temporary_directory() -> PathBuf {
    static ORDINAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let ordinal = ORDINAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "folio-wsl-{}-{unique}-{ordinal}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    directory
}

/// One executable fixture on the temporary `PATH`.
///
/// A shebang line and nothing else that is MSYS-specific: these are the
/// programs the script under test is entitled to find in a distribution, stood
/// in for by files this test wrote.
fn write_program(directory: &Path, name: &str, body: &str) {
    let path = directory.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
}

/// A shell that reports what it was exec'd as, instead of becoming one, written
/// at `<directory>/<home>/<name>` and given back as the absolute path a password
/// entry would name it by.
///
/// The `--init-file` branch would otherwise start a real interactive bash and
/// this test would be waiting for somebody to type into it. The path is
/// absolute for the same reason it is in a distribution: `getent passwd` names
/// a shell in full, and the arm under test is the one that reads the *name* out
/// of that path.
fn shell_fixture(directory: &Path, home: &str, name: &str) -> String {
    let home = directory.join(home);
    std::fs::create_dir_all(&home).unwrap();
    write_program(
        &home,
        name,
        &format!(
            "echo 'shell={name}'\necho \"argv=$*\"\n\
             echo \"BT_SHELL_INTEGRATION=${{BT_SHELL_INTEGRATION-<unset>}}\"
             echo \"ZDOTDIR=${{ZDOTDIR-<unset>}}\"
             echo \"BT_USER_ZDOTDIR=${{BT_USER_ZDOTDIR-<unset>}}\""
        ),
    );
    msys_spelling(&home.join(name))
}

/// `C:\Users\dev\…\bash` → `/c/Users/dev/…/bash`.
///
/// **A password entry's seventh field cannot carry a drive letter**, and not
/// because MSYS would refuse to `exec` one: `getent passwd` is colon-separated
/// and the product reads it with `cut -d: -f7`, so a `C:` in the path would put
/// the drive in field seven and the rest of the path in field eight. That is a
/// property of the fixture rather than of the product — a real distribution's
/// shell is `/bin/bash` — so the fixture speaks the namespace the shell running
/// it is standing in.
fn msys_spelling(path: &Path) -> String {
    let windows = path.to_string_lossy().replace('\\', "/");
    match windows.split_once(":/") {
        Some((drive, tail)) if drive.len() == 1 => {
            format!("/{}/{tail}", drive.to_ascii_lowercase())
        }
        _ => windows,
    }
}

/// Run the product's question against a distribution whose password database
/// says `login_shell`, and give back everything it printed.
///
/// `stdin` is closed, so a branch that really does exec a login shell ends at
/// once rather than waiting to be typed into.
fn ask(directory: &Path, login_shell: &str, init_file: &str) -> String {
    ask_with(directory, login_shell, init_file, ZDOTDIR)
}

/// The directory zsh's branch is handed, in the spelling a distribution reads.
const ZDOTDIR: &str = "/mnt/c/Users/dev/AppData/Roaming/Folio/shell-integration/zdotdir";

/// The same, with both doors named — the shape the launcher really passes.
fn ask_with(directory: &Path, login_shell: &str, init_file: &str, zdotdir: &str) -> String {
    let mut path = OsString::from(directory);
    path.push(";");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let output = Command::new(git_sh())
        .arg("-c")
        .arg(question())
        .arg("folio")
        .arg(init_file)
        .arg(zdotdir)
        .current_dir(directory)
        .env("PATH", path)
        .env("FOLIO_TEST_LOGIN_SHELL", login_shell)
        // The one variable the bash branch is supposed to introduce. Cleared
        // here so that a branch which merely inherited it could not pass.
        .env_remove("BT_SHELL_INTEGRATION")
        // The two zsh's branch introduces, cleared for the same reason.
        .env_remove("ZDOTDIR")
        .env_remove("BT_USER_ZDOTDIR")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("the POSIX shell beside git.exe runs");
    assert!(
        output.status.success(),
        "the question exited {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A `getent` that answers out of `FOLIO_TEST_LOGIN_SHELL`, in the seven-field
/// shape `passwd` has — so that the product's `cut -d: -f7` is reading a real
/// password entry rather than a bare word this test handed it.
fn write_distribution(directory: &Path) {
    write_program(
        directory,
        "getent",
        "printf 'dev:x:1000:1000:Dev:/home/dev:%s\\n' \"$FOLIO_TEST_LOGIN_SHELL\"",
    );
}

/// RED — **a distribution that logs into bash is handed the init file, and one
/// that logs into anything else keeps its own shell.**
///
/// The whole of the 2026-09-07 fix, exercised as the distribution sees it. Both
/// branches fail silently in production: without the first, every WSL pane goes
/// back to no `OSC 133` and no `OSC 7`; without the second, a zsh reader's shell
/// is replaced by bash on every tab and the symptom — "my prompt is gone" —
/// names neither this terminal nor the flag that did it.
///
/// MUTATIONS: drop the `bash)` arm and the first case prints `argv=-l`; drop the
/// `export` and it prints `BT_SHELL_INTEGRATION=<unset>`; match on `$shell`
/// rather than on `${shell##*/}` and `/usr/bin/bash` falls through to the
/// default arm, which is every distribution that keeps bash outside `/bin`.
#[test]
fn bash_is_handed_the_init_file_and_every_other_login_shell_is_left_alone() {
    let directory = temporary_directory();
    write_distribution(&directory);
    let init = "/mnt/c/Users/dev/AppData/Roaming/Folio/shell-integration/folio.bash";

    // **bash, wherever the distribution keeps it.** A password entry always
    // names an absolute path, and which absolute path is the distribution's
    // business: `/bin/bash` on Ubuntu, `/usr/bin/bash` on Fedora, `/bin/bash`
    // that is a symlink into `/usr` on an Arch. The arm therefore matches the
    // name the shell is invoked under, not the directory it lives in, and these
    // two spellings are that difference stood in for.
    for home in ["bin", "opt/somebodys-bash"] {
        let shell = shell_fixture(&directory, home, "bash");
        let said = ask(&directory, &shell, init);
        assert!(said.contains("shell=bash"), "{home}: {said}");
        assert!(
            said.contains(&format!("argv=--init-file {init} -i")),
            "{home}: the init file is bash's own flag and its own argument: {said}"
        );
        assert!(
            said.contains("BT_SHELL_INTEGRATION=login"),
            "{home}: the script is told which chain it owes, and `wsl.exe` starts a login shell:              {said}"
        );
    }

    // **zsh takes the directory** (review row R3-6). It has no `--init-file` and
    // refuses the flag, so what it is handed is `ZDOTDIR` — and it is still the
    // login shell `wsl.exe` would have started.
    let shell = shell_fixture(&directory, "bin", "zsh");
    let said = ask(&directory, &shell, init);
    assert!(said.contains("shell=zsh"), "{said}");
    assert!(said.contains("argv=-l"), "zsh keeps its own login: {said}");
    assert!(
        !said.contains("--init-file"),
        "handing bash's flag to a zsh is handing it one it refuses: {said}"
    );
    assert!(said.contains(&format!("ZDOTDIR={ZDOTDIR}")), "{said}");
    assert!(
        said.contains("BT_SHELL_INTEGRATION=<unset>"),
        "zsh never reads bash's script, so it must not inherit bash's marker: {said}"
    );
    assert!(
        said.contains("BT_USER_ZDOTDIR=<unset>"),
        "a session with no directory of its own says so by absence: {said}"
    );

    // Anything else keeps its shell and is started as the login shell `wsl.exe`
    // would have started — and is *not* told that somebody ran its startup
    // files, because nobody did.
    for name in ["fish", "elvish"] {
        let shell = shell_fixture(&directory, "bin", name);
        let said = ask(&directory, &shell, init);
        assert!(said.contains(&format!("shell={name}")), "{said}");
        assert!(said.contains("argv=-l"), "{name}: {said}");
        assert!(
            !said.contains("--init-file"),
            "{name}: handing bash's flag to a shell that is not bash replaces it: {said}"
        );
        assert!(
            said.contains("BT_SHELL_INTEGRATION=<unset>"),
            "{name}: a shell that never reads the script must not inherit the marker, or every \
             nested bash is told its startup files have already been run: {said}"
        );
        assert!(
            said.contains("ZDOTDIR=<unset>"),
            "{name}: and neither door is opened for a shell that reads neither: {said}"
        );
    }

    // A path this machine keeps somewhere the distribution cannot name travels
    // as the empty string, and the branch that needed it falls through to the
    // plain login shell rather than to a flag with nothing behind it.
    let shell = shell_fixture(&directory, "bin", "zsh");
    let said = ask_with(&directory, &shell, init, "");
    assert!(said.contains("argv=-l"), "{said}");
    assert!(said.contains("ZDOTDIR=<unset>"), "{said}");
    let shell = shell_fixture(&directory, "bin", "bash");
    let said = ask_with(&directory, &shell, "", ZDOTDIR);
    assert!(said.contains("argv=-l"), "{said}");
    assert!(!said.contains("--init-file"), "{said}");

    std::fs::remove_dir_all(&directory).ok();
}

/// PIN — a distribution that cannot say gets a shell rather than an error.
///
/// A `getent` that is not there, or a user whose account is not in the local
/// database: the answer is empty, and `exec ""` would close the pane the instant
/// it opened with a message about the empty string. `/bin/sh` is the one shell a
/// POSIX system is required to have.
#[test]
fn a_distribution_that_will_not_say_still_gets_a_shell() {
    let directory = temporary_directory();
    // Present but silent, which is the shape both failures take by the time the
    // script sees them.
    write_program(&directory, "getent", "exit 2");
    let _unreachable = shell_fixture(&directory, "bin", "bash");
    let said = ask(
        &directory,
        "ignored — getent answers nothing",
        "/mnt/c/folio.bash",
    );
    assert!(
        !said.contains("shell=bash"),
        "an unanswered database is not assumed to log into bash: {said}"
    );
    assert!(!said.contains("--init-file"), "{said}");
    std::fs::remove_dir_all(&directory).ok();
}

/// PIN — the question this test runs is the question the product asks.
///
/// The anti-drift gate `shell_integration_cmd.rs` has, for its reason: the copy
/// above is a copy, and a rewrite of the constant that this file did not follow
/// would leave it exercising a script nothing ships.
#[test]
fn the_question_this_test_runs_is_the_question_the_product_asks() {
    let source = shell_integration_source();
    for fragment in QUESTION {
        assert!(
            source.contains(fragment),
            "crates/bt-app/src/shell_integration.rs no longer spells {fragment:?} — the copy in \
             this file is the thing under test and has to follow it"
        );
    }
    // And it is one argument, so it may not grow a newline.
    assert!(!question().contains('\n'));
}
