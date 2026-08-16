//! `session.json` v1 — docs/M2-persistence-schema-v1.md §3.

use serde::{Deserialize, Serialize};

use crate::layout::LayoutNodeV1;

/// Current `schema_version` for `session.json`.
///
/// v6 is the first bump that changes an existing field's *meaning* rather than adding one:
/// `profile_id` becomes a stable profile slug. See [`crate::migrate`]'s `migrate_session_v5_to_v6`
/// for the ruling and the one-time mapping. v7 adds `view` to a `files` leaf — which of the
/// column's two pages it was showing (R1).
pub const SESSION_SCHEMA_VERSION: u32 = 7;

/// Persisted theme mode restored with the session. `System` is resolved by the app against winit's
/// OS theme; `BT_BG` remains a process diagnostic override and is never persisted as a mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionThemeV1 {
    #[default]
    Dark,
    Light,
    System,
}

/// Focused cursor shape restored with the session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionCursorStyleV1 {
    #[default]
    Bar,
    Block,
    Underline,
}

/// Where the tab strip rests: along the top edge, or down the side as a column.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionTabLayoutV1 {
    #[default]
    Horizontal,
    Vertical,
}

/// Resting width of the vertical sidebar: full labels, or collapsed to its icon rail.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionSidebarModeV1 {
    #[default]
    Expanded,
    Icons,
}

/// `session.json` v1 top-level structure — docs/M2-persistence-schema-v1.md §3.5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionV1 {
    pub schema_version: u32,
    /// Added in schema v2 and expanded with `system` in schema v4. `default` also makes
    /// hand-authored documents with the field omitted degrade to the dark product default rather
    /// than losing the entire session.
    #[serde(default)]
    pub theme: SessionThemeV1,
    /// Added in schema v3; missing values degrade to the historical bar cursor.
    #[serde(default)]
    pub cursor_style: SessionCursorStyleV1,
    /// Added in schema v5; missing values degrade to the horizontal strip, which is the only
    /// arrangement that existed before this field.
    #[serde(default)]
    pub tab_layout: SessionTabLayoutV1,
    /// Added in schema v5; missing values degrade to the expanded sidebar, matching every session
    /// written before the icon rail existed.
    #[serde(default)]
    pub sidebar_mode: SessionSidebarModeV1,
    pub window: WindowStateV1,
    pub tabs: Vec<TabV1>,
    pub active_tab: u32,
    pub recent: Vec<RecentEntryV1>,
}

impl Default for SessionV1 {
    fn default() -> Self {
        Self {
            schema_version: SESSION_SCHEMA_VERSION,
            theme: SessionThemeV1::Dark,
            cursor_style: SessionCursorStyleV1::Bar,
            tab_layout: SessionTabLayoutV1::Horizontal,
            sidebar_mode: SessionSidebarModeV1::Expanded,
            window: WindowStateV1::default(),
            tabs: Vec::new(),
            active_tab: 0,
            recent: Vec::new(),
        }
    }
}

impl SessionV1 {
    /// Read-time per-leaf degradation (§5.4 case 3: "文件可解析但字段不满足
    /// 不变量…逐叶降级,不因单叶损坏丢整棵树"). Currently this clamps
    /// out-of-range split ratios (see [`LayoutNodeV1::degrade_in_place`]);
    /// unknown leaf kinds already degraded to placeholders during parsing.
    /// `cwd`/`profile_id` validity is deliberately not checked here — see
    /// [`crate::layout::TermLeafV1`]'s docs for why that is the consumer's
    /// job, not this crate's.
    ///
    /// Returns a summary the caller can use to decide whether to surface a
    /// degradation banner (§5.3: "显式告警,绝不假装成功").
    pub fn degrade_in_place(&mut self) -> DegradationReport {
        let mut report = DegradationReport::default();
        for tab in &mut self.tabs {
            tab.root.degrade_in_place(&mut report);
        }
        report
    }
}

/// Records what [`SessionV1::degrade_in_place`] had to fix, so a caller can
/// decide whether to surface a banner without this crate reaching into UI
/// concerns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DegradationReport {
    pub clamped_ratios: u32,
    pub unknown_leaves: u32,
}

impl DegradationReport {
    /// True when at least one leaf needed degrading.
    pub fn is_clean(&self) -> bool {
        self.clamped_ratios == 0 && self.unknown_leaves == 0
    }
}

/// `window` — docs/M2-persistence-schema-v1.md §3.1. v1 is single-window
/// (the field is named `window`, singular, deliberately not `windows: []` —
/// see §3.1's "多窗口落地时用 schema_version bump 把 window 升格为 windows[]").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowStateV1 {
    pub bounds: WindowBoundsV1,
    /// Per-monitor DPI recorded at capture time, used on restore to judge
    /// whether the layout needs to be recomputed (§3.1).
    pub dpi: u32,
    pub maximized: bool,
    /// Best-effort monitor identifier (§3.1: "不保证跨驱动更新稳定"). Absent
    /// when no monitor could be identified at capture time.
    pub monitor_id: Option<String>,
}

impl Default for WindowStateV1 {
    fn default() -> Self {
        // A safe, unopinionated placeholder — not a product decision about
        // where new windows should appear. §3.1's clamp-to-visible-workarea
        // rule governs actual placement; this is only the fallback shape
        // when there is no prior session to read at all.
        Self {
            bounds: WindowBoundsV1 {
                x: 100,
                y: 100,
                width: 1280,
                height: 800,
            },
            dpi: 96,
            maximized: false,
            monitor_id: None,
        }
    }
}

/// Logical-pixel window rectangle (§3.1: "逻辑像素").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowBoundsV1 {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// One tab — docs/M2-persistence-schema-v1.md §3.5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabV1 {
    pub root: LayoutNodeV1,
    pub pinned: bool,
    /// Stable leaf ID of the focused pane within `root`.
    pub focused_leaf: String,
    /// What this tab's preview panes were showing, and which buffers it had
    /// open — the tab's **content section**.
    ///
    /// It is here rather than on the preview leaf because of red line L1: the
    /// layout tree is the solver's input and carries *geometry*, and the pin is
    /// the only thing about a preview pane that is geometry (see
    /// [`crate::layout::LeafNodeV1::Preview`]). Which file a pane was showing is
    /// content, and the pool it came out of belongs to the *tab* by the
    /// 2026-07-17 ruling (`docs/DESIGN.md` §7.1.3: one buffer per file per tab,
    /// so a file open in two panes cannot fork). Both facts therefore have to
    /// sit beside the tree rather than inside it, and this is that place.
    ///
    /// Additive, and absent when there is nothing to say: every document
    /// written before this field still reads, and every tab without a preview
    /// pane still writes exactly the bytes it used to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<TabPreviewV1>,
}

/// `tab.preview` — see [`TabV1::preview`].
///
/// **Paths and names only. No content, and no dirty bit.** That is the same
/// honesty `session.json` already practises about scrollback ("不存输出历史"):
/// a restored pane must not pretend nothing happened, so unsaved edits die with
/// the process and the pane comes back showing the file as it is on disk. A
/// dirty buffer is not lost quietly on the way — the three dirty gates
/// (`docs/DESIGN.md` §7.1.3) name every one of them before the app shuts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabPreviewV1 {
    /// One entry per preview leaf of `root`, in tree order.
    pub panes: Vec<PreviewPaneV1>,
    /// The tab's shared buffer pool, oldest first — the *history* the filename
    /// switcher lists. A superset of what the panes are showing: a buffer the
    /// user browsed away from is still in the pool, and dropping it here would
    /// make the switcher shorter after every restart.
    pub pool: Vec<PreviewPoolEntryV1>,
}

/// One preview pane and the file it was showing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewPaneV1 {
    /// Which leaf of `root` this is, as the positional `leaf-N` token
    /// [`TabV1::focused_leaf`] already uses — the in-order index of the leaf in
    /// the saved tree.
    ///
    /// A token rather than a bare position in this list, because this list is
    /// *outside* the tree: a term or files leaf is paired with its seat by where
    /// it sits in the tree the file carries, and a section standing beside the
    /// tree has no such position to be read from. Nothing on disk names a
    /// runtime seat id (§3.2) and nothing should; the in-order index is a
    /// function of the same tree shape the file already carries, so it cannot
    /// point outside it.
    pub leaf: String,
    /// The file this pane was showing, or `null` for a pane that was showing
    /// nothing. Written rather than omitted: an empty preview pane is a pane,
    /// and a reader that inferred it from a missing row could not tell it from
    /// a pane the writer forgot.
    pub cur: Option<String>,
    /// **Which branches this pane's commit graph was of** (T2/T3, v2 ③).
    ///
    /// Here rather than on the preview *leaf* for [`TabV1::preview`]'s own
    /// reason: red line L1 keeps the layout tree to geometry, and a filter is not
    /// geometry — it is what the pane was looking at, which is content and
    /// therefore belongs in this section beside `cur`.
    ///
    /// **Durable, unlike everything else a graph seat holds.** A scroll, an
    /// expansion and a comparison are glances at a history that may have moved
    /// on; "show me these branches" is a question about the repository, and it is
    /// as true after a restart as it was before. It survives even though the
    /// *document* does not — `session.json` has no vocabulary for a git-backed
    /// buffer, so a graph is not reopened by a restore — because the pane is
    /// named either way, and a reader who opens the graph again in that pane
    /// finds the filter they left rather than a page of every branch.
    ///
    /// Additive and absent when nothing was filtered, so every document written
    /// before this field still reads and every pane that never touched the filter
    /// still writes exactly the bytes it used to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<GraphFilterV1>,
}

/// `pane.graph` — see [`PreviewPaneV1::graph`].
///
/// The three fields the filter menu sets, in its own vocabulary rather than
/// git's: `branches` is a list of local branch **names** and not the rev
/// arguments they are turned into, because the arguments are a fact about how
/// this build spells a question to git and the names are a fact about what the
/// reader chose. A file carrying `--branches --tags HEAD` would be a session
/// remembering an implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphFilterV1 {
    /// The branches picked by hand. **Empty is "all branches"**, which is what
    /// an unfiltered graph shows — see the runtime type for why that is the only
    /// honest reading of an empty list here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<String>,
    /// Whether remote-tracking names were shown. Defaulted **true**, because the
    /// resting state of a checkbox reading "Show remote branches" is ticked and a
    /// document written before this field describes a graph that was showing
    /// them.
    #[serde(default = "yes")]
    pub remotes: bool,
    /// Whether tags were, on the same footing.
    #[serde(default = "yes")]
    pub tags: bool,
}

/// `true`, as a function serde can name.
///
/// A `#[serde(default)]` on a `bool` gives `false`, and both of the flags above
/// rest at `true` — so the derive would read every document written before them
/// as a graph with its two checkboxes cleared, which is not the graph anybody
/// was looking at.
const fn yes() -> bool {
    true
}

/// One buffer in a tab's shared preview pool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewPoolEntryV1 {
    pub path: String,
    /// The display name the pane's head and the switcher print. Stored rather
    /// than re-derived, for the same reason `files` leaves store their root:
    /// this crate does not own a path grammar, and a name split off a path by a
    /// rule that drifts is a switcher row labelled differently after a restart.
    pub name: String,
}

/// One `recent` entry — docs/M2-persistence-schema-v1.md §3.5, deduped by
/// `profile_id+cwd+manual_name` per `docs/DESIGN.md` §7.1.4 (the dedup key
/// itself is computed by the caller and carried in `key`; this crate does
/// not recompute or validate it, matching the "seed/locus" split it neither
/// owns nor interprets).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecentEntryV1 {
    pub key: String,
    pub seed: RecentSeedV1,
    /// ISO-8601, kept as an opaque string (§3.5: "含时间戳"; this crate has
    /// no reason to parse it — only the age-based Recent eviction policy,
    /// which is a call-site concern per §7#6, would need to).
    pub timestamp: String,
    /// The files this tab's preview panes were showing when it was closed, in
    /// tree order — 裁决 10 (2026-08-12).
    ///
    /// **The bug this closes is a named one.** `docs/DESIGN.md` §7.1.4 already
    /// records what happens when a leaf kind is left out of the vault: a
    /// files-only tab came back through the shutdown prompt and could not be
    /// reached by Ctrl+Shift+T — two doors onto one store with one of them
    /// broken. Now that the session file brings a preview pane back
    /// ([`TabV1::preview`]), an entry that did not carry it would be that same
    /// asymmetry a second time, in the same store, for the same reason.
    ///
    /// A list of paths and nothing else. Recent is a *launcher*: it restores
    /// the places you were, not a layout — the pool regrows on demand and the
    /// pins are not promises anyone made about a closed tab.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previews: Vec<String>,
}

/// `docs/DESIGN.md` §7.1.4: "Recent 条目 = 终端 seed **或** files 场所" — the
/// spec names two possible shapes for the `seed` object (`docs/M2-persistence-schema-v1.md`
/// §3.5 shows only `"seed": {...}` without pinning the internal shape, deferring
/// to `DESIGN.md`'s own description of what a term seed and a files locus each
/// contain: `term 叶 seed{profile_id,cwd,手动名}` and `files 叶{root,...}`).
/// Tagged the same way as [`crate::layout::LeafNodeV1`] for the same reason:
/// it mirrors the two content kinds a Recent entry can come from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecentSeedV1 {
    Term {
        profile_id: String,
        cwd: String,
        manual_name: Option<String>,
    },
    Files {
        root: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_session_is_empty_at_current_version() {
        let defaults = SessionV1::default();
        assert_eq!(defaults.schema_version, SESSION_SCHEMA_VERSION);
        assert_eq!(defaults.theme, SessionThemeV1::Dark);
        assert_eq!(defaults.cursor_style, SessionCursorStyleV1::Bar);
        assert_eq!(defaults.tab_layout, SessionTabLayoutV1::Horizontal);
        assert_eq!(defaults.sidebar_mode, SessionSidebarModeV1::Expanded);
        assert!(defaults.tabs.is_empty());
        assert!(defaults.recent.is_empty());
        assert_eq!(defaults.active_tab, 0);
    }

    #[test]
    fn degrade_in_place_reports_clean_when_nothing_wrong() {
        let mut session = SessionV1::default();
        session.tabs.push(TabV1 {
            root: LayoutNodeV1::Leaf(crate::layout::LeafNodeV1::Term(crate::layout::TermLeafV1 {
                profile_id: "pwsh.exe".to_string(),
                cwd: "C:\\".to_string(),
                manual_name: None,
            })),
            pinned: false,
            focused_leaf: "leaf-0".to_string(),
            preview: None,
        });
        let report = session.degrade_in_place();
        assert!(report.is_clean());
    }
}
