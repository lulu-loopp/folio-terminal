//! Notices: the card that says what just went wrong, and then goes away.
//!
//! **Why this exists at all** (user ruling, 2026-08-16). The Git page reported a
//! refused verb by carving a line off the top of its own body and printing git's
//! sentence there in the error ink. Two things were wrong with it and only one
//! was cosmetic. The cosmetic one: a 240-pixel column truncates "fatal:
//! 't1-tab-basics' is already used by worktree at …" to about four words, so the
//! one report a refusal got said nothing. The structural one: a strip carved out
//! of the body is *permanent furniture* for a *transient fact* — the rows below
//! it moved down and stayed down, and the sentence stayed up until the next
//! attempt cleared it, which is a page holding a grudge.
//!
//! A toast is the opposite of both: it stands **over** the surface rather than
//! inside it, so nothing moves; it is as wide as the surface can give it rather
//! than as wide as one row; and it leaves on its own, because a transient fact
//! that needs dismissing is a modal wearing a small coat.
//!
//! **Where it appears** (the same ruling, amending its own first draft). The
//! first draft put every toast in the window's bottom-right corner, which is
//! where the web puts them and which is wrong here for a plain reason: this
//! window is a workspace of panes, and the thing you just pressed was in one of
//! them. A notice in a far corner about a button under your hand asks the eye to
//! travel the whole window to read a sentence about where it already was. So a
//! toast is anchored to the **top of the surface whose action raised it** — see
//! [`ToastAnchor`] — and the corner survives only as the fallback for a surface
//! that has since closed.
//!
//! **What it is made of.** No mock-up rule governs this surface; it is built out
//! of tokens that already exist, so that it reads as part of the house on the
//! day it is born: the menu's own face, hairline and shadow (through
//! [`push_float_window`], which is what a tip and a float window are also drawn
//! with), the `.pv-tool` reveal ladder for its `×`, the status inks for its mark,
//! and the badge's 15% tint for the mark's ground.
//!
//! Three pieces, deliberately apart, on [`crate::tooltip`]'s own division:
//!
//! * [`ToastHost`] — the state and the three clocks. It knows nothing about
//!   seats, panes or git.
//! * [`place`] — where the cards land, given what each anchor resolves to this
//!   frame. Geometry only.
//! * [`build`] — the paint. One layer per card, so each carries its own fade.

use std::time::{Duration, Instant};

use bt_layout::SeatId;
use bt_render::{ChromeLabel, ChromeLabelWeight, ChromePalette, OverlayQuad};

use crate::marks::{ChromeMark, ChromeSprite, OverlayLayer};
use crate::settings::push_float_window;
use crate::{EASE, LeafId, Motion, cubic_bezier};

// ── the clocks ─────────────────────────────────────────────────────────────

/// How long a card takes to arrive: the **base** span, paired with the window's
/// one travel.
///
/// Longer than the tip's fast span (`TOOLTIP_FADE`) and for the opposite reason.
/// A tip is summoned by *not moving* and has to feel instantaneous once the wait
/// is over; a toast arrives unbidden, and something that appears unbidden at
/// full strength in one frame reads as a flash. The step up a rung is the
/// difference between a card sliding into place and a card blinking on.
pub const TOAST_ENTER: Duration = bt_render::MOTION_BASE;

/// How long it takes to leave: the **fast** span, because leaving is the half
/// nobody is waiting for and there is nothing to read on the way out.
pub const TOAST_EXIT: Duration = bt_render::MOTION_FAST;

/// How long a failure stands: six seconds.
///
/// The number is a reading time and not a taste. What an error toast carries in
/// this product is git's own sentence — "fatal: 't1-tab-basics' is already used
/// by worktree at C:/…" — which is forty to eighty characters of *unfamiliar*
/// text including a path, and unfamiliar text with a path in it is read at
/// something like fifteen characters a second, not the two hundred a minute of
/// prose. Six seconds is that, plus the second it takes to notice something
/// appeared.
pub const TOAST_LIFE_ERROR: Duration = Duration::from_millis(6000);

/// How long a confirmation or an aside stands: four seconds.
///
/// Shorter because it is shorter: "Copied" and "Branch switched" are read at a
/// glance, and a card that lingers after it has been read is a card in the way.
pub const TOAST_LIFE_QUIET: Duration = Duration::from_millis(4000);

/// How far the card travels as it arrives, in logical pixels.
///
/// Away from the edge it comes from and never across the surface: an anchored
/// card falls from under the head it hangs off, and the corner fallback rises
/// off the window's floor. The distance is small on purpose — motion here is
/// *provenance*, telling the eye where this thing came from — and it is
/// [`bt_render::MOTION_TRAVEL_LOGICAL_PX`] rather than a number of this file's
/// own. **Four rather than the eight it used to be**: at eight the card was
/// travelling far enough to read as a slide, which is a claim about distance,
/// and every other layer in this window that arrives from somewhere says the
/// same thing in four.
pub const TOAST_SLIDE_LOGICAL_PX: f32 = bt_render::MOTION_TRAVEL_LOGICAL_PX;

/// How many cards one anchor may show at once.
///
/// Three, and the fourth does not queue — it evicts the oldest, which begins
/// leaving at once. Queuing would be the worse answer twice over: a notice held
/// back is a notice about something you have stopped doing by the time it
/// arrives, and a stack that grows without bound eventually covers the surface
/// the notices are about. Per **anchor** rather than per window, because two
/// columns failing at once are two independent conversations.
pub const TOAST_CAP: usize = 3;

// ── the box ────────────────────────────────────────────────────────────────

/// How far a card stands off the three edges of the body it hangs in.
pub const TOAST_ANCHOR_INSET_LOGICAL_PX: f32 = 8.0;
/// How far the corner fallback stands off the window's right and bottom.
///
/// Twice the anchored inset, and deliberately: eight pixels off a pane's body is
/// a card sitting *in* that pane, and sixteen off the window is a card floating
/// clear of everything. Different distances because they are different claims.
pub const TOAST_WINDOW_INSET_LOGICAL_PX: f32 = 16.0;
/// How far apart two cards on one anchor stand.
pub const TOAST_GAP_LOGICAL_PX: f32 = 8.0;
/// How wide the corner fallback is, before the window's own width bounds it.
///
/// The tip's [`crate::tooltip::TIP_MAX_WIDTH_LOGICAL_PX`], for that constant's
/// own reason: 360 sets a paragraph to about sixty characters, which is the
/// measure every other run of prose in this window is set to.
pub const TOAST_WINDOW_WIDTH_LOGICAL_PX: f32 = 360.0;
/// The narrowest a card may be squeezed to before it stops giving ground.
///
/// A column can be dragged narrower than a sentence needs. Below this the card
/// stops matching the body and simply takes all of it, because a 90-pixel card
/// is not a smaller notice — it is six words stacked one per line.
pub const TOAST_MIN_WIDTH_LOGICAL_PX: f32 = 200.0;
/// `border-radius: 8px` — the float window's own round, not the tip's 5.
pub const TOAST_RADIUS_LOGICAL_PX: f32 = 8.0;
/// `border: 1px solid var(--border)`, as every surface through
/// [`push_float_window`] wears.
pub const TOAST_BORDER_LOGICAL_PX: f32 = 1.0;
/// The `12px` of `padding: 10px 12px`.
pub const TOAST_PADDING_X_LOGICAL_PX: f32 = 12.0;
/// The `10px` of `padding: 10px 12px`.
pub const TOAST_PADDING_Y_LOGICAL_PX: f32 = 10.0;
/// `font-size: 12px` — the body.
pub const TOAST_BODY_FONT_LOGICAL_PX: f32 = 12.0;
/// `font-size: 12.5px; font-weight: 500` — the title, when there is one.
pub const TOAST_TITLE_FONT_LOGICAL_PX: f32 = 12.5;
/// The mark column — the box the dot is centred in, so the text of every card
/// starts at the same x whatever the kind.
pub const TOAST_MARK_LOGICAL_PX: f32 = 14.0;
/// `.tdot { width: 6px; height: 6px; border-radius: 50% }` — the mock-up's own
/// toast mark, and the only mark a card wears (user ruling 2026-08-16).
///
/// **A dot and not a glyph in a round.** The first cut drew a `×` inside a
/// tinted circle for an error and a check for a success — the vocabulary of
/// Sonner and Radix, and of nothing else in this house. Here a circle is only
/// ever a *state*: the filled dot on the current branch, the hollow one on the
/// others, the node on a commit, the unread dot on a tab, the pip's dot. It is
/// never a container for an icon. So the card says what the tab and the branch
/// row say — one dot, in the ink that names the state — and a `×` on a card
/// means exactly one thing, which is the dismiss verb at its right.
pub const TOAST_DOT_LOGICAL_PX: f32 = 6.0;
/// The gap between the mark column and the text.
pub const TOAST_MARK_GAP_LOGICAL_PX: f32 = 8.0;
/// `.gact`'s own 18 — the dismiss verb's box.
pub const TOAST_CLOSE_LOGICAL_PX: f32 = 18.0;
/// `.gact { border-radius: 5px }` — its pill.
pub const TOAST_CLOSE_RADIUS_LOGICAL_PX: f32 = 5.0;
/// The `×` inside that box.
pub const TOAST_CLOSE_GLYPH_LOGICAL_PX: f32 = 8.0;
/// The verb's own type — the body's 12px, because it stands under the body and
/// a second size would read as a second voice.
pub const TOAST_ACTION_FONT_LOGICAL_PX: f32 = 12.0;
/// How far the verb's row stands off the last line of the body.
///
/// Six and not the eight two cards stand apart: the verb belongs to the sentence
/// above it, and a gap as wide as the one between two separate notices would
/// have made it look like a second card that lost its box.
pub const TOAST_ACTION_MARGIN_TOP_LOGICAL_PX: f32 = 6.0;
/// The padding either side of the verb, which is what its pressable box is
/// wider than its word by.
///
/// `.gact`'s register carried onto a word instead of a glyph: the `×` gets an
/// 18px box round an 8px mark, and this gets the same five pixels of air on the
/// axis a word actually grows along.
pub const TOAST_ACTION_PADDING_X_LOGICAL_PX: f32 = 6.0;
/// `.gact { border-radius: 5px }`, the dismiss verb's own pill — one round for
/// the two pressable things on a card.
pub const TOAST_ACTION_RADIUS_LOGICAL_PX: f32 = 5.0;

/// The most lines of body text a card will show before it stops.
///
/// Six, and the seventh is not shown but *reported* — the last line kept ends in
/// an ellipsis, so a card that has cut something says so. A notice is not a
/// document: past six lines the card has stopped being a notice and started
/// being a wall, and the full text of a git failure is a thing the terminal
/// beside this window prints in full anyway.
pub const TOAST_MAX_LINES: usize = 6;

/// The line box every other run of chrome text is laid out in.
///
/// [`crate::tooltip`]'s own constant and its reasoning: `shape_chrome_labels`
/// sizes a label's buffer to `font_size * 1.4`, and a card whose rows disagreed
/// with every single-line label beside it would be a third answer to a question
/// this renderer has already answered twice.
const CHROME_LINE_HEIGHT: f32 = 1.4;

// ── what a notice is ───────────────────────────────────────────────────────

/// What kind of thing happened.
///
/// Three and not more, because these are the three claims a status ink in this
/// palette can make: something failed, something finished, something is worth
/// knowing. A fourth kind would need a fourth ink, and the palette's whole
/// discipline is that a colour means one thing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToastKind {
    /// `status_err` — the rose. Git refused, a write would not go through.
    ///
    /// The first and, today, the only caller: [`crate::git::write_refusal`]'s
    /// sentence, raised where the verb was pressed.
    Error,
    /// `accent` — a fact, in the colour this product says "look here" in.
    ///
    /// It had no caller when it was written, and that was deliberate: a host
    /// that could only carry failures would be a *failure* host, and the first
    /// quiet thing this window wanted to say would have arrived as a second
    /// mechanism beside it. The day arrived with v2 ② and cost one call site —
    /// a seek that ran out of history (`git_graph::graph_seek_gave_up`), which
    /// is a fact about the reading and not a fault of anything.
    Info,
    /// `status_ok` — it worked, and the working is worth saying.
    ///
    /// Its first caller is v2 ②'s copy verbs (D7), and it is the case the kind
    /// was reserved for: a copy's whole effect is somewhere the reader cannot
    /// see, so a verb that said nothing would be a verb nobody could tell had
    /// run.
    Ok,
}

impl ToastKind {
    /// The ink this kind's mark is struck in.
    #[must_use]
    pub fn ink(self, palette: &ChromePalette) -> [u8; 3] {
        match self {
            Self::Error => palette.status_err,
            Self::Info => palette.accent,
            Self::Ok => palette.status_ok,
        }
    }

    /// How long a card of this kind stands once it has arrived.
    #[must_use]
    pub fn life(self) -> Duration {
        match self {
            Self::Error => TOAST_LIFE_ERROR,
            Self::Info | Self::Ok => TOAST_LIFE_QUIET,
        }
    }
}

/// Which surface a notice belongs to.
///
/// **Identity and not a rectangle.** The rectangle is resolved afresh every
/// frame, exactly as a tooltip's text is: a column can be dragged wider, a seat
/// can be split, and a card holding the geometry it was born with would be a
/// confident sentence standing where its subject used to be.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToastAnchor {
    /// A docked files column — the seat whose Git page issued the verb.
    ///
    /// It stays anchored here even when the column is flipped back to the Files
    /// page, and that is the ruling rather than an oversight: the notice is about
    /// what *that column* was asked to do, and moving it to the corner because
    /// the reader glanced at the tree would be the window losing the thread.
    /// Only a column that has closed altogether has no surface left to stand on.
    FilesColumn(SeatId),
    /// A preview seat — the graph whose double-click checkout git refused.
    ///
    /// **The whole [`LeafId`], not the seat number** (§7.12 ⓑ). A notice lives
    /// on the window and outlives a tab switch, while a seat number means
    /// something only inside its own tab; anchoring by the number alone pointed
    /// a card at whichever pane the tab in front happened to have numbered the
    /// same. A card whose pane is on a tab you are not looking at has no surface
    /// to stand on and takes the corner — see `Runtime::toast_anchor_rect`.
    PreviewSeat(LeafId),
    /// No surface: the window itself, bottom-right. **The fallback only.**
    Window,
}

/// Which notice. Identity only — never the words.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ToastId(pub u64);

/// One notice, and the three instants that govern it.
///
/// The clock is held in three parts rather than one deadline, because a hover
/// stops it: a single `expires_at` would have to be rewritten every time the
/// pointer crossed the card, and a deadline that is rewritten is a deadline that
/// can be rewritten wrong. Instead the epoch never moves, the time spent under
/// the pointer is accumulated, and expiry is arithmetic — which is also what
/// makes "how long has this been held" a question the tests can ask directly.
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    id: ToastId,
    kind: ToastKind,
    anchor: ToastAnchor,
    title: Option<String>,
    body: String,
    /// The one verb this card offers, if it offers one — `Undo`.
    ///
    /// **A card grows an action only where the thing it reports is
    /// irreversible.** A notice saying a branch was switched has nothing to
    /// offer: the reader can switch it back through the surface that switched
    /// it. A notice saying a profile was deleted has, because the row it names
    /// is gone from the list the reader would have used. Plan §2.3 and the
    /// ruling behind it are explicit that what deletion is owed is an *undo*
    /// and not a confirmation: this dialog writes every choice the instant it
    /// is made and has no dirty gate to route a question through, so a
    /// confirmation here would be the first modal over a modal in this product,
    /// while an undo is a precedent it already struck as `Ctrl+Shift+T`.
    ///
    /// One verb and not a list. A card is a notice, and a notice with two
    /// things to press is a dialog that forgot to say it was one.
    action: Option<String>,
    /// When it arrived. The epoch for the entrance *and* for the life.
    born: Instant,
    /// Time already spent under the pointer, which the life does not count.
    held: Duration,
    /// When the pointer arrived, while it is still there.
    hover_since: Option<Instant>,
    /// When the exit began, and the opacity it began from.
    ///
    /// The opacity is carried because a card can be dismissed *during* its
    /// entrance — a fourth notice evicting a third that is 40ms old — and a fade
    /// that started from 1.0 there would make the card flash brighter on its way
    /// out than it ever was on its way in.
    leaving: Option<(Instant, f32)>,
}

impl Toast {
    /// Which surface this card belongs to — the one thing about a notice the
    /// runtime has to ask, because only the runtime can turn it into a rectangle.
    #[must_use]
    pub fn anchor(&self) -> ToastAnchor {
        self.anchor
    }

    /// Whether the exit has begun.
    #[must_use]
    pub fn leaving(&self) -> bool {
        self.leaving.is_some()
    }

    /// How solid this card is drawn at this instant.
    ///
    /// `EASE` on the way in and on the way out — the mock-up's own keyword for
    /// every transition it declares, and the one [`crate::tooltip`] already
    /// fades on.
    ///
    /// **Reduced motion is consulted here, and it was not always.** The old
    /// arrangement pushed the preference up into the caller: `raise` took a
    /// `still` flag and back-dated the card's birth so that its entrance was
    /// already over. That answered the entrance and left the *exit* running —
    /// ninety milliseconds of decorative fade, with the frames to draw it, in a
    /// window that had asked for no animation — and it bought the wrong thing
    /// besides, because a card born in the past has a life that started in the
    /// past. Reading the preference here is one line and it covers both ends.
    #[must_use]
    pub fn opacity(&self, now: Instant, motion: Motion) -> f32 {
        if let Some((left, from)) = self.leaving {
            if motion == Motion::Reduced {
                return 0.0;
            }
            let progress = ratio(now.saturating_duration_since(left), TOAST_EXIT);
            return from * (1.0 - cubic_bezier(progress, EASE));
        }
        if motion == Motion::Reduced {
            return 1.0;
        }
        cubic_bezier(
            ratio(now.saturating_duration_since(self.born), TOAST_ENTER),
            EASE,
        )
    }

    /// How far off its resting place this card is drawn, in **logical** pixels —
    /// negative is up.
    ///
    /// It travels only on the way in. A card leaving does not slide back where it
    /// came from, because the two motions do not mean the same thing: arriving
    /// from an edge says where a thing came from, and retreating to an edge says
    /// it is going somewhere — and it is not, it is ending.
    /// Under [`Motion::Reduced`] it does not travel at all — the card is simply
    /// where it belongs, which is the whole of what the four pixels were saying.
    #[must_use]
    pub fn slide(&self, now: Instant, motion: Motion) -> f32 {
        if self.leaving.is_some() || motion == Motion::Reduced {
            return 0.0;
        }
        let progress = cubic_bezier(
            ratio(now.saturating_duration_since(self.born), TOAST_ENTER),
            EASE,
        );
        // Anchored cards hang off the top of a body and therefore fall into
        // place; the corner fallback stands on the window's floor and rises.
        let from = match self.anchor {
            ToastAnchor::Window => TOAST_SLIDE_LOGICAL_PX,
            ToastAnchor::FilesColumn(_) | ToastAnchor::PreviewSeat(_) => -TOAST_SLIDE_LOGICAL_PX,
        };
        from * (1.0 - progress)
    }

    /// When this card's own time runs out — the instant its exit begins.
    ///
    /// While the pointer is on it this instant keeps moving away, which is
    /// exactly what "hovering holds the clock" means: reading a notice is not
    /// the same as having read it.
    #[must_use]
    fn expires_at(&self, now: Instant) -> Instant {
        self.born + self.kind.life() + self.held_at(now)
    }

    /// How much of this card's life has been spent under the pointer by `now`.
    fn held_at(&self, now: Instant) -> Duration {
        self.held
            + self
                .hover_since
                .map_or(Duration::ZERO, |since| now.saturating_duration_since(since))
    }

    /// The next instant this card needs a frame at, for whatever it is doing.
    ///
    /// `None` while it is held: a stopped clock owes no wake-ups, and the pointer
    /// leaving is an event that will re-arm it. This is the same silence a tip
    /// with nothing settling keeps.
    ///
    /// Under [`Motion::Reduced`] the two animation deadlines — the end of the
    /// entrance and the end of the exit — are both gone, and the only instant
    /// left is the life running out. That is not a tween; it is when the notice
    /// stops being true. A departing card owes nothing at all there, because
    /// under reduced motion a departing card is already off the list.
    fn deadline(&self, now: Instant, motion: Motion) -> Option<Instant> {
        if let Some((left, _)) = self.leaving {
            return (motion == Motion::Full).then_some(left + TOAST_EXIT);
        }
        if motion == Motion::Full {
            let arrived = self.born + TOAST_ENTER;
            if now < arrived {
                return Some(arrived);
            }
        }
        self.hover_since.is_none().then(|| self.expires_at(now))
    }

    /// Begin the exit, from wherever this card currently stands.
    fn depart(&mut self, now: Instant, motion: Motion) {
        if self.leaving.is_none() {
            self.leaving = Some((now, self.opacity(now, motion)));
        }
    }
}

/// `elapsed / span`, clamped — the progress of anything on a fixed clock.
fn ratio(elapsed: Duration, span: Duration) -> f32 {
    (elapsed.as_secs_f32() / span.as_secs_f32()).clamp(0.0, 1.0)
}

/// Every notice this window is showing, oldest first.
///
/// One host for the whole window rather than one per anchor, and the anchors are
/// a *field* on each card: there is one pointer, one z-order and one frame debt,
/// and three hosts would have to agree about all three. The cap is applied per
/// anchor inside [`Self::raise`], which is where the per-anchor rule belongs —
/// it is a rule about crowding one surface, not about how state is stored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToastHost {
    toasts: Vec<Toast>,
    next_id: u64,
}

impl ToastHost {
    /// Raise a notice, and evict the oldest of its anchor if that anchor is full.
    ///
    /// `still` is the reduced-motion preference: with it set the card is born at
    /// its resting place and full strength, which is the same thing every other
    /// animation in this window does when the system asks for stillness. It is
    /// spelled by moving `born` back past the entrance rather than by a flag,
    /// so that exactly one code path computes an opacity.
    ///
    /// `action` is the one verb the card offers, or `None` for the ordinary
    /// notice that offers none — see [`Toast::action`]. It does **not** get a
    /// clock of its own: a card with an undo on it lives exactly as long as a
    /// card of its kind without one, and the reason is that this file already
    /// has the mechanism a longer deadline would be reaching for. The life is
    /// suspended while the pointer is on the card ([`Self::hover`]), and a
    /// reader deciding whether to undo is a reader whose pointer is on the card
    /// — so the four seconds are four seconds of *not* reaching for it. A
    /// second clock keyed on "does this one have a verb" would be a second
    /// answer to how long a notice stands, and the first answer is a reading
    /// time that the verb does not change.
    #[expect(
        clippy::too_many_arguments,
        reason = "a card is a notice, an anchor, three strings and two instants; \
                  bundling them into a struct would put a builder in front of the \
                  one call this module exists to serve"
    )]
    pub fn raise(
        &mut self,
        kind: ToastKind,
        anchor: ToastAnchor,
        title: Option<String>,
        body: impl Into<String>,
        action: Option<String>,
        motion: Motion,
        now: Instant,
    ) -> ToastId {
        // The fourth card on a crowded anchor pushes the oldest out *now*, not
        // when it would have gone by itself: what makes room is the departure,
        // and a departure scheduled for later is a stack of four in the meantime.
        while self
            .toasts
            .iter()
            .filter(|toast| toast.anchor == anchor && !toast.leaving())
            .count()
            >= TOAST_CAP
        {
            let Some(oldest) = self
                .toasts
                .iter_mut()
                .find(|toast| toast.anchor == anchor && !toast.leaving())
            else {
                break;
            };
            oldest.depart(now, motion);
            self.retire_the_departed(now, motion);
        }
        let id = ToastId(self.next_id);
        self.next_id += 1;
        self.toasts.push(Toast {
            id,
            kind,
            anchor,
            title,
            body: body.into(),
            action,
            born: now,
            held: Duration::ZERO,
            hover_since: None,
            leaving: None,
        });
        id
    }

    /// Every card, oldest first.
    #[must_use]
    pub fn toasts(&self) -> &[Toast] {
        &self.toasts
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    /// Send one card away at once — the `×`, pressed. Returns whether it was
    /// there to send.
    pub fn dismiss(&mut self, id: ToastId, now: Instant, motion: Motion) -> bool {
        let Some(toast) = self.toasts.iter_mut().find(|toast| toast.id == id) else {
            return false;
        };
        let already = toast.leaving();
        toast.depart(now, motion);
        self.retire_the_departed(now, motion);
        !already
    }

    /// Note which card the pointer is on. Returns whether anything changed.
    ///
    /// A card that is already leaving is not held: its clock has stopped
    /// mattering, and a hand that happens to rest on a card fading out would
    /// otherwise freeze it there for as long as it stayed.
    pub fn hover(&mut self, id: Option<ToastId>, now: Instant) -> bool {
        let mut changed = false;
        for toast in &mut self.toasts {
            let wanted = Some(toast.id) == id && !toast.leaving();
            match (toast.hover_since, wanted) {
                (None, true) => {
                    toast.hover_since = Some(now);
                    changed = true;
                }
                (Some(since), false) => {
                    toast.held += now.saturating_duration_since(since);
                    toast.hover_since = None;
                    changed = true;
                }
                _ => {}
            }
        }
        changed
    }

    /// Move every clock forward: start the exits that are due, and drop the
    /// cards whose exits have finished. Returns whether anything changed.
    pub fn advance(&mut self, now: Instant, motion: Motion) -> bool {
        let mut changed = false;
        for toast in &mut self.toasts {
            if toast.leaving.is_none() && now >= toast.expires_at(now) {
                toast.depart(now, motion);
                changed = true;
            }
        }
        let before = self.toasts.len();
        self.retire_the_departed(now, motion);
        changed || self.toasts.len() != before
    }

    /// Drop the cards whose exit is over — which, under [`Motion::Reduced`], is
    /// every card that has begun one.
    ///
    /// Called from all three places a card can be sent away rather than only
    /// from [`Self::advance`], and that is what makes the reduced-motion promise
    /// hold rather than nearly hold. With no exit fade there is no span to hold
    /// a departing card on the list for, and a card that stayed there at zero
    /// opacity would be invisible, un-hittable, and still occupying one of the
    /// three places an anchor has.
    fn retire_the_departed(&mut self, now: Instant, motion: Motion) {
        let exit = match motion {
            Motion::Full => TOAST_EXIT,
            Motion::Reduced => Duration::ZERO,
        };
        self.toasts.retain(|toast| {
            toast
                .leaving
                .is_none_or(|(left, _)| now.saturating_duration_since(left) < exit)
        });
    }

    /// The next instant this host needs a frame: the end of an entrance, a life
    /// running out, or the end of an exit — whichever comes first across every
    /// card. `None` when there is nothing to wait for.
    #[must_use]
    pub fn deadline(&self, now: Instant, motion: Motion) -> Option<Instant> {
        self.toasts
            .iter()
            .filter_map(|toast| toast.deadline(now, motion))
            .min()
    }

    /// What should be on screen this instant: every card's id and its opacity.
    ///
    /// The strip's frame-debt idea ([`crate::tooltip::TooltipHost`] applies the
    /// same one to its fade): compare this against what was last *painted*
    /// rather than asking "is an animation running", because the two differ at
    /// exactly the instant that matters — the frame an animation lands on, which
    /// "still running" answers `false` for and which therefore never gets drawn.
    #[must_use]
    pub fn frame_state(&self, now: Instant, motion: Motion) -> Vec<(ToastId, f32)> {
        self.toasts
            .iter()
            .map(|toast| (toast.id, toast.opacity(now, motion)))
            .collect()
    }
}

// ── where the cards land ───────────────────────────────────────────────────

/// One placed card. Every rectangle is physical pixels, at rest — the slide is
/// applied by [`build`], because it is a property of the instant and not of the
/// layout.
#[derive(Clone, Debug, PartialEq)]
pub struct ToastLayout {
    pub id: ToastId,
    pub kind: ToastKind,
    /// `[left, top, right, bottom]` of the whole card.
    pub frame: [f32; 4],
    /// The kind's round, at the left of the first text line.
    pub mark: [f32; 4],
    /// The dismiss verb's box — **reserved whether or not it is drawn**, which is
    /// what makes the hit test and the paint one derivation rather than two.
    pub close: [f32; 4],
    /// The title's row, when there is a title.
    pub title: Option<([f32; 4], String)>,
    /// One row per body line, in order.
    pub lines: Vec<([f32; 4], String)>,
    /// The action verb's pressable box and its word, when the card has one.
    ///
    /// **A row of its own, under the body, right-aligned on the text column's
    /// own trailing edge.** Two things follow from that and both are the reason
    /// for it. It cannot collide with the body, because a verb tucked after the
    /// last wrapped line would land somewhere different on every card and
    /// nowhere at all on a card whose last line is full — so the card never has
    /// to grow wider to fit the two side by side, which is a width this layout
    /// does not get to choose (an anchored card takes its body's width). And it
    /// cannot collide with the `×`, because the `×` has a column of its own
    /// reserved for the card's whole height and this verb stops at
    /// `text_right`, one gap short of it — pinned by
    /// `the_two_pressable_things_on_a_card_never_share_a_pixel`.
    pub action: Option<([f32; 4], String)>,
}

/// What a press on a card means.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToastHit {
    /// The `×`.
    Close(ToastId),
    /// The card's one verb — `Undo`.
    ///
    /// What it *does* is not this module's to know: a card carries a word and a
    /// rectangle, and the runtime that raised it is the only thing that knows
    /// what pressing it undoes. That is [`ToastAnchor`]'s own division kept —
    /// identity here, meaning at the door.
    Action(ToastId),
    /// Anywhere else on the card — which is **not** nothing: a toast is not
    /// click-through, and a press that fell past it onto the rows underneath
    /// would stage a file because the card you were dismissing was in the way.
    Card(ToastId),
}

/// Which card the pointer is on, topmost first.
///
/// Later cards are drawn over earlier ones on a crowded anchor only in the sense
/// that they are lower down; they never overlap. The reverse walk is still the
/// right one, because it is the paint order reversed, and a hit test that
/// disagreed with the paint order is the invisible button.
#[must_use]
pub fn at(layouts: &[ToastLayout], x: f32, y: f32) -> Option<ToastHit> {
    layouts.iter().rev().find_map(|layout| {
        if !contains(layout.frame, x, y) {
            return None;
        }
        // The two boxes are disjoint by construction (see
        // `ToastLayout::action`), so the order of these two arms is a reading
        // order and not a precedence — but they are still asked before the card
        // itself, because the card is what is left over.
        Some(if contains(layout.close, x, y) {
            ToastHit::Close(layout.id)
        } else if layout
            .action
            .as_ref()
            .is_some_and(|(rect, _)| contains(*rect, x, y))
        {
            ToastHit::Action(layout.id)
        } else {
            ToastHit::Card(layout.id)
        })
    })
}

fn contains(rect: [f32; 4], x: f32, y: f32) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

/// Place every card.
///
/// `anchor_rect` answers "where is this surface, this frame" and `None` means
/// "it is not on screen" — a column that closed, a seat that was folded away.
/// Those cards fall to the window's corner, which is the whole of what
/// [`ToastAnchor::Window`] is for.
///
/// `measure` is the font's answer to "how wide is this string, at this size",
/// handed in for the reason every measured caption in this codebase hands it in:
/// only the thing holding the font can say.
#[must_use]
pub fn place(
    toasts: &[Toast],
    anchor_rect: impl Fn(ToastAnchor) -> Option<[f32; 4]>,
    window: (f32, f32),
    scale: f32,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> Vec<ToastLayout> {
    let px = |logical: f32| logical * scale;
    // Group by where each card actually lands, keeping the host's order inside
    // each group so "newest last" survives the grouping.
    let mut groups: Vec<(Option<[f32; 4]>, Vec<&Toast>)> = Vec::new();
    for toast in toasts {
        let body = anchor_rect(toast.anchor).filter(|rect| rect[2] > rect[0] && rect[3] > rect[1]);
        match groups.iter_mut().find(|(rect, _)| *rect == body) {
            Some((_, group)) => group.push(toast),
            None => groups.push((body, vec![toast])),
        }
    }

    let mut placed = Vec::new();
    for (body, group) in groups {
        // The corner fallback's width is the tip's bound, held inside the window;
        // an anchored card is as wide as its body allows, with a floor under it.
        let (width, left_of) = match body {
            Some(body) => {
                let body_width = body[2] - body[0];
                let inset = px(TOAST_ANCHOR_INSET_LOGICAL_PX);
                let width = (body_width - 2.0 * inset)
                    .max(px(TOAST_MIN_WIDTH_LOGICAL_PX))
                    .min(body_width)
                    .round();
                // Centred, which *is* the inset whenever the body is wide enough
                // to give it — and the graceful answer when it is not, instead of
                // a card whose right edge has left the pane.
                (width, (body[0] + (body_width - width) / 2.0).round())
            }
            None => {
                let inset = px(TOAST_WINDOW_INSET_LOGICAL_PX);
                let width = px(TOAST_WINDOW_WIDTH_LOGICAL_PX)
                    .min(window.0 - 2.0 * inset)
                    .max(0.0)
                    .round();
                (width, (window.0 - inset - width).round())
            }
        };

        // Laid out downward from a common top in both cases; the corner group is
        // then translated so its *last* card sits on the window's floor, which is
        // what "stacked upward, newest at the bottom" is once the heights are
        // known.
        let gap = px(TOAST_GAP_LOGICAL_PX);
        let first_top = match body {
            Some(body) => (body[1] + px(TOAST_ANCHOR_INSET_LOGICAL_PX)).round(),
            None => 0.0,
        };
        let mut top = first_top;
        let start = placed.len();
        for toast in group {
            let layout = lay_one(toast, [left_of, top, left_of + width], scale, measure);
            top = layout.frame[3] + gap;
            placed.push(layout);
        }
        if body.is_none()
            && let Some(last) = placed.last()
        {
            let floor = (window.1 - px(TOAST_WINDOW_INSET_LOGICAL_PX)).round();
            let shift = floor - last.frame[3];
            for layout in &mut placed[start..] {
                layout.shift(shift);
            }
        }
    }
    placed
}

impl ToastLayout {
    /// Move every rectangle of this card by `dy`.
    fn shift(&mut self, dy: f32) {
        let slide = |rect: &mut [f32; 4]| {
            rect[1] += dy;
            rect[3] += dy;
        };
        slide(&mut self.frame);
        slide(&mut self.mark);
        slide(&mut self.close);
        if let Some((rect, _)) = self.title.as_mut() {
            slide(rect);
        }
        for (rect, _) in &mut self.lines {
            slide(rect);
        }
        if let Some((rect, _)) = self.action.as_mut() {
            slide(rect);
        }
    }
}

/// One card, laid out into a left/top/right that has already been decided.
fn lay_one(
    toast: &Toast,
    span: [f32; 3],
    scale: f32,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> ToastLayout {
    let px = |logical: f32| logical * scale;
    let [left, top, right] = span;
    let border = px(TOAST_BORDER_LOGICAL_PX);
    let pad_x = px(TOAST_PADDING_X_LOGICAL_PX);
    let pad_y = px(TOAST_PADDING_Y_LOGICAL_PX);
    let mark_box = px(TOAST_MARK_LOGICAL_PX);
    let mark_gap = px(TOAST_MARK_GAP_LOGICAL_PX);
    let close_box = px(TOAST_CLOSE_LOGICAL_PX);
    let body_font = px(TOAST_BODY_FONT_LOGICAL_PX);
    let title_font = px(TOAST_TITLE_FONT_LOGICAL_PX);
    let body_line = (body_font * CHROME_LINE_HEIGHT).round();
    let title_line = (title_font * CHROME_LINE_HEIGHT).round();

    // The `×`'s column is reserved for the whole height of the card and not only
    // for its own row. Text that ran under a button which appears on hover would
    // be text that is legible until you reach for the thing that dismisses it.
    let text_left = left + border + pad_x + mark_box + mark_gap;
    let text_right = (right - border - pad_x - close_box - mark_gap).max(text_left + 1.0);

    let wrapped = wrap_body(&toast.body, text_right - text_left, body_font, measure);

    let action_font = px(TOAST_ACTION_FONT_LOGICAL_PX);
    let action_line = (action_font * CHROME_LINE_HEIGHT).round();
    let action_margin = px(TOAST_ACTION_MARGIN_TOP_LOGICAL_PX);

    let mut height = 2.0 * (border + pad_y);
    if toast.title.is_some() {
        height += title_line;
    }
    height += wrapped.len() as f32 * body_line;
    if toast.action.is_some() {
        height += action_margin + action_line;
    }
    // A one-line card is shorter than its own dismiss button; the button is what
    // sets the floor, because a verb hanging out of the card it belongs to is
    // not a smaller card.
    height = height.max(2.0 * (border + pad_y) + close_box).round();

    let frame = [left, top, right, top + height];
    let mut y = top + border + pad_y;
    let title = toast.title.as_ref().map(|text| {
        let row = ([text_left, y, text_right, y + title_line], text.clone());
        y += title_line;
        row
    });
    let first_body_top = y;
    let lines: Vec<([f32; 4], String)> = wrapped
        .into_iter()
        .map(|line| {
            let row = ([text_left, y, text_right, y + body_line], line);
            y += body_line;
            row
        })
        .collect();

    // The verb's box hugs its word and hangs off the text column's trailing
    // edge, which is one whole gap short of the `×`'s column — so the two
    // pressable things on a card can never share a pixel however long the word
    // is. Held off `text_left` rather than allowed to run past it: in a column
    // dragged narrower than a single word the verb takes the whole text column
    // and the label clips, which is the same ground the body's own wrap gives
    // when it runs out of room.
    let action = toast.action.as_ref().map(|word| {
        y += action_margin;
        let wanted =
            measure(word, action_font).round() + 2.0 * px(TOAST_ACTION_PADDING_X_LOGICAL_PX);
        let width = wanted.min(text_right - text_left);
        let row = (
            [text_right - width, y, text_right, y + action_line],
            word.clone(),
        );
        y += action_line;
        row
    });

    // The mark is centred on the first line of *text*, whichever kind of line
    // that is: a card with a title leads with the title, and a mark that sat
    // beside the body under it would be pointing at the second sentence.
    let first_line_top = title.as_ref().map_or(first_body_top, |(rect, _)| rect[1]);
    let first_line_height = if title.is_some() {
        title_line
    } else {
        body_line
    };
    let mark_top = (first_line_top + (first_line_height - mark_box) / 2.0).round();
    let mark_left = left + border + pad_x;
    let close_left = right - border - pad_x - close_box;
    let close_top = top + border + pad_y;

    ToastLayout {
        id: toast.id,
        kind: toast.kind,
        frame,
        mark: [
            mark_left,
            mark_top,
            mark_left + mark_box,
            mark_top + mark_box,
        ],
        close: [
            close_left,
            close_top,
            close_left + close_box,
            close_top + close_box,
        ],
        title,
        lines,
        action,
    }
}

/// Break the body into the lines a card will draw, and say so when it has cut.
///
/// [`crate::tooltip::wrap`] does the breaking — one implementation of "how does
/// text become lines" in this window, and a card that broke its sentences by a
/// different rule than a tip does would be two answers to one question. What is
/// added here is the cap: past [`TOAST_MAX_LINES`] the rest is dropped and the
/// last line kept ends in an ellipsis, so the card admits there was more.
#[must_use]
fn wrap_body(
    text: &str,
    max_width: f32,
    font_px: f32,
    measure: &mut dyn FnMut(&str, f32) -> f32,
) -> Vec<String> {
    let mut lines = crate::tooltip::wrap(text, max_width, |run| measure(run, font_px));
    if lines.len() > TOAST_MAX_LINES {
        lines.truncate(TOAST_MAX_LINES);
        if let Some(last) = lines.last_mut() {
            last.push('…');
        }
    }
    lines
}

// ── the paint ──────────────────────────────────────────────────────────────

/// Which rung of the reveal ladder the `×` is on.
///
/// `.pv-tool`'s own three (`crate::seats::PREVIEW_TOOL_REVEAL`), and the same
/// three the Git page's hover verbs climb: absent while the pointer is elsewhere,
/// seven-tenths once the card has it, whole — over its own pill — once the button
/// does. One ladder for every hover verb in this product.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToastPointer {
    /// The card under the pointer.
    pub card: Option<ToastId>,
    /// The `×` under the pointer.
    pub close: Option<ToastId>,
    /// The action verb under the pointer.
    ///
    /// It takes the `×`'s lit pill and **not** its reveal ladder: the dismiss
    /// verb may hide while the pointer is elsewhere because a card dismisses
    /// itself anyway, and an undo may not, because the card's own expiry is the
    /// deadline for pressing it. A verb you have to find by hovering is a verb
    /// that is gone by the time you have found it.
    pub action: Option<ToastId>,
}

/// Paint the cards — **one layer each**, so each carries its own fade.
///
/// One layer for all of them would mean one opacity for all of them, and the
/// whole point of the host is that three cards are three independent clocks.
#[must_use]
pub fn build(
    layouts: &[ToastLayout],
    host: &ToastHost,
    pointer: ToastPointer,
    palette: &ChromePalette,
    scale: f32,
    now: Instant,
    motion: Motion,
) -> Vec<OverlayLayer> {
    let px = |logical: f32| logical * scale;
    let alpha = |value: u8| f32::from(value) / 255.0;
    let mut layers = Vec::new();

    for layout in layouts {
        let Some(toast) = host.toasts.iter().find(|toast| toast.id == layout.id) else {
            continue;
        };
        let opacity = toast.opacity(now, motion);
        if opacity <= 0.0 {
            continue;
        }
        let dy = (toast.slide(now, motion) * scale).round();
        let mut card = layout.clone();
        card.shift(dy);

        let mut quads: Vec<OverlayQuad> = Vec::new();
        push_float_window(
            &mut quads,
            card.frame,
            px(TOAST_RADIUS_LOGICAL_PX),
            px(TOAST_BORDER_LOGICAL_PX),
            px(bt_render::FLOAT_WINDOW_SHADOW_LOGICAL_PX),
            palette.menu_surface,
            palette.menu_shadow,
            alpha(palette.menu_shadow_inner_alpha),
            alpha(palette.menu_shadow_outer_alpha),
            palette.menu_border,
            alpha(palette.menu_border_alpha),
        );

        let mut sprites = Vec::new();
        let ink = toast.kind.ink(palette);
        // The kind's dot — `.tdot`, in the ink that names the state, centred in
        // the mark column. See [`TOAST_DOT_LOGICAL_PX`] for why it is a dot and
        // not a glyph in a round.
        let dot = centred(card.mark, px(TOAST_DOT_LOGICAL_PX));
        sprites.push(ChromeSprite::new(
            ChromeMark::ControlPill {
                radius_px: (px(TOAST_DOT_LOGICAL_PX) / 2.0).round().max(1.0) as u32,
            },
            dot,
            ink,
        ));

        // The dismiss verb, on the ladder.
        let lit = pointer.close == Some(toast.id);
        let revealed = lit || pointer.card == Some(toast.id);
        if revealed {
            if lit {
                sprites.push(ChromeSprite::new(
                    ChromeMark::ControlPill {
                        radius_px: (px(TOAST_CLOSE_RADIUS_LOGICAL_PX)).round().max(1.0) as u32,
                    },
                    card.close,
                    palette.menu_item_hover,
                ));
            }
            let mut glyph = ChromeSprite::new(
                ChromeMark::TabClose,
                centred(card.close, px(TOAST_CLOSE_GLYPH_LOGICAL_PX)),
                if lit {
                    palette.menu_item_text_selected
                } else {
                    palette.menu_item_text
                },
            );
            glyph.opacity = if lit {
                1.0
            } else {
                crate::seats::PREVIEW_TOOL_REVEAL
            };
            sprites.push(glyph);
        }

        let mut labels = Vec::new();
        if let Some((rect, text)) = &card.title {
            labels.push(ChromeLabel {
                mono: false,
                text: text.clone(),
                rect: *rect,
                font_size_px: px(TOAST_TITLE_FONT_LOGICAL_PX),
                // `--ink` over `--menu`: the title is the one line on this card
                // that names *what* is speaking, and it is the only thing here
                // drawn at the surface's full ink.
                color: palette.menu_item_text_selected,
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::Medium,
                tabular_numerals: false,
                clip: Some(card.frame),
            });
        }
        for (rect, text) in &card.lines {
            labels.push(ChromeLabel {
                mono: false,
                text: text.clone(),
                rect: *rect,
                font_size_px: px(TOAST_BODY_FONT_LOGICAL_PX),
                color: palette.menu_item_text,
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::Regular,
                tabular_numerals: false,
                clip: Some(card.frame),
            });
        }
        // The one verb, always drawn — see `ToastPointer::action`. It is struck
        // at the surface's full ink and Medium, which is the register the title
        // wears, because on a card those two are the same claim: this line is
        // not the prose. Under the pointer it takes the `×`'s own lit pill, so
        // that the two pressable things on a card answer a hand the same way.
        if let Some((rect, word)) = &card.action {
            if pointer.action == Some(toast.id) {
                sprites.push(ChromeSprite::new(
                    ChromeMark::ControlPill {
                        radius_px: px(TOAST_ACTION_RADIUS_LOGICAL_PX).round().max(1.0) as u32,
                    },
                    *rect,
                    palette.menu_item_hover,
                ));
            }
            labels.push(ChromeLabel {
                mono: false,
                text: word.clone(),
                rect: [
                    rect[0] + px(TOAST_ACTION_PADDING_X_LOGICAL_PX),
                    rect[1],
                    rect[2] - px(TOAST_ACTION_PADDING_X_LOGICAL_PX),
                    rect[3],
                ],
                font_size_px: px(TOAST_ACTION_FONT_LOGICAL_PX),
                color: palette.menu_item_text_selected,
                align_right: false,
                align_center: true,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::Medium,
                tabular_numerals: false,
                clip: Some(card.frame),
            });
        }

        layers.push(OverlayLayer {
            quads,
            labels,
            sprites,
            opacity,
            ..OverlayLayer::default()
        });
    }
    layers
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
    use crate::Motion;

    const SCALE: f32 = 1.0;
    const WINDOW: (f32, f32) = (1000.0, 700.0);
    const SEAT: SeatId = SeatId(1);

    /// A measure where every character is ten wide at any size, so widths are
    /// countable and a wrap can be reasoned about on paper.
    fn ten_per_char(run: &str, _size: f32) -> f32 {
        run.chars().count() as f32 * 10.0
    }

    fn host_with(kind: ToastKind, anchor: ToastAnchor, now: Instant) -> (ToastHost, ToastId) {
        let mut host = ToastHost::default();
        let id = host.raise(kind, anchor, None, "git said no", None, Motion::Full, now);
        (host, id)
    }

    fn opacity_of(host: &ToastHost, id: ToastId, now: Instant) -> Option<f32> {
        seen_as(host, id, now, Motion::Full)
    }

    fn seen_as(host: &ToastHost, id: ToastId, now: Instant, motion: Motion) -> Option<f32> {
        host.toasts()
            .iter()
            .find(|toast| toast.id == id)
            .map(|toast| toast.opacity(now, motion))
    }

    // ── the three clocks ───────────────────────────────────────────────────

    /// PIN — **the whole timeline of an error card, and the three instants it
    /// asks to be woken at.**
    ///
    /// Mutation: make the life run from the end of the entrance instead of from
    /// birth and the 6s assertions move by 120ms; drop the exit and the card
    /// never leaves.
    #[test]
    fn an_error_card_arrives_over_a_tenth_of_a_second_stands_six_and_leaves_in_ninety() {
        let start = Instant::now();
        let (mut host, id) = host_with(ToastKind::Error, ToastAnchor::Window, start);

        assert!(opacity_of(&host, id, start).unwrap() < 0.001, "born unseen");
        let middle = opacity_of(&host, id, start + Duration::from_millis(60)).unwrap();
        assert!(middle > 0.0 && middle < 1.0, "climbing: {middle}");
        assert!(
            (opacity_of(&host, id, start + TOAST_ENTER).unwrap() - 1.0).abs() < 0.001,
            "solid at a hundred and twenty"
        );
        // The first thing it asks for is the frame its entrance lands on.
        assert_eq!(
            host.deadline(start, Motion::Full),
            Some(start + TOAST_ENTER)
        );

        // Standing. Solid the whole way, and the wake-up it now asks for is the
        // instant its time runs out.
        let expiry = start + TOAST_LIFE_ERROR;
        let nearly = expiry - Duration::from_millis(1);
        assert!((opacity_of(&host, id, nearly).unwrap() - 1.0).abs() < 0.001);
        assert!(!host.advance(nearly, Motion::Full), "nothing is due yet");
        assert_eq!(host.deadline(nearly, Motion::Full), Some(expiry));

        // Its time is up: the exit begins, and the last wake-up is its end.
        assert!(host.advance(expiry, Motion::Full), "the exit begins");
        assert_eq!(
            host.deadline(expiry, Motion::Full),
            Some(expiry + TOAST_EXIT)
        );
        let fading = opacity_of(&host, id, expiry + TOAST_EXIT / 2).unwrap();
        assert!(fading > 0.0 && fading < 1.0, "fading: {fading}");

        assert!(
            host.advance(expiry + TOAST_EXIT, Motion::Full),
            "and it is gone"
        );
        assert!(host.is_empty());
        assert_eq!(
            host.deadline(expiry + TOAST_EXIT, Motion::Full),
            None,
            "and owes nothing"
        );
    }

    /// PIN — **time under the pointer does not count.** Three seconds of reading
    /// buys three seconds of standing, and the card's clock resumes where it was
    /// rather than restarting.
    #[test]
    fn a_card_held_under_the_pointer_for_three_seconds_stands_three_seconds_longer() {
        let start = Instant::now();
        let (mut host, id) = host_with(ToastKind::Error, ToastAnchor::Window, start);

        let entered = start + Duration::from_secs(1);
        assert!(host.hover(Some(id), entered));
        assert!(host.toasts()[0].hover_since.is_some());
        // A held card asks for no wake-ups at all: its clock is stopped, and the
        // pointer leaving is the event that starts it again.
        assert_eq!(host.deadline(entered, Motion::Full), None);

        let left = entered + Duration::from_secs(3);
        assert!(host.hover(None, left));
        assert!(host.toasts()[0].hover_since.is_none());

        let expiry = start + TOAST_LIFE_ERROR + Duration::from_secs(3);
        assert_eq!(host.deadline(left, Motion::Full), Some(expiry));
        assert!(
            !host.advance(start + TOAST_LIFE_ERROR, Motion::Full),
            "the unheld deadline has passed and the card is still standing"
        );
        assert!(
            host.advance(expiry, Motion::Full),
            "and it leaves three seconds later"
        );
        assert!(host.advance(expiry + TOAST_EXIT, Motion::Full));
        assert!(host.is_empty());
    }

    /// PIN — a quiet card stands four seconds, not six: the two lifetimes are
    /// different numbers because they are different reading jobs.
    #[test]
    fn a_confirmation_stands_four_seconds_where_a_failure_stands_six() {
        assert_eq!(ToastKind::Ok.life(), TOAST_LIFE_QUIET);
        assert_eq!(ToastKind::Info.life(), TOAST_LIFE_QUIET);
        assert_eq!(ToastKind::Error.life(), TOAST_LIFE_ERROR);

        let start = Instant::now();
        let (mut host, _) = host_with(ToastKind::Ok, ToastAnchor::Window, start);
        assert!(!host.advance(
            start + TOAST_LIFE_QUIET - Duration::from_millis(1),
            Motion::Full
        ));
        assert!(host.advance(start + TOAST_LIFE_QUIET, Motion::Full));
    }

    /// PIN — **the fourth card evicts the oldest of its own anchor, now.**
    ///
    /// And "of its own anchor" is the half a per-window cap would get wrong: a
    /// column reporting three failures must not silence a graph's first one.
    #[test]
    fn a_fourth_card_on_one_anchor_sends_that_anchors_oldest_away_at_once() {
        let start = Instant::now();
        let mut host = ToastHost::default();
        let column = ToastAnchor::FilesColumn(SEAT);
        let ids: Vec<ToastId> = (0..3)
            .map(|n| {
                host.raise(
                    ToastKind::Error,
                    column,
                    None,
                    format!("{n}"),
                    None,
                    Motion::Full,
                    start,
                )
            })
            .collect();
        // A card on another anchor is not part of this crowd.
        let elsewhere = host.raise(
            ToastKind::Error,
            ToastAnchor::PreviewSeat(LeafId {
                tab: crate::TabId(1),
                seat: SEAT,
            }),
            None,
            "graph",
            None,
            Motion::Full,
            start,
        );
        assert!(host.toasts().iter().all(|toast| !toast.leaving()));

        let fourth = start + Duration::from_millis(500);
        let last = host.raise(
            ToastKind::Error,
            column,
            None,
            "fourth",
            None,
            Motion::Full,
            fourth,
        );
        let leaving: Vec<ToastId> = host
            .toasts()
            .iter()
            .filter(|toast| toast.leaving())
            .map(|toast| toast.id)
            .collect();
        assert_eq!(
            leaving,
            vec![ids[0]],
            "the oldest of that anchor, and only it"
        );
        assert!(host.toasts().iter().any(|toast| toast.id == elsewhere));
        assert!(host.toasts().iter().any(|toast| toast.id == last));

        // And it is gone once its own exit has run, leaving three again.
        assert!(host.advance(fourth + TOAST_EXIT, Motion::Full));
        assert_eq!(
            host.toasts()
                .iter()
                .filter(|toast| toast.anchor() == column)
                .count(),
            3
        );
    }

    /// PIN — the `×` starts the exit at once, from wherever the card had got to,
    /// and a card dismissed twice does not restart its fade.
    #[test]
    fn dismissing_a_card_sends_it_away_from_where_it_stood() {
        let start = Instant::now();
        let (mut host, id) = host_with(ToastKind::Error, ToastAnchor::Window, start);
        // Forty milliseconds in — a third of the way up.
        let early = start + Duration::from_millis(40);
        let stood = opacity_of(&host, id, early).unwrap();
        assert!(stood < 1.0);
        assert!(host.dismiss(id, early, Motion::Full));
        assert!(
            opacity_of(&host, id, early).unwrap() <= stood + 0.001,
            "it does not brighten on the way out"
        );
        assert!(!host.dismiss(id, early + Duration::from_millis(10), Motion::Full));
        assert!(host.advance(early + TOAST_EXIT, Motion::Full));
        assert!(host.is_empty());
        assert!(!host.dismiss(id, early + TOAST_EXIT, Motion::Full));
    }

    /// Stillness lands the card where it is going on the frame it is born, and
    /// then it owes nothing until its own time is up.
    /// PIN — **a still window gets the card, and none of the journey.**
    ///
    /// The card is whole on the frame it is born, does not travel, and asks for
    /// no frame at all until its life runs out — no entrance to land, no exit to
    /// fade. The life itself is the plain six seconds from birth, which is the
    /// half the old arrangement got wrong: it obtained the still first frame by
    /// back-dating the card's birth by an entrance, so a notice under reduced
    /// motion stood for a hundred and forty milliseconds less than one beside it.
    ///
    /// Mutation: read the preference anywhere but in `opacity`/`slide`/`deadline`
    /// and one of these three lines fails.
    #[test]
    fn a_still_window_gets_the_card_without_the_journey() {
        let start = Instant::now();
        let mut host = ToastHost::default();
        let id = host.raise(
            ToastKind::Error,
            ToastAnchor::Window,
            None,
            "no",
            None,
            Motion::Reduced,
            start,
        );
        assert!((seen_as(&host, id, start, Motion::Reduced).unwrap() - 1.0).abs() < 0.001);
        let toast = &host.toasts()[0];
        assert!(
            toast.slide(start, Motion::Reduced).abs() < 0.001,
            "and it does not travel"
        );
        assert_eq!(
            host.deadline(start, Motion::Reduced),
            Some(start + TOAST_LIFE_ERROR),
            "the only instant it wants is the end of its life"
        );
    }

    /// PIN — **under reduced motion a card that has been sent away is gone, not
    /// fading.**
    ///
    /// The two halves are one promise: nothing decorative is drawn, and nothing
    /// decorative is *waited for*. A card left on the list at zero opacity would
    /// satisfy the first and break the second, and would also hold one of the
    /// three places its anchor has.
    ///
    /// Mutation: retire only from `advance` and the dismissed card lingers
    /// invisibly; give the exit its span back and the deadline reappears.
    #[test]
    fn a_still_window_takes_a_dismissed_card_away_at_once_and_waits_for_nothing() {
        let start = Instant::now();
        let mut host = ToastHost::default();
        let id = host.raise(
            ToastKind::Error,
            ToastAnchor::Window,
            None,
            "no",
            None,
            Motion::Reduced,
            start,
        );
        assert!(host.dismiss(id, start, Motion::Reduced), "it was there");
        assert!(host.is_empty(), "and it is not there now");
        assert_eq!(host.deadline(start, Motion::Reduced), None);

        // The same card in a window that wants animation keeps both halves.
        let mut moving = ToastHost::default();
        let id = moving.raise(
            ToastKind::Error,
            ToastAnchor::Window,
            None,
            "no",
            None,
            Motion::Full,
            start,
        );
        assert!(moving.dismiss(id, start, Motion::Full));
        assert!(!moving.is_empty(), "it is still leaving");
        assert_eq!(
            moving.deadline(start, Motion::Full),
            Some(start + TOAST_EXIT)
        );
    }

    /// The entrance travels toward the edge it came from, and only the entrance.
    #[test]
    fn a_card_falls_from_the_head_it_hangs_off_and_rises_from_the_window_floor() {
        let start = Instant::now();
        let (anchored, _) = host_with(ToastKind::Error, ToastAnchor::FilesColumn(SEAT), start);
        let (corner, id) = host_with(ToastKind::Error, ToastAnchor::Window, start);
        assert!(
            (anchored.toasts()[0].slide(start, Motion::Full) + TOAST_SLIDE_LOGICAL_PX).abs()
                < 0.001,
            "an anchored card starts above its place"
        );
        assert!(
            (corner.toasts()[0].slide(start, Motion::Full) - TOAST_SLIDE_LOGICAL_PX).abs() < 0.001,
            "the corner card starts below its place"
        );
        assert!(
            anchored.toasts()[0]
                .slide(start + TOAST_ENTER, Motion::Full)
                .abs()
                < 0.001
        );

        let mut corner = corner;
        corner.dismiss(id, start + TOAST_ENTER, Motion::Full);
        assert!(
            corner.toasts()[0]
                .slide(start + TOAST_ENTER + TOAST_EXIT / 2, Motion::Full)
                .abs()
                < 0.001,
            "and nothing travels on the way out"
        );
    }

    // ── where they land ────────────────────────────────────────────────────

    fn placed(host: &ToastHost, body: Option<[f32; 4]>) -> Vec<ToastLayout> {
        place(
            host.toasts(),
            |anchor| match anchor {
                ToastAnchor::Window => None,
                _ => body,
            },
            WINDOW,
            SCALE,
            &mut ten_per_char,
        )
    }

    /// PIN — **a card hangs eight pixels inside the top of the body its action
    /// came from**, as wide as that body less eight each side, and a second card
    /// stacks below it with an eight-pixel gap.
    ///
    /// This is the 2026-08-16 amendment in one test: the first draft put it in
    /// the window's corner, and what the ruling bought is that the notice is
    /// where the hand already was.
    #[test]
    fn a_card_hangs_inside_the_top_of_the_body_that_raised_it() {
        let start = Instant::now();
        let body = [100.0, 60.0, 400.0, 600.0];
        let mut host = ToastHost::default();
        let first = host.raise(
            ToastKind::Error,
            ToastAnchor::FilesColumn(SEAT),
            None,
            "one",
            None,
            Motion::Full,
            start,
        );
        let second = host.raise(
            ToastKind::Error,
            ToastAnchor::FilesColumn(SEAT),
            None,
            "two",
            None,
            Motion::Full,
            start,
        );
        let laid = placed(&host, Some(body));
        assert_eq!(laid.len(), 2);
        assert_eq!(laid[0].id, first, "oldest at the top");
        assert_eq!(laid[1].id, second, "newest at the bottom");

        assert!(
            (laid[0].frame[0] - (body[0] + 8.0)).abs() < 0.001,
            "{laid:?}"
        );
        assert!(
            (laid[0].frame[2] - (body[2] - 8.0)).abs() < 0.001,
            "{laid:?}"
        );
        assert!(
            (laid[0].frame[1] - (body[1] + 8.0)).abs() < 0.001,
            "{laid:?}"
        );
        assert!(
            (laid[1].frame[1] - (laid[0].frame[3] + 8.0)).abs() < 0.001,
            "stacked downward with an eight-pixel gap"
        );
        // It overlays the body; it does not carve anything out of it.
        assert!(laid[1].frame[3] < body[3]);
    }

    /// PIN — the corner is the fallback and nothing else: a card whose surface
    /// cannot be found this frame stands sixteen off the window's right and
    /// bottom, at the tip's own width, with the newest lowest.
    #[test]
    fn a_card_with_no_surface_left_falls_to_the_window_corner() {
        let start = Instant::now();
        let mut host = ToastHost::default();
        let first = host.raise(
            ToastKind::Error,
            ToastAnchor::FilesColumn(SEAT),
            None,
            "one",
            None,
            Motion::Full,
            start,
        );
        let second = host.raise(
            ToastKind::Error,
            ToastAnchor::FilesColumn(SEAT),
            None,
            "two",
            None,
            Motion::Full,
            start,
        );
        // The column has closed: its rectangle cannot be resolved.
        let laid = placed(&host, None);
        assert_eq!(laid.len(), 2);
        assert!((laid[0].frame[2] - (WINDOW.0 - 16.0)).abs() < 0.001);
        assert!(
            (laid[1].frame[3] - (WINDOW.1 - 16.0)).abs() < 0.001,
            "the newest is on the floor"
        );
        assert!(
            laid[0].frame[3] < laid[1].frame[1],
            "the older one is above it"
        );
        assert!(
            (laid[1].frame[1] - (laid[0].frame[3] + 8.0)).abs() < 0.001,
            "the same eight-pixel gap"
        );
        assert!((laid[0].frame[2] - laid[0].frame[0] - 360.0).abs() < 0.001);
        assert_eq!((laid[0].id, laid[1].id), (first, second));
    }

    /// PIN — a body too narrow to give the card its floor is taken whole rather
    /// than obeyed, and the card never leaves the body it hangs in.
    #[test]
    fn a_narrow_column_gives_the_card_all_it_has_and_no_more() {
        let start = Instant::now();
        let (host, _) = host_with(ToastKind::Error, ToastAnchor::FilesColumn(SEAT), start);

        // 210 wide: eight each side would leave 194, under the floor of 200.
        let squeezed = placed(&host, Some([0.0, 0.0, 210.0, 400.0]));
        let width = squeezed[0].frame[2] - squeezed[0].frame[0];
        assert!((width - 200.0).abs() < 0.5, "the floor holds: {width}");
        assert!(squeezed[0].frame[0] >= 0.0 && squeezed[0].frame[2] <= 210.0);

        // 140 wide: even the floor will not fit, so the card takes the body.
        let tiny = placed(&host, Some([0.0, 0.0, 140.0, 400.0]));
        let width = tiny[0].frame[2] - tiny[0].frame[0];
        assert!(
            (width - 140.0).abs() < 0.5,
            "never wider than the body: {width}"
        );
        assert!(tiny[0].frame[0] >= 0.0 && tiny[0].frame[2] <= 140.0);
    }

    /// PIN — the body wraps inside the card's own text column and stops at six
    /// lines, and the sixth says that it stopped.
    #[test]
    fn a_long_sentence_wraps_at_the_cards_measure_and_the_sixth_line_admits_the_rest() {
        let start = Instant::now();
        let mut host = ToastHost::default();
        // Twenty words of ten characters each, in a card whose text column is
        // narrow enough that no more than two fit on a line.
        let words: Vec<String> = (0..20).map(|n| format!("word{n:0>6}")).collect();
        host.raise(
            ToastKind::Error,
            ToastAnchor::FilesColumn(SEAT),
            None,
            words.join(" "),
            None,
            Motion::Full,
            start,
        );
        let laid = placed(&host, Some([0.0, 0.0, 260.0, 600.0]));
        let card = &laid[0];
        assert_eq!(card.lines.len(), TOAST_MAX_LINES, "capped at six");
        assert!(
            card.lines.last().unwrap().1.ends_with('…'),
            "and it says it was cut: {:?}",
            card.lines.last()
        );
        let column = card.lines[0].0[2] - card.lines[0].0[0];
        for (rect, line) in &card.lines {
            assert!(
                ten_per_char(line.trim_end_matches('…'), 12.0) <= column + 0.001,
                "no line is wider than the text column: {line:?}"
            );
            assert!(rect[0] >= card.frame[0] && rect[2] <= card.frame[2]);
        }
        // The text column starts past the mark and stops before the `×`.
        assert!(card.lines[0].0[0] > card.mark[2]);
        assert!(card.lines[0].0[2] <= card.close[0]);
    }

    /// A title takes the first line and the mark lines up with *it*, not with
    /// the sentence under it.
    #[test]
    fn a_titled_card_leads_with_the_title_and_the_mark_stands_beside_it() {
        let start = Instant::now();
        let mut host = ToastHost::default();
        host.raise(
            ToastKind::Error,
            ToastAnchor::FilesColumn(SEAT),
            Some("Git".to_owned()),
            "fatal: no",
            None,
            Motion::Full,
            start,
        );
        let laid = placed(&host, Some([0.0, 0.0, 300.0, 400.0]));
        let card = &laid[0];
        let (title_rect, title) = card.title.as_ref().expect("a title");
        assert_eq!(title, "Git");
        assert!(
            title_rect[3] <= card.lines[0].0[1] + 0.001,
            "above the body"
        );
        let mark_middle = (card.mark[1] + card.mark[3]) / 2.0;
        let title_middle = (title_rect[1] + title_rect[3]) / 2.0;
        assert!(
            (mark_middle - title_middle).abs() <= 1.0,
            "beside the title"
        );
    }

    // ── the hit test ───────────────────────────────────────────────────────

    /// PIN — **one derivation for the paint and the press.** The `×` answers
    /// `Close`, the rest of the card answers `Card` — which is not `None`,
    /// because a toast is not click-through — and outside answers nothing.
    #[test]
    fn the_dismiss_box_answers_close_the_rest_of_the_card_answers_card() {
        let start = Instant::now();
        let (host, id) = host_with(ToastKind::Error, ToastAnchor::FilesColumn(SEAT), start);
        let laid = placed(&host, Some([100.0, 60.0, 400.0, 600.0]));
        let card = &laid[0];

        let middle = |rect: [f32; 4]| ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
        let (cx, cy) = middle(card.close);
        assert_eq!(at(&laid, cx, cy), Some(ToastHit::Close(id)));
        // The reserved box is exactly where the drawn one is, so this holds
        // whether or not the `×` is revealed this frame.
        assert!(card.close[2] <= card.frame[2] && card.close[1] >= card.frame[1]);

        assert_eq!(
            at(&laid, card.frame[0] + 2.0, card.frame[3] - 2.0),
            Some(ToastHit::Card(id)),
            "the body of the card is the card's"
        );
        assert_eq!(at(&laid, card.frame[0] - 1.0, cy), None);
        assert_eq!(at(&laid, cx, card.frame[3] + 1.0), None);
        assert!(
            card.action.is_none(),
            "a card that was raised without a verb has no box for one"
        );
    }

    /// PIN — **the two pressable things on a card never share a pixel**, and
    /// the verb never reaches the `×`'s column however long its word is.
    ///
    /// Red gate: right-align the verb on the card's own inner edge rather than
    /// on the text column's, and the two boxes overlap on every card — at which
    /// point the hit test's arm order silently becomes a precedence, and a press
    /// meant for `Undo` dismisses the only thing that could have undone it.
    ///
    /// The long word is not decoration either: it is what proves the clamp is a
    /// clamp. A verb wider than the column it hangs in gives ground at its
    /// *leading* edge, exactly as the wrapped body does, rather than growing out
    /// of the card.
    #[test]
    fn the_two_pressable_things_on_a_card_never_share_a_pixel() {
        let start = Instant::now();
        for word in [
            "Undo",
            "Undo this deletion and put the row back where it was",
        ] {
            let mut host = ToastHost::default();
            let id = host.raise(
                ToastKind::Info,
                ToastAnchor::FilesColumn(SEAT),
                None,
                "PowerShell 7 copy is gone.",
                Some(word.to_owned()),
                Motion::Full,
                start,
            );
            let laid = placed(&host, Some([0.0, 0.0, 300.0, 400.0]));
            let card = &laid[0];
            let (action, drawn) = card.action.as_ref().expect("a verb");
            assert_eq!(drawn, word);
            assert!(
                action[2] <= card.close[0],
                "{word:?}: the verb ends at {} and the × begins at {}",
                action[2],
                card.close[0]
            );
            assert!(
                action[0] >= card.frame[0] && action[3] <= card.frame[3],
                "{word:?}: the verb stays inside the card it belongs to"
            );
            assert!(
                action[1] >= card.lines.last().expect("a body").0[3],
                "{word:?}: and it stands under the sentence, not across it"
            );

            let middle = |rect: [f32; 4]| ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
            let (ax, ay) = middle(*action);
            assert_eq!(at(&laid, ax, ay), Some(ToastHit::Action(id)));
            let (cx, cy) = middle(card.close);
            assert_eq!(at(&laid, cx, cy), Some(ToastHit::Close(id)));
            assert_eq!(
                at(&laid, action[0] - 2.0, ay),
                Some(ToastHit::Card(id)),
                "{word:?}: beside the verb is the card, and the card is not click-through"
            );
        }
    }

    /// PIN — **a card grows by exactly one row for its verb, and a card without
    /// one is laid out as it was before this slice existed.**
    ///
    /// Red gate: add the verb's height unconditionally, and every notice in the
    /// product — none of which has a verb — gains a band of empty card under its
    /// last line.
    #[test]
    fn a_verb_costs_one_row_and_a_card_without_one_costs_nothing() {
        let start = Instant::now();
        let body = Some([0.0, 0.0, 300.0, 400.0]);
        let mut plain = ToastHost::default();
        plain.raise(
            ToastKind::Info,
            ToastAnchor::FilesColumn(SEAT),
            None,
            "PowerShell 7 copy is gone.",
            None,
            Motion::Full,
            start,
        );
        let mut verbed = ToastHost::default();
        verbed.raise(
            ToastKind::Info,
            ToastAnchor::FilesColumn(SEAT),
            None,
            "PowerShell 7 copy is gone.",
            Some("Undo".to_owned()),
            Motion::Full,
            start,
        );
        let plain = placed(&plain, body).remove(0);
        let verbed = placed(&verbed, body).remove(0);
        assert_eq!(
            plain.lines, verbed.lines,
            "the sentence falls in the same place either way"
        );
        let row = (TOAST_ACTION_FONT_LOGICAL_PX * CHROME_LINE_HEIGHT).round()
            + TOAST_ACTION_MARGIN_TOP_LOGICAL_PX;
        assert!(
            ((verbed.frame[3] - verbed.frame[1]) - (plain.frame[3] - plain.frame[1]) - row).abs()
                < 0.001,
            "one row taller and no more: {plain:?} against {verbed:?}"
        );
    }

    // ── the paint ──────────────────────────────────────────────────────────

    /// One layer per card, each carrying its own opacity; a card faded to nothing
    /// is not drawn at all.
    #[test]
    fn every_card_is_its_own_layer_wearing_its_own_fade() {
        let palette = bt_render::chrome_palette();
        let start = Instant::now();
        let mut host = ToastHost::default();
        let first = host.raise(
            ToastKind::Error,
            ToastAnchor::FilesColumn(SEAT),
            Some("Git".to_owned()),
            "fatal: no",
            None,
            Motion::Full,
            start,
        );
        // Born an entrance later, so on the frame below it is exactly at zero
        // while the card above it is exactly whole.
        let arrived = start + TOAST_ENTER;
        host.raise(
            ToastKind::Ok,
            ToastAnchor::FilesColumn(SEAT),
            None,
            "done",
            None,
            Motion::Full,
            arrived,
        );
        let laid = placed(&host, Some([100.0, 60.0, 400.0, 600.0]));
        let layers = build(
            &laid,
            &host,
            ToastPointer {
                card: Some(first),
                close: Some(first),
                action: None,
            },
            &palette,
            SCALE,
            arrived,
            Motion::Full,
        );
        // The second card is at zero on the frame it is born, so it draws nothing.
        assert_eq!(layers.len(), 1, "a card at zero is not a layer");
        let layer = &layers[0];
        assert!((layer.opacity - 1.0).abs() < 0.001);
        assert_eq!(layer.labels.len(), 2, "a title and one line");
        assert_eq!(layer.labels[0].weight, ChromeLabelWeight::Medium);
        assert_eq!(layer.labels[0].color, palette.menu_item_text_selected);
        assert_eq!(layer.labels[1].color, palette.menu_item_text);
        assert!(
            layer
                .quads
                .iter()
                .any(|quad| quad.color == palette.menu_surface),
            "the menu's own face"
        );
        // The kind's dot, the lit `×`'s pill and the `×` itself.
        assert_eq!(layer.sprites.len(), 3);
        assert_eq!(layer.sprites[0].color, palette.status_err, "the rose");
        assert!((layer.sprites.last().unwrap().opacity - 1.0).abs() < 0.001);
    }

    /// The three kinds are three inks and one mark — a dot, `.tdot`, in the
    /// kind's ink — and none of the inks is a literal: every one comes out of
    /// the palette. **One dot and nothing in a round** (user ruling 2026-08-16):
    /// a circle in this house is a state, never a container for an icon, and a
    /// `×` on a card is only ever the dismiss verb.
    #[test]
    fn each_kind_wears_its_own_ink_on_one_dot() {
        let palette = bt_render::chrome_palette();
        assert_eq!(ToastKind::Error.ink(&palette), palette.status_err);
        assert_eq!(ToastKind::Info.ink(&palette), palette.accent);
        assert_eq!(ToastKind::Ok.ink(&palette), palette.status_ok);

        let start = Instant::now();
        for kind in [ToastKind::Error, ToastKind::Ok, ToastKind::Info] {
            let mut host = ToastHost::default();
            host.raise(
                kind,
                ToastAnchor::Window,
                None,
                "x",
                None,
                Motion::Reduced,
                start,
            );
            let laid = placed(&host, None);
            let layers = build(
                &laid,
                &host,
                ToastPointer::default(),
                &palette,
                SCALE,
                start,
                Motion::Reduced,
            );
            let marks: Vec<&ChromeSprite> = layers[0].sprites.iter().collect();
            assert_eq!(marks.len(), 1, "{kind:?}: the dot and nothing else at rest");
            assert!(
                matches!(marks[0].mark, ChromeMark::ControlPill { .. }),
                "{kind:?}: a filled round"
            );
            assert_eq!(marks[0].color, kind.ink(&palette), "{kind:?}");
            let side = marks[0].rect[2] - marks[0].rect[0];
            assert!(
                (side - TOAST_DOT_LOGICAL_PX * SCALE).abs() < 0.001,
                "{kind:?}: six logical pixels across, {side}"
            );
            assert!(
                !layers[0]
                    .sprites
                    .iter()
                    .any(|s| s.mark == ChromeMark::TabClose),
                "{kind:?}: no × anywhere on a card nobody is pointing at"
            );
        }
    }

    /// The `×` climbs `.pv-tool`'s ladder: absent, seven-tenths, whole.
    #[test]
    fn the_dismiss_verb_is_absent_until_the_card_is_pointed_at() {
        let palette = bt_render::chrome_palette();
        let start = Instant::now();
        let (host, id) = host_with(ToastKind::Error, ToastAnchor::Window, start);
        let laid = placed(&host, None);
        let sprites = |pointer| {
            build(
                &laid,
                &host,
                pointer,
                &palette,
                SCALE,
                start + TOAST_ENTER,
                Motion::Full,
            )[0]
            .sprites
            .clone()
        };

        assert_eq!(sprites(ToastPointer::default()).len(), 1, "the dot only");
        let over_card = sprites(ToastPointer {
            card: Some(id),
            close: None,
            action: None,
        });
        assert_eq!(over_card.len(), 2);
        assert!(
            (over_card[1].opacity - crate::seats::PREVIEW_TOOL_REVEAL).abs() < 0.001,
            "seven-tenths once the card has the pointer"
        );
        let over_button = sprites(ToastPointer {
            card: Some(id),
            close: Some(id),
            action: None,
        });
        assert_eq!(over_button.len(), 3, "and it gains a pill of its own");
        assert!((over_button[2].opacity - 1.0).abs() < 0.001);
    }

    /// PIN — **what a refused git verb actually puts on the glass**: one error
    /// card, over the column that asked, carrying git's sentence whole.
    ///
    /// The runtime half of this — *which* answers raise a notice and which stay
    /// silent — is pinned beside the wiring, in `main.rs`'s
    /// `a_refused_verb_raises_one_notice_and_a_read_that_failed_raises_none`.
    /// What is pinned here is the other half: given those words, the host makes
    /// exactly one card of them, and the card says all of it.
    ///
    /// The sentence is the one from the user's report — the case that started
    /// this whole surface, which the red banner it replaced cut off after four
    /// words in a 240-pixel column.
    #[test]
    fn a_refused_checkout_becomes_one_card_over_the_column_that_asked() {
        let words = "fatal: 't1-tab-basics' is already used by worktree at D:/Developer/x";
        let start = Instant::now();
        let mut host = ToastHost::default();
        host.raise(
            ToastKind::Error,
            ToastAnchor::FilesColumn(SEAT),
            Some("Git".to_owned()),
            words,
            None,
            Motion::Full,
            start,
        );
        assert_eq!(host.toasts().len(), 1, "one answer, one card");
        let card = &host.toasts()[0];
        assert_eq!(card.kind, ToastKind::Error);
        assert_eq!(card.anchor(), ToastAnchor::FilesColumn(SEAT));
        assert_eq!(card.title.as_deref(), Some("Git"));
        assert_eq!(card.body, words, "git's own sentence, not a summary of it");

        // And on a column of a realistic width it is *read*, not truncated: the
        // banner had one line of about 240 pixels, and this has as many lines as
        // the sentence needs, up to six.
        let laid = placed(&host, Some([0.0, 60.0, 260.0, 600.0]));
        let said: String = laid[0]
            .lines
            .iter()
            .map(|(_, line)| line.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(said, words, "every word of it is on the card");
        assert!(laid[0].lines.len() > 1, "which took more than one line");
    }

    /// The frame-debt reading: what should be on screen, id by id.
    #[test]
    fn the_frame_state_reports_every_card_and_what_it_should_look_like() {
        let start = Instant::now();
        let (host, id) = host_with(ToastKind::Error, ToastAnchor::Window, start);
        let state = host.frame_state(start + TOAST_ENTER, Motion::Full);
        assert_eq!(state.len(), 1);
        assert_eq!(state[0].0, id);
        assert!((state[0].1 - 1.0).abs() < 0.001);
        assert_ne!(
            host.frame_state(start, Motion::Full),
            state,
            "and it moves with the fade"
        );
    }
}
