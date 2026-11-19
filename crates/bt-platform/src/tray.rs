//! **Folio's icon in the notification area** (0.4.10 plan row 87, part 87b;
//! T-TRAY-87B-WINDOWS-ICON; `docs/DESIGN.md` §7.54 and its 2026-11 entries).
//!
//! The icon is the run's while the switch that keeps it (`close_keeps_running`) is on: a click
//! or Enter on it brings a put-away window back through `bt-app`'s one door, and its menu has
//! three rows — show Folio, a new window, quit Folio. What it shows is `bt-app`'s fold over
//! every tab, handed in as a [`TrayPicture`]: the program's icon, with a badge for the loudest
//! claim, and a tip that counts what is working and what is waiting.
//!
//! # The host window, and what its procedure does
//!
//! A notification icon is filed against a window, and the shell sends its callbacks there. The
//! host is a window of its own ([`TrayIcon`]), never one of Folio's, so a window put away or
//! closed takes nothing of the icon with it: a top-level `WS_POPUP` with `WS_EX_TOOLWINDOW`, never
//! shown — no taskbar button, no place in the window switcher — and not a message-only window,
//! which could not take the foreground the menu needs. It is made on the window thread, from
//! the module this process is running (`GetModuleHandleW`), so no window of Folio's anchors it.
//!
//! Its procedure parks and wakes, and acts on nothing of the application's:
//!
//! * **the shell's callback** ([`TRAY_CALLBACK_MESSAGE`], version 4): decoded by the pure
//!   [`tray_callback_for`]. A click or Enter parks [`TrayGesture::Show`] and wakes the loop; a
//!   right click or the menu key posts the menu to the host itself ([`TRAY_LATER_MESSAGE`]), so
//!   `TrackPopupMenu`'s nested pump never runs inside the dispatch the callback arrived in;
//! * **the posted menu**: drawn with the caller's labels; the row chosen is parked;
//! * **`TaskbarCreated`**: the shell restarted and took the icon with it, so it is added again;
//! * **a display or a setting changed** (`WM_DPICHANGED`, `WM_DISPLAYCHANGE`,
//!   `WM_SETTINGCHANGE`): a redraw is posted to the host, which draws the icon again only when
//!   the side or the taskbar's theme moved;
//! * **`WM_CLOSE`**: answered 0. A tool that closes every top-level window of the process
//!   (`taskkill` without `/F`) would otherwise destroy the host and leave a dead icon. It is not
//!   a reader of the close: it neither quits nor hides anything.
//!
//! Everything else goes to `DefWindowProcW`, `WM_QUERYENDSESSION` and Folio's quit from outside
//! included: the host installs no watch of its own on the session's end (Folio's windows hear
//! it, one subclass each, `crate::session_end`).
//!
//! # What is pure
//!
//! The decode ([`tray_callback_for`]), the menu's numbering ([`TrayGesture::menu_id`]), the tip's
//! cut ([`tray_tip`]), the procedure's answer to each message ([`tray_host_answer`]) and the
//! badge ([`badge_the_icon`]) are functions a test holds on any host: they are the parts that can
//! be wrong without a taskbar.
//!
//! # macOS and the rest
//!
//! There is no notification area to put an icon in (`HostPlatform::has_a_notification_area`);
//! the value is constructible and shows nothing, so the shared code compiles without a `cfg`.

/// **One verb a person asked of the icon**, parked by the host and spent by the loop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayGesture {
    /// A click, Enter, or the menu's first row: the window, back on the glass.
    Show,
    /// The menu's second row: a new window, and only that.
    NewWindow,
    /// The menu's third row: Folio's quit, the same transaction every quit is.
    Quit,
}

impl TrayGesture {
    /// The menu's rows, in the order it draws them.
    pub const MENU: [Self; 3] = [Self::Show, Self::NewWindow, Self::Quit];

    /// **The number this row is chosen by** in `TrackPopupMenu`'s answer: one-based, because the
    /// answer `0` is a menu dismissed with nothing chosen.
    #[must_use]
    pub const fn menu_id(self) -> u32 {
        match self {
            Self::Show => 1,
            Self::NewWindow => 2,
            Self::Quit => 3,
        }
    }

    /// The row `TrackPopupMenu` answered, or `None` for a menu dismissed (`0`) or a number that
    /// is no row.
    #[must_use]
    pub const fn from_menu_id(id: u32) -> Option<Self> {
        match id {
            1 => Some(Self::Show),
            2 => Some(Self::NewWindow),
            3 => Some(Self::Quit),
            _ => None,
        }
    }
}

/// **The words of the icon's menu**, handed in by the caller (T-NATIVE-LABELS-FROM-THE-CALLER):
/// this crate holds no words of its own, and the interface language can change while the program
/// runs.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TrayLabels {
    pub show: String,
    pub new_window: String,
    pub quit: String,
}

impl TrayLabels {
    /// The label of one row.
    #[must_use]
    pub fn of(&self, row: TrayGesture) -> &str {
        match row {
            TrayGesture::Show => &self.show,
            TrayGesture::NewWindow => &self.new_window,
            TrayGesture::Quit => &self.quit,
        }
    }
}

/// **The badge on the icon**: its ink on a dark taskbar and on a light one, and whether it is a
/// ring rather than a filled disc. The host draws the ink of the taskbar's own theme.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrayBadge {
    pub ink_on_dark: [u8; 3],
    pub ink_on_light: [u8; 3],
    pub hollow: bool,
}

impl TrayBadge {
    /// The ink drawn on a taskbar that is light, or dark.
    #[must_use]
    pub const fn ink(self, light_taskbar: bool) -> [u8; 3] {
        if light_taskbar {
            self.ink_on_light
        } else {
            self.ink_on_dark
        }
    }
}

/// **What the icon shows**: the program's icon with a badge or none, and the tip the shell shows
/// when the pointer rests on it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TrayPicture {
    pub badge: Option<TrayBadge>,
    pub tip: String,
}

/// **What one callback from the shell means**, decoded by [`tray_callback_for`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayCallback {
    /// A click on the icon, or Enter (or Space) on it from the keyboard.
    Show,
    /// A right click, or the menu key: the menu, at this point of the screen.
    Menu { x: i32, y: i32 },
}

/// The id the host files its one icon under: an icon is keyed by its window and this number, and
/// the host has one icon.
pub const TRAY_ICON_ID: u32 = 1;

/// **The private message the shell is asked to send its callbacks as** (`WM_APP + 0x4bb`; the
/// numbers below `0x4bb` are the deferred gestures' in `lib.rs`).
pub const TRAY_CALLBACK_MESSAGE: u32 = WM_APP + 0x4bb;

/// **The message the host posts to itself for what must not run inside another dispatch**
/// (`WM_APP + 0x4bc`): the menu ([`TRAY_LATER_MENU`]) and a redraw ([`TRAY_LATER_REDRAW`]), told
/// apart by its `wParam`.
pub const TRAY_LATER_MESSAGE: u32 = WM_APP + 0x4bc;

/// [`TRAY_LATER_MESSAGE`]'s `wParam` for the menu.
pub const TRAY_LATER_MENU: usize = 1;

/// [`TRAY_LATER_MESSAGE`]'s `wParam` for a redraw at the display's side and the taskbar's theme.
pub const TRAY_LATER_REDRAW: usize = 2;

/// **The class the host window is made from.** The release smoke finds the host among the
/// process's windows by this name and asks the shell for the icon's rectangle
/// (`scripts/release/smoke.ps1`, gate 6), so the two are held to each other by a test.
pub const TRAY_HOST_CLASS: &str = "FolioTrayHost";

/// **How many UTF-16 units of tip the shell holds**, the terminator included:
/// `NOTIFYICONDATAW::szTip` is 128 units (held against the structure by a Windows test).
pub const TRAY_TIP_CAPACITY: usize = 128;

/// Win32's numbers, written as the numbers they are, so the pure decode is the same function on
/// every platform; a Windows test holds each against the `windows` crate's.
const WM_APP: u32 = 0x8000;
const WM_CLOSE: u32 = 0x0010;
const WM_CONTEXTMENU: u32 = 0x007b;
const WM_SETTINGCHANGE: u32 = 0x001a;
const WM_DISPLAYCHANGE: u32 = 0x007e;
const WM_DPICHANGED: u32 = 0x02e0;
const NIN_SELECT: u32 = 0x0400;
/// `NIN_KEYSELECT`, which `ShellAPI.h` spells `NIN_SELECT | NINF_KEY`.
const NIN_KEYSELECT: u32 = 0x0401;

/// **Which callback this is**, or `None` for one this program has no verb for — or one about an
/// icon that is not ours.
///
/// **The version-4 shape**, which the host asks the shell for (`NIM_SETVERSION`): the event is
/// the low word of `lParam` and the icon's id its high word; `wParam` carries the point the
/// menu opens at, its x in the low word and its y in the high one, each signed. `NIN_SELECT` is
/// a click and `NIN_KEYSELECT` the keyboard's select; `WM_CONTEXTMENU` is a right click or the
/// menu key. A button's press and release, a hover, a balloon are facts about the icon this
/// program has no verb for.
#[must_use]
pub fn tray_callback_for(wparam: usize, lparam: isize, icon_id: u32) -> Option<TrayCallback> {
    let event = (lparam & 0xffff) as u32;
    let id = ((lparam >> 16) & 0xffff) as u32;
    if id != icon_id {
        return None;
    }
    match event {
        NIN_SELECT | NIN_KEYSELECT => Some(TrayCallback::Show),
        WM_CONTEXTMENU => Some(TrayCallback::Menu {
            x: i32::from(wparam as u16 as i16),
            y: i32::from((wparam >> 16) as u16 as i16),
        }),
        _ => None,
    }
}

/// **What the host's procedure does with one message** ([`tray_host_answer`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayHostAnswer {
    /// The shell's callback: decode it ([`tray_callback_for`]).
    Callback,
    /// The posted menu.
    Menu,
    /// The posted redraw.
    Redraw,
    /// The shell restarted: add the icon again.
    AddAgain,
    /// A display or a setting changed: post a redraw to the host.
    PostRedraw,
    /// A close: answered 0, nothing done.
    IgnoreTheClose,
    /// Everything else: `DefWindowProcW`.
    Default,
}

/// **The host's answer to `message`**, `taskbar_created` being the number the session gave
/// `TaskbarCreated` (`RegisterWindowMessageW`, `0` when it would not say). Pure, so the
/// procedure's table is a fact a test can state.
#[must_use]
pub fn tray_host_answer(message: u32, wparam: usize, taskbar_created: u32) -> TrayHostAnswer {
    match message {
        TRAY_CALLBACK_MESSAGE => TrayHostAnswer::Callback,
        TRAY_LATER_MESSAGE if wparam == TRAY_LATER_MENU => TrayHostAnswer::Menu,
        TRAY_LATER_MESSAGE if wparam == TRAY_LATER_REDRAW => TrayHostAnswer::Redraw,
        WM_CLOSE => TrayHostAnswer::IgnoreTheClose,
        WM_DPICHANGED | WM_DISPLAYCHANGE | WM_SETTINGCHANGE => TrayHostAnswer::PostRedraw,
        _ if taskbar_created != 0 && message == taskbar_created => TrayHostAnswer::AddAgain,
        _ => TrayHostAnswer::Default,
    }
}

/// **A tip cut to what the shell holds**, on a character boundary, with its terminator.
///
/// The count is UTF-16 units, which is what `szTip` holds: sixty Chinese characters fit and
/// sixty emoji do not. A surrogate pair cut in half is dropped rather than drawn as the
/// replacement glyph.
#[must_use]
pub fn tray_tip(tip: &str) -> Vec<u16> {
    let mut units: Vec<u16> = tip.encode_utf16().take(TRAY_TIP_CAPACITY - 1).collect();
    if units
        .last()
        .is_some_and(|unit| (0xd800..0xdc00).contains(unit))
    {
        units.pop();
    }
    units.push(0);
    units
}

/// The badge's diameter, as a share of the icon's side.
const BADGE_DIAMETER: f32 = 0.5;
/// The transparent gap around the badge, in pixels at the 16-pixel side (scaled with the side,
/// never under one pixel): it parts the badge from the icon beneath it on any colour.
const BADGE_GAP_AT_16: f32 = 1.0;
/// A ring's stroke, as a share of the badge's radius (never under one pixel).
const RING_STROKE: f32 = 0.45;
/// The samples per pixel along each axis the badge's edge is measured with.
const BADGE_SAMPLES: u32 = 4;

/// **The badge drawn into an icon's pixels**: a disc (or, `hollow`, a ring) of `ink` in the
/// bottom-right corner, touching its two edges, with a transparent gap around it.
///
/// `pixels` is the icon's colour, `side` × `side`, top row first, four bytes a pixel in the
/// order Windows keeps an icon's colour (blue, green, red, alpha), alpha not premultiplied. The
/// edge is measured at [`BADGE_SAMPLES`]² points a pixel; a pixel the gap and the badge do not
/// reach is left as it was. Pure: the part of the drawing a test can hold without a taskbar.
pub fn badge_the_icon(pixels: &mut [u8], side: u32, ink: [u8; 3], hollow: bool) {
    let side_f = side as f32;
    let radius = side_f * BADGE_DIAMETER / 2.0;
    let gap = (BADGE_GAP_AT_16 * side_f / 16.0).max(1.0);
    let stroke = (radius * RING_STROKE).max(1.0);
    let centre = side_f - radius;
    let samples = BADGE_SAMPLES * BADGE_SAMPLES;
    let step = 1.0 / BADGE_SAMPLES as f32;
    for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        let index = index as u32;
        let (x, y) = ((index % side) as f32, (index / side) as f32);
        let (mut cut, mut covered) = (0_u32, 0_u32);
        for sample in 0..samples {
            let sx = x + ((sample % BADGE_SAMPLES) as f32 + 0.5) * step;
            let sy = y + ((sample / BADGE_SAMPLES) as f32 + 0.5) * step;
            let distance = ((sx - centre).powi(2) + (sy - centre).powi(2)).sqrt();
            if distance < radius + gap {
                cut += 1;
            }
            if distance < radius && (!hollow || distance >= radius - stroke) {
                covered += 1;
            }
        }
        if cut == 0 {
            continue;
        }
        let keep = (samples - cut) as f32 / samples as f32;
        let cover = covered as f32 / samples as f32;
        let below = f32::from(pixel[3]) / 255.0 * keep;
        let alpha = below + cover;
        // Blue, green, red: the ink is red, green, blue.
        for (channel, ink_channel) in [(0, ink[2]), (1, ink[1]), (2, ink[0])] {
            let premultiplied = f32::from(pixel[channel]) * below + f32::from(ink_channel) * cover;
            pixel[channel] = if alpha > 0.0 {
                (premultiplied / alpha).round().clamp(0.0, 255.0) as u8
            } else {
                0
            };
        }
        pixel[3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
    }
}

#[cfg(windows)]
pub use windows_tray::TrayIcon;

#[cfg(windows)]
mod windows_tray {
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;
    use std::rc::Rc;
    use std::sync::OnceLock;

    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, DIB_RGB_COLORS, DeleteObject, GetDC,
        GetDIBits, HGDIOBJ, MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint, ReleaseDC,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
        NIM_SETVERSION, NOTIFY_ICON_MESSAGE, NOTIFYICON_VERSION_4, NOTIFYICONDATAW,
        Shell_NotifyIconW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, ChangeWindowMessageFilterEx, CreateIconIndirect, CreatePopupMenu,
        CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow, GWLP_USERDATA,
        GetCursorPos, GetIconInfo, GetWindowLongPtrW, HICON, ICONINFO, MF_STRING, MSGFLT_ALLOW,
        PostMessageW, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow,
        SetWindowLongPtrW, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_NULL, WNDCLASSW,
        WS_EX_TOOLWINDOW, WS_POPUP,
    };
    use windows::core::{PCWSTR, w};

    use super::{
        TRAY_CALLBACK_MESSAGE, TRAY_HOST_CLASS, TRAY_ICON_ID, TRAY_LATER_MENU, TRAY_LATER_MESSAGE,
        TRAY_LATER_REDRAW, TrayBadge, TrayCallback, TrayGesture, TrayHostAnswer, TrayLabels,
        TrayPicture, badge_the_icon, tray_callback_for, tray_host_answer, tray_tip,
    };

    /// **The number the session gave `TaskbarCreated`**, read once (`RegisterWindowMessageW`
    /// answers the same number for the life of the session); `0` when it would not say.
    fn taskbar_created_message() -> u32 {
        static MESSAGE: OnceLock<u32> = OnceLock::new();
        // SAFETY: a static NUL-terminated wide string, which the call copies.
        *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
    }

    /// The icon as it is drawn: the handle the shell was handed, and what it was drawn for.
    struct Drawn {
        icon: HICON,
        side: u32,
        light: bool,
        badge: Option<TrayBadge>,
    }

    impl Drop for Drawn {
        fn drop(&mut self) {
            if !self.icon.is_invalid() {
                // SAFETY: the icon was made for this value alone (`LoadImageW` without
                // `LR_SHARED`, or `CreateIconIndirect`), and the shell copies what it is handed.
                let _ = unsafe { DestroyIcon(self.icon) };
            }
        }
    }

    /// **What the host's procedure and its owner share**, on the one thread both run on.
    struct HostState {
        /// What a person asked and the loop has not taken.
        gestures: RefCell<Vec<TrayGesture>>,
        labels: RefCell<TrayLabels>,
        picture: RefCell<TrayPicture>,
        drawn: RefCell<Option<Drawn>>,
        /// Whether the shell holds the icon: `false` until an add is answered, and again after
        /// the shell refused one (it is added when the taskbar comes back, `TaskbarCreated`).
        in_the_shell: Cell<bool>,
        /// Whether the refusal was said: once, until an add is answered again.
        refusal_said: Cell<bool>,
        /// What to call after parking a gesture, and all it may do.
        wake: Box<dyn Fn()>,
        /// The note door (`diagnostics.log`).
        note: Box<dyn Fn(&str)>,
    }

    impl HostState {
        fn park(&self, gesture: TrayGesture) {
            self.gestures.borrow_mut().push(gesture);
            (self.wake)();
        }

        /// **The icon drawn again if the display's side, the taskbar's theme or the badge moved**
        /// since it was last drawn; answers whether it was.
        fn redraw(&self) -> bool {
            let side = notification_area_side();
            let light = taskbar_is_light();
            let badge = self.picture.borrow().badge;
            let unchanged = self.drawn.borrow().as_ref().is_some_and(|drawn| {
                drawn.side == side && drawn.light == light && drawn.badge == badge
            });
            if unchanged {
                return false;
            }
            let icon = draw_the_icon(side, light, badge).unwrap_or_else(|why| {
                (self.note)(&format!("BT_TRAY the icon could not be drawn: {why}"));
                HICON::default()
            });
            // The one before is destroyed once the shell has been handed this one (the caller's
            // next `tell`); dropping it here would leave the shell a handle to nothing for a
            // moment, which it draws as a blank.
            let before = self.drawn.replace(Some(Drawn {
                icon,
                side,
                light,
                badge,
            }));
            DRAWN_BEFORE.with_borrow_mut(|kept| *kept = before);
            true
        }

        /// One `Shell_NotifyIconW` of the whole description: the add or a change.
        fn tell(&self, hwnd: HWND, message: NOTIFY_ICON_MESSAGE) -> bool {
            let tip = tray_tip(&self.picture.borrow().tip);
            let icon = self
                .drawn
                .borrow()
                .as_ref()
                .map_or_else(HICON::default, |drawn| drawn.icon);
            let mut data = NOTIFYICONDATAW {
                cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: TRAY_ICON_ID,
                uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
                uCallbackMessage: TRAY_CALLBACK_MESSAGE,
                hIcon: icon,
                ..Default::default()
            };
            data.szTip[..tip.len()].copy_from_slice(&tip);
            // SAFETY: `data` is fully initialised with its own size; the call copies out of it.
            let told = unsafe { Shell_NotifyIconW(message, &data) }.as_bool();
            // The icon drawn before this one is let go once the shell holds the new one.
            DRAWN_BEFORE.with_borrow_mut(|kept| *kept = None);
            told
        }

        /// **The icon added to the shell, in the version-4 shape**; answers whether the shell
        /// took it. A refusal is said once and the host waits for `TaskbarCreated`.
        fn add(&self, hwnd: HWND) -> bool {
            let added = self.tell(hwnd, NIM_ADD) && set_version_4(hwnd);
            self.in_the_shell.set(added);
            if added {
                self.refusal_said.set(false);
            } else if !self.refusal_said.replace(true) {
                (self.note)(
                    "BT_TRAY the shell did not take the icon; it is added when the taskbar comes back",
                );
            }
            added
        }

        /// **The description told again**: a change while the shell holds the icon, the add
        /// when it does not or when a change is refused (the shell lost the icon unannounced).
        fn tell_again(&self, hwnd: HWND) {
            if self.in_the_shell.get() && self.tell(hwnd, NIM_MODIFY) {
                return;
            }
            let _ = self.add(hwnd);
        }
    }

    thread_local! {
        /// **The icon drawn before the one the shell is being handed**, kept until it has been
        /// (see `HostState::redraw`).
        static DRAWN_BEFORE: RefCell<Option<Drawn>> = const { RefCell::new(None) };
    }

    /// `NIM_SETVERSION` to version 4: the callback shape [`tray_callback_for`] decodes. It does
    /// not outlive the icon, so it follows every add.
    fn set_version_4(hwnd: HWND) -> bool {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            ..Default::default()
        };
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        // SAFETY: `data` is fully initialised with its own size; the call copies out of it.
        unsafe { Shell_NotifyIconW(NIM_SETVERSION, &data) }.as_bool()
    }

    /// **The small icon's side on the primary display**, where Windows 11 draws the notification
    /// area: `SM_CXSMICON` at its scale ([`crate::program_icon_sides`]); 96 DPI when the system
    /// will not say.
    fn notification_area_side() -> u32 {
        let (mut dpi_x, mut dpi_y) = (0_u32, 0_u32);
        // SAFETY: `MonitorFromPoint` with `MONITOR_DEFAULTTOPRIMARY` always answers a monitor;
        // both out-parameters are exclusively borrowed for the call.
        let read = unsafe {
            GetDpiForMonitor(
                MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY),
                MDT_EFFECTIVE_DPI,
                &mut dpi_x,
                &mut dpi_y,
            )
        };
        let dpi = if read.is_ok() && dpi_x > 0 { dpi_x } else { 96 };
        crate::program_icon_sides(f64::from(dpi) / 96.0).0
    }

    /// **Whether the taskbar wears the light theme**: `SystemUsesLightTheme`, the switch of the
    /// taskbar and the Start menu (`AppsUseLightTheme` is the applications'). A machine that does
    /// not say has the dark taskbar Windows ships.
    fn taskbar_is_light() -> bool {
        let mut value = 0_u32;
        let mut bytes = size_of::<u32>() as u32;
        // SAFETY: two static NUL-terminated wide strings; `value` and `bytes` are owned locals and
        // `bytes` states the capacity behind the pointer.
        let read = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
                w!("SystemUsesLightTheme"),
                RRF_RT_REG_DWORD,
                None,
                Some((&raw mut value).cast::<c_void>()),
                Some(&mut bytes),
            )
        };
        read.is_ok() && value != 0
    }

    /// **The program's icon at `side`, with `badge` drawn in the taskbar's ink**, as a handle
    /// the caller owns; the plain icon when there is no badge.
    fn draw_the_icon(side: u32, light: bool, badge: Option<TrayBadge>) -> Result<HICON, String> {
        let plain = crate::box_icon::icon_of_this_module(side)
            .ok_or("the program's icon is not in its module")?;
        let Some(badge) = badge else {
            return Ok(plain);
        };
        let drawn = badge_this_icon(plain, side, badge.ink(light), badge.hollow);
        // SAFETY: `plain` was loaded above for this call alone; the badged copy is a new icon.
        let _ = unsafe { DestroyIcon(plain) };
        drawn
    }

    /// A badged copy of `icon`: its colour read out, the badge drawn in ([`badge_the_icon`]), a
    /// new icon made of it.
    fn badge_this_icon(
        icon: HICON,
        side: u32,
        ink: [u8; 3],
        hollow: bool,
    ) -> Result<HICON, String> {
        let width = i32::try_from(side).map_err(|_| "the side is too large".to_owned())?;
        let mut info = ICONINFO::default();
        // SAFETY: a live icon this module loaded; the info is an owned out-parameter, and its two
        // bitmaps are deleted below on every path.
        unsafe { GetIconInfo(icon, &mut info) }.map_err(|error| format!("GetIconInfo: {error}"))?;
        let mut pixels = vec![0_u8; (side * side * 4) as usize];
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Negative: the rows top first, as `badge_the_icon` reads them.
                biHeight: -width,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: the screen's device context, released below; the buffer holds `side` rows of
        // `side` four-byte pixels, which is what the header asks for.
        let rows = unsafe {
            let screen = GetDC(None);
            let rows = GetDIBits(
                screen,
                info.hbmColor,
                0,
                side,
                Some(pixels.as_mut_ptr().cast::<c_void>()),
                &mut header,
                DIB_RGB_COLORS,
            );
            ReleaseDC(None, screen);
            let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
            let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
            rows
        };
        if rows != width {
            return Err(format!("GetDIBits read {rows} rows of {side}"));
        }
        badge_the_icon(&mut pixels, side, ink, hollow);
        // A monochrome mask of zeros: with a colour of 32 bits, its alpha is the shape.
        let mask_bytes = vec![0_u8; side.div_ceil(16) as usize * 2 * side as usize];
        // SAFETY: both buffers hold what their bitmaps' sizes ask for (a mask row is word
        // aligned); the icon copies both bitmaps, which are deleted right after.
        unsafe {
            let colour = CreateBitmap(width, width, 1, 32, Some(pixels.as_ptr().cast::<c_void>()));
            let mask = CreateBitmap(
                width,
                width,
                1,
                1,
                Some(mask_bytes.as_ptr().cast::<c_void>()),
            );
            let made = CreateIconIndirect(&ICONINFO {
                fIcon: true.into(),
                xHotspot: 0,
                yHotspot: 0,
                hbmMask: mask,
                hbmColor: colour,
            });
            let _ = DeleteObject(HGDIOBJ(colour.0));
            let _ = DeleteObject(HGDIOBJ(mask.0));
            made.map_err(|error| format!("CreateIconIndirect: {error}"))
        }
    }

    /// The host's class name, as a wide string.
    fn host_class_name() -> &'static [u16] {
        static NAME: OnceLock<Vec<u16>> = OnceLock::new();
        NAME.get_or_init(|| TRAY_HOST_CLASS.encode_utf16().chain([0]).collect())
    }

    /// **The host's class, registered once** for this process.
    fn host_class(instance: HINSTANCE) -> Result<PCWSTR, String> {
        static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
        let name = PCWSTR(host_class_name().as_ptr());
        REGISTERED
            .get_or_init(|| {
                let class = WNDCLASSW {
                    lpfnWndProc: Some(tray_host_procedure),
                    hInstance: instance,
                    lpszClassName: name,
                    ..Default::default()
                };
                // SAFETY: every field is zero or a value that outlives the process: a `'static`
                // procedure and a `'static` name. The class is never unregistered.
                if unsafe { RegisterClassW(&class) } == 0 {
                    return Err("RegisterClassW(tray host) refused".to_owned());
                }
                Ok(())
            })
            .clone()
            .map(|()| name)
    }

    /// **Folio's icon in the notification area, for as long as this value lives.**
    ///
    /// Neither `Send` nor `Sync`: a window is destroyed by the thread that made it.
    pub struct TrayIcon {
        hwnd: HWND,
        /// Reached by the procedure through the window's user data; `Rc` because the one thread
        /// re-enters through `TrackPopupMenu`'s nested pump, which the procedure holds a count
        /// across.
        state: Rc<HostState>,
    }

    impl TrayIcon {
        /// **The host made and the icon added**, showing `picture`; or why the host could not be
        /// made. An icon the shell does not take is not a refusal: the host stays, says so once,
        /// and adds it when the taskbar comes back (`TaskbarCreated`) or on the next change.
        pub fn put_up(
            labels: TrayLabels,
            picture: TrayPicture,
            wake: Box<dyn Fn()>,
            note: Box<dyn Fn(&str)>,
        ) -> Result<Self, String> {
            // SAFETY: no name, so the call answers the executable's own module.
            let module = unsafe { GetModuleHandleW(PCWSTR::null()) }
                .map_err(|error| format!("GetModuleHandleW: {error}"))?;
            let instance = HINSTANCE::from(module);
            let class = host_class(instance)?;
            let state = Rc::new(HostState {
                gestures: RefCell::new(Vec::new()),
                labels: RefCell::new(labels),
                picture: RefCell::new(picture),
                drawn: RefCell::new(None),
                in_the_shell: Cell::new(false),
                refusal_said: Cell::new(false),
                wake,
                note,
            });
            // SAFETY: the class was registered above against this instance; the name pointers
            // are live wide strings; the window is destroyed by `Drop`, on this thread.
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_TOOLWINDOW,
                    class,
                    w!("Folio"),
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    Some(instance),
                    None,
                )
            }
            .map_err(|error| format!("CreateWindowExW(tray host): {error}"))?;
            // The state is published before the icon is added: the first callback arrives after
            // `NIM_ADD`, and a procedure that read the user data before this line would read null.
            //
            // SAFETY: the window was made on this thread and is alive; the pointer is kept live by
            // the `Rc` this value holds, and is cleared before the window is destroyed.
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, Rc::as_ptr(&state) as isize);
            }
            // An elevated Folio still hears Explorer (UIPI): the shell's restart and its callbacks
            // come from a process of lower integrity.
            for message in [taskbar_created_message(), TRAY_CALLBACK_MESSAGE] {
                if message != 0 {
                    // SAFETY: a live window of this thread; no out-structure.
                    let _ =
                        unsafe { ChangeWindowMessageFilterEx(hwnd, message, MSGFLT_ALLOW, None) };
                }
            }
            let icon = Self { hwnd, state };
            let _ = icon.state.redraw();
            let _ = icon.state.add(icon.hwnd);
            Ok(icon)
        }

        /// **Show `picture`**: the icon drawn again if its badge moved, and the shell told.
        pub fn change(&self, picture: &TrayPicture) {
            *self.state.picture.borrow_mut() = picture.clone();
            let _ = self.state.redraw();
            self.state.tell_again(self.hwnd);
        }

        /// The menu's words, for the next time it opens.
        pub fn relabel(&self, labels: &TrayLabels) {
            *self.state.labels.borrow_mut() = labels.clone();
        }

        /// **What a person asked since this was last asked**, in their order.
        #[must_use]
        pub fn take_gestures(&self) -> Vec<TrayGesture> {
            std::mem::take(&mut *self.state.gestures.borrow_mut())
        }
    }

    impl Drop for TrayIcon {
        fn drop(&mut self) {
            // The icon first: a host destroyed under a live icon leaves the shell drawing one
            // whose clicks reach nothing. Then the user data, so a message still queued finds a
            // null. Then the window.
            let data = NOTIFYICONDATAW {
                cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: TRAY_ICON_ID,
                ..Default::default()
            };
            // SAFETY: dropped on the thread that made the window; `data` is fully initialised.
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &data);
                SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }

    /// **The host's procedure**, which parks and wakes and acts on nothing of the application's
    /// (the module's header; [`tray_host_answer`] is its table).
    unsafe extern "system" fn tray_host_procedure(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let answer = tray_host_answer(message, wparam.0, taskbar_created_message());
        if answer == TrayHostAnswer::IgnoreTheClose {
            return LRESULT(0);
        }
        // SAFETY: this window's own user data, set by `put_up` to an `Rc` pointer that outlives
        // the window and cleared before it is destroyed.
        let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const HostState;
        if pointer.is_null() || answer == TrayHostAnswer::Default {
            // SAFETY: the default handling of a message the host does not answer itself.
            return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
        }
        // SAFETY: the owning `TrayIcon` holds one count at entry; this one keeps the state alive
        // across `TrackPopupMenu`'s nested pump even if the owner is dropped inside it.
        unsafe { Rc::increment_strong_count(pointer) };
        // SAFETY: the increment above made the count this consumes.
        let state = unsafe { Rc::from_raw(pointer) };
        match answer {
            TrayHostAnswer::Callback => match tray_callback_for(wparam.0, lparam.0, TRAY_ICON_ID) {
                Some(TrayCallback::Show) => state.park(TrayGesture::Show),
                Some(TrayCallback::Menu { x, y }) => {
                    MENU_AT.set(Some((x, y)));
                    post_later(hwnd, TRAY_LATER_MENU);
                }
                None => {}
            },
            TrayHostAnswer::Menu => {
                let at = MENU_AT.take();
                if let Some(row) = track_tray_menu(hwnd, &state.labels.borrow().clone(), at) {
                    state.park(row);
                }
            }
            TrayHostAnswer::Redraw => {
                if state.redraw() {
                    state.tell_again(hwnd);
                }
            }
            TrayHostAnswer::AddAgain => {
                let _ = state.redraw();
                if state.add(hwnd) {
                    (state.note)("BT_TRAY the icon is added again: the taskbar came back");
                }
            }
            TrayHostAnswer::PostRedraw => {
                post_later(hwnd, TRAY_LATER_REDRAW);
                // SAFETY: the broadcast is the system's to answer as well.
                return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
            }
            TrayHostAnswer::IgnoreTheClose | TrayHostAnswer::Default => {}
        }
        LRESULT(0)
    }

    thread_local! {
        /// The point the last right click asked the menu at, kept for the posted menu.
        static MENU_AT: Cell<Option<(i32, i32)>> = const { Cell::new(None) };
    }

    /// Post `what` to the host, to run after the dispatch in progress has returned.
    fn post_later(hwnd: HWND, what: usize) {
        // SAFETY: `PostMessageW` copies its value parameters into this thread's queue.
        let _ = unsafe { PostMessageW(Some(hwnd), TRAY_LATER_MESSAGE, WPARAM(what), LPARAM(0)) };
    }

    /// **The icon's menu**, run only from the posted message: three rows in the caller's words,
    /// at the point the shell named (the pointer's when it named none). The host takes the
    /// foreground first, so clicking elsewhere dismisses the menu, and a `WM_NULL` after it lets
    /// the dismissal finish (the documented pair for a notification icon's menu).
    fn track_tray_menu(
        hwnd: HWND,
        labels: &TrayLabels,
        at: Option<(i32, i32)>,
    ) -> Option<TrayGesture> {
        // SAFETY: the window and the menu are this thread's; the menu made here is destroyed
        // here on every path; each label is a live NUL-terminated wide string across its call.
        unsafe {
            let menu = Result::ok(CreatePopupMenu())?;
            let chosen = (|| {
                for row in TrayGesture::MENU {
                    let mut label: Vec<u16> = labels.of(row).encode_utf16().collect();
                    label.push(0);
                    Result::ok(AppendMenuW(
                        menu,
                        MF_STRING,
                        row.menu_id() as usize,
                        PCWSTR(label.as_ptr()),
                    ))?;
                }
                let (x, y) = match at {
                    Some(point) => point,
                    None => {
                        let mut cursor = POINT::default();
                        Result::ok(GetCursorPos(&mut cursor))?;
                        (cursor.x, cursor.y)
                    }
                };
                let _ = SetForegroundWindow(hwnd);
                let chosen = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    x,
                    y,
                    None,
                    hwnd,
                    None,
                );
                let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
                TrayGesture::from_menu_id(chosen.0 as u32)
            })();
            let _ = DestroyMenu(menu);
            chosen
        }
    }
}

#[cfg(not(windows))]
pub use portable_tray::TrayIcon;

/// **The icon on a platform with no notification area**: constructible, showing nothing, asked
/// for nothing (`HostPlatform::has_a_notification_area` answers `false` there, so `bt-app` never
/// puts one up). It exists so the shared code compiles without a `cfg`.
#[cfg(not(windows))]
mod portable_tray {
    use super::{TrayGesture, TrayLabels, TrayPicture};

    pub struct TrayIcon;

    impl TrayIcon {
        /// Constructed, and nothing shown.
        pub fn put_up(
            _labels: TrayLabels,
            _picture: TrayPicture,
            _wake: Box<dyn Fn()>,
            _note: Box<dyn Fn(&str)>,
        ) -> Result<Self, String> {
            Ok(Self)
        }

        pub fn change(&self, _picture: &TrayPicture) {}

        pub fn relabel(&self, _labels: &TrayLabels) {}

        #[must_use]
        pub fn take_gestures(&self) -> Vec<TrayGesture> {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TRAY_CALLBACK_MESSAGE, TRAY_HOST_CLASS, TRAY_ICON_ID, TRAY_LATER_MENU, TRAY_LATER_MESSAGE,
        TRAY_LATER_REDRAW, TRAY_TIP_CAPACITY, TrayCallback, TrayGesture, TrayHostAnswer,
        badge_the_icon, tray_callback_for, tray_host_answer, tray_tip,
    };

    /// The `lParam` of a version-4 callback: the event in the low word, the icon's id in the high.
    fn callback(event: u32, id: u32) -> isize {
        ((id << 16) | event) as isize
    }

    /// RED (T13; 87b-P16) — **the three callbacks the icon answers, and only about our own icon.**
    ///
    /// PLANT 87b-P16: drop the id clause from `tray_callback_for` — another icon's select is read
    /// as ours.
    #[test]
    fn only_a_select_or_a_menu_over_our_own_icon_is_a_gesture() {
        let ours = |event| tray_callback_for(0, callback(event, TRAY_ICON_ID), TRAY_ICON_ID);
        assert_eq!(ours(0x0400), Some(TrayCallback::Show), "a click");
        assert_eq!(ours(0x0401), Some(TrayCallback::Show), "Enter on the icon");
        assert_eq!(
            tray_callback_for(
                (0x0040_usize << 16) | 0xfff6,
                callback(0x007b, TRAY_ICON_ID),
                TRAY_ICON_ID
            ),
            Some(TrayCallback::Menu { x: -10, y: 64 }),
            "a right click, at the point the shell names, each half signed"
        );
        for event in [0x0201, 0x0202, 0x0204, 0x0205, 0x0200, 0x0406] {
            assert_eq!(ours(event), None, "{event:#x} is no gesture");
        }
        assert_eq!(
            tray_callback_for(0, callback(0x0400, TRAY_ICON_ID + 1), TRAY_ICON_ID),
            None,
            "another icon's select is another icon's"
        );
    }

    /// RED (T13) — **the menu's rows survive the trip through `TrackPopupMenu`'s numbering**, and
    /// none of them is `0`, which is a menu dismissed.
    #[test]
    fn every_menu_row_is_its_own_number_and_none_of_them_is_zero() {
        for row in TrayGesture::MENU {
            assert_ne!(row.menu_id(), 0, "{row:?} is not a dismissal");
            assert_eq!(TrayGesture::from_menu_id(row.menu_id()), Some(row));
        }
        assert_eq!(
            TrayGesture::MENU,
            [TrayGesture::Show, TrayGesture::NewWindow, TrayGesture::Quit],
            "show Folio, a new window, quit Folio — in that order"
        );
        assert_eq!(TrayGesture::from_menu_id(0), None);
    }

    /// RED (T13) — **a tip too long for the shell is cut where a reader can still read it.**
    #[test]
    fn a_tip_is_cut_to_what_the_shell_holds_and_never_mid_character() {
        let short = tray_tip("Folio");
        assert_eq!(short.len(), 6, "five units and the terminator");
        let long = tray_tip(&"x".repeat(500));
        assert_eq!(
            long.len(),
            TRAY_TIP_CAPACITY,
            "exactly what the array holds"
        );
        assert_eq!(long.last(), Some(&0));
        let astral = tray_tip(&"\u{1f5dd}".repeat(200));
        let text = &astral[..astral.len() - 1];
        assert!(
            String::from_utf16(text).is_ok(),
            "no half of a surrogate pair is left at the end"
        );
    }

    /// RED (T13) — **the host's table**: the shell's callback is decoded, the posted menu and
    /// redraw run, the shell's restart adds the icon again, a display or setting change posts a
    /// redraw, and a close is answered and ignored; everything else is the system's.
    ///
    /// PLANT: answer `WM_CLOSE` as `Default` — `DefWindowProcW` destroys the host.
    #[test]
    fn the_host_adds_the_icon_again_when_the_taskbar_comes_back_and_ignores_a_close() {
        let created = 0xc0de;
        assert_eq!(
            tray_host_answer(created, 0, created),
            TrayHostAnswer::AddAgain
        );
        assert_eq!(
            tray_host_answer(0x0010, 0, created),
            TrayHostAnswer::IgnoreTheClose
        );
        assert_eq!(
            tray_host_answer(TRAY_CALLBACK_MESSAGE, 0, created),
            TrayHostAnswer::Callback
        );
        assert_eq!(
            tray_host_answer(TRAY_LATER_MESSAGE, TRAY_LATER_MENU, created),
            TrayHostAnswer::Menu
        );
        assert_eq!(
            tray_host_answer(TRAY_LATER_MESSAGE, TRAY_LATER_REDRAW, created),
            TrayHostAnswer::Redraw
        );
        for broadcast in [0x02e0, 0x007e, 0x001a] {
            assert_eq!(
                tray_host_answer(broadcast, 0, created),
                TrayHostAnswer::PostRedraw
            );
        }
        for other in [0x0011, 0x0016, 0x0001, 0x0002] {
            assert_eq!(
                tray_host_answer(other, 0, created),
                TrayHostAnswer::Default,
                "{other:#x}: the session's end, a creation, a destruction are the system's"
            );
        }
        assert_eq!(
            tray_host_answer(0, 0, 0),
            TrayHostAnswer::Default,
            "a session that gave no number for the restart has none to match"
        );
    }

    /// An icon of one opaque colour, `side` × `side`, in Windows' byte order (blue first).
    fn opaque(side: u32, rgb: [u8; 3]) -> Vec<u8> {
        (0..side * side)
            .flat_map(|_| [rgb[2], rgb[1], rgb[0], 255])
            .collect()
    }

    /// The pixel at `(x, y)`, as red, green, blue, alpha.
    fn at(pixels: &[u8], side: u32, x: u32, y: u32) -> [u8; 4] {
        let index = ((y * side + x) * 4) as usize;
        [
            pixels[index + 2],
            pixels[index + 1],
            pixels[index],
            pixels[index + 3],
        ]
    }

    /// RED (T13b) — **the badge lands in the bottom-right corner at the side's scale**, in its
    /// ink, parted from the icon by a transparent gap; a ring is open in its middle; the rest of
    /// the icon is as it was.
    #[test]
    fn the_badge_is_drawn_in_its_corner_and_nothing_else_moves() {
        let ink = [0xe0, 0x40, 0x30];
        for side in [16, 20, 24, 32] {
            let base = [0x10, 0x80, 0xf0];
            let mut filled = opaque(side, base);
            badge_the_icon(&mut filled, side, ink, false);
            let centre = side - side / 4;
            assert_eq!(
                at(&filled, side, centre, centre),
                [ink[0], ink[1], ink[2], 255],
                "{side}: the disc's middle is its ink"
            );
            assert_eq!(
                at(&filled, side, 0, 0),
                [base[0], base[1], base[2], 255],
                "{side}: the far corner is the icon's"
            );
            // The pixel inside the middle, up and to the left: past the badge and its gap at
            // every side (the gap reaches the middle itself at 16 px).
            assert_eq!(
                at(&filled, side, side / 2 - 1, side / 2 - 1),
                [base[0], base[1], base[2], 255],
                "{side}: the icon by its middle is the icon's"
            );
            // Along the row from the disc's middle leftwards to the icon: some pixel the gap
            // clears entirely (the gap is at least a pixel wide, and a row crosses it square).
            let gap_cleared = (0..side / 2).any(|step| {
                let at_step = at(&filled, side, centre - step, centre);
                at_step[3] == 0
            });
            assert!(
                gap_cleared,
                "{side}: a transparent gap parts the disc from the icon"
            );

            let mut ring = opaque(side, base);
            badge_the_icon(&mut ring, side, ink, true);
            assert_eq!(
                at(&ring, side, centre, centre)[3],
                0,
                "{side}: a ring is open in its middle"
            );
            assert_eq!(
                at(&ring, side, centre, side - 1),
                [ink[0], ink[1], ink[2], 255],
                "{side}: and inked at its edge"
            );
        }
    }

    /// RED (T13c) — **the release smoke asks for the host by the class the host is made from**:
    /// gate 6's icon half finds the host among the process's windows by that name, and a name
    /// the smoke did not know would be an icon it reports gone.
    ///
    /// A guard of a script (its subject is how the smoke is written), as 87a's pin of the outside
    /// quit's name is.
    #[test]
    fn the_smoke_asks_for_the_host_by_its_class() {
        let smoke = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/release/smoke.ps1");
        let smoke = std::fs::read_to_string(smoke).expect("the release smoke is in the tree");
        assert!(
            smoke.contains(&format!("'{}'", TRAY_HOST_CLASS)),
            "the smoke names the tray host's class '{}'",
            TRAY_HOST_CLASS
        );
    }

    /// RED (T13) — **only Windows has a notification area to put the run's icon in**: macOS's
    /// way back is the Dock (part 87c), and a Linux desktop's tray is not written.
    #[test]
    fn only_windows_has_a_notification_area() {
        use crate::HostPlatform;
        assert!(HostPlatform::Windows.has_a_notification_area());
        assert!(!HostPlatform::MacOs.has_a_notification_area());
        assert!(!HostPlatform::OtherUnix.has_a_notification_area());
    }
}

/// The numbers written down above, held against the ones Windows publishes.
#[cfg(all(test, windows))]
mod win32_constant_tests {
    use windows::Win32::UI::Shell::{NIN_SELECT, NINF_KEY, NOTIFYICONDATAW};
    use windows::Win32::UI::WindowsAndMessaging::{
        WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_SETTINGCHANGE,
    };

    /// RED (T13) — this module's copies of Win32's numbers are Win32's.
    #[test]
    fn the_numbers_written_down_are_the_numbers_windows_publishes() {
        assert_eq!(super::WM_APP, WM_APP);
        assert_eq!(super::WM_CLOSE, WM_CLOSE);
        assert_eq!(super::WM_CONTEXTMENU, WM_CONTEXTMENU);
        assert_eq!(super::WM_SETTINGCHANGE, WM_SETTINGCHANGE);
        assert_eq!(super::WM_DISPLAYCHANGE, WM_DISPLAYCHANGE);
        assert_eq!(super::WM_DPICHANGED, WM_DPICHANGED);
        assert_eq!(super::NIN_SELECT, NIN_SELECT);
        assert_eq!(super::NIN_KEYSELECT, NIN_SELECT | NINF_KEY);
    }

    /// RED (T13) — the tip capacity written down is the array the shell has.
    #[test]
    fn tray_tip_capacity_is_the_one_the_shell_publishes() {
        assert_eq!(
            super::TRAY_TIP_CAPACITY,
            NOTIFYICONDATAW::default().szTip.len()
        );
    }
}
