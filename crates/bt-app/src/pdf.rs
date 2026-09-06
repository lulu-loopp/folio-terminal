//! **What a glance can learn about a PDF without opening it in the engine**
//! (user rulings 2026-08-25; `docs/DESIGN.md` §7.10 ⑥).
//!
//! Two answers live here and they are answered two very different ways:
//!
//! * [`page_count`] — **how many pages the file holds**, read off its own
//!   structure by a tokenizer that renders, decompresses and resolves nothing,
//!   and — for the documents whose structure is compressed out of a tokenizer's
//!   reach — off the page list of a parse (2026-09-05).
//! * [`page_raster`] — **what the first page looks like**, rasterised.
//!
//! # Why the count exists at all
//!
//! The glance card has no engine — a page is drawn by WebView2, on a seat, and
//! the pane's pixels never cross back into this process. What the card can do is
//! state the facts a reader hovers a `.pdf` to learn, and the first ruling names
//! two: how large the file is, and how many pages are in it. The first is a
//! `metadata` call. The second is [`page_count`].
//!
//! Its answer's type carries the ruling's own fallback: when neither reader can
//! say how many pages there are, the answer is `None` and the card states the
//! size alone. A wrong number would be worse than no number, so every place
//! neither reader can see is a `None` rather than a guess.
//!
//! # And why a picture arrived beside it
//!
//! "How many pages" and "how big" place a file; they do not *show* it, and the
//! second ruling is that a reader hovering a report wants to see the report. The
//! sentence the card was built on — *there is no engine on a hover card* — is
//! still true of the **pane's** engine and stopped being true of the card the
//! moment this process grew a rasteriser of its own. [`page_raster`] is that
//! rasteriser's one door.
//!
//! **It is [`hayro`], and it is in this build already.** `typst-svg` rasterises
//! the PDFs a Typst document embeds with it, so the formula lane has been
//! carrying it since the day formulas landed; naming it a dependency of this
//! crate costs no new package, which is `docs/DESIGN.md` §8's bar. What it buys
//! over a pdfium binding is the whole distribution question: hayro is Rust that
//! links into `folio.exe`, so there is no 5MB C++ library to ship beside the
//! exe, to find at startup, to fail to find, or to write a degraded card for.
//! The feature either compiles or it does not.
//!
//! # What [`page_raster`] refuses, and the shape of every refusal
//!
//! `None`, always — the same word the count answers with, so the card has one
//! silence rather than a vocabulary of failures. It is the answer for a file
//! that is not a PDF, a PDF whose cross-reference table is past repairing, one
//! that is encrypted, one with no pages, and one larger than
//! [`MAX_RASTER_BYTES`]. The card draws its ground and no page, exactly as it
//! does for a `.png` whose decode failed, and the two fact lines are unaffected:
//! they came down a different lane and a file with no readable picture very
//! often still has a readable size.
//!
//! # What it reads, in the order it trusts them
//!
//! 1. **The page tree's own `/Count`.** A `/Type /Pages` dictionary declares how
//!    many pages hang below it, and the root of the tree declares the whole
//!    document — so the greatest count any `/Pages` node declares *is* the page
//!    count. The maximum rather than the first, because a file written in
//!    incremental updates carries several generations of the same node and the
//!    tree may be split across several; and it is taken **per dictionary** —
//!    `/Count` is a key in outlines and in article threads too, and an outline
//!    with more entries than the document has pages would otherwise answer this
//!    question with somebody else's number.
//! 2. **The page objects themselves.** With no `/Pages` node in reach, every
//!    `/Type /Page` dictionary is one page, counted.
//!
//! # What the scan cannot see, and who answers instead (2026-09-05)
//!
//! **Object streams** (PDF 1.5+): a file may pack its catalogue, its page tree
//! and its page objects into compressed streams, and none of the three is
//! readable without inflating them. The scan finds neither a `/Count` nor a
//! `/Page` in such a file and has nothing to say about it.
//!
//! It said `None` for a year, and `None` reached the card as **a page with no
//! page count and a column that would not turn** — `peek_page_column_max_scroll`
//! is computed from the count, so a document whose count is unknown is a
//! document one page long. That is most of the world, and it was measured
//! rather than assumed: over the 1622 PDFs on one disk (2026-09-05), 180 came
//! out of pdfTeX or LaTeX and 172 of those carry object streams — 159 of them
//! do not contain the four characters `/Count` anywhere at all — while all 60
//! written by Skia (headless Edge, Chromium's print) are in the clear. The
//! fixture beside this crate is countable only because it is one of the second
//! kind.
//!
//! So the count has **a second reader behind the scan**, and it is the
//! rasteriser already linked into this binary: [`hayro`] inflates the object
//! streams, resolves the cross-reference stream and hands over its page list,
//! whose length is the count. The order is the point —
//!
//! 1. the byte scan, which costs [`CHUNK_BYTES`] of memory over a file of any
//!    size and answers every document whose structure is in the clear;
//! 2. and only when that says nothing, the parse, which costs the file resident
//!    (bounded by [`MAX_RASTER_BYTES`], the same bound the raster is under) and
//!    takes a millisecond on a small paper, fifteen on a 2MB book of slides.
//!
//! Both run on the preview worker, never on the thread that draws.
//!
//! **A `/Count` written as an indirect reference** (`/Count 12 0 R`) is legal
//! and is read by the scan as the object number it begins with — a number that
//! is not a page count. It is not written by any producer this window has met,
//! and a file that did would be answered wrongly by the scan rather than passed
//! to the reader, because the scan's answer wins when it has one.

use std::io::Read;
use std::path::Path;

/// How much of the file is read at a time.
///
/// The scan is a state machine over a byte stream rather than a search over a
/// buffer, so this number is a syscall size and nothing else: no token can be
/// missed by falling across a chunk boundary, which is the whole reason it is
/// written this way instead of as a `read_to_end` and a slice search. A hovered
/// file may be a hundred megabytes and the card owes it no more memory than a
/// terminal read.
const CHUNK_BYTES: usize = 64 * 1024;

/// How far into the file the `%PDF-` header may stand.
///
/// The specification allows a header preceded by other bytes and asks readers to
/// look this far in; a file with nothing that looks like a header in its first
/// kilobyte is not a PDF, whatever it is called, and is answered `None` rather
/// than scanned for names that would then mean nothing.
const HEADER_WINDOW_BYTES: usize = 1024;

/// The deepest nesting of dictionaries the scan keeps frames for.
///
/// Real documents nest a handful deep. The cap is here because the stack is fed
/// by bytes that may not be structure at all — an unbalanced `<<` inside data
/// this scan mistook for markup would otherwise grow it without end — and going
/// over it costs only the `/Count` of something nested past any real document.
const MAX_DICT_DEPTH: usize = 64;

/// How many pages the PDF at `path` holds, or `None` when neither of its two
/// readers can say — see the module's own note for what each of them reads.
///
/// The scan first, because it is a stream over the file and answers most
/// documents; the parse behind it, because a document that packs its structure
/// into object streams is unreadable to any scan and is not thereby a document
/// without pages.
#[must_use]
pub fn page_count(path: &Path) -> Option<u32> {
    let file = std::fs::File::open(path).ok()?;
    if let Some(count) = count_pages(file) {
        return Some(count);
    }
    parse_page_count(read_capped(path)?)
}

/// [`page_count`]'s second reader: the page list of a parsed document.
///
/// `None` for everything [`page_raster`] refuses for — this is the same parse,
/// and a file that cannot be read has no count to give — and for a document
/// whose page list is empty, so that the card states the size alone rather than
/// "0 pages".
fn parse_page_count(bytes: Vec<u8>) -> Option<u32> {
    let pdf = hayro::hayro_syntax::Pdf::new(bytes).ok()?;
    u32::try_from(pdf.pages().len())
        .ok()
        .filter(|count| *count > 0)
}

/// **The file at `path`, read whole and bounded by [`MAX_RASTER_BYTES`]** — what
/// both of the parsing lanes need and neither may exceed.
///
/// The length is asked of the handle the bytes are then read through, and the
/// read is bounded by the cap rather than by that length: a file being appended
/// to between the two calls is a file this window may not be handed
/// unboundedly much of.
fn read_capped(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len == 0 || len > MAX_RASTER_BYTES {
        return None;
    }
    let mut bytes = Vec::with_capacity(usize::try_from(len).ok()?);
    (&file)
        .take(MAX_RASTER_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}

/// **The largest file this module will parse** — [`page_raster`], and the
/// second reader [`page_count`] falls back on.
///
/// The count's *scan* is a stream: it reads [`CHUNK_BYTES`] at a time and a
/// hundred-megabyte file costs it a hundred megabytes of *reading* and sixty-four
/// kilobytes of memory. A parse cannot be written that way — a PDF is read
/// through a cross-reference table that points anywhere in the file, so the
/// bytes have to be resident — and the number below is what keeps a hover from
/// being able to ask this process for arbitrarily much of them. A file over the
/// cap is answered by the scan or not at all, which is the honest shape: the
/// window will not spend a hundred megabytes to print a number.
///
/// It is set where real documents are not: a 128MiB PDF is a scan of a book, and
/// the card answers one by drawing its ground and printing its size, which is
/// the same thing it does for a file it cannot parse. Memory-mapping the file
/// instead would remove the cap and add `unsafe` plus a promise nobody can keep
/// — the mapping is torn out from under the reader if the file is truncated
/// while the card is up.
pub const MAX_RASTER_BYTES: u64 = 128 * 1024 * 1024;

/// The longest edge a raster may be asked for, which is [`hayro`]'s own limit
/// stated as ours: its pixmaps are addressed in `u16`. The card asks for a box a
/// few hundred pixels across, so this is a type boundary rather than a policy —
/// it is here so that the conversion below is a clamp instead of a cast that
/// could wrap.
const MAX_RASTER_EDGE_PX: u32 = u16::MAX as u32;

/// **One page, rastered** — straight (non-premultiplied) RGBA8, row-major, the
/// same shape every other picture in this window reaches the renderer as.
///
/// It is opaque: the page is drawn onto white, because that is the paper a PDF
/// assumes under it and a page composited onto the card's own ground would show
/// the terminal's background through everything its author left blank.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageRaster {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// **The first page of the PDF at `path`, fitted inside `fit_width` ×
/// `fit_height`**, or `None` for every one of the refusals in the module's own
/// note.
///
/// # Fitted, not resampled
///
/// The picture lane this joins on the card decodes at the file's native size and
/// then runs a Lanczos3 pass down to the display box, because a `.png` has one
/// resolution and that is the only way to get another. A page has no native
/// resolution at all — it is a description — so the fit is computed first and the
/// rasteriser is asked for exactly those pixels. The page is drawn once, at the
/// size it will be seen at, which is both cheaper than the two-step and sharper
/// than it.
///
/// # Where it may be called from
///
/// **Never the thread that draws.** Parsing and rastering a page is tens to
/// hundreds of milliseconds, and on the window's thread that is a hover over a
/// report freezing the window — the same sentence that already keeps
/// [`page_count`] on the preview worker. Its one caller is the decoration
/// worker, beside the formula engine and the image decoder, which is where this
/// window puts CPU work that answers a pointer.
#[must_use]
pub fn page_raster(path: &Path, index: u32, fit_width: u32, fit_height: u32) -> Option<PageRaster> {
    raster_page(read_capped(path)?, index, fit_width, fit_height)
}

/// [`page_raster`] over bytes already in hand — the seam the tests feed.
fn raster_page(bytes: Vec<u8>, index: u32, fit_width: u32, fit_height: u32) -> Option<PageRaster> {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;
    use hayro::vello_cpu::color::palette::css::WHITE;
    use hayro::{RenderCache, RenderSettings};

    let fit_width = fit_width.clamp(1, MAX_RASTER_EDGE_PX);
    let fit_height = fit_height.clamp(1, MAX_RASTER_EDGE_PX);

    let pdf = Pdf::new(bytes).ok()?;
    // **The page the caller asked for**, and `None` for one this document does
    // not have (user ruling 2026-08-26). It was `pages().first()` for as long as
    // the card showed a cover; a column that scrolls asks for each page in turn,
    // and an index past the end is an ordinary answer here rather than an error
    // — the count the card scrolls against is read by `page_count`, off the
    // file's structure, and a file whose structure over-reports is a file this
    // simply draws nothing for.
    let page = pdf.pages().get(usize::try_from(index).ok()?)?;
    // Already rotated: `render_dimensions` applies the page's own `/Rotate`, so a
    // landscape scan of a portrait page is fitted as the landscape it displays
    // as rather than as the portrait it is stored as.
    let (page_width, page_height) = page.render_dimensions();
    if !(page_width.is_finite() && page_height.is_finite() && page_width > 0.0 && page_height > 0.0)
    {
        return None;
    }
    // `contain`, not `cover`: the page keeps its proportions and the card centres
    // it in what is left over, which is the same bargain the picture body strikes
    // and is why a wide page and a tall one are both themselves.
    let scale = (fit_width as f32 / page_width).min(fit_height as f32 / page_height);
    // `.min(fit)` is not decoration: the limiting axis works out to `fit` exactly
    // in real arithmetic and to whatever the float said in this one, and a
    // raster one pixel wider than the ground it is centred on would be drawn
    // over the card's own border.
    let edge = |points: f32, fit: u32| ((points * scale).round().max(1.0) as u32).min(fit);
    let (width, height) = (edge(page_width, fit_width), edge(page_height, fit_height));

    let pixmap = hayro::render(
        page,
        // One page is rastered per hover, so the cache that exists to be reused
        // across the pages of one document has exactly one user here. It is
        // still passed rather than worked around: it is the argument, and a
        // document whose first page draws the same pattern twice pays for it
        // once.
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings {
            x_scale: scale,
            y_scale: scale,
            width: Some(width as u16),
            height: Some(height as u16),
            // The paper. See [`PageRaster`].
            bg_color: WHITE,
        },
    );
    Some(PageRaster {
        rgba: pixmap
            .take_unpremultiplied()
            .into_iter()
            .flat_map(|pixel| pixel.to_u8_array())
            .collect(),
        width,
        height,
    })
}

/// [`page_count`] over any stream of bytes — the seam the tests feed.
fn count_pages(mut reader: impl Read) -> Option<u32> {
    let mut scan = Scan::default();
    let mut buffer = vec![0_u8; CHUNK_BYTES];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => scan.feed(&buffer[..read]),
            // A file that stopped answering half way through is a file this
            // question has no answer about. The card says the size, which came
            // from a `metadata` call that already succeeded.
            Err(_) => return None,
        }
    }
    scan.finish()
}

/// One dictionary the scan is inside.
#[derive(Clone, Copy, Debug, Default)]
struct Dict {
    /// Whether this dictionary said `/Type /Pages`.
    pages: bool,
    /// Whatever `/Count` it declared.
    count: Option<u64>,
}

/// What the tokenizer is in the middle of reading.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Token {
    #[default]
    Between,
    /// `/Name` — the slash is eaten, the letters are collecting.
    Name,
    /// A bare keyword such as `obj`, `endobj` or `stream`.
    Word,
    /// An integer. `None` once it has overflowed, which no real count does and
    /// no wrong answer may come out of.
    Number(Option<u64>),
}

/// The longest token this scan needs to recognise is `endstream` at nine bytes;
/// anything longer cannot be one of the words it is looking for, so it is
/// collected up to here and then known to be something else.
const MAX_TOKEN_BYTES: usize = 16;

/// The tokenizer, and everything it has learned so far.
#[derive(Debug, Default)]
struct Scan {
    /// The first [`HEADER_WINDOW_BYTES`], kept until the header is found in
    /// them.
    head: Vec<u8>,
    header: bool,
    token: Token,
    /// The bytes of the token being collected, and whether it ran past
    /// [`MAX_TOKEN_BYTES`].
    word: [u8; MAX_TOKEN_BYTES],
    word_len: usize,
    word_over: bool,
    /// Dictionaries currently open, innermost last, and how many opens were
    /// dropped on the floor for standing past [`MAX_DICT_DEPTH`] — kept so the
    /// closes that answer them do not pop somebody else's frame.
    dicts: Vec<Dict>,
    dropped: usize,
    /// Whether the last byte was a `<` (or a `>`) still looking for its pair.
    angle_open: bool,
    angle_close: bool,
    /// The name just read was `/Type`, so the next name is a type's own name.
    expecting_type: bool,
    /// The name just read was `/Count`, so the next number is its value.
    expecting_count: bool,
    /// Inside a `stream`'s bytes: how many characters of `endstream` have
    /// matched so far, or `None` when this is structure rather than data.
    stream: Option<usize>,
    /// Inside a `%` comment, whose bytes are prose to the end of the line.
    comment: bool,
    /// The greatest `/Count` any `/Type /Pages` dictionary declared.
    tree: Option<u64>,
    /// How many `/Type /Page` dictionaries were seen.
    objects: u32,
}

/// The six bytes PDF calls white space, `NUL` included.
fn is_space(byte: u8) -> bool {
    matches!(byte, 0 | b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}

/// The bytes that end a token by being something else: PDF's delimiters.
fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// Whether `byte` may stand inside a name or a keyword.
fn is_regular(byte: u8) -> bool {
    !is_space(byte) && !is_delimiter(byte)
}

impl Scan {
    /// Take the next run of bytes.
    fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.byte(byte);
        }
    }

    fn byte(&mut self, byte: u8) {
        if !self.header {
            self.probe_header(byte);
        }
        // A stream's bytes are data — compressed, encrypted or simply binary —
        // and reading names out of them is how a scan invents pages that are not
        // there. Everything up to `endstream` is skipped whole, which is also
        // what makes the dictionary stack below trustworthy.
        if let Some(matched) = self.stream {
            self.stream_byte(byte, matched);
            return;
        }
        if self.comment {
            // A comment runs to the end of the line, and `/Type /Page` written
            // in one is a sentence about a file rather than a page in it.
            self.comment = !matches!(byte, b'\n' | b'\r');
            return;
        }
        if self.collecting(byte) {
            return;
        }
        self.begin(byte);
    }

    /// The first kilobyte, held until `%PDF-` turns up in it.
    fn probe_header(&mut self, byte: u8) {
        if self.head.len() < HEADER_WINDOW_BYTES {
            self.head.push(byte);
        }
        // Asked on every byte rather than once at the end, so the window can be
        // dropped the moment it has answered.
        if self.head.len() >= 5 && self.head[self.head.len() - 5..] == *b"%PDF-" {
            self.header = true;
            self.head = Vec::new();
        }
    }

    /// One byte of a stream's data: everything is skipped until `endstream`.
    fn stream_byte(&mut self, byte: u8, matched: usize) {
        const END: &[u8] = b"endstream";
        let matched = if byte == END[matched] {
            matched + 1
        } else if byte == END[0] {
            1
        } else {
            0
        };
        self.stream = (matched < END.len()).then_some(matched);
    }

    /// Feed `byte` to the token in progress; answer whether it was consumed.
    fn collecting(&mut self, byte: u8) -> bool {
        match self.token {
            Token::Between => false,
            Token::Name | Token::Word => {
                if is_regular(byte) {
                    self.push(byte);
                    return true;
                }
                let token = self.token;
                // Copied out before the token is handed over, so that the arms
                // below take `&mut self` with nothing borrowed out of it. A word
                // that ran past the buffer is `None` rather than its own first
                // sixteen bytes: none of the four names this scan knows is that
                // long, so a truncated one could only ever be a false match.
                let collected = self.word;
                let word = (!self.word_over).then_some(&collected[..self.word_len]);
                self.token = Token::Between;
                match token {
                    Token::Name => self.name(word),
                    _ => self.keyword(word),
                }
                false
            }
            Token::Number(value) => {
                if byte.is_ascii_digit() {
                    let digit = u64::from(byte - b'0');
                    self.token = Token::Number(
                        value.and_then(|value| value.checked_mul(10)?.checked_add(digit)),
                    );
                    return true;
                }
                self.token = Token::Between;
                self.number(value);
                false
            }
        }
    }

    /// `byte` is not part of what came before it: start whatever it starts.
    fn begin(&mut self, byte: u8) {
        let (was_open, was_close) = (self.angle_open, self.angle_close);
        self.angle_open = false;
        self.angle_close = false;
        match byte {
            b'/' => {
                self.token = Token::Name;
                self.word_len = 0;
                self.word_over = false;
            }
            b'%' => self.comment = true,
            b'<' => {
                if was_open {
                    self.open_dict();
                } else {
                    self.angle_open = true;
                }
            }
            b'>' => {
                if was_close {
                    self.close_dict();
                } else {
                    self.angle_close = true;
                }
            }
            byte if byte.is_ascii_digit() => {
                self.token = Token::Number(Some(u64::from(byte - b'0')));
            }
            byte if is_regular(byte) => {
                self.token = Token::Word;
                self.word_len = 0;
                self.word_over = false;
                self.push(byte);
            }
            _ => {}
        }
    }

    fn push(&mut self, byte: u8) {
        if self.word_len < MAX_TOKEN_BYTES {
            self.word[self.word_len] = byte;
            self.word_len += 1;
        } else {
            self.word_over = true;
        }
    }

    fn open_dict(&mut self) {
        if self.dicts.len() < MAX_DICT_DEPTH {
            self.dicts.push(Dict::default());
        } else {
            self.dropped += 1;
        }
    }

    /// A dictionary closed: if it was a page-tree node that declared a count,
    /// that count stands against every other node's.
    fn close_dict(&mut self) {
        if self.dropped > 0 {
            self.dropped -= 1;
            return;
        }
        let Some(dict) = self.dicts.pop() else {
            return;
        };
        if let Some(count) = dict.count.filter(|_| dict.pages) {
            self.tree = Some(self.tree.unwrap_or(0).max(count));
        }
    }

    /// One `/Name`, whole. `None` when it was too long to be any of the four
    /// this scan knows.
    fn name(&mut self, name: Option<&[u8]>) {
        // Whatever this name is, it is not the number `/Count` was waiting for.
        self.expecting_count = false;
        if std::mem::take(&mut self.expecting_type) {
            match name {
                Some(b"Page") => self.objects = self.objects.saturating_add(1),
                Some(b"Pages") => {
                    if let Some(dict) = self.dicts.last_mut() {
                        dict.pages = true;
                    }
                }
                _ => {}
            }
            return;
        }
        match name {
            Some(b"Type") => self.expecting_type = true,
            Some(b"Count") => self.expecting_count = true,
            _ => {}
        }
    }

    /// One bare keyword. Only `stream` matters: what follows it is data.
    fn keyword(&mut self, word: Option<&[u8]>) {
        self.expecting_type = false;
        self.expecting_count = false;
        if word == Some(&b"stream"[..]) {
            self.stream = Some(0);
        }
    }

    /// One integer, which is a page count only when `/Count` asked for it.
    fn number(&mut self, value: Option<u64>) {
        self.expecting_type = false;
        if std::mem::take(&mut self.expecting_count)
            && let Some(value) = value
            && let Some(dict) = self.dicts.last_mut()
        {
            dict.count = Some(value);
        }
    }

    /// The answer, once the bytes have run out.
    fn finish(mut self) -> Option<u32> {
        // A token pressed against the end of the file still counts: a page
        // object may be the last thing in a truncated download.
        self.byte(b' ');
        if !self.header {
            return None;
        }
        let tree = self
            .tree
            .filter(|count| *count > 0)
            .and_then(|count| u32::try_from(count).ok());
        tree.or_else(|| (self.objects > 0).then_some(self.objects))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture the ruling was demonstrated on — three pages, printed by
    /// headless Edge.
    const FIXTURE: &[u8] = include_bytes!("../../../test-assets/folio-pdf-test.pdf");

    /// The fixture the 2026-09-05 report was answered with — five pages, all of
    /// its structure inside one compressed object stream, so that nothing a scan
    /// looks for is anywhere in the bytes.
    const OBJSTM_FIXTURE: &[u8] = include_bytes!("../../../test-assets/folio-pdf-objstm-test.pdf");

    fn count(bytes: &[u8]) -> Option<u32> {
        count_pages(bytes)
    }

    /// A fixture's path on this disk — what [`page_count`] takes, since its
    /// second reader opens the file itself.
    fn fixture_path(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-assets")
            .join(name)
    }

    /// The card's own box at scale 1, so the assertions below are about the
    /// shape a reader actually gets rather than a number chosen for the test.
    const FIT: (u32, u32) = (280, 160);

    fn raster(bytes: &[u8]) -> Option<PageRaster> {
        raster_page(bytes.to_vec(), 0, FIT.0, FIT.1)
    }

    /// How many of the raster's pixels are not the paper it was drawn on.
    ///
    /// The page is rendered onto white, so "there is ink" and "this is not a
    /// blank rectangle" are the same count — and it is the only assertion that
    /// can tell a rasteriser that ran from one that returned a correctly sized
    /// nothing.
    fn ink(page: &PageRaster) -> usize {
        page.rgba
            .chunks_exact(4)
            .filter(|pixel| pixel[..3] != [255, 255, 255])
            .count()
    }

    /// PIN — **the shipped fixture's first page comes back as pixels, fitted,
    /// opaque, and with ink on it.**
    ///
    /// The one assertion in this module that a rasteriser cannot satisfy by
    /// agreeing with synthetic bytes: it is the file the ruling was demonstrated
    /// on, printed by headless Edge, and what is being checked is that a real
    /// producer's real page arrives drawn.
    ///
    /// The ink count is the assertion that tells a rasteriser which ran from one
    /// that handed back a correctly sized nothing, and it is why the count is
    /// here rather than a dimensions check alone.
    ///
    /// RED GATE: draw onto `TRANSPARENT` instead of the paper and the opacity
    /// assertion goes red — which on the card is a page with the terminal's
    /// background showing through everything its author left blank.
    #[test]
    fn the_shipped_fixture_rasters_its_first_page() {
        let page = raster(FIXTURE).expect("a real PDF's first page rasters");
        assert!(
            page.width <= FIT.0 && page.height <= FIT.1,
            "fitted inside the card's box: {}x{}",
            page.width,
            page.height
        );
        assert_eq!(
            page.rgba.len(),
            page.width as usize * page.height as usize * 4,
            "straight RGBA8, one row after another"
        );
        assert!(
            page.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255),
            "drawn onto paper, so every pixel is opaque"
        );
        assert!(
            ink(&page) > 100,
            "a page with text on it is not a blank rectangle: {} inked pixels",
            ink(&page)
        );
    }

    /// PIN — **the fit is `contain`: the page keeps its proportions.**
    ///
    /// The card centres the picture in a 280×160 ground, so a portrait page has
    /// to arrive narrow rather than arrive stretched — the same bargain the
    /// image body strikes, and the reason the raster is asked for at the fitted
    /// size instead of at the box's.
    ///
    /// MUTATION: pass `fit_width`/`fit_height` as the scale for each axis
    /// separately (`x_scale` and `y_scale` computed apart) and the aspect
    /// assertion goes red at the box's own 1.75.
    #[test]
    fn a_page_keeps_its_proportions_inside_the_box() {
        let page = raster(FIXTURE).expect("a real PDF's first page rasters");
        // The fixture is US Letter portrait: 612 × 792 points.
        let want = 612.0_f32 / 792.0;
        let got = page.width as f32 / page.height as f32;
        assert!(
            (got - want).abs() < 0.02,
            "portrait stays portrait: {}x{} is {got}, wanted {want}",
            page.width,
            page.height
        );
        // And a box the other way round fits by the other axis.
        let wide = raster_page(FIXTURE.to_vec(), 0, 160, 280).expect("still rasters");
        assert!(wide.width <= 160 && wide.height <= 280);
        assert!(
            (wide.width as f32 / wide.height as f32 - want).abs() < 0.02,
            "and by the other axis it is still the same page: {}x{}",
            wide.width,
            wide.height
        );
    }

    /// PIN — **a file this rasteriser cannot read answers `None`, and answering
    /// is all it does.**
    ///
    /// The card's whole degradation path is this word: no picture, the ground
    /// drawn empty, and the two fact lines — which come down another lane —
    /// untouched. A hover over a renamed archive, a half-downloaded report or a
    /// PDF whose xref is rubble must cost the same nothing.
    ///
    /// MUTATION: unwrap the `Pdf::new` result instead of `.ok()?` and every case
    /// below panics on the worker that was answering a hover.
    #[test]
    fn nothing_this_cannot_read_is_rastered_and_nothing_panics() {
        assert!(raster(b"not a pdf at all").is_none());
        assert!(raster(b"PK\x03\x04 not a pdf either").is_none());
        assert!(raster(b"").is_none());
        // A header and nothing behind it: no catalogue, no page tree, no pages.
        assert!(raster(b"%PDF-1.7\n").is_none());
        // Rubble that begins convincingly — the shape a truncated download takes.
        assert!(raster(b"%PDF-1.7\n1 0 obj\n<< /Type /Pages /Count 3 >>\nendo").is_none());
    }

    /// PIN — **a corrupted real PDF does not take the hover down with it.**
    ///
    /// Every prefix of the fixture is a file that could genuinely exist on a
    /// disk mid-copy, and each one is handed to the rasteriser. The assertion is
    /// not that any of them draws — most cannot — but that the answer is always
    /// an answer: `None`, or a picture that fits the box it was asked for.
    ///
    /// It is worth a test of its own rather than being folded into the refusals
    /// above because a *nearly* valid PDF is the input that gets furthest into a
    /// parser before it goes wrong, and this lane's failure mode is not a wrong
    /// picture — it is the decoration worker dying on a hover, taking the
    /// formula engine and the image decoder down with it for the rest of the
    /// session.
    ///
    /// MUTATION: `Pdf::new(bytes).unwrap()` — several of these prefixes panic,
    /// and each one is that worker.
    #[test]
    fn every_truncation_of_a_real_pdf_is_survived() {
        for cut in (0..FIXTURE.len()).step_by(FIXTURE.len() / 32 + 1) {
            let Some(page) = raster(&FIXTURE[..cut]) else {
                continue;
            };
            assert!(
                page.width >= 1 && page.width <= FIT.0,
                "cut at {cut}: width {}",
                page.width
            );
            assert!(
                page.height >= 1 && page.height <= FIT.1,
                "cut at {cut}: height {}",
                page.height
            );
            assert_eq!(
                page.rgba.len(),
                page.width as usize * page.height as usize * 4,
                "cut at {cut}"
            );
        }
    }

    /// PIN — **a file larger than [`MAX_RASTER_BYTES`] is not read into this
    /// process.**
    ///
    /// The cap is the only thing standing between a hover and an unbounded
    /// allocation, because a rasteriser — unlike the counter above it — needs
    /// the whole file resident. Asserted through [`page_raster`]'s own door on a
    /// real file, because the cap is a `metadata` call and a `take`, and neither
    /// is visible from [`raster_page`].
    ///
    /// MUTATION: drop the `len > MAX_RASTER_BYTES` guard and this file is read
    /// whole; drop the `.take(MAX_RASTER_BYTES)` and a file that grows between
    /// the stat and the read is too.
    #[test]
    fn a_file_past_the_cap_is_refused_before_it_is_read() {
        let dir = std::env::temp_dir().join(format!(
            "folio-pdf-cap-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let path = dir.join("huge.pdf");
        let file = std::fs::File::create(&path).expect("a scratch file");
        // Sparse on NTFS: the bytes are never written and the length is real,
        // which is exactly the file the guard has to answer about.
        file.set_len(MAX_RASTER_BYTES + 1).expect("a stated length");
        drop(file);
        assert_eq!(page_raster(&path, 0, FIT.0, FIT.1), None);
        // And the guard is about the size and not about the emptiness: the same
        // file one byte under the cap is read, and answers `None` because it is
        // zeros rather than because it was refused unopened.
        std::fs::remove_file(&path).expect("the scratch file goes");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// PIN — **a file that is not there, and a file with nothing in it, are both
    /// `None` rather than either of them being an error.**
    #[test]
    fn a_missing_or_empty_file_has_no_first_page() {
        let missing = std::env::temp_dir().join("folio-no-such-file-at-all.pdf");
        assert_eq!(page_raster(&missing, 0, FIT.0, FIT.1), None);
    }

    /// RED — **every page of the fixture draws, and the page after the last one
    /// does not** (user ruling 2026-08-26; the card's column).
    ///
    /// The whole of what the index argument bought: a column that scrolls asks
    /// for page 1 and page 2 by number and must get *different* pictures, not
    /// three copies of the cover. Ink is what separates them — a rasteriser that
    /// silently answered page 0 for every index would return three identical
    /// buffers, and the equality assertion is the one that catches it.
    ///
    /// And the end of the document is an ordinary `None`: the count the card
    /// scrolls against is read by a different reader ([`count_pages`]) off a
    /// different part of the file, so the two can disagree, and the disagreement
    /// must be a blank slot rather than a panic.
    ///
    /// RED GATE: go back to `pages().first()` and the two "different page"
    /// assertions fail while everything else stays green — which is exactly how
    /// the defect would have shipped.
    #[test]
    fn each_page_of_the_fixture_draws_as_itself_and_the_fourth_does_not() {
        let pages: Vec<PageRaster> = (0..3)
            .map(|index| {
                raster_page(FIXTURE.to_vec(), index, FIT.0, FIT.1)
                    .unwrap_or_else(|| panic!("page {index} of a three-page fixture rasters"))
            })
            .collect();
        for page in &pages {
            assert!(
                ink(page) > 100,
                "every page of the fixture has something drawn on it"
            );
        }
        assert_ne!(
            pages[0].rgba, pages[1].rgba,
            "page 2 is not page 1 drawn again"
        );
        assert_ne!(
            pages[1].rgba, pages[2].rgba,
            "and page 3 is not page 2 drawn again"
        );
        assert_eq!(
            raster_page(FIXTURE.to_vec(), 3, FIT.0, FIT.1),
            None,
            "a three-page document has no fourth page, and says so quietly"
        );
    }

    /// PIN — **a real PDF is counted, and it is counted from the tree.**
    ///
    /// The fixture is the file the ruling itself was demonstrated on, so this is
    /// the one assertion in the module that cannot be satisfied by a scan that
    /// merely agrees with its own synthetic bytes.
    ///
    /// RED GATE: return the page-object tally before the tree's `/Count` and
    /// this still passes — the two agree on a well-formed file — but drop either
    /// route and the fixture goes to `None`.
    #[test]
    fn the_shipped_fixture_holds_three_pages() {
        assert_eq!(count(FIXTURE), Some(3));
    }

    /// PIN — **a document that compresses its structure is still counted**
    /// (user report 2026-09-05; `docs/DESIGN.md` §7.10 ⑥).
    ///
    /// The reported file was a four-page paper out of `pdflatex`, whose card
    /// showed its first page and the size and no page count at all — and a card
    /// with no count has a column one page long, so the wheel had nothing to
    /// turn either. The fixture here is that file's shape with none of its
    /// content: catalogue, page tree and all five pages inside one Flate
    /// `/Type /ObjStm`, reached through a cross-reference stream
    /// (`scripts/dev/make-objstm-pdf.py` writes it).
    ///
    /// Both halves are asserted, because the defect was that the first half is
    /// the *whole* answer:
    ///
    /// * the scan finds nothing in it — no `/Type /Pages`, no `/Count`, no
    ///   `/Type /Page` survives compression;
    /// * and [`page_count`] answers 5 anyway, off the parse behind the scan.
    ///
    /// RED GATE: delete the `parse_page_count` arm from [`page_count`] and this
    /// goes back to `None`, which is the bug exactly.
    #[test]
    fn a_document_with_its_structure_in_an_object_stream_is_counted() {
        assert_eq!(
            count(OBJSTM_FIXTURE),
            None,
            "the scan cannot read a compressed page tree, and must not pretend to"
        );
        assert_eq!(
            page_count(&fixture_path("folio-pdf-objstm-test.pdf")),
            Some(5)
        );
    }

    /// PIN — **the scan's answer is the one that stands when it has one.**
    ///
    /// The parse is behind the scan and not in front of it: the Skia fixture's
    /// structure is in the clear, so its count is read off [`CHUNK_BYTES`] of
    /// memory rather than off a resident copy of the file, and the number is the
    /// same three the scan has always said.
    ///
    /// MUTATION: put the parse first and this still passes — the two agree —
    /// which is why the assertion below is about the *scan* answering on its
    /// own, and it is `the_shipped_fixture_holds_three_pages` next door that
    /// pins it.
    #[test]
    fn a_document_in_the_clear_is_counted_without_being_parsed() {
        let path = fixture_path("folio-pdf-test.pdf");
        assert_eq!(count(FIXTURE), Some(3), "the scan answers this one alone");
        assert_eq!(page_count(&path), Some(3));
    }

    /// PIN — **a document with nothing in it is `None` through both readers.**
    ///
    /// The parse was added behind the scan, so every refusal the scan used to
    /// make now has a second chance to become a wrong answer. It may not: a
    /// `.pdf` that is prose, an empty file, a missing one, and **a well-formed
    /// document whose page tree is empty** are all the same silence, and the
    /// card states the size it already has.
    ///
    /// RED GATE: drop the `> 0` filter from `parse_page_count` and the last row
    /// is a card that says "0 pages" — hayro parses that file happily and
    /// answers with an empty page list (measured 2026-09-05), so the filter is
    /// the only thing between a document with no pages and a fact about it that
    /// is not one.
    #[test]
    fn neither_reader_invents_a_count_for_what_is_not_a_document() {
        let dir = std::env::temp_dir().join("folio-pdf-count-none-3f1a");
        std::fs::create_dir_all(&dir).expect("a directory under the temp dir");
        for (name, bytes) in [
            (
                "not-a-pdf.pdf",
                &b"this file is prose and calls itself a pdf"[..],
            ),
            ("empty.pdf", &b""[..]),
            (
                "no-pages.pdf",
                &b"%PDF-1.4\n\
                   1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                   2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
                   trailer\n<< /Size 3 /Root 1 0 R >>\n%%EOF\n"[..],
            ),
        ] {
            let path = dir.join(name);
            std::fs::write(&path, bytes).expect("a fixture written under the temp dir");
            assert_eq!(page_count(&path), None, "{name}");
        }
        assert_eq!(page_count(&dir.join("no-such-file.pdf")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// PIN — **the page tree's own `/Count` is the answer when it is there.**
    ///
    /// MUTATION: take the first `/Count` instead of the greatest and the split
    /// tree below comes back as 2, which is one node's share rather than the
    /// document.
    #[test]
    fn the_count_comes_off_the_page_tree() {
        let pdf = b"%PDF-1.7\n\
            1 0 obj\n<< /Type /Pages /Kids [2 0 R 3 0 R] /Count 5 >>\nendobj\n\
            2 0 obj\n<< /Type /Pages /Kids [4 0 R 5 0 R] /Count 2 >>\nendobj\n\
            3 0 obj\n<< /Type /Pages /Kids [6 0 R] /Count 3 >>\nendobj\n";
        assert_eq!(count(pdf), Some(5));
    }

    /// PIN — **with no tree in reach, the page objects are counted.**
    ///
    /// MUTATION: delete the `objects` fallback and this file — which is a
    /// perfectly ordinary shape for a hand-written PDF — answers `None`.
    #[test]
    fn page_objects_are_counted_when_no_tree_declares_itself() {
        let pdf = b"%PDF-1.4\n\
            1 0 obj\n<</Type/Page/Parent 9 0 R>>\nendobj\n\
            2 0 obj\n<</Type/Page/Parent 9 0 R>>\nendobj\n";
        assert_eq!(count(pdf), Some(2));
    }

    /// PIN — **`/Count` belongs to the dictionary that wrote it.**
    ///
    /// An outline with twelve entries is not a document with twelve pages, and a
    /// scan that took the greatest `/Count` in the file without asking whose it
    /// was would say it was.
    ///
    /// MUTATION: record every `/Count` rather than a `/Pages` dictionary's own —
    /// the answer becomes 12.
    #[test]
    fn an_outlines_count_is_not_a_page_count() {
        let pdf = b"%PDF-1.4\n\
            1 0 obj\n<< /Type /Outlines /First 5 0 R /Count 12 >>\nendobj\n\
            2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 2 >>\nendobj\n";
        assert_eq!(count(pdf), Some(2));
    }

    /// PIN — **a stream's bytes are data, not structure.**
    ///
    /// Content streams are compressed and may hold any byte sequence at all,
    /// `/Type /Page` included; a scan that read them would report pages that do
    /// not exist on files it has no way of recognising as unusual.
    ///
    /// MUTATION: drop the `stream`/`endstream` skip and the answer becomes 3.
    #[test]
    fn bytes_inside_a_stream_are_not_read_as_structure() {
        let pdf = b"%PDF-1.4\n\
            1 0 obj\n<< /Type /Page >>\nendobj\n\
            2 0 obj\n<< /Length 40 >>\nstream\n/Type /Page /Type /Page << /Count 9\nendstream\nendobj\n";
        assert_eq!(count(pdf), Some(1));
    }

    /// PIN — **a comment is prose.**
    ///
    /// MUTATION: stop skipping to the end of the line and the commented page is
    /// counted, which makes any producer that annotates its output wrong by
    /// however many notes it wrote.
    #[test]
    fn a_comment_is_not_a_page() {
        let pdf = b"%PDF-1.4\n\
            % a note: /Type /Page is what the next object says\n\
            1 0 obj\n<< /Type /Page >>\nendobj\n";
        assert_eq!(count(pdf), Some(1));
    }

    /// PIN — **`/Pages` is not `/Page`.**
    ///
    /// The two names differ by one letter and the tokenizer is what tells them
    /// apart: a prefix match would count every node of the tree as a page.
    ///
    /// MUTATION: compare with `starts_with(b"Page")` and this answers 3.
    #[test]
    fn a_tree_node_is_not_one_of_the_pages_it_holds() {
        let pdf = b"%PDF-1.4\n\
            1 0 obj\n<< /Type /Pages /Kids [2 0 R] >>\nendobj\n\
            2 0 obj\n<< /Type /Page >>\nendobj\n\
            3 0 obj\n<< /Type /Page >>\nendobj\n";
        assert_eq!(count(pdf), Some(2));
    }

    /// PIN — **a file that is not a PDF has no page count**, whatever it is
    /// called.
    ///
    /// The card's fallback is the ruling's: no number, and the size alone. A
    /// renamed archive that happened to contain the letters `/Type /Page` would
    /// otherwise be reported as a document with pages in it.
    ///
    /// MUTATION: drop the header probe and the second case answers `Some(1)`.
    #[test]
    fn nothing_but_a_pdf_is_counted() {
        assert_eq!(count(b"not a pdf at all"), None);
        assert_eq!(count(b"PK\x03\x04 /Type /Page"), None);
        // And a PDF with nothing this scan can see — a page tree packed into an
        // object stream — is `None` rather than a number.
        assert_eq!(
            count(b"%PDF-1.5\n1 0 obj\n<< /Type /ObjStm >>\nendobj\n"),
            None
        );
    }

    /// PIN — **no token is lost at a chunk boundary.**
    ///
    /// The scan is a state machine for exactly this reason, and the reason is
    /// worth a test rather than a comment: a buffered search with an overlap
    /// would answer differently depending on how the operating system happened
    /// to split the read.
    ///
    /// MUTATION: reset the tokenizer between chunks — every count the boundary
    /// falls inside goes missing.
    #[test]
    fn a_token_split_across_two_reads_is_still_one_token() {
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Pages /Count 7 >>\nendobj\n";
        for cut in 1..pdf.len() {
            let mut scan = Scan::default();
            scan.feed(&pdf[..cut]);
            scan.feed(&pdf[cut..]);
            assert_eq!(scan.finish(), Some(7), "cut at {cut}");
        }
    }
}
