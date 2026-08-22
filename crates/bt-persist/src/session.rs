//! `session.json` v1 — docs/M2-persistence-schema-v1.md §3.

use serde::{Deserialize, Serialize};

use crate::layout::LayoutNodeV1;

/// Current `schema_version` for `session.json`.
///
/// v6 is the first bump that changes an existing field's *meaning* rather than adding one:
/// `profile_id` becomes a stable profile slug. See [`crate::migrate`]'s `migrate_session_v5_to_v6`
/// for the ruling and the one-time mapping. v7 adds `view` to a `files` leaf — which of the
/// column's two pages it was showing (R1). v8 adds a third shape to [`RecentSeedV1`] — a closed
/// tab that was one preview pane, identified by the file it was on — which is the one thing the
/// sessionless-tab slice (`docs/DESIGN.md` §7.1.6h) could not already write. **v9 is the one
/// `docs/M2-persistence-schema-v1.md` §3.1 has been promising since v1**: `window` becomes
/// `windows[]`, and the four keys that were about "the window" — its placement, its tab strip's
/// layout, its sidebar's width and its tabs — move inside a [`SessionWindowV1`] so a document can
/// hold more than one of them (`docs/DESIGN.md` §2.4 D 段). **v10 adds `card_skip` to a `term`
/// leaf** — where that pane's focus card aims its window, in rows above the tail (§7.1.6b′, user
/// ruling 2026-08-21), which is v7's shape on the other kind of leaf: a fact about the pane's
/// *shape* rather than its content, and one a reader sets once and expects to find again.
/// **v11 lets a preview row name a page as well as a file** (Web 预览块 W2 片③, user ruling
/// 2026-08-22): [`PreviewSourceV1`] joins the pane's `cur`, the pool row and [`RecentSeedV1::Preview`]
/// as the one field that says which of the two a string is.
///
/// # The plaintext clause (`docs/plans/web-preview/plan.md` §3, user ruling 2026-08-22)
///
/// **A page's URL is written verbatim** — scheme, host, port, path, **query and fragment** — into
/// this file and into `pins.json`, in the clear. Query and fragment are part of what was asked for
/// and therefore part of the row's identity (`bt_app::webnav::switcher_key` keys the switcher on
/// exactly this string), so a document that dropped them would restore a different page than the
/// one that was closed. The consequence is stated rather than hidden: **a URL carrying a session
/// token is stored in the clear**, and a restore whose token has expired lands on a login page,
/// which is the normal outcome and not a failure of this file.
pub const SESSION_SCHEMA_VERSION: u32 = 11;

/// **What a preview row's string names** — schema v11.
///
/// The one field that tells a path from a page. Everything else about the row is unchanged: a page
/// is listed, restored, deduplicated, pinned and put in the vault through the fields a file already
/// used, which is `docs/DESIGN.md` §7.7 ①'s "同一张表、同一个索引空间" said about the disk.
///
/// **`File` is the default and is not written**, so a document with no page in it is byte for byte
/// the document a v10 build wrote: every string a v10 build ever put in one of these fields was a
/// path, and a reader that guessed otherwise would hand `http://localhost:5173/` to a filesystem.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewSourceV1 {
    /// A file on a disk — the only thing a preview row could name before v11.
    #[default]
    File,
    /// A page. The string is the normalised, whole URL; see the plaintext clause above.
    Url,
}

impl PreviewSourceV1 {
    /// Whether this is the historical answer, which is what `skip_serializing_if` asks.
    ///
    /// A method rather than a closure at each of the three fields, because "a document with no page
    /// in it writes the bytes it always wrote" is one promise and three copies of it is three
    /// chances for one field to start writing `"source": "file"` into every session on disk.
    #[must_use]
    pub fn is_file(&self) -> bool {
        matches!(self, Self::File)
    }
}

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
///
/// **Everything left at this level is a fact about the *process*** (`docs/DESIGN.md` §2.4's own
/// question, asked of a file instead of a struct): the theme and the cursor shape are
/// `bt-render` statics that one process owns one of, and the vault is `App::recent` — the one
/// store pin, Recent and undo-close all read. Everything that is a fact about *a window* is in
/// [`SessionWindowV1`], once per window, because a document that kept them here could only ever
/// describe the last window to write it.
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
    /// **Every window this process had open, in the order they opened** (schema v9).
    ///
    /// Empty is a first run, and it is the only spelling of one: a window with no tabs is not a
    /// window, so nothing that is open writes an empty entry here.
    pub windows: Vec<SessionWindowV1>,
    pub recent: Vec<RecentEntryV1>,
}

impl Default for SessionV1 {
    fn default() -> Self {
        Self {
            schema_version: SESSION_SCHEMA_VERSION,
            theme: SessionThemeV1::Dark,
            cursor_style: SessionCursorStyleV1::Bar,
            windows: Vec::new(),
            recent: Vec::new(),
        }
    }
}

/// **One window** — schema v9's whole reason for existing.
///
/// The four keys that used to sit at the top level of the document, moved inside the thing they
/// were always about. `docs/M2-persistence-schema-v1.md` §3.1 wrote the promissory note in v1
/// ("多窗口落地时用 schema_version bump 把 window 升格为 windows[]") and named this as its
/// price; [`crate::migrate`]'s `migrate_session_v8_to_v9` is the one-time payment.
///
/// **What is here and what is not** is decided by one question asked of `WindowRuntime`'s 157
/// fields (`docs/DESIGN.md` §2.4): is this durable, and is it the window's? Placement, the tab
/// tree, which tab was on top, and the two halves of the rail's resting shape all answer yes to
/// both. A hover, a gesture, a menu and every cache answer no to the first. The theme and the
/// cursor answer no to the second — they are process statics, and they stay at the top level.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionWindowV1 {
    /// Where the window was and how big — see [`WindowStateV1`].
    #[serde(default)]
    pub placement: WindowStateV1,
    /// Where this window's tab strip rested. Per window from v9: two windows can stand with two
    /// different strips, and a document that recorded one of them recorded the other's as a lie.
    /// Missing values degrade to the horizontal strip, which is the only arrangement that existed
    /// before schema v5.
    #[serde(default)]
    pub tab_layout: SessionTabLayoutV1,
    /// And how wide this window's sidebar rested, on exactly the same terms.
    #[serde(default)]
    pub sidebar_mode: SessionSidebarModeV1,
    pub tabs: Vec<TabV1>,
    pub active_tab: u32,
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
        for window in &mut self.windows {
            for tab in &mut window.tabs {
                tab.root.degrade_in_place(&mut report);
            }
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

/// **Where one window was** — docs/M2-persistence-schema-v1.md §3.1.
///
/// From v1 to v8 this was the document's one `window` key, singular and deliberately so; §3.1
/// said what would end that ("多窗口落地时用 schema_version bump 把 window 升格为 windows[]")
/// and v9 is it. The type did not have to change to make the step — a rectangle, a DPI, a
/// posture and a monitor were always facts about *a* window rather than about *the* window —
/// so it is the same four fields, now reached as [`SessionWindowV1::placement`].
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
    /// The file — or, from schema v11, the page — this pane was showing, or
    /// `null` for a pane that was showing nothing. Written rather than omitted:
    /// an empty preview pane is a pane, and a reader that inferred it from a
    /// missing row could not tell it from a pane the writer forgot.
    pub cur: Option<String>,
    /// Which of the two [`Self::cur`] is (schema v11).
    ///
    /// Beside `cur` rather than inside it, because that is what keeps this
    /// section the shape it has always been: a pane row is `{leaf, cur}` and
    /// still is, and a document with no page in it is byte for byte the document
    /// a v10 build wrote. See [`PreviewSourceV1`], and the plaintext clause on
    /// [`SESSION_SCHEMA_VERSION`] for what a URL here carries.
    #[serde(default, skip_serializing_if = "PreviewSourceV1::is_file")]
    pub cur_source: PreviewSourceV1,
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
    ///
    /// **A page's name is its title**, on exactly those terms: it is not derived
    /// from the URL, because the two are different sentences and the title is
    /// the one the switcher lists.
    pub name: String,
    /// Which of the two [`Self::path`] is (schema v11) — see [`PreviewSourceV1`].
    #[serde(default, skip_serializing_if = "PreviewSourceV1::is_file")]
    pub source: PreviewSourceV1,
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
    /// A list of places and nothing else. Recent is a *launcher*: it restores
    /// the places you were, not a layout — the pool regrows on demand and the
    /// pins are not promises anyone made about a closed tab.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previews: Vec<RecentPreviewV1>,
}

/// One thing a closed tab's preview pane was on — **a file, spelled as the bare
/// string it has always been, or a page** (schema v11).
///
/// The discriminator here is the element's *shape* and not a field beside it,
/// which is the opposite of [`RecentSeedV1::Preview`]'s choice one field over.
/// Both are the same rule applied to two different forms: keep what the file
/// already says, and add only the word that says which. A seed is an object that
/// already had a `path` key, so the word goes beside it; this is a list of bare
/// strings, so the word is *that a page is not a bare string*. The result either
/// way is that **every row on every disk is byte for byte what it was** — no
/// migration step, no rewritten list, and a v10 build reading a v11 vault meets a
/// shape it cannot read rather than a path it would hand to a filesystem.
///
/// `untagged`, and the two arms cannot be confused: a JSON string and a JSON
/// object are disjoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RecentPreviewV1 {
    /// A file on a disk — `"D:\\work\\notes.md"`.
    File(String),
    /// A page — `{ "url": "http://localhost:5173/app?tab=logs" }`. See the
    /// plaintext clause on [`SESSION_SCHEMA_VERSION`] for what that string may
    /// carry.
    Page { url: String },
}

impl RecentPreviewV1 {
    /// The string this row names, whichever of the two it is.
    #[must_use]
    pub fn target(&self) -> &str {
        match self {
            Self::File(path) => path,
            Self::Page { url } => url,
        }
    }
}

/// `docs/DESIGN.md` §7.1.4: "Recent 条目 = 终端 seed **或** files 场所" — the
/// spec names two possible shapes for the `seed` object (`docs/M2-persistence-schema-v1.md`
/// §3.5 shows only `"seed": {...}` without pinning the internal shape, deferring
/// to `DESIGN.md`'s own description of what a term seed and a files locus each
/// contain: `term 叶 seed{profile_id,cwd,手动名}` and `files 叶{root,...}`).
/// Tagged the same way as [`crate::layout::LeafNodeV1`] for the same reason:
/// it mirrors the content kinds a Recent entry can come from.
///
/// **The third shape arrived with schema v8** and it arrived because the tab it
/// names now exists: `docs/DESIGN.md` §7.1.6h makes a lone preview pane a tab of
/// its own, identified by the file it is on. Without a seed for it, closing such
/// a tab would put nothing in the vault — and the `seed` module's own header
/// records exactly what that costs: "a files-only tab that could be restored by
/// the shutdown prompt but not by Ctrl+Shift+T would be two doors onto one store
/// with one of them broken."
///
/// The **version** is bumped rather than the variant merely added, because this
/// enum has no `#[serde(other)]` arm: a v7 reader handed `"kind": "preview"`
/// would fail the whole document, and the version is what makes it refuse for
/// the right reason (§5.4's future-version refusal) rather than report a corrupt
/// file. Nothing in an existing document changes, so the step itself carries no
/// field work — see [`crate::migrate`]'s `migrate_session_v7_to_v8`.
///
/// **The fourth shape arrived with schema v9**, by the same sentence a third
/// time: multiwindow slice D made *a window* a thing that can be closed, so the
/// vault owes it a row for the reason it owes one to every other shape a close
/// can take. It rides the version step `windows[]` was already paying for, and
/// the future-version refusal above is why it may.
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
    /// The file a lone preview pane was showing (§7.1.6h).
    ///
    /// A path and nothing else, on [`TabPreviewV1`]'s own ruling: a restored
    /// preview shows the file as it is on disk, so there is no view, no scroll
    /// and no dirty bit here to be a promise nobody can keep.
    ///
    /// **The fifth shape is not a fifth variant** (schema v11, user ruling
    /// 2026-08-22): a page closes exactly as a file does, so it is this row with
    /// [`Self::Preview::source`] saying which of the two the string is. Without
    /// it, closing a web tab would be the one close in this window with no way
    /// back — which is the asymmetry this whole enum exists to prevent, said a
    /// fourth time. The field is named `source` and not `kind` because `kind` is
    /// already this enum's own serde tag.
    Preview {
        path: String,
        #[serde(default, skip_serializing_if = "PreviewSourceV1::is_file")]
        source: PreviewSourceV1,
    },
    /// **A whole window that was closed** (multiwindow slice D, 2026-08-19 ruling
    /// ②: 关一扇非最后的窗不弹提示,该窗的种子进 Recent).
    ///
    /// Closing a tab has always filled the vault, and closing a window closes
    /// every tab in it — so without this shape the one gesture that can throw
    /// away six tabs at once would be the only one with no way back, which is
    /// the asymmetry this whole store exists to prevent. One row rather than
    /// six, because what the reader lost was a window and "重开丢的窗从 Recent
    /// 一步找回" is one step.
    ///
    /// **The seeds of its tabs, and nothing else** — not the tree, not the
    /// rectangle, not the rail. That is [`RecentEntryV1`]'s own standing rule
    /// said about a bigger object: "Recent is a *launcher*: it restores the
    /// places you were, not a layout", which is already why a closed files tab
    /// forgets its column width. A window taken back out of the vault opens
    /// where a new window opens, holding the places its tabs stood in.
    ///
    /// Recursive rather than a flat list of three-field rows, so that the shapes
    /// a window's tabs can be are *by construction* the shapes a tab can be:
    /// a fourth kind of tab would otherwise have to be added here a second time,
    /// and the copy that drifts is the one nothing draws.
    Window {
        seeds: Vec<RecentSeedV1>,
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
        assert!(
            defaults.windows.is_empty(),
            "no windows at all is how a first run spells itself (schema v9)"
        );
        assert!(defaults.recent.is_empty());
    }

    /// The window record's own defaults, which are what a hand-authored document
    /// with only `tabs` in it degrades to.
    #[test]
    fn a_window_record_defaults_to_the_arrangement_that_predates_the_fields() {
        let window = SessionWindowV1::default();
        assert_eq!(window.tab_layout, SessionTabLayoutV1::Horizontal);
        assert_eq!(window.sidebar_mode, SessionSidebarModeV1::Expanded);
        assert_eq!(window.placement, WindowStateV1::default());
        assert_eq!(window.active_tab, 0);
        assert!(window.tabs.is_empty());
    }

    #[test]
    fn degrade_in_place_reports_clean_when_nothing_wrong() {
        let mut session = SessionV1::default();
        session.windows.push(SessionWindowV1 {
            tabs: vec![TabV1 {
                root: LayoutNodeV1::Leaf(crate::layout::LeafNodeV1::Term(
                    crate::layout::TermLeafV1 {
                        profile_id: "pwsh.exe".to_string(),
                        cwd: "C:\\".to_string(),
                        manual_name: None,
                        card_skip: 0,
                    },
                )),
                pinned: false,
                focused_leaf: "leaf-0".to_string(),
                preview: None,
            }],
            ..SessionWindowV1::default()
        });
        let report = session.degrade_in_place();
        assert!(report.is_clean());
    }

    /// PIN (multiwindow slice D) — **the second window's tree is degraded too.**
    ///
    /// The walk gained a level when the document did, and a walk that stopped at
    /// the first window would leave every later one's out-of-range ratio on the
    /// disk it came from — silently, because a clamp that does not happen looks
    /// exactly like a document that never needed one.
    ///
    /// Red gate: iterate `self.windows.first_mut()` instead of `&mut
    /// self.windows` and the count below is 1.
    #[test]
    fn every_window_is_degraded_and_not_only_the_first() {
        let leaf = || {
            Box::new(LayoutNodeV1::Leaf(crate::layout::LeafNodeV1::Files(
                crate::layout::FilesLeafV1 {
                    root: "D:\\".to_string(),
                    open: Vec::new(),
                    sel: None,
                    width: 260,
                    view: crate::layout::FilesViewV1::Files,
                    remotes_open: false,
                },
            )))
        };
        let split = |ratio: u32| {
            LayoutNodeV1::Split(crate::layout::SplitNodeV1 {
                dir: crate::layout::SplitDirV1::Row,
                ratio,
                children: [leaf(), leaf()],
            })
        };
        let mut session = SessionV1::default();
        for ratio in [u32::MAX, 4_000_000] {
            session.windows.push(SessionWindowV1 {
                tabs: vec![TabV1 {
                    root: split(ratio),
                    pinned: false,
                    focused_leaf: "leaf-0".to_string(),
                    preview: None,
                }],
                ..SessionWindowV1::default()
            });
        }
        let report = session.degrade_in_place();
        assert_eq!(
            report.clamped_ratios, 2,
            "one clamp per window, not one clamp for the first one"
        );
    }
}
