//! The window's own tooltip: one host, any anchor.
//!
//! `title=` is the operating system's box, and the operating system's box obeys
//! nothing this window decides — not the theme, not the type, not the corner
//! radius, and it appears on its own schedule wherever the platform feels like
//! putting it (mock-up 1199-1206). So the tip is ours, drawn in the same
//! material as every other popup, and this module is the whole of it.
//!
//! Three pieces, deliberately apart:
//!
//! * [`TooltipAnchors`] — what is tippable *this frame*. Rebuilt from live
//!   geometry every time the chrome is, so a tip can never show a string the
//!   thing under the pointer stopped meaning.
//! * [`TooltipHost`] — the singleton state: which anchor is settling, which one
//!   is showing, and the two clocks that move between them. It knows nothing
//!   about tabs.
//! * [`layout`] and [`build`] — the box, placed and painted.
//!
//! The split is what lets a button that does not exist yet get a tooltip by
//! pushing one line into an anchor list.

use std::time::{Duration, Instant};

use bt_render::{ChromeLabel, ChromeLabelWeight, ChromePalette, OverlayQuad};
use bt_term::ProgressState;

use crate::marks::OverlayLayer;
use crate::settings::push_float_window;
use crate::{EASE, Motion, cubic_bezier};

/// How long the pointer must rest on a chrome anchor before its tip appears
/// (mock-up 8716).
///
/// This really is the 300-500ms case the guidance is about, and what it guards
/// against is *transit*: the window's controls are strung along the paths a
/// pointer takes to somewhere else, so the wait is what tells a hand that was
/// only passing from a hand that is asking. A tip is content laid over what you
/// are reading, and a false positive costs you the view.
///
/// Not every face is passed over — [`PEEK_INTENT_DELAY`] is the other story, and
/// the two constants are written side by side because they are one decision seen
/// twice. (The tab strip's hover-peek flyout is a third clock again, and lives
/// with the flyout in [`crate::peek_strip`].)
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(380);

/// How long the pointer must rest on a command tick before its glance card
/// appears (user report, 2026-08-19).
///
/// Shorter than [`TOOLTIP_DELAY`] because the two faces answer different pointer
/// stories. A chrome control is *passed over*; a rail tick is *travelled to*. The
/// rail is a two-pixel-thick column standing on the pane's right edge — nothing
/// is on the way to it, and a hand only arrives there by leaving what it was
/// doing and crossing the pane on purpose. The question is asked by the journey,
/// so by the time the pointer lands the transit guard has nothing left to guard
/// and the rest of it is the waiting the report is about.
///
/// Not zero, though. The band answers the *nearest ordinal* rather than the tick
/// under the pixel, so a hand walking down the rail changes subject every few
/// pixels; with no settle at all the card would flicker through every command it
/// passed on the way to the one it wants. This is the length of that settle and
/// nothing else.
///
/// Neither of this window's other 120s: `cmdrail::TICK_OPACITY_TRANSITION` is how
/// long a tick takes to *change colour*, which is a transition and not a wait, and
/// `file_peek::PEEK_INTENT_MS` is 350 because a file row is a row in a list the
/// pointer runs down — the passed-over story again, wearing a peek.
pub const PEEK_INTENT_DELAY: Duration = Duration::from_millis(120);

/// `transition: opacity .09s ease` (mock-up 1220).
pub const TOOLTIP_FADE: Duration = Duration::from_millis(90);

/// `border-radius: 5px`.
pub const TIP_RADIUS_LOGICAL_PX: f32 = 5.0;
/// `border: 1px solid var(--border)`.
pub const TIP_BORDER_LOGICAL_PX: f32 = 1.0;
/// The `7px` of `padding: 3px 7px`.
pub const TIP_PADDING_X_LOGICAL_PX: f32 = 7.0;
/// The `3px` of `padding: 3px 7px`.
pub const TIP_PADDING_Y_LOGICAL_PX: f32 = 3.0;
/// `font-size: 11px`.
pub const TIP_FONT_LOGICAL_PX: f32 = 11.0;
/// The one number `showTip` uses for both jobs (mock-up 8698-8703): how far the
/// tip stands off its host, and how close it may come to the window's edge.
/// They are the same gap in the mock-up and stay one constant here, because the
/// day they differ is the day someone has to explain why.
pub const TIP_GAP_LOGICAL_PX: f32 = 6.0;
/// The widest a tip may grow before its text wraps (user report, 2026-08-16).
///
/// The mock-up's `.tip` has no width bound because nothing in the mock-up's data
/// ever needed one: a tab's name, a formula tool's verb. A commit row's tip is
/// its whole subject line, and a subject can be a paragraph — the report shows
/// one running the full width of a 2.7k-pixel window as a single line. Menus and
/// float windows in this house are 260–430 wide; a tip that wraps at 360 reads
/// as a paragraph of about sixty characters, which is the measure prose is set
/// to everywhere else, and never as a banner.
pub const TIP_MAX_WIDTH_LOGICAL_PX: f32 = 360.0;

// ── `#cmd-peek`: the same machine wearing a second face (inventory D-19) ──
//
// *"the glance card: monospace one-liner, structurally non-interactive"* (mock
// 1483-1491). Every one of these is a `#cmd-peek` declaration and every one of
// them differs from `.tip` above, which is the whole reason [`TipFace`] exists:
// the delay, the fade, the anchor bookkeeping and the surface are the tip's and
// must not be written twice, while the type, the measure and the placement are
// the card's and are not the tip's in any of the four.

/// `font: 12px/1.5 Consolas, "Cascadia Mono", monospace` — the size.
///
/// A point larger than the chrome tip beside it, and set in the *terminal's* face
/// rather than the window's, because what it quotes is a command line: the card
/// shows a thing the reader typed at a grid, and a proportional rendering of it is
/// a paraphrase.
pub const PEEK_FONT_LOGICAL_PX: f32 = 12.0;
/// The `1.5` of the same declaration.
pub const PEEK_LINE_HEIGHT: f32 = 1.5;
/// The `10px` of `padding: 5px 10px`.
pub const PEEK_PADDING_X_LOGICAL_PX: f32 = 10.0;
/// The `5px` of `padding: 5px 10px`.
pub const PEEK_PADDING_Y_LOGICAL_PX: f32 = 5.0;
/// `border-radius: 8px` — the card is a *card*, rounded like the menus and the
/// float windows, where the tip's five is the smaller radius of a label.
pub const PEEK_RADIUS_LOGICAL_PX: f32 = 8.0;
/// `max-width: 460px; overflow: hidden; text-overflow: ellipsis; white-space:
/// nowrap` — one line, cut with an ellipsis, and never wider than this.
///
/// **Ellipsis and not wrapping**, which is the opposite of what the tip does and
/// deliberately so: a wrapped command line is a command line whose shape has been
/// changed, and the shape is half of what makes one recognisable. A cut one is
/// obviously cut.
pub const PEEK_MAX_WIDTH_LOGICAL_PX: f32 = 460.0;
/// What a cut line ends with.
pub const PEEK_ELLIPSIS: &str = "…";

// ── the colour swatch: the same machine wearing a third face (§7.1.6c-4c) ──
//
// A `#rrggbb` under the pointer in a text preview. Same clock, same anchors,
// same surface as the two above — the reuse D-19 ruled for `#cmd-peek`, taken at
// its word a second time rather than treated as a one-off.

/// The side of the colour well drawn before the text, in logical pixels.
///
/// Twenty-eight, which is the height of a `.btn` in this dialog and about two
/// lines of the card's own type: big enough that a dark colour and a slightly
/// darker one are told apart side by side, small enough that the card is still a
/// card and not a colour picker.
pub const SWATCH_SIZE_LOGICAL_PX: f32 = 28.0;
/// `border-radius: 6px` — the well is a rounded square, on the same ladder as the
/// button (6) and the card (8) it sits inside.
pub const SWATCH_RADIUS_LOGICAL_PX: f32 = 6.0;
/// The gap between the well and the token it is showing.
pub const SWATCH_GAP_LOGICAL_PX: f32 = 8.0;

/// Which of the faces a tip is drawn in.
///
/// D-19 ruled that `#cmd-peek` reuses this module rather than becoming a second
/// popup, and this is the shape of that reuse: one enum carrying the seven things
/// that differ — type, leading, padding, radius, width bound, whether the text
/// wraps or is cut, and how long the pointer must hold still — so that everything
/// which does *not* differ (the fade, [`TooltipAnchors`], [`TooltipHost`], the
/// surface, the shadow) has exactly one implementation.
///
/// **The delay was on the shared side of that list until 2026-08-19**, when a user
/// reported the rail's card as slow and it turned out to be the shared 380ms doing
/// exactly what it was written to do, in the one place there was nothing to guard
/// against. The sentence that put it there was not quite true: what the faces
/// share is *one clock* — one host, one countdown, one subject at a time — and
/// that is the thing D-19 was protecting, because two clocks over one pointer is
/// how a window ends up showing two boxes at once. **How long** that one countdown
/// runs is a separate question, and the faces answer it differently because they
/// answer different pointer stories: a chrome control is passed over, a rail tick
/// is travelled to. [`TOOLTIP_DELAY`] and [`PEEK_INTENT_DELAY`] carry the two
/// halves of that argument; [`Self::intent_delay`] is where they meet. A second
/// popup would still have meant a second clock, and this does not add one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TipFace {
    /// `.tip` — the window talking about its own controls.
    Chrome,
    /// `#cmd-peek` — the glance card beside a command tick.
    Peek {
        /// The text is the ledger's honest gap rather than a command, so it is
        /// drawn in `--ink3` instead of `--ink2` (`cmdrail::peek_empty_text`).
        muted: bool,
    },
    /// The colour under the pointer in a text preview ([`crate::hex_peek`]).
    Swatch {
        /// The colour the token spells, straight (non-premultiplied) RGBA. Part
        /// of the face and not of the text, because it is what is *drawn* — the
        /// text is the token, which the caller reads out of the document like
        /// every other tip's words.
        rgba: [u8; 4],
    },
}

impl TipFace {
    /// How long the pointer must hold still on this face's anchor before its box
    /// is due.
    ///
    /// The one place either number is read. [`TooltipHost`] arms exactly one
    /// deadline, in exactly one line, and asks this for its length — so the day a
    /// fourth face arrives it answers here and nothing downstream learns a new
    /// word.
    ///
    /// The swatch keeps the chrome tip's wait, and that is not an oversight: a
    /// `#rrggbb` in a preview is a run of six characters in the middle of a
    /// document the pointer crosses on its way to anywhere, which is the
    /// passed-over story exactly. It shares the card's *type* because it quotes a
    /// document, and the tip's *clock* because it is stumbled upon.
    #[must_use]
    pub fn intent_delay(self) -> Duration {
        match self {
            Self::Chrome | Self::Swatch { .. } => TOOLTIP_DELAY,
            Self::Peek { .. } => PEEK_INTENT_DELAY,
        }
    }

    #[must_use]
    pub fn font_logical_px(self) -> f32 {
        match self {
            Self::Chrome => TIP_FONT_LOGICAL_PX,
            Self::Peek { .. } | Self::Swatch { .. } => PEEK_FONT_LOGICAL_PX,
        }
    }

    #[must_use]
    pub fn line_height(self) -> f32 {
        match self {
            Self::Chrome => CHROME_LINE_HEIGHT,
            Self::Peek { .. } => PEEK_LINE_HEIGHT,
            // **The swatch is the line box.** A well 28 logical pixels tall
            // inside a card sized to a 12/1.5 line would overflow the card by
            // ten pixels, so the leading is what gives way: one line, as tall as
            // the well beside it, with the token set on its centre. Derived
            // rather than written as 2.333 so that moving either number keeps
            // the two the same height.
            Self::Swatch { .. } => SWATCH_SIZE_LOGICAL_PX / PEEK_FONT_LOGICAL_PX,
        }
    }

    /// `(horizontal, vertical)` padding, logical pixels.
    #[must_use]
    pub fn padding_logical_px(self) -> (f32, f32) {
        match self {
            Self::Chrome => (TIP_PADDING_X_LOGICAL_PX, TIP_PADDING_Y_LOGICAL_PX),
            Self::Peek { .. } | Self::Swatch { .. } => {
                (PEEK_PADDING_X_LOGICAL_PX, PEEK_PADDING_Y_LOGICAL_PX)
            }
        }
    }

    #[must_use]
    pub fn radius_logical_px(self) -> f32 {
        match self {
            Self::Chrome => TIP_RADIUS_LOGICAL_PX,
            Self::Peek { .. } | Self::Swatch { .. } => PEEK_RADIUS_LOGICAL_PX,
        }
    }

    #[must_use]
    pub fn max_width_logical_px(self) -> f32 {
        match self {
            Self::Chrome => TIP_MAX_WIDTH_LOGICAL_PX,
            Self::Peek { .. } | Self::Swatch { .. } => PEEK_MAX_WIDTH_LOGICAL_PX,
        }
    }

    /// Whether text too wide for the bound is broken onto more lines, or cut with
    /// an ellipsis.
    #[must_use]
    pub fn wraps(self) -> bool {
        matches!(self, Self::Chrome)
    }

    /// Whether the text is set in the terminal's face.
    ///
    /// A `#rrggbb` is a token out of a document the pane draws in the terminal's
    /// face, and quoting it in the window's sans face would be a paraphrase —
    /// the same sentence [`PEEK_FONT_LOGICAL_PX`] makes about a command line.
    #[must_use]
    pub fn monospace(self) -> bool {
        matches!(self, Self::Peek { .. } | Self::Swatch { .. })
    }

    /// How much room stands to the left of the text inside the box.
    ///
    /// Zero for a face that is only words. The swatch's own side plus its gap,
    /// for the one that is not — and it is a property of the *face* rather than
    /// something the caller adds to its measured widths, because otherwise the
    /// box would grow while the text stayed at the padding and the well would be
    /// drawn on top of it.
    #[must_use]
    pub fn leading_logical_px(self) -> f32 {
        match self {
            Self::Chrome | Self::Peek { .. } => 0.0,
            Self::Swatch { .. } => SWATCH_SIZE_LOGICAL_PX + SWATCH_GAP_LOGICAL_PX,
        }
    }

    /// Whether this face is placed by the *card's* rule — left of its host and
    /// eight pixels above it — rather than the tip's.
    ///
    /// Split out of [`Self::monospace`], which used to answer both questions
    /// because for two faces the answers coincided. They do not for the third: a
    /// swatch quotes a token in the terminal's face as the card does, but its
    /// host is a run of six characters in the middle of a document, and a card
    /// hung to its left would cover the line it is about. It is placed like a
    /// tip, which is what "beside the thing it explains" means when the thing is
    /// small.
    #[must_use]
    pub fn placed_like_card(self) -> bool {
        matches!(self, Self::Peek { .. })
    }
}

/// Which anchor a tip belongs to.
///
/// Identity only — never the text. The text is recomputed from the anchor every
/// frame (mock-up's own `el.title = tabTip(w)` on every paint), so a tip that is
/// already up follows its subject: rename the tab under an open tip and the tip
/// says the new name on the next frame.
///
/// Deliberately *not* [`crate::seats::ChromeTarget`]. That enum answers "what
/// does a click here do", and the two questions have different shapes: the tab's
/// mark is a tip of its own while a command runs (D38) but has never been a
/// click target, and the `×` is a click target the mock-up gives no tip at all.
/// Folding them together would have meant teaching the press, drag and cursor
/// machinery about a target that exists only to be hovered.
/// Which of a page's head verbs an anchor is for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebNavTool {
    Back,
    Forward,
    Reload,
    DevTools,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TooltipAnchorId {
    /// The tab body — index into the strip, matching `ChromeTarget::Tab`.
    Tab(usize),
    /// The mark slot: the working icon, or the progress ring that replaces it.
    TabIcon(usize),
    /// The pin, in the `×`'s own slot.
    TabPin(usize),
    /// The folder trigger — "Peek files here" (H108), the same words on both
    /// surfaces because it is the same action in both.
    TabFiles(usize),
    NewTab,
    NewTabMenu,
    /// A row of the open profile picker — a profile the machine cannot start, or
    /// a Recent row whose caption is only the last segment of its path.
    ///
    /// It carries [`crate::profiles::MenuRow`] rather than a bare index for the
    /// reason that type exists at all: the picker shows two lists, and a number
    /// that could mean either names the wrong row silently.
    ProfileRow(crate::profiles::MenuRow),
    /// A row of the open root menu, whose caption is only the last segment of
    /// the folder it offers (E53) — so the tip is where the whole path is said.
    RootRow(crate::profiles::RootMenuRow),
    /// One row of a Git page — a heading's teaching sentence, a changed file's
    /// full path, a commit's author (R16 keeps it out of the row and here).
    GitRow(bt_layout::SeatId, usize),
    /// One of the masthead's ahead/behind pills.
    ///
    /// Its own id rather than a part of the masthead row, because R5's sentence
    /// is about *that count*: a tip anchored to the whole heading would say "2
    /// commits ahead" while the pointer was on the behind pill.
    GitPill(bt_layout::SeatId, usize),
    /// One of a Git row's hover verbs. Its own id, and pushed before the row it
    /// sits in, because the two say different things about the same pixels: the
    /// row says which file, the button says what pressing it does — and for the
    /// `×` that difference is *restore* versus *delete*.
    GitAct(bt_layout::SeatId, usize, crate::git_panel::GitAct),
    /// One of the commit graph toolbar's controls (T1, v2 (3)).
    ///
    /// Three of the four are marks with no words beside them, which is exactly
    /// the case a tip exists for: a chevron, a cross and a circular arrow are
    /// idioms, and an idiom is a guess until something says what it does.
    ///
    /// Addressed by **surface** and not by seat, because the graph it belongs to
    /// is a document and a document can be torn off into a window — the tools
    /// are the same three marks there, and an idiom does not stop being one
    /// because it is in a float.
    GitGraphTool(crate::PreviewSurface, crate::git_graph::GraphTool),
    /// A pane head's `⌄` — the one control in a pane head with no word beside
    /// it and more than one thing behind it (user ruling, 2026-08-16).
    ///
    /// The head's other two controls do not register: a folder and a `×` are
    /// idioms this product has taught elsewhere, while a chevron says only "there
    /// is a list here" and never what is on it. It is exactly the case a tip is
    /// for.
    PaneChevron(bt_layout::SeatId),
    /// The 🗀 beside a **lone** pane's corner ghost (user proposal, Claude 认可
    /// 2026-08-25) — "Peek files here", [`Self::TabFiles`]'s own words, because
    /// it is that action again.
    ///
    /// **Only the corner's**, and the clause above is why: the head's folder
    /// still does not register, because it stands in a run inside a head that is
    /// itself saying what the pane is. This one floats bare over the terminal's
    /// output with nothing around it — which is the case the chevron beside it
    /// registers for, and the sentence about idioms taught elsewhere does not
    /// reach a mark with no surface under it.
    PaneFiles(bt_layout::SeatId),
    /// A preview head's `↗` — the hand-off arrow a page's seat wears (user
    /// ruling 2026-08-20).
    ///
    /// **It registers** for [`Self::PaneChevron`]'s own reason rather than in
    /// spite of it: a `×`, a folder and a floppy disk are idioms this product
    /// has taught elsewhere, and so is a framed arrow leaving a box — which is
    /// exactly the trouble. The framed one is drawn immediately to this button's
    /// right and means "this pane leaves the tree"; a *bare* arrow beside it has
    /// to say which of the two leavings it is, and nothing in the picture can.
    ///
    /// (This doc said "a `×`, a folder, **a pin** and a floppy disk" until
    /// 2026-08-23, and the pin in that list was the whole of what §7.7 ⑧ went on
    /// to overturn: the sentence was true of the tab's pin and the switcher
    /// row's, and the preview head's control was resting on their tuition while
    /// meaning something else. It is a padlock now and it registers too — see
    /// [`Self::PreviewLock`].)
    PreviewBrowser(bt_layout::SeatId),
    /// A preview head's padlock — "do not reuse this pane" (§7.7 ⑧, Claude 定
    /// 2026-08-23).
    ///
    /// **The second preview tool that registers, and the ruling is why.** The
    /// exemption above — a mark this product has taught elsewhere needs no tip —
    /// covered this control only while it was drawn as a pin, and it covered it
    /// wrongly: what the pin taught is a tab's and a row's "this one stays in
    /// the list", which is not what this button does. Given its own glyph it is
    /// the one mark in that head with nothing behind it, so it is exactly the
    /// case a tip is for.
    ///
    /// The tip changes with the state, on [`Self::PreviewWebNav`]'s reload/stop
    /// precedent: the button changes, so a single word for both would be the
    /// head describing what it was a press ago.
    PreviewLock(bt_layout::SeatId),
    /// **A page's three navigation buttons and its developer tools** (§7.7 ②,
    /// W2 slice ④), and every one of them registers where the hand-off arrow
    /// above says why one has to: a `<` and a `>` in a 22px box are the
    /// submenu's own arrow turned, a circular arrow is this window's refresh,
    /// and `</>` is what markdown's `Edit source` wears — four glyphs a reader
    /// has met here meaning something else. The reload's tip changes with the
    /// button, because the button changes into a stop.
    PreviewWebNav(bt_layout::SeatId, WebNavTool),
    /// **One control of a preview's address or breadcrumb row** (user ruling
    /// 2026-08-25 — 「预览头与地址行/面包屑行的新控件全部挂 tooltip」).
    ///
    /// The five that row grew when the 2026-08-24 ruling built it, and every one
    /// of them registers for the reason the four above do: `⧉`, `</>` and `⌄`
    /// are marks with no word beside them, and two of the three are marks this
    /// window has already taught meaning something else one row up. The other
    /// two are the row's own inventions — a breadcrumb segment, whose word is
    /// only the last piece of the place it names, and the `…`, whose entire
    /// meaning is *there is something here you cannot see*.
    ///
    /// It carries [`crate::seats::PreviewRailTip`], whose segment arm is a depth
    /// into the *whole* path rather than an index into the drawn run — see that
    /// type — so a tip already up follows its folder through a re-fold instead of
    /// quietly starting to name a different one.
    ///
    /// **Addressed by surface and not by seat** (§7.7 ⑩ 欠账, 2026-08-25), which
    /// is [`Self::GitGraphTool`]'s own reason arriving one row down: this band
    /// belongs to a *document*, a document can be torn off into a window, and a
    /// `⧉` does not stop needing a word for what it copies because the pane it
    /// was in became a window.
    PreviewRail(crate::PreviewSurface, crate::seats::PreviewRailTip),
    /// One tick of a pane's command marks rail — *"hover **glances** the command"*
    /// (mock 4604).
    ///
    /// It carries the tick's **subject** and not the tick's index, and the
    /// difference is the fisheye: a bucket opening under the pointer renumbers
    /// every tick below it, so an index would have the card follow the pointer's
    /// *position* through a re-layout while the hand had not moved. A
    /// `CommandMarkId` survives aggregation, expansion, eviction and resize, which
    /// is exactly the set of things that can happen underneath a card that is
    /// already up.
    ///
    /// A [`crate::cmdrail::Target`] rather than a bare mark since S4: while a
    /// search is open the same rail also carries matched lines, and a card over a
    /// match has to be a different card from the one over the command beside it.
    ///
    /// There is deliberately **no plain tip on a tick beside this**. "Jump to this
    /// command" would be a second box explaining a first one; the card *is* the
    /// tip, and what it says is the command — or, while the results rail is up,
    /// the line.
    CommandTick(bt_layout::SeatId, crate::cmdrail::Target),
    /// One control of the in-pane search capsule (§7.1.5d, B66).
    ///
    /// It carries the *element* and not a seat, because there is one capsule in
    /// the window: the singleton the prototype's own state and lookup both
    /// declare (mock 8515-8519) means a second identifier would have exactly one
    /// value. Six of the capsule's nine children register — the three
    /// two-letter toggles, the two chevrons and the cross — and the field does
    /// not, because a box you type into has its own placeholder to say what it
    /// is for.
    SearchControl(crate::search::SearchElement),
    /// A `#rrggbb` under the pointer in a text preview (§7.1.6c-4c).
    ///
    /// It carries the token's **byte offset in the document** and not its screen
    /// box, so that a card already up follows its token through a scroll or a
    /// reflow instead of being retired and re-armed — the same reasoning
    /// [`Self::CommandTick`] gives for carrying a mark rather than an index. An
    /// edit that moves the token does change the offset, and that is correct:
    /// the text under the pointer is then a different piece of text.
    PreviewHex(crate::PreviewSurface, usize),
    Settings,
    /// `.panel-toggle` — the rail's fold-away button, which the vertical layout
    /// puts at the far left of the title bar.
    PanelToggle,
    Minimize,
    Maximize,
    CloseWindow,
}

/// One tippable box.
#[derive(Clone, Debug, PartialEq)]
pub struct TooltipAnchor {
    pub id: TooltipAnchorId,
    /// `[left, top, right, bottom]`, physical pixels of the whole surface.
    pub rect: [f32; 4],
    /// What the tip says. Never empty — see [`TooltipAnchors::push`].
    pub text: String,
    /// Which face this anchor's tip is drawn in.
    ///
    /// On the anchor rather than derived from the id at paint time, because
    /// [`TipFace::Peek`]'s muteness is a fact about the *text* — whether the
    /// ledger had a command to quote — and the id knows only which mark it is.
    /// Deriving it would mean the painter re-asking the ledger a question the
    /// anchor already answered, one frame later than it answered it.
    pub face: TipFace,
}

/// Everything tippable this frame, innermost first.
///
/// The mock-up resolves an anchor with `target.closest("[title], [data-tip]")`
/// and takes the *nearest* one up the tree, so a control inside a tab answers
/// before the tab does. A flat list in innermost-first order is that rule
/// without a tree: push the children, then the thing they sit in, and the first
/// box that contains the pointer wins.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TooltipAnchors {
    entries: Vec<TooltipAnchor>,
}

impl TooltipAnchors {
    /// Register an anchor — unless it has nothing to say, in which case it is not
    /// an anchor at all (M141).
    ///
    /// "No text" and "empty text" are the same thing here, and that is the whole
    /// mechanism behind the mock-up's most careful line: `paintStrip` *removes*
    /// the icon's `title` when the command stops running rather than setting it
    /// to `""` (mock-up 4341-4343), because an empty tip would not merely draw an
    /// empty box — it would stop the pointer from ever reaching the tab
    /// underneath. Refusing to register is what lets the idle mark fall through
    /// to its tab.
    pub fn push(&mut self, id: TooltipAnchorId, rect: [f32; 4], text: impl Into<String>) {
        self.push_faced(id, rect, text, TipFace::Chrome);
    }

    /// The same, in a face of its own — the glance card's one caller.
    pub fn push_faced(
        &mut self,
        id: TooltipAnchorId,
        rect: [f32; 4],
        text: impl Into<String>,
        face: TipFace,
    ) {
        let text = text.into();
        if text.trim().is_empty() || rect[2] <= rect[0] || rect[3] <= rect[1] {
            return;
        }
        self.entries.push(TooltipAnchor {
            id,
            rect,
            text,
            face,
        });
    }

    /// The innermost anchor under this point.
    #[must_use]
    pub fn at(&self, x: f32, y: f32) -> Option<&TooltipAnchor> {
        self.entries.iter().find(|anchor| {
            x >= anchor.rect[0] && x < anchor.rect[2] && y >= anchor.rect[1] && y < anchor.rect[3]
        })
    }

    /// This frame's box and text for an anchor that is already showing.
    ///
    /// `None` when the anchor is gone — the tab closed, the strip scrolled it
    /// away, the command finished and its ring with it. A tip whose subject has
    /// left has nothing to say and is taken down.
    #[must_use]
    pub fn find(&self, id: TooltipAnchorId) -> Option<&TooltipAnchor> {
        self.entries.iter().find(|anchor| anchor.id == id)
    }
}

/// Which layer of the title stack a tab's displayed name actually came from
/// (mock-up 3010: `nameSource`).
///
/// Where a name came from is real information, and the mock-up spends exactly
/// one tooltip on it (4193-4196): a badge on every tab would spend permanent
/// pixels on a question you ask twice a month.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NameSource {
    /// You typed it.
    Manual,
    /// The program announced it (OSC 2).
    Program,
    /// It is the working folder's leaf (OSC 7).
    Cwd,
}

impl NameSource {
    /// The mock-up's own wording — `NAME_SOURCE`, line 3011. Not paraphrased:
    /// these three strings are the entire user-facing explanation of a system
    /// with four layers in it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => crate::i18n::Text::NameSourceManual.text(),
            Self::Program => crate::i18n::Text::NameSourceProgram.text(),
            Self::Cwd => crate::i18n::Text::NameSourceCwd.text(),
        }
    }
}

/// The one mark this window writes between a name and the place that name
/// belongs to.
///
/// Two readers, one glyph, and it is a constant rather than two literals because
/// they are deliberately the *same* punctuation saying the same thing: the tab
/// tip's `Working folder · C:\src` (M140) and the pane head's `vim main.rs ·
/// C:\src`, where a program has announced something and the head still has to
/// say where that something is standing. A head that separated its two halves
/// with a dash while the tip beside it used a middle dot would be two spellings
/// of one idea, and the eye reads punctuation as meaning.
pub const NAME_PLACE_SEPARATOR: &str = " · ";

/// A tab's tip: its name, where the name came from, and where it is standing
/// (M140, mock-up 4197-4201).
///
/// `source` is `None` when no layer won — a tab showing the profile's default
/// title has no provenance to report, and inventing one ("Working folder" for a
/// tab that has never announced a folder) would be the tip lying about the one
/// thing it exists to explain. With neither a source nor a folder the second
/// line is not written at all, which leaves a one-line tip saying the name. That
/// is M141's rule applied inside a string rather than across one.
#[must_use]
pub fn tab_tip(name: &str, source: Option<NameSource>, cwd: Option<&str>, pinned: bool) -> String {
    let mut tip = name.to_owned();
    let provenance = match (source, cwd) {
        (Some(source), Some(cwd)) => Some(format!("{}{NAME_PLACE_SEPARATOR}{cwd}", source.label())),
        (Some(source), None) => Some(source.label().to_owned()),
        (None, Some(cwd)) => Some(cwd.to_owned()),
        (None, None) => None,
    };
    if let Some(provenance) = provenance {
        tip.push('\n');
        tip.push_str(&provenance);
    }
    if pinned {
        // F46's wording, and it earns its own line: it is a fact about the tab's
        // future rather than about what it is showing now.
        tip.push_str(crate::i18n::Text::TabTipPinned.text());
    }
    tip
}

/// What the mark slot says while a command is running (D38, mock-up 4124-4128).
///
/// Empty when there is nothing to report, which is how the mark hands the
/// question back to the tab it sits on — see [`TooltipAnchors::push`]. The
/// mock-up arrives at the same place by removing the attribute (4341-4343).
#[must_use]
pub fn mark_tip(progress: Option<ProgressState>, working: bool) -> String {
    let Some(progress) = progress else {
        // `.ticon.working` carries `title="Working"` and nothing else — no
        // ellipsis, because this is a state and not a running commentary.
        return if working {
            crate::i18n::Text::MarkWorking.text().to_owned()
        } else {
            String::new()
        };
    };
    // `Math.max(0, Math.min(100, Math.round(p.pct || 0)))`: a reading outside the
    // scale is clamped rather than shown, and a kind that carries no reading at
    // all reports zero, exactly as `p.pct || 0` does.
    let percent = |value: Option<u8>| u32::from(value.unwrap_or(0)).min(100);
    match progress {
        // The one kind with no number to show: its arc has no length to mean
        // anything, so the tip says what is true instead.
        ProgressState::Indeterminate => crate::i18n::Text::MarkWorkingIndeterminate
            .text()
            .to_owned(),
        ProgressState::Normal(value) => crate::i18n::progress_percent(percent(Some(value))),
        ProgressState::Error(value) => crate::i18n::progress_error(percent(value)),
        ProgressState::Paused(value) => crate::i18n::progress_paused(percent(value)),
    }
}

/// The singleton: which anchor is settling, which is showing, and since when.
///
/// Modelled on `PeekHover`/`HyperlinkHover`, which already solve the same shape
/// — arm a clock on a subject, do not restart it while the subject holds still,
/// promote when it elapses. The difference is the fade: a tip that has appeared
/// keeps a second clock, because it owes frames for 90ms after it arrives.
///
/// Two states, each an anchor paired with the instant that governs it. Pairing
/// them is what makes the invariants structural rather than remembered: a
/// deadline with no subject and a subject with no deadline are both states this
/// host simply cannot be in, so no code has to defend against either — and an
/// unreachable defence is worse than none, because it quietly heals the bug a
/// test was written to catch.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TooltipHost {
    /// The anchor the pointer is resting on, and when its tip comes due.
    settling: Option<(TooltipAnchorId, Instant)>,
    /// The anchor whose tip is on screen, and when it appeared — the fade's own
    /// epoch.
    showing: Option<(TooltipAnchorId, Instant)>,
}

impl TooltipHost {
    /// Track the anchor under the pointer and the face its tip would wear. `None`
    /// means "nothing tippable", and every suppression the design asks for is
    /// spelled as `None` by the caller: a drag in flight, the anchor that owns an
    /// open menu (I94), the tab being renamed. Returns whether anything visible
    /// changed.
    ///
    /// The face comes in with the anchor because this is the only line in the
    /// window that arms a deadline, and since 2026-08-19 the length of that
    /// deadline is the face's own ([`TipFace::intent_delay`]). One host, one
    /// countdown, one subject — the face decides how long, not how many.
    ///
    /// Resting on the anchor that is *already* showing is not a new subject and
    /// must not re-arm anything — otherwise a hand that trembles on a button
    /// takes the tip down and puts it back up a delay later, forever. Sameness is
    /// asked of the *anchor* and never of the pair: a card whose ledger just
    /// handed it a real command where it had only the honest gap changes face
    /// without the pointer having moved, and that must not restart anything
    /// either.
    pub fn observe(&mut self, anchor: Option<(TooltipAnchorId, TipFace)>, now: Instant) -> bool {
        let subject = anchor.map(|(id, _)| id);
        if subject.is_some() && subject == self.active() {
            self.settling = None;
            return false;
        }
        // M142: the pointer left the host, so the tip goes at once and the timer
        // goes with it. Not a fade-out — the mock-up's `.tip` transitions on the
        // way in and simply loses `.show` on the way out.
        let hidden = self.showing.take().is_some();
        match anchor {
            Some((id, face)) => {
                if self.settling.map(|(settling, _)| settling) != Some(id) {
                    self.settling = Some((id, now + face.intent_delay()));
                }
            }
            None => self.settling = None,
        }
        hidden
    }

    /// Promote a candidate whose delay has elapsed. Returns whether it did.
    pub fn activate_if_due(&mut self, now: Instant) -> bool {
        let Some((anchor, due)) = self.settling else {
            return false;
        };
        if now < due {
            return false;
        }
        self.settling = None;
        self.showing = Some((anchor, now));
        true
    }

    /// Forget whatever this host is pointing at that no longer exists. Returns
    /// whether a *visible* tip was taken down.
    ///
    /// Both states, not just the showing one, and that is the whole point: a tab
    /// closed during the wait would otherwise leave a candidate that matures into
    /// a tip with no anchor — a box that cannot be laid out, cannot be painted,
    /// and cannot stop asking for the frame it will never manage to draw. "The thing it describes is still there" is one condition and it
    /// applies to a tip that is coming as much as to one that has arrived.
    pub fn retain(&mut self, exists: impl Fn(TooltipAnchorId) -> bool) -> bool {
        if self.settling.is_some_and(|(id, _)| !exists(id)) {
            self.settling = None;
        }
        if self.showing.is_some_and(|(id, _)| !exists(id)) {
            return self.showing.take().is_some();
        }
        false
    }

    /// Take the tip down and disarm the clock — any press, the window losing
    /// focus, a menu opening (M142, I94). Returns whether anything was visible.
    pub fn hide(&mut self) -> bool {
        self.settling = None;
        self.showing.take().is_some()
    }

    /// The anchor whose tip is on screen.
    #[must_use]
    pub fn active(&self) -> Option<TooltipAnchorId> {
        self.showing.map(|(anchor, _)| anchor)
    }

    /// The next instant this host has something to do: the settle deadline while
    /// one is armed, the next frame of the fade while one is running.
    ///
    /// Handed to the loop's `earliest_deadline`, so a window with a tip settling
    /// wakes exactly when it is due and a window without one costs nothing.
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
            .is_some_and(|(_, shown)| now.duration_since(shown) < TOOLTIP_FADE)
    }

    /// How solid the tip is drawn this frame — `opacity 0 -> 1` over
    /// [`TOOLTIP_FADE`] on the mock-up's own `ease`.
    ///
    /// Reduced motion gets the end state immediately. The mock-up's own
    /// reduced-motion block does not name `.tip`, but every other transition in
    /// this window stands down when the system asks for stillness, and a tip is
    /// the one popup you summon by *not moving* — a fade-in is exactly the kind
    /// of unrequested motion the preference is about.
    #[must_use]
    pub fn opacity(&self, now: Instant, motion: Motion) -> f32 {
        let Some((_, shown)) = self.showing else {
            return 0.0;
        };
        if motion == Motion::Reduced {
            return 1.0;
        }
        let elapsed = now.duration_since(shown).as_secs_f32();
        let progress = (elapsed / TOOLTIP_FADE.as_secs_f32()).clamp(0.0, 1.0);
        cubic_bezier(progress, EASE)
    }
}

/// A placed tip: the box, and the row each line of text sits in.
#[derive(Clone, Debug, PartialEq)]
pub struct TooltipLayout {
    /// `[left, top, right, bottom]`, physical pixels.
    pub frame: [f32; 4],
    /// One row per line, in order, each the full inner width.
    pub lines: Vec<([f32; 4], String)>,
}

/// Place the tip against its host (M139).
///
/// `line_widths` is the measured width of each line — only the font knows how
/// wide a string is, so the caller measures and this decides.
///
/// Horizontal: centred on the host, then clamped so neither edge comes within
/// the gap of the window's. Vertical: below the host by the gap, flipping above
/// when the bottom would not clear the window's own margin.
///
/// The mock-up's arithmetic (8701-8702) has no second guard for a tip that fits
/// neither above nor below, because no tip of its could be that tall. A wrapped
/// tip can be (user report, 2026-08-16), so a third case now holds such a tip
/// inside the window, covering its host if it must.
///
/// # The card's placement is a different rule and lives in the same door
///
/// `#cmd-peek` stands to the **left** of its tick and eight pixels above it
/// (mock 8474-8476), which is not "centred below" with different numbers — it is
/// another rule, and it has to be, because its host is on the pane's right edge
/// where a centred card would hang half off the window and a card below would
/// cover the tick under it. Both live here so that the day a third surface asks
/// for a tip there is one place to read what "placed" means.
#[must_use]
pub fn place(
    host: [f32; 4],
    line_widths: &[f32],
    window: (f32, f32),
    scale: f32,
    face: TipFace,
) -> Option<([f32; 4], f32, f32)> {
    if line_widths.is_empty() {
        return None;
    }
    let px = |logical: f32| logical * scale;
    let (pad_x, pad_y) = face.padding_logical_px();
    let (pad_x, pad_y) = (px(pad_x), px(pad_y));
    let border = px(TIP_BORDER_LOGICAL_PX);
    let gap = px(TIP_GAP_LOGICAL_PX);
    let line_height = (px(face.font_logical_px()) * face.line_height()).round();

    let text_width = line_widths.iter().copied().fold(0.0_f32, f32::max);
    let width = (text_width + px(face.leading_logical_px()) + 2.0 * (pad_x + border)).round();
    let height = (line_widths.len() as f32 * line_height + 2.0 * (pad_y + border)).round();

    let (window_width, window_height) = window;
    if face.placed_like_card() {
        // `left = max(6, r.left − peekWidth − 12)`, `top = max(6, r.top − 8)`.
        // The mock-up clamps only the near edges, because its card can only ever
        // be pushed off the top-left; the far edges are clamped here as well, for
        // a rail on a pane narrow enough that 460 pixels of card do not fit
        // between it and the window's left edge.
        let left = (host[0] - px(crate::cmdrail::PEEK_GAP_LOGICAL_PX) - width)
            .min(window_width - width - gap)
            .max(gap)
            .round();
        let top = (host[1] - px(crate::cmdrail::PEEK_RISE_LOGICAL_PX))
            .min(window_height - height - gap)
            .max(gap)
            .round();
        return Some(([left, top, left + width, top + height], line_height, border));
    }
    let centred = (host[0] + host[2]) / 2.0 - width / 2.0;
    let left = centred.min(window_width - width - gap).max(gap).round();

    let below = host[3] + gap;
    let above = host[1] - height - gap;
    let top = if below + height <= window_height - gap {
        below
    } else if above >= gap {
        above
    } else {
        // Neither side has room — reachable now that a tip can be a paragraph
        // (a wrapped commit subject over a row near the bottom of a short
        // window). Below by preference, held inside the window: it may cover
        // its host, and that is better than the half of it that would otherwise
        // stand off the screen.
        below.min(window_height - gap - height).max(gap)
    }
    .round();

    Some(([left, top, left + width, top + height], line_height, border))
}

/// The line box every other piece of chrome text is laid out in — see
/// `shape_chrome_labels`, which sizes a label's buffer to `font_size * 1.4`.
///
/// The mock-up leaves `.tip` on the document's inherited `line-height: normal`,
/// which is the *face's* own metric and therefore not a number the mock-up
/// states. Borrowing the one this renderer already uses everywhere is what keeps
/// a two-line tip's rows agreeing with every single-line label beside it; a
/// third number invented here would only be a guess at what Segoe happens to
/// report.
const CHROME_LINE_HEIGHT: f32 = 1.4;

/// Break a tip's text into the lines it will be drawn as.
///
/// `white-space: pre-line`, plus the width bound the mock-up did not have: every
/// `\n` in the text is a line break, and a line wider than `max_width` is broken
/// at spaces so no line exceeds it. A single word wider than the bound — a path,
/// a hash — is broken between characters instead, because a tip that ran off the
/// window to keep a word whole would be keeping the wrong promise.
///
/// `measure` is the font's answer to "how wide is this string", handed in for
/// the reason `place` takes measured widths: only the renderer knows. It is
/// asked once per word (and once for the space), never once per candidate line,
/// so a paragraph-long subject costs the number of words it has and not their
/// square. The line the caller then measures for `layout` may differ from the
/// sum by a kerning pair's worth, which is why the bound is checked here on the
/// sum and the *box* is sized on the measured line — the box always fits its
/// lines, and the lines never exceed the bound by more than that pair.
#[must_use]
pub fn wrap(text: &str, max_width: f32, mut measure: impl FnMut(&str) -> f32) -> Vec<String> {
    let space = measure(" ");
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        let mut line_width = 0.0_f32;
        for word in paragraph.split(' ') {
            let width = measure(word);
            if line.is_empty() {
                // The first word of a line always goes on it; if it is wider than
                // the bound on its own it is broken below, character by
                // character, and its tail becomes the line in hand.
                (line, line_width) = break_word(word, width, max_width, &mut measure, &mut lines);
                continue;
            }
            if line_width + space + width <= max_width {
                line.push(' ');
                line.push_str(word);
                line_width += space + width;
            } else {
                lines.push(std::mem::take(&mut line));
                (line, line_width) = break_word(word, width, max_width, &mut measure, &mut lines);
            }
        }
        lines.push(line);
    }
    lines
}

/// Put `word` at the start of a fresh line, breaking it between characters if it
/// is wider than the bound on its own. Whole lines cut off its front are pushed
/// to `lines`; what remains is returned as the line in hand and its width.
fn break_word(
    word: &str,
    width: f32,
    max_width: f32,
    measure: &mut impl FnMut(&str) -> f32,
    lines: &mut Vec<String>,
) -> (String, f32) {
    if width <= max_width || word.chars().count() < 2 {
        return (word.to_owned(), width);
    }
    let mut rest = word;
    loop {
        // The longest prefix that fits, and at least one character so the loop
        // always advances even when a single glyph is wider than the bound.
        let mut cut = 0;
        let mut cut_width = 0.0;
        // A word too wide for the line is nearly always a path or a URL, and a
        // path has joints: breaking *after* a separator keeps each segment
        // whole (`BetterTerminal/` then `.claude/`) where a break between
        // letters gives `BetterTermin` and `al/` — which the real machine
        // showed on the first refused checkout it drew. The joint that fits is
        // preferred; the character cut below is only for a segment that is
        // itself wider than the line.
        let mut joint = 0;
        let mut joint_width = 0.0;
        for (at, ch) in rest.char_indices() {
            let end = at + ch.len_utf8();
            let candidate = measure(&rest[..end]);
            if candidate > max_width && cut > 0 {
                break;
            }
            cut = end;
            cut_width = candidate;
            if is_word_joint(ch) && end < rest.len() {
                joint = end;
                joint_width = candidate;
            }
        }
        if cut >= rest.len() {
            return (rest.to_owned(), cut_width);
        }
        let (cut, _) = if joint > 0 {
            (joint, joint_width)
        } else {
            (cut, cut_width)
        };
        lines.push(rest[..cut].to_owned());
        rest = &rest[cut..];
    }
}

/// The characters after which an over-wide word may be broken without cutting
/// a segment in two: path separators, and the hyphen and underscore that
/// compound words and identifiers are joined with.
fn is_word_joint(ch: char) -> bool {
    matches!(ch, '/' | '\\' | '-' | '_')
}

/// Cut `text` to `max_width`, ending it with an ellipsis when it had to be cut.
///
/// `text-overflow: ellipsis` on a `white-space: nowrap` box, which is what the
/// glance card is. The cut lands on a **character** boundary and not a byte one,
/// and the ellipsis is measured as part of every candidate rather than added
/// afterwards — a cut made without it and then appended to overflows the box by
/// exactly the width of the mark that was supposed to prove it fits.
///
/// Binary search over the character boundaries, so a five-hundred-character
/// command line costs about ten shapings a frame rather than five hundred.
#[must_use]
pub fn ellipsize(text: &str, max_width: f32, mut measure: impl FnMut(&str) -> f32) -> String {
    if measure(text) <= max_width {
        return text.to_owned();
    }
    let cuts: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
    // The longest prefix whose ellipsised form still fits. `lo` is always a
    // fitting answer (the empty prefix, which is the bare ellipsis) and `hi` is
    // always past the last one, so the loop cannot fail to terminate on a face
    // whose widths are not monotonic in the string's length.
    let (mut lo, mut hi) = (0_usize, cuts.len() - 1);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        let candidate = format!("{}{PEEK_ELLIPSIS}", &text[..cuts[mid]]);
        if measure(&candidate) <= max_width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    format!("{}{PEEK_ELLIPSIS}", &text[..cuts[lo]])
}

/// Lay the tip out: the box, and one row per line.
///
/// The mock-up's `.tip` is `white-space: pre-line`: every `\n` is a line. The
/// width bound is applied before this — [`wrap`] or [`ellipsize`] has already
/// dealt with it, and the widths handed in are the resulting lines' — so this
/// splits on `\n` and nothing else, and shrink-wraps the box to the longest line.
#[must_use]
pub fn layout(
    text: &str,
    host: [f32; 4],
    line_widths: &[f32],
    window: (f32, f32),
    scale: f32,
    face: TipFace,
) -> Option<TooltipLayout> {
    let (frame, line_height, border) = place(host, line_widths, window, scale, face)?;
    let (pad_x, pad_y) = face.padding_logical_px();
    let pad_x = pad_x * scale;
    let pad_y = pad_y * scale;
    // The leading is asymmetric on purpose: it is a *column* before the text and
    // not padding around it, so it moves the left edge in and leaves the right
    // where the box's own padding put it. Adding it to `pad_x` would take the
    // same width off the right and give the text a box narrower than the one it
    // was measured for.
    let leading = face.leading_logical_px() * scale;
    let lines = text
        .split('\n')
        .enumerate()
        .map(|(row, line)| {
            let top = frame[1] + border + pad_y + row as f32 * line_height;
            (
                [
                    frame[0] + border + pad_x + leading,
                    top,
                    frame[2] - border - pad_x,
                    top + line_height,
                ],
                line.to_owned(),
            )
        })
        .collect();
    Some(TooltipLayout { frame, lines })
}

/// Paint the tip — one layer, always the last one handed to the renderer.
///
/// `z-index: 60` against the menu's `30` (mock-up 1207 and the note at 7339):
/// the tip is the only thing in this window that is *never* covered, because it
/// is the only thing that exists to explain what is under it.
#[must_use]
pub fn build(
    layout: &TooltipLayout,
    palette: &ChromePalette,
    scale: f32,
    opacity: f32,
    face: TipFace,
) -> Vec<OverlayLayer> {
    let px = |logical: f32| logical * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let mut quads: Vec<OverlayQuad> = Vec::new();

    push_float_window(
        &mut quads,
        layout.frame,
        px(face.radius_logical_px()),
        px(TIP_BORDER_LOGICAL_PX),
        px(bt_render::FLOAT_WINDOW_SHADOW_LOGICAL_PX),
        palette.menu_surface,
        palette.menu_shadow,
        alpha(palette.tip_shadow_inner_alpha),
        alpha(palette.tip_shadow_outer_alpha),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
    );

    // **The colour well**, in the column [`TipFace::leading_logical_px`]
    // reserved for it: a rounded square of the colour, with the card's own
    // hairline round it.
    //
    // The hairline is not decoration. A swatch of `#1b1b1b` on a dark card and a
    // swatch of the card's own surface would otherwise have no edge at all, and
    // a card showing an invisible square is a card that looks broken rather than
    // one that has just told you the colour is the same as the paper.
    //
    // A colour that carries alpha is composited **over the card's surface** and
    // labelled by its own text, which spells the alpha out. The alternative —
    // a chequerboard — is a second convention to learn, and it would be showing
    // the colour over a pattern this window paints nowhere else.
    if let TipFace::Swatch { rgba } = face {
        let side = px(SWATCH_SIZE_LOGICAL_PX);
        let (pad_x, pad_y) = face.padding_logical_px();
        let border = px(TIP_BORDER_LOGICAL_PX);
        let left = layout.frame[0] + border + px(pad_x);
        let top = layout.frame[1] + border + px(pad_y);
        let well = [left, top, left + side, top + side];
        let radius = px(SWATCH_RADIUS_LOGICAL_PX);
        quads.extend(bt_render::rounded_overlay_fill(
            well,
            radius,
            [rgba[0], rgba[1], rgba[2]],
            f32::from(rgba[3]) / 255.0,
        ));
        quads.extend(bt_render::rounded_overlay_halo(
            well,
            radius,
            border,
            palette.menu_border,
            alpha(palette.menu_border_alpha),
        ));
    }

    // `color: var(--ink2)` over `--menu` — the same ink a menu row that is not
    // the selected one is drawn in, which is what `--ink2` on that surface
    // already means. The card's honest gap drops one step to `--ink3`, which is
    // the ink this palette already keeps for a menu row's annotation.
    let ink = match face {
        TipFace::Peek { muted: true } => palette.menu_item_hint_text,
        _ => palette.menu_item_text,
    };

    // **Monospace rides the body channel, not the label channel.** A
    // [`ChromeLabel`] is set in the window's sans face and has no say in the
    // matter; [`bt_render::PreviewRun`] is the pipeline's one run of text that can
    // ask for the terminal's face, and an [`OverlayLayer`]'s body is drawn inside
    // that layer rather than a pass earlier — so the card's own surface is still
    // underneath its text. Teaching `ChromeLabel` a face instead would have been a
    // field on a struct with a hundred literal construction sites, every one of
    // which would have had to say "sans" to go on meaning what it already means.
    if face.monospace() {
        let font_size_px = px(face.font_logical_px());
        let paragraphs = layout
            .lines
            .iter()
            .map(|(rect, text)| bt_render::PreviewParagraph {
                runs: vec![bt_render::PreviewRun {
                    text: text.clone(),
                    color: ink,
                    mono: true,
                    bold: false,
                    font_scale: 1.0,
                    inline_box_px: None,
                }],
                rect: *rect,
                font_size_px,
                line_height_px: rect[3] - rect[1],
                // `white-space: nowrap`: the text was cut to the bound by
                // [`ellipsize`] before it ever got here, and a reflow at this
                // point would be a second opinion about the same measurement.
                wrap: false,
                letter_spacing_em: 0.0,
                align_right: false,
                align_center: false,
            })
            .collect();
        return vec![OverlayLayer {
            quads,
            body: Some(bt_render::PreviewBody {
                clip: layout.frame,
                quads: Vec::new(),
                paragraphs,
                blocks: Vec::new(),
                rasters: Vec::new(),
            }),
            opacity,
            ..OverlayLayer::default()
        }];
    }

    let labels = layout
        .lines
        .iter()
        .map(|(rect, text)| ChromeLabel {
            mono: false,
            text: text.clone(),
            rect: *rect,
            font_size_px: px(face.font_logical_px()),
            color: ink,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        })
        .collect();

    vec![OverlayLayer {
        quads,
        labels,
        opacity,
        ..OverlayLayer::default()
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALE: f32 = 1.0;
    const WINDOW: (f32, f32) = (1000.0, 700.0);

    fn host(left: f32, top: f32, right: f32, bottom: f32) -> [f32; 4] {
        [left, top, right, bottom]
    }

    /// Every test below the card's own section is about `.tip`, so these two
    /// shadow the faced versions with the chrome face bound. Stated once here
    /// rather than repeated at fifty call sites, and the card's face is passed
    /// explicitly by the tests that are about it — which is how a reader can tell
    /// which face a given assertion is making a claim about.
    fn place(
        host: [f32; 4],
        line_widths: &[f32],
        window: (f32, f32),
        scale: f32,
    ) -> Option<([f32; 4], f32, f32)> {
        super::place(host, line_widths, window, scale, TipFace::Chrome)
    }

    fn layout(
        text: &str,
        host: [f32; 4],
        line_widths: &[f32],
        window: (f32, f32),
        scale: f32,
    ) -> Option<TooltipLayout> {
        super::layout(text, host, line_widths, window, scale, TipFace::Chrome)
    }

    fn build(
        layout: &TooltipLayout,
        palette: &ChromePalette,
        scale: f32,
        opacity: f32,
    ) -> Vec<OverlayLayer> {
        super::build(layout, palette, scale, opacity, TipFace::Chrome)
    }

    // ── M139: placement ────────────────────────────────────────────────────

    #[test]
    fn a_tip_centres_on_its_host_and_stands_six_pixels_below_it() {
        let anchor = host(400.0, 10.0, 460.0, 40.0);
        let (frame, ..) = place(anchor, &[50.0], WINDOW, SCALE).expect("a tip is placed");
        let tip_centre = (frame[0] + frame[2]) / 2.0;
        let host_centre = (anchor[0] + anchor[2]) / 2.0;
        assert!(
            (tip_centre - host_centre).abs() <= 0.5,
            "centred: {tip_centre} vs {host_centre}"
        );
        assert!((frame[1] - (anchor[3] + 6.0)).abs() < 0.001, "{}", frame[1]);
    }

    #[test]
    fn a_tip_near_an_edge_is_pushed_in_to_six_pixels_and_no_further() {
        // Hard against the left: centring would put it off-window.
        let (left_frame, ..) = place(host(0.0, 10.0, 20.0, 40.0), &[200.0], WINDOW, SCALE).unwrap();
        assert!((left_frame[0] - 6.0).abs() < 0.001, "{}", left_frame[0]);

        // Hard against the right — the caption buttons live here, so this is the
        // case every window-control tip actually takes.
        let (right_frame, ..) =
            place(host(980.0, 10.0, 1000.0, 40.0), &[200.0], WINDOW, SCALE).unwrap();
        assert!(
            (right_frame[2] - (WINDOW.0 - 6.0)).abs() < 0.001,
            "{}",
            right_frame[2]
        );
    }

    #[test]
    fn a_tip_with_no_room_below_flips_above_its_host() {
        let anchor = host(400.0, 660.0, 460.0, 690.0);
        let (frame, ..) = place(anchor, &[50.0], WINDOW, SCALE).expect("a tip is placed");
        assert!(
            (frame[3] - (anchor[1] - 6.0)).abs() < 0.001,
            "sits above: {frame:?}"
        );
    }

    // ── the width bound (user report, 2026-08-16) ──────────────────────────

    /// A measure where every character is ten wide, so widths are countable.
    fn ten_per_char(run: &str) -> f32 {
        run.chars().count() as f32 * 10.0
    }

    /// PIN — **a long line wraps at spaces and no line exceeds the bound; the
    /// text's own newlines are kept.** The commit subject the report shows was
    /// one line across a whole screen; this is that subject at toy scale.
    ///
    /// Mutation: have `wrap` return `text.split('\n')` and the width assertion
    /// goes red on the first line.
    #[test]
    fn a_long_line_wraps_at_spaces_and_a_newline_stays_a_newline() {
        let text = "the spinner stops taxing the whole house and the ring is held\nWeiyi Shi";
        let lines = wrap(text, 200.0, ten_per_char);
        for line in &lines {
            assert!(
                ten_per_char(line) <= 200.0,
                "no line is wider than the bound: {line:?}"
            );
            assert!(
                !line.starts_with(' ') && !line.ends_with(' '),
                "a break eats the space it broke at: {line:?}"
            );
        }
        assert_eq!(
            lines.last().map(String::as_str),
            Some("Weiyi Shi"),
            "the text's own newline is a line break, and the short second paragraph is its own line"
        );
        assert_eq!(
            lines.join(" ").replace(" Weiyi", "\nWeiyi"),
            text,
            "nothing is lost or reordered"
        );
        assert!(
            lines[0].split(' ').count() >= 3,
            "greedy: as many words as fit go on a line, not one per line: {lines:?}"
        );
    }

    /// PIN — a word wider than the bound on its own is broken between characters
    /// rather than left to run off the window, and a text that already fits is
    /// returned as it came.
    #[test]
    fn a_word_wider_than_the_bound_is_broken_and_a_short_text_is_untouched() {
        let hash = "0123456789abcdef0123456789abcdef";
        let lines = wrap(hash, 100.0, ten_per_char);
        assert_eq!(
            lines.len(),
            4,
            "thirty-two characters at ten wide over a hundred: {lines:?}"
        );
        assert!(lines.iter().all(|line| ten_per_char(line) <= 100.0));
        assert_eq!(
            lines.concat(),
            hash,
            "and every character is still there, in order"
        );

        // A path breaks at its joints, not between letters, when a joint fits.
        let path = "D:/Developer/BetterTerminal/.claude/worktrees/t1-tab-basics";
        let broken = wrap(path, 200.0, ten_per_char);
        assert!(broken.iter().all(|line| ten_per_char(line) <= 200.0));
        assert_eq!(broken.concat(), path);
        assert!(
            broken[..broken.len() - 1]
                .iter()
                .all(|line| line.ends_with('/') || line.ends_with('-')),
            "every line but the last ends at a joint: {broken:?}"
        );

        assert_eq!(
            wrap("main · C:/x\npinned", 400.0, ten_per_char),
            vec!["main · C:/x".to_owned(), "pinned".to_owned()],
            "a tip that fits is the tip that was written"
        );
        assert_eq!(wrap("", 400.0, ten_per_char), vec![String::new()]);
    }

    /// PIN — a tip that fits neither below nor above its host is held inside the
    /// window rather than pushed off it, and the ordinary flip is untouched.
    #[test]
    fn a_tip_too_tall_for_either_side_stays_inside_the_window() {
        // Twelve lines is taller than a 300-pixel window has on either side of a
        // host in its middle, and shorter than the window itself.
        let widths = vec![50.0; 12];
        let short = (1000.0, 300.0);
        let (frame, ..) = place(host(400.0, 140.0, 460.0, 160.0), &widths, short, SCALE).unwrap();
        assert!(
            frame[1] >= TIP_GAP_LOGICAL_PX,
            "top inside the window: {frame:?}"
        );
        assert!(
            frame[3] <= short.1 - TIP_GAP_LOGICAL_PX + 0.5,
            "and its foot too, even if that means covering the host: {frame:?}"
        );
        // A tip that fits below still goes below; one that fits only above still
        // flips — the third case is reached only when both are refused.
        let (below, ..) = place(host(400.0, 10.0, 460.0, 40.0), &[50.0], WINDOW, SCALE).unwrap();
        assert!(below[1] > 40.0);
        let (above, ..) = place(host(400.0, 660.0, 460.0, 690.0), &[50.0], WINDOW, SCALE).unwrap();
        assert!(above[3] < 660.0);
    }

    /// The box grows with the number of lines, and the padding is spent on both
    /// sides of both axes. A tab's tip is two lines by design and a pinned tab's
    /// is three, so this is the common case and not the exotic one.
    #[test]
    fn the_box_wraps_its_longest_line_and_stacks_the_rest() {
        let anchor = host(400.0, 10.0, 460.0, 40.0);
        let (one, line_height, border) = place(anchor, &[50.0], WINDOW, SCALE).unwrap();
        let (two, ..) = place(anchor, &[50.0, 120.0], WINDOW, SCALE).unwrap();

        // Width answers to the widest line, never the first or the last.
        assert!((one[2] - one[0] - (50.0 + 2.0 * (7.0 + 1.0))).abs() < 1.0);
        assert!((two[2] - two[0] - (120.0 + 2.0 * (7.0 + 1.0))).abs() < 1.0);
        // Height answers to the count.
        assert!(((two[3] - two[1]) - (one[3] - one[1]) - line_height).abs() < 1.0);
        assert!((border - 1.0).abs() < 0.001);
    }

    #[test]
    fn every_line_gets_its_own_row_inside_the_padding() {
        let anchor = host(400.0, 10.0, 460.0, 40.0);
        let laid = layout("first\nsecond", anchor, &[40.0, 60.0], WINDOW, SCALE)
            .expect("a two-line tip is laid out");
        assert_eq!(laid.lines.len(), 2);
        assert_eq!(laid.lines[0].1, "first");
        assert_eq!(laid.lines[1].1, "second");
        // Rows stack without overlapping, and both stay inside the frame.
        assert!(laid.lines[0].0[3] <= laid.lines[1].0[1] + 0.001);
        assert!(laid.lines[0].0[1] >= laid.frame[1]);
        assert!(laid.lines[1].0[3] <= laid.frame[3] + 0.001);
        // The text box is inset by the border and the horizontal padding.
        assert!((laid.lines[0].0[0] - (laid.frame[0] + 1.0 + 7.0)).abs() < 0.001);
    }

    // ── M141: an anchor with nothing to say is not an anchor ────────────────

    #[test]
    fn an_anchor_with_no_text_is_never_registered_and_falls_through_to_its_parent() {
        let mut anchors = TooltipAnchors::default();
        // The idle mark, inside the tab: pushed first because it is innermost.
        anchors.push(TooltipAnchorId::TabIcon(0), [10.0, 0.0, 30.0, 40.0], "");
        anchors.push(
            TooltipAnchorId::Tab(0),
            [0.0, 0.0, 200.0, 40.0],
            "bash\nWorking folder · /tmp",
        );

        let hit = anchors.at(20.0, 20.0).expect("the pointer is on the mark");
        assert_eq!(
            hit.id,
            TooltipAnchorId::Tab(0),
            "an idle mark hands the question to its tab"
        );
    }

    #[test]
    fn a_child_with_something_to_say_answers_before_the_tab_it_sits_in() {
        let mut anchors = TooltipAnchors::default();
        anchors.push(TooltipAnchorId::TabIcon(0), [10.0, 0.0, 30.0, 40.0], "42%");
        anchors.push(
            TooltipAnchorId::Tab(0),
            [0.0, 0.0, 200.0, 40.0],
            "bash\nWorking folder · /tmp",
        );

        assert_eq!(
            anchors.at(20.0, 20.0).map(|a| a.id),
            Some(TooltipAnchorId::TabIcon(0))
        );
        // …and the tab still answers everywhere the child is not.
        assert_eq!(
            anchors.at(100.0, 20.0).map(|a| a.id),
            Some(TooltipAnchorId::Tab(0))
        );
    }

    #[test]
    fn a_blank_string_is_no_more_a_tip_than_an_empty_one() {
        let mut anchors = TooltipAnchors::default();
        anchors.push(TooltipAnchorId::Settings, [0.0, 0.0, 10.0, 10.0], "   ");
        assert_eq!(anchors.at(5.0, 5.0), None);
        assert_eq!(anchors.find(TooltipAnchorId::Settings), None);
    }

    // ── M140 / D38: what the strings say ───────────────────────────────────

    /// The mock-up's `tabTip` (4197-4201), line for line. These are the
    /// user-facing words and they are copied, not paraphrased.
    #[test]
    fn a_tabs_tip_names_it_then_says_where_the_name_came_from() {
        assert_eq!(
            tab_tip(
                "claude",
                Some(NameSource::Manual),
                Some("C:\\src\\app"),
                false
            ),
            "claude\nNamed by you · C:\\src\\app"
        );
        assert_eq!(
            tab_tip(
                "npm run dev",
                Some(NameSource::Program),
                Some("C:\\src"),
                false
            ),
            "npm run dev\nSet by the program · C:\\src"
        );
        assert_eq!(
            tab_tip("app", Some(NameSource::Cwd), Some("C:\\src\\app"), false),
            "app\nWorking folder · C:\\src\\app"
        );
    }

    /// F46's wording, on its own line, and only when it is true.
    #[test]
    fn a_pinned_tab_says_it_will_come_back() {
        let pinned = tab_tip("app", Some(NameSource::Cwd), Some("C:\\src"), true);
        assert_eq!(
            pinned,
            "app\nWorking folder · C:\\src\nPinned — restored next launch"
        );
        assert_eq!(pinned.lines().count(), 3);
        assert!(!tab_tip("app", Some(NameSource::Cwd), Some("C:\\src"), false).contains("Pinned"));
    }

    /// A tab wearing the profile's default title has no provenance, and the tip
    /// must not manufacture one. It says the name and stops.
    #[test]
    fn a_tab_with_nothing_to_report_says_only_its_name() {
        assert_eq!(tab_tip("PowerShell", None, None, false), "PowerShell");
        assert_eq!(
            tab_tip("PowerShell", None, None, true),
            "PowerShell\nPinned — restored next launch"
        );
        // A folder with no winning layer is still a place worth naming.
        assert_eq!(
            tab_tip("PowerShell", None, Some("C:\\src"), false),
            "PowerShell\nC:\\src"
        );
    }

    #[test]
    fn the_mark_reports_the_run_and_stays_silent_otherwise() {
        assert_eq!(mark_tip(None, false), "", "an idle mark is not an anchor");
        assert_eq!(mark_tip(None, true), "Working");
        assert_eq!(
            mark_tip(Some(ProgressState::Indeterminate), true),
            "Working…"
        );
        assert_eq!(mark_tip(Some(ProgressState::Normal(42)), true), "42%");
        assert_eq!(
            mark_tip(Some(ProgressState::Error(Some(80))), true),
            "80% — error"
        );
        assert_eq!(
            mark_tip(Some(ProgressState::Paused(Some(15))), true),
            "15% — paused"
        );
        // `p.pct || 0` — a kind that carries no reading still reports a number.
        assert_eq!(
            mark_tip(Some(ProgressState::Error(None)), true),
            "0% — error"
        );
        assert_eq!(
            mark_tip(Some(ProgressState::Paused(None)), true),
            "0% — paused"
        );
        // `Math.min(100, …)` — a shell that reports past the end of the scale.
        assert_eq!(mark_tip(Some(ProgressState::Normal(200)), true), "100%");
        // The ring outranks the breath: what you are pointing at is the ring.
        assert_eq!(mark_tip(Some(ProgressState::Normal(7)), false), "7%");
    }

    // ── M137 / M142: the two clocks ────────────────────────────────────────

    #[test]
    fn a_chrome_tip_waits_three_hundred_and_eighty_milliseconds_and_not_a_moment_less() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some((TooltipAnchorId::Settings, TipFace::Chrome)), start);

        assert!(!host.activate_if_due(start + Duration::from_millis(379)));
        assert_eq!(host.active(), None);
        assert!(host.activate_if_due(start + TOOLTIP_DELAY));
        assert_eq!(host.active(), Some(TooltipAnchorId::Settings));
    }

    #[test]
    fn resting_on_a_showing_tip_does_not_restart_its_clock() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some((TooltipAnchorId::Settings, TipFace::Chrome)), start);
        assert!(host.activate_if_due(start + TOOLTIP_DELAY));

        // A hand that trembles on the button reports the same anchor again.
        let changed = host.observe(
            Some((TooltipAnchorId::Settings, TipFace::Chrome)),
            start + TOOLTIP_DELAY,
        );
        assert!(!changed, "nothing changed");
        assert_eq!(
            host.active(),
            Some(TooltipAnchorId::Settings),
            "the tip stays up"
        );
        assert_eq!(
            host.deadline(
                start + TOOLTIP_DELAY,
                Motion::Reduced,
                Duration::from_millis(16)
            ),
            None
        );
    }

    #[test]
    fn moving_to_a_new_anchor_takes_the_old_tip_down_and_starts_over() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some((TooltipAnchorId::Settings, TipFace::Chrome)), start);
        assert!(host.activate_if_due(start + TOOLTIP_DELAY));

        let moved = start + TOOLTIP_DELAY + Duration::from_millis(1);
        assert!(host.observe(Some((TooltipAnchorId::Minimize, TipFace::Chrome)), moved));
        assert_eq!(host.active(), None, "the old tip is gone at once");
        // And the new one waits its own full delay rather than inheriting.
        assert!(!host.activate_if_due(moved + Duration::from_millis(379)));
        assert!(host.activate_if_due(moved + TOOLTIP_DELAY));
        assert_eq!(host.active(), Some(TooltipAnchorId::Minimize));
    }

    /// M142: leaving the host hides *and* disarms. Hiding without clearing the
    /// timer is the bug where the tip lands 380ms later over nothing.
    #[test]
    fn leaving_an_anchor_clears_the_timer_as_well_as_the_tip() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some((TooltipAnchorId::Settings, TipFace::Chrome)), start);
        host.observe(None, start + Duration::from_millis(10));

        // Asked *before* anything is polled: the clock has to be gone the moment
        // the pointer leaves, not merely gone by the time something notices. Poll
        // first and a host that quietly self-heals on the way past looks identical
        // to one that never armed.
        assert_eq!(
            host.deadline(start, Motion::Full, Duration::from_millis(16)),
            None,
            "a disarmed host asks for no wakeups"
        );
        assert!(!host.activate_if_due(start + Duration::from_secs(5)));
        assert_eq!(host.active(), None);
    }

    #[test]
    fn a_press_or_a_lost_window_takes_the_tip_down_immediately() {
        for settle in [false, true] {
            let mut host = TooltipHost::default();
            let start = Instant::now();
            host.observe(Some((TooltipAnchorId::Settings, TipFace::Chrome)), start);
            if settle {
                assert!(host.activate_if_due(start + TOOLTIP_DELAY));
            }
            assert_eq!(host.hide(), settle, "reports whether anything was visible");
            assert_eq!(host.active(), None);
            // And it does not come back on its own.
            assert!(!host.activate_if_due(start + Duration::from_secs(5)));
        }
    }

    /// A tab closed while its tip was counting down must take the countdown with
    /// it. Left behind, the candidate matures into a tip with no anchor: nothing
    /// to lay out, nothing to paint, and a frame debt that can never be settled.
    #[test]
    fn a_subject_that_leaves_takes_its_pending_tip_with_it() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some((TooltipAnchorId::Tab(3), TipFace::Chrome)), start);

        // The tab closes 100ms into the wait.
        assert!(
            !host.retain(|id| id != TooltipAnchorId::Tab(3)),
            "nothing was visible yet"
        );
        assert_eq!(
            host.deadline(start, Motion::Full, Duration::from_millis(16)),
            None
        );
        assert!(!host.activate_if_due(start + Duration::from_secs(5)));
        assert_eq!(host.active(), None);
    }

    #[test]
    fn a_subject_that_leaves_takes_its_showing_tip_with_it() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some((TooltipAnchorId::Tab(3), TipFace::Chrome)), start);
        assert!(host.activate_if_due(start + TOOLTIP_DELAY));

        assert!(
            host.retain(|id| id != TooltipAnchorId::Tab(3)),
            "a visible tip came down"
        );
        assert_eq!(host.active(), None);
        // And a subject that is still there is left entirely alone.
        host.observe(Some((TooltipAnchorId::Tab(1), TipFace::Chrome)), start);
        assert!(host.activate_if_due(start + TOOLTIP_DELAY));
        assert!(!host.retain(|_| true));
        assert_eq!(host.active(), Some(TooltipAnchorId::Tab(1)));
    }

    #[test]
    fn an_armed_host_asks_to_be_woken_exactly_when_the_delay_is_up() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some((TooltipAnchorId::Settings, TipFace::Chrome)), start);
        assert_eq!(
            host.deadline(start, Motion::Full, Duration::from_millis(16)),
            Some(start + TOOLTIP_DELAY)
        );
    }

    // ── M136: the fade ─────────────────────────────────────────────────────

    #[test]
    fn the_tip_fades_in_over_ninety_milliseconds_and_owes_frames_while_it_does() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some((TooltipAnchorId::Settings, TipFace::Chrome)), start);
        let shown = start + TOOLTIP_DELAY;
        assert!(host.activate_if_due(shown));

        assert!(
            (host.opacity(shown, Motion::Full) - 0.0).abs() < 0.001,
            "starts invisible"
        );
        let middle = host.opacity(shown + Duration::from_millis(45), Motion::Full);
        assert!(middle > 0.0 && middle < 1.0, "climbing: {middle}");
        assert!((host.opacity(shown + TOOLTIP_FADE, Motion::Full) - 1.0).abs() < 0.001);
        assert!((host.opacity(shown + Duration::from_secs(9), Motion::Full) - 1.0).abs() < 0.001);

        // While it climbs it owes the next frame; once landed it owes nothing.
        let frame = Duration::from_millis(16);
        assert!(host.is_fading(shown + Duration::from_millis(45), Motion::Full));
        assert_eq!(
            host.deadline(shown + Duration::from_millis(45), Motion::Full, frame),
            Some(shown + Duration::from_millis(45) + frame)
        );
        assert!(!host.is_fading(shown + TOOLTIP_FADE, Motion::Full));
        assert_eq!(
            host.deadline(shown + TOOLTIP_FADE, Motion::Full, frame),
            None
        );
    }

    /// It is `ease` and not a straight ramp — the mock-up names the keyword, and
    /// `ease` leaves quickly and arrives slowly.
    #[test]
    fn the_fade_follows_the_mockups_own_ease_curve() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some((TooltipAnchorId::Settings, TipFace::Chrome)), start);
        let shown = start + TOOLTIP_DELAY;
        host.activate_if_due(shown);
        let half = host.opacity(shown + TOOLTIP_FADE / 2, Motion::Full);
        assert!(
            half > 0.55,
            "ease is ahead of linear at the midpoint: {half}"
        );
    }

    #[test]
    fn stillness_skips_the_fade_and_owes_nothing() {
        let mut host = TooltipHost::default();
        let start = Instant::now();
        host.observe(Some((TooltipAnchorId::Settings, TipFace::Chrome)), start);
        let shown = start + TOOLTIP_DELAY;
        host.activate_if_due(shown);

        assert!(
            (host.opacity(shown, Motion::Reduced) - 1.0).abs() < 0.001,
            "there at once"
        );
        assert!(!host.is_fading(shown, Motion::Reduced));
        assert_eq!(
            host.deadline(shown, Motion::Reduced, Duration::from_millis(16)),
            None,
            "a still tip asks for no animation frames"
        );
    }

    #[test]
    fn a_host_with_nothing_showing_is_fully_transparent() {
        let host = TooltipHost::default();
        assert!((host.opacity(Instant::now(), Motion::Full) - 0.0).abs() < 0.001);
        assert!((host.opacity(Instant::now(), Motion::Reduced) - 0.0).abs() < 0.001);
    }

    // ── the painted layer ──────────────────────────────────────────────────

    #[test]
    fn the_tip_paints_one_layer_carrying_its_own_opacity_and_one_label_per_line() {
        let palette = bt_render::chrome_palette();
        let laid = layout(
            "bash\nWorking folder · /tmp",
            host(400.0, 10.0, 460.0, 40.0),
            &[40.0, 160.0],
            WINDOW,
            SCALE,
        )
        .unwrap();
        let layers = build(&laid, &palette, SCALE, 0.4);
        assert_eq!(layers.len(), 1, "a tip is one layer");
        let layer = &layers[0];
        assert!((layer.opacity - 0.4).abs() < 0.001);
        assert_eq!(layer.labels.len(), 2);
        assert_eq!(layer.labels[0].text, "bash");
        assert_eq!(layer.labels[1].text, "Working folder · /tmp");
        assert_eq!(layer.labels[0].font_size_px, 11.0);
        assert_eq!(layer.labels[0].color, palette.menu_item_text);
        assert!(layer.sprites.is_empty(), "a tip is words and a box");
        // Lift, hairline and face: the box reaches past its own frame on every
        // side, and its face is the menu's.
        assert!(
            layer.quads.iter().any(|quad| quad.rect[1] < laid.frame[1]),
            "lifted"
        );
        assert!(
            layer
                .quads
                .iter()
                .any(|quad| quad.color == palette.menu_surface),
            "faced"
        );
    }

    // ── the swatch: the third face (§7.1.6c-4c) ───────────────────────────

    /// PIN — **the swatch's card keeps a column for the colour and is exactly as
    /// tall as the well in it.**
    ///
    /// Both halves are one claim about the same number: the leading widens the
    /// box and moves the text, and the line box *is* the well, so a card that
    /// got either wrong would either overlap its own swatch or overflow it.
    ///
    /// MUTATION: return `0.0` from `leading_logical_px` and the text starts under
    /// the well; add the leading to `pad_x` instead of to the left edge alone and
    /// the text box comes out narrower than the string it was measured for.
    #[test]
    fn a_swatch_card_keeps_a_column_for_the_colour_and_is_as_tall_as_the_well() {
        let face = TipFace::Swatch {
            rgba: [0x1b, 0x1b, 0x1b, 0xff],
        };
        assert!(face.monospace(), "it quotes a token out of a document");
        assert!(!face.placed_like_card(), "and it is placed like a tip");
        assert_eq!(face.leading_logical_px(), 36.0, "28 of well, 8 of gap");
        assert_eq!(face.radius_logical_px(), PEEK_RADIUS_LOGICAL_PX);

        let token = host(400.0, 300.0, 449.0, 318.0);
        let text_width = 49.0_f32;
        let (frame, line_height, border) =
            super::place(token, &[text_width], WINDOW, SCALE, face).unwrap();
        assert_eq!(line_height, SWATCH_SIZE_LOGICAL_PX);
        assert_eq!(
            frame[3] - frame[1],
            (SWATCH_SIZE_LOGICAL_PX + 2.0 * (PEEK_PADDING_Y_LOGICAL_PX + border)).round(),
            "the box is the well plus its own padding, and nothing else"
        );
        assert_eq!(
            frame[2] - frame[0],
            (text_width + 36.0 + 2.0 * (PEEK_PADDING_X_LOGICAL_PX + border)).round(),
            "and wide enough for the well, the gap and the token"
        );
        assert_eq!(
            frame[1],
            token[3] + TIP_GAP_LOGICAL_PX,
            "below the token, the tip's own rule"
        );

        let laid = super::layout("#1b1b1b", token, &[text_width], WINDOW, SCALE, face)
            .expect("a swatch is placed");
        let (text_box, text) = &laid.lines[0];
        assert_eq!(text, "#1b1b1b");
        assert_eq!(
            text_box[0],
            frame[0] + border + PEEK_PADDING_X_LOGICAL_PX + 36.0,
            "the text starts after the well, not under it"
        );
        assert_eq!(
            text_box[2],
            frame[2] - border - PEEK_PADDING_X_LOGICAL_PX,
            "and still ends at the box's own padding"
        );
        assert!(text_box[2] - text_box[0] >= text_width);
    }

    /// PIN — **the well is painted in the colour the token spells**, with the
    /// card's own hairline round it so a swatch the colour of the card is still a
    /// square.
    ///
    /// MUTATION: drop the halo and `#1b1b1b` on a dark card is an invisible
    /// square — a card that looks broken rather than one saying "this colour is
    /// the colour of the paper".
    #[test]
    fn the_well_is_painted_in_the_token_s_own_colour_inside_the_card_s_hairline() {
        let palette = bt_render::chrome_palette();
        let colour = [0x7a, 0x99, 0xff, 0xff];
        let face = TipFace::Swatch { rgba: colour };
        let token = host(400.0, 300.0, 449.0, 318.0);
        let laid = super::layout("#7a99ff", token, &[49.0], WINDOW, SCALE, face).unwrap();
        let layers = super::build(&laid, &palette, SCALE, 1.0, face);
        let layer = &layers[0];

        let well: Vec<&bt_render::OverlayQuad> = layer
            .quads
            .iter()
            .filter(|quad| quad.color == [colour[0], colour[1], colour[2]])
            .collect();
        assert!(!well.is_empty(), "the colour itself is drawn");
        let left = well
            .iter()
            .fold(f32::MAX, |low, quad| low.min(quad.rect[0]));
        let right = well
            .iter()
            .fold(0.0_f32, |high, quad| high.max(quad.rect[2]));
        let top = well
            .iter()
            .fold(f32::MAX, |low, quad| low.min(quad.rect[1]));
        let bottom = well
            .iter()
            .fold(0.0_f32, |high, quad| high.max(quad.rect[3]));
        assert_eq!(right - left, SWATCH_SIZE_LOGICAL_PX, "a square, not a bar");
        assert_eq!(bottom - top, SWATCH_SIZE_LOGICAL_PX);
        assert_eq!(
            left,
            laid.frame[0] + TIP_BORDER_LOGICAL_PX + PEEK_PADDING_X_LOGICAL_PX,
            "in the column the leading reserved"
        );
        assert!(
            right <= laid.lines[0].0[0],
            "and clear of the text beside it"
        );
        assert!(
            layer
                .quads
                .iter()
                .any(|quad| quad.color == palette.menu_border && quad.rect[1] >= top - 2.0),
            "with a hairline of its own"
        );
        // The token still rides the monospace channel: this face is a card with a
        // square in it, not a second kind of popup.
        let body = layer.body.as_ref().expect("the monospace channel");
        assert_eq!(body.paragraphs[0].runs[0].text, "#7a99ff");
        assert!(body.paragraphs[0].runs[0].mono);
    }

    /// A colour that carries alpha is composited over the card's own surface,
    /// and the text is what says how much alpha there was.
    #[test]
    fn a_colour_with_alpha_is_laid_over_the_card_rather_than_over_a_chequerboard() {
        let palette = bt_render::chrome_palette();
        let face = TipFace::Swatch {
            rgba: [0x7a, 0x99, 0xff, 0x80],
        };
        let token = host(400.0, 300.0, 465.0, 318.0);
        let laid = super::layout("#7a99ff80", token, &[63.0], WINDOW, SCALE, face).unwrap();
        let layer = &super::build(&laid, &palette, SCALE, 1.0, face)[0];
        let inside: Vec<&bt_render::OverlayQuad> = layer
            .quads
            .iter()
            .filter(|quad| quad.color == [0x7a, 0x99, 0xff])
            .collect();
        assert!(!inside.is_empty());
        assert!(
            inside
                .iter()
                .all(|quad| quad.alpha <= 128.0 / 255.0 + f32::EPSILON),
            "no pixel of the well is more opaque than the colour says"
        );
        assert_eq!(
            layer.body.as_ref().unwrap().paragraphs[0].runs[0].text,
            "#7a99ff80"
        );
    }

    // ── `#cmd-peek`: the second face (D-19) ────────────────────────────────

    /// A monospace measure with one number in it: every character is `advance`
    /// wide, which is what a monospace face means and all these tests need.
    fn mono(advance: f32) -> impl FnMut(&str) -> f32 {
        move |text: &str| text.chars().count() as f32 * advance
    }

    /// The card's metrics are the card's, and none of them is the tip's.
    #[test]
    fn the_glance_cards_face_is_larger_rounder_and_wider_than_the_tips() {
        let peek = TipFace::Peek { muted: false };
        assert_eq!(peek.font_logical_px(), 12.0);
        assert_eq!(peek.line_height(), 1.5);
        assert_eq!(peek.padding_logical_px(), (10.0, 5.0));
        assert_eq!(peek.radius_logical_px(), 8.0);
        assert_eq!(peek.max_width_logical_px(), 460.0);
        assert!(peek.monospace());
        assert!(!peek.wraps(), "one line, cut — never reflowed");
        assert_eq!(TipFace::Chrome.font_logical_px(), TIP_FONT_LOGICAL_PX);
        assert!(TipFace::Chrome.wraps() && !TipFace::Chrome.monospace());
    }

    /// **The card stands to the left of its tick and eight pixels above it**, and
    /// is held inside the window on every side.
    ///
    /// MUTATION: place it below and centred like a tip, and every card the rail
    /// raises hangs half off the right edge of the window — the rail is *on* that
    /// edge, which is the whole reason this face has a placement of its own.
    #[test]
    fn the_glance_card_stands_to_the_left_of_its_tick_and_never_leaves_the_window() {
        let peek = TipFace::Peek { muted: false };
        // A tick on a pane's right edge, as the rail always is.
        let tick = host(960.0, 300.0, 987.0, 302.0);
        let (frame, line_height, _) = super::place(tick, &[200.0], WINDOW, SCALE, peek).unwrap();
        assert_eq!(frame[2], tick[0] - 12.0, "twelve pixels off the tick");
        assert_eq!(frame[1], tick[1] - 8.0, "eight pixels above it");
        assert_eq!(line_height, (12.0 * 1.5_f32).round());
        assert_eq!(
            frame[3] - frame[1],
            (line_height + 2.0 * (5.0 + 1.0)).round()
        );
        assert_eq!(
            frame[2] - frame[0],
            (200.0_f32 + 2.0 * (10.0 + 1.0)).round()
        );
        // Hard against the top of the window: the card is pushed in to six and no
        // further, which is the mock-up's own `max(6, …)`.
        let high =
            super::place(host(960.0, 2.0, 987.0, 4.0), &[200.0], WINDOW, SCALE, peek).unwrap();
        assert_eq!(high.0[1], 6.0);
        // And a card too wide for the room to the left of its tick — the mock-up
        // clamps only the near edge; the far one is clamped here too.
        let wide = super::place(
            host(80.0, 300.0, 107.0, 302.0),
            &[900.0],
            WINDOW,
            SCALE,
            peek,
        )
        .unwrap();
        assert_eq!(wide.0[0], 6.0);
        assert!(wide.0[2] <= WINDOW.0 - 6.0);
        // Near the bottom, the card is held above the window's own margin rather
        // than running off it.
        let low = super::place(
            host(960.0, 695.0, 987.0, 697.0),
            &[200.0],
            WINDOW,
            SCALE,
            peek,
        )
        .unwrap();
        assert!(low.0[3] <= WINDOW.1 - 6.0);
    }

    /// One line, cut at the bound, with the ellipsis measured as part of the cut.
    ///
    /// MUTATION: cut first and append the mark afterwards and every cut line is
    /// one ellipsis wider than the box that was supposed to prove it fits.
    #[test]
    fn a_command_too_long_for_the_card_is_cut_with_an_ellipsis_that_fits_inside_the_bound() {
        // 460 logical pixels at a seven-pixel advance is sixty-five characters,
        // one of which the ellipsis takes.
        let bound = TipFace::Peek { muted: false }.max_width_logical_px();
        let short = "cargo test --workspace";
        assert_eq!(ellipsize(short, bound, mono(7.0)), short, "it already fits");
        let long = "x".repeat(200);
        let cut = ellipsize(&long, bound, mono(7.0));
        assert!(cut.ends_with(PEEK_ELLIPSIS));
        assert!(
            mono(7.0)(&cut) <= bound,
            "the cut line fits its own box: {}",
            mono(7.0)(&cut)
        );
        assert_eq!(
            cut.chars().count(),
            65,
            "as many characters as fit, and no more"
        );
        // One more character would not have fitted — the cut is the *longest* one
        // that does, not merely one that does.
        let over = format!("{}{PEEK_ELLIPSIS}", &long[..cut.chars().count()]);
        assert!(mono(7.0)(&over) > bound);
        // Multi-byte text is cut on a character boundary and never inside one.
        let cjk = "回声 ".repeat(200);
        let cut = ellipsize(&cjk, bound, mono(7.0));
        assert!(cut.ends_with(PEEK_ELLIPSIS));
        assert!(mono(7.0)(&cut) <= bound);
        // A bound too small for anything at all still answers with something
        // drawable rather than panicking on an empty slice.
        assert_eq!(ellipsize(&long, 1.0, mono(7.0)), PEEK_ELLIPSIS);
    }

    /// The card's text rides the monospace channel, and its honest gap is drawn
    /// one ink lighter.
    ///
    /// MUTATION: put it in `labels` and the command is set in the window's sans
    /// face — a paraphrase of the thing the reader typed at a grid.
    #[test]
    fn the_glance_card_draws_its_line_in_the_terminals_own_face() {
        let palette = bt_render::chrome_palette();
        let peek = TipFace::Peek { muted: false };
        let laid = super::layout(
            "cargo test --workspace",
            host(960.0, 300.0, 987.0, 302.0),
            &[180.0],
            WINDOW,
            SCALE,
            peek,
        )
        .expect("a card is placed");
        let layers = super::build(&laid, &palette, SCALE, 1.0, peek);
        let layer = &layers[0];
        assert!(layer.labels.is_empty(), "not a chrome label");
        let body = layer.body.as_ref().expect("the monospace channel");
        assert_eq!(body.clip, laid.frame);
        assert_eq!(body.paragraphs.len(), 1);
        let paragraph = &body.paragraphs[0];
        assert!(!paragraph.wrap, "white-space: nowrap");
        assert_eq!(paragraph.font_size_px, 12.0);
        assert_eq!(paragraph.runs.len(), 1);
        assert!(paragraph.runs[0].mono);
        assert_eq!(paragraph.runs[0].text, "cargo test --workspace");
        assert_eq!(paragraph.runs[0].color, palette.menu_item_text);
        // The card is still a card: the menu's face, the menu's hairline, and a
        // radius of its own.
        assert!(
            layer
                .quads
                .iter()
                .any(|quad| quad.color == palette.menu_surface)
        );
        // The ledger's honest gap drops a step, to the ink a menu row's
        // annotation already wears.
        let muted = super::build(&laid, &palette, SCALE, 1.0, TipFace::Peek { muted: true });
        assert_eq!(
            muted[0].body.as_ref().unwrap().paragraphs[0].runs[0].color,
            palette.menu_item_hint_text
        );
        assert_ne!(palette.menu_item_hint_text, palette.menu_item_text);
    }

    /// The card is a tip: it is raised by this host and no other, and it fades in
    /// on the tip's own 90ms. Since 2026-08-19 the *wait* in front of that is its
    /// own, which is why the face is handed in rather than assumed.
    #[test]
    fn the_glance_card_is_raised_by_the_tips_host_and_takes_the_tips_own_fade() {
        let seat = bt_layout::SeatId(3);
        let anchor = TooltipAnchorId::CommandTick(
            seat,
            crate::cmdrail::Target::Command(bt_term::CommandMarkId(11)),
        );
        let face = TipFace::Peek { muted: false };
        let mut host = TooltipHost::default();
        let now = Instant::now();
        host.observe(Some((anchor, face)), now);
        assert_eq!(host.active(), None, "still settling");
        assert!(!host.activate_if_due(now + PEEK_INTENT_DELAY - Duration::from_millis(1)));
        assert!(host.activate_if_due(now + PEEK_INTENT_DELAY));
        assert_eq!(host.active(), Some(anchor));
        // And the fade is the tip's 90ms, not a number of the card's own.
        assert!(host.is_fading(now + PEEK_INTENT_DELAY, Motion::Full));
        assert!(!host.is_fading(now + PEEK_INTENT_DELAY + TOOLTIP_FADE, Motion::Full));
        // A tick that is no longer under the pointer takes its card with it —
        // which is how the rail's own `pointerleave` reaches the card.
        assert!(host.retain(|_| false));
        assert_eq!(host.active(), None);
    }

    /// The report of 2026-08-19, driven straight at the state machine: one
    /// observation, and the instant it comes due is the face's answer.
    ///
    /// The chrome tip's 380 is asserted in the same test rather than left to the
    /// ones above, because what was reported is a *difference* — a card that made
    /// the reader wait as long as a title bar button — and a regression that moved
    /// both numbers together would leave every other assertion in this file green.
    #[test]
    fn the_rail_card_is_due_at_a_hundred_and_twenty_while_the_chrome_tip_keeps_its_three_eighty() {
        assert_eq!(PEEK_INTENT_DELAY, Duration::from_millis(120));
        assert_eq!(TOOLTIP_DELAY, Duration::from_millis(380));
        let tick = TooltipAnchorId::CommandTick(
            bt_layout::SeatId(1),
            crate::cmdrail::Target::Command(bt_term::CommandMarkId(2)),
        );
        let start = Instant::now();
        let frame = Duration::from_millis(16);

        // The card, in both of its texts: an empty ledger is answered no slower
        // than a quoted command, because muteness is a fact about the words and
        // not about the journey the hand made to reach them.
        for muted in [false, true] {
            let mut host = TooltipHost::default();
            host.observe(Some((tick, TipFace::Peek { muted })), start);
            assert_eq!(
                host.deadline(start, Motion::Full, frame),
                Some(start + Duration::from_millis(120)),
                "the loop is asked to wake at 120ms, not merely to notice later"
            );
            assert!(!host.activate_if_due(start + Duration::from_millis(119)));
            assert!(host.activate_if_due(start + Duration::from_millis(120)));
        }

        // The chrome tip, unmoved.
        let mut host = TooltipHost::default();
        host.observe(Some((TooltipAnchorId::Settings, TipFace::Chrome)), start);
        assert_eq!(
            host.deadline(start, Motion::Full, frame),
            Some(start + Duration::from_millis(380))
        );
        assert!(!host.activate_if_due(start + Duration::from_millis(379)));
        assert!(host.activate_if_due(start + Duration::from_millis(380)));

        // A hand that leaves the button for the rail is on the rail's clock from
        // the moment it arrives — the new subject brings its own length rather
        // than serving out whatever the old one had left.
        let crossed = start + Duration::from_millis(380);
        assert!(host.observe(Some((tick, TipFace::Peek { muted: false })), crossed));
        assert_eq!(
            host.deadline(crossed, Motion::Full, frame),
            Some(crossed + Duration::from_millis(120))
        );

        // And the swatch keeps the chrome clock while wearing the card's type.
        let mut host = TooltipHost::default();
        host.observe(
            Some((
                TooltipAnchorId::PreviewHex(
                    crate::PreviewSurface::Seat(crate::LeafId {
                        tab: crate::TabId(1),
                        seat: bt_layout::SeatId(1),
                    }),
                    12,
                ),
                TipFace::Swatch {
                    rgba: [0x22, 0x88, 0xff, 0xff],
                },
            )),
            start,
        );
        assert!(!host.activate_if_due(start + Duration::from_millis(379)));
        assert!(host.activate_if_due(start + Duration::from_millis(380)));
    }

    /// The face travels on the anchor, because muteness is a fact about the text.
    #[test]
    fn an_anchor_carries_the_face_its_tip_is_drawn_in() {
        let mut anchors = TooltipAnchors::default();
        anchors.push(TooltipAnchorId::NewTab, [0.0, 0.0, 10.0, 10.0], "New tab");
        anchors.push_faced(
            TooltipAnchorId::CommandTick(
                bt_layout::SeatId(1),
                crate::cmdrail::Target::Command(bt_term::CommandMarkId(4)),
            ),
            [20.0, 0.0, 60.0, 10.0],
            "command",
            TipFace::Peek { muted: true },
        );
        assert_eq!(anchors.at(5.0, 5.0).unwrap().face, TipFace::Chrome);
        assert_eq!(
            anchors.at(30.0, 5.0).unwrap().face,
            TipFace::Peek { muted: true }
        );
        // And an anchor with nothing to say is still not an anchor, whichever
        // face it asked for.
        anchors.push_faced(
            TooltipAnchorId::CommandTick(
                bt_layout::SeatId(1),
                crate::cmdrail::Target::Command(bt_term::CommandMarkId(5)),
            ),
            [70.0, 0.0, 90.0, 10.0],
            "   ",
            TipFace::Peek { muted: false },
        );
        assert!(anchors.at(80.0, 5.0).is_none());
    }
}
