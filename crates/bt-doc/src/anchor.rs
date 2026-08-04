use std::cmp::Ordering;

use bt_transcript::{GraphemeOffset, SourceGeneration, StagingId, TranscriptId};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ScreenId {
    Primary,
    Alternate,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GridPoint {
    pub row: u32,
    pub column: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Bias {
    Before,
    After,
}

/// DESIGN.md §3.2 cross-plane coordinate. Generations are validated by consumers before use.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ContentAnchor {
    History {
        id: TranscriptId,
        offset: GraphemeOffset,
        bias: Bias,
        generation: SourceGeneration,
    },
    Staging {
        id: StagingId,
        offset: GraphemeOffset,
        bias: Bias,
        generation: SourceGeneration,
    },
    Live {
        screen: ScreenId,
        point: GridPoint,
        bias: Bias,
        generation: super::GridGeneration,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AnchorId(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Selection {
    pub start: AnchorId,
    pub end: AnchorId,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum AnchorError {
    #[error("alternate-screen anchors live in an isolated namespace")]
    IsolatedScreen,
    #[error("unknown anchor")]
    UnknownAnchor,
    #[error("anchor generation no longer matches its source")]
    StaleGeneration,
    #[error("live anchor is outside the addressable grid")]
    LiveOutOfBounds,
}

fn primary_key(anchor: &ContentAnchor) -> Option<(u8, u64, u64, u64)> {
    match anchor {
        ContentAnchor::History {
            id, offset, bias, ..
        } => Some((0, id.0, u64::from(offset.0), *bias as u64)),
        ContentAnchor::Staging {
            id, offset, bias, ..
        } => Some((1, id.0, u64::from(offset.0), *bias as u64)),
        ContentAnchor::Live {
            screen: ScreenId::Primary,
            point,
            bias,
            ..
        } => Some((
            2,
            u64::from(point.row),
            u64::from(point.column),
            *bias as u64,
        )),
        ContentAnchor::Live {
            screen: ScreenId::Alternate,
            ..
        } => None,
    }
}

/// DESIGN.md §3.2 total order within the primary document namespace.
pub fn compare_anchors(
    left: &ContentAnchor,
    right: &ContentAnchor,
) -> Result<Ordering, AnchorError> {
    let left = primary_key(left).ok_or(AnchorError::IsolatedScreen)?;
    let right = primary_key(right).ok_or(AnchorError::IsolatedScreen)?;
    Ok(left.cmp(&right))
}

/// Whether `candidate` lies in the half-open span `[start, end)`, comparing *within one plane*.
///
/// Deliberately not `compare_anchors`: that is the primary document's total order, which by
/// construction cannot see the alternate screen (`IsolatedScreen`) and would happily order a
/// `History` endpoint against a `Live` one across a generation boundary. A span is a claim about
/// one carrier — one transcript line, one staged row, one grid generation of one screen — so all
/// three anchors must name the same carrier or the answer is simply "no". That makes the predicate
/// total, alternate-screen safe, and unable to spread a decoration across a plane boundary.
///
/// Bias is ignored: an endpoint's bias says which side of a grapheme it clings to as content moves,
/// not where the span ends. Shared by the record-coverage questions in bt-term and by the frame
/// decoration in bt-viewport, so a decoration is painted over exactly the cells a record claims.
pub fn content_anchor_between(
    candidate: &ContentAnchor,
    start: &ContentAnchor,
    end: &ContentAnchor,
) -> bool {
    match (candidate, start, end) {
        (
            ContentAnchor::History {
                id,
                offset,
                generation,
                ..
            },
            ContentAnchor::History {
                id: start_id,
                offset: start_offset,
                generation: start_generation,
                ..
            },
            ContentAnchor::History {
                id: end_id,
                offset: end_offset,
                generation: end_generation,
                ..
            },
        ) => {
            id == start_id
                && id == end_id
                && generation == start_generation
                && generation == end_generation
                && start_offset <= offset
                && offset < end_offset
        }
        (
            ContentAnchor::Staging {
                id,
                offset,
                generation,
                ..
            },
            ContentAnchor::Staging {
                id: start_id,
                offset: start_offset,
                generation: start_generation,
                ..
            },
            ContentAnchor::Staging {
                id: end_id,
                offset: end_offset,
                generation: end_generation,
                ..
            },
        ) => {
            id == start_id
                && id == end_id
                && generation == start_generation
                && generation == end_generation
                && start_offset <= offset
                && offset < end_offset
        }
        (
            ContentAnchor::Live {
                screen,
                point,
                generation,
                ..
            },
            ContentAnchor::Live {
                screen: start_screen,
                point: start_point,
                generation: start_generation,
                ..
            },
            ContentAnchor::Live {
                screen: end_screen,
                point: end_point,
                generation: end_generation,
                ..
            },
        ) => {
            screen == start_screen
                && screen == end_screen
                && generation == start_generation
                && generation == end_generation
                && (point.row, point.column) >= (start_point.row, start_point.column)
                && (point.row, point.column) < (end_point.row, end_point.column)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GridGeneration;

    #[test]
    fn alternate_namespace_is_not_comparable() {
        let primary = ContentAnchor::Live {
            screen: ScreenId::Primary,
            point: GridPoint { row: 2, column: 0 },
            bias: Bias::Before,
            generation: GridGeneration(3),
        };
        let alternate = ContentAnchor::Live {
            screen: ScreenId::Alternate,
            point: GridPoint { row: 2, column: 0 },
            bias: Bias::Before,
            generation: GridGeneration(3),
        };
        assert_eq!(
            compare_anchors(&primary, &alternate),
            Err(AnchorError::IsolatedScreen)
        );
    }
}
