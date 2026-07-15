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
