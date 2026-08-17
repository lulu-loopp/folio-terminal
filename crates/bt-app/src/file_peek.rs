//! **The glance card over a file row** — `.file-peek` (P143-P150, mock-up
//! 1762-1795 / 6367-6427 / 8845-8882, §7.1.3 附则, user ruling 2026-07-17).
//!
//! # What it is, and the one sentence that decides everything else
//!
//! **A read-only preview card the pointer may enter** — a hover card of the kind
//! an editor's symbol popup is, not a picture hanging out of reach.
//!
//! ## The sentence this replaced, and why (user ruling, 2026-08-14)
//!
//! The mock-up's founding line was "**Non-interactive by construction
//! (`pointer-events: none`): it can neither trap the pointer nor flicker;
//! wanting to interact IS the signal to Enter/double-click into the real preview
//! pane**" (1763-1766), and P143 built exactly that: no hit test anywhere, the
//! card drawn and never asked about.
//!
//! Real use overturned it. Once the card started showing the *document* (the
//! 2026-08-13 ruling), a long file arrived cut off at 264 pixels with no way to
//! see the next line — and the moment the pointer moved toward the card to try,
//! it left the row and the card vanished. The old sentence's bargain ("wanting
//! to interact is the signal to open the pane") only holds when there is nothing
//! in the card worth reaching for; a card with a document in it is a card people
//! reach for, and it answered by disappearing.
//!
//! So the promise is now the other one, and it is the one every hover card on
//! the desk makes: **you may walk into it**. What the old sentence was really
//! protecting — the flicker between "the row is hovered" and "the card is
//! hovered" — is protected instead by [`corridor`], which makes the row, the
//! gap and the card one region for the purpose of staying alive. The oscillation
//! is impossible there too, and by geometry rather than by abstinence.
//!
//! It is still not a small preview pane and not a preview float — it is a
//! *third* form, and the seed's guess that it would land on the float chassis is
//! corrected by the mock-up itself (S1's erratum).
//!
//! ## What is still read-only
//!
//! The card takes the gestures its *document* takes and no others: the wheel
//! scrolls it, its bar drags, a wide table or a fence inside it scrolls
//! sideways under Shift and under its own bar exactly as the same block does in
//! the pane ([`body_at`]), and a click anywhere else opens the real pane. There
//! is no caret in it, no selection, no typing and no focus —
//! [`crate::PreviewSurface::Peek`] is still absent from the roll of surfaces the
//! keyboard can reach, and "read-only" now names that absence exactly instead of
//! naming the absence of a hit test.
//!
//! ## Which file it is about, while it is up
//!
//! The card is not nailed to the row that raised it. A pointer that comes to
//! rest on **another** file row for the same 350ms moves the card to that file,
//! re-armed from nothing; a pointer merely crossing those rows on its way into
//! the card moves nothing at all. See [`dwell`], and [`corridor`] for the debt
//! that pays off.
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
///
/// It keeps the strip's left hand. The right hand is the one place on this card
/// that *does* change with the file (user ruling, 2026-08-15) — see
/// [`crate::seats::dress_foot`] — and this sentence is what gives up the width
/// when the two meet, because it is the half you have already read.
pub const PEEK_FOOT_TEXT: &str = "Enter / double-click opens the preview pane";

/// The foot's own text run: the strip inside its horizontal padding.
///
/// Handed out so the caller can measure the card's words beside the renderer,
/// exactly as it already measures the name and the type chip — this module holds
/// no font.
#[must_use]
pub fn foot_run(layout: &PeekLayout, scale: f32) -> [f32; 4] {
    let pad = PEEK_FOOT_PADDING_X_LOGICAL_PX * scale;
    [
        layout.foot[0] + pad,
        layout.foot[1] + PEEK_FOOT_PADDING_Y_LOGICAL_PX * scale,
        layout.foot[2] - pad,
        layout.foot[3],
    ]
}

/// **The refusal** (6406) — the same sentence the preview pane's unknown card
/// says, said in one line.
pub const PEEK_UNKNOWN_TEXT: &str = "No preview — binary or unrecognized type.";

/// Whether `at` lies inside `rect`, half-open on the far edges the way every
/// other hit test in this window is.
#[must_use]
pub fn contains(rect: [f32; 4], at: [f32; 2]) -> bool {
    rect[0] <= at[0] && at[0] < rect[2] && rect[1] <= at[1] && at[1] < rect[3]
}

/// **The region the pointer may travel through without the card coming down**
/// (user ruling, 2026-08-14): the envelope of the row and the card.
///
/// # Why an envelope, and not a velocity
///
/// The rule the ruling asks for is "if the pointer is heading *for* the card,
/// the card stays". The usual way to build that is to watch the direction of
/// travel; this window builds it out of geometry instead, because the two are
/// the same answer here and only one of them can be wrong.
///
/// The card stands ten pixels off the row's own edge and is hung eight pixels
/// above its top: the gap between them is small and it is *between* them, so the
/// bounding box of the two rectangles is very nearly the corridor a hand would
/// draw. A pointer inside it is either on the row, on the card, or crossing the
/// space that joins them — which is the whole of "heading for the card" — and a
/// pointer outside it has gone somewhere else whatever its velocity said one
/// frame ago. A velocity test, by contrast, has to answer for a hand that pauses
/// (velocity zero: heading nowhere) and for one that overshoots and comes back,
/// and both of those are the flicker this is here to prevent.
///
/// It costs the corner of the envelope that belongs to neither rectangle. That
/// corner is a few hundred square pixels of empty terminal next to a card that
/// is *already* going to survive a 220ms grace, so what it buys — no state, no
/// history, no jitter — is worth more than it costs.
///
/// # What it outranks
///
/// **The rows underneath it**, and that is the ruling's own second sentence:
/// while the card is alive and the pointer is in here, another row neither takes
/// the card nor restarts the 350ms intent. It has to be that way round or the
/// first paragraph buys nothing — the card hangs *below* the row it is about, so
/// a hand reaching for its middle crosses a dozen rows of the very tree it came
/// from, and rows that could still claim the card as they passed would pull it
/// down under the hand that was reaching for it. The corridor would be
/// protecting the empty gap and nothing else.
///
/// # What it costs, and how that debt is paid (user ruling, 2026-08-14)
///
/// It used to cost those rows outright: *a file listed just under the one being
/// glanced could not be glanced until this card was down*, and the only way out
/// was to leave the corridor — a few pixels up, or out of the tree — and let
/// the next row arm normally.
///
/// That was too much to charge. The rows a reach happens to cross are ordinary
/// rows of an ordinary tree, and being unable to glance one because a card is
/// standing over it is the corridor solving its own problem with somebody
/// else's rows. So the outranking above is narrowed to what it was actually for:
/// it outranks a hand **crossing** those rows, and not a hand that **stops** on
/// one.
///
/// The two are told apart by the clock the first card was already armed on —
/// [`PEEK_INTENT_MS`], see [`dwell`]. Cross three rows in less than that and
/// three clocks start and none finishes, so the reach is exactly as protected as
/// it was. Rest on one for that long and the card moves to it, re-armed from
/// scratch: a new buffer, a new document, a new scroll, placed against the new
/// row. Nothing about the envelope changes; what changed is that standing still
/// inside it is no longer read as travelling through it.
///
/// See [`crate::Runtime::observe_file_peek`] and
/// [`crate::Runtime::dwell_file_peek`].
#[must_use]
pub fn corridor(row: [f32; 4], frame: [f32; 4]) -> [f32; 4] {
    [
        row[0].min(frame[0]),
        row[1].min(frame[1]),
        row[2].max(frame[2]),
        row[3].max(frame[3]),
    ]
}

/// What a pointer event does to a card that is already on screen.
///
/// The whole of the 2026-08-14 lifecycle in four words, kept as a value so that
/// the rule can be read — and tested — without a window: [`Runtime`]'s side of
/// it is a `match` and nothing else.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Life {
    /// The pointer is **in** the card: it is being used, and no clock runs.
    Held,
    /// The pointer is in the corridor — on the row, or crossing the gap toward
    /// the card. Alive, and any grace already running is cancelled.
    Kept,
    /// The pointer has left the corridor. The grace starts and the card stays
    /// drawn until it runs out.
    Released,
    /// Down **now**, no grace: a drag has begun, or there is no pointer in this
    /// window at all. A grace is a courtesy to a hand that is still reaching for
    /// the card, and a hand that is carrying a file somewhere is not.
    Gone,
}

/// [`Life`] for a card standing at `frame` over `row`, with the pointer at `at`.
///
/// `at` is `None` when the pointer has left the window; `dragging` is whether a
/// drag is in flight, which outranks every geometry below it.
#[must_use]
pub fn life(row: [f32; 4], frame: [f32; 4], at: Option<[f32; 2]>, dragging: bool) -> Life {
    if dragging {
        return Life::Gone;
    }
    let Some(at) = at else {
        return Life::Gone;
    };
    if contains(frame, at) {
        return Life::Held;
    }
    if contains(corridor(row, frame), at) {
        return Life::Kept;
    }
    Life::Released
}

/// What a **left press** inside the card means.
///
/// Two answers and no third, which is what keeps "read-only" true of a card the
/// pointer can now reach. See [`Runtime::press_file_peek`] for the argument.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Press {
    /// On the scroll thumb, this far into its own length.
    Thumb(f32),
    /// Anywhere else in the card: open the real preview pane.
    Open,
    /// Not the card's press at all.
    Elsewhere,
}

/// [`Press`] for a press at `at` on a card standing at `frame`, wearing `bar`.
///
/// **The thumb is asked first**, for the reason a text editor asks it first: a
/// bar drawn inside a scrolling region is still a bar, and a press on it means
/// the bar rather than the words behind it.
#[must_use]
pub fn press_at(frame: [f32; 4], bar: Option<&crate::preview::ScrollBar>, at: [f32; 2]) -> Press {
    if !contains(frame, at) {
        return Press::Elsewhere;
    }
    match bar.filter(|bar| contains(bar.grab, at)) {
        Some(bar) => Press::Thumb((at[1] - bar.thumb[1]).clamp(0.0, bar.thumb[3] - bar.thumb[1])),
        None => Press::Open,
    }
}

// **The card's own `scroll_bar` stood here** until 2026-08-14, when the docked
// pane and the preview float grew the same bar. It was `preview::scroll_bar`
// stood on its end with the card's body filled in — and once three surfaces
// wore it, "the card's bar" stopped being a thing there could be one of: the
// arithmetic moved to `crate::preview_body_bar`, which every one of the three
// goes through, and the card hands it the box its own layout produced exactly as
// a pane hands it the box the tree gave. A wrapper here would have been the
// second door the block bar's history warns about.

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
    /// **It scrolls** (user ruling, 2026-08-14), and the sentence that used to
    /// stand here is worth keeping as a record of why it does now:
    ///
    /// > *It does not scroll, and that is a decision rather than an omission. A
    /// > glance is for placing a file, and the head answers that; the foot's
    /// > fixed sentence is the way to the rest of it. A scrollable card would
    /// > also need to be a card the pointer can enter, and P143 says it can never
    /// > be one.*
    ///
    /// The last clause was the load-bearing one and it is the one that fell. The
    /// argument was sound given P143; with P143 overturned the conclusion goes
    /// with it, and what is left is a 264-pixel window onto a document people
    /// were visibly trying to read further into.
    ///
    /// The height carried here is still the *document's*, not the card's: the
    /// card shrink-wraps a short file and caps a long one, and the difference
    /// between the two is exactly what decides whether a scroll bar is drawn at
    /// all.
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
    foot: &crate::seats::FootWords,
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
    // The way out on the left, the standing fact on the right — the same strip
    // every other foot in this window now keeps (user ruling, 2026-08-15). The
    // sentence is what yields, because it is the same sentence on every card and
    // the phrase is the only thing on the strip that is news.
    labels.push(label(
        &foot.lead,
        foot.lead_box,
        px(PEEK_FOOT_FONT_LOGICAL_PX),
        palette.body_hint_text,
    ));
    if !foot.notice.is_empty() {
        labels.push(ChromeLabel {
            align_right: true,
            clip: Some(foot.notice_box),
            ..label(
                &foot.notice,
                foot.notice_box,
                px(PEEK_FOOT_FONT_LOGICAL_PX),
                palette.body_hint_text,
            )
        });
    }

    OverlayLayer {
        quads,
        labels,
        sprites: std::mem::take(&mut sprites),
        images,
        ..OverlayLayer::default()
    }
}

/// **Where a pointer question about the card's document goes.**
///
/// The card is not in the layout tree and not in the float host, so
/// [`crate::Runtime::preview_surface_at`] — the walk that answers every other
/// surface — cannot see it at all. Anything that wants to ask the *document* in
/// the card a question (which wide block is under the pointer, whose scroll
/// thumb this is) has to be handed the card's body box instead, and this is the
/// one place that hands it over.
///
/// `frame` is where the card came to rest and `body` is the document's box
/// inside it; the answer is the body when the pointer is in the card, and
/// nothing when it is not. Written as a function rather than as two lines at the
/// call site because "the card takes the question first, and it takes it by its
/// whole frame" is the rule, and a rule with one reader is still a rule.
#[must_use]
pub fn body_at(frame: [f32; 4], body: [f32; 4], at: [f32; 2]) -> Option<[f32; 4]> {
    contains(frame, at).then_some(body)
}

/// **What a pointer resting inside the corridor does to the card that is up**
/// (user ruling, 2026-08-14: *穿行不换,停留即换* — crossing does not move the
/// card, resting does).
///
/// The corridor's own rule ([`corridor`]) says another row may neither take the
/// card nor restart its intent while the pointer is inside the envelope. That
/// bought the reach and cost the rows underneath it: a file listed just below
/// the one being glanced could not be glanced at all until the card came down.
/// The ruling pays that back without giving the corridor up, by distinguishing
/// the two things a pointer can be doing over those rows — *crossing* them on
/// the way to the card, and *stopping* on one of them.
///
/// The distinction is the same 350ms the first card was armed on
/// ([`PEEK_INTENT_MS`]), because it is the same question: a hand that has held
/// still on a row for that long is asking about that row. A hand crossing three
/// rows in less starts three clocks and finishes none — every crossing is a
/// [`Dwell::Start`] that replaces the one before it — which is exactly why
/// travelling through costs nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dwell {
    /// Nothing to count. The pointer is on the card's own row, on a directory,
    /// on the card itself, or on no row at all — and any clock that was running
    /// is dropped, because a hand that has left the row it was resting on has
    /// stopped resting on it.
    ///
    /// **The card is untouched either way**: a directory row inside the corridor
    /// neither moves the card nor takes it down, which is the corridor's own
    /// standing answer and is deliberately left alone.
    Idle,
    /// A row the clock is not already on: start [`PEEK_INTENT_MS`] against it.
    Start,
    /// The row the clock is already on: leave it alone, exactly as
    /// [`crate::Runtime::observe_file_peek`] leaves a running intent alone.
    /// Restarting here would mean a hand that trembles never dwells.
    Keep,
}

/// [`Dwell`] for a pointer over `over`, with `waiting` the row a dwell is
/// already counting against.
///
/// Generic in the row's identity because *which* row is the caller's fact — a
/// host and a key over there, nothing at all in here — while the rule is this
/// module's. It is the same division [`life`] makes: the geometry is handed in,
/// the judgement is here.
#[must_use]
pub fn dwell<T: PartialEq + ?Sized>(over: Option<&T>, waiting: Option<&T>) -> Dwell {
    match (over, waiting) {
        (None, _) => Dwell::Idle,
        (Some(over), Some(waiting)) if over == waiting => Dwell::Keep,
        (Some(_), _) => Dwell::Start,
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

    /// Half the point size a glyph — about what Segoe UI averages — so an
    /// assertion about a strip's division is arithmetic rather than a
    /// measurement of the shipped font.
    fn ruler(text: &str, size: f32) -> f32 {
        text.chars().count() as f32 * size / 2.0
    }

    /// The card's foot, dressed the way [`crate::Runtime::file_peek_layer`]
    /// dresses it — the fixed sentence, and whatever phrase is hung beside it.
    fn foot(layout: &PeekLayout, notice: &str) -> crate::seats::FootWords {
        crate::seats::dress_foot(
            crate::seats::FootDress {
                run: foot_run(layout, SCALE),
                lead: PEEK_FOOT_TEXT,
                flash: None,
                notice,
                cut_left: false,
                font_px: PEEK_FOOT_FONT_LOGICAL_PX * SCALE,
                gap_px: crate::seats::FILES_FOOT_NOTICE_GAP_LOGICAL_PX * SCALE,
            },
            &mut ruler,
        )
    }

    const LINE_HEIGHT: f32 = 18.0;

    /// PIN (user ruling, 2026-08-15) — **the glance card's foot hangs the same
    /// phrase on the same side, and the way out survives it.**
    ///
    /// The card used to be excluded from every notice by name, and the argument
    /// was good: its foot is one fixed sentence and that sentence is the card's
    /// only exit, so a warning that *took* the strip would have replaced the way
    /// out with a complaint about a file you were merely looking at. The ruling
    /// keeps the exit and drops the exclusion, because the phrase does not take
    /// the strip — it takes the right-hand end of it, and the sentence is what
    /// gives up the width.
    ///
    /// The card is the one foot in this window whose lead is not a path, which
    /// is why it is cut from the **back**: a sentence reads forwards, and
    /// "…double-click opens the preview pane" is a sentence with its verb
    /// missing.
    ///
    /// MUTATION ①: pass `cut_left: true` for the card and the ellipsis assertion
    /// goes red on a sentence beheaded instead of trimmed.
    /// MUTATION ②: draw the phrase before the sentence without the split — give
    /// both the whole run — and the disjointness assertion goes red, which is
    /// the two printing through each other in 280 pixels.
    #[test]
    fn the_glance_cards_foot_keeps_its_way_out_beside_the_phrase() {
        let window = (1600.0, 900.0);
        let card = content(lines(6));
        let layout = layout(
            &card,
            [40.0, 300.0, 240.0, 320.0],
            window,
            60.0,
            24.0,
            SCALE,
        );
        let notice = crate::preview::preview_truncated_notice();
        let dressed = foot(&layout, notice);
        let layer = build(
            &layout,
            &card,
            &dressed,
            None,
            &bt_render::chrome_palette(),
            SCALE,
        );

        let hung = layer
            .labels
            .iter()
            .find(|label| label.text == notice)
            .expect("the phrase is on the card");
        let exit = layer
            .labels
            .iter()
            .find(|label| label.text.starts_with("Enter / double-click"))
            .expect("and the way out is still printed");
        assert!(hung.align_right, "flush with the strip's right edge");
        assert_eq!(hung.rect[2], foot_run(&layout, SCALE)[2]);
        assert!(
            exit.rect[2] < hung.rect[0],
            "the sentence stops before the phrase begins: {:?} vs {:?}",
            exit.rect,
            hung.rect
        );
        assert!(
            !dressed.lead.starts_with('…'),
            "a sentence is cut from the back, not the front: {}",
            dressed.lead
        );

        // And with nothing hung on it the card is exactly what it was.
        let bare = foot(&layout, "");
        assert_eq!(bare.lead, PEEK_FOOT_TEXT, "the sentence, whole");
        assert_eq!(bare.lead_box, foot_run(&layout, SCALE), "in the whole run");
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
        let layer = build(
            &layout,
            &card,
            &foot(&layout, ""),
            None,
            &bt_render::chrome_palette(),
            SCALE,
        );

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
            &foot(&layout, ""),
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

    // ── the card the pointer may enter (user ruling, 2026-08-14) ────────────

    /// A card over a row, with a document too tall for it — the shape every
    /// test below is about.
    fn tall_card(row: [f32; 4]) -> PeekLayout {
        self::layout(&content(lines(40)), row, (1200.0, 800.0), 60.0, 24.0, SCALE)
    }

    /// PIN — **the corridor: the row, the gap and the card are one region for
    /// the purpose of staying alive** (user ruling, 2026-08-14).
    ///
    /// The card is hung eight pixels above its row and stands ten off its right
    /// edge, so a hand reaching for the *middle* of a 264px card travels right
    /// and **down** — across empty space that belongs to neither rectangle, and
    /// across the rows of the tree it came from. P143's card died in exactly
    /// that space, and its dying there is the whole reason P143 was overturned:
    /// the gesture the card invites was the gesture that dismissed it.
    ///
    /// So the region is the envelope of the two rectangles, and it is geometry
    /// rather than velocity for the reason [`corridor`] argues: a hand that
    /// pauses has velocity zero and is heading nowhere, and a hand that
    /// overshoots and comes back is the flicker this exists to prevent.
    ///
    /// MUTATIONS that must turn it red:
    /// ① have `corridor` answer the row alone (`row`) — the reach across the
    ///    gap comes back `Released` and the card dies under the hand again;
    /// ② drop the `contains(frame, at)` branch from `life` — arriving in the
    ///    card reads `Kept` instead of `Held`, and a card that is merely kept is
    ///    one a grace can still be started against;
    /// ③ have `life` answer `Kept` when the pointer is outside the envelope —
    ///    the three departures stop being departures and the card never falls.
    #[test]
    fn the_row_the_gap_and_the_card_are_one_corridor_a_hand_may_cross() {
        let row = [40.0, 300.0, 240.0, 320.0];
        let frame = tall_card(row).frame;
        assert!(
            frame[0] > row[2] && frame[3] > row[3],
            "the fixture is the real placement: off the row's right edge and \
             hanging below it, which is where the reach goes — {frame:?}"
        );

        // ① The shape itself: the bounding box of the two, and nothing else.
        assert_eq!(
            corridor(row, frame),
            [
                row[0].min(frame[0]),
                row[1].min(frame[1]),
                row[2].max(frame[2]),
                row[3].max(frame[3]),
            ]
        );

        let at = |x: f32, y: f32| life(row, frame, Some([x, y]), false);
        let middle = [(frame[0] + frame[2]) / 2.0, (frame[1] + frame[3]) / 2.0];

        // ② On the row it is about.
        assert_eq!(at(140.0, 310.0), Life::Kept, "the card's own row");

        // ③ **The reach.** Right and down, through the ten-pixel gap, level with
        //    the middle of the card — the point the old card vanished at.
        assert_eq!(
            at((row[2] + frame[0]) / 2.0, middle[1]),
            Life::Kept,
            "crossing the gap toward the card is heading for the card"
        );

        // ④ Arrived: the card is being used, and no clock runs.
        assert_eq!(at(middle[0], middle[1]), Life::Held);

        // ⑤ Gone somewhere else, on all three sides the envelope has an outside.
        assert_eq!(at(140.0, row[1] - 40.0), Life::Released, "above the row");
        assert_eq!(at(frame[2] + 20.0, 310.0), Life::Released, "past the card");
        assert_eq!(at(140.0, frame[3] + 20.0), Life::Released, "below both");
    }

    /// PIN — **the card's document takes the questions its document is entitled
    /// to** (user ruling, 2026-08-14).
    ///
    /// A table too wide for the card is the same scrolling region a table too
    /// wide for the pane is, and the ruling that gave the card a wheel and a
    /// thumb gave it the blocks inside it too. But the card is in no layout tree
    /// and in no float host, so the walk that answers "which surface is the
    /// pointer in" cannot see it: without this, a hand on the bar under a wide
    /// table in a glance finds nothing, the offset stays nailed to zero, and the
    /// press falls through to the door instead.
    ///
    /// It is deliberately *not* the whole pane walk. The card is read-only, and
    /// that walk also answers presses, carets, selections and the edit focus —
    /// so the card enters through here, by name, for exactly the questions it is
    /// entitled to.
    ///
    /// MUTATIONS that must turn it red:
    /// ① have `body_at` answer `None` always — "the card's blocks never move",
    ///    which is the bug this closes;
    /// ② have it answer the *frame* rather than the body — the block hit test
    ///    would be run against the head and the foot as well, and a press on the
    ///    file's name would scroll a table.
    #[test]
    fn a_wide_block_inside_the_card_is_asked_about_in_the_cards_own_body() {
        let row = [40.0, 300.0, 240.0, 320.0];
        let card = tall_card(row);

        // ① Inside the card, the document's box — not the frame, so the head and
        //    the foot are outside every question the document answers.
        let middle = [
            (card.body[0] + card.body[2]) / 2.0,
            (card.body[1] + card.body[3]) / 2.0,
        ];
        assert_eq!(body_at(card.frame, card.body, middle), Some(card.body));
        assert!(
            card.body[1] > card.frame[1] && card.body[3] < card.frame[3],
            "the fixture's body really is inset from its frame, or ② proves nothing"
        );

        // ② The head is the card, so the question still belongs to the card —
        //    and it is still asked against the *body*, which is what keeps a
        //    press on the file's name from landing on a table's thumb.
        assert_eq!(
            body_at(card.frame, card.body, [card.name[0], card.name[1] + 1.0]),
            Some(card.body)
        );

        // ③ Outside the card the card answers nothing at all, and the pane walk
        //    behind it gets the question it was always going to get.
        assert_eq!(
            body_at(card.frame, card.body, [card.frame[2] + 4.0, middle[1]]),
            None
        );
        assert_eq!(
            body_at(card.frame, card.body, [middle[0], card.frame[3] + 4.0]),
            None
        );
    }

    /// PIN — **穿行不换,停留即换** (user ruling, 2026-08-14): a hand crossing the
    /// rows under the corridor does not move the card, and a hand that stops on
    /// one of them does.
    ///
    /// [`corridor`] bought the reach by ruling that a row under the envelope may
    /// neither take the card nor restart its intent, and it had to be that way
    /// round: the card hangs *below* the row it is about, so a hand reaching for
    /// its middle crosses a dozen rows of the very tree it came from. What that
    /// cost was those rows — a file listed just under the one being glanced
    /// could not be glanced at all until the card came down.
    ///
    /// The way out is not to weaken the corridor but to tell the two gestures
    /// apart, and the thing that tells them apart is the clock the first card was
    /// already armed on: [`PEEK_INTENT_MS`]. Crossing three rows in less than
    /// that starts three clocks and finishes none, because every crossing
    /// replaces the one before it.
    ///
    /// MUTATIONS that must turn it red:
    /// ① answer `Keep` when the rows differ — 掠过即换: the card follows the
    ///    pointer down the list and the reach can never land, which is the
    ///    corridor undone;
    /// ② answer `Start` when the rows are the same — a hand that trembles
    ///    restarts its own clock and never dwells at all;
    /// ③ answer anything but `Idle` for no row — a folder or the gap between
    ///    two trees would keep a stale clock running and switch the card to a
    ///    row the pointer left long ago.
    #[test]
    fn crossing_the_rows_under_the_corridor_does_not_move_the_card_but_resting_does() {
        // Rows as the caller identifies them: a host and a key. The rule knows
        // nothing about either — see [`dwell`].
        let (a, b, c) = ("tree/a.rs", "tree/b.rs", "tree/c.rs");

        // ① Nothing is counting and the hand arrives on a row: start.
        assert_eq!(dwell(Some(b), None), Dwell::Start);

        // ② **穿行.** Three rows crossed is three starts and no maturity: each
        //    one replaces the clock the last one set, so no single row ever
        //    holds the pointer for its own 350ms.
        assert_eq!(dwell(Some(c), Some(b)), Dwell::Start);
        assert_eq!(dwell(Some(a), Some(c)), Dwell::Start);

        // ③ **停留.** The same row again leaves the clock alone, which is the
        //    only way it can ever run out — and it runs out at the same number
        //    the first card was armed on, not a second one invented for this.
        assert_eq!(dwell(Some(b), Some(b)), Dwell::Keep);
        assert_eq!(
            PEEK_INTENT_MS, 350,
            "the dwell is the intent, so there is one number and this is it"
        );

        // ④ A folder, a notice, the card's own row, the space past the end of
        //    the list: all of them are "no row", and all of them drop the clock
        //    without touching the card.
        assert_eq!(dwell(None, Some(b)), Dwell::Idle);
        assert_eq!(dwell::<str>(None, None), Dwell::Idle);
    }

    /// PIN — **a hand that has taken hold of something puts the card down at
    /// once, with no grace at all.**
    ///
    /// The grace is a courtesy to a hand still reaching for the card. A hand
    /// carrying a file somewhere is not reaching for anything, and a card left
    /// standing over a tree whose rows are moving is describing where a file
    /// used to be — which is `hidePeek()` being the first line of `startDrag`,
    /// one surface further in.
    ///
    /// A pointer that has left the window entirely is the same answer for the
    /// same reason: there is no hand to be gentle with.
    ///
    /// MUTATIONS that must turn it red:
    /// ① drop `if dragging { return Life::Gone }` from `life` — a drag begun
    ///    inside the card reads `Held` and the card rides the whole gesture;
    /// ② answer `Life::Released` for `None` — a pointer gone from the window
    ///    leaves the card up for a further 220ms with nothing to dismiss it.
    #[test]
    fn a_hand_that_has_taken_hold_of_something_puts_the_card_down_at_once() {
        let row = [40.0, 300.0, 240.0, 320.0];
        let frame = tall_card(row).frame;
        let middle = [(frame[0] + frame[2]) / 2.0, (frame[1] + frame[3]) / 2.0];
        let on_row = [140.0, 310.0];

        assert_eq!(
            life(row, frame, Some(middle), true),
            Life::Gone,
            "a drag outranks standing in the card"
        );
        assert_eq!(
            life(row, frame, Some(on_row), true),
            Life::Gone,
            "and outranks standing on the row"
        );
        assert_eq!(
            life(row, frame, None, false),
            Life::Gone,
            "a pointer that left the window takes the card with it"
        );
        assert_eq!(
            life(row, frame, Some(middle), false),
            Life::Held,
            "and the same point with a free hand is still the card's"
        );
    }

    /// PIN — **the card's scroll bar is the block's bar stood on its end**: the
    /// same proportion, the same grab tolerance, the same linear map, read down
    /// the body's right edge instead of along its foot.
    ///
    /// One geometry for the painter, the hit test and the drag alike. The
    /// block's bar has already been through one round of "the picture and the
    /// hit test disagreed", and a second function for a second axis is how that
    /// bug comes back — so the axis is a field on the bar and the drag reads it
    /// rather than guessing.
    ///
    /// MUTATIONS that must turn it red:
    /// ① give `preview::scroll_bar`'s `Vertical` arm the horizontal triple
    ///    (`clip[0], clip[2], clip[3]`) — the track lands along the foot and the
    ///    overflow is measured across the wrong extent;
    /// ② drop the `grow` widening from `grab` — two drawn pixels are the whole
    ///    target again, which is the defect the block's bar was built to fix;
    /// ③ have `scroll_dragged_to` read `bar.track[0]` instead of
    ///    `bar.track_start()` — the drag maps from the wrong origin and the
    ///    thumb answers hundreds of pixels away from the hand.
    #[test]
    fn the_cards_thumb_rides_its_own_edge_and_reads_the_offset_backwards() {
        let row = [40.0, 300.0, 240.0, 320.0];
        let card = tall_card(row);
        let document = LINE_HEIGHT * 40.0;
        let page = card.body[3] - card.body[1];
        assert!(
            document > page,
            "the fixture overflows the card, or there is no bar to test"
        );

        let bar = crate::preview_body_bar(
            card.body,
            crate::preview::ScrollAxis::Vertical,
            [0.0, 0.0],
            document,
            SCALE,
        )
        .expect("a document taller than the card wears a bar");
        assert_eq!(bar.axis, crate::preview::ScrollAxis::Vertical);

        // ① Down the right edge of the body, full height, hugging the far side.
        assert_eq!([bar.track[1], bar.track[3]], [card.body[1], card.body[3]]);
        assert_eq!(bar.track[2], card.body[2]);
        assert!(
            bar.track[0] > card.body[0],
            "a rule down the edge, not a curtain across the body"
        );
        assert_eq!(bar.overflow, document - page);

        // ② The thumb is the visible share of the content, and at rest it is at
        //    the head of the track.
        assert!(
            (bar.thumb[3] - bar.thumb[1] - page * (page / document)).abs() < 0.5,
            "the thumb is a picture of how much of the file is showing"
        );
        assert_eq!(bar.thumb[1], card.body[1]);
        let scrolled = crate::preview_body_bar(
            card.body,
            crate::preview::ScrollAxis::Vertical,
            [0.0, bar.overflow],
            document,
            SCALE,
        )
        .expect("still overflowing");
        assert!(
            (scrolled.thumb[3] - card.body[3]).abs() < 0.5,
            "and at the end of the document it is at the end of the track"
        );

        // ③ Wider to a hand than to the eye — **inward, and up to the card's own
        //    edge but never across it** (real-machine finding, 2026-08-14). The
        //    sentence this replaces was "on every side", and it was written when
        //    this bar was the only one of its kind: a card floats over the
        //    terminal, so five pixels past its edge cost nobody anything. The
        //    same bar on a docked pane lands inside a divider's seam band or the
        //    window's own resize border, both of which are asked before this
        //    window is — so the growth is inward for all three faces, and the
        //    tolerance is now a property of the hand *and* of what the far side
        //    already belongs to.
        assert!(
            bar.grab[0] <= bar.thumb[0] - crate::preview::BODY_SCROLL_INWARD_HIT_LOGICAL_PX * SCALE
                && bar.grab[1] < bar.thumb[1]
                && bar.grab[3] > bar.thumb[3],
            "the tolerance reaches into the card, and reaches past both ends of the thumb"
        );
        assert_eq!(
            bar.grab[2], card.body[2],
            "and stops at the edge it rides, whoever is on the other side"
        );

        // ④ Dragging reads that picture backwards, on the bar's own axis,
        //    linearly, and stops where the wheel stops.
        let held = 4.0_f32;
        let dragged = |y: f32| crate::preview::scroll_dragged_to(&bar, bar.along([0.0, y]), held);
        let home = bar.thumb[1] + held;
        assert_eq!(dragged(home), 0.0, "at rest it has not moved");
        assert!(
            (dragged(home + bar.travel / 2.0) - bar.overflow / 2.0).abs() < 0.5,
            "half the track is half the document"
        );
        assert_eq!(dragged(home + bar.travel * 4.0), bar.overflow, "and stops");
        assert_eq!(dragged(home - 400.0), 0.0, "at both ends");

        // ⑤ A document that fits wears nothing: a track with no thumb is a
        //    promise of somewhere to go in a card that has nowhere.
        let short = self::layout(&content(lines(2)), row, (1200.0, 800.0), 60.0, 24.0, SCALE);
        assert!(
            crate::preview_body_bar(
                short.body,
                crate::preview::ScrollAxis::Vertical,
                [0.0, 0.0],
                LINE_HEIGHT * 2.0,
                SCALE
            )
            .is_none(),
            "a two-line file has nowhere to scroll and says so by drawing nothing"
        );
    }

    /// PIN — **a press in the card has two answers and no third** (user ruling,
    /// 2026-08-14), which is what keeps "read-only" true of a card the pointer
    /// can now reach.
    ///
    /// The thumb is asked first, for the reason a text editor asks it first: a
    /// bar drawn inside a scrolling region is still a bar, and a press on it
    /// means the bar rather than the words behind it. Everywhere else in the
    /// card is the door to the real preview pane — the same door Enter and the
    /// double-click take, so the foot's promise and the click agree about where
    /// they land.
    ///
    /// There is deliberately no third answer. A press in a document is a place
    /// to put a caret in a *pane*; the card has nothing to type into, and that
    /// absence is now the whole of what "read-only" names.
    ///
    /// MUTATIONS that must turn it red:
    /// ① have `press_at` skip the bar and answer `Open` inside the card — the
    ///    thumb becomes a picture again and dragging it opens the file instead;
    /// ② drop the `contains(frame, at)` guard — a press out on the terminal
    ///    beside the card opens a file nobody clicked;
    /// ③ have `Press::Thumb` carry `0.0` instead of the distance into the thumb
    ///    — the thumb jumps to put its own head under the pointer at the press.
    #[test]
    fn a_press_in_the_card_is_the_door_to_the_pane_and_never_a_caret() {
        let row = [40.0, 300.0, 240.0, 320.0];
        let card = tall_card(row);
        let document = LINE_HEIGHT * 40.0;
        let bar = crate::preview_body_bar(
            card.body,
            crate::preview::ScrollAxis::Vertical,
            [0.0, 0.0],
            document,
            SCALE,
        )
        .expect("the fixture overflows");

        // ① On the thumb — asked first, and carrying how far into it the hand
        //    took hold.
        let held = 4.0_f32;
        let on_thumb = [(bar.grab[0] + bar.grab[2]) / 2.0, bar.thumb[1] + held];
        assert_eq!(
            press_at(card.frame, Some(&bar), on_thumb),
            Press::Thumb(held)
        );

        // ② In the document beside it — the door, not a caret.
        let in_document = [card.body[0] + 20.0, (card.body[1] + card.body[3]) / 2.0];
        assert_eq!(press_at(card.frame, Some(&bar), in_document), Press::Open);
        // And the head and the foot are the card too: the whole face is one
        // door, which is what "click the card" means.
        assert_eq!(
            press_at(card.frame, Some(&bar), [card.name[0], card.name[1] + 1.0]),
            Press::Open
        );

        // ③ Outside the card is nobody's press — the guard that keeps a click on
        //    the terminal beside the card from opening a file.
        assert_eq!(
            press_at(
                card.frame,
                Some(&bar),
                [card.frame[2] + 4.0, in_document[1]]
            ),
            Press::Elsewhere
        );
        assert_eq!(
            press_at(
                card.frame,
                Some(&bar),
                [in_document[0], card.frame[1] - 4.0]
            ),
            Press::Elsewhere
        );

        // ④ With no bar at all — a card whose document fits — the same point on
        //    the edge is the door, because there is nothing there to hold.
        assert_eq!(press_at(card.frame, None, on_thumb), Press::Open);
    }
}
