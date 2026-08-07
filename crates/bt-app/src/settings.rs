//! The settings dialog — the modal the gear opens.
//!
//! Spec authority is `design/ui-mockup.html`'s `#settings-overlay` block: the
//! `.overlay` scrim, the `.settings` dialog on it, its `header`/`h1`/`.dlg-close`,
//! and the `.content` rows inside. Every number below is that stylesheet's own,
//! and the ones that are line boxes rather than declarations were measured in the
//! mock-up's own renderer rather than guessed from a line-height factor — a box
//! model derived from *our* font would drift the moment the font did, and the
//! design is the thing being reproduced.
//!
//! Two rulings from `docs/DESIGN.md` §7.1.5 shape the module:
//!
//! * **A modal means MODAL.** [`hit`] never returns "nothing" — every point in
//!   the window belongs to the overlay while it is up, so nothing behind the
//!   scrim can be dragged, clicked, selected or hovered. The mock-up says the
//!   same thing twice: the scrim is a real element at `z-index: 70`, and its
//!   keyboard guards are commented "a modal means MODAL: no layout edits behind
//!   the scrim".
//! * **The scrim outranks every popup and float.** That is a draw-order fact and
//!   lives in `bt-render`; what lives here is that the dialog is built out of
//!   [`bt_render::OverlayQuad`]s, which blend, because a scrim and a hairline
//!   over an unknown surface cannot be pre-composited.
//!
//! Nothing here is layout: the dialog is not a seat, takes no space from the
//! solver, and is never persisted (a dialog does not survive a restart).

use bt_render::{
    ChromeLabel, FLOAT_WINDOW_BORDER_LOGICAL_PX, FLOAT_WINDOW_RADIUS_LOGICAL_PX,
    FLOAT_WINDOW_SHADOW_LOGICAL_PX, OverlayQuad, Theme, WINDOW_CAPTION_GLYPH_LOGICAL_PX,
    chrome_palette, rounded_overlay_fill, rounded_overlay_halo,
};

use crate::marks::{ChromeMark, ChromeSprite};

// ── `.settings`, `.overlay` ────────────────────────────────────────────────
/// `.settings { width: min(480px, 92%) }` — the cap and the share.
const DIALOG_MAX_WIDTH_LOGICAL_PX: f32 = 480.0;
const DIALOG_WIDTH_RATIO: f32 = 0.92;
/// `.settings { margin: 54px auto 0 }` — the drop from the window's top, and
/// `auto` on both sides, which is what centres it.
const DIALOG_TOP_LOGICAL_PX: f32 = 54.0;
/// `.settings { max-height: calc(100% - 72px) }`, of which 54 is the margin
/// above; this is what the rule leaves below.
const DIALOG_BOTTOM_LOGICAL_PX: f32 = 18.0;

// ── `.settings header` ─────────────────────────────────────────────────────
/// `padding: 16px 12px 10px 22px` around a 30px `.dlg-close`: 16 + 30 + 10.
const HEADER_HEIGHT_LOGICAL_PX: f32 = 56.0;
const HEADER_PADDING_TOP_LOGICAL_PX: f32 = 16.0;
const HEADER_PADDING_LEFT_LOGICAL_PX: f32 = 22.0;
/// 12 rather than 22 so the *icon* inside the 30px button ends 22 from the
/// dialog's edge — the mock-up says so in as many words.
const HEADER_PADDING_RIGHT_LOGICAL_PX: f32 = 12.0;
/// `.settings header h1 { font-size: 16px; font-weight: 600 }`. Weight is not
/// expressible through a chrome label and is noted as a deviation.
const HEADER_TITLE_FONT_LOGICAL_PX: f32 = 16.0;

// ── `.dlg-close` ───────────────────────────────────────────────────────────
/// `width: 30px; height: 30px`. Deliberately not the caption run's 46x40: the
/// mock-up rules that a dialog's close is not a caption button, because a
/// caption button's hard-edged rectangle is owed to the window edge it fills.
const CLOSE_SIDE_LOGICAL_PX: f32 = 30.0;
/// `border-radius: 6px`.
const CLOSE_RADIUS_LOGICAL_PX: f32 = 6.0;

// ── `.settings .content` ───────────────────────────────────────────────────
const CONTENT_PADDING_X_LOGICAL_PX: f32 = 22.0;
const CONTENT_PADDING_TOP_LOGICAL_PX: f32 = 2.0;
const CONTENT_PADDING_BOTTOM_LOGICAL_PX: f32 = 18.0;

// ── `.group-label` ─────────────────────────────────────────────────────────
const GROUP_LABEL_FONT_LOGICAL_PX: f32 = 11.0;
/// The 11px line box, measured in the mock-up.
const GROUP_LABEL_LINE_LOGICAL_PX: f32 = 13.0;
const GROUP_LABEL_MARGIN_TOP_LOGICAL_PX: f32 = 10.0;
const GROUP_LABEL_MARGIN_BOTTOM_LOGICAL_PX: f32 = 2.0;
/// `letter-spacing: .05em` at 11px.
const GROUP_LABEL_TRACKING_EM: f32 = 0.05;

// ── `.row` ─────────────────────────────────────────────────────────────────
const ROW_PADDING_Y_LOGICAL_PX: f32 = 11.0;
const ROW_PADDING_X_LOGICAL_PX: f32 = 2.0;
const ROW_GAP_LOGICAL_PX: f32 = 16.0;
const ROW_TITLE_FONT_LOGICAL_PX: f32 = 13.5;
/// The 13.5px line box, measured in the mock-up.
const ROW_TITLE_LINE_LOGICAL_PX: f32 = 16.5;
const ROW_DESC_FONT_LOGICAL_PX: f32 = 12.0;
/// The 12px line box, measured in the mock-up.
const ROW_DESC_LINE_LOGICAL_PX: f32 = 14.5;
const ROW_DESC_MARGIN_TOP_LOGICAL_PX: f32 = 1.0;

// ── `.combo` ───────────────────────────────────────────────────────────────
const COMBO_MIN_WIDTH_LOGICAL_PX: f32 = 118.0;
/// 5 + the 13px line box (15.5) + 5 + two 1px borders.
const COMBO_HEIGHT_LOGICAL_PX: f32 = 27.5;
const COMBO_RADIUS_LOGICAL_PX: f32 = 6.0;
const COMBO_PADDING_LEFT_LOGICAL_PX: f32 = 12.0;
const COMBO_PADDING_RIGHT_LOGICAL_PX: f32 = 10.0;
const COMBO_FONT_LOGICAL_PX: f32 = 13.0;
/// `.combo .chev { font-size: 8.5px }`.
///
/// The chevron's own column is reserved at this same number: `▼` at 8.5px inks
/// 7.33px wide in the mock-up, so its em box is the tightest bound that cannot
/// cut the glyph, and a bound is all the value's own rectangle needs.
const COMBO_CHEVRON_FONT_LOGICAL_PX: f32 = 8.5;
/// `.combo > button { gap: 10px }` — between the value and the chevron.
const COMBO_GAP_LOGICAL_PX: f32 = 10.0;
/// The mock-up's chevron is the character, not the `#i-chev` symbol: a solid
/// down-pointing triangle set as text beside the value.
const COMBO_CHEVRON: &str = "\u{25bc}";

// ── `.combo-menu` / `.combo-item` ──────────────────────────────────────────
/// `border-radius: 8px` — a popup menu's own round, and deliberately not the
/// 10px every *window* that floats shares.
const MENU_RADIUS_LOGICAL_PX: f32 = 8.0;
const MENU_PADDING_LOGICAL_PX: f32 = 4.0;
/// `top: calc(100% + 4px)`.
const MENU_OFFSET_LOGICAL_PX: f32 = 4.0;
/// The mock-up's own flip test: `menu.bottom > clip.bottom - 8`.
const MENU_CLEARANCE_LOGICAL_PX: f32 = 8.0;
const ITEM_HEIGHT_LOGICAL_PX: f32 = 27.5;
const ITEM_RADIUS_LOGICAL_PX: f32 = 5.0;
const ITEM_PADDING_X_LOGICAL_PX: f32 = 10.0;
const ITEM_GAP_LOGICAL_PX: f32 = 8.0;
/// `.combo-item .tick { width: 14px; font-size: 11px }` — a fixed column, so
/// every item's text starts at the same x whether or not it is the selected one.
const TICK_WIDTH_LOGICAL_PX: f32 = 14.0;
const TICK_FONT_LOGICAL_PX: f32 = 11.0;
const TICK: &str = "\u{2713}";

/// The theme options, in the mock-up's own order.
///
/// The mock-up lists three — Light, Dark, System — and this build has two. See
/// the deviation note in the module's own delivery report: `bt_render::Theme` is
/// a two-value runtime switch with no follow-the-OS resolution behind it, and
/// offering a third entry that resolves to nothing would be a control that lies.
pub const THEME_OPTIONS: [Theme; 2] = [Theme::Light, Theme::Dark];

/// The label a theme wears in the picker, matching the mock-up's own casing.
fn theme_label(theme: Theme) -> &'static str {
    match theme {
        Theme::Light => "Light",
        Theme::Dark => "Dark",
    }
}

/// Whether the dialog is up, and what is open inside it.
///
/// App state and nothing else: it is not a seat, so the solver never sees it;
/// it is not an intent, so the session file never sees it. A dialog that
/// survived a restart would be a window that opens with a question on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SettingsPanel {
    open: bool,
    /// The theme picker's own menu. Nested state, because Esc unwinds one layer
    /// per press (§7.1.5) and "the menu is open" is the top layer.
    menu_open: bool,
    hover: Option<SettingsTarget>,
}

impl SettingsPanel {
    pub fn is_open(self) -> bool {
        self.open
    }

    pub fn menu_is_open(self) -> bool {
        self.menu_open
    }

    /// The gear: open when shut, shut when open. Closing takes the menu with it.
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.menu_open = false;
        self.hover = None;
    }

    /// Close the top-most open layer and report whether there was one — the Esc
    /// route of §7.1.5, which unwinds exactly one layer per press.
    pub fn close_one_layer(&mut self) -> bool {
        if self.menu_open {
            self.menu_open = false;
            self.hover = None;
            return true;
        }
        if self.open {
            self.open = false;
            self.hover = None;
            return true;
        }
        false
    }

    /// Shut everything, whatever was open.
    pub fn close(&mut self) {
        self.open = false;
        self.menu_open = false;
        self.hover = None;
    }

    pub fn set_menu_open(&mut self, open: bool) {
        self.menu_open = open;
    }

    /// Returns whether the hover changed, so a caller can skip a repaint.
    pub fn set_hover(&mut self, hover: Option<SettingsTarget>) -> bool {
        let changed = self.hover != hover;
        self.hover = hover;
        changed
    }

    pub fn hover(self) -> Option<SettingsTarget> {
        self.hover
    }
}

/// Something in the overlay the pointer can be over.
///
/// There is no `None`: while the dialog is up every point in the window is one
/// of these, which is the whole of what "modal" means here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTarget {
    /// The dimmed world behind the dialog. A press here closes.
    Scrim,
    /// The dialog itself, away from any control. A press here does nothing, and
    /// in particular does not close — the mock-up closes only on the scrim.
    Panel,
    Close,
    ThemeCombo,
    /// The open menu's own body, between or around its items.
    ThemeMenu,
    ThemeOption(Theme),
}

/// Every rectangle the overlay draws and hit-tests, in physical pixels of the
/// whole surface.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsLayout {
    scale: f32,
    surface: [f32; 2],
    /// The dialog's border box.
    frame: [f32; 4],
    /// The header's content box — what `align-items: center` centres things on.
    header_content: [f32; 4],
    close: [f32; 4],
    /// The `.content` padding box, which is also what content is clipped to.
    content: [f32; 4],
    group_label: [f32; 4],
    row_title: [f32; 4],
    row_desc: [f32; 4],
    combo: [f32; 4],
    /// The open menu's border box and its items, top to bottom in
    /// [`THEME_OPTIONS`] order. Empty when the menu is shut.
    menu: Option<[f32; 4]>,
    items: Vec<[f32; 4]>,
}

/// The theme a press on the overlay asks the process to switch to, if it asks
/// for one at all.
///
/// A named function rather than a `match` at the call site so the mapping from
/// "what the pointer hit" to "what `bt_render::set_theme` is told" is one thing
/// that can be stated, and pinned, without a live window.
#[must_use]
pub fn theme_requested(target: SettingsTarget) -> Option<Theme> {
    match target {
        SettingsTarget::ThemeOption(theme) => Some(theme),
        _ => None,
    }
}

fn contains(rect: [f32; 4], x: f32, y: f32) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

/// Intersect a content rectangle with the box that clips it, or drop it.
fn clipped(rect: [f32; 4], clip: [f32; 4]) -> Option<[f32; 4]> {
    let out = [
        rect[0].max(clip[0]),
        rect[1].max(clip[1]),
        rect[2].min(clip[2]),
        rect[3].min(clip[3]),
    ];
    (out[2] > out[0] && out[3] > out[1]).then_some(out)
}

/// Where every part of the dialog lands in a window this size, or `None` when
/// the window cannot host it.
///
/// `None` is a real answer, not a failure: `max-height: calc(100% - 72px)` can
/// go to nothing, and a scrim over a dialog that is not there would be a window
/// nobody can use. The runtime treats it as "not open", so no input is trapped
/// behind an invisible modal.
#[must_use]
pub fn layout(
    surface_width: f32,
    surface_height: f32,
    scale: f32,
    menu_open: bool,
) -> Option<SettingsLayout> {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let width = px(DIALOG_MAX_WIDTH_LOGICAL_PX)
        .min(surface_width * DIALOG_WIDTH_RATIO)
        .round();
    let top = px(DIALOG_TOP_LOGICAL_PX).round();
    let available = (surface_height - top - px(DIALOG_BOTTOM_LOGICAL_PX)).round();
    // `.row .text` is two stacked line boxes; the row is as tall as the taller
    // of that column and the control beside it, which is what `align-items:
    // center` on a flex row resolves to.
    let text_height =
        px(ROW_TITLE_LINE_LOGICAL_PX + ROW_DESC_MARGIN_TOP_LOGICAL_PX + ROW_DESC_LINE_LOGICAL_PX);
    let row_height =
        2.0 * px(ROW_PADDING_Y_LOGICAL_PX) + text_height.max(px(COMBO_HEIGHT_LOGICAL_PX));
    let content_height = px(CONTENT_PADDING_TOP_LOGICAL_PX)
        + px(GROUP_LABEL_MARGIN_TOP_LOGICAL_PX)
        + px(GROUP_LABEL_LINE_LOGICAL_PX)
        + px(GROUP_LABEL_MARGIN_BOTTOM_LOGICAL_PX)
        + row_height
        + px(CONTENT_PADDING_BOTTOM_LOGICAL_PX);
    let header = px(HEADER_HEIGHT_LOGICAL_PX);
    let height = (2.0 * border + header + content_height)
        .min(available)
        .round();
    // Below the header plus its own two borders there is no dialog left to draw,
    // only a lid — and a lid with no body is not the design's dialog.
    if width < px(COMBO_MIN_WIDTH_LOGICAL_PX) || height <= 2.0 * border + header {
        return None;
    }
    let left = ((surface_width - width) / 2.0).round();
    let frame = [left, top, left + width, top + height];
    let inner = [
        frame[0] + border,
        frame[1] + border,
        frame[2] - border,
        frame[3] - border,
    ];
    let header_content = [
        inner[0] + px(HEADER_PADDING_LEFT_LOGICAL_PX),
        inner[1] + px(HEADER_PADDING_TOP_LOGICAL_PX),
        inner[2] - px(HEADER_PADDING_RIGHT_LOGICAL_PX),
        inner[1] + px(HEADER_PADDING_TOP_LOGICAL_PX) + px(CLOSE_SIDE_LOGICAL_PX),
    ];
    // Only the dialog's own frame is snapped to the pixel grid; everything
    // inside it is the design's exact geometry off that frame. Rounding a box's
    // two edges independently is how a 27.5px control becomes a 28px one, and
    // the shapes snap themselves at draw time anyway.
    let close_side = px(CLOSE_SIDE_LOGICAL_PX);
    let close = [
        header_content[2] - close_side,
        header_content[1],
        header_content[2],
        header_content[1] + close_side,
    ];
    let content = [
        inner[0],
        inner[1] + header,
        inner[2],
        inner[3].max(inner[1] + header),
    ];
    let text_left = content[0] + px(CONTENT_PADDING_X_LOGICAL_PX);
    let text_right = content[2] - px(CONTENT_PADDING_X_LOGICAL_PX);
    let group_top =
        content[1] + px(CONTENT_PADDING_TOP_LOGICAL_PX + GROUP_LABEL_MARGIN_TOP_LOGICAL_PX);
    let group_label = [
        text_left,
        group_top,
        text_right,
        group_top + px(GROUP_LABEL_LINE_LOGICAL_PX),
    ];
    let row_top = group_label[3] + px(GROUP_LABEL_MARGIN_BOTTOM_LOGICAL_PX);
    let row_content_top = row_top + px(ROW_PADDING_Y_LOGICAL_PX);
    let row_content_height = text_height.max(px(COMBO_HEIGHT_LOGICAL_PX));
    let row_left = text_left + px(ROW_PADDING_X_LOGICAL_PX);
    let row_right = text_right - px(ROW_PADDING_X_LOGICAL_PX);
    let combo_width = px(COMBO_MIN_WIDTH_LOGICAL_PX);
    let combo_height = px(COMBO_HEIGHT_LOGICAL_PX);
    let combo_top = row_content_top + (row_content_height - combo_height) / 2.0;
    let combo = [
        row_right - combo_width,
        combo_top,
        row_right,
        combo_top + combo_height,
    ];
    // `.row .text` is `flex: 1` beside a `flex: none` control, one gap apart.
    let text_column_right = combo[0] - px(ROW_GAP_LOGICAL_PX);
    let row_title = [
        row_left,
        row_content_top,
        text_column_right,
        row_content_top + px(ROW_TITLE_LINE_LOGICAL_PX),
    ];
    let row_desc = [
        row_left,
        row_title[3] + px(ROW_DESC_MARGIN_TOP_LOGICAL_PX),
        text_column_right,
        row_title[3] + px(ROW_DESC_MARGIN_TOP_LOGICAL_PX + ROW_DESC_LINE_LOGICAL_PX),
    ];
    let (menu, items) = if menu_open {
        menu_layout(combo, surface_height, scale, border)
    } else {
        (None, Vec::new())
    };
    Some(SettingsLayout {
        scale,
        surface: [surface_width, surface_height],
        frame,
        header_content,
        close,
        content,
        group_label,
        row_title,
        row_desc,
        combo,
        menu,
        items,
    })
}

/// The theme picker's popup: `min-width: 100%` and `right: 0` off the button,
/// one `MENU_OFFSET` below it, flipped above when it would spill.
///
/// What it is measured against is the *window*, not the dialog's `.content`.
/// The mock-up's own rule is "whatever actually clips it — its scroll container
/// if it has one, else the window", and with one row the content does not
/// scroll and therefore clips nothing. Measured against `.content` anyway, this
/// menu flips up into the header and gets its first item cut off — which the
/// prototype does, and which is the artefact that rule exists to avoid.
fn menu_layout(
    combo: [f32; 4],
    surface_height: f32,
    scale: f32,
    border: f32,
) -> (Option<[f32; 4]>, Vec<[f32; 4]>) {
    let px = |value: f32| value * scale;
    let width = combo[2] - combo[0];
    let height = 2.0 * border
        + 2.0 * px(MENU_PADDING_LOGICAL_PX)
        + THEME_OPTIONS.len() as f32 * px(ITEM_HEIGHT_LOGICAL_PX);
    let below = combo[3] + px(MENU_OFFSET_LOGICAL_PX);
    let top = if below + height > surface_height - px(MENU_CLEARANCE_LOGICAL_PX) {
        combo[1] - px(MENU_OFFSET_LOGICAL_PX) - height
    } else {
        below
    };
    let frame = [combo[0], top, combo[0] + width, top + height];
    let item_left = frame[0] + border + px(MENU_PADDING_LOGICAL_PX);
    let item_right = frame[2] - border - px(MENU_PADDING_LOGICAL_PX);
    let item_height = px(ITEM_HEIGHT_LOGICAL_PX);
    let items = (0..THEME_OPTIONS.len())
        .map(|index| {
            let item_top =
                frame[1] + border + px(MENU_PADDING_LOGICAL_PX) + index as f32 * item_height;
            [item_left, item_top, item_right, item_top + item_height]
        })
        .collect();
    (Some(frame), items)
}

/// What the pointer is over. Never `None`: a modal owns every pixel.
///
/// Smallest target first, the same ruling `seats::hit_chrome` follows — an item
/// sits inside a menu, a menu and a close button sit on the dialog, and the
/// dialog sits on the scrim.
#[must_use]
pub fn hit(layout: &SettingsLayout, x: f64, y: f64) -> SettingsTarget {
    let (x, y) = (x as f32, y as f32);
    if let Some(menu) = layout.menu {
        for (index, item) in layout.items.iter().enumerate() {
            if contains(*item, x, y) {
                return SettingsTarget::ThemeOption(THEME_OPTIONS[index]);
            }
        }
        if contains(menu, x, y) {
            return SettingsTarget::ThemeMenu;
        }
    }
    if contains(layout.close, x, y) {
        return SettingsTarget::Close;
    }
    if contains(layout.combo, x, y) {
        return SettingsTarget::ThemeCombo;
    }
    if contains(layout.frame, x, y) {
        return SettingsTarget::Panel;
    }
    SettingsTarget::Scrim
}

/// Every fill, label and mark the overlay draws, back to front.
///
/// Order matters and is not incidental: the renderer draws all of the fills,
/// then all of the marks, then all of the text, so anything that must cover a
/// fill has to be a later fill. The open menu is therefore pushed after the
/// combo it covers.
#[must_use]
pub fn build(
    layout: &SettingsLayout,
    hover: Option<SettingsTarget>,
    selected: Theme,
) -> (Vec<OverlayQuad>, Vec<ChromeLabel>, Vec<ChromeSprite>) {
    let palette = chrome_palette();
    let scale = layout.scale;
    let px = |value: f32| value * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let mut quads = Vec::new();
    let mut labels = Vec::new();
    let mut sprites = Vec::new();

    // The scrim: the whole window, dimmed. Not a rounded shape and not inset —
    // it is `position: absolute; inset: 0` on the window's own client area.
    quads.push(OverlayQuad {
        rect: [0.0, 0.0, layout.surface[0], layout.surface[1]],
        color: palette.modal_scrim,
        alpha: alpha(palette.modal_scrim_alpha),
    });

    // The dialog: the floating-window craft — lift, hairline, face — at the
    // 10px round every window that floats shares, with `--win` for its face
    // because a dialog stands on the window's plane rather than a menu's.
    push_float_window(
        &mut quads,
        layout.frame,
        px(FLOAT_WINDOW_RADIUS_LOGICAL_PX),
        border,
        px(FLOAT_WINDOW_SHADOW_LOGICAL_PX),
        palette.dialog_surface,
        palette.menu_shadow,
        alpha(palette.menu_shadow_inner_alpha),
        alpha(palette.menu_shadow_outer_alpha),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
    );

    labels.push(ChromeLabel {
        text: "Settings".to_owned(),
        rect: [
            layout.header_content[0],
            layout.header_content[1],
            layout.close[0],
            layout.header_content[3],
        ],
        font_size_px: px(HEADER_TITLE_FONT_LOGICAL_PX),
        color: palette.dialog_title_text,
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
    });

    let close_hovered = hover == Some(SettingsTarget::Close);
    if close_hovered {
        quads.extend(rounded_overlay_fill(
            layout.close,
            px(CLOSE_RADIUS_LOGICAL_PX),
            palette.dialog_hover,
            1.0,
        ));
    }
    let glyph = px(WINDOW_CAPTION_GLYPH_LOGICAL_PX).round().max(1.0);
    let glyph_left = ((layout.close[0] + layout.close[2]) / 2.0 - glyph / 2.0).round();
    let glyph_top = ((layout.close[1] + layout.close[3]) / 2.0 - glyph / 2.0).round();
    sprites.push(ChromeSprite {
        mark: ChromeMark::WindowClose,
        rect: [glyph_left, glyph_top, glyph_left + glyph, glyph_top + glyph],
        color: if close_hovered {
            palette.dialog_title_text
        } else {
            palette.dialog_secondary_text
        },
    });

    // Everything below the header is clipped to the content box, which is what
    // `max-height` plus `overflow-y` leaves when the window is too short.
    let clip = layout.content;
    if let Some(rect) = clipped(layout.group_label, clip) {
        labels.push(ChromeLabel {
            // `text-transform: uppercase` applied at the source: the chrome text
            // path has no transform, and the design's word is "Appearance".
            text: "APPEARANCE".to_owned(),
            rect,
            font_size_px: px(GROUP_LABEL_FONT_LOGICAL_PX),
            color: palette.dialog_muted_text,
            align_right: false,
            align_center: false,
            // A ratio, so it carries no `scale`: the shaper adds it to a glyph's
            // advance before the font size multiplies both.
            letter_spacing_em: GROUP_LABEL_TRACKING_EM,
        });
    }
    if let Some(rect) = clipped(layout.row_title, clip) {
        labels.push(ChromeLabel {
            text: "Theme".to_owned(),
            rect,
            font_size_px: px(ROW_TITLE_FONT_LOGICAL_PX),
            color: palette.dialog_title_text,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
        });
    }
    if let Some(rect) = clipped(layout.row_desc, clip) {
        labels.push(ChromeLabel {
            // The mock-up's own line names a third option this build does not
            // have; a description that promises what the picker cannot do is a
            // lie in the one place the user goes to find out what it does.
            text: "Light or dark".to_owned(),
            rect,
            font_size_px: px(ROW_DESC_FONT_LOGICAL_PX),
            color: palette.dialog_muted_text,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
        });
    }

    // `.combo > button`: `background: none` at rest, so its face is the dialog's
    // own; `border: 1px solid var(--border)` drawn as a border-box, the whole
    // box in the hairline's colour with the face laid one border in.
    if let Some(rect) = clipped(layout.combo, clip) {
        let combo_hovered = hover == Some(SettingsTarget::ThemeCombo);
        let radius = px(COMBO_RADIUS_LOGICAL_PX);
        quads.extend(rounded_overlay_fill(
            rect,
            radius,
            palette.menu_border,
            alpha(palette.menu_border_alpha),
        ));
        quads.extend(rounded_overlay_fill(
            [
                rect[0] + border,
                rect[1] + border,
                rect[2] - border,
                rect[3] - border,
            ],
            radius - border,
            if combo_hovered {
                palette.dialog_hover
            } else {
                palette.dialog_surface
            },
            1.0,
        ));
        // `justify-content: space-between` with a 10px gap: the value gets the
        // room the chevron and that gap do not. A chrome label clips to its own
        // rectangle, so this bound is what keeps a long value from running under
        // the chevron rather than being cut off before it.
        let chevron_column = px(COMBO_CHEVRON_FONT_LOGICAL_PX + COMBO_GAP_LOGICAL_PX);
        labels.push(ChromeLabel {
            text: theme_label(selected).to_owned(),
            rect: [
                rect[0] + border + px(COMBO_PADDING_LEFT_LOGICAL_PX),
                rect[1],
                rect[2] - border - px(COMBO_PADDING_RIGHT_LOGICAL_PX) - chevron_column,
                rect[3],
            ],
            font_size_px: px(COMBO_FONT_LOGICAL_PX),
            color: palette.dialog_title_text,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
        });
        labels.push(ChromeLabel {
            text: COMBO_CHEVRON.to_owned(),
            rect: [
                rect[0],
                rect[1],
                rect[2] - border - px(COMBO_PADDING_RIGHT_LOGICAL_PX),
                rect[3],
            ],
            font_size_px: px(COMBO_CHEVRON_FONT_LOGICAL_PX),
            color: palette.dialog_muted_text,
            align_right: true,
            align_center: false,
            letter_spacing_em: 0.0,
        });
    }

    if let Some(menu) = layout.menu {
        push_float_window(
            &mut quads,
            menu,
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
            let option = THEME_OPTIONS[index];
            let is_selected = option == selected;
            if hover == Some(SettingsTarget::ThemeOption(option)) {
                quads.extend(rounded_overlay_fill(
                    *item,
                    px(ITEM_RADIUS_LOGICAL_PX),
                    palette.menu_item_hover,
                    1.0,
                ));
            }
            let tick_left = item[0] + px(ITEM_PADDING_X_LOGICAL_PX);
            let tick_right = tick_left + px(TICK_WIDTH_LOGICAL_PX);
            if is_selected {
                labels.push(ChromeLabel {
                    text: TICK.to_owned(),
                    rect: [tick_left, item[1], tick_right, item[3]],
                    font_size_px: px(TICK_FONT_LOGICAL_PX),
                    color: palette.accent,
                    align_right: false,
                    align_center: false,
                    letter_spacing_em: 0.0,
                });
            }
            labels.push(ChromeLabel {
                text: theme_label(option).to_owned(),
                rect: [
                    tick_right + px(ITEM_GAP_LOGICAL_PX),
                    item[1],
                    item[2] - px(ITEM_PADDING_X_LOGICAL_PX),
                    item[3],
                ],
                font_size_px: px(COMBO_FONT_LOGICAL_PX),
                color: if is_selected || hover == Some(SettingsTarget::ThemeOption(option)) {
                    palette.menu_item_text_selected
                } else {
                    palette.menu_item_text
                },
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
            });
        }
    }

    (quads, labels, sprites)
}

/// A surface that floats: its lift, its hairline, its face — the three planes
/// the hover-peek flyout is already built from, in the same order.
#[allow(clippy::too_many_arguments)]
fn push_float_window(
    quads: &mut Vec<OverlayQuad>,
    frame: [f32; 4],
    radius: f32,
    border: f32,
    spread: f32,
    face: [u8; 3],
    shadow: [u8; 3],
    shadow_inner_alpha: f32,
    shadow_outer_alpha: f32,
    hairline: [u8; 3],
    hairline_alpha: f32,
) {
    // The wider, fainter ring first, so the two compose into a falloff rather
    // than a band.
    for (extent, alpha) in [
        (spread, shadow_outer_alpha),
        (spread / 2.0, shadow_inner_alpha),
    ] {
        quads.extend(rounded_overlay_halo(frame, radius, extent, shadow, alpha));
    }
    quads.extend(rounded_overlay_fill(
        frame,
        radius,
        hairline,
        hairline_alpha,
    ));
    // Concentric with the box: one border in on every side, so one border less
    // radius — anything else thickens the hairline through the corner.
    quads.extend(rounded_overlay_fill(
        [
            frame[0] + border,
            frame[1] + border,
            frame[2] - border,
            frame[3] - border,
        ],
        radius - border,
        face,
        1.0,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window big enough that nothing is clamped: the shape every geometry
    /// claim below is stated against.
    const SURFACE: (f32, f32) = (1280.0, 800.0);

    fn open(scale: f32, menu_open: bool) -> SettingsLayout {
        layout(
            (SURFACE.0 * scale).round(),
            (SURFACE.1 * scale).round(),
            scale,
            menu_open,
        )
        .expect("this window can host the dialog")
    }

    fn centre(rect: [f32; 4]) -> (f64, f64) {
        (
            f64::from((rect[0] + rect[2]) / 2.0),
            f64::from((rect[1] + rect[3]) / 2.0),
        )
    }

    fn width(rect: [f32; 4]) -> f32 {
        rect[2] - rect[0]
    }

    fn height(rect: [f32; 4]) -> f32 {
        rect[3] - rect[1]
    }

    /// PIN (mock-up geometry): the dialog lands exactly where
    /// `design/ui-mockup.html` puts it — `width: min(480px, 92%)`,
    /// `margin: 54px auto 0`, and a height its own content decides.
    ///
    /// The 157 is not a guess: it is `1 + 56 + 99 + 1` — two hairlines, the
    /// header's `16 + 30 + 10`, and a content box of
    /// `2 + 10 + 13 + 2 + (11 + 32 + 11) + 18` — and the mock-up's own renderer
    /// reports 157 for a dialog holding this one group and this one row.
    ///
    /// Red gate: every term is load-bearing. Drop the `auto` centring and `left`
    /// moves; drop the 54 and `top` moves; use the row's *border* box (55, which
    /// is what it measures when it is not the last child) and the height is 158.
    #[test]
    fn the_dialog_lands_where_the_mock_up_puts_it() {
        let placed = open(1.0, false);
        assert_eq!(width(placed.frame), 480.0, "min(480px, 92%) at 1280 wide");
        assert_eq!(placed.frame[1], 54.0, "margin-top: 54px");
        assert_eq!(
            placed.frame[0],
            (SURFACE.0 - 480.0) / 2.0,
            "margin-left/right: auto"
        );
        assert_eq!(height(placed.frame), 157.0, "content decides the height");

        // The 92% share takes over below 480/0.92 ~= 521.7 logical pixels.
        let narrow = layout(480.0, 800.0, 1.0, false).expect("480 wide still hosts the dialog");
        assert_eq!(
            width(narrow.frame),
            (480.0_f32 * DIALOG_WIDTH_RATIO).round(),
            "under the cap the dialog takes 92% of the window"
        );
        assert_eq!(
            narrow.frame[0],
            ((480.0 - width(narrow.frame)) / 2.0).round()
        );
    }

    /// PIN (mock-up geometry): every control inside the dialog is the box the
    /// stylesheet gives it, at the offset the stylesheet puts it at.
    ///
    /// Read against the mock-up's own measurements, taken with the dialog open
    /// at 1280x800: `.dlg-close` 30x30 with its icon ending 22 from the dialog's
    /// edge, `.combo > button` 118x27.5 flush with the content's right padding,
    /// `.row .text` one gap to its left.
    #[test]
    fn every_control_is_the_box_the_stylesheet_gives_it() {
        let placed = open(1.0, false);
        assert_eq!((width(placed.close), height(placed.close)), (30.0, 30.0));
        assert_eq!(
            placed.frame[2] - placed.close[2],
            1.0 + HEADER_PADDING_RIGHT_LOGICAL_PX,
            "the 30px button ends 12 inside the dialog's hairline, so its 10px \
             icon ends 22 from the edge - the mock-up's own reasoning"
        );
        assert_eq!(
            placed.close[1],
            placed.frame[1] + 1.0 + 16.0,
            "padding-top: 16px"
        );

        assert_eq!(width(placed.combo), 118.0, "min-width: 118px");
        assert_eq!(height(placed.combo), 27.5, "5 + 15.5 + 5 + two borders");
        assert_eq!(
            placed.frame[2] - placed.combo[2],
            1.0 + CONTENT_PADDING_X_LOGICAL_PX + ROW_PADDING_X_LOGICAL_PX,
            "the control is flush with the row's own right edge"
        );
        assert_eq!(
            placed.combo[0] - placed.row_title[2],
            ROW_GAP_LOGICAL_PX,
            ".row gap: 16px between the text column and the control"
        );
        // `align-items: center`: the 27.5 control is centred on the 32 the two
        // stacked lines take, not top-aligned with them.
        let text_axis = (placed.row_title[1] + placed.row_desc[3]) / 2.0;
        let combo_axis = (placed.combo[1] + placed.combo[3]) / 2.0;
        assert!(
            (text_axis - combo_axis).abs() <= 0.5,
            "the row's items share one axis: {text_axis} vs {combo_axis}"
        );
        assert_eq!(
            placed.row_desc[1] - placed.row_title[3],
            ROW_DESC_MARGIN_TOP_LOGICAL_PX,
            ".desc margin-top: 1px"
        );
    }

    /// PIN (DPI): the dialog is one design at every scale — nothing collapses,
    /// nothing keeps a scale-1 size, and every box stays within a rounding pixel
    /// of the scaled design number.
    ///
    /// Red gate: a box built from an unscaled constant passes at 1.0 and fails
    /// at every other entry in this list.
    #[test]
    fn the_dialog_is_one_design_at_every_dpi() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let placed = open(scale, true);
            let near = |want: f32, got: f32, what: &str| {
                assert!(
                    (got - want).abs() <= 1.0,
                    "scale {scale}: {what} is {got}, wanted {want}"
                );
            };
            near(480.0 * scale, width(placed.frame), "the dialog's width");
            near(157.0 * scale, height(placed.frame), "the dialog's height");
            near(54.0 * scale, placed.frame[1], "the dialog's drop");
            near(30.0 * scale, width(placed.close), "the close button");
            near(30.0 * scale, height(placed.close), "the close button");
            near(118.0 * scale, width(placed.combo), "the combo");
            near(27.5 * scale, height(placed.combo), "the combo");
            let menu = placed.menu.expect("the menu is open");
            near(118.0 * scale, width(menu), "the menu");
            near(
                (2.0 + 8.0 + THEME_OPTIONS.len() as f32 * 27.5) * scale,
                height(menu),
                "the menu",
            );
            for item in &placed.items {
                near(27.5 * scale, height(*item), "a menu item");
            }
            assert_eq!(placed.items.len(), THEME_OPTIONS.len());
        }
    }

    /// PIN (modal): while the dialog is up every point in the window belongs to
    /// the overlay, and the points that are *not* the dialog belong to the
    /// scrim — including the caption run the gear itself lives in.
    ///
    /// Red gate: the gear's own box is computed here from the same numbers
    /// `seats::hit_window_chrome` uses, and that function is asserted to still
    /// call it the gear. If the point were not really the gear the claim below
    /// would be vacuous.
    #[test]
    fn the_scrim_swallows_the_caption_run_the_gear_lives_in() {
        let placed = open(1.0, false);
        let (surface_width, scale) = (SURFACE.0, 1.0_f32);
        let button = bt_render::WINDOW_CAPTION_BUTTON_LOGICAL_PX * scale;
        let gear = f64::from(surface_width - 4.0 * button + button / 2.0);
        let y = f64::from(bt_render::WINDOW_TITLE_BAR_LOGICAL_PX * scale / 2.0);
        assert_eq!(
            crate::seats::hit_window_chrome(surface_width, scale, gear, y),
            Some(crate::seats::ChromeTarget::Settings),
            "the point under test really is the gear"
        );
        assert_eq!(
            hit(&placed, gear, y),
            SettingsTarget::Scrim,
            "a modal means MODAL: the gear is behind the scrim like everything else"
        );
    }

    /// PIN (modal): a sweep of the whole window never lands outside the overlay,
    /// and every point that is not the dialog or its menu is the scrim.
    #[test]
    fn no_point_in_the_window_escapes_the_overlay() {
        let placed = open(1.0, true);
        let menu = placed.menu.expect("the menu is open");
        let mut seen_scrim = false;
        let mut seen_panel = false;
        let mut y = 0.0_f32;
        while y < SURFACE.1 {
            let mut x = 0.0_f32;
            while x < SURFACE.0 {
                let target = hit(&placed, f64::from(x), f64::from(y));
                let inside_dialog = contains(placed.frame, x, y);
                let inside_menu = contains(menu, x, y);
                match target {
                    SettingsTarget::Scrim => {
                        assert!(
                            !inside_dialog && !inside_menu,
                            "({x}, {y}) is on the dialog and was called scrim"
                        );
                        seen_scrim = true;
                    }
                    _ => {
                        assert!(
                            inside_dialog || inside_menu,
                            "({x}, {y}) is behind the scrim and was called {target:?}"
                        );
                        seen_panel = true;
                    }
                }
                x += 7.0;
            }
            y += 7.0;
        }
        assert!(seen_scrim && seen_panel, "the sweep must reach both");
    }

    /// PIN (hit test): every control answers where it is drawn, and the dialog's
    /// own body answers "nowhere" rather than "close".
    #[test]
    fn every_control_answers_where_it_is_drawn() {
        let placed = open(1.0, true);
        let (x, y) = centre(placed.close);
        assert_eq!(hit(&placed, x, y), SettingsTarget::Close);
        let (x, y) = centre(placed.combo);
        assert_eq!(hit(&placed, x, y), SettingsTarget::ThemeCombo);
        for (index, item) in placed.items.iter().enumerate() {
            let (x, y) = centre(*item);
            assert_eq!(
                hit(&placed, x, y),
                SettingsTarget::ThemeOption(THEME_OPTIONS[index]),
                "item {index} must answer for its own option"
            );
        }
        // The menu's own padding: inside the popup, on none of its items.
        let menu = placed.menu.expect("the menu is open");
        assert_eq!(
            hit(&placed, f64::from(menu[0] + 1.0), f64::from(menu[1] + 1.0)),
            SettingsTarget::ThemeMenu
        );
        // The header, left of the title, is the dialog and nothing more.
        assert_eq!(
            hit(
                &placed,
                f64::from(placed.frame[0] + 4.0),
                f64::from(placed.frame[1] + 4.0),
            ),
            SettingsTarget::Panel
        );
    }

    /// PIN: the mapping a press on a picker item makes to `set_theme` — the one
    /// thing between "the user clicked Light" and the process being light.
    ///
    /// Red gate: nothing else in the overlay asks for a theme, so a router that
    /// switched on, say, the combo button would fail the second half.
    #[test]
    fn only_a_picker_item_asks_for_a_theme_and_it_asks_for_its_own() {
        let placed = open(1.0, true);
        for (index, item) in placed.items.iter().enumerate() {
            let (x, y) = centre(*item);
            assert_eq!(
                theme_requested(hit(&placed, x, y)),
                Some(THEME_OPTIONS[index])
            );
        }
        assert_eq!(THEME_OPTIONS, [Theme::Light, Theme::Dark]);
        for target in [
            SettingsTarget::Scrim,
            SettingsTarget::Panel,
            SettingsTarget::Close,
            SettingsTarget::ThemeCombo,
            SettingsTarget::ThemeMenu,
        ] {
            assert_eq!(
                theme_requested(target),
                None,
                "{target:?} asks for no theme"
            );
        }
    }

    /// PIN: the picker's menu opens below its button, and flips above it when
    /// there is no room — measured against the window, which is what actually
    /// clips it (the mock-up's own rule, and its `.content` does not scroll).
    #[test]
    fn the_menu_opens_below_and_flips_up_when_it_would_spill() {
        let tall = open(1.0, true);
        let menu = tall.menu.expect("the menu is open");
        assert_eq!(
            menu[1] - tall.combo[3],
            MENU_OFFSET_LOGICAL_PX,
            "top: calc(100% + 4px)"
        );
        assert_eq!(menu[2], tall.combo[2], "right: 0");
        assert_eq!(width(menu), width(tall.combo), "min-width: 100%");

        // A window whose bottom is right under the combo leaves no room below.
        let short = layout(1280.0, 200.0, 1.0, true).expect("200 tall still hosts the dialog");
        let menu = short.menu.expect("the menu is open");
        assert_eq!(
            short.combo[1] - menu[3],
            MENU_OFFSET_LOGICAL_PX,
            "flipped up, the same 4px gap sits above the button"
        );
        assert!(
            menu[1] >= 0.0,
            "the flipped menu still starts inside the window"
        );
        // Items keep their order under the flip: the first option is still the
        // top one, so "the second item is Dark" does not depend on the flip.
        assert_eq!(short.items.len(), THEME_OPTIONS.len());
        assert!(short.items[0][1] < short.items[1][1]);
    }

    /// PIN: a window that cannot host the dialog answers `None` rather than a
    /// squashed one — and the runtime reads `None` as "shut", so nothing is
    /// trapped behind a modal with nothing on it.
    #[test]
    fn a_window_too_small_to_host_the_dialog_says_so() {
        assert!(layout(1280.0, 100.0, 1.0, false).is_none(), "too short");
        assert!(layout(100.0, 800.0, 1.0, false).is_none(), "too narrow");
        assert!(layout(1280.0, 800.0, 1.0, false).is_some(), "a real window");
    }

    /// PIN (Esc): one layer per press — the open menu first, then the dialog,
    /// then nothing, and "nothing" is reported so the key can fall through to
    /// whoever owns it next.
    #[test]
    fn escape_unwinds_one_layer_per_press() {
        let mut panel = SettingsPanel::default();
        assert!(!panel.close_one_layer(), "nothing is open yet");
        panel.toggle();
        panel.set_menu_open(true);
        assert!(panel.close_one_layer());
        assert!(
            panel.is_open() && !panel.menu_is_open(),
            "the menu went first"
        );
        assert!(panel.close_one_layer());
        assert!(!panel.is_open(), "the dialog went second");
        assert!(!panel.close_one_layer(), "and then there is nothing left");
    }

    /// PIN: the gear toggles, and shutting the dialog takes the menu with it —
    /// so reopening never shows a picker the user never opened.
    #[test]
    fn the_gear_toggles_and_shutting_takes_the_menu_with_it() {
        let mut panel = SettingsPanel::default();
        panel.toggle();
        assert!(panel.is_open());
        panel.set_menu_open(true);
        panel.toggle();
        assert!(!panel.is_open() && !panel.menu_is_open());
        panel.toggle();
        assert!(panel.is_open() && !panel.menu_is_open());
        panel.close();
        assert!(!panel.is_open());
    }

    fn quads_of(
        placed: &SettingsLayout,
        hover: Option<SettingsTarget>,
        selected: Theme,
    ) -> Vec<OverlayQuad> {
        build(placed, hover, selected).0
    }

    /// Visual PIN: the scrim is the mock-up's own `rgba(15,15,15,.35)` across
    /// the whole window, and it is the *first* thing drawn, so everything else
    /// stands on it rather than under it.
    #[test]
    fn the_scrim_is_the_mock_ups_own_alpha_over_the_whole_window() {
        let placed = open(1.0, false);
        let palette = chrome_palette();
        let scrim = quads_of(&placed, None, Theme::Dark)[0];
        assert_eq!(scrim.rect, [0.0, 0.0, SURFACE.0, SURFACE.1]);
        assert_eq!(scrim.color, [0x0f, 0x0f, 0x0f]);
        assert_eq!(scrim.color, palette.modal_scrim);
        assert!(
            (scrim.alpha - 0.35).abs() < 0.005,
            "the scrim is .35, saw {}",
            scrim.alpha
        );
    }

    /// Visual PIN (float-window craft): the dialog is a lift, a hairline and a
    /// face, and its corners carry partial coverage rather than a staircase —
    /// the same claim `rounded_rect.rs` pins for every window that floats.
    ///
    /// Red gate: nested rectangles put no pixel strictly between 0 and 1, so the
    /// partial-coverage count alone rules them out.
    #[test]
    fn the_dialog_wears_the_float_windows_craft_at_every_dpi() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let placed = open(scale, false);
            let palette = chrome_palette();
            let quads = quads_of(&placed, None, Theme::Dark);
            let border_alpha = f32::from(palette.menu_border_alpha) / 255.0;
            let hairline = quads
                .iter()
                .filter(|quad| quad.color == palette.menu_border)
                .filter(|quad| (quad.alpha - border_alpha).abs() < 1e-4)
                .count();
            assert!(
                hairline > 0,
                "scale {scale}: the dialog wears --border at the mock-up's own alpha"
            );
            let face = quads
                .iter()
                .filter(|quad| quad.color == palette.dialog_surface && quad.alpha == 1.0)
                .count();
            assert!(face > 0, "scale {scale}: the dialog's face is --win");
            let lift = quads
                .iter()
                .filter(|quad| quad.color == palette.menu_shadow)
                .count();
            assert!(
                lift > 0,
                "scale {scale}: the dialog is lifted off the scrim"
            );
            let radius = FLOAT_WINDOW_RADIUS_LOGICAL_PX * scale;
            let partial = quads
                .iter()
                .filter(|quad| quad.alpha > 0.0 && quad.alpha < 1.0)
                .filter(|quad| quad.rect[2] - quad.rect[0] == 1.0)
                .filter(|quad| quad.color == palette.dialog_surface)
                .count();
            assert!(
                partial >= radius as usize,
                "scale {scale}: an antialiased quarter circle spends at least one \
                 partial pixel per row on the face, saw {partial}"
            );
        }
    }

    /// Visual PIN: the picker shows the theme in force, and marks it with the
    /// accent tick when the menu is open — exactly one tick, never two.
    #[test]
    fn the_picker_shows_and_ticks_the_theme_in_force() {
        for selected in THEME_OPTIONS {
            let placed = open(1.0, true);
            let palette = chrome_palette();
            let (_, labels, _) = build(&placed, None, selected);
            let ticks: Vec<_> = labels.iter().filter(|label| label.text == TICK).collect();
            assert_eq!(ticks.len(), 1, "exactly one option is the one in force");
            assert_eq!(ticks[0].color, palette.accent, "the tick is the accent");
            // The button says what the tick marks.
            let shown = theme_label(selected);
            let on_button = labels
                .iter()
                .find(|label| {
                    label.text == shown
                        && label.rect[1] >= placed.combo[1]
                        && label.rect[3] <= placed.combo[3]
                })
                .expect("the button shows the theme in force");
            assert_eq!(on_button.color, palette.dialog_title_text);
            // And the tick sits in the row of the option it marks.
            let index = THEME_OPTIONS
                .iter()
                .position(|option| *option == selected)
                .expect("the theme in force is one of the options");
            let item = placed.items[index];
            assert!(ticks[0].rect[1] >= item[1] && ticks[0].rect[3] <= item[3]);
        }
    }

    /// Visual PIN: hover paints one thing and only when something is hovered —
    /// the mock-up's `--hover` over the surface the control actually sits on.
    #[test]
    fn hover_paints_the_control_under_the_pointer_and_nothing_else() {
        let placed = open(1.0, true);
        let palette = chrome_palette();
        let count = |hover, color| {
            quads_of(&placed, hover, Theme::Dark)
                .iter()
                .filter(|quad| quad.color == color)
                .count()
        };
        assert_eq!(
            count(None, palette.dialog_hover),
            0,
            "nothing is lit when nothing is hovered"
        );
        assert!(
            count(Some(SettingsTarget::Close), palette.dialog_hover) > 0,
            "the close button takes --hover over --win"
        );
        assert!(
            count(Some(SettingsTarget::ThemeCombo), palette.dialog_hover) > 0,
            "so does the combo button"
        );
        assert_eq!(
            count(Some(SettingsTarget::ThemeMenu), palette.menu_item_hover),
            0,
            "the menu's own body is not an item"
        );
        assert!(
            count(
                Some(SettingsTarget::ThemeOption(THEME_OPTIONS[0])),
                palette.menu_item_hover
            ) > 0,
            "an item takes --hover over --menu, which is a different grey"
        );
    }

    /// The group heading is uppercase and tracked, which is the whole of what
    /// makes it read as a heading at 11px rather than as a small sentence.
    #[test]
    fn the_group_heading_is_uppercase_and_tracked() {
        let placed = open(1.0, false);
        let (_, labels, _) = build(&placed, None, Theme::Dark);
        let heading = labels
            .iter()
            .find(|label| label.text == "APPEARANCE")
            .expect("the Appearance group is headed");
        assert_eq!(heading.font_size_px, GROUP_LABEL_FONT_LOGICAL_PX);
        assert!(
            (heading.letter_spacing_em - 0.05).abs() < 1e-4,
            "letter-spacing: .05em, and em is a ratio"
        );
        assert_eq!(heading.color, chrome_palette().dialog_muted_text);
        // Red gate for a real bug this pass shipped and then caught on screen:
        // tracking was multiplied by the scaled font size on the way in, which
        // at 200% asked the shaper for 1.1 *em* per glyph and spelled the
        // heading out letter by letter. A ratio does not carry the DPI scale.
        for scale in [1.0_f32, 2.0] {
            let placed = open(scale, false);
            let (_, labels, _) = build(&placed, None, Theme::Dark);
            let heading = labels
                .iter()
                .find(|label| label.text == "APPEARANCE")
                .expect("the Appearance group is headed");
            assert!(
                (heading.letter_spacing_em - 0.05).abs() < 1e-4,
                "scale {scale}: tracking is the same ratio at every DPI, saw {}",
                heading.letter_spacing_em
            );
        }
    }

    /// The dialog's close affordance is the mock-up's own `#i-close`, and it is
    /// the only mark the overlay draws.
    #[test]
    fn the_close_affordance_wears_the_mock_ups_own_close_symbol() {
        let placed = open(1.0, true);
        let (_, _, sprites) = build(&placed, None, Theme::Dark);
        assert_eq!(sprites.len(), 1);
        assert_eq!(sprites[0].mark, ChromeMark::WindowClose);
        let glyph = sprites[0].rect;
        assert_eq!(width(glyph), 10.0, "the close icon is 10px");
        assert_eq!(height(glyph), 10.0);
        assert!(
            (((glyph[0] + glyph[2]) / 2.0) - ((placed.close[0] + placed.close[2]) / 2.0)).abs()
                <= 0.5,
            "the icon is centred in its 30px button"
        );
    }

    /// A fresh panel is shut, with nothing open inside it and nothing hovered.
    #[test]
    fn a_fresh_panel_is_shut() {
        let panel = SettingsPanel::default();
        assert!(!panel.is_open());
        assert!(!panel.menu_is_open());
        assert_eq!(panel.hover(), None);
    }
}
