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
//! **Nothing in this module decides where a mark goes** — the renderer does,
//! from the band's own geometry — and nothing in it holds a clock. It is handed
//! the boxes, what the pointer is doing, and one opacity, and it answers with
//! sprites; that is the same division `seats.rs` keeps with the solver, and it
//! is what lets every clause of the ruling be pinned without a GPU.

use bt_render::{ChromePalette, MATH_TOOL_PILL_RADIUS_LOGICAL_PX, MathToolBoxes};
use bt_viewport::MathBlockDisplay;

use crate::{
    icons::MarkSlot,
    marks::{ChromeMark, ChromeSprite},
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

#[cfg(test)]
mod tests {
    use super::*;
    use bt_render::{DARK_CHROME, LIGHT_CHROME};

    fn boxes(display: MathBlockDisplay) -> MathToolBoxes {
        MathToolBoxes {
            anchor: bt_viewport::MathBlockAnchor::History {
                run: None,
                start: bt_transcript::TranscriptId(1),
                end: bt_transcript::TranscriptId(1),
            },
            display,
            block: [40.0, 10.0, 200.0, 58.0],
            source: [200.0, 22.0, 224.0, 46.0],
            copy: [226.0, 22.0, 250.0, 46.0],
        }
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
}
