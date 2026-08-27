//! The checked-in recordings are shipped in a public repository, so none of
//! them may carry the identity of the person who recorded it.
//!
//! Scanning the raw file is not enough. ConPTY bakes its line wrapping into the
//! stream: at the wrap it emits CR LF, a CUP back to the last column, and a
//! repeat of the character already standing there, which splits a name or a
//! path across two fragments that no literal search would find. The gate
//! therefore reconstructs the logical character stream the terminal renders and
//! scans that as well.

use std::{
    fs,
    path::{Path, PathBuf},
};

use bt_corpus::{Corpus, EventKind};

/// Substrings that must never appear in a recording, matched case-insensitively.
/// Personal names, the recording machine's home and project roots, account
/// banners, and the shapes credentials take.
const FORBIDDEN: &[&str] = &[
    "weiyi",
    "umich",
    "c:\\users\\",
    "d:\\developer",
    "welcome back",
    "organization",
    "laptop-",
    "desktop-",
    "sk-ant-",
    "sk-proj-",
    "ghp_",
    "github_pat_",
    "-----begin ",
];

/// The one mail domain a scrubbed recording is allowed to name.
const PLACEHOLDER_MAIL_DOMAIN: &str = "example.com";

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("corpus")
}

fn recordings() -> Vec<PathBuf> {
    let mut paths = fs::read_dir(corpus_dir())
        .expect("corpus directory")
        .map(|entry| entry.expect("corpus entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "btcr")
        })
        .collect::<Vec<_>>();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "no recordings found in {:?}",
        corpus_dir()
    );
    paths
}

/// Every output byte of the recording, in replay order.
fn output_bytes(path: &Path) -> Vec<u8> {
    let corpus = Corpus::read_from(fs::File::open(path).expect("open recording"))
        .expect("recording parses as a corpus");
    let mut bytes = Vec::new();
    for event in &corpus.events {
        if let EventKind::Output(chunk) = &event.kind {
            bytes.extend_from_slice(chunk);
        }
    }
    bytes
}

/// Length of the ConPTY wrap marker at `at`: CR LF, CUP with a row and a
/// column, and the repaint of the character already at that cell. Zero when the
/// stream does not wrap here.
fn wrap_marker(bytes: &[u8], at: usize) -> usize {
    let rest = &bytes[at..];
    if !rest.starts_with(b"\r\n\x1b[") {
        return 0;
    }
    let mut cursor = 4;
    let digits = |cursor: &mut usize| {
        let start = *cursor;
        while rest.get(*cursor).is_some_and(u8::is_ascii_digit) {
            *cursor += 1;
        }
        *cursor > start
    };
    if !digits(&mut cursor) || rest.get(cursor) != Some(&b';') {
        return 0;
    }
    cursor += 1;
    if !digits(&mut cursor) || rest.get(cursor) != Some(&b'H') {
        return 0;
    }
    cursor += 1;
    // The repainted character must match the one the wrap left behind.
    match (
        at.checked_sub(1).map(|prior| bytes[prior]),
        rest.get(cursor),
    ) {
        (Some(before), Some(after)) if before == *after => cursor + 1,
        _ => 0,
    }
}

/// The character stream the terminal renders, with ConPTY's wrapping undone.
fn unwrapped(bytes: &[u8]) -> Vec<u8> {
    let mut logical = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        let marker = wrap_marker(bytes, cursor);
        if marker > 0 {
            cursor += marker;
            continue;
        }
        logical.push(bytes[cursor]);
        cursor += 1;
    }
    logical
}

fn lowercased(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().map(u8::to_ascii_lowercase).collect()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.len() >= needle.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn is_mail_local(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn is_mail_domain(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')
}

/// Mail domains named by the recording, other than the sanctioned placeholder.
fn foreign_mail_domains(bytes: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    for (at, byte) in bytes.iter().enumerate() {
        if *byte != b'@' || at == 0 || !is_mail_local(bytes[at - 1]) {
            continue;
        }
        let mut end = at + 1;
        while bytes.get(end).is_some_and(|byte| is_mail_domain(*byte)) {
            end += 1;
        }
        let domain = String::from_utf8_lossy(&bytes[at + 1..end])
            .trim_end_matches('.')
            .to_ascii_lowercase();
        if domain.contains('.') && domain != PLACEHOLDER_MAIL_DOMAIN && !found.contains(&domain) {
            found.push(domain);
        }
    }
    found
}

#[test]
fn no_recording_carries_a_person() {
    let mut offences = Vec::new();
    for path in recordings() {
        let name = path
            .file_name()
            .expect("file name")
            .to_string_lossy()
            .to_string();
        let raw = fs::read(&path).expect("read recording");
        let logical = unwrapped(&output_bytes(&path));
        for view in [lowercased(&raw), lowercased(&logical)] {
            for needle in FORBIDDEN {
                if contains(&view, needle.as_bytes()) {
                    let offence = format!("{name} names `{needle}`");
                    if !offences.contains(&offence) {
                        offences.push(offence);
                    }
                }
            }
            for domain in foreign_mail_domains(&view) {
                let offence = format!("{name} names the mail domain `{domain}`");
                if !offences.contains(&offence) {
                    offences.push(offence);
                }
            }
        }
    }
    assert!(
        offences.is_empty(),
        "recordings carry identity that must be scrubbed:\n  {}",
        offences.join("\n  ")
    );
}

#[test]
fn the_gate_reads_through_conpty_line_wrapping() {
    // A forbidden phrase split by a wrap marker survives a literal search of the
    // raw bytes and must not survive this gate.
    let wrapped = b"set DIR=D:\\Devel\r\n\x1b[19;120Hloper\\project".to_vec();
    assert!(!contains(&lowercased(&wrapped), b"d:\\developer"));
    assert!(contains(
        &lowercased(&unwrapped(&wrapped)),
        b"d:\\developer"
    ));
    // An ordinary line break is data, not a wrap, and is left alone.
    let plain = b"first\r\nsecond".to_vec();
    assert_eq!(unwrapped(&plain), plain);
}
