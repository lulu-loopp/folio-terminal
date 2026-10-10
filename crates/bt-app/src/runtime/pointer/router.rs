//! **The router** (`docs/plans/design/pointer-capture-2026-10-09.md` §2.1):
//! one list of layers, top first, walked once per pointer event over the
//! frame's [`PointerFacts`].
//!
//! The list is the overlay's paint list read top first
//! ([`crate::OverlayStack::bands_bottom_first`]), less the bands that take no
//! pointer, with the window's own planes under it. The first layer that claims
//! a point is the whole answer.
//!
//! **What is walked here and what is asked of the window.** The overlay's
//! layers (one to twelve) are answered here, over the facts the frame measured
//! and the layouts the window keeps as it last drew them — a pure walk that
//! shapes nothing and allocates nothing. The four planes beneath the overlay
//! (docked chrome, pane furniture, the hosted page, the pane body) are asked of
//! the window through the walk's `plane` argument, by the hit tests those
//! planes have today; they move onto the facts with cut 9 (R-3), which rewrites
//! every one of their rungs to claim what the frame drew.

#[cfg(test)]
use crate::OverlayBand;
use crate::{
    IN_PANE_SURFACES_TOP_FIRST, InPaneSurface, LeafId, NoticeHost, NoticeStrip, PreviewSurface,
    file_peek, float, git_graph, git_panel, notice, palette, profiles, search, seats, seed, toast,
    websheet,
};
use bt_layout::SeatId;
use std::collections::BTreeMap;
use winit::dpi::PhysicalPosition;

/// **One layer the pointer can be over** — a band of the overlay, or one of the
/// window's planes under it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PointerLayer {
    /// The glance card.
    GlanceCard,
    /// Every toast card.
    Toasts,
    /// The command palette: its list, its field and its padding.
    Palette,
    TabMenu,
    TermMenu,
    GitMenu,
    PaneMenu,
    FileMenu,
    /// The whole window while a full-window card or the settings sheet is up;
    /// otherwise the frames of the four menus staged in the same band (the
    /// profile picker, a files root's menu, the graph's filter, the preview
    /// switcher).
    Modal,
    /// Every floating window, front first, each over its whole risen frame.
    Floats,
    /// A page's download sheet: its scrim and its card.
    DownloadSheet,
    /// The surfaces inside a pane, [`IN_PANE_SURFACES_TOP_FIRST`].
    InPane,
    /// Below the overlay: the tab list, the rail, the heads and their
    /// controls, the dividers, the files rows, a Git page, the caption buttons
    /// and the title-bar handle.
    DockedChrome,
    /// The pane's own furniture: a terminal's thumb lane and foot mark and its
    /// command rail.
    PaneFurniture,
    /// A hosted page, over its shown bounds.
    HostedPage,
    /// The pane body under everything.
    PaneBody,
}

/// **The pointer's layers, top first** — the paint order of
/// [`crate::OverlayStack::bands_bottom_first`] read downwards, then the planes
/// beneath the overlay (§2.1).
pub(crate) const POINTER_LAYERS_TOP_FIRST: [PointerLayer; 16] = [
    PointerLayer::GlanceCard,
    PointerLayer::Toasts,
    PointerLayer::Palette,
    PointerLayer::TabMenu,
    PointerLayer::TermMenu,
    PointerLayer::GitMenu,
    PointerLayer::PaneMenu,
    PointerLayer::FileMenu,
    PointerLayer::Modal,
    PointerLayer::Floats,
    PointerLayer::DownloadSheet,
    PointerLayer::InPane,
    PointerLayer::DockedChrome,
    PointerLayer::PaneFurniture,
    PointerLayer::HostedPage,
    PointerLayer::PaneBody,
];

/// **The overlay bands that never take the pointer**: pictures that follow it
/// or explain it, and drawings on the layout, which a hand points *through*.
#[cfg(test)]
pub(crate) const BANDS_THAT_TAKE_NO_POINTER: [OverlayBand; 8] = [
    OverlayBand::LayoutPeek,
    OverlayBand::KeyHint,
    OverlayBand::CardHint,
    OverlayBand::Tooltip,
    OverlayBand::DragGhost,
    OverlayBand::WindowRing,
    OverlayBand::Flight,
    OverlayBand::Ground,
];

/// A plane beneath the overlay — the layers the walk asks of the window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Plane {
    DockedChrome,
    PaneFurniture,
    HostedPage,
    PaneBody,
}

impl PointerLayer {
    /// **The overlay bands this layer is painted in, top first.** Empty for a
    /// plane drawn beneath the overlay — except the docked chrome, whose icon
    /// rail is an overlay band.
    #[cfg(test)]
    pub(crate) const fn bands(self) -> &'static [OverlayBand] {
        match self {
            Self::GlanceCard => &[OverlayBand::FilePeek],
            Self::Toasts => &[OverlayBand::Toast],
            Self::Palette => &[OverlayBand::Palette],
            Self::TabMenu => &[OverlayBand::TabMenu],
            Self::TermMenu => &[OverlayBand::TermMenu],
            Self::GitMenu => &[OverlayBand::GitMenu],
            Self::PaneMenu => &[OverlayBand::PaneMenu],
            Self::FileMenu => &[OverlayBand::FileMenu],
            Self::Modal => &[OverlayBand::Modal],
            Self::Floats => &[OverlayBand::Float],
            Self::DownloadSheet => &[OverlayBand::WebSheet],
            Self::InPane => &[OverlayBand::InPane],
            Self::DockedChrome => &[OverlayBand::Rail],
            Self::PaneFurniture => &[
                OverlayBand::FormulaTools,
                OverlayBand::CommandRail,
                OverlayBand::TerminalBars,
                OverlayBand::VideoBars,
                OverlayBand::PreviewBars,
            ],
            Self::HostedPage | Self::PaneBody => &[],
        }
    }

    /// The plane this layer is, when it is one of the four beneath the
    /// overlay that the window answers.
    const fn plane(self) -> Option<Plane> {
        match self {
            Self::DockedChrome => Some(Plane::DockedChrome),
            Self::PaneFurniture => Some(Plane::PaneFurniture),
            Self::HostedPage => Some(Plane::HostedPage),
            Self::PaneBody => Some(Plane::PaneBody),
            Self::GlanceCard
            | Self::Toasts
            | Self::Palette
            | Self::TabMenu
            | Self::TermMenu
            | Self::GitMenu
            | Self::PaneMenu
            | Self::FileMenu
            | Self::Modal
            | Self::Floats
            | Self::DownloadSheet
            | Self::InPane => None,
        }
    }
}

/// **What the router says is under a point**: the layer, and the part of it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PointerHit {
    GlanceCard,
    Toast(toast::ToastHit),
    /// A row of the palette's list, or the box and no row.
    Palette(Option<usize>),
    Menu(MenuHit),
    /// A full-window card or the settings sheet: the whole window.
    Modal,
    Float(float::FloatId, float::FloatPart),
    /// A floating window's own notice pill, which stands inside its body.
    FloatPill(float::FloatId, notice::NoticeElement),
    /// A page's download sheet, and the control on it if the point is on one.
    DownloadSheet(SeatId, Option<seats::ChromeTarget>),
    InPane(InPaneHit),
    Chrome(seats::ChromeTarget),
    Furniture(Furniture),
    Page(LeafId),
    Body(SeatId),
}

/// Which menu, and what in it — each menu's own hit test's answer.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MenuHit {
    Tab(profiles::TabMenuHit),
    Term(profiles::TermMenuHit),
    Git(Option<profiles::GitMenuRow>),
    Pane(profiles::PaneMenuHit),
    File(Option<profiles::FileMenuRow>),
    Profile(Option<profiles::MenuRow>),
    Root(Option<profiles::RootMenuHit>),
    GraphFilter(Option<profiles::GitFilterRow>),
    Preview(Option<profiles::PreviewMenuHit>),
}

/// A surface inside a pane, and its part.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InPaneHit {
    Search(search::SearchElement),
    Notice(SeatId, notice::NoticeElement),
}

/// A pane's own furniture under the point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Furniture {
    TerminalLane(SeatId),
    TerminalFootMark(SeatId),
    /// A command rail and the tick nearest the point.
    CommandRail(SeatId, usize),
}

/// **What the frame measured for the walk** (§2.1): built once per frame where
/// the overlay is built, kept until the next.
///
/// It holds what only a frame can say — a menu's measured layout, a floating
/// window's risen frame and the captions its head was laid out with, which list
/// its body is showing — so the walk measures and shapes nothing. What the
/// window already keeps as it last drew it (the toasts, the palette, the
/// strips, the capsule, the download sheets, a Git page's and a graph's
/// content) is borrowed by the walk through [`PointerScene`], not copied here.
#[derive(Default)]
pub(crate) struct PointerFacts {
    pub(crate) scale: f32,
    /// The glance card's frame, while one is on the glass.
    pub(crate) glance: Option<[f32; 4]>,
    pub(crate) tab_menu: Option<profiles::TabMenuLayout>,
    pub(crate) term_menu: Option<profiles::TermMenuLayout>,
    pub(crate) git_menu: Option<profiles::GitMenuLayout>,
    pub(crate) pane_menu: Option<profiles::PaneMenuLayout>,
    pub(crate) file_menu: Option<profiles::FileMenuLayout>,
    /// A full-window card or the settings sheet is up.
    pub(crate) modal_covers_window: bool,
    pub(crate) profile_menu: Option<profiles::ProfileMenuLayout>,
    pub(crate) root_menu: Option<profiles::RootMenuLayout>,
    pub(crate) graph_filter_menu: Option<profiles::GitFilterMenuLayout>,
    pub(crate) preview_menu: Option<profiles::PreviewMenuLayout>,
    pub(crate) preview_menu_items: Vec<profiles::PreviewMenuItem>,
    /// Every live floating window, **front first** — the order the walk asks
    /// them in, so it iterates this list in place.
    pub(crate) floats: Vec<FloatFacts>,
    /// The ids of [`Self::floats`], gathered before each is measured (the
    /// measuring borrows the window the ids are read from); kept so the
    /// list is refilled rather than reallocated.
    pub(crate) float_order: Vec<float::FloatId>,
    /// The walk's own visit counter (§4.1 cut 1), for the tests that hold its
    /// cost to a formula.
    #[cfg(test)]
    pub(crate) visits: std::cell::Cell<Visits>,
}

/// One floating window as the frame drew it.
#[derive(Clone, Debug)]
pub(crate) struct FloatFacts {
    pub(crate) id: float::FloatId,
    /// Laid out on the risen frame with the head's measured captions.
    pub(crate) geometry: float::FloatGeometry,
    /// The row under the head, as it was drawn.
    pub(crate) rail: Option<seats::PreviewRailGeometry>,
    /// The no-preview card's button, where the paint centred it.
    pub(crate) card_button: Option<[f32; 4]>,
    /// The tree its body shows, when it shows one.
    pub(crate) tree: Option<seats::FilesTreeGeometry>,
    /// The Git page its body shows, laid out; the page's rows are borrowed from
    /// the window by this window's id.
    pub(crate) git: Option<git_panel::GitPanelGeometry>,
}

/// **What the walk borrows from the window** for one event: the layouts the
/// window keeps as it last drew them, and the content a floating window's body
/// shows, read by id rather than copied.
pub(crate) struct PointerScene<'a> {
    pub(crate) toasts: &'a [toast::ToastLayout],
    pub(crate) palette: Option<&'a palette::PaletteLayout>,
    pub(crate) profile_programs: &'a profiles::ProfilePrograms,
    pub(crate) recent: &'a [seed::RecentEntry],
    pub(crate) float_git_pages: &'a BTreeMap<float::FloatId, git_panel::GitPanelContent>,
    pub(crate) graphs: &'a BTreeMap<PreviewSurface, git_graph::GraphContent>,
    pub(crate) float_hover: Option<(float::FloatId, float::FloatPart)>,
    pub(crate) notices: &'a BTreeMap<NoticeHost, NoticeStrip>,
    pub(crate) web_sheets: &'a [(SeatId, websheet::SheetLayout)],
    /// The search capsule as it was last drawn.
    pub(crate) search: Option<&'a search::Capsule>,
}

/// **What one walk visited** (§4.1 cut 1): one per layer it asked, one frame
/// test per floating window down to the first that claims, and the parts of
/// that one window only.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Visits {
    /// Layers asked above the floating windows.
    pub(crate) above_floats: usize,
    /// Floating windows whose frame was tested.
    pub(crate) float_frames: usize,
    /// Floating windows whose parts were resolved.
    pub(crate) float_parts: usize,
    /// Layers asked below the floating windows.
    pub(crate) below_floats: usize,
}

/// Which count one step of the walk adds to.
#[derive(Clone, Copy)]
enum Step {
    LayerAboveFloats,
    LayerBelowFloats,
    FloatFrame,
    FloatParts,
}

impl PointerFacts {
    fn visited(&self, step: Step) {
        #[cfg(test)]
        {
            let mut visits = self.visits.get();
            match step {
                Step::LayerAboveFloats => visits.above_floats += 1,
                Step::LayerBelowFloats => visits.below_floats += 1,
                Step::FloatFrame => visits.float_frames += 1,
                Step::FloatParts => visits.float_parts += 1,
            }
            self.visits.set(visits);
        }
        #[cfg(not(test))]
        let _ = step;
    }
}

/// **The walk** (R-1–R-4): [`POINTER_LAYERS_TOP_FIRST`] asked in order, and
/// the first layer that claims the point is the answer.
///
/// A pure function of the frame's facts, the window's kept layouts and the
/// point; the four planes beneath the overlay are asked of `plane`.
pub(crate) fn walk_pointer_layers(
    scene: &PointerScene<'_>,
    facts: &PointerFacts,
    position: PhysicalPosition<f64>,
    mut plane: impl FnMut(Plane, PhysicalPosition<f64>) -> Option<PointerHit>,
) -> Option<PointerHit> {
    let mut below_floats = false;
    for layer in POINTER_LAYERS_TOP_FIRST {
        let hit = if layer == PointerLayer::Floats {
            below_floats = true;
            floats_at(scene, facts, position)
        } else {
            facts.visited(if below_floats {
                Step::LayerBelowFloats
            } else {
                Step::LayerAboveFloats
            });
            match layer.plane() {
                Some(which) => plane(which, position),
                None => overlay_layer_at(layer, scene, facts, position),
            }
        };
        if hit.is_some() {
            return hit;
        }
    }
    None
}

/// One overlay layer's claim at a point, by that layer's own hit test.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a pointer position is a surface coordinate, as every hit test here reads it"
)]
fn overlay_layer_at(
    layer: PointerLayer,
    scene: &PointerScene<'_>,
    facts: &PointerFacts,
    position: PhysicalPosition<f64>,
) -> Option<PointerHit> {
    let (x, y) = (position.x, position.y);
    let (fx, fy) = (x as f32, y as f32);
    match layer {
        PointerLayer::GlanceCard => facts
            .glance
            .filter(|frame| file_peek::contains(*frame, [fx, fy]))
            .map(|_| PointerHit::GlanceCard),
        PointerLayer::Toasts => toast::at(scene.toasts, fx, fy).map(PointerHit::Toast),
        PointerLayer::Palette => scene
            .palette
            .and_then(|layout| palette::hit(layout, x, y))
            .map(PointerHit::Palette),
        PointerLayer::TabMenu => facts
            .tab_menu
            .as_ref()
            .and_then(|layout| profiles::tab_menu_hit(layout, x, y))
            .map(|hit| PointerHit::Menu(MenuHit::Tab(hit))),
        PointerLayer::TermMenu => facts
            .term_menu
            .as_ref()
            .and_then(|layout| profiles::term_menu_hit(layout, x, y))
            .map(|hit| PointerHit::Menu(MenuHit::Term(hit))),
        PointerLayer::GitMenu => facts
            .git_menu
            .as_ref()
            .and_then(|layout| profiles::git_menu_hit(layout, x, y))
            .map(|row| PointerHit::Menu(MenuHit::Git(row))),
        PointerLayer::PaneMenu => facts
            .pane_menu
            .as_ref()
            .and_then(|layout| profiles::pane_menu_hit(layout, x, y))
            .map(|hit| PointerHit::Menu(MenuHit::Pane(hit))),
        PointerLayer::FileMenu => facts
            .file_menu
            .as_ref()
            .and_then(|layout| profiles::file_menu_hit(layout, x, y))
            .map(|row| PointerHit::Menu(MenuHit::File(row))),
        PointerLayer::Modal => modal_at(scene, facts, position),
        PointerLayer::DownloadSheet => scene
            .web_sheets
            .iter()
            .find(|(_, layout)| websheet::covers(layout, fx, fy))
            .map(|(seat, layout)| {
                PointerHit::DownloadSheet(*seat, websheet::hit(layout, *seat, fx, fy))
            }),
        PointerLayer::InPane => IN_PANE_SURFACES_TOP_FIRST
            .iter()
            .find_map(|surface| match surface {
                InPaneSurface::SearchCapsule => scene
                    .search
                    .and_then(|capsule| search::hit(capsule, fx, fy))
                    .map(InPaneHit::Search),
                InPaneSurface::NoticeStrip => {
                    scene.notices.iter().find_map(|(host, strip)| match host {
                        NoticeHost::Seat(seat) => notice::claim(&strip.bar, fx, fy)
                            .map(|element| InPaneHit::Notice(*seat, element)),
                        NoticeHost::Float(_) => None,
                    })
                }
            })
            .map(PointerHit::InPane),
        // Asked by `walk` itself: the floats count their own visits, and the
        // planes are the window's.
        PointerLayer::Floats
        | PointerLayer::DockedChrome
        | PointerLayer::PaneFurniture
        | PointerLayer::HostedPage
        | PointerLayer::PaneBody => None,
    }
}

/// The modal band: the whole window under a full-window card or the settings
/// sheet, else the frames of the four menus staged in it.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a pointer position is a surface coordinate, as every hit test here reads it"
)]
fn modal_at(
    scene: &PointerScene<'_>,
    facts: &PointerFacts,
    position: PhysicalPosition<f64>,
) -> Option<PointerHit> {
    if facts.modal_covers_window {
        return Some(PointerHit::Modal);
    }
    let (x, y) = (position.x, position.y);
    let (fx, fy) = (x as f32, y as f32);
    let menu = |hit| Some(PointerHit::Menu(hit));
    if let Some(layout) = facts.profile_menu.as_ref()
        && layout.contains(fx, fy)
    {
        return menu(MenuHit::Profile(
            profiles::hit(layout, scene.profile_programs, scene.recent, x, y).flatten(),
        ));
    }
    if let Some(row) = facts
        .root_menu
        .as_ref()
        .and_then(|layout| profiles::root_menu_hit(layout, x, y))
    {
        return menu(MenuHit::Root(row));
    }
    if let Some(row) = facts
        .graph_filter_menu
        .as_ref()
        .and_then(|layout| profiles::git_filter_menu_hit(layout, x, y))
    {
        return menu(MenuHit::GraphFilter(row));
    }
    facts
        .preview_menu
        .as_ref()
        .and_then(|layout| profiles::preview_menu_hit(layout, &facts.preview_menu_items, x, y))
        .and_then(|row| menu(MenuHit::Preview(row)))
}

/// **The floating windows, front first**: one frame test per window down to
/// the first that claims the point, then that window's parts and no other's.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a pointer position is a surface coordinate, as every hit test here reads it"
)]
fn floats_at(
    scene: &PointerScene<'_>,
    facts: &PointerFacts,
    position: PhysicalPosition<f64>,
) -> Option<PointerHit> {
    let (x, y) = (position.x as f32, position.y as f32);
    let claimant = facts.floats.iter().find(|win| {
        facts.visited(Step::FloatFrame);
        let frame = win.geometry.frame;
        x >= frame[0] && x < frame[2] && y >= frame[1] && y < frame[3]
    })?;
    facts.visited(Step::FloatParts);
    Some(float_part_at(scene, facts.scale, claimant, x, y))
}

/// What part of one floating window a point inside its frame is on: its own
/// notice pill first (the pill stands inside the body), then the chassis, and
/// inside the body whichever list the body is showing.
fn float_part_at(
    scene: &PointerScene<'_>,
    scale: f32,
    win: &FloatFacts,
    x: f32,
    y: f32,
) -> PointerHit {
    if let Some(element) = scene
        .notices
        .get(&NoticeHost::Float(win.id))
        .and_then(|strip| notice::claim(&strip.bar, x, y))
    {
        return PointerHit::FloatPill(win.id, element);
    }
    let body = win.geometry.body;
    let part = float::float_hit(&win.geometry, x, y, win.rail.as_ref(), |x, y| {
        let (x, y) = (x + body[0], y + body[1]);
        if let Some(button) = win.card_button
            && x >= button[0]
            && x < button[2]
            && y >= button[1]
            && y < button[3]
        {
            return Some(float::FloatPart::CardButton);
        }
        if let Some(graph) = scene.graphs.get(&PreviewSurface::Float(win.id)) {
            return git_graph::graph_hit(body, graph, scale, x, y).map(|hit| match hit {
                git_graph::GraphHit::Tool(tool) => float::FloatPart::GraphTool(tool),
                git_graph::GraphHit::Detail { index, part } => {
                    float::FloatPart::GraphDetail { index, part }
                }
                git_graph::GraphHit::Row(index) => float::FloatPart::GraphRow(index),
            });
        }
        if let (Some(geometry), Some(page)) = (win.git.as_ref(), scene.float_git_pages.get(&win.id))
        {
            let index = geometry.row_at(x, y)?;
            let row = page.rows.get(index)?;
            let hovered = matches!(
                scene.float_hover,
                Some((on, float::FloatPart::Row(at) | float::FloatPart::GitAct { index: at, .. }))
                    if on == win.id && at == index
            );
            return Some(
                git_panel::act_at(row, geometry.row_rect(index), scale, hovered, x, y)
                    .map_or(float::FloatPart::Row(index), |act| {
                        float::FloatPart::GitAct { index, act }
                    }),
            );
        }
        win.tree.as_ref()?.row_at(x, y).map(float::FloatPart::Row)
    })
    // A point inside the frame is always one of the window's parts; the
    // chassis answers `Head` for whatever its boxes leave over.
    .unwrap_or(float::FloatPart::Head);
    PointerHit::Float(win.id, part)
}
