#![cfg(target_os = "linux")]
#![allow(clippy::disallowed_methods)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use bt_platform::{
    LINUX_CLIPBOARD_OPERATION_BUDGET, LinuxClipboardBackend, LinuxX11ClaimOutcome as ClaimOutcome,
    LinuxX11OwnerCandidate, ThreadPriority, clipboard_text_on_worker, release_clipboard_on_worker,
    set_clipboard_text_on_worker, spawn_at_priority,
};
use x11rb::protocol::xproto::{ImageOrder, Screen, Setup};
use x11rb::x11_utils::Serialize;

const PRIVATE_XORG_CHILD: &str = "FOLIO_PRIVATE_XORG_CHILD";
const PRIVATE_XORG_TEST: &str = "private_xorg_copy_paste_uses_confirmed_owner";
const FAKE_ACK_CHILD: &str = "FOLIO_FAKE_X11_ACK_CHILD";
const FAKE_ACK_TEST: &str = "unconfirmed_claim_reconciles_without_reclaiming";
const FAKE_ACK_NOTICE: &str = "FOLIO_X11_OWNER_UNCONFIRMED";

struct XorgServer {
    child: Child,
    scratch: std::path::PathBuf,
    keep_artifacts: bool,
}

impl Drop for XorgServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if !self.keep_artifacts {
            let _ = std::fs::remove_dir_all(&self.scratch);
        }
    }
}

#[test]
fn private_xorg_copy_paste_uses_confirmed_owner() {
    if std::env::var_os(PRIVATE_XORG_CHILD).is_some() {
        run_copy_paste_flow();
        return;
    }
    if std::env::var_os("BT_RUN_PRIVATE_XORG").is_none() {
        return;
    }

    let xorg = required_test_path("FOLIO_PRIVATE_XORG");
    let module_overlay = required_test_path("FOLIO_PRIVATE_XORG_MODULE_OVERLAY");
    let config = required_test_path("FOLIO_PRIVATE_XORG_CONFIG");
    let library_path = required_test_path("FOLIO_PRIVATE_XORG_LD_LIBRARY_PATH");
    let keep_artifacts = std::env::var_os("FOLIO_PRIVATE_XORG_EVIDENCE_DIR").is_some();
    let scratch = std::env::var_os("FOLIO_PRIVATE_XORG_EVIDENCE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "folio-private-xorg-clipboard-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
        });
    std::fs::create_dir_all(&scratch).unwrap();
    let display_log = scratch.join("Xorg.log");
    let (display_parent, display_child) = UnixStream::pair().unwrap();
    let display_fd = display_child.as_raw_fd();
    let mut command = Command::new(&xorg);
    command
        .args([
            "-displayfd",
            &display_fd.to_string(),
            "-config",
            config.to_str().unwrap(),
            "-modulepath",
            module_overlay.to_str().unwrap(),
            "-logfile",
            display_log.to_str().unwrap(),
            "-nolisten",
            "tcp",
            "-ac",
            "-noreset",
            "-novtswitch",
            "-sharevts",
        ])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &scratch)
        .env("LD_LIBRARY_PATH", &library_path)
        .current_dir(&scratch)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: the displayfd socket is the owned descriptor named in the
    // Xorg arguments; clearing CLOEXEC lets the private server publish its
    // ready display number to this test process.
    unsafe {
        command.pre_exec(move || {
            let flags = libc::fcntl(display_fd, libc::F_GETFD);
            if flags < 0 || libc::fcntl(display_fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let server = XorgServer {
        child: command.spawn().unwrap(),
        scratch,
        keep_artifacts,
    };
    drop(display_child);
    let mut display_reader = BufReader::new(display_parent);
    let mut display_number = String::new();
    let bytes_read = display_reader.read_line(&mut display_number).unwrap();
    assert!(
        bytes_read > 0,
        "private Xorg exited before displayfd readiness: {}",
        std::fs::read_to_string(display_log).unwrap_or_default()
    );
    let display_number = display_number.trim();
    assert!(display_number.bytes().all(|byte| byte.is_ascii_digit()));

    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", PRIVATE_XORG_TEST, "--nocapture"])
        .env(PRIVATE_XORG_CHILD, "1")
        .env("DISPLAY", format!(":{display_number}"))
        .output()
        .unwrap();
    std::fs::write(server.scratch.join("copy-paste.stdout"), &output.stdout).unwrap();
    std::fs::write(server.scratch.join("copy-paste.stderr"), &output.stderr).unwrap();
    std::fs::write(
        server.scratch.join("display.txt"),
        format!("DISPLAY=:{display_number}\n"),
    )
    .unwrap();
    std::fs::write(
        server.scratch.join("run.env"),
        format!(
            "FOLIO_PRIVATE_XORG={}\nFOLIO_PRIVATE_XORG_MODULE_OVERLAY={}\nFOLIO_PRIVATE_XORG_CONFIG={}\nFOLIO_PRIVATE_XORG_LD_LIBRARY_PATH={}\nRUSTUP_HOME={}\nRUSTFLAGS={}\n",
            xorg.display(),
            module_overlay.display(),
            config.display(),
            library_path.display(),
            std::env::var("RUSTUP_HOME").unwrap_or_default(),
            std::env::var("RUSTFLAGS").unwrap_or_default(),
        ),
    )
    .unwrap();
    let worktree = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    std::fs::write(
        server.scratch.join("run-private-xorg.sh"),
        format!(
            "#!/bin/sh\nset -eu\ncd {}\ntimeout 60s env BT_RUN_PRIVATE_XORG=1 FOLIO_PRIVATE_XORG={} FOLIO_PRIVATE_XORG_MODULE_OVERLAY={} FOLIO_PRIVATE_XORG_CONFIG={} FOLIO_PRIVATE_XORG_LD_LIBRARY_PATH={} FOLIO_PRIVATE_XORG_EVIDENCE_DIR={} RUSTUP_HOME={} RUSTFLAGS='-Ctarget-feature=-crt-static' cargo +1.94.1 test -p bt-platform --test linux_clipboard_x11 private_xorg_copy_paste_uses_confirmed_owner -- --nocapture\n",
            shell_quote(&worktree.display().to_string()),
            shell_quote(&xorg.display().to_string()),
            shell_quote(&module_overlay.display().to_string()),
            shell_quote(&config.display().to_string()),
            shell_quote(&library_path.display().to_string()),
            shell_quote(&server.scratch.display().to_string()),
            shell_quote(&std::env::var("RUSTUP_HOME").unwrap_or_default()),
        ),
    )
    .unwrap();
    assert!(
        output.status.success(),
        "private Xorg copy/paste child failed:\n{}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        std::fs::read_to_string(display_log).unwrap_or_default()
    );
    drop(server);
}

#[test]
fn unconfirmed_claim_reconciles_without_reclaiming() {
    if std::env::var_os(FAKE_ACK_CHILD).is_some() {
        run_unconfirmed_reconcile_child();
        return;
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    assert!(port > 6000);
    let display = format!("127.0.0.1:{}", port - 6000);
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let server = std::thread::spawn(move || serve_fake_x11(listener, release_rx));

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", FAKE_ACK_TEST, "--nocapture"])
        .env(FAKE_ACK_CHILD, "1")
        .env("DISPLAY", display)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child_input = child.stdin.take().unwrap();
    let mut child_output = BufReader::new(child.stdout.take().unwrap());
    let mut transcript = String::new();
    let mut line = String::new();
    let mut reached_unconfirmed = false;
    while child_output.read_line(&mut line).unwrap() != 0 {
        transcript.push_str(&line);
        if line.contains(FAKE_ACK_NOTICE) {
            reached_unconfirmed = true;
            break;
        }
        line.clear();
    }
    let _ = release_tx.send(());
    if reached_unconfirmed {
        child_input.write_all(b"g").unwrap();
    }
    child_output.read_to_string(&mut transcript).unwrap();
    drop(child_input);
    let status = child.wait().unwrap();
    let stderr = child
        .stderr
        .take()
        .map(|mut stderr| {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        })
        .unwrap_or_default();
    let facts = server.join().unwrap().unwrap();
    assert!(
        reached_unconfirmed,
        "child never reported an unconfirmed claim: {transcript}\n{stderr}"
    );
    assert!(
        status.success(),
        "fake X11 child failed:\n{transcript}\n{stderr}"
    );
    assert_eq!(
        facts.claims, 2,
        "the next copy claims only after reconciliation"
    );
    assert_eq!(
        facts.delayed_replies, 2,
        "the fake server must release both ordered owner replies"
    );
    assert_eq!(
        facts.destroyed_windows, 1,
        "the replaced candidate is destroyed, while cutoff expiry cancels the retained candidate before retry"
    );
    assert!(
        !facts.claim_before_reconciliation,
        "the second selection request cannot overtake the unresolved first claim"
    );
}

fn run_copy_paste_flow() {
    let text = "private Xwayland copy to paste".to_owned();
    let copied = text.clone();
    spawn_at_priority(
        "linux-x11-private-copy-paste-test",
        ThreadPriority::Normal,
        move |worker| {
            let cancelled = Arc::new(AtomicBool::new(false));
            let deadline = Instant::now() + Duration::from_secs(4);
            let mut owner =
                LinuxX11OwnerCandidate::start(worker, copied, deadline, Arc::clone(&cancelled))
                    .unwrap();
            assert_eq!(
                owner.await_claim_until(worker, deadline, &cancelled),
                ClaimOutcome::Confirmed
            );

            let paste_cancelled = Arc::new(AtomicBool::new(false));
            let pasted = spawn_at_priority(
                "linux-x11-private-paste-test",
                ThreadPriority::Normal,
                move |paste_worker| {
                    clipboard_text_on_worker(
                        paste_worker,
                        LinuxClipboardBackend::X11,
                        Instant::now() + Duration::from_secs(4),
                        paste_cancelled,
                    )
                },
            )
            .unwrap()
            .join()
            .unwrap()
            .unwrap();
            assert_eq!(pasted, text);

            assert!(
                owner
                    .retire_until_cancellable(
                        worker,
                        Instant::now() + Duration::from_secs(3),
                        Some(Arc::new(AtomicBool::new(true))),
                    )
                    .is_err()
            );

            let expected = text.clone();
            let pasted_after_canceled_retire = spawn_at_priority(
                "linux-x11-private-paste-test",
                ThreadPriority::Normal,
                move |paste_worker| {
                    clipboard_text_on_worker(
                        paste_worker,
                        LinuxClipboardBackend::X11,
                        Instant::now() + Duration::from_secs(4),
                        Arc::new(AtomicBool::new(false)),
                    )
                },
            )
            .unwrap()
            .join()
            .unwrap()
            .unwrap();
            assert_eq!(pasted_after_canceled_retire, expected);

            owner
                .retire_until(worker, Instant::now() + Duration::from_secs(3))
                .unwrap();
        },
    )
    .unwrap()
    .join()
    .unwrap();
}

fn required_test_path(name: &str) -> std::path::PathBuf {
    std::env::var_os(name)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| panic!("{name} is required with BT_RUN_PRIVATE_XORG=1"))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn run_unconfirmed_reconcile_child() {
    let (outcome_tx, outcome_rx) = mpsc::channel();
    let (continue_tx, continue_rx) = mpsc::channel();
    let worker = spawn_at_priority(
        "linux-x11-fake-ack-test",
        ThreadPriority::Normal,
        move |worker| {
            let cancelled = Arc::new(AtomicBool::new(false));
            let deadline = Instant::now() + Duration::from_millis(300);
            let first = set_clipboard_text_on_worker(
                worker,
                LinuxClipboardBackend::X11,
                "retained after delayed acknowledgement".to_owned(),
                deadline,
                Arc::clone(&cancelled),
            )
            .map_err(|error| error.to_string());
            outcome_tx.send(first).unwrap();
            continue_rx.recv().unwrap();
            let next_deadline = Instant::now() + LINUX_CLIPBOARD_OPERATION_BUDGET;
            set_clipboard_text_on_worker(
                worker,
                LinuxClipboardBackend::X11,
                "later confirmed write".to_owned(),
                next_deadline,
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
            assert!(release_clipboard_on_worker(worker, Instant::now()).is_err());
            release_clipboard_on_worker(worker, Instant::now() + Duration::from_secs(3)).unwrap();
        },
    )
    .unwrap();

    let first_error = outcome_rx.recv().unwrap().unwrap_err();
    assert!(
        first_error.contains("deadline"),
        "the first dispatcher write reports its withheld acknowledgement: {first_error}"
    );
    println!("{FAKE_ACK_NOTICE}");
    std::io::stdout().flush().unwrap();
    let mut release = [0; 1];
    std::io::stdin().read_exact(&mut release).unwrap();
    continue_tx.send(()).unwrap();
    worker.join().unwrap();
}

#[derive(Clone, Default)]
struct FakeX11Facts {
    claims: usize,
    delayed_replies: usize,
    destroyed_windows: usize,
    claim_before_reconciliation: bool,
}

struct FakeX11Shared {
    facts: Mutex<FakeX11Facts>,
    selected_owner: Mutex<u32>,
    reconciliation_complete: AtomicBool,
}

fn serve_fake_x11(
    listener: TcpListener,
    release: Arc<Mutex<Receiver<()>>>,
) -> std::io::Result<FakeX11Facts> {
    let shared = Arc::new(FakeX11Shared {
        facts: Mutex::new(FakeX11Facts::default()),
        selected_owner: Mutex::new(0),
        reconciliation_complete: AtomicBool::new(false),
    });
    let mut clients = Vec::new();
    for connection_index in 0..2 {
        let (stream, _) = listener.accept()?;
        let shared = Arc::clone(&shared);
        let release = Arc::clone(&release);
        clients.push(std::thread::spawn(move || {
            serve_fake_x11_client(stream, connection_index, shared, release)
        }));
    }
    for client in clients {
        client
            .join()
            .map_err(|_| std::io::Error::other("fake X11 client worker panicked"))??;
    }
    Ok(shared
        .facts
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone())
}

fn serve_fake_x11_client(
    mut stream: TcpStream,
    connection_index: usize,
    shared: Arc<FakeX11Shared>,
    release: Arc<Mutex<Receiver<()>>>,
) -> std::io::Result<()> {
    let mut setup_request = [0; 12];
    stream.read_exact(&mut setup_request)?;
    let auth_name_len = usize::from(u16::from_le_bytes([setup_request[6], setup_request[7]]));
    let auth_data_len = usize::from(u16::from_le_bytes([setup_request[8], setup_request[9]]));
    let extra = auth_name_len.div_ceil(4) * 4 + auth_data_len.div_ceil(4) * 4;
    let mut auth = vec![0; extra];
    stream.read_exact(&mut auth)?;
    stream.write_all(
        &Setup {
            status: 1,
            protocol_major_version: 11,
            length: 18,
            resource_id_base: 0x100000,
            resource_id_mask: 0x000fff,
            maximum_request_length: u16::MAX,
            image_byte_order: ImageOrder::LSB_FIRST,
            bitmap_format_bit_order: ImageOrder::LSB_FIRST,
            bitmap_format_scanline_unit: 8,
            bitmap_format_scanline_pad: 8,
            min_keycode: 8,
            max_keycode: 255,
            roots: vec![Screen {
                root: 1,
                root_visual: 3,
                root_depth: 24,
                ..Screen::default()
            }],
            ..Setup::default()
        }
        .serialize(),
    )?;

    let mut sequence = 0u16;
    let mut next_atom = 20u32;
    let mut pending_owner_reply = None;
    loop {
        let mut header = [0; 4];
        match stream.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error),
        }
        sequence = sequence.wrapping_add(1);
        let request_bytes = usize::from(u16::from_le_bytes([header[2], header[3]])) * 4;
        if request_bytes < 4 {
            return Err(std::io::Error::other("invalid X11 request length"));
        }
        let mut body = vec![0; request_bytes - 4];
        stream.read_exact(&mut body)?;
        match header[0] {
            16 => {
                let atom = next_atom;
                next_atom = next_atom.wrapping_add(1);
                let mut reply = [0; 32];
                reply[0] = 1;
                reply[2..4].copy_from_slice(&sequence.to_le_bytes());
                reply[8..12].copy_from_slice(&atom.to_le_bytes());
                stream.write_all(&reply)?;
            }
            22 => {
                *shared
                    .selected_owner
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) =
                    u32::from_le_bytes(body[0..4].try_into().unwrap());
                let mut facts = shared
                    .facts
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                facts.claims += 1;
                if facts.claims > 1 && !shared.reconciliation_complete.load(Ordering::Acquire) {
                    facts.claim_before_reconciliation = true;
                }
            }
            23 => {
                if connection_index == 0 && !shared.reconciliation_complete.load(Ordering::Acquire)
                {
                    if let Some(previous_sequence) = pending_owner_reply.take() {
                        release
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .recv()
                            .map_err(std::io::Error::other)?;
                        let selected_owner = *shared
                            .selected_owner
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
                        write_owner_reply(&mut stream, previous_sequence, selected_owner)?;
                        write_owner_reply(&mut stream, sequence, selected_owner)?;
                        shared
                            .reconciliation_complete
                            .store(true, Ordering::Release);
                        shared
                            .facts
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .delayed_replies += 2;
                    } else {
                        pending_owner_reply = Some(sequence);
                    }
                } else {
                    let selected_owner = *shared
                        .selected_owner
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    write_owner_reply(&mut stream, sequence, selected_owner)?;
                }
            }
            4 => {
                shared
                    .facts
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .destroyed_windows += 1;
            }
            _ => {}
        }
    }
    Ok(())
}

fn write_owner_reply(stream: &mut TcpStream, sequence: u16, owner: u32) -> std::io::Result<()> {
    let mut reply = [0; 32];
    reply[0] = 1;
    reply[2..4].copy_from_slice(&sequence.to_le_bytes());
    reply[8..12].copy_from_slice(&owner.to_le_bytes());
    stream.write_all(&reply)
}
