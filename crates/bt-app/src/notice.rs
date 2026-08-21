//! The PowerShell integration notice — one sentence, two words and a close, in
//! a strip across the top of one pane's body (`docs/DESIGN.md` §7.1.6j, user
//! ruling 2026-08-21).
//!
//! Spec authority is `design/ui-mockup.html`: the `.pnotice` block for the
//! surface and its three design notes, and `psNoticeHtml` / `psNoticePress` for
//! what it says and what a press means. Every number below is that stylesheet's.
//!
//! **It takes a row and the body yields**, which is the one thing that separates
//! it from the search capsule it otherwise borrows from. The capsule overlays a
//! corner because search is a state entered on purpose and left in seconds; this
//! is a notice nobody asked for that stays until it is answered, and a strip
//! parked on the terminal's first line would hide output for as long as it is
//! up. So the pane's body is measured one bar shorter while it is there —
//! [`crate::seats::pane_body_viewport`] is the one place that subtraction is
//! made, exactly where the pane head's own is made.
//!
//! **It is not a dialog and it does not dim.** The pane behind it is a working
//! shell — the shell it is about — and `crate::restore` already settled that a
//! question over a working app is not a gate in front of one.
//!
//! **Its ground is `--panel`.** It is a band in the pane's own column, not a
//! thing floating over the text, so it takes the floor the panes sit on rather
//! than a floating window's face; a `--menu` surface with a shadow under it
//! would be the capsule wearing a bar's shape. Every ink below is that ground's
//! own composite — the title bar's family, which is `--panel`'s.

use bt_render::{ChromeLabel, ChromeLabelWeight, ChromePalette, OverlayQuad, rounded_overlay_fill};

use crate::{
    i18n::Text,
    marks::{ChromeMark, ChromeSprite, OverlayLayer},
};

/// `.pnotice { height: 30px }` — and it is the pane head's own 30, because the
/// two are the same kind of band and a reader who sees both must not see two
/// heights.
pub const BAR_HEIGHT_LOGICAL_PX: f32 = 30.0;
/// `padding: 0 8px 0 11px`.
const PADDING_LEFT_LOGICAL_PX: f32 = 11.0;
const PADDING_RIGHT_LOGICAL_PX: f32 = 8.0;
/// `gap: 8px`.
const GAP_LOGICAL_PX: f32 = 8.0;
/// `font-size: 12px`, for the sentence and for the verbs alike.
pub const FONT_LOGICAL_PX: f32 = 12.0;
/// `.pn-act { padding: 3px 7px; border-radius: 5px }`, around a 12px line box.
const VERB_PADDING_X_LOGICAL_PX: f32 = 7.0;
const VERB_HEIGHT_LOGICAL_PX: f32 = 22.0;
const VERB_RADIUS_LOGICAL_PX: f32 = 5.0;
/// `.pn-act:last-of-type { margin-right: 2px }` — the trailing column belongs to
/// the `×`, so the last verb and the close can never share a pixel however long
/// the word gets.
const VERB_TRAILING_GAP_LOGICAL_PX: f32 = 2.0;
/// `.pn-x { width: 22px; height: 22px; border-radius: 6px }` with a 10px glyph.
const CLOSE_BOX_LOGICAL_PX: f32 = 22.0;
const CLOSE_RADIUS_LOGICAL_PX: f32 = 6.0;
const CLOSE_GLYPH_LOGICAL_PX: f32 = 10.0;

/// One of the strip's pressable words.
///
/// Three and not two, because the strip has two states and the second one
/// offers a different verb — see [`Notice`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeVerb {
    /// Write the line into this pane's `$PROFILE`, keeping a copy first.
    Add,
    /// Stop offering, for good, everywhere — the setting and not a dismissal.
    Never,
    /// Start this pane's shell again, so the line that was just written is read.
    Restart,
}

impl NoticeVerb {
    /// The word.
    ///
    /// `Restart` spends the pane menu's own string rather than a second
    /// spelling of one verb: both surfaces call `Runtime::restart_shell`, and a
    /// verb written twice is a verb that will one day be translated twice.
    #[must_use]
    pub fn text(self) -> &'static str {
        match self {
            Self::Add => Text::PowerShellNoticeAdd.text(),
            Self::Never => Text::PowerShellNoticeNever.text(),
            Self::Restart => Text::TermMenuShellAgain.text(),
        }
    }
}

/// What the strip is saying, which decides what it offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// The integration is not installed. Two verbs: install it, or end the
    /// asking.
    Offer,
    /// It has been written into the file. **One** verb, because the strip is now
    /// a report of one thing that happened and the only thing left to decide is
    /// whether to make it true now or when the next shell starts.
    Added,
}

impl Notice {
    /// The sentence.
    #[must_use]
    pub fn text(self) -> &'static str {
        match self {
            Self::Offer => Text::PowerShellNoticeBody.text(),
            Self::Added => Text::PowerShellNoticeAdded.text(),
        }
    }

    /// The verbs, left to right.
    #[must_use]
    pub fn verbs(self) -> &'static [NoticeVerb] {
        match self {
            Self::Offer => &[NoticeVerb::Add, NoticeVerb::Never],
            Self::Added => &[NoticeVerb::Restart],
        }
    }
}

/// Something on the strip the pointer can be over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeElement {
    Verb(NoticeVerb),
    Close,
    /// The strip's own width, which answers no press but takes it — a bar with a
    /// hole in it would let a click through onto a terminal cell that is not
    /// under the pointer.
    Body,
}

/// Every rectangle the strip draws and hit-tests, in physical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct NoticeBar {
    pub frame: [f32; 4],
    /// The hairline across its foot — the whole of its separation from the
    /// terminal below, exactly as the pane head's is.
    pub edge: [f32; 4],
    /// Where the sentence goes. What gives way in a narrow pane, because a strip
    /// whose verbs had been squeezed out would be a strip with nothing to press.
    pub text: [f32; 4],
    pub verbs: Vec<(NoticeVerb, [f32; 4])>,
    pub close: [f32; 4],
}

/// Lay the strip out in `strip`, the row [`crate::seats::pane_notice_strip`]
/// took from the pane's body.
///
/// `widths` is each verb's measured text width in physical pixels, in the order
/// [`Notice::verbs`] gives them — measured by the caller for the reason the
/// search capsule's counter is: this module has no font, and a width guessed
/// from a character count is a box that fits in English and clips in Chinese.
///
/// **Laid out from the right.** The `×` keeps its column, the verbs keep theirs,
/// and the sentence takes what is left — which can be nothing, and a sentence
/// with no room is a sentence that is not drawn rather than one that overruns
/// the words beside it.
#[must_use]
pub fn lay_out(strip: [f32; 4], notice: Notice, widths: &[f32], scale: f32) -> NoticeBar {
    let px = |logical: f32| logical * scale;
    let hairline = px(1.0).round().max(1.0);
    let middle = (strip[1] + strip[3]) / 2.0;
    let centred_box = |height: f32| {
        let top = (middle - height / 2.0).round();
        (top, top + height)
    };

    let close_box = px(CLOSE_BOX_LOGICAL_PX).round();
    let (close_top, close_bottom) = centred_box(close_box);
    let close_right = (strip[2] - px(PADDING_RIGHT_LOGICAL_PX)).round();
    let close = [
        close_right - close_box,
        close_top,
        close_right,
        close_bottom,
    ];

    let verb_height = px(VERB_HEIGHT_LOGICAL_PX).round();
    let (verb_top, verb_bottom) = centred_box(verb_height);
    let verb_padding = px(VERB_PADDING_X_LOGICAL_PX).round();
    let gap = px(GAP_LOGICAL_PX).round();
    let mut right = close[0] - px(VERB_TRAILING_GAP_LOGICAL_PX).round();
    let mut verbs: Vec<(NoticeVerb, [f32; 4])> = Vec::new();
    for (verb, width) in notice.verbs().iter().rev().zip(widths.iter().rev()) {
        let box_width = (width + 2.0 * verb_padding).round();
        let left = right - box_width;
        verbs.push((*verb, [left, verb_top, right, verb_bottom]));
        right = left - gap;
    }
    // Laid out right to left and read left to right, which is the order the
    // caller's `widths` are in and the order a hit test walks.
    verbs.reverse();

    let text_left = (strip[0] + px(PADDING_LEFT_LOGICAL_PX)).round();
    let text_right = (right - gap).max(text_left);
    NoticeBar {
        frame: strip,
        edge: [strip[0], strip[3] - hairline, strip[2], strip[3]],
        text: [text_left, strip[1], text_right, strip[3]],
        verbs,
        close,
    }
}

/// What is under the pointer, or `None` when the pointer is not on the strip.
#[must_use]
pub fn hit(bar: &NoticeBar, x: f32, y: f32) -> Option<NoticeElement> {
    let inside = |rect: [f32; 4]| x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3];
    if !inside(bar.frame) {
        return None;
    }
    if inside(bar.close) {
        return Some(NoticeElement::Close);
    }
    Some(
        bar.verbs
            .iter()
            .find(|(_, box_)| inside(*box_))
            .map_or(NoticeElement::Body, |(verb, _)| NoticeElement::Verb(*verb)),
    )
}

/// Draw one strip.
#[must_use]
pub fn build(
    bar: &NoticeBar,
    notice: Notice,
    hover: Option<NoticeElement>,
    palette: &ChromePalette,
    scale: f32,
) -> OverlayLayer {
    let px = |logical: f32| logical * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let mut quads = vec![
        OverlayQuad {
            rect: bar.frame,
            color: palette.termhost,
            alpha: 1.0,
        },
        // `--border` rather than the pane head's `--border-soft`: the head's
        // hairline is a composite over `--termbg` and this band is not on
        // `--termbg`, so it takes the one border in the palette that carries its
        // own alpha and is therefore true on any ground.
        OverlayQuad {
            rect: bar.edge,
            color: palette.menu_border,
            alpha: alpha(palette.menu_border_alpha),
        },
    ];
    let mut labels = Vec::new();
    let mut sprites = Vec::new();

    if bar.text[2] > bar.text[0] {
        labels.push(ChromeLabel {
            mono: false,
            text: notice.text().to_owned(),
            rect: bar.text,
            font_size_px: px(FONT_LOGICAL_PX),
            // `--ink2` over `--panel`: a notice is wayfinding rather than
            // content, and it sits a step below the terminal's own ink for the
            // same reason the title bar does.
            color: palette.title_text,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: Some(bar.text),
        });
    }

    for (verb, box_) in &bar.verbs {
        let hovered = hover == Some(NoticeElement::Verb(*verb));
        if hovered {
            quads.extend(rounded_overlay_fill(
                *box_,
                px(VERB_RADIUS_LOGICAL_PX),
                palette.caption_hover,
                1.0,
            ));
        }
        labels.push(ChromeLabel {
            mono: false,
            text: verb.text().to_owned(),
            rect: *box_,
            font_size_px: px(FONT_LOGICAL_PX),
            // The full ink of this ground against the sentence's muted one, at
            // rest and under the pointer alike: a word that only becomes ink
            // when the pointer finds it is a word nobody knows is pressable.
            color: palette.title_text_hover,
            align_right: false,
            align_center: true,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Medium,
            tabular_numerals: false,
            clip: Some(*box_),
        });
    }

    let closing = hover == Some(NoticeElement::Close);
    if closing {
        quads.extend(rounded_overlay_fill(
            bar.close,
            px(CLOSE_RADIUS_LOGICAL_PX),
            palette.caption_hover,
            1.0,
        ));
    }
    sprites.push(ChromeSprite::new(
        ChromeMark::TabClose,
        centred(bar.close, px(CLOSE_GLYPH_LOGICAL_PX)),
        if closing {
            palette.title_text_hover
        } else {
            palette.title_text_muted
        },
    ));

    OverlayLayer {
        quads,
        labels,
        sprites,
        ..OverlayLayer::default()
    }
}

/// A box of `size` centred in `rect`.
fn centred(rect: [f32; 4], size: f32) -> [f32; 4] {
    let x = (rect[0] + (rect[2] - rect[0] - size) / 2.0).round();
    let y = (rect[1] + (rect[3] - rect[1] - size) / 2.0).round();
    [x, y, x + size, y + size]
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRIP: [f32; 4] = [100.0, 40.0, 700.0, 70.0];

    /// The `×` keeps the trailing column, the verbs keep theirs in the order
    /// they are read, and the sentence takes what is left — the strip's whole
    /// arithmetic, in the order it is decided.
    #[test]
    fn the_close_keeps_the_trailing_column_and_the_sentence_takes_what_is_left() {
        let bar = lay_out(STRIP, Notice::Offer, &[90.0, 100.0], 1.0);
        assert_eq!(bar.close[2], 692.0, "8px of padding off the right edge");
        assert_eq!(bar.close[2] - bar.close[0], 22.0);
        let boxes: Vec<[f32; 4]> = bar.verbs.iter().map(|(_, box_)| *box_).collect();
        assert_eq!(
            bar.verbs.iter().map(|(verb, _)| *verb).collect::<Vec<_>>(),
            [NoticeVerb::Add, NoticeVerb::Never],
            "read left to right in the order the notice gives them"
        );
        assert!(
            boxes[0][2] < boxes[1][0],
            "the two words never share a pixel: {boxes:?}"
        );
        assert!(
            boxes[1][2] <= bar.close[0],
            "nor does the last word share one with the ×"
        );
        assert_eq!(
            boxes[1][2] - boxes[1][0],
            114.0,
            "100 measured + 7 each side"
        );
        assert!(
            bar.text[2] <= boxes[0][0],
            "the sentence stops one gap short of the first word"
        );
        assert_eq!(bar.text[0], 111.0, "11px of padding off the left edge");
    }

    /// A pane too narrow to hold the sentence still holds the verbs. The strip
    /// exists to be pressed, and a bar that gave its last pixels to a sentence
    /// nobody can act on would be the wrong half surviving.
    #[test]
    fn a_narrow_pane_keeps_its_verbs_and_gives_up_its_sentence() {
        let bar = lay_out([0.0, 0.0, 200.0, 30.0], Notice::Offer, &[90.0, 100.0], 1.0);
        assert_eq!(bar.close[2] - bar.close[0], 22.0);
        assert_eq!(bar.verbs.len(), 2);
        assert!(
            bar.text[2] <= bar.text[0],
            "an empty box, which `build` draws nothing into: {:?}",
            bar.text
        );
        assert!(
            build(&bar, Notice::Offer, None, &bt_render::chrome_palette(), 1.0)
                .labels
                .iter()
                .all(|label| label.text != Notice::Offer.text()),
            "the sentence is not drawn where there is no room for it"
        );
    }

    /// The whole frame is claimed. A press on the strip's empty width is the
    /// strip's, because a bar with a hole in it lets a click through onto a
    /// terminal cell that is nowhere near the pointer.
    #[test]
    fn the_strip_claims_its_whole_width_and_answers_for_every_part_of_it() {
        let bar = lay_out(STRIP, Notice::Offer, &[90.0, 100.0], 1.0);
        assert_eq!(hit(&bar, 105.0, 55.0), Some(NoticeElement::Body));
        assert_eq!(hit(&bar, 690.0, 55.0), Some(NoticeElement::Close));
        let (verb, box_) = bar.verbs[0];
        assert_eq!(
            hit(&bar, (box_[0] + box_[2]) / 2.0, 55.0),
            Some(NoticeElement::Verb(verb))
        );
        assert_eq!(hit(&bar, 105.0, 39.0), None, "above the strip is nobody's");
        assert_eq!(hit(&bar, 105.0, 70.0), None, "nor is the row below it");
    }

    /// The second state offers one verb and not three. A card is the report of
    /// one thing that happened, and the moment it offers two answers it is a
    /// dialog.
    #[test]
    fn the_written_state_offers_one_verb() {
        assert_eq!(Notice::Added.verbs(), [NoticeVerb::Restart]);
        let bar = lay_out(STRIP, Notice::Added, &[80.0], 1.0);
        assert_eq!(bar.verbs.len(), 1);
        assert_eq!(bar.verbs[0].0, NoticeVerb::Restart);
    }

    /// The verb reuses the pane menu's own string, which is the one this build
    /// already ships for the one function both surfaces call.
    #[test]
    fn the_restart_verb_is_the_pane_menus_verb() {
        assert_eq!(NoticeVerb::Restart.text(), Text::TermMenuShellAgain.text());
    }
}
