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
//!   and what each one was.
//! - **Stopping is a signal, never a count or a time.** `stop` raises the flag,
//!   shuts the connection being served (if any) and cancels a read or a write
//!   already waiting on it, so the answer ends, wakes a [`Stop::pause`], and makes one connection of its own so a
//!   worker blocked in `accept` sees the flag. Then it joins the worker. Every
//!   wait it ends is one it causes.
//!
//! Tests only: this crate's own (`cfg(test)`), and those of a crate that turns
//! on `trust-harness` (the trust harness's time-stamp authority stands on it).

use std::io;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

/// **A loopback server**, answering every connection with its `answer` and
/// recording what each answer returned, until [`Server::stop`].
pub struct Server<E> {
    address: SocketAddr,
    shared: Arc<Shared>,
    worker: JoinHandle<Vec<E>>,
}

/// What the test's side and the worker share.
struct Shared {
    state: Mutex<State>,
    /// Signalled when the stop is raised, for [`Stop::pause`].
    stopping: Condvar,
}

#[derive(Default)]
struct State {
    stopped: bool,
    /// A handle on the connection the worker is serving, so the stop can shut
    /// it and end a read or a write the client is not ending.
    serving: Option<TcpStream>,
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
        mut answer: impl FnMut(&mut TcpStream, &Stop) -> E + Send + 'static,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            stopping: Condvar::new(),
        });
        let stop = Stop(Arc::clone(&shared));
        let worker = crate::spawn_at_priority(name, band, move |_| {
            let mut record = Vec::new();
            loop {
                // A failed `accept` is the client's (a connection reset while
                // it waited in the backlog): the listener is still there.
                let Ok((mut connection, _)) = listener.accept() else {
                    if stop.requested() {
                        break;
                    }
                    continue;
                };
                {
                    let mut state = stop.0.state();
                    if state.stopped {
                        // The stop's own wake-up, or a client that came after
                        // the test was done: neither is a request of the test.
                        break;
                    }
                    state.serving = connection.try_clone().ok();
                }
                record.push(answer(&mut connection, &stop));
                stop.0.state().serving = None;
            }
            record
        })?;
        Ok(Self {
            address,
            shared,
            worker,
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
    /// connection it served, in order.
    ///
    /// # Panics
    ///
    /// With the worker's own panic, when an `answer` panicked.
    pub fn stop(self) -> Vec<E> {
        self.finish()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    }

    /// [`Server::stop`], handing a panicked worker's panic back rather than
    /// raising it — for a stop made while a test is already unwinding.
    ///
    /// # Errors
    ///
    /// The worker's panic, when an `answer` panicked.
    pub fn finish(self) -> std::thread::Result<Vec<E>> {
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
        // A worker blocked in `accept` wakes on this connection and ends on the
        // flag. A worker that has already ended (on a late client's connection
        // after the flag) has dropped its listener, and this connect is
        // refused: there is nobody left to wake.
        let _ = TcpStream::connect(self.address);
        self.worker.join()
    }
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
            server.stop(),
            vec![b"one".to_vec(), b"two".to_vec(), b"three".to_vec()],
            "one record per connection, and the stop's own wake-up is not one"
        );
    }

    /// RED (row 127) — **the stop ends a connection the client is holding
    /// open without a word**, and a pause the script is in: the stop returns
    /// within the suites' patience, and the record says what each was.
    ///
    /// MUTATION: leave out `cancel_outstanding` in `finish` and, on Windows,
    /// the first stop waits on a read nobody ends ("nothing within the door
    /// suites' patience"; `shutdown` alone does not end it); wait in `pause`
    /// with a sleep and the second does.
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
        let (stopped, stops) = channel();
        std::thread::spawn(move || {
            let _ = stopped.send(server.stop());
        });
        assert_eq!(
            received(&stops, "the stop of a server reading a held connection"),
            vec![(0, true)]
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
        let (stopped, stops) = channel();
        std::thread::spawn(move || {
            let _ = stopped.send(server.stop());
        });
        assert_eq!(
            received(&stops, "the stop of a paused server"),
            vec![true],
            "the pause ended on the stop"
        );
    }

    /// RED (row 127) — **a server nobody connects to stops**: the stop's own
    /// connection wakes the `accept`, and the record is empty.
    ///
    /// MUTATION: drop the wake-up connect from `finish` and the stop waits in
    /// `accept` forever ("nothing within the door suites' patience").
    #[test]
    fn a_server_nobody_asked_stops_with_an_empty_record() {
        let server = Server::start("bt-loopback-test", ThreadPriority::Normal, |_, _| ())
            .expect("a loopback server");
        let (stopped, stops) = channel();
        std::thread::spawn(move || {
            let _ = stopped.send(server.stop());
        });
        assert!(received(&stops, "the stop of an idle server").is_empty());
    }
}
