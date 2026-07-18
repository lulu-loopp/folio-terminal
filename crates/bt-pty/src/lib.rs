//! ConPTY ownership and the bounded PTY-to-Term transport from DESIGN.md §1.3.

use std::{
    collections::VecDeque,
    ffi::OsString,
    io::{Read, Write},
    num::{NonZeroU16, NonZeroUsize},
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    thread::JoinHandle,
};

use portable_pty::{
    Child, CommandBuilder, ExitStatus, MasterPty, PtySize as BackendSize, native_pty_system,
};
use thiserror::Error;

#[cfg(windows)]
pub use portable_pty::win::{CONPTY_SIDECAR_VERSION, ConPtySource};

#[cfg(not(windows))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConPtySource {
    NotWindows,
}

#[cfg(not(windows))]
impl std::fmt::Display for ConPtySource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("not-windows")
    }
}

/// Resolve the same process-wide ConPTY implementation used by subsequent PTY creation.
#[cfg(windows)]
pub fn conpty_source() -> ConPtySource {
    portable_pty::win::conpty_source()
}

#[cfg(not(windows))]
pub fn conpty_source() -> ConPtySource {
    ConPtySource::NotWindows
}

/// DESIGN.md §1.3: each session has exactly one MiB of buffered PTY output.
pub const PTY_RING_BYTES: NonZeroUsize = NonZeroUsize::new(1024 * 1024).unwrap();
/// Matches the serialized Term actor quantum from DESIGN.md §1.3.
pub const TERM_READ_QUANTUM: NonZeroUsize = NonZeroUsize::new(256 * 1024).unwrap();
const READER_CHUNK_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PtySize {
    pub columns: NonZeroU16,
    pub rows: NonZeroU16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl PtySize {
    pub fn cells(columns: NonZeroU16, rows: NonZeroU16) -> Self {
        Self {
            columns,
            rows,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    fn backend(self) -> BackendSize {
        BackendSize {
            rows: self.rows.get(),
            cols: self.columns.get(),
            pixel_width: self.pixel_width,
            pixel_height: self.pixel_height,
        }
    }

    fn from_backend(size: BackendSize) -> Result<Self, PtyError> {
        Ok(Self {
            columns: NonZeroU16::new(size.cols).ok_or(PtyError::ZeroBackendDimension)?,
            rows: NonZeroU16::new(size.rows).ok_or(PtyError::ZeroBackendDimension)?,
            pixel_width: size.pixel_width,
            pixel_height: size.pixel_height,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PtyCommand {
    pub program: OsString,
    pub arguments: Vec<OsString>,
    pub working_directory: Option<PathBuf>,
}

impl PtyCommand {
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            arguments: Vec::new(),
            working_directory: None,
        }
    }

    pub fn arg(mut self, argument: impl Into<OsString>) -> Self {
        self.arguments.push(argument.into());
        self
    }

    pub fn working_directory(mut self, directory: PathBuf) -> Self {
        self.working_directory = Some(directory);
        self
    }

    pub fn powershell() -> Self {
        Self::new("powershell.exe").arg("-NoLogo")
    }
}

#[derive(Debug, Error)]
pub enum PtyError {
    #[error("PTY backend error: {0}")]
    Backend(String),
    #[error("PTY I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("PTY output ring was closed")]
    RingClosed,
    #[error("PTY backend reported a zero cell dimension")]
    ZeroBackendDimension,
    #[error("PTY reader thread panicked")]
    ReaderPanicked,
}

fn backend(error: impl std::fmt::Display) -> PtyError {
    PtyError::Backend(error.to_string())
}

#[derive(Default)]
struct RingState {
    chunks: VecDeque<Vec<u8>>,
    bytes: usize,
    maximum_bytes: usize,
    blocked_pushes: u64,
    closed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RingStats {
    pub capacity: usize,
    pub current_bytes: usize,
    pub maximum_bytes: usize,
    pub blocked_pushes: u64,
    pub closed: bool,
}

/// Byte-counted bounded transport. The ConPTY reader blocks here instead of dropping VT bytes.
pub struct OutputRing {
    capacity: NonZeroUsize,
    state: Mutex<RingState>,
    changed: Condvar,
}

impl OutputRing {
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            capacity,
            state: Mutex::new(RingState::default()),
            changed: Condvar::new(),
        }
    }

    fn state(&self) -> MutexGuard<'_, RingState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn push(&self, chunk: Vec<u8>) -> Result<(), PtyError> {
        if chunk.len() > self.capacity.get() {
            return Err(PtyError::Backend(format!(
                "reader chunk {} exceeds ring capacity {}",
                chunk.len(),
                self.capacity
            )));
        }
        let mut state = self.state();
        let mut counted = false;
        while !state.closed && state.bytes + chunk.len() > self.capacity.get() {
            if !counted {
                state.blocked_pushes += 1;
                counted = true;
            }
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        if state.closed {
            return Err(PtyError::RingClosed);
        }
        state.bytes += chunk.len();
        state.maximum_bytes = state.maximum_bytes.max(state.bytes);
        state.chunks.push_back(chunk);
        self.changed.notify_all();
        Ok(())
    }

    pub fn try_pop(&self, quantum: NonZeroUsize) -> Vec<u8> {
        let mut state = self.state();
        let mut output = Vec::with_capacity(quantum.get().min(state.bytes));
        while output.len() < quantum.get() {
            let Some(mut chunk) = state.chunks.pop_front() else {
                break;
            };
            let remaining = quantum.get() - output.len();
            if chunk.len() <= remaining {
                state.bytes -= chunk.len();
                output.extend(chunk);
            } else {
                let tail = chunk.split_off(remaining);
                state.bytes -= chunk.len();
                output.extend(chunk);
                state.chunks.push_front(tail);
            }
        }
        self.changed.notify_all();
        output
    }

    pub fn close(&self) {
        let mut state = self.state();
        state.closed = true;
        self.changed.notify_all();
    }

    pub fn is_closed_and_drained(&self) -> bool {
        let state = self.state();
        state.closed && state.bytes == 0
    }

    pub fn stats(&self) -> RingStats {
        let state = self.state();
        RingStats {
            capacity: self.capacity.get(),
            current_bytes: state.bytes,
            maximum_bytes: state.maximum_bytes,
            blocked_pushes: state.blocked_pushes,
            closed: state.closed,
        }
    }
}

pub type OutputWake = Arc<dyn Fn() + Send + Sync + 'static>;

/// One shell process, one ConPTY, and one bounded reader thread.
pub struct PtySession {
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    output: Arc<OutputRing>,
    reader: Option<JoinHandle<()>>,
    conpty_source: ConPtySource,
}

impl PtySession {
    pub fn spawn_default(size: PtySize, wake: OutputWake) -> Result<Self, PtyError> {
        let command = PtyCommand::powershell()
            .working_directory(std::env::current_dir().map_err(PtyError::Io)?);
        Self::spawn(command, size, wake)
    }

    pub fn spawn(command: PtyCommand, size: PtySize, wake: OutputWake) -> Result<Self, PtyError> {
        let conpty_source = conpty_source();
        let pair = native_pty_system()
            .openpty(size.backend())
            .map_err(backend)?;
        let mut builder = CommandBuilder::new(command.program);
        for argument in command.arguments {
            builder.arg(argument);
        }
        if let Some(directory) = command.working_directory {
            builder.cwd(directory);
        }
        let child = pair.slave.spawn_command(builder).map_err(backend)?;
        drop(pair.slave);
        let writer = pair.master.take_writer().map_err(backend)?;
        let mut reader = pair.master.try_clone_reader().map_err(backend)?;
        let output = Arc::new(OutputRing::new(PTY_RING_BYTES));
        let reader_output = Arc::clone(&output);
        let reader_thread = std::thread::spawn(move || {
            let mut buffer = [0_u8; READER_CHUNK_BYTES];
            loop {
                let count = match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => count,
                };
                if reader_output.push(buffer[..count].to_vec()).is_err() {
                    break;
                }
                wake();
            }
            reader_output.close();
            wake();
        });
        Ok(Self {
            master: Some(pair.master),
            writer: Some(writer),
            child: Some(child),
            output,
            reader: Some(reader_thread),
            conpty_source,
        })
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<(), PtyError> {
        let writer = self.writer.as_mut().ok_or(PtyError::RingClosed)?;
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, size: PtySize) -> Result<(), PtyError> {
        self.master
            .as_ref()
            .ok_or(PtyError::RingClosed)?
            .resize(size.backend())
            .map_err(backend)
    }

    pub fn size(&self) -> Result<PtySize, PtyError> {
        PtySize::from_backend(
            self.master
                .as_ref()
                .ok_or(PtyError::RingClosed)?
                .get_size()
                .map_err(backend)?,
        )
    }

    pub fn read_output(&self) -> Vec<u8> {
        self.output.try_pop(TERM_READ_QUANTUM)
    }

    pub fn output_is_drained(&self) -> bool {
        self.output.is_closed_and_drained()
    }

    pub fn ring_stats(&self) -> RingStats {
        self.output.stats()
    }

    pub fn conpty_source(&self) -> &ConPtySource {
        &self.conpty_source
    }

    pub fn child_id(&self) -> Option<u32> {
        self.child.as_ref().and_then(|child| child.process_id())
    }

    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, PtyError> {
        let Some(child) = self.child.as_mut() else {
            return Ok(None);
        };
        let status = child.try_wait()?;
        if status.is_some() {
            self.child = None;
        }
        Ok(status)
    }

    pub fn shutdown(&mut self) -> Result<Option<ExitStatus>, PtyError> {
        self.writer.take();
        let status = if let Some(mut child) = self.child.take() {
            if let Some(status) = child.try_wait()? {
                Some(status)
            } else {
                child.kill()?;
                Some(child.wait()?)
            }
        } else {
            None
        };
        self.master.take();
        self.output.close();
        if let Some(reader) = self.reader.take() {
            reader.join().map_err(|_| PtyError::ReaderPanicked)?;
        }
        Ok(status)
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        num::NonZeroU32,
        sync::mpsc,
        time::{Duration, Instant},
    };

    use super::*;
    use bt_term::TerminalAdapter;

    const ORACLE_PROMPT: &str = "BT_PROMPT> ";
    const ORACLE_EMPTY_PROMPT_LINE: &str = "BT_PROMPT>";
    const ORACLE_HISTORY_COMMAND: &str = "echo BTHT";
    const ORACLE_HISTORY_OUTPUT: &[u8] = b"BTHT";

    #[derive(Debug)]
    struct CursorOracleEvidence {
        synchronization_dsr_requests: usize,
        synchronization_replies: String,
        synchronization_output: String,
        recalled_line: String,
        cleared_line: String,
        recalled_screen: Vec<String>,
        cleared_screen: Vec<String>,
    }

    struct InteractiveOracle {
        session: PtySession,
        terminal: TerminalAdapter,
        raw_output: Vec<u8>,
        pty_replies: Vec<u8>,
    }

    impl InteractiveOracle {
        fn spawn() -> Self {
            let startup = r#"Set-PSReadLineOption -HistorySaveStyle SaveNothing; function global:prompt { 'BT_PROMPT> ' }"#;
            let command = PtyCommand::new("powershell.exe")
                .arg("-NoLogo")
                .arg("-NoProfile")
                .arg("-NoExit")
                .arg("-Command")
                .arg(startup);
            let session = PtySession::spawn(command, size(52, 9), no_wake()).unwrap();
            let terminal = TerminalAdapter::new(nz32(52), nz32(9));
            Self {
                session,
                terminal,
                raw_output: Vec::new(),
                pty_replies: Vec::new(),
            }
        }

        fn pump_once(&mut self) -> bool {
            let bytes = self.session.read_output();
            let had_output = !bytes.is_empty();
            if had_output {
                self.raw_output.extend_from_slice(&bytes);
                self.terminal.feed(&bytes);
            }
            for reply in self.terminal.take_pty_writes() {
                self.pty_replies.extend_from_slice(&reply);
                self.session.write(&reply).unwrap();
            }
            had_output
        }

        fn pump_for(&mut self, duration: Duration) {
            let deadline = Instant::now() + duration;
            while Instant::now() < deadline {
                self.pump_once();
                std::thread::sleep(Duration::from_millis(2));
            }
            self.pump_once();
        }

        fn pump_until_quiet(&mut self, maximum: Duration) {
            let deadline = Instant::now() + maximum;
            let mut quiet_since = Instant::now();
            while Instant::now() < deadline {
                if self.pump_once() {
                    quiet_since = Instant::now();
                } else if quiet_since.elapsed() >= Duration::from_millis(100) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            panic!(
                "interactive ConPTY output did not become quiet; current line {:?}, screen {:?}",
                self.current_line(),
                self.terminal.visible_text()
            );
        }

        fn wait_for_current_line(&mut self, expected: &str) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                self.pump_once();
                if self.current_line() == expected {
                    return;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            panic!(
                "timed out waiting for current line {expected:?}; got {:?}, screen {:?}",
                self.current_line(),
                self.terminal.visible_text()
            );
        }

        fn wait_for_output_since(&mut self, start: usize, expected: &[u8]) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                self.pump_once();
                if self.raw_output[start..]
                    .windows(expected.len())
                    .any(|window| window == expected)
                {
                    return;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            panic!(
                "timed out waiting for output marker {:?}; current line {:?}, screen {:?}",
                String::from_utf8_lossy(expected),
                self.current_line(),
                self.terminal.visible_text()
            );
        }

        fn current_line(&self) -> String {
            let cursor = self.terminal.cursor();
            self.terminal
                .visible_text()
                .get(cursor.row as usize)
                .cloned()
                .unwrap_or_default()
        }

        fn current_prompt_text(&self) -> String {
            let cursor_row = self.terminal.cursor().row as usize;
            let rows = self.terminal.visible_text();
            let prompt_row = (0..=cursor_row)
                .rev()
                .find(|row| rows[*row].starts_with(ORACLE_EMPTY_PROMPT_LINE));
            prompt_row
                .map(|start| rows[start..=cursor_row].concat())
                .unwrap_or_default()
        }

        fn write_line(&mut self, command: &str) {
            self.session.write(command.as_bytes()).unwrap();
            self.session.write(b"\r").unwrap();
        }

        fn resize_terminal(&mut self, columns: u16, rows: u16) {
            self.terminal.resize(nz32(columns), nz32(rows));
        }

        fn resize_conpty(&mut self, columns: u16, rows: u16) {
            self.session.resize(size(columns, rows)).unwrap();
        }
    }

    fn size(columns: u16, rows: u16) -> PtySize {
        PtySize::cells(
            NonZeroU16::new(columns).unwrap(),
            NonZeroU16::new(rows).unwrap(),
        )
    }

    fn no_wake() -> OutputWake {
        Arc::new(|| {})
    }

    fn nz32(value: u16) -> NonZeroU32 {
        NonZeroU32::new(u32::from(value)).unwrap()
    }

    fn run_resize_cursor_oracle() -> CursorOracleEvidence {
        let mut oracle = InteractiveOracle::spawn();
        oracle.wait_for_current_line(ORACLE_EMPTY_PROMPT_LINE);
        let flood_start = oracle.raw_output.len();
        oracle.write_line(
            "1..80 | ForEach-Object { Write-Output ('BT_FILL_{0:D3}_XXXXXXXXXXXXXXXXXXXXXXXX' -f $_) }",
        );
        oracle.wait_for_output_since(flood_start, b"BT_FILL_080_XXXXXXXXXXXXXXXXXXXXXXXX");
        oracle.pump_until_quiet(Duration::from_secs(3));
        oracle.wait_for_current_line(ORACLE_EMPTY_PROMPT_LINE);
        let history_start = oracle.raw_output.len();
        oracle.write_line(ORACLE_HISTORY_COMMAND);
        oracle.wait_for_output_since(history_start, ORACLE_HISTORY_OUTPUT);
        oracle.pump_until_quiet(Duration::from_secs(3));
        oracle.wait_for_current_line(ORACLE_EMPTY_PROMPT_LINE);

        let synchronization_output_start = oracle.raw_output.len();
        let synchronization_reply_start = oracle.pty_replies.len();
        let resize_storm = [
            (31, 6),
            (83, 12),
            (37, 7),
            (76, 11),
            (29, 6),
            (91, 13),
            (43, 8),
            (68, 10),
            (34, 7),
            (88, 12),
            (40, 8),
            (73, 11),
            (22, 9),
        ];
        for (columns, rows) in resize_storm {
            oracle.resize_terminal(columns, rows);
            oracle.pump_for(Duration::from_millis(4));
        }
        let (final_columns, final_rows) = resize_storm[resize_storm.len() - 1];
        oracle.resize_conpty(final_columns, final_rows);
        oracle.pump_for(Duration::from_millis(500));
        oracle.pump_until_quiet(Duration::from_secs(3));
        oracle.wait_for_current_line(ORACLE_EMPTY_PROMPT_LINE);

        oracle.session.write(b"\x1b[A").unwrap();
        oracle.pump_for(Duration::from_millis(300));
        let recalled_line = oracle.current_prompt_text();
        let recalled_screen = oracle.terminal.visible_text();

        oracle.session.write(b"\x1b[B").unwrap();
        oracle.pump_for(Duration::from_millis(300));
        let cleared_line = oracle.current_prompt_text();
        let cleared_screen = oracle.terminal.visible_text();
        let synchronization_dsr_requests = oracle.raw_output[synchronization_output_start..]
            .windows(b"\x1b[6n".len())
            .filter(|window| *window == b"\x1b[6n")
            .count();
        let synchronization_replies =
            String::from_utf8_lossy(&oracle.pty_replies[synchronization_reply_start..])
                .escape_debug()
                .collect();
        let synchronization_output =
            String::from_utf8_lossy(&oracle.raw_output[synchronization_output_start..])
                .escape_debug()
                .collect();

        CursorOracleEvidence {
            synchronization_dsr_requests,
            synchronization_replies,
            synchronization_output,
            recalled_line,
            cleared_line,
            recalled_screen,
            cleared_screen,
        }
    }

    #[test]
    fn full_ring_blocks_writer_until_term_drains_bytes() {
        let ring = Arc::new(OutputRing::new(NonZeroUsize::new(4).unwrap()));
        ring.push(vec![1, 2, 3, 4]).unwrap();
        let producer_ring = Arc::clone(&ring);
        let (done_tx, done_rx) = mpsc::channel();
        let producer = std::thread::spawn(move || {
            done_tx.send(producer_ring.push(vec![5])).unwrap();
        });
        assert!(done_rx.recv_timeout(Duration::from_millis(20)).is_err());
        assert_eq!(
            ring.try_pop(NonZeroUsize::new(4).unwrap()),
            vec![1, 2, 3, 4]
        );
        assert!(
            done_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .is_ok()
        );
        producer.join().unwrap();
        assert_eq!(ring.stats().maximum_bytes, 4);
        assert_eq!(ring.stats().blocked_pushes, 1);
    }

    #[test]
    fn real_conpty_delivers_command_output_without_exceeding_ring() {
        let command = PtyCommand::new("cmd.exe")
            .arg("/D")
            .arg("/C")
            .arg("echo BT_PTY_OK");
        let mut session = PtySession::spawn(command, size(40, 8), no_wake()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        let mut answered_cursor_query = false;
        let mut child_exited = false;
        while std::time::Instant::now() < deadline {
            output.extend(session.read_output());
            if !answered_cursor_query && output.windows(4).any(|bytes| bytes == b"\x1b[6n") {
                session.write(b"\x1b[1;1R").unwrap();
                answered_cursor_query = true;
            }
            child_exited |= session.try_wait().unwrap().is_some();
            if child_exited && session.output_is_drained() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        output.extend(session.read_output());
        session.shutdown().unwrap();
        assert!(
            String::from_utf8_lossy(&output).contains("BT_PTY_OK"),
            "ConPTY output was {:?}",
            String::from_utf8_lossy(&output)
        );
        assert!(session.ring_stats().maximum_bytes <= PTY_RING_BYTES.get());
    }

    #[test]
    fn resize_reaches_the_real_conpty_and_shutdown_reaps_child() {
        let command = PtyCommand::new("powershell.exe")
            .arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-Command")
            .arg("Start-Sleep -Seconds 30");
        let mut session = PtySession::spawn(command, size(40, 8), no_wake()).unwrap();
        session.resize(size(96, 31)).unwrap();
        let actual = session.size().unwrap();
        assert_eq!((actual.columns.get(), actual.rows.get()), (96, 31));
        assert!(session.child_id().is_some());
        assert!(session.shutdown().unwrap().is_some());
        assert!(session.child_id().is_none());
    }

    #[test]
    fn sidecar_resize_keeps_history_navigation_on_a_clean_prompt_line() {
        let source = conpty_source();
        assert_eq!(CONPTY_SIDECAR_VERSION, "1.25.260710002-preview");
        assert!(
            matches!(source, ConPtySource::Sidecar { .. }),
            "test executable must have the pinned ConPTY sidecar beside it; selected {}",
            source
        );
        let evidence = run_resize_cursor_oracle();
        eprintln!("BT_CONPTY_ORACLE {source} evidence={evidence:?}");
        assert_eq!(
            evidence.synchronization_dsr_requests, 1,
            "pinned ConPTY preview must request one cursor synchronization after the committed resize: {evidence:?}"
        );
        assert!(
            evidence.synchronization_replies.contains('R'),
            "terminal did not answer ConPTY's DSR with a CPR: {evidence:?}"
        );
        assert_eq!(
            evidence.recalled_line,
            format!("{ORACLE_PROMPT}{ORACLE_HISTORY_COMMAND}"),
            "CSI A mixed history text into the wrong prompt row; screen={:?}, output={:?}: {evidence:?}",
            evidence.recalled_screen,
            evidence.synchronization_output
        );
        assert_eq!(
            evidence.cleared_line, ORACLE_EMPTY_PROMPT_LINE,
            "CSI B did not restore one clean empty prompt row; screen={:?}: {evidence:?}",
            evidence.cleared_screen
        );
    }

    #[test]
    #[ignore = "known system ConPTY cursor desync: https://github.com/microsoft/terminal/issues/18725"]
    fn system_conpty_known_resize_cursor_desync_oracle() {
        assert!(
            std::env::var_os("BT_CONPTY_FORCE_SYSTEM").is_some(),
            "run in a fresh process with BT_CONPTY_FORCE_SYSTEM=1"
        );
        assert_eq!(conpty_source(), ConPtySource::System);
        let evidence = run_resize_cursor_oracle();
        eprintln!("BT_CONPTY_ORACLE {} evidence={evidence:?}", conpty_source());
        assert!(
            evidence.synchronization_dsr_requests > 0,
            "known upstream system-ConPTY failure: no post-resize DSR/CPR synchronization; https://github.com/microsoft/terminal/issues/18725; {evidence:?}"
        );
        assert_eq!(
            evidence.recalled_line,
            format!("{ORACLE_PROMPT}{ORACLE_HISTORY_COMMAND}"),
            "known upstream system-ConPTY failure: https://github.com/microsoft/terminal/issues/18725; {evidence:?}"
        );
        assert_eq!(
            evidence.cleared_line, ORACLE_EMPTY_PROMPT_LINE,
            "known upstream system-ConPTY failure: https://github.com/microsoft/terminal/issues/18725; {evidence:?}"
        );
    }
}
