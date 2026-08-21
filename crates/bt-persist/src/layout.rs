//! The `split{dir,ratio}|leaf` tree persisted as part of `session.json`.
//!
//! docs/M2-persistence-schema-v1.md §3.2: "session.json 持久化的是**求解器的输入**
//! (树形状 + `ratio`)" — the runtime `id` on each split is not persisted (§3.2:
//! "`id` 字段…不持久化——它是运行时句柄,恢复时按树形状重新铸造"); only shape and
//! ratio survive to disk.

use serde::{Deserialize, Serialize};

/// Upper bound of [`SplitNodeV1::ratio`] — the ratio is the fraction (of the
/// split's first child) encoded as parts-per-million, so it is always a
/// `u32` in `0..=RATIO_PPM_MAX`.
///
/// docs/M2-persistence-schema-v1.md §3.2 explicitly defers the *solver's*
/// clamp algorithm and precision to `docs/M2-layout-solver-spec.md` (not yet
/// written), promising only that "`session.json` 里 `ratio` 就是一个 JSON
/// number". This crate pins the on-disk *representation* — an integer
/// parts-per-million count rather than a float — because floats are not
/// guaranteed to round-trip byte-for-byte through `serde_json`, which would
/// break the bit-stable round-trip this crate must guarantee (§4 of the
/// implementation brief). `RATIO_PPM_MAX` is therefore the boundary of what
/// this crate can honestly own: "is this a valid fraction at all", not the
/// solver's eventual min-pane-size clamp.
pub const RATIO_PPM_MAX: u32 = 1_000_000;

/// `split { "dir": "row" | "col", "ratio": number, "children": [node, node] }`
/// or `leaf { "kind": "term" | "files", ... }` — docs/M2-persistence-schema-v1.md §3.2.
///
/// Untagged: the wire format distinguishes the two shapes structurally (a
/// split has `dir`/`ratio`/`children`, a leaf has `kind`), matching the
/// literal JSON shown in the spec rather than introducing a wrapper
/// discriminant the spec does not document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LayoutNodeV1 {
    Split(SplitNodeV1),
    Leaf(LeafNodeV1),
}

/// `dir: "row" | "col"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirV1 {
    Row,
    Col,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SplitNodeV1 {
    pub dir: SplitDirV1,
    /// Parts-per-million; see [`RATIO_PPM_MAX`]. May be out of range after a
    /// hand-edit or a future writer with a wider notion of "valid" — see
    /// [`SessionV1::degrade_in_place`](crate::SessionV1::degrade_in_place),
    /// which clamps it back into range at read time (§5.4 "非法 `ratio`").
    pub ratio: u32,
    pub children: [Box<LayoutNodeV1>; 2],
}

/// `leaf { "kind": "term" | "files", ... }` — docs/M2-persistence-schema-v1.md §3.3/§3.4.
///
/// Internally tagged on `kind` so the wire format matches the spec's literal
/// JSON exactly (the tag sits alongside the leaf's own fields, not wrapping
/// them). [`LeafNodeV1::Unknown`] is the §5.4 "未知 `leaf.kind`" placeholder:
/// serde routes any `kind` value this build does not recognize here instead
/// of failing the whole document — that is the per-leaf degradation the
/// persistence brief calls for, and it falls naturally out of `#[serde(other)]`
/// rather than needing a hand-rolled dispatcher.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LeafNodeV1 {
    Term(TermLeafV1),
    Files(FilesLeafV1),
    /// `preview { "kind": "preview", "pinned": bool }`.
    ///
    /// docs/M2-persistence-schema-v1.md §3.2 lists only `term` and `files`,
    /// because it was written before the layout solver's seat family was
    /// ruled. `docs/M2-layout-solver-spec.md` §5 then put "the preview seat's
    /// `pinned` flag" in the durable column outright — a field that cannot be
    /// written is a ruling that cannot be kept, and the tree that
    /// `session.json` exists to carry is by §3.2's own words "the solver's
    /// input", whose leaf kinds are the solver's. This variant closes that gap
    /// in the direction the later, more specific document requires; it is
    /// additive, so every v1 document written before it still reads.
    ///
    /// No content field: red line L1 keeps content identity out of the layout
    /// tree, and *which file* the preview was showing is content. `pinned` is
    /// geometry — it decides whether a new preview reuses this seat (§1.3 tier
    /// ①) or opens a column beside it.
    Preview(PreviewLeafV1),
    /// A leaf kind this build does not recognize (written by a newer
    /// Folio). Read-time placeholder only — this crate never
    /// constructs it on purpose, and the consumer is expected to render it
    /// as an inert placeholder pane (§5.4: "占位 pane(可见但不可用)").
    #[serde(other)]
    Unknown,
}

/// `term { "kind": "term", "profile_id": ..., "cwd": ..., "manual_name": ... }`
/// — docs/M2-persistence-schema-v1.md §3.3.
///
/// `cwd` is carried through unvalidated: whether the path still exists is a
/// filesystem question, and per the implementation brief that check is *not*
/// this crate's job — it is surfaced to the consumer to degrade (fall back to
/// `HOME` and show a banner) at the point where a filesystem is actually
/// available to ask. Same for `profile_id`: this crate has no profile
/// registry to validate against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TermLeafV1 {
    pub profile_id: String,
    pub cwd: String,
    pub manual_name: Option<String>,
    /// **Where this pane's focus card aims its window**, in rows above the tail
    /// (`docs/DESIGN.md` §7.1.6b′, user ruling 2026-08-21). `0` is the tail.
    ///
    /// Durable on [`FilesLeafV1::view`]'s own footing and for its reason: this
    /// is a shape of the *pane* rather than a piece of its content, so red line
    /// L1 lets it in, and a reader who aims a card past an agent's status block
    /// every morning is being told the product did not notice.
    ///
    /// Defaulted, so every document written before v10 still reads. It is
    /// **not** skipped when zero, unlike `remotes_open`: the v9 → v10 step
    /// writes the key into every `term` leaf, and a writer that then dropped it
    /// again would make the migration's own output differ from what the next
    /// save produces.
    #[serde(default)]
    pub card_skip: u32,
}

/// `preview { "kind": "preview", "pinned": bool }` — see [`LeafNodeV1::Preview`]
/// for why this leaf exists and why it carries nothing but the flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewLeafV1 {
    pub pinned: bool,
}

/// `files { "kind": "files", "root": ..., "open": [...], "sel": ..., "width": number, "view": "files" | "git" }`
/// — docs/M2-persistence-schema-v1.md §3.4.
///
/// `open`/`sel` are opaque stable IDs (§3.4: reuses the node-stable-ID
/// mechanism from `docs/DESIGN.md` §7.1.3); this crate does not interpret
/// them, only stores and returns them unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilesLeafV1 {
    pub root: String,
    pub open: Vec<String>,
    pub sel: Option<String>,
    /// Logical pixels (matches `DESIGN.md`'s `FILES_W`/`FILES_W_MIN`
    /// constants, both whole pixel counts).
    pub width: u32,
    /// Which of the column's two pages was showing (R1, ruled 2026-08-15).
    ///
    /// **Durable, and that is the ruling.** A Files column is a *place's* view
    /// and the repository is the same place seen another way (mock-up 1577), so
    /// which way you were looking at it is the same kind of fact as which folders
    /// were expanded — a shape of the column, not a piece of its content, and red
    /// line L1 lets it in on exactly that footing. The alternative the mock-up
    /// happens to implement is a lazy JavaScript property that dies with the page;
    /// a user who works in the Git page all morning and finds a file tree every
    /// time the window reopens is being told the product did not notice.
    ///
    /// It is *not* gated on the "Git panel" setting. A column whose page was Git
    /// while the panel was on, and which comes back while it is off, shows the
    /// tree — the reader decides that, because it is the reader that knows the
    /// setting. What is written here stays what the user last chose, so turning
    /// the panel back on restores the page rather than resetting it.
    pub view: FilesViewV1,
    /// Whether the Git page's **REMOTES** sub-group is unfolded (T9, v2 ③).
    ///
    /// Durable on [`Self::view`]'s own footing and for its own reason: this is a
    /// shape of the column rather than a piece of its content, so red line L1
    /// lets it in, and a reader who works against a fork and finds the remotes
    /// folded away at every restart is being told the product did not notice.
    ///
    /// Defaulted and skipped when false, so every document written before this
    /// field still reads and every column that has never opened the sub-group
    /// still writes exactly the bytes it used to.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remotes_open: bool,
}

/// `view: "files" | "git"`.
///
/// Spelled like [`SplitDirV1`] and defaulted like it is not: this is the only
/// field of a leaf whose absence has a right answer. Every document written
/// before v7 was written by a build with one page, and one page is `Files`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilesViewV1 {
    #[default]
    Files,
    Git,
}

impl LayoutNodeV1 {
    /// Walks the tree, clamping any [`SplitNodeV1::ratio`] outside
    /// `0..=RATIO_PPM_MAX` back into range. Returns how many splits were
    /// touched and how many `Unknown` leaves were found, so the caller can
    /// decide whether to surface a degradation banner — see
    /// [`crate::DegradationReport`].
    ///
    /// This is the only per-leaf degradation this crate performs itself
    /// (§5.4 "非法 `ratio`" → clamp). Unknown leaf kinds are already
    /// resolved to [`LeafNodeV1::Unknown`] during parsing (see that
    /// variant's docs); invalid `cwd`/unknown `profile_id` are deliberately
    /// left untouched — see [`TermLeafV1`]'s docs.
    pub(crate) fn degrade_in_place(&mut self, report: &mut crate::DegradationReport) {
        match self {
            LayoutNodeV1::Split(split) => {
                if split.ratio > RATIO_PPM_MAX {
                    split.ratio = RATIO_PPM_MAX;
                    report.clamped_ratios += 1;
                }
                for child in &mut split.children {
                    child.degrade_in_place(report);
                }
            }
            LayoutNodeV1::Leaf(LeafNodeV1::Unknown) => {
                report.unknown_leaves += 1;
            }
            LayoutNodeV1::Leaf(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DegradationReport;

    fn leaf(kind: LeafNodeV1) -> Box<LayoutNodeV1> {
        Box::new(LayoutNodeV1::Leaf(kind))
    }

    fn term() -> LeafNodeV1 {
        LeafNodeV1::Term(TermLeafV1 {
            profile_id: "pwsh.exe".to_string(),
            cwd: "C:\\Users\\test".to_string(),
            manual_name: None,
            card_skip: 0,
        })
    }

    #[test]
    fn ratio_within_range_is_untouched() {
        let mut tree = LayoutNodeV1::Split(SplitNodeV1 {
            dir: SplitDirV1::Row,
            ratio: 500_000,
            children: [leaf(term()), leaf(term())],
        });
        let mut report = DegradationReport::default();
        tree.degrade_in_place(&mut report);
        assert_eq!(report.clamped_ratios, 0);
        let LayoutNodeV1::Split(split) = &tree else {
            unreachable!("tree was constructed as a split")
        };
        assert_eq!(split.ratio, 500_000);
    }

    #[test]
    fn illegal_ratio_clamps_to_max() {
        let mut tree = LayoutNodeV1::Split(SplitNodeV1 {
            dir: SplitDirV1::Col,
            ratio: 4_000_000,
            children: [leaf(term()), leaf(term())],
        });
        let mut report = DegradationReport::default();
        tree.degrade_in_place(&mut report);
        assert_eq!(report.clamped_ratios, 1);
        let LayoutNodeV1::Split(split) = &tree else {
            unreachable!("tree was constructed as a split")
        };
        assert_eq!(split.ratio, RATIO_PPM_MAX);
    }

    #[test]
    fn unknown_leaf_kind_parses_to_placeholder() {
        let raw = r#"{"kind":"quake_overlay","some_future_field":42}"#;
        let node: LeafNodeV1 = serde_json::from_str(raw).expect("unknown kind must still parse");
        assert_eq!(node, LeafNodeV1::Unknown);
    }

    #[test]
    fn nested_ratio_is_clamped_deep_in_the_tree() {
        let mut tree = LayoutNodeV1::Split(SplitNodeV1 {
            dir: SplitDirV1::Row,
            ratio: 300_000,
            children: [
                leaf(term()),
                Box::new(LayoutNodeV1::Split(SplitNodeV1 {
                    dir: SplitDirV1::Col,
                    ratio: u32::MAX,
                    children: [leaf(term()), leaf(term())],
                })),
            ],
        });
        let mut report = DegradationReport::default();
        tree.degrade_in_place(&mut report);
        assert_eq!(report.clamped_ratios, 1);
        let LayoutNodeV1::Split(outer) = &tree else {
            unreachable!("tree was constructed as a split")
        };
        let LayoutNodeV1::Split(inner) = outer.children[1].as_ref() else {
            unreachable!("second child was constructed as a split")
        };
        assert_eq!(inner.ratio, RATIO_PPM_MAX);
    }
}
