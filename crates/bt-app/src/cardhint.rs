//! **The one sentence the Cards column owes a reader who has never been in it**
//! (`docs/DESIGN.md` §7.21, user ruling 2026-08-27).
//!
//! # Why this surface exists at all
//!
//! The 2026-08-26 gesture audit filed exactly seven gestures as 丙 — *invented
//! here, and with not one clue on the glass* — and 丙1 is this one: in Cards,
//! `Alt`+wheel over a card's terminal seat scrolls **that card's own window**, a
//! row per notch. Every surface that could have named it had already been ruled
//! out by something else: the column refuses tooltips, a card is not a `⌄`
//! menu's subject, the wheel is not in the shortcut table, and the 2026-08-20
//! ruling took new affordances off the cards themselves. What was left was a
//! sentence on the *tail* of a settings row about card **height** — and the
//! audit's own verdict on that trade is the reason this file exists: a reader
//! does not go looking for scrolling inside a row about pixels.
//!
//! # What it is, and the two things it is not
//!
//! It is a **bubble bitten onto the staged card**: the card whose tab is on
//! stage, the one a reader is already looking at. A 6px tail names the subject,
//! which is the rule every menu in this house already obeys — an explanation
//! drops out of the thing it belongs to.
//!
//! **It is not a control.** It has no hit box, it is not in any hit test, and
//! nothing anywhere can press it; it is drawn into the overlay and dismissed by
//! its own clock. That is not a promise kept by remembering — there is no path
//! from `chrome_target_at` to anything in this file.
//!
//! **It is not a second answer to "is the reader in Cards".** [`CardHintHost`]
//! is told, every frame, whether the column is drawing and whether the offer is
//! still owed; it holds no opinion of its own about either.
//!
//! # The three clocks, and where each of them lives
//!
//! * **In** — [`bt_render::POPUP_ENTER`] with
//!   [`bt_render::MOTION_TRAVEL_LOGICAL_PX`] of travel, and **not one line of it
//!   is here**: the band goes through [`crate::arrival::Passages::stage`] like
//!   every other layer this window raises, so the entrance is the one every
//!   popup gets, from the direction of the card it grew out of.
//! * **Stand** — [`crate::toast::TOAST_LIFE_QUIET`]. Four seconds is what a
//!   confirmation stands for in this window, and a hint nobody asked for has no
//!   claim to stand longer than a receipt for something they did. This is the
//!   one span this file owns, and it owns it by *naming* the toast's rather than
//!   writing a fourth number.
//! * **Out** — [`bt_render::POPUP_EXIT`], a fade with no travel, also the
//!   passage register's.
//!
//! So the motion register gains no row: every duration on this surface is a
//! constant that was already in it.
//!
//! # The accompaniment, and the red line it is drawn on
//!
//! While the bubble stands, the card it points at **nudges its own content down
//! one row and back** ([`CardHintHost::nudge_rows`]) — out on the slow span,
//! standing for the slow span, back on the slow span, all three from
//! [`bt_render::MOTION_SLOW`], beginning once the bubble has finished arriving.
//! One row and not two, because one row is exactly what one notch of the
//! gesture does: the demonstration and the thing demonstrated are the same
//! number.
//!
//! **Under [`Motion::Reduced`] the nudge does not run, and the bubble still
//! works.** That is the whole reason the nudge is an accompaniment and not the
//! message. The small sample's third candidate put the message *in* the tween —
//! a card that scrolls itself and says nothing — and under reduced motion it
//! degenerated into a single-frame jump carrying no information at all. Here the
//! chord is glyphs and the verb is words, both of them static; what the
//! preference removes is a flourish, and the card that is left says the same
//! thing in the same place for the same four seconds.
//!
//! # Two seats, one rule
//!
//! [`place`] is handed the staged card's head **if it is on the glass**, and
//! answers a bubble bitten onto it. Handed `None` — the column scrolled, the
//! staged card gone past the clip box — it answers the same card in the window's
//! bottom-right corner with no tail, which is [`crate::toast`]'s own anchor and
//! the small sample's variant A. One function, two seats, and the fallback is a
//! *ruling* rather than a guard: a tail that points at nothing is worse than no
//! tail, and a sentence the reader can still read is better than no sentence.

use std::time::{Duration, Instant};

use bt_render::{
    ChromeLabel, ChromeLabelWeight, ChromePalette, MOTION_SLOW, OverlayQuad, POPUP_ENTER, Travel,
};

use crate::icons::{ActionIcon, MarkSlot};
use crate::keyhint::{
    KEY_HINT_BORDER_LOGICAL_PX, KEY_HINT_CAP_GAP_LOGICAL_PX, KEY_HINT_NAME_FONT_LOGICAL_PX,
    KEY_HINT_PADDING_X_LOGICAL_PX, KEY_HINT_PADDING_Y_LOGICAL_PX, KEY_HINT_RADIUS_LOGICAL_PX,
    KEY_HINT_WINDOW_INSET_LOGICAL_PX, cap_box_width,
};
use crate::marks::{ChromeMark, ChromeSprite, OverlayLayer};
use crate::settings::{CAP_FONT_LOGICAL_PX, CAP_HEIGHT_LOGICAL_PX, push_cap, push_float_window};
use crate::toast::TOAST_LIFE_QUIET;
use crate::{EASE, Motion, cubic_bezier};

// ── the box ────────────────────────────────────────────────────────────────

/// How wide the tail is, and therefore how far the bubble stands off its card.
///
/// Six, which is the small sample's own number and the smallest bite that still
/// reads as a point rather than as a bevel. The tail is twice as tall as it is
/// wide, so its apex is a right angle — the geometry a 45°-turned square has,
/// which is how the sample drew it and how every speech bubble in this house's
/// vocabulary would be drawn if there were another one.
pub const CARD_HINT_TAIL_WIDTH_LOGICAL_PX: f32 = 6.0;

/// The gap between the three pieces of the chord: the cap, the `+`, the wheel.
///
/// Six, and deliberately not [`crate::settings::CAP_GAP_LOGICAL_PX`]'s four.
/// Four is the gap between two *caps* — two objects of one kind, read as one
/// chord — and these three are three kinds of object, one of which is a
/// conjunction. The sample's own six is what stops `Alt`, `+` and the wheel
/// from setting as a single crowded glyph.
pub const CARD_HINT_CHORD_GAP_LOGICAL_PX: f32 = 6.0;

/// The `+` between the cap and the wheel, at the muted ink.
///
/// Eleven, the sample's, and the one size in this box that is not
/// [`KEY_HINT_NAME_FONT_LOGICAL_PX`]: a conjunction set at the sentence's own
/// size reads as a word in the sentence.
pub const CARD_HINT_PLUS_FONT_LOGICAL_PX: f32 = 11.0;

/// The cap this card prints, which is the modifier the gesture is held under.
///
/// Written here rather than derived from the shortcut table, and that is the
/// honest arrangement: `Alt`+wheel is not a row of `Shortcuts::BINDINGS` at all
/// — it is a wheel gesture, and the table has no wheel in it — so a lookup
/// would be this file pretending to read something that does not exist. The
/// name it prints is the same name [`crate::shortcuts::live_caps`] prints for
/// the same key, which is the only agreement that has to hold.
pub const CARD_HINT_CAP: &str = "Alt";

// ── the clock ──────────────────────────────────────────────────────────────

/// How long the nudge waits before it starts: the bubble's own entrance.
///
/// A card that moved while the thing explaining it was still fading in would be
/// the demonstration arriving before its caption.
const NUDGE_LEAD: Duration = POPUP_ENTER;

/// The whole of the nudge, from the bubble landing to the content being home:
/// out on the slow span, standing for the slow span, back on the slow span.
///
/// Three legs of one archived number rather than a shape of its own. A hold in
/// the middle is what makes the eye read *a row moved* rather than *something
/// flickered*, and taking that hold from the same constant as the travel is
/// what keeps this surface out of the motion register.
const NUDGE_SPAN: Duration = MOTION_SLOW;

/// When the nudge is finished and the card owes no more frames.
const NUDGE_END: Duration =
    Duration::from_millis(NUDGE_LEAD.as_millis() as u64 + 3 * (NUDGE_SPAN.as_millis() as u64));

// ── what a hint is ─────────────────────────────────────────────────────────

/// What one turn of [`CardHintHost`] did to the glass.
///
/// Three answers rather than a `bool`, because the caller has to tell two of
/// them apart and a `bool` cannot: **`Raised` is the one frame on which the
/// offer is spent**, and it is the caller — which owns the settings file — that
/// writes that down. A host that wrote it itself would be a state machine
/// reaching into a store, and a caller that inferred it from "the bubble is up
/// now and was not before" would be reading the same fact out of two places.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CardHint {
    /// Nothing on the glass moved.
    Unchanged,
    /// The bubble went up on this turn, and the offer is now spent.
    Raised,
    /// The bubble came down — its four seconds ran out, or the column did.
    Lowered,
}

/// The singleton: whether the bubble is up, and since when.
///
/// One field, and that is the measure of how much of this surface is somebody
/// else's: the entrance and the exit belong to [`crate::arrival`], the offer
/// belongs to `settings.json`, and which card is staged belongs to the window.
/// What is left here is an instant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CardHintHost {
    /// When the bubble was raised — the epoch both the dwell and the nudge are
    /// measured from.
    shown: Option<Instant>,
}

impl CardHintHost {
    /// **Note where the window is, and raise the bubble on the frame Cards
    /// first appears.**
    ///
    /// `in_cards` is whether the column is drawing *right now*, and `offered` is
    /// whether the reader is still owed the sentence. Both are parameters and
    /// neither is a field, for [`crate::keyhint::KeyHintHost::observe`]'s own
    /// stated reason: they are facts about the window and the file, and a host
    /// that remembered either would be remembering it from the wrong moment.
    ///
    /// **Every door into Cards is covered by there being no door here.** The
    /// mode is reached by a chord, by a settings row, and by a window opening
    /// straight into it because that is what the file said — three call sites,
    /// and a hook on each would be three chances for the fourth to be missed.
    /// Asked once a frame against the posture the column is actually drawn from,
    /// this cannot be reached without being told.
    ///
    /// Three sentences:
    ///
    /// 1. **Leaving Cards takes the bubble with it.** It is bitten onto a card,
    ///    and a card that is no longer on the glass is not something a tail can
    ///    point at. The dwell is *not* resumed on the way back in — the offer
    ///    was spent on the frame it was raised, which is the whole of "once".
    /// 2. **A bubble that is up stays up.** Its clock is [`Self::expire`]'s.
    /// 3. **An offer that is not owed raises nothing**, which is the state every
    ///    reader is in from their second visit onwards and the state this
    ///    surface costs nothing at all in.
    pub fn observe(&mut self, in_cards: bool, offered: bool, now: Instant) -> CardHint {
        if !in_cards {
            return match self.shown.take() {
                Some(_) => CardHint::Lowered,
                None => CardHint::Unchanged,
            };
        }
        if self.shown.is_some() || !offered {
            return CardHint::Unchanged;
        }
        self.shown = Some(now);
        CardHint::Raised
    }

    /// Take the bubble down once its four seconds are up.
    ///
    /// Separate from [`Self::observe`] because it answers a different question —
    /// that one is driven by the window changing shape, this one by a clock — and
    /// folding them would mean a window that is perfectly still never noticing
    /// that the dwell had ended.
    pub fn expire(&mut self, now: Instant) -> CardHint {
        let Some(shown) = self.shown else {
            return CardHint::Unchanged;
        };
        if now.saturating_duration_since(shown) < TOAST_LIFE_QUIET {
            return CardHint::Unchanged;
        }
        self.shown = None;
        CardHint::Lowered
    }

    /// Whether the bubble is on the glass this frame.
    #[must_use]
    pub fn showing(&self) -> bool {
        self.shown.is_some()
    }

    /// **When this window next has hint work**, or `None` for the window of any
    /// reader who has already been shown it.
    ///
    /// Two answers and the difference between them is the whole of what reduced
    /// motion means here:
    ///
    /// * **[`Motion::Reduced`] owes exactly one instant** — the end of the
    ///   dwell. Not a frame cadence, not a fade, not a nudge: the card appears
    ///   whole on one frame, stands for four seconds costing the loop nothing,
    ///   and is gone on one frame. A reader with the preference set gets a
    ///   window that sleeps through the entire life of this surface bar its two
    ///   ends.
    /// * **[`Motion::Full`] owes frames while the nudge is running**, and the
    ///   dwell's end after that. The entrance and the exit are not asked about
    ///   here at all — they are the passage register's, and the loop already
    ///   folds in [`crate::arrival::Passages::moving`].
    #[must_use]
    pub fn deadline(&self, now: Instant, motion: Motion, frame: Duration) -> Option<Instant> {
        let shown = self.shown?;
        let expiry = shown + TOAST_LIFE_QUIET;
        if motion == Motion::Reduced || now >= shown + NUDGE_END {
            return Some(expiry);
        }
        Some((now + frame).min(expiry))
    }

    /// Whether the accompaniment is still moving, and therefore still owes the
    /// card it is on a frame.
    ///
    /// Asked of the *nudge* rather than of the bubble, which is the whole point
    /// of it being a separate question: the bubble stands for four seconds and
    /// the card is home after the first three quarters of one, and a window that
    /// redrew the column for the other three and a bit seconds would be paying
    /// for a picture that had stopped changing.
    #[must_use]
    pub fn nudge_moving(&self, now: Instant, motion: Motion) -> bool {
        if motion == Motion::Reduced {
            return false;
        }
        self.shown
            .is_some_and(|shown| now.saturating_duration_since(shown) < NUDGE_END)
    }

    /// **How far the pointed-at card's content stands off its home this frame,
    /// in mini rows** — `0.0` at rest, `1.0` one row down.
    ///
    /// *Down*, because wheel-up is what the gesture's positive notch is and
    /// wheel-up lifts the window: the rows a reader would uncover come in from
    /// the top, so the rows already there move away from it. A nudge that went
    /// the other way would be a demonstration of the gesture run backwards.
    ///
    /// **One row is the unit the gesture itself moves in**, so this is not a
    /// distance somebody chose — it is `card_skip += 1` drawn without being
    /// done. The caller multiplies it by the seat's own line height, which is
    /// the same expression the rows are laid out at.
    ///
    /// Reduced motion answers `0.0` for the whole of the bubble's life. That is
    /// the accompaniment not running, not a nudge quantised to nothing: there is
    /// no end state here to arrive at early, because the end state *is* home.
    #[must_use]
    pub fn nudge_rows(&self, now: Instant, motion: Motion) -> f32 {
        if motion == Motion::Reduced {
            return 0.0;
        }
        let Some(shown) = self.shown else {
            return 0.0;
        };
        let since = now.saturating_duration_since(shown);
        let Some(elapsed) = since.checked_sub(NUDGE_LEAD) else {
            return 0.0;
        };
        let leg = NUDGE_SPAN.as_secs_f32();
        let elapsed = elapsed.as_secs_f32();
        if elapsed < leg {
            return cubic_bezier(elapsed / leg, EASE);
        }
        if elapsed < 2.0 * leg {
            return 1.0;
        }
        if elapsed < 3.0 * leg {
            return 1.0 - cubic_bezier((elapsed - 2.0 * leg) / leg, EASE);
        }
        0.0
    }
}

// ── where the bubble lands ─────────────────────────────────────────────────

/// Which of the two seats a placed bubble took.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CardHintSeat {
    /// Bitten onto a card, with the tail's box — the ordinary seat.
    OnCard {
        /// The tail's own rectangle, its apex on the left wall.
        tail: [f32; 4],
    },
    /// The window's bottom-right corner, with no tail — the seat a bubble takes
    /// when the card it is about is not on the glass to be pointed at.
    OnFloor,
}

/// A placed bubble. Every rectangle is physical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct CardHintLayout {
    /// `[left, top, right, bottom]` of the whole card.
    pub frame: [f32; 4],
    /// Which seat it took, and the tail if it has one.
    pub seat: CardHintSeat,
    /// The `Alt` cap's box.
    pub cap: [f32; 4],
    /// The `+` between the cap and the wheel.
    pub plus: [f32; 4],
    /// The wheel's own box, at the house's own mark box for a row of marks
    /// beside a row of words.
    pub wheel: [f32; 4],
    /// The verb's row.
    pub say: [f32; 4],
    /// What the verb's row says.
    pub title: String,
}

/// **Place the bubble beside `card_head`, or on the window's floor when there is
/// no card to place it beside.**
///
/// `card_head` is the staged card's head band — the part of a card that is the
/// card rather than the picture inside it — clipped to the column's viewport by
/// the caller, and `None` when that band is not on the glass at all. `measure`
/// is the font's answer to "how wide is this string at this size", handed in for
/// the reason every measured caption in this codebase hands it in: only the
/// thing holding the font can say.
///
/// **The fallback is a ruling and not a guard.** A tail is a claim about which
/// object a sentence is about, and a tail drawn at a card that has scrolled past
/// the clip box would be that claim made about whatever happens to be under it.
/// The alternative to a wrong tail is not silence — the reader is still in Cards
/// and still has not been told — so the sentence keeps its four seconds and
/// gives up only the pointing.
///
/// The same answer is given when the bubble would not *fit* beside the card:
/// there is no half-anchored seat where the tail reaches and the words do not.
///
/// **It never answers "nowhere", which is why it does not answer an `Option`.**
/// [`crate::keyhint::place`]'s `None` is a card with nothing to say — a hold
/// whose table is empty — and this card always has exactly one thing to say. The
/// question it is really being asked is *which seat*, and that is what
/// [`CardHintSeat`] answers.
#[must_use]
pub fn place(
    card_head: Option<[f32; 4]>,
    window: (f32, f32),
    scale: f32,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> CardHintLayout {
    let px = |logical: f32| logical * scale;
    let title = crate::i18n::Text::CardGestureHint.text().to_owned();
    let cap_font = px(CAP_FONT_LOGICAL_PX);
    let name_font = px(KEY_HINT_NAME_FONT_LOGICAL_PX);
    let plus_font = px(CARD_HINT_PLUS_FONT_LOGICAL_PX);
    let cap_height = px(CAP_HEIGHT_LOGICAL_PX);
    let pad_x = px(KEY_HINT_PADDING_X_LOGICAL_PX);
    let pad_y = px(KEY_HINT_PADDING_Y_LOGICAL_PX);
    let border = px(KEY_HINT_BORDER_LOGICAL_PX);
    let inset = px(KEY_HINT_WINDOW_INSET_LOGICAL_PX);
    let chord_gap = px(CARD_HINT_CHORD_GAP_LOGICAL_PX);

    let cap_width = cap_box_width(CARD_HINT_CAP, cap_font, scale, measure).round();
    let plus_width = measure("+", plus_font).ceil();
    // The wheel takes the house's own box for a mark standing beside words,
    // which is the box the optical gate holds every drawing to — see
    // `icons::MarkSlot`. Written as the slot rather than as a number, so a
    // re-tuned slot re-tunes this bubble with every menu row in the product.
    let [wheel_width, wheel_height] = MarkSlot::Menu.mark_box_logical_px(ChromeMark::MouseWheel);
    let (wheel_width, wheel_height) = (px(wheel_width), px(wheel_height));
    let say_width = measure(&title, name_font).ceil();

    let inner_width = cap_width
        + chord_gap
        + plus_width
        + chord_gap
        + wheel_width
        + px(KEY_HINT_CAP_GAP_LOGICAL_PX)
        + say_width;
    let inner_height = cap_height.max(wheel_height);
    let width = (inner_width + 2.0 * (pad_x + border)).round();
    let height = (inner_height + 2.0 * (pad_y + border)).round();

    let tail_width = px(CARD_HINT_TAIL_WIDTH_LOGICAL_PX).round().max(1.0);
    let tail_height = 2.0 * tail_width;
    let radius = px(KEY_HINT_RADIUS_LOGICAL_PX);

    // **Both seats are solved here, and the frame is what the tail is then
    // derived from** — never the other way round. A tail placed against the
    // rectangle the bubble *wanted* is a tail that misses by exactly however far
    // the clamp moved it, which is the small sample's own note about deriving a
    // number from an input that is still being edited.
    let (frame, seat) = match card_head {
        Some(head) if head[2] + tail_width + width + inset <= window.0 => {
            let left = (head[2] + tail_width).round();
            let aim = (head[1] + head[3]) / 2.0;
            let top = (aim - height / 2.0)
                .round()
                .clamp(inset, (window.1 - inset - height).max(inset));
            let frame = [left, top, left + width, top + height];
            // The apex sits on the card's own middle where the frame allows it,
            // and stays inside the frame's straight run — a tail growing out of
            // a rounded corner is a tail growing out of nothing.
            let apex = aim.clamp(
                frame[1] + radius + tail_height / 2.0,
                (frame[3] - radius - tail_height / 2.0).max(frame[1] + tail_height / 2.0),
            );
            let tail_top = (apex - tail_height / 2.0).round();
            (
                frame,
                CardHintSeat::OnCard {
                    tail: [left - tail_width, tail_top, left, tail_top + tail_height],
                },
            )
        }
        _ => {
            let right = (window.0 - inset).round();
            let bottom = (window.1 - inset).round();
            (
                [right - width, bottom - height, right, bottom],
                CardHintSeat::OnFloor,
            )
        }
    };

    let inner_left = frame[0] + border + pad_x;
    let middle = (frame[1] + frame[3]) / 2.0;
    let row = |box_height: f32, left: f32, box_width: f32| -> [f32; 4] {
        let top = (middle - box_height / 2.0).round();
        [left, top, left + box_width, top + box_height]
    };
    let cap = row(cap_height, inner_left, cap_width);
    let plus = row(cap_height, cap[2] + chord_gap, plus_width);
    let wheel = row(wheel_height, plus[2] + chord_gap, wheel_width);
    let say = row(
        cap_height,
        wheel[2] + px(KEY_HINT_CAP_GAP_LOGICAL_PX),
        say_width,
    );

    CardHintLayout {
        frame,
        seat,
        cap,
        plus,
        wheel,
        say,
        title,
    }
}

// ── the paint ──────────────────────────────────────────────────────────────

/// Paint the bubble — **one layer**, so the passage register carries one fade
/// over the whole of it, tail included.
#[must_use]
pub fn build(layout: &CardHintLayout, palette: &ChromePalette, scale: f32) -> Vec<OverlayLayer> {
    let px = |logical: f32| logical * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let border = px(KEY_HINT_BORDER_LOGICAL_PX);
    let mut quads: Vec<OverlayQuad> = Vec::new();
    push_float_window(
        &mut quads,
        layout.frame,
        px(KEY_HINT_RADIUS_LOGICAL_PX),
        border,
        px(bt_render::FLOAT_WINDOW_SHADOW_LOGICAL_PX),
        palette.menu_surface,
        palette.menu_shadow,
        alpha(palette.menu_shadow_inner_alpha),
        alpha(palette.menu_shadow_outer_alpha),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
    );
    let mut layer = OverlayLayer {
        quads,
        ..OverlayLayer::default()
    };

    // **The tail, struck the way a cap is struck**: the hairline's triangle
    // first, then the face's a border further along the axis it points down.
    // Shifted rather than inset, and that is what leaves the *base* unstruck —
    // the face's base lands a border inside the bubble, over the bubble's own
    // left edge, so the join has no line drawn across it. The two slanted edges
    // keep their hairline, at `border × sin θ` of it, which at one physical
    // pixel is the same one pixel every other edge in this card wears.
    if let CardHintSeat::OnCard { tail } = layout.seat {
        layer.sprites.push(ChromeSprite::new(
            ChromeMark::HintTail,
            tail,
            palette.menu_border,
        ));
        layer.sprites.push(ChromeSprite::new(
            ChromeMark::HintTail,
            [tail[0] + border, tail[1], tail[2] + border, tail[3]],
            palette.menu_surface,
        ));
    }

    // **The cap is drawn held down**, which is the one thing a picture of a
    // chord can say that a list of keys cannot: this key is not the next thing
    // you press, it is the thing you are already holding while you do something
    // else with your other hand. The wash is `--active` over `--menu` — the
    // layer `:active` already wears everywhere in this product — rather than a
    // fifth colour invented for one card.
    push_cap(
        &mut layer,
        layout.cap,
        CARD_HINT_CAP,
        palette.peek_leaf_focus_fill,
        palette.menu_item_text_selected,
        scale,
        border,
        *palette,
    );
    layer.labels.push(ChromeLabel {
        mono: false,
        text: "+".to_owned(),
        rect: layout.plus,
        font_size_px: px(CARD_HINT_PLUS_FONT_LOGICAL_PX),
        color: palette.menu_item_hint_text,
        align_right: false,
        align_center: true,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: Some(layout.frame),
    });
    layer.sprites.push(ChromeSprite::new(
        ActionIcon::MouseWheel.mark(),
        layout.wheel,
        palette.menu_item_text_selected,
    ));
    layer.labels.push(ChromeLabel {
        mono: false,
        text: layout.title.clone(),
        rect: layout.say,
        font_size_px: px(KEY_HINT_NAME_FONT_LOGICAL_PX),
        color: palette.menu_item_text,
        align_right: false,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        tabular_numerals: false,
        clip: Some(layout.frame),
    });
    vec![layer]
}

/// Which way the bubble grew, for [`crate::arrival::Passages::stage`].
///
/// Away from the card it is bitten onto, which is [`Travel::away_from`]'s own
/// job — and away from the window's own corner for the floor seat, where there
/// is no anchor and the corner is the anchor.
#[must_use]
pub fn travel(layout: &CardHintLayout) -> Travel {
    match layout.seat {
        CardHintSeat::OnCard { tail } => Travel::away_from(tail, layout.frame),
        CardHintSeat::OnFloor => Travel::Up,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALE: f32 = 1.0;
    const WINDOW: (f32, f32) = (1200.0, 800.0);
    /// A card's head in the column: 280 logical wide, so its right edge is 280.
    const HEAD: [f32; 4] = [22.0, 100.0, 280.0, 128.0];

    /// Every character ten wide at any size, so widths are countable and a
    /// layout can be reasoned about on paper.
    fn ten_per_char(run: &str, _size: f32) -> f32 {
        run.chars().count() as f32 * 10.0
    }

    fn placed(card: Option<[f32; 4]>) -> CardHintLayout {
        place(card, WINDOW, SCALE, &mut ten_per_char)
    }

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    /// RED — **the bubble is offered once, and being shown is what spends it.**
    ///
    /// The three sentences of `observe` in the order a reader meets them: a
    /// first entry raises it, a second entry with the offer spent raises
    /// nothing, and the round trip out of Cards and back in does not resurrect
    /// it.
    ///
    /// MUTATION: raise it whenever `in_cards` turns true and the second entry
    /// greets the reader again; keep the dwell across a departure and a reader
    /// who leaves Cards at three seconds and comes back gets the last second of
    /// a card they already read.
    #[test]
    fn a_reader_is_told_once_and_the_telling_is_what_spends_the_offer() {
        let start = Instant::now();
        let mut host = CardHintHost::default();

        // Not in Cards: nothing is owed and nothing happens, however often it
        // is asked.
        assert_eq!(host.observe(false, true, start), CardHint::Unchanged);
        assert!(!host.showing());

        // The first entry.
        assert_eq!(host.observe(true, true, start), CardHint::Raised);
        assert!(host.showing());
        // And the same frame asked twice does not raise a second one.
        assert_eq!(host.observe(true, true, start), CardHint::Unchanged);

        // The caller has written the spend down, so the offer is now false.
        // Leaving takes the card with it.
        assert_eq!(
            host.observe(false, false, at(start, 500)),
            CardHint::Lowered
        );
        assert!(!host.showing());

        // And every entry after that is free.
        assert_eq!(
            host.observe(true, false, at(start, 600)),
            CardHint::Unchanged
        );
        assert!(!host.showing());
        assert_eq!(host.deadline(at(start, 600), Motion::Full, FRAME), None);
    }

    const FRAME: Duration = Duration::from_millis(16);

    /// RED — **under reduced motion this surface asks the loop for exactly one
    /// wake-up: the end of its own dwell.**
    ///
    /// Not a frame cadence anywhere in its life, and the nudge flat at home for
    /// the whole of it. This is the red line the small sample's third candidate
    /// failed: what the preference takes away here is a flourish, and the card
    /// that is left stands for the same four seconds saying the same words.
    ///
    /// MUTATION: return `now + frame` under `Reduced` and a window with the
    /// preference set spins at frame rate for four seconds drawing a picture
    /// that never changes; let the nudge run under `Reduced` and the card the
    /// reader asked to hold still moves.
    #[test]
    fn reduced_motion_owes_one_instant_and_the_card_it_points_at_holds_still() {
        let start = Instant::now();
        let mut host = CardHintHost::default();
        assert_eq!(host.observe(true, true, start), CardHint::Raised);

        let expiry = start + TOAST_LIFE_QUIET;
        for step in [0_u64, 1, 140, 300, 500, 740, 2_000, 3_999] {
            let now = at(start, step);
            assert_eq!(
                host.deadline(now, Motion::Reduced, FRAME),
                Some(expiry),
                "reduced motion owes the end of the dwell and nothing else, at {step}ms",
            );
            assert_eq!(
                host.nudge_rows(now, Motion::Reduced),
                0.0,
                "the accompaniment does not run at {step}ms",
            );
        }

        // And it still ends on time: the dwell is a preference-independent fact.
        assert_eq!(host.expire(at(start, 3_999)), CardHint::Unchanged);
        assert_eq!(host.expire(expiry), CardHint::Lowered);
        assert!(!host.showing());
    }

    /// RED — **the nudge is one row out and one row back, and it is finished
    /// long before the bubble is.**
    ///
    /// The shape in full: home while the bubble is still arriving, out over the
    /// slow span, standing for the slow span, home again over the slow span, and
    /// home for the rest of the four seconds. The frames it owes stop when the
    /// movement does.
    ///
    /// MUTATION: drop the lead and the card moves under a caption that is still
    /// fading in; drop the hold in the middle and the row flicks rather than
    /// travels; leave the deadline at frame rate past the end and the window
    /// spends three seconds a visit redrawing a card at rest.
    #[test]
    fn the_card_nudges_one_row_out_and_back_and_then_stops_costing_frames() {
        let start = Instant::now();
        let mut host = CardHintHost::default();
        assert_eq!(host.observe(true, true, start), CardHint::Raised);
        let full = |ms: u64| host.nudge_rows(at(start, ms), Motion::Full);

        // The lead: the bubble's own entrance, and the card has not moved.
        assert_eq!(full(0), 0.0);
        assert_eq!(full(139), 0.0);
        // Out, over the slow span, and never past one row.
        assert!(full(240) > 0.0 && full(240) < 1.0);
        assert!(full(240) < full(300), "still travelling outward");
        // Standing at one row for the whole middle leg.
        assert_eq!(full(340), 1.0);
        assert_eq!(full(539), 1.0);
        // Back home over the third.
        assert!(full(640) > 0.0 && full(640) < 1.0);
        assert!(full(640) > full(700), "coming back");
        assert_eq!(full(740), 0.0);
        // And home for the rest of the bubble's life.
        assert_eq!(full(1_500), 0.0);
        assert_eq!(full(3_900), 0.0);

        // The frames follow the movement, not the dwell.
        let moving = host
            .deadline(at(start, 300), Motion::Full, FRAME)
            .expect("a moving card owes a frame");
        assert_eq!(moving, at(start, 316));
        assert_eq!(
            host.deadline(at(start, 1_000), Motion::Full, FRAME),
            Some(start + TOAST_LIFE_QUIET),
            "once the nudge is home the only thing left is the end of the dwell",
        );
    }

    /// RED — **one rule, two seats: the tail bites the card, and a card that is
    /// not there gets the window's own corner instead.**
    ///
    /// The half that would rot is the second: a bubble that simply refused to
    /// draw when the staged card had scrolled past the clip box would leave a
    /// reader in Cards, four seconds into their first visit, with nothing on the
    /// glass at all — and the offer already spent.
    ///
    /// MUTATION: answer `None` for the scrolled-out case and the sentence is
    /// lost for good; keep the tail on the floor seat and it points at the
    /// window's own edge.
    #[test]
    fn the_bubble_bites_the_card_and_falls_back_to_the_corner_when_there_is_none() {
        let bitten = placed(Some(HEAD));
        let CardHintSeat::OnCard { tail } = bitten.seat else {
            panic!("a card on the glass is bitten");
        };
        // The tail stands between the card's right edge and the bubble's left.
        assert_eq!(tail[2], bitten.frame[0]);
        assert_eq!(tail[0], HEAD[2]);
        assert_eq!(tail[2] - tail[0], CARD_HINT_TAIL_WIDTH_LOGICAL_PX);
        assert_eq!(
            tail[3] - tail[1],
            2.0 * CARD_HINT_TAIL_WIDTH_LOGICAL_PX,
            "a right-angled apex, which is a 45° square's own geometry",
        );
        // And it aims at the head it is about.
        let aim = (HEAD[1] + HEAD[3]) / 2.0;
        assert!(
            ((tail[1] + tail[3]) / 2.0 - aim).abs() <= 1.0,
            "the tail points at the middle of the card's head",
        );
        // The whole card is inside the frame, which is inside the window.
        assert!(bitten.frame[2] <= WINDOW.0);
        assert!(bitten.say[2] <= bitten.frame[2]);

        // The card scrolled away: same words, same box, the window's corner.
        let floored = placed(None);
        assert_eq!(floored.seat, CardHintSeat::OnFloor);
        assert_eq!(floored.title, bitten.title);
        assert_eq!(
            [
                floored.frame[2] - floored.frame[0],
                floored.frame[3] - floored.frame[1]
            ],
            [
                bitten.frame[2] - bitten.frame[0],
                bitten.frame[3] - bitten.frame[1]
            ],
            "one box, two seats",
        );
        assert_eq!(
            floored.frame[2],
            WINDOW.0 - KEY_HINT_WINDOW_INSET_LOGICAL_PX
        );
        assert_eq!(
            floored.frame[3],
            WINDOW.1 - KEY_HINT_WINDOW_INSET_LOGICAL_PX
        );

        // A card so far right that the bubble would hang off the window takes
        // the corner too, rather than a seat with the tail on and the words off.
        let squeezed = placed(Some([0.0, 100.0, WINDOW.0 - 40.0, 128.0]));
        assert_eq!(squeezed.seat, CardHintSeat::OnFloor);
    }

    /// RED — **the chord is glyphs and the sentence is the verb, and neither
    /// says the other's half.**
    ///
    /// The de-duplication ruling, as a gate on the strings themselves: the words
    /// this card prints never contain the modifier's name, in either language,
    /// because the cap beside them is already printing it.
    ///
    /// MUTATION: put the chord back into the sentence ("Alt + scroll to scroll a
    /// card") and this names it.
    #[test]
    fn the_words_never_repeat_the_key_that_is_drawn_beside_them() {
        for lang in [crate::i18n::Lang::English, crate::i18n::Lang::Chinese] {
            let said = crate::i18n::Text::CardGestureHint.in_lang(lang);
            assert!(
                !said.contains(CARD_HINT_CAP),
                "{lang:?} says {said:?}, which prints the cap a second time",
            );
            assert!(
                !said.to_lowercase().contains("alt"),
                "{lang:?} says {said:?}",
            );
            assert!(!said.trim().is_empty(), "{lang:?} says nothing");
        }
    }

    /// PIN — the box is the key hint's box, to the pixel.
    ///
    /// Not a copy of its numbers: this file imports the constants, and the pin
    /// is what says out loud that it must go on doing so. A bubble with its own
    /// padding would be a second card shape in a window that has one.
    #[test]
    fn the_bubble_wears_the_key_hints_own_box() {
        let bubble = placed(Some(HEAD));
        let height = bubble.frame[3] - bubble.frame[1];
        assert_eq!(
            height,
            CAP_HEIGHT_LOGICAL_PX
                + 2.0 * (KEY_HINT_PADDING_Y_LOGICAL_PX + KEY_HINT_BORDER_LOGICAL_PX),
            "one row of caps inside the key hint's own padding and hairline",
        );
        // The chord reads left to right with the sentence last, and nothing
        // overlaps.
        assert!(bubble.cap[2] <= bubble.plus[0]);
        assert!(bubble.plus[2] <= bubble.wheel[0]);
        assert!(bubble.wheel[2] <= bubble.say[0]);
        assert!(bubble.frame[0] < bubble.cap[0]);
    }
}
