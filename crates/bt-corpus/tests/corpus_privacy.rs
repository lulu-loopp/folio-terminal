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
    // Spelled in two halves so the source file itself never carries the word the gate bans
    // (`scripts/check-machine-paths.ps1` reads this file too).
    concat!("wei", "yi"),
    concat!("um", "ich"),
    "c:\\users\\",
    // The recording machine's folders at a drive root are not substrings: see `DRIVE_ROOT_FOLDERS`.
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

/// The recording machine's own folders, each refused where it stands at a drive root: what the
/// folder is, and its path below the drive, lower-case, one segment per entry (spelled in halves,
/// as above). The development checkout's parent, the synced-documents folder, and the
/// software-install folder that conda's and Git's directories on a recorded `PATH` sit in.
///
/// A fixed substring cannot say "at a drive root": until 2026-11-02 (0.4.9 row 183) the
/// checkout needle named one drive only and the install needle wanted a backslash after the
/// name, so another drive, a forward slash, the folder last in a path and the Unix-style
/// spellings of a drive all passed. `folder_at_drive_root` asks what
/// `scripts/check-machine-paths.ps1` asks: any drive, in every spelling a tool prints one.
const DRIVE_ROOT_FOLDERS: &[(&str, &[&str])] = &[
    ("the development checkout's parent folder", &["developer"]),
    (
        "the synced-documents folder",
        &["documents", concat!("sync", "folder")],
    ),
    ("the software-install folder", &[concat!("a", "pp")]),
];

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn is_separator(byte: u8) -> bool {
    byte == b'\\' || byte == b'/'
}

/// Whether a path segment starting at `at` stands at a drive root: one or more separators, and in
/// front of them a drive, in any of the spellings a tool prints:
/// - `x:`, a letter standing alone (no letter or digit before it, so a URL scheme's colon is not
///   a drive) — with any run of separators, so `file:///x:/` and `\\?\x:\` count too;
/// - WSL's `/mnt/x` and Cygwin's `/cygdrive/x`;
/// - Git Bash's `/x`, where it is not inside a path or a URL (no word character, `/`, `\`, `.`,
///   `~` or `-` before it); after `:`, `=`, `;`, a space or a quote it counts, which is how a Git
///   Bash `PATH` lists it.
///
/// The view is the recording's bytes, so a line break before a drive is a real control byte, not
/// a source escape; it stands alone without the scan's escape rule.
fn at_drive_root(view: &[u8], at: usize) -> bool {
    let mut cursor = at;
    while cursor > 0 && is_separator(view[cursor - 1]) {
        cursor -= 1;
    }
    if cursor == at || cursor == 0 {
        return false;
    }
    let letter_at = cursor - 1;
    if view[letter_at] == b':' {
        return letter_at >= 1
            && view[letter_at - 1].is_ascii_alphabetic()
            && (letter_at == 1 || !view[letter_at - 2].is_ascii_alphanumeric());
    }
    if !view[letter_at].is_ascii_alphabetic() || letter_at == 0 || view[letter_at - 1] != b'/' {
        return false;
    }
    let slash = letter_at - 1;
    let unix_mount =
        view[..letter_at].ends_with(b"/mnt/") || view[..letter_at].ends_with(b"/cygdrive/");
    let git_bash = at - cursor == 1
        && view[at - 1] == b'/'
        && (slash == 0
            || !(is_word(view[slash - 1])
                || matches!(view[slash - 1], b'/' | b'\\' | b'.' | b'~' | b'-')));
    unix_mount || git_bash
}

/// Whether the lower-case `view` holds the folder whose path below the drive is `segments`: the
/// segments in order, joined by runs of separators, the last one ending at a non-word byte or at
/// the end, and the first one at a drive root.
fn folder_at_drive_root(view: &[u8], segments: &[&str]) -> bool {
    let first = segments[0].as_bytes();
    (0..view.len()).any(|at| {
        if !view[at..].starts_with(first) || !at_drive_root(view, at) {
            return false;
        }
        let mut cursor = at + first.len();
        for segment in &segments[1..] {
            let run = view[cursor..]
                .iter()
                .take_while(|byte| is_separator(**byte))
                .count();
            if run == 0 || !view[cursor + run..].starts_with(segment.as_bytes()) {
                return false;
            }
            cursor += run + segment.len();
        }
        view.get(cursor).is_none_or(|byte| !is_word(*byte))
    })
}

/// The one mail domain a scrubbed recording is allowed to name.
const PLACEHOLDER_MAIL_DOMAIN: &str = "example.com";

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests")
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
            for (folder, segments) in DRIVE_ROOT_FOLDERS {
                if folder_at_drive_root(&view, segments) {
                    let offence = format!("{name} names {folder} at a drive root");
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
    // The drive-root folders are asked of the same two views.
    assert!(!folder_at_drive_root(&lowercased(&wrapped), &["developer"]));
    assert!(folder_at_drive_root(
        &lowercased(&unwrapped(&wrapped)),
        &["developer"]
    ));
    // An ordinary line break is data, not a wrap, and is left alone.
    let plain = b"first\r\nsecond".to_vec();
    assert_eq!(unwrapped(&plain), plain);
}

#[test]
fn a_folder_at_a_drive_root_is_found_in_every_spelling_a_tool_prints() {
    // A stand-in folder, so that this file names none of the machine's: the grammar is the same
    // for every entry of `DRIVE_ROOT_FOLDERS`.
    let found = |text: &str| folder_at_drive_root(&lowercased(text.as_bytes()), &["kit"]);
    for spelling in [
        "d:\\kit\\conda",
        "x:/kit/git",
        "file:///d:/kit/git",
        "\\\\?\\d:\\kit\\git",
        "d:\\\\kit\\\\git",
        "d:\\\\\\\\kit",
        "path d:\\kit;c:\\tools",
        "\"d:\\kit\"",
        "\r\nd:\\kit",
        "/mnt/d/kit/git",
        "/cygdrive/d/kit",
        "PATH=/c/x:/d/kit/git/bin:/usr/bin",
        "cd /d/kit",
        "D:\\Kit-old",
    ] {
        assert!(found(spelling), "{spelling:?} is a folder at a drive root");
    }
    for spelling in [
        "webpack://kit/src",
        "webpack:/kit/src",
        "node:\\kit",
        "ad:\\kit",
        "9d:\\kit",
        "https://host/a/kit",
        "http://a/kit",
        "./x/kit",
        "~/x/kit",
        "$U/kit",
        "d:\\kits",
        "d:\\kit_old",
        "d:\\tools\\kit",
        "\":\\kit\\\"",
    ] {
        assert!(
            !found(spelling),
            "{spelling:?} is not a folder at a drive root"
        );
    }
    // A folder two segments below the drive is matched segment by segment.
    let nested = |text: &str| folder_at_drive_root(&lowercased(text.as_bytes()), &["docs", "kit"]);
    assert!(nested("d:\\Docs\\Kit\\notes"));
    assert!(nested("/d/docs//kit"));
    assert!(!nested("d:\\docs\\kitchen"));
    assert!(!nested("d:\\tools\\docs\\kit"));
}
