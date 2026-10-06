// MODIFIED BY THE FOLIO CONTRIBUTORS: verify owned-copy retirement and server-barrier reconciliation.

use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use proptest::prelude::*;
use wayland_protocols_wlr::data_control::v1::server::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1;

use crate::copy::*;
use crate::paste;
use crate::paste::get_contents_internal;
use crate::tests::state::*;
use crate::tests::TestServer;

#[test]
fn clear_test() {
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
                    data: vec![("regular".into(), vec![1, 2, 3])],
                }),
                primary_offer: Some(OfferInfo::Buffered {
                    data: vec![("primary".into(), vec![1, 2, 3])],
                }),
            },
        )]),
        ..Default::default()
    };
    state.create_seats(&server);
    let state = Arc::new(Mutex::new(state));

    let socket_name = server.socket_name().to_owned();
    server.run_mutex(state.clone());

    clear_internal(ClipboardType::Regular, Seat::All, Some(socket_name)).unwrap();

    let state = state.lock().unwrap();
    assert!(state.seats["seat0"].offer.is_none());
    assert!(state.seats["seat0"].primary_offer.is_some());
}

#[test]
fn copy_test() {
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let (tx, rx) = channel();

    let state = State {
        seats: HashMap::from([(
            "seat0".into(),
            SeatInfo {
                ..Default::default()
            },
        )]),
        selection_updated_sender: Some(tx),
        ..Default::default()
    };
    state.create_seats(&server);

    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let sources = vec![MimeSource {
        source: Source::Bytes([1, 3, 3, 7][..].into()),
        mime_type: MimeType::Specific("test".into()),
    }];
    copy_internal(Options::new(), sources, Some(socket_name.clone())).unwrap();

    // Wait for the copy.
    let mime_types = rx.recv().unwrap().unwrap();
    assert_eq!(mime_types, ["test"]);

    let (mut read, mime_type) = get_contents_internal(
        paste::ClipboardType::Regular,
        paste::Seat::Unspecified,
        paste::MimeType::Any,
        Some(socket_name.clone()),
    )
    .unwrap();

    let mut contents = vec![];
    read.read_to_end(&mut contents).unwrap();

    assert_eq!(mime_type, "test");
    assert_eq!(contents, [1, 3, 3, 7]);

    clear_internal(ClipboardType::Both, Seat::All, Some(socket_name)).unwrap();
}

#[test]
fn cancelling_a_replaced_owned_copy_leaves_the_new_selection_readable() {
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let (tx, rx) = channel();
    let state = State {
        seats: HashMap::from([("seat0".into(), SeatInfo::default())]),
        selection_updated_sender: Some(tx),
        ..Default::default()
    };
    state.create_seats(&server);
    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let mut options = Options::new();
    options.foreground(true);
    let mut owned = copy_multi_owned_with_socket(
        options,
        vec![MimeSource {
            source: Source::Bytes(b"first owner"[..].into()),
            mime_type: MimeType::Specific("text/plain".into()),
        }],
        socket_name.to_owned(),
    )
    .unwrap();
    assert!(rx
        .recv()
        .unwrap()
        .unwrap()
        .iter()
        .any(|mime_type| mime_type == "text/plain"));

    copy_internal(
        Options::new(),
        vec![MimeSource {
            source: Source::Bytes(b"later owner"[..].into()),
            mime_type: MimeType::Specific("text/plain".into()),
        }],
        Some(socket_name.to_owned()),
    )
    .unwrap();
    assert!(rx
        .recv()
        .unwrap()
        .unwrap()
        .iter()
        .any(|mime_type| mime_type == "text/plain"));

    owned
        .retire_until(Instant::now() + Duration::from_secs(4))
        .unwrap();

    let (mut contents, mime_type) = get_contents_internal(
        paste::ClipboardType::Regular,
        paste::Seat::Unspecified,
        paste::MimeType::Specific("text/plain"),
        Some(socket_name.to_owned()),
    )
    .unwrap();
    let mut bytes = Vec::new();
    contents.read_to_end(&mut bytes).unwrap();
    assert_eq!(mime_type, "text/plain");
    assert_eq!(bytes, b"later owner");

    clear_internal(ClipboardType::Regular, Seat::All, Some(socket_name)).unwrap();
}

#[test]
fn owned_copy_waits_for_a_server_barrier_and_reconciles_before_reading() {
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let (selected_tx, selected_rx) = channel();
    let hold_flush = Arc::new(AtomicBool::new(false));
    let state = State {
        seats: HashMap::from([("seat0".into(), SeatInfo::default())]),
        selection_updated_sender: Some(selected_tx),
        hold_selection_flush: Some(Arc::clone(&hold_flush)),
        ..Default::default()
    };
    state.create_seats(&server);
    let socket_name = server.socket_name().to_owned();
    let mut flush_gate = server.run_with_flush_gate(state, Arc::clone(&hold_flush));

    let mut options = Options::new();
    options.foreground(true);
    let mut owner = copy_multi_owned_with_socket(
        options,
        vec![MimeSource {
            source: Source::Bytes(b"barrier-owned text"[..].into()),
            mime_type: MimeType::Specific("text/plain".into()),
        }],
        socket_name.to_owned(),
    )
    .unwrap();
    assert!(selected_rx
        .recv()
        .unwrap()
        .unwrap()
        .iter()
        .any(|mime_type| mime_type == "text/plain"));
    flush_gate.wait_until_held();

    let operation_cancelled = AtomicBool::new(false);
    assert!(matches!(
        owner.await_claim_until(Instant::now(), &operation_cancelled),
        CopyClaimOutcome::Unconfirmed(_)
    ));

    // The server installed the source, but the client has not received its
    // sync reply. Release the acknowledged flush gate and wake its event loop;
    // reconciliation must receive its server barrier before a paste opens a
    // separate connection.
    flush_gate.release();
    assert_eq!(
        owner.reconcile_until(
            Instant::now() + Duration::from_secs(4),
            Arc::new(AtomicBool::new(false)),
        ),
        CopyClaimOutcome::Confirmed
    );

    let (mut contents, mime_type) = get_contents_internal(
        paste::ClipboardType::Regular,
        paste::Seat::Unspecified,
        paste::MimeType::Specific("text/plain"),
        Some(socket_name.to_owned()),
    )
    .unwrap();
    let mut bytes = Vec::new();
    contents.read_to_end(&mut bytes).unwrap();
    assert_eq!(mime_type, "text/plain");
    assert_eq!(bytes, b"barrier-owned text");

    // A later write is permitted only after that reconciliation. It claims on
    // a fresh connection, waits for its own server barrier, and becomes the
    // next selection in server order.
    let mut next_options = Options::new();
    next_options.foreground(true);
    let mut next_owner = copy_multi_owned_with_socket(
        next_options,
        vec![MimeSource {
            source: Source::Bytes(b"later write"[..].into()),
            mime_type: MimeType::Specific("text/plain".into()),
        }],
        socket_name.to_owned(),
    )
    .unwrap();
    assert!(selected_rx
        .recv()
        .unwrap()
        .unwrap()
        .iter()
        .any(|mime_type| mime_type == "text/plain"));
    assert_eq!(
        next_owner.await_claim_until(
            Instant::now() + Duration::from_secs(4),
            &operation_cancelled,
        ),
        CopyClaimOutcome::Confirmed
    );

    owner
        .retire_until(Instant::now() + Duration::from_secs(4))
        .unwrap();

    let (mut contents, mime_type) = get_contents_internal(
        paste::ClipboardType::Regular,
        paste::Seat::Unspecified,
        paste::MimeType::Specific("text/plain"),
        Some(socket_name.to_owned()),
    )
    .unwrap();
    let mut bytes = Vec::new();
    contents.read_to_end(&mut bytes).unwrap();
    assert_eq!(mime_type, "text/plain");
    assert_eq!(bytes, b"later write");

    let retire_cancelled = AtomicBool::new(true);
    assert!(next_owner
        .retire_until_cancellable(
            Instant::now() + Duration::from_secs(4),
            Some(&retire_cancelled),
        )
        .is_err());
    let (mut contents, mime_type) = get_contents_internal(
        paste::ClipboardType::Regular,
        paste::Seat::Unspecified,
        paste::MimeType::Specific("text/plain"),
        Some(socket_name.to_owned()),
    )
    .unwrap();
    let mut bytes = Vec::new();
    contents.read_to_end(&mut bytes).unwrap();
    assert_eq!(mime_type, "text/plain");
    assert_eq!(bytes, b"later write");

    assert!(next_owner.retire_until(Instant::now()).is_err());
    next_owner
        .retire_until(Instant::now() + Duration::from_secs(4))
        .unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn cancelling_an_owned_copy_interrupts_a_stalled_send() {
    const PROBE: &str = "FOLIO_WL_OWNED_SEND_PROBE";
    if std::env::var_os(PROBE).is_some() {
        run_owned_send_probe();
        return;
    }

    let runtime =
        std::env::temp_dir().join(format!("wl-clipboard-owned-send-{}", std::process::id()));
    std::fs::create_dir(&runtime).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let _runtime = OwnedSendProbeDirectory(runtime.clone());

    let executable = std::env::current_exe().unwrap();
    let output = std::process::Command::new("timeout")
        .args(["--signal=KILL", "10s"])
        .arg(executable)
        .args([
            "--exact",
            "tests::copy::cancelling_an_owned_copy_interrupts_a_stalled_send",
            "--nocapture",
        ])
        .env(PROBE, "1")
        .env("XDG_RUNTIME_DIR", &runtime)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "owned-send probe failed: status={:?}; stdout={}; stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[cfg(target_os = "linux")]
struct OwnedSendProbeDirectory(std::path::PathBuf);

#[cfg(target_os = "linux")]
impl Drop for OwnedSendProbeDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(target_os = "linux")]
fn run_owned_send_probe() {
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let (selected, selection) = channel();
    let state = State {
        seats: HashMap::from([("seat0".into(), SeatInfo::default())]),
        selection_updated_sender: Some(selected),
        ..Default::default()
    };
    state.create_seats(&server);
    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let (capacity_reader, capacity_writer) = os_pipe::pipe().unwrap();
    use std::os::fd::AsRawFd;
    // SAFETY: F_GETPIPE_SZ takes no pointer, and the reader owns this valid pipe descriptor.
    let pipe_capacity = unsafe { libc::fcntl(capacity_reader.as_raw_fd(), libc::F_GETPIPE_SZ) };
    assert!(pipe_capacity > 0);
    drop((capacity_reader, capacity_writer));
    let payload = vec![b'x'; pipe_capacity as usize * 2];

    let mut options = Options::new();
    options.foreground(true);
    let mut owner = copy_multi_owned_with_socket(
        options,
        vec![MimeSource {
            source: Source::Bytes(payload.into_boxed_slice()),
            mime_type: MimeType::Specific("application/octet-stream".into()),
        }],
        socket_name.to_owned(),
    )
    .unwrap();
    assert!(selection
        .recv()
        .unwrap()
        .unwrap()
        .contains(&"application/octet-stream".to_owned()));

    let (transfer, mime_type) = get_contents_internal(
        paste::ClipboardType::Regular,
        paste::Seat::Unspecified,
        paste::MimeType::Specific("application/octet-stream"),
        Some(socket_name.to_owned()),
    )
    .unwrap();
    assert_eq!(mime_type, "application/octet-stream");

    while rustix::io::ioctl_fionread(&transfer).unwrap() < pipe_capacity as u64 {
        std::hint::spin_loop();
    }
    eprintln!("the unread transfer pipe is full; requesting owner cancellation");
    owner.cancel();
    eprintln!("owner cancellation requested; waiting for its serving thread");
    owner
        .retire_until(Instant::now() + Duration::from_secs(4))
        .unwrap();

    drop(transfer);
}

#[test]
fn copy_multi_test() {
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let (tx, rx) = channel();

    let state = State {
        seats: HashMap::from([(
            "seat0".into(),
            SeatInfo {
                ..Default::default()
            },
        )]),
        selection_updated_sender: Some(tx),
        ..Default::default()
    };
    state.create_seats(&server);

    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let sources = vec![
        MimeSource {
            source: Source::Bytes([1, 3, 3, 7][..].into()),
            mime_type: MimeType::Specific("test".into()),
        },
        MimeSource {
            source: Source::Bytes([2, 4, 4][..].into()),
            mime_type: MimeType::Specific("test2".into()),
        },
        // Ignored because it's the second "test" MIME type.
        MimeSource {
            source: Source::Bytes([4, 3, 2, 1][..].into()),
            mime_type: MimeType::Specific("test".into()),
        },
        // The first text source, additional text types should fall back here.
        MimeSource {
            source: Source::Bytes(b"hello fallback"[..].into()),
            mime_type: MimeType::Text,
        },
        // A specific override of an additional text type.
        MimeSource {
            source: Source::Bytes(b"hello TEXT"[..].into()),
            mime_type: MimeType::Specific("TEXT".into()),
        },
    ];
    copy_internal(Options::new(), sources, Some(socket_name.clone())).unwrap();

    // Wait for the copy.
    let mut mime_types = rx.recv().unwrap().unwrap();
    mime_types.sort_unstable();
    assert_eq!(
        mime_types,
        [
            "STRING",
            "TEXT",
            "UTF8_STRING",
            "test",
            "test2",
            "text/plain",
            "text/plain;charset=utf-8",
        ]
    );

    let expected = [
        ("test", &[1, 3, 3, 7][..]),
        ("test2", &[2, 4, 4][..]),
        ("STRING", &b"hello fallback"[..]),
        ("TEXT", &b"hello TEXT"[..]),
    ];

    for (mime_type, expected_contents) in expected {
        let mut read = get_contents_internal(
            paste::ClipboardType::Regular,
            paste::Seat::Unspecified,
            paste::MimeType::Specific(mime_type),
            Some(socket_name.clone()),
        )
        .unwrap()
        .0;

        let mut contents = vec![];
        read.read_to_end(&mut contents).unwrap();

        assert_eq!(contents, expected_contents);
    }

    clear_internal(ClipboardType::Both, Seat::All, Some(socket_name)).unwrap();
}

#[test]
fn copy_multi_no_additional_text_mime_types_test() {
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let (tx, rx) = channel();

    let state = State {
        seats: HashMap::from([(
            "seat0".into(),
            SeatInfo {
                ..Default::default()
            },
        )]),
        selection_updated_sender: Some(tx),
        ..Default::default()
    };
    state.create_seats(&server);

    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let mut opts = Options::new();
    opts.omit_additional_text_mime_types(true);
    let sources = vec![
        MimeSource {
            source: Source::Bytes([1, 3, 3, 7][..].into()),
            mime_type: MimeType::Specific("test".into()),
        },
        MimeSource {
            source: Source::Bytes([2, 4, 4][..].into()),
            mime_type: MimeType::Specific("test2".into()),
        },
        // Ignored because it's the second "test" MIME type.
        MimeSource {
            source: Source::Bytes([4, 3, 2, 1][..].into()),
            mime_type: MimeType::Specific("test".into()),
        },
        // A specific override of an additional text type.
        MimeSource {
            source: Source::Bytes(b"hello TEXT"[..].into()),
            mime_type: MimeType::Specific("TEXT".into()),
        },
    ];
    copy_internal(opts, sources, Some(socket_name.clone())).unwrap();

    // Wait for the copy.
    let mut mime_types = rx.recv().unwrap().unwrap();
    mime_types.sort_unstable();
    assert_eq!(mime_types, ["TEXT", "test", "test2"]);

    let expected = [
        ("test", &[1, 3, 3, 7][..]),
        ("test2", &[2, 4, 4][..]),
        ("TEXT", &b"hello TEXT"[..]),
    ];

    for (mime_type, expected_contents) in expected {
        let mut read = get_contents_internal(
            paste::ClipboardType::Regular,
            paste::Seat::Unspecified,
            paste::MimeType::Specific(mime_type),
            Some(socket_name.clone()),
        )
        .unwrap()
        .0;

        let mut contents = vec![];
        read.read_to_end(&mut contents).unwrap();

        assert_eq!(contents, expected_contents);
    }

    clear_internal(ClipboardType::Both, Seat::All, Some(socket_name)).unwrap();
}

#[test]
fn password_manager_hint_is_offered_last() {
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let (tx, rx) = channel();
    let state = State {
        seats: HashMap::from([("seat0".into(), SeatInfo::default())]),
        selection_updated_sender: Some(tx),
        ..Default::default()
    };
    state.create_seats(&server);
    let socket_name = server.socket_name().to_owned();
    server.run(state);

    copy_internal(
        Options::new(),
        vec![
            MimeSource {
                source: Source::Bytes(b"secret"[..].into()),
                mime_type: MimeType::Specific("x-kde-passwordManagerHint".into()),
            },
            MimeSource {
                source: Source::Bytes(b"actual contents"[..].into()),
                mime_type: MimeType::Specific("image/png".into()),
            },
        ],
        Some(socket_name.clone()),
    )
    .unwrap();

    assert_eq!(
        rx.recv().unwrap().unwrap(),
        ["image/png", "x-kde-passwordManagerHint"]
    );
    clear_internal(ClipboardType::Regular, Seat::All, Some(socket_name)).unwrap();
}

#[test]
fn password_manager_hint_does_not_count_toward_serve_requests() {
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let (tx, rx) = channel();
    let state = State {
        seats: HashMap::from([("seat0".into(), SeatInfo::default())]),
        selection_updated_sender: Some(tx),
        ..Default::default()
    };
    state.create_seats(&server);
    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let socket_clone = socket_name.clone();
    let copy_thread = thread::spawn(move || {
        let mut opts = Options::new();
        opts.foreground(true).serve_requests(ServeRequests::Only(1));
        copy_internal(
            opts,
            vec![
                MimeSource {
                    source: Source::Bytes(b"secret"[..].into()),
                    mime_type: MimeType::Specific("x-kde-passwordManagerHint".into()),
                },
                MimeSource {
                    source: Source::Bytes(b"actual contents"[..].into()),
                    mime_type: MimeType::Specific("image/png".into()),
                },
            ],
            Some(socket_clone),
        )
    });

    rx.recv().unwrap();
    for (mime_type, expected) in [
        ("x-kde-passwordManagerHint", &b"secret"[..]),
        ("image/png", &b"actual contents"[..]),
    ] {
        let (mut read, selected) = get_contents_internal(
            paste::ClipboardType::Regular,
            paste::Seat::Unspecified,
            paste::MimeType::Specific(mime_type),
            Some(socket_name.clone()),
        )
        .unwrap();
        assert_eq!(selected, mime_type);
        let mut contents = Vec::new();
        read.read_to_end(&mut contents).unwrap();
        assert_eq!(contents, expected);
    }

    copy_thread.join().unwrap().unwrap();
}

#[test]
fn sensitive_option_adds_hint_without_overwriting_explicit_data() {
    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let (tx, rx) = channel();
    let state = State {
        seats: HashMap::from([("seat0".into(), SeatInfo::default())]),
        selection_updated_sender: Some(tx),
        ..Default::default()
    };
    state.create_seats(&server);
    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let mut opts = Options::new();
    opts.sensitive(true);
    copy_internal(
        opts.clone(),
        vec![MimeSource {
            source: Source::Bytes(b"contents"[..].into()),
            mime_type: MimeType::Specific("image/png".into()),
        }],
        Some(socket_name.clone()),
    )
    .unwrap();
    assert_eq!(
        rx.recv().unwrap().unwrap(),
        ["image/png", "x-kde-passwordManagerHint"]
    );

    let read_hint = || {
        let (mut read, _) = get_contents_internal(
            paste::ClipboardType::Regular,
            paste::Seat::Unspecified,
            paste::MimeType::Specific("x-kde-passwordManagerHint"),
            Some(socket_name.clone()),
        )
        .unwrap();
        let mut contents = String::new();
        read.read_to_string(&mut contents).unwrap();
        contents
    };
    assert_eq!(read_hint(), "secret");

    copy_internal(
        opts,
        vec![
            MimeSource {
                source: Source::Bytes(b"contents"[..].into()),
                mime_type: MimeType::Specific("image/png".into()),
            },
            MimeSource {
                source: Source::Bytes(b"custom"[..].into()),
                mime_type: MimeType::Specific("x-kde-passwordManagerHint".into()),
            },
        ],
        Some(socket_name.clone()),
    )
    .unwrap();
    assert_eq!(
        rx.recv().unwrap().unwrap(),
        ["image/png", "x-kde-passwordManagerHint"]
    );
    assert_eq!(read_hint(), "custom");

    clear_internal(ClipboardType::Regular, Seat::All, Some(socket_name)).unwrap();
}

// The idea here is to exceed the pipe capacity. This fails unless O_NONBLOCK is cleared when
// sending data over the pipe using cat.
#[test]
fn copy_large() {
    // Assuming the default pipe capacity is 65536.
    let mut bytes_to_copy = vec![];
    for i in 0..65536 * 10 {
        bytes_to_copy.push((i % 256) as u8);
    }

    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let (tx, rx) = channel();

    let state = State {
        seats: HashMap::from([(
            "seat0".into(),
            SeatInfo {
                ..Default::default()
            },
        )]),
        selection_updated_sender: Some(tx),
        // Emulate what XWayland does and set O_NONBLOCK.
        set_nonblock_on_write_fd: true,
        ..Default::default()
    };
    state.create_seats(&server);

    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let sources = vec![MimeSource {
        source: Source::Bytes(bytes_to_copy.clone().into_boxed_slice()),
        mime_type: MimeType::Specific("test".into()),
    }];
    copy_internal(Options::new(), sources, Some(socket_name.clone())).unwrap();

    // Wait for the copy.
    let mime_types = rx.recv().unwrap().unwrap();
    assert_eq!(mime_types, ["test"]);

    let (mut read, mime_type) = get_contents_internal(
        paste::ClipboardType::Regular,
        paste::Seat::Unspecified,
        paste::MimeType::Any,
        Some(socket_name.clone()),
    )
    .unwrap();

    let mut contents = vec![];
    read.read_to_end(&mut contents).unwrap();

    assert_eq!(mime_type, "test");
    assert_eq!(contents.len(), bytes_to_copy.len());
    assert_eq!(contents, bytes_to_copy);

    clear_internal(ClipboardType::Both, Seat::All, Some(socket_name)).unwrap();
}

#[test]
fn copy_large_epipe() {
    // Data larger than the default pipe capacity of 65536 ensures the writer
    // blocks, making it likely to hit EPIPE when the reader closes early.
    let mut bytes_to_copy = vec![];
    for i in 0..65536 * 3 {
        bytes_to_copy.push((i % 256) as u8);
    }

    let server = TestServer::new();
    server
        .display
        .handle()
        .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

    let (tx, rx) = channel();

    let state = State {
        seats: HashMap::from([(
            "seat0".into(),
            SeatInfo {
                ..Default::default()
            },
        )]),
        selection_updated_sender: Some(tx),
        set_nonblock_on_write_fd: true,
        ..Default::default()
    };
    state.create_seats(&server);

    let socket_name = server.socket_name().to_owned();
    server.run(state);

    let sources = vec![MimeSource {
        source: Source::Bytes(bytes_to_copy.into_boxed_slice()),
        mime_type: MimeType::Specific("test".into()),
    }];

    // Use foreground mode with a single serve request so errors propagate
    // to the caller. Run in a separate thread so we can paste concurrently.
    let socket_clone = socket_name.clone();
    let copy_thread = thread::spawn(move || {
        let mut opts = Options::new();
        opts.foreground(true);
        opts.serve_requests(ServeRequests::Only(1));
        copy_internal(opts, sources, Some(socket_clone))
    });

    // Wait for the copy.
    let mime_types = rx.recv().unwrap().unwrap();
    assert_eq!(mime_types, ["test"]);

    // Start a paste and close the read end immediately to trigger EPIPE on
    // the copy side. With data larger than the pipe buffer, the writer is
    // still in progress, so closing the read end causes EPIPE on the next
    // write. The copy should still succeed because EPIPE is treated as
    // the destination closing the pipe early, which is valid.
    let (read, mime_type) = get_contents_internal(
        paste::ClipboardType::Regular,
        paste::Seat::Unspecified,
        paste::MimeType::Any,
        Some(socket_name),
    )
    .unwrap();

    assert_eq!(mime_type, "test");

    // Drop the read end while the writer is still writing to trigger EPIPE.
    drop(read);

    // The copy must complete without error despite the EPIPE.
    copy_thread.join().unwrap().unwrap();
}

proptest! {
    #[test]
    fn copy_randomized(
        mut state: State,
        clipboard_type: ClipboardType,
        source: Source,
        mime_type: MimeType,
        seat_index: prop::sample::Index,
        clipboard_type_index: prop::sample::Index,
    ) {
        prop_assume!(!state.seats.is_empty());

        let server = TestServer::new();
        server
            .display
            .handle()
            .create_global::<State, ZwlrDataControlManagerV1, ()>(2, ());

        let (tx, rx) = channel();
        state.selection_updated_sender = Some(tx);

        state.create_seats(&server);

        let seat_index = seat_index.index(state.seats.len());
        let seat_name = state.seats.keys().nth(seat_index).unwrap();
        let seat_name = seat_name.to_owned();

        let paste_clipboard_type = match clipboard_type {
            ClipboardType::Regular => paste::ClipboardType::Regular,
            ClipboardType::Primary => paste::ClipboardType::Primary,
            ClipboardType::Both => *clipboard_type_index
                .get(&[paste::ClipboardType::Regular, paste::ClipboardType::Primary]),
        };

        let socket_name = server.socket_name().to_owned();
        server.run(state);

        let expected_contents = match &source {
            Source::Bytes(bytes) => bytes.clone(),
            Source::StdIn => unreachable!(),
        };

        let sources = vec![MimeSource {
            source,
            mime_type: mime_type.clone(),
        }];
        let mut opts = Options::new();
        opts.clipboard(clipboard_type);
        opts.seat(Seat::Specific(seat_name.clone()));
        opts.omit_additional_text_mime_types(true);
        copy_internal(opts, sources, Some(socket_name.clone())).unwrap();

        // Wait for the copy.
        let mut mime_types = rx.recv().unwrap().unwrap();
        mime_types.sort_unstable();
        match &mime_type {
            MimeType::Autodetect => unreachable!(),
            MimeType::Text => assert_eq!(mime_types, ["text/plain"]),
            MimeType::Specific(mime) => assert_eq!(mime_types, std::slice::from_ref(mime)),
        }

        let paste_mime_type = match mime_type {
            MimeType::Autodetect => unreachable!(),
            MimeType::Text => "text/plain".into(),
            MimeType::Specific(mime) => mime,
        };
        let (mut read, mime_type) = get_contents_internal(
            paste_clipboard_type,
            paste::Seat::Specific(&seat_name),
            paste::MimeType::Specific(&paste_mime_type),
            Some(socket_name.clone()),
        )
        .unwrap();

        let mut contents = vec![];
        read.read_to_end(&mut contents).unwrap();

        assert_eq!(mime_type, paste_mime_type);
        assert_eq!(contents.into_boxed_slice(), expected_contents);

        clear_internal(clipboard_type, Seat::Specific(seat_name), Some(socket_name)).unwrap();
    }
}
