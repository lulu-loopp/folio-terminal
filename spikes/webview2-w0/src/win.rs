//! The host window: a plain Win32 window, its own message pump, and the two
//! things the pump has to be able to say — *which* window a keystroke was
//! delivered to, and what the mouse did before anyone forwarded it.
//!
//! There is deliberately no winit here. The question every input gate asks is
//! "did the message reach the host's HWND", and a pump that owns
//! `PeekMessageW` answers it directly instead of through a translation layer.

use anyhow::{Context as _, Result};
use std::cell::RefCell;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, MapVirtualKeyW, SendInput, SetFocus, VIRTUAL_KEY, VK_CONTROL, VK_MENU,
    VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW,
    GetClientRect, GetForegroundWindow, MSG, PM_REMOVE, PeekMessageW, PostQuitMessage,
    RegisterClassW, SW_SHOW, SW_SHOWNOACTIVATE, SetForegroundWindow, ShowWindow, TranslateMessage,
    WM_CHAR,
    WM_DESTROY, WM_DPICHANGED, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
    WNDCLASSW, WS_EX_NOREDIRECTIONBITMAP, WS_OVERLAPPEDWINDOW,
};
use windows::core::{PCWSTR, w};

/// One keyboard message, and — the load-bearing field — the window it was
/// delivered to. In composition hosting WebView2 owns a 0×0 child HWND on this
/// same thread, so the host's pump *sees* the web's keystrokes go by; only the
/// target tells the two apart.
#[derive(Clone, Copy, Debug)]
pub struct KeyMessage {
    pub hwnd: isize,
    pub message: u32,
    pub vk: u32,
}

impl KeyMessage {
    pub fn is_down(&self) -> bool {
        self.message == WM_KEYDOWN || self.message == WM_SYSKEYDOWN
    }
}

thread_local! {
    /// Messages the *window procedure* saw, which is a strictly smaller set than
    /// the pump sees: only what was dispatched to this class.
    static WNDPROC_KEYS: RefCell<Vec<KeyMessage>> = const { RefCell::new(Vec::new()) };
    static DPI_CHANGES: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
}

/// Whether this run is allowed to take the foreground.
///
/// Every input gate needs it and says so by leaving this alone. The one that
/// does not is the slice 6 measurement, which injects no key and no click and
/// therefore has no business pulling a window out from under whoever is using
/// the machine.
static TAKE_FOREGROUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn leave_the_foreground_alone() {
    TAKE_FOREGROUND.store(false, std::sync::atomic::Ordering::Relaxed);
}

pub fn clear_wndproc_keys() {
    WNDPROC_KEYS.with(|cell| cell.borrow_mut().clear());
}

pub fn dpi_changes() -> Vec<u32> {
    DPI_CHANGES.with(|cell| cell.borrow().clone())
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP | WM_CHAR => {
            WNDPROC_KEYS.with(|cell| {
                cell.borrow_mut().push(KeyMessage {
                    hwnd: hwnd.0 as isize,
                    message,
                    vk: wparam.0 as u32,
                });
            });
        }
        WM_DPICHANGED => {
            DPI_CHANGES.with(|cell| cell.borrow_mut().push((wparam.0 & 0xffff) as u32));
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

pub struct HostWindow {
    pub hwnd: HWND,
}

impl HostWindow {
    pub fn create(title: PCWSTR, width: i32, height: i32) -> Result<Self> {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance = GetModuleHandleW(None).context("GetModuleHandleW")?;
            let class = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                lpszClassName: w!("BtSpikeW0Host"),
                // A class background that is nobody's visual: wherever neither the
                // web visual nor the wgpu visual paints, *this* is what shows, and
                // seeing it is how the pixel gates tell "a hole" from "black paint".
                hbrBackground: HBRUSH(
                    CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x0020_1008)).0,
                ),
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                anyhow::bail!(
                    "RegisterClassW failed: {:?}",
                    windows::core::Error::from_thread()
                );
            }
            let hwnd = CreateWindowExW(
                // No redirection bitmap: the window's pixels come from the
                // DirectComposition tree, never from GDI's shadow surface.
                WS_EX_NOREDIRECTIONBITMAP,
                w!("BtSpikeW0Host"),
                title,
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                width,
                height,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .context("CreateWindowExW")?;
            if TAKE_FOREGROUND.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = SetForegroundWindow(hwnd);
                let _ = SetFocus(Some(hwnd));
            } else {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
            Ok(Self { hwnd })
        }
    }

    pub fn client_size(&self) -> (u32, u32) {
        let mut rect = RECT::default();
        let _ = unsafe { GetClientRect(self.hwnd, &mut rect) };
        (
            (rect.right - rect.left).max(1) as u32,
            (rect.bottom - rect.top).max(1) as u32,
        )
    }

    pub fn dpi(&self) -> u32 {
        unsafe { GetDpiForWindow(self.hwnd) }
    }

    pub fn is_foreground(&self) -> bool {
        unsafe { GetForegroundWindow() == self.hwnd }
    }

    pub fn focus_self(&self) {
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.hwnd));
        }
    }
}

/// Every keyboard message the pump removed from the queue this drain, with the
/// window each was addressed to.
#[derive(Default)]
pub struct PumpLog {
    pub keys: Vec<KeyMessage>,
    /// `WM_POINTER*`, which only ever arrives from the OS input stack — there
    /// is no way for this probe to post one to itself, so a row here is proof
    /// that the injected contact travelled the driver path.
    pub pointers: Vec<PointerMessage>,
    pub quit: bool,
}

/// One pointer message, with the contact it belongs to.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct PointerMessage {
    pub hwnd: isize,
    pub message: u32,
    pub pointer_id: u32,
}

/// Pump messages for `duration`, letting `before_dispatch` see each message
/// first. Returns everything the pump learned.
pub fn pump_for<F>(duration: Duration, mut before_dispatch: F) -> PumpLog
where
    F: FnMut(&MSG),
{
    let deadline = Instant::now() + duration;
    let mut log = PumpLog::default();
    loop {
        let mut msg = MSG::default();
        let mut drained = false;
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            drained = true;
            if msg.message == WM_QUIT {
                log.quit = true;
                return log;
            }
            if matches!(
                msg.message,
                WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP | WM_CHAR
            ) {
                log.keys.push(KeyMessage {
                    hwnd: msg.hwnd.0 as isize,
                    message: msg.message,
                    vk: msg.wParam.0 as u32,
                });
            }
            // WM_POINTERUPDATE .. WM_POINTERLEAVE, the whole family.
            if (0x0245..=0x024A).contains(&msg.message) {
                log.pointers.push(PointerMessage {
                    hwnd: msg.hwnd.0 as isize,
                    message: msg.message,
                    pointer_id: (msg.wParam.0 & 0xffff) as u32,
                });
            }
            before_dispatch(&msg);
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if Instant::now() >= deadline {
                break;
            }
        }
        if Instant::now() >= deadline {
            return log;
        }
        if !drained {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// Pump until `ready` says so or `timeout` expires. Returns whether it was
/// `ready` that ended the wait.
pub fn pump_until<R>(timeout: Duration, mut ready: R) -> bool
where
    R: FnMut() -> bool,
{
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if ready() {
            return true;
        }
        pump_for(Duration::from_millis(8), |_| {});
    }
    ready()
}

// ── Synthetic input ────────────────────────────────────────────────────────
//
// Every injection here goes through `SendInput` with a 40-byte x64 `INPUT`,
// which is the shape the 2026-08 ui-probe finding says must be honoured: a
// short struct makes `SendInput` reject the whole batch and report zero events
// while the caller reads it as "sent".

fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    let scan = unsafe {
        MapVirtualKeyW(
            vk.0 as u32,
            windows::Win32::UI::Input::KeyboardAndMouse::MAPVK_VK_TO_VSC,
        )
    } as u16;
    let extended = matches!(
        vk.0,
        // The arrows, Insert/Delete/Home/End/PageUp/PageDown and right-hand
        // modifiers are the extended set; without the flag they arrive as their
        // numeric-keypad twins.
        0x21..=0x28 | 0x2d | 0x2e | 0xa3 | 0xa5
    );
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Send one chord: modifiers down, key down, key up, modifiers up.
///
/// Returns the number of events `SendInput` actually accepted, so a caller can
/// tell "the key did nothing" from "the key was never sent".
pub fn send_chord(ctrl: bool, shift: bool, alt: bool, vk: u16) -> u32 {
    let mut batch = Vec::new();
    if ctrl {
        batch.push(key_input(VK_CONTROL, false));
    }
    if shift {
        batch.push(key_input(VK_SHIFT, false));
    }
    if alt {
        batch.push(key_input(VK_MENU, false));
    }
    batch.push(key_input(VIRTUAL_KEY(vk), false));
    batch.push(key_input(VIRTUAL_KEY(vk), true));
    if alt {
        batch.push(key_input(VK_MENU, true));
    }
    if shift {
        batch.push(key_input(VK_SHIFT, true));
    }
    if ctrl {
        batch.push(key_input(VK_CONTROL, true));
    }
    let sent = unsafe { SendInput(&batch, size_of::<INPUT>() as i32) };
    debug_assert_eq!(sent as usize, batch.len());
    sent
}

/// Hold one key down `count` times without releasing it — autorepeat, as the
/// keyboard driver would produce it.
///
/// **The presses are spaced, and that is not a detail.** Sent as one `SendInput`
/// batch the six presses arrive with one timestamp between them, and the engine
/// answers with a single callback for the release — which reads as "autorepeat
/// does not reach the host" and is really "that was not autorepeat". A real
/// driver repeats at the system rate, tens of milliseconds apart, so this waits
/// between presses and lets the caller pump.
pub fn send_autorepeat_spaced(vk: u16, count: u32, gap: Duration, mut pump: impl FnMut()) -> u32 {
    let mut sent = 0;
    for _ in 0..count {
        sent += unsafe {
            SendInput(
                &[key_input(VIRTUAL_KEY(vk), false)],
                size_of::<INPUT>() as i32,
            )
        };
        pump();
        std::thread::sleep(gap);
    }
    sent + unsafe {
        SendInput(
            &[key_input(VIRTUAL_KEY(vk), true)],
            size_of::<INPUT>() as i32,
        )
    }
}

/// The top-level window that owns the pixels at a screen point.
///
/// Injected touch goes to whoever owns those pixels, so this is the check that
/// stands between a measurement and touching a stranger's window.
pub fn root_window_from_point(screen: POINT) -> isize {
    use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, WindowFromPoint};
    unsafe {
        let hwnd = WindowFromPoint(screen);
        if hwnd.0.is_null() {
            return 0;
        }
        GetAncestor(hwnd, GA_ROOT).0 as isize
    }
}

/// Every child window of `hwnd`, with the class and rectangle of each.
///
/// This is the airspace question asked without touching the mouse: composition
/// hosting leaves exactly one 0×0 invisible child (the keyboard and IME sink),
/// while child-window hosting leaves three that fill the seat and take every
/// message the seat would have received.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ChildWindow {
    pub hwnd: String,
    pub class: String,
    pub rect: [i32; 4],
    pub visible: bool,
}

pub fn child_windows(parent: HWND) -> Vec<ChildWindow> {
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, GetClassNameW, GetWindowRect, IsWindowVisible,
    };

    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        unsafe {
            let found = &mut *(lparam.0 as *mut Vec<ChildWindow>);
            let mut class = [0u16; 128];
            let length = GetClassNameW(hwnd, &mut class) as usize;
            let mut rect = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rect);
            found.push(ChildWindow {
                hwnd: format!("{:#x}", hwnd.0 as isize),
                class: String::from_utf16_lossy(&class[..length]),
                rect: [rect.left, rect.top, rect.right, rect.bottom],
                visible: IsWindowVisible(hwnd).as_bool(),
            });
            windows::core::BOOL(1)
        }
    }

    let mut found: Vec<ChildWindow> = Vec::new();
    let _ = unsafe {
        EnumChildWindows(
            Some(parent),
            Some(visit),
            LPARAM(std::ptr::from_mut(&mut found) as isize),
        )
    };
    found
}

pub fn client_to_screen(hwnd: HWND, point: POINT) -> POINT {
    let mut out = point;
    let _ = unsafe { windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut out) };
    out
}
