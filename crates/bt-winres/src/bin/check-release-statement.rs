//! **A release, checked against its signed statement with the keys this build embeds**
//! (0.4.10 plan row 102, ruling R10): the check the release script runs over the package
//! directory before the upload, and the release workflow over the assets it downloads from the
//! published page.
//!
//! ```text
//! cargo run -q -p bt-winres --features release-key,inflate --bin check-release-statement -- verify <folder> <tag>
//! cargo run -q -p bt-winres --features release-key,inflate --bin check-release-statement -- fixture <live|spare> <out file>
//! ```
//!
//! `verify` reads `RELEASE-STATEMENT.txt`, its `.sig` and every asset the statement lists from
//! `<folder>`, and refuses — with the refusal's name on stderr and a failing exit code — a
//! signature that does not verify with the embedded key the statement names, a statement for
//! another tag, an asset whose size or SHA-256 differs, or a Windows archive whose members are
//! not the statement's. A build that embeds a test key refuses to vouch for anything.
//!
//! `fixture` writes the fixed statement the tests hold the embedded key of `<live|spare>` to
//! (`bt_winres::release_statement::fixture_statement`), byte for byte, for whoever holds that
//! key to sign with OpenSSL when the keys change. It writes the file itself because a shell's
//! redirection may re-encode it.

#![allow(clippy::disallowed_methods)]

use std::path::Path;
use std::process::ExitCode;

use bt_winres::release_statement::{
    KeySet, RELEASE_KEYS, check_published, embeds_test_keys, fixture_statement,
};

const USAGE: &str =
    "usage: check-release-statement verify <folder> <tag> | fixture <live|spare> <out file>";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match arguments.as_slice() {
        ["verify", folder, tag] => verify(Path::new(folder), tag),
        ["fixture", slot, out] => fixture(slot, Path::new(out)),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn verify(folder: &Path, tag: &str) -> ExitCode {
    if embeds_test_keys() {
        eprintln!("refused: this build embeds a test key, and no release may be checked by it");
        return ExitCode::FAILURE;
    }
    let mut read =
        |name: &str| std::fs::read(folder.join(name)).map_err(|error| format!("{name}: {error}"));
    match check_published(&mut read, tag, &KeySet::EMBEDDED) {
        Ok(statement) => {
            let slot = RELEASE_KEYS
                .iter()
                .find(|key| key.key_id() == statement.key)
                .map_or("?", |key| key.slot);
            println!(
                "verified {tag}: signed by key {} ({slot}), {} assets, {} {} members, authenticode {}",
                statement.key,
                statement.assets.len(),
                statement.members.len(),
                statement.members_of,
                statement.authenticode
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("refused: {error}");
            ExitCode::FAILURE
        }
    }
}

fn fixture(slot: &str, out: &Path) -> ExitCode {
    let Some(key) = RELEASE_KEYS.iter().find(|key| key.slot == slot) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    match std::fs::write(out, fixture_statement(key)) {
        Ok(()) => {
            println!(
                "wrote the {slot} key's fixture statement to {}",
                out.display()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("write {}: {error}", out.display());
            ExitCode::FAILURE
        }
    }
}
