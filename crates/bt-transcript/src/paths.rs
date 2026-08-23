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
    collections::{BTreeMap, BTreeSet},
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

/// The `:line[:col]` a printed reference may end with — the shape an agent, a compiler, a linter
/// and `grep -n` all print a place in a file as.
///
/// Both numbers are decimal positive integers, counted the way every one of those printers counts
/// them: the first line of a file is 1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrintedPathLocation {
    pub line: u32,
    pub column: Option<u32>,
}

impl PrintedPathLocation {
    /// The URI fragment a location is carried in.
    ///
    /// A location has to travel **inside the `file:` target**, because §7.1.5j ① folded a printed
    /// path into exactly the object an application would have declared over OSC 8 and that object
    /// is one string. A fragment is the one part of a URI that is not the file's name, which is
    /// precisely what a line number is — and it costs the other four arms of §7.1.5g nothing,
    /// since every one of them reaches its path through a decoder that cuts fragments already.
    ///
    /// The spelling is the one a reader of these targets will recognize from a browser's address
    /// bar and a code host's deep link: `L13`, and `L13C5` when a column was named.
    #[must_use]
    pub fn uri_fragment(self) -> String {
        match self.column {
            Some(column) => format!("L{}C{column}", self.line),
            None => format!("L{}", self.line),
        }
    }

    /// The location a `file:` target names, if it names one. The inverse of [`Self::uri_fragment`],
    /// and the only reader of that spelling.
    #[must_use]
    pub fn from_uri(uri: &str) -> Option<Self> {
        let fragment = uri.split_once('#')?.1;
        let (line, column) = match fragment.strip_prefix('L')?.split_once('C') {
            Some((line, column)) => (line, Some(positive_integer(column)?)),
            None => (fragment.strip_prefix('L')?, None),
        };
        Some(Self {
            line: positive_integer(line)?,
            column,
        })
    }
}

/// One decimal positive integer, or `None` for anything else — an empty run, a sign, a digit of
/// another script, a number that overflows, or the zero no line is numbered with.
fn positive_integer(text: &str) -> Option<u32> {
    text.bytes()
        .all(|byte| byte.is_ascii_digit())
        .then(|| text.parse::<u32>().ok())
        .flatten()
        .filter(|value| *value >= 1)
}

/// One printed path candidate: the half-open byte range it occupies in the line, its spelling, and
/// the place inside the file it may name.
///
/// **`byte_end` covers the whole reference, `:line:col` included**, because that whole string is
/// one thing's name: what a reader points at in `docs/a.md:12:3` is *that line of that file*, not
/// a link with three characters of prose behind it. `path_byte_end` is where the file's own name
/// stops, and the two are equal exactly when there is no location.
///
/// The range is the text a pointer must be over to reach the reference, which for a URI is *not*
/// the path — see [`PrintedPathSpelling::Uri`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrintedPathCandidate {
    pub byte_start: usize,
    pub path_byte_end: usize,
    pub byte_end: usize,
    pub spelling: PrintedPathSpelling,
    pub location: Option<PrintedPathLocation>,
}

impl PrintedPathCandidate {
    /// The file's own name, cut from the line it was found in — what this candidate must be
    /// resolved and verified as.
    pub fn path_text<'line>(&self, line: &'line str) -> &'line str {
        &line[self.byte_start..self.path_byte_end]
    }

    /// The whole reference as printed, location included — what a pointer must be over, and what
    /// an underline covers.
    pub fn reference_text<'line>(&self, line: &'line str) -> &'line str {
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
        let token = if quoted {
            bytes[start..]
                .iter()
                .position(|byte| *byte == b'"')
                .map(|offset| start + offset)
        } else {
            Some(token_end(text, start))
        };
        let Some(token) = token.filter(|end| *end > start) else {
            cursor += 1;
            continue;
        };
        // Quoting is a declaration of extent, so nothing inside quotes is prose to be released.
        let end = if quoted {
            token
        } else {
            release_prose_tail(text, start, token)
        };
        let (path_length, location) = split_printed_location(&text[start..end]);
        let path_byte_end = start + path_length;
        if is_local_absolute_path(Path::new(&text[start..path_byte_end])) {
            candidates.push(PrintedPathCandidate {
                byte_start: start,
                path_byte_end,
                byte_end: end,
                spelling: PrintedPathSpelling::Absolute,
                location,
            });
        }
        cursor = if quoted {
            token.saturating_add(1)
        } else {
            token.max(cursor.saturating_add(1))
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
/// `accepted` is the caller's own reading of what counts as a reference — asked of the **path**,
/// with any `:line[:col]` already split off, because an extension allowlist is a question about a
/// file's name and `sunset.png:12` names `sunset.png`. It is a **parameter of
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
        let token = if quoted {
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
        let Some(token) = token.filter(|end| *end > start) else {
            cursor += 1;
            continue;
        };
        // Both of these read a **bounded tail** — a run of non-ASCII punctuation, a run of digits
        // and colons — so they may stand in front of the opening test below without costing this
        // loop the bound that test buys it.
        let end = if quoted {
            token
        } else {
            release_prose_tail(text, start, token)
        };
        let reference = &text[start..end];
        let (path_length, location) = split_printed_location(reference);
        let candidate = &reference[..path_length];
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
                path_byte_end: start + path_length,
                byte_end: end,
                spelling: PrintedPathSpelling::Relative,
                location,
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
/// Asked of the **path** — [`split_printed_location`] has already taken any `:line[:col]` off the
/// end — which is why a located reference can clear the no-colon rule below without that rule being
/// loosened: `docs/a.md:12:3` reaches this as `docs/a.md`, and `docs/a.md:abc` reaches it whole and
/// is refused exactly as it always was.
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

/// Where the path stops inside one reference, and the `:line[:col]` that follows it.
///
/// This is the shape an agent, a compiler, a linter and `grep -n` all print a place in a file as,
/// and it is **one reference**: what a reader points at in `docs/a.md:12:3` is *that line of that
/// file*, not a link with three characters of prose behind it. So the span the caller builds covers
/// the whole of it, while the name the caller resolves and verifies stops here.
///
/// # Why the drive's own colon can never be read as a line
///
/// The search is the maximal **trailing** run of digits and colons, and a drive-rooted path carries
/// a `\` or a `/` at its third byte — `is_drive_prefix_at` demands one — so the run stops there and
/// never arrives at the `:` behind the letter. That is a property of the shapes rather than a case
/// this function tests for, which is why `C:\12` keeps its whole name and `C:\a\b.md:12` does not.
///
/// Inside that run the **leftmost** colon wins, so a reference naming both numbers is read as both:
/// `a.md:12:3` is line 12 column 3, never file `a.md:12` at line 3. Anything that is not a decimal
/// positive integer is not a number at all — `docs/a.md:abc` and `docs/a.md:0` come back whole, and
/// the caller's own shape gate then refuses or admits them exactly as it did before this existed.
fn split_printed_location(reference: &str) -> (usize, Option<PrintedPathLocation>) {
    let bytes = reference.as_bytes();
    let mut run = reference.len();
    while run > 0 && matches!(bytes[run - 1], b':' | b'0'..=b'9') {
        run -= 1;
    }
    for offset in run..reference.len() {
        if bytes[offset] != b':' {
            continue;
        }
        let tail = &reference[offset + 1..];
        let location = match tail.split_once(':') {
            Some((line, column)) => {
                positive_integer(line)
                    .zip(positive_integer(column))
                    .map(|(line, column)| PrintedPathLocation {
                        line,
                        column: Some(column),
                    })
            }
            None => positive_integer(tail).map(|line| PrintedPathLocation { line, column: None }),
        };
        if location.is_some() {
            return (offset, location);
        }
    }
    (reference.len(), None)
}

/// Release the prose an unquoted token swallowed: its trailing run of **non-ASCII punctuation**.
///
/// [`token_end`] stops at whitespace and at closing delimiters, which leaves `。`、`，`、`、` and the
/// rest of CJK sentence punctuation inside the token — so `见 D:\x\a.md。` went to the disk under a
/// name with a full stop welded to it and came back "not found" (§7.1.5j boundary table row 17).
/// The sibling scan for bare web addresses has released its own tail since the day it was written
/// ([`crate::detect_http_urls`]); this is the same debt on the same kind of text.
///
/// **A class, not a list** — the same discipline §7.1.5h ⑤ settled for URLs: enumerating full-width
/// stops means adding one every time a new script turns up, and each addition only restates what
/// the class already said. The class is "not ASCII, and not a character a path is spelled with".
///
/// Two things it deliberately does not touch. **ASCII punctuation stays**, so the trailing `.` of
/// `见 D:\x\a.md.` is still part of the reference — Windows eats a name's trailing dots, so that is
/// the same file and the link stands (boundary table row 16). And only the **tail** is released, so
/// a filename that really carries CJK punctuation in the middle of it (`D:\资料\A、B.md`) is read
/// whole; what sits at the very end of a token is prose, what sits inside it is somebody's name.
fn release_prose_tail(text: &str, start: usize, end: usize) -> usize {
    let mut end = end;
    while let Some(character) = text[start..end].chars().next_back() {
        if character.is_ascii() || is_path_tail_char(character) {
            break;
        }
        end -= character.len_utf8();
    }
    end
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
        let (path_length, location) = split_printed_location(&text[cursor..end]);
        let path_byte_end = cursor + path_length;
        if file_uri_to_local_reference(&text[cursor..path_byte_end]).is_some() {
            candidates.push(PrintedPathCandidate {
                byte_start: cursor,
                path_byte_end,
                byte_end: end,
                spelling: PrintedPathSpelling::Uri,
                location,
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
/// measured from, and what a worker found on the disk for each name it was asked about.
///
/// **Verification is the whole of what makes a path a link.** A shape is not a promise — the window
/// underlines what it has opened, not what it can parse — so this carries the worker's answers and
/// nothing else, and a path it has not been told about produces no link at all. The unknowns are
/// handed back through [`Self::links_in`]'s sink, which is how the layer that draws a frame tells
/// the layer that owns a worker what the frame would have liked to know.
///
/// # Why both answers are carried, and not just the yes
///
/// A "no" is an answer, and a frame that cannot see it has no way to tell a name nobody has looked
/// at yet from one the disk has already denied. Carrying only the yes made every dead name on the
/// screen look permanently new: the projection reported it as a question on every frame for the rest
/// of the session, and since one pass may only report a bounded number of questions, a screen
/// printing more path-shaped words than that budget spent all of it on names that were already
/// answered — so a real file lower down was never asked about at all, and never became a link
/// however long the program repainted (§7.1.5j, user report 2026-08-23).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PrintedPathLinks {
    working_directory: Option<PathBuf>,
    /// Every name this window has been given an answer for, and what the answer was. Absence means
    /// "nobody has looked", which is the one state that is worth a question.
    verdicts: BTreeMap<PathBuf, bool>,
}

impl PrintedPathLinks {
    pub fn new(working_directory: Option<PathBuf>, verdicts: BTreeMap<PathBuf, bool>) -> Self {
        Self {
            working_directory,
            verdicts,
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
            let Some(path) = self.resolve(candidate.path_text(text), candidate.spelling) else {
                continue;
            };
            // **The line is never part of what is asked about.** A reference to line 9999 of a real
            // file is a reference to a real file, and a probe carrying `:9999` would be a question
            // about a name no filesystem holds — permanently unanswerable, permanently unlinked.
            match self.verdicts.get(&path) {
                Some(true) => {
                    let mut uri = local_path_to_file_uri(&path);
                    if let Some(location) = candidate.location {
                        uri.push('#');
                        uri.push_str(&location.uri_fragment());
                    }
                    links.push((
                        HyperlinkRange {
                            byte_start: candidate.byte_start,
                            byte_end: candidate.byte_end,
                        },
                        uri,
                    ));
                }
                // Answered "no": nothing to draw and, above all, nothing to ask again. This is the
                // arm that keeps a repainting screen's question budget for the names that need it.
                Some(false) => {}
                None => {
                    unknown.insert(path);
                }
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
            .map(|candidate| (candidate.reference_text(text), candidate.spelling))
            .collect()
    }

    fn spans(text: &str) -> Vec<&str> {
        candidates(text)
            .into_iter()
            .map(|(text, _)| text)
            .collect::<Vec<_>>()
    }

    /// Every candidate as `(path text, location)` — the two halves a located reference splits into.
    fn located(text: &str) -> Vec<(&str, Option<PrintedPathLocation>)> {
        let mut found = detect_absolute_path_candidates(text);
        found.extend(detect_relative_path_candidates(text, &|_| true));
        found.extend(detect_file_uri_candidates(text));
        found.sort_by_key(|candidate| candidate.byte_start);
        found
            .into_iter()
            .map(|candidate| (candidate.path_text(text), candidate.location))
            .collect()
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

    /// The three spellings of one file that shared a screen on 2026-08-23, read at the lexer.
    ///
    /// A full-width `（` opens a relative reference for the same reason `路径：D:\x\a.md` opens an
    /// absolute one: CJK punctuation is prose, not a path character. Leading indentation opens one
    /// because whitespace always did. If any of these three stops here, the fault is lexical; if
    /// all three arrive, the fault is downstream and this test is the proof of that.
    #[test]
    fn the_three_printed_spellings_of_one_file_all_open() {
        assert_eq!(
            spans("Write(docs\\plans\\multiwindow-ef\\plan.md)"),
            ["docs\\plans\\multiwindow-ef\\plan.md"]
        );
        assert_eq!(
            spans("方案写完（docs/plans/multiwindow-ef/plan.md）。"),
            ["docs/plans/multiwindow-ef/plan.md"]
        );
        assert_eq!(
            spans("  docs/plans/multiwindow-ef/plan.md 的要点:"),
            ["docs/plans/multiwindow-ef/plan.md"]
        );
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
        let links = PrintedPathLinks::new(None, BTreeMap::new());
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
            BTreeMap::from([(real.clone(), true)]),
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

    /// A name the disk has already denied is neither a link nor a question ever again.
    ///
    /// The second half is the load-bearing one and the reason both answers are carried (§7.1.5j,
    /// user report 2026-08-23): the caller's question budget is bounded and refilled every frame, so
    /// a dead name that keeps asking is a dead name that keeps a live one from ever being asked.
    #[test]
    fn a_path_the_disk_has_denied_is_never_asked_about_again() {
        let links = PrintedPathLinks::new(
            Some(PathBuf::from("D:\\src")),
            BTreeMap::from([
                (PathBuf::from("D:\\src\\real.md"), true),
                (PathBuf::from("D:\\src\\gone.md"), false),
            ]),
        );
        let mut unknown = BTreeSet::new();
        let found = links.links_in("./real.md ./gone.md ./fresh.md", &mut unknown);
        assert_eq!(
            found
                .iter()
                .map(|(_, uri)| uri.as_str())
                .collect::<Vec<_>>(),
            ["file:///D:/src/real.md"]
        );
        assert_eq!(
            unknown,
            BTreeSet::from([PathBuf::from("D:\\src\\fresh.md")]),
            "only the name nobody has looked at yet is worth a question"
        );
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
                .map(|index| (PathBuf::from(format!("D:\\src\\notes-{index}.md")), true))
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

    /// Boundary table rows 18 and 19: a `:line` or `:line:col` behind a reference is part of the
    /// reference, in every one of the three spellings.
    ///
    /// The span is the whole of it because the whole of it is **one thing's name** — what the
    /// reader is pointing at is "this line of this file", not a link with some prose stuck to it.
    #[test]
    fn a_line_and_column_belong_to_the_reference_they_follow() {
        assert_eq!(
            spans("docs/HANDOFF-2026-08-21.md:13"),
            ["docs/HANDOFF-2026-08-21.md:13"]
        );
        assert_eq!(
            spans("docs/HANDOFF-2026-08-21.md:13:5"),
            ["docs/HANDOFF-2026-08-21.md:13:5"]
        );
        assert_eq!(spans("D:\\a\\b.md:12:3"), ["D:\\a\\b.md:12:3"]);
        assert_eq!(spans("file:///D:/x/a.md:9"), ["file:///D:/x/a.md:9"]);
        assert_eq!(
            located("docs/HANDOFF-2026-08-21.md:13:5"),
            [(
                "docs/HANDOFF-2026-08-21.md",
                Some(PrintedPathLocation {
                    line: 13,
                    column: Some(5)
                })
            )]
        );
        assert_eq!(
            located("D:\\a\\b.md:12"),
            [(
                "D:\\a\\b.md",
                Some(PrintedPathLocation {
                    line: 12,
                    column: None
                })
            )]
        );
        assert_eq!(
            located("file:///D:/x/a.md:9"),
            [(
                "file:///D:/x/a.md",
                Some(PrintedPathLocation {
                    line: 9,
                    column: None
                })
            )]
        );
    }

    /// Boundary table row 20: the colon that makes a path absolute is never the colon that opens a
    /// line number.
    ///
    /// It is out of reach **by construction** rather than by a special case: the location is read
    /// as the trailing run of digits and colons, and a drive-rooted path carries a `\` or `/` at
    /// its third byte, which stops that run long before it could arrive at the drive's own colon.
    #[test]
    fn a_drives_colon_is_never_a_line_number() {
        assert_eq!(located("C:\\12"), [("C:\\12", None)]);
        assert_eq!(located("C:/12"), [("C:/12", None)]);
        assert_eq!(
            located("C:\\a\\b.md:12:3"),
            [(
                "C:\\a\\b.md",
                Some(PrintedPathLocation {
                    line: 12,
                    column: Some(3)
                })
            )]
        );
        // A name that is itself a number keeps it: the run stops at the separator in front of it,
        // so only what follows a colon can ever be a line.
        assert_eq!(
            located("D:\\a\\1:2"),
            [(
                "D:\\a\\1",
                Some(PrintedPathLocation {
                    line: 2,
                    column: None
                })
            )]
        );
    }

    /// Boundary table row 21: what follows the colon must be a decimal positive integer, and text
    /// that is not one leaves the reference exactly as it was — which is a refusal, not a guess.
    #[test]
    fn text_behind_a_colon_that_is_not_a_number_is_not_a_location() {
        // Relative: the candidate still carries a `:`, which `is_relative_reference` refuses, so
        // the line offers nothing at all — the same "not recognized" it has always answered with.
        assert!(spans("docs/a.md:abc").is_empty());
        // Absolute: the shape gate is the drive prefix, so the whole token stays one candidate
        // with no location, goes to the disk under that name and is not found.
        assert_eq!(located("D:\\x\\a.md:abc"), [("D:\\x\\a.md:abc", None)]);
        // Zero is not a positive integer, and a line count starts at one.
        assert_eq!(located("D:\\x\\a.md:0"), [("D:\\x\\a.md:0", None)]);
    }

    /// Boundary table row 17, the debt this slice pays: a trailing full-width stop is prose behind
    /// the reference, not the tail of a filename.
    #[test]
    fn a_reference_releases_the_non_ascii_punctuation_behind_it() {
        assert_eq!(spans("见 D:\\x\\a.md。"), ["D:\\x\\a.md"]);
        assert_eq!(spans("见 docs/a.md。"), ["docs/a.md"]);
        assert_eq!(spans("见 D:\\x\\a.md:12。"), ["D:\\x\\a.md:12"]);
        assert_eq!(spans("见 D:\\x\\a.md》"), ["D:\\x\\a.md"]);
        // Interior punctuation is a filename's own: only the tail is released, so a name that
        // really carries a `、` in the middle of it is still read whole.
        assert_eq!(spans("D:\\资料\\A、B.md"), ["D:\\资料\\A、B.md"]);
        // And the limit of that, written down rather than discovered: a sentence that goes on
        // **past** the punctuation puts its own words inside the token, and words are what a
        // filename is made of. The token is then a name nothing on the disk carries, so the line
        // offers no link — the same safe "not recognized" this whole boundary table is built on.
        assert_eq!(spans("见 D:\\x\\a.md，然后"), ["D:\\x\\a.md，然后"]);
    }

    /// Boundary table row 16, nailed down so the row above cannot break it: Windows eats a trailing
    /// ASCII `.`, so the dot names the same file and stays inside the reference.
    #[test]
    fn an_ascii_full_stop_stays_inside_the_reference() {
        assert_eq!(spans("见 D:\\x\\a.md."), ["D:\\x\\a.md."]);
        assert_eq!(spans("see docs/a.md."), ["docs/a.md."]);
    }

    /// A located reference is a link to the file, and the line rides in the target's fragment:
    /// `verified` is asked about the **file**, because a reference to line 9999 of a real file is
    /// still a reference to a real file.
    #[test]
    fn a_located_reference_is_verified_by_its_file_and_carries_its_line_in_the_target() {
        let links = PrintedPathLinks::new(
            Some(PathBuf::from("D:\\src")),
            BTreeMap::from([(PathBuf::from("D:\\src\\real.md"), true)]),
        );
        let mut unknown = BTreeSet::new();
        let line = "real/../real.md:13:5 and D:\\src\\real.md:9999";
        let found = links.links_in(line, &mut unknown);
        assert_eq!(
            found
                .iter()
                .map(|(range, uri)| (&line[range.byte_start..range.byte_end], uri.as_str()))
                .collect::<Vec<_>>(),
            [
                ("real/../real.md:13:5", "file:///D:/src/real.md#L13C5"),
                ("D:\\src\\real.md:9999", "file:///D:/src/real.md#L9999"),
            ]
        );
        assert!(
            unknown.is_empty(),
            "the file is known; the line is not asked"
        );
    }

    /// The fragment a location is carried in is read back as itself, and nothing else is read as a
    /// location.
    #[test]
    fn a_locations_fragment_is_written_and_read_by_one_rule() {
        for location in [
            PrintedPathLocation {
                line: 13,
                column: None,
            },
            PrintedPathLocation {
                line: 13,
                column: Some(5),
            },
        ] {
            let uri = format!(
                "{}#{}",
                local_path_to_file_uri(Path::new("D:\\src\\a.md")),
                location.uri_fragment()
            );
            assert_eq!(PrintedPathLocation::from_uri(&uri), Some(location));
        }
        assert_eq!(PrintedPathLocation::from_uri("file:///D:/src/a.md"), None);
        assert_eq!(
            PrintedPathLocation::from_uri("file:///D:/src/a.md#intro"),
            None
        );
    }

    /// A link's range covers the text that was printed, which for a URI is not the path — and no
    /// two spellings ever claim the same stretch of a line.
    #[test]
    fn a_links_range_covers_the_printed_text_and_never_a_neighbours() {
        let path = PathBuf::from("D:\\src\\a.md");
        let links = PrintedPathLinks::new(
            Some(PathBuf::from("D:\\src")),
            BTreeMap::from([(path.clone(), true)]),
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
