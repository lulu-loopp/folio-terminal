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

impl PreviewFtype {
    /// The word the type chip prints (P147, mock-up 6422).
    ///
    /// The mock-up interpolates the ftype string itself into `.fpeek-type`, so
    /// the chip says exactly what the classifier calls the file and there is no
    /// second, prettier vocabulary to keep in step with it. These are those five
    /// strings.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Markdown => "markdown",
            Self::Table => "table",
            Self::Text => "text",
            Self::Unknown => "unknown",
        }
    }
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
    /// `[text](url)` — **the text only** is printed, in the accent colour; the
    /// target rides beside it in [`Span::target`].
    ///
    /// It used to be printed and nothing else: "what a click on a link *does*
    /// is a decision about opening browsers and relative paths that belongs to
    /// the day the preview grows a navigation model." That day is 2026-08-13
    /// and the decision is [`link_action`]. What has not changed is that the
    /// URL is never *printed* — that was wrong under every future ruling and it
    /// is still wrong under this one.
    Link,
}

/// One run of inline text inside a markdown block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: SpanStyle,
    /// Where a [`SpanStyle::Link`] points, exactly as the document wrote it.
    ///
    /// **Unresolved.** A relative target means nothing without the document it
    /// was written in, and the parser does not know which file it is reading —
    /// so it keeps the author's string and [`link_action`] does the resolving,
    /// where the document's own path is in hand. `None` for every other style,
    /// which is what makes "a run that answers a click" a thing the type can
    /// state rather than a convention two modules have to agree on.
    pub target: Option<String>,
}

impl Span {
    fn styled(text: &str, style: SpanStyle) -> Self {
        Self {
            text: text.to_owned(),
            style,
            target: None,
        }
    }

    pub fn plain(text: &str) -> Self {
        Self::styled(text, SpanStyle::Plain)
    }

    pub fn bold(text: &str) -> Self {
        Self::styled(text, SpanStyle::Bold)
    }

    pub fn code(text: &str) -> Self {
        Self::styled(text, SpanStyle::Code)
    }

    pub fn link(text: &str, target: &str) -> Self {
        Self {
            text: text.to_owned(),
            style: SpanStyle::Link,
            target: Some(target.to_owned()),
        }
    }
}

/// One row of a rendered markdown table: one cell per column, each already
/// split into its inline runs.
pub type TableRow = Vec<Vec<Span>>;

/// One block of a rendered markdown document.
///
/// **The support surface is the product's, not the prototype's.** The mock-up's
/// own renderer stops at headings, lists, fences and two inline styles and says
/// so in a comment — "completeness is the product's problem" (4914-4941). This
/// is the product, the file the user read it against is `docs/DESIGN.md`, and
/// what that file uses and the prototype could not draw is exactly the five
/// members below the first four.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MarkdownBlock {
    Heading {
        /// `1..=6`. The prototype allowed `1..=3` and printed the rest as
        /// paragraphs beginning with hashes, which is what a `####` in a real
        /// document looks like when it is not supported.
        level: u8,
        spans: Vec<Span>,
    },
    /// Consecutive list rows, gathered the way the mock-up's `flushList` gathers
    /// them: a list is one block, not one block per bullet.
    List {
        /// `Some(n)` for a `n.`-numbered list, `None` for a bulleted one.
        ///
        /// The *first* number rather than a flag, because a list that starts at
        /// `3.` is a list that starts at 3 — renumbering it from one is the
        /// renderer overruling the document about its own contents.
        ordered: Option<u64>,
        items: Vec<Vec<Span>>,
    },
    Code {
        lang: Option<String>,
        text: String,
    },
    /// `| a | b |` under a `|---|---|` — **and only under one**.
    ///
    /// The separator row is what makes a table a table. Without it a line full
    /// of pipes is a line full of pipes, which is the common case in prose about
    /// shell commands and in ASCII art, and a renderer that tabulated those
    /// would be wrong far more often than it was right.
    Table {
        /// The heading row first, then the body. Never empty: a table exists
        /// only where a heading row was found.
        rows: Vec<TableRow>,
    },
    /// Consecutive `>` rows, one entry per line, gathered as one block so the
    /// accent bar down their left is one bar rather than several.
    Quote(Vec<Vec<Span>>),
    /// `---` or `***` alone on a line.
    Rule,
    Paragraph(Vec<Span>),
}

/// Split one line into its inline runs.
///
/// **Three passes, and the order is the ruling.** Backticks first, which is the
/// mock-up's order (4915-4917) and the one that makes `` `**not bold**` `` come
/// out as literal code rather than as a bold run inside a code span; then links,
/// so a `**[bold link](url)**`'s brackets are gone before the asterisks are
/// read; then asterisks over whatever is still plain.
///
/// **One door for every block that has text in it.** A table cell, a list item,
/// a quote line and a paragraph all come through here, which is the whole of why
/// `` `code` `` inside a table cell works without a line of its own: there is no
/// second inline parser to teach.
pub fn parse_inline(line: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        // A backtick with no partner is a backtick, not the start of anything.
        let Some(close) = rest[open + 1..].find('`') else {
            break;
        };
        push_link_runs(&rest[..open], &mut spans);
        spans.push(Span::code(&rest[open + 1..open + 1 + close]));
        rest = &rest[open + 1 + close + 1..];
    }
    push_link_runs(rest, &mut spans);
    spans
}

/// The second pass: `[text](url)` inside whatever the code pass left plain.
///
/// The label is emitted as one run and the target rides beside it, unresolved.
/// A label carrying its own emphasis (`[**a**](b)`) keeps the asterisks visible
/// rather than nesting two styles in one run — a deliberate floor, because the
/// alternative is a span model with a stack in it and the case is vanishing.
fn push_link_runs(text: &str, spans: &mut Vec<Span>) {
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(label_end) = rest[open..].find(']').map(|at| open + at) else {
            break;
        };
        // The target has to follow the label immediately, which is what keeps
        // `[a] (b)` and a bare `[TODO]` out of this branch.
        if !rest[label_end + 1..].starts_with('(') {
            push_bold_runs(&rest[..label_end + 1], spans);
            rest = &rest[label_end + 1..];
            continue;
        }
        let target_open = label_end + 1;
        let Some(target_end) = rest[target_open..].find(')').map(|at| target_open + at) else {
            break;
        };
        // `[]()` is punctuation, not an empty link.
        if label_end == open + 1 {
            push_bold_runs(&rest[..target_end + 1], spans);
            rest = &rest[target_end + 1..];
            continue;
        }
        push_bold_runs(&rest[..open], spans);
        spans.push(Span::link(
            &rest[open + 1..label_end],
            rest[target_open + 1..target_end].trim(),
        ));
        rest = &rest[target_end + 1..];
    }
    push_bold_runs(rest, spans);
}

/// The third pass: `**bold**` inside whatever the two above left plain.
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

/// Add plain text, **joined to the plain run before it if there is one**.
///
/// The three passes hand each other the text they did not claim, so a line the
/// link pass looked at and left alone comes back in two or three pieces. Two
/// adjacent plain runs shape and draw identically to one, but they are not one:
/// the shaper is given a rich-text sequence and a break opportunity between two
/// runs is not the same thing as one inside a run, so `a [TODO] note` split at
/// the bracket could wrap where the text does not permit it.
fn push_plain(text: &str, spans: &mut Vec<Span>) {
    if text.is_empty() {
        return;
    }
    match spans.last_mut() {
        Some(last) if last.style == SpanStyle::Plain => last.text.push_str(text),
        _ => spans.push(Span::plain(text)),
    }
}

/// The markdown renderer: headings, lists, fences, tables, quotes, rules, links
/// and three inline styles.
///
/// **Indexed rather than streamed, and the reason is the table.** A pipe row is
/// only a table row if the row *after* it is a separator, so the scanner needs
/// one line of lookahead; every other block is decidable from its own first
/// line. Written as an index walk rather than an iterator with a peek because
/// the table then consumes its own run in one place instead of leaving a
/// half-open state machine for the next four blocks to step around.
///
/// What is not negotiable is the honesty this inherits from the prototype: it
/// renders **the argument**, because the mock-up's first rendered view was a
/// static mock that showed the same document whatever the buffer held (P103).
///
/// **A paragraph is a run of lines, not a line** (user ruling, 2026-08-13, and
/// CommonMark §4.8). The prototype emitted one block per source line, which is
/// invisible against a document written unwrapped and is the whole of the
/// reported seam against one written wrapped: `docs/DESIGN.md` folds at eighty
/// columns, so every paragraph in it arrived as five blocks with a paragraph gap
/// between each pair. Consecutive non-blank lines are gathered here, joined with
/// a single space, and handed to [`parse_inline`] **once** — which is also
/// CommonMark's order, and the reason emphasis opened on one source line and
/// closed on the next comes out as one run instead of two literal asterisk
/// pairs. Every other block still interrupts prose on its own first line, so the
/// gathering can never swallow a heading, a fence, a rule, a table, a quote or a
/// list marker.
pub fn parse_markdown(src: &str) -> Vec<MarkdownBlock> {
    let lines: Vec<&str> = src.lines().collect();
    let mut blocks = Vec::new();
    // Both accumulators hold **source text**, not spans, because both of them
    // join across source lines and inline parsing has to see the joined text.
    let mut list: Vec<String> = Vec::new();
    let mut ordered: Option<u64> = None;
    let mut paragraph: Vec<&str> = Vec::new();
    let mut index = 0usize;

    while index < lines.len() {
        let line = lines[index];

        // ── the fence, which swallows everything until it closes ────────────
        if let Some(rest) = line.strip_prefix("```") {
            flush_paragraph(&mut paragraph, &mut blocks);
            flush_list(&mut list, &mut ordered, &mut blocks);
            let lang = rest.trim();
            let lang = (!lang.is_empty()).then(|| lang.to_owned());
            let mut body = Vec::new();
            index += 1;
            while index < lines.len() && !lines[index].starts_with("```") {
                body.push(lines[index]);
                index += 1;
            }
            // A fence nobody closed still draws, rather than swallowing the rest
            // of the document in silence (mock-up 4939) — which is what the
            // `index < len` bound above means when the loop runs off the end.
            index += usize::from(index < lines.len());
            blocks.push(MarkdownBlock::Code {
                lang,
                text: body.join("\n"),
            });
            continue;
        }

        // ── the table, which is the one block needing lookahead ─────────────
        if is_pipe_row(line)
            && lines
                .get(index + 1)
                .is_some_and(|next| is_table_separator(next))
        {
            flush_paragraph(&mut paragraph, &mut blocks);
            flush_list(&mut list, &mut ordered, &mut blocks);
            let mut rows = vec![split_pipe_row(line)];
            // Past the separator, then every pipe row that follows without a
            // break. A blank line ends the table exactly as it ends a paragraph.
            index += 2;
            while index < lines.len() && is_pipe_row(lines[index]) {
                rows.push(split_pipe_row(lines[index]));
                index += 1;
            }
            blocks.push(MarkdownBlock::Table { rows });
            continue;
        }

        index += 1;

        if let Some(heading) = parse_heading(line) {
            flush_paragraph(&mut paragraph, &mut blocks);
            flush_list(&mut list, &mut ordered, &mut blocks);
            blocks.push(heading);
            continue;
        }
        // **After the table and before the list**, which is what keeps a `---`
        // that is a table's separator out of here and a `- item` out of the
        // rule: a separator is only ever reached by the branch above (which
        // consumed it), and a rule is three or more of one character with
        // nothing else on the line, which `- item` is not.
        if is_thematic_break(line) {
            flush_paragraph(&mut paragraph, &mut blocks);
            flush_list(&mut list, &mut ordered, &mut blocks);
            blocks.push(MarkdownBlock::Rule);
            continue;
        }
        if let Some((number, item)) = parse_list_row(line) {
            flush_paragraph(&mut paragraph, &mut blocks);
            // A bulleted list and a numbered one standing next to each other are
            // two lists, not one list that changes its mind halfway down.
            if !list.is_empty() && ordered.is_some() != number.is_some() {
                flush_list(&mut list, &mut ordered, &mut blocks);
            }
            if list.is_empty() {
                ordered = number;
            }
            list.push(item.trim().to_owned());
            continue;
        }
        if let Some(first) = strip_quote(line) {
            flush_paragraph(&mut paragraph, &mut blocks);
            flush_list(&mut list, &mut ordered, &mut blocks);
            // A quote's own lines gather exactly as prose does — a wrapped quote
            // is one quoted paragraph — and a bare `>` is the blank line that
            // separates two of them.
            let mut quoted = Vec::new();
            let mut run: Vec<&str> = Vec::new();
            let push_run = |run: &mut Vec<&str>, quoted: &mut Vec<Vec<Span>>| {
                if !run.is_empty() {
                    quoted.push(parse_inline(&join_source_lines(run)));
                    run.clear();
                }
            };
            let mut quoted_line = Some(first);
            while let Some(text) = quoted_line {
                if text.trim().is_empty() {
                    push_run(&mut run, &mut quoted);
                } else {
                    run.push(text);
                }
                quoted_line = lines.get(index).and_then(|line| strip_quote(line));
                index += usize::from(quoted_line.is_some());
            }
            push_run(&mut run, &mut quoted);
            blocks.push(MarkdownBlock::Quote(quoted));
            continue;
        }
        if line.trim().is_empty() {
            flush_paragraph(&mut paragraph, &mut blocks);
            flush_list(&mut list, &mut ordered, &mut blocks);
            continue;
        }
        // **Lazy continuation** (CommonMark §5.2): a plain line under an open
        // list belongs to the item above it, not to a paragraph of its own. A
        // bullet that wraps in the source is one bullet, which is the same
        // ruling the paragraph join is, applied where the text is indented under
        // a marker instead of standing on its own.
        match list.last_mut() {
            Some(item) if paragraph.is_empty() => {
                item.push(' ');
                item.push_str(line.trim());
            }
            _ => paragraph.push(line),
        }
    }
    flush_paragraph(&mut paragraph, &mut blocks);
    flush_list(&mut list, &mut ordered, &mut blocks);
    blocks
}

/// The source lines of one paragraph, as the single line CommonMark reads them
/// as.
///
/// Joined with a space and each line trimmed, which is what a soft line break
/// renders as. Trimming is what makes an indented continuation line join
/// cleanly; joining rather than concatenating is what keeps the last word of one
/// source line from running into the first word of the next.
fn join_source_lines(lines: &[&str]) -> String {
    lines
        .iter()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join(" ")
}

fn flush_paragraph(paragraph: &mut Vec<&str>, blocks: &mut Vec<MarkdownBlock>) {
    if paragraph.is_empty() {
        return;
    }
    let text = join_source_lines(paragraph);
    paragraph.clear();
    blocks.push(MarkdownBlock::Paragraph(parse_inline(&text)));
}

/// `#` through `######` followed by a space.
///
/// Six levels rather than the mock-up's three (`#{1,3}`), because a `####` in a
/// real document rendered as a paragraph beginning with four hashes is precisely
/// what "the prototype stops here" looks like from the outside.
fn parse_heading(line: &str) -> Option<MarkdownBlock> {
    let hashes = line.len() - line.trim_start_matches('#').len();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = line[hashes..].strip_prefix(' ')?;
    Some(MarkdownBlock::Heading {
        level: hashes as u8,
        spans: parse_inline(rest),
    })
}

/// One list row: its number if it had one, and what it says.
fn parse_list_row(line: &str) -> Option<(Option<u64>, &str)> {
    if let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        return Some((None, item));
    }
    let digits = line.len()
        - line
            .trim_start_matches(|ch: char| ch.is_ascii_digit())
            .len();
    if digits == 0 {
        return None;
    }
    let item = line[digits..].strip_prefix(". ")?;
    // A number too long to be a number is prose that happens to start with
    // digits; `u64` overflowing is the honest edge to refuse at.
    Some((Some(line[..digits].parse().ok()?), item))
}

/// A quoted line, without its marker. `>` alone is an empty quoted line.
fn strip_quote(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('>')?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

/// `---`, `***` or `___` alone on a line.
///
/// Three or more of one character and nothing else. **Not** a setext heading
/// underline, which is what CommonMark would call a `---` under a paragraph —
/// the preview has no setext headings, so reading it as a rule is the reading
/// that is right in every case this renderer can distinguish.
fn is_thematic_break(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    matches!(first, '-' | '*' | '_')
        && trimmed.chars().count() >= 3
        && trimmed.chars().all(|ch| ch == first)
}

/// Whether a line is shaped like a table row: a pipe somewhere in it, and
/// something other than pipes and spaces.
fn is_pipe_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('|') && trimmed.chars().any(|ch| ch != '|' && !ch.is_whitespace())
}

/// Whether a line is the `|---|:--:|` under a heading row.
fn is_table_separator(line: &str) -> bool {
    let cells = split_pipe_cells(line);
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let cell = cell.trim().trim_start_matches(':').trim_end_matches(':');
            !cell.is_empty() && cell.chars().all(|ch| ch == '-')
        })
}

/// One table row's cells, still as text.
///
/// The leading and trailing pipes are optional and are not cells: `| a | b |`
/// and `a | b` are the same two columns, which is what every markdown renderer
/// agrees on and what a document written by hand relies on.
fn split_pipe_cells(line: &str) -> Vec<&str> {
    let trimmed = line.trim();
    let trimmed = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix('|').unwrap_or(trimmed);
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed.split('|').collect()
}

/// One table row, each cell already split into its inline runs.
fn split_pipe_row(line: &str) -> TableRow {
    split_pipe_cells(line)
        .into_iter()
        .map(|cell| parse_inline(cell.trim()))
        .collect()
}

fn flush_list(list: &mut Vec<String>, ordered: &mut Option<u64>, blocks: &mut Vec<MarkdownBlock>) {
    if !list.is_empty() {
        blocks.push(MarkdownBlock::List {
            ordered: *ordered,
            // Parsed here rather than as each row arrives, because a row may
            // still grow: an item's continuation lines are appended to its
            // source, and inline runs cut before the last of them would split a
            // code span or an emphasis pair across the fold.
            items: std::mem::take(list)
                .iter()
                .map(|item| parse_inline(item))
                .collect(),
        });
    }
    *ordered = None;
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

/// The read-only degradation §7.1.3 asks for, **as the phrase the pane's foot
/// hangs on its right hand** (user ruling, 2026-08-15).
///
/// It was a sentence — "Read-only — showing the first 64 KB of this file" —
/// standing in a 28px bar of its own directly above the path strip. The ruling
/// retired that bar (two strips of identical height stacked at the bottom of one
/// pane), and a phrase that has to share a 28px strip with a path is a phrase
/// that says the two facts and stops: what the file is (read-only) and how much
/// of it you are looking at.
///
/// The size is [`PREVIEW_HEAD_BYTES`] said the way [`format_byte_size`] says it,
/// pinned by a test rather than left as two numbers that can drift apart.
pub const PREVIEW_TRUNCATED_NOTICE: &str = "Read-only · 64 KB";

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

    /// Whether this buffer would be shown on a surface that edits, **as the
    /// surface asking is showing it**.
    ///
    /// The name's judgement ([`is_editable`]) **and two facts only a body
    /// knows**. A buffer with no body has nothing to put a caret in; a
    /// *truncated* one has only the first 64KB of its file, and an edit surface
    /// over the head of a file is a save button wired to `truncate`. §7.1.3's
    /// "超大文件只读降级" is exactly this line — the degradation is read-only,
    /// and read-only has to be enforced where the editing is, not where the
    /// notice is printed.
    ///
    /// `md_source` is the *view's*, not the buffer's, and that is the 2026-08-13
    /// ruling: a rendered markdown page has nothing to type into and its source
    /// has, so whether this file is editable right now is a question about the
    /// surface looking at it. Two surfaces on one markdown file can answer it
    /// differently at the same moment, and both are right.
    pub fn is_editable(&self, md_source: bool) -> bool {
        self.load == PreviewLoad::Ready
            && self.content.is_some()
            && !self.truncated
            && is_editable(&self.name, self.ftype, md_source)
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

    /// Which body this buffer is drawn as **on the surface asking**.
    ///
    /// Parameterised for [`Self::is_editable`]'s reason: the flip is the view's,
    /// so a markdown file is a rendered page in one pane and a text surface in
    /// another at the same instant.
    pub fn view(&self, md_source: bool) -> PreviewView {
        preview_view(&self.name, self.ftype, md_source)
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

    /// Lift a buffer **out** of this pool, whole.
    ///
    /// The migrating half of the one case where a buffer changes tabs: a preview
    /// float docked into a tab that is not the one it was born in. It is not
    /// `open`'s business and must not be — `open` is the only door that ever
    /// reads a disk, and a buffer that has travelled is a buffer whose edits and
    /// whose dirty bit have to arrive intact rather than be read again.
    pub fn take(&mut self, path: &Path) -> Option<PreviewBuffer> {
        let index = self.index_of(path)?;
        Some(self.buffers.remove(index))
    }

    /// Put a buffer **in**, replacing whatever was under that path.
    ///
    /// [`Self::take`]'s other half. It does not evict: the cap is `open`'s law
    /// and is about a pool growing by *browsing*, while this is one buffer moving
    /// house — and a migration that silently dropped somebody's unsaved edit to
    /// keep a ceiling would be the ceiling costing exactly what it is not worth
    /// (see `open`'s own note).
    pub fn insert(&mut self, buffer: PreviewBuffer) {
        match self.index_of(&buffer.path) {
            Some(index) => self.buffers[index] = buffer,
            None => self.buffers.push(buffer),
        }
    }

    /// **P122 — one buffer arriving from another tab's pool, merged in under the
    /// law: one buffer per file, dirty wins, a tie stays with the incumbent.**
    ///
    /// The single-buffer form because there are two callers and they differ only
    /// in how many buffers they hand over: a whole tab merging in gives its whole
    /// pool ([`Self::merge_from`]), a float docking into a tab it was not born in
    /// gives exactly the one it is carrying. Writing the law twice is how the two
    /// gestures would come to disagree about what "dirty wins" means.
    ///
    /// **Why a tie leaves the incumbent standing.** Two clean copies of a file
    /// are the same file, so nothing is at stake and the cheapest correct answer
    /// is to change nothing — and two *dirty* copies are the one case §7.1.3
    /// declines to arbitrate ("共享范围到 tab 为止,同文件跨 tab 的并发编辑留给
    /// 产品端磁盘冲突检测"). Preferring the arrival there would silently discard
    /// unsaved work that has been sitting in this tab, which is the one outcome
    /// the whole dirty-gate apparatus exists to prevent.
    ///
    /// **The winner takes the loser's place in the list, not the end of it.** The
    /// order is the history the switcher lists (see the type's own note), and it
    /// is the *staying* tab's history: a file that has been open here all along
    /// does not jump to the front because a copy of it walked in.
    ///
    /// **The revision is bumped past both.** A revision counts changes to *one*
    /// buffer's body, so two buffers' counters mean nothing to each other — and a
    /// surface showing this path caches its parsed document against
    /// `(path, revision)`. Without the bump, an arrival whose counter happens to
    /// match the buffer it replaced would leave every pane on that file drawing
    /// the body it no longer holds.
    ///
    /// Panes need no redirecting afterwards, and that falls out of the port
    /// rather than being arranged: a surface names its buffer **by path**
    /// ([`crate::PreviewPane::buffer`] is a `PathBuf`), so "one buffer per file"
    /// and "every pane showing the loser now reads the winner" are the same
    /// sentence here. The mock-up needed a redirect pass because its panes held
    /// object references.
    pub fn merge_buffer(&mut self, mut incoming: PreviewBuffer) {
        let Some(index) = self.index_of(&incoming.path) else {
            self.buffers.push(incoming);
            return;
        };
        let twin = &self.buffers[index];
        let (twin_is_dirty, twin_revision) = (twin.dirty, twin.revision);
        if incoming.dirty && !twin_is_dirty {
            incoming.revision = incoming.revision.max(twin_revision) + 1;
            self.buffers[index] = incoming;
        }
    }

    /// **P122/P127 — a whole pool arriving, buffer by buffer.**
    ///
    /// Taken **by value** because that is the ruling: the pool travels, it is not
    /// copied. §7.1.3's "整池随行" is about orphaned dirty buffers staying
    /// reachable somewhere, and a source left holding a second copy of everything
    /// would be exactly the fork the one-buffer-per-file law forbids — with the
    /// added cruelty that the copy nobody can reach is the one the dirty gates
    /// would go on asking about.
    ///
    /// In pool order, so the arriving tab's own history keeps its shape among the
    /// entries this tab has never seen.
    pub fn merge_from(&mut self, incoming: PreviewPool) {
        for buffer in incoming.buffers {
            self.merge_buffer(buffer);
        }
    }

    fn index_of(&self, path: &Path) -> Option<usize> {
        self.buffers.iter().position(|buffer| buffer.path == path)
    }

    /// Every buffer in the pool, in the order the switcher lists them.
    ///
    /// The order *is* the history (see the type's own note), so this is the one
    /// door the switcher, the count badge and the dirty gates all read through.
    pub fn buffers(&self) -> impl Iterator<Item = &PreviewBuffer> {
        self.buffers.iter()
    }

    /// The names of every dirty buffer, **except the one a pane is showing**
    /// when `shown` names it.
    ///
    /// Two readers with one question between them. The header's count badge asks
    /// it with `shown = Some(the buffer on screen)`, because the pane's own dot
    /// already speaks for that one and a badge that also lit for it would be the
    /// same fact twice (P19's `othersDirty`); the three dirty gates ask it with
    /// `None`, because a gate is about *everything* that would be lost (P120).
    pub fn dirty_names(&self, shown: Option<&Path>) -> impl Iterator<Item = &str> {
        self.buffers
            .iter()
            .filter(move |buffer| buffer.dirty && Some(buffer.path.as_path()) != shown)
            .map(|buffer| buffer.name.as_str())
    }

    /// Forget everything. The two gates that take a pool's *home* away call it
    /// once the user has said the edits may go (P123/P124): closing the last
    /// preview pane strands the pool, and closing the tab is the pool's owner
    /// going away.
    pub fn clear(&mut self) {
        self.buffers.clear();
    }

    /// Forget only what the user just agreed to lose.
    ///
    /// **The shut gate's half of [`Self::clear`], and the difference is that a
    /// shut does not take the pool's home away** — the tab is about to be
    /// written to `session.json`, and its pool goes with it as the list of files
    /// the switcher will list on the next launch. Emptying it outright would
    /// answer "discard my unsaved changes" by also discarding the browsing
    /// history, which is a second thing the user was never asked about.
    ///
    /// It still satisfies what the gate needs of it: the gate raises itself off
    /// [`Self::dirty_names`], so a pool with no dirty buffer left in it cannot
    /// ask the question a second time.
    pub fn discard_dirty(&mut self) {
        self.buffers.retain(|buffer| !buffer.dirty);
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
/// and since 2026-08-15 it is printed there on every surface that has one — a
/// docked pane, a torn-off float — in the strip's **left** hand, where the
/// reveal's confirmation goes, while the strip's right hand steps aside for as
/// long as it stands.
pub const PREVIEW_SAVED_NOTICE: &str = "Saved";

/// What the window says instead of overwriting somebody else's write.
///
/// It says what happened, what was *not* done, and what is still true — the
/// edits are still here — because a conflict notice that only announces failure
/// leaves the user believing their work is gone.
///
/// **All three facts, in a phrase** (user ruling, 2026-08-15). It used to be a
/// full sentence on a floating strip inside the body; the ruling moved every
/// standing notice to the right hand of the path foot, so the sentence had to
/// become something that fits beside a path. What it must not lose is the third
/// clause, and it has not: a user who reads only "Not saved" is the reader this
/// wording exists for.
pub const PREVIEW_CONFLICT_NOTICE: &str = "Not saved · changed on disk · edits kept";

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

/// What a click on a markdown link does (user ruling, 2026-08-13).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkAction {
    /// A file, resolved — it opens **here**, in this window's own preview.
    Preview(PathBuf),
    /// A web address, for the system browser.
    Browse(String),
    /// Nothing this window will act on.
    Nowhere,
}

/// Resolve a link target written in `document` into what pressing it does.
///
/// # The ruling
///
/// **A link that points at a file is a way of pointing at a file**, and this
/// window has exactly one answer for that: show it in the preview. It is the
/// same sentence [`crate::files_row_activation`] makes about a row in the tree
/// and the file menu makes about its first item — 指到文件=预览它 — and a
/// third answer for the third door would be three things to keep in step.
/// Anything the preview cannot read is *still* previewed: it lands on the seat
/// as an unknown buffer and the card offers 「Open in default app」, which is
/// the escape hatch chosen rather than the fork fallen down.
///
/// `http`/`https` keep going to the system browser, which is where the web
/// has always gone. **Every other scheme is refused** — `mailto:`, `ftp:`,
/// `javascript:` and whatever else a document may carry — for the reason the
/// terminal's own OSC-8 handler refuses them: a document is untrusted text, and
/// handing an arbitrary scheme to `ShellExecute` is handing it whatever the
/// machine has registered for that scheme.
///
/// **「Open the containing folder」 is not here**, deliberately. That is the
/// foot's Reveal button and it stays the foot's: a link names a *file*, and
/// answering it with its parent directory is answering a question nobody asked.
///
/// Resolution rules, in order:
///
/// * an empty target, or a bare `#fragment`, is nothing — there is no
///   within-document navigation to do yet, and jumping to the top would be a
///   worse answer than none;
/// * a trailing `#fragment` is **cut** off a path first: `DESIGN.md#7.1.2`
///   names `DESIGN.md`, and the anchor is simply a part of the address this
///   window cannot honour yet;
/// * `file:` is unwrapped to the path it carries, percent-escapes and all;
/// * anything else carrying a `scheme:` is refused, *except* that a bare
///   Windows drive letter (`C:\x`) is a path and not a scheme — one letter
///   before the colon cannot be a scheme, and RFC 3986 says so too;
/// * an absolute path is taken as it stands; a relative one is resolved
///   against the **document's own directory**, which is the only frame a
///   relative link has ever meant.
#[must_use]
pub fn link_action(target: &str, document: &Path) -> LinkAction {
    let target = target.trim();
    if target.is_empty() || target.starts_with('#') {
        return LinkAction::Nowhere;
    }
    let lower = target.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return LinkAction::Browse(target.to_owned());
    }
    let path = if lower.starts_with("file:") {
        let Some(path) = file_url_path(target) else {
            return LinkAction::Nowhere;
        };
        path
    } else if let Some(scheme) = scheme_of(target) {
        // A drive letter is not a scheme; every real scheme left here is one
        // this window does not open.
        if scheme.len() > 1 {
            return LinkAction::Nowhere;
        }
        PathBuf::from(strip_fragment(target))
    } else {
        PathBuf::from(strip_fragment(target))
    };
    if path.as_os_str().is_empty() {
        return LinkAction::Nowhere;
    }
    if path.is_absolute() {
        return LinkAction::Preview(normalized(&path));
    }
    match document.parent() {
        Some(directory) => LinkAction::Preview(normalized(&directory.join(path))),
        // A document with no directory is one with no relative frame; there is
        // nowhere for the link to be relative *to*.
        None => LinkAction::Nowhere,
    }
}

/// Fold `.` and `..` out of a path, **textually**.
///
/// # Why lexically, and why at all (user report, 2026-08-13)
///
/// The first version left the climb in — `…\preview-samples\../../docs/DESIGN.md`
/// — on the reasoning that the file system resolves `..` anyway and folding it
/// here would be guessing about symlinks. Opening the file worked. Everything
/// *else* did not: the foot printed that string at the user, and Explorer's
/// `/select` was handed it and quietly opened the wrong folder. A path that
/// leaves this window — into a caption, into another program's command line —
/// has to be the path a person would have written.
///
/// **Lexically is the correct algorithm here, not a shortcut.** A markdown link
/// is resolved the way a URL reference is (RFC 3986 §5.2.4 removes `..`
/// segments by pure string surgery, before anything is dereferenced), so
/// folding the text *is* what the author meant. `canonicalize` would be the
/// wrong tool twice over: it asks the disk, so it fails for a link to a file
/// that does not exist yet, and it returns a `\\?\` extended path that no
/// caption should ever show.
fn normalized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // A `..` climbs over a real name; one at the root has nothing
                // above it to climb, and one already following a `..` is part
                // of the same climb rather than the end of it.
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else if out.has_root() {
                    // Above the root there is nothing. Windows agrees: `C:\..`
                    // is `C:\`.
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The `scheme` of `scheme:rest`, when the text in front of the first colon
/// looks like one (RFC 3986: a letter, then letters, digits, `+`, `-`, `.`).
fn scheme_of(target: &str) -> Option<&str> {
    let colon = target.find(':')?;
    let scheme = &target[..colon];
    let mut characters = scheme.chars();
    let first = characters.next()?;
    (first.is_ascii_alphabetic()
        && characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    .then_some(scheme)
}

/// Everything before a trailing `#anchor`.
fn strip_fragment(target: &str) -> &str {
    target.split_once('#').map_or(target, |(path, _)| path)
}

/// The path inside a `file:` URL — `file:///C:/a/b`, `file://host/share/a` and
/// the abbreviated `file:/C:/a` alike, with percent-escapes undone.
fn file_url_path(target: &str) -> Option<PathBuf> {
    let rest = strip_fragment(target).get("file:".len()..)?;
    // `file://host/share` is a UNC path and keeps its two leading slashes;
    // `file:///C:/x` and `file:/C:/x` are local and lose all of theirs.
    let local = rest.strip_prefix("//").map_or(rest, |authority| {
        authority.strip_prefix('/').unwrap_or(authority)
    });
    let text = if rest.starts_with("//") && !rest.starts_with("///") {
        format!(r"\\{}", percent_decode(local))
    } else {
        percent_decode(local.trim_start_matches('/'))
    };
    (!text.is_empty()).then(|| PathBuf::from(text.replace('/', r"\")))
}

/// `%20` and its kin, undone. A `%` that does not begin a valid escape is a
/// literal `%`, which is what every lenient reader does and what a hand-written
/// link most often means.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escape = (bytes[index] == b'%')
            .then(|| {
                text.get(index + 1..index + 3)
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            })
            .flatten();
        match escape {
            Some(byte) => {
                out.push(byte);
                index += 3;
            }
            None => {
                out.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// How thick a scrolling region's bar is *drawn*.
pub const BLOCK_SCROLL_THICKNESS_LOGICAL_PX: f32 = 2.0;
/// How thick it is to a **hand** — the divider's `SEAT_DIVIDER_HIT_LOGICAL_PX`
/// and for its reason: one drawn pixel is not a target.
///
/// The reported bug (2026-08-12) was that the bar could not be dragged at all;
/// half of the answer was giving it a drag, and the other half is admitting
/// that two pixels is not something anyone can put a pointer on. The band is
/// grown around the drawn rule on every side, so the tolerance is the same
/// whether the approach is from inside the block or from the gap below it.
pub const BLOCK_SCROLL_HIT_LOGICAL_PX: f32 = 7.0;
/// How far **inward** a bar riding a *surface's own edge* reaches for a hand
/// (real-machine finding, 2026-08-14).
///
/// # The pixels at a surface's edge are already somebody's
///
/// [`BLOCK_SCROLL_HIT_LOGICAL_PX`] grows a bar's target on every side, and that
/// is right for a block's bar: it lies inside a document, and the pixels just
/// past it are the same document's. A *surface's* bar is at the surface's own
/// edge, and what lies past that edge is never content — it is the next
/// sovereign band along, and both of the ones this window has are wider than the
/// growth:
///
/// * a **divider** between two panes claims [`crate::seats`]'s
///   `SEAT_DIVIDER_HIT_LOGICAL_PX` around the seam, and it claims it first —
///   the pane bar of a preview with a neighbour lies entirely inside that band;
/// * the **window's own resize border** claims eight logical pixels of the far
///   edge before the application is asked at all. A press there is a
///   `WM_NCHITTEST` answer, not a pointer event: the pane bar of the right-most
///   preview never reached this window's code.
///
/// Measured on a real window (2026-08-14): a docked preview's bar could be seen,
/// tracked the wheel exactly, and could not be taken by any hand — at the window
/// edge because the press was a resize, at a seam because the press was a
/// divider.
///
/// So the *picture* stays where the ruling put it — a rule on the surface's own
/// far edge — and the *target* grows inward instead, which is what every overlay
/// scrollbar on the desk does when a pointer approaches it. Nothing is taken
/// from either sovereign: the band grows into the surface's own content, and the
/// far side is clamped to the edge rather than reaching across it.
pub const BODY_SCROLL_INWARD_HIT_LOGICAL_PX: f32 = 16.0;

/// The shortest a thumb may be *drawn* (ruling 2026-08-14, both axes): the
/// honest proportional share of a long document collapses toward one pixel,
/// and a thumb that cannot be seen cannot be taken. Every desktop scrollbar
/// floors its thumb for the same reason.
pub const BLOCK_SCROLL_MIN_THUMB_LOGICAL_PX: f32 = 24.0;

/// Which way a scrolling region runs beneath its bar.
///
/// The axis is carried on the bar rather than known by its callers because the
/// *drag* has to read it back: "how far along the track did the hand get" is a
/// question about x for one of these and about y for the other, and a drag that
/// guessed would move the content sideways when the thumb went down.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollAxis {
    /// A markdown block too wide for its page: a rule along the bottom edge, the
    /// thumb travelling left to right.
    Horizontal,
    /// A glance card whose document is taller than the card: a rule down the
    /// right edge, the thumb travelling top to bottom.
    Vertical,
}

/// The scroll bar a region wears along the edge it overflows past.
///
/// One answer for the painter, the hit test and the drag alike: a thumb drawn
/// somewhere the pointer is not tested is a thumb that looks draggable and
/// is not, which is the whole of the bug this replaced.
///
/// **Grown to two axes on 2026-08-14**, when the glance card became a surface a
/// hand could scroll. The card's bar is this one stood on its end: the same
/// proportion, the same thickness, the same grab tolerance, the same linear map
/// from thumb to offset. Copying it into a second function is how two scrollbars
/// that are the same scrollbar drift apart — the block's bar has already been
/// through one round of "the picture and the hit test disagreed", and that is
/// the bug a copy re-opens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollBar {
    pub axis: ScrollAxis,
    /// The full-length rule the thumb runs along.
    pub track: [f32; 4],
    /// The visible share of the content, drawn in proportion.
    pub thumb: [f32; 4],
    /// The thumb widened to something a hand can land on.
    pub grab: [f32; 4],
    /// How far the thumb's leading edge may travel along the track.
    pub travel: f32,
    /// How far the content may travel under the region's own rectangle.
    pub overflow: f32,
}

impl ScrollBar {
    /// Where the track begins, on the bar's own axis — the origin every
    /// travelled distance is measured from.
    #[must_use]
    pub fn track_start(&self) -> f32 {
        match self.axis {
            ScrollAxis::Horizontal => self.track[0],
            ScrollAxis::Vertical => self.track[1],
        }
    }

    /// The same bar with its target reaching `inward` pixels **into** the
    /// surface, and not one pixel past the surface's own edge.
    ///
    /// See [`BODY_SCROLL_INWARD_HIT_LOGICAL_PX`] for what lives on the other
    /// side of that edge and why nothing may be taken from it. The growth along
    /// the bar's own axis is left exactly as [`scroll_bar`] made it — the ends
    /// of a thumb are as hard to land on as they ever were, and nothing on that
    /// axis belongs to anybody else.
    #[must_use]
    pub fn grown_inward(self, inward: f32) -> Self {
        let grab = match self.axis {
            ScrollAxis::Vertical => [
                (self.thumb[0] - inward).min(self.grab[0]),
                self.grab[1],
                self.track[2],
                self.grab[3],
            ],
            ScrollAxis::Horizontal => [
                self.grab[0],
                (self.thumb[1] - inward).min(self.grab[1]),
                self.grab[2],
                self.track[3],
            ],
        };
        Self { grab, ..self }
    }

    /// Where a pointer at `at` stands on the bar's own axis.
    #[must_use]
    pub fn along(&self, at: [f32; 2]) -> f32 {
        match self.axis {
            ScrollAxis::Horizontal => at[0],
            ScrollAxis::Vertical => at[1],
        }
    }
}

/// The bar for a region of `content` pixels shown through `clip` along `axis`,
/// scrolled by `offset` — or `None` when the whole of it fits and there is
/// nothing to say.
///
/// The two axes are the same six lines of arithmetic read against different
/// components of the rectangle, which is why they are one function: the page is
/// the clip's extent along the axis, the rule lies against the clip's far edge
/// across it, and the thumb is the visible share of the content placed in
/// proportion. Nothing about "wide block" or "tall card" survives into the
/// numbers.
#[must_use]
pub fn scroll_bar(
    clip: [f32; 4],
    axis: ScrollAxis,
    offset: f32,
    content: f32,
    scale: f32,
) -> Option<ScrollBar> {
    // `near`/`far` bound the page along the axis; `edge` is the side the rule
    // lies against — the bottom of a horizontal region, the right of a vertical
    // one, which is where every scrollbar on the desk puts it.
    let (near, far, edge) = match axis {
        ScrollAxis::Horizontal => (clip[0], clip[2], clip[3]),
        ScrollAxis::Vertical => (clip[1], clip[3], clip[2]),
    };
    let page = (far - near).max(1.0);
    let overflow = content - page;
    if overflow <= 0.0 {
        return None;
    }
    let thickness = (BLOCK_SCROLL_THICKNESS_LOGICAL_PX * scale).round().max(1.0);
    let rule = edge - thickness;
    // The proportional length, floored at a graspable minimum (ruling
    // 2026-08-14): a document long enough shrinks the honest share to a
    // one-pixel sliver, and a thumb that cannot be seen cannot be taken. The
    // floor is capped by the page itself; the travel mapping below stays linear
    // over whatever travel remains.
    let length = (page * (page / content))
        .max(BLOCK_SCROLL_MIN_THUMB_LOGICAL_PX * scale)
        .min(page)
        .max(1.0);
    let travel = (page - length).max(0.0);
    let start = near + travel * (offset.clamp(0.0, overflow) / overflow);
    let (track, thumb) = match axis {
        ScrollAxis::Horizontal => ([near, rule, far, edge], [start, rule, start + length, edge]),
        ScrollAxis::Vertical => ([rule, near, edge, far], [rule, start, edge, start + length]),
    };
    // Grown on every side by the same amount, the way `seats::hit_band` grows a
    // divider: the tolerance is a property of the hand, not of the direction it
    // comes from.
    let grow = ((BLOCK_SCROLL_HIT_LOGICAL_PX * scale - thickness) / 2.0).max(0.0);
    Some(ScrollBar {
        axis,
        track,
        thumb,
        grab: [
            thumb[0] - grow,
            thumb[1] - grow,
            thumb[2] + grow,
            thumb[3] + grow,
        ],
        travel,
        overflow,
    })
}

/// Where a thumb dragged to `along` — the pointer's coordinate on the bar's own
/// axis, held `grab` pixels from the thumb's own leading edge — leaves the
/// region's offset.
///
/// **Linear in the track, clamped at both ends by the same numbers the wheel
/// clamps by**: a thumb is a picture of the offset, so dragging it is reading
/// that picture backwards and nothing else. A track with no travel (a thumb as
/// long as its track, which cannot happen while `overflow > 0`) answers zero
/// rather than dividing by it.
#[must_use]
pub fn scroll_dragged_to(bar: &ScrollBar, along: f32, grab: f32) -> f32 {
    if bar.travel <= 0.0 {
        return 0.0;
    }
    let travelled = (along - grab - bar.track_start()) / bar.travel;
    (travelled * bar.overflow).clamp(0.0, bar.overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ruling 2026-08-14, both axes: however long the document, the thumb is
    /// never drawn shorter than a hand can see and take — and the floor costs
    /// the mapping nothing, because the drag still reads linearly over the
    /// travel that remains and reaches both ends of the clamp.
    #[test]
    fn a_thumb_is_never_thinner_than_a_hand_and_still_reaches_both_ends() {
        let scale = 2.0;
        for axis in [ScrollAxis::Vertical, ScrollAxis::Horizontal] {
            let bar = scroll_bar([0.0, 0.0, 300.0, 264.0], axis, 0.0, 50_000.0, scale)
                .expect("fifty thousand pixels overflow any card");
            let length = match axis {
                ScrollAxis::Horizontal => bar.thumb[2] - bar.thumb[0],
                ScrollAxis::Vertical => bar.thumb[3] - bar.thumb[1],
            };
            assert!(
                length >= BLOCK_SCROLL_MIN_THUMB_LOGICAL_PX * scale,
                "{axis:?}: a {length}px thumb is a sliver, not a handle"
            );
            assert_eq!(
                scroll_dragged_to(&bar, bar.track_start(), 0.0),
                0.0,
                "{axis:?}: the near end of the track is still offset zero"
            );
            assert_eq!(
                scroll_dragged_to(&bar, bar.track_start() + bar.travel, 0.0),
                bar.overflow,
                "{axis:?}: the far end of the travel is still the full overflow"
            );
        }
    }

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

    /// PIN (P19/P120) — **the pool's two dirty questions, and the one answer
    /// that must not be the other.**
    ///
    /// The header's count badge asks "is anything I am *not* showing dirty",
    /// because the pane already wears its own dot; the three gates ask "what
    /// would be lost", which includes the buffer on screen. Folding them would
    /// light the badge for the file you are looking at (a fact already stated
    /// beside it) or, far worse, leave the file you are looking at out of the
    /// gate that is about to discard it.
    ///
    /// MUTATIONS:
    /// ① drop the `shown` filter — the badge assertion goes red;
    /// ② apply the filter unconditionally — the gate assertion goes red, and it
    ///    is the one that loses work.
    #[test]
    fn the_pool_answers_two_different_dirty_questions() {
        let mut pool = PreviewPool::default();
        for name in ["a.txt", "b.md", "c.rs"] {
            let path = PathBuf::from(format!(r"C:\w\{name}"));
            pool.open(path.clone(), name.to_owned(), &[]);
            pool.get_mut(&path).unwrap().dirty = name != "c.rs";
        }
        let shown = PathBuf::from(r"C:\w\a.txt");
        // The badge: everything dirty except the one on screen.
        assert_eq!(
            pool.dirty_names(Some(&shown)).collect::<Vec<_>>(),
            vec!["b.md"]
        );
        // The gates: everything dirty, in the pool's own order, by name.
        assert_eq!(
            pool.dirty_names(None).collect::<Vec<_>>(),
            vec!["a.txt", "b.md"]
        );
        // And a pool with nothing dirty asks nothing of anybody.
        pool.get_mut(&shown).unwrap().dirty = false;
        pool.get_mut(Path::new(r"C:\w\b.md")).unwrap().dirty = false;
        assert_eq!(pool.dirty_names(None).count(), 0);
        pool.clear();
        assert_eq!(pool.len(), 0);
    }

    /// **A shut discards the edits, not the history** (slice 7).
    ///
    /// The shut gate is the one gate whose pool has somewhere to go afterwards:
    /// the tab is about to be written to `session.json`, and its pool is the
    /// list of files next launch's switcher will show. `clear()` there answered
    /// one question by silently deciding a second — measured on the real
    /// machine, where a shut with one dirty buffer wrote `"pool": []` and a
    /// three-file history came back empty.
    ///
    /// MUTATION: put `clear()` back in `discard_dirty` and the survivors
    /// assertion goes red; drop the `retain` predicate's `!` and the gate can
    /// ask its question forever.
    #[test]
    fn a_discarded_edit_takes_its_own_buffer_and_leaves_the_history() {
        let mut pool = PreviewPool::default();
        for name in ["a.txt", "b.md", "c.rs"] {
            let path = PathBuf::from(format!(r"C:\w\{name}"));
            pool.open(path.clone(), name.to_owned(), &[]);
            pool.get_mut(&path).unwrap().dirty = name == "b.md";
        }
        pool.discard_dirty();
        assert_eq!(
            pool.buffers().map(|b| b.name.as_str()).collect::<Vec<_>>(),
            vec!["a.txt", "c.rs"],
            "the clean history survives, in its own order"
        );
        assert_eq!(
            pool.dirty_names(None).count(),
            0,
            "and the gate has nothing left to ask about, so it cannot re-raise"
        );
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
             ####### not a heading\n",
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
                MarkdownBlock::List {
                    ordered: None,
                    items: vec![vec![Span::plain("first")], vec![Span::plain("second")]],
                },
                MarkdownBlock::Heading {
                    level: 2,
                    spans: vec![Span::plain("Sub")],
                },
                MarkdownBlock::Code {
                    lang: Some("rust".to_owned()),
                    text: "let x = 1;".to_owned(),
                },
                // Seven hashes is not a heading in any dialect, and the ceiling
                // has to be *somewhere* or `#` would be a heading marker for a
                // line of nothing but hashes.
                MarkdownBlock::Paragraph(vec![Span::plain("####### not a heading")]),
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

    /// PIN (user report, 2026-08-13: "做得不太好") — **the five block kinds the
    /// prototype could not draw**, each classified from its own first line.
    ///
    /// `docs/DESIGN.md` is the file the report was made against and it uses
    /// every one of them. Asserted as classification rather than as pixels,
    /// because classification is where all five of them can go wrong: a table
    /// that is really two paragraphs of pipes, a `####` that is really a
    /// paragraph of hashes, a `>` that is really prose beginning with a chevron.
    ///
    /// MUTATIONS, one per member:
    /// ① drop the `1..=6` bound back to `1..=3` — the `####` row goes red;
    /// ② accept a pipe row without looking ahead for the separator — the last
    ///    assertion in this test (a pipe row that is *not* a table) goes red;
    /// ③ drop the `ordered` split in `flush_list` — the numbered list arrives as
    ///    a bulleted one;
    /// ④ let `is_thematic_break` run before the table branch — the separator row
    ///    is eaten as a rule and the table loses its heading;
    /// ⑤ drop `strip_quote` — the quote arrives as two paragraphs with chevrons.
    ///
    /// The quote's own shape changed with the paragraph ruling of 2026-08-13 and
    /// the expectation below moved with it: its two source lines are now one
    /// quoted paragraph.
    #[test]
    fn the_five_blocks_the_prototype_could_not_draw() {
        let doc = parse_markdown(
            "#### Fourth\n\
             ##### Fifth\n\
             ###### Sixth\n\
             1. one\n\
             2. two\n\
             - bullet\n\
             > quoted\n\
             > still quoted\n\
             ---\n\
             | a | `b` |\n\
             |---|:--:|\n\
             | 1 | **2** |\n\
             \n\
             not | a | table\n",
        );
        assert_eq!(
            doc,
            vec![
                MarkdownBlock::Heading {
                    level: 4,
                    spans: vec![Span::plain("Fourth")],
                },
                MarkdownBlock::Heading {
                    level: 5,
                    spans: vec![Span::plain("Fifth")],
                },
                MarkdownBlock::Heading {
                    level: 6,
                    spans: vec![Span::plain("Sixth")],
                },
                MarkdownBlock::List {
                    ordered: Some(1),
                    items: vec![vec![Span::plain("one")], vec![Span::plain("two")]],
                },
                // The bullet does not join the numbers: two lists.
                MarkdownBlock::List {
                    ordered: None,
                    items: vec![vec![Span::plain("bullet")]],
                },
                // Two source lines, one quoted paragraph — the same join prose
                // gets (user ruling, 2026-08-13). A bare `>` is what separates
                // two of them; see
                // `a_hard_wrapped_paragraph_is_one_paragraph`.
                MarkdownBlock::Quote(vec![vec![Span::plain("quoted still quoted")]]),
                MarkdownBlock::Rule,
                MarkdownBlock::Table {
                    rows: vec![
                        vec![vec![Span::plain("a")], vec![Span::code("b")]],
                        vec![vec![Span::plain("1")], vec![Span::bold("2")]],
                    ],
                },
                // **The pipe row with no separator under it stays prose.** This
                // is the assertion that keeps every sentence about `a | b` in a
                // shell out of a grid.
                MarkdownBlock::Paragraph(vec![Span::plain("not | a | table")]),
            ]
        );
        // A list that starts at three is a list that starts at three.
        assert_eq!(
            parse_markdown("3. third\n4. fourth"),
            vec![MarkdownBlock::List {
                ordered: Some(3),
                items: vec![vec![Span::plain("third")], vec![Span::plain("fourth")]],
            }]
        );
    }

    /// PIN — `[text](url)` renders its text and **never its url**, keeps that
    /// url to answer a press with, and the three inline passes keep their order.
    ///
    /// MUTATION ①: emit `text (url)` and the second assertion goes red — which
    /// is the one failure mode a link renderer must not have, because a printed
    /// URL is wrong under every ruling about what a click does.
    /// MUTATION ②: run the link pass before the code pass and the third
    /// assertion goes red: a bracket inside a code span stops being literal.
    /// MUTATION ③: drop the target on the floor again (`target: None`) and the
    /// first assertion goes red — the press would have nothing to resolve.
    #[test]
    fn a_link_renders_its_label_and_keeps_its_target_unprinted() {
        assert_eq!(
            parse_inline("see [the design](docs/DESIGN.md) first"),
            vec![
                Span::plain("see "),
                Span::link("the design", "docs/DESIGN.md"),
                Span::plain(" first"),
            ]
        );
        let rendered: String = parse_inline("[a](http://example.com/x)")
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(rendered, "a", "the target is not printed anywhere");
        // Backticks first: a bracket inside a code span is a bracket.
        assert_eq!(
            parse_inline("`[a](b)`"),
            vec![Span::code("[a](b)")],
            "the code pass runs before the link pass"
        );
        // Punctuation that only looks like a link stays punctuation.
        assert_eq!(
            parse_inline("a [TODO] note"),
            vec![Span::plain("a [TODO] note")]
        );
        assert_eq!(parse_inline("[unclosed"), vec![Span::plain("[unclosed")]);
    }

    /// PIN (user ruling, 2026-08-13) — **a link that names a file opens it in
    /// our own preview**; the web still goes to the browser; nothing else goes
    /// anywhere.
    ///
    /// The report was that pressing a file link opened the containing folder in
    /// Explorer. It could not have: nothing in this build read a preview link
    /// at all — [`push_link_runs`] threw the target away, so there was no click
    /// handler for one and could not be. What the user reached was the foot's
    /// Reveal, which opens a folder because that is Reveal's whole job. The
    /// ruling settles both halves: the link gets the tree's own answer (指到
    /// 文件=预览它), and 「open the containing folder」 stays with the foot.
    ///
    /// MUTATIONS:
    /// ① answer a file link with its parent directory — the first assertion
    ///    goes red, and that is the reported behaviour written down;
    /// ② resolve a relative target against the process's working directory
    ///    rather than the document's — the second goes red, and every link in
    ///    every document read from anywhere else points at nothing;
    /// ③ let any scheme through to the browser — the `mailto:` assertion goes
    ///    red and a document gets to name a handler;
    /// ④ stop folding the climb out (`directory.join(path)` raw) and the first
    ///    assertion goes red — which is the second half of the same report: the
    ///    file opened, but the foot printed
    ///    `…\preview-samples\../../docs/DESIGN.md` at the user and Explorer's
    ///    `/select` was handed it and opened the wrong folder.
    #[test]
    fn a_file_link_opens_in_the_preview_and_only_the_web_leaves_the_window() {
        let document = Path::new(r"D:\repo\test-assets\preview-samples\stress.md");
        let here = Path::new(r"D:\repo\test-assets\preview-samples");

        // ① A relative file link, resolved against the document's own folder —
        //    and the climb folded out, because this path is about to be printed
        //    in a caption and handed to another program.
        assert_eq!(
            link_action("../../docs/DESIGN.md", document),
            LinkAction::Preview(PathBuf::from(r"D:\repo\docs\DESIGN.md")),
            "a file link is a way of pointing at a file, and this window \
             previews files"
        );
        assert_eq!(
            link_action("./sample.csv", document),
            LinkAction::Preview(here.join("sample.csv")),
            "and a `.` is not part of anybody's idea of a path"
        );
        assert_eq!(
            link_action(r"..\sample.csv", document),
            LinkAction::Preview(PathBuf::from(r"D:\repo\test-assets\sample.csv")),
            "a backslash-written link is a link too"
        );
        assert_eq!(
            link_action("../../../../../../x.md", document),
            LinkAction::Preview(PathBuf::from(r"D:\x.md")),
            "and a climb past the root stops at the root, as Windows does"
        );

        // ② An absolute one stands as it is; a `file:` URL is unwrapped.
        assert_eq!(
            link_action(r"C:\notes\a.md", document),
            LinkAction::Preview(PathBuf::from(r"C:\notes\a.md")),
            "a drive letter is a path, not a scheme"
        );
        assert_eq!(
            link_action("file:///C:/notes/a%20b.md", document),
            LinkAction::Preview(PathBuf::from(r"C:\notes\a b.md")),
            "unwrapped, and its escapes undone"
        );

        // ③ The web leaves the window, and nothing else does.
        assert_eq!(
            link_action("https://example.com/x", document),
            LinkAction::Browse("https://example.com/x".to_owned())
        );
        for refused in [
            "mailto:someone@example.com",
            "ftp://example.com/x",
            "javascript:alert(1)",
        ] {
            assert_eq!(
                link_action(refused, document),
                LinkAction::Nowhere,
                "{refused}: a document does not get to name a handler"
            );
        }

        // ④ An anchor is cut off a path, and an anchor alone goes nowhere —
        //    there is no within-document navigation to do yet.
        assert_eq!(
            link_action("DESIGN.md#7.1.2", document),
            LinkAction::Preview(here.join("DESIGN.md"))
        );
        assert_eq!(link_action("#section", document), LinkAction::Nowhere);
        assert_eq!(link_action("   ", document), LinkAction::Nowhere);
    }

    /// PIN (user ruling, 2026-08-13) — **a hard-wrapped source paragraph is one
    /// paragraph**, the way CommonMark says and every markdown reader draws it.
    ///
    /// The prototype made one block per *source line* (`renderMarkdownMock`,
    /// 4914-4941), and against a real document that is the seam the report was
    /// about: `docs/DESIGN.md` is written wrapped at eighty columns, so every
    /// paragraph in it came out as five separately-wrapped blocks with a
    /// paragraph gap between each pair — a page of prose printed as a page of
    /// stanzas, with the last word of each source line stranded on a line of its
    /// own whenever the pane was wide.
    ///
    /// Joining is done on the **source text, before the inline pass**, which is
    /// also what CommonMark does and what makes the second assertion here
    /// possible: emphasis opened on one source line and closed on the next is one
    /// bold run, where a per-line parser produced two literal asterisk pairs.
    ///
    /// MUTATIONS:
    /// ① go back to one block per line (`blocks.push(Paragraph(parse_inline(
    ///    line)))` in the fall-through) — the first assertion sees three
    ///    paragraphs instead of one;
    /// ② join across the blank line as well (drop the blank-line flush) — the
    ///    first assertion sees one paragraph where there must be two;
    /// ③ let a heading, a fence or a list marker be swallowed as continuation
    ///    text — the third assertion loses its block boundaries.
    #[test]
    fn a_hard_wrapped_paragraph_is_one_paragraph() {
        assert_eq!(
            parse_markdown(
                "The rule is that consecutive non-blank lines\n\
                 are one paragraph, and a blank line ends it.\n\
                 This is the third source line.\n\
                 \n\
                 A second paragraph.\n",
            ),
            vec![
                MarkdownBlock::Paragraph(vec![Span::plain(
                    "The rule is that consecutive non-blank lines \
                     are one paragraph, and a blank line ends it. \
                     This is the third source line."
                )]),
                MarkdownBlock::Paragraph(vec![Span::plain("A second paragraph.")]),
            ]
        );
        // Inline runs are parsed over the joined paragraph, so emphasis may
        // straddle a source break.
        assert_eq!(
            parse_markdown("a **bold phrase\nspanning the fold** end\n"),
            vec![MarkdownBlock::Paragraph(vec![
                Span::plain("a "),
                Span::bold("bold phrase spanning the fold"),
                Span::plain(" end"),
            ])]
        );
        // Every other block still interrupts prose on its own first line.
        assert_eq!(
            parse_markdown(
                "prose\n\
                 # heading\n\
                 prose\n\
                 - item\n\
                 prose again\n\
                 \n\
                 prose\n\
                 > quoted\n\
                 prose\n\
                 ---\n\
                 prose\n\
                 ```\n\
                 fenced\n\
                 ```\n",
            ),
            vec![
                MarkdownBlock::Paragraph(vec![Span::plain("prose")]),
                MarkdownBlock::Heading {
                    level: 1,
                    spans: vec![Span::plain("heading")],
                },
                MarkdownBlock::Paragraph(vec![Span::plain("prose")]),
                // **The continuation line joins the item**, which is the other
                // half of the ruling: a wrapped bullet is one bullet.
                MarkdownBlock::List {
                    ordered: None,
                    items: vec![vec![Span::plain("item prose again")]],
                },
                MarkdownBlock::Paragraph(vec![Span::plain("prose")]),
                MarkdownBlock::Quote(vec![vec![Span::plain("quoted")]]),
                MarkdownBlock::Paragraph(vec![Span::plain("prose")]),
                MarkdownBlock::Rule,
                MarkdownBlock::Paragraph(vec![Span::plain("prose")]),
                MarkdownBlock::Code {
                    lang: None,
                    text: "fenced".to_owned(),
                },
            ]
        );
        // A quote wrapped in the source is one quoted paragraph, for the same
        // reason and by the same join; a bare `>` separates two of them.
        assert_eq!(
            parse_markdown("> one line\n> and its fold\n>\n> a second\n"),
            vec![MarkdownBlock::Quote(vec![
                vec![Span::plain("one line and its fold")],
                vec![Span::plain("a second")],
            ])]
        );
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

    /// PIN (user ruling, 2026-08-15) — **the conflict phrase still says all
    /// three things it was written to say.**
    ///
    /// The ruling moved every standing notice out of the body and onto the right
    /// hand of the path foot, which meant a sentence had to become something
    /// that fits beside a path. That is exactly the edit that quietly loses a
    /// clause, and the clause at risk is the third one: a user who reads only
    /// "Not saved" believes their work is gone. It is not — the buffer survives
    /// the refusal intact, and saying so is the whole reason this string is not
    /// two words.
    ///
    /// Mutation: shorten it to "Not saved · changed on disk" — a perfectly
    /// reasonable-looking abbreviation — and the third assertion goes red.
    #[test]
    fn the_conflict_phrase_says_what_happened_what_was_not_done_and_what_survived() {
        let phrase = PREVIEW_CONFLICT_NOTICE;
        assert!(phrase.contains("Not saved"), "what was not done: {phrase}");
        assert!(
            phrase.contains("changed on disk"),
            "what happened: {phrase}"
        );
        assert!(
            phrase.contains("edits kept"),
            "and what is still true — the clause a shortening drops first: {phrase}"
        );
        // Short enough to share a 28px strip with a path, which is the whole
        // reason it stopped being a sentence.
        assert!(
            phrase.chars().count() < PREVIEW_TRUNCATED_NOTICE.chars().count() * 3,
            "and it is a phrase, not a paragraph: {phrase}"
        );
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
        assert!(
            !buffer.is_editable(false),
            "a buffer with no body has no caret"
        );
        buffer.accept(read("fn main() {}\n", false));
        assert!(buffer.is_editable(false));
        buffer.accept(read("fn main() {}\n", true));
        assert!(!buffer.is_editable(false));
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
