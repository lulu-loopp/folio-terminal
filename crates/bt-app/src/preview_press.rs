//! **A gesture on a rendered page is answered when it ends, not when it begins**
//! (owner's report and ruling 2026-09-21; `docs/DESIGN.md`'s entry of that date,
//! refining §7.1.3q and §7.1.3w).
//!
//! [`crate::preview_live`]'s one rule is about *drawing*: the block whose source
//! range contains the caret is drawn as source. This module is about **when the
//! caret moves**, which is the other half of that sentence and the half nobody
//! had said out loud. It moved on the press, so the block under the pointer put
//! its marks back and re-flowed the instant the button went down — and went on
//! changing shape under a hand that was still drawing a selection across it.
//!
//! So a press on a rendered page **records** what it would do and does nothing,
//! and the release **spends** the record. There is one rule and no special case
//! in it: a press names one of two things ([`Pressed`]), the release turns that
//! and the shape of the gesture into one answer ([`Spend`]), and the window's
//! whole part is to work out the two file offsets and then obey.
//!
//! **What the gesture draws in the meantime is the page's own pieces**
//! ([`crate::preview_select`]), which is the model that needs no block to be
//! source: the caret has not moved, so the caret has nothing to draw. That is
//! also why what a drag copies while it is in flight is the *rendered* text and
//! what it copies once the button is up is the file's own bytes — the two models
//! hand over at the release, exactly where the page changes face.
//!
//! **One press is answered where it stands, and the rule says which** (closure
//! review of this ruling, 2026-09-21). The page's face is a function of the
//! caret's *seat* alone ([`caret_seat`]) — so a caret moving **inside the seat
//! the page is already drawing** changes no face, and there is nothing to
//! defer. [`keeps_the_seat`] is that question, and it is the whole of the
//! exception: a press inside the block already drawn as source seats the caret
//! at once and its drag goes on extending it, which is the ordinary editing
//! gesture and must go on showing the selection as it is drawn. The moment the
//! hand leaves the seat the caret stops following it — the seat may not change
//! while a gesture is in flight — and the release places it where the hand let
//! go.
//!
//! **Pure, in [`crate::preview_live`]'s style**: no window, no pane and no
//! pointer. Which byte a point names is the window's question and is asked
//! before a value of either type is made; everything after that is a function of
//! the record and of whether the hand travelled.

use std::ops::Range;

use crate::preview_live::caret_seat;
use crate::preview_select::Grain;

/// **Whether a press may be answered where it stands**: the byte it named and
/// the caret already standing in the page are in one seat, so putting the caret
/// there changes no block's face.
///
/// `caret` is the caret the page is *drawing from* — `None` for a page nobody
/// has entered, which draws no source block at all and therefore has no seat to
/// keep. That is why a first press into a rendered page always waits for the
/// release and a press inside the block already open never does.
///
/// Asked in seats rather than in ranges so that the two positions a range cannot
/// tell apart — the end of an unterminated last block, and the blank line after
/// a paragraph — are answered here exactly as the page answers them when it
/// decides what to draw. A gap is a seat like any other: the empty line it is
/// drawn as stands in the same place whichever of its bytes the caret is on.
#[must_use]
pub fn keeps_the_seat(
    content: &str,
    ranges: &[Range<usize>],
    caret: Option<usize>,
    offset: usize,
) -> bool {
    let Some(caret) = caret else {
        return false;
    };
    caret_seat(content, ranges, caret) == caret_seat(content, ranges, offset)
}

/// **What a press on a rendered page named**, kept until the gesture it began
/// has ended.
///
/// Two arms, because a press on a page that can be typed into is one of exactly
/// two things: it names a byte of the file, or it names none — the margin beside
/// a block, the ground under the last one — which is the page's empty ground and
/// is what takes a reader back out of it (owner's ruling 2026-09-10).
///
/// A press on a **link**, and a press on a page nobody can type into, make no
/// record at all: neither of them has a caret to place, so neither has anything
/// to spend. The window says so by not building one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pressed {
    /// The byte of the file the pointer named, the grain a repeated press asks
    /// for, and whether the gesture extends what is already standing.
    Byte {
        offset: usize,
        grain: Grain,
        extend: bool,
    },
    /// No byte at all: the page's empty ground.
    Ground,
}

/// **What the release does with the record** — the same three answers the press
/// used to give itself, given one gesture later.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Spend {
    /// Put the caret in the page: at `offset`, grown to `grain`, keeping the
    /// standing anchor when `extend`, and then drawn out to `head` when the hand
    /// travelled before it let go.
    ///
    /// `head` is `None` for a gesture that never left the press's own six pixels
    /// — a click — and for a gesture that let go where the page has no byte to
    /// name.
    Caret {
        offset: usize,
        grain: Grain,
        extend: bool,
        head: Option<usize>,
    },
    /// Take the caret out of the page and draw every block rendered again.
    Ground,
}

impl Pressed {
    /// **A press that named a byte**, with the one thing the two properties owe
    /// each other settled here rather than at the release.
    ///
    /// **A shift-press is an extension and has its own grain already** (§7.31
    /// ⑦'s rule, carried onto the caret by T5 ①): it reaches from the standing
    /// anchor to where the pointer is, so the word or the paragraph a repeat
    /// count would have asked for is not what is being asked for. Normalised
    /// into the value so that neither the release nor a second caller can spell
    /// the combination that has no meaning.
    #[must_use]
    pub fn byte(offset: usize, grain: Grain, extend: bool) -> Self {
        Self::Byte {
            offset,
            grain: if extend { Grain::Character } else { grain },
            extend,
        }
    }

    /// **The record, spent** — the whole of the ruling, as a function.
    ///
    /// `travelled` is the six-pixel latch's own answer (`DragLatch::begun`), so
    /// the split between a click and a drag is the very one the link is answered
    /// by: a press that held still is a click, a press that travelled is a
    /// selection, and this makes no second judgement about which.
    ///
    /// `head` is where the button came up, in the file's own bytes. A click
    /// spends none — it has not drawn anything out — which is what keeps a
    /// double click's word from being taken and then immediately collapsed by
    /// the release landing a byte away from the press.
    #[must_use]
    pub fn spend(self, travelled: bool, head: Option<usize>) -> Spend {
        match self {
            Self::Byte {
                offset,
                grain,
                extend,
            } => Spend::Caret {
                offset,
                grain,
                extend,
                head: if travelled { head } else { None },
            },
            Self::Ground => Spend::Ground,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview_edit::EditCaret;
    use crate::preview_live::{CaretSeat, caret_seat};
    use crate::preview_select::{word_end, word_start};
    use std::ops::Range;

    /// The page the owner's report is about: a heading and two paragraphs, with
    /// a blank line between each pair.
    const PAGE: &str = "# Title\n\nfirst paragraph\n\nsecond paragraph\n";

    /// Its blocks, at the ranges [`crate::preview::parse_markdown_ranged`] gives
    /// them — each one covering its own last line's ending (§7.1.3o).
    fn ranges() -> Vec<Range<usize>> {
        vec![0..8, 9..25, 26..43]
    }

    /// **A live rendered page, as much of one as this rule can be held
    /// against**: the caret standing in it, and whether one is standing at all.
    ///
    /// Those two are the whole of what decides which block is drawn as source
    /// ([`caret_seat`], §7.1.3q), which is what the owner's report is about —
    /// so a gesture that leaves them alone is a page that does not change shape,
    /// whatever else the window is doing.
    struct Page {
        caret: EditCaret,
        /// `PreviewPane::md_caret`: whether anybody has entered this page.
        entered: bool,
        /// The gesture in flight — what the press recorded, and the last byte
        /// the hand has reached.
        flight: Option<(Pressed, Option<usize>)>,
        /// `PreviewTextDrag::seated`: whether the press was answered where it
        /// stood, so that this gesture is already extending the caret.
        seated: bool,
    }

    impl Page {
        /// A page being read: rendered end to end, with no caret in it.
        fn read() -> Self {
            Self {
                caret: EditCaret::default(),
                entered: false,
                flight: None,
                seated: false,
            }
        }

        /// **Which block this page draws as source**, and `None` when it draws
        /// none — the one rule, asked of the caret and of nothing else.
        fn source_block(&self) -> Option<usize> {
            self.entered
                .then(|| caret_seat(PAGE, &ranges(), self.caret.caret))
                .and_then(CaretSeat::block)
        }

        /// The caret the page is drawing from, which a page nobody has entered
        /// does not have.
        fn drawing_from(&self) -> Option<usize> {
            self.entered.then_some(self.caret.caret)
        }

        /// The button going down on a byte of the file — and answered here and
        /// now when it keeps the seat, exactly as the window's own press does.
        fn press(&mut self, pressed: Pressed) {
            self.flight = Some((pressed, None));
            self.seated = match pressed {
                Pressed::Byte { offset, .. } => {
                    keeps_the_seat(PAGE, &ranges(), self.drawing_from(), offset)
                }
                Pressed::Ground => false,
            };
            if self.seated {
                self.spend(pressed.spend(false, None));
            }
        }

        /// The hand moving with the button down, over another byte.
        ///
        /// **The caret follows it while the seat does not change, and no
        /// further**: past the edge of the seat it stops, because a page may not
        /// change face under a gesture that has not let go.
        fn drag_to(&mut self, offset: usize) {
            let Some((_, reached)) = self.flight.as_mut() else {
                return;
            };
            *reached = Some(offset);
            if self.seated && keeps_the_seat(PAGE, &ranges(), self.drawing_from(), offset) {
                self.caret.place(PAGE, offset, true);
            }
        }

        /// The button coming up — the record spent whatever the press already
        /// did with it, which is what keeps every gesture's end state one
        /// function of the record.
        fn release(&mut self, travelled: bool) {
            let Some((pressed, reached)) = self.flight.take() else {
                return;
            };
            self.seated = false;
            self.spend(pressed.spend(travelled, reached));
        }

        /// The window's own [`Spend`], through the same functions it obeys it
        /// with ([`EditCaret::place`], [`word_start`]/[`word_end`]).
        fn spend(&mut self, spend: Spend) {
            match spend {
                Spend::Ground => self.entered = false,
                Spend::Caret {
                    offset,
                    grain,
                    extend,
                    head,
                } => {
                    self.caret.place(PAGE, offset, extend && self.entered);
                    self.entered = true;
                    if grain == Grain::Word {
                        self.caret.anchor = word_start(PAGE, self.caret.caret);
                        self.caret.caret = word_end(PAGE, self.caret.caret);
                    }
                    if let Some(head) = head {
                        self.caret.place(PAGE, head, true);
                    }
                }
            }
        }

        /// What the caret model would put on the clipboard — and, while a
        /// gesture is in flight, what it is drawing a band over.
        fn selected(&self) -> &'static str {
            &PAGE[self.caret.range()]
        }
    }

    /// RED — **(a) a press inside a live page turns no block into source while
    /// the button is down** (owner's report 2026-09-21).
    ///
    /// The whole of the report: the paragraph under the pointer used to put its
    /// marks back and re-flow the instant the button went down. Nothing the
    /// press does can be seen, because the only thing the page's face is a
    /// function of is where the caret is, and the press does not move it.
    ///
    /// MUTATION: let [`Page::press`] spend its own record — which is what the
    /// window did before this ruling — and the second assertion goes red.
    #[test]
    fn a_press_turns_no_block_into_source_while_the_button_is_down() {
        let mut page = Page::read();
        assert_eq!(
            page.source_block(),
            None,
            "a page being read draws no source"
        );
        page.press(Pressed::byte(12, Grain::Character, false));
        assert_eq!(
            page.source_block(),
            None,
            "the press moved the caret, so the block under the pointer re-flowed \
             under a hand that has not let go",
        );
        assert!(
            page.caret.range().is_empty(),
            "and the caret model has nothing to copy while a gesture is in \
             flight, so what a copy answers with is the rendered selection the \
             gesture is drawing",
        );
    }

    /// RED — **(b) a click that never travelled seats the caret at the release**,
    /// which is the moment the link standing in the same prose would have
    /// answered.
    #[test]
    fn a_click_enters_the_block_it_landed_in_when_the_button_comes_up() {
        let mut page = Page::read();
        page.press(Pressed::byte(12, Grain::Character, false));
        page.release(false);
        assert_eq!(page.caret.caret, 12, "the byte the press named");
        assert_eq!(
            page.source_block(),
            Some(1),
            "and only now is its block drawn as source",
        );
        assert!(page.selected().is_empty(), "a click takes no text");
    }

    /// RED — **(c) a drag across two blocks leaves the page rendered until the
    /// button comes up**, and then takes the run of the file it was drawn over.
    ///
    /// MUTATION: spend the record on every [`Page::drag_to`] — the caret drag
    /// this ruling replaced — and the page changes face twice under the hand
    /// before it has let go.
    #[test]
    fn a_drag_across_two_blocks_leaves_the_page_rendered_until_it_ends() {
        let mut page = Page::read();
        page.press(Pressed::byte(12, Grain::Character, false));
        page.drag_to(20);
        assert_eq!(
            page.source_block(),
            None,
            "a block turned to source mid-drag"
        );
        page.drag_to(32);
        assert_eq!(
            page.source_block(),
            None,
            "the block the drag reached turned to source under the pointer",
        );
        page.release(true);
        assert_eq!(
            page.selected(),
            "st paragraph\n\nsecond",
            "the run drawn over"
        );
        assert_eq!(
            page.source_block(),
            Some(2),
            "and the caret's own block is source once the gesture is over",
        );
    }

    /// RED — **(d) a double click resolves its word at the release**, grain and
    /// all.
    ///
    /// The grain is the press's — a repeat count is a property of the press and
    /// of nothing else — and it is carried rather than acted on, which is what
    /// keeps `word` meaning the same thing a gesture later.
    #[test]
    fn a_double_click_takes_its_word_when_the_button_comes_up() {
        let mut page = Page::read();
        page.press(Pressed::byte(12, Grain::Word, false));
        assert_eq!(page.source_block(), None, "still nothing while it is down");
        page.release(false);
        assert_eq!(page.selected(), "first", "the word the press landed in");
        assert_eq!(page.source_block(), Some(1));
    }

    /// **A shift-press is an extension and has no grain of its own**, so the two
    /// cannot be spelled together at the release.
    #[test]
    fn a_shift_press_keeps_the_anchor_and_drops_the_repeat_count() {
        let mut page = Page::read();
        page.press(Pressed::byte(12, Grain::Character, false));
        page.release(false);
        page.press(Pressed::byte(35, Grain::Word, true));
        assert_eq!(
            Pressed::byte(35, Grain::Word, true),
            Pressed::Byte {
                offset: 35,
                grain: Grain::Character,
                extend: true,
            },
            "a repeated shift-press would otherwise take a word and a stretch",
        );
        page.release(false);
        assert_eq!(page.selected(), "st paragraph\n\nsecond pa");
        assert_eq!(page.source_block(), Some(2));
    }

    /// RED — **a press inside the seat the page is already drawing is answered
    /// where it stands, and its drag draws as it goes** (closure review of this
    /// ruling, 2026-09-21).
    ///
    /// The ordinary editing gesture: drag across the words you are about to
    /// replace, inside the paragraph you are already editing. Nothing about the
    /// page's face can change — the caret never leaves the seat — so there is
    /// nothing to wait for, and waiting would mean a hand selecting text with no
    /// highlight under it until it let go.
    ///
    /// MUTATION: make [`Page::press`] defer this one too (drop the
    /// [`keeps_the_seat`] arm) and the band is empty for the whole gesture,
    /// which is the regression this test was written against.
    #[test]
    fn a_press_inside_the_standing_seat_is_answered_where_it_stands() {
        let mut page = Page::read();
        page.press(Pressed::byte(12, Grain::Character, false));
        page.release(false);
        assert_eq!(page.source_block(), Some(1), "the page has been entered");
        // And now the second gesture, inside the block that is already open.
        page.press(Pressed::byte(10, Grain::Character, false));
        assert_eq!(page.caret.caret, 10, "the caret did not follow the press");
        page.drag_to(20);
        assert_eq!(
            page.selected(),
            "irst parag",
            "no band is drawn while the hand is selecting the words it is editing",
        );
        assert_eq!(
            page.source_block(),
            Some(1),
            "and the face is the one it was: the caret never left the seat",
        );
        page.release(true);
        assert_eq!(page.selected(), "irst parag", "the end state is the drag's");
        assert_eq!(page.source_block(), Some(1));
    }

    /// RED — **a drag out of the seat freezes the caret until the release**
    /// (closure review, 2026-09-21).
    ///
    /// The other half of the same sentence: the caret may move while the seat is
    /// unchanged, **and no further**. A caret that went on following the hand
    /// would take the source block with it, which is the half of the owner's
    /// report about a page changing shape under a selection being drawn.
    #[test]
    fn a_drag_out_of_the_seat_freezes_the_caret_until_the_release() {
        let mut page = Page::read();
        page.press(Pressed::byte(12, Grain::Character, false));
        page.release(false);
        page.press(Pressed::byte(10, Grain::Character, false));
        page.drag_to(20);
        page.drag_to(35);
        assert_eq!(
            page.caret.caret, 20,
            "the caret followed the hand out of its own seat",
        );
        assert_eq!(
            page.source_block(),
            Some(1),
            "so the paragraph under the pointer re-flowed mid-gesture",
        );
        page.release(true);
        assert_eq!(
            page.selected(),
            "irst paragraph\n\nsecond pa",
            "and the release reaches the byte the hand let go over",
        );
        assert_eq!(page.source_block(), Some(2));
    }

    /// **A repeated press inside the standing seat takes its word at once**,
    /// which is what the seat rule costs and what it is worth.
    ///
    /// The first click of a double click enters the page, so the second press is
    /// inside the seat and is answered where it stands. Nothing changes face —
    /// the word is inside the block that is already open — and the reader sees
    /// the word the moment the second press lands, as every editor does.
    #[test]
    fn a_double_click_inside_the_standing_seat_takes_its_word_at_once() {
        let mut page = Page::read();
        page.press(Pressed::byte(12, Grain::Character, false));
        page.release(false);
        page.press(Pressed::byte(12, Grain::Word, false));
        assert_eq!(
            page.selected(),
            "first",
            "the word is not taken until later"
        );
        assert_eq!(page.source_block(), Some(1));
        page.release(false);
        assert_eq!(page.selected(), "first", "and the release says the same");
    }

    /// **A press that named no byte of the file is the page's empty ground**,
    /// and it too is spent when the gesture ends.
    #[test]
    fn a_press_on_the_ground_renders_the_page_again_at_the_release() {
        let mut page = Page::read();
        page.press(Pressed::byte(12, Grain::Character, false));
        page.release(false);
        assert_eq!(page.source_block(), Some(1));
        page.press(Pressed::Ground);
        assert_eq!(
            page.source_block(),
            Some(1),
            "the page changed shape while the button was still down",
        );
        page.release(false);
        assert_eq!(
            page.source_block(),
            None,
            "and it renders again at the release"
        );
    }

    /// **A click spends no head**, whatever the pointer was over when the button
    /// came up.
    ///
    /// The release is a byte away from the press on any hand that is not a
    /// machine's, and a click that drew itself out to that byte would select a
    /// character nobody asked for — and would collapse the word a double click
    /// had just taken.
    #[test]
    fn a_click_spends_no_head_and_a_drag_spends_the_one_it_reached() {
        let pressed = Pressed::byte(12, Grain::Word, false);
        assert_eq!(
            pressed.spend(false, Some(13)),
            Spend::Caret {
                offset: 12,
                grain: Grain::Word,
                extend: false,
                head: None,
            },
        );
        assert_eq!(
            pressed.spend(true, Some(13)),
            Spend::Caret {
                offset: 12,
                grain: Grain::Word,
                extend: false,
                head: Some(13),
            },
        );
        assert_eq!(
            pressed.spend(true, None),
            Spend::Caret {
                offset: 12,
                grain: Grain::Word,
                extend: false,
                head: None,
            },
            "a hand that let go where the page has no byte leaves the caret \
             where the press named it",
        );
        assert_eq!(Pressed::Ground.spend(true, Some(13)), Spend::Ground);
    }
}
