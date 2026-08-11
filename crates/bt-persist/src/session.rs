//! `session.json` v1 — docs/M2-persistence-schema-v1.md §3.

use serde::{Deserialize, Serialize};

use crate::layout::LayoutNodeV1;

/// Current `schema_version` for `session.json`.
///
/// v6 is the first bump that changes an existing field's *meaning* rather than adding one:
/// `profile_id` becomes a stable profile slug. See [`crate::migrate`]'s `migrate_session_v5_to_v6`
/// for the ruling and the one-time mapping.
pub const SESSION_SCHEMA_VERSION: u32 = 6;

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
        });
        let report = session.degrade_in_place();
        assert!(report.is_clean());
    }
}
