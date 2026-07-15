use std::{
    num::NonZeroU32,
    sync::{Arc, Mutex, MutexGuard},
};

use alacritty_terminal::{
    Term,
    event::EventListener,
    grid::Dimensions,
    index::{Column, Line},
    term::{Config, ScrollOutCause, ScrollRegionScope, TranscriptEvent, TranscriptScreen},
    vte::ansi::Processor,
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
}

impl EventListener for CaptureListener {}

fn lock_events(listener: &CaptureListener) -> MutexGuard<'_, Vec<TranscriptEvent>> {
    listener
        .transcript_events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Vendor-facing terminal adapter. It translates upstream facts into stable BetterTerminal facts
/// and never owns or mutates the canonical transcript.
pub struct TerminalAdapter {
    term: Term<CaptureListener>,
    processor: Processor,
    listener: CaptureListener,
    columns: NonZeroU32,
    rows: NonZeroU32,
}

impl TerminalAdapter {
    pub fn new(columns: NonZeroU32, rows: NonZeroU32) -> Self {
        let config = Config {
            scrolling_history: SCROLLBACK_LINES,
            ..Config::default()
        };
        let size = GridSize { columns, rows };
        let listener = CaptureListener::default();
        let transcript_events = listener.transcript_events.clone();
        let mut term = Term::new(config, &size, listener.clone());
        term.set_transcript_hook(Some(Arc::new(move |event| {
            transcript_events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(event);
        })));
        Self {
            term,
            processor: Processor::new(),
            listener,
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
        self.drain_transcript_events()
    }

    pub fn resize(&mut self, columns: NonZeroU32, rows: NonZeroU32) -> Vec<AdapterEvent> {
        self.term.resize(GridSize { columns, rows });
        self.columns = columns;
        self.rows = rows;
        self.drain_transcript_events()
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
    fn resize_reports_the_existing_vendor_removed_set_as_a_fact() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(4));
        terminal.feed(b"a\r\nb\r\nc\r\nd");
        let events = terminal.resize(nz(8), nz(2));
        assert_eq!(
            removed_context(&events),
            Some(RemovalContext {
                cause: RemovalCause::Resize,
                screen: RemovalScreen::Primary,
                scope: RemovalScope::FullScreen,
            })
        );
        assert!(matches!(
            &events[0],
            AdapterEvent::RowsRemoved { rows, .. }
                if rows.len() == 2 && rows[0].row.cells[0].text == "a"
        ));
    }

    #[test]
    fn reset_and_deccolm_are_distinct_facts() {
        let mut terminal = TerminalAdapter::new(nz(8), nz(3));
        assert!(terminal.feed(b"\x1bc").contains(&AdapterEvent::Reset));
        assert!(terminal.feed(b"\x1b[?3h").contains(&AdapterEvent::Deccolm));
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
