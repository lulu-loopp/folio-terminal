//! **The download that could not be handed on** — the one failure card in this
//! window that stands *over* a page rather than instead of one
//! (`docs/DESIGN.md` §7.7 ④, user ruling 2026-08-22).
//!
//! # Why one of the five is a sheet
//!
//! Four of §7.7 ④'s five states leave the seat with nothing in it: a WebView
//! that is dead, absent or refused draws the same black hole a hidden one does
//! (`w0-evidence.md` §2⑨), so on those the card **is** the seat's content and
//! is drawn as ordinary pane chrome. This one is different, and the plan says
//! why in one clause: 方案 §0 cancels the *download* and 「不可重放者提示无法
//! 下载」 — the page that asked for the file is still standing, still scrolled
//! where the reader left it, and blanking it would throw away more than the
//! failure did.
//!
//! So it is the same drawing on a scrim: one mark, one sentence, at most one
//! line of fact, one verb. Esc puts it away, and it is the only one of the five
//! that has an Esc — taking any of the other four away would leave the hole.
//!
//! # Why it is an overlay layer and the other four are not
//!
//! The page is composed *under* wgpu and is visible exactly where this surface
//! is transparent, and `bt_render::WindowRenderer::set_web_holes` punches that
//! transparency **over everything the seats themselves drew** (§7.8 ②). A card
//! painted into the pane's own chrome would therefore be erased by the hole for
//! as long as the page is visible — which is exactly why the four that replace a
//! page can be pane chrome: the seat hides the page first, so there is no hole.
//! This one keeps the page, so it has to be drawn where the hole cannot reach
//! it, and that is the overlay.
//!
//! # Its ground
//!
//! `--menu` with a hairline, an 8px round and the menu's own shadow, and that is
//! measured rather than chosen (§7.7 ④): with only a scrim, the page's body
//! reads *through* the card's sentences, the two runs of text interleave, and a
//! notice becomes one more line of the document instead of the answer to a verb
//! somebody pressed.

use bt_layout::SeatId;
use bt_render::{ChromeLabel, ChromeLabelWeight, ChromePalette, OverlayQuad, rounded_overlay_fill};

use crate::marks::{ChromeMark, ChromeSprite, OverlayLayer};
use crate::seats::ChromeTarget;

/// The card's round — the menu's 8, not a floating window's 10. It is a panel
/// laid on one pane, not a window opened over the desk.
const RADIUS_LOGICAL_PX: f32 = 8.0;
/// How far the card is inset from the seat's body on every side, at most.
const MARGIN_LOGICAL_PX: f32 = 24.0;
/// The card's own padding.
const PADDING_LOGICAL_PX: f32 = 22.0;
/// The widest the card is allowed to grow. `38ch` at the sentence's size is what
/// `.pv-blank .pvb-say { max-width: 38ch }` asks for, and a line longer than
/// that is a paragraph rather than a sentence.
const MAX_WIDTH_LOGICAL_PX: f32 = 420.0;
/// The gap between the mark, the sentence, the fact and the verb — the card's
/// own `gap: 10px`.
const GAP_LOGICAL_PX: f32 = 10.0;
/// `.pv-blank > svg { width: 30px }`.
const MARK_LOGICAL_PX: f32 = 30.0;
/// `.pv-blank { font-size: 12.5px }`.
const SAY_FONT_LOGICAL_PX: f32 = 12.5;
/// `.pv-blank .pvb-detail { font: 11.5px/1.5 … monospace }` — the fact, in the
/// face this window writes facts in everywhere else.
const DETAIL_FONT_LOGICAL_PX: f32 = 11.5;
/// `.pv-blank button { font-size: 12px }`.
const VERB_FONT_LOGICAL_PX: f32 = 12.0;
/// `.pv-blank button { padding: 5px 12px }`.
const VERB_PADDING_X_LOGICAL_PX: f32 = 12.0;
const VERB_PADDING_Y_LOGICAL_PX: f32 = 5.0;
/// `.pv-blank button { border-radius: 6px }`.
const VERB_RADIUS_LOGICAL_PX: f32 = 6.0;
/// The line box a sentence or a fact gets, as a multiple of its size — the
/// card's own `line-height: 1.5`.
const LINE_HEIGHT: f32 = 1.5;

/// The size the verb's caption is measured at — the one number a caller needs
/// before it can lay a sheet out, and the reason it is a function rather than a
/// public constant is that a caller multiplying by the scale itself is a caller
/// that can forget to.
#[must_use]
pub fn verb_font_px(scale: f32) -> f32 {
    VERB_FONT_LOGICAL_PX * scale
}

/// One sheet's boxes, in physical pixels of the whole surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetLayout {
    /// The seat's body — what the scrim covers, and what the card is centred in.
    pub body: [f32; 4],
    pub frame: [f32; 4],
    pub mark: [f32; 4],
    pub say: [f32; 4],
    /// Empty (zero height) when the fault has no fact worth quoting.
    pub detail: [f32; 4],
    pub verb: [f32; 4],
    pub scale: f32,
}

/// Lay one sheet out inside a seat's body.
///
/// `verb_width` is the button caption's measured width, which only something
/// holding a font can answer — `seats::preview_card_geometry`'s own division,
/// and for its reason.
#[must_use]
pub fn lay_out(body: [f32; 4], verb_width: f32, has_detail: bool, scale: f32) -> SheetLayout {
    let px = |logical: f32| logical * scale;
    let padding = px(PADDING_LOGICAL_PX).round();
    let gap = px(GAP_LOGICAL_PX).round();
    let mark = px(MARK_LOGICAL_PX).round().max(1.0);
    let say_line = (px(SAY_FONT_LOGICAL_PX) * LINE_HEIGHT).round().max(1.0);
    let detail_line = (px(DETAIL_FONT_LOGICAL_PX) * LINE_HEIGHT).round().max(1.0);
    let verb_height = ((px(VERB_FONT_LOGICAL_PX) * LINE_HEIGHT).round()
        + px(VERB_PADDING_Y_LOGICAL_PX).round() * 2.0)
        .max(1.0);
    let verb_box_width = (verb_width + px(VERB_PADDING_X_LOGICAL_PX) * 2.0)
        .round()
        .max(1.0);

    let margin = px(MARGIN_LOGICAL_PX).round();
    let available = (body[2] - body[0] - margin * 2.0).max(1.0);
    // The card is as wide as it is allowed to be and no wider — the sentence
    // wraps to the box rather than the box growing to the sentence, because a
    // card sized by its longest error message is a card that changes shape per
    // failure.
    let width = available.min(px(MAX_WIDTH_LOGICAL_PX)).max(1.0);
    let content_height = mark
        + gap
        + say_line
        + if has_detail { gap + detail_line } else { 0.0 }
        + gap
        + verb_height;
    let height = content_height + padding * 2.0;
    let centre_x = ((body[0] + body[2]) / 2.0).round();
    let top = ((body[1] + body[3] - height) / 2.0).round().max(body[1]);
    let frame = [
        centre_x - (width / 2.0).round(),
        top,
        centre_x + (width / 2.0).round(),
        top + height,
    ];
    let column_left = frame[0] + padding;
    let column_right = frame[2] - padding;
    let mark_top = frame[1] + padding;
    let say_top = mark_top + mark + gap;
    let detail_top = say_top + say_line + gap;
    let verb_top = if has_detail {
        detail_top + detail_line + gap
    } else {
        say_top + say_line + gap
    };
    SheetLayout {
        body,
        frame,
        mark: [
            centre_x - mark / 2.0,
            mark_top,
            centre_x + mark / 2.0,
            mark_top + mark,
        ],
        say: [column_left, say_top, column_right, say_top + say_line],
        detail: if has_detail {
            [
                column_left,
                detail_top,
                column_right,
                detail_top + detail_line,
            ]
        } else {
            [column_left, detail_top, column_right, detail_top]
        },
        verb: [
            centre_x - (verb_box_width / 2.0).round(),
            verb_top,
            centre_x + (verb_box_width / 2.0).round(),
            verb_top + verb_height,
        ],
        scale,
    }
}

/// What one sheet says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetContent<'a> {
    pub say: &'a str,
    /// The one line of fact, or empty.
    pub detail: &'a str,
    pub verb: &'a str,
    pub verb_hovered: bool,
}

/// Draw one sheet as an overlay layer — scrim, card, and the one verb on it.
#[must_use]
pub fn build(
    layout: &SheetLayout,
    content: SheetContent<'_>,
    palette: &ChromePalette,
) -> OverlayLayer {
    let scale = layout.scale;
    let px = |logical: f32| logical * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    // **The scrim covers the seat's body and no more.** A modal's scrim covers
    // the window because a modal is the window asking a question; this is one
    // pane's page answering for one verb pressed inside it, and the tab beside
    // it never stopped working.
    let mut quads = vec![OverlayQuad {
        rect: layout.body,
        color: palette.modal_scrim,
        alpha: alpha(palette.modal_scrim_alpha),
    }];
    let radius = px(RADIUS_LOGICAL_PX).round();
    crate::settings::push_float_window(
        &mut quads,
        layout.frame,
        radius,
        (px(1.0)).max(1.0),
        px(f32::from(SHADOW_SPREAD_LOGICAL_PX)),
        palette.menu_surface,
        palette.menu_shadow,
        alpha(palette.menu_shadow_inner_alpha),
        alpha(palette.menu_shadow_outer_alpha),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
    );
    let mut labels = Vec::new();
    let mut sprites = vec![
        ChromeSprite::new(ChromeMark::Globe, layout.mark, palette.files_row_muted)
            .with_opacity(MARK_OPACITY),
    ];
    labels.push(ChromeLabel {
        mono: false,
        text: content.say.to_owned(),
        rect: layout.say,
        font_size_px: px(SAY_FONT_LOGICAL_PX),
        color: palette.menu_item_text,
        align_right: false,
        align_center: true,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: Some(layout.say),
    });
    if !content.detail.is_empty() {
        labels.push(ChromeLabel {
            mono: true,
            text: content.detail.to_owned(),
            rect: layout.detail,
            font_size_px: px(DETAIL_FONT_LOGICAL_PX),
            color: palette.menu_item_hint_text,
            align_right: false,
            align_center: true,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: Some(layout.detail),
        });
    }
    if content.verb_hovered {
        quads.extend(rounded_overlay_fill(
            layout.verb,
            px(VERB_RADIUS_LOGICAL_PX).round(),
            palette.menu_item_hover,
            1.0,
        ));
    }
    // `border: 1px solid var(--border)` — a ring and not a fill, so the one verb
    // reads as an offer rather than as a primary action. The card's own rule,
    // quoted from the four that stand in a seat so the five look like one card.
    sprites.push(ChromeSprite::new(
        ChromeMark::ControlPillRing {
            radius_px: px(VERB_RADIUS_LOGICAL_PX).round().max(0.0) as u32,
            stroke_px: scale.round().max(1.0) as u32,
        },
        layout.verb,
        palette.menu_border,
    ));
    labels.push(ChromeLabel {
        mono: false,
        text: content.verb.to_owned(),
        rect: layout.verb,
        font_size_px: px(VERB_FONT_LOGICAL_PX),
        color: if content.verb_hovered {
            palette.menu_item_text_selected
        } else {
            palette.menu_item_text
        },
        align_right: false,
        align_center: true,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: Some(layout.verb),
    });
    OverlayLayer {
        quads,
        labels,
        sprites,
        ..OverlayLayer::default()
    }
}

/// `.pv-blank > svg { opacity: .5 }`.
const MARK_OPACITY: f32 = 0.5;
/// The menu's own lift, in logical pixels — `0 16px 48px` reduced to the one
/// spread `push_float_window` samples the falloff over.
const SHADOW_SPREAD_LOGICAL_PX: u8 = 24;

/// Which part of a sheet the pointer is on.
///
/// Only the verb answers, and the sheet swallows everything else: a press on the
/// scrim is **not** a dismissal, because the one thing a reader must not be able
/// to do by accident is lose the only notice that says a file they asked for did
/// not arrive. Esc is the way out, and it is the only one.
#[must_use]
pub fn hit(layout: &SheetLayout, seat: SeatId, x: f32, y: f32) -> Option<ChromeTarget> {
    inside(layout.verb, x, y).then_some(ChromeTarget::PreviewFaultVerb(seat))
}

/// Whether a point is anywhere on the sheet — the verb, the card or the scrim.
///
/// The press router asks this and stops: a press on the scrim is **not** a
/// dismissal, because the one thing a reader must not be able to do by accident
/// is lose the only notice that says a file they asked for did not arrive. Esc
/// is the way out and it is the only one.
#[must_use]
pub fn covers(layout: &SheetLayout, x: f32, y: f32) -> bool {
    inside(layout.body, x, y)
}

fn inside(box_: [f32; 4], x: f32, y: f32) -> bool {
    x >= box_[0] && x < box_[2] && y >= box_[1] && y < box_[3]
}
