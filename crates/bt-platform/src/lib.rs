//! Audited native bridges that are not exposed by winit's safe APIs.

use std::num::NonZeroIsize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WheelScrollAmount {
    Lines(u32),
    Page,
}

/// Geometry consumed by the pure half of the Win32 `WM_NCHITTEST` bridge.
/// All values are physical pixels; callers derive them from the live window DPI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CustomFrameMetrics {
    pub width: i32,
    pub height: i32,
    pub title_bar_height: i32,
    pub tab_strip_right_px: i32,
    pub caption_button_width: i32,
    pub caption_button_count: i32,
    pub resize_border: i32,
    pub resizable: bool,
}

/// The self-drawn frame's two logical measurements, handed in at install time
/// rather than restated here.
///
/// # One number for the bar that is painted and the bar that is clicked
///
/// This crate used to keep its own `TITLE_BAR_LOGICAL_PX = 40` and
/// `CAPTION_BUTTON_LOGICAL_PX = 46` for the hit test while `bt-render` exported
/// `WINDOW_TITLE_BAR_LOGICAL_PX` and `WINDOW_CAPTION_BUTTON_LOGICAL_PX` for the
/// painting — two copies of the same design decision, in two crates, agreeing
/// by coincidence. The multiwindow spike (Q5, item 2) called it in: what is
/// drawn and what is clicked must be the same number, and the number belongs to
/// the side that draws it. So it arrives as an argument, and this crate no
/// longer has an opinion about how tall a title bar is.
///
/// Logical pixels at Win32's 96-DPI baseline; every read scales them by the
/// window's live DPI, so one window at 1.5x and another at 2.0x are two
/// different physical bars from one pair of numbers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CustomFrameGeometry {
    pub title_bar_logical_px: u32,
    pub caption_button_logical_px: u32,
}

/// Win32 non-client regions expressed without Win32 constants so their mapping
/// can be pinned on every host used by the workspace tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CustomFrameHit {
    Client,
    Caption,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Map one client-coordinate point to the native non-client region it represents.
/// Resize edges win over the title bar, and the complete settings/caption-button
/// run stays `Client` so the application can paint and handle those buttons.
#[must_use]
pub fn custom_frame_hit_test(metrics: CustomFrameMetrics, x: i32, y: i32) -> CustomFrameHit {
    let border = metrics.resize_border.max(0);
    let left = metrics.resizable && x >= 0 && x < border;
    let right = metrics.resizable && x >= metrics.width.saturating_sub(border) && x < metrics.width;
    let top = metrics.resizable && y >= 0 && y < border;
    let bottom =
        metrics.resizable && y >= metrics.height.saturating_sub(border) && y < metrics.height;

    match (left, right, top, bottom) {
        (true, _, true, _) => return CustomFrameHit::TopLeft,
        (_, true, true, _) => return CustomFrameHit::TopRight,
        (true, _, _, true) => return CustomFrameHit::BottomLeft,
        (_, true, _, true) => return CustomFrameHit::BottomRight,
        (true, _, _, _) => return CustomFrameHit::Left,
        (_, true, _, _) => return CustomFrameHit::Right,
        (_, _, true, _) => return CustomFrameHit::Top,
        (_, _, _, true) => return CustomFrameHit::Bottom,
        _ => {}
    }

    let buttons_width = metrics
        .caption_button_width
        .max(0)
        .saturating_mul(metrics.caption_button_count.max(0));
    let buttons_left = metrics.width.saturating_sub(buttons_width);
    if y < border || y >= metrics.title_bar_height.max(border) {
        return CustomFrameHit::Client;
    }
    if x < metrics.tab_strip_right_px.max(0) {
        return CustomFrameHit::Client;
    }
    if x < buttons_left || x >= metrics.width {
        CustomFrameHit::Caption
    } else {
        CustomFrameHit::Client
    }
}

/// Scale a logical chrome measurement using Win32's 96-DPI baseline.
#[must_use]
pub fn logical_px_for_dpi(logical_px: u32, dpi: u32) -> i32 {
    let scaled = u64::from(logical_px)
        .saturating_mul(u64::from(dpi.max(1)))
        .saturating_add(48)
        / 96;
    scaled.min(i32::MAX as u64) as i32
}

/// The offset a [`Compositor`] gives its GPU visual, in the physical pixels
/// every rectangle in this program is already expressed in.
///
/// # A physical pixel is already a physical pixel
///
/// DirectComposition takes `f32` where the rest of this program carries `i32`,
/// and the whole of the conversion is that cast — which is exactly the point of
/// naming it. A visual's offset is in **physical** pixels (WebView2 spike Q4:
/// measured on a 1.5x monitor and correct to the pixel), and every rectangle
/// `bt-layout` produces is already physical, so the one mistake available here
/// is to helpfully multiply by a scale factor on the way past. There is nothing
/// to multiply by; there is a cast.
///
/// The cast is exact for every coordinate a desktop can produce: `f32` carries
/// every integer up to 2^24, and a virtual desktop spanning four 8K monitors
/// does not reach one ten-thousandth of that.
#[must_use]
pub fn composition_visual_offset(x: i32, y: i32) -> (f32, f32) {
    (x as f32, y as f32)
}

/// The parameters `explorer.exe` is handed to **reveal** a path (user ruling,
/// 2026-08-13).
///
/// # 「Show me where this is」, not 「open the folder it is in」
///
/// Every foot in this window carries a path and offers to take you to it. What
/// that used to mean was `ShellExecute("open", <the parent folder>)` — Explorer
/// opened on a directory and the file the foot was actually naming was one of
/// two hundred rows, indistinguishable from the rest. `/select` is the verb
/// that means what the foot says: the folder opens **with that item
/// highlighted**.
///
/// A directory keeps the old answer, and that is a judgement rather than a
/// limitation: `/select` on a folder opens its *parent* with the folder
/// highlighted, which is one level further out than a foot pointing at a root
/// is offering. Looking *inside* it is the natural reading of a tree's own
/// root, so a directory is opened and a file is selected.
///
/// Pure, because the one thing that can be wrong here is the string — Explorer
/// parses `/select,<path>` as a single token and wants the path quoted, and a
/// command line that is a quote out is a command line that silently opens
/// `Documents` instead.
#[must_use]
pub fn reveal_arguments(path: &std::path::Path, is_directory: bool) -> std::ffi::OsString {
    let mut arguments = std::ffi::OsString::new();
    if !is_directory {
        arguments.push("/select,");
    }
    arguments.push("\"");
    arguments.push(path.as_os_str());
    arguments.push("\"");
    arguments
}

/// The extensions this product will decode a picture from, lower case.
///
/// **One list, three readers.** It is the file chooser's filter
/// ([`image_file_filter_spec`]), it is `validate_local_image_path`'s admission
/// gate, and it is the same set `bt_term::has_admissible_image_extension`
/// answers with — a chooser that offered a format the decoder refuses is a
/// dialog that lets you pick a file and then says no, which is worse than not
/// offering it. `svg` is on the list and does not go through the `image` crate;
/// it is rasterised by `bt_math`, and that is the decoder's business rather than
/// this list's.
pub const IMAGE_FILE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "gif", "svg"];

/// [`IMAGE_FILE_EXTENSIONS`] as a common-dialog filter — `*.png;*.jpg;…`.
///
/// Built rather than written out, so the filter cannot fall behind the list the
/// decoder actually honours. One spec and not one per format: a chooser with
/// six entries in its type dropdown asks the user to classify their own
/// wallpaper before they can see it.
#[must_use]
pub fn image_file_filter_spec() -> String {
    IMAGE_FILE_EXTENSIONS
        .iter()
        .map(|extension| format!("*.{extension}"))
        .collect::<Vec<_>>()
        .join(";")
}

/// One monospaced family this machine has, named and located.
///
/// **Both halves, and that is why the type exists.** A font picker needs the
/// name — it is what goes in the list, what is written to `settings.json`, and
/// what a shaper is later asked to resolve. But a name alone cannot be honoured:
/// `bt-render`'s font database is deliberately not a system-wide enumeration
/// (see its `terminal_font_system`), so a family it has never loaded is a family
/// `Family::Name("…")` falls back out of. Handing back the files alongside the
/// name is what lets the chosen family actually be loaded, and it is why this
/// goes through DirectWrite rather than through GDI's cheaper
/// `EnumFontFamiliesExW`, which answers only the first half.
///
/// `files` may hold several paths — a family's regular, bold, italic and bold
/// italic are four faces and usually four files — and may be *empty*, which is
/// not a failure: see [`DEFAULT_MONOSPACE_FAMILY`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonospaceFamily {
    /// The family name as DirectWrite reports it, preferring the machine's own
    /// UI language and falling back to the first localized name the family has.
    pub name: String,
    /// Every file the family's faces live in, de-duplicated, in the order the
    /// faces were reported.
    pub files: Vec<std::path::PathBuf>,
}

/// The family the renderer draws when a settings file names none.
///
/// It is a constant here rather than a lookup because [`monospace_font_families`]
/// promises the list contains it: a picker whose list can come back without the
/// entry that is currently selected is a picker that shows a blank row on the
/// one machine where DirectWrite refuses.
pub const DEFAULT_MONOSPACE_FAMILY: &str = "Consolas";

/// Sort the enumerated families into the order a list draws them in, and
/// guarantee the default is among them.
///
/// Split out from the enumeration itself so the ordering promise can be tested
/// on any host: the DirectWrite half needs a Windows font collection, this half
/// needs nothing, and the property worth pinning — *the list is deterministic
/// and always contains the default* — lives entirely here.
///
/// Case-insensitive because "consolas" and "Consolas" are the same family to
/// every user who reads the list, and because DirectWrite's own ordering is the
/// collection's internal one, which is not stable between machines or between
/// font installs on one machine.
///
/// The default, if the collection did not report it, is inserted with **no
/// files**, and that is the honest value rather than a placeholder: the
/// renderer already has Consolas loaded from its fixed startup list, so there is
/// nothing to load — an empty `files` says "this family needs no loading", which
/// is exactly true of every face the startup list already covers.
#[must_use]
pub fn order_monospace_families(mut families: Vec<MonospaceFamily>) -> Vec<MonospaceFamily> {
    families.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    families.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    if !families
        .iter()
        .any(|family| family.name.eq_ignore_ascii_case(DEFAULT_MONOSPACE_FAMILY))
    {
        let at = families
            .iter()
            .position(|family| family.name.to_lowercase() > DEFAULT_MONOSPACE_FAMILY.to_lowercase())
            .unwrap_or(families.len());
        families.insert(
            at,
            MonospaceFamily {
                name: DEFAULT_MONOSPACE_FAMILY.to_owned(),
                files: Vec::new(),
            },
        );
    }
    families
}

#[cfg(windows)]
mod windows_impl {
    use std::{
        ffi::c_void,
        os::windows::ffi::OsStrExt,
        path::{Path, PathBuf},
        sync::{
            Arc, Mutex, OnceLock,
            atomic::{AtomicI32, Ordering},
        },
    };
    use windows::core::{HRESULT, IUnknown, Interface, PCWSTR, PWSTR};

    use windows::Win32::{
        Foundation::{
            COLORREF, CloseHandle, ERROR_CANCELLED, GENERIC_WRITE, GetLastError, GlobalFree,
            HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, POINT, RECT, RPC_E_CHANGED_MODE, SetLastError,
            WAIT_EVENT, WAIT_OBJECT_0, WIN32_ERROR, WPARAM,
        },
        Globalization::{GetUserDefaultUILanguage, GetUserPreferredUILanguages, MUI_LANGUAGE_NAME},
        Graphics::DirectComposition::{
            DCompositionCreateDevice3, IDCompositionDesktopDevice, IDCompositionTarget,
            IDCompositionVisual,
        },
        Graphics::DirectWrite::{
            DWRITE_FACTORY_TYPE_SHARED, DWriteCreateFactory, IDWriteFactory, IDWriteFont1,
            IDWriteFontCollection, IDWriteFontFace, IDWriteFontFile, IDWriteLocalFontFileLoader,
            IDWriteLocalizedStrings,
        },
        Graphics::Dwm::{
            DWM_SYSTEMBACKDROP_TYPE, DWM_WINDOW_CORNER_PREFERENCE, DWMSBT_AUTO, DWMSBT_NONE,
            DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
            DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
        },
        Graphics::Gdi::{
            CreateSolidBrush, DeleteObject, GetMonitorInfoW, HGDIOBJ, MONITOR_DEFAULTTONEAREST,
            MONITORINFO, MonitorFromWindow,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
            FILE_FLAGS_AND_ATTRIBUTES, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE,
            FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_DIR_NAME,
            FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
            ReadDirectoryChangesW,
        },
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
            },
            // The front door's console half. `folio.exe --help` has to reach
            // whoever typed it, and on Windows that is a console this process may
            // not own — see `write_to_console`.
            Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetConsoleWindow, WriteConsoleW},
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
                OpenClipboard, SetClipboardData,
            },
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
            Threading::{
                CreateEventW, GetCurrentThread, GetThreadPriority, INFINITE, ResetEvent, SetEvent,
                SetThreadPriority, THREAD_PRIORITY, THREAD_PRIORITY_ABOVE_NORMAL,
                THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_NORMAL, WaitForMultipleObjects,
            },
        },
        UI::{
            HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
            Input::KeyboardAndMouse::GetKeyboardLayout,
            Shell::{
                Common::COMDLG_FILTERSPEC, DefSubclassProc, FO_DELETE, FOF_ALLOWUNDO,
                FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING,
                FOLDERID_Documents, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
                FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem, KF_FLAG_DEFAULT,
                RemoveWindowSubclass, SHCreateItemFromParsingName, SHFILEOPSTRUCTW,
                SHFileOperationW, SHGetKnownFolderPath, SIGDN_FILESYSPATH, SetWindowSubclass,
                ShellExecuteW,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreateCaret, CreatePopupMenu, DestroyCaret, DestroyMenu,
                GCLP_HBRBACKGROUND, GetClientRect, GetCursorPos, GetWindowRect, HTBOTTOM,
                HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP,
                HTTOPLEFT, HTTOPRIGHT, HWND_NOTOPMOST, HWND_TOPMOST, IsIconic, IsZoomed,
                MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MF_STRING, MINMAXINFO, MessageBoxW,
                NCCALCSIZE_PARAMS, PostMessageW, SM_CXFRAME, SM_CXPADDEDBORDER,
                SPI_GETCLIENTAREAANIMATION, SPI_GETWHEELSCROLLLINES, SW_SHOWNORMAL,
                SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
                SetCaretPos, SetClassLongPtrW, SetWindowPos, SystemParametersInfoW, TPM_RETURNCMD,
                TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_CLOSE, WM_GETMINMAXINFO, WM_NCCALCSIZE,
                WM_NCHITTEST,
            },
        },
    };

    use super::{
        CustomFrameGeometry, CustomFrameHit, CustomFrameMetrics, NonZeroIsize, ThreadPriority,
        WheelScrollAmount, WindowRect, composition_visual_offset, custom_frame_hit_test,
        logical_px_for_dpi,
    };

    /// GDI brush currently owned by this process and installed on winit's shared window class.
    /// The class itself outlives individual windows; theme switches replace this handle in place.
    static WINDOW_CLASS_BACKGROUND: OnceLock<Mutex<Option<isize>>> = OnceLock::new();
    const CF_UNICODETEXT: u32 = 13;
    const CLIPBOARD_OPEN_RETRY_DELAYS: [std::time::Duration; 4] = [
        std::time::Duration::from_millis(5),
        std::time::Duration::from_millis(10),
        std::time::Duration::from_millis(20),
        std::time::Duration::from_millis(40),
    ];
    const WHEEL_PAGESCROLL: u32 = u32::MAX;
    const DEFERRED_MATH_MENU_MESSAGE: u32 = WM_APP + 0x4b7;
    const DEFERRED_FOLDER_PICKER_MESSAGE: u32 = WM_APP + 0x4b8;
    const DEFERRED_IMAGE_PICKER_MESSAGE: u32 = WM_APP + 0x4b9;
    const MATH_MENU_SUBCLASS_ID: usize = 0x4254_4d4d;
    const FOLDER_PICKER_SUBCLASS_ID: usize = 0x4254_4650;
    const IMAGE_PICKER_SUBCLASS_ID: usize = 0x4254_4950;
    const CUSTOM_FRAME_SUBCLASS_ID: usize = 0x4254_4346;
    const CAPTION_BUTTON_COUNT: i32 = 4;

    /// The DirectComposition visual tree one window presents through.
    ///
    /// # Why the picture stopped going straight to the HWND
    ///
    /// It is the same picture, drawn by the same renderer, and on screen it is
    /// the same pixels. What changed is the door: wgpu's dx12 backend offers
    /// [`PreMultiplied`][premultiplied] composite alpha **only** to a visual
    /// target (`wgpu-hal-30.0.0/src/dx12/adapter.rs:1364` answers a
    /// `WndHandle` target with `vec![Opaque]` and nothing else), and a window
    /// that cannot be configured `PreMultiplied` can never have a hole cut in
    /// it for a web preview to show through. So the ground moves first, alone,
    /// with nothing above it: this slice changes where the swapchain hangs and
    /// changes nothing about what is drawn into it. (WebView2 spike, 切片建议
    /// item 1.)
    ///
    /// # The tree
    ///
    /// ```text
    /// target  ← CreateTargetForHwnd(hwnd, topmost = true)
    ///  └─ root                    ← IDCompositionTarget::SetRoot
    ///      └─ gpu                 ← the swapchain wgpu hangs here
    /// ```
    ///
    /// `topmost = true` is load-bearing rather than decorative: the whole tree
    /// is composed **above** the window's own painting, so anywhere the visuals
    /// do not cover shows the window class background brush
    /// ([`install_window_class_background`]) and not the desktop behind it.
    /// That is the same brush that has always been under the swapchain, so a
    /// frame that has not been presented yet looks exactly as it did before.
    ///
    /// The web preview's own visual becomes a second child of `root` in a later
    /// slice; the shape above is already the shape that takes it.
    ///
    /// # Failure is failure
    ///
    /// There is no fallback to an HWND swapchain. DirectComposition is present
    /// on every Windows this product supports, and a second presentation path
    /// kept alive "just in case" would be a second set of alpha semantics, a
    /// second resize behaviour and a second thing to photograph at acceptance —
    /// carried permanently against a failure mode that does not exist. If this
    /// constructor fails, the window fails to open and says why.
    ///
    /// [premultiplied]: https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/ne-dxgi1_2-dxgi_alpha_mode
    pub struct Compositor {
        /// The one object that commits. See [`Compositor::commit`].
        device: IDCompositionDesktopDevice,
        /// Held for its lifetime and read by nothing: this handle **is** the
        /// binding between the tree and the HWND, and dropping it unbinds it.
        _target: IDCompositionTarget,
        /// Likewise: the root is reached through the target from here on, and
        /// the slice that adds a web visual will reach it through a method
        /// rather than through this field.
        _root: IDCompositionVisual,
        gpu: IDCompositionVisual,
    }

    impl Compositor {
        /// Build the tree for one window. The window must already exist.
        pub fn new(hwnd: NonZeroIsize) -> Result<Self, String> {
            let hwnd = HWND(hwnd.get() as *mut c_void);
            // A null rendering device is the documented way to ask for a
            // composition device that only arranges visuals: this one never
            // rasterizes anything itself, because the only content it will ever
            // hold is a swapchain wgpu made on its own D3D12 device.
            let device: IDCompositionDesktopDevice =
                unsafe { DCompositionCreateDevice3(None::<&IUnknown>) }
                    .map_err(|error| compositor_failure("DCompositionCreateDevice3", &error))?;
            let target = unsafe { device.CreateTargetForHwnd(hwnd, true) }.map_err(|error| {
                compositor_failure("IDCompositionDesktopDevice::CreateTargetForHwnd", &error)
            })?;
            let root = Self::create_visual(&device, "root")?;
            let gpu = Self::create_visual(&device, "gpu")?;
            unsafe { root.AddVisual(&gpu, true, None::<&IDCompositionVisual>) }
                .map_err(|error| compositor_failure("IDCompositionVisual::AddVisual", &error))?;
            unsafe { target.SetRoot(&root) }
                .map_err(|error| compositor_failure("IDCompositionTarget::SetRoot", &error))?;
            let compositor = Self {
                device,
                _target: target,
                _root: root,
                gpu,
            };
            // The tree exists on screen only once it is committed, and this one
            // is empty until wgpu sets the swapchain on `gpu` — so what this
            // commit publishes is "a tree with nothing in it", which looks
            // exactly like the window did a moment ago. Committing here anyway
            // keeps the invariant simple: at no point does this type hold
            // uncommitted structure it is relying on someone else to publish.
            compositor.commit()?;
            Ok(compositor)
        }

        fn create_visual(
            device: &IDCompositionDesktopDevice,
            role: &str,
        ) -> Result<IDCompositionVisual, String> {
            let visual = unsafe { device.CreateVisual() }.map_err(|error| {
                compositor_failure(
                    &format!("IDCompositionDevice2::CreateVisual({role})"),
                    &error,
                )
            })?;
            // `CreateVisual` hands back an `IDCompositionVisual2`; the base
            // interface is what carries the offset and what wgpu expects on the
            // other side of `gpu_visual_ptr`, so the narrowing happens once,
            // here, rather than at every call.
            visual.cast::<IDCompositionVisual>().map_err(|error| {
                compositor_failure(&format!("IDCompositionVisual2::cast({role})"), &error)
            })
        }

        /// The raw `IDCompositionVisual` wgpu builds its surface upon.
        ///
        /// No ownership travels with it. wgpu increments the visual's COM
        /// refcount when it takes the pointer and holds that reference for as
        /// long as the surface lives (`wgpu-hal-30.0.0/src/dx12/mod.rs:551`), so
        /// the surface cannot outlive its content even if this `Compositor` is
        /// dropped first.
        #[must_use]
        pub fn gpu_visual_ptr(&self) -> *mut c_void {
            self.gpu.as_raw()
        }

        /// Move the GPU visual inside the window, in **physical** pixels.
        ///
        /// The main window's is `(0, 0)` and stays there — the swapchain covers
        /// the whole client area. The parameter exists because a window whose
        /// swapchain is one child among several is the shape this tree was built
        /// for, and because putting the conversion in one place is what stops
        /// someone scaling a physical rectangle by a scale factor a second time
        /// (see [`composition_visual_offset`]).
        ///
        /// Takes effect on the next [`Compositor::commit`], with that frame,
        /// atomically — which is the whole reason the WebView2 spike measured
        /// zero seam here and 4–10px of tearing on the child-window path.
        pub fn set_gpu_offset(&self, x: i32, y: i32) -> Result<(), String> {
            let (x, y) = composition_visual_offset(x, y);
            unsafe { self.gpu.SetOffsetX2(x) }
                .map_err(|error| compositor_failure("IDCompositionVisual::SetOffsetX", &error))?;
            unsafe { self.gpu.SetOffsetY2(y) }
                .map_err(|error| compositor_failure("IDCompositionVisual::SetOffsetY", &error))
        }

        /// Publish this frame. **Call once per presented frame.**
        ///
        /// # wgpu does not commit, and nothing else will
        ///
        /// When wgpu creates or resizes the swapchain it calls `SetContent` on
        /// our visual and stops there — `wgpu-hal-30.0.0/src/dx12/mod.rs:1619`,
        /// the `SurfaceTarget::Visual` arm, which is deliberately *not* the
        /// `VisualFromWndHandle` arm right above it where wgpu owns the
        /// composition device and does commit. Whoever owns the DirectComposition
        /// device owns the commit, and that is this type. Miss it and the picture
        /// does not move — not a dropped frame, not a stutter: nothing on screen
        /// ever changes again, while every trace in the program reports frames
        /// being presented normally.
        ///
        /// The app therefore calls this at exactly one place, immediately after
        /// the one call that presents a frame — see `Runtime::present_seats_and_commit`
        /// in `crates/bt-app/src/main.rs`, which is the only caller of
        /// `Renderer::present_seats` in the program.
        pub fn commit(&self) -> Result<(), String> {
            unsafe { self.device.Commit() }
                .map_err(|error| compositor_failure("IDCompositionDevice2::Commit", &error))
        }
    }

    /// One shape for every DirectComposition refusal: the call that refused, in
    /// its own name, and the `HRESULT` in both of the forms a search will be
    /// run on — Windows' sentence and the hex code the documentation indexes.
    ///
    /// Pure, and separated from the call sites for the same reason every other
    /// bridge in this crate separates its pure half: it is the part that can be
    /// wrong without a window, and therefore the part a test can hold.
    fn compositor_failure(step: &str, error: &windows::core::Error) -> String {
        format!(
            "{step} failed: {} (0x{:08X})",
            error.message(),
            error.code().0 as u32
        )
    }

    /// Keeps winit's ordinary overlapped-window styles (and therefore native
    /// snap, resize borders, minimize animation and system-menu semantics) while
    /// extending the client area through the system caption.
    pub struct CustomWindowFrame {
        hwnd: HWND,
        state: Box<CustomFrameState>,
    }

    /// Everything the subclass procedure reads out of the owning
    /// `CustomWindowFrame`. It is reached through the subclass reference data, so
    /// it must be a single stable allocation the frame keeps alive.
    struct CustomFrameState {
        /// This window's frame measurements, as the caller stated them. Plain
        /// values rather than atomics because they are settled at install and
        /// nothing may move them afterwards — a title bar that changed height
        /// mid-session would be painted at one number and clicked at another,
        /// which is the very thing this field exists to prevent.
        geometry: CustomFrameGeometry,
        tab_strip_right_px: AtomicI32,
        /// Smallest client size the window may be dragged to, in logical pixels,
        /// or `(0, 0)` for "no minimum". Logical rather than physical so the
        /// constraint survives a DPI change without anyone recomputing it.
        min_client_logical_width: AtomicI32,
        min_client_logical_height: AtomicI32,
    }

    impl CustomWindowFrame {
        pub fn install(hwnd: NonZeroIsize, geometry: CustomFrameGeometry) -> Result<Self, String> {
            let hwnd = HWND(hwnd.get() as *mut c_void);
            let state = Box::new(CustomFrameState {
                geometry,
                tab_strip_right_px: AtomicI32::new(0),
                min_client_logical_width: AtomicI32::new(0),
                min_client_logical_height: AtomicI32::new(0),
            });
            let reference_data = (&*state as *const CustomFrameState) as usize;
            let installed = unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(custom_frame_subclass),
                    CUSTOM_FRAME_SUBCLASS_ID,
                    reference_data,
                )
            };
            if !installed.as_bool() {
                return Err(format!(
                    "SetWindowSubclass(custom frame) failed: {}",
                    unsafe { GetLastError().0 }
                ));
            }
            // Re-run non-client calculation now that the subclass owns it. No
            // position, size or z-order changes are requested.
            if let Err(error) = unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            } {
                let _ = unsafe {
                    RemoveWindowSubclass(
                        hwnd,
                        Some(custom_frame_subclass),
                        CUSTOM_FRAME_SUBCLASS_ID,
                    )
                };
                return Err(format!("SetWindowPos(SWP_FRAMECHANGED) failed: {error}"));
            }
            // Claiming the caption does not surrender the Windows 11 rounded
            // corners every ordinary window wears; state the preference
            // explicitly so DWM keeps clipping the frame like the system
            // default. Best-effort: Windows 10 has no such attribute and is
            // square everywhere, which is its default too.
            let preference = DWMWCP_ROUND;
            let _ = unsafe {
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_WINDOW_CORNER_PREFERENCE,
                    &preference as *const DWM_WINDOW_CORNER_PREFERENCE as *const c_void,
                    std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
                )
            };
            Ok(Self { hwnd, state })
        }

        pub fn set_tab_strip_right_px(&self, tab_strip_right_px: i32) {
            self.state
                .tab_strip_right_px
                .store(tab_strip_right_px.max(0), Ordering::Relaxed);
        }

        /// Constrain how far the window may be resized, in logical pixels of
        /// *client* area. `None` lifts the constraint.
        ///
        /// This is the self-drawn frame's replacement for winit's
        /// `set_min_inner_size`, and it exists because winit's setter cannot be
        /// used on a window whose `WM_NCCALCSIZE` has made the client area the
        /// entire outer rect: winit implements it by re-requesting the current
        /// inner size, and re-requesting runs `AdjustWindowRectExForDpi`, which
        /// adds a frame margin this window does not have. The window grew by that
        /// margin on every call. Owning `WM_GETMINMAXINFO` states the same
        /// constraint to the same OS with no size request at all.
        pub fn set_min_client_size(&self, logical: Option<(u32, u32)>) -> Result<(), String> {
            let (width, height) = logical.unwrap_or((0, 0));
            let width = i32::try_from(width).unwrap_or(i32::MAX);
            let height = i32::try_from(height).unwrap_or(i32::MAX);
            self.state
                .min_client_logical_width
                .store(width, Ordering::Relaxed);
            self.state
                .min_client_logical_height
                .store(height, Ordering::Relaxed);
            if width <= 0 || height <= 0 {
                return Ok(());
            }
            // `WM_GETMINMAXINFO` only governs future sizing, so a window that is
            // already smaller than a freshly raised minimum has to be grown here.
            // A maximized window is not user-resizable and its rect belongs to the
            // monitor, so it is left alone.
            // SAFETY: `self.hwnd` is the live window this frame is installed on.
            if unsafe { IsZoomed(self.hwnd) }.as_bool() {
                return Ok(());
            }
            // SAFETY: as above; both queries are read-only and `rect` is
            // exclusively borrowed for the call.
            let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
            let mut rect = RECT::default();
            unsafe { GetWindowRect(self.hwnd, &mut rect) }
                .map_err(|error| format!("GetWindowRect failed: {error}"))?;
            let current_width = rect.right.saturating_sub(rect.left);
            let current_height = rect.bottom.saturating_sub(rect.top);
            let target_width = current_width.max(logical_px_for_dpi(width as u32, dpi));
            let target_height = current_height.max(logical_px_for_dpi(height as u32, dpi));
            if target_width == current_width && target_height == current_height {
                return Ok(());
            }
            // SAFETY: `self.hwnd` is live; no insert-after handle is passed, so
            // the null `hwndinsertafter` is inert under `SWP_NOZORDER`.
            unsafe {
                SetWindowPos(
                    self.hwnd,
                    None,
                    0,
                    0,
                    target_width,
                    target_height,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            }
            .map_err(|error| format!("SetWindowPos(grow to minimum) failed: {error}"))
        }
    }

    impl Drop for CustomWindowFrame {
        fn drop(&mut self) {
            let _ = unsafe {
                RemoveWindowSubclass(
                    self.hwnd,
                    Some(custom_frame_subclass),
                    CUSTOM_FRAME_SUBCLASS_ID,
                )
            };
        }
    }

    unsafe extern "system" fn custom_frame_subclass(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        reference_data: usize,
    ) -> LRESULT {
        match message {
            WM_NCCALCSIZE => {
                // A zoomed overlapped window deliberately extends its outer
                // resize frame beyond the monitor. With the entire outer rect
                // made client, those pixels would clip our titlebar/content.
                // Keep that invisible native frame as a maximized inset while
                // still removing the ordinary system caption everywhere else.
                if wparam.0 != 0 && lparam.0 != 0 && unsafe { IsZoomed(hwnd) }.as_bool() {
                    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
                    let border = native_resize_border(dpi);
                    let params = unsafe { &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS) };
                    params.rgrc[0].left = params.rgrc[0].left.saturating_add(border);
                    params.rgrc[0].top = params.rgrc[0].top.saturating_add(border);
                    params.rgrc[0].right = params.rgrc[0].right.saturating_sub(border);
                    params.rgrc[0].bottom = params.rgrc[0].bottom.saturating_sub(border);
                }
                LRESULT(0)
            }
            WM_GETMINMAXINFO => {
                let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
                if lparam.0 == 0 {
                    return result;
                }
                // SAFETY: `CustomWindowFrame` owns this allocation and removes the subclass
                // before dropping it, so the reference-data pointer is live for every callback.
                let state = unsafe { &*(reference_data as *const CustomFrameState) };
                let width = state.min_client_logical_width.load(Ordering::Relaxed);
                let height = state.min_client_logical_height.load(Ordering::Relaxed);
                if width <= 0 || height <= 0 {
                    return result;
                }
                let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
                // `ptMinTrackSize` is an *outer* rect bound. Under this frame the
                // client area is the outer rect, so a minimum client size is that
                // bound byte for byte, with no non-client margin to add.
                // SAFETY: for WM_GETMINMAXINFO the OS passes a live, writable
                // MINMAXINFO in `lparam`, checked non-null above.
                let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
                info.ptMinTrackSize.x = logical_px_for_dpi(width as u32, dpi);
                info.ptMinTrackSize.y = logical_px_for_dpi(height as u32, dpi);
                result
            }
            WM_NCHITTEST => {
                let mut client = RECT::default();
                if unsafe { GetClientRect(hwnd, &mut client) }.is_err() {
                    return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
                }
                let mut point = POINT {
                    x: low_word_signed(lparam.0),
                    y: high_word_signed(lparam.0),
                };
                if !unsafe { windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut point) }
                    .as_bool()
                {
                    return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
                }
                let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
                let resize_border = if unsafe { IsZoomed(hwnd) }.as_bool() {
                    0
                } else {
                    native_resize_border(dpi)
                };
                // SAFETY: `CustomWindowFrame` owns this allocation and removes the subclass
                // before dropping it, so the reference-data pointer is live for every callback.
                let state = unsafe { &*(reference_data as *const CustomFrameState) };
                let tab_strip_right_px = state.tab_strip_right_px.load(Ordering::Relaxed);
                let hit = custom_frame_hit_test(
                    CustomFrameMetrics {
                        width: client.right.saturating_sub(client.left),
                        height: client.bottom.saturating_sub(client.top),
                        title_bar_height: logical_px_for_dpi(
                            state.geometry.title_bar_logical_px,
                            dpi,
                        ),
                        tab_strip_right_px,
                        caption_button_width: logical_px_for_dpi(
                            state.geometry.caption_button_logical_px,
                            dpi,
                        ),
                        caption_button_count: CAPTION_BUTTON_COUNT,
                        resize_border,
                        resizable: resize_border > 0,
                    },
                    point.x,
                    point.y,
                );
                LRESULT(match hit {
                    CustomFrameHit::Client => HTCLIENT,
                    CustomFrameHit::Caption => HTCAPTION,
                    CustomFrameHit::Left => HTLEFT,
                    CustomFrameHit::Right => HTRIGHT,
                    CustomFrameHit::Top => HTTOP,
                    CustomFrameHit::Bottom => HTBOTTOM,
                    CustomFrameHit::TopLeft => HTTOPLEFT,
                    CustomFrameHit::TopRight => HTTOPRIGHT,
                    CustomFrameHit::BottomLeft => HTBOTTOMLEFT,
                    CustomFrameHit::BottomRight => HTBOTTOMRIGHT,
                } as isize)
            }
            _ => unsafe { DefSubclassProc(hwnd, message, wparam, lparam) },
        }
    }

    fn low_word_signed(value: isize) -> i32 {
        (value as u16 as i16) as i32
    }

    fn high_word_signed(value: isize) -> i32 {
        ((value as usize >> 16) as u16 as i16) as i32
    }

    fn native_resize_border(dpi: u32) -> i32 {
        unsafe {
            GetSystemMetricsForDpi(SM_CXFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
        }
        .max(1)
    }

    pub fn request_window_close(hwnd: NonZeroIsize) -> Result<(), String> {
        unsafe {
            PostMessageW(
                Some(HWND(hwnd.get() as *mut c_void)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            )
        }
        .map_err(|error| format!("PostMessageW(WM_CLOSE) failed: {error}"))
    }

    /// Ask Windows to open one already-policy-checked target with its registered default handler.
    /// Scheme allowlisting deliberately belongs to the caller; this bridge only supplies the
    /// audited UTF-16 and ShellExecuteW boundary.
    pub fn shell_execute(hwnd: NonZeroIsize, target: &str) -> Result<(), String> {
        if target.contains('\0') {
            return Err("ShellExecuteW target contains an embedded NUL".to_owned());
        }
        let hwnd = HWND(hwnd.get() as *mut c_void);
        let mut operation = "open".encode_utf16().collect::<Vec<_>>();
        operation.push(0);
        let mut target = target.encode_utf16().collect::<Vec<_>>();
        target.push(0);
        // SAFETY: both strings are live, NUL-terminated UTF-16 buffers for the duration of the
        // synchronous call. No parameters or working directory are supplied, so the URI is never
        // reparsed as a command line. `hwnd` is winit's live top-level window.
        let result = unsafe {
            ShellExecuteW(
                Some(hwnd),
                PCWSTR(operation.as_ptr()),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        let code = result.0 as isize;
        if code <= 32 {
            Err(format!("ShellExecuteW failed with code {code}"))
        } else {
            Ok(())
        }
    }

    /// Open one worker-validated local image with its registered default handler.
    ///
    /// The caller must obtain `path` from a successful image decode record, never directly from
    /// terminal text. This bridge independently enforces the slice's immutable syntax policy
    /// (drive-rooted, supported extension, no embedded NUL) and supplies no parameters or working
    /// directory, preventing command-line reinterpretation. It performs no event-thread file I/O.
    pub fn open_local_file(hwnd: NonZeroIsize, path: &Path) -> Result<(), String> {
        validate_local_image_path(path)?;
        let hwnd = HWND(hwnd.get() as *mut c_void);
        let mut operation = "open".encode_utf16().collect::<Vec<_>>();
        operation.push(0);
        let mut target = path.as_os_str().encode_wide().collect::<Vec<_>>();
        target.push(0);
        // SAFETY: the worker-success capability supplied an existing decoded image path, and both
        // buffers remain live and NUL-terminated for this synchronous call. Parameters and working
        // directory are null, so Windows never receives a command line to reparse.
        let result = unsafe {
            ShellExecuteW(
                Some(hwnd),
                PCWSTR(operation.as_ptr()),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        let code = result.0 as isize;
        if code <= 32 {
            Err(format!("ShellExecuteW failed with code {code}"))
        } else {
            Ok(())
        }
    }

    /// Open one file the user picked out of a directory listing with its
    /// registered default handler — and never run a program.
    ///
    /// **Why a second bridge rather than widening the first.** [`open_local_file`]
    /// serves paths *scraped out of terminal text*, where the only defence
    /// against a hostile line of output is that the syntax policy is narrow
    /// enough to be immutable. A row of the files tree has the opposite
    /// provenance: the user chose the root, this process enumerated the
    /// directory, and the user pressed the row. Making the two share one
    /// validator would mean either the tree can open nothing but pictures or
    /// terminal output can open anything.
    ///
    /// **Why programs are refused.** Not as a hedge — as the product rule
    /// `DESIGN.md` §7.1.3 already implies by making activation mean *open the
    /// preview*: the tree is a way of looking at files, and the thing next to it
    /// that runs programs is the terminal, where running one is a line you typed
    /// and can see. A double click that silently starts an executable is a verb
    /// this pane does not have, and the fact that the pane sits half an inch
    /// from a shell prompt is exactly why it should not acquire it by accident.
    pub fn open_local_path(hwnd: NonZeroIsize, path: &Path) -> Result<(), String> {
        validate_openable_path(path)?;
        if names_a_program(path, std::env::var("PATHEXT").unwrap_or_default().as_str()) {
            return Err(PROGRAM_REFUSED.to_owned());
        }
        let hwnd = HWND(hwnd.get() as *mut c_void);
        let mut operation = "open".encode_utf16().collect::<Vec<_>>();
        operation.push(0);
        let mut target = path.as_os_str().encode_wide().collect::<Vec<_>>();
        target.push(0);
        // SAFETY: the path was enumerated by this process from a directory the
        // user rooted a column at, both buffers stay live and NUL-terminated for
        // this synchronous call, and parameters and working directory are null,
        // so Windows never receives a command line to reparse.
        let result = unsafe {
            ShellExecuteW(
                Some(hwnd),
                PCWSTR(operation.as_ptr()),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        let code = result.0 as isize;
        if code <= 32 {
            Err(format!("ShellExecuteW failed with code {code}"))
        } else {
            Ok(())
        }
    }

    /// Open Explorer on a path, with a file **highlighted** inside its folder
    /// (user ruling, 2026-08-13).
    ///
    /// **A third bridge, and deliberately not a widening of the second.**
    /// [`open_local_path`] hands a path to *whatever the machine has registered
    /// for it* — which is why it reads `PATHEXT` and refuses programs, since
    /// the whole risk there is that opening a thing runs it. This one hands the
    /// path to `explorer.exe` **as text to look at**, and never executes the
    /// target at all: a `.exe` revealed is a `.exe` sitting highlighted in a
    /// folder window, which is precisely what somebody asking "where is this"
    /// wants to see and is not a way to start it. So the extension gate is
    /// absent on purpose, and the shape gate — absolute, nameable, no embedded
    /// NUL — is exactly the one its neighbour keeps.
    ///
    /// The one program this can ever launch is Explorer.
    pub fn reveal_in_explorer(hwnd: NonZeroIsize, path: &Path) -> Result<(), String> {
        validate_openable_path(path)?;
        let hwnd = HWND(hwnd.get() as *mut c_void);
        let mut operation = "open".encode_utf16().collect::<Vec<_>>();
        operation.push(0);
        let mut program = "explorer.exe".encode_utf16().collect::<Vec<_>>();
        program.push(0);
        let mut arguments = super::reveal_arguments(path, path.is_dir())
            .encode_wide()
            .collect::<Vec<_>>();
        arguments.push(0);
        // SAFETY: the target is a path this process enumerated or was given by
        // the user, `validate_openable_path` has refused any embedded NUL, all
        // three buffers stay live and NUL-terminated across this synchronous
        // call, and the only program named is Explorer.
        let result = unsafe {
            ShellExecuteW(
                Some(hwnd),
                PCWSTR(operation.as_ptr()),
                PCWSTR(program.as_ptr()),
                PCWSTR(arguments.as_ptr()),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        let code = result.0 as isize;
        if code <= 32 {
            Err(format!("ShellExecuteW failed with code {code}"))
        } else {
            Ok(())
        }
    }

    /// The refusal's own words, so the caller can tell "this window will not do
    /// that" apart from "Windows could not".
    pub const PROGRAM_REFUSED: &str = "the files tree does not run programs";

    /// The extensions that are a program whatever this machine's `PATHEXT` says.
    ///
    /// `PATHEXT` is the system's own list of what a *command line* will execute
    /// and it is read as well, but it is not the whole answer: a `.lnk` is not
    /// on it and points at anything at all, a `.scr` is an executable wearing a
    /// screensaver's name, and `.hta`, `.reg`, `.msi` and `.url` are each opened
    /// by a handler whose whole job is to act. Reading both means the list grows
    /// with a machine that has added to `PATHEXT` without shrinking on one that
    /// has emptied it.
    const ALWAYS_A_PROGRAM: &[&str] = &[
        "appref-ms",
        "bat",
        "cmd",
        "com",
        "cpl",
        "exe",
        "hta",
        "jar",
        "js",
        "jse",
        "lnk",
        "msc",
        "msi",
        "msp",
        "ps1",
        "pif",
        "reg",
        "scf",
        "scr",
        "url",
        "vb",
        "vbe",
        "vbs",
        "wsf",
        "wsh",
    ];

    /// Whether opening this name would start something rather than show it.
    ///
    /// Split out and given `pathext` as an argument rather than reading the
    /// environment itself, so the rule is answerable in a test on a machine
    /// whose own `PATHEXT` is whatever it is.
    fn names_a_program(path: &Path, pathext: &str) -> bool {
        let Some(extension) = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
        else {
            // No extension means no registered handler to speak of, and
            // `ShellExecute` falls back to the "open with" chooser rather than
            // to running anything. That is a dialog, not an execution.
            return false;
        };
        ALWAYS_A_PROGRAM.contains(&extension.as_str())
            || pathext.split(';').any(|entry| {
                entry
                    .trim()
                    .trim_start_matches('.')
                    .eq_ignore_ascii_case(&extension)
            })
    }

    /// The shape gate the tree's own bridge keeps: absolute and nameable.
    ///
    /// Wider than [`validate_local_image_path`] in exactly one way — a UNC share
    /// is allowed — because a files column may legitimately be rooted at
    /// `\\server\share`, and a tree that can list a path it then refuses to open
    /// is a tree that lies about what its rows are.
    fn validate_openable_path(path: &Path) -> Result<(), String> {
        let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if units.contains(&0) {
            return Err("path contains an embedded NUL".to_owned());
        }
        let text = path.as_os_str().to_string_lossy();
        let bytes = text.as_bytes();
        let drive_rooted = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/');
        let unc = text.starts_with(r"\\") && text.len() > 2;
        if !drive_rooted && !unc {
            return Err("path must be absolute".to_owned());
        }
        Ok(())
    }

    fn validate_local_image_path(path: &Path) -> Result<(), String> {
        let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if units.contains(&0) {
            return Err("local image path contains an embedded NUL".to_owned());
        }
        let text = path.as_os_str().to_string_lossy();
        let bytes = text.as_bytes();
        if bytes.len() < 3
            || !bytes[0].is_ascii_alphabetic()
            || bytes[1] != b':'
            || !matches!(bytes[2], b'\\' | b'/')
        {
            return Err("local image path must be drive-rooted and absolute".to_owned());
        }
        let allowed_extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                let extension = extension.to_ascii_lowercase();
                crate::IMAGE_FILE_EXTENSIONS.contains(&extension.as_str())
            });
        if !allowed_extension {
            return Err("local image path extension is not supported".to_owned());
        }
        Ok(())
    }

    pub fn wheel_scroll_amount() -> Result<WheelScrollAmount, String> {
        let mut lines = 0u32;
        // SAFETY: SPI_GETWHEELSCROLLLINES writes one u32 to the provided live stack pointer.
        unsafe {
            SystemParametersInfoW(
                SPI_GETWHEELSCROLLLINES,
                0,
                Some((&mut lines as *mut u32).cast()),
                Default::default(),
            )
        }
        .map_err(|error| {
            format!("SystemParametersInfoW(SPI_GETWHEELSCROLLLINES) failed: {error}")
        })?;
        Ok(if lines == WHEEL_PAGESCROLL {
            WheelScrollAmount::Page
        } else {
            WheelScrollAmount::Lines(lines)
        })
    }

    /// Whether the system wants animation inside a window's client area.
    ///
    /// `SPI_GETCLIENTAREAANIMATION` is Windows' own name for the preference a
    /// browser reports as `prefers-reduced-motion` — Settings → Accessibility →
    /// Visual effects → Animation effects. It is the setting the mock-up's
    /// `@media (prefers-reduced-motion: reduce)` blocks answer to, so reading it
    /// here is what makes those rules real on this platform rather than a
    /// stylesheet branch nothing ever takes.
    ///
    /// `TRUE` means animation is *wanted*, so the polarity is the opposite of
    /// the CSS query's — the caller does that mapping, and pins it.
    pub fn client_area_animation_enabled() -> Result<bool, String> {
        // `BOOL` is a 32-bit int across the Win32 ABI, and this is the shape
        // `SystemParametersInfoW` writes through the void pointer it is given.
        let mut enabled = 0_i32;
        // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL to the provided
        // live stack pointer, exactly as SPI_GETWHEELSCROLLLINES writes one u32.
        unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                Some((&mut enabled as *mut i32).cast()),
                Default::default(),
            )
        }
        .map_err(|error| {
            format!("SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION) failed: {error}")
        })?;
        Ok(enabled != 0)
    }

    fn retry_open_clipboard(
        mut open: impl FnMut() -> Result<(), String>,
        mut wait: impl FnMut(std::time::Duration),
    ) -> Result<(), String> {
        for delay in CLIPBOARD_OPEN_RETRY_DELAYS {
            match open() {
                Ok(()) => return Ok(()),
                Err(_) => wait(delay),
            }
        }
        open().map_err(|error| {
            format!(
                "OpenClipboard failed after {} attempts (retry wait capped at {} ms): {error}",
                CLIPBOARD_OPEN_RETRY_DELAYS.len() + 1,
                CLIPBOARD_OPEN_RETRY_DELAYS
                    .iter()
                    .map(std::time::Duration::as_millis)
                    .sum::<u128>()
            )
        })
    }

    fn open_clipboard_with_retry(hwnd: HWND) -> Result<(), String> {
        retry_open_clipboard(
            || {
                // SAFETY: the caller supplies winit's live HWND and all clipboard transactions run
                // on its event-loop thread. A failed open acquires no resource that needs cleanup.
                unsafe { OpenClipboard(Some(hwnd)) }.map_err(|error| error.to_string())
            },
            std::thread::sleep,
        )
    }

    pub fn clipboard_text(hwnd: NonZeroIsize) -> Result<String, String> {
        let hwnd = HWND(hwnd.get() as *mut c_void);
        // SAFETY: all calls run on winit's event-loop thread. The clipboard remains open while the
        // borrowed global-memory handle is locked, its UTF-16 content is copied, and then both the
        // memory and clipboard are released before returning.
        open_clipboard_with_retry(hwnd)?;
        unsafe {
            let result = (|| {
                IsClipboardFormatAvailable(CF_UNICODETEXT)
                    .map_err(|_| "clipboard has no Unicode text".to_owned())?;
                let handle = GetClipboardData(CF_UNICODETEXT)
                    .map_err(|error| format!("GetClipboardData(CF_UNICODETEXT) failed: {error}"))?;
                let global = HGLOBAL(handle.0);
                let byte_len = GlobalSize(global);
                if byte_len < size_of::<u16>() {
                    return Ok(String::new());
                }
                let pointer = GlobalLock(global).cast::<u16>();
                if pointer.is_null() {
                    return Err(format!(
                        "GlobalLock(clipboard text) failed: {}",
                        GetLastError().0
                    ));
                }
                let units = std::slice::from_raw_parts(pointer, byte_len / size_of::<u16>());
                let nul = units
                    .iter()
                    .position(|unit| *unit == 0)
                    .unwrap_or(units.len());
                let copied = units[..nul].to_vec();
                // GlobalUnlock returns false after releasing the final lock; GetLastError
                // distinguishes that successful case, so no recovery is attached to its Result.
                let _ = GlobalUnlock(global);
                String::from_utf16(&copied)
                    .map_err(|error| format!("clipboard text is invalid UTF-16: {error}"))
            })();
            let close = CloseClipboard().map_err(|error| format!("CloseClipboard failed: {error}"));
            match (result, close) {
                (Ok(text), Ok(())) => Ok(text),
                (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            }
        }
    }

    pub fn set_clipboard_text(hwnd: NonZeroIsize, text: &str) -> Result<(), String> {
        let hwnd = HWND(hwnd.get() as *mut c_void);
        let mut units = text.encode_utf16().collect::<Vec<_>>();
        units.push(0);
        // SAFETY: the event-loop thread owns the clipboard for this transaction. The movable
        // allocation is locked only while copying the NUL-terminated UTF-16 payload. After a
        // successful SetClipboardData Windows owns it; every earlier failure frees it locally.
        open_clipboard_with_retry(hwnd)?;
        unsafe {
            let result = (|| {
                EmptyClipboard().map_err(|error| format!("EmptyClipboard failed: {error}"))?;
                let byte_len = units.len() * size_of::<u16>();
                let global = GlobalAlloc(GMEM_MOVEABLE, byte_len)
                    .map_err(|error| format!("GlobalAlloc(clipboard text) failed: {error}"))?;
                let pointer = GlobalLock(global).cast::<u16>();
                if pointer.is_null() {
                    let error = GetLastError().0;
                    let _ = GlobalFree(Some(global));
                    return Err(format!("GlobalLock(clipboard text) failed: {error}"));
                }
                std::ptr::copy_nonoverlapping(units.as_ptr(), pointer, units.len());
                let _ = GlobalUnlock(global);
                if let Err(error) = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(global.0))) {
                    let _ = GlobalFree(Some(global));
                    return Err(format!("SetClipboardData(CF_UNICODETEXT) failed: {error}"));
                }
                Ok(())
            })();
            let close = CloseClipboard().map_err(|error| format!("CloseClipboard failed: {error}"));
            match (result, close) {
                (Ok(()), Ok(())) => Ok(()),
                (Err(error), _) | (Ok(()), Err(error)) => Err(error),
            }
        }
    }

    /// One deferred native gesture, from asked-for to answered.
    ///
    /// `Ask` is what the request carried and `Answer` is what it came back with.
    /// The request's own arguments ride in `Posted` rather than in a second
    /// field beside the phase, because they are only meaningful while the phase
    /// is `Posted`: a start folder left lying around after the dialog closed is
    /// a value with no owner and no expiry.
    #[derive(Debug)]
    enum DeferredPhase<Ask, Answer> {
        Idle,
        Posted(Ask),
        Showing,
        Complete(Answer),
    }

    /// The gate that keeps a nested native message pump out of a winit callback.
    ///
    /// Generic over both ends so the one state machine serves every gesture that
    /// has to be deferred this way — the formula menu, whose nested pump is
    /// `TrackPopupMenu`'s, and the folder picker, whose nested pump is
    /// `IFileDialog::Show`'s. Both have exactly the same hazard and therefore
    /// exactly the same shape: post a private message, do the blocking thing
    /// after the initiating callback has returned, and leave the answer where
    /// the next turn of the loop will find it.
    #[derive(Debug)]
    struct DeferredState<Ask, Answer> {
        phase: Mutex<DeferredPhase<Ask, Answer>>,
    }

    impl<Ask, Answer> DeferredState<Ask, Answer> {
        fn new() -> Self {
            Self {
                phase: Mutex::new(DeferredPhase::Idle),
            }
        }

        fn begin_request(&self, ask: Ask) -> bool {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if !matches!(*phase, DeferredPhase::Idle) {
                return false;
            }
            *phase = DeferredPhase::Posted(ask);
            true
        }

        fn cancel_request(&self) {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if matches!(*phase, DeferredPhase::Posted(_)) {
                *phase = DeferredPhase::Idle;
            }
        }

        /// Take the gesture's arguments and move it to `Showing`, or `None` when
        /// this is not a posted request to begin.
        fn begin_showing(&self) -> Option<Ask> {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if !matches!(*phase, DeferredPhase::Posted(_)) {
                return None;
            }
            let DeferredPhase::Posted(ask) = std::mem::replace(&mut *phase, DeferredPhase::Showing)
            else {
                unreachable!("phase was matched as posted immediately before replacement")
            };
            Some(ask)
        }

        fn complete(&self, answer: Answer) {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if matches!(*phase, DeferredPhase::Showing) {
                *phase = DeferredPhase::Complete(answer);
            }
        }

        fn take_result(&self) -> Option<Answer> {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            let DeferredPhase::Complete(_) = &*phase else {
                return None;
            };
            let DeferredPhase::Complete(answer) =
                std::mem::replace(&mut *phase, DeferredPhase::Idle)
            else {
                unreachable!("phase was matched as complete immediately before replacement")
            };
            Some(answer)
        }
    }

    type MathMenuState = DeferredState<(), Result<bool, String>>;

    /// Formula context-menu bridge whose nested native message pump never starts inside a winit
    /// application callback. `request` only posts a private window message. The subclass receives
    /// it after the current `DispatchMessageW`/winit callback has returned, so any RedrawRequested
    /// emitted by TrackPopupMenu's nested pump finds winit's event-handler slot restored.
    pub struct MathContextMenu {
        hwnd: HWND,
        state: Arc<MathMenuState>,
    }

    impl MathContextMenu {
        pub fn new(hwnd: NonZeroIsize) -> Result<Self, String> {
            let hwnd = HWND(hwnd.get() as *mut c_void);
            let state = Arc::new(MathMenuState::new());
            // SAFETY: installation and removal occur on the HWND's event-loop thread. The Arc
            // keeps dwRefData live for the full installed interval; the callback takes its own
            // temporary strong reference before entering the nested menu loop.
            let installed = unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(math_context_menu_subclass),
                    MATH_MENU_SUBCLASS_ID,
                    Arc::as_ptr(&state) as usize,
                )
            };
            if !installed.as_bool() {
                return Err(format!("SetWindowSubclass(math menu) failed: {}", unsafe {
                    GetLastError().0
                }));
            }
            Ok(Self { hwnd, state })
        }

        /// Queue the native menu once. A second request while the first is posted, showing, or
        /// waiting to be consumed is an ordinary coalesced UI race and returns `Ok(false)`.
        pub fn request(&self) -> Result<bool, String> {
            if !self.state.begin_request(()) {
                return Ok(false);
            }
            // SAFETY: PostMessageW copies these value parameters into the owning thread's queue
            // and never dispatches the subclass synchronously on this callback stack.
            if let Err(error) = unsafe {
                PostMessageW(
                    Some(self.hwnd),
                    DEFERRED_MATH_MENU_MESSAGE,
                    WPARAM(0),
                    LPARAM(0),
                )
            } {
                self.state.cancel_request();
                return Err(format!("PostMessageW(math menu) failed: {error}"));
            }
            Ok(true)
        }

        pub fn take_result(&self) -> Option<Result<bool, String>> {
            self.state.take_result()
        }
    }

    impl Drop for MathContextMenu {
        fn drop(&mut self) {
            // SAFETY: this object is dropped on the same event-loop thread that installed the
            // subclass. A callback already inside TrackPopupMenu owns a temporary Arc, so nested
            // CloseRequested teardown cannot invalidate its state.
            let _ = unsafe {
                RemoveWindowSubclass(
                    self.hwnd,
                    Some(math_context_menu_subclass),
                    MATH_MENU_SUBCLASS_ID,
                )
            };
        }
    }

    unsafe extern "system" fn math_context_menu_subclass(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        reference_data: usize,
    ) -> LRESULT {
        if message != DEFERRED_MATH_MENU_MESSAGE {
            // SAFETY: forwarding untouched messages is the required subclass contract.
            return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        }
        let state_pointer = reference_data as *const MathMenuState;
        if state_pointer.is_null() {
            return LRESULT(0);
        }
        // SAFETY: the installed MathContextMenu owns one Arc at callback entry. Incrementing before
        // constructing the temporary Arc keeps state alive even if a nested CloseRequested drops
        // the Runtime while TrackPopupMenu is open.
        unsafe { Arc::increment_strong_count(state_pointer) };
        // SAFETY: the increment immediately above created the strong reference consumed here.
        let state = unsafe { Arc::from_raw(state_pointer) };
        if state.begin_showing().is_some() {
            state.complete(track_math_context_menu(hwnd));
        }
        LRESULT(0)
    }

    fn track_math_context_menu(hwnd: HWND) -> Result<bool, String> {
        // SAFETY: the HWND and menu belong to the current GUI thread. This function is reached only
        // from the posted-message subclass, after winit's initiating callback has returned.
        unsafe {
            let menu =
                CreatePopupMenu().map_err(|error| format!("CreatePopupMenu failed: {error}"))?;
            let result = (|| {
                AppendMenuW(menu, MF_STRING, 1, windows::core::w!("Copy LaTeX"))
                    .map_err(|error| format!("AppendMenuW(Copy LaTeX) failed: {error}"))?;
                let mut cursor = POINT::default();
                GetCursorPos(&mut cursor)
                    .map_err(|error| format!("GetCursorPos failed: {error}"))?;
                let command = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    cursor.x,
                    cursor.y,
                    None,
                    hwnd,
                    None,
                );
                Ok(command.0 as usize == 1)
            })();
            let destroy = DestroyMenu(menu).map_err(|error| format!("DestroyMenu failed: {error}"));
            match (result, destroy) {
                (Ok(selected), Ok(())) => Ok(selected),
                (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            }
        }
    }

    type FolderPickerState = DeferredState<Vec<u16>, Result<Option<PathBuf>, String>>;

    /// The system's own folder chooser, on the same deferred footing as
    /// [`MathContextMenu`] and for a sharper version of the same reason (E55).
    ///
    /// `IFileDialog::Show` is **modal**: it runs a nested message loop that goes
    /// on dispatching to this window for as long as the dialog is open. Started
    /// from inside a winit callback that would re-enter the application's own
    /// event handling while the `&mut` borrow that started it is still live —
    /// the exact hazard the formula menu's comment records, except that a menu
    /// closes in a moment and a folder chooser can stand open for a minute while
    /// somebody goes looking. So the press only *posts*: the dialog opens from
    /// the subclass, after the callback has returned, and the answer waits in
    /// this state until the next turn of the loop collects it.
    pub struct FolderPicker {
        hwnd: HWND,
        state: Arc<FolderPickerState>,
    }

    impl FolderPicker {
        pub fn new(hwnd: NonZeroIsize) -> Result<Self, String> {
            let hwnd = HWND(hwnd.get() as *mut c_void);
            let state = Arc::new(FolderPickerState::new());
            // SAFETY: installation and removal occur on the HWND's event-loop thread. The Arc
            // keeps dwRefData live for the full installed interval; the callback takes its own
            // temporary strong reference before entering the nested dialog loop.
            let installed = unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(folder_picker_subclass),
                    FOLDER_PICKER_SUBCLASS_ID,
                    Arc::as_ptr(&state) as usize,
                )
            };
            if !installed.as_bool() {
                return Err(format!(
                    "SetWindowSubclass(folder picker) failed: {}",
                    unsafe { GetLastError().0 }
                ));
            }
            Ok(Self { hwnd, state })
        }

        /// Queue the chooser once, starting at `start` if that names a folder.
        ///
        /// A second request while one is posted, showing or waiting to be
        /// collected returns `Ok(false)` — the same coalescing the formula menu
        /// does, and here it is also what stops a second press on `Browse…`
        /// from stacking a second modal dialog behind the first.
        pub fn request(&self, start: Option<&Path>) -> Result<bool, String> {
            let start = start
                .map(|start| {
                    let mut units = start.as_os_str().encode_wide().collect::<Vec<_>>();
                    units.push(0);
                    units
                })
                .unwrap_or_default();
            if !self.state.begin_request(start) {
                return Ok(false);
            }
            // SAFETY: PostMessageW copies these value parameters into the owning thread's queue
            // and never dispatches the subclass synchronously on this callback stack.
            if let Err(error) = unsafe {
                PostMessageW(
                    Some(self.hwnd),
                    DEFERRED_FOLDER_PICKER_MESSAGE,
                    WPARAM(0),
                    LPARAM(0),
                )
            } {
                self.state.cancel_request();
                return Err(format!("PostMessageW(folder picker) failed: {error}"));
            }
            Ok(true)
        }

        /// The chosen folder, `None` for a cancelled dialog, or the reason the
        /// chooser could not be shown — once, and only once the dialog is shut.
        pub fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
            self.state.take_result()
        }
    }

    impl Drop for FolderPicker {
        fn drop(&mut self) {
            // SAFETY: this object is dropped on the same event-loop thread that installed the
            // subclass. A callback already inside the dialog owns a temporary Arc, so nested
            // CloseRequested teardown cannot invalidate its state.
            let _ = unsafe {
                RemoveWindowSubclass(
                    self.hwnd,
                    Some(folder_picker_subclass),
                    FOLDER_PICKER_SUBCLASS_ID,
                )
            };
        }
    }

    type ImagePickerState = DeferredState<Vec<u16>, Result<Option<PathBuf>, String>>;

    /// The system's own file chooser, filtered to the pictures this product can
    /// decode — [`FolderPicker`]'s twin, and every word of its doc comment
    /// applies unchanged.
    ///
    /// A second subclass on the same window rather than a mode on the first,
    /// because the deferral's whole contract is "one gesture in flight": a
    /// picker that coalesced a wallpaper request into a pending folder request
    /// would answer the wrong row. Two states, two private messages, two
    /// subclass ids — and one dialog function underneath
    /// ([`show_shell_picker`]), because the COM dance is the part that must not
    /// be written twice.
    pub struct ImagePicker {
        hwnd: HWND,
        state: Arc<ImagePickerState>,
    }

    impl ImagePicker {
        pub fn new(hwnd: NonZeroIsize) -> Result<Self, String> {
            let hwnd = HWND(hwnd.get() as *mut c_void);
            let state = Arc::new(ImagePickerState::new());
            // SAFETY: installation and removal occur on the HWND's event-loop thread. The Arc
            // keeps dwRefData live for the full installed interval; the callback takes its own
            // temporary strong reference before entering the nested dialog loop.
            let installed = unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(image_picker_subclass),
                    IMAGE_PICKER_SUBCLASS_ID,
                    Arc::as_ptr(&state) as usize,
                )
            };
            if !installed.as_bool() {
                return Err(format!(
                    "SetWindowSubclass(image picker) failed: {}",
                    unsafe { GetLastError().0 }
                ));
            }
            Ok(Self { hwnd, state })
        }

        /// Queue the chooser once, starting in `start` if that names a folder.
        ///
        /// `start` is a **folder** and not the picture currently chosen: a shell
        /// item that is a file is not a place to open at, and the useful place
        /// to open at is the one the last picture came from.
        pub fn request(&self, start: Option<&Path>) -> Result<bool, String> {
            let start = start
                .map(|start| {
                    let mut units = start.as_os_str().encode_wide().collect::<Vec<_>>();
                    units.push(0);
                    units
                })
                .unwrap_or_default();
            if !self.state.begin_request(start) {
                return Ok(false);
            }
            // SAFETY: PostMessageW copies these value parameters into the owning thread's queue
            // and never dispatches the subclass synchronously on this callback stack.
            if let Err(error) = unsafe {
                PostMessageW(
                    Some(self.hwnd),
                    DEFERRED_IMAGE_PICKER_MESSAGE,
                    WPARAM(0),
                    LPARAM(0),
                )
            } {
                self.state.cancel_request();
                return Err(format!("PostMessageW(image picker) failed: {error}"));
            }
            Ok(true)
        }

        /// The chosen picture, `None` for a cancelled dialog, or the reason the
        /// chooser could not be shown — once, and only once the dialog is shut.
        pub fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
            self.state.take_result()
        }
    }

    impl Drop for ImagePicker {
        fn drop(&mut self) {
            // SAFETY: this object is dropped on the same event-loop thread that installed the
            // subclass. A callback already inside the dialog owns a temporary Arc, so nested
            // CloseRequested teardown cannot invalidate its state.
            let _ = unsafe {
                RemoveWindowSubclass(
                    self.hwnd,
                    Some(image_picker_subclass),
                    IMAGE_PICKER_SUBCLASS_ID,
                )
            };
        }
    }

    unsafe extern "system" fn image_picker_subclass(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        reference_data: usize,
    ) -> LRESULT {
        if message != DEFERRED_IMAGE_PICKER_MESSAGE {
            // SAFETY: forwarding untouched messages is the required subclass contract.
            return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        }
        let state_pointer = reference_data as *const ImagePickerState;
        if state_pointer.is_null() {
            return LRESULT(0);
        }
        // SAFETY: the installed ImagePicker owns one Arc at callback entry. Incrementing before
        // constructing the temporary Arc keeps state alive even if a nested CloseRequested drops
        // the Runtime while the dialog is open.
        unsafe { Arc::increment_strong_count(state_pointer) };
        // SAFETY: the increment immediately above created the strong reference consumed here.
        let state = unsafe { Arc::from_raw(state_pointer) };
        if let Some(start) = state.begin_showing() {
            state.complete(show_shell_picker(hwnd, &start, ShellPickKind::Image));
        }
        LRESULT(0)
    }

    /// Put this window above (or back among) the others — `HWND_TOPMOST` /
    /// `HWND_NOTOPMOST`.
    ///
    /// The first caller in this crate to touch z-order at all: every other
    /// `SetWindowPos` here passes `SWP_NOZORDER` because it is moving or
    /// resizing and has no business reordering anything. This one is *only*
    /// about order, so it passes neither a position nor a size.
    ///
    /// `SWP_NOACTIVATE`, because "stay in front" is not "come to the front now":
    /// switching the row on while another window has the keyboard must not steal
    /// it, and switching it off must not either.
    pub fn set_window_topmost(hwnd: NonZeroIsize, topmost: bool) -> Result<(), String> {
        let after = if topmost {
            HWND_TOPMOST
        } else {
            HWND_NOTOPMOST
        };
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle, and the insert-after
        // handle is one of the two documented sentinels rather than a window we might outlive.
        unsafe {
            SetWindowPos(
                HWND(hwnd.get() as *mut c_void),
                Some(after),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        }
        .map_err(|error| format!("SetWindowPos(topmost={topmost}) failed: {error}"))
    }

    /// Tell DWM which of the two canvases this window is wearing —
    /// `DWMWA_USE_IMMERSIVE_DARK_MODE`.
    ///
    /// **This is the whole of "the acrylic plate follows the scheme"**
    /// (§7.1.6c-4f amendment, measured on this machine 2026-08-18). The plate
    /// `DWMSBT_TRANSIENTWINDOW` draws is DWM's, not ours, and the one thing DWM
    /// lets a window say about its colour is this flag — so the plate is dark
    /// exactly when the window has declared itself dark. Measured, light desktop
    /// `#F2F2F2` behind, Solarized Dark at 30 %: the pane body reads
    /// `(156,177,183)` without the flag and `(99,120,126)` with it.
    ///
    /// **The order does not matter and the plate is not latched.** 4f wrote down
    /// that setting this to 1 did not darken the material; it does. Setting it
    /// before the backdrop, after the backdrop, and either side of a
    /// `DWMSBT_NONE` round trip all measured the identical `(99,120,126)`, so a
    /// window may say this whenever its canvas moves and need not re-ask for the
    /// backdrop afterwards.
    ///
    /// **Not tied to the Acrylic row.** The flag is a statement about the
    /// window, not about one setting: it is also what colours the one-pixel DWM
    /// border (`(176,176,176)` -> `(153,153,153)` in the same measurement), and
    /// a dark window with the blur switched off still owes itself a dark border.
    ///
    /// Best-effort, like the corner preference above and unlike
    /// [`set_system_backdrop`]: there is no settings row whose position claims
    /// this happened, so a Windows too old to know the attribute (it is
    /// 20H1's; 1809 spelled it 19) simply keeps the border it already had.
    pub fn set_window_dark_mode(hwnd: NonZeroIsize, dark: bool) -> Result<(), String> {
        let value = i32::from(dark);
        // SAFETY: as `set_system_backdrop` — the pointer is to a live local of
        // exactly the size passed, and DWM copies it before returning.
        unsafe {
            DwmSetWindowAttribute(
                HWND(hwnd.get() as *mut c_void),
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                std::ptr::from_ref(&value).cast::<c_void>(),
                size_of::<i32>() as u32,
            )
        }
        .map_err(|error| format!("DwmSetWindowAttribute(immersive dark={dark}) failed: {error}"))
    }

    /// Ask DWM for (or withdraw) the acrylic system backdrop —
    /// `DWMWA_SYSTEMBACKDROP_TYPE` = `DWMSBT_TRANSIENTWINDOW` / `DWMSBT_NONE`.
    ///
    /// `DWMSBT_TRANSIENTWINDOW` and not `DWMSBT_MAINWINDOW`: the two differ in
    /// how much of what is behind survives the blur, and the transient one is
    /// the stronger, more saturated recipe Windows uses for flyouts and command
    /// palettes. A terminal with a picture behind it wants the one that still
    /// shows you what is back there.
    ///
    /// Withdrawing is `DWMSBT_NONE` and not `DWMSBT_AUTO`: `AUTO` hands the
    /// decision back to DWM, which is a different answer from "off" and would
    /// leave a Mica-eligible window quietly wearing Mica after the row said no.
    ///
    /// Failure is returned rather than swallowed, unlike the corner-preference
    /// call above it, because this one has a row on a settings page: a user who
    /// switches it on is owed the reason it did not take, and
    /// [`system_backdrop_available`] is that reason asked in advance.
    pub fn set_system_backdrop(hwnd: NonZeroIsize, acrylic: bool) -> Result<(), String> {
        let backdrop = if acrylic {
            DWMSBT_TRANSIENTWINDOW
        } else {
            DWMSBT_NONE
        };
        // SAFETY: the pointer is to a live local of exactly the size passed, and DWM copies it
        // before returning.
        unsafe {
            DwmSetWindowAttribute(
                HWND(hwnd.get() as *mut c_void),
                DWMWA_SYSTEMBACKDROP_TYPE,
                std::ptr::from_ref(&backdrop).cast::<c_void>(),
                size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
            )
        }
        .map_err(|error| format!("DwmSetWindowAttribute(system backdrop) failed: {error}"))
    }

    /// Whether this Windows knows what a system backdrop is.
    ///
    /// **Asked of DWM, not of a build number.** `DWMWA_SYSTEMBACKDROP_TYPE`
    /// arrived in Windows 11 22H2 and every older Windows answers `E_INVALIDARG`
    /// for an attribute it has never heard of — so setting the attribute to its
    /// own default (`DWMSBT_AUTO`, which is what an untouched window already
    /// has) both changes nothing and returns the exact fact the row needs. A
    /// build-number gate would be this crate maintaining a table of which
    /// Windows has which feature, which is a table that is wrong the moment
    /// anybody backports anything.
    #[must_use]
    pub fn system_backdrop_available(hwnd: NonZeroIsize) -> bool {
        let probe = DWMSBT_AUTO;
        // SAFETY: as `set_system_backdrop`; the value written is the attribute's own default.
        unsafe {
            DwmSetWindowAttribute(
                HWND(hwnd.get() as *mut c_void),
                DWMWA_SYSTEMBACKDROP_TYPE,
                std::ptr::from_ref(&probe).cast::<c_void>(),
                size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
            )
        }
        .is_ok()
    }

    unsafe extern "system" fn folder_picker_subclass(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        reference_data: usize,
    ) -> LRESULT {
        if message != DEFERRED_FOLDER_PICKER_MESSAGE {
            // SAFETY: forwarding untouched messages is the required subclass contract.
            return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        }
        let state_pointer = reference_data as *const FolderPickerState;
        if state_pointer.is_null() {
            return LRESULT(0);
        }
        // SAFETY: the installed FolderPicker owns one Arc at callback entry. Incrementing before
        // constructing the temporary Arc keeps state alive even if a nested CloseRequested drops
        // the Runtime while the dialog is open.
        unsafe { Arc::increment_strong_count(state_pointer) };
        // SAFETY: the increment immediately above created the strong reference consumed here.
        let state = unsafe { Arc::from_raw(state_pointer) };
        if let Some(start) = state.begin_showing() {
            state.complete(show_shell_picker(hwnd, &start, ShellPickKind::Folder));
        }
        LRESULT(0)
    }

    /// A NUL-terminated UTF-16 copy, for the Win32 calls that take a `PCWSTR`
    /// and read until the terminator.
    fn wide_null(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Which of the two things `IFileOpenDialog` is being asked for.
    ///
    /// One dialog function and two guises rather than two functions, because
    /// everything around the two lines that differ — the apartment, its balance,
    /// the cancel/error split, the shell allocation nobody else frees — is
    /// identical and is the part that is easy to get subtly wrong twice.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum ShellPickKind {
        Folder,
        Image,
    }

    /// Show `IFileOpenDialog` and report what came back.
    ///
    /// `IFileOpenDialog` rather than the older `SHBrowseForFolder`, because the
    /// latter draws a tree from 1995 with no address bar, no typing, no
    /// favourites and no resize — and the folder row exists precisely for the
    /// folders the quick list above it could not name, which are the ones you
    /// need to navigate to.
    ///
    /// `FOS_FORCEFILESYSTEM` is what keeps the answer a *path*: without it the
    /// dialog will happily return a shell namespace item — a library, a phone
    /// over MTP, a search results folder — that has no directory behind it for
    /// anything here to enumerate. For a picture it does a second job: an image
    /// on a phone over MTP has no path for the decoder to open.
    fn show_shell_picker(
        hwnd: HWND,
        start: &[u16],
        kind: ShellPickKind,
    ) -> Result<Option<PathBuf>, String> {
        // SAFETY: every call below runs on the window's own GUI thread, reached only from the
        // posted-message subclass after winit's initiating callback has returned. The apartment
        // is initialized and balanced here, and each COM object is dropped by the `windows`
        // crate's own `Release` at the end of its scope.
        unsafe {
            let apartment = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
            if apartment == RPC_E_CHANGED_MODE {
                // The thread is already a multi-threaded apartment, which a shell
                // dialog may not be shown from. Reported rather than forced: the
                // caller's answer to a chooser it cannot show is to leave the
                // root where it was, which is the correct outcome and not one
                // worth breaking someone's apartment model to avoid.
                return Err("the event-loop thread is not a single-threaded apartment".to_owned());
            }
            // S_FALSE means "already initialized, and this call counted" — it is a
            // success that still owes a `CoUninitialize`, which is why the balance
            // is taken from `is_ok` rather than from `== S_OK`.
            let balance = apartment.is_ok();
            let result = (|| {
                let dialog: IFileOpenDialog =
                    CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                        .map_err(|error| format!("CoCreateInstance(FileOpenDialog): {error}"))?;
                let options = dialog
                    .GetOptions()
                    .map_err(|error| format!("IFileDialog::GetOptions: {error}"))?;
                let options = match kind {
                    ShellPickKind::Folder => {
                        options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST
                    }
                    // `FOS_FILEMUSTEXIST` and not `FOS_PATHMUSTEXIST` alone: the
                    // answer is going straight to a decoder, and a name typed
                    // into the box for a file that is not there would arrive as
                    // a decode failure a second later, with the dialog already
                    // gone and nothing on screen to connect the two.
                    ShellPickKind::Image => {
                        options | FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST
                    }
                };
                dialog
                    .SetOptions(options)
                    .map_err(|error| format!("IFileDialog::SetOptions: {error}"))?;
                // The filter is built from the one extension list the decoder
                // honours, so the chooser cannot offer a format that would be
                // refused a moment after it was picked. Its own failure is not
                // fatal — an unfiltered dialog still chooses a file, and the
                // decoder is still the thing that decides.
                let filter_name: Vec<u16>;
                let filter_spec: Vec<u16>;
                if kind == ShellPickKind::Image {
                    filter_name = wide_null("Images");
                    filter_spec = wide_null(&crate::image_file_filter_spec());
                    let filters = [COMDLG_FILTERSPEC {
                        pszName: PCWSTR(filter_name.as_ptr()),
                        pszSpec: PCWSTR(filter_spec.as_ptr()),
                    }];
                    let _ = dialog.SetFileTypes(&filters);
                }
                // Where the column is already rooted, so the chooser opens where
                // you are looking rather than wherever Windows last left it. Its
                // failure is not this function's failure: a root that has since
                // been deleted or unplugged is a perfectly good reason to open at
                // the system's default instead of refusing to open at all.
                if start.len() > 1
                    && let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(
                        PCWSTR(start.as_ptr()),
                        None,
                    )
                {
                    let _ = dialog.SetFolder(&folder);
                }
                if let Err(error) = dialog.Show(Some(hwnd)) {
                    return if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                        Ok(None)
                    } else {
                        Err(format!("IFileDialog::Show: {error}"))
                    };
                }
                let item = dialog
                    .GetResult()
                    .map_err(|error| format!("IFileOpenDialog::GetResult: {error}"))?;
                let name = item
                    .GetDisplayName(SIGDN_FILESYSPATH)
                    .map_err(|error| format!("IShellItem::GetDisplayName: {error}"))?;
                let path = name.to_string();
                // The shell allocated it; the shell's allocator frees it. This is
                // the one allocation in this function that Rust does not own.
                CoTaskMemFree(Some(name.0.cast()));
                let noun = match kind {
                    ShellPickKind::Folder => "folder",
                    ShellPickKind::Image => "picture",
                };
                path.map(|path| Some(PathBuf::from(path)))
                    .map_err(|error| format!("the chosen {noun}'s name is not UTF-16: {error}"))
            })();
            if balance {
                CoUninitialize();
            }
            result
        }
    }

    /// Thread-affine native positioning bridge used by Microsoft Pinyin.
    ///
    /// Some Chinese IMEs follow the Win32 system caret instead of IMM32 candidate-form requests.
    /// winit remains the sole owner of the HIMC candidate/composition forms; this bridge only
    /// supplies the otherwise invisible 1x1 caret signal.
    pub struct ImeSystemCaret {
        hwnd: HWND,
        active: bool,
    }

    impl ImeSystemCaret {
        pub fn new(hwnd: NonZeroIsize) -> Self {
            Self {
                hwnd: HWND(hwnd.get() as *mut c_void),
                active: false,
            }
        }

        pub fn update(&mut self, x: i32, y: i32) -> Result<(), String> {
            if !active_layout_is_chinese() {
                self.destroy();
                return Ok(());
            }

            // SAFETY: the HWND originates from winit and calls occur on its event-loop thread.
            // A Win32 caret is owned by that GUI thread. The null bitmap plus 1x1 dimensions is
            // the non-painted compatibility caret used only as an IME positioning signal.
            unsafe {
                if !self.active {
                    CreateCaret(self.hwnd, None, 1, 1)
                        .map_err(|error| format!("CreateCaret(1x1) failed: {error}"))?;
                    self.active = true;
                }
                SetCaretPos(x, y).map_err(|error| format!("SetCaretPos failed: {error}"))?;
            }
            Ok(())
        }

        pub fn destroy(&mut self) {
            if !self.active {
                return;
            }
            // SAFETY: this object is dropped/disabled on the same event-loop thread that created
            // the thread-affine caret. There is no borrowed memory and failure needs no recovery.
            unsafe {
                let _ = DestroyCaret();
            }
            self.active = false;
        }
    }

    impl Drop for ImeSystemCaret {
        fn drop(&mut self) {
            self.destroy();
        }
    }

    fn active_layout_is_chinese() -> bool {
        // SAFETY: thread id zero asks user32 for the calling event-loop thread's active layout.
        let layout = unsafe { GetKeyboardLayout(0) };
        primary_language_id(layout.0 as usize as u16) == 0x04
    }

    fn primary_language_id(language_id: u16) -> u16 {
        language_id & 0x03ff
    }

    pub fn get_dpi_for_window(hwnd: NonZeroIsize) -> Result<u32, String> {
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle. GetDpiForWindow only
        // reads the DPI associated with that window and returns zero for an invalid handle.
        let dpi = unsafe { GetDpiForWindow(HWND(hwnd.get() as *mut c_void)) };
        if dpi == 0 {
            Err("GetDpiForWindow returned zero".to_owned())
        } else {
            Ok(dpi)
        }
    }

    /// Whether the window is minimized (iconic).
    ///
    /// The companion of the `IsZoomed` test the snapshot already makes, and it
    /// exists for the same reason: while a window is iconic, `GetWindowRect`
    /// stops describing the window and starts describing the icon — Windows
    /// parks the rectangle far off-screen at a token size (`-32000, -32000` in
    /// the classic shell; a 157x25 rect at `x = -16000` was what this app
    /// actually recorded). That rectangle is not a place the user ever put the
    /// window, so nothing may be derived from it.
    ///
    /// Deliberately *not* `GetWindowPlacement().rcNormalPosition`, which looks
    /// like the more direct answer and is a trap: `rcNormalPosition` is stated
    /// in workspace coordinates, so on any machine with a taskbar it differs
    /// from `GetWindowRect`'s screen coordinates by the work-area origin. Mixing
    /// the two would reintroduce exactly the per-restart drift that making this
    /// module speak one rectangle — the outer rect — was meant to end.
    pub fn is_window_minimized(hwnd: NonZeroIsize) -> bool {
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle.
        // IsIconic only reads window state and reports false for a bad handle.
        unsafe { IsIconic(HWND(hwnd.get() as *mut c_void)) }.as_bool()
    }

    pub fn get_window_rect(hwnd: NonZeroIsize) -> Result<WindowRect, String> {
        let mut rect = RECT::default();
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle and `rect` remains valid
        // and exclusively borrowed for the duration of this read-only query.
        unsafe { GetWindowRect(HWND(hwnd.get() as *mut c_void), &mut rect) }
            .map_err(|error| format!("GetWindowRect failed: {error}"))?;
        Ok(WindowRect {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        })
    }

    /// Place the window's *outer* rectangle exactly, in physical screen pixels.
    ///
    /// The counterpart of [`get_window_rect`], and the only sizing call that is
    /// meaningful once [`CustomWindowFrame`] is installed: winit sizes by client
    /// area and derives the outer rect with `AdjustWindowRectExForDpi`, but this
    /// window's `WM_NCCALCSIZE` has made the two the same rectangle, so that
    /// derivation adds a frame margin the window does not wear. Passing the outer
    /// rect straight through is what makes `GetWindowRect` -> save -> restore ->
    /// `GetWindowRect` an identity.
    pub fn set_window_outer_rect(hwnd: NonZeroIsize, rect: WindowRect) -> Result<(), String> {
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle. No
        // insert-after handle is passed, which `SWP_NOZORDER` makes inert.
        unsafe {
            SetWindowPos(
                HWND(hwnd.get() as *mut c_void),
                None,
                rect.left,
                rect.top,
                rect.right.saturating_sub(rect.left).max(1),
                rect.bottom.saturating_sub(rect.top).max(1),
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        }
        .map_err(|error| format!("SetWindowPos(restore window rect) failed: {error}"))
    }

    /// The work area — the monitor minus the taskbar and other appbars — of the
    /// display this window sits on, in physical pixels.
    ///
    /// `docs/M2-layout-solver-spec.md` §2.6.5 clamps the window's minimum inner
    /// size to 60% of *this* rectangle, not of the monitor: a minimum computed
    /// against the full monitor would quietly include the taskbar's strip and let
    /// the window refuse to shrink past something the user can never see all of.
    /// Failure is reported rather than guessed at, because tiny-window §4.4 rules
    /// that a never-observed work area means "set no minimum at all".
    pub fn get_work_area(hwnd: NonZeroIsize) -> Result<WindowRect, String> {
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle.
        // MonitorFromWindow with MONITOR_DEFAULTTONEAREST always returns a valid
        // monitor handle, and `info` stays valid and exclusively borrowed across
        // this read-only query with its `cbSize` set as the API requires.
        let ok = unsafe {
            let monitor =
                MonitorFromWindow(HWND(hwnd.get() as *mut c_void), MONITOR_DEFAULTTONEAREST);
            GetMonitorInfoW(monitor, &mut info)
        };
        if !ok.as_bool() {
            return Err("GetMonitorInfoW failed".to_owned());
        }
        Ok(WindowRect {
            left: info.rcWork.left,
            top: info.rcWork.top,
            right: info.rcWork.right,
            bottom: info.rcWork.bottom,
        })
    }

    /// **Send one file to the Recycle Bin** (§7.1.6c-4d).
    ///
    /// The whole of the difference between this and `std::fs::remove_file` is
    /// the promise the card makes: `Delete scheme` says the file was moved to
    /// the Recycle Bin, so it has to be findable there. A colour scheme is a
    /// file the user wrote by hand — often over an afternoon — and a delete
    /// button in a settings dialog that destroys one is a button nobody can
    /// afford to press by accident.
    ///
    /// `SHFileOperationW` rather than `IFileOperation`: the newer interface
    /// wants an apartment and an `IShellItem` per file, and what is being asked
    /// here is one path with one flag. The flags say exactly that and no more —
    /// no confirmation (the dialog already knows what it is deleting), no
    /// progress UI and no error UI for a single small file, and **`ALLOWUNDO`
    /// paired with `WANTNUKEWARNING`**: a file the bin cannot take must raise
    /// the shell's own "this will be deleted permanently" prompt rather than be
    /// destroyed silently, because "moved to the Recycle Bin" would then be a
    /// sentence this product had told and not kept.
    ///
    /// `Ok(false)` is the user answering that prompt with no: nothing happened,
    /// and it is not an error.
    ///
    /// The path is passed **double-NUL terminated**, which is this API's list
    /// convention: a single terminator would leave the shell reading whatever
    /// follows the buffer as the second file to delete.
    pub fn recycle(path: &std::path::Path) -> Result<bool, String> {
        let mut wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .chain(std::iter::once(0))
            .collect();
        if wide.iter().filter(|unit| **unit == 0).count() != 2 {
            return Err("a path with an interior NUL is not a file name".to_owned());
        }
        let mut operation = SHFILEOPSTRUCTW {
            wFunc: FO_DELETE,
            pFrom: PCWSTR(wide.as_mut_ptr()),
            fFlags: (FOF_ALLOWUNDO
                | FOF_NOCONFIRMATION
                | FOF_WANTNUKEWARNING
                | FOF_NOERRORUI
                | FOF_SILENT)
                .0 as u16,
            ..Default::default()
        };
        // SAFETY: `operation` is exclusively borrowed for the call and `wide`
        // outlives it, holding a double-NUL-terminated UTF-16 path as `pFrom`
        // requires. The call performs no callbacks and returns a status code.
        let status = unsafe { SHFileOperationW(&raw mut operation) };
        if status != 0 {
            return Err(format!("SHFileOperationW failed: {status}"));
        }
        Ok(!operation.fAnyOperationsAborted.as_bool())
    }

    /// Paint this window's own background in `rgb`, or in **nothing** at all.
    ///
    /// `None` installs the null brush, and that is what a translucent ground is
    /// made of (§7.1.6c-4b). The composition tree is created `topmost = true`,
    /// which composites it **above** the window's own painting (§2.3 A2) — so
    /// wherever the swapchain is translucent, what shows through is this brush
    /// and not the desktop. An alpha on the clear alone therefore produces a
    /// window that is see-through onto its own opaque background, which looks
    /// exactly like a window that is not see-through at all. Removing the brush
    /// is the other half of the feature.
    ///
    /// The cost is stated rather than hidden: with no brush, the band a resize
    /// opens up before the swapchain reaches it shows the desktop instead of the
    /// theme colour. That is the correct answer for a window the user has asked
    /// to be see-through, and it is why the opaque case keeps its brush.
    pub fn install_window_class_background(
        hwnd: NonZeroIsize,
        rgb: Option<[u8; 3]>,
    ) -> Result<(), String> {
        let mut installed = WINDOW_CLASS_BACKGROUND
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "window class background brush lock poisoned".to_owned())?;
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle on the event-loop thread.
        // CreateSolidBrush returns an independent GDI brush. SetClassLongPtrW atomically replaces
        // the class brush; only after that succeeds do we delete the previous brush that *we* own.
        // The original winit brush returned on the first call is not ours and is never deleted here.
        unsafe {
            let brush = match rgb {
                Some([r, g, b]) => {
                    let color = COLORREF(u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16));
                    let brush = CreateSolidBrush(color);
                    if brush.is_invalid() {
                        return Err(format!("CreateSolidBrush failed: {}", GetLastError().0));
                    }
                    Some(brush)
                }
                None => None,
            };

            SetLastError(WIN32_ERROR(0));
            let previous = SetClassLongPtrW(
                HWND(hwnd.get() as *mut c_void),
                GCLP_HBRBACKGROUND,
                brush.map_or(0, |brush| brush.0 as isize),
            );
            let error = GetLastError();
            if previous == 0 && error.0 != 0 {
                if let Some(brush) = brush {
                    let _ = DeleteObject(HGDIOBJ(brush.0));
                }
                return Err(format!(
                    "SetClassLongPtrW(GCLP_HBRBACKGROUND) failed: {}",
                    error.0
                ));
            }
            let old_owned = match brush {
                Some(brush) => installed.replace(brush.0 as isize),
                None => installed.take(),
            };
            if let Some(old_owned) = old_owned {
                let _ = DeleteObject(HGDIOBJ(old_owned as *mut c_void));
            }
        }
        Ok(())
    }

    /// The BCP 47 tag Windows would write this user's *interface* in, e.g.
    /// `"zh-Hans-CN"`, `"en-US"`, `"ja-JP"`.
    ///
    /// **The UI language and deliberately not the locale.** `GetUserDefaultLocaleName`
    /// answers a different question — which calendar, which decimal point, which
    /// sort order — and on a great many machines the two disagree: a Chinese
    /// developer running an English Windows has `zh-CN` formats and an `en-US`
    /// shell, and a product that read the locale would put its menus in a language
    /// that user deliberately did not install. What "System" means in the Language
    /// row is *the language the rest of your Windows is in*, and this is the call
    /// that answers it.
    ///
    /// Preferred-languages first because that is the ordered list the user
    /// actually edits in Settings, and it comes back as a name rather than as a
    /// LANGID — no table of 16-bit numbers to keep. [`GetUserDefaultUILanguage`]
    /// is the floor under it: it cannot fail, and the only two ids this product
    /// has to tell apart are the Chinese ones, whose primary language is
    /// `LANG_CHINESE = 0x04`. A machine that answered neither would be read as
    /// English, which is what the source language is.
    #[must_use]
    pub fn os_ui_language() -> String {
        let mut count = 0u32;
        let mut length = 0u32;
        if unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut length) }
            .is_ok()
            && length > 0
        {
            let mut buffer = vec![0u16; length as usize];
            if unsafe {
                GetUserPreferredUILanguages(
                    MUI_LANGUAGE_NAME,
                    &mut count,
                    Some(PWSTR(buffer.as_mut_ptr())),
                    &mut length,
                )
            }
            .is_ok()
                && count > 0
            {
                // A `MULTI_SZ`: the first NUL ends the first — the preferred — tag.
                let first: Vec<u16> = buffer.into_iter().take_while(|unit| *unit != 0).collect();
                if !first.is_empty() {
                    return String::from_utf16_lossy(&first);
                }
            }
        }
        // LANGID floor. `0x04` is `LANG_CHINESE`; the sub-language distinguishes
        // Simplified from Traditional, and neither this crate nor its caller has
        // to know which, because both are `zh`.
        let langid = unsafe { GetUserDefaultUILanguage() };
        if langid & 0x3ff == 0x04 {
            "zh".to_owned()
        } else {
            "en".to_owned()
        }
    }

    /// This user's `Documents` folder, as Windows itself resolves it.
    ///
    /// **Never `%USERPROFILE%\Documents`.** That string is wrong on a great many
    /// real machines and wrong in the way that matters here: a redirected
    /// Documents (OneDrive, a roaming profile, a second drive) is exactly where
    /// PowerShell's `$HOME\Documents\WindowsPowerShell\Modules` actually
    /// resolves to, because PowerShell asks the same known folder. Writing a
    /// module to the literal path on a redirected machine puts it somewhere
    /// PowerShell will never look, and the only symptom is a module that
    /// installs successfully and does nothing.
    ///
    /// `KF_FLAG_DEFAULT` and not `KF_FLAG_CREATE`: this answers where the folder
    /// *is*, and creating a user's Documents folder as a side effect of reading
    /// a path is not this function's business. The caller creates the
    /// directories it is about to write into, which it has to do anyway.
    #[must_use]
    pub fn documents_directory() -> Option<PathBuf> {
        let raw =
            unsafe { SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, None) }.ok()?;
        if raw.is_null() {
            return None;
        }
        let text = unsafe { raw.to_string() }.ok();
        unsafe { CoTaskMemFree(Some(raw.0.cast())) };
        let text = text?;
        if text.is_empty() {
            None
        } else {
            Some(PathBuf::from(text))
        }
    }

    /// Every monospaced family installed on this machine, named and located,
    /// in the order a list draws them.
    ///
    /// # Why DirectWrite and not GDI
    ///
    /// GDI's `EnumFontFamiliesExW` is cheaper and answers half the question: it
    /// hands back family names and a `lfPitchAndFamily` whose low bits say
    /// `FIXED_PITCH`. What it never hands back is *where the family lives*, and
    /// without that the answer cannot be honoured — `bt-render` builds its font
    /// database from a fixed file list on purpose, so a family it has not loaded
    /// is a family its shaper silently falls back out of. DirectWrite answers
    /// name, monospace-ness and file paths in one walk, which is the only shape
    /// that can be fed to a `fontdb`.
    ///
    /// `IDWriteFont1::IsMonospacedFont` is the monospace criterion rather than a
    /// width comparison of two glyphs, because it is the answer the font itself
    /// gives: it reads the face's own `post` table `isFixedPitch` flag and its
    /// OS/2 panose, which is what "this is a programmer's font" means. Measuring
    /// `i` against `M` would additionally admit any proportional face whose two
    /// sampled glyphs happened to tie.
    ///
    /// # What a failure looks like
    ///
    /// A `Vec` and never a `Result`, because there is nothing a caller could do
    /// with the error that this function has not already done: a machine whose
    /// DirectWrite refuses still gets [`DEFAULT_MONOSPACE_FAMILY`] in the list,
    /// which is the face the renderer is already drawing. The picker degrades to
    /// one row rather than to an empty list or a dialog.
    ///
    /// A family whose faces are all in a *custom* loader — a font streamed by an
    /// application rather than installed as a file — is skipped, because
    /// `IDWriteLocalFontFileLoader` is the only loader that can name a path and
    /// a family with no path is a row that cannot be honoured if it is chosen.
    #[must_use]
    pub fn monospace_font_families() -> Vec<super::MonospaceFamily> {
        super::order_monospace_families(collect_monospace_families().unwrap_or_default())
    }

    fn collect_monospace_families() -> windows::core::Result<Vec<super::MonospaceFamily>> {
        let factory: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }?;
        let mut collection: Option<IDWriteFontCollection> = None;
        // `false` — do not ask DirectWrite to re-scan the font directory. The
        // list is being drawn for a human who is about to pick from it, not
        // audited; a rescan is a disk walk this call has no reason to pay for.
        unsafe { factory.GetSystemFontCollection(&mut collection, false) }?;
        let Some(collection) = collection else {
            return Ok(Vec::new());
        };

        let locale = super::os_ui_language();
        let mut families = Vec::new();
        for index in 0..unsafe { collection.GetFontFamilyCount() } {
            let Ok(family) = (unsafe { collection.GetFontFamily(index) }) else {
                continue;
            };
            let mut files: Vec<std::path::PathBuf> = Vec::new();
            let mut monospaced = false;
            for face_index in 0..unsafe { family.GetFontCount() } {
                let Ok(font) = (unsafe { family.GetFont(face_index) }) else {
                    continue;
                };
                // `IDWriteFont1` is the Windows 8 interface. A machine that
                // cannot produce it cannot answer the question, and guessing
                // would put proportional faces in a monospace list.
                let Ok(font1) = font.cast::<IDWriteFont1>() else {
                    continue;
                };
                if !unsafe { font1.IsMonospacedFont() }.as_bool() {
                    continue;
                }
                monospaced = true;
                let Ok(face) = (unsafe { font.CreateFontFace() }) else {
                    continue;
                };
                for path in font_face_files(&face) {
                    if !files.contains(&path) {
                        files.push(path);
                    }
                }
            }
            if !monospaced || files.is_empty() {
                continue;
            }
            let Ok(names) = (unsafe { family.GetFamilyNames() }) else {
                continue;
            };
            if let Some(name) = localized_string(&names, &locale) {
                families.push(super::MonospaceFamily { name, files });
            }
        }
        Ok(families)
    }

    /// The files one face's outlines live in, skipping any the local loader
    /// cannot name.
    fn font_face_files(face: &IDWriteFontFace) -> Vec<std::path::PathBuf> {
        let mut count = 0u32;
        if unsafe { face.GetFiles(&mut count, None) }.is_err() || count == 0 {
            return Vec::new();
        }
        let mut slots: Vec<Option<IDWriteFontFile>> = vec![None; count as usize];
        if unsafe { face.GetFiles(&mut count, Some(slots.as_mut_ptr())) }.is_err() {
            return Vec::new();
        }
        slots
            .into_iter()
            .flatten()
            .filter_map(|file| font_file_path(&file))
            .collect()
    }

    fn font_file_path(file: &IDWriteFontFile) -> Option<std::path::PathBuf> {
        let mut key: *mut c_void = std::ptr::null_mut();
        let mut key_size = 0u32;
        unsafe { file.GetReferenceKey(&mut key, &mut key_size) }.ok()?;
        let loader = unsafe { file.GetLoader() }.ok()?;
        let local = loader.cast::<IDWriteLocalFontFileLoader>().ok()?;
        let length = unsafe { local.GetFilePathLengthFromKey(key, key_size) }.ok()?;
        // `GetFilePathFromKey` writes the terminating NUL, so the buffer is one
        // longer than the reported length and the NUL is trimmed back off.
        let mut buffer = vec![0u16; length as usize + 1];
        unsafe { local.GetFilePathFromKey(key, key_size, &mut buffer) }.ok()?;
        let text: Vec<u16> = buffer.into_iter().take_while(|unit| *unit != 0).collect();
        if text.is_empty() {
            return None;
        }
        Some(std::path::PathBuf::from(String::from_utf16_lossy(&text)))
    }

    /// The family's name in the machine's own UI language, falling back to the
    /// first name it has.
    ///
    /// The fallback is index 0 and not `"en-us"`, because a family may
    /// legitimately have exactly one localized name in a language neither this
    /// user nor English speaks — a Chinese-only face on a Chinese Windows — and
    /// dropping it would hide an installed font from its owner.
    fn localized_string(names: &IDWriteLocalizedStrings, locale: &str) -> Option<String> {
        let mut index = 0u32;
        let mut exists = windows::core::BOOL(0);
        let wide: Vec<u16> = locale.encode_utf16().chain(std::iter::once(0)).collect();
        let _ = unsafe { names.FindLocaleName(PCWSTR(wide.as_ptr()), &mut index, &mut exists) };
        if !exists.as_bool() {
            index = 0;
        }
        let length = unsafe { names.GetStringLength(index) }.ok()?;
        let mut buffer = vec![0u16; length as usize + 1];
        unsafe { names.GetString(index, &mut buffer) }.ok()?;
        let text: Vec<u16> = buffer.into_iter().take_while(|unit| *unit != 0).collect();
        if text.is_empty() {
            None
        } else {
            Some(String::from_utf16_lossy(&text))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{
            CLIPBOARD_OPEN_RETRY_DELAYS, FolderPickerState, ImagePickerState, MathMenuState,
            compositor_failure, names_a_program, primary_language_id, retry_open_clipboard,
            validate_local_image_path, validate_openable_path, wide_null,
        };
        use std::path::{Path, PathBuf};

        /// A DirectComposition refusal has to be readable by the person holding
        /// the machine it refused on, and there is no fallback path to soften it
        /// — the window simply does not open. So the message must carry three
        /// things: which call refused, what Windows called it, and the hex code
        /// the documentation is indexed by.
        ///
        /// MUTATIONS:
        /// ① drop the `{step}` and the message no longer says which of the six
        ///    DComp calls failed — every one of them reads "failed: ...";
        /// ② format the code as `{:X}` of the `i32` and `E_OUTOFMEMORY` prints
        ///    as `-2147024882`'s hex, which is not a string anyone can search.
        #[test]
        fn a_composition_failure_names_the_call_and_carries_the_hresult_both_ways() {
            let error = windows::core::Error::from_hresult(super::HRESULT(0x8007_000E_u32 as i32));
            let message =
                compositor_failure("IDCompositionDesktopDevice::CreateTargetForHwnd", &error);
            assert!(
                message.starts_with("IDCompositionDesktopDevice::CreateTargetForHwnd failed: "),
                "the call that refused leads: {message}"
            );
            assert!(
                message.ends_with("(0x8007000E)"),
                "and the searchable code closes it: {message}"
            );
            assert!(
                message.len()
                    > "IDCompositionDesktopDevice::CreateTargetForHwnd failed:  (0x8007000E)".len(),
                "with Windows' own sentence in between: {message}"
            );
        }

        #[test]
        fn clipboard_open_retry_is_bounded_and_can_recover() {
            let mut attempts = 0;
            let mut waits = Vec::new();
            let result = retry_open_clipboard(
                || {
                    attempts += 1;
                    (attempts == 3)
                        .then_some(())
                        .ok_or_else(|| "clipboard busy".to_owned())
                },
                |delay| waits.push(delay),
            );

            assert_eq!(result, Ok(()));
            assert_eq!(attempts, 3);
            assert_eq!(waits, CLIPBOARD_OPEN_RETRY_DELAYS[..2]);
        }

        #[test]
        fn clipboard_open_retry_reports_the_last_failure_after_its_wait_budget() {
            let mut attempts = 0;
            let mut waits = Vec::new();
            let error = retry_open_clipboard(
                || {
                    attempts += 1;
                    Err(format!("busy-{attempts}"))
                },
                |delay| waits.push(delay),
            )
            .unwrap_err();

            assert_eq!(attempts, CLIPBOARD_OPEN_RETRY_DELAYS.len() + 1);
            assert_eq!(waits, CLIPBOARD_OPEN_RETRY_DELAYS);
            assert!(error.contains("busy-5"));
            assert!(error.contains("75 ms"));
        }

        #[test]
        fn chinese_system_caret_gate_uses_primary_language_bits() {
            assert_eq!(primary_language_id(0x0804), 0x0004);
            assert_eq!(primary_language_id(0x0404), 0x0004);
            assert_ne!(primary_language_id(0x0409), 0x0004);
        }

        #[test]
        fn deferred_menu_state_requires_posted_dispatch_before_showing() {
            let state = MathMenuState::new();
            assert_eq!(state.take_result(), None);
            assert!(state.begin_request(()));
            assert!(!state.begin_request(()));
            assert_eq!(state.take_result(), None);
            assert!(state.begin_showing().is_some());
            assert!(state.begin_showing().is_none());
            state.complete(Ok(true));
            assert_eq!(state.take_result(), Some(Ok(true)));
            assert!(state.begin_request(()));
        }

        /// PIN — the gesture's own arguments survive the deferral, and only the
        /// dispatch that actually shows the dialog receives them.
        ///
        /// The red gate this stands in front of: a folder chooser whose start
        /// folder was read at *show* time rather than at *request* time would
        /// open wherever the column happened to be pointing by then — which,
        /// because the whole point of the deferral is that time passes, is not
        /// necessarily where it was pointing when the row was pressed.
        #[test]
        fn a_deferred_request_carries_its_arguments_to_the_dispatch_that_shows_it() {
            let state = FolderPickerState::new();
            let start: Vec<u16> = "C:\\work\0".encode_utf16().collect();
            assert!(state.begin_request(start.clone()));
            assert!(
                !state.begin_request(Vec::new()),
                "a second ask while one is queued is coalesced, not stacked"
            );
            assert_eq!(state.begin_showing(), Some(start));
            assert_eq!(
                state.begin_showing(),
                None,
                "and the arguments are handed out exactly once"
            );
            state.complete(Ok(Some(PathBuf::from(r"C:\chosen"))));
            assert_eq!(
                state.take_result(),
                Some(Ok(Some(PathBuf::from(r"C:\chosen"))))
            );
            assert_eq!(state.take_result(), None);
        }

        /// PIN — a cancelled chooser and a chooser that could not be shown are
        /// two different answers, and neither is "a folder was chosen".
        #[test]
        fn a_cancelled_chooser_is_not_a_failure_and_not_a_choice() {
            let state = FolderPickerState::new();
            assert!(state.begin_request(Vec::new()));
            assert!(state.begin_showing().is_some());
            state.complete(Ok(None));
            assert_eq!(state.take_result(), Some(Ok(None)));
        }

        #[test]
        fn local_file_bridge_rejects_every_path_outside_the_image_slice() {
            assert!(validate_local_image_path(Path::new(r"C:\tmp\image.png")).is_ok());
            assert!(validate_local_image_path(Path::new("C:/tmp/IMAGE.JPEG")).is_ok());
            assert!(validate_local_image_path(Path::new(r"relative\image.png")).is_err());
            assert!(validate_local_image_path(Path::new(r"\\server\share\image.png")).is_err());
            assert!(validate_local_image_path(Path::new(r"C:\tmp\image.svg")).is_ok());
            assert!(validate_local_image_path(Path::new(r"C:\tmp\image.bmp")).is_err());
            assert!(validate_local_image_path(Path::new("C:\\tmp\\bad\0.png")).is_err());
        }

        /// PIN (§7.1.6c-4b) — the picture chooser's filter and the bridge's
        /// admission gate are one list read twice.
        ///
        /// The failure this forbids is a dialog that shows you a `.bmp`, lets
        /// you double-click it, and is then told by the decoder that it does not
        /// take those — with the dialog already closed and nothing left on
        /// screen connecting the refusal to the choice. Every spelling the
        /// filter offers must pass the gate, and the gate must admit nothing the
        /// filter hides.
        #[test]
        fn the_pictures_filter_offers_exactly_what_the_bridge_admits() {
            let spec = crate::image_file_filter_spec();
            for extension in crate::IMAGE_FILE_EXTENSIONS {
                assert!(
                    spec.contains(&format!("*.{extension}")),
                    "the filter must offer {extension}: {spec}"
                );
                assert!(
                    validate_local_image_path(Path::new(&format!(r"C:\tmp\picture.{extension}")))
                        .is_ok(),
                    "the gate must admit {extension}"
                );
                assert!(
                    validate_local_image_path(Path::new(&format!(
                        r"C:\tmp\picture.{}",
                        extension.to_ascii_uppercase()
                    )))
                    .is_ok(),
                    "a chooser hands back whatever case the file system holds"
                );
            }
            assert_eq!(
                spec.matches('*').count(),
                crate::IMAGE_FILE_EXTENSIONS.len(),
                "one entry per extension and no extras: {spec}"
            );
            assert!(
                !spec.contains("bmp") && validate_local_image_path(Path::new(r"C:\a.bmp")).is_err(),
                "a format the decoder is not built with must be absent from both"
            );
        }

        /// PIN — the picture chooser defers exactly the way the folder chooser
        /// does, and the two do not share a queue.
        ///
        /// The second half is the interesting one: one state per gesture is what
        /// stops a wallpaper request posted while a folder dialog is pending
        /// from being coalesced away and answering the wrong row.
        #[test]
        fn the_picture_chooser_defers_on_its_own_state_and_not_the_folders() {
            let folders = FolderPickerState::new();
            let pictures = ImagePickerState::new();
            let start: Vec<u16> = "C:\\Users\\me\\Pictures\0".encode_utf16().collect();

            assert!(pictures.begin_request(start.clone()));
            assert!(
                folders.begin_request(Vec::new()),
                "a pending picture request must not swallow a folder request"
            );
            assert!(
                !pictures.begin_request(Vec::new()),
                "a second ask for the same gesture is coalesced, not stacked"
            );

            assert_eq!(pictures.begin_showing(), Some(start));
            assert_eq!(pictures.begin_showing(), None);
            pictures.complete(Ok(Some(PathBuf::from(r"C:\Users\me\Pictures\ridge.jpg"))));
            assert_eq!(
                pictures.take_result(),
                Some(Ok(Some(PathBuf::from(r"C:\Users\me\Pictures\ridge.jpg"))))
            );
            assert_eq!(pictures.take_result(), None);
            assert_eq!(
                folders.begin_showing(),
                Some(Vec::new()),
                "and the folder request is still sitting where it was left"
            );
        }

        /// PIN — a NUL-terminated wide copy is what the shell reads, and it is
        /// terminated exactly once.
        #[test]
        fn a_wide_string_for_win32_ends_at_its_terminator() {
            let units = wide_null("*.png;*.jpg");
            assert_eq!(units.last(), Some(&0));
            assert_eq!(
                units.iter().filter(|unit| **unit == 0).count(),
                1,
                "a second NUL would truncate the filter at the first one"
            );
            assert_eq!(
                String::from_utf16_lossy(&units[..units.len() - 1]),
                "*.png;*.jpg"
            );
            assert_eq!(wide_null(""), vec![0]);
        }

        /// PIN — the tree's bridge opens documents and refuses programs, and the
        /// refusal does not depend on this machine's `PATHEXT` being anything in
        /// particular.
        #[test]
        fn the_tree_bridge_opens_what_it_can_show_and_never_what_it_would_run() {
            let empty = "";
            for document in [
                r"C:\notes\readme.md",
                r"C:\notes\report.pdf",
                r"C:\notes\archive.zip",
                r"C:\notes\NOEXTENSION",
                r"C:\notes\photo.PNG",
            ] {
                assert!(
                    !names_a_program(Path::new(document), empty),
                    "{document} is something to look at"
                );
            }
            for program in [
                r"C:\bin\tool.exe",
                r"C:\bin\TOOL.EXE",
                r"C:\bin\run.bat",
                r"C:\bin\install.msi",
                r"C:\bin\shortcut.lnk",
                r"C:\bin\saver.scr",
                r"C:\bin\page.hta",
                r"C:\bin\keys.reg",
                r"C:\bin\script.ps1",
            ] {
                assert!(
                    names_a_program(Path::new(program), empty),
                    "{program} would run"
                );
            }
        }

        /// PIN — a machine that has taught its command line to execute a new
        /// extension has taught this bridge to refuse it.
        #[test]
        fn a_machine_that_makes_something_executable_makes_it_refused_here() {
            let path = Path::new(r"C:\bin\macro.xyz");
            assert!(!names_a_program(path, ".EXE;.BAT"));
            assert!(names_a_program(path, ".EXE;.XYZ"));
            assert!(names_a_program(path, ".exe;.xyz"));
            // An emptied `PATHEXT` cannot open the door the fixed list shuts.
            assert!(names_a_program(Path::new(r"C:\bin\tool.exe"), ""));
        }

        /// A column may be rooted on a share, so its rows have to be openable —
        /// which is the one way this gate is wider than the image bridge's.
        #[test]
        fn the_tree_bridge_takes_a_share_and_still_refuses_a_relative_name() {
            assert!(validate_openable_path(Path::new(r"C:\notes\a.txt")).is_ok());
            assert!(validate_openable_path(Path::new(r"\\server\share\a.txt")).is_ok());
            assert!(validate_openable_path(Path::new(r"notes\a.txt")).is_err());
            assert!(validate_openable_path(Path::new("C:\\notes\\bad\0.txt")).is_err());
        }
    }

    /// **A subscription to one directory tree's change notifications.**
    ///
    /// The kernel's own `ReadDirectoryChangesW`, held open by a thread of its
    /// own: the filesystem tells us when something under `path` moved, and
    /// nothing here ever asks. That distinction is the whole reason this type is
    /// allowed to exist under DESIGN §7.1.3g ② (R31) — a repository is not read
    /// because time passed, and a change notification is not time passing.
    ///
    /// **It says only that something changed.** The `FILE_NOTIFY_INFORMATION`
    /// records the kernel writes are read for nothing at all — not the names, not
    /// the actions — because the caller's next move is to ask `git status`, which
    /// is the one thing that can say what a change *means*. Parsing them here
    /// would be a second, worse answer to a question this crate cannot answer:
    /// whether a write to `target\debug\foo.pdb` matters is a question about a
    /// `.gitignore`, and a watcher that tried to decide it would be wrong on
    /// somebody's repository and silent about it. A zero-length completion —
    /// the kernel's way of saying the buffer overflowed and it has stopped
    /// keeping track — is therefore not a special case but the ordinary one:
    /// *something changed*, which is all any of them ever say.
    ///
    /// **Dropping it cancels.** The watcher thread waits on the directory's
    /// completion and on a stop event at once, so `drop` is a `SetEvent` and a
    /// join rather than a flag the thread notices on its next notification —
    /// which for a directory nothing is writing to would be never.
    pub struct DirWatch {
        dir: SendHandle,
        stop: SendHandle,
        change: SendHandle,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    /// A kernel handle on its way to the thread that will use it.
    ///
    /// A raw `HANDLE` is not `Send` because it is a pointer-shaped value and the
    /// compiler cannot know what it points at. These three are process-wide
    /// kernel objects with exactly one user apiece — the watcher thread — and
    /// [`DirWatch::drop`] joins that thread before closing any of them, so there
    /// is no moment at which two threads hold one of these and no moment at which
    /// a closed handle is still reachable.
    #[derive(Clone, Copy)]
    struct SendHandle(HANDLE);

    // SAFETY: see the type's own note. The handle is created on the calling
    // thread, used only by the watcher thread, and closed by the calling thread
    // after that thread has been joined.
    unsafe impl Send for SendHandle {}

    /// 64 KiB, the largest buffer the kernel will fill for a *network* directory
    /// and a comfortable one for a local tree.
    ///
    /// A bigger buffer buys fewer overflows and nothing else, and an overflow is
    /// not a failure here: it is the same word — *something changed* — arriving
    /// with less detail than usual, and this watcher reads no detail. So the size
    /// is chosen to be unremarkable rather than tuned.
    const DIR_WATCH_BUFFER_BYTES: usize = 64 * 1024;

    /// Everything that can move under a working tree and mean something to git:
    /// files and folders appearing, disappearing or being renamed, contents
    /// written, sizes changing, and attributes (which is how a read-only flag or
    /// a hidden bit arrives).
    ///
    /// Deliberately **not** `LAST_ACCESS`: reading a file changes nothing git can
    /// see, and a grep across the tree would otherwise be a storm of notifications
    /// about nothing.
    const DIR_WATCH_FILTER: FILE_NOTIFY_CHANGE = FILE_NOTIFY_CHANGE(
        FILE_NOTIFY_CHANGE_FILE_NAME.0
            | FILE_NOTIFY_CHANGE_DIR_NAME.0
            | FILE_NOTIFY_CHANGE_LAST_WRITE.0
            | FILE_NOTIFY_CHANGE_SIZE.0
            | FILE_NOTIFY_CHANGE_ATTRIBUTES.0,
    );

    impl DirWatch {
        /// Start watching `path` and everything under it.
        ///
        /// `wake` is called on the watcher thread, once per notification, and is
        /// expected to do nothing but record the news and nudge whatever loop is
        /// going to act on it. Anything slower belongs on the other side of that
        /// nudge: this thread is the only thing standing between the kernel's
        /// buffer and an overflow.
        ///
        /// **Failure is quiet and final.** A path on a network share, a `\\wsl$`
        /// mount, a directory the process may not open — all of them come back as
        /// an error here and the caller's answer is to have no watcher for that
        /// repository, not to try again in a moment. Retrying is a timer, and a
        /// timer is the thing this whole mechanism exists to avoid.
        pub fn start(
            path: &Path,
            wake: impl Fn() + Send + 'static,
        ) -> Result<Self, std::io::Error> {
            let mut units = path.as_os_str().encode_wide().collect::<Vec<u16>>();
            if units.contains(&0) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "watched path contains an embedded NUL",
                ));
            }
            units.push(0);
            // `FILE_LIST_DIRECTORY` is the access right `ReadDirectoryChangesW`
            // needs, `BACKUP_SEMANTICS` is what lets `CreateFileW` open a
            // directory at all, and `OVERLAPPED` is what lets the read be
            // cancelled — without it the thread would block in the kernel with
            // no way out but a change that may never come.
            //
            // All three shares are granted because this handle must not be the
            // reason somebody else cannot rename or delete a file in their own
            // working tree.
            let dir = unsafe {
                CreateFileW(
                    PCWSTR(units.as_ptr()),
                    FILE_LIST_DIRECTORY.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                    None,
                )
            }
            .map_err(win32_io_error)?;
            // Manual-reset, both of them. The stop event has to stay signalled
            // once it is set — the thread may be anywhere between two waits when
            // `drop` fires — and the completion event is reset by hand before
            // each read so that a stale signal cannot be mistaken for an answer
            // to the read that has not been issued yet.
            let change = match unsafe { CreateEventW(None, true, false, PCWSTR::null()) } {
                Ok(handle) => handle,
                Err(error) => {
                    unsafe { close(dir) };
                    return Err(win32_io_error(error));
                }
            };
            let stop = match unsafe { CreateEventW(None, true, false, PCWSTR::null()) } {
                Ok(handle) => handle,
                Err(error) => {
                    unsafe { close(dir) };
                    unsafe { close(change) };
                    return Err(win32_io_error(error));
                }
            };
            let (dir, change, stop) = (SendHandle(dir), SendHandle(change), SendHandle(stop));
            let thread = std::thread::Builder::new()
                .name("bt-dir-watch".to_owned())
                .spawn(move || watch_loop(dir, change, stop, wake));
            match thread {
                Ok(thread) => Ok(Self {
                    dir,
                    stop,
                    change,
                    thread: Some(thread),
                }),
                Err(error) => {
                    unsafe { close(dir.0) };
                    unsafe { close(change.0) };
                    unsafe { close(stop.0) };
                    Err(error)
                }
            }
        }
    }

    impl Drop for DirWatch {
        fn drop(&mut self) {
            // The thread cancels its own read: the stop event is one of the two
            // things it is waiting on, so setting it is enough, and a
            // `CancelIoEx` from here would be a second thread reaching into an
            // operation whose `OVERLAPPED` lives on that thread's stack.
            unsafe {
                let _ = SetEvent(self.stop.0);
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
            // Only now: a handle closed while the thread still held it would be a
            // handle reused for something else by the time it got round to using
            // it, which is the one Win32 bug that reads as a different subsystem
            // misbehaving.
            unsafe {
                close(self.dir.0);
                close(self.change.0);
                close(self.stop.0);
            }
        }
    }

    /// The watcher thread: issue a read, wait for it or for the stop, repeat.
    fn watch_loop(dir: SendHandle, change: SendHandle, stop: SendHandle, wake: impl Fn()) {
        // `u32` and not `u8`: the kernel writes `FILE_NOTIFY_INFORMATION` records
        // into this and requires DWORD alignment, which a `Vec<u8>` does not
        // promise. Nothing reads the records — see [`DirWatch`] — but the
        // alignment is a precondition of the call, not of the parsing.
        let mut buffer = vec![0u32; DIR_WATCH_BUFFER_BYTES / std::mem::size_of::<u32>()];
        // Declared once, outside the loop, so that its address is fixed for as
        // long as the kernel may be writing to it — and written afresh at the top
        // of every pass, because the kernel leaves its own status in the fields a
        // second read would otherwise inherit. The read is always awaited or
        // cancelled before the next iteration reuses it.
        let mut overlapped;
        loop {
            overlapped = OVERLAPPED {
                hEvent: change.0,
                ..OVERLAPPED::default()
            };
            unsafe {
                if ResetEvent(change.0).is_err() {
                    return;
                }
            }
            let issued = unsafe {
                ReadDirectoryChangesW(
                    dir.0,
                    buffer.as_mut_ptr().cast::<std::ffi::c_void>(),
                    u32::try_from(DIR_WATCH_BUFFER_BYTES).unwrap_or(u32::MAX),
                    // Recursively. A repository is a tree and a commit touches
                    // any depth of it.
                    true,
                    DIR_WATCH_FILTER,
                    None,
                    Some(std::ptr::from_mut(&mut overlapped)),
                    None,
                )
            };
            if issued.is_err() {
                // The directory went away, or the handle did. There is nothing
                // left to watch and nothing to report: the caller keeps whatever
                // it last knew, and the page's own refresh is still there.
                return;
            }
            let signalled = unsafe { WaitForMultipleObjects(&[stop.0, change.0], false, INFINITE) };
            if signalled != WAIT_EVENT(WAIT_OBJECT_0.0 + 1) {
                // Stopped, or the wait itself failed. Either way this thread is
                // finished — but the read it issued is still outstanding, and the
                // `OVERLAPPED` it is writing into is about to go out of scope, so
                // it is cancelled and *waited for* before that happens.
                unsafe {
                    let _ = CancelIoEx(dir.0, Some(std::ptr::from_ref(&overlapped)));
                    let mut ignored = 0u32;
                    let _ = GetOverlappedResult(dir.0, &overlapped, &mut ignored, true);
                }
                return;
            }
            let mut written = 0u32;
            let completed = unsafe { GetOverlappedResult(dir.0, &overlapped, &mut written, false) };
            if completed.is_err() {
                return;
            }
            // `written == 0` is the kernel saying the buffer overflowed and it
            // has stopped keeping track of what changed. It is reported exactly
            // like every other notification, because it carries exactly the same
            // information this watcher uses: something changed.
            wake();
        }
    }

    /// Close a handle, ignoring the failure that can only mean it was not one.
    unsafe fn close(handle: HANDLE) {
        unsafe {
            let _ = CloseHandle(handle);
        }
    }

    fn win32_io_error(error: windows::core::Error) -> std::io::Error {
        std::io::Error::from_raw_os_error(error.code().0)
    }

    /// Put the calling thread in one of the three bands. See [`ThreadPriority`].
    ///
    /// The answer is whether the kernel took it. There is exactly one way this
    /// fails in practice — a job object or a policy that caps the process's
    /// priority — and the honest response to that is to go on running at
    /// whatever the machine allows, because a terminal that refused to start
    /// because it could not be one step more important than a background thread
    /// would be a worse program than a slightly slower one.
    pub fn set_current_thread_priority(priority: ThreadPriority) -> bool {
        // `GetCurrentThread` is a pseudo-handle — a constant meaning "me" — so
        // there is nothing to close and nothing that can outlive the call.
        unsafe { SetThreadPriority(GetCurrentThread(), win32_priority(priority)) }.is_ok()
    }

    /// Which band the calling thread is in, or `None` if it is in none of them.
    ///
    /// Exists so the spawn helper's contract can be *tested* rather than
    /// asserted in a comment: a worker is a thread that reports
    /// [`ThreadPriority::BelowNormal`] from inside itself.
    #[must_use]
    pub fn current_thread_priority() -> Option<ThreadPriority> {
        let raw = unsafe { GetThreadPriority(GetCurrentThread()) };
        match THREAD_PRIORITY(raw) {
            THREAD_PRIORITY_ABOVE_NORMAL => Some(ThreadPriority::AboveNormal),
            THREAD_PRIORITY_NORMAL => Some(ThreadPriority::Normal),
            THREAD_PRIORITY_BELOW_NORMAL => Some(ThreadPriority::BelowNormal),
            _ => None,
        }
    }

    fn win32_priority(priority: ThreadPriority) -> THREAD_PRIORITY {
        match priority {
            ThreadPriority::AboveNormal => THREAD_PRIORITY_ABOVE_NORMAL,
            ThreadPriority::Normal => THREAD_PRIORITY_NORMAL,
            ThreadPriority::BelowNormal => THREAD_PRIORITY_BELOW_NORMAL,
        }
    }

    /// Spawn a named thread that is already in its band before it does anything.
    ///
    /// **The band is set from inside the new thread, not from the spawner**, and
    /// that is the whole reason this helper exists rather than a
    /// `set_priority(&handle)` called after `spawn`: between a `spawn` and a
    /// call on its `JoinHandle` the new thread is already running, and under the
    /// exact saturation this is for, "already running" can mean "has already
    /// decoded the image" — a worker that spends its first and busiest
    /// milliseconds at the frame's priority. Here the first statement the thread
    /// executes is the one that gets out of the frame's way.
    pub fn spawn_at_priority<T: Send + 'static>(
        name: &str,
        priority: ThreadPriority,
        body: impl FnOnce() -> T + Send + 'static,
    ) -> std::io::Result<std::thread::JoinHandle<T>> {
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                set_current_thread_priority(priority);
                body()
            })
    }

    /// Put one block of text where the person who started this process will see
    /// it, and say whether there was anywhere to put it.
    ///
    /// **The one thing a Windows GUI program cannot do is print.** `folio.exe`
    /// answers `--help` and refuses a mistyped flag, and both answers are text —
    /// but a process launched from Explorer, from a shortcut or by a shell that
    /// does not wait for it has no console of its own to write to, and `println!`
    /// on such a process writes to a handle nobody is reading. `bt-app`'s front
    /// door therefore asks *this*, and falls back to [`message_box`] when the
    /// answer is `false`.
    ///
    /// Two steps, in this order:
    ///
    /// 1. **A console this process already has** — which today it always does,
    ///    because `folio.exe` is still a console-subsystem binary. `GetConsoleWindow`
    ///    is the test, and a non-null answer means the second step must be
    ///    skipped: `AttachConsole` fails with `ERROR_ACCESS_DENIED` on a process
    ///    that is already attached, and reading that as "no console" would send
    ///    a usage block to a message box on the one machine configuration where
    ///    the console was right there.
    /// 2. **The parent's console**, through `ATTACH_PARENT_PROCESS`. This is the
    ///    half written for the day the binary becomes a windows-subsystem one —
    ///    which the Explorer verb wants, since a right-click that flashes a
    ///    console window is a right-click that looks broken. It fails, correctly,
    ///    when the parent has no console either: a double-click from Explorer,
    ///    or a launch by the shell's COM activation.
    ///
    /// The text goes to `CONOUT$` rather than to `std::io::stdout`, and that is
    /// not belt-and-braces. `AttachConsole` gives the process a console; it does
    /// not reliably rewrite the three standard handles the C runtime and Rust's
    /// `Stdout` resolve through, and on a process that was started with its
    /// output redirected it must not — the redirection is the caller's. Opening
    /// the console's own pseudo-file names exactly one destination: the screen
    /// the console is drawn on.
    ///
    /// `WriteConsoleW` and not `WriteFile`, so that the UTF-16 goes to the
    /// console as characters. A console's code page is very often 936 or 437 on
    /// the machines this product runs on, and a byte-oriented write would put
    /// mojibake on the screen for the half of this text that is Chinese.
    /// Give a windows-subsystem process its parent's console for its standard
    /// handles, when it has none of its own.
    ///
    /// `#![windows_subsystem = "windows"]` (user report 2026-08-18: launching
    /// `folio.exe` raised a Windows Terminal window first) means the loader no
    /// longer conjures a console — which also means a developer running
    /// `folio.exe` from a shell with `BT_STARTUP_TRACE` set would watch nothing
    /// arrive: the process starts with null standard handles and Rust's `print`
    /// family quietly discards into them. This adopts the parent's console and
    /// points the null handles at its screen, so a trace asked for from a shell
    /// still lands in that shell.
    ///
    /// **A redirection is never touched.** Only a handle that is null is
    /// replaced; `folio.exe 2>trace.txt` keeps its file, because the caller who
    /// redirected owns where that stream goes. And a double-click stays silent:
    /// with no parent console `AttachConsole` fails and this is a no-op.
    pub fn adopt_parent_console() {
        use windows::Win32::System::Console::{
            GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
        };
        let null = |slot| {
            unsafe { GetStdHandle(slot) }
                .map(|handle| handle.is_invalid())
                .unwrap_or(true)
        };
        if !null(STD_OUTPUT_HANDLE) && !null(STD_ERROR_HANDLE) {
            return;
        }
        if unsafe { GetConsoleWindow() }.is_invalid()
            && unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_err()
        {
            return;
        }
        let name: Vec<u16> = "CONOUT$ ".encode_utf16().collect();
        let Ok(conout) = (unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        }) else {
            return;
        };
        for slot in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            if null(slot) {
                let _ = unsafe { SetStdHandle(slot, HANDLE(conout.0)) };
            }
        }
    }

    pub fn write_to_console(text: &str) -> bool {
        let attached = unsafe { !GetConsoleWindow().is_invalid() }
            || unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_ok();
        if !attached {
            return false;
        }
        let name: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
        let Ok(handle) = (unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        }) else {
            return false;
        };
        // A console line ends in CRLF. `ENABLE_PROCESSED_OUTPUT` will usually
        // turn a bare LF into one, but "usually" is a mode the caller of this
        // process can have turned off, and the failure it produces is a usage
        // block drawn as a staircase down the right of the screen.
        let units: Vec<u16> = text
            .replace("\r\n", "\n")
            .replace('\n', "\r\n")
            .encode_utf16()
            .collect();
        let mut written = 0u32;
        let wrote = unsafe { WriteConsoleW(handle, &units, Some(&mut written), None) }.is_ok();
        let _ = unsafe { CloseHandle(handle) };
        wrote
    }

    /// Say one thing in a box, with no window behind it.
    ///
    /// **The single message box this product is allowed to raise**, and the
    /// reason it is allowed is the reason every other one is not: there is no
    /// window yet. Everything Folio says once it has a window it says on a card
    /// anchored at the surface the news is about (`Runtime::toast`); a modal is
    /// how a program interrupts you, and a program that has drawn nothing has
    /// nothing to interrupt.
    ///
    /// `HWND::default()` is the ownerless box: the process has no window to be
    /// modal to, which is the whole situation. `MB_SETFOREGROUND` because the
    /// caller is a shell or Explorer that has just taken the focus back — a box
    /// nobody sees is the same as no box, and this one carries the only
    /// explanation of a launch that is about to exit.
    pub fn message_box(title: &str, text: &str) {
        let title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            MessageBoxW(
                Some(HWND::default()),
                PCWSTR(text.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND,
            );
        }
    }
}

/// The three scheduling bands this application's threads run in.
///
/// **A terminal under a saturated machine is a scheduling problem, not a
/// throughput problem.** Every core busy with `cargo` means the window's own
/// loop is one runnable thread among a hundred at the same priority, and the
/// scheduler's answer is a fair share — which for a thread that must answer a
/// wheel notch inside a frame is not the right answer at all. The window is not
/// asking for more of the machine; it is asking to be the first of this
/// process's threads to get whatever the machine hands it.
///
/// So the process is *ordered*, in three bands and no more:
///
/// * [`Self::AboveNormal`] — the one thread that owns the window: the event
///   loop, which is also the render thread. One step, not two: `+1` is enough to
///   put the loop ahead of every ordinary background thread on the machine, and
///   it leaves the priority classes above it (`HIGHEST`, `TIME_CRITICAL`) to the
///   things that genuinely cannot be late.
/// * [`Self::Normal`] — the PTY readers. A reader that falls behind is a child
///   process blocked on a full pipe, which is back-pressure reaching the wrong
///   place; it is not the frame's competitor either, because it does nothing but
///   move bytes into a ring.
/// * [`Self::BelowNormal`] — every worker. A `git status`, a directory read, a
///   PNG decode and a formula raster are all answers to questions nobody is
///   holding their breath for, and none of them may ever be the reason a frame
///   was late. This is the band that matters most under starvation: it is what
///   stops the process from competing with itself.
///
/// **Not MMCSS.** `AvSetMmThreadCharacteristicsW("Window Manager")` was
/// considered and declined. It pulls in `avrt.dll` and a revert that has to be
/// paired with it on every exit path; it lands the thread in the multimedia
/// scheduling class, whose boost is far larger than one step and would let the
/// loop starve *this process's own* PTY readers and workers under exactly the
/// saturation it is meant to survive; and MMCSS brings its own throttle — the
/// registry's `SystemResponsiveness` reserves a slice of every period for
/// non-multimedia work — so it can add latency as easily as remove it. One step
/// of ordinary thread priority is the smallest change that answers the actual
/// complaint, and it is a change this process can make about itself without
/// making a claim about the machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadPriority {
    AboveNormal,
    Normal,
    BelowNormal,
}

#[cfg(windows)]
pub use windows_impl::{
    Compositor, CustomWindowFrame, DirWatch, FolderPicker, ImagePicker, ImeSystemCaret,
    MathContextMenu, PROGRAM_REFUSED, adopt_parent_console, client_area_animation_enabled,
    clipboard_text, current_thread_priority, documents_directory, get_dpi_for_window,
    get_window_rect, get_work_area, install_window_class_background, is_window_minimized,
    message_box, monospace_font_families, open_local_file, open_local_path, os_ui_language,
    recycle, request_window_close, reveal_in_explorer, set_clipboard_text,
    set_current_thread_priority, set_system_backdrop, set_window_dark_mode, set_window_outer_rect,
    set_window_topmost, shell_execute, spawn_at_priority, system_backdrop_available,
    wheel_scroll_amount, write_to_console,
};

/// The bands, asked of the kernel rather than of the source.
///
/// These are here and not in `bt-app` because what is being pinned is the
/// *syscall's* effect: that a thread started through [`spawn_at_priority`] is
/// already out of the frame's way by the time its body runs, and that the band
/// a caller names is the band the thread is actually in. Which crate spawns
/// which worker is a separate question, tested where those spawns live.
#[cfg(all(test, windows))]
mod thread_priority_tests {
    use super::{ThreadPriority, current_thread_priority, set_current_thread_priority};

    /// PIN — a worker reports its band from *inside itself*.
    ///
    /// The bug this forecloses is the one the helper exists to prevent: setting
    /// the priority on the `JoinHandle` after `spawn`, which leaves the thread's
    /// first and busiest milliseconds — the decode, the `git status`, the
    /// directory walk — running at the frame's priority. Asking the thread
    /// itself, as its first act, is the only question whose answer distinguishes
    /// the two.
    #[test]
    fn a_worker_is_already_below_normal_when_its_body_starts() {
        let thread =
            super::spawn_at_priority("bt-test-worker", ThreadPriority::BelowNormal, || {
                current_thread_priority()
            })
            .expect("spawn a worker");
        assert_eq!(
            thread.join().expect("join the worker"),
            Some(ThreadPriority::BelowNormal),
            "a worker must be out of the frame's way before it does anything"
        );
    }

    /// PIN — the three bands are three different answers, and each round-trips.
    ///
    /// A mapping that collapsed two of them would be a process that believes it
    /// is ordered and is not, and nothing else in the tree would notice.
    #[test]
    fn every_band_round_trips_through_the_kernel() {
        for band in [
            ThreadPriority::AboveNormal,
            ThreadPriority::Normal,
            ThreadPriority::BelowNormal,
        ] {
            let thread = super::spawn_at_priority("bt-test-band", band, current_thread_priority)
                .expect("spawn a thread in a band");
            assert_eq!(
                thread.join().expect("join the thread"),
                Some(band),
                "{band:?} did not survive the round trip"
            );
        }
    }

    /// PIN — the loop's own thread can raise itself, which is the half of the
    /// design that has no spawn to hang off: `main` is handed its thread by the
    /// runtime and has to ask for the band in place.
    #[test]
    fn a_thread_can_raise_itself_in_place() {
        let raised = std::thread::spawn(|| {
            let taken = set_current_thread_priority(ThreadPriority::AboveNormal);
            (taken, current_thread_priority())
        })
        .join()
        .expect("join the raised thread");
        assert_eq!(raised, (true, Some(ThreadPriority::AboveNormal)));
    }
}

/// The one test in this crate that talks to the kernel about a real directory.
///
/// It is here rather than in `bt-app` because the thing under test is the
/// subscription itself: that `ReadDirectoryChangesW` reaches a callback at all,
/// and that dropping the handle stops it. Everything above this — when a change
/// becomes a re-read, and of what — is arithmetic and is tested where it lives
/// (`bt_app::git_watch`).
#[cfg(all(test, windows))]
mod dir_watch_tests {
    use super::DirWatch;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    /// Generous, because this is a claim about *eventually* and the machine
    /// running it may be building something. A notification for a local
    /// directory arrives in single-digit milliseconds; five seconds is the
    /// difference between "slow" and "never", which is the only difference this
    /// test is about.
    const ARRIVES_WITHIN: Duration = Duration::from_secs(5);
    /// And the other way round, where the claim is "nothing at all": long enough
    /// that a notification which was going to come would have.
    const SILENCE_FOR: Duration = Duration::from_millis(600);

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "bt-dir-watch-{}-{name}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("make a scratch directory");
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// PIN — **a file appearing under a watched tree reaches the callback, and a
    /// dropped watch stops reaching it.**
    ///
    /// The two halves are one test because the second is only meaningful after
    /// the first: "no events arrived" is what a watcher that never worked also
    /// reports. Proving the wake-up first is what makes the silence afterwards
    /// evidence of a cancellation rather than of a mistake in the setup.
    #[test]
    fn a_watched_directory_reports_a_change_and_stops_when_dropped() {
        let scratch = Scratch::new("basic");
        let (tx, rx) = mpsc::channel::<()>();
        let watch = DirWatch::start(&scratch.0, move || {
            let _ = tx.send(());
        })
        .expect("watch a directory this process just made");

        std::fs::write(scratch.0.join("appeared.txt"), b"hello").expect("write a file");
        rx.recv_timeout(ARRIVES_WITHIN)
            .expect("the kernel reports a file appearing under a watched tree");

        // Depth: a repository is a tree, and a commit touches any depth of it.
        while rx.try_recv().is_ok() {}
        std::fs::create_dir_all(scratch.0.join("a").join("b")).expect("make a subtree");
        std::fs::write(scratch.0.join("a").join("b").join("deep.txt"), b"hi").expect("write deep");
        rx.recv_timeout(ARRIVES_WITHIN)
            .expect("and reports one several directories down: the watch is recursive");

        drop(watch);
        // Drain whatever was already in flight when the watch was dropped — the
        // claim is about what happens *after* the cancellation, not about a
        // notification that had already been posted.
        while rx.try_recv().is_ok() {}
        std::fs::write(scratch.0.join("after.txt"), b"and this").expect("write after the drop");
        let deadline = Instant::now() + SILENCE_FOR;
        while Instant::now() < deadline {
            assert!(
                rx.try_recv().is_err(),
                "a dropped watch has stopped: nothing written afterwards reaches it"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// PIN — **a path that cannot be watched fails quietly and finally.**
    ///
    /// The caller's answer to this is to have no watcher for that repository,
    /// which is a state the rest of the machinery is built to live in: the
    /// window-focus trigger and the page's own refresh button are what cover a
    /// network share. What it must never be is an error a user is shown or a
    /// thing that is retried, because retrying on a schedule is the polling this
    /// whole mechanism exists to avoid.
    #[test]
    fn a_directory_that_is_not_there_declines_to_be_watched() {
        let missing = std::env::temp_dir().join("bt-dir-watch-no-such-directory-ever");
        let _ = std::fs::remove_dir_all(&missing);
        assert!(
            DirWatch::start(&missing, || {}).is_err(),
            "nothing to subscribe to, and no thread left running to say so later"
        );
    }
}

#[cfg(test)]
mod monospace_family_tests {
    use super::{DEFAULT_MONOSPACE_FAMILY, MonospaceFamily, order_monospace_families};

    fn named(name: &str) -> MonospaceFamily {
        MonospaceFamily {
            name: name.to_owned(),
            files: vec![std::path::PathBuf::from(format!(
                "C:\\Windows\\Fonts\\{name}.ttf"
            ))],
        }
    }

    fn names(families: &[MonospaceFamily]) -> Vec<&str> {
        families.iter().map(|f| f.name.as_str()).collect()
    }

    /// PIN — the list is sorted case-insensitively, so the row a user is looking
    /// for is where the alphabet says it is.
    ///
    /// DirectWrite hands families back in the font collection's internal order,
    /// which is neither alphabetical nor stable: it changes when a font is
    /// installed and differs between two machines with the same fonts. A picker
    /// whose rows move between launches is a picker nobody can use twice.
    ///
    /// MUTATIONS: ① sort by `name` directly and `MS Gothic` sorts before
    /// `Consolas`, because every uppercase letter sorts before every lowercase
    /// one; ② drop the sort and the order is whatever the machine said.
    #[test]
    fn the_family_list_is_alphabetical_without_regard_to_case() {
        let ordered = order_monospace_families(vec![
            named("MS Gothic"),
            named("consolas"),
            named("Cascadia Mono"),
            named("Lucida Console"),
        ]);
        assert_eq!(
            names(&ordered),
            vec!["Cascadia Mono", "consolas", "Lucida Console", "MS Gothic"]
        );
    }

    /// PIN — the default face is in the list even when the machine did not
    /// report it, and it lands in alphabetical position rather than on the end.
    ///
    /// The failure this guards is specific and silent: `settings.json` may hold
    /// no family at all, in which case the picker's selected row is the default
    /// one. If the enumeration came back without it — DirectWrite refused, or
    /// this Windows genuinely lacks Consolas — the combo would show a selected
    /// row that is not in its own list, which draws as a blank.
    #[test]
    fn the_default_face_is_always_a_row_and_sits_where_the_alphabet_puts_it() {
        let ordered = order_monospace_families(vec![named("Cascadia Mono"), named("MS Gothic")]);
        assert_eq!(
            names(&ordered),
            vec!["Cascadia Mono", DEFAULT_MONOSPACE_FAMILY, "MS Gothic"]
        );
        assert!(
            ordered[1].files.is_empty(),
            "a default the machine did not report needs no loading — the renderer \
             already has it from its fixed startup list, and an invented path \
             would be a file that does not exist"
        );
        assert_eq!(
            names(&order_monospace_families(Vec::new())),
            vec![DEFAULT_MONOSPACE_FAMILY],
            "a machine whose DirectWrite refused outright still gets one row"
        );
    }

    /// PIN — a machine that reports the default itself keeps its own files, and
    /// the inserted entry does not appear beside it.
    ///
    /// The bug this shape catches is inserting unconditionally: two `Consolas`
    /// rows, one of which cannot be loaded, is worse than either alternative.
    #[test]
    fn a_reported_default_keeps_its_files_and_is_not_doubled() {
        let ordered = order_monospace_families(vec![named("Consolas"), named("Cascadia Mono")]);
        assert_eq!(names(&ordered), vec!["Cascadia Mono", "Consolas"]);
        assert!(
            !ordered[1].files.is_empty(),
            "the machine's own files survive"
        );
    }

    /// PIN — a family reported twice becomes one row.
    ///
    /// DirectWrite will not normally repeat a family, but the localized-name
    /// walk can land two collection entries on one string — a family whose
    /// English and native names coincide — and a list with the same name twice
    /// gives a combo two rows that select differently while reading identically.
    #[test]
    fn a_family_reported_twice_is_one_row() {
        let ordered = order_monospace_families(vec![
            named("Consolas"),
            named("consolas"),
            named("Fira Code"),
        ]);
        assert_eq!(
            names(&ordered),
            vec!["Consolas", "Fira Code"],
            "which of the two spellings survives matters less than that the same              one survives every time — the tie is broken by exact bytes, so the              answer cannot depend on the order the machine reported them in"
        );
    }
}

/// The enumeration against the real font collection of the machine the tests run
/// on. Windows only, because there is nothing to enumerate elsewhere.
#[cfg(all(test, windows))]
mod monospace_enumeration_tests {
    use super::{DEFAULT_MONOSPACE_FAMILY, monospace_font_families};

    /// PIN — a real Windows answers with families that can actually be loaded.
    ///
    /// Deliberately not an assertion about *which* families: the machine running
    /// the test decides that, and pinning "Cascadia Mono is present" would fail
    /// on a Windows 10 without it. What is pinned is the contract every caller
    /// depends on — a name that is not empty, at least one file per row, files
    /// that exist on disk, and the default among them — because a row failing
    /// any of those is a picker row that silently does nothing when chosen.
    #[test]
    fn the_machines_own_monospaced_families_are_named_and_locatable() {
        let families = monospace_font_families();
        assert!(
            families
                .iter()
                .any(|family| family.name.eq_ignore_ascii_case(DEFAULT_MONOSPACE_FAMILY)),
            "the default face is promised to be a row on every machine"
        );
        for family in &families {
            assert!(!family.name.trim().is_empty(), "a row must have a name");
            if family.name.eq_ignore_ascii_case(DEFAULT_MONOSPACE_FAMILY) && family.files.is_empty()
            {
                // The inserted default — see `order_monospace_families`.
                continue;
            }
            assert!(
                !family.files.is_empty(),
                "{} came back with no file, so choosing it could not be honoured",
                family.name
            );
            for path in &family.files {
                assert!(
                    path.is_absolute(),
                    "{} names {path:?}, which is not a path a loader can open",
                    family.name
                );
            }
        }
    }
}

#[cfg(test)]
mod composition_offset_tests {
    use super::composition_visual_offset;

    /// PIN (WebView2 spike Q4) — **a visual's offset is in physical pixels and
    /// nothing multiplies it again.**
    ///
    /// The spike measured this on a 144-DPI monitor: the `DeviceRect` the layout
    /// produced, handed over unchanged, landed on the pixel. The failure this
    /// pins is the plausible one — someone reads "offset" and "DPI" in the same
    /// paragraph and scales by `scale_factor` on the way through, which is
    /// correct-looking at 96 DPI and puts the whole picture at double the offset
    /// on the machine this product is developed on.
    ///
    /// MUTATIONS:
    /// ① multiply by any constant other than one and the first two assertions
    ///    go red at once;
    /// ② round or clamp the input and the negative case goes red — a visual left
    ///    of its parent's origin is an ordinary thing to ask for.
    #[test]
    fn a_visual_offset_is_the_physical_pixel_it_was_handed() {
        assert_eq!(composition_visual_offset(0, 0), (0.0, 0.0));
        assert_eq!(composition_visual_offset(786, 710), (786.0, 710.0));
        assert_eq!(composition_visual_offset(-3780, -160), (-3780.0, -160.0));
    }

    /// And the cast is exact across every coordinate a desktop can name, which
    /// is what makes the `i32` -> `f32` narrowing a rename rather than a
    /// rounding. `f32` is exact to 2^24; the largest virtual desktop anyone can
    /// assemble is three orders of magnitude short of that.
    #[test]
    fn the_cast_to_the_compositors_float_loses_nothing_a_desktop_can_produce() {
        for pixel in [-131_072, -65_536, -1, 1, 65_536, 131_072] {
            let (x, y) = composition_visual_offset(pixel, pixel);
            assert_eq!(x as i32, pixel, "x survived the round trip");
            assert_eq!(y as i32, pixel, "y survived the round trip");
        }
    }
}

#[cfg(test)]
mod reveal_tests {
    use super::*;
    use std::path::Path;

    /// PIN (user ruling, 2026-08-13) — **a foot reveals the thing it names**,
    /// which for a file means Explorer opens with that file highlighted.
    ///
    /// The old answer was to open the containing folder, and against a folder
    /// of two hundred rows that is not an answer: the path the foot was
    /// printing arrived indistinguishable from everything beside it. `/select`
    /// is the verb that means what the foot says.
    ///
    /// The quoting is the whole of what can be wrong here. Explorer parses
    /// `/select,<path>` as one token and wants the path quoted; a command line
    /// one quote out opens `Documents` and reports success, which is the worst
    /// shape a failure can take.
    ///
    /// MUTATIONS:
    /// ① go back to a bare folder open — drop `/select,` — and the first
    ///    assertion goes red, which is the reported behaviour written down;
    /// ② drop the quotes and the third goes red: every path with a space in it
    ///    silently reveals the wrong thing.
    #[test]
    fn a_file_is_revealed_by_selecting_it_and_a_folder_by_opening_it() {
        let file = Path::new(r"C:\repo\test-assets\preview-samples\stress.md");
        let arguments = reveal_arguments(file, false);
        let text = arguments.to_string_lossy();
        assert!(
            text.starts_with("/select,"),
            "a file is highlighted where it lives, not merely surrounded: {text}"
        );
        assert!(
            text.contains(&*file.to_string_lossy()),
            "and it is that file that is named: {text}"
        );
        assert_eq!(
            text,
            format!("/select,\"{}\"", file.display()),
            "as one quoted token, which is the only form Explorer parses"
        );

        // A directory is opened rather than selected: `/select` on a folder
        // opens its *parent*, one level further out than a root is offering.
        let folder = Path::new(r"C:\repo\test-assets\preview-samples");
        let opened = reveal_arguments(folder, true);
        let text = opened.to_string_lossy();
        assert!(
            !text.contains("/select"),
            "a tree's own root is a place to look inside: {text}"
        );
        assert_eq!(text, format!("\"{}\"", folder.display()));

        // A path with a space survives, which is what the quotes are for.
        let spaced = Path::new(r"C:\My Documents\a file.md");
        assert_eq!(
            reveal_arguments(spaced, false).to_string_lossy(),
            "/select,\"C:\\My Documents\\a file.md\""
        );
    }
}

#[cfg(test)]
mod custom_frame_tests {
    use super::*;

    fn metrics(dpi: u32) -> CustomFrameMetrics {
        CustomFrameMetrics {
            width: logical_px_for_dpi(960, dpi),
            height: logical_px_for_dpi(600, dpi),
            title_bar_height: logical_px_for_dpi(40, dpi),
            tab_strip_right_px: logical_px_for_dpi(240, dpi),
            caption_button_width: logical_px_for_dpi(46, dpi),
            caption_button_count: 4,
            resize_border: logical_px_for_dpi(8, dpi),
            resizable: true,
        }
    }

    /// Red gate: every region returned to Windows is pinned, including corners
    /// (which must win over caption), the drag band and the app-owned buttons.
    #[test]
    fn custom_frame_hit_test_maps_drag_resize_edges_and_caption_buttons() {
        let m = metrics(96);
        assert_eq!(custom_frame_hit_test(m, 0, 0), CustomFrameHit::TopLeft);
        assert_eq!(custom_frame_hit_test(m, 959, 0), CustomFrameHit::TopRight);
        assert_eq!(custom_frame_hit_test(m, 0, 599), CustomFrameHit::BottomLeft);
        assert_eq!(
            custom_frame_hit_test(m, 959, 599),
            CustomFrameHit::BottomRight
        );
        assert_eq!(custom_frame_hit_test(m, 0, 300), CustomFrameHit::Left);
        assert_eq!(custom_frame_hit_test(m, 959, 300), CustomFrameHit::Right);
        assert_eq!(custom_frame_hit_test(m, 400, 0), CustomFrameHit::Top);
        assert_eq!(custom_frame_hit_test(m, 400, 599), CustomFrameHit::Bottom);
        assert_eq!(custom_frame_hit_test(m, 100, 20), CustomFrameHit::Client);
        assert_eq!(custom_frame_hit_test(m, 300, 20), CustomFrameHit::Caption);
        assert_eq!(custom_frame_hit_test(m, 800, 20), CustomFrameHit::Client);
        assert_eq!(custom_frame_hit_test(m, 300, 60), CustomFrameHit::Client);
    }

    /// DPI changes scale both the logical title/button geometry and the resize
    /// band; testing equivalent logical points catches a hard-coded 96-DPI map.
    #[test]
    fn custom_frame_hit_test_is_dpi_scaled() {
        for dpi in [96, 120, 144, 168, 192, 240] {
            let m = metrics(dpi);
            assert_eq!(
                custom_frame_hit_test(m, logical_px_for_dpi(120, dpi), logical_px_for_dpi(20, dpi),),
                CustomFrameHit::Client,
                "tab strip at {dpi} DPI"
            );
            assert_eq!(
                custom_frame_hit_test(m, logical_px_for_dpi(300, dpi), logical_px_for_dpi(20, dpi),),
                CustomFrameHit::Caption,
                "drag region beside tab strip at {dpi} DPI"
            );
            assert_eq!(
                custom_frame_hit_test(
                    m,
                    m.width - logical_px_for_dpi(23, dpi),
                    logical_px_for_dpi(20, dpi),
                ),
                CustomFrameHit::Client,
                "caption button at {dpi} DPI"
            );
            assert_eq!(
                custom_frame_hit_test(m, 1, m.height / 2),
                CustomFrameHit::Left,
                "resize border at {dpi} DPI"
            );
        }
    }

    #[test]
    fn maximized_frame_has_no_resize_hits_but_keeps_caption_dragging() {
        let mut m = metrics(144);
        m.resizable = false;
        m.resize_border = 0;
        assert_eq!(custom_frame_hit_test(m, 0, 0), CustomFrameHit::Client);
        assert_eq!(
            custom_frame_hit_test(m, m.tab_strip_right_px + 1, 1),
            CustomFrameHit::Caption
        );
        assert_eq!(
            custom_frame_hit_test(m, m.width - 1, 1),
            CustomFrameHit::Client
        );
    }
}
