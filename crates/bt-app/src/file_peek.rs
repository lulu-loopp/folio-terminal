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
// `.fpeek-head .dirty { font-size: 9px }` is gone: the unsaved-edits dot is a
// drawing rather than a codepoint since the icon block, and its diameter is
// `crate::marks::DIRTY_DOT_LOGICAL_PX` — the same one the two preview heads
// strike, where this head used to set the same `●` four points smaller.
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

/// **The box a page is fitted into** (user ruling 2026-08-25) — the picture's
/// width and forty pixels more of height.
///
/// Wider than it is tall would be the wrong frame for the one thing this box
/// ever holds. A picture may be any shape; a *page* is portrait far more often
/// than not, and a portrait page fitted into the picture's 280×120 comes out
/// eighty-five pixels wide with two hundred pixels of empty ground beside it.
///
/// 160 rather than more, because the card's own cap decides it: 264 less its two
/// hairlines, its head, its foot, the two fact lines under the page and the
/// padding around all of them leaves 162, and this is that number with the
/// rounding taken off. A page box any taller would push the card past
/// [`PEEK_MAX_HEIGHT_LOGICAL_PX`], and what `overflow: hidden` would then cut is
/// the size line — see [`PeekBody::Facts`].
pub const PEEK_PAGE_W_LOGICAL_PX: f32 = 280.0;
pub const PEEK_PAGE_H_LOGICAL_PX: f32 = 160.0;
/// The air between the page and the first line of facts under it.
pub const PEEK_PAGE_GAP_LOGICAL_PX: f32 = 8.0;

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
#[must_use]
pub fn peek_foot_text() -> &'static str {
    crate::i18n::Text::PeekFoot.text()
}

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
#[must_use]
pub fn peek_unknown_text() -> &'static str {
    crate::i18n::Text::PeekUnknown.text()
}

/// **What the card says over a name that opens as a page** (user ruling
/// 2026-08-23; `docs/DESIGN.md` §7.10 ⑥).
///
/// A statement of what this row does, and nothing else: the card cannot show a
/// page, so what it owes its reader is the one fact that places the row — that
/// opening it will render rather than refuse. See [`PeekBody::Page`].
#[must_use]
pub fn peek_page_text() -> &'static str {
    crate::i18n::Text::PeekOpensAsPage.text()
}

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
/// Three answers and no fourth, which is what keeps "read-only" true of a card
/// the pointer can now reach. See [`Runtime::press_file_peek`] for the argument.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Press {
    /// On the scroll thumb, this far into its own length.
    Thumb(f32),
    /// **On the head** — the strip that names the file (user ruling 2026-08-27,
    /// `docs/DESIGN.md` §7.29).
    ///
    /// A press here is not yet anything: it is six pixels of intent away from
    /// being a carry and one release away from being [`Self::Open`]. The head is
    /// the card's *handle*, and this arm is what lets one press mean both — the
    /// same shape a float's own header press has had since 2026-08-12, where the
    /// gesture is decided by whether the hand travels rather than by which
    /// button was pressed.
    Head,
    /// Anywhere else in the card: open the real preview pane.
    Open,
    /// Not the card's press at all.
    Elsewhere,
}

/// [`Press`] for a press at `at` on a card standing at `frame`, whose head is
/// `head`, wearing `bar`.
///
/// **The thumb is asked first**, for the reason a text editor asks it first: a
/// bar drawn inside a scrolling region is still a bar, and a press on it means
/// the bar rather than the words behind it. The head is asked next and the door
/// last, which is the order they are stacked in: the head is a band across the
/// top of a face that is otherwise all door.
#[must_use]
pub fn press_at(
    frame: [f32; 4],
    head: [f32; 4],
    bar: Option<&crate::preview::ScrollBar>,
    at: [f32; 2],
) -> Press {
    if !contains(frame, at) {
        return Press::Elsewhere;
    }
    match bar.filter(|bar| contains(bar.grab, at)) {
        Some(bar) => Press::Thumb((at[1] - bar.thumb[1]).clamp(0.0, bar.thumb[3] - bar.thumb[1])),
        None if contains(head, at) => Press::Head,
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
    /// **A name that opens as a page** (user ruling 2026-08-23,
    /// `docs/DESIGN.md` §7.10 ⑥).
    ///
    /// One line, laid out exactly as [`Self::Refused`]'s is, and a separate
    /// variant rather than a second sentence squeezed through that one because
    /// they are opposites: the refusal says nothing will happen, and this says
    /// what will. It is the card's half of the ruling — until it, a `.pdf` row
    /// drew "no preview" under a pointer and opened a rendered page under a
    /// double click, and the card was the half that was lying.
    ///
    /// No body is asked for and none arrives: a page is drawn by the engine, on
    /// a seat, and there is no engine on a hover card.
    ///
    /// **What is left of it since 2026-08-25**: the pages whose own file this
    /// window cannot read at all. `.html` stopped being one of them that day —
    /// its source is text and the card shows it, down [`Self::Document`]'s lane
    /// — and `.pdf` states [`Self::Facts`] instead. Nothing reaches this variant
    /// from the files column today, and it is kept rather than deleted because
    /// it is the honest answer for the next page-class member whose bytes are
    /// neither text nor countable: one line saying what the row does.
    Page,
    /// **A page this window drew for itself, and the two facts under it** (user
    /// rulings 2026-08-25; [`crate::preview::PageGlance::Facts`]).
    ///
    /// A `.pdf` opens in the pane on WebView2's engine, and that engine hands
    /// this process no pixels — so for a day the card's answer was two sentences
    /// and no picture: how large the file is and how many pages it holds, the
    /// two facts a reader hovers one to learn, both readable without a renderer.
    ///
    /// The second ruling is that a reader hovering a report wants to see the
    /// report. The card still has no *engine*; what it grew is a rasteriser of
    /// its own ([`crate::pdf::page_raster`]). So the body is a page over two
    /// lines: the picture that says which document this is, and the facts that
    /// say how much of it there is.
    ///
    /// **And the picture is a *column* the reader can wind through** (user
    /// ruling 2026-08-26). One page was the second ruling's whole scope and the
    /// machine showed what that meant: a card over a three-page report drew page
    /// one, showed a document three pages long in the line underneath, and
    /// answered a wheel by doing nothing at all. The report was there and it
    /// could not be read.
    ///
    /// **It is the card's own scroller and not a second one.** A wheel over the
    /// card has gone down `Runtime::scroll_preview_body` since the 2026-08-14
    /// ruling put a markdown glance on the pane's own door; a page column takes
    /// the identical route, so the notch travel, the clamp and the thumb are one
    /// implementation for both bodies. The source of an `.html` and the pages of
    /// a `.pdf` scroll the same way because they scroll through the same code.
    ///
    /// **Every page gets the same slot, and that is what makes the reach
    /// knowable before the pixels are.** The count is read off the file's
    /// structure ([`crate::pdf::page_count`]) in the time a scan takes, while a
    /// page is tens to hundreds of milliseconds of rasterisation each. So the
    /// column is `pages` boxes of [`PEEK_PAGE_H_LOGICAL_PX`] with
    /// [`PEEK_PAGE_GAP_LOGICAL_PX`] between them, reserved the moment the count
    /// lands; a page that arrives is fitted inside its own box exactly as the
    /// cover always was — its own proportions, centred — so a document of mixed
    /// page sizes lays out correctly without the column ever changing length
    /// underneath the hand that is scrolling it.
    ///
    /// **All three parts are optional and the box is not.** The picture is
    /// rastered on a worker and the facts are read on another, so both land a
    /// frame or several after the card is up; a body that grew when they arrived
    /// would be a card that jumped under the pointer. The height is the page box
    /// plus two lines from the first frame, whether or not anything has filled
    /// them. What never arrives is simply not drawn — the same silence
    /// [`Self::Image`] keeps while a decode is in flight, and the whole of this
    /// feature's degradation: a file that will not raster leaves an empty ground
    /// with its size printed underneath.
    Facts {
        /// How far down the column of pages this card is wound, in physical
        /// pixels — always between zero and
        /// [`peek_page_column_max_scroll`].
        ///
        /// **State on a layout, deliberately.** Every other thing in this enum
        /// is a size, and this is not one; it is here because the card's slot
        /// rectangles are the *only* thing it changes, and a painter that had to
        /// be handed the offset separately from the body it belongs to would be
        /// a card whose pages and whose scroll bar could disagree by a frame.
        /// The offset itself lives where every preview surface's does — on
        /// `PreviewPane::scroll`, through `PreviewSurface::Peek`.
        scroll: f32,
        bytes: Option<u64>,
        pages: Option<u32>,
    },
    /// **One frame of a video, and the two lines that say what it is** (user
    /// ruling 2026-08-27; `docs/DESIGN.md` §7.23).
    ///
    /// [`Self::Facts`] with the column taken out of it, and every sentence about
    /// that body holds here: the box is reserved from the first frame whether or
    /// not anything has filled it, all three parts are optional, and what never
    /// arrives is simply not drawn. It takes the **page's** ground rather than
    /// the picture's for the same reason a PDF does — there are two lines under
    /// it, and the picture's box is sized for a body with nothing underneath.
    ///
    /// **There is no column, and that is the difference.** A PDF has pages and a
    /// reader winds through them; a video has *time*, and the frame this card
    /// shows is one moment of it chosen by
    /// [`bt_platform::video::SEEK_FRACTION`]. Winding a video is playing it, and
    /// playing it is not what this build does — so the body is one sheet, the
    /// card has no scroller over it, and the wheel does what it does over any
    /// other unscrollable card.
    ///
    /// **`width`/`height` are the frame's own, already fitted**, exactly as
    /// [`Self::Image`]'s are; zero while the decode and the resample are in
    /// flight, which draws the ground and no picture.
    Frame {
        width: f32,
        height: f32,
        /// What the same open of the file learned besides its picture. Rendered
        /// through [`crate::preview::video_fact_lines`], which is the one place
        /// those two sentences are written for either surface that shows them.
        facts: crate::preview::VideoFacts,
    },
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
            // One line each, and the same one: two sentences of the same size in
            // the same box, so a column of rows does not change shape as the
            // pointer walks from a file with no reader to a file that opens as a
            // page.
            Self::Refused | Self::Page => {
                px(PEEK_NONE_PADDING_TOP_LOGICAL_PX)
                    + none_line(scale)
                    + px(PEEK_NONE_PADDING_BOTTOM_LOGICAL_PX)
            }
            // **The page's whole box and two lines, always** — see
            // [`Self::Facts`] for why none of the three waits to learn whether
            // it has anything in it. The frame around the page is the picture's
            // (2 above, 8 below the lot), because that is what it is.
            //
            // **And a video's frame is that same box**, one sheet instead of a
            // column: it is a picture with two lines under it, which is the
            // shape this arm already describes.
            Self::Facts { .. } | Self::Frame { .. } => {
                px(PEEK_BODY_PADDING_TOP_LOGICAL_PX)
                    + px(PEEK_PAGE_H_LOGICAL_PX)
                    + px(PEEK_PAGE_GAP_LOGICAL_PX)
                    + none_line(scale) * 2.0
                    + px(PEEK_BODY_PADDING_BOTTOM_LOGICAL_PX)
            }
        }
    }
}

/// One line of the card's own voice — the height a sentence in
/// [`PEEK_NONE_FONT_LOGICAL_PX`] occupies.
fn none_line(scale: f32) -> f32 {
    (PEEK_NONE_FONT_LOGICAL_PX * scale * 1.4).round()
}

/// **The two lines a `.pdf`'s card prints**, in the order a reader wants them
/// (user ruling 2026-08-25).
///
/// The page count first, because it is the fact that is about the *document* —
/// how much there is to read — and the size second, because it is about the
/// file. Either may be missing and neither is replaced by a placeholder when it
/// is: what is unknown is left unsaid, and the line below it does not move up to
/// take its place, so the two facts always stand where the last card put them.
#[must_use]
pub fn facts_lines(bytes: Option<u64>, pages: Option<u32>) -> [Option<String>; 2] {
    [
        pages.map(|pages| crate::i18n::peek_page_count(pages as usize)),
        bytes.map(crate::preview::format_byte_size),
    ]
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

/// One page of a [`PeekBody::Facts`] column, and **which page it is**.
///
/// The index rides with the pixels because the column is drawn from what has
/// arrived rather than from what was asked for: pages come home in whatever
/// order the worker finishes them, the cache holds the last few, and a picture
/// that did not carry its own number would have to be placed by its position in
/// a list — which is how page three ends up drawn in page two's slot the first
/// time a scroll outruns the rasteriser.
pub struct PeekPage<'a> {
    pub index: u32,
    pub picture: PeekPicture<'a>,
}

/// The ground something drawn stands on: a `width` × `height` frame hung under
/// the body's own top padding, centred in the card and cut by it if the card is
/// short.
///
/// One function for the picture's frame and the page's because they are one
/// shape asked for at two sizes — see [`picture_ground`] and [`page_ground`],
/// which are what the rest of the module says instead of repeating the numbers.
#[must_use]
fn fitted_ground(body: [f32; 4], scale: f32, width: f32, height: f32) -> [f32; 4] {
    let px = |logical: f32| logical * scale;
    let width = px(width).min(body[2] - body[0] - px(PEEK_BODY_PADDING_X_LOGICAL_PX) * 2.0);
    let left = ((body[0] + body[2] - width) / 2.0).round();
    let top = body[1] + px(PEEK_BODY_PADDING_TOP_LOGICAL_PX);
    let bottom = (top + px(height)).min(body[3]);
    [left, top.round(), left + width, bottom.round()]
}

/// The ground a picture stands on: the mock-up's own 280×120 frame.
#[must_use]
fn picture_ground(body: [f32; 4], scale: f32) -> [f32; 4] {
    fitted_ground(
        body,
        scale,
        PEEK_IMAGE_W_LOGICAL_PX,
        PEEK_IMAGE_H_LOGICAL_PX,
    )
}

/// The ground a page stands on — the same frame, forty pixels taller, because a
/// page is usually portrait ([`PEEK_PAGE_H_LOGICAL_PX`]).
/// **The viewport the column of pages is wound through** — one page's box, in
/// the place the cover always stood.
///
/// `pub` since the column exists (user ruling 2026-08-26): the scroll bar rides
/// this rectangle rather than the card's whole body, because it is the reach of
/// *this* scroller and a rule down the side of the fact lines underneath would
/// be a bar claiming to measure two sentences that never move.
#[must_use]
pub fn page_ground(body: [f32; 4], scale: f32) -> [f32; 4] {
    fitted_ground(body, scale, PEEK_PAGE_W_LOGICAL_PX, PEEK_PAGE_H_LOGICAL_PX)
}

/// How far apart two pages' boxes start, in physical pixels — one page plus the
/// air under it.
#[must_use]
fn page_pitch(scale: f32) -> f32 {
    (PEEK_PAGE_H_LOGICAL_PX + PEEK_PAGE_GAP_LOGICAL_PX) * scale
}

/// **The whole column's height** for a document of `pages` pages, in physical
/// pixels — what the scroll bar's thumb is a proportion of.
///
/// `n` boxes with `n − 1` gaps between them: the air *under* the last page is
/// the air above the fact lines, which [`PeekBody::height`] already reserves, so
/// counting it here would give the column one gap of reach that shows nothing.
///
/// A document whose count has not arrived (or that reports none) is one box
/// tall, which is exactly the cover the card drew before this ruling — a
/// scroller with nowhere to go, and the honest picture of "one page is all this
/// window can say there is".
#[must_use]
pub fn peek_page_column_height(pages: u32, scale: f32) -> f32 {
    let slots = pages.max(1) as f32;
    slots * PEEK_PAGE_H_LOGICAL_PX * scale + (slots - 1.0) * PEEK_PAGE_GAP_LOGICAL_PX * scale
}

/// **How far the column can be wound**, in physical pixels.
///
/// The column less the one page's worth of it that is on screen — and the whole
/// reason this is knowable from a page *count* is [`PeekBody::Facts`]'s uniform
/// slot: no raster has to have arrived for the reach to be right, so a wheel
/// works on the frame the count lands rather than on the frame the last page
/// finishes drawing.
#[must_use]
pub fn peek_page_column_max_scroll(pages: u32, scale: f32) -> f32 {
    (peek_page_column_height(pages, scale) - PEEK_PAGE_H_LOGICAL_PX * scale).max(0.0)
}

/// **Which pages a column wound to `scroll` is showing**, as a half-open range.
///
/// The question the request lane asks: a page nobody can see is a page nobody
/// should be rastering, and a page one pixel of which is visible is a page whose
/// absence the reader can see. So a slot counts as in view when it *overlaps*
/// the viewport at all rather than when it is wholly inside it — the same test
/// the drawing does, which is what keeps "asked for" and "drawn" the same set.
///
/// Clamped to the document, so an offset left over from a longer file cannot ask
/// for pages that are not there.
#[must_use]
pub fn peek_pages_in_view(pages: u32, scroll: f32, scale: f32) -> std::ops::Range<u32> {
    let count = pages.max(1);
    let pitch = page_pitch(scale);
    if pitch <= 0.0 {
        return 0..count.min(1);
    }
    let scroll = scroll.max(0.0);
    let first = (scroll / pitch).floor().max(0.0) as u32;
    // The last slot whose top is above the viewport's bottom edge. The viewport
    // is exactly one page tall, so this is `first + 1` unless the offset has
    // stopped between two pages, in which case both are on screen.
    let last = ((scroll + PEEK_PAGE_H_LOGICAL_PX * scale) / pitch)
        .ceil()
        .max(1.0) as u32;
    first.min(count.saturating_sub(1))..last.min(count).max(first.min(count.saturating_sub(1)) + 1)
}

/// **Where page `index` stands** in a column wound to `scroll`, in the viewport
/// `ground`.
///
/// Off the top or off the bottom is an ordinary answer: the caller draws it and
/// the clip on the picture cuts it. One arithmetic for the request lane, the
/// drawing and the tests, which is what keeps the page a reader sees and the
/// page this window asked for the same page.
#[must_use]
pub fn peek_page_slot(ground: [f32; 4], index: u32, scroll: f32, scale: f32) -> [f32; 4] {
    let top = (ground[1] + index as f32 * page_pitch(scale) - scroll).round();
    [
        ground[0],
        top,
        ground[2],
        top + (PEEK_PAGE_H_LOGICAL_PX * scale).round(),
    ]
}

/// Something already fitted, centred on the ground it was fitted to.
#[must_use]
fn centred_on(ground: [f32; 4], width: f32, height: f32) -> Option<[f32; 4]> {
    if !(width >= 1.0 && height >= 1.0) {
        return None;
    }
    let (width, height) = (
        width.min(ground[2] - ground[0]),
        height.min(ground[3] - ground[1]),
    );
    let left = ((ground[0] + ground[2] - width) / 2.0).round();
    let top = ((ground[1] + ground[3] - height) / 2.0).round();
    Some([left, top, left + width, top + height])
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
    centred_on(picture_ground(layout.body, scale), width, height)
}

/// Where one rastered page goes **inside the slot the column reserved for it**:
/// its own size, centred.
///
/// The slot rather than the ground, which is the whole difference the column
/// made: every page keeps its own proportions in a box of one fixed height, so a
/// report of mixed page sizes reads correctly and none of them moves the pages
/// after it. `None` for a raster that has not come home, or came home
/// empty-handed — the slot's ground is drawn either way, which is what makes a
/// page still loading a blank sheet rather than a gap.
#[must_use]
pub fn peek_page_rect(slot: [f32; 4], size: Option<[f32; 2]>) -> Option<[f32; 4]> {
    let [width, height] = size?;
    centred_on(slot, width, height)
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
    /// **The whole head band**, hairline to body — the strip the mark, the name
    /// and the type chip stand in, and the card's handle (user ruling
    /// 2026-08-27).
    ///
    /// Derived here rather than reassembled by a hit test out of `mark`, `name`
    /// and `ftype`, because those three are what is *drawn* in it and the band
    /// is wider than all of them put together: the padding either side of the
    /// name is head too, and a handle that stopped at the last glyph would be a
    /// handle with holes in it. One derivation, two readers — the painter and
    /// [`press_at`] — which is the same rule the frame itself is filed under.
    pub head: [f32; 4],
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
        head: [
            frame[0] + border,
            frame[1] + border,
            frame[2] - border,
            body_top,
        ],
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
    pages: &[PeekPage<'_>],
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
        mono: false,
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
        // The same drawing and the same ink the preview head and the switcher
        // put on an unsaved buffer — one dot means one thing, and since the icon
        // block that is true of the *drawing* rather than of a codepoint. It was
        // `●` at 9px here and `●` at 13px there, which is two diameters for one
        // claim before the font gets a say; `marks::dirty_dot_sprite` strikes
        // the one circle at the one size.
        sprites.push(crate::marks::dirty_dot_sprite(rect, palette.accent, scale));
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
        mono: false,
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
                    // The card's picture is always fitted to its ground, so it
                    // is inside its own box by construction — the crop is the
                    // preview float's, whose picture can be zoomed past its body.
                    clip: None,
                    above_text: false,
                });
            }
        }
        PeekBody::Refused | PeekBody::Page => {
            let left = layout.body[0] + px(PEEK_NONE_PADDING_X_LOGICAL_PX);
            let right = layout.body[2] - px(PEEK_NONE_PADDING_X_LOGICAL_PX);
            let top = layout.body[1] + px(PEEK_NONE_PADDING_TOP_LOGICAL_PX);
            let words = if matches!(content.body, PeekBody::Page) {
                peek_page_text()
            } else {
                peek_unknown_text()
            };
            labels.push(label(
                words,
                [left, top, right, layout.body[3]],
                px(PEEK_NONE_FONT_LOGICAL_PX),
                palette.body_hint_text,
            ));
        }
        // The column of pages over the facts. The pages are drawn exactly as the
        // image body's picture is — their own box, their own fit — and the two
        // sentences stand under them in the card's own voice, which is what that
        // ink is for. A fact that has not arrived leaves its line empty rather
        // than shifting the other one, and a page that has not arrived leaves a
        // blank sheet rather than moving anything — see [`PeekBody::Facts`].
        PeekBody::Facts {
            bytes,
            pages: count,
            scroll,
        } => {
            let ground = page_ground(layout.body, scale);
            quads.extend(bt_render::rounded_overlay_fill(
                ground,
                px(PEEK_IMAGE_RADIUS_LOGICAL_PX),
                // **The gutter between the sheets**, and the window they are
                // seen through: whatever is behind the drawn thing is the
                // terminal's own. It is drawn once, for the whole viewport,
                // rather than once per sheet — a rounded corner belongs to the
                // window a column is wound past and not to a page that happens
                // to be halfway out of it.
                bt_render::background_rgb(),
                1.0,
            ));
            let total = count.unwrap_or(1);
            for index in peek_pages_in_view(total, *scroll, scale) {
                let slot = peek_page_slot(ground, index, *scroll, scale);
                // **Every slot gets its paper before any of them gets its ink.**
                // A page is rastered onto white ([`crate::pdf::PageRaster`]), so
                // a white sheet is not a placeholder standing in for the page —
                // it is the page, with nothing printed on it yet, and the raster
                // landing changes no pixel that was not going to change. Flat
                // and cropped, because a sheet scrolled halfway out of the
                // window is cut by the window's edge.
                let sheet = [
                    slot[0],
                    slot[1].max(ground[1]),
                    slot[2],
                    slot[3].min(ground[3]),
                ];
                if sheet[3] > sheet[1] {
                    quads.push(OverlayQuad {
                        rect: sheet,
                        color: [255, 255, 255],
                        alpha: 1.0,
                    });
                }
                let Some(page) = pages.iter().find(|page| page.index == index) else {
                    continue;
                };
                let Some(rect) = peek_page_rect(
                    slot,
                    Some([page.picture.width_px as f32, page.picture.height_px as f32]),
                ) else {
                    continue;
                };
                images.push(bt_render::ChromeIcon {
                    key: page.picture.key.to_owned(),
                    rect,
                    rgba: std::sync::Arc::clone(page.picture.rgba),
                    width_px: page.picture.width_px,
                    height_px: page.picture.height_px,
                    opacity: 1.0,
                    // **Cropped, unlike the cover it replaced.** That one was
                    // fitted to a box it could never leave; these are wound past
                    // a window, and the sheet at each end of the run is
                    // deliberately half outside it.
                    clip: Some(ground),
                    above_text: false,
                });
            }
            let left = layout.body[0] + px(PEEK_NONE_PADDING_X_LOGICAL_PX);
            let right = layout.body[2] - px(PEEK_NONE_PADDING_X_LOGICAL_PX);
            // Under the page's whole box rather than under the picture that
            // landed in it: the lines stand where the last card put them.
            let top = ground[3] + px(PEEK_PAGE_GAP_LOGICAL_PX);
            let line = none_line(scale);
            for (row, words) in facts_lines(*bytes, *count).into_iter().enumerate() {
                let Some(words) = words else {
                    continue;
                };
                let top = top + line * row as f32;
                labels.push(label(
                    &words,
                    [left, top, right, (top + line).min(layout.body[3])],
                    px(PEEK_NONE_FONT_LOGICAL_PX),
                    palette.body_hint_text,
                ));
            }
        }
        // One frame over two lines. The ground, the fit and the ink are the two
        // bodies above put together — a picture's rounded window onto the
        // terminal's own background, the page box's taller shape because there
        // are sentences underneath, and the card's own voice for them. What is
        // deliberately *not* here is a sheet: a PDF's slot is drawn white
        // because a page is printed on paper and a blank sheet is the page with
        // nothing on it yet, while a video frame that has not arrived is not a
        // white rectangle of anything — so its ground stays the window's, which
        // is the same silence [`PeekBody::Image`] keeps.
        PeekBody::Frame {
            width,
            height,
            facts,
        } => {
            let ground = page_ground(layout.body, scale);
            quads.extend(bt_render::rounded_overlay_fill(
                ground,
                px(PEEK_IMAGE_RADIUS_LOGICAL_PX),
                bt_render::background_rgb(),
                1.0,
            ));
            if let Some((rect, picture)) = centred_on(ground, *width, *height).zip(picture) {
                images.push(bt_render::ChromeIcon {
                    key: picture.key.to_owned(),
                    rect,
                    rgba: std::sync::Arc::clone(picture.rgba),
                    width_px: picture.width_px,
                    height_px: picture.height_px,
                    opacity: 1.0,
                    // Fitted to its ground by construction, exactly as the
                    // picture body's is — nothing here is wound past a window.
                    clip: None,
                    above_text: false,
                });
            }
            let left = layout.body[0] + px(PEEK_NONE_PADDING_X_LOGICAL_PX);
            let right = layout.body[2] - px(PEEK_NONE_PADDING_X_LOGICAL_PX);
            // Under the frame's whole box rather than under the frame that
            // landed in it: the lines stand where the last card put them.
            let top = ground[3] + px(PEEK_PAGE_GAP_LOGICAL_PX);
            let line = none_line(scale);
            // **The extension comes off the card's own name**, which is the one
            // spelling of this file the card already has. It is what
            // `video_fact_lines` falls back to when the frame did not decode and
            // there is no length or resolution to print.
            let extension = std::path::Path::new(&content.name)
                .extension()
                .and_then(std::ffi::OsStr::to_str);
            for (row, words) in crate::preview::video_fact_lines(extension, *facts)
                .into_iter()
                .enumerate()
            {
                let Some(words) = words else {
                    continue;
                };
                let top = top + line * row as f32;
                labels.push(label(
                    &words,
                    [left, top, right, (top + line).min(layout.body[3])],
                    px(PEEK_NONE_FONT_LOGICAL_PX),
                    palette.body_hint_text,
                ));
            }
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
            mono: false,
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
                dissolved: 0.0,
                run: foot_run(layout, SCALE),
                lead: peek_foot_text(),
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
            &[],
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
        assert_eq!(bare.lead, peek_foot_text(), "the sentence, whole");
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
            &[],
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
                .any(|label| label.text == peek_foot_text()),
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

    /// RED (user ruling 2026-08-27; `docs/DESIGN.md` §7.23) — **the card over a
    /// video shows the video, says how long it is, and does not jump when either
    /// arrives.**
    ///
    /// Until this ruling a `.mp4` row drew one line — "No preview for this file
    /// type" — under a pointer, and the same sentence larger under a double
    /// click. What this asserts is the whole of what replaced it: a frame in the
    /// page's ground, two lines under it in the card's own voice, and a box that
    /// was already that size before any of it landed.
    ///
    /// **The last of those is not decoration.** The frame comes off a decoder on
    /// another thread and the facts come with it; a body that grew when they
    /// arrived would be a card jumping under a resting pointer, which is the one
    /// thing a glance may never do. So the empty card is measured against the
    /// full one and they are the same height.
    ///
    /// RED GATE ①: fold [`PeekBody::Frame`] into the `Refused | Page` height arm
    /// and the empty-versus-full assertion still passes while the *first* one
    /// fails — the card shrinks to one line and the frame is drawn over the
    /// foot. RED GATE ②: draw the frame through the `Image` arm and the two fact
    /// lines vanish, which is a card that shows a picture and cannot say what it
    /// is a picture of.
    #[test]
    fn a_video_card_shows_a_frame_over_two_lines() {
        let window = (1200.0, 900.0);
        let mut card = content(PeekBody::Frame {
            width: 280.0,
            height: 158.0,
            facts: crate::preview::VideoFacts {
                duration_ms: Some(6_200),
                native: Some((1920, 1080)),
                bytes: Some(12_582_912),
            },
        });
        card.name = "clip.mp4".to_owned();
        card.ftype = "video".to_owned();
        let row = [40.0, 300.0, 240.0, 320.0];
        let stated = layout(&card, row, window, 120.0, 24.0, SCALE);
        let picture: std::sync::Arc<[u8]> = std::sync::Arc::from(vec![0_u8; 4].into_boxed_slice());
        let layer = build(
            &stated,
            &card,
            &foot(&stated, ""),
            Some(PeekPicture {
                key: "video-frame:clip",
                rgba: &picture,
                width_px: 280,
                height_px: 158,
            }),
            &[],
            &bt_render::chrome_palette(),
            SCALE,
        );
        // **The box is a frame's box and not a sentence's**: the page's ground
        // plus the gap plus two lines, which is exactly what a PDF's card
        // reserves — and three times what the refusal it replaced took.
        assert_eq!(
            PeekBody::Frame {
                width: 280.0,
                height: 158.0,
                facts: crate::preview::VideoFacts::default(),
            }
            .height(SCALE),
            PeekBody::Facts {
                scroll: 0.0,
                bytes: None,
                pages: None,
            }
            .height(SCALE),
            "a frame over two lines is the same box a page over two lines is"
        );
        assert!(
            PeekBody::Frame {
                width: 0.0,
                height: 0.0,
                facts: crate::preview::VideoFacts::default(),
            }
            .height(SCALE)
                > PeekBody::Refused.height(SCALE) * 2.0,
            "and it is not the one-line box the refusal used"
        );
        let said = |text: &str| layer.labels.iter().find(|label| label.text == text);
        let what =
            said("0:06 \u{b7} 1920 \u{d7} 1080").expect("the card says how long and how big");
        let size = said("12.0 MB").expect("and how large the file is");
        assert!(
            what.rect[1] < size.rect[1],
            "the recording's fact stands above the file's: {:?} vs {:?}",
            what.rect,
            size.rect
        );
        let frame = layer
            .images
            .iter()
            .find(|image| image.key == "video-frame:clip")
            .expect("the frame is drawn");
        let ground = page_ground(stated.body, SCALE);
        assert!(
            frame.rect[1] >= ground[1] - 0.5 && frame.rect[3] <= ground[3] + 0.5,
            "the frame stands inside the ground it was fitted to: {:?} in {:?}",
            frame.rect,
            ground
        );
        assert!(
            frame.rect[3] <= what.rect[1] + 0.5,
            "and the lines stand under it, not over it"
        );
        assert!(
            said(peek_unknown_text()).is_none(),
            "the refusal this ruling abolished is gone"
        );
        assert!(said("clip.mp4").is_some(), "the head names the file");
        assert!(said("video").is_some(), "and the chip calls it a video");

        // **The box does not wait to learn what is in it**, which is the same
        // sentence the facts body makes and for the same reason: a decoder
        // answers a frame or two after the card is up.
        let empty = content(PeekBody::Frame {
            width: 0.0,
            height: 0.0,
            facts: crate::preview::VideoFacts::default(),
        });
        let pending = layout(&empty, row, window, 120.0, 24.0, SCALE);
        assert_eq!(
            pending.body[3] - pending.body[1],
            stated.body[3] - stated.body[1],
            "the frame's box is the same height before the frame arrives"
        );
        let blank = build(
            &pending,
            &empty,
            &foot(&pending, ""),
            None,
            &[],
            &bt_render::chrome_palette(),
            SCALE,
        );
        assert!(
            blank.images.is_empty(),
            "a frame that has not arrived draws no picture"
        );
        assert!(
            !blank.quads.is_empty(),
            "and the ground it will land in is there from the first frame"
        );
    }

    /// PIN (user ruling 2026-08-25; `docs/DESIGN.md` §7.10 ⑥) — **the card over a
    /// page states the two facts a renderer is not needed for.**
    ///
    /// A `.pdf` row used to draw one line — `Opens as a page.` — which told a
    /// reader what a double click does and nothing about the file they were
    /// pointing at. The two facts here are the ones that can be read off the
    /// file, and they are drawn in the card's own voice, in the box the refusal's
    /// line stood in: this is the card *saying* something, not the card showing a
    /// document, so it belongs on the layer that draws the head and the foot.
    ///
    /// The picture that later arrived above them is the next test's; every card
    /// built here is one whose raster has not come home, which is also the shape
    /// every `.pdf` card wears for its first frames.
    ///
    /// The head and the foot are asserted with them because they are the half of
    /// the ruling that did **not** change: the chip still says `web` and the foot
    /// still says how to open the row.
    ///
    /// RED GATE: draw [`PeekBody::Facts`] through the `Refused | Page` arm — the
    /// card comes back with `Opens as a page.` in it and not one fact, which is
    /// the state this ruling overturned.
    #[test]
    fn a_page_card_states_the_two_facts_it_can_read() {
        let window = (1200.0, 900.0);
        let mut card = content(PeekBody::Facts {
            scroll: 0.0,
            bytes: Some(83_387),
            pages: Some(3),
        });
        card.name = "folio-pdf-test.pdf".to_owned();
        card.ftype = "web".to_owned();
        let row = [40.0, 300.0, 240.0, 320.0];
        let stated = layout(&card, row, window, 120.0, 24.0, SCALE);
        let layer = build(
            &stated,
            &card,
            &foot(&stated, ""),
            None,
            &[],
            &bt_render::chrome_palette(),
            SCALE,
        );
        let said = |text: &str| layer.labels.iter().find(|label| label.text == text);
        let pages = said("3 pages").expect("the card says how many pages there are");
        let size = said("81 KB").expect("and how large the file is");
        assert!(
            pages.rect[1] < size.rect[1],
            "the document's fact stands above the file's: {:?} vs {:?}",
            pages.rect,
            size.rect
        );
        for fact in [pages, size] {
            assert!(
                fact.rect[1] >= stated.body[1] - 0.5 && fact.rect[3] <= stated.body[3] + 0.5,
                "a fact stands outside the body box: {:?}",
                fact.rect
            );
        }
        assert!(
            said(peek_page_text()).is_none(),
            "and it does not also say the sentence it replaced"
        );
        assert!(
            said(peek_unknown_text()).is_none(),
            "nor the refusal, which was never true of a row that opens"
        );
        // The half the ruling did not touch.
        assert!(
            said("folio-pdf-test.pdf").is_some(),
            "the head names the file"
        );
        assert!(said("web").is_some(), "the chip still calls it a page");
        assert!(said(peek_foot_text()).is_some(), "the foot still says how");

        // **The box does not wait to learn what is in it.** The facts arrive from
        // a worker one or two frames after the card is up, and a body that grew
        // when they landed would be a card that jumped under the pointer.
        let empty = content(PeekBody::Facts {
            scroll: 0.0,
            bytes: None,
            pages: None,
        });
        let pending = layout(&empty, row, window, 120.0, 24.0, SCALE);
        assert_eq!(
            pending.body[3] - pending.body[1],
            stated.body[3] - stated.body[1],
            "the facts box is the same height before its facts arrive"
        );
        let blank = build(
            &pending,
            &empty,
            &foot(&pending, ""),
            None,
            &[],
            &bt_render::chrome_palette(),
            SCALE,
        );
        let inside =
            |rect: [f32; 4]| rect[1] >= pending.body[1] - 0.5 && rect[3] <= pending.body[3] + 0.5;
        assert!(
            !blank.labels.iter().any(|label| inside(label.rect)),
            "a fact that has not arrived is silence, not a placeholder"
        );

        // And a file the disk would not stat still says what it can: one fact is
        // better than none, and it stands where it always stands.
        let one = content(PeekBody::Facts {
            scroll: 0.0,
            bytes: None,
            pages: Some(12),
        });
        let single = layout(&one, row, window, 120.0, 24.0, SCALE);
        let layer = build(
            &single,
            &one,
            &foot(&single, ""),
            None,
            &[],
            &bt_render::chrome_palette(),
            SCALE,
        );
        let lone = layer
            .labels
            .iter()
            .find(|label| label.text == "12 pages")
            .expect("the fact that is known is printed");
        assert_eq!(
            lone.rect[1], pages.rect[1],
            "and the surviving fact stands on the line it always stands on"
        );

        // The other way round: with only the size known, the size does **not**
        // climb into the page count's line.
        let other = content(PeekBody::Facts {
            scroll: 0.0,
            bytes: Some(83_387),
            pages: None,
        });
        let sized = layout(&other, row, window, 120.0, 24.0, SCALE);
        let layer = build(
            &sized,
            &other,
            &foot(&sized, ""),
            None,
            &[],
            &bt_render::chrome_palette(),
            SCALE,
        );
        let lone = layer
            .labels
            .iter()
            .find(|label| label.text == "81 KB")
            .expect("the fact that is known is printed");
        assert_eq!(
            lone.rect[1], size.rect[1],
            "the second line is still the second line with the first one missing"
        );
    }

    /// RED (user rulings 2026-08-25 and 2026-08-26) — **the page card draws a
    /// column of pages over its two facts, scrolls it, and reserves the box for
    /// it before any raster is home.**
    ///
    /// The card's founding sentence was *there is no engine on a hover card*, and
    /// it stayed true of the pane's engine and stopped being true of the card the
    /// day this window grew a rasteriser ([`crate::pdf::page_raster`]). The
    /// 2026-08-25 ruling put one page there; this pins what the 2026-08-26 ruling
    /// made of it, which is the report the user filed with a screenshot: a card
    /// over a three-page document that drew page one, said `3 pages` underneath,
    /// and answered a wheel by doing nothing.
    ///
    /// Four properties, and each is a way the column could be wrong:
    ///
    /// * the pages are **above the words**, which the single page already was;
    /// * the card is the **same height** at every offset and every page count, so
    ///   nothing moves under a stationary pointer;
    /// * **winding it moves the pages and nothing else** — the facts underneath
    ///   are the card speaking and do not scroll;
    /// * a page arrives **in its own slot**, found by its index rather than by
    ///   its place in the run that happened to be home.
    ///
    /// RED GATE ①: draw every arrived page at `page_ground` instead of at its own
    /// [`peek_page_slot`] and the second page lands on the first — the offset
    /// assertion fails. RED GATE ②: leave the column out of [`PeekBody::height`]
    /// and the card waiting for its rasters is 160 pixels shorter than the one
    /// that has them, which on screen is a card growing under a stationary
    /// pointer. RED GATE ③: place a picture by its position in `pages` rather
    /// than by `PeekPage::index` and the last block fails with page 2 drawn in
    /// page 1's slot, which is what a scroll that outruns the rasteriser
    /// produces.
    #[test]
    fn a_page_card_draws_a_column_of_pages_over_the_facts() {
        let window = (1200.0, 900.0);
        let row = [40.0, 300.0, 240.0, 320.0];
        let card = |scroll: f32| {
            content(PeekBody::Facts {
                scroll,
                bytes: Some(83_387),
                pages: Some(3),
            })
        };
        let rested = card(0.0);
        let at_rest = layout(&rested, row, window, 120.0, 24.0, SCALE);
        let wound_card = card(peek_page_column_max_scroll(3, SCALE));
        let wound = layout(&wound_card, row, window, 120.0, 24.0, SCALE);
        assert_eq!(
            at_rest.frame, wound.frame,
            "the card is the same card wherever the column is wound to"
        );

        // A US Letter page fitted into 280×160 by height, which is the shape
        // nearly every hovered `.pdf` produces.
        let pixels: std::sync::Arc<[u8]> =
            std::sync::Arc::from(vec![0_u8; 124 * 160 * 4].into_boxed_slice());
        let page = |index: u32, key: &'static str| PeekPage {
            index,
            picture: PeekPicture {
                key,
                rgba: &pixels,
                width_px: 124,
                height_px: 160,
            },
        };
        let drawn = |laid: &PeekLayout, card: &PeekContent, pages: &[PeekPage<'_>]| {
            build(
                laid,
                card,
                &foot(laid, ""),
                None,
                pages,
                &bt_render::chrome_palette(),
                SCALE,
            )
        };

        let layer = drawn(&at_rest, &rested, &[page(0, "peek-page:1")]);
        let first = layer
            .images
            .iter()
            .find(|icon| icon.key == "peek-page:1")
            .expect("the page that has arrived reaches the renderer");
        assert!(
            (first.rect[2] - first.rect[0] - 124.0).abs() < 0.5
                && (first.rect[3] - first.rect[1] - 160.0).abs() < 0.5,
            "drawn at the size the rasteriser fitted it to: {:?}",
            first.rect
        );
        let said = |layer: &crate::marks::OverlayLayer, text: &str| {
            layer
                .labels
                .iter()
                .find(|label| label.text == text)
                .unwrap_or_else(|| panic!("the card says {text}"))
                .rect
        };
        assert!(
            first.rect[3] <= said(&layer, "3 pages")[1],
            "the column stands above the facts: {:?} vs {:?}",
            first.rect,
            said(&layer, "3 pages")
        );

        // **The pages move and the sentences do not.** Half a slot of the column,
        // which is the offset that puts two pages in the window at once — the
        // first one wound off the top and cut by it, the second arriving from
        // below — while `3 pages` has not shifted a pixel.
        let one_slot = (PEEK_PAGE_H_LOGICAL_PX + PEEK_PAGE_GAP_LOGICAL_PX) * SCALE;
        let half = (one_slot / 2.0).round();
        let notched_card = card(half);
        let notched = layout(&notched_card, row, window, 120.0, 24.0, SCALE);
        let after = drawn(
            &notched,
            &notched_card,
            &[page(0, "peek-page:1"), page(1, "peek-page:2")],
        );
        let moved = after
            .images
            .iter()
            .find(|icon| icon.key == "peek-page:1")
            .expect("the page wound off the top is still drawn, and clipped");
        assert!(
            (first.rect[1] - moved.rect[1] - half).abs() < 1.5,
            "the wheel moved page 1 by exactly what it was wound: {:?} then {:?}",
            first.rect,
            moved.rect
        );
        assert_eq!(
            said(&layer, "3 pages"),
            said(&after, "3 pages"),
            "and the facts underneath did not move at all"
        );
        let second = after
            .images
            .iter()
            .find(|icon| icon.key == "peek-page:2")
            .expect("and page 2 is on the glass now that it is in view");
        assert!(
            (second.rect[1] - moved.rect[1] - one_slot).abs() < 1.5,
            "page 2 stands one slot below page 1: {:?} then {:?}",
            moved.rect,
            second.rect
        );
        for icon in [moved, second] {
            assert_eq!(
                icon.clip,
                Some(page_ground(notched.body, SCALE)),
                "a wound page is cut by the window it is wound past"
            );
        }

        // **A page is placed by its number and never by its turn.** Hand the
        // painter only page 2 — which is what a scroll that outran the rasteriser
        // leaves in the cache — and it must land where page 2 goes.
        let alone = drawn(&notched, &notched_card, &[page(1, "peek-page:2")]);
        assert_eq!(
            alone
                .images
                .iter()
                .find(|icon| icon.key == "peek-page:2")
                .expect("the one page that is home is drawn")
                .rect,
            second.rect,
            "page 2 is drawn in page 2's slot whoever else is missing"
        );

        // The whole arrangement fits the card's own cap, which is what decides
        // how tall the page box may be — see [`PEEK_PAGE_H_LOGICAL_PX`].
        assert!(
            at_rest.frame[3] - at_rest.frame[1] <= PEEK_MAX_HEIGHT_LOGICAL_PX,
            "the card is inside its cap: {}",
            at_rest.frame[3] - at_rest.frame[1]
        );
        assert!(
            at_rest.foot[1] >= at_rest.body[3],
            "so nothing is cut off the bottom: {at_rest:?}"
        );
    }

    /// RED (user ruling 2026-08-26) — **how far a column of pages reaches, and
    /// which of them are on screen.**
    ///
    /// The arithmetic the whole feature rests on, pinned on its own because three
    /// separate things read it and must read it identically: the wheel's clamp
    /// (through `Runtime::preview_max_scroll`), the request lane that decides
    /// which pages to raster, and the paint that decides which to draw. A reach
    /// that disagreed with the drawing is a document with pages the reader can
    /// see and cannot get to; a range that disagreed with the requests is a slot
    /// that stays a blank sheet for ever because nobody asked for it.
    ///
    /// RED GATE ①: count the gap *under* the last page in
    /// [`peek_page_column_height`] and the reach gains a slot of air the reader
    /// can wind into and find nothing in. RED GATE ②: make
    /// [`peek_pages_in_view`] answer only the page that is wholly on screen and
    /// the second block fails — mid-scroll, the half-page at the bottom of the
    /// window is never asked for, so it is a blank sheet exactly while a reader
    /// is looking at it.
    #[test]
    fn a_page_columns_reach_is_its_page_count_and_its_view_is_what_overlaps() {
        for scale in [1.0_f32, 2.0] {
            let slot = PEEK_PAGE_H_LOGICAL_PX * scale;
            let pitch = (PEEK_PAGE_H_LOGICAL_PX + PEEK_PAGE_GAP_LOGICAL_PX) * scale;
            assert_eq!(
                peek_page_column_height(3, scale),
                slot * 3.0 + PEEK_PAGE_GAP_LOGICAL_PX * scale * 2.0,
                "three pages and the two gaps between them, at {scale}×"
            );
            assert_eq!(
                peek_page_column_max_scroll(3, scale),
                pitch * 2.0,
                "so the reach is two whole slots, at {scale}×"
            );
            assert_eq!(
                peek_page_column_max_scroll(1, scale),
                0.0,
                "a one-page document does not scroll"
            );
            assert_eq!(
                peek_page_column_max_scroll(0, scale),
                0.0,
                "and neither does one whose count never arrived"
            );

            assert_eq!(
                peek_pages_in_view(3, 0.0, scale),
                0..1,
                "at rest the window holds exactly the first page"
            );
            assert_eq!(
                peek_pages_in_view(3, pitch, scale),
                1..2,
                "one whole slot down it holds exactly the second"
            );
            assert_eq!(
                peek_pages_in_view(3, pitch / 2.0, scale),
                0..2,
                "and stopped between them it holds both — which is what has to be asked for"
            );
            assert_eq!(
                peek_pages_in_view(3, pitch * 2.0, scale),
                2..3,
                "at the end of the reach it holds the last page and nothing past it"
            );
            assert_eq!(
                peek_pages_in_view(1, 0.0, scale),
                0..1,
                "a one-page document is one page in view"
            );
        }
    }

    /// PIN — **a page card reserves its page box and both of its lines, and
    /// nothing else.**
    ///
    /// The whole card shrink-wraps its body, so the body's own arithmetic is what
    /// decides the shape of every `.pdf` glance on screen — and that shape has to
    /// be settled before either worker answers, or the card changes size under
    /// the pointer twice ([`PeekBody::Facts`]).
    ///
    /// MUTATION ①: fold [`PeekBody::Facts`] into the `Refused | Page` height arm
    /// — the page and the second fact are both drawn under the card's own bottom
    /// edge.
    /// MUTATION ②: size the box to the page that arrived rather than to
    /// [`PEEK_PAGE_H_LOGICAL_PX`] and the first assertion goes red, which is the
    /// jump.
    #[test]
    fn the_page_body_reserves_its_picture_and_both_of_its_lines() {
        let window = (1200.0, 900.0);
        let row = [40.0, 300.0, 240.0, 320.0];
        let refusal = layout(&content(PeekBody::Refused), row, window, 60.0, 24.0, SCALE);
        let facts = |pages, scroll| {
            layout(
                &content(PeekBody::Facts {
                    scroll,
                    bytes: Some(1),
                    pages,
                }),
                row,
                window,
                60.0,
                24.0,
                SCALE,
            )
        };
        let line = (PEEK_NONE_FONT_LOGICAL_PX * 1.4).round();
        let waiting = facts(Some(1), 0.0);
        let body = waiting.body[3] - waiting.body[1];
        assert_eq!(
            body,
            PEEK_BODY_PADDING_TOP_LOGICAL_PX
                + PEEK_PAGE_H_LOGICAL_PX
                + PEEK_PAGE_GAP_LOGICAL_PX
                + line * 2.0
                + PEEK_BODY_PADDING_BOTTOM_LOGICAL_PX,
            "the page's whole box and two lines, before either has arrived"
        );
        // **A one-page document and a two-hundred-page one are the same card, at
        // any offset**: the box is a window and the column is what moves behind
        // it (user ruling 2026-08-26). A body that grew with the page count would
        // be a card whose height changed the moment a count landed, and one that
        // grew with the offset would be a card that changed shape under a wheel.
        for (pages, scroll) in [
            (Some(1), 0.0),
            (Some(3), 0.0),
            (Some(3), 336.0),
            (Some(200), 900.0),
            (None, 0.0),
        ] {
            let drawn = facts(pages, scroll);
            assert_eq!(
                drawn.body[3] - drawn.body[1],
                body,
                "{pages:?} pages wound to {scroll} changed the body"
            );
        }
        assert!(
            body > refusal.body[3] - refusal.body[1],
            "and it is taller than the one-line refusal it grew out of"
        );
        assert!(
            waiting.foot[1] >= waiting.body[3],
            "and the foot still stands below the body: {waiting:?}"
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
            &[],
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
    ///    — the thumb jumps to put its own head under the pointer at the press;
    /// ④ answer `Open` on the head — the handle goes back to being face, and
    ///    six pixels of intent over the name open a pane instead of keeping a
    ///    window (user ruling 2026-08-27, §7.29);
    /// ⑤ let the head arm swallow the foot as well — a press on "Click to
    ///    open" stops opening anything.
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
            press_at(card.frame, card.head, Some(&bar), on_thumb),
            Press::Thumb(held)
        );

        // ② In the document beside it — the door, not a caret.
        let in_document = [card.body[0] + 20.0, (card.body[1] + card.body[3]) / 2.0];
        assert_eq!(
            press_at(card.frame, card.head, Some(&bar), in_document),
            Press::Open
        );
        // And the foot is the card too: the face below the head is one door,
        // which is what "click the card" means.
        assert_eq!(
            press_at(
                card.frame,
                card.head,
                Some(&bar),
                [card.foot[0] + 20.0, (card.foot[1] + card.foot[3]) / 2.0]
            ),
            Press::Open
        );

        // ③ **The head is not face any more — it is the handle** (user ruling
        //    2026-08-27, §7.29). The name stands in it, and so does the padding
        //    either side of the name: a handle that stopped at the last glyph
        //    would be a handle with holes in it.
        assert_eq!(
            press_at(
                card.frame,
                card.head,
                Some(&bar),
                [card.name[0], card.name[1] + 1.0]
            ),
            Press::Head
        );
        assert_eq!(
            press_at(
                card.frame,
                card.head,
                Some(&bar),
                [(card.head[0] + card.head[2]) / 2.0, card.head[1] + 1.0]
            ),
            Press::Head
        );
        // And it stops where the body starts: one pixel lower is the door.
        assert_eq!(
            press_at(
                card.frame,
                card.head,
                Some(&bar),
                [card.body[0] + 20.0, card.body[1] + 1.0]
            ),
            Press::Open
        );

        // ④ Outside the card is nobody's press — the guard that keeps a click on
        //    the terminal beside the card from opening a file.
        assert_eq!(
            press_at(
                card.frame,
                card.head,
                Some(&bar),
                [card.frame[2] + 4.0, in_document[1]]
            ),
            Press::Elsewhere
        );
        assert_eq!(
            press_at(
                card.frame,
                card.head,
                Some(&bar),
                [in_document[0], card.frame[1] - 4.0]
            ),
            Press::Elsewhere
        );

        // ⑤ With no bar at all — a card whose document fits — the same point on
        //    the edge is the door, because there is nothing there to hold.
        assert_eq!(press_at(card.frame, card.head, None, on_thumb), Press::Open);
    }
}
