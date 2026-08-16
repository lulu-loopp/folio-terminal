//! Canonical frozen transcript and mutable staging primitives.

use std::{collections::VecDeque, num::NonZeroUsize};

use bitflags::bitflags;
use unicode_segmentation::UnicodeSegmentation;

pub const DEFAULT_STAGING_QUOTA: NonZeroUsize = NonZeroUsize::new(4096).unwrap();
/// Spike-only value; M0 must replace it with a measured or configured quota.
pub const SPIKE_DEFAULT_FROZEN_QUOTA: NonZeroUsize = NonZeroUsize::new(100_000).unwrap();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HyperlinkRange {
    pub byte_start: usize,
    pub byte_end: usize,
}

/// Recognize deliberately narrow bare web URLs without changing transcript source text.
///
/// Candidates must begin at a conservative prose boundary, use `http://` or `https://`, and
/// contain an unambiguous host. In particular, single-label hosts are rejected except for the
/// explicitly supported `localhost` development case.
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
        while end > cursor + scheme_len
            && matches!(
                bytes[end - 1],
                b')' | b'.' | b',' | b';' | b':' | b'!' | b'?'
            )
        {
            end -= 1;
        }
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

fn http_scheme_len(bytes: &[u8]) -> Option<usize> {
    bytes
        .starts_with(b"http://")
        .then_some(7)
        .or_else(|| bytes.starts_with(b"https://").then_some(8))
}

fn is_url_leading_boundary(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'"' | b'\'' | b'(' | b'[' | b'{' | b'<')
}

fn is_url_terminator(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'"' | b'\'' | b'`' | b'<' | b'>')
}

fn bare_http_url_is_valid(candidate: &str, scheme_len: usize) -> bool {
    if !candidate.is_ascii()
        || candidate
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
}

impl CapturedRow {
    pub fn plain(text: &str, continues: bool) -> Self {
        Self {
            cells: text.chars().map(CapturedCell::plain).collect(),
            continues,
            shell_mark: None,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhysicalFragment {
    pub byte_start: u32,
    pub byte_end: u32,
    pub soft_wrapped: bool,
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
            tombstones: Vec::new(),
            pending_evictions: Vec::new(),
        }
    }

    pub fn staging_len(&self) -> usize {
        self.staging_rows + self.resize_staging.len()
    }
    pub fn frozen(&self) -> &VecDeque<FrozenLine> {
        &self.frozen
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
        let overflow = self.frozen.len().saturating_sub(self.frozen_quota);
        if overflow != 0 {
            let removed = self.evict_oldest(overflow);
            self.pending_evictions.extend(removed);
        }
        FinalizedLine { line, mappings }
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
