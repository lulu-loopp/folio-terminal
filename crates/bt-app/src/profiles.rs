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

use bt_render::{
    ChromeLabel, FLOAT_WINDOW_BORDER_LOGICAL_PX, FLOAT_WINDOW_SHADOW_LOGICAL_PX, OverlayQuad,
    chrome_palette, rounded_overlay_fill,
};

use crate::{
    marks::{ChromeMark, ChromeSprite},
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
const HINT_FONT_LOGICAL_PX: f32 = 11.0;
const HINT_TEXT: &str = "default";

/// A profile the picker can start a tab from.
///
/// One entry, and the list is a list rather than a constant because that is the
/// honest shape of it: the mock-up carries four (PowerShell, WSL, Git Bash,
/// Command Prompt) and this build launches exactly one shell. Offering the other
/// three would be three rows that cannot do what they say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    pub title: &'static str,
    /// A profile's icon is its mark, not a letter that happens to be in its
    /// prompt — the mock-up says so in as many words at `const mark`.
    pub mark: ChromeMark,
}

pub const PROFILES: [Profile; 1] = [Profile {
    title: "PowerShell",
    mark: ChromeMark::ProfilePowerShell,
}];

/// The index a new tab is started from when nobody picks — `state.defaultProfile`.
pub const DEFAULT_PROFILE: usize = 0;

/// Whether the picker is up, and which row the pointer is on.
///
/// App state and nothing else: not a seat, so the solver never sees it; not an
/// intent, so the session file never sees it. A menu that survived a restart
/// would be a window that opens mid-gesture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProfileMenu {
    open: bool,
    hover: Option<usize>,
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
    pub fn set_hover(&mut self, hover: Option<usize>) -> bool {
        let hover = if self.open { hover } else { None };
        let changed = self.hover != hover;
        self.hover = hover;
        changed
    }

    pub fn hover(self) -> Option<usize> {
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
}

/// The menu hung under `anchor` — the `˅`'s own box, in physical pixels — inside
/// a surface this wide.
#[must_use]
pub fn layout(anchor: [f32; 4], surface_width: f32, scale: f32) -> ProfileMenuLayout {
    let px = |value: f32| value * scale;
    let border = (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale).max(1.0);
    let padding = px(MENU_PADDING_LOGICAL_PX);
    let item_height = px(ITEM_HEIGHT_LOGICAL_PX).round();
    let width = px(MENU_MIN_WIDTH_LOGICAL_PX).round();
    let top = (anchor[3] + px(MENU_OFFSET_LOGICAL_PX)).round();
    let left = anchor[0]
        .min(surface_width - width - px(MENU_EDGE_MARGIN_LOGICAL_PX))
        .max(0.0)
        .round();
    let height = (2.0 * (border + padding) + item_height * PROFILES.len() as f32).round();
    let frame = [left, top, left + width, top + height];
    let items = (0..PROFILES.len())
        .map(|index| {
            let item_top = frame[1] + border + padding + item_height * index as f32;
            [
                frame[0] + border + padding,
                item_top,
                frame[2] - border - padding,
                item_top + item_height,
            ]
        })
        .collect();
    ProfileMenuLayout {
        scale,
        frame,
        items,
    }
}

/// What a point is over: a row, `Some(None)` for the menu's own body between and
/// around its rows, and `None` for anywhere else in the window.
///
/// The two negatives are different answers and the difference is the whole of
/// what "popup" means here: a press on the body is the menu's and does nothing,
/// a press outside it belongs to whatever is there and merely closes the menu on
/// its way past.
#[must_use]
pub fn hit(layout: &ProfileMenuLayout, x: f64, y: f64) -> Option<Option<usize>> {
    let (x, y) = (x as f32, y as f32);
    for (index, item) in layout.items.iter().enumerate() {
        if contains(*item, x, y) {
            return Some(Some(index));
        }
    }
    contains(layout.frame, x, y).then_some(None)
}

fn contains(rect: [f32; 4], x: f32, y: f32) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

/// The menu's three planes and its rows.
#[must_use]
pub fn build(
    layout: &ProfileMenuLayout,
    hover: Option<usize>,
) -> (Vec<OverlayQuad>, Vec<ChromeLabel>, Vec<ChromeSprite>) {
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
        let hovered = hover == Some(index);
        if hovered {
            quads.extend(rounded_overlay_fill(
                *item,
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
        sprites.push(ChromeSprite {
            mark: profile.mark,
            rect: [mark_left, mark_top, mark_left + mark, mark_top + mark],
            color: palette.accent,
        });
        labels.push(ChromeLabel {
            text: profile.title.to_owned(),
            rect: [
                column_right + px(ITEM_GAP_LOGICAL_PX),
                item[1],
                item[2] - px(ITEM_PADDING_X_LOGICAL_PX),
                item[3],
            ],
            font_size_px: px(ITEM_FONT_LOGICAL_PX),
            color: if hovered {
                palette.menu_item_text_selected
            } else {
                palette.menu_item_text
            },
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
        });
        // `margin-left: auto` puts the hint hard against the row's trailing
        // padding, and it names a fact about the profile rather than the row's
        // state — so it does not answer to hover.
        if index == DEFAULT_PROFILE {
            labels.push(ChromeLabel {
                text: HINT_TEXT.to_owned(),
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
            });
        }
    }

    (quads, labels, sprites)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `˅`'s box in a 960x600 window at 1x, taken from the strip's own
    /// geometry rather than restated here.
    fn anchor(scale: f32) -> [f32; 4] {
        crate::seats::tab_strip_geometry(960.0 * scale, scale, 1, 0, 0.0).new_tab_menu
    }

    /// PIN — the menu hangs 4px under the button that opened it, at the button's
    /// own left edge, and it is the mock-up's 180px wide.
    #[test]
    fn the_menu_hangs_under_its_button_at_the_mockup_s_own_width() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let button = anchor(scale);
            let layout = layout(button, 960.0 * scale, scale);
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
        let layout = layout(button, 960.0, scale);
        let frame = layout.frame;
        let item = layout.items[0];
        assert_eq!(
            hit(
                &layout,
                f64::from((item[0] + item[2]) / 2.0),
                f64::from((item[1] + item[3]) / 2.0)
            ),
            Some(Some(0))
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
        let layout = layout([260.0, 9.0, 288.0, 37.0], surface, scale);
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
        assert!(!menu.set_hover(Some(0)), "a shut menu has no hovered row");
        assert_eq!(menu.hover(), None);
        menu.toggle();
        assert!(menu.is_open());
        assert!(menu.set_hover(Some(0)));
        assert_eq!(menu.hover(), Some(0));
        assert!(menu.close());
        assert_eq!(menu.hover(), None);
        assert!(!menu.close(), "closing a shut menu consumes nothing");
    }

    /// PIN — the hovered row wears `--ink` on `--hover`, and the resting one
    /// `--ink2` on nothing; the row also carries its profile's own mark.
    #[test]
    fn a_hovered_row_lights_up_and_every_row_wears_its_profile_s_mark() {
        let scale = 1.0;
        let layout = layout(anchor(scale), 960.0, scale);
        let palette = chrome_palette();
        let (rest_quads, rest_labels, sprites) = build(&layout, None);
        let (hover_quads, hover_labels, _) = build(&layout, Some(0));
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

        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let layout = layout(anchor(scale), 960.0 * scale, scale);
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
        let layout = layout(anchor(scale), 960.0, scale);
        let palette = chrome_palette();
        let (_, labels, sprites) = build(&layout, None);
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
}
