//! Layout-free history document, cross-plane anchors, and shared version types.

#![cfg_attr(test, allow(clippy::disallowed_methods))]
// Every `stderr` line goes to the writer that owns where `stderr` points, never written by the
// thread that said it (T-TRACE-SINK-OFF-THE-CALLING-THREAD): `bt-app`'s `diagnostics::stderr_line`, or
// the road `fn main` installs in a crate below it. `stdout` is the same file in a Log run, so no
// product line goes there either. Tests: `clippy.toml`'s `allow-print-in-tests`.
#![deny(clippy::print_stderr)]
#![deny(clippy::print_stdout)]

mod anchor;
mod document;
pub mod math;
pub mod svg;
mod versions;

pub use anchor::{
    AnchorError, AnchorId, Bias, ContentAnchor, GridPoint, ScreenId, Selection, compare_anchors,
    content_anchor_between,
};
pub use document::{HistoryDocument, HistoryEntry, LiveRowRemoval};
pub use versions::{
    BlockKind, DecorationIntent, DecorationLifecycle, DetectionRevision, GridGeneration,
    InlineMathSite, InlineRunPlacement, InvalidSourceTransition, LayoutKey, MathMode,
    SUBPIXELS_PER_PX, SourceLifecycle, VersionStamp, ViewGeneration,
};
