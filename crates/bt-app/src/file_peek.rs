//! **The glance card over a file row** — `.file-peek` (P143-P150, mock-up
//! 1762-1795 / 6367-6427 / 8845-8882, §7.1.3 附则, user ruling 2026-07-17).
//!
//! # What it is, and the one sentence that decides everything else
//!
//! "A read-only glance card over a FILE row. **Non-interactive by construction
//! (`pointer-events: none`): it can neither trap the pointer nor flicker;
//! wanting to interact IS the signal to Enter/double-click into the real preview
//! pane.**" (1763-1766.)
//!
//! Every rule below falls out of that. It is not a small preview pane and not a
//! preview float — it is a *third* form, and the seed's guess that it would land
//! on the float chassis is corrected by the mock-up itself (S1's erratum). It
//! has no hit test at all: [`Runtime::file_peek_layer`] builds a drawing and
//! nothing in this window ever asks what is under it, which is the strongest
//! reading of `pointer-events: none` available in a build that owns its own
//! pointer routing. A card that cannot be pressed cannot flicker between "the
//! row is hovered" and "the card is hovered", and that oscillation is the entire
//! failure mode this shape exists to make impossible.
//!
//! # What it borrows rather than re-decides
//!
//! * **The type judgement.** `previewFtype` and the refusal for unknown types
//!   are the preview pane's ([`crate::preview::preview_ftype`]), asked here
//!   rather than copied — "reuses the preview pane's judgement — same ftype
//!   rules, same refusal for unknown types" (6368-6370).
//! * **The buffer.** When the file is already open, the glance shows *the tab's
//!   pool buffer*, dirty dot and all, "so the glance never lies about unsaved
//!   edits" (6370-6372).
//! * **The intent.** 350ms, "the tab-peek constant" — the same number the layout
//!   peek and the files flyout arm on.
//!
//! # The numbers
//!
//! All of them are the mock-up's, copied here because this is the only place
//! that draws them and a constant with one reader belongs beside it.

use bt_render::{ChromeLabel, ChromeLabelWeight, ChromePalette, OverlayQuad};

use crate::marks::{ChromeMark, ChromeSprite, OverlayLayer};
use crate::settings::push_float_window;

/// How long the pointer must rest on a row before the card comes up — "350ms
/// intent (the tab-peek constant)" (6367).
pub const PEEK_INTENT_MS: u64 = 350;

/// `.file-peek { width: 300px }`.
pub const PEEK_WIDTH_LOGICAL_PX: f32 = 300.0;
/// `.file-peek { max-height: 264px }` — and it is a *max*, not a height: the
/// card shrink-wraps its head, its body and its foot and stops here.
pub const PEEK_MAX_HEIGHT_LOGICAL_PX: f32 = 264.0;
/// `.file-peek { border-radius: 8px }`.
pub const PEEK_RADIUS_LOGICAL_PX: f32 = 8.0;
/// `.file-peek { border: 1px solid var(--border) }`.
pub const PEEK_BORDER_LOGICAL_PX: f32 = 1.0;
/// `box-shadow: 0 10px 28px` — the spread half, which is what this renderer's
/// halo takes.
pub const PEEK_SHADOW_LOGICAL_PX: f32 = 28.0;

/// `.fpeek-head { padding: 7px 10px 5px }` — the three sides that differ.
pub const PEEK_HEAD_PADDING_TOP_LOGICAL_PX: f32 = 7.0;
pub const PEEK_HEAD_PADDING_X_LOGICAL_PX: f32 = 10.0;
pub const PEEK_HEAD_PADDING_BOTTOM_LOGICAL_PX: f32 = 5.0;
/// `.fpeek-head { font-size: 11.5px }`.
pub const PEEK_HEAD_FONT_LOGICAL_PX: f32 = 11.5;
/// `.fpeek-head { gap: 6px }`.
pub const PEEK_HEAD_GAP_LOGICAL_PX: f32 = 6.0;
/// `.pmark { width: 15px }` (mock-up 246), which is the head's file mark.
pub const PEEK_MARK_LOGICAL_PX: f32 = 15.0;
/// `.fpeek-head .dirty { font-size: 9px }` — the unsaved-edits dot.
pub const PEEK_DIRTY_FONT_LOGICAL_PX: f32 = 9.0;
/// The slot that dot is given, so a name does not reflow when it appears.
pub const PEEK_DIRTY_SLOT_LOGICAL_PX: f32 = 10.0;
/// `.fpeek-type { font-size: 10px }`.
pub const PEEK_TYPE_FONT_LOGICAL_PX: f32 = 10.0;
/// `.fpeek-type { padding: 0 5px }`.
pub const PEEK_TYPE_PADDING_X_LOGICAL_PX: f32 = 5.0;
/// `.fpeek-type { border-radius: 4px }`.
pub const PEEK_TYPE_RADIUS_LOGICAL_PX: f32 = 4.0;

/// `.fpeek-body { padding: 2px 10px 8px }`.
pub const PEEK_BODY_PADDING_TOP_LOGICAL_PX: f32 = 2.0;
pub const PEEK_BODY_PADDING_X_LOGICAL_PX: f32 = 10.0;
pub const PEEK_BODY_PADDING_BOTTOM_LOGICAL_PX: f32 = 8.0;
/// `.fpeek-body { font: 11px/1.55 "Cascadia Mono" … }` — the size.
pub const PEEK_BODY_FONT_LOGICAL_PX: f32 = 11.0;
/// …and the line height it is set solid against.
pub const PEEK_BODY_LINE_HEIGHT: f32 = 1.55;
/// How many lines of a text body the card shows before it says `…` (6410).
pub const PEEK_BODY_LINES: usize = 14;

/// `.fpeek-none { padding: 14px 10px 12px }`.
pub const PEEK_NONE_PADDING_TOP_LOGICAL_PX: f32 = 14.0;
pub const PEEK_NONE_PADDING_X_LOGICAL_PX: f32 = 10.0;
pub const PEEK_NONE_PADDING_BOTTOM_LOGICAL_PX: f32 = 12.0;
/// `.fpeek-none { font-size: 11px }`.
pub const PEEK_NONE_FONT_LOGICAL_PX: f32 = 11.0;

/// The picture placeholder's box: `<svg viewBox="0 0 280 120" width="280"
/// height="120">` (6402).
pub const PEEK_IMAGE_W_LOGICAL_PX: f32 = 280.0;
pub const PEEK_IMAGE_H_LOGICAL_PX: f32 = 120.0;
/// `<rect rx="6">` — the placeholder's own corner.
pub const PEEK_IMAGE_RADIUS_LOGICAL_PX: f32 = 6.0;

/// `.fpeek-foot { padding: 5px 10px }`.
pub const PEEK_FOOT_PADDING_Y_LOGICAL_PX: f32 = 5.0;
pub const PEEK_FOOT_PADDING_X_LOGICAL_PX: f32 = 10.0;
/// `.fpeek-foot { font-size: 10px }`.
pub const PEEK_FOOT_FONT_LOGICAL_PX: f32 = 10.0;
/// `.fpeek-foot { border-top: 1px solid var(--border-soft) }`.
pub const PEEK_FOOT_RULE_LOGICAL_PX: f32 = 1.0;

/// How far to the right of the row the card stands (6413).
pub const PEEK_ROW_GAP_LOGICAL_PX: f32 = 10.0;
/// How far *up* from the row's top it is hung, so the card reads as belonging to
/// the row rather than hanging off it (`r.top - 8`).
pub const PEEK_ROW_RISE_LOGICAL_PX: f32 = 8.0;
/// The viewport safety margin the placement clamps to, on every side (6414-6420).
pub const PEEK_VIEWPORT_MARGIN_LOGICAL_PX: f32 = 8.0;

/// **The fixed sentence along the bottom** (6421).
///
/// It never varies, and that is the point: the card exists to say "there is more
/// of this behind a real gesture", and a foot whose words changed with the file
/// would be a second thing to read.
pub const PEEK_FOOT_TEXT: &str = "Enter / double-click opens the preview pane";

/// **The refusal** (6406) — the same sentence the preview pane's unknown card
/// says, said in one line.
pub const PEEK_UNKNOWN_TEXT: &str = "No preview — binary or unrecognized type.";

/// What the card's middle is showing.
///
/// Three shapes, and they are the three the preview pane already has: a picture,
/// a refusal, or lines of text. There is deliberately no "loading" variant — a
/// glance whose body has not arrived draws no lines, which is the same picture
/// as an empty file and is over in a few milliseconds either way. A word that
/// appears for two frames and is replaced is worse than the space it occupied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeekBody {
    /// The head of the file, already cut to [`PEEK_BODY_LINES`] and with the
    /// `…` appended if it was cut.
    Lines(Vec<String>),
    /// A picture: the placeholder, at its own size.
    Image,
    /// A file this window will not read.
    Refused,
}

impl PeekBody {
    /// How tall this body is, in physical pixels.
    fn height(&self, scale: f32) -> f32 {
        let px = |logical: f32| logical * scale;
        match self {
            Self::Lines(lines) => {
                let line = (px(PEEK_BODY_FONT_LOGICAL_PX) * PEEK_BODY_LINE_HEIGHT).round();
                px(PEEK_BODY_PADDING_TOP_LOGICAL_PX)
                    + line * lines.len() as f32
                    + px(PEEK_BODY_PADDING_BOTTOM_LOGICAL_PX)
            }
            Self::Image => {
                px(PEEK_BODY_PADDING_TOP_LOGICAL_PX)
                    + px(PEEK_IMAGE_H_LOGICAL_PX)
                    + px(PEEK_BODY_PADDING_BOTTOM_LOGICAL_PX)
            }
            Self::Refused => {
                let line = (px(PEEK_NONE_FONT_LOGICAL_PX) * 1.4).round();
                px(PEEK_NONE_PADDING_TOP_LOGICAL_PX)
                    + line
                    + px(PEEK_NONE_PADDING_BOTTOM_LOGICAL_PX)
            }
        }
    }
}

/// Everything the card says, before it is placed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeekContent {
    pub name: String,
    /// The ftype string, printed as it stands — the mock-up prints the word
    /// itself into `.fpeek-type` (6422) rather than a prettified one, because
    /// the chip is a *type*, and the type is what the preview pane calls it.
    pub ftype: String,
    /// Whether the tab's pool holds this file with unsaved edits (P147).
    pub dirty: bool,
    pub body: PeekBody,
}

/// The card, laid out.
#[derive(Clone, Debug, PartialEq)]
pub struct PeekLayout {
    pub frame: [f32; 4],
    pub mark: [f32; 4],
    pub name: [f32; 4],
    /// `None` when the buffer is clean or is not in the pool at all.
    pub dirty: Option<[f32; 4]>,
    pub ftype: [f32; 4],
    /// The body's own box, and what goes in it.
    pub body: [f32; 4],
    pub body_kind: PeekBody,
    pub foot: [f32; 4],
}

/// Lay the card out beside `row`, inside `window`.
///
/// **The placement is the mock-up's, in its own order** (6413-6420): to the
/// right of the row with a 10px gap; flipped to the *left* when there is no room
/// there; and vertically hung 8px above the row's own top, clamped to an 8px
/// margin at both ends of the viewport. Main-axis flip, cross-axis clamp — the
/// rule `M2-tiny-window-priority.md` §3.3 asks every floating thing in this
/// window to follow, and the same one [`crate::float::float_placement`] obeys.
///
/// `name_width` and `ftype_width` are measured by the caller, beside the
/// renderer, exactly as the tip's and the ghost's are: only the font knows how
/// wide a line is.
#[must_use]
pub fn layout(
    content: &PeekContent,
    row: [f32; 4],
    window: (f32, f32),
    name_width: f32,
    ftype_width: f32,
    scale: f32,
) -> PeekLayout {
    let px = |logical: f32| logical * scale;
    let margin = px(PEEK_VIEWPORT_MARGIN_LOGICAL_PX);
    let border = px(PEEK_BORDER_LOGICAL_PX).max(1.0).round();
    let width = px(PEEK_WIDTH_LOGICAL_PX).round();

    let head_line = px(PEEK_HEAD_FONT_LOGICAL_PX) * 1.4;
    let head_height = (px(PEEK_HEAD_PADDING_TOP_LOGICAL_PX)
        + head_line.max(px(PEEK_MARK_LOGICAL_PX))
        + px(PEEK_HEAD_PADDING_BOTTOM_LOGICAL_PX))
    .round();
    let foot_height =
        (px(PEEK_FOOT_PADDING_Y_LOGICAL_PX) * 2.0 + px(PEEK_FOOT_FONT_LOGICAL_PX) * 1.4).round();
    let body_height = content.body.height(scale).round();
    // `max-height: 264px; overflow: hidden` — the card shrink-wraps and then
    // stops. The body is what gives way, because the head names the file and the
    // foot says how to open it: both are the card's *sentence*, and a card that
    // cut them would have shown a document with nothing saying what it was.
    let natural = border * 2.0 + head_height + body_height + foot_height;
    let height = natural.min(px(PEEK_MAX_HEIGHT_LOGICAL_PX).round());
    let body_height = (height - border * 2.0 - head_height - foot_height).max(0.0);

    let right = row[2] + px(PEEK_ROW_GAP_LOGICAL_PX);
    let left = if right + width > window.0 - margin {
        (row[0] - width - px(PEEK_ROW_GAP_LOGICAL_PX)).max(margin)
    } else {
        right
    }
    .round();
    let top = (row[1] - px(PEEK_ROW_RISE_LOGICAL_PX))
        .clamp(margin, (window.1 - height - margin).max(margin))
        .round();
    let frame = [left, top, left + width, top + height];

    let head_top = frame[1] + border + px(PEEK_HEAD_PADDING_TOP_LOGICAL_PX);
    let head_left = frame[0] + border + px(PEEK_HEAD_PADDING_X_LOGICAL_PX);
    let head_right = frame[2] - border - px(PEEK_HEAD_PADDING_X_LOGICAL_PX);
    let mark_size = px(PEEK_MARK_LOGICAL_PX);
    let mark = [
        head_left,
        (head_top + (head_line - mark_size) / 2.0).round(),
        head_left + mark_size,
        (head_top + (head_line + mark_size) / 2.0).round(),
    ];
    let name_left = mark[2] + px(PEEK_HEAD_GAP_LOGICAL_PX);
    // The chip is `margin-left: auto`, so it is hung off the right edge and the
    // name takes whatever is left. A name too long for what is left is clipped
    // by its own box rather than pushing the chip out of the card.
    let chip_width = (ftype_width + px(PEEK_TYPE_PADDING_X_LOGICAL_PX) * 2.0).round();
    let ftype = [
        head_right - chip_width,
        head_top,
        head_right,
        head_top + head_line,
    ];
    let dirty_slot = px(PEEK_DIRTY_SLOT_LOGICAL_PX);
    let name_room = (ftype[0] - px(PEEK_HEAD_GAP_LOGICAL_PX) - name_left).max(0.0);
    let name_width = name_width.min(if content.dirty {
        (name_room - dirty_slot - px(PEEK_HEAD_GAP_LOGICAL_PX)).max(0.0)
    } else {
        name_room
    });
    let name = [
        name_left,
        head_top,
        name_left + name_width,
        head_top + head_line,
    ];
    let dirty = content.dirty.then(|| {
        let left = name[2] + px(PEEK_HEAD_GAP_LOGICAL_PX);
        [left, head_top, left + dirty_slot, head_top + head_line]
    });

    let body_top = frame[1] + border + head_height;
    let body = [
        frame[0] + border,
        body_top,
        frame[2] - border,
        body_top + body_height,
    ];
    let foot = [
        frame[0] + border,
        frame[3] - border - foot_height,
        frame[2] - border,
        frame[3] - border,
    ];
    PeekLayout {
        frame,
        mark,
        name,
        dirty,
        ftype,
        body,
        body_kind: content.body.clone(),
        foot,
    }
}

/// Paint the card — one layer, above the pinned float (P143: "z-index above the
/// pinned flyout (60) — flyout rows peek too").
#[must_use]
pub fn build(
    layout: &PeekLayout,
    content: &PeekContent,
    palette: &ChromePalette,
    scale: f32,
) -> OverlayLayer {
    let px = |logical: f32| logical * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let mut quads: Vec<OverlayQuad> = Vec::new();

    push_float_window(
        &mut quads,
        layout.frame,
        px(PEEK_RADIUS_LOGICAL_PX),
        px(PEEK_BORDER_LOGICAL_PX).max(1.0).round(),
        px(PEEK_SHADOW_LOGICAL_PX),
        palette.menu_surface,
        palette.menu_shadow,
        alpha(palette.tip_shadow_inner_alpha),
        alpha(palette.tip_shadow_outer_alpha),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
    );

    let mut labels: Vec<ChromeLabel> = Vec::new();
    let mut sprites = vec![ChromeSprite::new(
        ChromeMark::File,
        layout.mark,
        palette.accent,
    )];

    let label = |text: &str, rect: [f32; 4], size: f32, color: [u8; 3]| ChromeLabel {
        text: text.to_owned(),
        rect,
        clip: None,
        font_size_px: size,
        color,
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
    };

    labels.push(label(
        &content.name,
        layout.name,
        px(PEEK_HEAD_FONT_LOGICAL_PX),
        palette.menu_item_text,
    ));
    if let Some(rect) = layout.dirty {
        // `●` in the accent, the same glyph and the same ink the preview head and
        // the switcher print for an unsaved buffer — one dot means one thing.
        labels.push(label(
            "●",
            rect,
            px(PEEK_DIRTY_FONT_LOGICAL_PX),
            palette.accent,
        ));
    }
    // The chip: a hairline box with the type word inside it.
    quads.extend(bt_render::rounded_overlay_fill(
        layout.ftype,
        px(PEEK_TYPE_RADIUS_LOGICAL_PX),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
    ));
    quads.extend(bt_render::rounded_overlay_fill(
        [
            layout.ftype[0] + 1.0,
            layout.ftype[1] + 1.0,
            layout.ftype[2] - 1.0,
            layout.ftype[3] - 1.0,
        ],
        (px(PEEK_TYPE_RADIUS_LOGICAL_PX) - 1.0).max(0.0),
        palette.menu_surface,
        1.0,
    ));
    labels.push(ChromeLabel {
        align_center: true,
        ..label(
            &content.ftype,
            layout.ftype,
            px(PEEK_TYPE_FONT_LOGICAL_PX),
            palette.body_hint_text,
        )
    });

    match &layout.body_kind {
        PeekBody::Lines(lines) => {
            let line_height = (px(PEEK_BODY_FONT_LOGICAL_PX) * PEEK_BODY_LINE_HEIGHT).round();
            let left = layout.body[0] + px(PEEK_BODY_PADDING_X_LOGICAL_PX);
            let right = layout.body[2] - px(PEEK_BODY_PADDING_X_LOGICAL_PX);
            let top = layout.body[1] + px(PEEK_BODY_PADDING_TOP_LOGICAL_PX);
            for (index, text) in lines.iter().enumerate() {
                let row_top = top + line_height * index as f32;
                // `overflow: hidden` — a card that was cut by its max-height
                // stops drawing where the box stops rather than spilling onto
                // the foot.
                if row_top + line_height > layout.body[3] {
                    break;
                }
                labels.push(ChromeLabel {
                    // `white-space: pre` — no wrapping, and a long line runs out
                    // of the card rather than reflowing it.
                    clip: Some(layout.body),
                    ..label(
                        text,
                        [left, row_top, right, row_top + line_height],
                        px(PEEK_BODY_FONT_LOGICAL_PX),
                        palette.menu_item_text,
                    )
                });
            }
        }
        PeekBody::Image => {
            let width = px(PEEK_IMAGE_W_LOGICAL_PX).min(layout.body[2] - layout.body[0]);
            let left = (layout.body[0] + layout.body[2] - width) / 2.0;
            let top = layout.body[1] + px(PEEK_BODY_PADDING_TOP_LOGICAL_PX);
            let bottom = (top + px(PEEK_IMAGE_H_LOGICAL_PX)).min(layout.body[3]);
            quads.extend(bt_render::rounded_overlay_fill(
                [left, top, left + width, bottom],
                px(PEEK_IMAGE_RADIUS_LOGICAL_PX),
                // `fill: var(--termbg)` — the placeholder is a window onto the
                // picture's own ground, which is the terminal's.
                bt_render::background_rgb(),
                1.0,
            ));
        }
        PeekBody::Refused => {
            let left = layout.body[0] + px(PEEK_NONE_PADDING_X_LOGICAL_PX);
            let right = layout.body[2] - px(PEEK_NONE_PADDING_X_LOGICAL_PX);
            let top = layout.body[1] + px(PEEK_NONE_PADDING_TOP_LOGICAL_PX);
            labels.push(label(
                PEEK_UNKNOWN_TEXT,
                [left, top, right, layout.body[3]],
                px(PEEK_NONE_FONT_LOGICAL_PX),
                palette.body_hint_text,
            ));
        }
    }

    // `border-top: 1px solid var(--border-soft)` over the foot.
    quads.push(OverlayQuad {
        rect: [
            layout.foot[0],
            layout.foot[1],
            layout.foot[2],
            layout.foot[1] + px(PEEK_FOOT_RULE_LOGICAL_PX).max(1.0).round(),
        ],
        color: palette.menu_border,
        alpha: alpha(palette.menu_border_alpha),
    });
    labels.push(label(
        PEEK_FOOT_TEXT,
        [
            layout.foot[0] + px(PEEK_FOOT_PADDING_X_LOGICAL_PX),
            layout.foot[1] + px(PEEK_FOOT_PADDING_Y_LOGICAL_PX),
            layout.foot[2] - px(PEEK_FOOT_PADDING_X_LOGICAL_PX),
            layout.foot[3],
        ],
        px(PEEK_FOOT_FONT_LOGICAL_PX),
        palette.body_hint_text,
    ));

    OverlayLayer {
        quads,
        labels,
        sprites: std::mem::take(&mut sprites),
        ..OverlayLayer::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALE: f32 = 1.0;

    fn content(body: PeekBody) -> PeekContent {
        PeekContent {
            name: "main.rs".to_owned(),
            ftype: "text".to_owned(),
            dirty: false,
            body,
        }
    }

    fn lines(count: usize) -> PeekBody {
        PeekBody::Lines((0..count).map(|index| format!("line {index}")).collect())
    }

    /// PIN — **P148: the card stands to the right of its row, and flips to the
    /// left rather than running off the screen.**
    ///
    /// Main axis flips, cross axis clamps. The 8px margin is a *safety* margin
    /// and the flip is what keeps the card attached to the row it is about — a
    /// card hauled to the viewport edge by a clamp alone has stopped pointing at
    /// anything.
    ///
    /// Mutation: drop the `right + width > window - margin` test and always
    /// place to the right — the second assertion comes back at 1190 and the card
    /// is half off the screen.
    #[test]
    fn the_card_stands_beside_its_row_and_flips_when_there_is_no_room() {
        let window = (1200.0, 800.0);
        let card = content(lines(3));
        let roomy = layout(
            &card,
            [40.0, 300.0, 240.0, 320.0],
            window,
            60.0,
            24.0,
            SCALE,
        );
        assert_eq!(
            roomy.frame[0], 250.0,
            "ten pixels to the right of the row's own right edge"
        );

        let tight = layout(
            &card,
            [900.0, 300.0, 1180.0, 320.0],
            window,
            60.0,
            24.0,
            SCALE,
        );
        assert_eq!(
            tight.frame[0],
            900.0 - PEEK_WIDTH_LOGICAL_PX - PEEK_ROW_GAP_LOGICAL_PX,
            "no room on the right, so it stands on the left of the row"
        );
        assert!(
            tight.frame[2] <= window.0 - PEEK_VIEWPORT_MARGIN_LOGICAL_PX,
            "and inside the viewport either way"
        );
    }

    /// PIN — the 8px margin holds at both ends of the vertical axis.
    ///
    /// A row at the very top of a tree would hang the card above the window, and
    /// a row at the very bottom would hang it below: the clamp is what makes a
    /// glance over the first row and a glance over the last row both readable.
    ///
    /// Mutation: replace the `clamp` with the bare `row.top - 8` — the top case
    /// comes back at -8 and the bottom case runs off the foot of the window.
    #[test]
    fn the_card_keeps_eight_pixels_of_air_at_both_ends() {
        let window = (1200.0, 400.0);
        let card = content(lines(14));
        let high = layout(&card, [40.0, 2.0, 240.0, 22.0], window, 60.0, 24.0, SCALE);
        assert_eq!(high.frame[1], PEEK_VIEWPORT_MARGIN_LOGICAL_PX);
        let low = layout(
            &card,
            [40.0, 380.0, 240.0, 398.0],
            window,
            60.0,
            24.0,
            SCALE,
        );
        assert!(
            low.frame[3] <= window.1 - PEEK_VIEWPORT_MARGIN_LOGICAL_PX,
            "the foot of the card is inside the foot of the window: {:?}",
            low.frame
        );
    }

    /// PIN — **the card never grows past 264px, and the head and the foot are
    /// never what gets cut.**
    ///
    /// `max-height: 264px; overflow: hidden` on a flex column whose head and
    /// foot are both `flex: none`. The head names the file and the foot says how
    /// to open it — a card that had cut either would be showing a document with
    /// nothing to say what it was or what to do about it.
    ///
    /// Mutation: let the body take its natural height without the `min` — a
    /// fourteen-line card comes back over the cap and the assertion fails.
    #[test]
    fn a_long_body_is_cut_and_the_sentence_around_it_is_not() {
        let window = (1200.0, 900.0);
        let long = layout(
            &content(lines(PEEK_BODY_LINES)),
            [40.0, 300.0, 240.0, 320.0],
            window,
            60.0,
            24.0,
            SCALE,
        );
        assert!(
            long.frame[3] - long.frame[1] <= PEEK_MAX_HEIGHT_LOGICAL_PX,
            "the card stops at its cap: {:?}",
            long.frame
        );
        assert!(
            long.foot[3] > long.foot[1],
            "the foot is still a foot inside the cut card"
        );
        assert!(
            long.name[3] > long.name[1] && long.mark[3] > long.mark[1],
            "and the head still names the file"
        );

        // A short body shrink-wraps instead: the cap is a maximum, not a height.
        let short = layout(
            &content(lines(2)),
            [40.0, 300.0, 240.0, 320.0],
            window,
            60.0,
            24.0,
            SCALE,
        );
        assert!(
            short.frame[3] - short.frame[1] < long.frame[3] - long.frame[1],
            "two lines is a smaller card than fourteen"
        );
    }

    /// PIN — the dot has a slot of its own, and the name gives way to it.
    ///
    /// P147's dirty dot appears *between* the name and the type chip. Without a
    /// reserved slot the name would be measured against the whole gap and the
    /// dot would be drawn over its last letters, which is the one place in this
    /// card where two things can collide.
    ///
    /// Mutation: take the `content.dirty` branch out of the `name_width` min —
    /// the dirty name's box comes back the same width as the clean one and the
    /// dot lands on top of it.
    #[test]
    fn an_unsaved_buffer_gets_a_dot_and_the_name_makes_room_for_it() {
        let window = (1200.0, 900.0);
        let row = [40.0, 300.0, 240.0, 320.0];
        let clean = layout(&content(lines(2)), row, window, 400.0, 24.0, SCALE);
        let mut dirty_card = content(lines(2));
        dirty_card.dirty = true;
        let dirty = layout(&dirty_card, row, window, 400.0, 24.0, SCALE);

        assert!(clean.dirty.is_none(), "a clean buffer says nothing");
        let dot = dirty.dirty.expect("an unsaved buffer shows its dot");
        assert!(
            dirty.name[2] < clean.name[2],
            "the name gave the dot its room"
        );
        assert!(
            dot[0] >= dirty.name[2],
            "and the dot stands after the name, not on it"
        );
        assert!(dot[2] <= dirty.ftype[0], "and before the type chip");
    }
}
