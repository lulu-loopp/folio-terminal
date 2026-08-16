//! Folio Windows-landing probe #2: taskbar progress via ITaskbarList3.
//!
//! Creates a real top-level window (so it gets a taskbar button), waits for the
//! `TaskbarButtonCreated` registered message -- which is the ONLY moment the shell guarantees
//! the button exists and ITaskbarList3 calls will stick -- then walks the whole progress
//! vocabulary on a timer so a capture loop can photograph each state:
//!
//!   t=0s   window up, no progress
//!   t=3s   TBPF_NORMAL 40%
//!   t=6s   TBPF_INDETERMINATE
//!   t=9s   TBPF_ERROR 70%
//!   t=12s  TBPF_PAUSED 55%
//!   t=15s  TBPF_NORMAL 40% + SetOverlayIcon (the "attention" affordance)
//!   t=18s  SetOverlayIcon(none) + TBPF_NOPROGRESS
//!   t=21s  quit
//!
//! Prints the HWND and each transition with a timestamp so frames can be correlated.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Shell::{
    ITaskbarList3, TBPF_ERROR, TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL, TBPF_PAUSED,
    TaskbarList,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW,
    GetMessageW, HICON, IDI_ERROR, IDI_WARNING, LoadIconW, MSG, PostQuitMessage,
    RegisterClassExW, RegisterWindowMessageW, SW_SHOWNORMAL, SetTimer, ShowWindow, WM_DESTROY,
    WM_TIMER, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};
use windows::core::{PCWSTR, w};

static TASKBAR_BUTTON_CREATED: OnceLock<u32> = OnceLock::new();
static TASKBAR: OnceLock<TaskbarHolder> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();
static STEP: AtomicUsize = AtomicUsize::new(0);

/// ITaskbarList3 is apartment-threaded and everything here happens on the one UI thread that
/// created it, so parking it in a static is sound in this probe.
struct TaskbarHolder(ITaskbarList3);
unsafe impl Send for TaskbarHolder {}
unsafe impl Sync for TaskbarHolder {}

fn stamp() -> String {
    let t = START.get().map(|s| s.elapsed().as_secs_f32()).unwrap_or(0.0);
    format!("t={t:5.1}s")
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        // The shell tells every top-level window when its taskbar button is ready. Calling
        // ITaskbarList3 before this arrives is the classic "progress silently does nothing"
        // bug -- and it also re-fires if explorer.exe restarts, which is why the state has to
        // be re-applied here rather than set once at startup.
        if Some(&msg) == TASKBAR_BUTTON_CREATED.get() {
            println!("{} WM_TASKBARBUTTONCREATED (msg={msg}) -- button exists now", stamp());
            if let Some(tb) = TASKBAR.get() {
                let _ = tb.0.HrInit();
                println!("{} HrInit() after button-created", stamp());
            }
            return LRESULT(0);
        }
        match msg {
            WM_TIMER => {
                // A single repeating timer drives a step counter. The first draft armed seven
                // timers with different periods and SetTimer repeats, so by t=18s four of them
                // fired in the same second and the last one won -- every state after the first
                // was overwritten before it could be photographed.
                let step = STEP.fetch_add(1, Ordering::Relaxed) + 1;
                let _ = w;
                let tb = TASKBAR.get().map(|h| &h.0);
                let Some(tb) = tb else { return LRESULT(0) };
                match step {
                    1 => {
                        let r = tb.SetProgressState(hwnd, TBPF_NORMAL);
                        let v = tb.SetProgressValue(hwnd, 40, 100);
                        println!("{} TBPF_NORMAL 40/100 state={r:?} value={v:?}", stamp());
                    }
                    2 => {
                        let r = tb.SetProgressState(hwnd, TBPF_INDETERMINATE);
                        println!("{} TBPF_INDETERMINATE state={r:?}", stamp());
                    }
                    3 => {
                        let r = tb.SetProgressState(hwnd, TBPF_ERROR);
                        let v = tb.SetProgressValue(hwnd, 70, 100);
                        println!("{} TBPF_ERROR 70/100 state={r:?} value={v:?}", stamp());
                    }
                    4 => {
                        let r = tb.SetProgressState(hwnd, TBPF_PAUSED);
                        let v = tb.SetProgressValue(hwnd, 55, 100);
                        println!("{} TBPF_PAUSED 55/100 state={r:?} value={v:?}", stamp());
                    }
                    5 => {
                        let _ = tb.SetProgressState(hwnd, TBPF_NORMAL);
                        let _ = tb.SetProgressValue(hwnd, 40, 100);
                        let icon: HICON = LoadIconW(None, IDI_WARNING).unwrap_or_default();
                        let r = tb.SetOverlayIcon(hwnd, icon, w!("needs attention"));
                        println!("{} TBPF_NORMAL 40 + SetOverlayIcon(IDI_WARNING) -> {r:?}", stamp());
                    }
                    6 => {
                        let o = tb.SetOverlayIcon(hwnd, HICON::default(), PCWSTR::null());
                        let r = tb.SetProgressState(hwnd, TBPF_NOPROGRESS);
                        println!("{} overlay cleared={o:?} TBPF_NOPROGRESS={r:?}", stamp());
                    }
                    7 => {
                        println!("{} quitting", stamp());
                        PostQuitMessage(0);
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, w, l),
        }
    }
}

fn main() -> windows::core::Result<()> {
    let _ = START.set(Instant::now());
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;

        // Must be registered before the window is created so the wndproc can recognise it.
        let m = RegisterWindowMessageW(w!("TaskbarButtonCreated"));
        let _ = TASKBAR_BUTTON_CREATED.set(m);
        println!("{} RegisterWindowMessageW(TaskbarButtonCreated) = {m}", stamp());

        let taskbar: ITaskbarList3 = CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)?;
        println!("{} CoCreateInstance(TaskbarList) -> ITaskbarList3 ok", stamp());
        let _ = TASKBAR.set(TaskbarHolder(taskbar.clone()));

        let instance = GetModuleHandleW(None)?;
        let class = w!("FolioTaskbarProbe");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class,
            hIcon: LoadIconW(None, IDI_ERROR).unwrap_or_default(),
            ..Default::default()
        };
        let atom = RegisterClassExW(&wc);
        assert!(atom != 0, "RegisterClassExW failed");

        let hwnd = CreateWindowExW(
            Default::default(),
            class,
            w!("Folio taskbar progress probe"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            900,
            520,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        println!("HWND={}", hwnd.0 as isize);
        println!("PID={}", std::process::id());
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);

        // HrInit before the button exists is the wrong order; we call it again on
        // TaskbarButtonCreated. Calling it here too shows whether it is even needed.
        let early = taskbar.HrInit();
        println!("{} early HrInit() = {early:?}", stamp());

        SetTimer(Some(hwnd), 1, 3000, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
