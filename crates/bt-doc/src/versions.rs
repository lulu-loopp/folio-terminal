use std::num::NonZeroU32;

use bt_transcript::SourceGeneration;
use thiserror::Error;

/// Fixed-point denominator shared by detection artifacts and viewport layout.
pub const SUBPIXELS_PER_PX: i64 = 1024;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MathMode {
    Display,
    Inline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceLifecycle {
    Live,
    Frozen,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("invalid source transition from {from:?} to {to:?}")]
pub struct InvalidSourceTransition {
    pub from: SourceLifecycle,
    pub to: SourceLifecycle,
}

impl SourceLifecycle {
    pub fn transition(&mut self, next: Self) -> Result<(), InvalidSourceTransition> {
        if (*self, next) != (Self::Live, Self::Frozen) {
            return Err(InvalidSourceTransition {
                from: *self,
                to: next,
            });
        }
        *self = next;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecorationIntent {
    Plain,
    Math {
        byte_start: u32,
        byte_end: u32,
        mode: MathMode,
        detection_revision: DetectionRevision,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecorationLifecycle {
    None,
    Pending,
    Ready,
    Failed,
    Suppressed,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DetectionRevision(pub u64);

/// DESIGN.md §3.3 layout invalidation key. Every field participates in cache identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LayoutKey {
    pub width_cells: NonZeroU32,
    pub dpi_milli: NonZeroU32,
    pub font_rev: u64,
    pub theme_rev: u64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GridGeneration(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ViewGeneration(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct VersionStamp {
    pub source: SourceGeneration,
    pub detection: DetectionRevision,
    pub layout: LayoutKey,
    pub view: ViewGeneration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn illegal_source_transition_is_observable() {
        let mut source = SourceLifecycle::Frozen;
        assert_eq!(
            source.transition(SourceLifecycle::Frozen),
            Err(InvalidSourceTransition {
                from: SourceLifecycle::Frozen,
                to: SourceLifecycle::Frozen,
            })
        );
    }
}
