//! **What a window tells UI Automation about its terminals** (0.4.9 plan row 88, T-UIA-SKELETON).
//!
//! # Why
//!
//! Every assistive and input surface of Windows decides what a window *is* from its UI Automation
//! report: the touch keyboard shows itself when focus lands on an editable text element, Narrator
//! and NVDA read the element's Text pattern, Magnifier follows its caret, Voice Access dictates into
//! it, and a phone remote decides "this is a text field" the same way. A window that answers
//! `WM_GETOBJECT` with nothing is, to all of them, one empty pane. Folio drew everything itself and
//! answered nothing (`reports/T-WHAT-FOLIO-TELLS-WINDOWS-AUDIT.md`).
//!
//! # The shape
//!
//! Two halves, and the line between them is the whole design:
//!
//! * **This file, on every host** — the *value* a window publishes ([`WindowText`]: per terminal
//!   pane of the active tab, its visible rows as text, its caret, its selection, its rectangle and
//!   whether it holds the keyboard) and every piece of arithmetic UI Automation's text ranges are
//!   answered by (offsets, units, boundaries, rectangles). Pure, so it is tested on the Mac gate
//!   too.
//! * **`uia_windows.rs`, Windows only** — the subclass that answers `WM_GETOBJECT` with
//!   `UiaRootObjectId`, the COM providers, and the event road. Every answer a provider gives is
//!   computed from the last published [`WindowText`] and from nothing else in the process: the
//!   providers hold an `Arc` of it and never reach into the application's state.
//!
//! # Offsets
//!
//! A pane's text is a sequence of *units*: one per non-spacer cell of each visible row (a wide
//! glyph is one unit covering two columns; its spacer column is no unit; a cluster with combining
//! marks is one unit), and one line break after every row that does not continue into the next
//! (a soft-wrapped row joins the next with nothing between them). An offset is a position between
//! units, `0..=len`. Every range is a pair of offsets; every UI Automation unit is a set of
//! boundary offsets ([`Boundary`]).

/// A rectangle in the window's client area, in physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClientRect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

impl ClientRect {
    /// Whether the client point is inside.
    #[must_use]
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.left && x < self.left + self.width && y >= self.top && y < self.top + self.height
    }

    /// **The part of this rectangle inside `bounds`**, or `None` when nothing of it is.
    #[must_use]
    pub fn clipped(&self, bounds: &ClientRect) -> Option<ClientRect> {
        let left = self.left.max(bounds.left);
        let top = self.top.max(bounds.top);
        let right = (self.left + self.width).min(bounds.left + bounds.width);
        let bottom = (self.top + self.height).min(bounds.top + bounds.height);
        (right > left && bottom > top).then_some(ClientRect {
            left,
            top,
            width: right - left,
            height: bottom - top,
        })
    }
}

/// **A pane's identity in its window**: the tab it stands in and its seat. A seat number is
/// unique only inside its tab (every tab's first pane is seat 1); a tab number is the process's,
/// never given out twice. So the pair names one pane for as long as the window lives, and a pane
/// of another tab is never mistaken for it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PaneId {
    pub tab: u64,
    pub seat: u64,
}

/// **One window's publication**: the terminal panes of its active tab, in tree order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowText {
    pub panes: Vec<PaneText>,
}

impl WindowText {
    /// The pane with this identity.
    #[must_use]
    pub fn pane(&self, id: PaneId) -> Option<&PaneText> {
        self.panes.iter().find(|pane| pane.id == id)
    }

    /// The pane that holds the keyboard, if one does.
    #[must_use]
    pub fn focused(&self) -> Option<PaneId> {
        self.panes
            .iter()
            .find(|pane| pane.holds_keyboard)
            .map(|pane| pane.id)
    }
}

/// One cell as the builder hands it over.
#[derive(Clone, Copy, Debug)]
pub struct CellIn<'a> {
    /// The cell's cluster; an empty string is a blank.
    pub text: &'a str,
    /// The second column of a wide glyph: no unit of its own.
    pub spacer: bool,
    /// A word begins at this cell (the double-click rule's boundary, decided by the caller).
    pub word_start: bool,
}

/// One unit of a row: a cell's cluster, where it stands, and whether a word starts at it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Unit {
    /// Byte offset of the cluster in [`RowText::text`].
    byte: u32,
    /// The first column the cluster covers.
    column: u32,
    /// How many columns it covers (2 for a wide glyph).
    columns: u32,
    word_start: bool,
}

/// One visible row: its text, its units, and where it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct RowText {
    text: String,
    units: Vec<Unit>,
    /// The client x of column 0, and one column's width, in physical pixels.
    left: f64,
    cell_width: f64,
    top: f64,
    height: f64,
    /// This row continues into the next (a soft wrap): no line break between them.
    continues: bool,
}

/// Where a row is drawn, in client pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowPlace {
    pub left: f64,
    pub cell_width: f64,
    pub top: f64,
    pub height: f64,
}

impl RowText {
    /// **A row from its cells.** Spacers make no unit. Trailing blank cells are dropped, except on
    /// a row that continues (they are real columns of one logical line) and up to `keep_columns`
    /// (the caret's column on its own row, so a caret after a typed space keeps the space).
    #[must_use]
    pub fn from_cells<'a>(
        cells: impl IntoIterator<Item = CellIn<'a>>,
        place: RowPlace,
        continues: bool,
        keep_columns: u32,
    ) -> Self {
        let mut text = String::new();
        let mut units: Vec<Unit> = Vec::new();
        let mut column = 0u32;
        for cell in cells {
            if cell.spacer {
                if let Some(last) = units.last_mut()
                    && last.column + last.columns == column
                {
                    last.columns += 1;
                }
                column += 1;
                continue;
            }
            let byte = u32::try_from(text.len()).unwrap_or(u32::MAX);
            if cell.text.is_empty() {
                text.push(' ');
            } else {
                text.push_str(cell.text);
            }
            units.push(Unit {
                byte,
                column,
                columns: 1,
                word_start: cell.word_start,
            });
            column += 1;
        }
        if !continues {
            while let Some(last) = units.last().copied() {
                let blank = text[last.byte as usize..].chars().all(char::is_whitespace);
                if last.column < keep_columns || !blank {
                    break;
                }
                text.truncate(last.byte as usize);
                units.pop();
            }
        }
        Self {
            text,
            units,
            left: place.left,
            cell_width: place.cell_width,
            top: place.top,
            height: place.height,
            continues,
        }
    }

    fn unit_text(&self, index: usize) -> &str {
        let start = self.units[index].byte as usize;
        let end = self
            .units
            .get(index + 1)
            .map_or(self.text.len(), |next| next.byte as usize);
        &self.text[start..end]
    }

    /// The unit index a column falls in: the unit covering it, or the row's end past the last.
    fn index_at_column(&self, column: u32) -> usize {
        self.units
            .iter()
            .position(|unit| column < unit.column + unit.columns)
            .unwrap_or(self.units.len())
    }

    fn unit_left(&self, index: usize) -> f64 {
        match self.units.get(index) {
            Some(unit) => self.left + f64::from(unit.column) * self.cell_width,
            None => self.units.last().map_or(self.left, |unit| {
                self.left + f64::from(unit.column + unit.columns) * self.cell_width
            }),
        }
    }
}

/// A cell position as the builder knows it: a visible row and a column of it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellPoint {
    pub row: u32,
    pub column: u32,
}

/// **One terminal pane's publication.**
#[derive(Clone, Debug, PartialEq)]
pub struct PaneText {
    pub id: PaneId,
    pub name: String,
    /// The pane body, in client pixels.
    pub body: ClientRect,
    pub holds_keyboard: bool,
    rows: Vec<RowText>,
    /// The offset each row starts at, and the document's length after the last.
    starts: Vec<u32>,
    caret: Option<u32>,
    selection: Option<(u32, u32)>,
}

/// What a boundary is for: UI Automation's text units, reduced to the ones a pane supports.
/// `Format` answers as [`Boundary::Word`] and `Page` as [`Boundary::Document`]: an unsupported unit
/// takes the next larger supported one (`ITextRangeProvider::ExpandToEnclosingUnit`'s rule).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Boundary {
    Character,
    Word,
    Line,
    Paragraph,
    Document,
}

impl Boundary {
    /// UI Automation's `TextUnit` number (`TextUnit_Character` = 0 … `TextUnit_Document` = 6),
    /// mapped to the unit a pane answers it with.
    #[must_use]
    pub fn of_text_unit(unit: i32) -> Self {
        match unit {
            0 => Self::Character,
            1 | 2 => Self::Word,
            3 => Self::Line,
            4 => Self::Paragraph,
            _ => Self::Document,
        }
    }
}

/// What stands at an offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum At {
    /// The unit-th unit of a row.
    Cell { row: usize, unit: usize },
    /// The line break after a row.
    Break { row: usize },
    /// The document's end.
    End,
}

impl PaneText {
    /// **A pane from its rows.** `caret` and `selection` are cell positions; a caret on a wide
    /// glyph's spacer column lands on the glyph, and a selection's end is exclusive.
    #[must_use]
    pub fn new(
        id: PaneId,
        name: String,
        body: ClientRect,
        holds_keyboard: bool,
        rows: Vec<RowText>,
        caret: Option<CellPoint>,
        selection: Option<(CellPoint, CellPoint)>,
    ) -> Self {
        let mut starts = Vec::with_capacity(rows.len() + 1);
        let mut offset = 0u32;
        let last = rows.len().saturating_sub(1);
        for (index, row) in rows.iter().enumerate() {
            starts.push(offset);
            let units = u32::try_from(row.units.len()).unwrap_or(u32::MAX);
            let line_break = u32::from(index < last && !row.continues);
            offset = offset.saturating_add(units).saturating_add(line_break);
        }
        starts.push(offset);
        let mut pane = Self {
            id,
            name,
            body,
            holds_keyboard,
            rows,
            starts,
            caret: None,
            selection: None,
        };
        pane.caret = caret.map(|point| pane.offset_of(point));
        pane.selection = selection.map(|(start, end)| {
            let (start, end) = (pane.offset_of(start), pane.offset_of(end));
            (start.min(end), start.max(end))
        });
        pane
    }

    /// The document's length in units.
    #[must_use]
    pub fn len(&self) -> u32 {
        self.starts.last().copied().unwrap_or(0)
    }

    /// Whether the pane has no text at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The caret's offset, when the cursor is on the glass.
    #[must_use]
    pub fn caret(&self) -> Option<u32> {
        self.caret
    }

    /// The selection, as two offsets in order.
    #[must_use]
    pub fn selection(&self) -> Option<(u32, u32)> {
        self.selection
    }

    /// The offset of a cell position: the unit covering the column, or the row's end past it.
    #[must_use]
    pub fn offset_of(&self, point: CellPoint) -> u32 {
        let Some(row) = self.rows.get(point.row as usize) else {
            return self.len();
        };
        let index = row.index_at_column(point.column);
        self.starts[point.row as usize] + u32::try_from(index).unwrap_or(u32::MAX)
    }

    /// The row an offset belongs to: the last row whose start is at or before it.
    fn row_of(&self, offset: u32) -> usize {
        let rows = self.rows.len();
        if rows == 0 {
            return 0;
        }
        let index = self.starts[..rows].partition_point(|start| *start <= offset);
        index.saturating_sub(1)
    }

    fn at(&self, offset: u32) -> At {
        if offset >= self.len() {
            return At::End;
        }
        let row = self.row_of(offset);
        let local = (offset - self.starts[row]) as usize;
        if local < self.rows[row].units.len() {
            At::Cell { row, unit: local }
        } else {
            At::Break { row }
        }
    }

    /// Clamp an offset another publication made into this one.
    #[must_use]
    pub fn clamp(&self, offset: u32) -> u32 {
        offset.min(self.len())
    }

    /// **Whether `offset` is a boundary of `unit`.** The document's two ends are boundaries of
    /// every unit.
    #[must_use]
    pub fn is_boundary(&self, offset: u32, unit: Boundary) -> bool {
        if offset == 0 || offset >= self.len() {
            return true;
        }
        match unit {
            Boundary::Character => true,
            Boundary::Document => false,
            Boundary::Line => self.starts[..self.rows.len()].contains(&offset),
            Boundary::Paragraph => {
                let row = self.row_of(offset);
                self.starts[row] == offset && (row == 0 || !self.rows[row - 1].continues)
            }
            Boundary::Word => match (self.at(offset), self.at(offset - 1)) {
                (_, At::Break { .. }) | (At::Break { .. }, _) => true,
                (At::Cell { row, unit }, _) => self.rows[row].units[unit].word_start,
                (At::End, _) => true,
            },
        }
    }

    /// The first boundary after `offset`, or `None` at the end.
    #[must_use]
    pub fn next_boundary(&self, offset: u32, unit: Boundary) -> Option<u32> {
        let len = self.len();
        if offset >= len {
            return None;
        }
        match unit {
            Boundary::Document => Some(len),
            Boundary::Line | Boundary::Paragraph => {
                let row = self.row_of(offset);
                (row + 1..self.rows.len())
                    .map(|next| self.starts[next])
                    .find(|start| self.is_boundary(*start, unit))
                    .or(Some(len))
            }
            Boundary::Character | Boundary::Word => {
                (offset + 1..=len).find(|next| self.is_boundary(*next, unit))
            }
        }
    }

    /// The last boundary before `offset`, or `None` at the start.
    #[must_use]
    pub fn previous_boundary(&self, offset: u32, unit: Boundary) -> Option<u32> {
        let offset = offset.min(self.len());
        if offset == 0 {
            return None;
        }
        match unit {
            Boundary::Document => Some(0),
            Boundary::Line | Boundary::Paragraph => {
                let row = self.row_of(offset - 1);
                (0..=row)
                    .rev()
                    .map(|earlier| self.starts[earlier])
                    .find(|start| *start < offset && self.is_boundary(*start, unit))
                    .or(Some(0))
            }
            Boundary::Character | Boundary::Word => (0..offset)
                .rev()
                .find(|earlier| self.is_boundary(*earlier, unit)),
        }
    }

    /// **The unit enclosing `offset`** (`ExpandToEnclosingUnit`): from the boundary at or before
    /// it to the next one. At the document's end, the last unit — unless the last row is empty:
    /// its start *is* the document's end, so every unit but the document answers the empty range
    /// there (a caret on an empty last row reads as that row, never as the row above it).
    #[must_use]
    pub fn enclosing(&self, offset: u32, unit: Boundary) -> (u32, u32) {
        let len = self.len();
        if len == 0 {
            return (0, 0);
        }
        let offset = offset.min(len);
        let empty_last_row = self.rows.last().is_some_and(|row| row.units.is_empty());
        if offset == len && empty_last_row && unit != Boundary::Document {
            return (len, len);
        }
        let start = if offset < len && self.is_boundary(offset, unit) {
            offset
        } else {
            self.previous_boundary(offset, unit).unwrap_or(0)
        };
        let end = self.next_boundary(start, unit).unwrap_or(len);
        (start, end)
    }

    /// **An offset moved by `count` boundaries of `unit`**, and how many it really moved
    /// (`Move`, `MoveEndpointByUnit`). A count past either end stops there.
    #[must_use]
    pub fn moved(&self, offset: u32, unit: Boundary, count: i32) -> (u32, i32) {
        let mut at = offset.min(self.len());
        let mut moved = 0;
        while moved != count {
            let next = if count > 0 {
                self.next_boundary(at, unit)
            } else {
                self.previous_boundary(at, unit)
            };
            let Some(next) = next else { break };
            at = next;
            moved += count.signum();
        }
        (at, moved)
    }

    /// **A range moved by `count` units** (`ITextRangeProvider::Move`): a degenerate range moves
    /// its one point; any other is first made one whole unit at its start, then moved a whole
    /// unit at a time, and stays one unit long. The document's end starts no unit, so a forward
    /// move never lands there.
    #[must_use]
    pub fn moved_range(&self, start: u32, end: u32, unit: Boundary, count: i32) -> (u32, u32, i32) {
        if start == end {
            let (at, moved) = self.moved(start, unit, count);
            return (at, at, moved);
        }
        let len = self.len();
        let (mut at, _) = self.enclosing(start, unit);
        let mut moved = 0;
        while moved != count {
            let next = if count > 0 {
                self.next_boundary(at, unit).filter(|next| *next < len)
            } else {
                self.previous_boundary(at, unit)
            };
            let Some(next) = next else { break };
            at = next;
            moved += count.signum();
        }
        (at, self.next_boundary(at, unit).unwrap_or(len), moved)
    }

    /// **The text between two offsets**: each unit's cluster, and `"\r\n"` for each line break.
    /// `max` counts UTF-16 code units, as `GetText`'s `maxLength` does; negative is no limit.
    #[must_use]
    pub fn text(&self, start: u32, end: u32, max: i32) -> String {
        let mut text = String::new();
        let mut room = usize::try_from(max).unwrap_or(usize::MAX);
        for offset in start.min(self.len())..end.min(self.len()) {
            let piece = match self.at(offset) {
                At::Cell { row, unit } => self.rows[row].unit_text(unit),
                At::Break { .. } => "\r\n",
                At::End => break,
            };
            for character in piece.chars() {
                let width = character.len_utf16();
                if width > room {
                    return text;
                }
                room -= width;
                text.push(character);
            }
        }
        text
    }

    /// **Where a range is drawn**: one rectangle per row it covers, in client pixels, each clipped
    /// to the pane's body (a partly drawn row at the body's edge, or a row of the overscan, never
    /// reaches outside the pane). A degenerate range is a one-pixel-wide rectangle at its point,
    /// held inside the body's columns.
    #[must_use]
    pub fn rectangles(&self, start: u32, end: u32) -> Vec<ClientRect> {
        let (start, end) = (start.min(self.len()), end.min(self.len()));
        if self.rows.is_empty() {
            return Vec::new();
        }
        if start == end {
            let row = self.row_of(start);
            let row_text = &self.rows[row];
            let local = (start.saturating_sub(self.starts[row]) as usize).min(row_text.units.len());
            let right_edge = (self.body.left + self.body.width - 1.0).max(self.body.left);
            let point = ClientRect {
                left: row_text.unit_left(local).clamp(self.body.left, right_edge),
                top: row_text.top,
                width: 1.0,
                height: row_text.height,
            };
            return point.clipped(&self.body).into_iter().collect();
        }
        let mut rectangles = Vec::new();
        for row in self.row_of(start)..=self.row_of(end - 1) {
            let row_text = &self.rows[row];
            let row_start = self.starts[row];
            let units = row_text.units.len() as u32;
            let first = start.saturating_sub(row_start).min(units);
            let last = (end - row_start).min(units);
            if first >= last {
                continue;
            }
            let left = row_text.unit_left(first as usize);
            let right = row_text.unit_left(last as usize);
            let drawn = ClientRect {
                left,
                top: row_text.top,
                width: right - left,
                height: row_text.height,
            };
            rectangles.extend(drawn.clipped(&self.body));
        }
        rectangles
    }

    /// **The offset under a client point** (`RangeFromPoint`): the row whose band holds it (the
    /// nearest row above or below the text otherwise), then the unit whose cell holds it, the
    /// row's end past its last unit.
    #[must_use]
    pub fn offset_at(&self, x: f64, y: f64) -> u32 {
        if self.rows.is_empty() {
            return 0;
        }
        let row = self
            .rows
            .iter()
            .position(|row| y < row.top + row.height)
            .unwrap_or(self.rows.len() - 1);
        let row_text = &self.rows[row];
        let column = if row_text.cell_width > 0.0 {
            ((x - row_text.left) / row_text.cell_width).floor().max(0.0)
        } else {
            0.0
        };
        // An f64 of a column on the glass: a terminal grid is far narrower than `u32::MAX`.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let column = column as u32;
        self.starts[row] + row_text.index_at_column(column) as u32
    }

    /// **The first match of `needle` within `start..end`** (`FindText`), as two offsets.
    #[must_use]
    pub fn find(
        &self,
        start: u32,
        end: u32,
        needle: &str,
        backward: bool,
        ignore_case: bool,
    ) -> Option<(u32, u32)> {
        if needle.is_empty() {
            return None;
        }
        let fold = |text: &str| -> Vec<char> {
            if ignore_case {
                text.chars().flat_map(char::to_lowercase).collect()
            } else {
                text.chars().collect()
            }
        };
        // Each offset's characters, folded, and the offset every folded character came from.
        let mut haystack: Vec<char> = Vec::new();
        let mut origin: Vec<u32> = Vec::new();
        for offset in start.min(self.len())..end.min(self.len()) {
            let piece = self.text(offset, offset + 1, -1);
            for character in fold(&piece) {
                haystack.push(character);
                origin.push(offset);
            }
        }
        let needle = fold(needle);
        if needle.len() > haystack.len() {
            return None;
        }
        let matches = |at: usize| haystack[at..at + needle.len()] == needle[..];
        let positions = 0..=haystack.len() - needle.len();
        let found = if backward {
            positions.rev().find(|at| matches(*at))
        } else {
            positions.into_iter().find(|at| matches(*at))
        }?;
        let first = origin[found];
        let last = origin[found + needle.len() - 1] + 1;
        Some((first, last))
    }
}

/// **What a terminal pane says it is** — the one choice the owner's experiment (E-1) settles.
///
/// Windows Terminal's pane says [`Shape::Text`] with the Text pattern
/// (`TermControlAutomationPeer`: `AutomationControlType::Text` and `ITextProvider`); the touch
/// keyboard and a phone remote decide "a text field" from the control type, so the experiment
/// tries the three a terminal could honestly be.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Shape {
    /// An edit control: the Text pattern and the Value pattern, not read-only.
    Edit,
    /// A document: the Text pattern alone.
    Document,
    /// Text, as Windows Terminal's pane is: the Text pattern alone.
    #[default]
    Text,
}

impl Shape {
    /// UI Automation's control type id (`UIA_EditControlTypeId` 50004, `UIA_DocumentControlTypeId`
    /// 50030, `UIA_TextControlTypeId` 50020).
    #[must_use]
    pub fn control_type_id(self) -> i32 {
        match self {
            Self::Edit => 50_004,
            Self::Document => 50_030,
            Self::Text => 50_020,
        }
    }

    /// Whether the pane answers the Value pattern too (an edit control is expected to).
    #[must_use]
    pub fn has_value_pattern(self) -> bool {
        matches!(self, Self::Edit)
    }
}

#[cfg(windows)]
#[path = "uia_windows.rs"]
mod imp;
#[cfg(windows)]
pub use imp::{UiaEvents, UiaHook};

/// **Which thread a client's first call came in on, beside the window's own** (the review's F3,
/// measured on the E-1 build): the providers say `ProviderOptions_UseComThreading` and are
/// free-threaded objects, so a call is expected on one of UI Automation's threads and never on
/// the window's; the application says the two numbers once in `diagnostics.log`, so the owner's
/// run shows which it was.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientThreads {
    pub window: u32,
    pub first_call: u32,
}

/// `WM_GETOBJECT`, as `WinUser.h` numbers it. Written out so that [`asks_for_the_root`] is the
/// same function on every host; the Windows test holds it against the `windows` crate's constant.
pub const WM_GETOBJECT: u32 = 0x003D;

/// `UiaRootObjectId`, as `UIAutomationCoreApi.h` numbers it (see [`WM_GETOBJECT`]).
pub const UIA_ROOT_OBJECT_ID: i32 = -25;

/// **Whether a window message is UI Automation asking for the window's root provider.** The
/// object id travels in `lParam`'s low 32 bits; a 64-bit `lParam` carries it sign-extended or not
/// depending on the caller, so only those bits are read.
#[must_use]
pub fn asks_for_the_root(message: u32, lparam: isize) -> bool {
    // The low 32 bits are the object id by `WM_GETOBJECT`'s definition.
    #[allow(clippy::cast_possible_truncation)]
    let object = lparam as i32;
    message == WM_GETOBJECT && object == UIA_ROOT_OBJECT_ID
}

/// **Off Windows, a hook that is never asked** (the Mac mirror is NSAccessibility, a 0.5 row):
/// constructible, never wanted, so the application builds nothing for it.
#[cfg(not(windows))]
pub struct UiaHook {
    _nothing: (),
}

#[cfg(not(windows))]
impl UiaHook {
    /// Constructs, subscribes to nothing, never calls `asked`.
    ///
    /// # Errors
    /// None; the signature is the Windows arm's.
    pub fn install(
        window: crate::NativeWindow,
        shape: Shape,
        asked: Box<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        let _ = (window, shape, asked);
        Ok(Self { _nothing: () })
    }

    /// Never: nothing off Windows asks.
    #[must_use]
    pub fn wanted(&self) -> bool {
        false
    }

    /// Nothing is published where nothing asks.
    pub fn publish(&self, text: WindowText) -> Option<UiaEvents> {
        let _ = text;
        None
    }

    /// Nothing asks for a pane's focus.
    #[must_use]
    pub fn take_focus_request(&self) -> Option<PaneId> {
        None
    }

    /// No client ever calls.
    #[must_use]
    pub fn threads(&self) -> Option<ClientThreads> {
        None
    }
}

/// **Off Windows, no events.**
#[cfg(not(windows))]
pub struct UiaEvents {
    _nothing: (),
}

#[cfg(not(windows))]
impl UiaEvents {
    /// Nothing to raise.
    pub fn raise(self, worker: &crate::admission::WorkerCtx) {
        let _ = worker;
    }
}

#[cfg(test)]
#[path = "uia_tests.rs"]
mod tests;
