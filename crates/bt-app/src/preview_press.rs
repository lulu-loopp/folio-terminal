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
//! **Pure, in [`crate::preview_live`]'s style**: no window, no pane and no
//! pointer. Which byte a point names is the window's question and is asked
//! before a value of either type is made; everything after that is a function of
//! the record and of whether the hand travelled.

use crate::preview_select::Grain;

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
    }

    impl Page {
        /// A page being read: rendered end to end, with no caret in it.
        fn read() -> Self {
            Self {
                caret: EditCaret::default(),
                entered: false,
                flight: None,
            }
        }

        /// **Which block this page draws as source**, and `None` when it draws
        /// none — the one rule, asked of the caret and of nothing else.
        fn source_block(&self) -> Option<usize> {
            self.entered
                .then(|| caret_seat(PAGE, &ranges(), self.caret.caret))
                .and_then(CaretSeat::block)
        }

        /// The button going down on a byte of the file.
        fn press(&mut self, pressed: Pressed) {
            self.flight = Some((pressed, None));
        }

        /// The hand moving with the button down, over another byte.
        fn drag_to(&mut self, offset: usize) {
            if let Some((_, reached)) = self.flight.as_mut() {
                *reached = Some(offset);
            }
        }

        /// The button coming up — the window's own release, as [`Spend`]
        /// dictates it and through the same two functions the window obeys it
        /// with ([`EditCaret::place`], [`word_start`]/[`word_end`]).
        fn release(&mut self, travelled: bool) {
            let Some((pressed, reached)) = self.flight.take() else {
                return;
            };
            match pressed.spend(travelled, reached) {
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

        /// What the caret model would put on the clipboard.
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
