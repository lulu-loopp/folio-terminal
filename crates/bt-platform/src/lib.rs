//! Audited native presentation bridges that are not exposed by winit/wgpu's safe APIs.

use std::num::NonZeroIsize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DxgiScaling {
    Stretch,
    None,
    AspectRatioStretch,
    Unknown(i32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DxgiPresentationState {
    pub scaling: DxgiScaling,
    pub background_rgb: [f32; 3],
    pub source_size: (u32, u32),
}

#[cfg(windows)]
mod windows_impl {
    use std::{ffi::c_void, sync::OnceLock};

    use windows::Win32::{
        Foundation::{COLORREF, GetLastError, HWND, SetLastError, WIN32_ERROR},
        Graphics::{
            Dxgi::{
                DXGI_RGBA, DXGI_SCALING_ASPECT_RATIO_STRETCH, DXGI_SCALING_NONE,
                DXGI_SCALING_STRETCH,
            },
            Gdi::{CreateSolidBrush, DeleteObject, HGDIOBJ},
        },
        UI::WindowsAndMessaging::{GCLP_HBRBACKGROUND, SetClassLongPtrW},
    };

    use super::{DxgiPresentationState, DxgiScaling, NonZeroIsize};

    static WINDOW_CLASS_BACKGROUND: OnceLock<Result<(), String>> = OnceLock::new();
    static BACKGROUND_COLOR_FAILURE_REPORTED: OnceLock<()> = OnceLock::new();

    pub fn install_window_class_background(hwnd: NonZeroIsize, rgb: [u8; 3]) -> Result<(), String> {
        WINDOW_CLASS_BACKGROUND
            .get_or_init(|| install_window_class_background_once(hwnd, rgb))
            .clone()
    }

    fn install_window_class_background_once(
        hwnd: NonZeroIsize,
        [r, g, b]: [u8; 3],
    ) -> Result<(), String> {
        let color = COLORREF(u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16));
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle on the event-loop thread.
        // CreateSolidBrush returns an independent GDI brush. Once installed, the brush must remain
        // alive as long as winit's shared class; winit never unregisters that process class, so the
        // successful path intentionally transfers it to process lifetime. On failure we delete it.
        unsafe {
            let brush = CreateSolidBrush(color);
            if brush.is_invalid() {
                return Err(format!("CreateSolidBrush failed: {}", GetLastError().0));
            }

            SetLastError(WIN32_ERROR(0));
            let previous = SetClassLongPtrW(
                HWND(hwnd.get() as *mut c_void),
                GCLP_HBRBACKGROUND,
                brush.0 as isize,
            );
            let error = GetLastError();
            if previous == 0 && error.0 != 0 {
                let _ = DeleteObject(HGDIOBJ(brush.0));
                return Err(format!(
                    "SetClassLongPtrW(GCLP_HBRBACKGROUND) failed: {}",
                    error.0
                ));
            }
        }
        Ok(())
    }

    pub fn configure_dxgi_presentation(
        surface: &wgpu::Surface<'_>,
        [r, g, b]: [u8; 3],
        source_size: (u32, u32),
    ) -> Result<DxgiPresentationState, String> {
        let swap_chain = dxgi_swap_chain(surface)?;
        let color = DXGI_RGBA {
            r: f32::from(r) / 255.0,
            g: f32::from(g) / 255.0,
            b: f32::from(b) / 255.0,
            a: 1.0,
        };

        // SAFETY: the COM handle is a cloned reference obtained through wgpu-hal's read-only
        // surface guard. These calls only update presentation metadata; they neither destroy nor
        // resize buffers and are serialized on the renderer/event-loop thread.
        unsafe {
            if let Err(error) = swap_chain.SetBackgroundColor(&color)
                && BACKGROUND_COLOR_FAILURE_REPORTED.set(()).is_ok()
            {
                eprintln!(
                    "native presentation diagnostic: \
                     IDXGISwapChain1::SetBackgroundColor failed; continuing because \
                     DXGI_SCALING_STRETCH does not display it: {error}"
                );
            }
            swap_chain
                .SetSourceSize(source_size.0, source_size.1)
                .map_err(|error| format!("IDXGISwapChain2::SetSourceSize failed: {error}"))?;
            let desc = swap_chain
                .GetDesc1()
                .map_err(|error| format!("IDXGISwapChain1::GetDesc1 failed: {error}"))?;
            let observed = swap_chain
                .GetBackgroundColor()
                .map_err(|error| format!("IDXGISwapChain1::GetBackgroundColor failed: {error}"))?;
            let mut source_width = 0;
            let mut source_height = 0;
            swap_chain
                .GetSourceSize(&mut source_width, &mut source_height)
                .map_err(|error| format!("IDXGISwapChain2::GetSourceSize failed: {error}"))?;

            Ok(DxgiPresentationState {
                scaling: if desc.Scaling == DXGI_SCALING_STRETCH {
                    DxgiScaling::Stretch
                } else if desc.Scaling == DXGI_SCALING_NONE {
                    DxgiScaling::None
                } else if desc.Scaling == DXGI_SCALING_ASPECT_RATIO_STRETCH {
                    DxgiScaling::AspectRatioStretch
                } else {
                    DxgiScaling::Unknown(desc.Scaling.0)
                },
                background_rgb: [observed.r, observed.g, observed.b],
                source_size: (source_width, source_height),
            })
        }
    }

    pub fn set_dxgi_source_size(
        surface: &wgpu::Surface<'_>,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let swap_chain = dxgi_swap_chain(surface)?;
        // SAFETY: as above, this changes presentation metadata only and runs on the renderer
        // thread. The caller guarantees the source dimensions do not exceed the back buffers.
        unsafe { swap_chain.SetSourceSize(width, height) }
            .map_err(|error| format!("IDXGISwapChain2::SetSourceSize failed: {error}"))
    }

    fn dxgi_swap_chain(
        surface: &wgpu::Surface<'_>,
    ) -> Result<windows::Win32::Graphics::Dxgi::IDXGISwapChain3, String> {
        // SAFETY: the guard is used only long enough to clone the COM swapchain reference. The
        // clone may be dropped at any time and no wgpu-owned object is destroyed or aliased for
        // mutation. All calls are serialized with configure/acquire on the renderer thread.
        let hal_surface = unsafe { surface.as_hal::<wgpu::hal::api::Dx12>() }
            .ok_or_else(|| "wgpu surface is not backed by DX12".to_owned())?;
        hal_surface
            .swap_chain()
            .ok_or_else(|| "DX12 surface has no configured swapchain".to_owned())
    }
}

#[cfg(windows)]
pub use windows_impl::{
    configure_dxgi_presentation, install_window_class_background, set_dxgi_source_size,
};
