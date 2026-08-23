//! WebView2 in composition hosting, with every event the plan names attached
//! before anything navigates, and with a record of what each one said.
//!
//! The evidence table is shared by `Rc<RefCell<_>>` rather than by channel: all
//! of this is one thread by construction — WebView2 delivers its callbacks on
//! the thread that created the environment, and that is the same thread that
//! owns the message pump and the visual tree.

use anyhow::{Context as _, Result};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::*;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::core::{BOOL, HSTRING, Interface as _, PCWSTR, PWSTR};

// ── Out-parameter helpers ──────────────────────────────────────────────────
//
// Every WebView2 getter is `fn(&self, *mut T) -> Result<()>`. Reading one at
// each call site would bury the evidence in five lines of ceremony apiece, so
// the ceremony lives here once. A getter that fails yields the type's default,
// which for every field below reads as "the engine did not say".

fn read<T: Default>(getter: impl FnOnce(*mut T) -> windows::core::Result<()>) -> T {
    let mut value = T::default();
    let _ = getter(&mut value);
    value
}

fn read_bool(getter: impl FnOnce(*mut BOOL) -> windows::core::Result<()>) -> bool {
    read::<BOOL>(getter).as_bool()
}

fn read_string(getter: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>) -> String {
    let mut value = PWSTR::null();
    match getter(&mut value) {
        Ok(()) => take_pwstr(value),
        Err(_) => String::new(),
    }
}

// ── Evidence ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize)]
pub struct NavStarting {
    pub uri: String,
    pub user_initiated: bool,
    pub redirected: bool,
    pub cancelled: bool,
    pub refusal: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct NavCompleted {
    pub uri: String,
    pub success: bool,
    pub web_error_status: i32,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Accelerator {
    pub vk: u32,
    pub kind: i32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub repeat: bool,
    pub handled: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ProcessFailure {
    pub kind: i32,
    pub reason: i32,
    pub exit_code: i32,
    pub description: String,
}

#[derive(Default)]
pub struct Evidence {
    pub nav_starting: Vec<NavStarting>,
    pub nav_completed: Vec<NavCompleted>,
    pub source_changed: Vec<String>,
    pub accelerators: Vec<Accelerator>,
    pub move_focus_requested: Vec<i32>,
    pub focus_changed: Vec<&'static str>,
    pub cursor_changed: Vec<u32>,
    pub process_failures: Vec<ProcessFailure>,
    pub browser_exited: Vec<i32>,
    pub new_window_requested: Vec<(String, bool)>,
    pub downloads: Vec<String>,
    pub permissions: Vec<i32>,
    pub web_messages: Vec<String>,
    pub new_version_available: u32,
    pub script_dialogs: u32,
    pub launching_external: Vec<String>,
    /// URLs the policy has sanctioned for this navigation, if any.
    pub sanctioned_file: Option<String>,
    /// Set true to make `NavigationStarting` enforce §3. Off during setup so a
    /// deliberate red test can be told apart from the ordinary first flight.
    pub enforce_policy: bool,
}

impl Evidence {
    pub fn new() -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            enforce_policy: true,
            ..Default::default()
        }))
    }
}

// ── Runtime detection ──────────────────────────────────────────────────────

/// The only authoritative answer to "is the runtime installed". The registry
/// lies when `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` points somewhere empty; this
/// call does not.
pub fn runtime_version() -> Result<String, String> {
    let mut version = PWSTR::null();
    let result =
        unsafe { GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut version) };
    match result {
        Ok(()) if !version.is_null() => Ok(take_pwstr(version)),
        Ok(()) => Err("returned S_OK with a null version".to_owned()),
        Err(error) => Err(format!("{error}")),
    }
}

/// What the registry claims, for the contrast the 2026-08-13 spike drew.
pub fn runtime_registry_claims() -> Vec<(String, String)> {
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ, RegCloseKey,
        RegGetValueW, RegOpenKeyExW,
    };
    const CLIENT: &str =
        r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    const WOW: &str =
        r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    let mut found = Vec::new();
    for (label, root, path) in [
        ("HKLM", HKEY_LOCAL_MACHINE, CLIENT),
        ("HKLM\\WOW6432Node", HKEY_LOCAL_MACHINE, WOW),
        ("HKCU", HKEY_CURRENT_USER, CLIENT),
    ] {
        unsafe {
            let mut key = HKEY::default();
            if RegOpenKeyExW(root, &HSTRING::from(path), Some(0), KEY_READ, &mut key).is_ok() {
                let mut buffer = [0u16; 128];
                let mut size = (buffer.len() * 2) as u32;
                if RegGetValueW(
                    key,
                    PCWSTR::null(),
                    &HSTRING::from("pv"),
                    RRF_RT_REG_SZ,
                    None,
                    Some(buffer.as_mut_ptr().cast()),
                    Some(&mut size),
                )
                .is_ok()
                {
                    let length = (size as usize / 2).saturating_sub(1);
                    found.push((
                        String::from(label),
                        String::from_utf16_lossy(&buffer[..length]),
                    ));
                }
                let _ = RegCloseKey(key);
            }
        }
    }
    found
}

// ── Environment ────────────────────────────────────────────────────────────

thread_local! {
    /// **One environment per process.** Two environments over one user data
    /// folder with different options is 0x8007139F, and two with the same
    /// options is two browser process trees for no reason.
    static ENVIRONMENT: RefCell<Option<ICoreWebView2Environment>> = const { RefCell::new(None) };
}

/// Get the process-wide environment, creating it on first call.
///
/// Returns `(environment, was_created_now)` so a caller can prove the second
/// call did not build a second one.
pub fn environment(user_data_folder: &Path) -> Result<(ICoreWebView2Environment, bool)> {
    if let Some(existing) = ENVIRONMENT.with(|cell| cell.borrow().clone()) {
        return Ok((existing, false));
    }
    let folder = HSTRING::from(user_data_folder.as_os_str());
    let holder: Rc<RefCell<Option<ICoreWebView2Environment>>> = Rc::new(RefCell::new(None));
    let sink = Rc::clone(&holder);
    CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                PCWSTR(folder.as_ptr()),
                None::<&ICoreWebView2EnvironmentOptions>,
                &handler,
            )
            .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |result, created| {
            result?;
            *sink.borrow_mut() = created;
            Ok(())
        }),
    )
    .map_err(|error| anyhow::anyhow!("CreateCoreWebView2EnvironmentWithOptions: {error:?}"))?;
    let created = holder
        .borrow_mut()
        .take()
        .context("environment callback delivered no environment")?;
    ENVIRONMENT.with(|cell| *cell.borrow_mut() = Some(created.clone()));
    Ok((created, true))
}

/// Forget the cached environment without closing it — used only by the gate that
/// proves a *second* `environment()` call returns the same object.
pub fn environment_is_cached() -> bool {
    ENVIRONMENT.with(|cell| cell.borrow().is_some())
}

/// Drop the cached environment so the next `environment()` builds a new one.
///
/// This is the step a runtime update forces and the one it is easiest to leave
/// out: a new controller made over the *old* environment is a controller on the
/// old browser build, so the update takes effect for nobody. The caller must
/// already have closed every controller and waited for the browser to go —
/// a new environment over a folder the old browser still holds does not fail,
/// it simply never calls back.
pub fn forget_environment() {
    ENVIRONMENT.with(|cell| *cell.borrow_mut() = None);
}

// ── The host ───────────────────────────────────────────────────────────────

/// Where the WebView2 believes its own origin is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoundsOrigin {
    /// `SetBounds` gets the seat rectangle at its real position in the client
    /// area, and the visual carries no offset of its own.
    AtSeat,
    /// `SetBounds` gets `(0, 0, w, h)` and the *visual* is offset to the seat.
    /// This is the arrangement every visual-hosting sample uses.
    AtZero,
}

pub struct Host {
    pub environment: ICoreWebView2Environment,
    pub composition: ICoreWebView2CompositionController,
    pub controller: ICoreWebView2Controller,
    pub webview: ICoreWebView2,
    pub evidence: Rc<RefCell<Evidence>>,
    pub seat: RECT,
    pub origin: BoundsOrigin,
    /// Which injected frame the next pointer belongs to. Contacts sent inside
    /// one `frame` share it; `next_pointer_frame` moves it on.
    frame_id: std::cell::Cell<u32>,
}

impl Host {
    /// Start a new pointer frame. Two fingers sent after the same call are two
    /// contacts of one frame; a finger sent after another call is a later
    /// frame.
    pub fn next_pointer_frame(&self) -> u32 {
        let next = self.frame_id.get() + 1;
        self.frame_id.set(next);
        next
    }
}

impl Host {
    /// Create the controller and attach **every** handler before returning. The
    /// caller navigates afterwards; nothing here does.
    pub fn create(
        environment: &ICoreWebView2Environment,
        parent: HWND,
        evidence: Rc<RefCell<Evidence>>,
    ) -> Result<Self> {
        let environment3: ICoreWebView2Environment3 =
            environment.cast().context("ICoreWebView2Environment3")?;
        let holder: Rc<RefCell<Option<ICoreWebView2CompositionController>>> =
            Rc::new(RefCell::new(None));
        let sink = Rc::clone(&holder);
        CreateCoreWebView2CompositionControllerCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe {
                environment3
                    .CreateCoreWebView2CompositionController(parent, &handler)
                    .map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |result, controller| {
                result?;
                *sink.borrow_mut() = controller;
                Ok(())
            }),
        )
        .map_err(|error| anyhow::anyhow!("CreateCoreWebView2CompositionController: {error:?}"))?;
        let composition = holder
            .borrow_mut()
            .take()
            .context("composition controller callback delivered nothing")?;
        let controller: ICoreWebView2Controller =
            composition.cast().context("ICoreWebView2Controller")?;
        let webview = unsafe { controller.CoreWebView2() }.context("CoreWebView2")?;

        let host = Self {
            environment: environment.clone(),
            composition,
            controller,
            webview,
            evidence,
            seat: RECT::default(),
            origin: BoundsOrigin::AtZero,
            frame_id: std::cell::Cell::new(1),
        };
        host.configure_settings()?;
        host.attach_events()?;
        Ok(host)
    }

    fn configure_settings(&self) -> Result<()> {
        unsafe {
            let settings = self.webview.Settings().context("Settings")?;
            // The page talks back through `window.chrome.webview.postMessage`;
            // that is the probe's only channel for "what did the page actually
            // receive", and it is the one bridge §3 says a *file:* page must not
            // get. The gates that load a file page turn it off again.
            settings
                .SetIsWebMessageEnabled(true)
                .context("IsWebMessageEnabled")?;
            settings
                .SetAreDevToolsEnabled(true)
                .context("AreDevToolsEnabled")?;
            settings
                .SetIsStatusBarEnabled(false)
                .context("IsStatusBarEnabled")?;
            let controller3: ICoreWebView2Controller3 =
                self.controller.cast().context("ICoreWebView2Controller3")?;
            // Physical pixels in, physical pixels out. `bt_layout` already works
            // in device pixels; multiplying by a scale factor here would do it
            // twice.
            controller3
                .SetBoundsMode(COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS)
                .context("SetBoundsMode")?;
            controller3
                .SetShouldDetectMonitorScaleChanges(true)
                .context("SetShouldDetectMonitorScaleChanges")?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn attach_events(&self) -> Result<()> {
        let mut token = 0i64;
        unsafe {
            // ── navigation ────────────────────────────────────────────────
            let evidence = Rc::clone(&self.evidence);
            self.webview
                .add_NavigationStarting(
                    &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let uri = read_string(|out| args.Uri(out));
                        let user_initiated = read_bool(|out| args.IsUserInitiated(out));
                        let redirected = read_bool(|out| args.IsRedirected(out));
                        let mut evidence = evidence.borrow_mut();
                        let refusal = if evidence.enforce_policy {
                            crate::policy::navigation_starting(
                                &uri,
                                evidence.sanctioned_file.as_deref(),
                            )
                            .err()
                            .map(|refusal| format!("{refusal:?}"))
                        } else {
                            None
                        };
                        let cancelled = refusal.is_some();
                        if cancelled {
                            args.SetCancel(true)?;
                        }
                        evidence.nav_starting.push(NavStarting {
                            uri,
                            user_initiated,
                            redirected,
                            cancelled,
                            refusal,
                        });
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_NavigationStarting")?;

            let evidence = Rc::clone(&self.evidence);
            self.webview
                .add_NavigationCompleted(
                    &NavigationCompletedEventHandler::create(Box::new(move |view, args| {
                        let Some(args) = args else { return Ok(()) };
                        let success = read_bool(|out| args.IsSuccess(out));
                        let status =
                            read::<COREWEBVIEW2_WEB_ERROR_STATUS>(|out| args.WebErrorStatus(out)).0;
                        let uri = view
                            .map(|view| read_string(|out| view.Source(out)))
                            .unwrap_or_default();
                        evidence.borrow_mut().nav_completed.push(NavCompleted {
                            uri,
                            success,
                            web_error_status: status,
                        });
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_NavigationCompleted")?;

            let evidence = Rc::clone(&self.evidence);
            self.webview
                .add_SourceChanged(
                    &SourceChangedEventHandler::create(Box::new(move |view, _| {
                        if let Some(view) = view {
                            let uri = read_string(|out| view.Source(out));
                            evidence.borrow_mut().source_changed.push(uri);
                        }
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_SourceChanged")?;

            // ── window.open / downloads / permissions / external ──────────
            let evidence = Rc::clone(&self.evidence);
            self.webview
                .add_NewWindowRequested(
                    &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let uri = read_string(|out| args.Uri(out));
                        let user_initiated = read_bool(|out| args.IsUserInitiated(out));
                        // §0: a user-initiated new window becomes a navigation in
                        // this same seat; anything else is a popup and is
                        // cancelled. Either way the engine does not get to open a
                        // window of its own.
                        args.SetHandled(true)?;
                        evidence
                            .borrow_mut()
                            .new_window_requested
                            .push((uri, user_initiated));
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_NewWindowRequested")?;

            let evidence = Rc::clone(&self.evidence);
            let downloads: ICoreWebView2_4 = self.webview.cast().context("ICoreWebView2_4")?;
            downloads
                .add_DownloadStarting(
                    &DownloadStartingEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let uri = args
                            .DownloadOperation()
                            .map(|operation| read_string(|out| operation.Uri(out)))
                            .unwrap_or_default();
                        // v1 has no download manager: cancel, and let the chrome
                        // decide whether the URL is replayable as a GET.
                        args.SetCancel(true)?;
                        evidence.borrow_mut().downloads.push(uri);
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_DownloadStarting")?;

            let evidence = Rc::clone(&self.evidence);
            self.webview
                .add_PermissionRequested(
                    &PermissionRequestedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let kind =
                            read::<COREWEBVIEW2_PERMISSION_KIND>(|out| args.PermissionKind(out)).0;
                        args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
                        evidence.borrow_mut().permissions.push(kind);
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_PermissionRequested")?;

            let evidence = Rc::clone(&self.evidence);
            let external: ICoreWebView2_18 = self.webview.cast().context("ICoreWebView2_18")?;
            external
                .add_LaunchingExternalUriScheme(
                    &LaunchingExternalUriSchemeEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let uri = read_string(|out| args.Uri(out));
                        args.SetCancel(true)?;
                        evidence.borrow_mut().launching_external.push(uri);
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_LaunchingExternalUriScheme")?;

            let evidence = Rc::clone(&self.evidence);
            self.webview
                .add_ScriptDialogOpening(
                    &ScriptDialogOpeningEventHandler::create(Box::new(move |_, _| {
                        evidence.borrow_mut().script_dialogs += 1;
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_ScriptDialogOpening")?;

            // ── process lifetime ──────────────────────────────────────────
            let evidence = Rc::clone(&self.evidence);
            self.webview
                .add_ProcessFailed(
                    &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let kind = read::<COREWEBVIEW2_PROCESS_FAILED_KIND>(|out| {
                            args.ProcessFailedKind(out)
                        })
                        .0;
                        let (reason, exit_code, description) =
                            match args.cast::<ICoreWebView2ProcessFailedEventArgs2>() {
                                Ok(args2) => (
                                    read::<COREWEBVIEW2_PROCESS_FAILED_REASON>(|out| {
                                        args2.Reason(out)
                                    })
                                    .0,
                                    read::<i32>(|out| args2.ExitCode(out)),
                                    read_string(|out| args2.ProcessDescription(out)),
                                ),
                                Err(_) => (-1, 0, String::new()),
                            };
                        evidence.borrow_mut().process_failures.push(ProcessFailure {
                            kind,
                            reason,
                            exit_code,
                            description,
                        });
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_ProcessFailed")?;

            // ── keyboard / focus ──────────────────────────────────────────
            let evidence = Rc::clone(&self.evidence);
            self.controller
                .add_AcceleratorKeyPressed(
                    &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let vk = read::<u32>(|out| args.VirtualKey(out));
                        let kind =
                            read::<COREWEBVIEW2_KEY_EVENT_KIND>(|out| args.KeyEventKind(out)).0;
                        let status = read::<COREWEBVIEW2_PHYSICAL_KEY_STATUS>(|out| {
                            args.PhysicalKeyStatus(out)
                        });
                        let modifiers = current_modifiers();
                        // The whole reason this callback matters: it runs on the
                        // host thread *before* the page sees the key, so a chord
                        // this product owns can be taken back here.
                        let claimed =
                            crate::bindings::claims(vk, modifiers.0, modifiers.1, modifiers.2);
                        if claimed {
                            args.SetHandled(true)?;
                        }
                        evidence.borrow_mut().accelerators.push(Accelerator {
                            vk,
                            kind,
                            ctrl: modifiers.0,
                            shift: modifiers.1,
                            alt: modifiers.2,
                            repeat: status.RepeatCount > 1,
                            handled: claimed,
                        });
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_AcceleratorKeyPressed")?;

            let evidence = Rc::clone(&self.evidence);
            self.controller
                .add_MoveFocusRequested(
                    &MoveFocusRequestedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let reason =
                            read::<COREWEBVIEW2_MOVE_FOCUS_REASON>(|out| args.Reason(out)).0;
                        // The host takes the focus back: this is the Tab contract.
                        args.SetHandled(true)?;
                        evidence.borrow_mut().move_focus_requested.push(reason);
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_MoveFocusRequested")?;

            let evidence = Rc::clone(&self.evidence);
            self.controller
                .add_GotFocus(
                    &FocusChangedEventHandler::create(Box::new(move |_, _| {
                        evidence.borrow_mut().focus_changed.push("got");
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_GotFocus")?;
            let evidence = Rc::clone(&self.evidence);
            self.controller
                .add_LostFocus(
                    &FocusChangedEventHandler::create(Box::new(move |_, _| {
                        evidence.borrow_mut().focus_changed.push("lost");
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_LostFocus")?;

            // ── cursor ────────────────────────────────────────────────────
            let evidence = Rc::clone(&self.evidence);
            self.composition
                .add_CursorChanged(
                    &CursorChangedEventHandler::create(Box::new(move |controller, _| {
                        if let Some(controller) = controller {
                            let mut id = 0u32;
                            let _ = controller.SystemCursorId(&mut id);
                            evidence.borrow_mut().cursor_changed.push(id);
                        }
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_CursorChanged")?;

            // ── the page's own voice ──────────────────────────────────────
            let evidence = Rc::clone(&self.evidence);
            self.webview
                .add_WebMessageReceived(
                    &WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let json = read_string(|out| args.WebMessageAsJson(out));
                        evidence.borrow_mut().web_messages.push(json);
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_WebMessageReceived")?;
        }
        Ok(())
    }

    /// Environment-level events. Separate from the controller's because they
    /// outlive any one controller — `BrowserProcessExited` is the signal a user
    /// data folder may finally be deleted.
    pub fn attach_environment_events(&self) -> Result<()> {
        let mut token = 0i64;
        unsafe {
            let environment5: ICoreWebView2Environment5 = self
                .environment
                .cast()
                .context("ICoreWebView2Environment5")?;
            let evidence = Rc::clone(&self.evidence);
            environment5
                .add_BrowserProcessExited(
                    &BrowserProcessExitedEventHandler::create(Box::new(move |_, args| {
                        let kind = args
                            .map(|args| {
                                read::<COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND>(|out| {
                                    args.BrowserProcessExitKind(out)
                                })
                                .0
                            })
                            .unwrap_or(-1);
                        evidence.borrow_mut().browser_exited.push(kind);
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_BrowserProcessExited")?;
            let evidence = Rc::clone(&self.evidence);
            self.environment
                .add_NewBrowserVersionAvailable(
                    &NewBrowserVersionAvailableEventHandler::create(Box::new(move |_, _| {
                        evidence.borrow_mut().new_version_available += 1;
                        Ok(())
                    })),
                    &mut token,
                )
                .context("add_NewBrowserVersionAvailable")?;
        }
        Ok(())
    }

    /// Point the controller's rendering at our visual.
    pub fn set_root_visual(&self, visual: &windows::core::IUnknown) -> Result<()> {
        unsafe {
            self.composition
                .SetRootVisualTarget(visual)
                .context("SetRootVisualTarget")
        }
    }

    pub fn set_seat(&mut self, seat: RECT, origin: BoundsOrigin) -> Result<()> {
        self.seat = seat;
        self.origin = origin;
        let bounds = match origin {
            BoundsOrigin::AtSeat => seat,
            BoundsOrigin::AtZero => RECT {
                left: 0,
                top: 0,
                right: seat.right - seat.left,
                bottom: seat.bottom - seat.top,
            },
        };
        unsafe { self.controller.SetBounds(bounds).context("SetBounds") }
    }

    /// Client-area point → the coordinate space `SendMouseInput` expects.
    pub fn to_webview_point(&self, client: POINT) -> POINT {
        match self.origin {
            BoundsOrigin::AtSeat => client,
            BoundsOrigin::AtZero => POINT {
                x: client.x - self.seat.left,
                y: client.y - self.seat.top,
            },
        }
    }

    pub fn set_visible(&self, visible: bool) -> Result<()> {
        unsafe {
            self.controller
                .SetIsVisible(visible)
                .context("SetIsVisible")
        }
    }

    pub fn navigate(&self, url: &str) -> Result<()> {
        unsafe {
            self.webview
                .Navigate(&HSTRING::from(url))
                .with_context(|| format!("Navigate({url})"))
        }
    }

    pub fn reload(&self) -> Result<()> {
        unsafe { self.webview.Reload().context("Reload") }
    }

    pub fn source(&self) -> String {
        read_string(|out| unsafe { self.webview.Source(out) })
    }

    pub fn move_focus_into_web(&self) -> Result<()> {
        unsafe {
            self.controller
                .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)
                .context("MoveFocus")
        }
    }

    pub fn browser_process_id(&self) -> u32 {
        read::<u32>(|out| unsafe { self.webview.BrowserProcessId(out) })
    }

    /// Run a script and wait for its JSON result.
    pub fn execute_script(&self, script: &str, timeout: Duration) -> Option<String> {
        let holder: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&holder);
        let handler = ExecuteScriptCompletedHandler::create(Box::new(move |result, json| {
            if result.is_ok() {
                *sink.borrow_mut() = Some(json);
            }
            Ok(())
        }));
        if unsafe { self.webview.ExecuteScript(&HSTRING::from(script), &handler) }.is_err() {
            return None;
        }
        crate::win::pump_until(timeout, || holder.borrow().is_some());
        holder.borrow_mut().take()
    }

    /// `CapturePreview` into a file, timed end to end.
    ///
    /// The clock starts before the call and stops in the completion handler, so
    /// what it measures is exactly what the plan calls "readback + encode +
    /// round-trip" — not the render.
    pub fn capture_preview(&self, path: &Path, timeout: Duration) -> Result<Duration> {
        use windows::Win32::System::Com::{IStream, STGM_CREATE, STGM_READWRITE};
        use windows::Win32::UI::Shell::SHCreateStreamOnFileEx;
        let stream: IStream = unsafe {
            SHCreateStreamOnFileEx(
                &HSTRING::from(path.as_os_str()),
                (STGM_CREATE | STGM_READWRITE).0,
                windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL.0,
                true,
                None,
            )
        }
        .context("SHCreateStreamOnFileEx")?;
        let done: Rc<RefCell<Option<Duration>>> = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&done);
        let started = Instant::now();
        let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
            result?;
            *sink.borrow_mut() = Some(started.elapsed());
            Ok(())
        }));
        unsafe {
            self.webview
                .CapturePreview(
                    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                    &stream,
                    &handler,
                )
                .context("CapturePreview")?;
        }
        crate::win::pump_until(timeout, || done.borrow().is_some());
        let elapsed = done
            .borrow_mut()
            .take()
            .context("CapturePreview did not complete inside the timeout")?;
        drop(stream);
        Ok(elapsed)
    }

    /// `CapturePreview` into **memory**, with the two clocks kept apart.
    ///
    /// [`Self::capture_preview`] answers one number — how long the engine takes
    /// to hand a picture back — and that number is a *latency*. A caller with a
    /// frame budget needs the other one: how much of its own thread the ask
    /// costs. So this method times the synchronous call on its own, times the
    /// wait separately, and reads the bytes out of an `HGLOBAL` stream rather
    /// than a file, because a thumbnail that went through the disk would be
    /// measuring the disk.
    pub fn capture_preview_to_memory(
        &self,
        format: COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT,
        timeout: Duration,
    ) -> Result<CaptureTiming> {
        use windows::Win32::Foundation::HGLOBAL;
        use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
        use windows::Win32::System::Com::{STREAM_SEEK_END, STREAM_SEEK_SET};
        let stream = unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) }
            .context("CreateStreamOnHGlobal")?;
        let done: Rc<RefCell<Option<Duration>>> = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&done);
        let started = Instant::now();
        let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
            result?;
            *sink.borrow_mut() = Some(started.elapsed());
            Ok(())
        }));
        let issued = Instant::now();
        unsafe {
            self.webview
                .CapturePreview(format, &stream, &handler)
                .context("CapturePreview")?;
        }
        let issue = issued.elapsed();
        crate::win::pump_until(timeout, || done.borrow().is_some());
        let complete = done
            .borrow_mut()
            .take()
            .context("CapturePreview did not complete inside the timeout")?;
        let read_started = Instant::now();
        let mut length = 0u64;
        unsafe { stream.Seek(0, STREAM_SEEK_END, Some(&mut length)) }.context("Seek(end)")?;
        unsafe { stream.Seek(0, STREAM_SEEK_SET, None) }.context("Seek(set)")?;
        let mut bytes = vec![0u8; length as usize];
        let mut read = 0u32;
        unsafe {
            stream.Read(
                bytes.as_mut_ptr().cast(),
                bytes.len() as u32,
                Some(&mut read),
            )
        }
        .ok()
        .context("Read")?;
        bytes.truncate(read as usize);
        let read_back = read_started.elapsed();
        drop(stream);
        Ok(CaptureTiming {
            issue,
            complete,
            read_back,
            bytes,
        })
    }

    /// Start a `CapturePreview` and **do not wait for it**.
    ///
    /// The shape a window can actually use: the ask is made on one frame, the
    /// frame ends, and the answer lands on whatever later pump the completion
    /// happens to fall in. [`Self::capture_preview_to_memory`] blocks, which is
    /// the arrangement no frame loop can have, and measuring only that one would
    /// price a design nobody would build.
    pub fn begin_capture_preview(
        &self,
        format: COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT,
    ) -> Result<CaptureInFlight> {
        use windows::Win32::Foundation::HGLOBAL;
        use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
        let stream = unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) }
            .context("CreateStreamOnHGlobal")?;
        let done: Rc<RefCell<Option<Duration>>> = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&done);
        let started = Instant::now();
        let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
            result?;
            *sink.borrow_mut() = Some(started.elapsed());
            Ok(())
        }));
        let issued = Instant::now();
        unsafe {
            self.webview
                .CapturePreview(format, &stream, &handler)
                .context("CapturePreview")?;
        }
        Ok(CaptureInFlight {
            issue: issued.elapsed(),
            done,
            stream,
        })
    }

    pub fn close(&self) {
        let _ = unsafe { self.controller.Close() };
    }
}

/// A capture that has been asked for and not yet answered.
pub struct CaptureInFlight {
    /// What the ask cost the asking thread.
    pub issue: Duration,
    done: Rc<RefCell<Option<Duration>>>,
    stream: windows::Win32::System::Com::IStream,
}

impl CaptureInFlight {
    /// The latency, once the completion handler has run — `None` while it has
    /// not. Asked on the frame clock, which is the only clock a window has.
    pub fn settled(&self) -> Option<Duration> {
        *self.done.borrow()
    }

    /// The encoded bytes, read out of the stream. Only meaningful once
    /// [`Self::settled`] has answered.
    pub fn bytes(&self) -> Result<Vec<u8>> {
        use windows::Win32::System::Com::{STREAM_SEEK_END, STREAM_SEEK_SET};
        let mut length = 0u64;
        unsafe { self.stream.Seek(0, STREAM_SEEK_END, Some(&mut length)) }.context("Seek(end)")?;
        unsafe { self.stream.Seek(0, STREAM_SEEK_SET, None) }.context("Seek(set)")?;
        let mut bytes = vec![0u8; length as usize];
        let mut read = 0u32;
        unsafe {
            self.stream.Read(
                bytes.as_mut_ptr().cast(),
                bytes.len() as u32,
                Some(&mut read),
            )
        }
        .ok()
        .context("Read")?;
        bytes.truncate(read as usize);
        Ok(bytes)
    }
}

/// One `CapturePreview`, in the three clocks a frame budget cares about.
pub struct CaptureTiming {
    /// The synchronous call: what the asking thread actually pays to ask.
    pub issue: Duration,
    /// Ask to completion handler — the engine's latency, and the number the
    /// pixel matrix already recorded.
    pub complete: Duration,
    /// Getting the encoded bytes back out of the stream afterwards.
    pub read_back: Duration,
    pub bytes: Vec<u8>,
}

// ── Mouse and pointer forwarding ───────────────────────────────────────────

/// Everything `SendMouseInput` can be asked to deliver, named the way a reader
/// of the gate table names it.
#[derive(Clone, Copy, Debug)]
pub enum MouseEvent {
    Move,
    Leave,
    LeftDown,
    LeftUp,
    LeftDoubleClick,
    RightDown,
    RightUp,
    MiddleDown,
    MiddleUp,
    XDown(u16),
    XUp(u16),
    Wheel(i16),
    HorizontalWheel(i16),
}

impl MouseEvent {
    fn kind(self) -> COREWEBVIEW2_MOUSE_EVENT_KIND {
        match self {
            Self::Move => COREWEBVIEW2_MOUSE_EVENT_KIND_MOVE,
            Self::Leave => COREWEBVIEW2_MOUSE_EVENT_KIND_LEAVE,
            Self::LeftDown => COREWEBVIEW2_MOUSE_EVENT_KIND_LEFT_BUTTON_DOWN,
            Self::LeftUp => COREWEBVIEW2_MOUSE_EVENT_KIND_LEFT_BUTTON_UP,
            Self::LeftDoubleClick => COREWEBVIEW2_MOUSE_EVENT_KIND_LEFT_BUTTON_DOUBLE_CLICK,
            Self::RightDown => COREWEBVIEW2_MOUSE_EVENT_KIND_RIGHT_BUTTON_DOWN,
            Self::RightUp => COREWEBVIEW2_MOUSE_EVENT_KIND_RIGHT_BUTTON_UP,
            Self::MiddleDown => COREWEBVIEW2_MOUSE_EVENT_KIND_MIDDLE_BUTTON_DOWN,
            Self::MiddleUp => COREWEBVIEW2_MOUSE_EVENT_KIND_MIDDLE_BUTTON_UP,
            Self::XDown(_) => COREWEBVIEW2_MOUSE_EVENT_KIND_X_BUTTON_DOWN,
            Self::XUp(_) => COREWEBVIEW2_MOUSE_EVENT_KIND_X_BUTTON_UP,
            Self::Wheel(_) => COREWEBVIEW2_MOUSE_EVENT_KIND_WHEEL,
            Self::HorizontalWheel(_) => COREWEBVIEW2_MOUSE_EVENT_KIND_HORIZONTAL_WHEEL,
        }
    }

    /// `mouseData`: the wheel delta, or which X button, or zero.
    fn data(self) -> u32 {
        match self {
            Self::Wheel(delta) | Self::HorizontalWheel(delta) => delta as i32 as u32,
            Self::XDown(button) | Self::XUp(button) => u32::from(button),
            _ => 0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Leave => "leave",
            Self::LeftDown => "left-down",
            Self::LeftUp => "left-up",
            Self::LeftDoubleClick => "left-double-click",
            Self::RightDown => "right-down",
            Self::RightUp => "right-up",
            Self::MiddleDown => "middle-down",
            Self::MiddleUp => "middle-up",
            Self::XDown(_) => "x-down",
            Self::XUp(_) => "x-up",
            Self::Wheel(_) => "wheel",
            Self::HorizontalWheel(_) => "horizontal-wheel",
        }
    }
}

impl Host {
    /// Forward one mouse event. `client` is in the host window's client
    /// coordinates; the translation into the WebView's own space happens here
    /// and nowhere else.
    pub fn send_mouse(&self, event: MouseEvent, client: POINT, buttons_down: u32) -> Result<()> {
        let point = self.to_webview_point(client);
        unsafe {
            self.composition
                .SendMouseInput(
                    event.kind(),
                    COREWEBVIEW2_MOUSE_EVENT_VIRTUAL_KEYS(buttons_down as i32),
                    event.data(),
                    point,
                )
                .with_context(|| format!("SendMouseInput({})", event.name()))
        }
    }

    /// Forward one pointer event — the pen/touch path, which is a different
    /// entry point entirely and needs a `ICoreWebView2PointerInfo` the
    /// environment mints.
    ///
    /// `pointer_id` is the contact. **One `ICoreWebView2PointerInfo` per
    /// contact, each with its own id** — a host that mints one id and moves it
    /// around has a mouse with a different name, and every second finger it
    /// sends replaces the first.
    pub fn send_pointer(
        &self,
        kind: COREWEBVIEW2_POINTER_EVENT_KIND,
        pointer_kind: u32,
        pointer_id: u32,
        client: POINT,
        pressure: u32,
        flags: u32,
    ) -> Result<()> {
        let environment3: ICoreWebView2Environment3 = self
            .environment
            .cast()
            .context("ICoreWebView2Environment3")?;
        let info = unsafe { environment3.CreateCoreWebView2PointerInfo() }
            .context("CreateCoreWebView2PointerInfo")?;
        let point = self.to_webview_point(client);
        let frame_id = self.frame_id.get();
        let parent = read::<HWND>(|out| unsafe { self.controller.ParentWindow(out) });
        let screen = crate::win::client_to_screen(parent, client);
        // The device and display rectangles are what a real digitizer would
        // report; a pointer message with empty ones is rejected before it
        // reaches the page.
        let display = RECT {
            left: screen.x - point.x,
            top: screen.y - point.y,
            right: screen.x - point.x + (self.seat.right - self.seat.left),
            bottom: screen.y - point.y + (self.seat.bottom - self.seat.top),
        };
        unsafe {
            info.SetPointerKind(pointer_kind)?;
            info.SetPointerId(pointer_id)?;
            // Contacts that belong to the same frame share a frame id; that is
            // how the engine knows two fingers are simultaneous rather than
            // sequential.
            info.SetFrameId(frame_id)?;
            info.SetPointerFlags(flags)?;
            info.SetPointerDeviceRect(display)?;
            info.SetDisplayRect(display)?;
            info.SetPixelLocation(point)?;
            info.SetPixelLocationRaw(point)?;
            info.SetTime(0)?;
            info.SetHistoryCount(1)?;
            match pointer_kind {
                // PT_PEN
                3 => {
                    // PEN_MASK_PRESSURE
                    info.SetPenMask(0x0000_0001)?;
                    info.SetPenPressure(pressure)?;
                }
                // PT_TOUCH
                2 => {
                    // TOUCH_MASK_PRESSURE
                    info.SetTouchMask(0x0000_0004)?;
                    info.SetTouchPressure(pressure)?;
                    info.SetTouchContact(RECT {
                        left: point.x - 4,
                        top: point.y - 4,
                        right: point.x + 4,
                        bottom: point.y + 4,
                    })?;
                }
                _ => {}
            }
            self.composition
                .SendPointerInput(kind, &info)
                .context("SendPointerInput")
        }
    }
}

/// Which modifiers are physically down right now. `AcceleratorKeyPressed` hands
/// over the key but not the modifier state, so the host has to read it.
fn current_modifiers() -> (bool, bool, bool) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
    let down = |vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| {
        (unsafe { GetKeyState(i32::from(vk.0)) } as u16 & 0x8000) != 0
    };
    (down(VK_CONTROL), down(VK_SHIFT), down(VK_MENU))
}
