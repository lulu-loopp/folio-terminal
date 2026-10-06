//! Linux display queries from the selected native window backend.

use std::collections::HashSet;
use std::env;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::JoinHandle;

use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::ErrorKind;
use x11rb::protocol::randr::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, Atom, AtomEnum, ConnectionExt as _, Window};
use x11rb::resource_manager;
use x11rb::rust_connection::RustConnection;

use crate::linux_window::Backend;
use crate::{NativeWindow, WindowRect};

static BACKEND: OnceLock<Backend> = OnceLock::new();
static X11_SESSION: OnceLock<Mutex<Option<Arc<X11Session>>>> = OnceLock::new();

const EMPTY_RECT: WindowRect = WindowRect {
    left: 0,
    top: 0,
    right: 0,
    bottom: 0,
};
const PROPERTY_LONGS: u32 = 16 * 1024;

/// Install the backend selected from the live winit window handle.
pub fn install_backend(backend: Backend) -> Result<(), String> {
    if let Some(installed) = BACKEND.get() {
        return if *installed == backend {
            Ok(())
        } else {
            Err("the Linux display backend cannot change after window creation".to_owned())
        };
    }
    BACKEND
        .set(backend)
        .map_err(|_| "the Linux display backend has already been installed".to_owned())
}

#[must_use]
pub fn active_backend() -> Option<Backend> {
    BACKEND.get().copied()
}

/// One X11 display query delivered back to the window loop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinuxDisplayReady {
    /// The window-side owner token supplied with the request.
    pub owner: u64,
    /// The request within that owner's operation generation.
    pub request_id: u64,
    /// The owner's generation at submission time.
    pub generation: u64,
}

/// A query whose X11 connection and replies are owned by the display worker.
#[derive(Clone, Debug)]
pub enum LinuxDisplayQuery {
    /// Read the same point, work area, monitor name and scale used to summon the quake window.
    SummonScreen {
        window: NativeWindow,
        cached_dpi: u32,
    },
    /// Use the root point carried by the activating X11 key press.
    SummonScreenAt {
        window: NativeWindow,
        cached_dpi: u32,
        x: i32,
        y: i32,
    },
    /// Read the scale and work area used to place a torn out window at this point.
    TearOutScreen { x: i32, y: i32 },
    /// Read the rectangle, display name and scale used to remember a hand placed summon.
    SummonedArrangement { window: NativeWindow },
    /// Read the current pointer position in one window's client coordinates.
    PointerInWindow { window: NativeWindow },
    /// Read a window rectangle; callers retain their existing winit fallback on refusal.
    WindowRect { window: NativeWindow },
    /// Read the native work area for one window.
    WindowWorkArea { window: NativeWindow },
}

/// One answer from the X11 display worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinuxDisplayAnswer {
    /// The summon query's matched display facts, including its existing fallbacks.
    SummonScreen {
        work: WindowRect,
        monitor_id: Option<String>,
        dpi: u32,
    },
    /// The tear out query's work area and scale at the captured pointer point.
    TearOutScreen { work: WindowRect, dpi: u32 },
    /// The new arrangement if native geometry and a display name were available.
    SummonedArrangement(Option<(WindowRect, String, u32)>),
    /// The actual pointer in the window, or no answer from the platform.
    PointerInWindow(Option<(i32, i32)>),
    /// Native geometry, or the same refusal the old synchronous query returned.
    WindowRect(Result<LinuxWindowFacts, String>),
    /// The work-area read's native answer or its original refusal.
    WindowWorkArea(Result<WindowRect, String>),
}

/// The native facts read together for one X11 window.
///
/// `None` means the window manager did not provide a valid `_NET_WM_STATE` property; it is not
/// evidence that the window is normal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinuxWindowFacts {
    /// The rectangle observed by the display worker.
    pub rect: WindowRect,
    /// Whether both maximize state atoms were present, or absent.
    pub maximized: Option<bool>,
    /// Whether the hidden state atom was present, or absent.
    pub minimized: Option<bool>,
}

/// A pending query. Its receiver never waits; the matching ready event means its answer is parked.
pub struct LinuxDisplayRequest {
    ready: LinuxDisplayReady,
    answer: mpsc::Receiver<LinuxDisplayAnswer>,
}

impl LinuxDisplayRequest {
    /// The event-loop correlation id for this request.
    #[must_use]
    pub const fn ready(&self) -> LinuxDisplayReady {
        self.ready
    }

    /// Take the worker's answer after its ready event, without waiting.
    pub fn try_take(&self) -> Result<LinuxDisplayAnswer, mpsc::TryRecvError> {
        self.answer.try_recv()
    }
}

type DisplayWake = Box<dyn Fn(LinuxDisplayReady) + Send + Sync + 'static>;
static DISPLAY_WAKE: OnceLock<DisplayWake> = OnceLock::new();
static DISPLAY_SERVICE: OnceLock<Mutex<Option<DisplayService>>> = OnceLock::new();
static DISPLAY_STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
static DISPLAY_WORKER_STOPPED: AtomicBool = AtomicBool::new(false);
static NEXT_DISPLAY_REQUEST: AtomicU64 = AtomicU64::new(0);
const DISPLAY_THREAD: &str = "bt-linux-display";

struct DisplayService {
    sender: mpsc::Sender<DisplayCommand>,
    _worker: JoinHandle<()>,
}

enum DisplayCommand {
    Stop,
    Query {
        query: LinuxDisplayQuery,
        ready: LinuxDisplayReady,
        answer: mpsc::Sender<LinuxDisplayAnswer>,
    },
}

/// Stop accepting display queries without waiting for the worker to finish.
///
/// A worker already blocked on X11 drops its pending answer when that call returns. The
/// desktop-retirement owner can poll [`display_service_stopped`] within its existing budget;
/// this function never joins the worker.
pub fn stop_display_service() {
    DISPLAY_STOP_REQUESTED.store(true, Ordering::Release);
    let Some(service) = DISPLAY_SERVICE.get() else {
        DISPLAY_WORKER_STOPPED.store(true, Ordering::Release);
        return;
    };
    let current = service
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(service) = current.as_ref() {
        let _ = service.sender.send(DisplayCommand::Stop);
    } else {
        DISPLAY_WORKER_STOPPED.store(true, Ordering::Release);
    }
}

/// Whether the display worker has exited after [`stop_display_service`].
#[must_use]
pub fn display_service_stopped() -> bool {
    DISPLAY_STOP_REQUESTED.load(Ordering::Acquire) && DISPLAY_WORKER_STOPPED.load(Ordering::Acquire)
}

/// Install the event-loop wake before any Linux display query can be submitted.
pub fn install_display_wake(
    wake: impl Fn(LinuxDisplayReady) + Send + Sync + 'static,
) -> Result<(), String> {
    DISPLAY_WAKE
        .set(Box::new(wake))
        .map_err(|_| "the Linux display wake has already been installed".to_owned())
}

/// Submit one addressed query without waiting for X11 connection setup or a server reply.
pub fn request_display(
    owner: u64,
    generation: u64,
    query: LinuxDisplayQuery,
) -> Result<LinuxDisplayRequest, String> {
    if DISPLAY_STOP_REQUESTED.load(Ordering::Acquire) {
        return Err("the Linux display worker is stopping".to_owned());
    }
    if BACKEND.get().is_none() {
        return Err("the Linux display backend has not been installed".to_owned());
    }
    if DISPLAY_WAKE.get().is_none() {
        return Err("the Linux display event-loop wake is not installed".to_owned());
    }
    let sender = display_sender()?;
    if DISPLAY_STOP_REQUESTED.load(Ordering::Acquire) {
        return Err("the Linux display worker is stopping".to_owned());
    }
    let request_id = NEXT_DISPLAY_REQUEST
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1);
    let ready = LinuxDisplayReady {
        owner,
        request_id,
        generation,
    };
    let (answer, receiver) = mpsc::channel();
    sender
        .send(DisplayCommand::Query {
            query,
            ready,
            answer,
        })
        .map_err(|_| "the Linux display worker stopped".to_owned())?;
    if DISPLAY_STOP_REQUESTED.load(Ordering::Acquire) {
        return Err("the Linux display worker is stopping".to_owned());
    }
    Ok(LinuxDisplayRequest {
        ready,
        answer: receiver,
    })
}

fn display_sender() -> Result<mpsc::Sender<DisplayCommand>, String> {
    if DISPLAY_STOP_REQUESTED.load(Ordering::Acquire) {
        return Err("the Linux display worker is stopping".to_owned());
    }
    let service = DISPLAY_SERVICE.get_or_init(|| Mutex::new(None));
    let mut current = service
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if DISPLAY_STOP_REQUESTED.load(Ordering::Acquire) {
        return Err("the Linux display worker is stopping".to_owned());
    }
    if let Some(service) = current.as_ref() {
        return Ok(service.sender.clone());
    }

    let (sender, receiver) = mpsc::channel();
    let worker = crate::spawn_at_priority(
        DISPLAY_THREAD,
        crate::ThreadPriority::BelowNormal,
        move |ctx| display_worker(receiver, ctx),
    )
    .map_err(|error| format!("could not start the Linux display worker: {error}"))?;
    *current = Some(DisplayService {
        sender: sender.clone(),
        _worker: worker,
    });
    Ok(sender)
}

fn display_worker(receiver: mpsc::Receiver<DisplayCommand>, _worker: &crate::admission::WorkerCtx) {
    loop {
        if DISPLAY_STOP_REQUESTED.load(Ordering::Acquire) {
            break;
        }
        let command = match receiver.recv() {
            Ok(command) => command,
            Err(_) => break,
        };
        if DISPLAY_STOP_REQUESTED.load(Ordering::Acquire) {
            break;
        }
        let DisplayCommand::Query {
            query,
            ready,
            answer,
        } = command
        else {
            break;
        };
        let result = run_display_query(query);
        if DISPLAY_STOP_REQUESTED.load(Ordering::Acquire) {
            break;
        }
        let _ = answer.send(result);
        if DISPLAY_STOP_REQUESTED.load(Ordering::Acquire) {
            break;
        }
        if let Some(wake) = DISPLAY_WAKE.get() {
            wake(ready);
        }
    }
    DISPLAY_WORKER_STOPPED.store(true, Ordering::Release);
}

fn summon_screen_answer(
    window: NativeWindow,
    cached_dpi: u32,
    pointer: Option<(i32, i32)>,
) -> LinuxDisplayAnswer {
    let work = pointer
        .and_then(|(x, y)| work_area_at(x, y).ok())
        .or_else(|| get_work_area(window).ok())
        .unwrap_or_else(virtual_screen_rect);
    LinuxDisplayAnswer::SummonScreen {
        work,
        monitor_id: pointer.and_then(|(x, y)| monitor_id_at(x, y)),
        dpi: pointer.map_or(cached_dpi, |(x, y)| dpi_at(x, y)),
    }
}

fn run_display_query(query: LinuxDisplayQuery) -> LinuxDisplayAnswer {
    match query {
        LinuxDisplayQuery::SummonScreen { window, cached_dpi } => {
            summon_screen_answer(window, cached_dpi, pointer_position())
        }
        LinuxDisplayQuery::SummonScreenAt {
            window,
            cached_dpi,
            x,
            y,
        } => summon_screen_answer(window, cached_dpi, Some((x, y))),
        LinuxDisplayQuery::TearOutScreen { x, y } => LinuxDisplayAnswer::TearOutScreen {
            dpi: dpi_at(x, y),
            work: work_area_at(x, y).unwrap_or_else(|_| virtual_screen_rect()),
        },
        LinuxDisplayQuery::SummonedArrangement { window } => {
            let arrangement = get_window_rect(window).ok().and_then(|rect| {
                monitor_id_at(rect.left, rect.top)
                    .map(|monitor| (rect, monitor, dpi_at(rect.left, rect.top)))
            });
            LinuxDisplayAnswer::SummonedArrangement(arrangement)
        }
        LinuxDisplayQuery::PointerInWindow { window } => {
            LinuxDisplayAnswer::PointerInWindow(pointer_position_in_window(window))
        }
        LinuxDisplayQuery::WindowRect { window } => {
            LinuxDisplayAnswer::WindowRect(get_window_facts(window))
        }
        LinuxDisplayQuery::WindowWorkArea { window } => {
            LinuxDisplayAnswer::WindowWorkArea(get_work_area(window))
        }
    }
}

#[must_use]
pub fn pointer_position() -> Option<(i32, i32)> {
    with_x11("reading the pointer position", |session| {
        let reply = session
            .connection
            .query_pointer(session.root)
            .map_err(|error| request_error("querying the X11 pointer", error))?
            .reply()
            .map_err(|error| reply_error("querying the X11 pointer", error))?;
        Ok(reply
            .same_screen
            .then_some((i32::from(reply.root_x), i32::from(reply.root_y))))
    })
    .ok()
    .flatten()
}

#[must_use]
pub fn pointer_position_in_window(window: NativeWindow) -> Option<(i32, i32)> {
    with_x11("reading the pointer inside an X11 window", |session| {
        let reply = session
            .connection
            .query_pointer(window.as_x11_window())
            .map_err(|error| request_error("querying the X11 window pointer", error))?
            .reply()
            .map_err(|error| reply_error("querying the X11 window pointer", error))?;
        Ok(reply
            .same_screen
            .then_some((i32::from(reply.win_x), i32::from(reply.win_y))))
    })
    .ok()
    .flatten()
}

pub fn get_window_rect(window: NativeWindow) -> Result<WindowRect, String> {
    with_x11("reading a window's rectangle", |session| {
        window_rect(session, window.as_x11_window())
    })
}

/// Read a window's geometry and EWMH posture on the display worker.
pub fn get_window_facts(window: NativeWindow) -> Result<LinuxWindowFacts, String> {
    with_x11("reading a window's rectangle and state", |session| {
        let rect = window_rect(session, window.as_x11_window())?;
        let (maximized, minimized) = window_state_facts(session, window.as_x11_window());
        Ok(LinuxWindowFacts {
            rect,
            maximized,
            minimized,
        })
    })
}

fn window_state_facts(session: &X11Session, window: Window) -> (Option<bool>, Option<bool>) {
    let values = match read_window_property32(
        session,
        window,
        session.atoms.net_wm_state,
        AtomEnum::ATOM.into(),
        "_NET_WM_STATE",
    ) {
        Ok(Some(values)) => values,
        Ok(None) => return (Some(false), Some(false)),
        Err(_) => return (None, None),
    };
    window_state_from_atoms(
        &values,
        session.atoms.net_wm_state_hidden,
        session.atoms.net_wm_state_maximized_horz,
        session.atoms.net_wm_state_maximized_vert,
    )
}

fn window_state_from_atoms(
    values: &[Atom],
    hidden: Atom,
    maximized_horz: Atom,
    maximized_vert: Atom,
) -> (Option<bool>, Option<bool>) {
    let horizontal = values.contains(&maximized_horz);
    let vertical = values.contains(&maximized_vert);
    let maximized = (horizontal == vertical).then_some(horizontal);
    (maximized, Some(values.contains(&hidden)))
}
pub fn get_work_area(window: NativeWindow) -> Result<WindowRect, String> {
    with_x11("reading a window's display work area", |session| {
        let (bounds, root) = window_rect_and_root(session, window.as_x11_window())?;
        let x = midpoint(bounds.left, bounds.right);
        let y = midpoint(bounds.top, bounds.bottom);
        work_area_for_point(session, root, x, y)
    })
}

pub fn work_area_at(x: i32, y: i32) -> Result<WindowRect, String> {
    with_x11("reading the work area at a point", |session| {
        work_area_for_point(session, session.root, x, y)
    })
}

#[must_use]
pub fn virtual_screen_rect() -> WindowRect {
    with_x11("reading the virtual screen rectangle", |session| {
        let monitors = monitor_list(session, session.root)?;
        Ok(union_rectangles(
            monitors.iter().map(|monitor| monitor.bounds),
        ))
    })
    .unwrap_or(EMPTY_RECT)
}

#[must_use]
pub fn monitor_id_at(x: i32, y: i32) -> Option<String> {
    with_x11("naming the display at a point", |session| {
        let monitors = monitor_list(session, session.root)?;
        Ok(nearest_monitor(&monitors, x, y).and_then(|monitor| monitor.name.clone()))
    })
    .ok()
    .flatten()
}

#[must_use]
pub fn dpi_at(x: i32, y: i32) -> u32 {
    with_x11("reading the display scale at a point", |session| {
        let monitors = monitor_list(session, session.root)?;
        let Some(monitor) = nearest_monitor(&monitors, x, y) else {
            return Ok(96);
        };
        let factor = scale_factor(session, monitor).unwrap_or(1.0);
        Ok((factor * 96.0).round().clamp(1.0, f64::from(u32::MAX)) as u32)
    })
    .unwrap_or(96)
}

fn with_x11<T>(
    operation: &str,
    query: impl FnOnce(&X11Session) -> Result<T, String>,
) -> Result<T, String> {
    let backend = BACKEND
        .get()
        .copied()
        .ok_or_else(|| "the Linux display backend has not been installed".to_owned())?;
    if backend == Backend::Wayland {
        return Err(match operation {
            "reading a window's rectangle" => {
                "Wayland does not expose a window's global position".to_owned()
            }
            "reading a window's display work area" => {
                "Wayland does not expose a window's global display or work area".to_owned()
            }
            "reading the work area at a point" => {
                "Wayland does not expose global coordinates or point-based work areas".to_owned()
            }
            _ => format!("{operation}: X11 display queries are unavailable on Wayland"),
        });
    }

    let session_slot = X11_SESSION.get_or_init(|| Mutex::new(None));
    let session = {
        let mut slot = session_slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(session) = slot.as_ref() {
            Arc::clone(session)
        } else {
            let session = Arc::new(X11Session::connect()?);
            *slot = Some(Arc::clone(&session));
            session
        }
    };
    query(&session)
}

struct X11Session {
    connection: RustConnection,
    root: Window,
    xsettings_selection: Atom,
    randr_version: Option<(u32, u32)>,
    atoms: Atoms,
}

impl X11Session {
    fn connect() -> Result<Self, String> {
        let (connection, screen) = RustConnection::connect(None)
            .map_err(|error| format!("could not connect to the configured X11 server: {error}"))?;
        let root = connection
            .setup()
            .roots
            .get(screen)
            .ok_or_else(|| "the X11 connection has no default screen".to_owned())?
            .root;
        let atoms = Atoms::new(&connection)?;
        let selection_name = format!("_XSETTINGS_S{screen}");
        let xsettings_selection = connection
            .intern_atom(false, selection_name.as_bytes())
            .map_err(|error| request_error("interning the XSettings selection", error))?
            .reply()
            .map_err(|error| reply_error("interning the XSettings selection", error))?
            .atom;
        let randr_version = connection
            .randr_query_version(1, 3)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| (reply.major_version, reply.minor_version));
        Ok(Self {
            connection,
            root,
            xsettings_selection,
            randr_version,
            atoms,
        })
    }
}

#[derive(Clone, Copy)]
struct Atoms {
    net_client_list: Atom,
    net_current_desktop: Atom,
    net_desktop_viewport: Atom,
    net_frame_extents: Atom,
    net_number_of_desktops: Atom,
    net_supporting_wm_check: Atom,
    net_workarea: Atom,
    net_wm_desktop: Atom,
    net_wm_strut: Atom,
    net_wm_strut_partial: Atom,
    net_wm_window_type: Atom,
    net_wm_window_type_dock: Atom,
    resource_manager: Atom,
    xsettings_settings: Atom,
    net_wm_state: Atom,
    net_wm_state_hidden: Atom,
    net_wm_state_maximized_horz: Atom,
    net_wm_state_maximized_vert: Atom,
}

impl Atoms {
    fn new(connection: &RustConnection) -> Result<Self, String> {
        let names = [
            "_NET_CLIENT_LIST",
            "_NET_CURRENT_DESKTOP",
            "_NET_DESKTOP_VIEWPORT",
            "_NET_FRAME_EXTENTS",
            "_NET_NUMBER_OF_DESKTOPS",
            "_NET_SUPPORTING_WM_CHECK",
            "_NET_WORKAREA",
            "_NET_WM_DESKTOP",
            "_NET_WM_STRUT",
            "_NET_WM_STRUT_PARTIAL",
            "_NET_WM_WINDOW_TYPE",
            "_NET_WM_WINDOW_TYPE_DOCK",
            "RESOURCE_MANAGER",
            "_XSETTINGS_SETTINGS",
            "_NET_WM_STATE",
            "_NET_WM_STATE_HIDDEN",
            "_NET_WM_STATE_MAXIMIZED_HORZ",
            "_NET_WM_STATE_MAXIMIZED_VERT",
        ];
        let cookies = names
            .iter()
            .map(|name| {
                connection
                    .intern_atom(false, name.as_bytes())
                    .map_err(|error| request_error("interning an X11 display atom", error))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let atoms = cookies
            .into_iter()
            .map(|cookie| {
                cookie
                    .reply()
                    .map(|reply| reply.atom)
                    .map_err(|error| reply_error("interning an X11 display atom", error))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            net_client_list: atoms[0],
            net_current_desktop: atoms[1],
            net_desktop_viewport: atoms[2],
            net_frame_extents: atoms[3],
            net_number_of_desktops: atoms[4],
            net_supporting_wm_check: atoms[5],
            net_workarea: atoms[6],
            net_wm_desktop: atoms[7],
            net_wm_strut: atoms[8],
            net_wm_strut_partial: atoms[9],
            net_wm_window_type: atoms[10],
            net_wm_window_type_dock: atoms[11],
            resource_manager: atoms[12],
            xsettings_settings: atoms[13],
            net_wm_state: atoms[14],
            net_wm_state_hidden: atoms[15],
            net_wm_state_maximized_horz: atoms[16],
            net_wm_state_maximized_vert: atoms[17],
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Monitor {
    bounds: WindowRect,
    name: Option<String>,
    width_px: u32,
    height_px: u32,
    width_mm: u32,
    height_mm: u32,
}

fn monitor_list(session: &X11Session, root: Window) -> Result<Vec<Monitor>, String> {
    let Some((major, minor)) = session.randr_version else {
        return Ok(vec![root_monitor(session, root)?]);
    };
    let crtcs = if (major, minor) >= (1, 3) {
        session
            .connection
            .randr_get_screen_resources_current(root)
            .map_err(|error| request_error("querying X11 RandR resources", error))?
            .reply()
            .map_err(|error| reply_error("querying X11 RandR resources", error))?
            .crtcs
    } else {
        session
            .connection
            .randr_get_screen_resources(root)
            .map_err(|error| request_error("querying X11 RandR resources", error))?
            .reply()
            .map_err(|error| reply_error("querying X11 RandR resources", error))?
            .crtcs
    };

    let mut monitors = Vec::new();
    for crtc in crtcs {
        let crtc_info = session
            .connection
            .randr_get_crtc_info(crtc, x11rb::CURRENT_TIME)
            .map_err(|error| request_error("querying an X11 RandR output", error))?
            .reply()
            .map_err(|error| reply_error("querying an X11 RandR output", error))?;
        if crtc_info.mode == 0
            || crtc_info.outputs.is_empty()
            || crtc_info.width == 0
            || crtc_info.height == 0
        {
            continue;
        }
        let output_info = session
            .connection
            .randr_get_output_info(crtc_info.outputs[0], x11rb::CURRENT_TIME)
            .map_err(|error| request_error("querying an X11 RandR output name", error))?
            .reply()
            .map_err(|error| reply_error("querying an X11 RandR output name", error))?;
        if output_info.connection != randr::Connection::CONNECTED {
            continue;
        }
        monitors.push(Monitor {
            bounds: rect_from_origin_size(
                i32::from(crtc_info.x),
                i32::from(crtc_info.y),
                i32::from(crtc_info.width),
                i32::from(crtc_info.height),
            ),
            name: String::from_utf8(output_info.name).ok(),
            width_px: u32::from(crtc_info.width),
            height_px: u32::from(crtc_info.height),
            width_mm: output_info.mm_width,
            height_mm: output_info.mm_height,
        });
    }

    if monitors.is_empty() {
        Ok(vec![root_monitor(session, root)?])
    } else {
        Ok(monitors)
    }
}

fn root_monitor(session: &X11Session, root: Window) -> Result<Monitor, String> {
    let geometry = session
        .connection
        .get_geometry(root)
        .map_err(|error| request_error("querying the X11 root geometry", error))?
        .reply()
        .map_err(|error| reply_error("querying the X11 root geometry", error))?;
    Ok(Monitor {
        bounds: rect_from_origin_size(0, 0, i32::from(geometry.width), i32::from(geometry.height)),
        name: None,
        width_px: u32::from(geometry.width),
        height_px: u32::from(geometry.height),
        width_mm: 0,
        height_mm: 0,
    })
}

fn nearest_monitor(monitors: &[Monitor], x: i32, y: i32) -> Option<&Monitor> {
    monitors
        .iter()
        .find(|monitor| contains(monitor.bounds, x, y))
        .or_else(|| {
            monitors
                .iter()
                .min_by_key(|monitor| distance_outside(monitor.bounds, x, y))
        })
}

fn contains(rect: WindowRect, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

fn distance_outside(rect: WindowRect, x: i32, y: i32) -> u128 {
    let x = i128::from(x);
    let y = i128::from(y);
    let dx = (i128::from(rect.left) - x)
        .max(x - i128::from(rect.right) + 1)
        .max(0) as u128;
    let dy = (i128::from(rect.top) - y)
        .max(y - i128::from(rect.bottom) + 1)
        .max(0) as u128;
    dx * dx + dy * dy
}

fn union_rectangles(rectangles: impl Iterator<Item = WindowRect>) -> WindowRect {
    let mut rectangles = rectangles;
    let Some(first) = rectangles.next() else {
        return EMPTY_RECT;
    };
    rectangles.fold(first, |union, rect| WindowRect {
        left: union.left.min(rect.left),
        top: union.top.min(rect.top),
        right: union.right.max(rect.right),
        bottom: union.bottom.max(rect.bottom),
    })
}

fn midpoint(near: i32, far: i32) -> i32 {
    (i64::from(near) + i64::from(far)).div_euclid(2) as i32
}

fn rect_from_origin_size(left: i32, top: i32, width: i32, height: i32) -> WindowRect {
    WindowRect {
        left,
        top,
        right: left.saturating_add(width),
        bottom: top.saturating_add(height),
    }
}

fn request_error(what: &str, error: impl std::fmt::Display) -> String {
    format!("{what}: {error}")
}

fn reply_error(what: &str, error: impl std::fmt::Display) -> String {
    format!("{what}: {error}")
}

fn window_rect(session: &X11Session, window: Window) -> Result<WindowRect, String> {
    window_rect_and_root(session, window).map(|(rect, _)| rect)
}

fn window_rect_and_root(
    session: &X11Session,
    window: Window,
) -> Result<(WindowRect, Window), String> {
    let geometry = session
        .connection
        .get_geometry(window)
        .map_err(|error| request_error("querying an X11 window's geometry", error))?
        .reply()
        .map_err(|error| reply_error("querying an X11 window's geometry", error))?;
    let root = geometry.root;
    let mut frame = window;
    loop {
        let tree = session
            .connection
            .query_tree(frame)
            .map_err(|error| request_error("finding the X11 window's frame", error))?
            .reply()
            .map_err(|error| reply_error("finding the X11 window's frame", error))?;
        if tree.parent == root {
            break;
        }
        if tree.parent == 0 || tree.parent == frame {
            return Err("the X11 window has no path to its root window".to_owned());
        }
        frame = tree.parent;
    }

    let outer = session
        .connection
        .get_geometry(frame)
        .map_err(|error| request_error("querying the X11 frame geometry", error))?
        .reply()
        .map_err(|error| reply_error("querying the X11 frame geometry", error))?;
    let border = i32::from(outer.border_width) * 2;
    let mut rect = rect_from_origin_size(
        i32::from(outer.x),
        i32::from(outer.y),
        i32::from(outer.width).saturating_add(border),
        i32::from(outer.height).saturating_add(border),
    );

    if frame == window
        && let Some(extents) = read_property32(
            session,
            window,
            session.atoms.net_frame_extents,
            AtomEnum::CARDINAL.into(),
            "_NET_FRAME_EXTENTS",
        )?
    {
        if extents.len() != 4 {
            return Err("_NET_FRAME_EXTENTS did not contain four cardinals".to_owned());
        }
        rect.left = rect.left.saturating_sub(cardinal_coordinate(extents[0]));
        rect.right = rect.right.saturating_add(cardinal_coordinate(extents[1]));
        rect.top = rect.top.saturating_sub(cardinal_coordinate(extents[2]));
        rect.bottom = rect.bottom.saturating_add(cardinal_coordinate(extents[3]));
    }
    Ok((rect, root))
}

fn work_area_for_point(
    session: &X11Session,
    root: Window,
    x: i32,
    y: i32,
) -> Result<WindowRect, String> {
    let monitors = monitor_list(session, root)?;
    let monitor = nearest_monitor(&monitors, x, y)
        .ok_or_else(|| "the X11 server reports no active display".to_owned())?;
    work_area_for_monitor(session, root, monitor)
}

fn work_area_for_monitor(
    session: &X11Session,
    root: Window,
    monitor: &Monitor,
) -> Result<WindowRect, String> {
    let root_bounds = root_monitor(session, root)?.bounds;
    let desktop = current_desktop(session, root)?;
    let docks = dock_struts(session, root, desktop, root_bounds)?;
    let relevant: Vec<_> = docks
        .iter()
        .filter(|strut| strut.reserves_on(monitor.bounds, root_bounds))
        .collect();
    if !relevant.is_empty() {
        let work = relevant.iter().fold(monitor.bounds, |work, strut| {
            strut.apply_to(work, root_bounds)
        });
        return nonempty_work_area(work, "X11 dock struts leave no work area on this display");
    }
    if !docks.is_empty() {
        return Ok(monitor.bounds);
    }

    if let Some(work) = ewmh_work_area(session, root, desktop)? {
        let work = intersect_rectangles(monitor.bounds, work);
        return nonempty_work_area(
            work,
            "_NET_WORKAREA does not intersect the selected display",
        );
    }

    if has_ewmh_window_manager(session, root)? {
        return Err(
            "the X11 window manager published neither dock struts nor _NET_WORKAREA".to_owned(),
        );
    }
    Ok(monitor.bounds)
}

fn nonempty_work_area(rect: WindowRect, error: &str) -> Result<WindowRect, String> {
    if rect.left < rect.right && rect.top < rect.bottom {
        Ok(rect)
    } else {
        Err(error.to_owned())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DockStrut {
    left: u32,
    right: u32,
    top: u32,
    bottom: u32,
    left_start_y: u32,
    left_end_y: u32,
    right_start_y: u32,
    right_end_y: u32,
    top_start_x: u32,
    top_end_x: u32,
    bottom_start_x: u32,
    bottom_end_x: u32,
}

impl DockStrut {
    fn from_partial(values: &[u32]) -> Result<Self, String> {
        if values.len() != 12 {
            return Err("_NET_WM_STRUT_PARTIAL did not contain twelve cardinals".to_owned());
        }
        Ok(Self {
            left: values[0],
            right: values[1],
            top: values[2],
            bottom: values[3],
            left_start_y: values[4],
            left_end_y: values[5],
            right_start_y: values[6],
            right_end_y: values[7],
            top_start_x: values[8],
            top_end_x: values[9],
            bottom_start_x: values[10],
            bottom_end_x: values[11],
        })
    }

    fn from_basic(values: &[u32], root: WindowRect) -> Result<Self, String> {
        if values.len() != 4 {
            return Err("_NET_WM_STRUT did not contain four cardinals".to_owned());
        }
        Ok(Self {
            left: values[0],
            right: values[1],
            top: values[2],
            bottom: values[3],
            left_start_y: 0,
            left_end_y: root.bottom.saturating_sub(root.top).saturating_sub(1) as u32,
            right_start_y: 0,
            right_end_y: root.bottom.saturating_sub(root.top).saturating_sub(1) as u32,
            top_start_x: 0,
            top_end_x: root.right.saturating_sub(root.left).saturating_sub(1) as u32,
            bottom_start_x: 0,
            bottom_end_x: root.right.saturating_sub(root.left).saturating_sub(1) as u32,
        })
    }

    fn reserves_on(self, monitor: WindowRect, root: WindowRect) -> bool {
        (self.left > 0
            && segment_overlaps(
                monitor.top,
                monitor.bottom,
                root.top
                    .saturating_add(cardinal_coordinate(self.left_start_y)),
                root.top
                    .saturating_add(cardinal_coordinate(self.left_end_y))
                    .saturating_add(1),
            )
            && monitor.right > root.left
            && monitor.left < root.left.saturating_add(cardinal_coordinate(self.left)))
            || (self.right > 0
                && segment_overlaps(
                    monitor.top,
                    monitor.bottom,
                    root.top
                        .saturating_add(cardinal_coordinate(self.right_start_y)),
                    root.top
                        .saturating_add(cardinal_coordinate(self.right_end_y))
                        .saturating_add(1),
                )
                && monitor.left < root.right
                && monitor.right > root.right.saturating_sub(cardinal_coordinate(self.right)))
            || (self.top > 0
                && segment_overlaps(
                    monitor.left,
                    monitor.right,
                    root.left
                        .saturating_add(cardinal_coordinate(self.top_start_x)),
                    root.left
                        .saturating_add(cardinal_coordinate(self.top_end_x))
                        .saturating_add(1),
                )
                && monitor.bottom > root.top
                && monitor.top < root.top.saturating_add(cardinal_coordinate(self.top)))
            || (self.bottom > 0
                && segment_overlaps(
                    monitor.left,
                    monitor.right,
                    root.left
                        .saturating_add(cardinal_coordinate(self.bottom_start_x)),
                    root.left
                        .saturating_add(cardinal_coordinate(self.bottom_end_x))
                        .saturating_add(1),
                )
                && monitor.top < root.bottom
                && monitor.bottom > root.bottom.saturating_sub(cardinal_coordinate(self.bottom)))
    }

    fn apply_to(self, mut work: WindowRect, root: WindowRect) -> WindowRect {
        if self.left > 0
            && segment_overlaps(
                work.top,
                work.bottom,
                root.top
                    .saturating_add(cardinal_coordinate(self.left_start_y)),
                root.top
                    .saturating_add(cardinal_coordinate(self.left_end_y))
                    .saturating_add(1),
            )
            && work.right > root.left
            && work.left < root.left.saturating_add(cardinal_coordinate(self.left))
        {
            work.left = work
                .left
                .max(root.left.saturating_add(cardinal_coordinate(self.left)));
        }
        if self.right > 0
            && segment_overlaps(
                work.top,
                work.bottom,
                root.top
                    .saturating_add(cardinal_coordinate(self.right_start_y)),
                root.top
                    .saturating_add(cardinal_coordinate(self.right_end_y))
                    .saturating_add(1),
            )
            && work.left < root.right
            && work.right > root.right.saturating_sub(cardinal_coordinate(self.right))
        {
            work.right = work
                .right
                .min(root.right.saturating_sub(cardinal_coordinate(self.right)));
        }
        if self.top > 0
            && segment_overlaps(
                work.left,
                work.right,
                root.left
                    .saturating_add(cardinal_coordinate(self.top_start_x)),
                root.left
                    .saturating_add(cardinal_coordinate(self.top_end_x))
                    .saturating_add(1),
            )
            && work.bottom > root.top
            && work.top < root.top.saturating_add(cardinal_coordinate(self.top))
        {
            work.top = work
                .top
                .max(root.top.saturating_add(cardinal_coordinate(self.top)));
        }
        if self.bottom > 0
            && segment_overlaps(
                work.left,
                work.right,
                root.left
                    .saturating_add(cardinal_coordinate(self.bottom_start_x)),
                root.left
                    .saturating_add(cardinal_coordinate(self.bottom_end_x))
                    .saturating_add(1),
            )
            && work.top < root.bottom
            && work.bottom > root.bottom.saturating_sub(cardinal_coordinate(self.bottom))
        {
            work.bottom = work
                .bottom
                .min(root.bottom.saturating_sub(cardinal_coordinate(self.bottom)));
        }
        work
    }
}

fn segment_overlaps(a_start: i32, a_end: i32, b_start: i32, b_end: i32) -> bool {
    a_start < b_end && b_start < a_end
}

fn cardinal_coordinate(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn intersect_rectangles(left: WindowRect, right: WindowRect) -> WindowRect {
    WindowRect {
        left: left.left.max(right.left),
        top: left.top.max(right.top),
        right: left.right.min(right.right),
        bottom: left.bottom.min(right.bottom),
    }
}

fn current_desktop(session: &X11Session, root: Window) -> Result<u32, String> {
    let current = read_property32(
        session,
        root,
        session.atoms.net_current_desktop,
        AtomEnum::CARDINAL.into(),
        "_NET_CURRENT_DESKTOP",
    )?
    .and_then(|values| values.first().copied())
    .unwrap_or(0);
    let count = read_property32(
        session,
        root,
        session.atoms.net_number_of_desktops,
        AtomEnum::CARDINAL.into(),
        "_NET_NUMBER_OF_DESKTOPS",
    )?
    .and_then(|values| values.first().copied())
    .unwrap_or(1);
    if current >= count {
        return Err("_NET_CURRENT_DESKTOP exceeds _NET_NUMBER_OF_DESKTOPS".to_owned());
    }
    Ok(current)
}

fn dock_struts(
    session: &X11Session,
    root: Window,
    desktop: u32,
    root_bounds: WindowRect,
) -> Result<Vec<DockStrut>, String> {
    let mut candidates = HashSet::new();
    if let Some(windows) = read_property32(
        session,
        root,
        session.atoms.net_client_list,
        AtomEnum::WINDOW.into(),
        "_NET_CLIENT_LIST",
    )? {
        candidates.extend(windows);
    }
    let tree = session
        .connection
        .query_tree(root)
        .map_err(|error| request_error("querying X11 root children", error))?
        .reply()
        .map_err(|error| reply_error("querying X11 root children", error))?;
    candidates.extend(tree.children);

    let mut struts = Vec::new();
    for window in candidates {
        let attributes = match session.connection.get_window_attributes(window) {
            Ok(cookie) => match cookie.reply() {
                Ok(reply) => reply,
                Err(error) if bad_window(&error) => continue,
                Err(error) => return Err(reply_error("querying an X11 dock window", error)),
            },
            Err(error) => return Err(request_error("querying an X11 dock window", error)),
        };
        if attributes.map_state != xproto::MapState::VIEWABLE {
            continue;
        }
        let types = match read_window_property32(
            session,
            window,
            session.atoms.net_wm_window_type,
            AtomEnum::ATOM.into(),
            "_NET_WM_WINDOW_TYPE",
        ) {
            Ok(Some(types)) => types,
            Ok(None) => continue,
            Err(WindowPropertyError::Disappeared) => continue,
            Err(WindowPropertyError::Failed(error)) => return Err(error),
        };
        if !types.contains(&session.atoms.net_wm_window_type_dock) {
            continue;
        }
        let desktop_property = match read_window_property32(
            session,
            window,
            session.atoms.net_wm_desktop,
            AtomEnum::CARDINAL.into(),
            "_NET_WM_DESKTOP",
        ) {
            Ok(property) => property,
            Err(WindowPropertyError::Disappeared) => continue,
            Err(WindowPropertyError::Failed(error)) => return Err(error),
        };
        if let Some(window_desktop) = desktop_property.and_then(|values| values.first().copied())
            && window_desktop != u32::MAX
            && window_desktop != desktop
        {
            continue;
        }

        let partial = match read_window_property32(
            session,
            window,
            session.atoms.net_wm_strut_partial,
            AtomEnum::CARDINAL.into(),
            "_NET_WM_STRUT_PARTIAL",
        ) {
            Ok(partial) => partial,
            Err(WindowPropertyError::Disappeared) => continue,
            Err(WindowPropertyError::Failed(error)) => return Err(error),
        };
        let strut = if let Some(values) = partial {
            DockStrut::from_partial(&values)?
        } else {
            let basic = match read_window_property32(
                session,
                window,
                session.atoms.net_wm_strut,
                AtomEnum::CARDINAL.into(),
                "_NET_WM_STRUT",
            ) {
                Ok(basic) => basic,
                Err(WindowPropertyError::Disappeared) => continue,
                Err(WindowPropertyError::Failed(error)) => return Err(error),
            };
            let Some(values) = basic else {
                continue;
            };
            DockStrut::from_basic(&values, root_bounds)?
        };
        if strut.left != 0 || strut.right != 0 || strut.top != 0 || strut.bottom != 0 {
            struts.push(strut);
        }
    }
    struts.sort_unstable();
    Ok(struts)
}

fn ewmh_work_area(
    session: &X11Session,
    root: Window,
    desktop: u32,
) -> Result<Option<WindowRect>, String> {
    let Some(work_areas) = read_property32(
        session,
        root,
        session.atoms.net_workarea,
        AtomEnum::CARDINAL.into(),
        "_NET_WORKAREA",
    )?
    else {
        return Ok(None);
    };
    let index = usize::try_from(desktop)
        .ok()
        .and_then(|desktop| desktop.checked_mul(4))
        .ok_or_else(|| "_NET_WORKAREA desktop index overflowed".to_owned())?;
    let Some(values) = work_areas.get(index..index.saturating_add(4)) else {
        return Err("_NET_WORKAREA did not contain the current desktop".to_owned());
    };
    let viewport = read_property32(
        session,
        root,
        session.atoms.net_desktop_viewport,
        AtomEnum::CARDINAL.into(),
        "_NET_DESKTOP_VIEWPORT",
    )?;
    let viewport = viewport
        .as_deref()
        .and_then(|values| values.get(index / 2..index / 2 + 2))
        .map_or((0, 0), |values| (values[0] as i32, values[1] as i32));
    let left = (values[0] as i32).saturating_add(viewport.0);
    let top = (values[1] as i32).saturating_add(viewport.1);
    let width = cardinal_coordinate(values[2]);
    let height = cardinal_coordinate(values[3]);
    Ok(Some(rect_from_origin_size(left, top, width, height)))
}

fn has_ewmh_window_manager(session: &X11Session, root: Window) -> Result<bool, String> {
    Ok(read_property32(
        session,
        root,
        session.atoms.net_supporting_wm_check,
        AtomEnum::WINDOW.into(),
        "_NET_SUPPORTING_WM_CHECK",
    )?
    .is_some_and(|windows| windows.first().is_some_and(|window| *window != 0)))
}

fn read_property32(
    session: &X11Session,
    window: Window,
    property: Atom,
    expected_type: Atom,
    name: &str,
) -> Result<Option<Vec<u32>>, String> {
    read_property32_inner(session, window, property, expected_type, name).map_err(|error| {
        match error {
            WindowPropertyError::Disappeared => {
                format!("{name}: the X11 window disappeared during the query")
            }
            WindowPropertyError::Failed(error) => error,
        }
    })
}

enum WindowPropertyError {
    Disappeared,
    Failed(String),
}

fn read_window_property32(
    session: &X11Session,
    window: Window,
    property: Atom,
    expected_type: Atom,
    name: &str,
) -> Result<Option<Vec<u32>>, WindowPropertyError> {
    read_property32_inner(session, window, property, expected_type, name)
}

fn read_property32_inner(
    session: &X11Session,
    window: Window,
    property: Atom,
    expected_type: Atom,
    name: &str,
) -> Result<Option<Vec<u32>>, WindowPropertyError> {
    let reply = session
        .connection
        .get_property(false, window, property, AtomEnum::ANY, 0, PROPERTY_LONGS)
        .map_err(|error| {
            WindowPropertyError::Failed(request_error(&format!("reading {name}"), error))
        })?
        .reply()
        .map_err(|error| {
            if bad_window(&error) {
                WindowPropertyError::Disappeared
            } else {
                WindowPropertyError::Failed(reply_error(&format!("reading {name}"), error))
            }
        })?;
    if reply.type_ == u32::from(AtomEnum::NONE) {
        return Ok(None);
    }
    if reply.type_ != expected_type || reply.format != 32 {
        return Err(WindowPropertyError::Failed(format!(
            "{name} was not a 32-bit property of the expected type"
        )));
    }
    if reply.bytes_after != 0 {
        return Err(WindowPropertyError::Failed(format!(
            "{name} exceeded the X11 property reply limit"
        )));
    }
    let values = reply
        .value32()
        .ok_or_else(|| WindowPropertyError::Failed(format!("{name} had no 32-bit values")))?
        .collect();
    Ok(Some(values))
}

fn bad_window(error: &ReplyError) -> bool {
    matches!(error, ReplyError::X11Error(error) if error.error_kind == ErrorKind::Window)
}

fn scale_factor(session: &X11Session, monitor: &Monitor) -> Option<f64> {
    if let Ok(override_value) = env::var("WINIT_X11_SCALE_FACTOR") {
        if override_value.eq_ignore_ascii_case("randr") {
            return Some(randr_scale_factor(monitor).unwrap_or(1.0));
        }
        if !override_value.is_empty() {
            return override_value
                .parse::<f64>()
                .ok()
                .filter(|factor| valid_scale_factor(*factor));
        }
    }

    xsettings_dpi(session)
        .filter(|dpi| valid_scale_factor(*dpi / 96.0))
        .or_else(|| xft_dpi(session).filter(|dpi| valid_scale_factor(*dpi / 96.0)))
        .map(|dpi| dpi / 96.0)
        .or_else(|| randr_scale_factor(monitor))
        .or(Some(1.0))
}

fn valid_scale_factor(factor: f64) -> bool {
    factor.is_sign_positive() && factor.is_normal()
}

fn randr_scale_factor(monitor: &Monitor) -> Option<f64> {
    if monitor.width_mm == 0 || monitor.height_mm == 0 {
        return None;
    }
    let width_px = f64::from(monitor.width_px);
    let height_px = f64::from(monitor.height_px);
    let width_mm = f64::from(monitor.width_mm);
    let height_mm = f64::from(monitor.height_mm);
    let pixels_per_mm = ((width_px * height_px) / (width_mm * height_mm)).sqrt();
    let factor = ((pixels_per_mm * (12.0 * 25.4 / 96.0)).round() / 12.0).max(1.0);
    (factor <= 20.0).then_some(factor)
}

fn xsettings_dpi(session: &X11Session) -> Option<f64> {
    let owner = session
        .connection
        .get_selection_owner(session.xsettings_selection)
        .ok()?
        .reply()
        .ok()?
        .owner;
    if owner == 0 {
        return None;
    }
    let reply = session
        .connection
        .get_property(
            false,
            owner,
            session.atoms.xsettings_settings,
            session.atoms.xsettings_settings,
            0,
            PROPERTY_LONGS,
        )
        .ok()?
        .reply()
        .ok()?;
    if reply.type_ != session.atoms.xsettings_settings
        || reply.format != 8
        || reply.bytes_after != 0
    {
        return None;
    }
    xsettings_dpi_value(&reply.value)
}

fn xft_dpi(session: &X11Session) -> Option<f64> {
    let reply = session
        .connection
        .get_property(
            false,
            session.root,
            session.atoms.resource_manager,
            AtomEnum::STRING,
            0,
            PROPERTY_LONGS,
        )
        .ok()?
        .reply()
        .ok()?;
    if reply.format != 8 || reply.bytes_after != 0 {
        return None;
    }
    let database = resource_manager::Database::new_from_get_property_reply(&reply)?;
    database
        .get_string("Xft.dpi", "")?
        .parse::<f64>()
        .ok()
        .filter(|dpi| valid_scale_factor(*dpi))
}

fn xsettings_dpi_value(data: &[u8]) -> Option<f64> {
    let little_endian = match *data.first()? {
        b'l' => true,
        b'B' => false,
        _ => return None,
    };
    let mut offset = 8_usize;
    let count = read_i32(data, &mut offset, little_endian)?;
    if count < 0 {
        return None;
    }
    for _ in 0..count {
        let setting_type = read_u8(data, &mut offset)?;
        offset = offset.checked_add(1)?;
        let name_length = usize::from(read_u16(data, &mut offset, little_endian)?);
        let name = read_bytes(data, &mut offset, name_length)?;
        skip_padding(&mut offset, name_length)?;
        offset = offset.checked_add(4)?;
        match setting_type {
            0 => {
                let value = read_i32(data, &mut offset, little_endian)?;
                if name == b"Xft/DPI" {
                    return Some(f64::from(value) / 1024.0);
                }
            }
            1 => {
                let value_length =
                    usize::try_from(read_i32(data, &mut offset, little_endian)?).ok()?;
                read_bytes(data, &mut offset, value_length)?;
                skip_padding(&mut offset, value_length)?;
            }
            2 => offset = offset.checked_add(8)?,
            _ => return None,
        }
        if offset > data.len() {
            return None;
        }
    }
    None
}

fn read_u8(data: &[u8], offset: &mut usize) -> Option<u8> {
    let value = *data.get(*offset)?;
    *offset = (*offset).checked_add(1)?;
    Some(value)
}

fn read_u16(data: &[u8], offset: &mut usize, little_endian: bool) -> Option<u16> {
    let bytes: [u8; 2] = read_bytes(data, offset, 2)?.try_into().ok()?;
    Some(if little_endian {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    })
}

fn read_i32(data: &[u8], offset: &mut usize, little_endian: bool) -> Option<i32> {
    let bytes: [u8; 4] = read_bytes(data, offset, 4)?.try_into().ok()?;
    Some(if little_endian {
        i32::from_le_bytes(bytes)
    } else {
        i32::from_be_bytes(bytes)
    })
}

fn read_bytes<'a>(data: &'a [u8], offset: &mut usize, length: usize) -> Option<&'a [u8]> {
    let end = (*offset).checked_add(length)?;
    let bytes = data.get(*offset..end)?;
    *offset = end;
    Some(bytes)
}

fn skip_padding(offset: &mut usize, length: usize) -> Option<()> {
    let padding = (4 - (length % 4)) % 4;
    *offset = (*offset).checked_add(padding)?;
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(left: i32, top: i32, right: i32, bottom: i32) -> Monitor {
        Monitor {
            bounds: WindowRect {
                left,
                top,
                right,
                bottom,
            },
            name: None,
            width_px: (right - left) as u32,
            height_px: (bottom - top) as u32,
            width_mm: 0,
            height_mm: 0,
        }
    }

    #[test]
    fn ewmh_window_state_requires_a_complete_maximize_pair() {
        const HIDDEN: Atom = 101;
        const MAXIMIZED_HORZ: Atom = 102;
        const MAXIMIZED_VERT: Atom = 103;
        assert_eq!(
            window_state_from_atoms(
                &[HIDDEN, MAXIMIZED_HORZ, MAXIMIZED_VERT],
                HIDDEN,
                MAXIMIZED_HORZ,
                MAXIMIZED_VERT
            ),
            (Some(true), Some(true))
        );
        assert_eq!(
            window_state_from_atoms(&[MAXIMIZED_HORZ], HIDDEN, MAXIMIZED_HORZ, MAXIMIZED_VERT),
            (None, Some(false))
        );
        assert_eq!(
            window_state_from_atoms(&[], HIDDEN, MAXIMIZED_HORZ, MAXIMIZED_VERT),
            (Some(false), Some(false))
        );
    }

    #[test]
    fn point_selection_uses_containment_then_nearest_monitor() {
        let monitors = [monitor(0, 0, 1920, 1080), monitor(1920, 0, 3840, 1080)];
        assert_eq!(nearest_monitor(&monitors, 1920, 40), Some(&monitors[1]));
        assert_eq!(nearest_monitor(&monitors, -1, 40), Some(&monitors[0]));
        assert_eq!(nearest_monitor(&[], 0, 0), None);
    }

    #[test]
    fn partial_dock_strut_does_not_shrink_the_adjacent_monitor() {
        let root = WindowRect {
            left: 0,
            top: 0,
            right: 3840,
            bottom: 1080,
        };
        let left = monitor(0, 0, 1920, 1080);
        let right = monitor(1920, 0, 3840, 1080);
        let panel = DockStrut {
            left: 0,
            right: 0,
            top: 36,
            bottom: 0,
            left_start_y: 0,
            left_end_y: 0,
            right_start_y: 0,
            right_end_y: 0,
            top_start_x: 0,
            top_end_x: 1919,
            bottom_start_x: 0,
            bottom_end_x: 0,
        };

        assert!(panel.reserves_on(left.bounds, root));
        assert!(!panel.reserves_on(right.bounds, root));
        assert_eq!(
            panel.apply_to(left.bounds, root),
            WindowRect {
                top: 36,
                ..left.bounds
            }
        );
        assert_eq!(panel.apply_to(right.bounds, root), right.bounds);
    }

    #[test]
    fn basic_dock_strut_spans_the_root_width() {
        let root = WindowRect {
            left: 0,
            top: 0,
            right: 3840,
            bottom: 1080,
        };
        let left = monitor(0, 0, 1920, 1080);
        let right = monitor(1920, 0, 3840, 1080);
        let panel = DockStrut::from_basic(&[0, 0, 28, 0], root).expect("parse a basic strut");

        assert!(panel.reserves_on(left.bounds, root));
        assert!(panel.reserves_on(right.bounds, root));
        assert_eq!(panel.apply_to(left.bounds, root).top, 28);
        assert_eq!(panel.apply_to(right.bounds, root).top, 28);
    }

    #[test]
    fn xsettings_dpi_parser_reads_integer_values_in_both_byte_orders() {
        assert_eq!(
            xsettings_dpi_value(&xsettings_dpi_setting(true, 96 * 1024)),
            Some(96.0)
        );
        assert_eq!(
            xsettings_dpi_value(&xsettings_dpi_setting(false, 144 * 1024)),
            Some(144.0)
        );
        assert_eq!(
            xsettings_dpi_value(&xsettings_dpi_setting(true, 0)),
            Some(0.0)
        );
        assert_eq!(xsettings_dpi_value(b"invalid"), None);
    }

    fn xsettings_dpi_setting(little_endian: bool, dpi: i32) -> Vec<u8> {
        let mut bytes = vec![if little_endian { b'l' } else { b'B' }, 0, 0, 0];
        bytes.extend_from_slice(&ordered_i32(0, little_endian));
        bytes.extend_from_slice(&ordered_i32(1, little_endian));
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&ordered_u16(7, little_endian));
        bytes.extend_from_slice(b"Xft/DPI");
        bytes.push(0);
        bytes.extend_from_slice(&ordered_i32(0, little_endian));
        bytes.extend_from_slice(&ordered_i32(dpi, little_endian));
        bytes
    }

    fn ordered_i32(value: i32, little_endian: bool) -> [u8; 4] {
        if little_endian {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        }
    }

    fn ordered_u16(value: u16, little_endian: bool) -> [u8; 2] {
        if little_endian {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        }
    }
}

#[cfg(test)]
mod stalled_server_tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread::JoinHandle;
    use x11rb::protocol::xproto::{BackingStore, ImageOrder, Screen, Setup};
    use x11rb::x11_utils::Serialize;

    const CHILD_MARKER: &str = "FOLIO_PR20_X11_QUERY_RETURNED";
    const CHILD_MARKER_ENV: &str = "FOLIO_PR20_X11_STALL_CHILD";

    struct StalledX11Server {
        display: u16,
        port: u16,
        stalled: Receiver<Result<(), String>>,
        release: Option<Sender<()>>,
        worker: Option<JoinHandle<()>>,
    }

    impl StalledX11Server {
        fn start() -> Self {
            let (display, port, listener) = (10_000_u16..=59_535)
                .find_map(|display| {
                    let port = 6000_u16.checked_add(display)?;
                    TcpListener::bind(("127.0.0.1", port))
                        .ok()
                        .map(|listener| (display, port, listener))
                })
                .expect("bind an unused local X11 test port");
            let (stalled_tx, stalled) = mpsc::channel();
            let (release, release_rx) = mpsc::channel();
            let worker = std::thread::spawn(move || {
                let result = serve_until_atom_reply(listener, &stalled_tx, &release_rx);
                if let Err(error) = result {
                    let _ = stalled_tx.send(Err(error));
                }
            });
            Self {
                display,
                port,
                stalled,
                release: Some(release),
                worker: Some(worker),
            }
        }

        fn wait_until_stalled(&self) -> Result<(), String> {
            self.stalled.recv().map_err(|error| {
                format!("the client never reached the stalled X11 reply: {error}")
            })?
        }
    }

    impl Drop for StalledX11Server {
        fn drop(&mut self) {
            if let Some(release) = self.release.take() {
                let _ = release.send(());
            }
            let _ = TcpStream::connect(("127.0.0.1", self.port));
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    struct ChildGuard(Child);

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn serve_until_atom_reply(
        listener: TcpListener,
        stalled: &Sender<Result<(), String>>,
        release: &Receiver<()>,
    ) -> Result<(), String> {
        let (mut stream, _) = listener
            .accept()
            .map_err(|error| format!("accepting the X11 client: {error}"))?;
        let mut setup_request = [0; 12];
        stream
            .read_exact(&mut setup_request)
            .map_err(|error| format!("reading the X11 setup request: {error}"))?;
        stream
            .write_all(&setup_packet())
            .map_err(|error| format!("answering the X11 setup request: {error}"))?;

        let mut request_header = [0; 4];
        stream
            .read_exact(&mut request_header)
            .map_err(|error| format!("reading the first X11 request: {error}"))?;
        if request_header[0] != 16 {
            return Err(format!(
                "expected InternAtom opcode 16, got {}",
                request_header[0]
            ));
        }
        let request_length =
            usize::from(u16::from_le_bytes([request_header[2], request_header[3]]))
                .saturating_mul(4);
        if request_length < request_header.len() {
            return Err(format!("invalid X11 request length {request_length}"));
        }
        let mut request_body = vec![0; request_length - request_header.len()];
        stream
            .read_exact(&mut request_body)
            .map_err(|error| format!("reading the InternAtom request: {error}"))?;

        stalled
            .send(Ok(()))
            .map_err(|error| format!("reporting the stalled reply: {error}"))?;
        release
            .recv()
            .map_err(|error| format!("releasing the stalled X11 reply: {error}"))
    }

    fn setup_packet() -> Vec<u8> {
        let mut setup = Setup {
            status: 1,
            protocol_major_version: 11,
            protocol_minor_version: 0,
            length: 0,
            release_number: 1,
            resource_id_base: 0x0010_0000,
            resource_id_mask: 0x001f_ffff,
            motion_buffer_size: 0,
            maximum_request_length: u16::MAX,
            image_byte_order: ImageOrder::LSB_FIRST,
            bitmap_format_bit_order: ImageOrder::LSB_FIRST,
            bitmap_format_scanline_unit: 8,
            bitmap_format_scanline_pad: 8,
            min_keycode: 8,
            max_keycode: 255,
            vendor: Vec::new(),
            pixmap_formats: Vec::new(),
            roots: vec![Screen {
                root: 1,
                default_colormap: 1,
                white_pixel: 0x00ff_ffff,
                black_pixel: 0,
                width_in_pixels: 1920,
                height_in_pixels: 1080,
                width_in_millimeters: 0,
                height_in_millimeters: 0,
                min_installed_maps: 1,
                max_installed_maps: 1,
                root_visual: 1,
                backing_stores: BackingStore::NOT_USEFUL,
                save_unders: false,
                root_depth: 24,
                ..Screen::default()
            }],
        };
        let length = setup.serialize().len();
        setup.length = u16::try_from((length - 8) / 4).expect("setup length fits the protocol");
        setup.serialize()
    }

    #[test]
    fn client_child() {
        if std::env::var_os(CHILD_MARKER_ENV).is_none() {
            return;
        }
        assert!(crate::admission::enter_window_thread());
        install_backend(Backend::X11).expect("select the fake X11 backend");
        install_display_wake(|_| {}).expect("install the fake event-loop wake");
        let window = crate::NativeWindow::from_x11(
            std::num::NonZeroU32::new(1).expect("the fixture X11 window is nonzero"),
        );
        let _request = request_display(1, 1, LinuxDisplayQuery::PointerInWindow { window })
            .expect("enqueue the X11 query off the window thread");
        let mut stdout = std::io::stdout().lock();
        stdout
            .write_all(format!("{CHILD_MARKER}\n").as_bytes())
            .expect("write the request-return marker");
        stdout.flush().expect("flush the request-return marker");
        let mut release = [0];
        std::io::stdin()
            .read_exact(&mut release)
            .expect("the parent confirms the held reply before exit");
        std::process::exit(0);
    }

    #[test]
    fn a_window_thread_query_returns_while_the_x11_server_withholds_its_reply() {
        if std::env::var_os(CHILD_MARKER_ENV).is_some() {
            return;
        }
        let server = StalledX11Server::start();
        let authority = std::env::temp_dir().join(format!(
            "folio-pr20-xauth-{}-{}",
            std::process::id(),
            server.display
        ));
        let stderr_path = std::env::temp_dir().join(format!(
            "folio-pr20-x11-stderr-{}-{}",
            std::process::id(),
            server.display
        ));
        std::fs::write(&authority, []).expect("write empty Xauthority data");
        let stderr_file = std::fs::File::create(&stderr_path).expect("capture child diagnostics");
        let mut child = ChildGuard(
            Command::new(std::env::current_exe().expect("locate this test binary"))
                .args([
                    "--exact",
                    "linux_display::stalled_server_tests::client_child",
                    "--test-threads=1",
                    "--nocapture",
                ])
                .env(CHILD_MARKER_ENV, "1")
                .env("DISPLAY", format!("127.0.0.1:{}", server.display))
                .env("XAUTHORITY", &authority)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::from(stderr_file))
                .spawn()
                .expect("start an isolated window-thread client"),
        );
        let stdout = child.0.stdout.take().expect("capture the client marker");
        let mut child_stdin = child
            .0
            .stdin
            .take()
            .expect("release the child after the barrier");
        let (returned_tx, returned) = mpsc::channel();
        let marker_reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) if line.contains(CHILD_MARKER) => {
                        let _ = returned_tx.send(());
                        return;
                    }
                    Ok(_) => {}
                    Err(_) => return,
                }
            }
        });

        server
            .wait_until_stalled()
            .expect("the X11 server received InternAtom");
        let returned = returned.recv();
        if returned.is_ok() {
            child_stdin
                .write_all(&[0])
                .expect("release the child after the server withheld its reply");
        } else {
            let _ = child.0.kill();
        }
        let _ = child.0.wait();
        let _ = marker_reader.join();
        let _ = std::fs::remove_file(authority);
        let child_stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
        let _ = std::fs::remove_file(stderr_path);
        assert!(
            returned.is_ok(),
            "the window-thread call must return while the X11 server withholds its InternAtom reply; child stderr: {child_stderr}"
        );
    }
}
