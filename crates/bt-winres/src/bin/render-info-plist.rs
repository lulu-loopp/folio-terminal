//! **`Info.plist` on stdout**, for the shell script that assembles `Folio.app`.
//!
//! ```text
//! cargo run -q -p bt-winres --bin render-info-plist > Folio.app/Contents/Info.plist
//! ```
//!
//! The version is this crate's `CARGO_PKG_VERSION`, which is
//! `[workspace.package] version` — every crate here inherits that one line, so
//! the bundle gets its version from the same place `folio --version` does
//! without the script having to read, parse or repeat it. A script that grepped
//! `Cargo.toml` itself would be a second reader of that line and therefore a
//! second thing to go wrong at a release.
//!
//! `bt-winres` has no dependencies, so this costs the bundle machine one small
//! compilation and no network.
//!
//! The template is `packaging/macos/Info.plist.in` in this checkout unless a
//! path is given as the single argument. Anything it cannot honour — a version
//! `CFBundleVersion` will not take, a placeholder nothing fills — goes to
//! stderr and the exit code, so `set -e` stops the bundle rather than signing an
//! `Info.plist` with `@VERSION@` still in it.

use std::process::ExitCode;

/// The template this repository ships, found relative to the crate rather than
/// to whatever directory the script happened to be run from.
const TEMPLATE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packaging/macos/Info.plist.in"
);

fn main() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let path = arguments.next().map_or_else(
        || std::path::PathBuf::from(TEMPLATE),
        std::path::PathBuf::from,
    );
    if arguments.next().is_some() {
        eprintln!("usage: render-info-plist [path to Info.plist.in]");
        return ExitCode::FAILURE;
    }

    let template = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("read {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    };

    match bt_winres::plist::render(&template, env!("CARGO_PKG_VERSION")) {
        Ok(plist) => {
            print!("{plist}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}: {error}", path.display());
            ExitCode::FAILURE
        }
    }
}
