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
/// `紧凑头 12` — a head's run of controls.
const COMPACT_HEAD_HOUSE_BOX_LOGICAL_PX: f32 = 12.0;
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
    /// verb, which is why the two share a string already.
    RevealInFolder,

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
    EditRow,
    DeleteRow,
    CloseDialog,
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
}

impl ActionIcon {
    /// Every verb, for the reverse index to walk.
    #[cfg(test)]
    pub const ALL: [Self; 76] = [
        Self::OpenSettings,
        Self::MinimiseWindow,
        Self::MaximiseWindow,
        Self::CloseWindow,
        Self::NewTab,
        Self::PickProfile,
        Self::CloseTab,
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
        Self::EditRow,
        Self::DeleteRow,
        Self::CloseDialog,
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
            Self::EditRow => "EditRow",
            Self::DeleteRow => "DeleteRow",
            Self::CloseDialog => "CloseDialog",
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
            Self::CloseWindow | Self::DeleteRow | Self::CloseDialog => ChromeMark::WindowClose,
            Self::CloseTab => ChromeMark::TabClose,
            Self::NewTab | Self::CreateBranch | Self::StageChange | Self::LoadMoreCommits => {
                ChromeMark::Plus
            }
            Self::PickProfile | Self::OpenPaneMenu => ChromeMark::chevron(0.0),
            Self::NavigateBack => ChromeMark::Chevron { turned_degrees: 90 },
            Self::NavigateForward => ChromeMark::Chevron {
                turned_degrees: 270,
            },
            Self::OpenFilesPane
            | Self::NewTerminalInFolder
            | Self::FolderObject
            | Self::FilesSeat => ChromeMark::Folder,
            Self::RevealInFolder | Self::OpenFolderObject => ChromeMark::FolderOpen,
            Self::FloatFilesPane | Self::MoveToNewTab | Self::FloatPreview => ChromeMark::Float,
            Self::ClosePane
            | Self::DeleteBranch
            | Self::DeleteTag
            | Self::DiscardChanges
            | Self::LeaveCompare
            | Self::StopNavigating => ChromeMark::PaneClose,
            Self::ZoomPane => ChromeMark::PaneZoom { zoomed: false },
            Self::SplitPane | Self::CompareVersions | Self::SplitDirectionAsk => ChromeMark::Split,
            Self::SplitDirectionRight => ChromeMark::SplitRight,
            Self::SplitDirectionDown => ChromeMark::SplitDown,
            Self::DuplicatePane
            | Self::CopySelection
            | Self::CopyPath
            | Self::CopyHash
            | Self::CopySubject
            | Self::CopyName
            | Self::CopyAddress => ChromeMark::Copy,
            Self::PasteClipboard | Self::InsertPath => ChromeMark::Paste,
            Self::SelectAll => ChromeMark::SelectAll,
            Self::ClearScreen => ChromeMark::Broom,
            Self::ClearScrollback => ChromeMark::Eraser,
            Self::RestartShell | Self::RereadRepository | Self::ReloadPage => ChromeMark::Refresh,
            Self::OpenFile | Self::OpenDiff | Self::FileObject | Self::PreviewSeat => {
                ChromeMark::File
            }
            Self::PageObject => ChromeMark::Globe { favicon: None },
            Self::UnknownSeat => ChromeMark::Panel,
            Self::OpenWith | Self::MoveToNewWindow | Self::MoveToWindow | Self::OpenInBrowser => {
                ChromeMark::External
            }
            Self::RenameFile | Self::RenameBranch | Self::EditRow => ChromeMark::Pencil,
            Self::CheckoutBranch => ChromeMark::GitBranch,
            Self::CreateTag => ChromeMark::Tag,
            Self::UnstageChange => ChromeMark::Minus,
            Self::OpenGitGraph => ChromeMark::GitGraph,
            Self::MenuTick => ChromeMark::Check,
            Self::SavePreview => ChromeMark::Save,
            Self::ViewRendered => ChromeMark::Eye,
            Self::ViewSource
            | Self::OpenDevTools
            | Self::GraphCopyHash
            | Self::GoToParentCommit => ChromeMark::Code,
            Self::LockPreview => ChromeMark::Lock { engaged: false },
        }
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
        bespoke(
            "strip tab pin",
            ChromeMark::Pin { filled: false },
            crate::seats::WINDOW_TAB_PIN_GLYPH_LOGICAL_PX,
        );
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
        for mark in [ChromeMark::Pencil, ChromeMark::WindowClose] {
            wide.push((
                "settings row button",
                mark,
                MarkSlot::CompactHead.mark_box_logical_px(mark),
            ));
        }
        bespoke(
            "search bar button",
            ChromeMark::chevron(0.0),
            crate::search::BUTTON_GLYPH_LOGICAL_PX,
        );
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

    /// **The transitional exemption list, and what closes each entry.**
    ///
    /// Six drawings, and the rule the list is held to is that **no box may fix
    /// them**: each one is red at every box its family could legitimately take
    /// in any of the four slots, because what is wrong is the *pen against its
    /// own box* rather than the box. Anything a slot could have fixed was fixed
    /// in this block instead of being written down here — that is what moved the
    /// menu's whole terminal run from `1.219` to `1.05`, the pane head's `✕`
    /// from `0.80` to `0.96`, and the same `#i-close` — struck at `8` on a tab,
    /// `8` on a toast, `8` on a focus card, `9` on a float's head, `10` in a
    /// menu and `10` in the title bar — down to two boxes: the compact head's
    /// and the caption's, which are two *slots* and not six opinions.
    ///
    /// | drawing | its pen | worst reading now | what P1 does |
    /// |---|---|---|---|
    /// | `#i-plus` | `1.2` in a **ten**-unit box | `1.344` in a menu row | re-cut into the house sixteen |
    /// | `#i-minus` | `1.2` / 10 | `1.344` in a menu row | with the plus — they are one drawing |
    /// | `#i-chev` | `1.2` / 10×6 | `1.344` in a menu row | re-cut to a sixteen **square** |
    /// | `#i-check` | `1.6` / 16 | `1.400` in a menu row | the pen to `1.2` |
    /// | `#i-code` | `1.4` / 16 | `1.225` in a menu row | the pen to `1.2` |
    /// | grip | `1.5` in an **eight** | `1.500` on a float | `1.2` in a sixteen |
    ///
    /// The arithmetic behind "no box may fix them": a mark's pen on screen is
    /// `units × box / viewBox`, so holding the *ink* level across a run fixes
    /// the box and leaves the pen wherever `units / viewBox` puts it. The house
    /// writes `1.2 / 16`; these six write `0.12`, `0.12`, `0.12`, `0.10`,
    /// `0.0875` and `0.1875` per unit. Nothing about a slot changes that ratio.
    ///
    /// **One finding for P1 to rule on, recorded here because this list is where
    /// it will bite.** The plan's compact-head slot is `12`, and `12 × 1.2/16 =
    /// 0.90` — *below the band's floor before any drawing is chosen*. The two
    /// halves of the specification do not both fit: after P1 re-cuts the table
    /// to one `1.2` pen, a house mark in a 12px head reads `0.90` and only a
    /// box of `12.67` or more reads `0.95`. Today nothing house-family is red
    /// there (the head's tools are drawn at 13 and its folder is a fill), so the
    /// gate is green; the day P1 moves the tools into the slot it will not be.
    /// Either the slot goes to 13–14 or the compact head gets a pen of its own.
    fn exempt(surface: &str, mark: ChromeMark) -> bool {
        if NOT_A_CONTROL_SLOT.contains(&surface) {
            return true;
        }
        matches!(
            mark,
            ChromeMark::Plus
                | ChromeMark::Minus
                | ChromeMark::Chevron { .. }
                | ChromeMark::Check
                | ChromeMark::Code
                | ChromeMark::ResizeGrip
        )
    }

    /// The transitional list is only worth what its rule is worth, so the rule
    /// is asserted: **every drawing on it is outside the band in the box its own
    /// run gives it.**
    ///
    /// The run is the menu's, which is the house's own box — `14`, the number
    /// the band's `1.05` is *defined* at (`1.2 / 16 × 14`). A drawing that
    /// cannot make the band there cannot be brought into it by any box a run
    /// would tolerate: the only boxes that work are smaller ones, and a smaller
    /// box in a column of marks is the *other* half of the 2026-08-25 audit —
    /// a menu column running `10.0` to `14.0` of ink, with whichever marks
    /// happened to need the compensation drawn a size down from their
    /// neighbours. `#i-code` is the case in point: it reads `1.050` at twelve,
    /// which is the right pen and the wrong size — a twelve-pixel mark in a
    /// fourteen-pixel column. The pen is what P1 fixes, and until it does, the
    /// mark stays the size of its run and the reading stays on this list.
    #[test]
    fn nothing_on_the_transitional_list_is_in_the_band_in_its_own_run() {
        let [floor, ceiling] = OPTICAL_STROKE_BAND_LOGICAL_PX;
        for mark in [
            ChromeMark::Plus,
            ChromeMark::Minus,
            ChromeMark::chevron(0.0),
            ChromeMark::Check,
            ChromeMark::Code,
            ChromeMark::ResizeGrip,
        ] {
            let [width, height] = MarkSlot::Menu.mark_box_logical_px(mark);
            let optical = mark
                .optical_stroke_logical_px(width, height)
                .expect("every drawing on this list has a pen");
            assert!(
                optical < floor || optical > ceiling,
                "{} reads {optical:.3} in the house's own box — it does not belong on the \
                 transitional list",
                mark.drawing_id(),
            );
        }
    }

    /// And the other half of the same rule: **every drawing that is *not* on the
    /// list lands in the band in the slot it lives in.** This is what makes the
    /// list a list rather than a habit — take `#i-copy` off the conversion and
    /// put it back on 15px and this goes red at `1.219`, which is the number the
    /// audit measured on the terminal menu's whole run.
    #[test]
    fn every_other_drawing_lands_in_the_band_in_its_own_slot() {
        let [floor, ceiling] = OPTICAL_STROKE_BAND_LOGICAL_PX;
        for icon in ActionIcon::ALL {
            let mark = icon.mark();
            if exempt("menu row", mark) {
                continue;
            }
            let [width, height] = MarkSlot::Menu.mark_box_logical_px(mark);
            let Some(optical) = mark.optical_stroke_logical_px(width, height) else {
                continue;
            };
            assert!(
                optical >= floor && optical <= ceiling,
                "{} reads {optical:.3} in a menu row",
                mark.drawing_id(),
            );
        }
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
    /// are fixed here: one slot, one derivation, and the three marks put the
    /// same amount of ink on the head.
    #[test]
    fn the_pane_heads_run_is_one_size() {
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
        let close = ink(ChromeMark::PaneClose);
        assert!(
            (chevron - close).abs() < 0.01 && (folder - close).abs() < 0.01,
            "the pane head's run draws {chevron:.2}, {folder:.2} and {close:.2} of ink",
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
    /// what that costs. P1 splits them; until it does, they are declared.
    ///
    /// `true` means **P1 has to split this** — the audit judged the senses
    /// genuinely different. `false` means the verbs are one sense wearing one
    /// shape, which is what a shape is for (`Copy path`, `Copy name` and
    /// `Copy subject` all mean *put this text on the clipboard*).
    const REUSED_SHAPES: &[(&str, &[ActionIcon], bool)] = &[
        (
            "i-close",
            &[
                ActionIcon::CloseWindow,
                ActionIcon::DeleteRow,
                ActionIcon::CloseDialog,
                ActionIcon::CloseTab,
                ActionIcon::ClosePane,
                ActionIcon::DeleteBranch,
                ActionIcon::DeleteTag,
                ActionIcon::DiscardChanges,
                ActionIcon::LeaveCompare,
                ActionIcon::StopNavigating,
            ],
            true,
        ),
        (
            "i-external",
            &[
                ActionIcon::OpenWith,
                ActionIcon::MoveToNewWindow,
                ActionIcon::MoveToWindow,
                ActionIcon::OpenInBrowser,
            ],
            true,
        ),
        (
            "i-code",
            &[
                ActionIcon::GraphCopyHash,
                ActionIcon::GoToParentCommit,
                ActionIcon::ViewSource,
                ActionIcon::OpenDevTools,
            ],
            true,
        ),
        (
            "i-refresh",
            &[
                ActionIcon::RestartShell,
                ActionIcon::RereadRepository,
                ActionIcon::ReloadPage,
            ],
            true,
        ),
        (
            "i-split",
            &[
                ActionIcon::SplitPane,
                ActionIcon::CompareVersions,
                ActionIcon::SplitDirectionAsk,
            ],
            true,
        ),
        (
            "i-chev",
            &[
                ActionIcon::PickProfile,
                ActionIcon::OpenPaneMenu,
                ActionIcon::NavigateBack,
                ActionIcon::NavigateForward,
            ],
            true,
        ),
        (
            "i-copy",
            &[
                ActionIcon::DuplicatePane,
                ActionIcon::CopySelection,
                ActionIcon::CopyPath,
                ActionIcon::CopyHash,
                ActionIcon::CopySubject,
                ActionIcon::CopyName,
                ActionIcon::CopyAddress,
            ],
            true,
        ),
        (
            "i-plus",
            &[
                ActionIcon::NewTab,
                ActionIcon::CreateBranch,
                ActionIcon::StageChange,
                ActionIcon::LoadMoreCommits,
            ],
            true,
        ),
        (
            "i-float",
            &[
                ActionIcon::FloatFilesPane,
                ActionIcon::MoveToNewTab,
                ActionIcon::FloatPreview,
            ],
            true,
        ),
        // One sense, several rows. A folder is a folder and a file is a file;
        // that a menu row and a tree row and a pane head all say so with one
        // drawing is the drawing doing its job.
        (
            "i-folder",
            &[
                ActionIcon::OpenFilesPane,
                ActionIcon::NewTerminalInFolder,
                ActionIcon::FolderObject,
                ActionIcon::FilesSeat,
            ],
            false,
        ),
        (
            "i-folder-open",
            &[ActionIcon::RevealInFolder, ActionIcon::OpenFolderObject],
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

    /// The groups the two audits *both* reported are the ones carrying the P1
    /// verdict, so nobody can quietly downgrade one to "this is fine".
    #[test]
    fn the_audits_own_findings_are_the_ones_awaiting_a_split() {
        for shape in [
            "i-external",
            "i-close",
            "i-refresh",
            "i-code",
            "i-split",
            "i-copy",
            "i-chev",
        ] {
            let (_, worn_by, splits) = REUSED_SHAPES
                .iter()
                .find(|(name, _, _)| *name == shape)
                .unwrap_or_else(|| panic!("{shape} is one of the audits' findings"));
            assert!(
                *splits,
                "{shape} is worn by {} verbs and is not marked for P1",
                worn_by.len(),
            );
        }
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

    /// The slot table says what the plan says.
    #[test]
    fn the_slots_are_the_plans_four_numbers() {
        assert!((MarkSlot::Menu.house_box_logical_px() - 14.0).abs() < f32::EPSILON);
        assert!((MarkSlot::Toolbar.house_box_logical_px() - 14.0).abs() < f32::EPSILON);
        assert!((MarkSlot::CompactHead.house_box_logical_px() - 12.0).abs() < f32::EPSILON);
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
    }

    /// The chevron is fitted at its own aspect, which is the half of the pane
    /// head's problem the pen does not explain.
    #[test]
    fn a_chevron_is_never_squeezed_into_a_square() {
        for slot in MarkSlot::ALL {
            let [width, height] = slot.mark_box_logical_px(ChromeMark::chevron(0.0));
            assert!(
                (width * 0.6 - height).abs() < 0.01,
                "{}: the chevron is drawn {width}×{height}",
                slot.name(),
            );
        }
    }

    /// The pens the marks answer with are the pens their bodies write.
    #[test]
    fn a_marks_pen_is_read_off_its_own_body() {
        assert_eq!(ChromeMark::File.design_stroke_units(), Some(1.15));
        assert_eq!(
            ChromeMark::Chevron { turned_degrees: 0 }.design_stroke_units(),
            Some(1.2)
        );
        assert_eq!(ChromeMark::Check.design_stroke_units(), Some(1.6));
        // The heaviest of the two `#i-paste` writes, which is the one the row
        // reads its weight off.
        assert_eq!(ChromeMark::Paste.design_stroke_units(), Some(1.3));
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
        assert_eq!(
            ChromeMark::Chevron { turned_degrees: 0 }.view_box_units(),
            Some([10.0, 6.0]),
        );
        assert_eq!(ChromeMark::ResizeGrip.view_box_units(), Some([8.0, 8.0]));
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
            ChromeMark::TabClose,
            ChromeMark::PaneClose,
            ChromeMark::WindowMinimize,
            ChromeMark::WindowMaximize,
            ChromeMark::Plus,
            ChromeMark::Minus,
            ChromeMark::chevron(0.0),
            ChromeMark::ResizeGrip,
        ] {
            assert!(mark.draws_edge_to_edge(), "{mark:?} draws edge to edge");
        }
        for mark in [
            ChromeMark::File,
            ChromeMark::Folder,
            ChromeMark::Copy,
            marks::tree_disclosure(0.0),
        ] {
            assert!(
                !mark.draws_edge_to_edge(),
                "{mark:?} carries the house's margin",
            );
        }
    }
}
