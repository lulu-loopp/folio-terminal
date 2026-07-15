//! Conservative block-level `$$...$$` detection and the dual lifecycle/version gate.

use bt_doc::{DecorationIntent, HistoryDocument};
pub use bt_doc::{
    DecorationLifecycle, DetectionRevision, LayoutKey, SUBPIXELS_PER_PX, SourceLifecycle,
    VersionStamp, ViewGeneration,
};
use bt_transcript::{SourceGeneration, TranscriptId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MathSpan {
    pub byte_start: u32,
    pub byte_end: u32,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaceholderArtifact {
    pub key: String,
    pub height_subpixels: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetectionTask {
    pub transcript_id: TranscriptId,
    pub span: MathSpan,
    pub versions: VersionStamp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecorationRecord {
    pub source: SourceLifecycle,
    pub decoration: DecorationLifecycle,
    pub versions: VersionStamp,
    pub artifact: Option<PlaceholderArtifact>,
}

impl DecorationRecord {
    pub fn frozen(versions: VersionStamp) -> Self {
        Self {
            source: SourceLifecycle::Frozen,
            decoration: DecorationLifecycle::None,
            versions,
            artifact: None,
        }
    }

    pub fn schedule(
        &mut self,
        transcript_id: TranscriptId,
        span: MathSpan,
    ) -> Option<DetectionTask> {
        if self.source != SourceLifecycle::Frozen || self.decoration != DecorationLifecycle::None {
            return None;
        }
        self.decoration = DecorationLifecycle::Pending;
        Some(DetectionTask {
            transcript_id,
            span,
            versions: self.versions,
        })
    }

    /// Worker results are never rewritten; every relevant version must still match.
    pub fn complete(&mut self, task: &DetectionTask, artifact: PlaceholderArtifact) -> bool {
        if self.source != SourceLifecycle::Frozen
            || self.decoration != DecorationLifecycle::Pending
            || task.versions != self.versions
        {
            return false;
        }
        self.artifact = Some(artifact);
        self.decoration = DecorationLifecycle::Ready;
        true
    }

    pub fn source_changed(&mut self, generation: SourceGeneration) {
        self.versions.source = generation;
        self.artifact = None;
        self.decoration = DecorationLifecycle::None;
    }

    pub fn detector_changed(&mut self, revision: DetectionRevision) {
        self.versions.detection = revision;
        self.artifact = None;
        self.decoration = DecorationLifecycle::None;
    }

    pub fn layout_changed(&mut self, layout: LayoutKey) {
        self.versions.layout = layout;
        self.artifact = None;
        if self.decoration != DecorationLifecycle::Suppressed {
            self.decoration = DecorationLifecycle::None;
        }
    }

    pub fn view_changed(&mut self, view: ViewGeneration) {
        self.versions.view = view;
        if self.decoration == DecorationLifecycle::Pending {
            self.decoration = DecorationLifecycle::None;
        }
    }

    pub fn suppress(&mut self) {
        if self.source == SourceLifecycle::Frozen {
            self.decoration = DecorationLifecycle::Suppressed;
            self.artifact = None;
        }
    }
}

/// Detection is the owner of intent rebuilding. A viewport may only consume the resulting
/// revision; it must not impersonate redetection by clearing layout entries alone.
pub fn redetect_document(
    document: &mut HistoryDocument,
    revision: DetectionRevision,
) -> Vec<(TranscriptId, MathSpan)> {
    document.clear_decorations();
    let mut detected = Vec::new();
    let inputs = document
        .entries()
        .iter()
        .map(|(id, entry)| (*id, entry.line.text.clone()))
        .collect::<Vec<_>>();
    for (id, text) in inputs {
        if let Some(span) = detect_block_math(&text).into_iter().next() {
            document.set_decoration(
                id,
                DecorationIntent::Math {
                    byte_start: span.byte_start,
                    byte_end: span.byte_end,
                    detection_revision: revision,
                },
            );
            detected.push((id, span));
        }
    }
    detected
}

pub fn detect_block_math(text: &str) -> Vec<MathSpan> {
    let mut spans = Vec::new();
    let mut cursor = 0;
    while let Some(open_rel) = text[cursor..].find("$$") {
        let open = cursor + open_rel;
        let content_start = open + 2;
        let Some(close_rel) = text[content_start..].find("$$") else {
            break;
        };
        let close = content_start + close_rel;
        if close > content_start {
            spans.push(MathSpan {
                byte_start: open as u32,
                byte_end: (close + 2) as u32,
                source: text[content_start..close].to_string(),
            });
        }
        cursor = close + 2;
    }
    spans
}

pub fn render_placeholder(task: &DetectionTask) -> PlaceholderArtifact {
    PlaceholderArtifact {
        key: format!(
            "math:{}:{}:{}",
            task.transcript_id.0, task.span.byte_start, task.versions.detection.0
        ),
        height_subpixels: 64 * SUBPIXELS_PER_PX,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_transcript::{CapturedRow, TranscriptStore};
    use std::{num::NonZeroU32, num::NonZeroUsize};

    fn nz32(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).unwrap()
    }

    fn stamp() -> VersionStamp {
        VersionStamp {
            source: SourceGeneration(1),
            detection: DetectionRevision(1),
            layout: LayoutKey {
                width_cells: nz32(80),
                dpi_milli: nz32(1000),
                font_rev: 1,
                theme_rev: 1,
            },
            view: ViewGeneration(1),
        }
    }

    #[test]
    fn only_closed_block_delimiters_are_detected() {
        let spans = detect_block_math("$PATH $$x^2$$ tail $$broken");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].source, "x^2");
    }

    #[test]
    fn stale_worker_generation_is_discarded_without_leak() {
        let span = detect_block_math("$$x$$").remove(0);
        let mut record = DecorationRecord::frozen(stamp());
        let task = record.schedule(TranscriptId(1), span).unwrap();
        record.source_changed(SourceGeneration(2));
        assert!(!record.complete(&task, render_placeholder(&task)));
        assert!(record.artifact.is_none());
    }

    #[test]
    fn four_versions_have_distinct_invalidation_boundaries() {
        let span = detect_block_math("$$x$$").remove(0);
        let mut record = DecorationRecord::frozen(stamp());
        let task = record.schedule(TranscriptId(1), span.clone()).unwrap();
        assert!(record.complete(&task, render_placeholder(&task)));

        let source_before = record.versions.source;
        record.layout_changed(LayoutKey {
            width_cells: nz32(40),
            ..stamp().layout
        });
        assert_eq!(record.versions.source, source_before);
        assert_eq!(record.decoration, DecorationLifecycle::None);

        let old_detection_task = record.schedule(TranscriptId(1), span.clone()).unwrap();
        record.detector_changed(DetectionRevision(2));
        assert!(!record.complete(&old_detection_task, render_placeholder(&old_detection_task)));
        assert_eq!(record.decoration, DecorationLifecycle::None);

        let view_task = record.schedule(TranscriptId(1), span).unwrap();
        record.view_changed(ViewGeneration(2));
        assert!(!record.complete(&view_task, render_placeholder(&view_task)));

        record.suppress();
        assert_eq!(record.source, SourceLifecycle::Frozen);
        assert_eq!(record.decoration, DecorationLifecycle::Suppressed);
    }

    #[test]
    fn redetection_revision_is_recorded_in_rebuilt_intent() {
        let mut store = TranscriptStore::new(NonZeroUsize::new(8).unwrap());
        let finalized = store
            .capture(CapturedRow::plain("$$x$$", false))
            .finalized
            .remove(0);
        let id = finalized.line.id;
        let mut document = HistoryDocument::default();
        document.finalize_transaction(finalized);

        redetect_document(&mut document, DetectionRevision(7));
        assert!(matches!(
            document.entries()[&id].decoration,
            DecorationIntent::Math {
                detection_revision: DetectionRevision(7),
                ..
            }
        ));
        redetect_document(&mut document, DetectionRevision(8));
        assert!(matches!(
            document.entries()[&id].decoration,
            DecorationIntent::Math {
                detection_revision: DetectionRevision(8),
                ..
            }
        ));
    }
}
