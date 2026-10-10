// MODIFIED BY THE FOLIO CONTRIBUTORS: tests retained offers, bounded content, cancellation, and
// deadline behavior for `OfferSession`. See CHANGES-FOLIO.md.

use std::collections::HashMap;
use std::io::Read;

use proptest::prelude::*;
#[cfg(target_os = "linux")]
use wayland_client::ConnectError;
use wayland_protocols_wlr::data_control::v1::server::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1;

#[cfg(target_os = "linux")]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::process::{Command, Output};
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::paste::*;
use crate::tests::state::*;
use crate::tests::TestServer;

#[cfg(target_os = "linux")]
const CONNECT_CHILD_MODE: &str = "WL_CLIPBOARD_RS_CONNECT_CHILD";

#[cfg(target_os = "linux")]
struct PrivateWaylandRuntime {
    directory: PathBuf,
    socket_path: PathBuf,
    _listener: UnixListener,
    queued_clients: Vec<UnixStream>,
}

#[cfg(target_os = "linux")]
impl PrivateWaylandRuntime {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;

        static NEXT_RUNTIME: AtomicUsize = AtomicUsize::new(1);
        let directory = std::env::temp_dir().join(format!(
            "wl-clipboard-connect-{}-{}",
            std::process::id(),
            NEXT_RUNTIME.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket_path = directory.join("wayland-test");
        let listener = UnixListener::bind(&socket_path).unwrap();
        rustix::net::listen(&listener, 1).unwrap();

        Self {
            directory,
            socket_path,
            _listener: listener,
            queued_clients: Vec::new(),
        }
    }

    fn fill_backlog(&mut self) {
        self.queued_clients
            .push(UnixStream::connect(&self.socket_path).unwrap());
        self.queued_clients
            .push(UnixStream::connect(&self.socket_path).unwrap());
        match crate::common::connect_unix_stream_nonblocking(&self.socket_path) {
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("full-listener probe failed with {error}"),
            Ok(_) => panic!("listen(1) accepted a third queued connection"),
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for PrivateWaylandRuntime {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[cfg(target_os = "linux")]
fn run_connect_test_child(test_name: &str, configure: impl FnOnce(&mut Command)) -> Output {
    let mut command = Command::new("timeout");
    command
        .args(["--signal=KILL", "5s"])
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
        .env(CONNECT_CHILD_MODE, "1")
        .env_remove("WAYLAND_SOCKET");
    configure(&mut command);
    command.output().unwrap()
}

#[cfg(target_os = "linux")]
fn assert_connect_child_succeeded(output: Output) {
    assert!(
        output.status.success(),
        "controlled clipboard child did not finish successfully: status={:?}; stdout={}; stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[cfg(target_os = "linux")]
#[test]
fn offer_session_returns_when_default_wayland_backlog_is_full() {
    if std::env::var_os(CONNECT_CHILD_MODE).is_some() {
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let result = OfferSession::open_for_mimes(
            ClipboardType::Regular,
            Seat::Unspecified,
            &["text/plain"],
            std::time::Instant::now() + std::time::Duration::from_secs(4),
            &cancelled,
        );
        assert!(matches!(
            result,
            Err(Error::WaylandConnection(ConnectError::NoCompositor))
        ));
        return;
    }

    let mut runtime = PrivateWaylandRuntime::new();
    runtime.fill_backlog();
    let output = run_connect_test_child(
        "tests::paste::offer_session_returns_when_default_wayland_backlog_is_full",
        |command| {
            command
                .env("WAYLAND_DISPLAY", "wayland-test")
                .env("XDG_RUNTIME_DIR", &runtime.directory);
        },
    );
    assert_connect_child_succeeded(output);
}

#[test]
fn automatic_selection_skips_password_manager_hint_when_possible() {
    let hint = "x-kde-passwordManagerHint";
    let available = vec![hint.to_owned(), "image/png".to_owned()];
    assert_eq!(
        select_mime_type(available.clone(), MimeType::Any),
        Some("image/png".to_owned())
    );
    assert_eq!(
        select_mime_type(available, MimeType::Specific(hint)),
        Some(hint.to_owned())
    );
    assert_eq!(
        select_mime_type(vec![hint.to_owned()], MimeType::Any),
        Some(hint.to_owned())
    );
}

#[test]
fn get_mime_types_test() {
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let state = State {
        seats: HashMap::from([(
            "seat0".into(),
            SeatInfo {
                offer: Some(OfferInfo::Buffered {
                    data: vec![
                        ("first".into(), vec![]),
                        ("second".into(), vec![]),
                        ("third".into(), vec![]),
                    ],
                }),
                ..Default::default()
            },
        )]),
        ..Default::default()
    };
    state.create_seats(&server);

    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let mime_types =
        get_mime_types_internal(ClipboardType::Regular, Seat::Unspecified, Some(socket_name))
            .unwrap();

    let expected = Vec::from(["first", "second", "third"].map(String::from));
    assert_eq!(mime_types, expected);
}

#[test]
fn get_mime_types_no_data_control() {
    let server = TestServer::new();

    let state = State {
        seats: HashMap::from([(
            "seat0".into(),
            SeatInfo {
                ..Default::default()
            },
        )]),
        ..Default::default()
    };
    state.create_seats(&server);

    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let result =
        get_mime_types_internal(ClipboardType::Regular, Seat::Unspecified, Some(socket_name));
    assert!(matches!(
        result,
        Err(Error::MissingProtocol {
            name: "ext-data-control, or wlr-data-control",
            version: 1
        })
    ));
}

#[test]
fn get_mime_types_no_data_control_2() {
    let server = TestServer::new();

    let state = State {
        seats: HashMap::from([(
            "seat0".into(),
            SeatInfo {
                ..Default::default()
            },
        )]),
        ..Default::default()
    };
    state.create_seats(&server);

    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let result =
        get_mime_types_internal(ClipboardType::Primary, Seat::Unspecified, Some(socket_name));
    assert!(matches!(
        result,
        Err(Error::MissingProtocol {
            name: "ext-data-control, or wlr-data-control",
            version: 2
        })
    ));
}

#[test]
fn get_mime_types_no_seats() {
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let state = State {
        ..Default::default()
    };
    state.create_seats(&server);

    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let result =
        get_mime_types_internal(ClipboardType::Primary, Seat::Unspecified, Some(socket_name));
    assert!(matches!(result, Err(Error::NoSeats)));
}

#[test]
fn get_mime_types_empty_clipboard() {
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let state = State {
        seats: HashMap::from([(
            "seat0".into(),
            SeatInfo {
                ..Default::default()
            },
        )]),
        ..Default::default()
    };
    state.create_seats(&server);

    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let result =
        get_mime_types_internal(ClipboardType::Primary, Seat::Unspecified, Some(socket_name));
    assert!(matches!(result, Err(Error::ClipboardEmpty)));
}

#[test]
fn get_mime_types_specific_seat() {
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let state = State {
        seats: HashMap::from([
            (
                "seat0".into(),
                SeatInfo {
                    ..Default::default()
                },
            ),
            (
                "yay".into(),
                SeatInfo {
                    offer: Some(OfferInfo::Buffered {
                        data: vec![
                            ("first".into(), vec![]),
                            ("second".into(), vec![]),
                            ("third".into(), vec![]),
                        ],
                    }),
                    ..Default::default()
                },
            ),
        ]),
        ..Default::default()
    };
    state.create_seats(&server);

    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let mime_types = get_mime_types_internal(
        ClipboardType::Regular,
        Seat::Specific("yay"),
        Some(socket_name),
    )
    .unwrap();

    let expected = Vec::from(["first", "second", "third"].map(String::from));
    assert_eq!(mime_types, expected);
}

#[test]
fn get_mime_types_primary() {
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let state = State {
        seats: HashMap::from([(
            "seat0".into(),
            SeatInfo {
                primary_offer: Some(OfferInfo::Buffered {
                    data: vec![
                        ("first".into(), vec![]),
                        ("second".into(), vec![]),
                        ("third".into(), vec![]),
                    ],
                }),
                ..Default::default()
            },
        )]),
        ..Default::default()
    };
    state.create_seats(&server);

    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let mime_types =
        get_mime_types_internal(ClipboardType::Primary, Seat::Unspecified, Some(socket_name))
            .unwrap();

    let expected = Vec::from(["first", "second", "third"].map(String::from));
    assert_eq!(mime_types, expected);
}

#[test]
fn get_contents_test() {
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let state = State {
        seats: HashMap::from([(
            "seat0".into(),
            SeatInfo {
                offer: Some(OfferInfo::Buffered {
                    data: vec![("application/octet-stream".into(), vec![1, 3, 3, 7])],
                }),
                ..Default::default()
            },
        )]),
        ..Default::default()
    };
    state.create_seats(&server);

    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let (mut read, mime_type) = get_contents_internal(
        ClipboardType::Regular,
        Seat::Unspecified,
        MimeType::Any,
        Some(socket_name),
    )
    .unwrap();

    assert_eq!(mime_type, "application/octet-stream");

    let mut contents = vec![];
    read.read_to_end(&mut contents).unwrap();
    assert_eq!(contents, [1, 3, 3, 7]);
}

#[test]
fn offer_session_keeps_the_read_start_offer_across_mime_fallback() {
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::channel;
    use std::time::{Duration, Instant};

    use crate::copy::{copy_internal, MimeSource, Options, Source};

    const TEXT_MIME: &str = "text/plain;charset=utf-8";
    const IMAGE_MIME: &str = "image/png";
    let old_text = b"original clipboard text".to_vec();
    let old_png = [0x89, b'P', b'N', b'G'];

    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());
    let (updated, update_seen) = channel();
    let state = State {
        seats: HashMap::from([(
            "seat0".into(),
            SeatInfo {
                offer: Some(OfferInfo::Buffered {
                    data: vec![
                        (TEXT_MIME.into(), old_text.clone()),
                        (IMAGE_MIME.into(), old_png.to_vec()),
                        ("text/html".into(), b"unsupported candidate".to_vec()),
                    ],
                }),
                ..Default::default()
            },
        )]),
        selection_updated_sender: Some(updated),
        ..Default::default()
    };
    state.create_seats(&server);
    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let cancelled = AtomicBool::new(false);
    let mut session = OfferSession::open_for_mimes_internal(
        ClipboardType::Regular,
        Seat::Unspecified,
        &[TEXT_MIME, IMAGE_MIME],
        Instant::now() + Duration::from_secs(3),
        &cancelled,
        Some(socket_name.clone()),
    )
    .unwrap();
    assert_eq!(
        session.mime_types(),
        &[TEXT_MIME.to_owned(), IMAGE_MIME.to_owned()]
    );

    copy_internal(
        Options::new(),
        vec![
            MimeSource {
                source: Source::Bytes(b"replacement text"[..].into()),
                mime_type: crate::copy::MimeType::Specific(TEXT_MIME.into()),
            },
            MimeSource {
                source: Source::Bytes([0x89, b'P', b'N', b'G'][..].into()),
                mime_type: crate::copy::MimeType::Specific(IMAGE_MIME.into()),
            },
        ],
        Some(socket_name),
    )
    .unwrap();
    update_seen.recv_timeout(Duration::from_secs(2)).unwrap();

    let (text, returned_text_mime) = session.read_contents(TEXT_MIME, 1024).unwrap();
    let (png, returned_png_mime) = session.read_contents(IMAGE_MIME, 1024).unwrap();
    assert_eq!(returned_text_mime, TEXT_MIME);
    assert_eq!(returned_png_mime, IMAGE_MIME);
    assert_eq!(text, old_text);
    assert_eq!(png, old_png);
    assert!(matches!(
        session.read_contents(TEXT_MIME, 2),
        Err(Error::ContentTooLarge)
    ));
}

#[test]
fn offer_session_checks_cancel_and_absolute_deadline_before_connecting() {
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    let cancelled = AtomicBool::new(true);
    let cancelled_result = OfferSession::open_for_mimes(
        ClipboardType::Regular,
        Seat::Unspecified,
        &["text/plain"],
        Instant::now() + Duration::from_secs(1),
        &cancelled,
    );
    assert!(matches!(cancelled_result, Err(Error::Cancelled)));

    let not_cancelled = AtomicBool::new(false);
    let expired_result = OfferSession::open_for_mimes(
        ClipboardType::Regular,
        Seat::Unspecified,
        &["text/plain"],
        Instant::now() - Duration::from_millis(1),
        &not_cancelled,
    );
    assert!(matches!(expired_result, Err(Error::DeadlineExceeded)));
}

#[test]
fn offer_session_cancels_a_stalled_offer_transfer_without_sleeping() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};
    use std::time::{Duration, Instant};

    const TEXT_MIME: &str = "text/plain;charset=utf-8";
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());
    let state = State {
        seats: HashMap::from([(
            "seat0".into(),
            SeatInfo {
                offer: Some(OfferInfo::Stalled {
                    data: vec![(TEXT_MIME.into(), b"released after cancel".to_vec())],
                    entered: Arc::clone(&entered),
                    release: Arc::clone(&release),
                }),
                ..Default::default()
            },
        )]),
        ..Default::default()
    };
    state.create_seats(&server);
    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let cancelled = AtomicBool::new(false);
    let mut session = OfferSession::open_for_mimes_internal(
        ClipboardType::Regular,
        Seat::Unspecified,
        &[TEXT_MIME],
        Instant::now() + Duration::from_secs(3),
        &cancelled,
        Some(socket_name),
    )
    .unwrap();

    std::thread::scope(|threads| {
        let cancelled = &cancelled;
        let entered = Arc::clone(&entered);
        let release = Arc::clone(&release);
        threads.spawn(move || {
            entered.wait();
            cancelled.store(true, Ordering::Release);
            release.wait();
        });

        assert!(matches!(
            session.read_contents(TEXT_MIME, 1024),
            Err(Error::Cancelled)
        ));
    });
}

#[test]
fn get_contents_wrong_mime_type() {
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let state = State {
        seats: HashMap::from([(
            "seat0".into(),
            SeatInfo {
                offer: Some(OfferInfo::Buffered {
                    data: vec![("application/octet-stream".into(), vec![1, 3, 3, 7])],
                }),
                ..Default::default()
            },
        )]),
        ..Default::default()
    };
    state.create_seats(&server);

    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let result = get_contents_internal(
        ClipboardType::Regular,
        Seat::Unspecified,
        MimeType::Specific("wrong"),
        Some(socket_name),
    );
    assert!(matches!(result, Err(Error::NoMimeType)));
}

proptest! {
    #[test]
    fn get_mime_types_randomized(
        mut state: State,
        clipboard_type: ClipboardType,
        seat_index: prop::sample::Index,
    ) {
        let server = TestServer::new();
        let socket_name = server.socket_name().to_owned();
        server
            .display
            .handle()
            .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

        state.create_seats(&server);

        if state.seats.is_empty() {
            server.run(state);

            let result = get_mime_types_internal(clipboard_type, Seat::Unspecified, Some(socket_name));
            prop_assert!(matches!(result, Err(Error::NoSeats)));
        } else {
            let seat_index = seat_index.index(state.seats.len());
            let (seat_name, seat_info) = state.seats.iter().nth(seat_index).unwrap();
            let seat_name = seat_name.to_owned();
            let seat_info = (*seat_info).clone();

            server.run(state);

            let result = get_mime_types_internal(
                clipboard_type,
                Seat::Specific(&seat_name),
                Some(socket_name),
            );

            let expected_offer = match clipboard_type {
                ClipboardType::Regular => &seat_info.offer,
                ClipboardType::Primary => &seat_info.primary_offer,
            };
            match expected_offer {
                None => prop_assert!(matches!(result, Err(Error::ClipboardEmpty))),
                Some(offer) => prop_assert_eq!(result.unwrap(), offer.data().iter().map(|(k, _)| k.clone()).collect::<Vec<String>>()),
            }
        }
    }

    #[test]
    fn get_contents_randomized(
        mut state: State,
        clipboard_type: ClipboardType,
        seat_index: prop::sample::Index,
        mime_index: prop::sample::Index,
    ) {
        let server = TestServer::new();
        let socket_name = server.socket_name().to_owned();
        server
            .display
            .handle()
            .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

        state.create_seats(&server);

        if state.seats.is_empty() {
            server.run(state);

            let result = get_mime_types_internal(clipboard_type, Seat::Unspecified, Some(socket_name));
            prop_assert!(matches!(result, Err(Error::NoSeats)));
        } else {
            let seat_index = seat_index.index(state.seats.len());
            let (seat_name, seat_info) = state.seats.iter().nth(seat_index).unwrap();
            let seat_name = seat_name.to_owned();
            let seat_info = (*seat_info).clone();

            let expected_offer = match clipboard_type {
                ClipboardType::Regular => &seat_info.offer,
                ClipboardType::Primary => &seat_info.primary_offer,
            };

            let mime_type = match expected_offer {
                Some(offer) if !offer.data().is_empty() => {
                    let mime_index = mime_index.index(offer.data().len());
                    Some(offer.data().iter().map(|(k, _)| k).nth(mime_index).unwrap())
                }
                _ => None,
            };

            server.run(state);

            let result = get_contents_internal(
                clipboard_type,
                Seat::Specific(&seat_name),
                mime_type.map_or(MimeType::Any, |name| MimeType::Specific(name)),
                Some(socket_name),
            );

            match expected_offer {
                None => prop_assert!(matches!(result, Err(Error::ClipboardEmpty))),
                Some(offer) => {
                    if offer.data().is_empty() {
                        prop_assert!(matches!(result, Err(Error::NoMimeType)));
                    } else {
                        let mime_type = mime_type.unwrap();

                        let (mut read, recv_mime_type) = result.unwrap();
                        prop_assert_eq!(&recv_mime_type, mime_type);

                        let mut contents = vec![];
                        read.read_to_end(&mut contents).unwrap();
                        prop_assert_eq!(&contents, offer.data().iter().find(|(k, _)| k == mime_type).map(|(_, v)| &v[..]).unwrap());
                    }
                },
            }

        }
    }
}
