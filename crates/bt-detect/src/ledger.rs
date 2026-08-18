//! Token-ownership ledger and the split source-integrity / detector-containment red gates.
//!
//! Batch ⑥ of the formula-detection rearchitecture (Codex plan review §2.2 ownership ledger, §3.2
//! decidable red gate; 3rd-round audit `stream-mispair-3rd-audit.md` 补法). The ledger is pure
//! instrumentation: it records, for every *structural* display delimiter the authoritative scanner
//! toggles on (`$$`, `\[`, `\]`, `\begin{env}`, `\end{env}`), which fate that same scanner already
//! assigned — owned by a detected block, one of the enumerated *legitimate* rejections, or an
//! unaccounted *orphan*. It never re-derives detection with a second heuristic: the recorder is
//! threaded through the real `scan_math_blocks_impl` and is `None` on every product path, so product
//! behaviour is byte-identical (the ledger only ever observes).
//!
//! The red gate is split into two layers so a fixed recording that still contains genuine upstream
//! byte damage can go green:
//!   * **source-integrity** — byte-level upstream damage (e.g. a compression reflow that ate a `$$`
//!     opener). Annotated precisely per recording; reported, never release-blocking. Not our failure.
//!   * **detector-containment** — the detector must *isolate* annotated damage: it may not let one
//!     damaged delimiter desync downstream blocks that could otherwise pair. Any **unannotated**
//!     orphan is a containment failure and reds the exit. This is the standing guard batch 2 must
//!     drive to zero.

use crate::{DelimiterKind, LiveDetectionInput, LiveDetectionSource, MathSourceLine};

/// A structural display delimiter — one that owns its line at `delimiter_start` and drives the
/// scanner's open/close toggle. Inline `$…$` runs are never structural and never enter the ledger.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StructuralDelimiterKind {
    /// `$$` — the same token opens or closes depending on parser phase.
    Dollars,
    /// `\[`
    BracketOpen,
    /// `\]`
    BracketClose,
    /// `\begin{env}`
    EnvironmentOpen(String),
    /// `\end{env}`
    EnvironmentClose(String),
}

/// The enumerated legitimate reasons a structural delimiter is accounted without owning a rendered
/// block. Each variant is one decidable path the scanner already took (review §3.2); the ledger
/// attaches the reason at that exact site, it does not invent one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LegitimateRejection {
    /// A single trailing opener still active when the reconstructed live region ends: a block still
    /// streaming, legitimately unclosed. At most one per contiguous region.
    StreamingUnclosedTail,
    /// A closer consumed for an opening whose opener is above the scanned window (carried by the
    /// initial parser context, `start_index == None`) — the frozen bridge / row-0 carry case. The
    /// opener is owned by an existing occurrence, not by this window.
    OpenerAboveWindow,
    /// The delimiter sits inside a CommonMark fenced or indented code context; it is inert text.
    CommonMarkCodeContext,
    /// M1.9p: a symmetric multi-line `$$` opener refused under an `Ambiguous` prefix (scrollback
    /// first-detection NO-GO). Recorded as a known refusal, never an orphan.
    AmbiguousPrefixSuppressed,
    /// A paired span whose joined body failed `valid_display_body` on its own content — empty body,
    /// prose, CJK prose, oversize, or the Jump-chip overlay. Parity-neutral: both delimiters drop
    /// together.
    GuardRejectedBody,
    /// The frozen→live parity resync abandoned a stale frozen `$$` opener (lost-opener phantom,
    /// `3da6d64` / `3875209`). This is that abandoned opener.
    PhantomOpenerAbandoned,
    /// The environment swallow-radius bound abandoned a stale unclosed `\begin{env}` when a display
    /// opener appeared in its body (`89ed339`). This is that abandoned environment opener.
    EnvironmentSwallowAbandoned,
}

/// Why a structural delimiter is an orphan (unaccounted). Both kinds are containment failures when
/// unannotated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OrphanKind {
    /// The first grid `$$` is really the *closer* of a block whose opener scrolled above grid row 0,
    /// yet no opener was carried into the grid (the parser reached the seam in the closed phase).
    /// Reading it as a fresh opener desyncs every block below it. General and hold-independent — the
    /// exact round-3 topology `isolation_gap` is structurally blind to.
    ClippedOpen,
    /// A structural `$$` the scanner consumed without producing a detected block or any enumerated
    /// legitimate account — a lone unmatched delimiter (odd parity in the rendered region).
    UnbalancedResidue,
}

/// The fate the authoritative scanner assigned to one structural delimiter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenFate {
    /// Consumed as the opener or closer of a detected block. `block_start`/`block_end` are the
    /// block's logical-line indices within the scanned window.
    Owned { block_start: u32, block_end: u32 },
    /// Accounted without a rendered block, for an enumerated legitimate reason.
    Rejected(LegitimateRejection),
    /// Unaccounted. A containment failure unless a source-integrity annotation covers it.
    Orphan(OrphanKind),
}

/// One structural delimiter and its fate, with the batch-2 interface fields populated but unused
/// this batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerEntry {
    /// Lineage (世系, §2.2): the reconstructed source row this delimiter came from — a frozen
    /// transcript id or a live-grid row. Populated by the entry point from the input sources.
    pub source_line: Option<MathSourceLine>,
    /// Logical-line index within the scanned window (grid+frozen reconstruction).
    pub logical_index: u32,
    /// UTF-8 byte offset of the delimiter's first byte on its logical line.
    pub byte_offset: u32,
    pub kind: StructuralDelimiterKind,
    pub fate: TokenFate,
    /// Dependency interval (依赖区间, §1.2/§2.2): the inclusive logical-index span whose parser
    /// state this token's fate depended on. Batch 2 keys interval invalidation off this so a rewrite
    /// inside `[dependency_start, dependency_end]` retires exactly the affected ledger entries. This
    /// batch only records it.
    pub dependency_start: u32,
    pub dependency_end: u32,
}

/// A precise per-recording note that a structural delimiter (or its absence) is upstream byte damage
/// — not a detector failure. Matches a ledger orphan by source row so the containment gate can
/// tolerate that specific orphan while still redding any *other* (spilled, unannotated) orphan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceIntegrityAnnotation {
    /// The reconstructed source row known to carry (or to be missing) an upstream-damaged delimiter.
    pub source_line: MathSourceLine,
    pub note: String,
}

/// The detector-containment verdict for one ledger, given the recording's source-integrity
/// annotations. `red` is the release-blocking exit signal.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContainmentVerdict {
    /// Distinct detected blocks (owned delimiter pairs).
    pub detected: usize,
    /// Legitimately rejected structural delimiters.
    pub legitimate_rejections: usize,
    /// Orphans covered by a source-integrity annotation — reported, tolerated.
    pub annotated_damage: usize,
    /// Orphans NOT covered by any annotation — the containment failure count.
    pub orphans: usize,
    /// Whether a clipped-open orphan was found (the round-3 topology).
    pub clipped_open: bool,
    /// `orphans > 0`. Reds the oracle exit.
    pub red: bool,
}

/// The recorded ledger for one scan of a reconstructed live region.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OwnershipLedger {
    pub entries: Vec<LedgerEntry>,
    /// The frozen→live seam: first grid logical index, if the window spans a frozen prefix.
    pub boundary: Option<u32>,
    /// `original_source` of every display block the authoritative scan Owned this pass, in
    /// detection order — one string per `TokenFate::Owned` pair (batch ③). This is the exact key the
    /// presentation layer preserves holds on, so a still-displayed hold is *backed* iff its source
    /// appears here and *`HeldUnbacked`* (the detector no longer accounts the block a hold is showing)
    /// when it does not. Filled by `live_detection_ownership_ledger` from the same scan the recorder
    /// observed; empty on a recorder that was finalized without it.
    pub owned_block_sources: Vec<String>,
}

impl OwnershipLedger {
    pub fn detected(&self) -> usize {
        self.entries
            .iter()
            .filter_map(|entry| match &entry.fate {
                TokenFate::Owned {
                    block_start,
                    block_end,
                } => Some((*block_start, *block_end)),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    }

    pub fn legitimate_rejections(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.fate, TokenFate::Rejected(_)))
            .count()
    }

    pub fn orphan_entries(&self) -> impl Iterator<Item = &LedgerEntry> {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.fate, TokenFate::Orphan(_)))
    }

    pub fn orphans(&self) -> usize {
        self.orphan_entries().count()
    }

    /// Whether the authoritative scan currently Owns a display block with this exact
    /// `original_source`. Batch ③: the presentation layer preserves a hold by exact source equality,
    /// so a still-displayed record whose source this returns `false` for is a `HeldUnbacked` strand —
    /// shown by a hold while detection no longer accounts it.
    pub fn owns_source(&self, original_source: &str) -> bool {
        self.owned_block_sources
            .iter()
            .any(|source| source == original_source)
    }

    /// Split the orphans by whether a source-integrity annotation covers them, and produce the
    /// containment verdict. An orphan is tolerated only when an annotation names its exact source
    /// row; every other orphan reds the gate.
    pub fn containment(&self, annotations: &[SourceIntegrityAnnotation]) -> ContainmentVerdict {
        let mut annotated = 0usize;
        let mut orphans = 0usize;
        let mut clipped_open = false;
        for entry in self.orphan_entries() {
            if matches!(entry.fate, TokenFate::Orphan(OrphanKind::ClippedOpen)) {
                clipped_open = true;
            }
            let covered = entry.source_line.is_some_and(|line| {
                annotations
                    .iter()
                    .any(|annotation| annotation.source_line == line)
            });
            if covered {
                annotated += 1;
            } else {
                orphans += 1;
            }
        }
        ContainmentVerdict {
            detected: self.detected(),
            legitimate_rejections: self.legitimate_rejections(),
            annotated_damage: annotated,
            orphans,
            clipped_open,
            red: orphans > 0,
        }
    }
}

/// Scanner-facing recorder. `scan_math_blocks_impl` calls these methods at the exact sites it toggles
/// the open/close state; a `None` recorder (every product path) makes each call a no-op. The recorder
/// mirrors the scanner's single active-opening slot so it can finalize a pending opener when it is
/// owned, rejected, abandoned, or left as a streaming tail.
#[derive(Debug, Default)]
pub struct OwnershipRecorder {
    entries: Vec<RawEntry>,
    /// The single active opener the scanner is tracking, if it originates inside this window. A
    /// carried opener (from the initial context, `start_index == None`) is `Some(Pending{ entry:
    /// None, .. })` so its closer can still be recorded as `OpenerAboveWindow`.
    pending: Option<Pending>,
}

#[derive(Debug)]
struct Pending {
    /// Index into `entries` of the opener delimiter, or `None` when the opener is above the window.
    entry: Option<usize>,
    /// Opener logical index if it is inside the window.
    open_index: Option<usize>,
}

#[derive(Debug)]
struct RawEntry {
    logical_index: u32,
    byte_offset: u32,
    kind: StructuralDelimiterKind,
    fate: RawFate,
    dependency_start: u32,
    dependency_end: u32,
}

#[derive(Debug)]
enum RawFate {
    Owned { block_start: u32, block_end: u32 },
    Rejected(LegitimateRejection),
    Orphan(OrphanKind),
}

impl OwnershipRecorder {
    /// Seed the recorder with an opening carried in from the initial parser context (its opener is
    /// above the window). No entry is created; a later closer records `OpenerAboveWindow`.
    pub(crate) fn seed_carried_opening(&mut self) {
        self.pending = Some(Pending {
            entry: None,
            open_index: None,
        });
    }

    /// A fresh opener began at `index`.
    pub(crate) fn open(&mut self, index: usize, byte: usize, kind: StructuralDelimiterKind) {
        let entry = self.entries.len();
        self.entries.push(RawEntry {
            logical_index: index as u32,
            byte_offset: byte as u32,
            kind,
            // Overwritten when the opener is resolved; a never-resolved opener is a streaming tail.
            fate: RawFate::Rejected(LegitimateRejection::StreamingUnclosedTail),
            dependency_start: index as u32,
            dependency_end: index as u32,
        });
        self.pending = Some(Pending {
            entry: Some(entry),
            open_index: Some(index),
        });
    }

    /// The active opening closed into a detected block spanning `[block_start, block_end]`.
    pub(crate) fn close_owned(
        &mut self,
        closer_index: usize,
        closer_byte: usize,
        closer_kind: StructuralDelimiterKind,
        block_start: usize,
        block_end: usize,
    ) {
        let (bs, be) = (block_start as u32, block_end as u32);
        if let Some(pending) = self.pending.take()
            && let Some(entry) = pending.entry
        {
            self.entries[entry].fate = RawFate::Owned {
                block_start: bs,
                block_end: be,
            };
            self.entries[entry].dependency_start = bs;
            self.entries[entry].dependency_end = be;
        }
        self.entries.push(RawEntry {
            logical_index: closer_index as u32,
            byte_offset: closer_byte as u32,
            kind: closer_kind,
            fate: RawFate::Owned {
                block_start: bs,
                block_end: be,
            },
            dependency_start: bs,
            dependency_end: be,
        });
    }

    /// The active opening's closer was consumed but the joined body failed `valid_display_body`;
    /// both delimiters are legitimately rejected as a self-contained non-block.
    pub(crate) fn close_rejected(
        &mut self,
        closer_index: usize,
        closer_byte: usize,
        closer_kind: StructuralDelimiterKind,
        reason: LegitimateRejection,
    ) {
        let pending = self.pending.take();
        let (dep_start, dep_end) = match &pending {
            Some(Pending {
                open_index: Some(open),
                ..
            }) => (*open as u32, closer_index as u32),
            _ => (closer_index as u32, closer_index as u32),
        };
        if let Some(Pending {
            entry: Some(entry), ..
        }) = pending
        {
            self.entries[entry].fate = RawFate::Rejected(reason.clone());
            self.entries[entry].dependency_start = dep_start;
            self.entries[entry].dependency_end = dep_end;
        }
        // A closer for an above-window opener (`OpenerAboveWindow`) has no opener entry; the closer
        // itself carries the reason.
        self.entries.push(RawEntry {
            logical_index: closer_index as u32,
            byte_offset: closer_byte as u32,
            kind: closer_kind,
            fate: RawFate::Rejected(reason),
            dependency_start: dep_start,
            dependency_end: dep_end,
        });
    }

    /// The active opening lost its closer to a self-contained block on this line: it is genuinely
    /// unpaired odd-parity residue, not a legitimate rejection. A no-op when the opener is above the
    /// window (nothing was recorded for it).
    pub(crate) fn orphan_pending(&mut self) {
        if let Some(Pending {
            entry: Some(entry), ..
        }) = self.pending.take()
        {
            self.entries[entry].fate = RawFate::Orphan(OrphanKind::UnbalancedResidue);
        }
    }

    /// The active opening was abandoned in place (resync phantom or environment swallow bound); the
    /// delimiter that triggered the abandonment is recorded separately by the caller.
    pub(crate) fn abandon_pending(&mut self, reason: LegitimateRejection) {
        if let Some(Pending {
            entry: Some(entry),
            open_index,
        }) = self.pending.take()
        {
            self.entries[entry].fate = RawFate::Rejected(reason);
            if let Some(open) = open_index {
                self.entries[entry].dependency_start = open as u32;
                self.entries[entry].dependency_end = open as u32;
            }
        }
    }

    /// A self-contained block on one line: two delimiters, owned together (or rejected together).
    pub(crate) fn self_contained(
        &mut self,
        index: usize,
        open_byte: usize,
        close_byte: usize,
        open_kind: StructuralDelimiterKind,
        close_kind: StructuralDelimiterKind,
        owned: bool,
    ) {
        let idx = index as u32;
        let fate = |owned: bool| {
            if owned {
                RawFate::Owned {
                    block_start: idx,
                    block_end: idx,
                }
            } else {
                RawFate::Rejected(LegitimateRejection::GuardRejectedBody)
            }
        };
        self.entries.push(RawEntry {
            logical_index: idx,
            byte_offset: open_byte as u32,
            kind: open_kind,
            fate: fate(owned),
            dependency_start: idx,
            dependency_end: idx,
        });
        self.entries.push(RawEntry {
            logical_index: idx,
            byte_offset: close_byte as u32,
            kind: close_kind,
            fate: fate(owned),
            dependency_start: idx,
            dependency_end: idx,
        });
    }

    /// A single structural delimiter rejected on its own line for a non-pairing reason
    /// (M1.9p-suppressed opener, or a delimiter inside a code context).
    pub(crate) fn reject_single(
        &mut self,
        index: usize,
        byte: usize,
        kind: StructuralDelimiterKind,
        reason: LegitimateRejection,
    ) {
        let idx = index as u32;
        self.entries.push(RawEntry {
            logical_index: idx,
            byte_offset: byte as u32,
            kind,
            fate: RawFate::Rejected(reason),
            dependency_start: idx,
            dependency_end: idx,
        });
    }

    /// Finalize: consume this into an `OwnershipLedger`, mapping logical indices to source lineage
    /// and computing the clipped-open orphan reclassification. `id_to_logical` is unused directly;
    /// `source_of` resolves a logical index to its reconstructed source row, and `clipped_open_index`
    /// (if any) is the logical index whose first-grid `$$` is a clipped-open orphan.
    pub(crate) fn finish(
        self,
        boundary: Option<usize>,
        source_of: impl Fn(u32) -> Option<MathSourceLine>,
        clipped_open_index: Option<u32>,
    ) -> OwnershipLedger {
        let entries = self
            .entries
            .into_iter()
            .map(|raw| {
                let mut fate = match raw.fate {
                    RawFate::Owned {
                        block_start,
                        block_end,
                    } => TokenFate::Owned {
                        block_start,
                        block_end,
                    },
                    RawFate::Rejected(reason) => TokenFate::Rejected(reason),
                    RawFate::Orphan(kind) => TokenFate::Orphan(kind),
                };
                // The clip-aware scanner (batch 2) consumes a clipped-open closer as an above-window
                // closer (`OpenerAboveWindow`), containing the round-3 topology: the grid re-pairs
                // cleanly and no orphan spills. When it has done so, that fate is authoritative — keep
                // it. This reclassification is only the defensive fallback for a path that computed
                // `clipped_open_index` for the ledger but scanned WITHOUT the clip evidence (so the
                // recorder filed the delimiter as a streaming tail or a spurious-forward-pair
                // rejection): surface it as the clipped-open orphan so the containment gate still sees
                // an unhandled round-3 clip. Never overrides a genuinely owned block, nor a contained
                // above-window closer.
                if clipped_open_index == Some(raw.logical_index)
                    && matches!(raw.kind, StructuralDelimiterKind::Dollars)
                    && !matches!(fate, TokenFate::Owned { .. })
                    && !matches!(
                        fate,
                        TokenFate::Rejected(LegitimateRejection::OpenerAboveWindow)
                    )
                {
                    fate = TokenFate::Orphan(OrphanKind::ClippedOpen);
                }
                LedgerEntry {
                    source_line: source_of(raw.logical_index),
                    logical_index: raw.logical_index,
                    byte_offset: raw.byte_offset,
                    kind: raw.kind,
                    fate,
                    dependency_start: raw.dependency_start,
                    dependency_end: raw.dependency_end,
                }
            })
            .collect();
        OwnershipLedger {
            entries,
            boundary: boundary.map(|b| b as u32),
            // Filled by `live_detection_ownership_ledger` from the same scan's detected blocks; the
            // recorder itself never sees block source text.
            owned_block_sources: Vec::new(),
        }
    }
}

/// Map a `DelimiterKind` plus phase to a structural ledger kind. `opening` distinguishes `\[`/`\]`
/// and `\begin`/`\end`; `$$` is phase-agnostic.
pub(crate) fn structural_kind(kind: &DelimiterKind, opening: bool) -> StructuralDelimiterKind {
    match kind {
        DelimiterKind::Dollars => StructuralDelimiterKind::Dollars,
        DelimiterKind::Brackets => {
            if opening {
                StructuralDelimiterKind::BracketOpen
            } else {
                StructuralDelimiterKind::BracketClose
            }
        }
        // A table never reaches here: the ledger accounts for structural `$$`/`\[`/`egin`
        // tokens the display scanner paired or refused, and a table's `|---|` row is neither
        // paired nor refused — it is read once, in one place, by `table::table_at`. `Dollars` is
        // the inert answer, chosen because a delimiter kind that cannot occur must still not
        // invent a ledger category nobody counts.
        DelimiterKind::Table => StructuralDelimiterKind::Dollars,
        DelimiterKind::Environment(name) => {
            if opening {
                StructuralDelimiterKind::EnvironmentOpen(name.clone())
            } else {
                StructuralDelimiterKind::EnvironmentClose(name.clone())
            }
        }
    }
}

/// Resolve a reconstructed logical-line index to its source lineage, given the live inputs and the
/// logical→input fragment mapping the caller built. The first input fragment of a logical line
/// determines its source row (History id or Grid row).
pub(crate) fn source_line_of(input: &LiveDetectionInput) -> MathSourceLine {
    match input.source {
        LiveDetectionSource::History { id } => MathSourceLine::Transcript(id),
        LiveDetectionSource::Grid { row, .. } => MathSourceLine::LiveGrid(row),
    }
}
