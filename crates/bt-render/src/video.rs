//! **The layer a playing video is drawn on, and the one rule its picture is
//! fitted by** (user ruling 2026-08-28, route B slice ①; `docs/DESIGN.md`
//! §7.42).
//!
//! # Why this is not the picture channel
//!
//! Every other raster in this window — a decoded photograph, a rastered PDF
//! page, a formula, the still taken out of a video — reaches the glass through
//! [`crate::PreviewImage`] and the shared texture cache behind it: an LRU keyed
//! by a string, sized by bytes, holding rasters that are uploaded once and
//! sampled for as long as they are on screen.
//!
//! A playing video is the opposite of every assumption in that sentence. Its
//! raster changes thirty or sixty times a second, so a cache keyed by content
//! would insert a new entry per frame and evict the whole of everything else
//! within a minute of playback; and it does not want a *new* texture per frame
//! either, because the size never changes and re-creating a texture is the one
//! part of an upload that is expensive. What a video wants is **one texture per
//! playing video, written over**, which is what [`VideoLayer`] is.
//!
//! # Two things the picture channel also cannot do
//!
//! **A rounded corner.** A floating window has one, and a picture drawn into it
//! square would sit in the corner the float's own face does not cover. The
//! preview pane's picture never needed this — it is inside a square pane — so
//! the mechanism does not exist: `rounded_overlay_fill` paints a rounded
//! *colour* by handing the rectangle pipeline a coverage-weighted run per row,
//! and there is no such trick for a sampled texture. So this layer carries a
//! radius and the shader masks by the signed distance to the rounded box, which
//! is the same shape `rounded_rect_coverage` computes on the CPU for a fill.
//!
//! **A ground of its own.** A picture that keeps its proportion inside a box
//! that is a different proportion leaves two bars, and something has to be in
//! them. In a pane that something is the pane's own body colour, already
//! painted underneath — so [`VideoLayer::ground`] is `None` there, and the
//! layer draws one quad. It is `Some` where nothing else has painted the box,
//! and then the ground is drawn by this same pipeline so that the *rounding* is
//! the same rounding: a square ground under a rounded picture would show four
//! corners of the wrong colour.
//!
//! # One fit rule, and it is not the picture channel's
//!
//! [`crate::preview_image_extent`] fits a picture inside a box **and never
//! enlarges it** — `scale.min(1.0)` — because a photograph blown past its own
//! pixels is a photograph somebody would rather see at its true size, and
//! because this window resamples on the CPU and inventing pixels there costs
//! memory.
//!
//! For a video that rule was measured to be wrong, twice, on the machine. A
//! 160×120 recording in a full-height pane was drawn 160×120: a postage stamp
//! in a field of ground (§7.23 ⑪, reported off `next12`). Route A's answer was
//! `object-fit: contain` in the shell page's stylesheet — `width:100%;
//! height:100%`, so the picture *fills* the pane at its own proportion — and
//! [`video_fit_extent`] is that same rule written where this window can apply
//! it.
//!
//! **The still and the playback share it**, which is the whole reason it is one
//! function. A pane showing the first frame of a video and the same pane a
//! moment later playing it are the same rectangle at the same size; a reader
//! pressing play and seeing the picture jump would be watching this window
//! disagree with itself about what "fits" means. `bt-app` fits the still with
//! this and the renderer fits the playing frame with this, and
//! `the_still_and_the_playback_share_one_fit_rule` is what says so.

use std::sync::Arc;

use crate::SeatViewport;

/// **The size a video is drawn at inside a box: as large as fits, keeping its
/// proportion.**
///
/// `contain` in the CSS sense, and unlike [`crate::preview_image_extent`] it
/// **enlarges**. A recording smaller than the box is scaled up to it; the
/// sampler does that on the GPU from the file's own pixels, so nothing is
/// resampled twice and nothing is stored twice.
///
/// `None` for a box or a source with a zero side, which is not a rectangle.
/// Otherwise the answer touches the box on at least one axis exactly — that is
/// what "fills" means, and it is the half of this rule a test can state without
/// naming a number.
#[must_use]
pub fn video_fit_extent(
    box_width_px: u32,
    box_height_px: u32,
    video_width_px: u32,
    video_height_px: u32,
) -> Option<(u32, u32)> {
    if box_width_px == 0 || box_height_px == 0 || video_width_px == 0 || video_height_px == 0 {
        return None;
    }
    let scale = (f64::from(box_width_px) / f64::from(video_width_px))
        .min(f64::from(box_height_px) / f64::from(video_height_px));
    let width = (f64::from(video_width_px) * scale).round() as u32;
    let height = (f64::from(video_height_px) * scale).round() as u32;
    Some((width.clamp(1, box_width_px), height.clamp(1, box_height_px)))
}

/// **Where inside `box_` the picture lands** — [`video_fit_extent`]'s size,
/// centred, in the same whole-surface pixels the box is expressed in.
///
/// `[left, top, right, bottom]`, and the leftover on each axis is split by a
/// floor rather than shared out: a one-pixel letterbox is a pixel of ground on
/// one side and nothing on the other, which is what every other centred thing in
/// this window does and is invisible either way.
#[must_use]
pub fn video_frame_rect(
    box_: SeatViewport,
    video_width_px: u32,
    video_height_px: u32,
) -> Option<[f32; 4]> {
    let (width, height) =
        video_fit_extent(box_.width, box_.height, video_width_px, video_height_px)?;
    let left = box_.x as f32 + ((box_.width - width) / 2) as f32;
    let top = box_.y as f32 + ((box_.height - height) / 2) as f32;
    Some([left, top, left + width as f32, top + height as f32])
}

/// **One decoded frame on its way to a texture.**
///
/// `bgra` is straight (non-premultiplied) BGRA8, row-major, `width_px *
/// height_px * 4` bytes — the byte order `bt_platform::video::engine` asks the
/// decoder for, so that nothing between the demuxer and the sampler touches a
/// channel.
///
/// `generation` is what makes the upload conditional. A window redraws for a
/// hundred reasons that are not a new video frame — a cursor blink, a hover, a
/// resize — and a layer that uploaded on every one of them would spend a
/// megabyte of bus per redraw to write the pixels that are already there. The
/// renderer keeps the last generation it wrote per key and skips anything not
/// newer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoFrameUpload {
    pub bgra: Arc<[u8]>,
    pub width_px: u32,
    pub height_px: u32,
    /// Strictly increasing for the life of one playing video, starting at one.
    pub generation: u64,
}

/// **Which of this window's three stacks a playing video is drawn in** (route B
/// slice ②, 2026-08-28; `docs/DESIGN.md` §7.44 ③).
///
/// Slice ① had one answer because it had one surface. A video now plays on three
/// — a preview pane, a floating window and a hover card — and those are not
/// three positions in one list: the pane's picture belongs in the slot the
/// pane's own still is in, and the other two belong inside the overlay layer
/// their surface *is*, each with its own things above and below it. A layer that ignored the
/// difference would be right about the pane and wrong about the other two in
/// opposite directions: drawn in the pane's slot, a float's video sits **under**
/// the float's own opaque face and is invisible; drawn last of everything, it
/// sits over every float stacked above it and over the modal scrim.
///
/// **This is [`WebHole::above`]'s shape, and deliberately so.** That field
/// solves the identical problem for the rectangle a browser is composed
/// through — one hole per surface, punched in that surface's own place in the
/// z-order rather than at one global height — and a second mechanism with a
/// different spelling for the same question would be the second authority this
/// renderer keeps not having.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoStage {
    /// **The preview seat's picture slot** — over the seat's body chrome, under
    /// the peek card, under every floating window. Slice ①'s only answer, and
    /// still the pane's: a video and a still are the same rectangle in the same
    /// place (§7.42 ⑤), so they are drawn in the same slot.
    Seat,
    /// **Directly above one overlay layer's ground** — a floating window's
    /// body, or a glance card's picture window.
    ///
    /// One variant covers both because in this window both *are* overlay
    /// layers: a card is `OverlayStack::file_peek` and a float is
    /// `OverlayStack::float`, and a second variant naming the card would be a
    /// second spelling of the same position.
    ///
    /// The index is into the list [`crate::WindowRenderer::set_modal_overlay`]
    /// was last given, exactly as [`WebHole::above`]'s is, and a layer naming an
    /// index that is not there draws nothing: it is a video whose window has
    /// gone, and the honest picture of that is the window not being there.
    ///
    /// Above the ground and **below** that layer's own fills, marks and
    /// captions, which is what puts a float's hairlines, its head and this
    /// video's own control bar over the picture instead of under it.
    Overlay(usize),
}

/// **A playing video, and the box it plays in.**
///
/// One per playing engine. The renderer is handed the whole list every frame —
/// the same shape [`crate::WindowRenderer::set_preview_bodies`] takes, and for
/// the same reason: the caller recomputes the rectangles from the layout anyway,
/// so there is no cheaper half to move separately, and a key that stops
/// appearing is a video that has stopped, which is how its texture is released.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoLayer {
    /// The identity of the texture across frames. One playing video, one key —
    /// so two panes showing the same file at different positions are two keys,
    /// because they are two engines and two pictures.
    pub key: String,
    /// The box the picture is fitted into, in whole-surface pixels.
    pub box_: SeatViewport,
    /// The box it may be *seen* in — the scissor. Equal to [`Self::box_`] at
    /// rest and different for exactly the reason [`crate::PreviewImage::clip`]
    /// is: a pane in flight is drawn at its destination size and cropped by the
    /// animating rectangle on its way there.
    pub clip: SeatViewport,
    /// The newest frame, or `None` when nothing has been decoded yet — a video
    /// that has been opened and not yet played. The ground still draws, which is
    /// the difference between a pane that is loading and a pane that is broken.
    pub frame: Option<VideoFrameUpload>,
    /// What the letterbox bars are painted in, or `None` when something
    /// underneath has already painted the box — which is the pane case, where
    /// the bars are the pane's own body colour and drawing them again would be a
    /// second authority for it.
    pub ground: Option<[u8; 3]>,
    /// The corner radius of both the ground and the picture, in physical
    /// pixels. Zero in a pane; a float's own radius in a float.
    pub radius_px: f32,
    /// A uniform multiplier on the whole layer, for an arrival or a departure.
    pub opacity: f32,
    /// Which of the renderer's three stacks this one is drawn in. See
    /// [`VideoStage`].
    pub stage: VideoStage,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RED — **the still and the playback are fitted by one rule, and that rule
    /// fills the box** (user ruling 2026-08-28; `docs/DESIGN.md` §7.42).
    ///
    /// Three claims, and the third is the one that makes the first two worth
    /// asserting.
    ///
    /// ① **It fills.** For every shape a recording comes in, against every shape
    /// a pane comes in, the answer touches the box exactly on one axis. A rule
    /// that only shrank would fail this for the small-clip case and pass it for
    /// every other, which is exactly the defect that shipped in `next12`: the
    /// bug was invisible on a 4K capture and unmissable on a screen recording of
    /// a terminal.
    ///
    /// ② **It keeps the proportion.** Within a pixel of rounding, the answer's
    /// aspect ratio is the source's.
    ///
    /// ③ **The rectangle the renderer draws is that size, centred.**
    /// [`video_frame_rect`] is what the playing layer is drawn by and
    /// [`video_fit_extent`] is what `bt-app` fits the still with; if they could
    /// part company, "the still and the playback share one fit rule" would be a
    /// comment rather than a property. They cannot, because one calls the other.
    ///
    /// RED GATE: put back the `.min(1.0)` that [`crate::preview_image_extent`]
    /// carries — which is the rule the still was fitted by before this slice —
    /// and the small-clip cases in ① go red while every other line stays green.
    #[test]
    fn the_still_and_the_playback_share_one_fit_rule() {
        let boxes = [(1200_u32, 800_u32), (280, 160), (400, 400), (1920, 1080)];
        let sources = [
            (160_u32, 120_u32), // the fixture: far smaller than any pane
            (1920, 1080),
            (1080, 1920),
            (640, 640),
        ];
        for (box_width, box_height) in boxes {
            for (source_width, source_height) in sources {
                let (width, height) =
                    video_fit_extent(box_width, box_height, source_width, source_height)
                        .expect("two real rectangles fit");
                // ① one axis is filled exactly, and neither overflows.
                assert!(width <= box_width && height <= box_height);
                assert!(
                    width == box_width || height == box_height,
                    "{source_width}x{source_height} in {box_width}x{box_height} \
                     filled neither axis: {width}x{height}"
                );
                // ② the proportion survives.
                let wanted = f64::from(source_width) / f64::from(source_height);
                let got = f64::from(width) / f64::from(height);
                assert!(
                    (wanted - got).abs() < 0.02,
                    "{source_width}x{source_height} became {width}x{height}"
                );
                // ③ the drawn rectangle is that size, centred in the box.
                let box_ = SeatViewport {
                    x: 40,
                    y: 12,
                    width: box_width,
                    height: box_height,
                };
                let [left, top, right, bottom] =
                    video_frame_rect(box_, source_width, source_height).expect("a rectangle");
                assert_eq!((right - left) as u32, width);
                assert_eq!((bottom - top) as u32, height);
                assert_eq!(left as u32, box_.x + (box_width - width) / 2);
                assert_eq!(top as u32, box_.y + (box_height - height) / 2);
            }
        }
        // And the difference from the picture channel's rule, stated once: a
        // recording smaller than its box is *enlarged* here and is not there.
        assert_eq!(video_fit_extent(1200, 800, 160, 120), Some((1067, 800)));
        assert_eq!(
            crate::preview_image_extent(1200, 800, 160, 120),
            Some((160, 120)),
            "the picture channel still never enlarges, which is its own ruling"
        );
    }

    /// PIN — **a rectangle with a zero side is not a rectangle.**
    ///
    /// A pane mid-collapse, a video whose metadata has not arrived, a box behind
    /// a divider dragged to the edge. All three answer `None` rather than a
    /// division by zero or a one-pixel smear.
    #[test]
    fn a_box_or_a_source_with_no_area_has_no_fit() {
        assert_eq!(video_fit_extent(0, 800, 160, 120), None);
        assert_eq!(video_fit_extent(1200, 0, 160, 120), None);
        assert_eq!(video_fit_extent(1200, 800, 0, 120), None);
        assert_eq!(video_fit_extent(1200, 800, 160, 0), None);
        assert_eq!(video_frame_rect(SeatViewport::whole(0, 0), 160, 120), None);
    }

    /// PIN — **a fit that rounds to nothing is still a pixel.**
    ///
    /// A very wide recording in a very narrow box scales its short side below
    /// half a pixel; the clamp is what keeps that a one-pixel line rather than a
    /// zero-height quad the driver would reject.
    #[test]
    fn a_fit_never_rounds_a_side_away() {
        let (width, height) = video_fit_extent(100, 100, 10_000, 1).expect("a fit");
        assert_eq!(width, 100);
        assert!(height >= 1, "{height}");
    }
}
