//! Linux global summon on X11, with Wayland's window-manager boundary stated.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::JoinHandle;

use crate::admission::WorkerCtx;
use crate::hotkey::{Hotkey, HotkeyFault, holds_a_summon_modifier};
use crate::linux_window::Backend;
use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::ErrorKind;
use x11rb::protocol::Event;
use x11rb::protocol::xkb::{self, BoolCtrl, PerClientFlag};
use x11rb::protocol::xproto::{
    ConnectionExt, GrabMode, KeyButMask, KeyPressEvent, KeyReleaseEvent, Keycode, ModMask, Window,
};

const HOTKEY_THREAD: &str = "bt-linux-global-hotkey";

/// Why the X11 service woke the event loop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinuxHotkeyEvent {
    /// The actual X11 grab succeeded or failed.
    Ready { id: i32, generation: u64 },
    /// The grabbed chord was pressed.
    Activated {
        id: i32,
        generation: u64,
        pointer: (i32, i32),
    },
}

/// Whether the X11 server has accepted this claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinuxHotkeyStatus {
    /// Registration is being performed on the X11 worker.
    Pending,
    /// The X11 server accepted the claim.
    Active,
    /// The X11 server refused the claim or could not be reached.
    Failed(HotkeyFault),
}

type HotkeyWake = Box<dyn Fn(LinuxHotkeyEvent) + Send + Sync + 'static>;
static HOTKEY_WAKE: OnceLock<HotkeyWake> = OnceLock::new();
static SERVICE: OnceLock<Mutex<ServiceState>> = OnceLock::new();
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Register the X11 hotkey and activation event-loop wake once at startup.
pub fn install_hotkey_wake(
    wake: impl Fn(LinuxHotkeyEvent) + Send + Sync + 'static,
) -> Result<(), String> {
    HOTKEY_WAKE
        .set(Box::new(wake))
        .map_err(|_| "the Linux hotkey wake has already been installed".to_owned())
}

fn wake(event: LinuxHotkeyEvent) {
    if let Some(wake) = HOTKEY_WAKE.get() {
        wake(event);
    }
}

/// Claim a global shortcut on the window's live Linux backend.
///
/// X11 returns a pending claim; the returned value changes to `Active` or
/// `Failed` after the X11 server answers. Wayland refuses here because Folio's
/// current summoned-window lifecycle requires hide, restore and focus
/// operations that winit 0.30.13 does not expose on Wayland. A desktop may
/// offer the GlobalShortcuts portal, but this backend does not request a portal
/// binding for a toggle it cannot complete. `folio --new-window` remains a
/// manual new-window fallback, not a toggle summon.
///
/// The caller supplies `backend` from the live window's raw handle. This
/// function never guesses from `DISPLAY` or `WAYLAND_DISPLAY`; Wayland is
/// refused before any X11 connection is opened. For X11 it opens the same
/// process-default display path winit uses and keeps one worker-owned client
/// connection for every grab and release.
pub fn register_global_hotkey(
    backend: Backend,
    id: i32,
    hotkey: Hotkey,
) -> Result<LinuxGlobalHotkey, HotkeyFault> {
    if !holds_a_summon_modifier(hotkey) {
        return Err(HotkeyFault::NoModifier);
    }
    if backend == Backend::Wayland {
        return Err(HotkeyFault::Refused(WAYLAND_SUMMON_REFUSAL.to_owned()));
    }
    if HOTKEY_WAKE.get().is_none() {
        return Err(HotkeyFault::Refused(
            "the Linux hotkey event-loop wake is not installed".to_owned(),
        ));
    }
    if hotkey.virtual_key == 0 {
        return Err(HotkeyFault::NoSuchKey);
    }

    let sender = service_sender().map_err(HotkeyFault::Refused)?;
    let generation = NEXT_GENERATION
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1);
    let status = Arc::new(Mutex::new(LinuxHotkeyStatus::Pending));
    let activation = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    sender
        .send(WorkerCommand::Register {
            id,
            generation,
            hotkey,
            status: Arc::clone(&status),
            activation: Arc::clone(&activation),
            cancelled: Arc::clone(&cancelled),
        })
        .map_err(HotkeyFault::Refused)?;
    Ok(LinuxGlobalHotkey {
        id,
        generation,
        status,
        activation,
        cancelled,
        sender,
    })
}

/// The reason native Wayland summon cannot complete the existing quake action.
pub const WAYLAND_SUMMON_REFUSAL: &str = "native Wayland summon is unavailable: a GlobalShortcuts portal binding alone cannot hide, restore, or focus the existing window through winit 0.30.13; use `folio --new-window` to open another window";

/// A claim whose registration and release are serialized by one X11 worker.
pub struct LinuxGlobalHotkey {
    id: i32,
    generation: u64,
    status: Arc<Mutex<LinuxHotkeyStatus>>,
    activation: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    sender: CommandSender,
}

impl LinuxGlobalHotkey {
    /// The id supplied when this chord was requested.
    #[must_use]
    pub const fn id(&self) -> i32 {
        self.id
    }

    /// Distinguish this async answer from an older claim for the same id.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Read the latest X11 registration answer without waiting.
    #[must_use]
    pub fn status(&self) -> LinuxHotkeyStatus {
        self.status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Take one activation the worker has parked.
    #[must_use]
    pub fn take_activation(&self) -> bool {
        self.activation.swap(false, Ordering::AcqRel)
    }

    fn queue_release(
        &self,
        acknowledgment: Option<mpsc::SyncSender<Result<(), String>>>,
    ) -> Result<(), String> {
        self.sender.send(WorkerCommand::Release {
            id: self.id,
            generation: self.generation,
            acknowledgment,
        })
    }
}

impl Drop for LinuxGlobalHotkey {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.queue_release(None);
    }
}

enum WorkerCommand {
    Register {
        id: i32,
        generation: u64,
        hotkey: Hotkey,
        status: Arc<Mutex<LinuxHotkeyStatus>>,
        activation: Arc<AtomicBool>,
        cancelled: Arc<AtomicBool>,
    },
    Release {
        id: i32,
        generation: u64,
        acknowledgment: Option<mpsc::SyncSender<Result<(), String>>>,
    },
    Shutdown,
}

struct ActiveGrab {
    generation: u64,
    keycode: Keycode,
    masks: Vec<ModMask>,
    base_mask: u16,
    ignored_locks: u16,
    pressed: bool,
    status: Arc<Mutex<LinuxHotkeyStatus>>,
    activation: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone)]
struct CommandSender {
    sender: mpsc::Sender<WorkerCommand>,
    wake: Arc<Mutex<UnixStream>>,
}

impl CommandSender {
    fn send(&self, command: WorkerCommand) -> Result<(), String> {
        self.sender
            .send(command)
            .map_err(|_| "the Linux X11 hotkey worker stopped".to_owned())?;
        let mut wake = self
            .wake
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            match wake.write(&[1]) {
                Ok(1) => return Ok(()),
                Ok(_) => {
                    return Err("the Linux X11 hotkey wake wrote no byte".to_owned());
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) => {
                    return Err(format!(
                        "could not wake the Linux X11 hotkey worker: {error}"
                    ));
                }
            }
        }
    }
}

struct HotkeyService {
    sender: CommandSender,
    worker: JoinHandle<()>,
}

enum ServiceState {
    NotStarted,
    Running(HotkeyService),
    Stopped,
}

fn service_sender() -> Result<CommandSender, String> {
    let service = SERVICE.get_or_init(|| Mutex::new(ServiceState::NotStarted));
    let mut current = service
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match &*current {
        ServiceState::Running(service) => return Ok(service.sender.clone()),
        ServiceState::Stopped => {
            return Err("the Linux X11 hotkey worker has shut down".to_owned());
        }
        ServiceState::NotStarted => {}
    }

    let (sender, receiver) = mpsc::channel();
    let (wake_reader, wake_writer) = UnixStream::pair()
        .map_err(|error| format!("could not create the Linux X11 worker wake: {error}"))?;
    wake_reader
        .set_nonblocking(true)
        .map_err(|error| format!("could not prepare the Linux X11 worker wake: {error}"))?;
    wake_writer
        .set_nonblocking(true)
        .map_err(|error| format!("could not prepare the Linux X11 worker wake: {error}"))?;
    let command_sender = CommandSender {
        sender,
        wake: Arc::new(Mutex::new(wake_writer)),
    };
    let worker = crate::spawn_at_priority(
        HOTKEY_THREAD,
        crate::ThreadPriority::BelowNormal,
        move |_ctx| x11_worker(receiver, wake_reader),
    )
    .map_err(|error| format!("could not start the Linux X11 hotkey worker: {error}"))?;
    let service = HotkeyService {
        sender: command_sender.clone(),
        worker,
    };
    *current = ServiceState::Running(service);
    Ok(command_sender)
}

/// Stop the process-wide X11 worker and join it after the application loop ends.
pub fn shutdown_hotkey_worker(_worker: &WorkerCtx) -> Result<(), String> {
    let service = SERVICE.get_or_init(|| Mutex::new(ServiceState::NotStarted));
    let service = {
        let mut state = service
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match std::mem::replace(&mut *state, ServiceState::Stopped) {
            ServiceState::Running(service) => Some(service),
            ServiceState::NotStarted | ServiceState::Stopped => None,
        }
    };
    let Some(service) = service else {
        return Ok(());
    };
    let sent = service.sender.send(WorkerCommand::Shutdown);
    let worker: std::thread::JoinHandle<()> = service.worker;
    let joined = worker
        .join()
        .map_err(|_| "the Linux X11 hotkey worker panicked".to_owned());
    sent?;
    joined
}

fn x11_worker(receiver: mpsc::Receiver<WorkerCommand>, mut wake_reader: UnixStream) {
    let mut connection = None;
    let mut root = 0;
    let mut active = HashMap::<i32, ActiveGrab>::new();
    loop {
        while let Ok(command) = receiver.try_recv() {
            if !handle_command(command, &mut connection, &mut root, &mut active) {
                ungrab_all(connection.as_ref(), root, &active);
                return;
            }
        }
        if let Some(conn) = connection.as_ref()
            && let Err(error) = drain_events(conn, &mut active)
        {
            fail_active(
                &mut active,
                HotkeyFault::Refused(format!("the X11 hotkey connection closed: {error}")),
            );
            connection = None;
            root = 0;
        }
        if active.is_empty() {
            connection = None;
            root = 0;
        }
        let readiness = match wait_for_event_or_command(connection.as_ref(), &wake_reader) {
            Ok(readiness) => readiness,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                let refusal =
                    HotkeyFault::Refused(format!("the Linux X11 hotkey wait failed: {error}"));
                fail_active(&mut active, refusal);
                connection = None;
                root = 0;
                continue;
            }
        };
        if readiness.command
            && let Err(error) = drain_wake(&mut wake_reader)
        {
            let refusal =
                HotkeyFault::Refused(format!("the Linux X11 hotkey wake closed: {error}"));
            fail_active(&mut active, refusal);
            return;
        }
        if readiness.connection_error {
            let refusal = HotkeyFault::Refused("the X11 hotkey connection closed".to_owned());
            fail_active(&mut active, refusal);
            connection = None;
            root = 0;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Readiness {
    command: bool,
    connection_error: bool,
}

fn wait_for_event_or_command(
    connection: Option<&x11rb::rust_connection::RustConnection>,
    wake_reader: &UnixStream,
) -> io::Result<Readiness> {
    let mut descriptors = [
        libc::pollfd {
            fd: wake_reader.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: connection.map_or(-1, |connection| connection.stream().as_raw_fd()),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let count = if connection.is_some() { 2 } else { 1 };
    // SAFETY: `descriptors` is initialized storage for `count` live file
    // descriptors, and `poll` only reads and writes those entries.
    let result = unsafe { libc::poll(descriptors.as_mut_ptr(), count as libc::nfds_t, -1) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    let command_events = libc::POLLIN | libc::POLLERR | libc::POLLHUP | libc::POLLNVAL;
    let connection_errors = libc::POLLERR | libc::POLLHUP | libc::POLLNVAL;
    Ok(Readiness {
        command: descriptors[0].revents & command_events != 0,
        connection_error: connection.is_some() && descriptors[1].revents & connection_errors != 0,
    })
}

fn drain_wake(wake_reader: &mut UnixStream) -> io::Result<()> {
    let mut bytes = [0_u8; 128];
    loop {
        match wake_reader.read(&mut bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "wake pipe closed",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) => return Err(error),
        }
    }
}

fn drain_events(
    connection: &x11rb::rust_connection::RustConnection,
    active: &mut HashMap<i32, ActiveGrab>,
) -> Result<(), String> {
    loop {
        match connection.poll_for_event() {
            Ok(Some(event)) => handle_event(event, active),
            Ok(None) => return Ok(()),
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn fail_active(active: &mut HashMap<i32, ActiveGrab>, fault: HotkeyFault) {
    for (id, grab) in active.drain() {
        set_status(&grab.status, LinuxHotkeyStatus::Failed(fault.clone()));
        wake(LinuxHotkeyEvent::Ready {
            id,
            generation: grab.generation,
        });
    }
}

fn ungrab_all(
    connection: Option<&x11rb::rust_connection::RustConnection>,
    root: Window,
    active: &HashMap<i32, ActiveGrab>,
) {
    if let Some(connection) = connection {
        for grab in active.values() {
            let _ = ungrab(connection, root, grab);
        }
        let _ = connection.flush();
    }
}

fn handle_command(
    command: WorkerCommand,
    connection: &mut Option<x11rb::rust_connection::RustConnection>,
    root: &mut Window,
    active: &mut HashMap<i32, ActiveGrab>,
) -> bool {
    match command {
        WorkerCommand::Register {
            id,
            generation,
            hotkey,
            status,
            activation,
            cancelled,
        } => {
            if cancelled.load(Ordering::Acquire) {
                return true;
            }
            if let Some(previous) = active.remove(&id) {
                let release_error = connection
                    .as_ref()
                    .and_then(|conn| ungrab(conn, *root, &previous).err());
                if let Some(error) = release_error {
                    *connection = None;
                    *root = 0;
                    fail_active(
                        active,
                        HotkeyFault::Refused(format!(
                            "could not release the previous X11 hotkey: {error}"
                        )),
                    );
                }
            }
            let registration = (|| {
                if cancelled.load(Ordering::Acquire) {
                    return Err(HotkeyFault::Refused(
                        "the X11 shortcut request was cancelled".to_owned(),
                    ));
                }
                if connection.is_none() {
                    let (conn, screen) = x11rb::connect(None).map_err(|error| {
                        HotkeyFault::Refused(format!("could not connect to X11: {error}"))
                    })?;
                    *root = conn
                        .setup()
                        .roots
                        .get(screen)
                        .map(|screen| screen.root)
                        .ok_or_else(|| {
                            HotkeyFault::Refused("the X11 connection has no screen".to_owned())
                        })?;
                    enable_detectable_auto_repeat(&conn)?;
                    *connection = Some(conn);
                }
                let conn = connection.as_ref().ok_or_else(|| {
                    HotkeyFault::Refused("the X11 connection is unavailable".to_owned())
                })?;
                let keyboard = keyboard_map(conn)?;
                let keycode = find_keycode(&keyboard, u32::from(hotkey.virtual_key))
                    .ok_or(HotkeyFault::NoSuchKey)?;
                let (base_mask, ignored_locks) = modifier_mask(conn, &keyboard, hotkey)?;
                let masks = lock_variants(base_mask, ignored_locks);
                if cancelled.load(Ordering::Acquire) {
                    return Err(HotkeyFault::Refused(
                        "the X11 shortcut request was cancelled".to_owned(),
                    ));
                }
                for mask in &masks {
                    let cookie = match conn.grab_key(
                        false,
                        *root,
                        *mask,
                        keycode,
                        GrabMode::ASYNC,
                        GrabMode::ASYNC,
                    ) {
                        Ok(cookie) => cookie,
                        Err(error) => {
                            let _ = ungrab_key(conn, *root, keycode, &masks);
                            return Err(HotkeyFault::Refused(format!(
                                "could not request XGrabKey: {error}"
                            )));
                        }
                    };
                    if let Err(error) = cookie.check() {
                        let _ = ungrab_key(conn, *root, keycode, &masks);
                        return Err(grab_fault(error));
                    }
                }
                if let Err(error) = conn.flush() {
                    let _ = ungrab_key(conn, *root, keycode, &masks);
                    return Err(HotkeyFault::Refused(format!(
                        "could not flush the X11 shortcut grab: {error}"
                    )));
                }
                Ok((keycode, masks, base_mask, ignored_locks))
            })();

            if cancelled.load(Ordering::Acquire) {
                if let Ok((keycode, masks, ..)) = registration
                    && let Some(conn) = connection.as_ref()
                {
                    let _ = ungrab_key(conn, *root, keycode, &masks);
                }
                return true;
            }

            match registration {
                Ok((keycode, masks, base_mask, ignored_locks)) => {
                    active.insert(
                        id,
                        ActiveGrab {
                            generation,
                            keycode,
                            masks,
                            base_mask,
                            ignored_locks,
                            pressed: false,
                            status: Arc::clone(&status),
                            activation,
                            cancelled,
                        },
                    );
                    set_status(&status, LinuxHotkeyStatus::Active);
                }
                Err(fault) => set_status(&status, LinuxHotkeyStatus::Failed(fault)),
            }
            wake(LinuxHotkeyEvent::Ready { id, generation });
            true
        }
        WorkerCommand::Release {
            id,
            generation,
            acknowledgment,
        } => {
            if let Err(error) = release_active_grab(
                active,
                id,
                generation,
                |grab| {
                    connection
                        .as_ref()
                        .map_or(Ok(()), |conn| ungrab(conn, *root, grab))
                },
                acknowledgment,
            ) {
                *connection = None;
                *root = 0;
                fail_active(
                    active,
                    HotkeyFault::Refused(format!("could not release the X11 hotkey: {error}")),
                );
            }
            true
        }
        WorkerCommand::Shutdown => false,
    }
}

fn release_active_grab(
    active: &mut HashMap<i32, ActiveGrab>,
    id: i32,
    generation: u64,
    ungrab: impl FnOnce(&ActiveGrab) -> Result<(), String>,
    acknowledgment: Option<mpsc::SyncSender<Result<(), String>>>,
) -> Result<(), String> {
    let result = match active.get(&id) {
        Some(grab) if grab.generation == generation => {
            active.remove(&id).map_or(Ok(()), |grab| ungrab(&grab))
        }
        _ => Ok(()),
    };
    if let Some(acknowledgment) = acknowledgment {
        let _ = acknowledgment.send(result.clone());
    }
    result
}

fn handle_event(event: Event, active: &mut HashMap<i32, ActiveGrab>) {
    match event {
        Event::KeyPress(event) => handle_key_press(event, active),
        Event::KeyRelease(event) => handle_key_release(event, active),
        _ => {}
    }
}

fn handle_key_press(event: KeyPressEvent, active: &mut HashMap<i32, ActiveGrab>) {
    for (id, grab) in active.iter_mut() {
        if grab.cancelled.load(Ordering::Acquire) || event.detail != grab.keycode {
            continue;
        }
        let modifiers_match = normalize_state(event.state, grab.ignored_locks) == grab.base_mask;
        if !begin_press(&mut grab.pressed, modifiers_match) {
            continue;
        }
        grab.activation.store(true, Ordering::Release);
        wake(LinuxHotkeyEvent::Activated {
            id: *id,
            generation: grab.generation,
            pointer: (i32::from(event.root_x), i32::from(event.root_y)),
        });
    }
}

fn handle_key_release(event: KeyReleaseEvent, active: &mut HashMap<i32, ActiveGrab>) {
    for grab in active.values_mut() {
        if event.detail == grab.keycode {
            end_press(&mut grab.pressed);
        }
    }
}

fn begin_press(pressed: &mut bool, matches_chord: bool) -> bool {
    if *pressed || !matches_chord {
        return false;
    }
    *pressed = true;
    true
}

fn end_press(pressed: &mut bool) {
    *pressed = false;
}

fn normalize_state(state: KeyButMask, ignored_locks: u16) -> u16 {
    u16::from(state) & !ignored_locks
}

fn set_status(target: &Mutex<LinuxHotkeyStatus>, status: LinuxHotkeyStatus) {
    *target
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = status;
}

fn enable_detectable_auto_repeat(conn: &impl Connection) -> Result<(), HotkeyFault> {
    let use_extension = xkb::use_extension(conn, 1, 0)
        .map_err(|error| HotkeyFault::Refused(format!("could not query XKB: {error}")))?
        .reply()
        .map_err(|error| HotkeyFault::Refused(format!("could not query XKB: {error}")))?;
    if !use_extension.supported {
        return Err(HotkeyFault::Refused(
            "the X11 server does not support XKB detectable auto-repeat".to_owned(),
        ));
    }
    let flag = PerClientFlag::DETECTABLE_AUTO_REPEAT;
    let reply = xkb::per_client_flags(
        conn,
        u16::from(xkb::ID::USE_CORE_KBD),
        flag,
        flag,
        BoolCtrl::default(),
        BoolCtrl::default(),
        BoolCtrl::default(),
    )
    .map_err(|error| {
        HotkeyFault::Refused(format!(
            "could not enable XKB auto-repeat handling: {error}"
        ))
    })?
    .reply()
    .map_err(|error| {
        HotkeyFault::Refused(format!(
            "could not enable XKB auto-repeat handling: {error}"
        ))
    })?;
    if u32::from(reply.supported) & u32::from(flag) == 0
        || u32::from(reply.value) & u32::from(flag) == 0
    {
        return Err(HotkeyFault::Refused(
            "the X11 server refused detectable auto-repeat handling".to_owned(),
        ));
    }
    Ok(())
}

struct KeyboardMap {
    min_keycode: u8,
    per_keycode: usize,
    keysyms: Vec<u32>,
}

impl KeyboardMap {
    fn symbols(&self, keycode: u8) -> &[u32] {
        let index = usize::from(keycode.saturating_sub(self.min_keycode)) * self.per_keycode;
        self.keysyms
            .get(index..index.saturating_add(self.per_keycode))
            .unwrap_or_default()
    }
}

fn keyboard_map(conn: &impl Connection) -> Result<KeyboardMap, HotkeyFault> {
    let setup = conn.setup();
    let count = setup
        .max_keycode
        .checked_sub(setup.min_keycode)
        .and_then(|range| range.checked_add(1))
        .ok_or_else(|| HotkeyFault::Refused("the X11 keycode range is invalid".to_owned()))?;
    let reply = conn
        .get_keyboard_mapping(setup.min_keycode, count)
        .map_err(|error| {
            HotkeyFault::Refused(format!("could not read the X11 keyboard map: {error}"))
        })?
        .reply()
        .map_err(|error| {
            HotkeyFault::Refused(format!("could not read the X11 keyboard map: {error}"))
        })?;
    Ok(KeyboardMap {
        min_keycode: setup.min_keycode,
        per_keycode: usize::from(reply.keysyms_per_keycode),
        keysyms: reply.keysyms,
    })
}

fn find_keycode(map: &KeyboardMap, keysym: u32) -> Option<Keycode> {
    if map.per_keycode == 0 {
        return None;
    }
    (map.min_keycode..)
        .take(map.keysyms.len() / map.per_keycode)
        .find(|keycode| map.symbols(*keycode).contains(&keysym))
}

fn modifier_mask(
    conn: &impl Connection,
    keyboard: &KeyboardMap,
    hotkey: Hotkey,
) -> Result<(u16, u16), HotkeyFault> {
    let mapping = conn
        .get_modifier_mapping()
        .map_err(|error| {
            HotkeyFault::Refused(format!("could not read the X11 modifier map: {error}"))
        })?
        .reply()
        .map_err(|error| {
            HotkeyFault::Refused(format!("could not read the X11 modifier map: {error}"))
        })?;
    let slots = usize::from(mapping.keycodes_per_modifier());
    let mut alt = None;
    let mut alt_split = false;
    let mut super_key = None;
    let mut super_split = false;
    let mut locks = 0_u16;
    if slots != 0 {
        for (slot, keycodes) in mapping.keycodes.chunks(slots).enumerate() {
            let mask = 1_u16 << slot;
            for keycode in keycodes.iter().copied().filter(|keycode| *keycode != 0) {
                let symbols = keyboard.symbols(keycode);
                if symbols
                    .iter()
                    .any(|keysym| *keysym == xkeysym::key::Alt_L || *keysym == xkeysym::key::Alt_R)
                {
                    note_modifier_group(&mut alt, &mut alt_split, mask);
                }
                if symbols.iter().any(|keysym| {
                    *keysym == xkeysym::key::Super_L || *keysym == xkeysym::key::Super_R
                }) {
                    note_modifier_group(&mut super_key, &mut super_split, mask);
                }
                if symbols.iter().any(|keysym| {
                    *keysym == xkeysym::key::Caps_Lock || *keysym == xkeysym::key::Shift_Lock
                }) {
                    locks |= mask;
                }
                if symbols.iter().any(|keysym| {
                    *keysym == xkeysym::key::Num_Lock || *keysym == xkeysym::key::Scroll_Lock
                }) {
                    locks |= mask;
                }
            }
        }
    }
    let mut mask = 0_u16;
    if hotkey.ctrl {
        mask |= u16::from(ModMask::CONTROL);
    }
    if hotkey.shift {
        mask |= u16::from(ModMask::SHIFT);
    }
    if hotkey.alt {
        if alt_split {
            return Err(HotkeyFault::Refused(
                "the X11 modifier map places left and right Alt in different groups".to_owned(),
            ));
        }
        add_modifier_group(
            &mut mask,
            alt.ok_or_else(|| {
                HotkeyFault::Refused("the X11 modifier map has no Alt key".to_owned())
            })?,
        )?;
    }
    if hotkey.win {
        if super_split {
            return Err(HotkeyFault::Refused(
                "the X11 modifier map places left and right Super in different groups".to_owned(),
            ));
        }
        add_modifier_group(
            &mut mask,
            super_key.ok_or_else(|| {
                HotkeyFault::Refused("the X11 modifier map has no Super key".to_owned())
            })?,
        )?;
    }
    if mask == 0 || mask & locks != 0 {
        return Err(HotkeyFault::Refused(
            "the X11 modifier map cannot express this chord without colliding with a lock key"
                .to_owned(),
        ));
    }
    Ok((mask, locks))
}

fn note_modifier_group(group: &mut Option<u16>, split: &mut bool, mask: u16) {
    match group {
        Some(existing) if *existing != mask => *split = true,
        Some(_) => {}
        None => *group = Some(mask),
    }
}

fn add_modifier_group(mask: &mut u16, group: u16) -> Result<(), HotkeyFault> {
    if *mask & group != 0 {
        return Err(HotkeyFault::Refused(
            "the X11 modifier map cannot distinguish the requested chord".to_owned(),
        ));
    }
    *mask |= group;
    Ok(())
}

fn lock_variants(base_mask: u16, ignored_locks: u16) -> Vec<ModMask> {
    let lock_bits = (0..8)
        .filter_map(|slot| {
            let bit = 1_u16 << slot;
            (ignored_locks & bit != 0).then_some(bit)
        })
        .collect::<Vec<_>>();
    let mut masks = vec![base_mask];
    for bit in lock_bits {
        let additional = masks.iter().map(|mask| *mask | bit).collect::<Vec<_>>();
        masks.extend(additional);
    }
    masks.sort_unstable();
    masks.dedup();
    masks.into_iter().map(ModMask::from).collect()
}

fn ungrab(conn: &impl Connection, root: Window, grab: &ActiveGrab) -> Result<(), String> {
    ungrab_key(conn, root, grab.keycode, &grab.masks)
}

fn ungrab_key(
    conn: &impl Connection,
    root: Window,
    keycode: Keycode,
    masks: &[ModMask],
) -> Result<(), String> {
    let mut first_error = None;
    for mask in masks {
        match conn.ungrab_key(keycode, root, *mask) {
            Ok(cookie) => {
                if let Err(error) = cookie.check()
                    && first_error.is_none()
                {
                    first_error = Some(error.to_string());
                }
            }
            Err(error) if first_error.is_none() => first_error = Some(error.to_string()),
            Err(_) => {}
        }
    }
    if let Err(error) = conn.flush()
        && first_error.is_none()
    {
        first_error = Some(error.to_string());
    }
    first_error.map_or(Ok(()), Err)
}

fn grab_fault(error: ReplyError) -> HotkeyFault {
    match error {
        ReplyError::X11Error(ref error) if error.error_kind == ErrorKind::Access => {
            HotkeyFault::AlreadyRegistered
        }
        other => HotkeyFault::Refused(format!("XGrabKey failed: {other}")),
    }
}

/// Convert a recorded Linux key into the XKB keysym currency stored in `Hotkey`.
#[must_use]
pub fn keysym_for(key: crate::hotkey::SummonKey) -> Option<u16> {
    use crate::hotkey::SummonKey;
    use crate::hotkey::SummonNamedKey;
    use xkeysym::key;

    let keysym = match key {
        SummonKey::Character(character) => xkeysym::Keysym::from_char(character).raw(),
        SummonKey::Named(named) => match named {
            SummonNamedKey::Tab => key::Tab,
            SummonNamedKey::Escape => key::Escape,
            SummonNamedKey::Enter => key::Return,
            SummonNamedKey::Space => key::space,
            SummonNamedKey::Backspace => key::BackSpace,
            SummonNamedKey::Delete => key::Delete,
            SummonNamedKey::Insert => key::Insert,
            SummonNamedKey::Home => key::Home,
            SummonNamedKey::End => key::End,
            SummonNamedKey::PageUp => key::Page_Up,
            SummonNamedKey::PageDown => key::Page_Down,
            SummonNamedKey::ArrowLeft => key::Left,
            SummonNamedKey::ArrowUp => key::Up,
            SummonNamedKey::ArrowRight => key::Right,
            SummonNamedKey::ArrowDown => key::Down,
            SummonNamedKey::F1 => key::F1,
            SummonNamedKey::F2 => key::F2,
            SummonNamedKey::F3 => key::F3,
            SummonNamedKey::F4 => key::F4,
            SummonNamedKey::F5 => key::F5,
            SummonNamedKey::F6 => key::F6,
            SummonNamedKey::F7 => key::F7,
            SummonNamedKey::F8 => key::F8,
            SummonNamedKey::F9 => key::F9,
            SummonNamedKey::F10 => key::F10,
            SummonNamedKey::F11 => key::F11,
            SummonNamedKey::F12 => key::F12,
        },
    };
    if keysym == xkeysym::key::NoSymbol {
        None
    } else {
        u16::try_from(keysym).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActiveGrab, LinuxGlobalHotkey, LinuxHotkeyStatus, SERVICE, ServiceState,
        WAYLAND_SUMMON_REFUSAL, add_modifier_group, begin_press, end_press, grab_fault, keysym_for,
        lock_variants, note_modifier_group, release_active_grab, service_sender,
        shutdown_hotkey_worker,
    };
    use crate::hotkey::{Hotkey, HotkeyFault, SummonKey, SummonNamedKey};
    use crate::linux_window::Backend;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, mpsc};

    fn active_grab(generation: u64) -> ActiveGrab {
        ActiveGrab {
            generation,
            keycode: 33,
            masks: vec![x11rb::protocol::xproto::ModMask::CONTROL],
            base_mask: 1 << 2,
            ignored_locks: 0,
            pressed: false,
            status: Arc::new(Mutex::new(LinuxHotkeyStatus::Active)),
            activation: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn default_grave_and_named_keys_have_xkb_symbols() {
        assert_eq!(keysym_for(SummonKey::Character('`')), Some(u16::from(b'`')));
        assert_eq!(
            keysym_for(SummonKey::Named(SummonNamedKey::Escape)),
            Some(xkeysym::key::Escape as u16)
        );
        assert_eq!(
            keysym_for(SummonKey::Named(SummonNamedKey::F12)),
            Some(xkeysym::key::F12 as u16)
        );
    }

    #[test]
    fn keysyms_outside_the_shared_hotkey_field_are_refused() {
        assert_eq!(keysym_for(SummonKey::Character('中')), None);
    }

    #[test]
    fn native_wayland_refusal_does_not_claim_a_portal_binding() {
        let hotkey = Hotkey {
            ctrl: true,
            alt: false,
            shift: false,
            win: false,
            virtual_key: u16::from(b'`'),
        };
        let error = match super::register_global_hotkey(Backend::Wayland, 1, hotkey) {
            Err(error) => error,
            Ok(_) => panic!("Wayland toggle needs unsupported window lifecycle operations"),
        };
        assert_eq!(
            error,
            HotkeyFault::Refused(WAYLAND_SUMMON_REFUSAL.to_owned())
        );
        assert!(WAYLAND_SUMMON_REFUSAL.contains("GlobalShortcuts portal"));
        assert!(WAYLAND_SUMMON_REFUSAL.contains("hide, restore, or focus"));
        assert!(WAYLAND_SUMMON_REFUSAL.contains("folio --new-window"));
    }

    #[test]
    fn x11_bad_access_is_reported_as_a_conflicting_claim() {
        let error = x11rb::errors::ReplyError::X11Error(x11rb::x11_utils::X11Error {
            error_kind: x11rb::protocol::ErrorKind::Access,
            error_code: 10,
            sequence: 1,
            bad_value: 0,
            minor_opcode: 0,
            major_opcode: 0,
            extension_name: None,
            request_name: Some("GrabKey"),
        });
        assert_eq!(grab_fault(error), HotkeyFault::AlreadyRegistered);
    }

    #[test]
    fn x11_grabs_cover_each_state_of_the_two_ignored_lock_modifiers() {
        let masks = lock_variants(1 << 6, (1 << 1) | (1 << 4));
        let raw = masks
            .iter()
            .map(|mask| u16::from(*mask))
            .collect::<Vec<_>>();
        assert_eq!(raw, [0x40, 0x42, 0x50, 0x52]);
    }

    #[test]
    fn x11_does_not_add_modifier_bits_absent_from_the_keyboard_map() {
        assert_eq!(
            lock_variants(0x40, 0)
                .iter()
                .map(|mask| u16::from(*mask))
                .collect::<Vec<_>>(),
            [0x40]
        );
    }

    #[test]
    fn modifier_groups_are_not_guessed_when_the_keyboard_map_splits_them() {
        let mut alt = None;
        let mut split = false;
        note_modifier_group(&mut alt, &mut split, 1 << 3);
        note_modifier_group(&mut alt, &mut split, 1 << 3);
        assert!(!split, "left and right Alt share one modifier group");
        note_modifier_group(&mut alt, &mut split, 1 << 4);
        assert!(
            split,
            "split Alt mappings cannot be represented by one grab"
        );

        let mut mask = u16::from(x11rb::protocol::xproto::ModMask::CONTROL);
        let control = u16::from(x11rb::protocol::xproto::ModMask::CONTROL);
        assert!(add_modifier_group(&mut mask, control).is_err());
        assert_eq!(mask, control);
    }

    #[test]
    fn a_held_key_activates_once_until_its_release() {
        let mut pressed = false;
        assert!(
            !begin_press(&mut pressed, false),
            "wrong chord does not activate"
        );
        assert!(begin_press(&mut pressed, true));
        assert!(
            !begin_press(&mut pressed, true),
            "repeat presses are suppressed"
        );
        end_press(&mut pressed);
        assert!(
            begin_press(&mut pressed, true),
            "a later press is a new activation"
        );
    }

    #[test]
    fn release_ack_follows_the_ungrab_and_stale_generations_keep_the_live_grab() {
        let mut active = HashMap::from([(7, active_grab(12))]);
        let released = AtomicBool::new(false);
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        release_active_grab(
            &mut active,
            7,
            12,
            |grab| {
                assert_eq!(grab.generation, 12);
                released.store(true, Ordering::Release);
                Ok(())
            },
            Some(ack_tx),
        )
        .expect("the matching grab released");
        assert_eq!(ack_rx.recv().unwrap(), Ok(()));
        assert!(released.load(Ordering::Acquire));
        assert!(!active.contains_key(&7));

        active.insert(7, active_grab(13));
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        release_active_grab(
            &mut active,
            7,
            12,
            |_| panic!("a stale generation must not release the current grab"),
            Some(ack_tx),
        )
        .expect("a stale release is harmless");
        assert_eq!(ack_rx.recv().unwrap(), Ok(()));
        assert_eq!(active.get(&7).map(|grab| grab.generation), Some(13));
    }

    #[test]
    fn dropping_a_claim_queues_release_and_shutdown_joins_the_idle_worker() {
        assert!(matches!(
            &*SERVICE
                .get_or_init(|| Mutex::new(ServiceState::NotStarted))
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            ServiceState::NotStarted
        ));
        let sender = service_sender().expect("start idle worker without opening a display");
        let claim = LinuxGlobalHotkey {
            id: 41,
            generation: 9,
            status: Arc::new(Mutex::new(LinuxHotkeyStatus::Pending)),
            activation: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            sender,
        };
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        claim
            .queue_release(Some(ack_tx))
            .expect("queue release acknowledgment");
        assert_eq!(ack_rx.recv().unwrap(), Ok(()));
        drop(claim);

        let (answer, wait) = mpsc::channel();
        let shutdown_worker = crate::spawn_at_priority(
            "bt-linux-hotkey-shutdown-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| {
                let _ = answer.send(shutdown_hotkey_worker(worker));
            },
        )
        .expect("start shutdown worker");
        wait.recv()
            .expect("shutdown worker returns")
            .expect("stop and join the worker after its release");
        shutdown_worker.join().expect("shutdown worker joins");
        assert!(matches!(
            &*SERVICE
                .get_or_init(|| Mutex::new(ServiceState::NotStarted))
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            ServiceState::Stopped
        ));
    }
}
