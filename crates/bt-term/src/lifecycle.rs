//! Executable DESIGN.md §3.1 policy.
//!
//! The adapter supplies terminal facts. This module decides which steady-state removed rows enter
//! transcript staging and which semantic terminal events mutate the transcript. During a resize
//! transaction the session uses these row directives only to rebase live anchors; vendor history is
//! the sole mutable-tail owner. Soft-wrap completion and the 4K staging bound remain enforced by
//! `TranscriptStore`; viewport-only scrolling and an unterminated live last line produce no adapter
//! event and therefore cannot enter this policy pipeline.

use std::num::NonZeroU32;

use bt_transcript::CapturedRow;

use crate::{
    adapter::{
        AdapterEvent, RemovalCause, RemovalContext, RemovalScope, RemovalScreen, RemovedLiveRow,
    },
    cell_capture::captured_row_is_blank,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RowShape {
    Blank,
    NonBlank,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RowAction {
    CaptureAndRebase,
    DiscardAndRebase,
    Ignore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchValue<T> {
    Any,
    Exact(T),
}

impl<T: Copy + Eq> MatchValue<T> {
    fn matches(self, value: T) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(expected) => expected == value,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LifecycleRule {
    pub screen: MatchValue<RemovalScreen>,
    pub scope: MatchValue<RemovalScope>,
    pub cause: MatchValue<RemovalCause>,
    pub shape: MatchValue<RowShape>,
    pub action: RowAction,
}

impl LifecycleRule {
    fn matches(self, context: RemovalContext, shape: RowShape) -> bool {
        self.screen.matches(context.screen)
            && self.scope.matches(context.scope)
            && self.cause.matches(context.cause)
            && self.shape.matches(shape)
    }
}

/// Ordered DESIGN.md §3.1 row-removal decision table.
///
/// `FullScreen` scope means the scroll feeds scrollback under xterm/alacritty semantics: any
/// output scroll whose removed rows leave through row 0 — including a top-anchored DECSTBM region
/// whose bottom sits above the last line, which is how ratatui/Codex-style inline TUIs commit
/// finalized lines above their bottom viewport. The last rule is a fail-closed catch-all:
/// non-top-anchored region scrolls, IL/DL, and alternate-screen removal are observable facts but
/// never become canonical history.
pub const LIFECYCLE_RULES: [LifecycleRule; 4] = [
    LifecycleRule {
        screen: MatchValue::Exact(RemovalScreen::Primary),
        scope: MatchValue::Exact(RemovalScope::FullScreen),
        cause: MatchValue::Exact(RemovalCause::NormalScroll),
        shape: MatchValue::Any,
        action: RowAction::CaptureAndRebase,
    },
    LifecycleRule {
        screen: MatchValue::Exact(RemovalScreen::Primary),
        scope: MatchValue::Exact(RemovalScope::FullScreen),
        cause: MatchValue::Exact(RemovalCause::Resize),
        shape: MatchValue::Exact(RowShape::NonBlank),
        action: RowAction::CaptureAndRebase,
    },
    LifecycleRule {
        screen: MatchValue::Exact(RemovalScreen::Primary),
        scope: MatchValue::Exact(RemovalScope::FullScreen),
        cause: MatchValue::Exact(RemovalCause::Resize),
        shape: MatchValue::Exact(RowShape::Blank),
        action: RowAction::DiscardAndRebase,
    },
    LifecycleRule {
        screen: MatchValue::Any,
        scope: MatchValue::Any,
        cause: MatchValue::Any,
        shape: MatchValue::Any,
        action: RowAction::Ignore,
    },
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RowDirective {
    Capture { live_row: u32, row: CapturedRow },
    DiscardFromTop { live_row: u32 },
    Ignore,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LifecycleDirective {
    RowsRemoved {
        rows: Vec<RowDirective>,
    },
    GridCoordinatesInvalidated,
    ClearHistoryAndStaging,
    InvalidateStaging,
    ParkPrimary,
    RestorePrimary,
    InlineImage {
        screen: RemovalScreen,
        row: u32,
        column: u32,
        encoded: Vec<u8>,
    },
    ShellIntegration {
        screen: RemovalScreen,
        row: u32,
        column: u32,
        marker: crate::inline_image::ShellIntegrationMarker,
    },
    WorkingDirectory {
        uri: String,
    },
    GridWrites {
        screen: RemovalScreen,
        rows: Vec<u32>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResizePlan {
    pub begin_transaction: bool,
}

/// DESIGN.md §3.1 resize policy derived only from old/new terminal facts.
pub fn plan_resize(old: (NonZeroU32, NonZeroU32), new: (NonZeroU32, NonZeroU32)) -> ResizePlan {
    ResizePlan {
        begin_transaction: old != new,
    }
}

/// Consume one adapter fact and return a payload-safe directive. Session code never pairs an
/// independently looked-up action with the original event, so an event/directive mismatch is not
/// representable.
pub fn classify(event: AdapterEvent) -> LifecycleDirective {
    match event {
        AdapterEvent::RowsRemoved { context, rows } => LifecycleDirective::RowsRemoved {
            rows: rows
                .into_iter()
                .map(|row| classify_row(context, row))
                .collect(),
        },
        AdapterEvent::GridScrolled | AdapterEvent::ScreenCleared => {
            LifecycleDirective::GridCoordinatesInvalidated
        }
        AdapterEvent::ClearHistory => LifecycleDirective::ClearHistoryAndStaging,
        AdapterEvent::Reset | AdapterEvent::Deccolm => LifecycleDirective::InvalidateStaging,
        AdapterEvent::PrimaryParked => LifecycleDirective::ParkPrimary,
        AdapterEvent::PrimaryRestored => LifecycleDirective::RestorePrimary,
        AdapterEvent::InlineImage {
            screen,
            row,
            column,
            encoded,
        } => LifecycleDirective::InlineImage {
            screen,
            row,
            column,
            encoded,
        },
        AdapterEvent::ShellIntegration {
            screen,
            row,
            column,
            marker,
        } => LifecycleDirective::ShellIntegration {
            screen,
            row,
            column,
            marker,
        },
        AdapterEvent::WorkingDirectory { uri } => LifecycleDirective::WorkingDirectory { uri },
        AdapterEvent::GridWrites { screen, rows } => {
            LifecycleDirective::GridWrites { screen, rows }
        }
    }
}

fn classify_row(context: RemovalContext, row: RemovedLiveRow) -> RowDirective {
    let shape = if captured_row_is_blank(&row.row) {
        RowShape::Blank
    } else {
        RowShape::NonBlank
    };
    let action = LIFECYCLE_RULES
        .iter()
        .find(|rule| rule.matches(context, shape))
        .map_or(RowAction::Ignore, |rule| rule.action);
    match action {
        RowAction::CaptureAndRebase => RowDirective::Capture {
            live_row: row.live_row,
            row: row.row,
        },
        RowAction::DiscardAndRebase => RowDirective::DiscardFromTop {
            live_row: row.live_row,
        },
        RowAction::Ignore => RowDirective::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_transcript::CapturedRow;

    fn row(text: &str) -> RemovedLiveRow {
        RemovedLiveRow {
            live_row: 2,
            row: CapturedRow::plain(text, false),
        }
    }

    fn context(screen: RemovalScreen, scope: RemovalScope, cause: RemovalCause) -> RemovalContext {
        RemovalContext {
            cause,
            screen,
            scope,
        }
    }

    #[test]
    fn row_table_covers_every_fact_combination_and_uses_the_first_matching_rule() {
        for screen in [RemovalScreen::Primary, RemovalScreen::Alternate] {
            for scope in [RemovalScope::FullScreen, RemovalScope::Partial] {
                for cause in [
                    RemovalCause::NormalScroll,
                    RemovalCause::DeleteLines,
                    RemovalCause::Resize,
                ] {
                    for shape in [RowShape::Blank, RowShape::NonBlank] {
                        let context = context(screen, scope, cause);
                        assert!(
                            LIFECYCLE_RULES
                                .iter()
                                .any(|rule| rule.matches(context, shape)),
                            "missing lifecycle rule for {context:?} {shape:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn resize_blank_is_discarded_while_nonblank_is_captured() {
        let context = context(
            RemovalScreen::Primary,
            RemovalScope::FullScreen,
            RemovalCause::Resize,
        );
        assert!(matches!(
            classify_row(context, row("   ")),
            RowDirective::DiscardFromTop { live_row: 2 }
        ));
        assert!(matches!(
            classify_row(context, row("kept")),
            RowDirective::Capture { live_row: 2, .. }
        ));
    }

    #[test]
    fn local_delete_and_alternate_rows_are_ignored() {
        for context in [
            context(
                RemovalScreen::Primary,
                RemovalScope::Partial,
                RemovalCause::NormalScroll,
            ),
            context(
                RemovalScreen::Primary,
                RemovalScope::FullScreen,
                RemovalCause::DeleteLines,
            ),
            context(
                RemovalScreen::Alternate,
                RemovalScope::FullScreen,
                RemovalCause::NormalScroll,
            ),
        ] {
            assert_eq!(classify_row(context, row("drop")), RowDirective::Ignore);
        }
    }

    #[test]
    fn any_dimension_change_begins_a_vendor_owned_resize_transaction() {
        let nz = |value| NonZeroU32::new(value).unwrap();
        assert!(plan_resize((nz(80), nz(24)), (nz(100), nz(30))).begin_transaction);
        assert!(plan_resize((nz(80), nz(24)), (nz(80), nz(30))).begin_transaction);
        assert!(!plan_resize((nz(80), nz(24)), (nz(80), nz(24))).begin_transaction);
    }
}
