//! **How many pages a PDF holds, read off its own structure** (user ruling
//! 2026-08-25; `docs/DESIGN.md` §7.10 ⑥).
//!
//! # Why this exists at all
//!
//! The glance card cannot render a page — a page is drawn by the engine, on a
//! seat, and there is no engine on a hover card. What the card can do is state
//! the facts a reader hovers a `.pdf` to learn, and the ruling names two: how
//! large the file is, and how many pages are in it. The first is a `metadata`
//! call. The second is this module.
//!
//! **Nothing here renders, decompresses or resolves anything.** It is a
//! tokenizer over the file's own bytes that answers one question, and the
//! ruling's own fallback is written into the answer's type: when the structure
//! does not yield a count, the answer is `None` and the card states the size
//! alone. A wrong number would be worse than no number, so every place this
//! scan cannot see is a `None` rather than a guess.
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
//! # What it cannot see, and says so
//!
//! **Object streams** (PDF 1.5+): a file may pack its catalogue, its page tree
//! and its page objects into compressed streams, and none of the three is
//! readable without inflating them. The scan then finds neither a `/Count` nor a
//! `/Page`, answers `None`, and the card states the size — which is the ruling's
//! own instruction for exactly this case. Inflating an object stream is a
//! decompressor, an xref parse and a cross-reference resolver, which is a PDF
//! reader; this window has one already and it is the engine a double click
//! opens.
//!
//! **A `/Count` written as an indirect reference** (`/Count 12 0 R`) is legal
//! and is read here as the object number it begins with. It is not written by
//! any producer this window has met, and the alternative — resolving references
//! — is the same reader again.

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

/// How many pages the PDF at `path` holds, or `None` when its structure does not
/// say — see the module's own note for the three ways that happens.
#[must_use]
pub fn page_count(path: &Path) -> Option<u32> {
    let file = std::fs::File::open(path).ok()?;
    count_pages(file)
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

    fn count(bytes: &[u8]) -> Option<u32> {
        count_pages(bytes)
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
