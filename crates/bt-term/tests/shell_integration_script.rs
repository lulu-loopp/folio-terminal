//! The shell integration script is the other half of OSC 7: nothing this terminal does with a
//! working directory matters if the shell never names one. These pins run
//! `scripts/shell-integration/folio.ps1` in a real Windows PowerShell 5.1 — the older of
//! the two supported generations, and the one whose language limits the script is written to — and
//! feed exactly what it puts on the wire back into a session.

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use bt_term::DualPlaneSession;

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
/// the bytes its `prompt` writes.
///
/// The user's own prompt is stubbed to a plain ASCII string first, exactly as a real profile would
/// have defined one before dot-sourcing: the script wraps whatever prompt it finds, and stubbing it
/// keeps the *prompt's* text out of the bytes under test without touching the markers around it.
fn prompt_bytes(directory: &Path) -> Vec<u8> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let driver = std::env::temp_dir().join(format!(
        "betterterminal-osc7-driver-{}-{unique}.ps1",
        std::process::id()
    ));
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
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
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

/// A temporary directory whose name carries a space and CJK, so the encoder is exercised on both
/// the byte that must become `%20` and the multi-byte characters that must become their UTF-8
/// escapes rather than anything the console codepage would produce.
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
