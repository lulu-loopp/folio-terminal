// MODIFIED BY THE FOLIO CONTRIBUTORS: control compositor reply flushing in owner tests; see CHANGES-FOLIO.md.
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;

use os_pipe::{PipeReader, PipeWriter};
use rustix::buffer::spare_capacity;
use rustix::event::epoll;
use wayland_backend::server::ClientData;
use wayland_server::{Display, ListeningSocket};

mod copy;
mod paste;
mod state;
mod utils;
mod watch;

pub struct TestServer<S: 'static> {
    pub display: Display<S>,
    pub socket: ListeningSocket,
    pub epoll: OwnedFd,
}

pub struct FlushGate {
    gate: Arc<AtomicBool>,
    wake: Option<PipeWriter>,
    held: Receiver<()>,
}

impl FlushGate {
    pub fn wait_until_held(&self) {
        self.held
            .recv()
            .expect("the compositor reached its held-flush point");
    }

    pub fn release(&mut self) {
        self.gate.store(false, std::sync::atomic::Ordering::Release);
        if let Some(mut wake) = self.wake.take() {
            wake.write_all(&[1])
                .expect("wake the compositor after releasing the flush gate");
        }
    }
}

struct ClientCounter(AtomicU8);

impl ClientData for ClientCounter {
    fn disconnected(
        &self,
        _client_id: wayland_backend::server::ClientId,
        _reason: wayland_backend::server::DisconnectReason,
    ) {
        self.0.fetch_sub(1, SeqCst);
    }
}

impl<S: Send + 'static> TestServer<S> {
    pub fn new() -> Self {
        let mut display = Display::new().unwrap();
        let socket = ListeningSocket::bind_auto("wl-clipboard-rs-test", 0..).unwrap();

        let epoll = epoll::create(epoll::CreateFlags::CLOEXEC).unwrap();

        epoll::add(
            &epoll,
            &socket,
            epoll::EventData::new_u64(0),
            epoll::EventFlags::IN,
        )
        .unwrap();
        epoll::add(
            &epoll,
            display.backend().poll_fd(),
            epoll::EventData::new_u64(1),
            epoll::EventFlags::IN,
        )
        .unwrap();

        TestServer {
            display,
            socket,
            epoll,
        }
    }

    pub fn socket_name(&self) -> &OsStr {
        self.socket.socket_name().unwrap()
    }

    pub fn run(self, mut state: S) {
        thread::spawn(move || self.run_internal(&mut state, None, None, None));
    }

    pub fn run_with_flush_gate(self, mut state: S, gate: Arc<AtomicBool>) -> FlushGate {
        let (wake_reader, wake_writer) = os_pipe::pipe().unwrap();
        epoll::add(
            &self.epoll,
            &wake_reader,
            epoll::EventData::new_u64(2),
            epoll::EventFlags::IN,
        )
        .unwrap();
        let (held_tx, held_rx) = sync_channel(1);
        let server_gate = Arc::clone(&gate);
        thread::spawn(move || {
            self.run_internal(
                &mut state,
                Some(server_gate),
                Some(wake_reader),
                Some(held_tx),
            )
        });
        FlushGate {
            gate,
            wake: Some(wake_writer),
            held: held_rx,
        }
    }

    pub fn run_mutex(self, state: Arc<Mutex<S>>) {
        thread::spawn(move || {
            let mut state = state.lock().unwrap();
            self.run_internal(&mut *state, None, None, None);
        });
    }

    fn run_internal(
        mut self,
        state: &mut S,
        flush_gate: Option<Arc<AtomicBool>>,
        mut flush_wake: Option<PipeReader>,
        mut flush_held: Option<SyncSender<()>>,
    ) {
        let mut waiting_for_first_client = true;
        let client_counter = Arc::new(ClientCounter(AtomicU8::new(0)));

        while client_counter.0.load(SeqCst) > 0 || waiting_for_first_client {
            // Wait for requests from the client.
            let mut events = Vec::with_capacity(2);
            epoll::wait(&self.epoll, spare_capacity(&mut events), None).unwrap();

            for event in events.drain(..) {
                match event.data.u64() {
                    0 => {
                        // Try to accept a new client.
                        if let Some(stream) = self.socket.accept().unwrap() {
                            waiting_for_first_client = false;
                            client_counter.0.fetch_add(1, SeqCst);
                            self.display
                                .handle()
                                .insert_client(stream, client_counter.clone())
                                .unwrap();
                        }
                    }
                    1 => {
                        // Try to dispatch client messages.
                        self.display.dispatch_clients(state).unwrap();
                        if flush_gate
                            .as_ref()
                            .is_some_and(|gate| gate.load(std::sync::atomic::Ordering::Acquire))
                        {
                            if let Some(held) = flush_held.take() {
                                let _ = held.send(());
                            }
                        } else {
                            self.display.flush_clients().unwrap();
                        }
                    }
                    2 => {
                        let mut byte = [0];
                        if let Some(mut reader) = flush_wake.take() {
                            let _ = reader.read(&mut byte);
                            epoll::delete(&self.epoll, &reader).unwrap();
                            drop(reader);
                        }
                        if !flush_gate
                            .as_ref()
                            .is_some_and(|gate| gate.load(std::sync::atomic::Ordering::Acquire))
                        {
                            self.display.flush_clients().unwrap();
                        }
                    }
                    x => panic!("unexpected epoll event: {x}"),
                }
            }
        }
    }
}

// https://github.com/Smithay/wayland-rs/blob/90a9ad1f8f1fdef72e96d3c48bdb76b53a7722ff/wayland-tests/tests/helpers/mod.rs
#[macro_export]
macro_rules! server_ignore_impl {
    ($handler:ty => [$($iface:ty),*]) => {
        $(
            impl wayland_server::Dispatch<$iface, ()> for $handler {
                fn request(
                    _: &mut Self,
                    _: &wayland_server::Client,
                    _: &$iface,
                    _: <$iface as wayland_server::Resource>::Request,
                    _: &(),
                    _: &wayland_server::DisplayHandle,
                    _: &mut wayland_server::DataInit<'_, Self>,
                ) {
                }
            }
        )*
    }
}

#[macro_export]
macro_rules! server_ignore_global_impl {
    ($handler:ty => [$($iface:ty),*]) => {
        $(
            impl wayland_server::GlobalDispatch<$iface, ()> for $handler {
                fn bind(
                    _: &mut Self,
                    _: &wayland_server::DisplayHandle,
                    _: &wayland_server::Client,
                    new_id: wayland_server::New<$iface>,
                    _: &(),
                    data_init: &mut wayland_server::DataInit<'_, Self>,
                ) {
                    data_init.init(new_id, ());
                }
            }
        )*
    }
}
