//! The quick-edit surface's text model: where the caret is, what is selected,
//! and what one keystroke does to a body.
//!
//! **Quick edit, not an IDE** — the mock-up says so in as many words at 4980
//! ("plain textarea, explicit save"), and every absence here is that sentence
//! rather than a gap: no second caret, no language server, no folding, no
//! syntax. What is left is the vocabulary a `<textarea>` has, which is the
//! vocabulary this module implements.
//!
//! **Everything here is a pure function of a `String` and an offset.** The
//! runtime owns the buffer, the clipboard and the disk; this owns only the
//! arithmetic, which is what lets the whole of "what does Backspace at the
//! start of a line do to a CRLF file" be answered in a test with no window.
//!
//! Three rules run through all of it and are worth stating once:
//!
//! * **A caret is a byte offset**, always on a character boundary and never
//!   between a `\r` and the `\n` it belongs to. [`normalize`] is the one place
//!   that is enforced, and every entry point goes through it.
//! * **A column is what is drawn, not what is stored.** `tab-size: 4` (mock-up
//!   603) means a tab is a *stop*, so the caret's horizontal position — the
//!   thing Up and Down have to preserve — is measured in the cells the line
//!   draws as, exactly as [`crate::preview::expand_tabs`] draws them.
//! * **A line break is the file's own.** A file written with CRLF stays a CRLF
//!   file when Enter is pressed in it ([`eol_of`]); a preview that silently
//!   re-lined a file it was only asked to fix a typo in would be a preview that
//!   rewrites every line of the next diff.

use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::preview::{PREVIEW_TEXT_TAB_WIDTH, expand_tabs};

// ── the line model ──────────────────────────────────────────────────────────

/// Where every line of a body starts, in bytes.
///
/// Always at least one entry, because a body always has at least one line — an
/// empty file is one empty line, and a body ending in a break has an empty line
/// *after* it. That last one is the difference between this and [`str::lines`],
/// and it is not a detail: the caret has to be able to stand on the line a file
/// ends with, and `"a\n".lines()` claims that line does not exist.
pub fn line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (index, byte) in content.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(index + 1);
        }
    }
    starts
}

/// Which line an offset is on.
pub fn line_index(starts: &[usize], offset: usize) -> usize {
    starts.partition_point(|start| *start <= offset).max(1) - 1
}

/// One line's text bounds, the break excluded.
///
/// The break is excluded on both halves: a caret at "the end of the line" is
/// before the `\r` of a CRLF, not between the two bytes, which is the whole of
/// why [`normalize`] exists.
pub fn line_bounds(content: &str, starts: &[usize], line: usize) -> (usize, usize) {
    let start = starts.get(line).copied().unwrap_or(content.len());
    let mut end = starts.get(line + 1).map_or(content.len(), |next| next - 1);
    if end > start && content.as_bytes()[end - 1] == b'\r' {
        end -= 1;
    }
    (start, end)
}

/// One line's text, without whatever ends it.
pub fn line_text<'a>(content: &'a str, starts: &[usize], line: usize) -> &'a str {
    let (start, end) = line_bounds(content, starts, line);
    &content[start..end]
}

/// Every line of a body as it is drawn — tabs already the spaces they stand in
/// for.
///
/// The painter's view of the same lines [`line_starts`] indexes, so the row a
/// click lands on and the row a caret is drawn on cannot come apart.
pub fn display_lines(content: &str) -> Vec<String> {
    let starts = line_starts(content);
    (0..starts.len())
        .map(|line| expand_tabs(line_text(content, &starts, line)))
        .collect()
}

/// How far into a line, in drawn columns, a byte offset sits.
pub fn column_of(line: &str, byte: usize) -> usize {
    let mut column = 0usize;
    let mut at = 0usize;
    for cluster in bt_unicode::graphemes(line) {
        if at >= byte {
            break;
        }
        column += cluster_columns(cluster, column);
        at += cluster.len();
    }
    column
}

/// The byte offset a drawn column names.
///
/// Rounds *forward* out of a cluster it lands inside: a click halfway through a
/// tab is a click after the tab, because the alternative is a caret drawn four
/// cells away from where the pointer was.
pub fn byte_at_column(line: &str, column: usize) -> usize {
    let mut at = 0usize;
    let mut drawn = 0usize;
    for cluster in bt_unicode::graphemes(line) {
        if drawn >= column {
            return at;
        }
        drawn += cluster_columns(cluster, drawn);
        at += cluster.len();
    }
    line.len()
}

/// How wide one cluster draws, standing at `column`.
fn cluster_columns(cluster: &str, column: usize) -> usize {
    if cluster == "\t" {
        PREVIEW_TEXT_TAB_WIDTH - column % PREVIEW_TEXT_TAB_WIDTH
    } else {
        bt_unicode::cluster_width(cluster)
    }
}

/// How wide a line draws, in the columns the caret is measured in.
pub fn line_columns(line: &str) -> usize {
    column_of(line, line.len())
}

// ── the soft wrap (user ruling, 2026-08-13) ─────────────────────────────────
//
// **The text surface wraps; the diff does not.** The mock-up's `.pv-edit` is
// `white-space: pre` (609) and this window kept that faithfully for a slice,
// which is how the report arrived: a preview is a *quick look*, and content that
// runs off the right edge of a quick look reads as a file that has been cut
// short, not as a file that has somewhere else to be. A horizontal scrollbar is
// a hidden gesture, and a hidden gesture is not an answer to "is this all of
// it". Indentation survives because it is at the *start* of a line, which is the
// one place wrapping never moves anything.
//
// The diff keeps `pre`, and that is not an inconsistency: a patch's alignment
// between its two columns and the full-width tint under each row are what a
// patch *means*, and both are destroyed by reflow. So is a `.csv`'s grid, which
// has never wrapped either.
//
// Continuation lines start at the left margin with no hanging indent — the
// simplest rule that is never wrong, chosen deliberately over guessing at a
// list's or a comment's intended overhang.

/// Where one display line's soft breaks fall, in **drawn columns**.
///
/// Columns rather than bytes throughout, because columns are the coordinate the
/// caret, the click and the painter already share on a monospace surface: a
/// break recorded in bytes would have to be converted three times, and each
/// conversion is a place for a wide character to be counted once.
///
/// Greedy, breaking after the last space that fits and hard-breaking a word too
/// long to fit at all — which is what a `<textarea>` with `wrap="soft"` does, and
/// the reason a 400-character path is fully visible rather than fully hidden.
fn wrap_columns(line: &str, width: usize) -> Vec<usize> {
    let width = width.max(1);
    let mut breaks = Vec::new();
    let mut row_start = 0usize;
    let mut column = 0usize;
    // The column just after the most recent space on this row: where a break
    // would leave the space at the end of the row it belongs to.
    let mut after_space: Option<usize> = None;
    for cluster in bt_unicode::graphemes(line) {
        let cells = bt_unicode::cluster_width(cluster);
        if column > row_start && column + cells > row_start + width {
            let at = after_space
                .filter(|at| *at > row_start && *at <= column)
                .unwrap_or(column);
            breaks.push(at);
            row_start = at;
            after_space = None;
        }
        column += cells;
        if cluster == " " {
            after_space = Some(column);
        }
    }
    breaks
}

/// Every display line's soft breaks, and the visual rows they add up to.
///
/// **The one authority for "which row is this and which line is that row on".**
/// The painter, the hit test, the caret's drawn position and Up/Down all ask it
/// the same question in the same coordinate, so none of them can be off by a row
/// from any other.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WrapLayout {
    /// How wide a row is, in columns. `None` for a surface that does not wrap,
    /// where every logical line is exactly one visual row.
    width: Option<usize>,
    /// Per line, the starting column of each row after its first.
    breaks: Vec<Vec<usize>>,
    /// Per line, the index of its first visual row. One longer than `breaks`,
    /// so the last entry is the total number of rows.
    firsts: Vec<usize>,
    /// Per line, the columns it draws in — the far end of its last row.
    widths: Vec<usize>,
}

impl WrapLayout {
    /// A surface that does not wrap: one row per line, whatever their length.
    pub fn unwrapped(lines: &[String]) -> Self {
        Self::build(lines, None)
    }

    /// A surface `width` columns across.
    pub fn wrapped(lines: &[String], width: usize) -> Self {
        Self::build(lines, Some(width.max(1)))
    }

    fn build(lines: &[String], width: Option<usize>) -> Self {
        let mut breaks = Vec::with_capacity(lines.len());
        let mut firsts = Vec::with_capacity(lines.len() + 1);
        let mut widths = Vec::with_capacity(lines.len());
        let mut row = 0usize;
        for line in lines {
            firsts.push(row);
            let line_breaks = match width {
                Some(width) => wrap_columns(line, width),
                None => Vec::new(),
            };
            row += line_breaks.len() + 1;
            breaks.push(line_breaks);
            widths.push(line_columns(line));
        }
        firsts.push(row);
        Self {
            width,
            breaks,
            firsts,
            widths,
        }
    }

    /// Whether this surface reflows at all.
    pub fn wraps(&self) -> bool {
        self.width.is_some()
    }

    /// How many visual rows the whole body draws as.
    pub fn rows(&self) -> usize {
        self.firsts.last().copied().unwrap_or(0)
    }

    /// The visual row a line's column stands on, and the column that row starts
    /// at — everything a caret or a band needs to move from the line model to
    /// the drawn one.
    pub fn row_of(&self, line: usize, column: usize) -> (usize, usize) {
        let Some(breaks) = self.breaks.get(line) else {
            return (self.rows().saturating_sub(1), 0);
        };
        // The last break at or before the column: a caret sitting exactly on a
        // break belongs to the row that break opens, which is where the next
        // character it types will appear.
        let index = breaks.partition_point(|start| *start <= column);
        let start = if index == 0 { 0 } else { breaks[index - 1] };
        (self.firsts[line] + index, start)
    }

    /// What one visual row holds: its line, and the half-open column range of
    /// that line it draws.
    ///
    /// The far end of a line's **last** row runs one column past its own text,
    /// because that column is where the line break is — a selection that covers
    /// the break has to have somewhere to draw it.
    pub fn row_span(&self, row: usize) -> Option<(usize, usize, usize)> {
        let line = self.line_of_row(row)?;
        let breaks = &self.breaks[line];
        let index = row - self.firsts[line];
        let from = if index == 0 { 0 } else { breaks[index - 1] };
        let to = breaks
            .get(index)
            .copied()
            .unwrap_or_else(|| self.widths[line] + 1);
        Some((line, from, to))
    }

    /// Which line a visual row belongs to.
    pub fn line_of_row(&self, row: usize) -> Option<usize> {
        (row < self.rows()).then(|| self.firsts.partition_point(|first| *first <= row) - 1)
    }
}

/// The break this file uses.
///
/// Decided from the first one in the body rather than by counting, because a
/// file's first break is the one its author chose and a file with mixed breaks
/// has no majority worth honouring — only a first.
pub fn eol_of(content: &str) -> &'static str {
    match content.find('\n') {
        Some(0) => "\n",
        Some(index) if content.as_bytes()[index - 1] == b'\r' => "\r\n",
        Some(_) => "\n",
        None => "\n",
    }
}

/// The same text, with every line break rewritten as this file's.
///
/// A clipboard on this platform carries CRLF whatever it was copied from, so a
/// paste that went in verbatim would leave a bare-newline file with one CRLF
/// line in the middle of it — a one-line paste that turns the next diff into a
/// whole-file rewrite.
pub fn with_eol(text: &str, eol: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(['\r', '\n']) {
        out.push_str(&rest[..at]);
        out.push_str(eol);
        rest = if rest[at..].starts_with("\r\n") {
            &rest[at + 2..]
        } else {
            &rest[at + 1..]
        };
    }
    out.push_str(rest);
    out
}

/// Put an offset somewhere a caret may actually stand.
pub fn normalize(content: &str, offset: usize) -> usize {
    let mut offset = offset.min(content.len());
    while offset > 0 && !content.is_char_boundary(offset) {
        offset -= 1;
    }
    // Never between the two halves of a break: `\r\n` is one line ending, and a
    // caret inside it is a caret in the middle of a character as far as every
    // question below is concerned.
    if offset > 0
        && offset < content.len()
        && content.as_bytes()[offset - 1] == b'\r'
        && content.as_bytes()[offset] == b'\n'
    {
        offset -= 1;
    }
    offset
}

// ── the caret ───────────────────────────────────────────────────────────────

/// Where the caret is, what it has dragged over, and the column it is trying to
/// keep.
///
/// **A view's property, not a buffer's** (ruling 8⑧, 2026-08-12). Two panes on
/// one buffer are one buffer with two carets, and the buffer is the one thing
/// they must agree about — so this lives beside the scroll offset, in the view
/// layer, and is remembered per path when a pane switches away.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EditCaret {
    /// Where the selection began. Equal to [`Self::caret`] when nothing is
    /// selected, which is what makes "extend" and "move" one operation.
    pub anchor: usize,
    pub caret: usize,
    /// The column Up and Down are trying to land on.
    ///
    /// `None` until a vertical move needs one. It survives a run of them and
    /// nothing else, which is why walking down through a short line and out the
    /// other side comes back to the column you started in — the behaviour every
    /// editor has and none of them writes down.
    pub desired_column: Option<usize>,
}

impl EditCaret {
    /// The selected span, in order.
    pub fn range(&self) -> std::ops::Range<usize> {
        if self.anchor <= self.caret {
            self.anchor..self.caret
        } else {
            self.caret..self.anchor
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.caret
    }

    fn collapse_to(&mut self, offset: usize) {
        self.anchor = offset;
        self.caret = offset;
        self.desired_column = None;
    }

    /// Put the caret somewhere, taking the anchor with it unless the gesture is
    /// an extension.
    pub fn place(&mut self, content: &str, offset: usize, extend: bool) {
        let offset = normalize(content, offset);
        self.caret = offset;
        if !extend {
            self.anchor = offset;
        }
        self.desired_column = None;
    }

    /// Everything the caret has dragged over.
    pub fn selected<'a>(&self, content: &'a str) -> &'a str {
        let range = self.range();
        &content[range.start.min(content.len())..range.end.min(content.len())]
    }

    /// Put both ends back inside a body that may have changed under them.
    pub fn heal(&mut self, content: &str) {
        self.anchor = normalize(content, self.anchor);
        self.caret = normalize(content, self.caret);
    }
}

// ── the verbs ───────────────────────────────────────────────────────────────

/// One direction a caret travels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    LineEnd,
    PageUp,
    PageDown,
    DocStart,
    DocEnd,
}

/// What one keystroke means to the edit surface.
///
/// Every key that reaches the surface has one, including the ones with nothing
/// to do: **the editor owns the keyboard outright while it is focused** (P139,
/// "editor keys are the editor's — the terminal must not hear them"), so a key
/// this list has no verb for is [`Self::Ignore`] and is swallowed, not passed
/// down to a shell the user is not looking at.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditCommand {
    Insert(String),
    /// Enter — the file's own break, not a chosen one.
    Newline,
    /// A literal tab. `tab-size: 4` is how it is *drawn*; what is stored is one
    /// byte, because a preview that expanded tabs on the way in would rewrite
    /// the indentation of every file it was opened in.
    Tab,
    Backspace,
    Delete,
    Move {
        motion: Motion,
        extend: bool,
    },
    SelectAll,
    Copy,
    Cut,
    Paste,
    Save,
    /// Esc — the keyboard goes back to the terminal.
    Release,
    /// The editor's key, with nothing for it to do.
    Ignore,
}

/// What one key means with the edit surface focused.
///
/// [`crate::files::tree_command`]'s twin, and total for the reason that one is
/// partial: a files column has nothing to type into, so a letter arriving there
/// means nothing; an editor is exactly a thing to type into, so every key it
/// receives is its own.
pub fn command(key: &Key, modifiers: ModifiersState) -> EditCommand {
    let ctrl = modifiers.control_key();
    let alt = modifiers.alt_key();
    let shift = modifiers.shift_key();
    // AltGr arrives as Ctrl+Alt and is *typing* — the one chord shape where a
    // Ctrl means nothing at all. This is the same exemption the PTY encoder
    // already makes, applied to the one other surface that takes characters.
    let altgr = ctrl && alt;
    if ctrl && !alt {
        if let Key::Character(text) = key {
            return match text.to_ascii_lowercase().as_str() {
                "s" => EditCommand::Save,
                "c" => EditCommand::Copy,
                "x" => EditCommand::Cut,
                "v" => EditCommand::Paste,
                "a" => EditCommand::SelectAll,
                _ => EditCommand::Ignore,
            };
        }
        return match key {
            Key::Named(NamedKey::Home) => EditCommand::Move {
                motion: Motion::DocStart,
                extend: shift,
            },
            Key::Named(NamedKey::End) => EditCommand::Move {
                motion: Motion::DocEnd,
                extend: shift,
            },
            _ => EditCommand::Ignore,
        };
    }
    if let Key::Named(named) = key {
        let motion = match named {
            NamedKey::ArrowLeft => Some(Motion::Left),
            NamedKey::ArrowRight => Some(Motion::Right),
            NamedKey::ArrowUp => Some(Motion::Up),
            NamedKey::ArrowDown => Some(Motion::Down),
            NamedKey::Home => Some(Motion::LineStart),
            NamedKey::End => Some(Motion::LineEnd),
            NamedKey::PageUp => Some(Motion::PageUp),
            NamedKey::PageDown => Some(Motion::PageDown),
            _ => None,
        };
        if let Some(motion) = motion {
            return EditCommand::Move {
                motion,
                extend: shift,
            };
        }
        return match named {
            NamedKey::Enter => EditCommand::Newline,
            NamedKey::Tab => EditCommand::Tab,
            NamedKey::Backspace => EditCommand::Backspace,
            NamedKey::Delete => EditCommand::Delete,
            NamedKey::Space => EditCommand::Insert(" ".to_owned()),
            NamedKey::Escape => EditCommand::Release,
            _ => EditCommand::Ignore,
        };
    }
    match key {
        // A bare character, or one wearing AltGr. Alt alone is a menu
        // accelerator on this platform and types nothing.
        Key::Character(text) if !alt || altgr => EditCommand::Insert(text.to_string()),
        _ => EditCommand::Ignore,
    }
}

// ── the edits ───────────────────────────────────────────────────────────────

/// Put text where the caret is, replacing whatever it had dragged over.
///
/// Reports whether the body actually changed, which is what the dirty bit is
/// written from: an insert of nothing over nothing is not an edit.
pub fn insert(content: &mut String, caret: &mut EditCaret, text: &str) -> bool {
    caret.heal(content);
    let range = caret.range();
    if range.is_empty() && text.is_empty() {
        return false;
    }
    let at = range.start;
    content.replace_range(range, text);
    caret.collapse_to(at + text.len());
    true
}

/// Delete backwards: the selection if there is one, otherwise one cluster —
/// **or one whole line break**, `\r\n` included, because half a break is not a
/// character and leaving the `\r` behind is how a file grows invisible bytes.
pub fn backspace(content: &mut String, caret: &mut EditCaret) -> bool {
    caret.heal(content);
    if !caret.is_empty() {
        return insert(content, caret, "");
    }
    if caret.caret == 0 {
        return false;
    }
    let previous = previous_boundary(content, caret.caret);
    content.replace_range(previous..caret.caret, "");
    caret.collapse_to(previous);
    true
}

/// Delete forwards, on the same terms.
pub fn delete_forward(content: &mut String, caret: &mut EditCaret) -> bool {
    caret.heal(content);
    if !caret.is_empty() {
        return insert(content, caret, "");
    }
    if caret.caret >= content.len() {
        return false;
    }
    let next = next_boundary(content, caret.caret);
    let at = caret.caret;
    content.replace_range(at..next, "");
    caret.collapse_to(at);
    true
}

/// Everything, selected.
pub fn select_all(content: &str, caret: &mut EditCaret) {
    caret.anchor = 0;
    caret.caret = content.len();
    caret.desired_column = None;
}

/// The offset one cluster before `offset`, breaks counted whole.
pub fn previous_boundary(content: &str, offset: usize) -> usize {
    let bytes = content.as_bytes();
    if offset >= 2 && bytes[offset - 1] == b'\n' && bytes[offset - 2] == b'\r' {
        return offset - 2;
    }
    if offset >= 1 && (bytes[offset - 1] == b'\n' || bytes[offset - 1] == b'\r') {
        return offset - 1;
    }
    let line_start = content[..offset].rfind('\n').map_or(0, |at| at + 1);
    let mut boundary = line_start;
    let mut at = line_start;
    for cluster in bt_unicode::graphemes(&content[line_start..offset]) {
        boundary = at;
        at += cluster.len();
    }
    boundary
}

/// The offset one cluster after `offset`.
pub fn next_boundary(content: &str, offset: usize) -> usize {
    let rest = &content[offset..];
    if rest.starts_with("\r\n") {
        return offset + 2;
    }
    if rest.starts_with('\n') || rest.starts_with('\r') {
        return offset + 1;
    }
    match bt_unicode::graphemes(rest).next() {
        Some(cluster) => offset + cluster.len(),
        None => offset,
    }
}

/// Move the caret, dragging the anchor with it unless the key wore Shift.
///
/// `page_rows` is how many lines the body can show, which only the geometry
/// knows — a page is a *screenful*, so a taller pane pages further.
pub fn move_caret(
    content: &str,
    caret: &mut EditCaret,
    motion: Motion,
    extend: bool,
    page_rows: usize,
) {
    caret.heal(content);
    let starts = line_starts(content);
    let line = line_index(&starts, caret.caret);
    let vertical = matches!(
        motion,
        Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown
    );
    let target = match motion {
        // A left with a selection collapses to its near end rather than moving
        // — the one asymmetry every text field has, and the reason arrowing out
        // of a selection does not eat a character of it.
        Motion::Left if !extend && !caret.is_empty() => caret.range().start,
        Motion::Right if !extend && !caret.is_empty() => caret.range().end,
        Motion::Left => previous_boundary(content, caret.caret),
        Motion::Right => next_boundary(content, caret.caret),
        Motion::LineStart => line_bounds(content, &starts, line).0,
        Motion::LineEnd => line_bounds(content, &starts, line).1,
        Motion::DocStart => 0,
        Motion::DocEnd => content.len(),
        Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown => {
            let step = match motion {
                Motion::Up | Motion::Down => 1isize,
                _ => page_rows.max(1) as isize,
            };
            let step = if matches!(motion, Motion::Up | Motion::PageUp) {
                -step
            } else {
                step
            };
            let (start, _) = line_bounds(content, &starts, line);
            let column = caret.desired_column.unwrap_or_else(|| {
                column_of(line_text(content, &starts, line), caret.caret - start)
            });
            let wanted = line as isize + step;
            if wanted < 0 {
                // Off the top is the top, which is what a textarea does — and
                // the desired column is kept, so coming back down returns to it.
                let offset = 0;
                finish_move(content, caret, offset, extend, Some(column));
                return;
            }
            let wanted = wanted as usize;
            if wanted >= starts.len() {
                finish_move(content, caret, content.len(), extend, Some(column));
                return;
            }
            let (start, _) = line_bounds(content, &starts, wanted);
            let text = line_text(content, &starts, wanted);
            let offset = start + byte_at_column(text, column);
            finish_move(content, caret, offset, extend, Some(column));
            return;
        }
    };
    finish_move(content, caret, target, extend, vertical.then_some(0));
}

fn finish_move(
    content: &str,
    caret: &mut EditCaret,
    offset: usize,
    extend: bool,
    desired: Option<usize>,
) {
    let offset = normalize(content, offset);
    caret.caret = offset;
    if !extend {
        caret.anchor = offset;
    }
    caret.desired_column = desired;
}

/// The offset a row and a drawn column name — the click's own question.
pub fn offset_at(content: &str, line: usize, column: usize) -> usize {
    let starts = line_starts(content);
    let line = line.min(starts.len() - 1);
    let (start, _) = line_bounds(content, &starts, line);
    start + byte_at_column(line_text(content, &starts, line), column)
}

/// Which columns of one line a selection covers, if any.
///
/// The far end of a line that is *inside* the selection runs one column past
/// its own text: the break is selected too, and a band that stopped at the last
/// character would say a multi-line selection was several unrelated ones.
pub fn selected_columns(
    content: &str,
    starts: &[usize],
    line: usize,
    range: &std::ops::Range<usize>,
) -> Option<(usize, usize)> {
    if range.is_empty() {
        return None;
    }
    let (start, end) = line_bounds(content, starts, line);
    let break_end = starts.get(line + 1).map_or(content.len(), |next| *next);
    if range.end <= start || range.start > break_end {
        return None;
    }
    if range.start == break_end && break_end != end {
        return None;
    }
    let text = line_text(content, starts, line);
    let from = column_of(text, range.start.saturating_sub(start).min(text.len()));
    let to = if range.end > end {
        line_columns(text) + 1
    } else {
        column_of(text, range.end - start)
    };
    (to > from).then_some((from, to))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A caret with nothing selected, at one offset.
    fn at(offset: usize) -> EditCaret {
        EditCaret {
            anchor: offset,
            caret: offset,
            desired_column: None,
        }
    }

    /// PIN (user ruling, 2026-08-13) — **a line too wide for the pane is folded,
    /// not cropped**, and the fold is answerable in both directions.
    ///
    /// The reported symptom was a `.txt` whose four-hundred-character lines ran
    /// off the right edge of a narrow preview: everything past the edge needed a
    /// horizontal scroll to reach, and a quick look with hidden content reads as
    /// a file that was truncated. What is pinned here is the arithmetic under
    /// the fix, in the coordinate everything else on this surface uses:
    ///
    /// ① a line's rows grow as the pane narrows, and every column of the line
    ///    lands on exactly one of them — nothing is dropped and nothing doubled;
    /// ② the break prefers a space, so a fold falls between words when it can;
    /// ③ a word longer than the pane is broken anyway, because the alternative
    ///    is a row that overflows and the whole point was to stop overflowing;
    /// ④ `row_of` and `row_span` are inverses, which is what makes a caret drawn
    ///    at a click land back on the byte that was clicked.
    ///
    /// MUTATION ①: drop the `after_space` branch and take `column` always — the
    /// word-boundary assertion goes red and every fold lands mid-word.
    /// MUTATION ②: return `Vec::new()` from `wrap_columns` — the row counts
    /// collapse to one per line, which is the pre-ruling `white-space: pre`.
    #[test]
    fn a_line_wider_than_the_pane_folds_at_a_word_and_keeps_every_column() {
        let lines = vec!["the quick brown fox jumps".to_owned()];
        let wide = WrapLayout::wrapped(&lines, 25);
        assert_eq!(wide.rows(), 1, "a line that fits is one row");

        // Twelve and not ten: at ten the word boundaries fall exactly on the
        // hard edge, so a renderer that ignored words entirely would pass.
        let narrow = WrapLayout::wrapped(&lines, 12);
        assert_eq!(narrow.rows(), 3, "and folds as the pane closes on it");
        let spans: Vec<(usize, usize, usize)> = (0..narrow.rows())
            .map(|row| narrow.row_span(row).expect("every row has a span"))
            .collect();
        // ② the folds fall after "quick " and after "brown ", not mid-word.
        let text = &lines[0];
        let piece = |from: usize, to: usize| {
            text[byte_at_column(text, from)..byte_at_column(text, to).min(text.len())].to_owned()
        };
        assert_eq!(piece(spans[0].1, spans[0].2), "the quick ");
        assert_eq!(piece(spans[1].1, spans[1].2), "brown fox ");
        assert_eq!(piece(spans[2].1, spans[2].2), "jumps");
        // ① every row is on the one line and the rows tile it end to end.
        assert!(spans.iter().all(|(line, ..)| *line == 0));
        assert_eq!(spans[0].1, 0);
        for pair in spans.windows(2) {
            assert_eq!(
                pair[0].2, pair[1].1,
                "the rows tile with no gap and no overlap"
            );
        }

        // ③ a word with nowhere to break is broken.
        let long = vec!["antidisestablishmentarianism".to_owned()];
        let folded = WrapLayout::wrapped(&long, 10);
        assert_eq!(folded.rows(), 3);
        assert_eq!(folded.row_span(1), Some((0, 10, 20)));

        // ④ the two directions are inverses on every column of every row.
        for row in 0..narrow.rows() {
            let (line, from, to) = narrow.row_span(row).unwrap();
            for column in from..to.min(line_columns(&lines[line])) {
                assert_eq!(
                    narrow.row_of(line, column),
                    (row, from),
                    "column {column} belongs to row {row}"
                );
            }
        }

        // And the surface that does not wrap is one row per line, always.
        let plain = WrapLayout::unwrapped(&lines);
        assert!(!plain.wraps());
        assert_eq!(plain.rows(), 1);
        assert_eq!(plain.row_span(0), Some((0, 0, line_columns(&lines[0]) + 1)));
    }

    /// PIN — a wide character occupies the two cells it draws in, on both sides
    /// of the fold.
    ///
    /// Mutation: count clusters instead of columns in `wrap_columns` and a row
    /// of CJK holds twice the cells the pane has.
    #[test]
    fn a_fold_counts_the_cells_a_character_draws_in_not_the_characters() {
        // Ten CJK characters: twenty cells, so a ten-column pane holds five of
        // them per row.
        let lines = vec!["一二三四五六七八九十".to_owned()];
        let wrapped = WrapLayout::wrapped(&lines, 10);
        assert_eq!(wrapped.rows(), 2);
        assert_eq!(wrapped.row_span(0), Some((0, 0, 10)));
        assert_eq!(wrapped.row_span(1), Some((0, 10, 21)));
    }

    /// PIN — a body always has the line a caret can stand on, including the
    /// empty one a trailing break makes.
    ///
    /// Mutation: build the lines with [`str::lines`], which drops it.
    #[test]
    fn a_body_ending_in_a_break_has_a_line_after_it() {
        assert_eq!(line_starts(""), vec![0]);
        assert_eq!(line_starts("a"), vec![0]);
        assert_eq!(line_starts("a\n"), vec![0, 2]);
        assert_eq!(line_starts("a\nb\n"), vec![0, 2, 4]);
        assert_eq!(display_lines("a\n"), vec!["a".to_owned(), String::new()]);
        // A CRLF file's lines are its text, not its text plus a stray return.
        let content = "a\r\nb\r\n";
        let starts = line_starts(content);
        assert_eq!(line_text(content, &starts, 0), "a");
        assert_eq!(line_text(content, &starts, 1), "b");
        assert_eq!(line_text(content, &starts, 2), "");
    }

    /// PIN — a column is what the line *draws*, tab stops included.
    ///
    /// Mutation: count characters instead of columns in [`column_of`].
    #[test]
    fn a_column_is_measured_in_the_cells_the_line_draws_as() {
        assert_eq!(column_of("\tx", 1), 4, "a leading tab is four cells");
        assert_eq!(column_of("ab\tx", 3), 4, "two in, two to go");
        assert_eq!(column_of("\u{4f60}x", 3), 2, "a wide character is two");
        assert_eq!(byte_at_column("\tx", 4), 1);
        assert_eq!(byte_at_column("\tx", 2), 1, "inside a tab rounds forward");
        assert_eq!(byte_at_column("abc", 99), 3, "past the end is the end");
        assert_eq!(line_columns("ab\tc"), 5);
    }

    /// PIN — a caret never stands inside a line break.
    ///
    /// Mutation: drop the CRLF clause of [`normalize`].
    #[test]
    fn a_caret_never_stands_inside_a_break() {
        let content = "a\r\nb";
        assert_eq!(normalize(content, 2), 1, "between \\r and \\n is before it");
        assert_eq!(normalize(content, 1), 1);
        assert_eq!(normalize(content, 3), 3);
        assert_eq!(normalize("\u{4f60}", 1), 0, "mid-character is before it");
        assert_eq!(normalize("abc", 99), 3);
    }

    /// PIN — Backspace over a CRLF takes the whole break.
    ///
    /// Mutation: step back one byte rather than through
    /// [`previous_boundary`]'s break clause; the `\r` is then left behind as an
    /// invisible byte at the end of the joined line.
    #[test]
    fn backspace_at_a_line_start_joins_the_whole_break() {
        let mut content = "one\r\ntwo".to_owned();
        let mut caret = at(5);
        assert!(backspace(&mut content, &mut caret));
        assert_eq!(content, "onetwo");
        assert_eq!(caret.caret, 3);

        let mut content = "one\ntwo".to_owned();
        let mut caret = at(4);
        assert!(backspace(&mut content, &mut caret));
        assert_eq!(content, "onetwo");

        // At the very start there is nothing to take.
        let mut caret = at(0);
        assert!(!backspace(&mut content, &mut caret));
        assert_eq!(content, "onetwo");
    }

    /// PIN — Enter writes the break the file already uses.
    ///
    /// Mutation: return `"\n"` unconditionally from [`eol_of`], which turns
    /// every edited CRLF file into a mixed one.
    #[test]
    fn enter_writes_the_break_the_file_already_uses() {
        assert_eq!(eol_of("a\r\nb"), "\r\n");
        assert_eq!(eol_of("a\nb"), "\n");
        assert_eq!(eol_of("no breaks at all"), "\n");
        assert_eq!(eol_of("\na"), "\n");

        let mut content = "a\r\nb".to_owned();
        let mut caret = at(4);
        let eol = eol_of(&content);
        assert!(insert(&mut content, &mut caret, eol));
        assert_eq!(content, "a\r\nb\r\n");
    }

    /// PIN — a vertical run keeps the column it started in.
    ///
    /// Mutation: clear `desired_column` on every move.
    #[test]
    fn a_vertical_run_keeps_the_column_it_started_in() {
        let content = "long line here\nsh\nlong line here";
        let mut caret = at(10);
        move_caret(content, &mut caret, Motion::Down, false, 10);
        assert_eq!(caret.caret, 17, "the short line's end");
        move_caret(content, &mut caret, Motion::Down, false, 10);
        assert_eq!(caret.caret, 28, "and back out to column ten");
        // A horizontal move gives the memory up, as every editor does.
        move_caret(content, &mut caret, Motion::Left, false, 10);
        assert_eq!(caret.desired_column, None);
    }

    /// PIN — Shift extends, a bare arrow does not.
    ///
    /// Mutation: move the anchor in [`finish_move`] whatever `extend` says.
    #[test]
    fn shift_extends_and_a_bare_arrow_collapses() {
        let content = "abcdef";
        let mut caret = at(2);
        move_caret(content, &mut caret, Motion::Right, true, 10);
        move_caret(content, &mut caret, Motion::Right, true, 10);
        assert_eq!(caret.range(), 2..4);
        assert_eq!(caret.selected(content), "cd");
        // A bare Left out of a selection lands on its near end and takes
        // nothing with it.
        move_caret(content, &mut caret, Motion::Left, false, 10);
        assert_eq!(caret.caret, 2);
        assert!(caret.is_empty());
    }

    /// PIN — the editor owns every key it is handed.
    ///
    /// P139: "editor keys are the editor's — the terminal must not hear them".
    /// A key with no verb is [`EditCommand::Ignore`] and is still swallowed;
    /// nothing returns "not mine".
    ///
    /// Mutation: return [`EditCommand::Ignore`] for `Key::Character`, which is
    /// exactly the bug where typing in the preview lands in the shell.
    #[test]
    fn the_editor_owns_every_key_it_is_handed() {
        let ch = |text: &str| Key::Character(text.into());
        let none = ModifiersState::empty();
        assert_eq!(
            command(&ch("a"), none),
            EditCommand::Insert("a".to_owned()),
            "a letter is typed, never encoded"
        );
        assert_eq!(
            command(&ch("s"), ModifiersState::CONTROL),
            EditCommand::Save
        );
        assert_eq!(
            command(&ch("v"), ModifiersState::CONTROL),
            EditCommand::Paste
        );
        assert_eq!(
            command(&Key::Named(NamedKey::Escape), none),
            EditCommand::Release
        );
        assert_eq!(command(&Key::Named(NamedKey::Tab), none), EditCommand::Tab);
        assert_eq!(
            command(&Key::Named(NamedKey::Enter), none),
            EditCommand::Newline
        );
        assert_eq!(
            command(&Key::Named(NamedKey::ArrowUp), ModifiersState::SHIFT),
            EditCommand::Move {
                motion: Motion::Up,
                extend: true
            }
        );
        assert_eq!(
            command(&Key::Named(NamedKey::Home), ModifiersState::CONTROL),
            EditCommand::Move {
                motion: Motion::DocStart,
                extend: false
            }
        );
        // AltGr is typing, not a chord — the one place a Ctrl means nothing.
        assert_eq!(
            command(
                &ch("\u{20ac}"),
                ModifiersState::CONTROL | ModifiersState::ALT
            ),
            EditCommand::Insert("\u{20ac}".to_owned())
        );
        // And a key with nothing to do is still the editor's.
        assert_eq!(
            command(&Key::Named(NamedKey::F5), none),
            EditCommand::Ignore
        );
    }

    /// PIN — a selection's band covers the break at the end of every line
    /// inside it.
    ///
    /// Mutation: stop the band at the line's own text, which draws a multi-line
    /// selection as several unrelated ones.
    #[test]
    fn a_selection_band_covers_the_break_of_every_line_inside_it() {
        let content = "one\ntwo\nthree";
        let starts = line_starts(content);
        let range = 1..9;
        assert_eq!(selected_columns(content, &starts, 0, &range), Some((1, 4)));
        assert_eq!(selected_columns(content, &starts, 1, &range), Some((0, 4)));
        assert_eq!(selected_columns(content, &starts, 2, &range), Some((0, 1)));
        let empty = 3..3;
        assert_eq!(selected_columns(content, &starts, 0, &empty), None);
        let inside = 1..2;
        assert_eq!(selected_columns(content, &starts, 1, &inside), None);
    }
}
