//! The preview's content plane: what a file *is*, the tab's shared pool of live
//! buffers, and the thread that reads their heads off the event loop.
//!
//! **A buffer belongs to a FILE; a pane is a VIEW** (`DESIGN.md` §7.1.3, user
//! ruling 2026-07-17, which moved buffer ownership up from the pane to the tab).
//! Each tab owns one shared pool: a file open in two panes *is* the same buffer,
//! so edits cannot fork, and unsaved edits survive switching with no prompt at
//! all. The pool is a **history, not a second tab system** — clean buffers are
//! capped at [`PV_BUFFER_CAP`] and regrow on demand, while dirty ones and ones a
//! pane is currently showing are never evicted. Pane count grows only through the
//! explicit pin in the header.
//!
//! **Why a thread.** The same sentence `files` opens with: a file is not a data
//! structure, it is a question for a disk. §7.1.3 asks for an asynchronous,
//! cancellable head read of at most [`PREVIEW_HEAD_BYTES`], and this module owns
//! the shape `bt-files-worker` already owns — a named thread, a request channel,
//! a response channel, an [`AppEvent`] to wake the loop, newest-per-target
//! coalescing, and a one-way degradation when the thread is gone. Not a second
//! paradigm; the same one, on its own lane, so a slow file cannot sit behind a
//! slow directory.
//!
//! **What is decided here rather than there.** Two of §7.1.3's refusals are
//! answerable without touching the disk — a type this window has no reader for,
//! and a network path, which the design says is not to be read *automatically*
//! at all — so [`PreviewBuffer::new`] settles them and no request is ever sent.
//! The two that need bytes, a binary body and a file past the head limit, are the
//! worker's to answer.

use std::io::Read;
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::mpsc;
use std::thread;
use std::time::SystemTime;

use anyhow::{Context, Result};
use winit::event_loop::EventLoopProxy;

use crate::{AppEvent, TabId};

/// How many buffers one tab's pool keeps.
///
/// `DESIGN.md` §7.1.3 writes the number down as the prototype's ("原型 8"),
/// which is to say it is a policy dial and not a law: what is load-bearing is
/// that the ceiling exists and that [`PreviewPool::open`] refuses to buy room
/// for it with anything a user would miss.
pub const PV_BUFFER_CAP: usize = 8;

/// How much of a file a preview reads.
///
/// §7.1.3's number, and the reason it is a *head* read rather than a whole one:
/// a preview is a look, and a look at the first screenful of a gigabyte costs
/// exactly as much as a look at the first screenful of a kilobyte. Past this the
/// buffer is marked truncated, which is what makes the read-only degradation the
/// design asks for expressible rather than silent.
pub const PREVIEW_HEAD_BYTES: usize = 64 * 1024;

/// The notice shown once when the preview worker has stopped.
///
/// Worded like [`crate::files::FILES_WORKER_STOPPED_NOTICE`] and for the same
/// reason: a worker dying is a feature going away, not a session ending, and the
/// sentence has to say which half still works.
pub const PREVIEW_WORKER_STOPPED_NOTICE: &str =
    "File preview reading stopped; terminal input and output remain available";

/// What kind of document a name claims to be.
///
/// The mock-up's `previewFtype` (3087-3096), variant for variant. It is asked of
/// the *name* and nothing else — no magic numbers, no MIME sniffing — because the
/// answer has to be available before anything has been read, and because a name
/// is what a user is looking at when they decide what they expect to see. What a
/// name cannot know is answered later and separately: a `.txt` full of NULs is
/// still `Text` here and is refused by [`read_head`] on the evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewFtype {
    Image,
    Markdown,
    Table,
    Text,
    /// No reader in this window. The "no preview" card, by name alone.
    Unknown,
}

/// Extensions that name a picture — the mock-up's list (3090).
const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "svg", "gif", "webp"];

/// Extensions that name something this window can show as text (3093).
const TEXT_EXTENSIONS: [&str; 14] = [
    "rs",
    "py",
    "js",
    "ts",
    "json",
    "toml",
    "html",
    "txt",
    "gitignore",
    "lock",
    "yml",
    "yaml",
    "diff",
    "patch",
];

/// The extension a name carries, lower-cased, or `""` when it carries none.
fn extension_of(name: &str) -> String {
    match name.rfind('.') {
        Some(dot) => name[dot + 1..].to_ascii_lowercase(),
        None => String::new(),
    }
}

/// Which of the five classes a file name belongs to.
pub fn preview_ftype(name: &str) -> PreviewFtype {
    let ext = extension_of(name);
    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return PreviewFtype::Image;
    }
    if ext == "md" {
        return PreviewFtype::Markdown;
    }
    if ext == "csv" {
        return PreviewFtype::Table;
    }
    // A name that begins with a dot is a dotfile — `.env`, `.gitattributes` —
    // and dotfiles are text by convention rather than by extension, which is
    // exactly why the name and not the extension is asked here.
    if TEXT_EXTENSIONS.contains(&ext.as_str()) || name.starts_with('.') {
        return PreviewFtype::Text;
    }
    PreviewFtype::Unknown
}

/// Whether a name is a patch, which is read and never edited.
///
/// `#[allow(dead_code)]`: this and [`is_editable`] are the block's *judgement*
/// about which surface a file gets, and the surfaces that consume it — the diff
/// view and the quick-edit textarea — are slices 2 and 3. The judgement is
/// written and pinned by test now rather than then, because the mock-up got it
/// wrong twice in two places (P58/P107) and a rule discovered at the second call
/// site is a rule the first one has already broken.
///
/// The judgement is the name's rather than the type's because `.diff` and
/// `.patch` sit inside the text list: they are text, they are shown as a diff,
/// and *editing a diff edits nothing real* (mock-up 4970-4978).
#[allow(dead_code)]
pub fn is_diff_name(name: &str) -> bool {
    matches!(extension_of(name).as_str(), "diff" | "patch")
}

/// Whether this content would be shown on a surface that edits.
///
/// **The judgement is about the surface, not about the extension** (ruling 3,
/// 2026-08-12). The mock-up asked it twice and got two different answers — the
/// pane said `text || markdown`, the float said `text || table || (markdown &&
/// mdSource)` — and both were wrong in the same way: they named types rather
/// than the view those types actually get. A table is a grid and a diff is a
/// reading, so neither is editable however much its extension looks like text,
/// and a rendered markdown view is not an editor until it has been flipped to
/// source.
pub fn is_editable(name: &str, ftype: PreviewFtype, md_source: bool) -> bool {
    if is_diff_name(name) {
        return false;
    }
    match ftype {
        PreviewFtype::Text => true,
        PreviewFtype::Markdown => md_source,
        PreviewFtype::Image | PreviewFtype::Table | PreviewFtype::Unknown => false,
    }
}

/// Which body a buffer is drawn as.
///
/// The mock-up's `previewBodyHtml` (4942-4988) is a ladder of `if`s and **the
/// order is a ruling**, not an accident of writing. Kept as one function
/// returning one value so that every surface — the pane, the float, the hover
/// card, and whatever asks next — reads the same answer rather than
/// re-descending a ladder of its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewView {
    Image,
    /// Markdown, rendered. The source view is [`Self::Text`].
    Markdown,
    Table,
    Diff,
    Text,
    /// The "no preview" card.
    None,
}

/// Which body this name, type and flip state earn.
pub fn preview_view(name: &str, ftype: PreviewFtype, md_source: bool) -> PreviewView {
    if ftype == PreviewFtype::Image {
        return PreviewView::Image;
    }
    if ftype == PreviewFtype::Markdown && !md_source {
        return PreviewView::Markdown;
    }
    if ftype == PreviewFtype::Table {
        return PreviewView::Table;
    }
    // **Before the text surface, which is the whole point.** `.diff` is text by
    // extension, so asking the type first would hand a patch a textarea — and
    // editing a diff edits nothing real.
    if is_diff_name(name) {
        return PreviewView::Diff;
    }
    if ftype == PreviewFtype::Text || (ftype == PreviewFtype::Markdown && md_source) {
        return PreviewView::Text;
    }
    PreviewView::None
}

/// What one line of a diff is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffLineKind {
    /// `+++`, `---`, `diff …` — the header, which is about the *file*.
    Meta,
    /// `@@ … @@` — where in the file the next lines are.
    Hunk,
    Add,
    Del,
    /// Everything else: the unchanged lines a hunk carries for context.
    Context,
}

impl DiffLineKind {
    /// Whether this line stands on a coloured band.
    pub fn tints(self) -> bool {
        matches!(self, Self::Add | Self::Del)
    }
}

/// Classify one line of a diff.
///
/// **The three-character prefixes are asked first**, and that ordering is the
/// only thing keeping `--- a/src/main.rs` — a diff's own header — out of the red
/// band it would otherwise be painted in. The mock-up gets this right at
/// 4973-4976 and it is easy to get wrong by writing the shorter test first.
pub fn diff_line_kind(line: &str) -> DiffLineKind {
    if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") {
        DiffLineKind::Meta
    } else if line.starts_with("@@") {
        DiffLineKind::Hunk
    } else if line.starts_with('+') {
        DiffLineKind::Add
    } else if line.starts_with('-') {
        DiffLineKind::Del
    } else {
        DiffLineKind::Context
    }
}

/// How one run of inline text is set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpanStyle {
    Plain,
    Bold,
    /// `` `like this` `` — set in the monospace face.
    Code,
}

/// One run of inline text inside a markdown block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: SpanStyle,
}

impl Span {
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            style: SpanStyle::Plain,
        }
    }

    pub fn bold(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            style: SpanStyle::Bold,
        }
    }

    pub fn code(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            style: SpanStyle::Code,
        }
    }
}

/// One block of a rendered markdown document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MarkdownBlock {
    Heading {
        level: u8,
        spans: Vec<Span>,
    },
    /// Consecutive `-`/`*` rows, gathered the way the mock-up's `flushList`
    /// gathers them: a list is one block, not one block per bullet.
    List(Vec<Vec<Span>>),
    Code {
        lang: Option<String>,
        text: String,
    },
    Paragraph(Vec<Span>),
}

/// Split one line into its inline runs.
///
/// Backticks first and asterisks second, which is the mock-up's order (4915-4917)
/// and the one that makes `` `**not bold**` `` come out as literal code rather
/// than as a bold run inside a code span.
pub fn parse_inline(line: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        // A backtick with no partner is a backtick, not the start of anything.
        let Some(close) = rest[open + 1..].find('`') else {
            break;
        };
        push_bold_runs(&rest[..open], &mut spans);
        spans.push(Span::code(&rest[open + 1..open + 1 + close]));
        rest = &rest[open + 1 + close + 1..];
    }
    push_bold_runs(rest, &mut spans);
    spans
}

/// The second pass: `**bold**` inside whatever the code pass left plain.
fn push_bold_runs(text: &str, spans: &mut Vec<Span>) {
    let mut rest = text;
    while let Some(open) = rest.find("**") {
        let Some(close) = rest[open + 2..].find("**") else {
            break;
        };
        // `****` is not an empty bold run, it is four asterisks — the mock-up's
        // `[^*]+` refuses to match nothing, and so does this.
        if close == 0 {
            break;
        }
        push_plain(&rest[..open], spans);
        spans.push(Span::bold(&rest[open + 2..open + 2 + close]));
        rest = &rest[open + 2 + close + 2..];
    }
    push_plain(rest, spans);
}

fn push_plain(text: &str, spans: &mut Vec<Span>) {
    if !text.is_empty() {
        spans.push(Span::plain(text));
    }
}

/// A deliberately small markdown renderer: headings, lists, fences, inline code
/// and bold.
///
/// The mock-up's own support surface (4910-4941), and its own reason for
/// stopping there: "enough for the rendered view to be an honest mirror of the
/// editable source — completeness is the product's problem". What is *not*
/// negotiable is the honesty: this renders **the argument**, because the
/// prototype's first rendered view was a static mock that showed the same
/// document whatever the buffer held (P103).
pub fn parse_markdown(src: &str) -> Vec<MarkdownBlock> {
    let mut blocks = Vec::new();
    let mut fence: Option<(Option<String>, Vec<&str>)> = None;
    let mut list: Vec<Vec<Span>> = Vec::new();

    for line in src.lines() {
        if let Some(rest) = line.strip_prefix("```") {
            match fence.take() {
                None => {
                    flush_list(&mut list, &mut blocks);
                    let lang = rest.trim();
                    fence = Some(((!lang.is_empty()).then(|| lang.to_owned()), Vec::new()));
                }
                Some((lang, body)) => blocks.push(MarkdownBlock::Code {
                    lang,
                    text: body.join("\n"),
                }),
            }
            continue;
        }
        if let Some((_, body)) = fence.as_mut() {
            body.push(line);
            continue;
        }
        if let Some(heading) = parse_heading(line) {
            flush_list(&mut list, &mut blocks);
            blocks.push(heading);
            continue;
        }
        if let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            list.push(parse_inline(item));
            continue;
        }
        flush_list(&mut list, &mut blocks);
        if !line.trim().is_empty() {
            blocks.push(MarkdownBlock::Paragraph(parse_inline(line)));
        }
    }
    flush_list(&mut list, &mut blocks);
    // A fence nobody closed still draws, rather than swallowing the rest of the
    // document in silence (mock-up 4939).
    if let Some((lang, body)) = fence {
        blocks.push(MarkdownBlock::Code {
            lang,
            text: body.join("\n"),
        });
    }
    blocks
}

/// `#`, `##` or `###` followed by a space — three levels, exactly as the
/// mock-up's `#{1,3}` allows.
fn parse_heading(line: &str) -> Option<MarkdownBlock> {
    let hashes = line.len() - line.trim_start_matches('#').len();
    if !(1..=3).contains(&hashes) {
        return None;
    }
    let rest = line[hashes..].strip_prefix(' ')?;
    Some(MarkdownBlock::Heading {
        level: hashes as u8,
        spans: parse_inline(rest),
    })
}

fn flush_list(list: &mut Vec<Vec<Span>>, blocks: &mut Vec<MarkdownBlock>) {
    if !list.is_empty() {
        blocks.push(MarkdownBlock::List(std::mem::take(list)));
    }
}

/// Split a comma-separated file into rows of cells.
///
/// **Quote-aware**, which the mock-up's `r.split(",")` is not. The prototype's
/// own fixture has no quoted fields so the naive split was never wrong there;
/// a real file has them, and a grid whose columns shift at the first quoted
/// comma is not a table. Everything else is the mock-up's (4963-4968): no
/// sorting, no frozen header, no paging.
pub fn csv_rows(content: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = content.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted => {
                // `""` inside a quoted field is one literal quote.
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            }
            '"' => quoted = true,
            ',' if !quoted => cells.push(std::mem::take(&mut cell)),
            '\n' if !quoted => {
                cells.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut cells));
            }
            '\r' if !quoted => {}
            other => cell.push(other),
        }
    }
    if !cell.is_empty() || !cells.is_empty() {
        cells.push(cell);
        rows.push(cells);
    }
    // A file that is nothing but whitespace is an empty table, not a table with
    // one empty cell — the mock-up trims before it splits.
    rows.retain(|row| row.iter().any(|cell| !cell.trim().is_empty()));
    rows
}

/// `.pv-edit { tab-size: 4 }` (mock-up 603).
///
/// Expanded into spaces on the way to the shaper rather than shaped as a tab,
/// because a tab is a *stop* and cosmic-text has no tab stops: what the CSS
/// property names is a column grid, and in a monospace face a column grid is
/// exactly N columns.
pub const PREVIEW_TEXT_TAB_WIDTH: usize = 4;

/// Expand tab stops the way `tab-size: 4` does.
///
/// **Column-aware, not a blind replace.** A tab advances to the next multiple of
/// four *columns*, so a tab after two characters is worth two spaces and a tab
/// at the start of a line is worth four. Replacing each with four spaces is what
/// misaligns every continuation line of an indented file, which is precisely
/// what a preview of source code is for. Columns and not characters, so a wide
/// character counts for the two cells it draws in.
pub fn expand_tabs(line: &str) -> String {
    if !line.contains('\t') {
        return line.to_owned();
    }
    let mut out = String::with_capacity(line.len());
    let mut column = 0usize;
    for cluster in bt_unicode::graphemes(line) {
        if cluster == "\t" {
            let advance = PREVIEW_TEXT_TAB_WIDTH - column % PREVIEW_TEXT_TAB_WIDTH;
            out.extend(std::iter::repeat_n(' ', advance));
            column += advance;
        } else {
            out.push_str(cluster);
            column += bt_unicode::cluster_width(cluster);
        }
    }
    out
}

/// How wide the widest line of a body is, in the columns it will draw as.
///
/// **Columns, not bytes and not characters.** It is what the horizontal
/// scroller's extent is derived from, so a wide character measured as one column
/// would leave the end of its own line permanently unreachable.
fn widest_line_columns(text: &str) -> usize {
    text.lines()
        .map(|line| bt_unicode::text_width(&expand_tabs(line)))
        .max()
        .unwrap_or(0)
}

/// The read-only degradation §7.1.3 asks for, as a sentence.
pub const PREVIEW_TRUNCATED_NOTICE: &str = "Read-only — showing the first 64 KB of this file";

/// A byte count the way a file manager says it.
///
/// Binary units, because that is what Explorer's own column shows on this
/// platform and a preview that disagreed with the property sheet beside it would
/// be the one making the user check.
pub fn format_byte_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    match bytes {
        b if b < KB => format!("{b} B"),
        b if b < MB => format!("{} KB", b.div_ceil(KB / 2) / 2),
        b if b < GB => format!("{:.1} MB", b as f64 / MB as f64),
        b => format!("{:.1} GB", b as f64 / GB as f64),
    }
}

/// Whether a path names a share on another machine.
///
/// §7.1.3, with §3.4's attachment discipline behind it: **a network path is not
/// previewed automatically.** The cost of being wrong is not a slow frame, it is
/// a hover that dials a disconnected share and blocks for the operating system's
/// own timeout, and the read is not something the user asked for by name.
///
/// Decided from the path's *prefix* rather than by looking for two backslashes,
/// because `\\?\C:\…` also starts with two and is as local as a path gets.
pub fn is_network_path(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..))
    )
}

/// The four ways a file declines to be previewed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewRefusal {
    /// Nothing in this window reads this kind of file.
    Type,
    /// The head held a NUL, whatever the name claimed.
    Binary,
    /// A share on another machine, which §7.1.3 does not read unasked.
    NetworkPath,
    /// The disk was asked and said no.
    Fault(PreviewFault),
}

impl PreviewRefusal {
    /// The card's own words for itself.
    ///
    /// The mock-up's card says one sentence (4986); a real one has four things
    /// to say and saying the right one is the whole difference between "this
    /// window cannot" and "this file is not there".
    pub fn notice(self) -> &'static str {
        match self {
            Self::Type => "No preview for this file type",
            Self::Binary => "No preview — this looks like a binary file",
            Self::NetworkPath => "No preview — network paths are not read automatically",
            Self::Fault(fault) => fault.notice(),
        }
    }
}

/// The ways a file can decline to be read, mirroring [`crate::files::DirFault`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewFault {
    PermissionDenied,
    NotFound,
    Unreadable,
}

impl PreviewFault {
    fn from_io(error: &std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            std::io::ErrorKind::NotFound => Self::NotFound,
            _ => Self::Unreadable,
        }
    }

    pub fn notice(self) -> &'static str {
        match self {
            Self::PermissionDenied => "No preview — permission denied",
            Self::NotFound => "No preview — file not found",
            Self::Unreadable => "No preview — could not read this file",
        }
    }
}

/// How far along a buffer's body is.
///
/// Three states rather than an `Option<String>`, for the reason
/// [`crate::files::DirNode`] has three: "not read yet" and "will never be read"
/// are different answers and draw differently, and folding them is how a refusal
/// comes to look like a slow disk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewLoad {
    /// Asked, or about to be; the worker has not answered.
    Pending,
    /// Read. `content` holds the head.
    Ready,
    /// No body will ever arrive, and the card says why.
    Refused(PreviewRefusal),
}

/// One file's live buffer.
///
/// Owned by the tab's [`PreviewPool`] and *referred* to by the panes showing it,
/// which is what makes "a file open in two panes is one buffer" true by
/// construction rather than by two panes agreeing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewBuffer {
    /// The identity. Deliberately a `PathBuf` and deliberately the whole path:
    /// two files called `main.rs` are two buffers.
    pub path: PathBuf,
    /// What the header and the switcher call it.
    pub name: String,
    pub ftype: PreviewFtype,
    /// The head of the file, once read.
    pub content: Option<String>,
    /// Whether [`PREVIEW_HEAD_BYTES`] cut the body short. The read-only
    /// degradation §7.1.3 asks for hangs off this; slice 1 carries the fact and
    /// the view that says so is slice 2's.
    pub truncated: bool,
    /// Unsaved edits.
    ///
    /// **Set by the first real change and cleared only by a save.** Editing a
    /// file back to the words it started with does not clean it: the buffer has
    /// been through a state the disk never saw, the undo history a real editor
    /// would compare against does not exist here, and a dot that went out
    /// because you happened to retype what you deleted is a dot that cannot be
    /// trusted the one time it matters.
    pub dirty: bool,
    /// How many times this body has changed.
    ///
    /// **A revision, not a length.** Everything derived from the content — the
    /// parsed document, its measured blocks — is cached against this, and the
    /// length it replaced was a counter that could not see a same-length edit:
    /// swapping one letter for another left the cache convinced it was still
    /// looking at the old text.
    pub revision: u64,
    /// When the file was last written, as of the read this body came from.
    ///
    /// The other half of ruling 8⑨'s minimum concurrent-edit story: a save
    /// compares this against what the disk says *now*, and a disagreement means
    /// somebody else wrote the file while it was open here. `None` for a buffer
    /// that has no body yet — there is nothing to be stale.
    pub disk_mtime: Option<SystemTime>,
    /// Whether a markdown buffer is showing its source rather than its render.
    pub md_source: bool,
    pub load: PreviewLoad,
    /// The widest line of [`Self::content`], in drawn columns.
    ///
    /// Derived once, when the body lands, rather than per frame: it is what the
    /// horizontal scroller's extent is, a scroll happens sixty times a second,
    /// and re-walking sixty-four kilobytes to answer it each time would put the
    /// file's size into the frame budget the head read exists to keep it out of.
    pub max_columns: usize,
}

impl PreviewBuffer {
    /// A buffer for this path, with everything answerable without a disk already
    /// answered.
    pub fn new(path: PathBuf, name: String) -> Self {
        let ftype = preview_ftype(&name);
        let load = if is_network_path(&path) {
            PreviewLoad::Refused(PreviewRefusal::NetworkPath)
        } else {
            match ftype {
                PreviewFtype::Text | PreviewFtype::Markdown | PreviewFtype::Table => {
                    PreviewLoad::Pending
                }
                // A picture's pixels come down the decode lane that already
                // exists, so its buffer is complete the moment it is made.
                PreviewFtype::Image => PreviewLoad::Ready,
                PreviewFtype::Unknown => PreviewLoad::Refused(PreviewRefusal::Type),
            }
        };
        Self {
            path,
            name,
            ftype,
            content: None,
            truncated: false,
            dirty: false,
            revision: 0,
            disk_mtime: None,
            md_source: false,
            load,
            max_columns: 0,
        }
    }

    /// Whether this buffer is still waiting on a head read.
    pub fn wants_head_read(&self) -> bool {
        self.load == PreviewLoad::Pending
            && matches!(
                self.ftype,
                PreviewFtype::Text | PreviewFtype::Markdown | PreviewFtype::Table
            )
    }

    /// Whether this buffer would be shown on a surface that edits.
    ///
    /// The name's judgement ([`is_editable`]) **and two facts only a body
    /// knows**. A buffer with no body has nothing to put a caret in; a
    /// *truncated* one has only the first 64KB of its file, and an edit surface
    /// over the head of a file is a save button wired to `truncate`. §7.1.3's
    /// "超大文件只读降级" is exactly this line — the degradation is read-only,
    /// and read-only has to be enforced where the editing is, not where the
    /// notice is printed.
    pub fn is_editable(&self) -> bool {
        self.load == PreviewLoad::Ready
            && self.content.is_some()
            && !self.truncated
            && is_editable(&self.name, self.ftype, self.md_source)
    }

    /// Hand the body to an edit, and file everything one implies.
    ///
    /// **One door**, for the reason [`PreviewPool::open`] is one door: an edit
    /// owes three things — the dirty bit, the revision every cache is keyed on,
    /// and the widest line the horizontal scroller is derived from — and three
    /// call sites each remembering all three is three chances to forget one.
    /// The closure reports whether anything actually changed, so an insert of
    /// nothing does not dirty a file.
    pub fn edit_content(&mut self, edit: impl FnOnce(&mut String) -> bool) -> bool {
        let Some(content) = self.content.as_mut() else {
            return false;
        };
        if !edit(content) {
            return false;
        }
        self.max_columns = widest_line_columns(self.content.as_deref().unwrap_or_default());
        self.revision += 1;
        self.dirty = true;
        true
    }

    /// Write the body back to its file.
    ///
    /// Three refusals and one write, in this order and for this reason:
    ///
    /// * A buffer with no body has nothing to write, and writing "nothing"
    ///   would empty the file a failed read was about.
    /// * A body the disk has moved on from is [`SaveOutcome::Conflict`]
    ///   (ruling 8⑨). **Not a prompt and not a blind write** — this slice's
    ///   minimum is that the window says so and keeps the edits, because the
    ///   one unrecoverable outcome is overwriting a change nobody has seen.
    /// * The write itself is atomic ([`save_atomically`]).
    ///
    /// The mtime is re-read from the file that was just written rather than
    /// remembered from the write, so the next save compares against what the
    /// filesystem actually recorded.
    pub fn save(&mut self) -> SaveOutcome {
        let Some(content) = self.content.as_deref() else {
            return SaveOutcome::Failed("there is nothing to save".to_owned());
        };
        if file_mtime(&self.path) != self.disk_mtime {
            return SaveOutcome::Conflict;
        }
        if let Err(error) = save_atomically(&self.path, content) {
            return SaveOutcome::Failed(error.to_string());
        }
        self.disk_mtime = file_mtime(&self.path);
        self.dirty = false;
        SaveOutcome::Saved
    }

    /// The sentence a body that was cut short owes its reader, if it was.
    ///
    /// §7.1.3's "超大文件只读降级": the degradation is not that the file failed,
    /// it is that what is on screen is the beginning of it — and a preview that
    /// showed the first 64KB without saying so would be a preview quietly
    /// claiming the file ends there.
    pub fn truncation_notice(&self) -> Option<&'static str> {
        self.truncated.then_some(PREVIEW_TRUNCATED_NOTICE)
    }

    /// Which body this buffer is drawn as.
    pub fn view(&self) -> PreviewView {
        preview_view(&self.name, self.ftype, self.md_source)
    }

    /// Why there is no body to show, when there is none.
    pub fn refusal(&self) -> Option<PreviewRefusal> {
        match self.load {
            PreviewLoad::Refused(refusal) => Some(refusal),
            PreviewLoad::Pending | PreviewLoad::Ready => None,
        }
    }

    /// File the worker's answer.
    pub fn accept(&mut self, outcome: HeadOutcome) {
        self.revision += 1;
        match outcome {
            HeadOutcome::Read {
                text,
                truncated,
                mtime,
            } => {
                self.max_columns = widest_line_columns(&text);
                self.content = Some(text);
                self.truncated = truncated;
                self.disk_mtime = mtime;
                self.load = PreviewLoad::Ready;
            }
            HeadOutcome::Refused(refusal) => {
                self.content = None;
                self.truncated = false;
                self.max_columns = 0;
                self.disk_mtime = None;
                self.load = PreviewLoad::Refused(refusal);
            }
        }
    }
}

/// One tab's shared pool of live buffers.
///
/// A `Vec` and not a map, because the order *is* the history the filename
/// switcher lists and the order the eviction law is written in terms of
/// ("the earliest clean one"). One entry per path is an invariant of
/// [`Self::open`], which is the only door in.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PreviewPool {
    buffers: Vec<PreviewBuffer>,
}

impl PreviewPool {
    /// How many buffers are alive. The header's count badge is slice 4's; this
    /// is what the cap is asserted against.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    pub fn get(&self, path: &Path) -> Option<&PreviewBuffer> {
        self.buffers.iter().find(|buffer| buffer.path == path)
    }

    pub fn get_mut(&mut self, path: &Path) -> Option<&mut PreviewBuffer> {
        self.buffers.iter_mut().find(|buffer| buffer.path == path)
    }

    /// The buffer for this file, found or made — the one door into the pool.
    ///
    /// **Finding beats making**, which is the whole of "already-open file means
    /// the same buffer, edits intact, whichever pane showed it" (mock-up
    /// 5023-5026).
    ///
    /// Making one may push the pool over [`PV_BUFFER_CAP`], and then the law is
    /// the mock-up's word for word (3152-3156): evict the *earliest* buffer that
    /// is not dirty, is not the one just opened, and is not on any pane's screen
    /// — and if there is no such buffer, **evict nothing**. A pool over its cap
    /// is a pool holding nothing but state a user would miss, and a ceiling is
    /// not worth losing an unsaved edit for.
    pub fn open(
        &mut self,
        path: PathBuf,
        name: String,
        displayed: &[PathBuf],
    ) -> &mut PreviewBuffer {
        if let Some(index) = self.index_of(&path) {
            return &mut self.buffers[index];
        }
        self.buffers.push(PreviewBuffer::new(path.clone(), name));
        while self.buffers.len() > PV_BUFFER_CAP {
            let Some(index) = self.buffers.iter().position(|buffer| {
                !buffer.dirty && buffer.path != path && !displayed.contains(&buffer.path)
            }) else {
                break;
            };
            self.buffers.remove(index);
        }
        let index = self
            .index_of(&path)
            .expect("the buffer just opened is never the one evicted");
        &mut self.buffers[index]
    }

    fn index_of(&self, path: &Path) -> Option<usize> {
        self.buffers.iter().position(|buffer| buffer.path == path)
    }
}

/// What is being asked about a file.
///
/// Two questions on one lane rather than two lanes, because they are the same
/// kind of question about the same file and neither is worth a second thread:
/// what separates them is only that one is answered by bytes and the other by a
/// directory entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewWant {
    /// At most [`PREVIEW_HEAD_BYTES`] of the body.
    Head,
    /// How large the whole file is — the third field of a picture's meta line
    /// (mock-up 4955), which is the only thing on that line the decoder cannot
    /// answer for itself.
    Size,
}

/// "Answer this about this file for this tab."
///
/// Addressed by [`TabId`] rather than by a seat, because the pool is the tab's:
/// the answer belongs to the buffer, and which pane happens to be showing it
/// when the disk answers is none of the worker's business.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewRequest {
    pub tab: TabId,
    pub path: PathBuf,
    pub want: PreviewWant,
}

impl PreviewRequest {
    /// Two requests are the same question when they ask the same thing about
    /// the same file of the same tab.
    ///
    /// **`want` is part of the target.** Without it a size question would
    /// supersede the head read of the same picture and the body would never
    /// arrive, which is coalescing turned into cancellation.
    fn same_target(&self, other: &Self) -> bool {
        self.tab == other.tab && self.path == other.path && self.want == other.want
    }
}

/// What the worker found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewResponse {
    pub tab: TabId,
    pub path: PathBuf,
    pub answer: PreviewAnswer,
}

/// One answer to one [`PreviewWant`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreviewAnswer {
    Head(HeadOutcome),
    /// `None` when the file could not be stat'ed, which the meta line simply
    /// leaves out rather than turning into an error of its own.
    Size(Option<u64>),
}

/// How large a file is, without reading it.
pub fn read_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|meta| meta.len())
}

/// A head either reads or it does not, and both are answers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HeadOutcome {
    Read {
        text: String,
        /// Whether [`PREVIEW_HEAD_BYTES`] cut it short.
        truncated: bool,
        /// When the file said it was last written, **asked of the handle these
        /// bytes came out of**. A second `metadata` call by path could answer
        /// about a file that had already been replaced between the two, which
        /// is precisely the race the answer exists to detect.
        mtime: Option<SystemTime>,
    },
    Refused(PreviewRefusal),
}

/// How a save turned out.
///
/// Three outcomes rather than a `Result`, because the middle one is not a
/// failure: the disk moved and the window is declining to guess, which is a
/// sentence the user is owed and a state the buffer survives intact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SaveOutcome {
    Saved,
    /// The file on disk is not the file that was read (ruling 8⑨).
    Conflict,
    Failed(String),
}

/// The acknowledgement a save gets, and how long it stands.
///
/// Ruling 6 (2026-08-12): the mock-up's four feedback durations collapse to the
/// one the foot's "Revealed" already used. The word belongs to the pane foot,
/// which is slice 4's; until there is one it is printed on the same strip the
/// truncation notice stands on, which is the body's existing channel for a
/// sentence about the file rather than of it.
pub const PREVIEW_SAVED_NOTICE: &str = "Saved";

/// What the window says instead of overwriting somebody else's write.
///
/// It says what happened, what was *not* done, and what is still true — the
/// edits are still here — because a conflict notice that only announces failure
/// leaves the user believing their work is gone.
pub const PREVIEW_CONFLICT_NOTICE: &str =
    "Not saved — this file changed on disk since it was opened; your edits are still here";

/// When a file was last written, or `None` if it will not say.
pub fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
}

/// Where a save stages its bytes before they become the file.
///
/// A sibling of the target and not a `%TEMP%` entry, for the one reason that
/// decides it: [`save_atomically`]'s last step is a rename, and a rename is only
/// atomic *within a volume*. A staging file on another drive would turn the
/// whole guarantee into a copy — which is the non-atomic write this exists to
/// avoid, wearing a temporary name.
///
/// Named from the process as well as the file so two windows saving the same
/// path cannot stage over each other, and dot-prefixed so it is hidden by the
/// same convention every tool on this platform already honours.
pub fn preview_temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.bt-save-{}", std::process::id()))
}

/// Write a file so that it is either the old one or the new one, never half of
/// either.
///
/// **Staged and renamed.** Opening the target and writing into it is the way
/// every editor loses a file to a full disk: the truncate has already happened
/// when the write fails, and what is left is neither version. Here the target is
/// not touched at all until the bytes are on the disk and flushed, and the last
/// step is a single rename the filesystem either performs or does not.
pub fn save_atomically(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    let temp = preview_temp_path(path);
    let staged = (|| {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(contents.as_bytes())?;
        // Flushed before the rename, so a crash between the two cannot leave the
        // *name* switched over to a body that never reached the platter.
        file.sync_all()
    })();
    if let Err(error) = staged {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

/// Read at most [`PREVIEW_HEAD_BYTES`] of a file, and decide what it is.
///
/// **The size question and the binary question are the same read.** Both are
/// facts about the first 64KB, so asking the disk twice would buy nothing but
/// two chances to disagree — the head is taken once, its length answers
/// truncation, and its bytes answer whether this is text at all.
pub fn read_head(path: &Path) -> HeadOutcome {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            return HeadOutcome::Refused(PreviewRefusal::Fault(PreviewFault::from_io(&error)));
        }
    };
    // One byte past the limit, which is the cheapest honest way to learn that
    // there *is* more: a length is a second question and a metadata read can
    // disagree with the bytes on a file being written to right now.
    let mut head = Vec::new();
    if let Err(error) = file
        .by_ref()
        .take(PREVIEW_HEAD_BYTES as u64 + 1)
        .read_to_end(&mut head)
    {
        return HeadOutcome::Refused(PreviewRefusal::Fault(PreviewFault::from_io(&error)));
    }
    let truncated = head.len() > PREVIEW_HEAD_BYTES;
    head.truncate(PREVIEW_HEAD_BYTES);
    // Asked of the handle the bytes came out of, not of the path: between two
    // calls by name a file can be replaced entirely, and a stamp belonging to a
    // file other than the one that was read is worse than no stamp at all.
    let mtime = file.metadata().ok().and_then(|meta| meta.modified().ok());
    // The one sniff §7.1.3 asks for, and the only one that is nearly free and
    // nearly never wrong: text does not hold a NUL, and every binary format
    // worth refusing holds one in its first few bytes.
    if head.contains(&0) {
        return HeadOutcome::Refused(PreviewRefusal::Binary);
    }
    HeadOutcome::Read {
        text: decode_head(&head, truncated),
        truncated,
        mtime,
    }
}

/// Turn a head of bytes into text.
///
/// Lossy, because a preview that refuses a file over one bad byte is a preview
/// that refuses log files. The one thing done first is dropping a multi-byte
/// character the *cut* broke in half: that replacement character would be an
/// artefact of the limit rather than of the file, and it would sit at the end of
/// every truncated CJK document.
fn decode_head(head: &[u8], truncated: bool) -> String {
    let head = if truncated {
        trim_partial_utf8(head)
    } else {
        head
    };
    String::from_utf8_lossy(head).into_owned()
}

/// Drop a trailing UTF-8 sequence the caller's cut left incomplete.
fn trim_partial_utf8(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    // A UTF-8 sequence is at most four bytes, so at most three continuations can
    // precede the lead byte being looked for.
    let mut continuations = 0usize;
    while end > 0 && continuations < 4 {
        let byte = bytes[end - 1];
        if byte & 0b1100_0000 == 0b1000_0000 {
            end -= 1;
            continuations += 1;
            continue;
        }
        let needed = if byte < 0x80 {
            1
        } else if byte >> 5 == 0b110 {
            2
        } else if byte >> 4 == 0b1110 {
            3
        } else if byte >> 3 == 0b1_1110 {
            4
        } else {
            // A stray continuation or an illegal lead: not a sequence this can
            // reason about, so leave it to the lossy decode.
            1
        };
        return if needed <= continuations + 1 {
            bytes
        } else {
            &bytes[..end - 1]
        };
    }
    bytes
}

/// The queue in front of the disk, newest question per target.
#[derive(Default)]
struct PendingPreviewRequests {
    requests: std::collections::VecDeque<PreviewRequest>,
}

impl PendingPreviewRequests {
    fn push_latest(&mut self, request: PreviewRequest) {
        if let Some(index) = self
            .requests
            .iter()
            .position(|queued| queued.same_target(&request))
        {
            self.requests.remove(index);
        }
        self.requests.push_back(request);
    }

    fn pop_front(&mut self) -> Option<PreviewRequest> {
        self.requests.pop_front()
    }

    fn contains_target(&self, request: &PreviewRequest) -> bool {
        self.requests
            .iter()
            .any(|queued| queued.same_target(request))
    }

    fn drain_channel(&mut self, receiver: &mpsc::Receiver<PreviewRequest>) {
        while let Ok(request) = receiver.try_recv() {
            self.push_latest(request);
        }
    }
}

/// Serve file questions, newest per target first.
///
/// Split from [`PreviewWorker::spawn`] so the coalescing can be tested without a
/// filesystem or an event loop, exactly as `run_dir_worker` is.
fn run_preview_worker(
    receiver: mpsc::Receiver<PreviewRequest>,
    mut execute: impl FnMut(PreviewRequest),
) {
    let mut pending = PendingPreviewRequests::default();
    while let Ok(request) = receiver.recv() {
        pending.push_latest(request);
        pending.drain_channel(&receiver);
        while let Some(request) = pending.pop_front() {
            pending.drain_channel(&receiver);
            if pending.contains_target(&request) {
                continue;
            }
            execute(request);
        }
    }
}

/// The thread, and the two ends of the conversation with it.
pub struct PreviewWorker {
    requests: mpsc::Sender<PreviewRequest>,
    pub responses: mpsc::Receiver<PreviewResponse>,
}

impl PreviewWorker {
    pub fn spawn(proxy: EventLoopProxy<AppEvent>) -> Result<Self> {
        let (request_tx, request_rx) = mpsc::channel::<PreviewRequest>();
        let (response_tx, response_rx) = mpsc::channel::<PreviewResponse>();
        thread::Builder::new()
            .name("bt-preview-worker".to_owned())
            .spawn(move || {
                run_preview_worker(request_rx, |request| {
                    let answer = match request.want {
                        PreviewWant::Head => PreviewAnswer::Head(read_head(&request.path)),
                        PreviewWant::Size => PreviewAnswer::Size(read_size(&request.path)),
                    };
                    if response_tx
                        .send(PreviewResponse {
                            tab: request.tab,
                            path: request.path,
                            answer,
                        })
                        .is_ok()
                    {
                        let _ = proxy.send_event(AppEvent::PreviewReady);
                    }
                });
            })
            .context("spawn file preview reading worker")?;
        Ok(Self {
            requests: request_tx,
            responses: response_rx,
        })
    }

    /// Ask, reporting whether the worker was still there to be asked.
    #[must_use]
    pub fn request(&self, request: PreviewRequest) -> bool {
        self.requests.send(request).is_ok()
    }
}

/// Turn preview reading off for the rest of the run, once.
pub fn disable_preview_worker_state(running: &mut bool, notice_pending: &mut bool) -> bool {
    if !*running {
        return false;
    }
    *running = false;
    *notice_pending = true;
    eprintln!("file preview reading worker stopped; terminal input and output remain available");
    true
}

pub fn take_preview_worker_notice(notice_pending: &mut bool) -> Option<&'static str> {
    if std::mem::take(notice_pending) {
        Some(PREVIEW_WORKER_STOPPED_NOTICE)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bt-preview-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn buffer<'pool>(pool: &'pool PreviewPool, path: &str) -> &'pool PreviewBuffer {
        pool.get(Path::new(path)).expect("the pool holds this path")
    }

    /// A worker answer for a body that never came off a disk.
    fn read(text: &str, truncated: bool) -> HeadOutcome {
        HeadOutcome::Read {
            text: text.to_owned(),
            truncated,
            mtime: None,
        }
    }

    /// A file on disk, with a buffer already reading from it.
    fn opened(dir: &Path, name: &str, body: &str) -> PreviewBuffer {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        let mut buffer = PreviewBuffer::new(path.clone(), name.to_owned());
        buffer.accept(read_head(&path));
        buffer
    }

    /// ① One file, one buffer — a second open of the same path is the same
    /// buffer, edits and all.
    ///
    /// Mutation: make [`PreviewPool::open`] push unconditionally instead of
    /// looking for the path first.
    #[test]
    fn a_second_open_of_the_same_path_is_the_same_buffer() {
        let mut pool = PreviewPool::default();
        pool.open(PathBuf::from(r"C:\w\a.rs"), "a.rs".to_owned(), &[])
            .dirty = true;
        pool.open(PathBuf::from(r"C:\w\a.rs"), "a.rs".to_owned(), &[]);
        assert_eq!(pool.len(), 1);
        assert!(buffer(&pool, r"C:\w\a.rs").dirty);
    }

    /// ② The cap evicts the oldest clean buffer nobody is showing, and nothing
    /// else.
    ///
    /// Mutation: drop the `!buffer.dirty` clause, or the `displayed` clause,
    /// from the eviction predicate.
    #[test]
    fn the_cap_evicts_the_oldest_clean_unshown_buffer() {
        let mut pool = PreviewPool::default();
        for index in 0..PV_BUFFER_CAP {
            let path = PathBuf::from(format!(r"C:\w\f{index}.rs"));
            pool.open(path, format!("f{index}.rs"), &[]);
        }
        // The oldest is dirty and the second oldest is on screen, so the third
        // is the first evictable one.
        pool.get_mut(Path::new(r"C:\w\f0.rs")).unwrap().dirty = true;
        let shown = vec![PathBuf::from(r"C:\w\f1.rs")];
        pool.open(PathBuf::from(r"C:\w\new.rs"), "new.rs".to_owned(), &shown);
        assert_eq!(pool.len(), PV_BUFFER_CAP);
        assert!(pool.get(Path::new(r"C:\w\f0.rs")).is_some());
        assert!(pool.get(Path::new(r"C:\w\f1.rs")).is_some());
        assert!(pool.get(Path::new(r"C:\w\f2.rs")).is_none());
        assert!(pool.get(Path::new(r"C:\w\new.rs")).is_some());
    }

    /// ② (b) When everything left is dirty or on screen, nothing is evicted.
    ///
    /// Mutation: replace the `else { break }` in [`PreviewPool::open`] with a
    /// `remove(0)`.
    #[test]
    fn a_pool_of_dirty_buffers_grows_past_the_cap_rather_than_lose_one() {
        let mut pool = PreviewPool::default();
        for index in 0..=PV_BUFFER_CAP {
            let path = PathBuf::from(format!(r"C:\w\f{index}.rs"));
            pool.open(path.clone(), format!("f{index}.rs"), &[]);
            pool.get_mut(&path).unwrap().dirty = true;
        }
        assert_eq!(pool.len(), PV_BUFFER_CAP + 1);
    }

    /// ③ The extension table, class by class.
    ///
    /// Mutation: move `"svg"` out of [`IMAGE_EXTENSIONS`], or drop the
    /// `name.starts_with('.')` clause.
    #[test]
    fn the_extension_table_answers_each_class() {
        for name in ["a.png", "a.jpg", "a.jpeg", "a.svg", "a.gif", "a.webp"] {
            assert_eq!(preview_ftype(name), PreviewFtype::Image, "{name}");
        }
        assert_eq!(preview_ftype("README.md"), PreviewFtype::Markdown);
        assert_eq!(preview_ftype("cases.csv"), PreviewFtype::Table);
        for name in [
            "a.rs",
            "a.py",
            "a.js",
            "a.ts",
            "a.json",
            "a.toml",
            "a.html",
            "a.txt",
            "a.gitignore",
            "Cargo.lock",
            "a.yml",
            "a.yaml",
            "a.diff",
            "a.patch",
        ] {
            assert_eq!(preview_ftype(name), PreviewFtype::Text, "{name}");
        }
        // A name that is nothing but an extension is a dotfile, and dotfiles are
        // text.
        assert_eq!(preview_ftype(".gitignore"), PreviewFtype::Text);
        assert_eq!(preview_ftype(".env"), PreviewFtype::Text);
        for name in ["a.exe", "a.dll", "a", "a.zip", "a.PNG.zip"] {
            assert_eq!(preview_ftype(name), PreviewFtype::Unknown, "{name}");
        }
        // The table is case-insensitive on the extension.
        assert_eq!(preview_ftype("A.PNG"), PreviewFtype::Image);
        assert_eq!(preview_ftype("A.RS"), PreviewFtype::Text);
    }

    /// ④ `editable` names the surface that actually edits.
    ///
    /// Mutation: return `true` for [`PreviewFtype::Table`], or delete the
    /// [`is_diff_name`] guard.
    #[test]
    fn editable_is_the_surface_that_actually_edits() {
        assert!(is_editable("a.rs", PreviewFtype::Text, false));
        assert!(!is_editable("cases.csv", PreviewFtype::Table, false));
        assert!(!is_editable("a.diff", PreviewFtype::Text, false));
        assert!(!is_editable("a.patch", PreviewFtype::Text, false));
        assert!(!is_editable("a.PATCH", PreviewFtype::Text, false));
        assert!(!is_editable("README.md", PreviewFtype::Markdown, false));
        assert!(is_editable("README.md", PreviewFtype::Markdown, true));
        assert!(!is_editable("a.png", PreviewFtype::Image, true));
        assert!(!is_editable("a.exe", PreviewFtype::Unknown, true));
    }

    /// ⑤ A NUL in the head is a binary file, whatever its name claims.
    ///
    /// Mutation: delete the `head.contains(&0)` guard in [`read_head`].
    #[test]
    fn a_nul_in_the_head_is_a_binary_file() {
        let dir = scratch("binary");
        let path = dir.join("looks-like-text.txt");
        std::fs::write(&path, b"MZ\x90\x00\x03text after the nul").unwrap();
        assert_eq!(
            read_head(&path),
            HeadOutcome::Refused(PreviewRefusal::Binary)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ⑥ A file past the head limit is read short and says so.
    ///
    /// Mutation: `take(PREVIEW_HEAD_BYTES as u64)` instead of `+ 1`, which makes
    /// every file look complete.
    #[test]
    fn a_file_past_the_head_limit_is_read_short_and_says_so() {
        let dir = scratch("big");
        let big = dir.join("big.txt");
        std::fs::write(&big, "x".repeat(PREVIEW_HEAD_BYTES + 4096)).unwrap();
        match read_head(&big) {
            HeadOutcome::Read {
                text, truncated, ..
            } => {
                assert!(truncated);
                assert_eq!(text.len(), PREVIEW_HEAD_BYTES);
            }
            other => panic!("expected a truncated read, got {other:?}"),
        }
        let small = dir.join("small.txt");
        std::fs::write(&small, "one line\n").unwrap();
        assert_eq!(
            read_head(&small),
            HeadOutcome::Read {
                text: "one line\n".to_owned(),
                truncated: false,
                mtime: file_mtime(&small),
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A cut that lands inside a character drops the half rather than showing a
    /// replacement the file does not contain.
    #[test]
    fn a_cut_inside_a_character_drops_the_half_it_made() {
        // "你" is three bytes; keep two of them.
        let broken = [0xE4, 0xBD];
        assert_eq!(decode_head(&broken, true), "");
        // The same bytes from a file that simply ends there are the file's own
        // problem, not the limit's, and are shown lossily.
        assert_eq!(decode_head(&broken, false), "\u{fffd}");
        // A whole character at the cut survives.
        let whole = [0xE4, 0xBD, 0xA0];
        assert_eq!(decode_head(&whole, true), "\u{4f60}");
    }

    /// ⑦ A network path is refused without a read.
    ///
    /// Mutation: make [`is_network_path`] test `starts_with(r"\\")` on the
    /// string, which drags `\\?\C:\…` in with it.
    #[test]
    fn a_network_path_is_refused_without_a_read() {
        assert!(is_network_path(Path::new(r"\\server\share\notes.txt")));
        assert!(is_network_path(Path::new(
            r"\\?\UNC\server\share\notes.txt"
        )));
        assert!(!is_network_path(Path::new(r"C:\w\notes.txt")));
        assert!(!is_network_path(Path::new(r"\\?\C:\w\notes.txt")));

        let buffer = PreviewBuffer::new(
            PathBuf::from(r"\\server\share\notes.txt"),
            "notes.txt".to_owned(),
        );
        assert_eq!(
            buffer.load,
            PreviewLoad::Refused(PreviewRefusal::NetworkPath)
        );
        assert!(!buffer.wants_head_read());
    }

    /// A type with no reader never asks the disk either.
    ///
    /// Mutation: give [`PreviewFtype::Unknown`] `PreviewLoad::Pending`.
    #[test]
    fn an_unreadable_type_is_refused_before_the_disk_is_asked() {
        let buffer = PreviewBuffer::new(PathBuf::from(r"C:\w\a.exe"), "a.exe".to_owned());
        assert_eq!(buffer.load, PreviewLoad::Refused(PreviewRefusal::Type));
        assert!(!buffer.wants_head_read());
        let text = PreviewBuffer::new(PathBuf::from(r"C:\w\a.rs"), "a.rs".to_owned());
        assert_eq!(text.load, PreviewLoad::Pending);
        assert!(text.wants_head_read());
    }

    // ── slice 2: the read-only view family ──────────────────────────────────

    /// PIN — the dispatch asks its questions in the mock-up's order, and **a
    /// name beats a type**.
    ///
    /// `.diff` and `.patch` sit inside the text extension list, so by type they
    /// are text and by name they are a reading. The mock-up settles it by asking
    /// the name *before* the textarea (4970 before 4980), and that order is the
    /// whole of ruling 3: a diff never gets an edit surface and therefore never
    /// gets a save button.
    ///
    /// Mutation: move the diff arm below the text arm in [`preview_view`].
    #[test]
    fn a_diff_is_a_diff_by_name_even_though_it_is_text_by_type() {
        let view = |name: &str, md_source: bool| preview_view(name, preview_ftype(name), md_source);
        assert_eq!(preview_ftype("a.diff"), PreviewFtype::Text);
        assert_eq!(view("a.diff", false), PreviewView::Diff);
        assert_eq!(view("a.patch", false), PreviewView::Diff);
        assert_eq!(view("A.PATCH", false), PreviewView::Diff);
        assert_eq!(view("a.rs", false), PreviewView::Text);
        assert_eq!(view("cases.csv", false), PreviewView::Table);
        assert_eq!(view("README.md", false), PreviewView::Markdown);
        // Flipped to source, a markdown buffer is the text surface — slice 4
        // owns the control, the rule lives here.
        assert_eq!(view("README.md", true), PreviewView::Text);
        assert_eq!(view("a.png", false), PreviewView::Image);
        assert_eq!(view("a.exe", false), PreviewView::None);
    }

    /// PIN — the diff's five line classes, in the order the prefixes overlap.
    ///
    /// `---` is a deletion's prefix *and* a meta line's; `+++` likewise. The
    /// mock-up asks the three-character forms first (4973-4976) and that is the
    /// only thing keeping a diff's own header out of the red band.
    ///
    /// Mutation: test `starts_with("-")` before `starts_with("---")`.
    #[test]
    fn a_diffs_own_header_is_not_a_deletion() {
        use DiffLineKind::*;
        assert_eq!(diff_line_kind("--- a/src/main.rs"), Meta);
        assert_eq!(diff_line_kind("+++ b/src/main.rs"), Meta);
        assert_eq!(diff_line_kind("diff --git a/x b/x"), Meta);
        assert_eq!(diff_line_kind("@@ -1,7 +1,9 @@ fn main()"), Hunk);
        assert_eq!(diff_line_kind("+    let x = 1;"), Add);
        assert_eq!(diff_line_kind("-    let x = 0;"), Del);
        assert_eq!(diff_line_kind("     unchanged"), Context);
        assert_eq!(diff_line_kind(""), Context);
        assert!(diff_line_kind("+    let x = 1;").tints());
        assert!(diff_line_kind("-    let x = 0;").tints());
        assert!(!diff_line_kind("@@ -1 +1 @@").tints());
        assert!(!diff_line_kind("--- a/x").tints());
    }

    /// PIN — the small renderer's whole support surface (mock-up 4914-4941).
    ///
    /// Mutation: accept `####` as a heading (the mock-up's regex is `#{1,3}`),
    /// or stop flushing the open list before a heading.
    #[test]
    fn the_markdown_renderer_supports_exactly_what_the_mock_up_draws() {
        let doc = parse_markdown(
            "# Title\n\
             \n\
             A paragraph with `code` and **bold**.\n\
             - first\n\
             * second\n\
             ## Sub\n\
             ```rust\n\
             let x = 1;\n\
             ```\n\
             #### not a heading\n",
        );
        assert_eq!(
            doc,
            vec![
                MarkdownBlock::Heading {
                    level: 1,
                    spans: vec![Span::plain("Title")],
                },
                MarkdownBlock::Paragraph(vec![
                    Span::plain("A paragraph with "),
                    Span::code("code"),
                    Span::plain(" and "),
                    Span::bold("bold"),
                    Span::plain("."),
                ]),
                // Both bullet characters, one list — and the list is closed by
                // the heading rather than swallowing it.
                MarkdownBlock::List(vec![
                    vec![Span::plain("first")],
                    vec![Span::plain("second")],
                ]),
                MarkdownBlock::Heading {
                    level: 2,
                    spans: vec![Span::plain("Sub")],
                },
                MarkdownBlock::Code {
                    lang: Some("rust".to_owned()),
                    text: "let x = 1;".to_owned(),
                },
                MarkdownBlock::Paragraph(vec![Span::plain("#### not a heading")]),
            ]
        );
        // A fence never closed still renders, rather than eating the rest of the
        // document in silence (mock-up 4939).
        assert_eq!(
            parse_markdown("```\nunfinished\n"),
            vec![MarkdownBlock::Code {
                lang: None,
                text: "unfinished".to_owned(),
            }]
        );
        // A blank line is a separator, not a paragraph.
        assert_eq!(parse_markdown("\n\n"), vec![]);
    }

    /// PIN — the rendered view renders **the content**.
    ///
    /// P103's named lie: the prototype's first rendered view was a static mock
    /// that showed the same document whatever the buffer held. Asserted as the
    /// property rather than as a string — whatever the buffer says has to come
    /// out the other end.
    ///
    /// Mutation: make `parse_markdown` ignore its argument and return a fixed
    /// document.
    #[test]
    fn the_rendered_view_renders_the_buffers_own_text() {
        for body in ["# One", "# Two", "# Three"] {
            assert_eq!(
                parse_markdown(body),
                vec![MarkdownBlock::Heading {
                    level: 1,
                    spans: vec![Span::plain(&body[2..])],
                }]
            );
        }
    }

    /// PIN — the first row is headings and the rest are cells.
    ///
    /// Mutation: drop the `skip(1)`/`first` split so every row is a data row.
    #[test]
    fn a_tables_first_row_is_its_heading_row() {
        let rows = csv_rows("case,cols,expect\ncjk-width,2,PASS\nemoji-vs16,2,FAIL\n");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], vec!["case", "cols", "expect"]);
        assert_eq!(rows[2], vec!["emoji-vs16", "2", "FAIL"]);
        // A quoted field keeps the comma inside it. The mock-up splits naively
        // because its own fixture has no quotes; a real file does, and a grid
        // that shifts a column at the first quoted comma is not a table.
        assert_eq!(
            csv_rows("a,\"b,c\",d"),
            vec![vec!["a".to_owned(), "b,c".to_owned(), "d".to_owned()]]
        );
        assert_eq!(
            csv_rows("\"say \"\"hi\"\"\",2"),
            vec![vec!["say \"hi\"".to_owned(), "2".to_owned()]]
        );
        assert!(csv_rows("   \n").is_empty());
    }

    /// PIN — a truncated buffer says so, and a whole one says nothing.
    ///
    /// The read-only degradation §7.1.3 asks for. Slice 1 carried the fact;
    /// this is the sentence it earns.
    ///
    /// Mutation: return the notice unconditionally.
    #[test]
    fn only_a_truncated_buffer_carries_the_read_only_notice() {
        let mut buffer = PreviewBuffer::new(PathBuf::from(r"C:\w\a.rs"), "a.rs".to_owned());
        buffer.accept(read("fn main() {}\n", false));
        assert_eq!(buffer.truncation_notice(), None);
        buffer.accept(read("fn main() {}\n", true));
        assert_eq!(buffer.truncation_notice(), Some(PREVIEW_TRUNCATED_NOTICE));
    }

    /// PIN — the widest line is measured in **columns**, not bytes or chars,
    /// and after tabs have become the spaces they draw as.
    ///
    /// It is what the horizontal scroller's extent is derived from, so a wide
    /// character that measured one column would leave the end of its own line
    /// unreachable.
    ///
    /// Mutation: `line.chars().count()` instead of the display width.
    #[test]
    fn the_widest_line_is_measured_in_the_columns_it_will_draw_as() {
        let mut buffer = PreviewBuffer::new(PathBuf::from(r"C:\w\a.rs"), "a.rs".to_owned());
        buffer.accept(read("ab\n\t\tx\n\u{4f60}\u{597d}\n", false));
        // Two tabs are eight columns, plus the `x`.
        assert_eq!(buffer.max_columns, 9);
        buffer.accept(read("\u{4f60}\u{597d}\u{4e16}\u{754c}\u{ff01}", false));
        assert_eq!(
            buffer.max_columns, 10,
            "five wide characters are ten columns"
        );
    }

    /// PIN — a tab advances to the next stop rather than to four more spaces.
    ///
    /// `tab-size: 4` (mock-up 603) names a column grid, and replacing each tab
    /// with four spaces is what misaligns every continuation line of an indented
    /// file — which is precisely what a preview of source code is for.
    ///
    /// Mutation: `repeat_n(' ', PREVIEW_TEXT_TAB_WIDTH)` instead of the computed
    /// advance.
    #[test]
    fn a_tab_advances_to_the_next_stop_rather_than_to_four_more_spaces() {
        assert_eq!(expand_tabs("\tfn main() {"), "    fn main() {");
        assert_eq!(expand_tabs("ab\tc"), "ab  c", "two columns in, two to go");
        assert_eq!(
            expand_tabs("abcd\te"),
            "abcd    e",
            "a full stop is skipped"
        );
        assert_eq!(expand_tabs("a\t\tb"), "a       b", "stops compose");
        assert_eq!(expand_tabs("no tabs here"), "no tabs here");
        // A wide character is two columns, so the stop after it is two away.
        assert_eq!(expand_tabs("\u{4f60}\tx"), "\u{4f60}  x");
    }

    /// PIN — the meta line's size is a real one.
    ///
    /// Mutation: divide by 1000 instead of 1024.
    #[test]
    fn a_files_size_reads_the_way_a_file_manager_says_it() {
        assert_eq!(format_byte_size(0), "0 B");
        assert_eq!(format_byte_size(945), "945 B");
        assert_eq!(format_byte_size(219_136), "214 KB");
        assert_eq!(format_byte_size(1024), "1 KB");
        assert_eq!(format_byte_size(5 * 1024 * 1024 + 512 * 1024), "5.5 MB");
        assert_eq!(format_byte_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    /// A size question is its own question on the same lane, and is coalesced
    /// separately from the head read of the same file.
    ///
    /// Mutation: leave `want` out of `same_target`.
    #[test]
    fn a_size_and_a_head_of_one_file_are_two_questions() {
        let dir = scratch("size");
        let path = dir.join("a.png");
        std::fs::write(&path, vec![7u8; 4096]).unwrap();
        assert_eq!(read_size(&path), Some(4096));
        assert_eq!(read_size(&dir.join("nope.png")), None);

        let (sender, receiver) = std::sync::mpsc::channel();
        let ask = |want| PreviewRequest {
            tab: crate::TabId(1),
            path: PathBuf::from("a.png"),
            want,
        };
        sender.send(ask(PreviewWant::Head)).unwrap();
        sender.send(ask(PreviewWant::Size)).unwrap();
        drop(sender);
        let mut asked = Vec::new();
        run_preview_worker(receiver, |request| asked.push(request.want));
        assert_eq!(
            asked,
            vec![PreviewWant::Head, PreviewWant::Size],
            "neither supersedes the other"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── slice 3: quick edit ─────────────────────────────────────────────────

    /// ① The first real change dirties the buffer, and editing back to the
    /// original does not clean it.
    ///
    /// The second half is the ruling and the reason the bit is monotonic: the
    /// buffer has been through a state the disk never saw, and a dot that goes
    /// out because you happened to retype what you deleted is a dot nobody can
    /// trust the one time it matters. Only a save cleans it.
    ///
    /// Mutation: set `dirty` from `content != original` rather than from the
    /// fact of an edit; or drop the `if !edit(content)` guard, which dirties a
    /// file for a keystroke that changed nothing.
    #[test]
    fn the_first_real_change_dirties_the_buffer_and_nothing_cleans_it_but_a_save() {
        let mut buffer = PreviewBuffer::new(PathBuf::from(r"C:\w\a.rs"), "a.rs".to_owned());
        buffer.accept(read("fn main() {}\n", false));
        assert!(!buffer.dirty, "a freshly read buffer is clean");

        // A no-op edit is not an edit.
        assert!(!buffer.edit_content(|_| false));
        assert!(!buffer.dirty);

        assert!(buffer.edit_content(|content| {
            content.insert_str(0, "// ");
            true
        }));
        assert!(buffer.dirty);
        assert_eq!(buffer.content.as_deref(), Some("// fn main() {}\n"));

        // Back to the words it started with — still dirty.
        assert!(buffer.edit_content(|content| {
            content.replace_range(0..3, "");
            true
        }));
        assert_eq!(buffer.content.as_deref(), Some("fn main() {}\n"));
        assert!(buffer.dirty, "editing back to the original does not clean");
    }

    /// ⑥ An edit is counted, so a cache keyed on the count sees a change that
    /// kept the length.
    ///
    /// The named bug: `content_len` was the revision counter until this slice,
    /// and swapping one letter for another left every derived document
    /// convinced it was still looking at the old text.
    ///
    /// Mutation: stop incrementing `revision` in [`PreviewBuffer::edit_content`].
    #[test]
    fn an_edit_that_keeps_the_length_still_counts() {
        let mut buffer = PreviewBuffer::new(PathBuf::from(r"C:\w\a.rs"), "a.rs".to_owned());
        buffer.accept(read("abc", false));
        let before = buffer.revision;
        assert!(buffer.edit_content(|content| {
            content.replace_range(2..3, "d");
            true
        }));
        assert_eq!(buffer.content.as_deref(), Some("abd"));
        assert_ne!(
            buffer.revision, before,
            "a same-length edit is still an edit"
        );
        // And the widest line follows the edit rather than the read.
        assert!(buffer.edit_content(|content| {
            content.push_str("\nlonger line");
            true
        }));
        assert_eq!(buffer.max_columns, 11);
    }

    /// The read-only degradation is enforced where the editing is.
    ///
    /// §7.1.3's "超大文件只读降级": a truncated buffer holds the first 64KB of
    /// its file, so an edit surface over it is a save button wired to
    /// `truncate`.
    ///
    /// Mutation: drop the `!self.truncated` clause from
    /// [`PreviewBuffer::is_editable`].
    #[test]
    fn a_truncated_buffer_is_read_only_however_editable_its_name_is() {
        let mut buffer = PreviewBuffer::new(PathBuf::from(r"C:\w\a.rs"), "a.rs".to_owned());
        assert!(!buffer.is_editable(), "a buffer with no body has no caret");
        buffer.accept(read("fn main() {}\n", false));
        assert!(buffer.is_editable());
        buffer.accept(read("fn main() {}\n", true));
        assert!(!buffer.is_editable());
    }

    /// ② A save writes the body to the disk and cleans the buffer.
    ///
    /// Mutation: return [`SaveOutcome::Saved`] without calling
    /// [`save_atomically`], or leave `dirty` set.
    #[test]
    fn a_save_writes_the_body_and_cleans_the_buffer() {
        let dir = scratch("save");
        let mut buffer = opened(&dir, "notes.txt", "one\ntwo\n");
        buffer.edit_content(|content| {
            content.push_str("three\n");
            true
        });
        assert!(buffer.dirty);
        assert_eq!(buffer.save(), SaveOutcome::Saved);
        assert!(!buffer.dirty, "a saved buffer is clean");
        assert_eq!(
            std::fs::read_to_string(&buffer.path).unwrap(),
            "one\ntwo\nthree\n",
            "the bytes on the disk are the bytes in the buffer"
        );
        // The stamp moved with the write, so the very next save is not a
        // conflict with itself.
        buffer.edit_content(|content| {
            content.push_str("four\n");
            true
        });
        assert_eq!(buffer.save(), SaveOutcome::Saved);
        // Nothing is left behind beside the file.
        assert!(!preview_temp_path(&buffer.path).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ③ A write that fails leaves the file it was aimed at untouched.
    ///
    /// The failure is injected the only honest way a filesystem allows: the
    /// staging path is occupied by a *directory*, so creating the staging file
    /// cannot succeed. Under the mutation the target is opened directly, the
    /// truncate has already happened when the failure arrives, and what is left
    /// on the disk is neither version.
    ///
    /// Mutation: write straight to `path` in [`save_atomically`] instead of
    /// staging and renaming.
    #[test]
    fn a_failed_write_leaves_the_original_file_whole() {
        let dir = scratch("atomic");
        let path = dir.join("notes.txt");
        std::fs::write(&path, "the original\n").unwrap();
        std::fs::create_dir_all(preview_temp_path(&path)).unwrap();

        let error = save_atomically(&path, "the replacement\n")
            .expect_err("a staging file cannot be created over a directory");
        assert!(!error.to_string().is_empty());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "the original\n",
            "the target was never opened"
        );

        // With the staging path free again the same call goes through.
        std::fs::remove_dir_all(preview_temp_path(&path)).unwrap();
        save_atomically(&path, "the replacement\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "the replacement\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ④ A file the disk has moved on from is not overwritten.
    ///
    /// Ruling 8⑨'s minimum: the window says so and keeps the edits. The stamp
    /// is set to a time no file has rather than raced against the clock, so the
    /// test asserts the comparison and not the resolution of a filesystem's
    /// timestamps.
    ///
    /// Mutation: drop the `file_mtime(&self.path) != self.disk_mtime` guard,
    /// which is exactly the blind write the ruling forbids.
    #[test]
    fn a_file_that_changed_on_disk_is_not_blindly_overwritten() {
        let dir = scratch("conflict");
        let mut buffer = opened(&dir, "notes.txt", "as it was read\n");
        buffer.edit_content(|content| {
            content.push_str("and as it was edited\n");
            true
        });
        // Somebody else wrote the file after this buffer read it.
        buffer.disk_mtime = Some(SystemTime::UNIX_EPOCH);

        assert_eq!(buffer.save(), SaveOutcome::Conflict);
        assert_eq!(
            std::fs::read_to_string(&buffer.path).unwrap(),
            "as it was read\n",
            "the other writer's file is still theirs"
        );
        assert!(buffer.dirty, "and the edits are still here");
        assert!(!preview_temp_path(&buffer.path).exists());

        // Re-reading the file settles the conflict, and the same save lands.
        buffer.disk_mtime = file_mtime(&buffer.path);
        assert_eq!(buffer.save(), SaveOutcome::Saved);
        assert_eq!(
            std::fs::read_to_string(&buffer.path).unwrap(),
            "as it was read\nand as it was edited\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A superseded question is dropped rather than asked — the files worker's
    /// coalescing, on this lane.
    ///
    /// Mutation: delete the `contains_target` check in [`run_preview_worker`].
    #[test]
    fn a_question_superseded_while_reading_is_dropped_rather_than_asked() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let ask = |path: &str| PreviewRequest {
            tab: crate::TabId(1),
            path: PathBuf::from(path),
            want: PreviewWant::Head,
        };
        sender.send(ask("a.rs")).unwrap();
        sender.send(ask("b.rs")).unwrap();
        sender.send(ask("a.rs")).unwrap();
        drop(sender);
        let mut asked = Vec::new();
        run_preview_worker(receiver, |request| asked.push(request.path.clone()));
        assert_eq!(
            asked,
            vec![PathBuf::from("b.rs"), PathBuf::from("a.rs")],
            "the superseded first question is never read"
        );
    }
}
