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
    cmp::Reverse,
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

/// The last visual cell of the **physical** line a logical line's text was printed on, when that
/// line is ended by an application newline rather than by a DEC soft wrap.
///
/// It is the whole of what the truncation gate (§7.1.5k ①) reads. A reference whose printed text
/// reaches this cell may be a complete reference that happened to fill the row, or the front half of
/// one the application cut in two; on the strength of one line nothing tells those apart, and the
/// front half of a cut path is exactly the kind of name that exists on the disk. So the gate does
/// not try to tell them apart — it declines to promise either.
///
/// The range is in the same byte space as the text handed to [`PrintedPathLinks::links_in`], which
/// is the logical line's text with its soft-wrapped rows already rejoined. `None` at a call site
/// means the caller has no cell geometry to offer and the gate cannot run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineEndCell {
    pub byte_start: usize,
    pub byte_end: usize,
}

/// Whether a printed range reaches the physical line's last visual cell — the one test the
/// truncation gate is.
///
/// Overlap, not equality: a wide glyph occupies the last two columns and a candidate that owns its
/// lead cell owns the row's end just as much as a narrow one that lands on the final column.
#[must_use]
pub fn touches_line_end(byte_start: usize, byte_end: usize, edge: Option<LineEndCell>) -> bool {
    edge.is_some_and(|cell| byte_start < cell.byte_end && cell.byte_start < byte_end)
}

/// The bare web addresses one logical line offers, with the truncation gate already applied.
///
/// It sits here rather than at the projection because the gate is **one ruling about two shapes**:
/// §7.1.5k ① presses down an inferred path and an inferred URL for exactly the same reason, and a
/// URL is the shape where being wrong is worst — a cut address is a perfectly working link to
/// somebody else's host, while a cut path at least usually fails to exist. Two copies of the gate
/// would be two opinions about where a reference stops, which is the thing this module exists to
/// prevent.
///
/// OSC 8 is not routed through here and is deliberately untouched: an application that declared a
/// target declared it whole, and no cell geometry can contradict it.
#[must_use]
pub fn inferred_url_ranges(text: &str, edge: Option<LineEndCell>) -> Vec<HyperlinkRange> {
    crate::detect_http_urls(text)
        .into_iter()
        .filter(|range| !touches_line_end(range.byte_start, range.byte_end, edge))
        .collect()
}

/// The scheme-less bare domains one logical line offers, with the same truncation gate applied
/// (§7.38, §7.1.5k ①).
///
/// It sits beside [`inferred_url_ranges`] because it is the same ruling on the same kind of shape:
/// a bare host touching the physical line's last cell may be the front of a longer address the
/// application cut in two, and a cut address is a working link to somewhere else — the worst place
/// to be wrong. So a candidate that reaches the row's end is pressed down exactly as a schemed URL
/// or a printed path is.
#[must_use]
pub fn inferred_bare_domain_ranges(text: &str, edge: Option<LineEndCell>) -> Vec<HyperlinkRange> {
    crate::detect_bare_domains(text)
        .into_iter()
        .filter(|range| !touches_line_end(range.byte_start, range.byte_end, edge))
        .collect()
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
///
/// One unquoted token may come back as **several candidates sharing a start** — its whole self
/// first, then one shorter reading for every prose seam it carries (§7.30). They are readings of
/// one token and not several references: whoever goes to the disk asks about the longest of them
/// first and stops at the first that is there.
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
        // Quoting is a declaration of extent, so nothing inside quotes is prose to be released —
        // and, for the same reason, nothing inside them is prose to be cut at either (§7.30).
        let end = if quoted {
            token
        } else {
            release_prose_tail(text, start, token)
        };
        let token_text = &text[start..end];
        // A quoted token offers no shorter form: its quotes declared its extent. An unquoted one is
        // searched whole — a drive prefix is as rare as an anchor, so this walk happens once per
        // prefix and [`prose_seam_ends`] answers "no seams" at the cost of the walk itself.
        let seams = if quoted {
            Vec::new()
        } else {
            prose_seam_ends(token_text, token_text.len())
        };
        for form_end in std::iter::once(end).chain(seams.into_iter().map(|offset| start + offset)) {
            let (path_length, location) = split_printed_location(&text[start..form_end]);
            let path_byte_end = start + path_length;
            if is_local_absolute_path(Path::new(&text[start..path_byte_end])) {
                candidates.push(PrintedPathCandidate {
                    byte_start: start,
                    path_byte_end,
                    byte_end: form_end,
                    spelling: PrintedPathSpelling::Absolute,
                    location,
                });
            }
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
/// One unquoted token may come back as **several candidates sharing a start**, longest first — see
/// [`detect_absolute_path_candidates`] and §7.30. `accepted` is asked of each of them, which is the
/// point: `local-images/图.png,这里是说明` offers a name a picture may be found under, and the
/// whole token offers one it may not.
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
    // Whether that token can carry a seam at all — cached beside its end for the same reason and
    // read for one purpose: a token holding neither a non-ASCII character nor an opening bracket
    // offers no shorter form (§7.30, [`token_may_carry_a_seam`]) and needs no search. A screenful
    // of ordinary program output is such a screen, and this is what keeps the seam free there.
    let mut token_may_seam = false;
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
                token_may_seam = token_may_carry_a_seam(&text[start..token_end_seen]);
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
        // §7.30. Quoting is a declaration of extent, so a quoted token offers no shorter form. An
        // anchored opening carries a mark of its own and is as rare as a drive prefix, so its whole
        // token is searched. A **bare** opening is the one this loop must keep a bound on, and
        // [`bare_form_limit`] is that bound — read it there for why nothing past it could be
        // admitted anyway.
        let seams = if quoted || !token_may_seam {
            Vec::new()
        } else {
            let token_text = &text[start..end];
            let limit = if is_relative_prefix_at(bytes, start) {
                token_text.len()
            } else {
                bare_form_limit(token_text)
            };
            prose_seam_ends(token_text, limit)
        };
        // The longest admitted form, which is what this opening consumes.
        let mut admitted = None;
        for form_end in std::iter::once(end).chain(seams.into_iter().map(|offset| start + offset)) {
            let reference = &text[start..form_end];
            let (path_length, location) = split_printed_location(reference);
            let candidate = &reference[..path_length];
            // What opened the candidate is asked before what it says, because the bare opening is
            // the one test whose cost this loop can bound: it stops at the first character that is
            // not a path character, and every opening has such a character in front of it, so no
            // two openings can read the same stretch twice. Asking the whole candidate first —
            // separators, colon, and whatever `accepted` reads — would read one long line without
            // terminators once per character.
            //
            // Quoting is a declaration of extent: it says where the reference begins and ends, so
            // it needs neither an anchor nor a pure run to be read as one.
            if (quoted
                || is_relative_prefix_at(bytes, start)
                || bare_candidate_opens_at(text, start, candidate))
                && is_relative_reference(candidate)
                && accepted(candidate)
            {
                candidates.push(PrintedPathCandidate {
                    byte_start: start,
                    path_byte_end: start + path_length,
                    byte_end: form_end,
                    spelling: PrintedPathSpelling::Relative,
                    location,
                });
                admitted.get_or_insert(form_end);
            }
        }
        // An admitted candidate consumes its text, so nothing is read out of the middle of one. A
        // refused one consumes a single character: whatever it was, the reference may still begin
        // one character later — behind the `：` in `路径：dir/a.png`, or at the quote in
        // `path="./a.png"` — and a refusal is never evidence about the text that follows it.
        cursor = if let Some(admitted) = admitted {
            if quoted {
                admitted.saturating_add(1)
            } else {
                admitted.max(cursor.saturating_add(1))
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
/// Four refusals. A candidate with no separator is a single bare name, which is out of scope. One
/// that *opens* with a separator names a place from the drive root rather than from here —
/// `/usr/share/x.png` in a log line, or the `//host/x.png` a scheme leaves behind — and joining it
/// to a working directory would invent a location nobody named. One containing `:` is not relative
/// at all: the colon is exactly the character that makes text absolute (`D:\…`) or schemed
/// (`file:…`, `https:…`), both of which are other scans' business and must never be claimed twice.
///
/// # A reading must end on a character that is part of a name (user ruling 2026-08-28, §7.30)
///
/// The fourth refusal is a **trailing** separator. `src/` is a directory prefix, and a directory
/// prefix is a name nobody wrote — not even when the disk holds it, because existence was only ever
/// the licence to *draw* a reference, never to invent one. The line that settled it is git's rename
/// compression, `src/{old => new}/main.rs`, where the brace seams (§7.30) and the reading in front
/// of it is exactly such a prefix.
///
/// It closes a hole in the first refusal rather than adding a rule beside it. A bare reference is
/// admitted on the strength of **carrying a separator** — that mark is the only thing ordinary prose
/// does not have — and a trailing separator is the one place where the mark is not evidence of two
/// segments at all. `docs/` is `docs` with a slash after it, and `docs` alone was never a reference.
///
/// The absolute scan is deliberately not given the same clause: there the separator is never the
/// evidence that something is a reference (the drive prefix is), and `D:\` is a real place whose own
/// spelling ends in one.
pub fn is_relative_reference(candidate: &str) -> bool {
    !candidate.starts_with(['/', '\\'])
        && !candidate.ends_with(['/', '\\'])
        && candidate.contains(['/', '\\'])
        && !candidate.contains(':')
}

/// Whether a bare (unanchored, unquoted) candidate may open where it does.
///
/// An anchored reference declares where it begins — `./` and `../` are marks, not prose. A bare one
/// declares nothing, so the character class must speak for it: the candidate is a run of path
/// characters and nothing else, and no **binding** colon stands in front of it.
///
/// Both halves are what keep a bare reference out of a URL without anyone sniffing for URLs. The
/// run rule is why `路径：local-images/sunset.svg` is read as the reference after the colon rather
/// than as one long candidate starting at `路` — and why `x(dir/a.png` is not read from `x`. The
/// binding-colon rule is why `https://host:8080/img/x.png` offers nothing: `//host` opens with a
/// separator, and the `8080/img/x.png` behind the port colon would otherwise be a perfectly
/// well-formed relative name. A colon binds leftward; what follows it belongs to whatever the colon
/// already made absolute or schemed.
fn bare_candidate_opens_at(text: &str, start: usize, candidate: &str) -> bool {
    candidate.chars().all(is_path_tail_char) && !a_binding_colon_stands_before(text, start)
}

/// Whether the text behind `start` ends in a colon that has **made something absolute or schemed**
/// — §7.30 (user report 2026-09-03, on next29).
///
/// The colon that closes a bare opening is the drive's, the scheme's or the host's, and a drive
/// letter, a scheme ([RFC 3986] `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`) and a host are
/// **spelled in ASCII, every one of them**. So a colon with another script in front of it has made
/// nothing absolute and nothing schemed: it is the sentence's punctuation, and the name behind it
/// opens like any other.
///
/// This is [`prose_seam_ends`] read from the other side, and it is the same single character-class
/// transition. A seam is an ASCII separator with a non-ASCII character glued **after** it, which
/// says the name ended there; this is an ASCII separator with a non-ASCII character glued
/// **before** it, which says the name may begin there. `早就在:dist\\folio.exe` and
/// `docs/a.md,这里` are one person's habit seen from its two ends — half-width punctuation,
/// no space after it — and it was only ever read from one of them.
///
/// **A colon with nothing in front of it stays binding.** Absence of a witness is not a witness,
/// and a bare reference is admitted on evidence throughout this scan (a separator it carries, an
/// anchor it opens with) rather than on the lack of it.
///
/// [RFC 3986]: https://www.rfc-editor.org/rfc/rfc3986#section-3.1
fn a_binding_colon_stands_before(text: &str, start: usize) -> bool {
    let mut behind = text[..start].chars().rev();
    behind.next() == Some(':') && behind.next().is_none_or(|character| character.is_ascii())
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
///
/// The backtick is here for the same reason the web-address scan has stopped on it since it was
/// written ([`crate::is_url_terminator`]): it is a code-span delimiter, never a character a path is
/// spelled with, and an agent prints a **runnable** name — `dist\folio.exe` — inside one far more
/// often than in the open. Its opening half already makes a boundary; without this, its closing
/// half welds onto the name and the reference is asked about under a name no disk holds (user
/// report 2026-08-28: an `.exe` in a code span went unrecognized while a bare `.md` did not). A
/// filename that genuinely carries a backtick is as rare as one that ends in `)` and, like it, can
/// still be quoted.
pub fn is_path_terminator_char(character: char) -> bool {
    character.is_whitespace() || is_closing_delimiter(character) || character == '`'
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

/// The ASCII characters a **seam** may sit on — §7.30.
///
/// **A class, not a table** (user ruling 2026-08-28; the same discipline §7.1.5h ⑤ settled for
/// URLs' trailing punctuation and [`release_prose_tail`] settled for CJK stops). It was once the
/// five sentence separators `, ; : ! ?`, and the fifth screenshot proved a table is the wrong shape
/// for this: an agent wrote `docs/a.md(正斜杠)…` and `dist\folio.exe(反斜杠)…`, and because `(`
/// was not one of the five, the candidate ate its way to the closing `)` and went to the disk under
/// a name no file holds. Enumerating brackets and operators one at a time is the very thing the
/// class was invented to end.
///
/// The class is: **ASCII punctuation a path is never spelled with.** [`is_ascii_punctuation`] minus
/// the path-structure characters `/ \ . - _ ~` — everything else, from the sentence separators to
/// `(`, `[`, `{`, `"`, `'`, `=`, `+`, `*`, `#`, `@`, `&`, `%`, `|`, is a mark a name is glued to,
/// not part of the name.
///
/// [`is_ascii_punctuation`]: char::is_ascii_punctuation
fn is_seam_separator(character: char) -> bool {
    character.is_ascii_punctuation() && !matches!(character, '/' | '\\' | '.' | '-' | '_' | '~')
}

/// The ASCII **opening brackets**, which are seams on their own account — §7.30 (user ruling
/// 2026-08-28, on next16).
///
/// This is the one mark whose seam does not consult the character behind it. The line that settled
/// it is `dist\folio-next16.exe(0.1.0 (84d843f47e))`: the byte after the bracket is an ASCII `0`,
/// so the class-transition rule saw no transition, the token ate its way to the closing `)`, and a
/// file that is really on the disk went unmarked. What a version banner, a `(1)` copy suffix and
/// `docs/a.md(说明)` have in common is not the script behind the bracket — it is the bracket, which
/// opens an aside about the thing just named rather than continuing its name.
///
/// The class is stated once and not enumerated twice: these are the **opening halves of the ASCII
/// bracket pairs whose closing halves already end a token** ([`is_closing_delimiter`] holds `)`,
/// `]`, `}` and `>`). None of the four is legal in a Windows path, which is why a name loses
/// nothing by being read up to one.
///
/// A bracket is a seam and **not** a terminator, and the difference is the whole of the ruling: a
/// terminator would make `D:\x\a(1).txt` unaskable, while a seam merely offers `D:\x\a` as a
/// shorter reading — asked only after the disk has denied the longer one.
fn is_ascii_opening_bracket(character: char) -> bool {
    matches!(character, '(' | '[' | '{' | '<')
}

/// Whether a token can carry a seam at all — the cheap per-token test that keeps an ordinary
/// screenful free (§7.30 ④).
///
/// A seam is either a transition into another script or an opening bracket, so a token holding
/// neither offers no shorter form and needs no search. Read over bytes rather than characters
/// because the two questions agree byte for byte: every byte of a multi-byte character is non-ASCII,
/// and no bracket byte can appear inside one.
fn token_may_carry_a_seam(token: &str) -> bool {
    token
        .bytes()
        .any(|byte| !byte.is_ascii() || is_ascii_opening_bracket(char::from(byte)))
}

/// Where one unquoted token offers a **shorter form** of itself, longest first — §7.30.
///
/// A seam is one **character-class transition** and not a list of stops: a [`is_seam_separator`]
/// with a non-ASCII character glued straight onto it. That is a person writing another script
/// through an ASCII keyboard — `docs/a.md,这里是操作顺序` — and the separator is the sentence's,
/// not the name's. Both halves of the transition are load-bearing:
///
/// * ASCII → ASCII is **not** a seam. `D:\x\a.md,b` is one name; a comma is legal in a Windows
///   filename and nothing on the line says this one is punctuation.
/// * non-ASCII → anything is **not** a seam. A full-width stop is released whole by
///   [`release_prose_tail`] (boundary table row 17), and `D:\资料\A、B.md` is somebody's filename
///   read whole (row 19 stands unmoved).
///
/// **An [`is_ascii_opening_bracket`] is the exception, and it is a seam whatever follows it** (user
/// ruling 2026-08-28) — read that function for why a bracket carries its own evidence and needs no
/// witness behind it. It is found in this same pass and not a second one: a seam search is one walk
/// over the token, and the two questions are asked of each character where it stands.
///
/// The offsets are the separator's own, so the form ends **before** it, and they come back
/// descending so a caller reads the longest form first. `limit` is the last offset a seam may sit
/// on; a caller that can prove no longer form could ever be admitted passes it to keep this scan
/// inside the bound its loop is read under.
fn prose_seam_ends(token: &str, limit: usize) -> Vec<usize> {
    let mut seams = Vec::new();
    for (offset, character) in token.char_indices() {
        if offset > limit {
            break;
        }
        // `offset + 1` is a character boundary: every separator is one ASCII byte.
        let seams_here = is_ascii_opening_bracket(character)
            || (is_seam_separator(character)
                && token[offset + 1..]
                    .chars()
                    .next()
                    .is_some_and(|next| !next.is_ascii()));
        if seams_here {
            seams.push(offset);
        }
    }
    seams.reverse();
    seams
}

/// How far a **bare** opening's forms can possibly reach, and therefore where its seam search may
/// stop — the same bound [`bare_candidate_opens_at`] buys the scan that calls it.
///
/// A bare candidate's path must be a run of path characters ([`bare_candidate_opens_at`]), and the
/// only thing that may follow that run is the `:line[:col]` [`split_printed_location`] takes off.
/// So no form ending past this offset can ever be admitted: its path would have to carry the first
/// non-path character of the token, and a form whose path carries one is refused. Reading the whole
/// token for seams instead would cost this loop the bound that keeps a line without terminators
/// from being read once per character.
fn bare_form_limit(token: &str) -> usize {
    let mut limit = token
        .char_indices()
        .find(|(_, character)| !is_path_tail_char(*character))
        .map_or(token.len(), |(offset, _)| offset);
    // At most a line and a column, which is the whole of the location syntax.
    for _ in 0..2 {
        let Some(tail) = token[limit..].strip_prefix(':') else {
            break;
        };
        let digits = tail.len()
            - tail
                .trim_start_matches(|character: char| character.is_ascii_digit())
                .len();
        if digits == 0 {
            break;
        }
        limit += 1 + digits;
    }
    limit
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

/// The refusals §7.1.5k ④ lays **on top of** the boundary table's lexical reading, without altering
/// a word of it.
///
/// Every one of them answers the same question in the same direction: the text is shaped like a
/// reference, and something else on the line says the reference is not this window's to promise.
/// They live here rather than inside the scans because the boundary table's 23 rows are a settled
/// account of where a *token* stops, and none of these is about where a token stops — they are
/// about who owns it, what expanded it, and whether the name reaches an object a click can open.
///
/// `scheme_spans` is passed in because it is one answer for the whole line and would otherwise be
/// recomputed once per candidate.
fn is_promisable(
    text: &str,
    candidate: &PrintedPathCandidate,
    scheme_spans: &[HyperlinkRange],
) -> bool {
    // A scheme owns its own text — including the drive-rooted path somebody put in a query string.
    if scheme_spans
        .iter()
        .any(|span| span.byte_start < candidate.byte_end && candidate.byte_start < span.byte_end)
    {
        return false;
    }
    let path = candidate.path_text(text);
    let before = text[..candidate.byte_start].chars().next_back();
    // `$HOME/docs/a.md`: the tail of a variable expression is shaped exactly like a bare relative
    // name, and joining it to this window's working directory invents a place nobody named. The
    // other spellings (`${…}`, `%…%`, `$env:…`) are already refused by the boundary rules.
    if before == Some('$') {
        return false;
    }
    // A candidate that stopped at a **space** inside an opening quote has not reached the end of
    // what was quoted — the quote said where the reference ends and the candidate is short of it.
    // A candidate that stopped at the closing quote itself is complete and is left alone.
    if let Some(opening) = before
        && let Some(closing) = matching_quote(opening)
        && text[candidate.byte_end..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        && text[candidate.byte_end..].contains(closing)
    {
        return false;
    }
    // Git's diff namespace. `a/` and `b/` are not directories in the working tree, and a tree that
    // happens to hold one of them would otherwise be pointed at instead of the file being diffed.
    if (text.starts_with("--- a/") || text.starts_with("+++ b/")) && candidate.byte_start == 4 {
        return false;
    }
    match candidate.spelling {
        // A URI's printed text is not a path; its target is what a decoder already read, and that
        // decoder has refused an interior empty segment since it was written.
        PrintedPathSpelling::Uri => file_uri_to_local_reference(path)
            .is_none_or(|decoded| !names_a_dos_device(&decoded.to_string_lossy())),
        // `D:\\case\\src\\main.rs` on the screen is a serialized string, not a path literal.
        // Windows may collapse the repeats onto the same file, but that is a coincidence of the API
        // rather than something the printed text said, and one round of unescaping is a guess about
        // which producer wrote it.
        //
        // A DOS device name is refused for the neighbouring reason: metadata succeeds on it, and
        // whatever opens it afterwards may reach an entirely different kind of object.
        _ => !has_interior_empty_component(path) && !names_a_dos_device(path),
    }
}

/// The closing partner of an opening quote, for the quotes that come in pairs and are **not**
/// already boundary characters. ASCII `"` is absent because it declares extent by itself and is
/// read by the scans; `'` is absent as a closer because it doubles as an apostrophe.
fn matching_quote(opening: char) -> Option<char> {
    match opening {
        '\'' => Some('\''),
        '\u{201c}' => Some('\u{201d}'), // “ ”
        '\u{2018}' => Some('\u{2019}'), // ‘ ’
        _ => None,
    }
}

/// Whether a path carries an empty component somewhere other than at its very end — `D:\\a\\b`,
/// `docs//a.md`. A single trailing separator is a directory's own and is not one of these.
fn has_interior_empty_component(path: &str) -> bool {
    let rest = if is_windows_drive_absolute(path) {
        &path[3..]
    } else {
        path
    };
    let mut segments = rest.split(['/', '\\']).collect::<Vec<_>>();
    segments.pop();
    segments.iter().any(|segment| segment.is_empty())
}

/// Whether a path's last component is one of the names Windows reserves for devices, with or
/// without an extension: `D:\case\CON`, `D:\case\NUL.txt`.
fn names_a_dos_device(path: &str) -> bool {
    const DEVICES: [&str; 6] = ["CON", "PRN", "AUX", "NUL", "COM", "LPT"];
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = name.split('.').next().unwrap_or(name);
    DEVICES.iter().any(|device| {
        stem.eq_ignore_ascii_case(device)
            // Compared as bytes, never sliced as a string: a four-byte stem is
            // as often `*⑥` — one ASCII byte and one three-byte character — as
            // it is `COM1`, and `stem[..3]` on the former is a panic in the
            // middle of the ⑥ (crash report 2026-08-27, four times in two
            // minutes because the session restore re-printed the same line).
            // A non-ASCII byte never equals an ASCII one, so the byte comparison
            // answers the same question without asking where the characters are.
            || (matches!(*device, "COM" | "LPT")
                && stem.len() == device.len() + 1
                && stem.as_bytes()[..device.len()].eq_ignore_ascii_case(device.as_bytes())
                && stem.as_bytes()[device.len()].is_ascii_digit()
                && stem.as_bytes()[device.len()] != b'0')
    })
}

/// The fields whose value this window will read as a Windows **search path** rather than as one
/// name — §7.1.5k ③.
///
/// It is a short, explicit list and not a rule about semicolons, because a semicolon is a perfectly
/// legal character in a Windows filename: `"D:\case\real;D:\case\missing"` may be one file that
/// does not exist, and splitting it because the whole string was not found would light up
/// `D:\case\real` — turning "this is not here" into a promise about somewhere the writer never
/// named. Only a field that has *said* the value is a list is evidence that it is one.
const PATH_LIST_FIELDS: [&str; 2] = ["PATH", "PSMODULEPATH"];

/// The `NAME=` field a path list is declared by, if this line declares one: the byte range of its
/// value, which runs to the end of the logical line.
fn path_list_value(text: &str) -> Option<std::ops::Range<usize>> {
    let bytes = text.as_bytes();
    for (offset, byte) in bytes.iter().enumerate() {
        if *byte != b'=' {
            continue;
        }
        let head = &text[..offset];
        // `get` and not indexing: the label is matched by **bytes** from the end, and the byte that
        // many places back is a UTF-8 continuation byte in every line whose field is written in
        // another script (`路径=D:\bin`). Slicing there is a panic; asking for the slice is a `None`.
        if PATH_LIST_FIELDS.iter().any(|field| {
            head.len().checked_sub(field.len()).is_some_and(|start| {
                head.get(start..)
                    .is_some_and(|tail| tail.eq_ignore_ascii_case(field))
                    && candidate_start_boundary(text, start)
            })
        }) {
            return Some(offset + 1..text.len());
        }
    }
    None
}

/// The segments of a declared path list, as candidates in the line's own byte space.
///
/// Windows list syntax and nothing more: segments separated by `;`, each optionally carrying its
/// own quotes, and — because the whole value is often quoted once around a list rather than once
/// per segment — one pair of quotes around the entire value stripped first. **The semicolon is
/// never part of a candidate**, and neither are the quotes.
///
/// The first version links only a literal drive-rooted segment. An empty segment is Windows'
/// spelling of "the current directory", a `%VAR%` segment is the *printing* process's environment
/// and not this one's, and a relative segment is measured from a directory nobody reported — three
/// different unknowns, none of which a single name can be promised for.
fn detect_path_list_candidates(text: &str) -> Vec<PrintedPathCandidate> {
    let Some(value) = path_list_value(text) else {
        return Vec::new();
    };
    // An unbalanced quote is an incomplete value: the line was cut, or the field is not what it
    // looks like. Either way the syntax is not complete and the list is not read.
    if text[value.clone()]
        .bytes()
        .filter(|byte| *byte == b'"')
        .count()
        % 2
        != 0
    {
        return Vec::new();
    }
    let mut span = value.clone();
    if text[span.clone()].len() >= 2
        && text[span.clone()].starts_with('"')
        && text[span.clone()].ends_with('"')
        && !text[span.start + 1..span.end - 1].contains('"')
    {
        span = span.start + 1..span.end - 1;
    }
    let mut candidates = Vec::new();
    let mut cursor = span.start;
    let mut quoted = false;
    let mut segment = span.start;
    let bytes = text.as_bytes();
    while cursor <= span.end {
        let ends = cursor == span.end || (bytes[cursor] == b';' && !quoted);
        if !ends {
            if bytes[cursor] == b'"' {
                quoted = !quoted;
            }
            cursor += 1;
            continue;
        }
        let mut start = segment;
        let mut end = cursor;
        if end > start + 1 && bytes[start] == b'"' && bytes[end - 1] == b'"' {
            start += 1;
            end -= 1;
        }
        if is_local_absolute_path(Path::new(&text[start..end])) && !text[start..end].contains('"') {
            candidates.push(PrintedPathCandidate {
                byte_start: start,
                path_byte_end: end,
                byte_end: end,
                spelling: PrintedPathSpelling::Absolute,
                location: None,
            });
        }
        cursor += 1;
        segment = cursor;
    }
    candidates
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
/// hold a `pwsh` in `D:\src` and a WSL `bash` in `/home/alice/src`, and only one of those two
/// sentences can be spelled with a drive letter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rooting {
    /// `D:\…` and nothing else.
    DriveOnly,
    /// `D:\…`, or a POSIX absolute path (`/home/alice`, `/mnt/d/src`).
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

/// A reference an application cut across a real newline, put back together — and the receipt that
/// says exactly what it was put back together **from**.
///
/// Everything in it is part of the promise, not decoration. The two spans are the text the two
/// halves occupy on their own physical lines; the target is the file the lexer split out of the
/// joined text; the base is the directory a relative half was measured from. A projection rebuilds
/// this from the current geometry on every pass, so a resize, a reflow, a scroll that changes which
/// rows are neighbours, or a working directory that moved does not leave a stale receipt behind —
/// the next pass produces a different one, or none at all.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RejoinedReference {
    /// Byte range inside the upper physical line.
    pub upper: HyperlinkRange,
    /// Byte range inside the lower physical line, always opening at 0.
    pub lower: HyperlinkRange,
    /// The disk target, with any `:line[:col]` already taken off.
    pub target: PathBuf,
    /// The directory a relative half was measured from, and `None` for a drive-rooted one.
    pub resolution_base: Option<PathBuf>,
    /// The `file:` target, location carried in the fragment exactly as a single-line reference
    /// carries it.
    pub uri: String,
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
    /// so no two of them can claim the same text. The several readings one seam-bearing token
    /// offers (§7.30) do share text, and at most one of them ever becomes a link — which is what
    /// the walk below is: one token, one answer.
    pub fn links_in(
        &self,
        text: &str,
        edge: Option<LineEndCell>,
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
        let candidates = self.candidates_in(text);
        let mut links = Vec::new();
        let mut index = 0usize;
        while index < candidates.len() {
            // One token's forms (§7.30), longest first. They share a start — nothing else does,
            // because the three spellings are held off each other by their own boundary rules — so
            // a run of equal starts is exactly one token's worth of readings, and the ruling is
            // asked of that run as a whole rather than of each reading on its own.
            let start = candidates[index].byte_start;
            let mut forms = index;
            while forms < candidates.len() && candidates[forms].byte_start == start {
                forms += 1;
            }
            // Whether a **longer** form of this token is still waiting for the disk. Until it
            // answers, a shorter form that already exists is not drawn: the ruling is longest
            // first, and a frame that has not been told yet is a frame with nothing to promise.
            let mut pending = false;
            for candidate in &candidates[index..forms] {
                // §7.1.5k ①, **in front of the ledger and in front of the probe queue**. A
                // candidate that reaches the physical line's last visual cell may be the front half
                // of a reference the application cut in two, and the front half of a cut path is
                // exactly the kind of name that exists on the disk — so neither an answer already
                // in the ledger nor one this frame could go and fetch is allowed to decide it.
                // Pressing it down here is what keeps an old `yes` for `D:\WINDOWS\system` from
                // lighting up a span that means `D:\WINDOWS\system32\…` (scenario 67).
                //
                // The gate is asked **per form**: a shorter form stops before the seam's own
                // punctuation and the prose behind it, so it never touches the row's last cell, and
                // that punctuation is itself the evidence that the name ended before any cut.
                if touches_line_end(candidate.byte_start, candidate.byte_end, edge) {
                    continue;
                }
                let Some(path) = self.resolve(candidate.path_text(text), candidate.spelling) else {
                    continue;
                };
                // **The line is never part of what is asked about.** A reference to line 9999 of a
                // real file is a reference to a real file, and a probe carrying `:9999` would be a
                // question about a name no filesystem holds — permanently unanswerable,
                // permanently unlinked.
                match self.verdicts.get(&path) {
                    Some(true) => {
                        if !pending {
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
                        // Nothing shorter can be reached past a form the disk holds, answered or
                        // not: this token's reading is settled here.
                        break;
                    }
                    // Answered "no": nothing to draw and, above all, nothing to ask again. This is
                    // the arm that keeps a repainting screen's question budget for the names that
                    // need it — and, for a seam, the arm that hands the token to its next form.
                    Some(false) => {}
                    None => {
                        unknown.insert(path);
                        pending = true;
                    }
                }
            }
            index = forms;
        }
        links
    }

    /// Put back together a reference the application itself cut across a real newline — §7.1.5k ②,
    /// and the debt §7.1.5j left as #16.
    ///
    /// **Five gates, and the first version keeps every one of them shut as far as it goes.** A
    /// soft wrap is rejoined by the terminal's own `continues` record and never reaches here; an
    /// application newline leaves no record at all, so nothing here is read from provenance and
    /// everything is read from evidence:
    ///
    /// 1. the upper half reaches its physical line's last visual cell — otherwise the application
    ///    had room and chose to stop, which is not a cut;
    /// 2. the lower half opens at visual column 0. *Not* "at the same indent as the line above":
    ///    two stack frames, two diagnostics and two directory entries all share an indent, and a
    ///    shared indent is what peer lines look like rather than what a wrap looks like;
    /// 3. the joined text, handed back to **this same lexer**, is exactly one candidate covering
    ///    all of it. Not "contains a candidate" — a query string, a tail of prose or a second
    ///    reference would then be silently dropped and the promise would cover text it did not
    ///    read;
    /// 4. the disk holds the target the lexer split out — the file's own name, never the printed
    ///    string with `:line:col` welded to it;
    /// 5. neither half is a verified reference on its own. This is the gate that keeps two real
    ///    neighbouring log lines from being spliced into a third real path, and it is deliberately
    ///    **not** relaxed for the case that started this slice: `D:\WINDOWS\system` +
    ///    `32\…\Modules` ends as a blank, and a blank is the honest answer.
    ///
    /// A bare **web address** is never rejoined here, and cannot be: gate 4 is a witness on this
    /// machine's disk, and no such witness exists for a host. Its cut upper half is pressed down by
    /// [`inferred_url_ranges`] instead, and its lower half is judged on its own terms.
    pub fn rejoin_across_newline(
        &self,
        upper: &str,
        upper_edge: LineEndCell,
        lower: &str,
        unknown: &mut BTreeSet<PathBuf>,
    ) -> Option<RejoinedReference> {
        // Gate 1. Exactly one candidate may reach the row's end: two would mean the lexer cannot
        // say which of them the application cut.
        let upper_candidates = self.candidates_in(upper);
        let mut reaching = upper_candidates
            .iter()
            .copied()
            .filter(|candidate| {
                touches_line_end(candidate.byte_start, candidate.byte_end, Some(upper_edge))
            })
            .collect::<Vec<_>>();
        let head = reaching.pop().filter(|_| reaching.is_empty())?;
        let upper_tail = upper.get(head.byte_start..upper_edge.byte_end)?;

        // Gate 5, asked before the disk so a refusal costs nothing: a half that already names a
        // file of its own is that file's reference and not the front of somebody else's. **Any of
        // its forms counts** — a seam that has already earned the upper half a link is that link's
        // evidence, and rejoining would lay a second promise over text the first one covers. It is
        // asked of the halves rather than of the join, so it is asked once, ahead of the readings.
        if upper_candidates.iter().any(|candidate| {
            candidate.byte_start == head.byte_start && self.is_verified(candidate, upper)
        }) {
            return None;
        }
        if self
            .candidates_in(lower)
            .into_iter()
            .any(|candidate| candidate.byte_start == 0 && self.is_verified(&candidate, lower))
        {
            return None;
        }

        // Gate 2. The lower line's first cell is the continuation, so its first token opens at 0.
        //
        // §7.30 coexisting with the rejoin (user note 2026-08-28): the continuation may carry a
        // seam of its own — `5.exe(反斜杠)`, an agent's prose glued to the tail of the name it just
        // wrapped — and that seam is the sentence's, not the name's. So the continuation offers the
        // same **several readings of one token** the single-line scan offers, its whole self first
        // and then one per seam, and they are tried **longest first against the disk** exactly as
        // they are there. That ordering is what keeps an opening bracket a seam and not a
        // terminator across a wrap too: `a(1` + `).txt` is asked whole before `a` is asked at all.
        // When the token carries no seam this is the whole of it, byte for byte as before.
        let raw_lower_end = token_end(lower, 0);
        let readings = std::iter::once(raw_lower_end)
            .chain(prose_seam_ends(&lower[..raw_lower_end], raw_lower_end));
        for lower_end in readings {
            let Some(lower_head) = lower.get(..lower_end).filter(|head| !head.is_empty()) else {
                continue;
            };

            // Gate 3. One candidate, covering all of it — one *token*, that is: the shorter forms a
            // seam offers (§7.30) are readings of the same token and not a second reference, so what
            // this gate refuses is a second **start**, exactly as it always did. A reading that does
            // not spell one whole reference is not a question about the disk; the next one may be.
            let joined = format!("{upper_tail}{lower_head}");
            let spelled = self.candidates_in(&joined);
            let Some(whole) = spelled.first().copied().filter(|candidate| {
                candidate.byte_start == 0
                    && candidate.byte_end == joined.len()
                    && spelled.iter().all(|form| form.byte_start == 0)
            }) else {
                continue;
            };

            // Gate 4. The witness, asked about the file's own name. A denial hands the question to
            // the next shorter reading; a name **nobody has looked at yet** ends the search instead,
            // because a shorter reading may never be promised while a longer one is still unanswered
            // (§7.30 ②) — the answer arrives and the next frame decides.
            let Some(target) = self.resolve(whole.path_text(&joined), whole.spelling) else {
                continue;
            };
            match self.verdicts.get(&target) {
                Some(true) => {}
                Some(false) => continue,
                None => {
                    unknown.insert(target);
                    return None;
                }
            }
            let mut uri = local_path_to_file_uri(&target);
            if let Some(location) = whole.location {
                uri.push('#');
                uri.push_str(&location.uri_fragment());
            }
            return Some(RejoinedReference {
                upper: HyperlinkRange {
                    byte_start: head.byte_start,
                    byte_end: upper_edge.byte_end,
                },
                lower: HyperlinkRange {
                    byte_start: 0,
                    byte_end: lower_end,
                },
                target,
                resolution_base: match whole.spelling {
                    PrintedPathSpelling::Relative => self.working_directory.clone(),
                    _ => None,
                },
                uri,
            });
        }
        None
    }

    /// Every candidate one text offers, in reading order — the one scan both the single-line pass
    /// and the rejoin read, so the two can never disagree about where a reference stops.
    fn candidates_in(&self, text: &str) -> Vec<PrintedPathCandidate> {
        let mut candidates = detect_absolute_path_candidates(text);
        if self.working_directory.is_some() {
            candidates.extend(detect_relative_path_candidates(text, &|_| true));
        }
        candidates.extend(detect_file_uri_candidates(text));
        // §7.1.5k ③. A declared search path owns its whole value: inside it the ordinary reading —
        // "one unquoted token, semicolons and all" — is not a second opinion worth keeping, it is
        // the wrong one, and leaving it in would put a link on `D:\bin;;%SystemRoot%\System32`.
        if let Some(value) = path_list_value(text) {
            candidates.retain(|candidate| {
                candidate.byte_start >= value.end || candidate.byte_end <= value.start
            });
            candidates.extend(detect_path_list_candidates(text));
        }
        let scheme_spans = crate::http_scheme_spans(text);
        candidates.retain(|candidate| is_promisable(text, candidate, &scheme_spans));
        // Reading order, and inside one token the **longest form first** (§7.30): the forms of one
        // seam-bearing token share a start, and the whole of the ruling is that the disk is asked
        // about the longest of them before any shorter one may be promised. The six refusals above
        // are asked of every form on its own — `docs/NUL,这里` is not a device name and
        // `docs/NUL` is.
        candidates.sort_by_key(|candidate| (candidate.byte_start, Reverse(candidate.byte_end)));
        candidates
    }

    /// Whether one candidate already names a file the disk has been read for and answered yes to.
    fn is_verified(&self, candidate: &PrintedPathCandidate, text: &str) -> bool {
        self.resolve(candidate.path_text(text), candidate.spelling)
            .is_some_and(|path| self.verdicts.get(&path) == Some(&true))
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
            spans("D:\\Developer\\folio-terminal\\README.md"),
            ["D:\\Developer\\folio-terminal\\README.md"]
        );
        assert_eq!(
            spans("D:/Developer/folio-terminal/README.md"),
            ["D:/Developer/folio-terminal/README.md"]
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
            candidates("file:///D:/Developer/folio-terminal/test.md"),
            [(
                "file:///D:/Developer/folio-terminal/test.md",
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
            "D:\\Developer\\folio-terminal\\README.md",
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
                .links_in("./test.md docs/a.md", None, &mut unknown)
                .is_empty()
        );
        assert!(unknown.is_empty());
    }

    /// The last visual cell of `text`, as the gate reads it when the whole line is one row of
    /// exactly `columns` narrow cells and the reference ends the row.
    fn last_cell_of(text: &str) -> Option<LineEndCell> {
        let last = text.char_indices().next_back()?;
        Some(LineEndCell {
            byte_start: last.0,
            byte_end: text.len(),
        })
    }

    /// Every link one line offers as `(printed span, target)` — the oracle the matrix asserts on,
    /// which is span *and* target and never merely "underlined or not".
    fn linked<'line>(
        links: &PrintedPathLinks,
        line: &'line str,
        edge: Option<LineEndCell>,
    ) -> Vec<(&'line str, String)> {
        let mut unknown = BTreeSet::new();
        links
            .links_in(line, edge, &mut unknown)
            .into_iter()
            .map(|(range, uri)| (&line[range.byte_start..range.byte_end], uri))
            .collect()
    }

    fn ledger(working_directory: &str, verdicts: &[(&str, bool)]) -> PrintedPathLinks {
        PrintedPathLinks::new(
            Some(PathBuf::from(working_directory)),
            verdicts
                .iter()
                .map(|(path, answer)| (PathBuf::from(path), *answer))
                .collect(),
        )
    }

    /// §7.1.5k ① / scenario 53, 54, 55: a reference that reaches the physical line's last visual
    /// cell is `edge-suspect` and is not drawn, and one that stops short of it is untouched.
    ///
    /// The middle of the three is the whole ruling: `D:\case\src\main.rs` **exists**, and existence
    /// is refused as a licence precisely because the front half of a path an application cut in two
    /// is the kind of name that exists. Row 55 is the other half of the boundary — the last cell
    /// belongs to a closing bracket, so the candidate never reached it and nothing is suspect.
    #[test]
    fn a_reference_that_reaches_the_last_visual_cell_is_not_drawn() {
        let links = ledger("D:\\case", &[("D:\\case\\src\\main.rs", true)]);
        let full = "D:\\case\\src\\main.rs";
        assert_eq!(
            linked(&links, full, None),
            [(full, String::from("file:///D:/case/src/main.rs"))],
            "scenario 54: a candidate away from the row's end is an ordinary link"
        );
        assert_eq!(
            linked(&links, full, last_cell_of(full)),
            [],
            "scenario 53: exactly filling the row is indistinguishable from being cut, and \
             existence is not a licence"
        );
        let bracketed = "(D:\\case\\src\\main.rs)";
        assert_eq!(
            linked(&links, bracketed, last_cell_of(bracketed)),
            [(full, String::from("file:///D:/case/src/main.rs"))],
            "scenario 55: the last cell is a prose delimiter, so the candidate never reached it"
        );
    }

    /// Cross-verification of the 2026-08-28 report (`dist\folio-next15.exe` was said not to be
    /// underlined while `docs/plans/release/…​.md` was): the four separator × extension
    /// combinations, each verified on the disk, every one produces a `file:` link. The recognition
    /// layer reads `/` and `\` alike (both split the same components) and applies **no** extension
    /// gate here (`candidates_in` accepts every reference), so `.exe` is no different from `.md`.
    /// Neither the backslash nor the extension is the discriminator.
    #[test]
    fn a_relative_reference_is_recognized_in_either_slash_and_for_any_extension() {
        let links = ledger(
            "D:\\src",
            &[
                ("D:\\src\\docs\\plans\\a.md", true),
                ("D:\\src\\dist\\folio.exe", true),
                ("D:\\src\\docs\\plans\\a.exe", true),
                ("D:\\src\\dist\\folio.md", true),
            ],
        );
        for (line, uri) in [
            ("docs/plans/a.md", "file:///D:/src/docs/plans/a.md"),
            ("dist\\folio.exe", "file:///D:/src/dist/folio.exe"),
            ("docs/plans/a.exe", "file:///D:/src/docs/plans/a.exe"),
            ("dist\\folio.md", "file:///D:/src/dist/folio.md"),
        ] {
            assert_eq!(
                linked(&links, line, None),
                [(line, String::from(uri))],
                "the {line:?} reference links like any other"
            );
        }
        // As it really appears: a Windows prompt with an absolute cwd, then the bare relative form.
        assert!(
            linked(&links, "PS D:\\src> dist\\folio.exe", None)
                .iter()
                .any(|(text, _)| *text == "dist\\folio.exe"),
            "the bare .exe after a prompt is linked"
        );
    }

    /// §7.1.5j / §7.30: a reference an agent prints inside an inline code span — the ordinary form
    /// for a **runnable** name like `dist\folio.exe` — is still a reference. Its opening backtick
    /// already made a boundary; its closing backtick has to end the token the way whitespace and a
    /// closing bracket do, or the `` ` `` welds onto the name and it is asked about under a name no
    /// disk holds. The web-address scan has terminated on a backtick since it was written
    /// ([`crate::detect_http_urls`] via [`crate::is_url_terminator`]); this is the same debt on the
    /// same kind of text, and both slashes carry it.
    #[test]
    fn a_reference_inside_an_inline_code_span_is_still_recognized() {
        let links = ledger(
            "D:\\src",
            &[
                ("D:\\src\\dist\\folio.exe", true),
                ("D:\\src\\docs\\a.md", true),
            ],
        );
        assert_eq!(
            linked(&links, "run `dist\\folio.exe` to test", None),
            [(
                "dist\\folio.exe",
                String::from("file:///D:/src/dist/folio.exe")
            )],
        );
        assert_eq!(
            linked(&links, "see `docs/a.md`.", None),
            [("docs/a.md", String::from("file:///D:/src/docs/a.md"))],
        );
    }

    /// §7.1.5k ①: the gate stands **in front of** the ledger, so a suspect span is not even a
    /// question — and an old `yes` for the truncated prefix cannot light it up (scenario 67).
    #[test]
    fn an_edge_suspect_reference_is_neither_asked_about_nor_lit_by_an_old_yes() {
        let links = ledger(
            "D:\\case",
            &[("D:\\WINDOWS\\system", true), ("D:\\case\\src", true)],
        );
        let line = "D:\\WINDOWS\\system";
        assert_eq!(linked(&links, line, last_cell_of(line)), []);

        let fresh = ledger("D:\\case", &[]);
        let mut unknown = BTreeSet::new();
        let unseen = "D:\\case\\deep\\name.rs";
        assert!(
            fresh
                .links_in(unseen, last_cell_of(unseen), &mut unknown)
                .is_empty()
        );
        assert!(
            unknown.is_empty(),
            "a span the gate has pressed down is not worth a probe: {unknown:?}"
        );
    }

    /// The rejoin of two physical lines, as the five gates answer it.
    fn rejoin<'a>(
        links: &PrintedPathLinks,
        upper: &'a str,
        lower: &'a str,
    ) -> Option<(&'a str, &'a str, String)> {
        let mut unknown = BTreeSet::new();
        links
            .rejoin_across_newline(upper, last_cell_of(upper)?, lower, &mut unknown)
            .map(|joined| {
                (
                    &upper[joined.upper.byte_start..joined.upper.byte_end],
                    &lower[joined.lower.byte_start..joined.lower.byte_end],
                    joined.uri,
                )
            })
    }

    /// §7.1.5k ②, the one shape the five gates let through (scenario 57): the upper half reaches
    /// its row's last cell, the lower half starts at column 0, the two spell **exactly one**
    /// reference, the disk holds it, and neither half is a verified path on its own.
    #[test]
    fn two_physical_lines_rejoin_only_when_all_five_gates_pass() {
        let links = ledger("D:\\case", &[("D:\\case\\very\\long\\path\\file.rs", true)]);
        assert_eq!(
            rejoin(&links, "D:\\case\\very\\long\\pa", "th\\file.rs:12:3"),
            Some((
                "D:\\case\\very\\long\\pa",
                "th\\file.rs:12:3",
                "file:///D:/case/very/long/path/file.rs#L12C3".to_owned()
            ))
        );
    }

    /// §7.1.5k ② gate ⑤ and its whole reason for existing (scenarios 56 and 59): **a verified
    /// upper half stops the rejoin**, and the truncation gate then stops the upper half from being
    /// drawn on its own. The end of case A is an honest blank, not a link.
    #[test]
    fn a_verified_upper_half_refuses_the_rejoin_and_is_not_drawn_either() {
        let links = ledger(
            "D:\\case",
            &[
                ("D:\\WINDOWS\\system", true),
                (
                    "D:\\WINDOWS\\system32\\WindowsPowerShell\\v1.0\\Modules",
                    true,
                ),
                ("D:\\case\\src", true),
                ("D:\\case\\src\\main.rs", true),
            ],
        );
        let upper = "D:\\WINDOWS\\system";
        assert_eq!(
            rejoin(&links, upper, "32\\WindowsPowerShell\\v1.0\\Modules"),
            None,
            "scenario 56: the disk holding both halves is not evidence of what was meant"
        );
        assert_eq!(linked(&links, upper, last_cell_of(upper)), []);
        assert_eq!(
            rejoin(&links, "D:\\case\\src", "\\main.rs"),
            None,
            "scenario 59: two peer lines that happen to spell a real file are still two lines"
        );
    }

    /// §7.1.5k ② gates ② and ⑤ from the other side (scenarios 58 and 60): a lower half that does
    /// not start at column 0 is not a continuation, and a lower half that is already a verified
    /// reference of its own is left to be that reference.
    #[test]
    fn an_indented_or_already_verified_lower_half_refuses_the_rejoin() {
        let links = ledger(
            "D:\\case",
            &[
                ("D:\\case\\very\\long\\path\\file.rs", true),
                // Scenario 60's fixture, sharpened so gate ⑤ is the gate that answers: the joined
                // target exists **too**, so only "the lower half is already a reference" can refuse.
                ("D:\\case\\prefix\\main.rs", true),
                ("D:\\case\\fix\\main.rs", true),
            ],
        );
        assert_eq!(
            rejoin(&links, "    D:\\case\\very\\long\\pa", "    th\\file.rs"),
            None,
            "scenario 58: a shared indent is what peer lines look like, not what a wrap looks like"
        );
        assert_eq!(
            rejoin(&links, "D:\\case\\pre", "fix/main.rs"),
            None,
            "scenario 60: the lower half already names a file, so no third target is invented"
        );
        // And that lower half keeps its own ordinary link.
        assert_eq!(
            linked(&links, "fix/main.rs", None),
            [("fix/main.rs", "file:///D:/case/fix/main.rs".to_owned())]
        );
    }

    /// The three rows of the user's 2026-08-25 `[file]` chip, taken apart into the two things the
    /// application's layout puts around the path: a left **gutter** in front of every row, and a
    /// size **column** behind the ones that have room for it.
    const CHIP_HEAD: &str = "C:\\Users\\Alice\\AppData\\Local\\Temp\\claude\\D--Developer-Bett";
    const CHIP_TAIL: &str =
        "erTerminal\\ccea9546-63d0-4a20-ba77-75caa4e8533c\\scratchpad\\folio-pdf-test.pdf";
    const CHIP_TARGET: &str = "C:\\Users\\Alice\\AppData\\Local\\Temp\\claude\\\
        D--Developer-BetterTerminal\\ccea9546-63d0-4a20-ba77-75caa4e8533c\\scratchpad\\\
        folio-pdf-test.pdf";
    const CHIP_URI: &str = "file:///C:/Users/Alice/AppData/Local/Temp/claude/\
        D--Developer-BetterTerminal/ccea9546-63d0-4a20-ba77-75caa4e8533c/scratchpad/\
        folio-pdf-test.pdf";

    /// PIN (user report 2026-08-25, case A's inferred twin) — **an application that lays a path
    /// into a column of its own and prints another column after it infers nothing at all.**
    ///
    /// The picture is Claude Code's `[file]` chip: a 58-column path column with a left gutter
    /// (`>`, `[file]`) in front of every row and the size column's ink (`(58.2K`, `B)`) behind the
    /// ones with room for it. When the application *declares* the target with OSC 8 this is one
    /// link and bt-viewport rejoins it on the label; with **no** declaration the joined text is
    /// nobody's statement of anything and the rows must stay blank.
    ///
    /// Three separate gates keep them blank, and the case list below is arranged so that each one
    /// answers alone. The ledger holds **only the joined target**, so gate ④ says yes to the right
    /// name and gate ⑤ has nothing to object to.
    ///
    /// * **gate ①** — with a space in front of it the size column ends the upper token, so the
    ///   candidate stops short of the row's last cell: the application had room and chose to stop,
    ///   which is not a cut. MUTATION: drop the `touches_line_end` filter and this one links,
    ///   because the two fragments do spell the target exactly.
    /// * **gate ③** — with no space the size column is *inside* the token (`(` opens a bracket
    ///   pair and nothing closes it), and what the two rows then spell is not one candidate but
    ///   two: `…-Bett(58.2KerTerminal…` and, opening after the bracket, `58.2KerTerminal…`. "One
    ///   candidate covering all of it" is exactly the rule that refuses a promise laid over text
    ///   it did not read, and it refuses **in front of the disk** — the case below proves it by
    ///   asserting that no question was asked at all.
    /// * **gate ②** — the lower fragment opens at the gutter's width and not at visual column 0,
    ///   which is what a peer row looks like rather than what a wrap looks like.
    ///
    /// The last case is the **positive control** — strip both pieces of the application's layout
    /// and the very same text rejoins into the very same file — without which the refusals could
    /// equally be a lexer that never links anything.
    #[test]
    fn an_application_column_layout_infers_no_reference_from_its_fragments() {
        // The gutter is written in ASCII so that one cell is one byte; what is being pinned is
        // that ink stands in front of the fragment, not which glyph the vendor chose for it.
        let gutter_and_size = format!(">      {CHIP_HEAD} (58.2K");
        let gutter_only = format!("[file] {CHIP_TAIL}");
        let spaced_size = format!("{CHIP_HEAD} (58.2K");
        let joined_size = format!("{CHIP_HEAD}(58.2K");
        let links = ledger("D:\\case", &[(CHIP_TARGET, true)]);

        assert_eq!(
            rejoin(&links, &gutter_and_size, &gutter_only),
            None,
            "the picture as printed: no gate is satisfied"
        );
        assert_eq!(
            rejoin(&links, &spaced_size, CHIP_TAIL),
            None,
            "gate ①: the size column stands behind the upper candidate, so it never reached the \
             row's end"
        );

        let mut asked = BTreeSet::new();
        assert_eq!(
            links.rejoin_across_newline(
                &joined_size,
                last_cell_of(&joined_size).unwrap(),
                CHIP_TAIL,
                &mut asked
            ),
            None,
            "gate ③: an unclosed bracket keeps the size column inside the token, and the joined \
             text then spells two candidates rather than one covering all of it"
        );
        assert!(
            asked.is_empty(),
            "and it refused in front of the disk, so no name was even asked about: {asked:?}"
        );

        assert_eq!(
            rejoin(&links, CHIP_HEAD, &gutter_only),
            None,
            "gate ②: the lower fragment opens at the gutter's width, which is what a peer row \
             looks like and not what a wrap looks like"
        );
        assert_eq!(
            rejoin(&links, CHIP_HEAD, CHIP_TAIL),
            Some((CHIP_HEAD, CHIP_TAIL, CHIP_URI.to_owned())),
            "the positive control, and the URI this exact path folds into — a doubled hyphen in a \
             directory name is ordinary path text and is escaped by nothing"
        );
    }

    /// PIN (user report 2026-08-25, case C) — boundary table row 27 met in the wild: the same path,
    /// cut by the application over two rows that share an indent, is **not** rejoined even when the
    /// disk holds the joined target.
    ///
    /// Recorded separately from scenario 58's synthetic pair because this is the shape a reader
    /// will photograph and ask about: a real, existing file, spelled correctly across two rows, and
    /// a deliberate blank underneath it. The ledger holds the joined target, so gate ④ would say
    /// yes and gate ② is the gate that answers — the refusal is the design's answer, not a gap.
    #[test]
    fn a_real_path_cut_over_two_equally_indented_rows_stays_two_rows() {
        let upper = format!("  {CHIP_HEAD}");
        let lower = format!("  {CHIP_TAIL}");
        let links = ledger("D:\\case", &[(CHIP_TARGET, true)]);
        assert_eq!(rejoin(&links, &upper, &lower), None);
    }

    /// §7.1.5k ② gate ③, written as the precise assertion the ruling asks for: the two halves must
    /// spell **exactly one** candidate covering all of the joined text. Anything the lexer would
    /// quietly drop — a second reference, a tail of prose — is a refusal.
    #[test]
    fn a_rejoin_that_does_not_spell_exactly_one_reference_is_refused() {
        let links = ledger(
            "D:\\case",
            &[
                ("D:\\case\\very\\long\\path\\file.rs", true),
                ("D:\\case\\a", true),
            ],
        );
        assert_eq!(
            rejoin(
                &links,
                "D:\\case\\very\\long\\pa",
                "th\\file.rs，然后见 D:\\case\\a"
            ),
            None,
            "the joined token carries prose the lexer would release, so it is not one reference"
        );
        assert_eq!(
            rejoin(&links, "see D:\\case\\very\\long\\pa", "th\\file.rs"),
            Some((
                "D:\\case\\very\\long\\pa",
                "th\\file.rs",
                "file:///D:/case/very/long/path/file.rs".to_owned()
            )),
            "prose in front of the upper candidate is not part of the join"
        );
    }

    /// §7.1.5k ② gate ④: the rejoin is verified by the **disk target the lexer split out**, and an
    /// unanswered one is a question rather than a link — the same discipline a single-line
    /// reference lives under.
    #[test]
    fn a_rejoined_reference_asks_about_the_file_and_not_the_printed_string() {
        let links = ledger("D:\\case", &[]);
        let mut unknown = BTreeSet::new();
        let upper = "D:\\case\\very\\long\\pa";
        assert_eq!(
            links.rejoin_across_newline(
                upper,
                last_cell_of(upper).unwrap(),
                "th\\file.rs:12:3",
                &mut unknown
            ),
            None
        );
        assert_eq!(
            unknown,
            BTreeSet::from([PathBuf::from("D:\\case\\very\\long\\path\\file.rs")]),
            "the line number is never part of what is asked about"
        );
    }

    /// §7.1.5k ③ (scenarios 48, 49, 51, 73): a semicolon is a legal character in a Windows
    /// filename, so it is split on **only** where a field has said the value is a search path —
    /// and then per segment, with the semicolon itself never drawn.
    #[test]
    fn a_semicolon_is_split_on_only_inside_a_declared_path_list() {
        let links = ledger(
            "D:\\case",
            &[
                ("D:\\mods\\A", true),
                ("D:\\mods\\B", true),
                ("D:\\Program Files\\Mods", true),
                ("D:\\case\\real", true),
                ("D:\\a", true),
            ],
        );
        // Scenario 49: one quoted value, two segments, and neither the quotes nor the `;` drawn.
        assert_eq!(
            linked(&links, "PSModulePath=\"D:\\mods\\A;D:\\mods\\B\"", None),
            [
                ("D:\\mods\\A", "file:///D:/mods/A".to_owned()),
                ("D:\\mods\\B", "file:///D:/mods/B".to_owned()),
            ]
        );
        // Scenario 51: the quoting is per segment, not "the first pair of quotes is the value".
        assert_eq!(
            linked(
                &links,
                "PSModulePath=\"D:\\Program Files\\Mods\";D:\\mods\\B",
                None
            ),
            [
                (
                    "D:\\Program Files\\Mods",
                    "file:///D:/Program%20Files/Mods".to_owned()
                ),
                ("D:\\mods\\B", "file:///D:/mods/B".to_owned()),
            ]
        );
        // Scenario 48: no field, no list. The whole quoted string is one name, and it is not on
        // the disk, so the segment that happens to be is **not** offered in its place.
        assert_eq!(
            linked(&links, "\"D:\\case\\real;D:\\case\\missing\"", None),
            []
        );
        // Scenario 52: a field that is not a search path, holding an address whose own syntax uses
        // a semicolon. Neither the list mode nor the relative scan may reach inside it.
        assert_eq!(
            linked_on_a_full_disk(
                Some("D:\\case"),
                "ARG=\"https://host.invalid/a;b?next=docs/a.md\""
            ),
            []
        );
        // Scenario 73: the same, for a single file whose name really carries a semicolon.
        assert_eq!(linked(&links, "\"D:\\a;b\\c.md\"", None), []);
    }

    /// §7.1.5k ③, the segments the first version refuses (scenario 50): an empty segment means
    /// "the current directory" to Windows, a `%VAR%` segment is the *printing* process's
    /// environment, and a relative segment is measured from a directory this window was never told
    /// about. Only a literal drive-rooted segment resolves to one file on its own.
    #[test]
    fn a_path_list_links_only_its_literal_drive_rooted_segments() {
        let links = ledger(
            "D:\\case",
            &[
                ("D:\\bin", true),
                ("D:\\case", true),
                ("D:\\case\\tools", true),
                ("C:\\Windows\\System32", true),
            ],
        );
        assert_eq!(
            linked(
                &links,
                "PATH=D:\\bin;;%SystemRoot%\\System32;.\\tools",
                None
            ),
            [("D:\\bin", "file:///D:/bin".to_owned())]
        );
        // The label is matched by bytes from the `=` backwards, so a field written in another
        // script lands mid-character. Reading it must be a refusal and not a panic.
        assert_eq!(
            linked(&links, "路径=D:\\bin;D:\\case", None),
            [],
            "no list context, so the value is one name that carries a semicolon — and that name is \
             not on the disk, so neither half of it is offered in its place"
        );
        assert_eq!(linked(&links, "P=D:\\bin", None).len(), 1);
    }

    /// Every link one line offers **on a disk that says yes to everything it is asked**.
    ///
    /// It is the harshest fixture in the matrix and the one most of these rows run under: a refusal
    /// that only holds because the dangerous prefix happened not to exist is not a refusal, it is
    /// luck, and this makes luck impossible. Two passes — the first collects the questions, the
    /// second answers them all and reads the links back.
    fn linked_on_a_full_disk<'line>(
        working_directory: Option<&str>,
        line: &'line str,
    ) -> Vec<(&'line str, String)> {
        let base = working_directory.map(PathBuf::from);
        let mut unknown = BTreeSet::new();
        PrintedPathLinks::new(base.clone(), BTreeMap::new()).links_in(line, None, &mut unknown);
        let links =
            PrintedPathLinks::new(base, unknown.into_iter().map(|path| (path, true)).collect());
        linked(&links, line, None)
    }

    /// Group A of the scenario list, rows 5 to 15 — quoting, provider prefixes and the position
    /// syntaxes a compiler, a linter, a traceback and `grep -n` print.
    ///
    /// Every one of them runs on a disk that says yes to everything it is asked, so a row that
    /// comes back empty came back empty on the strength of the **lexer** and not of a fixture. The
    /// rows the current contract does not spell out — MSVC's `(12,34)`, .NET's `:line 42`, `grep`'s
    /// `path:line:text` — are pinned as "nothing", which is the scenario list's own requirement:
    /// until the explicit syntax exists, the honest answer is no link at all rather than a link
    /// over some prefix of it.
    #[test]
    fn group_a_quoting_provider_prefixes_and_position_syntax() {
        let cwd = Some("D:\\case");
        // 5 — ASCII quotes declare the extent, and neither quote nor comma is inside it.
        assert_eq!(
            linked_on_a_full_disk(cwd, "File \"D:\\Program Files\\app.py\", line 3, in f"),
            [(
                "D:\\Program Files\\app.py",
                "file:///D:/Program%20Files/app.py".to_owned()
            )]
        );
        // 6 — a serialized JSON value is not a path literal, and one round of unescaping is a guess.
        assert_eq!(
            linked_on_a_full_disk(cwd, "{\"file\":\"D:\\\\case\\\\src\\\\main.rs\"}"),
            []
        );
        // 7, 8, 9, 10 — the position syntaxes this contract does not spell out. Their fixture is
        // the scenario list's own: **the named file exists**, and the answer is still no link,
        // because a link over `…main.cpp(12,34` would point at a name nobody printed.
        let named = ledger(
            "D:\\case",
            &[
                ("D:\\case\\src\\main.cpp", true),
                ("D:\\case\\src\\app.ts", true),
                ("D:\\case\\Foo.cs", true),
                ("D:\\case\\src\\main.rs", true),
            ],
        );
        for line in [
            "at Foo in D:\\case\\Foo.cs:line 42",
            "src/main.rs:12:let x = 1",
        ] {
            assert_eq!(
                linked(&named, line, None),
                [],
                "{line} has no settled position syntax, so it has no link at all"
            );
        }
        // 7 and 8 as the opening-bracket seam leaves them (§7.30, user ruling 2026-08-28 evening).
        // The bracket is a seam whatever follows it, so the reading in front of it is **the path
        // itself** — which is what these two rows always wanted ("只画路径"), short of the
        // `(12,34)` fragment no contract spells out yet. What the scenario forbade — a link over
        // `…main.cpp(12,34`, a name nobody printed — is still not offered by anything.
        let with_position_syntax = ledger(
            "D:\\case",
            &[
                ("D:\\case\\src\\main.cpp", true),
                ("D:\\case\\src\\main.cpp(12,34", false),
                ("D:\\case\\src\\app.ts", true),
            ],
        );
        assert_eq!(
            linked(
                &with_position_syntax,
                "D:\\case\\src\\main.cpp(12,34): error C2143",
                None
            ),
            [(
                "D:\\case\\src\\main.cpp",
                "file:///D:/case/src/main.cpp".to_owned()
            )]
        );
        assert_eq!(
            linked(
                &with_position_syntax,
                "src/app.ts(7,19): error TS2322",
                None
            ),
            [("src/app.ts", "file:///D:/case/src/app.ts".to_owned())]
        );
        // And the absolute spelling's first frame is the ordinary two-frame rhythm, not a promise:
        // the whole printed string is a name of its own until the disk has denied it.
        let mut unknown = BTreeSet::new();
        assert_eq!(
            named.links_in(
                "D:\\case\\src\\main.cpp(12,34): error C2143",
                None,
                &mut unknown
            ),
            [],
            "the shorter reading waits for the longer one's verdict"
        );
        // 11, 12 — the two position shapes that *are* unambiguous today.
        assert_eq!(
            linked_on_a_full_disk(cwd, "--> src/main.rs:12:5"),
            [(
                "src/main.rs:12:5",
                "file:///D:/case/src/main.rs#L12C5".to_owned()
            )]
        );
        assert_eq!(
            linked_on_a_full_disk(cwd, "at f (D:\\case\\app.js:10:2)"),
            [(
                "D:\\case\\app.js:10:2",
                "file:///D:/case/app.js#L10C2".to_owned()
            )]
        );
        // 13 — a PowerShell provider qualifier is not part of the target.
        assert_eq!(
            linked_on_a_full_disk(
                cwd,
                "Microsoft.PowerShell.Core\\FileSystem::D:\\case\\a.ps1:12"
            ),
            [("D:\\case\\a.ps1:12", "file:///D:/case/a.ps1#L12".to_owned())]
        );
        // 14, 15 — a non-filesystem provider is not a disk path, and its tail is not a relative one.
        assert_eq!(linked_on_a_full_disk(cwd, "HKLM:\\Software\\Vendor"), []);
        assert_eq!(
            linked_on_a_full_disk(cwd, "Cert:\\CurrentUser\\My\\ABCDEF"),
            []
        );
    }

    /// Group A rows 3 and 4: an opening quote of **any** script declares an extent, so a candidate
    /// that stopped at a space *inside* one has not reached the end of what was quoted.
    ///
    /// The gate is about the space, not about the quote character: `“D:\x\a.md”` stops at the
    /// closing quote itself and is an ordinary, complete reference, which is why the second half of
    /// this row is asserted beside the first.
    #[test]
    fn a_candidate_cut_by_a_space_inside_an_opening_quote_is_not_drawn() {
        // The scenario list's fixture: the dangerous prefix and the whole script both exist.
        let links = ledger(
            "D:\\case",
            &[
                ("D:\\Program", true),
                ("D:\\Program Files\\Tool\\run.ps1", true),
                ("D:\\x\\a.md", true),
            ],
        );
        assert_eq!(
            linked(&links, "'D:\\Program Files\\Tool\\run.ps1'", None),
            []
        );
        assert_eq!(
            linked(&links, "“D:\\Program Files\\Tool\\run.ps1”", None),
            []
        );
        assert_eq!(
            linked(&links, "“D:\\x\\a.md”", None),
            [("D:\\x\\a.md", "file:///D:/x/a.md".to_owned())],
            "a quoted reference that reaches its own closing quote was never cut"
        );
    }

    /// Group B of the scenario list (16 to 26): **one span, one owner.** A scheme's text belongs to
    /// the scheme, so no second, differently-rooted link is ever built out of a query parameter.
    #[test]
    fn group_b_urls_file_uris_and_span_ownership() {
        let cwd = Some("D:\\case");
        // 18, 19 — the two shapes that used to grow a nested file link inside an address.
        assert_eq!(
            linked_on_a_full_disk(cwd, "https://host:8080/open?next=docs/a.md#L2"),
            []
        );
        assert_eq!(
            linked_on_a_full_disk(cwd, "http://localhost:3000/?file=D:\\case\\a.txt"),
            []
        );
        // 21, 22 — local file URIs, decoded exactly once.
        assert_eq!(
            linked_on_a_full_disk(cwd, "file:///D:/case/a%20b.txt"),
            [(
                "file:///D:/case/a%20b.txt",
                "file:///D:/case/a%20b.txt".to_owned()
            )]
        );
        assert_eq!(
            linked_on_a_full_disk(cwd, "file://localhost/D:/case/a.txt"),
            [(
                "file://localhost/D:/case/a.txt",
                "file:///D:/case/a.txt".to_owned()
            )]
        );
        // 23, 24 — somebody else's machine, and a root this one has no letter for. Neither may
        // decay into the relative reference hiding inside it.
        assert_eq!(linked_on_a_full_disk(cwd, "file://server/share/a.txt"), []);
        assert_eq!(linked_on_a_full_disk(cwd, "file:///etc/passwd"), []);
        // 25 — one round of percent decoding, so `%2520` is a literal `%20` in a filename.
        assert_eq!(
            linked_on_a_full_disk(cwd, "file:///D:/case/a%2520b.txt")
                .into_iter()
                .map(|(_, uri)| uri)
                .collect::<Vec<_>>(),
            ["file:///D:/case/a%2520b.txt".to_owned()],
            "the decoded name carries a literal %20, and folding it back escapes the percent"
        );
        // 26 — a query and a printed position on one URI have no settled order of resolution, so
        // the reference is read whole, asked about whole, and never quietly rewritten into the
        // file beside it. Fixture: `D:\case\a.txt` is on the disk and the welded name is not.
        assert_eq!(
            linked(
                &ledger("D:\\case", &[("D:\\case\\a.txt", true)]),
                "file:///D:/case/a.txt:12?raw=1",
                None
            ),
            []
        );
    }

    /// Group B rows 16, 17 and 20, which are about the **address** rather than about a file: an
    /// address owns its port, its query and its fragment, and a bracket it opened itself.
    #[test]
    fn group_b_addresses_keep_their_own_punctuation() {
        for line in [
            "https://host.invalid:8080/a/b?q=x%2Fy#frag",
            "https://[2001:db8::1]:8443/a?q=b",
        ] {
            assert_eq!(
                inferred_url_ranges(line, None)
                    .into_iter()
                    .map(|range| &line[range.byte_start..range.byte_end])
                    .collect::<Vec<_>>(),
                [line]
            );
        }
        let line = "see (https://host.invalid/a_(b)?x=(c)).";
        assert_eq!(
            inferred_url_ranges(line, None)
                .into_iter()
                .map(|range| &line[range.byte_start..range.byte_end])
                .collect::<Vec<_>>(),
            ["https://host.invalid/a_(b)?x=(c)"],
            "the brackets the address opened are its own; the prose's last one is not"
        );
    }

    /// Group C of the scenario list (27 to 41): variable expansions, UNC, the extended namespace,
    /// drive-relative paths, case folding, and the two Windows corners where a name means more than
    /// one object.
    #[test]
    fn group_c_variables_unc_long_paths_and_windows_corners() {
        let cwd = Some("D:\\case");
        for line in [
            // 27 to 31 — an expansion is the printing process's, never this window's.
            "$HOME/docs/a.md",
            "${HOME}/docs/a.md",
            "%USERPROFILE%\\docs\\a.md",
            "$env:USERPROFILE\\docs\\a.md",
            "~\\docs\\a.md",
            // 32 — `C:foo` is relative to C:'s own current directory, which nobody reported.
            "C:foo\\bar.txt",
            // 33 to 36 — UNC, the extended namespace and device objects, none of which may be
            // reached by stripping a prefix off the front.
            "\\\\server\\share\\dir\\a.txt",
            "\\\\?\\D:\\case\\a.txt",
            "\\\\?\\UNC\\server\\share\\a.txt",
            "\\\\.\\PIPE\\build-daemon",
            // 39 — a trailing space under the extended namespace is a different real name.
            "\"\\\\?\\D:\\case\\a.txt \"",
            // 41 — a DOS device name is not a file, whatever metadata says.
            "D:\\case\\CON",
            "D:\\case\\NUL.txt",
        ] {
            assert_eq!(
                linked_on_a_full_disk(cwd, line),
                [],
                "{line} names nothing this window may promise"
            );
        }
        // 37 — case folding belongs to the filesystem, not to the lexer: the printed spelling is
        // the span and the printed spelling is the target.
        assert_eq!(
            linked_on_a_full_disk(cwd, "d:\\CASE\\SRC\\MAIN.rs"),
            [(
                "d:\\CASE\\SRC\\MAIN.rs",
                "file:///d:/CASE/SRC/MAIN.rs".to_owned()
            )]
        );
        // 38 — the accepted Win32 compatibility: a trailing ASCII dot names the same file, so it
        // stays inside the span (boundary table row 16).
        assert_eq!(
            linked_on_a_full_disk(cwd, "D:\\case\\a.txt."),
            [("D:\\case\\a.txt.", "file:///D:/case/a.txt.".to_owned())]
        );
        // 40 — a positive integer behind a colon is a **position**, fixed by contract, so a
        // machine that happens to hold an alternate data stream named `12` cannot change it.
        assert_eq!(
            linked_on_a_full_disk(cwd, "D:\\case\\a.txt:12"),
            [("D:\\case\\a.txt:12", "file:///D:/case/a.txt#L12".to_owned())]
        );
    }

    /// Group D of the scenario list (42 to 47): a synthesized name is not a filename. `a/` and `b/`
    /// are git's diff namespace, `{old => new}` is its rename compression, `\346\226\207` is its
    /// quoted-path encoding, and `webpack://` and `node:` name modules rather than files.
    #[test]
    fn group_d_git_and_virtual_schemes() {
        let cwd = Some("D:\\case");
        // 44 keeps its "no link at all" across the opening-bracket seam (§7.30, user ruling
        // 2026-08-28 evening). The brace does seam, but the reading in front of it is `src/` — a
        // directory prefix, and a reading must end on a character that is part of a name. See
        // `a_reading_that_ends_on_a_separator_is_not_a_name`.
        for line in [
            "--- a/src/main.rs",
            "+++ b/src/main.rs",
            "src/{old => new}/main.rs",
            "\"a/\\346\\226\\207.txt\"",
            "webpack://app/src/main.ts:7:2",
            "node:internal/modules/cjs/loader:123:4",
        ] {
            assert_eq!(
                linked_on_a_full_disk(cwd, line),
                [],
                "{line} is a synthesized name and not this working tree's"
            );
        }
    }

    /// Group G of the scenario list (68 to 73) — **the boundary table's new placement axis.**
    ///
    /// Not one of the 23 rows changes a word. What changes is that each of them is now asked twice:
    /// once with the reference inside the row, where it answers exactly what it always answered,
    /// and once with the reference's last cell **being** the row's last cell, where §7.1.5k ①
    /// presses it down. The rows that keep their answer at both placements keep it because their
    /// candidate never reached the end — a closing bracket, a full-width stop or a closing quote
    /// stands there instead, and that is evidence of a complete reference rather than of a cut one.
    #[test]
    fn group_g_every_boundary_table_row_is_asked_at_both_placements() {
        let links = ledger(
            "D:\\case",
            &[
                ("D:\\Developer\\folio-terminal\\README.md", true),
                ("D:\\Developer\\folio-terminal\\test.md", true),
                ("D:\\case\\docs\\HANDOFF-2026-08-20.md", true),
                ("D:\\case\\test.md", true),
                ("D:\\x\\a.md", true),
                ("D:\\x\\a.md.", true),
                ("D:\\case\\docs\\a.md", true),
                ("C:\\12", true),
                ("D:\\case\\docs\\plans\\x\\plan.md", true),
                ("D:\\a b\\c.md", true),
            ],
        );
        // Rows 1, 2, 10, 11, 13, 15, 16, 18, 20 and 23: the reference runs to the end of the line,
        // so at the edge it is pressed down and inside the row it is untouched.
        for line in [
            "D:\\Developer\\folio-terminal\\README.md",
            "D:/Developer/folio-terminal/README.md",
            "docs/HANDOFF-2026-08-20.md",
            "./test.md",
            "file:///D:/Developer/folio-terminal/test.md",
            "file://localhost/D:/x/a.md",
            "见 D:\\x\\a.md.",
            "docs/a.md:13",
            "C:\\12",
            "  docs/plans/x/plan.md",
        ] {
            assert_eq!(
                linked(&links, line, None).len(),
                1,
                "{line} is one link inside the row"
            );
            assert_eq!(
                linked(&links, line, last_cell_of(line)),
                [],
                "{line} is pressed down when its own last cell is the row's"
            );
        }
        // Row 12 is the same ruling for an address (scenario 69). Its printed host has one label,
        // which this window never offered, so the row is asked at a host it would.
        let row_12 = "https://host.invalid:8080/img/x.png";
        assert_eq!(inferred_url_ranges(row_12, None).len(), 1);
        assert_eq!(inferred_url_ranges(row_12, last_cell_of(row_12)), []);
        // Rows 3, 5, 6, 17 and 22 (scenarios 70, 71, 72): the last cell is a delimiter the prose
        // owns, so the candidate never reached it and both placements answer alike.
        for line in [
            "\"D:\\a b\\c.md\"",
            "(D:\\x\\a.md)",
            "（D:\\x\\a.md）",
            "见 D:\\x\\a.md。",
            "方案写完（docs/plans/x/plan.md）。",
        ] {
            assert_eq!(
                linked(&links, line, None),
                linked(&links, line, last_cell_of(line)),
                "{line} answers alike at both placements"
            );
            assert_eq!(linked(&links, line, last_cell_of(line)).len(), 1);
        }
        // Rows 4, 8, 9, 14, 19 and 21 had no link at either placement and still have none: a gate
        // that presses candidates down cannot turn "nothing" into one.
        for line in [
            "D:\\a b\\c.md",
            "中文D:\\x\\a.md",
            "README",
            "file://server/share/a.md",
            "见 D:\\x\\a.md，然后",
            "docs/a.md:abc",
        ] {
            assert_eq!(linked(&links, line, None), []);
            assert_eq!(linked(&links, line, last_cell_of(line)), []);
        }
    }

    /// Scenario 1 and 2 — **a conflict, recorded rather than worked around.**
    ///
    /// `At D:\Program Files\Tool\run.ps1:12 char:3` cuts at the space and leaves `D:\Program`,
    /// which on a great many machines is a real directory; the same shape comes out of `npm ERR!
    /// path …`. The scenario list wants no link at all, and this window still draws one.
    ///
    /// It is not fixed here because every lexical rule that would fix it contradicts a ruling this
    /// module already carries. Boundary table row 4 settles that an unquoted space ends a token and
    /// that what follows it is a reference of its own, so "a candidate followed by a space and more
    /// path-shaped text is suspect" would darken every `ls`-style line that prints two real paths
    /// side by side. The discriminating fact is semantic and not lexical — `D:\Program` is a
    /// *directory* and the reference continues into it — and reading it means probing the longer
    /// candidates as well, which is disk work this slice is explicitly not allowed to add
    /// (§7.1.5j's probe budget). Rows 3 and 4 of the same group *are* fixed, because an opening
    /// quote is a declaration of extent and gives the evidence a bare space cannot.
    #[test]
    #[ignore = "§7.1.5k conflict: an unquoted space-cut prefix needs either a rule that contradicts \
                boundary table row 4 or extra disk probes the budget forbids; see the doc comment"]
    fn an_unquoted_path_cut_at_a_space_does_not_link_its_prefix() {
        let links = ledger(
            "D:\\case",
            &[
                ("D:\\Program", true),
                ("D:\\Program Files\\Tool\\run.ps1", true),
                ("D:\\Program Files\\nodejs\\node_modules\\x", true),
            ],
        );
        assert_eq!(
            linked(
                &links,
                "At D:\\Program Files\\Tool\\run.ps1:12 char:3",
                None
            ),
            []
        );
        assert_eq!(
            linked(
                &links,
                "npm ERR! path D:\\Program Files\\nodejs\\node_modules\\x",
                None
            ),
            []
        );
    }

    /// Scenario 64 — **a conflict, recorded rather than worked around.**
    ///
    /// A relative reference printed while the shell stood in `D:\repoA` must keep naming
    /// `D:\repoA\src\main.rs` after the shell moves to `D:\repoB`, even though the old line is
    /// still on the screen. [`PrintedPathLinks`] holds **one** working directory — the last one
    /// reported — and every line on the screen is measured from it, so the old line re-points.
    /// Carrying a resolution base per *line* means the transcript recording the directory each line
    /// was printed under, which is a change to what a line is and not to how one is read.
    #[test]
    #[ignore = "§7.1.5k conflict: a per-line resolution base does not exist yet; the ledger holds \
                one working directory for the whole screen"]
    fn a_scrollback_relative_reference_keeps_the_directory_it_was_printed_under() {
        let printed_under_a = ledger("D:\\repoA", &[("D:\\repoA\\src\\main.rs", true)]);
        let moved_to_b = ledger(
            "D:\\repoB",
            &[
                ("D:\\repoA\\src\\main.rs", true),
                ("D:\\repoB\\src\\main.rs", true),
            ],
        );
        assert_eq!(
            linked(&printed_under_a, "src/main.rs", None),
            linked(&moved_to_b, "src/main.rs", None),
            "the old line names the file it named when it was printed"
        );
    }

    /// Scenario 65 — **settled, and no longer a conflict** (user ruling 2026-08-25).
    ///
    /// A name the disk denied is not asked about again *while the command that named it is still
    /// running*, which is what keeps a repainting screen's bounded question budget for the names
    /// that need it (§7.1.5j, user report 2026-08-23, and
    /// `a_path_the_disk_has_denied_is_never_asked_about_again` directly above). The scenario list
    /// asked for the opposite: a file that did not exist at first print and is built afterwards
    /// should become a link. Both are now true, because the ruling gave the "no" the one expiry
    /// that is a fact rather than a guess — **the pane's next `OSC 133 D`**. A command has ended,
    /// so the disk may have moved; and a command is not a frame, so the budget is untouched. See
    /// `bt_term::DualPlaneSession::expire_denied_paths`, which is where the expiry lives: this
    /// layer's contract is unchanged, it answers whichever ledger it is handed.
    ///
    /// **The conflict was not hypothetical, and 2026-08-25 is the day it was photographed.** The
    /// reader reported that `D:\Developer\folio-terminal\test-assets\folio-pdf-test.pdf` — an
    /// indented, unadorned, drive-rooted path to a file that exists — carried no underline at all.
    /// Reproduced in a fresh pane the same line lights on the first frame, so nothing lexical or
    /// geometric was refusing it. What refused it was this arm: minutes earlier the same screen had
    /// printed that exact path inside the command that was about to *create* the file
    /// (`--print-to-pdf="D:\…\test-assets\folio-pdf-test.pdf"`), the pass asked the disk about a
    /// name that was not there yet, and the `no` outlived the file's arrival. Any application that
    /// prints where it is about to write — a build, an installer, a `cp`, an agent's own shell line
    /// — produces this, so the shape is common rather than exotic.
    ///
    /// The three states, in the order the photograph went through them.
    #[test]
    fn a_file_built_after_its_first_mention_can_still_become_a_link() {
        let named = "generated/out.js";
        let full = PathBuf::from("D:\\case\\generated\\out.js");

        // ① While the command runs: the disk has said no, and it is neither drawn nor re-asked.
        let during = ledger("D:\\case", &[("D:\\case\\generated\\out.js", false)]);
        let mut unknown = BTreeSet::new();
        assert!(during.links_in(named, None, &mut unknown).is_empty());
        assert!(
            unknown.is_empty(),
            "a standing verdict is not re-asked frame by frame — the 2026-08-23 budget ruling, \
             untouched"
        );

        // ② The command ends. `expire_denied_paths` takes the denial off the books, so the ledger
        //    this layer is handed no longer holds it, and the second sighting asks again.
        let after = ledger("D:\\case", &[]);
        let mut unknown = BTreeSet::new();
        assert!(after.links_in(named, None, &mut unknown).is_empty());
        assert_eq!(
            unknown,
            BTreeSet::from([full.clone()]),
            "a sighting after the command that could have created it is worth one question"
        );

        // ③ And the answer this time is yes.
        let built = ledger("D:\\case", &[("D:\\case\\generated\\out.js", true)]);
        let mut unknown = BTreeSet::new();
        assert_eq!(
            built
                .links_in(named, None, &mut unknown)
                .iter()
                .map(|(_, uri)| uri.as_str())
                .collect::<Vec<_>>(),
            ["file:///D:/case/generated/out.js"],
            "the file that was built is a link"
        );
        assert!(unknown.is_empty());
    }

    /// §7.1.5k ①, the widest half of the ruling: the gate covers an inferred **URL** as well, or
    /// the most dangerous shape of all — a cut address that resolves to somebody else's host —
    /// survives the hardening untouched (scenario 61, boundary table row 12 under `placement`).
    #[test]
    fn an_inferred_url_that_reaches_the_last_visual_cell_is_not_drawn() {
        // Scenario 61 verbatim. Its single-label host was never an address this window offers, so
        // the row below carries the same shape at a host that is.
        assert_eq!(inferred_url_ranges("https://host:8080/api/us", None), []);
        let line = "https://host.invalid:8080/api/us";
        assert_eq!(
            inferred_url_ranges(line, None)
                .into_iter()
                .map(|range| &line[range.byte_start..range.byte_end])
                .collect::<Vec<_>>(),
            [line]
        );
        assert_eq!(
            inferred_url_ranges(line, last_cell_of(line)),
            [],
            "an address cut at the row's end is a working link to the wrong place"
        );
        // The rest of scenario 61: the lower half is judged on its own terms, and on its own terms
        // it is not an address at all. Nothing rejoins the two — an address has no witness on this
        // machine's disk, so there is no fourth gate for it to pass.
        assert_eq!(inferred_url_ranges("ers?id=7", None), []);
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
        let found = links.links_in(
            "real.md ./real.md ./gone.md D:\\src\\real.md",
            None,
            &mut unknown,
        );
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
        let found = links.links_in("./real.md ./gone.md ./fresh.md", None, &mut unknown);
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
                    found += links.links_in(line, None, &mut unknown).len();
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

    /// §7.1.5k combination row 34: a **relative** reference carries a `:line[:col]` exactly as a
    /// drive-rooted one does, in every spelling of "relative" there is.
    ///
    /// It has done so since §7.1.5j ⑨, and the reason is worth reading off the code rather than
    /// off a memory of it: `is_relative_reference`'s "no colon" refusal is asked of the **path**,
    /// which [`split_printed_location`] has already cut the location off of. So `docs/a.md:12:3`
    /// reaches that refusal as `docs/a.md` and passes, while `docs/a.md:abc` reaches it whole and
    /// is refused exactly as the row above says. What is pinned here is that every anchoring gets
    /// there — bare, `./`, `..\`, backslashed — and that each of them survives the prose that
    /// opens a candidate around it.
    #[test]
    fn a_located_relative_reference_is_read_like_a_drive_rooted_one() {
        let links = ledger("D:\\case", &[("D:\\case\\docs\\a.md", true)]);
        for (line, span, uri) in [
            (
                "docs/a.md:12:3",
                "docs/a.md:12:3",
                "file:///D:/case/docs/a.md#L12C3",
            ),
            (
                "docs\\a.md:12",
                "docs\\a.md:12",
                "file:///D:/case/docs/a.md#L12",
            ),
            (
                "./docs/a.md:12:3",
                "./docs/a.md:12:3",
                "file:///D:/case/docs/a.md#L12C3",
            ),
            (
                "..\\case\\docs\\a.md:12",
                "..\\case\\docs\\a.md:12",
                "file:///D:/case/docs/a.md#L12",
            ),
            (
                "见 docs/a.md:12:3。",
                "docs/a.md:12:3",
                "file:///D:/case/docs/a.md#L12C3",
            ),
            (
                "(docs/a.md:12:3)",
                "docs/a.md:12:3",
                "file:///D:/case/docs/a.md#L12C3",
            ),
            (
                "\"docs/a.md:12:3\"",
                "docs/a.md:12:3",
                "file:///D:/case/docs/a.md#L12C3",
            ),
        ] {
            assert_eq!(
                linked(&links, line, None),
                [(span, uri.to_owned())],
                "{line}"
            );
        }
        // The disk is asked about the file and never about the printed string — the same
        // discipline the drive-rooted spelling lives under, asserted on the spelling that has to
        // be joined to a directory first.
        let fresh = ledger("D:\\case", &[]);
        let mut unknown = BTreeSet::new();
        assert!(
            fresh
                .links_in("docs/a.md:12:3", None, &mut unknown)
                .is_empty()
        );
        assert_eq!(
            unknown,
            BTreeSet::from([PathBuf::from("D:\\case\\docs\\a.md")])
        );
    }

    /// §7.1.5k ① over a located relative reference (combination row 35): the gate reads the
    /// **whole** reference, so a cut line number is as suspect as a cut name.
    ///
    /// `docs/a.md:12:3` ending the row may be all of `…:12:34`, and the file's own name is
    /// complete either way — so the disk would answer yes, which is precisely the licence the gate
    /// refuses. One cell of prose behind it is enough to make it an ordinary link again.
    #[test]
    fn the_truncation_gate_reads_a_located_relative_reference_to_the_end_of_its_line() {
        let links = ledger("D:\\case", &[("D:\\case\\docs\\a.md", true)]);
        let line = "docs/a.md:12:3";
        let mut unknown = BTreeSet::new();
        assert!(
            links
                .links_in(line, last_cell_of(line), &mut unknown)
                .is_empty()
        );
        assert!(
            unknown.is_empty(),
            "a span the gate has pressed down is not worth a probe: {unknown:?}"
        );
        let inside = "docs/a.md:12:3 x";
        assert_eq!(
            linked(&links, inside, last_cell_of(inside)),
            [(
                "docs/a.md:12:3",
                "file:///D:/case/docs/a.md#L12C3".to_owned()
            )]
        );
    }

    /// §7.1.5k ② over a relative reference (combination rows 36 and 37).
    ///
    /// Row 36 is the ordinary rejoin with the location riding along, and its receipt has to name
    /// the directory the halves were measured from: a relative rejoin is only true of the working
    /// directory it was joined to, and the next pass rebuilds it against whatever that is then.
    ///
    /// Row 37 is the shape only a located reference can have — **the cut fell inside the line
    /// number**. Gate ⑤ answers it: the upper half is already a verified reference of its own
    /// (`docs/a.md` at line 1), so no third target is invented; gate ① then keeps that upper half
    /// from being drawn as the line-1 reference it is not. A blank is the honest end of it, the
    /// same end §7.1.5k ② gives `D:\WINDOWS\system`.
    #[test]
    fn two_physical_lines_rejoin_a_relative_reference_and_its_line_rides_along() {
        let links = ledger(
            "D:\\case",
            &[
                ("D:\\case\\docs\\a.md", true),
                ("D:\\case\\very\\long\\path\\file.rs", true),
            ],
        );
        let upper = "very/long/pa";
        assert_eq!(
            rejoin(&links, upper, "th/file.rs:12:3"),
            Some((
                upper,
                "th/file.rs:12:3",
                "file:///D:/case/very/long/path/file.rs#L12C3".to_owned()
            ))
        );
        let mut unknown = BTreeSet::new();
        let joined = links
            .rejoin_across_newline(
                upper,
                last_cell_of(upper).unwrap(),
                "th/file.rs:12:3",
                &mut unknown,
            )
            .expect("the five gates pass on the relative spelling too");
        assert_eq!(
            joined.target,
            PathBuf::from("D:\\case\\very\\long\\path\\file.rs")
        );
        assert_eq!(joined.resolution_base, Some(PathBuf::from("D:\\case")));

        let cut_number = "docs/a.md:1";
        assert_eq!(
            rejoin(&links, cut_number, "2:3"),
            None,
            "row 37: the upper half is a verified reference of its own, so gate ⑤ refuses it"
        );
        assert_eq!(
            linked(&links, cut_number, last_cell_of(cut_number)),
            [],
            "and the half that refused the rejoin is not drawn as the line-1 reference it is not"
        );
    }

    /// §7.1.5k ③ over a located relative segment (combination row 38): a declared search path owns
    /// its whole value, and a `:12` behind a relative segment does not buy that segment the base
    /// it never had.
    #[test]
    fn a_declared_search_path_owns_a_located_relative_segment_too() {
        assert_eq!(
            linked_on_a_full_disk(Some("D:\\case"), "PATH=docs/a.md:12;D:\\bin"),
            [("D:\\bin", "file:///D:/bin".to_owned())]
        );
    }

    /// §7.1.5k ④ over a located relative reference (combination row 39): those refusals are about
    /// **who owns the text**, so a line number behind the name changes none of them.
    ///
    /// Every row runs on the disk that says yes to everything it is asked, so no refusal here is
    /// luck — each one is the rule doing the refusing.
    #[test]
    fn the_ownership_refusals_hold_over_a_located_relative_reference() {
        for line in [
            "$HOME/docs/a.md:12",
            "--- a/docs/a.md:12",
            "+++ b/docs/a.md:12",
            "docs//a.md:12",
            "docs/NUL:12",
            "http://localhost:3000/?file=docs/a.md:12",
        ] {
            assert_eq!(linked_on_a_full_disk(Some("D:\\case"), line), [], "{line}");
        }
    }

    /// The direction this whole table leans, asked of the shape a line number makes possible:
    /// **a number behind a colon is not a reference**. `12:30` is a time and `3:2` is a score, and
    /// neither carries the one mark a bare relative reference must carry — a separator. Nothing
    /// about locations loosened that, because the separator rule is asked of the path.
    ///
    /// The neighbouring shape that *does* carry a separator is written down beside it rather than
    /// left to be discovered: `2026/08/23:14` is a candidate, and it always was one without the
    /// `:14`. **The witness is what refuses it**, and a name the disk has denied stays denied
    /// whether or not a line number was printed behind it.
    #[test]
    fn a_number_behind_a_colon_in_prose_is_not_a_reference() {
        for line in ["时间 12:30", "比分 3:2", "Elapsed 1:23:45", "-j12:30"] {
            assert!(spans(line).is_empty(), "{line}");
        }
        assert_eq!(
            located("2026/08/23:14"),
            [(
                "2026/08/23",
                Some(PrintedPathLocation {
                    line: 14,
                    column: None
                })
            )]
        );
        let denied = ledger("D:\\case", &[("D:\\case\\2026\\08\\23", false)]);
        assert_eq!(linked(&denied, "2026/08/23:14", None), []);
    }

    /// Boundary table row 20 read from the relative side, in both directions — the ambiguity a
    /// located reference is owed an explicit judge for.
    ///
    /// A drive letter is a colon with a **separator behind it** (`is_drive_prefix_at` demands one
    /// at the third byte); a line number is a colon with a **decimal positive integer behind it**
    /// and nothing else. No colon can be both, so nothing has to choose between them — and no two
    /// scans can claim one reference.
    #[test]
    fn a_drive_letter_and_a_line_number_cannot_be_the_same_colon() {
        // A drive-rooted path buried inside a token belongs to nobody: the relative reading
        // refuses it for the colon it still carries, and the absolute one cannot open behind a `/`.
        assert!(spans("docs/C:\\a.md").is_empty());
        // One reference, claimed once — by the spelling whose colon has a separator behind it.
        assert_eq!(
            candidates("see D:\\case\\docs\\a.md:12"),
            [("D:\\case\\docs\\a.md:12", PrintedPathSpelling::Absolute)]
        );
        assert_eq!(
            located("see D:\\case\\docs\\a.md:12"),
            [(
                "D:\\case\\docs\\a.md",
                Some(PrintedPathLocation {
                    line: 12,
                    column: None
                })
            )]
        );
        // And the mirror: a single letter that is *not* a drive, because nothing separates it from
        // the number, names a file called `D` at line 12.
        assert_eq!(
            located("sub\\D:12"),
            [(
                "sub\\D",
                Some(PrintedPathLocation {
                    line: 12,
                    column: None
                })
            )]
        );
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

    /// §7.30, boundary table rows 40 and 41 — the seam, read at the lexer: one ASCII separator with
    /// a word of another script glued to it is where a name may have stopped, so the token offers a
    /// shorter form as well as its whole self.
    #[test]
    fn an_ascii_separator_glued_to_a_cjk_word_offers_a_shorter_form() {
        // The user's line, 2026-08-27. The whole token carries a `,`, which is not a path
        // character, so it is not even a bare candidate — the seam is the only reason this line
        // offers anything at all.
        assert_eq!(
            spans("docs/plans/release/clean-vm.md,这里是操作顺序"),
            ["docs/plans/release/clean-vm.md"]
        );
        // A drive-rooted token *is* a candidate whole, so both forms are offered, longest first.
        assert_eq!(
            spans("见 D:\\x\\a.md,然后"),
            ["D:\\x\\a.md,然后", "D:\\x\\a.md"]
        );
        // The whole group, one character class and not a list of stops.
        for line in [
            "docs/a.md,这里",
            "docs/a.md;然后",
            "docs/a.md:然后",
            "docs/a.md!然后",
            "docs/a.md?然后",
        ] {
            assert_eq!(spans(line), ["docs/a.md"], "{line} seams at its separator");
        }
    }

    /// §7.30, boundary table rows 46–48 (user ruling 2026-08-28): the seam is a **class**, so every
    /// ASCII punctuation mark a path is not spelled with cuts — a bracket every bit as much as a
    /// comma. This is the fifth screenshot: `docs/a.md(说明)` and its backslash twin both went dark
    /// because `(` was not on the old five-mark table.
    #[test]
    fn a_bracket_glued_to_a_cjk_word_seams_like_any_other_separator() {
        // Row 46: an opening bracket then CJK — the shorter form is offered. The bare spelling is
        // no candidate whole (`(` is not a path character), so the seam is the only reading there is.
        assert_eq!(spans("docs/a.md(说明)"), ["docs/a.md"]);
        assert_eq!(spans("dist\\folio.exe(说明)"), ["dist\\folio.exe"]); // the backslash twin seams too
        // The drive-rooted form is a candidate whole (a `(` is legal in an absolute path), so both
        // readings are offered, longest first, and the disk chooses.
        assert_eq!(
            spans("见 D:\\x\\a.md(说明)"),
            ["D:\\x\\a.md(说明", "D:\\x\\a.md"]
        );
        // Row 47: other brackets and operators are the same class, no table to extend.
        assert_eq!(spans("docs/a.md[注]"), ["docs/a.md"]);
        assert_eq!(
            spans("见 D:\\x\\a.md{批}"),
            ["D:\\x\\a.md{批", "D:\\x\\a.md"]
        );
        assert_eq!(
            spans("见 D:\\x\\a.md=值"),
            ["D:\\x\\a.md=值", "D:\\x\\a.md"]
        );
        // Row 48 as it stands after the evening ruling of the same day (see
        // `an_opening_bracket_is_a_seam_whatever_follows_it`): an opening bracket seams whatever
        // follows it, so `docs/a.md(1).txt` **does** offer the shorter reading — it is offered, not
        // promised, and the disk still decides. What has not changed is that `(` is a seam and not
        // a terminator, which is what leaves a name that really carries a bracket askable at all.
        assert_eq!(spans("docs/a.md(1).txt"), ["docs/a.md"]);
        assert_eq!(
            spans("见 D:\\x\\a.md(1"),
            ["D:\\x\\a.md(1", "D:\\x\\a.md"],
            "the whole name is still the first reading offered"
        );
    }

    /// §7.30, boundary table rows 50 and 51 (user ruling 2026-08-28, on next16): an **ASCII
    /// opening bracket** is a seam on its own account, **whatever follows it** — the one place in
    /// this ruling where the character after the mark is not consulted.
    ///
    /// The line that proved it: `dist\folio-next16.exe(0.1.0 (84d843f47e))`, where the byte behind
    /// the bracket is an ASCII `0`. The transition rule saw no transition, the token ate its way to
    /// the closing `)`, and a file that is really on the disk went unmarked. A bracket does not open
    /// a filename in the wild — it opens an aside about the thing just named — so it offers a
    /// shorter reading, and the disk still decides between the readings, longest first.
    #[test]
    fn an_opening_bracket_is_a_seam_whatever_follows_it() {
        // Row 50, the user's line. The bare spelling is no candidate whole (`(` is not a path
        // character), so the seam is the only reading there is — and it is a real file.
        assert_eq!(
            spans("dist\\folio-next16.exe(0.1.0 (84d843f47e))"),
            ["dist\\folio-next16.exe"]
        );
        // The drive-rooted spelling *is* a candidate whole, so both readings are offered, longest
        // first, exactly as a comma's seam offers them.
        assert_eq!(
            spans("见 D:\\dist\\x.exe(0.1.0"),
            ["D:\\dist\\x.exe(0.1.0", "D:\\dist\\x.exe"]
        );
        // All four openers, and an ASCII digit, letter and space-less word behind each of them: the
        // class is "the opening half of the bracket pairs whose closing half already ends a token".
        for line in ["docs/a.md(1", "docs/a.md[2", "docs/a.md{v3", "docs/a.md<x"] {
            assert_eq!(spans(line), ["docs/a.md"], "{line} seams at its bracket");
        }
        // Row 51 at the disk, all three frames. Frame one: neither reading has an answer, so
        // nothing is promised and both are asked — the shorter one may not be drawn ahead of the
        // longer one's verdict.
        let mut unknown = BTreeSet::new();
        let asking = ledger("D:\\case", &[]);
        assert_eq!(
            asking.links_in("见 D:\\x\\a.exe(0.1.0", None, &mut unknown),
            []
        );
        assert_eq!(
            unknown.into_iter().collect::<Vec<_>>(),
            [
                PathBuf::from("D:\\x\\a.exe"),
                PathBuf::from("D:\\x\\a.exe(0.1.0")
            ]
        );
        // Frame two, the ordinary answer: the printed string is nobody's name, the name in front of
        // the bracket is.
        let denied = ledger(
            "D:\\case",
            &[("D:\\x\\a.exe", true), ("D:\\x\\a.exe(0.1.0", false)],
        );
        assert_eq!(
            linked(&denied, "见 D:\\x\\a.exe(0.1.0", None),
            [("D:\\x\\a.exe", "file:///D:/x/a.exe".to_owned())]
        );
        // Frame two, the other answer: a name that really carries a bracket wins whole, which is
        // why the bracket is a seam and not a terminator.
        let whole = ledger("D:\\case", &[("D:\\x\\a(1", true), ("D:\\x\\a", true)]);
        assert_eq!(
            linked(&whole, "见 D:\\x\\a(1", None),
            [("D:\\x\\a(1", "file:///D:/x/a%281".to_owned())]
        );
    }

    /// §7.30, boundary table row 54 (user report 2026-09-03, on next29): **a colon another script
    /// stands in front of has made nothing absolute or schemed**, so the name behind it opens like
    /// any other bare reference.
    ///
    /// The line that proved it is an agent's, printed full-screen into a pane standing in the
    /// directory it names: `早就在:dist\\folio-next29.exe(73.9MB,…)`. Every other rule on this
    /// line already worked — the opening bracket seams (row 50), the bold run is not part of the
    /// text a line is recognized from — and the reference still went dark, because
    /// [`bare_candidate_opens_at`] refuses any opening a colon stands in front of.
    ///
    /// That refusal is the mirror of the seam and it was written with only one side of the
    /// transition read. A colon binds leftward to what it made absolute or schemed — a drive
    /// letter, a scheme, a host — and **every one of those is spelled in ASCII**. A colon with
    /// another script in front of it has made nothing: it is the sentence's punctuation, exactly as
    /// the separator in `docs/a.md,这里` is, and the only difference is which side of it the name
    /// is on.
    #[test]
    fn a_colon_behind_another_script_is_prose_and_not_a_scheme() {
        let cwd = Some("D:\\case");
        // The user's line, whole, with the half-width colon a Chinese writer types on an ASCII
        // keyboard. The bracket's seam is the only reading (`(` is not a path character), and the
        // colon in front of `dist` is the sentence's.
        let printed = "● 好了,早就在:dist\\folio-next29.exe(73.9MB,--version 报 0.1.1 (c11b72e1f0),CI 全绿)。直接开它验:";
        assert_eq!(spans(printed), ["dist\\folio-next29.exe"]);
        assert_eq!(
            linked_on_a_full_disk(cwd, printed),
            [(
                "dist\\folio-next29.exe",
                "file:///D:/case/dist/folio-next29.exe".to_owned()
            )]
        );
        // The full-width twin reads the same name: the colon's width was never the question, the
        // script of the character in front of it is.
        assert_eq!(
            spans("早就在：dist\\folio-next29.exe(73.9MB"),
            ["dist\\folio-next29.exe"]
        );
        // Every script, and the image scan that shares this lexer.
        for line in [
            "路径:local-images/日落.png",
            "パス:local-images/日落.png",
            "경로:local-images/日落.png",
        ] {
            assert_eq!(
                spans(line),
                ["local-images/日落.png"],
                "{line} opens behind its own sentence's colon"
            );
        }
        // What the colon still refuses, and why the ASCII half of the transition is load-bearing:
        // a scheme, a host and a port are ASCII, so a colon with ASCII in front of it binds
        // leftward exactly as it always did.
        assert_eq!(
            linked_on_a_full_disk(cwd, "https://host:8080/img/x.png"),
            []
        );
        assert_eq!(
            linked_on_a_full_disk(cwd, "node:internal/modules/cjs/loader:123:4"),
            []
        );
        assert_eq!(
            linked_on_a_full_disk(cwd, "webpack://app/src/main.ts:7:2"),
            []
        );
        // No witness is not a witness: a colon with nothing in front of it has proved nothing
        // about the script it belongs to, and this line's discipline is that a bare reference
        // opens on evidence rather than on the absence of it.
        assert!(spans(":8080/img/x.png").is_empty());
    }

    /// §7.30, boundary table row 52 (user ruling 2026-08-28, evening): **a reading must end on a
    /// character that is part of a name.** A trailing `/` or `\` is not; what stands in front of one
    /// is a directory prefix, and a directory prefix is a name nobody wrote — not even when the disk
    /// holds it, because existence was never the licence to invent a reference, only to draw one.
    ///
    /// This is the same discipline that keeps a single bare word out (`README` is prose until
    /// something says otherwise): a bare reference is admitted on the strength of carrying a
    /// separator, and a **trailing** separator is the one place where that mark is not evidence of
    /// two segments at all. So it closes a hole in the old rule rather than adding a new one, and it
    /// is asked of every reading — the seam's shorter forms and the whole token alike.
    #[test]
    fn a_reading_that_ends_on_a_separator_is_not_a_name() {
        let cwd = Some("D:\\case");
        // The line that settled it: git's rename compression, where the opening brace seams and the
        // reading in front of it is a directory prefix. Scenario 44 keeps its "no link at all".
        assert_eq!(linked_on_a_full_disk(cwd, "src/{old => new}/main.rs"), []);
        // The same shape without any seam: a lone directory prefix is refused at the lexer, so it
        // never reaches the disk. `docs` alone was already out; `docs/` is out for the same reason.
        assert!(spans("docs/").is_empty());
        assert!(spans("cd docs/").is_empty());
        assert!(spans("./").is_empty(), "an anchor alone names no file");
        assert_eq!(linked_on_a_full_disk(cwd, "cd docs/"), []);
        // What the rule must not touch: a reading that ends on a name still stands, seam or no seam.
        assert_eq!(spans("docs/a.md(1"), ["docs/a.md"]);
        assert_eq!(
            linked_on_a_full_disk(cwd, "cd docs/a.md"),
            [("docs/a.md", "file:///D:/case/docs/a.md".to_owned())]
        );
    }

    /// §7.30 coexisting with §7.1.5k ② (user note 2026-08-28): an agent cut `dist\folio-next15.exe`
    /// across its **own** newline and glued prose to the tail of the lower half — `5.exe(反斜杠)`.
    /// The rejoin cuts that seam off the continuation first, joins the name alone, and the link
    /// covers `5.exe` on the lower line and nothing of the sentence behind it.
    #[test]
    fn a_wrapped_lower_half_that_carries_a_seam_rejoins_the_name_alone() {
        let links = ledger("D:\\src", &[("D:\\src\\dist\\folio-next15.exe", true)]);
        assert_eq!(
            rejoin(&links, "dist\\folio-next1", "5.exe(反斜杠)"),
            Some((
                "dist\\folio-next1",
                "5.exe",
                "file:///D:/src/dist/folio-next15.exe".to_owned()
            ))
        );
    }

    /// The crash of 2026-08-27: `names_a_dos_device` sliced a four-byte stem at byte three to
    /// compare it with `COM`/`LPT`, and a stem of one ASCII byte and one three-byte character —
    /// `*⑥`, as an agent numbers its own list — is four bytes with no boundary at three. The
    /// window panicked on the line being printed, and the session restore printed it again on
    /// every start. The device question must be answerable for **any** spelling, so it is asked
    /// of every four-byte shape that is not a device, and of the two that are.
    #[test]
    fn a_four_byte_stem_that_is_not_ascii_is_not_a_device_and_not_a_panic() {
        for not_a_device in ["*⑥", "D:\\case\\*⑥", "docs\\⑥*", "D:\\case\\⑥.txt", "COM⑥"]
        {
            assert!(
                !names_a_dos_device(not_a_device),
                "{not_a_device} is not a device name and must not panic"
            );
        }
        assert!(names_a_dos_device("D:\\case\\COM1"));
        assert!(names_a_dos_device("lpt9.txt"));
        assert!(!names_a_dos_device("COM0"));
    }

    /// §7.30, boundary table rows 42 and 43 — the two transitions that are **not** seams, kept
    /// beside the one that is: ASCII to ASCII is a filename's own punctuation (row 16's discipline
    /// on a comma), and a non-ASCII stop is released whole by row 17 and cuts nothing.
    #[test]
    fn a_seam_is_one_character_class_transition_and_not_a_list_of_stops() {
        // Row 42: `,b` is as much a name as `.md` is, so nothing is cut and the token stands whole.
        assert_eq!(spans("见 D:\\x\\a.md,b"), ["D:\\x\\a.md,b"]);
        // The bare spelling of the same text offers nothing at all, exactly as it did before this
        // slice: a comma is not a path character, so the run rule refuses the opening.
        assert!(spans("docs/a.md,b").is_empty());
        // Row 43 is boundary table row 19 unmoved: a full-width comma is not an ASCII separator,
        // so the sentence behind it stays welded to the token and the line offers no link.
        assert_eq!(spans("见 D:\\x\\a.md，然后"), ["D:\\x\\a.md，然后"]);
    }

    /// §7.30, boundary table row 40 at the disk: the shorter form is a link the moment the disk
    /// holds it, and the underline covers exactly that form.
    #[test]
    fn the_form_a_seam_cuts_is_the_link_and_the_underline_is_its_own() {
        // The user's line as it was printed, closing bracket and colon and all.
        let printed = ledger(
            "D:\\case",
            &[("D:\\case\\docs\\plans\\release\\clean-vm.md", true)],
        );
        assert_eq!(
            linked(
                &printed,
                "docs/plans/release/clean-vm.md,这里是操作顺序):",
                None
            ),
            [(
                "docs/plans/release/clean-vm.md",
                "file:///D:/case/docs/plans/release/clean-vm.md".to_owned()
            )]
        );
        let links = ledger("D:\\case", &[("D:\\case\\docs\\a.md", true)]);
        for line in [
            "见 docs/plans/../a.md,这里是操作顺序",
            "见 docs/a.md;然后",
            "见 docs/a.md:然后",
        ] {
            let drawn = linked(&links, line, None);
            assert_eq!(drawn.len(), 1, "{line} draws one link");
            assert_eq!(
                drawn[0].1, "file:///D:/case/docs/a.md",
                "{line} points at the file the seam cut out"
            );
        }
    }

    /// §7.30, boundary table row 44 — **longest first, and the whole string wins when the disk
    /// holds it.** A filename may really carry `,中文`, so the shorter form is only reached once
    /// the disk has denied the longer one, and never before an answer for it exists.
    #[test]
    fn a_seam_is_asked_longest_first_and_a_name_that_really_carries_one_wins() {
        // Frame one: nobody has looked at either form, so nothing is promised and both are asked.
        let mut unknown = BTreeSet::new();
        let asking = ledger("D:\\case", &[]);
        assert_eq!(
            asking.links_in("见 D:\\x\\a.md,然后", None, &mut unknown),
            []
        );
        assert_eq!(
            unknown.into_iter().collect::<Vec<_>>(),
            [
                PathBuf::from("D:\\x\\a.md"),
                PathBuf::from("D:\\x\\a.md,然后")
            ]
        );
        // Frame two, the ordinary answer: the long name is not there and the short one is.
        let denied = ledger(
            "D:\\case",
            &[("D:\\x\\a.md", true), ("D:\\x\\a.md,然后", false)],
        );
        assert_eq!(
            linked(&denied, "见 D:\\x\\a.md,然后", None),
            [("D:\\x\\a.md", "file:///D:/x/a.md".to_owned())]
        );
        // Frame two, the other answer: the disk holds the whole string, so the whole string is the
        // reference and the seam never comes into it.
        let whole = ledger(
            "D:\\case",
            &[("D:\\x\\a.md", true), ("D:\\x\\a.md,然后", true)],
        );
        assert_eq!(
            linked(&whole, "见 D:\\x\\a.md,然后", None),
            [(
                "D:\\x\\a.md,然后",
                "file:///D:/x/a.md%2C%E7%84%B6%E5%90%8E".to_owned()
            )]
        );
        // And a shorter form that exists is still not drawn while a longer one is unanswered: the
        // frame that would draw it is the frame that has not been told yet.
        let half = ledger("D:\\case", &[("D:\\x\\a.md", true)]);
        assert_eq!(linked(&half, "见 D:\\x\\a.md,然后", None), []);
    }

    /// §7.30 and §7.1.5j ⑨ share one colon without fighting over it: `:` opens a seam only when
    /// what follows it is not a decimal line number, and a located reference keeps its location
    /// through the cut.
    #[test]
    fn a_seam_and_a_line_number_never_claim_the_same_colon() {
        let links = ledger("D:\\case", &[("D:\\case\\docs\\a.md", true)]);
        for (line, span, uri) in [
            (
                "docs/a.md:12",
                "docs/a.md:12",
                "file:///D:/case/docs/a.md#L12",
            ),
            ("docs/a.md:然后", "docs/a.md", "file:///D:/case/docs/a.md"),
            (
                "docs/a.md:12,这里",
                "docs/a.md:12",
                "file:///D:/case/docs/a.md#L12",
            ),
            (
                "docs/a.md:12:3,这里",
                "docs/a.md:12:3",
                "file:///D:/case/docs/a.md#L12C3",
            ),
            (
                "docs/a.md:12:这里",
                "docs/a.md:12",
                "file:///D:/case/docs/a.md#L12",
            ),
        ] {
            assert_eq!(
                linked(&links, line, None),
                [(span, uri.to_owned())],
                "{line} is read as {span}"
            );
        }
    }

    /// §7.30 under §7.1.5k ① — the gate is asked **per form**. The whole token reaches the row's
    /// last cell and is pressed down; the shorter form stops well short of it, and the seam's own
    /// punctuation standing between the two is the evidence that the name ended before the cut.
    #[test]
    fn the_truncation_gate_presses_the_whole_token_and_leaves_the_shorter_form_standing() {
        let links = ledger("D:\\case", &[("D:\\case\\docs\\a.md", true)]);
        let line = "docs/a.md,这里";
        assert_eq!(
            linked(&links, line, last_cell_of(line)),
            [("docs/a.md", "file:///D:/case/docs/a.md".to_owned())]
        );
        // The same name with nothing behind it does reach the last cell, and is pressed down.
        assert_eq!(linked(&links, "docs/a.md", last_cell_of("docs/a.md")), []);
    }

    /// §7.30's whole cost: one extra question, and only on a token that carries a seam.
    #[test]
    fn the_extra_question_a_seam_costs_is_asked_only_where_there_is_a_seam() {
        let asked = |line: &str| {
            let mut unknown = BTreeSet::new();
            ledger("D:\\case", &[]).links_in(line, None, &mut unknown);
            unknown.len()
        };
        assert_eq!(asked("docs/a.md"), 1);
        assert_eq!(asked("D:\\x\\a.md"), 1);
        assert_eq!(asked("D:\\x\\a.md,b"), 1);
        assert_eq!(asked("D:\\x\\a.md,然后"), 2);
        // The bare spelling costs nothing extra at all: the whole token was never a candidate.
        assert_eq!(asked("docs/a.md,这里"), 1);
    }

    /// §7.30 does not reach the two shapes that are ASCII by construction, **because they have
    /// carried the same ruling since they were written**: a URI token and a bare address both end
    /// at the first non-ASCII byte and then release the ASCII stop behind them. A native path
    /// cannot end that way — a filename is spelled in any script — which is the whole reason the
    /// seam had to be spelled out for it.
    #[test]
    fn a_uri_and_an_address_have_stopped_at_the_first_non_ascii_byte_since_they_were_written() {
        assert_eq!(spans("见 file:///D:/x/a.md,这里"), ["file:///D:/x/a.md"]);
        let line = "见 https://host.invalid/a,这里";
        assert_eq!(
            inferred_url_ranges(line, None)
                .into_iter()
                .map(|range| &line[range.byte_start..range.byte_end])
                .collect::<Vec<_>>(),
            ["https://host.invalid/a"]
        );
    }

    /// §7.1.5k ② gate ⑤ reads the seam too: a half that names a file **through one of its own
    /// forms** is that file's reference, and rejoining it would put a second promise over text the
    /// first one already covers.
    #[test]
    fn a_rejoin_is_refused_when_the_upper_half_is_verified_through_a_seam() {
        let links = ledger(
            "D:\\case",
            &[
                ("D:\\x\\a.md", true),
                ("D:\\x\\a.md,然后", false),
                ("D:\\x\\a.md,然后是", true),
            ],
        );
        assert_eq!(rejoin(&links, "D:\\x\\a.md,然后", "是"), None);
        // And the upper half keeps the link its own seam earned it.
        let upper = "D:\\x\\a.md,然后";
        assert_eq!(
            linked(&links, upper, last_cell_of(upper)),
            [("D:\\x\\a.md", "file:///D:/x/a.md".to_owned())]
        );
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
        let found = links.links_in(line, None, &mut unknown);
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
        let found = links.links_in(line, None, &mut unknown);
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
