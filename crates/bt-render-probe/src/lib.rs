//! **The render probe: a tool's GPU submissions, outside the product library** (thread-door note
//! 2026-09-26, revision (l)4; ticket A2p, round R2 of T-A2-BARE-SITE-LINT).
//!
//! Two instruments, each the last half of work `bt-render` prepared:
//!
//! - [`HeadlessRenderProbe`] — the process's [`GpuContext`] and one [`WindowRenderer`] whose
//!   target is a texture: `bt-replay` and `bt-zoom-perf` time a frame through it;
//! - [`read_back`] — a frame's pixels, which the glyph gate, the pixel tests and the video probe
//!   take their evidence through.
//!
//! `bt_render::probe_surface` prepares, encodes and reads; it never submits and never presents.
//! The submit and the present are here, in a crate no product crate depends on, so the product
//! library holds no probe effect and A2e's lint finds none in it. This crate is a tool, and its
//! root carries the excluded form of revision (j)2.

#![allow(clippy::disallowed_methods)]

use bt_render::probe_surface::{
    ProbeWork, probe_frame_finish, probe_frame_work, queue, read_back_pixels, read_back_work,
};
use bt_render::{CellMetrics, GpuContext, RenderError, RenderProbeSample, WindowRenderer};
use bt_viewport::ViewportFrame;
use web_time::Instant;

/// **Read `window`'s last frame back**, as `[b, g, r, a]` rows of its width — an offscreen
/// target only (it panics on a window with a swapchain, which a swapchain cannot be read).
///
/// `bt-render` encodes the copy and maps the buffer ([`read_back_work`],
/// [`read_back_pixels`]); this submits the copy between them, the one statement that left
/// `WindowRenderer::read_back` when the probe's submissions became a tool's.
///
/// # Errors
///
/// [`RenderError::DeviceLost`] when the device is gone before the buffer is minted, after it,
/// or after the copy; a mapping that failed.
pub fn read_back(window: &WindowRenderer, gpu: &GpuContext) -> Result<Vec<[u8; 4]>, RenderError> {
    let (commands, back) = read_back_work(window, gpu)?;
    queue(gpu).submit([commands]);
    read_back_pixels(gpu, back)
}

/// Headless render-path instrumentation used by `bt-replay`.
///
/// Two layers and nothing else: the process's [`GpuContext`], and one
/// [`WindowRenderer`] whose target is a texture instead of a swapchain. The
/// multiwindow spike called this shape the template for the split — "the device
/// layer without a surface" — and folding it back onto the real types is what
/// keeps the claim honest: the probe measures the production shaping, atlas and
/// encoding paths because it *is* a window renderer, not because a second copy
/// of one was kept in step by hand.
pub struct HeadlessRenderProbe {
    gpu: GpuContext,
    window: WindowRenderer,
    adapter_name: String,
}

impl HeadlessRenderProbe {
    /// A device without a surface, and one offscreen window on it.
    ///
    /// # Errors
    ///
    /// No adapter, no device, or no window on it.
    pub async fn new(width: u32, height: u32, scale_factor: f64) -> Result<Self, RenderError> {
        // Named outright rather than asked of a swapchain, which is the whole of
        // what "headless" means here. It is the format the window path picks on
        // every machine this ships to, so the atlas and pipelines the probe
        // measures are the ones a window would have built.
        let format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let mut gpu = GpuContext::headless(format).await?;
        let adapter_name = gpu.adapter_name();
        let window = WindowRenderer::offscreen(&mut gpu, width, height, scale_factor, format)?;
        Ok(Self {
            gpu,
            window,
            adapter_name,
        })
    }

    #[must_use]
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    #[must_use]
    pub fn max_texture_dimension_2d(&self) -> u32 {
        self.gpu.max_texture_dimension_2d()
    }

    /// The window's cell, for building a frame at its scale.
    #[must_use]
    pub fn base_metrics(&self) -> CellMetrics {
        self.window.base_metrics()
    }

    /// Re-measure the window at `scale_factor`.
    ///
    /// # Errors
    ///
    /// The window's own re-measure failure.
    pub fn update_scale_factor(&mut self, scale_factor: f64) -> Result<CellMetrics, RenderError> {
        self.window.update_scale_factor(&mut self.gpu, scale_factor)
    }

    /// **Draw one frame's grid text and report what the CPU spent doing it.**
    ///
    /// `bt-render` prepares and encodes it ([`probe_frame_work`]); this submits the commands,
    /// takes the instant the encode-and-submit span ends at, presents a swapchain frame if the
    /// window had one, and lets `bt-render` close the frame ([`probe_frame_finish`]) — the
    /// statements, in the order, `WindowRenderer::probe_frame` ran them.
    ///
    /// # Errors
    ///
    /// The frame's shape, the glyph prepares, or the acquire.
    pub fn prepare_frame(
        &mut self,
        frame: &ViewportFrame,
    ) -> Result<RenderProbeSample, RenderError> {
        let ProbeWork {
            commands,
            swapchain,
            timings,
        } = probe_frame_work(&mut self.window, &mut self.gpu, frame)?;
        queue(&self.gpu).submit([commands]);
        let submitted_at = Instant::now();
        if let Some(texture) = swapchain {
            queue(&self.gpu).present(texture);
        }
        Ok(probe_frame_finish(
            &mut self.window,
            &mut self.gpu,
            timings,
            submitted_at,
        ))
    }
}
