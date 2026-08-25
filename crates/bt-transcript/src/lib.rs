//! Canonical frozen transcript and mutable staging primitives.

use std::{collections::VecDeque, num::NonZeroUsize};

use bitflags::bitflags;
use unicode_segmentation::UnicodeSegmentation;

pub mod paths;
pub mod search;

pub const DEFAULT_STAGING_QUOTA: NonZeroUsize = NonZeroUsize::new(4096).unwrap();
/// Spike-only value; M0 must replace it with a measured or configured quota.
pub const SPIKE_DEFAULT_FROZEN_QUOTA: NonZeroUsize = NonZeroUsize::new(100_000).unwrap();

/// How many bytes of pane memory one line of the reader's chosen capacity buys
/// (`docs/DESIGN.md` §1.3, §7.1.6g ③).
///
/// **The reader's unit stays lines.** `Scrollback` on the Terminal page counts
/// lines, `settings.json` stores lines, and nothing here adds a knob. This is the
/// engineering ceiling standing behind that answer: `frozen_quota × this` is the
/// most frozen history one pane may hold, and when a pane's lines are wide enough
/// that the ceiling arrives first, [`TranscriptStore`] evicts from the oldest end
/// through the same road a line-count overflow already takes.
///
/// **The number is the published ladder times a margin, not a taste.** §7.1.6g
/// costed one 80-column frozen line and read the ladder off it — 25,000 through
/// 200,000 lines is "about 14 MB to 112 MB a pane". This build measures that same
/// line at 632 bytes (`the_shape_of_a_frozen_line_is_measured_not_remembered`), so
/// 2,048 is **3.2x** the shape the ladder was costed on, and the whole ladder sits
/// under the ceiling it derives: what a pane may hold runs 48 MiB to 391 MiB while
/// what §7.1.6g promises runs 15 MB to 126 MB.
///
/// The margin is spent on the things a plain ASCII line has none of. Measured on
/// this build: a coloured, hyperlinked 80-column line costs 1,260 bytes, and the
/// heaviest 80-column shape there is — a new colour every fourth column, 20 style
/// runs — costs 2,000. So an ordinary pane still gets every line its capacity
/// promised, and the ceiling only overtakes the reader's number when the *average*
/// logical line passes ~363 columns of filled plain text. A history that averages
/// that for 100,000 consecutive lines is a flood, not a session.
///
/// **A `StyleSpan` that grows re-opens this number**, which is why the test
/// asserts the 2,000 rather than describing it: at 72 bytes a span, twenty of them
/// are most of the ceiling, and the arithmetic has to be re-derived rather than
/// remembered the next time that struct gains a field.
///
/// What it stops is the shape the ladder never costed: a program printing 4,000
/// characters to a line joins 50 physical rows into one logical line of 20,232
/// bytes, and 100,000 of those is 1.88 GiB in a single pane. Under this ceiling
/// the same flood settles at 195 MiB, roughly 10,100 lines deep.
pub const FROZEN_BYTES_PER_LINE: usize = 2048;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HyperlinkRange {
    pub byte_start: usize,
    pub byte_end: usize,
}

/// Recognize deliberately narrow bare web URLs without changing transcript source text.
///
/// Candidates must begin at a conservative prose boundary, use `http://` or `https://`, and
/// contain an unambiguous host. In particular, single-label hosts are rejected except for the
/// explicitly supported `localhost` development case. A candidate ends at the first byte that
/// cannot be part of an address ([`is_url_terminator`]), and trailing prose punctuation is then
/// released.
pub fn detect_http_urls(text: &str) -> Vec<HyperlinkRange> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let Some(scheme_len) = http_scheme_len(&bytes[cursor..]) else {
            cursor += 1;
            continue;
        };
        if cursor != 0 && !is_url_leading_boundary(bytes[cursor - 1]) {
            cursor += scheme_len;
            continue;
        }
        let mut end = cursor + scheme_len;
        while end < bytes.len() && !is_url_terminator(bytes[end]) {
            end += 1;
        }
        end = release_url_tail(&text[cursor..end]) + cursor;
        if bare_http_url_is_valid(&text[cursor..end], scheme_len) {
            ranges.push(HyperlinkRange {
                byte_start: cursor,
                byte_end: end,
            });
            cursor = end;
        } else {
            cursor += scheme_len;
        }
    }
    ranges
}

/// Where every `http(s)://` token on one line **starts and stops**, whether or not it turns out to
/// be an address this window would offer.
///
/// Ownership, not recognition (§7.1.5k ④). A scheme declares that the text behind it is addressed
/// the scheme's way, and that declaration stands even when the address is one we refuse: the
/// `D:\case\a.txt` inside `http://localhost:3000/?file=D:\case\a.txt` is a query parameter of
/// somebody else's server and is not this machine's file, so no path scan may claim it. Reading
/// that off [`detect_http_urls`] instead would be exactly wrong — that scan drops the address here,
/// which is precisely when the nested claim would appear.
#[must_use]
pub fn http_scheme_spans(text: &str) -> Vec<HyperlinkRange> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let Some(scheme_len) = http_scheme_len(&bytes[cursor..]) else {
            cursor += 1;
            continue;
        };
        if cursor != 0 && !is_url_leading_boundary(bytes[cursor - 1]) {
            cursor += scheme_len;
            continue;
        }
        let mut end = cursor + scheme_len;
        while end < bytes.len() && !is_url_terminator(bytes[end]) {
            end += 1;
        }
        spans.push(HyperlinkRange {
            byte_start: cursor,
            byte_end: end,
        });
        cursor = end.max(cursor + scheme_len);
    }
    spans
}

/// Release the prose an address swallowed at its end: sentence punctuation, and a closing bracket
/// **only when it closes one the address never opened**.
///
/// Stripping every trailing `)` is what turned `see (https://host/a_(b)?x=(c)).` into a link to
/// `https://host/a_(b)?x=(c` — a shorter address that works and goes somewhere else. Counting is
/// the whole fix: an address that opened a bracket may close it, and the one left over belongs to
/// the sentence.
fn release_url_tail(candidate: &str) -> usize {
    let bytes = candidate.as_bytes();
    let mut end = candidate.len();
    while end > 0 {
        match bytes[end - 1] {
            b'.' | b',' | b';' | b':' | b'!' | b'?' => end -= 1,
            b')' => {
                let opened = bytes[..end].iter().filter(|byte| **byte == b'(').count();
                let closed = bytes[..end].iter().filter(|byte| **byte == b')').count();
                if closed <= opened {
                    break;
                }
                end -= 1;
            }
            _ => break,
        }
    }
    end
}

fn http_scheme_len(bytes: &[u8]) -> Option<usize> {
    bytes
        .starts_with(b"http://")
        .then_some(7)
        .or_else(|| bytes.starts_with(b"https://").then_some(8))
}

fn is_url_leading_boundary(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'"' | b'\'' | b'(' | b'[' | b'{' | b'<')
}

/// Where a bare URL stops: at the first byte that cannot belong to one.
///
/// An address we are willing to offer is pure ASCII — [`bare_http_url_is_valid`] reads nothing
/// else, and a host it would accept cannot be spelled with anything else — so a byte belonging to
/// a UTF-8 sequence (any byte `>= 0x80`) ends the candidate exactly as a space does. Saying it
/// here rather than only at the validator is what keeps prose pressed against an address from
/// swallowing it: the scan reaches such a byte only from ASCII, so it is a lead byte and the range
/// always ends on a character boundary.
fn is_url_terminator(byte: u8) -> bool {
    !byte.is_ascii()
        || byte.is_ascii_whitespace()
        || matches!(byte, b'"' | b'\'' | b'`' | b'<' | b'>')
}

fn bare_http_url_is_valid(candidate: &str, scheme_len: usize) -> bool {
    if candidate
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte == b'\\')
    {
        return false;
    }
    let remainder = &candidate[scheme_len..];
    let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    let Some(host) = authority_host(authority) else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || valid_ipv4(host)
        || host.contains(':')
        || valid_dns_name(host)
}

fn authority_host(authority: &str) -> Option<&str> {
    if let Some(bracketed) = authority.strip_prefix('[') {
        let close = bracketed.find(']')?;
        let host = &bracketed[..close];
        let suffix = &bracketed[close + 1..];
        if host.is_empty()
            || host.parse::<std::net::Ipv6Addr>().is_err()
            || (!suffix.is_empty() && !valid_port(suffix.strip_prefix(':')?))
        {
            return None;
        }
        return Some(host);
    }
    let (host, port) = authority
        .rsplit_once(':')
        .map_or((authority, None), |(host, port)| (host, Some(port)));
    if host.is_empty() || port.is_some_and(|port| !valid_port(port)) {
        return None;
    }
    Some(host)
}

fn valid_port(port: &str) -> bool {
    !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|port| port != 0)
}

fn valid_ipv4(host: &str) -> bool {
    let mut parts = host.split('.');
    (0..4).all(|_| {
        parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.parse::<u8>().is_ok())
    }) && parts.next().is_none()
}

fn valid_dns_name(host: &str) -> bool {
    host.contains('.')
        && host.len() <= 253
        && host
            .rsplit('.')
            .next()
            .is_some_and(|label| label.bytes().any(|byte| byte.is_ascii_alphabetic()))
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
        })
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TranscriptId(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StagingId(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceGeneration(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GraphemeOffset(pub u32);

/// Stable transcript color vocabulary; no upstream discriminants cross this boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TerminalColor {
    Named(u8),
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// What [`TerminalColor::Indexed`] means for the 240 slots the *protocol* owns
/// rather than the palette.
///
/// The 256-colour space is two different kinds of thing wearing one numbering.
/// `0..16` are the scheme's sixteen — nothing here can answer them, and this
/// returns `None` so that a caller which has no palette cannot be handed a
/// plausible-looking wrong colour. `16..232` is xterm's 6x6x6 cube and
/// `232..256` its 24-step grey ramp, and those two are **constants of the
/// escape-code protocol**: every terminal on earth resolves index 196 to
/// `#ff0000`, no scheme gets a vote, and a window that answered `OSC 4;196;?`
/// with something else would simply be wrong.
///
/// It lives in the transcript crate because that is the one crate both readers
/// share. The renderer resolves an `Indexed` cell with it, and the terminal
/// answers `OSC 4;N;?` with it; before this existed the cube was written out
/// twice, which is one arithmetic slip away from a window that draws a colour
/// it does not admit to.
#[must_use]
pub fn indexed_cube_color(index: u8) -> Option<[u8; 3]> {
    if index < 16 {
        return None;
    }
    if index < 232 {
        let cube = index - 16;
        // 0, 95, 135, 175, 215, 255 - xterm's own five-step ramp, which is not
        // linear: the first step is 95 and every later one is 40.
        let component = |value: u8| if value == 0 { 0 } else { 55 + 40 * value };
        return Some([
            component(cube / 36),
            component((cube % 36) / 6),
            component(cube % 6),
        ]);
    }
    let grey = 8 + 10 * (index - 232);
    Some([grey, grey, grey])
}

bitflags! {
    /// Stable transcript style flags. Bit positions are owned by Folio.
    #[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
    pub struct CellFlags: u16 {
        const INVERSE = 1 << 0;
        const BOLD = 1 << 1;
        const ITALIC = 1 << 2;
        const UNDERLINE = 1 << 3;
        const DIM = 1 << 4;
        const HIDDEN = 1 << 5;
        const STRIKEOUT = 1 << 6;
        const DOUBLE_UNDERLINE = 1 << 7;
        const UNDERCURL = 1 << 8;
        const DOTTED_UNDERLINE = 1 << 9;
        const DASHED_UNDERLINE = 1 << 10;
        const WIDE_CHAR = 1 << 11;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CellStyle {
    pub flags: CellFlags,
    pub foreground: TerminalColor,
    pub background: TerminalColor,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            flags: CellFlags::empty(),
            // Named codes are Folio-owned; 16/17 mean default foreground/background.
            foreground: TerminalColor::Named(16),
            background: TerminalColor::Named(17),
        }
    }
}

/// A cell's hyperlink: the target `uri` plus the OSC 8 `id` grouping key. The id is what makes a
/// soft-wrapped multi-segment link one link (the vendor terminal synthesizes a per-emission id
/// when the application sends none), but it is presentation grouping metadata only: it changes on
/// every application repaint, so it MUST NOT participate in content identity. Equality and
/// hashing therefore cover the uri alone — content fingerprints, preservation's proven-source
/// exact equality, and shaped-row caches all stay byte-stable across repaints, exactly as when
/// only the uri was stored. Link grouping reads `.id` explicitly.
#[derive(Clone, Debug)]
pub struct CellHyperlink {
    pub id: Option<String>,
    pub uri: String,
}

impl CellHyperlink {
    /// An implicitly detected link (bare URL in transcript text): no OSC 8 id exists, so the
    /// link's extent is defined by cell contiguity.
    pub fn implicit(uri: impl Into<String>) -> Self {
        Self {
            id: None,
            uri: uri.into(),
        }
    }
}

impl PartialEq for CellHyperlink {
    fn eq(&self, other: &Self) -> bool {
        self.uri == other.uri
    }
}

impl Eq for CellHyperlink {}

impl std::hash::Hash for CellHyperlink {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.uri.hash(state);
    }
}

/// How many UTF-8 bytes a cell's text keeps without touching the heap.
///
/// A cell holds one grapheme cluster: a base character plus whatever zero-width
/// marks the terminal attached to it. Four bytes covers every single codepoint;
/// twenty-two covers a base character with several combining marks, a regional
/// indicator pair, and the ZWJ sequences that ordinary emoji are made of. It is
/// also the number that makes [`CellText`] exactly the size of the [`String`] it
/// replaced — twenty-four bytes — so the grids, rows and frames built out of
/// these are byte-for-byte the same size they were.
const CELL_TEXT_INLINE_BYTES: usize = 22;

#[derive(Clone, Debug)]
enum CellTextRepr {
    Inline {
        bytes: [u8; CELL_TEXT_INLINE_BYTES],
        len: u8,
    },
    /// A cluster longer than any terminal cell realistically holds. Kept
    /// because "realistically" is not "never" — a pathological ZWJ chain is
    /// still text somebody typed, and losing it would be a lie about the grid.
    Spilled(Box<str>),
}

/// One cell's text, stored inline.
///
/// **This was a `String`, and the `String` was the single largest cost in this
/// application.** A cell is one grapheme cluster — one to four bytes, nearly
/// always — and an eighty-by-thirty grid is 2,600 of them. Capturing the grid
/// therefore made 2,600 heap allocations, and the grid was captured several
/// times per published frame: sampling the main thread found it *allocator
/// bound*, with ntdll at 50% self time and `__rdl_dealloc` on three quarters of
/// all stacks.
///
/// Inline storage removes every one of those allocations without changing what
/// a cell is. It derefs to `str`, so everything that read a cell's text still
/// reads it the same way.
#[derive(Clone)]
pub struct CellText(CellTextRepr);

impl CellText {
    /// The empty cluster — a blank cell.
    pub const fn new() -> Self {
        Self(CellTextRepr::Inline {
            bytes: [0; CELL_TEXT_INLINE_BYTES],
            len: 0,
        })
    }

    /// Empty the cluster, back to inline storage.
    ///
    /// A blanked cell releases whatever it had spilled, because a cell that was
    /// cleared is a cell nobody is going to ask for the old bytes of.
    pub fn clear(&mut self) {
        self.0 = CellTextRepr::Inline {
            bytes: [0; CELL_TEXT_INLINE_BYTES],
            len: 0,
        };
    }

    /// How many bytes of *heap* this cluster is holding — zero for every cell
    /// that fits inline, which is very nearly all of them.
    ///
    /// The question the resident-bytes accounting is actually asking. A
    /// `String`'s `capacity()` answered it too, back when every cell had one.
    pub fn heap_bytes(&self) -> usize {
        match &self.0 {
            CellTextRepr::Inline { .. } => 0,
            CellTextRepr::Spilled(text) => text.len(),
        }
    }

    pub fn as_str(&self) -> &str {
        match &self.0 {
            CellTextRepr::Inline { bytes, len } => std::str::from_utf8(&bytes[..usize::from(*len)])
                .expect("a CellText's inline bytes are UTF-8 by construction"),
            CellTextRepr::Spilled(text) => text,
        }
    }

    /// Extend the cluster with one more character.
    ///
    /// The capture path's only mutation: a base character, then whatever
    /// zero-width marks the terminal hung on it. Spills to the heap at the
    /// first cluster that outgrows the inline room, and never comes back —
    /// a cell that long is not going to get shorter mid-frame.
    pub fn push(&mut self, character: char) {
        let mut encoded = [0_u8; 4];
        let encoded = character.encode_utf8(&mut encoded).as_bytes();
        match &mut self.0 {
            CellTextRepr::Inline { bytes, len } => {
                let end = usize::from(*len) + encoded.len();
                if end <= CELL_TEXT_INLINE_BYTES {
                    bytes[usize::from(*len)..end].copy_from_slice(encoded);
                    *len = end as u8;
                    return;
                }
                let mut spilled = String::with_capacity(end);
                spilled.push_str(
                    std::str::from_utf8(&bytes[..usize::from(*len)])
                        .expect("a CellText's inline bytes are UTF-8 by construction"),
                );
                spilled.push(character);
                self.0 = CellTextRepr::Spilled(spilled.into_boxed_str());
            }
            CellTextRepr::Spilled(text) => {
                let mut spilled = String::with_capacity(text.len() + encoded.len());
                spilled.push_str(text);
                spilled.push(character);
                *text = spilled.into_boxed_str();
            }
        }
    }

    /// Extend the cluster with a whole string — the reflow path's form of
    /// [`Self::push`], where a cluster arrives already assembled.
    pub fn push_str(&mut self, addition: &str) {
        if addition.is_empty() {
            return;
        }
        match &mut self.0 {
            CellTextRepr::Inline { bytes, len } => {
                let end = usize::from(*len) + addition.len();
                if end <= CELL_TEXT_INLINE_BYTES {
                    bytes[usize::from(*len)..end].copy_from_slice(addition.as_bytes());
                    *len = end as u8;
                    return;
                }
                let mut spilled = String::with_capacity(end);
                spilled.push_str(
                    std::str::from_utf8(&bytes[..usize::from(*len)])
                        .expect("a CellText's inline bytes are UTF-8 by construction"),
                );
                spilled.push_str(addition);
                self.0 = CellTextRepr::Spilled(spilled.into_boxed_str());
            }
            CellTextRepr::Spilled(text) => {
                let mut spilled = String::with_capacity(text.len() + addition.len());
                spilled.push_str(text);
                spilled.push_str(addition);
                *text = spilled.into_boxed_str();
            }
        }
    }
}

impl Default for CellText {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for CellText {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for CellText {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<&str> for CellText {
    fn from(text: &str) -> Self {
        let bytes = text.as_bytes();
        if bytes.len() <= CELL_TEXT_INLINE_BYTES {
            let mut inline = [0_u8; CELL_TEXT_INLINE_BYTES];
            inline[..bytes.len()].copy_from_slice(bytes);
            return Self(CellTextRepr::Inline {
                bytes: inline,
                len: bytes.len() as u8,
            });
        }
        Self(CellTextRepr::Spilled(Box::from(text)))
    }
}

impl From<String> for CellText {
    fn from(text: String) -> Self {
        Self::from(text.as_str())
    }
}

impl From<&String> for CellText {
    fn from(text: &String) -> Self {
        Self::from(text.as_str())
    }
}

impl From<char> for CellText {
    fn from(character: char) -> Self {
        let mut text = Self::new();
        text.push(character);
        text
    }
}

impl std::fmt::Debug for CellText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_str(), formatter)
    }
}

impl std::fmt::Display for CellText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PartialEq for CellText {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for CellText {}

impl PartialEq<str> for CellText {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for CellText {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for CellText {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<CellText> for str {
    fn eq(&self, other: &CellText) -> bool {
        self == other.as_str()
    }
}

impl PartialEq<CellText> for &str {
    fn eq(&self, other: &CellText) -> bool {
        *self == other.as_str()
    }
}

impl PartialOrd for CellText {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CellText {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl std::hash::Hash for CellText {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl std::borrow::Borrow<str> for CellText {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl FromIterator<char> for CellText {
    fn from_iter<I: IntoIterator<Item = char>>(characters: I) -> Self {
        let mut text = Self::new();
        for character in characters {
            text.push(character);
        }
        text
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CapturedCell {
    pub text: CellText,
    pub style: CellStyle,
    pub hyperlink: Option<CellHyperlink>,
    /// A terminal wide-character spacer has no source text of its own.
    pub wide_spacer: bool,
}

impl CapturedCell {
    pub fn plain(text: impl Into<CellText>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedRow {
    pub cells: Vec<CapturedCell>,
    /// True when this physical row soft-wraps into the next physical row.
    pub continues: bool,
    pub shell_mark: Option<String>,
    /// How wide the terminal grid was when this row was taken off it.
    ///
    /// Capture geometry, and nothing else. It is what [`PhysicalFragment::captured_columns`] is
    /// made of, and the reason it has to be recorded rather than recomputed is that the pane is
    /// resized and the frozen line is not: after a resize there is no longer anything on the
    /// screen that remembers how wide the row was when the application chose to end it.
    ///
    /// Zero means a row with no capture geometry — a synthetic row, a fixture — and the truncation
    /// gate declines to run on such a row rather than guessing a width for it.
    pub captured_columns: u32,
}

impl CapturedRow {
    /// One row of narrow characters, captured on a grid exactly as wide as the text.
    pub fn plain(text: &str, continues: bool) -> Self {
        let cells: Vec<CapturedCell> = text.chars().map(CapturedCell::plain).collect();
        Self {
            captured_columns: cells.len() as u32,
            cells,
            continues,
            shell_mark: None,
        }
    }

    /// One row of narrow characters, captured on a grid of a stated width.
    ///
    /// For the case the truncation gate is entirely about: text that stops short of the row's
    /// last column, and text that fills it, are the same string and differ only in the grid they
    /// were written on.
    pub fn plain_on_grid(text: &str, continues: bool, captured_columns: u32) -> Self {
        Self {
            captured_columns,
            ..Self::plain(text, continues)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StyleSpan {
    pub byte_start: u32,
    pub byte_end: u32,
    pub style: CellStyle,
    pub hyperlink: Option<CellHyperlink>,
}

/// One physical row's share of a rejoined logical line, and the grid geometry it was taken on.
///
/// # Why the width is stored and not recomputed
///
/// §7.1.5k ①'s truncation gate asks one question — did the printed reference reach the last cell
/// of the row the application wrote it on — and until 2026-08-24 the only way to answer it was to
/// replay the wrap at whatever width the pane happens to be **now**. That answer changes when the
/// reader drags the window: a reference that filled an eighty-column row stops being suspect the
/// moment the pane is a hundred columns wide, though nothing about what the application did has
/// changed. `captured_columns` is immutable provenance, so the gate has a fixed ruler
/// (`docs/plans/horizontal-scroll/plan.md` §5.4).
///
/// One logical line may hold fragments captured at different widths — a resize in the middle of
/// a wrapped line is ordinary — so the gate reads each fragment's own number and never spreads
/// one across the line.
///
/// It is **not** the line's content width. What can be presented is a question about the retained
/// payload and is answered by the flattened column count; this is a question about the grid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhysicalFragment {
    pub byte_start: u32,
    pub byte_end: u32,
    pub soft_wrapped: bool,
    /// The physical grid width this fragment was captured at; zero when the row carried no
    /// capture geometry at all.
    pub captured_columns: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrozenLine {
    pub id: TranscriptId,
    pub source_generation: SourceGeneration,
    pub text: String,
    /// UTF-8 byte positions for every grapheme boundary, including 0 and len.
    pub grapheme_boundaries: Vec<u32>,
    pub styles: Vec<StyleSpan>,
    pub fragments: Vec<PhysicalFragment>,
    pub shell_marks: Vec<(u32, String)>,
    pub wrap_split: bool,
}

impl FrozenLine {
    /// Everything this line costs the process: its own struct plus every
    /// allocation hanging off it.
    ///
    /// `capacity`, not `len`, because a pane's working set is what the allocator
    /// is holding and not what the line would need if it were rebuilt. The copy
    /// [`TranscriptStore`] keeps is a clone, and cloning a `Vec`/`String`
    /// allocates exactly its length, so for the stored line the two agree — but a
    /// caller measuring a line it built itself gets the truth about that line.
    ///
    /// It is the arithmetic the byte ceiling is made of, so it must count the
    /// parts that actually grow with a line's width: `text`, and the u32 per
    /// grapheme boundary that shadows it. On a 4,000-column line those two are
    /// 4,000 and 16,004 bytes, which is why such a line costs 32x an 80-column
    /// one rather than the 50x its text alone would suggest.
    #[must_use]
    pub fn resident_bytes(&self) -> usize {
        let hyperlink_bytes = |link: &Option<CellHyperlink>| {
            link.as_ref().map_or(0, |link| {
                link.uri.capacity() + link.id.as_ref().map_or(0, String::capacity)
            })
        };
        std::mem::size_of::<Self>()
            + self.text.capacity()
            + self.grapheme_boundaries.capacity() * std::mem::size_of::<u32>()
            + self.styles.capacity() * std::mem::size_of::<StyleSpan>()
            + self
                .styles
                .iter()
                .map(|span| hyperlink_bytes(&span.hyperlink))
                .sum::<usize>()
            + self.fragments.capacity() * std::mem::size_of::<PhysicalFragment>()
            + self.shell_marks.capacity() * std::mem::size_of::<(u32, String)>()
            + self
                .shell_marks
                .iter()
                .map(|(_, mark)| mark.capacity())
                .sum::<usize>()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedRow {
    pub id: StagingId,
    pub row: CapturedRow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FreezeCandidate {
    rows: Vec<StagedRow>,
    /// Mutable snapshot of the still-live continuation. It is never copied into frozen source;
    /// the authoritative cells are captured when that physical row actually scrolls out.
    live_tail: Option<CapturedRow>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnchorMapping {
    pub staging_id: StagingId,
    pub transcript_id: TranscriptId,
    pub grapheme_base: GraphemeOffset,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalizedLine {
    pub line: FrozenLine,
    pub mappings: Vec<AnchorMapping>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureResult {
    pub staging_id: StagingId,
    pub finalized: Vec<FinalizedLine>,
}

/// The only owner and quota authority for frozen terminal history.
#[derive(Clone, Debug)]
pub struct TranscriptStore {
    staging_quota: usize,
    frozen_quota: usize,
    next_staging: u64,
    next_transcript: u64,
    source_generation: SourceGeneration,
    staging_rows: usize,
    staging: VecDeque<FreezeCandidate>,
    /// Resize-owned rows temporarily transferred out of vendor history between actor operations.
    /// They are ordinary staging-plane sources for projection, but are not freeze candidates: the
    /// next resize/output operation returns the whole batch to vendor reflow first.
    resize_staging: Vec<StagedRow>,
    frozen: VecDeque<FrozenLine>,
    /// Running sum of [`FrozenLine::resident_bytes`] over `frozen`, maintained at
    /// every push and pop so the ceiling never has to walk history to ask how big
    /// it is.
    frozen_bytes: usize,
    tombstones: Vec<TranscriptId>,
    pending_evictions: Vec<TranscriptId>,
}

impl Default for TranscriptStore {
    fn default() -> Self {
        Self::new(DEFAULT_STAGING_QUOTA)
    }
}

impl TranscriptStore {
    pub fn new(quota: NonZeroUsize) -> Self {
        Self::with_quotas(quota, SPIKE_DEFAULT_FROZEN_QUOTA)
    }

    pub fn with_quotas(staging_quota: NonZeroUsize, frozen_quota: NonZeroUsize) -> Self {
        Self {
            staging_quota: staging_quota.get(),
            frozen_quota: frozen_quota.get(),
            next_staging: 1,
            next_transcript: 1,
            source_generation: SourceGeneration(1),
            staging_rows: 0,
            staging: VecDeque::new(),
            resize_staging: Vec::new(),
            frozen: VecDeque::new(),
            frozen_bytes: 0,
            tombstones: Vec::new(),
            pending_evictions: Vec::new(),
        }
    }

    pub fn staging_len(&self) -> usize {
        self.staging_rows + self.resize_staging.len()
    }

    /// How many frozen logical lines this store will hold.
    pub fn frozen_quota(&self) -> usize {
        self.frozen_quota
    }

    /// What the frozen history in this store currently costs the process.
    ///
    /// A running sum, not a walk: it is asked once per frozen line and the answer
    /// has to be free at that rate.
    #[must_use]
    pub fn frozen_bytes(&self) -> usize {
        self.frozen_bytes
    }

    /// The most memory this store's frozen history may hold — the reader's line
    /// capacity read as a memory budget (`FROZEN_BYTES_PER_LINE`).
    ///
    /// **It is derived, never stored.** There is no second setting and no second
    /// source of truth: move `Scrollback` and the ceiling moves with it in the same
    /// call, which is why [`Self::set_frozen_quota`] needs no ceiling argument.
    #[must_use]
    pub fn frozen_byte_budget(&self) -> usize {
        self.frozen_quota.saturating_mul(FROZEN_BYTES_PER_LINE)
    }

    /// Name a new frozen capacity and hand back the lines it costs, oldest first.
    ///
    /// **The number binds now.** Installing it and waiting would not spare a single
    /// line: [`Self::finalize`] evicts `len - quota` in one batch, so a smaller
    /// quota already collapses history the next time anything is frozen — at
    /// whichever later moment that happens to be. Doing the eviction here makes it
    /// happen because of the answer that caused it, and in the same turn, which is
    /// what lets the caller push the ids through its ordinary history-deletion
    /// pipeline instead of discovering them later in the pending channel.
    ///
    /// Growing is the asymmetric half and it is honest rather than lossy: nothing
    /// is returned, nothing is resurrected — a deleted line is gone — and the only
    /// thing a larger number changes is how much of the future is kept.
    ///
    /// The removals are **returned** rather than added to `pending_evictions`,
    /// because this is a call and not an observation: the caller is standing right
    /// here holding the answer, whereas that channel exists for lines evicted
    /// inside a `capture` the caller only learns about afterwards.
    pub fn set_frozen_quota(&mut self, quota: NonZeroUsize) -> Vec<TranscriptId> {
        self.frozen_quota = quota.get();
        self.enforce_frozen_limits()
    }
    pub fn frozen(&self) -> &VecDeque<FrozenLine> {
        &self.frozen
    }

    /// Frozen history as `(id, logical line)` pairs, oldest first — the shape the pattern engine
    /// eats (`search::find_all`).
    ///
    /// "Logical" is the whole point and is free here: `FrozenLine::text` is already the rejoined
    /// line with the styling held beside it, so a match spanning a soft-wrap boundary is an
    /// ordinary match, and so is one spanning two differently-coloured runs — the engine never sees
    /// the seam. Both were prototype-only limits the native indexer was promised not to inherit
    /// (DESIGN §7.1.5d, "原型双限已注记").
    ///
    /// **This is history and only history.** Staged rows are still `CapturedRow` cells with no
    /// rejoined text of their own, and the live grid is not in this store at all — so a caller that
    /// wants the word the user can see on the prompt line right now has to supply those two planes
    /// itself, in document order after these (`History < Staging < Live`, DESIGN §3.2). Materializing
    /// staged text here would mean a second copy of `normalize`'s wrap/padding rules, and two
    /// implementations of "what is this row's text" is exactly the drift this store exists to avoid.
    pub fn logical_lines(&self) -> impl Iterator<Item = (crate::search::LineId, &str)> {
        self.frozen
            .iter()
            .map(|line| (crate::search::LineId::History(line.id), line.text.as_str()))
    }

    /// Mutable scroll-out rows in capture order. Viewports may window these rows, but must never
    /// treat them as frozen source or mutate them outside the transcript owner.
    pub fn staged_rows(&self) -> impl Iterator<Item = &StagedRow> {
        self.staging
            .iter()
            .flat_map(|candidate| candidate.rows.iter())
            .chain(self.resize_staging.iter())
    }

    pub fn resize_staging_len(&self) -> usize {
        self.resize_staging.len()
    }

    /// Admit one vendor history snapshot into the existing staging plane without guessing which
    /// physical row closes a logical line. The batch is reversible as a whole until the resize
    /// transaction reaches its final harvest.
    pub fn stage_resize_rows(&mut self, rows: Vec<CapturedRow>) -> Vec<StagingId> {
        debug_assert!(self.resize_staging.is_empty());
        let mut ids = Vec::with_capacity(rows.len());
        for row in rows {
            let id = StagingId(self.next_staging);
            self.next_staging += 1;
            ids.push(id);
            self.resize_staging.push(StagedRow { id, row });
        }
        ids
    }

    /// Return the reversible resize batch to its vendor escrow before native reflow continues.
    pub fn take_resize_staging(&mut self) -> Vec<StagedRow> {
        std::mem::take(&mut self.resize_staging)
    }

    pub fn resize_staged_rows(&self) -> &[StagedRow] {
        &self.resize_staging
    }

    /// Commit the final reversible resize batch into the normal freeze-candidate pipeline without
    /// changing its staging identities. Viewport anchors can therefore resolve through the same
    /// staging-to-history relocation as ordinary scroll-out instead of vanishing at quiescence.
    pub fn commit_resize_staging(&mut self) -> Vec<CaptureResult> {
        let staged = std::mem::take(&mut self.resize_staging);
        staged
            .into_iter()
            .map(|row| self.capture_staged(row))
            .collect()
    }
    pub fn tombstones(&self) -> &[TranscriptId] {
        &self.tombstones
    }
    pub fn source_generation(&self) -> SourceGeneration {
        self.source_generation
    }

    pub fn take_evictions(&mut self) -> Vec<TranscriptId> {
        std::mem::take(&mut self.pending_evictions)
    }

    pub fn capture(&mut self, row: CapturedRow) -> CaptureResult {
        let id = StagingId(self.next_staging);
        self.next_staging += 1;
        self.capture_staged(StagedRow { id, row })
    }

    fn capture_staged(&mut self, staged: StagedRow) -> CaptureResult {
        let id = staged.id;
        let completes_candidate = !staged.row.continues;

        if let Some(candidate) = self
            .staging
            .back_mut()
            .filter(|candidate| candidate.rows.last().is_some_and(|row| row.row.continues))
        {
            candidate.rows.push(staged);
        } else {
            self.staging.push_back(FreezeCandidate {
                rows: vec![staged],
                live_tail: None,
            });
        }
        self.staging_rows += 1;

        let mut finalized = Vec::new();
        if completes_candidate && let Some(candidate) = self.staging.pop_back() {
            self.staging_rows -= candidate.rows.len();
            finalized.push(self.finalize(candidate, false));
        }
        finalized.extend(self.enforce_staging_quota());

        CaptureResult {
            staging_id: id,
            finalized,
        }
    }

    /// Freeze one harvested physical row as an independent wrap-split candidate.
    ///
    /// Resize-transaction harvest cannot prove whether a native `WRAPLINE` belongs to the next
    /// repainted row. Keeping the original row flag preserves every boundary cell, while forcing a
    /// candidate boundary prevents an observationally unrelated next row from being welded on.
    pub fn capture_wrap_split(&mut self, row: CapturedRow) -> CaptureResult {
        let id = StagingId(self.next_staging);
        self.next_staging += 1;
        let candidate = FreezeCandidate {
            rows: vec![StagedRow { id, row }],
            live_tail: None,
        };
        let finalized = vec![self.finalize(candidate, true)];
        CaptureResult {
            staging_id: id,
            finalized,
        }
    }

    /// A width change never joins a staged head with a live-grid tail.
    pub fn finalize_all_candidates(&mut self) -> Vec<FinalizedLine> {
        let candidates = self.staging.drain(..).collect::<Vec<_>>();
        self.staging_rows -= candidates
            .iter()
            .map(|candidate| candidate.rows.len())
            .sum::<usize>();
        candidates
            .into_iter()
            .map(|candidate| self.finalize(candidate, true))
            .collect()
    }

    pub fn rewrite_staged(&mut self, id: StagingId, row: CapturedRow) -> bool {
        for candidate in &mut self.staging {
            if candidate.rows.iter().any(|staged| staged.id == id) {
                candidate.live_tail = Some(row);
                return true;
            }
        }
        false
    }

    pub fn staged_tail(&self, id: StagingId) -> Option<&CapturedRow> {
        self.staging
            .iter()
            .find(|candidate| candidate.rows.iter().any(|staged| staged.id == id))
            .and_then(|candidate| candidate.live_tail.as_ref())
    }

    /// Number of physical rows in the sole candidate which still continues into live row zero.
    pub fn unclosed_candidate_len(&self) -> usize {
        self.staging
            .back()
            .filter(|candidate| candidate.rows.last().is_some_and(|row| row.row.continues))
            .map_or(0, |candidate| candidate.rows.len())
    }

    /// Return the unfinished logical-line prefix to its vendor-native resize owner.
    ///
    /// This is the bounded inverse of a resize harvest. Finalized lines never enter this path.
    pub fn take_unclosed_candidate(&mut self) -> Vec<StagedRow> {
        if self.unclosed_candidate_len() == 0 {
            return Vec::new();
        }
        let candidate = self
            .staging
            .pop_back()
            .expect("an unclosed candidate was observed immediately before removal");
        self.staging_rows -= candidate.rows.len();
        candidate.rows
    }

    pub fn evict_oldest(&mut self, count: usize) -> Vec<TranscriptId> {
        let mut removed = Vec::new();
        for _ in 0..count {
            if let Some(line) = self.frozen.pop_front() {
                self.frozen_bytes = self.frozen_bytes.saturating_sub(line.resident_bytes());
                removed.push(line.id);
                self.tombstones.push(line.id);
            }
        }
        if !removed.is_empty() {
            self.source_generation.0 += 1;
        }
        removed
    }

    /// ED3 and quota eviction intentionally share this tombstoning pipeline.
    pub fn clear_history(&mut self) -> Vec<TranscriptId> {
        let mut removed = self
            .frozen
            .drain(..)
            .map(|line| line.id)
            .collect::<Vec<_>>();
        self.frozen_bytes = 0;
        self.staging.clear();
        self.staging_rows = 0;
        self.resize_staging.clear();
        self.tombstones.extend(removed.iter().copied());
        self.source_generation.0 += 1;
        // Staging IDs are not tombstones. The caller must explicitly relocate their anchors by
        // invoking HistoryDocument::delete_transaction with clear_staging=true; generation only
        // invalidates versioned work and is not an anchor-deletion mechanism.
        removed.shrink_to_fit();
        removed
    }

    /// RIS/DECCOLM invalidates candidates but retains already frozen history.
    pub fn invalidate_staging(&mut self) {
        self.staging.clear();
        self.staging_rows = 0;
        self.resize_staging.clear();
        self.source_generation.0 += 1;
    }

    fn finalize(&mut self, candidate: FreezeCandidate, wrap_split: bool) -> FinalizedLine {
        let id = TranscriptId(self.next_transcript);
        self.next_transcript += 1;
        let (line, mappings) = normalize(id, self.source_generation, candidate.rows, wrap_split);
        self.frozen.push_back(line.clone());
        // The stored copy, not the one handed back: cloning a `Vec`/`String` sizes
        // the allocation to its length, so the store's line is the cheaper of the
        // two and it is the one the process is holding.
        self.frozen_bytes += self
            .frozen
            .back()
            .expect("the line was pushed immediately above")
            .resident_bytes();
        let removed = self.enforce_frozen_limits();
        self.pending_evictions.extend(removed);
        FinalizedLine { line, mappings }
    }

    /// Bring frozen history back inside **both** of its limits, oldest first.
    ///
    /// The reader named a number of lines; [`FROZEN_BYTES_PER_LINE`] turns that
    /// same number into the memory it may cost. Whichever arrives first is the one
    /// that binds, and neither is a second eviction mechanism: both count how many
    /// lines have to go and then hand that one number to [`Self::evict_oldest`], so
    /// the tombstones, the single source-generation bump, and everything
    /// `delete_history` retires downstream are character for character what a
    /// line-count overflow has always produced.
    ///
    /// **The newest line is never evicted.** A capacity of N lines promises at
    /// minimum the line just printed, so a single line wider than the whole ceiling
    /// stays and the ceiling is exceeded — the alternative is a terminal that
    /// forgets its own last output.
    ///
    /// Costed at the rate it is called: the line-count arm is a subtraction, and
    /// the byte arm walks only the lines it is about to delete, which in steady
    /// state is one. On a pane that is inside both limits — every ordinary pane —
    /// it is two comparisons and a return.
    fn enforce_frozen_limits(&mut self) -> Vec<TranscriptId> {
        let budget = self.frozen_byte_budget();
        let count_overflow = self.frozen.len().saturating_sub(self.frozen_quota);
        if self.frozen_bytes <= budget {
            // The ordinary pane, and the reason this costs nothing there: history
            // is inside the ceiling, so the reader's line count is the only limit
            // that has anything to say and the ceiling is one comparison.
            return if count_overflow == 0 {
                Vec::new()
            } else {
                self.evict_oldest(count_overflow)
            };
        }
        let total = self.frozen.len();
        let mut doomed = 0_usize;
        let mut carried = self.frozen_bytes;
        for line in &self.frozen {
            let over_lines = doomed < count_overflow;
            let over_bytes = carried > budget && total - doomed > 1;
            if !over_lines && !over_bytes {
                break;
            }
            carried -= line.resident_bytes();
            doomed += 1;
        }
        if doomed == 0 {
            return Vec::new();
        }
        self.evict_oldest(doomed)
    }

    fn enforce_staging_quota(&mut self) -> Vec<FinalizedLine> {
        let mut finalized = Vec::new();
        while self.staging_rows > self.staging_quota {
            let Some(candidate) = self.staging.pop_front() else {
                break;
            };
            self.staging_rows -= candidate.rows.len();
            let wrap_split = candidate.rows.last().is_some_and(|row| row.row.continues);
            finalized.push(self.finalize(candidate, wrap_split));
        }
        finalized
    }
}

fn normalize(
    id: TranscriptId,
    generation: SourceGeneration,
    rows: Vec<StagedRow>,
    wrap_split: bool,
) -> (FrozenLine, Vec<AnchorMapping>) {
    let mut text = String::new();
    let mut styles: Vec<StyleSpan> = Vec::new();
    let mut fragments = Vec::new();
    let mut shell_marks = Vec::new();
    let mut mappings = Vec::new();

    for staged in rows {
        let fragment_start = text.len() as u32;
        let grapheme_base = text.graphemes(true).count() as u32;
        mappings.push(AnchorMapping {
            staging_id: staged.id,
            transcript_id: id,
            grapheme_base: GraphemeOffset(grapheme_base),
        });
        let CapturedRow {
            mut cells,
            continues,
            shell_mark,
            captured_columns,
        } = staged.row;
        if let Some(mark) = shell_mark {
            shell_marks.push((fragment_start, mark));
        }

        // A WRAPLINE fragment owns every cell through its wrap boundary.  In particular a space
        // in the final column is source text, not padding; trimming it turns "find path" into
        // "findpath" when logical rows are later rejoined.  Only hard line ends trim padding,
        // and only visually inert padding: a trailing space carrying a non-default background
        // (or reverse video) paints a bar the application drew — Codex's prompt echo — so it is
        // content and must survive freezing.
        if !continues {
            while cells.last().is_some_and(|c| {
                !c.wide_spacer
                    && c.text.chars().all(char::is_whitespace)
                    && c.style.background == TerminalColor::Named(17)
                    && !c.style.flags.contains(CellFlags::INVERSE)
            }) {
                cells.pop();
            }
        }
        for cell in cells.into_iter().filter(|c| !c.wide_spacer) {
            let start = text.len() as u32;
            text.push_str(&cell.text);
            let end = text.len() as u32;
            if let Some(previous) = styles.last_mut().filter(|s| {
                s.byte_end == start && s.style == cell.style && s.hyperlink == cell.hyperlink
            }) {
                previous.byte_end = end;
            } else if start != end {
                styles.push(StyleSpan {
                    byte_start: start,
                    byte_end: end,
                    style: cell.style,
                    hyperlink: cell.hyperlink,
                });
            }
        }
        fragments.push(PhysicalFragment {
            byte_start: fragment_start,
            byte_end: text.len() as u32,
            soft_wrapped: continues,
            captured_columns,
        });
    }

    let mut grapheme_boundaries = text
        .grapheme_indices(true)
        .map(|(i, _)| i as u32)
        .collect::<Vec<_>>();
    grapheme_boundaries.push(text.len() as u32);
    grapheme_boundaries.dedup();
    (
        FrozenLine {
            id,
            source_generation: generation,
            text,
            grapheme_boundaries,
            styles,
            fragments,
            shell_marks,
            wrap_split,
        },
        mappings,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_two_hundred_forty_protocol_slots_are_xterms_and_the_sixteen_are_nobodys() {
        use super::indexed_cube_color;

        // The scheme's sixteen have no protocol answer.
        for index in 0..16u8 {
            assert_eq!(indexed_cube_color(index), None, "index {index}");
        }
        // The cube's corners, which every terminal agrees on.
        assert_eq!(indexed_cube_color(16), Some([0x00, 0x00, 0x00]));
        assert_eq!(indexed_cube_color(21), Some([0x00, 0x00, 0xff]));
        assert_eq!(indexed_cube_color(196), Some([0xff, 0x00, 0x00]));
        assert_eq!(indexed_cube_color(231), Some([0xff, 0xff, 0xff]));
        // The grey ramp's ends, which stop short of both black and white on
        // purpose - 8 and 238, not 0 and 255.
        assert_eq!(indexed_cube_color(232), Some([0x08, 0x08, 0x08]));
        assert_eq!(indexed_cube_color(255), Some([0xee, 0xee, 0xee]));
    }

    use super::*;

    fn nz(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).unwrap()
    }

    #[test]
    fn partial_wrap_waits_then_freezes_as_one_logical_line() {
        let mut store = TranscriptStore::new(nz(8));
        let first = store.capture(CapturedRow::plain("ab  ", true));
        assert!(first.finalized.is_empty());
        let second = store.capture(CapturedRow::plain("c", false));
        assert_eq!(second.finalized[0].line.text, "ab  c");
        assert_eq!(second.finalized[0].line.fragments.len(), 2);
    }

    /// Every fragment carries the grid it came off, and a resize in the middle of a wrapped line
    /// leaves one logical line holding two different ones.
    ///
    /// It has to be recorded here because there is nowhere later to recover it from: the pane is
    /// resized and the frozen line is not, so once the row is off the grid nothing on the screen
    /// remembers how wide it was when the application chose to end it. §7.1.5k ①'s gate is the
    /// consumer (`docs/plans/horizontal-scroll/plan.md` §5.4), and it reads each fragment's own
    /// number rather than spreading one across the line.
    #[test]
    fn every_fragment_remembers_the_grid_it_was_captured_on() {
        let mut store = TranscriptStore::new(nz(8));
        store.capture(CapturedRow::plain_on_grid("first half ", true, 20));
        let finalized = store.capture(CapturedRow::plain_on_grid("tail", false, 9));
        let line = &finalized.finalized[0].line;
        assert_eq!(line.text, "first half tail");
        assert_eq!(
            line.fragments
                .iter()
                .map(|fragment| (fragment.soft_wrapped, fragment.captured_columns))
                .collect::<Vec<_>>(),
            vec![(true, 20), (false, 9)]
        );
        // `plain` states its own width, so an ordinary fixture is honest without being told.
        assert_eq!(CapturedRow::plain("abc", false).captured_columns, 3);
    }

    #[test]
    fn soft_wrap_preserves_a_boundary_space_while_hard_end_trims_padding() {
        let mut store = TranscriptStore::new(nz(8));
        store.capture(CapturedRow::plain("find ", true));
        let finalized = store.capture(CapturedRow::plain("path   ", false));
        assert_eq!(finalized.finalized[0].line.text, "find path");
    }

    #[test]
    fn harvested_wrap_split_preserves_boundary_cells_without_joining_the_next_row() {
        let mut store = TranscriptStore::new(nz(8));
        let first = store.capture_wrap_split(CapturedRow::plain("find ", true));
        let second = store.capture_wrap_split(CapturedRow::plain("path", false));

        assert_eq!(first.finalized[0].line.text, "find ");
        assert!(first.finalized[0].line.wrap_split);
        assert_eq!(second.finalized[0].line.text, "path");
        assert_eq!(store.frozen().len(), 2);
    }

    #[test]
    fn harvest_batch_boundary_prevents_wrapline_from_joining_the_next_batch() {
        let mut store = TranscriptStore::new(nz(8));
        store.capture(CapturedRow::plain("batch-one", true));
        let first = store.finalize_all_candidates();
        let second = store.capture(CapturedRow::plain("batch-two", false));

        assert_eq!(first[0].line.text, "batch-one");
        assert!(first[0].line.wrap_split);
        assert_eq!(second.finalized[0].line.text, "batch-two");
        assert_eq!(store.frozen().len(), 2);
    }

    #[test]
    fn resize_and_quota_force_wrap_split() {
        let mut store = TranscriptStore::new(nz(1));
        let first = store.capture(CapturedRow::plain("head", true));
        assert!(first.finalized.is_empty());
        let overflow = store.capture(CapturedRow::plain("tail", true));
        assert!(overflow.finalized[0].line.wrap_split);

        store.capture(CapturedRow::plain("again", true));
        assert!(store.finalize_all_candidates()[0].line.wrap_split);
    }

    #[test]
    fn resize_staging_is_projectable_but_stays_reversible_as_one_batch() {
        let mut store = TranscriptStore::new(nz(8));
        let ids = store.stage_resize_rows(vec![
            CapturedRow::plain("closed", false),
            CapturedRow::plain("wrapped", true),
        ]);

        assert_eq!(store.staging_len(), 2);
        assert_eq!(
            store.staged_rows().map(|row| row.id).collect::<Vec<_>>(),
            ids
        );
        assert!(store.frozen().is_empty());

        let returned = store.take_resize_staging();
        assert_eq!(returned.len(), 2);
        assert_eq!(store.staging_len(), 0);
        assert!(store.frozen().is_empty());
    }

    #[test]
    fn final_resize_commit_preserves_staging_ids_for_anchor_relocation() {
        let mut store = TranscriptStore::new(nz(8));
        let ids = store.stage_resize_rows(vec![
            CapturedRow::plain("closed", false),
            CapturedRow::plain("wrapped", true),
        ]);

        let committed = store.commit_resize_staging();
        assert_eq!(committed.len(), 2);
        assert_eq!(committed[0].finalized[0].mappings[0].staging_id, ids[0]);
        assert!(committed[1].finalized.is_empty());
        assert_eq!(store.staged_rows().next().map(|row| row.id), Some(ids[1]));
        assert_eq!(store.unclosed_candidate_len(), 1);
    }

    #[test]
    fn normalization_keeps_graphemes_links_and_drops_wide_spacers() {
        let mut store = TranscriptStore::new(nz(8));
        let linked = CapturedCell {
            text: "e\u{301}".into(),
            hyperlink: Some(CellHyperlink::implicit("https://example.test")),
            ..CapturedCell::default()
        };
        let spacer = CapturedCell {
            wide_spacer: true,
            ..CapturedCell::default()
        };
        let result = store.capture(CapturedRow {
            cells: vec![linked, spacer, CapturedCell::plain(" ")],
            continues: false,
            shell_mark: Some("prompt".into()),
            captured_columns: 3,
        });
        let line = &result.finalized[0].line;
        assert_eq!(line.text, "e\u{301}");
        assert_eq!(line.grapheme_boundaries, vec![0, 3]);
        assert_eq!(
            line.styles[0]
                .hyperlink
                .as_ref()
                .map(|link| link.uri.as_str()),
            Some("https://example.test")
        );
        assert_eq!(line.shell_marks[0].1, "prompt");
    }

    #[test]
    fn mutable_staging_can_be_rewritten_and_eviction_leaves_tombstone() {
        let mut store = TranscriptStore::new(nz(8));
        let staged = store.capture(CapturedRow::plain("old", true));
        assert!(store.rewrite_staged(staged.staging_id, CapturedRow::plain("new", true)));
        assert_eq!(
            store.staged_tail(staged.staging_id),
            Some(&CapturedRow::plain("new", true))
        );
        let finalized = store.finalize_all_candidates().remove(0);
        assert_eq!(finalized.line.text, "old");
        let removed = store.evict_oldest(1);
        assert_eq!(removed, vec![finalized.line.id]);
        assert_eq!(store.tombstones(), removed);
    }

    #[test]
    fn unfinished_candidate_can_return_to_vendor_without_thawing_frozen_lines() {
        let mut store = TranscriptStore::new(nz(8));
        let frozen = store.capture(CapturedRow::plain("closed", false));
        assert_eq!(frozen.finalized.len(), 1);
        let first = store.capture(CapturedRow::plain("active-1", true));
        let second = store.capture(CapturedRow::plain("active-2", true));

        assert_eq!(store.unclosed_candidate_len(), 2);
        assert_eq!(
            store
                .take_unclosed_candidate()
                .into_iter()
                .map(|row| row.id)
                .collect::<Vec<_>>(),
            [first.staging_id, second.staging_id]
        );
        assert_eq!(store.unclosed_candidate_len(), 0);
        assert_eq!(store.staging_len(), 0);
        assert_eq!(store.frozen().len(), 1);
        assert_eq!(store.frozen()[0].text, "closed");
    }

    #[test]
    fn frozen_quota_is_enforced_by_the_store() {
        let mut store = TranscriptStore::with_quotas(nz(8), nz(2));
        for text in ["one", "two", "three"] {
            store.capture(CapturedRow::plain(text, false));
        }
        assert_eq!(
            store
                .frozen()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["two", "three"]
        );
        assert_eq!(store.take_evictions(), vec![TranscriptId(1)]);
        assert_eq!(store.tombstones(), &[TranscriptId(1)]);
    }

    /// PIN — **a capacity binds the moment it is named**, and lowering one deletes
    /// the oldest lines at once rather than waiting for output that may never come.
    ///
    /// The alternative was never "keep them": `finalize` already evicts
    /// `len - quota` in one go, so a smaller number installed lazily would still
    /// drop every overflowing line — just at whichever unpredictable later moment
    /// the next line was frozen. Doing it here makes the deletion attributable to
    /// the answer that caused it, which is also what lets a caller run the removed
    /// ids through the ordinary history-deletion pipeline in the same turn.
    #[test]
    fn lowering_the_frozen_quota_evicts_the_oldest_at_once() {
        let mut store = TranscriptStore::with_quotas(nz(8), nz(8));
        for text in ["one", "two", "three", "four"] {
            store.capture(CapturedRow::plain(text, false));
        }
        assert_eq!(store.frozen().len(), 4);
        assert!(store.take_evictions().is_empty());

        let removed = store.set_frozen_quota(nz(2));
        assert_eq!(removed, vec![TranscriptId(1), TranscriptId(2)]);
        assert_eq!(
            store
                .frozen()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["three", "four"],
            "the oldest go, because history is read from its newest end"
        );
        assert_eq!(store.tombstones(), &[TranscriptId(1), TranscriptId(2)]);
        assert_eq!(store.frozen_quota(), 2);
        assert!(
            store.take_evictions().is_empty(),
            "the removal is handed to the caller directly, not left in the pending \
             channel for whoever drains it next"
        );
    }

    /// PIN — **raising a capacity resurrects nothing and admits more**, which is
    /// the honest asymmetry of the pair: a line already deleted is gone, so the
    /// only thing a larger number can change is the future.
    #[test]
    fn raising_the_frozen_quota_keeps_what_is_there_and_admits_more() {
        let mut store = TranscriptStore::with_quotas(nz(8), nz(2));
        for text in ["one", "two", "three"] {
            store.capture(CapturedRow::plain(text, false));
        }
        assert_eq!(store.frozen().len(), 2);

        assert!(
            store.set_frozen_quota(nz(4)).is_empty(),
            "growing deletes nothing"
        );
        assert_eq!(
            store
                .frozen()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["two", "three"],
            "and `one` does not come back"
        );
        for text in ["four", "five"] {
            store.capture(CapturedRow::plain(text, false));
        }
        assert_eq!(
            store
                .frozen()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["two", "three", "four", "five"],
            "the new room is real"
        );
    }

    /// One 80-column line's cost, and the shapes that leave it behind.
    ///
    /// §7.1.6g read the whole `Scrollback` ladder off a single measurement — "one
    /// 80-column frozen line occupies 588 bytes, so the ladder spans about 14 MB to
    /// 112 MB a pane" — and then the measurement was left to drift: `CellHyperlink`
    /// grew its OSC 8 `id`, which widened every `StyleSpan` by eight bytes. A number
    /// a document quotes has to be a number something re-derives, so this is where
    /// the ladder's arithmetic lives from now on.
    ///
    /// It also prints the shape the ladder never costed, which is the whole reason
    /// there is a byte ceiling at all.
    #[test]
    fn the_shape_of_a_frozen_line_is_measured_not_remembered() {
        fn cost(text: &str) -> usize {
            let mut store = TranscriptStore::new(DEFAULT_STAGING_QUOTA);
            store.capture(CapturedRow::plain(text, false));
            store.frozen()[0].resident_bytes()
        }

        let plain_80 = cost(&"x".repeat(80));
        let wide_4000 = cost(&"x".repeat(4_000));
        let cjk_80 = cost(&"漢".repeat(40));

        // The heaviest ordinary line there is: 80 columns changing colour every
        // fourth one, which is about as much styling as a terminal ever paints.
        let mut store = TranscriptStore::new(DEFAULT_STAGING_QUOTA);
        store.capture(CapturedRow {
            cells: (0..80_u8)
                .map(|column| {
                    let mut cell = CapturedCell::plain("x");
                    cell.style.foreground = TerminalColor::Indexed(16 + column / 4);
                    cell
                })
                .collect(),
            continues: false,
            shell_mark: None,
            captured_columns: 80,
        });
        let rainbow_80 = store.frozen()[0].resident_bytes();

        eprintln!(
            "FROZEN_LINE_COST style_span={} frozen_line_struct={} plain_80={plain_80} \
             cjk_80={cjk_80} rainbow_80={rainbow_80} wide_4000={wide_4000} \
             ceiling_per_line={FROZEN_BYTES_PER_LINE}",
            std::mem::size_of::<StyleSpan>(),
            std::mem::size_of::<FrozenLine>(),
        );
        // Asserted, not described: twenty style spans are most of the ceiling, so
        // a `StyleSpan` that gains a field has to re-open `FROZEN_BYTES_PER_LINE`
        // rather than quietly shorten a rainbow pane's history.
        assert_eq!(rainbow_80, 2_000, "the heaviest 80-column line there is");
        assert!(
            rainbow_80 < FROZEN_BYTES_PER_LINE,
            "the heaviest 80-column line costs {rainbow_80}, which the ceiling would take \
             history for"
        );

        // The ladder §7.1.6g published, re-derived rather than remembered.
        assert_eq!(plain_80, 632, "one 80-column frozen line");
        for (lines, megabytes) in [(25_000, 15), (100_000, 63), (200_000, 126)] {
            assert_eq!((lines * plain_80) / 1_000_000, megabytes, "{lines} lines");
        }

        // And the shape that is not on it: joining fifty physical rows into one
        // logical line costs its text once and a u32 per grapheme on top.
        assert!(
            wide_4000 > 20 * plain_80,
            "a 4,000-column line costs {wide_4000}, only {:.1}x an 80-column one",
            wide_4000 as f64 / plain_80 as f64
        );
        assert!(
            SPIKE_DEFAULT_FROZEN_QUOTA.get() * wide_4000 > 1_500 * 1024 * 1024,
            "the flood the ceiling exists for should still measure over 1.5 GiB a pane"
        );
    }

    /// RED (user report 2026-08-24, activity line) — **a line quota is not a memory
    /// bound**, and one pane can spend gigabytes obeying it exactly.
    ///
    /// `for(;;){ [Console]::Out.WriteLine('x'*4000) }` freezes one logical line per
    /// fifty physical rows, each about 20 KB, and the store keeps 100,000 of them
    /// because 100,000 is what it was told to keep. The window stayed usable (that
    /// was the livelock, fixed separately); the memory did not stop.
    ///
    /// Red run, before the ceiling existed: `lines_kept=2000 of 2000
    /// resident=40464000 budget=4096000`. Green: `lines_kept=202
    /// resident=4086864`. The store here is scaled down 50x; the shipped
    /// configuration is the same arithmetic — 1.88 GiB becomes 195 MiB.
    #[test]
    fn a_wide_line_flood_is_stopped_by_bytes_before_it_is_stopped_by_lines() {
        const LINE_QUOTA: usize = 2_000;
        const WIDTH: usize = 4_000;

        let mut store = TranscriptStore::with_quotas(DEFAULT_STAGING_QUOTA, nz(LINE_QUOTA));
        let wide = "x".repeat(WIDTH);
        let mut first = None;
        let mut last = None;
        for _ in 0..LINE_QUOTA {
            for finalized in store.capture(CapturedRow::plain(&wide, false)).finalized {
                first.get_or_insert(finalized.line.id);
                last = Some(finalized.line.id);
            }
        }
        let first = first.expect("the flood froze at least one line");
        let last = last.expect("the flood froze at least one line");

        let resident = store
            .frozen()
            .iter()
            .map(FrozenLine::resident_bytes)
            .sum::<usize>();
        eprintln!(
            "WIDE_FLOOD lines_kept={} of {LINE_QUOTA} resident={resident} budget={}",
            store.frozen().len(),
            store.frozen_byte_budget()
        );

        // The ceiling holds, and it is the thing that bound: far fewer lines than
        // the reader's number are still here.
        assert_eq!(
            resident,
            store.frozen_bytes(),
            "the running sum is the truth"
        );
        assert!(
            resident <= store.frozen_byte_budget(),
            "a pane held {resident} bytes of frozen history against a {} byte ceiling",
            store.frozen_byte_budget()
        );
        assert!(
            store.frozen().len() < LINE_QUOTA / 4,
            "bytes must bind before lines under a wide flood, but {} of {LINE_QUOTA} lines \
             survived",
            store.frozen().len()
        );

        // Oldest first, newest still arriving — the same order every other eviction
        // in this store uses.
        assert!(
            !store.frozen().iter().any(|line| line.id == first),
            "the oldest line is the one that goes"
        );
        assert_eq!(
            store.frozen().back().map(|line| line.id),
            Some(last),
            "and the newest line is still admitted"
        );
        assert!(
            store.tombstones().contains(&first),
            "a byte-evicted line leaves the same tombstone a line-evicted one does"
        );
    }

    /// PIN — **the reader's number is still the number that binds** for a pane
    /// printing ordinary output. A ceiling that quietly shortened everybody's
    /// history would be a second, worse answer to a question the `Scrollback` row
    /// already answers.
    #[test]
    fn an_ordinary_pane_keeps_every_line_its_capacity_promised() {
        const LINE_QUOTA: usize = 5_000;

        let mut store = TranscriptStore::with_quotas(DEFAULT_STAGING_QUOTA, nz(LINE_QUOTA));
        // 80 columns, eight style runs and an OSC 8 target — a coloured `ls` line,
        // which is far heavier than the plain line the ladder was costed on.
        let mut cells = Vec::new();
        for column in 0..80_u8 {
            let mut cell = CapturedCell::plain("x");
            cell.style.foreground = TerminalColor::Indexed(16 + column / 10);
            if column < 12 {
                cell.hyperlink = Some(CellHyperlink::implicit("https://example.test/a/b/c"));
            }
            cells.push(cell);
        }
        for _ in 0..LINE_QUOTA {
            store.capture(CapturedRow {
                cells: cells.clone(),
                continues: false,
                shell_mark: None,
                captured_columns: 80,
            });
        }

        eprintln!(
            "ORDINARY_PANE lines={} resident={} budget={} per_line={}",
            store.frozen().len(),
            store.frozen_bytes(),
            store.frozen_byte_budget(),
            store.frozen_bytes() / store.frozen().len().max(1),
        );
        assert_eq!(
            store.frozen().len(),
            LINE_QUOTA,
            "the ceiling took history from a pane of ordinary width"
        );
        assert!(store.take_evictions().is_empty());
    }

    /// The ceiling has to be free on the pane that never meets it, because every
    /// pane asks it once per frozen line forever.
    ///
    /// Two arms measured against each other in the same window: a store at its line
    /// quota, evicting one line per capture with the ceiling nowhere near — the
    /// ordinary pane — versus the same work with room to spare and no eviction at
    /// all. What the ceiling adds to the first is one comparison against a running
    /// sum plus one `resident_bytes` walk of the line being deleted, and
    /// `resident_bytes` is O(style runs) where `normalize` was already O(cells).
    #[test]
    fn the_byte_ceiling_costs_an_ordinary_pane_nothing() {
        const LINES: usize = 100_000;

        fn drive(quota: usize) -> std::time::Duration {
            let mut store = TranscriptStore::with_quotas(DEFAULT_STAGING_QUOTA, nz(quota));
            let row = CapturedRow::plain(
                "cargo:rerun-if-changed=crates/bt-term/src/session.rs   ok  0.42s",
                false,
            );
            let started = std::time::Instant::now();
            for _ in 0..LINES {
                store.capture(row.clone());
                store.take_evictions();
            }
            let measured = started.elapsed();
            assert!(store.frozen_bytes() <= store.frozen_byte_budget());
            measured
        }

        // Warm the allocator and the branch predictor before either arm is timed.
        drive(1_000);
        let evicting = drive(1_000);
        let roomy = drive(LINES + 1);
        let overhead = evicting.as_secs_f64() / roomy.as_secs_f64();
        eprintln!(
            "CEILING_COST lines={LINES} at_quota={evicting:?} with_room={roomy:?} \
             ratio={overhead:.2}"
        );
        assert!(
            overhead <= 2.0,
            "freezing at the line quota costs {overhead:.2}x freezing with room to spare \
             ({evicting:?} vs {roomy:?}); the ceiling is supposed to be a comparison"
        );
    }

    /// PIN — **a capacity of N lines promises the newest line unconditionally.**
    /// One line wider than the whole ceiling is still that pane's most recent
    /// output, and a store that answered "nothing" would be a terminal that forgets
    /// the line it just printed.
    #[test]
    fn the_byte_ceiling_never_evicts_the_only_line() {
        let mut store = TranscriptStore::with_quotas(DEFAULT_STAGING_QUOTA, nz(1));
        let enormous = "x".repeat(FROZEN_BYTES_PER_LINE * 4);
        store.capture(CapturedRow::plain(&enormous, false));

        assert_eq!(store.frozen().len(), 1);
        assert!(store.frozen_bytes() > store.frozen_byte_budget());
        assert_eq!(store.frozen()[0].text.len(), enormous.len());
    }

    /// PIN — **the ceiling is derived, not stored.** `Scrollback` moves and the
    /// ceiling moves with it in the same turn, through the same
    /// [`TranscriptStore::set_frozen_quota`] that hands its removals back to the
    /// caller rather than leaving them in the pending channel.
    #[test]
    fn retuning_the_line_capacity_retunes_the_byte_ceiling_with_it() {
        const WIDTH: usize = 4_000;

        let mut store = TranscriptStore::with_quotas(DEFAULT_STAGING_QUOTA, nz(2_000));
        let wide = "x".repeat(WIDTH);
        for _ in 0..400 {
            store.capture(CapturedRow::plain(&wide, false));
        }
        let before = store.frozen().len();
        assert_eq!(store.frozen_byte_budget(), 2_000 * FROZEN_BYTES_PER_LINE);
        // The flood's own evictions are the capture path's; drain them so what is
        // left is the retune's alone.
        assert!(!store.take_evictions().is_empty());

        let removed = store.set_frozen_quota(nz(100));
        assert_eq!(store.frozen_byte_budget(), 100 * FROZEN_BYTES_PER_LINE);
        assert!(
            !removed.is_empty() && store.frozen().len() < before,
            "a smaller capacity binds now, by bytes as well as by lines"
        );
        assert!(store.frozen_bytes() <= store.frozen_byte_budget());
        assert!(
            store.frozen().len() < 100,
            "the new ceiling arrives before the new line count: {} lines kept for a \
             capacity of 100",
            store.frozen().len()
        );
        assert!(
            store.take_evictions().is_empty(),
            "a retune's removals are returned to the caller, not left pending"
        );
    }

    /// PIN — clearing history clears the ceiling's arithmetic with it. A running
    /// sum that survived `ESC [ 3 J` would refuse to admit lines into an empty
    /// store.
    #[test]
    fn clearing_history_returns_the_byte_ledger_to_zero() {
        let mut store = TranscriptStore::with_quotas(DEFAULT_STAGING_QUOTA, nz(8));
        for text in ["one", "two", "three"] {
            store.capture(CapturedRow::plain(text, false));
        }
        assert!(store.frozen_bytes() > 0);

        store.clear_history();
        assert_eq!(store.frozen_bytes(), 0);

        store.capture(CapturedRow::plain("after", false));
        assert_eq!(store.frozen().len(), 1);
        assert_eq!(store.frozen_bytes(), store.frozen()[0].resident_bytes());
    }

    #[test]
    fn bare_http_urls_strip_terminal_prose_punctuation() {
        let text = "See (https://example.test/a?q=1). Then \"http://localhost:3000/x!\"";
        let ranges = detect_http_urls(text);
        assert_eq!(
            ranges
                .iter()
                .map(|range| &text[range.byte_start..range.byte_end])
                .collect::<Vec<_>>(),
            ["https://example.test/a?q=1", "http://localhost:3000/x"]
        );
    }

    #[test]
    fn bare_http_urls_require_conservative_boundaries_and_hosts() {
        let text = concat!(
            "xhttps://example.test ",
            "ftp://example.test ",
            "http://intranet ",
            "http://localhost:0 ",
            "http://localhost:65536 ",
            "https://good.example"
        );
        let ranges = detect_http_urls(text);
        assert_eq!(ranges.len(), 1);
        assert_eq!(
            &text[ranges[0].byte_start..ranges[0].byte_end],
            "https://good.example"
        );
    }

    /// PIN (user report 2026-08-20) — **a bare URL ends at the first byte that cannot belong to
    /// it, and every non-ASCII byte is such a byte.**
    ///
    /// Claude Code printed three bare addresses in a row. The two with a line break after them
    /// were links; the third, followed immediately by `（带图片和表格，内容更复杂）`, was not a
    /// link at all — the scan ran on through the prose to the end of the line, and the validator
    /// then threw the entire candidate away for not being ASCII. The validator's demand that a
    /// candidate be pure ASCII and the scanner's idea of where a candidate stops must say the
    /// same sentence, so a byte `>= 0x80` terminates exactly like a space does.
    #[test]
    fn a_bare_url_ends_at_the_first_non_ascii_byte() {
        for (text, expected) in [
            (
                "https://raw.githubusercontent.com/microsoft/terminal/main/README.md（带图片和表",
                Some("https://raw.githubusercontent.com/microsoft/terminal/main/README.md"),
            ),
            (
                "https://example.test/a.md（中文）",
                Some("https://example.test/a.md"),
            ),
            (
                "https://example.test/a.md。",
                Some("https://example.test/a.md"),
            ),
            (
                "https://example.test/a.md，下一句",
                Some("https://example.test/a.md"),
            ),
            // The leading boundary is unchanged: prose pressed against the scheme is still no
            // address at all, because a boundary we cannot read is not a boundary.
            ("中文https://example.test/x", None),
        ] {
            let ranges = detect_http_urls(text);
            assert_eq!(
                ranges
                    .iter()
                    .map(|range| &text[range.byte_start..range.byte_end])
                    .collect::<Vec<_>>(),
                expected.into_iter().collect::<Vec<_>>(),
                "reading `{text}`"
            );
            if expected.is_some() {
                assert!(
                    !text.as_bytes()[ranges[0].byte_end].is_ascii(),
                    "the byte the address stops before is the non-ASCII one, in `{text}`"
                );
            }
        }
    }

    #[test]
    fn trailing_spaces_with_a_painted_background_survive_freezing() {
        // Codex echoes the user's prompt on a background bar that extends past the text with
        // background-colored spaces. Those cells are visible content, not padding: trimming them
        // truncated the bar at the last glyph once the line froze into history.
        let mut store = TranscriptStore::new(NonZeroUsize::new(4).unwrap());
        let mut bar_space = CapturedCell::plain(" ");
        bar_space.style.background = TerminalColor::Rgb(41, 41, 41);
        let mut glyph = CapturedCell::plain("x");
        glyph.style.background = TerminalColor::Rgb(41, 41, 41);
        let result = store.capture(CapturedRow {
            cells: vec![
                glyph,
                bar_space.clone(),
                bar_space.clone(),
                CapturedCell::plain(" "),
            ],
            continues: false,
            shell_mark: None,
            captured_columns: 4,
        });
        let line = &result.finalized[0].line;
        assert_eq!(
            line.text, "x  ",
            "background-painted spaces stay; the default-background pad is trimmed"
        );
        let last = line.styles.last().unwrap();
        assert_eq!(last.byte_end, 3);
        assert_eq!(last.style.background, TerminalColor::Rgb(41, 41, 41));
    }
}
