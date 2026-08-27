//! The card that answers a hand that has stopped on its modifiers.
//!
//! **Why this exists** (user proposal, Claude 定形 2026-08-25, `docs/DESIGN.md`
//! §7.1.5e′). The shortcut table has thirty-nine rows and exactly one door onto
//! them: a page inside Settings that a reader has to already suspect exists.
//! Everything else this window can do announces itself where the hand already is
//! — a menu drops out of the head it belongs to, a tip explains the button under
//! the pointer — and the keyboard was the one instrument with no such offer. A
//! hand that puts `Ctrl` down and then *stops* is a hand that has forgotten
//! something, and stopping is the only signal it gives.
//!
//! **What it must never be.** It never takes a key: the card is raised by a wait
//! and taken down by the first thing that is not one, so the chord a reader was
//! reaching for lands on exactly the row it would have landed on with no card at
//! all. It never covers what a hand is aiming at: it stands on the window's own
//! floor, where nothing in this product is pressed. And it never lists a verb
//! that would not fire — see [`Shortcuts::hint_lines`](crate::shortcuts::Shortcuts::hint_lines)
//! for the scope filter and [`KeyHintHost::observe`] for the states in which no
//! card is raised at all.
//!
//! Three pieces, deliberately apart, on [`crate::toast`]'s own division:
//!
//! * [`KeyHintHost`] — the state machine and its two clocks. It knows nothing
//!   about the table, the window, or where anything is drawn.
//! * [`place`] — where the card lands and how its lines are dealt into columns.
//!   Geometry only.
//! * [`build`] — the paint. One layer, so it carries one fade.

use std::time::{Duration, Instant};

use bt_render::{ChromeLabel, ChromeLabelWeight, ChromePalette, OverlayQuad};
use winit::keyboard::ModifiersState;

use crate::marks::OverlayLayer;
use crate::settings::push_float_window;
use crate::shortcuts::HintLine;
use crate::{EASE, Motion, cubic_bezier};

// ── the clocks ─────────────────────────────────────────────────────────────

/// How long the modifiers have to be down, alone, before the card is offered.
///
/// **800ms, and the number is the difference between two hands rather than a
/// taste.** A hand that knows the chord it wants has both keys down and the
/// letter after them inside a couple of hundred milliseconds; a hand that has
/// stopped has stopped. Anything under half a second would put a card in front
/// of every deliberate `Ctrl+Shift+N` in the product, and the one rule this
/// surface has is that it is never in the way. It is deliberately more than
/// twice [`crate::tooltip::TOOLTIP_DELAY`]: a tip is summoned by a pointer that
/// is *resting*, which is the ordinary state of a pointer, while this is
/// summoned by fingers that are holding keys down — an expensive posture nobody
/// holds by accident, and one whose cost is what makes the long wait affordable.
pub const KEY_HINT_DELAY: Duration = Duration::from_millis(800);

/// How long the card takes to arrive: the tip's own 90ms.
///
/// [`crate::tooltip::TOOLTIP_FADE`]'s number and its argument — this is the
/// other surface in this window you summon by *not moving*, so once the wait is
/// over it has to feel like it was already there.
///
/// **This is the entrance only, and it is the whole of what this file owns.**
/// The card had no exit at all until the animation block's first slice, whose
/// argument for one is in `arrival.rs`: the card leaves as a *picture*, over
/// [`bt_render::POPUP_EXIT`], through [`crate::arrival::Passages::stage_departure`]
/// — which is why it can leave at all without the state that draws it staying
/// alive one frame past the key that dismissed it. The reason this file once
/// gave for having no exit ("leaving is the half nobody is waiting for") is the
/// reason that half is *fast* and does not travel, not a reason for it to be a
/// hard cut; ninety milliseconds of ink is not standing over anything.
pub const KEY_HINT_FADE: Duration = bt_render::MOTION_FAST;

// ── the box ────────────────────────────────────────────────────────────────

/// How far the card stands off the window's floor.
///
/// [`crate::toast::TOAST_WINDOW_INSET_LOGICAL_PX`]'s own sixteen: this card is
/// anchored to the window and not to any surface inside it, and sixteen is the
/// distance this product already uses to say "floating clear of everything".
pub const KEY_HINT_WINDOW_INSET_LOGICAL_PX: f32 = 16.0;
/// `border-radius: 8px` — the float window's round, as every card here wears.
pub const KEY_HINT_RADIUS_LOGICAL_PX: f32 = 8.0;
/// `border: 1px solid var(--border)`, as everything through
/// [`push_float_window`] wears.
pub const KEY_HINT_BORDER_LOGICAL_PX: f32 = 1.0;
/// The `12px` of `padding: 10px 12px` — the toast's own box.
pub const KEY_HINT_PADDING_X_LOGICAL_PX: f32 = 12.0;
/// The `10px` of the same.
pub const KEY_HINT_PADDING_Y_LOGICAL_PX: f32 = 10.0;
/// The row a single line of the card is laid out in.
///
/// Four more than a cap is tall ([`crate::settings::CAP_HEIGHT_LOGICAL_PX`]), so
/// that two caps stacked have two pixels of air between them and the column
/// reads as a list rather than as a keyboard.
pub const KEY_HINT_ROW_HEIGHT_LOGICAL_PX: f32 = 24.0;
/// The gap between a cap and the name of what it does.
pub const KEY_HINT_CAP_GAP_LOGICAL_PX: f32 = 10.0;
/// The gap between two columns of lines.
///
/// Wider than the gap inside a line, and that is the whole of what makes the
/// columns readable: the eye has to be able to tell "this name belongs to the
/// cap on its left" from "this cap starts the next column".
pub const KEY_HINT_COLUMN_GAP_LOGICAL_PX: f32 = 24.0;
/// The gap between the head — the modifiers this card is answering — and the
/// first line under it.
pub const KEY_HINT_HEAD_GAP_LOGICAL_PX: f32 = 8.0;
/// `font-size: 12px` — a line's name, the toast body's own size.
pub const KEY_HINT_NAME_FONT_LOGICAL_PX: f32 = 12.0;

/// The most of the window's height the card's own lines may take.
///
/// Not a row count, because a row count is a guess about a window: the same
/// eighteen lines are a comfortable two columns on a full-screen window and a
/// wall on a half-height one. Rows per column are derived from this and the row
/// height, so the card is always about half the window at most however tall the
/// window is, and the lines that do not fit are dealt into the next column
/// rather than dropped.
pub const KEY_HINT_MAX_BODY_FRACTION: f32 = 0.45;

// ── what a hint is ─────────────────────────────────────────────────────────

/// The singleton: which hold is settling, which is showing, and whether the hand
/// has already spent this one.
///
/// Modelled on [`crate::tooltip::TooltipHost`], which solves the same shape — arm
/// a clock on a subject, do not restart it while the subject holds still, promote
/// when it elapses — with one field it does not need. `spent` is the whole of
/// "the hint never eats a key": a press that is not a modifier takes the card
/// down *and closes the offer*, so a reader who has just pressed `Ctrl+Shift+N`
/// and is still holding `Ctrl+Shift` is not asked a question they have already
/// answered. The offer opens again when the last modifier comes up, which is the
/// only event that says the hand has finished.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyHintHost {
    /// The hold under the fingers, and when its card comes due.
    settling: Option<(ModifiersState, Instant)>,
    /// The hold whose card is on the glass, and when it appeared — the fade's
    /// own epoch.
    showing: Option<(ModifiersState, Instant)>,
    /// Whether a key that is not a modifier has been pressed since the hand was
    /// last empty.
    spent: bool,
}

impl KeyHintHost {
    /// Note what the modifiers are now. Returns whether anything visible changed.
    ///
    /// Driven from `WindowEvent::ModifiersChanged` and from nothing else: winit
    /// reports the whole state on every press and release of a modifier, so this
    /// window never has to keep a second opinion about which keys are down
    /// (§7.1.5e′, and the standing rule that the shortcut machinery reads
    /// [`ModifiersState`] rather than hooking the keyboard).
    ///
    /// `offered` is whether a card may be raised at all this instant — the
    /// setting, and the states in which a chord would not fire anyway. It is a
    /// parameter and not a field because it is a fact about the *window* that can
    /// change while a hold is in progress, and a host that remembered it would be
    /// remembering it from the wrong moment.
    ///
    /// Four sentences, and the third is the one worth reading twice:
    ///
    /// 1. **No modifiers is the end of the hold.** The card goes, the clock goes,
    ///    and the offer opens again — this is the only event that clears `spent`.
    /// 2. **A hand that has already pressed something is not asking.** See
    ///    `spent`.
    /// 3. **A hold that changes while the card is up changes the card, not the
    ///    clock.** Adding `Shift` to a `Ctrl` that is already answered is the
    ///    reader narrowing the same question, and making them wait another
    ///    800ms for the answer would be the card punishing them for reading it.
    ///    The fade's epoch is kept, so the card does not blink.
    /// 4. **A hold that changes before the card is up starts the clock over.**
    ///    They are still assembling the chord, and the wait is exactly the
    ///    measure of "they have stopped assembling it".
    pub fn observe(&mut self, modifiers: ModifiersState, offered: bool, now: Instant) -> bool {
        if modifiers.is_empty() {
            self.spent = false;
            self.settling = None;
            return self.showing.take().is_some();
        }
        if !offered {
            self.settling = None;
            return self.showing.take().is_some();
        }
        if self.spent {
            return false;
        }
        if let Some((shown, at)) = self.showing {
            if shown == modifiers {
                return false;
            }
            self.showing = Some((modifiers, at));
            self.settling = None;
            return true;
        }
        if self.settling.map(|(held, _)| held) != Some(modifiers) {
            self.settling = Some((modifiers, now + KEY_HINT_DELAY));
        }
        false
    }

    /// Promote a hold whose wait has elapsed. Returns whether it did.
    pub fn activate_if_due(&mut self, now: Instant) -> bool {
        let Some((held, due)) = self.settling else {
            return false;
        };
        if now < due {
            return false;
        }
        self.settling = None;
        self.showing = Some((held, now));
        true
    }

    /// **A key that is not a modifier was pressed**, with `modifiers` down at the
    /// time. Returns whether a card was taken down.
    ///
    /// The press itself is untouched — this is called from the top of
    /// `keyboard_input` and answers nothing, so the chord goes on to the same row
    /// of the same table it would have reached with no card on the glass. That is
    /// the surface's first promise and it is kept structurally: there is no path
    /// from here that can consume an event.
    ///
    /// **`modifiers` is what makes this a *hold* being spent rather than a key
    /// being pressed**, and leaving it out was a real bug: with the offer closed
    /// by every keystroke, ordinary typing followed by a `Ctrl` that went down
    /// before the loop next turned left `spent` standing over a hold nobody had
    /// used, and the card simply never appeared. A press with an empty hand
    /// spends nothing, because there was nothing there to spend — which is the
    /// same sentence [`Self::observe`]'s first arm makes about letting go.
    pub fn spend(&mut self, modifiers: ModifiersState) -> bool {
        self.spent = !modifiers.is_empty();
        self.settling = None;
        self.showing.take().is_some()
    }

    /// Take the card down and disarm the clock without closing the offer — the
    /// window losing focus, a menu opening, the setting being switched off.
    ///
    /// Not [`Self::spend`], and the difference is which hand is being described:
    /// spending is something the reader did with the keys they are holding, and
    /// this is something that happened to the window. A window that came back to
    /// a hand still holding `Ctrl` should answer it again.
    pub fn hide(&mut self) -> bool {
        self.settling = None;
        self.showing.take().is_some()
    }

    /// The hold whose card is on the glass.
    #[must_use]
    pub fn active(&self) -> Option<ModifiersState> {
        self.showing.map(|(held, _)| held)
    }

    /// The next instant this host has something to do: the wait while one is
    /// armed, the next frame of the fade while one is running.
    ///
    /// Handed to the loop's `earliest_deadline`, so a window with a hold settling
    /// wakes exactly when it is due and a window whose hands are empty costs
    /// nothing at all.
    #[must_use]
    pub fn deadline(&self, now: Instant, motion: Motion, frame: Duration) -> Option<Instant> {
        if let Some((_, due)) = self.settling {
            return Some(due);
        }
        self.is_fading(now, motion).then(|| now + frame)
    }

    /// Whether the fade is still running, and therefore still owes frames.
    #[must_use]
    pub fn is_fading(&self, now: Instant, motion: Motion) -> bool {
        if motion == Motion::Reduced {
            return false;
        }
        self.showing
            .is_some_and(|(_, shown)| now.duration_since(shown) < KEY_HINT_FADE)
    }

    /// How solid the card is drawn this frame — `opacity 0 -> 1` over
    /// [`KEY_HINT_FADE`] on the mock-up's own `ease`.
    ///
    /// Reduced motion gets the end state on the first frame, which is the tip's
    /// own answer and its argument: this is the one popup you summon by not
    /// moving, so a fade-in is exactly the unrequested motion the preference is
    /// about.
    #[must_use]
    pub fn opacity(&self, now: Instant, motion: Motion) -> f32 {
        let Some((_, shown)) = self.showing else {
            return 0.0;
        };
        if motion == Motion::Reduced {
            return 1.0;
        }
        let elapsed = now.duration_since(shown).as_secs_f32();
        let progress = (elapsed / KEY_HINT_FADE.as_secs_f32()).clamp(0.0, 1.0);
        cubic_bezier(progress, EASE)
    }
}

// ── where the card lands ───────────────────────────────────────────────────

/// One drawn line: the cap's box and what is printed in it, then the name's row
/// and the name.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyHintPlacedLine {
    pub cap: [f32; 4],
    pub key: String,
    pub name: [f32; 4],
    pub title: String,
}

/// A placed card. Every rectangle is physical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyHintLayout {
    /// `[left, top, right, bottom]` of the whole card.
    pub frame: [f32; 4],
    /// The modifier caps this card is answering, left to right, with their
    /// boxes.
    pub head: Vec<([f32; 4], String)>,
    /// The lines, in the order [`crate::shortcuts::Shortcuts::hint_lines`] gave
    /// them, dealt down each column before starting the next.
    pub lines: Vec<KeyHintPlacedLine>,
    /// The row that says how many lines would not fit, when some did not.
    ///
    /// **A card that has cut something says so**, which is
    /// [`crate::toast::TOAST_MAX_LINES`]'s own rule one surface over. Silently
    /// showing fourteen of sixteen would be this window teaching a reader that
    /// two chords do not exist.
    pub overflow: Option<([f32; 4], String)>,
}

/// Place the card, or answer `None` when there is nothing to place.
///
/// `lines` is what the table said; `held` is the modifier caps for the head.
/// `measure` is the font's answer to "how wide is this string, at this size",
/// handed in for the reason every measured caption in this codebase hands it in:
/// only the thing holding the font can say.
///
/// **An empty list raises no card, and that is a ruling rather than a guard.** A
/// hold that claims nothing in the scope the keyboard is actually in — `Shift`
/// on a terminal with no search open, `Alt` on its own, anything with the
/// Windows key in it — has no answer, and a card reading "nothing here" would be
/// a surface that appears in order to say it should not have.
#[must_use]
pub fn place(
    lines: &[HintLine],
    held: &[String],
    window: (f32, f32),
    scale: f32,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> Option<KeyHintLayout> {
    if lines.is_empty() || held.is_empty() {
        return None;
    }
    let px = |logical: f32| logical * scale;
    let cap_font = px(crate::settings::CAP_FONT_LOGICAL_PX);
    let name_font = px(KEY_HINT_NAME_FONT_LOGICAL_PX);
    let cap_height = px(crate::settings::CAP_HEIGHT_LOGICAL_PX);
    let row_height = px(KEY_HINT_ROW_HEIGHT_LOGICAL_PX);
    let pad_x = px(KEY_HINT_PADDING_X_LOGICAL_PX);
    let pad_y = px(KEY_HINT_PADDING_Y_LOGICAL_PX);
    let border = px(KEY_HINT_BORDER_LOGICAL_PX);

    // **One cap width for the whole card**, taken from the widest key in it. Caps
    // that each took their own width would leave the names on a ragged edge, and
    // a column of names that does not start at one `x` is a column the eye has to
    // re-find on every row.
    let cap_width = lines
        .iter()
        .map(|line| cap_box_width(&line.key, cap_font, scale, measure))
        .fold(px(crate::settings::CAP_MIN_WIDTH_LOGICAL_PX), f32::max)
        .round();
    let name_width = lines
        .iter()
        .map(|line| measure(line.title, name_font))
        .fold(0.0_f32, f32::max)
        .ceil();
    let column_width = cap_width + px(KEY_HINT_CAP_GAP_LOGICAL_PX) + name_width;
    let column_gap = px(KEY_HINT_COLUMN_GAP_LOGICAL_PX);

    // How many lines a column may hold, from the window's height rather than from
    // a row count — see [`KEY_HINT_MAX_BODY_FRACTION`].
    let per_column = ((window.1 * KEY_HINT_MAX_BODY_FRACTION) / row_height).floor();
    let per_column = (per_column as usize).max(1);
    // And how many columns the window's width can take. The card never grows past
    // the window: a hint that hung off the edge would be hiding the very keys it
    // was listing.
    let room = window.0 - 2.0 * px(KEY_HINT_WINDOW_INSET_LOGICAL_PX) - 2.0 * (pad_x + border);
    let max_columns = ((room + column_gap) / (column_width + column_gap)).floor();
    let max_columns = (max_columns as usize).max(1);

    let wanted = lines.len().div_ceil(per_column);
    let columns = wanted.min(max_columns);
    let capacity = columns * per_column;
    let (shown, cut) = if capacity < lines.len() {
        // The last cell goes to the count, so the card is exactly as tall as it
        // claimed to be and still says what it left out.
        (capacity - 1, lines.len() - (capacity - 1))
    } else {
        (lines.len(), 0)
    };
    let rows_used = shown + usize::from(cut > 0);
    let rows_in_tallest = rows_used.div_ceil(columns).max(1);

    let head_height = cap_height;
    let body_top_gap = px(KEY_HINT_HEAD_GAP_LOGICAL_PX);
    let body_height = rows_in_tallest as f32 * row_height;
    let inner_height = head_height + body_top_gap + body_height;
    // **The head is a floor under the card's width, not a passenger.** A single
    // narrow column — `Tab` against `Next tab` — is narrower than the two caps
    // `Ctrl Shift` takes, and a card sized from its body alone would print the
    // second of them past its own right edge.
    let head_width = held
        .iter()
        .map(|cap| cap_box_width(cap, cap_font, scale, measure).round())
        .sum::<f32>()
        + (held.len().saturating_sub(1)) as f32 * px(crate::settings::CAP_GAP_LOGICAL_PX);
    let inner_width = (columns as f32 * column_width
        + (columns.saturating_sub(1)) as f32 * column_gap)
        .max(head_width);

    let width = (inner_width + 2.0 * (pad_x + border)).round();
    let height = (inner_height + 2.0 * (pad_y + border)).round();
    let left = ((window.0 - width) / 2.0).round().max(0.0);
    let bottom = (window.1 - px(KEY_HINT_WINDOW_INSET_LOGICAL_PX)).round();
    let top = bottom - height;
    let frame = [left, top, left + width, bottom];

    let inner_left = left + border + pad_x;
    let inner_top = top + border + pad_y;

    // The head: the modifiers as caps, left to right, in the order
    // `modifier_caps` writes them.
    let mut head = Vec::new();
    let mut at = inner_left;
    for cap in held {
        let cap_w = cap_box_width(cap, cap_font, scale, measure).round();
        head.push((
            [at, inner_top, at + cap_w, inner_top + cap_height],
            cap.clone(),
        ));
        at += cap_w + px(crate::settings::CAP_GAP_LOGICAL_PX);
    }

    let body_top = inner_top + head_height + body_top_gap;
    let cell = |index: usize| -> ([f32; 4], [f32; 4]) {
        let column = index / rows_in_tallest;
        let row = index % rows_in_tallest;
        let column_left = inner_left + column as f32 * (column_width + column_gap);
        let row_top = body_top + row as f32 * row_height;
        let middle = row_top + row_height / 2.0;
        let cap = [
            column_left,
            (middle - cap_height / 2.0).round(),
            column_left + cap_width,
            (middle + cap_height / 2.0).round(),
        ];
        let name = [
            column_left + cap_width + px(KEY_HINT_CAP_GAP_LOGICAL_PX),
            row_top,
            column_left + column_width,
            row_top + row_height,
        ];
        (cap, name)
    };

    let placed = lines
        .iter()
        .take(shown)
        .enumerate()
        .map(|(index, line)| {
            let (cap, name) = cell(index);
            KeyHintPlacedLine {
                cap,
                key: line.key.clone(),
                name,
                title: line.title.to_owned(),
            }
        })
        .collect();
    let overflow = (cut > 0).then(|| {
        let (cap, name) = cell(shown);
        (
            [cap[0], name[1], name[2], name[3]],
            crate::i18n::key_hint_more(cut),
        )
    });

    Some(KeyHintLayout {
        frame,
        head,
        lines: placed,
        overflow,
    })
}

/// How wide a key cap's box is for one word — the settings page's own
/// arithmetic, so a cap on this card and a cap on that page are the same object.
fn cap_box_width(
    text: &str,
    font_size_px: f32,
    scale: f32,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> f32 {
    (measure(text, font_size_px) + 2.0 * scale * crate::settings::CAP_PADDING_X_LOGICAL_PX)
        .max(scale * crate::settings::CAP_MIN_WIDTH_LOGICAL_PX)
}

// ── the paint ──────────────────────────────────────────────────────────────

/// Paint the card — **one layer**, so it carries one fade.
#[must_use]
pub fn build(
    layout: &KeyHintLayout,
    palette: &ChromePalette,
    scale: f32,
    opacity: f32,
) -> Vec<OverlayLayer> {
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
        opacity,
        ..OverlayLayer::default()
    };

    // The head's caps are struck in the muted ink and the lines' caps in the
    // full one, which is the whole of what the two rows are saying: these are
    // already down, and one of these is what you press next.
    for (rect, cap) in &layout.head {
        crate::settings::push_cap(
            &mut layer,
            *rect,
            cap,
            palette.menu_surface,
            palette.menu_item_hint_text,
            scale,
            border,
            *palette,
        );
    }
    for line in &layout.lines {
        crate::settings::push_cap(
            &mut layer,
            line.cap,
            &line.key,
            palette.menu_item_hover,
            palette.menu_item_text_selected,
            scale,
            border,
            *palette,
        );
        layer.labels.push(ChromeLabel {
            mono: false,
            text: line.title.clone(),
            rect: line.name,
            font_size_px: px(KEY_HINT_NAME_FONT_LOGICAL_PX),
            color: palette.menu_item_text,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: Some(layout.frame),
        });
    }
    if let Some((rect, text)) = &layout.overflow {
        layer.labels.push(ChromeLabel {
            mono: false,
            text: text.clone(),
            rect: *rect,
            font_size_px: px(KEY_HINT_NAME_FONT_LOGICAL_PX),
            color: palette.menu_item_hint_text,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: Some(layout.frame),
        });
    }
    vec![layer]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALE: f32 = 1.0;
    const WINDOW: (f32, f32) = (1200.0, 800.0);

    /// A measure where every character is ten wide at any size, so widths are
    /// countable and a wrap can be reasoned about on paper.
    fn ten_per_char(run: &str, _size: f32) -> f32 {
        run.chars().count() as f32 * 10.0
    }

    fn lines(count: usize) -> Vec<HintLine> {
        (0..count)
            .map(|index| HintLine {
                key: index.to_string(),
                title: "Do a thing",
            })
            .collect()
    }

    const CTRL_SHIFT: ModifiersState = ModifiersState::CONTROL.union(ModifiersState::SHIFT);

    /// PIN §7.1.5e′ — **the card is not due until the whole wait has passed**,
    /// and a hold that has not been interrupted does not restart it.
    ///
    /// Mutation: promote in `activate_if_due` without comparing `now` to `due`.
    #[test]
    fn a_hold_owes_the_whole_delay_before_a_card_is_offered() {
        let start = Instant::now();
        let mut host = KeyHintHost::default();
        assert!(!host.observe(ModifiersState::CONTROL, true, start));
        assert!(!host.activate_if_due(start + KEY_HINT_DELAY - Duration::from_millis(1)));
        assert_eq!(host.active(), None);
        assert!(host.activate_if_due(start + KEY_HINT_DELAY));
        assert_eq!(host.active(), Some(ModifiersState::CONTROL));
    }

    /// PIN §7.1.5e′ — **the same hold reported again does not restart the
    /// clock.** winit re-reports the state on every modifier event, and a host
    /// that re-armed on a repeat would be a card that never appears while two
    /// keys are being held down one after the other.
    ///
    /// Mutation: drop the `!= Some(modifiers)` guard in `observe`.
    #[test]
    fn a_hold_reported_twice_is_still_one_wait() {
        let start = Instant::now();
        let mut host = KeyHintHost::default();
        host.observe(ModifiersState::CONTROL, true, start);
        host.observe(
            ModifiersState::CONTROL,
            true,
            start + Duration::from_millis(400),
        );
        assert!(host.activate_if_due(start + KEY_HINT_DELAY));
    }

    /// PIN §7.1.5e′ — **a real key takes the card down and closes the offer
    /// until the hand is empty.**
    ///
    /// Mutation: make `spend` clear `spent` instead of setting it.
    #[test]
    fn a_real_key_takes_the_card_down_and_keeps_it_down_until_the_hand_lets_go() {
        let start = Instant::now();
        let mut host = KeyHintHost::default();
        host.observe(CTRL_SHIFT, true, start);
        assert!(host.activate_if_due(start + KEY_HINT_DELAY));
        assert!(host.spend(CTRL_SHIFT));
        assert_eq!(host.active(), None);
        // Still holding both, and now letting one go: no second card.
        assert!(!host.observe(
            ModifiersState::CONTROL,
            true,
            start + Duration::from_secs(2)
        ));
        assert!(!host.activate_if_due(start + Duration::from_secs(9)));
        assert_eq!(host.active(), None);
        // The hand is empty: the offer opens again.
        assert!(!host.observe(
            ModifiersState::empty(),
            true,
            start + Duration::from_secs(3)
        ));
        assert!(!host.observe(
            ModifiersState::CONTROL,
            true,
            start + Duration::from_secs(3)
        ));
        assert!(host.activate_if_due(start + Duration::from_secs(3) + KEY_HINT_DELAY));
    }

    /// PIN §7.1.5e′ — **typing with an empty hand spends nothing**, so a `Ctrl`
    /// pressed straight after a word is still answered.
    ///
    /// Found by reading rather than on the machine, and it is a real ordering:
    /// `keyboard_input` spends at its top and `note_key_hint` clears on the next
    /// turn, so a modifier that goes down inside the same burst of events as the
    /// last letter would meet a hold that had been marked spent before it
    /// existed.
    ///
    /// Mutation: `self.spent = true` in `spend`.
    #[test]
    fn typing_with_no_modifiers_down_leaves_the_next_hold_answerable() {
        let start = Instant::now();
        let mut host = KeyHintHost::default();
        assert!(!host.spend(ModifiersState::empty()));
        // No turn of the loop in between — the `Ctrl` arrives in the same burst.
        assert!(!host.observe(ModifiersState::CONTROL, true, start));
        assert!(host.activate_if_due(start + KEY_HINT_DELAY));
        assert_eq!(host.active(), Some(ModifiersState::CONTROL));
    }

    /// PIN §7.1.5e′ — **letting every modifier go takes the card down at once.**
    ///
    /// Mutation: return `false` from the empty-modifiers arm without taking
    /// `showing`.
    #[test]
    fn letting_the_modifiers_go_takes_the_card_down() {
        let start = Instant::now();
        let mut host = KeyHintHost::default();
        host.observe(ModifiersState::CONTROL, true, start);
        host.activate_if_due(start + KEY_HINT_DELAY);
        assert!(host.observe(ModifiersState::empty(), true, start + KEY_HINT_DELAY));
        assert_eq!(host.active(), None);
    }

    /// PIN §7.1.5e′ ③ — **changing the hold while the card is up changes the
    /// card and not the clock**, and the fade's epoch is kept so it does not
    /// blink.
    ///
    /// Mutation: re-arm `settling` instead of rewriting `showing`.
    #[test]
    fn adding_a_modifier_to_a_card_that_is_up_answers_at_once() {
        let start = Instant::now();
        let mut host = KeyHintHost::default();
        host.observe(ModifiersState::CONTROL, true, start);
        let shown = start + KEY_HINT_DELAY;
        assert!(host.activate_if_due(shown));
        let landed = shown + KEY_HINT_FADE;
        assert!(host.observe(CTRL_SHIFT, true, landed));
        assert_eq!(host.active(), Some(CTRL_SHIFT));
        assert!(
            !host.is_fading(landed, Motion::Full),
            "the epoch is kept, so a card that had already landed does not fade in again"
        );
    }

    /// PIN §7.1.5e′ ④ — **a hold that changes before the card is up starts the
    /// wait over**, because the hand is still assembling the chord.
    ///
    /// Mutation: leave `settling`'s deadline alone when the set changes.
    #[test]
    fn adding_a_modifier_before_the_card_is_up_starts_the_wait_over() {
        let start = Instant::now();
        let mut host = KeyHintHost::default();
        host.observe(ModifiersState::CONTROL, true, start);
        let later = start + Duration::from_millis(700);
        host.observe(CTRL_SHIFT, true, later);
        assert!(!host.activate_if_due(start + KEY_HINT_DELAY));
        assert!(host.activate_if_due(later + KEY_HINT_DELAY));
        assert_eq!(host.active(), Some(CTRL_SHIFT));
    }

    /// PIN §7.1.5e′ — **a window that is not offering raises nothing and takes
    /// down whatever is up**, without closing the offer for the next hold.
    ///
    /// Mutation: ignore `offered`.
    #[test]
    fn a_window_that_is_not_offering_raises_no_card() {
        let start = Instant::now();
        let mut host = KeyHintHost::default();
        assert!(!host.observe(ModifiersState::CONTROL, false, start));
        assert!(!host.activate_if_due(start + KEY_HINT_DELAY));
        assert_eq!(host.active(), None);
    }

    /// PIN §7.1.5e′ — **a card that is up is taken down the moment the window
    /// stops offering** — the setting switched off, a menu opening.
    #[test]
    fn a_card_goes_when_the_window_stops_offering() {
        let start = Instant::now();
        let mut host = KeyHintHost::default();
        host.observe(ModifiersState::CONTROL, true, start);
        host.activate_if_due(start + KEY_HINT_DELAY);
        assert!(host.observe(ModifiersState::CONTROL, false, start + KEY_HINT_DELAY));
        assert_eq!(host.active(), None);
    }

    /// PIN — **a settling hold asks for exactly one wake-up, and a landed card
    /// asks for none.** A host that reported a deadline forever would be a loop
    /// that never sleeps (the `WaitUntil` pin in `main.rs`).
    #[test]
    fn the_host_asks_for_a_wake_up_only_while_it_owes_one() {
        let start = Instant::now();
        let frame = Duration::from_millis(16);
        let mut host = KeyHintHost::default();
        assert_eq!(host.deadline(start, Motion::Full, frame), None);
        host.observe(ModifiersState::CONTROL, true, start);
        assert_eq!(
            host.deadline(start, Motion::Full, frame),
            Some(start + KEY_HINT_DELAY)
        );
        let shown = start + KEY_HINT_DELAY;
        host.activate_if_due(shown);
        assert_eq!(
            host.deadline(shown, Motion::Full, frame),
            Some(shown + frame),
            "the fade owes frames"
        );
        let landed = shown + KEY_HINT_FADE;
        assert_eq!(host.deadline(landed, Motion::Full, frame), None);
    }

    /// PIN §7.1.5e′ — **reduced motion gets the card whole on its first frame
    /// and asks for no frames at all.**
    ///
    /// Mutation: drop the `Motion::Reduced` arm from `opacity`.
    #[test]
    fn reduced_motion_shows_the_card_whole_and_asks_for_no_frames() {
        let start = Instant::now();
        let frame = Duration::from_millis(16);
        let mut host = KeyHintHost::default();
        host.observe(ModifiersState::CONTROL, true, start);
        let shown = start + KEY_HINT_DELAY;
        host.activate_if_due(shown);
        assert!((host.opacity(shown, Motion::Reduced) - 1.0).abs() < f32::EPSILON);
        assert_eq!(host.deadline(shown, Motion::Reduced, frame), None);
        assert!(host.opacity(shown, Motion::Full) < 1.0);
    }

    /// PIN §7.1.5e′ — **nothing to say raises no card.**
    #[test]
    fn a_hold_that_claims_nothing_places_no_card() {
        assert!(
            place(&[], &["Ctrl".to_owned()], WINDOW, SCALE, &mut ten_per_char).is_none(),
            "an empty list is not a card reading `nothing here`"
        );
    }

    /// PIN §7.1.5e′ — **the card stands on the window's floor and is centred on
    /// it**, which is the one place in this window nothing is pressed.
    ///
    /// Mutation: anchor the card to the window's top.
    #[test]
    fn the_card_stands_centred_on_the_windows_floor() {
        let layout = place(
            &lines(4),
            &["Ctrl".to_owned()],
            WINDOW,
            SCALE,
            &mut ten_per_char,
        )
        .expect("four lines make a card");
        let [left, _, right, bottom] = layout.frame;
        assert_eq!(bottom, WINDOW.1 - KEY_HINT_WINDOW_INSET_LOGICAL_PX);
        assert!(
            (((left + right) / 2.0) - WINDOW.0 / 2.0).abs() <= 1.0,
            "centred on the window: {left} .. {right}"
        );
    }

    /// PIN §7.1.5e′ — **the lines are dealt into columns rather than run off the
    /// bottom of the window**, and no card takes more of the window's height
    /// than [`KEY_HINT_MAX_BODY_FRACTION`] plus its own box.
    ///
    /// Mutation: make `per_column` `lines.len()`.
    #[test]
    fn a_long_list_is_dealt_into_columns_instead_of_running_off_the_glass() {
        let short = (1200.0, 400.0);
        let layout = place(
            &lines(24),
            &["Ctrl".to_owned()],
            short,
            SCALE,
            &mut ten_per_char,
        )
        .expect("twenty-four lines make a card");
        let height = layout.frame[3] - layout.frame[1];
        assert!(
            height < short.1 * 0.75,
            "a card {height} tall on a {} window has run off the glass",
            short.1
        );
        assert!(layout.frame[0] >= 0.0);
        assert!(layout.frame[2] <= short.0);
        // Every line placed, in columns: the last one starts to the right of the
        // first and no lower than it.
        let first = &layout.lines[0];
        let last = layout.lines.last().expect("lines were placed");
        assert!(last.cap[0] > first.cap[0], "a second column was opened");
    }

    /// PIN §7.1.5e′ — **a card that could not fit everything says how much it
    /// left out**, and never silently teaches that a chord does not exist.
    ///
    /// Mutation: drop the `overflow` arm and show `capacity` lines.
    #[test]
    fn a_card_that_cannot_fit_everything_reports_the_count() {
        // One column's worth of room and a great many lines.
        let narrow = (260.0, 240.0);
        let layout = place(
            &lines(40),
            &["Ctrl".to_owned()],
            narrow,
            SCALE,
            &mut ten_per_char,
        )
        .expect("forty lines make a card");
        let (_, text) = layout.overflow.as_ref().expect("the card cut something");
        assert!(!text.is_empty());
        assert!(
            layout.lines.len() < 40,
            "the card would not have cut anything"
        );
    }

    /// PIN §7.1.5e′ — **the names all start at one `x`**, which is what the one
    /// cap width across the card buys.
    ///
    /// Mutation: give each cap its own width in `place`.
    #[test]
    fn every_name_in_a_column_starts_at_the_same_x() {
        let mixed = vec![
            HintLine {
                key: "N".to_owned(),
                title: "New tab",
            },
            HintLine {
                key: "PageDown".to_owned(),
                title: "A much longer verb",
            },
        ];
        let layout = place(
            &mixed,
            &["Ctrl".to_owned()],
            WINDOW,
            SCALE,
            &mut ten_per_char,
        )
        .expect("two lines make a card");
        assert_eq!(layout.lines[0].name[0], layout.lines[1].name[0]);
        assert_eq!(layout.lines[0].cap[2], layout.lines[1].cap[2]);
    }

    /// PIN §7.1.5e′ — **the head never prints past the card's own edge**, which
    /// is what makes it a floor under the width rather than a passenger on it.
    ///
    /// Found by reading: one narrow column (`Tab` against a short verb) is
    /// narrower than `Ctrl Shift`, and a card sized from its body alone puts the
    /// second cap outside its own frame.
    ///
    /// Mutation: drop the `.max(head_width)`.
    #[test]
    fn a_card_narrower_than_its_head_is_widened_to_hold_it() {
        let narrow = vec![HintLine {
            key: "N".to_owned(),
            title: "Go",
        }];
        let layout = place(
            &narrow,
            &["Ctrl".to_owned(), "Shift".to_owned()],
            WINDOW,
            SCALE,
            &mut ten_per_char,
        )
        .expect("one line makes a card");
        let last = layout.head.last().expect("two caps");
        assert!(
            last.0[2] <= layout.frame[2] - KEY_HINT_PADDING_X_LOGICAL_PX,
            "the head's last cap ends at {} and the card at {}",
            last.0[2],
            layout.frame[2]
        );
    }

    /// PIN §7.1.5e′ — **the head carries the caps that are already down**, so a
    /// reader can tell which hold the card is answering when it changes under
    /// their fingers.
    #[test]
    fn the_head_says_which_hold_is_being_answered() {
        let layout = place(
            &lines(2),
            &["Ctrl".to_owned(), "Shift".to_owned()],
            WINDOW,
            SCALE,
            &mut ten_per_char,
        )
        .expect("two lines make a card");
        let words: Vec<&str> = layout.head.iter().map(|(_, cap)| cap.as_str()).collect();
        assert_eq!(words, ["Ctrl", "Shift"]);
        assert!(
            layout.head[1].0[0] > layout.head[0].0[2] - 1.0,
            "the second cap stands to the right of the first"
        );
        assert!(
            layout.lines[0].cap[1] >= layout.head[0].0[3],
            "the lines stand under the head"
        );
    }
}
