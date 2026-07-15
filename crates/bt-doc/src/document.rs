use std::collections::{BTreeMap, BTreeSet};

use bt_transcript::{
    FinalizedLine, FrozenLine, GraphemeOffset, SourceGeneration, StagingId, TranscriptId,
};

use crate::{
    AnchorError, AnchorId, Bias, ContentAnchor, DecorationIntent, GridGeneration, GridPoint,
    ScreenId, Selection,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveRowRemoval {
    pub row: u32,
    pub staging: Option<(StagingId, SourceGeneration)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntry {
    pub line: FrozenLine,
    pub decoration: DecorationIntent,
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

    /// DESIGN.md §3.2 capture transaction: migrate removed rows and rebase survivors atomically.
    pub fn capture_rows_transaction(
        &mut self,
        removals: &[LiveRowRemoval],
        live_generation: GridGeneration,
    ) {
        let replacements = self
            .anchors
            .iter()
            .filter_map(|(id, anchor)| match anchor {
                ContentAnchor::Live {
                    screen: ScreenId::Primary,
                    point,
                    bias,
                    generation,
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
                    (removed_before != 0 || *generation != live_generation).then(|| {
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

    /// DESIGN.md §3.2 finalize transaction: insert source and migrate Staging anchors together.
    pub fn finalize_transaction(&mut self, finalized: FinalizedLine) {
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

    /// DESIGN.md §3.2 shared ED3/quota deletion with deterministic successor degradation.
    pub fn delete_transaction(
        &mut self,
        removed: &[TranscriptId],
        clear_staging: bool,
        live_gen: GridGeneration,
    ) {
        if removed.is_empty() && !clear_staging {
            return;
        }
        let removed_set = removed.iter().copied().collect::<BTreeSet<_>>();
        for id in removed {
            if let Some(entry) = self.entries.remove(id) {
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
                    .map(|(id, entry)| (*id, entry.line.source_generation))
            });
            let replacement = successor.map_or(
                ContentAnchor::Live {
                    screen: ScreenId::Primary,
                    point: GridPoint { row: 0, column: 0 },
                    bias: Bias::Before,
                    generation: live_gen,
                },
                |(successor, generation)| ContentAnchor::History {
                    id: successor,
                    offset: GraphemeOffset(0),
                    bias: Bias::Before,
                    generation,
                },
            );
            self.anchors.insert(id, replacement);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use super::*;
    use bt_transcript::{CapturedRow, TranscriptStore};

    fn store() -> TranscriptStore {
        TranscriptStore::new(NonZeroUsize::new(8).unwrap())
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
    fn deletion_uses_successor_then_live_origin_and_clears_deleted_selection() {
        let mut store = store();
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
        assert!(matches!(
            document.anchor(a).unwrap(),
            ContentAnchor::History { id, bias: Bias::Before, .. } if *id == second.line.id
        ));
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
