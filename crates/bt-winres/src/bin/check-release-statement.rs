//! **A release, checked against its signed statement with the keys this build embeds**
//! (0.4.10 plan row 102, ruling R10): the check the release script runs over the package
//! directory before the upload, and the release workflow over the assets it downloads from the
//! published page.
//!
//! ```text
//! cargo run -q -p bt-winres --features release-key,inflate --bin check-release-statement -- verify <folder> <tag>
//! cargo run -q -p bt-winres --features release-key,inflate --bin check-release-statement -- fixture <live|spare> <out file>
//! check-release-statement compose <folder> <tag> <date> <min_from> <true|false> <live|spare> <platform>=<file>... [retire=<key id>...]
//! ```
//!
//! `compose` writes `<folder>/RELEASE-STATEMENT.txt` for the release whose files are in
//! `<folder>` (part 2, `scripts/release/release-statement.ps1`): each `<platform>=<file>` read
//! for its SHA-256 and size, the members of the `windows-x64` archive, and the facts the
//! arguments give, through `compose_statement` — so the statement is this crate's grammar,
//! written by this crate. The named key is the one that will sign it; nothing here signs.
//!
//! `verify` reads `RELEASE-STATEMENT.txt`, its `.sig` and every asset the statement lists from
//! `<folder>`, and refuses — with the refusal's name on stderr and a failing exit code — a
//! signature that does not verify with the embedded key the statement names, a statement for
//! another tag, an asset whose size or SHA-256 differs, or a Windows archive whose members are
//! not the statement's. A build that embeds a test key refuses to vouch for anything
//! (`release_check`, `test-key`).
//!
//! `fixture` writes the fixed statement the tests hold the embedded key of `<live|spare>` to
//! (`bt_winres::release_statement::fixture_statement`), byte for byte, for whoever holds that
//! key to sign with OpenSSL when the keys change. It writes the file itself because a shell's
//! redirection may re-encode it.

#![allow(clippy::disallowed_methods)]

use std::path::Path;
use std::process::ExitCode;

use bt_winres::release_statement::{
    KeyId, KeySet, RELEASE_KEYS, STATEMENT_FILE, StatementInputs, compose_statement,
    fixture_statement, release_check,
};

const USAGE: &str = "usage: check-release-statement verify <folder> <tag> | fixture <live|spare> <out file> | compose <folder> <tag> <date> <min_from> <true|false> <live|spare> <platform>=<file>... [retire=<key id>...]";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match arguments.as_slice() {
        ["verify", folder, tag] => verify(Path::new(folder), tag),
        ["fixture", slot, out] => fixture(slot, Path::new(out)),
        [
            "compose",
            folder,
            tag,
            date,
            min_from,
            authenticode,
            slot,
            listed @ ..,
        ] => compose(
            Path::new(folder),
            [tag, date, min_from, authenticode, slot],
            listed,
        ),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn verify(folder: &Path, tag: &str) -> ExitCode {
    let mut read =
        |name: &str| std::fs::read(folder.join(name)).map_err(|error| format!("{name}: {error}"));
    match release_check(&mut read, tag, &KeySet::EMBEDDED) {
        Ok(verified) => {
            let statement = verified.what_it_says();
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

/// **`compose`**: the statement of the release in `folder`, written beside its files.
fn compose(
    folder: &Path,
    [tag, date, min_from, authenticode, slot]: [&str; 5],
    listed: &[&str],
) -> ExitCode {
    let Some(key) = RELEASE_KEYS.iter().find(|key| key.slot == slot) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let authenticode = match authenticode {
        "true" => true,
        "false" => false,
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    let mut assets = Vec::new();
    let mut retires = Vec::new();
    for entry in listed {
        match entry.split_once('=') {
            Some(("retire", id)) => match KeyId::parse_key_id(id) {
                Some(id) => retires.push(id),
                None => {
                    eprintln!("refused: {id} is not a key id (sixteen lower-case hex digits)");
                    return ExitCode::FAILURE;
                }
            },
            Some((platform, file)) => assets.push((platform, file)),
            None => {
                eprintln!("{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }
    let inputs = StatementInputs {
        tag,
        date,
        min_from,
        authenticode,
        key,
        retires,
        assets,
    };
    let mut read =
        |name: &str| std::fs::read(folder.join(name)).map_err(|error| format!("{name}: {error}"));
    let text = match compose_statement(&mut read, &inputs) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("refused: {error}");
            return ExitCode::FAILURE;
        }
    };
    let out = folder.join(STATEMENT_FILE);
    match std::fs::write(&out, &text) {
        Ok(()) => {
            println!(
                "wrote {} for {tag}, to be signed by the {slot} key {}",
                out.display(),
                key.key_id()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("write {}: {error}", out.display());
            ExitCode::FAILURE
        }
    }
}
