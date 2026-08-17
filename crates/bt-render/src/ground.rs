//! The window's ground: a picture behind everything, and how much of the
//! window is there at all — `docs/DESIGN.md` §7.1.6c-4b.
//!
//! # What "the ground" is, exactly
//!
//! One value, and it is already in the renderer: **the clear colour**.
//! `rectangles()` emits a quad per cell only where the resolved background
//! differs from the theme's default, so the surface every pane appears to be
//! painted on is literally the `LoadOp::Clear` at the top of the frame. That is
//! why translucency costs one alpha and not a pass over every surface in the
//! product: put an alpha on the clear and the ground is translucent; every glyph,
//! rule, menu, dialog and float drawn afterwards blends over it with
//! `BlendState::ALPHA_BLENDING`, whose alpha half is `{One, OneMinusSrcAlpha}` —
//! an opaque source over a translucent destination comes out opaque, with no
//! change anywhere. **"Text stays opaque" is not a rule this module enforces; it
//! is the arithmetic that was already there.**
//!
//! # The picture is one quad, under everything
//!
//! Between the clear and the first seat, one quad over the whole surface. It is
//! per **window** and not per pane because a split is two views of one place: a
//! picture drawn per pane would cut itself at every divider and jump every time
//! one was dragged. It also costs the same at one pane as at nine.
//!
//! The quad's fragment writes the *finished* ground and its blend is `Replace`,
//! because it is not compositing onto the clear — it is replacing it:
//!
//! ```text
//! p   = image opacity × the image's own alpha      (how much picture, per texel)
//! rgb = A · (img·p + bg·(1 − p))                   (picture mixed into the scheme's background)
//! a   = A                                          (A = the ground opacity)
//! ```
//!
//! One rule, and both percentages do exactly what their names say: `p` decides
//! how much of the picture is in the ground, `A` decides how much of the ground
//! is in the window. All of it in **linear** light, because the surface format is
//! sRGB and wgpu encodes a linear clear value once — the same reason
//! `theme_clear_color` has always linearised.
//!
//! # Fit is a pure function of two sizes
//!
//! [`background_uv_rect`] returns the texture rectangle the full-window quad
//! samples, and nothing else in the pipeline changes between the three fits. No
//! CPU resample happens on resize: the sampler is already doing that work, so a
//! live drag costs zero re-uploads and zero worker turns, and the texture the
//! window holds stays the one the file decoded to.

use std::sync::{Arc, OnceLock, RwLock};

use crate::theme::{ThemeChange, bump_theme_revision};

/// How a picture meets a window that is not its shape.
///
/// The mirror of `bt_persist::BackgroundFitV1`, kept as its own type for the
/// reason `Theme` is not `ThemeModeV1`: this crate must not depend on the file
/// format, and the file format must not learn what a UV rectangle is.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BackgroundFit {
    /// The window's shape wins. UV is the whole texture; the aspect ratio is
    /// whatever the window's is.
    Stretch,
    /// The picture's shape wins and the window is covered. The overflowing axis
    /// is cropped evenly at both ends, so the middle of the picture is the
    /// middle of the window.
    #[default]
    Fill,
    /// Neither is scaled. The picture repeats at its own pixel size from the
    /// window's top-left, which is why this is the one fit that needs a `Repeat`
    /// sampler.
    Tile,
}

/// A decoded picture, ready to become one texture.
///
/// `Arc<[u8]>` in straight (unpremultiplied) sRGB RGBA8, tightly packed — the
/// byte contract `bt_term::DecodedInlineImage` and the math rasters already
/// share, because this is the same decoder's output and there is only one
/// decoder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackgroundImage {
    /// Content identity, and therefore texture identity. Two windows naming the
    /// same file ask the device the same question and get one texture.
    pub key: String,
    pub rgba: Arc<[u8]>,
    pub width_px: u32,
    pub height_px: u32,
}

/// Everything the ground is, in one value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowGround {
    /// The picture, or none. `Arc` because the frame path reads it under a lock
    /// it must not hold while uploading.
    pub image: Option<Arc<BackgroundImage>>,
    pub fit: BackgroundFit,
    /// How much of the picture reaches the window, 0.0..=1.0.
    pub image_opacity: f32,
    /// How much of the ground reaches the window, [`MINIMUM_GROUND_ALPHA`]..=1.0.
    pub alpha: f32,
}

impl WindowGround {
    /// An opaque window with no picture — what every build before this one drew.
    #[must_use]
    pub fn opaque() -> Self {
        Self {
            image: None,
            fit: BackgroundFit::default(),
            image_opacity: 1.0,
            alpha: 1.0,
        }
    }

    /// Whether this ground is the one the clear alone can draw.
    ///
    /// The frame path's one question: a ground with no picture at full alpha
    /// needs neither the extra quad nor the extra pipeline, and that is the
    /// overwhelmingly common case.
    #[must_use]
    pub fn is_plain(&self) -> bool {
        self.image.is_none()
    }
}

/// The floor under the ground's alpha — `bt_persist::MINIMUM_BACKGROUND_OPACITY`
/// as a fraction, and the same ruling.
///
/// Restated here rather than imported because this crate does not depend on the
/// file format; the pin `the_floor_here_is_the_floor_in_the_file_format` holds
/// the two together.
pub const MINIMUM_GROUND_ALPHA: f32 = 0.3;

fn process_ground() -> &'static RwLock<WindowGround> {
    static GROUND: OnceLock<RwLock<WindowGround>> = OnceLock::new();
    GROUND.get_or_init(|| RwLock::new(WindowGround::opaque()))
}

/// The ground in force, cloned out from under the lock.
///
/// A poisoned lock is read through rather than panicked on, for
/// `active_schemes`' reason: the write path replaces the whole value in one
/// move, so the worst a panicking writer can have done is fail before assigning,
/// and the previous ground is a better answer than taking every window down.
#[must_use]
pub fn window_ground() -> WindowGround {
    process_ground()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Put a ground in force and bump `theme_revision`.
///
/// **One call for all four**, `set_schemes`' reason: they are one decision made
/// in four parts, and setting them separately would mean four revisions and
/// three wasted repaints for a state nobody was ever shown. Returns
/// [`ThemeChange::Unchanged`] when nothing moved, so a settings write that did
/// not touch the ground costs nothing.
///
/// The revision is the same one a theme flip advances, which is the whole reason
/// this lives beside the palettes rather than in a struct of its own: every
/// artefact keyed on it — CPU math rasters, their GPU textures, the composed-row
/// cache — is invalidated by one mechanism and not by a second list somebody has
/// to remember to extend.
pub fn set_window_ground(ground: WindowGround) -> ThemeChange {
    let ground = WindowGround {
        image_opacity: ground.image_opacity.clamp(0.0, 1.0),
        alpha: ground.alpha.clamp(MINIMUM_GROUND_ALPHA, 1.0),
        ..ground
    };
    {
        let mut current = process_ground()
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *current == ground {
            return ThemeChange::Unchanged;
        }
        *current = ground;
    }
    bump_theme_revision();
    ThemeChange::Changed
}

/// The texture rectangle a full-window quad samples, as `(u0, v0, u1, v1)`.
///
/// Pure, and in **physical** pixels on both sides — Tile is the fit that makes
/// that matter: a picture repeating at "its own size" means its own size on the
/// screen, so a window moved to a 200% monitor shows half as many copies of it,
/// which is what a texture is expected to do.
///
/// Zero on any of the four dimensions yields the whole texture: a quad is about
/// to be skipped anyway, and a division is not a way to find that out.
#[must_use]
pub fn background_uv_rect(
    fit: BackgroundFit,
    window_width_px: u32,
    window_height_px: u32,
    image_width_px: u32,
    image_height_px: u32,
) -> [f32; 4] {
    let window_width = window_width_px as f32;
    let window_height = window_height_px as f32;
    let image_width = image_width_px as f32;
    let image_height = image_height_px as f32;
    if window_width_px == 0 || window_height_px == 0 || image_width_px == 0 || image_height_px == 0
    {
        return [0.0, 0.0, 1.0, 1.0];
    }
    match fit {
        BackgroundFit::Stretch => [0.0, 0.0, 1.0, 1.0],
        BackgroundFit::Fill => {
            // Cover: the larger of the two scales is the one that leaves no gap,
            // and the axis that did not need it is the axis that gets cropped.
            // The visible span on each axis is the window measured in units of
            // the scaled picture, which is `1.0` exactly on the axis that set
            // the scale — so one of these two is always the whole texture and
            // the other is centred inside it.
            let scale = (window_width / image_width).max(window_height / image_height);
            let u_span = (window_width / (image_width * scale)).min(1.0);
            let v_span = (window_height / (image_height * scale)).min(1.0);
            let u0 = (1.0 - u_span) / 2.0;
            let v0 = (1.0 - v_span) / 2.0;
            [u0, v0, u0 + u_span, v0 + v_span]
        }
        // Anchored top-left rather than centred: a tile's grid has to start
        // somewhere, and a centred one moves every seam by half a pixel every
        // time the window resizes by an odd number.
        BackgroundFit::Tile => [
            0.0,
            0.0,
            window_width / image_width,
            window_height / image_height,
        ],
    }
}

/// The clear colour for a ground of this alpha, premultiplied in linear light.
///
/// Premultiplied because the swapchain is configured `PreMultiplied` (§2.3 A2) —
/// straight alpha on a premultiplied surface is the bug that shows as a window
/// whose translucent parts are too bright, and it is invisible on a dark
/// desktop, which is why `BT_BG=#FFFFFF` is in the acceptance shots.
///
/// In **linear** and not sRGB: the surface format is sRGB and wgpu encodes the
/// clear value once, so the multiply has to happen on the same side of that
/// encode as the colour does.
#[must_use]
pub fn premultiplied_clear(linear_rgb: [f64; 3], alpha: f32) -> wgpu::Color {
    let alpha = f64::from(alpha.clamp(0.0, 1.0));
    wgpu::Color {
        r: linear_rgb[0] * alpha,
        g: linear_rgb[1] * alpha,
        b: linear_rgb[2] * alpha,
        a: alpha,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — Stretch samples the whole picture whatever either shape is.
    #[test]
    fn stretch_always_takes_the_whole_texture() {
        for (window, image) in [
            ((1920, 1080), (1920, 1080)),
            ((800, 600), (3840, 2160)),
            ((100, 4000), (7, 7)),
        ] {
            assert_eq!(
                background_uv_rect(BackgroundFit::Stretch, window.0, window.1, image.0, image.1),
                [0.0, 0.0, 1.0, 1.0],
                "window {window:?} over image {image:?}"
            );
        }
    }

    /// PIN — Fill crops the axis that did not set the scale, evenly, and leaves
    /// the other one whole.
    ///
    /// The three cases are the three shapes the arithmetic has to tell apart: a
    /// window wider than the picture (crop top and bottom), taller (crop left
    /// and right), and the same shape (crop nothing). The "crop nothing" case is
    /// the one a `max`/`min` mix-up still passes the other two while breaking.
    #[test]
    fn fill_crops_only_the_axis_that_overflows_and_crops_it_evenly() {
        // 16:9 window, 1:1 picture — the picture is too tall, so the top and
        // bottom go. The visible band is 9/16 of the picture's height, centred.
        let [u0, v0, u1, v1] = background_uv_rect(BackgroundFit::Fill, 1600, 900, 1000, 1000);
        assert!(
            (u0 - 0.0).abs() < 1e-6 && (u1 - 1.0).abs() < 1e-6,
            "{u0} {u1}"
        );
        assert!((v1 - v0 - 0.5625).abs() < 1e-6, "{v0} {v1}");
        assert!(
            (v0 - (1.0 - 0.5625) / 2.0).abs() < 1e-6,
            "the crop is shared between the two ends, not taken off one: {v0}"
        );

        // 1:2 window, 2:1 picture — now it is the sides that go, and hard: the
        // visible band is a quarter of the picture's width.
        let [u0, v0, u1, v1] = background_uv_rect(BackgroundFit::Fill, 500, 1000, 2000, 1000);
        assert!(
            (v0 - 0.0).abs() < 1e-6 && (v1 - 1.0).abs() < 1e-6,
            "{v0} {v1}"
        );
        assert!((u1 - u0 - 0.25).abs() < 1e-6, "{u0} {u1}");
        assert!((u0 - 0.375).abs() < 1e-6, "{u0}");

        // Same shape at a different size — nothing is cropped, and this is the
        // case a `min`/`max` swap survives the other two on.
        assert_eq!(
            background_uv_rect(BackgroundFit::Fill, 1280, 720, 3840, 2160),
            [0.0, 0.0, 1.0, 1.0]
        );
    }

    /// PIN — Tile counts copies in physical pixels, from the top-left, and lets
    /// the last one be a fraction.
    ///
    /// The fractional part is the point: a window is never a whole number of
    /// tiles, and a fit that rounded the count would either scale the picture
    /// (which is the other two fits' job) or leave a bare strip at the edge.
    #[test]
    fn tile_repeats_at_the_pictures_own_size_from_the_top_left() {
        assert_eq!(
            background_uv_rect(BackgroundFit::Tile, 1920, 1080, 640, 360),
            [0.0, 0.0, 3.0, 3.0]
        );
        let [u0, v0, u1, v1] = background_uv_rect(BackgroundFit::Tile, 1000, 700, 300, 300);
        assert_eq!((u0, v0), (0.0, 0.0));
        assert!((u1 - 10.0 / 3.0).abs() < 1e-6, "{u1}");
        assert!((v1 - 7.0 / 3.0).abs() < 1e-6, "{v1}");
        // A picture larger than the window shows less than one copy of itself —
        // Tile does not fall back to Fit when it would only draw part of a tile.
        assert_eq!(
            background_uv_rect(BackgroundFit::Tile, 400, 300, 800, 600),
            [0.0, 0.0, 0.5, 0.5]
        );
    }

    /// PIN — a zero on any side is the whole texture and never a division.
    #[test]
    fn a_degenerate_size_asks_for_the_whole_texture_rather_than_dividing_by_zero() {
        for fit in [
            BackgroundFit::Stretch,
            BackgroundFit::Fill,
            BackgroundFit::Tile,
        ] {
            for (w, h, iw, ih) in [(0, 100, 10, 10), (100, 0, 10, 10), (100, 100, 0, 10)] {
                let uv = background_uv_rect(fit, w, h, iw, ih);
                assert_eq!(uv, [0.0, 0.0, 1.0, 1.0], "{fit:?} {w}x{h} / {iw}x{ih}");
                assert!(uv.iter().all(|value| value.is_finite()));
            }
        }
    }

    /// PIN — the clear is premultiplied, and an opaque ground is bit-identical
    /// to what the renderer wrote before this module existed.
    ///
    /// The second half is the one with teeth: every acceptance shot in §2.3 A2
    /// was taken at zero pixel difference, and an alpha that touched the opaque
    /// case would invalidate all of them silently.
    #[test]
    fn the_clear_is_premultiplied_and_an_opaque_ground_is_unchanged() {
        let linear = [0.25, 0.5, 1.0];
        let opaque = premultiplied_clear(linear, 1.0);
        assert_eq!(
            (opaque.r, opaque.g, opaque.b, opaque.a),
            (0.25, 0.5, 1.0, 1.0)
        );

        let half = premultiplied_clear(linear, 0.5);
        assert!((half.r - 0.125).abs() < 1e-12);
        assert!((half.g - 0.25).abs() < 1e-12);
        assert!((half.b - 0.5).abs() < 1e-12);
        assert!((half.a - 0.5).abs() < 1e-12);

        // Fully transparent leaves nothing behind — a straight-alpha clear would
        // leave the colour at full strength here and the window would glow.
        let none = premultiplied_clear(linear, 0.0);
        assert_eq!((none.r, none.g, none.b, none.a), (0.0, 0.0, 0.0, 0.0));
    }

    /// PIN — a ground with no picture is the one the clear alone draws.
    #[test]
    fn a_ground_with_no_picture_needs_no_quad() {
        assert!(WindowGround::opaque().is_plain());
        assert!(
            WindowGround {
                alpha: 0.4,
                ..WindowGround::opaque()
            }
            .is_plain(),
            "translucency is the clear's alpha and costs no second pipeline"
        );
        assert!(
            !WindowGround {
                image: Some(Arc::new(BackgroundImage {
                    key: "bg:1".to_owned(),
                    rgba: Arc::from(vec![0u8; 4]),
                    width_px: 1,
                    height_px: 1,
                })),
                ..WindowGround::opaque()
            }
            .is_plain()
        );
    }
}
// The clamping and revision contract of `set_window_ground` is pinned in
// `lib.rs`'s test module rather than here: the ground is process-wide, so a test
// that moves it has to take the same `THEME_TEST_LOCK` every other test that
// moves process colour already takes.
