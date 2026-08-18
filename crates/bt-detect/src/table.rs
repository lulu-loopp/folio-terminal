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
//! 2. **A body row whose cell count differs from the header's ends the table.**
//!    GFM pads a short row with empty cells and drops a long row's excess
//!    (§4.10: "The remainder of the table's rows may vary in the number of
//!    cells"). That rule is right for an authored document, where the text after
//!    a table is more of the same document; it is wrong here, where the text
//!    after a table is arbitrary program output that may well contain a pipe.
//!    Ending is the conservative reading and it cannot poison what follows.
//! 3. **A header row that is itself a delimiter row is not a header.** Two rule
//!    lines stacked (`|---|---|` twice) is somebody drawing a box, and GFM's own
//!    answer — a table whose headings are the three characters `---` — is a
//!    table nobody wrote on purpose.
//!
//! Everything else is GFM's own answer, including the one the real-world sample
//! asked about: **an empty header cell is legal.** `| | 计划发卡 β 峰 | 时间 |`
//! is three cells, the first of them empty, and §4.10 constrains only the *count*
//! of the header row's cells, never their content. With `|---|---|---|` under it
//! the counts match, so it is a table, and we render it.
//!
//! [GitHub Flavored Markdown Spec, §4.10 "Tables (extension)"]: https://github.github.com/gfm/#tables-extension-

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
}

/// Recognise a table beginning at `lines[0]`.
///
/// Returns `None` unless `lines[0]` is a header row and `lines[1]` is a
/// delimiter row of the same width. The table then extends over every following
/// line that is a row of that same width; the first line that is not ends it,
/// and that line is *not* consumed.
#[must_use]
pub fn table_at(lines: &[&str]) -> Option<TableSpan> {
    let header = split_row(lines.first()?)?;
    // Rule 3: two stacked rule lines are a box, not a heading over a rule.
    if delimiter_row(lines[0]).is_some() {
        return None;
    }
    let alignments = delimiter_row(lines.get(1)?)?;
    // GFM §4.10: "The header row must match the delimiter row in the number of
    // cells. If not, a table will not be recognized."
    if alignments.len() != header.len() {
        return None;
    }
    let mut body = Vec::new();
    let mut line_count = 2;
    for line in &lines[2..] {
        // Rule 2: a row of another width ends the table rather than being padded
        // into it.
        let Some(row) = body_row(line).filter(|row| row.len() == header.len()) else {
            break;
        };
        body.push(row);
        line_count += 1;
    }
    Some(TableSpan {
        alignments,
        header,
        body,
        line_count,
    })
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

    fn table(source: &str) -> Option<TableSpan> {
        let lines: Vec<&str> = source.lines().collect();
        table_at(&lines)
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
        let lines: Vec<&str> = "|-----|-----|\n| a | b |\n|-----|-----|\n| 1 | 2 |"
            .lines()
            .collect();
        assert!(table_at(&lines).is_none(), "not at the rule");
        let span = table_at(&lines[1..]).expect("a table at the heading");
        assert_eq!(span.header, vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(span.body, vec![vec!["1".to_owned(), "2".to_owned()]]);
    }

    #[test]
    fn a_body_row_of_another_width_ends_the_table_instead_of_poisoning_it() {
        let span =
            table("| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 | 5 |\n| 6 | 7 |").expect("a table");
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
