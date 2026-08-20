//! The visual tree, in the shape the product already ships.
//!
//! ```text
//! target  ← CreateTargetForHwnd(hwnd, topmost = true)
//!  └─ root
//!      ├─ web   ← ICoreWebView2CompositionController::SetRootVisualTarget
//!      └─ gpu   ← the wgpu swapchain, PreMultiplied, added ABOVE web
//! ```
//!
//! `bt_platform::Compositor` builds `root` + `gpu` and reserves the second child
//! for the web preview; this probe is that second child, built the same way, so
//! that anything gate 1 measures here is a statement about the product's tree
//! and not about a look-alike.

use anyhow::{Context as _, Result};
use std::ffi::c_void;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice3, IDCompositionDesktopDevice, IDCompositionDevice3,
    IDCompositionRectangleClip, IDCompositionTarget, IDCompositionVisual, IDCompositionVisual3,
};
use windows::core::{IUnknown, Interface as _};

pub struct Tree {
    device: IDCompositionDesktopDevice,
    device3: IDCompositionDevice3,
    _target: IDCompositionTarget,
    _root: IDCompositionVisual,
    web: IDCompositionVisual,
    gpu: IDCompositionVisual,
}

impl Tree {
    pub fn new(hwnd: HWND) -> Result<Self> {
        unsafe {
            let device: IDCompositionDesktopDevice = DCompositionCreateDevice3(None::<&IUnknown>)
                .context("DCompositionCreateDevice3")?;
            let device3: IDCompositionDevice3 =
                device.cast().context("cast IDCompositionDevice3")?;
            let target = device
                .CreateTargetForHwnd(hwnd, true)
                .context("CreateTargetForHwnd")?;
            let root = make_visual(&device)?;
            let web = make_visual(&device)?;
            let gpu = make_visual(&device)?;
            // **Order is the whole point, and the argument that sets it is not
            // the one it reads like.** `AddVisual(visual, insertAbove, NULL)`
            // with `insertAbove = TRUE` puts the visual at the *beginning* of
            // the child list, and the beginning of a DirectComposition child
            // list is the BOTTOM. Building the tree with `true` for both
            // children therefore renders the wgpu overlay *under* the web page:
            // the first run of this probe photographed exactly that, with a
            // floating panel sliced off at the seat's left edge and looking for
            // all the world like an airspace failure it was not.
            //
            // Appending both with `false` gives `[web, gpu]`, and the last child
            // is the one on top.
            root.AddVisual(&web, false, None::<&IDCompositionVisual>)
                .context("AddVisual(web)")?;
            root.AddVisual(&gpu, false, None::<&IDCompositionVisual>)
                .context("AddVisual(gpu)")?;
            target.SetRoot(&root).context("SetRoot")?;
            let tree = Self {
                device,
                device3,
                _target: target,
                _root: root,
                web,
                gpu,
            };
            tree.commit()?;
            Ok(tree)
        }
    }

    pub fn gpu_visual_ptr(&self) -> *mut c_void {
        self.gpu.as_raw()
    }

    /// The `IUnknown` WebView2 is handed as its root visual target.
    pub fn web_visual(&self) -> IUnknown {
        self.web.cast().expect("IDCompositionVisual is an IUnknown")
    }

    /// Physical pixels, always. `BoundsMode = USE_RAW_PIXELS` on the WebView2
    /// side means neither end multiplies by a scale factor.
    pub fn set_web_offset(&self, x: f32, y: f32) -> Result<()> {
        unsafe {
            self.web.SetOffsetX2(x).context("SetOffsetX")?;
            self.web.SetOffsetY2(y).context("SetOffsetY")?;
        }
        Ok(())
    }

    /// A square clip on the web visual, in the visual's own coordinates.
    pub fn set_web_clip(&self, width: f32, height: f32) -> Result<()> {
        unsafe {
            let clip = self
                .device3
                .CreateRectangleClip()
                .context("CreateRectangleClip")?;
            clip.SetLeft2(0.0).context("SetLeft")?;
            clip.SetTop2(0.0).context("SetTop")?;
            clip.SetRight2(width).context("SetRight")?;
            clip.SetBottom2(height).context("SetBottom")?;
            self.web.SetClip(&clip).context("SetClip")?;
        }
        Ok(())
    }

    /// The same clip with corner radii — the question gate 1 asks as "圆角遮罩".
    pub fn set_web_rounded_clip(&self, width: f32, height: f32, radius: f32) -> Result<()> {
        unsafe {
            let clip: IDCompositionRectangleClip = self
                .device3
                .CreateRectangleClip()
                .context("CreateRectangleClip")?;
            clip.SetLeft2(0.0).context("SetLeft")?;
            clip.SetTop2(0.0).context("SetTop")?;
            clip.SetRight2(width).context("SetRight")?;
            clip.SetBottom2(height).context("SetBottom")?;
            clip.SetTopLeftRadiusX2(radius)
                .context("SetTopLeftRadiusX")?;
            clip.SetTopLeftRadiusY2(radius)
                .context("SetTopLeftRadiusY")?;
            clip.SetTopRightRadiusX2(radius)
                .context("SetTopRightRadiusX")?;
            clip.SetTopRightRadiusY2(radius)
                .context("SetTopRightRadiusY")?;
            clip.SetBottomLeftRadiusX2(radius)
                .context("SetBottomLeftRadiusX")?;
            clip.SetBottomLeftRadiusY2(radius)
                .context("SetBottomLeftRadiusY")?;
            clip.SetBottomRightRadiusX2(radius)
                .context("SetBottomRightRadiusX")?;
            clip.SetBottomRightRadiusY2(radius)
                .context("SetBottomRightRadiusY")?;
            self.web.SetClip(&clip).context("SetClip")?;
        }
        Ok(())
    }

    pub fn clear_web_clip(&self) -> Result<()> {
        unsafe {
            self.web
                .SetClip(None::<&windows::Win32::Graphics::DirectComposition::IDCompositionClip>)
                .context("SetClip(None)")
        }
    }

    pub fn set_web_opacity(&self, opacity: f32) -> Result<()> {
        let visual: IDCompositionVisual3 = self.web.cast().context("cast IDCompositionVisual3")?;
        unsafe { visual.SetOpacity2(opacity).context("SetOpacity") }
    }

    /// Publish everything set since the last call. wgpu calls `SetContent` on
    /// our visual and never commits (`wgpu-hal-30.0.0/src/dx12/mod.rs:1619`), so
    /// this is the only place a frame becomes visible.
    pub fn commit(&self) -> Result<()> {
        unsafe { self.device.Commit().context("IDCompositionDevice2::Commit") }
    }
}

fn make_visual(device: &IDCompositionDesktopDevice) -> Result<IDCompositionVisual> {
    unsafe {
        device
            .CreateVisual()
            .context("CreateVisual")?
            .cast()
            .context("cast IDCompositionVisual")
    }
}
