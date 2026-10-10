//! Shared nonblocking X11 clipboard transport for reads and retained owners.

use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::admission::WorkerCtx;
use x11rb::reexports::x11rb_protocol::parse_display::{ConnectAddress, parse_display};
use x11rb::reexports::x11rb_protocol::xauth::get_auth;
use x11rb::rust_connection::{DefaultStream, PollMode, RustConnection, Stream};

pub(crate) const CANCEL_POLL: Duration = Duration::from_millis(10);

#[derive(Clone)]
enum CancelFlag<'a> {
    Borrowed(&'a AtomicBool),
    Shared(std::sync::Arc<AtomicBool>),
}

impl CancelFlag<'_> {
    fn is_set(&self) -> bool {
        match self {
            Self::Borrowed(flag) => flag.load(Ordering::Acquire),
            Self::Shared(flag) => flag.load(Ordering::Acquire),
        }
    }
}

#[derive(Clone)]
enum ControlMode<'a> {
    Operation {
        deadline: Instant,
        cancelled: CancelFlag<'a>,
        lifecycle_cancelled: Option<CancelFlag<'a>>,
        label: &'static str,
    },
    Serving {
        lifecycle_cancelled: CancelFlag<'a>,
    },
    Retirement {
        deadline: Instant,
        cancelled: Option<CancelFlag<'a>>,
        lifecycle_cancelled: CancelFlag<'a>,
    },
}

/// Mutable control for one X11 connection. The same stream can move from
/// operation deadline, to retained serving, and back to bounded retirement.
pub(crate) struct X11TransportControl<'a> {
    mode: Mutex<ControlMode<'a>>,
}

impl<'a> X11TransportControl<'a> {
    pub(crate) fn reader(deadline: Instant, cancelled: &'a AtomicBool) -> Self {
        Self {
            mode: Mutex::new(ControlMode::Operation {
                deadline,
                cancelled: CancelFlag::Borrowed(cancelled),
                lifecycle_cancelled: None,
                label: "X11 clipboard read",
            }),
        }
    }

    pub(crate) fn owner_operation(
        deadline: Instant,
        cancelled: std::sync::Arc<AtomicBool>,
        lifecycle_cancelled: std::sync::Arc<AtomicBool>,
    ) -> Self {
        Self {
            mode: Mutex::new(ControlMode::Operation {
                deadline,
                cancelled: CancelFlag::Shared(cancelled),
                lifecycle_cancelled: Some(CancelFlag::Shared(lifecycle_cancelled)),
                label: "X11 clipboard owner operation",
            }),
        }
    }

    pub(crate) fn begin_owner_operation(
        &self,
        deadline: Instant,
        cancelled: std::sync::Arc<AtomicBool>,
        lifecycle_cancelled: std::sync::Arc<AtomicBool>,
    ) {
        *self
            .mode
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = ControlMode::Operation {
            deadline,
            cancelled: CancelFlag::Shared(cancelled),
            lifecycle_cancelled: Some(CancelFlag::Shared(lifecycle_cancelled)),
            label: "X11 clipboard owner operation",
        };
    }

    pub(crate) fn serving(&self, lifecycle_cancelled: std::sync::Arc<AtomicBool>) {
        *self
            .mode
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = ControlMode::Serving {
            lifecycle_cancelled: CancelFlag::Shared(lifecycle_cancelled),
        };
    }

    pub(crate) fn retiring(
        &self,
        deadline: Instant,
        cancelled: Option<std::sync::Arc<AtomicBool>>,
        lifecycle_cancelled: std::sync::Arc<AtomicBool>,
    ) {
        *self
            .mode
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = ControlMode::Retirement {
            deadline,
            cancelled: cancelled.map(CancelFlag::Shared),
            lifecycle_cancelled: CancelFlag::Shared(lifecycle_cancelled),
        };
    }

    fn snapshot(&self) -> ControlMode<'a> {
        self.mode
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn check(&self) -> io::Result<()> {
        match self.snapshot() {
            ControlMode::Operation {
                deadline,
                cancelled,
                lifecycle_cancelled,
                label,
            } => {
                if cancelled.is_set() || lifecycle_cancelled.is_some_and(|flag| flag.is_set()) {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        format!("{label} was cancelled"),
                    ));
                }
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("{label} exceeded its deadline"),
                    ));
                }
            }
            ControlMode::Serving {
                lifecycle_cancelled,
            } => {
                if lifecycle_cancelled.is_set() {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "X11 clipboard owner serving was cancelled",
                    ));
                }
            }
            ControlMode::Retirement {
                deadline,
                cancelled,
                lifecycle_cancelled,
            } => {
                if cancelled.is_some_and(|flag| flag.is_set()) {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "X11 clipboard owner retirement was canceled by its caller",
                    ));
                }
                if lifecycle_cancelled.is_set() {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "X11 clipboard owner retirement was cancelled",
                    ));
                }
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "X11 clipboard owner retirement exceeded its deadline",
                    ));
                }
            }
        }
        Ok(())
    }

    fn next_poll_interval(&self) -> io::Result<Duration> {
        self.check()?;
        let interval = match self.snapshot() {
            ControlMode::Operation { deadline, .. } | ControlMode::Retirement { deadline, .. } => {
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(CANCEL_POLL)
            }
            ControlMode::Serving { .. } => CANCEL_POLL,
        };
        Ok(interval)
    }
}

pub(crate) struct DeadlineStream<'worker, 'control> {
    inner: DefaultStream,
    worker: &'worker WorkerCtx,
    control: std::sync::Arc<X11TransportControl<'control>>,
}

impl<'worker, 'control> DeadlineStream<'worker, 'control> {
    pub(crate) fn new(
        inner: DefaultStream,
        worker: &'worker WorkerCtx,
        control: std::sync::Arc<X11TransportControl<'control>>,
    ) -> Self {
        Self {
            inner,
            worker,
            control,
        }
    }

    fn check_control(&self) -> io::Result<()> {
        self.control.check()
    }
}

impl Stream for DeadlineStream<'_, '_> {
    fn poll(&self, mode: PollMode) -> io::Result<()> {
        let mut events = 0;
        if mode.readable() {
            events |= libc::POLLIN;
        }
        if mode.writable() {
            events |= libc::POLLOUT;
        }
        poll_until(self.worker, self.inner.as_raw_fd(), events, &self.control)
    }

    fn read(
        &self,
        bytes: &mut [u8],
        fd_storage: &mut Vec<x11rb::utils::RawFdContainer>,
    ) -> io::Result<usize> {
        self.check_control()?;
        self.inner.read(bytes, fd_storage)
    }

    fn write(
        &self,
        bytes: &[u8],
        fds: &mut Vec<x11rb::utils::RawFdContainer>,
    ) -> io::Result<usize> {
        self.check_control()?;
        self.inner.write(bytes, fds)
    }

    fn write_vectored(
        &self,
        bufs: &[std::io::IoSlice<'_>],
        fds: &mut Vec<x11rb::utils::RawFdContainer>,
    ) -> io::Result<usize> {
        self.check_control()?;
        self.inner.write_vectored(bufs, fds)
    }
}

fn poll_until(
    _worker: &WorkerCtx,
    fd: RawFd,
    events: libc::c_short,
    control: &X11TransportControl<'_>,
) -> io::Result<()> {
    loop {
        let interval = control.next_poll_interval()?;
        let timeout = interval
            .as_millis()
            .saturating_add(if !interval.subsec_nanos().is_multiple_of(1_000_000) {
                1
            } else {
                0
            })
            .max(1)
            .min(i32::MAX as u128) as i32;
        let mut descriptor = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        // The descriptor is borrowed for this synchronous poll and remains
        // owned by the caller for the entire call.
        let result = unsafe { libc::poll(&mut descriptor, 1, timeout) };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        control.check()?;
        if result > 0 {
            return Ok(());
        }
    }
}

pub(crate) fn connect_display<'a>(
    worker: &'a WorkerCtx,
    deadline: Instant,
    cancelled: &'a AtomicBool,
) -> Result<(RustConnection<DeadlineStream<'a, 'a>>, usize), String> {
    let control = std::sync::Arc::new(X11TransportControl::reader(deadline, cancelled));
    connect_display_controlled(worker, control)
}

pub(crate) fn connect_display_controlled<'worker, 'control>(
    worker: &'worker WorkerCtx,
    control: std::sync::Arc<X11TransportControl<'control>>,
) -> Result<(RustConnection<DeadlineStream<'worker, 'control>>, usize), String> {
    let display = parse_display(None)
        .map_err(|error| format!("X11 clipboard DISPLAY parsing failed: {error}"))?;
    let screen = usize::from(display.screen);
    let mut last_error = None;
    for address in display.connect_instruction() {
        control.check().map_err(|error| error.to_string())?;
        let connected = match address {
            ConnectAddress::Socket(path) => connect_unix_display(worker, &path, &control),
            ConnectAddress::Hostname(host, port) => {
                connect_tcp_display(worker, host, port, &control)
            }
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "X11 clipboard display address is unsupported",
            )),
        };
        let (stream, (family, peer)) = match connected {
            Ok(connected) => connected,
            Err(error) => {
                if let Err(control_error) = control.check() {
                    return Err(control_error.to_string());
                }
                last_error = Some(error);
                continue;
            }
        };
        control.check().map_err(|error| error.to_string())?;
        let (auth_name, auth_data) = get_auth(family, &peer, display.display)
            .unwrap_or(None)
            .unwrap_or_default();
        let stream = DeadlineStream::new(stream, worker, control);
        let connection =
            RustConnection::connect_to_stream_with_auth_info(stream, screen, auth_name, auth_data)
                .map_err(|error| format!("X11 clipboard connection failed: {error}"))?;
        return Ok((connection, screen));
    }
    Err(format!(
        "X11 clipboard connection failed: {}",
        last_error.map_or_else(
            || "DISPLAY has no supported address".to_owned(),
            |error| error.to_string()
        )
    ))
}

fn connect_unix_display(
    worker: &WorkerCtx,
    path: &str,
    control: &X11TransportControl<'_>,
) -> io::Result<(
    DefaultStream,
    (x11rb::reexports::x11rb_protocol::xauth::Family, Vec<u8>),
)> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if let Ok(stream) = connect_unix_socket(worker, path.as_bytes(), true, control) {
        return DefaultStream::from_unix_stream(stream);
    }
    control.check()?;
    let stream = connect_unix_socket(worker, path.as_bytes(), false, control)?;
    DefaultStream::from_unix_stream(stream)
}

fn connect_unix_socket(
    worker: &WorkerCtx,
    path: &[u8],
    abstract_name: bool,
    control: &X11TransportControl<'_>,
) -> io::Result<UnixStream> {
    if path.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "X11 clipboard Unix socket path contains NUL",
        ));
    }
    let descriptor = new_nonblocking_socket(libc::AF_UNIX)?;
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let bytes = if abstract_name {
        if path.len() + 1 > address.sun_path.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "X11 clipboard abstract socket path is too long",
            ));
        }
        for (index, byte) in path.iter().copied().enumerate() {
            address.sun_path[index + 1] = byte as libc::c_char;
        }
        path.len() + 1
    } else {
        if path.len() + 1 > address.sun_path.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "X11 clipboard Unix socket path is too long",
            ));
        }
        for (index, byte) in path.iter().copied().enumerate() {
            address.sun_path[index] = byte as libc::c_char;
        }
        address.sun_path[path.len()] = 0;
        path.len() + 1
    };
    let address_len = (std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes)
        .try_into()
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "X11 clipboard Unix socket address is too long",
            )
        })?;
    finish_connect(
        worker,
        descriptor.as_raw_fd(),
        (&raw const address).cast(),
        address_len,
        control,
    )?;
    Ok(UnixStream::from(descriptor))
}

fn connect_tcp_display(
    worker: &WorkerCtx,
    host: &str,
    port: u16,
    control: &X11TransportControl<'_>,
) -> io::Result<(
    DefaultStream,
    (x11rb::reexports::x11rb_protocol::xauth::Family, Vec<u8>),
)> {
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    let ip = host.parse::<IpAddr>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "X11 clipboard remote hostnames are unsupported; use a local display socket or literal IP",
        )
    })?;
    let mut last_error = None;
    for address in SocketAddr::new(ip, port).to_socket_addrs()? {
        control.check()?;
        match connect_tcp_socket(worker, address, control) {
            Ok(stream) => return DefaultStream::from_tcp_stream(stream),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "no X11 clipboard TCP address",
        )
    }))
}

fn connect_tcp_socket(
    worker: &WorkerCtx,
    address: SocketAddr,
    control: &X11TransportControl<'_>,
) -> io::Result<TcpStream> {
    match address {
        SocketAddr::V4(address) => {
            let descriptor = new_nonblocking_socket(libc::AF_INET)?;
            let raw = libc::sockaddr_in {
                sin_family: libc::AF_INET as libc::sa_family_t,
                sin_port: address.port().to_be(),
                sin_addr: libc::in_addr {
                    s_addr: u32::from_ne_bytes(address.ip().octets()),
                },
                sin_zero: [0; 8],
            };
            finish_connect(
                worker,
                descriptor.as_raw_fd(),
                (&raw const raw).cast(),
                std::mem::size_of_val(&raw)
                    .try_into()
                    .map_err(|_| io::Error::other("X11 clipboard TCP address is too large"))?,
                control,
            )?;
            Ok(TcpStream::from(descriptor))
        }
        SocketAddr::V6(address) => {
            let descriptor = new_nonblocking_socket(libc::AF_INET6)?;
            let raw = libc::sockaddr_in6 {
                sin6_family: libc::AF_INET6 as libc::sa_family_t,
                sin6_port: address.port().to_be(),
                sin6_flowinfo: address.flowinfo().to_be(),
                sin6_addr: libc::in6_addr {
                    s6_addr: address.ip().octets(),
                },
                sin6_scope_id: address.scope_id(),
            };
            finish_connect(
                worker,
                descriptor.as_raw_fd(),
                (&raw const raw).cast(),
                std::mem::size_of_val(&raw)
                    .try_into()
                    .map_err(|_| io::Error::other("X11 clipboard TCP address is too large"))?,
                control,
            )?;
            Ok(TcpStream::from(descriptor))
        }
    }
}

fn new_nonblocking_socket(domain: libc::c_int) -> io::Result<OwnedFd> {
    // SAFETY: socket returns a new descriptor on success; it is immediately
    // placed under OwnedFd, and all arguments are Linux socket constants.
    let descriptor = unsafe {
        libc::socket(
            domain,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            0,
        )
    };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `descriptor` is a newly created owned socket descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(descriptor) })
}

fn finish_connect(
    worker: &WorkerCtx,
    fd: RawFd,
    address: *const libc::sockaddr,
    address_len: libc::socklen_t,
    control: &X11TransportControl<'_>,
) -> io::Result<()> {
    control.check()?;
    // SAFETY: `address` points to a fully initialized sockaddr of the
    // matching family and length, and `fd` remains owned by the caller.
    let result = unsafe { libc::connect(fd, address, address_len) };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if !matches!(
        error.raw_os_error(),
        Some(libc::EINTR | libc::EINPROGRESS | libc::EALREADY | libc::EAGAIN)
    ) {
        return Err(error);
    }
    poll_until(worker, fd, libc::POLLIN | libc::POLLOUT, control)?;
    let mut socket_error: libc::c_int = 0;
    let mut socket_error_len = std::mem::size_of_val(&socket_error) as libc::socklen_t;
    // SAFETY: both output pointers are valid for their declared sizes and
    // the socket remains owned by the caller.
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            (&raw mut socket_error).cast(),
            &mut socket_error_len,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if socket_error != 0 {
        return Err(io::Error::from_raw_os_error(socket_error));
    }
    control.check()
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc;

    use super::*;
    use x11rb::rust_connection::DefaultStream;

    #[test]
    fn retained_owner_transport_ignores_request_control_and_stops_without_an_x_event() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let (serving_tx, serving_rx) = mpsc::channel();
        let (polling_tx, polling_rx) = mpsc::channel();
        let owner_cancelled = std::sync::Arc::new(AtomicBool::new(false));
        let lifecycle_cancelled = std::sync::Arc::new(AtomicBool::new(false));
        let thread_lifecycle = std::sync::Arc::clone(&lifecycle_cancelled);

        let thread = crate::spawn_at_priority(
            "linux-x11-owner-transport-test",
            crate::ThreadPriority::Normal,
            move |worker| {
                let (inner, _) = DefaultStream::from_unix_stream(client).unwrap();
                let control = std::sync::Arc::new(X11TransportControl::owner_operation(
                    Instant::now() - Duration::from_secs(1),
                    std::sync::Arc::clone(&owner_cancelled),
                    std::sync::Arc::clone(&thread_lifecycle),
                ));
                let stream = DeadlineStream::new(inner, worker, std::sync::Arc::clone(&control));

                control.serving(std::sync::Arc::clone(&thread_lifecycle));
                owner_cancelled.store(true, Ordering::Release);
                serving_tx.send(()).unwrap();
                stream.poll(PollMode::Readable).unwrap();
                let mut byte = [0; 1];
                stream.read(&mut byte, &mut Vec::new()).unwrap();
                assert_eq!(byte, [b'x']);

                polling_tx.send(()).unwrap();
                let error = stream.poll(PollMode::Readable).unwrap_err();
                assert!(error.to_string().contains("owner serving was cancelled"));
            },
        )
        .unwrap();

        serving_rx.recv().unwrap();
        server.write_all(b"x").unwrap();
        polling_rx.recv().unwrap();
        lifecycle_cancelled.store(true, Ordering::Release);
        thread.join().unwrap();
    }

    #[test]
    fn owner_retirement_observes_request_cancel_without_canceling_lifecycle() {
        let operation_cancelled = std::sync::Arc::new(AtomicBool::new(true));
        let lifecycle_cancelled = std::sync::Arc::new(AtomicBool::new(false));
        let cutoff = Instant::now() + CANCEL_POLL;
        let control = X11TransportControl::owner_operation(
            cutoff,
            std::sync::Arc::clone(&operation_cancelled),
            std::sync::Arc::clone(&lifecycle_cancelled),
        );

        control.retiring(
            cutoff,
            Some(operation_cancelled),
            std::sync::Arc::clone(&lifecycle_cancelled),
        );

        assert!(
            control
                .check()
                .unwrap_err()
                .to_string()
                .contains("canceled by its caller")
        );
        assert!(!lifecycle_cancelled.load(Ordering::Acquire));
    }
}
