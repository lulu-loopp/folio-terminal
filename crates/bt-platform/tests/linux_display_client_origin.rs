#![cfg(target_os = "linux")]

use bt_platform::linux_display::{self, LinuxDisplayAnswer, LinuxDisplayQuery, LinuxDisplayReady};
use bt_platform::linux_window::Backend;
use bt_platform::{NativeWindow, WindowRect};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, Window, WindowClass};
use x11rb::rust_connection::RustConnection;

const CHILD_ENV: &str = "FOLIO_PR20_CLIENT_ORIGIN_CHILD";

struct Xvfb(Child);

impl Drop for Xvfb {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn start_xvfb() -> (Xvfb, String) {
    let mut server = Command::new("Xvfb")
        .args([
            "-displayfd",
            "1",
            "-screen",
            "0",
            "1024x768x24",
            "-nolisten",
            "tcp",
            "-ac",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start a private Xvfb for the client-origin test");
    let mut display_number = String::new();
    BufReader::new(
        server
            .stdout
            .take()
            .expect("capture the private display number"),
    )
    .read_line(&mut display_number)
    .expect("read Xvfb's display-ready signal");
    let display_number = display_number.trim();
    assert!(
        !display_number.is_empty(),
        "Xvfb exited before display startup"
    );
    (Xvfb(server), format!(":{display_number}"))
}

fn reparented_client(
    connection: &RustConnection,
    root: Window,
    visual: u32,
    frame_origin: (i16, i16),
    client_offset: (i16, i16),
) -> Window {
    let frame = connection.generate_id().expect("allocate the frame id");
    connection
        .create_window(
            0,
            frame,
            root,
            frame_origin.0,
            frame_origin.1,
            300,
            220,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &CreateWindowAux::new(),
        )
        .expect("send frame creation")
        .check()
        .expect("create the reparenting frame");

    let client = connection.generate_id().expect("allocate the client id");
    connection
        .create_window(
            0,
            client,
            root,
            500,
            500,
            240,
            160,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &CreateWindowAux::new(),
        )
        .expect("send client creation")
        .check()
        .expect("create the client window");
    connection
        .reparent_window(client, frame, client_offset.0, client_offset.1)
        .expect("send the reparent request")
        .check()
        .expect("reparent the client beneath its frame");
    connection
        .map_window(frame)
        .expect("send frame mapping")
        .check()
        .expect("map the frame");
    connection
        .map_window(client)
        .expect("send client mapping")
        .check()
        .expect("map the client");
    connection.flush().expect("flush the X11 fixture");
    let tree = connection
        .query_tree(client)
        .expect("send the frame-parent query")
        .reply()
        .expect("confirm the client is reparented");
    assert_eq!(tree.parent, frame, "the frame owns this client");
    client
}

fn client_window_from_env(name: &str) -> NativeWindow {
    let raw = std::env::var(name)
        .unwrap_or_else(|error| panic!("read the fixture window id {name}: {error}"));
    let id = raw
        .parse::<u32>()
        .unwrap_or_else(|error| panic!("parse the fixture window id {name}: {error}"));
    NativeWindow::from_x11(std::num::NonZeroU32::new(id).expect("X11 window id is nonzero"))
}

fn read_facts(
    wake: &mpsc::Receiver<LinuxDisplayReady>,
    window: NativeWindow,
    request_id: u64,
    expected_rect: WindowRect,
    expected_client_origin: (i32, i32),
) {
    let request =
        linux_display::request_display(91, request_id, LinuxDisplayQuery::WindowRect { window })
            .expect("enqueue the native window-facts query");
    let ready = wake.recv().expect("receive the worker's completion event");
    assert_eq!(
        request.ready(),
        ready,
        "the worker wakes the matching request"
    );
    let LinuxDisplayAnswer::WindowRect(Ok(facts)) = request
        .try_take()
        .expect("the answer is ready with its completion event")
    else {
        panic!("the display worker did not return window facts");
    };
    assert_eq!(
        facts.rect, expected_rect,
        "the frame remains the outer rectangle"
    );
    assert_eq!(
        facts.client_origin,
        Some(expected_client_origin),
        "the worker translates the reparented client origin to the root"
    );
    assert_ne!(
        (facts.rect.left, facts.rect.top),
        expected_client_origin,
        "the inner client origin differs from the outer frame origin"
    );
}

#[test]
fn client_origin_child() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    linux_display::install_backend(Backend::X11).expect("select the private X11 backend");
    let (wake_tx, wake_rx) = mpsc::channel();
    linux_display::install_display_wake(move |ready| {
        let _ = wake_tx.send(ready);
    })
    .expect("install the private display wake");
    read_facts(
        &wake_rx,
        client_window_from_env("FOLIO_PR20_POSITIVE_CLIENT"),
        1,
        WindowRect {
            left: 120,
            top: 90,
            right: 420,
            bottom: 310,
        },
        (137, 113),
    );
    read_facts(
        &wake_rx,
        client_window_from_env("FOLIO_PR20_NEGATIVE_CLIENT"),
        2,
        WindowRect {
            left: -60,
            top: -40,
            right: 240,
            bottom: 180,
        },
        (-53, -31),
    );
    linux_display::stop_display_service();
}

#[test]
fn reparented_x11_clients_report_their_client_origins_through_the_worker() {
    let (server, display) = start_xvfb();
    let (connection, screen) = x11rb::connect(Some(&display))
        .expect("connect to the private Xvfb, never the user's display");
    let root = connection.setup().roots[screen].root;
    let visual = connection.setup().roots[screen].root_visual;
    let positive = reparented_client(&connection, root, visual, (120, 90), (17, 23));
    let negative = reparented_client(&connection, root, visual, (-60, -40), (7, 9));
    let child = Command::new(std::env::current_exe().expect("locate the test binary"))
        .args([
            "--exact",
            "client_origin_child",
            "--test-threads=1",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        .env("DISPLAY", &display)
        .env("XAUTHORITY", "/dev/null")
        .env("FOLIO_PR20_POSITIVE_CLIENT", positive.to_string())
        .env("FOLIO_PR20_NEGATIVE_CLIENT", negative.to_string())
        .output()
        .expect("start the private X11 worker client");
    assert!(
        child.status.success(),
        "the X11 worker returned incorrect client facts:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    drop(connection);
    drop(server);
}
