use std::collections::{BTreeMap, BTreeSet};

use bt_transcript::{
    FinalizedLine, FrozenLine, GraphemeOffset, SourceGeneration, StagingId, TranscriptId,
};

use crate::{
    AnchorError, AnchorId, Bias, ContentAnchor, DecorationIntent, GridGeneration, GridPoint,
    InlineMathSite, ScreenId, Selection,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveRowRemoval {
    pub row: u32,
    pub staging: Option<(StagingId, SourceGeneration)>,
    /// Terminal cell column to source-grapheme offset. Wide spacers repeat their lead offset.
    pub grapheme_offsets: Vec<GraphemeOffset>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntry {
    pub line: FrozenLine,
    pub decoration: DecorationIntent,
    /// Where this line stood in the shell's command lifecycle at the moment it froze.
    ///
    /// **This is a stored fact, not a derived one, and the difference is the whole point.** A
    /// line's site is a proof taken while the evidence existed: the OSC 133 region that covered
    /// it was alive and its anchors still resolved when the line entered history, so the session
    /// wrote the verdict down here. Asking the question again later cannot reach the same answer,
    /// because the region dies with its own first line — eviction, mark retirement, a reflow that
    /// declined to re-seat an anchor — while the output lines below it stay resident and keep
    /// showing what the command printed. A re-derived site therefore decays to
    /// [`InlineMathSite::Ineligible`] and un-typesets an inline formula that was proven eligible
    /// when it was printed; a recorded one does not.
    ///
    /// Only the session may write it ([`HistoryDocument::set_inline_site`]); a line that has not
    /// been judged reads [`InlineMathSite::Ineligible`], which is the conservative verdict.
    pub inline_site: InlineMathSite,
    /// The staging rows this line was frozen from — the keys of its own entries in
    /// [`HistoryDocument`]'s staging relocations.
    ///
    /// **Kept so that deleting the line deletes exactly those and touches nothing else** (0.4.10
    /// plan row 105). A relocation lives as long as its line, because a scroll anchor that still
    /// names a staging row resolves through it on every frame. Deletion used to find a line's
    /// relocations by walking all of them, and once history is full every frozen line deletes
    /// one: on the clean VM that walk was 500,000–800,000 entries per line, 6 ms each, and a
    /// flood fell from 10,000 lines a second to 143.
    relocated_from: Box<[StagingId]>,
}

#[derive(Clone, Debug, Default)]
pub struct HistoryDocument {
    entries: BTreeMap<TranscriptId, HistoryEntry>,
    anchors: BTreeMap<AnchorId, ContentAnchor>,
    staging_relocations: BTreeMap<StagingId, StagingRelocation>,
    selection: Option<Selection>,
    next_anchor: u64,
    /// How many staging relocations deletion has visited, ever. See
    /// [`Self::relocation_visits`].
    relocation_visits: u64,
    /// How many lines have entered or left this document anywhere but at its two ends. See
    /// [`Self::interior_edits`].
    interior_edits: u64,
}

#[derive(Clone, Copy, Debug)]
struct StagingRelocation {
    transcript_id: TranscriptId,
    grapheme_base: GraphemeOffset,
    generation: SourceGeneration,
}

impl HistoryDocument {
    pub fn entries(&self) -> &BTreeMap<TranscriptId, HistoryEntry> {
        &self.entries
    }

    /// **The work deletion has done on the staging relocations, counted rather than timed.**
    ///
    /// One visit per relocation entry a [`Self::delete_transaction`] looks at. Deleting a line
    /// visits that line's own rows and nothing else, so the count a line costs is the same in a
    /// history of a thousand lines and of a hundred thousand — which is the claim the tests make
    /// with it (0.4.10 plan row 105). Hidden: it is a measurement, not an API.
    #[doc(hidden)]
    #[must_use]
    pub fn relocation_visits(&self) -> u64 {
        self.relocation_visits
    }

    /// **How many lines have entered or left this document anywhere but at its two ends**, ever:
    /// a line frozen with an id below the newest one, or a line deleted while an older one stays.
    ///
    /// A running pane only ever does the other two things — freezing appends the next id, an
    /// eviction takes the oldest line and ED3 takes all of them — so there this stays at zero. A
    /// reader that keeps its own ordered copy of the document (the viewport's projection) can
    /// therefore bring that copy up to date by dropping its front and appending what follows its
    /// end, and this is the number that tells it when it may not: any change here means a line
    /// moved in the middle, and only a rebuild is the document again.
    #[must_use]
    pub fn interior_edits(&self) -> u64 {
        self.interior_edits
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

    /// Give back one registered anchor.
    ///
    /// **The registry never decides on its own that an id has stopped meaning something.** A
    /// deletion *degrades* every anchor it touches onto a surviving neighbour rather than removing
    /// it ([`Self::delete_transaction`]), exactly so that a holder is never left resolving a
    /// dangling id — which is the right rule for reading and the wrong one for keeping, because
    /// nothing in here can tell a degraded anchor somebody still holds from one whose holder is
    /// long gone. So releasing is the holder's job: whoever asked for the id says when the thing
    /// it named has left, and until somebody does, every scroll and every resize walks it.
    pub fn release_anchor(&mut self, id: AnchorId) {
        self.anchors.remove(&id);
    }

    /// Every registered anchor, so a caller that is about to move the content under them can say
    /// what each one names before it moves.
    ///
    /// **The registry does not decide which of them are in danger**, and that is deliberate: what
    /// a reflow disturbs is the live grid and the escrow the vendor is holding beside it, and only
    /// the session knows which staging ids are that escrow's and which belong to lines already on
    /// their way to history. Handing over the whole list keeps that judgment in one place —
    /// `DualPlaneSession::reflow_witnesses` — instead of splitting it across a filter here and a
    /// second filter there.
    pub fn anchors(&self) -> impl Iterator<Item = (AnchorId, &ContentAnchor)> {
        self.anchors.iter().map(|(id, anchor)| (*id, anchor))
    }

    /// Re-seat one registered persistent anchor as part of a caller-owned reconciliation
    /// transaction. Selection anchors use `replace_selection`; decoration identities use this
    /// narrower single-anchor operation.
    pub fn replace_anchor(
        &mut self,
        id: AnchorId,
        anchor: ContentAnchor,
    ) -> Result<(), AnchorError> {
        let registered = self
            .anchors
            .get_mut(&id)
            .ok_or(AnchorError::UnknownAnchor)?;
        *registered = anchor;
        Ok(())
    }

    /// Resolve a viewport-owned staging coordinate through the same causal mapping used to migrate
    /// registered persistent anchors. This keeps scroll anchors semantic without making every
    /// viewport anchor part of the document's persistent anchor registry.
    pub fn resolve_anchor(&self, anchor: &ContentAnchor) -> ContentAnchor {
        let ContentAnchor::Staging {
            id, offset, bias, ..
        } = anchor
        else {
            return anchor.clone();
        };
        self.staging_relocations.get(id).map_or_else(
            || anchor.clone(),
            |relocation| ContentAnchor::History {
                id: relocation.transcript_id,
                offset: GraphemeOffset(relocation.grapheme_base.0.saturating_add(offset.0)),
                bias: *bias,
                generation: relocation.generation,
            },
        )
    }

    pub fn set_selection(&mut self, start: AnchorId, end: AnchorId) {
        self.selection = Some(Selection { start, end });
    }

    pub fn clear_selection(&mut self) {
        if let Some(selection) = self.selection.take() {
            self.anchors.remove(&selection.start);
            self.anchors.remove(&selection.end);
        }
    }

    pub fn replace_selection(&mut self, start: ContentAnchor, end: ContentAnchor) {
        if let Some(selection) = &self.selection {
            self.anchors.insert(selection.start, start);
            self.anchors.insert(selection.end, end);
        } else {
            let start = self.register_anchor(start);
            let end = self.register_anchor(end);
            self.selection = Some(Selection { start, end });
        }
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
                                offset: removal
                                    .grapheme_offsets
                                    .get(point.column as usize)
                                    .copied()
                                    .unwrap_or(GraphemeOffset(point.column)),
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
        // Every mapping of a frozen line names that line (`bt_transcript`'s `normalize`), which is
        // what lets the line's own `relocated_from` stand for "the relocations of this line".
        debug_assert!(
            finalized
                .mappings
                .iter()
                .all(|mapping| mapping.transcript_id == transcript_id),
            "a finalized line's mappings name that line"
        );
        let mut relocated_from = finalized
            .mappings
            .iter()
            .map(|mapping| mapping.staging_id)
            .collect::<Vec<_>>();
        // A freeze over an id the document already holds replaces that entry. Its relocations
        // still name this line, so the new entry keeps their keys too, or deleting the line could
        // no longer find them. A key a later line has taken over is skipped at deletion by the
        // relocation's own `transcript_id`.
        if let Some(previous) = self.entries.get(&transcript_id) {
            let kept = previous
                .relocated_from
                .iter()
                .filter(|staging| !relocated_from.contains(staging))
                .copied()
                .collect::<Vec<_>>();
            relocated_from.extend(kept);
        }
        let relocated_from = relocated_from.into_boxed_slice();
        for mapping in &finalized.mappings {
            self.staging_relocations.insert(
                mapping.staging_id,
                StagingRelocation {
                    transcript_id: mapping.transcript_id,
                    grapheme_base: mapping.grapheme_base,
                    generation,
                },
            );
        }
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

        if self
            .entries
            .last_key_value()
            .is_some_and(|(newest, _)| *newest >= transcript_id)
        {
            self.interior_edits = self.interior_edits.wrapping_add(1);
        }
        self.entries.insert(
            transcript_id,
            HistoryEntry {
                line: finalized.line,
                decoration: DecorationIntent::Plain,
                inline_site: InlineMathSite::Ineligible,
                relocated_from,
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

    /// Record where a line stood in the command lifecycle, once, as it freezes.
    ///
    /// The caller is the session, which is the only holder of the OSC 133 bookkeeping this answer
    /// comes from, and it asks while that bookkeeping is still standing. See
    /// [`HistoryEntry::inline_site`] for why the answer is kept instead of asked again.
    pub fn set_inline_site(&mut self, id: TranscriptId, site: InlineMathSite) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.inline_site = site;
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
            let oldest = self.entries.first_key_value().map(|(first, _)| *first);
            let Some(entry) = self.entries.remove(id) else {
                continue;
            };
            if oldest != Some(*id) {
                self.interior_edits = self.interior_edits.wrapping_add(1);
            }
            // The line's own relocations, found by the keys it carries rather than by walking
            // every relocation in the document (row 105). A relocation is only ever written for
            // the line whose mappings named it, so these are exactly the ones naming this line.
            for staging in &entry.relocated_from {
                self.relocation_visits = self.relocation_visits.wrapping_add(1);
                if self
                    .staging_relocations
                    .get(staging)
                    .is_some_and(|relocation| relocation.transcript_id == *id)
                {
                    self.staging_relocations.remove(staging);
                }
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
                || deleted_anchors.contains_key(&selection.end)
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
                    grapheme_offsets: vec![GraphemeOffset(0); 8],
                },
                LiveRowRemoval {
                    row: 1,
                    staging: Some((StagingId(11), SourceGeneration(2))),
                    grapheme_offsets: (0..8).map(GraphemeOffset).collect(),
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

    /// Rows per logical line in the eviction fixture: five soft-wrapped rows and the row that ends.
    const ROWS_PER_LINE: u64 = 6;

    /// Freeze one logical line of [`ROWS_PER_LINE`] physical rows, and run whatever it evicted
    /// through the document's deletion, as the session does.
    fn print_line(store: &mut TranscriptStore, document: &mut HistoryDocument) {
        for row in 0..ROWS_PER_LINE {
            for finalized in store
                .capture(CapturedRow::plain("row", row + 1 < ROWS_PER_LINE))
                .finalized
            {
                document.finalize_transaction(finalized);
            }
        }
        let evicted = store.take_evictions();
        document.delete_transaction(&evicted, false, GridGeneration(1));
    }

    /// **Evicting a line visits that line's own staging rows and no other relocation**, at a
    /// thousand lines of history and at twenty thousand alike (0.4.10 plan row 105, T1).
    ///
    /// Deletion used to find a line's relocations with a `retain` over all of them — one per
    /// frozen physical row — so the work per evicted line grew with history: with
    /// `staging_relocations.retain` back in `delete_transaction`, the thousand-line history reads
    /// 6 006: every relocation in it, the new line's six included.
    #[test]
    fn an_eviction_visits_only_its_own_rows() {
        for depth in [1_000, 20_000] {
            let mut store = TranscriptStore::with_quotas(
                bt_transcript::DEFAULT_STAGING_QUOTA,
                NonZeroUsize::new(depth).unwrap(),
            );
            let mut document = HistoryDocument::default();
            for _ in 0..depth {
                print_line(&mut store, &mut document);
            }
            assert_eq!(document.entries().len(), depth, "history is full");
            assert_eq!(
                document.relocation_visits(),
                0,
                "and nothing has left it yet"
            );

            print_line(&mut store, &mut document);
            assert_eq!(document.entries().len(), depth, "one line in, one line out");
            assert_eq!(
                document.relocation_visits(),
                ROWS_PER_LINE,
                "evicting one line of {ROWS_PER_LINE} rows from a {depth}-line history"
            );
        }
    }

    /// **A line deleted while an older one stays is an interior edit; an eviction is not** — the
    /// number the viewport reads to know it may follow the document's two ends instead of
    /// projecting it again (0.4.10 plan row 105).
    #[test]
    fn a_line_from_the_middle_is_an_interior_edit() {
        let mut store = TranscriptStore::with_quotas(
            bt_transcript::DEFAULT_STAGING_QUOTA,
            NonZeroUsize::new(3).unwrap(),
        );
        let mut document = HistoryDocument::default();
        for _ in 0..5 {
            print_line(&mut store, &mut document);
        }
        assert_eq!(
            document.interior_edits(),
            0,
            "evictions take the oldest line"
        );

        let ids = document.entries().keys().copied().collect::<Vec<_>>();
        document.delete_transaction(&[ids[1]], false, GridGeneration(1));
        assert_eq!(document.interior_edits(), 1, "the middle line went");
        document.delete_transaction(&[ids[0], ids[2]], false, GridGeneration(1));
        assert_eq!(document.interior_edits(), 1, "each went as the oldest");

        print_line(&mut store, &mut document);
        assert_eq!(
            document.interior_edits(),
            1,
            "the next line freezes past the newest"
        );
        let newest = *document.entries().keys().next_back().unwrap();
        document.finalize_transaction(frozen_again(ids[2], &store));
        assert_eq!(
            document.interior_edits(),
            2,
            "a line frozen below the newest one entered in the middle"
        );
        document.finalize_transaction(frozen_again(newest, &store));
        assert_eq!(
            document.interior_edits(),
            3,
            "and one frozen over the newest replaced it"
        );
    }

    /// A line frozen by hand over `id`, with no mappings and a newer source generation, as a
    /// rewrite in place builds one (the public API admits it; the product's freezes always take
    /// the store's next id).
    fn frozen_again(id: TranscriptId, store: &TranscriptStore) -> FinalizedLine {
        let text = "again".to_owned();
        FinalizedLine {
            line: FrozenLine {
                id,
                source_generation: SourceGeneration(store.source_generation().0 + 1),
                grapheme_boundaries: (0..=text.len() as u32).collect(),
                text,
                styles: Vec::new(),
                fragments: Vec::new(),
                shell_marks: Vec::new(),
                wrap_split: false,
                command_output_write: false,
            },
            mappings: Vec::new(),
        }
    }

    /// **A line frozen again over its own id keeps the staging rows it was first frozen from, and
    /// deleting it then removes their relocations too** (0.4.10 plan row 105, review P2-1).
    ///
    /// The second freeze brings no mappings of its own, so a line that carried only the new
    /// freeze's keys would leave its first rows' relocations behind for good: a staging anchor
    /// would still resolve into a line the document no longer holds.
    #[test]
    fn a_line_frozen_again_takes_its_first_rows_with_it() {
        let mut store = store();
        let mut document = HistoryDocument::default();
        let mut rows_of = BTreeMap::<TranscriptId, Vec<StagingId>>::new();
        for row in 0..3 {
            for finalized in store
                .capture(CapturedRow::plain(&format!("line {row}"), false))
                .finalized
            {
                rows_of.insert(
                    finalized.line.id,
                    finalized.mappings.iter().map(|m| m.staging_id).collect(),
                );
                document.finalize_transaction(finalized);
            }
        }
        let ids = document.entries().keys().copied().collect::<Vec<_>>();
        assert!(ids.len() >= 2, "the fixture froze lines: {ids:?}");
        let middle = ids[ids.len() - 2];
        let first_rows = rows_of[&middle].clone();
        assert!(!first_rows.is_empty(), "the line was frozen from rows");
        let staged = |id: StagingId| ContentAnchor::Staging {
            id,
            offset: GraphemeOffset(0),
            bias: Bias::After,
            generation: SourceGeneration(0),
        };
        for row in &first_rows {
            assert!(
                matches!(document.resolve_anchor(&staged(*row)), ContentAnchor::History { id, .. } if id == middle),
                "a row of the line resolves into it"
            );
        }

        document.finalize_transaction(frozen_again(middle, &store));
        for row in &first_rows {
            assert!(
                matches!(document.resolve_anchor(&staged(*row)), ContentAnchor::History { id, .. } if id == middle),
                "frozen again, the line is still where its first rows went"
            );
        }

        document.delete_transaction(&[middle], false, GridGeneration(1));
        for row in &first_rows {
            assert_eq!(
                document.resolve_anchor(&staged(*row)),
                staged(*row),
                "the line is gone, and a row it was first frozen from still resolves into it"
            );
        }
    }
}
