//! The shell integration script is the other half of OSC 7: nothing this terminal does with a
//! working directory matters if the shell never names one. These pins run
//! `scripts/shell-integration/folio.ps1` in a real Windows PowerShell 5.1 — the older of
//! the two supported generations, and the one whose language limits the script is written to — and
//! feed exactly what it puts on the wire back into a session.

#![allow(clippy::disallowed_methods)]

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use bt_term::DualPlaneSession;

static NEXT_TEMP_PATH: AtomicU64 = AtomicU64::new(0);

fn nz(value: u32) -> std::num::NonZeroU32 {
    std::num::NonZeroU32::new(value).unwrap()
}

fn script_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/shell-integration/folio.ps1")
        .canonicalize()
        .expect("the integration script ships in the repository")
}

/// Run the integration script in a child Windows PowerShell 5.1, `cd` to `directory`, and return
/// the bytes its `prompt` writes. The child declares the terminal it runs in exactly as a Folio
/// pane does (`TERM_PROGRAM` = [`bt_pty::TERM_PROGRAM`]) — the script acts only there — and
/// [`prompt_bytes_outside_folio`] runs it with no declaration at all.
///
/// The user's own prompt is stubbed to a plain ASCII string first, exactly as a real profile would
/// have defined one before dot-sourcing: the script wraps whatever prompt it finds, and stubbing it
/// keeps the *prompt's* text out of the bytes under test without touching the markers around it.
fn prompt_bytes(directory: &Path) -> Vec<u8> {
    prompt_bytes_declaring(directory, Some(bt_pty::TERM_PROGRAM))
}

/// [`prompt_bytes`] in a child that declares no terminal, as in any terminal but Folio. The test
/// shell has already removed every variable Folio announces to a pane, so the child sees no
/// `TERM_PROGRAM` even when this test runs from inside one.
fn prompt_bytes_outside_folio(directory: &Path) -> Vec<u8> {
    prompt_bytes_declaring(directory, None)
}

fn prompt_bytes_declaring(directory: &Path, terminal: Option<&str>) -> Vec<u8> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let driver = driver_path_at(unique);
    std::fs::write(
        &driver,
        format!(
            "function global:prompt {{ 'PS> ' }}\n\
             . '{}'\n\
             Set-Location -LiteralPath $args[0]\n\
             [Console]::Out.Write((prompt))\n",
            script_path().display()
        ),
    )
    .unwrap();
    // Through `bt_pty::test_shell::Hygiene`: `-NoProfile` and a temporary HOME/APPDATA.
    let hygiene = bt_pty::test_shell::Hygiene::new();
    let mut command = hygiene.command("powershell.exe", Command::new);
    if let Some(terminal) = terminal {
        command.env("TERM_PROGRAM", terminal);
    }
    let output = command
        .args(["-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&driver)
        .arg(directory)
        .output()
        .expect("Windows PowerShell 5.1 is present on every supported host");
    std::fs::remove_file(&driver).unwrap();
    assert!(
        output.status.success(),
        "the script must install cleanly: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn fixture_path_at(stem: &str, timestamp: u128, extension: &str) -> PathBuf {
    let ordinal = NEXT_TEMP_PATH.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "{stem}-{}-{timestamp}-{ordinal}{extension}",
        std::process::id(),
    ))
}

fn driver_path_at(timestamp: u128) -> PathBuf {
    fixture_path_at("betterterminal-osc7-driver", timestamp, ".ps1")
}

fn directory_path_at(timestamp: u128) -> PathBuf {
    fixture_path_at("betterterminal 图 片", timestamp, "")
}

#[test]
fn equal_clock_samples_get_distinct_fixture_paths() {
    let first_driver = driver_path_at(42);
    let second_driver = driver_path_at(42);
    assert_ne!(
        first_driver, second_driver,
        "wall-clock samples are not driver path identities"
    );

    let first_directory = directory_path_at(42);
    let second_directory = directory_path_at(42);
    assert_ne!(
        first_directory, second_directory,
        "wall-clock samples are not directory path identities"
    );
}

/// A temporary directory whose name carries a space and CJK, so the encoder is exercised on both
/// the byte that must become `%20` and the multi-byte characters that must become their UTF-8
/// escapes rather than anything the console codepage would produce.
fn temporary_directory() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = directory_path_at(unique);
    std::fs::create_dir(&directory).unwrap();
    // The shell names its location in long form; on a host whose %TEMP% is
    // spelled with an 8.3 short component (RUNNER~1), the path just joined is
    // not the path the shell will report. Canonicalize to the long spelling
    // and drop the verbatim prefix, which no file:// URI carries.
    let canonical = std::fs::canonicalize(&directory).unwrap();
    match canonical.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(plain) => PathBuf::from(plain),
        None => canonical,
    }
}

/// PIN (relative path ruling, 2026-08-03 (f)): the script emits one OSC 7 report per prompt, ahead
/// of the `133;A` that opens the prompt region, as a `file://` URI with an empty authority and a
/// minimally percent-encoded path — and a session fed those exact bytes ends up holding the exact
/// directory the shell was in. Round trip, not shape-matching: the encoder and the decoder are
/// pinned against each other, so neither can drift alone.
#[test]
fn the_integration_script_reports_its_working_directory_over_osc_7() {
    let directory = temporary_directory();
    let bytes = prompt_bytes(&directory);
    let text = String::from_utf8(bytes.clone()).expect("a URI and FTCS markers are ASCII");

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
        "a space is %20 and CJK is its UTF-8 escapes: {uri:?}"
    );
    assert!(
        !uri.contains(' ') && uri.is_ascii(),
        "a URI on the wire is ASCII with no literal space: {uri:?}"
    );
    assert!(
        report_end
            < text
                .find("\u{1b}]133;A\u{7}")
                .expect("the prompt markers are unchanged"),
        "the directory is reported before the prompt region it describes: {text:?}"
    );

    // The whole prompt burst, byte for byte, through the terminal that must understand it.
    let mut session = DualPlaneSession::new(nz(120), nz(8));
    session.feed(&bytes).unwrap();
    assert_eq!(
        session.working_directory(),
        Some(directory.as_path()),
        "the session holds the directory the shell was actually in"
    );

    std::fs::remove_dir(&directory).unwrap();
}

/// PIN (relative path ruling, 2026-08-03 (f)): a location with no filesystem directory behind it
/// retracts the previous report instead of leaving it to answer for a place the shell has left.
#[test]
fn a_non_filesystem_location_retracts_the_reported_working_directory() {
    let directory = temporary_directory();
    let mut session = DualPlaneSession::new(nz(120), nz(8));
    session.feed(&prompt_bytes(&directory)).unwrap();
    assert_eq!(session.working_directory(), Some(directory.as_path()));

    session.feed(&prompt_bytes(Path::new("HKLM:\\"))).unwrap();
    assert_eq!(
        session.working_directory(),
        None,
        "a registry location resolves no relative image path, and says so"
    );

    std::fs::remove_dir(&directory).unwrap();
}

/// PIN — **outside Folio the script does nothing** (T-INTEGRATION-INJECT-4: a `$PROFILE` line is
/// inert in every other terminal). With no `TERM_PROGRAM` declared, the prompt is the user's own,
/// byte for byte: no OSC 7, no OSC 133, no OSC 8 declaration — nothing but `PS> `.
///
/// RED (mutation: delete the `TERM_PROGRAM` guard at the top of `folio.ps1`).
#[test]
fn outside_folio_the_integration_script_emits_nothing() {
    let directory = temporary_directory();
    let bytes = prompt_bytes_outside_folio(&directory);
    assert_eq!(
        String::from_utf8_lossy(&bytes),
        "PS> ",
        "the user's prompt alone, with no sequence of this terminal's around it"
    );
    std::fs::remove_dir(&directory).unwrap();
}
