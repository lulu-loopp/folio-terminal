use std::{
    num::NonZeroU32,
    sync::{Arc, Mutex, MutexGuard},
};

use alacritty_terminal::{
    Term,
    event::{Event, EventListener},
    grid::Dimensions,
    index::{Column, Line},
    term::{
        Config, ScrollOutCause, ScrollRegionScope, TermMode, TranscriptEvent, TranscriptScreen,
    },
    vte::{Params, Parser, Perform, ansi::Processor},
};
use bt_transcript::CapturedRow;

use crate::cell_capture::{snapshot, to_captured_row};

pub const SCROLLBACK_LINES: usize = 0;

#[derive(Clone, Copy)]
struct GridSize {
    columns: NonZeroU32,
    rows: NonZeroU32,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows.get() as usize
    }

    fn screen_lines(&self) -> usize {
        self.rows.get() as usize
    }

    fn columns(&self) -> usize {
        self.columns.get() as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemovalCause {
    NormalScroll,
    DeleteLines,
    Resize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemovalScreen {
    Primary,
    Alternate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemovalScope {
    FullScreen,
    Partial,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemovalContext {
    pub cause: RemovalCause,
    pub screen: RemovalScreen,
    pub scope: RemovalScope,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemovedLiveRow {
    pub live_row: u32,
    pub row: CapturedRow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalCursor {
    pub row: u32,
    pub column: u32,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseTracking {
    Off,
    Click,
    Drag,
    Motion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalModes {
    pub alternate_screen: bool,
    pub alternate_scroll: bool,
    pub sgr_mouse: bool,
    pub mouse_tracking: MouseTracking,
}

/// Facts emitted by the alacritty compatibility seam. DESIGN.md §3.1 policy is intentionally
/// absent: every removed row carries its cause, screen, scope, and stable captured cells so the
/// lifecycle layer can decide whether the transcript changes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdapterEvent {
    RowsRemoved {
        context: RemovalContext,
        rows: Vec<RemovedLiveRow>,
    },
    ClearHistory,
    Reset,
    Deccolm,
    PrimaryParked,
    PrimaryRestored,
}

#[derive(Clone, Default)]
pub(crate) struct CaptureListener {
    transcript_events: Arc<Mutex<Vec<TranscriptEvent>>>,
    pty_writes: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl EventListener for CaptureListener {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event {
            self.pty_writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(text.into_bytes());
        }
    }
}

fn lock_events(listener: &CaptureListener) -> MutexGuard<'_, Vec<TranscriptEvent>> {
    listener
        .transcript_events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn install_transcript_hook(term: &mut Term<CaptureListener>, listener: &CaptureListener) {
    let transcript_events = listener.transcript_events.clone();
    term.set_transcript_hook(Some(Arc::new(move |event| {
        transcript_events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(event);
    })));
}

fn discard_listener_output(listener: &CaptureListener) {
    lock_events(listener).clear();
    listener
        .pty_writes
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

/// Vendor-facing terminal adapter. It translates upstream facts into stable BetterTerminal facts
/// and never owns or mutates the canonical transcript.
pub struct TerminalAdapter {
    term: Term<CaptureListener>,
    processor: Processor,
    listener: CaptureListener,
    parser_boundary: Parser,
    parser_tail: Vec<u8>,
    parser_sync_active: bool,
    parser_dcs_active: bool,
    resize_canonical: Option<ResizeCanonical>,
    columns: NonZeroU32,
    rows: NonZeroU32,
}

struct ResizeCanonical {
    term: Term<CaptureListener>,
    processor: Processor,
    listener: CaptureListener,
}

#[derive(Default)]
struct BoundaryPerformer {
    complete: bool,
    execute_at_ground: bool,
    sync_start: bool,
    sync_end: bool,
    dcs_hook: bool,
    dcs_put: bool,
}

impl Perform for BoundaryPerformer {
    fn print(&mut self, _character: char) {
        self.complete = true;
    }

    fn execute(&mut self, byte: u8) {
        self.complete = self.execute_at_ground || matches!(byte, 0x18 | 0x1a);
    }

    fn unhook(&mut self) {
        self.complete = true;
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {
        self.dcs_hook = true;
    }

    fn put(&mut self, _byte: u8) {
        self.dcs_put = true;
    }

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {
        self.complete = true;
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        self.complete = true;
        let sync_mode = intermediates == b"?"
            && params
                .iter()
                .next()
                .is_some_and(|parameter| parameter == [2026]);
        self.sync_start = sync_mode && action == 'h';
        self.sync_end = sync_mode && action == 'l';
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {
        self.complete = true;
    }
}

impl TerminalAdapter {
    pub fn new(columns: NonZeroU32, rows: NonZeroU32) -> Self {
        let config = Config {
            scrolling_history: SCROLLBACK_LINES,
            ..Config::default()
        };
        let size = GridSize { columns, rows };
        let listener = CaptureListener::default();
        let mut term = Term::new(config, &size, listener.clone());
        install_transcript_hook(&mut term, &listener);
        Self {
            term,
            processor: Processor::new(),
            listener,
            parser_boundary: Parser::new(),
            parser_tail: Vec::new(),
            parser_sync_active: false,
            parser_dcs_active: false,
            resize_canonical: None,
            columns,
            rows,
        }
    }

    pub fn alacritty_history_size(&self) -> usize {
        self.term.grid().history_size()
    }

    pub fn dimensions(&self) -> (NonZeroU32, NonZeroU32) {
        (self.columns, self.rows)
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<AdapterEvent> {
        self.processor.advance(&mut self.term, bytes);
        if let Some(canonical) = self.resize_canonical.as_mut() {
            canonical.processor.advance(&mut canonical.term, bytes);
            discard_listener_output(&canonical.listener);
        }
        self.observe_parser_boundary(bytes);
        self.drain_transcript_events()
    }

    pub fn resize(&mut self, columns: NonZeroU32, rows: NonZeroU32) -> Vec<AdapterEvent> {
        self.term.resize(GridSize { columns, rows });
        self.columns = columns;
        self.rows = rows;
        self.drain_transcript_events()
    }

    pub fn begin_resize_transaction(&mut self) -> usize {
        if self.resize_canonical.is_some() {
            return 0;
        }

        let restored = self.term.begin_resize_transaction();
        let listener = CaptureListener::default();
        let mut term = self.term.fork(listener.clone());
        install_transcript_hook(&mut term, &listener);

        // A transaction can begin between two bytes of a CSI/OSC/DCS/UTF-8 sequence or while a
        // synchronized update is buffered. Seed a fresh processor with that exact uncommitted raw
        // tail against a disposable fork; the canonical term already contains every committed
        // semantic action and must not receive the tail twice.
        let seed_listener = CaptureListener::default();
        let mut seed_term = self.term.fork(seed_listener);
        let mut processor = Processor::new();
        processor.advance(&mut seed_term, &self.parser_tail);

        self.resize_canonical = Some(ResizeCanonical {
            term,
            processor,
            listener,
        });
        restored
    }

    pub fn finish_resize_transaction(&mut self) -> Vec<CapturedRow> {
        // The normal final-size commit consumes the canonical branch first. This fallback only
        // covers callers which abort a transaction without committing a pseudoconsole resize.
        self.resize_canonical = None;
        self.term
            .finish_resize_transaction()
            .iter()
            .map(|row| to_captured_row(&row[..]))
            .collect()
    }

    pub fn clear_resize_transaction_history(&mut self) {
        self.term.clear_resize_transaction_history();
        if let Some(canonical) = self.resize_canonical.as_mut() {
            canonical.term.clear_resize_transaction_history();
        }
    }

    pub fn resize_transaction_history_size(&self) -> usize {
        self.term.resize_transaction_history_size()
    }

    pub fn retain_resize_staging_candidate_rows(&mut self, rows: usize) {
        self.term.retain_resize_staging_candidate_rows(rows);
    }

    pub fn resize_staging_candidate_rows(&self) -> usize {
        self.term.resize_staging_candidate_rows()
    }

    pub fn reconcile_resize_transaction_to_viewport(&mut self) -> (usize, usize) {
        let history_before = self.term.resize_transaction_history_size();
        let Some(mut canonical) = self.resize_canonical.take() else {
            return self.term.reconcile_resize_transaction_to_viewport();
        };

        canonical.term.resize(GridSize {
            columns: self.columns,
            rows: self.rows,
        });
        discard_listener_output(&canonical.listener);
        let (_, history_after) = canonical.term.reconcile_resize_transaction_to_viewport();
        discard_listener_output(&canonical.listener);

        // Only the displayed branch can own replies. Preserve any reply queued immediately before
        // the atomic branch replacement; the canonical parser's duplicate replies were discarded.
        let pending_writes = std::mem::take(
            &mut *self
                .listener
                .pty_writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        canonical
            .listener
            .pty_writes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extend(pending_writes);

        self.term = canonical.term;
        self.processor = canonical.processor;
        self.listener = canonical.listener;
        (history_before, history_after)
    }

    pub fn visible_text(&self) -> Vec<String> {
        snapshot(&self.term)
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.c)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    pub fn visible_row(&self, row: u32) -> Option<CapturedRow> {
        (row < self.rows.get()).then(|| {
            let cells = (0..self.columns.get())
                .map(|column| self.term.grid()[Line(row as i32)][Column(column as usize)].clone())
                .collect::<Vec<_>>();
            to_captured_row(&cells)
        })
    }

    pub fn cursor(&self) -> TerminalCursor {
        let point = self.term.grid().cursor.point;
        TerminalCursor {
            row: point.line.0.max(0) as u32,
            column: point.column.0 as u32,
            visible: self.term.mode().contains(TermMode::SHOW_CURSOR),
        }
    }

    /// Read DEC private modes from the vendor terminal, the single protocol-state authority.
    pub fn application_cursor_mode(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    pub fn bracketed_paste_mode(&self) -> bool {
        self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    pub fn modes(&self) -> TerminalModes {
        let mode = self.term.mode();
        let mouse_tracking = if mode.contains(TermMode::MOUSE_MOTION) {
            MouseTracking::Motion
        } else if mode.contains(TermMode::MOUSE_DRAG) {
            MouseTracking::Drag
        } else if mode.contains(TermMode::MOUSE_REPORT_CLICK) {
            MouseTracking::Click
        } else {
            MouseTracking::Off
        };
        TerminalModes {
            alternate_screen: mode.contains(TermMode::ALT_SCREEN),
            alternate_scroll: mode.contains(TermMode::ALTERNATE_SCROLL),
            sgr_mouse: mode.contains(TermMode::SGR_MOUSE),
            mouse_tracking,
        }
    }

    /// Drain protocol replies generated by the terminal state machine (for example DSR).
    pub fn take_pty_writes(&self) -> Vec<Vec<u8>> {
        std::mem::take(
            &mut *self
                .listener
                .pty_writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }

    fn drain_transcript_events(&mut self) -> Vec<AdapterEvent> {
        let transcript_events = std::mem::take(&mut *lock_events(&self.listener));
        transcript_events
            .into_iter()
            .map(|event| match event {
                TranscriptEvent::ScrollOut { cause, rows } => AdapterEvent::RowsRemoved {
                    context: removal_context(cause),
                    rows: rows
                        .into_iter()
                        .map(|row| RemovedLiveRow {
                            live_row: row.live_row as u32,
                            row: to_captured_row(&row.cells),
                        })
                        .collect(),
                },
                TranscriptEvent::ClearHistory => AdapterEvent::ClearHistory,
                TranscriptEvent::Reset => AdapterEvent::Reset,
                TranscriptEvent::Deccolm => AdapterEvent::Deccolm,
                TranscriptEvent::PrimaryParked => AdapterEvent::PrimaryParked,
                TranscriptEvent::PrimaryRestored => AdapterEvent::PrimaryRestored,
            })
            .collect()
    }

    fn observe_parser_boundary(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            let execute_at_ground = self.parser_tail.is_empty() && !self.parser_sync_active;
            self.parser_tail.push(byte);
            let mut performer = BoundaryPerformer {
                execute_at_ground,
                ..BoundaryPerformer::default()
            };
            self.parser_boundary
                .advance(&mut performer, std::slice::from_ref(&byte));

            if performer.dcs_hook {
                self.parser_dcs_active = true;
            } else if performer.dcs_put && self.parser_dcs_active {
                // Once the DCS hook has selected its handler, payload bytes do not affect parser
                // state. The disposable seed term must only replay the introducer, not retain an
                // unbounded sixel/image payload.
                self.parser_tail.pop();
            }

            if performer.sync_start {
                self.parser_sync_active = true;
            } else if performer.sync_end {
                self.parser_sync_active = false;
                self.parser_tail.clear();
            } else if performer.complete && !self.parser_sync_active {
                self.parser_dcs_active = false;
                self.parser_tail.clear();
                // ESC can terminate OSC/DCS while simultaneously starting the ST escape. Keep it
                // as the seed for the parser's new Escape state.
                if byte == 0x1b {
                    self.parser_tail.push(byte);
                }
            }
        }
    }
}

fn removal_context(cause: ScrollOutCause) -> RemovalContext {
    let stable_screen = |screen| match screen {
        TranscriptScreen::Primary => RemovalScreen::Primary,
        TranscriptScreen::Alternate => RemovalScreen::Alternate,
    };
    let stable_scope = |scope| match scope {
        ScrollRegionScope::FullScreen => RemovalScope::FullScreen,
        ScrollRegionScope::Partial => RemovalScope::Partial,
    };
    match cause {
        ScrollOutCause::Normal { screen, scope } => RemovalContext {
            cause: RemovalCause::NormalScroll,
            screen: stable_screen(screen),
            scope: stable_scope(scope),
        },
        ScrollOutCause::DeleteLines { screen, scope } => RemovalContext {
            cause: RemovalCause::DeleteLines,
            screen: stable_screen(screen),
            scope: stable_scope(scope),
        },
        ScrollOutCause::Resize => RemovalContext {
            cause: RemovalCause::Resize,
            screen: RemovalScreen::Primary,
            scope: RemovalScope::FullScreen,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nz(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).unwrap()
    }

    fn apply_r2_extreme_resize_trace(terminal: &mut TerminalAdapter) {
        let sizes = [
            (95, 24),
            (62, 16),
            (48, 12),
            (38, 10),
            (28, 8),
            (25, 8),
            (23, 7),
            (20, 7),
            (18, 6),
            (16, 6),
            (16, 5),
            (16, 6),
            (19, 7),
            (23, 8),
            (26, 8),
            (29, 9),
            (36, 11),
            (41, 12),
            (46, 14),
            (49, 14),
            (52, 15),
            (56, 16),
            (61, 17),
            (66, 17),
            (71, 18),
            (77, 20),
            (85, 21),
            (78, 19),
            (66, 17),
            (60, 16),
            (52, 14),
            (25, 9),
            (13, 6),
            (12, 6),
            (15, 7),
            (33, 10),
            (51, 13),
            (60, 15),
            (61, 15),
            (47, 12),
            (23, 7),
            (11, 3),
            (11, 4),
            (11, 6),
            (22, 8),
            (30, 9),
            (34, 10),
            (37, 10),
            (38, 10),
            (38, 11),
            (38, 10),
            (26, 8),
            (12, 6),
            (11, 5),
            (13, 6),
            (35, 10),
            (46, 12),
            (49, 12),
            (43, 10),
            (39, 10),
            (38, 10),
            (37, 10),
            (35, 9),
            (34, 9),
            (31, 9),
            (30, 9),
        ];
        for (columns, rows) in sizes {
            terminal.resize(nz(columns), nz(rows));
        }
    }

    fn removed_context(events: &[AdapterEvent]) -> Option<RemovalContext> {
        events.iter().find_map(|event| match event {
            AdapterEvent::RowsRemoved { context, .. } => Some(*context),
            _ => None,
        })
    }

    #[test]
    fn full_screen_scroll_reports_cells_without_owning_history() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(2));
        let events = terminal.feed(b"one\r\ntwo\r\nthree");
        assert_eq!(terminal.alacritty_history_size(), 0);
        assert_eq!(
            removed_context(&events),
            Some(RemovalContext {
                cause: RemovalCause::NormalScroll,
                screen: RemovalScreen::Primary,
                scope: RemovalScope::FullScreen,
            })
        );
        assert!(matches!(
            &events[0],
            AdapterEvent::RowsRemoved { rows, .. }
                if rows.first().is_some_and(|row| row.row.cells[0].text == "o")
        ));
    }

    #[test]
    fn local_scroll_delete_lines_and_alt_screen_are_reported_as_facts() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(4));
        terminal.feed(b"a\r\nb\r\nc\r\nd");
        let local = terminal.feed(b"\x1b[2;3r\x1b[3;1H\n");
        assert_eq!(
            removed_context(&local),
            Some(RemovalContext {
                cause: RemovalCause::NormalScroll,
                screen: RemovalScreen::Primary,
                scope: RemovalScope::Partial,
            })
        );

        let deleted = terminal.feed(b"\x1b[r\x1b[1;1H\x1b[1M");
        assert_eq!(
            removed_context(&deleted),
            Some(RemovalContext {
                cause: RemovalCause::DeleteLines,
                screen: RemovalScreen::Primary,
                scope: RemovalScope::FullScreen,
            })
        );

        terminal.feed(b"\x1b[?1049h");
        let alternate = terminal.feed(b"1\r\n2\r\n3\r\n4\r\n5");
        assert_eq!(
            removed_context(&alternate),
            Some(RemovalContext {
                cause: RemovalCause::NormalScroll,
                screen: RemovalScreen::Alternate,
                scope: RemovalScope::FullScreen,
            })
        );
    }

    #[test]
    fn explicit_screen_scroll_is_not_a_transcript_removal_fact() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(4));
        terminal.feed(b"a\r\nb\r\nc\r\nd");

        assert!(terminal.feed(b"\x1b[S").is_empty());
        assert_eq!(terminal.visible_text(), ["b", "c", "d", ""]);

        // LF at the bottom remains output scroll and still carries exact removed cells.
        let output = terminal.feed(b"\x1b[4;1H\n");
        assert_eq!(
            removed_context(&output),
            Some(RemovalContext {
                cause: RemovalCause::NormalScroll,
                screen: RemovalScreen::Primary,
                scope: RemovalScope::FullScreen,
            })
        );
    }

    #[test]
    fn oversized_delete_lines_reports_only_rows_inside_the_remaining_region() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(4));
        terminal.feed(b"a\r\nb\r\nc\r\nd");
        let events = terminal.feed(b"\x1b[4;1H\x1b[999M");
        let rows = events.iter().find_map(|event| match event {
            AdapterEvent::RowsRemoved { rows, .. } => Some(rows),
            _ => None,
        });
        assert_eq!(rows.map(Vec::len), Some(1));
    }

    #[test]
    fn resize_transaction_vendor_history_is_the_only_mutable_tail_owner() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(4));
        terminal.feed(b"a\r\nb\r\nc\r\nd");
        terminal.begin_resize_transaction();
        let events = terminal.resize(nz(8), nz(2));
        assert!(matches!(&events[0], AdapterEvent::RowsRemoved { rows, .. } if rows.len() == 2));
        assert_eq!(terminal.resize_transaction_history_size(), 2);

        terminal.resize(nz(8), nz(4));
        assert!(terminal.finish_resize_transaction().is_empty());
        assert_eq!(terminal.visible_text(), vec!["a", "b", "c", "d"]);
        assert_eq!(terminal.alacritty_history_size(), 0);
    }

    #[test]
    fn r2_extreme_local_path_reconciles_to_the_coalesced_conpty_viewport() {
        const WARNING: &str = "Did not find path entry D:\\App\\Base\\anaconda3\\bin";
        const PROMPT: &str = "(base) PS D:\\Developer\\BetterTerminal> ";
        let input = format!("{WARNING}\r\n{PROMPT}");

        let mut direct = TerminalAdapter::new(nz(104), nz(26));
        direct.feed(input.as_bytes());
        direct.begin_resize_transaction();
        direct.resize(nz(30), nz(9));
        assert_eq!(direct.reconcile_resize_transaction_to_viewport(), (2, 0));
        let direct_rows = direct.visible_text();
        let direct_cursor = direct.cursor();
        assert_eq!((direct_cursor.row, direct_cursor.column), (3, 9));

        let mut unreconciled = TerminalAdapter::new(nz(104), nz(26));
        unreconciled.feed(input.as_bytes());
        unreconciled.begin_resize_transaction();
        apply_r2_extreme_resize_trace(&mut unreconciled);
        assert_eq!(unreconciled.resize_transaction_history_size(), 2);
        let harvested = unreconciled.finish_resize_transaction();
        let harvested_text = harvested
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .filter(|cell| !cell.wide_spacer)
                    .map(|cell| cell.text.as_str())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            harvested_text,
            ["Did not find path entry D:\\App", "\\Base\\anaconda3\\bin",]
        );

        let mut reconciled = TerminalAdapter::new(nz(104), nz(26));
        reconciled.feed(input.as_bytes());
        reconciled.begin_resize_transaction();
        apply_r2_extreme_resize_trace(&mut reconciled);
        assert_eq!(
            reconciled.reconcile_resize_transaction_to_viewport(),
            (2, 0)
        );
        assert_eq!(reconciled.visible_text(), direct_rows);
        assert_eq!(reconciled.cursor(), direct_cursor);
        assert!(reconciled.finish_resize_transaction().is_empty());
    }

    #[test]
    fn coalesced_final_resize_replaces_the_path_dependent_live_branch() {
        let sizes = [
            (111, 20),
            (46, 7),
            (12, 1),
            (13, 2),
            (28, 7),
            (71, 15),
            (79, 16),
            (66, 14),
            (22, 7),
            (18, 6),
            (42, 12),
            (98, 21),
            (60, 14),
            (16, 6),
            (27, 9),
            (79, 17),
            (89, 19),
            (85, 18),
            (25, 7),
            (19, 7),
            (51, 11),
            (90, 16),
            (53, 11),
            (11, 5),
            (42, 10),
            (86, 15),
            (85, 15),
            (49, 10),
            (31, 8),
            (64, 13),
            (104, 18),
            (99, 17),
            (46, 10),
            (38, 9),
            (59, 14),
            (117, 21),
            (118, 21),
            (72, 13),
            (33, 9),
            (39, 10),
            (79, 18),
            (92, 20),
            (95, 20),
            (96, 20),
        ];
        // Deterministic reduction of the S12 mix: soft wraps, CUP, save/restore, erase, and cursor
        // visibility. The transient storm finishes with no native history, but its cursor is still
        // path-dependent; this is exactly the branch the old history-only reconcile skipped.
        let mut state = 10u64;
        let mut input = String::new();
        for _ in 0..48 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let row = 1 + ((state >> 8) % 22);
            let column = 1 + ((state >> 16) % 118);
            let length = 1 + ((state >> 24) % 180) as usize;
            match state % 9 {
                0 => input.push_str(&format!("\x1b[{row};{column}H")),
                1 => input.push_str("\r\n"),
                2 => input.push_str("\x1b[2K"),
                3 => input.push_str("\x1b[K"),
                4 => input.push_str("\x1b7"),
                5 => input.push_str("\x1b8"),
                6 => input.push_str(&"q".repeat(length)),
                7 => input.push_str(&format!("\x1b[93m{}\x1b[0m", "p".repeat(length))),
                _ => input.push_str("\x1b[?25l\x1b[?25h"),
            }
        }

        let mut direct = TerminalAdapter::new(nz(119), nz(23));
        direct.feed(input.as_bytes());
        direct.begin_resize_transaction();
        direct.resize(nz(96), nz(20));
        direct.reconcile_resize_transaction_to_viewport();
        let direct_cursor = direct.cursor();
        let direct_rows = direct.visible_text();

        let mut storm = TerminalAdapter::new(nz(119), nz(23));
        storm.feed(input.as_bytes());
        storm.begin_resize_transaction();
        for (columns, rows) in sizes {
            storm.resize(nz(columns), nz(rows));
        }
        assert_eq!(storm.resize_transaction_history_size(), 0);
        assert_ne!(storm.cursor(), direct_cursor);

        assert_eq!(storm.reconcile_resize_transaction_to_viewport(), (0, 0));
        assert_eq!(storm.cursor(), direct_cursor);
        assert_eq!(storm.visible_text(), direct_rows);
    }

    #[test]
    fn canonical_resize_branch_inherits_a_split_parser_sequence() {
        let mut direct = TerminalAdapter::new(nz(20), nz(4));
        direct.feed(b"prompt> \x1b[");
        direct.begin_resize_transaction();
        direct.feed(b"93mhistory\x1b[0m");
        direct.resize(nz(12), nz(4));
        direct.reconcile_resize_transaction_to_viewport();

        let mut storm = TerminalAdapter::new(nz(20), nz(4));
        storm.feed(b"prompt> \x1b[");
        storm.begin_resize_transaction();
        storm.resize(nz(5), nz(2));
        storm.feed(b"93mhistory\x1b[0m");
        storm.resize(nz(12), nz(4));
        storm.reconcile_resize_transaction_to_viewport();

        assert_eq!(storm.visible_text(), direct.visible_text());
        assert_eq!(storm.cursor(), direct.cursor());
    }

    #[test]
    fn canonical_resize_branch_inherits_a_buffered_synchronized_update() {
        let prefix = b"base\x1b[?2026h\x1b[93mheld";
        let suffix = b"-until-end\x1b[0m\x1b[?2026l";

        let mut direct = TerminalAdapter::new(nz(20), nz(4));
        direct.feed(prefix);
        assert!(!direct.parser_tail.is_empty());
        direct.begin_resize_transaction();
        direct.feed(suffix);
        direct.resize(nz(12), nz(4));
        direct.reconcile_resize_transaction_to_viewport();

        let mut storm = TerminalAdapter::new(nz(20), nz(4));
        storm.feed(prefix);
        storm.begin_resize_transaction();
        storm.resize(nz(5), nz(2));
        storm.feed(suffix);
        storm.resize(nz(12), nz(4));
        storm.reconcile_resize_transaction_to_viewport();

        assert!(storm.parser_tail.is_empty());
        assert_eq!(storm.visible_text(), direct.visible_text());
        assert_eq!(storm.cursor(), direct.cursor());
    }

    #[test]
    fn canonical_parser_seed_does_not_retain_dcs_payload() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(4));
        terminal.feed(b"\x1bPq");
        terminal.feed(&vec![b'x'; 64 * 1024]);

        assert!(terminal.parser_dcs_active);
        assert_eq!(terminal.parser_tail, b"\x1bPq");
        terminal.begin_resize_transaction();
        terminal.resize(nz(12), nz(3));
        terminal.feed(b"\x1b\\done");
        terminal.reconcile_resize_transaction_to_viewport();

        assert!(!terminal.parser_dcs_active);
        assert!(terminal.parser_tail.is_empty());
        assert!(
            terminal
                .visible_text()
                .iter()
                .any(|row| row.contains("done"))
        );
    }

    #[test]
    fn transaction_harvest_preserves_an_internal_user_blank_line() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(6));
        terminal.feed(b"top\r\n\r\nmiddle\r\nlower\r\ntail\r\nend");

        terminal.begin_resize_transaction();
        terminal.resize(nz(8), nz(3));
        let rows = terminal.finish_resize_transaction();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].cells[0].text, "t");
        assert!(rows[1].cells.iter().all(|cell| cell.text.trim().is_empty()));
        assert_eq!(rows[2].cells[0].text, "m");
        assert_eq!(terminal.alacritty_history_size(), 0);
    }

    #[test]
    fn native_transaction_tail_reflows_a_large_hard_line_without_loss() {
        let mut terminal = TerminalAdapter::new(nz(80), nz(104));
        let expected = (0..104)
            .map(|index| format!("{index:03}{}", "x".repeat(76)))
            .collect::<Vec<_>>();
        terminal.feed(expected.join("\r\n").as_bytes());

        terminal.begin_resize_transaction();
        terminal.resize(nz(80), nz(4));
        terminal.resize(nz(30), nz(4));
        terminal.resize(nz(80), nz(104));
        assert!(terminal.finish_resize_transaction().is_empty());

        assert_eq!(terminal.visible_text(), expected);
        assert_eq!(terminal.alacritty_history_size(), 0);
    }

    #[test]
    fn reset_and_deccolm_are_distinct_facts() {
        let mut terminal = TerminalAdapter::new(nz(4), nz(3));
        assert!(terminal.feed(b"\x1bc").contains(&AdapterEvent::Reset));
        assert!(terminal.feed(b"\x1b[?3h").contains(&AdapterEvent::Deccolm));
    }

    #[test]
    fn terminal_protocol_replies_are_exposed_to_the_single_pty_writer() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        terminal.feed(b"\x1b[6n");
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[1;1R".to_vec()]);
        assert!(terminal.take_pty_writes().is_empty());
    }

    #[test]
    fn input_modes_are_read_directly_from_vendor_decset_state() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        assert!(!terminal.application_cursor_mode());
        assert!(!terminal.bracketed_paste_mode());

        terminal.feed(b"\x1b[?1h\x1b[?2004h");
        assert!(terminal.application_cursor_mode());
        assert!(terminal.bracketed_paste_mode());

        terminal.feed(b"\x1b[?1l\x1b[?2004l");
        assert!(!terminal.application_cursor_mode());
        assert!(!terminal.bracketed_paste_mode());

        terminal.feed(b"\x1b[?1049h\x1b[?1007h\x1b[?1002h\x1b[?1006h");
        assert_eq!(
            terminal.modes(),
            TerminalModes {
                alternate_screen: true,
                alternate_scroll: true,
                sgr_mouse: true,
                mouse_tracking: MouseTracking::Drag,
            }
        );
        terminal.feed(b"\x1b[?1003h");
        assert_eq!(terminal.modes().mouse_tracking, MouseTracking::Motion);
        terminal.feed(b"\x1b[?1049l\x1b[?1007l\x1b[?1002l\x1b[?1003l\x1b[?1006l");
        assert_eq!(terminal.modes().mouse_tracking, MouseTracking::Off);
        assert!(!terminal.modes().alternate_screen);
    }

    #[test]
    fn dec_mode_2027_query_set_and_reset_use_standard_decrqm_semantics() {
        let mut terminal = TerminalAdapter::new(nz(20), nz(3));
        terminal.feed(b"\x1b[?2027$p");
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[?2027;2$y".to_vec()]);

        terminal.feed(b"\x1b[?2027h\x1b[?2027$p");
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[?2027;1$y".to_vec()]);

        terminal.feed(b"\x1b[?2027l\x1b[?2027$p");
        assert_eq!(terminal.take_pty_writes(), vec![b"\x1b[?2027;2$y".to_vec()]);
    }

    #[test]
    fn grapheme_mode_clusters_the_m1_width_matrix_while_legacy_mode_stays_compatible() {
        let cases = [
            ("👨‍👩‍👧‍👦", 2),
            ("👍🏽", 2),
            ("e\u{301}", 1),
            ("☂\u{fe0e}", 1),
            ("☂\u{fe0f}", 2),
            ("⌚\u{fe0e}", 1),
            ("🇺🇸", 2),
            ("☆", 1),
        ];
        for (text, expected) in cases {
            let mut terminal = TerminalAdapter::new(nz(20), nz(3));
            terminal.feed(b"\x1b[?2027h");
            for byte in text.as_bytes() {
                terminal.feed(std::slice::from_ref(byte));
            }
            assert_eq!(terminal.cursor().column, expected, "{text:?}");
            let row = terminal.visible_row(0).unwrap();
            assert_eq!(row.cells[0].text, text, "{text:?}");
            assert_eq!(
                row.cells.get(1).is_some_and(|cell| cell.wide_spacer),
                expected == 2
            );
        }

        let mut legacy = TerminalAdapter::new(nz(20), nz(3));
        legacy.feed("👨‍👩‍👧‍👦".as_bytes());
        assert_eq!(legacy.cursor().column, 8);
        legacy.feed(b"\x1b[?2027h\r");
        legacy.feed("👨‍👩‍👧‍👦".as_bytes());
        assert_eq!(legacy.cursor().column, 2);
        legacy.feed(b"\x1b[?2027l\r");
        legacy.feed("👍🏽".as_bytes());
        assert_eq!(legacy.cursor().column, 4);
    }

    #[test]
    fn decawm_margin_pressure_then_decrst_2027_cannot_consume_legacy_text() {
        let mut terminal = TerminalAdapter::new(nz(80), nz(24));
        terminal.feed(b"\x1b[?2027h\x1b[?7l\x1b[999G");
        terminal.feed("☂\u{fe0f}".as_bytes());
        terminal.feed(b"\r\n\x1b[?7hBT_PANIC_SURVIVED\r\n\x1b[?2027l|");
        terminal.feed("👨\u{200d}👩\u{200d}👧\u{200d}👦".as_bytes());
        terminal.feed(b"|");

        let family_row = terminal
            .visible_text()
            .iter()
            .position(|row| row.starts_with('|'))
            .expect("post-DECRST family row remains visible");
        let row = terminal.visible_row(family_row as u32).unwrap();
        assert_eq!(row.cells[0].text, "|");
        assert!(row.cells[1].text.starts_with('👨'));
        assert_eq!(row.cells[9].text, "|");
        assert_eq!(terminal.cursor().column, 10);
        assert!(
            terminal
                .visible_text()
                .iter()
                .any(|row| row.contains("BT_PANIC_SURVIVED"))
        );
    }

    #[test]
    fn mixed_clusters_wrap_as_an_indivisible_wide_lead_and_spacer() {
        let mut terminal = TerminalAdapter::new(nz(4), nz(3));
        terminal.feed(b"\x1b[?2027habc");
        terminal.feed("👨‍👩‍👧‍👦中Z".as_bytes());

        let first = terminal.visible_row(0).unwrap();
        assert_eq!(first.cells[0].text, "a");
        assert_eq!(first.cells[3].text, " ");
        let second = terminal.visible_row(1).unwrap();
        assert_eq!(second.cells[0].text, "👨‍👩‍👧‍👦");
        assert!(
            second.cells[0]
                .style
                .flags
                .contains(bt_transcript::CellFlags::WIDE_CHAR)
        );
        assert!(second.cells[1].wide_spacer);
        assert_eq!(second.cells[2].text, "中");
        assert!(second.cells[3].wide_spacer);
        let third = terminal.visible_row(2).unwrap();
        assert_eq!(third.cells[0].text, "Z");
    }

    #[test]
    fn late_vs_and_flag_width_changes_rewrite_atomically_at_the_right_margin() {
        for text in ["☂\u{fe0f}", "🇺🇸"] {
            let mut terminal = TerminalAdapter::new(nz(4), nz(3));
            terminal.feed(b"\x1b[?2027habc");
            terminal.feed(text.as_bytes());
            assert_eq!(terminal.cursor().row, 1, "{text:?}");
            assert_eq!(terminal.cursor().column, 2, "{text:?}");
            let first = terminal.visible_row(0).unwrap();
            assert_eq!(first.cells[3].text, " ", "{text:?}");
            let second = terminal.visible_row(1).unwrap();
            assert_eq!(second.cells[0].text, text, "{text:?}");
            assert!(second.cells[1].wide_spacer, "{text:?}");
        }

        let mut text_presentation = TerminalAdapter::new(nz(4), nz(3));
        text_presentation.feed(b"\x1b[?2027habc");
        text_presentation.feed("⌚\u{fe0e}".as_bytes());
        assert_eq!(text_presentation.cursor().row, 0);
        assert_eq!(text_presentation.cursor().column, 3);
        let first = text_presentation.visible_row(0).unwrap();
        assert_eq!(first.cells[3].text, "⌚\u{fe0e}");
        assert!(
            !first.cells[3]
                .style
                .flags
                .contains(bt_transcript::CellFlags::WIDE_CHAR)
        );
        let cleared = text_presentation.visible_row(1).unwrap();
        assert!(cleared.cells[0].text.trim().is_empty());
        assert!(!cleared.cells[0].wide_spacer);
    }

    #[test]
    fn late_cluster_width_changes_preserve_insert_mode_tail_cells() {
        let mut upgrade = TerminalAdapter::new(nz(8), nz(2));
        upgrade.feed(b"ABCDE\r\x1b[2C\x1b[4h\x1b[?2027h");
        upgrade.feed("🇺🇸".as_bytes());
        let row = upgrade.visible_row(0).unwrap();
        assert_eq!(row.cells[0].text, "A");
        assert_eq!(row.cells[1].text, "B");
        assert_eq!(row.cells[2].text, "🇺🇸");
        assert!(row.cells[3].wide_spacer);
        assert_eq!(row.cells[4].text, "C");
        assert_eq!(row.cells[5].text, "D");
        assert_eq!(row.cells[6].text, "E");

        let mut shrink = TerminalAdapter::new(nz(8), nz(2));
        shrink.feed(b"ABCDE\r\x1b[2C\x1b[4h\x1b[?2027h");
        shrink.feed("⌚\u{fe0e}".as_bytes());
        let row = shrink.visible_row(0).unwrap();
        assert_eq!(row.cells[2].text, "⌚\u{fe0e}");
        assert!(!row.cells[3].wide_spacer);
        assert_eq!(row.cells[3].text, "C");
        assert_eq!(row.cells[4].text, "D");
        assert_eq!(row.cells[5].text, "E");
    }

    #[test]
    fn resize_never_separates_cluster_text_from_its_wide_spacer() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        terminal.feed(b"\x1b[?2027hA");
        terminal.feed("👨‍👩‍👧‍👦".as_bytes());
        terminal.feed(b"BCDE");
        let events = terminal.resize(nz(8), nz(3));

        let cluster = (0..3).find_map(|row| {
            let row = terminal.visible_row(row)?;
            let column = row.cells.iter().position(|cell| cell.text == "👨‍👩‍👧‍👦")?;
            Some((row, column))
        });
        let (row, column) = cluster.unwrap_or_else(|| {
            panic!(
                "cluster survived resize: {:?}; {events:?}",
                terminal.visible_text()
            )
        });
        assert!(
            row.cells[column]
                .style
                .flags
                .contains(bt_transcript::CellFlags::WIDE_CHAR)
        );
        assert!(row.cells[column + 1].wide_spacer);
    }

    #[test]
    fn resize_between_codepoints_reanchors_the_in_progress_cluster() {
        let mut family = TerminalAdapter::new(nz(4), nz(3));
        family.feed(b"\x1b[?2027hA");
        family.feed("👨‍".as_bytes());
        family.resize(nz(8), nz(3));
        family.feed("👩‍👧‍👦".as_bytes());
        let row = family.visible_row(0).unwrap();
        assert_eq!(row.cells[1].text, "👨‍👩‍👧‍👦");
        assert!(row.cells[2].wide_spacer);
        assert_eq!(family.cursor().column, 3);

        let mut flag = TerminalAdapter::new(nz(4), nz(3));
        flag.feed(b"\x1b[?2027habc");
        flag.feed("🇺".as_bytes());
        flag.resize(nz(8), nz(3));
        flag.feed("🇸".as_bytes());
        let row = flag.visible_row(0).unwrap();
        assert_eq!(row.cells[3].text, "🇺🇸");
        assert!(row.cells[4].wide_spacer);
        assert_eq!(flag.cursor().column, 5);
    }

    #[test]
    fn clear_history_is_only_reported_by_the_vt_ed3_action() {
        for payload in [
            b"\x1b_payload [3J\x1b\\".as_slice(),
            b"\x1bP0;1|[3J\x1b\\".as_slice(),
        ] {
            let mut terminal = TerminalAdapter::new(nz(8), nz(2));
            assert!(!terminal.feed(payload).contains(&AdapterEvent::ClearHistory));
        }

        let mut terminal = TerminalAdapter::new(nz(8), nz(2));
        assert!(
            terminal
                .feed(b"\x1b[3J")
                .contains(&AdapterEvent::ClearHistory)
        );
    }

    #[test]
    fn unterminated_apc_and_dcs_can_resynchronize_on_escape() {
        for introducer in [b"\x1b_stuck".as_slice(), b"\x1bP0;1|stuck".as_slice()] {
            let mut terminal = TerminalAdapter::new(nz(8), nz(2));
            terminal.feed(introducer);
            terminal.feed(b"\x1b[2J\x1b[1;1Hhello");
            assert!(terminal.visible_text().iter().any(|line| line == "hello"));
        }
    }
}
