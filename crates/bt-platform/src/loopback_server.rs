//! **A server on this machine's own loopback for tests, which answers every
//! connection until the test tells it to stop** (0.4.9 plan row 127).
//!
//! The tests' servers used to serve a fixed number of connections and then
//! drop their listener: the trust harness's time-stamp authority answered one,
//! and the download tests' server answered "up to N" for at most 60 s, polling
//! a non-blocking `accept` every 5 ms. A client that connects one more time
//! than the count — a retry after a late answer, a reconnect after an
//! abandoned attempt — then meets a closed port. On 2026-10-31 two signing
//! tests failed that way under load with `SignerSignEx3 0x80072EFD`
//! (`ERROR_WINHTTP_CANNOT_CONNECT`), and passed alone.
//!
//! Here the count and the clock are gone:
//!
//! - **The server answers every connection, one after another, until
//!   [`Server::stop`].** What a connection is answered with is the caller's
//!   `answer`, and what it returns for each connection is **recorded**. The
//!   record comes back from `stop`, so a test can say how many connections came
//!   and what each one was — a connection that arrived after the stop was
//!   raised, never served, and a failed `accept` included ([`Entry`]).
//! - **One connection at a time.** A client that opens a connection and keeps
//!   it silent holds every later connection in the backlog until the stop
//!   (the clients these servers stand in for — WinHTTP per request, and
//!   `NSURLSession` with one connection per host — do not). Each connection
//!   is closed after its client closes (row 154: a half-close, then a read
//!   to the client's end), so a client that keeps a connection open once
//!   its answer is done holds the later ones the same way, until it closes
//!   or the stop ends it; those clients close on `Connection: close`.
//! - **Stopping is a signal, never a count or a time.** `stop` raises the flag,
//!   shuts the connection being served (if any) and cancels a read or a write
//!   already waiting on it, so the answer ends, wakes a [`Stop::pause`], and
//!   makes one connection of its own so a worker blocked in `accept` sees the
//!   flag. Then it joins the worker. Every wait it ends is one it causes. A
//!   server dropped without `stop` stops the same way (and, when the test is
//!   already unwinding, does not wait for its worker).
//!
//! Tests only: this crate's own (`cfg(test)`), and those of a crate that turns
//! on `trust-harness` (the trust harness's time-stamp authority stands on it).

use std::io;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

/// **A loopback server**, answering every connection with its `answer` and
/// recording what each answer returned, until [`Server::stop`] or its drop.
pub struct Server<E> {
    address: SocketAddr,
    shared: Arc<Shared>,
    /// Taken by `stop` or the drop, whichever comes first.
    worker: Option<JoinHandle<Vec<Entry<E>>>>,
}

/// **One entry of a server's record**, in the order the worker met them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry<E> {
    /// A connection served: what its `answer` returned.
    Served(E),
    /// A connection from this peer, accepted after the stop was raised and
    /// never served — a client still in the backlog when the test was done
    /// (the stop's own wake-up connection is not one).
    ArrivedAfterTheStop(SocketAddr),
    /// An `accept` that failed. One of a connection reset or aborted while it
    /// waited in the backlog is that client's, and the worker goes on; any
    /// other is the listener's own, would fail again, and ends the worker.
    AcceptFailed { kind: io::ErrorKind, error: String },
}

/// What the test's side and the worker share.
struct Shared {
    state: Mutex<State>,
    /// Signalled when the stop is raised, for [`Stop::pause`], and when the
    /// stop's wake-up connection is made.
    stopping: Condvar,
}

#[derive(Default)]
struct State {
    stopped: bool,
    /// A handle on the connection the worker is serving, so the stop can shut
    /// it and end a read or a write the client is not ending.
    serving: Option<TcpStream>,
    /// The stop's own wake-up connection, as the worker will see it.
    wake_up: WakeUp,
}

/// The stop's wake-up connection: not made yet, made from this local address,
/// or refused (the worker had already ended and its listener with it).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum WakeUp {
    #[default]
    Pending,
    From(SocketAddr),
    Refused,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// **The stop, as an `answer` sees it**: whether it has been asked for, and a
/// pause that ends when it is.
pub struct Stop(Arc<Shared>);

impl Stop {
    /// Whether the test has asked the server to stop.
    #[must_use]
    pub fn requested(&self) -> bool {
        self.0.state().stopped
    }

    /// **Say nothing for `length`, or until the stop**, answering whether the
    /// stop came. The length is the script's (a server that goes silent is
    /// what some tests are about); the stop always cuts it short.
    #[must_use]
    pub fn pause(&self, length: Duration) -> bool {
        let state = self.0.state();
        let (state, _) = self
            .0
            .stopping
            .wait_timeout_while(state, length, |state| !state.stopped)
            .unwrap_or_else(PoisonError::into_inner);
        state.stopped
    }
}

impl<E: Send + 'static> Server<E> {
    /// **Bind a loopback port of the kernel's choosing and answer every
    /// connection to it** with `answer`, on a worker named `name` in `band`,
    /// one connection after another, until [`Server::stop`].
    ///
    /// The port listens from the moment this returns: a client that connects
    /// before the worker reaches `accept` waits in the backlog, it is never
    /// refused.
    ///
    /// # Errors
    ///
    /// When the port cannot be bound or the worker cannot be started.
    pub fn start(
        name: &'static str,
        band: crate::ThreadPriority,
        answer: impl FnMut(&mut TcpStream, &Stop) -> E + Send + 'static,
    ) -> io::Result<Self> {
        Self::start_on(TcpListener::bind("127.0.0.1:0")?, name, band, answer)
    }

    /// [`Server::start`] on a listener already bound.
    fn start_on(
        listener: TcpListener,
        name: &'static str,
        band: crate::ThreadPriority,
        mut answer: impl FnMut(&mut TcpStream, &Stop) -> E + Send + 'static,
    ) -> io::Result<Self> {
        let address = listener.local_addr()?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            stopping: Condvar::new(),
        });
        let stop = Stop(Arc::clone(&shared));
        let worker = crate::spawn_at_priority(name, band, move |_| {
            let mut record = Vec::new();
            loop {
                let (mut connection, peer) = match listener.accept() {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        let kind = error.kind();
                        record.push(Entry::AcceptFailed {
                            kind,
                            error: error.to_string(),
                        });
                        match kind {
                            io::ErrorKind::ConnectionReset
                            | io::ErrorKind::ConnectionAborted
                            | io::ErrorKind::Interrupted => continue,
                            _ => break,
                        }
                    }
                };
                {
                    let mut state = stop.0.state();
                    if state.stopped {
                        // The stop's own wake-up, or a client that came after
                        // the test was done: told apart by the wake-up's
                        // address, which the stop publishes once its connect
                        // returns.
                        let state = stop
                            .0
                            .stopping
                            .wait_while(state, |state| state.wake_up == WakeUp::Pending)
                            .unwrap_or_else(PoisonError::into_inner);
                        if state.wake_up == WakeUp::From(peer) {
                            break;
                        }
                        record.push(Entry::ArrivedAfterTheStop(peer));
                        if state.wake_up == WakeUp::Refused {
                            break;
                        }
                        continue;
                    }
                    state.serving = Some(
                        connection
                            .try_clone()
                            .expect("a second handle on the connection being served"),
                    );
                }
                record.push(Entry::Served(answer(&mut connection, &stop)));
                close_after_the_client(&mut connection);
                stop.0.state().serving = None;
            }
            record
        })?;
        Ok(Self {
            address,
            shared,
            worker: Some(worker),
        })
    }

    /// The address the server listens on.
    #[must_use]
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// The port the server listens on.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.address.port()
    }

    /// **Stop the server and answer what it recorded**, one entry per
    /// connection it met (and per failed `accept`), in order.
    ///
    /// # Panics
    ///
    /// When an `answer` panicked: the worker's panic is carried on (its own
    /// message is on the test's output already).
    pub fn stop(mut self) -> Vec<Entry<E>> {
        let worker = self.worker.take().expect("a server is stopped once");
        self.raise_the_stop();
        worker.join().expect("a loopback server's answer panicked")
    }
}

impl<E> Server<E> {
    /// The stop: the flag, the connection being served shut and its waiting
    /// call cancelled, a pause woken, and a worker in `accept` woken by a
    /// connection whose address it is told.
    fn raise_the_stop(&self) {
        {
            let mut state = self.shared.state();
            state.stopped = true;
            if let Some(serving) = state.serving.take() {
                // Every later read or write on the connection is refused...
                let _ = serving.shutdown(Shutdown::Both);
                // ...and one already waiting is ended.
                cancel_outstanding(&serving);
            }
        }
        self.shared.stopping.notify_all();
        // A worker blocked in `accept` wakes on this connection and ends on it.
        // A worker that has already ended (on a failed `accept`) has dropped
        // its listener, and this connect is refused: there is nobody to wake.
        let wake_up = match TcpStream::connect(self.address) {
            Ok(connection) => WakeUp::From(
                connection
                    .local_addr()
                    .expect("the address of a connection just made"),
            ),
            Err(_) => WakeUp::Refused,
        };
        self.shared.state().wake_up = wake_up;
        self.shared.stopping.notify_all();
    }
}

impl<E> Drop for Server<E> {
    /// **A server the test never stopped stops on its drop** — and one dropped
    /// while the test is unwinding raises the stop without waiting for its
    /// worker, so it neither hangs on it nor panics a second time over it.
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.raise_the_stop();
            if !std::thread::panicking() {
                worker.join().expect("a loopback server's answer panicked");
            }
        }
    }
}

/// **Close a connection whose answer is done without resetting its client**
/// (0.4.9 plan row 154): end the sending side, so the client reads the end of
/// the answer, and read until the client closes before letting go.
///
/// On Windows and Linux a socket closed while bytes it never read are
/// waiting sends a reset instead of an end (RFC 1122 §4.2.2.13), and a reset
/// discards whatever of the answer the client had not read yet — the bytes a
/// request carries past what its answer read (the stray CRLF after a body
/// that RFC 9112 §2.2 says some clients send) are enough. This is the
/// half-close of RFC 9112 §9.6.
///
/// The read ends when the client closes, or on the stop: the worker's
/// `serving` handle is still published, so [`Server::stop`]'s shutdown and
/// cancellation end a read already waiting here, and a read begun after them
/// is refused at once.
fn close_after_the_client(connection: &mut TcpStream) {
    use std::io::Read as _;
    let _ = connection.shutdown(Shutdown::Write);
    let mut unread = [0u8; 512];
    while matches!(connection.read(&mut unread), Ok(read) if read > 0) {}
}

/// **End a read or a write already waiting on `connection`.** On Windows a
/// `shutdown` refuses only the calls made after it: a `recv` the worker is
/// already blocked in stays blocked until the client closes (observed
/// 2026-10-31: `the_stop_ends_a_held_connection_and_a_pause` waited out the
/// suites' patience). `CancelIoEx` cancels the I/O outstanding on the socket
/// (`connection` is a duplicate of the worker's handle; the two name one
/// socket), and the blocked call returns `ERROR_OPERATION_ABORTED`.
#[cfg(windows)]
fn cancel_outstanding(connection: &TcpStream) {
    use std::os::windows::io::AsRawSocket as _;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::IO::CancelIoEx;
    // SAFETY: the socket is open for the whole call (`connection` owns it), and
    // `CancelIoEx` only asks for the cancellation of I/O on it.
    let _ = unsafe { CancelIoEx(HANDLE(connection.as_raw_socket() as *mut _), None) };
}

/// Elsewhere `shutdown` already ends a `recv` or a `send` that is waiting.
#[cfg(not(windows))]
fn cancel_outstanding(_connection: &TcpStream) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ThreadPriority;
    use crate::door_skipped::received;
    use std::io::{Read, Write};
    use std::sync::mpsc::channel;

    /// **`server` stopped, within the suites' patience** — a stop that waits
    /// for something nobody will do fails the test by name rather than
    /// hanging it.
    fn stopped<E: Send + 'static>(server: Server<E>, what: &str) -> Vec<Entry<E>> {
        let (said, heard) = channel();
        std::thread::spawn(move || {
            let _ = said.send(server.stop());
        });
        received(&heard, what)
    }

    /// The bytes of one connection, read until the client closes its side.
    fn everything(connection: &mut TcpStream) -> Vec<u8> {
        let mut bytes = Vec::new();
        let _ = connection.read_to_end(&mut bytes);
        bytes
    }

    /// Say `what` on a connection of its own, close the sending side, and
    /// answer what came back.
    fn say(address: SocketAddr, what: &[u8]) -> io::Result<Vec<u8>> {
        let mut connection = TcpStream::connect(address)?;
        connection.write_all(what)?;
        connection.shutdown(Shutdown::Write)?;
        let mut answer = Vec::new();
        connection.read_to_end(&mut answer)?;
        Ok(answer)
    }

    /// RED (row 127) — **every connection is answered, however many come, and
    /// the record holds each one in order.**
    ///
    /// MUTATION: end the worker's loop after its first answer (the one-shot
    /// server this replaces) and the second `say` is refused
    /// (`ConnectionRefused`, os error 10061 on Windows).
    #[test]
    fn every_connection_is_answered_until_the_stop() {
        let server = Server::start(
            "bt-loopback-test",
            ThreadPriority::Normal,
            |connection, _| {
                let heard = everything(connection);
                let _ = connection.write_all(&heard);
                heard
            },
        )
        .expect("a loopback server");
        for word in [&b"one"[..], b"two", b"three"] {
            let echoed = say(server.address(), word).expect("the server answers every connection");
            assert_eq!(echoed, word);
        }
        assert_eq!(
            stopped(server, "the stop of a server that answered three"),
            vec![
                Entry::Served(b"one".to_vec()),
                Entry::Served(b"two".to_vec()),
                Entry::Served(b"three".to_vec())
            ],
            "one record per connection, and the stop's own wake-up is not one"
        );
    }

    /// RED (row 127) — **the stop ends a connection the client is holding
    /// open without a word**, and a pause the script is in: the stop returns
    /// within the suites' patience, and the record says what each was.
    ///
    /// MUTATION: leave out `cancel_outstanding` in `raise_the_stop` and, on
    /// Windows, the first stop waits on a read nobody ends ("nothing within
    /// the door suites' patience"; `shutdown` alone does not end it); wait in
    /// `pause` with a sleep and the second does.
    #[test]
    fn the_stop_ends_a_held_connection_and_a_pause() {
        let (reading, read) = channel();
        let server = Server::start(
            "bt-loopback-test",
            ThreadPriority::Normal,
            move |connection, stop| {
                let _ = reading.send(());
                let heard = everything(connection);
                (heard.len(), stop.requested())
            },
        )
        .expect("a loopback server");
        let _held = TcpStream::connect(server.address()).expect("a connection");
        received(&read, "the server reading the held connection");
        assert_eq!(
            stopped(server, "the stop of a server reading a held connection"),
            vec![Entry::Served((0, true))]
        );

        let (pausing, paused) = channel();
        let server = Server::start(
            "bt-loopback-test",
            ThreadPriority::Normal,
            move |_, stop| {
                let _ = pausing.send(());
                stop.pause(Duration::from_secs(3_600))
            },
        )
        .expect("a loopback server");
        let _client = TcpStream::connect(server.address()).expect("a connection");
        received(&paused, "the server pausing");
        assert_eq!(
            stopped(server, "the stop of a paused server"),
            vec![Entry::Served(true)],
            "the pause ended on the stop"
        );
    }

    /// RED (row 127) — **a server nobody connects to stops**: the stop's own
    /// connection wakes the `accept`, and the record is empty.
    ///
    /// MUTATION: drop the wake-up connect from `raise_the_stop` and the stop
    /// waits in `accept` forever ("nothing within the door suites'
    /// patience").
    #[test]
    fn a_server_nobody_asked_stops_with_an_empty_record() {
        let server = Server::start("bt-loopback-test", ThreadPriority::Normal, |_, _| ())
            .expect("a loopback server");
        assert!(stopped(server, "the stop of an idle server").is_empty());
    }

    /// RED (row 127 review F1) — **a connection that arrives after the stop
    /// is raised is recorded as arrived and never served**, and the stop's
    /// own wake-up still is not: a client the worker had not reached when the
    /// test was done is evidence too.
    ///
    /// The second client connects while the first is being served, so it
    /// waits in the backlog ahead of the stop's wake-up, and the worker
    /// accepts it with the flag already up.
    ///
    /// MUTATION: end the worker on any connection accepted after the flag
    /// (the shape this replaces) and the record holds the served connection
    /// only.
    #[test]
    fn a_connection_after_the_stop_is_recorded_as_never_served() {
        let (reading, read) = channel();
        let server = Server::start(
            "bt-loopback-test",
            ThreadPriority::Normal,
            move |connection, _| {
                let _ = reading.send(());
                everything(connection).len()
            },
        )
        .expect("a loopback server");
        let _held = TcpStream::connect(server.address()).expect("a connection");
        received(&read, "the server reading the held connection");
        let late = TcpStream::connect(server.address()).expect("a second connection");
        let late = late.local_addr().expect("the late client's address");
        assert_eq!(
            stopped(server, "the stop of a server with a client in its backlog"),
            vec![Entry::Served(0), Entry::ArrivedAfterTheStop(late)]
        );
    }

    /// RED (row 154) — **a client that sends more than its answer reads gets
    /// the answer and a clean close, never a reset.** The server half-closes
    /// after its answer and reads until the client closes; on Windows and
    /// Linux a socket closed with bytes still unread sends a reset instead of
    /// an end (RFC 1122 §4.2.2.13), and a reset discards the answer the
    /// client had not read yet (a BSD stack, macOS's, discards the unread
    /// bytes and ends cleanly, so there this test is green either way).
    /// The bytes past the request here are the stray CRLF after a body that
    /// RFC 9112 §2.2 says some clients send.
    ///
    /// MUTATION: drop the half-close and the read after it from the worker
    /// (the server closes as soon as its answer returns) and the client's
    /// read fails with `ConnectionReset` (os error 10054 on Windows).
    #[test]
    fn a_client_that_says_more_than_is_read_is_answered_without_a_reset() {
        let server = Server::start(
            "bt-loopback-test",
            ThreadPriority::Normal,
            |connection, _| {
                let mut heard = [0u8; 3];
                let read = connection.read_exact(&mut heard).is_ok();
                let _ = connection.write_all(b"ok");
                read.then_some(heard)
            },
        )
        .expect("a loopback server");
        let mut connection = TcpStream::connect(server.address()).expect("a connection");
        connection
            .write_all(b"one\r\n")
            .expect("a request and a stray CRLF");
        let mut answer = Vec::new();
        let read = connection.read_to_end(&mut answer);
        drop(connection);
        assert!(read.is_ok(), "the answer ends cleanly: {read:?}");
        assert_eq!(answer, b"ok");
        assert_eq!(
            stopped(server, "the stop of a server that answered a chatty client"),
            vec![Entry::Served(Some(*b"one"))]
        );
    }

    /// RED (row 127 review N3) — **an `accept` that fails for the listener's
    /// own reason ends the worker, the failure recorded** — it never spins
    /// at the worker's band until the stop. A listener left non-blocking is
    /// one whose every `accept` fails (`WouldBlock`).
    ///
    /// MUTATION: go back to `accept` again on every failure and the worker
    /// never ends on its own ("nothing within the door suites' patience").
    #[test]
    fn a_failing_accept_ends_the_worker_with_the_failure_recorded() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        listener
            .set_nonblocking(true)
            .expect("a non-blocking listener");
        let mut server = Server::start_on(
            listener,
            "bt-loopback-test",
            ThreadPriority::Normal,
            |_, _| (),
        )
        .expect("a loopback server");
        let worker = server.worker.take().expect("the worker");
        let (said, heard) = channel();
        std::thread::spawn(move || {
            let _ = said.send(worker.join().expect("the worker ends without a panic"));
        });
        let record = received(&heard, "the worker ending on its own");
        assert!(
            matches!(
                record.as_slice(),
                [Entry::AcceptFailed {
                    kind: io::ErrorKind::WouldBlock,
                    ..
                }]
            ),
            "{record:?}"
        );
    }
}
