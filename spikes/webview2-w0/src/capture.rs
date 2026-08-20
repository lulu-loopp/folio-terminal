//! The two OS oracles, and the ruler laid across what they return.
//!
//! §1 of the plan says the three pixel channels are three different contracts.
//! This module implements the two that produce *pixels a host can read*:
//! Windows.Graphics.Capture of the whole window, and `PrintWindow`. WebView2's
//! own `CapturePreview` lives on [`crate::host::Host`] because it is the engine's
//! call, not the OS's — and telling those apart is most of what gate 2 is for.

use anyhow::{Context as _, Result};
use std::path::Path;
use std::time::{Duration, Instant};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice,
    ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::core::Interface as _;

/// A window's pixels, BGRA, top row first.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    pub fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let index = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[index],
            self.pixels[index + 1],
            self.pixels[index + 2],
            self.pixels[index + 3],
        ]
    }

    /// Blue, green, red — the order the buffer is in, kept explicit so a reader
    /// of a comparison never has to guess which end is which.
    pub fn bgr(&self, x: u32, y: u32) -> [u8; 3] {
        let pixel = self.at(x, y);
        [pixel[0], pixel[1], pixel[2]]
    }

    pub fn save_png(&self, path: &Path) -> Result<()> {
        let mut rgba = Vec::with_capacity(self.pixels.len());
        for chunk in self.pixels.chunks_exact(4) {
            rgba.extend_from_slice(&[chunk[2], chunk[1], chunk[0], 255]);
        }
        let buffer =
            image::RgbaImage::from_raw(self.width, self.height, rgba).context("build image")?;
        buffer
            .save(path)
            .with_context(|| format!("save {path:?}"))?;
        Ok(())
    }

    /// How many pixels differ, and by how much on the worst channel. The
    /// product's own DComp acceptance used exactly this pair.
    pub fn difference(&self, other: &Self) -> (u64, u8) {
        if self.width != other.width || self.height != other.height {
            return (u64::from(self.width) * u64::from(self.height), 255);
        }
        let mut differing = 0;
        let mut worst = 0;
        for (left, right) in self
            .pixels
            .chunks_exact(4)
            .zip(other.pixels.chunks_exact(4))
        {
            let mut different = false;
            for channel in 0..3 {
                let delta = left[channel].abs_diff(right[channel]);
                if delta > 0 {
                    different = true;
                    worst = worst.max(delta);
                }
            }
            if different {
                differing += 1;
            }
        }
        (differing, worst)
    }

    /// The rectangle every pixel near `colour` fits inside, if there are any.
    ///
    /// Gates locate things by colour rather than by arithmetic on window
    /// coordinates, because a capture of a whole window includes non-client
    /// chrome whose size is the shell's business and not this probe's.
    pub fn bounding_box(&self, colour: [u8; 3], tolerance: u8) -> Option<(u32, u32, u32, u32)> {
        let mut bounds: Option<(u32, u32, u32, u32)> = None;
        for y in 0..self.height {
            for x in 0..self.width {
                if near(self.bgr(x, y), colour, tolerance) {
                    bounds = Some(match bounds {
                        None => (x, y, x, y),
                        Some((left, top, right, bottom)) => {
                            (left.min(x), top.min(y), right.max(x), bottom.max(y))
                        }
                    });
                }
            }
        }
        bounds
    }

    /// The commonest colour in a region, quantised to 4 levels per channel so
    /// that anti-aliasing and text do not each count as their own colour.
    ///
    /// This is how the gates learn what a colour *is* instead of assuming it:
    /// the host's own rectangles come back from the capture with the exact bytes
    /// it drew, but a web page's do not, so the page's landmarks have to be
    /// read off the screen before they can be looked for.
    pub fn modal_colour(&self, left: u32, top: u32, right: u32, bottom: u32) -> Option<[u8; 3]> {
        let mut buckets: std::collections::HashMap<[u8; 3], (u64, [u64; 3])> =
            std::collections::HashMap::new();
        for y in top..=bottom.min(self.height.saturating_sub(1)) {
            for x in left..=right.min(self.width.saturating_sub(1)) {
                let pixel = self.bgr(x, y);
                let key = [pixel[0] & 0xc0, pixel[1] & 0xc0, pixel[2] & 0xc0];
                let entry = buckets.entry(key).or_insert((0, [0; 3]));
                entry.0 += 1;
                for (sum, &value) in entry.1.iter_mut().zip(pixel.iter()) {
                    *sum += u64::from(value);
                }
            }
        }
        let (_, (count, sums)) = buckets.into_iter().max_by_key(|(_, (count, _))| *count)?;
        Some([
            (sums[0] / count) as u8,
            (sums[1] / count) as u8,
            (sums[2] / count) as u8,
        ])
    }

    /// How many pixels are within `tolerance` of `colour`.
    pub fn count_near(&self, colour: [u8; 3], tolerance: u8) -> u64 {
        self.pixels
            .chunks_exact(4)
            .filter(|pixel| near([pixel[0], pixel[1], pixel[2]], colour, tolerance))
            .count() as u64
    }
}

pub fn near(pixel: [u8; 3], colour: [u8; 3], tolerance: u8) -> bool {
    pixel[0].abs_diff(colour[0]) <= tolerance
        && pixel[1].abs_diff(colour[1]) <= tolerance
        && pixel[2].abs_diff(colour[2]) <= tolerance
}

/// What one scan line found where the host's border meets the web page.
#[derive(Debug, serde::Serialize)]
pub struct Seam {
    pub row: u32,
    pub border_start: Option<u32>,
    pub border_end: Option<u32>,
    /// How many pixels of bare window background follow the border's last
    /// pixel. Zero is the only passing answer: anything else is the web
    /// rectangle lagging the layout and letting the class brush show through —
    /// the tear child-window hosting produced in 3 frames out of 14.
    ///
    /// The test is stated against the *hole* colour rather than the page's,
    /// because the page's first pixels are its own border ring and a scan that
    /// insisted on the background colour would count the page's own design as a
    /// gap.
    pub gap: Option<u32>,
    pub gap_colour: Option<[u8; 3]>,
}

/// Walk one row left to right: window background, the host's border, then
/// whatever the seat contains.
pub fn seam_scan(image: &Image, row: u32, border: [u8; 3], hole: [u8; 3], tolerance: u8) -> Seam {
    let mut border_start = None;
    let mut border_end = None;
    for x in 0..image.width {
        if near(image.bgr(x, row), border, tolerance) {
            if border_start.is_none() {
                border_start = Some(x);
            }
            border_end = Some(x);
        } else if border_start.is_some() {
            break;
        }
    }
    let Some(end) = border_end else {
        return Seam {
            row,
            border_start,
            border_end,
            gap: None,
            gap_colour: None,
        };
    };
    let mut gap = 0;
    let mut colour = None;
    let mut x = end + 1;
    while x < image.width && near(image.bgr(x, row), hole, tolerance) {
        if colour.is_none() {
            colour = Some(image.bgr(x, row));
        }
        gap += 1;
        x += 1;
        if gap > 200 {
            break;
        }
    }
    Seam {
        row,
        border_start,
        border_end,
        gap: Some(gap),
        gap_colour: colour,
    }
}

// ── Windows.Graphics.Capture ───────────────────────────────────────────────

/// A capture session held open across many frames, because tearing one down and
/// building it up again per frame costs more than the frame does and would make
/// the timing numbers meaningless.
pub struct WindowCapture {
    _device: ID3D11Device,
    context: ID3D11DeviceContext,
    _item: GraphicsCaptureItem,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
}

impl WindowCapture {
    pub fn start(hwnd: HWND) -> Result<Self> {
        unsafe {
            let mut device: Option<ID3D11Device> = None;
            let mut context: Option<ID3D11DeviceContext> = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                windows::Win32::Foundation::HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                windows::Win32::Graphics::Direct3D11::D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .context("D3D11CreateDevice")?;
            let device = device.context("D3D11CreateDevice returned no device")?;
            let context = context.context("D3D11CreateDevice returned no context")?;
            let dxgi: IDXGIDevice = device.cast().context("IDXGIDevice")?;
            let inspectable = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)
                .context("CreateDirect3D11DeviceFromDXGIDevice")?;
            let d3d: IDirect3DDevice = inspectable.cast().context("IDirect3DDevice")?;

            let interop: IGraphicsCaptureItemInterop =
                windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                    .context("IGraphicsCaptureItemInterop")?;
            let item: GraphicsCaptureItem =
                interop.CreateForWindow(hwnd).context("CreateForWindow")?;
            let size = item.Size().context("item size")?;
            let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
                &d3d,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                2,
                size,
            )
            .context("CreateFreeThreaded")?;
            let session = pool
                .CreateCaptureSession(&item)
                .context("CreateCaptureSession")?;
            let _ = session.SetIsCursorCaptureEnabled(false);
            // The yellow capture border Windows draws around a captured window
            // is the OS telling the user a capture is running. It is outside the
            // client area, and every measurement below finds its landmark by
            // colour rather than by coordinate, so it changes no reading.
            // A frame pool with no arrived handler still fills; the handler is
            // only here so the free-threaded pool has somewhere to signal.
            let _ = pool.FrameArrived(&TypedEventHandler::<
                Direct3D11CaptureFramePool,
                windows::core::IInspectable,
            >::new(|_, _| Ok(())));
            session.StartCapture().context("StartCapture")?;
            Ok(Self {
                _device: device,
                context,
                _item: item,
                pool,
                session,
            })
        }
    }

    /// Throw away every frame already sitting in the pool.
    ///
    /// The pool buffers two frames, and both can predate the change being
    /// photographed. Without this, a screenshot taken right after a present
    /// shows the *previous* state — which is how the first run of this probe
    /// recorded a translucent panel as opaque.
    pub fn discard_queued(&self) -> usize {
        let mut dropped = 0;
        while dropped < 16 {
            match self.pool.TryGetNextFrame() {
                Ok(frame) => {
                    drop(frame);
                    dropped += 1;
                }
                Err(_) => break,
            }
        }
        dropped
    }

    /// Pull the next frame, waiting up to `timeout` for one to arrive.
    pub fn frame(&self, timeout: Duration) -> Result<Image> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(frame) = self.pool.TryGetNextFrame() {
                let surface = frame.Surface().context("frame surface")?;
                let access: IDirect3DDxgiInterfaceAccess =
                    surface.cast().context("IDirect3DDxgiInterfaceAccess")?;
                let texture: ID3D11Texture2D =
                    unsafe { access.GetInterface() }.context("GetInterface")?;
                let image = self.read_back(&texture)?;
                drop(frame);
                return image_or_error(image);
            }
            if Instant::now() >= deadline {
                anyhow::bail!("no capture frame within {timeout:?}");
            }
            crate::win::pump_for(Duration::from_millis(8), |_| {});
        }
    }

    fn read_back(&self, texture: &ID3D11Texture2D) -> Result<Image> {
        unsafe {
            let mut description = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut description);
            let staging_description = D3D11_TEXTURE2D_DESC {
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
                ..description
            };
            let mut staging: Option<ID3D11Texture2D> = None;
            self._device
                .CreateTexture2D(&staging_description, None, Some(&mut staging))
                .context("CreateTexture2D(staging)")?;
            let staging = staging.context("staging texture")?;
            self.context.CopyResource(&staging, texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .context("Map")?;
            let width = description.Width;
            let height = description.Height;
            let mut pixels = vec![0u8; (width * height * 4) as usize];
            for y in 0..height {
                let source = (mapped.pData as *const u8).add((y * mapped.RowPitch) as usize);
                let destination = pixels.as_mut_ptr().add((y * width * 4) as usize);
                std::ptr::copy_nonoverlapping(source, destination, (width * 4) as usize);
            }
            self.context.Unmap(&staging, 0);
            Ok(Image {
                width,
                height,
                pixels,
            })
        }
    }
}

impl Drop for WindowCapture {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

fn image_or_error(image: Image) -> Result<Image> {
    if image.width == 0 || image.height == 0 {
        anyhow::bail!("capture returned an empty frame");
    }
    Ok(image)
}

// ── PrintWindow ────────────────────────────────────────────────────────────

// `PrintWindow` has no binding in `windows` 0.62 — only its flags do — so the
// one import it needs is declared here rather than pulling in a second Win32
// binding crate for one function.
#[link(name = "user32")]
unsafe extern "system" {
    fn PrintWindow(
        hwnd: HWND,
        hdc_blt: windows::Win32::Graphics::Gdi::HDC,
        flags: u32,
    ) -> windows::core::BOOL;
}

/// The cheap oracle, and the one with the most surprising answer for a window
/// whose pixels never touch GDI.
pub fn print_window(hwnd: HWND) -> Result<Image> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
        DeleteDC, DeleteObject, GetDC, HBITMAP, ReleaseDC, SelectObject,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, PW_RENDERFULLCONTENT};
    unsafe {
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect).context("GetClientRect")?;
        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Negative: a top-down DIB, so row 0 is the top row and no flip
                // is needed to line this up with the capture path.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let bitmap: HBITMAP =
            CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0)
                .context("CreateDIBSection")?;
        let previous = SelectObject(memory, bitmap.into());
        let ok = PrintWindow(hwnd, memory, PW_RENDERFULLCONTENT).as_bool();
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        if ok && !bits.is_null() {
            std::ptr::copy_nonoverlapping(bits.cast::<u8>(), pixels.as_mut_ptr(), pixels.len());
        }
        SelectObject(memory, previous);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        if !ok {
            anyhow::bail!("PrintWindow returned FALSE");
        }
        Ok(Image {
            width: width as u32,
            height: height as u32,
            pixels,
        })
    }
}

/// Read a PNG `CapturePreview` wrote, so gate 2 can compare the two oracles
/// pixel for pixel rather than by eye.
pub fn load_png(path: &Path) -> Result<Image> {
    let decoded = image::open(path)
        .with_context(|| format!("open {path:?}"))?
        .to_rgba8();
    let (width, height) = decoded.dimensions();
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for pixel in decoded.pixels() {
        pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}
