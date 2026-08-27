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
//! # And an `×`, since 2026-08-27
//!
//! The Esc used to be the only way out, and this header used to say so with
//! some pride. The gesture audit (`docs/plans/ui-style/invisible-gestures-2026-08-26.md`
//! 丙6) read the same fact off the other side of the glass: a reader who wants
//! the card gone and does not want the verb has three habits — press the `×`,
//! press outside, press the card — and all three did nothing, so the card read
//! as a window that had stopped answering. The `×` is added and the two
//! swallows are kept: pressing the scrim is still not a dismissal, because
//! losing the only notice that a file did not arrive must stay something a
//! reader *does* rather than something a stray click does to them.
//!
//! It is safe here for the same reason the Esc was, and it is safe **only**
//! here: there is a page underneath to come back to. The other four failure
//! states *are* the seat, and a close on one of those would leave the black
//! hole a hidden WebView draws.
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
/// The `×`'s box and its round — the notice strip's `.pn-x { width: 22px;
/// height: 22px; border-radius: 6px }`, quoted rather than chosen so that a
/// reader who has met one close in this window has met them all. The glyph
/// inside it is not a number here: it comes from the slot table, like every
/// other drawing this window puts in a box.
const CLOSE_BOX_LOGICAL_PX: f32 = 22.0;
const CLOSE_RADIUS_LOGICAL_PX: f32 = 6.0;
/// How far the `×` sits in from the card's own corner. Less than the card's
/// [`PADDING_LOGICAL_PX`], because the padding is the *text* column's inset and
/// a corner control that honoured it would read as part of the sentence's block
/// rather than as the card's own furniture.
const CLOSE_INSET_LOGICAL_PX: f32 = 8.0;

/// The size the verb's caption is measured at — the one number a caller needs
/// before it can lay a sheet out, and the reason it is a function rather than a
/// public constant is that a caller multiplying by the scale itself is a caller
/// that can forget to.
#[must_use]
pub fn verb_font_px(scale: f32) -> f32 {
    VERB_FONT_LOGICAL_PX * scale
}

/// The width the sentence is wrapped to — the card less its own padding.
///
/// Handed out so the caller can wrap before it lays out, which is the one order
/// that works: how many lines the sentence takes is what decides how tall the
/// card is.
#[must_use]
pub fn say_width(body: [f32; 4], scale: f32) -> f32 {
    let px = |logical: f32| logical * scale;
    let margin = px(MARGIN_LOGICAL_PX).round();
    let available = (body[2] - body[0] - margin * 2.0).max(1.0);
    (available.min(px(MAX_WIDTH_LOGICAL_PX)).max(1.0) - px(PADDING_LOGICAL_PX).round() * 2.0)
        .max(1.0)
}

/// The size the sentence is measured at.
#[must_use]
pub fn say_font_px(scale: f32) -> f32 {
    SAY_FONT_LOGICAL_PX * scale
}

/// One sheet's boxes, in physical pixels of the whole surface.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetLayout {
    /// The seat's body — what the scrim covers, and what the card is centred in.
    pub body: [f32; 4],
    pub frame: [f32; 4],
    pub mark: [f32; 4],
    /// One box per line of the sentence — a card's sentence wraps, and the
    /// download's is two sentences long in §7.7 ④'s own table.
    pub say: Vec<[f32; 4]>,
    /// Empty (zero height) when the fault has no fact worth quoting.
    pub detail: [f32; 4],
    pub verb: [f32; 4],
    /// The `×` in the card's top-right corner (丙6).
    ///
    /// It is laid out from the frame's corner and takes no room out of the
    /// column, which is what keeps it from moving the sentence: the card is as
    /// wide as it is allowed to be whatever this control does, and the mark
    /// under it is centred on the card rather than on the space beside it.
    pub close: [f32; 4],
    pub scale: f32,
}

/// Lay one sheet out inside a seat's body.
///
/// `verb_width` is the button caption's measured width, which only something
/// holding a font can answer — `seats::preview_card_geometry`'s own division,
/// and for its reason.
#[must_use]
pub fn lay_out(
    body: [f32; 4],
    verb_width: f32,
    say_lines: usize,
    has_detail: bool,
    scale: f32,
) -> SheetLayout {
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
    let says = say_lines.max(1) as f32;
    let content_height = mark
        + gap
        + say_line * says
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
    let say_bottom = say_top + say_line * says;
    let detail_top = say_bottom + gap;
    let verb_top = if has_detail {
        detail_top + detail_line + gap
    } else {
        say_bottom + gap
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
        say: (0..say_lines.max(1))
            .map(|line| {
                let top = say_top + say_line * line as f32;
                [column_left, top, column_right, top + say_line]
            })
            .collect(),
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
        close: {
            let box_ = px(CLOSE_BOX_LOGICAL_PX).round().max(1.0);
            let inset = px(CLOSE_INSET_LOGICAL_PX).round();
            let right = (frame[2] - inset).round();
            let top = (frame[1] + inset).round();
            [right - box_, top, right, top + box_]
        },
        scale,
    }
}

/// What one sheet says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetContent<'a> {
    /// The sentence, already wrapped to [`SheetLayout::say`]'s boxes.
    pub say: &'a [String],
    /// The one line of fact, or empty.
    pub detail: &'a str,
    pub verb: &'a str,
    pub verb_hovered: bool,
    pub close_hovered: bool,
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
        // **The class's mark and never the site's**, and that is §7.7 ④'s own
        // word for it: a failure card is 「一枚本类的记号、一句话、至多一行事实、
        // 唯一一个动词」. What this card is about is *this window's* refusal to
        // replay a download, so the drawing at the top of it is the drawing for
        // "a web page" and not the drawing a particular server chose for itself
        // — a favicon here would read as the site's own notice.
        ChromeSprite::new(
            ChromeMark::Globe { favicon: None },
            layout.mark,
            palette.files_row_muted,
        )
        .with_opacity(MARK_OPACITY),
    ];
    for (line, rect) in content.say.iter().zip(layout.say.iter()) {
        labels.push(ChromeLabel {
            mono: false,
            text: line.clone(),
            rect: *rect,
            font_size_px: px(SAY_FONT_LOGICAL_PX),
            color: palette.menu_item_text,
            align_right: false,
            align_center: true,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: Some(*rect),
        });
    }
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
    // **The `×`, in the corner every other close in this window is in** (丙6).
    //
    // Drawn last so it sits over the card's own ground, and drawn at rest
    // rather than on hover: the notice strip's `×` is always on its bar, and a
    // close that had to be found before it could be pressed would be the very
    // thing this control was added to end.
    if content.close_hovered {
        quads.extend(rounded_overlay_fill(
            layout.close,
            px(CLOSE_RADIUS_LOGICAL_PX).round(),
            palette.menu_item_hover,
            1.0,
        ));
    }
    let close_mark = crate::icons::ActionIcon::CloseTab.mark();
    sprites.push(ChromeSprite::new(
        close_mark,
        centred_in(
            layout.close,
            px(crate::seats::compact_head_glyph_logical_px(close_mark)),
        ),
        if content.close_hovered {
            palette.menu_item_text_selected
        } else {
            palette.menu_item_hint_text
        },
    ));
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
/// **Two controls answer — the verb and the `×`** — and the sheet swallows
/// everything else: a press on the scrim is still **not** a dismissal, because
/// the one thing a reader must not be able to do by accident is lose the only
/// notice that says a file they asked for did not arrive. The `×` is that
/// dismissal done on purpose, and Escape remains the same door from the
/// keyboard.
///
/// The `×` is asked first, on the house's smallest-target-first rule: it is the
/// smaller box, and the two do not overlap anyway.
#[must_use]
pub fn hit(layout: &SheetLayout, seat: SeatId, x: f32, y: f32) -> Option<ChromeTarget> {
    if inside(layout.close, x, y) {
        return Some(ChromeTarget::PreviewSheetClose(seat));
    }
    inside(layout.verb, x, y).then_some(ChromeTarget::PreviewFaultVerb(seat))
}

/// Whether a point is anywhere on the sheet — a control, the card or the scrim.
///
/// The press router asks this and stops: a press on the scrim is **not** a
/// dismissal, because the one thing a reader must not be able to do by accident
/// is lose the only notice that says a file they asked for did not arrive. The
/// `×` and Escape are the two ways out, and both are things a reader does on
/// purpose.
#[must_use]
pub fn covers(layout: &SheetLayout, x: f32, y: f32) -> bool {
    inside(layout.body, x, y)
}

fn inside(box_: [f32; 4], x: f32, y: f32) -> bool {
    x >= box_[0] && x < box_[2] && y >= box_[1] && y < box_[3]
}

/// A square of `size` centred in `box_`, on integral pixels.
fn centred_in(box_: [f32; 4], size: f32) -> [f32; 4] {
    let x = (box_[0] + (box_[2] - box_[0] - size) / 2.0).round();
    let y = (box_[1] + (box_[3] - box_[1] - size) / 2.0).round();
    [x, y, x + size, y + size]
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: [f32; 4] = [960.0, 88.0, 1920.0, 1150.0];

    /// PIN (§7.7 ④) — **the card is centred on the seat's body and stacks its
    /// four rows in one order**: mark, sentence, fact, verb.
    ///
    /// MUTATION: hold the fact's gap open when there is no fact and the last
    /// assertion goes red — a card that reserves room for a line it has not got
    /// is a card with a hole in it.
    #[test]
    fn a_sheet_stacks_its_rows_and_stands_in_the_middle_of_the_body() {
        let laid = lay_out(BODY, 180.0, 1, true, 1.0);
        assert!(laid.mark[3] <= laid.say[0][1]);
        assert!(laid.say[0][3] <= laid.detail[1]);
        assert!(laid.detail[3] <= laid.verb[1]);
        // Inside the body, and centred in it.
        assert!(laid.frame[1] >= BODY[1] && laid.frame[3] <= BODY[3]);
        let card_middle = (laid.frame[1] + laid.frame[3]) / 2.0;
        let body_middle = (BODY[1] + BODY[3]) / 2.0;
        assert!((card_middle - body_middle).abs() < 1.5);
        let centre = (BODY[0] + BODY[2]) / 2.0;
        assert!(((laid.frame[0] + laid.frame[2]) / 2.0 - centre).abs() < 1.5);
        // No fact, no line and no gap held open for one.
        let plain = lay_out(BODY, 180.0, 1, false, 1.0);
        assert_eq!(plain.detail[3] - plain.detail[1], 0.0);
        assert!(plain.frame[3] - plain.frame[1] < laid.frame[3] - laid.frame[1]);
    }

    /// PIN (§7.7 ④) — **a sentence that wraps makes the card taller and moves
    /// nothing else out of order.**
    ///
    /// The download's sentence is two sentences long in the ruling's own table,
    /// so the card has to hold more than one line: at one line it was cut in
    /// half on the real window (2026-08-22).
    #[test]
    fn a_wrapped_sentence_makes_the_card_taller_and_keeps_its_order() {
        let one = lay_out(BODY, 180.0, 1, true, 1.0);
        let two = lay_out(BODY, 180.0, 2, true, 1.0);
        assert_eq!(one.say.len(), 1);
        assert_eq!(two.say.len(), 2);
        assert!(two.frame[3] - two.frame[1] > one.frame[3] - one.frame[1]);
        assert!(two.say[0][3] <= two.say[1][1], "the lines stack");
        assert!(two.say[1][3] <= two.detail[1], "and the fact follows them");
    }

    /// PIN (§7.7 ④) — **the verb answers and the scrim swallows.**
    ///
    /// A press on the scrim is not a dismissal: the one thing a reader must not
    /// be able to lose by accident is the only notice saying a file they asked
    /// for did not arrive. Escape is the way out and it is the only one.
    ///
    /// MUTATION: let `covers` answer `false` outside the card and a press beside
    /// the sheet reaches the page under it, which is a scrim that is not one.
    #[test]
    fn the_verb_answers_and_the_scrim_swallows() {
        let laid = lay_out(BODY, 180.0, 1, true, 1.0);
        let seat = SeatId(2);
        let centre = |box_: [f32; 4]| ((box_[0] + box_[2]) / 2.0, (box_[1] + box_[3]) / 2.0);
        let (x, y) = centre(laid.verb);
        assert_eq!(
            hit(&laid, seat, x, y),
            Some(ChromeTarget::PreviewFaultVerb(seat))
        );
        assert!(covers(&laid, x, y));
        // The card's own ground, above the verb: no target, still covered.
        let (x, y) = centre(laid.mark);
        assert_eq!(hit(&laid, seat, x, y), None);
        assert!(covers(&laid, x, y));
        // The scrim, well clear of the card: no target, still covered.
        let corner = (BODY[0] + 4.0, BODY[1] + 4.0);
        assert_eq!(hit(&laid, seat, corner.0, corner.1), None);
        assert!(covers(&laid, corner.0, corner.1));
        // And outside the seat's body it is nobody's.
        assert!(!covers(&laid, BODY[0] - 4.0, BODY[1] + 4.0));
    }

    /// RED (gesture audit 2026-08-26, 丙6) — **the card carries an `×`.**
    ///
    /// Every close a reader has met in this window — the toast's, the notice
    /// strip's, a floating window's — is an `×` in a corner, and this card had
    /// none: the three ways a reader would try to put it away (press the `×`,
    /// press outside it, press the card) were all nothing, and the one that
    /// works was a key nobody was told about. The module's own header said so
    /// in as many words: *"Esc puts it away, and it is the only one."*
    ///
    /// It is safe here and nowhere else among the five failure states, and for
    /// the reason the Escape was: there is a page underneath to come back to.
    /// The other four *are* the seat, and a close on one of those would leave
    /// the black hole a hidden WebView draws.
    ///
    /// MUTATION: put the `×` where the mark is and the last assertion goes red
    /// — a close that covers the card's own drawing is a second verb.
    #[test]
    fn the_card_carries_a_close_in_its_corner() {
        let laid = lay_out(BODY, 180.0, 2, true, 1.0);
        let seat = SeatId(2);
        let centre = |box_: [f32; 4]| ((box_[0] + box_[2]) / 2.0, (box_[1] + box_[3]) / 2.0);
        // In the card, in its top-right corner, and clear of the card's edge.
        assert!(laid.close[0] > laid.frame[0] && laid.close[2] < laid.frame[2]);
        assert!(laid.close[1] > laid.frame[1] && laid.close[3] < laid.frame[3]);
        assert!(laid.frame[2] - laid.close[2] < (laid.close[2] - laid.frame[0]) / 2.0);
        // It answers, and the verb still answers for itself.
        let (x, y) = centre(laid.close);
        assert_eq!(
            hit(&laid, seat, x, y),
            Some(ChromeTarget::PreviewSheetClose(seat))
        );
        assert!(covers(&laid, x, y));
        let (x, y) = centre(laid.verb);
        assert_eq!(
            hit(&laid, seat, x, y),
            Some(ChromeTarget::PreviewFaultVerb(seat))
        );
        // And it stands beside the card's drawing rather than over it.
        let overlaps =
            |a: [f32; 4], b: [f32; 4]| a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];
        assert!(!overlaps(laid.close, laid.mark));
        assert!(!overlaps(laid.close, laid.say[0]));
    }

    /// PIN — **the sentence is wrapped to the card's own column**, not to the
    /// seat: a card sized by its longest error message is a card that changes
    /// shape per failure.
    #[test]
    fn the_sentence_is_wrapped_to_the_card_and_not_to_the_seat() {
        let column = say_width(BODY, 1.0);
        let laid = lay_out(BODY, 180.0, 1, true, 1.0);
        assert!(column < BODY[2] - BODY[0]);
        assert!((column - (laid.say[0][2] - laid.say[0][0])).abs() < 1.0);
        // A narrow seat gives a narrower column rather than overflowing it.
        let narrow = [0.0, 0.0, 300.0, 400.0];
        assert!(say_width(narrow, 1.0) < column);
    }
}
