//! **The icon system's two missing tables** — which shape a verb wears, and how
//! big a surface draws it.
//!
//! [`crate::marks`] holds the third table, the artwork, and it has held it
//! alone: forty-six symbol bodies quoted from `design/ui-mockup.html`, every one
//! of them drawn through one rasterizer in one colour discipline. The two
//! audits of 2026-08-25 both arrived at the same sentence about what was still
//! missing — *the geometry has a single source, the size and the meaning do
//! not*. Forty-odd drawing points each decided for themselves how many pixels a
//! mark got, and fourteen `fn mark()` dispatchers each decided for themselves
//! which drawing a verb got, with no way to ask the reverse question. So the
//! `×` came to mean close, delete, discard and stop; the bare `↗` came to mean
//! four different things two of which sat in adjacent rows; and one pane head
//! ended up drawing a `⌄` at 1.56 logical pixels of stroke beside a `✕` at
//! 0.80.
//!
//! This module is those two tables and the gates that hold them:
//!
//! * [`ActionIcon`] — **one verb, one shape.** The registry the fourteen
//!   dispatchers now read out of. Its reverse index is a test: a shape two
//!   verbs share has to be written down as shared, so the day somebody adds a
//!   third the build says so instead of the user.
//! * [`MarkSlot`] — **four boxes, and a drawing point names one of them.** A
//!   call site says *this is a menu row* rather than *this is fifteen pixels*,
//!   and the box it gets is derived from the mark's own geometry rather than
//!   guessed at the call site.
//!
//! And the gate over both, in this module's own tests: for every box the chrome
//! draws a mark in, the pen that mark ends up with on screen has to land in the
//! band `1.05 ± 0.10` logical pixels. Everything still outside it is named in
//! this module's `exempt`, with the redraw that closes it — six drawings whose
//! pen is wrong against their own box, and a list of surfaces that draw below
//! button scale on purpose and are not what the band is about. **Nothing may
//! join either list which moving a slot would have fixed**; that rule is itself
//! a test.

use crate::marks::{self, ChromeMark};

// ── the slot table ─────────────────────────────────────────────────────────

/// **The four boxes the chrome draws marks in.**
///
/// Codex's half of the 2026-08-25 specification, adopted by the plan: a drawing
/// point reports which of these it is and not how many pixels it wants. Four
/// numbers replace the forty-odd `*_GLYPH_LOGICAL_PX` constants that each
/// answered the question locally — and, more to the point, *the four numbers are
/// boxes for the house's own sixteen-unit family*. What a mark cut in some other
/// box gets is [`Self::mark_box_logical_px`]'s answer, derived from the drawing
/// rather than written down again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkSlot {
    /// A popup menu's icon column — the profile menu, the file row's menu, the
    /// terminal menu, the pane menu, the Git menus.
    Menu,
    /// A panel's own head: the Git panel's masthead and the buttons that live
    /// beside it.
    Toolbar,
    /// The run of controls on a pane head, a preview head or a float's head —
    /// the tightest slot the chrome has that is still a *button*.
    CompactHead,
    /// The title bar's own controls, which are the platform's size and not this
    /// design's: minimise, maximise, close.
    Caption,
}

/// `菜单 14` — the menu row's box for a house mark.
const MENU_HOUSE_BOX_LOGICAL_PX: f32 = 14.0;
/// `工具栏 14` — the same number for a panel head, and the same reason: both are
/// a column of marks beside a column of words.
const TOOLBAR_HOUSE_BOX_LOGICAL_PX: f32 = 14.0;
/// `紧凑头 13` — a head's run of controls.
///
/// **The plan wrote `12` and P0 found that the two halves of the specification
/// did not both fit.** A house mark's pen on screen is `1.2 / 16 × box`, so a
/// twelve-pixel box draws `0.90` — below the band's floor before any drawing
/// has been chosen, which P0 recorded in its own exemption note as the one
/// thing P1 had to rule on. It stayed green only because nothing house-family
/// was drawn there yet: the head's tools were at `13`, its folder is a fill.
/// P1 moves the whole run into this slot, so the day had come.
///
/// Thirteen, and not a pen of the compact head's own. The alternative was to
/// let one surface carry a heavier pen so its smaller box came out level, which
/// is the *compensation* the 2026-08-25 audit found the menu column doing by
/// hand — `#i-code` reading a correct `1.050` at twelve beside neighbours drawn
/// at fourteen, a twelve-pixel mark in a fourteen-pixel column. One pen and one
/// box per slot is the whole of what this table is for; thirteen is the
/// smallest box that carries the house pen, and a head's run is the tightest
/// place the chrome puts a button.
const COMPACT_HEAD_HOUSE_BOX_LOGICAL_PX: f32 = 13.0;
/// `标题栏 10` — **and this one is written for the other family.**
///
/// The three caption controls are the ten-unit window-control glyphs, drawn edge
/// to edge in their own box, and `10` is the box *they* take: it is what
/// `bt_render::WINDOW_CAPTION_GLYPH_LOGICAL_PX` has always been, and routing the
/// caption through this slot changes nothing on screen. The house box for this
/// slot is therefore derived the other way round — see
/// [`MarkSlot::house_box_logical_px`].
const CAPTION_EDGE_TO_EDGE_BOX_LOGICAL_PX: f32 = 10.0;

impl MarkSlot {
    /// Every slot, for the gate to walk.
    #[cfg(test)]
    pub const ALL: [Self; 4] = [Self::Menu, Self::Toolbar, Self::CompactHead, Self::Caption];

    /// The slot's own name, for a failing assertion to say.
    #[cfg(test)]
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Menu => "menu row",
            Self::Toolbar => "panel head",
            Self::CompactHead => "compact head",
            Self::Caption => "window caption",
        }
    }

    /// The box a **house** mark takes here — one cut in the sixteen-unit grid
    /// with the house's unit and a half of air on each side.
    #[must_use]
    pub const fn house_box_logical_px(self) -> f32 {
        match self {
            Self::Menu => MENU_HOUSE_BOX_LOGICAL_PX,
            Self::Toolbar => TOOLBAR_HOUSE_BOX_LOGICAL_PX,
            Self::CompactHead => COMPACT_HEAD_HOUSE_BOX_LOGICAL_PX,
            Self::Caption => CAPTION_EDGE_TO_EDGE_BOX_LOGICAL_PX / marks::HOUSE_INK_RATIO,
        }
    }

    /// The box an **edge-to-edge** mark takes here.
    ///
    /// **[`marks::HOUSE_INK_RATIO`] of the house box, and that is a derivation
    /// rather than a taste.** A mark whose artwork runs to the edge of its own
    /// `viewBox` puts a whole box of ink on the row; a house mark puts 80% of
    /// one. Drawn at the same box the two are not the same size on screen at
    /// all, which is precisely the measurement
    /// `profiles::ITEM_MARK_EDGE_TO_EDGE_LOGICAL_PX` was written from — the
    /// user's 2026-08-16 and 2026-08-19 reports were both "this row's mark is a
    /// size bigger than the others". That constant answered with a hand-picked
    /// `10` against a `15` column (a ratio of 0.67, which overshot: at 10 the
    /// `×`'s ink was *below* its neighbours' band rather than level with it).
    /// This answers with the ink the two families actually carry, so the two are
    /// level by construction and stay level when a slot moves.
    #[must_use]
    pub fn edge_to_edge_box_logical_px(self) -> f32 {
        match self {
            Self::Caption => CAPTION_EDGE_TO_EDGE_BOX_LOGICAL_PX,
            other => other.house_box_logical_px() * marks::HOUSE_INK_RATIO,
        }
    }

    /// **The box this mark takes in this slot**, as `[width, height]`.
    ///
    /// Two derivations, both off the drawing and neither off a list of names:
    ///
    /// 1. Which family the mark is in — [`ChromeMark::draws_edge_to_edge`],
    ///    which is a fact about where its ink stops.
    /// 2. **Its own aspect.** The slot names one number and a mark that is not
    ///    square is given that number on its *long* side, so the short side
    ///    keeps the proportion the drawing was cut at. This is the second half
    ///    of the pane head's 1.95× problem: `#i-chev` is a `10×6` arrow, and a
    ///    `10×6` arrow squeezed into a square box is scaled by `min(w/10, h/6)`,
    ///    which is the *width* ratio — so a 13px square drew it 1.3× rather than
    ///    the 0.8× the neighbouring `✕` got, before either mark's pen was
    ///    considered. Fitted at its own aspect the arrow is scaled once, by the
    ///    slot, like everything else in the run.
    ///
    /// A mark with no `viewBox` of its own — the generated family — is drawn at
    /// the slot's house box square, because there is no proportion to keep.
    #[must_use]
    pub fn mark_box_logical_px(self, mark: ChromeMark) -> [f32; 2] {
        let across = if mark.draws_edge_to_edge() {
            self.edge_to_edge_box_logical_px()
        } else {
            self.house_box_logical_px()
        };
        let Some([view_width, view_height]) = mark.view_box_units() else {
            return [across, across];
        };
        let scale = across / view_width.max(view_height);
        [view_width * scale, view_height * scale]
    }
}

/// **Where a mark's pen has to land on screen**, in logical pixels.
///
/// `1.05 ± 0.10`, the plan's own band. `1.05` is what the house pen draws at the
/// menu slot: [`marks::PROFILE_LINE_STROKE_UNITS`]' `1.2` — the weight the
/// 2026-08-25 ruling cut the line profiles to, and therefore the house's — over
/// the sixteen-unit grid, at a fourteen-pixel box. The tolerance is what a
/// second slot is allowed to differ by before a reader sees two weights in one
/// window.
#[cfg(test)]
pub const OPTICAL_STROKE_BAND_LOGICAL_PX: [f32; 2] = [0.95, 1.15];

/// **How far apart two controls standing in one run may look in size** — the
/// optical band's second half, and 裁2's (2026-08-26) own gate.
///
/// The band above governs a mark's **pen**, and a companion derivation
/// ([`marks::HOUSE_INK_RATIO`]) levels its **ink width**. A pane head passed
/// both and the acceptance still reported *`✕` 视觉上比 `⌄` 大*, because
/// neither is what a reader compares. What a reader compares is the **picture**
/// — the box the ink fills — and its size is that box's diagonal
/// ([`marks::ChromeMark::optical_picture_logical_px`]). Measured there the run
/// read `14.71 : 13.44 : 11.72` for the `✕`, the `🗀` and the `⌄`: a quarter
/// as big again across a run of three buttons, all three of them inside every
/// band this module had.
///
/// **Twenty per cent, and the number is the house's own floor rather than a
/// taste.** On that same head the two drawings nobody has complained about —
/// the flat arrow and the folder silhouette, each at the box its own grid asks
/// for — differ by `14.7%`, and no box moves that: it is what a `12.8 × 6.4`
/// arrow and a `12.8 × 10.4` folder *are*. A band tighter than `14.7%` would
/// therefore be this gate condemning a drawing the design is happy with, which
/// is the failure mode `NOT_A_CONTROL_SLOT` was written to avoid one rule up.
/// Twenty is that floor with a little air, and it is still tight enough to have
/// been red at `25.5%` on the morning the report came in — see
/// `a_heads_run_is_one_picture`, which is that measurement as an assertion.
#[cfg(test)]
pub const OPTICAL_PICTURE_SPREAD: f32 = 0.20;

// ── the verb table ─────────────────────────────────────────────────────────

/// **One verb, one shape** — the chrome's whole vocabulary of actions and the
/// mark each one wears.
///
/// The table the fourteen `fn mark()` dispatchers now read out of instead of
/// each holding an opinion. Two things follow from its being one table, and
/// they are the two the audits asked for:
///
/// * A verb cannot wear two shapes on two surfaces, because there is one arm to
///   answer with. `Rename` in a file menu and `Rename branch` in a Git menu were
///   a pencil and *nothing at all* for eleven days, on a comment that said the
///   house had no pencil while the pencil sat seven hundred lines further down
///   the same file.
/// * A shape worn by two verbs is *visible*, because the reverse index is a
///   test. See `REUSED_SHAPES` in this module's tests: every shape more than one
///   verb wears is written down with the whole list, so adding an unlisted reuse
///   fails the build rather than the reader.
///
/// **What is not here.** Marks that stand for an *object* rather than an act —
/// a session's profile mark, a page's favicon, a status dot, the graph's own
/// curves — are not verbs and have no entry. The handful of fixed object marks a
/// row can wear (a folder, a file, a page) do, because those are exactly the
/// arms the dispatchers were choosing by hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionIcon {
    // ── the window's own furniture ───────────────────────────────────────
    /// The caption's settings button.
    OpenSettings,
    MinimiseWindow,
    MaximiseWindow,
    CloseWindow,
    /// The strip's `+`.
    NewTab,
    /// The `⌄` beside it, and every other list-that-is-folded-away trigger.
    PickProfile,
    CloseTab,
    /// **The `▸` a menu row wears when it opens another menu** — the pane
    /// menu's `Move to window ▸`, the file menu's submenus.
    ///
    /// One of the chrome's **two** disclosure languages and not a third. See
    /// [`Self::FoldFolder`], which is this drawing turned, and the 2026-08-26
    /// ruling that keeps the triangle in exactly these two places.
    OpenSubmenu,
    /// **The files tree's own triangle**, and the file menu's `Expand` /
    /// `Collapse` row, which is a word for the same gesture.
    ///
    /// [`Self::OpenSubmenu`]'s drawing at the other end of its turn — one
    /// disclosure triangle, two orientations, which is what the two draw sites
    /// asking the registry now makes checkable.
    FoldFolder,

    // ── a pane's head ────────────────────────────────────────────────────
    /// The head's `⌄`, and the lone pane's corner ghost, which opens the same
    /// menu.
    OpenPaneMenu,
    /// The head's `🗀`, which puts a files pane beside this one.
    OpenFilesPane,
    /// The files head's `↗`.
    FloatFilesPane,
    /// The head's `✕`, and the pane menu's `Close pane` — **one verb, and since
    /// this table one variant name.** [`ChromeMark::WindowClose`],
    /// [`ChromeMark::TabClose`] and [`ChromeMark::PaneClose`] are three names for
    /// one `<symbol>`, kept apart so a control that might one day be re-struck
    /// has a cache slot of its own; which of the three a *menu row* took was
    /// arbitrary, and the pane menu took the tab's.
    ClosePane,

    // ── the pane menu ────────────────────────────────────────────────────
    ZoomPane,
    SplitPane,
    NewTerminalInFolder,
    DuplicatePane,
    MoveToNewTab,
    MoveToNewWindow,
    MoveToWindow,

    // ── the terminal menu ────────────────────────────────────────────────
    CopySelection,
    PasteClipboard,
    SelectAll,
    ClearScreen,
    ClearScrollback,
    RestartShell,

    // ── a file row's menu ────────────────────────────────────────────────
    OpenFile,
    OpenWith,
    RenameFile,
    CopyPath,
    InsertPath,
    /// `Reveal in folder`, and the Git row menu's `Reveal in Explorer` — one
    /// verb, which is why the two share a string already. Also the files pane's
    /// own foot and a docked float's, which are the same act on the folder the
    /// column is rooted at.
    RevealInFolder,
    /// `Browse…` — the root menu's escape hatch, which opens the system's own
    /// folder picker.
    ///
    /// **An act, and the rows above it are places** — which P2 drew a line
    /// along and 裁1 (2026-08-26) rubbed out. Both wear the mock-up's open
    /// folder, solid, because both are about a folder; what tells this row from
    /// the places above it is its own word.
    BrowseForFolder,

    // ── the Git menus, panel and graph ───────────────────────────────────
    CheckoutBranch,
    CreateBranch,
    CreateTag,
    /// **`Rename…`, which had no mark at all until this table.** The arm read
    /// `None` under a comment saying "the house's mark set is cut from geometry
    /// and has no pencil in it" — untrue since `#i-pencil` landed on 2026-08-19,
    /// and untrue *in the same file*, where `Rename` on a file row already wore
    /// it. A verb table is what stops one arm of one dispatcher going stale
    /// against another arm of another.
    RenameBranch,
    DeleteBranch,
    DeleteTag,
    DiscardChanges,
    StageChange,
    UnstageChange,
    LoadMoreCommits,
    OpenDiff,
    CopyHash,
    /// The graph detail card's own `Copy hash`, which draws `#i-code` where the
    /// row menu draws `#i-copy` — **one verb in two shapes**, recorded rather
    /// than quietly unified. The card's own note argues the `< >` is what tells
    /// a hexadecimal name from a sentence when the two sit side by side, and it
    /// is right that they need telling apart; P1's answer is a `Hash` mark that
    /// says so without borrowing the source view's.
    GraphCopyHash,
    CopySubject,
    CopyName,
    /// Jumping to a parent commit, from the graph's detail card.
    GoToParentCommit,
    CompareVersions,
    LeaveCompare,
    OpenGitGraph,
    RereadRepository,
    /// The tick a menu puts against the row that is on.
    MenuTick,

    // ── a preview's head and its rail ────────────────────────────────────
    SavePreview,
    ViewSource,
    ViewRendered,
    OpenDevTools,
    FloatPreview,
    LockPreview,
    NavigateBack,
    NavigateForward,
    ReloadPage,
    StopNavigating,
    CopyAddress,
    OpenInBrowser,

    // ── the settings dialog ──────────────────────────────────────────────
    /// `Find…`, on the terminal's own context menu — **the row that had no
    /// mark**, because until P1 the house had no magnifier. It read as a glyph
    /// that had fallen out of the column: rows above it and below it wore one.
    FindInTerminal,
    EditRow,
    DeleteRow,
    CloseDialog,
    /// **A page's `Advanced` header**, which turns a chevron (the 2026-08-26
    /// ruling: the triangle stays in the tree and the submenu, the dialog and
    /// both breadcrumbs take the arrow).
    ///
    /// P1 changed the drawing and left the drawing point choosing it by hand;
    /// P2 routes it here, so the chrome's three disclosure sites are three rows
    /// of one table instead of three opinions in three files.
    ExpandAdvanced,
    /// **The four verbs the dialog was spending font characters on** (P1, the
    /// plan's 字符退役). `↺`, `↑`, `↓`, `⋯` and `✕` were drawn as text runs
    /// in the dialog's own type, which is the thing `marks.rs`'s opening
    /// paragraph forbids: a codepoint is a fact about the installed font, its
    /// width is another, and the row beside it is drawing geometry.
    RestoreRowDefaults,
    MoveRowUp,
    MoveRowDown,
    OpenRowMenu,
    /// The dialog's `✕` on an environment row — a *removal*, which is why it
    /// joins [`Self::DeleteRow`] on the bin rather than the close cross.
    RemoveEnvironmentRow,
    /// The three pictures the `Split direction` combo draws: *ask me*, *to the
    /// right*, *below*.
    SplitDirectionAsk,
    SplitDirectionRight,
    SplitDirectionDown,

    // ── the objects a row can be about ───────────────────────────────────
    /// A seat holding the file tree.
    FilesSeat,
    /// A seat holding a preview, before anybody has said what is in it.
    PreviewSeat,
    /// A seat this build has no word for.
    UnknownSeat,
    /// A shut folder — a tree row, a breadcrumb, a menu's path chip.
    FolderObject,
    /// An open one.
    OpenFolderObject,
    /// A file, wherever a row stands for one.
    FileObject,
    /// A page, wherever a row stands for one and has no favicon of its own.
    PageObject,
    /// **A mouse with a wheel** — the half of a chord that has no key cap
    /// (`docs/DESIGN.md` §7.21).
    ///
    /// A thing and not an act, and the difference decides the drawing: this
    /// entry never says *scroll*, it says *the wheel*, exactly as
    /// [`Self::FileObject`] says *a file* rather than *open*. The verb it is
    /// standing next to is written in words beside it, because the one thing a
    /// chord notation must not do is say the same thing twice.
    ///
    /// It is nonetheless struck, and that is a fact about a mouse rather than
    /// an exception to the fill policy: the shell is a frame, so the frame is
    /// drawn, and the field inside it — the wheel — is filled.
    MouseWheel,
}

impl ActionIcon {
    /// Every verb, for the reverse index to walk.
    #[cfg(test)]
    pub const ALL: [Self; 87] = [
        Self::OpenSettings,
        Self::MinimiseWindow,
        Self::MaximiseWindow,
        Self::CloseWindow,
        Self::NewTab,
        Self::PickProfile,
        Self::CloseTab,
        Self::OpenSubmenu,
        Self::FoldFolder,
        Self::OpenPaneMenu,
        Self::OpenFilesPane,
        Self::FloatFilesPane,
        Self::ClosePane,
        Self::ZoomPane,
        Self::SplitPane,
        Self::NewTerminalInFolder,
        Self::DuplicatePane,
        Self::MoveToNewTab,
        Self::MoveToNewWindow,
        Self::MoveToWindow,
        Self::CopySelection,
        Self::PasteClipboard,
        Self::SelectAll,
        Self::ClearScreen,
        Self::ClearScrollback,
        Self::RestartShell,
        Self::OpenFile,
        Self::OpenWith,
        Self::RenameFile,
        Self::CopyPath,
        Self::InsertPath,
        Self::RevealInFolder,
        Self::BrowseForFolder,
        Self::CheckoutBranch,
        Self::CreateBranch,
        Self::CreateTag,
        Self::RenameBranch,
        Self::DeleteBranch,
        Self::DeleteTag,
        Self::DiscardChanges,
        Self::StageChange,
        Self::UnstageChange,
        Self::LoadMoreCommits,
        Self::OpenDiff,
        Self::CopyHash,
        Self::GraphCopyHash,
        Self::CopySubject,
        Self::CopyName,
        Self::GoToParentCommit,
        Self::CompareVersions,
        Self::LeaveCompare,
        Self::OpenGitGraph,
        Self::RereadRepository,
        Self::MenuTick,
        Self::SavePreview,
        Self::ViewSource,
        Self::ViewRendered,
        Self::OpenDevTools,
        Self::FloatPreview,
        Self::LockPreview,
        Self::NavigateBack,
        Self::NavigateForward,
        Self::ReloadPage,
        Self::StopNavigating,
        Self::CopyAddress,
        Self::OpenInBrowser,
        Self::FindInTerminal,
        Self::EditRow,
        Self::DeleteRow,
        Self::CloseDialog,
        Self::ExpandAdvanced,
        Self::RestoreRowDefaults,
        Self::MoveRowUp,
        Self::MoveRowDown,
        Self::OpenRowMenu,
        Self::RemoveEnvironmentRow,
        Self::SplitDirectionAsk,
        Self::SplitDirectionRight,
        Self::SplitDirectionDown,
        Self::FilesSeat,
        Self::PreviewSeat,
        Self::UnknownSeat,
        Self::FolderObject,
        Self::OpenFolderObject,
        Self::FileObject,
        Self::PageObject,
        Self::MouseWheel,
    ];

    /// The verb's own name, for a failing assertion to say.
    #[cfg(test)]
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::OpenSettings => "OpenSettings",
            Self::MinimiseWindow => "MinimiseWindow",
            Self::MaximiseWindow => "MaximiseWindow",
            Self::CloseWindow => "CloseWindow",
            Self::NewTab => "NewTab",
            Self::PickProfile => "PickProfile",
            Self::CloseTab => "CloseTab",
            Self::OpenSubmenu => "OpenSubmenu",
            Self::FoldFolder => "FoldFolder",
            Self::OpenPaneMenu => "OpenPaneMenu",
            Self::OpenFilesPane => "OpenFilesPane",
            Self::FloatFilesPane => "FloatFilesPane",
            Self::ClosePane => "ClosePane",
            Self::ZoomPane => "ZoomPane",
            Self::SplitPane => "SplitPane",
            Self::NewTerminalInFolder => "NewTerminalInFolder",
            Self::DuplicatePane => "DuplicatePane",
            Self::MoveToNewTab => "MoveToNewTab",
            Self::MoveToNewWindow => "MoveToNewWindow",
            Self::MoveToWindow => "MoveToWindow",
            Self::CopySelection => "CopySelection",
            Self::PasteClipboard => "PasteClipboard",
            Self::SelectAll => "SelectAll",
            Self::ClearScreen => "ClearScreen",
            Self::ClearScrollback => "ClearScrollback",
            Self::RestartShell => "RestartShell",
            Self::OpenFile => "OpenFile",
            Self::OpenWith => "OpenWith",
            Self::RenameFile => "RenameFile",
            Self::CopyPath => "CopyPath",
            Self::InsertPath => "InsertPath",
            Self::RevealInFolder => "RevealInFolder",
            Self::BrowseForFolder => "BrowseForFolder",
            Self::CheckoutBranch => "CheckoutBranch",
            Self::CreateBranch => "CreateBranch",
            Self::CreateTag => "CreateTag",
            Self::RenameBranch => "RenameBranch",
            Self::DeleteBranch => "DeleteBranch",
            Self::DeleteTag => "DeleteTag",
            Self::DiscardChanges => "DiscardChanges",
            Self::StageChange => "StageChange",
            Self::UnstageChange => "UnstageChange",
            Self::LoadMoreCommits => "LoadMoreCommits",
            Self::OpenDiff => "OpenDiff",
            Self::CopyHash => "CopyHash",
            Self::GraphCopyHash => "GraphCopyHash",
            Self::CopySubject => "CopySubject",
            Self::CopyName => "CopyName",
            Self::GoToParentCommit => "GoToParentCommit",
            Self::CompareVersions => "CompareVersions",
            Self::LeaveCompare => "LeaveCompare",
            Self::OpenGitGraph => "OpenGitGraph",
            Self::RereadRepository => "RereadRepository",
            Self::MenuTick => "MenuTick",
            Self::SavePreview => "SavePreview",
            Self::ViewSource => "ViewSource",
            Self::ViewRendered => "ViewRendered",
            Self::OpenDevTools => "OpenDevTools",
            Self::FloatPreview => "FloatPreview",
            Self::LockPreview => "LockPreview",
            Self::NavigateBack => "NavigateBack",
            Self::NavigateForward => "NavigateForward",
            Self::ReloadPage => "ReloadPage",
            Self::StopNavigating => "StopNavigating",
            Self::CopyAddress => "CopyAddress",
            Self::OpenInBrowser => "OpenInBrowser",
            Self::FindInTerminal => "FindInTerminal",
            Self::EditRow => "EditRow",
            Self::DeleteRow => "DeleteRow",
            Self::CloseDialog => "CloseDialog",
            Self::ExpandAdvanced => "ExpandAdvanced",
            Self::RestoreRowDefaults => "RestoreRowDefaults",
            Self::MoveRowUp => "MoveRowUp",
            Self::MoveRowDown => "MoveRowDown",
            Self::OpenRowMenu => "OpenRowMenu",
            Self::RemoveEnvironmentRow => "RemoveEnvironmentRow",
            Self::SplitDirectionAsk => "SplitDirectionAsk",
            Self::SplitDirectionRight => "SplitDirectionRight",
            Self::SplitDirectionDown => "SplitDirectionDown",
            Self::FilesSeat => "FilesSeat",
            Self::PreviewSeat => "PreviewSeat",
            Self::UnknownSeat => "UnknownSeat",
            Self::FolderObject => "FolderObject",
            Self::OpenFolderObject => "OpenFolderObject",
            Self::FileObject => "FileObject",
            Self::PageObject => "PageObject",
            Self::MouseWheel => "MouseWheel",
        }
    }

    /// **The shape this verb wears.** The whole table, in one match.
    ///
    /// The rotating families answer with their resting frame: a chevron's angle
    /// and a disclosure triangle's are *states of one drawing* and not different
    /// shapes, which `ChromeMark::id` already says by giving every angle one
    /// cache name. A caller that turns one asks [`ChromeMark::chevron`] for the
    /// frame it wants; what it may not do is reach for a second drawing.
    #[must_use]
    pub fn mark(self) -> ChromeMark {
        match self {
            Self::OpenSettings => ChromeMark::Gear,
            Self::MinimiseWindow => ChromeMark::WindowMinimize,
            Self::MaximiseWindow => ChromeMark::WindowMaximize,
            // **Closing a surface, and nothing else.** P1 took the other three
            // senses off the cross: deleting goes to the bin below, stopping to
            // the square below that. What is left is one act — and, since 裁2
            // (2026-08-26), *two drawings of it*: the caption's cross is the
            // platform's ten and every other cross in this house is
            // `#i-cross`, which is the same two strokes cut in the house's own
            // grid at the house's own pen. See `marks::ChromeMark::PaneClose`
            // for why that had to be a re-cut and not a smaller box.
            //
            // The dialog's `✕` closes the *dialog*, which is this house's
            // surface and not Windows's, so it is on the house's cross with the
            // tab's and the pane's.
            Self::CloseWindow => ChromeMark::WindowClose,
            Self::CloseTab | Self::CloseDialog => ChromeMark::TabClose,
            // Leaving a comparison closes it: the two versions stop being on
            // screen and nothing is destroyed, which is the cross's own sense
            // and not the bin's.
            Self::ClosePane | Self::LeaveCompare => ChromeMark::PaneClose,
            // **Destroying something.** A branch, a tag, a working-tree change,
            // a row of the reader's own settings.
            Self::DeleteRow
            | Self::RemoveEnvironmentRow
            | Self::DeleteBranch
            | Self::DeleteTag
            | Self::DiscardChanges => ChromeMark::Trash,
            Self::StopNavigating => ChromeMark::Stop,
            // **Making something that was not there.** `Load more` is off this
            // list since P1: it reveals what is already written down.
            Self::NewTab | Self::CreateBranch | Self::StageChange => ChromeMark::Plus,
            Self::LoadMoreCommits => ChromeMark::MoreDown,
            // A list that is folded away, and nothing else: the browser's Back
            // and Forward moved to the arrow below.
            Self::PickProfile | Self::OpenPaneMenu | Self::ExpandAdvanced => {
                ChromeMark::chevron(0.0)
            }
            // **The other disclosure language, and the last one.** The 2026-08-26
            // ruling keeps the filled triangle in the files tree and on a
            // submenu row and nowhere else; both of those are here, so "nowhere
            // else" is a fact about this table rather than a hope about the
            // drawing points.
            Self::OpenSubmenu | Self::FoldFolder => marks::tree_disclosure(0.0),
            // One arrow at four quarter turns. East is forward, west is back,
            // north is the parent of this commit and the row above it, south is
            // the row below.
            Self::NavigateForward => ChromeMark::Arrow { turned_degrees: 0 },
            Self::MoveRowDown => ChromeMark::Arrow { turned_degrees: 90 },
            Self::NavigateBack => ChromeMark::Arrow {
                turned_degrees: 180,
            },
            Self::GoToParentCommit | Self::MoveRowUp => ChromeMark::Arrow {
                turned_degrees: 270,
            },
            // **A folder, once** (裁1, 2026-08-26 — *还是原来的好看*). P2 gave
            // the object a solid and the act a struck rendition of the same
            // silhouette; the acceptance sent the struck pair back and the two
            // drawings are gone. What survives of P2 here is the *reason* the
            // ruling is not an exception to the fill policy: a folder is an
            // object, and a row about an object is a row about a thing however
            // its sentence is worded — which is the entry the fill gate's
            // `FILLED_WITH_A_REASON` now carries under its second class.
            Self::FolderObject
            | Self::FilesSeat
            | Self::OpenFilesPane
            | Self::NewTerminalInFolder => ChromeMark::Folder,
            Self::OpenFolderObject | Self::RevealInFolder | Self::BrowseForFolder => {
                ChromeMark::FolderOpen
            }
            // **One gesture, four containers.** The frame says which one, which
            // is the whole of the 裁2 quarrel: a pane pops out into a float, or
            // leaves for a tab, or for a window of its own, or for one already
            // open. Until P1 the first two were the same drawing.
            Self::FloatFilesPane | Self::FloatPreview => ChromeMark::Float,
            Self::MoveToNewTab => ChromeMark::TabNew,
            Self::MoveToNewWindow => ChromeMark::WindowNew,
            Self::MoveToWindow => ChromeMark::WindowPick,
            // And the bare arrow, which now says one thing: the machine's own
            // program takes this.
            Self::OpenWith | Self::OpenInBrowser => ChromeMark::External,
            Self::ZoomPane => ChromeMark::PaneZoom { zoomed: false },
            Self::SplitPane | Self::SplitDirectionAsk => ChromeMark::Split,
            Self::SplitDirectionRight => ChromeMark::SplitRight,
            Self::SplitDirectionDown => ChromeMark::SplitDown,
            Self::CompareVersions => ChromeMark::Compare,
            // Putting text on the clipboard — one sense, whatever the text is.
            Self::CopySelection
            | Self::CopyPath
            | Self::CopySubject
            | Self::CopyName
            | Self::CopyAddress => ChromeMark::Copy,
            Self::CopyHash | Self::GraphCopyHash => ChromeMark::Hash,
            Self::DuplicatePane => ChromeMark::Duplicate,
            Self::PasteClipboard | Self::InsertPath => ChromeMark::Paste,
            Self::SelectAll => ChromeMark::SelectAll,
            Self::ClearScreen => ChromeMark::Broom,
            Self::ClearScrollback => ChromeMark::Eraser,
            // Fetching the same thing again — and *not* restarting a shell,
            // which throws a process away.
            Self::RereadRepository | Self::ReloadPage => ChromeMark::Refresh,
            Self::RestartShell => ChromeMark::Restart,
            Self::RestoreRowDefaults => ChromeMark::HistoryRestore,
            Self::OpenRowMenu => ChromeMark::More,
            Self::FindInTerminal => ChromeMark::Search,
            Self::OpenFile | Self::OpenDiff | Self::FileObject | Self::PreviewSeat => {
                ChromeMark::File
            }
            Self::PageObject => ChromeMark::Globe { favicon: None },
            Self::MouseWheel => ChromeMark::MouseWheel,
            Self::UnknownSeat => ChromeMark::Panel,
            Self::RenameFile | Self::RenameBranch | Self::EditRow => ChromeMark::Pencil,
            Self::CheckoutBranch => ChromeMark::GitBranch,
            Self::CreateTag => ChromeMark::Tag,
            Self::UnstageChange => ChromeMark::Minus,
            Self::OpenGitGraph => ChromeMark::GitGraph,
            Self::MenuTick => ChromeMark::Check,
            Self::SavePreview => ChromeMark::Save,
            Self::ViewRendered => ChromeMark::Eye,
            Self::ViewSource => ChromeMark::Code,
            Self::OpenDevTools => ChromeMark::DevTools,
            Self::LockPreview => ChromeMark::Lock { engaged: false },
        }
    }

    /// **Whether this entry names a thing or an act** — P2's fill policy, and
    /// the line it is drawn along.
    ///
    /// The registry's opening paragraph already said the table is a table of
    /// *verbs*, "with the handful of fixed object marks a row can wear" allowed
    /// in because those were the arms the dispatchers were choosing by hand.
    /// P2 needs the two halves told apart rather than merely acknowledged,
    /// because the fill policy applies to one of them and not the other:
    ///
    /// * **An act is struck.** A menu's icon column is a run of outlines at one
    ///   weight, and a solid among them reads as a badge somebody pasted in —
    ///   which is the 2026-08-26 measurement of the pane menu, where every row
    ///   came into a `1.36×` ink band except the filled folder at more than
    ///   twice its neighbours'.
    /// * **A thing may be filled.** A tree row, a breadcrumb chip, a tab's
    ///   mark, a seat's: these say *what this is*, they stand beside a name
    ///   rather than beside a verb, and a solid is how a small identity mark
    ///   survives being small.
    ///
    /// **裁1 (2026-08-26) moved the folder across that line rather than moving
    /// the line.** P2 read `Reveal in folder` as an act and struck it; the
    /// acceptance's answer was *还是原来的好看*, and the reason the ruling is
    /// right is that a folder row is not saying *reveal* with a picture, it is
    /// saying *a folder* beside the word `Reveal`. The four folder rows are
    /// therefore filled with the reason a thing is filled — see
    /// `FILLED_WITH_A_REASON`'s second class, which is where an entry that is a
    /// *verb* wearing an object's drawing has to argue for itself. This
    /// function stays the list of entries that name nothing but the thing.
    #[must_use]
    #[cfg(test)]
    pub fn is_an_object(self) -> bool {
        matches!(
            self,
            Self::FilesSeat
                | Self::PreviewSeat
                | Self::UnknownSeat
                | Self::FolderObject
                | Self::OpenFolderObject
                | Self::FileObject
                | Self::PageObject
                | Self::MouseWheel
        )
    }

    /// **The same drawing, at the frame of its turn this control is showing.**
    ///
    /// The chevron and the disclosure triangle are one drawing each with an
    /// angle on them, which is why `ChromeMark::id` gives every angle one cache
    /// name. A control that turns one therefore asks the registry *which
    /// drawing* and supplies *which frame* — the two halves of the sentence
    /// stay where each of them belongs, and a control cannot quietly swap in a
    /// second arrow for its open state.
    #[must_use]
    pub fn turned(self, turn: f32) -> ChromeMark {
        match self.mark() {
            ChromeMark::Chevron { .. } => ChromeMark::chevron(turn),
            ChromeMark::TreeDisclosure { .. } => marks::tree_disclosure(turn),
            still => still,
        }
    }

    /// **The same drawing, in the state this control is in** — the pin's fill,
    /// the lock's shackle, the zoom's brackets.
    ///
    /// [`Self::turned`]'s argument in the other shape the mock-up uses for a
    /// state: Fluent's fill axis, where regular-and-open is the action and
    /// filled-and-shut is the state. Same division of labour — the registry
    /// answers which drawing, the control answers which face.
    #[must_use]
    pub fn engaged(self, on: bool) -> ChromeMark {
        match self.mark() {
            ChromeMark::Lock { .. } => ChromeMark::Lock { engaged: on },
            ChromeMark::Pin { .. } => ChromeMark::Pin { filled: on },
            ChromeMark::PaneZoom { .. } => ChromeMark::PaneZoom { zoomed: on },
            still => still,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// **Every box the chrome draws a mark in**, for the optical gate to walk.
    ///
    /// Two kinds of row, and the difference between them is the whole state of
    /// this block of work:
    ///
    /// * A row built from a [`MarkSlot`] is a drawing point that has been
    ///   converted — it reports its slot and the box is derived here. It cannot
    ///   drift, because there is no second number.
    /// * A row built from a `*_LOGICAL_PX` constant is a drawing point that has
    ///   not been converted yet. It names the crate's own constant rather than
    ///   repeating its value, so a surface that re-sizes itself is re-measured
    ///   here on the next run.
    fn draw_sites() -> Vec<(&'static str, ChromeMark, [f32; 2])> {
        let mut sites: Vec<(&'static str, ChromeMark, [f32; 2])> = Vec::new();

        // ── converted: the slot table's own tenants ──────────────────────
        //
        // A menu row can wear any verb's mark, so the menu slot is walked
        // against the whole registry rather than against a list somebody would
        // have to remember to extend.
        for icon in ActionIcon::ALL {
            let mark = icon.mark();
            sites.push(("menu row", mark, MarkSlot::Menu.mark_box_logical_px(mark)));
        }
        for mark in [
            ChromeMark::chevron(0.0),
            // The head's `🗀` is the solid again since 裁1 — see this module's
            // fill policy, which is now a policy with one argued exception in
            // it rather than a second folder.
            ChromeMark::Folder,
            ChromeMark::PaneClose,
        ] {
            sites.push((
                "pane head run",
                mark,
                MarkSlot::CompactHead.mark_box_logical_px(mark),
            ));
        }
        for mark in [
            ChromeMark::WindowMinimize,
            ChromeMark::WindowMaximize,
            ChromeMark::WindowClose,
        ] {
            sites.push((
                "window caption",
                mark,
                MarkSlot::Caption.mark_box_logical_px(mark),
            ));
        }
        sites.push((
            "Git panel masthead",
            ChromeMark::GitBranch,
            MarkSlot::Toolbar.mark_box_logical_px(ChromeMark::GitBranch),
        ));

        // ── not converted yet: bespoke boxes, read off their own constants ──
        //
        // Named rather than repeated: the value is the crate's own constant, so
        // a surface that re-sizes itself is re-measured here on the next run.
        let mut squares: Vec<(&'static str, ChromeMark, f32)> = Vec::new();
        let mut wide: Vec<(&'static str, ChromeMark, [f32; 2])> = Vec::new();
        let mut bespoke = |surface: &'static str, mark: ChromeMark, side: f32| {
            squares.push((surface, mark, side));
        };
        bespoke(
            "pane head float",
            ChromeMark::Float,
            crate::seats::PANE_HEAD_FLOAT_GLYPH_LOGICAL_PX,
        );
        bespoke(
            "pane head zoom",
            ChromeMark::PaneZoom { zoomed: true },
            crate::seats::PANE_ZOOM_MARK_LOGICAL_PX,
        );
        bespoke(
            "files row",
            ChromeMark::File,
            crate::seats::FILES_ROW_ICON_LOGICAL_PX,
        );
        bespoke(
            "files foot",
            ChromeMark::Folder,
            crate::seats::FILES_FOOT_MARK_LOGICAL_PX,
        );
        bespoke(
            "preview head tool",
            ChromeMark::Save,
            crate::seats::PREVIEW_TOOL_GLYPH_LOGICAL_PX,
        );
        bespoke(
            "preview head tool",
            ChromeMark::Code,
            crate::seats::PREVIEW_TOOL_GLYPH_LOGICAL_PX,
        );
        bespoke(
            "preview head tool",
            ChromeMark::Eye,
            crate::seats::PREVIEW_TOOL_GLYPH_LOGICAL_PX,
        );
        bespoke(
            "preview head tool",
            ChromeMark::Lock { engaged: false },
            crate::seats::PREVIEW_TOOL_GLYPH_LOGICAL_PX,
        );
        for mark in [
            ChromeMark::Refresh,
            ChromeMark::PaneClose,
            ChromeMark::Chevron { turned_degrees: 90 },
        ] {
            wide.push((
                "preview head nav",
                mark,
                MarkSlot::CompactHead.mark_box_logical_px(mark),
            ));
        }
        wide.push((
            "preview switcher chevron",
            ChromeMark::chevron(0.0),
            [
                crate::seats::PREVIEW_SWITCH_CHEVRON_WIDTH_LOGICAL_PX,
                crate::seats::PREVIEW_SWITCH_CHEVRON_HEIGHT_LOGICAL_PX,
            ],
        ));
        wide.push((
            "files root chevron",
            ChromeMark::chevron(0.0),
            [
                crate::seats::FILES_ROOT_CHEVRON_WIDTH_LOGICAL_PX,
                crate::seats::FILES_ROOT_CHEVRON_HEIGHT_LOGICAL_PX,
            ],
        ));
        wide.push((
            "lone pane ghost chevron",
            ChromeMark::chevron(0.0),
            [
                crate::seats::PANE_GHOST_GLYPH_WIDTH_LOGICAL_PX,
                crate::seats::PANE_GHOST_GLYPH_HEIGHT_LOGICAL_PX,
            ],
        ));
        wide.push((
            "strip profile picker chevron",
            ChromeMark::chevron(0.0),
            [
                bt_render::WINDOW_NEW_TAB_CHEVRON_WIDTH_LOGICAL_PX,
                bt_render::WINDOW_NEW_TAB_CHEVRON_HEIGHT_LOGICAL_PX,
            ],
        ));
        bespoke(
            "strip new tab",
            ChromeMark::Plus,
            bt_render::WINDOW_NEW_TAB_GLYPH_LOGICAL_PX,
        );
        for surface in ["strip tab close", "focus card close", "toast close"] {
            wide.push((
                surface,
                ChromeMark::TabClose,
                MarkSlot::CompactHead.mark_box_logical_px(ChromeMark::TabClose),
            ));
        }
        // The pin, on the two surfaces that wear it, at one drawing size: a
        // card's slot is 16px against the strip's 17, near enough that a second
        // cut would be two drawings of one control — and the card's own 11px,
        // which the mock-up asked for, draws a 0.825px pen against a band of
        // 0.95..=1.15. Both faces, because the fill axis is what carries the
        // state.
        for surface in ["strip tab pin", "focus card pin"] {
            for filled in [false, true] {
                bespoke(
                    surface,
                    ChromeMark::Pin { filled },
                    crate::seats::WINDOW_TAB_PIN_GLYPH_LOGICAL_PX,
                );
            }
        }
        bespoke(
            "strip tab mark",
            ChromeMark::File,
            bt_render::WINDOW_TAB_MARK_LOGICAL_PX,
        );
        bespoke(
            "window panel toggle",
            ChromeMark::Panel,
            crate::seats::WINDOW_PANEL_TOGGLE_GLYPH_LOGICAL_PX,
        );
        bespoke(
            "caption gear",
            ChromeMark::Gear,
            bt_render::WINDOW_CAPTION_GEAR_GLYPH_LOGICAL_PX,
        );
        bespoke(
            "Git row button",
            ChromeMark::Plus,
            crate::git_panel::GIT_ACT_GLYPH_LOGICAL_PX,
        );
        bespoke(
            "Git row button",
            ChromeMark::Minus,
            crate::git_panel::GIT_ACT_GLYPH_LOGICAL_PX,
        );
        bespoke(
            "Git row button",
            ChromeMark::Refresh,
            crate::git_panel::GIT_ACT_GLYPH_LOGICAL_PX,
        );
        bespoke(
            "Git row button",
            ChromeMark::GitGraph,
            crate::git_panel::GIT_ACT_GLYPH_LOGICAL_PX,
        );
        bespoke(
            "Git remotes head",
            ChromeMark::GitBranch,
            crate::git_panel::GIT_REMOTES_MARK_LOGICAL_PX,
        );
        bespoke(
            "graph tool",
            ChromeMark::Code,
            crate::git_graph::GRAPH_TOOL_MARK_LOGICAL_PX,
        );
        bespoke(
            "graph refresh",
            ChromeMark::Refresh,
            crate::git_graph::GRAPH_REFRESH_MARK_LOGICAL_PX,
        );
        bespoke(
            "graph ref tag",
            ChromeMark::Tag,
            crate::git_graph::GRAPH_REF_TAG_MARK_LOGICAL_PX,
        );
        bespoke(
            "float head mark",
            ChromeMark::File,
            crate::float::FLOAT_HEAD_MARK_LOGICAL_PX,
        );
        wide.push((
            "float close",
            ChromeMark::PaneClose,
            MarkSlot::CompactHead.mark_box_logical_px(ChromeMark::PaneClose),
        ));
        bespoke(
            "float dock",
            ChromeMark::DockLeft,
            crate::float::FLOAT_DOCK_GLYPH_LOGICAL_PX,
        );
        bespoke(
            "float grip",
            ChromeMark::ResizeGrip,
            crate::float::FLOAT_GRIP_GLYPH_LOGICAL_PX,
        );
        bespoke(
            "peek strip leaf",
            ChromeMark::File,
            crate::peek_strip::LEAF_MARK_LOGICAL_PX,
        );
        bespoke(
            "peek strip list",
            ChromeMark::File,
            crate::peek_strip::LIST_MARK_LOGICAL_PX,
        );
        bespoke(
            "file peek head",
            ChromeMark::File,
            crate::file_peek::PEEK_MARK_LOGICAL_PX,
        );
        bespoke(
            "restore row",
            ChromeMark::File,
            crate::restore::ROW_MARK_LOGICAL_PX,
        );
        bespoke(
            "toast mark",
            ChromeMark::File,
            crate::toast::TOAST_MARK_LOGICAL_PX,
        );
        for mark in [ChromeMark::Pencil, ChromeMark::Trash] {
            wide.push((
                "settings row button",
                mark,
                MarkSlot::CompactHead.mark_box_logical_px(mark),
            ));
        }
        // The dialog's own header cross, which took the compact head's box in
        // 裁2 — see `settings::push_dialog_head`.
        wide.push((
            "settings dialog close",
            ActionIcon::CloseDialog.mark(),
            MarkSlot::CompactHead.mark_box_logical_px(ActionIcon::CloseDialog.mark()),
        ));
        bespoke(
            "search bar button",
            ChromeMark::chevron(0.0),
            crate::search::BUTTON_GLYPH_LOGICAL_PX,
        );
        // **The two breadcrumbs' punctuation is not here** (裁3, 2026-08-26).
        // P1 drew each surface's `‹` and `›` as the house's arrow at the compact
        // head's box, and this gate walked them; the acceptance sent them back
        // to the characters they were, on the argument this module's own
        // `no_font_character_stands_in_for_a_mark` now carries as a class — a
        // breadcrumb is a sentence and a guillemet is its comma. A run of type
        // has no pen for a band to hold.
        //
        // The settings dialog's own row verbs, which were five font characters
        // in the same edit.
        for icon in [
            ActionIcon::RestoreRowDefaults,
            ActionIcon::MoveRowUp,
            ActionIcon::MoveRowDown,
            ActionIcon::OpenRowMenu,
            ActionIcon::RemoveEnvironmentRow,
        ] {
            wide.push((
                "settings row verb",
                icon.mark(),
                MarkSlot::CompactHead.mark_box_logical_px(icon.mark()),
            ));
        }
        sites.extend(
            squares
                .into_iter()
                .map(|(surface, mark, side)| (surface, mark, [side, side])),
        );
        sites.extend(wide);
        sites
    }

    /// **What the band does not govern**, and why.
    ///
    /// The band is a rule about *control glyphs* — a mark on a button, in a
    /// menu, on a head. These surfaces draw a mark below button scale on
    /// purpose, and holding a nine-pixel schematic to a button's pen would be
    /// the rule making the schematic wrong rather than the schematic making the
    /// rule wrong:
    ///
    /// * **The peek strip's schematic** (`9`/`11`) is a *diagram of the window*
    ///   at the size of a strip row; its marks are the pane heads' own artwork
    ///   with the size dropped, which is the whole point of sharing them.
    /// * **The graph's furniture** (`9`/`10`/`13`) is measured against a commit
    ///   row's height, not against a button.
    /// * **The Git panel's inline row buttons** (`11`) are a run inside a list
    ///   row, and its remotes header (`10`) is a section label's mark.
    /// * **Identity marks** — the float head's `#i-file`, the tab strip's, the
    ///   files column's, the restore list's, a toast's — say *what this is*
    ///   rather than *what pressing this does*, and they are sized to the line
    ///   of type they stand beside.
    /// * **The caption's gear** is the one foreign drawing (Material's 24-unit
    ///   fill) and the 2026-08-26 ruling keeps it; a fill has no pen anyway.
    ///
    /// P2 decides which of these should become slots. Naming them here rather
    /// than quietly skipping them is the difference between a scope and a hole.
    const NOT_A_CONTROL_SLOT: &[&str] = &[
        "peek strip leaf",
        "peek strip list",
        "file peek head",
        "graph tool",
        "graph refresh",
        "graph ref tag",
        "Git row button",
        "Git remotes head",
        "float head mark",
        "float dock",
        "files row",
        "files foot",
        "strip tab mark",
        "strip tab pin",
        "window panel toggle",
        "caption gear",
        "restore row",
        "toast mark",
        "preview switcher chevron",
        "files root chevron",
        "search bar button",
        "lone pane ghost chevron",
        "pane head float",
        "pane head zoom",
    ];

    /// **The transitional exemption list is empty.**
    ///
    /// P0 left six drawings on it, and the rule it held them to was that *no
    /// box could fix them*: each was outside the band at every box its family
    /// could legitimately take, because what was wrong was the pen against its
    /// own box rather than the box. `#i-plus` and `#i-minus` wrote `0.12` of
    /// pen per unit and `#i-chev` the same, `#i-code` `0.0875`, `#i-check`
    /// `0.10`, the grip `0.1875`; the house writes `1.2 / 16`, which is
    /// `0.075`. Nothing about a slot changes a ratio.
    ///
    /// P1 changed the ratios, which was the only thing left to change. Four of
    /// the six were re-cut into the house's sixteen (`#i-plus`, `#i-minus`,
    /// `#i-chev`, the grip) and two had their pens brought to the house's
    /// (`#i-check` from `1.6`, `#i-code` from `1.4`). What remains here is
    /// [`NOT_A_CONTROL_SLOT`], which is a different claim entirely — not "this
    /// drawing is wrong" but "the band is not about this surface".
    fn exempt(surface: &str, _mark: ChromeMark) -> bool {
        NOT_A_CONTROL_SLOT.contains(&surface)
    }

    /// The other half of P0's rule, and now the whole of it: **every drawing
    /// the chrome puts on a button lands in the band in the house's own box.**
    ///
    /// The box is the menu's `14`, which is the number the band's `1.05` is
    /// *defined* at (`1.2 / 16 × 14`). Take `#i-check` back to `1.6` and this
    /// goes red at `1.400`; take `#i-chev` back to `10 × 6` and it goes red at
    /// `1.344`; put `#i-copy` back on a 15px box and it reads `1.219`, which is
    /// the number the 2026-08-25 audit measured across the terminal menu's
    /// whole run.
    #[test]
    fn no_drawing_is_waiting_for_a_re_cut() {
        let [floor, ceiling] = OPTICAL_STROKE_BAND_LOGICAL_PX;
        let mut wrong = Vec::new();
        for icon in ActionIcon::ALL {
            let mark = icon.mark();
            let [width, height] = MarkSlot::Menu.mark_box_logical_px(mark);
            let Some(optical) = mark.optical_stroke_logical_px(width, height) else {
                continue;
            };
            if optical < floor || optical > ceiling {
                wrong.push(format!("{} reads {optical:.3}", mark.drawing_id()));
            }
        }
        assert!(
            wrong.is_empty(),
            "these do not draw the house pen in the house's own box:\n{}",
            wrong.join("\n"),
        );
    }

    /// **The red gate.** Every mark, in every box the chrome draws it in, has to
    /// land inside [`OPTICAL_STROKE_BAND_LOGICAL_PX`].
    ///
    /// Written before the sizing was changed and red on the day it was written:
    /// the pane head's `⌄` measured `1.56` against its `✕`'s `0.80`, `#i-minus`
    /// measured `1.80` in a Git menu against the `#i-plus` it is contractually
    /// the same weight as (`marks.rs`'s own note on `Minus`), and the peek
    /// strip's `#i-file` measured `0.65`. What the gate buys is that none of
    /// those can come back quietly: a new drawing point is a new row here, and a
    /// row that misses the band has to be argued onto `exempt` with the redraw
    /// that closes it.
    #[test]
    fn every_mark_in_every_slot_draws_the_house_pen() {
        let [floor, ceiling] = OPTICAL_STROKE_BAND_LOGICAL_PX;
        let mut unexpected = Vec::new();
        for (surface, mark, [width, height]) in draw_sites() {
            let Some(optical) = mark.optical_stroke_logical_px(width, height) else {
                // A pure fill, a brand mark or a generated shape: no pen, so no
                // band. `design_stroke_units` is where that list is stated.
                continue;
            };
            if optical >= floor && optical <= ceiling {
                continue;
            }
            if exempt(surface, mark) {
                continue;
            }
            unexpected.push(format!(
                "{surface}: {} at {width}\u{d7}{height} draws {optical:.3}px",
                mark.drawing_id(),
            ));
        }
        assert!(
            unexpected.is_empty(),
            "these draw the house pen at the wrong weight and are not on the transitional list:\n{}",
            unexpected.join("\n"),
        );
    }

    /// The gate above is only worth what its list of boxes is worth, so the list
    /// has to actually contain the surfaces the audit measured.
    #[test]
    fn the_gate_walks_the_surfaces_the_audit_measured() {
        let sites = draw_sites();
        for surface in [
            "menu row",
            "pane head run",
            "window caption",
            "preview head tool",
            "peek strip leaf",
            "Git row button",
            "float close",
            "settings dialog close",
            "settings row verb",
        ] {
            assert!(
                sites.iter().any(|(name, _, _)| *name == surface),
                "the optical gate does not walk {surface}",
            );
        }
    }

    /// **The pane head's three, which is the report this block started from.**
    ///
    /// A user reported the run reading as three different weights; the audit
    /// measured `1.56 : 0.80` between the `⌄` and the `✕`, and traced it to two
    /// causes stacked — a `13px` square handed to a `10×6` arrow, and an `8px`
    /// box handed to the `×` beside it. Both are the slot's business, so both
    /// are fixed here: one slot, one derivation, and the three marks lay the
    /// same width of ink across the head.
    ///
    /// **This is one of the two things a run has to get right and it was never
    /// the one the reader was looking at** — see `a_heads_run_is_one_picture`,
    /// which is the other. The cross is off this assertion since 裁2: it lays
    /// `8.1px` of ink where its neighbours lay `10.4`, deliberately, because
    /// levelling its *picture* is what levelling a cross means.
    #[test]
    fn the_pane_heads_run_lays_one_width_of_ink() {
        // How many logical pixels of ink the mark lays across the head: the box
        // it is given, times the fraction of its own box its artwork covers.
        let ink = |mark: ChromeMark| {
            let [width, _] = MarkSlot::CompactHead.mark_box_logical_px(mark);
            if mark.draws_edge_to_edge() {
                width
            } else {
                width * marks::HOUSE_INK_RATIO
            }
        };
        let chevron = ink(ChromeMark::chevron(0.0));
        let folder = ink(ChromeMark::Folder);
        assert!(
            (chevron - folder).abs() < 0.01,
            "the pane head's run draws {chevron:.2} and {folder:.2} of ink",
        );
    }

    /// **The runs the chrome stands marks in side by side**, for the picture
    /// band to walk — each one a surface and the marks it draws, left to right.
    ///
    /// A *run* is the thing this band is about, and it is a narrower claim than
    /// a slot. `MarkSlot::Menu` is not a run: a menu row can wear any of the
    /// registry's eighty-six verbs, the reader is reading names down a column
    /// rather than comparing three buttons, and the column's own pictures span
    /// `9.2` (`#i-tri`) to `16.8` (`#i-pencil`) — a spread of eighty per cent
    /// that no re-cut should close, because a tick and a pencil are not the
    /// same kind of sign and a menu does not ask anyone to weigh them against
    /// each other. Neither is the window caption, whose three are the
    /// platform's own set at the platform's own sizes (a `10px` bar beside a
    /// `10px` square is already `41%` apart, and Windows draws it that way).
    ///
    /// What is here is the place a reader *does* compare: a short row of
    /// buttons on one head, met as a group.
    fn head_runs() -> Vec<(&'static str, Vec<ChromeMark>)> {
        vec![(
            "pane head",
            vec![
                ChromeMark::chevron(0.0),
                ChromeMark::Folder,
                ChromeMark::PaneClose,
            ],
        )]
    }

    /// **A run of buttons on one head is one picture** —
    /// [`OPTICAL_PICTURE_SPREAD`], measured off the raster.
    ///
    /// RED EVIDENCE (2026-08-26, 裁2 — 「`✕` 视觉上比 `⌄` 大」). With
    /// `PaneClose` on `#i-close`, the caption's ten-unit cross, the pane head's
    /// run measured:
    ///
    /// ```text
    /// i-chev         ink 12.88 × 6.50 units   →  10.46 × 5.28 px   picture 11.718
    /// i-folder       ink 12.88 × 10.38 units  →  10.46 × 8.43 px   picture 13.435
    /// i-close        ink 10.00 × 10.00 units  →  10.40 × 10.40 px  picture 14.708
    /// ```
    ///
    /// — `14.708 / 11.718 = 1.255`, red against a band of `1.20`. Every other
    /// gate in this module was green on that run: the pens read `0.975`, `—`
    /// and `1.040`, all inside the stroke band, and the ink widths read
    /// `10.46`, `10.46` and `10.40`, level to a twentieth of a pixel. The
    /// report was about none of that.
    ///
    /// MUTATION: point `ClosePane` back at a ten-unit cross — give
    /// `ChromeMark::PaneClose` `symbol_index` `3` again — and this goes red
    /// with the numbers above.
    #[test]
    fn a_heads_run_is_one_picture() {
        let mut wrong = Vec::new();
        for (surface, run) in head_runs() {
            let pictures: Vec<(&'static str, f32)> = run
                .iter()
                .filter_map(|mark| {
                    let [width, height] = MarkSlot::CompactHead.mark_box_logical_px(*mark);
                    Some((
                        mark.drawing_id(),
                        mark.optical_picture_logical_px(width, height)?,
                    ))
                })
                .collect();
            let biggest = pictures
                .iter()
                .copied()
                .fold(
                    ("", f32::MIN),
                    |so_far, one| if one.1 > so_far.1 { one } else { so_far },
                );
            let smallest = pictures
                .iter()
                .copied()
                .fold(
                    ("", f32::MAX),
                    |so_far, one| if one.1 < so_far.1 { one } else { so_far },
                );
            let spread = biggest.1 / smallest.1 - 1.0;
            if spread <= OPTICAL_PICTURE_SPREAD {
                continue;
            }
            wrong.push(format!(
                "{surface}: {} makes a {:.3} picture beside {}'s {:.3} — {:.1}% apart",
                biggest.0,
                biggest.1,
                smallest.0,
                smallest.1,
                spread * 100.0,
            ));
        }
        assert!(
            wrong.is_empty(),
            "a run of buttons on one head is read as one group:\n{}",
            wrong.join("\n"),
        );
    }

    /// And the two the report actually names, on their own, so that "the `✕` is
    /// the size of the `⌄`" is a sentence in this file rather than an average
    /// over a run that a third drawing could carry.
    #[test]
    fn a_pane_heads_cross_is_the_size_of_its_arrow() {
        let picture = |mark: ChromeMark| {
            let [width, height] = MarkSlot::CompactHead.mark_box_logical_px(mark);
            mark.optical_picture_logical_px(width, height)
                .expect("a quoted symbol has a picture")
        };
        let arrow = picture(ChromeMark::chevron(0.0));
        let cross = picture(ChromeMark::PaneClose);
        let apart = (cross / arrow - 1.0).abs();
        assert!(
            apart <= 0.05,
            "the head's cross makes a {cross:.3} picture beside its arrow's \
             {arrow:.3} — {:.1}% apart",
            apart * 100.0,
        );
    }

    /// **The derivation the cross's own grid is cut from**, checked against the
    /// drawing it was derived off rather than left as a number in a comment.
    ///
    /// `#i-cross` is ten units of ink in the house's sixteen because a square
    /// whose *diagonal* is the arrow's picture measures `14.31 / √2 = 10.12` on
    /// a side. Re-cut `#i-chev` and this says so, which is the point: the two
    /// drawings are tied together by the reason one of them is that size.
    #[test]
    fn the_crosss_ink_is_the_arrows_picture_turned_square() {
        let [across, down] = ChromeMark::chevron(0.0)
            .ink_extent_units()
            .expect("the arrow is a quoted symbol");
        let wanted = across.hypot(down) / std::f32::consts::SQRT_2;
        let [cross_across, cross_down] = ChromeMark::PaneClose
            .ink_extent_units()
            .expect("the cross is a quoted symbol");
        assert!(
            (cross_across - cross_down).abs() < 0.2,
            "the cross's ink is square: {cross_across:.2} × {cross_down:.2}",
        );
        assert!(
            (cross_across - wanted).abs() < 0.3,
            "the cross is cut at {cross_across:.2} units where the arrow's \
             picture asks for {wanted:.2}",
        );
    }

    /// **`Plus` and `Minus` are one drawing minus a stroke, so they are one
    /// weight** — `marks.rs`'s own written contract, which the menu's sizing
    /// broke by listing `Plus` among the edge-to-edge marks and leaving `Minus`
    /// out of the list. `Unstage` drew at `1.80` where `Stage` drew at `1.20`.
    #[test]
    fn the_plus_and_the_minus_are_the_same_weight_in_every_slot() {
        for slot in MarkSlot::ALL {
            let plus = ChromeMark::Plus
                .optical_stroke_logical_px(
                    slot.mark_box_logical_px(ChromeMark::Plus)[0],
                    slot.mark_box_logical_px(ChromeMark::Plus)[1],
                )
                .expect("the plus has a pen");
            let minus = ChromeMark::Minus
                .optical_stroke_logical_px(
                    slot.mark_box_logical_px(ChromeMark::Minus)[0],
                    slot.mark_box_logical_px(ChromeMark::Minus)[1],
                )
                .expect("the minus has a pen");
            assert!(
                (plus - minus).abs() < f32::EPSILON,
                "{}: the plus draws {plus} and the minus {minus}",
                slot.name(),
            );
        }
    }

    /// **The reverse index.** Which verbs wear each shape, and every shape more
    /// than one verb wears written down here.
    ///
    /// The list is the audit's own finding, in the audit's own words — a shape
    /// shared by two verbs is a shape the reader has to disambiguate from the
    /// row's text, and the four rows the users actually reported (`Move pane to
    /// new window` beside `Move to window ▸`, rasterizing *identically*) are
    /// what that costs.
    ///
    /// `true` means **a split is still owed**. **Nothing on this list carries
    /// one any more**, which is the other half of P1's acceptance: the nine
    /// groups P0 recorded as owing one were split, and what is left is the five
    /// P0 already judged to be one sense wearing one shape, plus the two the
    /// splits produced. A shape worn by several verbs is not a fault — it is
    /// what a shape is *for*, when the verbs are the same act aimed at
    /// different nouns. What was a fault was the shape standing for two
    /// different acts, and there is none of that left.
    const REUSED_SHAPES: &[(&str, &[ActionIcon], bool)] = &[
        // Closing a surface. Four names for one act, and `LeaveCompare` because
        // leaving a comparison puts two versions away and destroys nothing —
        // which is the cross's sense and not the bin's.
        //
        // **`#i-close` came off this list in 裁2** and is worn by
        // `CloseWindow` alone: the caption's cross is Windows's drawing at
        // Windows's size, and every cross this house draws for itself is the
        // one below. A split by *rendition* rather than by sense, on the two
        // folders' precedent — one act, two grids, and the reader never meets
        // the two in one run.
        (
            "i-cross",
            &[
                ActionIcon::CloseDialog,
                ActionIcon::CloseTab,
                ActionIcon::ClosePane,
                ActionIcon::LeaveCompare,
            ],
            false,
        ),
        // Destroying something. P1 struck this to take four verbs off the
        // cross: a branch, a tag, a working-tree change and a row of the
        // reader's own settings are not "closed".
        (
            "i-trash",
            &[
                ActionIcon::DeleteRow,
                ActionIcon::RemoveEnvironmentRow,
                ActionIcon::DeleteBranch,
                ActionIcon::DeleteTag,
                ActionIcon::DiscardChanges,
            ],
            false,
        ),
        // One arrow at four quarter turns, which is a direction and not a
        // second drawing — `ChromeMark::PaneZoom`'s note said the same thing
        // about a pair of brackets.
        (
            "i-arrow",
            &[
                ActionIcon::NavigateBack,
                ActionIcon::NavigateForward,
                ActionIcon::GoToParentCommit,
                ActionIcon::MoveRowUp,
                ActionIcon::MoveRowDown,
            ],
            false,
        ),
        // Making something that was not there. `Load more` came off this list
        // in P1 and wears `#i-more-down`.
        (
            "i-plus",
            &[
                ActionIcon::NewTab,
                ActionIcon::CreateBranch,
                ActionIcon::StageChange,
            ],
            false,
        ),
        // A list that is folded away. The browser's Back and Forward came off
        // this list in P1 and wear the arrow above; the settings dialog's
        // `Advanced` joined it in P2, which is where it had been drawing since
        // P1 without the table knowing.
        (
            "i-chev",
            &[
                ActionIcon::PickProfile,
                ActionIcon::OpenPaneMenu,
                ActionIcon::ExpandAdvanced,
            ],
            false,
        ),
        // **The other disclosure, and the last one.** One triangle at two
        // orientations: pointing right on a row that opens another menu,
        // turned down on a folder that is open. The 2026-08-26 ruling keeps it
        // in exactly these two places, and both of them are now rows of this
        // table rather than constructions in two files.
        (
            "i-tri",
            &[ActionIcon::OpenSubmenu, ActionIcon::FoldFolder],
            false,
        ),
        // Fetching the same thing again. `Restart shell` came off this list in
        // P1 and wears `#i-restart`, because restarting throws a process away.
        (
            "i-refresh",
            &[ActionIcon::RereadRepository, ActionIcon::ReloadPage],
            false,
        ),
        // Cutting a pane in two. `Compare with…` came off this list in P1.
        (
            "i-split",
            &[ActionIcon::SplitPane, ActionIcon::SplitDirectionAsk],
            false,
        ),
        // Putting text on the clipboard. `Duplicate pane` and `Copy hash` came
        // off this list in P1 — the first is not text and the second is a
        // hexadecimal name, which the graph's detail card was right to want
        // told apart.
        (
            "i-copy",
            &[
                ActionIcon::CopySelection,
                ActionIcon::CopyPath,
                ActionIcon::CopySubject,
                ActionIcon::CopyName,
                ActionIcon::CopyAddress,
            ],
            false,
        ),
        // A commit's hexadecimal name, wherever it is copied from.
        (
            "i-hash",
            &[ActionIcon::CopyHash, ActionIcon::GraphCopyHash],
            false,
        ),
        // Popping a pane out into a floating window. `Move pane to new tab`
        // came off this list in P1 and wears a tab.
        (
            "i-float",
            &[ActionIcon::FloatFilesPane, ActionIcon::FloatPreview],
            false,
        ),
        // Handing content to the program the machine keeps for it. `Move to new
        // window` and `Move to window ▸` came off this list in P1 and wear
        // windows, which is what settled 裁2: a bare arrow means one thing.
        (
            "i-external",
            &[ActionIcon::OpenWith, ActionIcon::OpenInBrowser],
            false,
        ),
        // One sense, several rows. A folder is a folder and a file is a file;
        // that a tree row and a seat's mark both say so with one drawing is the
        // drawing doing its job.
        //
        // **P2 split the rendition and 裁1 put it back** (2026-08-26): the four
        // rows below are a tree row, a seat's mark, a head's button and a menu
        // row, and all four are about a folder. That the acts sit on this list
        // beside the objects is the ruling written where a reader of the table
        // meets it.
        (
            "i-folder",
            &[
                ActionIcon::FolderObject,
                ActionIcon::FilesSeat,
                ActionIcon::OpenFilesPane,
                ActionIcon::NewTerminalInFolder,
            ],
            false,
        ),
        // The open folder: a row that is one, and the two rows that go and look
        // at one somewhere outside this window — File Explorer, or the system's
        // own picker.
        (
            "i-folder-open",
            &[
                ActionIcon::OpenFolderObject,
                ActionIcon::RevealInFolder,
                ActionIcon::BrowseForFolder,
            ],
            false,
        ),
        (
            "i-file",
            &[
                ActionIcon::OpenFile,
                ActionIcon::OpenDiff,
                ActionIcon::FileObject,
                ActionIcon::PreviewSeat,
            ],
            false,
        ),
        (
            "i-paste",
            &[ActionIcon::PasteClipboard, ActionIcon::InsertPath],
            false,
        ),
        (
            "i-pencil",
            &[
                ActionIcon::RenameFile,
                ActionIcon::RenameBranch,
                ActionIcon::EditRow,
            ],
            false,
        ),
    ];

    /// Every shape two verbs share is on [`REUSED_SHAPES`], with the whole list
    /// of verbs, and nothing on that list has stopped being shared.
    ///
    /// The gate cuts both ways on purpose. A new verb that reaches for a shape
    /// already spoken for fails here rather than shipping as a fifth meaning of
    /// the bare arrow; and a shape P1 splits stops matching its entry, so the
    /// entry has to come off the list rather than sit there as folklore.
    #[test]
    fn a_shape_two_verbs_share_is_written_down() {
        let mut by_shape: BTreeMap<&'static str, Vec<ActionIcon>> = BTreeMap::new();
        for icon in ActionIcon::ALL {
            by_shape
                .entry(icon.mark().drawing_id())
                .or_default()
                .push(icon);
        }
        for (shape, worn_by) in &by_shape {
            if worn_by.len() < 2 {
                continue;
            }
            let listed = REUSED_SHAPES
                .iter()
                .find(|(name, _, _)| name == shape)
                .unwrap_or_else(|| {
                    panic!(
                        "{shape} is worn by {} verbs and is not written down: {:?}",
                        worn_by.len(),
                        worn_by.iter().map(|icon| icon.name()).collect::<Vec<_>>()
                    )
                });
            let mut declared: Vec<&str> = listed.1.iter().map(|icon| icon.name()).collect();
            let mut actual: Vec<&str> = worn_by.iter().map(|icon| icon.name()).collect();
            declared.sort_unstable();
            actual.sort_unstable();
            assert_eq!(
                declared, actual,
                "{shape}'s list of verbs is not the list the table answers with",
            );
        }
        for (shape, worn_by, _) in REUSED_SHAPES {
            assert!(
                by_shape.get(shape).is_some_and(|actual| actual.len() > 1),
                "{shape} is written down as shared and is not shared by {} verbs",
                worn_by.len(),
            );
        }
    }

    /// **Nothing is waiting for a split.** The other half of P1's acceptance,
    /// and the reason the flag survives the block rather than being deleted
    /// with the last `true`: the day somebody finds a shape doing two jobs
    /// again, it is written down here as owing a split, and this goes red until
    /// the split lands.
    ///
    /// The seven the two audits *both* reported — `i-external`, `i-close`,
    /// `i-refresh`, `i-code`, `i-split`, `i-copy`, `i-chev` — are named
    /// explicitly, so that "no shape owes a split" cannot be reached by quietly
    /// dropping one of them off the list instead of splitting it.
    #[test]
    fn no_shape_is_still_waiting_for_a_split() {
        let owed: Vec<&str> = REUSED_SHAPES
            .iter()
            .filter(|(_, _, splits)| *splits)
            .map(|(shape, _, _)| *shape)
            .collect();
        assert!(owed.is_empty(), "still awaiting a split: {owed:?}");
        for shape in [
            "i-external",
            // The audit's `i-close`, which 裁2 re-cut and renamed: the four
            // verbs it found sharing one cross still share one, and it is the
            // house's rather than the caption's.
            "i-cross",
            "i-refresh",
            "i-split",
            "i-copy",
            "i-chev",
        ] {
            let (_, worn_by, _) = REUSED_SHAPES
                .iter()
                .find(|(name, _, _)| *name == shape)
                .unwrap_or_else(|| panic!("{shape} is one of the audits' findings"));
            assert!(
                !worn_by.is_empty(),
                "{shape} was split by emptying it rather than by splitting it",
            );
        }
        // `#i-code` is the one of the seven that came out of P1 worn by a
        // single verb, so it has no entry at all: `View source` kept it,
        // `DevTools` took the spanner, `Copy hash` took the hash and `Go to
        // parent` took the arrow.
        assert!(
            !REUSED_SHAPES.iter().any(|(name, _, _)| *name == "i-code"),
            "#i-code is worn by one verb and should not be on the shared list",
        );
        assert_eq!(ActionIcon::ViewSource.mark(), ChromeMark::Code);
    }

    /// **Every verb answers, and answers once.** The registry is a `match`, so
    /// the compiler already guarantees totality; what this adds is that the
    /// walking list `ALL` is the whole enum rather than most of it, which is the
    /// half a hand-written list gets wrong.
    #[test]
    fn the_registry_lists_every_verb_it_can_answer_for() {
        let mut names: Vec<&str> = ActionIcon::ALL.iter().map(|icon| icon.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            ActionIcon::ALL.len(),
            "a verb is listed twice in ActionIcon::ALL",
        );
    }

    /// **No codepoint stands in for a drawing** — the law `crate::marks`'
    /// opening paragraph states, checked over the source of every file that
    /// paints chrome.
    ///
    /// R4 struck this once, over the Git masthead's `⎇`, and wrote down why: a
    /// codepoint is a fact about the font the machine happens to have, its
    /// width is another, and a masthead whose first mark is a hollow box on a
    /// machine without it is worse than no mark. Two audits on 2026-08-25 then
    /// found eight survivors — the settings dialog's `↺ ↑ ↓ ⋯ ✕`, two
    /// breadcrumbs' `‹ ›`, and the preview switcher's `●` — which is what a
    /// law with no gate under it is worth.
    ///
    /// **And 裁3 (2026-08-26) gave the law its one class of exception**, which
    /// is a smaller thing than an exception: the two breadcrumbs' guillemets
    /// are back, and they are back because they were never marks. A breadcrumb
    /// is a *sentence* — `‹ Profiles › Windows PowerShell`, `project › src ›
    /// main.rs` — and the guillemet in it is the sentence's comma: it sits on
    /// the run's baseline, at the run's size, tracked by the run's tracking,
    /// inked by the run's colour, and it is measured by the same call that
    /// measures the words either side of it. Struck as a mark it became a glyph
    /// in the middle of a phrase, centred in a box the phrase's metrics know
    /// nothing about. The law says a codepoint may not stand in for **a mark**;
    /// punctuation inside a line of type is not one, and [`PUNCTUATION`] is
    /// where a character has to argue that it is that and not this.
    ///
    /// **What this reads is the escape spelling**, `\u{...}`, and that is the
    /// whole of what makes the gate precise rather than noisy. The characters
    /// themselves appear all over this crate for reasons that are none of this
    /// rule's business: `↑ ↓ ← →` are the names of four keys in
    /// `shortcuts.rs`; `›` joins a path into a sentence for a window title;
    /// `→` sits inside the translated sentence `Comparing a → b`; `×` is a
    /// multiplication sign in `800×600`; a hundred doc comments write `▸` and
    /// `✕` while explaining the drawings that replaced them. None of those is
    /// a glyph pretending to be an icon, and none of them is written as an
    /// escape. A drawn one was, every single time, because a control glyph is
    /// bound to a named constant and this house spells an invisible character
    /// in a constant by its codepoint.
    #[test]
    fn no_font_character_stands_in_for_a_mark() {
        // The retired eight, less the two 裁3 sent back, and `⎇`, which R4
        // struck first.
        const RETIRED: &[(&str, &str)] = &[
            ("21ba", "restore defaults"),
            ("2191", "move this row up"),
            ("2193", "move this row down"),
            ("22ef", "the rest of this row's verbs"),
            ("2715", "remove this row"),
            ("25cf", "unsaved edits"),
            ("2387", "a branch"),
            ("25b8", "a submenu"),
            ("2304", "a list folded away"),
        ];
        // **Punctuation inside a line of type**, which is the class 裁3
        // (2026-08-26) added and the only thing that may be spelled as a
        // character. A gate whose exceptions are written down is a scope; one
        // that quietly skips a file is a hole — so each of these names its
        // file, its codepoint, and the sentence it is punctuating.
        const PUNCTUATION: &[(&str, &str, &str)] = &[
            (
                "seats.rs",
                "203a",
                "the preview rail's breadcrumb, and the same constant joins a \
                 path's segments into a window title or a tooltip line. One \
                 phrase, one face, one comma — 裁3, 2026-08-26.",
            ),
            (
                "settings.rs",
                "203a",
                "`CRUMB_SEPARATOR`, between the editor's way back and where you \
                 are. Two breadcrumbs in one window may not be two \
                 vocabularies, so it came back with the rail's.",
            ),
            (
                "settings.rs",
                "2039",
                "`CRUMB_BACK_PREFIX` — the same character turned, measured \
                 inside the phrase `‹ Profiles` rather than beside it.",
            ),
        ];
        for (file, source) in [
            ("settings.rs", include_str!("settings.rs")),
            ("seats.rs", include_str!("seats.rs")),
            ("profiles.rs", include_str!("profiles.rs")),
            ("git_panel.rs", include_str!("git_panel.rs")),
            ("git_graph.rs", include_str!("git_graph.rs")),
            ("float.rs", include_str!("float.rs")),
            ("toast.rs", include_str!("toast.rs")),
            ("notice.rs", include_str!("notice.rs")),
            ("search.rs", include_str!("search.rs")),
            ("peek_strip.rs", include_str!("peek_strip.rs")),
            ("file_peek.rs", include_str!("file_peek.rs")),
            ("restore.rs", include_str!("restore.rs")),
        ] {
            for (codepoint, meaning) in RETIRED {
                let escape = format!("\\u{{{codepoint}}}");
                assert!(
                    !source.contains(&escape),
                    "{file} spells U+{} — {meaning} — as a character. \
                     It is a drawing: see crate::marks.",
                    codepoint.to_uppercase(),
                );
            }
            // And the punctuation class cuts the same way `REUSED_SHAPES` does:
            // a character written down as punctuation *in this file* has to
            // still be in it, so a surface that stops setting one takes its
            // entry off the list instead of leaving it as folklore.
            for (where_, codepoint, why) in PUNCTUATION {
                if *where_ != file {
                    continue;
                }
                assert!(
                    source.contains(&format!("\\u{{{codepoint}}}")),
                    "{file} is written down as punctuating with U+{} ({why}) \
                     and does not",
                    codepoint.to_uppercase(),
                );
            }
        }
    }

    /// And the class is held to its own claim: **every character on it is set
    /// as type, in a run, and none of them is centred in a box of its own.**
    ///
    /// `ChromeLabel` is how a run of type reaches the renderer and `ChromeSprite`
    /// is how a mark does, so the claim "this is punctuation and not a mark" is
    /// checkable: the two breadcrumbs push labels. This is the assertion the
    /// exception above is worth exactly as much as — without it, `PUNCTUATION`
    /// would be a place to write a sentence that excuses a glyph.
    #[test]
    fn the_punctuation_that_stayed_a_character_is_set_as_type() {
        let seats = include_str!("seats.rs");
        assert!(
            seats.contains("text: PREVIEW_CRUMB_SEPARATOR.to_owned(),"),
            "the preview rail sets its separator as type",
        );
        let settings = include_str!("settings.rs");
        assert!(
            settings.contains("(crumb.separator, CRUMB_SEPARATOR.to_owned(), true, false),"),
            "the editor's breadcrumb sets its separator as type",
        );
        assert!(
            settings.contains("format!(\"{CRUMB_BACK_PREFIX}{}\", Text::CategoryProfiles.text())"),
            "and its way back carries the guillemet inside the phrase",
        );
        // And neither surface has kept a mark to draw one with: the three
        // functions P1 cut for this are gone, so there is nothing to fall back
        // to and nothing to drift.
        for (name, source) in [("seats.rs", seats), ("settings.rs", settings)] {
            for gone in [
                "crumb_separator_mark",
                "crumb_back_mark",
                "crumb_punctuation_box",
            ] {
                assert!(
                    !source.contains(gone),
                    "{name} still reaches for {gone}, which 裁3 retired",
                );
            }
        }
    }

    // ── P2: the fill policy, and the two disclosure languages ──────────────

    /// **The drawings a verb may wear that have no pen at all**, and which of
    /// the four classes each one is.
    ///
    /// P2's policy, in one sentence: *an act's silhouette is struck, and ink is
    /// laid down solid only where a stroke would say something else.* Four
    /// places it says something else, and nothing may join the list without
    /// naming one of them:
    ///
    /// 1. **A state.** The on-face of a two-state control — `#i-pinned`,
    ///    `#i-locked` — which is Fluent's fill axis and the reason the registry
    ///    answers with the *off* face: a control asks [`ActionIcon::engaged`]
    ///    for the other one. Nothing on this list, because nothing on this list
    ///    is what a verb resolves to.
    /// 2. **A thing.** A brand chassis, a favicon, a status dot, and the object
    ///    folders — see [`ActionIcon::is_an_object`], which is where that half
    ///    is decided rather than here.
    /// 3. **A field inside a struck frame.** `#i-dock-left`'s panel, on
    ///    `marks::MarkLayer::Field`. The mark's silhouette is still the frame,
    ///    so it is not on this list either.
    /// 4. **A sign whose whole meaning is the solid.** Outlined, each of these
    ///    becomes a *different sign*: three dots become three rings, a stop
    ///    square becomes a panel, a disclosure triangle becomes a play button.
    ///    That is the argument, and it is the only argument this list accepts.
    ///
    /// Plus the one foreign drawing the 2026-08-26 ruling keeps as it is.
    const FILLED_WITH_A_REASON: &[(&str, &str)] = &[
        (
            "i-folder",
            "class 2, and a user ruling with it (裁1, 2026-08-26): *还是原来的\
             好看*. P2 struck the folder for the rows that are acts and the \
             acceptance sent the struck rendition back — a folder is an object, \
             and `Open files pane` puts one beside a name for the same reason a \
             tree row does. What the class costs is written down beside it: the \
             solid is `1.36×` its column's ink where every struck neighbour is \
             level, and that is the price of the ruling rather than an \
             oversight",
        ),
        (
            "i-folder-open",
            "class 2, 裁1 again and the same object: `Reveal in folder` and \
             `Browse…` wear the open folder solid because the row is about a \
             folder. One ruling about one object covers both of its frames",
        ),
        (
            "i-gear",
            "the one foreign drawing (Material's 24-unit fill); 裁5, 2026-08-26, \
             keeps it — and a fill has no pen to hold to the band anyway",
        ),
        (
            "i-tri",
            "class 4, and a user ruling with it (裁6, 2026-08-26): the disclosure \
             triangle stays filled in the files tree and on a submenu row. \
             Outlined at fourteen pixels a 4.6-unit triangle is three hairlines \
             meeting, and what it reads as is a play button",
        ),
        (
            "i-stop",
            "class 4 — the transport control's solid. An outlined rounded square \
             is `#i-panel`, which is a different sign standing in the same head",
        ),
        (
            "i-more",
            "class 4 — three dots `2.4` units across, which is two pens: a ring \
             drawn at the house's weight has no hole left in it",
        ),
    ];

    /// **An act is struck; a thing may be filled.** P2's fill policy, over the
    /// whole registry.
    ///
    /// RED EVIDENCE (2026-08-26, before P2): `i-folder` and `i-folder-open` —
    /// worn by `Open files pane`, `New terminal in folder…` and `Reveal in
    /// folder`, three rows of two menus — are pure fills, and P1's own实机
    /// reading of the pane menu says what that costs: every row in the column
    /// came into a `1.36×` ink band *except* the solid folder, which stayed the
    /// outlier at more than twice its neighbours' ink.
    ///
    /// **The acceptance ruled that this is a price and not a fault** (裁1,
    /// 2026-08-26 — *还是原来的好看*), so the gate no longer reads the two
    /// folders as owing a stroke; it reads them as a class-2 entry on
    /// [`FILLED_WITH_A_REASON`] with the `1.36×` written into the entry. That
    /// is the whole difference between a policy with an argued exception and a
    /// policy with a hole: the measurement is still on the page.
    ///
    /// MUTATION: take `i-folder` off [`FILLED_WITH_A_REASON`] and this goes red
    /// naming the four folder rows; point any other verb at a drawing with no
    /// pen — `ClearScreen` at `ChromeMark::Pin { filled: true }`, say — and it
    /// goes red naming that.
    #[test]
    fn an_act_is_struck_and_only_a_thing_is_filled() {
        let mut unstruck = Vec::new();
        for icon in ActionIcon::ALL {
            let mark = icon.mark();
            if mark.is_struck() || icon.is_an_object() {
                continue;
            }
            let shape = mark.drawing_id();
            if FILLED_WITH_A_REASON
                .iter()
                .any(|(named, _)| *named == shape)
            {
                continue;
            }
            unstruck.push(format!("{} wears {shape}, which has no pen", icon.name()));
        }
        assert!(
            unstruck.is_empty(),
            "an act's silhouette is struck with the house pen:\n{}",
            unstruck.join("\n"),
        );
    }

    /// And the list cuts the other way: **a drawing written down as filled with
    /// a reason has to still be filled**, so a re-cut takes its entry off the
    /// list instead of leaving it there as folklore.
    ///
    /// This is `REUSED_SHAPES`' own discipline applied to the other list. The
    /// two folders are the case in point in both directions: P2 wrote them down
    /// as *struck*, and had 裁1 not also deleted the struck renditions an entry
    /// saying so would have outlived the drawing.
    #[test]
    fn nothing_is_written_down_as_filled_that_is_not() {
        let filled: Vec<&str> = ActionIcon::ALL
            .iter()
            .map(|icon| icon.mark())
            .chain([marks::tree_disclosure(0.0)])
            .filter(|mark| !mark.is_struck())
            .map(marks::ChromeMark::drawing_id)
            .collect();
        for (shape, why) in FILLED_WITH_A_REASON {
            assert!(
                filled.contains(shape),
                "{shape} is written down as filled ({why}) and is struck",
            );
        }
        // And the object has one rendition again: 裁1 took the struck pair off
        // the sheet, so there is no second folder for a drawing point to reach
        // for. That is the compiler's fact now rather than a convention — the
        // two variants are gone from `marks::ChromeMark` — and what is left to
        // check here is that the one that survived is the solid.
        assert!(!marks::ChromeMark::Folder.is_struck());
        assert!(!marks::ChromeMark::FolderOpen.is_struck());
    }

    /// **No body carries a bare alpha** — the 2026-08-25 specification's last
    /// line about colour (*不要在 SVG body 内写任意 opacity*), as a gate.
    ///
    /// Four drawings did: `#i-folder-open`'s back plate and the float grip at
    /// `.55`, `#i-dock-left` and `#i-dock-right`'s panel at `.7`, each with a
    /// comment recording where the number came from — which is a provenance and
    /// not a meaning. They are two things and they now have two names; what a
    /// body writes is `marks::MarkLayer`'s token, and the alpha is substituted
    /// on the way to the rasterizer beside `currentColor`.
    ///
    /// Both halves are checked, because either alone leaves the hole open: that
    /// no source in the crate spells an opacity that is not a layer, and that
    /// no token survives into a rendered document (a substitution that stopped
    /// happening would otherwise pass silently, drawing every layer at full
    /// strength).
    #[test]
    fn no_drawing_carries_a_bare_alpha() {
        const ATTRIBUTE: &str = "opacity=\"";
        let source = include_str!("marks.rs");
        let mut bare = Vec::new();
        let mut rest = source;
        while let Some(at) = rest.find(ATTRIBUTE) {
            rest = &rest[at + ATTRIBUTE.len()..];
            let end = rest.find('"').expect("an attribute closes its quote");
            let value = &rest[..end];
            if !marks::MarkLayer::ALL
                .iter()
                .any(|layer| layer.token() == value)
            {
                bare.push(value.to_owned());
            }
            rest = &rest[end..];
        }
        assert!(
            bare.is_empty(),
            "these alphas are written into a drawing instead of being a named \
             layer (see marks::MarkLayer): {bare:?}",
        );
        for layer in marks::MarkLayer::ALL {
            assert!(
                source.contains(layer.token()),
                "{:?} is a layer nothing rides, so the name is folklore",
                layer,
            );
            assert!(
                !layer.alpha().is_empty() && layer.alpha() != layer.token(),
                "a layer's token is not its alpha",
            );
        }
    }

    /// **The chrome has two disclosure languages, and this is the whole of
    /// them** (user ruling 2026-08-26, 裁6).
    ///
    /// The 2026-08-25 audit found three in one window: a filled triangle in the
    /// files tree and on a submenu row, a chevron on the pane head and the
    /// profile picker, and the characters `‹ ›` in two breadcrumbs. P1 retired
    /// the characters and moved the settings dialog's `Advanced` onto the
    /// chevron; what P2 adds is that the surviving two are **rows of the
    /// registry** rather than glyph names written into three files, so a fourth
    /// cannot appear without appearing here.
    ///
    /// The triangle's two uses are one drawing at two orientations, which is the
    /// claim the ruling rests on — the reader who opens a folder learns what a
    /// turned triangle means, and the submenu row is the same sentence pointing
    /// the other way.
    #[test]
    fn the_chrome_folds_things_away_in_two_languages_and_not_three() {
        let languages: BTreeMap<&str, Vec<&str>> = [
            ActionIcon::PickProfile,
            ActionIcon::OpenPaneMenu,
            ActionIcon::ExpandAdvanced,
            ActionIcon::OpenSubmenu,
            ActionIcon::FoldFolder,
        ]
        .into_iter()
        .fold(BTreeMap::new(), |mut by_shape, icon| {
            by_shape
                .entry(icon.mark().drawing_id())
                .or_default()
                .push(icon.name());
            by_shape
        });
        assert_eq!(
            languages.keys().copied().collect::<Vec<_>>(),
            vec!["i-chev", "i-tri"],
            "two drawings say *there is more here, folded away*: {languages:?}",
        );
        // One triangle, two orientations — and the tree's open frame really is
        // the other end of the submenu row's turn rather than a second glyph.
        let shut = ActionIcon::OpenSubmenu.mark();
        let open = ActionIcon::FoldFolder.turned(1.0);
        assert_eq!(shut.drawing_id(), open.drawing_id());
        assert_eq!(shut, marks::tree_disclosure(0.0));
        assert_eq!(
            open,
            ChromeMark::TreeDisclosure {
                turned_degrees: marks::TREE_DISCLOSURE_OPEN_DEGREES,
            },
        );
        assert_ne!(shut, open, "the two ends of the turn are two frames");
        // And the chevron's three turn rather than swapping: the same drawing
        // at rest and fully over.
        assert_eq!(
            ActionIcon::ExpandAdvanced.turned(1.0).drawing_id(),
            ActionIcon::ExpandAdvanced.mark().drawing_id(),
        );
    }

    /// The two draw sites the ruling names **ask the registry**, so "the
    /// triangle lives in the tree and on a submenu row and nowhere else" is
    /// checkable rather than merely written down.
    ///
    /// Read off the sources that paint chrome, on
    /// `no_font_character_stands_in_for_a_mark`'s precedent: a surface that
    /// constructs the glyph itself is a surface the table above cannot see, and
    /// three of them are exactly how the build came to have three disclosure
    /// languages in the first place.
    #[test]
    fn no_surface_reaches_past_the_registry_for_a_disclosure() {
        for (file, source) in [
            ("seats.rs", include_str!("seats.rs")),
            ("settings.rs", include_str!("settings.rs")),
            ("git_panel.rs", include_str!("git_panel.rs")),
            ("git_graph.rs", include_str!("git_graph.rs")),
            ("float.rs", include_str!("float.rs")),
            ("search.rs", include_str!("search.rs")),
        ] {
            assert!(
                !source.contains("TreeDisclosure { turned_degrees:"),
                "{file} strikes the disclosure triangle itself; it is \
                 ActionIcon::OpenSubmenu or ::FoldFolder",
            );
        }
        // `profiles.rs` draws both submenu indicators and holds the file menu's
        // `Expand`/`Collapse` row, so it is the file the rule is most about.
        let profiles = include_str!("profiles.rs");
        assert!(
            !profiles.contains("TreeDisclosure { turned_degrees:"),
            "the two submenu indicators ask the registry",
        );
        assert!(
            profiles.contains("ActionIcon::OpenSubmenu.mark()"),
            "and they ask it by name",
        );
    }

    /// The slot table says what the plan says.
    #[test]
    fn the_slots_are_the_plans_four_numbers() {
        assert!((MarkSlot::Menu.house_box_logical_px() - 14.0).abs() < f32::EPSILON);
        assert!((MarkSlot::Toolbar.house_box_logical_px() - 14.0).abs() < f32::EPSILON);
        // `13` and not the plan's `12` — the 2026-08-26 ruling on the finding
        // P0 filed against its own exemption note. See the constant.
        assert!((MarkSlot::CompactHead.house_box_logical_px() - 13.0).abs() < f32::EPSILON);
        let [floor, _] = OPTICAL_STROKE_BAND_LOGICAL_PX;
        for slot in MarkSlot::ALL {
            let pen = ChromeMark::File
                .optical_stroke_logical_px(
                    slot.mark_box_logical_px(ChromeMark::File)[0],
                    slot.mark_box_logical_px(ChromeMark::File)[1],
                )
                .expect("the house pen");
            if slot == MarkSlot::Caption {
                // The caption's box is the *edge-to-edge* family's ten, and a
                // house mark has no business there: its box is derived the
                // other way round, which is what the slot's own note says.
                continue;
            }
            assert!(
                pen >= floor,
                "{}: a house mark draws {pen:.3}, under the band's floor",
                slot.name(),
            );
        }
        assert!((MarkSlot::Caption.edge_to_edge_box_logical_px() - 10.0).abs() < f32::EPSILON);
    }

    /// The window caption is where the edge-to-edge family's box is *written*
    /// rather than derived, so the derivation must not move it: routing the
    /// caption through the slot table draws exactly what it drew before.
    #[test]
    fn the_caption_draws_what_it_always_drew() {
        for mark in [
            ChromeMark::WindowMinimize,
            ChromeMark::WindowMaximize,
            ChromeMark::WindowClose,
        ] {
            assert_eq!(
                MarkSlot::Caption.mark_box_logical_px(mark),
                [
                    bt_render::WINDOW_CAPTION_GLYPH_LOGICAL_PX,
                    bt_render::WINDOW_CAPTION_GLYPH_LOGICAL_PX
                ],
            );
        }
        // And 裁2 left it there: the caption's cross is still the platform's
        // ten-unit drawing at the platform's box, beside a bar and a square cut
        // to match it. What moved is every *other* cross in the window — see
        // `marks::ChromeMark::PaneClose`, and `the_house_and_the_platform_draw_
        // two_crosses` for the line between them.
        assert!(ChromeMark::WindowClose.draws_edge_to_edge());
        assert_eq!(ChromeMark::WindowClose.drawing_id(), "i-close");
    }

    /// **Two crosses, and which surface gets which** (裁2, 2026-08-26).
    ///
    /// The caption's three are Windows's furniture at Windows's size and this
    /// house does not re-cut them; everything else that closes something is the
    /// house's own control on the house's own grid. The rule is worth stating
    /// as an assertion because the two are one act — a reader who sees them as
    /// one act is right — and the only thing keeping them two drawings is that
    /// they stand in two different companies.
    #[test]
    fn the_house_and_the_platform_draw_two_crosses() {
        assert_eq!(
            ActionIcon::CloseWindow.mark().drawing_id(),
            "i-close",
            "the caption's cross is the platform's",
        );
        for icon in [
            ActionIcon::CloseTab,
            ActionIcon::ClosePane,
            ActionIcon::CloseDialog,
            ActionIcon::LeaveCompare,
        ] {
            assert_eq!(
                icon.mark().drawing_id(),
                "i-cross",
                "{} closes one of this house's own surfaces",
                icon.name(),
            );
        }
        // And the house's cross is a house drawing all through: the house's
        // grid, the house's pen, the house's air.
        assert_eq!(
            ChromeMark::PaneClose.view_box_units(),
            Some([marks::HOUSE_GRID_UNITS, marks::HOUSE_GRID_UNITS]),
        );
        assert_eq!(ChromeMark::PaneClose.design_stroke_units(), Some(1.2));
        assert!(!ChromeMark::PaneClose.draws_edge_to_edge());
        // Two cache slots, one drawing — which is what the two variant names
        // were kept for and what they still are. (That the two *cache* names
        // stay apart is `marks`' own business and is checked there.)
        assert_eq!(
            ChromeMark::TabClose.drawing_id(),
            ChromeMark::PaneClose.drawing_id(),
        );
        assert_ne!(
            ChromeMark::TabClose.drawing_id(),
            ChromeMark::WindowClose.drawing_id(),
        );
    }

    /// **No mark is squeezed into somebody else's proportion**, which is the
    /// half of the pane head's 1.95× the pen does not explain.
    ///
    /// It used to be a statement about the chevron alone — that a `10 × 6`
    /// arrow had to be given a `10 × 6` box or `xMidYMid meet` would scale it
    /// by whichever of the two ratios happened to be smaller. P1 deleted the
    /// premise: the arrow is a house square, so it is fitted like everything
    /// else. What the rule becomes is the general one it always was — a slot
    /// hands out the mark's own aspect, whatever that is.
    #[test]
    fn no_mark_is_squeezed_into_a_box_of_another_shape() {
        for slot in MarkSlot::ALL {
            for icon in ActionIcon::ALL {
                let mark = icon.mark();
                let Some([view_width, view_height]) = mark.view_box_units() else {
                    continue;
                };
                let [width, height] = slot.mark_box_logical_px(mark);
                assert!(
                    (width * view_height - height * view_width).abs() < 0.01,
                    "{}: {} is drawn {width}×{height} out of a {view_width}×{view_height} box",
                    slot.name(),
                    mark.drawing_id(),
                );
            }
        }
        // And the arrow itself: the box it is cut in is square, so a run of
        // three on a head can give it the same box as its neighbours.
        assert_eq!(
            ChromeMark::chevron(0.0).view_box_units(),
            Some([marks::HOUSE_GRID_UNITS, marks::HOUSE_GRID_UNITS]),
        );
    }

    /// The pens the marks answer with are the pens their bodies write.
    #[test]
    fn a_marks_pen_is_read_off_its_own_body() {
        // **One pen, and P1 is where it became one.** Every drawing on this
        // sheet that the theme inks now writes `1.2`: `#i-file` came up from
        // `1.15`, `#i-check` down from `1.6`, `#i-paste` from a `1.3` frame
        // around a `1.1` sheet, the grip from `1.5` in an eight-unit box.
        for mark in [
            ChromeMark::File,
            ChromeMark::Chevron { turned_degrees: 0 },
            ChromeMark::Check,
            ChromeMark::Paste,
            ChromeMark::Code,
            ChromeMark::Copy,
            ChromeMark::Split,
            ChromeMark::Panel,
            ChromeMark::Eye,
            ChromeMark::Tag,
            ChromeMark::GitBranch,
            ChromeMark::GitGraph,
            ChromeMark::Globe { favicon: None },
            ChromeMark::Plus,
            ChromeMark::Minus,
            ChromeMark::ResizeGrip,
            ChromeMark::Lock { engaged: false },
            ChromeMark::PaneZoom { zoomed: false },
            ChromeMark::Pin { filled: false },
            ChromeMark::Trash,
            ChromeMark::Search,
            ChromeMark::Arrow { turned_degrees: 0 },
            ChromeMark::Hash,
            ChromeMark::Compare,
            ChromeMark::Duplicate,
            ChromeMark::DevTools,
            ChromeMark::Restart,
            ChromeMark::MoreDown,
            ChromeMark::HistoryRestore,
            ChromeMark::TabNew,
            ChromeMark::WindowNew,
            ChromeMark::WindowPick,
            // 裁2's re-cut: the house's cross came from the platform's `1.0` in
            // a ten to the house's `1.2` in a sixteen, which is what pays for
            // its smaller ink.
            ChromeMark::TabClose,
            ChromeMark::PaneClose,
        ] {
            assert_eq!(
                mark.design_stroke_units(),
                Some(1.2),
                "{} is not struck with the house pen",
                mark.drawing_id(),
            );
        }
        // The caption family keeps the platform's hairline in the platform's
        // own ten-unit box, which the slot table's own note is about.
        assert_eq!(ChromeMark::WindowClose.design_stroke_units(), Some(1.0));
        // Pure fills and brand marks have no pen a slot could hold them to.
        assert_eq!(ChromeMark::Folder.design_stroke_units(), None);
        assert_eq!(ChromeMark::Gear.design_stroke_units(), None);
        assert_eq!(ChromeMark::ProfilePowerShell.design_stroke_units(), None);
        assert_eq!(ChromeMark::Fill.design_stroke_units(), None);
        // And the line profiles answer with the pen their ruling cut them to.
        assert_eq!(
            ChromeMark::ProfileLine(marks::ProfileGlyph::Console).design_stroke_units(),
            Some(1.2),
        );
    }

    /// The boxes the marks answer with are the boxes the table writes.
    #[test]
    fn a_marks_box_is_read_off_the_view_box_table() {
        assert_eq!(ChromeMark::File.view_box_units(), Some([16.0, 16.0]));
        // **The two boxes P1 closed.** The arrow was `10 × 6` and the grip an
        // eight, and both were the mock-up quoting a *placement* as though it
        // were a grid.
        assert_eq!(
            ChromeMark::Chevron { turned_degrees: 0 }.view_box_units(),
            Some([16.0, 16.0]),
        );
        assert_eq!(ChromeMark::ResizeGrip.view_box_units(), Some([16.0, 16.0]));
        // What is left off the house's sixteen, and why, is written at
        // `SYMBOL_VIEW_BOX`: the caption family's ten, `#i-tri`'s ten, the
        // gear's twenty-four and the merge curve's row.
        assert_eq!(ChromeMark::WindowClose.view_box_units(), Some([10.0, 10.0]));
        assert_eq!(
            marks::tree_disclosure(0.0).view_box_units(),
            Some([10.0, 10.0]),
        );
        assert_eq!(
            ChromeMark::GitMergeCurve.view_box_units(),
            Some([14.0, 27.0])
        );
        assert_eq!(ChromeMark::Fill.view_box_units(), None);
    }

    /// The edge-to-edge family is the drawings whose ink runs to their own edge,
    /// and `#i-tri` — cut in a ten-unit box like the caption family — is not one
    /// of them.
    #[test]
    fn the_edge_to_edge_family_is_the_drawings_that_have_no_margin() {
        for mark in [
            ChromeMark::WindowClose,
            ChromeMark::WindowMinimize,
            ChromeMark::WindowMaximize,
        ] {
            assert!(mark.draws_edge_to_edge(), "{mark:?} draws edge to edge");
        }
        for mark in [
            ChromeMark::File,
            ChromeMark::Folder,
            ChromeMark::Copy,
            marks::tree_disclosure(0.0),
            // **The four P1 took off the list**, by re-cutting them into the
            // house's grid rather than by arguing about them.
            ChromeMark::Plus,
            ChromeMark::Minus,
            ChromeMark::chevron(0.0),
            ChromeMark::ResizeGrip,
            // **And the two 裁2 took off it**, the same way and for the same
            // reason one rule up: a cross with its ink on its own wall is a
            // cross whose picture no slot can level.
            ChromeMark::TabClose,
            ChromeMark::PaneClose,
        ] {
            assert!(
                !mark.draws_edge_to_edge(),
                "{mark:?} carries the house's margin",
            );
        }
    }
}
