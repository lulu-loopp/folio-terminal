//! Printed file paths in terminal text: where one begins, where it ends, and what file it names.
//!
//! This is the sibling of [`crate::detect_http_urls`] and it lives beside it for the same reason: a
//! terminal's output is prose with references buried in it, and *where a reference stops* is one
//! question that must have one answer. Two layers ask it — the projection, which turns a verified
//! path into a link on the glass, and the session, which decides which files to put in front of a
//! worker — and a second copy of these boundary rules would be a second opinion about what the user
//! is pointing at.
//!
//! Nothing here touches a disk. A candidate is a **shape**, and the only thing that turns a shape
//! into a link is [`PrintedPathLinks`], which is told the answers by someone who did the reading
//! off the event thread.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use crate::HyperlinkRange;

/// How a reference is spelled on the line — the one property that decides how it is turned into a
/// path, kept on the candidate so no later layer has to re-derive it from the text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrintedPathSpelling {
    /// A drive-rooted Windows path: `D:\src\a.md`, `D:/src/a.md`, or either of those in quotes.
    Absolute,
    /// A reference that names nothing until it is joined to a directory: `./a.md`, `../a.md`,
    /// `docs/a.md`, `docs\a.md`.
    Relative,
    /// A `file://` URI printed as **text** — never an OSC 8 target, which is a different shape
    /// carried by a different field and read by a different pass.
    Uri,
}

/// One printed path candidate: the half-open byte range it occupies in the line, and its spelling.
///
/// The range is the text a pointer must be over to reach the reference, which for a URI is *not*
/// the path — see [`PrintedPathSpelling::Uri`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrintedPathCandidate {
    pub byte_start: usize,
    pub byte_end: usize,
    pub spelling: PrintedPathSpelling,
}

impl PrintedPathCandidate {
    /// The candidate's own text, cut from the line it was found in.
    pub fn text<'line>(&self, line: &'line str) -> &'line str {
        &line[self.byte_start..self.byte_end]
    }
}

/// The shape gate every local reference shares: drive-rooted and nameable by this filesystem.
///
/// It says nothing about *what* the path names, which is the point — a working directory reported
/// over OSC 7 is a directory and has no extension to allow, while an image must additionally clear
/// an extension list. Keeping the two halves apart is what lets one URI decoder serve both without
/// either shape inheriting the other's privileges.
pub fn is_local_absolute_path(path: &Path) -> bool {
    let text = path.as_os_str().to_string_lossy();
    is_windows_drive_absolute(&text) && !text.contains('\0')
}

fn is_windows_drive_absolute(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

/// Allocation-light lexical candidate scan for the event thread. It recognizes only drive-rooted
/// Windows paths. Unquoted paths open at a token boundary ([`candidate_start_boundary`]) and close
/// at whitespace or a closing delimiter ([`is_path_terminator_char`]); quoted paths may contain
/// whitespace and any delimiter, and must have a closing quote. Existence, file kind, size and
/// content format are nobody's business here.
pub fn detect_absolute_path_candidates(text: &str) -> Vec<PrintedPathCandidate> {
    let bytes = text.as_bytes();
    let mut candidates = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let quoted = bytes[cursor] == b'"';
        let start = if quoted {
            cursor.saturating_add(1)
        } else {
            cursor
        };
        if !is_drive_prefix_at(bytes, start) || (!quoted && !candidate_start_boundary(text, start))
        {
            cursor += 1;
            continue;
        }
        let end = if quoted {
            bytes[start..]
                .iter()
                .position(|byte| *byte == b'"')
                .map(|offset| start + offset)
        } else {
            Some(token_end(text, start))
        };
        let Some(end) = end.filter(|end| *end > start) else {
            cursor += 1;
            continue;
        };
        if is_local_absolute_path(Path::new(&text[start..end])) {
            candidates.push(PrintedPathCandidate {
                byte_start: start,
                byte_end: end,
                spelling: PrintedPathSpelling::Absolute,
            });
        }
        cursor = if quoted {
            end.saturating_add(1)
        } else {
            end.max(cursor.saturating_add(1))
        };
    }
    candidates
}

/// Allocation-light lexical scan for relative references.
///
/// The returned range is the candidate text **exactly as printed** and is deliberately *not* a path
/// yet: a relative reference names nothing until it is joined to a directory, and this terminal only
/// ever learns a directory by being told one (OSC 7). [`resolve_relative_reference`] is the join.
///
/// Scope (user ruling 2026-08-03, widened the same day, widened again 2026-08-20 to every file):
/// `./`- and `../`-**anchored** references, and **bare** ones that carry at least one separator —
/// `local-images/sunset.svg`, `docs/HANDOFF-2026-08-20.md`. A single-segment bare name (`README`,
/// `readme.png`) stays out: one word is prose until something says otherwise, and nothing in the
/// text ever says so. The separator *is* that boundary — it is the only mark a bare reference
/// carries that ordinary prose does not.
///
/// `accepted` is the caller's own reading of what counts as a reference, and it is a **parameter of
/// the scan rather than a filter over its result** because admission decides how far the cursor
/// moves: an admitted candidate consumes its text, a refused one gives up a single character so
/// that a reference may still begin one character later. Filtering afterwards would let a caller
/// with a narrower reading lose a candidate that begins inside a token a wider reading swallowed.
pub fn detect_relative_path_candidates(
    text: &str,
    accepted: &dyn Fn(&str) -> bool,
) -> Vec<PrintedPathCandidate> {
    let bytes = text.as_bytes();
    let mut candidates = Vec::new();
    let mut cursor = 0usize;
    // The first terminator at or after some earlier opening — which is therefore the first
    // terminator at or after *every* opening before it, since no terminator lies in between. The
    // absolute scan can afford to find a token's end once per drive prefix because drive prefixes
    // are rare; a bare reference has no prefix, so every character in `{"a":1,"b":2,…}` opens a
    // candidate, and finding the same token's end once per opening would read the line once per
    // character. Reusing it reads each token once, whatever opens inside it.
    let mut token_end_seen = 0usize;
    while cursor < bytes.len() {
        // A candidate opens on a character, so a cursor resting mid-character opens nothing. The
        // absolute scan is spared this test by its drive prefix, which is ASCII and proves the
        // boundary; a bare relative reference has no such prefix to be proved by.
        if !text.is_char_boundary(cursor) {
            cursor += 1;
            continue;
        }
        let quoted = bytes[cursor] == b'"';
        let start = if quoted {
            cursor.saturating_add(1)
        } else {
            cursor
        };
        if !quoted && !candidate_start_boundary(text, start) {
            cursor += 1;
            continue;
        }
        let end = if quoted {
            bytes[start..]
                .iter()
                .position(|byte| *byte == b'"')
                .map(|offset| start + offset)
        } else {
            if start >= token_end_seen {
                token_end_seen = token_end(text, start);
            }
            Some(token_end_seen)
        };
        let Some(end) = end.filter(|end| *end > start) else {
            cursor += 1;
            continue;
        };
        let candidate = &text[start..end];
        // What opened the candidate is asked before what it says, because the bare opening is the
        // one test whose cost this loop can bound: it stops at the first character that is not a
        // path character, and every opening has such a character in front of it, so no two
        // openings can read the same stretch twice. Asking the whole candidate first — separators,
        // colon, and whatever `accepted` reads — would read one long line without terminators once
        // per character.
        //
        // Quoting is a declaration of extent: it says where the reference begins and ends, so it
        // needs neither an anchor nor a pure run to be read as one.
        let admitted = (quoted
            || is_relative_prefix_at(bytes, start)
            || bare_candidate_opens_at(text, start, candidate))
            && is_relative_reference(candidate)
            && accepted(candidate);
        if admitted {
            candidates.push(PrintedPathCandidate {
                byte_start: start,
                byte_end: end,
                spelling: PrintedPathSpelling::Relative,
            });
        }
        // An admitted candidate consumes its text, so nothing is read out of the middle of one. A
        // refused one consumes a single character: whatever it was, the reference may still begin
        // one character later — behind the `：` in `路径：dir/a.png`, or at the quote in
        // `path="./a.png"` — and a refusal is never evidence about the text that follows it.
        cursor = if admitted {
            if quoted {
                end.saturating_add(1)
            } else {
                end.max(cursor.saturating_add(1))
            }
        } else {
            cursor.saturating_add(1)
        };
    }
    candidates
}

/// The shape a relative reference must have, whatever opened it.
///
/// Three refusals. A candidate with no separator is a single bare name, which is out of scope. One
/// that *opens* with a separator names a place from the drive root rather than from here —
/// `/usr/share/x.png` in a log line, or the `//host/x.png` a scheme leaves behind — and joining it
/// to a working directory would invent a location nobody named. One containing `:` is not relative
/// at all: the colon is exactly the character that makes text absolute (`D:\…`) or schemed
/// (`file:…`, `https:…`), both of which are other scans' business and must never be claimed twice.
pub fn is_relative_reference(candidate: &str) -> bool {
    !candidate.starts_with(['/', '\\'])
        && candidate.contains(['/', '\\'])
        && !candidate.contains(':')
}

/// Whether a bare (unanchored, unquoted) candidate may open where it does.
///
/// An anchored reference declares where it begins — `./` and `../` are marks, not prose. A bare one
/// declares nothing, so the character class must speak for it: the candidate is a run of path
/// characters and nothing else, and the character before it is not `:`.
///
/// Both halves are what keep a bare reference out of a URL without anyone sniffing for URLs. The
/// run rule is why `路径：local-images/sunset.svg` is read as the reference after the colon rather
/// than as one long candidate starting at `路` — and why `x(dir/a.png` is not read from `x`. The
/// preceding-`:` rule is why `https://host:8080/img/x.png` offers nothing: `//host` opens with a
/// separator, and the `8080/img/x.png` behind the port colon would otherwise be a perfectly
/// well-formed relative name. A colon binds leftward; what follows it belongs to whatever the colon
/// already made absolute or schemed.
fn bare_candidate_opens_at(text: &str, start: usize, candidate: &str) -> bool {
    candidate.chars().all(is_path_tail_char)
        && text[..start]
            .chars()
            .next_back()
            .is_none_or(|character| character != ':')
}

/// A `.` or `..` component followed by a separator, and nothing else.
fn is_relative_prefix_at(bytes: &[u8], start: usize) -> bool {
    if bytes.get(start) != Some(&b'.') {
        return false;
    }
    let after_dots = if bytes.get(start + 1) == Some(&b'.') {
        start + 2
    } else {
        start + 1
    };
    bytes
        .get(after_dots)
        .is_some_and(|byte| matches!(*byte, b'/' | b'\\'))
}

/// Join a relative candidate onto an authoritative working directory and normalize it lexically.
///
/// Lexical by construction: `..` pops the component to its left instead of asking the filesystem
/// what a symlink underneath it resolves to. That is exactly what keeps this on the event thread —
/// existence and file kind stay worker-only, precisely as they are for a printed absolute path.
///
/// `..` that would climb past the drive root names nothing a filesystem can hold, and a join that
/// lands on the bare drive root names a directory rather than a file; both are simply not
/// candidates.
pub fn resolve_relative_reference(working_directory: &Path, relative: &str) -> Option<PathBuf> {
    if !is_local_absolute_path(working_directory) {
        return None;
    }
    let base = working_directory.as_os_str().to_str()?;
    let (drive, rest) = base.split_at(2);
    let mut components = Vec::new();
    for component in rest.split(['/', '\\']).chain(relative.split(['/', '\\'])) {
        match component {
            "" | "." => {}
            ".." => {
                components.pop()?;
            }
            named => components.push(named),
        }
    }
    if components.is_empty() {
        return None;
    }
    let mut native = String::from(drive);
    for component in components {
        native.push('\\');
        native.push_str(component);
    }
    let path = PathBuf::from(native);
    is_local_absolute_path(&path).then_some(path)
}

fn is_drive_prefix_at(bytes: &[u8], start: usize) -> bool {
    bytes.get(start).is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(start + 1) == Some(&b':')
        && bytes
            .get(start + 2)
            .is_some_and(|byte| matches!(*byte, b'\\' | b'/'))
}

/// Characters that could legitimately be the tail of a longer token, so a drive prefix that follows
/// one is a suffix of that token rather than a path of its own: alphanumerics of **any** script
/// (`prefixXC:\a.png`, `图片D:\a.png`) plus the path-structure characters that continue a path or a
/// filename stem (`sub\D:\a.png`, `file:///D:/a.png`, `v1.2D:\a.png`, `x-D:\a.png`).
///
/// `/` and `\` are on this list for one load-bearing reason: it is what keeps the `D:/…` embedded in
/// a `file:///D:/…` URI from being read as a native path. URIs reach the detector through
/// [`detect_file_uri_candidates`], which decodes them properly; they must never be half-read here.
/// The same clause serves the bare relative scan, where it does double duty: it refuses every
/// mid-URL opening (`a.b/x.png` inside `https://a.b/x.png` is preceded by `/`), and, read as a class
/// rather than a boundary, it is the run a bare candidate must consist of.
///
/// Everything else opens a path — whitespace of any width, opening brackets and quotes of any script
/// (`(`、`（`、`「`、`“`), separators (`:`、`：`、`=`、`,`), and the rest of punctuation. That
/// generality is the point: a path is no less a path for sitting in CJK prose.
pub fn is_path_tail_char(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '/' | '\\' | '.' | '-' | '_')
}

/// Closing delimiters that end an unquoted path token. Unicode General_Category Pe
/// (Close_Punctuation) as it appears in terminal prose, plus ASCII `>` and its full-width form —
/// `<>` is a bracket pair everywhere and neither character is legal in a Windows path anyway.
///
/// Final_Punctuation (Pf) is deliberately split: `»`、`›`、`”` are admitted because they only ever
/// close, while `’` is not, because it doubles as an apostrophe inside ordinary filenames
/// (`Bob’s photo.png`). A filename that really ends in a closing delimiter can still be quoted.
fn is_closing_delimiter(character: char) -> bool {
    matches!(
        character,
        ')' | ']'
            | '}'
            | '>'
            | '\u{ff09}' // ）
            | '\u{ff3d}' // ］
            | '\u{ff5d}' // ｝
            | '\u{ff60}' // ｠
            | '\u{ff63}' // ｣
            | '\u{ff1e}' // ＞
            | '\u{3009}' // 〉
            | '\u{300b}' // 》
            | '\u{300d}' // 」
            | '\u{300f}' // 』
            | '\u{3011}' // 】
            | '\u{3015}' // 〕
            | '\u{3017}' // 〗
            | '\u{3019}' // 〙
            | '\u{301b}' // 〛
            | '\u{27e9}' // ⟩
            | '\u{27eb}' // ⟫
            | '\u{00bb}' // »
            | '\u{203a}' // ›
            | '\u{201d}' // ”
    )
}

/// Where an unquoted path token stops.
pub fn is_path_terminator_char(character: char) -> bool {
    character.is_whitespace() || is_closing_delimiter(character)
}

/// End of the unquoted token starting at `start` (a byte offset on a character boundary).
fn token_end(text: &str, start: usize) -> usize {
    text[start..]
        .char_indices()
        .find(|(_, character)| is_path_terminator_char(*character))
        .map_or(text.len(), |(offset, _)| start + offset)
}

/// Whether a candidate at byte offset `start` opens a token rather than continuing one. The decision
/// is per *character*: the byte before a candidate is the last byte of a multi-byte character in
/// every CJK-adjacent line, so a byte test would reject `（D:\a.png）` and `路径：D:\a.png` on the
/// strength of a UTF-8 continuation byte alone.
pub fn candidate_start_boundary(text: &str, start: usize) -> bool {
    start == 0
        || text[..start]
            .chars()
            .next_back()
            .is_none_or(|character| !is_path_tail_char(character))
}

/// Lexical scan for `file://` URIs printed as text.
///
/// Kept apart from [`detect_absolute_path_candidates`] because the two shapes are read differently:
/// the byte span covers the URI text that must be hovered, while the file it names is only knowable
/// after decoding. A candidate is reported only when it decodes to a drive-rooted local path — a
/// `file://server/share/x` names somebody else's machine and a `file:///etc/passwd` names nothing on
/// this one.
pub fn detect_file_uri_candidates(text: &str) -> Vec<PrintedPathCandidate> {
    const SCHEME: &str = "file://";
    let bytes = text.as_bytes();
    let mut candidates = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        // The scheme is ASCII, so a byte match proves `cursor` is a character boundary before
        // `candidate_start_boundary` decodes the character preceding it.
        let scheme_matches = bytes
            .get(cursor..cursor + SCHEME.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(SCHEME.as_bytes()));
        if !scheme_matches || !candidate_start_boundary(text, cursor) {
            cursor += 1;
            continue;
        }
        let end = uri_token_end(text, cursor + SCHEME.len());
        if file_uri_to_local_reference(&text[cursor..end]).is_some() {
            candidates.push(PrintedPathCandidate {
                byte_start: cursor,
                byte_end: end,
                spelling: PrintedPathSpelling::Uri,
            });
        }
        cursor = end.max(cursor + 1);
    }
    candidates
}

/// End of a URI token that starts at `scheme_end`. A URI is ASCII by construction (RFC 3986 —
/// anything else must be percent-encoded), so any character outside the allowed set ends it, which
/// is what lets a full-width `）` close a URI that ASCII `)` cannot. Trailing sentence punctuation is
/// then released, matching [`crate::detect_http_urls`], whose prose-boundary problem is the same one.
fn uri_token_end(text: &str, scheme_end: usize) -> usize {
    let bytes = text.as_bytes();
    let mut end = scheme_end;
    while end < bytes.len() && is_uri_byte(bytes[end]) {
        end += 1;
    }
    while end > scheme_end
        && matches!(
            bytes[end - 1],
            b')' | b'.' | b',' | b';' | b':' | b'!' | b'?'
        )
    {
        end -= 1;
    }
    end
}

/// RFC 3986 unreserved + reserved + the percent sign.
fn is_uri_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b'%'
                | b':'
                | b'/'
                | b'?'
                | b'#'
                | b'['
                | b']'
                | b'@'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
        )
}

/// Whether a trailing `/` is a directory's own slash or an empty final name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrailingSlash {
    Directory,
    Reject,
}

/// Which spellings of "absolute" a decoded URI is allowed to name.
///
/// Two, because the two callers are asking about two different things. A file URI printed into the
/// flow is a **reference this terminal may open**, and opening happens through Windows, so it has to
/// be drive-rooted or it is nothing. An OSC 7 report is a **statement about where the shell that
/// sent it is standing**, and that shell may not be a Windows process at all — the same window can
/// hold a `pwsh` in `D:\src` and a WSL `bash` in `/home/weiyi/src`, and only one of those two
/// sentences can be spelled with a drive letter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rooting {
    /// `D:\…` and nothing else.
    DriveOnly,
    /// `D:\…`, or a POSIX absolute path (`/home/weiyi`, `/mnt/d/src`).
    DriveOrPosixRoot,
}

/// The `file://` URI a printed reference may name a local file with: drive-rooted, no trailing
/// slash, no host but this one.
pub fn file_uri_to_local_reference(uri: &str) -> Option<PathBuf> {
    decode_file_uri(uri, None, TrailingSlash::Reject, Rooting::DriveOnly)
}

/// Decode a `file://` URI to the local path it names, applying only the shape gate every local
/// reference shares ([`is_local_absolute_path`]) and no extension allowlist.
///
/// Resolution is per URI segment: each is percent-decoded on its own and the results are joined with
/// `\`. That is what RFC 3986 means — a `%2F` inside a segment decodes to a literal `/` in a
/// filename, never to a separator — and it costs nothing, since such a name simply fails to exist. A
/// single **trailing** empty segment is a directory's trailing slash rather than an empty name
/// (`file:///D:/src/` and `file:///D:/` both name directories); an interior one (`file:///D://a`)
/// stays rejected.
///
/// A non-empty authority is accepted only when it is `localhost` or `local_host`, this machine's own
/// name — the two spellings of "this host" that a file URI has. Anything else is a remote share
/// (`file://server/share/a.png`), which no local read may follow. Callers that must not honour a
/// hostname at all pass `None`.
pub fn decode_file_uri(
    uri: &str,
    local_host: Option<&str>,
    trailing_slash: TrailingSlash,
    rooting: Rooting,
) -> Option<PathBuf> {
    let rest = uri
        .get(..7)
        .filter(|scheme| scheme.eq_ignore_ascii_case("file://"))
        .map(|scheme| &uri[scheme.len()..])?;
    let (authority, path) = rest.split_at(rest.find('/')?);
    let authority_is_this_host = authority.is_empty()
        || authority.eq_ignore_ascii_case("localhost")
        || local_host.is_some_and(|host| authority.eq_ignore_ascii_case(host));
    if !authority_is_this_host {
        return None;
    }
    // Query and fragment are not part of the path; a filename containing `?` or `#` must have
    // percent-encoded them.
    let path = &path[..path.find(['?', '#']).unwrap_or(path.len())];
    let mut segments = path.split('/').skip(1).collect::<Vec<_>>();
    if trailing_slash == TrailingSlash::Directory
        && segments.len() > 1
        && segments.last() == Some(&"")
    {
        segments.pop();
    }
    let mut decoded_segments = Vec::with_capacity(segments.len());
    for segment in segments {
        let decoded = percent_decode(segment)?;
        if decoded.is_empty() {
            return None;
        }
        decoded_segments.push(decoded);
    }
    let mut native = decoded_segments.join("\\");
    // `file:///D:/` names the drive root: the separator that makes it a root belongs to the path.
    if native.len() == 2 && native.ends_with(':') {
        native.push('\\');
    }
    if is_local_absolute_path(Path::new(&native)) {
        return Some(PathBuf::from(native));
    }
    // Not a drive letter. The same segments spell a POSIX absolute path, and whether that is a path
    // at all is the caller's question — see [`Rooting`]. The separator is the one the shell that
    // sent it uses: `\mnt\d\src` would name the same place to `std::path`, and would be read back by
    // a person as a Windows path that has lost its drive.
    if rooting == Rooting::DriveOnly {
        return None;
    }
    let posix = format!("/{}", decoded_segments.join("/"));
    (!posix.contains('\0')).then(|| PathBuf::from(posix))
}

/// Percent-decode one URI segment. `None` when an escape is malformed, the result is not UTF-8, or
/// it carries a control character — each of which means the text was never a path we may read.
fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = segment
                .get(index + 1..index + 3)
                .filter(|hex| hex.bytes().all(|byte| byte.is_ascii_hexdigit()))?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(decoded).ok()?;
    (!decoded.chars().any(char::is_control)).then_some(decoded)
}

/// Spell a local path as the `file://` URI a link target is written in — the inverse of
/// [`decode_file_uri`] for the one shape this crate produces.
///
/// It exists because a recognized bare path has to become **the same kind of object an application
/// would have declared over OSC 8**, and that object's target is a URI. Everything downstream — the
/// hit, the hover line, the five-armed router — then reads one shape and cannot tell the two apart,
/// which is the whole point of folding a bare path into a `file:` target rather than routing it
/// separately.
///
/// Percent-encoding covers every byte outside RFC 3986's unreserved set, apart from the two
/// structural characters this shape needs literally (`/` and the drive's `:`). A space becomes
/// `%20`, and a name in any script becomes its UTF-8 bytes escaped one at a time.
pub fn local_path_to_file_uri(path: &Path) -> String {
    const UNRESERVED_EXTRA: &[u8] = b"-._~";
    let mut uri = String::from("file:///");
    for byte in path.as_os_str().to_string_lossy().bytes() {
        match byte {
            b'\\' | b'/' => uri.push('/'),
            b':' => uri.push(':'),
            byte if byte.is_ascii_alphanumeric() || UNRESERVED_EXTRA.contains(&byte) => {
                uri.push(byte as char);
            }
            byte => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

/// What this window has been told about the printed paths it can see: where relative text is
/// measured from, and which files a worker has found on the disk.
///
/// **Verification is the whole of what makes a path a link.** A shape is not a promise — the window
/// underlines what it has opened, not what it can parse — so this carries the worker's answers and
/// nothing else, and a path it has not been told about produces no link at all. The unknowns are
/// handed back through [`Self::links_in`]'s sink, which is how the layer that draws a frame tells
/// the layer that owns a worker what the frame would have liked to know.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PrintedPathLinks {
    working_directory: Option<PathBuf>,
    verified: BTreeSet<PathBuf>,
}

impl PrintedPathLinks {
    pub fn new(working_directory: Option<PathBuf>, verified: BTreeSet<PathBuf>) -> Self {
        Self {
            working_directory,
            verified,
        }
    }

    /// The `file:` links one logical line of text offers, and — into `unknown` — every path it
    /// names that nobody has been asked about yet.
    ///
    /// Ranges come back in reading order and never overlap: the three spellings are held off each
    /// other by their own boundary rules (a URI's embedded `D:/…` and a native path's `\.\` are both
    /// preceded by a path character, and [`is_relative_reference`] refuses anything carrying a `:`),
    /// so no two of them can claim the same text.
    pub fn links_in(
        &self,
        text: &str,
        unknown: &mut BTreeSet<PathBuf>,
    ) -> Vec<(HyperlinkRange, String)> {
        // Every shape this reads needs a separator: a drive-rooted path by its `X:\`, a relative
        // reference by definition, a `file://` URI by its own scheme. A line with neither slash in
        // it cannot hold one, and most lines a program prints are such a line. This is an
        // equivalence and not a guess — the three scans below would each return empty — and it is
        // what keeps a screenful of prose from being read three times to find nothing.
        if !text.contains(['/', '\\']) {
            return Vec::new();
        }
        let mut candidates = detect_absolute_path_candidates(text);
        if self.working_directory.is_some() {
            candidates.extend(detect_relative_path_candidates(text, &|_| true));
        }
        candidates.extend(detect_file_uri_candidates(text));
        candidates.sort_by_key(|candidate| candidate.byte_start);
        let mut links = Vec::new();
        for candidate in candidates {
            let Some(path) = self.resolve(candidate.text(text), candidate.spelling) else {
                continue;
            };
            if self.verified.contains(&path) {
                links.push((
                    HyperlinkRange {
                        byte_start: candidate.byte_start,
                        byte_end: candidate.byte_end,
                    },
                    local_path_to_file_uri(&path),
                ));
            } else {
                unknown.insert(path);
            }
        }
        links
    }

    /// The file one candidate names, or `None` when this window cannot say.
    ///
    /// A relative candidate with no working directory is the `None` that matters (user ruling): a
    /// relative path without an authoritative directory is a guess, and this terminal does not guess
    /// where a line of text was printed from. The scan above never even offers one.
    fn resolve(&self, text: &str, spelling: PrintedPathSpelling) -> Option<PathBuf> {
        match spelling {
            PrintedPathSpelling::Absolute => {
                let path = PathBuf::from(text);
                is_local_absolute_path(&path).then_some(path)
            }
            PrintedPathSpelling::Relative => {
                resolve_relative_reference(self.working_directory.as_deref()?, text)
            }
            PrintedPathSpelling::Uri => file_uri_to_local_reference(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every candidate one line offers, as `(text, spelling)` pairs in reading order.
    fn candidates(text: &str) -> Vec<(&str, PrintedPathSpelling)> {
        let mut found = detect_absolute_path_candidates(text);
        found.extend(detect_relative_path_candidates(text, &|_| true));
        found.extend(detect_file_uri_candidates(text));
        found.sort_by_key(|candidate| candidate.byte_start);
        found
            .into_iter()
            .map(|candidate| (candidate.text(text), candidate.spelling))
            .collect()
    }

    fn spans(text: &str) -> Vec<&str> {
        candidates(text)
            .into_iter()
            .map(|(text, _)| text)
            .collect::<Vec<_>>()
    }

    /// Boundary table row 1 and 2: the two spellings of a drive-rooted path, each read whole.
    #[test]
    fn a_drive_rooted_path_is_read_whole_in_either_slash() {
        assert_eq!(
            spans("D:\\Developer\\BetterTerminal\\README.md"),
            ["D:\\Developer\\BetterTerminal\\README.md"]
        );
        assert_eq!(
            spans("D:/Developer/BetterTerminal/README.md"),
            ["D:/Developer/BetterTerminal/README.md"]
        );
    }

    /// Boundary table rows 3 and 4. A space is the ordinary end of an unquoted token, and quoting is
    /// the one declaration of extent that lets a path carry one.
    #[test]
    fn a_space_belongs_to_a_path_only_inside_quotes() {
        assert_eq!(spans("\"D:\\a b\\c.md\""), ["D:\\a b\\c.md"]);
        // Unquoted, the same characters are two references and not one, which is the honest
        // reading: the space ended a drive-rooted token, and what follows it is a perfectly
        // well-formed relative name. Neither of them exists, so neither becomes a link.
        assert_eq!(spans("D:\\a b\\c.md"), ["D:\\a", "b\\c.md"]);
    }

    /// Boundary table rows 5 and 6: a closing delimiter ends the token, in either width.
    #[test]
    fn a_closing_delimiter_ends_an_unquoted_path() {
        assert_eq!(spans("(D:\\x\\a.md)"), ["D:\\x\\a.md"]);
        assert_eq!(spans("（D:\\x\\a.md）"), ["D:\\x\\a.md"]);
        assert_eq!(spans("[D:\\x\\a.md]"), ["D:\\x\\a.md"]);
    }

    /// Boundary table rows 7 and 8, the pair that has to be read together: CJK punctuation opens a
    /// candidate and a CJK *word* does not, because a path is no less a path for sitting in prose
    /// while an unreadable boundary is no boundary at all.
    #[test]
    fn cjk_punctuation_opens_a_path_and_a_cjk_word_does_not() {
        assert_eq!(spans("路径：D:\\x\\a.md"), ["D:\\x\\a.md"]);
        assert!(spans("中文D:\\x\\a.md").is_empty());
    }

    /// Boundary table row 9: the separator is the whole of what tells a bare reference from prose,
    /// which is why the two words the ruling names by hand are out.
    #[test]
    fn a_bare_relative_reference_needs_a_separator() {
        assert_eq!(
            spans("docs/HANDOFF-2026-08-20.md"),
            ["docs/HANDOFF-2026-08-20.md"]
        );
        assert_eq!(
            spans("docs\\HANDOFF-2026-08-20.md"),
            ["docs\\HANDOFF-2026-08-20.md"]
        );
        assert!(spans("README").is_empty());
        assert!(spans("Cargo").is_empty());
        assert!(spans("Cargo.toml").is_empty());
    }

    /// Boundary table row 10: `./` and `../` are marks, not prose, so they open a candidate wherever
    /// they stand.
    #[test]
    fn an_anchored_relative_reference_opens_on_its_anchor() {
        assert_eq!(spans("./test.md"), ["./test.md"]);
        assert_eq!(spans("see ../docs/a.md here"), ["../docs/a.md"]);
        assert_eq!(spans(".\\test.md"), [".\\test.md"]);
    }

    /// Boundary table row 11: the relative shape never claims text a colon has already made
    /// absolute or schemed, so a URL offers it nothing at all.
    #[test]
    fn a_relative_reference_never_claims_absolute_or_schemed_text() {
        assert_eq!(
            candidates("https://host:8080/img/x.png")
                .into_iter()
                .filter(|(_, spelling)| *spelling == PrintedPathSpelling::Relative)
                .count(),
            0
        );
        assert_eq!(
            candidates("D:\\x\\a.md")
                .into_iter()
                .map(|(_, spelling)| spelling)
                .collect::<Vec<_>>(),
            [PrintedPathSpelling::Absolute]
        );
    }

    /// Boundary table row 12: a `file:` URI printed as text is one candidate spelled its own way,
    /// and the `D:/…` inside it is never half-read as a native path.
    #[test]
    fn a_printed_file_uri_is_one_candidate_of_its_own_spelling() {
        assert_eq!(
            candidates("file:///D:/Developer/BetterTerminal/test.md"),
            [(
                "file:///D:/Developer/BetterTerminal/test.md",
                PrintedPathSpelling::Uri
            )]
        );
    }

    /// Boundary table row 13: a URI that names another machine, or a root this machine has no
    /// letter for, names nothing here.
    #[test]
    fn a_file_uri_off_this_machine_is_not_a_local_reference() {
        assert!(spans("file://server/share/a.md").is_empty());
        assert!(spans("file:///etc/passwd").is_empty());
        assert_eq!(
            spans("file://localhost/D:/x/a.md"),
            ["file://localhost/D:/x/a.md"]
        );
    }

    /// The fold and the unfold are one function read twice: whatever a path is spelled with, the
    /// URI a link carries decodes back to exactly it.
    #[test]
    fn the_uri_a_path_is_folded_into_decodes_back_to_it() {
        for path in [
            "D:\\Developer\\BetterTerminal\\README.md",
            "D:\\a b\\c.md",
            "D:\\文档\\说明.md",
            "D:\\p#q\\r%s.md",
        ] {
            let uri = local_path_to_file_uri(Path::new(path));
            assert!(
                uri.starts_with("file:///"),
                "{path} folds into a rooted file URI, not {uri}"
            );
            assert_eq!(
                file_uri_to_local_reference(&uri).as_deref(),
                Some(Path::new(path)),
                "{uri} is the same file as {path}"
            );
        }
    }

    /// The ruling's `None`: a relative reference measured from nowhere is not measured. It becomes
    /// neither a link nor even a question, because there is nothing to ask about.
    #[test]
    fn a_relative_reference_without_a_working_directory_is_neither_link_nor_question() {
        let links = PrintedPathLinks::new(None, BTreeSet::new());
        let mut unknown = BTreeSet::new();
        assert!(
            links
                .links_in("./test.md docs/a.md", &mut unknown)
                .is_empty()
        );
        assert!(unknown.is_empty());
    }

    /// Verification is the whole of what makes a path a link: what the worker has answered becomes
    /// one, and what it has not becomes a question for the next frame to be answered by.
    #[test]
    fn only_a_verified_path_becomes_a_link_and_the_rest_become_questions() {
        let real = PathBuf::from("D:\\src\\real.md");
        let links = PrintedPathLinks::new(
            Some(PathBuf::from("D:\\src")),
            BTreeSet::from([real.clone()]),
        );
        let mut unknown = BTreeSet::new();
        let found = links.links_in("real.md ./real.md ./gone.md D:\\src\\real.md", &mut unknown);
        assert_eq!(
            found
                .iter()
                .map(|(_, uri)| uri.as_str())
                .collect::<Vec<_>>(),
            ["file:///D:/src/real.md", "file:///D:/src/real.md"],
            "the bare `real.md` has no separator and is prose, `./gone.md` is not on the disk, and \
             the anchored and drive-rooted spellings both name the one file that is"
        );
        assert_eq!(unknown, BTreeSet::from([PathBuf::from("D:\\src\\gone.md")]));
    }

    /// §7.1.5j's alternate-screen budget, **measured rather than argued**.
    ///
    /// The surface this has to be affordable on is a full-screen program repainting itself: 200×60
    /// of ordinary TUI prose, every frame. The disk side of the cost is answered elsewhere and by
    /// construction (the ledger remembers both answers, so a repaint asks nothing — see bt-term's
    /// `a_repainting_screen_asks_the_disk_nothing_after_the_first_frame`). What is left, and what
    /// this reads, is the lexer: one pass over text the projection was already walking to find bare
    /// URLs in.
    ///
    /// No assertion beyond completing. A wall clock on a loaded developer machine measures the
    /// machine; the number is printed so it can be reported and compared.
    #[test]
    fn a_full_screenful_is_scanned_in_a_time_worth_printing() {
        // What Claude Code actually prints: prose, tree-drawing, a couple of names per line.
        let screen = (0..60u32)
            .map(|row| {
                format!(
                    "  ⎿  Read crates/bt-app/src/main.rs ({row} lines) and D:\\src\\notes-{row}.md \
                     — updated {row} files, 0 errors, https://example.invalid/{row}"
                )
            })
            .collect::<Vec<_>>();
        let links = PrintedPathLinks::new(
            Some(PathBuf::from("D:\\src")),
            (0..64u32)
                .map(|index| PathBuf::from(format!("D:\\src\\notes-{index}.md")))
                .collect(),
        );

        // And what most of a screen actually is: prose with no separator in it at all, which the
        // scan proves it can skip whole.
        let prose = (0..60u32)
            .map(|row| {
                format!(
                    "  I will read the file and then update the section that mentions it, row {row}"
                )
            })
            .collect::<Vec<_>>();

        for (label, corpus) in [("dense", &screen), ("prose", &prose)] {
            let started = std::time::Instant::now();
            let mut frames = 0u32;
            let mut found = 0usize;
            while started.elapsed() < std::time::Duration::from_millis(200) {
                let mut unknown = BTreeSet::new();
                for line in corpus {
                    found += links.links_in(line, &mut unknown).len();
                }
                frames += 1;
            }
            let per_frame = started.elapsed() / frames.max(1);
            println!(
                "§7.1.5j {label}: 60 rows × {} bytes, {} links per frame -> {:?} per frame \
                 ({frames} frames)",
                corpus[0].len(),
                found / frames.max(1) as usize,
                per_frame
            );
        }
    }

    /// A link's range covers the text that was printed, which for a URI is not the path — and no
    /// two spellings ever claim the same stretch of a line.
    #[test]
    fn a_links_range_covers_the_printed_text_and_never_a_neighbours() {
        let path = PathBuf::from("D:\\src\\a.md");
        let links = PrintedPathLinks::new(
            Some(PathBuf::from("D:\\src")),
            BTreeSet::from([path.clone()]),
        );
        let line = "D:\\src\\a.md and file:///D:/src/a.md";
        let mut unknown = BTreeSet::new();
        let found = links.links_in(line, &mut unknown);
        assert_eq!(
            found
                .iter()
                .map(|(range, _)| &line[range.byte_start..range.byte_end])
                .collect::<Vec<_>>(),
            ["D:\\src\\a.md", "file:///D:/src/a.md"]
        );
        for pair in found.windows(2) {
            assert!(
                pair[0].0.byte_end <= pair[1].0.byte_start,
                "ranges come back in reading order and disjoint"
            );
        }
    }
}
