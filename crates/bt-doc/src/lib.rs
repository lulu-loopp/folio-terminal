//! Layout-free history document and the cross-plane anchor protocol.

use std::{cmp::Ordering, collections::BTreeMap};

use bt_transcript::{
    Finalized, FrozenLine, GraphemeOffset, SourceGeneration, StagingId, TranscriptId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum ScreenId {
    Primary,
    Alternate,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct GridGeneration(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct GridPoint {
    pub row: u32,
    pub column: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Bias {
    Before,
    After,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
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
        generation: GridGeneration,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AnchorId(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Selection {
    pub start: AnchorId,
    pub end: AnchorId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveRowRemoval {
    pub row: u32,
    pub staging: Option<(StagingId, SourceGeneration)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SourceLifecycle {
    Live,
    Frozen,
    Tombstoned,
}

impl SourceLifecycle {
    pub fn transition(&mut self, next: Self) -> bool {
        let allowed = matches!(
            (*self, next),
            (Self::Live, Self::Frozen) | (Self::Frozen, Self::Tombstoned)
        );
        if allowed {
            *self = next;
        }
        allowed
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DecorationIntent {
    Plain,
    Math {
        byte_start: u32,
        byte_end: u32,
        detection_revision: DetectionRevision,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DecorationLifecycle {
    None,
    Pending,
    Ready,
    Failed,
    Suppressed,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct DetectionRevision(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct LayoutKey {
    pub width_cells: u32,
    pub dpi_milli: u32,
    pub font_rev: u64,
    pub theme_rev: u64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ViewGeneration(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct VersionStamp {
    pub source: SourceGeneration,
    pub detection: DetectionRevision,
    pub layout: LayoutKey,
    pub view: ViewGeneration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntry {
    pub line: FrozenLine,
    pub source: SourceLifecycle,
    pub decoration: DecorationIntent,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum AnchorError {
    #[error("alternate-screen anchors live in an isolated namespace")]
    IsolatedScreen,
    #[error("unknown anchor")]
    UnknownAnchor,
    #[error("live anchor is outside the addressable grid")]
    LiveOutOfBounds,
}

#[derive(Clone, Debug, Default)]
pub struct HistoryDocument {
    entries: BTreeMap<TranscriptId, HistoryEntry>,
    anchors: BTreeMap<AnchorId, ContentAnchor>,
    selection: Option<Selection>,
    next_anchor: u64,
    tombstones: Vec<TranscriptId>,
}

impl HistoryDocument {
    pub fn entries(&self) -> &BTreeMap<TranscriptId, HistoryEntry> {
        &self.entries
    }
    pub fn tombstones(&self) -> &[TranscriptId] {
        &self.tombstones
    }
    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }

    pub fn register_anchor(&mut self, anchor: ContentAnchor) -> AnchorId {
        self.next_anchor += 1;
        let id = AnchorId(self.next_anchor);
        self.anchors.insert(id, anchor);
        id
    }

    pub fn anchor(&self, id: AnchorId) -> Result<&ContentAnchor, AnchorError> {
        self.anchors.get(&id).ok_or(AnchorError::UnknownAnchor)
    }

    pub fn set_selection(&mut self, start: AnchorId, end: AnchorId) {
        self.selection = Some(Selection { start, end });
    }

    /// Capture transaction: all matching persistent primary-live anchors move together.
    pub fn capture_transaction(
        &mut self,
        live_row: u32,
        staging_id: StagingId,
        source_gen: SourceGeneration,
        live_generation: GridGeneration,
    ) {
        self.capture_rows_transaction(
            &[LiveRowRemoval {
                row: live_row,
                staging: Some((staging_id, source_gen)),
            }],
            live_generation,
        );
    }

    /// Atomically migrate removed rows and rebase every surviving primary Live anchor.
    pub fn capture_rows_transaction(
        &mut self,
        removals: &[LiveRowRemoval],
        live_generation: GridGeneration,
    ) {
        if removals.is_empty() {
            return;
        }
        let replacements = self
            .anchors
            .iter()
            .filter_map(|(id, anchor)| match anchor {
                ContentAnchor::Live {
                    screen: ScreenId::Primary,
                    point,
                    bias,
                    ..
                } => {
                    if let Some(removal) = removals.iter().find(|item| item.row == point.row) {
                        let replacement = removal.staging.map_or(
                            ContentAnchor::Live {
                                screen: ScreenId::Primary,
                                point: GridPoint { row: 0, column: 0 },
                                bias: Bias::Before,
                                generation: live_generation,
                            },
                            |(staging_id, source_gen)| ContentAnchor::Staging {
                                id: staging_id,
                                offset: GraphemeOffset(point.column),
                                bias: *bias,
                                generation: source_gen,
                            },
                        );
                        return Some((*id, replacement));
                    }
                    let removed_before =
                        removals.iter().filter(|item| item.row < point.row).count() as u32;
                    (removed_before != 0).then(|| {
                        (
                            *id,
                            ContentAnchor::Live {
                                screen: ScreenId::Primary,
                                point: GridPoint {
                                    row: point.row - removed_before,
                                    column: point.column,
                                },
                                bias: *bias,
                                generation: live_generation,
                            },
                        )
                    })
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for (id, anchor) in replacements {
            self.anchors.insert(id, anchor);
        }
    }

    /// Finalize transaction: insert source and migrate every Staging anchor using one mapping table.
    pub fn finalize_transaction(&mut self, finalized: Finalized) {
        let generation = finalized.line.source_generation;
        let transcript_id = finalized.line.id;
        let replacements = self
            .anchors
            .iter()
            .filter_map(|(anchor_id, anchor)| {
                let ContentAnchor::Staging {
                    id, offset, bias, ..
                } = anchor
                else {
                    return None;
                };
                let mapping = finalized
                    .mappings
                    .iter()
                    .find(|mapping| mapping.staging_id == *id)?;
                Some((
                    *anchor_id,
                    ContentAnchor::History {
                        id: mapping.transcript_id,
                        offset: GraphemeOffset(mapping.grapheme_base.0 + offset.0),
                        bias: *bias,
                        generation,
                    },
                ))
            })
            .collect::<Vec<_>>();

        self.entries.insert(
            transcript_id,
            HistoryEntry {
                line: finalized.line,
                source: SourceLifecycle::Frozen,
                decoration: DecorationIntent::Plain,
            },
        );
        for (id, anchor) in replacements {
            self.anchors.insert(id, anchor);
        }
    }

    pub fn set_decoration(&mut self, id: TranscriptId, intent: DecorationIntent) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.decoration = intent;
        }
    }

    pub fn clear_decorations(&mut self) {
        for entry in self.entries.values_mut() {
            entry.decoration = DecorationIntent::Plain;
        }
    }

    /// Shared ED3/quota deletion transaction with deterministic successor degradation.
    pub fn delete_transaction(
        &mut self,
        removed: &[TranscriptId],
        clear_staging: bool,
        live_gen: GridGeneration,
    ) {
        if removed.is_empty() && !clear_staging {
            return;
        }
        let removed_set = removed
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        for id in removed {
            if let Some(mut entry) = self.entries.remove(id) {
                entry.source = SourceLifecycle::Tombstoned;
                self.tombstones.push(entry.line.id);
            }
        }
        let deleted_anchors = self
            .anchors
            .iter()
            .filter_map(|(anchor_id, anchor)| match anchor {
                ContentAnchor::History { id, .. } if removed_set.contains(id) => {
                    Some((*anchor_id, Some(*id)))
                }
                ContentAnchor::Staging { .. } if clear_staging => Some((*anchor_id, None)),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();

        if self.selection.as_ref().is_some_and(|selection| {
            deleted_anchors.contains_key(&selection.start)
                && deleted_anchors.contains_key(&selection.end)
        }) {
            self.selection = None;
        }

        for (id, deleted_history_id) in deleted_anchors {
            let successor = deleted_history_id.and_then(|deleted_id| {
                self.entries
                    .range((
                        std::ops::Bound::Excluded(deleted_id),
                        std::ops::Bound::Unbounded,
                    ))
                    .next()
                    .map(|(id, _)| *id)
            });
            let replacement = successor.map_or(
                ContentAnchor::Live {
                    screen: ScreenId::Primary,
                    point: GridPoint { row: 0, column: 0 },
                    bias: Bias::Before,
                    generation: live_gen,
                },
                |successor| ContentAnchor::History {
                    id: successor,
                    offset: GraphemeOffset(0),
                    bias: Bias::Before,
                    generation: self.entries[&successor].line.source_generation,
                },
            );
            self.anchors.insert(id, replacement);
        }
    }
}

/// Total order exists only in the primary document namespace.
pub fn compare_anchors(
    left: &ContentAnchor,
    right: &ContentAnchor,
) -> Result<Ordering, AnchorError> {
    use ContentAnchor::*;
    if matches!(
        left,
        Live {
            screen: ScreenId::Alternate,
            ..
        }
    ) || matches!(
        right,
        Live {
            screen: ScreenId::Alternate,
            ..
        }
    ) {
        return Err(AnchorError::IsolatedScreen);
    }
    let key = |anchor: &ContentAnchor| match anchor {
        History {
            id, offset, bias, ..
        } => (0, id.0, offset.0 as u64, *bias as u64),
        Staging {
            id, offset, bias, ..
        } => (1, id.0, offset.0 as u64, *bias as u64),
        Live {
            screen: ScreenId::Primary,
            point,
            bias,
            ..
        } => (2, point.row as u64, point.column as u64, *bias as u64),
        Live {
            screen: ScreenId::Alternate,
            ..
        } => unreachable!(),
    };
    Ok(key(left).cmp(&key(right)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_transcript::{CapturedRow, TranscriptStore};

    #[test]
    fn anchors_migrate_live_to_staging_to_history() {
        let mut document = HistoryDocument::default();
        let anchor = document.register_anchor(ContentAnchor::Live {
            screen: ScreenId::Primary,
            point: GridPoint { row: 0, column: 2 },
            bias: Bias::After,
            generation: GridGeneration(7),
        });
        let mut store = TranscriptStore::new(8);
        let captured = store.capture(CapturedRow::plain("abc", false));
        document.capture_transaction(
            0,
            captured.staging_id,
            store.source_generation(),
            GridGeneration(8),
        );
        assert!(matches!(
            document.anchor(anchor).unwrap(),
            ContentAnchor::Staging { .. }
        ));
        document.finalize_transaction(captured.finalized.into_iter().next().unwrap());
        assert!(matches!(
            document.anchor(anchor).unwrap(),
            ContentAnchor::History {
                offset: GraphemeOffset(2),
                bias: Bias::After,
                ..
            }
        ));
    }

    #[test]
    fn capture_batch_migrates_removed_rows_and_rebases_survivors() {
        let mut document = HistoryDocument::default();
        let removed = document.register_anchor(ContentAnchor::Live {
            screen: ScreenId::Primary,
            point: GridPoint { row: 1, column: 2 },
            bias: Bias::After,
            generation: GridGeneration(1),
        });
        let survivor = document.register_anchor(ContentAnchor::Live {
            screen: ScreenId::Primary,
            point: GridPoint { row: 3, column: 4 },
            bias: Bias::After,
            generation: GridGeneration(1),
        });
        document.capture_rows_transaction(
            &[
                LiveRowRemoval {
                    row: 0,
                    staging: Some((StagingId(10), SourceGeneration(2))),
                },
                LiveRowRemoval {
                    row: 1,
                    staging: Some((StagingId(11), SourceGeneration(2))),
                },
            ],
            GridGeneration(3),
        );
        assert!(matches!(
            document.anchor(removed).unwrap(),
            ContentAnchor::Staging {
                id: StagingId(11),
                offset: GraphemeOffset(2),
                ..
            }
        ));
        assert!(matches!(
            document.anchor(survivor).unwrap(),
            ContentAnchor::Live {
                point: GridPoint { row: 1, column: 4 },
                generation: GridGeneration(3),
                ..
            }
        ));
    }

    #[test]
    fn alternate_namespace_is_not_comparable() {
        let primary = ContentAnchor::Live {
            screen: ScreenId::Primary,
            point: GridPoint { row: 0, column: 0 },
            bias: Bias::Before,
            generation: GridGeneration(1),
        };
        let alternate = ContentAnchor::Live {
            screen: ScreenId::Alternate,
            point: GridPoint { row: 0, column: 0 },
            bias: Bias::Before,
            generation: GridGeneration(1),
        };
        assert_eq!(
            compare_anchors(&primary, &alternate),
            Err(AnchorError::IsolatedScreen)
        );
    }

    #[test]
    fn deletion_uses_successor_then_live_origin_and_clears_deleted_selection() {
        let mut store = TranscriptStore::new(8);
        let first = store
            .capture(CapturedRow::plain("one", false))
            .finalized
            .remove(0);
        let second = store
            .capture(CapturedRow::plain("two", false))
            .finalized
            .remove(0);
        let mut document = HistoryDocument::default();
        let a = document.register_anchor(ContentAnchor::History {
            id: first.line.id,
            offset: GraphemeOffset(1),
            bias: Bias::After,
            generation: first.line.source_generation,
        });
        let b = document.register_anchor(ContentAnchor::History {
            id: first.line.id,
            offset: GraphemeOffset(2),
            bias: Bias::After,
            generation: first.line.source_generation,
        });
        document.finalize_transaction(first.clone());
        document.finalize_transaction(second.clone());
        document.set_selection(a, b);
        document.delete_transaction(&[first.line.id], false, GridGeneration(9));
        assert!(document.selection().is_none());
        assert!(
            matches!(document.anchor(a).unwrap(), ContentAnchor::History { id, bias: Bias::Before, .. } if *id == second.line.id)
        );
        document.delete_transaction(&[second.line.id], true, GridGeneration(10));
        assert!(matches!(
            document.anchor(a).unwrap(),
            ContentAnchor::Live {
                point: GridPoint { row: 0, column: 0 },
                bias: Bias::Before,
                ..
            }
        ));
    }
}
