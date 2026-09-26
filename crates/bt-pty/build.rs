//! Writes the ConPTY sidecar (`conpty.dll`, `OpenConsole.exe`) out of the vendored NuGet package
//! into Cargo's profile directory and its `deps` directory, when the target is Windows.
//!
//! The target and not the host: the unpacking is Rust (`src/conpty_sidecar.rs`), so a cross build
//! of `x86_64-pc-windows-msvc` from Linux or macOS writes the same files a Windows machine does.
//!
//! And it says where it wrote them. `Cargo.toml`'s `links = "conpty"` lets each
//! `cargo:<key>=<path>` printed below reach `bt-app`'s build script as `DEP_CONPTY_<KEY>`, which
//! hashes the two files into the release manifest `folio.exe` carries (0.4.6 ticket U-9).

use std::{env, fs, path::Path, process};

#[path = "src/conpty_sidecar.rs"]
mod conpty_sidecar;

fn main() {
    let manifest_dir = required_env("CARGO_MANIFEST_DIR");
    let workspace = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("bt-pty must remain under WORKSPACE/crates/bt-pty");
    let package = workspace
        .join("vendor")
        .join("conpty")
        .join(conpty_sidecar::PACKAGE);

    // The package is what this script reads while it runs. The unpacking code is compiled into the
    // script, so Cargo already rebuilds and reruns it when that changes.
    println!("cargo:rerun-if-changed={}", package.display());

    if env::var_os("CARGO_CFG_TARGET_OS").as_deref() != Some("windows".as_ref()) {
        return;
    }

    let out_dir = required_env("OUT_DIR");
    let profile_dir = out_dir
        .ancestors()
        .nth(3)
        .expect("OUT_DIR must be PROFILE/build/bt-pty-HASH/out");
    let test_dir = profile_dir.join("deps");

    let bytes =
        fs::read(&package).unwrap_or_else(|error| panic!("read {}: {error}", package.display()));
    let sidecar = conpty_sidecar::unpack(&bytes)
        .unwrap_or_else(|error| panic!("ConPTY sidecar extraction failed: {error}"));
    for (file, contents) in &sidecar {
        for directory in [profile_dir, test_dir.as_path()] {
            for relative in file.targets {
                write_unless_already_there(&directory.join(relative), contents, file.sha256);
            }
        }
    }
    for (key, path) in conpty_sidecar::exported(profile_dir) {
        println!("cargo:{key}={}", path.display());
    }
}

/// Writes `contents` to `target` through a temporary file beside it and a rename, so a reader never
/// sees half a file, and leaves a `target` that already hashes to `sha256` untouched: an
/// `OpenConsole.exe` some test still has running cannot be replaced, and need not be.
fn write_unless_already_there(target: &Path, contents: &[u8], sha256: &str) {
    if let Ok(existing) = fs::read(target)
        && conpty_sidecar::hex(&conpty_sidecar::sha256(&existing)) == sha256
    {
        return;
    }
    let directory = target
        .parent()
        .expect("every sidecar target is below a destination directory");
    fs::create_dir_all(directory)
        .unwrap_or_else(|error| panic!("create {}: {error}", directory.display()));
    let mut temporary = target.as_os_str().to_owned();
    temporary.push(format!(".bt-extract-{}.tmp", process::id()));
    let temporary = Path::new(&temporary);
    let written = fs::write(temporary, contents).and_then(|()| fs::rename(temporary, target));
    if let Err(error) = written {
        let _ = fs::remove_file(temporary);
        panic!("write {}: {error}", target.display());
    }
}

fn required_env(name: &str) -> std::path::PathBuf {
    env::var_os(name)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| panic!("Cargo did not set {name}"))
}
