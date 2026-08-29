//! **One strip: a sentence, some words and a close, across the top of one
//! pane's body** (`docs/DESIGN.md` §7.1.6j, user ruling 2026-08-21; widened by
//! the user ruling of 2026-08-29).
//!
//! It was written for the PowerShell integration offer and it is still shaped by
//! it, but nothing below this line knows what a shell is. What it knows is a
//! band: a sentence that gives way, a row of pressable words that never do, and
//! an `×` that outlives both. The second thing that wears it is a **preview**
//! whose file moved on the disk under unsaved edits — the same band, in the same
//! row of the same body, because a reader who meets both must not meet two
//! heights, two grounds and two ideas of where the close button lives.
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
    /// Throw this window's unsaved edits away and take the file as it now is.
    ReloadFromDisk,
    /// Keep the edits and take the strip down. Nothing is written and nothing is
    /// read; the disagreement is still there and [`crate::preview::PreviewBuffer::save`]
    /// is still the thing that will report it.
    KeepMyEdits,
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
            Self::ReloadFromDisk => Text::PreviewDiskReload.text(),
            Self::KeepMyEdits => Text::PreviewDiskKeep.text(),
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
    /// **A preview's file was rewritten under unsaved edits** (user ruling
    /// 2026-08-29). Two verbs, and they are the two answers a person can give:
    /// take the file, or keep what you typed.
    DiskChanged,
    /// **A preview's file is gone.** **No** verb at all: there is nothing to
    /// reload and nothing to keep — the buffer is already kept, which is the
    /// ruling — so the only control is the `×` that says "I have read this".
    DiskDeleted,
}

impl Notice {
    /// The sentence.
    #[must_use]
    pub fn text(self) -> &'static str {
        match self {
            Self::Offer => Text::PowerShellNoticeBody.text(),
            Self::Added => Text::PowerShellNoticeAdded.text(),
            Self::DiskChanged => Text::PreviewDiskChanged.text(),
            Self::DiskDeleted => Text::PreviewDiskDeleted.text(),
        }
    }

    /// The verbs, left to right.
    #[must_use]
    pub fn verbs(self) -> &'static [NoticeVerb] {
        match self {
            Self::Offer => &[NoticeVerb::Add, NoticeVerb::Never],
            Self::Added => &[NoticeVerb::Restart],
            // **Reload on the right**, nearest the `×`: it is the destructive
            // one, and the layout drops words leftmost-first when the pane
            // narrows — so the word that survives a narrow pane must be the one
            // whose absence is recoverable. Keeping the edits is what happens
            // anyway if nothing is pressed.
            Self::DiskChanged => &[NoticeVerb::KeepMyEdits, NoticeVerb::ReloadFromDisk],
            Self::DiskDeleted => &[],
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
///
/// # What gives way, in what order (user ruling 2026-08-28, §7.43)
///
/// Gate 5's 700×420 window photographed the strip with the sentence lying across
/// `Add to $PROFILE`. The sentence *was* already being given the leftovers — and
/// the leftovers were being computed against verbs that had walked off the
/// strip's left edge, so "what is left" was a negative number that clamped to
/// the whole width and the label was drawn over the words it was supposed to
/// stop short of.
///
/// So the order is now written down and it is the ruling's: **a button is never
/// cut and never wrapped**. First the sentence is elided to what is left of the
/// row (the caller does that, with the font — see [`build`]); then, when the
/// buttons themselves no longer fit between the `×` and the strip's own left
/// padding, the sentence goes entirely and the buttons keep the row; then the
/// buttons go, leftmost first, because the one nearest the `×` is the one the
/// second state leaves standing alone. What survives every width is the `×`,
/// which is the only control that can end the asking without an answer.
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
    let text_left = (strip[0] + px(PADDING_LEFT_LOGICAL_PX)).round();
    let mut right = close[0] - px(VERB_TRAILING_GAP_LOGICAL_PX).round();
    let mut verbs: Vec<(NoticeVerb, [f32; 4])> = Vec::new();
    for (verb, width) in notice.verbs().iter().rev().zip(widths.iter().rev()) {
        let box_width = (width + 2.0 * verb_padding).round();
        let left = right - box_width;
        // **A word that does not fit is a word that is not offered**, and every
        // word to the left of it goes with it: they are laid out right to left,
        // so the first one to run out of room is the first one whose neighbours
        // have less room still. Drawing it anyway is what put a button half off
        // the strip and a sentence on top of the other half.
        if left < text_left {
            break;
        }
        verbs.push((*verb, [left, verb_top, right, verb_bottom]));
        right = left - gap;
    }
    // A word had to be dropped, so the row is already narrower than the strip's
    // own furniture: the sentence is gone rather than creeping back into the
    // space the dropped word left, which would be a notice that grows its prose
    // as the pane gets smaller.
    let all_offered = verbs.len() == notice.verbs().len();
    // Laid out right to left and read left to right, which is the order the
    // caller's `widths` are in and the order a hit test walks.
    verbs.reverse();

    let text_right = if all_offered {
        (right - gap).max(text_left)
    } else {
        text_left
    };
    NoticeBar {
        frame: strip,
        edge: [strip[0], strip[3] - hairline, strip[2], strip[3]],
        text: [text_left, strip[1], text_right, strip[3]],
        verbs,
        close,
    }
}

/// **The sentence as much of it as the row it was left can hold** (§7.43).
///
/// The ruling's first clause — 「按钮保持完整,文案截断带省略号」 — turned into the
/// one call this window already makes for that question, with the caller's own
/// font handed in because a prefix is only the right prefix against the face
/// that will draw it.
///
/// **A lone `…` is nothing said.** `settings::ellipsized` answers a bare
/// ellipsis when not even one character fits, which is what CSS draws and is
/// right for a label in a table; in a strip whose other half is a row of buttons
/// it is a dot of noise where a sentence used to be, and the ruling's next
/// clause is that the prose is what disappears. So the floor is one character
/// plus the ellipsis, and under it the row is the buttons and the `×`.
#[must_use]
pub fn sentence(
    notice: Notice,
    bar: &NoticeBar,
    font_px: f32,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> String {
    let available = bar.text[2] - bar.text[0];
    if available <= 0.0 {
        return String::new();
    }
    let said = crate::settings::ellipsized(notice.text(), available, font_px, measure);
    if said.chars().all(|character| character == '\u{2026}') {
        return String::new();
    }
    said
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
///
/// `say` is the sentence **as it fits** — [`Notice::text`] when the whole of it
/// does, and the longest prefix of it with a `…` when it does not. Elided by the
/// caller and not here, on this module's own standing division: the box is a
/// number and the prefix that fills it is a question only something holding a
/// font can answer (`settings::ellipsized`, the same one the settings page's
/// three-line descriptions use). Empty draws nothing, which is what a row with
/// no room for prose is.
#[must_use]
pub fn build(
    bar: &NoticeBar,
    say: &str,
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

    if bar.text[2] > bar.text[0] && !say.is_empty() {
        labels.push(ChromeLabel {
            mono: false,
            text: say.to_owned(),
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
        let bar = lay_out([0.0, 0.0, 275.0, 30.0], Notice::Offer, &[90.0, 100.0], 1.0);
        assert_eq!(bar.close[2] - bar.close[0], 22.0);
        assert_eq!(bar.verbs.len(), 2);
        assert!(
            bar.text[2] <= bar.text[0],
            "an empty box, which `build` draws nothing into: {:?}",
            bar.text
        );
        assert!(
            build(&bar, "", None, &bt_render::chrome_palette(), 1.0)
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

    /// A stand-in for a proportional face: every character the same width, which
    /// is all this arithmetic needs and is monotonic in a prefix's length, which
    /// is what `settings::ellipsized`'s binary search needs.
    fn measured(text: &str, font_size_px: f32) -> f32 {
        text.chars().count() as f32 * font_size_px * 0.55
    }

    /// RED — **a button is never drawn under the sentence and never drawn
    /// half** (user ruling 2026-08-28, `docs/DESIGN.md` §7.43).
    ///
    /// Gate 5's 700×420 photograph: `PowerShell integration is not installed.`
    /// lying across `Add to $PROFILE`. Two things were wrong and the second is
    /// the one that made a picture — the verbs were laid out right to left with
    /// nothing stopping them walking off the strip's left edge, so at that width
    /// `Add to $PROFILE` started at a negative x; and the sentence's box was
    /// `(right - gap).max(text_left)`, which with a negative `right` clamps to
    /// the whole row. The label was then drawn from the left padding across
    /// everything.
    ///
    /// The ruling's order, asserted at every width between "roomy" and "one
    /// pixel": the buttons keep whole boxes, the sentence is elided into what is
    /// left, and when a button will not fit whole it is dropped rather than cut,
    /// taking the sentence with it. The `×` outlives everything.
    ///
    /// MUTATIONS: take the `left < text_left` break out and the "wholly inside"
    /// assertion goes red at the narrow widths; restore
    /// `(right - gap).max(text_left)` unconditionally and the "no overlap"
    /// assertion goes red at exactly the widths gate 5 photographed.
    #[test]
    fn a_notice_bars_actions_never_overlap_its_text() {
        let font = FONT_LOGICAL_PX;
        let widths: Vec<f32> = Notice::Offer
            .verbs()
            .iter()
            .map(|verb| measured(verb.text(), font))
            .collect();
        let mut seen_without_every_verb = false;
        let mut seen_elided = false;
        for width in (1..=900).map(|step| step as f32) {
            let strip = [0.0, 0.0, width, 30.0];
            let bar = lay_out(strip, Notice::Offer, &widths, 1.0);
            let available = bar.text[2] - bar.text[0];
            let say = sentence(Notice::Offer, &bar, font, &mut measured);
            seen_elided |= say.ends_with('\u{2026}');
            seen_without_every_verb |= bar.verbs.len() < Notice::Offer.verbs().len();

            for (index, (verb, box_)) in bar.verbs.iter().enumerate() {
                // Whole, which is the half of the ruling that is about the
                // buttons: the box is exactly its caption plus its padding, and
                // all of it is on the strip. The caption is looked up by the
                // verb and never by position, because a row that has dropped a
                // word is exactly the row where the two disagree.
                let offered = Notice::Offer
                    .verbs()
                    .iter()
                    .position(|offered| offered == verb)
                    .expect("a word on the strip is one the notice offers");
                assert_eq!(
                    box_[2] - box_[0],
                    (widths[offered] + 2.0 * VERB_PADDING_X_LOGICAL_PX).round(),
                    "at {width}px `{}` is not drawn whole: {box_:?}",
                    verb.text()
                );
                assert!(
                    box_[0] >= strip[0] && box_[2] <= strip[2],
                    "at {width}px `{}` hangs off the strip: {box_:?}",
                    verb.text()
                );
                assert!(
                    box_[2] <= bar.close[0],
                    "at {width}px `{}` reaches into the ×: {box_:?} / {:?}",
                    verb.text(),
                    bar.close
                );
                if index + 1 < bar.verbs.len() {
                    assert!(
                        box_[2] <= bar.verbs[index + 1].1[0],
                        "at {width}px two words share a pixel: {:?}",
                        bar.verbs
                    );
                }
                // And the sentence's own box stops short of every one of them,
                // which is the half that made the photograph.
                assert!(
                    bar.text[2] <= box_[0] || bar.text[2] <= bar.text[0],
                    "at {width}px the sentence's box runs into `{}`: {:?} / {box_:?}",
                    verb.text(),
                    bar.text
                );
            }
            assert!(
                bar.text[2] <= bar.close[0] || bar.text[2] <= bar.text[0],
                "at {width}px the sentence's box runs into the ×: {:?}",
                bar.text
            );
            // What is drawn fits what it was given, which is what the elision
            // buys over a clip: a clipped word ends mid-letter and reads as a
            // rendering fault rather than as "there is more".
            if !say.is_empty() {
                assert!(
                    measured(&say, font) <= available,
                    "at {width}px the drawn sentence is wider than its box: {say:?}"
                );
                assert!(
                    Notice::Offer
                        .text()
                        .starts_with(say.trim_end_matches('\u{2026}')),
                    "at {width}px the sentence is not a prefix of itself: {say:?}"
                );
            }
            // The × is the last thing standing, at every width.
            assert_eq!(bar.close[2] - bar.close[0], (CLOSE_BOX_LOGICAL_PX).round());
        }
        assert!(
            seen_elided,
            "no width in the sweep ever elided the sentence, so the ellipsis half \
             of the ruling is untested"
        );
        assert!(
            seen_without_every_verb,
            "no width in the sweep ever dropped a word, so the 「a button is never \
             cut」 half of the ruling is untested"
        );
    }

    /// RED — **and the widths gate 5 actually photographed.**
    ///
    /// A pane in a 700-pixel window is about 660 wide once the window's own
    /// chrome is off it; the sweep above covers it, and this names it so the
    /// number in the report is in the file.
    #[test]
    fn the_seven_hundred_pixel_window_draws_no_word_under_its_sentence() {
        let font = FONT_LOGICAL_PX;
        let widths: Vec<f32> = Notice::Offer
            .verbs()
            .iter()
            .map(|verb| measured(verb.text(), font))
            .collect();
        let bar = lay_out([0.0, 0.0, 660.0, 30.0], Notice::Offer, &widths, 1.0);
        assert_eq!(bar.verbs.len(), 2, "both words fit a 660px row whole");
        assert!(
            bar.text[2] <= bar.verbs[0].1[0],
            "the sentence stops short of the first word: {:?} / {:?}",
            bar.text,
            bar.verbs
        );
        let say = sentence(Notice::Offer, &bar, font, &mut measured);
        let layer = build(&bar, &say, None, &bt_render::chrome_palette(), 1.0);
        for label in &layer.labels {
            assert!(
                measured(&label.text, font) <= label.rect[2] - label.rect[0] + 0.5,
                "{:?} does not fit the box it is drawn in: {:?}",
                label.text,
                label.rect
            );
        }
    }
}
