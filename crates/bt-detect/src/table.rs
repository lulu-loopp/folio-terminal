//! Strict GitHub-Flavoured-Markdown pipe-table recognition for terminal output.
//!
//! # Why this is not the preview's parser
//!
//! `bt-app`'s preview reads a *file the user opened*, and a file that opens in a
//! markdown view has already declared what it is. Terminal output has declared
//! nothing: it is log lines, ASCII art, `||` in a shell condition, box-drawing
//! from another program's own table, and — every so often — a real GFM table an
//! agent printed. So the preview's `is_pipe_row` ("a pipe somewhere in it") is
//! exactly the wrong rule here, and this module states the strict one instead.
//!
//! # The rule, and where each half of it comes from
//!
//! GFM ([GitHub Flavored Markdown Spec, §4.10 "Tables (extension)"]) says a
//! table is *a header row, a delimiter row, and zero or more body rows*:
//!
//! * **The delimiter row** "consists of cells whose only content are hyphens
//!   (`-`), and optionally, a leading or trailing colon (`:`), or both, to
//!   indicate left, right, or center alignment respectively."
//! * **"The header row must match the delimiter row in the number of cells. If
//!   not, a table will not be recognized."** — this is why a jagged
//!   header/delimiter pair is not a table at all, rather than a table with a
//!   missing column.
//! * Leading and trailing pipes are optional: `| a | b |` and `a | b` are the
//!   same two columns.
//! * A pipe inside a cell is written `\|`.
//!
//! Three rules here are **deliberately stricter than GFM**, each because the
//! input is a terminal and not a document. They are stated, not hidden:
//!
//! 1. **Both the header row and the delimiter row must contain an unescaped
//!    `|`.** GFM reaches a one-column pipeless table only in constructions
//!    CommonMark resolves elsewhere (`abc` over `---` is a setext heading, not a
//!    table), and a terminal prints `---` under a word constantly. Requiring the
//!    pipe encodes the outcome without depending on setext precedence, which
//!    this scanner does not implement.
//! 2. **A body row whose cell count differs from the header's ends the table,
//!    and refuses it whole if that row begins with a pipe.** GFM pads a short
//!    row with empty cells and drops a long row's excess (§4.10: "The remainder
//!    of the table's rows may vary in the number of cells"). That rule is right
//!    for an authored document, where the text after a table is more of the same
//!    document; it is wrong here, where the text after a table is arbitrary
//!    program output that may well contain a pipe — `git log --graph`'s `|/`, a
//!    rustc gutter's `  | ^^^ expected integer`, an ASCII frame's
//!    `| WARNING: disconnected |`, a psql row with more columns than these
//!    headings have. Padding any of those into the table would print somebody
//!    else's output as this table's data.
//!
//!    So the count stays exact, and what a mismatching line does depends on
//!    whether it leads with a pipe. A line that does not — prose, a blank line,
//!    the end of the input — simply ends the table where GFM ends it, and the
//!    rows above it are drawn. A line that **does** lead with an unescaped `|`
//!    (after any indentation) and is not a row of this table refuses the whole
//!    candidate: nothing is drawn and every line of it stays text. That line is
//!    either this table's own row arriving damaged — the report of 2026-09-08,
//!    where a cell held an unescaped `|β|` and counted five cells against three
//!    — or it is unrelated output, and nothing here can tell the two apart. A
//!    half-drawn table is worse than none: it publishes a table with a row
//!    missing and says nothing about the row.
//! 3. **A header row that is itself a delimiter row is not a header.** Two rule
//!    lines stacked (`|---|---|` twice) is somebody drawing a box, and GFM's own
//!    answer — a table whose headings are the three characters `---` — is a
//!    table nobody wrote on purpose.
//!
//! # A row the printing program wrapped
//!
//! A program that lays its own output out to the terminal's width wraps a long
//! row itself, and its wrap is in the bytes: the second half arrives as its own
//! line, and that line does not begin with `|`. Read literally that is a row of
//! the wrong width and, by rule 2, the end of the table — with the first half
//! left standing as a complete row, its last cell cut in two.
//!
//! Such a row is rejoined, but only on evidence, and the evidence is provenance
//! rather than punctuation. [`TableLine::captured_columns`] is the width of the
//! grid the physical row was taken off, recorded at capture time and immutable
//! afterwards, so it still answers after the pane has been resized. A line joins
//! the line under it only when all three hold:
//!
//! * the line **filled the row it was printed on** — its own cells, counting
//!   indentation and counting a wide character as the two it occupies, come to
//!   exactly its captured column count, which is what a program running out of
//!   row looks like;
//! * the line under it **does not begin with an unescaped `|`**, because that is
//!   a row opening and never a row's tail;
//! * and the rejoined line splits into **exactly the header's number of cells**.
//!
//! More than one continuation is allowed while each line before it filled its
//! row too. The join puts back one space when the two characters either side of
//! the break are both ASCII word characters — a Latin word wrap dropped one —
//! and nothing otherwise, since a CJK or mid-token break dropped nothing. A row
//! that cannot be rejoined under these conditions is not a row, and rule 2 then
//! says what happens to the table.
//!
//! Everything else is GFM's own answer, including the one the real-world sample
//! asked about: **an empty header cell is legal.** `| | 计划发卡 β 峰 | 时间 |`
//! is three cells, the first of them empty, and §4.10 constrains only the *count*
//! of the header row's cells, never their content. With `|---|---|---|` under it
//! the counts match, so it is a table, and we render it.
//!
//! [GitHub Flavored Markdown Spec, §4.10 "Tables (extension)"]: https://github.github.com/gfm/#tables-extension-

use bt_unicode::text_width;

/// One input line, and the terminal geometry it was captured on.
///
/// The text alone cannot say whether a program wrapped a row: `| a | b` is the same string
/// whether the program ended it there or ran out of row. `captured_columns` is the grid width the
/// physical row was taken off — immutable provenance recorded at capture time, exactly as
/// `bt_transcript::PhysicalFragment::captured_columns` records it — and it is the only thing that
/// can answer the question. Zero means a line with no capture geometry (a fixture, a synthetic
/// line, a logical line the terminal itself rejoined out of several physical rows), and a line
/// with no geometry never joins a continuation onto itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TableLine<'a> {
    pub text: &'a str,
    pub captured_columns: u32,
}

impl<'a> TableLine<'a> {
    /// A line the caller can say nothing about the grid for.
    #[must_use]
    pub fn without_geometry(text: &'a str) -> Self {
        Self {
            text,
            captured_columns: 0,
        }
    }

    /// A line captured on a grid of a stated width.
    #[must_use]
    pub fn on_grid(text: &'a str, captured_columns: u32) -> Self {
        Self {
            text,
            captured_columns,
        }
    }

    /// Did this line's text run to the last cell of the row it was printed on?
    ///
    /// Indentation counts as the spaces it is and a wide character counts as the two cells it
    /// occupies, because the ruler is the grid and not the byte count.
    fn filled_its_row(self) -> bool {
        self.captured_columns != 0
            && u32::try_from(text_width(self.text)).is_ok_and(|used| used == self.captured_columns)
    }
}

/// Borrow a slice of lines with no capture geometry at all.
#[must_use]
pub fn lines_without_geometry<'a>(texts: &[&'a str]) -> Vec<TableLine<'a>> {
    texts
        .iter()
        .copied()
        .map(TableLine::without_geometry)
        .collect()
}

/// What stands at the head of a slice of lines.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TableCandidate {
    /// A table, and the rows it resolved.
    Proven(TableSpan),
    /// A header row over a delimiter row stood here, and the line immediately after the last row
    /// it could accept begins with an unescaped `|` and is not a row of this table. Nothing is
    /// drawn: see the module's rule 2.
    ///
    /// `line_count` counts the header row through the last row that *would* have been accepted,
    /// so `lines[line_count]` is the refusing line itself — which is not part of the refusal and
    /// may open a table of its own.
    Refused { line_count: usize },
}

/// What a delimiter-row cell's colons declared for its column.
///
/// `None` is not `Left`: GFM's default is "whatever the renderer does with an
/// undeclared column", and keeping the two apart lets the painter treat an
/// undeclared column as ordinary text while an explicit `:---` is a decision the
/// author made.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ColumnAlignment {
    #[default]
    None,
    Left,
    Center,
    Right,
}

/// One recognised table: its heading row, its body, and what its colons said.
///
/// Cells are the source text of each cell with surrounding whitespace trimmed
/// and `\|` unescaped — that is, exactly the text a renderer would then parse
/// inline runs out of. The original terminal bytes are **not** here: they stay in
/// the transcript, which is the whole point of a rendered block being a view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableSpan {
    /// One entry per column; `alignments.len()` is the column count and every
    /// row has exactly that many cells.
    pub alignments: Vec<ColumnAlignment>,
    /// The heading row.
    pub header: Vec<String>,
    /// Zero or more body rows.
    pub body: Vec<Vec<String>>,
    /// How many input lines the table consumed, delimiter row included. Always
    /// at least 2.
    pub line_count: usize,
}

impl TableSpan {
    /// Column count — the same number for the header, every body row and the
    /// alignment list, by construction.
    #[must_use]
    pub fn columns(&self) -> usize {
        self.alignments.len()
    }

    /// Every row, heading first — the shape a painter walks.
    pub fn rows(&self) -> impl Iterator<Item = &Vec<String>> {
        std::iter::once(&self.header).chain(self.body.iter())
    }

    /// Total row count, heading included.
    #[must_use]
    pub fn row_count(&self) -> usize {
        1 + self.body.len()
    }

    /// The rows this span resolved, written back out one row to a line.
    ///
    /// # Why the painter is handed this and not the terminal's own bytes
    ///
    /// A row a program wrapped arrives as two lines and is one row, and only the capture geometry
    /// could say so. The geometry belongs to the transcript and the painter is three crates away
    /// from it, so handing the painter the raw bytes would mean asking it to reach the same verdict
    /// again from less evidence — and a painter that disagreed would draw a different table from
    /// the one that was proven. This is the resolution written down: exactly one line per row,
    /// every literal pipe escaped again, so [`from_resolved_source`] reads it back with no geometry
    /// at all and cannot reach any other answer. It is also why a resize cannot re-split a joined
    /// row: the join was over the bytes the program printed, and this is where that reading lives.
    #[must_use]
    pub fn resolved_source(&self) -> String {
        let mut source = String::new();
        push_resolved_row(&mut source, &self.header);
        source.push('\n');
        source.push('|');
        for alignment in &self.alignments {
            source.push_str(match alignment {
                ColumnAlignment::None => "---",
                ColumnAlignment::Left => ":--",
                ColumnAlignment::Center => ":-:",
                ColumnAlignment::Right => "--:",
            });
            source.push('|');
        }
        for row in &self.body {
            source.push('\n');
            push_resolved_row(&mut source, row);
        }
        source
    }
}

/// Read back a source [`TableSpan::resolved_source`] wrote.
///
/// No geometry, because none is needed: every row already stands on its own line, so no join can
/// be owed and none is attempted.
#[must_use]
pub fn from_resolved_source(source: &str) -> Option<TableSpan> {
    let texts: Vec<&str> = source.lines().collect();
    match table_at(&lines_without_geometry(&texts))? {
        TableCandidate::Proven(span) => Some(span),
        TableCandidate::Refused { .. } => None,
    }
}

/// Write one row as `| a | b |`, escaping a literal pipe back into `\|`.
fn push_resolved_row(source: &mut String, cells: &[String]) {
    source.push('|');
    for cell in cells {
        source.push(' ');
        for character in cell.chars() {
            if character == '|' {
                source.push('\\');
            }
            source.push(character);
        }
        source.push_str(" |");
    }
}

/// Recognise a table beginning at `lines[0]`.
///
/// `None` unless `lines[0]` is a header row and `lines[1]` is a delimiter row of the same width.
/// The table then extends over every following line that is a row of that same width — including a
/// row the printing program wrapped over two or more lines, which the capture geometry proves and
/// which is rejoined here.
///
/// The first line that is not a row ends the run and is never consumed. If it leads with an
/// unescaped pipe the whole candidate is [`TableCandidate::Refused`] and nothing is drawn;
/// otherwise the rows above it are a table. Both halves are rule 2 — see the module documentation.
#[must_use]
pub fn table_at(lines: &[TableLine]) -> Option<TableCandidate> {
    let header = split_row(lines.first()?.text)?;
    // Rule 3: two stacked rule lines are a box, not a heading over a rule.
    if delimiter_row(lines[0].text).is_some() {
        return None;
    }
    let alignments = delimiter_row(lines.get(1)?.text)?;
    // GFM §4.10: "The header row must match the delimiter row in the number of
    // cells. If not, a table will not be recognized."
    if alignments.len() != header.len() {
        return None;
    }
    let mut body = Vec::new();
    let mut line_count = 2;
    while let Some((row, consumed)) = row_at(lines, line_count, header.len()) {
        body.push(row);
        line_count += consumed;
    }
    // Rule 2. The line that ended the run decides whether there is a table here at all: one that
    // leads with a pipe and is not a row of this table is either the table's own row damaged in
    // transit or somebody else's pipe, and neither can be told from the other. Drawing the rows
    // above it would publish a table that is missing a row and say nothing about it.
    if lines
        .get(line_count)
        .is_some_and(|line| begins_with_unescaped_pipe(line.text))
    {
        return Some(TableCandidate::Refused { line_count });
    }
    Some(TableCandidate::Proven(TableSpan {
        alignments,
        header,
        body,
        line_count,
    }))
}

/// One body row starting at `lines[index]`, and how many input lines it took.
///
/// The joined reading wins whenever it is available, and it has to: a row the program wrapped
/// after its last complete cell (`| a | b | c` over `d |`) is a perfectly well-formed row of the
/// right width on its own, and reading it that way draws the row with its tail missing.
fn row_at(lines: &[TableLine], index: usize, columns: usize) -> Option<(Vec<String>, usize)> {
    let line = *lines.get(index)?;
    if line.filled_its_row()
        && let Some(joined) = joined_row(lines, index, columns)
    {
        return Some(joined);
    }
    body_row(line.text)
        .filter(|row| row.len() == columns)
        .map(|row| (row, 1))
}

/// Rejoin a row the printing program wrapped, or `None` if nothing proves one was wrapped.
///
/// Three things have to hold together, and each is doing its own work. The line being continued
/// **filled the row it was printed on** — the caller has checked it, and every further line must
/// too, which is what lets a row wrapped onto three physical lines rejoin while a row that simply
/// ended stops at one. The continuation **does not lead with a pipe**, because a line that leads
/// with a pipe is a row's own opening and never the tail of one. And the rejoined line **splits
/// into exactly the header's number of cells**, which is the only thing that can tell a wrap from
/// two unrelated lines that happen to sit next to each other: a prompt and the status line under it
/// join into one cell, not three, and are refused here.
fn joined_row(lines: &[TableLine], index: usize, columns: usize) -> Option<(Vec<String>, usize)> {
    let mut text = lines[index].text.to_owned();
    let mut consumed = 1;
    loop {
        let next = *lines.get(index + consumed)?;
        if begins_with_unescaped_pipe(next.text) {
            return None;
        }
        text = join_continuation(&text, next.text);
        consumed += 1;
        if let Some(row) = body_row(&text).filter(|row| row.len() == columns) {
            return Some((row, consumed));
        }
        if !next.filled_its_row() {
            return None;
        }
    }
}

/// Put two physical lines of one row back together.
///
/// A space goes back in only where one was taken out. A program that wraps Latin text breaks at a
/// space and drops it, so `and` over `six` is `and six`; a program that wraps CJK, or that runs out
/// of row in the middle of a token, breaks between two characters that were adjacent, and putting a
/// space there would insert a word the program never printed.
fn join_continuation(left: &str, right: &str) -> String {
    let mut joined = String::with_capacity(left.len() + right.len() + 1);
    joined.push_str(left);
    if is_ascii_word(left.chars().next_back()) && is_ascii_word(right.chars().next()) {
        joined.push(' ');
    }
    joined.push_str(right);
    joined
}

fn is_ascii_word(character: Option<char>) -> bool {
    character.is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// Whether the first thing on the line, after any indentation, is an unescaped pipe.
fn begins_with_unescaped_pipe(text: &str) -> bool {
    text.trim_start().starts_with('|')
}

/// Whether a line, on its own, could be a table row: it holds an unescaped pipe
/// and something that is not a pipe or a space.
///
/// This is the cheap gate a line-at-a-time scanner asks before it is willing to
/// look at the line after this one.
#[must_use]
pub fn is_row_shaped(line: &str) -> bool {
    split_row(line).is_some()
}

/// Split one row into its cells, or `None` if the line is not a row at all.
///
/// A row must carry at least one unescaped `|` (strict rule 1) and at least one
/// character that is neither a pipe nor whitespace — `||` in a shell condition
/// has the pipes and none of the content.
///
/// The content requirement is what a line must clear to *open* a table, and it
/// is why this is not the rule for body rows: see [`body_row`].
#[must_use]
pub fn split_row(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed
        .chars()
        .any(|character| character != '|' && !character.is_whitespace())
    {
        return None;
    }
    body_row(line)
}

/// Split a row inside an already-opened table.
///
/// Identical to [`split_row`] except that a row of wholly empty cells is
/// allowed: `|   |   |` is a legal GFM row, and once a header and a delimiter
/// row have proven a table stands here, a line of the right width made of
/// nothing is far likelier to be that table's blank row than it is to be the
/// `||` this module refuses to open on.
#[must_use]
pub fn body_row(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !has_unescaped_pipe(trimmed) {
        return None;
    }
    let inner = strip_edge_pipes(trimmed);
    if inner.is_empty() {
        return None;
    }
    Some(split_cells(inner))
}

/// Read a delimiter row's alignments, or `None` if the line is not one.
///
/// Every cell must be, after trimming, an optional leading `:`, one or more `-`,
/// and an optional trailing `:` — nothing else. A cell holding a letter, a `+`,
/// an em dash, or nothing at all disqualifies the whole row.
#[must_use]
pub fn delimiter_row(line: &str) -> Option<Vec<ColumnAlignment>> {
    let cells = split_row(line)?;
    cells.iter().map(|cell| cell_alignment(cell)).collect()
}

fn cell_alignment(cell: &str) -> Option<ColumnAlignment> {
    let cell = cell.trim();
    let (left, cell) = match cell.strip_prefix(':') {
        Some(rest) => (true, rest),
        None => (false, cell),
    };
    let (right, cell) = match cell.strip_suffix(':') {
        Some(rest) => (true, rest),
        None => (false, cell),
    };
    if cell.is_empty() || !cell.chars().all(|character| character == '-') {
        return None;
    }
    Some(match (left, right) {
        (true, true) => ColumnAlignment::Center,
        (true, false) => ColumnAlignment::Left,
        (false, true) => ColumnAlignment::Right,
        (false, false) => ColumnAlignment::None,
    })
}

/// Drop one optional leading and one optional trailing pipe.
///
/// Only an *unescaped* trailing pipe is an edge: a row ending `…\|` ends with a
/// literal pipe inside its last cell, and eating it would silently lose a
/// character of the user's text.
fn strip_edge_pipes(trimmed: &str) -> &str {
    let without_leading = trimmed.strip_prefix('|').unwrap_or(trimmed);
    match without_leading.strip_suffix('|') {
        Some(without_trailing) if !ends_with_escape(without_trailing) => without_trailing,
        _ => without_leading,
    }
}

/// Split on unescaped pipes, trimming each cell and unescaping `\|`.
fn split_cells(inner: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut escaped = false;
    for character in inner.chars() {
        match character {
            _ if escaped => {
                // Only `\|` is a pipe escape; every other `\x` keeps both
                // characters, because this module is not an inline parser and
                // must not eat a backslash that means something downstream.
                if character != '|' {
                    cell.push('\\');
                }
                cell.push(character);
                escaped = false;
            }
            '\\' => escaped = true,
            '|' => cells.push(std::mem::take(&mut cell).trim().to_owned()),
            _ => cell.push(character),
        }
    }
    if escaped {
        cell.push('\\');
    }
    cells.push(cell.trim().to_owned());
    cells
}

fn has_unescaped_pipe(text: &str) -> bool {
    let mut escaped = false;
    for character in text.chars() {
        match character {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '|' => return true,
            _ => {}
        }
    }
    false
}

/// Whether `text` ends in an odd run of backslashes, which is what makes the
/// character after it escaped.
fn ends_with_escape(text: &str) -> bool {
    text.chars().rev().take_while(|it| *it == '\\').count() % 2 == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table from the report, bars and program wrap and all.
    const REPORTED_TABLE: &str = "| 项目 | A 方案 | B 方案 |
|---|---|---|
| 漂 vs |β|≤5° 的 grip | 快 3.4% | 快 3.7% |
| 走廊 / 入弯余量 | 3.0 零越 / 3.1-3.9 m 未花 | 3.0 零越
/ 3.7-4.1 m 未花 |";

    /// The same table with the two literal bars escaped, which is what a producer has to write.
    const REPORTED_TABLE_ESCAPED: &str = "| 项目 | A 方案 | B 方案 |
|---|---|---|
| 漂 vs \\|β\\|≤5° 的 grip | 快 3.4% | 快 3.7% |
| 走廊 / 入弯余量 | 3.0 零越 / 3.1-3.9 m 未花 | 3.0 零越
/ 3.7-4.1 m 未花 |";

    /// The grid the report was printed on: the wrapped row's first physical line fills it.
    fn reported_grid() -> u32 {
        fills("| 走廊 / 入弯余量 | 3.0 零越 / 3.1-3.9 m 未花 | 3.0 零越")
    }

    /// A perfectly ordinary two-column table, for the lines that come after one.
    const POISON_BASE: &str = "| name | count |
| --- | ---: |
| alpha | 3 |";

    /// Read `source` as lines captured on a grid `columns` wide.
    fn candidate(source: &str, columns: u32) -> Option<TableCandidate> {
        let texts: Vec<&str> = source.lines().collect();
        let lines: Vec<TableLine> = texts
            .iter()
            .map(|text| TableLine::on_grid(text, columns))
            .collect();
        table_at(&lines)
    }

    /// Read `source` with no capture geometry at all, which is what every test that is not about
    /// a program's own wrap wants: no line can be shown to have filled its row, so no line joins.
    fn table(source: &str) -> Option<TableSpan> {
        match candidate(source, 0) {
            Some(TableCandidate::Proven(span)) => Some(span),
            _ => None,
        }
    }

    /// The grid width `text` exactly fills.
    fn fills(text: &str) -> u32 {
        text_width(text) as u32
    }

    #[test]
    fn a_header_over_a_delimiter_row_is_a_table() {
        let span = table("| a | b |\n| --- | --- |\n| 1 | 2 |").expect("a table");
        assert_eq!(span.header, vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(span.body, vec![vec!["1".to_owned(), "2".to_owned()]]);
        assert_eq!(span.columns(), 2);
        assert_eq!(span.row_count(), 2);
        assert_eq!(span.line_count, 3);
    }

    #[test]
    fn the_edge_pipes_are_optional_on_every_row_independently() {
        let span = table("a | b\n--- | ---\n1 | 2").expect("a table");
        assert_eq!(span.header, vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(span.body, vec![vec!["1".to_owned(), "2".to_owned()]]);
        let mixed = table("| a | b\n--- | ---|\n| 1 | 2").expect("a table");
        assert_eq!(mixed.header, span.header);
        assert_eq!(mixed.body, span.body);
    }

    #[test]
    fn a_table_may_have_no_body_at_all() {
        let span = table("| a | b |\n|---|---|").expect("a table");
        assert!(span.body.is_empty());
        assert_eq!(span.row_count(), 1);
        assert_eq!(span.line_count, 2);
    }

    #[test]
    fn the_colons_say_which_way_each_column_is_set() {
        let span = table("| l | c | r | n |\n| :-- | :-: | --: | --- |").expect("a table");
        assert_eq!(
            span.alignments,
            vec![
                ColumnAlignment::Left,
                ColumnAlignment::Center,
                ColumnAlignment::Right,
                ColumnAlignment::None,
            ]
        );
    }

    /// GFM §4.10 constrains the header row's cell **count**, never its content.
    /// The user's real-world sample is therefore a table, and the empty first
    /// heading is a heading that says nothing — which is what a corner cell over
    /// a column of row labels is *for*.
    #[test]
    fn an_empty_header_cell_is_legal_gfm_and_the_sample_renders() {
        let span = table(
            "| | 计划发卡 β 峰 | 时间 |\n|---|---|---|\n| 甲 | 12.5 | 08:00 |\n| 乙 | 9.0 | 09:30 |",
        )
        .expect("gfm accepts an empty header cell");
        assert_eq!(
            span.header,
            vec![String::new(), "计划发卡 β 峰".to_owned(), "时间".to_owned()]
        );
        assert_eq!(span.columns(), 3);
        assert_eq!(span.body.len(), 2);
        assert_eq!(span.line_count, 4);
    }

    #[test]
    fn an_empty_body_cell_is_legal_too() {
        let span = table("| a | b |\n|---|---|\n| | 2 |").expect("a table");
        assert_eq!(span.body, vec![vec![String::new(), "2".to_owned()]]);
    }

    #[test]
    fn a_lone_pipe_in_a_log_line_is_not_a_table() {
        assert!(table("2026-08-18 12:00:01 INFO | starting up").is_none());
        assert!(
            table("2026-08-18 12:00:01 INFO | starting up\n2026-08-18 12:00:02 INFO | ready")
                .is_none()
        );
    }

    #[test]
    fn box_drawing_characters_never_trigger() {
        assert!(table("│ a │ b │\n├───┼───┤\n│ 1 │ 2 │").is_none());
        assert!(table("┌───┬───┐\n│ a │ b │\n└───┴───┘").is_none());
        // The other common ASCII frame: a `+---+` rule is not a delimiter row,
        // because `+---+---+` splits into one cell holding plus signs.
        assert!(table("+---+---+\n| a | b |\n+---+---+").is_none());
    }

    #[test]
    fn a_shell_conditions_double_pipe_is_not_a_table() {
        assert!(table("if [ -f x ] || [ -f y ]; then\n  echo both\nfi").is_none());
        // `||` alone has pipes but no content, so it cannot open a table.
        assert!(split_row("||").is_none());
        assert!(split_row("|  |").is_none(), "nor can a row of nothing");
    }

    #[test]
    fn an_opened_table_may_carry_a_row_of_empty_cells() {
        let span = table("| a | b |\n|---|---|\n|   |   |\n| 1 | 2 |").expect("a table");
        assert_eq!(
            span.body,
            vec![
                vec![String::new(), String::new()],
                vec!["1".to_owned(), "2".to_owned()],
            ]
        );
    }

    #[test]
    fn a_jagged_header_and_delimiter_pair_is_not_a_table() {
        assert!(table("| a | b | c |\n| --- | --- |\n| 1 | 2 | 3 |").is_none());
        assert!(table("| a | b |\n| --- | --- | --- |\n| 1 | 2 |").is_none());
    }

    #[test]
    fn a_delimiter_row_with_a_letter_in_it_is_not_a_delimiter_row() {
        assert!(table("| a | b |\n| --a-- | --- |\n| 1 | 2 |").is_none());
        assert!(delimiter_row("| --- | -x- |").is_none());
        assert!(
            delimiter_row("| --- | |").is_none(),
            "an empty cell is not dashes"
        );
        assert!(
            delimiter_row("| --- | :: |").is_none(),
            "colons alone are not dashes"
        );
        assert!(
            delimiter_row("| — | --- |").is_none(),
            "an em dash is not a hyphen"
        );
    }

    #[test]
    fn two_stacked_rule_lines_are_a_box_not_a_heading() {
        assert!(table("|---|---|\n|---|---|\n| 1 | 2 |").is_none());
    }

    #[test]
    fn a_table_drawn_with_a_top_rule_still_finds_its_real_heading() {
        // Line 0 is a rule; the table begins at line 1, where a real heading
        // stands over a real delimiter.
        let texts: Vec<&str> = "|-----|-----|\n| a | b |\n|-----|-----|\n| 1 | 2 |"
            .lines()
            .collect();
        let lines = lines_without_geometry(&texts);
        assert!(table_at(&lines).is_none(), "not at the rule");
        let TableCandidate::Proven(span) = table_at(&lines[1..]).expect("a table at the heading")
        else {
            panic!("a table, not a refusal");
        };
        assert_eq!(span.header, vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(span.body, vec![vec!["1".to_owned(), "2".to_owned()]]);
    }

    #[test]
    fn a_body_row_of_another_width_ends_the_table_instead_of_poisoning_it() {
        assert_eq!(
            candidate(
                "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 | 5 |\n| 6 | 7 |",
                0
            ),
            Some(TableCandidate::Refused { line_count: 3 }),
            "the jagged row leads with a pipe, so it is never padded and never drawn around"
        );
        // The same jaggedness without the leading pipe is an ordinary boundary: the table ends
        // above it and the rows that were proven are drawn.
        let span = table("| a | b |\n|---|---|\n| 1 | 2 |\n3 | 4 | 5").expect("a table");
        assert_eq!(span.body, vec![vec!["1".to_owned(), "2".to_owned()]]);
        assert_eq!(span.line_count, 3, "the jagged row is not consumed");
    }

    #[test]
    fn a_blank_line_ends_the_table_and_so_does_prose() {
        let blank = table("| a | b |\n|---|---|\n| 1 | 2 |\n\n| 3 | 4 |").expect("a table");
        assert_eq!(blank.line_count, 3);
        let prose = table("| a | b |\n|---|---|\n| 1 | 2 |\nand then some prose").expect("a table");
        assert_eq!(prose.line_count, 3);
    }

    #[test]
    fn a_pipeless_word_over_a_rule_is_a_setext_heading_and_never_a_table() {
        assert!(table("abc\n---\nbody").is_none());
        assert!(table("Total\n-----").is_none());
    }

    #[test]
    fn a_pipe_inside_a_cell_is_written_with_a_backslash() {
        let escaped = table("| a \\| b | c |\n|---|---|").expect("a table");
        assert_eq!(escaped.header, vec!["a | b".to_owned(), "c".to_owned()]);
        assert_eq!(escaped.columns(), 2);
    }

    #[test]
    fn a_trailing_escaped_pipe_is_content_and_not_an_edge() {
        let span = table("| a | b\\| |\n|---|---|").expect("a table");
        assert_eq!(span.header, vec!["a".to_owned(), "b|".to_owned()]);
    }

    #[test]
    fn a_backslash_before_anything_else_keeps_both_characters() {
        assert!(table("| \\d+ | b |").is_none(), "one line is never a table");
        let span = table("| \\d+ | b |\n|---|---|").expect("a table");
        assert_eq!(span.header, vec!["\\d+".to_owned(), "b".to_owned()]);
    }

    #[test]
    fn one_column_is_a_table_when_it_carries_its_pipes() {
        let span = table("| a |\n| --- |\n| 1 |").expect("a table");
        assert_eq!(span.columns(), 1);
        assert_eq!(span.body, vec![vec!["1".to_owned()]]);
    }

    #[test]
    fn the_rows_iterator_puts_the_heading_first() {
        let span = table("| a | b |\n|---|---|\n| 1 | 2 |").expect("a table");
        let rows: Vec<&Vec<String>> = span.rows().collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], &span.header);
        assert_eq!(rows[1], &span.body[0]);
    }

    /// The table from the 2026-09-08 report, at the width Claude Code printed it: a body row
    /// carrying an unescaped `|β|` counts five cells against three, and it begins with a pipe, so
    /// the whole candidate is refused and nothing is drawn.
    #[test]
    fn the_reported_table_is_refused_whole_because_of_its_unescaped_bars() {
        let refused = candidate(REPORTED_TABLE, reported_grid()).expect("a candidate stands here");
        assert_eq!(
            refused,
            TableCandidate::Refused { line_count: 2 },
            "the header and the delimiter row stood, and the very next line refused them"
        );
    }

    /// The same table with the bars written `\|`, at the same width: three cells everywhere, and
    /// the row the program itself wrapped is read as the one row it was printed as.
    #[test]
    fn the_reported_table_with_escaped_bars_rejoins_its_wrapped_row() {
        let TableCandidate::Proven(span) =
            candidate(REPORTED_TABLE_ESCAPED, reported_grid()).expect("a table")
        else {
            panic!("a table, not a refusal");
        };
        assert_eq!(span.columns(), 3);
        assert_eq!(span.body.len(), 2, "two body rows, not three");
        assert_eq!(
            span.body[1],
            vec![
                "走廊 / 入弯余量".to_owned(),
                "3.0 零越 / 3.1-3.9 m 未花".to_owned(),
                "3.0 零越/ 3.7-4.1 m 未花".to_owned(),
            ]
        );
        assert_eq!(span.line_count, 5, "both physical lines of the wrapped row");
    }

    /// A row the program wrapped, at the width it wrapped at.
    const WRAPPED: &str = "a | b | c
--- | --- | ---
one | two | three
four | five and
six | seven";

    #[test]
    fn a_row_wrapped_at_the_width_it_filled_is_rejoined_as_one_row() {
        let TableCandidate::Proven(span) =
            candidate(WRAPPED, fills("four | five and")).expect("a table")
        else {
            panic!("a table, not a refusal");
        };
        assert_eq!(span.columns(), 3);
        assert_eq!(
            span.body,
            vec![
                vec!["one".to_owned(), "two".to_owned(), "three".to_owned()],
                vec![
                    "four".to_owned(),
                    "five and six".to_owned(),
                    "seven".to_owned()
                ],
            ],
            "the Latin word wrap gets its space back"
        );
        assert_eq!(span.line_count, 5);
    }

    /// The same bytes, captured on a wider grid: the first line stopped short of the row it was
    /// printed on, so nothing proves the program wrapped it and nothing is joined.
    #[test]
    fn the_same_bytes_on_a_wider_grid_are_not_joined() {
        let TableCandidate::Proven(span) = candidate(WRAPPED, 23).expect("a table") else {
            panic!("a table, not a refusal");
        };
        assert_eq!(
            span.body,
            vec![vec!["one".to_owned(), "two".to_owned(), "three".to_owned()]],
            "the table ends before the half row"
        );
        assert_eq!(
            span.line_count, 3,
            "and the half row is not consumed; it does not begin with a pipe, so the rows above              it stand"
        );
    }

    /// Everything the review named as output that leads with a pipe and is not a row. Each one
    /// refuses the table it stands under, whole.
    #[test]
    fn a_pipe_leading_line_that_is_not_a_row_refuses_the_table_whole() {
        for poison in [
            "| * abc123 topic",
            "|/",
            "  | ^^^ expected integer",
            "| WARNING: disconnected |",
            "| Alice | 42 | active |",
            "| 12 | 34 | 56 | 78 |",
        ] {
            let source = format!(
                "{POISON_BASE}
{poison}"
            );
            assert_eq!(
                candidate(&source, 0),
                Some(TableCandidate::Refused { line_count: 3 }),
                "{poison}"
            );
        }
    }

    /// A prompt that fills its row and a status line under it: the join needs the header's own
    /// cell count and never gets it, so no row is fabricated and the table is refused.
    #[test]
    fn the_join_never_fires_on_a_prompt_and_the_status_line_under_it() {
        let source = format!(
            "{POISON_BASE}
| enter command>
status |"
        );
        assert_eq!(
            candidate(&source, fills("| enter command>")),
            Some(TableCandidate::Refused { line_count: 3 }),
        );
    }

    /// GFM's own ending is untouched: a line that does not begin with a pipe ends the table where
    /// it stands, and the rows above it are drawn.
    #[test]
    fn a_line_that_does_not_begin_with_a_pipe_still_only_ends_the_table() {
        let TableCandidate::Proven(span) = candidate(
            &format!(
                "{POISON_BASE}
and then some prose"
            ),
            0,
        )
        .expect("a table") else {
            panic!("a table, not a refusal");
        };
        assert_eq!(span.body.len(), 1);
        assert_eq!(span.line_count, 3);
    }

    /// The rows the detector resolved travel to the painter as source, and reading that source
    /// back with no geometry at all reaches the same rows. That is what keeps a rejoined row one
    /// row after a resize: the join is over the bytes the program printed, and the resolved source
    /// is where it was written down.
    #[test]
    fn the_resolved_source_reads_back_as_the_same_rows_without_any_geometry() {
        let TableCandidate::Proven(span) =
            candidate(REPORTED_TABLE_ESCAPED, reported_grid()).expect("a table")
        else {
            panic!("a table, not a refusal");
        };
        let read_back = from_resolved_source(&span.resolved_source()).expect("a table");
        assert_eq!(read_back.header, span.header);
        assert_eq!(read_back.body, span.body);
        assert_eq!(read_back.alignments, span.alignments);
        assert_eq!(read_back.line_count, span.row_count() + 1);
    }

    #[test]
    fn a_table_streams_in_and_grows_a_row_at_a_time() {
        // Nothing is a table until the delimiter row has arrived; from then on
        // every complete row extends it, and the partial line at the tail is
        // simply not yet a row.
        assert!(table("| a | b |").is_none());
        let two = table("| a | b |\n|---|---|").expect("a table with no body");
        assert_eq!(two.row_count(), 1);
        let three = table("| a | b |\n|---|---|\n| 1 | 2 |").expect("a table");
        assert_eq!(three.row_count(), 2);
        let four = table("| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |").expect("a table");
        assert_eq!(four.row_count(), 3);
        assert_eq!(four.line_count, 4);
    }
}
