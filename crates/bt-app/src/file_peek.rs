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
//! * **The document itself** (user ruling, 2026-08-13: *"what the preview on the
//!   right looks like is what the hover preview looks like, only read-only"*).
//!   The mock-up's card prints fourteen lines of plain text whatever the file is,
//!   and P147 copied that; the ruling overturns it on the same ground the
//!   markdown renderer was grown on — a glance that renders a table as raw commas
//!   is a glance that lies about what opening the file will show. So the card's
//!   middle is a [`bt_render::PreviewBody`] built by
//!   [`crate::PreviewSurface::Peek`] going down the *pane's* pipeline: markdown
//!   renders, csv is a table, a diff has its three inks, text folds. What is left
//!   of "read-only" is real and is the whole of the difference — see
//!   [`PeekBody::Document`].
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
///
/// **A picture's frame only**, since the 2026-08-13 ruling. A document brings the
/// preview pane's own padding with it (`.pv-md`'s 12/16, a mono body's own), and
/// laying the card's 2/10/8 around *that* would be one inset stated twice — the
/// card would read as a document in a box inside a box.
pub const PEEK_BODY_PADDING_TOP_LOGICAL_PX: f32 = 2.0;
pub const PEEK_BODY_PADDING_X_LOGICAL_PX: f32 = 10.0;
pub const PEEK_BODY_PADDING_BOTTOM_LOGICAL_PX: f32 = 8.0;

/// `.fpeek-none { padding: 14px 10px 12px }`.
pub const PEEK_NONE_PADDING_TOP_LOGICAL_PX: f32 = 14.0;
pub const PEEK_NONE_PADDING_X_LOGICAL_PX: f32 = 10.0;
pub const PEEK_NONE_PADDING_BOTTOM_LOGICAL_PX: f32 = 12.0;
/// `.fpeek-none { font-size: 11px }`.
pub const PEEK_NONE_FONT_LOGICAL_PX: f32 = 11.0;

/// The box a picture is fitted into: `<svg viewBox="0 0 280 120" width="280"
/// height="120">` (6402), which was the placeholder's size and is now the real
/// thumbnail's bound.
///
/// Fitted rather than filled — the picture keeps its own proportions and is
/// centred in whatever is left over, so a tall image and a wide one are both
/// themselves rather than both this rectangle.
pub const PEEK_IMAGE_W_LOGICAL_PX: f32 = 280.0;
pub const PEEK_IMAGE_H_LOGICAL_PX: f32 = 120.0;
/// `<rect rx="6">` — the corner of the ground the picture stands on.
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
/// Three shapes, and they are the three the preview pane already has: a document,
/// a picture, or a refusal. There is deliberately no "loading" variant — a glance
/// whose body has not arrived draws an empty document, which is the same picture
/// as an empty file and is over in a few milliseconds either way. A word that
/// appears for two frames and is replaced is worse than the space it occupied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PeekBody {
    /// **The preview pane's own body**, carrying how tall it came out in physical
    /// pixels.
    ///
    /// The document itself is not in here: it is a [`bt_render::PreviewBody`]
    /// built by the pane's pipeline and handed to the renderer on the card's
    /// [`crate::marks::OverlayLayer::body`] channel, the same door a preview
    /// float's document goes through. What this variant carries is the one thing
    /// the *card's* geometry needs from it — its height — so that a two-line file
    /// still gets a two-line card.
    ///
    /// **It does not scroll, and that is a decision rather than an omission.**
    /// The card is rendered from the head of the document and cut off at the
    /// card's own height. A glance is for *placing* a file — is this the README I
    /// meant, is this csv the one with the headers — and the head answers that;
    /// the foot's fixed sentence is the way to the rest of it. A scrollable card
    /// would also need to be a card the pointer can enter, and P143 says it can
    /// never be one.
    Document(f32),
    /// A picture, already fitted: its physical size inside
    /// [`PEEK_IMAGE_W_LOGICAL_PX`] × [`PEEK_IMAGE_H_LOGICAL_PX`], proportions
    /// intact.
    ///
    /// Zero-sized while the decode and the resample are in flight, which draws
    /// the ground and no picture — see [`crate::Runtime::file_peek_layer`].
    Image { width: f32, height: f32 },
    /// A file this window will not read.
    Refused,
}

impl PeekBody {
    /// How tall this body is, in physical pixels.
    fn height(&self, scale: f32) -> f32 {
        let px = |logical: f32| logical * scale;
        match self {
            // A document brings its own padding: see
            // `PEEK_BODY_PADDING_TOP_LOGICAL_PX`.
            Self::Document(height) => *height,
            // **The whole fit box, whatever the picture turned out to be.** The
            // card comes up at 350ms and the decode lands whenever it lands; a
            // body sized to the picture would be a card that changed height under
            // the pointer the instant the pixels arrived. So the box is reserved
            // and the picture is centred in it, which is what `object-fit:
            // contain` in a fixed frame does anyway.
            Self::Image { .. } => {
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

/// How wide the card's body box is, in physical pixels — the card less its two
/// hairlines.
///
/// Public because the document that goes in it has to be *laid out* at this
/// width before the card can be laid out at all: how tall a wrapped markdown page
/// is depends on how wide it is, and how tall the card is depends on the page.
/// The circle is broken here, at the one number that does not depend on either.
#[must_use]
pub fn body_width(scale: f32) -> f32 {
    let border = (PEEK_BORDER_LOGICAL_PX * scale).max(1.0).round();
    (PEEK_WIDTH_LOGICAL_PX * scale).round() - border * 2.0
}

/// The tallest the card's body can be: the cap, less the two hairlines and the
/// head and the foot that are never what gets cut (P147).
#[must_use]
pub fn body_max_height(scale: f32) -> f32 {
    let border = (PEEK_BORDER_LOGICAL_PX * scale).max(1.0).round();
    ((PEEK_MAX_HEIGHT_LOGICAL_PX * scale).round()
        - border * 2.0
        - head_height(scale)
        - foot_height(scale))
    .max(0.0)
}

/// `.fpeek-head`'s own height — its padding around the taller of its line and its
/// file mark.
fn head_height(scale: f32) -> f32 {
    let px = |logical: f32| logical * scale;
    (px(PEEK_HEAD_PADDING_TOP_LOGICAL_PX)
        + (px(PEEK_HEAD_FONT_LOGICAL_PX) * 1.4).max(px(PEEK_MARK_LOGICAL_PX))
        + px(PEEK_HEAD_PADDING_BOTTOM_LOGICAL_PX))
    .round()
}

/// `.fpeek-foot`'s own height.
fn foot_height(scale: f32) -> f32 {
    let px = |logical: f32| logical * scale;
    (px(PEEK_FOOT_PADDING_Y_LOGICAL_PX) * 2.0 + px(PEEK_FOOT_FONT_LOGICAL_PX) * 1.4).round()
}

/// The pixels behind a [`PeekBody::Image`], already resampled to the size the
/// card is going to draw them at.
///
/// Borrowed rather than owned because the card is rebuilt whenever the chrome is
/// and the raster is megabytes: the one `Arc` in the window's cache is cloned
/// once, into the draw list, and never for a layout.
pub struct PeekPicture<'a> {
    /// The display-sized texture identity — see
    /// [`bt_render::ChromeIcon::key`].
    pub key: &'a str,
    pub rgba: &'a std::sync::Arc<[u8]>,
    pub width_px: u32,
    pub height_px: u32,
}

/// The ground a picture stands on: the mock-up's own 280×120 frame, centred in
/// the card's body and cut by it if the card is short.
#[must_use]
fn picture_ground(body: [f32; 4], scale: f32) -> [f32; 4] {
    let px = |logical: f32| logical * scale;
    let width = px(PEEK_IMAGE_W_LOGICAL_PX)
        .min(body[2] - body[0] - px(PEEK_BODY_PADDING_X_LOGICAL_PX) * 2.0);
    let left = ((body[0] + body[2] - width) / 2.0).round();
    let top = body[1] + px(PEEK_BODY_PADDING_TOP_LOGICAL_PX);
    let bottom = (top + px(PEEK_IMAGE_H_LOGICAL_PX)).min(body[3]);
    [left, top.round(), left + width, bottom.round()]
}

/// Where the picture itself goes: its own size, centred on its ground.
///
/// `None` while there is nothing to draw — the decode and the resample are on
/// workers, and until they answer the card shows the ground and no picture. That
/// gap is short and silent by the same rule the document's is: a word for two
/// frames is worse than the space it occupied.
#[must_use]
pub fn picture_rect(layout: &PeekLayout, scale: f32) -> Option<[f32; 4]> {
    let PeekBody::Image { width, height } = layout.body_kind else {
        return None;
    };
    if !(width >= 1.0 && height >= 1.0) {
        return None;
    }
    let ground = picture_ground(layout.body, scale);
    let (width, height) = (
        width.min(ground[2] - ground[0]),
        height.min(ground[3] - ground[1]),
    );
    let left = ((ground[0] + ground[2] - width) / 2.0).round();
    let top = ((ground[1] + ground[3] - height) / 2.0).round();
    Some([left, top, left + width, top + height])
}

/// Everything the card says, before it is placed.
#[derive(Clone, Debug, PartialEq)]
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
    let head_height = head_height(scale);
    let foot_height = foot_height(scale);
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
        body_kind: content.body,
        foot,
    }
}

/// Paint the card — one layer, above the pinned float (P143: "z-index above the
/// pinned flyout (60) — flyout rows peek too").
#[must_use]
pub fn build(
    layout: &PeekLayout,
    content: &PeekContent,
    picture: Option<PeekPicture<'_>>,
    palette: &ChromePalette,
    scale: f32,
) -> OverlayLayer {
    let px = |logical: f32| logical * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let mut quads: Vec<OverlayQuad> = Vec::new();
    let mut images: Vec<bt_render::ChromeIcon> = Vec::new();

    push_float_window(
        &mut quads,
        layout.frame,
        px(PEEK_RADIUS_LOGICAL_PX),
        px(PEEK_BORDER_LOGICAL_PX).max(1.0).round(),
        px(PEEK_SHADOW_LOGICAL_PX),
        palette.menu_surface,
        palette.menu_shadow,
        // The card's own `box-shadow`, not the tooltip's — see
        // `ChromePalette::peek_card_shadow_inner_alpha` for the day it stopped
        // borrowing.
        alpha(palette.peek_card_shadow_inner_alpha),
        alpha(palette.peek_card_shadow_outer_alpha),
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
        // Nothing to draw here: the document is a `PreviewBody` on the layer's
        // own channel, clipped to this very box by the builder that made it.
        // Painting it as labels would be the second renderer the 2026-08-13
        // ruling exists to prevent.
        PeekBody::Document(_) => {}
        PeekBody::Image { .. } => {
            quads.extend(bt_render::rounded_overlay_fill(
                picture_ground(layout.body, scale),
                px(PEEK_IMAGE_RADIUS_LOGICAL_PX),
                // `fill: var(--termbg)` — the ground under the picture is a
                // window onto the picture's own, which is the terminal's.
                bt_render::background_rgb(),
                1.0,
            ));
            if let Some((rect, picture)) = picture_rect(layout, scale).zip(picture) {
                images.push(bt_render::ChromeIcon {
                    key: picture.key.to_owned(),
                    rect,
                    rgba: std::sync::Arc::clone(picture.rgba),
                    width_px: picture.width_px,
                    height_px: picture.height_px,
                    opacity: 1.0,
                });
            }
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
        images,
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

    /// A document `count` lines tall, in the mono body's own line height — the
    /// stand-in for whatever the preview pipeline hands the card.
    fn lines(count: usize) -> PeekBody {
        PeekBody::Document(LINE_HEIGHT * count as f32)
    }

    const LINE_HEIGHT: f32 = 18.0;

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
    /// forty-line card comes back over the cap and the assertion fails.
    #[test]
    fn a_long_body_is_cut_and_the_sentence_around_it_is_not() {
        let window = (1200.0, 900.0);
        let long = layout(
            &content(lines(40)),
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

    /// PIN — **the card paints nothing in its own middle** (user ruling,
    /// 2026-08-13: "what the preview on the right looks like is what the hover
    /// preview looks like").
    ///
    /// The ruling is about *who renders*, and this is that stated as a shape: the
    /// card's own layer draws its head, its chip, its rule and its foot, and the
    /// box between them is left empty for the document that arrives on the body
    /// channel. Every word inside that box would be a word the card wrote itself
    /// — a second renderer, disagreeing with the pane about what a table is —
    /// which is exactly the plain-text body this replaced.
    ///
    /// Mutation: put the old body back (`PeekBody::Lines`, fourteen labels laid
    /// down the body box) and the first assertion goes red with fourteen
    /// trespassers; drop the `layer.body` hand-off in `file_peek_layer` and the
    /// card comes up empty on screen, which is the same fact seen from the other
    /// side.
    #[test]
    fn the_cards_middle_is_left_to_the_document_and_written_by_nobody_else() {
        let window = (1200.0, 900.0);
        let card = content(lines(6));
        let layout = layout(
            &card,
            [40.0, 300.0, 240.0, 320.0],
            window,
            60.0,
            24.0,
            SCALE,
        );
        assert!(
            matches!(layout.body_kind, PeekBody::Document(_)),
            "a text file is a document, not a list of lines"
        );
        let layer = build(&layout, &card, None, &bt_render::chrome_palette(), SCALE);

        let inside =
            |rect: [f32; 4]| rect[1] >= layout.body[1] - 0.5 && rect[3] <= layout.body[3] + 0.5;
        let trespassers: Vec<&str> = layer
            .labels
            .iter()
            .filter(|label| inside(label.rect))
            .map(|label| label.text.as_str())
            .collect();
        assert!(
            trespassers.is_empty(),
            "the card wrote its own body: {trespassers:?}"
        );
        assert!(
            layer.images.is_empty(),
            "and put no picture in a document's box"
        );
        // The head and the foot are still the card's own, or it would have
        // stopped being a card.
        assert!(
            layer.labels.iter().any(|label| label.text == "main.rs"),
            "the head still names the file"
        );
        assert!(
            layer
                .labels
                .iter()
                .any(|label| label.text == PEEK_FOOT_TEXT),
            "and the foot still says how to open it"
        );

        // The body box is the document's own height, which is what makes a short
        // file a short card — the shrink-wrap the ruling did not change.
        assert!(
            (layout.body[3] - layout.body[1] - LINE_HEIGHT * 6.0).abs() <= 1.0,
            "the body box is as tall as the document said: {:?}",
            layout.body
        );
    }

    /// PIN — **a picture is a picture, fitted and centred, not a placeholder.**
    ///
    /// The mock-up's `<svg viewBox="0 0 280 120">` was a grey rectangle standing
    /// in for a thumbnail. The frame survives — it is what the card reserves, so
    /// the card does not change height when the decode lands — and what goes in
    /// it now is the file, at its own proportions, in the middle.
    ///
    /// Mutation: keep drawing only the ground (`images` never pushed, or
    /// `picture_rect` answering `None` unconditionally) and the raster
    /// assertions go red; fill the frame instead of fitting into it and the
    /// aspect assertion does.
    #[test]
    fn a_picture_card_draws_the_file_at_its_own_proportions() {
        let window = (1200.0, 900.0);
        // A 4:1 panorama: nothing that fills a 280×120 frame is ever this shape.
        let (native_w, native_h) = (1200_u32, 300_u32);
        let (fit_w, fit_h) = bt_render::preview_image_extent(
            PEEK_IMAGE_W_LOGICAL_PX as u32,
            PEEK_IMAGE_H_LOGICAL_PX as u32,
            native_w,
            native_h,
        )
        .expect("a picture with pixels fits somewhere");
        let mut card = content(PeekBody::Image {
            width: fit_w as f32,
            height: fit_h as f32,
        });
        card.name = "wide.png".to_owned();
        card.ftype = "image".to_owned();
        let layout = layout(
            &card,
            [40.0, 300.0, 240.0, 320.0],
            window,
            60.0,
            24.0,
            SCALE,
        );

        let rect = picture_rect(&layout, SCALE).expect("a decoded picture has a place to stand");
        let (width, height) = (rect[2] - rect[0], rect[3] - rect[1]);
        assert!(
            (width / height - native_w as f32 / native_h as f32).abs() < 0.05,
            "the picture keeps its own proportions: {width}×{height}"
        );
        assert!(
            rect[1] >= layout.body[1] && rect[3] <= layout.body[3],
            "and stands inside the card's body: {rect:?} in {:?}",
            layout.body
        );
        let ground = picture_ground(layout.body, SCALE);
        assert!(
            ((rect[0] - ground[0]) - (ground[2] - rect[2])).abs() <= 1.0
                && ((rect[1] - ground[1]) - (ground[3] - rect[3])).abs() <= 1.0,
            "centred on its ground: {rect:?} in {ground:?}"
        );

        let rgba: std::sync::Arc<[u8]> = vec![0_u8; (fit_w * fit_h * 4) as usize].into();
        let layer = build(
            &layout,
            &card,
            Some(PeekPicture {
                key: "peek:wide.png@280x70",
                rgba: &rgba,
                width_px: fit_w,
                height_px: fit_h,
            }),
            &bt_render::chrome_palette(),
            SCALE,
        );
        let icon = layer
            .images
            .first()
            .expect("the decoded pixels reach the draw list");
        assert_eq!(icon.rect, rect, "drawn where the fit put it");
        assert_eq!((icon.width_px, icon.height_px), (fit_w, fit_h));

        // And with nothing decoded yet, the frame is still reserved: the card
        // must not change height when the picture lands.
        let waiting = content(PeekBody::Image {
            width: 0.0,
            height: 0.0,
        });
        let empty = self::layout(
            &waiting,
            [40.0, 300.0, 240.0, 320.0],
            window,
            60.0,
            24.0,
            SCALE,
        );
        assert_eq!(
            empty.body[3] - empty.body[1],
            layout.body[3] - layout.body[1],
            "the frame is reserved before the pixels arrive"
        );
        assert!(
            picture_rect(&empty, SCALE).is_none(),
            "and nothing is drawn in it yet"
        );
    }
}
