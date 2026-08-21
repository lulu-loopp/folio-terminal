use std::{env, ffi::OsString, path::Path, process::Command};

fn main() {
    let manifest_dir = required_env("CARGO_MANIFEST_DIR");
    let workspace = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("bt-pty must remain under WORKSPACE/crates/bt-pty");
    let package = workspace
        .join("vendor")
        .join("conpty")
        .join("Microsoft.Windows.Console.ConPTY.1.25.260710002-preview.nupkg");
    let extractor = manifest_dir.join("extract-conpty-sidecar.ps1");

    println!("cargo:rerun-if-changed={}", package.display());
    println!("cargo:rerun-if-changed={}", extractor.display());
    println!("cargo:rerun-if-env-changed=SystemRoot");
    println!("cargo:rerun-if-env-changed=ProgramFiles");

    if env::var_os("CARGO_CFG_TARGET_OS").as_deref() != Some("windows".as_ref()) {
        return;
    }

    let out_dir = required_env("OUT_DIR");
    let profile_dir = out_dir
        .ancestors()
        .nth(3)
        .expect("OUT_DIR must be PROFILE/build/bt-pty-HASH/out");
    let test_dir = profile_dir.join("deps");
    let system_root = required_env("SystemRoot");
    let windows_powershell = system_root
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0");
    let powershell = windows_powershell.join("powershell.exe");

    let output = Command::new(&powershell)
        .env(
            "PSModulePath",
            windows_powershell_module_path(&required_env("ProgramFiles"), &windows_powershell),
        )
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&extractor)
        .arg("-Package")
        .arg(&package)
        .arg("-Destination")
        .arg(profile_dir)
        .arg("-TestDestination")
        .arg(&test_dir)
        .output()
        .unwrap_or_else(|error| panic!("launch {}: {error}", powershell.display()));
    if !output.status.success() {
        panic!(
            "ConPTY sidecar extraction failed ({}):\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Windows PowerShell 5.1's own two module directories, **named rather than inherited**.
///
/// The extractor above is a 5.1 script, and 5.1 finds `Get-FileHash` by autoloading
/// `Microsoft.PowerShell.Utility` off `PSModulePath`. Inheriting that variable made the build a
/// property of whatever shell happened to invoke `cargo`: this repository is normally driven from
/// PowerShell 7, whose `PSModulePath` puts 7's module directories first, and 5.1 walking that list
/// reaches 7's `Microsoft.PowerShell.Utility` — a `Core`-only manifest it will not load — before
/// its own, so `Get-FileHash` came back "not recognized" and the ConPTY sidecar never extracted.
/// Reproduced 2/2 against 2/2 on 2026-08-20; that failure is the entire reason the repository's
/// test discipline carried an `export PSModulePath=...` line, and declaring the value here retires
/// it. See `docs/DESIGN.md` and the `BT_PSREADLINE_MODULE_PATH` note in `src/lib.rs`.
///
/// Derived from the machine's own `%ProgramFiles%` and `%SystemRoot%` rather than written out as
/// the `C:\Program Files\...` and `C:\WINDOWS\...` that the retired discipline line spelled: the
/// literal is what that line got wrong in principle even where it happened to be right in fact.
fn windows_powershell_module_path(program_files: &Path, windows_powershell: &Path) -> OsString {
    let mut value = OsString::from(program_files.join("WindowsPowerShell").join("Modules"));
    value.push(";");
    value.push(windows_powershell.join("Modules"));
    value
}

fn required_env(name: &str) -> std::path::PathBuf {
    env::var_os(name)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| panic!("Cargo did not set {name}"))
}
