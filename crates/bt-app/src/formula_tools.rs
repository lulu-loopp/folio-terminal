//! **A formula band's two verbs, as house marks** (owner's ruling 2026-09-14;
//! `docs/DESIGN.md` §7.1.5p).
//!
//! The band itself is a terminal object and `bt_render` draws it: the raster,
//! the ground under it, the overflow fades at its two ends, and — since this
//! module — the two boxes the marks stand in
//! ([`bt_render::WindowRenderer::math_tool_boxes`]). What lives here is the
//! other half, and it lives here because it *has* to: a mark in this window is
//! a drawing quoted from `docs/design/ui-mockup.html` and rasterized in
//! [`crate::marks`], and that module is a crate above the renderer.
//!
//! Until this ruling the two verbs were drawn in `bt_render` as a pair of small
//! square bordered chips with an eye and two overlapping outlines struck inside
//! them out of hand-placed rectangles. That is two separate faults. The chip is
//! this window's *floating tag* — what a tooltip and a hover-address bubble
//! wear — and a verb you press is not a tag, it is a control, and every other
//! control here is a mark on a pill. And the drawings were second cuts of three
//! marks the house already owns (`#i-code`, `#i-eye`, `#i-copy`), which is the
//! drift [`crate::marks`]'s own header exists to forbid.
//!
//! **Since the owner's ruling of 2026-09-15 ② the two marks stand *inside* the
//! block**, in the whole cell columns of ground it keeps at its right edge, on
//! its midline, and they are the pane head's own button — the same box, the
//! same corner, the same ink. Nothing about that is decided here either: the
//! seat is `bt_render`'s `math_tool_boxes_px`, and what this module holds is
//! the pin that the box really is the head's
//! (`tests::a_bands_marks_wear_the_pane_heads_own_button`), because this is the
//! one crate that can see both constants.
//!
//! **Nothing in this module decides where a mark goes** — the renderer does,
//! from the band's own geometry. [`sprites`] is handed the boxes, what the
//! pointer is doing, and one opacity, and it answers with sprites; that is the
//! same division `seats.rs` keeps with the solver, and it is what lets every
//! clause of the ruling be pinned without a GPU.
//!
//! **What the module does hold, since the owner's report of the evening of
//! 2026-09-14 (T-MATH-TOOLS-FOLLOW), is the marks' own two clocks**
//! ([`FormulaToolFollow`]). The renderer still answers *where the boxes are on
//! this picture*; what nobody answered was what the marks should do when that
//! answer changes under a pointer that has not moved — a press on `‹›` makes the
//! block taller, a re-wrap moves its rows, a scale change resizes everything —
//! and "be re-struck somewhere else" is not an answer a window this size is
//! allowed to give. The follow is still nothing but arithmetic over instants,
//! for the same reason the drawing is: every clause of this half is pinned
//! without a GPU too.

use std::time::Instant;

use bt_render::{
    ChromeLabel, ChromeLabelWeight, ChromePalette, MATH_TOOL_PILL_RADIUS_LOGICAL_PX, MathToolBoxes,
};
use bt_viewport::{MathBlockAnchor, MathBlockDisplay};

use crate::{
    Motion, PasteTarget,
    icons::MarkSlot,
    marks::{ChromeMark, ChromeSprite},
    tooltip,
};

/// Which of a band's two marks a gesture is on.
///
/// [`bt_render::MathHitTarget`]'s two verb arms, without the two that are not
/// verbs (`Block` and `Failure`): this module draws controls, and a press on
/// the formula itself is not one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormulaTool {
    /// Turn the rendered formula back into its `$$…$$` source, or back again.
    ToggleSource,
    /// Put the original LaTeX on the clipboard.
    CopyLatex,
}

/// **What the pointer and the clipboard have to say about one band's marks.**
///
/// Three independent facts and not a state machine: a mark can be hovered and
/// held at once, and the copy mark can be showing its tick while the pointer has
/// already moved to the other one.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FormulaToolState {
    /// The mark under the pointer, if the pointer is on one rather than on the
    /// band.
    pub hovered: Option<FormulaTool>,
    /// The mark a button is being held down on.
    pub pressed: Option<FormulaTool>,
    /// Whether the copy mark is inside its acknowledgement window and should be
    /// wearing the tick instead of the sheets.
    pub copied: bool,
}

/// **The mark each verb wears right now.**
///
/// The first is the markdown flip's own rule, read on a formula: the drawing
/// names the view you are going *to*, not the one you are in (mock-up 2170-2171,
/// and `crate::float` reads the identical pair for the preview head's flip). A
/// rendered formula therefore wears `#i-code` — press it and you get source —
/// and a source block wears `#i-eye`.
///
/// The second is the acknowledgement: `#i-check` for as long as the caller says
/// the copy is fresh, which is the same swap the files foot makes in place of
/// its folder glyph, on the same clock (`FOOT_REVEAL_FEEDBACK`).
#[must_use]
pub fn marks(display: MathBlockDisplay, copied: bool) -> [ChromeMark; 2] {
    [
        if display == MathBlockDisplay::Rendered {
            ChromeMark::Code
        } else {
            ChromeMark::Eye
        },
        if copied {
            ChromeMark::Check
        } else {
            ChromeMark::Copy
        },
    ]
}

/// **One band's marks, and the pills under the ones being touched.**
///
/// `boxes` is in physical pixels of the whole surface — the renderer answers in
/// the pane body's own pixels and the caller has already moved them to the
/// body's corner. `opacity` is the hover fade
/// ([`crate::tooltip::hover_fade_opacity`]); at zero this draws nothing at all,
/// which is the ruling's "at rest they are NOT drawn" expressed where it cannot
/// be forgotten rather than at each call site.
///
/// **The pill is drawn only under a mark that is being touched.** `.math-tools
/// button { border: none; background: none }` (mock-up 2127-2129) is the whole
/// of the resting state, exactly as `.newtab` in the strip is a glyph on the
/// title bar until the pointer arrives.
#[must_use]
pub fn sprites(
    boxes: &MathToolBoxes,
    state: FormulaToolState,
    palette: &ChromePalette,
    scale: f32,
    opacity: f32,
) -> Vec<ChromeSprite> {
    if opacity <= 0.0 {
        return Vec::new();
    }
    let radius_px = (MATH_TOOL_PILL_RADIUS_LOGICAL_PX * scale).round().max(1.0) as u32;
    let [source_mark, copy_mark] = marks(boxes.display, state.copied);
    let mut sprites = Vec::with_capacity(4);
    for (box_, verb, mark) in [
        (boxes.source, FormulaTool::ToggleSource, source_mark),
        (boxes.copy, FormulaTool::CopyLatex, copy_mark),
    ] {
        let pressed = state.pressed == Some(verb);
        let hovered = state.hovered == Some(verb);
        if pressed || hovered {
            sprites.push(
                ChromeSprite::new(
                    ChromeMark::ControlPill { radius_px },
                    box_,
                    // `--active` under a held mark and `--hover` under a merely
                    // lit one: the press is the darker of the strip's two washes
                    // and the hover is the one the strip's own `+`/`˅` pair
                    // wears, read on the terminal's surface.
                    if pressed {
                        palette.formula_tool_pill_pressed
                    } else {
                        palette.formula_tool_pill
                    },
                )
                .with_opacity(opacity),
            );
        }
        let [width, height] = MarkSlot::CompactHead.mark_box_logical_px(mark);
        let width = (width * scale).round().max(1.0);
        let height = (height * scale).round().max(1.0);
        let left = ((box_[0] + box_[2] - width) / 2.0).round();
        let top = ((box_[1] + box_[3] - height) / 2.0).round();
        let ink = if mark == ChromeMark::Check {
            // `.fly-foot.done { color: var(--accent) }` — an acknowledgement is
            // the one moment a mark in this family is allowed to carry a colour,
            // and it is the colour the float's own foot already confirms in.
            palette.accent
        } else if pressed || hovered {
            palette.formula_tool_glyph_on_pill
        } else {
            palette.formula_tool_glyph
        };
        sprites.push(
            ChromeSprite::new(mark, [left, top, left + width, top + height], ink)
                .with_opacity(opacity),
        );
    }
    sprites
}

/// **The `$$…$$` source, drawn over the band while the two faces cross-fade** (owner's ruling
/// 2026-09-15, T-MATH-TOGGLE-MOTION; `docs/DESIGN.md` §7.1.5p ⑪).
///
/// One label per row, in the **terminal's** face and the terminal's ink, at the positions the rows
/// themselves will take — which is the whole of the correctness here: on the frame the change
/// lands, the block's presented height is the rows' own height and these labels stand exactly where
/// the transcript rows the document is about to reveal will stand, so the switch is invisible.
/// [`bt_render::MathBandFace`] is where every one of those numbers comes from, so nothing about a
/// row's place is decided in this module, exactly as nothing about a mark's is.
///
/// **Clipped down to the band and across to the pane.** A source row is an ordinary row of this
/// terminal: it begins at column zero and runs to the pane's edge, not to the edge of the ground
/// the picture kept around itself. What it may not do is spill onto the lines above and below the
/// block — the band is still growing towards the room these rows need — so the clip is the band's
/// own top and bottom and the pane's own left and right.
///
/// The fade itself is not here: these go on a layer, and a layer has an `opacity` (§7.1.5p ②'s own
/// arrangement for the marks). One thing fading, and not a row at a time.
#[must_use]
pub fn source_face_labels(
    face: &bt_render::MathBandFace,
    rows: &[String],
    ink: [u8; 3],
    font_size_px: f32,
) -> Vec<ChromeLabel> {
    let [_, band_top, _, band_bottom] = face.block;
    if band_bottom <= band_top || face.rows_right <= face.rows_left {
        return Vec::new();
    }
    let mut labels = Vec::with_capacity(rows.len());
    for (index, text) in rows.iter().enumerate() {
        if text.is_empty() {
            continue;
        }
        let top = face.rows_top + index as f32 * face.row_height;
        let bottom = top + face.row_height;
        // A row the band has not grown far enough to show yet, or one scrolled past the pane's
        // edge: the clip below would draw nothing of it anyway, and an empty draw still costs a
        // shaping pass.
        if bottom <= band_top || top >= band_bottom {
            continue;
        }
        labels.push(ChromeLabel {
            text: text.clone(),
            rect: [face.rows_left, top, face.rows_right, bottom],
            clip: Some([
                face.rows_left,
                top.max(band_top),
                face.rows_right,
                bottom.min(band_bottom),
            ]),
            font_size_px,
            color: ink,
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            mono: true,
        });
    }
    labels
}

/// **One value on its way to another, on the window's own fast ease.**
///
/// The curve, the span and the reduced-motion answer are all the tip's
/// ([`crate::tooltip::hover_fade_opacity`]) — read here as *how far along a
/// journey is* rather than as ink, which is the whole of what the ruling's "move
/// with the same short ease" is as arithmetic. One function and not a second
/// curve beside it: a mark that slides to a new place must not end up sounding
/// different from the fade that put it there, and stillness is honoured once,
/// where that function already honours it.
///
/// **A journey is retargeted, never restarted from where it was going.**
/// [`Ease::retarget`] takes the value the ease is showing *at this instant* as
/// the new start, so a band whose geometry moves twice inside ninety
/// milliseconds — a toggle that re-wraps and then settles — travels from where
/// the marks actually are rather than snapping back to where the first journey
/// began.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Ease<T> {
    from: T,
    to: T,
    since: Instant,
}

impl<T: Lerp> Ease<T> {
    /// A value that is not going anywhere.
    fn settled(value: T, now: Instant) -> Self {
        Self {
            from: value,
            to: value,
            since: now,
        }
    }

    /// Where the journey is at `now` — and **exactly** its end once it has
    /// landed, which under [`Motion::Reduced`] is the frame it began.
    ///
    /// The exactness is the point rather than a nicety: an eased value that
    /// stops at 0.9997 of its way is a mark parked a fraction of a pixel off its
    /// box, and an opacity that never quite reaches full, for ever, on a band
    /// nobody is touching any more.
    fn at(self, now: Instant, motion: Motion) -> T {
        let elapsed = now.saturating_duration_since(self.since);
        if !tooltip::hover_fade_owes_frames(elapsed, motion) {
            return self.to;
        }
        self.from
            .lerp(self.to, tooltip::hover_fade_opacity(elapsed, motion))
    }

    /// Send the value somewhere else, from wherever it is now. `false` when it
    /// was already going there, which is what keeps a still band from asking the
    /// glass for anything.
    fn retarget(&mut self, to: T, now: Instant, motion: Motion) -> bool {
        if self.to == to {
            return false;
        }
        *self = Self {
            from: self.at(now, motion),
            to,
            since: now,
        };
        true
    }

    /// Whether this journey still owes the glass a frame.
    fn owes_frames(self, now: Instant, motion: Motion) -> bool {
        self.from != self.to
            && tooltip::hover_fade_owes_frames(now.saturating_duration_since(self.since), motion)
    }
}

/// What it takes to be carried by an [`Ease`]: a value with points in between.
///
/// Two implementations, and one of them derives the other — a number, and any
/// fixed-size array of them, which is how a box (`[f32; 4]`) and a whole
/// placement (`[[f32; 4]; 3]`) travel without either of them being written out.
trait Lerp: Copy + PartialEq {
    fn lerp(self, to: Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(self, to: Self, t: f32) -> Self {
        self + (to - self) * t
    }
}

impl<T: Lerp, const N: usize> Lerp for [T; N] {
    fn lerp(self, to: Self, t: f32) -> Self {
        std::array::from_fn(|index| self[index].lerp(to[index], t))
    }
}

/// **The hovered band's marks as something that arrives, follows and leaves**
/// (owner's report 2026-09-14 evening, T-MATH-TOOLS-FOLLOW).
///
/// Everything above this is a function of the picture in hand; this is the one
/// thing in the module that remembers, and it exists because the report's three
/// clauses are about *change* rather than about a picture: which mark the
/// pointer is on, where the band's boxes went when the block changed shape, and
/// how long each of those takes to become true on the glass.
///
/// It holds two journeys and no rules of its own. The opacity's is the arrival
/// and the departure. The placement's is the block changing shape underneath a
/// pointer that never moved — a toggle to source, a resize that re-wraps, a
/// scale change — and the marks *travel* to the new boxes rather than being
/// re-struck in them. Both are an [`Ease`], so both are the tip's ninety
/// milliseconds and both stand down under [`Motion::Reduced`].
///
/// **The band's identity is `MathBlockAnchor::same_block`'s**, which is the
/// identity the hover sweep and the renderer's own placement lookup already key
/// on (§7.1.5p ⑥): a block whose rows a fold or a re-wrap has moved is the same
/// block, and that is precisely the case this type exists to travel rather than
/// to re-place. A *different* block is a different arrival, fade and all.
#[derive(Clone, Debug, PartialEq)]
pub struct FormulaToolFollow {
    /// The band these marks belong to.
    anchor: MathBlockAnchor,
    /// The face it is wearing, which decides whether the first mark is `#i-code`
    /// or `#i-eye`. It flips with the geometry a toggle changes, in the same
    /// breath, because the mark names the view the press leads to.
    display: MathBlockDisplay,
    /// `[the band, the source mark's box, the copy mark's box]`, in the
    /// surface's own pixels.
    place: Ease<[[f32; 4]; 3]>,
    /// The next frame without a flight is still its landing frame. This is a
    /// receipt, not a clock: consuming it cannot create animation debt.
    ride_landing: bool,
    /// How solid they are drawn: `0 -> 1` on arrival, `-> 0` on leaving.
    opacity: Ease<f32>,
    /// The mark under the pointer, if it is on one.
    hovered: Option<FormulaTool>,
}

impl FormulaToolFollow {
    /// Marks arriving beside a band: placed where the picture says, coming up
    /// from nothing.
    #[must_use]
    pub fn arriving(boxes: &MathToolBoxes, hovered: Option<FormulaTool>, now: Instant) -> Self {
        Self {
            anchor: boxes.anchor.clone(),
            display: boxes.display,
            place: Ease::settled([boxes.block, boxes.source, boxes.copy], now),
            ride_landing: false,
            opacity: Ease {
                from: 0.0,
                to: 1.0,
                since: now,
            },
            hovered,
        }
    }

    /// **The picture in hand, read against the marks on the glass.**
    ///
    /// Returns whether anything the overlay draws has changed — a new target for
    /// either journey, a different face, or a different mark under the pointer —
    /// so a band standing still under a still pointer asks for nothing at all.
    ///
    /// A picture of a *different* block is not a move but an arrival: the marks
    /// come up beside it the way they came up beside the first one, which is the
    /// reading §7.1.5p ② already gave crossing from one formula to the next.
    pub fn follow(
        &mut self,
        boxes: &MathToolBoxes,
        hovered: Option<FormulaTool>,
        now: Instant,
        motion: Motion,
    ) -> bool {
        if !self.anchor.same_block(&boxes.anchor) {
            *self = Self::arriving(boxes, hovered, now);
            return true;
        }
        if std::mem::take(&mut self.ride_landing) {
            let changed = self.ride(boxes, hovered, now, motion);
            self.ride_landing = false;
            return changed;
        }
        // Kept as the picture spells it: `same_block` is the identity, and the
        // inline run inside an anchor belongs to the frame rather than to the
        // block.
        self.anchor = boxes.anchor.clone();
        let mut changed = self
            .place
            .retarget([boxes.block, boxes.source, boxes.copy], now, motion);
        // A band the pointer came back to before its exit had finished turns
        // round from wherever it had faded to, rather than starting again at
        // nothing.
        changed |= self.opacity.retarget(1.0, now, motion);
        if self.display != boxes.display {
            self.display = boxes.display;
            changed = true;
        }
        if self.hovered != hovered {
            self.hovered = hovered;
            changed = true;
        }
        changed
    }

    /// **The band's own height is travelling, so the marks are carried on it rather than
    /// travelling to it** (owner's ruling 2026-09-15, T-MATH-TOGGLE-MOTION; §7.1.5p ⑪).
    ///
    /// [`Self::follow`] exists for geometry that *jumped* — a re-wrap, a scale change, a toggle
    /// that used to land in a single frame — and easing towards a box that is **itself** easing is
    /// two journeys over one distance: the marks would trail the edge they are supposed to ride,
    /// and settle a whole ninety milliseconds after the block had stopped. So while the block is
    /// changing face the placement is settled on every frame and the *ride* is the block's own.
    ///
    /// Everything else is `follow`'s, unchanged: a different block is still an arrival, the
    /// opacity's own journey still arrives, leaves and turns round on the tip's ninety
    /// milliseconds, and the answer is still "did anything the overlay draws change".
    pub fn ride(
        &mut self,
        boxes: &MathToolBoxes,
        hovered: Option<FormulaTool>,
        now: Instant,
        motion: Motion,
    ) -> bool {
        if !self.anchor.same_block(&boxes.anchor) {
            *self = Self::arriving(boxes, hovered, now);
            self.ride_landing = true;
            return true;
        }
        self.ride_landing = true;
        self.anchor = boxes.anchor.clone();
        let place = [boxes.block, boxes.source, boxes.copy];
        let mut changed = self.place.at(now, motion) != place;
        self.place = Ease::settled(place, now);
        changed |= self.opacity.retarget(1.0, now, motion);
        if self.display != boxes.display {
            self.display = boxes.display;
            changed = true;
        }
        if self.hovered != hovered {
            self.hovered = hovered;
            changed = true;
        }
        changed
    }

    /// Settlement leaves the named marks a receipt even if an interruption
    /// lands before the first travelling present. No other band can inherit it.
    pub fn finish_ride(&mut self, anchor: &MathBlockAnchor) {
        self.ride_landing = self.anchor.same_block(anchor);
    }

    #[must_use]
    pub fn is_riding(&self, anchor: &MathBlockAnchor, in_flight: bool) -> bool {
        in_flight || (self.ride_landing && self.anchor.same_block(anchor))
    }

    /// The present consumes the receipt even when its named band is absent.
    pub fn finish_landing_frame(&mut self) -> bool {
        std::mem::take(&mut self.ride_landing)
    }

    /// The band is not hovered any more: the marks go out over the same span.
    ///
    /// **This is the half of §7.1.5p ② the owner revised on the evening of
    /// 2026-09-14.** That clause spent the glance card's asymmetry here — a fade
    /// in and no fade out — and the revision is narrow: the 500 ms grace is
    /// untouched, the band's own floor still leaves with the picture that stops
    /// lighting it, and what fades is the two marks, over the ninety
    /// milliseconds they arrived on.
    pub fn leave(&mut self, now: Instant, motion: Motion) -> bool {
        self.ride_landing = false;
        self.opacity.retarget(0.0, now, motion)
    }

    /// Whether there is nothing left to draw — the exit has landed, and under
    /// stillness that is the frame it was asked for.
    #[must_use]
    pub fn gone(&self, now: Instant, motion: Motion) -> bool {
        self.opacity.to == 0.0 && !self.opacity.owes_frames(now, motion)
    }

    /// The mark the pointer is on, as the drawing half is told it.
    #[must_use]
    pub fn hovered(&self) -> Option<FormulaTool> {
        self.hovered
    }

    /// Update only which mark the pointer is on. Geometry is owned by the frame
    /// at the present door; a pointer crossing between the two already-placed
    /// boxes must not need an earlier picture merely to change their ink.
    pub fn observe_hovered(&mut self, hovered: Option<FormulaTool>) -> bool {
        if self.hovered == hovered {
            return false;
        }
        self.hovered = hovered;
        true
    }

    /// How solid the marks are drawn this frame.
    #[must_use]
    pub fn opacity(&self, now: Instant, motion: Motion) -> f32 {
        self.opacity.at(now, motion)
    }

    /// Where they stand this frame — the band's own boxes when nothing is
    /// moving, and a point on the way when the block has just changed shape.
    #[must_use]
    pub fn placed(&self, now: Instant, motion: Motion) -> MathToolBoxes {
        let [block, source, copy] = self.place.at(now, motion);
        MathToolBoxes {
            anchor: self.anchor.clone(),
            display: self.display,
            block,
            source,
            copy,
        }
    }

    /// **Whether either journey still owes the glass a frame**, which is also
    /// the whole of what wakes the loop for this surface: a pointer resting on a
    /// formula whose marks have arrived costs no wake-ups at all, and under
    /// [`Motion::Reduced`] nothing here ever does.
    #[must_use]
    pub fn owes_frames(&self, now: Instant, motion: Motion) -> bool {
        self.place.owes_frames(now, motion) || self.opacity.owes_frames(now, motion)
    }
}

/// **One block on its way from one of its faces to the other** (owner's ruling 2026-09-15,
/// T-MATH-TOGGLE-MOTION; `docs/DESIGN.md` §7.1.5p ⑪).
///
/// Pressing `‹›` used to be one frame: a picture, and then four rows of `$$…$$` where it had been,
/// with everything below jumping by the difference. The ruling is that it travels — and the whole
/// of what makes that possible is that **the document does not change while it does**. A block is
/// either one entry with an artifact height or the rows that entry was swallowing; there is no
/// third thing to interpolate, and a row *count* is not a quantity with points in between. So for
/// the ninety milliseconds the change takes, the session keeps the representation it is already in
/// — one entry with an artifact height — and what travels is the presentation:
///
/// - the band is **presented** at a height on its way from one face's to the other's
///   ([`Self::height_subpixels`]), which the projection reads where it reads any artifact's, so
///   everything under it moves the way it will end up moving;
/// - the picture is drawn at [`Self::picture_opacity_milli`] and the source text is drawn over the
///   same band at what is left ([`Self::source_opacity`]) — one cross-fade over one rectangle;
/// - and the document is told **once**, on the frame the journey lands, so that the frame after the
///   change is the frame before it was made.
///
/// Which end the telling happens at is the only asymmetry, and it is not a choice: the
/// representation that can be presented at any height is the artifact one, so a block **leaving**
/// its picture keeps it until the far end and a block **returning** to it is switched at the near
/// end. Both directions therefore run with the block as an artifact, and that is what makes
/// [`Self::reverse`] free — a second press never touches the document at all, it turns the journey
/// round from wherever it stands.
///
/// The curve, the span and the reduced-motion answer are [`Ease`]'s, which are the tip's: this
/// surface keeps no second number, exactly as the marks beside it keep none.
#[derive(Clone, Debug, PartialEq)]
pub struct FormulaToggleMotion {
    /// The tab, seat and shell incarnation from the press; delayed work validates this owner
    /// before measuring, changing or presenting its block.
    target: PasteTarget,
    /// The block, by the identity every other reader of a band keys on.
    anchor: MathBlockAnchor,
    /// Where this is heading. `true` is the `$$…$$` source — which the document is told about when
    /// the journey lands — and `false` is the picture, which it was told about when it began.
    to_source: bool,
    /// `[the band's presented height in subpixels, how far over to the source face]`. One journey
    /// and not two, so a reversal cannot leave the height and the cross-fade disagreeing about
    /// where they are or how long they have left.
    journey: Ease<[f32; 2]>,
    /// The rows the source face draws while it is still an overlay — the pane's own answer
    /// (`bt_viewport::ViewportProjection::math_source_face`), which is the same answer the height
    /// this is travelling to was measured from.
    source_face: bt_viewport::MathSourceFace,
}

impl FormulaToggleMotion {
    /// The change begins.
    ///
    /// `heights` is `[the typeset face's band height, the source rows' height]`, both in subpixels
    /// and both measured by the pane the block is in; `source_face` carries those rows and their measured width.
    #[must_use]
    pub fn begin(
        target: PasteTarget,
        anchor: MathBlockAnchor,
        heights: [i64; 2],
        to_source: bool,
        source_face: bt_viewport::MathSourceFace,
        now: Instant,
    ) -> Self {
        let [rendered, source] = heights.map(|height| height.max(1) as f32);
        let (picture, text) = ([rendered, 0.0], [source, 1.0]);
        let (from, to) = if to_source {
            (picture, text)
        } else {
            (text, picture)
        };
        Self {
            target,
            anchor,
            to_source,
            journey: Ease {
                from,
                to,
                since: now,
            },
            source_face,
        }
    }

    /// **The mark was pressed again before the change landed.**
    ///
    /// The journey turns round from the height and the strength it is actually showing
    /// ([`Ease::retarget`]), so a hand that changes its mind half-way sees the block stop and come
    /// back rather than snap to one end and set off from there. **The document is not touched**,
    /// and cannot need to be: the block has been an artifact for the whole of the flight, in both
    /// directions, so turning round only moves where the telling happens — and the caller learns
    /// that from [`Self::switch_owed`] as it always does.
    ///
    /// `heights` is asked for again rather than remembered: the pane may have re-wrapped under the
    /// first half of the journey, and the end of this one has to be the height the block will
    /// really stand at.
    pub fn reverse(
        &mut self,
        heights: [i64; 2],
        source_face: bt_viewport::MathSourceFace,
        now: Instant,
        motion: Motion,
    ) {
        let [rendered, source] = heights.map(|height| height.max(1) as f32);
        self.to_source = !self.to_source;
        self.source_face = source_face;
        let to = if self.to_source {
            [source, 1.0]
        } else {
            [rendered, 0.0]
        };
        self.journey.retarget(to, now, motion);
    }

    /// The pane this journey is happening in.
    #[must_use]
    pub fn target(&self) -> PasteTarget {
        self.target
    }

    /// The block this belongs to.
    #[must_use]
    pub fn anchor(&self) -> &MathBlockAnchor {
        &self.anchor
    }

    /// **Whether the document still owes this change** — true exactly while the block is travelling
    /// towards its source, which is the direction whose telling happens at the far end.
    #[must_use]
    pub fn switch_owed(&self) -> bool {
        self.to_source
    }

    /// Whether the journey has arrived — and under [`Motion::Reduced`] that is the frame it began.
    #[must_use]
    pub fn landed(&self, now: Instant, motion: Motion) -> bool {
        !self.journey.owes_frames(now, motion)
    }

    /// **The instant this journey is due to have arrived** (review 2026-09-18, P1).
    ///
    /// The one clock of a change of face that is never paced, and the reason it is spelled here
    /// rather than inferred by the caller: what happens at the far end is owed to the *document* —
    /// the block is told which face it wears — and nobody is waiting for a display to take that.
    /// Behind a frame gate it is a block a busy neighbouring pane can hold half way over for as
    /// long as it keeps printing.
    ///
    /// The span is [`tooltip::TOOLTIP_FADE`] and is not spelled a second time: this reads the
    /// journey's own epoch and adds the one number every fade in this window is drawn on.
    #[must_use]
    pub fn lands_at(&self) -> Instant {
        self.journey.since + tooltip::TOOLTIP_FADE
    }

    /// Whether this is still travelling towards the height the block's face will really stand at.
    ///
    /// `false` once a re-wrap, a scale change or a font change has moved that height under the
    /// flight — at which point the only honest thing left to do is land, because the end of a
    /// journey that is not where the block ends up is the jump this whole clause exists to remove.
    #[must_use]
    pub fn still_measures(&self, heights: [i64; 2]) -> bool {
        let [rendered, source] = heights;
        let target = if self.to_source { source } else { rendered };
        (self.journey.to[0] - target.max(1) as f32).abs() < 0.5
    }

    /// The height the band is presented at on this frame — and **exactly** the face's own height
    /// once it has landed, which is what makes the last frame of the change and the first frame
    /// after it one picture.
    #[must_use]
    pub fn height_subpixels(&self, now: Instant, motion: Motion) -> i64 {
        self.journey.at(now, motion)[0].round() as i64
    }

    /// How far over to the source face the cross-fade has got, `0` at the picture and `1` at the
    /// text.
    #[must_use]
    pub fn source_opacity(&self, now: Instant, motion: Motion) -> f32 {
        self.journey.at(now, motion)[1].clamp(0.0, 1.0)
    }

    /// How solid the picture is drawn on this frame, in the thousandths
    /// `bt_viewport::MathBlockPlacement::picture_opacity_milli` is counted in.
    #[must_use]
    pub fn picture_opacity_milli(&self, now: Instant, motion: Motion) -> u16 {
        ((1.0 - self.source_opacity(now, motion)) * 1000.0).round() as u16
    }

    /// Geometry and opacity sample the same journey, but geometry never reads
    /// opacity as its authority: future fading changes must not change the seat.
    #[must_use]
    pub fn face_milli(&self, now: Instant, motion: Motion) -> u16 {
        (self.source_opacity(now, motion) * 1000.0).round() as u16
    }

    #[must_use]
    pub fn source_width_cells(&self) -> u32 {
        self.source_face.width_cells
    }

    /// The rows the source face draws while it is an overlay.
    #[must_use]
    pub fn source_rows(&self) -> &[String] {
        &self.source_face.rows
    }

    /// **Whether this still owes the glass a frame** — and under [`Motion::Reduced`] it never does,
    /// which is the whole of that setting's answer here: the switch is made on the frame it is
    /// asked for and nothing is ever presented at a height between the two.
    #[must_use]
    pub fn owes_frames(&self, now: Instant, motion: Motion) -> bool {
        self.journey.owes_frames(now, motion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_render::{DARK_CHROME, LIGHT_CHROME};

    /// A synthetic placement on a real viewport frame, using the production
    /// CPU geometry API. No device, window, sleeps or timer advances are needed.
    fn landing_frame() -> (
        bt_render::CellMetrics,
        bt_render::SeatViewport,
        bt_viewport::ViewportFrame,
    ) {
        use bt_viewport::{MathBlockPlacement, ProjectedMathArtifact, RgbaArtifactKind};
        let mut fonts = bt_render::preview_measure_font_system();
        let metrics = bt_render::CellMetrics::measure(&mut fonts, 1.0).unwrap();
        let session = bt_term::DualPlaneSession::new(
            std::num::NonZeroU32::new(80).unwrap(),
            std::num::NonZeroU32::new(20).unwrap(),
        );
        let mut projection = session.new_projection(session.layout_key());
        let mut frame = session.viewport_frame(&mut projection).unwrap();
        let height = 6 * metrics.cell_height_subpixels().get();
        frame.math_blocks.push(MathBlockPlacement {
            start: bt_transcript::TranscriptId(1),
            anchor: boxes(MathBlockDisplay::Rendered).anchor,
            source: "$$x^2$$".to_owned(),
            artifact: ProjectedMathArtifact {
                inline_runs: Vec::new(),
                key: "landing-fixture".to_owned(),
                end: bt_transcript::TranscriptId(1),
                rgba: std::sync::Arc::from([0_u8; 4]),
                width_px: (20.0 * metrics.cell_width_px) as u32,
                height_px: (height / 1024) as u32,
                height_subpixels: height,
                baseline_subpixels: 0,
                mode: bt_detect::MathMode::Display,
                kind: RgbaArtifactKind::Math,
                vertical_padding_subpixels: 0,
                render_scale_milli: 1000,
                source: "x^2".to_owned(),
            },
            top_subpixels: 0,
            left_subpixels: 2 * 1024,
            content_offset_subpixels: 0,
            clip_height_subpixels: height,
            display: MathBlockDisplay::Rendered,
            source_width_cells: 6,
            horizontal_overflow: bt_viewport::BlockOverflowOwner::Block,
            horizontal_scroll_px: 0,
            vertical_scroll_px: 0,
            toolbar_visible: true,
            occluded_source_rows: 0,
            occluded_visible_rows: Vec::new(),
            live_occurrence_id: None,
            frozen_prefix_rows: 0,
            clipped_top_rows: 0,
            clipped_bottom_rows: 0,
            picture_opacity_milli: 1000,
            face_milli: None,
            selection_spans: Vec::new(),
        });
        (metrics, bt_render::SeatViewport::whole(1000, 1000), frame)
    }

    #[test]
    fn the_marks_do_not_travel_after_the_band_has_landed() {
        let t0 = Instant::now();
        let (metrics, pane, fixture) = landing_frame();
        let mut landings = Vec::new();
        for to_source in [true, false] {
            let mut frame = fixture.clone();
            let anchor = frame.math_blocks[0].anchor.clone();
            let heights = [6, 4].map(|rows| rows * metrics.cell_height_subpixels().get());
            let journey = FormulaToggleMotion::begin(
                flight(to_source, t0).target(),
                anchor.clone(),
                heights,
                to_source,
                bt_viewport::MathSourceFace {
                    rows: vec!["$$x^2$$".to_owned()],
                    width_cells: 6,
                    height_subpixels: heights[1],
                },
                t0,
            );
            let block = &mut frame.math_blocks[0];
            block.display = if to_source {
                MathBlockDisplay::Rendered
            } else {
                MathBlockDisplay::Source
            };
            block.left_subpixels = if to_source { 2 * 1024 } else { 0 };
            block.clip_height_subpixels = journey.height_subpixels(t0, Motion::Full);
            let start = bt_render::math_tool_boxes_for(metrics, pane, &frame, &anchor).unwrap();
            let mut marks = FormulaToolFollow::arriving(&start, None, t0 - tooltip::TOOLTIP_FADE);
            let mut last_width = 0.0;
            for ms in [0, 25, 50, 75, 89] {
                let now = t0 + std::time::Duration::from_millis(ms);
                let block = &mut frame.math_blocks[0];
                block.display = MathBlockDisplay::Rendered;
                block.left_subpixels = 2 * 1024;
                block.face_milli = Some(journey.face_milli(now, Motion::Full));
                block.picture_opacity_milli = journey.picture_opacity_milli(now, Motion::Full);
                block.clip_height_subpixels = journey.height_subpixels(now, Motion::Full);
                let seat = bt_render::math_tool_boxes_for(metrics, pane, &frame, &anchor).unwrap();
                marks.ride(&seat, None, now, Motion::Full);
                assert_eq!(marks.placed(now, Motion::Full), seat);
                last_width = seat.block[2] - seat.block[0];
            }
            let landed = t0 + tooltip::TOOLTIP_FADE;
            let block = &mut frame.math_blocks[0];
            block.display = if to_source {
                MathBlockDisplay::Source
            } else {
                MathBlockDisplay::Rendered
            };
            block.left_subpixels = if to_source { 0 } else { 2 * 1024 };
            block.face_milli = None;
            block.clip_height_subpixels = journey.height_subpixels(landed, Motion::Full);
            let end = bt_render::math_tool_boxes_for(metrics, pane, &frame, &anchor).unwrap();
            // Settlement takes the flight before this frame. The first follow
            // consumes the receipt left by ride, so it cannot start another ease.
            marks.follow(&end, None, landed, Motion::Full);
            landings.push((
                to_source,
                marks.placed(landed, Motion::Full) == end,
                marks.owes_frames(landed, Motion::Full),
            ));
            assert!((last_width - (end.block[2] - end.block[0])).abs() <= 1.0);
            assert!(!marks.follow(&end, None, landed + tooltip::TOOLTIP_FADE, Motion::Full));
        }
        assert_eq!(
            landings,
            [(true, true, false), (false, true, false)],
            "(to_source, exactly_seated, owes_frames) at the landing instant"
        );
    }

    /// A band with its two marks where the renderer now seats them: **inside the
    /// block, in the room it keeps at its right edge, on its midline** (owner's
    /// ruling 2026-09-15 ②).
    ///
    /// The pane head's 19px box with the mock-up's 2px between them, so the pair
    /// is 40 wide, and the block's own right inset is what it stands in — which
    /// is what `bt_render`'s `math_tool_boxes_px` answers and what these
    /// fixtures have to depict, or every pin below would be struck on a
    /// placement the product does not draw.
    fn boxes(display: MathBlockDisplay) -> MathToolBoxes {
        MathToolBoxes {
            anchor: bt_viewport::MathBlockAnchor::History {
                run: None,
                start: bt_transcript::TranscriptId(1),
                end: bt_transcript::TranscriptId(1),
            },
            display,
            block: [40.0, 10.0, 300.0, 59.0],
            source: [230.0, 25.0, 249.0, 44.0],
            copy: [251.0, 25.0, 270.0, 44.0],
        }
    }

    #[test]
    fn formula_landing_receipt_is_spent_once_and_cannot_reach_a_neighbour() {
        let now = Instant::now();
        let start = boxes(MathBlockDisplay::Rendered);
        let mut marks = FormulaToolFollow::arriving(&start, None, now - tooltip::TOOLTIP_FADE);
        let end = source_band();
        // Resize/interrupt before the first travelling frame still lands directly.
        marks.finish_ride(&start.anchor);
        assert!(marks.is_riding(&end.anchor, false));
        marks.follow(&end, None, now, Motion::Full);
        assert_eq!(marks.placed(now, Motion::Full), end);
        assert!(!marks.owes_frames(now, Motion::Full));
        assert!(!marks.is_riding(&end.anchor, false));
        // A later re-wrap is an ordinary follow, proving the receipt is one-use.
        marks.follow(&start, None, now, Motion::Full);
        assert!(marks.owes_frames(now, Motion::Full));
        assert_ne!(marks.placed(now, Motion::Full).block, start.block);
        // A frame without the named band consumes the landing too.
        marks.ride(&start, None, now, Motion::Full);
        assert!(marks.finish_landing_frame());
        assert!(!marks.finish_landing_frame());
        assert!(!marks.is_riding(&start.anchor, false));
        let mut neighbour = start.clone();
        neighbour.anchor = bt_viewport::MathBlockAnchor::History {
            run: None,
            start: bt_transcript::TranscriptId(99),
            end: bt_transcript::TranscriptId(99),
        };
        marks.finish_ride(&neighbour.anchor);
        assert!(!marks.is_riding(&start.anchor, false));
        marks.ride(&start, None, now, Motion::Full);
        marks.leave(now, Motion::Full);
        assert!(!marks.is_riding(&start.anchor, false));
    }

    #[test]
    fn formula_present_uses_one_guard_and_consumes_the_landing_receipt() {
        let guard = method_body("formula_overlay_is_active");
        assert!(guard.contains(
            "self.window.math_hover_anchor.is_some() || self.window.math_tools.is_some()"
        ));
        let carry = method_body("carry_live_journeys");
        assert!(carry.contains("let formula_present = self.formula_overlay_is_active();"));
        assert!(carry.contains("formula_overlay_owed |= running.chrome || running.overlay"));
        let refresh = method_body("refresh_formula_overlay_for_present");
        assert!(refresh.contains("std::mem::take(&mut self.window.formula_overlay_owed)"));
        assert!(refresh.contains("if !carried && !moved && !in_flight"));
        let settle = method_body("settle_math_toggle");
        assert!(
            settle.find("follow.finish_ride(flight.anchor())").unwrap()
                < settle.find("self.repaint_pane_change(seat)").unwrap()
        );
        let sync = method_body("sync_math_tools");
        assert!(sync.contains("follow.is_riding(&boxes.anchor, riding)"));
        assert!(sync.contains("changed |= follow.finish_landing_frame()"));
        for name in ["redraw", "present_retained_picture"] {
            let present = method_body(name);
            let guard = present.find("if self.formula_overlay_is_active()").unwrap();
            assert!(
                guard
                    < present
                        .find("self.math_band_trace_for_present(frame_for)")
                        .unwrap()
            );
            assert!(guard < present.find("self.math_tool_placement(frame_for)").unwrap());
            assert!(
                present
                    .find("self.refresh_formula_overlay_for_present")
                    .unwrap()
                    < present
                        .find("self.math_band_trace_line(now, trace)")
                        .unwrap()
            );
            assert!(
                present.find("Self::present_seats_and_commit").unwrap()
                    < present
                        .find("self.trace_math_band(math_band_trace)")
                        .unwrap()
            );
        }
        let trace_line = method_body("math_band_trace_line");
        assert!(
            trace_line.contains("seat=none display=none"),
            "departing marks still get one self-report even without a lit band"
        );
        let trace = method_body("math_band_trace_for_present");
        assert!(
            trace.find("if !self.app.trace_perf").unwrap()
                < trace.find("self.sessions.keys()").unwrap()
        );
        // R5/R6: each closure pairs a seat's frame with that seat's body, with
        // the same focused fallback the retained-seat builder actually draws.
        let retained = method_body("present_retained_picture");
        assert!(retained.contains("find(|pane| pane.seat == seat)"));
        assert!(retained.contains(".then(|| self.window.renderer.seat_viewport())"));
        assert!(retained.contains(".get(&seat)?"));
        let redraw = method_body("redraw");
        // Since ticket 37 each pair is a triple: the frame travels with the metrics it is
        // drawn at, so the formula lanes measure it in the pane's own cells.
        assert!(redraw.contains("return Some((focused_body.viewport, &frame, metrics));"));
        assert!(redraw.contains("find(|(pane, _)| pane.seat == seat)"));
        assert!(redraw.contains(".map(|(pane, projected)| (pane.viewport, projected, metrics))"));
    }

    // ── the bodies these four pins are about ─────────────────────────────
    //
    // **P3's deletion commit for this module** (`docs/plans/bt-app-split-prep.md`
    // §6.3, and §6.0 rule 3). The commit before this one read every body twice
    // — once as a slice of `main.rs`, once as the body of an item of this crate
    // — and asserted the two were the same bytes; this one removes the older of
    // the two, because two implementations of one judgement do not vouch for
    // each other (`docs/CONVENTIONS.md` §十 rule 4).
    //
    // **The pattern is `main.rs::pty_drain_budget_tests`' and is not
    // re-derived**; that module's header carries the six points behind
    // `source_index` and `method_body`. What went with the slice: the owner,
    // which the old reading could not tell (it took the first match for a
    // signature prefix, which is a method of whatever `impl` comes first), and
    // the *next* method's declaration and doc comment, which the slice ran on
    // into — `turn`'s was 45,871 bytes around a 44,426-byte body.
    //
    // `include_str!("formula_tools.rs")` stays: it belongs to P14's row for
    // `the_marks_fade_in_travel_and_fade_out_on_the_tips_own_ninety_milliseconds`,
    // which reads this file's own text above its tests.

    /// **This crate, indexed once per process** — the workspace read, this
    /// package's own `src/` declared as the universe and lowered, on the first
    /// ask of the process, behind one call (`bt_source::Index::of_package`).
    ///
    /// The package is named here and nowhere else in the module.
    fn source_index() -> &'static bt_source::Index {
        bt_source::Index::of_package("bt-app")
    }

    /// The body of one inherent method of `Runtime`, braces included — the
    /// identity of §2.4 rather than a line of `main.rs`.
    fn method_body(name: &str) -> &'static str {
        source_index()
            .body_of(&bt_source::ItemQuery::method("Runtime", name))
            .unwrap_or_else(|failure| panic!("{failure}"))
    }

    /// RED (2026-09-20, T-MARKS-FRAME-IN-HAND): the marks stand on the band in the
    /// exact `ViewportFrame` this present draws, including the first and landing frames.
    #[test]
    fn the_marks_stand_on_the_band_of_the_frame_being_drawn() {
        let placement = method_body("math_tool_placement");
        assert!(
            placement.contains("frame_for(*seat)") && !placement.contains("last_presented_frame"),
            "the runtime must place from the frame its caller hands it:\n{placement}"
        );
    }

    /// RED (2026-09-20, T-MARKS-FRAME-IN-HAND): no presented mark rectangle is
    /// borrowed from another frame, so the landing frame has nothing left to snap to.
    #[test]
    fn the_landing_frame_needs_no_snap() {
        let redraw = method_body("redraw");
        let handed = redraw
            .find("self.refresh_formula_overlay_for_present")
            .expect("redraw hands its composed pane frames to the formula lanes");
        let present = redraw
            .find("Self::present_seats_and_commit(")
            .expect("redraw reaches the glass through the present funnel");
        assert!(
            handed < present,
            "the handoff must precede the present:\n{redraw}"
        );
    }

    /// RED regression guard (not a reproduction): today both lanes already read the same
    /// stale frame. This pins that after the owner changes, the source face and the marks still
    /// read one frame — now the frame this present draws.
    #[test]
    fn the_source_face_and_the_marks_read_one_frame() {
        let marks = method_body("math_tool_placement");
        let source = method_body("formula_toggle_layers");
        for (name, lane) in [("marks", marks), ("source face", source)] {
            assert!(
                lane.contains("frame_for(") && !lane.contains("last_presented_frame"),
                "the {name} lane must consume the handed present frame:\n{lane}"
            );
        }
    }

    /// **The same block after a press on `‹›`.** The source face is taller, so
    /// the band's rows and the two boxes on its midline have all moved — and the
    /// anchor has not, which is exactly the case
    /// [`MathBlockAnchor::same_block`] exists to answer and the case the marks
    /// must *travel* rather than be re-struck in.
    fn toggled(geometry: &MathToolBoxes) -> MathToolBoxes {
        MathToolBoxes {
            display: MathBlockDisplay::Source,
            block: [40.0, 10.0, 300.0, 107.0],
            source: [230.0, 49.0, 249.0, 68.0],
            copy: [251.0, 49.0, 270.0, 68.0],
            ..geometry.clone()
        }
    }

    /// A different formula on the same screen.
    fn another_band() -> MathToolBoxes {
        MathToolBoxes {
            anchor: bt_viewport::MathBlockAnchor::History {
                run: None,
                start: bt_transcript::TranscriptId(9),
                end: bt_transcript::TranscriptId(9),
            },
            block: [40.0, 200.0, 300.0, 249.0],
            source: [230.0, 215.0, 249.0, 234.0],
            copy: [251.0, 215.0, 270.0, 234.0],
            ..boxes(MathBlockDisplay::Rendered)
        }
    }

    /// Whether every mark of `geometry` is seated the way the ruling asks: inside
    /// the block, right of everything else in it, and on its midline.
    fn seated_inside_the_block(geometry: &MathToolBoxes) -> bool {
        let [left, top, right, bottom] = geometry.block;
        let midline = (top + bottom) / 2.0;
        [geometry.source, geometry.copy].into_iter().all(|mark| {
            mark[0] >= left
                && mark[2] <= right
                && mark[1] >= top
                && mark[3] <= bottom
                && ((mark[1] + mark[3]) / 2.0 - midline).abs() <= 0.5
        })
    }

    /// **What the overlay lane would put on the glass for this follow.**
    ///
    /// `Runtime::formula_tool_layers` with the window's own palette and scale
    /// filled in — which is all of that method there is, since the day it stopped
    /// deciding anything (owner's report 2026-09-14 evening).
    fn drawn(follow: &FormulaToolFollow, now: Instant, motion: Motion) -> Vec<ChromeSprite> {
        sprites(
            &follow.placed(now, motion),
            FormulaToolState {
                hovered: follow.hovered(),
                ..FormulaToolState::default()
            },
            &DARK_CHROME,
            1.0,
            follow.opacity(now, motion),
        )
    }

    fn pill_of(sprites: &[ChromeSprite]) -> Option<ChromeSprite> {
        sprites
            .iter()
            .find(|sprite| matches!(sprite.mark, ChromeMark::ControlPill { .. }))
            .copied()
    }

    fn mark_of(sprites: &[ChromeSprite], mark: ChromeMark) -> Option<ChromeSprite> {
        sprites.iter().find(|sprite| sprite.mark == mark).copied()
    }

    /// PIN (owner's ruling 2026-09-14 ②): **at rest there is nothing there.**
    ///
    /// Two rests, and they are two different facts. The band nobody is pointing
    /// at has no boxes at all — that one is the renderer's
    /// (`math_block_ground_is_drawn` / `toolbar_visible`) and is pinned there.
    /// This one is the fade's own floor: a band whose tools have not begun to
    /// arrive draws no sprite, rather than drawing two invisible ones.
    ///
    /// MUTATION: return the sprites and let the layer's opacity do the fading —
    /// a mark at `0.0` is still a rasterized texture and still a hit target's
    /// worth of drawing every frame the pointer is elsewhere.
    #[test]
    fn a_band_whose_marks_have_not_arrived_draws_nothing() {
        let at_rest = sprites(
            &boxes(MathBlockDisplay::Rendered),
            FormulaToolState::default(),
            &DARK_CHROME,
            1.0,
            0.0,
        );
        assert!(at_rest.is_empty(), "{at_rest:?}");
    }

    /// PIN (owner's ruling 2026-09-14 ②): **hovering the band puts two marks in
    /// the band's own boxes, and no pill under either.**
    ///
    /// MUTATIONS: draw a pill unconditionally → ②; centre the mark on the box's
    /// corner instead of its middle → ③; let a mark's box exceed its pill → ④.
    #[test]
    fn a_hovered_band_shows_two_marks_inside_their_own_boxes_and_no_pill() {
        let geometry = boxes(MathBlockDisplay::Rendered);
        let drawn = sprites(
            &geometry,
            FormulaToolState::default(),
            &DARK_CHROME,
            1.0,
            1.0,
        );

        // ① Two marks, and they are the house's own two.
        assert_eq!(drawn.len(), 2, "{drawn:?}");
        assert!(mark_of(&drawn, ChromeMark::Code).is_some());
        assert!(mark_of(&drawn, ChromeMark::Copy).is_some());

        // ② No pill: `background: none` is the whole of the resting state.
        assert!(pill_of(&drawn).is_none(), "{drawn:?}");

        // ③ Each mark is inside the box the renderer gave it, and centred in it.
        for (sprite, box_) in [(drawn[0], geometry.source), (drawn[1], geometry.copy)] {
            assert!(
                sprite.rect[0] >= box_[0]
                    && sprite.rect[1] >= box_[1]
                    && sprite.rect[2] <= box_[2]
                    && sprite.rect[3] <= box_[3],
                "{:?} escaped its box {box_:?}",
                sprite.rect
            );
            assert!(
                ((sprite.rect[0] + sprite.rect[2]) / 2.0 - (box_[0] + box_[2]) / 2.0).abs() <= 0.5
            );
            assert!(
                ((sprite.rect[1] + sprite.rect[3]) / 2.0 - (box_[1] + box_[3]) / 2.0).abs() <= 0.5
            );
        }

        // ④ And the ink is the quiet one, on both canvases.
        for palette in [DARK_CHROME, LIGHT_CHROME] {
            let drawn = sprites(&geometry, FormulaToolState::default(), &palette, 1.0, 1.0);
            assert!(
                drawn
                    .iter()
                    .all(|sprite| sprite.color == palette.formula_tool_glyph)
            );
        }
    }

    /// PIN (owner's ruling 2026-09-14 ②): **the pointer lays the strip's wash
    /// under one mark and the press lays the darker one.**
    ///
    /// The two washes have to be *different*, and that is the third assertion:
    /// a press that drew the hover's own ink would be a control with no held
    /// state at all, which is what this ruling was written to fix.
    ///
    /// MUTATIONS: use one ink for both → ③; put the pill under both marks → ①;
    /// leave the glyph at its quiet ink once lit → ④.
    #[test]
    fn a_hovered_mark_wears_the_pill_and_a_pressed_one_wears_the_darker_ink() {
        let geometry = boxes(MathBlockDisplay::Rendered);
        for palette in [DARK_CHROME, LIGHT_CHROME] {
            let hovered = sprites(
                &geometry,
                FormulaToolState {
                    hovered: Some(FormulaTool::CopyLatex),
                    ..FormulaToolState::default()
                },
                &palette,
                1.0,
                1.0,
            );
            // ① One pill, under the copy mark's box and nowhere else.
            let pill = pill_of(&hovered).expect("a hovered mark stands in a pill");
            assert_eq!(pill.rect, geometry.copy);
            assert_eq!(
                hovered
                    .iter()
                    .filter(|sprite| matches!(sprite.mark, ChromeMark::ControlPill { .. }))
                    .count(),
                1
            );
            // ② Its ink is the strip's control-pill wash.
            assert_eq!(pill.color, palette.formula_tool_pill);

            let pressed = sprites(
                &geometry,
                FormulaToolState {
                    hovered: Some(FormulaTool::CopyLatex),
                    pressed: Some(FormulaTool::CopyLatex),
                    ..FormulaToolState::default()
                },
                &palette,
                1.0,
                1.0,
            );
            let held = pill_of(&pressed).expect("a held mark stands in a pill too");
            // ③ And holding it darkens that wash, rather than repeating it.
            assert_eq!(held.color, palette.formula_tool_pill_pressed);
            assert_ne!(held.color, pill.color);

            // ④ The glyph rises out of its quiet ink the moment it is the
            //    subject, and the mark next to it does not.
            let lit = mark_of(&hovered, ChromeMark::Copy).expect("the copy mark");
            let quiet = mark_of(&hovered, ChromeMark::Code).expect("the source mark");
            assert_eq!(lit.color, palette.formula_tool_glyph_on_pill);
            assert_eq!(quiet.color, palette.formula_tool_glyph);
        }
    }

    /// PIN (owner's ruling 2026-09-14 ②): **a copy that landed says so, in the
    /// copy mark's own slot, and then gives the slot back.**
    ///
    /// The tick stands where the sheets were rather than beside them, which is
    /// the files foot's own idiom (`FOOT_REVEAL_FEEDBACK`) and the reason the
    /// row does not move while it is up.
    ///
    /// MUTATION: leave `copied` out of [`marks`] — the clipboard write becomes
    /// the one verb in this window with no visible effect at all.
    #[test]
    fn a_landed_copy_shows_the_houses_tick_in_the_copy_marks_own_box() {
        let geometry = boxes(MathBlockDisplay::Rendered);
        let said = sprites(
            &geometry,
            FormulaToolState {
                copied: true,
                ..FormulaToolState::default()
            },
            &DARK_CHROME,
            1.0,
            1.0,
        );
        let tick = mark_of(&said, ChromeMark::Check).expect("a landed copy wears the tick");
        assert!(mark_of(&said, ChromeMark::Copy).is_none(), "not both");
        assert!(
            tick.rect[0] >= geometry.copy[0] && tick.rect[2] <= geometry.copy[2],
            "the tick stands in the copy mark's own slot"
        );
        assert_eq!(tick.color, DARK_CHROME.accent);

        // And the slot comes back: the same call with the window closed is the
        // sheets again, in the same box.
        let after = sprites(
            &geometry,
            FormulaToolState::default(),
            &DARK_CHROME,
            1.0,
            1.0,
        );
        assert_eq!(
            mark_of(&after, ChromeMark::Copy)
                .expect("the sheets return")
                .rect,
            tick.rect
        );
    }

    /// PIN (owner's ruling 2026-09-14 ②): **the first mark names the view the
    /// press leads to, not the one the band is in.**
    ///
    /// The mock-up's own rule for the markdown flip (2170-2171), which
    /// `crate::float` already reads for the preview head. A band showing its
    /// rendered face offers source; a band showing source offers the rendering.
    ///
    /// MUTATION: swap the two arms — the eye that was drawn on every band until
    /// 2026-09-14 comes back, and it said "you are looking at a picture" on a
    /// block that was showing `$$…$$`.
    #[test]
    fn the_source_mark_names_where_the_press_goes() {
        assert_eq!(
            marks(MathBlockDisplay::Rendered, false)[0],
            ChromeMark::Code
        );
        assert_eq!(marks(MathBlockDisplay::Source, false)[0], ChromeMark::Eye);
    }

    /// PIN (owner's ruling 2026-09-14 ②): **the fade reaches the pill as well as
    /// the marks.**
    ///
    /// A pill arriving solid under a mark that is still coming up is the same
    /// defect the glance card's own fade was written against: one surface, two
    /// arrival times.
    #[test]
    fn the_fade_carries_every_piece_the_band_puts_up() {
        let drawn = sprites(
            &boxes(MathBlockDisplay::Rendered),
            FormulaToolState {
                hovered: Some(FormulaTool::ToggleSource),
                ..FormulaToolState::default()
            },
            &DARK_CHROME,
            1.0,
            0.5,
        );
        assert!(!drawn.is_empty());
        assert!(
            drawn.iter().all(|sprite| sprite.opacity == 0.5),
            "{drawn:?}"
        );
    }

    /// PIN (owner's ruling 2026-09-14 ②): **the marks come up over the tip's own
    /// ninety milliseconds, and over no second curve of this module's own.**
    ///
    /// Three claims. The duration is the house's fast tier and not a number
    /// written here (`MOTION_FAST`, and `TOOLTIP_FADE` is its one alias — the
    /// motion archive exists so that a fourth spelling of 90 cannot appear). The
    /// fade starts at nothing and lands at full, which is what "not drawn at
    /// rest" and "there once the hand has arrived" mean as arithmetic. And under
    /// `prefers-reduced-motion` there is no fade at all, on the first frame.
    ///
    /// **The mock-up says 100ms here** (`.math-tools { transition: opacity
    /// 100ms }`, 2117-2121) and the ruling says 90. The ruling wins, and not
    /// merely because it is newer: the 100 was struck before this window had a
    /// motion archive, and every other hover-revealed control in it — the pane
    /// head's run, the tip, the glance card — has since been collapsed onto the
    /// one fast span. A formula band keeping its own 100 would be the window
    /// making two different sounds at the same gesture, which is the argument
    /// the glance card's ruling turns on.
    ///
    /// MUTATIONS: bless a `Duration::from_millis(90)` of this module's own → ①;
    /// start the ramp at 1.0 → ②; ignore `Motion::Reduced` → ④.
    #[test]
    fn the_marks_arrive_on_the_tips_own_ninety_milliseconds() {
        use crate::tooltip::{TOOLTIP_FADE, hover_fade_opacity, hover_fade_owes_frames};
        use std::time::Duration;

        // ① One span, and it is the archive's fast tier.
        assert_eq!(TOOLTIP_FADE, bt_render::MOTION_FAST);
        assert_eq!(TOOLTIP_FADE, Duration::from_millis(90));

        // ② Nothing at the start, everything at the end, and something in
        //    between that is neither.
        assert_eq!(
            hover_fade_opacity(Duration::ZERO, crate::Motion::Full),
            0.0,
            "a mark that is already there has not faded in"
        );
        assert!((hover_fade_opacity(TOOLTIP_FADE, crate::Motion::Full) - 1.0).abs() < 0.001);
        let half = hover_fade_opacity(TOOLTIP_FADE / 2, crate::Motion::Full);
        assert!(half > 0.0 && half < 1.0, "{half}");

        // ③ And the ramp owes frames exactly while it is climbing.
        assert!(hover_fade_owes_frames(Duration::ZERO, crate::Motion::Full));
        assert!(!hover_fade_owes_frames(TOOLTIP_FADE, crate::Motion::Full));

        // ④ Reduced motion lands on the frame the band was entered.
        assert_eq!(
            hover_fade_opacity(Duration::ZERO, crate::Motion::Reduced),
            1.0
        );
        assert!(!hover_fade_owes_frames(
            Duration::ZERO,
            crate::Motion::Reduced
        ));

        // ⑤ And the two ends reach the sprites: nothing at zero, everything at
        //    the span's end.
        let geometry = boxes(MathBlockDisplay::Rendered);
        let state = FormulaToolState::default();
        assert!(
            sprites(
                &geometry,
                state,
                &DARK_CHROME,
                1.0,
                hover_fade_opacity(Duration::ZERO, crate::Motion::Full),
            )
            .is_empty()
        );
        assert!(
            sprites(
                &geometry,
                state,
                &DARK_CHROME,
                1.0,
                hover_fade_opacity(TOOLTIP_FADE, crate::Motion::Full),
            )
            .iter()
            .all(|sprite| sprite.opacity == 1.0)
        );
    }

    /// PIN: **the marks grow with the display, and so does their pill.**
    #[test]
    fn the_marks_and_the_pill_are_struck_at_the_displays_own_scale() {
        let geometry = MathToolBoxes {
            source: [400.0, 44.0, 448.0, 92.0],
            copy: [452.0, 44.0, 500.0, 92.0],
            ..boxes(MathBlockDisplay::Rendered)
        };
        let drawn = sprites(
            &geometry,
            FormulaToolState {
                hovered: Some(FormulaTool::ToggleSource),
                ..FormulaToolState::default()
            },
            &DARK_CHROME,
            2.0,
            1.0,
        );
        let pill = pill_of(&drawn).expect("a pill");
        let ChromeMark::ControlPill { radius_px } = pill.mark else {
            panic!("the pill is a pill");
        };
        assert_eq!(
            radius_px,
            (MATH_TOOL_PILL_RADIUS_LOGICAL_PX * 2.0).round() as u32
        );
        let mark = mark_of(&drawn, ChromeMark::Code).expect("the source mark");
        let one_to_one = sprites(
            &geometry,
            FormulaToolState::default(),
            &DARK_CHROME,
            1.0,
            1.0,
        );
        let small = mark_of(&one_to_one, ChromeMark::Code).expect("the source mark");
        assert!(
            (mark.rect[2] - mark.rect[0]) > (small.rect[2] - small.rect[0]),
            "a mark struck without the scale is the same size on a retina display"
        );
    }

    /// RED — **the mark under the pointer is lit, and the one beside it is
    /// not** (owner's report 2026-09-14 evening ①).
    ///
    /// The owner's sentence was that the pointer over `‹›` or `⧉` changes
    /// nothing at all, and the ink was never the fault: [`sprites`] has drawn a
    /// pill and the risen glyph for a hovered mark since the ruling that made
    /// these house marks. What was missing is the **state** — nobody kept which
    /// mark the pointer was on, so nobody could notice it changing, so the glass
    /// was never asked for a new picture and the band kept the one composed
    /// while the pointer was still out on the formula. This is that fact as
    /// arithmetic: crossing onto a mark is a *change*, and a change is what the
    /// overlay is rebuilt for.
    ///
    /// MUTATIONS: let [`FormulaToolFollow::follow`] ignore the mark it is handed
    /// and the first assertion fails — which is the build the owner was looking
    /// at. Answer `false` from it when only the hover moved and the second
    /// fails: the state would be right and the glass would never hear about it.
    /// Give the resting and the lit mark one ink and the last pair fails.
    #[test]
    fn the_mark_under_the_pointer_is_lit_and_the_one_beside_it_is_not() {
        let now = Instant::now();
        let geometry = boxes(MathBlockDisplay::Rendered);
        let mut follow = FormulaToolFollow::arriving(&geometry, None, now);
        let settled = now + tooltip::TOOLTIP_FADE;

        // At rest — the band is hovered, the marks are up, the pointer is on
        // neither of them — there is no pill and the glyphs wear the quiet ink.
        let resting = drawn(&follow, settled, Motion::Full);
        assert!(pill_of(&resting).is_none(), "{resting:?}");

        // ① The pointer crosses onto the source mark, and ② the glass is owed a
        //    new picture for it.
        assert!(
            follow.follow(
                &geometry,
                Some(FormulaTool::ToggleSource),
                settled,
                Motion::Full
            ),
            "a pointer arriving on a mark is a change the overlay has to be rebuilt for"
        );
        assert_eq!(follow.hovered(), Some(FormulaTool::ToggleSource));

        // ③ It stands in the strip's own control-pill wash, in its own box.
        let lit = drawn(&follow, settled, Motion::Full);
        let pill = pill_of(&lit).expect("a hovered mark stands in a pill");
        assert_eq!(pill.rect, geometry.source);
        assert_eq!(pill.color, DARK_CHROME.formula_tool_pill);

        // ④ And its ink has risen out of the resting one, while the mark beside
        //    it has not moved at all.
        let before = mark_of(&resting, ChromeMark::Code).expect("the source mark at rest");
        let after = mark_of(&lit, ChromeMark::Code).expect("the source mark, lit");
        assert_ne!(after.color, before.color);
        assert_eq!(after.color, DARK_CHROME.formula_tool_glyph_on_pill);
        assert_eq!(
            mark_of(&lit, ChromeMark::Copy)
                .expect("the copy mark")
                .color,
            DARK_CHROME.formula_tool_glyph
        );

        // ⑤ Leaving the mark for the band is a change too, and puts it back.
        assert!(follow.follow(&geometry, None, settled, Motion::Full));
        assert!(pill_of(&drawn(&follow, settled, Motion::Full)).is_none());

        // ⑥ And a pointer that has not moved asks for nothing: a band standing
        //    still costs the glass no frames at all.
        assert!(!follow.follow(&geometry, None, settled, Motion::Full));
        assert!(!follow.owes_frames(settled, Motion::Full));
    }

    /// RED — **a block that changed shape takes its marks with it, on the next
    /// frame and with no pointer move** (owner's report 2026-09-14 evening ②).
    ///
    /// The owner's case is a press on `‹›`: the block becomes its source, which
    /// is taller, and the two marks stayed beside the geometry that is not there
    /// any more until the pointer left the band and came back. The "which block"
    /// answer never changed — `same_block` says so, which is the point — only
    /// its rows and pixels did, and the same is true of a resize that re-wraps
    /// and of a scale change.
    ///
    /// MUTATIONS: place the marks once and keep the boxes — the follow answers
    /// `false` here and the first assertion fails, which is the build the owner
    /// reported. Compare anchors with `==` instead of `same_block` and the flip
    /// reads as a *different* block: the marks would be re-struck with a fresh
    /// fade instead of travelling, and ③ fails. Leave `display` where it was and
    /// ② fails — the eye would go on offering source on a block already showing
    /// it.
    #[test]
    fn a_block_that_changed_shape_takes_its_marks_with_it_with_no_pointer_move() {
        use std::time::Duration;

        let now = Instant::now();
        let rendered = boxes(MathBlockDisplay::Rendered);
        let mut follow = FormulaToolFollow::arriving(&rendered, None, now);
        let settled = now + tooltip::TOOLTIP_FADE;
        assert_eq!(follow.placed(settled, Motion::Full).source, rendered.source);

        let source_face = toggled(&rendered);
        assert!(
            source_face.anchor.same_block(&rendered.anchor),
            "a toggle changes the block's shape and not its identity"
        );

        // ① The very next frame the picture carries the new geometry, the marks
        //    are owed a new place — with no pointer event of any kind.
        assert!(
            follow.follow(&source_face, None, settled, Motion::Full),
            "the frame that publishes the new geometry is the frame the marks move on"
        );

        // ② And the first mark has already flipped: it names the view the press
        //    leads to, and the press has happened.
        let placed = follow.placed(settled, Motion::Full);
        assert_eq!(marks(placed.display, false)[0], ChromeMark::Eye);

        // ③ They travel rather than jump — somewhere in between at half the
        //    span, and neither of the two ends.
        let half = follow.placed(settled + tooltip::TOOLTIP_FADE / 2, Motion::Full);
        for (moving, from, to) in [
            (half.source, rendered.source, source_face.source),
            (half.copy, rendered.copy, source_face.copy),
        ] {
            assert_ne!(moving, from, "a mark still at the old boxes has not moved");
            assert_ne!(moving, to, "and one already at the new boxes has jumped");
            assert!(
                moving[1] > from[1] && moving[1] < to[1],
                "{moving:?} is not on the way from {from:?} to {to:?}"
            );
        }

        // ④ **No stale overlay survives the change.** By the end of the span the
        //    marks are at the new boxes exactly, and they stay there.
        for ms in [90, 200, 9_000] {
            let landed = follow.placed(settled + Duration::from_millis(ms), Motion::Full);
            assert_eq!(landed.source, source_face.source, "at {ms}ms");
            assert_eq!(landed.copy, source_face.copy, "at {ms}ms");
        }

        // ⑤ The journey owes the glass its frames while it runs, and none after.
        assert!(follow.owes_frames(settled, Motion::Full));
        assert!(follow.owes_frames(settled + tooltip::TOOLTIP_FADE / 2, Motion::Full));
        assert!(!follow.owes_frames(settled + tooltip::TOOLTIP_FADE, Motion::Full));
    }

    /// RED — **the marks fade in, travel and fade out on the tip's own ninety
    /// milliseconds** (owner's report 2026-09-14 evening ③).
    ///
    /// Three motions and one span, which is the whole of the clause: the arrival
    /// the ruling of that morning already gave them, the exit it explicitly
    /// withheld — the owner asked for it this evening — and the move ② is about.
    /// All three are read out of [`crate::tooltip::hover_fade_opacity`], so a
    /// day that changes the window's fast tier changes them together, and this
    /// module keeps no second copy of the curve or the number.
    ///
    /// MUTATIONS: take the marks down in one frame and the exit's climb-down
    /// fails. Start the exit at full instead of from where the fade had got to
    /// and the re-entry pin below fails. Spell a span of this module's own and
    /// the last pair fails.
    #[test]
    fn the_marks_fade_in_travel_and_fade_out_on_the_tips_own_ninety_milliseconds() {
        use std::time::Duration;

        let now = Instant::now();
        let geometry = boxes(MathBlockDisplay::Rendered);
        let mut follow = FormulaToolFollow::arriving(&geometry, None, now);

        // ① In: nothing on the frame they appear, climbing, full at ninety.
        assert_eq!(follow.opacity(now, Motion::Full), 0.0);
        assert!(drawn(&follow, now, Motion::Full).is_empty());
        let at = |ms: u64| follow.opacity(now + Duration::from_millis(ms), Motion::Full);
        for (earlier, later) in [(0, 20), (20, 45), (45, 70), (70, 89)] {
            assert!(
                at(earlier) < at(later),
                "{earlier}ms is not below {later}ms"
            );
        }
        assert_eq!(at(90), 1.0, "and it lands exactly");
        assert!(follow.owes_frames(now, Motion::Full));
        assert!(!follow.owes_frames(now + tooltip::TOOLTIP_FADE, Motion::Full));

        // ② Out: the same span, climbing down, and only then is there nothing
        //    left to draw.
        let left = now + tooltip::TOOLTIP_FADE;
        assert!(follow.leave(left, Motion::Full));
        let going = |ms: u64| follow.opacity(left + Duration::from_millis(ms), Motion::Full);
        for (earlier, later) in [(0, 20), (20, 45), (45, 70), (70, 89)] {
            assert!(
                going(earlier) > going(later),
                "{earlier}ms is not above {later}ms"
            );
        }
        assert_eq!(going(90), 0.0);
        assert!(
            !follow.gone(left, Motion::Full),
            "it is still on its way out"
        );
        assert!(follow.gone(left + tooltip::TOOLTIP_FADE, Motion::Full));
        assert!(follow.owes_frames(left, Motion::Full));
        assert!(!follow.owes_frames(left + tooltip::TOOLTIP_FADE, Motion::Full));

        // ③ And the fade reaches the glass rather than being a number nobody
        //    draws with.
        let halfway = drawn(&follow, left + tooltip::TOOLTIP_FADE / 2, Motion::Full);
        assert!(!halfway.is_empty());
        assert!(
            halfway
                .iter()
                .all(|sprite| sprite.opacity > 0.0 && sprite.opacity < 1.0)
        );

        // ④ One span for all three, and it is the archive's fast rung — this
        //    module spells neither the number nor the curve.
        assert_eq!(tooltip::TOOLTIP_FADE, bt_render::MOTION_FAST);
        // This module's own code, read as text — everything above the tests,
        // whose own clocks are written in milliseconds on purpose. The marker is
        // assembled rather than written out for the reason the house's other
        // text pins assemble theirs: a literal here would be a second occurrence
        // of the very string being searched for.
        const SOURCE: &str = include_str!("formula_tools.rs");
        let module = SOURCE
            .split(&["#[cfg(", "test)]"].concat())
            .next()
            .unwrap_or(SOURCE);
        for second_copy in [["from_", "millis"].concat(), ["cubic_", "bezier"].concat()] {
            assert!(
                !module.contains(&second_copy),
                "the band's marks keep a {second_copy} of their own"
            );
        }
    }

    /// RED — **stillness settles the arrival, the move and the exit at once**
    /// (owner's report 2026-09-14 evening ③, and the house rule under it).
    ///
    /// A reader who has asked the system for stillness gets the end state on the
    /// frame each of the three is asked for, and a window that wakes up for none
    /// of them. `Motion::Reduced` is honoured in exactly one place — the tip's
    /// own pair of functions — so there is no arm here to forget.
    ///
    /// MUTATION: read the fade's progress without the motion setting (a plain
    /// ratio of elapsed to span) and every one of these fails at once, which is
    /// the whole reason the journey asks that function rather than the clock.
    #[test]
    fn stillness_settles_the_arrival_the_move_and_the_exit_at_once() {
        let now = Instant::now();
        let geometry = boxes(MathBlockDisplay::Rendered);
        let mut follow = FormulaToolFollow::arriving(&geometry, None, now);

        // ① There, solid, on the frame it appears.
        assert_eq!(follow.opacity(now, Motion::Reduced), 1.0);
        assert!(!follow.owes_frames(now, Motion::Reduced));

        // ② And a block that changes shape simply *is* somewhere else.
        let source_face = toggled(&geometry);
        assert!(follow.follow(&source_face, None, now, Motion::Reduced));
        assert_eq!(
            follow.placed(now, Motion::Reduced).source,
            source_face.source
        );
        assert_eq!(follow.placed(now, Motion::Reduced).copy, source_face.copy);
        assert!(!follow.owes_frames(now, Motion::Reduced));

        // ③ And leaving is the frame it is asked for, with nothing left to draw.
        assert!(follow.leave(now, Motion::Reduced));
        assert_eq!(follow.opacity(now, Motion::Reduced), 0.0);
        assert!(drawn(&follow, now, Motion::Reduced).is_empty());
        assert!(follow.gone(now, Motion::Reduced));
    }

    /// RED — **a band re-entered before its exit landed turns round where it
    /// stands.**
    ///
    /// The 500ms grace forgives a pointer that clips the corner of a mark on its
    /// way to it; this forgives the ninety milliseconds after that. A journey is
    /// retargeted from the value it is showing, never restarted from the value it
    /// was going to, which is what keeps a hand that hesitates on the edge of a
    /// band from making the marks flash.
    ///
    /// MUTATION: restart the fade at `0.0` on the way back in — the marks blink
    /// out and climb again under a pointer that never left the band.
    #[test]
    fn a_band_re_entered_before_its_exit_landed_turns_round_where_it_stands() {
        let now = Instant::now();
        let geometry = boxes(MathBlockDisplay::Rendered);
        let mut follow = FormulaToolFollow::arriving(&geometry, None, now);
        let settled = now + tooltip::TOOLTIP_FADE;
        follow.leave(settled, Motion::Full);

        let halfway = settled + tooltip::TOOLTIP_FADE / 2;
        let caught = follow.opacity(halfway, Motion::Full);
        assert!(caught > 0.0 && caught < 1.0, "{caught}");

        assert!(follow.follow(&geometry, None, halfway, Motion::Full));
        assert_eq!(
            follow.opacity(halfway, Motion::Full),
            caught,
            "the way back begins from where the way out had got to"
        );
        assert_eq!(
            follow.opacity(halfway + tooltip::TOOLTIP_FADE, Motion::Full),
            1.0
        );
        assert!(!follow.gone(halfway + tooltip::TOOLTIP_FADE, Motion::Full));
    }

    /// RED — **a different block is an arrival, not a journey.**
    ///
    /// The one case that must *not* tween: marks sliding across the pane from
    /// one formula to another would be this window claiming the two bands are
    /// one surface. §7.1.5p ② already ruled on it for the fade — crossing
    /// straight from one formula to the next starts the second band's own — and
    /// the placement follows the same rule for the same reason.
    ///
    /// MUTATION: retarget on a different anchor and the marks crawl between two
    /// blocks with the first band's ink half spent.
    #[test]
    fn a_different_block_is_an_arrival_and_not_a_journey() {
        let now = Instant::now();
        let geometry = boxes(MathBlockDisplay::Rendered);
        let mut follow = FormulaToolFollow::arriving(&geometry, Some(FormulaTool::CopyLatex), now);
        let settled = now + tooltip::TOOLTIP_FADE;

        let next = another_band();
        assert!(!next.anchor.same_block(&geometry.anchor));
        assert!(follow.follow(&next, None, settled, Motion::Full));

        // Placed beside the new band at once, and coming up from nothing there —
        // no slide across the pane, and no ink inherited from the band left
        // behind.
        assert_eq!(follow.placed(settled, Motion::Full).source, next.source);
        assert_eq!(follow.opacity(settled, Motion::Full), 0.0);
        assert_eq!(follow.hovered(), None);
        assert_eq!(
            follow.opacity(settled + tooltip::TOOLTIP_FADE, Motion::Full),
            1.0
        );
    }

    /// PIN (owner's ruling 2026-09-15 ②): **a band's marks are the pane head's
    /// own buttons.**
    ///
    /// The ruling's sentence is about reuse — "the same size and ink as the
    /// pane-header buttons already in the app" — and the two numbers live in two
    /// crates that cannot see each other (`bt_render` gives the boxes, `bt_app`
    /// draws the marks in them). This is the one place that sees both, which is
    /// the arrangement `the_default_family_is_the_one_the_renderer_draws` keeps
    /// for the grid's own face.
    ///
    /// The inks are a derivation rather than a comparison: `formula_tool_glyph`
    /// *is* `ink3(termbg)`, which is `pane_close_glyph`'s own expression, and
    /// the two pills are the same ladder one rung apart — the hover wash and,
    /// under a held mark, the pane head's own. Asserting the numbers would pin
    /// the coincidence; asserting that the held mark wears the head's pill pins
    /// the sentence.
    ///
    /// MUTATIONS: put 24 back in `MATH_TOOL_BUTTON_LOGICAL_PX` → ①; strike the
    /// pill with a corner of its own → ②; give the marks an ink that is not the
    /// head's → ③.
    #[test]
    fn a_bands_marks_wear_the_pane_heads_own_button() {
        // ① The box, and ② the corner.
        assert_eq!(
            bt_render::MATH_TOOL_BUTTON_LOGICAL_PX,
            crate::seats::PANE_HEAD_TRIGGER_BOX_LOGICAL_PX
        );
        assert_eq!(
            MATH_TOOL_PILL_RADIUS_LOGICAL_PX,
            crate::seats::PANE_HEAD_TRIGGER_RADIUS_LOGICAL_PX
        );

        // ③ And the ink, on both canvases: the resting glyph is the head's
        //    resting glyph, and a held mark stands in the head's own pill.
        for palette in [DARK_CHROME, LIGHT_CHROME] {
            assert_eq!(palette.formula_tool_glyph, palette.pane_close_glyph);
            assert_eq!(palette.formula_tool_pill_pressed, palette.pane_close_pill);
        }
    }

    /// PIN (owner's ruling 2026-09-15 ②/③): **the marks are seated inside the
    /// block on both faces, and they slide along its right-hand midline as its
    /// height changes.**
    ///
    /// The seat is the ruling's ②; the slide is its ③, and the slide is the one
    /// half of that clause this module owns — the block's own height travel and
    /// the cross-fade between the picture and the source text are the
    /// projection's, not the overlay's. What is pinned here is that a mark never
    /// leaves the block during the ninety milliseconds, at either end or
    /// anywhere between: a mark that hung off the block for three frames would
    /// be the placement the ruling overturned, reappearing while the block
    /// resized.
    ///
    /// MUTATIONS: seat the marks from the ink rather than from the block, or
    /// from the block's top rather than its midline — `seated_inside_the_block`
    /// fails on one of the two faces; lerp the marks independently of the block
    /// box → the mid-travel assertion fails, because the block travels and they
    /// do not.
    #[test]
    fn the_marks_ride_the_blocks_right_hand_midline_while_it_changes_height() {
        let now = Instant::now();
        let rendered = boxes(MathBlockDisplay::Rendered);
        let source_face = toggled(&rendered);

        // ① Both endpoints are seats the ruling accepts.
        assert!(seated_inside_the_block(&rendered));
        assert!(seated_inside_the_block(&source_face));
        assert!(
            source_face.block[3] - source_face.block[1] > rendered.block[3] - rendered.block[1],
            "the source face is the taller of the two, which is what makes this a travel"
        );

        let mut follow = FormulaToolFollow::arriving(&rendered, None, now);
        let settled = now + tooltip::TOOLTIP_FADE;
        assert!(follow.follow(&source_face, None, settled, Motion::Full));

        // ② Every frame of the travel is a seat too — the block box is eased
        //    beside the two marks, so the three cannot come apart.
        for step in 0..=6 {
            let at = settled + tooltip::TOOLTIP_FADE * step / 6;
            let placed = follow.placed(at, Motion::Full);
            assert!(
                seated_inside_the_block(&placed),
                "at step {step} the marks left the block: {placed:?}"
            );
        }

        // ③ The endpoints are exact: the block box lands on the new one, not a
        //    fraction short of it, and so do the marks.
        let landed = follow.placed(settled + tooltip::TOOLTIP_FADE, Motion::Full);
        assert_eq!(landed.block, source_face.block);
        assert_eq!(landed.source, source_face.source);
        assert_eq!(landed.copy, source_face.copy);

        // ④ Halfway the block is genuinely between the two heights, and the
        //    marks are on *that* block's midline rather than on either end's.
        let half = follow.placed(settled + tooltip::TOOLTIP_FADE / 2, Motion::Full);
        assert!(
            half.block[3] > rendered.block[3] && half.block[3] < source_face.block[3],
            "the block's own height is not on the way: {:?}",
            half.block
        );

        // ⑤ And a reader who asked for stillness gets the far end on the frame
        //    the press happened, with no travel to watch and no frame owed.
        let mut still = FormulaToolFollow::arriving(&rendered, None, now);
        assert!(still.follow(&source_face, None, now, Motion::Reduced));
        let snapped = still.placed(now, Motion::Reduced);
        assert_eq!(snapped.block, source_face.block);
        assert_eq!(snapped.source, source_face.source);
        assert_eq!(snapped.copy, source_face.copy);
        assert!(!still.owes_frames(now, Motion::Reduced));
    }

    /// RED (owner's ruling 2026-09-15, T-MATH-TOGGLE-MOTION): **a band whose own height is
    /// travelling carries its marks instead of easing them towards it.**
    ///
    /// ⑦ iii's travel is for geometry that *jumped*, and until this ruling a toggle was such a
    /// jump. Now the block's height is itself a ninety-millisecond journey, and easing towards a
    /// box that is already easing is two journeys over one distance: the marks would trail the edge
    /// they are supposed to ride and settle a whole span after the block had stopped.
    ///
    /// MUTATION: call `follow` for a band that is changing face — the second arm below is exactly
    /// what the glass would then show, the marks still at the height the block has already left.
    #[test]
    fn a_band_that_is_travelling_carries_its_marks_rather_than_easing_them_to_it() {
        let now = Instant::now();
        let rendered = boxes(MathBlockDisplay::Rendered);
        let source_face = toggled(&rendered);
        let settled = now + tooltip::TOOLTIP_FADE;

        let mut ridden = FormulaToolFollow::arriving(&rendered, None, now);
        assert!(ridden.ride(&source_face, None, settled, Motion::Full));
        let placed = ridden.placed(settled, Motion::Full);
        assert_eq!(placed.block, source_face.block);
        assert_eq!(placed.source, source_face.source);
        assert_eq!(placed.copy, source_face.copy);
        assert!(seated_inside_the_block(&placed));

        let mut eased = FormulaToolFollow::arriving(&rendered, None, now);
        assert!(eased.follow(&source_face, None, settled, Motion::Full));
        assert_eq!(
            eased.placed(settled, Motion::Full).block,
            rendered.block,
            "a followed band starts its own journey where the marks already were"
        );

        // And a band that has stopped moving asks the glass for nothing.
        assert!(!ridden.ride(&source_face, None, settled, Motion::Full));
    }

    /// **The band once the change has landed**, as `bt_render` now answers for a source face
    /// (owner's ruling 2026-09-16, T-MATH-SOURCE-BAND-HUGS-TEXT): the region is the rows' own
    /// band, which begins at column zero and **stops where the longest of those rows stops**, plus
    /// the ground ⑨ i keeps round it — so the two marks stand in that ground's right-hand columns,
    /// centred in them on the band's midline, exactly as a typeset face's do. It was the whole
    /// pane until that ruling, and the marks were flush against the far edge of it.
    ///
    /// Deliberately a *wider* rectangle than [`toggled`]'s, which is the band mid-flight while the
    /// block is still an artifact: that difference is the leftover rect the pin below is about.
    fn source_band() -> MathToolBoxes {
        MathToolBoxes {
            display: MathBlockDisplay::Source,
            block: [8.0, 10.0, 400.0, 70.0],
            source: [355.0, 30.5, 374.0, 49.5],
            copy: [376.0, 30.5, 395.0, 49.5],
            ..boxes(MathBlockDisplay::Rendered)
        }
    }

    /// RED GATE (owner's report 2026-09-15, T-MATH-MARKS-IN-SOURCE-FACE; §7.1.5p ⑨ ii + ⑪):
    /// **when the change lands, the marks settle on the source band's own rect and keep nothing of
    /// the rect they rode.**
    ///
    /// The old pin waited another full span after settlement before inspecting
    /// the marks. T-MARKS-LANDING makes that same assertion on the landing
    /// instant: no old rectangle may survive it, even if interruption changed
    /// the endpoint by much more than the last travelling frame's distance.
    #[test]
    fn the_marks_settle_on_the_source_bands_own_rect_when_the_change_lands() {
        let now = Instant::now();
        let typeset = boxes(MathBlockDisplay::Rendered);
        let mut follow = FormulaToolFollow::arriving(&typeset, None, now);
        let settled = now + tooltip::TOOLTIP_FADE;

        // ① The flight: the marks are carried on the band, so they stand on the rect the artifact
        //    is presented at — which is a picture's region and not the rows'.
        let travelling = toggled(&typeset);
        assert!(follow.ride(&travelling, None, settled, Motion::Full));
        assert_eq!(follow.placed(settled, Motion::Full).block, travelling.block);

        // ② The landing settles on the rows' own rect in this very frame.
        let landed = source_band();
        assert!(follow.follow(&landed, None, settled, Motion::Full));
        let arrived = settled;
        let placed = follow.placed(arrived, Motion::Full);
        assert_eq!(
            placed.block, landed.block,
            "a typeset rect outlived the change"
        );
        assert_eq!(placed.source, landed.source);
        assert_eq!(placed.copy, landed.copy);
        assert_eq!(placed.display, MathBlockDisplay::Source);
        assert!(!follow.owes_frames(arrived, Motion::Full));

        // ③ And where they came to rest is a legal seat for a source face: inside the band, on its
        //    midline, in the room the band keeps at its right edge rather than jammed into the
        //    corner of it. Since the band hugs its rows (owner's ruling 2026-09-16) there is a
        //    reserve to be centred in, so a source face takes ⑨ ii's own seat and not the
        //    degradation of it the full-pane band left as the only possibility.
        assert!(seated_inside_the_block(&placed));
        assert!(
            placed.copy[2] < placed.block[2],
            "{:?} is jammed into the band's corner",
            placed.copy
        );
    }

    /// One cell of the grid these fixtures are measured on, in subpixels.
    const CELL: i64 = 18 * 1024;

    /// `[the typeset face's band height, the source rows' height]` — a three-row picture whose
    /// `$$…$$` source takes five rows, which is the ordinary shape of this gesture.
    const HEIGHTS: [i64; 2] = [3 * CELL, 5 * CELL];

    fn flight_anchor() -> MathBlockAnchor {
        MathBlockAnchor::History {
            run: None,
            start: bt_transcript::TranscriptId(4),
            end: bt_transcript::TranscriptId(7),
        }
    }

    fn source_lines() -> Vec<String> {
        [
            "$$",
            "\\int_0^\\infty e^{-x^2}\\,dx",
            "= \\frac{\\sqrt{\\pi}}{2}",
            "$$",
            "",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect()
    }

    fn source_face() -> bt_viewport::MathSourceFace {
        bt_viewport::MathSourceFace {
            height_subpixels: HEIGHTS[1],
            width_cells: 28,
            rows: source_lines(),
        }
    }

    fn flight(to_source: bool, now: Instant) -> FormulaToggleMotion {
        FormulaToggleMotion::begin(
            PasteTarget {
                tab: crate::TabId(7),
                seat: crate::SeatId(1),
                incarnation: 42,
            },
            flight_anchor(),
            HEIGHTS,
            to_source,
            source_face(),
            now,
        )
    }

    /// RED (owner's ruling 2026-09-15, T-MATH-TOGGLE-MOTION): **the block travels between the two
    /// heights it really stands at, and the two faces cross-fade across the same span.**
    ///
    /// Both ends are *exact*, in both directions, and that is the point rather than a nicety: the
    /// far end is the frame on which the document is told, so a band that stopped a subpixel short
    /// of the rows' own height would put the jump back — smaller, and still a jump.
    ///
    /// MUTATIONS: start the journey at the face it is going to, and the first frame is the switch
    /// this clause removes; fade the picture on a curve of its own and the halfway assertion's pair
    /// come apart, which is the window speaking with two voices about one gesture.
    #[test]
    fn a_change_of_face_travels_between_the_two_heights_the_block_really_stands_at() {
        let now = Instant::now();
        let landing = now + tooltip::TOOLTIP_FADE;

        let leaving = flight(true, now);
        assert_eq!(leaving.height_subpixels(now, Motion::Full), HEIGHTS[0]);
        assert_eq!(leaving.picture_opacity_milli(now, Motion::Full), 1000);
        assert_eq!(leaving.height_subpixels(landing, Motion::Full), HEIGHTS[1]);
        assert_eq!(leaving.picture_opacity_milli(landing, Motion::Full), 0);
        assert!(
            leaving.switch_owed(),
            "a block leaving its picture is told at the far end, which is where the identity is"
        );

        let returning = flight(false, now);
        assert_eq!(returning.height_subpixels(now, Motion::Full), HEIGHTS[1]);
        assert_eq!(returning.picture_opacity_milli(now, Motion::Full), 0);
        assert_eq!(
            returning.height_subpixels(landing, Motion::Full),
            HEIGHTS[0]
        );
        assert_eq!(returning.picture_opacity_milli(landing, Motion::Full), 1000);
        assert!(
            !returning.switch_owed(),
            "a block returning to its picture was told at the near end and owes nothing"
        );

        // Halfway is genuinely between, on both axes and by the same amount — one journey, so the
        // height and the cross-fade cannot be at different points of it.
        let half = now + tooltip::TOOLTIP_FADE / 2;
        let height = leaving.height_subpixels(half, Motion::Full);
        assert!(height > HEIGHTS[0] && height < HEIGHTS[1], "{height}");
        let travelled = (height - HEIGHTS[0]) as f32 / (HEIGHTS[1] - HEIGHTS[0]) as f32;
        assert!(
            (leaving.source_opacity(half, Motion::Full) - travelled).abs() < 0.001,
            "the picture and the band are at different points of one journey"
        );
        assert!(leaving.owes_frames(half, Motion::Full));
        assert!(!leaving.owes_frames(landing, Motion::Full));
    }

    /// RED — **the band's height really is drawn at every frame the display has, and it lands on
    /// the far face exactly** (review 2026-09-18, question ①).
    ///
    /// The clause above pins the two ends and the midpoint; what the review asked is whether the
    /// *loop* draws the middle — or whether the block arrives at its new height in one step while
    /// the two marks glide to it, which reads as a jerk however well the frames are paced. So this
    /// walks the loop the window actually runs: an admitted turn samples the journey, the present
    /// stamps the frame clock, and the deadline books the next one ([`crate::pace`]). Every one of
    /// those samples has to be a **different** height, or the frames in between are pictures of the
    /// same band and the travel is a step with padding around it.
    ///
    /// The count is the display's and not a constant: six frames of ninety milliseconds at 60 Hz,
    /// thirteen at 144. Fewer frames must draw the same journey rather than a shorter one, which is
    /// what the exact landing at the far end says.
    ///
    /// MUTATION: return the far height from the first frame — the one-frame switch this clause
    /// replaced — and the strict monotonicity goes red on the second sample rather than on the
    /// hundredth frame of somebody's screen.
    #[test]
    fn a_journey_draws_one_distinct_height_per_paced_frame() {
        for (millihertz, frames) in [(60_000_u32, 6_usize), (144_000, 13)] {
            let mut clock = crate::pace::FrameClock::default();
            assert!(clock.follow(Some(millihertz)));
            let start = Instant::now();
            let leaving = flight(true, start);

            let mut heights = Vec::new();
            let mut opacities = Vec::new();
            let mut presented = None;
            let mut now = start;
            while leaving.owes_frames(now, Motion::Full) {
                assert!(
                    clock.is_due(presented, now),
                    "the gate refused a frame its own deadline booked"
                );
                heights.push(leaving.height_subpixels(now, Motion::Full));
                opacities.push(leaving.picture_opacity_milli(now, Motion::Full));
                presented = Some(now);
                now = clock.next_frame(presented, now);
            }

            assert_eq!(
                heights.len(),
                frames,
                "a {millihertz} mHz display draws {frames} frames of ninety milliseconds: {heights:?}"
            );
            assert_eq!(heights[0], HEIGHTS[0], "the first frame is the near face");
            assert!(
                heights.windows(2).all(|pair| pair[0] < pair[1]),
                "two frames of a travelling band stand at two heights: {heights:?}"
            );
            assert!(
                opacities.windows(2).all(|pair| pair[0] > pair[1]),
                "and the picture thins on every one of them: {opacities:?}"
            );
            assert_eq!(
                opacities[0], 1000,
                "the first frame is the picture at full strength"
            );

            // And the frame the loop stops on is the far face to the subpixel — the frame on which
            // the document is told, so a band a subpixel short would put the jump back.
            assert!(leaving.landed(now, Motion::Full));
            assert_eq!(leaving.height_subpixels(now, Motion::Full), HEIGHTS[1]);
            assert_eq!(leaving.picture_opacity_milli(now, Motion::Full), 0);
        }
    }

    /// RED — **a journey under continuous unrelated traffic is drawn by that traffic's frames, and
    /// lands on its own clock** (review 2026-09-18, P1).
    ///
    /// The schedule the review built, and the failure it demonstrated: a formula begins its ninety
    /// milliseconds in one pane while a neighbouring pane prints every five milliseconds. Every one
    /// of those presents refuses the frame gate and postpones the debt, so the flight's own tick was
    /// admitted **zero** times in a thousand turns — and because that tick both sampled the journey
    /// *and* settled it, the band sat at the height the press left it at and could not land for as
    /// long as the neighbour kept printing.
    ///
    /// The repair is not to unpace the tick. It is that a journey is sampled by **whoever composes
    /// a frame** rather than by a tick that caches its answer: `Runtime::carry_live_journeys` is
    /// asked at the head of every compose, so the flood's own frames draw the journey, and the
    /// landing is a plain deadline ([`FormulaToggleMotion::lands_at`]) that no gate stands in front
    /// of. This walks that schedule over the real curve.
    ///
    /// **Three animations of three different kinds**, because the mechanism is the window's and not
    /// this block's: the band's height (a terminal picture, projected), the marks that ride it (an
    /// overlay layer), and the tip's own fade (the span every hover in this window is drawn on).
    ///
    /// MUTATION: sample any of them once and reuse it — which is what the cached presentation did —
    /// and its column of samples collapses to one value repeated.
    #[test]
    fn a_journey_under_a_flood_is_drawn_by_the_floods_own_frames() {
        const FLOOD: std::time::Duration = std::time::Duration::from_millis(5);
        let start = Instant::now();
        let leaving = flight(true, start);
        let geometry = boxes(MathBlockDisplay::Rendered);
        let marks = FormulaToolFollow::arriving(&geometry, None, start);

        let mut heights = Vec::new();
        let mut mark_opacities = Vec::new();
        let mut tip_opacities = Vec::new();
        let mut at = start;
        while at <= start + tooltip::TOOLTIP_FADE {
            // What a frame composed for somebody else draws, asked at the instant that frame is of.
            heights.push(leaving.height_subpixels(at, Motion::Full));
            mark_opacities.push((marks.opacity(at, Motion::Full) * 1000.0).round() as i64);
            tip_opacities.push(
                (tooltip::hover_fade_opacity(at.saturating_duration_since(start), Motion::Full)
                    * 1000.0)
                    .round() as i64,
            );
            at += FLOOD;
        }

        let drawn = tooltip::TOOLTIP_FADE.as_millis() as usize / FLOOD.as_millis() as usize;
        for (what, samples) in [
            ("the band's height", &heights),
            ("the marks' fade", &mark_opacities),
            ("the tip's fade", &tip_opacities),
        ] {
            assert_eq!(
                samples.len(),
                drawn + 1,
                "{what}: the flood composed {drawn} frames inside the ninety milliseconds"
            );
            assert!(
                samples.windows(2).all(|pair| pair[0] < pair[1]),
                "{what} stood still across the flood's frames: {samples:?}"
            );
        }
        // And the far end is exact on the frame the journey is due, whoever composed it.
        assert_eq!(
            leaving.height_subpixels(start + tooltip::TOOLTIP_FADE, Motion::Full),
            HEIGHTS[1]
        );

        // **The landing is the flight's own clock and no gate stands in front of it.** It is due
        // at the span's end, not at the end of the flood — which is the half of the review's
        // finding that made a busy neighbour able to hold a block half way over indefinitely.
        assert_eq!(leaving.lands_at(), start + tooltip::TOOLTIP_FADE);
        assert!(!leaving.landed(start + tooltip::TOOLTIP_FADE / 2, Motion::Full));
        assert!(leaving.landed(leaving.lands_at(), Motion::Full));
    }

    /// RED (owner's ruling 2026-09-15, T-MATH-TOGGLE-MOTION): **a second press turns the change
    /// round from where it stands, with no jump and without touching the document.**
    ///
    /// The whole reason this is possible is that the block is one entry with an artifact height for
    /// the length of the flight *in both directions*: a reversal therefore moves only which end the
    /// telling happens at, which is what [`FormulaToggleMotion::switch_owed`] answers.
    ///
    /// MUTATIONS: restart the journey from the face it set out from and the block snaps backwards
    /// under the hand; tell the document on the reversal and the block changes twice for one press.
    #[test]
    fn a_second_press_turns_the_change_round_where_it_stands() {
        let now = Instant::now();
        let half = now + tooltip::TOOLTIP_FADE / 2;
        let mut leaving = flight(true, now);

        let at_the_press = leaving.height_subpixels(half, Motion::Full);
        let fade_at_the_press = leaving.source_opacity(half, Motion::Full);
        leaving.reverse(HEIGHTS, source_face(), half, Motion::Full);

        assert_eq!(
            leaving.height_subpixels(half, Motion::Full),
            at_the_press,
            "the band jumped on the frame the hand changed its mind"
        );
        assert!(
            (leaving.source_opacity(half, Motion::Full) - fade_at_the_press).abs() < 0.001,
            "and so did the cross-fade"
        );
        assert!(
            !leaving.switch_owed(),
            "the block is heading back to the picture it never stopped being"
        );
        assert_eq!(
            leaving.height_subpixels(half + tooltip::TOOLTIP_FADE, Motion::Full),
            HEIGHTS[0],
            "and it lands exactly on the face it turned round towards"
        );
        assert_eq!(
            leaving.picture_opacity_milli(half + tooltip::TOOLTIP_FADE, Motion::Full),
            1000
        );
    }

    /// RED (owner's ruling 2026-09-15, T-MATH-TOGGLE-MOTION): **stillness is the far end on the
    /// frame it was asked for, and no frame is owed for any of it.**
    ///
    /// The tip's own answer, given here by the same function the marks beside this read — so there
    /// is no second arm to forget.
    ///
    /// MUTATION: pay the journey's frames under `Motion::Reduced` and a reader who asked for no
    /// motion watches a band grow; read a span of this surface's own and the setting is honoured in
    /// two places, one of which will drift.
    #[test]
    fn stillness_makes_a_change_of_face_a_single_frame() {
        let now = Instant::now();
        let leaving = flight(true, now);
        assert_eq!(leaving.height_subpixels(now, Motion::Reduced), HEIGHTS[1]);
        assert_eq!(leaving.picture_opacity_milli(now, Motion::Reduced), 0);
        assert!(leaving.landed(now, Motion::Reduced));
        assert!(!leaving.owes_frames(now, Motion::Reduced));

        let returning = flight(false, now);
        assert_eq!(returning.height_subpixels(now, Motion::Reduced), HEIGHTS[0]);
        assert_eq!(returning.picture_opacity_milli(now, Motion::Reduced), 1000);
        assert!(returning.landed(now, Motion::Reduced));
    }

    /// RED (owner's ruling 2026-09-15, T-MATH-TOGGLE-MOTION): **a change whose far end has moved
    /// under it stops being one this window may keep flying.**
    ///
    /// A re-wrap, a font change or a change to the breathing a band keeps moves the height the
    /// journey is travelling to. Landing there would put the block somewhere it does not stand, and
    /// the switch would jump by the difference — which is the very fault this clause removes, back
    /// again by another road. `Runtime::advance_math_toggle_if_due` asks this on every turn and
    /// settles the change the moment the answer is no.
    ///
    /// MUTATION: remember the endpoints instead of re-reading them and a pane resized mid-flight
    /// lands its block at the old width's height.
    #[test]
    fn a_change_whose_far_end_has_moved_is_no_longer_one_to_keep_flying() {
        let now = Instant::now();
        let leaving = flight(true, now);
        assert!(leaving.still_measures(HEIGHTS));
        assert!(!leaving.still_measures([HEIGHTS[0], HEIGHTS[1] + CELL]));
        // The face it is *leaving* may move all it likes: the journey is already past it, and what
        // has to be true is only where it lands.
        assert!(leaving.still_measures([HEIGHTS[0] + CELL, HEIGHTS[1]]));

        let returning = flight(false, now);
        assert!(returning.still_measures(HEIGHTS));
        assert!(!returning.still_measures([HEIGHTS[0] + CELL, HEIGHTS[1]]));
    }

    /// A band mid-change: five rows tall, standing 40px down the pane, in a pane 600px wide.
    fn band_face(rows_top: f32, band_height: f32) -> bt_render::MathBandFace {
        bt_render::MathBandFace {
            block: [24.0, rows_top, 300.0, rows_top + band_height],
            rows_top,
            rows_left: 32.0,
            rows_right: 600.0,
            row_height: 18.0,
            display: MathBlockDisplay::Rendered,
        }
    }

    /// RED (owner's ruling 2026-09-15, T-MATH-TOGGLE-MOTION): **the source face stands on the rows
    /// the block is about to reveal, and nowhere else.**
    ///
    /// Row `k` at `rows_top + k * row_height` is not a convention this module chose — it is where
    /// the transcript rows themselves land, so at the far end of the journey these labels and the
    /// real rows occupy the same boxes and the switch is invisible. Across, a source row is an
    /// ordinary row of this terminal and runs to the pane's edge rather than to the edge of the
    /// ground the picture kept; down, it may not spill onto the lines above and below a band that
    /// has not finished growing.
    ///
    /// MUTATIONS: lay the rows out from the block's left edge and every line of the source shifts a
    /// cell when the change lands; clip them to the *rows*' own boxes rather than to the band and a
    /// half-grown block writes its source over the text underneath it.
    #[test]
    fn the_source_face_stands_on_the_rows_the_block_is_about_to_reveal() {
        let ink = [200, 200, 200];
        let rows = source_lines();

        // The far end: the band is as tall as the five rows, so every non-empty row is drawn whole.
        let landed = band_face(40.0, 5.0 * 18.0);
        let labels = source_face_labels(&landed, &rows, ink, 13.0);
        assert_eq!(labels.len(), 4, "the empty fifth row asks for no draw");
        for (index, label) in labels.iter().enumerate() {
            let top = 40.0 + index as f32 * 18.0;
            assert_eq!(label.rect, [32.0, top, 600.0, top + 18.0]);
            assert_eq!(label.clip, Some([32.0, top, 600.0, top + 18.0]));
            assert!(
                label.mono,
                "a document's own bytes are set in the grid's face"
            );
            assert_eq!(label.color, ink);
        }

        // Half-way: the band is two rows tall, so the third row is cut by the band's own bottom and
        // the fourth is not drawn at all.
        let growing = band_face(40.0, 2.5 * 18.0);
        let labels = source_face_labels(&growing, &rows, ink, 13.0);
        assert_eq!(labels.len(), 3);
        assert_eq!(
            labels[2].rect,
            [32.0, 40.0 + 2.0 * 18.0, 600.0, 40.0 + 3.0 * 18.0]
        );
        assert_eq!(
            labels[2].clip,
            Some([32.0, 40.0 + 2.0 * 18.0, 600.0, 40.0 + 2.5 * 18.0]),
            "a row the band has not grown far enough to hold is cut by the band, not by itself"
        );
    }
}
