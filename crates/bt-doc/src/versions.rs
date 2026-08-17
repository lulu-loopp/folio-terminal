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

/// Where one inline `$…$` run sits inside the composite raster its logical line rendered to.
///
/// A line may carry several runs. They are rasterized one at a time and composited into a single
/// image at per-run x offsets, so the composite alone cannot say which run is which — and three
/// separate questions need exactly that: which terminal cells to blank, which run the pointer is
/// over, and which run's LaTeX the copy button puts on the clipboard.
///
/// It also records the *outcome*: a run whose raster is wider than its own source cells falls back
/// to source by itself and is simply absent from this list, leaving its neighbours composited and
/// its own terminal text untouched.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InlineRunPlacement {
    /// Index into the span's `inline_runs`. This is the run's identity: stable across relayout,
    /// and the value an anchor carries so a copy resolves to one formula rather than the line.
    pub run: u32,
    /// Left edge inside the composite raster, in raster pixels.
    pub x_px: u32,
    pub width_px: u32,
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
    /// How many times the window's **language** has moved (§7.1.6c-3c).
    ///
    /// The last of the four to arrive, and the one whose absence was an
    /// architectural fact rather than an oversight: `crates/bt-app/src/i18n.rs`
    /// held the language in a `OnceLock` precisely because there was no number
    /// here for a switch to advance, and named this field as the thing that had
    /// to exist first. It exists now, so the language is a member of the layout's
    /// identity in the same way the palette is — an artefact measured, typeset or
    /// rastered while the window spoke one language can no longer be handed back
    /// after it has started speaking another.
    pub lang_rev: u64,
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

    /// PIN (§7.1.6c-3c) — **the language is part of the key**, by equality and
    /// by hash alike.
    ///
    /// Both halves are load-bearing and they fail differently. Equality is what
    /// `DualPlaneSession::set_layout_key` compares to decide that a re-layout is
    /// owed; the hash is what the math texture cache looks entries up by. A
    /// field that participated in one and not the other would give a window that
    /// correctly decided to re-typeset and was then handed the old raster back.
    #[test]
    fn a_language_change_is_a_different_layout_key() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let base = LayoutKey {
            width_cells: NonZeroU32::new(80).unwrap(),
            dpi_milli: NonZeroU32::new(1000).unwrap(),
            font_rev: 1,
            theme_rev: 1,
            lang_rev: 0,
        };
        let switched = LayoutKey {
            lang_rev: 1,
            ..base
        };
        assert_ne!(base, switched);

        let hash_of = |key: LayoutKey| {
            let mut hasher = DefaultHasher::new();
            key.hash(&mut hasher);
            hasher.finish()
        };
        assert_ne!(hash_of(base), hash_of(switched));
    }
}
