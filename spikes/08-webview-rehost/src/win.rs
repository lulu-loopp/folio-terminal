//! The Win32 the probe needs and `bt-platform` does not export: two real
//! top-level windows, a message pump, a screen grab and a look at whose child
//! the engine's own hidden input window is.

use std::path::Path;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    CreateSolidBrush, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HBRUSH, ReleaseDC,
    SRCCOPY, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    EnumChildWindows, GetClassNameW, GetWindowRect, HWND_TOPMOST, MSG, PM_REMOVE, PeekMessageW,
    RegisterClassW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    SetWindowPos, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};
use windows::core::{HSTRING, w};

/// Every probe process sets this, and this one has two windows and a page whose
/// rastering is the thing under measurement — a process left system-aware would
/// be measuring Windows' own bitmap stretch.
pub fn become_dpi_aware() {
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// A single-threaded apartment, which is what WebView2 requires of the thread
/// that makes an environment — `CreateCoreWebView2Environment` answers
/// `0x800401F0 CoInitialize has not been called` without it. winit does this for
/// the real window; a probe with its own pump has to do it itself.
pub fn enter_apartment() -> Result<(), String> {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if result.is_err() {
        return Err(format!("CoInitializeEx: {result:?}"));
    }
    Ok(())
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

pub fn register_class() -> Result<(), String> {
    let instance =
        unsafe { GetModuleHandleW(None) }.map_err(|e| format!("GetModuleHandleW: {e}"))?;
    let brush: HBRUSH = unsafe { CreateSolidBrush(COLORREF(0x0018_100C)) };
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        hInstance: instance.into(),
        hbrBackground: brush,
        lpszClassName: w!("SpikeWebviewRehost"),
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err(String::from("RegisterClassW refused"));
    }
    Ok(())
}

/// A visible, **never activated** window. The probe runs while somebody else is
/// working: it may sit on top for the few seconds it takes, and it may not take
/// the foreground.
pub fn create_window(title: &str, x: i32, y: i32, width: i32, height: i32) -> Result<HWND, String> {
    let instance =
        unsafe { GetModuleHandleW(None) }.map_err(|e| format!("GetModuleHandleW: {e}"))?;
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("SpikeWebviewRehost"),
            &HSTRING::from(title),
            WS_OVERLAPPEDWINDOW,
            x,
            y,
            width,
            height,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
    .map_err(|e| format!("CreateWindowExW: {e}"))?;
    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
    }
    .map_err(|e| format!("SetWindowPos: {e}"))?;
    Ok(hwnd)
}

pub fn close_window(hwnd: HWND) {
    let _ = unsafe { DestroyWindow(hwnd) };
}

pub fn dpi_of(hwnd: HWND) -> u32 {
    unsafe { GetDpiForWindow(hwnd) }
}

/// Put this window on top **without taking the foreground**, and give the
/// desktop compositor a moment to publish it.
///
/// Only for the instant a photograph is taken. What is being photographed is
/// what DWM composed, so a window with somebody else's window over it
/// photographs somebody else's window — measured the first time this ran, and
/// the reason this exists.
pub fn raise(hwnd: HWND) {
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
    };
    let until = std::time::Instant::now() + std::time::Duration::from_millis(220);
    while std::time::Instant::now() < until {
        pump();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// One monitor, in **physical** virtual-screen coordinates, with its effective
/// scale.
#[derive(Clone, Copy, Debug)]
pub struct Monitor {
    pub left: i32,
    pub top: i32,
    /// Printed into the log rather than read by the probe: the size is how a
    /// reader of the evidence tells which physical screen a row is.
    #[allow(dead_code, reason = "evidence, through Debug")]
    pub width: i32,
    #[allow(dead_code, reason = "evidence, through Debug")]
    pub height: i32,
    pub dpi: u32,
}

unsafe extern "system" fn each_monitor(
    monitor: windows::Win32::Graphics::Gdi::HMONITOR,
    _dc: windows::Win32::Graphics::Gdi::HDC,
    _clip: *mut RECT,
    lparam: LPARAM,
) -> windows::core::BOOL {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    let found = lparam.0 as *mut Vec<Monitor>;
    let mut info = MONITORINFO {
        cbSize: u32::try_from(size_of::<MONITORINFO>()).unwrap_or(40),
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        let mut dpi_x = 96u32;
        let mut dpi_y = 96u32;
        let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        unsafe { &mut *found }.push(Monitor {
            left: info.rcWork.left,
            top: info.rcWork.top,
            width: info.rcWork.right - info.rcWork.left,
            height: info.rcWork.bottom - info.rcWork.top,
            dpi: dpi_x,
        });
    }
    true.into()
}

/// Every monitor's work area and scale, so the probe can put its target window
/// somewhere the scale is **different** and ask who noticed.
pub fn monitors() -> Vec<Monitor> {
    use windows::Win32::Graphics::Gdi::EnumDisplayMonitors;
    let mut found: Vec<Monitor> = Vec::new();
    let _ = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(each_monitor),
            LPARAM(std::ptr::from_mut(&mut found) as isize),
        )
    };
    found
}

/// Drain the message queue once. Every WebView2 callback arrives on it.
pub fn pump() {
    let mut msg = MSG::default();
    while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// One child HWND of a window: what class it is and where it is.
///
/// The engine keeps a 0×0, invisible `Chrome_WidgetWin_0` under its parent even
/// in visual hosting — the 07 spike's most counter-intuitive finding, and the
/// reason keys and IME work without the host forwarding anything. Which window
/// that child hangs under **is** the answer to "did the keyboard move".
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Child {
    pub class: String,
    pub handle: isize,
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    let found = lparam.0 as *mut Vec<Child>;
    let mut name = [0u16; 128];
    let read = unsafe { GetClassNameW(hwnd, &mut name) };
    let class = String::from_utf16_lossy(&name[..read.max(0) as usize]);
    unsafe { &mut *found }.push(Child {
        class,
        handle: hwnd.0 as isize,
    });
    true.into()
}

pub fn children_of(hwnd: HWND) -> Vec<Child> {
    let mut found: Vec<Child> = Vec::new();
    let _ = unsafe {
        EnumChildWindows(
            Some(hwnd),
            Some(collect),
            LPARAM(std::ptr::from_mut(&mut found) as isize),
        )
    };
    found
}

/// Grab what is on screen where this window is, and write it out as a PNG.
///
/// From the **screen** and not from the window's own DC on purpose: the page is
/// composed by DWM out of a DirectComposition visual, and a window DC knows
/// nothing about it. What is being photographed is the finished composition,
/// which is the only thing that answers "is the page in this window".
pub fn screenshot(hwnd: HWND, path: &Path) -> Result<(), String> {
    raise(hwnd);
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.map_err(|e| format!("GetWindowRect: {e}"))?;
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return Err(String::from("the window has no area to photograph"));
    }
    let screen = unsafe { GetDC(None) };
    let memory = unsafe { CreateCompatibleDC(Some(screen)) };
    let bitmap = unsafe { CreateCompatibleBitmap(screen, width, height) };
    let previous = unsafe { SelectObject(memory, bitmap.into()) };
    let copied = unsafe {
        BitBlt(
            memory,
            0,
            0,
            width,
            height,
            Some(screen),
            rect.left,
            rect.top,
            SRCCOPY,
        )
    };
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).unwrap_or(40),
            biWidth: width,
            // Negative: top-down, which is the order an image encoder wants.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let rows = unsafe {
        GetDIBits(
            memory,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        SelectObject(memory, previous);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
    }
    copied.map_err(|e| format!("BitBlt: {e}"))?;
    if rows == 0 {
        return Err(String::from("GetDIBits returned no rows"));
    }
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 0xFF;
    }
    image::save_buffer(
        path,
        &pixels,
        width as u32,
        height as u32,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| format!("save {}: {e}", path.display()))
}

/// The window handle as `bt_platform` wants it.
pub fn handle(hwnd: HWND) -> std::num::NonZeroIsize {
    std::num::NonZeroIsize::new(hwnd.0 as isize).expect("a real window handle")
}

/// A window handle that is not a window: the one honest way to make
/// `put_ParentWindow` refuse on a live machine.
pub fn destroyed_handle() -> std::num::NonZeroIsize {
    let hwnd =
        create_window("spike scratch", -4000, -4000, 120, 90).expect("a scratch window to destroy");
    let handle = handle(hwnd);
    close_window(hwnd);
    pump();
    handle
}
