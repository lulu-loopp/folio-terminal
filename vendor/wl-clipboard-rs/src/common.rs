// MODIFIED BY THE FOLIO CONTRIBUTORS: share cancellable socket and roundtrip setup; see CHANGES-FOLIO.md.
use std::collections::HashMap;
use std::ffi::OsString;
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{env, io};

use rustix::event::{poll, PollFd, PollFlags, Timespec};
use wayland_backend::client::WaylandError;
use wayland_client::globals::{registry_queue_init, Global, GlobalError, GlobalListContents};
use wayland_client::protocol::wl_callback::WlCallback;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::{self, WlSeat};
use wayland_client::{
    ConnectError, Connection, Dispatch, DispatchError, EventQueue, Proxy, QueueHandle,
};
use wayland_protocols::ext::data_control::v1::client::ext_data_control_manager_v1::ExtDataControlManagerV1;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1;

use crate::data_control::Manager;
use crate::seat_data::SeatData;

const CANCELLATION_POLL: Duration = Duration::from_millis(20);

// MODIFIED BY THE FOLIO CONTRIBUTORS: path-based connections fail fast instead of waiting on a
// full socket backlog; see `CHANGES-FOLIO.md`.
pub(crate) fn connect_unix_stream_nonblocking(path: &Path) -> io::Result<UnixStream> {
    let socket = rustix::net::socket_with(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::STREAM,
        rustix::net::SocketFlags::NONBLOCK | rustix::net::SocketFlags::CLOEXEC,
        None,
    )
    .map_err(io::Error::from)?;
    let address = rustix::net::SocketAddrUnix::new(path).map_err(io::Error::from)?;
    rustix::net::connect(&socket, &address).map_err(io::Error::from)?;
    Ok(UnixStream::from(socket))
}

/// Completion token carried by a cancellable `wl_display.sync` callback.
pub(crate) struct SyncTicket(pub(crate) Arc<AtomicBool>);

/// A server roundtrip retained on the thread that owns its event queue.
pub(crate) struct PendingRoundtrip {
    _callback: WlCallback,
    completed: Arc<AtomicBool>,
}

impl PendingRoundtrip {
    pub(crate) fn is_complete(&self) -> bool {
        self.completed.load(Ordering::Acquire)
    }
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum RoundtripError {
    #[error("clipboard operation was cancelled")]
    Cancelled,
    #[error("clipboard operation exceeded its deadline")]
    DeadlineExceeded,
    #[error("Wayland compositor communication error")]
    Communication(#[source] DispatchError),
    #[error("Wayland clipboard wait failed: {0}")]
    Poll(#[source] io::Error),
}

fn check_control(deadline: Instant, cancelled: &AtomicBool) -> Result<(), RoundtripError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(RoundtripError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(RoundtripError::DeadlineExceeded);
    }
    Ok(())
}

/// Queue and flush a server sync request while retaining its callback proxy.
/// The caller must keep the returned ticket on the event-queue's owning thread.
pub(crate) fn begin_roundtrip<S>(
    connection: &Connection,
    queue: &EventQueue<S>,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<PendingRoundtrip, RoundtripError>
where
    S: Dispatch<WlCallback, SyncTicket> + 'static,
{
    check_control(deadline, cancelled)?;
    let completed = Arc::new(AtomicBool::new(false));
    let callback = connection
        .display()
        .sync(&queue.handle(), SyncTicket(Arc::clone(&completed)));
    queue
        .flush()
        .map_err(|error| RoundtripError::Communication(DispatchError::Backend(error)))?;
    check_control(deadline, cancelled)?;
    Ok(PendingRoundtrip {
        _callback: callback,
        completed,
    })
}

/// Drive the owning event queue until a previously sent sync callback is done.
/// Timeout leaves the ticket live so an owner can reconcile it later.
pub(crate) fn wait_roundtrip_cancellable<S>(
    queue: &mut EventQueue<S>,
    state: &mut S,
    pending: &PendingRoundtrip,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<(), RoundtripError>
where
    S: Dispatch<WlCallback, SyncTicket> + 'static,
{
    wait_roundtrip_cancellable_when(queue, state, pending, deadline, || {
        cancelled.load(Ordering::Acquire)
    })
}

/// The same queue-owned barrier wait when cancellation comes from more than
/// one owner, such as an operation and the serving connection's retirement.
pub(crate) fn wait_roundtrip_cancellable_when<S>(
    queue: &mut EventQueue<S>,
    state: &mut S,
    pending: &PendingRoundtrip,
    deadline: Instant,
    is_cancelled: impl Fn() -> bool,
) -> Result<(), RoundtripError>
where
    S: Dispatch<WlCallback, SyncTicket> + 'static,
{
    while !pending.is_complete() {
        if is_cancelled() {
            return Err(RoundtripError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(RoundtripError::DeadlineExceeded);
        }
        queue
            .dispatch_pending(state)
            .map_err(RoundtripError::Communication)?;
        if pending.is_complete() {
            break;
        }

        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        let timeout = Timespec::try_from(remaining.min(CANCELLATION_POLL))
            .map_err(|error| RoundtripError::Poll(io::Error::other(error.to_string())))?;
        let connection_fd = guard.connection_fd();
        let mut fds = [PollFd::new(&connection_fd, PollFlags::IN)];
        poll(&mut fds, Some(&timeout))
            .map_err(|error| RoundtripError::Poll(io::Error::from(error)))?;
        if is_cancelled() {
            return Err(RoundtripError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(RoundtripError::DeadlineExceeded);
        }
        if fds[0]
            .revents()
            .intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR)
        {
            guard
                .read()
                .map_err(|error| RoundtripError::Communication(DispatchError::Backend(error)))?;
            queue
                .dispatch_pending(state)
                .map_err(RoundtripError::Communication)?;
        }
    }
    Ok(())
}

pub(crate) fn roundtrip_cancellable<S>(
    connection: &Connection,
    queue: &mut EventQueue<S>,
    state: &mut S,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<(), RoundtripError>
where
    S: Dispatch<WlCallback, SyncTicket> + 'static,
{
    let pending = begin_roundtrip(connection, queue, deadline, cancelled)?;
    wait_roundtrip_cancellable(queue, state, &pending, deadline, cancelled)
}

/// Connect to the selected Wayland socket without waiting on a full listen
/// backlog. This is shared by reads and owned clipboard writes.
pub(crate) fn connect_wayland(socket_name: Option<OsString>) -> Result<Connection, Error> {
    match socket_name {
        Some(name) => {
            let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .ok_or(Error::WaylandConnection(ConnectError::NoCompositor))?;
            let stream = connect_unix_stream_nonblocking(&runtime_dir.join(name))
                .map_err(Error::SocketOpenError)?;
            Connection::from_socket(stream).map_err(Error::WaylandConnection)
        }
        None => {
            // Match wayland-client's inherited-socket handling without peeking
            // at the variable and delegating to a second environment read.
            if let Ok(socket_fd) = env::var("WAYLAND_SOCKET") {
                let socket_fd = socket_fd
                    .parse::<i32>()
                    .map_err(|_| Error::WaylandConnection(ConnectError::InvalidFd))?;
                return connect_wayland_socket_fd(socket_fd);
            }

            let socket_name = env::var_os("WAYLAND_DISPLAY")
                .map(PathBuf::from)
                .ok_or(Error::WaylandConnection(ConnectError::NoCompositor))?;
            let socket_path = if socket_name.is_absolute() {
                socket_name
            } else {
                let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .ok_or(Error::WaylandConnection(ConnectError::NoCompositor))?;
                runtime_dir.join(socket_name)
            };
            let stream = connect_unix_stream_nonblocking(&socket_path)
                .map_err(|_| Error::WaylandConnection(ConnectError::NoCompositor))?;
            Connection::from_socket(stream).map_err(Error::WaylandConnection)
        }
    }
}

#[allow(unsafe_code)]
fn connect_wayland_socket_fd(socket_fd: i32) -> Result<Connection, Error> {
    // SAFETY: `WAYLAND_SOCKET` transfers ownership of this already-connected fd
    // to the client, matching wayland-client's `connect_to_env` implementation.
    let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(socket_fd) };
    env::remove_var("WAYLAND_SOCKET");
    let flags = rustix::io::fcntl_getfd(&fd)
        .map_err(|_| Error::WaylandConnection(ConnectError::InvalidFd))?;
    rustix::io::fcntl_setfd(&fd, flags | rustix::io::FdFlags::CLOEXEC)
        .map_err(|_| Error::WaylandConnection(ConnectError::InvalidFd))?;
    let stream = UnixStream::from(fd);
    Connection::from_socket(stream).map_err(Error::WaylandConnection)
}

pub struct State {
    pub seats: HashMap<WlSeat, SeatData>,
    pub clipboard_manager: Manager,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[allow(clippy::enum_variant_names)]
    #[error("Couldn't open the provided Wayland socket")]
    SocketOpenError(#[source] io::Error),

    #[error("Couldn't connect to the Wayland compositor")]
    WaylandConnection(#[source] ConnectError),

    #[error("Wayland compositor communication error")]
    WaylandCommunication(#[source] WaylandError),

    #[error(
        "A required Wayland protocol ({name} version {version}) is not supported by the compositor"
    )]
    MissingProtocol { name: &'static str, version: u32 },
}

impl<S> Dispatch<WlSeat, (), S> for State
where
    S: Dispatch<WlSeat, ()> + AsMut<State>,
{
    fn event(
        parent: &mut S,
        seat: &WlSeat,
        event: <WlSeat as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &wayland_client::Connection,
        _qh: &wayland_client::QueueHandle<S>,
    ) {
        let state = parent.as_mut();

        if let wl_seat::Event::Name { name } = event {
            state.seats.get_mut(seat).unwrap().set_name(name);
        }
    }
}

pub fn initialize<S>(
    primary: bool,
    socket_name: Option<OsString>,
) -> Result<(Connection, EventQueue<S>, State), Error>
where
    S: Dispatch<WlRegistry, GlobalListContents> + 'static,
    S: Dispatch<ZwlrDataControlManagerV1, ()>,
    S: Dispatch<ExtDataControlManagerV1, ()>,
    S: Dispatch<WlSeat, ()>,
    S: AsMut<State>,
{
    let conn = connect_wayland(socket_name)?;

    // Retrieve the global interfaces.
    let (globals, queue) =
        registry_queue_init::<S>(&conn).map_err(|err| match err {
                                           GlobalError::Backend(err) => Error::WaylandCommunication(err),
                                           GlobalError::InvalidId(err) => panic!("How's this possible? \
                                                                                  Is there no wl_registry? \
                                                                                  {:?}",
                                                                                 err),
                                       })?;
    let qh = queue.handle();
    let registry = globals.registry();
    let collected = globals.contents().with_list(|globals| {
        globals
            .iter()
            .map(|global| Global {
                name: global.name,
                interface: global.interface.clone(),
                version: global.version,
            })
            .collect::<Vec<_>>()
    });
    let state = initialize_from_globals(primary, &registry, &collected, &qh)?;
    Ok((conn, queue, state))
}

/// Bind the clipboard manager and seats from a completed registry snapshot.
/// Both the legacy initializer and the deadline-controlled owner use this
/// one implementation.
pub(crate) fn initialize_from_globals<S>(
    primary: bool,
    registry: &WlRegistry,
    globals: &[Global],
    queue_handle: &QueueHandle<S>,
) -> Result<State, Error>
where
    S: Dispatch<ZwlrDataControlManagerV1, ()>
        + Dispatch<ExtDataControlManagerV1, ()>
        + Dispatch<WlSeat, ()>
        + AsMut<State>
        + 'static,
{
    let ext_manager = globals
        .iter()
        .find(|global| {
            global.interface == ExtDataControlManagerV1::interface().name && global.version >= 1
        })
        .map(|global| Manager::Ext(registry.bind(global.name, 1, queue_handle, ())))
        .or_else(|| {
            let version = if primary { 2 } else { 1 };
            globals
                .iter()
                .find(|global| {
                    global.interface == ZwlrDataControlManagerV1::interface().name
                        && global.version >= version
                })
                .map(|global| Manager::Zwlr(registry.bind(global.name, version, queue_handle, ())))
        });
    let clipboard_manager = ext_manager.ok_or_else(|| Error::MissingProtocol {
        name: "ext-data-control, or wlr-data-control",
        version: if primary { 2 } else { 1 },
    })?;
    let seats = globals
        .iter()
        .filter(|global| global.interface == WlSeat::interface().name && global.version >= 2)
        .map(|global| {
            let seat = registry.bind(global.name, 2, queue_handle, ());
            (seat, SeatData::default())
        })
        .collect();
    Ok(State {
        seats,
        clipboard_manager,
    })
}
