//! The profile picker — the menu the tab strip's `˅` opens.
//!
//! Spec authority is `design/ui-mockup.html`: the `.profile-menu` / `.profile-item`
//! block (lines 976-1002) for the surface and its rows, and `openProfileMenu`
//! (line 7296) for where the menu lands and what a click on a row does. Every
//! number below is that stylesheet's own.
//!
//! Two facts decide the shape of this module:
//!
//! * **It is a popup, not a modal.** There is no scrim, so unlike [`crate::settings`]
//!   its [`hit`] returns `None` for a point that is not on the menu, and a press
//!   there closes the menu and then goes on about its business — which is exactly
//!   what the mock-up's `document.addEventListener("click", closeProfileMenu)`
//!   does.
//! * **It floats, so it blends.** Its lift, its hairline and its face are the
//!   same three planes every floating surface in this product is made of, built
//!   through the same [`crate::settings::push_float_window`] — a popup drawn out
//!   of opaque chrome quads would have to know what is under it, and nothing is
//!   under a popup but whatever the terminal happens to be showing.
//! * **It shows two lists, so a row is not a number.** Under the profiles sits
//!   `Recently opened` (mock-up 7311-7320), and its rows index the seed vault
//!   rather than [`PROFILES`]. Both [`hit`] and the hover therefore speak in
//!   [`MenuRow`], because the one thing a bare index cannot say is which list it
//!   came from — and the answer it gets wrong is silent.

use std::time::SystemTime;

use bt_render::{
    ChromeLabel, ChromeLabelWeight, ChromePalette, FLOAT_WINDOW_BORDER_LOGICAL_PX,
    FLOAT_WINDOW_SHADOW_LOGICAL_PX, OverlayQuad, chrome_palette, rounded_overlay_fill,
};

use crate::{
    marks::{ChromeMark, ChromeSprite, OverlayLayer},
    seed::{RECENT_CAPACITY, RecentEntry, Seed, ago_label},
    settings::push_float_window,
};

// ── `.profile-menu` ────────────────────────────────────────────────────────
/// `min-width: 180px`. It is the only width the menu has: every row is one mark
/// and one short name, so nothing here ever asks for more than the minimum.
const MENU_MIN_WIDTH_LOGICAL_PX: f32 = 180.0;
/// `border-radius: 8px` — a popup menu's own round, the same one the theme
/// picker's menu wears, and deliberately not the 10px a floating *window* gets.
const MENU_RADIUS_LOGICAL_PX: f32 = 8.0;
const MENU_PADDING_LOGICAL_PX: f32 = 4.0;
/// `menu.style.top = a.bottom + 4` — the gap between the button and its menu.
const MENU_OFFSET_LOGICAL_PX: f32 = 4.0;
/// `Math.min(a.left, win.width - mw - 8)` — the menu never touches the window's
/// right edge, however near the edge the button that opened it sits.
const MENU_EDGE_MARGIN_LOGICAL_PX: f32 = 8.0;

// ── `.profile-item` ────────────────────────────────────────────────────────
/// `padding: 7px 10px` around a 13px line box, which measures 15.5px in the
/// mock-up's own renderer: 7 + 15.5 + 7.
const ITEM_HEIGHT_LOGICAL_PX: f32 = 29.5;
const ITEM_RADIUS_LOGICAL_PX: f32 = 5.0;
const ITEM_PADDING_X_LOGICAL_PX: f32 = 10.0;
/// `.profile-item { gap: 10px }`.
const ITEM_GAP_LOGICAL_PX: f32 = 10.0;
const ITEM_FONT_LOGICAL_PX: f32 = 13.0;
/// `.profile-item .ticon { width: 14px }` — the column. The mark inside it is
/// the strip's own 15px `.pmark`, centred, exactly as the flex box centres it.
const ITEM_ICON_COLUMN_LOGICAL_PX: f32 = 14.0;
const ITEM_MARK_LOGICAL_PX: f32 = 15.0;
/// `.default-hint { margin-left: auto; font-size: 11px; color: var(--ink3) }`.
///
/// Two annotations ride in this one slot: the profile list's `default`, and a
/// recent row's `agoLabel` (mock-up 7319). They are the same declaration in the
/// same place, so they are the same number here.
const HINT_FONT_LOGICAL_PX: f32 = 11.0;
const HINT_TEXT: &str = "default";

// ── `.menu-sep` (mock-up line 996) ─────────────────────────────────────────
/// `height: 1px`, taken to whole device pixels and never below one.
///
/// Rounded rather than left fractional, which is where the floating window's own
/// border differs: a border is four edges around a rounded box that the coverage
/// pass is already antialiasing, while this is a single horizontal line, and a
/// horizontal line 1.25px tall is drawn as two rows of partial ink — a blurred
/// grey band instead of a rule. The `max` keeps it from rounding away entirely
/// at the scales where the ink is thinnest.
const SEPARATOR_THICKNESS_LOGICAL_PX: f32 = 1.0;
/// `margin: 5px 0`.
const SEPARATOR_MARGIN_Y_LOGICAL_PX: f32 = 5.0;
/// `background: var(--border-soft)` — `rgba(255,255,255,.06)` on dark,
/// `rgba(0,0,0,.055)` on light (mock-up lines 20 and 50).
///
/// The ink is the one `ChromePalette::menu_border` already carries (both tokens
/// are the theme's own black or white); only this softer alpha is missing from
/// the palette, so the pair is stated here and chosen **off the ink the palette
/// handed us** rather than off [`bt_render::current_theme`]. That is not a
/// detour: the palette is picked by background luma and the theme by the user's
/// setting, and under a `BT_BG` override those two answers differ — asking the
/// palette keeps the hairline in the same theme as the surface under it.
///
/// Its proper home is a pre-composited `--border-soft` over `--menu` in
/// [`ChromePalette`], which is a bt-render change this work item may not make.
const SEPARATOR_ALPHA_ON_DARK: f32 = 0.06;
/// The light theme's half of [`SEPARATOR_ALPHA_ON_DARK`].
const SEPARATOR_ALPHA_ON_LIGHT: f32 = 0.055;

// ── `.menu-label` (mock-up lines 997-1000) ─────────────────────────────────
const SECTION_LABEL_FONT_LOGICAL_PX: f32 = 10.5;
/// The 10.5px line box, measured in the mock-up's own renderer (Inter at
/// `line-height: normal`) — 12.5px, the same ladder its 11px group label climbs
/// at 13px and its 13px row at 15.5px.
const SECTION_LABEL_LINE_LOGICAL_PX: f32 = 12.5;
/// `letter-spacing: .05em` at `font-weight: 600` — the settings dialog's
/// `.group-label` craft, which is the same heading in a different surface.
const SECTION_LABEL_TRACKING_EM: f32 = 0.05;
/// `padding: 3px 10px 5px` — top, both sides, bottom.
const SECTION_LABEL_PADDING_TOP_LOGICAL_PX: f32 = 3.0;
const SECTION_LABEL_PADDING_X_LOGICAL_PX: f32 = 10.0;
const SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX: f32 = 5.0;
/// `Recently opened` under `text-transform: uppercase`.
///
/// The transform is a *rendering* of the heading, and this pipeline has no
/// transform: a chrome label draws the string it is given. So the string it is
/// given is the drawn one, and the mock-up's own casing lives in the doc line
/// above rather than in a lowercase constant nothing would uppercase.
const RECENT_SECTION_LABEL: &str = "RECENTLY OPENED";

// ── `.recent-item` (mock-up lines 1001-1002) ───────────────────────────────
/// `max-width: 260px`.
///
/// It is a real clamp on the row's box and it cannot bind today: the menu is
/// [`MENU_MIN_WIDTH_LOGICAL_PX`] wide and nothing here measures text, so every
/// row is already 170px of content. In the mock-up the menu is content-sized
/// (`min-width: 180px` over `white-space: nowrap` rows) and this is what stops
/// one long path from stretching the popup across the window — the day this
/// module can measure a string, that growth and the ellipsis at mock-up 1002
/// arrive together, and the clamp is already where it belongs.
const RECENT_ITEM_MAX_WIDTH_LOGICAL_PX: f32 = 260.0;

/// A profile the picker can start a tab from.
///
/// One entry, and the list is a list rather than a constant because that is the
/// honest shape of it: the mock-up carries four (PowerShell, WSL, Git Bash,
/// Command Prompt) and this build launches exactly one shell. Offering the other
/// three would be three rows that cannot do what they say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    /// The name a seed keeps this profile by — `docs/DESIGN.md` §7.1.4 requires a
    /// "**稳定 profile_id**（不是标题、不是展示对象）".
    ///
    /// It is deliberately not [`Self::title`]: a title is a display object, and
    /// display objects get renamed, localised and reworded. A seed keyed on one
    /// would stop matching its own profile the day the strip's wording changed,
    /// and the tab would come back as somebody else. It is not the executable
    /// path either — that is what the shell *is*, not which profile chose it, and
    /// two profiles can legitimately launch the same binary.
    pub id: &'static str,
    pub title: &'static str,
    /// A profile's icon is its mark, not a letter that happens to be in its
    /// prompt — the mock-up says so in as many words at `const mark`.
    pub mark: ChromeMark,
}

pub const PROFILES: [Profile; 1] = [Profile {
    id: "pwsh",
    title: "PowerShell",
    mark: ChromeMark::ProfilePowerShell,
}];

/// The index a new tab is started from when nobody picks — `state.defaultProfile`.
pub const DEFAULT_PROFILE: usize = 0;

/// Which profile a seed's `profile_id` names, or [`DEFAULT_PROFILE`] when the
/// file names one this build does not have.
///
/// Falling back rather than refusing is the schema's own rule — `§5.4` 逐叶降级,
/// "未知 profile→默认": a profile that was removed (or that a newer build wrote)
/// must cost you that tab's *shell choice*, never the tab. The place you were
/// standing is the part worth keeping, and it survives this.
#[must_use]
pub fn index_of_id(id: &str) -> usize {
    PROFILES
        .iter()
        .position(|profile| profile.id == id)
        .unwrap_or(DEFAULT_PROFILE)
}

/// Which row of the menu, and **what kind of row** — the two lists the picker
/// shows are indexed separately and a bare number cannot say which one it is
/// counting.
///
/// The tag is load-bearing rather than tidy. The menu used to be [`PROFILES`]
/// and nothing else, so a row index *was* a profile index and the two could be
/// the same integer; the moment a Recent section sits under the profiles, that
/// same integer names two different things, and the failure it produces is not
/// a panic but a silent one — clicking `~/repo · 3m ago` launching a plain
/// PowerShell in the wrong place, which looks like the menu working.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuRow {
    /// An index into [`PROFILES`]: start a new tab from this profile.
    Profile(usize),
    /// An index into the vault slice the menu was laid out from: revive this
    /// seed. It is the vault's own index, so [`crate::seed::SeedVault::take`]
    /// consumes it directly.
    Recent(usize),
}

/// Whether the picker is up, and which row the pointer is on.
///
/// App state and nothing else: not a seat, so the solver never sees it; not an
/// intent, so the session file never sees it. A menu that survived a restart
/// would be a window that opens mid-gesture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProfileMenu {
    open: bool,
    hover: Option<MenuRow>,
}

impl ProfileMenu {
    pub fn is_open(self) -> bool {
        self.open
    }

    /// The chevron: open when shut, shut when open. A control that opens
    /// something must also put it away — the mock-up learned that one the hard
    /// way, and its comment says so.
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.hover = None;
    }

    /// Shut it, and report whether there was anything to shut — which is what
    /// tells Esc and a press elsewhere whether they consumed anything.
    pub fn close(&mut self) -> bool {
        let was_open = self.open;
        self.open = false;
        self.hover = None;
        was_open
    }

    /// Returns whether the hover changed, so a caller can skip a repaint.
    pub fn set_hover(&mut self, hover: Option<MenuRow>) -> bool {
        let hover = if self.open { hover } else { None };
        let changed = self.hover != hover;
        self.hover = hover;
        changed
    }

    pub fn hover(self) -> Option<MenuRow> {
        self.hover
    }
}

/// Every rectangle the menu draws and hit-tests, in physical pixels of the whole
/// surface.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileMenuLayout {
    scale: f32,
    /// The menu's border box.
    frame: [f32; 4],
    /// One row per entry of [`PROFILES`], top to bottom.
    items: Vec<[f32; 4]>,
    /// `.menu-sep`'s 1px rule, or `None` when there is nothing to separate.
    ///
    /// The three Recent boxes are `Option`/empty together and never singly:
    /// mock-up 7311 is one ternary over `state.recent.length`, and a heading
    /// over an empty list is a promise the menu cannot keep.
    separator: Option<[f32; 4]>,
    /// `.menu-label`'s band, padding included.
    section_label: Option<[f32; 4]>,
    /// One row per vault entry the menu shows, newest first.
    recent: Vec<[f32; 4]>,
}

/// What the menu shows of a vault: its first [`RECENT_CAPACITY`] entries.
///
/// The cap is the vault's own (`docs/DESIGN.md` §7.1.4, mock-up 4056) and not a
/// second policy invented here — but it is applied here too, because a menu is
/// a surface with a window edge under it and "however many the caller passed"
/// is not a height. Both [`layout`] and [`build`] read the slice through this,
/// so the rectangles and the rows drawn into them cannot disagree.
fn menu_rows(recent: &[RecentEntry]) -> &[RecentEntry] {
    &recent[..recent.len().min(RECENT_CAPACITY)]
}

/// Which way the menu hangs off the button that opened it.
///
/// `openProfileMenu` (mock-up 7357-7405) needs no such choice: it writes `top:
/// a.bottom + 4; left: a.left` off whatever element was clicked, and in a
/// document that is right for both layouts for free, because both chevrons are
/// real boxes and a menu below either one has the whole page to fall into.
///
/// This window is not a page. Below-and-left of a *rail* button is the rail's
/// own column — 46px of it while the rail is parked — so the menu would be laid
/// down the sidebar it was opened from. A vertical strip keeps its free space to
/// the side, which is the same reason [`crate::peek_strip::PeekSide`] exists and
/// the same answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuSide {
    /// Under the button, sharing its left edge. The horizontal strip.
    Below,
    /// To the right of the button, aligned with its top. The vertical rail.
    Beside,
}

/// The menu hung off `anchor` — the `˅`'s own box, in physical pixels — inside
/// a surface this big, showing `recent` under the profiles.
///
/// No clock is read here and none is passed: how long ago a seed was closed is
/// a fact about the moment it is *drawn*, so it belongs to [`build`], and a
/// layout that took the time would change shape between two frames of one open
/// menu.
#[must_use]
pub fn layout(
    anchor: [f32; 4],
    side: MenuSide,
    surface: (f32, f32),
    scale: f32,
    recent: &[RecentEntry],
) -> ProfileMenuLayout {
    let px = |value: f32| value * scale;
    let recent = menu_rows(recent);
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let padding = px(MENU_PADDING_LOGICAL_PX);
    let item_height = px(ITEM_HEIGHT_LOGICAL_PX).round();
    let separator_thickness = (SEPARATOR_THICKNESS_LOGICAL_PX * scale).round().max(1.0);
    let separator_margin = px(SEPARATOR_MARGIN_Y_LOGICAL_PX).round();
    // `margin: 5px 0` above and below the rule, and nothing to collapse against:
    // a row carries no vertical margin of its own.
    //
    // Every term here is a whole number of device pixels, and that is what makes
    // the section *additive*: the menu's height is the rounded sum, so a section
    // measured in whole pixels adds exactly its own height to it rather than a
    // pixel more or less depending on where the fraction under it happened to
    // sit.
    let separator_block = 2.0 * separator_margin + separator_thickness;
    let section_block = px(SECTION_LABEL_PADDING_TOP_LOGICAL_PX
        + SECTION_LABEL_LINE_LOGICAL_PX
        + SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX)
    .round();
    let recent_block = if recent.is_empty() {
        0.0
    } else {
        separator_block + section_block + item_height * recent.len() as f32
    };

    let width = px(MENU_MIN_WIDTH_LOGICAL_PX).round();
    let height =
        (2.0 * (border + padding) + item_height * PROFILES.len() as f32 + recent_block).round();
    let (surface_width, surface_height) = surface;
    let edge = px(MENU_EDGE_MARGIN_LOGICAL_PX);
    let (left, top) = match side {
        // `menu.style.top = a.bottom + 4; menu.style.left = Math.min(a.left,
        // win.width - mw - 8)` — the mock-up's own two lines.
        MenuSide::Below => (
            anchor[0].min(surface_width - width - edge).max(0.0).round(),
            (anchor[3] + px(MENU_OFFSET_LOGICAL_PX)).round(),
        ),
        // The same four pixels turned through a right angle. The rail's `˅`
        // stands beside its `+` when the panel is open and collapses to nothing
        // when it is parked (Q181), so the box handed in here is the chevron's
        // in one state and the `+`'s in the other — and "clear of its right
        // edge, level with its top" is the one placement that reads the same for
        // both, because the two share that edge and that top by construction.
        MenuSide::Beside => (
            (anchor[2] + px(MENU_OFFSET_LOGICAL_PX))
                .min(surface_width - width - edge)
                .max(0.0)
                .round(),
            anchor[1]
                .min(surface_height - height - edge)
                .max(0.0)
                .round(),
        ),
    };
    let frame = [left, top, left + width, top + height];

    let content_left = frame[0] + border + padding;
    let content_right = frame[2] - border - padding;
    let mut cursor = frame[1] + border + padding;
    let mut items = Vec::with_capacity(PROFILES.len());
    for _ in 0..PROFILES.len() {
        items.push([content_left, cursor, content_right, cursor + item_height]);
        cursor += item_height;
    }
    let (separator, section_label, recent_rows) = if recent.is_empty() {
        (None, None, Vec::new())
    } else {
        let separator = [
            content_left,
            cursor + separator_margin,
            content_right,
            cursor + separator_margin + separator_thickness,
        ];
        cursor += separator_block;
        let section_label = [content_left, cursor, content_right, cursor + section_block];
        cursor += section_block;
        // `.recent-item { max-width: 260px }` — see the constant: a clamp that
        // cannot bind while the menu keeps its min-width, and the right place
        // for it the day the menu is content-sized.
        let recent_right = content_right.min(content_left + px(RECENT_ITEM_MAX_WIDTH_LOGICAL_PX));
        let mut rows = Vec::with_capacity(recent.len());
        for _ in recent {
            rows.push([content_left, cursor, recent_right, cursor + item_height]);
            cursor += item_height;
        }
        (Some(separator), Some(section_label), rows)
    };

    ProfileMenuLayout {
        scale,
        frame,
        items,
        separator,
        section_label,
        recent: recent_rows,
    }
}

/// What a point is over: a row and which list it belongs to, `Some(None)` for
/// the menu's own body between and around its rows, and `None` for anywhere else
/// in the window.
///
/// The two negatives are different answers and the difference is the whole of
/// what "popup" means here: a press on the body is the menu's and does nothing,
/// a press outside it belongs to whatever is there and merely closes the menu on
/// its way past.
///
/// The separator and the heading are body, not rows — they are the two things in
/// the menu that name nothing you can open.
#[must_use]
pub fn hit(layout: &ProfileMenuLayout, x: f64, y: f64) -> Option<Option<MenuRow>> {
    let (x, y) = (x as f32, y as f32);
    for (index, item) in layout.items.iter().enumerate() {
        if contains(*item, x, y) {
            return Some(Some(MenuRow::Profile(index)));
        }
    }
    for (index, row) in layout.recent.iter().enumerate() {
        if contains(*row, x, y) {
            return Some(Some(MenuRow::Recent(index)));
        }
    }
    contains(layout.frame, x, y).then_some(None)
}

fn contains(rect: [f32; 4], x: f32, y: f32) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

/// The menu's three planes and its rows, as one overlay layer.
///
/// One layer and not more: a popup with nothing of its own inside it has nothing
/// to cover but the window, and the window is not the overlay's to draw. The
/// stack exists so a surface can cover another surface the overlay drew — see
/// [`crate::settings::build`], where the picker is a second layer over the dialog
/// it hangs off.
#[must_use]
pub fn build(
    layout: &ProfileMenuLayout,
    hover: Option<MenuRow>,
    recent: &[RecentEntry],
    now: SystemTime,
) -> Vec<OverlayLayer> {
    let palette = chrome_palette();
    let scale = layout.scale;
    let px = |value: f32| value * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let mut quads = Vec::new();
    let mut labels = Vec::new();
    let mut sprites = Vec::new();

    push_float_window(
        &mut quads,
        layout.frame,
        px(MENU_RADIUS_LOGICAL_PX),
        border,
        px(FLOAT_WINDOW_SHADOW_LOGICAL_PX),
        palette.menu_surface,
        palette.menu_shadow,
        alpha(palette.menu_popup_shadow_inner_alpha),
        alpha(palette.menu_popup_shadow_outer_alpha),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
    );

    for (index, item) in layout.items.iter().enumerate() {
        let profile = PROFILES[index];
        push_row(
            &Row {
                rect: *item,
                mark: profile.mark,
                name: profile.title,
                // `margin-left: auto` puts the hint hard against the row's
                // trailing padding, and it names a fact about the profile rather
                // than the row's state — so it does not answer to hover.
                hint: (index == DEFAULT_PROFILE).then_some(HINT_TEXT.to_owned()),
                hovered: hover == Some(MenuRow::Profile(index)),
            },
            scale,
            palette,
            &mut quads,
            &mut labels,
            &mut sprites,
        );
    }

    if let Some(rule) = layout.separator {
        quads.push(OverlayQuad {
            rect: rule,
            color: palette.menu_border,
            alpha: separator_alpha(palette.menu_border),
        });
    }

    if let Some(band) = layout.section_label {
        labels.push(ChromeLabel {
            text: RECENT_SECTION_LABEL.to_owned(),
            // The band's content box: padding stripped, so the 12.5px line box
            // is centred in exactly its own height and the 3px above it and 5px
            // below it stay the stylesheet's rather than the renderer's.
            rect: [
                band[0] + px(SECTION_LABEL_PADDING_X_LOGICAL_PX),
                band[1] + px(SECTION_LABEL_PADDING_TOP_LOGICAL_PX),
                band[2] - px(SECTION_LABEL_PADDING_X_LOGICAL_PX),
                band[3] - px(SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX),
            ],
            font_size_px: px(SECTION_LABEL_FONT_LOGICAL_PX),
            // `--ink3` over `--menu` — the same ink the row hints wear, because
            // it is the same declaration on the same surface.
            color: palette.menu_item_hint_text,
            align_right: false,
            align_center: false,
            letter_spacing_em: SECTION_LABEL_TRACKING_EM,
            weight: ChromeLabelWeight::SemiBold,
            tabular_numerals: false,
            clip: None,
        });
    }

    for (index, (row, entry)) in layout.recent.iter().zip(menu_rows(recent)).enumerate() {
        push_row(
            &Row {
                rect: *row,
                mark: recent_mark(&entry.seed),
                name: recent_label(&entry.seed),
                hint: Some(ago_label(entry.at, now)),
                hovered: hover == Some(MenuRow::Recent(index)),
            },
            scale,
            palette,
            &mut quads,
            &mut labels,
            &mut sprites,
        );
    }

    vec![OverlayLayer {
        quads,
        labels,
        sprites,
        ..Default::default()
    }]
}

/// One `.profile-item`, whichever list it belongs to.
///
/// The two lists are the same row — mock-up 7317 is `class="profile-item
/// recent-item"`, and `.recent-item` adds a width and nothing else. So they are
/// drawn by one function rather than two that look alike, because the way two
/// menu rows drift apart is that somebody fixes the ink on one of them.
struct Row<'a> {
    rect: [f32; 4],
    mark: ChromeMark,
    name: &'a str,
    /// The `.default-hint` slot: `default` on the default profile, `3m ago` on
    /// a recent row, nothing on the rest.
    hint: Option<String>,
    hovered: bool,
}

fn push_row(
    row: &Row<'_>,
    scale: f32,
    palette: ChromePalette,
    quads: &mut Vec<OverlayQuad>,
    labels: &mut Vec<ChromeLabel>,
    sprites: &mut Vec<ChromeSprite>,
) {
    let px = |value: f32| value * scale;
    let item = row.rect;
    if row.hovered {
        quads.extend(rounded_overlay_fill(
            item,
            px(ITEM_RADIUS_LOGICAL_PX),
            palette.menu_item_hover,
            1.0,
        ));
    }
    // The 15px mark centred on its own 14px column, which is what a flex box
    // does with a child one pixel wider than the box it is in.
    let column_left = item[0] + px(ITEM_PADDING_X_LOGICAL_PX);
    let column_right = column_left + px(ITEM_ICON_COLUMN_LOGICAL_PX);
    let mark = px(ITEM_MARK_LOGICAL_PX).round();
    let mark_left = ((column_left + column_right - mark) / 2.0).round();
    let mark_top = ((item[1] + item[3] - mark) / 2.0).round();
    sprites.push(ChromeSprite::new(
        row.mark,
        [mark_left, mark_top, mark_left + mark, mark_top + mark],
        palette.accent,
    ));
    labels.push(ChromeLabel {
        text: row.name.to_owned(),
        // The name's box ends at the row's trailing padding, and the row's own
        // right edge is where `.recent-item`'s `max-width` already landed. A
        // `ChromeLabel` clips per glyph and per pixel, so a name too long for
        // that box is cropped exactly as CSS `overflow: hidden` crops it —
        // mock-up 1002 asks for `text-overflow: ellipsis` instead, and the `…`
        // needs a measured string this module is not given.
        rect: [
            column_right + px(ITEM_GAP_LOGICAL_PX),
            item[1],
            item[2] - px(ITEM_PADDING_X_LOGICAL_PX),
            item[3],
        ],
        font_size_px: px(ITEM_FONT_LOGICAL_PX),
        color: if row.hovered {
            palette.menu_item_text_selected
        } else {
            palette.menu_item_text
        },
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: None,
    });
    if let Some(hint) = &row.hint {
        labels.push(ChromeLabel {
            text: hint.clone(),
            rect: [
                item[0],
                item[1],
                item[2] - px(ITEM_PADDING_X_LOGICAL_PX),
                item[3],
            ],
            font_size_px: px(HINT_FONT_LOGICAL_PX),
            // `--ink3` over `--menu`. It used to be `dialog_muted_text`,
            // which is the same ink over `--win` — the settings dialog's
            // surface, not this one. Identical in the light theme, six levels
            // adrift in the dark.
            color: palette.menu_item_hint_text,
            align_right: true,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        });
    }
}

/// `--border-soft`'s alpha for the theme whose `--border` is drawn in `ink`.
///
/// White is the dark theme's hairline and black is the light theme's — the
/// palette's own convention, documented at `ChromePalette::menu_border`.
fn separator_alpha(ink: [u8; 3]) -> f32 {
    if ink == [0xff, 0xff, 0xff] {
        SEPARATOR_ALPHA_ON_DARK
    } else {
        SEPARATOR_ALPHA_ON_LIGHT
    }
}

/// The mark a recent row wears — mock-up 7314/7318.
///
/// A terminal seed wears **its own profile's** mark rather than a generic one:
/// the row is offering to reopen that shell, and the picker's rows one section
/// up are already teaching what the mark means. A files locus has no profile,
/// so it wears the folder the pane is (`#i-folder` in `--accent`, mock-up 7314).
fn recent_mark(seed: &Seed) -> ChromeMark {
    match seed {
        Seed::Term { profile_id, .. } => PROFILES[index_of_id(profile_id)].mark,
        Seed::Files { .. } => ChromeMark::Folder,
    }
}

/// What a recent row calls itself — mock-up 7318: `r.seed.name || cwdLeaf(r.seed)`.
///
/// Your own name for the tab wins, and the folder it stood in answers when you
/// never gave it one. An empty manual name is not a name: `||` in the mock-up
/// falls through an empty string, and a row captioned with nothing would be a
/// row you cannot tell from the one above it.
fn recent_label(seed: &Seed) -> &str {
    match seed {
        Seed::Term {
            cwd, manual_name, ..
        } => manual_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| cwd_leaf(cwd)),
        // A files locus has no name of its own; the mock-up captions it with the
        // same leaf rule applied to its root.
        Seed::Files { root } => cwd_leaf(root),
    }
}

/// The last segment of a path, drive-root aware: `C:\` is `C:` and not the empty
/// string a naive split leaves behind the trailing separator.
///
/// **Duplicated** from `main.rs`'s `cwd_leaf`, deliberately and temporarily: that
/// one is the tab-title layer's, it takes a `&Path`, and `main.rs` is a binary
/// crate root that nothing can import from. The two must stay the same rule —
/// a Recent row that names a folder differently from the tab it reopens is the
/// same place under two names — so the day either moves, both move together.
fn cwd_leaf(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['\\', '/']);
    let leaf = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    if leaf.is_empty() { trimmed } else { leaf }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    /// The one layer a popup with nothing inside it draws.
    fn one_layer(layers: Vec<OverlayLayer>) -> OverlayLayer {
        let [layer]: [OverlayLayer; 1] = layers
            .try_into()
            .expect("a popup with no popup of its own is one layer");
        layer
    }

    /// The `˅`'s box in a 960x600 window at 1x, taken from the strip's own
    /// geometry rather than restated here — one ordinary unpinned tab.
    fn anchor(scale: f32) -> [f32; 4] {
        let strip = [crate::seats::TabTrailer {
            pinned: false,
            reveal: 0.0,
        }];
        crate::seats::tab_strip_geometry(960.0 * scale, scale, &strip, 0, 0.0).new_tab_menu
    }

    /// A vault with nothing in it: the menu every test that predates Recent was
    /// written against.
    const NO_RECENT: &[RecentEntry] = &[];

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    /// The moment the menu is drawn in these tests. A fixed one: the ago labels
    /// are a function of two instants and neither of them is the wall clock.
    fn now() -> SystemTime {
        at(100_000)
    }

    fn term(cwd: &str, manual_name: Option<&str>, secs_ago: u64) -> RecentEntry {
        RecentEntry {
            seed: Seed::Term {
                profile_id: PROFILES[DEFAULT_PROFILE].id.to_owned(),
                cwd: cwd.to_owned(),
                manual_name: manual_name.map(str::to_owned),
            },
            at: at(100_000 - secs_ago),
        }
    }

    fn files(root: &str, secs_ago: u64) -> RecentEntry {
        RecentEntry {
            seed: Seed::Files {
                root: root.to_owned(),
            },
            at: at(100_000 - secs_ago),
        }
    }

    /// The height the Recent section adds at `scale`: `.menu-sep` with its two
    /// margins, `.menu-label` with its padding, and one row per seed.
    fn recent_block(scale: f32, rows: usize) -> f32 {
        let separator = 2.0 * (SEPARATOR_MARGIN_Y_LOGICAL_PX * scale).round()
            + (SEPARATOR_THICKNESS_LOGICAL_PX * scale).round().max(1.0);
        let heading = ((SECTION_LABEL_PADDING_TOP_LOGICAL_PX
            + SECTION_LABEL_LINE_LOGICAL_PX
            + SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX)
            * scale)
            .round();
        separator + heading + (ITEM_HEIGHT_LOGICAL_PX * scale).round() * rows as f32
    }

    /// PIN — the menu hangs 4px under the button that opened it, at the button's
    /// own left edge, and it is the mock-up's 180px wide.
    #[test]
    fn the_menu_hangs_under_its_button_at_the_mockup_s_own_width() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let button = anchor(scale);
            let layout = layout(
                button,
                MenuSide::Below,
                (960.0 * scale, 600.0),
                scale,
                NO_RECENT,
            );
            let frame = layout.frame;
            assert_eq!(
                frame[1],
                (button[3] + 4.0 * scale).round(),
                "scale {scale}: the menu sits 4px under the button"
            );
            assert_eq!(
                frame[0],
                button[0].round(),
                "scale {scale}: the menu's left edge is the button's, on a whole pixel"
            );
            assert_eq!(
                (frame[2] - frame[0]).round(),
                (180.0 * scale).round(),
                "scale {scale}: `.profile-menu` is 180px wide"
            );
            assert_eq!(layout.items.len(), PROFILES.len());
        }
    }

    /// PIN — beside the rail's button, not under it, and the bug that asked.
    ///
    /// A real window in rail mode opened the picker adrift in the middle of the
    /// terminal, because the anchor was still read out of the *horizontal*
    /// strip's geometry — a pure function of a width and a trailer list, which
    /// goes on answering with a box in the title bar long after the tabs have
    /// moved down the side.
    ///
    /// With the rail's own box, "under and left" is still wrong: that is the
    /// rail's own column, 46px of it while parked. Beside, then, and level with
    /// the button's top — and, because Q181 collapses the `˅` while the rail is
    /// parked so the `+` is the anchor there instead, the placement is written
    /// so those two boxes give the same answer. They share a right edge and a
    /// top by construction, so the menu does not jump when the panel slides open
    /// and the chevron comes back.
    #[test]
    fn beside_the_rail_the_menu_clears_its_button_rather_than_hanging_down_it() {
        let scale = 1.0;
        // A 220px rail's `+` row, and the `˅` that stands at its right end:
        // `new_tab` is 173 wide, a 2px gap, then a 28px chevron (Q181).
        let plus = [8.0_f32, 400.0, 181.0, 430.0];
        let chevron = [183.0_f32, 400.0, 211.0, 430.0];

        let open = layout(chevron, MenuSide::Beside, (1400.0, 900.0), scale, NO_RECENT);
        assert_eq!(
            open.frame[0],
            (chevron[2] + 4.0 * scale).round(),
            "the menu stands clear of the chevron's right edge, not under it"
        );
        assert_eq!(
            open.frame[1], chevron[1],
            "and level with its top rather than below its bottom"
        );

        let parked = layout(plus, MenuSide::Beside, (1400.0, 900.0), scale, NO_RECENT);
        assert_eq!(
            parked.frame[1], open.frame[1],
            "a parked rail anchors on the `+` instead, and the two share a top, \
             so the menu does not jump as the panel opens"
        );

        // The `Below` placement is still the strip's own, and still different.
        let strip = layout(chevron, MenuSide::Below, (1400.0, 900.0), scale, NO_RECENT);
        assert_eq!(strip.frame[0], chevron[0].round());
        assert_eq!(strip.frame[1], (chevron[3] + 4.0 * scale).round());
    }

    /// PIN — a menu beside a button near the window's foot is pushed back up
    /// rather than hanging out of it. The `Below` placement never needed this —
    /// it only ever hangs off the title bar — which is why the clamp arrived
    /// with the rail.
    #[test]
    fn a_menu_beside_a_low_button_stays_inside_the_window() {
        let scale = 1.0;
        let surface = (1400.0_f32, 500.0_f32);
        let low = layout(
            [8.0, 470.0, 211.0, 500.0],
            MenuSide::Beside,
            surface,
            scale,
            NO_RECENT,
        );
        assert!(
            low.frame[3] <= surface.1 - 8.0 + 0.001,
            "the menu ran past the window's foot: {:?}",
            low.frame
        );
        assert!(low.frame[1] >= 0.0, "{:?}", low.frame);
    }

    /// PIN — the list is what this build can actually launch, and no more.
    #[test]
    fn the_picker_offers_exactly_the_profiles_this_build_has() {
        assert_eq!(PROFILES.len(), 1);
        assert_eq!(PROFILES[DEFAULT_PROFILE].title, "PowerShell");
        assert_eq!(
            PROFILES[DEFAULT_PROFILE].mark,
            ChromeMark::ProfilePowerShell,
            "a profile's icon is its mark"
        );
    }

    /// PIN — a popup is not a modal: a point off the menu belongs to nobody
    /// here, and a point on the menu's own body is not a row.
    ///
    /// Red gate: returning `Some(None)` for everything (the modal shape) would
    /// swallow every press in the window while the picker is up.
    #[test]
    fn the_menu_claims_its_own_box_and_nothing_else() {
        let scale = 1.0;
        let button = anchor(scale);
        let layout = layout(button, MenuSide::Below, (960.0, 600.0), scale, NO_RECENT);
        let frame = layout.frame;
        let item = layout.items[0];
        assert_eq!(
            hit(
                &layout,
                f64::from((item[0] + item[2]) / 2.0),
                f64::from((item[1] + item[3]) / 2.0)
            ),
            Some(Some(MenuRow::Profile(0)))
        );
        assert_eq!(
            hit(
                &layout,
                f64::from(frame[0] + 1.0),
                f64::from(frame[3] - 1.0)
            ),
            Some(None),
            "the menu's own padding is the menu's, not a row's"
        );
        assert_eq!(
            hit(
                &layout,
                f64::from(frame[0] - 4.0),
                f64::from(frame[1] + 4.0)
            ),
            None,
            "beside the menu belongs to whatever is there"
        );
        assert_eq!(hit(&layout, 400.0, 500.0), None);
    }

    /// PIN — the menu is pushed off the window's right edge by no more than the
    /// mock-up's own 8px margin, however near that edge the button sits.
    #[test]
    fn a_menu_opened_near_the_right_edge_stays_inside_the_window() {
        let scale = 1.0;
        let surface = 300.0;
        let layout = layout(
            [260.0, 9.0, 288.0, 37.0],
            MenuSide::Below,
            (surface, 600.0),
            scale,
            NO_RECENT,
        );
        let frame = layout.frame;
        assert!(
            frame[2] <= surface - 8.0,
            "the menu ran past the window edge: {frame:?}"
        );
        assert!(frame[0] >= 0.0);
    }

    /// PIN — hover is a fact about an open menu. A stale row cannot stay lit
    /// under a menu that is no longer there.
    #[test]
    fn hover_belongs_to_an_open_menu_only() {
        let mut menu = ProfileMenu::default();
        assert!(
            !menu.set_hover(Some(MenuRow::Profile(0))),
            "a shut menu has no hovered row"
        );
        assert_eq!(menu.hover(), None);
        menu.toggle();
        assert!(menu.is_open());
        assert!(menu.set_hover(Some(MenuRow::Profile(0))));
        assert_eq!(menu.hover(), Some(MenuRow::Profile(0)));
        assert!(
            menu.set_hover(Some(MenuRow::Recent(0))),
            "row 0 of the other list is a different row"
        );
        assert_eq!(menu.hover(), Some(MenuRow::Recent(0)));
        assert!(menu.close());
        assert_eq!(menu.hover(), None);
        assert!(!menu.close(), "closing a shut menu consumes nothing");
    }

    /// PIN — the hovered row wears `--ink` on `--hover`, and the resting one
    /// `--ink2` on nothing; the row also carries its profile's own mark.
    #[test]
    fn a_hovered_row_lights_up_and_every_row_wears_its_profile_s_mark() {
        let scale = 1.0;
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            NO_RECENT,
        );
        let palette = chrome_palette();
        let rest = one_layer(build(&layout, None, NO_RECENT, now()));
        let hover = one_layer(build(&layout, Some(MenuRow::Profile(0)), NO_RECENT, now()));
        let (rest_quads, rest_labels, sprites) = (rest.quads, rest.labels, rest.sprites);
        let (hover_quads, hover_labels) = (hover.quads, hover.labels);
        assert!(
            sprites
                .iter()
                .any(|sprite| sprite.mark == ChromeMark::ProfilePowerShell)
        );
        assert!(
            rest_labels
                .iter()
                .any(|label| label.text == "PowerShell" && label.color == palette.menu_item_text)
        );
        assert!(
            hover_labels.iter().any(|label| label.text == "PowerShell"
                && label.color == palette.menu_item_text_selected)
        );
        assert!(
            hover_quads.len() > rest_quads.len(),
            "the hovered row must add a fill"
        );
        assert!(
            hover_quads
                .iter()
                .any(|quad| quad.color == palette.menu_item_hover),
            "and that fill is `--hover` over `--menu`"
        );
        assert!(
            rest_labels.iter().any(|label| label.text == HINT_TEXT),
            "the default profile says so"
        );
    }

    /// PIN — I89/I90/I93/I95: every measured value of `.profile-menu` and
    /// `.profile-item` (mock-up lines 976-1002), nailed to the stylesheet.
    ///
    /// The surface, its rows and its ink are checked elsewhere in this module;
    /// what this pins is the ruler — the numbers a redesign would have to change
    /// deliberately rather than drift past.
    #[test]
    fn the_menu_measures_what_the_stylesheet_says_it_measures() {
        assert_eq!(MENU_MIN_WIDTH_LOGICAL_PX, 180.0, "min-width: 180px");
        assert_eq!(MENU_RADIUS_LOGICAL_PX, 8.0, "border-radius: 8px");
        assert_eq!(MENU_PADDING_LOGICAL_PX, 4.0, "padding: 4px");
        assert_eq!(MENU_OFFSET_LOGICAL_PX, 4.0, "top = anchor.bottom + 4");
        assert_eq!(MENU_EDGE_MARGIN_LOGICAL_PX, 8.0, "win.width - mw - 8");
        assert_eq!(ITEM_RADIUS_LOGICAL_PX, 5.0, ".profile-item radius 5px");
        assert_eq!(ITEM_PADDING_X_LOGICAL_PX, 10.0, "padding: 7px 10px");
        assert_eq!(ITEM_GAP_LOGICAL_PX, 10.0, "gap: 10px");
        assert_eq!(ITEM_FONT_LOGICAL_PX, 13.0, "font-size: 13px");
        assert_eq!(
            ITEM_ICON_COLUMN_LOGICAL_PX, 14.0,
            ".ticon {{ width: 14px }}"
        );
        assert_eq!(HINT_FONT_LOGICAL_PX, 11.0, ".default-hint font-size 11px");
        // 7 + 15.5 + 7: the 13px line box the mock-up's own renderer produces,
        // inside the row's vertical padding.
        assert_eq!(ITEM_HEIGHT_LOGICAL_PX, 29.5);

        // I92, the Recent section (mock-up lines 996-1002).
        assert_eq!(SEPARATOR_THICKNESS_LOGICAL_PX, 1.0, ".menu-sep height 1px");
        assert_eq!(SEPARATOR_MARGIN_Y_LOGICAL_PX, 5.0, ".menu-sep margin 5px 0");
        assert_eq!(
            SEPARATOR_ALPHA_ON_DARK, 0.06,
            "--border-soft rgba(255,255,255,.06)"
        );
        assert_eq!(
            SEPARATOR_ALPHA_ON_LIGHT, 0.055,
            "--border-soft rgba(0,0,0,.055)"
        );
        assert_eq!(
            SECTION_LABEL_FONT_LOGICAL_PX, 10.5,
            ".menu-label font-size 10.5px"
        );
        assert_eq!(
            SECTION_LABEL_TRACKING_EM, 0.05,
            ".menu-label letter-spacing .05em"
        );
        // 3 + 12.5 + 5: the 10.5px line box the mock-up's own renderer produces,
        // inside `padding: 3px 10px 5px`.
        assert_eq!(SECTION_LABEL_PADDING_TOP_LOGICAL_PX, 3.0);
        assert_eq!(SECTION_LABEL_PADDING_X_LOGICAL_PX, 10.0);
        assert_eq!(SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX, 5.0);
        assert_eq!(SECTION_LABEL_LINE_LOGICAL_PX, 12.5);
        assert_eq!(
            RECENT_ITEM_MAX_WIDTH_LOGICAL_PX, 260.0,
            ".recent-item max-width 260px"
        );
        assert_eq!(
            RECENT_SECTION_LABEL, "RECENTLY OPENED",
            "`Recently opened` under `text-transform: uppercase`"
        );

        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let layout = layout(
                anchor(scale),
                MenuSide::Below,
                (960.0 * scale, 600.0),
                scale,
                NO_RECENT,
            );
            let item = layout.items[0];
            assert_eq!(
                (item[3] - item[1]).round(),
                (ITEM_HEIGHT_LOGICAL_PX * scale).round(),
                "scale {scale}: a row is its own height"
            );
            // `padding: 4px` inside a 1px border: the row is inset from the
            // menu's edge by both.
            let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
            assert_eq!(
                item[0] - layout.frame[0],
                border + MENU_PADDING_LOGICAL_PX * scale,
                "scale {scale}: the menu's own padding sits outside its rows"
            );
            assert_eq!(layout.frame[2] - item[2], item[0] - layout.frame[0]);
        }
    }

    /// PIN — I93: the `default` hint is `--ink3` over `--menu`, and the mark
    /// column is the mock-up's 14px with its 15px mark centred on it.
    ///
    /// Red gate: the hint used to wear `dialog_muted_text` — the same ink
    /// composited over `--win`, the settings dialog's surface. The two agree in
    /// the light theme and part by six levels in the dark, which is exactly the
    /// kind of error that survives a light-theme review.
    #[test]
    fn the_default_hint_is_inked_for_a_menu_and_not_for_a_dialog() {
        let scale = 1.0;
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            NO_RECENT,
        );
        let palette = chrome_palette();
        let layers = build(&layout, None, NO_RECENT, now());
        let labels: Vec<_> = layers.iter().flat_map(|layer| &layer.labels).collect();
        let sprites: Vec<_> = layers.iter().flat_map(|layer| &layer.sprites).collect();
        let hint = labels
            .iter()
            .find(|label| label.text == HINT_TEXT)
            .expect("the default profile says so");
        assert_eq!(hint.color, palette.menu_item_hint_text);
        assert_eq!(hint.font_size_px, HINT_FONT_LOGICAL_PX * scale);
        assert!(
            hint.align_right,
            "`margin-left: auto` puts it against the row's trailing padding"
        );
        assert_eq!(
            hint.rect[2],
            layout.items[0][2] - ITEM_PADDING_X_LOGICAL_PX * scale,
            "and that padding is the row's own 10px"
        );
        // The 15px mark, centred on its 14px column — what a flex box does with
        // a child one pixel wider than its box.
        let mark = sprites
            .iter()
            .find(|sprite| sprite.mark == ChromeMark::ProfilePowerShell)
            .expect("every row wears its profile's mark");
        assert_eq!(mark.rect[2] - mark.rect[0], ITEM_MARK_LOGICAL_PX * scale);
        let column_left = layout.items[0][0] + ITEM_PADDING_X_LOGICAL_PX * scale;
        let column_mid = column_left + ITEM_ICON_COLUMN_LOGICAL_PX * scale / 2.0;
        assert!(
            ((mark.rect[0] + mark.rect[2]) / 2.0 - column_mid).abs() <= 0.5,
            "the mark is centred on its column, not aligned to it"
        );
        // And the row's own label clears the column plus the row's 10px gap.
        let title = labels
            .iter()
            .find(|label| label.text == "PowerShell")
            .expect("the row is named");
        assert_eq!(
            title.rect[0],
            column_left + ITEM_ICON_COLUMN_LOGICAL_PX * scale + ITEM_GAP_LOGICAL_PX * scale
        );
        assert_eq!(title.font_size_px, ITEM_FONT_LOGICAL_PX * scale);
    }

    /// PIN — I92, mock-up 7311: `state.recent.length ? … : ""`. An empty vault
    /// adds no rule, no heading and no rows, and leaves the menu at exactly the
    /// height it had before Recent existed.
    ///
    /// Red gate: a section that draws itself unconditionally — a hairline and a
    /// heading reading "RECENTLY OPENED" over nothing at all, which is chrome
    /// making a promise the menu cannot keep.
    #[test]
    fn an_empty_vault_adds_no_rule_no_heading_and_no_rows() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let layout = layout(
                anchor(scale),
                MenuSide::Below,
                (960.0 * scale, 600.0),
                scale,
                NO_RECENT,
            );
            let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
            assert_eq!(
                layout.frame[3] - layout.frame[1],
                (2.0 * (border + MENU_PADDING_LOGICAL_PX * scale)
                    + (ITEM_HEIGHT_LOGICAL_PX * scale).round() * PROFILES.len() as f32)
                    .round(),
                "scale {scale}: the profiles and the menu's own padding, and nothing else"
            );
            assert_eq!(layout.separator, None);
            assert_eq!(layout.section_label, None);
            assert!(layout.recent.is_empty());

            let layer = one_layer(build(&layout, None, NO_RECENT, now()));
            assert!(
                !layer
                    .labels
                    .iter()
                    .any(|label| label.text == RECENT_SECTION_LABEL),
                "scale {scale}: no heading over an empty list"
            );
            assert_eq!(
                layer.sprites.len(),
                PROFILES.len(),
                "scale {scale}: one mark per profile row and no more"
            );
        }
    }

    /// PIN — the Recent section is `.menu-sep` (1px between two 5px margins),
    /// `.menu-label` (3 + the 10.5px line box + 5) and one 29.5px row per seed,
    /// in that order, inside the menu's own padding.
    #[test]
    fn the_recent_section_is_a_rule_a_heading_and_one_row_for_each_seed() {
        let vault = [term("C:\\repo", None, 0), files("D:\\notes", 600)];
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let empty = layout(
                anchor(scale),
                MenuSide::Below,
                (960.0 * scale, 600.0),
                scale,
                NO_RECENT,
            );
            let full = layout(
                anchor(scale),
                MenuSide::Below,
                (960.0 * scale, 600.0),
                scale,
                &vault,
            );
            assert_eq!(
                (full.frame[3] - full.frame[1]) - (empty.frame[3] - empty.frame[1]),
                recent_block(scale, vault.len()),
                "scale {scale}: the section's own three blocks and nothing more"
            );

            let rule = full.separator.expect("a filled vault is separated");
            let band = full.section_label.expect("and titled");
            let last_profile = *full.items.last().expect("the profile list");
            assert_eq!(
                rule[1] - last_profile[3],
                (SEPARATOR_MARGIN_Y_LOGICAL_PX * scale).round(),
                "scale {scale}: `margin: 5px 0` above the rule"
            );
            assert_eq!(
                rule[3] - rule[1],
                (SEPARATOR_THICKNESS_LOGICAL_PX * scale).round().max(1.0),
                "scale {scale}: a rule of whole pixels, never rounded away to nothing"
            );
            assert_eq!(
                band[1] - rule[3],
                (SEPARATOR_MARGIN_Y_LOGICAL_PX * scale).round(),
                "scale {scale}: and 5px below it"
            );
            assert_eq!(
                band[3] - band[1],
                ((SECTION_LABEL_PADDING_TOP_LOGICAL_PX
                    + SECTION_LABEL_LINE_LOGICAL_PX
                    + SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX)
                    * scale)
                    .round(),
                "scale {scale}: `padding: 3px 10px 5px` around a 12.5px line box"
            );
            assert_eq!(rule[0], last_profile[0], "the rule spans the row's own box");
            assert_eq!(rule[2], last_profile[2]);

            assert_eq!(full.recent.len(), vault.len());
            assert_eq!(
                full.recent[0][1], band[3],
                "the first row follows the heading"
            );
            for row in &full.recent {
                assert_eq!(
                    row[3] - row[1],
                    (ITEM_HEIGHT_LOGICAL_PX * scale).round(),
                    "scale {scale}: a recent row is a `.profile-item`"
                );
                assert_eq!(row[0], last_profile[0]);
                assert!(
                    row[2] - row[0] <= (RECENT_ITEM_MAX_WIDTH_LOGICAL_PX * scale).round(),
                    "scale {scale}: `.recent-item {{ max-width: 260px }}`"
                );
            }
        }
    }

    /// PIN — a press on a recent row is that recent row, by the vault's own
    /// index, and never a profile.
    ///
    /// Red gate: the menu's rows used to be one untagged `usize` indexed
    /// straight into [`PROFILES`]. With a Recent section under them that number
    /// names two different things, and the bug it produces is silent — clicking
    /// the third recent seed launches a bare PowerShell in the wrong folder and
    /// looks, from the outside, exactly like the menu working.
    #[test]
    fn a_press_on_a_recent_row_is_that_seed_and_never_a_profile() {
        let scale = 1.0;
        let vault = [
            term("C:\\a", None, 0),
            term("C:\\b", None, 60),
            files("C:\\c", 120),
        ];
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            &vault,
        );
        let centre = |rect: [f32; 4]| {
            (
                f64::from((rect[0] + rect[2]) / 2.0),
                f64::from((rect[1] + rect[3]) / 2.0),
            )
        };
        for index in 0..vault.len() {
            let (x, y) = centre(layout.recent[index]);
            assert_eq!(
                hit(&layout, x, y),
                Some(Some(MenuRow::Recent(index))),
                "recent row {index} must answer with its own index in its own list"
            );
        }
        let (x, y) = centre(layout.items[0]);
        assert_eq!(hit(&layout, x, y), Some(Some(MenuRow::Profile(0))));

        // The rule and the heading name nothing you can open, so they are the
        // menu's body — a press there does nothing rather than something.
        let rule = layout.separator.expect("separated");
        let band = layout.section_label.expect("titled");
        for rect in [rule, band] {
            let (x, y) = centre(rect);
            assert_eq!(hit(&layout, x, y), Some(None));
        }
    }

    /// PIN — the menu shows at most the eight seeds the vault itself keeps
    /// (`docs/DESIGN.md` §7.1.4, mock-up 4056), whatever it is handed.
    ///
    /// Red gate: a menu whose height is "however many the caller passed" is a
    /// popup that grows off the bottom of the window, and every row past the
    /// edge is a row you can neither see nor click.
    #[test]
    fn the_menu_draws_at_most_the_eight_seeds_the_vault_keeps() {
        let scale = 1.0;
        let vault: Vec<RecentEntry> = (0..12)
            .map(|index| term(&format!("C:\\p{index}"), None, index * 60))
            .collect();
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            &vault,
        );
        assert_eq!(RECENT_CAPACITY, 8, "the vault's own cap, not a second one");
        assert_eq!(layout.recent.len(), RECENT_CAPACITY);
        assert_eq!(
            layout.frame[3] - layout.frame[1],
            (2.0 * ((FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0)
                + MENU_PADDING_LOGICAL_PX * scale)
                + (ITEM_HEIGHT_LOGICAL_PX * scale).round() * PROFILES.len() as f32
                + recent_block(scale, RECENT_CAPACITY))
            .round(),
            "and the menu is only as tall as the rows it draws"
        );

        let layer = one_layer(build(&layout, None, &vault, now()));
        assert!(
            layer.labels.iter().any(|label| label.text == "p7"),
            "the eighth seed is drawn"
        );
        assert!(
            !layer.labels.iter().any(|label| label.text == "p8"),
            "the ninth is not"
        );
        assert_eq!(
            layer.sprites.len(),
            PROFILES.len() + RECENT_CAPACITY,
            "one mark per drawn row"
        );
    }

    /// PIN — `.menu-label` is the settings dialog's group-heading craft on the
    /// menu's own surface: 10.5px, `600`, `.05em` tracked, `--ink3` over
    /// `--menu`, and drawn uppercase because `text-transform` has no renderer
    /// here.
    #[test]
    fn the_recent_heading_is_uppercase_tracked_and_inked_for_a_menu() {
        let scale = 1.0;
        let vault = [term("C:\\repo", None, 0)];
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            &vault,
        );
        let palette = chrome_palette();
        let layer = one_layer(build(&layout, None, &vault, now()));
        let heading = layer
            .labels
            .iter()
            .find(|label| label.text == RECENT_SECTION_LABEL)
            .expect("the section is titled");
        assert_eq!(heading.font_size_px, SECTION_LABEL_FONT_LOGICAL_PX * scale);
        assert_eq!(heading.letter_spacing_em, SECTION_LABEL_TRACKING_EM);
        assert_eq!(heading.weight, ChromeLabelWeight::SemiBold);
        assert_eq!(
            heading.color, palette.menu_item_hint_text,
            "`--ink3` over `--menu`, not the dialog's same-named ink"
        );
        assert!(!heading.align_right && !heading.align_center);

        let band = layout.section_label.expect("titled");
        assert_eq!(
            heading.rect[0],
            band[0] + SECTION_LABEL_PADDING_X_LOGICAL_PX * scale,
            "`padding: … 10px …`"
        );
        assert_eq!(
            heading.rect[1],
            band[1] + SECTION_LABEL_PADDING_TOP_LOGICAL_PX * scale,
            "3px above"
        );
        assert_eq!(
            heading.rect[3],
            band[3] - SECTION_LABEL_PADDING_BOTTOM_LOGICAL_PX * scale,
            "5px below, so the line box is centred in its own height"
        );

        // `--border-soft` is the same ink as the menu's own hairline at a
        // lighter alpha, and the two themes declare that alpha separately.
        let rule = layout.separator.expect("separated");
        let hairline = layer
            .quads
            .iter()
            .find(|quad| quad.rect == rule)
            .expect("the rule is drawn");
        assert_eq!(hairline.color, palette.menu_border);
        assert_eq!(hairline.alpha, separator_alpha(palette.menu_border));
        assert_eq!(separator_alpha([0xff, 0xff, 0xff]), SEPARATOR_ALPHA_ON_DARK);
        assert_eq!(
            separator_alpha([0x00, 0x00, 0x00]),
            SEPARATOR_ALPHA_ON_LIGHT
        );
    }

    /// PIN — mock-up 7318: a recent row is called by your own name for it, and
    /// by the folder it stood in when you never gave it one. The leaf rule is
    /// drive-root aware, so `C:\` is `C:` rather than the empty caption a naive
    /// split leaves behind a trailing separator.
    #[test]
    fn a_recent_row_wears_your_name_for_it_or_the_folder_it_stood_in() {
        assert_eq!(cwd_leaf("C:\\Users\\Weiyi\\repo"), "repo");
        assert_eq!(cwd_leaf("C:\\Users\\Weiyi\\repo\\"), "repo");
        assert_eq!(cwd_leaf("C:\\"), "C:", "a drive root names its drive");
        assert_eq!(cwd_leaf("C:"), "C:");
        assert_eq!(
            cwd_leaf("/home/weiyi/src"),
            "src",
            "and forward slashes too"
        );

        let vault = [
            term("C:\\Users\\Weiyi\\repo", Some("build"), 0),
            term("C:\\Users\\Weiyi\\notes", None, 60),
            // `||` in the mock-up falls through an empty string: a row captioned
            // with nothing is a row you cannot tell from the one above it.
            term("C:\\Users\\Weiyi\\empty", Some(""), 120),
            files("D:\\Developer\\BetterTerminal\\", 180),
        ];
        let layout = layout(anchor(1.0), MenuSide::Below, (960.0, 600.0), 1.0, &vault);
        let layer = one_layer(build(&layout, None, &vault, now()));
        let drawn: Vec<&str> = layer
            .labels
            .iter()
            .map(|label| label.text.as_str())
            .collect();
        for name in ["build", "notes", "empty", "BetterTerminal"] {
            assert!(drawn.contains(&name), "{name} is missing from {drawn:?}");
        }
    }

    /// PIN — mock-up 7314/7318: a terminal seed wears its own profile's mark,
    /// a files locus wears `#i-folder`, and both are `--accent`. The ago label
    /// rides in the `.default-hint` slot the `default` hint already owns.
    #[test]
    fn a_files_seed_wears_the_folder_and_a_terminal_seed_wears_its_profile_s_mark() {
        let scale = 1.0;
        let vault = [files("D:\\notes", 0), term("C:\\repo", None, 3 * 3600)];
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            &vault,
        );
        let palette = chrome_palette();
        let layer = one_layer(build(&layout, None, &vault, now()));

        let in_row = |row: [f32; 4], sprite: &ChromeSprite| {
            sprite.rect[1] >= row[1] && sprite.rect[3] <= row[3]
        };
        let folder = layer
            .sprites
            .iter()
            .find(|sprite| in_row(layout.recent[0], sprite))
            .expect("the files row wears a mark");
        assert_eq!(folder.mark, ChromeMark::Folder);
        assert_eq!(folder.color, palette.accent);
        let shell = layer
            .sprites
            .iter()
            .find(|sprite| in_row(layout.recent[1], sprite))
            .expect("the terminal row wears a mark");
        assert_eq!(shell.mark, PROFILES[DEFAULT_PROFILE].mark);
        assert_eq!(shell.color, palette.accent);
        // An id this build does not have costs the row its shell choice, never
        // its mark — `index_of_id` falls back rather than refusing.
        assert_eq!(
            recent_mark(&Seed::Term {
                profile_id: "a-shell-from-a-newer-build".to_owned(),
                cwd: "C:\\repo".to_owned(),
                manual_name: None,
            }),
            PROFILES[DEFAULT_PROFILE].mark
        );

        let hint = layer
            .labels
            .iter()
            .find(|label| label.text == "3h ago")
            .expect("a recent row says how long ago");
        assert_eq!(hint.font_size_px, HINT_FONT_LOGICAL_PX * scale);
        assert_eq!(hint.color, palette.menu_item_hint_text);
        assert!(hint.align_right, "`margin-left: auto`");
        assert_eq!(
            hint.rect[2],
            layout.recent[1][2] - ITEM_PADDING_X_LOGICAL_PX * scale,
            "against the row's own trailing padding"
        );
        assert!(
            layer.labels.iter().any(|label| label.text == "just now"),
            "and the newest one says so in the mock-up's own words"
        );
    }

    /// PIN — hovering a recent row lights that row and only that row.
    ///
    /// Red gate: the untagged index again, this time in ink — `Some(0)` used to
    /// mean "the first row", so pointing at the first recent seed lit the
    /// PowerShell row at the top of the menu.
    #[test]
    fn hovering_a_recent_row_lights_it_and_leaves_the_profile_above_it_dark() {
        let scale = 1.0;
        let vault = [term("C:\\repo", Some("build"), 0)];
        let layout = layout(
            anchor(scale),
            MenuSide::Below,
            (960.0, 600.0),
            scale,
            &vault,
        );
        let palette = chrome_palette();
        let layer = one_layer(build(&layout, Some(MenuRow::Recent(0)), &vault, now()));
        let row = layout.recent[0];
        assert!(
            layer
                .quads
                .iter()
                .any(|quad| quad.color == palette.menu_item_hover
                    && quad.rect[1] >= row[1]
                    && quad.rect[3] <= row[3]),
            "the hovered recent row wears `--hover` over `--menu`"
        );
        assert!(
            layer.labels.iter().any(
                |label| label.text == "build" && label.color == palette.menu_item_text_selected
            ),
            "and steps to `--ink`"
        );
        assert!(
            layer
                .labels
                .iter()
                .any(|label| label.text == "PowerShell" && label.color == palette.menu_item_text),
            "while the profile row it is not stays `--ink2`"
        );
    }
}
