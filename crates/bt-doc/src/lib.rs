//! Layout-free history document, cross-plane anchors, and shared version types.

mod anchor;
mod document;
mod versions;

pub use anchor::{
    AnchorError, AnchorId, Bias, ContentAnchor, GridPoint, ScreenId, Selection, compare_anchors,
    content_anchor_between,
};
pub use document::{HistoryDocument, HistoryEntry, LiveRowRemoval};
pub use versions::{
    DecorationIntent, DecorationLifecycle, DetectionRevision, GridGeneration,
    InvalidSourceTransition, LayoutKey, MathMode, SUBPIXELS_PER_PX, SourceLifecycle, VersionStamp,
    ViewGeneration,
};
